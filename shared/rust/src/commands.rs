//! The commands every DotAny page calls: settings and recent files, the open and save dialogs, the files beside this
//! one, and the window. `dotany_base::handler![…]` lists them all; a tool adds its own beside them. Each that is about
//! a file is about the file of the window that asked (`files`): Tauri hands the command its window, and the page never
//! says which.

use crate::{Base, family, file_name, folder, lock, platform, settings};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Emitter, State, WebviewWindow};

// ---------- settings ----------

/// The family's settings (`family::SHARED_KEYS`): every program follows them. The rest is the tool's own.
#[tauri::command(async)]
pub fn set_setting(state: State<Base>, key: String, value: Option<String>) {
    if family::is_shared(&key) {
        family::set_setting(&state.family_dir, &key, value.as_deref());
    }
    let mut settings = state.settings.lock().unwrap();
    match &value {
        Some(v) => {
            settings.values.insert(key, v.clone());
        }
        None => {
            settings.values.remove(&key);
        }
    }
    settings::store(&state.data_dir, &settings);
}

#[tauri::command(async)]
pub fn forget_recent(app: AppHandle, state: State<Base>, path: String) {
    family::forget_recent(&state.family_dir, &path);
    let mut settings = state.settings.lock().unwrap();
    settings.recents.retain(|p| p != &path);
    settings::store(&state.data_dir, &settings);
    drop(settings);
    let _ = app.emit("recents", state.recents());
}

// ---------- opening ----------

/// A quick look, before anything unsaved is asked about. The name decides whether the format is one the tool reads
/// ("refused": "This file format isn't supported yet."); a file of such a format whose bytes aren't what the tool
/// reads (empty, damaged, or not what its name says) is "damaged": "Unable to read this file."
#[tauri::command(async)]
pub fn can_open(state: State<Base>, path: String) -> &'static str {
    let p = Path::new(&path);
    if !p.is_file() {
        return "missing";
    }
    if !state.tool.allowed_name(&file_name(p)) {
        return "refused";
    }
    if !(state.tool.sniff)(p) {
        return "damaged";
    }
    "ok"
}

/// The window's Open…. On the Mac it is the system's open panel as the window's sheet, and the file chosen goes where a
/// file from Finder goes (`crate::chosen_in`): this window opens it only if it holds nothing, so the answer is None when
/// it went to another window.
#[tauri::command(async)]
pub fn pick_open(window: WebviewWindow, state: State<Base>, label: String) -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        let _ = label;
        let (tx, rx) = std::sync::mpsc::channel();
        platform::open_panel(Some(&window), state.tool.extensions, state.start_folder_in(window.label()), move |p| {
            let _ = tx.send(p);
        });
        let path = rx.recv().ok().flatten()?;
        crate::chosen_in(tauri::Manager::app_handle(&window), window.label(), path)
            .map(|p| p.to_string_lossy().into_owned())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let mut dialog = rfd::FileDialog::new().set_parent(&window).add_filter(label, state.tool.extensions);
        if let Some(dir) = state.start_folder_in(window.label()) {
            dialog = dialog.set_directory(dir);
        }
        dialog.pick_file().map(|p| p.to_string_lossy().into_owned())
    }
}

/// The system's save dialog: on the Mac its save panel, as the window's sheet.
#[tauri::command(async)]
pub fn pick_save(
    window: WebviewWindow,
    start: Option<String>,
    name: String,
    ext: String,
    label: String,
) -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        let _ = label;
        let (tx, rx) = std::sync::mpsc::channel();
        platform::save_panel(&window, start.map(PathBuf::from), name, ext, move |p| {
            let _ = tx.send(p);
        });
        rx.recv().ok().flatten().map(|p| p.to_string_lossy().into_owned())
    }
    #[cfg(not(target_os = "macos"))]
    pick_save_windows(window, start, name, ext, label)
}

#[cfg(not(target_os = "macos"))]
fn pick_save_windows(
    window: WebviewWindow,
    start: Option<String>,
    name: String,
    ext: String,
    label: String,
) -> Option<String> {
    let mut dialog = rfd::FileDialog::new().set_parent(&window).set_file_name(name);
    if let Some(dir) = start {
        dialog = dialog.set_directory(dir);
    }
    if let Some(ext) = ext.strip_prefix('.') {
        dialog = dialog.add_filter(label, &[ext]);
    }
    dialog.save_file().map(|p| p.to_string_lossy().into_owned())
}

#[tauri::command(async)]
pub fn default_folder(window: WebviewWindow, state: State<Base>) -> String {
    if let Some(dir) = state.start_folder_in(window.label()).filter(|d| d.is_dir()) {
        return dir.to_string_lossy().into_owned();
    }
    platform::user_dir(state.tool.user_dir).to_string_lossy().into_owned()
}

/// The next or previous file in this one's folder, by name as people read names.
#[tauri::command(async)]
pub fn neighbour(window: WebviewWindow, state: State<Base>, step: i32) -> Option<String> {
    let path: PathBuf = state.current_in(window.label())?;
    let tool = state.tool;
    folder::neighbour(&path, step, |n| tool.allowed_name(n)).map(|p| p.to_string_lossy().into_owned())
}

/// The window that asked closed its file: the family forgets it, and the tool drops what it kept of it. (Rust that puts
/// a file down itself calls `close_file_in`, or `close_file` for a tool with one window.)
#[tauri::command]
pub fn close_document(window: WebviewWindow) {
    crate::close_file_in(tauri::Manager::app_handle(&window), window.label());
}

/// "Keep mine": this window carries on with what it has. A save still checks the file hasn't changed under it. (A tool
/// that answers Keep Current Edits itself calls `keep_mine_in`, or `keep_mine` for a tool with one window.)
#[tauri::command(async)]
pub fn keep_mine(window: WebviewWindow) {
    crate::keep_mine_in(tauri::Manager::app_handle(&window), window.label());
}

// ---------- the window ----------

#[tauri::command]
pub fn set_frame(window: WebviewWindow, dark: bool) {
    // the window was made in the chosen colours; from here on the page decides, and the system's own light or dark must
    // keep reaching it, or System chosen later would stay on the colours the window started with
    let _ = window.set_theme(None);
    platform::paint_frame(&window, dark);
}

#[tauri::command]
pub fn window_control(window: WebviewWindow, action: String) {
    let _ = match action.as_str() {
        "minimize" => window.minimize(),
        "toggle" if window.is_maximized().unwrap_or(false) => window.unmaximize(),
        "toggle" => window.maximize(),
        "full" => window.set_fullscreen(!window.is_fullscreen().unwrap_or(false)),
        "unfull" => window.set_fullscreen(false),
        // the same way in as the system's own close: the page asks about unsaved work first
        "close" => window.close(),
        // the Mac's: a double-click on the top bar does what the system's setting says (zoom, minimize, nothing)
        #[cfg(target_os = "macos")]
        "title-double-click" => {
            platform::title_double_click(&window);
            Ok(())
        }
        // the Mac's: the question this window asked as it was closing was answered Cancel, so a quit stops here
        #[cfg(target_os = "macos")]
        "stay" => {
            platform::close_given_up(window.label());
            if !platform::quitting() {
                tauri::Manager::state::<Base>(&window).forget_held();
            }
            Ok(())
        }
        _ => Ok(()),
    };
}

#[tauri::command]
pub fn window_menu(window: WebviewWindow, x: i32, y: i32) {
    platform::show_system_menu(&window, x, y);
}

#[tauri::command]
pub fn set_snap_button(window: WebviewWindow, x: i32, y: i32, w: i32, h: i32) {
    platform::set_snap_button(&window, x, y, w, h);
}

/// The menu bar's words in the page's language, by item (the Mac's; Windows has no menu bar, and its page never asks).
#[tauri::command]
pub fn set_menu_words(app: AppHandle, words: std::collections::HashMap<String, String>) {
    platform::set_menu_words(&app, &words);
}

/// The window's title; on the Mac the page also says where its file is (`path`), which the system shows from the title
/// (`platform::set_represented`). Windows' page never sends it.
#[tauri::command]
pub fn set_title(window: WebviewWindow, title: String, path: Option<String>) {
    let _ = window.set_title(&title);
    #[cfg(target_os = "macos")]
    platform::set_represented(&window, path);
    #[cfg(not(target_os = "macos"))]
    let _ = path;
}

/// Whether the page has work not saved yet: the Mac shows it as a dot in the red close button. Windows has the top bar's
/// dot only, and its page never asks.
#[tauri::command]
pub fn set_edited(window: WebviewWindow, state: State<Base>, edited: bool) {
    // Quit asks the windows with unsaved work first (`platform::quit`)
    if let Some(row) = lock(&state.files).row(window.label()) {
        row.edited = edited;
    }
    platform::set_edited(&window, edited);
}

#[tauri::command]
pub fn window_ready(window: WebviewWindow) {
    // a file the Mac handed this window before its page could hear it
    crate::page_ready(tauri::Manager::app_handle(&window), window.label());
    // a window that came back after its WebView went stays where it was, behind the others
    if crate::webview::stays_behind(window.label()) {
        return;
    }
    let _ = window.set_focus();
}

/// Called after the first frame with the work on screen. With DOTANY_MEASURE set, writes the timings and exits.
#[tauri::command]
pub fn startup_done(app: AppHandle, state: State<Base>, js: serde_json::Value) {
    if state.shown.swap(true, Ordering::Relaxed) {
        return;
    }
    if let Some(path) = &state.measure {
        let report = serde_json::json!({
            "process_to_main_ms": state.since_process_start_ms,
            "window_shown_ms": state.window_shown_ms.load(Ordering::Relaxed),
            "main_to_painted_ms": state.boot.elapsed().as_secs_f64() * 1000.0,
            "process_to_painted_ms": platform::ms_since_process_start(),
            "page": js,
        });
        let _ = fs::write(path, report.to_string());
        app.exit(0);
    }
}

/// Each system's print window: for a tool that prints (createShell's `print`, Text, Image). Windows' through Tauri; the
/// Mac's run by the base itself, with the page's backgrounds on paper as WebView2 prints them (`mac_print`).
#[tauri::command]
pub fn print_window(window: WebviewWindow) {
    #[cfg(not(target_os = "macos"))]
    let _ = window.print();
    #[cfg(target_os = "macos")]
    crate::mac_print::print(&window);
}

/// The page has settled any unsaved work; this really closes the window.
#[tauri::command]
pub fn close_window(window: WebviewWindow, state: State<Base>) {
    lock(&state.closing).insert(window.label().to_string());
    let _ = window.destroy();
}

/// A web address a file links to, opened by the system's browser: http and https only (`links::open_web_link`).
#[tauri::command(async)]
pub fn open_web_link(url: String) -> bool {
    crate::links::open_web_link(&url)
}
