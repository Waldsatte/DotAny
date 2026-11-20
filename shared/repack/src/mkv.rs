//! An MKV's frames, read for the new film: the tracks' setup, then every block of the first picture track and the
//! default sound track (the first one when none is marked), without reading what's in them (but for a sound frame's first bytes, where its length in time is written).

use crate::codec;
use crate::{Film, Part, Refusal, Sample, Track};
use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

const EBML: u32 = 0x1A45_DFA3;
const SEGMENT: u32 = 0x1853_8067;
const INFO: u32 = 0x1549_A966;
const TRACKS: u32 = 0x1654_AE6B;
const CLUSTER: u32 = 0x1F43_B675;
const LEVEL_ONE: [u32; 8] = [0x114D_9B74, INFO, TRACKS, 0x1043_A770, CLUSTER, 0x1C53_BB6B, 0x1941_A469, 0x1254_C367];
/// A single element we read whole is never larger than this: a tracks list, a block group's head.
const LARGEST: u64 = 64 << 20;

struct R<'a> {
    r: BufReader<&'a mut File>,
    pos: u64,
    len: u64,
}

impl R<'_> {
    fn exact(&mut self, b: &mut [u8]) -> io::Result<()> {
        self.r.read_exact(b)?;
        self.pos += b.len() as u64;
        Ok(())
    }
    fn byte(&mut self) -> io::Result<u8> {
        let mut b = [0];
        self.exact(&mut b)?;
        Ok(b[0])
    }
    fn to(&mut self, at: u64) -> io::Result<()> {
        let by = at as i64 - self.pos as i64;
        self.r.seek_relative(by)?;
        self.pos = at;
        Ok(())
    }
    fn id(&mut self) -> io::Result<u32> {
        let first = self.byte()?;
        let n = first.leading_zeros() + 1;
        if n > 4 {
            return Err(io::ErrorKind::InvalidData.into());
        }
        let mut v = first as u32;
        for _ in 1..n {
            v = v << 8 | self.byte()? as u32;
        }
        Ok(v)
    }
    /// A size: `None` is "unknown" (a live recording's).
    fn size(&mut self) -> io::Result<Option<u64>> {
        let first = self.byte()?;
        let n = first.leading_zeros() + 1;
        if n > 8 {
            return Err(io::ErrorKind::InvalidData.into());
        }
        let mut v = (first as u64) & (0xFF >> n);
        let mut all = v == (0xFF >> n) as u64;
        for _ in 1..n {
            let b = self.byte()?;
            all &= b == 0xFF;
            v = v << 8 | b as u64;
        }
        Ok(if all { None } else { Some(v) })
    }
    fn body(&mut self, size: u64) -> io::Result<Vec<u8>> {
        if size > LARGEST || self.pos + size > self.len {
            return Err(io::ErrorKind::InvalidData.into());
        }
        let mut b = vec![0; size as usize];
        self.exact(&mut b)?;
        Ok(b)
    }
}

fn vint(b: &[u8]) -> Option<(u64, usize)> {
    let first = *b.first()?;
    let n = first.leading_zeros() as usize + 1;
    if n > 8 || b.len() < n {
        return None;
    }
    let mut v = (first as u64) & (0xFF >> n);
    for x in &b[1..n] {
        v = v << 8 | *x as u64;
    }
    Some((v, n))
}

/// The elements in a body we hold whole.
fn children(b: &[u8]) -> Vec<(u32, &[u8])> {
    let mut out = Vec::new();
    let mut at = 0;
    while at < b.len() {
        let Some(first) = b.get(at) else { break };
        let n = first.leading_zeros() as usize + 1;
        if n > 4 || at + n > b.len() {
            break;
        }
        let id = b[at..at + n].iter().fold(0u32, |v, x| v << 8 | *x as u32);
        let Some((size, m)) = vint(&b[at + n..]) else { break };
        let start = at + n + m;
        let end = start.saturating_add(size as usize).min(b.len());
        out.push((id, &b[start..end]));
        at = end;
    }
    out
}

fn uint(b: &[u8]) -> u64 {
    b.iter().take(8).fold(0, |v, x| v << 8 | *x as u64)
}

fn float(b: &[u8]) -> f64 {
    match b.len() {
        4 => f32::from_be_bytes(b.try_into().unwrap()) as f64,
        8 => f64::from_be_bytes(b.try_into().unwrap()),
        _ => 0.0,
    }
}

fn get<'a>(kids: &[(u32, &'a [u8])], id: u32) -> Option<&'a [u8]> {
    kids.iter().find(|(i, _)| *i == id).map(|(_, b)| *b)
}

struct Entry {
    number: u64,
    kind: u64,
    codec: String,
    private: Vec<u8>,
    /// Nanoseconds.
    default_duration: u64,
    width: u32,
    height: u32,
    shown: (u32, u32),
    rate: f64,
    channels: u32,
    bits: u32,
    encoded: bool,
    default: bool,
}

fn entries(tracks: &[u8]) -> Vec<Entry> {
    let mut out = Vec::new();
    for (id, e) in children(tracks) {
        if id != 0xAE {
            continue;
        }
        let k = children(e);
        let mut t = Entry {
            number: get(&k, 0xD7).map_or(0, uint),
            kind: get(&k, 0x83).map_or(0, uint),
            codec: get(&k, 0x86)
                .map(|b| String::from_utf8_lossy(b).trim_end_matches('\0').to_string())
                .unwrap_or_default(),
            private: get(&k, 0x63A2).map(<[u8]>::to_vec).unwrap_or_default(),
            default_duration: get(&k, 0x23_E383).map_or(0, uint),
            width: 0,
            height: 0,
            shown: (0, 0),
            rate: 8000.0,
            channels: 1,
            bits: 0,
            encoded: get(&k, 0x6D80).is_some(),
            default: get(&k, 0x88).is_none_or(|b| uint(b) != 0),
        };
        if let Some(v) = get(&k, 0xE0) {
            let v = children(v);
            t.width = get(&v, 0xB0).map_or(0, uint) as u32;
            t.height = get(&v, 0xBA).map_or(0, uint) as u32;
            let unit = get(&v, 0x54B2).map_or(0, uint);
            let (dw, dh) = (get(&v, 0x54B0).map_or(0, uint) as u32, get(&v, 0x54BA).map_or(0, uint) as u32);
            t.shown = if unit == 0 && dw > 0 && dh > 0 { (dw, dh) } else { (t.width, t.height) };
        }
        if let Some(a) = get(&k, 0xE1) {
            let a = children(a);
            t.rate = get(&a, 0xB5).map_or(8000.0, float);
            t.channels = get(&a, 0x9F).map_or(1, uint) as u32;
            t.bits = get(&a, 0x6264).map_or(0, uint) as u32;
        }
        out.push(t);
    }
    out
}

/// What a picture track becomes, if the Mac plays it.
fn picture_entry(t: &Entry) -> Option<codec::Entry> {
    let v = |kind: &[u8; 4], extra: &[u8]| codec::visual(kind, t.width, t.height, t.shown, extra);
    match t.codec.as_str() {
        "V_MPEG4/ISO/AVC" if !t.private.is_empty() => Some(v(b"avc1", &crate::mp4::bx(b"avcC", &t.private))),
        "V_MPEGH/ISO/HEVC" if !t.private.is_empty() => Some(v(b"hvc1", &crate::mp4::bx(b"hvcC", &t.private))),
        "V_MPEG4/ISO/ASP" | "V_MPEG4/ISO/SP" | "V_MPEG4/ISO/AP" => {
            Some(v(b"mp4v", &codec::esds(0x20, true, &t.private)))
        }
        "V_MPEG2" => Some(v(b"mp4v", &codec::esds(0x61, true, &t.private))),
        "V_MPEG1" => Some(v(b"mp4v", &codec::esds(0x6A, true, &t.private))),
        "V_MJPEG" => Some(v(b"jpeg", &[])),
        _ => None,
    }
}

/// The sound codecs the Mac plays in the new film. The entry is made once the first frame has been seen.
fn sound_playable(t: &Entry) -> bool {
    let c = t.codec.as_str();
    c.starts_with("A_AAC")
        || matches!(c, "A_AC3" | "A_EAC3" | "A_MPEG/L3" | "A_MPEG/L2" | "A_OPUS" | "A_FLAC")
        || (matches!(c, "A_PCM/INT/LIT" | "A_PCM/INT/BIG") && t.bits == 16)
}

pub(crate) fn picture_codec(path: &Path) -> Option<String> {
    let mut f = File::open(path).ok()?;
    let len = f.metadata().ok()?.len();
    let mut r = R { r: BufReader::with_capacity(64 * 1024, &mut f), pos: 0, len };
    let tracks = head(&mut r).ok()?.1?;
    entries(&tracks).into_iter().find(|t| t.kind == 1).map(|t| t.codec)
}

/// Reads up to the first cluster: the time scale and the tracks; answers where the segment ends and where the
/// clusters begin.
fn head(r: &mut R) -> io::Result<((u64, u64, u64), Option<Vec<u8>>)> {
    let bad = || io::Error::from(io::ErrorKind::InvalidData);
    if r.id()? != EBML {
        return Err(bad());
    }
    let size = r.size()?.ok_or_else(bad)?;
    r.to(r.pos + size)?;
    if r.id()? != SEGMENT {
        return Err(bad());
    }
    let end = r.size()?.map_or(r.len, |s| (r.pos + s).min(r.len));
    let (mut scale, mut tracks) = (1_000_000u64, None);
    while r.pos < end {
        let at = r.pos;
        let id = r.id()?;
        let size = r.size()?;
        if id == CLUSTER {
            r.to(at)?;
            return Ok(((scale, at, end), tracks));
        }
        let size = size.ok_or_else(bad)?;
        match id {
            INFO => {
                let b = r.body(size)?;
                scale = get(&children(&b), 0x2A_D7B1).map_or(1_000_000, uint).max(1);
            }
            TRACKS => tracks = Some(r.body(size)?),
            _ => r.to(r.pos + size)?,
        }
    }
    Ok(((scale, end, end), tracks))
}

/// One block of a track we keep.
struct Block {
    /// Nanoseconds from the start.
    ns: i64,
    key: bool,
    /// Its frames: where each is in the file, and how long.
    frames: Vec<(u64, u64)>,
}

/// The frames of a block whose body (after track, time and flags) starts at `at`.
fn lace(r: &mut R, flags: u8, at: u64, end: u64) -> io::Result<Vec<(u64, u64)>> {
    let kind = flags >> 1 & 3;
    if kind == 0 {
        return Ok(vec![(at, end.saturating_sub(at))]);
    }
    let bad = || io::Error::from(io::ErrorKind::InvalidData);
    let n = r.byte()? as usize + 1;
    let mut sizes = Vec::with_capacity(n);
    match kind {
        // Xiph: each size as a run of 255s and the rest
        1 => {
            for _ in 0..n - 1 {
                let mut s = 0u64;
                loop {
                    let b = r.byte()?;
                    s += b as u64;
                    if b != 255 {
                        break;
                    }
                }
                sizes.push(s);
            }
        }
        // EBML: the first size, then each as a signed difference from the one before
        3 => {
            let first = r.size()?.ok_or_else(bad)?;
            sizes.push(first);
            for _ in 1..n - 1 {
                let start = r.pos;
                let raw = r.size()?.ok_or_else(bad)? as i64;
                let bytes = (r.pos - start) as u32;
                let bias = (1i64 << (7 * bytes - 1)) - 1;
                let last = *sizes.last().unwrap() as i64;
                sizes.push((last + raw - bias).max(0) as u64);
            }
        }
        // fixed: all the same
        _ => {}
    }
    let mut pos = r.pos;
    let left = end.saturating_sub(pos);
    if kind == 2 {
        let each = left / n as u64;
        return Ok((0..n as u64).map(|i| (pos + i * each, each)).collect());
    }
    let named: u64 = sizes.iter().sum();
    if named > left {
        return Err(bad());
    }
    sizes.push(left - named);
    let mut out = Vec::with_capacity(n);
    for s in sizes {
        out.push((pos, s));
        pos += s;
    }
    Ok(out)
}

/// Walks every cluster, handing each block of a track in `wanted` to `on`.
fn blocks(
    r: &mut R,
    start: u64,
    end: u64,
    scale: u64,
    wanted: &[u64],
    on: &mut dyn FnMut(&mut R, u64, Block) -> io::Result<()>,
) -> io::Result<()> {
    r.to(start)?;
    while r.pos < end {
        let Ok(id) = r.id() else { break };
        let Ok(size) = r.size() else { break };
        let body = r.pos;
        if id != CLUSTER {
            match size {
                Some(s) => r.to(body + s)?,
                None => break,
            }
            continue;
        }
        let cluster_end = size.map_or(end, |s| (body + s).min(end));
        let mut time = 0u64;
        while r.pos < cluster_end {
            let at = r.pos;
            let Ok(kid) = r.id() else { return Ok(()) };
            // a cluster of unknown size ends where the next top-level element begins
            if size.is_none() && LEVEL_ONE.contains(&kid) {
                r.to(at)?;
                break;
            }
            let Ok(Some(kid_size)) = r.size() else { return Ok(()) };
            let kid_body = r.pos;
            let kid_end = (kid_body + kid_size).min(cluster_end);
            match kid {
                0xE7 => {
                    let b = r.body(kid_size.min(8))?;
                    time = uint(&b);
                }
                0xA3 | 0xA0 => {
                    let (block_at, block_end, mut key, simple) = if kid == 0xA3 {
                        (kid_body, kid_end, false, true)
                    } else {
                        // a group: its block, and whether it refers to another picture
                        let (mut found, mut refers) = (None, false);
                        while r.pos < kid_end {
                            let Ok(g) = r.id() else { break };
                            let Ok(Some(gs)) = r.size() else { break };
                            if g == 0xA1 {
                                found = Some((r.pos, (r.pos + gs).min(kid_end)));
                            } else if g == 0xFB {
                                refers = true;
                            }
                            let next = r.pos + gs;
                            r.to(next)?;
                        }
                        match found {
                            Some((a, z)) => (a, z, !refers, false),
                            None => {
                                r.to(kid_end)?;
                                continue;
                            }
                        }
                    };
                    r.to(block_at)?;
                    let track = r.size()?.unwrap_or(0);
                    if wanted.contains(&track) {
                        let mut t = [0u8; 3];
                        r.exact(&mut t)?;
                        let relative = i16::from_be_bytes([t[0], t[1]]);
                        if simple {
                            key = t[2] & 0x80 != 0;
                        }
                        let ticks = time as i64 + relative as i64;
                        let here = r.pos;
                        let frames = lace(r, t[2], here, block_end)?;
                        on(r, track, Block { ns: ticks * scale as i64, key, frames })?;
                    }
                    r.to(kid_end)?;
                }
                _ => r.to(kid_end)?,
            }
        }
        if size.is_some() {
            r.to(cluster_end)?;
        }
    }
    Ok(())
}

fn first_bytes(r: &mut R, at: u64, len: u64, n: usize) -> io::Result<Vec<u8>> {
    let back = r.pos;
    r.r.seek(SeekFrom::Start(at))?;
    r.pos = at;
    let mut b = vec![0; (len as usize).min(n)];
    r.exact(&mut b)?;
    r.r.seek(SeekFrom::Start(back))?;
    r.pos = back;
    Ok(b)
}

/// How the sound track's frames are timed and described, once its first frame has been seen.
struct Sound {
    entry: codec::Entry,
    timescale: u32,
    /// Sound frames per frame, if every frame holds the same.
    each: Option<u32>,
    opus: bool,
    pcm: Option<u32>,
}

fn sound(t: &Entry, first: &[u8]) -> Option<Sound> {
    let rate = t.rate.round() as u32;
    let c = t.codec.as_str();
    let plain = |entry, timescale, each| Some(Sound { entry, timescale, each: Some(each), opus: false, pcm: None });
    if let Some(profile) = c.strip_prefix("A_AAC") {
        let setup = if t.private.is_empty() {
            let p = if profile.contains("LC") || profile.is_empty() {
                2
            } else if profile.contains("MAIN") {
                1
            } else {
                2
            };
            codec::aac_setup(p, rate, t.channels)
        } else {
            t.private.clone()
        };
        let core = codec::aac_rate(&setup).unwrap_or(rate);
        return plain(codec::audio(b"mp4a", t.channels, core, &codec::esds(0x40, false, &setup)), core, 1024);
    }
    match c {
        "A_AC3" => {
            let a = codec::ac3(first)?;
            plain(codec::audio(b"ac-3", a.channels, a.rate, &crate::mp4::bx(b"dac3", &a.setup)), a.rate, 1536)
        }
        "A_EAC3" => {
            let e = codec::eac3(first)?;
            plain(codec::audio(b"ec-3", e.channels, e.rate, &crate::mp4::bx(b"dec3", &e.setup)), e.rate, e.frames)
        }
        "A_MPEG/L3" | "A_MPEG/L2" => {
            let m = codec::mpa(first)?;
            let kind = if m.layer == 3 { b".mp3" } else { b".mp2" };
            plain(codec::audio(kind, m.channels, m.rate, &[]), m.rate, m.frames)
        }
        "A_OPUS" => {
            let (dops, channels) = codec::dops(&t.private)?;
            Some(Sound {
                entry: codec::audio(b"Opus", channels, 48000, &dops),
                timescale: 48000,
                each: None,
                opus: true,
                pcm: None,
            })
        }
        "A_FLAC" => {
            let (dfla, rate, channels, block) = codec::dfla(&t.private)?;
            plain(codec::audio(b"fLaC", channels, rate, &dfla), rate, block)
        }
        "A_PCM/INT/LIT" | "A_PCM/INT/BIG" => {
            let kind = if c.ends_with("LIT") { b"sowt" } else { b"twos" };
            let bytes = 2 * t.channels;
            Some(Sound {
                entry: codec::audio(kind, t.channels, rate, &[]),
                timescale: rate,
                each: None,
                opus: false,
                pcm: Some(bytes),
            })
        }
        _ => None,
    }
}

pub(crate) fn read(f: &mut File) -> Result<Film, Refusal> {
    let len = f.metadata().map_err(|_| Refusal::Damaged)?.len();
    f.seek(SeekFrom::Start(0)).map_err(|_| Refusal::Damaged)?;
    let mut r = R { r: BufReader::with_capacity(256 * 1024, f), pos: 0, len };
    let ((scale, clusters, end), tracks) = head(&mut r).map_err(|_| Refusal::Damaged)?;
    let list = entries(&tracks.ok_or(Refusal::Damaged)?);
    let picture = list.iter().find(|t| t.kind == 1);
    let had_picture = picture.is_some();
    let picture = picture.filter(|t| !t.encoded).and_then(|t| picture_entry(t).map(|e| (t, e)));
    let Some((pic, pic_entry)) = picture else {
        return Err(if had_picture { Refusal::Picture } else { Refusal::Damaged });
    };
    let audio = list.iter().filter(|t| t.kind == 2).find(|t| t.default).or_else(|| list.iter().find(|t| t.kind == 2));
    let keep_audio = audio.filter(|t| !t.encoded && sound_playable(t));
    let sound_left_out = audio.is_some() && keep_audio.is_none();

    let to_90k = |ns: i64| (ns as i128 * 90_000 / 1_000_000_000) as i64;
    let mut video = Track {
        video: true,
        timescale: 90_000,
        entry: pic_entry,
        width: pic.shown.0,
        height: pic.shown.1,
        samples: Vec::new(),
        last: if pic.default_duration > 0 { to_90k(pic.default_duration as i64) as u32 } else { 3000 },
        pcm: None,
    };
    let mut snd: Option<Sound> = None;
    let mut audio_track: Option<Track> = None;
    let mut audio_at = 0i64;
    let mut audio_failed = false;
    let wanted: Vec<u64> = std::iter::once(pic.number).chain(keep_audio.map(|t| t.number)).collect();
    let pic_number = pic.number;
    let step = to_90k(pic.default_duration as i64);
    let walked = blocks(&mut r, clusters, end, scale, &wanted, &mut |r, track, b| {
        if track == pic_number {
            for (k, (at, len)) in b.frames.iter().enumerate() {
                video.samples.push(Sample {
                    pts: to_90k(b.ns) + k as i64 * step,
                    key: b.key,
                    parts: vec![Part::Source(*at, *len)],
                    order: *at,
                    frames: 0,
                });
            }
            return Ok(());
        }
        let Some(a) = keep_audio else { return Ok(()) };
        if audio_failed {
            return Ok(());
        }
        if snd.is_none() {
            let Some(&(at, len)) = b.frames.first() else { return Ok(()) };
            let head = first_bytes(r, at, len, 64)?;
            match sound(a, &head) {
                Some(s) => {
                    audio_at = (b.ns as i128 * s.timescale as i128 / 1_000_000_000) as i64;
                    audio_track = Some(Track {
                        video: false,
                        timescale: s.timescale,
                        entry: s.entry.clone(),
                        width: 0,
                        height: 0,
                        samples: Vec::new(),
                        last: s.each.unwrap_or(960),
                        pcm: s.pcm,
                    });
                    snd = Some(s);
                }
                None => {
                    audio_failed = true;
                    return Ok(());
                }
            }
        }
        let (Some(s), Some(t)) = (&snd, &mut audio_track) else { return Ok(()) };
        for &(at, len) in &b.frames {
            let (count, frames) = if let Some(bytes) = s.pcm {
                ((len / bytes as u64) as u32, (len / bytes as u64) as u32)
            } else if s.opus {
                (codec::opus_frames(&first_bytes(r, at, len, 2)?).unwrap_or(960), 0)
            } else {
                (s.each.unwrap_or(1024), 0)
            };
            t.samples.push(Sample { pts: audio_at, key: true, parts: vec![Part::Source(at, len)], order: at, frames });
            audio_at += count as i64;
        }
        Ok(())
    });
    if walked.is_err() && video.samples.is_empty() {
        return Err(Refusal::Damaged);
    }
    if video.samples.is_empty() {
        return Err(Refusal::Damaged);
    }
    // a frame cut short at the end of a file that stops early isn't played
    video.samples.retain(|s| matches!(s.parts[0], Part::Source(at, l) if at + l <= len));
    let mut tracks = vec![video];
    let mut left_out = sound_left_out || audio_failed;
    if let Some(mut t) = audio_track {
        t.samples.retain(|s| matches!(s.parts[0], Part::Source(at, l) if at + l <= len));
        if t.samples.is_empty() {
            left_out = true;
        } else {
            tracks.push(t);
        }
    }
    Ok(Film { tracks, had_picture: true, sound_left_out: left_out })
}
