//! What Video itself asks of the operating system. The family's platform layer (files, folders, the window's frame) is
//! dotany-base's (`shared/rust/src/platform`); this is only what is Video's own. Windows (`media_foundation.rs`) and macOS
//! (`macos.rs`) give the same items:
//!
//! - films the system plays, for what the WebView won't (AVI, WMV): `SystemFilm`, `SystemFacts`, `SystemError`, `Command`, `share`, `unshare`
//!   (macOS: none; every such film is answered `NoDecoder`)
//! - macOS only: `for_player`, an MKV or AVI repackaged for WKWebView

#[cfg(windows)]
mod media_foundation;
#[cfg(windows)]
pub use self::media_foundation::*;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use self::macos::*;
