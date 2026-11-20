//! What's been done to the sound, as a recipe, and the one piece of arithmetic that carries it out.
//! The speakers and every save go through this same code, so what you hear is what you get.

use crate::store::Pcm;
use realfft::num_complex::Complex;
use realfft::{ComplexToReal, RealFftPlanner, RealToComplex};
use serde::Deserialize;
use std::sync::Arc;

#[derive(Clone, Copy, PartialEq, Debug, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Space {
    #[default]
    Off,
    Room,
    Hall,
}

impl Space {
    fn seconds(self) -> f64 {
        match self {
            Space::Off => 0.0,
            Space::Room => 0.7,
            Space::Hall => 2.8,
        }
    }
    fn wet(self) -> f32 {
        match self {
            Space::Off => 0.0,
            Space::Room => 0.80,
            Space::Hall => 0.73,
        }
    }
}

/// `spans` are the pieces of the source that remain, in order, in frames.
#[derive(Clone, PartialEq, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Recipe {
    pub spans: Vec<(u64, u64)>,
    pub db: f32,
    pub level: bool,
    pub fade_in: f64,
    pub fade_out: f64,
    pub space: Space,
    pub wet: f32,
}

impl Recipe {
    pub fn whole(pcm: &Pcm) -> Self {
        Recipe {
            spans: vec![(0, pcm.frames)],
            db: 0.0,
            level: false,
            fade_in: 0.0,
            fade_out: 0.0,
            space: Space::Off,
            wet: 50.0,
        }
    }
    pub fn frames(&self) -> u64 {
        self.spans.iter().map(|(a, z)| z - a).sum()
    }
    /// Does this change the sound at all? How strong a reverb that's off would be doesn't.
    pub fn same_sound(&self, other: &Recipe) -> bool {
        let wet = |r: &Recipe| if r.space == Space::Off { 0.0 } else { r.wet };
        self.spans == other.spans
            && self.db == other.db
            && self.level == other.level
            && self.fade_in == other.fade_in
            && self.fade_out == other.fade_out
            && self.space == other.space
            && wet(self) == wet(other)
    }
    pub fn tail_frames(&self, rate: u32) -> u64 {
        (self.space.seconds() * rate as f64).round() as u64
    }
    pub fn gain(&self, level_k: f32) -> f32 {
        10f32.powf(self.db / 20.0) * if self.level { level_k } else { 1.0 }
    }
}

// how far from an easy listening level the kept parts sit. Never pushed past full scale.
const TARGET_LUFS: f64 = -16.0;

pub fn level_gain(pcm: &Pcm, spans: &[(u64, u64)]) -> f32 {
    let Some(lufs) = pcm.loudness(spans) else { return 1.0 };
    let peak = spans.iter().map(|&(a, z)| pcm.peak(a, z)).fold(0f32, f32::max);
    if peak <= 0.0 {
        return 1.0;
    }
    (10f64.powf((TARGET_LUFS - lufs) / 20.0) as f32).min(0.98 / peak)
}

// ---------- reading through the spans ----------

// a cut's two edges would click where they meet, so each fades over a few milliseconds
const JOIN_SECONDS: f64 = 0.0025;

pub struct Timeline {
    pub pcm: Arc<Pcm>,
    spans: Vec<(u64, u64)>,
    starts: Vec<u64>,
    pub frames: u64,
    join: u64,
}

impl Timeline {
    pub fn new(pcm: Arc<Pcm>, spans: &[(u64, u64)]) -> Self {
        let mut starts = Vec::with_capacity(spans.len());
        let mut at = 0;
        for &(a, z) in spans {
            starts.push(at);
            at += z - a;
        }
        let join = (pcm.rate as f64 * JOIN_SECONDS) as u64;
        Timeline { spans: spans.to_vec(), starts, frames: at, join, pcm }
    }

    /// Fills `out` from timeline position `at`. Returns how many frames there were.
    pub fn read(&self, at: u64, out: &mut [Vec<f32>], n: usize) -> usize {
        let c = self.pcm.channels;
        let mut done = 0;
        let mut pos = at;
        let mut i = self.starts.partition_point(|s| *s <= pos).saturating_sub(1);
        while done < n && i < self.spans.len() {
            let (a, z) = self.spans[i];
            let into = pos - self.starts[i];
            if a + into >= z {
                i += 1;
                continue;
            }
            let take = ((z - a - into) as usize).min(n - done);
            let src = self.pcm.frames_at(a + into, take);
            let got = src.len() / c;
            for ch in 0..c {
                let o = &mut out[ch];
                for f in 0..got {
                    o[done + f] = src[f * c + ch];
                }
            }
            // the fade at an inner edge
            let (len, last) = (z - a, self.spans.len() - 1);
            if self.join > 0 {
                for f in 0..got as u64 {
                    let x = into + f;
                    let mut k = 1.0f32;
                    if i > 0 && x < self.join {
                        k = (x as f32 + 0.5) / self.join as f32;
                    }
                    if i < last && len - 1 - x < self.join {
                        k = k.min(((len - 1 - x) as f32 + 0.5) / self.join as f32);
                    }
                    if k < 1.0 {
                        for ch in out.iter_mut().take(c) {
                            ch[done + f as usize] *= k;
                        }
                    }
                }
            }
            done += got;
            pos += got as u64;
            if got < take {
                break;
            }
            i += 1;
        }
        done
    }

    /// The peak of each bar across a stretch of the timeline, for the wave.
    pub fn bars(&self, from: f64, span: f64, bars: usize) -> Vec<f32> {
        let rate = self.pcm.rate as f64;
        let mut out = vec![0f32; bars];
        let each = span / bars as f64;
        for (b, slot) in out.iter_mut().enumerate() {
            let t = from + b as f64 * each;
            let (x, y) = ((t * rate).floor() as u64, ((t + each) * rate).floor() as u64);
            if y <= x {
                continue;
            }
            let mut i = self.starts.partition_point(|s| *s <= x).saturating_sub(1);
            let mut peak = 0f32;
            while i < self.spans.len() && self.starts[i] < y {
                let (a, z) = self.spans[i];
                let s = self.starts[i];
                let (m, n) = (a + x.saturating_sub(s), (a + (y - s)).min(z));
                if n > m {
                    peak = peak.max(self.pcm.peak(m, n));
                }
                i += 1;
            }
            *slot = peak;
        }
        out
    }
}

// ---------- reverb: a burst of decaying noise, the same noise every time ----------

fn impulse(space: Space, rate: u32, channel: usize) -> Vec<f32> {
    let n = (space.seconds() * rate as f64) as usize;
    let mut out = Vec::with_capacity(n);
    let mut s: u32 = 0x9E37_79B9u32.wrapping_mul(channel as u32 % 2 + 1) ^ (space as u32 + 1);
    for i in 0..n {
        s ^= s << 13;
        s ^= s >> 17;
        s ^= s << 5;
        let v = (s as f64 / u32::MAX as f64) * 2.0 - 1.0;
        out.push((v * (1.0 - i as f64 / n as f64).powf(3.2)) as f32);
    }
    out
}

// the same scale a browser's convolver gives: an impulse of any length is about as loud as any other
fn normal_scale(irs: &[Vec<f32>], rate: u32) -> f32 {
    let n: usize = irs.iter().map(Vec::len).sum();
    let power: f64 = irs.iter().flatten().map(|v| (*v as f64).powi(2)).sum::<f64>() / n.max(1) as f64;
    let rms = power.sqrt().max(1e-9);
    (0.00125 / rms * 44100.0 / rate as f64) as f32
}

struct Convolver {
    block: usize,
    parts: Arc<Vec<Vec<Complex<f32>>>>,
    history: Vec<Vec<Complex<f32>>>,
    at: usize,
    last: Vec<f32>,
    fwd: Arc<dyn RealToComplex<f32>>,
    inv: Arc<dyn ComplexToReal<f32>>,
    time: Vec<f32>,
    freq: Vec<Complex<f32>>,
    sum: Vec<Complex<f32>>,
    scratch_f: Vec<Complex<f32>>,
    scratch_i: Vec<Complex<f32>>,
}

impl Convolver {
    fn new(ir: &[f32], scale: f32, block: usize) -> Self {
        let mut planner = RealFftPlanner::<f32>::new();
        let fwd = planner.plan_fft_forward(block * 2);
        let inv = planner.plan_fft_inverse(block * 2);
        let bins = block + 1;
        let mut time = vec![0.0; block * 2];
        let mut scratch_f = fwd.make_scratch_vec();
        let parts: Vec<Vec<Complex<f32>>> = ir
            .chunks(block)
            .map(|chunk| {
                time.fill(0.0);
                for (t, v) in time.iter_mut().zip(chunk) {
                    *t = *v * scale;
                }
                let mut f = vec![Complex::new(0.0, 0.0); bins];
                let _ = fwd.process_with_scratch(&mut time, &mut f, &mut scratch_f);
                f
            })
            .collect();
        let history = vec![vec![Complex::new(0.0, 0.0); bins]; parts.len()];
        let scratch_i = inv.make_scratch_vec();
        Convolver {
            block,
            parts: Arc::new(parts),
            history,
            at: 0,
            last: vec![0.0; block],
            fwd,
            inv,
            time,
            freq: vec![Complex::new(0.0, 0.0); bins],
            sum: vec![Complex::new(0.0, 0.0); bins],
            scratch_f,
            scratch_i,
        }
    }

    // takes exactly one block, gives one block
    fn run(&mut self, input: &[f32], out: &mut [f32]) {
        let b = self.block;
        self.time[..b].copy_from_slice(&self.last);
        self.time[b..].copy_from_slice(input);
        self.last.copy_from_slice(input);
        let _ = self.fwd.process_with_scratch(&mut self.time, &mut self.freq, &mut self.scratch_f);
        self.history[self.at].copy_from_slice(&self.freq);
        self.sum.fill(Complex::new(0.0, 0.0));
        let n = self.parts.len();
        for k in 0..n {
            let h = &self.parts[k];
            let x = &self.history[(self.at + n - k) % n];
            for ((s, a), c) in self.sum.iter_mut().zip(h).zip(x) {
                *s += a * c;
            }
        }
        self.at = (self.at + 1) % n;
        let _ = self.inv.process_with_scratch(&mut self.sum, &mut self.time, &mut self.scratch_i);
        let norm = 1.0 / (2 * b) as f32;
        for (o, t) in out.iter_mut().zip(&self.time[b..]) {
            *o = t * norm;
        }
    }
}

// ---------- the chain: fades, volume, then the dry sound plus its reverb ----------

pub struct Chain {
    channels: usize,
    rate: u32,
    pub block: usize,
    total: u64,
    fade_in: f64,
    fade_out: f64,
    gain_target: f32,
    gain_now: f32,
    wet_target: f32,
    wet_now: f32,
    space: Space,
    conv: Vec<Convolver>,
    wet_buf: Vec<f32>,
    input_buf: Vec<f32>,
    limit: f32,
    limit_release: f32,
}

// dry plus reverb can pass full scale; the limiter turns the whole frame down just enough, and comes back over about 100 ms
const LIMIT_CEILING: f32 = 0.98;
const LIMIT_RELEASE_SECONDS: f64 = 0.1;

impl Chain {
    pub fn new(channels: usize, rate: u32) -> Self {
        // long impulses at high rates: longer blocks keep the work per second about level
        let block = 1024 * (rate as usize).div_ceil(48000).max(1);
        Chain {
            channels,
            rate,
            block,
            total: 0,
            fade_in: 0.0,
            fade_out: 0.0,
            gain_target: 1.0,
            gain_now: 1.0,
            wet_target: 0.0,
            wet_now: 0.0,
            space: Space::Off,
            conv: Vec::new(),
            wet_buf: vec![0.0; block],
            input_buf: vec![0.0; block],
            limit: 1.0,
            limit_release: (1.0 - (-1.0 / (LIMIT_RELEASE_SECONDS * rate as f64)).exp()) as f32,
        }
    }

    pub fn set(&mut self, recipe: &Recipe, level_k: f32, total: u64) {
        self.total = total;
        self.fade_in = recipe.fade_in;
        self.fade_out = recipe.fade_out;
        self.gain_target = recipe.gain(level_k);
        self.wet_target = recipe.space.wet() * recipe.wet / 50.0;
        if recipe.space != self.space {
            self.space = recipe.space;
            self.limit = 1.0;
            self.conv.clear();
            if recipe.space != Space::Off {
                let irs: Vec<Vec<f32>> = (0..2).map(|c| impulse(recipe.space, self.rate, c)).collect();
                let scale = normal_scale(&irs, self.rate);
                self.conv = (0..self.channels).map(|c| Convolver::new(&irs[c % 2], scale, self.block)).collect();
            }
        }
    }

    /// Starts at the settings just given, rather than easing toward them. For saves.
    pub fn settle(&mut self) {
        self.gain_now = self.gain_target;
        self.wet_now = self.wet_target;
    }

    pub fn has_tail(&self) -> bool {
        !self.conv.is_empty()
    }

    fn envelope(&self, time: f64) -> f32 {
        let total = self.total as f64 / self.rate as f64;
        let mut e = 1.0;
        if self.fade_in > 0.0 && time < self.fade_in {
            e *= time / self.fade_in;
        }
        if self.fade_out > 0.0 && time > total - self.fade_out {
            e *= ((total - time) / self.fade_out).max(0.0);
        }
        e as f32
    }

    /// One block, `at` being where it starts on the timeline. `live` is how many of its frames are really sound.
    pub fn run(&mut self, at: u64, buf: &mut [Vec<f32>], live: usize) {
        let b = self.block;
        let rate = self.rate as f64;
        let fades = self.fade_in > 0.0 || self.fade_out > 0.0;
        // volume eases across the block, so moving the slider doesn't crackle
        let (g0, g1) = (self.gain_now, self.gain_target);
        for (ch, samples) in buf.iter_mut().enumerate().take(self.channels) {
            samples[live..b].fill(0.0);
            for (f, v) in samples[..live].iter_mut().enumerate() {
                let g = g0 + (g1 - g0) * (f as f32 / b as f32);
                let e = if fades { self.envelope((at + f as u64) as f64 / rate) } else { 1.0 };
                *v *= g * e;
            }
            let _ = ch;
        }
        self.gain_now = g1;
        if !self.conv.is_empty() {
            let (w0, w1) = (self.wet_now, self.wet_target);
            for (ch, samples) in buf.iter_mut().enumerate().take(self.channels) {
                self.input_buf.copy_from_slice(&samples[..b]);
                let c = &mut self.conv[ch];
                c.run(&self.input_buf, &mut self.wet_buf);
                for (f, v) in samples[..b].iter_mut().enumerate() {
                    *v += self.wet_buf[f] * (w0 + (w1 - w0) * (f as f32 / b as f32));
                }
            }
            self.wet_now = w1;
            for f in 0..b {
                let peak = buf.iter().take(self.channels).fold(0f32, |m, c| m.max(c[f].abs()));
                self.limit += (1.0 - self.limit) * self.limit_release;
                if peak * self.limit > LIMIT_CEILING {
                    self.limit = LIMIT_CEILING / peak;
                }
                if self.limit < 1.0 {
                    for c in buf.iter_mut().take(self.channels) {
                        c[f] *= self.limit;
                    }
                }
            }
        }
    }
}

/// The whole sound with the recipe applied, a block at a time, ring-out included.
pub fn render(
    pcm: &Arc<Pcm>,
    recipe: &Recipe,
    sink: &mut dyn FnMut(&[Vec<f32>], usize) -> Result<(), String>,
) -> Result<(), String> {
    let timeline = Timeline::new(pcm.clone(), &recipe.spans);
    let mut chain = Chain::new(pcm.channels, pcm.rate);
    let k = if recipe.level { level_gain(pcm, &recipe.spans) } else { 1.0 };
    chain.set(recipe, k, timeline.frames);
    chain.settle();
    let b = chain.block;
    let mut buf = vec![vec![0f32; b]; pcm.channels];
    let mut at = 0u64;
    let total = timeline.frames + recipe.tail_frames(pcm.rate);
    while at < total {
        let live = if at < timeline.frames { timeline.read(at, &mut buf, b) } else { 0 };
        chain.run(at, &mut buf, live);
        let n = ((total - at) as usize).min(b);
        sink(&buf, n)?;
        at += n as u64;
    }
    Ok(())
}
