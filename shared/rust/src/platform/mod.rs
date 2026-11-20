//! Everything that differs between operating systems lives under this module, and nothing outside it names one.
//! Windows (`windows.rs`) and macOS (`macos.rs`) give the same functions; a Linux port adds a third sibling:
//!
//! - files: `create_temp`, `is_read_only`, `replace_file`, `place_file`, `write_failure`, `same_name`, `lock_file`
//! - per-user folders: `data_dir`, `user_dir`
//! - the window's frame: `frame`, `system_is_dark`, `paint_frame`, `prepare_frame`, `set_snap_button`,
//!   `show_system_menu`, `set_edited`
//! - the menu bar and the program's life: `menu`, `set_menu_words`, `STAYS_WITHOUT_WINDOWS`, `quitting`,
//!   `close_requested`, `window_gone`, `open_another`, `cascade` (Windows: no menu bar, the program ends with its last
//!   window, and it never has a second window for a file)
//! - measuring: `ms_since_process_start`
//! - the family: `sibling_tool`, `launch`
//! - the system's browser: `open_url` (macOS: `open <url>`)
//! - the WebView: `shared_webview_dir`, `webview`, `browser_args`, `watch_webview`, `rest`, `wake`, `normal_place` (Windows: the family's one
//!   WebView2; a platform without one gives its own window's WebView and does nothing for the rest)

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use self::windows::*;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use self::macos::*;
