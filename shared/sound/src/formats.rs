//! What a file is, by its bytes. The name can lie: Opus and WMA stay shut whatever the file is called.
//! No code for a refused format is compiled in, so refusing is the only thing that can happen to one;
//! looking first means the person hears why, rather than getting a decoder's failure.
//! M4A is the one format read by Windows rather than by us, and it's only ever read. So are the tracker modules
//! (MOD, XM, S3M, IT: `module`) and the old AU and VOC sounds (`oldsound`).

use serde::{Deserialize, Serialize};
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Mp3,
    Flac,
    Wav,
    Aiff,
    Ogg,
    /// Read only, decoded by Windows. Nothing here writes AAC or M4A.
    M4a,
    /// Tracker modules, played by libopenmpt. Read only.
    Mod,
    Xm,
    S3m,
    It,
    /// Sun/NeXT and Creative Voice: our own readers. Read only.
    Au,
    Voc,
}

impl Kind {
    /// ALAC in an M4A isn't, which is why its bits are kept: see `lossy_source`.
    pub fn lossy(self) -> bool {
        matches!(self, Kind::Mp3 | Kind::Ogg | Kind::M4a)
    }
    /// Only what Audio can also write: everything else opens, and is saved as a new file in one of these.
    pub fn writable(self) -> bool {
        matches!(self, Kind::Mp3 | Kind::Flac | Kind::Wav | Kind::Aiff | Kind::Ogg)
    }
    pub fn module(self) -> bool {
        matches!(self, Kind::Mod | Kind::Xm | Kind::S3m | Kind::It)
    }
    pub fn ext(self) -> &'static str {
        match self {
            Kind::Mp3 => "mp3",
            Kind::Flac => "flac",
            Kind::Wav => "wav",
            Kind::Aiff => "aiff",
            Kind::Ogg => "ogg",
            Kind::M4a => "m4a",
            Kind::Mod => "mod",
            Kind::Xm => "xm",
            Kind::S3m => "s3m",
            Kind::It => "it",
            Kind::Au => "au",
            Kind::Voc => "voc",
        }
    }
}

/// Whether a source has already lost something: an M4A holding ALAC hasn't, and only ALAC's decoder reports its bits.
pub fn lossy_source(kind: Kind, bits: Option<u32>) -> bool {
    kind.lossy() && !(kind == Kind::M4a && bits.is_some())
}

#[derive(PartialEq, Debug, Clone, Copy)]
pub enum Sniff {
    Is(Kind),
    Refused,
    Damaged,
}

pub const NAMES: [&str; 15] =
    ["mp3", "flac", "wav", "aiff", "aif", "ogg", "oga", "m4a", "mod", "xm", "s3m", "it", "au", "snd", "voc"];

pub fn allowed_name(name: &str) -> bool {
    let Some((_, ext)) = name.rsplit_once('.') else { return false };
    NAMES.iter().any(|n| n.eq_ignore_ascii_case(ext))
}

fn u32_le(b: &[u8], at: usize) -> Option<u32> {
    b.get(at..at + 4).map(|x| u32::from_le_bytes(x.try_into().unwrap()))
}
fn u32_be(b: &[u8], at: usize) -> Option<u32> {
    b.get(at..at + 4).map(|x| u32::from_be_bytes(x.try_into().unwrap()))
}
fn u16_le(b: &[u8], at: usize) -> Option<u16> {
    b.get(at..at + 2).map(|x| u16::from_le_bytes(x.try_into().unwrap()))
}

// a WAV can carry other codecs inside; only plain samples are ours to open
fn wav(b: &[u8]) -> Sniff {
    if b.get(8..12) != Some(b"WAVE") {
        return Sniff::Refused;
    }
    let mut p = 12;
    while let Some(len) = u32_le(b, p + 4) {
        if b.get(p..p + 4) == Some(b"fmt ") {
            let Some(mut tag) = u16_le(b, p + 8) else { return Sniff::Damaged };
            if tag == 0xFFFE {
                // the sub-format's first two bytes say which kind of samples
                let Some(sub) = u16_le(b, p + 32) else { return Sniff::Damaged };
                tag = sub;
            }
            let bits = u16_le(b, p + 22).unwrap_or(0);
            return match (tag, bits) {
                // G.711 (A-law, μ-law) is read by our own reader (decode.rs, oldsound::wav)
                (1, 8 | 16 | 24 | 32) | (3, 32 | 64) | (6 | 7, 8) => Sniff::Is(Kind::Wav),
                _ => Sniff::Refused,
            };
        }
        p += 8 + len as usize + (len as usize & 1);
    }
    Sniff::Damaged
}

// AIFC is usually compressed; a few of its names only say which order plain samples are in
fn aiff(b: &[u8]) -> Sniff {
    let form = b.get(8..12);
    if form == Some(b"AIFF") {
        return Sniff::Is(Kind::Aiff);
    }
    if form != Some(b"AIFC") {
        return Sniff::Refused;
    }
    let mut p = 12;
    while let Some(len) = u32_be(b, p + 4) {
        if b.get(p..p + 4) == Some(b"COMM") {
            return match b.get(p + 26..p + 30) {
                Some(b"NONE") | Some(b"twos") | Some(b"sowt") => Sniff::Is(Kind::Aiff),
                Some(_) => Sniff::Refused,
                None => Sniff::Damaged,
            };
        }
        p += 8 + len as usize + (len as usize & 1);
    }
    Sniff::Damaged
}

// an Ogg file says what it holds at the start of its first packet
fn ogg(b: &[u8]) -> Sniff {
    let Some(&segments) = b.get(26) else { return Sniff::Damaged };
    let start = 27 + segments as usize;
    let Some(first) = b.get(start..start + 8) else { return Sniff::Damaged };
    if first.starts_with(b"\x01vorbis") {
        return Sniff::Is(Kind::Ogg);
    }
    // anything else that names itself is another codec in the same box; nameless bytes are just damage
    let known: [&[u8]; 8] =
        [b"OpusHead", b"\x7fFLAC", b"Speex   ", b"\x80theora", b"fishead\0", b"PCM     ", b"\x80kate", b"BBCD"];
    if known.iter().any(|k| first.starts_with(k)) { Sniff::Refused } else { Sniff::Damaged }
}

const MPEG_RATES: [[u32; 3]; 4] = [[11025, 12000, 8000], [0, 0, 0], [22050, 24000, 16000], [44100, 48000, 32000]];
const V1_L3: [u32; 15] = [0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320];
const V2_L3: [u32; 15] = [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160];

// (is layer III, length of the frame)
fn mpeg_frame(h: &[u8]) -> Option<(bool, usize)> {
    if h.len() < 4 || h[0] != 0xFF || h[1] & 0xE0 != 0xE0 {
        return None;
    }
    let version = (h[1] >> 3) & 3;
    let layer = (h[1] >> 1) & 3;
    if version == 1 || layer == 0 {
        return None;
    }
    let (bitrate, rate) = ((h[2] >> 4) as usize, ((h[2] >> 2) & 3) as usize);
    if bitrate == 15 || rate == 3 {
        return None;
    }
    let hz = MPEG_RATES[version as usize][rate];
    if layer != 1 {
        // layer I or II: real audio, but not a format this tool opens
        return if bitrate == 0 { None } else { Some((false, 0)) };
    }
    if bitrate == 0 {
        return None;
    }
    let pad = ((h[2] >> 1) & 1) as usize;
    let len = if version == 3 { 144 * V1_L3[bitrate] * 1000 / hz } else { 72 * V2_L3[bitrate] * 1000 / hz };
    Some((true, len as usize + pad))
}

fn mpeg(b: &[u8]) -> Sniff {
    // AAC's own stream has the same first bits as MP3, with the layer set to zero
    if b.len() > 2 && b[0] == 0xFF && b[1] & 0xF6 == 0xF0 {
        return Sniff::Refused;
    }
    let scan = b.len().saturating_sub(4);
    let mut other = 0;
    for p in 0..scan {
        let Some((layer3, len)) = mpeg_frame(&b[p..]) else { continue };
        if !layer3 {
            other += 1;
            continue;
        }
        // one lucky pair of bytes isn't audio: the next frame has to start where this one says it ends
        match b.get(p + len..) {
            Some(rest) if rest.len() >= 4 => {
                if let Some((true, _)) = mpeg_frame(rest) {
                    return Sniff::Is(Kind::Mp3);
                }
            }
            // a file that's only a frame or two long
            _ => return Sniff::Is(Kind::Mp3),
        }
    }
    if other > 8 && b.first() == Some(&0xFF) {
        return Sniff::Refused;
    }
    Sniff::Damaged
}

// the four-letter mark at byte 1080 of a MOD: ProTracker's and its relatives', and the trackers' that wrote more channels
fn mod_mark(m: &[u8]) -> bool {
    let d = |c: u8| c.is_ascii_digit();
    matches!(m, b"M.K." | b"M!K!" | b"M&K!" | b"N.T." | b"FLT4" | b"FLT8" | b"CD81" | b"OKTA" | b"OCTA")
        || (d(m[0]) && &m[1..] == b"CHN")
        || (d(m[0]) && d(m[1]) && (&m[2..] == b"CH" || &m[2..] == b"CN"))
        || (&m[..3] == b"TDZ" && d(m[3]))
}

fn ext_is(name: &str, ext: &str) -> bool {
    name.rsplit_once('.').is_some_and(|(_, e)| e.eq_ignore_ascii_case(ext))
}

// the tracker modules and the old sounds say what they are in their first bytes (MOD at byte 1080)
fn old_kind(b: &[u8], name: &str) -> Option<Sniff> {
    use crate::oldsound::{self, Bad};
    let from = |r: Result<Kind, Bad>| match r {
        Ok(k) => Sniff::Is(k),
        Err(Bad::Refused) => Sniff::Refused,
        Err(Bad::Damaged) => Sniff::Damaged,
    };
    if oldsound::is_au(b) {
        return Some(from(oldsound::au(b, b.len()).map(|_| Kind::Au)));
    }
    if oldsound::is_voc(b) {
        return Some(from(oldsound::voc(b).map(|_| Kind::Voc)));
    }
    if b.starts_with(b"IMPM") {
        return Some(Sniff::Is(Kind::It));
    }
    if b.len() >= 17 && b[..17].eq_ignore_ascii_case(b"Extended Module: ") {
        return Some(Sniff::Is(Kind::Xm));
    }
    if b.get(44..48) == Some(b"SCRM") {
        return Some(Sniff::Is(Kind::S3m));
    }
    if b.get(1080..1084).is_some_and(mod_mark) {
        return Some(Sniff::Is(Kind::Mod));
    }
    // the earliest MODs have no mark at all; named so, libopenmpt decides whether it is one
    if ext_is(name, "mod") && b.len() >= 600 {
        return Some(Sniff::Is(Kind::Mod));
    }
    None
}

/// Judges the start of a file. With an ID3 tag in front, `b` is what follows the tag.
pub fn judge(b: &[u8], name: &str) -> Sniff {
    let verdict = if b.len() < 4 {
        Sniff::Damaged
    } else {
        match &b[..4] {
            b"RIFF" => wav(b),
            b"FORM" => aiff(b),
            b"OggS" => ogg(b),
            b"fLaC" => Sniff::Is(Kind::Flac),
            // WMA, Matroska, AC-3, DTS, FLAC's rivals: their first bytes are enough. M4A and MP4 need the file: see `mp4_audio`
            b"RF64" | b"RIFX" | b"caff" | b"MThd" | b"wvpk" | b"MAC " | b"#!AM" | b"MPCK" | b"TTA1" => Sniff::Refused,
            [0x30, 0x26, 0xB2, 0x75]
            | [0x1A, 0x45, 0xDF, 0xA3]
            | [0x7F, 0xFE, 0x80, 0x01]
            | [0xFE, 0x7F, 0x01, 0x80] => Sniff::Refused,
            [0x0B, 0x77, ..] => Sniff::Refused,
            _ if b.get(4..8) == Some(b"ftyp") => Sniff::Refused,
            _ => old_kind(b, name).unwrap_or_else(|| mpeg(b)),
        }
    };
    // bytes that mean nothing in a file not even named like audio isn't damaged audio; it's some other kind of file
    if verdict == Sniff::Damaged && !allowed_name(name) {
        return Sniff::Refused;
    }
    verdict
}

fn id3_size(h: &[u8]) -> Option<u64> {
    if h.len() < 10 || &h[..3] != b"ID3" {
        return None;
    }
    let s = (h[6] as u64) << 21 | (h[7] as u64) << 14 | (h[8] as u64) << 7 | h[9] as u64;
    Some(10 + s + if h[5] & 0x10 != 0 { 10 } else { 0 })
}

fn box_at(f: &mut (impl Read + Seek), at: u64, end: u64) -> Option<([u8; 4], u64, u64)> {
    if at + 8 > end {
        return None;
    }
    f.seek(SeekFrom::Start(at)).ok()?;
    let mut h = [0u8; 16];
    f.read_exact(&mut h[..8]).ok()?;
    let kind: [u8; 4] = h[4..8].try_into().unwrap();
    let (size, head) = match u32_be(&h, 0)? {
        0 => (end - at, 8),
        1 => {
            f.read_exact(&mut h[8..16]).ok()?;
            (u64::from_be_bytes(h[8..16].try_into().unwrap()), 16)
        }
        n => (n as u64, 8),
    };
    (size >= head && at + size <= end).then_some((kind, at + head, at + size))
}

fn children(b: &[u8]) -> impl Iterator<Item = (&[u8], &[u8])> {
    let mut p = 0;
    std::iter::from_fn(move || {
        let size = u32_be(b, p)? as usize;
        let kind = b.get(p + 4..p + 8)?;
        let body = b.get(p + 8..p + size.max(8))?;
        p += size.max(8);
        Some((kind, body))
    })
}

fn child<'a>(b: &'a [u8], name: &[u8]) -> Option<&'a [u8]> {
    children(b).find(|(k, _)| *k == name).map(|(_, body)| body)
}

/// What the movie box says its tracks are: (handler, first sample entry's name) for each.
pub fn mp4_tracks(moov: &[u8]) -> Vec<([u8; 4], [u8; 4])> {
    children(moov)
        .filter(|(k, _)| *k == b"trak")
        .filter_map(|(_, trak)| {
            let mdia = child(trak, b"mdia")?;
            let handler: [u8; 4] = child(mdia, b"hdlr")?.get(8..12)?.try_into().ok()?;
            let stsd = child(child(child(mdia, b"minf")?, b"stbl")?, b"stsd")?;
            let entry: [u8; 4] = stsd.get(12..16).and_then(|e| e.try_into().ok()).unwrap_or(*b"    ");
            Some((handler, entry))
        })
        .collect()
}

/// An MP4 file is M4A when its only tracks are sound in AAC or ALAC. A film is Video's; anything locked or in another codec is refused.
pub fn m4a_verdict(tracks: &[([u8; 4], [u8; 4])]) -> Sniff {
    let sound: Vec<_> = tracks.iter().filter(|(h, _)| h == b"soun").map(|(_, e)| e).collect();
    if tracks.iter().any(|(h, _)| h == b"vide") || sound.is_empty() {
        return Sniff::Refused;
    }
    if sound.iter().all(|e| *e == b"mp4a" || *e == b"alac") { Sniff::Is(Kind::M4a) } else { Sniff::Refused }
}

/// The stretch of the sound the file means to be heard, by its edit list: the encoder's priming before it and padding after it aren't the song.
/// Windows' decoder hands both over, so they're dropped here. Times are in `scale` units per second.
#[derive(Debug, PartialEq, Clone, Copy)]
pub struct Heard {
    pub skip: u64,
    pub skip_scale: u32,
    pub length: u64,
    pub length_scale: u32,
}

fn u64_be(b: &[u8], at: usize) -> Option<u64> {
    b.get(at..at + 8).map(|x| u64::from_be_bytes(x.try_into().unwrap()))
}

// a box's time scale: version 1 has 64-bit times before it
fn scale_of(header: &[u8]) -> Option<u32> {
    u32_be(header, if header.first() == Some(&1) { 20 } else { 12 })
}

pub fn m4a_heard(moov: &[u8]) -> Option<Heard> {
    let movie = scale_of(child(moov, b"mvhd")?)?;
    let trak = children(moov)
        .filter(|(k, _)| *k == b"trak")
        .map(|(_, t)| t)
        .find(|t| child(*t, b"mdia").and_then(|m| child(m, b"hdlr")).and_then(|h| h.get(8..12)) == Some(b"soun"))?;
    let media = scale_of(child(child(trak, b"mdia")?, b"mdhd")?)?;
    let elst = child(child(trak, b"edts")?, b"elst")?;
    let wide = elst.first() == Some(&1);
    let mut entries = (0..u32_be(elst, 4)? as usize).map(|i| {
        let at = 8 + i * if wide { 20 } else { 12 };
        if wide {
            Some((u64_be(elst, at)?, u64_be(elst, at + 8)? as i64))
        } else {
            Some((u32_be(elst, at)? as u64, u32_be(elst, at + 4)? as i32 as i64))
        }
    });
    // only the plain case: one stretch of the media, perhaps after an empty one
    let (mut length, mut start) = entries.next()??;
    if start == -1 {
        (length, start) = entries.next()??;
    }
    if entries.next().is_some() || start < 0 || movie == 0 || media == 0 {
        return None;
    }
    Some(Heard { skip: start as u64, skip_scale: media, length, length_scale: movie })
}

fn moov_of(f: &mut std::fs::File) -> Result<Vec<u8>, Sniff> {
    let end = f.metadata().map(|m| m.len()).map_err(|_| Sniff::Damaged)?;
    let mut at = 0;
    while let Some((kind, body, next)) = box_at(f, at, end) {
        if &kind == b"moov" {
            // a movie box this large isn't an audio file's
            if next - body > 64 << 20 {
                return Err(Sniff::Refused);
            }
            let mut moov = vec![0; (next - body) as usize];
            f.seek(SeekFrom::Start(body)).and_then(|_| f.read_exact(&mut moov)).map_err(|_| Sniff::Damaged)?;
            return Ok(moov);
        }
        at = next;
    }
    Err(Sniff::Damaged)
}

// the movie box may be at either end, so this walks the file rather than its start
fn mp4_audio(f: &mut std::fs::File) -> Sniff {
    match moov_of(f) {
        Ok(moov) => m4a_verdict(&mp4_tracks(&moov)),
        Err(s) => s,
    }
}

/// What an M4A means to be heard, if it says.
pub fn heard_of(path: &Path) -> Option<Heard> {
    m4a_heard(&moov_of(&mut std::fs::File::open(path).ok()?).ok()?)
}

pub fn sniff_file(path: &Path) -> std::io::Result<Sniff> {
    let mut f = std::fs::File::open(path)?;
    let mut first = [0u8; 12];
    if f.read_exact(&mut first).is_ok() && &first[4..8] == b"ftyp" {
        return Ok(mp4_audio(&mut f));
    }
    f.seek(SeekFrom::Start(0))?;
    let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let mut head = vec![0; 256 * 1024];
    let mut n = 0;
    while n < head.len() {
        let got = f.read(&mut head[n..])?;
        if got == 0 {
            break;
        }
        n += got;
    }
    head.truncate(n);
    if let Some(skip) = id3_size(&head) {
        f.seek(SeekFrom::Start(skip))?;
        let mut after = Vec::new();
        f.by_ref().take(256 * 1024).read_to_end(&mut after)?;
        return Ok(judge(&after, &name));
    }
    Ok(judge(&head, &name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert!(
            allowed_name("a.MP3")
                && allowed_name("b.aif")
                && allowed_name("c.m4a")
                && !allowed_name("d.m4b")
                && !allowed_name("wav")
        );
    }

    #[test]
    fn refused_by_bytes_whatever_the_name() {
        let mut m4a = vec![0, 0, 0, 32];
        m4a.extend_from_slice(b"ftypM4A ");
        m4a.resize(64, 0);
        assert_eq!(judge(&m4a, "song.mp3"), Sniff::Refused);
        let mut opus = b"OggS".to_vec();
        opus.resize(26, 0);
        opus.push(1);
        opus.push(19);
        opus.extend_from_slice(b"OpusHead");
        opus.resize(80, 0);
        assert_eq!(judge(&opus, "voice.ogg"), Sniff::Refused);
        assert_eq!(judge(&[0xFF, 0xF1, 0x50, 0x80, 0, 0, 0, 0], "a.mp3"), Sniff::Refused);
    }

    fn trak(handler: &[u8; 4], entry: &[u8; 4]) -> Vec<u8> {
        let wrap = |name: &[u8], body: Vec<u8>| -> Vec<u8> {
            [((body.len() + 8) as u32).to_be_bytes().to_vec(), name.to_vec(), body].concat()
        };
        let mut hdlr = vec![0; 8];
        hdlr.extend_from_slice(handler);
        hdlr.resize(24, 0);
        let mut stsd = vec![0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 16];
        stsd.extend_from_slice(entry);
        stsd.resize(28, 0);
        let stbl = wrap(b"stbl", wrap(b"stsd", stsd));
        wrap(b"trak", wrap(b"mdia", [wrap(b"hdlr", hdlr), wrap(b"minf", stbl)].concat()))
    }

    #[test]
    fn m4a_is_sound_only_in_aac_or_alac() {
        let read = |traks: &[Vec<u8>]| m4a_verdict(&mp4_tracks(&traks.concat()));
        assert_eq!(read(&[trak(b"soun", b"mp4a")]), Sniff::Is(Kind::M4a));
        assert_eq!(read(&[trak(b"soun", b"alac")]), Sniff::Is(Kind::M4a));
        // chapters as text beside the sound are fine
        assert_eq!(read(&[trak(b"soun", b"mp4a"), trak(b"text", b"text")]), Sniff::Is(Kind::M4a));
        // a film is Video's, a locked song stays locked, other codecs in the same box aren't ours
        assert_eq!(read(&[trak(b"vide", b"avc1"), trak(b"soun", b"mp4a")]), Sniff::Refused);
        assert_eq!(read(&[trak(b"soun", b"drms")]), Sniff::Refused);
        assert_eq!(read(&[trak(b"soun", b"enca")]), Sniff::Refused);
        assert_eq!(read(&[trak(b"soun", b"Opus")]), Sniff::Refused);
        assert_eq!(read(&[trak(b"soun", b"ac-3")]), Sniff::Refused);
        assert_eq!(read(&[]), Sniff::Refused);
    }

    fn module_bytes(at: usize, mark: &[u8], len: usize) -> Vec<u8> {
        let mut b = vec![0u8; len];
        b[at..at + mark.len()].copy_from_slice(mark);
        b
    }

    #[test]
    fn tracker_modules_by_their_marks() {
        assert_eq!(judge(&module_bytes(1080, b"M.K.", 2000), "song.mod"), Sniff::Is(Kind::Mod));
        assert_eq!(judge(&module_bytes(1080, b"8CHN", 2000), "song.bin"), Sniff::Is(Kind::Mod));
        assert_eq!(judge(&module_bytes(1080, b"16CH", 2000), "x"), Sniff::Is(Kind::Mod));
        assert_eq!(judge(&module_bytes(0, b"Extended Module: tune", 400), "a.xm"), Sniff::Is(Kind::Xm));
        assert_eq!(judge(&module_bytes(44, b"SCRM", 400), "a.s3m"), Sniff::Is(Kind::S3m));
        assert_eq!(judge(&module_bytes(0, b"IMPM", 400), "renamed.mp3"), Sniff::Is(Kind::It));
        // the earliest MODs have no mark; only the name says so, and libopenmpt decides
        assert_eq!(judge(&module_bytes(0, b"title", 2000), "old.mod"), Sniff::Is(Kind::Mod));
        assert_eq!(judge(&module_bytes(0, b"title", 2000), "old.wav"), Sniff::Damaged);
        // MO3, MPTM's cousins and the other tracker formats libopenmpt knows are not ours
        assert_eq!(judge(&module_bytes(0, b"MO3", 2000), "packed.mo3"), Sniff::Refused);
        assert!(Kind::It.module() && !Kind::It.writable() && !Kind::Au.writable() && !Kind::Voc.lossy());
    }

    #[test]
    fn old_sounds() {
        let mut au = b".snd".to_vec();
        for v in [24u32, 4, 1, 8000, 1] {
            au.extend_from_slice(&v.to_be_bytes());
        }
        au.extend_from_slice(&[0xFF; 4]);
        assert_eq!(judge(&au, "a.au"), Sniff::Is(Kind::Au));
        au[15] = 23; // G.721
        assert_eq!(judge(&au, "a.au"), Sniff::Refused);
        let mut voc = b"Creative Voice File".to_vec();
        voc.extend_from_slice(&[26, 0, 0x0A, 0x01, 0x29, 0x11]);
        voc.extend_from_slice(&[1, 4, 0, 0, 156, 0, 128, 128]);
        assert_eq!(judge(&voc, "a.voc"), Sniff::Is(Kind::Voc));
        voc[31] = 1; // Creative's ADPCM
        assert_eq!(judge(&voc, "a.voc"), Sniff::Refused);
        assert!(allowed_name("x.SND") && allowed_name("y.It") && allowed_name("z.voc"));
    }

    #[test]
    fn junk() {
        assert_eq!(judge(b"hello this is not audio at all", "a.mp3"), Sniff::Damaged);
        assert_eq!(judge(b"hello this is not audio at all", "a.txt"), Sniff::Refused);
    }
}
