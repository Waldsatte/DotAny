//! What DotAny asks of the operating system, the same functions on each: Windows' in `windows.rs` (the registry, one
//! DotAny through a mutex), the Mac's in `macos.rs`.
//!
//! - starting with the system: `autostart`, `set_autostart`
//! - the file manager's menu: `shell_menu`, `set_shell_menu`
//! - which program opens a kind of file: `opens_with_dotany`, `open_default_apps`
//! - a tool's version: `version_of`
//! - one DotAny at a time: `single`, `wait_for_knock`, `Single`
//! - the Mac only: the programs running from DotAny's .apps, before an update replaces them: `open_from`

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use self::windows::*;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use self::macos::*;
