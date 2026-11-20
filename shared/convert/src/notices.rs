//! Whose work the converter carries, for a licence page. The Rust crates are found from what is really compiled in by
//! `shared/scripts/notices.mjs`, the same way for every DotAny program; the full texts of the C libraries it can't see are
//! in `shared/convert/notices.mjs`. This list names them all, so a program can say what the converter brings without
//! reading Cargo's metadata.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Notice {
    pub name: &'static str,
    /// SPDX, as the library states it
    pub licence: &'static str,
    /// what it does here
    pub role: &'static str,
}

const LIST: &[Notice] = &[
    // C code built from vendor/ (shared/sound-encode, shared/sound, shared/unpack), or carried inside a crate
    Notice {
        name: "LAME 3.100 (libmp3lame.dll)",
        licence: "LGPL-2.0-or-later",
        role: "MP3 encoding, as Audio and Recorder; a DLL beside the program that can be replaced",
    },
    Notice { name: "libFLAC 1.5.0", licence: "BSD-3-Clause", role: "FLAC encoding" },
    Notice {
        name: "libarchive 3.8.11 (RAR readers only)",
        licence: "BSD-2-Clause",
        role: "reading RAR, never writing it",
    },
    Notice {
        name: "libopenmpt 0.8.9",
        licence: "BSD-3-Clause",
        role: "MOD, XM, S3M and IT reading, as Audio; built from shared/sound/vendor/libopenmpt",
    },
    Notice {
        name: "libwebp (inside libwebp-sys)",
        licence: "BSD-3-Clause, with Google's patent grant",
        role: "WebP reading and writing",
    },
    // Rust crates
    Notice { name: "image", licence: "MIT OR Apache-2.0", role: "PNG, JPEG, BMP and ICO reading, PNG and BMP writing" },
    Notice { name: "jpeg-encoder", licence: "(MIT OR Apache-2.0) AND IJG", role: "JPEG writing" },
    Notice { name: "tiff", licence: "MIT", role: "TIFF reading" },
    Notice { name: "gif", licence: "MIT OR Apache-2.0", role: "GIF reading" },
    Notice { name: "weezl", licence: "MIT OR Apache-2.0", role: "TIFF's LZW" },
    Notice {
        name: "fast_image_resize",
        licence: "MIT OR Apache-2.0",
        role: "Image's pixel work, shared with it (shared/picture)",
    },
    Notice { name: "symphonia", licence: "MPL-2.0", role: "MP3, FLAC, WAV, AIFF and Vorbis reading" },
    Notice { name: "lofty", licence: "MIT OR Apache-2.0", role: "carrying a song's tags across" },
    Notice { name: "vorbis_rs (aoTuV libvorbis, libogg)", licence: "BSD-3-Clause", role: "Ogg Vorbis writing" },
    Notice { name: "rubato", licence: "MIT OR Apache-2.0", role: "changing the sample rate for MP3" },
    Notice { name: "calamine", licence: "MIT", role: "XLSX, XLSM, XLS and ODS reading" },
    Notice { name: "chardetng", licence: "Apache-2.0 OR MIT", role: "finding a text file's encoding" },
    Notice { name: "encoding_rs", licence: "(Apache-2.0 OR MIT) AND BSD-3-Clause", role: "text encodings" },
    Notice { name: "quick-xml", licence: "MIT", role: "Word and 3MF reading" },
    Notice { name: "cfb", licence: "MIT", role: "Word 97 and old Excel files" },
    Notice { name: "regex", licence: "MIT OR Apache-2.0", role: "Markdown, as Text reads it" },
    Notice { name: "zip", licence: "MIT", role: "ZIP, and every Office and 3MF package" },
    Notice { name: "sevenz-rust2", licence: "Apache-2.0", role: "7z reading and writing" },
    Notice { name: "tar", licence: "MIT OR Apache-2.0", role: "TAR reading and writing" },
    Notice { name: "flate2", licence: "MIT OR Apache-2.0", role: "deflate" },
    Notice { name: "gltf", licence: "MIT OR Apache-2.0", role: "glTF and GLB reading" },
    Notice { name: "serde, serde_json", licence: "MIT OR Apache-2.0", role: "" },
    Notice { name: "rayon", licence: "MIT OR Apache-2.0", role: "" },
    Notice {
        name: "windows, windows-sys",
        licence: "MIT OR Apache-2.0",
        role: "Windows' PDF renderer, safe file moves",
    },
    // part of Windows, not shipped
    Notice {
        name: "Windows.Data.Pdf",
        licence: "part of Windows",
        role: "drawing PDF pages; nothing is shipped for it",
    },
];

/// Every library the converter carries, and the one part of Windows it uses.
pub fn notices() -> &'static [Notice] {
    LIST
}
