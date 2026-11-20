//! Pictures: png, jpg, webp, tiff and bmp both ways; gif, ico, tga, qoi, Netpbm, farbfeld, pcx and IFF ILBM are only read.
//! What a file is (`formats`), what it carries besides pixels (`metadata`, `exif`), pixel work (`ops`) and the PCX and ILBM
//! readers (`pcx`, `ilbm`) come from the `dotany-picture` crate (`shared/picture`), which DotAny Image uses too. The
//! writers are in `encode`, beside this file; only the five formats above are written, and a moving picture is never
//! written. A picture is read upright, as Image shows it, and only its first frame or page is converted. The colour
//! profile, EXIF and XMP go across the way Image's Save As carries them.

#![allow(dead_code)]

mod encode;

use crate::out::Outputs;
use crate::{Options, Reason, Step};
use dotany_picture::{exif, formats, ilbm, metadata, ops, pcx};
use formats::{Kind, Sniff};
use image::ImageDecoder;
use metadata::Meta;
use ops::{Geo, Image8, Rect};
use std::io::Cursor;
use std::path::Path;
use std::sync::Mutex;

#[cfg(windows)]
fn available_memory() -> u64 {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    // SAFETY: a zeroed struct with its length set, as the call asks
    let mut s: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    s.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    if unsafe { GlobalMemoryStatusEx(&mut s) } == 0 {
        return u64::MAX;
    }
    s.ullAvailPhys
}

/// The Mac: its memory times the share of it the system counts as free (what `memory_pressure` calls the system-wide
/// memory free percentage, which counts what can be given back without paging out).
#[cfg(target_os = "macos")]
fn available_memory() -> u64 {
    fn sysctl<T: Default>(name: &std::ffi::CStr) -> Option<T> {
        let mut v = T::default();
        let mut len = std::mem::size_of::<T>();
        // SAFETY: a value of the size the name's value has, and its length
        let r =
            unsafe { libc::sysctlbyname(name.as_ptr(), &mut v as *mut T as *mut _, &mut len, std::ptr::null_mut(), 0) };
        (r == 0 && len == std::mem::size_of::<T>()).then_some(v)
    }
    match (sysctl::<u64>(c"hw.memsize"), sysctl::<i32>(c"kern.memorystatus_level")) {
        (Some(total), Some(pct)) if (0..=100).contains(&pct) => total / 100 * pct as u64,
        _ => u64::MAX,
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
fn available_memory() -> u64 {
    u64::MAX
}

/// Room for the pixels, and for the writer's own copy of them.
fn enough_memory(w: u32, h: u32) -> bool {
    let need = w as u64 * h as u64 * 4;
    need < (1u64 << 40) && need * 2 + need / 4 < available_memory()
}

// ---------- reading ----------

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

fn from_layout(buf: Vec<u8>, c: image::ColorType) -> Option<Vec<u8>> {
    use image::ColorType as C;
    let to8 =
        |b: &[u8]| -> Vec<u8> { b.chunks_exact(2).map(|p| (u16::from_ne_bytes([p[0], p[1]]) >> 8) as u8).collect() };
    let la = |b: &[u8]| {
        let mut out = vec![0u8; b.len() * 2];
        for (d, s) in out.chunks_exact_mut(4).zip(b.chunks_exact(2)) {
            d[0] = s[0];
            d[1] = s[0];
            d[2] = s[0];
            d[3] = s[1];
        }
        out
    };
    Some(match c {
        C::Rgba8 => buf,
        C::Rgb8 => opaque_rgba(&buf),
        C::L8 => gray_rgba(&buf),
        C::La8 => la(&buf),
        C::Rgba16 => to8(&buf),
        C::Rgb16 => opaque_rgba(&to8(&buf)),
        C::L16 => gray_rgba(&to8(&buf)),
        C::La16 => la(&to8(&buf)),
        _ => return None,
    })
}

fn read_with<D: ImageDecoder>(dec: D) -> Result<Image8, Reason> {
    let (w, h) = dec.dimensions();
    if w == 0 || h == 0 {
        return Err(Reason::Damaged);
    }
    if !enough_memory(w, h) {
        return Err(Reason::TooLarge);
    }
    let c = dec.color_type();
    let size = dec.total_bytes();
    if size > (1 << 40) {
        return Err(Reason::TooLarge);
    }
    let mut buf = vec![0u8; size as usize];
    dec.read_image(&mut buf).map_err(|_| Reason::Damaged)?;
    let rgba = from_layout(buf, c).ok_or(Reason::Damaged)?;
    Ok(Image8 { w, h, rgba })
}

fn read_ico(b: &[u8]) -> Result<Image8, Reason> {
    // the largest size, the best of its depths
    let n = u16::from_le_bytes([*b.get(4).ok_or(Reason::Damaged)?, *b.get(5).ok_or(Reason::Damaged)?]) as usize;
    let mut best: Option<(u64, u64, Image8)> = None;
    for i in 0..n {
        let at = 6 + i * 16;
        let Some(e) = b.get(at..at + 16) else { break };
        let len = u32::from_le_bytes([e[8], e[9], e[10], e[11]]) as usize;
        let off = u32::from_le_bytes([e[12], e[13], e[14], e[15]]) as usize;
        let Some(data) = off.checked_add(len).and_then(|z| b.get(off..z)) else { continue };
        let mut one = Vec::with_capacity(22 + len);
        one.extend_from_slice(&[0, 0, 1, 0, 1, 0]);
        one.extend_from_slice(&e[..12]);
        one.extend_from_slice(&22u32.to_le_bytes());
        one.extend_from_slice(data);
        let Ok(d) = image::codecs::ico::IcoDecoder::new(Cursor::new(&one)) else { continue };
        let Ok(img) = read_with(d) else { continue };
        let bits = u16::from_le_bytes([e[6], e[7]]) as u64;
        let area = img.w as u64 * img.h as u64;
        if best.as_ref().is_none_or(|(a, bb, _)| area > *a || area == *a && bits > *bb) {
            best = Some((area, bits, img));
        }
    }
    best.map(|b| b.2).ok_or(Reason::Damaged)
}

fn tiff_first(source: &[u8]) -> Result<Image8, Reason> {
    use tiff::ColorType as C;
    use tiff::tags::Tag;
    let mut d = tiff::decoder::Decoder::new(Cursor::new(source))
        .map(|d| d.with_limits(tiff::decoder::Limits::unlimited()))
        .map_err(|_| Reason::Damaged)?;
    let (w, h) = d.dimensions().map_err(|_| Reason::Damaged)?;
    if w == 0 || h == 0 {
        return Err(Reason::Damaged);
    }
    if !enough_memory(w, h) {
        return Err(Reason::TooLarge);
    }
    let colour = d.colortype().map_err(|_| Reason::Damaged)?;
    let orientation = d.find_tag(Tag::Orientation).ok().flatten().and_then(|v| v.into_u16().ok()).unwrap_or(1) as u8;
    let palette: Option<Vec<u16>> = d.find_tag(Tag::ColorMap).ok().flatten().and_then(|v| v.into_u16_vec().ok());
    use tiff::decoder::DecodingResult as R;
    let data = match d.read_image().map_err(|_| Reason::Damaged)? {
        R::U8(v) => v,
        R::U16(v) => v.iter().map(|x| (x >> 8) as u8).collect(),
        R::U32(v) => v.iter().map(|x| (x >> 24) as u8).collect(),
        R::F32(v) => v.iter().map(|x| (x.clamp(0.0, 1.0) * 255.0 + 0.5) as u8).collect(),
        _ => return Err(Reason::Damaged),
    };
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
            let (bits, is_palette) = (bits as usize, palette.is_some());
            let row = (w as usize * bits).div_ceil(8);
            if data.len() < row * h as usize {
                return Err(Reason::Damaged);
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
                        let map = palette.as_ref().ok_or(Reason::Damaged)?;
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
        _ => return Err(Reason::Damaged),
    };
    Ok(upright(rgba, w, h, orientation))
}

/// The first frame of a GIF, as it shows.
fn gif_first(b: &[u8]) -> Result<Image8, Reason> {
    let mut o = gif::DecodeOptions::new();
    o.set_color_output(gif::ColorOutput::RGBA);
    o.set_memory_limit(gif::MemoryLimit::Bytes(std::num::NonZeroU64::new(1 << 32).unwrap()));
    o.check_frame_consistency(false);
    let mut d = o.read_info(Cursor::new(b)).map_err(|_| Reason::Damaged)?;
    let (w, h) = (d.width() as usize, d.height() as usize);
    if w == 0 || h == 0 {
        return Err(Reason::Damaged);
    }
    if !enough_memory(w as u32, h as u32) {
        return Err(Reason::TooLarge);
    }
    let mut pix = vec![0u8; w * h * 4];
    let f = d.read_next_frame().map_err(|_| Reason::Damaged)?.ok_or(Reason::Damaged)?;
    let (l, t, fw, fh) = (f.left as usize, f.top as usize, f.width as usize, f.height as usize);
    for y in 0..fh {
        if t + y >= h {
            break;
        }
        let src = &f.buffer[y * fw * 4..(y + 1) * fw * 4];
        let width = fw.min(w.saturating_sub(l));
        let dst = &mut pix[((t + y) * w + l) * 4..((t + y) * w + l + width) * 4];
        for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
            if s[3] != 0 {
                d.copy_from_slice(s);
            }
        }
    }
    Ok(Image8 { w: w as u32, h: h as u32, rgba: pix })
}

fn webp_still(b: &[u8]) -> Result<Image8, Reason> {
    use libwebp_sys as w;
    let (mut width, mut height) = (0i32, 0i32);
    // SAFETY: libwebp reads only within the slice it is given, and the buffer it returns is freed once copied
    unsafe {
        if w::WebPGetInfo(b.as_ptr(), b.len(), &mut width, &mut height) == 0 || width <= 0 || height <= 0 {
            return Err(Reason::Damaged);
        }
        if !enough_memory(width as u32, height as u32) {
            return Err(Reason::TooLarge);
        }
        let p = w::WebPDecodeRGBA(b.as_ptr(), b.len(), &mut width, &mut height);
        if p.is_null() {
            return Err(Reason::Damaged);
        }
        let n = width as usize * height as usize * 4;
        let out = std::slice::from_raw_parts(p, n).to_vec();
        w::WebPFree(p as *mut _);
        Ok(Image8 { w: width as u32, h: height as u32, rgba: out })
    }
}

/// The first frame of a moving WebP, painted on its canvas.
fn webp_first_frame(b: &[u8]) -> Result<Image8, Reason> {
    use libwebp_sys as w;
    // SAFETY: the decoder reads `b`, which outlives it, and is deleted before returning; the frame is copied out first
    unsafe {
        let mut opts = std::mem::zeroed::<w::WebPAnimDecoderOptions>();
        if w::WebPAnimDecoderOptionsInitInternal(&mut opts, w::WEBP_DEMUX_ABI_VERSION as _) == 0 {
            return Err(Reason::Damaged);
        }
        opts.color_mode = w::WEBP_CSP_MODE::MODE_RGBA;
        let data = w::WebPData { bytes: b.as_ptr(), size: b.len() };
        let dec = w::WebPAnimDecoderNewInternal(&data, &opts, w::WEBP_DEMUX_ABI_VERSION as _);
        if dec.is_null() {
            return Err(Reason::Damaged);
        }
        let result = (|| {
            let mut info = std::mem::zeroed::<w::WebPAnimInfo>();
            if w::WebPAnimDecoderGetInfo(dec, &mut info) == 0 || info.frame_count == 0 {
                return Err(Reason::Damaged);
            }
            let (cw, ch) = (info.canvas_width, info.canvas_height);
            if !enough_memory(cw, ch) {
                return Err(Reason::TooLarge);
            }
            let (mut buf, mut ts) = (std::ptr::null_mut::<u8>(), 0i32);
            if w::WebPAnimDecoderGetNext(dec, &mut buf, &mut ts) == 0 {
                return Err(Reason::Damaged);
            }
            Ok(Image8 { w: cw, h: ch, rgba: std::slice::from_raw_parts(buf, cw as usize * ch as usize * 4).to_vec() })
        })();
        w::WebPAnimDecoderDelete(dec);
        result
    }
}

fn is_moving_webp(b: &[u8]) -> bool {
    metadata::webp_chunks(b).first().is_some_and(|(t, r)| t == b"VP8X" && b.get(r.start).is_some_and(|f| f & 0x02 != 0))
}

/// What a file holds, upright, and what it carries besides pixels.
struct Read {
    kind: Kind,
    img: Image8,
    meta: Meta,
}

fn read(bytes: &[u8], name: &str) -> Result<Read, Reason> {
    let kind = match formats::sniff(bytes, name) {
        Sniff::Is(k) if k != Kind::Svg => k,
        // HEIC, AVIF, camera RAW and the rest: refused by rule, whatever the file is called
        Sniff::Refused | Sniff::Is(_) => return Err(Reason::Refused(crate::Refusal::Unsupported)),
        Sniff::Damaged => return Err(Reason::Damaged),
    };
    // a decoder that trips over a corrupt file must not take the program down
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| read_kind(kind, bytes))).unwrap_or(Err(Reason::Damaged))
}

fn read_kind(kind: Kind, b: &[u8]) -> Result<Read, Reason> {
    let meta = metadata::read(kind, b);
    let orientation = meta.exif.as_deref().map(exif::orientation).unwrap_or(1);
    let img = match kind {
        Kind::Jpg => {
            let mut d = image::codecs::jpeg::JpegDecoder::new(Cursor::new(b)).map_err(|_| Reason::Damaged)?;
            let _ = d.set_limits(image::Limits::no_limits());
            let i = read_with(d)?;
            upright(i.rgba, i.w, i.h, orientation)
        }
        Kind::Png => {
            let d = image::codecs::png::PngDecoder::with_limits(Cursor::new(b), image::Limits::no_limits())
                .map_err(|_| Reason::Damaged)?;
            let i = read_with(d)?;
            upright(i.rgba, i.w, i.h, orientation)
        }
        Kind::Bmp => read_with(image::codecs::bmp::BmpDecoder::new(Cursor::new(b)).map_err(|_| Reason::Damaged)?)?,
        Kind::Ico => read_ico(b)?,
        Kind::Tiff => {
            let grey = exif::palette_read_as_grey(b);
            tiff_first(grey.as_deref().unwrap_or(b))?
        }
        Kind::Gif => gif_first(b)?,
        Kind::Webp => {
            let i = if is_moving_webp(b) { webp_first_frame(b)? } else { webp_still(b)? };
            upright(i.rgba, i.w, i.h, orientation)
        }
        Kind::Tga => read_with(image::codecs::tga::TgaDecoder::new(Cursor::new(b)).map_err(|_| Reason::Damaged)?)?,
        Kind::Qoi => read_with(image::codecs::qoi::QoiDecoder::new(Cursor::new(b)).map_err(|_| Reason::Damaged)?)?,
        Kind::Pbm | Kind::Pgm | Kind::Ppm | Kind::Pam => {
            read_with(image::codecs::pnm::PnmDecoder::new(Cursor::new(b)).map_err(|_| Reason::Damaged)?)?
        }
        Kind::Ff => {
            read_with(image::codecs::farbfeld::FarbfeldDecoder::new(Cursor::new(b)).map_err(|_| Reason::Damaged)?)?
        }
        Kind::Pcx | Kind::Ilbm => {
            let fits = |w: u32, h: u32| enough_memory(w, h);
            let found = if kind == Kind::Pcx {
                pcx::decode(b, &fits).map(|p| (p.rgba, p.w, p.h))
            } else {
                ilbm::decode(b, &fits).map(|p| (p.rgba, p.w, p.h))
            };
            match found {
                Ok((rgba, w, h)) => Image8 { w, h, rgba },
                Err(pcx::Problem::TooLarge) => return Err(Reason::TooLarge),
                Err(pcx::Problem::Damaged) => return Err(Reason::Damaged),
            }
        }
        Kind::Svg => return Err(Reason::Damaged),
    };
    Ok(Read { kind, img, meta })
}

fn kind_of(target: &str) -> Kind {
    match target {
        "jpg" => Kind::Jpg,
        "webp" => Kind::Webp,
        "tiff" => Kind::Tiff,
        "bmp" => Kind::Bmp,
        _ => Kind::Png,
    }
}

/// What the output carries. The pixels are already upright, so the EXIF block is brought up to date with them.
fn carried(r: &Read, to: Kind) -> Meta {
    let can_hold_colour = matches!(to, Kind::Jpg | Kind::Png | Kind::Webp | Kind::Tiff);
    let rgb_profile = r.meta.icc.as_ref().filter(|i| i.len() > 20 && &i[16..20] == b"RGB ");
    let icc = if can_hold_colour { rgb_profile.cloned() } else { None };
    if !can_hold_colour {
        return Meta { icc, ..Default::default() };
    }
    Meta {
        icc,
        exif: r.meta.exif.as_deref().and_then(|b| exif::after_pixels_changed(b, r.img.w, r.img.h)),
        xmp: r.meta.xmp.clone(),
        iptc: None,
        comment: None,
        png_text: if to == Kind::Png { r.meta.png_text.clone() } else { Vec::new() },
        other: false,
    }
}

fn encode_to(img: &Image8, to: Kind, quality: u8, m: &Meta, quick: bool) -> Result<Vec<u8>, String> {
    match to {
        Kind::Jpg => encode::jpeg(img, quality, m),
        Kind::Webp => encode::webp(img, quality, false, m),
        Kind::Bmp => encode::bmp(img),
        Kind::Tiff => encode::tiff(std::iter::once(Ok(Image8 { w: img.w, h: img.h, rgba: img.rgba.clone() })), m),
        _ => encode::png(img, m, quick),
    }
}

fn too_large_for(to: Kind, img: &Image8) -> bool {
    match to {
        Kind::Jpg => img.w > 65535 || img.h > 65535,
        Kind::Webp => img.w > 16383 || img.h > 16383,
        _ => false,
    }
}

fn name_of(p: &Path) -> String {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

pub fn convert(p: &Path, target: &str, opts: &Options, out: &mut Outputs, step: Step) -> Result<(), Reason> {
    let bytes = crate::read_all(p)?;
    step(0.1);
    let r = read(&bytes, &name_of(p))?;
    drop(bytes);
    step(0.5);
    let to = kind_of(target);
    if too_large_for(to, &r.img) {
        return Err(Reason::TooLarge);
    }
    let m = carried(&r, to);
    let encoded = encode_to(&r.img, to, opts.quality.clamp(10, 100), &m, false).map_err(|_| Reason::TooLarge)?;
    step(0.9);
    out.write(target, &encoded)
}

// ---------- estimating: the real writer on a sample of the picture ----------

/// The last picture read for an estimate, so moving the quality slider doesn't read the file again. Only a sample of its
/// pixels is kept, never the whole picture.
struct Seen {
    path: std::path::PathBuf,
    len: u64,
    modified: Option<std::time::SystemTime>,
    read: Option<Sampled>,
}

struct Sampled {
    /// the picture's real size
    w: u32,
    h: u32,
    alpha: bool,
    sample: Image8,
    /// what it carries, as read: only its sizes are used here
    meta: Meta,
}

static SEEN: Mutex<Option<Seen>> = Mutex::new(None);

/// Strips across the whole picture, about a quarter million pixels in all, so the sample looks like the picture.
fn sample(img: &Image8) -> Image8 {
    const STRIP: u32 = 16;
    let want = 250_000u64;
    let total = img.w as u64 * img.h as u64;
    if total <= want * 2 {
        return Image8 { w: img.w, h: img.h, rgba: img.rgba.clone() };
    }
    let strips = ((want / (img.w as u64 * STRIP as u64)).max(1) as u32).min(img.h / STRIP).max(1);
    let gap = img.h / strips;
    let row = img.w as usize * 4;
    let mut rgba = Vec::with_capacity(strips as usize * STRIP as usize * row);
    for s in 0..strips {
        let y = s * gap + (gap.saturating_sub(STRIP)) / 2;
        let y = y.min(img.h - STRIP);
        rgba.extend_from_slice(&img.rgba[y as usize * row..(y + STRIP) as usize * row]);
    }
    Image8 { w: img.w, h: strips * STRIP, rgba }
}

pub fn estimate(p: &Path, size: u64, target: &str, opts: &Options) -> u64 {
    let meta = std::fs::metadata(p).ok();
    let modified = meta.as_ref().and_then(|m| m.modified().ok());
    let mut seen = SEEN.lock().unwrap_or_else(|e| e.into_inner());
    let fresh = seen.as_ref().is_some_and(|s| s.path == p && s.len == size && s.modified == modified);
    if !fresh {
        let read = std::fs::read(p).ok().and_then(|b| read(&b, &name_of(p)).ok()).map(|r| Sampled {
            w: r.img.w,
            h: r.img.h,
            alpha: r.img.has_alpha(),
            sample: sample(&r.img),
            meta: r.meta,
        });
        *seen = Some(Seen { path: p.to_path_buf(), len: size, modified, read });
    }
    let Some(r) = seen.as_ref().and_then(|s| s.read.as_ref()) else { return size };
    let to = kind_of(target);
    let can_hold_colour = matches!(to, Kind::Jpg | Kind::Png | Kind::Webp | Kind::Tiff);
    let extra = if can_hold_colour {
        r.meta.icc.as_ref().map_or(0, |v| v.len())
            + r.meta.exif.as_ref().map_or(0, |v| v.len())
            + r.meta.xmp.as_ref().map_or(0, |v| v.len())
    } else {
        0
    };
    if to == Kind::Bmp {
        let bpp = if r.alpha { 4 } else { 3 };
        let row = (r.w as u64 * bpp).div_ceil(4) * 4;
        return 54 + 84 + row * r.h as u64;
    }
    let part = &r.sample;
    let Ok(bytes) = encode_to(part, to, opts.quality.clamp(10, 100), &Meta::default(), true) else { return size };
    let scale = (r.w as f64 * r.h as f64) / (part.w as f64 * part.h as f64);
    // PNG was written quickly for the estimate; the real save takes more care and comes out a little smaller
    let care = if to == Kind::Png && scale > 1.0 { 0.92 } else { 1.0 };
    (bytes.len() as f64 * scale * care) as u64 + extra as u64
}
