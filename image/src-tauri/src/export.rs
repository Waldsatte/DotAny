//! What a save will do, how big it will be, and doing it. Nothing here needs a window, so tests run it on real files.
//! Order of the pipeline: turn, crop, sliders, what was drawn, size. The same order as the preview.

use crate::decode::{Body, Picture};
use crate::encode;
use crate::exif;
use crate::formats::Kind;
use crate::metadata::{self, Meta};
use crate::ops::{self, Adjust, Geo, Image8, Rect, Tile};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Deserialize, Clone, Debug)]
pub struct SlotEdit {
    pub geo: Geo,
    pub crop: Option<Rect>,
    #[serde(default)]
    pub adj: Adjust,
    /// Something was drawn on this slot.
    #[serde(default)]
    pub marked: bool,
}

impl SlotEdit {
    pub fn none() -> SlotEdit {
        SlotEdit { geo: Geo::NONE, crop: None, adj: Adjust::default(), marked: false }
    }
    fn touches_pixels(&self) -> bool {
        self.crop.is_some() || !self.adj.is_zero() || self.marked
    }
    fn is_clean(&self) -> bool {
        self.geo == Geo::NONE && !self.touches_pixels()
    }
}

#[derive(Deserialize, Clone, Debug)]
pub struct Request {
    pub format: Kind,
    pub quality: u8,
    /// The person moved the quality slider: they want this re-encoded at that quality.
    #[serde(default)]
    pub quality_touched: bool,
    #[serde(default)]
    pub lossless: bool,
    #[serde(default)]
    pub strip: bool,
    /// Output size of the slot on screen.
    pub w: u32,
    pub h: u32,
    pub slot: usize,
    #[serde(default)]
    pub frame: usize,
    /// One per slot.
    pub edits: Vec<SlotEdit>,
}

pub type Marks = HashMap<usize, Vec<Tile>>;

#[derive(Debug, PartialEq, Clone, Copy, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Route {
    /// The original bytes: nothing would change them.
    Copy,
    /// The original picture data, without its details.
    Strip,
    /// A JPG only turned or flipped: the picture data is untouched, only its direction mark changes.
    Turn,
    /// Pixels to a new file.
    Encode,
}

fn natural_size(pic: &Picture, r: &Request) -> (u32, u32) {
    let e = r.edits.get(r.slot);
    let s = &pic.slots[r.slot.min(pic.slots.len() - 1)];
    match e {
        Some(e) => match e.crop {
            Some(c) => (c.w, c.h),
            None => e.geo.size(s.w, s.h),
        },
        None => (s.w, s.h),
    }
}

fn keeps_all_slots(format: Kind, pic: &Picture) -> bool {
    format == pic.kind && (pic.slots.len() == 1 || matches!(format, Kind::Tiff | Kind::Ico))
}

fn output_is_moving(pic: &Picture, format: Kind) -> bool {
    pic.is_moving() && matches!(format, Kind::Gif | Kind::Webp)
}

pub fn route(pic: &Picture, r: &Request) -> Route {
    let same_kind = r.format == pic.kind;
    let plain_size = (r.w, r.h) == natural_size(pic, r);
    let lossy_kept = !r.quality_touched || !matches!(r.format, Kind::Jpg | Kind::Webp);
    let lossless_kept = !(r.format == Kind::Webp && r.lossless);
    let all = keeps_all_slots(r.format, pic);
    if same_kind && all && plain_size && lossy_kept && lossless_kept && !r.edits.iter().any(|e| e.touches_pixels()) {
        if r.edits.iter().all(SlotEdit::is_clean) {
            if !r.strip || !pic.meta.has_details() {
                return Route::Copy;
            }
            if metadata::strip_in_place(pic.kind, &pic.source).is_some() {
                return Route::Strip;
            }
        } else if pic.kind == Kind::Jpg
            && metadata::jpeg_with_orientation(&pic.source, turned_orientation(pic, r), r.strip).is_some()
        {
            return Route::Turn;
        }
    }
    if pic.kind == Kind::Svg && r.format == Kind::Svg {
        return Route::Copy;
    }
    Route::Encode
}

fn turned_orientation(pic: &Picture, r: &Request) -> u8 {
    let turn = r.edits.first().map(|e| e.geo).unwrap_or(Geo::NONE);
    Geo::from_orientation(pic.orientation).then(turn).orientation()
}

/// What the output will carry. Removal leaves only the colour profile, which isn't about anybody.
fn carried(pic: &Picture, r: &Request, w: u32, h: u32) -> Meta {
    let can_hold_colour = matches!(r.format, Kind::Jpg | Kind::Png | Kind::Webp | Kind::Tiff);
    let rgb_profile = pic.meta.icc.as_ref().filter(|i| i.len() > 20 && &i[16..20] == b"RGB ");
    let icc = if can_hold_colour { rgb_profile.cloned() } else { None };
    if r.strip || !can_hold_colour {
        return Meta { icc, ..Default::default() };
    }
    let exif_block = pic.meta.exif.as_deref().and_then(|b| exif::after_pixels_changed(b, w, h));
    Meta {
        icc,
        exif: exif_block,
        xmp: pic.meta.xmp.clone(),
        iptc: if r.format == pic.kind { pic.meta.iptc.clone() } else { None },
        comment: None,
        png_text: if r.format == Kind::Png { pic.meta.png_text.clone() } else { Vec::new() },
        other: false,
    }
}

fn scale_for(pic: &Picture, r: &Request) -> (f64, f64) {
    let (nw, nh) = natural_size(pic, r);
    (r.w as f64 / nw as f64, r.h as f64 / nh as f64)
}

/// One slot through the whole pipeline, at the size asked for.
fn render(pic: &Picture, r: &Request, slot: usize, frame: usize, marks: &mut Marks) -> encode::Res<Image8> {
    let s = pic.slots.get(slot).ok_or("no such slot")?;
    let e = r.edits.get(slot).cloned().unwrap_or_else(SlotEdit::none);
    if !e.geo.is_valid() {
        return Err("bad turn".into());
    }
    let (gw, gh) = e.geo.size(s.w, s.h);
    let crop = e.crop.unwrap_or(Rect { x: 0, y: 0, w: gw, h: gh });
    if !crop.fits(gw, gh) {
        return Err("crop outside the picture".into());
    }
    let (mut img, at) = if matches!(s.body, Body::Drawing) {
        // the page rasterised the drawing at the size asked for
        (Image8::blank(r.w, r.h), (0, 0))
    } else {
        let src = pic.pixels(slot, frame).ok_or("could not read this page")?;
        let mut img = ops::place(&src.rgba, src.w, src.h, e.geo, crop);
        ops::adjust_pixels(&mut img.rgba, &e.adj);
        (img, (crop.x, crop.y))
    };
    if let Some(tiles) = marks.remove(&slot) {
        for mut t in tiles {
            if let Some(c) = &pic.colour {
                c.from_srgb(&mut t.rgba);
            }
            ops::lay_tile(&mut img, at, &t);
        }
    }
    let (sx, sy) = scale_for(pic, r);
    let (w, h) = (((img.w as f64 * sx).round() as u32).max(1), ((img.h as f64 * sy).round() as u32).max(1));
    if !matches!(s.body, Body::Drawing) && (w != img.w || h != img.h) {
        img = ops::resize(&img, w, h);
    }
    // formats with no way to say which colours these numbers mean get them converted to the one everyone assumes
    if !matches!(r.format, Kind::Jpg | Kind::Png | Kind::Webp | Kind::Tiff) {
        if let Some(c) = &pic.colour {
            c.to_srgb(&mut img.rgba);
        }
    }
    Ok(img)
}

fn moving_frame(pic: &Picture, r: &Request, frame: usize) -> encode::Res<Image8> {
    let mut none = Marks::new();
    render(pic, r, 0, frame, &mut none)
}

fn encode_moving(pic: &Picture, r: &Request, picks: &[usize], meta: &Meta) -> encode::Res<Vec<u8>> {
    let store = pic.frames(0).ok_or("no frames")?;
    let delays = store.delays();
    let first = moving_frame(pic, r, picks[0])?;
    let (w, h) = (first.w, first.h);
    let delay = |i: usize| delays.get(i).copied().unwrap_or(100);
    match r.format {
        Kind::Gif => {
            let mut enc = encode::GifAnim::new(w, h, store.loops)?;
            enc.add(&first.rgba, delay(picks[0]))?;
            drop(first);
            for &i in &picks[1..] {
                enc.add(&moving_frame(pic, r, i)?.rgba, delay(i))?;
            }
            enc.finish()
        }
        Kind::Webp => {
            let mut enc = encode::WebpAnim::new(w, h, store.loops, r.quality, r.lossless)?;
            enc.add(&first.rgba, delay(picks[0]))?;
            drop(first);
            for &i in &picks[1..] {
                enc.add(&moving_frame(pic, r, i)?.rgba, delay(i))?;
            }
            enc.finish(meta)
        }
        _ => Err("not a moving format".into()),
    }
}

fn encode_still(img: &Image8, r: &Request, m: &Meta, quick: bool) -> encode::Res<Vec<u8>> {
    match r.format {
        Kind::Jpg => encode::jpeg(img, r.quality, m),
        Kind::Png => encode::png(img, m, quick),
        Kind::Webp => encode::webp(img, r.quality, r.lossless, m),
        Kind::Bmp => encode::bmp(img),
        Kind::Gif => {
            let mut enc = encode::GifAnim::new(img.w, img.h, 1)?;
            enc.add(&img.rgba, 0)?;
            enc.finish()
        }
        Kind::Tiff => encode::tiff(std::iter::once(Ok(Image8 { w: img.w, h: img.h, rgba: img.rgba.clone() })), m),
        Kind::Ico => encode::ico(std::slice::from_ref(img)),
        Kind::Svg => Err("an SVG can't be written from pixels".into()),
        k => Err(format!("{} is only read, never written", k.label())),
    }
}

/// The finished bytes of a save. When removal was asked for, they are checked here before anyone sees them.
pub fn produce(pic: &Picture, r: &Request, marks: &mut Marks) -> encode::Res<Vec<u8>> {
    // a format we only read is never a save's target, not even as a plain copy
    if r.format.read_only() {
        return Err(format!("{} is only read, never written", r.format.label()).into());
    }
    let bytes = match route(pic, r) {
        Route::Copy => pic.source.as_ref().clone(),
        Route::Strip => metadata::strip_in_place(pic.kind, &pic.source).ok_or("could not remove details")?,
        Route::Turn => {
            metadata::jpeg_with_orientation(&pic.source, turned_orientation(pic, r), r.strip).ok_or("could not turn")?
        }
        Route::Encode => {
            if output_is_moving(pic, r.format) {
                let store = pic.frames(0).ok_or("no frames")?;
                let n = store.wait_all();
                let picks: Vec<usize> = (0..n).collect();
                let meta = carried(pic, r, r.w, r.h);
                encode_moving(pic, r, &picks, &meta)?
            } else if r.format == Kind::Tiff && pic.kind == Kind::Tiff && pic.slots.len() > 1 {
                let meta = carried(pic, r, r.w, r.h);
                let n = pic.slots.len();
                let mut marks_taken = std::mem::take(marks);
                let pages = (0..n).map(|i| render(pic, r, i, 0, &mut marks_taken));
                encode::tiff(pages, &meta)?
            } else if r.format == Kind::Ico && pic.kind == Kind::Ico {
                let mut all = Vec::with_capacity(pic.slots.len());
                for i in 0..pic.slots.len() {
                    all.push(render(pic, r, i, 0, marks)?);
                }
                encode::ico(&all)?
            } else {
                let img = render(pic, r, r.slot, r.frame, marks)?;
                let meta = carried(pic, r, img.w, img.h);
                encode_still(&img, r, &meta, false)?
            }
        }
    };
    if r.strip {
        let left = metadata::details_left(r.format, &bytes);
        if !left.is_empty() {
            return Err(format!("details still in the output: {}", left.join(", ")));
        }
    }
    Ok(bytes)
}

/// Whether what's on disk afterward is this picture in full, so the window can treat it as saved and move to it.
pub fn is_whole(pic: &Picture, r: &Request) -> bool {
    keeps_all_slots(r.format, pic)
}

// ---------- telling the person beforehand ----------

#[derive(Serialize, Debug)]
pub struct Plan {
    pub route: Route,
    pub size: u64,
    /// Whether `size` is the file's exact size.
    pub exact: bool,
    /// Keys for what this save loses, in the page's copy.
    pub notes: Vec<&'static str>,
}

pub fn notes(pic: &Picture, r: &Request, how: Route) -> Vec<&'static str> {
    let mut out = Vec::new();
    if how == Route::Turn {
        out.push("turned");
    }
    let slot = pic.slots.get(r.slot);
    let alpha = match slot.map(|s| &s.body) {
        Some(Body::Frames(f)) => f.alpha.load(std::sync::atomic::Ordering::Relaxed),
        _ => slot.is_some_and(|s| s.alpha),
    };
    if r.format == Kind::Jpg && alpha {
        out.push("jpg-alpha");
    }
    if pic.is_moving() && !output_is_moving(pic, r.format) {
        out.push("one-frame");
    }
    if pic.hidden_frames && how == Route::Encode {
        out.push("one-frame");
    }
    if pic.slots.len() > 1 && !keeps_all_slots(r.format, pic) {
        out.push(if pic.kind == Kind::Ico { "one-size" } else { "one-page" });
    }
    if r.format == Kind::Gif && pic.kind != Kind::Gif {
        out.push("gif-colors");
    }
    if how == Route::Encode && pic.slots.iter().any(|s| s.deep) {
        out.push("eight-bit");
    }
    if !r.strip
        && pic.meta.has_details()
        && !matches!(r.format, Kind::Jpg | Kind::Png | Kind::Webp | Kind::Tiff)
        && how == Route::Encode
    {
        out.push("no-details");
    }
    if pic.colour.is_some() && !matches!(r.format, Kind::Jpg | Kind::Png | Kind::Webp | Kind::Tiff) {
        out.push("to-srgb");
    }
    out
}

const WINDOW: u32 = 512;

/// An honest number without doing the whole job: real pixels through the real encoder. Up to a couple of megapixels the
/// whole picture; above that, six windows at the size they'll really have, and the result scaled by area.
pub fn estimate(pic: &Picture, r: &Request) -> encode::Res<Plan> {
    let how = route(pic, r);
    let note_list = notes(pic, r, how);
    let done = |size: usize, exact: bool| Ok(Plan { route: how, size: size as u64, exact, notes: note_list.clone() });
    match how {
        Route::Copy => return done(pic.source.len(), true),
        Route::Strip => {
            return done(
                metadata::strip_in_place(pic.kind, &pic.source).map(|b| b.len()).unwrap_or(pic.source.len()),
                true,
            );
        }
        Route::Turn => {
            return done(
                metadata::jpeg_with_orientation(&pic.source, turned_orientation(pic, r), r.strip)
                    .map(|b| b.len())
                    .unwrap_or(pic.source.len()),
                true,
            );
        }
        Route::Encode => {}
    }
    let s = pic.slots.get(r.slot).ok_or("no such slot")?;
    if matches!(s.body, Body::Drawing) {
        // an SVG is rasterised in the page, so a rough figure of the right size: PNG of mostly flat colour
        return done((r.w as usize * r.h as usize) / 4 + 512, false);
    }
    let meta = carried(pic, r, r.w, r.h);
    let meta_size = meta.icc.as_ref().map_or(0, Vec::len)
        + meta.exif.as_ref().map_or(0, Vec::len)
        + meta.xmp.as_ref().map_or(0, Vec::len);
    let e = r.edits.get(r.slot).cloned().unwrap_or_else(SlotEdit::none);
    let (gw, gh) = e.geo.size(s.w, s.h);
    let crop = e.crop.unwrap_or(Rect { x: 0, y: 0, w: gw, h: gh });

    if output_is_moving(pic, r.format) {
        let store = pic.frames(0).ok_or("no frames")?;
        store.wait_for(1);
        let n = if store.done() { store.count() } else { store.count().max(2) };
        let picks: Vec<usize> = if n <= 6 { (0..n).collect() } else { (0..6).map(|i| i * (n - 1) / 5).collect() };
        let bytes = encode_moving(pic, r, &picks, &meta)?;
        let per = bytes.len() as f64 / picks.len() as f64;
        return done((per * n as f64) as usize, n <= 6);
    }
    if r.format == Kind::Bmp {
        let channels = if s.alpha { 4usize } else { 3 };
        let row = (r.w as usize * channels).div_ceil(4) * 4;
        return done(row * r.h as usize + 122, true);
    }
    if r.format == Kind::Ico && pic.kind == Kind::Ico {
        let mut none = Marks::new();
        let mut all = Vec::new();
        for i in 0..pic.slots.len() {
            all.push(render(pic, r, i, 0, &mut none)?);
        }
        return done(encode::ico(&all)?.len(), false);
    }
    let pages = if r.format == Kind::Tiff && pic.kind == Kind::Tiff { pic.slots.len() } else { 1 };
    let pixels = r.w as u64 * r.h as u64;
    if pixels <= 2_000_000 {
        let mut none = Marks::new();
        let img = render(pic, r, r.slot, r.frame, &mut none)?;
        let size = encode_still(&img, r, &meta, false)?.len();
        return done(size * pages, pages == 1 && e.marked);
    }
    let src = pic.pixels(r.slot, r.frame).ok_or("could not read this page")?;
    let k = crop.w as f64 / r.w as f64;
    let win = ((WINDOW as f64 * k).round() as u32).clamp(1, crop.w.min(crop.h));
    let out_win = ((win as f64 / k).round() as u32).max(1);
    let (cols, rows) = (3u32, 2u32);
    let mut mosaic = Image8::blank(out_win * cols, out_win * rows);
    for row in 0..rows {
        for col in 0..cols {
            let x = crop.x + (crop.w - win) * (2 * col + 1) / (2 * cols);
            let y = crop.y + (crop.h - win) * (2 * row + 1) / (2 * rows);
            let mut part = ops::place(&src.rgba, src.w, src.h, e.geo, Rect { x, y, w: win, h: win });
            ops::adjust_pixels(&mut part.rgba, &e.adj);
            if win != out_win {
                part = ops::resize(&part, out_win, out_win);
            }
            let tile = Tile { x: col * out_win, y: row * out_win, w: out_win, h: out_win, rgba: part.rgba };
            ops::lay_tile_solid(&mut mosaic, &tile);
        }
    }
    let bare = Meta::default();
    let bytes = encode_still(&mosaic, r, &bare, false)?.len();
    let area = pixels as f64 / (mosaic.w as f64 * mosaic.h as f64);
    done(((bytes as f64 * area) as usize + meta_size) * pages, false)
}

/// A part of the picture for the screen when zoomed in further than the preview holds, through the same arithmetic as a save.
#[allow(clippy::too_many_arguments)]
pub fn region(
    pic: &Picture,
    slot: usize,
    frame: usize,
    geo: Geo,
    adj: &Adjust,
    rect: Rect,
    out_w: u32,
    out_h: u32,
) -> Option<Image8> {
    if !geo.is_valid() {
        return None;
    }
    let s = pic.slots.get(slot)?;
    let (gw, gh) = geo.size(s.w, s.h);
    if !rect.fits(gw, gh) {
        return None;
    }
    let src = pic.pixels(slot, frame)?;
    let mut img = ops::place(&src.rgba, src.w, src.h, geo, rect);
    ops::adjust_pixels(&mut img.rgba, adj);
    if (out_w, out_h) != (img.w, img.h) {
        img = ops::shrink_fast(&img.rgba, img.w, img.h, out_w, out_h);
    }
    if let Some(c) = &pic.colour {
        c.to_srgb(&mut img.rgba);
    }
    Some(img)
}
