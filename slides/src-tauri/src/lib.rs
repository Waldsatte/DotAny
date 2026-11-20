pub mod app;
#[cfg(target_os = "macos")]
mod mac;
#[cfg(windows)]
mod media_foundation;
#[cfg(target_os = "macos")]
mod media_macos;
