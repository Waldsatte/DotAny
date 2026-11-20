//! Two old sounds read by our own few hundred lines, no library: Sun/NeXT `.au` (`.snd`) and Creative Voice `.voc`.
//! Only plain samples and G.711 (μ-law, A-law) are read. Everything compressed beyond that (G.72x and Creative's ADPCM)
//! is refused, so no code for it exists here. Both are only ever read.
//!
//! AU: a 24-byte header, then the samples. The Open Group's page is the reference: pubs.opengroup.org/external/auformat.html
//! VOC: a 26-byte header, then blocks. There is no official page; the structure is as Kaitai Struct and MultimediaWiki describe it.

use std::ops::Range;

#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Bad {
    /// It is one of these files, holding something we don't read.
    Refused,
    Damaged,
}

/// How each sample is stored.
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Enc {
    U8,
    S8,
    S16Le,
    S16Be,
    S24Be,
    S32Be,
    F32Be,
    F64Be,
    MuLaw,
    ALaw,
    // the same in a little-endian AU (DEC's ".snd" written backwards)
    S24Le,
    S32Le,
    F32Le,
    F64Le,
}

impl Enc {
    pub fn bytes(self) -> usize {
        match self {
            Enc::U8 | Enc::S8 | Enc::MuLaw | Enc::ALaw => 1,
            Enc::S16Le | Enc::S16Be => 2,
            Enc::S24Be | Enc::S24Le => 3,
            Enc::S32Be | Enc::S32Le | Enc::F32Be | Enc::F32Le => 4,
            Enc::F64Be | Enc::F64Le => 8,
        }
    }
    /// Bits per sample, as the rest of Audio counts them. G.711 holds 13 or 14 bits of range: 16 is where it fits.
    pub fn bits(self) -> u32 {
        match self {
            Enc::U8 | Enc::S8 => 8,
            Enc::S16Le | Enc::S16Be | Enc::MuLaw | Enc::ALaw => 16,
            Enc::S24Be | Enc::S24Le => 24,
            Enc::S32Be | Enc::S32Le | Enc::F32Be | Enc::F32Le => 32,
            Enc::F64Be | Enc::F64Le => 64,
        }
    }
    fn one(self, b: &[u8]) -> f32 {
        match self {
            Enc::U8 => (b[0] as f32 - 128.0) / 128.0,
            Enc::S8 => b[0] as i8 as f32 / 128.0,
            Enc::S16Le => i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0,
            Enc::S16Be => i16::from_be_bytes([b[0], b[1]]) as f32 / 32768.0,
            Enc::S24Be => (i32::from_be_bytes([b[0], b[1], b[2], 0]) >> 8) as f32 / 8388608.0,
            Enc::S24Le => (i32::from_le_bytes([0, b[0], b[1], b[2]]) >> 8) as f32 / 8388608.0,
            Enc::S32Be => i32::from_be_bytes([b[0], b[1], b[2], b[3]]) as f32 / 2147483648.0,
            Enc::S32Le => i32::from_le_bytes([b[0], b[1], b[2], b[3]]) as f32 / 2147483648.0,
            Enc::F32Be => finite(f32::from_be_bytes([b[0], b[1], b[2], b[3]])),
            Enc::F32Le => finite(f32::from_le_bytes([b[0], b[1], b[2], b[3]])),
            Enc::F64Be => finite(f64::from_be_bytes(b[..8].try_into().unwrap()) as f32),
            Enc::F64Le => finite(f64::from_le_bytes(b[..8].try_into().unwrap()) as f32),
            Enc::MuLaw => mulaw(b[0]) as f32 / 32768.0,
            Enc::ALaw => alaw(b[0]) as f32 / 32768.0,
        }
    }
}

// a float file can hold anything; what isn't a number is silence
fn finite(v: f32) -> f32 {
    if v.is_finite() { v } else { 0.0 }
}

/// ITU-T G.711 μ-law to linear, as 16-bit.
pub fn mulaw(u: u8) -> i16 {
    let u = !u;
    let t = ((((u & 0x0F) as i32) << 3) + 0x84) << ((u & 0x70) >> 4);
    (if u & 0x80 != 0 { 0x84 - t } else { t - 0x84 }) as i16
}

/// ITU-T G.711 A-law to linear, as 16-bit.
pub fn alaw(a: u8) -> i16 {
    let a = a ^ 0x55;
    let mut t = ((a & 0x0F) as i32) << 4;
    let seg = (a & 0x70) >> 4;
    match seg {
        0 => t += 8,
        1 => t += 0x108,
        _ => t = (t + 0x108) << (seg - 1),
    }
    (if a & 0x80 != 0 { t } else { -t }) as i16
}

/// Samples of one shape, side by side.
#[derive(Debug, PartialEq, Clone, Copy)]
pub struct Shape {
    pub channels: usize,
    pub rate: u32,
    pub enc: Enc,
}

#[derive(Debug, PartialEq, Clone)]
pub enum Part {
    /// Samples at these bytes of the file.
    Sound(Shape, Range<usize>),
    /// Quiet, this many seconds of it.
    Silence(f64),
}

/// Decodes a run of whole frames. Bytes that don't make a whole frame are left out.
pub fn decode(shape: &Shape, bytes: &[u8], out: &mut Vec<f32>) {
    let w = shape.enc.bytes();
    let frame = w * shape.channels;
    let n = bytes.len() / frame * frame;
    out.clear();
    out.reserve(n / w);
    out.extend(bytes[..n].chunks_exact(w).map(|s| shape.enc.one(s)));
}

fn be32(b: &[u8], at: usize) -> Option<u32> {
    b.get(at..at + 4).map(|x| u32::from_be_bytes(x.try_into().unwrap()))
}
fn le32(b: &[u8], at: usize) -> Option<u32> {
    b.get(at..at + 4).map(|x| u32::from_le_bytes(x.try_into().unwrap()))
}
fn le16(b: &[u8], at: usize) -> Option<u16> {
    b.get(at..at + 2).map(|x| u16::from_le_bytes(x.try_into().unwrap()))
}
fn le24(b: &[u8], at: usize) -> Option<u32> {
    b.get(at..at + 3).map(|x| x[0] as u32 | (x[1] as u32) << 8 | (x[2] as u32) << 16)
}

// what nobody records at: a rate or channel count outside these is damage, not sound
const RATES: std::ops::RangeInclusive<u32> = 1000..=768_000;
const MAX_CHANNELS: usize = 32;

fn sane(channels: usize, rate: u32) -> bool {
    (1..=MAX_CHANNELS).contains(&channels) && RATES.contains(&rate)
}

// ---------- WAV in G.711 ----------

/// The format tag of a WAV's `fmt ` chunk (an extensible one's sub-format), or None when there isn't one in `b`.
pub fn wav_tag(b: &[u8]) -> Option<u16> {
    wav_chunks(b).ok().and_then(|(fmt, _)| fmt).map(|(tag, ..)| tag)
}

/// A WAV whose samples are G.711 (A-law, tag 6; μ-law, tag 7): telephone and dictation recorders write these, and they
/// are as plain to read as an AU's. Symphonia's reader only takes their `fmt ` chunk at exactly 18 bytes.
pub fn is_g711_wav(b: &[u8]) -> bool {
    matches!(wav_tag(b), Some(6 | 7))
}

// the `fmt ` chunk's (tag, channels, rate, bits) and where `data` is, as far as the file goes
#[allow(clippy::type_complexity)]
fn wav_chunks(b: &[u8]) -> Result<(Option<(u16, usize, u32, u16)>, Option<Range<usize>>), Bad> {
    if b.get(0..4) != Some(b"RIFF") || b.get(8..12) != Some(b"WAVE") {
        return Err(Bad::Damaged);
    }
    let (mut fmt, mut data) = (None, None);
    let mut p = 12;
    while let Some(len) = le32(b, p + 4) {
        let start = p + 8;
        let end = start.saturating_add(len as usize);
        match b.get(p..p + 4) {
            Some(b"fmt ") => {
                let mut tag = le16(b, start).ok_or(Bad::Damaged)?;
                if tag == 0xFFFE {
                    // WAVE_FORMAT_EXTENSIBLE: the sub-format's first two bytes say which kind of samples
                    tag = le16(b, start + 24).ok_or(Bad::Damaged)?;
                }
                let channels = le16(b, start + 2).ok_or(Bad::Damaged)? as usize;
                let rate = le32(b, start + 4).ok_or(Bad::Damaged)?;
                let bits = le16(b, start + 14).ok_or(Bad::Damaged)?;
                fmt = Some((tag, channels, rate, bits));
            }
            // a recording stopped early says more than it has: what is there is the sound
            Some(b"data") => {
                data = Some(start.min(b.len())..end.min(b.len()));
                break;
            }
            _ => {}
        }
        p = end + (len as usize & 1);
    }
    Ok((fmt, data))
}

/// The sound of a G.711 WAV. Any other kind of WAV is Refused here (Symphonia reads plain samples).
pub fn wav(b: &[u8]) -> Result<Part, Bad> {
    let (fmt, data) = wav_chunks(b)?;
    let (tag, channels, rate, bits) = fmt.ok_or(Bad::Damaged)?;
    let enc = match (tag, bits) {
        (6, 8) => Enc::ALaw,
        (7, 8) => Enc::MuLaw,
        _ => return Err(Bad::Refused),
    };
    if !sane(channels, rate) {
        return Err(Bad::Damaged);
    }
    Ok(Part::Sound(Shape { channels, rate, enc }, data.ok_or(Bad::Damaged)?))
}

// ---------- AU ----------

pub fn is_au(b: &[u8]) -> bool {
    b.starts_with(b".snd") || b.starts_with(b"dns.")
}

/// The sound in an AU file. `len` is the whole file's length; `b` may be only its start, for a first look.
pub fn au(b: &[u8], len: usize) -> Result<Part, Bad> {
    let little = b.starts_with(b"dns.");
    if !is_au(b) {
        return Err(Bad::Damaged);
    }
    let get = |at| if little { le32(b, at) } else { be32(b, at) };
    let (Some(offset), Some(size), Some(code), Some(rate), Some(channels)) =
        (get(4), get(8), get(12), get(16), get(20))
    else {
        return Err(Bad::Damaged);
    };
    let enc = match (code, little) {
        (1, _) => Enc::MuLaw,
        (2, _) => Enc::S8,
        (3, false) => Enc::S16Be,
        (3, true) => Enc::S16Le,
        (4, false) => Enc::S24Be,
        (4, true) => Enc::S24Le,
        (5, false) => Enc::S32Be,
        (5, true) => Enc::S32Le,
        (6, false) => Enc::F32Be,
        (6, true) => Enc::F32Le,
        (7, false) => Enc::F64Be,
        (7, true) => Enc::F64Le,
        (27, _) => Enc::ALaw,
        // G.721, G.722, G.723, DSP data, fragmented sound: real AU files, not ours to read
        (8..=26, _) => return Err(Bad::Refused),
        _ => return Err(Bad::Damaged),
    };
    let channels = channels as usize;
    if offset < 24 || !sane(channels, rate) {
        return Err(Bad::Damaged);
    }
    let start = offset as usize;
    // the size may be "unknown" (all ones), or more than the file has if it was cut off: the file's end decides then
    let end = if size == u32::MAX { len } else { len.min(start.saturating_add(size as usize)) };
    if start > end {
        return Err(Bad::Damaged);
    }
    Ok(Part::Sound(Shape { channels, rate, enc }, start..end))
}

// ---------- VOC ----------

const VOC_MAGIC: &[u8] = b"Creative Voice File\x1A";

pub fn is_voc(b: &[u8]) -> bool {
    b.starts_with(VOC_MAGIC)
}

// the old blocks give the rate as a divisor of a megahertz
fn divisor_rate(d: u8) -> u32 {
    1_000_000 / (256 - d as u32)
}

const STANDARD: [u32; 9] = [8000, 11025, 16000, 22050, 24000, 32000, 44100, 48000, 96000];

/// A divisor or time constant only comes near the rate that was meant (44100 comes out as 44107; 11025 as 10989).
/// Within half a percent, the standard rate is taken: nobody hears that, and saved files get a rate players expect.
pub fn snap(rate: u32) -> u32 {
    STANDARD.iter().copied().find(|s| (rate as f64 - *s as f64).abs() <= *s as f64 * 0.005).unwrap_or(rate)
}

/// The parts of a VOC file, in order. `b` may be only the file's start, for a first look: a block cut off by its end
/// keeps what is there. Repeats are heard once; markers and text are skipped.
pub fn voc(b: &[u8]) -> Result<Vec<Part>, Bad> {
    if !is_voc(b) {
        return Err(Bad::Damaged);
    }
    let (Some(first), Some(version), Some(check)) = (le16(b, 20), le16(b, 22), le16(b, 24)) else {
        return Err(Bad::Damaged);
    };
    // the header's own check: the version's complement plus 0x1234
    if check != (!version).wrapping_add(0x1234) || (first as usize) < 26 {
        return Err(Bad::Damaged);
    }
    let mut parts = Vec::new();
    let mut p = first as usize;
    // what block 8 says the next block 1 really is
    let mut extended: Option<(usize, u32)> = None;
    // the last sound's shape, for block 2 (more of the same)
    let mut last: Option<Shape> = None;
    let mut refused = false;
    while let Some(&kind) = b.get(p) {
        if kind == 0 {
            break;
        }
        let Some(size) = le24(b, p + 1) else { break };
        let body = p + 4;
        let end = body.saturating_add(size as usize);
        let here = body.min(b.len())..end.min(b.len());
        match kind {
            1 => {
                let (Some(&div), Some(&codec)) = (b.get(body), b.get(body + 1)) else { break };
                let (channels, rate) = extended.take().unwrap_or((1, snap(divisor_rate(div))));
                match codec {
                    0 => {
                        let shape = Shape { channels, rate, enc: Enc::U8 };
                        if !sane(channels, rate) {
                            return Err(Bad::Damaged);
                        }
                        parts.push(Part::Sound(shape, (here.start + 2).min(here.end)..here.end));
                        last = Some(shape);
                    }
                    // Creative's 4-, 3- and 2.6-bit ADPCM, and 16-bit in this old block which nothing wrote
                    _ => {
                        refused = true;
                        last = None;
                    }
                }
            }
            2 => {
                if let Some(shape) = last {
                    parts.push(Part::Sound(shape, here.clone()));
                }
            }
            3 => {
                let (Some(n), Some(&div)) = (le16(b, body), b.get(body + 2)) else { break };
                parts.push(Part::Silence((n as f64 + 1.0) / divisor_rate(div) as f64));
            }
            8 => {
                // its packing byte repeats what the next block 1 says about its codec, so only that one is read
                let (Some(tc), Some(&mode)) = (le16(b, body), b.get(body + 3)) else { break };
                let channels = if mode == 1 { 2 } else { 1 };
                let rate = snap((256_000_000u64 / (channels as u64 * (65536 - tc as u64))) as u32);
                extended = Some((channels, rate));
            }
            9 => {
                let (Some(rate), Some(&bits), Some(&channels), Some(codec)) =
                    (le32(b, body), b.get(body + 4), b.get(body + 5), le16(b, body + 6))
                else {
                    break;
                };
                let enc = match (codec, bits) {
                    (0, 8) => Some(Enc::U8),
                    (4, 16) => Some(Enc::S16Le),
                    (6, _) => Some(Enc::ALaw),
                    (7, _) => Some(Enc::MuLaw),
                    _ => None,
                };
                match enc {
                    Some(enc) => {
                        let channels = channels as usize;
                        if !sane(channels, rate) {
                            return Err(Bad::Damaged);
                        }
                        let shape = Shape { channels, rate, enc };
                        parts.push(Part::Sound(shape, (here.start + 12).min(here.end)..here.end));
                        last = Some(shape);
                    }
                    None => {
                        refused = true;
                        last = None;
                    }
                }
            }
            // 4 marker, 5 text, 6 and 7 repeat: nothing to hear
            4..=7 => {}
            _ => return Err(Bad::Damaged),
        }
        if end >= b.len() {
            break;
        }
        p = end;
    }
    if !parts.iter().any(|x| matches!(x, Part::Sound(..))) {
        return Err(if refused { Bad::Refused } else { Bad::Damaged });
    }
    // a file that mixes plain blocks with compressed ones would play with holes in it: it's refused whole
    if refused {
        return Err(Bad::Refused);
    }
    Ok(parts)
}

/// The first shape a list of parts sounds in: what the whole file is read as.
pub fn first_shape(parts: &[Part]) -> Option<Shape> {
    parts.iter().find_map(|p| match p {
        Part::Sound(s, _) => Some(*s),
        Part::Silence(_) => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn au_file(code: u32, rate: u32, channels: u32, data: &[u8], size: Option<u32>) -> Vec<u8> {
        let mut f = b".snd".to_vec();
        // 24 bytes of header and 8 of note, as most writers leave it
        for v in [32, size.unwrap_or(data.len() as u32), code, rate, channels, 0, 0] {
            f.extend_from_slice(&v.to_be_bytes());
        }
        f.extend_from_slice(data);
        f
    }

    fn voc_head() -> Vec<u8> {
        let mut f = VOC_MAGIC.to_vec();
        let version: u16 = 0x010A;
        f.extend_from_slice(&26u16.to_le_bytes());
        f.extend_from_slice(&version.to_le_bytes());
        f.extend_from_slice(&(!version).wrapping_add(0x1234).to_le_bytes());
        f
    }
    fn block(kind: u8, body: &[u8]) -> Vec<u8> {
        let n = body.len() as u32;
        let mut v = vec![kind, n as u8, (n >> 8) as u8, (n >> 16) as u8];
        v.extend_from_slice(body);
        v
    }

    fn read_all(b: &[u8], part: &Part) -> Vec<f32> {
        let mut out = Vec::new();
        if let Part::Sound(s, r) = part {
            decode(s, &b[r.clone()], &mut out);
        }
        out
    }

    #[test]
    fn g711_matches_the_reference_values() {
        // G.711's own extremes and zero points
        assert_eq!(mulaw(0x00), -32124);
        assert_eq!(mulaw(0x80), 32124);
        assert_eq!(mulaw(0xFF), 0);
        assert_eq!(mulaw(0x7F), 0);
        assert_eq!(alaw(0xD5), 8);
        assert_eq!(alaw(0x55), -8);
        assert_eq!(alaw(0xAA), 32256);
        assert_eq!(alaw(0x2A), -32256);
        // every code decodes into range, and the curves only rise
        let mu: Vec<i16> = (0..128u8).map(|u| mulaw(0xFF - u)).collect();
        assert!(mu.windows(2).all(|w| w[0] <= w[1]));
    }

    #[test]
    fn au_plain_and_g711() {
        let f = au_file(3, 8000, 1, &[0x40, 0x00, 0xC0, 0x00, 0x7F], None);
        let part = au(&f, f.len()).unwrap();
        assert_eq!(part, Part::Sound(Shape { channels: 1, rate: 8000, enc: Enc::S16Be }, 32..37));
        // the odd byte at the end isn't a whole sample
        assert_eq!(read_all(&f, &part), vec![0.5, -0.5]);
        let f = au_file(1, 8000, 2, &[0xFF, 0x80, 0x7F, 0x00], None);
        let part = au(&f, f.len()).unwrap();
        let s = read_all(&f, &part);
        assert_eq!(s.len(), 4);
        assert!(s[0] == 0.0 && s[1] > 0.98 && s[3] < -0.98);
        assert!(matches!(
            au(&au_file(27, 8000, 1, &[0xD5], None), 33).unwrap(),
            Part::Sound(Shape { enc: Enc::ALaw, .. }, _)
        ));
        assert!(matches!(
            au(&au_file(6, 44100, 2, &[], None), 32).unwrap(),
            Part::Sound(Shape { enc: Enc::F32Be, .. }, _)
        ));
    }

    #[test]
    fn au_size_unknown_or_too_large_is_the_file_end() {
        let f = au_file(2, 11025, 1, &[1, 2, 3, 4], Some(u32::MAX));
        assert_eq!(au(&f, f.len()).unwrap(), Part::Sound(Shape { channels: 1, rate: 11025, enc: Enc::S8 }, 32..36));
        let f = au_file(2, 11025, 1, &[1, 2, 3, 4], Some(1000));
        assert_eq!(au(&f, f.len()).unwrap(), Part::Sound(Shape { channels: 1, rate: 11025, enc: Enc::S8 }, 32..36));
        // only the header is looked at first; the length comes from the file
        assert_eq!(
            au(&f[..24], 4000).unwrap(),
            Part::Sound(Shape { channels: 1, rate: 11025, enc: Enc::S8 }, 32..1032)
        );
    }

    #[test]
    fn au_little_endian() {
        let mut f = b"dns.".to_vec();
        for v in [24u32, 4, 3, 22050, 1] {
            f.extend_from_slice(&v.to_le_bytes());
        }
        f.extend_from_slice(&16384i16.to_le_bytes());
        f.extend_from_slice(&(-16384i16).to_le_bytes());
        let part = au(&f, f.len()).unwrap();
        assert_eq!(read_all(&f, &part), vec![0.5, -0.5]);
    }

    #[test]
    fn au_compressed_is_refused_and_nonsense_is_damage() {
        assert_eq!(au(&au_file(23, 8000, 1, &[0; 8], None), 40), Err(Bad::Refused));
        assert_eq!(au(&au_file(25, 8000, 1, &[0; 8], None), 40), Err(Bad::Refused));
        assert_eq!(au(&au_file(99, 8000, 1, &[0; 8], None), 40), Err(Bad::Damaged));
        assert_eq!(au(&au_file(3, 0, 1, &[0; 8], None), 40), Err(Bad::Damaged));
        assert_eq!(au(&au_file(3, 8000, 0, &[0; 8], None), 40), Err(Bad::Damaged));
        assert_eq!(au(&au_file(3, 8000, 900, &[0; 8], None), 40), Err(Bad::Damaged));
        assert_eq!(au(b".snd\0\0", 6), Err(Bad::Damaged));
        let mut f = au_file(3, 8000, 1, &[0; 8], None);
        f[7] = 8; // data before the header ends
        assert_eq!(au(&f, f.len()), Err(Bad::Damaged));
    }

    #[test]
    fn voc_old_blocks_with_silence_and_more_of_the_same() {
        let mut f = voc_head();
        // 8-bit at 1 MHz / (256 - 156) = 10 kHz
        f.extend(block(1, &[156, 0, 128, 255, 0]));
        f.extend(block(3, &[99, 0, 156]));
        f.extend(block(2, &[192]));
        f.extend(block(5, b"hello\0"));
        f.push(0);
        let parts = voc(&f).unwrap();
        let shape = Shape { channels: 1, rate: 10_000, enc: Enc::U8 };
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0], Part::Sound(shape, 32..35));
        assert_eq!(parts[1], Part::Silence(0.01));
        assert_eq!(parts[2], Part::Sound(shape, 46..47));
        assert_eq!(read_all(&f, &parts[0]), vec![0.0, 127.0 / 128.0, -1.0]);
        assert_eq!(read_all(&f, &parts[2]), vec![0.5]);
    }

    #[test]
    fn voc_stereo_through_block_8() {
        let mut f = voc_head();
        // 44.1 kHz stereo: 65536 - 256 000 000 / (2 × 44100)
        let tc = (65536 - 256_000_000 / (2 * 44100)) as u16;
        let mut b8 = tc.to_le_bytes().to_vec();
        b8.extend_from_slice(&[0, 1]);
        f.extend(block(8, &b8));
        f.extend(block(1, &[0, 0, 128, 128, 255, 0]));
        let parts = voc(&f).unwrap();
        match &parts[0] {
            Part::Sound(s, r) => {
                assert_eq!((s.channels, s.enc, r.len()), (2, Enc::U8, 4));
                // the time constant only comes close; a standard rate within half a percent is what was meant
                assert_eq!(s.rate, 44100);
            }
            p => panic!("{p:?}"),
        }
    }

    #[test]
    fn voc_new_block_16_bit_and_g711() {
        let mut f = voc_head();
        let mut b9 = 22050u32.to_le_bytes().to_vec();
        b9.extend_from_slice(&[16, 2, 4, 0, 0, 0, 0, 0]);
        b9.extend_from_slice(&16384i16.to_le_bytes());
        b9.extend_from_slice(&(-16384i16).to_le_bytes());
        f.extend(block(9, &b9));
        let parts = voc(&f).unwrap();
        assert_eq!(parts[0], Part::Sound(Shape { channels: 2, rate: 22050, enc: Enc::S16Le }, 42..46));
        assert_eq!(read_all(&f, &parts[0]), vec![0.5, -0.5]);
        let mut f = voc_head();
        let mut b9 = 8000u32.to_le_bytes().to_vec();
        b9.extend_from_slice(&[8, 1, 7, 0, 0, 0, 0, 0, 0xFF]);
        f.extend(block(9, &b9));
        assert!(matches!(voc(&f).unwrap()[0], Part::Sound(Shape { enc: Enc::MuLaw, .. }, _)));
    }

    #[test]
    fn voc_adpcm_is_refused() {
        let mut f = voc_head();
        f.extend(block(1, &[156, 1, 0x12, 0x34]));
        assert_eq!(voc(&f), Err(Bad::Refused));
        let mut f = voc_head();
        let mut b9 = 22050u32.to_le_bytes().to_vec();
        b9.extend_from_slice(&[4, 1, 0, 2, 0, 0, 0, 0, 1, 2]);
        f.extend(block(9, &b9));
        assert_eq!(voc(&f), Err(Bad::Refused));
        // plain sound followed by ADPCM would play with holes: refused whole
        let mut f = voc_head();
        f.extend(block(1, &[156, 0, 128, 128]));
        f.extend(block(1, &[156, 2, 0x12]));
        assert_eq!(voc(&f), Err(Bad::Refused));
    }

    #[test]
    fn voc_cut_off_keeps_what_is_there_and_nonsense_is_damage() {
        let mut f = voc_head();
        f.extend(block(1, &[156, 0, 1, 2, 3, 4, 5, 6, 7, 8]));
        let cut = &f[..f.len() - 5];
        assert_eq!(voc(cut).unwrap(), vec![Part::Sound(Shape { channels: 1, rate: 10_000, enc: Enc::U8 }, 32..35)]);
        // a wrong check, an unknown block, nothing to hear
        let mut bad = voc_head();
        bad[24] ^= 1;
        bad.extend(block(1, &[156, 0, 128]));
        assert_eq!(voc(&bad), Err(Bad::Damaged));
        let mut odd = voc_head();
        odd.extend(block(42, &[0, 0]));
        assert_eq!(voc(&odd), Err(Bad::Damaged));
        let mut quiet = voc_head();
        quiet.extend(block(3, &[10, 0, 156]));
        assert_eq!(voc(&quiet), Err(Bad::Damaged));
        assert_eq!(voc(b"Creative Voice File\x1A"), Err(Bad::Damaged));
    }

    // a G.711 WAV the way telephone recorders write it: fmt at 18 bytes, or 20 with two of its own, or extensible
    fn g711_wav(tag: u16, fmt_len: u32, extensible: bool, channels: u16, samples: &[u8], said: Option<u32>) -> Vec<u8> {
        let mut fmt = Vec::new();
        fmt.extend_from_slice(&(if extensible { 0xFFFE } else { tag }).to_le_bytes());
        fmt.extend_from_slice(&channels.to_le_bytes());
        fmt.extend_from_slice(&8000u32.to_le_bytes());
        fmt.extend_from_slice(&(8000 * channels as u32).to_le_bytes());
        fmt.extend_from_slice(&channels.to_le_bytes());
        fmt.extend_from_slice(&8u16.to_le_bytes());
        if extensible {
            fmt.extend_from_slice(&22u16.to_le_bytes());
            fmt.extend_from_slice(&8u16.to_le_bytes());
            fmt.extend_from_slice(&0u32.to_le_bytes());
            fmt.extend_from_slice(&tag.to_le_bytes());
            fmt.extend_from_slice(&[0, 0, 0, 0, 0x10, 0, 0x80, 0, 0, 0xAA, 0, 0x38, 0x9B, 0x71]);
        } else {
            fmt.resize(fmt_len as usize, 0);
        }
        let mut b = b"RIFF\0\0\0\0WAVE".to_vec();
        b.extend_from_slice(b"fmt ");
        b.extend_from_slice(&(fmt.len() as u32).to_le_bytes());
        b.extend_from_slice(&fmt);
        b.extend_from_slice(b"LIST\x04\0\0\0INFO");
        b.extend_from_slice(b"data");
        b.extend_from_slice(&said.unwrap_or(samples.len() as u32).to_le_bytes());
        b.extend_from_slice(samples);
        b
    }

    #[test]
    fn g711_wavs_of_every_shape() {
        let mu = [0xFFu8, 0x80, 0x00, 0x7F];
        for (fmt_len, extensible) in [(18, false), (20, false), (16, false), (0, true)] {
            let b = g711_wav(7, fmt_len, extensible, 2, &mu, None);
            assert!(is_g711_wav(&b), "{fmt_len} {extensible}");
            let Ok(Part::Sound(shape, range)) = wav(&b) else { panic!("{fmt_len} {extensible}") };
            assert_eq!((shape.channels, shape.rate, shape.enc, shape.enc.bits()), (2, 8000, Enc::MuLaw, 16));
            let mut out = Vec::new();
            decode(&shape, &b[range], &mut out);
            assert_eq!(out, mu.iter().map(|u| mulaw(*u) as f32 / 32768.0).collect::<Vec<_>>());
        }
        let a = g711_wav(6, 18, false, 1, &[0xD5, 0x55], None);
        assert!(matches!(wav(&a), Ok(Part::Sound(Shape { enc: Enc::ALaw, channels: 1, .. }, _))));
        // a recording stopped early: what is there is read
        let cut = g711_wav(7, 18, false, 1, &[0xFF, 0x80], Some(8000));
        let Ok(Part::Sound(_, range)) = wav(&cut) else { panic!() };
        assert_eq!(range.len(), 2);
        // plain samples are Symphonia's, other codecs aren't opened
        let pcm = g711_wav(1, 16, false, 1, &[0, 0], None);
        assert!(!is_g711_wav(&pcm));
        assert_eq!(wav(&pcm), Err(Bad::Refused));
        assert_eq!(wav(&g711_wav(2, 50, false, 1, &[0, 0], None)), Err(Bad::Refused));
        assert_eq!(wav(b"RIFF....AVI LIST"), Err(Bad::Damaged));
    }
}
