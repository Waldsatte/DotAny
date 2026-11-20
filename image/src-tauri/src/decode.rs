//! Opening a file: from its bytes to upright 8-bit RGBA, one slot per page or size, moving pictures frame by frame.
//! Every format is read here, never by the browser, so what is shown and what is saved come from the same pixels.

use crate::colour::Colour;
use crate::exif::{self, Info};
use crate::formats::{self, Kind, Sniff};
use crate::metadata::{self, Meta};
use crate::ops::{self, Geo, Image8, Rect};
use crate::store::FrameStore;
use crate::svg::{self, Svg};
use image::ImageDecoder;
use serde::Serialize;
use std::collections::HashMap;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

#[derive(Debug, PartialEq, Clone, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OpenError {
    Refused,
    Damaged,
    TooLarge,
}

pub enum Body {
    Still(Arc<Image8>),
    /// A TIFF page, decoded when it is looked at.
    Page(u32),
    Frames(Arc<FrameStore>),
    /// An SVG draws itself in the page.
    Drawing,
}

pub struct Slot {
    pub w: u32,
    pub h: u32,
    pub alpha: bool,
    /// Had more than 8 bits per channel.
    pub deep: bool,
    pub body: Body,
}

pub struct Picture {
    pub kind: Kind,
    pub source: Arc<Vec<u8>>,
    pub meta: Meta,
    /// The EXIF orientation the stored pixels were turned by to become upright.
    pub orientation: u8,
    pub info: Info,
    pub slots: Vec<Slot>,
    pub svg: Option<Svg>,
    /// A PNG with more frames than we show.
    pub hidden_frames: bool,
    pub colour: Option<Colour>,
    /// A TIFF's bytes as its reader will take them, when that differs from the file.
    readable_tiff: Option<Vec<u8>>,
    pages: Mutex<Vec<(u32, Arc<Image8>)>>,
    views: Mutex<HashMap<(usize, u32), Arc<Image8>>>,
}

struct Shared(Arc<Vec<u8>>);
impl AsRef<[u8]> for Shared {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

fn enough_memory(w: u32, h: u32) -> bool {
    let need = w as u64 * h as u64 * 4;
    need < (1u64 << 40) && need + need / 4 < crate::memory::available()
}

fn opaque_rgba(rgb: &[u8]) -> Vec<u8> {
    let mut out = vec![255u8; rgb.len() / 3 * 4];
    for (d, s) in out.chunks_exact_mut(4).zip(rgb.chunks_exact(3)) {
        d[..3].copy_from_slice(s);
    }
    out
}

fn gray_rgba(g: &[u8]) -> Vec<u8> {
    let mut out = vec![255u8; g.len() * 4];
    for (d, s) in out.chunks_exact_mut(4).zip(g.iter()) {
        d[0] = *s;
        d[1] = *s;
        d[2] = *s;
    }
    out
}

fn upright(rgba: Vec<u8>, w: u32, h: u32, orientation: u8) -> Image8 {
    if orientation <= 1 || orientation > 8 {
        return Image8 { w, h, rgba };
    }
    let geo = Geo::from_orientation(orientation);
    let (nw, nh) = geo.size(w, h);
    ops::place(&rgba, w, h, geo, Rect { x: 0, y: 0, w: nw, h: nh })
}

/// image's decoders all hand back the same few layouts.
fn from_layout(buf: Vec<u8>, c: image::ColorType) -> Option<(Vec<u8>, bool)> {
    use image::ColorType as C;
    let to8 =
        |b: &[u8]| -> Vec<u8> { b.chunks_exact(2).map(|p| (u16::from_ne_bytes([p[0], p[1]]) >> 8) as u8).collect() };
    Some(match c {
        C::Rgba8 => (buf, false),
        C::Rgb8 => (opaque_rgba(&buf), false),
        C::L8 => (gray_rgba(&buf), false),
        C::La8 => {
            let mut out = vec![0u8; buf.len() * 2];
            for (d, s) in out.chunks_exact_mut(4).zip(buf.chunks_exact(2)) {
                d[0] = s[0];
                d[1] = s[0];
                d[2] = s[0];
                d[3] = s[1];
            }
            (out, false)
        }
        C::Rgba16 => (to8(&buf), true),
        C::Rgb16 => (opaque_rgba(&to8(&buf)), true),
        C::L16 => (gray_rgba(&to8(&buf)), true),
        C::La16 => {
            let b = to8(&buf);
            let mut out = vec![0u8; b.len() * 2];
            for (d, s) in out.chunks_exact_mut(4).zip(b.chunks_exact(2)) {
                d[0] = s[0];
                d[1] = s[0];
                d[2] = s[0];
                d[3] = s[1];
            }
            (out, true)
        }
        _ => return None,
    })
}

fn read_with<D: ImageDecoder>(dec: D) -> Result<(Vec<u8>, u32, u32, bool), OpenError> {
    let (w, h) = dec.dimensions();
    if w == 0 || h == 0 {
        return Err(OpenError::Damaged);
    }
    if !enough_memory(w, h) {
        return Err(OpenError::TooLarge);
    }
    let c = dec.color_type();
    let size = dec.total_bytes();
    if size > (1 << 40) {
        return Err(OpenError::TooLarge);
    }
    let mut buf = vec![0u8; size as usize];
    dec.read_image(&mut buf).map_err(|_| OpenError::Damaged)?;
    let (rgba, deep) = from_layout(buf, c).ok_or(OpenError::Damaged)?;
    Ok((rgba, w, h, deep))
}

fn still(rgba: Vec<u8>, w: u32, h: u32, deep: bool, orientation: u8) -> Slot {
    let img = upright(rgba, w, h, orientation);
    Slot { w: img.w, h: img.h, alpha: img.has_alpha(), deep, body: Body::Still(Arc::new(img)) }
}

// ---------- JPEG, PNG, BMP ----------

fn read_jpeg(b: &[u8]) -> Result<(Vec<u8>, u32, u32, bool), OpenError> {
    let mut d = image::codecs::jpeg::JpegDecoder::new(Cursor::new(b)).map_err(|_| OpenError::Damaged)?;
    let _ = d.set_limits(image::Limits::no_limits());
    read_with(d)
}

pub fn read_png(b: &[u8]) -> Result<(Vec<u8>, u32, u32, bool), OpenError> {
    let d = image::codecs::png::PngDecoder::with_limits(Cursor::new(b), image::Limits::no_limits())
        .map_err(|_| OpenError::Damaged)?;
    read_with(d)
}

fn read_bmp(b: &[u8]) -> Result<(Vec<u8>, u32, u32, bool), OpenError> {
    let d = image::codecs::bmp::BmpDecoder::new(Cursor::new(b)).map_err(|_| OpenError::Damaged)?;
    read_with(d)
}

// ---------- the formats only read: TGA, QOI, Netpbm, farbfeld (image's readers), PCX and ILBM (ours) ----------

fn read_old(kind: Kind, b: &[u8]) -> Result<(Vec<u8>, u32, u32, bool), OpenError> {
    use image::codecs as c;
    let damaged = |_| OpenError::Damaged;
    match kind {
        Kind::Tga => read_with(c::tga::TgaDecoder::new(Cursor::new(b)).map_err(damaged)?),
        Kind::Qoi => read_with(c::qoi::QoiDecoder::new(Cursor::new(b)).map_err(damaged)?),
        Kind::Pbm | Kind::Pgm | Kind::Ppm | Kind::Pam => {
            read_with(c::pnm::PnmDecoder::new(Cursor::new(b)).map_err(damaged)?)
        }
        Kind::Ff => read_with(c::farbfeld::FarbfeldDecoder::new(Cursor::new(b)).map_err(damaged)?),
        Kind::Pcx | Kind::Ilbm => {
            let fits = |w: u32, h: u32| enough_memory(w, h);
            let found = if kind == Kind::Pcx {
                crate::pcx::decode(b, &fits).map(|p| (p.rgba, p.w, p.h))
            } else {
                crate::ilbm::decode(b, &fits).map(|p| (p.rgba, p.w, p.h))
            };
            match found {
                Ok((rgba, w, h)) => Ok((rgba, w, h, false)),
                Err(crate::pcx::Problem::TooLarge) => Err(OpenError::TooLarge),
                Err(crate::pcx::Problem::Damaged) => Err(OpenError::Damaged),
            }
        }
        _ => Err(OpenError::Damaged),
    }
}

// ---------- ICO: one slot per size, the best of each ----------

fn read_ico(b: &[u8]) -> Result<Vec<Slot>, OpenError> {
    let n = u16::from_le_bytes([b[4], b[5]]) as usize;
    if n == 0 {
        return Err(OpenError::Damaged);
    }
    let mut seen: Vec<(u32, u32, u64, Vec<u8>, bool)> = Vec::new();
    for i in 0..n {
        let at = 6 + i * 16;
        let e = b.get(at..at + 16).ok_or(OpenError::Damaged)?;
        let len = u32::from_le_bytes([e[8], e[9], e[10], e[11]]) as usize;
        let off = u32::from_le_bytes([e[12], e[13], e[14], e[15]]) as usize;
        let Some(data) = off.checked_add(len).and_then(|z| b.get(off..z)) else { continue };
        // a one-size icon, handed to the icon reader
        let mut one = Vec::with_capacity(22 + len);
        one.extend_from_slice(&[0, 0, 1, 0, 1, 0]);
        one.extend_from_slice(&e[..12]);
        one.extend_from_slice(&22u32.to_le_bytes());
        one.extend_from_slice(data);
        let Ok(d) = image::codecs::ico::IcoDecoder::new(Cursor::new(&one)) else { continue };
        let Ok((rgba, w, h, deep)) = read_with(d) else { continue };
        let bits = u16::from_le_bytes([e[6], e[7]]) as u64;
        match seen.iter_mut().find(|s| s.0 == w && s.1 == h) {
            Some(s) if s.2 >= bits => {}
            Some(s) => *s = (w, h, bits, rgba, deep),
            None => seen.push((w, h, bits, rgba, deep)),
        }
    }
    if seen.is_empty() {
        return Err(OpenError::Damaged);
    }
    seen.sort_by_key(|s| std::cmp::Reverse(s.0));
    Ok(seen.into_iter().map(|(w, h, _, rgba, deep)| still(rgba, w, h, deep, 1)).collect())
}

// ---------- TIFF: pages are decoded when looked at ----------

fn tiff_decoder(source: &[u8]) -> Result<tiff::decoder::Decoder<Cursor<&[u8]>>, OpenError> {
    tiff::decoder::Decoder::new(Cursor::new(source))
        .map(|d| d.with_limits(tiff::decoder::Limits::unlimited()))
        .map_err(|_| OpenError::Damaged)
}

fn to_u8(result: tiff::decoder::DecodingResult) -> Option<(Vec<u8>, bool)> {
    use tiff::decoder::DecodingResult as R;
    Some(match result {
        R::U8(v) => (v, false),
        R::U16(v) => (v.iter().map(|x| (x >> 8) as u8).collect(), true),
        R::U32(v) => (v.iter().map(|x| (x >> 24) as u8).collect(), true),
        R::F32(v) => (v.iter().map(|x| (x.clamp(0.0, 1.0) * 255.0 + 0.5) as u8).collect(), true),
        _ => return None,
    })
}

/// One page, upright.
fn tiff_page(source: &[u8], index: u32) -> Result<(Image8, bool), OpenError> {
    use tiff::ColorType as C;
    use tiff::tags::Tag;
    let mut d = tiff_decoder(source)?;
    d.seek_to_image(index as usize).map_err(|_| OpenError::Damaged)?;
    let (w, h) = d.dimensions().map_err(|_| OpenError::Damaged)?;
    if w == 0 || h == 0 {
        return Err(OpenError::Damaged);
    }
    if !enough_memory(w, h) {
        return Err(OpenError::TooLarge);
    }
    let colour = d.colortype().map_err(|_| OpenError::Damaged)?;
    let orientation = d.find_tag(Tag::Orientation).ok().flatten().and_then(|v| v.into_u16().ok()).unwrap_or(1) as u8;
    let palette: Option<Vec<u16>> = d.find_tag(Tag::ColorMap).ok().flatten().and_then(|v| v.into_u16_vec().ok());
    let (data, deep) = to_u8(d.read_image().map_err(|_| OpenError::Damaged)?).ok_or(OpenError::Damaged)?;
    let n = w as usize * h as usize;
    let rgba = match colour {
        C::RGBA(8) | C::RGBA(16) | C::RGBA(32) if data.len() >= n * 4 => {
            let mut v = data;
            v.truncate(n * 4);
            v
        }
        C::RGB(8) | C::RGB(16) | C::RGB(32) | C::YCbCr(8) if data.len() >= n * 3 => opaque_rgba(&data[..n * 3]),
        C::Gray(8) | C::Gray(16) | C::Gray(32) if data.len() >= n && palette.is_none() => gray_rgba(&data[..n]),
        C::GrayA(8) | C::GrayA(16) if data.len() >= n * 2 => {
            let mut out = vec![0u8; n * 4];
            for (o, s) in out.chunks_exact_mut(4).zip(data.chunks_exact(2)) {
                o[0] = s[0];
                o[1] = s[0];
                o[2] = s[0];
                o[3] = s[1];
            }
            out
        }
        C::CMYK(8) if data.len() >= n * 4 => {
            let mut out = vec![255u8; n * 4];
            for (o, s) in out.chunks_exact_mut(4).zip(data.chunks_exact(4)) {
                let k = 255 - s[3] as u32;
                for i in 0..3 {
                    o[i] = ((255 - s[i] as u32) * k / 255) as u8;
                }
            }
            out
        }
        C::Gray(bits @ (1 | 2 | 4 | 8)) => {
            // packed into bytes, each row starting on a byte
            let (bits, is_palette) = (bits as usize, palette.is_some());
            let row = (w as usize * bits).div_ceil(8);
            if data.len() < row * h as usize {
                return Err(OpenError::Damaged);
            }
            let colours = 1usize << bits;
            let top = (colours - 1) as u32;
            let mut out = vec![255u8; n * 4];
            for y in 0..h as usize {
                for x in 0..w as usize {
                    let bit = x * bits;
                    let v = ((data[y * row + bit / 8] as usize) >> (8 - bits - bit % 8)) & (colours - 1);
                    let o = &mut out[(y * w as usize + x) * 4..][..4];
                    if is_palette {
                        let map = palette.as_ref().ok_or(OpenError::Damaged)?;
                        let at = |c: usize| map.get(c * colours + v).map(|s| (s >> 8) as u8).unwrap_or(0);
                        o[0] = at(0);
                        o[1] = at(1);
                        o[2] = at(2);
                    } else {
                        let g = (v as u32 * 255 / top) as u8;
                        o[0] = g;
                        o[1] = g;
                        o[2] = g;
                    }
                }
            }
            out
        }
        _ => return Err(OpenError::Damaged),
    };
    Ok((upright(rgba, w, h, orientation), deep))
}

fn read_tiff_pages(source: &[u8]) -> Result<Vec<Slot>, OpenError> {
    use tiff::tags::Tag;
    let mut d = tiff_decoder(source)?;
    let mut slots = Vec::new();
    let mut index = 0u32;
    // a file can claim endless pages; nobody has this many
    while index < 10_000 {
        let (w, h) = d.dimensions().map_err(|_| OpenError::Damaged)?;
        let orientation =
            d.find_tag(Tag::Orientation).ok().flatten().and_then(|v| v.into_u16().ok()).unwrap_or(1) as u8;
        let (w, h) = Geo::from_orientation(orientation).size(w, h);
        let colour = d.colortype().map_err(|_| OpenError::Damaged)?;
        let alpha = matches!(colour, tiff::ColorType::RGBA(_) | tiff::ColorType::GrayA(_));
        let deep = colour.bit_depth() > 8;
        slots.push(Slot { w, h, alpha, deep, body: Body::Page(index) });
        if !d.more_images() {
            break;
        }
        if d.next_image().is_err() {
            break;
        }
        index += 1;
    }
    if slots.is_empty() {
        return Err(OpenError::Damaged);
    }
    // the first page has to be readable, or the file is damaged
    let (first, deep) = tiff_page(source, 0)?;
    slots[0].deep = deep;
    slots[0].alpha = first.has_alpha();
    slots[0].body = if slots.len() == 1 { Body::Still(Arc::new(first)) } else { Body::Page(0) };
    Ok(slots)
}

// ---------- GIF ----------

fn gif_decoder(data: Shared) -> Result<gif::Decoder<Cursor<Shared>>, OpenError> {
    let mut o = gif::DecodeOptions::new();
    o.set_color_output(gif::ColorOutput::RGBA);
    o.set_memory_limit(gif::MemoryLimit::Bytes(std::num::NonZeroU64::new(1 << 32).unwrap()));
    o.check_frame_consistency(false);
    o.read_info(Cursor::new(data)).map_err(|_| OpenError::Damaged)
}

/// Paints a frame on the canvas, and returns what to do before the next one.
struct GifCanvas {
    w: usize,
    h: usize,
    pix: Vec<u8>,
    undo: Option<(gif::DisposalMethod, usize, usize, usize, usize, Option<Vec<u8>>)>,
}

impl GifCanvas {
    fn paint(&mut self, f: &gif::Frame) {
        if let Some((how, l, t, fw, fh, saved)) = self.undo.take() {
            match how {
                gif::DisposalMethod::Background => {
                    for y in t..(t + fh).min(self.h) {
                        let a = (y * self.w + l) * 4;
                        let z = (y * self.w + (l + fw).min(self.w)) * 4;
                        self.pix[a..z].fill(0);
                    }
                }
                gif::DisposalMethod::Previous => {
                    if let Some(s) = saved {
                        self.pix = s;
                    }
                }
                _ => {}
            }
        }
        let (l, t, fw, fh) = (f.left as usize, f.top as usize, f.width as usize, f.height as usize);
        let saved = if f.dispose == gif::DisposalMethod::Previous { Some(self.pix.clone()) } else { None };
        self.undo = Some((f.dispose, l, t, fw, fh, saved));
        for y in 0..fh {
            if t + y >= self.h {
                break;
            }
            let src = &f.buffer[y * fw * 4..(y + 1) * fw * 4];
            let width = fw.min(self.w.saturating_sub(l));
            let dst = &mut self.pix[((t + y) * self.w + l) * 4..((t + y) * self.w + l + width) * 4];
            for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
                if s[3] != 0 {
                    d.copy_from_slice(s);
                }
            }
        }
    }
}

fn read_gif(source: &Arc<Vec<u8>>, temp: &Path) -> Result<Vec<Slot>, OpenError> {
    let mut d = gif_decoder(Shared(source.clone()))?;
    let (w, h) = (d.width() as u32, d.height() as u32);
    if w == 0 || h == 0 {
        return Err(OpenError::Damaged);
    }
    if !enough_memory(w, h) {
        return Err(OpenError::TooLarge);
    }
    let mut canvas = GifCanvas { w: w as usize, h: h as usize, pix: vec![0; w as usize * h as usize * 4], undo: None };
    let delay_of = |f: &gif::Frame| f.delay as u32 * 10;
    let first = d.read_next_frame().map_err(|_| OpenError::Damaged)?.ok_or(OpenError::Damaged)?;
    let d0 = delay_of(first);
    canvas.paint(first);
    let first_pix = canvas.pix.clone();
    let second = match d.read_next_frame() {
        Ok(Some(f)) => {
            let delay = delay_of(f);
            canvas.paint(f);
            Some(delay)
        }
        _ => None,
    };
    let Some(d1) = second else { return Ok(vec![still(first_pix, w, h, false, 1)]) };
    let loops = match d.repeat() {
        gif::Repeat::Infinite => 0,
        gif::Repeat::Finite(n) => n,
    };
    let store = FrameStore::new(w, h, loops, temp);
    store.push(&first_pix, d0).map_err(|_| OpenError::Damaged)?;
    store.push(&canvas.pix, d1).map_err(|_| OpenError::Damaged)?;
    let worker = store.clone();
    std::thread::spawn(move || {
        let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            loop {
                if worker.stop.load(Ordering::Relaxed) {
                    return true;
                }
                match d.read_next_frame() {
                    Ok(Some(f)) => {
                        let delay = f.delay as u32 * 10;
                        canvas.paint(f);
                        if worker.push(&canvas.pix, delay).is_err() {
                            return false;
                        }
                    }
                    Ok(None) => return true,
                    // the frames before the damage still play
                    Err(_) => return true,
                }
            }
        }));
        worker.finish(!matches!(run, Ok(true)));
    });
    let alpha = store.alpha.load(Ordering::Relaxed);
    Ok(vec![Slot { w, h, alpha, deep: false, body: Body::Frames(store) }])
}

// ---------- WebP ----------

fn webp_still(b: &[u8]) -> Result<(Vec<u8>, u32, u32), OpenError> {
    use libwebp_sys as w;
    let (mut width, mut height) = (0i32, 0i32);
    unsafe {
        if w::WebPGetInfo(b.as_ptr(), b.len(), &mut width, &mut height) == 0 || width <= 0 || height <= 0 {
            return Err(OpenError::Damaged);
        }
        if !enough_memory(width as u32, height as u32) {
            return Err(OpenError::TooLarge);
        }
        let p = w::WebPDecodeRGBA(b.as_ptr(), b.len(), &mut width, &mut height);
        if p.is_null() {
            return Err(OpenError::Damaged);
        }
        let n = width as usize * height as usize * 4;
        let out = std::slice::from_raw_parts(p, n).to_vec();
        w::WebPFree(p as *mut _);
        Ok((out, width as u32, height as u32))
    }
}

struct AnimDecoder(*mut libwebp_sys::WebPAnimDecoder);
unsafe impl Send for AnimDecoder {}
impl Drop for AnimDecoder {
    fn drop(&mut self) {
        unsafe { libwebp_sys::WebPAnimDecoderDelete(self.0) }
    }
}

fn webp_moving(source: &Arc<Vec<u8>>, temp: &Path) -> Result<Vec<Slot>, OpenError> {
    use libwebp_sys as w;
    unsafe {
        let mut opts = std::mem::zeroed::<w::WebPAnimDecoderOptions>();
        if w::WebPAnimDecoderOptionsInitInternal(&mut opts, w::WEBP_DEMUX_ABI_VERSION as _) == 0 {
            return Err(OpenError::Damaged);
        }
        opts.color_mode = w::WEBP_CSP_MODE::MODE_RGBA;
        let data = w::WebPData { bytes: source.as_ptr(), size: source.len() };
        let dec = w::WebPAnimDecoderNewInternal(&data, &opts, w::WEBP_DEMUX_ABI_VERSION as _);
        if dec.is_null() {
            return Err(OpenError::Damaged);
        }
        let dec = AnimDecoder(dec);
        let mut info = std::mem::zeroed::<w::WebPAnimInfo>();
        if w::WebPAnimDecoderGetInfo(dec.0, &mut info) == 0 || info.frame_count == 0 {
            return Err(OpenError::Damaged);
        }
        let (cw, ch) = (info.canvas_width, info.canvas_height);
        if !enough_memory(cw, ch) {
            return Err(OpenError::TooLarge);
        }
        let n = cw as usize * ch as usize * 4;
        let store = FrameStore::new(cw, ch, info.loop_count.min(65535) as u16, temp);
        // the first frame now; the time that comes with each frame is when it ends
        let (mut buf, mut ts) = (std::ptr::null_mut::<u8>(), 0i32);
        if w::WebPAnimDecoderGetNext(dec.0, &mut buf, &mut ts) == 0 {
            return Err(OpenError::Damaged);
        }
        let first = std::slice::from_raw_parts(buf, n);
        if info.frame_count == 1 {
            return Ok(vec![still(first.to_vec(), cw, ch, false, 1)]);
        }
        store.push(first, ts.max(0) as u32).map_err(|_| OpenError::Damaged)?;
        let (keep, worker) = (source.clone(), store.clone());
        std::thread::spawn(move || {
            let _keep = keep;
            let dec = dec;
            let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut last = ts;
                while w::WebPAnimDecoderHasMoreFrames(dec.0) != 0 && !worker.stop.load(Ordering::Relaxed) {
                    let (mut b, mut t) = (std::ptr::null_mut::<u8>(), 0i32);
                    if w::WebPAnimDecoderGetNext(dec.0, &mut b, &mut t) == 0 {
                        break;
                    }
                    if worker.push(std::slice::from_raw_parts(b, n), (t - last).max(0) as u32).is_err() {
                        return false;
                    }
                    last = t;
                }
                true
            }));
            worker.finish(!matches!(run, Ok(true)));
        });
        let alpha = store.alpha.load(Ordering::Relaxed);
        Ok(vec![Slot { w: cw, h: ch, alpha, deep: false, body: Body::Frames(store) }])
    }
}

fn is_moving_webp(b: &[u8]) -> bool {
    metadata::webp_chunks(b).first().is_some_and(|(t, r)| t == b"VP8X" && b.get(r.start).is_some_and(|f| f & 0x02 != 0))
}

// ---------- all together ----------

impl Picture {
    /// `temp` is where a huge moving picture's frames may go.
    pub fn open(bytes: Vec<u8>, name: &str, temp: &Path) -> Result<Picture, OpenError> {
        let kind = match formats::sniff(&bytes, name) {
            Sniff::Is(k) => k,
            Sniff::Refused => return Err(OpenError::Refused),
            Sniff::Damaged => return Err(OpenError::Damaged),
        };
        let temp: PathBuf = temp.to_path_buf();
        // a decoder that trips over a corrupt file must not take the program down
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| Self::open_kind(kind, bytes, &temp)))
            .unwrap_or(Err(OpenError::Damaged))
    }

    fn open_kind(kind: Kind, bytes: Vec<u8>, temp: &Path) -> Result<Picture, OpenError> {
        let source = Arc::new(bytes);
        let b: &[u8] = &source;
        let meta = metadata::read(kind, b);
        let orientation = meta.exif.as_deref().map(exif::orientation).unwrap_or(1);
        let info = match kind {
            // a TIFF is its own EXIF block
            Kind::Tiff => exif::info(b),
            _ => meta.exif.as_deref().map(exif::info).unwrap_or_default(),
        };
        let readable_tiff = if kind == Kind::Tiff { exif::palette_read_as_grey(b) } else { None };
        let mut hidden_frames = false;
        let mut svg_doc = None;
        let slots = match kind {
            Kind::Jpg => {
                let (rgba, w, h, deep) = read_jpeg(b)?;
                vec![still(rgba, w, h, deep, orientation)]
            }
            Kind::Png => {
                hidden_frames = metadata::png_chunks(b).iter().any(|(t, _)| t == b"acTL");
                let (rgba, w, h, deep) = read_png(b)?;
                vec![still(rgba, w, h, deep, orientation)]
            }
            Kind::Bmp => {
                let (rgba, w, h, deep) = read_bmp(b)?;
                vec![still(rgba, w, h, deep, 1)]
            }
            Kind::Ico => read_ico(b)?,
            Kind::Tiff => read_tiff_pages(readable_tiff.as_deref().unwrap_or(b))?,
            Kind::Gif => read_gif(&source, temp)?,
            Kind::Webp => {
                if is_moving_webp(b) {
                    webp_moving(&source, temp)?
                } else {
                    let (rgba, w, h) = webp_still(b)?;
                    vec![still(rgba, w, h, false, orientation)]
                }
            }
            Kind::Tga
            | Kind::Qoi
            | Kind::Pbm
            | Kind::Pgm
            | Kind::Ppm
            | Kind::Pam
            | Kind::Ff
            | Kind::Pcx
            | Kind::Ilbm => {
                let (rgba, w, h, deep) = read_old(kind, b)?;
                vec![still(rgba, w, h, deep, 1)]
            }
            Kind::Svg => {
                let doc = svg::sanitize(b).ok_or(OpenError::Damaged)?;
                let (w, h) = (doc.w, doc.h);
                svg_doc = Some(doc);
                vec![Slot { w, h, alpha: true, deep: false, body: Body::Drawing }]
            }
        };
        let colour = meta.icc.as_deref().and_then(Colour::from_profile);
        Ok(Picture {
            kind,
            source,
            meta,
            orientation,
            info,
            slots,
            svg: svg_doc,
            hidden_frames,
            colour,
            readable_tiff,
            pages: Mutex::default(),
            views: Mutex::default(),
        })
    }

    pub fn is_moving(&self) -> bool {
        self.slots.len() == 1 && matches!(self.slots[0].body, Body::Frames(_))
    }

    pub fn frames(&self, slot: usize) -> Option<&Arc<FrameStore>> {
        match &self.slots.get(slot)?.body {
            Body::Frames(s) => Some(s),
            _ => None,
        }
    }

    /// A slot's full pixels: at hand, or decoded now and kept for the next look.
    pub fn pixels(&self, slot: usize, frame: usize) -> Option<Arc<Image8>> {
        let s = self.slots.get(slot)?;
        match &s.body {
            Body::Still(i) => Some(i.clone()),
            Body::Page(n) => {
                let mut pages = self.pages.lock().unwrap();
                if let Some(at) = pages.iter().position(|p| p.0 == *n) {
                    let hit = pages.remove(at);
                    pages.push(hit.clone());
                    return Some(hit.1);
                }
                drop(pages);
                let (img, _) = tiff_page(self.readable_tiff.as_deref().unwrap_or(&self.source), *n).ok()?;
                let img = Arc::new(img);
                let mut pages = self.pages.lock().unwrap();
                pages.push((*n, img.clone()));
                if pages.len() > 2 {
                    pages.remove(0);
                }
                Some(img)
            }
            Body::Frames(store) => {
                if !store.wait_for(frame) {
                    return None;
                }
                Some(Arc::new(Image8 { w: store.w, h: store.h, rgba: store.get(frame)? }))
            }
            Body::Drawing => None,
        }
    }

    /// The slot at no more than this longest side, for the screen. Stills are remembered at each size asked for.
    pub fn view(&self, slot: usize, frame: usize, max_side: u32) -> Option<Arc<Image8>> {
        let s = self.slots.get(slot)?;
        let (w, h) = ops::fit(s.w, s.h, max_side);
        let moving = matches!(s.body, Body::Frames(_));
        if !moving {
            if let Some(hit) = self.views.lock().unwrap().get(&(slot, w)) {
                return Some(hit.clone());
            }
        }
        let full = self.pixels(slot, frame)?;
        if full.w == w && full.h == h {
            return Some(full);
        }
        let small = Arc::new(ops::shrink_fast(&full.rgba, full.w, full.h, w, h));
        if !moving {
            let mut views = self.views.lock().unwrap();
            if views.len() > 4 {
                views.clear();
            }
            views.insert((slot, w), small.clone());
        }
        Some(small)
    }

    pub fn stop(&self) {
        for s in &self.slots {
            if let Body::Frames(f) = &s.body {
                f.stop.store(true, Ordering::Relaxed);
            }
        }
    }
}

impl Drop for Picture {
    fn drop(&mut self) {
        self.stop();
    }
}
