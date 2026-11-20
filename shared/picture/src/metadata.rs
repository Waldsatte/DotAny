//! Where each container keeps what isn't pixels: colour profile, EXIF, XMP and so on. Reading it, carrying it across,
//! taking it out without touching the pixels, and checking in the finished bytes that it really is gone.

use crate::formats::Kind;
use std::io::{Read, Write};

#[derive(Default, Clone, Debug)]
pub struct Meta {
    pub icc: Option<Vec<u8>>,
    /// An EXIF block, starting at its II or MM.
    pub exif: Option<Vec<u8>>,
    pub xmp: Option<Vec<u8>>,
    /// Photoshop's APP13 block, which holds IPTC; for a TIFF, its IPTC tag.
    pub iptc: Option<Vec<u8>>,
    pub comment: Option<Vec<u8>>,
    /// PNG text chunks, as they were: (chunk type, chunk data).
    pub png_text: Vec<([u8; 4], Vec<u8>)>,
    /// Something else that isn't needed to show the picture: other JPEG segments, or data after its end.
    pub other: bool,
}

impl Meta {
    /// Anything a person might call "photo details". A colour profile isn't: it says how to show the colours and names nobody.
    pub fn has_details(&self) -> bool {
        self.exif.is_some()
            || self.xmp.is_some()
            || self.iptc.is_some()
            || self.comment.is_some()
            || !self.png_text.is_empty()
            || self.other
    }
}

const XMP_JPEG: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
const ICC_JPEG: &[u8] = b"ICC_PROFILE\0";
const EXIF_JPEG: &[u8] = b"Exif\0\0";
const IPTC_JPEG: &[u8] = b"Photoshop 3.0\0";

// ---------- JPEG ----------

/// Each segment before the picture data: (marker, payload range). Also where the picture data starts and where it ends.
pub struct JpegMap {
    pub segments: Vec<(u8, std::ops::Range<usize>)>,
    pub scan: usize,
    pub end: usize,
}

pub fn jpeg_map(b: &[u8]) -> Option<JpegMap> {
    if !b.starts_with(&[0xFF, 0xD8]) {
        return None;
    }
    let mut at = 2;
    let mut segments = Vec::new();
    loop {
        // padding before a marker is allowed
        while b.get(at) == Some(&0xFF) && b.get(at + 1) == Some(&0xFF) {
            at += 1;
        }
        if b.get(at) != Some(&0xFF) {
            return None;
        }
        let m = *b.get(at + 1)?;
        if m == 0xDA || (0xC0..=0xCF).contains(&m) && m != 0xC4 && m != 0xC8 && m != 0xCC {
            // from the frame header on, everything is needed to read the picture
            let end = jpeg_end(b, at);
            return Some(JpegMap { segments, scan: at, end });
        }
        if m == 0xD9 {
            return None;
        }
        let len = u16::from_be_bytes([*b.get(at + 2)?, *b.get(at + 3)?]) as usize;
        if len < 2 || at + 2 + len > b.len() {
            return None;
        }
        segments.push((m, at + 4..at + 2 + len));
        at += 2 + len;
    }
}

/// The byte after the end-of-image of the main picture. What follows is something else entirely.
fn jpeg_end(b: &[u8], from: usize) -> usize {
    let mut i = from;
    while i + 1 < b.len() {
        if b[i] != 0xFF {
            i += 1;
            continue;
        }
        let m = b[i + 1];
        if m == 0xD9 {
            return i + 2;
        }
        if m == 0 || m == 0xFF || (0xD0..=0xD7).contains(&m) {
            i += if m == 0xFF { 1 } else { 2 };
            continue;
        }
        // a segment, its length tells where it stops
        if i + 3 >= b.len() {
            break;
        }
        let len = u16::from_be_bytes([b[i + 2], b[i + 3]]) as usize;
        i += 2 + len;
    }
    b.len()
}

fn read_jpeg(b: &[u8], meta: &mut Meta) -> Option<()> {
    let map = jpeg_map(b)?;
    let mut icc: Vec<(u8, &[u8])> = Vec::new();
    for (m, r) in &map.segments {
        let p = &b[r.clone()];
        match m {
            0xE1 if p.starts_with(EXIF_JPEG) && meta.exif.is_none() => meta.exif = Some(p[6..].to_vec()),
            0xE1 if p.starts_with(XMP_JPEG) && meta.xmp.is_none() => meta.xmp = Some(p[XMP_JPEG.len()..].to_vec()),
            0xE2 if p.starts_with(ICC_JPEG) && p.len() > 14 => icc.push((p[12], &p[14..])),
            0xED if p.starts_with(IPTC_JPEG) => meta.iptc = Some(p.to_vec()),
            0xFE => meta.comment = Some(p.to_vec()),
            m if strippable_jpeg(*m, p) => meta.other = true,
            _ => {}
        }
    }
    if map.end < b.len() {
        meta.other = true;
    }
    if !icc.is_empty() {
        icc.sort_by_key(|i| i.0);
        meta.icc = Some(icc.iter().flat_map(|i| i.1.iter().copied()).collect());
    }
    Some(())
}

/// A JPEG segment's bytes, marker and length included.
fn jpeg_segment(marker: u8, payload: &[u8]) -> Vec<u8> {
    let mut v = vec![0xFF, marker];
    v.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
    v.extend_from_slice(payload);
    v
}

pub fn exif_payload(blob: &[u8]) -> Vec<u8> {
    let mut v = EXIF_JPEG.to_vec();
    v.extend_from_slice(blob);
    v
}

pub fn xmp_payload(xmp: &[u8]) -> Vec<u8> {
    let mut v = XMP_JPEG.to_vec();
    v.extend_from_slice(xmp);
    v
}

/// The same picture data with a new EXIF block. Nothing is recompressed: this is what makes turning a JPG lossless.
pub fn jpeg_with_orientation(b: &[u8], value: u8, strip: bool) -> Option<Vec<u8>> {
    let map = jpeg_map(b)?;
    let old = map.segments.iter().find(|(m, r)| *m == 0xE1 && b[r.clone()].starts_with(EXIF_JPEG));
    let blob = match (strip, old) {
        (false, Some((_, r))) => crate::exif::set_orientation(&b[r.start + 6..r.end], value)?,
        _ if value == 1 => Vec::new(),
        _ => crate::exif::orientation_only(value),
    };
    if blob.len() + 8 > u16::MAX as usize {
        return None;
    }
    let mut out = vec![0xFF, 0xD8];
    let segment = jpeg_segment(0xE1, &exif_payload(&blob));
    // JFIF stays first, as readers expect
    let mut placed = blob.is_empty();
    for (m, r) in &map.segments {
        let p = &b[r.clone()];
        if !placed && *m != 0xE0 {
            out.extend_from_slice(&segment);
            placed = true;
        }
        if (*m == 0xE1 && p.starts_with(EXIF_JPEG)) || (strip && strippable_jpeg(*m, p)) {
            continue;
        }
        out.extend_from_slice(&b[r.start - 4..r.end]);
    }
    if !placed {
        out.extend_from_slice(&segment);
    }
    // what follows the picture (a phone's motion clip, say) stays only if nothing is being removed
    out.extend_from_slice(if strip { &b[map.scan..map.end] } else { &b[map.scan..] });
    Some(out)
}

/// Details, not what's needed to read the picture: JFIF, Adobe's colour marker, tables and the colour profile stay.
fn strippable_jpeg(m: u8, p: &[u8]) -> bool {
    match m {
        0xFE | 0xE1 | 0xE3..=0xED | 0xEF => true,
        0xE2 => !p.starts_with(ICC_JPEG),
        _ => false,
    }
}

// ---------- PNG ----------

const PNG_SIGNATURE: &[u8] = &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];

/// (chunk type, data range) for each chunk.
pub fn png_chunks(b: &[u8]) -> Vec<([u8; 4], std::ops::Range<usize>)> {
    let mut out = Vec::new();
    let mut at = PNG_SIGNATURE.len();
    while at + 12 <= b.len() {
        let len = u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]) as usize;
        let ty = [b[at + 4], b[at + 5], b[at + 6], b[at + 7]];
        if at + 12 + len > b.len() {
            break;
        }
        out.push((ty, at + 8..at + 8 + len));
        at += 12 + len;
        if &ty == b"IEND" {
            break;
        }
    }
    out
}

fn inflate(data: &[u8], cap: u64) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    flate2::read::ZlibDecoder::new(data).take(cap).read_to_end(&mut out).ok()?;
    Some(out)
}

fn read_png(b: &[u8], meta: &mut Meta) {
    for (ty, r) in png_chunks(b) {
        let d = &b[r];
        match &ty {
            b"iCCP" => {
                if let Some(z) = d.iter().position(|c| *c == 0) {
                    meta.icc = d.get(z + 2..).and_then(|c| inflate(c, 16 << 20));
                }
            }
            b"eXIf" => meta.exif = Some(d.to_vec()),
            b"iTXt" if d.starts_with(b"XML:com.adobe.xmp\0") => {
                // keyword, then compression flag, method, language, translated keyword
                let rest = &d[18..];
                if rest.len() > 2 {
                    let flag = rest[0];
                    let mut i = 2;
                    for _ in 0..2 {
                        i += rest[i..].iter().position(|c| *c == 0).map(|p| p + 1).unwrap_or(rest.len() - i);
                    }
                    let text = &rest[i.min(rest.len())..];
                    meta.xmp = if flag == 0 { Some(text.to_vec()) } else { inflate(text, 16 << 20) };
                }
            }
            b"tEXt" | b"zTXt" | b"iTXt" => meta.png_text.push((ty, d.to_vec())),
            _ => {}
        }
    }
}

fn crc32(parts: &[&[u8]]) -> u32 {
    let mut h = flate2::Crc::new();
    for p in parts {
        h.update(p);
    }
    h.sum()
}

fn png_chunk(ty: &[u8; 4], data: &[u8]) -> Vec<u8> {
    let mut v = Vec::with_capacity(data.len() + 12);
    v.extend_from_slice(&(data.len() as u32).to_be_bytes());
    v.extend_from_slice(ty);
    v.extend_from_slice(data);
    v.extend_from_slice(&crc32(&[ty, data]).to_be_bytes());
    v
}

fn deflate(data: &[u8]) -> Vec<u8> {
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    let _ = z.write_all(data);
    z.finish().unwrap_or_default()
}

/// Puts what we carry into an encoded PNG, right after its header.
pub fn png_with(encoded: Vec<u8>, m: &Meta) -> Vec<u8> {
    let mut extra = Vec::new();
    if let Some(icc) = &m.icc {
        let mut d = b"ICC profile\0\0".to_vec();
        d.extend_from_slice(&deflate(icc));
        extra.extend_from_slice(&png_chunk(b"iCCP", &d));
    }
    if let Some(e) = &m.exif {
        extra.extend_from_slice(&png_chunk(b"eXIf", e));
    }
    if let Some(x) = &m.xmp {
        let mut d = b"XML:com.adobe.xmp\0\0\0\0\0".to_vec();
        d.extend_from_slice(x);
        extra.extend_from_slice(&png_chunk(b"iTXt", &d));
    }
    for (ty, d) in &m.png_text {
        extra.extend_from_slice(&png_chunk(ty, d));
    }
    if extra.is_empty() {
        return encoded;
    }
    // signature + IHDR: 8 + 4 + 4 + 13 + 4
    const AFTER_HEADER: usize = 33;
    let mut out = Vec::with_capacity(encoded.len() + extra.len());
    out.extend_from_slice(&encoded[..AFTER_HEADER]);
    out.extend_from_slice(&extra);
    out.extend_from_slice(&encoded[AFTER_HEADER..]);
    out
}

// ---------- WebP ----------

/// (four-letter name, data range). The data range doesn't include the pad byte.
pub fn webp_chunks(b: &[u8]) -> Vec<([u8; 4], std::ops::Range<usize>)> {
    let mut out = Vec::new();
    let mut at = 12;
    while at + 8 <= b.len() {
        let ty = [b[at], b[at + 1], b[at + 2], b[at + 3]];
        let len = u32::from_le_bytes([b[at + 4], b[at + 5], b[at + 6], b[at + 7]]) as usize;
        if at + 8 + len > b.len() {
            break;
        }
        out.push((ty, at + 8..at + 8 + len));
        at += 8 + len + (len & 1);
    }
    out
}

fn read_webp(b: &[u8], meta: &mut Meta) {
    for (ty, r) in webp_chunks(b) {
        let d = &b[r];
        match &ty {
            b"ICCP" => meta.icc = Some(d.to_vec()),
            b"EXIF" => meta.exif = Some(d.strip_prefix(EXIF_JPEG).unwrap_or(d).to_vec()),
            b"XMP " => meta.xmp = Some(d.to_vec()),
            _ => {}
        }
    }
}

fn riff_chunk(out: &mut Vec<u8>, ty: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(ty);
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());
    out.extend_from_slice(data);
    if data.len() % 2 == 1 {
        out.push(0);
    }
}

/// Rebuilds an encoded WebP so it carries what we want and nothing else. `w`, `h` and `alpha` describe the canvas.
pub fn webp_with(encoded: &[u8], m: &Meta, w: u32, h: u32, alpha: bool) -> Vec<u8> {
    let chunks = webp_chunks(encoded);
    let animated = chunks.iter().any(|(t, _)| t == b"ANIM");
    let body = |t: &[u8; 4]| !matches!(t, b"VP8X" | b"ICCP" | b"EXIF" | b"XMP ");
    if m.icc.is_none() && m.exif.is_none() && m.xmp.is_none() && !animated && chunks.iter().all(|(t, _)| body(t)) {
        return encoded.to_vec();
    }
    let mut flags = 0u8;
    if m.icc.is_some() {
        flags |= 0x20;
    }
    if alpha {
        flags |= 0x10;
    }
    if m.exif.is_some() {
        flags |= 0x08;
    }
    if m.xmp.is_some() {
        flags |= 0x04;
    }
    if animated {
        flags |= 0x02;
    }
    let mut x = vec![flags, 0, 0, 0];
    x.extend_from_slice(&(w - 1).to_le_bytes()[..3]);
    x.extend_from_slice(&(h - 1).to_le_bytes()[..3]);
    let mut inner = b"WEBP".to_vec();
    riff_chunk(&mut inner, b"VP8X", &x);
    if let Some(i) = &m.icc {
        riff_chunk(&mut inner, b"ICCP", i);
    }
    for (t, r) in chunks.iter().filter(|(t, _)| body(t)) {
        riff_chunk(&mut inner, t, &encoded[r.clone()]);
    }
    if let Some(e) = &m.exif {
        riff_chunk(&mut inner, b"EXIF", e);
    }
    if let Some(e) = &m.xmp {
        riff_chunk(&mut inner, b"XMP ", e);
    }
    let mut out = b"RIFF".to_vec();
    out.extend_from_slice(&(inner.len() as u32).to_le_bytes());
    out.extend_from_slice(&inner);
    out
}

/// From the VP8X, VP8 or VP8L header.
fn webp_canvas(b: &[u8]) -> Option<(u32, u32, bool)> {
    let chunks = webp_chunks(b);
    let (ty, r) = chunks.first()?;
    let d = &b[r.clone()];
    match ty {
        b"VP8X" if d.len() >= 10 => {
            let w = 1 + (d[4] as u32 | (d[5] as u32) << 8 | (d[6] as u32) << 16);
            let h = 1 + (d[7] as u32 | (d[8] as u32) << 8 | (d[9] as u32) << 16);
            Some((w, h, d[0] & 0x10 != 0))
        }
        b"VP8 " => {
            let w = u16::from_le_bytes([*d.get(6)?, *d.get(7)?]) as u32 & 0x3FFF;
            let h = u16::from_le_bytes([*d.get(8)?, *d.get(9)?]) as u32 & 0x3FFF;
            Some((w, h, false))
        }
        b"VP8L" => {
            let v = u32::from_le_bytes([*d.get(1)?, *d.get(2)?, *d.get(3)?, *d.get(4)?]);
            Some(((v & 0x3FFF) + 1, ((v >> 14) & 0x3FFF) + 1, v & (1 << 28) != 0))
        }
        _ => None,
    }
}

// ---------- TIFF ----------

fn read_tiff(b: &[u8], meta: &mut Meta) {
    for (tag, v) in crate::exif::first_ifd_raw(b, &[34675, 700, 33723]) {
        match tag {
            34675 => meta.icc = Some(v),
            700 => meta.xmp = Some(v),
            33723 => meta.iptc = Some(v),
            _ => {}
        }
    }
    if let Some(tree) = crate::exif::tree(b) {
        if !tree.is_empty() {
            meta.exif = Some(crate::exif::block(&tree));
        }
    }
}

// ---------- GIF ----------

/// Walks a GIF's blocks; calls `f` with (block start, block end, is it a comment or an application block other than looping).
fn gif_blocks(b: &[u8], mut f: impl FnMut(usize, usize, bool)) -> Option<usize> {
    let flags = *b.get(10)?;
    let mut at = 13 + if flags & 0x80 != 0 { 3usize << ((flags & 7) + 1) } else { 0 };
    let subs = |mut i: usize| -> Option<usize> {
        loop {
            let n = *b.get(i)? as usize;
            i += 1 + n;
            if n == 0 {
                return Some(i);
            }
        }
    };
    loop {
        match *b.get(at)? {
            0x3B => return Some(at + 1),
            0x21 => {
                let label = *b.get(at + 1)?;
                let end = subs(at + 2)?;
                let name = b.get(at + 3..at + 14);
                let detail = match label {
                    0xFE => true,
                    0xFF => name != Some(b"NETSCAPE2.0".as_ref()) && name != Some(b"ANIMEXTS1.0".as_ref()),
                    _ => false,
                };
                f(at, end, detail);
                at = end;
            }
            0x2C => {
                let fl = *b.get(at + 9)?;
                let table = if fl & 0x80 != 0 { 3usize << ((fl & 7) + 1) } else { 0 };
                let end = subs(at + 10 + table + 1)?;
                f(at, end, false);
                at = end;
            }
            _ => return None,
        }
    }
}

// ---------- the same for every kind ----------

pub fn read(kind: Kind, b: &[u8]) -> Meta {
    let mut m = Meta::default();
    match kind {
        Kind::Jpg => {
            let _ = read_jpeg(b, &mut m);
        }
        Kind::Png => read_png(b, &mut m),
        Kind::Webp => read_webp(b, &mut m),
        Kind::Tiff => read_tiff(b, &mut m),
        // the formats only read carry no details we keep
        _ => {}
    }
    m
}

/// The same file without its details, pixels untouched. None where that takes re-encoding (TIFF, and kinds that carry none).
pub fn strip_in_place(kind: Kind, b: &[u8]) -> Option<Vec<u8>> {
    match kind {
        Kind::Jpg => {
            let keep = read(Kind::Jpg, b).exif.map(|e| crate::exif::orientation(&e)).unwrap_or(1);
            jpeg_with_orientation(b, keep, true)
        }
        Kind::Png => {
            let mut out = PNG_SIGNATURE.to_vec();
            for (ty, r) in png_chunks(b) {
                if !matches!(&ty, b"eXIf" | b"tEXt" | b"zTXt" | b"iTXt" | b"tIME") {
                    out.extend_from_slice(&png_chunk(&ty, &b[r]));
                }
            }
            Some(out)
        }
        Kind::Webp => {
            let keep = Meta { icc: read(Kind::Webp, b).icc, ..Default::default() };
            let (w, h, alpha) = webp_canvas(b)?;
            let mut plain = b"RIFF\0\0\0\0WEBP".to_vec();
            for (t, r) in webp_chunks(b).iter().filter(|(t, _)| !matches!(t, b"EXIF" | b"XMP ")) {
                riff_chunk(&mut plain, t, &b[r.clone()]);
            }
            let n = (plain.len() - 8) as u32;
            plain[4..8].copy_from_slice(&n.to_le_bytes());
            Some(webp_with(&plain, &keep, w, h, alpha))
        }
        Kind::Gif => {
            let mut drop = Vec::new();
            gif_blocks(b, |a, z, detail| {
                if detail {
                    drop.push(a..z);
                }
            })?;
            let mut out = Vec::with_capacity(b.len());
            let mut at = 0;
            for r in drop {
                out.extend_from_slice(&b[at..r.start]);
                at = r.end;
            }
            out.extend_from_slice(&b[at..]);
            Some(out)
        }
        _ => None,
    }
}

/// What details remain in a finished file. The save stops if removal was asked for and this isn't empty.
/// A JPEG may keep a block that says only which way up it is.
pub fn details_left(kind: Kind, b: &[u8]) -> Vec<String> {
    let mut found = Vec::new();
    match kind {
        Kind::Jpg => {
            if let Some(map) = jpeg_map(b) {
                for (m, r) in &map.segments {
                    let p = &b[r.clone()];
                    if *m == 0xE1 && p.starts_with(EXIF_JPEG) {
                        if p[6..] != crate::exif::orientation_only(crate::exif::orientation(&p[6..]))[..] {
                            found.push("EXIF".into());
                        }
                    } else if strippable_jpeg(*m, p) {
                        found.push(format!("segment {m:02X}"));
                    }
                }
                if map.end < b.len() {
                    found.push("data after the picture".into());
                }
            }
        }
        Kind::Png => {
            for (t, _) in png_chunks(b) {
                if matches!(&t, b"eXIf" | b"tEXt" | b"zTXt" | b"iTXt" | b"tIME") {
                    found.push(String::from_utf8_lossy(&t).into_owned());
                }
            }
        }
        Kind::Webp => {
            for (t, _) in webp_chunks(b) {
                if matches!(&t, b"EXIF" | b"XMP ") {
                    found.push(String::from_utf8_lossy(&t).into_owned());
                }
            }
        }
        Kind::Tiff => {
            if crate::exif::first_ifd_has(
                b,
                &[0x010E, 0x010F, 0x0110, 0x0131, 0x0132, 0x013B, 0x8298, 700, 33723, 0x8769, 0x8825],
            ) {
                found.push("TIFF tags".into());
            }
        }
        Kind::Gif => {
            gif_blocks(b, |_, _, detail| {
                if detail {
                    found.push("GIF comment or application block".into());
                }
            });
        }
        _ => {}
    }
    found
}
