//! What a file is, from its name and its first bytes.

use serde::Serialize;
use std::fs::File;
use std::io::Read;
use std::path::Path;

pub const VIDEO_EXTENSIONS: [&str; 7] = ["mp4", "m4v", "mov", "mkv", "webm", "avi", "wmv"];
pub const SUBTITLE_EXTENSIONS: [&str; 4] = ["srt", "vtt", "ass", "ssa"];

#[derive(Clone, Copy, PartialEq, Debug, Serialize)]
pub enum Container {
    Mp4,
    Matroska,
    Webm,
    Avi,
    /// WMV: Microsoft's ASF box.
    Asf,
}

impl Container {
    pub fn label(self) -> &'static str {
        match self {
            Container::Mp4 => "mp4",
            Container::Matroska => "mkv",
            Container::Webm => "webm",
            Container::Avi => "avi",
            Container::Asf => "wmv",
        }
    }
    pub fn mime(self) -> &'static str {
        match self {
            Container::Mp4 => "video/mp4",
            Container::Matroska => "video/x-matroska",
            Container::Webm => "video/webm",
            Container::Avi => "video/x-msvideo",
            Container::Asf => "video/x-ms-wmv",
        }
    }
    /// WebView2 can't play these: Windows' Media Foundation does, and they're never rewritten.
    pub fn by_system(self) -> bool {
        matches!(self, Container::Avi | Container::Asf)
    }
}

/// The ASF header object's GUID, as it's stored.
const ASF: [u8; 16] = [0x30, 0x26, 0xB2, 0x75, 0x8E, 0x66, 0xCF, 0x11, 0xA6, 0xD9, 0x00, 0xAA, 0x00, 0x62, 0xCE, 0x6C];

fn extension(name: &str) -> String {
    name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default()
}

pub fn allowed_name(name: &str) -> bool {
    VIDEO_EXTENSIONS.contains(&extension(name).as_str())
}

pub fn subtitle_name(name: &str) -> bool {
    SUBTITLE_EXTENSIONS.contains(&extension(name).as_str())
}

/// A picture format built on the same boxes as MP4 starts with one of these brands, and isn't a film.
const STILLS: [&[u8; 4]; 8] = [b"heic", b"heix", b"hevc", b"heim", b"heis", b"mif1", b"msf1", b"avif"];

pub fn sniff(head: &[u8]) -> Option<Container> {
    if head.len() >= 12 {
        let kind = &head[4..8];
        if kind == b"ftyp" {
            return (!STILLS.iter().any(|b| &head[8..12] == *b)).then_some(Container::Mp4);
        }
        // an old QuickTime file may start straight in with its other boxes
        if [&b"moov"[..], b"mdat", b"free", b"skip", b"wide", b"pnot", b"styp"].contains(&kind) {
            return Some(Container::Mp4);
        }
        if head.starts_with(b"RIFF") && &head[8..12] == b"AVI " {
            return Some(Container::Avi);
        }
    }
    if head.starts_with(&ASF) {
        return Some(Container::Asf);
    }
    if head.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        // the doctype says which: anything that isn't WebM is read as Matroska
        let webm = head[..head.len().min(64)].windows(4).any(|w| w == b"webm");
        return Some(if webm { Container::Webm } else { Container::Matroska });
    }
    None
}

pub fn sniff_file(path: &Path) -> std::io::Result<Option<Container>> {
    let mut head = [0u8; 64];
    let mut f = File::open(path)?;
    let n = f.read(&mut head)?;
    Ok(sniff(&head[..n]))
}
