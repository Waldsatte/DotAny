//! The payload rides at the end of DotAny-Setup.exe:
//!
//! ```text
//! [the installer program][one zstd stream: every file, back to back][release JSON][footer]
//! footer = MAGIC (16 bytes) + where the stream starts (u64 LE) + its length (u64 LE) + the JSON's length (u64 LE)
//! ```
//!
//! One stream with a long window, because the programs are built from the same crates and the same Tauri runtime, so
//! much of each one is alike: the compressor finds what a program shares in the program before it, and the download
//! holds it about once. A signature added later goes after the footer, so the footer is looked for near the end rather
//! than at the very end.

use crate::manifest::{FileEntry, Release};
use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

pub const MAGIC: &[u8; 16] = b"DotAnyPayload-01";
pub const FOOTER: u64 = 16 + 8 * 3;
/// 2^26: 64 MB, more than the largest program, so the whole of the program before is still in reach.
pub const WINDOW_LOG: u32 = 26;

pub struct Payload {
    pub path: PathBuf,
    /// Where the stream starts, which is also how long the installer program alone is.
    pub program_len: u64,
    pub stream_len: u64,
    pub release: Release,
}

impl Payload {
    /// The payload at the end of this file, if it has one; the copied uninstaller has none.
    pub fn open(path: &Path) -> io::Result<Option<Payload>> {
        let mut file = File::open(path)?;
        let len = file.metadata()?.len();
        let tail = len.min(512 * 1024);
        file.seek(SeekFrom::Start(len - tail))?;
        let mut buf = vec![0u8; tail as usize];
        file.read_exact(&mut buf)?;
        let Some(at) = buf.windows(16).rposition(|w| w == MAGIC) else { return Ok(None) };
        if at + FOOTER as usize > buf.len() {
            return Ok(None);
        }
        let num = |i: usize| u64::from_le_bytes(buf[at + 16 + i * 8..at + 24 + i * 8].try_into().unwrap());
        let (program_len, stream_len, json_len) = (num(0), num(1), num(2));
        let footer_at = len - tail + at as u64;
        if program_len + stream_len + json_len != footer_at {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "the installer's payload is damaged"));
        }
        file.seek(SeekFrom::Start(program_len + stream_len))?;
        let mut json = vec![0u8; json_len as usize];
        file.read_exact(&mut json)?;
        let release: Release =
            serde_json::from_slice(&json).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        Ok(Some(Payload { path: path.to_path_buf(), program_len, stream_len, release }))
    }

    /// The unpacked stream from its start.
    pub fn stream(&self) -> io::Result<Stream> {
        let mut file = File::open(&self.path)?;
        file.seek(SeekFrom::Start(self.program_len))?;
        let mut decoder =
            zstd::stream::read::Decoder::with_buffer(BufReader::with_capacity(1 << 20, file.take(self.stream_len)))?;
        decoder.window_log_max(WINDOW_LOG)?;
        Ok(Stream { inner: decoder, at: 0 })
    }

    /// The installer program alone, without the payload: the uninstaller.
    pub fn program(&self) -> io::Result<Vec<u8>> {
        let mut file = File::open(&self.path)?;
        let mut out = vec![0u8; self.program_len as usize];
        file.read_exact(&mut out)?;
        Ok(out)
    }

    /// Files from the front of the stream, read into memory (the installer's own fonts).
    pub fn read_front(&self, wanted: &[&str]) -> io::Result<Vec<(String, Vec<u8>)>> {
        let mut stream = self.stream()?;
        let mut out = Vec::new();
        for entry in &self.release.files {
            if out.len() == wanted.len() {
                break;
            }
            stream.skip_to(entry.offset)?;
            if wanted.contains(&entry.path.as_str()) {
                let mut data = vec![0u8; entry.size as usize];
                stream.inner.read_exact(&mut data)?;
                stream.at += entry.size;
                out.push((entry.path.clone(), data));
            }
        }
        Ok(out)
    }
}

pub type Decoder = zstd::stream::read::Decoder<'static, BufReader<io::Take<File>>>;

pub struct Stream {
    pub inner: Decoder,
    pub at: u64,
}

impl Stream {
    pub fn skip_to(&mut self, offset: u64) -> io::Result<()> {
        if offset < self.at {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "the stream only reads forward"));
        }
        let n = io::copy(&mut (&mut self.inner).take(offset - self.at), &mut io::sink())?;
        self.at += n;
        if self.at != offset {
            return Err(io::ErrorKind::UnexpectedEof.into());
        }
        Ok(())
    }

    /// Copies one file out, checking `keep_going` between pieces so a cancel stops it straight away.
    pub fn copy_entry(
        &mut self,
        entry: &FileEntry,
        to: &mut impl io::Write,
        keep_going: &dyn Fn() -> bool,
    ) -> io::Result<String> {
        use sha2::{Digest, Sha256};
        self.skip_to(entry.offset)?;
        let mut left = entry.size;
        let mut buf = vec![0u8; 1 << 20];
        let mut hash = Sha256::new();
        while left > 0 {
            if !keep_going() {
                return Err(io::Error::new(io::ErrorKind::Interrupted, "cancelled"));
            }
            let n = (left as usize).min(buf.len());
            self.inner.read_exact(&mut buf[..n])?;
            hash.update(&buf[..n]);
            to.write_all(&buf[..n])?;
            left -= n as u64;
            self.at += n as u64;
        }
        Ok(hex(&hash.finalize()))
    }
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
