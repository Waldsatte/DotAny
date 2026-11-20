//! Tracker music, MOD, XM, S3M and IT, played by libopenmpt (BSD-3-Clause, built from vendor/libopenmpt by build.rs).
//! A module is a score and its little instruments; libopenmpt plays it once through, and what it plays is the sound
//! the rest of Audio sees, exactly as if a WAV of it had been opened. The module file is only ever read.
//!
//! libopenmpt reads dozens of formats. Only these four reach it: `formats::judge` lets nothing else through, and
//! the type libopenmpt reports is checked again here. MO3 and the other formats are never played. No MP3, Vorbis or
//! zlib code is built into it (build.rs; audio/scripts/check-deps.mjs fails Audio's build otherwise).

use crate::formats::Kind;
use std::ffi::{CStr, c_char, c_int, c_void};

#[repr(C)]
struct OpenmptModule {
    _private: [u8; 0],
}

#[repr(C)]
struct InitialCtl {
    ctl: *const c_char,
    value: *const c_char,
}

type LogFunc = unsafe extern "C" fn(*const c_char, *mut c_void);
type ErrorFunc = unsafe extern "C" fn(c_int, *mut c_void) -> c_int;

extern "C" {
    fn openmpt_module_create_from_memory2(
        filedata: *const c_void,
        filesize: usize,
        logfunc: Option<LogFunc>,
        loguser: *mut c_void,
        errfunc: Option<ErrorFunc>,
        erruser: *mut c_void,
        error: *mut c_int,
        error_message: *mut *const c_char,
        ctls: *const InitialCtl,
    ) -> *mut OpenmptModule;
    fn openmpt_module_destroy(module: *mut OpenmptModule);
    fn openmpt_module_set_repeat_count(module: *mut OpenmptModule, count: i32) -> c_int;
    fn openmpt_module_get_duration_seconds(module: *mut OpenmptModule) -> f64;
    fn openmpt_module_read_interleaved_float_stereo(
        module: *mut OpenmptModule,
        rate: i32,
        count: usize,
        out: *mut f32,
    ) -> usize;
    fn openmpt_module_get_metadata(module: *mut OpenmptModule, key: *const c_char) -> *const c_char;
    fn openmpt_free_string(s: *const c_char);
    fn openmpt_log_func_silent(message: *const c_char, user: *mut c_void);
    fn openmpt_error_func_ignore(error: c_int, user: *mut c_void) -> c_int;
    fn openmpt_get_library_version() -> u32;
}

/// The rate a module is played at: libopenmpt's own recommendation, and what most sound cards run at.
pub const RATE: u32 = 48000;
/// A song that loops forever back into itself is played once through; one that is simply that long stops here.
pub const MAX_SECONDS: f64 = 30.0 * 60.0;
/// A module is a few kilobytes to a few megabytes. Anything larger isn't one.
pub const MAX_FILE: u64 = 256 << 20;

#[derive(Debug, PartialEq)]
pub enum ModError {
    Damaged,
    /// libopenmpt read it, but as a format we don't open.
    Refused,
    Stopped,
}

pub struct Module {
    raw: *mut OpenmptModule,
    // libopenmpt may read from the bytes until it is destroyed
    _data: Vec<u8>,
}

// one thread at a time owns it
unsafe impl Send for Module {}

impl Drop for Module {
    fn drop(&mut self) {
        unsafe { openmpt_module_destroy(self.raw) };
    }
}

/// The type names libopenmpt gives each of ours. "stk" is the 15-instrument MOD of the earliest trackers.
fn types_for(kind: Kind) -> &'static [&'static str] {
    match kind {
        Kind::Mod => &["mod", "stk"],
        Kind::Xm => &["xm"],
        Kind::S3m => &["s3m"],
        Kind::It => &["it"],
        _ => &[],
    }
}

impl Module {
    pub fn load(data: Vec<u8>, kind: Kind) -> Result<Module, ModError> {
        let mut error: c_int = 0;
        let mut message: *const c_char = std::ptr::null();
        // played once: the song ends where it would loop back
        let end = [InitialCtl { ctl: std::ptr::null(), value: std::ptr::null() }];
        let raw = unsafe {
            openmpt_module_create_from_memory2(
                data.as_ptr() as *const c_void,
                data.len(),
                Some(openmpt_log_func_silent),
                std::ptr::null_mut(),
                Some(openmpt_error_func_ignore),
                std::ptr::null_mut(),
                &mut error,
                &mut message,
                end.as_ptr(),
            )
        };
        if !message.is_null() {
            unsafe { openmpt_free_string(message) };
        }
        if raw.is_null() {
            return Err(ModError::Damaged);
        }
        let m = Module { raw, _data: data };
        if !types_for(kind).contains(&m.meta("type").as_str()) {
            return Err(ModError::Refused);
        }
        unsafe { openmpt_module_set_repeat_count(m.raw, 0) };
        Ok(m)
    }

    pub fn meta(&self, key: &str) -> String {
        let key = std::ffi::CString::new(key).unwrap();
        let s = unsafe { openmpt_module_get_metadata(self.raw, key.as_ptr()) };
        if s.is_null() {
            return String::new();
        }
        let text = unsafe { CStr::from_ptr(s) }.to_string_lossy().trim().to_string();
        unsafe { openmpt_free_string(s) };
        text
    }

    /// How long libopenmpt reckons the song is, once through.
    pub fn seconds(&self) -> f64 {
        unsafe { openmpt_module_get_duration_seconds(self.raw) }
    }

    /// Plays the song through into `push`, stereo at `RATE`, a block at a time. `push` answers false to stop.
    pub fn render(&mut self, mut push: impl FnMut(&[f32]) -> bool) -> Result<(), ModError> {
        const BLOCK: usize = 4096;
        let mut buf = vec![0f32; BLOCK * 2];
        let limit = (MAX_SECONDS * RATE as f64) as u64;
        let mut done = 0u64;
        while done < limit {
            let want = BLOCK.min((limit - done) as usize);
            let got =
                unsafe { openmpt_module_read_interleaved_float_stereo(self.raw, RATE as i32, want, buf.as_mut_ptr()) };
            if got == 0 {
                break;
            }
            if !push(&buf[..got * 2]) {
                return Err(ModError::Stopped);
            }
            done += got as u64;
        }
        Ok(())
    }
}

/// The library's version, for the licence notes and the tests: "0.8.9".
pub fn library_version() -> String {
    let v = unsafe { openmpt_get_library_version() };
    format!("{}.{}.{}", v >> 24, (v >> 16) & 0xFF, v & 0xFFFF)
}
