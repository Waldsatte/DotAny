//! What's inside an archive, and reading it back out: ZIP, 7z, TAR (bare, or in gzip, bzip2, xz or zstd), a single file
//! in one of those four, RAR, LHA, cpio, ar (.deb is one) and RPM. Every entry keeps its index in the archive; the page
//! edits a list of paths that point back at these indexes, and writing or extracting walks the archive once, in its own
//! order, handing each wanted entry's bytes over as a stream. Only ZIP, 7z and TAR are ever written.

use crate::codec::{Codec, read_full};
use crate::libarchive::{self, LaReader};
use serde::Serialize;
use std::fs::File;
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    Zip,
    SevenZ,
    /// a TAR, bare or compressed
    Tar(Option<Codec>),
    /// one file, compressed: shown as an archive holding just it
    Single(Codec),
    Rar,
    Lha,
    /// a cpio, bare or compressed (an initramfs)
    Cpio(Option<Codec>),
    Ar,
    /// an ar whose first member is debian-binary
    Deb,
    /// an RPM: its headers, then a compressed cpio
    Rpm,
}

impl Kind {
    /// The format as the page names it (and shows it, upper-cased, as Format).
    pub fn name(self) -> &'static str {
        match self {
            Kind::Zip => "zip",
            Kind::SevenZ => "7z",
            Kind::Tar(None) => "tar",
            Kind::Tar(Some(Codec::Gzip)) => "tar.gz",
            Kind::Tar(Some(Codec::Bzip2)) => "tar.bz2",
            Kind::Tar(Some(Codec::Xz)) => "tar.xz",
            Kind::Tar(Some(Codec::Zstd)) => "tar.zst",
            Kind::Single(c) => c.suffix(),
            Kind::Rar => "rar",
            Kind::Lha => "lzh",
            Kind::Cpio(None) => "cpio",
            Kind::Cpio(Some(Codec::Gzip)) => "cpio.gz",
            Kind::Cpio(Some(Codec::Bzip2)) => "cpio.bz2",
            Kind::Cpio(Some(Codec::Xz)) => "cpio.xz",
            Kind::Cpio(Some(Codec::Zstd)) => "cpio.zst",
            Kind::Ar => "ar",
            Kind::Deb => "deb",
            Kind::Rpm => "rpm",
        }
    }

    /// Read through libarchive, as which of its formats.
    fn libarchive(self) -> Option<libarchive::Format> {
        match self {
            Kind::Rar => Some(libarchive::Format::Rar),
            Kind::Lha => Some(libarchive::Format::Lha),
            Kind::Cpio(_) | Kind::Rpm => Some(libarchive::Format::Cpio),
            Kind::Ar | Kind::Deb => Some(libarchive::Format::Ar),
            _ => None,
        }
    }
}

/// One entry as the page sees it. Names whose encoding is on record come as text; the rest as their raw bytes, which
/// the page decodes with the encoding it guesses (or the one picked).
#[derive(Serialize, Clone)]
pub struct Entry {
    pub i: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub n: Option<String>,
    /// raw name bytes, base64, when the archive doesn't say how they're encoded
    #[serde(skip_serializing_if = "Option::is_none")]
    pub r: Option<String>,
    pub d: bool,
    pub s: u64,
    pub c: u64,
    /// a DOS time is already local: "YYYY-MM-DD HH:MM"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub t: Option<String>,
    /// everything else: seconds since 1970, for the page to show in local time
    #[serde(skip_serializing_if = "Option::is_none")]
    pub u: Option<i64>,
    pub l: bool,
}

pub struct Source {
    path: PathBuf,
    pub kind: Kind,
    pub entries: Vec<Entry>,
    /// a 7z whose names are encrypted too was opened with this
    header_password: Option<String>,
    /// a bare TAR: where each entry's data starts, so one can be read without the rest
    tar_offsets: Vec<u64>,
    zip: Option<zip::ZipArchive<BufReader<File>>>,
}

#[derive(Debug)]
pub enum OpenError {
    Damaged,
    /// a 7z with encrypted names: nothing can be listed without the password
    Locked,
    WrongPassword,
}

/// Why one entry couldn't be read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryError {
    /// encrypted, and no password or the wrong one
    Password,
    /// encrypted in a way this program doesn't read (RAR's encryption)
    Unsupported,
    Damaged,
}

/// A 7z entry's bytes, held to the size and CRC the archive records: a wrong password can make the decoder stop early
/// without complaint, and a short entry must never pass for a whole one.
struct Verified<'a> {
    inner: &'a mut dyn Read,
    left: u64,
    crc: flate2::Crc,
    want: Option<u32>,
}

impl Read for Verified<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let n = self.inner.read(buf)?;
        if n == 0 {
            if self.left != 0 {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "entry ends early: damaged, or the wrong password",
                ));
            }
            if self.want.is_some_and(|w| w != self.crc.sum()) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "checksum mismatch: damaged, or the wrong password",
                ));
            }
            return Ok(0);
        }
        if n as u64 > self.left {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "entry longer than recorded"));
        }
        self.left -= n as u64;
        self.crc.update(&buf[..n]);
        Ok(n)
    }
}

pub fn sniff_kind(path: &Path) -> Option<Kind> {
    let mut head = [0u8; 512];
    let mut f = File::open(path).ok()?;
    let mut n = 0;
    while n < head.len() {
        match f.read(&mut head[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(_) => return None,
        }
    }
    let h = &head[..n];
    if h.starts_with(b"PK\x03\x04") || h.starts_with(b"PK\x05\x06") || h.starts_with(b"PK\x07\x08") {
        return Some(Kind::Zip);
    }
    if h.starts_with(b"7z\xBC\xAF\x27\x1C") {
        return Some(Kind::SevenZ);
    }
    if h.starts_with(b"Rar!\x1A\x07") {
        return Some(Kind::Rar);
    }
    if let Some(c) = Codec::of(h) {
        return Some(inside(path, c));
    }
    if h.starts_with(b"!<arch>\n") {
        // a .deb is an ar whose first member is debian-binary ("debian-binary   ", or GNU's "debian-binary/  ")
        return Some(if h.len() >= 21 && &h[8..21] == b"debian-binary" { Kind::Deb } else { Kind::Ar });
    }
    if h.starts_with(&[0xED, 0xAB, 0xEE, 0xDB]) {
        return Some(Kind::Rpm);
    }
    if cpio_magic(h) {
        return Some(Kind::Cpio(None));
    }
    if n >= 512 && tar_header_ok(h) {
        return Some(Kind::Tar(None));
    }
    if lha_header(h) {
        return Some(Kind::Lha);
    }
    None
}

/// What a compressed file holds, by its first decompressed bytes: a TAR, a cpio, or one plain file. A stream that won't
/// decompress even that far is taken as one file, and opening it says it's damaged.
fn inside(path: &Path, c: Codec) -> Kind {
    let mut head = [0u8; 512];
    let n = File::open(path).ok().and_then(|f| read_full(&mut *c.reader(Box::new(f)), &mut head).ok()).unwrap_or(0);
    let h = &head[..n];
    if n >= 512 && tar_header_ok(h) {
        Kind::Tar(Some(c))
    } else if cpio_magic(h) {
        Kind::Cpio(Some(c))
    } else {
        Kind::Single(c)
    }
}

/// cpio's portable ("070707"), new ("070701") and new-with-CRC ("070702") headers, and the old binary one either way round.
fn cpio_magic(h: &[u8]) -> bool {
    h.starts_with(b"070707")
        || h.starts_with(b"070701")
        || h.starts_with(b"070702")
        || h.starts_with(&[0xC7, 0x71])
        || h.starts_with(&[0x71, 0xC7])
}

/// An LHA header as libarchive bids on it: a method "-lh0-" to "-lh7-", "-lhd-", "-lzs-", "-lz4-" or "-lz5-" at byte 2,
/// and a header level of 0 to 3 at byte 20.
fn lha_header(h: &[u8]) -> bool {
    h.len() >= 22
        && h[2] == b'-'
        && h[3] == b'l'
        && h[6] == b'-'
        && (h[4] == b'h' && matches!(h[5], b'0'..=b'7' | b'd') || h[4] == b'z' && matches!(h[5], b's' | b'4' | b'5'))
        && h[20] <= 3
}

/// The name of the one file in a compressed file: the archive's own, without the compression's suffix (a .tgz holds a
/// .tar).
fn single_name(path: &Path) -> String {
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let lower = name.to_lowercase();
    for (suffix, becomes) in [
        (".tgz", ".tar"),
        (".tbz2", ".tar"),
        (".tbz", ".tar"),
        (".txz", ".tar"),
        (".tzst", ".tar"),
        (".gz", ""),
        (".bz2", ""),
        (".xz", ""),
        (".zst", ""),
    ] {
        if lower.ends_with(suffix) && name.len() > suffix.len() {
            return format!("{}{becomes}", &name[..name.len() - suffix.len()]);
        }
    }
    name
}

/// An RPM's payload: past its lead and two headers, decompressed. What's inside is a cpio.
fn rpm_payload(f: File) -> io::Result<Box<dyn Read + Send>> {
    let mut r = BufReader::with_capacity(1 << 16, f);
    let mut lead = [0u8; 96];
    r.read_exact(&mut lead)?;
    if lead[..4] != [0xED, 0xAB, 0xEE, 0xDB] {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "not an RPM"));
    }
    // the signature header, padded to 8 bytes, then the header proper
    for padded in [true, false] {
        let mut h = [0u8; 16];
        r.read_exact(&mut h)?;
        if h[..4] != [0x8E, 0xAD, 0xE8, 0x01] {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "RPM header"));
        }
        let entries = u32::from_be_bytes([h[8], h[9], h[10], h[11]]) as u64;
        let data = u32::from_be_bytes([h[12], h[13], h[14], h[15]]) as u64;
        let mut skip = entries * 16 + data;
        if padded {
            skip += (8 - (16 + skip) % 8) % 8;
        }
        if io::copy(&mut (&mut r).take(skip), &mut io::sink())? != skip {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "RPM header cut short"));
        }
    }
    let (codec, cpio) = {
        let head = r.fill_buf()?;
        (Codec::of(head), cpio_magic(head))
    };
    match codec {
        Some(c) => Ok(c.reader(Box::new(r))),
        None if cpio => Ok(Box::new(r)),
        // LZMA-alone payloads (older RPMs) and anything else: not read
        None => Err(io::Error::new(io::ErrorKind::InvalidData, "RPM payload compression")),
    }
}

fn tar_header_ok(h: &[u8]) -> bool {
    if &h[257..262] == b"ustar" {
        return true;
    }
    let stored = std::str::from_utf8(&h[148..156])
        .ok()
        .map(|s| s.trim_matches(|c: char| c == '\0' || c == ' '))
        .and_then(|s| u32::from_str_radix(s, 8).ok());
    let sum: u32 = h[..512].iter().enumerate().map(|(i, &b)| if (148..156).contains(&i) { 32 } else { b as u32 }).sum();
    stored == Some(sum) && h[0] != 0
}

fn b64(raw: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(raw.len().div_ceil(3) * 4);
    for c in raw.chunks(3) {
        let v = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for k in 0..4 {
            out.push(if k <= c.len() { T[(v >> (18 - 6 * k) & 63) as usize] as char } else { '=' });
        }
    }
    out
}

/// A raw name that is plain ASCII, or that the archive marks as UTF-8, is text; anything else waits for the page's guess.
fn name_of(raw: &[u8], utf8: bool) -> (Option<String>, Option<String>) {
    if utf8 || raw.is_ascii() {
        (Some(String::from_utf8_lossy(raw).into_owned()), None)
    } else {
        (None, Some(b64(raw)))
    }
}

fn zip_time(t: zip::DateTime) -> String {
    format!("{:04}-{:02}-{:02} {:02}:{:02}", t.year(), t.month(), t.day(), t.hour(), t.minute())
}

fn nt_to_unix(t: sevenz_rust2::NtTime) -> Option<i64> {
    let st: std::time::SystemTime = t.into();
    st.duration_since(std::time::UNIX_EPOCH).ok().map(|d| d.as_secs() as i64)
}

fn is_aes(block: &sevenz_rust2::Block) -> bool {
    block.coders.iter().any(|c| c.encoder_method_id() == sevenz_rust2::EncoderMethod::ID_AES256_SHA256)
}

impl Source {
    /// The archive file this reads.
    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn open(path: &Path, password: Option<&str>) -> Result<Source, OpenError> {
        let kind = sniff_kind(path).ok_or(OpenError::Damaged)?;
        let mut src = Source {
            path: path.to_path_buf(),
            kind,
            entries: Vec::new(),
            header_password: None,
            tar_offsets: Vec::new(),
            zip: None,
        };
        match kind {
            Kind::Zip => src.list_zip()?,
            Kind::SevenZ => src.list_7z(password)?,
            Kind::Tar(_) => src.list_tar()?,
            Kind::Single(_) => src.list_single()?,
            Kind::Rar | Kind::Lha | Kind::Cpio(_) | Kind::Ar | Kind::Deb | Kind::Rpm => src.list_libarchive()?,
        }
        Ok(src)
    }

    fn list_zip(&mut self) -> Result<(), OpenError> {
        let f = File::open(&self.path).map_err(|_| OpenError::Damaged)?;
        let z = zip::ZipArchive::new(BufReader::with_capacity(1 << 16, f)).map_err(|_| OpenError::Damaged)?;
        for i in 0..z.len() {
            let Ok(e) = z.by_index_data(i) else { continue };
            // bit 11: the name is UTF-8 (the zip crate also sets it when a Unicode path field was found)
            let utf8 = e.flags().as_u16() & 0x0800 != 0;
            let (n, r) = name_of(e.name_raw(), utf8);
            self.entries.push(Entry {
                i,
                n,
                r,
                d: e.is_dir(),
                s: e.size(),
                c: e.compressed_size(),
                t: e.last_modified().map(zip_time),
                u: None,
                l: e.encrypted(),
            });
        }
        self.zip = Some(z);
        Ok(())
    }

    fn list_7z(&mut self, password: Option<&str>) -> Result<(), OpenError> {
        let mut f = BufReader::new(File::open(&self.path).map_err(|_| OpenError::Damaged)?);
        let pw = password.map(sevenz_rust2::Password::from).unwrap_or_else(sevenz_rust2::Password::empty);
        let archive = match sevenz_rust2::Archive::read(&mut f, &pw) {
            Ok(a) => a,
            Err(sevenz_rust2::Error::PasswordRequired) => return Err(OpenError::Locked),
            Err(sevenz_rust2::Error::MaybeBadPassword(_)) if password.is_some() => {
                return Err(OpenError::WrongPassword);
            }
            Err(_) if password.is_some() => return Err(OpenError::WrongPassword),
            Err(_) => return Err(OpenError::Damaged),
        };
        if password.is_some() {
            self.header_password = password.map(str::to_string);
        }
        for (i, e) in archive.files.iter().enumerate() {
            let block = archive.stream_map.file_block_index.get(i).copied().flatten();
            let locked = block.is_some_and(|b| is_aes(&archive.blocks[b]));
            self.entries.push(Entry {
                i,
                n: Some(e.name.replace('\\', "/")),
                r: None,
                d: e.is_directory,
                s: e.size,
                c: e.compressed_size,
                t: None,
                u: e.has_last_modified_date.then(|| nt_to_unix(e.last_modified_date)).flatten(),
                l: locked,
            });
        }
        // 7z keeps packed sizes per block, not per file: share the whole among the files by their sizes
        let packed: u64 = archive.pack_sizes().iter().sum();
        let total: u64 = self.entries.iter().map(|e| e.s).sum();
        for e in &mut self.entries {
            e.c = if total > 0 { ((packed as u128 * e.s as u128) / total as u128) as u64 } else { 0 };
        }
        Ok(())
    }

    /// The file as a stream, decompressed when it is compressed: a TAR, a cpio or the one file of a single-file archive.
    fn stream(&self) -> io::Result<Box<dyn Read + Send>> {
        let f = File::open(&self.path)?;
        Ok(match self.kind {
            Kind::Tar(Some(c)) | Kind::Single(c) | Kind::Cpio(Some(c)) => c.reader(Box::new(f)),
            _ => Box::new(BufReader::with_capacity(1 << 16, f)),
        })
    }

    fn libarchive_reader(&self) -> io::Result<LaReader> {
        let format = self.kind.libarchive().ok_or_else(|| io::Error::other("not read by libarchive"))?;
        match self.kind {
            Kind::Cpio(Some(_)) => LaReader::stream(self.stream()?, format),
            Kind::Rpm => LaReader::stream(rpm_payload(File::open(&self.path)?)?, format),
            _ => LaReader::open(&self.path, format),
        }
    }

    /// Shares what the file takes on disk out among the entries by their sizes, for formats that don't record it per entry.
    fn share_packed_size(&mut self) {
        let total: u64 = self.entries.iter().map(|e| e.s).sum();
        let file = std::fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0);
        for e in &mut self.entries {
            e.c = if total > 0 { ((file as u128 * e.s as u128) / total as u128) as u64 } else { 0 };
        }
    }

    fn list_tar(&mut self) -> Result<(), OpenError> {
        let mut ar = tar::Archive::new(self.stream().map_err(|_| OpenError::Damaged)?);
        let entries = ar.entries().map_err(|_| OpenError::Damaged)?;
        let mut i = 0;
        for e in entries {
            let e = e.map_err(|_| OpenError::Damaged)?;
            let kind = e.header().entry_type();
            let dir = kind.is_dir();
            // links and devices are listed nowhere and never extracted
            if !dir && !kind.is_file() && kind != tar::EntryType::Continuous {
                i += 1;
                self.tar_offsets.push(e.raw_file_position());
                continue;
            }
            let raw = e.path_bytes().into_owned();
            let (n, r) = name_of(&raw, false);
            let size = e.size();
            self.tar_offsets.push(e.raw_file_position());
            self.entries.push(Entry {
                i,
                n,
                r,
                d: dir,
                s: if dir { 0 } else { size },
                c: if dir { 0 } else { size },
                t: None,
                u: e.header().mtime().ok().map(|m| m as i64),
                l: false,
            });
            i += 1;
        }
        if self.kind != Kind::Tar(None) {
            // what each entry takes in the file, in proportion: a compressed TAR doesn't record it per entry
            self.share_packed_size();
        }
        Ok(())
    }

    /// The one file: read to its end to learn its size (bzip2 doesn't record it, gzip's record wraps at 4 GB), which
    /// also proves the stream whole before anything is shown.
    fn list_single(&mut self) -> Result<(), OpenError> {
        let size = io::copy(&mut self.stream().map_err(|_| OpenError::Damaged)?, &mut io::sink())
            .map_err(|_| OpenError::Damaged)?;
        let meta = std::fs::metadata(&self.path).ok();
        // gzip records the file's own time (0 when it doesn't); otherwise the compressed file's time stands for it
        let mut head = [0u8; 8];
        let gz_time = (self.kind == Kind::Single(Codec::Gzip))
            .then(|| {
                File::open(&self.path)
                    .ok()
                    .and_then(|mut f| f.read_exact(&mut head).ok())
                    .map(|_| u32::from_le_bytes([head[4], head[5], head[6], head[7]]) as i64)
            })
            .flatten()
            .filter(|&t| t > 0);
        let file_time = meta
            .as_ref()
            .and_then(|m| m.modified().ok())
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_secs() as i64);
        let name = single_name(&self.path);
        self.entries.push(Entry {
            i: 0,
            n: Some(name),
            r: None,
            d: false,
            s: size,
            c: meta.map(|m| m.len()).unwrap_or(0),
            t: None,
            u: gz_time.or(file_time),
            l: false,
        });
        Ok(())
    }

    /// RAR, LHA, cpio, ar and RPM, through libarchive.
    fn list_libarchive(&mut self) -> Result<(), OpenError> {
        let mut r = self.libarchive_reader().map_err(|_| OpenError::Damaged)?;
        let mut i = 0;
        loop {
            match r.next() {
                Ok(Some(e)) => {
                    if !e.special {
                        let size = if e.dir { 0 } else { e.size };
                        let (n, raw) = match (e.name, e.raw) {
                            (_, Some(raw)) => name_of(&raw, false),
                            (n, None) => (Some(n.unwrap_or_default()), None),
                        };
                        self.entries.push(Entry {
                            i,
                            n,
                            r: raw,
                            d: e.dir,
                            s: size,
                            c: size,
                            t: None,
                            u: e.mtime,
                            l: e.locked,
                        });
                    }
                    i += 1;
                }
                Ok(None) => break,
                // an archive whose listing breaks off part-way still shows what it had
                Err(_) if !self.entries.is_empty() => break,
                Err(_) => return Err(OpenError::Damaged),
            }
        }
        // packed sizes aren't given per entry by libarchive: share the file's size out
        self.share_packed_size();
        Ok(())
    }

    /// Walks the archive once, in its own order, handing `each` every wanted entry's bytes (or why they can't be read).
    /// `each` answers false to stop. 7z and anything compressed can only be read from the start; ZIP, a bare TAR and the
    /// libarchive formats skip what's unwanted.
    pub fn walk(
        &mut self,
        password: Option<&str>,
        wanted: &dyn Fn(usize) -> bool,
        each: &mut dyn FnMut(usize, Result<&mut dyn Read, EntryError>) -> io::Result<bool>,
    ) -> io::Result<()> {
        match self.kind {
            Kind::Zip => {
                let indexes: Vec<usize> = self.entries.iter().map(|e| e.i).filter(|&i| wanted(i)).collect();
                let z = self.zip.as_mut().ok_or_else(|| io::Error::other("closed"))?;
                for i in indexes {
                    let locked = z.by_index_data(i).map(|e| e.encrypted()).unwrap_or(false);
                    let go = match (locked, password) {
                        (true, None) => each(i, Err(EntryError::Password))?,
                        (true, Some(pw)) => match z.by_index_decrypt(i, pw.as_bytes()) {
                            Ok(mut f) => each(i, Ok(&mut f))?,
                            Err(zip::result::ZipError::InvalidPassword) => each(i, Err(EntryError::Password))?,
                            Err(zip::result::ZipError::UnsupportedArchive(_)) => each(i, Err(EntryError::Unsupported))?,
                            Err(_) => each(i, Err(EntryError::Damaged))?,
                        },
                        (false, _) => match z.by_index(i) {
                            Ok(mut f) => each(i, Ok(&mut f))?,
                            Err(zip::result::ZipError::UnsupportedArchive(_)) => each(i, Err(EntryError::Unsupported))?,
                            Err(_) => each(i, Err(EntryError::Damaged))?,
                        },
                    };
                    if !go {
                        break;
                    }
                }
                Ok(())
            }
            Kind::SevenZ => {
                let pw_text = self.header_password.as_deref().or(password);
                let pw = pw_text.map(sevenz_rust2::Password::from).unwrap_or_else(sevenz_rust2::Password::empty);
                let mut f = BufReader::new(File::open(&self.path)?);
                let archive = sevenz_rust2::Archive::read(&mut f, &pw)
                    .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e.to_string()))?;
                let base = archive.files.as_ptr() as usize;
                let size = std::mem::size_of::<sevenz_rust2::ArchiveEntry>();
                // a locked block without a password can't be decoded: say so once for each of its files
                let locked_blocks: Vec<bool> = archive.blocks.iter().map(is_aes).collect();
                let file_block: Vec<Option<usize>> = archive.stream_map.file_block_index.clone();
                if pw_text.is_none() {
                    for (i, b) in file_block.iter().enumerate() {
                        if b.is_some_and(|b| locked_blocks[b]) && wanted(i) && !each(i, Err(EntryError::Password))? {
                            return Ok(());
                        }
                    }
                }
                let mut reader = sevenz_rust2::ArchiveReader::from_archive(archive, f, pw);
                let mut stop = false;
                let mut failed: Option<io::Error> = None;
                let mut seen = vec![false; file_block.len()];
                let run = reader.for_each_entries(|entry, data| {
                    let i = (entry as *const _ as usize - base) / size;
                    if stop
                        || !wanted(i)
                        || file_block.get(i).copied().flatten().is_some_and(|b| locked_blocks[b] && pw_text.is_none())
                    {
                        return Ok(!stop);
                    }
                    seen[i] = true;
                    let mut checked = Verified {
                        inner: data,
                        left: entry.size,
                        crc: flate2::Crc::new(),
                        want: entry.has_crc.then_some(entry.crc as u32),
                    };
                    match each(i, Ok(&mut checked)) {
                        Ok(true) => Ok(true),
                        Ok(false) => {
                            stop = true;
                            Ok(false)
                        }
                        Err(e) => {
                            failed = Some(e);
                            stop = true;
                            Ok(false)
                        }
                    }
                });
                if let Some(e) = failed {
                    return Err(e);
                }
                if let Err(e) = run {
                    if stop {
                        return Ok(());
                    }
                    // the block broke: everything wanted that wasn't handed over is damaged, or the password was wrong
                    let why = if matches!(e, sevenz_rust2::Error::MaybeBadPassword(_))
                        || pw_text.is_some() && matches!(e, sevenz_rust2::Error::ChecksumVerificationFailed)
                    {
                        EntryError::Password
                    } else {
                        EntryError::Damaged
                    };
                    for i in 0..file_block.len() {
                        if wanted(i) && !seen[i] && file_block[i].is_some() && !each(i, Err(why))? {
                            break;
                        }
                    }
                }
                Ok(())
            }
            Kind::Tar(None) => {
                let indexes: Vec<usize> = self.entries.iter().map(|e| e.i).filter(|&i| wanted(i)).collect();
                let mut f = File::open(&self.path)?;
                for i in indexes {
                    let entry = self.entries.iter().find(|e| e.i == i).cloned();
                    let Some(entry) = entry else { continue };
                    f.seek(SeekFrom::Start(self.tar_offsets[i]))?;
                    let mut part = (&mut f).take(entry.s);
                    if !each(i, Ok(&mut part))? {
                        break;
                    }
                }
                Ok(())
            }
            Kind::Tar(Some(_)) => {
                let mut ar = tar::Archive::new(self.stream()?);
                for (i, e) in ar.entries()?.enumerate() {
                    let mut e = match e {
                        Ok(e) => e,
                        Err(err) => return Err(io::Error::new(io::ErrorKind::InvalidData, err)),
                    };
                    if !wanted(i) {
                        continue;
                    }
                    if !each(i, Ok(&mut e))? {
                        break;
                    }
                }
                Ok(())
            }
            Kind::Single(_) => {
                if wanted(0) {
                    let mut r = self.stream()?;
                    each(0, Ok(&mut r))?;
                }
                Ok(())
            }
            Kind::Rar | Kind::Lha | Kind::Cpio(_) | Kind::Ar | Kind::Deb | Kind::Rpm => {
                let mut r = self.libarchive_reader()?;
                let mut i = 0;
                while let Some(e) = r.next()? {
                    if wanted(i) {
                        let go = if e.locked { each(i, Err(EntryError::Unsupported))? } else { each(i, Ok(&mut r))? };
                        if !go {
                            break;
                        }
                    }
                    i += 1;
                }
                Ok(())
            }
        }
    }

    /// The Unix permissions entries were packed with, by index: zip made on a Mac or Linux, 7z made by p7zip, any TAR.
    /// What keeps a program runnable when it is extracted, and says which zip and 7z entries are symbolic links
    /// (`S_IFLNK`, their data being the link's text). Only the Mac and other Unix systems have use for it; formats that
    /// don't record it, and entries made on Windows, have none.
    #[cfg(unix)]
    pub fn unix_modes(&self) -> std::collections::HashMap<usize, u32> {
        let mut out = std::collections::HashMap::new();
        match self.kind {
            Kind::Zip => {
                let Ok(f) = File::open(&self.path) else { return out };
                let Ok(mut z) = zip::ZipArchive::new(BufReader::with_capacity(1 << 16, f)) else { return out };
                for i in 0..z.len() {
                    if let Some(m) = z.by_index_raw(i).ok().and_then(|e| e.unix_mode()) {
                        out.insert(i, m);
                    }
                }
            }
            Kind::SevenZ => {
                let Ok(f) = File::open(&self.path) else { return out };
                let pw = self
                    .header_password
                    .as_deref()
                    .map(sevenz_rust2::Password::from)
                    .unwrap_or_else(sevenz_rust2::Password::empty);
                let Ok(archive) = sevenz_rust2::Archive::read(&mut BufReader::new(f), &pw) else { return out };
                // FILE_ATTRIBUTE_UNIX_EXTENSION: the mode is in the high 16 bits, as p7zip and 7-Zip write it
                for (i, e) in archive.files.iter().enumerate() {
                    if e.has_windows_attributes && e.windows_attributes & 0x8000 != 0 {
                        out.insert(i, e.windows_attributes >> 16);
                    }
                }
            }
            Kind::Tar(_) => {
                let Ok(s) = self.stream() else { return out };
                let mut ar = tar::Archive::new(s);
                let Ok(entries) = ar.entries() else { return out };
                for (i, e) in entries.enumerate() {
                    let Ok(e) = e else { break };
                    // permissions only: a TAR says what is a link by the entry's type, and its links aren't listed
                    if let Ok(m) = e.header().mode() {
                        out.insert(i, m & 0o7777);
                    }
                }
            }
            _ => {}
        }
        out
    }

    /// One entry copied into `to` as it is read, a small buffer at a time: an entry of any size (a 1 GB film in a zip)
    /// never sits in memory whole. Answers how many bytes went.
    pub fn copy_one(&mut self, i: usize, password: Option<&str>, to: &mut dyn Write) -> Result<u64, EntryError> {
        let mut out: Option<Result<u64, EntryError>> = None;
        let _ = self.walk(password, &|k| k == i, &mut |_, r| {
            out = Some(match r {
                Ok(r) => match io::copy(r, to) {
                    Ok(n) => Ok(n),
                    Err(e)
                        if e.kind() == io::ErrorKind::InvalidInput
                            || e.to_string().to_lowercase().contains("password") =>
                    {
                        Err(EntryError::Password)
                    }
                    Err(_) => Err(EntryError::Damaged),
                },
                Err(e) => Err(e),
            });
            Ok(false)
        });
        out.unwrap_or(Err(EntryError::Damaged))
    }

    /// One entry's first `limit` bytes, for the preview: `limit` is the page's, never the entry's declared size.
    pub fn read_one(&mut self, i: usize, password: Option<&str>, limit: u64) -> Result<Vec<u8>, EntryError> {
        let mut out: Option<Result<Vec<u8>, EntryError>> = None;
        let walked = self.walk(password, &|k| k == i, &mut |_, r| {
            out = Some(match r {
                Ok(r) => {
                    let mut v = Vec::new();
                    match r.take(limit).read_to_end(&mut v) {
                        Ok(_) => Ok(v),
                        Err(e)
                            if e.kind() == io::ErrorKind::InvalidInput
                                || e.to_string().to_lowercase().contains("password") =>
                        {
                            Err(EntryError::Password)
                        }
                        Err(_) => Err(EntryError::Damaged),
                    }
                }
                Err(e) => Err(e),
            });
            Ok(false)
        });
        match (out, walked) {
            (Some(r), _) => r,
            (None, _) => Err(EntryError::Damaged),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Counts what arrives and the largest single piece, keeping none of it.
    struct Meter {
        total: u64,
        largest: usize,
    }
    impl Write for Meter {
        fn write(&mut self, b: &[u8]) -> io::Result<usize> {
            self.total += b.len() as u64;
            self.largest = self.largest.max(b.len());
            Ok(b.len())
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[test]
    fn an_entry_is_copied_a_piece_at_a_time_never_whole() {
        // a 21 KB zip holding 64 MB of zeros: read whole, it would be 64 MB in memory
        let dir = std::env::temp_dir().join(format!("dotany-archive-copy-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("zeros.zip");
        {
            let mut z = zip::ZipWriter::new(File::create(&path).unwrap());
            z.start_file(
                "zeros.bin",
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated)
                    .large_file(false),
            )
            .unwrap();
            let block = vec![0u8; 1 << 20];
            for _ in 0..64 {
                z.write_all(&block).unwrap();
            }
            z.finish().unwrap();
        }
        assert!(std::fs::metadata(&path).unwrap().len() < 1 << 20, "the file itself is small");
        let mut src = Source::open(&path, None).unwrap();
        let i = src.entries.iter().find(|e| !e.d).unwrap().i;
        let mut m = Meter { total: 0, largest: 0 };
        assert_eq!(src.copy_one(i, None, &mut m), Ok(64 << 20));
        assert_eq!(m.total, 64 << 20);
        assert!(m.largest <= 1 << 20, "pieces of at most 1 MB, got {}", m.largest);
        drop(src);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ---------- the less common formats: every file is made here, from code ----------

    use crate::codec::tests::pack;

    const CODECS: [Codec; 4] = [Codec::Gzip, Codec::Bzip2, Codec::Xz, Codec::Zstd];

    /// A folder of its own for one test, removed when the test ends.
    struct Scratch(PathBuf);
    impl Scratch {
        fn new(name: &str) -> Scratch {
            let d = std::env::temp_dir().join(format!("dotany-archive-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&d);
            std::fs::create_dir_all(&d).unwrap();
            Scratch(d)
        }
        fn file(&self, name: &str, bytes: &[u8]) -> PathBuf {
            let p = self.0.join(name);
            std::fs::write(&p, bytes).unwrap();
            p
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn text(n: usize) -> Vec<u8> {
        (0..n).map(|i| b"the quick brown fox jumps over the lazy dog\n"[i % 44]).collect()
    }

    fn tar_of(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut b = tar::Builder::new(Vec::new());
        for (name, data) in files {
            let mut h = tar::Header::new_gnu();
            h.set_path(name).unwrap();
            h.set_size(data.len() as u64);
            h.set_mode(0o644);
            h.set_mtime(1_700_000_000);
            h.set_cksum();
            b.append(&h, *data).unwrap();
        }
        b.into_inner().unwrap()
    }

    /// cpio's "new" format (070701), as the kernel's initramfs and RPM use it.
    fn cpio_of(files: &[(&[u8], &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        let pad = |out: &mut Vec<u8>| {
            while out.len() % 4 != 0 {
                out.push(0);
            }
        };
        let mut put = |name: &[u8], mode: u32, data: &[u8], ino: u32| {
            let head = format!(
                "070701{ino:08X}{mode:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}{:08X}",
                0,
                0,
                1,
                1_700_000_000u32,
                data.len(),
                0,
                0,
                0,
                0,
                name.len() + 1,
                0
            );
            out.extend(head.as_bytes());
            out.extend(name);
            out.push(0);
            pad(&mut out);
            out.extend(data);
            pad(&mut out);
        };
        for (k, (name, data)) in files.iter().enumerate() {
            let dir = name.ends_with(b"/");
            let name = if dir { &name[..name.len() - 1] } else { name };
            put(name, if dir { 0o040755 } else { 0o100644 }, data, k as u32 + 1);
        }
        put(b"TRAILER!!!", 0, b"", 0);
        out
    }

    /// An ar archive, the common format (.deb, a static library).
    fn ar_of(members: &[(&str, &[u8])]) -> Vec<u8> {
        let mut out = b"!<arch>\n".to_vec();
        for (name, data) in members {
            out.extend(
                format!("{:<16}{:<12}{:<6}{:<6}{:<8}{:<10}`\n", name, 1_700_000_000u32, 0, 0, 100644, data.len())
                    .as_bytes(),
            );
            out.extend(*data);
            if data.len() % 2 == 1 {
                out.push(b'\n');
            }
        }
        out
    }

    /// An RPM: the lead, a signature header (padded to 8 bytes) and the header, each with one small entry, then `payload`.
    fn rpm_of(payload: &[u8]) -> Vec<u8> {
        let mut out = vec![0xED, 0xAB, 0xEE, 0xDB, 3, 0];
        out.resize(96, 0);
        for (data, pad) in [(4usize, true), (8usize, false)] {
            out.extend([0x8E, 0xAD, 0xE8, 0x01, 0, 0, 0, 0]);
            out.extend(1u32.to_be_bytes());
            out.extend((data as u32).to_be_bytes());
            out.extend([0, 0, 0x03, 0xE8, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0, 1]);
            out.extend(vec![7u8; data]);
            if pad {
                while (out.len() - 96) % 8 != 0 {
                    out.push(0);
                }
            }
        }
        out.extend(payload);
        out
    }

    /// LHA's CRC-16 (the ARC one: reflected 0x8005, from 0).
    fn crc16(data: &[u8]) -> u16 {
        let mut c = 0u16;
        for &b in data {
            c ^= b as u16;
            for _ in 0..8 {
                c = if c & 1 != 0 { (c >> 1) ^ 0xA001 } else { c >> 1 };
            }
        }
        c
    }

    /// An LHA archive of stored files (-lh0-), level-0 headers: names are the archive's bytes.
    fn lha_of(files: &[(&[u8], &[u8])]) -> Vec<u8> {
        let mut out = Vec::new();
        for (name, data) in files {
            let mut h = vec![0u8, 0];
            h.extend(b"-lh0-");
            h.extend((data.len() as u32).to_le_bytes());
            h.extend((data.len() as u32).to_le_bytes());
            // 03:04:06 on 2 January 2024, as MS-DOS time then date
            h.extend(((3u16 << 11) | (4 << 5) | 3).to_le_bytes());
            h.extend((((2024u16 - 1980) << 9) | (1 << 5) | 2).to_le_bytes());
            h.push(0x20);
            h.push(0);
            h.push(name.len() as u8);
            h.extend(*name);
            h.extend(crc16(data).to_le_bytes());
            h[0] = (h.len() - 2) as u8;
            h[1] = h[2..].iter().fold(0u8, |a, b| a.wrapping_add(*b));
            out.extend(h);
            out.extend(*data);
        }
        out.push(0);
        out
    }

    fn names(src: &Source) -> Vec<String> {
        src.entries.iter().map(|e| e.n.clone().unwrap_or_else(|| format!("raw:{}", e.r.clone().unwrap()))).collect()
    }

    fn read_all(src: &mut Source, i: usize) -> Vec<u8> {
        let mut v = Vec::new();
        src.copy_one(i, None, &mut v).unwrap();
        v
    }

    #[test]
    fn a_compressed_file_is_an_archive_of_one_file_named_after_it() {
        let s = Scratch::new("single");
        let data = text(300_000);
        for (c, name, inner) in [
            (Codec::Gzip, "server.log.gz", "server.log"),
            (Codec::Bzip2, "dump.sql.bz2", "dump.sql"),
            (Codec::Xz, "notes.txt.xz", "notes.txt"),
            (Codec::Zstd, "data.csv.zst", "data.csv"),
            (Codec::Gzip, "odd.tgz", "odd.tar"),
        ] {
            let packed = pack(c, &data);
            let p = s.file(name, &packed);
            assert_eq!(sniff_kind(&p), Some(Kind::Single(c)), "{name}");
            let mut src = Source::open(&p, None).unwrap();
            assert_eq!(src.kind.name(), c.suffix());
            assert_eq!(names(&src), [inner], "{name}");
            assert_eq!(
                (src.entries[0].s, src.entries[0].c, src.entries[0].d),
                (data.len() as u64, packed.len() as u64, false)
            );
            assert_eq!(read_all(&mut src, 0), data);
            assert_eq!(src.read_one(0, None, 10).unwrap(), &data[..10]);
            drop(src);
            // opening and reading never touch the file
            assert_eq!(std::fs::read(&p).unwrap(), packed);
        }
    }

    #[test]
    fn gzip_keeps_the_files_own_time() {
        let s = Scratch::new("gztime");
        let mut gz = flate2::GzBuilder::new().mtime(1_600_000_000).write(Vec::new(), flate2::Compression::fast());
        gz.write_all(b"hello").unwrap();
        let p = s.file("a.txt.gz", &gz.finish().unwrap());
        assert_eq!(Source::open(&p, None).unwrap().entries[0].u, Some(1_600_000_000));
    }

    #[test]
    fn a_compressed_tar_lists_and_reads_like_a_tar_gz() {
        let s = Scratch::new("ctar");
        let big = text(200_000);
        let tar = tar_of(&[("docs/readme.txt", b"read me"), ("docs/big.txt", &big), ("top.txt", b"top")]);
        for (c, name, kind) in [
            (Codec::Gzip, "a.tar.gz", "tar.gz"),
            (Codec::Bzip2, "a.tar.bz2", "tar.bz2"),
            (Codec::Xz, "a.txz", "tar.xz"),
            (Codec::Zstd, "a.tar.zst", "tar.zst"),
        ] {
            let p = s.file(name, &pack(c, &tar));
            assert_eq!(sniff_kind(&p), Some(Kind::Tar(Some(c))), "{name}");
            let mut src = Source::open(&p, None).unwrap();
            assert_eq!(src.kind.name(), kind);
            assert_eq!(names(&src), ["docs/readme.txt", "docs/big.txt", "top.txt"], "{name}");
            assert_eq!(src.entries[1].s, big.len() as u64);
            assert_eq!(read_all(&mut src, 1), big);
            assert_eq!(read_all(&mut src, 2), b"top");
            // what the whole takes on disk is shared out among the entries
            let packed: u64 = src.entries.iter().map(|e| e.c).sum();
            assert!(packed <= std::fs::metadata(&p).unwrap().len() && packed > 0);
        }
    }

    #[test]
    fn a_cut_or_broken_compressed_file_is_damaged() {
        let s = Scratch::new("cut");
        for c in CODECS {
            let mut packed = pack(c, &text(100_000).iter().enumerate().map(|(i, b)| b ^ (i as u8)).collect::<Vec<_>>());
            packed.truncate(packed.len() * 2 / 3);
            let p = s.file(&format!("cut.{}", c.suffix()), &packed);
            assert!(matches!(Source::open(&p, None), Err(OpenError::Damaged)), "{c:?}");
        }
    }

    #[test]
    fn cpio_bare_and_compressed() {
        let s = Scratch::new("cpio");
        let cpio = cpio_of(&[(b"etc/", b""), (b"etc/hostname", b"box\n"), (b"init", b"#!/bin/sh\necho hi\n")]);
        let bare = s.file("root.cpio", &cpio);
        assert_eq!(sniff_kind(&bare), Some(Kind::Cpio(None)));
        let mut src = Source::open(&bare, None).unwrap();
        assert_eq!(names(&src), ["etc", "etc/hostname", "init"]);
        assert!(src.entries[0].d);
        assert_eq!(read_all(&mut src, 2), b"#!/bin/sh\necho hi\n");
        assert_eq!(src.entries[1].u, Some(1_700_000_000));
        for c in CODECS {
            let p = s.file(&format!("initrd.{}", c.suffix()), &pack(c, &cpio));
            assert_eq!(sniff_kind(&p), Some(Kind::Cpio(Some(c))));
            let mut src = Source::open(&p, None).unwrap();
            assert_eq!(src.kind.name(), format!("cpio.{}", c.suffix()));
            assert_eq!(names(&src), ["etc", "etc/hostname", "init"]);
            assert_eq!(read_all(&mut src, 1), b"box\n");
        }
    }

    #[test]
    fn a_deb_shows_its_three_members_and_ar_hides_its_symbol_table() {
        let s = Scratch::new("ar");
        let control = pack(Codec::Gzip, &tar_of(&[("./control", b"Package: hello\n")]));
        let data = pack(Codec::Xz, &tar_of(&[("./usr/bin/hello", b"\x7fELF")]));
        let p = s.file(
            "hello_1.0_amd64.deb",
            &ar_of(&[("debian-binary", b"2.0\n"), ("control.tar.gz", &control), ("data.tar.xz", &data)]),
        );
        assert_eq!(sniff_kind(&p), Some(Kind::Deb));
        let mut src = Source::open(&p, None).unwrap();
        assert_eq!(src.kind.name(), "deb");
        assert_eq!(names(&src), ["debian-binary", "control.tar.gz", "data.tar.xz"]);
        assert_eq!(read_all(&mut src, 0), b"2.0\n");
        assert_eq!(read_all(&mut src, 2), data);
        // a static library: GNU's symbol table "/" is the linker's, not a member
        let lib = s.file("libx.a", &ar_of(&[("/", &[0, 0, 0, 0]), ("one.o/", b"object one"), ("two.o/", b"object 2")]));
        assert_eq!(sniff_kind(&lib), Some(Kind::Ar));
        let mut src = Source::open(&lib, None).unwrap();
        assert_eq!(names(&src), ["one.o", "two.o"]);
        let two = src.entries[1].i;
        assert_eq!(read_all(&mut src, two), b"object 2");
    }

    #[test]
    fn an_rpm_shows_the_files_of_its_payload() {
        let s = Scratch::new("rpm");
        let cpio =
            cpio_of(&[(b"./usr/share/doc/hello/README", b"hello from an rpm"), (b"./usr/bin/hello", b"\x7fELF bin")]);
        for c in CODECS {
            let p = s.file(&format!("hello-{}.rpm", c.suffix()), &rpm_of(&pack(c, &cpio)));
            assert_eq!(sniff_kind(&p), Some(Kind::Rpm));
            let mut src = Source::open(&p, None).unwrap();
            assert_eq!(names(&src), ["./usr/share/doc/hello/README", "./usr/bin/hello"], "{c:?}");
            assert_eq!(read_all(&mut src, 0), b"hello from an rpm");
        }
        // a payload compressed some other way isn't read
        let p = s.file("odd.rpm", &rpm_of(&[0x5D, 0, 0, 0x80, 0, 0xFF, 0xFF]));
        assert!(matches!(Source::open(&p, None), Err(OpenError::Damaged)));
    }

    #[test]
    fn lha_lists_and_reads_keeping_the_names_bytes() {
        let s = Scratch::new("lha");
        // 日本語.txt in Shift_JIS, as LHA archives from Japan carry it: the page guesses the encoding, as for old zips
        let sjis: &[u8] = b"\x93\xfa\x96\x7b\x8c\xea.txt";
        let p = s.file("old.lzh", &lha_of(&[(b"README.TXT", b"Read me first."), (sjis, "こんにちは".as_bytes())]));
        assert_eq!(sniff_kind(&p), Some(Kind::Lha));
        let mut src = Source::open(&p, None).unwrap();
        assert_eq!(src.kind.name(), "lzh");
        assert_eq!(src.entries[0].n.as_deref(), Some("README.TXT"));
        assert_eq!(src.entries[1].r.as_deref(), Some(b64(sjis).as_str()));
        assert_eq!(read_all(&mut src, 0), b"Read me first.");
        assert_eq!(read_all(&mut src, 1), "こんにちは".as_bytes());
        assert!(src.entries[0].u.is_some());
        // a damaged byte in the data: the entry's CRC says so
        let mut bad = lha_of(&[(b"A.TXT", b"0123456789")]);
        let at = bad.len() - 4;
        bad[at] ^= 1;
        let p = s.file("bad.lzh", &bad);
        let mut src = Source::open(&p, None).unwrap();
        assert!(src.copy_one(0, None, &mut Vec::new()).is_err());
    }

    /// The folder of archives made by WinRAR, 7-Zip and other programs that the test below reads, where there is one:
    /// otherwise that test ends at once.
    fn samples() -> Option<PathBuf> {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../archive/test/fixtures");
        if dir.is_dir() {
            return Some(dir);
        }
        eprintln!("skipped: {} is not here", dir.display());
        None
    }

    fn sample(name: &str) -> PathBuf {
        samples().unwrap().join(name)
    }

    /// The entry by its name (a folder's with or without its closing slash).
    fn entry<'a>(src: &'a Source, name: &str) -> &'a Entry {
        src.entries
            .iter()
            .find(|e| e.n.as_deref().map(|n| n.trim_end_matches('/')) == Some(name))
            .unwrap_or_else(|| panic!("{name}: {:?}", names(src)))
    }

    #[test]
    fn rar_7z_and_zip_made_elsewhere_list_and_read() {
        if samples().is_none() {
            return;
        }
        // RAR 4, through libarchive: a file, a folder, an empty folder (its link is left out, as Archive leaves it)
        let mut src = Source::open(&sample("rar.rar"), None).unwrap();
        assert_eq!(src.kind, Kind::Rar);
        let e = entry(&src, "test.txt").clone();
        assert_eq!((e.s, e.d), (20, false));
        assert_eq!(read_all(&mut src, e.i), b"test text document\r\n");
        let i = entry(&src, "testdir/test.txt").i;
        assert_eq!(read_all(&mut src, i), b"test text document\r\n");
        assert!(entry(&src, "testemptydir").d);
        // RAR 4's Unicode names, and RAR 5's (the Mac's libarchive gives them decomposed, NFD, so what is looked for
        // here has no letter that decomposes)
        let src = Source::open(&sample("rar_unicode.rar"), None).unwrap();
        assert!(names(&src).iter().any(|n| n.contains("長いファイル名long-filename-in-")), "{:?}", names(&src));
        // and a name stored as UTF-8 bytes without RAR's Unicode mark: read as written where names are UTF-8 (Windows
        // reads it in its own code page)
        #[cfg(target_os = "macos")]
        assert_eq!(entry(&src, "abcdefghijklmnopqrsテスト.txt").s, 16);
        let src = Source::open(&sample("rar5_unicode.rar"), None).unwrap();
        let all = names(&src);
        assert!(!all.is_empty());
        assert!(all.iter().all(|n| !n.is_empty() && !n.contains('?') && !n.contains('\u{fffd}')), "{all:?}");
        assert!(all.iter().any(|n| n.chars().any(|c| !c.is_ascii())), "{all:?}: names beyond ASCII");
        // RAR 5, compressed, read whole and held to its checksum by libarchive
        let mut src = Source::open(&sample("rar5_compressed.rar"), None).unwrap();
        let i = entry(&src, "test.bin").i;
        let data = read_all(&mut src, i);
        assert_eq!(data.len(), 1200);
        assert_eq!(&data[8..13], &[1, 0, 0, 0, 5]);
        // 7z (LZMA2) and zip (deflate), each held to its CRC
        let mut src = Source::open(&sample("7zip_lzma2.7z"), None).unwrap();
        assert_eq!(src.kind, Kind::SevenZ);
        let i = entry(&src, "file1").i;
        let data = read_all(&mut src, i);
        assert_eq!(data.len(), 2844);
        assert!(data.starts_with(b"The libarchive distribution as a whole is Copyright by Tim Kientzle"));
        let mut src = Source::open(&sample("zip_7z_deflate.zip"), None).unwrap();
        assert_eq!(src.kind, Kind::Zip);
        let i = entry(&src, "libxkbcommon-x11.so.0.0.0").i;
        let data = read_all(&mut src, i);
        assert_eq!((data.len(), &data[..4]), (50280, &b"\x7fELF"[..]));
        // and what WinRAR locked: never read without its password
        for name in ["rar4_encrypted.rar", "rar5_encrypted.rar"] {
            match Source::open(&sample(name), None) {
                Ok(mut src) => {
                    let locked: Vec<usize> = src.entries.iter().filter(|e| e.l && !e.d).map(|e| e.i).collect();
                    assert!(!locked.is_empty(), "{name}: locked entries");
                    assert!(src.copy_one(locked[0], None, &mut Vec::new()).is_err(), "{name}");
                }
                Err(e) => assert!(matches!(e, OpenError::Locked | OpenError::Damaged), "{name}: {e:?}"),
            }
        }
    }

    #[test]
    fn what_is_not_an_archive_stays_unknown() {
        let s = Scratch::new("none");
        for (name, bytes) in [
            ("a.txt", b"just words, nothing packed".to_vec()),
            ("b.bin", vec![0u8; 600]),
            ("c.lzh", b"-lh5- but not where it counts".to_vec()),
        ] {
            assert_eq!(sniff_kind(&s.file(name, &bytes)), None, "{name}");
        }
    }
}
