//! The one kind of file Recorder writes and reads back: WAV, 16-bit PCM. A take is written as it is recorded, so its
//! header can be behind its sound; `repair` brings it up to date from the file's real length.

use std::fs::{File, OpenOptions};
use std::io::{self, BufReader, Read, Seek, SeekFrom, Write};
use std::path::Path;

/// Where the sound starts in a file Recorder writes.
pub const HEADER: u64 = 44;

pub fn header(rate: u32, channels: u16, data_bytes: u64) -> [u8; 44] {
    let data = data_bytes.min(u32::MAX as u64 - 36) as u32;
    let block = channels as u32 * 2;
    let mut h = [0u8; 44];
    h[0..4].copy_from_slice(b"RIFF");
    h[4..8].copy_from_slice(&(36 + data).to_le_bytes());
    h[8..16].copy_from_slice(b"WAVEfmt ");
    h[16..20].copy_from_slice(&16u32.to_le_bytes());
    h[20..22].copy_from_slice(&1u16.to_le_bytes());
    h[22..24].copy_from_slice(&channels.to_le_bytes());
    h[24..28].copy_from_slice(&rate.to_le_bytes());
    h[28..32].copy_from_slice(&(rate * block).to_le_bytes());
    h[32..34].copy_from_slice(&(block as u16).to_le_bytes());
    h[34..36].copy_from_slice(&16u16.to_le_bytes());
    h[36..40].copy_from_slice(b"data");
    h[40..44].copy_from_slice(&data.to_le_bytes());
    h
}

/// What a WAV says about itself, and where its sound is.
#[derive(Clone, Copy, Debug)]
pub struct Info {
    pub rate: u32,
    pub channels: u16,
    pub data_at: u64,
    /// Bytes of sound as the header says, or to the end of the file when the header is behind.
    pub data_bytes: u64,
}

impl Info {
    pub fn frames(&self) -> u64 {
        self.data_bytes / (self.channels as u64 * 2)
    }
}

/// Reads the chunks up to the sound. Only 16-bit PCM with one or two channels: what Recorder itself makes.
pub fn read_info(f: &mut File) -> Option<Info> {
    let len = f.metadata().ok()?.len();
    f.seek(SeekFrom::Start(0)).ok()?;
    let mut head = [0u8; 12];
    f.read_exact(&mut head).ok()?;
    if &head[0..4] != b"RIFF" || &head[8..12] != b"WAVE" {
        return None;
    }
    let mut at = 12u64;
    let mut fmt: Option<(u16, u16, u32, u16)> = None;
    while at + 8 <= len {
        let mut c = [0u8; 8];
        f.seek(SeekFrom::Start(at)).ok()?;
        f.read_exact(&mut c).ok()?;
        let size = u32::from_le_bytes(c[4..8].try_into().unwrap()) as u64;
        match &c[0..4] {
            b"fmt " => {
                let mut b = [0u8; 16];
                f.read_exact(&mut b).ok()?;
                let tag = u16::from_le_bytes([b[0], b[1]]);
                let channels = u16::from_le_bytes([b[2], b[3]]);
                let rate = u32::from_le_bytes(b[4..8].try_into().unwrap());
                let bits = u16::from_le_bytes([b[14], b[15]]);
                fmt = Some((tag, channels, rate, bits));
            }
            b"data" => {
                let (tag, channels, rate, bits) = fmt?;
                if tag != 1 || bits != 16 || !(1..=2).contains(&channels) || rate < 8000 || rate > 192_000 {
                    return None;
                }
                let start = at + 8;
                let room = len.saturating_sub(start);
                // a header written before the sound caught up says less than is there, or nothing at all
                let said = if size == 0 || size == u32::MAX as u64 || size > room { room } else { size };
                let block = channels as u64 * 2;
                return Some(Info { rate, channels, data_at: start, data_bytes: said / block * block });
            }
            _ => {}
        }
        at += 8 + size + (size & 1);
    }
    None
}

pub fn open_info(path: &Path) -> Option<Info> {
    read_info(&mut File::open(path).ok()?)
}

/// Is this 44-byte header the kind Recorder writes: plain 16-bit PCM, one or two channels, `fmt ` then `data`?
fn is_take_header(head: &[u8; 44]) -> bool {
    let u16_at = |i: usize| u16::from_le_bytes([head[i], head[i + 1]]);
    let rate = u32::from_le_bytes(head[24..28].try_into().unwrap());
    &head[0..4] == b"RIFF"
        && &head[8..16] == b"WAVEfmt "
        && u32::from_le_bytes(head[16..20].try_into().unwrap()) == 16
        && u16_at(20) == 1
        && (1..=2).contains(&u16_at(22))
        && (8000..=192_000).contains(&rate)
        && u16_at(34) == 16
        && &head[36..40] == b"data"
}

/// Brings a take's header up to its real length, after a recording that didn't get to stop. Leaves the sound alone.
/// Only a header Recorder itself writes (16-bit PCM) is touched: any other WAV, 8-bit, 24-bit or float, is refused
/// and left exactly as it is, since cutting it to whole 16-bit samples would damage it.
pub fn repair(path: &Path) -> io::Result<Info> {
    let mut f = OpenOptions::new().read(true).write(true).open(path)?;
    let len = f.metadata()?.len();
    let mut head = [0u8; 44];
    let whole = len >= HEADER && f.read_exact(&mut head).is_ok();
    if !whole || !is_take_header(&head) {
        return Err(io::Error::new(io::ErrorKind::InvalidData, "not a take"));
    }
    let channels = u16::from_le_bytes([head[22], head[23]]);
    let rate = u32::from_le_bytes(head[24..28].try_into().unwrap());
    let block = channels as u64 * 2;
    let data = (len - HEADER) / block * block;
    // half a sample at the end, from a write cut short, goes
    if HEADER + data < len {
        f.set_len(HEADER + data)?;
    }
    f.seek(SeekFrom::Start(0))?;
    f.write_all(&header(rate, channels, data))?;
    f.sync_all()?;
    Ok(Info { rate, channels, data_at: HEADER, data_bytes: data })
}

/// The sound a block at a time, as floats, one Vec per channel.
pub struct Blocks {
    r: BufReader<File>,
    left: u64,
    channels: usize,
    raw: Vec<u8>,
}

impl Blocks {
    pub fn open(path: &Path) -> io::Result<(Info, Blocks)> {
        let mut f = File::open(path)?;
        let info = read_info(&mut f).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "not 16-bit PCM WAV"))?;
        f.seek(SeekFrom::Start(info.data_at))?;
        Ok((
            info,
            Blocks {
                r: BufReader::with_capacity(1 << 16, f),
                left: info.data_bytes,
                channels: info.channels as usize,
                raw: Vec::new(),
            },
        ))
    }

    /// Up to `frames` frames into `out`; the number read, 0 at the end.
    pub fn next(&mut self, frames: usize, out: &mut [Vec<f32>]) -> io::Result<usize> {
        let block = self.channels * 2;
        let want = (frames as u64 * block as u64).min(self.left) as usize;
        if want == 0 {
            return Ok(0);
        }
        self.raw.resize(want, 0);
        self.r.read_exact(&mut self.raw)?;
        self.left -= want as u64;
        let n = want / block;
        for (c, o) in out.iter_mut().enumerate().take(self.channels) {
            o.clear();
            o.extend((0..n).map(|i| {
                let at = i * block + c * 2;
                i16::from_le_bytes([self.raw[at], self.raw[at + 1]]) as f32 / 32768.0
            }));
        }
        Ok(n)
    }
}

/// One bar of the drawn line every this many frames: 4 pixels a bar at 60 pixels a second, so 15 bars a second.
pub fn frames_per_bar(rate: u32) -> usize {
    ((rate as f64) / 15.0).round().max(1.0) as usize
}

/// The loudest moment of each bar, 0 to 1, across the channels.
pub fn peaks(path: &Path) -> io::Result<(Info, Vec<f32>)> {
    let (info, mut blocks) = Blocks::open(path)?;
    let per = frames_per_bar(info.rate);
    let mut out = Vec::with_capacity(info.frames() as usize / per + 1);
    let mut bufs = vec![Vec::new(); info.channels as usize];
    let (mut peak, mut count) = (0f32, 0usize);
    loop {
        let n = blocks.next(per * 64, &mut bufs)?;
        if n == 0 {
            break;
        }
        for i in 0..n {
            for b in &bufs {
                peak = peak.max(b[i].abs());
            }
            count += 1;
            if count == per {
                out.push(peak);
                peak = 0.0;
                count = 0;
            }
        }
    }
    if count > 0 {
        out.push(peak);
    }
    Ok((info, out))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn header_with(bits: u16, tag: u16) -> [u8; 44] {
        let mut h = header(48000, 1, 0);
        h[20..22].copy_from_slice(&tag.to_le_bytes());
        h[34..36].copy_from_slice(&bits.to_le_bytes());
        h
    }

    #[test]
    fn only_a_16_bit_take_is_repaired() {
        let dir = std::env::temp_dir().join(format!("dotany-recorder-repair-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // 24-bit PCM, 32-bit float and 8-bit, with a stray byte at the end: refused, and untouched byte for byte
        for (name, h) in
            [("24.wav", header_with(24, 1)), ("float.wav", header_with(32, 3)), ("8.wav", header_with(8, 1))]
        {
            let p = dir.join(name);
            let body = [h.as_slice(), &[1, 2, 3, 4, 5, 6, 7, 8, 9]].concat();
            std::fs::write(&p, &body).unwrap();
            assert_eq!(repair(&p).unwrap_err().kind(), io::ErrorKind::InvalidData, "{name}");
            assert_eq!(std::fs::read(&p).unwrap(), body, "{name} left alone");
        }
        // a 16-bit take whose header is behind: brought up to its sound, the half sample at the end cut
        let p = dir.join("take.wav");
        std::fs::write(&p, [header(48000, 1, 0).as_slice(), &[1, 0, 2, 0, 3]].concat()).unwrap();
        let info = repair(&p).unwrap();
        assert_eq!((info.data_bytes, info.channels, info.rate), (4, 1, 48000));
        assert_eq!(std::fs::read(&p).unwrap().len(), 48);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
