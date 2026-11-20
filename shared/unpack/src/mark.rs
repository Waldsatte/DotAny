//! Where a file came from, as the system marks a download: Windows' Mark of the Web (the `Zone.Identifier` alternate
//! data stream) and the Mac's `com.apple.quarantine` extended attribute. Explorer, 7-Zip and the Mac's Archive Utility
//! give what they unpack from a marked archive the archive's mark, so SmartScreen warns before a program from it runs,
//! Office opens its documents in Protected View and Gatekeeper checks its programs; whatever is written out of an
//! archive here is marked the same way. An archive without a mark gives none.

use std::path::Path;

/// The mark a file carries, read once and copied as it is onto every file written out of it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Mark(Option<Vec<u8>>);

impl Mark {
    /// The mark `path` carries, if any.
    pub fn of(path: &Path) -> Mark {
        Mark(read(path))
    }

    pub fn is_some(&self) -> bool {
        self.0.is_some()
    }

    /// Gives `file` this mark; nothing when there is none. A disk that cannot hold one (FAT, some network shares) is not
    /// an error: the file stays as it was written.
    pub fn put(&self, file: &Path) {
        if let Some(v) = &self.0 {
            write(file, v);
        }
    }
}

/// A mark is a few lines of text; anything far larger is not one.
#[cfg(any(windows, target_os = "macos"))]
const MOST: usize = 64 << 10;

/// `path:Zone.Identifier`, a stream ordinary file calls open, read and write.
#[cfg(windows)]
fn stream(path: &Path) -> std::ffi::OsString {
    let mut s = path.as_os_str().to_owned();
    s.push(":Zone.Identifier");
    s
}

#[cfg(windows)]
fn read(path: &Path) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut v = Vec::new();
    std::fs::File::open(stream(path)).ok()?.take(MOST as u64 + 1).read_to_end(&mut v).ok()?;
    (v.len() <= MOST).then_some(v)
}

#[cfg(windows)]
fn write(file: &Path, v: &[u8]) {
    let _ = std::fs::write(stream(file), v);
}

#[cfg(target_os = "macos")]
const QUARANTINE: &[u8] = b"com.apple.quarantine\0";

#[cfg(target_os = "macos")]
fn c_path(path: &Path) -> Option<std::ffi::CString> {
    use std::os::unix::ffi::OsStrExt;
    std::ffi::CString::new(path.as_os_str().as_bytes()).ok()
}

#[cfg(target_os = "macos")]
fn read(path: &Path) -> Option<Vec<u8>> {
    let p = c_path(path)?;
    let mut v = vec![0u8; MOST];
    // getxattr(path, name, value, size, position, options): the value's length, or -1 when there is none
    let n = unsafe { libc::getxattr(p.as_ptr(), QUARANTINE.as_ptr().cast(), v.as_mut_ptr().cast(), v.len(), 0, 0) };
    if n < 0 {
        return None;
    }
    v.truncate(n as usize);
    Some(v)
}

#[cfg(target_os = "macos")]
fn write(file: &Path, v: &[u8]) {
    if let Some(p) = c_path(file) {
        let _ = unsafe { libc::setxattr(p.as_ptr(), QUARANTINE.as_ptr().cast(), v.as_ptr().cast(), v.len(), 0, 0) };
    }
}

/// Other systems keep no such mark.
#[cfg(not(any(windows, target_os = "macos")))]
fn read(_: &Path) -> Option<Vec<u8>> {
    None
}

#[cfg(not(any(windows, target_os = "macos")))]
fn write(_: &Path, _: &[u8]) {}

#[cfg(test)]
mod tests {
    use super::*;

    /// A folder of its own for one test, gone when it ends.
    struct Scratch(std::path::PathBuf);
    impl Scratch {
        fn new(name: &str) -> Scratch {
            let d = std::env::temp_dir().join(format!("dotany-unpack-mark-{name}-{}", std::process::id()));
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

    #[cfg(windows)]
    #[test]
    fn the_zone_identifier_stream_is_read_and_copied() {
        let s = Scratch::new("zone");
        let (from, to, plain) = (s.0.join("from.zip"), s.0.join("to.txt"), s.0.join("plain.txt"));
        for f in [&from, &to, &plain] {
            std::fs::write(f, "x").unwrap();
        }
        assert!(!Mark::of(&from).is_some(), "a file with no stream has no mark");
        let zone = b"[ZoneTransfer]\r\nZoneId=3\r\nHostUrl=https://example.com/a.zip\r\n";
        std::fs::write(stream(&from), zone).unwrap();
        let mark = Mark::of(&from);
        assert!(mark.is_some());
        mark.put(&to);
        assert_eq!(std::fs::read(stream(&to)).unwrap(), zone);
        // the file's own data is untouched
        assert_eq!(std::fs::read(&to).unwrap(), b"x");
        Mark::default().put(&plain);
        assert!(std::fs::metadata(stream(&plain)).is_err(), "no mark, no stream");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_quarantine_attribute_is_read_and_copied() {
        let s = Scratch::new("quarantine");
        let (from, to, plain) = (s.0.join("from.zip"), s.0.join("to.txt"), s.0.join("plain.txt"));
        for f in [&from, &to, &plain] {
            std::fs::write(f, "x").unwrap();
        }
        assert!(!Mark::of(&from).is_some(), "a file with no attribute has no mark");
        let value = b"0083;6745a1b2;Safari;1F2E3D4C-0000-0000-0000-000000000000";
        write(&from, value);
        let mark = Mark::of(&from);
        assert_eq!(mark, Mark(Some(value.to_vec())));
        mark.put(&to);
        assert_eq!(read(&to).as_deref(), Some(&value[..]));
        Mark::default().put(&plain);
        assert_eq!(read(&plain), None, "no mark, no attribute");
    }
}
