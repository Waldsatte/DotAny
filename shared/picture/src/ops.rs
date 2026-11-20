//! What happens to pixels: turning and flipping, cropping, the six sliders, what was drawn on top, resizing.
//! The sliders here are the same arithmetic as Image's shader in `image/src/render.js`, so a saved file looks like the preview.

use fast_image_resize as fr;
use rayon::prelude::*;
use serde::Deserialize;

pub struct Image8 {
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

impl Image8 {
    pub fn blank(w: u32, h: u32) -> Self {
        Image8 { w, h, rgba: vec![0; w as usize * h as usize * 4] }
    }
    pub fn has_alpha(&self) -> bool {
        has_alpha(&self.rgba)
    }
}

pub fn has_alpha(rgba: &[u8]) -> bool {
    rgba.par_chunks(1 << 20).any(|c| c.chunks_exact(4).any(|p| p[3] != 255))
}

// ---------- turning and flipping ----------

/// A turn or flip as a matrix on centred coordinates: new = M * old. Only eight of these exist.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub struct Geo(pub [i8; 4]);

impl Geo {
    pub const NONE: Geo = Geo([1, 0, 0, 1]);
    pub const CW: Geo = Geo([0, -1, 1, 0]);
    pub const CCW: Geo = Geo([0, 1, -1, 0]);
    pub const FLIP_H: Geo = Geo([-1, 0, 0, 1]);
    pub const FLIP_V: Geo = Geo([1, 0, 0, -1]);

    pub fn then(self, next: Geo) -> Geo {
        let (a, b) = (next.0, self.0);
        Geo([
            a[0] * b[0] + a[1] * b[2],
            a[0] * b[1] + a[1] * b[3],
            a[2] * b[0] + a[3] * b[2],
            a[2] * b[1] + a[3] * b[3],
        ])
    }
    fn swaps_sides(self) -> bool {
        self.0[0] == 0
    }
    pub fn is_valid(self) -> bool {
        let m = self.0;
        let square = |v: i8| v == 0 || v == 1 || v == -1;
        m.iter().all(|v| square(*v)) && (m[0] * m[3] - m[1] * m[2]).abs() == 1 && (m[0] * m[1] + m[2] * m[3]) == 0
    }
    pub fn size(self, w: u32, h: u32) -> (u32, u32) {
        if self.swaps_sides() { (h, w) } else { (w, h) }
    }
    /// EXIF's number for what turns stored pixels into this.
    pub fn orientation(self) -> u8 {
        match self.0 {
            [1, 0, 0, 1] => 1,
            [-1, 0, 0, 1] => 2,
            [-1, 0, 0, -1] => 3,
            [1, 0, 0, -1] => 4,
            [0, 1, 1, 0] => 5,
            [0, -1, 1, 0] => 6,
            [0, -1, -1, 0] => 7,
            [0, 1, -1, 0] => 8,
            _ => 1,
        }
    }
    pub fn from_orientation(o: u8) -> Geo {
        Geo(match o {
            2 => [-1, 0, 0, 1],
            3 => [-1, 0, 0, -1],
            4 => [1, 0, 0, -1],
            5 => [0, 1, 1, 0],
            6 => [0, -1, 1, 0],
            7 => [0, -1, -1, 0],
            8 => [0, 1, -1, 0],
            _ => [1, 0, 0, 1],
        })
    }
    /// For the pixel at (x, y) of the result, where it comes from: (x0 + ax*x + ay*y, y0 + bx*x + by*y).
    fn walk(self, w: u32, h: u32) -> (i64, i64, i64, i64, i64, i64) {
        // the inverse of a matrix like this is its transpose
        let m = self.0;
        let (ax, ay, bx, by) = (m[0] as i64, m[2] as i64, m[1] as i64, m[3] as i64);
        let (nw, nh) = self.size(w, h);
        // the middle of each pixel, doubled, so everything stays whole
        let (cx, cy) = (nw as i64 - 1, nh as i64 - 1);
        let x0 = (ax * -cx + ay * -cy + (w as i64 - 1)) / 2;
        let y0 = (bx * -cx + by * -cy + (h as i64 - 1)) / 2;
        (x0, y0, ax, ay, bx, by)
    }
}

/// The turned picture, or just the part of it `crop` asks for. `crop` is in the turned picture's coordinates.
pub fn place(src: &[u8], w: u32, h: u32, geo: Geo, crop: Rect) -> Image8 {
    let mut out = Image8::blank(crop.w, crop.h);
    let (x0, y0, ax, ay, bx, by) = geo.walk(w, h);
    let stride = w as usize * 4;
    out.rgba.par_chunks_mut(crop.w as usize * 4).enumerate().for_each(|(row, line)| {
        let y = crop.y as i64 + row as i64;
        if geo == Geo::NONE {
            let at = y as usize * stride + crop.x as usize * 4;
            line.copy_from_slice(&src[at..at + line.len()]);
            return;
        }
        for (col, px) in line.chunks_exact_mut(4).enumerate() {
            let x = crop.x as i64 + col as i64;
            let sx = x0 + ax * x + ay * y;
            let sy = y0 + bx * x + by * y;
            let at = sy as usize * stride + sx as usize * 4;
            px.copy_from_slice(&src[at..at + 4]);
        }
    });
    out
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl Rect {
    pub fn fits(self, w: u32, h: u32) -> bool {
        self.w > 0
            && self.h > 0
            && self.x.checked_add(self.w).is_some_and(|r| r <= w)
            && self.y.checked_add(self.h).is_some_and(|b| b <= h)
    }
}

// ---------- the sliders ----------

/// Each from -100 to 100, zero meaning untouched.
#[derive(Clone, Copy, Debug, Default, PartialEq, Deserialize)]
pub struct Adjust {
    #[serde(default)]
    pub exposure: i32,
    #[serde(default)]
    pub contrast: i32,
    #[serde(default)]
    pub saturation: i32,
    #[serde(default)]
    pub temperature: i32,
    #[serde(default)]
    pub highlights: i32,
    #[serde(default)]
    pub shadows: i32,
}

impl Adjust {
    pub fn is_zero(&self) -> bool {
        *self == Adjust::default()
    }
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

const LUM: [f32; 3] = [0.2126, 0.7152, 0.0722];

pub fn adjust_pixels(rgba: &mut [u8], a: &Adjust) {
    if a.is_zero() {
        return;
    }
    let e = a.exposure as f32 / 100.0;
    let scale = 2f32.powf(e * 1.5);
    let up: Vec<f32> = (0..256).map(|i| ((i as f32 / 255.0).powf(2.2) * scale).powf(1.0 / 2.2)).collect();
    let k = a.temperature as f32 / 100.0 * 0.12;
    let c = a.contrast as f32 / 100.0;
    let contrast = if c >= 0.0 { 1.0 + c } else { 1.0 + c * 0.7 };
    let (hi, lo) = (a.highlights as f32 / 100.0 * 0.28, a.shadows as f32 / 100.0 * 0.28);
    let sat = 1.0 + a.saturation as f32 / 100.0;
    rgba.par_chunks_mut(1 << 18).for_each(|chunk| {
        for p in chunk.chunks_exact_mut(4) {
            let mut r = up[p[0] as usize] + k;
            let mut g = up[p[1] as usize];
            let mut b = up[p[2] as usize] - k;
            r = (r - 0.5) * contrast + 0.5;
            g = (g - 0.5) * contrast + 0.5;
            b = (b - 0.5) * contrast + 0.5;
            let l = LUM[0] * r + LUM[1] * g + LUM[2] * b;
            let add = hi * smoothstep(0.45, 1.0, l) + lo * (1.0 - smoothstep(0.0, 0.55, l));
            r += add;
            g += add;
            b += add;
            let l = LUM[0] * r + LUM[1] * g + LUM[2] * b;
            r = l + (r - l) * sat;
            g = l + (g - l) * sat;
            b = l + (b - l) * sat;
            p[0] = (r.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            p[1] = (g.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
            p[2] = (b.clamp(0.0, 1.0) * 255.0 + 0.5) as u8;
        }
    });
}

// ---------- what was drawn on top ----------

/// A piece of the drawing layer, rendered by the page at full size, at (x, y) in the turned picture.
pub struct Tile {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
    pub rgba: Vec<u8>,
}

/// Lays a tile over the picture, whose top-left is at `at` in the same coordinates.
pub fn lay_tile(img: &mut Image8, at: (u32, u32), tile: &Tile) {
    let (ox, oy) = (at.0 as i64, at.1 as i64);
    let (w, h) = (img.w as i64, img.h as i64);
    let (tx, ty) = (tile.x as i64 - ox, tile.y as i64 - oy);
    let (l, r) = (tx.max(0), (tx + tile.w as i64).min(w));
    let (t, b) = (ty.max(0), (ty + tile.h as i64).min(h));
    if l >= r || t >= b {
        return;
    }
    let stride = w as usize * 4;
    img.rgba.par_chunks_mut(stride).enumerate().skip(t as usize).take((b - t) as usize).for_each(|(y, line)| {
        let from = ((y as i64 - ty) as usize * tile.w as usize + (l - tx) as usize) * 4;
        let src = &tile.rgba[from..from + (r - l) as usize * 4];
        let dst = &mut line[l as usize * 4..r as usize * 4];
        for (d, s) in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
            let a = s[3] as u32;
            if a == 0 {
                continue;
            }
            if a == 255 {
                d.copy_from_slice(s);
                continue;
            }
            // source over, with the result's alpha worked out too, on straight (not premultiplied) values
            let da = d[3] as u32;
            let oa = 255 * a + da * (255 - a);
            if oa == 0 {
                continue;
            }
            for i in 0..3 {
                d[i] = ((s[i] as u32 * a * 255 + d[i] as u32 * da * (255 - a) + oa / 2) / oa) as u8;
            }
            d[3] = ((oa + 127) / 255) as u8;
        }
    });
}

// ---------- changing size ----------

fn resize_with(src: &Image8, w: u32, h: u32, alg: fr::ResizeAlg) -> Image8 {
    if src.w == w && src.h == h {
        return Image8 { w, h, rgba: src.rgba.clone() };
    }
    let mut out = Image8::blank(w, h);
    let from = fr::images::ImageRef::new(src.w, src.h, &src.rgba, fr::PixelType::U8x4).expect("sizes match");
    let mut to = fr::images::Image::from_slice_u8(w, h, &mut out.rgba, fr::PixelType::U8x4).expect("sizes match");
    let mut resizer = fr::Resizer::new();
    let options = fr::ResizeOptions::new().resize_alg(alg);
    // handles alpha correctly on its own, so colour doesn't bleed out of transparent pixels
    let _ = resizer.resize(&from, &mut to, &options);
    out
}

/// For saving: Lanczos, the sharp and careful one.
pub fn resize(src: &Image8, w: u32, h: u32) -> Image8 {
    resize_with(src, w, h, fr::ResizeAlg::Convolution(fr::FilterType::Lanczos3))
}

/// For the screen: quick, for a picture being shown smaller than it is.
pub fn shrink_fast(src: &[u8], sw: u32, sh: u32, w: u32, h: u32) -> Image8 {
    let mut out = Image8::blank(w, h);
    let from = fr::images::ImageRef::new(sw, sh, src, fr::PixelType::U8x4).expect("sizes match");
    let mut to = fr::images::Image::from_slice_u8(w, h, &mut out.rgba, fr::PixelType::U8x4).expect("sizes match");
    let mut resizer = fr::Resizer::new();
    let _ = resizer.resize(
        &from,
        &mut to,
        &fr::ResizeOptions::new().resize_alg(fr::ResizeAlg::Convolution(fr::FilterType::Box)),
    );
    out
}

/// A picture's size scaled to fit a longest side, never upward.
pub fn fit(w: u32, h: u32, max_side: u32) -> (u32, u32) {
    let long = w.max(h);
    if long <= max_side {
        return (w, h);
    }
    let k = max_side as f64 / long as f64;
    (((w as f64 * k).round() as u32).max(1), ((h as f64 * k).round() as u32).max(1))
}

/// JPG has no transparency: what shows through becomes white.
pub fn flatten_white(rgba: &[u8]) -> Vec<u8> {
    let mut rgb = vec![0u8; rgba.len() / 4 * 3];
    rgb.par_chunks_mut(3 << 16).zip(rgba.par_chunks(4 << 16)).for_each(|(d, s)| {
        for (o, p) in d.chunks_exact_mut(3).zip(s.chunks_exact(4)) {
            let a = p[3] as u32;
            if a == 255 {
                o.copy_from_slice(&p[..3]);
            } else {
                for i in 0..3 {
                    o[i] = ((p[i] as u32 * a + 255 * (255 - a) + 127) / 255) as u8;
                }
            }
        }
    });
    rgb
}

pub fn drop_alpha(rgba: &[u8]) -> Vec<u8> {
    let mut rgb = vec![0u8; rgba.len() / 4 * 3];
    rgb.par_chunks_mut(3 << 16).zip(rgba.par_chunks(4 << 16)).for_each(|(d, s)| {
        for (o, p) in d.chunks_exact_mut(3).zip(s.chunks_exact(4)) {
            o.copy_from_slice(&p[..3]);
        }
    });
    rgb
}

/// A tile copied straight in, for assembling pieces rather than drawing.
pub fn lay_tile_solid(img: &mut Image8, tile: &Tile) {
    let stride = img.w as usize * 4;
    for y in 0..tile.h as usize {
        let at = (tile.y as usize + y) * stride + tile.x as usize * 4;
        let from = y * tile.w as usize * 4;
        img.rgba[at..at + tile.w as usize * 4].copy_from_slice(&tile.rgba[from..from + tile.w as usize * 4]);
    }
}
