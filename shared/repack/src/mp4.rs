//! The new film: an MP4, or a QuickTime movie when a track needs one (`codec::Entry`), its boxes first so the player
//! knows everything at once, then one `mdat` with the samples, interleaved as they were in the original. The boxes that
//! hold times are written in version 1 (64-bit), so a long film at a fine time scale doesn't overflow.

use crate::{Film, From, Part, Repacked, Stretch, Track};

pub(crate) fn bx(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut b = Vec::with_capacity(body.len() + 8);
    b.extend_from_slice(&(body.len() as u32 + 8).to_be_bytes());
    b.extend_from_slice(kind);
    b.extend_from_slice(body);
    b
}

pub(crate) fn full(kind: &[u8; 4], version: u8, flags: u32, body: &[u8]) -> Vec<u8> {
    let mut b = Vec::with_capacity(body.len() + 4);
    b.push(version);
    b.extend_from_slice(&flags.to_be_bytes()[1..]);
    b.extend_from_slice(body);
    bx(kind, &b)
}

/// The film's time scale, for the movie header and the edit lists.
const MOVIE_SCALE: u32 = 1000;
const MATRIX: [u32; 9] = [0x0001_0000, 0, 0, 0, 0x0001_0000, 0, 0, 0, 0x4000_0000];

struct Timed {
    #[cfg_attr(not(test), allow(dead_code))]
    dts: Vec<i64>,
    /// The composition offset of each sample (shown at dts + offset); all zero for most tracks.
    offset: Vec<i64>,
    dur: Vec<u32>,
    /// Where in the track's own time its first shown sample is, and how long it shows for.
    media_start: i64,
    media_len: i64,
    /// When the track starts, in the original's time, in seconds.
    starts: f64,
}

fn timing(t: &Track) -> Timed {
    let n = t.samples.len();
    let pts: Vec<i64> = if let Some(_) = t.pcm {
        // sound frames one after another from the first sample's time
        let mut at = t.samples.first().map_or(0, |s| s.pts);
        t.samples
            .iter()
            .map(|s| {
                let here = at;
                at += s.frames as i64;
                here
            })
            .collect()
    } else {
        t.samples.iter().map(|s| s.pts).collect()
    };
    // decoding times: the shown times in order, moved back just enough that none is shown before it is decoded
    let mut dts = pts.clone();
    if pts.windows(2).any(|w| w[1] < w[0]) {
        let mut sorted = pts.clone();
        sorted.sort_unstable();
        let back = sorted.iter().zip(&pts).map(|(s, p)| s - p).max().unwrap_or(0).max(0);
        dts = sorted.iter().map(|s| s - back).collect();
    }
    let d0 = dts.first().copied().unwrap_or(0);
    let mut dur = Vec::with_capacity(n);
    for i in 0..n {
        let d = if t.pcm.is_some() {
            t.samples[i].frames as i64
        } else if i + 1 < n {
            dts[i + 1] - dts[i]
        } else {
            t.last as i64
        };
        dur.push(d.clamp(0, u32::MAX as i64) as u32);
    }
    let offset: Vec<i64> = pts.iter().zip(&dts).map(|(p, d)| p - d).collect();
    let dts: Vec<i64> = dts.iter().map(|d| d - d0).collect();
    let shown: Vec<i64> = dts.iter().zip(&offset).map(|(d, o)| d + o).collect();
    let media_start = shown.iter().copied().min().unwrap_or(0);
    let media_end = shown.iter().zip(&dur).map(|(s, d)| s + *d as i64).max().unwrap_or(0);
    let first = pts.iter().copied().min().unwrap_or(0);
    Timed {
        dts,
        offset,
        dur,
        media_start,
        media_len: (media_end - media_start).max(0),
        starts: first as f64 / t.timescale as f64,
    }
}

/// A run of samples of one track that sit together in `mdat`.
struct Chunk {
    at: u64,
    /// Samples in it (sound frames, for PCM).
    count: u32,
}

fn runs(list: &[u32]) -> Vec<(u32, u32)> {
    let mut out: Vec<(u32, u32)> = Vec::new();
    for &v in list {
        match out.last_mut() {
            Some((c, last)) if *last == v => *c += 1,
            _ => out.push((1, v)),
        }
    }
    out
}

fn words(list: &[u32]) -> Vec<u8> {
    list.iter().flat_map(|w| w.to_be_bytes()).collect()
}

fn stbl(t: &Track, time: &Timed, chunks: &[Chunk], base: u64, quicktime: bool) -> Vec<u8> {
    let mut stsd = 1u32.to_be_bytes().to_vec();
    stsd.extend(t.entry.bytes(quicktime));
    let mut parts = vec![full(b"stsd", 0, 0, &stsd)];
    // times between decodes
    let deltas: Vec<u32> = if let Some(_) = t.pcm { vec![1; 1] } else { time.dur.clone() };
    let (stts_runs, total): (Vec<(u32, u32)>, u32) = if t.pcm.is_some() {
        let frames: u64 = t.samples.iter().map(|s| s.frames as u64).sum();
        (vec![(frames as u32, deltas[0])], frames as u32)
    } else {
        (runs(&deltas), t.samples.len() as u32)
    };
    let mut stts = (stts_runs.len() as u32).to_be_bytes().to_vec();
    for (c, d) in &stts_runs {
        stts.extend_from_slice(&c.to_be_bytes());
        stts.extend_from_slice(&d.to_be_bytes());
    }
    parts.push(full(b"stts", 0, 0, &stts));
    if time.offset.iter().any(|o| *o != 0) {
        let offs: Vec<u32> = time.offset.iter().map(|o| (*o).clamp(0, u32::MAX as i64) as u32).collect();
        let r = runs(&offs);
        let mut ctts = (r.len() as u32).to_be_bytes().to_vec();
        for (c, o) in r {
            ctts.extend_from_slice(&c.to_be_bytes());
            ctts.extend_from_slice(&o.to_be_bytes());
        }
        parts.push(full(b"ctts", 0, 0, &ctts));
    }
    if t.video && t.samples.iter().any(|s| !s.key) {
        let keys: Vec<u32> = t.samples.iter().enumerate().filter(|(_, s)| s.key).map(|(i, _)| i as u32 + 1).collect();
        let mut stss = (keys.len() as u32).to_be_bytes().to_vec();
        stss.extend(words(&keys));
        parts.push(full(b"stss", 0, 0, &stss));
    }
    // samples per chunk, as runs of chunks with the same count
    let mut stsc = Vec::new();
    let mut entries = 0u32;
    let mut last = None;
    for (i, c) in chunks.iter().enumerate() {
        if last != Some(c.count) {
            stsc.extend_from_slice(&(i as u32 + 1).to_be_bytes());
            stsc.extend_from_slice(&c.count.to_be_bytes());
            stsc.extend_from_slice(&1u32.to_be_bytes());
            entries += 1;
            last = Some(c.count);
        }
    }
    let mut b = entries.to_be_bytes().to_vec();
    b.extend(stsc);
    parts.push(full(b"stsc", 0, 0, &b));
    let mut stsz = Vec::new();
    if let Some(each) = t.pcm {
        stsz.extend_from_slice(&each.to_be_bytes());
        stsz.extend_from_slice(&total.to_be_bytes());
    } else {
        stsz.extend_from_slice(&0u32.to_be_bytes());
        stsz.extend_from_slice(&total.to_be_bytes());
        for s in &t.samples {
            stsz.extend_from_slice(&(s.size() as u32).to_be_bytes());
        }
    }
    parts.push(full(b"stsz", 0, 0, &stsz));
    let mut co64 = (chunks.len() as u32).to_be_bytes().to_vec();
    for c in chunks {
        co64.extend_from_slice(&(base + c.at).to_be_bytes());
    }
    parts.push(full(b"co64", 0, 0, &co64));
    bx(b"stbl", &parts.concat())
}

fn trak(id: u32, t: &Track, time: &Timed, origin: f64, chunks: &[Chunk], base: u64, quicktime: bool) -> (Vec<u8>, u64) {
    let scale = |media: i64| (media as f64 / t.timescale as f64 * MOVIE_SCALE as f64).round() as u64;
    let wait = ((time.starts - origin) * MOVIE_SCALE as f64).round().max(0.0) as u64;
    let shown = scale(time.media_len);
    let mut tkhd = Vec::new();
    tkhd.extend_from_slice(&[0; 16]);
    tkhd.extend_from_slice(&id.to_be_bytes());
    tkhd.extend_from_slice(&[0; 4]);
    tkhd.extend_from_slice(&(wait + shown).to_be_bytes());
    tkhd.extend_from_slice(&[0; 8]);
    tkhd.extend_from_slice(&0u16.to_be_bytes());
    tkhd.extend_from_slice(&(if t.video { 0u16 } else { 1 }).to_be_bytes());
    tkhd.extend_from_slice(&(if t.video { 0u16 } else { 0x0100 }).to_be_bytes());
    tkhd.extend_from_slice(&[0; 2]);
    tkhd.extend(words(&MATRIX));
    tkhd.extend_from_slice(&(t.width << 16).to_be_bytes());
    tkhd.extend_from_slice(&(t.height << 16).to_be_bytes());
    let mut elst = Vec::new();
    let mut entries = 0u32;
    if wait > 0 {
        elst.extend_from_slice(&wait.to_be_bytes());
        elst.extend_from_slice(&(-1i64).to_be_bytes());
        elst.extend_from_slice(&0x0001_0000u32.to_be_bytes());
        entries += 1;
    }
    elst.extend_from_slice(&shown.to_be_bytes());
    elst.extend_from_slice(&time.media_start.to_be_bytes());
    elst.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    entries += 1;
    let mut e = entries.to_be_bytes().to_vec();
    e.extend(elst);
    let edts = bx(b"edts", &full(b"elst", 1, 0, &e));
    let media_dur: u64 = if t.pcm.is_some() {
        t.samples.iter().map(|s| s.frames as u64).sum()
    } else {
        time.dur.iter().map(|d| *d as u64).sum()
    };
    let mut mdhd = vec![0; 16];
    mdhd.extend_from_slice(&t.timescale.to_be_bytes());
    mdhd.extend_from_slice(&media_dur.to_be_bytes());
    mdhd.extend_from_slice(&0x55C4u16.to_be_bytes());
    mdhd.extend_from_slice(&[0; 2]);
    let mut hdlr = if quicktime { b"mhlr".to_vec() } else { vec![0; 4] };
    hdlr.extend_from_slice(if t.video { b"vide" } else { b"soun" });
    hdlr.extend_from_slice(&[0; 12]);
    hdlr.push(0);
    let head = if t.video { full(b"vmhd", 0, 1, &[0; 8]) } else { full(b"smhd", 0, 0, &[0; 4]) };
    let mut dref = 1u32.to_be_bytes().to_vec();
    dref.extend(full(b"url ", 0, 1, &[]));
    let dinf = bx(b"dinf", &full(b"dref", 0, 0, &dref));
    let minf = bx(b"minf", &[head, dinf, stbl(t, time, chunks, base, quicktime)].concat());
    let mdia = bx(b"mdia", &[full(b"mdhd", 1, 0, &mdhd), full(b"hdlr", 0, 0, &hdlr), minf].concat());
    (bx(b"trak", &[full(b"tkhd", 1, 3, &tkhd), edts, mdia].concat()), wait + shown)
}

fn moov(film: &Film, times: &[Timed], origin: f64, chunks: &[Vec<Chunk>], base: u64, quicktime: bool) -> Vec<u8> {
    let mut traks = Vec::new();
    let mut longest = 0u64;
    for (i, t) in film.tracks.iter().enumerate() {
        let (b, len) = trak(i as u32 + 1, t, &times[i], origin, &chunks[i], base, quicktime);
        traks.extend(b);
        longest = longest.max(len);
    }
    let mut mvhd = vec![0; 16];
    mvhd.extend_from_slice(&MOVIE_SCALE.to_be_bytes());
    mvhd.extend_from_slice(&longest.to_be_bytes());
    mvhd.extend_from_slice(&0x0001_0000u32.to_be_bytes());
    mvhd.extend_from_slice(&0x0100u16.to_be_bytes());
    mvhd.extend_from_slice(&[0; 10]);
    mvhd.extend(words(&MATRIX));
    mvhd.extend_from_slice(&[0; 24]);
    mvhd.extend_from_slice(&(film.tracks.len() as u32 + 1).to_be_bytes());
    bx(b"moov", &[full(b"mvhd", 1, 0, &mvhd), traks].concat())
}

pub(crate) fn write(film: Film) -> Repacked {
    let times: Vec<Timed> = film.tracks.iter().map(timing).collect();
    let origin = times.iter().map(|t| t.starts).fold(f64::INFINITY, f64::min);
    let origin = if origin.is_finite() { origin } else { 0.0 };
    // every sample of every track, in the order they were in the original
    let mut order: Vec<(u64, usize, usize)> = Vec::new();
    for (ti, t) in film.tracks.iter().enumerate() {
        order.extend(t.samples.iter().enumerate().map(|(si, s)| (s.order, ti, si)));
    }
    order.sort_by_key(|&(o, ti, si)| (o, ti, si));
    let mut chunks: Vec<Vec<Chunk>> = film.tracks.iter().map(|_| Vec::new()).collect();
    let mut stretches: Vec<Stretch> = Vec::with_capacity(order.len());
    let mut own = Vec::new();
    let mut at = 0u64;
    let mut prev_track = usize::MAX;
    for &(_, ti, si) in &order {
        let s = &film.tracks[ti].samples[si];
        let count = if film.tracks[ti].pcm.is_some() { s.frames } else { 1 };
        if ti == prev_track {
            if let Some(c) = chunks[ti].last_mut() {
                c.count += count;
            }
        } else {
            chunks[ti].push(Chunk { at, count });
        }
        prev_track = ti;
        for p in &s.parts {
            let (len, from) = match p {
                Part::Source(o, l) => (*l, From::Source(*o)),
                Part::Own(b) => {
                    let o = own.len() as u64;
                    own.extend_from_slice(b);
                    (b.len() as u64, From::Own(o))
                }
            };
            if len == 0 {
                continue;
            }
            if let (Some(last), From::Source(o)) = (stretches.last_mut(), from) {
                if let From::Source(lo) = last.from {
                    if lo + last.len == o {
                        last.len += len;
                        at += len;
                        continue;
                    }
                }
            }
            stretches.push(Stretch { at, len, from });
            at += len;
        }
    }
    let payload = at;
    // QuickTime only where a track needs it (`codec::Entry`)
    let quicktime = film.tracks.iter().any(|t| t.entry.quicktime());
    let ftyp = if quicktime {
        bx(b"ftyp", &[b"qt  ".as_slice(), &0x0000_0200u32.to_be_bytes(), b"qt  "].concat())
    } else {
        bx(b"ftyp", &[b"isom".as_slice(), &0x0000_0200u32.to_be_bytes(), b"isomiso2mp41"].concat())
    };
    // the boxes' size doesn't depend on where the samples are (every place is 64 bits), so the first pass measures
    let first = moov(&film, &times, origin, &chunks, 0, quicktime);
    let base = (ftyp.len() + first.len() + 16) as u64;
    let moov = moov(&film, &times, origin, &chunks, base, quicktime);
    let mut head = ftyp;
    head.extend(moov);
    head.extend_from_slice(&1u32.to_be_bytes());
    head.extend_from_slice(b"mdat");
    head.extend_from_slice(&(16 + payload).to_be_bytes());
    debug_assert_eq!(head.len() as u64, base);
    for s in &mut stretches {
        s.at += base;
    }
    Repacked { size: base + payload, head, stretches, own, sound_left_out: film.sound_left_out, quicktime }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Sample;

    fn sample(pts: i64, key: bool, order: u64) -> Sample {
        Sample { pts, key, parts: vec![Part::Source(order * 10, 10)], order, frames: 0 }
    }

    #[test]
    fn b_frames_are_decoded_before_they_are_shown() {
        // I P B B as an MKV gives them: each with the time it is shown
        let t = Track {
            video: true,
            timescale: 1000,
            entry: crate::codec::visual(b"avc1", 0, 0, (0, 0), &[]),
            width: 0,
            height: 0,
            samples: [0, 120, 40, 80, 160].iter().enumerate().map(|(i, p)| sample(*p, i == 0, i as u64)).collect(),
            last: 40,
            pcm: None,
        };
        let time = timing(&t);
        for (i, s) in t.samples.iter().enumerate() {
            assert!(time.dts[i] + time.offset[i] - time.media_start == s.pts, "{i}");
            assert!(time.offset[i] >= 0);
        }
        assert!(time.dts.windows(2).all(|w| w[1] > w[0]));
        assert_eq!(time.media_len, 200);
    }

    #[test]
    fn the_new_film_reads_back_the_original_bytes_in_its_samples() {
        let t = Track {
            video: true,
            timescale: 1000,
            entry: crate::codec::visual(b"avc1", 16, 16, (16, 16), &[]),
            width: 16,
            height: 16,
            samples: (0..5).map(|i| sample(i * 40, i == 0, i as u64)).collect(),
            last: 40,
            pcm: None,
        };
        let r = write(Film { tracks: vec![t], had_picture: true, sound_left_out: false });
        // 5 samples of 10 bytes, each right after the other in the original: one stretch
        assert_eq!(r.stretches.len(), 1);
        assert_eq!(r.size, r.head.len() as u64 + 50);
        assert_eq!(&r.head[4..8], b"ftyp");
        let moov = &r.head[u32::from_be_bytes(r.head[..4].try_into().unwrap()) as usize..];
        assert_eq!(&moov[4..8], b"moov");
    }
}
