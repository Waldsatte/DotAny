//! Pictures as DotAny Image reads them, kept once: Image uses this crate, and so does the hub's converter
//! (`shared/convert`), so a fix here reaches both.
//! - `formats`: what a file really is, from its first bytes, and what is refused;
//! - `metadata`, `exif`: what a picture carries besides pixels (colour profile, EXIF, XMP and so on), read, carried
//!   across and taken out;
//! - `ops`: what happens to pixels: turning, cropping, the sliders, drawing, resizing;
//! - `pcx`, `ilbm`: our own readers for PCX and IFF ILBM.
//!
//! Decoding the other formats and writing each one stay with each program: Image keeps every frame and page of a picture
//! within its memory budget, the converter reads the first one.

pub mod exif;
pub mod formats;
pub mod ilbm;
pub mod metadata;
pub mod ops;
pub mod pcx;
