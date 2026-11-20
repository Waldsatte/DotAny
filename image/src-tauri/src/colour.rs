//! Colour profiles (README.md says more). A picture's numbers are never changed behind anyone's back.
//! Editing happens on the file's own numbers and the profile travels with them, so a wide-gamut photo stays wide-gamut.
//! The screen is shown the numbers converted to sRGB (so it looks right), and only formats that can't carry a profile
//! (GIF, BMP, ICO) get the conversion written into the file itself.

use qcms::{DataType, Intent, Profile, Transform};

/// Size of one side of the table the page uses to show a wide-gamut picture.
pub const LUT_SIDE: usize = 33;

pub struct Colour {
    pub profile: Vec<u8>,
    to_srgb: Transform,
    from_srgb: Option<Transform>,
}

/// The profile must describe RGB numbers; a grey or CMYK profile says nothing about the RGB we hold after decoding.
fn is_rgb_profile(icc: &[u8]) -> bool {
    icc.len() > 132 && &icc[16..20] == b"RGB "
}

impl Colour {
    /// None when there's no usable profile, or when it's sRGB in all but name, so nothing needs doing.
    pub fn from_profile(icc: &[u8]) -> Option<Colour> {
        if !is_rgb_profile(icc) {
            return None;
        }
        let mut input = Profile::new_from_slice(icc, false)?;
        let mut srgb = Profile::new_sRGB();
        srgb.precache_output_transform();
        let to_srgb = Transform::new(&input, &srgb, DataType::RGBA8, Intent::Perceptual)?;
        let colour_probe = Colour { profile: Vec::new(), to_srgb, from_srgb: None };
        if colour_probe.is_nearly_srgb() {
            return None;
        }
        input.precache_output_transform();
        let from_srgb = Transform::new(&Profile::new_sRGB(), &input, DataType::RGBA8, Intent::Perceptual);
        Some(Colour { profile: icc.to_vec(), to_srgb: colour_probe.to_srgb, from_srgb })
    }

    fn is_nearly_srgb(&self) -> bool {
        let mut worst = 0i32;
        let steps = [0u8, 32, 64, 128, 192, 224, 255];
        for &r in &steps {
            for &g in &steps {
                for &b in &steps {
                    let mut p = [r, g, b, 255];
                    self.to_srgb.apply(&mut p);
                    worst = worst
                        .max((p[0] as i32 - r as i32).abs())
                        .max((p[1] as i32 - g as i32).abs())
                        .max((p[2] as i32 - b as i32).abs());
                }
            }
        }
        worst <= 1
    }

    pub fn to_srgb(&self, rgba: &mut [u8]) {
        use rayon::prelude::*;
        rgba.par_chunks_mut(1 << 20).for_each(|c| self.to_srgb.apply(c));
    }

    /// For what was drawn in sRGB on a picture in this profile, so a colour picked is the colour seen.
    pub fn from_srgb(&self, rgba: &mut [u8]) {
        if let Some(t) = &self.from_srgb {
            t.apply(rgba);
        }
    }

    /// The conversion to sRGB as a table the page can apply on the graphics card.
    pub fn table(&self) -> Vec<u8> {
        let n = LUT_SIDE;
        let mut t = Vec::with_capacity(n * n * n * 4);
        let scale = |i: usize| ((i * 255 + (n - 1) / 2) / (n - 1)) as u8;
        for b in 0..n {
            for g in 0..n {
                for r in 0..n {
                    t.extend_from_slice(&[scale(r), scale(g), scale(b), 255]);
                }
            }
        }
        self.to_srgb.apply(&mut t);
        t
    }
}
