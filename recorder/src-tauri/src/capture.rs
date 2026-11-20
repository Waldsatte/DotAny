//! Recording: the microphone's samples go through a ring to a thread that writes them to the take's file as they come,
//! so whatever was heard is on disk within a few hundredths of a second and survives the program stopping short. The
//! same thread tells the page how loud each bar was, for the line it draws.

use crate::takes::{self, Meta};
use crate::wav;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use rtrb::RingBuffer;
use serde::Serialize;
use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};
use tauri::ipc::Channel;

#[derive(Serialize, Clone)]
pub struct Mic {
    pub id: String,
    pub name: String,
}

/// The inputs the system offers.
pub fn mics() -> Vec<Mic> {
    let host = cpal::default_host();
    let Ok(devices) = host.input_devices() else { return Vec::new() };
    devices.filter_map(|d| Some(Mic { id: d.id().ok()?.to_string(), name: d.to_string() })).collect()
}

/// What the page hears while recording: the new bars since the last message, how loud it is now, and how far along.
#[derive(Serialize, Clone)]
pub struct Live {
    pub frames: u64,
    pub peaks: Vec<f32>,
    pub level: f32,
    /// The microphone went away; what was recorded is kept.
    pub lost: bool,
    /// The disk stopped taking the sound; what reached it is kept, and the take ends.
    pub failed: bool,
}

/// Shared between the page's commands and the writing thread.
pub struct Shared {
    pub frames: AtomicU64,
    pub paused: AtomicBool,
    stop: AtomicBool,
    lost: AtomicBool,
    /// Writing to the disk failed: the take ends there and says it is incomplete.
    failed: AtomicBool,
    /// Samples the microphone gave while the ring was full (the disk fell more than two seconds behind). They are
    /// counted, not hidden: the take goes on, as only a gone microphone ends one, and says it is incomplete.
    pub dropped: AtomicU64,
    /// Seconds from the start, as the take's file counts them.
    pub marks: Mutex<Vec<f64>>,
}

pub struct Recording {
    pub path: PathBuf,
    pub rate: u32,
    pub meta: Meta,
    pub shared: Arc<Shared>,
    thread: Option<JoinHandle<Result<(), String>>>,
}

/// Where the sound comes from: a microphone, or with DOTANY_RECORDER_TEST_TONE set, a known tone made at the same pace,
/// so tests can check every sample that reaches the disk.
enum Source {
    Mic(Option<String>),
    Tone,
}

/// The tone the tests listen for: 440 Hz under a slow swell, one channel, made from the frame number alone.
pub fn tone(n: u64, rate: u32) -> f32 {
    let t = n as f64 / rate as f64;
    let swell = 0.25 + 0.6 * (std::f64::consts::PI * 0.7 * t).sin().abs();
    (0.5 * swell * (2.0 * std::f64::consts::PI * 440.0 * t).sin()) as f32
}

fn open_mic(id: Option<&str>) -> Result<(cpal::Device, cpal::SupportedStreamConfig), String> {
    let host = cpal::default_host();
    let chosen =
        id.and_then(|id| host.input_devices().ok()?.find(|d| d.id().ok().is_some_and(|x| x.to_string() == id)));
    let device = chosen.or_else(|| host.default_input_device()).ok_or("nomic")?;
    let mut config = device.default_input_config().map_err(|_| "nomic".to_string())?;
    // MP3 holds 48 kHz at most, and a voice needs no more: a device set higher is asked for 48 kHz when it can do it
    if config.sample_rate() > 48000 {
        if let Ok(ranges) = device.supported_input_configs() {
            if let Some(r) = ranges
                .filter(|r| {
                    r.sample_format() == config.sample_format()
                        && r.min_sample_rate() <= 48000
                        && r.max_sample_rate() >= 48000
                })
                .next()
            {
                config = r.with_sample_rate(48000);
            }
        }
    }
    Ok((device, config))
}

/// Starts writing a new take at `path`. The file and its JSON exist before the first sample does.
pub fn start(path: PathBuf, name: String, mic: Option<String>, on_data: Channel<Live>) -> Result<Recording, String> {
    let source = if std::env::var_os("DOTANY_RECORDER_TEST_TONE").is_some() { Source::Tone } else { Source::Mic(mic) };
    // ask for the microphone first, so a missing one leaves nothing behind
    let mic_cfg = match &source {
        Source::Mic(id) => Some(open_mic(id.as_deref())?),
        Source::Tone => None,
    };
    let rate = mic_cfg.as_ref().map_or(48000, |(_, c)| c.sample_rate());
    let meta = Meta { name, created: takes::now_ms(), recording: true, ..Default::default() };
    takes::write_meta(&path, &meta)?;
    // only this program writes the take while it records; others may read it (on the Mac every file is open to others
    // that way, there is no sharing to set)
    #[cfg(windows)]
    let file = {
        use std::os::windows::fs::OpenOptionsExt;
        OpenOptions::new().write(true).create_new(true).share_mode(1 /* FILE_SHARE_READ */).open(&path)
    };
    #[cfg(not(windows))]
    let file = OpenOptions::new().write(true).create_new(true).open(&path);
    let file = file.map_err(|e| {
        let _ = std::fs::remove_file(takes::meta_path(&path));
        e.to_string()
    })?;
    let shared = Arc::new(Shared {
        frames: AtomicU64::new(0),
        paused: AtomicBool::new(false),
        stop: AtomicBool::new(false),
        lost: AtomicBool::new(false),
        failed: AtomicBool::new(false),
        dropped: AtomicU64::new(0),
        marks: Mutex::new(Vec::new()),
    });
    let (ready_tx, ready_rx) = std::sync::mpsc::channel::<Result<(), String>>();
    let thread_shared = shared.clone();
    let thread = std::thread::Builder::new()
        .name("recorder".into())
        .spawn(move || run(file, rate, mic_cfg, thread_shared, on_data, ready_tx))
        .map_err(|e| e.to_string())?;
    match ready_rx.recv_timeout(Duration::from_secs(10)) {
        Ok(Ok(())) => Ok(Recording { path, rate, meta, shared, thread: Some(thread) }),
        failed => {
            shared.stop.store(true, Ordering::Release);
            let _ = thread.join();
            let _ = std::fs::remove_file(&path);
            let _ = std::fs::remove_file(takes::meta_path(&path));
            Err(match failed {
                Ok(Err(e)) => e,
                _ => "nomic".into(),
            })
        }
    }
}

/// The writing thread: opens the stream, then every few milliseconds moves what arrived to the file.
fn run(
    file: File,
    rate: u32,
    mic: Option<(cpal::Device, cpal::SupportedStreamConfig)>,
    shared: Arc<Shared>,
    on_data: Channel<Live>,
    ready: std::sync::mpsc::Sender<Result<(), String>>,
) -> Result<(), String> {
    let mut out = BufWriter::with_capacity(1 << 16, file);
    out.write_all(&wav::header(rate, 1, 0)).map_err(|e| e.to_string())?;
    out.flush().map_err(|e| e.to_string())?;
    // two seconds of room between the microphone and the disk
    let (tx, mut rx) = RingBuffer::<f32>::new(rate as usize * 2);
    // the microphone's callback owns the ring's writing end; without one, the test tone does
    let mut tone_tx = None;
    let _stream = match mic {
        Some((device, config)) => {
            let mut tx = tx;
            let channels = config.channels() as usize;
            let format = config.sample_format();
            let stream_config: cpal::StreamConfig = config.into();
            let cb_shared = shared.clone();
            let err_shared = shared.clone();
            macro_rules! build {
                ($t:ty) => {{
                    device.build_input_stream(
                        stream_config,
                        move |data: &[$t], _: &cpal::InputCallbackInfo| {
                            if cb_shared.paused.load(Ordering::Acquire) {
                                return;
                            }
                            // one channel: the voice, whichever side of the microphone it came in on
                            for frame in data.chunks(channels) {
                                let sum: f32 = frame.iter().map(|s| <f32 as cpal::FromSample<$t>>::from_sample_(*s)).sum();
                                if tx.push(sum / channels as f32).is_err() {
                                    cb_shared.dropped.fetch_add(1, Ordering::Relaxed);
                                }
                            }
                        },
                        // a missed buffer or a rerouted default is a glitch, not a lost microphone: only a gone one ends the take
                        move |e: cpal::Error| {
                            use cpal::ErrorKind::*;
                            if matches!(e.kind(), DeviceNotAvailable | HostUnavailable | StreamInvalidated | PermissionDenied) {
                                err_shared.lost.store(true, Ordering::Release);
                            }
                        },
                        None,
                    )
                }};
            }
            let stream = match format {
                cpal::SampleFormat::F32 => build!(f32),
                cpal::SampleFormat::I16 => build!(i16),
                cpal::SampleFormat::I32 => build!(i32),
                cpal::SampleFormat::U16 => build!(u16),
                _ => {
                    let _ = ready.send(Err("nomic".into()));
                    return Err("sample format".into());
                }
            };
            let stream =
                match stream.map_err(|e| e.to_string()).and_then(|s| s.play().map(|_| s).map_err(|e| e.to_string())) {
                    Ok(s) => s,
                    Err(e) => {
                        let _ = ready.send(Err("nomic".into()));
                        return Err(e);
                    }
                };
            Some(stream)
        }
        None => {
            tone_tx = Some(tx);
            None
        }
    };
    let _ = ready.send(Ok(()));

    let per_bar = wav::frames_per_bar(rate);
    let (mut peak, mut in_bar) = (0f32, 0usize);
    let mut bars: Vec<f32> = Vec::new();
    let mut bytes: Vec<u8> = Vec::with_capacity(rate as usize);
    let mut written: u64 = 0;
    let mut level = 0f32;
    let mut last_send = Instant::now();
    let mut last_sync = Instant::now();
    // the test tone keeps the same pace a microphone would, and pauses the same way
    let (mut tone_at, tone_clock, mut tone_paused) = (0u64, Instant::now(), Duration::ZERO);
    let mut paused_since: Option<Instant> = None;
    let mut result = Ok(());
    // tests: a disk that stalls once (the ring fills) or stops taking writes, at a known moment
    let test_ms = |name: &str| std::env::var(name).ok().and_then(|v| v.parse::<u64>().ok()).map(Duration::from_millis);
    let (mut stall, fail_after) =
        (test_ms("DOTANY_RECORDER_TEST_STALL_MS"), test_ms("DOTANY_RECORDER_TEST_FAIL_AFTER_MS"));
    let began = Instant::now();
    loop {
        let stopping = shared.stop.load(Ordering::Acquire);
        if let Some(tx) = tone_tx.as_mut() {
            let paused = shared.paused.load(Ordering::Acquire);
            match (paused, paused_since) {
                (true, None) => paused_since = Some(Instant::now()),
                (false, Some(at)) => {
                    tone_paused += at.elapsed();
                    paused_since = None;
                }
                _ => {}
            }
            if !paused {
                // a microphone doesn't wait for the disk: what doesn't fit in the ring is lost, and counted
                let want = ((tone_clock.elapsed() - tone_paused).as_secs_f64() * rate as f64) as u64;
                while tone_at < want {
                    if tx.push(tone(tone_at, rate)).is_err() {
                        shared.dropped.fetch_add(1, Ordering::Relaxed);
                    }
                    tone_at += 1;
                }
            }
        }
        bytes.clear();
        while let Ok(s) = rx.pop() {
            let v = (s.clamp(-1.0, 1.0) * 32768.0).round().clamp(-32768.0, 32767.0) as i16;
            bytes.extend_from_slice(&v.to_le_bytes());
            let a = s.abs();
            peak = peak.max(a);
            level = level.max(a);
            in_bar += 1;
            if in_bar == per_bar {
                bars.push(peak.min(1.0));
                peak = 0.0;
                in_bar = 0;
            }
        }
        if !bytes.is_empty() {
            // straight to the system: if the program stops short now, what was heard is already in the file
            let wrote = if fail_after.is_some_and(|d| began.elapsed() >= d) {
                Err(std::io::Error::other("test: the disk stopped taking writes"))
            } else {
                out.write_all(&bytes).and_then(|_| out.flush())
            };
            match wrote {
                Ok(()) => {
                    written += bytes.len() as u64;
                    shared.frames.store(written / 2, Ordering::Release);
                }
                // what couldn't be written isn't counted as written, and the take ends: never shown as whole
                Err(e) => {
                    result = Err(e.to_string());
                    shared.failed.store(true, Ordering::Release);
                }
            }
        }
        // and once a second, all the way to the disk, with the header saying how long it is so far
        if last_sync.elapsed() >= Duration::from_secs(1) {
            let _ = keep_header(&mut out, rate, written);
            last_sync = Instant::now();
        }
        let lost = shared.lost.load(Ordering::Acquire);
        let failed = shared.failed.load(Ordering::Acquire);
        if last_send.elapsed() >= Duration::from_millis(50) || stopping || lost || failed {
            let _ = on_data.send(Live { frames: written / 2, peaks: std::mem::take(&mut bars), level, lost, failed });
            level = 0.0;
            last_send = Instant::now();
        }
        if stopping || lost || failed {
            break;
        }
        if stall.is_some_and(|_| began.elapsed() >= Duration::from_millis(600)) {
            std::thread::sleep(stall.take().unwrap());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    drop(_stream);
    // the last part of a bar is a bar too
    if in_bar > 0 {
        let _ = on_data.send(Live {
            frames: written / 2,
            peaks: vec![peak.min(1.0)],
            level: 0.0,
            lost: false,
            failed: false,
        });
    }
    keep_header(&mut out, rate, written).map_err(|e| e.to_string())?;
    result
}

fn keep_header(out: &mut BufWriter<File>, rate: u32, written: u64) -> std::io::Result<()> {
    out.flush()?;
    let f = out.get_mut();
    f.seek(SeekFrom::Start(0))?;
    f.write_all(&wav::header(rate, 1, written))?;
    f.seek(SeekFrom::End(0))?;
    f.sync_data()
}

impl Recording {
    pub fn seconds(&self) -> f64 {
        self.shared.frames.load(Ordering::Acquire) as f64 / self.rate as f64
    }

    /// A mark here, now. The JSON beside the take keeps it at once.
    pub fn mark(&mut self) -> Vec<f64> {
        let at = self.seconds();
        let mut marks = self.shared.marks.lock().unwrap();
        marks.push(at);
        marks.sort_by(|a, b| a.total_cmp(b));
        self.meta.marks = marks.clone();
        let _ = takes::write_meta(&self.path, &self.meta);
        marks.clone()
    }

    /// Stops and closes the file with its final header. The take is said to be whole only when it is: a write that
    /// failed, or sound the ring had to drop, leaves it marked incomplete, with the count.
    pub fn finish(mut self) -> Result<Meta, String> {
        self.shared.stop.store(true, Ordering::Release);
        let done =
            self.thread.take().map(|t| t.join().unwrap_or_else(|_| Err("recording thread".into()))).unwrap_or(Ok(()));
        let dropped = self.shared.dropped.load(Ordering::Acquire);
        self.meta.recording = false;
        self.meta.dropped = dropped;
        self.meta.incomplete = done.is_err() || dropped > 0;
        takes::write_meta(&self.path, &self.meta)?;
        done.map(|_| self.meta.clone())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}
