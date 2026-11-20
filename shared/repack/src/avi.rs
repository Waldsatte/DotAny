//! An AVI's frames, read for the new film. The streams are said in `hdrl`; the frames are the chunks of every `movi`
//! list, in the first RIFF and in the ones an AVI over 1 GB adds (OpenDML's `AVIX`), so no index is needed. An AVI keeps
//! no times: a picture's time is its place in the stream, a sound's is the frames before it.

use crate::bits::{le16, le32};
use crate::{Film, Part, Refusal, Sample, Track, codec, h264};
use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom};

struct Stream {
    video: bool,
    handler: [u8; 4],
    scale: u32,
    rate: u32,
    format: Vec<u8>,
}

/// One chunk of `movi`: whose, where its bytes are, how many.
struct Chunk {
    stream: u32,
    at: u64,
    len: u64,
}

struct R<'a> {
    r: BufReader<&'a mut File>,
    pos: u64,
}

impl R<'_> {
    fn head(&mut self) -> io::Result<([u8; 4], u64)> {
        let mut b = [0u8; 8];
        self.r.read_exact(&mut b)?;
        self.pos += 8;
        Ok((b[..4].try_into().unwrap(), u32::from_le_bytes(b[4..].try_into().unwrap()) as u64))
    }
    fn to(&mut self, at: u64) -> io::Result<()> {
        self.r.seek_relative(at as i64 - self.pos as i64)?;
        self.pos = at;
        Ok(())
    }
    fn take(&mut self, n: u64) -> io::Result<Vec<u8>> {
        if n > 64 << 20 {
            return Err(io::ErrorKind::InvalidData.into());
        }
        let mut b = vec![0; n as usize];
        self.r.read_exact(&mut b)?;
        self.pos += n;
        Ok(b)
    }
}

/// The elements in a list we hold whole.
fn kids(b: &[u8]) -> Vec<([u8; 4], &[u8])> {
    let mut out = Vec::new();
    let mut at = 0;
    while at + 8 <= b.len() {
        let id: [u8; 4] = b[at..at + 4].try_into().unwrap();
        let n = le32(b, at + 4).unwrap_or(0) as usize;
        let end = (at + 8).saturating_add(n).min(b.len());
        out.push((id, &b[at + 8..end]));
        at = end + (n & 1);
    }
    out
}

fn streams(hdrl: &[u8]) -> Vec<Stream> {
    let mut out = Vec::new();
    for (id, body) in kids(hdrl) {
        if &id != b"LIST" || body.get(..4) != Some(b"strl") {
            continue;
        }
        let (mut strh, mut strf) = (None, Vec::new());
        for (k, b) in kids(&body[4..]) {
            match &k {
                b"strh" => strh = Some(b),
                b"strf" => strf = b.to_vec(),
                _ => {}
            }
        }
        let Some(h) = strh.filter(|h| h.len() >= 36) else {
            out.push(Stream { video: false, handler: [0; 4], scale: 0, rate: 0, format: Vec::new() });
            continue;
        };
        out.push(Stream {
            video: &h[..4] == b"vids",
            handler: h[4..8].try_into().unwrap(),
            scale: le32(h, 20).unwrap_or(0),
            rate: le32(h, 24).unwrap_or(0),
            format: strf,
        });
    }
    out
}

/// Walks a `movi` list's chunks (and the `rec ` lists inside it).
fn movi(r: &mut R, end: u64, out: &mut Vec<Chunk>) -> io::Result<()> {
    while r.pos + 8 <= end {
        let (id, n) = r.head()?;
        let body = r.pos;
        if &id == b"LIST" {
            r.to(body + 4)?;
            movi(r, (body + n).min(end), out)?;
        } else if id[0].is_ascii_digit() && id[1].is_ascii_digit() && n > 0 {
            let stream = ((id[0] - b'0') * 10 + (id[1] - b'0')) as u32;
            if matches!(&id[2..], b"dc" | b"db" | b"wb") {
                out.push(Chunk { stream, at: body, len: n.min(end.saturating_sub(body)) });
            }
        } else if id[0].is_ascii_digit() && &id[2..] == b"dc" {
            // an empty picture: the one before shows a frame longer
            out.push(Chunk { stream: ((id[0] - b'0') * 10 + (id[1] - b'0')) as u32, at: body, len: 0 });
        }
        r.to((body + n + (n & 1)).min(end))?;
    }
    Ok(())
}

enum Picture {
    H264,
    Mp4v,
    Jpeg,
    Dv,
}

fn picture_kind(s: &Stream) -> Option<Picture> {
    let compression = s.format.get(16..20).map(|c| c.to_ascii_uppercase());
    let handler = s.handler.to_ascii_uppercase();
    let is = |names: &[&[u8; 4]]| names.iter().any(|n| compression.as_deref() == Some(&n[..]) || handler == **n);
    if is(&[b"H264", b"X264", b"AVC1", b"DAVC", b"VSSH"]) {
        Some(Picture::H264)
    } else if is(&[b"XVID", b"DIVX", b"DX50", b"FMP4", b"MP4V", b"3IV2", b"M4S2"]) {
        Some(Picture::Mp4v)
    } else if is(&[b"MJPG", b"AVRN", b"JPEG"]) {
        Some(Picture::Jpeg)
    } else if is(&[b"DVSD", b"DV25", b"DVC "]) {
        Some(Picture::Dv)
    } else {
        None
    }
}

fn read_at(f: &mut BufReader<&mut File>, at: u64, len: u64) -> io::Result<Vec<u8>> {
    f.seek(SeekFrom::Start(at))?;
    let mut b = vec![0; len as usize];
    f.read_exact(&mut b)?;
    Ok(b)
}

pub(crate) fn read(f: &mut File) -> Result<Film, Refusal> {
    let len = f.metadata().map_err(|_| Refusal::Damaged)?.len();
    f.seek(SeekFrom::Start(0)).map_err(|_| Refusal::Damaged)?;
    let mut r = R { r: BufReader::with_capacity(256 * 1024, f), pos: 0 };
    let mut list: Option<Vec<Stream>> = None;
    let mut chunks = Vec::new();
    let walked = (|| -> io::Result<()> {
        while r.pos + 12 <= len {
            let (id, n) = r.head()?;
            let riff_end = (r.pos + n).min(len);
            if &id != b"RIFF" {
                break;
            }
            r.to(r.pos + 4)?;
            while r.pos + 8 <= riff_end {
                let (id, n) = r.head()?;
                let body = r.pos;
                let end = (body + n).min(riff_end);
                if &id == b"LIST" {
                    let mut kind = [0u8; 4];
                    r.r.read_exact(&mut kind)?;
                    r.pos += 4;
                    match &kind {
                        b"hdrl" if list.is_none() => list = Some(streams(&r.take(end - r.pos)?)),
                        b"movi" => movi(&mut r, end, &mut chunks)?,
                        _ => {}
                    }
                }
                r.to((body + n + (n & 1)).min(riff_end))?;
            }
            r.to(riff_end)?;
        }
        Ok(())
    })();
    let list = list.ok_or(Refusal::Damaged)?;
    if walked.is_err() && chunks.is_empty() {
        return Err(Refusal::Damaged);
    }
    let Some(vi) = list.iter().position(|s| s.video) else { return Err(Refusal::Damaged) };
    let vs = &list[vi];
    let Some(kind) = picture_kind(vs) else { return Err(Refusal::Picture) };
    if vs.scale == 0 || vs.rate == 0 {
        return Err(Refusal::Damaged);
    }
    let width = le32(&vs.format, 4).unwrap_or(0);
    let height = (le32(&vs.format, 8).unwrap_or(0) as i32).unsigned_abs();
    let mut samples = Vec::new();
    let mut frame_no = 0i64;
    let mut order = h264::Order::new();
    let (mut sps, mut pps) = (None::<Vec<u8>>, None::<Vec<u8>>);
    let mut vol = None;
    let file = &mut r.r;
    for c in chunks.iter().filter(|c| c.stream == vi as u32) {
        if c.len == 0 {
            frame_no += 1;
            continue;
        }
        match kind {
            Picture::H264 => {
                let b = read_at(file, c.at, c.len).map_err(|_| Refusal::Damaged)?;
                if !h264::annex_b(&b) {
                    return Err(Refusal::Picture);
                }
                for (a, z) in h264::units(&b) {
                    match b[a] & 0x1F {
                        7 if sps.is_none() => sps = Some(b[a..z].to_vec()),
                        8 if pps.is_none() => pps = Some(b[a..z].to_vec()),
                        _ => {}
                    }
                }
                let key = order.frame(&b);
                samples.push(Sample { pts: 0, key, parts: h264::parts(&b, c.at), order: c.at, frames: 0 });
            }
            Picture::Mp4v => {
                let b = read_at(file, c.at, c.len).map_err(|_| Refusal::Damaged)?;
                let kinds = codec::mp4v_pictures(&b);
                // DivX's B-frames come packed two to a chunk: their order can't be kept, so the film is refused
                if kinds.contains(&2) {
                    return Err(Refusal::Picture);
                }
                if vol.is_none() {
                    vol = codec::mp4v_setup(&b);
                }
                let key = kinds.first() == Some(&0);
                // a chunk without a coded picture (a placeholder) only holds the time
                if kinds.is_empty() && b.len() <= 8 {
                    frame_no += 1;
                    continue;
                }
                samples.push(Sample {
                    pts: frame_no * vs.scale as i64,
                    key,
                    parts: vec![Part::Source(c.at, c.len)],
                    order: c.at,
                    frames: 0,
                });
            }
            _ => samples.push(Sample {
                pts: frame_no * vs.scale as i64,
                key: true,
                parts: vec![Part::Source(c.at, c.len)],
                order: c.at,
                frames: 0,
            }),
        }
        frame_no += 1;
    }
    if samples.is_empty() {
        return Err(Refusal::Damaged);
    }
    // pixels that aren't square say so in MPEG-4's own setup (an AVI's `vprp` is seldom there)
    let shape = match kind {
        Picture::Mp4v => vol.as_deref().and_then(codec::mp4v_pixel_shape),
        _ => None,
    };
    let shown = match shape {
        Some((w, h)) if w != h => ((width as u64 * w as u64 / h as u64) as u32, height),
        _ => (width, height),
    };
    let entry = match kind {
        Picture::H264 => {
            let (Some(s), Some(p)) = (&sps, &pps) else { return Err(Refusal::Damaged) };
            for (s, rank) in samples.iter_mut().zip(order.shown()) {
                s.pts = rank as i64 * vs.scale as i64;
            }
            codec::visual(b"avc1", width, height, shown, &h264::avcc(s, p).ok_or(Refusal::Damaged)?)
        }
        Picture::Mp4v => {
            let setup = vol.or_else(|| vs.format.get(40..).map(<[u8]>::to_vec)).unwrap_or_default();
            codec::visual(b"mp4v", width, height, shown, &codec::esds(0x20, true, &setup))
        }
        Picture::Jpeg => codec::visual(b"jpeg", width, height, shown, &[]),
        Picture::Dv => codec::visual(if height == 576 { b"dvcp" } else { b"dvc " }, width, height, shown, &[]),
    };
    let video = Track {
        video: true,
        timescale: vs.rate,
        entry,
        width: shown.0,
        height: shown.1,
        samples,
        last: vs.scale,
        pcm: None,
    };
    let mut tracks = vec![video];
    let ai = list.iter().position(|s| !s.video && s.format.len() >= 16);
    let mut left_out = false;
    if let Some(ai) = ai {
        match sound(&list[ai], chunks.iter().filter(|c| c.stream == ai as u32), file) {
            Some(t) if !t.samples.is_empty() => tracks.push(t),
            _ => left_out = true,
        }
    }
    Ok(Film { tracks, had_picture: true, sound_left_out: left_out })
}

/// The sound stream: PCM as it is, MP3, MP2 and AC-3 cut into their frames (an AVI's chunks needn't end with one).
fn sound<'c>(s: &Stream, chunks: impl Iterator<Item = &'c Chunk>, file: &mut BufReader<&mut File>) -> Option<Track> {
    let tag = le16(&s.format, 0)?;
    let channels = le16(&s.format, 2)? as u32;
    let rate = le32(&s.format, 4)?;
    let align = le16(&s.format, 12)? as u32;
    let bits = le16(&s.format, 14)? as u32;
    let chunks: Vec<&Chunk> = chunks.collect();
    if tag == 1 {
        if bits != 16 || align == 0 || rate == 0 {
            return None;
        }
        let samples = chunks
            .iter()
            .filter(|c| c.len >= align as u64)
            .map(|c| {
                let frames = (c.len / align as u64) as u32;
                Sample {
                    pts: 0,
                    key: true,
                    parts: vec![Part::Source(c.at, frames as u64 * align as u64)],
                    order: c.at,
                    frames,
                }
            })
            .collect();
        return Some(Track {
            video: false,
            timescale: rate,
            entry: codec::audio(b"sowt", channels, rate, &[]),
            width: 0,
            height: 0,
            samples,
            last: 1,
            pcm: Some(align),
        });
    }
    if !matches!(tag, 0x55 | 0x50 | 0x2000) {
        return None;
    }
    // the stream's bytes end to end, and where each stretch of them is in the file
    let mut bytes = Vec::new();
    let mut map: Vec<(usize, u64)> = Vec::new();
    for c in &chunks {
        map.push((bytes.len(), c.at));
        bytes.extend(read_at(file, c.at, c.len).ok()?);
    }
    let place = |from: usize, len: usize| -> Vec<Part> {
        let mut parts = Vec::new();
        let (mut at, end) = (from, from + len);
        while at < end {
            let i = map.partition_point(|m| m.0 <= at) - 1;
            let stop = map.get(i + 1).map_or(bytes.len(), |m| m.0).min(end);
            parts.push(Part::Source(map[i].1 + (at - map[i].0) as u64, (stop - at) as u64));
            at = stop;
        }
        parts
    };
    let mut samples = Vec::new();
    let (mut at, mut pts) = (0usize, 0i64);
    let mut entry = None;
    let mut each = 0;
    let mut timescale = rate;
    while at + 8 <= bytes.len() {
        let found = if tag == 0x2000 {
            codec::ac3(&bytes[at..]).map(|a| {
                if entry.is_none() {
                    entry = Some(codec::audio(b"ac-3", a.channels, a.rate, &crate::mp4::bx(b"dac3", &a.setup)));
                    timescale = a.rate;
                }
                (a.len, 1536)
            })
        } else {
            codec::mpa(&bytes[at..]).map(|m| {
                if entry.is_none() {
                    entry = Some(codec::audio(if m.layer == 3 { b".mp3" } else { b".mp2" }, m.channels, m.rate, &[]));
                    timescale = m.rate;
                }
                (m.len, m.frames)
            })
        };
        match found {
            Some((n, frames)) if at + n <= bytes.len() => {
                let parts = place(at, n);
                let order = match parts.first() {
                    Some(Part::Source(o, _)) => *o,
                    _ => 0,
                };
                samples.push(Sample { pts, key: true, parts, order, frames: 0 });
                pts += frames as i64;
                each = frames;
                at += n;
            }
            // not a frame here: the stream lost its place (or a tag in front); look for the next one
            _ => at += 1,
        }
    }
    Some(Track { video: false, timescale, entry: entry?, width: 0, height: 0, samples, last: each, pcm: None })
}
