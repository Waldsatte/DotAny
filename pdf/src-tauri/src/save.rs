//! Writing a PDF from what the window holds. The page sends a plan: which pages in which order (from the open file,
//! from other PDFs, from pictures, or blank), how each is turned, what was drawn or written on it, which paragraphs were
//! changed, which pages carry a redaction (sent as the page drawn as a picture), the form's values and the size asked for.
//!
//! When the pages are the file's own, in its own order, at the size it already has, the save is an incremental update:
//! the original bytes come first, untouched, and only the changed objects are appended (ISO 32000-1, 7.5.6), encrypted
//! the same way if the file was. Anything else, and always a redaction, is a new file written whole, with nothing left in
//! it that no page uses. So is a file whose cross-reference table had to be repaired to be read at all: an update can't
//! point back to a table that is wrong.

use crate::embed::{self, Fonts, Style};
use crate::geom::{Mat, fmt, rgb};
use crate::{shrink, textcut};
use dotany_base::limits;
use lopdf::{Dictionary, Document, IncrementalDocument, Object, ObjectId, Stream, StringFormat, dictionary};
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

#[derive(Deserialize, Clone)]
pub struct Plan {
    pub pages: Vec<PagePlan>,
    #[serde(default)]
    pub fields: Vec<FieldValue>,
    #[serde(default = "same")]
    pub size: String,
}
fn same() -> String {
    "same".into()
}

#[derive(Deserialize, Clone)]
pub struct PagePlan {
    /// the source: 0 is the open file; others were added in the pages view
    pub src: u32,
    /// the page in its source, from 1; 0 for a picture or a blank page
    #[serde(default)]
    pub n: u32,
    /// the page's size as the window shows it, before the reader's own turning
    pub w: f32,
    pub h: f32,
    /// PDF.js's viewport transform at scale 1: user space to the window's page coordinates
    #[serde(default)]
    pub vt: Vec<f32>,
    #[serde(default)]
    pub rot: i32,
    #[serde(default)]
    pub blank: bool,
    #[serde(default)]
    pub marks: Vec<Mark>,
    /// a page with a redaction, drawn whole as a picture (a blob's id)
    #[serde(default)]
    pub raster: Option<String>,
}

#[derive(Deserialize, Clone)]
pub struct Line {
    pub text: String,
    pub x: f32,
    /// the baseline, in the page's coordinates
    pub y: f32,
    #[serde(default)]
    pub width: f32,
    #[serde(default)]
    pub justify: bool,
}

#[derive(Deserialize, Clone)]
#[serde(tag = "t")]
pub enum Mark {
    #[serde(rename = "hl")]
    Hl { color: String, rects: Vec<[f32; 4]> },
    #[serde(rename = "brush")]
    Brush { color: String, size: f32, pts: Vec<f32> },
    #[serde(rename = "text")]
    Note {
        x: f32,
        y: f32,
        words: String,
        size: f32,
        #[serde(default)]
        bold: bool,
        color: String,
    },
    #[serde(rename = "sign")]
    Sign {
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        sw: f32,
        sh: f32,
        #[serde(default)]
        strokes: Vec<Vec<f32>>,
        #[serde(default)]
        img: Option<String>,
    },
    #[serde(rename = "edit")]
    Edit {
        ox: f32,
        oy: f32,
        ow: f32,
        oh: f32,
        size: f32,
        #[serde(default)]
        bold: bool,
        #[serde(default)]
        serif: bool,
        color: String,
        lines: Vec<Line>,
    },
    /// burned into the page's picture by the window; nothing to do here
    #[serde(rename = "redact")]
    Redact {},
}

#[derive(Deserialize, Clone)]
pub struct FieldValue {
    pub src: u32,
    pub name: String,
    pub value: serde_json::Value,
}

pub enum Kind {
    Pdf,
    Image,
}

pub struct Source {
    pub bytes: Arc<Vec<u8>>,
    pub kind: Kind,
    pub password: Option<String>,
}

#[derive(Debug)]
pub enum Fail {
    /// the file couldn't be read again as a PDF
    Damaged(String),
    Other(String),
}

impl From<lopdf::Error> for Fail {
    fn from(e: lopdf::Error) -> Self {
        Fail::Other(e.to_string())
    }
}

pub fn load(src: &Source) -> Result<Document, Fail> {
    let r = match &src.password {
        Some(p) if !p.is_empty() => Document::load_mem_with_options(&src.bytes, lopdf::LoadOptions::with_password(p)),
        _ => Document::load_mem(&src.bytes),
    };
    let doc = r.map_err(|e| Fail::Damaged(e.to_string()))?;
    if doc.is_encrypted() {
        return Err(Fail::Damaged("still encrypted".into()));
    }
    Ok(doc)
}

/// What a person may do with an encrypted file opened with the password they typed (ISO 32000-1, table 22). The file's
/// owner password lifts the limits; the open password doesn't.
#[derive(serde::Serialize, Clone, Copy)]
pub struct Rights {
    pub encrypted: bool,
    pub modify: bool,
    pub copy: bool,
    pub annotate: bool,
    pub fill: bool,
    pub assemble: bool,
    /// lopdf read it; otherwise only an untouched copy can be saved
    pub writable: bool,
}

pub fn rights(src: &Source) -> Rights {
    let all = |w| Rights {
        encrypted: false,
        modify: true,
        copy: true,
        annotate: true,
        fill: true,
        assemble: true,
        writable: w,
    };
    let Ok(doc) = load(src) else { return all(false) };
    let Some(state) = doc.encryption_state.clone() else { return all(true) };
    // the Encrypt dictionary is set aside once decrypted: put it back on an empty document to ask whose password this is
    let owner = (|| -> Option<bool> {
        let mut probe = Document::new();
        let enc = state.encode().ok()?;
        let id = probe.add_object(Object::Dictionary(enc));
        probe.trailer.set("Encrypt", Object::Reference(id));
        if let Ok(ids) = doc.trailer.get(b"ID") {
            probe.trailer.set("ID", ids.clone());
        }
        Some(probe.authenticate_owner_password(src.password.as_deref().unwrap_or("")).is_ok())
    })()
    .unwrap_or(false);
    if owner {
        let mut r = all(true);
        r.encrypted = true;
        return r;
    }
    let p = state.permissions().bits() as i64;
    let bit = |n: u32| p & (1 << (n - 1)) != 0;
    Rights {
        encrypted: true,
        modify: bit(4),
        copy: bit(5),
        annotate: bit(6),
        fill: bit(6) || bit(9),
        assemble: bit(4) || bit(11),
        writable: true,
    }
}

fn get<'a>(doc: &'a Document, o: &'a Object) -> &'a Object {
    doc.dereference(o).map(|(_, o)| o).unwrap_or(o)
}

fn dict_of(doc: &Document, id: ObjectId) -> Dictionary {
    doc.get_dictionary(id).cloned().unwrap_or_default()
}

/// The page's own copies of what it would otherwise inherit from the page tree.
fn flatten_page(doc: &mut Document, page: ObjectId) {
    let mut found: Vec<(&[u8], Object)> = Vec::new();
    for key in [b"Resources".as_slice(), b"MediaBox", b"CropBox", b"Rotate"] {
        let mut at = Some(page);
        let mut depth = 0;
        while let Some(id) = at {
            let Ok(d) = doc.get_dictionary(id) else { break };
            if let Ok(v) = d.get(key) {
                if id != page {
                    found.push((key, v.clone()));
                }
                break;
            }
            at = d.get(b"Parent").and_then(|p| p.as_reference()).ok();
            depth += 1;
            if depth > 64 {
                break;
            }
        }
    }
    if let Ok(d) = doc.get_dictionary_mut(page) {
        for (k, v) in found {
            d.set(k, v);
        }
        if !d.has(b"MediaBox") {
            d.set("MediaBox", vec![0.into(), 0.into(), 612.into(), 792.into()]);
        }
    }
}

struct Work<'a> {
    doc: Document,
    dirty: BTreeSet<ObjectId>,
    fonts: Fonts,
    blobs: &'a HashMap<String, Vec<u8>>,
    serial: u32,
}

impl Work<'_> {
    fn add(&mut self, o: impl Into<Object>) -> ObjectId {
        let id = self.doc.add_object(o);
        self.dirty.insert(id);
        id
    }
    fn touch(&mut self, id: ObjectId) {
        self.dirty.insert(id);
    }
    fn page(&mut self, id: ObjectId) -> &mut Dictionary {
        self.dirty.insert(id);
        self.doc.get_dictionary_mut(id).expect("a page")
    }
    /// The page's resources as its own dictionary, with a /Font or /XObject sub-dictionary of its own too.
    fn own_resources(&mut self, page: ObjectId, sub: &[u8], add: Dictionary) {
        let res = self
            .doc
            .get_dictionary(page)
            .ok()
            .and_then(|d| d.get(b"Resources").ok())
            .cloned()
            .unwrap_or(Object::Dictionary(Dictionary::new()));
        let mut res = get(&self.doc, &res).as_dict().cloned().unwrap_or_default();
        let inner = res.get(sub).ok().cloned().unwrap_or(Object::Dictionary(Dictionary::new()));
        let mut inner = get(&self.doc, &inner).as_dict().cloned().unwrap_or_default();
        for (k, v) in add.iter() {
            inner.set(k.clone(), v.clone());
        }
        res.set(sub.to_vec(), Object::Dictionary(inner));
        self.page(page).set("Resources", Object::Dictionary(res));
    }
    fn page_resources(&self, page: ObjectId) -> Dictionary {
        let res = self
            .doc
            .get_dictionary(page)
            .ok()
            .and_then(|d| d.get(b"Resources").ok())
            .cloned()
            .unwrap_or(Object::Dictionary(Dictionary::new()));
        get(&self.doc, &res).as_dict().cloned().unwrap_or_default()
    }
    fn annots_push(&mut self, page: ObjectId, ids: Vec<ObjectId>) {
        if ids.is_empty() {
            return;
        }
        let cur = self.doc.get_dictionary(page).ok().and_then(|d| d.get(b"Annots").ok()).cloned();
        let mut list = cur.map(|o| get(&self.doc, &o).as_array().cloned().unwrap_or_default()).unwrap_or_default();
        list.extend(ids.into_iter().map(Object::Reference));
        self.page(page).set("Annots", Object::Array(list));
    }
    fn name(&mut self, kind: &str) -> String {
        self.serial += 1;
        format!("DotAny{kind}{}", self.serial)
    }
}

/// Text in the window's coordinates (y down) at a baseline, in one or more fonts. Justified lines spread their spaces.
fn text_ops(
    fonts: &mut Fonts,
    style: Style,
    size: f32,
    x: f32,
    y: f32,
    text: &str,
    justify_to: Option<f32>,
) -> (String, Vec<embed::Run>) {
    let runs = fonts.runs(style, text);
    let natural: f32 = runs.iter().flat_map(|r| r.adv.iter()).sum::<f32>() * size;
    let gaps: usize = runs.iter().flat_map(|r| r.chars.iter()).filter(|c| **c == ' ').count();
    let total_chars: usize = runs.iter().map(|r| r.chars.len()).sum();
    let extra = match justify_to {
        Some(w) if w > natural && total_chars > 1 => {
            if gaps > 0 {
                (w - natural) / gaps as f32
            } else {
                (w - natural) / (total_chars - 1) as f32
            }
        }
        _ => 0.0,
    };
    let mut s = format!("BT 1 0 0 -1 {} {} Tm ", fmt(x), fmt(y));
    let mut seen = 0usize;
    for r in &runs {
        s.push_str(&format!("/{} {} Tf ", String::from_utf8_lossy(&Fonts::res_name(r.font)), fmt(size)));
        if extra == 0.0 {
            s.push_str(&hex_str(&r.gids));
            s.push_str(" Tj ");
        } else {
            s.push('[');
            for (k, g) in r.gids.iter().enumerate() {
                s.push_str(&hex_str(&[*g]));
                seen += 1;
                let space = r.chars[k] == ' ';
                if (gaps > 0 && space) || (gaps == 0 && seen < total_chars) {
                    s.push_str(&format!(" {} ", fmt(-extra * 1000.0 / size)));
                }
            }
            s.push_str("] TJ ");
        }
    }
    s.push_str("ET\n");
    (s, runs)
}

fn hex_str(g: &[u16]) -> String {
    let mut s = String::from("<");
    for v in g {
        s.push_str(&format!("{v:04X}"));
    }
    s.push('>');
    s
}

/// A pen line through the points, smoothed the way the window draws it: midpoints joined by curves.
fn smooth_path(pts: &[f32], map: &dyn Fn(f32, f32) -> (f32, f32)) -> String {
    if pts.len() < 2 {
        return String::new();
    }
    let p = |i: usize| map(pts[i], pts[i + 1]);
    let (x0, y0) = p(0);
    let mut s = format!("{} {} m ", fmt(x0), fmt(y0));
    if pts.len() < 4 {
        s.push_str(&format!("{} {} l ", fmt(x0 + 0.01), fmt(y0)));
        return s;
    }
    let mut cur = (x0, y0);
    let mut i = 2;
    while i + 2 < pts.len() {
        let q = p(i);
        let n = p(i + 2);
        let end = ((q.0 + n.0) / 2.0, (q.1 + n.1) / 2.0);
        let c1 = (cur.0 + 2.0 / 3.0 * (q.0 - cur.0), cur.1 + 2.0 / 3.0 * (q.1 - cur.1));
        let c2 = (end.0 + 2.0 / 3.0 * (q.0 - end.0), end.1 + 2.0 / 3.0 * (q.1 - end.1));
        s.push_str(&format!(
            "{} {} {} {} {} {} c ",
            fmt(c1.0),
            fmt(c1.1),
            fmt(c2.0),
            fmt(c2.1),
            fmt(end.0),
            fmt(end.1)
        ));
        cur = end;
        i += 2;
    }
    let last = p(pts.len() - 2);
    s.push_str(&format!("{} {} l ", fmt(last.0), fmt(last.1)));
    s
}

fn color_ops(c: [f32; 3], stroke: bool) -> String {
    format!("{} {} {} {} ", fmt(c[0]), fmt(c[1]), fmt(c[2]), if stroke { "RG" } else { "rg" })
}

fn mat_ops(m: &Mat) -> String {
    let v = m.0;
    format!("{} {} {} {} {} {} cm\n", fmt(v[0]), fmt(v[1]), fmt(v[2]), fmt(v[3]), fmt(v[4]), fmt(v[5]))
}

/// The largest picture decoded into a page: 80 million pixels (about 10000 x 8000), far more than a page shows. A
/// picture says how big it is in its header, and that is checked before a single pixel is decoded: a few hundred
/// bytes of PNG can claim to be 100000 pixels square, 70 GB once decoded.
const MAX_PIXELS: u64 = 80_000_000;

/// What the picture's header says its size is, without decoding it.
fn picture_size(bytes: &[u8]) -> Option<(u32, u32)> {
    image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?.into_dimensions().ok()
}

/// The picture's pixels, only when its header says they fit under `MAX_PIXELS`; the decoder is held to that too.
fn decode_picture(bytes: &[u8]) -> Option<image::DynamicImage> {
    let (w, h) = picture_size(bytes)?;
    if w == 0 || h == 0 || w as u64 * h as u64 > MAX_PIXELS {
        return None;
    }
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes)).with_guessed_format().ok()?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(w);
    limits.max_image_height = Some(h);
    limits.max_alloc = Some(MAX_PIXELS * 8);
    reader.limits(limits);
    reader.decode().ok()
}

/// An image XObject from PNG or JPEG bytes: JPEG stays as it is, PNG becomes its pixels, alpha in a soft mask.
pub fn image_object(work_doc: &mut Document, bytes: &[u8]) -> Option<(ObjectId, u32, u32)> {
    if bytes.starts_with(&[0xFF, 0xD8]) {
        // the JPEG's own bytes go in: only its header is read, for the size and the colours
        let (comps, (w, h)) = {
            use image::ImageDecoder;
            let d = image::codecs::jpeg::JpegDecoder::new(std::io::Cursor::new(bytes)).ok()?;
            let comps = match d.color_type() {
                image::ColorType::L8 | image::ColorType::L16 => 1,
                _ => 3,
            };
            (comps, d.dimensions())
        };
        // CMYK JPEGs come back as RGB from the decoder: write the pixels instead of the original bytes
        let cmyk = jpeg_is_cmyk(bytes);
        if cmyk {
            let img = decode_picture(bytes)?;
            let (w, h) = (img.width(), img.height());
            let rgb = img.into_rgb8();
            let mut s = Stream::new(
                dictionary! { "Type" => "XObject", "Subtype" => "Image", "Width" => w as i64, "Height" => h as i64, "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8 },
                rgb.into_raw(),
            );
            let _ = s.compress();
            return Some((work_doc.add_object(s), w, h));
        }
        let s = Stream::new(
            dictionary! { "Type" => "XObject", "Subtype" => "Image", "Width" => w as i64, "Height" => h as i64, "ColorSpace" => if comps == 1 { "DeviceGray" } else { "DeviceRGB" }, "BitsPerComponent" => 8, "Filter" => "DCTDecode" },
            bytes.to_vec(),
        )
        .with_compression(false);
        return Some((work_doc.add_object(s), w, h));
    }
    let img = decode_picture(bytes)?;
    let (w, h) = (img.width(), img.height());
    let rgba = img.into_rgba8();
    let has_alpha = rgba.pixels().any(|p| p.0[3] != 255);
    let pixels = w as usize * h as usize;
    let mut rgb = limits::room(pixels * 3).ok()?;
    let mut alpha = limits::room(pixels).ok()?;
    for p in rgba.pixels() {
        rgb.extend_from_slice(&p.0[..3]);
        alpha.push(p.0[3]);
    }
    let mut d = dictionary! { "Type" => "XObject", "Subtype" => "Image", "Width" => w as i64, "Height" => h as i64, "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8 };
    if has_alpha {
        let mut m = Stream::new(
            dictionary! { "Type" => "XObject", "Subtype" => "Image", "Width" => w as i64, "Height" => h as i64, "ColorSpace" => "DeviceGray", "BitsPerComponent" => 8 },
            alpha,
        );
        let _ = m.compress();
        let mid = work_doc.add_object(m);
        d.set("SMask", Object::Reference(mid));
    }
    let mut s = Stream::new(d, rgb);
    let _ = s.compress();
    Some((work_doc.add_object(s), w, h))
}

fn jpeg_is_cmyk(b: &[u8]) -> bool {
    let mut i = 2;
    while i + 9 < b.len() {
        if b[i] != 0xFF {
            return false;
        }
        let m = b[i + 1];
        let len = ((b[i + 2] as usize) << 8) | b[i + 3] as usize;
        if (0xC0..=0xCF).contains(&m) && m != 0xC4 && m != 0xC8 && m != 0xCC {
            return b.get(i + 9) == Some(&4);
        }
        i += 2 + len;
    }
    false
}

fn data_url_bytes(u: &str) -> Option<Vec<u8>> {
    let comma = u.find(',')?;
    b64(&u[comma + 1..])
}

fn b64(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut buf = 0u32;
    let mut bits = 0;
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' | b'\n' | b'\r' | b' ' => continue,
            _ => return None,
        } as u32;
        buf = (buf << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Some(out)
}

const WHITE: [f32; 3] = [1.0, 1.0, 1.0];
const INK: &str = "#151413";

/// What the window drew and wrote on one page, put into the file.
fn apply_page(w: &mut Work, page: ObjectId, plan: &PagePlan) -> Result<(), Fail> {
    // the window's page coordinates (y down) to the page's user space
    let to_user =
        if plan.vt.len() == 6 { Mat::from(&plan.vt).invert() } else { Mat([1.0, 0.0, 0.0, -1.0, 0.0, plan.h]) };
    let to_view = to_user.invert();

    if let Some(blob) = &plan.raster {
        return raster_page(w, page, blob, plan, &to_user);
    }

    let mut overlay = String::new();
    let mut xobjects = Dictionary::new();
    let mut fonts_used: Vec<embed::Run> = Vec::new();

    // paragraphs that were changed or moved: the old words out of the content, the new ones drawn on top
    let edits: Vec<&Mark> = plan.marks.iter().filter(|m| matches!(m, Mark::Edit { .. })).collect();
    if !edits.is_empty() {
        let rects: Vec<[f32; 4]> = edits
            .iter()
            .map(|m| if let Mark::Edit { ox, oy, ow, oh, .. } = m { [*ox, *oy, *ow, *oh] } else { [0.0; 4] })
            .collect();
        let content = w.doc.get_page_content(page);
        let res = w.page_resources(page);
        let fonts =
            res.get(b"Font").map(|o| get(&w.doc, o)).ok().and_then(|o| o.as_dict().ok()).cloned().unwrap_or_default();
        let inside = |x: f32, y: f32| -> Option<usize> {
            let (vx, vy) = to_view.apply(x, y);
            rects.iter().position(|r| {
                vx >= r[0] - 1.0 && vx <= r[0] + r[2] + 1.0 && vy >= r[1] - 1.0 && vy <= r[1] + r[3] + 1.0
            })
        };
        let cut = textcut::cut(&w.doc, &content, &fonts, &inside, rects.len());
        let found = match cut {
            Some((bytes, found)) => {
                let mut s = Stream::new(dictionary! {}, bytes);
                let _ = s.compress();
                let id = w.add(s);
                w.page(page).set("Contents", Object::Reference(id));
                found
            }
            None => vec![0; rects.len()],
        };
        for (k, m) in edits.iter().enumerate() {
            let Mark::Edit { ox, oy, ow, oh, size, bold, serif, color, lines } = m else { continue };
            // words the content didn't give up (inside a form, say) are covered the way the window covers them
            if found[k] == 0 {
                overlay.push_str(&format!(
                    "{}{} {} {} {} re f\n",
                    color_ops(WHITE, false),
                    fmt(ox - 1.0),
                    fmt(oy - 1.0),
                    fmt(ow + 2.0),
                    fmt(oh + 3.0)
                ));
            }
            let style = Style { bold: *bold, serif: *serif };
            overlay.push_str(&color_ops(rgb(color), false));
            for l in lines {
                if l.text.trim().is_empty() {
                    continue;
                }
                let (ops, runs) = text_ops(
                    &mut w.fonts,
                    style,
                    *size,
                    l.x,
                    l.y,
                    l.text.trim_end(),
                    if l.justify { Some(l.width) } else { None },
                );
                overlay.push_str(&ops);
                fonts_used.extend(runs);
            }
        }
    }

    // signatures: the strokes as they were written, or the picture that was chosen
    for m in &plan.marks {
        let Mark::Sign { x, y, w: sw_box, h: sh_box, sw, sh, strokes, img } = m else { continue };
        if let Some(url) = img {
            let Some(bytes) = data_url_bytes(url) else { continue };
            let Some((id, _, _)) = image_object(&mut w.doc, &bytes) else { continue };
            w.touch(id);
            let name = w.name("Sig");
            xobjects.set(name.clone().into_bytes(), Object::Reference(id));
            overlay.push_str(&format!(
                "q {} 0 0 {} {} {} cm /{} Do Q\n",
                fmt(*sw_box),
                fmt(-sh_box),
                fmt(*x),
                fmt(y + sh_box),
                name
            ));
        } else {
            let k = sw_box / sw.max(1.0);
            let ky = sh_box / sh.max(1.0);
            let map = |px: f32, py: f32| (x + px * k, y + py * ky);
            overlay.push_str(&format!("q {}{} w 1 J 1 j ", color_ops(rgb(INK), true), fmt(2.4 * k)));
            for s in strokes {
                overlay.push_str(&smooth_path(s, &map));
                overlay.push_str("S ");
            }
            overlay.push_str("Q\n");
        }
    }

    if !overlay.is_empty() {
        let body = format!("Q q\n{}{}Q\n", mat_ops(&to_user), overlay);
        let mut start = Stream::new(dictionary! {}, b"q\n".to_vec());
        let _ = start.compress();
        let mut end = Stream::new(dictionary! {}, body.into_bytes());
        let _ = end.compress();
        let a = w.add(start);
        let b = w.add(end);
        let cur = w.doc.get_dictionary(page).ok().and_then(|d| d.get(b"Contents").ok()).cloned();
        let mut list = vec![Object::Reference(a)];
        match cur.as_ref().map(|o| get(&w.doc, o)) {
            Some(Object::Array(arr)) => list.extend(arr.iter().cloned()),
            Some(Object::Stream(_)) => list.push(cur.clone().unwrap()),
            _ => {}
        }
        list.push(Object::Reference(b));
        w.page(page).set("Contents", Object::Array(list));
        if !fonts_used.is_empty() {
            let f = w.fonts.resources(&mut w.doc, &fonts_used);
            w.own_resources(page, b"Font", f);
        }
        if !xobjects.is_empty() {
            w.own_resources(page, b"XObject", xobjects);
        }
    }

    // highlights, pen strokes and notes: annotations, each with its own appearance, so other readers show them as drawn
    let mut annots = Vec::new();
    let page_ref = Object::Reference(page);
    for m in &plan.marks {
        match m {
            Mark::Hl { color, rects } if !rects.is_empty() => {
                let c = rgb(color);
                // the window lays a 28% tint over the paper; on white that is this colour
                let tint = [1.0 - 0.28 * (1.0 - c[0]), 1.0 - 0.28 * (1.0 - c[1]), 1.0 - 0.28 * (1.0 - c[2])];
                let mut quads = Vec::new();
                let mut ap = format!("/DotAnyMul gs {}", color_ops(tint, false));
                let mut bb = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
                for r in rects {
                    let (x0, y0, x1, y1) = (r[0], r[1], r[0] + r[2], r[1] + r[3]);
                    for (px, py) in [(x0, y0), (x1, y0), (x0, y1), (x1, y1)] {
                        let (ux, uy) = to_user.apply(px, py);
                        quads.push(Object::Real(ux));
                        quads.push(Object::Real(uy));
                        bb = [bb[0].min(ux), bb[1].min(uy), bb[2].max(ux), bb[3].max(uy)];
                    }
                    let corners =
                        [to_user.apply(x0, y0), to_user.apply(x1, y0), to_user.apply(x1, y1), to_user.apply(x0, y1)];
                    ap.push_str(&format!(
                        "{} {} m {} {} l {} {} l {} {} l h f ",
                        fmt(corners[0].0),
                        fmt(corners[0].1),
                        fmt(corners[1].0),
                        fmt(corners[1].1),
                        fmt(corners[2].0),
                        fmt(corners[2].1),
                        fmt(corners[3].0),
                        fmt(corners[3].1)
                    ));
                }
                let rect = vec![Object::Real(bb[0]), Object::Real(bb[1]), Object::Real(bb[2]), Object::Real(bb[3])];
                let gs = dictionary! { "DotAnyMul" => dictionary! { "Type" => "ExtGState", "BM" => "Multiply" } };
                let form = Stream::new(
                    dictionary! { "Type" => "XObject", "Subtype" => "Form", "BBox" => rect.clone(), "Resources" => dictionary! { "ExtGState" => gs } },
                    ap.into_bytes(),
                );
                let fid = w.add(form);
                let nm = w.name("Hl");
                let a = dictionary! {
                    "Type" => "Annot", "Subtype" => "Highlight", "Rect" => rect, "QuadPoints" => quads, "C" => tint.iter().map(|v| Object::Real(*v)).collect::<Vec<_>>(),
                    "F" => 4, "P" => page_ref.clone(), "NM" => Object::string_literal(nm), "AP" => dictionary! { "N" => Object::Reference(fid) },
                };
                annots.push(w.add(a));
            }
            Mark::Brush { color, size, pts } if pts.len() >= 2 => {
                let c = rgb(color);
                let width = size * to_user.scale();
                let mut ink = Vec::new();
                let mut bb = [f32::MAX, f32::MAX, f32::MIN, f32::MIN];
                for p in pts.chunks(2) {
                    if p.len() < 2 {
                        break;
                    }
                    let (ux, uy) = to_user.apply(p[0], p[1]);
                    ink.push(Object::Real(ux));
                    ink.push(Object::Real(uy));
                    bb = [bb[0].min(ux), bb[1].min(uy), bb[2].max(ux), bb[3].max(uy)];
                }
                let pad = width / 2.0 + 1.0;
                let rect = vec![
                    Object::Real(bb[0] - pad),
                    Object::Real(bb[1] - pad),
                    Object::Real(bb[2] + pad),
                    Object::Real(bb[3] + pad),
                ];
                let map = |x: f32, y: f32| to_user.apply(x, y);
                let ap = format!("{}{} w 1 J 1 j {}S", color_ops(c, true), fmt(width), smooth_path(pts, &map));
                let form = Stream::new(
                    dictionary! { "Type" => "XObject", "Subtype" => "Form", "BBox" => rect.clone(), "Resources" => dictionary! {} },
                    ap.into_bytes(),
                );
                let fid = w.add(form);
                let nm = w.name("Ink");
                let a = dictionary! {
                    "Type" => "Annot", "Subtype" => "Ink", "Rect" => rect, "InkList" => vec![Object::Array(ink)], "C" => c.iter().map(|v| Object::Real(*v)).collect::<Vec<_>>(),
                    "BS" => dictionary! { "W" => Object::Real(width) }, "F" => 4, "P" => page_ref.clone(), "NM" => Object::string_literal(nm), "AP" => dictionary! { "N" => Object::Reference(fid) },
                };
                annots.push(w.add(a));
            }
            Mark::Note { x, y, words, size, bold, color } if !words.trim().is_empty() => {
                let style = Style { bold: *bold, serif: false };
                let width = w.fonts.width(style, words) * size;
                // the window centres the words on y + 0.6 of their size; the baseline sits a little below the middle
                let base = y + size * 0.6 + size * 0.36;
                let bb = to_user.bbox(*x, *y, width + 1.0, size * 1.2);
                let rect = vec![Object::Real(bb[0]), Object::Real(bb[1]), Object::Real(bb[2]), Object::Real(bb[3])];
                let (ops, runs) = text_ops(&mut w.fonts, style, *size, *x, base, words, None);
                let c = rgb(color);
                let ap = format!("q {}{}{}Q", mat_ops(&to_user), color_ops(c, false), ops);
                let fres = w.fonts.resources(&mut w.doc, &runs);
                let form = Stream::new(
                    dictionary! { "Type" => "XObject", "Subtype" => "Form", "BBox" => rect.clone(), "Resources" => dictionary! { "Font" => fres } },
                    ap.into_bytes(),
                );
                let fid = w.add(form);
                let nm = w.name("Note");
                let a = dictionary! {
                    "Type" => "Annot", "Subtype" => "FreeText", "Rect" => rect, "Contents" => text_string(words),
                    "DA" => Object::string_literal(format!("/Helv {} Tf {} {} {} rg", fmt(*size * to_user.scale()), fmt(c[0]), fmt(c[1]), fmt(c[2]))),
                    "F" => 4, "P" => page_ref.clone(), "NM" => Object::string_literal(nm), "BS" => dictionary! { "W" => 0 }, "AP" => dictionary! { "N" => Object::Reference(fid) },
                };
                annots.push(w.add(a));
            }
            _ => {}
        }
    }
    w.annots_push(page, annots);
    Ok(())
}

/// A page that had something redacted: its content, its annotations and its fields go; what's left is the picture the
/// window drew of it, with the covered part black. Nothing of the old page stays in the file.
fn raster_page(w: &mut Work, page: ObjectId, blob: &str, plan: &PagePlan, to_user: &Mat) -> Result<(), Fail> {
    let bytes = w.blobs.get(blob).ok_or_else(|| Fail::Other("raster missing".into()))?;
    let (img, _, _) = image_object(&mut w.doc, bytes).ok_or_else(|| Fail::Other("raster unreadable".into()))?;
    w.touch(img);
    // the visible box, the way PDF.js measured the page
    let bb = to_user.bbox(0.0, 0.0, plan.w, plan.h);
    let content =
        format!("q\n{}{} 0 0 {} 0 {} cm /DotAnyPage Do\nQ\n", mat_ops(to_user), fmt(plan.w), fmt(-plan.h), fmt(plan.h));
    let mut s = Stream::new(dictionary! {}, content.into_bytes());
    let _ = s.compress();
    let cid = w.add(s);
    let d = w.page(page);
    for k in [
        b"Annots".as_slice(),
        b"Group",
        b"Thumb",
        b"StructParents",
        b"PieceInfo",
        b"Tabs",
        b"AA",
        b"CropBox",
        b"B",
        b"Metadata",
    ] {
        d.remove(k);
    }
    d.set("MediaBox", vec![Object::Real(bb[0]), Object::Real(bb[1]), Object::Real(bb[2]), Object::Real(bb[3])]);
    d.set("Contents", Object::Reference(cid));
    d.set("Resources", dictionary! { "XObject" => dictionary! { "DotAnyPage" => Object::Reference(img) } });
    Ok(())
}

fn text_string(s: &str) -> Object {
    if s.is_ascii() && !s.contains(['(', ')', '\\']) {
        Object::string_literal(s)
    } else {
        let mut b = vec![0xFE, 0xFF];
        for u in s.encode_utf16() {
            b.extend_from_slice(&u.to_be_bytes());
        }
        Object::String(b, StringFormat::Hexadecimal)
    }
}

/// Every field in the form by its full name, as PDF.js names it, with its widgets. Two fields with the same name (some
/// programs write a radio group that way) are both there.
fn fields_by_name(doc: &Document, roots: &[Object]) -> BTreeMap<String, Vec<(ObjectId, Vec<ObjectId>)>> {
    let mut out = BTreeMap::new();
    fn walk(
        doc: &Document,
        id: ObjectId,
        prefix: &str,
        out: &mut BTreeMap<String, Vec<(ObjectId, Vec<ObjectId>)>>,
        depth: u32,
    ) {
        if depth > 32 {
            return;
        }
        let Ok(d) = doc.get_dictionary(id) else { return };
        let own = d.get(b"T").ok().and_then(|t| t.as_str().ok()).map(decode_text);
        let name = match (&own, prefix.is_empty()) {
            (Some(t), true) => t.clone(),
            (Some(t), false) => format!("{prefix}.{t}"),
            (None, _) => prefix.to_string(),
        };
        let kids: Vec<ObjectId> = d
            .get(b"Kids")
            .map(|k| get(doc, k))
            .ok()
            .and_then(|k| k.as_array().ok())
            .map(|a| a.iter().filter_map(|o| o.as_reference().ok()).collect())
            .unwrap_or_default();
        let kid_fields: Vec<ObjectId> =
            kids.iter().copied().filter(|k| doc.get_dictionary(*k).map(|kd| kd.has(b"T")).unwrap_or(false)).collect();
        if !kid_fields.is_empty() {
            for k in kid_fields {
                walk(doc, k, &name, out, depth + 1);
            }
            return;
        }
        let widgets = if kids.is_empty() { vec![id] } else { kids };
        if own.is_some() {
            out.entry(name).or_default().push((id, widgets));
        }
    }
    for r in roots {
        if let Ok(id) = r.as_reference() {
            walk(doc, id, "", &mut out, 0);
        }
    }
    out
}

fn decode_text(b: &[u8]) -> String {
    if b.starts_with(&[0xFE, 0xFF]) {
        let u: Vec<u16> = b[2..].chunks(2).filter(|c| c.len() == 2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
        String::from_utf16_lossy(&u)
    } else if b.starts_with(&[0xEF, 0xBB, 0xBF]) {
        String::from_utf8_lossy(&b[3..]).into_owned()
    } else {
        b.iter().map(|c| *c as char).collect()
    }
}

/// A field's value, in its widgets' appearance too, so every reader shows what was typed.
fn set_field(w: &mut Work, field: ObjectId, widgets: &[ObjectId], value: &serde_json::Value) {
    let inherited = |w: &Work, key: &[u8]| -> Option<Object> {
        let mut at = Some(field);
        for _ in 0..32 {
            let d = w.doc.get_dictionary(at?).ok()?;
            if let Ok(v) = d.get(key) {
                return Some(v.clone());
            }
            at = d.get(b"Parent").and_then(|p| p.as_reference()).ok();
        }
        None
    };
    let ft = inherited(w, b"FT").and_then(|o| o.as_name().ok().map(|n| n.to_vec())).unwrap_or_default();
    let flags = inherited(w, b"Ff").and_then(|o| o.as_i64().ok()).unwrap_or(0);
    if ft == b"Btn" {
        // a check box's "on" name is whatever its own appearance calls it; a radio button's is its export value
        let radio = flags & (1 << 15) != 0;
        let want = value.as_str().map(|s| s.to_string());
        let on = value.as_bool().unwrap_or(false);
        let mut chosen: Option<Vec<u8>> = None;
        for wid in widgets {
            let names: Vec<Vec<u8>> = w
                .doc
                .get_dictionary(*wid)
                .ok()
                .and_then(|d| d.get(b"AP").ok())
                .map(|o| get(&w.doc, o))
                .and_then(|o| o.as_dict().ok())
                .and_then(|ap| ap.get(b"N").ok())
                .map(|o| get(&w.doc, o))
                .and_then(|o| o.as_dict().ok())
                .map(|n| n.iter().map(|(k, _)| k.clone()).filter(|k| k != b"Off").collect())
                .unwrap_or_default();
            let mine = names.first().cloned().unwrap_or_else(|| b"Yes".to_vec());
            let set_on =
                if radio { want.as_deref().map(|v| v.as_bytes() == mine.as_slice()).unwrap_or(false) } else { on };
            if set_on {
                chosen = Some(mine.clone());
            }
            w.touch(*wid);
            if let Ok(d) = w.doc.get_dictionary_mut(*wid) {
                d.set("AS", Object::Name(if set_on { mine } else { b"Off".to_vec() }));
            }
        }
        w.touch(field);
        if let Ok(d) = w.doc.get_dictionary_mut(field) {
            d.set("V", Object::Name(chosen.unwrap_or_else(|| b"Off".to_vec())));
        }
        return;
    }
    let text = match value {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Bool(b) => {
            if *b {
                "Yes".into()
            } else {
                String::new()
            }
        }
        other => other.to_string(),
    };
    w.touch(field);
    if let Ok(d) = w.doc.get_dictionary_mut(field) {
        d.set("V", text_string(&text));
    }
    // the appearance: the family's font, the field's own size and colour from its DA
    let da = inherited(w, b"DA")
        .and_then(|o| o.as_str().ok().map(|s| String::from_utf8_lossy(s).into_owned()))
        .unwrap_or_default();
    let tokens: Vec<&str> = da.split_whitespace().collect();
    let size_da = tokens
        .iter()
        .position(|t| *t == "Tf")
        .and_then(|i| tokens.get(i.wrapping_sub(1)))
        .and_then(|s| s.parse::<f32>().ok())
        .unwrap_or(0.0);
    let color = if let Some(i) = tokens.iter().position(|t| *t == "rg") {
        [tokens.get(i.wrapping_sub(3)), tokens.get(i.wrapping_sub(2)), tokens.get(i.wrapping_sub(1))]
            .map(|t| t.and_then(|s| s.parse::<f32>().ok()).unwrap_or(0.0))
    } else if let Some(i) = tokens.iter().position(|t| *t == "g") {
        let g = tokens.get(i.wrapping_sub(1)).and_then(|s| s.parse::<f32>().ok()).unwrap_or(0.0);
        [g, g, g]
    } else {
        [0.0; 3]
    };
    let quad = inherited(w, b"Q").and_then(|o| o.as_i64().ok()).unwrap_or(0);
    let multiline = flags & (1 << 12) != 0;
    for wid in widgets {
        let Some(rect) = w
            .doc
            .get_dictionary(*wid)
            .ok()
            .and_then(|d| d.get(b"Rect").ok())
            .map(|o| get(&w.doc, o))
            .and_then(|o| o.as_array().ok())
            .map(|a| a.iter().map(|v| v.as_float().unwrap_or(0.0)).collect::<Vec<_>>())
        else {
            continue;
        };
        if rect.len() != 4 {
            continue;
        }
        let (bw, bh) = ((rect[2] - rect[0]).abs(), (rect[3] - rect[1]).abs());
        let size = if size_da > 0.0 { size_da } else { (bh * 0.62).clamp(4.0, 12.0) };
        let style = Style { bold: false, serif: false };
        let mut body = String::new();
        let mut runs_all = Vec::new();
        let lines: Vec<&str> = if multiline { text.split('\n').collect() } else { vec![text.as_str()] };
        for (k, line) in lines.iter().enumerate() {
            let tw = w.fonts.width(style, line) * size;
            let x = match quad {
                1 => (bw - tw) / 2.0,
                2 => bw - 2.0 - tw,
                _ => 2.0,
            };
            // y down inside the box, so the same text_ops serve
            let base = if multiline { 2.0 + size * (k as f32 + 0.9) } else { (bh + size * 0.7) / 2.0 };
            let (ops, runs) = text_ops(&mut w.fonts, style, size, x, base, line, None);
            body.push_str(&ops);
            runs_all.extend(runs);
        }
        let ap = format!(
            "/Tx BMC q 1 1 {} {} re W n 1 0 0 -1 0 {} cm {}{}Q EMC",
            fmt(bw - 2.0),
            fmt(bh - 2.0),
            fmt(bh),
            color_ops(color, false),
            body
        );
        let fres = w.fonts.resources(&mut w.doc, &runs_all);
        let form = Stream::new(
            dictionary! { "Type" => "XObject", "Subtype" => "Form", "BBox" => vec![0.into(), 0.into(), Object::Real(bw), Object::Real(bh)], "Resources" => dictionary! { "Font" => fres } },
            ap.into_bytes(),
        );
        let fid = w.add(form);
        w.touch(*wid);
        if let Ok(d) = w.doc.get_dictionary_mut(*wid) {
            d.set("AP", dictionary! { "N" => Object::Reference(fid) });
        }
    }
}

/// The plan written as a file. `sources` holds the open file (0) and anything added; `blobs` the pictures the window drew.
pub fn build(
    plan: &Plan,
    sources: &HashMap<u32, Source>,
    blobs: &HashMap<String, Vec<u8>>,
    files: &embed::Files,
) -> Result<Vec<u8>, Fail> {
    let main = sources.get(&0).ok_or_else(|| Fail::Other("nothing open".into()))?;
    let original = load(main)?;
    let main_pages = original.get_pages();
    let state = original.encryption_state.clone();

    let same_pages = plan.size == "same"
        && plan.pages.len() == main_pages.len()
        && plan
            .pages
            .iter()
            .enumerate()
            .all(|(i, p)| p.src == 0 && p.n as usize == i + 1 && !p.blank && p.raster.is_none());
    let untouched = same_pages
        && plan.fields.is_empty()
        && plan.pages.iter().all(|p| p.marks.is_empty() && p.rot.rem_euclid(360) == 0);
    if untouched {
        return Ok(main.bytes.to_vec());
    }
    // An update added after the original bytes leans on the original's own cross-reference table: its /Prev points
    // there for every object it doesn't repeat. A table lopdf had to rebuild by scanning (xref_start 0) or correct
    // (wrong offsets) can't be leaned on, so such a file is written whole instead.
    let in_place = same_pages && original.xref_start != 0 && original.xref_recovery.is_none();

    let mut w = Work { doc: original.clone(), dirty: BTreeSet::new(), fonts: Fonts::new(files), blobs, serial: 0 };
    w.serial =
        (std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u32).unwrap_or(0))
            % 100000;

    // ---------- in place: the original bytes, then what changed ----------
    if in_place {
        for (i, p) in plan.pages.iter().enumerate() {
            let id = main_pages[&((i + 1) as u32)];
            if p.rot.rem_euclid(360) != 0 {
                flatten_page(&mut w.doc, id);
                let was = w
                    .doc
                    .get_dictionary(id)
                    .ok()
                    .and_then(|d| d.get(b"Rotate").ok())
                    .and_then(|o| o.as_i64().ok())
                    .unwrap_or(0);
                w.page(id).set("Rotate", (was + p.rot as i64).rem_euclid(360));
            }
            if !p.marks.is_empty() {
                if p.marks.iter().any(|m| matches!(m, Mark::Edit { .. })) {
                    flatten_page(&mut w.doc, id);
                    w.touch(id);
                }
                apply_page(&mut w, id, p)?;
            }
        }
        apply_fields(&mut w, plan, &HashMap::new())?;
        w.fonts.finish(&mut w.doc).map_err(Fail::Other)?.into_iter().for_each(|id| {
            w.dirty.insert(id);
        });
        // the fonts' own objects were added straight to the document
        let max = w.doc.max_id;
        let mut inc = IncrementalDocument::create_from(main.bytes.to_vec(), original);
        let fresh: Vec<ObjectId> =
            w.doc.objects.keys().copied().filter(|id| id.0 > inc.get_prev_documents().max_id).collect();
        for id in w.dirty.iter().chain(fresh.iter()) {
            if let Some(o) = w.doc.objects.get(id) {
                inc.new_document.objects.insert(*id, o.clone());
            }
        }
        inc.new_document.max_id = max;
        let mut out = Vec::with_capacity(main.bytes.len() + 64 * 1024);
        inc.save_to(&mut out).map_err(|e| Fail::Other(e.to_string()))?;
        return Ok(out);
    }

    // ---------- a new file: pages gathered, ordered, everything unused left out ----------
    let mut by_src: HashMap<u32, BTreeMap<u32, ObjectId>> = HashMap::new();
    by_src.insert(0, main_pages.clone());
    for id in main_pages.values() {
        flatten_page(&mut w.doc, *id);
    }
    let mut extra_fields: HashMap<u32, Vec<Object>> = HashMap::new();
    let wanted: BTreeSet<u32> = plan.pages.iter().map(|p| p.src).filter(|s| *s != 0).collect();
    for s in wanted {
        let Some(src) = sources.get(&s) else { continue };
        if !matches!(src.kind, Kind::Pdf) {
            continue;
        }
        let mut other = load(src)?;
        other.renumber_objects_with(w.doc.max_id + 1);
        let pages = other.get_pages();
        for id in pages.values() {
            flatten_page(&mut other, *id);
        }
        if let Ok(af) = other.catalog().and_then(|c| c.get(b"AcroForm")).map(|o| get(&other, o).clone()) {
            if let Ok(f) = af.as_dict().and_then(|d| d.get(b"Fields")).map(|o| get(&other, o).clone()) {
                extra_fields.insert(s, f.as_array().cloned().unwrap_or_default());
            }
        }
        w.doc.max_id = other.max_id;
        w.doc.objects.extend(other.objects);
        by_src.insert(s, pages);
    }

    let root_pages = w.doc.catalog()?.get(b"Pages").and_then(|o| o.as_reference())?;
    let mut order: Vec<ObjectId> = Vec::new();
    let mut used: BTreeSet<ObjectId> = BTreeSet::new();
    for p in &plan.pages {
        let src = sources.get(&p.src);
        let id = if p.blank {
            let d = dictionary! { "Type" => "Page", "MediaBox" => vec![0.into(), 0.into(), Object::Real(p.w), Object::Real(p.h)], "Resources" => dictionary! {}, "Contents" => Vec::<Object>::new() };
            w.add(d)
        } else if let Some(Source { kind: Kind::Image, bytes, .. }) = src {
            let (img, _, _) = image_object(&mut w.doc, bytes).ok_or_else(|| Fail::Damaged("picture".into()))?;
            let content = format!("q {} 0 0 {} 0 0 cm /DotAnyPicture Do Q\n", fmt(p.w), fmt(p.h));
            let mut s = Stream::new(dictionary! {}, content.into_bytes());
            let _ = s.compress();
            let cid = w.add(s);
            let d = dictionary! { "Type" => "Page", "MediaBox" => vec![0.into(), 0.into(), Object::Real(p.w), Object::Real(p.h)], "Resources" => dictionary! { "XObject" => dictionary! { "DotAnyPicture" => Object::Reference(img) } }, "Contents" => Object::Reference(cid) };
            w.add(d)
        } else {
            let Some(id) = by_src.get(&p.src).and_then(|m| m.get(&p.n)).copied() else {
                return Err(Fail::Other(format!("no page {} in {}", p.n, p.src)));
            };
            if used.contains(&id) {
                // the same page twice: the second is a copy of its dictionary
                let d = dict_of(&w.doc, id);
                w.add(d)
            } else {
                id
            }
        };
        used.insert(id);
        if p.rot.rem_euclid(360) != 0 {
            let was = w
                .doc
                .get_dictionary(id)
                .ok()
                .and_then(|d| d.get(b"Rotate").ok())
                .and_then(|o| o.as_i64().ok())
                .unwrap_or(0);
            w.page(id).set("Rotate", (was + p.rot as i64).rem_euclid(360));
        }
        order.push(id);
    }

    // pages no longer in the file: every reference to them goes, so nothing of theirs is kept
    let gone: BTreeSet<ObjectId> =
        by_src.values().flat_map(|m| m.values().copied()).filter(|id| !used.contains(id)).collect();
    if !gone.is_empty() {
        for o in w.doc.objects.values_mut() {
            null_refs(o, &gone);
        }
        for id in &gone {
            w.doc.objects.remove(id);
        }
    }

    // one page tree, in the new order
    for id in &order {
        w.page(*id).set("Parent", Object::Reference(root_pages));
    }
    w.doc.objects.insert(root_pages, Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => order.iter().map(|i| Object::Reference(*i)).collect::<Vec<_>>(), "Count" => order.len() as i64 }));

    // the form: fields from added PDFs join; widgets on pages that went are dropped
    if !extra_fields.is_empty() {
        let cat = w.doc.catalog()?.clone();
        let mut af =
            cat.get(b"AcroForm").map(|o| get(&w.doc, o).as_dict().cloned().unwrap_or_default()).unwrap_or_default();
        let mut list =
            af.get(b"Fields").map(|o| get(&w.doc, o).as_array().cloned().unwrap_or_default()).unwrap_or_default();
        for f in extra_fields.values() {
            list.extend(f.iter().cloned());
        }
        af.set("Fields", list);
        if !af.has(b"DA") {
            af.set("DA", Object::string_literal("/Helv 0 Tf 0 g"));
        }
        let root = w.doc.trailer.get(b"Root").and_then(|o| o.as_reference())?;
        w.doc.get_dictionary_mut(root)?.set("AcroForm", af);
    }
    keep_live_fields(&mut w.doc, &order);

    for (i, p) in plan.pages.iter().enumerate() {
        if !p.marks.is_empty() || p.raster.is_some() {
            apply_page(&mut w, order[i], p)?;
        }
    }
    let field_src: HashMap<u32, Vec<Object>> = extra_fields;
    apply_fields(&mut w, plan, &field_src)?;

    // a redaction: nothing that could still say what was under it (tagged structure can carry the words as text)
    if plan.pages.iter().any(|p| p.raster.is_some()) {
        let root = w.doc.trailer.get(b"Root").and_then(|o| o.as_reference())?;
        let cat = w.doc.get_dictionary_mut(root)?;
        cat.remove(b"StructTreeRoot");
        cat.remove(b"MarkInfo");
        for id in &order {
            if let Ok(d) = w.doc.get_dictionary_mut(*id) {
                d.remove(b"StructParents");
            }
        }
    }

    w.fonts.finish(&mut w.doc).map_err(Fail::Other)?;
    w.doc.prune_objects();
    if plan.size != "same" {
        shrink::pictures(&mut w.doc, if plan.size == "least" { shrink::LEAST } else { shrink::SMALL });
    }
    w.doc.compress();
    if let Some(s) = state {
        w.doc.encrypt(&s)?;
    }
    let mut out = Vec::with_capacity(main.bytes.len());
    if plan.size == "same" {
        w.doc.save_to(&mut out).map_err(|e| Fail::Other(e.to_string()))?;
    } else {
        w.doc.save_modern(&mut out).map_err(|e| Fail::Other(e.to_string()))?;
    }
    Ok(out)
}

fn null_refs(o: &mut Object, gone: &BTreeSet<ObjectId>) {
    match o {
        Object::Reference(id) if gone.contains(id) => *o = Object::Null,
        Object::Array(a) => a.iter_mut().for_each(|x| null_refs(x, gone)),
        Object::Dictionary(d) => d.iter_mut().for_each(|(_, x)| null_refs(x, gone)),
        Object::Stream(s) => s.dict.iter_mut().for_each(|(_, x)| null_refs(x, gone)),
        _ => {}
    }
}

/// Only fields with a widget on a page that's still here stay in the form.
fn keep_live_fields(doc: &mut Document, pages: &[ObjectId]) {
    let mut live: BTreeSet<ObjectId> = BTreeSet::new();
    for p in pages {
        if let Ok(a) = doc.get_dictionary(*p).and_then(|d| d.get(b"Annots")).map(|o| get(doc, o).clone()) {
            for x in a.as_array().cloned().unwrap_or_default() {
                if let Ok(id) = x.as_reference() {
                    live.insert(id);
                }
            }
        }
    }
    let Ok(root) = doc.trailer.get(b"Root").and_then(|o| o.as_reference()) else { return };
    let Ok(af) = doc.get_dictionary(root).and_then(|c| c.get(b"AcroForm")).map(|o| get(doc, o).clone()) else { return };
    let Ok(mut af) = af.as_dict().cloned() else { return };
    let fields = af.get(b"Fields").map(|o| get(doc, o).as_array().cloned().unwrap_or_default()).unwrap_or_default();
    fn alive(doc: &mut Document, id: ObjectId, live: &BTreeSet<ObjectId>, depth: u32) -> bool {
        if depth > 32 {
            return false;
        }
        let Ok(d) = doc.get_dictionary(id) else { return false };
        let kids: Vec<Object> =
            d.get(b"Kids").map(|k| get(doc, k).as_array().cloned().unwrap_or_default()).unwrap_or_default();
        if kids.is_empty() {
            return live.contains(&id);
        }
        let keep: Vec<Object> = kids
            .into_iter()
            .filter(|k| k.as_reference().map(|kid| alive(doc, kid, live, depth + 1)).unwrap_or(false))
            .collect();
        let any = !keep.is_empty();
        if let Ok(d) = doc.get_dictionary_mut(id) {
            d.set("Kids", keep);
        }
        any
    }
    let kept: Vec<Object> =
        fields.into_iter().filter(|f| f.as_reference().map(|id| alive(doc, id, &live, 0)).unwrap_or(false)).collect();
    af.set("Fields", kept);
    if let Ok(c) = doc.get_dictionary_mut(root) {
        c.set("AcroForm", af);
    }
}

fn apply_fields(w: &mut Work, plan: &Plan, extra: &HashMap<u32, Vec<Object>>) -> Result<(), Fail> {
    if plan.fields.is_empty() {
        return Ok(());
    }
    let main_roots: Vec<Object> = w
        .doc
        .catalog()
        .ok()
        .and_then(|c| c.get(b"AcroForm").ok())
        .map(|o| get(&w.doc, o).clone())
        .and_then(|a| {
            a.as_dict()
                .ok()
                .and_then(|d| d.get(b"Fields").ok())
                .map(|o| get(&w.doc, o).as_array().cloned().unwrap_or_default())
        })
        .unwrap_or_default();
    let mut maps: HashMap<u32, BTreeMap<String, Vec<(ObjectId, Vec<ObjectId>)>>> = HashMap::new();
    for f in &plan.fields {
        let map = maps.entry(f.src).or_insert_with(|| {
            if f.src == 0 {
                fields_by_name(&w.doc, &main_roots)
            } else {
                fields_by_name(&w.doc, extra.get(&f.src).map(|v| v.as_slice()).unwrap_or(&[]))
            }
        });
        let Some(found) = map.get(&f.name).cloned() else { continue };
        // one group's widgets together, so a radio button turns the others off
        let widgets: Vec<ObjectId> = found.iter().flat_map(|(_, w)| w.iter().copied()).collect();
        for (id, _) in &found {
            set_field(w, *id, &widgets, &f.value);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The engine on its own, for working on it without the window: DOTANY_PDF_PLAN names a JSON file with
    /// { "sources": { "0": "path.pdf", "1": "picture.png" }, "passwords": { "0": "…" }, "blobs": { "id": "path" },
    ///   "out": "path.pdf", "plan": { … } }. Run: cargo test --lib plan_from_file -- --ignored --nocapture
    #[test]
    #[ignore]
    fn plan_from_file() {
        let Ok(path) = std::env::var("DOTANY_PDF_PLAN") else { return };
        let job: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        let mut sources = HashMap::new();
        for (k, v) in job["sources"].as_object().unwrap() {
            let bytes = std::fs::read(v.as_str().unwrap()).unwrap();
            let kind = if bytes.starts_with(b"%PDF") { Kind::Pdf } else { Kind::Image };
            let password = job["passwords"][k].as_str().map(|s| s.to_string());
            sources.insert(k.parse::<u32>().unwrap(), Source { bytes: Arc::new(bytes), kind, password });
        }
        let mut blobs = HashMap::new();
        if let Some(b) = job["blobs"].as_object() {
            for (k, v) in b {
                blobs.insert(k.clone(), std::fs::read(v.as_str().unwrap()).unwrap());
            }
        }
        let fonts_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../shared/fonts");
        let mut files = embed::Files::new();
        for (k, f) in [
            ("latin", "StackSansText-wght.ttf"),
            ("sans", "HarmonyOS_SansSC_Regular.ttf"),
            ("sans-bold", "HarmonyOS_SansSC_Bold.ttf"),
        ] {
            files.insert(k.to_string(), Arc::new(std::fs::read(fonts_dir.join(f)).unwrap()));
        }
        let plan: Plan = serde_json::from_value(job["plan"].clone()).unwrap();
        let t = std::time::Instant::now();
        match build(&plan, &sources, &blobs, &files) {
            Ok(bytes) => {
                std::fs::write(job["out"].as_str().unwrap(), &bytes).unwrap();
                println!("wrote {} bytes in {} ms", bytes.len(), t.elapsed().as_millis());
            }
            Err(e) => panic!("{e:?}"),
        }
    }
}

#[cfg(test)]
mod guard_tests {
    use super::*;

    fn small_pdf() -> Vec<u8> {
        let mut doc = Document::with_version("1.5");
        let pages_id = doc.new_object_id();
        let content = doc.add_object(Stream::new(dictionary! {}, b"0 0 m 100 100 l S".to_vec()));
        let page = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id, "Contents" => content,
            "MediaBox" => vec![0.into(), 0.into(), 200.into(), 200.into()],
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1 }),
        );
        let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalog);
        let mut out = Vec::new();
        doc.save_to(&mut out).unwrap();
        out
    }

    /// The same file with its `startxref` pointing nowhere, as a damaged download or a careless tool leaves it: readers
    /// that rebuild the table (PDF.js, lopdf) still open it.
    fn damaged(pdf: &[u8]) -> Vec<u8> {
        let at = pdf.windows(9).rposition(|w| w == b"startxref").unwrap() + 9;
        let digits_from = at + pdf[at..].iter().position(|b| b.is_ascii_digit()).unwrap();
        let digits_to = digits_from + pdf[digits_from..].iter().position(|b| !b.is_ascii_digit()).unwrap();
        [&pdf[..digits_from], b"99999999".as_slice(), &pdf[digits_to..]].concat()
    }

    fn turned(bytes: Vec<u8>) -> Result<Vec<u8>, Fail> {
        let plan: Plan = serde_json::from_value(
            serde_json::json!({ "pages": [{ "src": 0, "n": 1, "w": 200, "h": 200, "rot": 90 }] }),
        )
        .unwrap();
        let mut sources = HashMap::new();
        sources.insert(0, Source { bytes: Arc::new(bytes), kind: Kind::Pdf, password: None });
        build(&plan, &sources, &HashMap::new(), &embed::Files::new())
    }

    #[test]
    fn a_sound_file_is_still_saved_in_place() {
        let pdf = small_pdf();
        let out = turned(pdf.clone()).unwrap();
        assert!(out.starts_with(&pdf), "the original bytes come first");
        assert!(out.len() > pdf.len());
    }

    #[test]
    fn a_file_whose_xref_was_rebuilt_is_written_whole() {
        let bad = damaged(&small_pdf());
        let read = Document::load_mem(&bad).unwrap();
        assert!(read.xref_start == 0 || read.xref_recovery.is_some(), "lopdf had to repair it");
        let out = turned(bad.clone()).unwrap();
        assert!(!out.starts_with(&bad), "not appended to the damaged bytes");
        // written whole, its own table is sound: read again without any repair, the page turned
        let again = Document::load_mem(&out).unwrap();
        assert!(again.xref_start != 0 && again.xref_recovery.is_none(), "the new file's table is its own");
        let page = again.get_pages()[&1];
        assert_eq!(again.get_dictionary(page).unwrap().get(b"Rotate").unwrap().as_i64().unwrap(), 90);
    }

    fn crc32(data: &[u8]) -> u32 {
        let mut c = 0xFFFF_FFFFu32;
        for b in data {
            c ^= *b as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { (c >> 1) ^ 0xEDB8_8320 } else { c >> 1 };
            }
        }
        !c
    }

    fn chunk(kind: &[u8; 4], body: &[u8]) -> Vec<u8> {
        let mut out = (body.len() as u32).to_be_bytes().to_vec();
        out.extend_from_slice(kind);
        out.extend_from_slice(body);
        out.extend_from_slice(&crc32(&[kind.as_slice(), body].concat()).to_be_bytes());
        out
    }

    #[test]
    fn a_png_claiming_ten_billion_pixels_is_refused_before_decoding() {
        // a real PNG header for 100000 x 100000 RGBA, then one small compressed row: 70 GB if believed
        let mut ihdr = Vec::new();
        ihdr.extend_from_slice(&100_000u32.to_be_bytes());
        ihdr.extend_from_slice(&100_000u32.to_be_bytes());
        ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
        let png = [
            b"\x89PNG\r\n\x1a\n".as_slice(),
            &chunk(b"IHDR", &ihdr),
            &chunk(b"IDAT", &[0x78, 0x9C, 0x03, 0x00, 0x00, 0x00, 0x00, 0x01]),
            &chunk(b"IEND", &[]),
        ]
        .concat();
        assert_eq!(picture_size(&png), Some((100_000, 100_000)), "the header itself reads");
        let t = std::time::Instant::now();
        assert!(image_object(&mut Document::with_version("1.5"), &png).is_none());
        assert!(t.elapsed().as_secs() < 2);
    }

    #[test]
    fn an_ordinary_png_still_goes_in() {
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(3, 2, image::Rgba([10, 20, 30, 128])))
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let mut doc = Document::with_version("1.5");
        let (_, w, h) = image_object(&mut doc, &png).expect("placed");
        assert_eq!((w, h), (3, 2));
    }
}
