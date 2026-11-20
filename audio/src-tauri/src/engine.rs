//! Playing. A thread of its own reads the store, runs the same chain a save uses, changes the rate to the speakers', and keeps
//! a little sound waiting in a ring. The system's audio callback only ever takes from the ring, so nothing it does can stall.

use crate::fx::{Chain, Recipe, Timeline};
use crate::rate::Rate;
use crate::store::Pcm;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rtrb::{Consumer, Producer, RingBuffer};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::{Duration, Instant};

// how much waits in the ring: enough for a busy computer, little enough that a slider feels live
const AHEAD_SECONDS: f64 = 0.06;
const RAMP: u32 = 128;
const IDLE_CLOSE: Duration = Duration::from_secs(3);

pub enum Command {
    Load(Option<Arc<Pcm>>),
    Set(Recipe, f32),
    Play { from: f64, until: Option<f64> },
    Pause,
    Seek(f64),
}

#[derive(Debug, Clone, PartialEq)]
pub enum Note {
    /// Where it's got to, a few times a second while playing.
    At(f64),
    /// It reached the end, and this is where the line goes.
    Ended(f64),
    /// No device to play on.
    NoDevice,
}

#[derive(Default)]
pub struct Stats {
    /// Frames the real device has taken from us, and the loudest of them, for tests.
    pub device_frames: AtomicU64,
    pub device_peak: AtomicU32,
    pub device_rate: AtomicU32,
    pub device_channels: AtomicU32,
}

struct Shared {
    flush_asked: AtomicU64,
    flush_done: AtomicU64,
    played: AtomicU64,
    lost: AtomicBool,
    stats: Arc<Stats>,
}

struct Output {
    _stream: cpal::Stream,
    ring: Producer<f32>,
    rate: u32,
    channels: usize,
}

fn fill<T: cpal::SizedSample + cpal::FromSample<f32>>(
    data: &mut [T],
    channels: usize,
    ring: &mut Consumer<f32>,
    shared: &Shared,
    seen: &mut u64,
    out_left: &mut u32,
    in_left: &mut u32,
) {
    let asked = shared.flush_asked.load(Ordering::Acquire);
    if asked != *seen && *out_left == 0 {
        *out_left = RAMP;
    }
    let mut played = 0u64;
    let mut peak = 0f32;
    for frame in data.chunks_exact_mut(channels) {
        // a jump: what's waiting is let go gently, and what comes next arrives gently
        let gain = if *out_left > 0 {
            let k = *out_left as f32 / RAMP as f32;
            *out_left -= 1;
            if *out_left == 0 {
                let left = ring.slots();
                if let Ok(chunk) = ring.read_chunk(left) {
                    chunk.commit_all();
                }
                *seen = asked;
                shared.played.store(0, Ordering::Release);
                played = 0;
                shared.flush_done.store(asked, Ordering::Release);
                *in_left = RAMP;
            }
            k
        } else if *in_left > 0 {
            *in_left -= 1;
            1.0 - *in_left as f32 / RAMP as f32
        } else {
            1.0
        };
        if ring.slots() < channels {
            for s in frame.iter_mut() {
                *s = T::from_sample(0.0);
            }
            continue;
        }
        for s in frame.iter_mut() {
            let v = ring.pop().unwrap_or(0.0) * gain;
            peak = peak.max(v.abs());
            *s = T::from_sample(v);
        }
        played += 1;
    }
    if played > 0 {
        shared.played.fetch_add(played, Ordering::AcqRel);
        shared.stats.device_frames.fetch_add(played, Ordering::Relaxed);
    }
    let bits = shared.stats.device_peak.load(Ordering::Relaxed);
    if peak > f32::from_bits(bits) {
        shared.stats.device_peak.store(peak.to_bits(), Ordering::Relaxed);
    }
}

fn open_output(shared: &Arc<Shared>) -> Result<Output, String> {
    let host = cpal::default_host();
    let device = host.default_output_device().ok_or("no device")?;
    let config = device.default_output_config().map_err(|e| e.to_string())?;
    let format = config.sample_format();
    let stream_config: cpal::StreamConfig = config.into();
    let (rate, channels) = (stream_config.sample_rate, stream_config.channels as usize);
    let room = ((rate as f64 * 0.5) as usize).max(4096) * channels;
    let (ring, cons) = RingBuffer::<f32>::new(room);
    macro_rules! build {
        ($t:ty) => {{
            let shared_cb = shared.clone();
            let shared_err = shared.clone();
            let mut cons = cons;
            let (mut seen, mut out_left, mut in_left) = (shared.flush_asked.load(Ordering::Acquire), 0u32, 0u32);
            shared.flush_done.store(seen, Ordering::Release);
            device.build_output_stream(
                stream_config.clone(),
                move |data: &mut [$t], _: &cpal::OutputCallbackInfo| {
                    fill(data, channels, &mut cons, &shared_cb, &mut seen, &mut out_left, &mut in_left)
                },
                move |_| shared_err.lost.store(true, Ordering::Release),
                None,
            )
        }};
    }
    let stream = match format {
        cpal::SampleFormat::F32 => build!(f32),
        cpal::SampleFormat::I16 => build!(i16),
        cpal::SampleFormat::I32 => build!(i32),
        cpal::SampleFormat::U16 => build!(u16),
        other => return Err(format!("sample format {other:?}")),
    }
    .map_err(|e| e.to_string())?;
    stream.play().map_err(|e| e.to_string())?;
    shared.stats.device_rate.store(rate, Ordering::Relaxed);
    shared.stats.device_channels.store(channels as u32, Ordering::Relaxed);
    Ok(Output { _stream: stream, ring, rate, channels })
}

struct Song {
    pcm: Arc<Pcm>,
    timeline: Timeline,
    chain: Chain,
    recipe: Recipe,
    k: f32,
    block: Vec<Vec<f32>>,
    rate: Option<Rate>,
    rate_for: u32,
}

struct Worker {
    rx: Receiver<Command>,
    shared: Arc<Shared>,
    notify: Box<dyn Fn(Note) + Send>,
    tap: Option<std::fs::File>,
    song: Option<Song>,
    output: Option<Output>,
    // where the next block starts, and where the frames the device has taken began
    cursor: u64,
    base: f64,
    until: Option<u64>,
    source_on: bool,
    playing: bool,
    tail_left: u64,
    sent: u64,
    // how many frames had been sent when the song's own last frame went
    end_mark: u64,
    idle_since: Option<Instant>,
    last_note: Instant,
    out: Vec<f32>,
}

impl Worker {
    fn frames_end(&self) -> u64 {
        let s = self.song.as_ref().unwrap();
        self.until.unwrap_or(s.timeline.frames).min(s.timeline.frames)
    }

    fn time_now(&self) -> f64 {
        let Some(out) = &self.output else { return self.base };
        self.base + self.shared.played.load(Ordering::Acquire) as f64 / out.rate as f64
    }

    fn flush(&mut self) {
        let want = self.shared.flush_asked.fetch_add(1, Ordering::AcqRel) + 1;
        let start = Instant::now();
        while self.shared.flush_done.load(Ordering::Acquire) != want {
            if start.elapsed() > Duration::from_millis(300) {
                // the device stopped asking: it's gone, start again next time
                self.output = None;
                self.shared.flush_done.store(want, Ordering::Release);
                break;
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        self.shared.played.store(0, Ordering::Release);
        self.sent = 0;
        if let Some(s) = &mut self.song {
            if let Some(r) = &mut s.rate {
                r.reset();
            }
        }
    }

    fn ensure_output(&mut self) -> bool {
        if self.shared.lost.swap(false, Ordering::AcqRel) {
            self.output = None;
        }
        if self.output.is_some() {
            return true;
        }
        match open_output(&self.shared) {
            Ok(o) => {
                self.output = Some(o);
                self.sent = 0;
                self.shared.played.store(0, Ordering::Release);
                true
            }
            Err(_) => {
                (self.notify)(Note::NoDevice);
                self.playing = false;
                self.source_on = false;
                self.tail_left = 0;
                false
            }
        }
    }

    fn start(&mut self, from: f64, until: Option<f64>) {
        let Some(s) = &self.song else { return };
        let rate = s.pcm.rate as f64;
        let total = s.timeline.frames;
        let at = ((from * rate).round() as u64).min(total);
        // from the very end, playing starts again from the beginning
        let at = if at + (rate * 0.01) as u64 >= total { 0 } else { at };
        self.until = until.map(|u| (u * rate).round() as u64);
        if self.output.is_some() {
            self.flush();
        }
        if !self.ensure_output() {
            return;
        }
        self.cursor = at;
        self.base = at as f64 / rate;
        self.source_on = true;
        self.playing = true;
        self.tail_left = 0;
        self.idle_since = None;
    }

    fn stop_source(&mut self) {
        let was = self.time_now();
        self.flush();
        self.base = was;
        self.source_on = false;
        self.playing = false;
        // what's still ringing keeps ringing, as it would in a room
        if let Some(s) = &self.song {
            self.tail_left = if s.chain.has_tail() { s.recipe.tail_frames(s.pcm.rate) } else { 0 };
        }
    }

    fn handle(&mut self, cmd: Command) {
        match cmd {
            Command::Load(pcm) => {
                if self.output.is_some() {
                    self.flush();
                }
                self.source_on = false;
                self.playing = false;
                self.tail_left = 0;
                self.base = 0.0;
                self.cursor = 0;
                self.song = pcm.map(|pcm| {
                    let recipe = Recipe::whole(&pcm);
                    let timeline = Timeline::new(pcm.clone(), &recipe.spans);
                    let mut chain = Chain::new(pcm.channels, pcm.rate);
                    chain.set(&recipe, 1.0, timeline.frames);
                    chain.settle();
                    let block = vec![vec![0f32; chain.block]; pcm.channels];
                    Song { pcm, timeline, chain, recipe, k: 1.0, block, rate: None, rate_for: 0 }
                });
            }
            Command::Set(recipe, k) => {
                if let Some(s) = &mut self.song {
                    if recipe.spans != s.recipe.spans {
                        s.timeline = Timeline::new(s.pcm.clone(), &recipe.spans);
                    }
                    s.chain.set(&recipe, k, s.timeline.frames);
                    s.recipe = recipe;
                    s.k = k;
                }
            }
            Command::Play { from, until } => self.start(from, until),
            Command::Pause => {
                if self.playing {
                    self.stop_source();
                }
            }
            Command::Seek(t) => {
                if self.playing {
                    self.start(t, None);
                } else {
                    self.base = t;
                }
            }
        }
    }

    // sends one block toward the speakers; false when there's nothing more to send
    fn pump(&mut self) -> bool {
        if !self.source_on && self.tail_left == 0 {
            return false;
        }
        if !self.ensure_output() {
            return false;
        }
        let end = if self.source_on { self.frames_end() } else { 0 };
        let Some(s) = &mut self.song else { return false };
        let Some(out) = &mut self.output else { return false };
        let b = s.chain.block;
        let mut live = 0;
        let mut just_ended = false;
        let at = self.cursor;
        let resampling = s.pcm.rate != out.rate;
        if self.source_on {
            let n = ((end.saturating_sub(self.cursor)) as usize).min(b);
            live = s.timeline.read(self.cursor, &mut s.block, n);
            self.cursor += live as u64;
            if self.cursor >= end || live == 0 {
                self.source_on = false;
                just_ended = true;
                // what's still ringing keeps ringing; a rate change is holding a little back, and silence pushes it out
                self.tail_left = if s.chain.has_tail() {
                    s.recipe.tail_frames(s.pcm.rate)
                } else if resampling {
                    b as u64 * 2
                } else {
                    0
                };
            }
        } else {
            self.tail_left = self.tail_left.saturating_sub(b as u64);
        }
        s.chain.run(at, &mut s.block, live);
        let dst_rate = out.rate;
        if s.pcm.rate != dst_rate && (s.rate.is_none() || s.rate_for != dst_rate) {
            s.rate = Rate::new(s.pcm.rate, dst_rate, s.pcm.channels).ok();
            s.rate_for = dst_rate;
        }
        let dst_channels = out.channels;
        let src_channels = s.pcm.channels;
        let produced = &mut self.out;
        produced.clear();
        // past the song's end only the ring-out is left, and it fills the block
        let frames_for_speaker = if resampling || s.chain.has_tail() { b } else { live };
        let mut emit = |p: &[Vec<f32>], n: usize| -> Result<(), String> {
            map_channels(p, n, src_channels, dst_channels, produced);
            Ok(())
        };
        match &mut s.rate {
            Some(r) if s.pcm.rate != dst_rate => {
                let _ = r.push(&s.block, frames_for_speaker, &mut emit);
            }
            _ => {
                let _ = emit(&s.block, frames_for_speaker);
            }
        }
        if let Some(tap) = &mut self.tap {
            use std::io::Write;
            let bytes = unsafe { std::slice::from_raw_parts(produced.as_ptr() as *const u8, produced.len() * 4) };
            let _ = tap.write_all(bytes);
        }
        self.sent += (produced.len() / dst_channels) as u64;
        if just_ended {
            // the frames of this block after the song's last aren't the song; near enough for a line on the screen
            let extra = if !resampling && s.chain.has_tail() { (b - live) as u64 } else { 0 };
            self.end_mark = self.sent.saturating_sub(extra);
        }
        let mut i = 0;
        let mut waiting_since: Option<Instant> = None;
        while i < produced.len() {
            let room = out.ring.slots();
            if room == 0 {
                if self.shared.lost.load(Ordering::Acquire) {
                    break;
                }
                // the device empties the ring as it plays. One that stops without saying so is gone, as flush() decides
                // too: it's marked lost, and the loop starts again on whatever is the default, instead of waiting for ever
                let since = *waiting_since.get_or_insert_with(Instant::now);
                if since.elapsed() > Duration::from_millis(300) {
                    self.shared.lost.store(true, Ordering::Release);
                    break;
                }
                std::thread::sleep(Duration::from_millis(1));
                continue;
            }
            waiting_since = None;
            let take = room.min(produced.len() - i);
            if let Ok(mut chunk) = out.ring.write_chunk_uninit(take) {
                let (a, b) = chunk.as_mut_slices();
                for (slot, v) in a.iter_mut().chain(b.iter_mut()).zip(&produced[i..i + take]) {
                    slot.write(*v);
                }
                unsafe { chunk.commit_all() };
            }
            i += take;
        }
        true
    }

    fn ahead(&self) -> f64 {
        let Some(out) = &self.output else { return 0.0 };
        let played = self.shared.played.load(Ordering::Acquire);
        self.sent.saturating_sub(played) as f64 / out.rate as f64
    }

    fn run(mut self) {
        loop {
            let busy = self.source_on || self.tail_left > 0 || self.playing;
            let cmd = if busy {
                self.rx.recv_timeout(Duration::from_millis(4))
            } else {
                self.rx.recv_timeout(Duration::from_millis(100))
            };
            match cmd {
                Ok(c) => {
                    self.handle(c);
                    while let Ok(c) = self.rx.try_recv() {
                        self.handle(c);
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            if self.shared.lost.load(Ordering::Acquire) && self.playing {
                // the device went away: pick up where it stopped, on whatever is the default now
                let at = self.time_now();
                self.output = None;
                self.shared.lost.store(false, Ordering::Release);
                let until = self.until;
                let rate = self.song.as_ref().map(|s| s.pcm.rate as f64).unwrap_or(1.0);
                self.start(at, until.map(|u| u as f64 / rate));
            }
            while (self.source_on || self.tail_left > 0) && self.ahead() < AHEAD_SECONDS {
                if !self.pump() {
                    break;
                }
            }
            if self.playing {
                let now = Instant::now();
                if !self.source_on && self.shared.played.load(Ordering::Acquire) >= self.end_mark {
                    // the song's last frame has been heard; any ring-out carries on by itself
                    let rate = self.song.as_ref().map(|s| s.pcm.rate as f64).unwrap_or(1.0);
                    let rest = self.until.map(|u| u as f64 / rate).unwrap_or(0.0);
                    self.playing = false;
                    self.base = rest;
                    (self.notify)(Note::Ended(rest));
                } else if now.duration_since(self.last_note) > Duration::from_millis(120) {
                    self.last_note = now;
                    (self.notify)(Note::At(self.time_now()));
                }
            }
            // an open device with nothing to say is let go, so the next song finds whatever is the default then
            if !self.playing && self.tail_left == 0 && self.output.is_some() {
                match self.idle_since {
                    None => self.idle_since = Some(Instant::now()),
                    Some(t) if t.elapsed() > IDLE_CLOSE => {
                        self.output = None;
                        self.idle_since = None;
                    }
                    _ => {}
                }
            }
        }
    }
}

// mono to every speaker; stereo as it is; more channels folded to what the device has
fn map_channels(p: &[Vec<f32>], n: usize, from: usize, to: usize, out: &mut Vec<f32>) {
    out.reserve(n * to);
    for f in 0..n {
        if from == to {
            for c in p.iter().take(from) {
                out.push(c[f]);
            }
        } else if from == 1 {
            for _ in 0..to {
                out.push(p[0][f]);
            }
        } else if to == 1 {
            out.push(p.iter().take(from).map(|c| c[f]).sum::<f32>() / from as f32);
        } else if from < to {
            for i in 0..to {
                out.push(if i < from { p[i][f] } else { 0.0 });
            }
        } else {
            let mut mixed = vec![0f32; to];
            for (i, c) in p.iter().take(from).enumerate() {
                mixed[i % to] += c[f] * (to as f32 / from as f32);
            }
            out.extend_from_slice(&mixed);
        }
    }
}

#[derive(Clone)]
pub struct Engine {
    tx: Sender<Command>,
    pub stats: Arc<Stats>,
}

impl Engine {
    /// `tap`, for tests, gets a copy of everything sent toward the speakers.
    pub fn start(notify: impl Fn(Note) + Send + 'static, tap: Option<std::path::PathBuf>) -> Engine {
        let (tx, rx) = channel();
        let stats = Arc::new(Stats::default());
        let shared = Arc::new(Shared {
            flush_asked: AtomicU64::new(0),
            flush_done: AtomicU64::new(0),
            played: AtomicU64::new(0),
            lost: AtomicBool::new(false),
            stats: stats.clone(),
        });
        let tap = tap.and_then(|p| std::fs::File::create(p).ok());
        std::thread::Builder::new()
            .name("audio".into())
            .spawn(move || {
                Worker {
                    rx,
                    shared,
                    notify: Box::new(notify),
                    tap,
                    song: None,
                    output: None,
                    cursor: 0,
                    base: 0.0,
                    until: None,
                    source_on: false,
                    playing: false,
                    tail_left: 0,
                    sent: 0,
                    end_mark: 0,
                    idle_since: None,
                    last_note: Instant::now(),
                    out: Vec::new(),
                }
                .run()
            })
            .expect("audio thread");
        Engine { tx, stats }
    }

    pub fn send(&self, c: Command) {
        let _ = self.tx.send(c);
    }
}
