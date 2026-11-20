//! The films and sounds in a slide that the WebView won't play, on the Mac: none are played through this path. On
//! Windows, Media Foundation plays them (`media_foundation.rs`) and shares each picture with the page. The Mac's way is to
//! repackage AVI for WKWebView (`mac::repack`) and not to play WMV or WMA. Every such film or sound is answered
//! `NoDecoder` here, so the page says it can't be played; nothing below `open` can ever run, since no `SystemFilm` or
//! `Shared` can be made.
//!
//! The same items as `media_foundation.rs`, so `app.rs` reads the same on both systems; most of them are never made here.
#![allow(dead_code)]

use std::path::Path;

#[derive(Debug, PartialEq)]
pub enum SystemError {
    /// Nothing on this Mac plays it this way.
    NoDecoder,
    /// As on Windows: the system couldn't play it.
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
