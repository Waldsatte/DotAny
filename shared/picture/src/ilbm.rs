//! IFF ILBM, the Amiga's picture (Deluxe Paint and friends): read only, by the reader in this file.
//! An IFF FORM of chunks: BMHD (size, planes, mask, compression), CMAP (the palette), CAMG (the Amiga display mode) and
//! BODY (the lines, each bit plane in turn, ByteRun1 coded or plain). What's read: 1 to 8 planes with the palette, Extra
//! Half-Brite, Hold-And-Modify (HAM6 and HAM8), 24 and 32 planes of true colour, a mask plane or a transparent colour,
//! and Deluxe Paint's chunky "PBM " variant (one byte a pixel). Pixels are shown as stored: the Amiga's non-square
//! pixel shape (BMHD's aspect) isn't applied.

pub use crate::pcx::Problem;

pub struct Ilbm {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

/// CAMG's mode bits that change how planes become colours.
const HAM: u32 = 0x800;
const EHB: u32 = 0x80;

fn be16(b: &[u8], at: usize) -> u16 {
    u16::from_be_bytes([b[at], b[at + 1]])
}
fn be32(b: &[u8], at: usize) -> u32 {
    u32::from_be_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

/// ByteRun1 (PackBits): n in 0..=127 copies the next n+1 bytes, -127..=-1 repeats the next byte 1-n times, -128 is nothing.
/// Unpacks until `want` bytes are out, from `at` on; answers where it stopped. A body cut short leaves the rest zero.
fn byterun1(b: &[u8], mut at: usize, want: usize, out: &mut Vec<u8>) -> usize {
    let stop = out.len() + want;
    while out.len() < stop {
        let Some(&n) = b.get(at) else { break };
        at += 1;
        let n = n as i8;
        if n >= 0 {
            let take = (n as usize + 1).min(stop - out.len()).min(b.len().saturating_sub(at));
            out.extend_from_slice(&b[at..at + take]);
            at += n as usize + 1;
        } else if n != -128 {
            let Some(&v) = b.get(at) else { break };
            at += 1;
            let k = ((1 - n as i32) as usize).min(stop - out.len());
            out.extend(std::iter::repeat(v).take(k));
        }
    }
    out.resize(stop, 0);
    at
}

/// `fits(w, h)`: whether a picture this size may be made at all (memory).
pub fn decode(b: &[u8], fits: &dyn Fn(u32, u32) -> bool) -> Result<Ilbm, Problem> {
    if b.len() < 12 || &b[0..4] != b"FORM" {
        return Err(Problem::Damaged);
    }
    let chunky = match &b[8..12] {
        b"ILBM" => false,
        b"PBM " => true,
        _ => return Err(Problem::Damaged),
    };
    let end = (be32(b, 4) as usize).saturating_add(8).min(b.len());
    let (mut bmhd, mut cmap, mut camg, mut body) = (None, None, 0u32, None);
    let mut at = 12;
    while at + 8 <= end {
        let id = &b[at..at + 4];
        let len = be32(b, at + 4) as usize;
        let data = at + 8;
        let stop = data.saturating_add(len).min(end);
        match id {
            b"BMHD" if stop - data >= 20 => bmhd = Some(data),
            b"CMAP" => cmap = Some(&b[data..stop]),
            b"CAMG" if stop - data >= 4 => camg = be32(b, data),
            // the lines run to the end of what's there, even when the length says more
            b"BODY" => body = Some((data, stop)),
            _ => {}
        }
        // chunks are padded to an even length
        at = data.saturating_add(len).saturating_add(len & 1);
    }
    let (Some(h0), Some((body_at, body_end))) = (bmhd, body) else { return Err(Problem::Damaged) };
    let (w, h) = (be16(b, h0) as u32, be16(b, h0 + 2) as u32);
    let (planes, masking, compression) = (b[h0 + 8] as usize, b[h0 + 9], b[h0 + 10]);
    let transparent = be16(b, h0 + 12) as usize;
    if w == 0
        || h == 0
        || compression > 1
        || !(chunky && planes == 8 || !chunky && (1..=8).contains(&planes) || !chunky && matches!(planes, 24 | 32))
    {
        return Err(Problem::Damaged);
    }
    if !fits(w, h) {
        return Err(Problem::TooLarge);
    }
    let (wu, hu) = (w as usize, h as usize);
    let body = &b[..body_end];

    // the palette, scaled up when it was written for the old 4-bit chips (every low nibble zero)
    let mut pal: Vec<[u8; 3]> =
        cmap.map(|c| c.chunks_exact(3).map(|p| [p[0], p[1], p[2]]).collect()).unwrap_or_default();
    if !pal.is_empty() && pal.iter().all(|p| p.iter().all(|c| c & 0x0F == 0)) {
        for p in pal.iter_mut() {
            for c in p.iter_mut() {
                *c |= *c >> 4;
            }
        }
    }
    let ham = !chunky && camg & HAM != 0 && matches!(planes, 6 | 8);
    if !chunky && camg & EHB != 0 && planes == 6 && !ham {
        // Extra Half-Brite: 32 more colours, each the first 32 at half brightness
        pal.resize(32, [0, 0, 0]);
        for i in 0..32 {
            let [r, g, bl] = pal[i];
            pal.push([r >> 1, g >> 1, bl >> 1]);
        }
    }
    if pal.is_empty() && planes <= 8 {
        // no palette: a grey ramp, as other readers show it
        let n = 1usize << planes;
        pal = (0..n).map(|i| [(i * 255 / (n - 1).max(1)) as u8; 3]).collect();
    }
    let colour = |i: usize| -> [u8; 3] { pal.get(i).copied().unwrap_or([0, 0, 0]) };

    // one line: every plane (and the mask plane), each a whole number of 16-bit words
    let plane_bytes = wu.div_ceil(16) * 2;
    let with_mask = masking == 1 && !chunky;
    let line_bytes = if chunky { wu + (wu & 1) } else { plane_bytes * (planes + with_mask as usize) };
    let mut line = Vec::with_capacity(line_bytes);
    let mut at = body_at;
    let mut out = vec![255u8; wu * hu * 4];
    let mut index = vec![0usize; wu];
    for y in 0..hu {
        line.clear();
        if compression == 1 {
            at = byterun1(body, at, line_bytes, &mut line);
        } else {
            let take = line_bytes.min(body.len().saturating_sub(at));
            line.extend_from_slice(&body[at..at + take]);
            line.resize(line_bytes, 0);
            at += line_bytes;
        }
        let px = &mut out[y * wu * 4..(y + 1) * wu * 4];
        let bit = |plane: usize, x: usize| -> usize { (line[plane * plane_bytes + x / 8] >> (7 - x % 8) & 1) as usize };
        if chunky {
            for x in 0..wu {
                index[x] = line[x] as usize;
            }
        } else if planes >= 24 {
            for x in 0..wu {
                for c in 0..planes / 8 {
                    let mut v = 0usize;
                    for k in 0..8 {
                        v |= bit(c * 8 + k, x) << k;
                    }
                    px[x * 4 + c] = v as u8;
                }
            }
        } else {
            for (x, i) in index.iter_mut().enumerate() {
                *i = (0..planes).fold(0, |v, k| v | bit(k, x) << k);
            }
        }
        if planes < 24 || chunky {
            if ham {
                // Hold-And-Modify: the top two bits say keep the colour on the left and change one of its parts, or
                // take a palette colour; every line starts from colour 0
                let low = planes - 2;
                let mut last = colour(0);
                for x in 0..wu {
                    let (how, v) = (index[x] >> low, index[x] & ((1 << low) - 1));
                    let v8 = if low == 4 { (v * 17) as u8 } else { (v << 2 | v >> 4) as u8 };
                    match how {
                        0 => last = colour(v),
                        1 => last[2] = v8,
                        2 => last[0] = v8,
                        _ => last[1] = v8,
                    }
                    px[x * 4..x * 4 + 3].copy_from_slice(&last);
                }
            } else {
                for x in 0..wu {
                    px[x * 4..x * 4 + 3].copy_from_slice(&colour(index[x]));
                }
            }
        }
        if with_mask {
            for x in 0..wu {
                px[x * 4 + 3] = if bit(planes, x) == 1 { 255 } else { 0 };
            }
        } else if masking == 2 && planes < 24 {
            for x in 0..wu {
                if index[x] == transparent {
                    px[x * 4 + 3] = 0;
                }
            }
        }
    }
    Ok(Ilbm { w, h, rgba: out })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all(_: u32, _: u32) -> bool {
        true
    }
    fn chunk(id: &[u8], data: &[u8]) -> Vec<u8> {
        let mut c = id.to_vec();
        c.extend_from_slice(&(data.len() as u32).to_be_bytes());
        c.extend_from_slice(data);
        if data.len() & 1 == 1 {
            c.push(0);
        }
        c
    }
    fn bmhd(w: u16, h: u16, planes: u8, masking: u8, compression: u8, transparent: u16) -> Vec<u8> {
        let mut d = Vec::new();
        d.extend_from_slice(&w.to_be_bytes());
        d.extend_from_slice(&h.to_be_bytes());
        d.extend_from_slice(&[0, 0, 0, 0, planes, masking, compression, 0]);
        d.extend_from_slice(&transparent.to_be_bytes());
        d.extend_from_slice(&[10, 11]);
        d.extend_from_slice(&w.to_be_bytes());
        d.extend_from_slice(&h.to_be_bytes());
        chunk(b"BMHD", &d)
    }
    fn form(kind: &[u8], chunks: &[Vec<u8>]) -> Vec<u8> {
        let inner: Vec<u8> = chunks.concat();
        let mut f = b"FORM".to_vec();
        f.extend_from_slice(&((inner.len() + 4) as u32).to_be_bytes());
        f.extend_from_slice(kind);
        f.extend(inner);
        f
    }
    /// Pixel indices into bit-plane lines (no compression): each plane a whole number of 16-bit words.
    fn planar(idx: &[Vec<usize>], planes: usize) -> Vec<u8> {
        let w = idx[0].len();
        let pb = w.div_ceil(16) * 2;
        let mut out = Vec::new();
        for row in idx {
            for k in 0..planes {
                let mut p = vec![0u8; pb];
                for (x, &i) in row.iter().enumerate() {
                    if i >> k & 1 == 1 {
                        p[x / 8] |= 0x80 >> (x % 8);
                    }
                }
                out.extend(p);
            }
        }
        out
    }
    /// The ByteRun1 a writer would make: runs of three or more, literals otherwise.
    fn pack(data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < data.len() {
            let mut n = 1;
            while i + n < data.len() && data[i + n] == data[i] && n < 128 {
                n += 1;
            }
            if n >= 3 {
                out.push((1 - n as i32) as i8 as u8);
                out.push(data[i]);
                i += n;
            } else {
                let start = i;
                while i < data.len()
                    && i - start < 128
                    && !(i + 2 < data.len() && data[i] == data[i + 1] && data[i] == data[i + 2])
                {
                    i += 1;
                }
                out.push((i - start - 1) as u8);
                out.extend_from_slice(&data[start..i]);
            }
        }
        out
    }
    /// ByteRun1 as a writer makes it: each line packed on its own (a run never crosses into the next line).
    fn pack_lines(data: &[u8], line_bytes: usize) -> Vec<u8> {
        data.chunks(line_bytes).flat_map(pack).collect()
    }
    fn px(p: &Ilbm, x: u32, y: u32) -> [u8; 4] {
        let at = ((y * p.w + x) * 4) as usize;
        p.rgba[at..at + 4].try_into().unwrap()
    }

    #[test]
    fn byterun1_both_ways() {
        let data = [1u8, 2, 3, 3, 3, 3, 9, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 7];
        let mut out = Vec::new();
        let end = byterun1(&pack(&data), 0, data.len(), &mut out);
        assert_eq!(out, data);
        assert_eq!(end, pack(&data).len());
        // -128 is nothing
        let mut out = Vec::new();
        byterun1(&[0x80, 0xFE, 5], 0, 3, &mut out);
        assert_eq!(out, [5, 5, 5]);
    }

    #[test]
    fn palette_pictures_packed_and_plain() {
        let idx = vec![(0..20).map(|x| x % 4).collect::<Vec<_>>(), (0..20).map(|x| 3 - x % 4).collect()];
        let cmap = chunk(b"CMAP", &[0, 0, 0, 255, 0, 0, 0, 255, 0, 0, 0, 255]);
        for compression in [0u8, 1] {
            let body = planar(&idx, 2);
            let body = if compression == 1 { pack_lines(&body, 8) } else { body };
            let f = form(b"ILBM", &[bmhd(20, 2, 2, 0, compression, 0), cmap.clone(), chunk(b"BODY", &body)]);
            let p = decode(&f, &all).unwrap();
            assert_eq!((p.w, p.h), (20, 2));
            assert_eq!(px(&p, 1, 0), [255, 0, 0, 255]);
            assert_eq!(px(&p, 19, 0), [0, 0, 255, 255]);
            assert_eq!(px(&p, 0, 1), [0, 0, 255, 255]);
        }
    }

    #[test]
    fn four_bit_palettes_reach_white() {
        let f = form(
            b"ILBM",
            &[
                bmhd(2, 1, 1, 0, 0, 0),
                chunk(b"CMAP", &[0, 0, 0, 0xF0, 0xF0, 0xF0]),
                chunk(b"BODY", &planar(&[vec![0, 1]], 1)),
            ],
        );
        assert_eq!(px(&decode(&f, &all).unwrap(), 1, 0), [255, 255, 255, 255]);
    }

    #[test]
    fn extra_half_brite() {
        let mut cmap = vec![0u8; 96];
        cmap[3..6].copy_from_slice(&[200, 100, 50]);
        let f = form(
            b"ILBM",
            &[
                bmhd(2, 1, 6, 0, 0, 0),
                chunk(b"CMAP", &cmap),
                chunk(b"CAMG", &EHB.to_be_bytes()),
                chunk(b"BODY", &planar(&[vec![1, 33]], 6)),
            ],
        );
        let p = decode(&f, &all).unwrap();
        assert_eq!(px(&p, 0, 0), [200, 100, 50, 255]);
        assert_eq!(px(&p, 1, 0), [100, 50, 25, 255]);
    }

    #[test]
    fn hold_and_modify_six_and_eight() {
        // HAM6: palette 1, then set red, green, blue in turn; the next line starts again from colour 0
        let mut cmap = vec![0u8; 48];
        // (0x31: not every low nibble is zero, so the palette is taken as it is)
        cmap[0..3].copy_from_slice(&[0x10, 0x20, 0x31]);
        cmap[3..6].copy_from_slice(&[0x40, 0x50, 0x60]);
        let row = vec![0b00_0001, 0b10_1111, 0b11_0000, 0b01_1000];
        let f = form(
            b"ILBM",
            &[
                bmhd(4, 2, 6, 0, 1, 0),
                chunk(b"CMAP", &cmap),
                chunk(b"CAMG", &HAM.to_be_bytes()),
                chunk(b"BODY", &pack_lines(&planar(&[row, vec![0b01_0011; 4]], 6), 12)),
            ],
        );
        let p = decode(&f, &all).unwrap();
        assert_eq!(px(&p, 0, 0), [0x40, 0x50, 0x60, 255]);
        assert_eq!(px(&p, 1, 0), [255, 0x50, 0x60, 255]);
        assert_eq!(px(&p, 2, 0), [255, 0, 0x60, 255]);
        assert_eq!(px(&p, 3, 0), [255, 0, 0x88, 255]);
        assert_eq!(px(&p, 0, 1), [0x10, 0x20, 0x33, 255]);
        // HAM8: six bits of value
        let mut cmap = vec![0u8; 64 * 3];
        cmap[3..6].copy_from_slice(&[1, 2, 3]);
        let f = form(
            b"ILBM",
            &[
                bmhd(2, 1, 8, 0, 0, 0),
                chunk(b"CMAP", &cmap),
                chunk(b"CAMG", &HAM.to_be_bytes()),
                chunk(b"BODY", &planar(&[vec![1, 0b10_111111]], 8)),
            ],
        );
        let p = decode(&f, &all).unwrap();
        assert_eq!(px(&p, 0, 0), [1, 2, 3, 255]);
        assert_eq!(px(&p, 1, 0), [255, 2, 3, 255]);
    }

    #[test]
    fn masks_and_transparent_colour() {
        // a mask plane after the colour planes
        let mut body = Vec::new();
        for (k, plane) in planar(&[vec![1, 1, 0, 1]], 1).chunks(2).enumerate() {
            let _ = k;
            body.extend_from_slice(plane);
        }
        body.extend_from_slice(&[0b1010_0000, 0]);
        let f = form(b"ILBM", &[bmhd(4, 1, 1, 1, 0, 0), chunk(b"CMAP", &[0, 0, 0, 9, 9, 9]), chunk(b"BODY", &body)]);
        let p = decode(&f, &all).unwrap();
        assert_eq!([px(&p, 0, 0)[3], px(&p, 1, 0)[3], px(&p, 2, 0)[3], px(&p, 3, 0)[3]], [255, 0, 255, 0]);
        assert_eq!(px(&p, 0, 0), [9, 9, 9, 255]);
        // colour 2 is see-through
        let f = form(
            b"ILBM",
            &[bmhd(3, 1, 2, 2, 0, 2), chunk(b"CMAP", &[0; 12]), chunk(b"BODY", &planar(&[vec![0, 2, 3]], 2))],
        );
        let p = decode(&f, &all).unwrap();
        assert_eq!([px(&p, 0, 0)[3], px(&p, 1, 0)[3], px(&p, 2, 0)[3]], [255, 0, 255]);
    }

    #[test]
    fn true_colour_and_chunky() {
        // 24 planes: red's 8, green's 8, blue's 8, lowest bit first
        let mut body = Vec::new();
        for v in [0xA5u8, 0x3C, 0xFF] {
            for k in 0..8 {
                body.extend_from_slice(&[if v >> k & 1 == 1 { 0x80 } else { 0 }, 0]);
            }
        }
        let f = form(b"ILBM", &[bmhd(1, 1, 24, 0, 0, 0), chunk(b"BODY", &body)]);
        assert_eq!(px(&decode(&f, &all).unwrap(), 0, 0), [0xA5, 0x3C, 0xFF, 255]);
        // Deluxe Paint's PBM: a byte a pixel, lines padded to even
        let f = form(
            b"PBM ",
            &[
                bmhd(3, 2, 8, 0, 1, 0),
                chunk(b"CMAP", &[0, 0, 0, 1, 1, 1, 2, 2, 2]),
                chunk(b"BODY", &pack_lines(&[0, 1, 2, 0, 2, 2, 1, 0], 4)),
            ],
        );
        let p = decode(&f, &all).unwrap();
        assert_eq!([px(&p, 1, 0)[0], px(&p, 2, 0)[0], px(&p, 0, 1)[0], px(&p, 2, 1)[0]], [1, 2, 2, 1]);
    }

    #[test]
    fn nonsense_is_refused() {
        assert_eq!(decode(b"FORM\0\0\0\x04ILBM", &all).err(), Some(Problem::Damaged));
        assert_eq!(decode(b"FORM\0\0\0\x048SVX", &all).err(), Some(Problem::Damaged));
        let f = form(b"ILBM", &[bmhd(4, 4, 12, 0, 0, 0), chunk(b"BODY", &[0; 64])]);
        assert_eq!(decode(&f, &all).err(), Some(Problem::Damaged));
        let f = form(b"ILBM", &[bmhd(4, 4, 2, 0, 0, 0), chunk(b"BODY", &[0; 4])]);
        assert_eq!(decode(&f, &|_, _| false).err(), Some(Problem::TooLarge));
        // a body cut short: what's there, then black, never a crash
        let f = form(b"ILBM", &[bmhd(64, 64, 4, 0, 1, 0), chunk(b"BODY", &[0x05, 1, 2])]);
        assert!(decode(&f, &all).is_ok());
    }
}
