//! Writing a new archive from a list of entries: those still in the open archive are streamed out of it, files added
//! from disk are read from disk. ZIP (AES-256 when there's a password), 7z (AES-256), TAR and TAR.GZ. Never RAR.

use super::source::{EntryError, Source};
use serde::Deserialize;
use std::collections::HashMap;
use std::fs::File;
use std::io::{self, BufWriter, Read, Write};
use std::path::Path;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// One row of what the archive should hold.
#[derive(Deserialize, Clone, Debug)]
pub struct Node {
    pub path: String,
    #[serde(default)]
    pub dir: bool,
    /// the entry in the open archive it comes from
    pub i: Option<usize>,
    /// or the file on disk it comes from
    pub add: Option<String>,
    /// seconds since 1970
    pub u: Option<i64>,
    /// the same moment in local time, "YYYY-MM-DD HH:MM", for ZIP's DOS time
    pub t: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq, Deserialize, Debug)]
pub enum Format {
    #[serde(rename = "zip")]
    Zip,
    #[serde(rename = "7z")]
    SevenZ,
    #[serde(rename = "tar")]
    Tar,
    #[serde(rename = "tgz")]
    TarGz,
}

#[derive(Clone, Copy, PartialEq, Eq, Deserialize, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    Fast,
    Std,
    Tight,
}

#[derive(Debug)]
pub enum PackError {
    /// an encrypted entry with no password, or the wrong one
    Password,
    /// an entry in the open archive couldn't be read
    Damaged(String),
    Io(io::Error),
    Cancelled,
}

impl From<io::Error> for PackError {
    fn from(e: io::Error) -> Self {
        if e.kind() == io::ErrorKind::Interrupted {
            return PackError::Cancelled;
        }
        PackError::Io(e)
    }
}

/// A clean path inside an archive: forward slashes, no leading slash, no empty or `.` parts. `..` is refused.
pub fn clean(path: &str) -> Option<String> {
    let mut parts = Vec::new();
    for p in path.split(['/', '\\']) {
        match p {
            "" | "." => {}
            ".." => return None,
            p => parts.push(p),
        }
    }
    (!parts.is_empty()).then(|| parts.join("/"))
}

fn zip_date(node: &Node) -> Option<zip::DateTime> {
    let t = node.t.as_deref()?;
    let num = |a: usize, b: usize| t.get(a..b).and_then(|s| s.parse::<u16>().ok());
    zip::DateTime::from_date_and_time(
        num(0, 4)?,
        num(5, 7)? as u8,
        num(8, 10)? as u8,
        num(11, 13)? as u8,
        num(14, 16)? as u8,
        num(17, 19).unwrap_or(0) as u8,
    )
    .ok()
}

/// Reads through `inner` while counting bytes for the progress line, and stops when cancelled.
pub struct Counted<'a, R: Read + ?Sized> {
    pub inner: &'a mut R,
    pub done: &'a AtomicU64,
    pub stop: &'a AtomicBool,
}

impl<R: Read + ?Sized> Read for Counted<'_, R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.stop.load(Ordering::Relaxed) {
            return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
        }
        let n = self.inner.read(buf)?;
        self.done.fetch_add(n as u64, Ordering::Relaxed);
        Ok(n)
    }
}

/// Where each kind of writer puts one entry.
trait Sink {
    fn dir(&mut self, path: &str, node: &Node) -> io::Result<()>;
    fn file(&mut self, path: &str, node: &Node, size: u64, data: &mut dyn Read) -> io::Result<()>;
    fn finish(self: Box<Self>) -> io::Result<()>;
}

struct ZipSink<'p> {
    w: zip::ZipWriter<BufWriter<File>>,
    level: i64,
    password: Option<&'p str>,
}

fn zip_options<'p>(
    level: i64,
    password: Option<&'p str>,
    node: &Node,
    size: u64,
) -> zip::write::FileOptions<'p, 'static, ()> {
    let mut o = zip::write::FileOptions::<'p, 'static, ()>::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .compression_level(Some(level))
        .large_file(size >= 0xFFFF_FFFF);
    if let Some(t) = zip_date(node) {
        o = o.last_modified_time(t);
    }
    if let Some(pw) = password {
        o = o.with_aes_encryption(zip::AesMode::Aes256, pw);
    }
    o
}

impl Sink for ZipSink<'_> {
    fn dir(&mut self, path: &str, node: &Node) -> io::Result<()> {
        let mut o = zip::write::SimpleFileOptions::default();
        if let Some(t) = zip_date(node) {
            o = o.last_modified_time(t);
        }
        self.w.add_directory(format!("{path}/"), o).map_err(io::Error::other)
    }
    fn file(&mut self, path: &str, node: &Node, size: u64, data: &mut dyn Read) -> io::Result<()> {
        let o = zip_options(self.level, self.password, node, size);
        self.w.start_file(path, o).map_err(io::Error::other)?;
        io::copy(data, &mut self.w)?;
        Ok(())
    }
    fn finish(self: Box<Self>) -> io::Result<()> {
        let mut out = self.w.finish().map_err(io::Error::other)?;
        out.flush()
    }
}

struct TarSink<W: Write> {
    b: tar::Builder<W>,
    pax: usize,
    finish_inner: fn(W) -> io::Result<()>,
}

fn tar_header(node: &Node, size: u64, dir: bool) -> tar::Header {
    let mut h = tar::Header::new_gnu();
    h.set_size(size);
    h.set_mode(if dir { 0o755 } else { 0o644 });
    h.set_mtime(node.u.unwrap_or(0).max(0) as u64);
    h.set_entry_type(if dir { tar::EntryType::Directory } else { tar::EntryType::Regular });
    h
}

impl<W: Write> TarSink<W> {
    /// A name that isn't plain ASCII, or is too long for the old header, goes in a PAX record first: PAX names are
    /// UTF-8 by definition, so every reader (Windows' own tar among them) reads them the same. The header itself then
    /// carries a short ASCII stand-in, for readers that know no PAX.
    fn name(&mut self, path: &str) -> io::Result<String> {
        if path.is_ascii() && path.len() < 100 {
            return Ok(path.to_string());
        }
        let record = |k: &str, v: &str| {
            let body = format!(" {k}={v}\n");
            let mut n = body.len() + 1;
            while format!("{n}{body}").len() != n {
                n += 1;
            }
            format!("{n}{body}")
        };
        let pax = record("path", path);
        let mut h = tar::Header::new_ustar();
        h.set_entry_type(tar::EntryType::XHeader);
        h.set_size(pax.len() as u64);
        h.set_mode(0o644);
        self.pax += 1;
        h.set_path(format!("PaxHeader/{}", self.pax))?;
        h.set_cksum();
        self.b.append(&h, pax.as_bytes())?;
        let dir = path.ends_with('/');
        let ext = Path::new(path.trim_end_matches('/'))
            .extension()
            .and_then(|e| e.to_str())
            .filter(|e| e.is_ascii() && e.len() < 12)
            .map(|e| format!(".{e}"))
            .unwrap_or_default();
        Ok(format!("dotany-{}{ext}{}", self.pax, if dir { "/" } else { "" }))
    }
}

impl<W: Write> Sink for TarSink<W> {
    fn dir(&mut self, path: &str, node: &Node) -> io::Result<()> {
        let name = self.name(&format!("{path}/"))?;
        let mut h = tar_header(node, 0, true);
        h.set_path(&name)?;
        h.set_cksum();
        self.b.append(&h, io::empty())
    }
    fn file(&mut self, path: &str, node: &Node, size: u64, data: &mut dyn Read) -> io::Result<()> {
        let name = self.name(path)?;
        let mut h = tar_header(node, size, false);
        h.set_path(&name)?;
        h.set_cksum();
        // a tar header states the size first; a source that turns out shorter is padded, longer is cut, and the
        // entry's CRC check upstream reports it as damaged
        let mut exact = data.take(size).chain(io::repeat(0).take(size));
        let exact = (&mut exact).take(size);
        self.b.append(&h, exact)
    }
    fn finish(self: Box<Self>) -> io::Result<()> {
        let inner = self.b.into_inner()?;
        (self.finish_inner)(inner)
    }
}

struct SevenZSink {
    w: sevenz_rust2::ArchiveWriter<BufWriter<File>>,
}

fn nt_time(node: &Node) -> Option<sevenz_rust2::NtTime> {
    let u = node.u?;
    let t = std::time::UNIX_EPOCH.checked_add(std::time::Duration::from_secs(u.max(0) as u64))?;
    sevenz_rust2::NtTime::try_from(t).ok()
}

/// The attributes 7-Zip itself gives every entry. Without any, a reader has to guess: libarchive (the Mac's tar,
/// Windows' tar.exe) then makes a folder `0644`, which nobody can enter. With these it makes folders `0755` and files
/// `0644`, the modes the TAR writer here uses too. No Unix modes beside them: the entries carry none.
const ATTR_DIRECTORY: u32 = 0x10;
const ATTR_ARCHIVE: u32 = 0x20;

impl Sink for SevenZSink {
    fn dir(&mut self, path: &str, node: &Node) -> io::Result<()> {
        let mut e = sevenz_rust2::ArchiveEntry::new_directory(path);
        e.has_windows_attributes = true;
        e.windows_attributes = ATTR_DIRECTORY;
        if let Some(t) = nt_time(node) {
            e.last_modified_date = t;
            e.has_last_modified_date = true;
        }
        self.w.push_archive_entry::<&[u8]>(e, None).map_err(|e| io::Error::other(e.to_string()))?;
        Ok(())
    }
    fn file(&mut self, path: &str, node: &Node, size: u64, data: &mut dyn Read) -> io::Result<()> {
        let mut e = sevenz_rust2::ArchiveEntry::new_file(path);
        e.has_windows_attributes = true;
        e.windows_attributes = ATTR_ARCHIVE;
        if let Some(t) = nt_time(node) {
            e.last_modified_date = t;
            e.has_last_modified_date = true;
        }
        // an empty file has no data at all, as 7-Zip writes it: nothing to compress or lock
        if size == 0 {
            io::copy(data, &mut io::sink())?;
            self.w.push_archive_entry::<&[u8]>(e, None).map_err(|e| io::Error::other(e.to_string()))?;
            return Ok(());
        }
        // a cancelled or failed read surfaces through the writer as its own error; keep the real one
        let mut failure: Option<io::Error> = None;
        let mut guard = Guarded { inner: data, failure: &mut failure };
        let r = self.w.push_archive_entry(e, Some(&mut guard));
        if let Some(f) = failure {
            return Err(f);
        }
        r.map_err(|e| io::Error::other(e.to_string()))?;
        Ok(())
    }
    fn finish(self: Box<Self>) -> io::Result<()> {
        let mut out = self.w.finish()?;
        out.flush()
    }
}

struct Guarded<'a, 'b> {
    inner: &'a mut dyn Read,
    failure: &'b mut Option<io::Error>,
}

impl Read for Guarded<'_, '_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        match self.inner.read(buf) {
            Ok(n) => Ok(n),
            Err(e) => {
                let copy = io::Error::new(e.kind(), e.to_string());
                *self.failure = Some(e);
                Err(copy)
            }
        }
    }
}

fn sink<'p>(format: Format, level: Level, password: Option<&'p str>, file: File) -> io::Result<Box<dyn Sink + 'p>> {
    let out = BufWriter::with_capacity(1 << 20, file);
    Ok(match format {
        Format::Zip => Box::new(ZipSink {
            w: zip::ZipWriter::new(out),
            level: match level {
                Level::Fast => 1,
                Level::Std => 6,
                Level::Tight => 9,
            },
            password,
        }),
        Format::Tar => Box::new(TarSink { b: tar::Builder::new(out), pax: 0, finish_inner: |mut w| w.flush() }),
        Format::TarGz => {
            let gz = flate2::write::GzEncoder::new(
                out,
                flate2::Compression::new(match level {
                    Level::Fast => 1,
                    Level::Std => 6,
                    Level::Tight => 9,
                }),
            );
            Box::new(TarSink { b: tar::Builder::new(gz), pax: 0, finish_inner: |w| w.finish()?.flush() })
        }
        Format::SevenZ => {
            let mut w = sevenz_rust2::ArchiveWriter::new(out).map_err(|e| io::Error::other(e.to_string()))?;
            let lzma = sevenz_rust2::encoder_options::Lzma2Options::from_level(match level {
                Level::Fast => 1,
                Level::Std => 5,
                Level::Tight => 9,
            });
            let mut methods: Vec<sevenz_rust2::EncoderConfiguration> = Vec::new();
            if let Some(pw) = password {
                methods.push(
                    sevenz_rust2::encoder_options::AesEncoderOptions::new(sevenz_rust2::Password::from(pw)).into(),
                );
            }
            methods.push(lzma.into());
            w.set_content_methods(methods);
            // names stay readable without the password, as 7-Zip does by default; the contents are what it locks
            w.set_encrypt_header(false);
            Box::new(SevenZSink { w })
        }
    })
}

/// Writes `nodes` as a new archive into `out`. `done` counts bytes read for the progress line.
pub fn write_archive(
    out: &Path,
    format: Format,
    level: Level,
    password: Option<&str>,
    read_password: Option<&str>,
    source: Option<&mut Source>,
    nodes: &[Node],
    done: &AtomicU64,
    now: &dyn Fn(&str),
    stop: &AtomicBool,
) -> Result<(), PackError> {
    let pass = if format == Format::Tar || format == Format::TarGz { None } else { password.filter(|p| !p.is_empty()) };
    let mut w = sink(format, level, pass, File::create(out)?)?;
    // folders first, so every reader sees a folder before what's in it
    let mut written = std::collections::HashSet::new();
    for n in nodes.iter().filter(|n| n.dir) {
        let Some(p) = clean(&n.path) else { continue };
        if written.insert(p.clone()) {
            w.dir(&p, n)?;
        }
    }
    // entries from the open archive, in its own order: one pass, even through a solid 7z or a TAR.GZ
    let from_source: HashMap<usize, &Node> =
        nodes.iter().filter(|n| !n.dir).filter_map(|n| n.i.map(|i| (i, n))).collect();
    if let Some(src) = source {
        if !from_source.is_empty() {
            let sizes: HashMap<usize, u64> = src.entries.iter().map(|e| (e.i, e.s)).collect();
            let mut failure: Option<PackError> = None;
            src.walk(read_password, &|i| from_source.contains_key(&i), &mut |i, data| {
                let node = from_source[&i];
                let Some(p) = clean(&node.path) else { return Ok(true) };
                match data {
                    Ok(r) => {
                        let mut counted = Counted { inner: r, done, stop };
                        now(&p);
                        if let Err(e) = w.file(&p, node, sizes.get(&i).copied().unwrap_or(0), &mut counted) {
                            failure = Some(if e.kind() == io::ErrorKind::Interrupted {
                                PackError::Cancelled
                            } else if e.kind() == io::ErrorKind::InvalidData
                                || e.to_string().to_lowercase().contains("checksum")
                                || e.to_string().to_lowercase().contains("password")
                            {
                                if read_password.is_some() && e.to_string().to_lowercase().contains("password") {
                                    PackError::Password
                                } else {
                                    PackError::Damaged(p.clone())
                                }
                            } else {
                                PackError::Io(e)
                            });
                            return Ok(false);
                        }
                        Ok(true)
                    }
                    Err(EntryError::Password) => {
                        failure = Some(PackError::Password);
                        Ok(false)
                    }
                    Err(_) => {
                        failure = Some(PackError::Damaged(p));
                        Ok(false)
                    }
                }
            })?;
            if let Some(f) = failure {
                return Err(f);
            }
        }
    }
    // files added from disk
    for n in nodes.iter().filter(|n| !n.dir && n.i.is_none()) {
        let (Some(p), Some(from)) = (clean(&n.path), n.add.as_deref()) else { continue };
        let mut f = File::open(from)?;
        let size = f.metadata()?.len();
        let mut counted = Counted { inner: &mut f, done, stop };
        now(&p);
        w.file(&p, n, size, &mut counted)?;
    }
    if stop.load(Ordering::Relaxed) {
        return Err(PackError::Cancelled);
    }
    w.finish()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use dotany_unpack::source::Source;

    struct Scratch(std::path::PathBuf);
    impl Scratch {
        fn new(name: &str) -> Scratch {
            let d = std::env::temp_dir().join(format!("dotany-convert-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&d);
            std::fs::create_dir_all(&d).unwrap();
            Scratch(d)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn node(path: &str, dir: bool, add: Option<&Path>) -> Node {
        Node { path: path.into(), dir, i: None, add: add.map(|p| p.to_string_lossy().into_owned()), u: None, t: None }
    }

    /// A folder, an empty folder, a file and an empty file, written by the real 7z writer with and without a password:
    /// every entry carries the attributes 7-Zip gives it (so a reader that goes by them makes folders that can be
    /// entered), and the archive reads back whole.
    #[test]
    fn a_7z_written_here_carries_7zips_attributes() {
        let s = Scratch::new("7z-attributes");
        let (text, empty) = (s.0.join("b.txt"), s.0.join("zero.txt"));
        std::fs::write(&text, "some words").unwrap();
        std::fs::write(&empty, "").unwrap();
        let nodes = [
            node("a", true, None),
            node("a/empty", true, None),
            node("a/b.txt", false, Some(&text)),
            node("a/zero.txt", false, Some(&empty)),
        ];
        for password in [None, Some("pw")] {
            let out = s.0.join(if password.is_some() { "locked.7z" } else { "open.7z" });
            let (done, stop) = (AtomicU64::new(0), AtomicBool::new(false));
            let r =
                write_archive(&out, Format::SevenZ, Level::Std, password, None, None, &nodes, &done, &|_| {}, &stop);
            assert!(r.is_ok(), "{r:?}");

            let pw = password.map(sevenz_rust2::Password::from).unwrap_or_else(sevenz_rust2::Password::empty);
            let mut f = io::BufReader::new(File::open(&out).unwrap());
            let archive = sevenz_rust2::Archive::read(&mut f, &pw).unwrap();
            let attrs: Vec<(String, bool, bool, u32)> = archive
                .files
                .iter()
                .map(|e| (e.name.clone(), e.is_directory, e.has_windows_attributes, e.windows_attributes))
                .collect();
            assert_eq!(attrs.len(), 4, "{attrs:?}");
            for (name, dir, has, attr) in &attrs {
                assert!(has, "{name} has attributes");
                assert_eq!(*attr, if *dir { ATTR_DIRECTORY } else { ATTR_ARCHIVE }, "{name}: {attr:#x}");
            }

            // and as the converter itself reads it: the names without the password
            let src = Source::open(&out, None).unwrap();
            let mut listed: Vec<(String, bool, u64)> =
                src.entries.iter().map(|e| (e.n.clone().unwrap_or_default(), e.d, e.s)).collect();
            listed.sort();
            assert_eq!(
                listed,
                [
                    ("a".into(), true, 0),
                    ("a/b.txt".into(), false, 10),
                    ("a/empty".into(), true, 0),
                    ("a/zero.txt".into(), false, 0)
                ]
            );
        }
    }
}
