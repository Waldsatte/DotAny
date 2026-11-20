pub mod app;
mod capture;
mod encode;
#[cfg(target_os = "macos")]
mod mic_mac;
mod stream;
mod takes;
mod wav;
