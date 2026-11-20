//! What a save does: copy the file, change only its tags, or encode the sound afresh. And how large that will be.

use crate::atomic::{WriteError, WriteFailure, atomic_produce};
use crate::encode::{Encoder, Quality, Shape, shape_for};
use crate::flac::Out;
use crate::formats::{self, Kind};
use crate::fx::{Recipe, render};
use crate::rate::{Rate, frames_after};
use crate::store::Pcm;
use crate::tags::{self, Edit};
use lofty::tag::Tag;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{BufWriter, Cursor};
use std::path::Path;
use std::sync::Arc;

#[derive(Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub kind: Kind,
    /// Only for MP3 and Vorbis. None means nobody chose one, so nothing needs re-encoding if nothing else does.
    pub quality: Option<Quality>,
    pub recipe: Recipe,
    #[serde(default)]
    pub tags: Edit,
}

#[derive(Serialize, Clone, Copy, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Route {
    /// The same bytes.
    Copy,
    /// The same sound, with new tags.
    Tags,
    /// The sound goes through the encoder again.
    Encode,
}

/// What the file on disk was made from, so a save can tell whether anything has changed since.
#[derive(Clone)]
pub struct OnDisk {
    pub kind: Kind,
    pub recipe: Recipe,
    /// False when the file may no longer be the one this came from.
    pub trusted: bool,
}

pub fn route(disk: &OnDisk, req: &Request) -> Route {
    if disk.trusted && req.kind == disk.kind && req.quality.is_none() && req.recipe.same_sound(&disk.recipe) {
        if req.tags.is_empty() { Route::Copy } else { Route::Tags }
    } else {
        Route::Encode
    }
}

#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    pub route: Route,
    pub size: u64,
    pub notes: Vec<&'static str>,
}

// a few seconds from three places in the song, through the real encoder
const WINDOW_SECONDS: f64 = 3.0;

fn sample_encoded(pcm: &Arc<Pcm>, recipe: &Recipe, kind: Kind, shape: &Shape, q: Quality) -> Option<f64> {
    let total = recipe.frames();
    let rate = pcm.rate as f64;
    let win = ((WINDOW_SECONDS * rate) as u64).min(total).max(1);
    let spans = if total <= win * 3 { vec![0] } else { vec![total / 6, total / 2, total * 5 / 6] };
    let timeline = crate::fx::Timeline::new(pcm.clone(), &recipe.spans);
    let mut bytes = 0u64;
    let mut seconds = 0.0;
    for start in spans {
        let n = win.min(total - start) as usize;
        let mut planar = vec![vec![0f32; n]; pcm.channels];
        let got = timeline.read(start, &mut planar, n);
        if got == 0 {
            continue;
        }
        // the encoder takes its output away with it, so a shared buffer stands in
        let shared = SharedVec::default();
        let mut enc = Encoder::new(kind, Box::new(shared.clone()), shape, q, got as u64).ok()?;
        let mixed = mix_to(&planar, got, shape.channels);
        // a window at another rate is sized by time alone
        enc.write(&mixed, got).ok()?;
        enc.finish().ok()?;
        bytes += shared.len() as u64;
        seconds += got as f64 / rate;
    }
    (seconds > 0.0).then(|| bytes as f64 / seconds)
}

#[derive(Clone, Default)]
struct SharedVec(Arc<std::sync::Mutex<Cursor<Vec<u8>>>>);

impl SharedVec {
    fn len(&self) -> usize {
        self.0.lock().unwrap().get_ref().len()
    }
}
impl std::io::Write for SharedVec {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().write(b)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl std::io::Seek for SharedVec {
    fn seek(&mut self, p: std::io::SeekFrom) -> std::io::Result<u64> {
        self.0.lock().unwrap().seek(p)
    }
}

// more channels than a format holds are folded down evenly
fn mix_to(planar: &[Vec<f32>], n: usize, to: usize) -> Vec<Vec<f32>> {
    let from = planar.len();
    if from <= to {
        return planar.iter().map(|p| p[..n].to_vec()).collect();
    }
    let mut out = vec![vec![0f32; n]; to];
    let k = to as f32 / from as f32;
    for (c, p) in planar.iter().enumerate() {
        let o = &mut out[c % to];
        for f in 0..n {
            o[f] += p[f] * k;
        }
    }
    out
}

// a WAV or AIFF is as large as its samples say: no encoding needed to know
fn plain_size(pcm: &Pcm, req: &Request, shape: &Shape) -> u64 {
    let frames = req.recipe.frames() + req.recipe.tail_frames(pcm.rate);
    (frames_after(frames, pcm.rate, shape.rate) * shape.channels as u64 * (shape.bits as u64 / 8)) + 58
}

/// What a save loses, in the page's words: known from the song and the request alone, without encoding anything, so the
/// card can say it at once (save_notes). plan() says the same, with the size.
pub fn notes(
    pcm: &Arc<Pcm>,
    bits: Option<u32>,
    disk: &OnDisk,
    source_kind: Kind,
    req: &Request,
    cover_len: usize,
) -> Vec<&'static str> {
    let mut notes = Vec::new();
    match route(disk, req) {
        Route::Copy => {
            if req.kind.lossy() {
                notes.push("直接复制音频流，不重新编码，音质零损耗。");
            }
            return notes;
        }
        Route::Tags => {
            notes.push("仅更新曲目元数据，不重新编码音频，音质零损耗。");
            return notes;
        }
        Route::Encode => {}
    }
    let shape = shape_for(req.kind, pcm.rate, pcm.channels, bits);
    let source_lossy = formats::lossy_source(source_kind, bits);
    // the open file can't be written, so whatever is chosen becomes a new file beside it. The page puts the format's
    // own name where this says M4A (MOD, XM, AU...): one sentence for every format that is only read
    if !disk.kind.writable() {
        notes.push("M4A 格式当前仅支持读取，修改后将另存为新文件，原文件保持不变。");
    }
    if req.kind.lossy() && source_lossy {
        notes.push("音频将重新进行有损编码，音质会产生轻微损耗。");
    } else if req.kind.lossy() {
        notes.push("有损压缩会舍弃部分高频细节，原文件不受影响。");
    }
    if !req.kind.lossy() && source_lossy {
        notes.push("转为无损格式会增加文件体积，但无法恢复原文件已流失的音质细节。");
    }
    if shape.rate != pcm.rate {
        notes.push(if req.kind == Kind::Mp3 {
            "MP3 采样率上限为 48 kHz，超出部分将自动重采样。"
        } else {
            "目标格式不支持当前采样率，导出时将自动转换。"
        });
    }
    if shape.channels < pcm.channels {
        notes.push(if req.kind == Kind::Mp3 {
            "MP3 最多支持双声道，多声道音频将缩混为立体声。"
        } else {
            "目标格式的声道数量受限，多余声道将自动缩混。"
        });
    }
    if bits.is_some_and(|b| b > 24) && !req.kind.lossy() {
        notes.push("原音频位深超过 24-bit，导出后将转换为 24-bit。");
    }
    if matches!(req.kind, Kind::Wav | Kind::Aiff)
        && plain_size(pcm, req, &shape) + (cover_len + 128) as u64 > u32::MAX as u64 - 1024
    {
        notes.push("音频长度或体积超出该格式的上限，无法导出。");
    }
    notes
}

pub fn plan(
    pcm: &Arc<Pcm>,
    bits: Option<u32>,
    disk: &OnDisk,
    disk_size: u64,
    source_kind: Kind,
    req: &Request,
    cover_len: usize,
) -> Plan {
    let way = route(disk, req);
    let notes = notes(pcm, bits, disk, source_kind, req, cover_len);
    let extra = (cover_len + 128) as u64;
    if way == Route::Copy {
        return Plan { route: way, size: disk_size, notes };
    }
    if way == Route::Tags {
        return Plan { route: way, size: disk_size + extra, notes };
    }
    let shape = shape_for(req.kind, pcm.rate, pcm.channels, bits);
    let q = req.quality.unwrap_or(Quality::Hi);
    let frames = req.recipe.frames() + req.recipe.tail_frames(pcm.rate);
    let seconds = frames as f64 / pcm.rate as f64;
    let size = match req.kind {
        Kind::Wav | Kind::Aiff => plain_size(pcm, req, &shape),
        Kind::Mp3 => {
            // the slowest MP3 rates top out at 160 kbps and 8 kHz at far less; the window finds out
            let kbps = crate::encode::kbps(Kind::Mp3, q) as f64;
            if shape.rate >= 32000 {
                (kbps * 125.0 * seconds) as u64
            } else {
                sample_encoded(pcm, &req.recipe, req.kind, &shape, q)
                    .map(|r| (r * seconds) as u64)
                    .unwrap_or((kbps * 125.0 * seconds) as u64)
            }
        }
        Kind::Flac | Kind::Ogg => {
            sample_encoded(pcm, &req.recipe, req.kind, &shape, q).map(|r| (r * seconds) as u64).unwrap_or(0)
        }
        Kind::M4a | Kind::Mod | Kind::Xm | Kind::S3m | Kind::It | Kind::Au | Kind::Voc => 0,
    } + extra;
    Plan { route: way, size, notes }
}

fn encode_to(path: &Path, pcm: &Arc<Pcm>, req: &Request, bits: Option<u32>) -> Result<(), String> {
    let shape = shape_for(req.kind, pcm.rate, pcm.channels, bits);
    let q = req.quality.unwrap_or(Quality::Hi);
    let frames = frames_after(req.recipe.frames() + req.recipe.tail_frames(pcm.rate), pcm.rate, shape.rate);
    let file = fs::OpenOptions::new().write(true).read(true).truncate(true).open(path).map_err(|e| e.to_string())?;
    let out: Box<dyn Out> = Box::new(BufWriter::with_capacity(1 << 20, file));
    let mut enc = Encoder::new(req.kind, out, &shape, q, frames)?;
    let mix = pcm.channels > shape.channels;
    if shape.rate == pcm.rate {
        render(pcm, &req.recipe, &mut |p, n| {
            if mix { enc.write(&mix_to(p, n, shape.channels), n) } else { enc.write(p, n) }
        })?;
    } else {
        let mut resampler = Rate::new(pcm.rate, shape.rate, shape.channels)?;
        render(pcm, &req.recipe, &mut |p, n| {
            let mixed;
            let part: &[Vec<f32>] = if mix {
                mixed = mix_to(p, n, shape.channels);
                &mixed
            } else {
                p
            };
            resampler.push(part, n, &mut |o, m| enc.write(o, m))
        })?;
        resampler.finish(&mut |o, m| enc.write(o, m))?;
    }
    enc.finish()
}

pub struct Job<'a> {
    pub pcm: &'a Arc<Pcm>,
    pub bits: Option<u32>,
    pub disk: &'a OnDisk,
    /// The file the window is on, when it still exists.
    pub source: Option<&'a Path>,
    /// The tag the sound came with, carried into anything newly encoded.
    pub carry: Option<&'a Tag>,
    pub cover: Option<&'a [u8]>,
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

fn word_failure(e: String) -> WriteError {
    // the encoder's own output failing is almost always the disk
    if e.contains("too large") {
        return WriteError { kind: WriteFailure::Other, detail: "too large".into() };
    }
    if e.contains("os error 112") || e.contains("os error 39") {
        return WriteError { kind: WriteFailure::DiskFull, detail: e };
    }
    WriteError { kind: WriteFailure::Other, detail: e }
}

/// Writes the file. Whatever goes wrong, what was at `target` before is still there, untouched.
pub fn save(target: &Path, job: &Job, req: &Request) -> Result<Route, WriteError> {
    // M4A, the modules, AU and VOC are only read: never a save's format, and never what a save replaces
    if !req.kind.writable() || (!job.disk.kind.writable() && job.source.is_some_and(|s| same_file(s, target))) {
        return Err(WriteError { kind: WriteFailure::Other, detail: "this format is only ever read".into() });
    }
    let way = route(job.disk, req);
    let source = match way {
        Route::Encode => None,
        _ => Some(
            job.source
                .ok_or_else(|| WriteError { kind: WriteFailure::Missing, detail: "the file has gone".to_string() })?,
        ),
    };
    if way == Route::Copy && source.is_some_and(|s| same_file(s, target)) {
        return Ok(way);
    }
    atomic_produce(target, |temp| {
        match way {
            Route::Copy | Route::Tags => {
                let mut from = File::open(source.unwrap()).map_err(WriteError::from)?;
                let mut to = fs::OpenOptions::new().write(true).truncate(true).open(temp).map_err(WriteError::from)?;
                std::io::copy(&mut from, &mut to).map_err(WriteError::from)?;
                drop(to);
                if way == Route::Tags {
                    tags::write(temp, req.kind, None, &req.tags, job.cover).map_err(word_failure)?;
                }
            }
            Route::Encode => {
                encode_to(temp, job.pcm, req, job.bits).map_err(word_failure)?;
                tags::write(temp, req.kind, job.carry, &req.tags, job.cover).map_err(word_failure)?;
            }
        }
        // the disk failing partway is tested by asking for it
        if std::env::var_os("DOTANY_TEST_FAIL_WRITE").is_some() {
            return Err(WriteError { kind: WriteFailure::DiskFull, detail: "test".into() });
        }
        Ok(())
    })?;
    Ok(way)
}
