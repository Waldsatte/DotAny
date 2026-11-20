//! Samples to each format's bytes. All of them take the same blocks, and none of them touches tags: see tags.rs.

use crate::flac::{FlacEncoder, Out};
use crate::formats::Kind;
use crate::lame;
use serde::Deserialize;
use std::io::{Seek, SeekFrom, Write};
use std::num::{NonZeroU8, NonZeroU32};
use vorbis_rs::{VorbisBitrateManagementStrategy, VorbisEncoder, VorbisEncoderBuilder};

#[derive(Clone, Copy, PartialEq, Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Quality {
    Std,
    Hi,
    Top,
}

pub fn kbps(kind: Kind, q: Quality) -> u32 {
    match (kind, q) {
        (Kind::Mp3, Quality::Std) => 128,
        (Kind::Mp3, Quality::Hi) => 192,
        (Kind::Mp3, Quality::Top) => 320,
        (_, Quality::Std) => 112,
        (_, Quality::Hi) => 192,
        (_, Quality::Top) => 320,
    }
}

/// What a format can hold, from what the source has.
pub struct Shape {
    pub rate: u32,
    pub channels: usize,
    pub bits: u32,
}

const MP3_RATES: [u32; 9] = [8000, 11025, 12000, 16000, 22050, 24000, 32000, 44100, 48000];

pub fn shape_for(kind: Kind, rate: u32, channels: usize, bits: Option<u32>) -> Shape {
    let bits = match bits {
        Some(b) if b > 16 => 24,
        _ => 16,
    };
    match kind {
        Kind::Mp3 => Shape {
            rate: MP3_RATES.iter().copied().find(|r| *r >= rate).unwrap_or(48000),
            channels: channels.min(2),
            bits,
        },
        Kind::Ogg => Shape { rate: rate.clamp(8000, 192000), channels: channels.min(8), bits },
        Kind::Flac => Shape { rate, channels: channels.min(8), bits },
        Kind::Wav | Kind::Aiff => Shape { rate, channels, bits },
        // never written; this is only so the question has an answer
        Kind::M4a | Kind::Mod | Kind::Xm | Kind::S3m | Kind::It | Kind::Au | Kind::Voc => {
            Shape { rate, channels, bits }
        }
    }
}

pub enum Encoder {
    Pcm(PcmWriter),
    Flac(FlacEncoder),
    Mp3(Mp3Writer),
    Vorbis(VorbisEncoder<Box<dyn Out>>),
}

impl Encoder {
    pub fn new(kind: Kind, out: Box<dyn Out>, shape: &Shape, quality: Quality, frames: u64) -> Result<Self, String> {
        Ok(match kind {
            Kind::Wav => Encoder::Pcm(PcmWriter::new(out, shape, false, frames)?),
            Kind::Aiff => Encoder::Pcm(PcmWriter::new(out, shape, true, frames)?),
            Kind::Flac => Encoder::Flac(FlacEncoder::new(out, shape.channels, shape.rate, shape.bits, frames)?),
            Kind::Mp3 => Encoder::Mp3(Mp3Writer::new(out, shape, kbps(kind, quality))?),
            Kind::Ogg => Encoder::Vorbis(vorbis(out, shape, kbps(kind, quality))?),
            Kind::M4a | Kind::Mod | Kind::Xm | Kind::S3m | Kind::It | Kind::Au | Kind::Voc => {
                return Err("this format is only ever read".into());
            }
        })
    }
    pub fn write(&mut self, planar: &[Vec<f32>], n: usize) -> Result<(), String> {
        match self {
            Encoder::Pcm(w) => w.write(planar, n),
            Encoder::Flac(w) => w.write(planar, n),
            Encoder::Mp3(w) => w.write(planar, n),
            Encoder::Vorbis(w) => {
                let part: Vec<&[f32]> = planar.iter().map(|p| &p[..n]).collect();
                w.encode_audio_block(&part).map_err(|e| e.to_string())
            }
        }
    }
    pub fn finish(self) -> Result<(), String> {
        match self {
            Encoder::Pcm(w) => w.finish(),
            Encoder::Flac(w) => w.finish(),
            Encoder::Mp3(w) => w.finish(),
            Encoder::Vorbis(w) => w.finish().map_err(|e| e.to_string())?.flush().map_err(|e| e.to_string()),
        }
    }
}

// ---------- WAV and AIFF: plain samples ----------

pub struct PcmWriter {
    out: Box<dyn Out>,
    big_endian: bool,
    channels: usize,
    bits: u32,
    written: u64,
    buf: Vec<u8>,
    data_at: u64,
    extensible: bool,
}

fn aiff_rate(rate: u32) -> [u8; 10] {
    // the sample rate as an 80-bit float
    let mut out = [0u8; 10];
    let (mut e, mut m) = (0u16, rate as u64);
    while m >> 63 == 0 {
        m <<= 1;
        e += 1;
    }
    let exp = 16383 + 63 - e;
    out[..2].copy_from_slice(&exp.to_be_bytes());
    out[2..].copy_from_slice(&m.to_be_bytes());
    out
}

impl PcmWriter {
    fn new(mut out: Box<dyn Out>, shape: &Shape, big_endian: bool, frames: u64) -> Result<Self, String> {
        let block = shape.channels as u64 * (shape.bits as u64 / 8);
        let bytes = frames * block;
        // these formats count their bytes in 32 bits
        if bytes > u32::MAX as u64 - 1024 {
            return Err("too large".into());
        }
        let (extensible, data_at);
        if big_endian {
            extensible = false;
            let mut h = Vec::with_capacity(54);
            h.extend_from_slice(b"FORM\0\0\0\0AIFFCOMM");
            h.extend_from_slice(&18u32.to_be_bytes());
            h.extend_from_slice(&(shape.channels as u16).to_be_bytes());
            h.extend_from_slice(&0u32.to_be_bytes());
            h.extend_from_slice(&(shape.bits as u16).to_be_bytes());
            h.extend_from_slice(&aiff_rate(shape.rate));
            h.extend_from_slice(b"SSND\0\0\0\0\0\0\0\0\0\0\0\0");
            out.write_all(&h).map_err(|e| e.to_string())?;
            data_at = h.len() as u64;
        } else {
            // more than two channels have to say which is which
            extensible = shape.channels > 2;
            let mut h = Vec::with_capacity(68);
            h.extend_from_slice(b"RIFF\0\0\0\0WAVEfmt ");
            h.extend_from_slice(&(if extensible { 40u32 } else { 16 }).to_le_bytes());
            h.extend_from_slice(&(if extensible { 0xFFFEu16 } else { 1 }).to_le_bytes());
            h.extend_from_slice(&(shape.channels as u16).to_le_bytes());
            h.extend_from_slice(&shape.rate.to_le_bytes());
            h.extend_from_slice(&(shape.rate * block as u32).to_le_bytes());
            h.extend_from_slice(&(block as u16).to_le_bytes());
            h.extend_from_slice(&(shape.bits as u16).to_le_bytes());
            if extensible {
                h.extend_from_slice(&22u16.to_le_bytes());
                h.extend_from_slice(&(shape.bits as u16).to_le_bytes());
                h.extend_from_slice(&((1u32 << shape.channels.min(18)) - 1).to_le_bytes());
                h.extend_from_slice(&[1, 0, 0, 0, 0, 0, 0x10, 0, 0x80, 0, 0, 0xAA, 0, 0x38, 0x9B, 0x71]);
            }
            h.extend_from_slice(b"data\0\0\0\0");
            out.write_all(&h).map_err(|e| e.to_string())?;
            data_at = h.len() as u64;
        }
        Ok(PcmWriter {
            out,
            big_endian,
            channels: shape.channels,
            bits: shape.bits,
            written: 0,
            buf: Vec::new(),
            data_at,
            extensible,
        })
    }

    fn write(&mut self, planar: &[Vec<f32>], n: usize) -> Result<(), String> {
        self.buf.clear();
        let scale = (1u64 << (self.bits - 1)) as f32;
        let max = scale - 1.0;
        for f in 0..n {
            for p in planar.iter().take(self.channels) {
                let v = (p[f] as f64 * scale as f64).round().clamp(-(scale as f64), max as f64) as i32;
                let le = v.to_le_bytes();
                match (self.bits, self.big_endian) {
                    (16, false) => self.buf.extend_from_slice(&le[..2]),
                    (16, true) => self.buf.extend_from_slice(&[le[1], le[0]]),
                    (_, false) => self.buf.extend_from_slice(&le[..3]),
                    (_, true) => self.buf.extend_from_slice(&[le[2], le[1], le[0]]),
                }
            }
        }
        self.out.write_all(&self.buf).map_err(|e| e.to_string())?;
        self.written += self.buf.len() as u64;
        Ok(())
    }

    fn finish(mut self) -> Result<(), String> {
        let bytes = self.written;
        let e = |e: std::io::Error| e.to_string();
        if self.big_endian {
            if bytes & 1 == 1 {
                self.out.write_all(&[0]).map_err(e)?;
            }
            let frames = bytes / (self.channels as u64 * self.bits as u64 / 8);
            let form = 4 + 8 + 18 + 8 + 8 + bytes + (bytes & 1);
            self.out.seek(SeekFrom::Start(4)).map_err(e)?;
            self.out.write_all(&(form as u32).to_be_bytes()).map_err(e)?;
            self.out.seek(SeekFrom::Start(22)).map_err(e)?;
            self.out.write_all(&(frames as u32).to_be_bytes()).map_err(e)?;
            self.out.seek(SeekFrom::Start(42)).map_err(e)?;
            self.out.write_all(&((bytes + 8) as u32).to_be_bytes()).map_err(e)?;
        } else {
            if bytes & 1 == 1 {
                self.out.write_all(&[0]).map_err(e)?;
            }
            let riff = self.data_at - 8 + bytes + (bytes & 1);
            self.out.seek(SeekFrom::Start(4)).map_err(e)?;
            self.out.write_all(&(riff as u32).to_le_bytes()).map_err(e)?;
            self.out.seek(SeekFrom::Start(self.data_at - 4)).map_err(e)?;
            self.out.write_all(&(bytes as u32).to_le_bytes()).map_err(e)?;
        }
        let _ = self.extensible;
        self.out.seek(SeekFrom::End(0)).map_err(e)?;
        self.out.flush().map_err(e)
    }
}

// ---------- MP3 ----------

pub struct Mp3Writer {
    lame: &'static lame::Lame,
    handle: lame::Handle,
    out: Box<dyn Out>,
    channels: usize,
    buf: Vec<u8>,
    clip: Vec<Vec<f32>>,
}

unsafe impl Send for Mp3Writer {}

impl Mp3Writer {
    fn new(out: Box<dyn Out>, shape: &Shape, kbps: u32) -> Result<Self, String> {
        let l = lame::get()?;
        // SAFETY: LAME's documented order: init, set, init_params
        let handle = unsafe {
            let h = (l.init)();
            if h.is_null() {
                return Err("LAME init".into());
            }
            (l.set_in_rate)(h, shape.rate as i32);
            (l.set_out_rate)(h, shape.rate as i32);
            (l.set_channels)(h, shape.channels as i32);
            (l.set_mode)(h, if shape.channels == 1 { lame::MODE_MONO } else { lame::MODE_JOINT });
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
        Ok(Mp3Writer {
            lame: l,
            handle,
            out,
            channels: shape.channels,
            buf: Vec::new(),
            clip: vec![Vec::new(); shape.channels],
        })
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

// ---------- Vorbis ----------

fn vorbis(out: Box<dyn Out>, shape: &Shape, kbps: u32) -> Result<VorbisEncoder<Box<dyn Out>>, String> {
    let rate = NonZeroU32::new(shape.rate).ok_or("rate")?;
    let channels = NonZeroU8::new(shape.channels as u8).ok_or("channels")?;
    let aim = NonZeroU32::new(kbps * 1000).unwrap();
    // a very low rate or one channel can't reach every bitrate; then the quality setting that comes closest is what was asked for.
    // Building takes the output away even when it fails, so the question is first put to one that goes nowhere
    let mut asked = VorbisBitrateManagementStrategy::Vbr { target_bitrate: aim };
    let can = VorbisEncoderBuilder::new(rate, channels, std::io::sink())
        .map_err(|e| e.to_string())?
        .bitrate_management_strategy(asked)
        .build()
        .is_ok();
    if !can {
        let target_quality = match kbps {
            0..=128 => 0.3,
            129..=200 => 0.6,
            _ => 1.0,
        };
        asked = VorbisBitrateManagementStrategy::QualityVbr { target_quality };
    }
    VorbisEncoderBuilder::new(rate, channels, out)
        .map_err(|e| e.to_string())?
        .bitrate_management_strategy(asked)
        .build()
        .map_err(|e| e.to_string())
}
