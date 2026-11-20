//! Films the Mac's player won't open as they are, repackaged for it without decoding a single picture or sound
//! (for the Mac only): an MKV with H.264, HEVC, MPEG-4 Part 2, MPEG-2 or Motion JPEG,
//! and an AVI with H.264, MPEG-4 Part 2 (DivX 4+, Xvid), Motion JPEG or DV, become a QuickTime film whose samples are
//! the very bytes of the original, read from it as the player asks (`Repacked::read_at`), so nothing is written to the
//! disk and a film of any size opens at once.
//!
//! What WKWebView plays inside such a film, found by asking WKWebView itself: pictures in H.264, HEVC, MPEG-4 Part 2,
//! MPEG-2, Motion JPEG and DV; sound in AAC, AC-3, E-AC-3, MP3, MP2, Opus, FLAC and 16-bit PCM. Not AV1, not DivX 3
//! (MS MPEG-4 v3), not DTS, TrueHD or Vorbis in such a film. A film whose picture is none of those is refused
//! (`Refusal::Picture`); a sound the Mac can't play is left out, and the film plays without it, as WebView2 plays a
//! film whose sound it can't decode.
//!
//! An MKV whose picture is VP8 or VP9 is played by WKWebView as it is (WebKit reads Matroska itself, then), so it is not repackaged.
//! B-frames are kept in their order: an MKV gives every picture its own time; an AVI with H.264 has its order worked out
//! from the stream (`h264::Order`), and an AVI with MPEG-4 Part 2 B-frames (DivX's "packed" streams) is refused.

mod avi;
mod bits;
mod codec;
mod h264;
mod mkv;
mod mp4;

use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;

/// Why a film can't be repackaged.
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Refusal {
    /// Not a film this crate can read: damaged, cut short where nothing is left, or not what its name says.
    Damaged,
    /// Its picture is in a format the Mac doesn't play (or a kind of stream this crate doesn't rewrite).
    Picture,
}

/// Where a stretch of the new film comes from.
#[derive(Debug, Clone, Copy)]
enum From {
    /// The original file, at this place.
    Source(u64),
    /// `Repacked::own`, at this place: bytes made here (a length in front of each H.264 piece of an AVI).
    Own(u64),
}

#[derive(Debug, Clone, Copy)]
struct Stretch {
    /// Where it starts in the new film.
    at: u64,
    len: u64,
    from: From,
}

/// The new film: its head (the boxes that say what is where) and, after it, the samples, each a list of stretches of
/// the original file.
pub struct Repacked {
    head: Vec<u8>,
    stretches: Vec<Stretch>,
    own: Vec<u8>,
    size: u64,
    /// The original had a sound the Mac can't play, and the new film has none.
    pub sound_left_out: bool,
    quicktime: bool,
}

impl Repacked {
    pub fn size(&self) -> u64 {
        self.size
    }

    pub fn mime(&self) -> &'static str {
        if self.quicktime { "video/quicktime" } else { "video/mp4" }
    }

    /// Fills `buf` from `at` in the new film, reading the original `source` where the samples are; answers how much it
    /// filled (less only at the end).
    pub fn read_at(&self, source: &mut File, at: u64, buf: &mut [u8]) -> io::Result<usize> {
        let mut done = 0usize;
        let mut pos = at;
        while done < buf.len() && pos < self.size {
            let want = buf.len() - done;
            if pos < self.head.len() as u64 {
                let from = &self.head[pos as usize..];
                let n = from.len().min(want);
                buf[done..done + n].copy_from_slice(&from[..n]);
                done += n;
                pos += n as u64;
                continue;
            }
            let i = self.stretches.partition_point(|s| s.at + s.len <= pos);
            let Some(s) = self.stretches.get(i) else { break };
            let into = pos - s.at;
            let n = ((s.len - into) as usize).min(want);
            match s.from {
                From::Source(o) => {
                    source.seek(SeekFrom::Start(o + into))?;
                    source.read_exact(&mut buf[done..done + n])?;
                }
                From::Own(o) => {
                    let a = (o + into) as usize;
                    buf[done..done + n].copy_from_slice(&self.own[a..a + n]);
                }
            }
            done += n;
            pos += n as u64;
        }
        Ok(done)
    }

    /// The whole new film in memory (Slides: a film inside a slide is small, and its page plays it from memory).
    pub fn to_vec(&self, source: &mut File) -> io::Result<Vec<u8>> {
        let mut out = vec![0u8; self.size as usize];
        let n = self.read_at(source, 0, &mut out)?;
        out.truncate(n);
        Ok(out)
    }
}

/// What the first bytes say the file is.
#[derive(Debug, PartialEq, Clone, Copy)]
pub enum Kind {
    Matroska,
    Avi,
}

pub fn kind_of(head: &[u8]) -> Option<Kind> {
    if head.starts_with(&[0x1A, 0x45, 0xDF, 0xA3]) {
        return Some(Kind::Matroska);
    }
    if head.len() >= 12 && head.starts_with(b"RIFF") && &head[8..12] == b"AVI " {
        return Some(Kind::Avi);
    }
    None
}

/// Whether WKWebView plays this Matroska file as it is (its picture VP8 or VP9): then it isn't repackaged.
pub fn plays_as_it_is(path: &Path) -> bool {
    mkv::picture_codec(path).is_some_and(|c| c == "V_VP8" || c == "V_VP9")
}

/// Repackages an MKV or an AVI for the Mac's player.
pub fn repack(path: &Path) -> Result<Repacked, Refusal> {
    let mut f = File::open(path).map_err(|_| Refusal::Damaged)?;
    let mut head = [0u8; 12];
    let n = f.read(&mut head).map_err(|_| Refusal::Damaged)?;
    let film = match kind_of(&head[..n]) {
        Some(Kind::Matroska) => mkv::read(&mut f)?,
        Some(Kind::Avi) => avi::read(&mut f)?,
        None => return Err(Refusal::Damaged),
    };
    // nothing in it can be shown: a film that is only sound, or whose picture the Mac doesn't play
    if !film.tracks.iter().any(|t| t.video) {
        return Err(if film.had_picture { Refusal::Picture } else { Refusal::Damaged });
    }
    Ok(mp4::write(film))
}

// ---------- what the readers hand the writer ----------

/// A piece of one sample.
#[derive(Debug, Clone)]
pub(crate) enum Part {
    Source(u64, u64),
    Own(Vec<u8>),
}

#[derive(Debug, Clone)]
pub(crate) struct Sample {
    /// When it is shown, in the track's time scale, in the order the samples are decoded.
    pub pts: i64,
    pub key: bool,
    pub parts: Vec<Part>,
    /// Where it was in the original, so the new film keeps the tracks' samples interleaved as they were.
    pub order: u64,
    /// For PCM only: how many sound frames this sample holds (the writer counts each frame as a sample of its own).
    pub frames: u32,
}

impl Sample {
    pub fn size(&self) -> u64 {
        self.parts
            .iter()
            .map(|p| match p {
                Part::Source(_, l) => *l,
                Part::Own(b) => b.len() as u64,
            })
            .sum()
    }
}

pub(crate) struct Track {
    pub video: bool,
    pub timescale: u32,
    /// What the sample entry (`avc1`, `mp4a`…) says, with what the decoder needs.
    pub entry: crate::codec::Entry,
    /// The picture's size as shown, for the track header.
    pub width: u32,
    pub height: u32,
    pub samples: Vec<Sample>,
    /// How long the last sample lasts, when nothing after it says.
    pub last: u32,
    /// For PCM: bytes per sound frame; each stored sample is then a run of `frames` frames.
    pub pcm: Option<u32>,
}

pub(crate) struct Film {
    pub tracks: Vec<Track>,
    /// It had a picture, but not one the Mac plays.
    pub had_picture: bool,
    pub sound_left_out: bool,
}
