//! What a file really is, from its first bytes, and what is refused. A format that is left out on purpose is not opened
//! at all, not even to look at, whatever its name says: it gets `Sniff::Refused`.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Jpg,
    Png,
    Webp,
    Gif,
    Svg,
    Bmp,
    Tiff,
    Ico,
    // only read: a save goes to PNG, JPG or WebP
    Tga,
    Qoi,
    Pbm,
    Pgm,
    Ppm,
    Pam,
    Ff,
    Pcx,
    Ilbm,
}

impl Kind {
    pub fn ext(self) -> &'static str {
        match self {
            Kind::Jpg => "jpg",
            Kind::Png => "png",
            Kind::Webp => "webp",
            Kind::Gif => "gif",
            Kind::Svg => "svg",
            Kind::Bmp => "bmp",
            Kind::Tiff => "tiff",
            Kind::Ico => "ico",
            Kind::Tga => "tga",
            Kind::Qoi => "qoi",
            Kind::Pbm => "pbm",
            Kind::Pgm => "pgm",
            Kind::Ppm => "ppm",
            Kind::Pam => "pam",
            Kind::Ff => "ff",
            Kind::Pcx => "pcx",
            Kind::Ilbm => "iff",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            Kind::Jpg => "JPG",
            Kind::Png => "PNG",
            Kind::Webp => "WebP",
            Kind::Gif => "GIF",
            Kind::Svg => "SVG",
            Kind::Bmp => "BMP",
            Kind::Tiff => "TIFF",
            Kind::Ico => "ICO",
            Kind::Tga => "TGA",
            Kind::Qoi => "QOI",
            Kind::Pbm => "PBM",
            Kind::Pgm => "PGM",
            Kind::Ppm => "PPM",
            Kind::Pam => "PAM",
            Kind::Ff => "farbfeld",
            Kind::Pcx => "PCX",
            Kind::Ilbm => "IFF ILBM",
        }
    }
    /// Opened and shown, but never written: a save goes to one of the formats we write.
    pub fn read_only(self) -> bool {
        matches!(
            self,
            Kind::Tga | Kind::Qoi | Kind::Pbm | Kind::Pgm | Kind::Ppm | Kind::Pam | Kind::Ff | Kind::Pcx | Kind::Ilbm
        )
    }
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum Sniff {
    Is(Kind),
    /// Left out on purpose: the caller shows its "this format isn't supported" notice.
    Refused,
    /// Named like a picture of ours, but the bytes aren't one: damaged.
    Damaged,
}

/// Every name Image opens, and its open dialog's list (Image's app.rs).
pub const ALLOWED_EXT: [&str; 24] = [
    "jpg", "jpeg", "jfif", "png", "webp", "gif", "bmp", "svg", "ico", "tif", "tiff", "tga", "qoi", "pbm", "pgm", "ppm",
    "pnm", "pam", "ff", "pcx", "iff", "ilbm", "lbm", "farbfeld",
];

/// Names we never open, even when the bytes inside start like something we do: camera RAW files are TIFFs inside.
const REFUSED_EXT: [&str; 37] = [
    "heic", "heif", "hif", "avif", "avifs", "psd", "psb", "cr2", "cr3", "crw", "nef", "nrw", "arw", "srf", "sr2",
    "dng", "orf", "rw2", "raf", "pef", "rwl", "srw", "x3f", "erf", "kdc", "dcr", "mrw", "3fr", "mef", "mos", "iiq",
    "jxl", "jp2", "j2k", "jpx", "jxr", "wdp",
];

fn extension(name: &str) -> String {
    name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default()
}

pub fn allowed_name(name: &str) -> bool {
    ALLOWED_EXT.contains(&extension(name).as_str())
}

/// HEIC, AVIF and their relatives are all ISO boxes starting with `ftyp`; none of what we open is.
fn iso_box(bytes: &[u8]) -> bool {
    bytes.len() >= 12 && &bytes[4..8] == b"ftyp"
}

/// A TIFF that is really a camera file: Canon's marker in the header, or the DNG version tag in the first directory.
fn tiff_is_raw(bytes: &[u8]) -> bool {
    if bytes.len() >= 10 && &bytes[8..10] == b"CR" {
        return true;
    }
    crate::exif::first_ifd_has(bytes, &[0xC612, 0xC614])
}

/// Arithmetic-coded JPEG is left out like the other formats above (no code for it is compiled in), so it is refused:
/// a frame header of that kind (SOF9 to SOF11, SOF13 to SOF15), or its conditioning table (DAC).
fn jpeg_is_arithmetic(b: &[u8]) -> bool {
    let mut at = 2;
    while at + 4 <= b.len() && b[at] == 0xFF {
        let m = b[at + 1];
        if m == 0xFF {
            at += 1;
            continue;
        }
        if matches!(m, 0xC9 | 0xCA | 0xCB | 0xCC | 0xCD | 0xCE | 0xCF) {
            return true;
        }
        if m == 0xDA {
            break;
        }
        at += 2 + u16::from_be_bytes([b[at + 2], b[at + 3]]) as usize;
    }
    false
}

/// Truevision TGA has no signature: its header has to make sense. Colour-mapped, true-colour or grey, plain or run-length.
fn looks_like_tga(b: &[u8]) -> bool {
    if b.len() < 18 {
        return false;
    }
    let (map_type, image_type, depth) = (b[1], b[2], b[16]);
    let w = u16::from_le_bytes([b[12], b[13]]);
    let h = u16::from_le_bytes([b[14], b[15]]);
    let map_ok = match map_type {
        0 => true,
        1 => matches!(b[7], 15 | 16 | 24 | 32),
        _ => false,
    };
    let kind_ok = match image_type {
        1 | 9 => map_type == 1 && matches!(depth, 8 | 16),
        2 | 10 => matches!(depth, 15 | 16 | 24 | 32),
        3 | 11 => matches!(depth, 8 | 16),
        _ => false,
    };
    map_ok && kind_ok && w > 0 && h > 0
}

/// PBM, PGM, PPM (plain and raw) and PAM all start with P and a digit, then white space.
fn netpbm(b: &[u8]) -> Option<Kind> {
    if b.len() < 3 || b[0] != b'P' || !b[2].is_ascii_whitespace() {
        return None;
    }
    Some(match b[1] {
        b'1' | b'4' => Kind::Pbm,
        b'2' | b'5' => Kind::Pgm,
        b'3' | b'6' => Kind::Ppm,
        b'7' => Kind::Pam,
        _ => return None,
    })
}

/// ZSoft PCX: the maker byte, a version, run-length or plain, and a depth it can have.
fn looks_like_pcx(b: &[u8]) -> bool {
    b.len() >= 128
        && b[0] == 0x0A
        && matches!(b[1], 0 | 2 | 3 | 4 | 5)
        && b[2] <= 1
        && matches!(b[3], 1 | 2 | 4 | 8)
        && (1..=4).contains(&b[65])
}

fn looks_like_svg(bytes: &[u8]) -> bool {
    let head = &bytes[..bytes.len().min(8192)];
    let text = String::from_utf8_lossy(head);
    let t = text.trim_start_matches('\u{feff}').trim_start();
    t.starts_with('<') && t.contains("<svg")
}

pub fn sniff(bytes: &[u8], name: &str) -> Sniff {
    let ext = extension(name);
    if REFUSED_EXT.contains(&ext.as_str()) || iso_box(bytes) || bytes.starts_with(b"8BPS") {
        return Sniff::Refused;
    }
    let is = |m: &[u8]| bytes.starts_with(m);
    // JPEG XL's two forms, JPEG 2000 and JPEG XR have their own signatures
    if is(&[0xFF, 0x0A])
        || is(&[0, 0, 0, 0x0C, b'J', b'X', b'L', b' '])
        || is(&[0, 0, 0, 0x0C, b'j', b'P', b' ', b' '])
        || is(&[0xFF, 0x4F, 0xFF, 0x51])
        || is(&[b'I', b'I', 0xBC])
    {
        return Sniff::Refused;
    }
    if is(&[0xFF, 0xD8, 0xFF]) {
        return if jpeg_is_arithmetic(bytes) { Sniff::Refused } else { Sniff::Is(Kind::Jpg) };
    }
    if is(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        return Sniff::Is(Kind::Png);
    }
    if bytes.len() >= 12 && is(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return Sniff::Is(Kind::Webp);
    }
    if is(b"GIF87a") || is(b"GIF89a") {
        return Sniff::Is(Kind::Gif);
    }
    if is(b"BM") && bytes.len() >= 26 {
        return Sniff::Is(Kind::Bmp);
    }
    if is(&[b'I', b'I', 42, 0]) || is(&[b'M', b'M', 0, 42]) || is(&[b'I', b'I', 43, 0]) || is(&[b'M', b'M', 0, 43]) {
        return if tiff_is_raw(bytes) { Sniff::Refused } else { Sniff::Is(Kind::Tiff) };
    }
    if is(&[0, 0, 1, 0]) && bytes.len() >= 22 {
        return Sniff::Is(Kind::Ico);
    }
    if is(b"qoif") && bytes.len() >= 14 {
        return Sniff::Is(Kind::Qoi);
    }
    if is(b"farbfeld") && bytes.len() >= 16 {
        return Sniff::Is(Kind::Ff);
    }
    if let Some(k) = netpbm(bytes) {
        return Sniff::Is(k);
    }
    // IFF: an Amiga picture is a FORM of ILBM (or Deluxe Paint's chunky PBM); other IFF files (sound, animation) aren't ours
    if is(b"FORM") && bytes.len() >= 12 {
        return if &bytes[8..12] == b"ILBM" || &bytes[8..12] == b"PBM " {
            Sniff::Is(Kind::Ilbm)
        } else {
            Sniff::Refused
        };
    }
    if looks_like_pcx(bytes) && matches!(ext.as_str(), "pcx" | "") {
        return Sniff::Is(Kind::Pcx);
    }
    if ext == "tga" {
        return if looks_like_tga(bytes) { Sniff::Is(Kind::Tga) } else { Sniff::Damaged };
    }
    if ext == "svg" || (ext.is_empty() && looks_like_svg(bytes)) {
        return if looks_like_svg(bytes) { Sniff::Is(Kind::Svg) } else { Sniff::Damaged };
    }
    if ALLOWED_EXT.contains(&ext.as_str()) {
        return Sniff::Damaged;
    }
    Sniff::Refused
}
