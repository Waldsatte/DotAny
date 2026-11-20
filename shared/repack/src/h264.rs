//! H.264 from an AVI: start codes turned into lengths (as MP4 wants them), the stream's setup gathered into an `avcC`,
//! and the order the pictures are shown in, which an AVI doesn't keep (it has only the order they are decoded in): read
//! from each picture's own count (`pic_order_cnt`), as a decoder does.

use crate::Part;
use crate::bits::{Bits, unescape};

/// The units of an Annex B frame: where each starts and ends, start codes left out.
pub(crate) fn units(frame: &[u8]) -> Vec<(usize, usize)> {
    let mut starts = Vec::new();
    let mut i = 0;
    while i + 3 <= frame.len() {
        if frame[i] == 0 && frame[i + 1] == 0 && frame[i + 2] == 1 {
            starts.push((i, i + 3));
            i += 3;
        } else {
            i += 1;
        }
    }
    let mut out = Vec::new();
    for (k, &(_, body)) in starts.iter().enumerate() {
        let mut end = starts.get(k + 1).map_or(frame.len(), |s| s.0);
        // the zeros before the next start code belong to it (a four-byte start code), not to this unit
        while end > body && frame[end - 1] == 0 {
            end -= 1;
        }
        if end > body {
            out.push((body, end));
        }
    }
    out
}

/// Whether a frame is in Annex B form at all (an AVI may also hold H.264 as MP4 does, with lengths).
pub(crate) fn annex_b(frame: &[u8]) -> bool {
    frame.starts_with(&[0, 0, 1]) || frame.starts_with(&[0, 0, 0, 1])
}

/// The frame at `at` in the original, as MP4 parts: a four-byte length (written here) before each unit (the original's bytes).
pub(crate) fn parts(frame: &[u8], at: u64) -> Vec<Part> {
    let mut out = Vec::new();
    for (a, z) in units(frame) {
        out.push(Part::Own(((z - a) as u32).to_be_bytes().to_vec()));
        out.push(Part::Source(at + a as u64, (z - a) as u64));
    }
    out
}

/// The `avcC` box for a stream's first SPS and PPS.
pub(crate) fn avcc(sps: &[u8], pps: &[u8]) -> Option<Vec<u8>> {
    if sps.len() < 4 {
        return None;
    }
    let mut b = vec![1, sps[1], sps[2], sps[3], 0xFF, 0xE1];
    b.extend_from_slice(&(sps.len() as u16).to_be_bytes());
    b.extend_from_slice(sps);
    b.push(1);
    b.extend_from_slice(&(pps.len() as u16).to_be_bytes());
    b.extend_from_slice(pps);
    Some(crate::mp4::bx(b"avcC", &b))
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct Sps {
    separate_planes: bool,
    frame_num_bits: u32,
    poc_type: u32,
    poc_lsb_bits: u32,
    frame_mbs_only: bool,
}

fn skip_scaling_list(r: &mut Bits, size: usize) -> Option<()> {
    let (mut last, mut next) = (8i32, 8i32);
    for _ in 0..size {
        if next != 0 {
            next = (last + r.se()? + 256) % 256;
        }
        last = if next == 0 { last } else { next };
    }
    Some(())
}

pub(crate) fn sps(unit: &[u8]) -> Option<Sps> {
    let b = unescape(unit.get(1..)?);
    let mut r = Bits::new(&b);
    let profile = r.read(8)?;
    r.read(16)?;
    r.ue()?;
    let mut separate_planes = false;
    if [100, 110, 122, 244, 44, 83, 86, 118, 128, 138, 139, 134, 135].contains(&profile) {
        let chroma = r.ue()?;
        if chroma == 3 {
            separate_planes = r.bit()? == 1;
        }
        r.ue()?;
        r.ue()?;
        r.bit()?;
        if r.bit()? == 1 {
            for i in 0..if chroma == 3 { 12 } else { 8 } {
                if r.bit()? == 1 {
                    skip_scaling_list(&mut r, if i < 6 { 16 } else { 64 })?;
                }
            }
        }
    }
    let frame_num_bits = r.ue()? + 4;
    let poc_type = r.ue()?;
    let mut poc_lsb_bits = 0;
    if poc_type == 0 {
        poc_lsb_bits = r.ue()? + 4;
    } else if poc_type == 1 {
        r.bit()?;
        r.se()?;
        r.se()?;
        for _ in 0..r.ue()?.min(255) {
            r.se()?;
        }
    }
    r.ue()?;
    r.bit()?;
    r.ue()?;
    r.ue()?;
    let frame_mbs_only = r.bit()? == 1;
    Some(Sps { separate_planes, frame_num_bits, poc_type, poc_lsb_bits, frame_mbs_only })
}

/// Works out the order the pictures are shown in, one decoded picture at a time.
pub(crate) struct Order {
    sps: Option<Sps>,
    prev_msb: i64,
    prev_lsb: i64,
    /// IDR pictures so far: each starts the counting again.
    era: u64,
    keys: Vec<(u64, i64)>,
}

impl Order {
    pub fn new() -> Self {
        Order { sps: None, prev_msb: 0, prev_lsb: 0, era: 0, keys: Vec::new() }
    }

    /// One frame, in decode order; answers whether it is a keyframe (an IDR picture).
    pub fn frame(&mut self, frame: &[u8]) -> bool {
        let mut key = false;
        let mut counted = false;
        for (a, z) in units(frame) {
            let unit = &frame[a..z];
            let kind = unit[0] & 0x1F;
            if kind == 7 {
                if let Some(s) = sps(unit) {
                    self.sps = Some(s);
                }
            }
            if (kind == 1 || kind == 5) && !counted {
                counted = true;
                key = kind == 5;
                let poc = self.poc(unit, kind == 5).unwrap_or(self.prev_msb + self.prev_lsb);
                self.keys.push((self.era, poc));
            }
        }
        if !counted {
            // a frame without a picture keeps its place in the order it came
            let last = self.keys.last().copied().unwrap_or((self.era, 0));
            self.keys.push(last);
        }
        key
    }

    fn poc(&mut self, unit: &[u8], idr: bool) -> Option<i64> {
        let sps = self.sps?;
        let reference = unit[0] >> 5 & 3 != 0;
        let b = unescape(unit.get(1..unit.len().min(64))?);
        let mut r = Bits::new(&b);
        r.ue()?;
        r.ue()?;
        r.ue()?;
        if sps.separate_planes {
            r.read(2)?;
        }
        r.read(sps.frame_num_bits)?;
        if !sps.frame_mbs_only && r.bit()? == 1 {
            r.bit()?;
        }
        if idr {
            r.ue()?;
            self.era += 1;
            self.prev_msb = 0;
            self.prev_lsb = 0;
        }
        if sps.poc_type != 0 {
            // pictures are shown in the order they are decoded
            return Some(self.keys.len() as i64);
        }
        let lsb = r.read(sps.poc_lsb_bits)? as i64;
        let max = 1i64 << sps.poc_lsb_bits;
        let msb = if lsb < self.prev_lsb && self.prev_lsb - lsb >= max / 2 {
            self.prev_msb + max
        } else if lsb > self.prev_lsb && lsb - self.prev_lsb > max / 2 {
            self.prev_msb - max
        } else {
            self.prev_msb
        };
        if reference {
            self.prev_msb = msb;
            self.prev_lsb = lsb;
        }
        Some(msb + lsb)
    }

    /// Each frame's place in the order shown, in decode order.
    pub fn shown(&self) -> Vec<u64> {
        let mut by: Vec<usize> = (0..self.keys.len()).collect();
        by.sort_by_key(|&i| (self.keys[i], i));
        let mut rank = vec![0u64; self.keys.len()];
        for (r, i) in by.into_iter().enumerate() {
            rank[i] = r as u64;
        }
        rank
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn units_and_lengths() {
        let f = [0, 0, 0, 1, 0x67, 1, 2, 0, 0, 1, 0x68, 3, 0, 0, 0, 1, 0x65, 9, 9];
        assert_eq!(units(&f), vec![(4, 7), (10, 12), (16, 19)]);
        let p = parts(&f, 100);
        assert!(matches!(&p[0], Part::Own(b) if b == &[0, 0, 0, 3]));
        assert!(matches!(p[1], Part::Source(104, 3)));
    }
}
