//! ZSoft PCX, the DOS paint program's picture: read only, by the reader in this file. A 128-byte header,
//! then every line run-length coded, plane after plane. What's read: black and white, 4 and 16 colours (packed or in
//! planes, with the header's palette), 256 colours (with the palette at the end of the file, or grey without one), and
//! 24-bit colour (with a fourth plane read as transparency).

/// Why a file couldn't be read.
#[derive(Debug, PartialEq)]
pub enum Problem {
    Damaged,
    TooLarge,
}

pub struct Pcx {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

/// The 16-colour EGA palette: what a 16-colour file whose header carries no palette (version 3) was drawn with.
const EGA: [[u8; 3]; 16] = [
    [0, 0, 0],
    [0, 0, 170],
    [0, 170, 0],
    [0, 170, 170],
    [170, 0, 0],
    [170, 0, 170],
    [170, 85, 0],
    [170, 170, 170],
    [85, 85, 85],
    [85, 85, 255],
    [85, 255, 85],
    [85, 255, 255],
    [255, 85, 85],
    [255, 85, 255],
    [255, 255, 85],
    [255, 255, 255],
];

/// `fits(w, h)`: whether a picture this size may be made at all (memory).
pub fn decode(b: &[u8], fits: &dyn Fn(u32, u32) -> bool) -> Result<Pcx, Problem> {
    if b.len() < 128 || b[0] != 0x0A {
        return Err(Problem::Damaged);
    }
    let version = b[1];
    let rle = b[2] == 1;
    let bits = b[3] as usize;
    let le = |at: usize| u16::from_le_bytes([b[at], b[at + 1]]) as u32;
    let (x0, y0, x1, y1) = (le(4), le(6), le(8), le(10));
    if x1 < x0 || y1 < y0 {
        return Err(Problem::Damaged);
    }
    let (w, h) = (x1 - x0 + 1, y1 - y0 + 1);
    let planes = b[65] as usize;
    let line = le(66) as usize;
    if !matches!((bits, planes), (1, 1..=4) | (2, 1) | (4, 1) | (8, 1) | (8, 3) | (8, 4))
        || line * 8 < w as usize * bits
        || line == 0
    {
        return Err(Problem::Damaged);
    }
    if !fits(w, h) {
        return Err(Problem::TooLarge);
    }
    // the lines, every plane of each, as they unpack: a run may carry over from one line to the next
    let total = line * planes * h as usize;
    let mut raw = Vec::with_capacity(total);
    let mut at = 128;
    while raw.len() < total {
        let Some(&c) = b.get(at) else { break };
        at += 1;
        if rle && c >= 0xC0 {
            let Some(&v) = b.get(at) else { break };
            at += 1;
            let n = ((c & 0x3F) as usize).min(total - raw.len());
            raw.extend(std::iter::repeat(v).take(n));
        } else {
            raw.push(c);
        }
    }
    // a file cut short keeps what it had, the rest is black, as other readers show it; nothing at all is damage
    if raw.is_empty() {
        return Err(Problem::Damaged);
    }
    raw.resize(total, 0);

    let header_palette = |i: usize| -> [u8; 3] {
        if version == 3 {
            return EGA[i & 15];
        }
        let p = 16 + (i & 15) * 3;
        [b[p], b[p + 1], b[p + 2]]
    };
    // 256 colours: 0x0C and 768 bytes at the very end
    let end_palette =
        (bits == 8 && planes == 1 && b.len() >= 128 + 769 && b[b.len() - 769] == 0x0C).then(|| &b[b.len() - 768..]);

    let (wu, hu) = (w as usize, h as usize);
    let mut out = vec![255u8; wu * hu * 4];
    for y in 0..hu {
        let row = &raw[y * line * planes..(y + 1) * line * planes];
        let px = &mut out[y * wu * 4..(y + 1) * wu * 4];
        for x in 0..wu {
            let o = &mut px[x * 4..x * 4 + 4];
            match (bits, planes) {
                (8, 3) | (8, 4) => {
                    o[0] = row[x];
                    o[1] = row[line + x];
                    o[2] = row[2 * line + x];
                    if planes == 4 {
                        o[3] = row[3 * line + x];
                    }
                }
                (8, 1) => {
                    let i = row[x] as usize;
                    match end_palette {
                        Some(p) => o[..3].copy_from_slice(&p[i * 3..i * 3 + 3]),
                        None => o[..3].fill(i as u8),
                    }
                }
                (1, 1) => {
                    let on = row[x / 8] >> (7 - x % 8) & 1;
                    o[..3].fill(if on == 1 { 255 } else { 0 });
                }
                (1, n) => {
                    // EGA planes: bit k of the colour is in plane k
                    let mut i = 0usize;
                    for k in 0..n {
                        i |= ((row[k * line + x / 8] >> (7 - x % 8) & 1) as usize) << k;
                    }
                    o[..3].copy_from_slice(&header_palette(i));
                }
                (2, 1) => {
                    let i = (row[x / 4] >> (6 - 2 * (x % 4)) & 3) as usize;
                    o[..3].copy_from_slice(&header_palette(i));
                }
                (4, 1) => {
                    let i = (row[x / 2] >> (4 - 4 * (x % 2)) & 15) as usize;
                    o[..3].copy_from_slice(&header_palette(i));
                }
                _ => return Err(Problem::Damaged),
            }
        }
    }
    Ok(Pcx { w, h, rgba: out })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header(bits: u8, planes: u8, w: u16, h: u16, line: u16, version: u8) -> Vec<u8> {
        let mut b = vec![0u8; 128];
        b[0] = 0x0A;
        b[1] = version;
        b[2] = 1;
        b[3] = bits;
        b[8..10].copy_from_slice(&(w - 1).to_le_bytes());
        b[10..12].copy_from_slice(&(h - 1).to_le_bytes());
        b[65] = planes;
        b[66..68].copy_from_slice(&line.to_le_bytes());
        b
    }
    /// The run-length coding every PCX writer uses: runs of up to 63, and a lone byte of 0xC0 or more as a run of one.
    fn rle(data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < data.len() {
            let v = data[i];
            let mut n = 1;
            while i + n < data.len() && data[i + n] == v && n < 63 {
                n += 1;
            }
            if n > 1 || v >= 0xC0 {
                out.push(0xC0 | n as u8);
            }
            out.push(v);
            i += n;
        }
        out
    }
    fn all(_: u32, _: u32) -> bool {
        true
    }
    fn px(p: &Pcx, x: u32, y: u32) -> [u8; 4] {
        let at = ((y * p.w + x) * 4) as usize;
        p.rgba[at..at + 4].try_into().unwrap()
    }

    #[test]
    fn true_colour_planes() {
        // 3 x 2, each line red plane, green plane, blue plane; the line is padded to an even 4 bytes
        let mut b = header(8, 3, 3, 2, 4, 5);
        let mut body = Vec::new();
        for y in 0..2u8 {
            for c in 0..3u8 {
                body.extend_from_slice(&[c * 80 + y, c * 80 + 10 + y, c * 80 + 20 + y, 0]);
            }
        }
        b.extend(rle(&body));
        let p = decode(&b, &all).unwrap();
        assert_eq!((p.w, p.h), (3, 2));
        assert_eq!(px(&p, 0, 0), [0, 80, 160, 255]);
        assert_eq!(px(&p, 2, 1), [21, 101, 181, 255]);
    }

    #[test]
    fn indexed_with_the_palette_at_the_end() {
        let mut b = header(8, 1, 4, 1, 4, 5);
        b.extend(rle(&[0, 1, 1, 255]));
        b.push(0x0C);
        let mut pal = vec![0u8; 768];
        pal[3..6].copy_from_slice(&[10, 20, 30]);
        pal[765..768].copy_from_slice(&[200, 210, 220]);
        b.extend(pal);
        let p = decode(&b, &all).unwrap();
        assert_eq!(px(&p, 0, 0), [0, 0, 0, 255]);
        assert_eq!(px(&p, 1, 0), [10, 20, 30, 255]);
        assert_eq!(px(&p, 3, 0), [200, 210, 220, 255]);
    }

    #[test]
    fn sixteen_colours_in_planes_and_black_and_white() {
        // 8 pixels in 4 planes: pixel x has colour x (bit k of x in plane k)
        let mut b = header(1, 4, 8, 1, 2, 5);
        for i in 0..16 {
            b[16 + i * 3..16 + i * 3 + 3].copy_from_slice(&[i as u8 * 16, 0, 0]);
        }
        let mut body = Vec::new();
        for k in 0..4 {
            let mut byte = 0u8;
            for x in 0..8 {
                if x >> k & 1 == 1 {
                    byte |= 0x80 >> x;
                }
            }
            body.extend_from_slice(&[byte, 0]);
        }
        b.extend(rle(&body));
        let p = decode(&b, &all).unwrap();
        for x in 0..8 {
            assert_eq!(px(&p, x, 0), [x as u8 * 16, 0, 0, 255]);
        }
        let mut b = header(1, 1, 10, 1, 2, 5);
        b.extend(rle(&[0b1010_0000, 0b0100_0000]));
        let p = decode(&b, &all).unwrap();
        let row: Vec<u8> = (0..10).map(|x| px(&p, x, 0)[0]).collect();
        assert_eq!(row, [255, 0, 255, 0, 0, 0, 0, 0, 0, 255]);
    }

    #[test]
    fn version_three_draws_with_the_ega_colours() {
        let mut b = header(4, 1, 2, 1, 2, 3);
        b.extend(rle(&[0x1F, 0]));
        let p = decode(&b, &all).unwrap();
        assert_eq!(px(&p, 0, 0), [0, 0, 170, 255]);
        assert_eq!(px(&p, 1, 0), [255, 255, 255, 255]);
    }

    #[test]
    fn a_run_may_cross_into_the_next_line() {
        let mut b = header(8, 1, 2, 2, 2, 5);
        b.extend_from_slice(&[0xC4, 7]);
        let p = decode(&b, &all).unwrap();
        assert_eq!(px(&p, 1, 1), [7, 7, 7, 255]);
    }

    #[test]
    fn nonsense_is_refused_and_size_is_asked_first() {
        assert_eq!(decode(&[0x0A; 20], &all).err(), Some(Problem::Damaged));
        let b = header(8, 2, 4, 4, 4, 5);
        assert_eq!(decode(&b, &all).err(), Some(Problem::Damaged));
        let mut b = header(8, 1, 4, 4, 4, 5);
        b.extend_from_slice(&[1, 2, 3]);
        assert_eq!(decode(&b, &|_, _| false).err(), Some(Problem::TooLarge));
        let b = header(8, 1, 4, 4, 4, 5);
        assert_eq!(decode(&b, &all).err(), Some(Problem::Damaged));
    }
}
