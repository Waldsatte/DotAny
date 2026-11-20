//! Films on the Mac. On Windows, Media Foundation plays what WebView2 won't (AVI, WMV) and shares each picture with the
//! page. The Mac needs none of that: WKWebView plays MP4, MOV and WebM itself, and what it won't open (an MKV with
//! H.264, HEVC, MPEG-2 or MPEG-4 pictures; an AVI) is repackaged for it as a QuickTime film without decoding
//! (`for_player`, the family's `dotany-repack`), read from the original as it plays: nothing is written to the disk, so
//! there is no temporary file to clean up and none can fill it. WMV isn't played on the Mac at all (the page says so). So
//! no film is ever the system's here: `SystemFilm::open` answers `NoDecoder` and nothing below it can run, since no
//! `SystemFilm` or `Shared` can be made.

use crate::formats::Container;
use dotany_repack::Repacked;
use std::path::Path;
use std::sync::Arc;

/// The film the player gets for `path`: `None` plays the file as it is (MP4, MOV, WebM, an MKV with VP8 or VP9
/// pictures, which WebKit reads itself), or as nothing (a film we can't repackage: the player refuses it and the page
/// says the film can't be played).
pub fn for_player(path: &Path, kind: Container) -> Option<Arc<Repacked>> {
    match kind {
        Container::Matroska if dotany_repack::plays_as_it_is(path) => None,
        Container::Matroska | Container::Avi => dotany_repack::repack(path).ok().map(Arc::new),
        _ => None,
    }
}

#[derive(Debug, PartialEq)]
pub enum SystemError {
    /// Nothing on this Mac plays it yet.
    NoDecoder,
    /// As on Windows: the system couldn't play it, or it has no picture.
    Refused,
}

#[derive(Clone, Copy, Debug, serde::Serialize)]
pub struct SystemFacts {
    pub width: u32,
    pub height: u32,
    pub frame_width: u32,
    pub frame_height: u32,
    pub duration: f64,
    pub audio: bool,
}

impl SystemFacts {
    pub fn surface_len(&self) -> usize {
        0
    }
}

pub enum Command {
    Surface(usize, usize),
    Play,
    Pause,
    Seek(f64, u32),
    Volume(f64),
    Rate(f64),
    Close,
}

/// Has no values: what can't be made.
enum Never {}

pub struct SystemFilm {
    pub facts: SystemFacts,
    never: Never,
}

impl SystemFilm {
    pub fn open(_path: &Path) -> Result<SystemFilm, SystemError> {
        Err(SystemError::NoDecoder)
    }

    pub fn send(&self, _c: Command) {
        match self.never {}
    }

    pub fn close(self) {
        match self.never {}
    }
}

pub struct Shared(Never);

pub fn share(_window: &tauri::WebviewWindow, _len: usize, _gen: u64) -> Option<(Shared, usize)> {
    None
}

pub fn unshare(_window: &tauri::WebviewWindow, shared: Shared) {
    match shared.0 {}
}
