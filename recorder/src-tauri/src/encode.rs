//! A take to WAV, FLAC or MP3: 16-bit sound, one or two channels, nothing to resample and no tags. The FLAC and MP3
//! encoders are the family's (dotany-sound-encode).

use crate::wav::{self, Blocks};
use dotany_sound_encode::flac::{FlacEncoder, Out};
use dotany_sound_encode::lame;
use serde::Deserialize;
use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::Path;

#[derive(Clone, Copy, PartialEq, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Wav,
    Flac,
    Mp3,
}

#[derive(Clone, Copy, PartialEq, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Quality {
    Std,
    Hi,
    Top,
}

/// The MP3 bit rate behind each quality: Standard, High and Maximum.
pub fn kbps(q: Quality) -> u32 {
    match q {
        Quality::Std => 128,
        Quality::Hi => 192,
        Quality::Top => 320,
    }
}

const MP3_RATES: [u32; 9] = [8000, 11025, 12000, 16000, 22050, 24000, 32000, 44100, 48000];

/// Why a take can't be written in a format, before anything is written.
pub fn refusal(kind: Kind, rate: u32) -> Option<&'static str> {
    (kind == Kind::Mp3 && !MP3_RATES.contains(&rate)).then_some("mp3rate")
}

enum Encoder {
    Pcm(PcmWriter),
    Flac(FlacEncoder),
    Mp3(Mp3Writer),
}

impl Encoder {
    fn write(&mut self, planar: &[Vec<f32>], n: usize) -> Result<(), String> {
        match self {
            Encoder::Pcm(w) => w.write(planar, n),
            Encoder::Flac(w) => w.write(planar, n),
            Encoder::Mp3(w) => w.write(planar, n),
        }
    }
    fn finish(self) -> Result<(), String> {
        match self {
            Encoder::Pcm(w) => w.finish(),
            Encoder::Flac(w) => w.finish(),
            Encoder::Mp3(w) => w.finish(),
        }
    }
}

/// Writes the take at `from` to `to` (a temp file the caller swaps into place).
pub fn write(from: &Path, to: &Path, kind: Kind, quality: Quality) -> Result<(), String> {
    let (info, mut blocks) = Blocks::open(from).map_err(|e| e.to_string())?;
    if let Some(why) = refusal(kind, info.rate) {
        return Err(why.into());
    }
    let channels = info.channels as usize;
    let out: Box<dyn Out> = Box::new(BufWriter::with_capacity(1 << 16, File::create(to).map_err(|e| e.to_string())?));
    let mut enc = match kind {
        Kind::Wav => Encoder::Pcm(PcmWriter::new(out, info.rate, channels, info.frames())?),
        Kind::Flac => Encoder::Flac(FlacEncoder::new(out, channels, info.rate, 16, info.frames())?),
        Kind::Mp3 => Encoder::Mp3(Mp3Writer::new(out, info.rate, channels, kbps(quality))?),
    };
    let mut bufs = vec![Vec::new(); channels];
    loop {
        let n = blocks.next(1 << 15, &mut bufs).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        enc.write(&bufs, n)?;
    }
    enc.finish()
}

// ---------- WAV: plain samples ----------

struct PcmWriter {
    out: Box<dyn Out>,
    channels: usize,
    rate: u32,
    written: u64,
    buf: Vec<u8>,
}

impl PcmWriter {
    fn new(mut out: Box<dyn Out>, rate: u32, channels: usize, frames: u64) -> Result<Self, String> {
        // the format counts its bytes in 32 bits
        if frames * channels as u64 * 2 > u32::MAX as u64 - 1024 {
            return Err("toolarge".into());
        }
        out.write_all(&wav::header(rate, channels as u16, 0)).map_err(|e| e.to_string())?;
        Ok(PcmWriter { out, channels, rate, written: 0, buf: Vec::new() })
    }

    fn write(&mut self, planar: &[Vec<f32>], n: usize) -> Result<(), String> {
        self.buf.clear();
        for f in 0..n {
            for p in planar.iter().take(self.channels) {
                let v = (p[f] as f64 * 32768.0).round().clamp(-32768.0, 32767.0) as i16;
                self.buf.extend_from_slice(&v.to_le_bytes());
            }
        }
        self.out.write_all(&self.buf).map_err(|e| e.to_string())?;
        self.written += self.buf.len() as u64;
        Ok(())
    }

    fn finish(mut self) -> Result<(), String> {
        let e = |e: std::io::Error| e.to_string();
        self.out.seek(SeekFrom::Start(0)).map_err(e)?;
        self.out.write_all(&wav::header(self.rate, self.channels as u16, self.written)).map_err(e)?;
        self.out.seek(SeekFrom::End(0)).map_err(e)?;
        self.out.flush().map_err(e)
    }
}

// ---------- MP3 ----------

struct Mp3Writer {
    lame: &'static lame::Lame,
    handle: lame::Handle,
    out: Box<dyn Out>,
    channels: usize,
    buf: Vec<u8>,
    clip: Vec<Vec<f32>>,
}

unsafe impl Send for Mp3Writer {}

impl Mp3Writer {
    fn new(out: Box<dyn Out>, rate: u32, channels: usize, kbps: u32) -> Result<Self, String> {
        let l = lame::get()?;
        // SAFETY: LAME's documented order: init, set, init_params
        let handle = unsafe {
            let h = (l.init)();
            if h.is_null() {
                return Err("LAME init".into());
            }
            (l.set_in_rate)(h, rate as i32);
            (l.set_out_rate)(h, rate as i32);
            (l.set_channels)(h, channels as i32);
            (l.set_mode)(h, if channels == 1 { lame::MODE_MONO } else { lame::MODE_JOINT });
            (l.set_vbr)(h, lame::VBR_OFF);
            (l.set_brate)(h, kbps as i32);
            (l.set_quality)(h, 2);
            // the first frame says how long the file is and how much padding to skip, so the length stays exact
            (l.set_vbr_tag)(h, 1);
            if (l.init_params)(h) < 0 {
                (l.close)(h);
                return Err("LAME settings".into());
            }
            h
        };
        Ok(Mp3Writer { lame: l, handle, out, channels, buf: Vec::new(), clip: vec![Vec::new(); channels] })
    }

    fn write(&mut self, planar: &[Vec<f32>], n: usize) -> Result<(), String> {
        let need = n + n / 4 + 7200;
        if self.buf.len() < need {
            self.buf.resize(need, 0);
        }
        for (c, p) in self.clip.iter_mut().zip(planar) {
            c.clear();
            c.extend(p[..n].iter().map(|v| v.clamp(-1.0, 1.0)));
        }
        let right = if self.channels == 1 { std::ptr::null() } else { self.clip[1].as_ptr() };
        // SAFETY: both channels hold n samples and the buffer is LAME's documented worst case
        let got = unsafe {
            (self.lame.encode)(
                self.handle,
                self.clip[0].as_ptr(),
                right,
                n as i32,
                self.buf.as_mut_ptr(),
                self.buf.len() as i32,
            )
        };
        if got < 0 {
            return Err(format!("LAME {got}"));
        }
        self.out.write_all(&self.buf[..got as usize]).map_err(|e| e.to_string())
    }

    fn finish(mut self) -> Result<(), String> {
        let mut last = vec![0u8; 7200 + 1024];
        // SAFETY: the buffer is larger than the 7200 bytes LAME asks for
        let got = unsafe { (self.lame.flush)(self.handle, last.as_mut_ptr(), last.len() as i32) };
        if got < 0 {
            return Err(format!("LAME flush {got}"));
        }
        self.out.write_all(&last[..got as usize]).map_err(|e| e.to_string())?;
        let mut tag = vec![0u8; 2880];
        // SAFETY: LAME writes at most the size it's told
        let size = unsafe { (self.lame.tag_frame)(self.handle, tag.as_mut_ptr(), tag.len()) };
        if size > 0 && size <= tag.len() {
            self.out.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
            self.out.write_all(&tag[..size]).map_err(|e| e.to_string())?;
            self.out.seek(SeekFrom::End(0)).map_err(|e| e.to_string())?;
        }
        self.out.flush().map_err(|e| e.to_string())
    }
}

impl Drop for Mp3Writer {
    fn drop(&mut self) {
        // SAFETY: the handle came from lame_init and is closed once
        unsafe { (self.lame.close)(self.handle) };
    }
}
