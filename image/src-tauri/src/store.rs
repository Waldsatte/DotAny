//! The frames of a moving picture, kept small. Each frame is packed on its own; once they fill the memory allowance the
//! rest go to a temp file, so a huge GIF can't take the whole computer's memory. The file is deleted with the picture.

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};

/// How much packed frame data stays in memory.
pub const MEMORY_ALLOWANCE: usize = 256 << 20;
static SEQ: AtomicU64 = AtomicU64::new(0);

enum Where {
    Memory(Arc<Vec<u8>>),
    Disk { at: u64, len: u32 },
}

struct Frame {
    delay_ms: u32,
    place: Where,
}

#[derive(Default)]
struct Inner {
    frames: Vec<Frame>,
    in_memory: usize,
    spill: Option<File>,
    spill_len: u64,
    spill_path: Option<PathBuf>,
    done: bool,
    failed: bool,
}

pub struct FrameStore {
    pub w: u32,
    pub h: u32,
    pub loops: u16,
    pub alpha: AtomicBool,
    pub stop: AtomicBool,
    dir: PathBuf,
    inner: Mutex<Inner>,
    changed: Condvar,
    allowance: usize,
}

impl FrameStore {
    pub fn new(w: u32, h: u32, loops: u16, dir: &Path) -> Arc<FrameStore> {
        Self::with_allowance(w, h, loops, dir, MEMORY_ALLOWANCE)
    }

    pub fn with_allowance(w: u32, h: u32, loops: u16, dir: &Path, allowance: usize) -> Arc<FrameStore> {
        Arc::new(FrameStore {
            w,
            h,
            loops,
            alpha: AtomicBool::new(false),
            stop: AtomicBool::new(false),
            dir: dir.to_path_buf(),
            inner: Mutex::default(),
            changed: Condvar::new(),
            allowance,
        })
    }

    pub fn push(&self, rgba: &[u8], delay_ms: u32) -> std::io::Result<()> {
        if !self.alpha.load(Ordering::Relaxed) && crate::ops::has_alpha(rgba) {
            self.alpha.store(true, Ordering::Relaxed);
        }
        let packed = lz4_flex::compress_prepend_size(rgba);
        let mut g = self.inner.lock().unwrap();
        let place = if g.in_memory + packed.len() <= self.allowance {
            g.in_memory += packed.len();
            Where::Memory(Arc::new(packed))
        } else {
            if g.spill.is_none() {
                let _ = fs::create_dir_all(&self.dir);
                let n = SEQ.fetch_add(1, Ordering::Relaxed);
                let path = self.dir.join(format!("frames-{}-{n}.tmp", std::process::id()));
                g.spill = Some(OpenOptions::new().read(true).write(true).create_new(true).open(&path)?);
                g.spill_path = Some(path);
            }
            let at = g.spill_len;
            let f = g.spill.as_mut().unwrap();
            f.seek(SeekFrom::Start(at))?;
            f.write_all(&packed)?;
            g.spill_len += packed.len() as u64;
            Where::Disk { at, len: packed.len() as u32 }
        };
        g.frames.push(Frame { delay_ms, place });
        drop(g);
        self.changed.notify_all();
        Ok(())
    }

    pub fn finish(&self, failed: bool) {
        let mut g = self.inner.lock().unwrap();
        g.done = true;
        g.failed = failed;
        drop(g);
        self.changed.notify_all();
    }

    pub fn count(&self) -> usize {
        self.inner.lock().unwrap().frames.len()
    }

    pub fn delays(&self) -> Vec<u32> {
        self.inner.lock().unwrap().frames.iter().map(|f| f.delay_ms).collect()
    }

    pub fn done(&self) -> bool {
        self.inner.lock().unwrap().done
    }

    /// Blocks until every frame is in, and says how many.
    pub fn wait_all(&self) -> usize {
        let mut g = self.inner.lock().unwrap();
        while !g.done {
            g = self.changed.wait(g).unwrap();
        }
        g.frames.len()
    }

    /// Blocks until this frame exists, or there will never be one.
    pub fn wait_for(&self, i: usize) -> bool {
        let mut g = self.inner.lock().unwrap();
        while g.frames.len() <= i && !g.done {
            g = self.changed.wait(g).unwrap();
        }
        g.frames.len() > i
    }

    pub fn get(&self, i: usize) -> Option<Vec<u8>> {
        let mut g = self.inner.lock().unwrap();
        let (mem, disk) = match &g.frames.get(i)?.place {
            Where::Memory(data) => (Some(data.clone()), None),
            Where::Disk { at, len } => (None, Some((*at, *len))),
        };
        let packed = match (mem, disk) {
            (Some(d), _) => {
                drop(g);
                d
            }
            (_, Some((at, len))) => {
                let f = g.spill.as_mut()?;
                f.seek(SeekFrom::Start(at)).ok()?;
                let mut buf = vec![0u8; len as usize];
                f.read_exact(&mut buf).ok()?;
                drop(g);
                Arc::new(buf)
            }
            _ => return None,
        };
        lz4_flex::decompress_size_prepended(&packed).ok()
    }

    pub fn memory_used(&self) -> usize {
        self.inner.lock().unwrap().in_memory
    }

    pub fn spilled(&self) -> u64 {
        self.inner.lock().unwrap().spill_len
    }
}

impl Drop for FrameStore {
    fn drop(&mut self) {
        let g = self.inner.get_mut().unwrap();
        g.spill = None;
        if let Some(p) = g.spill_path.take() {
            let _ = fs::remove_file(p);
        }
    }
}

/// Left over from a crash: nothing else uses these.
pub fn sweep(dir: &Path) {
    if let Ok(list) = fs::read_dir(dir) {
        for e in list.flatten() {
            if e.file_name().to_string_lossy().starts_with("frames-") {
                let _ = fs::remove_file(e.path());
            }
        }
    }
}
