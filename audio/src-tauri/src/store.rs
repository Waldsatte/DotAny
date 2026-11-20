//! The decoded sound: every sample as a float, side by side, never changed once written. What's been done to it lives in the
//! page's recipe. Up to `KEEP_IN_MEMORY` stays in memory; the rest goes to a temp file deleted with the sound, and is read back
//! through a memory map, so an hour of 96 kHz doesn't take the computer down.
//! While it's being written, the store also keeps what later questions need at once: peaks for the wave, loudness for evening out.

use memmap2::Mmap;
use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub const KEEP_IN_MEMORY: usize = 512 * 1024 * 1024;
static SERIAL: AtomicU64 = AtomicU64::new(0);

// a peak every 256 frames, then every 4096, then every 65536
const FINE: u64 = 256;
const FACTOR: u64 = 16;
const HOP_SECONDS: f64 = 0.1;

enum Data {
    Mem(Vec<f32>),
    Map(Option<Mmap>, PathBuf),
}

pub struct Pcm {
    pub channels: usize,
    pub rate: u32,
    pub frames: u64,
    data: Data,
    peaks: [Vec<f32>; 3],
    // K-weighted energy of each 100 ms: what loudness is measured from
    hops: Vec<f32>,
    hop_frames: u64,
}

impl Drop for Pcm {
    fn drop(&mut self) {
        if let Data::Map(map, path) = &mut self.data {
            map.take();
            let _ = fs::remove_file(path);
        }
    }
}

/// Clears what an earlier run left if it was killed.
pub fn sweep(dir: &Path) {
    if let Ok(list) = fs::read_dir(dir) {
        for e in list.flatten() {
            let _ = fs::remove_file(e.path());
        }
    }
}

// ITU-R BS.1770's weighting, worked out for any sample rate
#[derive(Clone, Copy, Default)]
struct Biquad {
    b: [f64; 3],
    a: [f64; 2],
    z: [f64; 2],
}

impl Biquad {
    #[inline]
    fn run(&mut self, x: f64) -> f64 {
        let y = self.b[0] * x + self.z[0];
        self.z[0] = self.b[1] * x - self.a[0] * y + self.z[1];
        self.z[1] = self.b[2] * x - self.a[1] * y;
        y
    }
}

fn k_filter(rate: u32) -> [Biquad; 2] {
    let pi = std::f64::consts::PI;
    let (f0, g, q) = (1681.974450955533, 3.999843853973347, 0.7071752369554196);
    let k = (pi * f0 / rate as f64).tan();
    let vh = 10f64.powf(g / 20.0);
    let vb = vh.powf(0.4996667741545416);
    let a0 = 1.0 + k / q + k * k;
    let shelf = Biquad {
        b: [(vh + vb * k / q + k * k) / a0, 2.0 * (k * k - vh) / a0, (vh - vb * k / q + k * k) / a0],
        a: [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0],
        z: [0.0; 2],
    };
    let (f0, q) = (38.13547087602444, 0.5003270373238773);
    let k = (pi * f0 / rate as f64).tan();
    let a0 = 1.0 + k / q + k * k;
    let high = Biquad { b: [1.0, -2.0, 1.0], a: [2.0 * (k * k - 1.0) / a0, (1.0 - k / q + k * k) / a0], z: [0.0; 2] };
    [shelf, high]
}

pub struct Builder {
    channels: usize,
    rate: u32,
    frames: u64,
    mem: Vec<f32>,
    file: Option<(BufWriter<File>, PathBuf)>,
    temp_dir: PathBuf,
    peaks: [Vec<f32>; 3],
    open_peak: [f32; 3],
    filters: Vec<[Biquad; 2]>,
    hops: Vec<f32>,
    hop_frames: u64,
    hop_sum: f64,
    hop_fill: u64,
}

impl Builder {
    pub fn new(channels: usize, rate: u32, temp_dir: &Path) -> Self {
        Builder {
            channels,
            rate,
            frames: 0,
            mem: Vec::new(),
            file: None,
            temp_dir: temp_dir.to_path_buf(),
            peaks: [Vec::new(), Vec::new(), Vec::new()],
            open_peak: [0.0; 3],
            filters: vec![k_filter(rate); channels],
            hops: Vec::new(),
            hop_frames: ((rate as f64 * HOP_SECONDS).round() as u64).max(1),
            hop_sum: 0.0,
            hop_fill: 0,
        }
    }

    fn spill(&mut self) -> io::Result<()> {
        fs::create_dir_all(&self.temp_dir)?;
        let path =
            self.temp_dir.join(format!("pcm-{}-{}.tmp", std::process::id(), SERIAL.fetch_add(1, Ordering::Relaxed)));
        let mut w = BufWriter::with_capacity(1 << 20, File::create(&path)?);
        write_floats(&mut w, &self.mem)?;
        self.mem = Vec::new();
        self.file = Some((w, path));
        Ok(())
    }

    /// Takes whole frames of side-by-side samples.
    pub fn push(&mut self, samples: &[f32]) -> io::Result<()> {
        let c = self.channels;
        for frame in samples.chunks_exact(c) {
            let mut peak = 0f32;
            let mut energy = 0f64;
            for (i, &v) in frame.iter().enumerate() {
                peak = peak.max(v.abs());
                let [a, b] = &mut self.filters[i];
                let w = b.run(a.run(v as f64));
                energy += w * w;
            }
            for level in 0..3 {
                if self.open_peak[level] < peak {
                    self.open_peak[level] = peak;
                }
            }
            self.frames += 1;
            let mut span = FINE;
            for level in 0..3 {
                if self.frames % span == 0 {
                    self.peaks[level].push(self.open_peak[level]);
                    self.open_peak[level] = 0.0;
                }
                span *= FACTOR;
            }
            self.hop_sum += energy;
            self.hop_fill += 1;
            if self.hop_fill == self.hop_frames {
                self.hops.push((self.hop_sum / self.hop_frames as f64) as f32);
                self.hop_sum = 0.0;
                self.hop_fill = 0;
            }
        }
        match &mut self.file {
            Some((w, _)) => write_floats(w, samples),
            None => {
                self.mem.extend_from_slice(samples);
                if self.mem.len() * 4 > KEEP_IN_MEMORY {
                    self.spill()?;
                }
                Ok(())
            }
        }
    }

    pub fn frames(&self) -> u64 {
        self.frames
    }

    pub fn fits(&self, channels: usize, rate: u32) -> bool {
        self.channels == channels && self.rate == rate
    }

    pub fn finish(mut self) -> io::Result<Pcm> {
        let mut span = FINE;
        for level in 0..3 {
            if self.frames % span != 0 {
                self.peaks[level].push(self.open_peak[level]);
            }
            span *= FACTOR;
        }
        if self.hop_fill > 0 {
            self.hops.push((self.hop_sum / self.hop_fill as f64) as f32);
        }
        let data = match self.file.take() {
            None => {
                self.mem.shrink_to_fit();
                Data::Mem(std::mem::take(&mut self.mem))
            }
            Some((w, path)) => {
                let file = w.into_inner().map_err(|e| e.into_error())?;
                drop(file);
                let file = File::open(&path)?;
                // Safety: the file is ours, in our own folder, and nothing writes it again
                let map = unsafe { Mmap::map(&file)? };
                Data::Map(Some(map), path)
            }
        };
        Ok(Pcm {
            channels: self.channels,
            rate: self.rate,
            frames: self.frames,
            data,
            peaks: std::mem::take(&mut self.peaks),
            hops: std::mem::take(&mut self.hops),
            hop_frames: self.hop_frames,
        })
    }

    /// Reading was abandoned.
    pub fn discard(mut self) {
        if let Some((w, path)) = self.file.take() {
            drop(w);
            let _ = fs::remove_file(path);
        }
    }
}

fn write_floats(w: &mut impl Write, samples: &[f32]) -> io::Result<()> {
    // Safety: floats have no padding, and this only runs where they're little-endian in memory, as they are in the file
    #[cfg(target_endian = "little")]
    let bytes = unsafe { std::slice::from_raw_parts(samples.as_ptr() as *const u8, samples.len() * 4) };
    w.write_all(bytes)
}

impl Pcm {
    /// Frames side by side, as many as exist of those asked for.
    pub fn frames_at(&self, start: u64, count: usize) -> &[f32] {
        let c = self.channels as u64;
        let (a, z) = (start.min(self.frames) * c, (start + count as u64).min(self.frames) * c);
        match &self.data {
            Data::Mem(v) => &v[a as usize..z as usize],
            Data::Map(map, _) => {
                let bytes = &map.as_ref().unwrap()[(a * 4) as usize..(z * 4) as usize];
                // Safety: the map starts page-aligned and holds only floats, so a multiple of 4 from it is aligned too
                unsafe { std::slice::from_raw_parts(bytes.as_ptr() as *const f32, bytes.len() / 4) }
            }
        }
    }

    pub fn in_memory(&self) -> bool {
        matches!(self.data, Data::Mem(_))
    }

    /// The loudest sample between two frames, exact to the finest level asked.
    pub fn peak(&self, a: u64, z: u64) -> f32 {
        if z <= a {
            return 0.0;
        }
        let want = (z - a) / 2;
        let (mut span, mut level) = (FINE, usize::MAX);
        for l in 0..3 {
            if span <= want {
                level = l;
            }
            span *= FACTOR;
        }
        if level == usize::MAX {
            // too close in for the summaries: read the samples
            let stride = ((z - a) / 180).max(1) as usize * self.channels;
            return self
                .frames_at(a, (z - a) as usize)
                .iter()
                .step_by(1.max(stride / self.channels))
                .fold(0f32, |m, v| m.max(v.abs()));
        }
        let span = FINE * FACTOR.pow(level as u32);
        let list = &self.peaks[level];
        let (i, j) = ((a / span) as usize, (z.div_ceil(span)) as usize);
        list.get(i..j.min(list.len())).map(|s| s.iter().fold(0f32, |m, v| m.max(*v))).unwrap_or(0.0)
    }

    /// The whole of each 400 ms block inside these spans, for the loudness measure.
    pub fn loudness(&self, spans: &[(u64, u64)]) -> Option<f64> {
        // the blocks overlap by three quarters, as the standard asks
        let mut blocks = Vec::new();
        for &(a, z) in spans {
            let (i, j) = ((a / self.hop_frames) as usize, ((z / self.hop_frames) as usize).min(self.hops.len()));
            if j.saturating_sub(i) < 4 {
                if j > i {
                    blocks.push(self.hops[i..j].iter().map(|v| *v as f64).sum::<f64>() / (j - i) as f64);
                }
                continue;
            }
            for k in i..=j - 4 {
                blocks.push(self.hops[k..k + 4].iter().map(|v| *v as f64).sum::<f64>() / 4.0);
            }
        }
        let lufs = |e: f64| -0.691 + 10.0 * e.max(1e-20).log10();
        let loud: Vec<f64> = blocks.into_iter().filter(|e| lufs(*e) > -70.0).collect();
        if loud.is_empty() {
            return None;
        }
        let mean = loud.iter().sum::<f64>() / loud.len() as f64;
        let gate = lufs(mean) - 10.0;
        let kept: Vec<f64> = loud.into_iter().filter(|e| lufs(*e) > gate).collect();
        if kept.is_empty() {
            return Some(lufs(mean));
        }
        Some(lufs(kept.iter().sum::<f64>() / kept.len() as f64))
    }
}
