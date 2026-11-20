//! MP3 and FLAC writing as DotAny Audio does it, kept once: Recorder uses this crate, and Audio and the hub's converter
//! use it through dotany-sound (`shared/sound`), so a fix here reaches all three.
//! - `lame`: MP3 through LAME, which is LGPL, so it is its own `libmp3lame.dll` that build.rs puts beside the exe;
//! - `flac`: FLAC through libFLAC, which is BSD and built into the exe.
//!
//! Both are built from vendor/ by build.rs, with DotAny Audio's switches.

pub mod flac;
pub mod lame;
