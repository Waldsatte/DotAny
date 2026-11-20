//! Sound as DotAny Audio reads and writes it, kept once: Audio uses this crate, and so does the hub's converter
//! (`shared/convert`), so a fix here reaches both.
//! - `formats`: what a file is, by its bytes, and what is refused whatever it is called;
//! - `oldsound`: AU and VOC, and WAVs in A-law or μ-law, our own readers;
//! - `module`: MOD, XM, S3M and IT, played by libopenmpt, which build.rs builds from vendor/;
//! - `encode`: samples to each format's bytes, through `flac` and `lame` for FLAC and MP3;
//! - `rate`: changing the sample rate;
//! - `tags`: words and cover, carried and written by lofty.
//!
//! Reading MP3, FLAC, WAV, AIFF and Vorbis themselves is each program's own (symphonia, in Audio's `decode.rs` and the
//! converter's `sound/mod.rs`), and so is M4A, which only Audio reads, through Windows.

pub mod encode;
pub mod formats;
pub mod module;
pub mod oldsound;
pub mod rate;
pub mod tags;
// libFLAC's and LAME's doors (dotany-sound-encode), which `encode` writes through; Audio and the converter reach them here
pub use dotany_sound_encode::{flac, lame};
