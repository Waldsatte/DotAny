//! A smaller file: the pictures inside are read and written again as JPEG, smaller where they're larger than a page
//! needs. Only pictures this can read exactly are touched (JPEG, and 8-bit grey or RGB pixels); a picture that comes out
//! no smaller is left as it was. Text, drawings and fonts stay as they are.

use lopdf::{Document, Object, Stream};

pub struct Level {
    /// the longest side a picture keeps, in pixels
    pub side: u32,
    pub quality: u8,
}
pub const SMALL: Level = Level { side: 1700, quality: 72 };
pub const LEAST: Level = Level { side: 1100, quality: 48 };

fn name_of<'a>(doc: &'a Document, o: &'a Object) -> Option<&'a [u8]> {
    match o {
        Object::Name(n) => Some(n),
        Object::Reference(id) => doc.get_object(*id).ok().and_then(|o| o.as_name().ok()),
        Object::Array(a) if a.len() == 1 => name_of(doc, &a[0]),
        _ => None,
    }
}

/// How many colour channels a colour space has, when it's one this can write as JPEG.
fn channels(doc: &Document, cs: &Object) -> Option<u8> {
    let cs = match cs {
        Object::Reference(id) => doc.get_object(*id).ok()?,
        o => o,
    };
    match cs {
        Object::Name(n) if n == b"DeviceRGB" => Some(3),
        Object::Name(n) if n == b"DeviceGray" => Some(1),
        Object::Array(a) if a.first().and_then(|o| o.as_name().ok()) == Some(b"ICCBased") => {
            let s = a
                .get(1)
                .and_then(|o| o.as_reference().ok())
                .and_then(|id| doc.get_object(id).ok())
                .and_then(|o| o.as_stream().ok())?;
            match s.dict.get(b"N").ok().and_then(|o| o.as_i64().ok()) {
                Some(3) => Some(3),
                Some(1) => Some(1),
                _ => None,
            }
        }
        _ => None,
    }
}

pub fn pictures(doc: &mut Document, level: Level) {
    let ids: Vec<_> = doc.objects.iter().filter(|(_, o)| matches!(o, Object::Stream(s) if s.dict.get(b"Subtype").and_then(|v| v.as_name()).ok() == Some(b"Image"))).map(|(id, _)| *id).collect();
    for id in ids {
        let Some(Object::Stream(s)) = doc.objects.get(&id) else { continue };
        if let Some(new) = smaller(doc, s, &level) {
            doc.objects.insert(id, Object::Stream(new));
        }
    }
}

fn smaller(doc: &Document, s: &Stream, level: &Level) -> Option<Stream> {
    let d = &s.dict;
    // masks, stencils, decode arrays and odd depths are left alone
    if d.has(b"ImageMask") || d.has(b"Decode") || d.has(b"Mask") {
        return None;
    }
    if d.get(b"BitsPerComponent").ok().and_then(|o| o.as_i64().ok()) != Some(8) {
        return None;
    }
    let w = d.get(b"Width").ok()?.as_i64().ok()? as u32;
    let h = d.get(b"Height").ok()?.as_i64().ok()? as u32;
    if w < 64 || h < 64 {
        return None;
    }
    let n = channels(doc, d.get(b"ColorSpace").ok()?)?;
    let filter = d.get(b"Filter").ok().and_then(|f| name_of(doc, f)).map(|f| f.to_vec());
    let pixels: image::DynamicImage = match filter.as_deref() {
        Some(b"DCTDecode") => {
            let img = image::load_from_memory_with_format(&s.content, image::ImageFormat::Jpeg).ok()?;
            if (img.color().channel_count() == 1) != (n == 1) {
                return None;
            }
            img
        }
        Some(b"FlateDecode") | None => {
            let raw = if filter.is_some() { s.decompressed_content().ok()? } else { s.content.clone() };
            if raw.len() != (w * h * n as u32) as usize {
                return None;
            }
            if n == 1 {
                image::DynamicImage::ImageLuma8(image::GrayImage::from_raw(w, h, raw)?)
            } else {
                image::DynamicImage::ImageRgb8(image::RgbImage::from_raw(w, h, raw)?)
            }
        }
        _ => return None,
    };
    let long = w.max(h);
    let img = if long > level.side {
        let k = level.side as f32 / long as f32;
        pixels.resize_exact(
            ((w as f32 * k).round() as u32).max(1),
            ((h as f32 * k).round() as u32).max(1),
            image::imageops::FilterType::Triangle,
        )
    } else {
        pixels
    };
    let mut out = Vec::new();
    let enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut out, level.quality);
    if n == 1 {
        img.to_luma8().write_with_encoder(enc).ok()?;
    } else {
        img.to_rgb8().write_with_encoder(enc).ok()?;
    }
    let before = s.content.len();
    if out.len() >= before {
        return None;
    }
    let mut nd = d.clone();
    nd.set("Filter", "DCTDecode");
    nd.remove(b"DecodeParms");
    nd.set("Width", img.width() as i64);
    nd.set("Height", img.height() as i64);
    nd.set("Length", out.len() as i64);
    // a soft mask keeps its own size: PDF allows a mask of different size from its picture
    Some(Stream::new(nd, out).with_compression(false))
}
