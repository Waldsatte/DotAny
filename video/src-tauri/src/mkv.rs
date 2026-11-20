//! MKV and WebM: only the elements around the blocks. Blocks are copied unread, apart from the two bytes that say when a block
//! shows, and only in the one cluster that starts before a cut.
//! A new file is laid out as: header, seek table, info, tracks, chapters, attachments, tags, clusters, cues.

use crate::formats::Container;
use crate::media::{Cover, Edit, Facts, Plan};
use dotany_base::limits;
use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

const EBML: u32 = 0x1A45_DFA3;
const SEGMENT: u32 = 0x1853_8067;
const SEEK_HEAD: u32 = 0x114D_9B74;
const INFO: u32 = 0x1549_A966;
const TRACKS: u32 = 0x1654_AE6B;
const CHAPTERS: u32 = 0x1043_A770;
const CLUSTER: u32 = 0x1F43_B675;
const CUES: u32 = 0x1C53_BB6B;
const ATTACHMENTS: u32 = 0x1941_A469;
const TAGS: u32 = 0x1254_C367;
const VOID: u32 = 0xEC;
const CRC: u32 = 0xBF;
const TIMESTAMP: u32 = 0xE7;
const POSITION: u32 = 0xA7;
const PREV_SIZE: u32 = 0xAB;
const SIMPLE_BLOCK: u32 = 0xA3;
const BLOCK_GROUP: u32 = 0xA0;
const BLOCK: u32 = 0xA1;
const REFERENCE: u32 = 0xFB;
const DURATION: u32 = 0x4489;
const TITLE: u32 = 0x7BA9;
const SCALE: u32 = 0x2A_D7B1;
const ATTACHED_FILE: u32 = 0x61A7;
const FILE_NAME: u32 = 0x466E;
const FILE_MIME: u32 = 0x4660;
const FILE_DATA: u32 = 0x465C;
const FILE_UID: u32 = 0x46AE;

const LEVEL_ONE: [u32; 8] = [SEEK_HEAD, INFO, TRACKS, CHAPTERS, CLUSTER, CUES, ATTACHMENTS, TAGS];

fn read_id(r: &mut impl Read) -> io::Result<(u32, u64)> {
    let mut b = [0u8; 1];
    r.read_exact(&mut b)?;
    let n = b[0].leading_zeros() as usize + 1;
    if n > 4 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "bad element id"));
    }
    let mut id = b[0] as u32;
    for _ in 1..n {
        r.read_exact(&mut b)?;
        id = (id << 8) | b[0] as u32;
    }
    Ok((id, n as u64))
}

/// A length; None means "until something ends it".
fn read_size(r: &mut impl Read) -> io::Result<(Option<u64>, u64)> {
    let mut b = [0u8; 1];
    r.read_exact(&mut b)?;
    let n = b[0].leading_zeros() as usize + 1;
    if n > 8 {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "bad element size"));
    }
    let mut v = (b[0] as u64) & (0xFFu64 >> n);
    let mut all = v == (0xFFu64 >> n);
    for _ in 1..n {
        r.read_exact(&mut b)?;
        v = (v << 8) | b[0] as u64;
        all = all && b[0] == 0xFF;
    }
    Ok((if all { None } else { Some(v) }, n as u64))
}

/// Walks the elements in a piece already in memory.
fn children(b: &[u8]) -> Vec<(u32, &[u8])> {
    let mut out = Vec::new();
    let mut c = io::Cursor::new(b);
    while (c.position() as usize) < b.len() {
        let Ok((id, _)) = read_id(&mut c) else { break };
        let Ok((size, _)) = read_size(&mut c) else { break };
        let at = c.position() as usize;
        let end = size.map_or(b.len(), |s| (at as u64 + s).min(b.len() as u64) as usize);
        out.push((id, &b[at..end]));
        c.set_position(end as u64);
    }
    out
}

fn uint(b: &[u8]) -> u64 {
    b.iter().take(8).fold(0, |v, x| (v << 8) | *x as u64)
}

fn float(b: &[u8]) -> f64 {
    match b.len() {
        4 => f32::from_be_bytes(b.try_into().unwrap()) as f64,
        8 => f64::from_be_bytes(b.try_into().unwrap()),
        _ => 0.0,
    }
}

fn id_bytes(id: u32) -> Vec<u8> {
    let b = id.to_be_bytes();
    let skip = b.iter().position(|x| *x != 0).unwrap_or(3);
    b[skip..].to_vec()
}

/// A length in the widest spelling, so a size never depends on the value it holds.
fn wide_size(n: u64) -> [u8; 8] {
    let mut b = n.to_be_bytes();
    b[0] = 0x01;
    b
}

fn element(id: u32, body: &[u8]) -> Vec<u8> {
    let mut out = id_bytes(id);
    out.extend_from_slice(&wide_size(body.len() as u64));
    out.extend_from_slice(body);
    out
}

fn uint_element(id: u32, v: u64) -> Vec<u8> {
    element(id, &v.to_be_bytes())
}

struct ClusterAt {
    payload: u64,
    end: u64,
    time: u64,
}

struct Attached {
    head: Vec<u8>,
    data: (u64, u64),
    is_cover: bool,
}

pub struct Matroska {
    doc: Vec<u8>,
    webm: bool,
    info: Vec<u8>,
    tracks: Vec<u8>,
    chapters: Option<Vec<u8>>,
    tags: Option<Vec<u8>>,
    attached: Vec<Attached>,
    clusters: Vec<ClusterAt>,
    /// Keyframes of the video track, in the file's ticks.
    keys: Vec<u64>,
    /// Nanoseconds per tick.
    scale: u64,
    duration: u64,
    video_track: Option<u64>,
    video_codec: Option<String>,
    audio_codec: Option<String>,
    width: u32,
    height: u32,
    title: Option<String>,
    encrypted: bool,
    /// The Mac: where each Attachments element is (start, end): WKWebView's WebM player hangs on one that comes before
    /// the clusters (`attachment_places`).
    #[cfg(target_os = "macos")]
    attachment_places: Vec<(u64, u64)>,
}

struct BlockHead {
    track: u64,
    relative: i16,
    key: bool,
    /// Where the two bytes of `relative` sit in the file.
    relative_at: u64,
}

fn read_block_head(r: &mut (impl Read + Seek), at: u64) -> io::Result<BlockHead> {
    r.seek(SeekFrom::Start(at))?;
    let (track, n) = read_size(r)?;
    let mut b = [0u8; 3];
    r.read_exact(&mut b)?;
    Ok(BlockHead {
        track: track.unwrap_or(0),
        relative: i16::from_be_bytes([b[0], b[1]]),
        key: b[2] & 0x80 != 0,
        relative_at: at + n,
    })
}

/// One block in a cluster: the element's place and its head.
struct Found {
    start: u64,
    end: u64,
    head: BlockHead,
    key: bool,
}

/// Every block in a stretch of a cluster, without reading what's in them.
fn blocks_in(r: &mut (impl Read + Seek), from: u64, to: u64, mut on: impl FnMut(Found) -> bool) -> io::Result<()> {
    let mut at = from;
    while at < to {
        r.seek(SeekFrom::Start(at))?;
        let (id, a) = read_id(r)?;
        let (size, b) = read_size(r)?;
        let body = at + a + b;
        let end = body + size.unwrap_or(0);
        match id {
            SIMPLE_BLOCK => {
                let head = read_block_head(r, body)?;
                let key = head.key;
                if !on(Found { start: at, end, head, key }) {
                    return Ok(());
                }
            }
            BLOCK_GROUP => {
                // a group is a keyframe when nothing is said to come before it
                let (mut inner, mut block_at, mut refers) = (body, None, false);
                while inner < end {
                    r.seek(SeekFrom::Start(inner))?;
                    let (kid, x) = read_id(r)?;
                    let (kid_size, y) = read_size(r)?;
                    if kid == BLOCK {
                        block_at = Some(inner + x + y);
                    } else if kid == REFERENCE {
                        refers = true;
                    }
                    inner += x + y + kid_size.unwrap_or(0);
                }
                if let Some(b) = block_at {
                    let head = read_block_head(r, b)?;
                    if !on(Found { start: at, end, head, key: !refers }) {
                        return Ok(());
                    }
                }
            }
            _ => {}
        }
        at = end;
    }
    Ok(())
}

impl Matroska {
    pub fn open(path: &Path, kind: Container) -> io::Result<Matroska> {
        let bad = |why: &str| io::Error::new(io::ErrorKind::InvalidData, why.to_string());
        let mut r = BufReader::with_capacity(64 * 1024, File::open(path)?);
        let len = r.get_ref().metadata()?.len();
        let (id, _) = read_id(&mut r)?;
        if id != EBML {
            return Err(bad("no header"));
        }
        let (size, _) = read_size(&mut r)?;
        let here = r.stream_position()?;
        let doc = limits::read_declared(&mut r, size.ok_or_else(|| bad("header"))?, len.saturating_sub(here))?;
        let (id, _) = read_id(&mut r)?;
        if id != SEGMENT {
            return Err(bad("no segment"));
        }
        let (size, _) = read_size(&mut r)?;
        let segment_at = r.stream_position()?;
        let segment_end = size.map_or(len, |s| segment_at.saturating_add(s).min(len));

        let (mut info, mut tracks, mut chapters, mut tags, mut cues) = (None, None, None, None, None);
        let (mut attached, mut clusters) = (Vec::new(), Vec::new());
        #[cfg(target_os = "macos")]
        let mut attachment_places = Vec::new();
        let mut at = segment_at;
        // a live recording may not say how long a cluster is, so a cluster is walked only then
        while at < segment_end {
            r.seek(SeekFrom::Start(at))?;
            let Ok((id, a)) = read_id(&mut r) else { break };
            let Ok((size, b)) = read_size(&mut r) else { break };
            let body = at + a + b;
            if id == CLUSTER {
                let Some(c) = read_cluster(&mut r, body, size, segment_end)? else { break };
                at = c.end;
                clusters.push(c);
                continue;
            }
            let end = body.saturating_add(size.ok_or_else(|| bad("unknown size"))?).min(segment_end);
            let mut take = || -> io::Result<Vec<u8>> {
                // held to the segment, which is held to the file
                r.seek(SeekFrom::Start(body))?;
                limits::read_declared(&mut r, end.saturating_sub(body), segment_end.saturating_sub(body))
            };
            #[cfg(target_os = "macos")]
            if id == ATTACHMENTS {
                attachment_places.push((at, end));
            }
            match id {
                INFO => info = Some(take()?),
                TRACKS => tracks = Some(take()?),
                CHAPTERS => chapters = Some(take()?),
                TAGS => tags = Some(take()?),
                CUES => cues = Some(take()?),
                ATTACHMENTS => attached = read_attachments(&mut r, body, end)?,
                _ => {}
            }
            at = end;
        }
        let info = info.ok_or_else(|| bad("no info"))?;
        let tracks = tracks.ok_or_else(|| bad("no tracks"))?;
        let scale = children(&info).iter().find(|(i, _)| *i == SCALE).map_or(1_000_000, |(_, b)| uint(b)).max(1);
        let title = children(&info)
            .iter()
            .find(|(i, _)| *i == TITLE)
            .map(|(_, b)| String::from_utf8_lossy(b).trim_end_matches('\0').to_string())
            .filter(|t| !t.trim().is_empty());
        let stated = children(&info).iter().find(|(i, _)| *i == DURATION).map_or(0.0, |(_, b)| float(b));

        let (mut video_track, mut video_codec, mut audio_codec, mut width, mut height, mut encrypted) =
            (None, None, None, 0, 0, false);
        for (id, entry) in children(&tracks) {
            if id != 0xAE {
                continue;
            }
            let kids = children(entry);
            let get = |want: u32| kids.iter().find(|(i, _)| *i == want).map(|(_, b)| *b);
            let codec = get(0x86).map(|b| String::from_utf8_lossy(b).trim_end_matches('\0').to_string());
            if get(0x6D80)
                .is_some_and(|e| children(e).iter().any(|(_, enc)| children(enc).iter().any(|(k, _)| *k == 0x5035)))
            {
                encrypted = true;
            }
            match get(0x83).map(uint) {
                Some(1) if video_track.is_none() => {
                    video_track = get(0xD7).map(uint);
                    video_codec = codec;
                    if let Some(v) = get(0xE0) {
                        for (k, b) in children(v) {
                            if k == 0xB0 {
                                width = uint(b) as u32
                            }
                            if k == 0xBA {
                                height = uint(b) as u32
                            }
                        }
                    }
                }
                Some(2) if audio_codec.is_none() => audio_codec = codec,
                _ => {}
            }
        }
        // the cue list names the keyframes; without one the blocks have to be walked
        let mut keys: Vec<u64> = Vec::new();
        if let (Some(c), Some(v)) = (&cues, video_track) {
            for (id, point) in children(c) {
                if id != 0xBB {
                    continue;
                }
                let kids = children(point);
                let time = kids.iter().find(|(i, _)| *i == 0xB3).map(|(_, b)| uint(b));
                let on_video =
                    kids.iter().any(|(i, p)| *i == 0xB7 && children(p).iter().any(|(k, b)| *k == 0xF7 && uint(b) == v));
                if let (Some(t), true) = (time, on_video) {
                    keys.push(t);
                }
            }
        }
        let mut last_seen = 0u64;
        if keys.len() < 2 {
            keys.clear();
            for c in &clusters {
                let time = c.time;
                blocks_in(&mut r, c.payload, c.end, |b| {
                    let t = (time as i64 + b.head.relative as i64).max(0) as u64;
                    last_seen = last_seen.max(t);
                    if b.key && Some(b.head.track) == video_track {
                        keys.push(t);
                    }
                    true
                })?;
            }
        } else if stated <= 0.0 {
            if let Some(c) = clusters.last() {
                let time = c.time;
                blocks_in(&mut r, c.payload, c.end, |b| {
                    last_seen = last_seen.max((time as i64 + b.head.relative as i64).max(0) as u64);
                    true
                })?;
            }
        }
        keys.sort_unstable();
        keys.dedup();
        let duration = if stated > 0.0 { stated.round() as u64 } else { last_seen };
        Ok(Matroska {
            doc,
            webm: kind == Container::Webm,
            info,
            tracks,
            chapters,
            tags,
            attached,
            clusters,
            keys,
            scale,
            duration,
            video_track,
            video_codec,
            audio_codec,
            width,
            height,
            title,
            encrypted,
            #[cfg(target_os = "macos")]
            attachment_places,
        })
    }

    /// The Mac: where the file's Attachments elements are (start, end). A cover or font carried the standard way, before
    /// the clusters, is what Video and most programs write; with such an element ahead of the clusters, WKWebView's
    /// WebM player never answers (no picture, no error, whatever the size of the attachment, and in an MKV too), while
    /// the same file with that element turned into a Void plays at once.
    #[cfg(target_os = "macos")]
    pub fn attachment_places(&self) -> &[(u64, u64)] {
        &self.attachment_places
    }

    fn seconds(&self, ticks: u64) -> f64 {
        ticks as f64 * self.scale as f64 / 1e9
    }

    pub fn facts(&self) -> Facts {
        Facts {
            container: if self.webm { "webm" } else { "mkv" },
            video: self.video_codec.clone(),
            audio: self.audio_codec.clone(),
            width: self.width,
            height: self.height,
            duration: self.seconds(self.duration),
            keys: self.keys.iter().map(|k| self.seconds(*k)).collect(),
            title: self.title.clone(),
            has_cover: self.attached.iter().any(|a| a.is_cover),
            can_trim: !self.encrypted && self.video_track.is_some() && !self.clusters.is_empty(),
            can_tag: !self.encrypted && !self.clusters.is_empty(),
            system: false,
        }
    }

    pub fn cover(&self, path: &Path) -> Option<Vec<u8>> {
        let a = self.attached.iter().find(|a| a.is_cover)?;
        let mut f = File::open(path).ok()?;
        f.seek(SeekFrom::Start(a.data.0)).ok()?;
        let len = f.metadata().ok()?.len();
        limits::read_declared(&mut f, a.data.1, len.saturating_sub(a.data.0)).ok()
    }

    pub fn plan(&self, path: &Path, edit: &Edit) -> Result<Plan, String> {
        let text = |e: io::Error| e.to_string();
        let facts = self.facts();
        let from = edit.from_key.filter(|k| *k > 0 && *k < self.keys.len());
        let to = edit.to_key.filter(|k| *k < self.keys.len());
        if let (Some(a), Some(b)) = (from, to) {
            if b <= a {
                return Err("the end of a cut has to come after its start".into());
            }
        }
        let cutting = from.is_some() || to.is_some();
        if (cutting && !facts.can_trim) || !facts.can_tag {
            return Err("this file's structure doesn't allow it".into());
        }
        let a = from.map_or(0, |k| self.keys[k]);
        let b = to.map_or(u64::MAX, |k| self.keys[k]);
        let margin = (1_000_000_000 / self.scale).max(1);
        let mut r = BufReader::with_capacity(64 * 1024, File::open(path).map_err(text)?);

        // which clusters stay, and how much of each
        let mut out: Vec<OutCluster> = Vec::new();
        let (mut saw_start, mut saw_end) = (from.is_none(), false);
        for (i, c) in self.clusters.iter().enumerate() {
            if to.is_some() && c.time > b.saturating_add(margin) {
                break;
            }
            let next = self.clusters.get(i + 1).map_or(u64::MAX, |n| n.time);
            if from.is_some() && next.saturating_add(margin) < a {
                continue;
            }
            let edge = (from.is_some() && c.time <= a.saturating_add(margin))
                || (to.is_some() && next.saturating_add(margin) >= b);
            let new_time = c.time.saturating_sub(a);
            if !edge {
                out.push(OutCluster {
                    time: new_time,
                    source: c.time,
                    parts: vec![Part::Raw(c.payload, c.end - c.payload)],
                });
                continue;
            }
            let mut parts = Vec::new();
            let shift = a.saturating_sub(c.time) as i64;
            let mut found = Vec::new();
            blocks_in(&mut r, c.payload, c.end, |block| {
                found.push(block);
                true
            })
            .map_err(text)?;
            let time_of = |f: &Found| c.time as i64 + f.head.relative as i64;
            let is_video = |f: &Found| Some(f.head.track) == self.video_track;
            // with open groups of pictures, frames shown before the end keyframe are stored after it, and need it to be drawn
            let end_at = found.iter().position(|f| to.is_some() && is_video(f) && f.key && time_of(f) as u64 == b);
            let last_needed = end_at.and_then(|e| {
                (e + 1..found.len()).rev().find(|i| is_video(&found[*i]) && time_of(&found[*i]) < b as i64)
            });
            for (i, block) in found.iter().enumerate() {
                let shows = time_of(block);
                if is_video(block) && block.key && shows as u64 == a {
                    saw_start = true;
                }
                if is_video(block) && !saw_start {
                    continue;
                }
                let keep = if is_video(block) {
                    match (end_at, last_needed) {
                        (Some(_), Some(l)) if i <= l => shows >= a as i64,
                        (Some(e), _) if i >= e => false,
                        _ => !saw_end && shows >= a as i64 && shows < b.min(i64::MAX as u64) as i64,
                    }
                } else {
                    shows >= a as i64 && shows < b.min(i64::MAX as u64) as i64
                };
                if !keep {
                    continue;
                }
                match i16::try_from(block.head.relative as i64 - shift) {
                    Ok(v) if shift != 0 => parts.push(Part::Moved {
                        start: block.start,
                        end: block.end,
                        relative_at: block.head.relative_at,
                        relative: v,
                    }),
                    Ok(_) => parts.push(Part::Raw(block.start, block.end - block.start)),
                    Err(_) => return Err("a block is too far from its cluster".into()),
                }
            }
            saw_end = saw_end || end_at.is_some();
            if !parts.is_empty() {
                out.push(OutCluster { time: new_time, source: c.time, parts });
            }
        }
        if !saw_start {
            return Err("the cut's keyframe wasn't where the file said".into());
        }
        if out.is_empty() {
            return Err("nothing left after the cut".into());
        }

        let kept_end = to.map_or(self.duration, |k| self.keys[k]);
        let length = kept_end.saturating_sub(a);
        let head = self.head_elements(&mut r, edit, a, kept_end, length, cutting)?;
        let key_times: Vec<u64> = self.keys.iter().copied().filter(|k| *k >= a && *k < b).collect();

        // laid out in this order; every length field is the wide spelling, so sizes can be added up first
        let cluster_len = |c: &OutCluster| -> u64 {
            let body: u64 = c.parts.iter().map(|p| p.len()).sum();
            4 + 8 + 9 + 1 + body
        };
        let seek_ids = head.iter().map(|(id, _)| *id).chain(std::iter::once(CUES)).collect::<Vec<_>>();
        let seek_len =
            element(SEEK_HEAD, &seek_table(&seek_ids.iter().map(|i| (*i, 0)).collect::<Vec<_>>())).len() as u64;
        let mut pos = seek_len;
        let mut places = Vec::new();
        for (id, bytes) in &head {
            places.push((*id, pos));
            pos += bytes.len() as u64;
        }
        let mut cluster_places = Vec::with_capacity(out.len());
        for c in &out {
            cluster_places.push(pos);
            pos += cluster_len(c);
        }
        let cues = cue_list(&key_times, a, self.video_track.unwrap_or(1), &out, &cluster_places);
        places.push((CUES, pos));
        let total = pos + cues.len() as u64;

        let mut plan = Plan::default();
        let mut start = element(EBML, &self.doc);
        start.extend_from_slice(&id_bytes(SEGMENT));
        start.extend_from_slice(&wide_size(total));
        start.extend_from_slice(&element(SEEK_HEAD, &seek_table(&places)));
        plan.bytes(start);
        for (_, bytes) in head {
            plan.bytes(bytes);
        }
        for c in &out {
            let body: u64 = c.parts.iter().map(|p| p.len()).sum();
            let mut h = id_bytes(CLUSTER);
            h.extend_from_slice(&wide_size(9 + 1 + body));
            // the timestamp: one id byte, one length byte, eight bytes of value (9 + 1 above)
            h.push(TIMESTAMP as u8);
            h.push(0x88);
            h.extend_from_slice(&c.time.to_be_bytes());
            plan.bytes(h);
            for p in &c.parts {
                match p {
                    Part::Raw(at, len) => plan.file(*at, *len),
                    Part::Moved { start, end, relative_at, relative } => {
                        plan.file(*start, relative_at - start);
                        plan.bytes(relative.to_be_bytes().to_vec());
                        plan.file(relative_at + 2, end - relative_at - 2);
                    }
                }
            }
        }
        plan.bytes(cues);
        Ok(plan)
    }

    fn head_elements(
        &self,
        source: &mut (impl Read + Seek),
        edit: &Edit,
        a: u64,
        end: u64,
        length: u64,
        cutting: bool,
    ) -> Result<Vec<(u32, Vec<u8>)>, String> {
        let mut out = Vec::new();
        // info: the length changes, and perhaps the title; a checksum over the old contents would now be wrong
        let mut info = Vec::new();
        for (id, b) in children(&self.info) {
            if id == CRC || id == VOID || id == DURATION || (id == TITLE && edit.title.is_some()) {
                continue;
            }
            info.extend_from_slice(&element(id, b));
        }
        if let Some(t) = edit.title.as_ref().filter(|t| !t.trim().is_empty()) {
            info.extend_from_slice(&element(TITLE, t.as_bytes()));
        }
        info.extend_from_slice(&element(DURATION, &(length as f64).to_be_bytes()));
        out.push((INFO, element(INFO, &info)));
        out.push((TRACKS, element(TRACKS, &self.tracks)));
        if let Some(c) = &self.chapters {
            let body = if cutting { cut_chapters(c, a * self.scale, end * self.scale) } else { c.clone() };
            if !body.is_empty() {
                out.push((CHAPTERS, element(CHAPTERS, &body)));
            }
        }
        // fonts and pictures: small next to a film, so they're carried in memory until the save
        let kept: Vec<&Attached> = self.attached.iter().filter(|a| edit.cover.is_none() || !a.is_cover).collect();
        if !kept.is_empty() || edit.cover.is_some() {
            let mut body = Vec::new();
            if let Some(Cover { bytes, png }) = &edit.cover {
                let mut file = element(FILE_NAME, if *png { b"cover.png" } else { b"cover.jpg" });
                file.extend_from_slice(&element(FILE_MIME, if *png { b"image/png" } else { b"image/jpeg" }));
                file.extend_from_slice(&uint_element(FILE_UID, cover_uid(bytes)));
                file.extend_from_slice(&element(FILE_DATA, bytes));
                body.extend_from_slice(&element(ATTACHED_FILE, &file));
            }
            for a in kept {
                source.seek(SeekFrom::Start(a.data.0)).map_err(|e| e.to_string())?;
                // held to the file when it was opened (read_attachments)
                let mut data = limits::zeroed(a.data.1).map_err(|e| e.to_string())?;
                source.read_exact(&mut data).map_err(|e| e.to_string())?;
                let mut file = a.head.clone();
                file.extend_from_slice(&element(FILE_DATA, &data));
                body.extend_from_slice(&element(ATTACHED_FILE, &file));
            }
            out.push((ATTACHMENTS, element(ATTACHMENTS, &body)));
        }
        if let Some(t) = &self.tags {
            let body = if cutting { cut_tags(t) } else { t.clone() };
            if !body.is_empty() {
                out.push((TAGS, element(TAGS, &body)));
            }
        }
        Ok(out)
    }
}

fn cover_uid(bytes: &[u8]) -> u64 {
    // a stable number for the same picture; only has to differ from the file's other attachments
    bytes.iter().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ *b as u64).wrapping_mul(0x100_0000_01B3)) | 1
}

enum Part {
    Raw(u64, u64),
    Moved { start: u64, end: u64, relative_at: u64, relative: i16 },
}

impl Part {
    fn len(&self) -> u64 {
        match self {
            Part::Raw(_, l) => *l,
            Part::Moved { start, end, .. } => end - start,
        }
    }
}

struct OutCluster {
    time: u64,
    source: u64,
    parts: Vec<Part>,
}

fn seek_table(places: &[(u32, u64)]) -> Vec<u8> {
    let mut out = Vec::new();
    for (id, at) in places {
        let mut one = element(0x53AB, &id_bytes(*id));
        one.extend_from_slice(&uint_element(0x53AC, *at));
        out.extend_from_slice(&element(0x4DBB, &one));
    }
    out
}

fn cue_list(keys: &[u64], a: u64, track: u64, clusters: &[OutCluster], places: &[u64]) -> Vec<u8> {
    let mut body = Vec::new();
    for k in keys {
        let i = clusters.partition_point(|c| c.source <= *k).saturating_sub(1);
        let mut pos = uint_element(0xF7, track);
        pos.extend_from_slice(&uint_element(0xF1, places[i]));
        let mut point = uint_element(0xB3, k - a);
        point.extend_from_slice(&element(0xB7, &pos));
        body.extend_from_slice(&element(0xBB, &point));
    }
    element(CUES, &body)
}

fn read_cluster(r: &mut (impl Read + Seek), body: u64, size: Option<u64>, limit: u64) -> io::Result<Option<ClusterAt>> {
    let (mut at, mut time, mut payload) = (body, None, None);
    let known_end = size.map(|s| (body + s).min(limit));
    let stop = known_end.unwrap_or(limit);
    // read as far as the first block; with a known length the rest can be skipped
    while at < stop {
        r.seek(SeekFrom::Start(at))?;
        let Ok((id, a)) = read_id(r) else { break };
        if known_end.is_none() && LEVEL_ONE.contains(&id) {
            break;
        }
        let Ok((kid, b)) = read_size(r) else { break };
        let inner = at + a + b;
        let next = inner + kid.unwrap_or(0);
        match id {
            TIMESTAMP => {
                let mut v = vec![0u8; kid.unwrap_or(0).min(8) as usize];
                r.read_exact(&mut v)?;
                time = Some(uint(&v));
            }
            CRC | POSITION | PREV_SIZE => {}
            _ => {
                if payload.is_none() {
                    payload = Some(at);
                }
                if known_end.is_some() && time.is_some() {
                    break;
                }
            }
        }
        at = next;
    }
    let Some(time) = time else { return Ok(None) };
    let end = known_end.unwrap_or(at.min(limit));
    Ok(Some(ClusterAt { payload: payload.unwrap_or(end), end, time }))
}

fn read_attachments(r: &mut (impl Read + Seek), body: u64, end: u64) -> io::Result<Vec<Attached>> {
    let mut out = Vec::new();
    let mut at = body;
    while at < end {
        r.seek(SeekFrom::Start(at))?;
        let (id, a) = read_id(r)?;
        let (size, b) = read_size(r)?;
        let inner = at + a + b;
        let next = inner.saturating_add(size.unwrap_or(0)).min(end);
        if id == ATTACHED_FILE {
            let (mut p, mut head, mut name, mut mime, mut data) =
                (inner, Vec::new(), String::new(), String::new(), None);
            while p < next {
                r.seek(SeekFrom::Start(p))?;
                let (kid, x) = read_id(r)?;
                let (kid_size, y) = read_size(r)?;
                let (kid_at, n) = (p + x + y, kid_size.unwrap_or(0));
                // a part of the attached file can't reach past the file around it
                if kid_at.saturating_add(n) > next {
                    data = None;
                    break;
                }
                if kid == FILE_DATA {
                    data = Some((kid_at, n));
                } else {
                    let v = limits::read_declared(r, n, next - kid_at)?;
                    if kid == FILE_NAME {
                        name = String::from_utf8_lossy(&v).into_owned()
                    }
                    if kid == FILE_MIME {
                        mime = String::from_utf8_lossy(&v).into_owned()
                    }
                    head.extend_from_slice(&element(kid, &v));
                }
                p = kid_at + n;
            }
            if let Some(data) = data {
                let is_cover = mime.starts_with("image/") && name.to_ascii_lowercase().contains("cover");
                out.push(Attached { head, data, is_cover });
            }
        }
        at = next;
    }
    Ok(out)
}

/// Chapters move with the cut; those wholly outside it go. Times are in nanoseconds.
fn cut_chapters(body: &[u8], a: u64, b: u64) -> Vec<u8> {
    fn atom(kids: &[u8], a: u64, b: u64) -> Option<Vec<u8>> {
        let list = children(kids);
        let start = list.iter().find(|(i, _)| *i == 0x91).map_or(0, |(_, v)| uint(v));
        if start >= b {
            return None;
        }
        let end = list.iter().find(|(i, _)| *i == 0x92).map(|(_, v)| uint(v));
        if end.is_some_and(|e| e <= a) {
            return None;
        }
        let mut out = uint_element(0x91, start.saturating_sub(a));
        if let Some(e) = end {
            out.extend_from_slice(&uint_element(0x92, e.min(b) - a));
        }
        for (id, v) in list {
            match id {
                0x91 | 0x92 => {}
                0xB6 => {
                    if let Some(x) = atom(v, a, b) {
                        out.extend_from_slice(&element(0xB6, &x));
                    }
                }
                _ => out.extend_from_slice(&element(id, v)),
            }
        }
        Some(out)
    }
    let mut out = Vec::new();
    for (id, edition) in children(body) {
        if id != 0x45B9 {
            continue;
        }
        let (mut kept, mut any) = (Vec::new(), false);
        for (k, v) in children(edition) {
            if k == 0xB6 {
                if let Some(x) = atom(v, a, b) {
                    kept.extend_from_slice(&element(0xB6, &x));
                    any = true;
                }
            } else {
                kept.extend_from_slice(&element(k, v));
            }
        }
        if any {
            out.extend_from_slice(&element(0x45B9, &kept));
        }
    }
    out
}

/// Counts of bytes, frames and length written by the muxer describe the old file, so they go after a cut.
fn cut_tags(body: &[u8]) -> Vec<u8> {
    let stale = |name: &[u8]| {
        let n = String::from_utf8_lossy(name).to_ascii_uppercase();
        let n = n.trim_end_matches('\0');
        n.starts_with("_STATISTICS_") || matches!(n, "DURATION" | "NUMBER_OF_FRAMES" | "NUMBER_OF_BYTES" | "BPS")
    };
    let mut out = Vec::new();
    for (id, tag) in children(body) {
        if id != 0x7373 {
            out.extend_from_slice(&element(id, tag));
            continue;
        }
        let (mut kept, mut any) = (Vec::new(), false);
        for (k, v) in children(tag) {
            if k == 0x67C8 {
                let name = children(v).iter().find(|(i, _)| *i == 0x45A3).map(|(_, b)| b.to_vec()).unwrap_or_default();
                if stale(&name) {
                    continue;
                }
                any = true;
            }
            kept.extend_from_slice(&element(k, v));
        }
        if any {
            out.extend_from_slice(&element(0x7373, &kept));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// An element whose size field says `claimed`, whatever really follows.
    fn lying(id: u32, claimed: u64, body: &[u8]) -> Vec<u8> {
        let mut out = id_bytes(id);
        out.extend_from_slice(&wide_size(claimed));
        out.extend_from_slice(body);
        out
    }

    #[test]
    fn a_header_claiming_terabytes_is_refused_without_asking_for_them() {
        let dir = std::env::temp_dir().join(format!("dotany-video-mkv-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("lying-header.mkv");
        std::fs::write(&path, lying(EBML, 1 << 48, &[0x42, 0x86, 0x81, 0x01])).unwrap();
        let e = Matroska::open(&path, Container::Matroska).err().expect("refused");
        assert_eq!(e.kind(), io::ErrorKind::InvalidData);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn attachment_parts_longer_than_the_file_are_dropped() {
        // an attached file whose name says it is 2^40 bytes long
        let file = [element(FILE_MIME, b"image/jpeg"), lying(FILE_NAME, 1 << 40, b"cover.jpg")].concat();
        let bytes = element(ATTACHED_FILE, &file);
        let end = bytes.len() as u64;
        let found = read_attachments(&mut Cursor::new(&bytes), 0, end).unwrap();
        assert!(found.is_empty());
        // and one whose data says it is 2^40 bytes: no cover that points past the end
        let file = [
            element(FILE_NAME, b"cover.jpg"),
            element(FILE_MIME, b"image/jpeg"),
            lying(FILE_DATA, 1 << 40, &[0xFF, 0xD8]),
        ]
        .concat();
        let bytes = element(ATTACHED_FILE, &file);
        let found = read_attachments(&mut Cursor::new(&bytes), 0, bytes.len() as u64).unwrap();
        assert!(found.is_empty());
        // a sound one still reads
        let file =
            [element(FILE_NAME, b"cover.jpg"), element(FILE_MIME, b"image/jpeg"), element(FILE_DATA, &[0xFF, 0xD8])]
                .concat();
        let bytes = element(ATTACHED_FILE, &file);
        let found = read_attachments(&mut Cursor::new(&bytes), 0, bytes.len() as u64).unwrap();
        assert_eq!(found.len(), 1);
        assert!(found[0].is_cover);
        assert_eq!(found[0].data.1, 2);
    }
}
