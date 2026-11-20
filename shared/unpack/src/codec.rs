//! The single-file compressions an archive (or one file) can be wrapped in: gzip, bzip2, xz and zstd, each known by its
//! first bytes and read as a stream, never whole. All four decoders are pure Rust: flate2 (zlib-rs), bzip2
//! (libbz2-rs-sys), lzma-rust2 and ruzstd. Nothing here compresses.

use std::io::{self, BufRead, BufReader, Read};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Codec {
    Gzip,
    Bzip2,
    Xz,
    Zstd,
}

impl Codec {
    /// The compression these first bytes start, if any.
    pub(crate) fn of(head: &[u8]) -> Option<Codec> {
        if head.starts_with(&[0x1F, 0x8B]) {
            return Some(Codec::Gzip);
        }
        // "BZh" and a block size of 1 to 9 (x 100 KB)
        if head.len() >= 4 && head.starts_with(b"BZh") && (b'1'..=b'9').contains(&head[3]) {
            return Some(Codec::Bzip2);
        }
        if head.starts_with(&[0xFD, b'7', b'z', b'X', b'Z', 0x00]) {
            return Some(Codec::Xz);
        }
        // a zstd frame, or a skippable frame (0x184D2A50 to 0x184D2A5F) that a zstd frame follows
        if head.starts_with(&[0x28, 0xB5, 0x2F, 0xFD])
            || head.len() >= 4 && head[0] & 0xF0 == 0x50 && head[1..4] == [0x2A, 0x4D, 0x18]
        {
            return Some(Codec::Zstd);
        }
        None
    }

    /// What it's called after a dot, as the page shows the format: "tar.xz", "gz"
    pub(crate) fn suffix(self) -> &'static str {
        match self {
            Codec::Gzip => "gz",
            Codec::Bzip2 => "bz2",
            Codec::Xz => "xz",
            Codec::Zstd => "zst",
        }
    }

    /// The decompressed stream. Concatenated members (gzip, bzip2, xz streams, zstd frames) read as one, as their
    /// command-line tools read them.
    pub(crate) fn reader<'a>(self, inner: Box<dyn Read + Send + 'a>) -> Box<dyn Read + Send + 'a> {
        let inner = BufReader::with_capacity(1 << 16, inner);
        match self {
            Codec::Gzip => Box::new(flate2::bufread::MultiGzDecoder::new(inner)),
            Codec::Bzip2 => Box::new(bzip2::bufread::MultiBzDecoder::new(inner)),
            Codec::Xz => Box::new(lzma_rust2::XzReader::new(inner, true)),
            Codec::Zstd => Box::new(Zstd { src: inner, frame: ruzstd::decoding::FrameDecoder::new(), started: false }),
        }
    }
}

/// zstd, frame after frame. ruzstd's own reader stops after one frame and refuses skippable ones; the file format allows
/// both, so this walks them: a skippable frame is passed over, each zstd frame's checksum (when it has one) is checked.
struct Zstd<R: BufRead> {
    src: R,
    frame: ruzstd::decoding::FrameDecoder,
    /// a frame is being decoded
    started: bool,
}

fn bad(e: impl std::fmt::Display) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, e.to_string())
}

impl<R: BufRead> Read for Zstd<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        loop {
            if self.started {
                while self.frame.can_collect() == 0 && !self.frame.is_finished() {
                    self.frame
                        .decode_blocks(&mut self.src, ruzstd::decoding::BlockDecodingStrategy::UptoBlocks(1))
                        .map_err(bad)?;
                }
                let n = self.frame.read(buf)?;
                if n > 0 {
                    return Ok(n);
                }
                if let (Some(want), Some(got)) =
                    (self.frame.get_checksum_from_data(), self.frame.get_calculated_checksum())
                {
                    if want != got {
                        return Err(bad("zstd checksum mismatch"));
                    }
                }
                self.started = false;
            }
            // the next frame, if anything is left
            if self.src.fill_buf()?.is_empty() {
                return Ok(0);
            }
            match self.frame.reset(&mut self.src) {
                Ok(()) => self.started = true,
                Err(ruzstd::decoding::errors::FrameDecoderError::ReadFrameHeaderError(
                    ruzstd::decoding::errors::ReadFrameHeaderError::SkipFrame { length, .. },
                )) => {
                    let skipped = io::copy(&mut (&mut self.src).take(length as u64), &mut io::sink())?;
                    if skipped != length as u64 {
                        return Err(bad("zstd skippable frame cut short"));
                    }
                }
                Err(e) => return Err(bad(e)),
            }
        }
    }
}

/// Reads until `buf` is full or the stream ends; answers how much came.
pub(crate) fn read_full(r: &mut dyn Read, buf: &mut [u8]) -> io::Result<usize> {
    let mut n = 0;
    while n < buf.len() {
        match r.read(&mut buf[n..]) {
            Ok(0) => break,
            Ok(k) => n += k,
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    Ok(n)
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::io::Write;

    /// `data` compressed with each codec, by the same libraries' encoders (tests only).
    pub fn pack(codec: Codec, data: &[u8]) -> Vec<u8> {
        match codec {
            Codec::Gzip => {
                let mut e = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
                e.write_all(data).unwrap();
                e.finish().unwrap()
            }
            Codec::Bzip2 => {
                let mut e = bzip2::write::BzEncoder::new(Vec::new(), bzip2::Compression::default());
                e.write_all(data).unwrap();
                e.finish().unwrap()
            }
            Codec::Xz => {
                let mut e = lzma_rust2::XzWriter::new(Vec::new(), lzma_rust2::XzOptions::with_preset(6)).unwrap();
                e.write_all(data).unwrap();
                e.finish().unwrap()
            }
            Codec::Zstd => ruzstd::encoding::compress_to_vec(data, ruzstd::encoding::CompressionLevel::Fastest),
        }
    }

    fn unpack(codec: Codec, packed: Vec<u8>) -> io::Result<Vec<u8>> {
        let mut out = Vec::new();
        codec.reader(Box::new(io::Cursor::new(packed))).read_to_end(&mut out)?;
        Ok(out)
    }

    const ALL: [Codec; 4] = [Codec::Gzip, Codec::Bzip2, Codec::Xz, Codec::Zstd];

    #[test]
    fn each_codec_is_known_by_its_first_bytes_and_reads_back() {
        let text: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8 ^ (i / 7) as u8).collect();
        for c in ALL {
            let packed = pack(c, &text);
            assert_eq!(Codec::of(&packed), Some(c), "{c:?}");
            assert_eq!(unpack(c, packed).unwrap(), text, "{c:?}");
        }
        assert_eq!(Codec::of(b"PK\x03\x04"), None);
        assert_eq!(Codec::of(b"BZh0"), None);
    }

    #[test]
    fn concatenated_members_read_as_one() {
        for c in ALL {
            let mut two = pack(c, b"first half, ");
            two.extend(pack(c, b"second half"));
            assert_eq!(unpack(c, two).unwrap(), b"first half, second half", "{c:?}");
        }
    }

    #[test]
    fn a_zstd_skippable_frame_is_passed_over() {
        let mut data = vec![0x50, 0x2A, 0x4D, 0x18, 5, 0, 0, 0, 1, 2, 3, 4, 5];
        assert_eq!(Codec::of(&data), Some(Codec::Zstd));
        data.extend(pack(Codec::Zstd, b"after the skip"));
        assert_eq!(unpack(Codec::Zstd, data).unwrap(), b"after the skip");
    }

    #[test]
    fn a_cut_stream_is_an_error_not_a_short_file() {
        let data: Vec<u8> = (0..100_000u32).map(|i| (i.wrapping_mul(2654435761) >> 24) as u8).collect();
        for c in ALL {
            let mut packed = pack(c, &data);
            packed.truncate(packed.len() / 2);
            assert!(unpack(c, packed).is_err(), "{c:?}");
        }
    }

    #[test]
    fn a_damaged_byte_is_caught() {
        // a flipped byte in the middle: the stream's checksum (or the decoder) says so
        let data: Vec<u8> = (0..50_000u32).map(|i| (i.wrapping_mul(2654435761) >> 24) as u8).collect();
        for c in ALL {
            let mut packed = pack(c, &data);
            let mid = packed.len() / 2;
            packed[mid] ^= 0x55;
            match unpack(c, packed) {
                Ok(out) => assert_ne!(out, data, "{c:?} passed a damaged stream off as whole"),
                Err(_) => {}
            }
        }
    }
}
