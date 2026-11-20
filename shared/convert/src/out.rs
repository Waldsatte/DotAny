//! Where outputs go: beside the original, never over anything. Each output is written to a hidden part file in the same
//! folder, and only a finished one is moved to its name, by a move that fails rather than replace (then the next free
//! name is taken). A conversion that stops leaves no part files behind.

use crate::Reason;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static SEQ: AtomicU64 = AtomicU64::new(0);

/// Before a part file's name: Windows hides it by its attribute (`hide`), the Mac by a dot in front.
#[cfg(windows)]
const HIDDEN: &str = "";
#[cfg(not(windows))]
const HIDDEN: &str = ".";

struct Part {
    temp: PathBuf,
    stem: String,
    ext: String,
    /// Written with the output it belongs to (a Markdown file's pictures) and placed like it, but not one of the outputs
    /// `commit` reports: it doesn't go on the recent list or into the count of converted files.
    beside: bool,
}

pub struct Outputs {
    dir: PathBuf,
    stem: String,
    parts: Vec<Part>,
    settled: bool,
}

impl Outputs {
    pub fn new(original: &Path) -> Self {
        let dir = original.parent().map(Path::to_path_buf).unwrap_or_default();
        Outputs { dir, stem: crate::stem_of(original), parts: Vec::new(), settled: false }
    }

    /// The original's name without its extension.
    pub fn stem(&self) -> &str {
        &self.stem
    }

    /// A new output named after the original: `name.ext`.
    pub fn file(&mut self, ext: &str) -> Result<File, Reason> {
        let stem = self.stem.clone();
        self.file_named(&stem, ext)
    }

    /// A new output with a name of its own (a page's picture, `name 3.png`).
    pub fn file_named(&mut self, stem: &str, ext: &str) -> Result<File, Reason> {
        let n = SEQ.fetch_add(1, Ordering::Relaxed);
        let temp = self.dir.join(format!("{HIDDEN}~{}.{}.{}-{n}.dotany-part", short(stem), ext, std::process::id()));
        let f = File::options().write(true).read(true).create_new(true).open(&temp).map_err(|_| Reason::Unwritable)?;
        hide(&temp, true);
        self.parts.push(Part { temp, stem: stem.to_string(), ext: ext.to_string(), beside: false });
        Ok(f)
    }

    /// Bytes made in memory, as one output.
    pub fn write(&mut self, ext: &str, bytes: &[u8]) -> Result<(), Reason> {
        use std::io::Write;
        let mut f = self.file(ext)?;
        f.write_all(bytes).map_err(|_| Reason::Unwritable)?;
        f.flush().map_err(|_| Reason::Unwritable)
    }

    pub fn write_named(&mut self, stem: &str, ext: &str, bytes: &[u8]) -> Result<(), Reason> {
        use std::io::Write;
        let mut f = self.file_named(stem, ext)?;
        f.write_all(bytes).map_err(|_| Reason::Unwritable)?;
        f.flush().map_err(|_| Reason::Unwritable)
    }

    /// A file that goes with an output (a Markdown file's picture): placed by the same rule, removed with the rest if
    /// the conversion stops, but not reported by `commit`.
    pub fn write_beside(&mut self, stem: &str, ext: &str, bytes: &[u8]) -> Result<(), Reason> {
        self.write_named(stem, ext, bytes)?;
        if let Some(p) = self.parts.last_mut() {
            p.beside = true;
        }
        Ok(())
    }

    /// The last output's part file, for a writer that needs a path rather than a file (tags are written into it).
    pub fn last_temp(&self) -> Option<&Path> {
        self.parts.last().map(|p| p.temp.as_path())
    }

    /// Every finished output moves to its free name. Returns the outputs' names, in the order made (not the files that
    /// only go with them, `write_beside`).
    pub fn commit(mut self) -> Vec<PathBuf> {
        self.settled = true;
        let mut done = Vec::new();
        for p in std::mem::take(&mut self.parts) {
            match place(&self.dir, &p) {
                Some(_) if p.beside => {}
                Some(path) => done.push(path),
                None => {
                    let _ = std::fs::remove_file(&p.temp);
                }
            }
        }
        done
    }

    /// Nothing of this file stays.
    pub fn abandon(mut self) {
        self.remove_all();
    }

    fn remove_all(&mut self) {
        self.settled = true;
        for p in self.parts.drain(..) {
            let _ = std::fs::remove_file(&p.temp);
        }
    }
}

impl Drop for Outputs {
    fn drop(&mut self) {
        if !self.settled {
            self.remove_all();
        }
    }
}

/// Part file names stay well short of the path limit; the real name is only used on the final move.
fn short(stem: &str) -> String {
    stem.chars().take(40).collect()
}

/// `name.ext`, else `name (2).ext`, `name (3).ext`, the first one free when the move happens.
fn place(dir: &Path, p: &Part) -> Option<PathBuf> {
    for i in 1..100_000u32 {
        let name = if i == 1 { format!("{}.{}", p.stem, p.ext) } else { format!("{} ({i}).{}", p.stem, p.ext) };
        let target = dir.join(name);
        if target.exists() {
            continue;
        }
        match move_no_replace(&p.temp, &target) {
            Ok(()) => {
                hide(&target, false);
                return Some(target);
            }
            // taken in the meantime by someone else: the next one
            Err(_) if target.exists() => continue,
            Err(_) => return None,
        }
    }
    None
}

#[cfg(windows)]
fn wide(p: &Path) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    // past 260 characters Windows' own calls want the long form, as Rust's file calls already use it
    let long = p.as_os_str().len() > 240 && p.is_absolute() && !p.as_os_str().to_string_lossy().starts_with(r"\\");
    let p = if long { std::ffi::OsString::from(format!(r"\\?\{}", p.display())) } else { p.as_os_str().to_os_string() };
    p.encode_wide().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
fn move_no_replace(from: &Path, to: &Path) -> std::io::Result<()> {
    use windows_sys::Win32::Storage::FileSystem::{MOVEFILE_WRITE_THROUGH, MoveFileExW};
    // SAFETY: two NUL-terminated wide paths; without MOVEFILE_REPLACE_EXISTING the move fails if `to` exists
    let ok = unsafe { MoveFileExW(wide(from).as_ptr(), wide(to).as_ptr(), MOVEFILE_WRITE_THROUGH) };
    if ok == 0 { Err(std::io::Error::last_os_error()) } else { Ok(()) }
}

/// The Mac: the file's bytes all the way onto the disk first (its fsync stops at the drive's cache; F_FULLFSYNC doesn't),
/// then a rename that fails rather than replace. A disk whose file system can't refuse a rename that way (some USB
/// sticks) takes the link-and-remove below, and one that has no links either the plainest move there is.
#[cfg(target_os = "macos")]
fn move_no_replace(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::io::AsRawFd;
    let file = File::open(from)?;
    // SAFETY: an open file's descriptor, for the duration of the call
    if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_FULLFSYNC) } == -1 {
        file.sync_all()?;
    }
    drop(file);
    let c = |p: &Path| std::ffi::CString::new(p.as_os_str().as_bytes()).map_err(std::io::Error::other);
    let (a, b) = (c(from)?, c(to)?);
    // SAFETY: two NUL-terminated paths
    if unsafe { libc::renamex_np(a.as_ptr(), b.as_ptr(), libc::RENAME_EXCL) } == 0 {
        return Ok(());
    }
    let e = std::io::Error::last_os_error();
    if !matches!(e.raw_os_error(), Some(libc::ENOTSUP) | Some(libc::EINVAL)) {
        return Err(e);
    }
    match std::fs::hard_link(from, to) {
        Ok(()) => std::fs::remove_file(from),
        Err(e) if to.exists() => Err(e),
        Err(_) => std::fs::rename(from, to),
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
fn move_no_replace(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::hard_link(from, to)?;
    std::fs::remove_file(from)
}

#[cfg(windows)]
fn hide(p: &Path, on: bool) {
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_HIDDEN, GetFileAttributesW, INVALID_FILE_ATTRIBUTES, SetFileAttributesW,
    };
    let w = wide(p);
    // SAFETY: a NUL-terminated wide path
    unsafe {
        let a = GetFileAttributesW(w.as_ptr());
        if a == INVALID_FILE_ATTRIBUTES {
            return;
        }
        SetFileAttributesW(w.as_ptr(), if on { a | FILE_ATTRIBUTE_HIDDEN } else { a & !FILE_ATTRIBUTE_HIDDEN });
    }
}

/// Elsewhere the dot in front of a part file's name hides it (`HIDDEN`), and the name it is moved to has none.
#[cfg(not(windows))]
fn hide(_: &Path, _: bool) {}
