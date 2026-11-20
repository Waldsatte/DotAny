//! The sample entries the new film describes its tracks with, and the little of each stream's own headers that has to be
//! read to write them (or to find where one sound frame ends in an AVI, which doesn't say).

use crate::bits::{Bits, be16, le16, le32};
use crate::mp4::{bx, full};

/// A track's sample entry, written for the kind of film it goes in (`bytes`): an MP4 (`isom`) when every track can be
/// in one, else a QuickTime film. In WKWebView, MP3, MP2 and 16-bit PCM play only from a QuickTime film, and there a
/// compressed sound needs the version 1 description with its setup inside a `wave` box (as QuickTime Player writes
/// it); Opus and FLAC play only from an MP4. Motion JPEG plays from either; DV only from QuickTime.
#[derive(Clone, Debug)]
pub(crate) enum Entry {
    Picture {
        kind: [u8; 4],
        width: u32,
        height: u32,
        shown: (u32, u32),
        extra: Vec<u8>,
    },
    /// `config` is the box the decoder needs (`esds`, `dac3`…), or nothing.
    Sound {
        kind: [u8; 4],
        channels: u32,
        rate: u32,
        config: Vec<u8>,
    },
}

pub(crate) fn visual(kind: &[u8; 4], width: u32, height: u32, shown: (u32, u32), extra: &[u8]) -> Entry {
    Entry::Picture { kind: *kind, width, height, shown, extra: extra.to_vec() }
}

pub(crate) fn audio(kind: &[u8; 4], channels: u32, rate: u32, extra: &[u8]) -> Entry {
    Entry::Sound { kind: *kind, channels, rate, config: extra.to_vec() }
}

impl Entry {
    /// Only a QuickTime film can hold it.
    pub fn quicktime(&self) -> bool {
        match self {
            Entry::Picture { kind, .. } => matches!(kind, b"dvc " | b"dvcp"),
            Entry::Sound { kind, .. } => matches!(kind, b".mp3" | b".mp2" | b"sowt" | b"twos"),
        }
    }

    pub fn bytes(&self, quicktime: bool) -> Vec<u8> {
        match self {
            Entry::Picture { kind, width, height, shown, extra } => {
                // Motion JPEG in an MP4 is MPEG-4's JPEG stream
                if kind == b"jpeg" && !quicktime {
                    return picture(b"mp4v", *width, *height, *shown, &esds(0x6C, true, &[]));
                }
                picture(kind, *width, *height, *shown, extra)
            }
            Entry::Sound { kind, channels, rate, config } => {
                let mut b = vec![0u8; 6];
                b.extend_from_slice(&1u16.to_be_bytes());
                let pcm = matches!(kind, b"sowt" | b"twos");
                let v1 = quicktime && !pcm;
                b.extend_from_slice(&(v1 as u16).to_be_bytes());
                b.extend_from_slice(&[0; 6]);
                b.extend_from_slice(&(*channels as u16).to_be_bytes());
                b.extend_from_slice(&16u16.to_be_bytes());
                b.extend_from_slice(&(if v1 { -2i16 } else { 0 }).to_be_bytes());
                b.extend_from_slice(&[0; 2]);
                b.extend_from_slice(&((*rate).min(0xFFFF) << 16).to_be_bytes());
                if !v1 {
                    b.extend_from_slice(config);
                    return bx(kind, &b);
                }
                let per_packet: u32 = match kind {
                    b".mp3" | b".mp2" => 1152,
                    b"mp4a" => 1024,
                    _ => 0,
                };
                for v in [per_packet, 0, 0, 2] {
                    b.extend_from_slice(&v.to_be_bytes());
                }
                if !config.is_empty() {
                    let mut wave = bx(b"frma", kind);
                    if kind == b"mp4a" {
                        wave.extend(bx(b"mp4a", &[0; 4]));
                    }
                    wave.extend_from_slice(config);
                    wave.extend_from_slice(&[0, 0, 0, 8, 0, 0, 0, 0]);
                    b.extend(bx(b"wave", &wave));
                }
                bx(kind, &b)
            }
        }
    }
}

/// A picture track's entry box (`avc1`, `hvc1`, `mp4v`, `jpeg`, `dvc `…): the fixed fields, then `extra` (its `avcC`…).
fn picture(kind: &[u8; 4], width: u32, height: u32, shown: (u32, u32), extra: &[u8]) -> Vec<u8> {
    let mut b = vec![0u8; 6];
    b.extend_from_slice(&1u16.to_be_bytes());
    b.extend_from_slice(&[0; 16]);
    b.extend_from_slice(&(width.min(0xFFFF) as u16).to_be_bytes());
    b.extend_from_slice(&(height.min(0xFFFF) as u16).to_be_bytes());
    b.extend_from_slice(&0x0048_0000u32.to_be_bytes());
    b.extend_from_slice(&0x0048_0000u32.to_be_bytes());
    b.extend_from_slice(&[0; 4]);
    b.extend_from_slice(&1u16.to_be_bytes());
    b.extend_from_slice(&[0; 32]);
    b.extend_from_slice(&0x0018u16.to_be_bytes());
    b.extend_from_slice(&(-1i16).to_be_bytes());
    b.extend_from_slice(extra);
    // pixels that aren't square: their shape, so the picture shows at `shown`
    if width > 0
        && height > 0
        && shown.0 > 0
        && shown.1 > 0
        && shown.0 as u64 * height as u64 != shown.1 as u64 * width as u64
    {
        let (h, v) = (shown.0 as u64 * height as u64, shown.1 as u64 * width as u64);
        let g = gcd(h, v);
        let mut p = ((h / g) as u32).to_be_bytes().to_vec();
        p.extend_from_slice(&((v / g) as u32).to_be_bytes());
        b.extend(bx(b"pasp", &p));
    }
    bx(kind, &b)
}

fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 { a.max(1) } else { gcd(b, a % b) }
}

fn descriptor(tag: u8, body: &[u8]) -> Vec<u8> {
    let n = body.len() as u32;
    let mut b = vec![
        tag,
        0x80 | (n >> 21 & 0x7F) as u8,
        0x80 | (n >> 14 & 0x7F) as u8,
        0x80 | (n >> 7 & 0x7F) as u8,
        (n & 0x7F) as u8,
    ];
    b.extend_from_slice(body);
    b
}

/// MPEG-4's elementary stream descriptor: which kind of stream (`object`: 0x40 AAC, 0x20 MPEG-4 Part 2, 0x61 MPEG-2…),
/// and what its decoder needs first (`setup`).
pub(crate) fn esds(object: u8, video: bool, setup: &[u8]) -> Vec<u8> {
    let mut config = vec![object, (if video { 4 } else { 5 }) << 2 | 1, 0, 0, 0];
    config.extend_from_slice(&[0; 8]);
    if !setup.is_empty() {
        config.extend(descriptor(5, setup));
    }
    let mut es = vec![0, 0, 0];
    es.extend(descriptor(4, &config));
    es.extend(descriptor(6, &[2]));
    full(b"esds", 0, 0, &descriptor(3, &es))
}

const AAC_RATES: [u32; 13] = [96000, 88200, 64000, 48000, 44100, 32000, 24000, 22050, 16000, 12000, 11025, 8000, 7350];

/// The rate an AAC stream's own setup names (the core rate, before any SBR doubling).
pub(crate) fn aac_rate(asc: &[u8]) -> Option<u32> {
    let mut r = Bits::new(asc);
    let mut object = r.read(5)?;
    if object == 31 {
        object = 32 + r.read(6)?;
    }
    let _ = object;
    let i = r.read(4)?;
    if i == 15 { r.read(24) } else { AAC_RATES.get(i as usize).copied() }
}

/// An AAC setup for a stream that came without one (an old MKV names its profile in the codec instead).
pub(crate) fn aac_setup(profile: u32, rate: u32, channels: u32) -> Vec<u8> {
    let i = AAC_RATES.iter().position(|r| *r == rate).unwrap_or(4) as u32;
    let v = profile << 11 | i << 7 | (channels & 15) << 3;
    (v as u16).to_be_bytes().to_vec()
}

/// AC-3's frame sizes in 16-bit words, by `frmsizecod / 2`, for 48, 44.1 and 32 kHz.
const AC3_WORDS: [[u16; 3]; 19] = [
    [64, 69, 96],
    [80, 87, 120],
    [96, 104, 144],
    [112, 121, 168],
    [128, 139, 192],
    [160, 174, 240],
    [192, 208, 288],
    [224, 243, 336],
    [256, 278, 384],
    [320, 348, 480],
    [384, 417, 576],
    [448, 487, 672],
    [512, 557, 768],
    [640, 696, 960],
    [768, 835, 1152],
    [896, 975, 1344],
    [1024, 1114, 1536],
    [1152, 1253, 1728],
    [1280, 1393, 1920],
];
const AC3_RATES: [u32; 3] = [48000, 44100, 32000];

pub(crate) struct Ac3 {
    pub rate: u32,
    pub channels: u32,
    pub len: usize,
    /// `dac3`'s three bytes.
    pub setup: [u8; 3],
}

/// An AC-3 frame's header.
pub(crate) fn ac3(b: &[u8]) -> Option<Ac3> {
    if be16(b, 0)? != 0x0B77 {
        return None;
    }
    let mut r = Bits::new(b.get(4..)?);
    let fscod = r.read(2)?;
    let frmsizecod = r.read(6)?;
    let bsid = r.read(5)?;
    let bsmod = r.read(3)?;
    let acmod = r.read(3)?;
    if fscod > 2 || frmsizecod > 37 || bsid > 10 {
        return None;
    }
    if acmod & 1 != 0 && acmod != 1 {
        r.read(2)?;
    }
    if acmod & 4 != 0 {
        r.read(2)?;
    }
    if acmod == 2 {
        r.read(2)?;
    }
    let lfe = r.read(1)?;
    let words = AC3_WORDS[(frmsizecod / 2) as usize][fscod as usize] as usize
        + if fscod == 1 { (frmsizecod & 1) as usize } else { 0 };
    let v = fscod << 22 | bsid << 17 | bsmod << 14 | acmod << 11 | lfe << 10 | (frmsizecod / 2) << 5;
    Some(Ac3 {
        rate: AC3_RATES[fscod as usize],
        channels: [2, 1, 2, 3, 3, 4, 4, 5][acmod as usize] + lfe,
        len: words * 2,
        setup: [(v >> 16) as u8, (v >> 8) as u8, v as u8],
    })
}

pub(crate) struct Eac3 {
    pub rate: u32,
    pub channels: u32,
    /// Sound frames in one of its frames: 256 per block.
    pub frames: u32,
    pub setup: Vec<u8>,
}

/// An E-AC-3 frame's header: one independent stream, as nearly every film has.
pub(crate) fn eac3(b: &[u8]) -> Option<Eac3> {
    if be16(b, 0)? != 0x0B77 {
        return None;
    }
    let mut r = Bits::new(b.get(2..)?);
    let _strmtyp = r.read(2)?;
    let _substream = r.read(3)?;
    let frmsiz = r.read(11)?;
    let fscod = r.read(2)?;
    let (rate, blocks) = if fscod == 3 {
        let half = r.read(2)?;
        ([24000, 22050, 16000].get(half as usize).copied()?, 6)
    } else {
        (AC3_RATES[fscod as usize], [1, 2, 3, 6][r.read(2)? as usize])
    };
    let acmod = r.read(3)?;
    let lfe = r.read(1)?;
    let bsid = r.read(5)?;
    if !(11..=16).contains(&bsid) {
        return None;
    }
    let bytes = (frmsiz as u64 + 1) * 2;
    let kbps = (bytes * 8 * rate as u64 / (256 * blocks as u64) / 1000) as u32;
    // data rate 13, one independent stream (0), fscod 2, bsid 5, reserved, asvc, bsmod 3, acmod 3, lfe, reserved 3, no
    // dependent streams 4, reserved
    let mut v: u64 = (kbps as u64 & 0x1FFF) << 27;
    v |= (fscod.min(2) as u64) << 22 | (bsid as u64) << 17 | (acmod as u64) << 9 | (lfe as u64) << 8;
    let setup = v.to_be_bytes()[3..].to_vec();
    Some(Eac3 { rate, channels: [2, 1, 2, 3, 3, 4, 4, 5][acmod as usize] + lfe, frames: 256 * blocks, setup })
}

pub(crate) struct Mpa {
    pub len: usize,
    pub rate: u32,
    pub channels: u32,
    pub frames: u32,
    /// 1, 2 or 3.
    pub layer: u32,
}

/// An MPEG audio frame's header (MP3, MP2).
pub(crate) fn mpa(b: &[u8]) -> Option<Mpa> {
    let h = u32::from_be_bytes(b.get(..4)?.try_into().ok()?);
    if h >> 21 != 0x7FF {
        return None;
    }
    let version = h >> 19 & 3; // 3: MPEG-1, 2: MPEG-2, 0: MPEG-2.5
    let layer = 4 - (h >> 17 & 3);
    let bitrate = h >> 12 & 15;
    let ri = h >> 10 & 3;
    if version == 1 || layer == 4 || bitrate == 0 || bitrate == 15 || ri == 3 {
        return None;
    }
    let pad = h >> 9 & 1;
    let rate = [44100, 48000, 32000][ri as usize]
        >> if version == 3 {
            0
        } else if version == 2 {
            1
        } else {
            2
        };
    const V1: [[u32; 15]; 3] = [
        [0, 32, 64, 96, 128, 160, 192, 224, 256, 288, 320, 352, 384, 416, 448],
        [0, 32, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 384],
        [0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320],
    ];
    const V2: [[u32; 15]; 2] = [
        [0, 32, 48, 56, 64, 80, 96, 112, 128, 144, 160, 176, 192, 224, 256],
        [0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160],
    ];
    let kbps = if version == 3 {
        V1[layer as usize - 1][bitrate as usize]
    } else {
        V2[(layer > 1) as usize][bitrate as usize]
    };
    let (len, frames) = match layer {
        1 => ((12 * kbps * 1000 / rate + pad) * 4, 384),
        2 => (144 * kbps * 1000 / rate + pad, 1152),
        _ if version == 3 => (144 * kbps * 1000 / rate + pad, 1152),
        _ => (72 * kbps * 1000 / rate + pad, 576),
    };
    let channels = if h >> 6 & 3 == 3 { 1 } else { 2 };
    (len >= 4).then_some(Mpa { len: len as usize, rate, channels, frames, layer })
}

/// Opus's `dOps` from the `OpusHead` an MKV keeps.
pub(crate) fn dops(head: &[u8]) -> Option<(Vec<u8>, u32)> {
    if !head.starts_with(b"OpusHead") || head.len() < 19 {
        return None;
    }
    let channels = head[9];
    let mut b = vec![0, channels];
    b.extend_from_slice(&le16(head, 10)?.to_be_bytes());
    b.extend_from_slice(&le32(head, 12)?.to_be_bytes());
    b.extend_from_slice(&le16(head, 16)?.to_be_bytes());
    b.push(head[18]);
    if head[18] != 0 {
        b.extend_from_slice(head.get(19..21 + channels as usize)?);
    }
    Some((bx(b"dOps", &b), channels as u32))
}

/// How many 48 kHz sound frames an Opus packet holds, from its first bytes.
pub(crate) fn opus_frames(p: &[u8]) -> Option<u32> {
    let toc = *p.first()?;
    let config = toc >> 3;
    let each = match config {
        0..=11 => [480, 960, 1920, 2880][(config & 3) as usize],
        12..=15 => [480, 960][(config & 1) as usize],
        _ => [120, 240, 480, 960][(config & 3) as usize],
    };
    let count = match toc & 3 {
        0 => 1,
        1 | 2 => 2,
        _ => (*p.get(1)? & 0x3F) as u32,
    };
    Some(each * count)
}

/// FLAC's `dfLa` from the `fLaC` header an MKV keeps, and the stream's rate, channels and fixed block size.
pub(crate) fn dfla(private: &[u8]) -> Option<(Vec<u8>, u32, u32, u32)> {
    let blocks = private.strip_prefix(b"fLaC").unwrap_or(private);
    let mut out = Vec::new();
    let mut at = 0;
    let mut info = None;
    while at + 4 <= blocks.len() {
        let kind = blocks[at] & 0x7F;
        let len = (blocks[at + 1] as usize) << 16 | (blocks[at + 2] as usize) << 8 | blocks[at + 3] as usize;
        let body = blocks.get(at + 4..at + 4 + len)?;
        if kind == 0 {
            let max_block = be16(body, 2)? as u32;
            let mut r = Bits::new(body.get(10..)?);
            let rate = r.read(20)?;
            let channels = r.read(3)? + 1;
            info = Some((rate, channels, max_block));
        }
        // only the stream's facts (STREAMINFO) are kept; the other metadata blocks stay behind
        if kind == 0 {
            out.push(kind);
            out.extend_from_slice(&blocks[at + 1..at + 4]);
            out.extend_from_slice(body);
        }
        at += 4 + len;
    }
    let (rate, channels, block) = info?;
    out[0] |= 0x80;
    Some((full(b"dfLa", 0, 0, &out), rate, channels, block))
}

/// MPEG-4 Part 2: what comes before the first picture in a frame (the stream's own setup, the VOL).
pub(crate) fn mp4v_setup(frame: &[u8]) -> Option<Vec<u8>> {
    let vop = frame.windows(4).position(|w| w == [0, 0, 1, 0xB6])?;
    (vop > 0).then(|| frame[..vop].to_vec())
}

/// The shape of an MPEG-4 Part 2 stream's pixels, from its setup (the VOL's `aspect_ratio_info`): width to height.
pub(crate) fn mp4v_pixel_shape(setup: &[u8]) -> Option<(u32, u32)> {
    let at = setup.windows(4).position(|w| w[0] == 0 && w[1] == 0 && w[2] == 1 && (0x20..=0x2F).contains(&w[3]))?;
    let mut r = Bits::new(&setup[at + 4..]);
    r.read(1)?;
    r.read(8)?;
    if r.read(1)? == 1 {
        r.read(7)?;
    }
    match r.read(4)? {
        2 => Some((12, 11)),
        3 => Some((10, 11)),
        4 => Some((16, 11)),
        5 => Some((40, 33)),
        15 => {
            let (w, h) = (r.read(8)?, r.read(8)?);
            (w > 0 && h > 0).then_some((w, h))
        }
        _ => None,
    }
}

/// The kind of each picture (0 I, 1 P, 2 B, 3 S) in an MPEG-4 Part 2 frame.
pub(crate) fn mp4v_pictures(frame: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 4 < frame.len() {
        if frame[i] == 0 && frame[i + 1] == 0 && frame[i + 2] == 1 && frame[i + 3] == 0xB6 {
            out.push(frame[i + 4] >> 6);
            i += 4;
        }
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mp3_frame() {
        // MPEG-1 layer III, 128 kbps, 44.1 kHz, stereo
        let m = mpa(&[0xFF, 0xFB, 0x90, 0x00]).unwrap();
        assert_eq!((m.len, m.rate, m.channels, m.frames, m.layer), (417, 44100, 2, 1152, 3));
    }

    #[test]
    fn ac3_frame() {
        // 48 kHz, 384 kbps (frmsizecod 28), bsid 8, 3/2 without LFE
        let a = ac3(&[0x0B, 0x77, 0, 0, 0b0001_1100, 0b0100_0000, 0b1110_0000, 0b0100_0000]).unwrap();
        assert_eq!((a.rate, a.len, a.channels), (48000, 1536, 5));
    }

    #[test]
    fn opus_packets() {
        // 20 ms CELT, one frame; 20 ms SILK, two frames
        assert_eq!(opus_frames(&[0b1111_1000]), Some(960));
        assert_eq!(opus_frames(&[0b0000_1001]), Some(1920));
    }

    #[test]
    fn descriptors_say_their_length() {
        let e = esds(0x40, false, &[0x12, 0x10]);
        assert_eq!(&e[4..8], b"esds");
        assert_eq!(u32::from_be_bytes(e[..4].try_into().unwrap()) as usize, e.len());
    }
}
