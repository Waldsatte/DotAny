//! RAR, LHA, cpio and ar, read only: libarchive's readers for these four, built from vendor/ by build.rs. Nothing here
//! writes any of them. Encrypted RAR data and encrypted RAR headers can't be read by libarchive; such entries are listed
//! as locked and reading them says so.
//!
//! RAR opens by its file name (libarchive seeks in it). The others are handed to libarchive as a stream this side reads,
//! so whatever wraps them (a cpio inside gzip, an RPM's payload) is unwrapped in Rust: libarchive is built with no
//! compression libraries at all.

use std::ffi::c_void;
use std::io::{self, Read};
use std::os::raw::{c_char, c_int};
use std::path::Path;

#[allow(non_camel_case_types)]
type la_ssize_t = i64;

const ARCHIVE_EOF: c_int = 1;
const ARCHIVE_OK: c_int = 0;
const ARCHIVE_WARN: c_int = -20;
const ARCHIVE_FATAL: c_int = -30;
const AE_IFMT: u32 = 0o170000;
const AE_IFDIR: u32 = 0o040000;
const AE_IFREG: u32 = 0o100000;

type ReadCallback = unsafe extern "C" fn(*mut c_void, *mut c_void, *mut *const c_void) -> la_ssize_t;

#[link(name = "dotany_libarchive", kind = "static")]
extern "C" {
    fn archive_read_new() -> *mut c_void;
    fn archive_read_support_format_rar(a: *mut c_void) -> c_int;
    fn archive_read_support_format_rar5(a: *mut c_void) -> c_int;
    fn archive_read_support_format_lha(a: *mut c_void) -> c_int;
    fn archive_read_support_format_cpio(a: *mut c_void) -> c_int;
    fn archive_read_support_format_ar(a: *mut c_void) -> c_int;
    #[cfg(windows)]
    fn archive_read_open_filename_w(a: *mut c_void, name: *const u16, block: usize) -> c_int;
    #[cfg(not(windows))]
    fn archive_read_open_filename(a: *mut c_void, name: *const c_char, block: usize) -> c_int;
    fn archive_read_open(
        a: *mut c_void,
        data: *mut c_void,
        open: Option<unsafe extern "C" fn()>,
        read: Option<ReadCallback>,
        close: Option<unsafe extern "C" fn()>,
    ) -> c_int;
    fn archive_read_next_header(a: *mut c_void, entry: *mut *mut c_void) -> c_int;
    fn archive_read_data(a: *mut c_void, buf: *mut c_void, size: usize) -> la_ssize_t;
    fn archive_read_free(a: *mut c_void) -> c_int;
    fn archive_error_string(a: *mut c_void) -> *const c_char;
    #[cfg(windows)]
    fn archive_entry_pathname_w(e: *mut c_void) -> *const u16;
    fn archive_entry_pathname(e: *mut c_void) -> *const c_char;
    fn archive_entry_pathname_utf8(e: *mut c_void) -> *const c_char;
    fn archive_entry_hardlink(e: *mut c_void) -> *const c_char;
    fn archive_entry_size(e: *mut c_void) -> i64;
    fn archive_entry_mtime(e: *mut c_void) -> i64;
    fn archive_entry_mtime_is_set(e: *mut c_void) -> c_int;
    fn archive_entry_filetype(e: *mut c_void) -> u32;
    fn archive_entry_is_encrypted(e: *mut c_void) -> c_int;
    fn archive_entry_is_data_encrypted(e: *mut c_void) -> c_int;
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Format {
    Rar,
    Lha,
    Cpio,
    Ar,
}

pub struct LaEntry {
    /// a name the archive gives as text (RAR's, which are Unicode)
    pub name: Option<String>,
    /// a name as the archive's bytes, encoding unsaid (LHA, cpio, ar): the page guesses it, as for old zips
    pub raw: Option<Vec<u8>>,
    pub dir: bool,
    pub size: u64,
    pub mtime: Option<i64>,
    pub locked: bool,
    /// a link, a device, a second name for a file already given, or ar's symbol table: listed nowhere, extracted never
    pub special: bool,
}

/// What the stream callback reads from, and the error it last met (libarchive only learns "it failed").
struct Feed {
    from: Box<dyn Read + Send>,
    buf: Vec<u8>,
    err: Option<io::Error>,
}

unsafe extern "C" fn feed_read(_a: *mut c_void, data: *mut c_void, out: *mut *const c_void) -> la_ssize_t {
    let feed = &mut *(data as *mut Feed);
    let got = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        loop {
            match feed.from.read(&mut feed.buf) {
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                r => return r,
            }
        }
    }));
    match got {
        Ok(Ok(n)) => {
            *out = feed.buf.as_ptr() as *const c_void;
            n as la_ssize_t
        }
        Ok(Err(e)) => {
            feed.err = Some(e);
            ARCHIVE_FATAL as la_ssize_t
        }
        Err(_) => {
            feed.err = Some(io::Error::other("reader panicked"));
            ARCHIVE_FATAL as la_ssize_t
        }
    }
}

/// One pass over an archive. Dropping it frees libarchive's reader (and then the stream it was reading).
pub struct LaReader {
    a: *mut c_void,
    format: Format,
    feed: *mut Feed,
}

// the reader is only ever used by one thread at a time, behind the document's lock
unsafe impl Send for LaReader {}

impl Drop for LaReader {
    fn drop(&mut self) {
        unsafe {
            archive_read_free(self.a);
            if !self.feed.is_null() {
                drop(Box::from_raw(self.feed));
            }
        }
    }
}

#[cfg(windows)]
unsafe fn from_wide(p: *const u16) -> Option<String> {
    if p.is_null() {
        return None;
    }
    let mut n = 0;
    while *p.add(n) != 0 {
        n += 1;
    }
    Some(String::from_utf16_lossy(std::slice::from_raw_parts(p, n)))
}

unsafe fn c_bytes(p: *const c_char) -> Option<Vec<u8>> {
    (!p.is_null()).then(|| std::ffi::CStr::from_ptr(p).to_bytes().to_vec())
}

/// The name as text: libarchive's wide form on Windows, its UTF-8 form elsewhere (and as a fallback).
unsafe fn text_name(e: *mut c_void) -> Option<String> {
    wide_name(e).or_else(|| c_bytes(archive_entry_pathname_utf8(e)).map(|b| String::from_utf8_lossy(&b).into_owned()))
}

#[cfg(windows)]
unsafe fn wide_name(e: *mut c_void) -> Option<String> {
    from_wide(archive_entry_pathname_w(e))
}

#[cfg(not(windows))]
unsafe fn wide_name(_e: *mut c_void) -> Option<String> {
    None
}

/// While it lives, this thread's C locale reads and writes UTF-8 (the rest of the program is not touched).
///
/// Off Windows libarchive turns a RAR's Unicode names into the C locale's character set, and a program that never set a
/// locale (this one) is in the plain C locale, where every letter beyond ASCII would come out as "?". In a UTF-8 locale
/// libarchive converts them itself, without iconv.
#[cfg(target_os = "macos")]
struct Utf8Names {
    was: libc::locale_t,
}

#[cfg(target_os = "macos")]
impl Utf8Names {
    fn new() -> Option<Utf8Names> {
        static UTF8: std::sync::OnceLock<usize> = std::sync::OnceLock::new();
        // SAFETY: a locale made once and never freed, so it outlives every thread using it
        let loc = *UTF8.get_or_init(|| unsafe {
            libc::newlocale(libc::LC_CTYPE_MASK, c"UTF-8".as_ptr(), std::ptr::null_mut()) as usize
        }) as libc::locale_t;
        if loc.is_null() {
            return None;
        }
        // SAFETY: a valid locale, set for this thread only; the previous one comes back on drop
        Some(Utf8Names { was: unsafe { libc::uselocale(loc) } })
    }
}

#[cfg(target_os = "macos")]
impl Drop for Utf8Names {
    fn drop(&mut self) {
        // SAFETY: the locale this thread had before
        unsafe { libc::uselocale(self.was) };
    }
}

impl LaReader {
    fn new(format: Format) -> io::Result<LaReader> {
        unsafe {
            let a = archive_read_new();
            if a.is_null() {
                return Err(io::Error::other("libarchive"));
            }
            match format {
                Format::Rar => {
                    archive_read_support_format_rar(a);
                    archive_read_support_format_rar5(a);
                }
                Format::Lha => {
                    archive_read_support_format_lha(a);
                }
                Format::Cpio => {
                    archive_read_support_format_cpio(a);
                }
                Format::Ar => {
                    archive_read_support_format_ar(a);
                }
            }
            Ok(LaReader { a, format, feed: std::ptr::null_mut() })
        }
    }

    /// An archive on disk. RAR is read by name; the rest stream from the file.
    pub fn open(path: &Path, format: Format) -> io::Result<LaReader> {
        if format != Format::Rar {
            return LaReader::stream(Box::new(std::fs::File::open(path)?), format);
        }
        let reader = LaReader::new(format)?;
        #[cfg(windows)]
        let opened = {
            use std::os::windows::ffi::OsStrExt;
            let name: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
            unsafe { archive_read_open_filename_w(reader.a, name.as_ptr(), 1 << 16) }
        };
        #[cfg(not(windows))]
        let opened = {
            use std::os::unix::ffi::OsStrExt;
            let name = std::ffi::CString::new(path.as_os_str().as_bytes()).map_err(|_| io::Error::other("name"))?;
            unsafe { archive_read_open_filename(reader.a, name.as_ptr(), 1 << 16) }
        };
        if opened != ARCHIVE_OK {
            return Err(reader.error());
        }
        Ok(reader)
    }

    /// An archive read from a stream, start to end, never seeking.
    pub fn stream(from: Box<dyn Read + Send>, format: Format) -> io::Result<LaReader> {
        let mut reader = LaReader::new(format)?;
        reader.feed = Box::into_raw(Box::new(Feed { from, buf: vec![0; 1 << 16], err: None }));
        if unsafe { archive_read_open(reader.a, reader.feed as *mut c_void, None, Some(feed_read), None) } != ARCHIVE_OK
        {
            return Err(reader.error());
        }
        Ok(reader)
    }

    fn error(&self) -> io::Error {
        unsafe {
            if !self.feed.is_null() {
                if let Some(e) = (*self.feed).err.take() {
                    return e;
                }
            }
            let s = archive_error_string(self.a);
            let text = if s.is_null() {
                "damaged".to_string()
            } else {
                std::ffi::CStr::from_ptr(s).to_string_lossy().into_owned()
            };
            io::Error::new(io::ErrorKind::InvalidData, text)
        }
    }

    /// The next entry's facts, or None at the end. Its data can be read until the next call.
    pub fn next(&mut self) -> io::Result<Option<LaEntry>> {
        // RAR's names are made and read in UTF-8 (the others keep their bytes as they are, in the C locale)
        #[cfg(target_os = "macos")]
        let _utf8 = if self.format == Format::Rar { Utf8Names::new() } else { None };
        unsafe {
            let mut e: *mut c_void = std::ptr::null_mut();
            let r = archive_read_next_header(self.a, &mut e);
            if r == ARCHIVE_EOF {
                return Ok(None);
            }
            if r != ARCHIVE_OK && r != ARCHIVE_WARN {
                return Err(self.error());
            }
            let kind = archive_entry_filetype(e) & AE_IFMT;
            let (name, raw) = if self.format == Format::Rar {
                let n = text_name(e)
                    .or_else(|| c_bytes(archive_entry_pathname(e)).map(|b| String::from_utf8_lossy(&b).into_owned()));
                (Some(n.unwrap_or_default().replace('\\', "/")), None)
            } else {
                // the bytes as they stand in the archive: libarchive keeps them as they are when no encoding is set
                // (the C locale), and the page reads them the way it reads an old zip's
                match c_bytes(archive_entry_pathname(e)) {
                    Some(b) => (None, Some(b)),
                    None => (text_name(e), None),
                }
            };
            let size = archive_entry_size(e).max(0) as u64;
            let shown = raw
                .as_deref()
                .map(|b| String::from_utf8_lossy(b).into_owned())
                .or_else(|| name.clone())
                .unwrap_or_default();
            // ar's symbol tables ("/", "/SYM64/", "__.SYMDEF"): a linker's index, not a member
            let index = self.format == Format::Ar
                && (shown.trim_matches('/').is_empty()
                    || shown.trim_end_matches('/') == "/SYM64"
                    || shown.starts_with("__.SYMDEF"));
            // cpio gives a hard link's second name with no data: the file is listed once, under the name holding it
            let alias = !archive_entry_hardlink(e).is_null() && size == 0;
            Ok(Some(LaEntry {
                name,
                raw,
                dir: kind == AE_IFDIR,
                size,
                mtime: (archive_entry_mtime_is_set(e) != 0).then(|| archive_entry_mtime(e)),
                locked: archive_entry_is_data_encrypted(e) != 0 || archive_entry_is_encrypted(e) != 0,
                special: kind != AE_IFDIR && kind != AE_IFREG || index || alias,
            }))
        }
    }
}

impl Read for LaReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let n = unsafe { archive_read_data(self.a, buf.as_mut_ptr() as *mut c_void, buf.len()) };
        if n < 0 {
            return Err(self.error());
        }
        Ok(n as usize)
    }
}
