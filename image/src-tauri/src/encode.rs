//! From finished pixels to the bytes of each format. Nothing here decides anything: `export.rs` says what to write.

use crate::exif::{self, Tag, Tree};
use crate::metadata::{self, Meta};
use crate::ops::{self, Image8};
use rayon::prelude::*;
use std::io::Cursor;

pub type Res<T> = Result<T, String>;

// ---------- JPG ----------

pub fn jpeg(img: &Image8, quality: u8, m: &Meta) -> Res<Vec<u8>> {
    use jpeg_encoder::{ColorType, Encoder, SamplingFactor};
    if img.w > 65535 || img.h > 65535 {
        return Err("too large for JPG".into());
    }
    let rgb = ops::flatten_white(&img.rgba);
    let mut out = Vec::with_capacity(img.rgba.len() / 8);
    {
        let mut e = Encoder::new(&mut out, quality.clamp(1, 100));
        // at high quality colour keeps its full detail, the way a person choosing high quality expects
        e.set_sampling_factor(if quality >= 90 { SamplingFactor::F_1_1 } else { SamplingFactor::F_2_2 });
        e.set_optimized_huffman_tables(true);
        if let Some(x) = &m.exif {
            if x.len() + 8 <= u16::MAX as usize {
                e.add_app_segment(1, metadata::exif_payload(x)).map_err(|e| e.to_string())?;
            }
        }
        if let Some(x) = &m.xmp {
            if x.len() + 31 <= u16::MAX as usize {
                e.add_app_segment(1, metadata::xmp_payload(x)).map_err(|e| e.to_string())?;
            }
        }
        if let Some(i) = &m.icc {
            e.add_icc_profile(i).map_err(|e| e.to_string())?;
        }
        if let Some(x) = &m.iptc {
            if x.len() + 2 <= u16::MAX as usize && x.starts_with(b"Photoshop") {
                e.add_app_segment(13, x.clone()).map_err(|e| e.to_string())?;
            }
        }
        e.encode(&rgb, img.w as u16, img.h as u16, ColorType::Rgb).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

// ---------- PNG ----------

pub fn png(img: &Image8, m: &Meta, quick: bool) -> Res<Vec<u8>> {
    use image::codecs::png::{CompressionType, FilterType, PngEncoder};
    use image::{ExtendedColorType, ImageEncoder};
    let mut out = Vec::new();
    let how = if quick { CompressionType::Fast } else { CompressionType::Default };
    let enc = PngEncoder::new_with_quality(&mut out, how, FilterType::Adaptive);
    if img.has_alpha() {
        enc.write_image(&img.rgba, img.w, img.h, ExtendedColorType::Rgba8).map_err(|e| e.to_string())?;
    } else {
        enc.write_image(&ops::drop_alpha(&img.rgba), img.w, img.h, ExtendedColorType::Rgb8)
            .map_err(|e| e.to_string())?;
    }
    Ok(metadata::png_with(out, m))
}

// ---------- WebP ----------

fn webp_config(quality: u8, lossless: bool) -> Res<libwebp_sys::WebPConfig> {
    let mut c = libwebp_sys::WebPConfig::new().map_err(|_| "webp config".to_string())?;
    c.quality = if lossless { 75.0 } else { quality as f32 };
    c.lossless = lossless as i32;
    c.method = 4;
    c.exact = lossless as i32;
    Ok(c)
}

struct Picture(libwebp_sys::WebPPicture);
impl Picture {
    fn from(rgba: &[u8], w: u32, h: u32) -> Res<Picture> {
        let mut p = libwebp_sys::WebPPicture::new().map_err(|_| "webp picture".to_string())?;
        p.width = w as i32;
        p.height = h as i32;
        p.use_argb = 1;
        if unsafe { libwebp_sys::WebPPictureImportRGBA(&mut p, rgba.as_ptr(), w as i32 * 4) } == 0 {
            return Err("out of memory".into());
        }
        Ok(Picture(p))
    }
}
impl Drop for Picture {
    fn drop(&mut self) {
        unsafe { libwebp_sys::WebPPictureFree(&mut self.0) }
    }
}

pub fn webp(img: &Image8, quality: u8, lossless: bool, m: &Meta) -> Res<Vec<u8>> {
    use libwebp_sys as w;
    if img.w > 16383 || img.h > 16383 {
        return Err("too large for WebP".into());
    }
    let cfg = webp_config(quality, lossless)?;
    let mut pic = Picture::from(&img.rgba, img.w, img.h)?;
    unsafe {
        let mut writer = std::mem::zeroed::<w::WebPMemoryWriter>();
        w::WebPMemoryWriterInit(&mut writer);
        pic.0.writer = Some(w::WebPMemoryWrite);
        pic.0.custom_ptr = &mut writer as *mut _ as *mut _;
        let ok = w::WebPEncode(&cfg, &mut pic.0);
        let out = if ok != 0 {
            Ok(std::slice::from_raw_parts(writer.mem, writer.size).to_vec())
        } else {
            Err(format!("webp {:?}", pic.0.error_code))
        };
        w::WebPMemoryWriterClear(&mut writer);
        Ok(metadata::webp_with(&out?, m, img.w, img.h, img.has_alpha()))
    }
}

/// Frames come one at a time, so none has to wait in memory.
pub struct WebpAnim {
    enc: *mut libwebp_sys::WebPAnimEncoder,
    cfg: libwebp_sys::WebPConfig,
    at: i32,
    w: u32,
    h: u32,
    alpha: bool,
}

impl WebpAnim {
    pub fn new(w: u32, h: u32, loops: u16, quality: u8, lossless: bool) -> Res<WebpAnim> {
        use libwebp_sys as s;
        if w > 16383 || h > 16383 {
            return Err("too large for WebP".into());
        }
        unsafe {
            let mut opts = std::mem::zeroed::<s::WebPAnimEncoderOptions>();
            if s::WebPAnimEncoderOptionsInitInternal(&mut opts, s::WEBP_MUX_ABI_VERSION as _) == 0 {
                return Err("webp options".into());
            }
            opts.anim_params.loop_count = loops as i32;
            opts.anim_params.bgcolor = 0;
            opts.allow_mixed = (!lossless) as i32;
            let enc = s::WebPAnimEncoderNewInternal(w as i32, h as i32, &opts, s::WEBP_MUX_ABI_VERSION as _);
            if enc.is_null() {
                return Err("webp encoder".into());
            }
            Ok(WebpAnim { enc, cfg: webp_config(quality, lossless)?, at: 0, w, h, alpha: false })
        }
    }

    pub fn add(&mut self, rgba: &[u8], delay_ms: u32) -> Res<()> {
        self.alpha = self.alpha || ops::has_alpha(rgba);
        let mut pic = Picture::from(rgba, self.w, self.h)?;
        if unsafe { libwebp_sys::WebPAnimEncoderAdd(self.enc, &mut pic.0, self.at, &self.cfg) } == 0 {
            return Err("webp frame".into());
        }
        // a frame needs some length to be a frame
        self.at += delay_ms.max(10) as i32;
        Ok(())
    }

    pub fn finish(self, m: &Meta) -> Res<Vec<u8>> {
        use libwebp_sys as s;
        unsafe {
            if s::WebPAnimEncoderAdd(self.enc, std::ptr::null_mut(), self.at, std::ptr::null()) == 0 {
                return Err("webp end".into());
            }
            let mut data = s::WebPData::default();
            if s::WebPAnimEncoderAssemble(self.enc, &mut data) == 0 {
                return Err("webp assemble".into());
            }
            let raw = std::slice::from_raw_parts(data.bytes, data.size).to_vec();
            s::WebPFree(data.bytes as *mut _);
            Ok(metadata::webp_with(&raw, m, self.w, self.h, self.alpha))
        }
    }
}

impl Drop for WebpAnim {
    fn drop(&mut self) {
        unsafe { libwebp_sys::WebPAnimEncoderDelete(self.enc) }
    }
}

// ---------- GIF ----------

pub struct GifAnim {
    enc: gif::Encoder<Vec<u8>>,
    w: u16,
    h: u16,
    last: Option<Vec<u8>>,
}

impl GifAnim {
    pub fn new(w: u32, h: u32, loops: u16) -> Res<GifAnim> {
        if w > 65535 || h > 65535 {
            return Err("too large for GIF".into());
        }
        let mut enc = gif::Encoder::new(Vec::new(), w as u16, h as u16, &[]).map_err(|e| e.to_string())?;
        enc.set_repeat(if loops == 0 { gif::Repeat::Infinite } else { gif::Repeat::Finite(loops) })
            .map_err(|e| e.to_string())?;
        Ok(GifAnim { enc, w: w as u16, h: h as u16, last: None })
    }

    /// `single` leaves out the looping block's effect by being one frame. Opaque frames after the first store only what changed.
    pub fn add(&mut self, rgba: &[u8], delay_ms: u32) -> Res<()> {
        let (w, h) = (self.w as usize, self.h as usize);
        let opaque = !ops::has_alpha(rgba);
        let delay = (delay_ms / 10).min(65535) as u16;
        let mut frame = match (&self.last, opaque) {
            (Some(prev), true) => {
                let same = |x: usize, y: usize| rgba[(y * w + x) * 4..][..3] == prev[(y * w + x) * 4..][..3];
                let (mut l, mut r, mut t, mut b) = (w, 0usize, h, 0usize);
                for y in 0..h {
                    for x in 0..w {
                        if !same(x, y) {
                            l = l.min(x);
                            r = r.max(x + 1);
                            t = t.min(y);
                            b = b.max(y + 1);
                        }
                    }
                }
                if l >= r {
                    // nothing moved: one pixel to hold the time
                    (l, r, t, b) = (0, 1, 0, 1);
                }
                let (fw, fh) = (r - l, b - t);
                let mut part = Vec::with_capacity(fw * fh * 4);
                for y in t..b {
                    for x in l..r {
                        let at = (y * w + x) * 4;
                        if same(x, y) && fw * fh > 1 {
                            part.extend_from_slice(&[0, 0, 0, 0]);
                        } else {
                            part.extend_from_slice(&rgba[at..at + 4]);
                        }
                    }
                }
                let mut f = gif::Frame::from_rgba_speed(fw as u16, fh as u16, &mut part, 10);
                f.left = l as u16;
                f.top = t as u16;
                f.dispose = gif::DisposalMethod::Keep;
                f
            }
            _ => {
                let mut whole = rgba.to_vec();
                let mut f = gif::Frame::from_rgba_speed(self.w, self.h, &mut whole, 10);
                f.dispose = if opaque { gif::DisposalMethod::Keep } else { gif::DisposalMethod::Background };
                f
            }
        };
        frame.delay = delay;
        self.enc.write_frame(&frame).map_err(|e| e.to_string())?;
        self.last = if opaque { Some(rgba.to_vec()) } else { None };
        Ok(())
    }

    pub fn finish(self) -> Res<Vec<u8>> {
        self.enc.into_inner().map_err(|e| e.to_string())
    }
}

// ---------- BMP, ICO ----------

pub fn bmp(img: &Image8) -> Res<Vec<u8>> {
    use image::ExtendedColorType;
    use image::codecs::bmp::BmpEncoder;
    let mut out = Vec::new();
    let mut e = BmpEncoder::new(&mut out);
    if img.has_alpha() {
        e.encode(&img.rgba, img.w, img.h, ExtendedColorType::Rgba8).map_err(|e| e.to_string())?;
    } else {
        e.encode(&ops::drop_alpha(&img.rgba), img.w, img.h, ExtendedColorType::Rgb8).map_err(|e| e.to_string())?;
    }
    Ok(out)
}

pub fn ico(sizes: &[Image8]) -> Res<Vec<u8>> {
    use image::ExtendedColorType;
    use image::codecs::ico::{IcoEncoder, IcoFrame};
    let frames: Vec<IcoFrame> = sizes
        .iter()
        .map(|i| IcoFrame::as_png(&i.rgba, i.w, i.h, ExtendedColorType::Rgba8).map_err(|e| e.to_string()))
        .collect::<Res<_>>()?;
    let mut out = Vec::new();
    IcoEncoder::new(Cursor::new(&mut out)).encode_images(&frames).map_err(|e| e.to_string())?;
    Ok(out)
}

// ---------- TIFF: written by hand, because a page has to carry metadata the libraries can't write ----------

const ROWS_PER_STRIP: u32 = 64;

fn short(tag: u16, v: u16) -> Tag {
    Tag { tag, typ: 3, count: 1, value: v.to_le_bytes().to_vec() }
}
fn long(tag: u16, v: u32) -> Tag {
    Tag { tag, typ: 4, count: 1, value: v.to_le_bytes().to_vec() }
}
fn longs(tag: u16, v: &[u32]) -> Tag {
    Tag { tag, typ: 4, count: v.len() as u32, value: v.iter().flat_map(|x| x.to_le_bytes()).collect() }
}

fn lzw_strip(rows: &[u8], stride: usize, channels: usize) -> Vec<u8> {
    // horizontal differencing makes pictures compress far better, and every reader of LZW TIFFs knows it
    let mut diff = rows.to_vec();
    for line in diff.chunks_exact_mut(stride) {
        for i in (channels..stride).rev() {
            line[i] = line[i].wrapping_sub(line[i - channels]);
        }
    }
    let mut enc = weezl::encode::Encoder::with_tiff_size_switch(weezl::BitOrder::Msb, 8);
    enc.encode(&diff).unwrap_or_default()
}

fn tiff_page_bytes(out: &mut Vec<u8>, img: &Image8, m: Option<(&Meta, &Tree)>, next_slot: &mut usize) -> Res<()> {
    let alpha = img.has_alpha();
    let channels = if alpha { 4 } else { 3 };
    let pixels = if alpha { None } else { Some(ops::drop_alpha(&img.rgba)) };
    let data: &[u8] = pixels.as_deref().unwrap_or(&img.rgba);
    let stride = img.w as usize * channels;
    let strips: Vec<Vec<u8>> =
        data.par_chunks(stride * ROWS_PER_STRIP as usize).map(|rows| lzw_strip(rows, stride, channels)).collect();
    let mut offsets = Vec::with_capacity(strips.len());
    let mut counts = Vec::with_capacity(strips.len());
    for s in &strips {
        if out.len() % 2 == 1 {
            out.push(0);
        }
        if out.len() + s.len() > u32::MAX as usize - (64 << 20) {
            return Err("too large for TIFF".into());
        }
        offsets.push(out.len() as u32);
        counts.push(s.len() as u32);
        out.extend_from_slice(s);
    }
    let bits: Vec<u8> = (0..channels).flat_map(|_| 8u16.to_le_bytes()).collect();
    let mut tags = vec![
        long(256, img.w),
        long(257, img.h),
        Tag { tag: 258, typ: 3, count: channels as u32, value: bits },
        short(259, 5), // LZW
        short(262, 2), // RGB
        longs(273, &offsets),
        short(274, 1), // upright
        short(277, channels as u16),
        long(278, ROWS_PER_STRIP),
        longs(279, &counts),
        short(284, 1),
        short(317, 2), // horizontal differencing
    ];
    if alpha {
        tags.push(short(338, 2)); // straight alpha
    }
    if let Some((meta, tree)) = m {
        tags.extend(tree.ifd0.iter().cloned());
        if out.len() % 2 == 1 {
            out.push(0);
        }
        exif::place_subdirs(out, tree, &mut tags);
        if let Some(i) = &meta.icc {
            tags.push(Tag { tag: 34675, typ: 7, count: i.len() as u32, value: i.clone() });
        }
        if let Some(x) = &meta.xmp {
            tags.push(Tag { tag: 700, typ: 1, count: x.len() as u32, value: x.clone() });
        }
        if let Some(x) = &meta.iptc {
            if !x.starts_with(b"Photoshop") {
                tags.push(Tag { tag: 33723, typ: 7, count: x.len() as u32, value: x.clone() });
            }
        }
    }
    if out.len() % 2 == 1 {
        out.push(0);
    }
    let here = out.len() as u32;
    out[*next_slot..*next_slot + 4].copy_from_slice(&here.to_le_bytes());
    // the "next" pointer is written as zero and filled in if another page follows
    let count = tags.len();
    exif::write_ifd(out, &tags, 0);
    *next_slot = here as usize + 2 + count * 12;
    Ok(())
}

/// All the pages. Metadata goes with the first, the way readers look for it.
pub fn tiff(pages: impl Iterator<Item = Res<Image8>>, m: &Meta) -> Res<Vec<u8>> {
    let tree = m.exif.as_deref().and_then(exif::tree).unwrap_or_default();
    let mut out = vec![b'I', b'I', 42, 0, 0, 0, 0, 0];
    let mut next_slot = 4usize;
    for (i, page) in pages.enumerate() {
        let img = page?;
        let carried = if i == 0 { Some((m, &tree)) } else { None };
        tiff_page_bytes(&mut out, &img, carried, &mut next_slot)?;
    }
    Ok(out)
}
