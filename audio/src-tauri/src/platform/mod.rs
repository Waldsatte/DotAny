//! What Audio itself asks of the operating system. The family's platform layer (files, folders, the window's frame) is
//! dotany-base's (`shared/rust/src/platform`); this is only what is Audio's own, the same functions on each system:
//!
//! - sound the system decodes (M4A): `system_decode`, `SystemError`
//!
//! Windows: Media Foundation's source reader (`media_foundation.rs`). The Mac: AudioToolbox's `ExtAudioFile`
//! (`macos.rs`), which always has the decoders, so an M4A there never says a component is missing.

#[cfg(windows)]
mod media_foundation;
#[cfg(windows)]
pub use self::media_foundation::*;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use self::macos::*;
