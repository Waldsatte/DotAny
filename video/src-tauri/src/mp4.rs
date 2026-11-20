//! MP4 and MOV: only the boxes around the samples. Every sample comes out as one list of where it is, how long it lasts and
//! whether it can start a picture, whether the file kept that in tables or in fragments. A save writes a new table for the
//! samples it keeps and copies those samples unread.
//!
//! Cutting: the film's video starts at a keyframe, and each edit list says which moment of the track the film starts on, so
//! frames that only exist to complete a picture stay hidden and sound stays in step. Files whose edit lists do more than
//! "wait, then play" can't be cut, but can still get a new title or cover.

use crate::media::{Cover, Edit, Facts, Plan};
use dotany_base::limits;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

const LARGEST_MOOV: u64 = 1 << 30;
const CHUNK_SECONDS: f64 = 0.5;

type Kind = [u8; 4];

#[derive(Clone)]
pub struct Atom {
    kind: Kind,
    body: Body,
}

#[derive(Clone)]
enum Body {
    Data(Vec<u8>),
    Kids { head: Vec<u8>, kids: Vec<Atom>, tail: Vec<u8> },
}

fn be16(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_be_bytes(b.get(i..i + 2)?.try_into().ok()?))
}
fn be32(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_be_bytes(b.get(i..i + 4)?.try_into().ok()?))
}
fn be64(b: &[u8], i: usize) -> Option<u64> {
    Some(u64::from_be_bytes(b.get(i..i + 8)?.try_into().ok()?))
}

fn holds_boxes(kind: &Kind) -> bool {
    matches!(kind, b"moov" | b"trak" | b"mdia" | b"minf" | b"stbl" | b"udta" | b"edts" | b"mvex" | b"moof" | b"traf")
}

fn parse_kids(b: &[u8]) -> (Vec<Atom>, Vec<u8>) {
    let (mut kids, mut at) = (Vec::new(), 0usize);
    while b.len() - at >= 8 {
        let mut size = be32(b, at).unwrap() as u64;
        let kind: Kind = b[at + 4..at + 8].try_into().unwrap();
        let mut head = 8usize;
        if size == 1 {
            let Some(big) = be64(b, at + 8) else { break };
            size = big;
            head = 16;
        } else if size == 0 {
            size = (b.len() - at) as u64;
        }
        if size < head as u64 || size > (b.len() - at) as u64 {
            break;
        }
        kids.push(parse_atom(kind, &b[at + head..at + size as usize]));
        at += size as usize;
    }
    (kids, b[at..].to_vec())
}

fn parse_atom(kind: Kind, b: &[u8]) -> Atom {
    if &kind == b"ilst" {
        // each item is a box holding its value in a `data` box
        let (items, tail) = parse_kids(b);
        let kids = items
            .into_iter()
            .map(|i| {
                let (inner, rest) = parse_kids(i.bytes());
                Atom { kind: i.kind, body: Body::Kids { head: Vec::new(), kids: inner, tail: rest } }
            })
            .collect();
        return Atom { kind, body: Body::Kids { head: Vec::new(), kids, tail } };
    }
    if holds_boxes(&kind) {
        let (kids, tail) = parse_kids(b);
        return Atom { kind, body: Body::Kids { head: Vec::new(), kids, tail } };
    }
    if &kind == b"meta" {
        // a full box in MP4, a plain one in some QuickTime files
        let skip = if b.get(4..8) == Some(b"hdlr") { 0 } else { 4.min(b.len()) };
        let (kids, tail) = parse_kids(&b[skip..]);
        return Atom { kind, body: Body::Kids { head: b[..skip].to_vec(), kids, tail } };
    }
    Atom { kind, body: Body::Data(b.to_vec()) }
}

impl Atom {
    fn data(kind: &Kind, b: Vec<u8>) -> Atom {
        Atom { kind: *kind, body: Body::Data(b) }
    }
    fn parent(kind: &Kind, head: Vec<u8>, kids: Vec<Atom>) -> Atom {
        Atom { kind: *kind, body: Body::Kids { head, kids, tail: Vec::new() } }
    }
    fn write(&self, out: &mut Vec<u8>) {
        let start = out.len();
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&self.kind);
        match &self.body {
            Body::Data(b) => out.extend_from_slice(b),
            Body::Kids { head, kids, tail } => {
                out.extend_from_slice(head);
                for k in kids {
                    k.write(out);
                }
                out.extend_from_slice(tail);
            }
        }
        let size = (out.len() - start) as u32;
        out[start..start + 4].copy_from_slice(&size.to_be_bytes());
    }
    fn bytes(&self) -> &[u8] {
        match &self.body {
            Body::Data(b) => b,
            Body::Kids { head, .. } => head,
        }
    }
    fn kids(&self) -> &[Atom] {
        match &self.body {
            Body::Kids { kids, .. } => kids,
            Body::Data(_) => &[],
        }
    }
    fn kids_mut(&mut self) -> Option<&mut Vec<Atom>> {
        match &mut self.body {
            Body::Kids { kids, .. } => Some(kids),
            Body::Data(_) => None,
        }
    }
    fn kid(&self, kind: &Kind) -> Option<&Atom> {
        self.kids().iter().find(|k| &k.kind == kind)
    }
    fn kid_mut(&mut self, kind: &Kind) -> Option<&mut Atom> {
        self.kids_mut()?.iter_mut().find(|k| &k.kind == kind)
    }
    fn walk(&self, path: &[&Kind]) -> Option<&Atom> {
        path.iter().try_fold(self, |a, k| a.kid(k))
    }
    fn set_data(&mut self, b: Vec<u8>) {
        self.body = Body::Data(b);
    }
}

#[derive(Clone, Copy)]
struct Sample {
    offset: u64,
    size: u32,
    dur: u32,
    cto: i32,
    desc: u32,
    sync: bool,
}

/// The track's place on the film's timeline: wait this many film ticks, then play from this moment of the track.
#[derive(Clone, Copy)]
struct Line {
    wait: u64,
    from: i64,
}

struct Track {
    at: usize,
    handler: Kind,
    scale: u32,
    samples: Vec<Sample>,
    dts: Vec<u64>,
    line: Option<Line>,
    codec: String,
    width: u32,
    height: u32,
    protected: bool,
    all_sync: bool,
}

impl Track {
    fn wait_s(&self, film_scale: u32) -> f64 {
        self.line.map_or(0.0, |l| l.wait as f64 / film_scale as f64)
    }
    fn from(&self) -> i64 {
        self.line.map_or(0, |l| l.from)
    }
    /// When a sample shows, in seconds on the film's timeline.
    fn shows_at(&self, i: usize, film_scale: u32) -> f64 {
        let s = &self.samples[i];
        self.wait_s(film_scale) + (self.dts[i] as i64 + s.cto as i64 - self.from()) as f64 / self.scale as f64
    }
    fn total(&self) -> u64 {
        self.samples.iter().map(|s| s.dur as u64).sum()
    }
    /// When the track ends, in seconds on the film's timeline.
    fn ends_at(&self, film_scale: u32) -> f64 {
        self.wait_s(film_scale) + (self.total() as i64 - self.from()) as f64 / self.scale as f64
    }
}

struct Key {
    time: f64,
    sample: usize,
}

pub struct Movie {
    ftyp: Option<Vec<u8>>,
    moov: Atom,
    /// Other top-level boxes worth keeping: where and how long.
    others: Vec<(u64, u64)>,
    tracks: Vec<Track>,
    film_scale: u32,
    fragmented: bool,
    main: Option<usize>,
    keys: Vec<Key>,
    duration: f64,
}

/// The sample tables. `file_len` bounds what the tables may claim: every sample is somewhere in the file, so a count
/// of fixed-size samples that couldn't fit in it is damage, and run lengths never reach past the last sample.
fn read_tables(stbl: &Atom, file_len: u64) -> Option<Vec<Sample>> {
    let table = |k: &Kind| stbl.kid(k).map(Atom::bytes);
    let stsz = table(b"stsz")?;
    let fixed = be32(stsz, 4)?;
    let n = be32(stsz, 8)? as usize;
    if !fixed_ok(n, fixed, stsz, file_len) {
        return None;
    }
    let mut out = limits::room(n).ok()?;
    for i in 0..n {
        out.push(Sample {
            offset: 0,
            size: if fixed != 0 { fixed } else { be32(stsz, 12 + i * 4)? },
            dur: 0,
            cto: 0,
            desc: 1,
            sync: false,
        });
    }
    // durations: a run is never longer than the samples left
    let stts = table(b"stts")?;
    let (mut i, mut at) = (0usize, 8usize);
    for _ in 0..be32(stts, 4)? {
        let (count, dur) = (be32(stts, at)?, be32(stts, at + 4)?);
        at += 8;
        let take = (count as usize).min(n - i);
        out[i..i + take].iter_mut().for_each(|s| s.dur = dur);
        i += take;
    }
    // picture order
    if let Some(ctts) = table(b"ctts") {
        let signed = ctts.first() == Some(&1);
        let (mut i, mut at) = (0usize, 8usize);
        for _ in 0..be32(ctts, 4)? {
            let (count, raw) = (be32(ctts, at)?, be32(ctts, at + 4)?);
            at += 8;
            let cto = if signed { raw as i32 } else { i32::try_from(raw).ok()? };
            let take = (count as usize).min(n - i);
            out[i..i + take].iter_mut().for_each(|s| s.cto = cto);
            i += take;
        }
    }
    // where each one is
    let stsc = table(b"stsc")?;
    let offsets: Vec<u64> = if let Some(co) = table(b"stco") {
        (0..be32(co, 4)? as usize).map(|i| be32(co, 8 + i * 4).map(u64::from)).collect::<Option<_>>()?
    } else {
        let co = table(b"co64")?;
        (0..be32(co, 4)? as usize).map(|i| be64(co, 8 + i * 8)).collect::<Option<_>>()?
    };
    let runs = be32(stsc, 4)? as usize;
    let (mut i, mut run) = (0usize, 0usize);
    for (c, chunk_at) in offsets.iter().enumerate() {
        while run + 1 < runs && (c as u32 + 1) >= be32(stsc, 8 + (run + 1) * 12)? {
            run += 1;
        }
        if runs == 0 {
            break;
        }
        let (count, desc) = (be32(stsc, 8 + run * 12 + 4)?, be32(stsc, 8 + run * 12 + 8)?);
        let mut pos = *chunk_at;
        for _ in 0..count {
            if i >= n {
                break;
            }
            out[i].offset = pos;
            out[i].desc = desc;
            pos += out[i].size as u64;
            i += 1;
        }
    }
    if i != n {
        return None;
    }
    // what can start a picture
    match table(b"stss") {
        Some(stss) => {
            for k in 0..be32(stss, 4)? as usize {
                let at = be32(stss, 8 + k * 4)? as usize;
                if at >= 1 && at <= n {
                    out[at - 1].sync = true;
                }
            }
        }
        None => out.iter_mut().for_each(|s| s.sync = true),
    }
    Some(out)
}

fn fixed_ok(n: usize, fixed: u32, stsz: &[u8], file_len: u64) -> bool {
    if fixed != 0 {
        limits::fits(n as u64, fixed as u64, file_len)
    } else {
        limits::fits(n as u64, 4, stsz.len().saturating_sub(12) as u64)
    }
}

fn read_line(trak: &Atom) -> Option<Line> {
    let Some(elst) = trak.walk(&[b"edts", b"elst"]) else { return Some(Line { wait: 0, from: 0 }) };
    let b = elst.bytes();
    let wide = b.first() == Some(&1);
    let n = be32(b, 4)? as usize;
    let mut line = Line { wait: 0, from: 0 };
    let mut played = false;
    for e in 0..n {
        let (dur, from, rate) = if wide {
            let at = 8 + e * 20;
            (be64(b, at)?, be64(b, at + 8)? as i64, be32(b, at + 16)?)
        } else {
            let at = 8 + e * 12;
            (be32(b, at)? as u64, be32(b, at + 4)? as i32 as i64, be32(b, at + 8)?)
        };
        if from == -1 {
            if played {
                return None;
            }
            line.wait += dur;
            continue;
        }
        // more than one stretch, or anything but normal speed
        if played || rate != 0x0001_0000 {
            return None;
        }
        line.from = from;
        played = true;
    }
    Some(line)
}

struct Trex {
    id: u32,
    desc: u32,
    dur: u32,
    size: u32,
    flags: u32,
}

fn read_fragment(
    moof: &Atom,
    moof_at: u64,
    ids: &[(u32, usize)],
    trex: &[Trex],
    tracks: &mut [Vec<Sample>],
    next_dts: &mut [u64],
    file_len: u64,
) -> Option<()> {
    let mut running = moof_at;
    for traf in moof.kids().iter().filter(|k| &k.kind == b"traf") {
        let tfhd = traf.kid(b"tfhd")?.bytes();
        let flags = be32(tfhd, 0)? & 0xFF_FFFF;
        let id = be32(tfhd, 4)?;
        let slot = ids.iter().find(|(i, _)| *i == id)?.1;
        let ex = trex.iter().find(|t| t.id == id);
        let mut at = 8;
        let mut base = if flags & 0x20000 != 0 { moof_at } else { running };
        if flags & 1 != 0 {
            base = be64(tfhd, at)?;
            at += 8;
        }
        let mut field = |bit: u32, fallback: u32| -> Option<u32> {
            if flags & bit != 0 {
                let v = be32(tfhd, at)?;
                at += 4;
                Some(v)
            } else {
                Some(fallback)
            }
        };
        let desc = field(2, ex.map_or(1, |t| t.desc))?;
        let def_dur = field(8, ex.map_or(0, |t| t.dur))?;
        let def_size = field(0x10, ex.map_or(0, |t| t.size))?;
        let def_flags = field(0x20, ex.map_or(0, |t| t.flags))?;
        if let Some(tfdt) = traf.kid(b"tfdt") {
            let b = tfdt.bytes();
            let dts = if b.first() == Some(&1) { be64(b, 4)? } else { be32(b, 4)? as u64 };
            if tracks[slot].is_empty() {
                next_dts[slot] = dts;
            }
        }
        let mut pos = base;
        for trun in traf.kids().iter().filter(|k| &k.kind == b"trun") {
            let b = trun.bytes();
            let tf = be32(b, 0)? & 0xFF_FFFF;
            let n = be32(b, 4)?;
            let mut at = 8;
            if tf & 1 != 0 {
                pos = (base as i64 + be32(b, at)? as i32 as i64) as u64;
                at += 4;
            }
            let mut first_flags = None;
            if tf & 4 != 0 {
                first_flags = Some(be32(b, at)?);
                at += 4;
            }
            // the count is the file's word: with fields per sample they have to be in the box; without, the samples
            // have to fit in the file
            let per = 4 * [0x100u32, 0x200, 0x400, 0x800].iter().filter(|bit| tf & **bit != 0).count() as u64;
            let fits = if per > 0 {
                limits::fits(n as u64, per, b.len().saturating_sub(at) as u64)
            } else {
                limits::fits(n as u64, def_size as u64, file_len)
            };
            if !fits || tracks[slot].try_reserve(n as usize).is_err() {
                return None;
            }
            for i in 0..n {
                let mut take = |bit: u32| -> Option<Option<u32>> {
                    if tf & bit != 0 {
                        let v = be32(b, at)?;
                        at += 4;
                        Some(Some(v))
                    } else {
                        Some(None)
                    }
                };
                let dur = take(0x100)?.unwrap_or(def_dur);
                let size = take(0x200)?.unwrap_or(def_size);
                let sflags = take(0x400)?.unwrap_or(if i == 0 { first_flags.unwrap_or(def_flags) } else { def_flags });
                let cto = take(0x800)?.unwrap_or(0) as i32;
                tracks[slot].push(Sample { offset: pos, size, dur, cto, desc, sync: sflags & 0x1_0000 == 0 });
                pos += size as u64;
            }
        }
        running = pos;
    }
    Some(())
}

fn fourcc(b: &[u8]) -> String {
    b.iter().map(|c| *c as char).collect::<String>().trim_end().to_string()
}

/// Seconds worth of ticks, rounded to the nearest.
fn ticks(seconds: f64, scale: u32) -> i64 {
    (seconds * scale as f64).round() as i64
}

impl Movie {
    pub fn open(path: &Path) -> io::Result<Movie> {
        let bad = |why: &str| io::Error::new(io::ErrorKind::InvalidData, why.to_string());
        let mut f = File::open(path)?;
        let len = f.metadata()?.len();
        let (mut at, mut ftyp, mut moov_raw) = (0u64, None, None);
        let mut others = Vec::new();
        let mut moofs: Vec<(u64, u64, u64)> = Vec::new();
        while at + 8 <= len {
            f.seek(SeekFrom::Start(at))?;
            let mut h = [0u8; 16];
            f.read_exact(&mut h[..8])?;
            let mut size = u32::from_be_bytes(h[..4].try_into().unwrap()) as u64;
            let kind: Kind = h[4..8].try_into().unwrap();
            let mut head = 8u64;
            if size == 1 {
                f.read_exact(&mut h[8..16])?;
                size = u64::from_be_bytes(h[8..16].try_into().unwrap());
                head = 16;
            } else if size == 0 {
                size = len - at;
            }
            if size < head {
                break;
            }
            let body = size.saturating_sub(head).min(len - at - head);
            match &kind {
                b"ftyp" | b"moov" => {
                    if body > LARGEST_MOOV {
                        return Err(bad("oversized header"));
                    }
                    let b = limits::read_declared(&mut f, body, len - at - head)?;
                    if &kind == b"ftyp" { ftyp = Some(b) } else { moov_raw = Some(b) }
                }
                b"moof" => moofs.push((at, at + head, body)),
                b"mdat" | b"free" | b"skip" | b"wide" | b"sidx" | b"mfra" | b"styp" | b"ssix" | b"prft" | b"pnot" => {}
                _ => others.push((at, size.min(len - at))),
            }
            at = at.saturating_add(size);
        }
        let moov = parse_atom(*b"moov", &moov_raw.ok_or_else(|| bad("no moov"))?);
        let mvhd = moov.kid(b"mvhd").ok_or_else(|| bad("no mvhd"))?.bytes();
        let film_scale = if mvhd.first() == Some(&1) { be32(mvhd, 20) } else { be32(mvhd, 12) }
            .filter(|s| *s > 0)
            .ok_or_else(|| bad("no scale"))?;
        let mut tracks = Vec::new();
        for (at, trak) in moov.kids().iter().enumerate().filter(|(_, k)| &k.kind == b"trak") {
            let track = (|| -> Option<Track> {
                let mdia = trak.kid(b"mdia")?;
                let mdhd = mdia.kid(b"mdhd")?.bytes();
                let scale =
                    if mdhd.first() == Some(&1) { be32(mdhd, 20) } else { be32(mdhd, 12) }.filter(|s| *s > 0)?;
                let handler: Kind = mdia.kid(b"hdlr")?.bytes().get(8..12)?.try_into().ok()?;
                let stbl = mdia.walk(&[b"minf", b"stbl"])?;
                let stsd = stbl.kid(b"stsd")?.bytes();
                let codec = stsd.get(12..16).map(fourcc).unwrap_or_default();
                let (width, height) = if &handler == b"vide" {
                    (be16(stsd, 8 + 32).unwrap_or(0) as u32, be16(stsd, 8 + 34).unwrap_or(0) as u32)
                } else {
                    (0, 0)
                };
                let all_sync = stbl.kid(b"stss").is_none();
                Some(Track {
                    at,
                    handler,
                    scale,
                    samples: read_tables(stbl, len)?,
                    dts: Vec::new(),
                    line: read_line(trak),
                    protected: codec == "encv" || codec == "enca",
                    codec,
                    width,
                    height,
                    all_sync,
                })
            })();
            tracks.push(track.ok_or_else(|| bad("track"))?);
        }
        // a file in fragments keeps its samples in boxes further on
        let fragmented = !moofs.is_empty();
        if fragmented {
            let ids: Vec<(u32, usize)> = tracks
                .iter()
                .enumerate()
                .filter_map(|(i, t)| {
                    let tkhd = moov.kids()[t.at].kid(b"tkhd")?.bytes();
                    Some((if tkhd.first() == Some(&1) { be32(tkhd, 20)? } else { be32(tkhd, 12)? }, i))
                })
                .collect();
            let trex: Vec<Trex> = moov
                .kid(b"mvex")
                .map(|m| {
                    m.kids()
                        .iter()
                        .filter(|k| &k.kind == b"trex")
                        .filter_map(|k| {
                            let b = k.bytes();
                            Some(Trex {
                                id: be32(b, 4)?,
                                desc: be32(b, 8)?,
                                dur: be32(b, 12)?,
                                size: be32(b, 16)?,
                                flags: be32(b, 20)?,
                            })
                        })
                        .collect()
                })
                .unwrap_or_default();
            let mut lists: Vec<Vec<Sample>> = tracks.iter_mut().map(|t| std::mem::take(&mut t.samples)).collect();
            let mut first = vec![0u64; tracks.len()];
            for (moof_at, body_at, body) in &moofs {
                if *body > LARGEST_MOOV {
                    return Err(bad("oversized fragment"));
                }
                f.seek(SeekFrom::Start(*body_at))?;
                let b = limits::read_declared(&mut f, *body, len.saturating_sub(*body_at))?;
                read_fragment(&parse_atom(*b"moof", &b), *moof_at, &ids, &trex, &mut lists, &mut first, len)
                    .ok_or_else(|| bad("fragment"))?;
            }
            for (i, list) in lists.into_iter().enumerate() {
                tracks[i].samples = list;
                tracks[i].all_sync = false;
                let mut d = first[i];
                tracks[i].dts = tracks[i]
                    .samples
                    .iter()
                    .map(|s| {
                        let at = d;
                        d += s.dur as u64;
                        at
                    })
                    .collect();
            }
        } else {
            for t in &mut tracks {
                let mut d = 0u64;
                t.dts = t
                    .samples
                    .iter()
                    .map(|s| {
                        let at = d;
                        d += s.dur as u64;
                        at
                    })
                    .collect();
            }
        }
        let is_picture =
            |t: &Track| t.samples.len() <= 1 && matches!(t.codec.as_str(), "jpeg" | "png" | "mjpa" | "mjpg");
        let main = tracks.iter().position(|t| &t.handler == b"vide" && !t.samples.is_empty() && !is_picture(t));
        let mut duration =
            tracks.iter().filter(|t| !t.samples.is_empty()).map(|t| t.ends_at(film_scale)).fold(0.0, f64::max);
        let mut keys = Vec::new();
        if let Some(m) = main {
            let t = &tracks[m];
            duration = duration.max(t.ends_at(film_scale));
            keys = t
                .samples
                .iter()
                .enumerate()
                .filter(|(_, s)| s.sync)
                .map(|(i, _)| Key { time: t.shows_at(i, film_scale), sample: i })
                .filter(|k| k.time > -0.001)
                .collect();
            keys.sort_by(|a, b| a.time.total_cmp(&b.time));
        }
        Ok(Movie { ftyp, moov, others, tracks, film_scale, fragmented, main, keys, duration })
    }

    fn sound(&self) -> Option<&Track> {
        self.tracks.iter().find(|t| &t.handler == b"soun")
    }

    fn sealed(&self) -> bool {
        self.tracks.iter().any(|t| t.protected)
    }

    fn items(&self) -> Option<&Atom> {
        self.moov.walk(&[b"udta", b"meta", b"ilst"])
    }

    fn title(&self) -> Option<String> {
        let item = self
            .items()
            .and_then(|i| i.kid(&NAME))
            .and_then(|n| n.kid(b"data"))
            .and_then(|d| d.bytes().get(8..))
            .map(|b| String::from_utf8_lossy(b).into_owned());
        // the older QuickTime spelling: a length, a language, then the words
        let old = || {
            let b = self.moov.walk(&[b"udta", &NAME])?.bytes();
            let n = be16(b, 0)? as usize;
            Some(String::from_utf8_lossy(b.get(4..4 + n)?).into_owned())
        };
        item.or_else(old).filter(|t| !t.trim().is_empty())
    }

    pub fn cover(&self) -> Option<Vec<u8>> {
        let d = self.items()?.kid(b"covr")?.kid(b"data")?.bytes().get(8..)?;
        Some(d.to_vec())
    }

    pub fn facts(&self) -> Facts {
        let video = self.main.map(|m| &self.tracks[m]);
        let sealed = self.sealed();
        Facts {
            container: "mp4",
            video: video.map(|t| t.codec.clone()),
            audio: self.sound().map(|t| t.codec.clone()),
            width: video.map_or(0, |t| t.width),
            height: video.map_or(0, |t| t.height),
            duration: self.duration,
            keys: self.keys.iter().map(|k| k.time).collect(),
            title: self.title(),
            has_cover: self.items().and_then(|i| i.kid(b"covr")).is_some(),
            can_trim: !sealed && video.is_some() && self.tracks.iter().all(|t| t.line.is_some()),
            can_tag: !sealed,
            system: false,
        }
    }

    pub fn plan(&self, edit: &Edit) -> Result<Plan, String> {
        let cut_from = edit.from_key.filter(|k| *k > 0 && *k < self.keys.len());
        let cut_to = edit.to_key.filter(|k| *k < self.keys.len());
        if let (Some(a), Some(b)) = (cut_from, cut_to) {
            if b <= a {
                return Err("the end of a cut has to come after its start".into());
            }
        }
        let cutting = cut_from.is_some() || cut_to.is_some();
        let facts = self.facts();
        if (cutting && !facts.can_trim) || !facts.can_tag {
            return Err("this file's structure doesn't allow it".into());
        }
        let a = cut_from.map_or(0.0, |k| self.keys[k].time);
        let b = cut_to.map_or(self.duration, |k| self.keys[k].time);
        let main = self.main;
        let keep: Vec<Option<Keep>> = self
            .tracks
            .iter()
            .enumerate()
            .map(|(i, t)| {
                if !cutting {
                    return (!t.samples.is_empty() || !self.fragmented).then_some(Keep {
                        first: 0,
                        last: t.samples.len(),
                        line: None,
                        cto_shift: 0,
                        clipped: None,
                    });
                }
                let (first, last) = if Some(i) == main {
                    let last = cut_to.map_or(t.samples.len(), |k| {
                        // with open groups of pictures, frames shown before the end keyframe are stored after it, and need it to be drawn.
                        // They're all kept, and the edit list stops the film before the keyframe itself shows
                        let at = self.keys[k].sample;
                        (at + 1..t.samples.len().min(at + 64))
                            .rev()
                            .find(|s| t.shows_at(*s, self.film_scale) < b - 1e-6)
                            .map_or(at, |s| s + 1)
                    });
                    (cut_from.map_or(0, |k| self.keys[k].sample), last)
                } else {
                    let n = t.samples.len();
                    let ends = |s: usize| t.shows_at(s, self.film_scale) + t.samples[s].dur as f64 / t.scale as f64;
                    let mut first = (0..n).find(|s| ends(*s) > a + 1e-6).unwrap_or(n);
                    // sound decoders use the sample before to settle; the edit list keeps it silent
                    if &t.handler == b"soun" {
                        first = first.saturating_sub(1);
                    }
                    if &t.handler == b"vide" {
                        while first > 0 && !t.samples[first].sync {
                            first -= 1;
                        }
                    }
                    (first, (first..n).find(|s| t.shows_at(*s, self.film_scale) >= b - 1e-6).unwrap_or(n))
                };
                if first >= last {
                    return None;
                }
                let wait = t.wait_s(self.film_scale);
                // words and chapters last as long as they say, and readers of chapters don't all follow edit lists:
                // so their first and last samples are made shorter instead. What they hold isn't touched
                if matches!(&t.handler, b"text" | b"sbtl" | b"subt") {
                    let mut kept = t.samples[first..last].to_vec();
                    let starts = t.shows_at(first, self.film_scale);
                    let n = kept.len();
                    let over_end =
                        (t.shows_at(last - 1, self.film_scale) + t.samples[last - 1].dur as f64 / t.scale as f64 - b)
                            .max(0.0);
                    kept[n - 1].dur = kept[n - 1].dur.saturating_sub(ticks(over_end, t.scale) as u32);
                    kept[0].dur = kept[0].dur.saturating_sub(ticks((a - starts).max(0.0), t.scale) as u32);
                    let held: u64 = kept.iter().map(|s| s.dur as u64).sum();
                    let delay = (starts - a).max(0.0);
                    return Some(Keep {
                        first,
                        last,
                        line: Some(NewLine {
                            delay: ticks(delay, self.film_scale) as u64,
                            from: 0,
                            plays: ticks(held as f64 / t.scale as f64, self.film_scale) as u64,
                        }),
                        cto_shift: 0,
                        clipped: Some(kept),
                    });
                }
                // where the film's new start falls in this track, counted from the first sample kept
                let delay = (wait - a).max(0.0);
                let start_at = if Some(i) == main && cut_from.is_some() {
                    t.dts[first] as i64 + t.samples[first].cto as i64
                } else {
                    t.from() + ticks((a - wait).max(0.0), t.scale)
                };
                let from = start_at - t.dts[first] as i64;
                let cto_shift = (-from).max(0) as i32;
                let plays = (b.min(t.ends_at(self.film_scale)) - a.max(wait)).max(0.0);
                Some(Keep {
                    first,
                    last,
                    line: Some(NewLine {
                        delay: ticks(delay, self.film_scale) as u64,
                        from: from.max(0),
                        plays: ticks(plays, self.film_scale) as u64,
                    }),
                    cto_shift,
                    clipped: None,
                })
            })
            .collect();

        // chunks, in time order, so sound and picture sit side by side in the new file
        let mut chunks: Vec<Chunk> = Vec::new();
        for (ti, k) in keep.iter().enumerate() {
            let Some(k) = k else { continue };
            let t = &self.tracks[ti];
            let limit = (CHUNK_SECONDS * t.scale as f64) as u64;
            let mut i = k.first;
            while i < k.last {
                let (start, desc) = (i, t.samples[i].desc);
                let (mut held, mut bytes) = (0u64, 0u64);
                while i < k.last && t.samples[i].desc == desc && (i == start || held < limit) {
                    held += t.samples[i].dur as u64;
                    bytes += t.samples[i].size as u64;
                    i += 1;
                }
                chunks.push(Chunk {
                    track: ti,
                    first: start,
                    count: i - start,
                    at: t.dts[start] as f64 / t.scale as f64,
                    bytes,
                    offset: 0,
                });
            }
        }
        chunks.sort_by(|x, y| x.at.total_cmp(&y.at).then(x.track.cmp(&y.track)));
        let payload: u64 = chunks.iter().map(|c| c.bytes).sum();

        let mut moov = self.moov.clone();
        self.tag(&mut moov, edit);
        moov.kids_mut().unwrap().retain(|k| &k.kind != b"mvex");
        // a second list of chapters, in the old film's times
        if cutting {
            if let Some(list) = moov.kid_mut(b"udta").and_then(Atom::kids_mut) {
                list.retain(|k| &k.kind != b"chpl");
            }
        }
        let wide = payload > u32::MAX as u64 - (1 << 26);
        let big_mdat = payload + 8 > u32::MAX as u64;
        let ftyp_len = self.ftyp.as_ref().map_or(0, |b| b.len() as u64 + 8);
        let others_len: u64 = self.others.iter().map(|o| o.1).sum();
        let mdat_head = if big_mdat { 16 } else { 8 };
        // once to learn the size, once with the real places: the size doesn't depend on them
        let first = self.build(&moov, &keep, &chunks, wide, cutting)?;
        let mut pos = ftyp_len + first.len() as u64 + others_len + mdat_head;
        for c in &mut chunks {
            c.offset = pos;
            pos += c.bytes;
        }
        let moov_bytes = self.build(&moov, &keep, &chunks, wide, cutting)?;
        debug_assert_eq!(first.len(), moov_bytes.len());

        let mut plan = Plan::default();
        if let Some(b) = &self.ftyp {
            let mut out = Vec::new();
            Atom::data(b"ftyp", b.clone()).write(&mut out);
            plan.bytes(out);
        }
        plan.bytes(moov_bytes);
        for (at, len) in &self.others {
            plan.file(*at, *len);
        }
        if big_mdat {
            let mut h = vec![0, 0, 0, 1];
            h.extend_from_slice(b"mdat");
            h.extend_from_slice(&(payload + 16).to_be_bytes());
            plan.bytes(h);
        } else {
            let mut h = ((payload + 8) as u32).to_be_bytes().to_vec();
            h.extend_from_slice(b"mdat");
            plan.bytes(h);
        }
        for c in &chunks {
            for s in &self.tracks[c.track].samples[c.first..c.first + c.count] {
                plan.file(s.offset, s.size as u64);
            }
        }
        Ok(plan)
    }

    fn tag(&self, moov: &mut Atom, edit: &Edit) {
        if edit.title.is_none() && edit.cover.is_none() {
            return;
        }
        let kids = moov.kids_mut().unwrap();
        if !kids.iter().any(|k| &k.kind == b"udta") {
            kids.push(Atom::parent(b"udta", Vec::new(), Vec::new()));
        }
        let udta = moov.kid_mut(b"udta").unwrap();
        let list = udta.kids_mut().unwrap();
        if !list.iter().any(|k| &k.kind == b"meta") {
            let mut hdlr = vec![0u8; 8];
            hdlr.extend_from_slice(b"mdirappl");
            hdlr.extend_from_slice(&[0u8; 9]);
            list.push(Atom::parent(b"meta", vec![0; 4], vec![Atom::data(b"hdlr", hdlr)]));
        }
        // the older spelling would disagree with the new title
        if edit.title.is_some() {
            list.retain(|k| k.kind != NAME);
        }
        let meta = udta.kid_mut(b"meta").unwrap();
        let mkids = meta.kids_mut().unwrap();
        if !mkids.iter().any(|k| &k.kind == b"ilst") {
            mkids.push(Atom::parent(b"ilst", Vec::new(), Vec::new()));
        }
        let items = meta.kid_mut(b"ilst").unwrap().kids_mut().unwrap();
        let item = |kind: Kind, flags: u32, bytes: &[u8]| {
            let mut d = flags.to_be_bytes().to_vec();
            d.extend_from_slice(&[0; 4]);
            d.extend_from_slice(bytes);
            Atom::parent(&kind, Vec::new(), vec![Atom::data(b"data", d)])
        };
        if let Some(t) = &edit.title {
            items.retain(|k| k.kind != NAME);
            if !t.trim().is_empty() {
                items.insert(0, item(NAME, 1, t.as_bytes()));
            }
        }
        if let Some(Cover { bytes, png }) = &edit.cover {
            items.retain(|k| &k.kind != b"covr");
            items.push(item(*b"covr", if *png { 14 } else { 13 }, bytes));
        }
    }

    fn build(
        &self,
        moov: &Atom,
        keep: &[Option<Keep>],
        chunks: &[Chunk],
        wide: bool,
        cutting: bool,
    ) -> Result<Vec<u8>, String> {
        let mut out_kids: Vec<Atom> = Vec::new();
        let mut trak_n = 0usize;
        let mut film_len = 0u64;
        for kid in moov.kids() {
            if &kid.kind != b"trak" {
                out_kids.push(kid.clone());
                continue;
            }
            let ti = trak_n;
            trak_n += 1;
            let Some(k) = &keep[ti] else { continue };
            let t = &self.tracks[ti];
            let mut trak = kid.clone();
            let samples = k.clipped.as_deref().unwrap_or(&t.samples[k.first..k.last]);
            let media_len: u64 = samples.iter().map(|s| s.dur as u64).sum();
            let stbl = trak
                .kid_mut(b"mdia")
                .and_then(|m| m.kid_mut(b"minf"))
                .and_then(|m| m.kid_mut(b"stbl"))
                .ok_or("no sample table")?;
            let offsets: Vec<&Chunk> = chunks.iter().filter(|c| c.track == ti).collect();
            let old = stbl.kids().to_vec();
            let kids = stbl.kids_mut().unwrap();
            kids.clear();
            let is_table =
                |k: &Kind| matches!(k, b"stts" | b"ctts" | b"stss" | b"stsc" | b"stsz" | b"stz2" | b"stco" | b"co64");
            for a in old {
                if &a.kind == b"stsd" || (!cutting && !self.fragmented && !is_table(&a.kind)) {
                    kids.push(a);
                }
            }
            kids.push(Atom::data(b"stts", stts(samples)));
            if let Some(b) = ctts(samples, k.cto_shift) {
                kids.push(Atom::data(b"ctts", b));
            }
            if !(t.all_sync || samples.iter().all(|s| s.sync)) {
                kids.push(Atom::data(b"stss", stss(samples)));
            }
            kids.push(Atom::data(b"stsc", stsc(t, &offsets)));
            kids.push(Atom::data(b"stsz", stsz(samples)));
            kids.push(if wide { Atom::data(b"co64", co64(&offsets)) } else { Atom::data(b"stco", stco(&offsets)) });
            // the track's own lengths
            if let Some(mdhd) = trak.kid_mut(b"mdia").and_then(|m| m.kid_mut(b"mdhd")) {
                let mut b = mdhd.bytes().to_vec();
                put_len(&mut b, 16, 24, media_len);
                mdhd.set_data(b);
            }
            let shown = if let Some(line) = &k.line {
                let list = trak.kids_mut().unwrap();
                list.retain(|a| &a.kind != b"edts");
                let at = list.iter().position(|a| &a.kind == b"tkhd").map_or(0, |p| p + 1);
                list.insert(at, Atom::parent(b"edts", Vec::new(), vec![Atom::data(b"elst", elst(line))]));
                line.delay + line.plays
            } else {
                // nothing cut: the track's original timing is kept as it was
                let worked_out = ticks(t.ends_at(self.film_scale), self.film_scale) as u64;
                if self.fragmented { worked_out } else { shown_len(kid).unwrap_or(worked_out) }
            };
            if let Some(tkhd) = trak.kid_mut(b"tkhd") {
                let mut b = tkhd.bytes().to_vec();
                put_len(&mut b, 20, 28, shown);
                tkhd.set_data(b);
            }
            film_len = film_len.max(shown);
            out_kids.push(trak);
        }
        for kid in &mut out_kids {
            if &kid.kind == b"mvhd" {
                let mut b = kid.bytes().to_vec();
                put_len(&mut b, 16, 24, film_len);
                kid.set_data(b);
            }
        }
        let mut out = Vec::new();
        Atom::parent(b"moov", Vec::new(), out_kids).write(&mut out);
        Ok(out)
    }
}

const NAME: Kind = [0xA9, b'n', b'a', b'm'];

struct NewLine {
    delay: u64,
    from: i64,
    plays: u64,
}

struct Keep {
    first: usize,
    last: usize,
    /// None when nothing is cut: the original edit list stays.
    line: Option<NewLine>,
    cto_shift: i32,
    /// The kept samples with new lengths, where those had to change.
    clipped: Option<Vec<Sample>>,
}

struct Chunk {
    track: usize,
    first: usize,
    count: usize,
    at: f64,
    bytes: u64,
    offset: u64,
}

fn shown_len(trak: &Atom) -> Option<u64> {
    let b = trak.kid(b"tkhd")?.bytes();
    if b.first() == Some(&1) { be64(b, 28) } else { be32(b, 20).map(u64::from) }
}

/// Writes a duration into a box that holds it at one place in version 0 and another, wider, in version 1.
fn put_len(b: &mut [u8], v0: usize, v1: usize, len: u64) {
    if b.first() == Some(&1) {
        if let Some(s) = b.get_mut(v1..v1 + 8) {
            s.copy_from_slice(&len.to_be_bytes());
        }
    } else if let Some(s) = b.get_mut(v0..v0 + 4) {
        s.copy_from_slice(&(len.min(u32::MAX as u64) as u32).to_be_bytes());
    }
}

fn runs(list: &mut Vec<u8>, count: u32) {
    list[4..8].copy_from_slice(&count.to_be_bytes());
}

fn stts(samples: &[Sample]) -> Vec<u8> {
    let mut out = vec![0u8; 8];
    let mut n = 0u32;
    let mut i = 0;
    while i < samples.len() {
        let d = samples[i].dur;
        let mut j = i + 1;
        while j < samples.len() && samples[j].dur == d {
            j += 1;
        }
        out.extend_from_slice(&((j - i) as u32).to_be_bytes());
        out.extend_from_slice(&d.to_be_bytes());
        n += 1;
        i = j;
    }
    runs(&mut out, n);
    out
}

fn ctts(samples: &[Sample], shift: i32) -> Option<Vec<u8>> {
    if samples.iter().all(|s| s.cto + shift == 0) {
        return None;
    }
    let signed = samples.iter().any(|s| s.cto + shift < 0);
    let mut out = vec![signed as u8, 0, 0, 0, 0, 0, 0, 0];
    let mut n = 0u32;
    let mut i = 0;
    while i < samples.len() {
        let c = samples[i].cto;
        let mut j = i + 1;
        while j < samples.len() && samples[j].cto == c {
            j += 1;
        }
        out.extend_from_slice(&((j - i) as u32).to_be_bytes());
        out.extend_from_slice(&(c + shift).to_be_bytes());
        n += 1;
        i = j;
    }
    runs(&mut out, n);
    Some(out)
}

fn stss(samples: &[Sample]) -> Vec<u8> {
    let mut out = vec![0u8; 8];
    let mut n = 0u32;
    for (i, s) in samples.iter().enumerate() {
        if s.sync {
            out.extend_from_slice(&(i as u32 + 1).to_be_bytes());
            n += 1;
        }
    }
    runs(&mut out, n);
    out
}

fn stsz(samples: &[Sample]) -> Vec<u8> {
    let mut out = vec![0u8; 4];
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&(samples.len() as u32).to_be_bytes());
    for s in samples {
        out.extend_from_slice(&s.size.to_be_bytes());
    }
    out
}

fn stsc(t: &Track, chunks: &[&Chunk]) -> Vec<u8> {
    let mut out = vec![0u8; 8];
    let mut n = 0u32;
    let mut last = (0u32, 0u32);
    for (i, c) in chunks.iter().enumerate() {
        let now = (c.count as u32, t.samples[c.first].desc);
        if now != last {
            out.extend_from_slice(&(i as u32 + 1).to_be_bytes());
            out.extend_from_slice(&now.0.to_be_bytes());
            out.extend_from_slice(&now.1.to_be_bytes());
            n += 1;
            last = now;
        }
    }
    runs(&mut out, n);
    out
}

fn stco(chunks: &[&Chunk]) -> Vec<u8> {
    let mut out = vec![0u8; 8];
    for c in chunks {
        out.extend_from_slice(&(c.offset as u32).to_be_bytes());
    }
    runs(&mut out, chunks.len() as u32);
    out
}

fn co64(chunks: &[&Chunk]) -> Vec<u8> {
    let mut out = vec![0u8; 8];
    for c in chunks {
        out.extend_from_slice(&c.offset.to_be_bytes());
    }
    runs(&mut out, chunks.len() as u32);
    out
}

fn elst(line: &NewLine) -> Vec<u8> {
    let rate = 0x0001_0000u32.to_be_bytes();
    let wide = line.delay > u32::MAX as u64 || line.plays > u32::MAX as u64 || line.from > i32::MAX as i64;
    let mut out = vec![wide as u8, 0, 0, 0];
    out.extend_from_slice(&(if line.delay > 0 { 2u32 } else { 1u32 }).to_be_bytes());
    let mut entry = |dur: u64, from: i64| {
        if wide {
            out.extend_from_slice(&dur.to_be_bytes());
            out.extend_from_slice(&from.to_be_bytes());
        } else {
            out.extend_from_slice(&(dur as u32).to_be_bytes());
            out.extend_from_slice(&(from as i32).to_be_bytes());
        }
        out.extend_from_slice(&rate);
    };
    if line.delay > 0 {
        entry(line.delay, -1);
    }
    entry(line.plays, line.from);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn boxed(kind: &Kind, body: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        Atom::data(kind, body.to_vec()).write(&mut out);
        out
    }
    fn words(w: &[u32]) -> Vec<u8> {
        w.iter().flat_map(|x| x.to_be_bytes()).collect()
    }
    fn stbl(parts: &[Vec<u8>]) -> Atom {
        parse_atom(*b"stbl", &parts.concat())
    }
    fn timed<T>(f: impl FnOnce() -> T) -> T {
        let t = std::time::Instant::now();
        let r = f();
        assert!(t.elapsed().as_secs() < 2, "took {:?}", t.elapsed());
        r
    }

    #[test]
    fn four_billion_fixed_size_samples_in_a_small_file_are_refused() {
        // stsz: version/flags, every sample 1 byte, 0xFFFFFFFF of them; the file is 1000 bytes
        let t = stbl(&[boxed(b"stsz", &words(&[0, 1, 0xFFFF_FFFF]))]);
        assert!(timed(|| read_tables(&t, 1000)).is_none());
        // and a table of sizes shorter than its count
        let t = stbl(&[boxed(b"stsz", &words(&[0, 0, 1000, 10]))]);
        assert!(read_tables(&t, 1 << 30).is_none());
    }

    #[test]
    fn a_run_longer_than_the_samples_stops_at_the_last_sample() {
        let t = stbl(&[
            boxed(b"stsz", &words(&[0, 0, 2, 10, 10])),
            // one run of four billion durations, one of four billion picture offsets
            boxed(b"stts", &words(&[0, 1, 0xFFFF_FFFF, 5])),
            boxed(b"ctts", &words(&[0, 1, 0xFFFF_FFFF, 3])),
            // both samples in one chunk at byte 100
            boxed(b"stsc", &words(&[0, 1, 1, 2, 1])),
            boxed(b"stco", &words(&[0, 1, 100])),
        ]);
        let samples = timed(|| read_tables(&t, 1000)).expect("the table reads");
        assert_eq!(samples.len(), 2);
        assert!(samples.iter().all(|s| s.dur == 5 && s.cto == 3));
        assert_eq!((samples[0].offset, samples[1].offset), (100, 110));
    }

    #[test]
    fn a_fragment_claiming_four_billion_samples_is_refused() {
        // traf: tfhd (no optional fields, track 1), trun with no per-sample fields and 0xFFFFFFFF samples
        let traf = [boxed(b"tfhd", &words(&[0, 1])), boxed(b"trun", &words(&[0, 0xFFFF_FFFF]))].concat();
        let mut moof_body = Vec::new();
        Atom::parent(b"traf", Vec::new(), parse_kids(&traf).0).write(&mut moof_body);
        let moof = parse_atom(*b"moof", &moof_body);
        let mut lists = vec![Vec::new()];
        let mut first = vec![0u64];
        assert!(timed(|| read_fragment(&moof, 0, &[(1, 0)], &[], &mut lists, &mut first, 1000)).is_none());
        assert!(lists[0].is_empty());
    }
}
