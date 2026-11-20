//! Reading a stream's headers bit by bit, and the little-endian and big-endian numbers of the containers.

pub(crate) struct Bits<'a> {
    b: &'a [u8],
    at: usize,
}

impl<'a> Bits<'a> {
    pub fn new(b: &'a [u8]) -> Self {
        Bits { b, at: 0 }
    }
    pub fn bit(&mut self) -> Option<u32> {
        let byte = *self.b.get(self.at / 8)?;
        let v = (byte >> (7 - self.at % 8)) & 1;
        self.at += 1;
        Some(v as u32)
    }
    pub fn read(&mut self, n: u32) -> Option<u32> {
        let mut v = 0u32;
        for _ in 0..n {
            v = v << 1 | self.bit()?;
        }
        Some(v)
    }
    /// An unsigned exponential-Golomb number (H.264's `ue(v)`).
    pub fn ue(&mut self) -> Option<u32> {
        let mut zeros = 0;
        while self.bit()? == 0 {
            zeros += 1;
            if zeros > 31 {
                return None;
            }
        }
        Some(((1u64 << zeros) - 1 + self.read(zeros)? as u64) as u32)
    }
    pub fn se(&mut self) -> Option<i32> {
        let k = self.ue()? as i64;
        Some(if k % 2 == 1 { ((k + 1) / 2) as i32 } else { -(k / 2) as i32 })
    }
}

/// An H.264 or HEVC unit without the bytes that keep it from looking like a start code (`00 00 03`).
pub(crate) fn unescape(b: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(b.len());
    let mut zeros = 0;
    for &x in b {
        if zeros >= 2 && x == 3 {
            zeros = 0;
            continue;
        }
        zeros = if x == 0 { zeros + 1 } else { 0 };
        out.push(x);
    }
    out
}

pub(crate) fn le16(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(i..i + 2)?.try_into().ok()?))
}
pub(crate) fn le32(b: &[u8], i: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(i..i + 4)?.try_into().ok()?))
}
pub(crate) fn be16(b: &[u8], i: usize) -> Option<u16> {
    Some(u16::from_be_bytes(b.get(i..i + 2)?.try_into().ok()?))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn golomb() {
        // 1, 010, 011, 00100 = 0, 1, 2, 3
        let b = [0b1010_0110, 0b0100_0000];
        let mut r = Bits::new(&b);
        assert_eq!((r.ue(), r.ue(), r.ue(), r.ue()), (Some(0), Some(1), Some(2), Some(3)));
        assert_eq!(unescape(&[0, 0, 3, 1, 0, 0, 3]), vec![0, 0, 1, 0, 0]);
    }
}
