//! What the two containers share: the facts the page wants, the edit a save asks for, and the save itself as a list of pieces.
//! Nothing in this program decodes or encodes: a sample's bytes are copied, unread, from the old file to the new one.

use crate::formats::Container;
use crate::{mkv, mp4};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::Path;

#[derive(Serialize, Clone, Default)]
pub struct Facts {
    pub container: &'static str,
    pub video: Option<String>,
    pub audio: Option<String>,
    pub width: u32,
    pub height: u32,
    pub duration: f64,
    /// Where lossless cuts can land, in seconds from the start of the film, in order.
    pub keys: Vec<f64>,
    pub title: Option<String>,
    pub has_cover: bool,
    pub can_trim: bool,
    pub can_tag: bool,
    /// Played by Windows rather than WebView2: AVI and WMV. Such a film is only played.
    pub system: bool,
}

#[derive(Deserialize, Clone, Default)]
pub struct Request {
    /// Index into `keys`. Left out, or at the very ends of the list, means that end isn't cut.
    pub from_key: Option<usize>,
    pub to_key: Option<usize>,
    pub title: Option<String>,
    #[serde(default)]
    pub cover: bool,
}

#[derive(Clone)]
pub struct Cover {
    pub bytes: Vec<u8>,
    pub png: bool,
}

impl Cover {
    /// Only JPG and PNG, by their first bytes.
    pub fn from_bytes(bytes: Vec<u8>) -> Option<Cover> {
        let png = bytes.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
        (png || bytes.starts_with(&[0xFF, 0xD8, 0xFF])).then_some(Cover { bytes, png })
    }
}

pub struct Edit {
    pub from_key: Option<usize>,
    pub to_key: Option<usize>,
    pub title: Option<String>,
    pub cover: Option<Cover>,
}

impl Edit {
    pub fn is_empty(&self) -> bool {
        self.from_key.is_none() && self.to_key.is_none() && self.title.is_none() && self.cover.is_none()
    }
}

pub enum Piece {
    Bytes(Vec<u8>),
    /// A stretch of the source file: where, how long.
    File(u64, u64),
}

/// What a full disk answers a write, for a test that breaks the disk (`write_until`): Windows' ERROR_DISK_FULL, the Mac's
/// ENOSPC. Each system's own, so the save is told apart as a full disk there (the family's `write_failure`).
#[cfg(not(target_os = "macos"))]
const DISK_FULL: i32 = 112;
#[cfg(target_os = "macos")]
const DISK_FULL: i32 = 28;

/// The whole new file, written down before any of it is written, so its size is known exactly.
#[derive(Default)]
pub struct Plan {
    pub pieces: Vec<Piece>,
    pub size: u64,
}

impl Plan {
    pub fn bytes(&mut self, b: Vec<u8>) {
        self.size += b.len() as u64;
        self.pieces.push(Piece::Bytes(b));
    }
    pub fn file(&mut self, at: u64, len: u64) {
        if len == 0 {
            return;
        }
        self.size += len;
        if let Some(Piece::File(a, l)) = self.pieces.last_mut() {
            if *a + *l == at {
                *l += len;
                return;
            }
        }
        self.pieces.push(Piece::File(at, len));
    }
    /// `fail_at`, for tests: the disk "breaks" after this many bytes.
    pub fn write_until(&self, source: &mut File, out: &mut File, fail_at: Option<u64>) -> io::Result<()> {
        let mut buf = vec![0u8; 1 << 20];
        let mut done = 0u64;
        let mut check = |n: u64| -> io::Result<()> {
            done += n;
            if fail_at.is_some_and(|f| done >= f) {
                return Err(io::Error::from_raw_os_error(DISK_FULL));
            }
            Ok(())
        };
        for piece in &self.pieces {
            match piece {
                Piece::Bytes(b) => {
                    out.write_all(b)?;
                    check(b.len() as u64)?;
                }
                Piece::File(at, len) => {
                    source.seek(SeekFrom::Start(*at))?;
                    let mut left = *len;
                    while left > 0 {
                        let n = left.min(buf.len() as u64) as usize;
                        source.read_exact(&mut buf[..n])?;
                        out.write_all(&buf[..n])?;
                        check(n as u64)?;
                        left -= n as u64;
                    }
                }
            }
        }
        Ok(())
    }
}

pub enum Media {
    Mp4(Box<mp4::Movie>),
    Mkv(Box<mkv::Matroska>),
    /// Plays if the system can play it; nothing in it is touched.
    Sealed(Facts),
}

impl Media {
    /// A file whose insides can't be read may still play, so this never refuses; it only turns the editing off.
    pub fn open(path: &Path, kind: Container) -> Media {
        let read = std::panic::catch_unwind(|| match kind {
            Container::Mp4 => mp4::Movie::open(path).ok().map(|m| Media::Mp4(Box::new(m))),
            Container::Matroska | Container::Webm => {
                mkv::Matroska::open(path, kind).ok().map(|m| Media::Mkv(Box::new(m)))
            }
            Container::Avi | Container::Asf => None,
        });
        read.unwrap_or(None).unwrap_or_else(|| {
            Media::Sealed(Facts { container: kind.label(), system: kind.by_system(), ..Facts::default() })
        })
    }
    pub fn facts(&self) -> Facts {
        match self {
            Media::Mp4(m) => m.facts(),
            Media::Mkv(m) => m.facts(),
            Media::Sealed(f) => f.clone(),
        }
    }
    pub fn cover(&self, path: &Path) -> Option<Vec<u8>> {
        match self {
            Media::Mp4(m) => m.cover(),
            Media::Mkv(m) => m.cover(path),
            Media::Sealed(_) => None,
        }
    }
    pub fn plan(&self, path: &Path, edit: &Edit) -> Result<Plan, String> {
        let made = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| match self {
            Media::Mp4(m) => m.plan(edit),
            Media::Mkv(m) => m.plan(path, edit),
            // nothing in it can change, so a save is the file itself, byte for byte
            Media::Sealed(_) if edit.is_empty() => {
                let mut plan = Plan::default();
                plan.file(0, std::fs::metadata(path).map_err(|e| e.to_string())?.len());
                Ok(plan)
            }
            Media::Sealed(_) => Err("nothing in this file can be changed".to_string()),
        }));
        made.unwrap_or_else(|_| Err("this file's structure isn't one we can rewrite".to_string()))
    }
}
