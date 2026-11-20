//! Writing a file so it is always either the old version or the new one, never half of each.
//! Temp file in the same folder, flush to disk, then swap. Any failure leaves the original untouched.
//! How the swap is done on each system is in the platform module.

use crate::platform;
use serde::Serialize;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum WriteFailure {
    ReadOnly,
    Locked,
    DiskFull,
    Denied,
    Missing,
    Other,
}

#[derive(Debug, Clone, Serialize)]
pub struct WriteError {
    pub kind: WriteFailure,
    pub detail: String,
}

fn classify(err: &io::Error) -> WriteError {
    WriteError { kind: platform::write_failure(err), detail: err.to_string() }
}

/// Temp files start with this, so the file watcher can ignore them.
pub const TEMP_PREFIX: &str = ".~dotany-";

pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), WriteError> {
    atomic_write_with(path, bytes, &mut |file, data| file.write_all(data))
}

/// `write` is a parameter so tests can make the disk fail in the middle.
pub fn atomic_write_with(
    path: &Path,
    bytes: &[u8],
    write: &mut dyn FnMut(&mut File, &[u8]) -> io::Result<()>,
) -> Result<(), WriteError> {
    let exists = path.exists();
    // a shortcut link keeps pointing at the real file, and the real file is what changes
    let target: PathBuf = if exists { fs::canonicalize(path).map_err(|e| classify(&e))? } else { path.to_path_buf() };
    if exists && platform::is_read_only(&target) {
        return Err(WriteError { kind: WriteFailure::ReadOnly, detail: "file is marked read-only".into() });
    }
    let dir = target.parent().ok_or(WriteError { kind: WriteFailure::Missing, detail: "no folder".into() })?;
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let temp = dir.join(format!("{TEMP_PREFIX}{}-{n}.tmp", std::process::id()));
    let mut file = platform::create_temp(&temp).map_err(|e| classify(&e))?;
    let written = write(&mut file, bytes).and_then(|_| file.flush()).and_then(|_| file.sync_all());
    // the Mac's sync leaves the bytes in the disk's own cache: they go down for good before the name changes
    #[cfg(target_os = "macos")]
    if written.is_ok() {
        platform::full_sync(&file);
    }
    drop(file);
    if let Err(e) = written {
        let _ = fs::remove_file(&temp);
        return Err(classify(&e));
    }
    let swapped = if exists { platform::replace_file(&target, &temp) } else { platform::place_file(&target, &temp) };
    if let Err(e) = swapped {
        let _ = fs::remove_file(&temp);
        return Err(classify(&e));
    }
    Ok(())
}

/// The same promise for a file too large to hold in memory: `produce` fills the temp file at the path it's given
/// and closes it. If it fails, the temp file goes and the original is as it was.
pub fn atomic_produce(path: &Path, produce: impl FnOnce(&Path) -> Result<(), WriteError>) -> Result<(), WriteError> {
    let exists = path.exists();
    let target: PathBuf = if exists { fs::canonicalize(path).map_err(|e| classify(&e))? } else { path.to_path_buf() };
    if exists && platform::is_read_only(&target) {
        return Err(WriteError { kind: WriteFailure::ReadOnly, detail: "file is marked read-only".into() });
    }
    let dir = target.parent().ok_or(WriteError { kind: WriteFailure::Missing, detail: "no folder".into() })?;
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let temp = dir.join(format!("{TEMP_PREFIX}{}-{n}.tmp", std::process::id()));
    drop(platform::create_temp(&temp).map_err(|e| classify(&e))?);
    let done = produce(&temp).and_then(|_| {
        let file = fs::OpenOptions::new().write(true).open(&temp).map_err(|e| classify(&e))?;
        file.sync_all().map_err(|e| classify(&e))?;
        #[cfg(target_os = "macos")]
        platform::full_sync(&file);
        Ok(())
    });
    if let Err(e) = done {
        let _ = fs::remove_file(&temp);
        return Err(e);
    }
    let swapped = if exists { platform::replace_file(&target, &temp) } else { platform::place_file(&target, &temp) };
    if let Err(e) = swapped {
        let _ = fs::remove_file(&temp);
        return Err(classify(&e));
    }
    Ok(())
}

impl From<io::Error> for WriteError {
    fn from(e: io::Error) -> Self {
        classify(&e)
    }
}

impl From<String> for WriteError {
    fn from(detail: String) -> Self {
        WriteError { kind: WriteFailure::Other, detail }
    }
}
