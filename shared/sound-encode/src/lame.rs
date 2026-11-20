//! MP3 encoding through LAME, which is LGPL: a separate `libmp3lame.dll` beside the exe (`libmp3lame.dylib` on the Mac,
//! beside the program or in its .app's Contents/Frameworks), opened when an MP3 is first saved. Anyone can replace that
//! file with their own build of LAME. Only the one beside the program is ever loaded.
//! The library holds LAME's encoder only; its decoder isn't in this source tree at all. See build.rs.

use libloading::Library;
use std::ffi::{CStr, c_char, c_float, c_int, c_uchar, c_void};
use std::path::PathBuf;
use std::sync::OnceLock;

pub type Handle = *mut c_void;

pub struct Lame {
    _lib: Library,
    pub version: unsafe extern "C" fn() -> *const c_char,
    pub init: unsafe extern "C" fn() -> Handle,
    pub close: unsafe extern "C" fn(Handle) -> c_int,
    pub set_in_rate: unsafe extern "C" fn(Handle, c_int) -> c_int,
    pub set_out_rate: unsafe extern "C" fn(Handle, c_int) -> c_int,
    pub set_channels: unsafe extern "C" fn(Handle, c_int) -> c_int,
    pub set_mode: unsafe extern "C" fn(Handle, c_int) -> c_int,
    pub set_brate: unsafe extern "C" fn(Handle, c_int) -> c_int,
    pub set_vbr: unsafe extern "C" fn(Handle, c_int) -> c_int,
    pub set_quality: unsafe extern "C" fn(Handle, c_int) -> c_int,
    pub set_vbr_tag: unsafe extern "C" fn(Handle, c_int) -> c_int,
    pub init_params: unsafe extern "C" fn(Handle) -> c_int,
    pub get_brate: unsafe extern "C" fn(Handle) -> c_int,
    pub encode: unsafe extern "C" fn(Handle, *const c_float, *const c_float, c_int, *mut c_uchar, c_int) -> c_int,
    pub flush: unsafe extern "C" fn(Handle, *mut c_uchar, c_int) -> c_int,
    pub tag_frame: unsafe extern "C" fn(Handle, *mut c_uchar, usize) -> usize,
}

// SAFETY: the function pointers are plain code, and each encode has its own handle
unsafe impl Sync for Lame {}
unsafe impl Send for Lame {}

/// The library's file name: `libmp3lame.dll` on Windows, `libmp3lame.dylib` on the Mac.
#[cfg(windows)]
pub const FILE: &str = "libmp3lame.dll";
#[cfg(not(windows))]
pub const FILE: &str = "libmp3lame.dylib";

fn beside_exe() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(me) = std::env::current_exe() {
        if let Some(dir) = me.parent() {
            out.push(dir.join(FILE));
            // in a Mac .app the program is in Contents/MacOS and its libraries in Contents/Frameworks
            #[cfg(target_os = "macos")]
            if let Some(contents) = dir.parent() {
                out.push(contents.join("Frameworks").join(FILE));
            }
            // cargo's own tests run from a folder below the one the exe is in
            if dir.file_name().is_some_and(|n| n == "deps") {
                if let Some(up) = dir.parent() {
                    out.push(up.join(FILE));
                }
            }
        }
    }
    out
}

fn load() -> Result<Lame, String> {
    let path = beside_exe().into_iter().find(|p| p.is_file()).ok_or_else(|| format!("{FILE} is missing"))?;
    // SAFETY: an absolute path to the library we ship, and these are LAME's published functions and types
    unsafe {
        let lib = Library::new(&path).map_err(|e| e.to_string())?;
        macro_rules! f {
            ($n:literal) => {
                *lib.get($n).map_err(|e| e.to_string())?
            };
        }
        Ok(Lame {
            version: f!(b"get_lame_version\0"),
            init: f!(b"lame_init\0"),
            close: f!(b"lame_close\0"),
            set_in_rate: f!(b"lame_set_in_samplerate\0"),
            set_out_rate: f!(b"lame_set_out_samplerate\0"),
            set_channels: f!(b"lame_set_num_channels\0"),
            set_mode: f!(b"lame_set_mode\0"),
            set_brate: f!(b"lame_set_brate\0"),
            set_vbr: f!(b"lame_set_VBR\0"),
            set_quality: f!(b"lame_set_quality\0"),
            set_vbr_tag: f!(b"lame_set_bWriteVbrTag\0"),
            init_params: f!(b"lame_init_params\0"),
            get_brate: f!(b"lame_get_brate\0"),
            encode: f!(b"lame_encode_buffer_ieee_float\0"),
            flush: f!(b"lame_encode_flush\0"),
            tag_frame: f!(b"lame_get_lametag_frame\0"),
            _lib: lib,
        })
    }
}

static LOADED: OnceLock<Result<Lame, String>> = OnceLock::new();

pub fn get() -> Result<&'static Lame, String> {
    LOADED.get_or_init(load).as_ref().map_err(Clone::clone)
}

pub fn version() -> Option<String> {
    let l = get().ok()?;
    // SAFETY: LAME returns a static string
    Some(unsafe { CStr::from_ptr((l.version)()) }.to_string_lossy().into_owned())
}

// LAME's own names for these
pub const MODE_JOINT: c_int = 1;
pub const MODE_MONO: c_int = 3;
pub const VBR_OFF: c_int = 0;
