//! dotany-base: what every DotAny tool shares on the Rust side. Atomic saves, settings and recent files, the file
//! watcher, the shared fonts, the files beside this one, the platform layer (Windows and macOS), and the window with its
//! frame, buttons and family commands.
//!
//! A tool describes itself once in a `static TOOL: Tool`, builds with `dotany_base::builder(&TOOL)`, lists its commands
//! with `dotany_base::handler![…]`, and calls `opened` and `saved` from its own open and save commands.
//!
//! The family keeps a file per window (`files`): on the Mac a program has one process and a window per file, on Windows
//! each file is a program of its own and the table has one row. A tool that keeps one builds with
//! `builder_per_window`, keeps its own state per window (`PerWindow`) and calls `opened_in` and `saved_in` with the
//! window that asked (shared/README.md, "A file per window"); a tool built with `builder` keeps one file for the program, in
//! its window `main`, and the calls without a window mean that one.
//!
//! The recent list and the family's settings (appearance, labels, sound, language, autosave) are shared by every
//! program (`family`): a tool shows its own files from the shared list, and follows a change made in any other
//! program while it is open.

pub mod atomic;
pub mod beside;
pub mod commands;
pub mod crash;
pub mod family;
mod files;
pub mod folder;
mod fonts;
pub mod limits;
pub mod links;
pub mod locks;
#[cfg(target_os = "macos")]
mod mac_print;
pub mod platform;
#[cfg(target_os = "macos")]
pub mod quickaction;
mod quit;
pub mod settings;
#[cfg(target_os = "macos")]
mod smart_zoom;
#[cfg(target_os = "macos")]
mod testdrive;
// the test driver's rules for who may reach it: plain functions, so they are tested on every system
#[cfg(any(target_os = "macos", test))]
mod testgate;
pub mod watcher;
pub mod webview;

pub use files::{MAIN, PerWindow};
/// The test driver's page hook, for a window a tool makes itself (Slides' presenter's window): with it the driver hears
/// that page's errors as it hears every window's the base makes. Empty outside a test.
#[cfg(target_os = "macos")]
pub fn test_hook() -> &'static str {
    if testdrive::port().is_some() { testdrive::TEST_HOOK } else { "" }
}
pub use locks::{lock, read_lock, write_lock};
pub use webview::{set_reopen, shown};

use files::Table;
use settings::Settings;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::{Instant, SystemTime};
use tauri::webview::WebviewWindowBuilder;
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindow, WindowEvent};
use watcher::FileWatch;

/// What makes one tool itself. Everything else is the family's.
pub struct Tool {
    /// The window's title and the about page's name: "DotAny Audio".
    pub name: &'static str,
    /// The settings folder under `%APPDATA%\DotAny`: "Audio".
    pub folder: &'static str,
    /// The variable that moves the settings folder, for tests: "DOTANY_AUDIO_DATA".
    pub data_env: &'static str,
    /// The user's folder a first open or save starts in: "Music", "Pictures", "Videos", "Documents".
    pub user_dir: &'static str,
    /// Whether the program opens files: every tool does, DotAny itself (the hub) doesn't. On the Mac a program that does
    /// has the File menu (Open…, Open Recent, Close), whether or not Finder hands it any kind of file: Recorder opens
    /// WAVs that Finder gives to Audio.
    pub opens_files: bool,
    /// What the open dialog shows and which names can be opened at all, without dots: ["mp3", "flac"].
    pub extensions: &'static [&'static str],
    /// A look at the bytes before anything unsaved is asked about; `|_| true` when the name is enough.
    pub sniff: fn(&Path) -> bool,
    /// The window's first size and its smallest.
    pub size: (f64, f64),
    pub min_size: (f64, f64),
    /// The tool's own state to drop when its file closes. A tool with a file per window says it in `FileWindows` instead.
    pub on_close: fn(&AppHandle),
}

impl Tool {
    pub fn allowed_name(&self, name: &str) -> bool {
        let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
        self.extensions.contains(&ext.as_str())
    }
}

pub type Meta = (u64, Option<SystemTime>);

pub fn meta_of(path: &Path) -> Option<Meta> {
    std::fs::metadata(path).ok().map(|m| (m.len(), m.modified().ok()))
}

pub fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

pub fn same_file(a: &Path, b: &Path) -> bool {
    match (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

/// How a tool that keeps a file per window joins the family (`builder_per_window`).
pub struct FileWindows {
    /// Drops the tool's own state for a window: its file closed (`close_document`), or the window went.
    pub forget: fn(&AppHandle, &str),
    /// What every window's page finds in `window.__DOTANY__` besides the family's own (Image: `nogl`).
    pub extra: fn() -> serde_json::Value,
}

/// `FileWindows::extra` for a tool whose pages want nothing more.
pub fn no_extra() -> serde_json::Value {
    serde_json::json!({})
}

pub struct Base {
    pub tool: &'static Tool,
    pub settings: Mutex<Settings>,
    pub data_dir: PathBuf,
    /// The family's shared folder: the recent list and the settings every tool follows.
    pub family_dir: PathBuf,
    family_watch: Mutex<Vec<FileWatch>>,
    /// Each window's file, by the window's label (`files`).
    files: Mutex<Table>,
    /// A tool that keeps a file per window (`builder_per_window`); None for one that keeps one for the program.
    each: Option<FileWindows>,
    /// The windows whose page has settled their unsaved work and may really close now.
    closing: Mutex<HashSet<String>>,
    window_shown_ms: AtomicU64,
    boot: Instant,
    since_process_start_ms: f64,
    measure: Option<PathBuf>,
    shown: AtomicBool,
    /// How the program opens its first window, again for a click on the Mac's Dock icon when no window is left.
    #[cfg(target_os = "macos")]
    opener: Mutex<Option<Opener>>,
    /// Files the Mac handed over before the program had made its first window (a file that starts the program can come
    /// before Tauri's setup); None once it has.
    #[cfg(target_os = "macos")]
    early: Mutex<Option<Vec<PathBuf>>>,
    /// New documents asked for (DotAny's New, `dotany-<tool>://new`) before the first window: started once it is there.
    #[cfg(target_os = "macos")]
    early_new: Mutex<Vec<String>>,
    /// The windows made at the tool's own size (its document windows): the size the person leaves them at is the next
    /// window's (`fitted_size`).
    #[cfg(target_os = "macos")]
    sized: Mutex<HashSet<String>>,
}

/// The tool's own setting that keeps the size its windows were last left at, in points: "1180x800" (the Mac's).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const WINDOW_SIZE: &str = "window-size";

/// The size a new document window opens at on the Mac: the one a window of this tool was last left at (`saved`, as
/// `WINDOW_SIZE` keeps it), else the tool's own; never larger than the screen's room for windows (`room`, so a window
/// left large on a bigger display still fits after the display changed), never smaller than the tool's smallest.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn fitted_size(saved: Option<&str>, default: (f64, f64), min: (f64, f64), room: Option<(f64, f64)>) -> (f64, f64) {
    let read = saved.and_then(|s| {
        let (w, h) = s.split_once('x')?;
        let (w, h) = (w.trim().parse::<f64>().ok()?, h.trim().parse::<f64>().ok()?);
        (w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0).then_some((w, h))
    });
    let (mut w, mut h) = read.unwrap_or(default);
    if let Some((rw, rh)) = room.filter(|(rw, rh)| *rw > 0.0 && *rh > 0.0) {
        w = w.min(rw);
        h = h.min(rh);
    }
    (w.max(min.0).round(), h.max(min.1).round())
}

#[cfg(target_os = "macos")]
type Opener = Box<dyn Fn(&AppHandle, Option<String>) -> tauri::Result<()> + Send>;

impl Base {
    /// The program's open file, for a tool with one window (`main`). A tool with a file per window asks `current_in`.
    pub fn current(&self) -> Option<PathBuf> {
        self.current_in(MAIN)
    }

    /// The file open in this window, if any.
    pub fn current_in(&self, label: &str) -> Option<PathBuf> {
        lock(&self.files).path(label)
    }

    /// Where a dialog starts, for a tool with one window; `start_folder_in` for one window of several.
    pub fn start_folder(&self) -> Option<PathBuf> {
        self.start_folder_in(MAIN)
    }

    /// Where a dialog in this window starts: its file's folder, else the last file's.
    pub fn start_folder_in(&self, label: &str) -> Option<PathBuf> {
        if let Some(p) = self.current_in(label) {
            return p.parent().map(Path::to_path_buf);
        }
        self.recents().first().and_then(|p| Path::new(p).parent().map(Path::to_path_buf))
    }

    /// This tool's recent files, newest first, from the family's shared list.
    pub fn recents(&self) -> Vec<String> {
        family::recents_of(&self.family_dir, self.tool.folder, settings::MAX_RECENT)
    }

    /// The program's file is as it was when this program read or wrote it, for a tool with one window; `unchanged_since_read_in`
    /// for one window of several.
    pub fn unchanged_since_read(&self) -> bool {
        self.unchanged_since_read_in(MAIN)
    }

    /// This window's file is as it was when this program read or wrote it: no other program has been at it.
    pub fn unchanged_since_read_in(&self, label: &str) -> bool {
        let (path, baseline) = match lock(&self.files).row(label) {
            Some(row) => (row.path.clone(), row.baseline),
            None => return false,
        };
        path.as_deref().is_some_and(|p| meta_of(p) == baseline)
    }

    /// Whether the tool keeps a file per window (`builder_per_window`).
    pub fn per_window(&self) -> bool {
        self.each.is_some()
    }

    /// A quit was given up: no window still waiting for its page is asked to close later.
    pub(crate) fn forget_held(&self) {
        lock(&self.files).forget_held();
    }
}

/// The tool has read `path`, for a tool with one window: it's now the program's open file (window `main`). A tool with a
/// file per window calls `opened_in`.
pub fn opened(app: &AppHandle, path: &Path) {
    opened_in(app, MAIN, path);
}

/// The tool has read `path` in this window: it's the window's open file now, watched, and first in the recent list.
pub fn opened_in(app: &AppHandle, label: &str, path: &Path) {
    let state = app.state::<Base>();
    let now = meta_of(path);
    let watch = {
        let (handle, label) = (app.clone(), label.to_string());
        watcher::watch(path, move || file_touched(&handle, &label))
    };
    // the old file's watcher, or this one if the window has gone meanwhile (it keeps no row), stops out here
    let unwanted = lock(&state.files).opened(label, path, now, watch);
    drop(unwanted);
    remember(app, &state, &path.to_string_lossy());
}

/// The tool has written `target`, for a tool with one window; `saved_in` for one window of several.
pub fn saved(app: &AppHandle, target: &Path) {
    saved_in(app, MAIN, target);
}

/// This window has written `target`. A save over its open file isn't news to the watcher; any save goes on the recent
/// list.
pub fn saved_in(app: &AppHandle, label: &str, target: &Path) {
    let state = app.state::<Base>();
    let open = state.current_in(label);
    if open.as_deref().is_some_and(|p| same_file(p, target)) {
        let now = meta_of(target);
        if let Some(row) = lock(&state.files).row(label) {
            row.baseline = now;
            row.announced = None;
        }
    }
    remember(app, &state, &target.to_string_lossy());
}

/// "Keep mine" (Keep Current Edits) for the program's file, for a tool with one window that answers it itself (Audio);
/// `keep_mine_in` for one window of several. The page's own is the `keep_mine` command.
pub fn keep_mine(app: &AppHandle) {
    keep_mine_in(app, MAIN);
}

/// "Keep mine": what is on disk now is this window's file's baseline; a save still checks it hasn't changed since.
pub fn keep_mine_in(app: &AppHandle, label: &str) {
    let state = app.state::<Base>();
    let path = state.current_in(label);
    let now = path.as_deref().and_then(meta_of);
    let mut files = lock(&state.files);
    if let Some(row) = files.row(label) {
        row.baseline = now;
        row.announced = None;
    }
}

/// The program's file closed, for a tool with one window (Recorder, as a take starts); `close_file_in` for one window of
/// several. The page's own Close is the `close_document` command.
pub fn close_file(app: &AppHandle) {
    close_file_in(app, MAIN);
}

/// This window closed its file: nothing to watch or step from, and the tool drops what it kept of the file
/// (`FileWindows::forget`; `Tool::on_close` for a tool with one window).
pub fn close_file_in(app: &AppHandle, label: &str) {
    let state = app.state::<Base>();
    // the watcher stops as it drops, outside the lock
    let watch = lock(&state.files).closed(label, false);
    drop(watch);
    match &state.each {
        Some(each) => (each.forget)(app, label),
        None => (state.tool.on_close)(app),
    }
}

/// This window holds work that has no file now (a picture pasted in): nothing to watch or step from, as with no file,
/// but on the Mac a file handed over goes to a new window rather than replace it. The tool keeps its own state: it has
/// just made it.
pub fn untitled_in(app: &AppHandle, label: &str) {
    let watch = lock(&app.state::<Base>().files).closed(label, true);
    drop(watch);
}

/// Only puts `path` first on the recent lists, the family's and the tool's own, and tells every window. For a tool
/// that keeps its own open files (Text, a file per window) instead of calling `opened` and `saved`.
pub fn remember_recent(app: &AppHandle, path: &Path) {
    let state = app.state::<Base>();
    remember(app, &state, &path.to_string_lossy());
}

fn remember(app: &AppHandle, state: &Base, path: &str) {
    family::add_recent(&state.family_dir, state.tool.folder, path);
    // the system's list too (the Dock icon's menu, Open Recent)
    #[cfg(target_os = "macos")]
    platform::note_recent(app, Path::new(path));
    // the tool's own settings file keeps its list too
    let mut settings = state.settings.lock().unwrap();
    settings::add_recent(&mut settings, path);
    settings::store(&state.data_dir, &settings);
    drop(settings);
    let _ = app.emit("recents", state.recents());
}

// ---------- another program changed what the family shares ----------

/// The shared settings as the page wants them: every shared key, with null for "follow the system".
fn shared_values(state: &Base) -> serde_json::Value {
    family::page_values(&state.settings.lock().unwrap().values)
}

fn shared_settings_changed(app: &AppHandle) {
    let state = app.state::<Base>();
    let Some(shared) = family::settings(&state.family_dir) else { return };
    {
        let mut settings = state.settings.lock().unwrap();
        if family::follow(&mut settings.values, &shared).is_empty() {
            return;
        }
        settings::store(&state.data_dir, &settings);
    }
    let _ = app.emit("shared-settings", shared_values(&state));
}

fn shared_recents_changed(app: &AppHandle) {
    let state = app.state::<Base>();
    let _ = app.emit("recents", state.recents());
    // the Mac's Open Recent, where it shows this list (Recorder's)
    #[cfg(target_os = "macos")]
    platform::family_recents_changed(app);
}

/// Watches the family's folder for as long as the program runs.
fn watch_family(app: &AppHandle) {
    let state = app.state::<Base>();
    let (a, b) = (app.clone(), app.clone());
    *state.family_watch.lock().unwrap() =
        family::watch(&state.family_dir, move || shared_settings_changed(&a), move || shared_recents_changed(&b));
}

// ---------- another program changed the file ----------

/// A tool that still holds the bytes it read can tell a file that was only touched (its time changed, its bytes not)
/// from one that really changed; then nothing is said. Set once, before the window opens (Image). Asked about the file
/// of the window it names.
static SAME_BYTES: std::sync::OnceLock<fn(&AppHandle, &str, &Path) -> bool> = std::sync::OnceLock::new();

pub fn set_same_bytes_check(check: fn(&AppHandle, &str, &Path) -> bool) {
    let _ = SAME_BYTES.set(check);
}

fn file_touched(app: &AppHandle, label: &str) {
    let state = app.state::<Base>();
    let mut files = lock(&state.files);
    let Some(file) = files.row(label) else { return };
    let Some(path) = file.path.clone() else { return };
    let now = meta_of(&path);
    if now == file.baseline || (now.is_some() && now == file.announced) {
        return;
    }
    if now.is_some() && SAME_BYTES.get().is_some_and(|same| same(app, label, &path)) {
        // touched but not changed
        file.baseline = now;
        file.announced = None;
        return;
    }
    if file.announced.is_some() && now.is_none() {
        return;
    }
    file.announced = now.or(Some((0, None)));
    drop(files);
    // the window that has it open; another window of the program has a file of its own
    let _ = app.emit_to(label, "file-changed", now.is_none());
}

// ---------- the window ----------

fn wants_dark(settings: &Settings) -> bool {
    match settings.values.get("dotany-theme").map(String::as_str) {
        Some("dark") => true,
        Some("light") => false,
        _ => platform::system_is_dark(),
    }
}

/// A window of the family's: its label, the page it shows, its first and smallest size, and whether it shows at once.
pub struct WindowSpec<'a> {
    pub label: &'a str,
    pub page: &'a str,
    pub size: (f64, f64),
    pub min_size: (f64, f64),
    pub visible: bool,
    /// What the page finds in `window.__DOTANY__` besides the family's own.
    pub extra: serde_json::Value,
}

/// Started by DotAny's New: --new, or --new=csv for one kind of file.
fn new_arg() -> Option<String> {
    std::env::args_os().skip(1).find_map(|a| {
        let a = a.to_string_lossy();
        if a == "--new" { Some(String::new()) } else { a.strip_prefix("--new=").map(str::to_string) }
    })
}

/// The window of a tool with one window: always `main`.
fn open_window(app: &AppHandle, start: Option<String>) -> tauri::Result<WebviewWindow> {
    let tool = app.state::<Base>().tool;
    let extra = serde_json::json!({ "start": start, "new": new_arg() });
    open_window_as(
        app,
        WindowSpec { label: MAIN, page: "index.html", size: tool.size, min_size: tool.min_size, visible: true, extra },
    )
}

/// A window of a tool with a file per window, with `start` in it or empty. The first is `main`; on
/// the Mac each later one is cascaded from the window in front, as a document window is, and comes forward (Windows
/// never has a second: each file is a program of its own).
fn open_file_window(app: &AppHandle, start: Option<String>) -> tauri::Result<WebviewWindow> {
    open_file_window_new(app, start, None)
}

/// `open_file_window`, starting a new document of the kind `new` names ("" for the tool's own choice), as DotAny's New
/// asks on the Mac (`start_new`).
fn open_file_window_new(app: &AppHandle, start: Option<String>, new: Option<String>) -> tauri::Result<WebviewWindow> {
    let state = app.state::<Base>();
    let Some(each) = &state.each else { return open_window(app, start) };
    let label = lock(&state.files).next_label();
    let first = label == MAIN;
    // New started the program: its first window makes the new file, not every window after it
    let new = new.or(if first { new_arg() } else { None });
    let mut extra = serde_json::json!({ "start": start, "new": new });
    if let (Some(e), serde_json::Value::Object(more)) = (extra.as_object_mut(), (each.extra)()) {
        e.extend(more);
    }
    let tool = state.tool;
    open_document_window(
        app,
        WindowSpec {
            label: &label,
            page: "index.html",
            size: tool.size,
            min_size: tool.min_size,
            visible: true,
            extra,
        },
    )
}

/// `open_window_as`, for a document window of a program that has several (Text's, or a tool's with a file per window):
/// on the Mac a window made while others are open is cascaded from the one in front, down and right as AppKit places
/// document windows, and shows once it is there. Windows places a program's windows itself.
pub fn open_document_window(app: &AppHandle, spec: WindowSpec) -> tauri::Result<WebviewWindow> {
    #[cfg(target_os = "macos")]
    {
        let front = if app.webview_windows().is_empty() { None } else { front_window(app) };
        if let Some(front) = front {
            let window = open_window_as(app, WindowSpec { visible: false, ..spec })?;
            platform::cascade(&window, &front);
            return Ok(window);
        }
    }
    open_window_as(app, spec)
}

/// The window in front, else the newest: where a new window is cascaded from.
#[cfg(target_os = "macos")]
fn front_window(app: &AppHandle) -> Option<WebviewWindow> {
    let windows = app.webview_windows();
    if let Some(w) = windows.values().find(|w| w.is_focused().unwrap_or(false)) {
        return Some(w.clone());
    }
    let newest = lock(&app.state::<Base>().files).newest();
    newest.and_then(|l| windows.get(&l).cloned())
}

/// A window with the family's frame, colours and settings. A tool with one window never calls this: `builder` does.
pub fn open_window_as(app: &AppHandle, spec: WindowSpec) -> tauri::Result<WebviewWindow> {
    open_window_at(app, spec, None)
}

/// `open_window_as`, made where an earlier window was (`webview`: a window coming back after its WebView went).
pub(crate) fn open_window_at(
    app: &AppHandle,
    spec: WindowSpec,
    place: Option<&webview::Place>,
) -> tauri::Result<WebviewWindow> {
    let state = app.state::<Base>();
    let tool = state.tool;
    let settings = state.settings.lock().unwrap().clone();
    let test = std::env::var_os("DOTANY_TEST").is_some();
    let mut boot = serde_json::json!({ "settings": settings.values, "recents": state.recents(), "start": null, "version": app.package_info().version.to_string(), "test": test });
    // kept, to make the window again if its WebView goes (`webview`)
    let kept = spec.extra.clone();
    // its row in the table of open files, before its page can ask anything; a window made with a file takes no other
    // (`start` is the file's path, or Text's { path, format })
    let starts_with_file = match spec.extra.get("start") {
        Some(serde_json::Value::String(_)) => true,
        Some(serde_json::Value::Object(o)) => o.get("path").is_some_and(|p| !p.is_null()),
        _ => false,
    };
    lock(&state.files).add(spec.label, starts_with_file);
    if let (Some(b), serde_json::Value::Object(more)) = (boot.as_object_mut(), spec.extra) {
        b.extend(more);
    }
    #[allow(unused_mut)]
    let mut script = format!("window.__DOTANY__={boot};");
    // the Mac's test driver hears the page's errors (testdrive.rs; WebView2 has DevTools for that)
    #[cfg(target_os = "macos")]
    if testdrive::port().is_some() {
        script.push_str(testdrive::TEST_HOOK);
    }
    // on the Mac a document window opens at the size the person last left one of this tool's at (a window coming back
    // keeps its own)
    #[allow(unused_mut)]
    let mut size = spec.size;
    #[cfg(target_os = "macos")]
    if spec.size == tool.size {
        lock(&state.sized).insert(spec.label.to_string());
        if place.is_none() {
            size =
                fitted_size(settings.values.get(WINDOW_SIZE).map(String::as_str), tool.size, spec.min_size, room(app));
        }
    }
    let dark = wants_dark(&settings);
    let bg =
        if dark { tauri::window::Color(0x16, 0x16, 0x16, 255) } else { tauri::window::Color(0xEC, 0xEB, 0xE7, 255) };
    // an explicit theme gives the frame its colour from the very first moment
    let theme = match settings.values.get("dotany-theme").map(String::as_str) {
        Some("dark") => Some(tauri::Theme::Dark),
        Some("light") => Some(tauri::Theme::Light),
        _ => None,
    };
    // the family's one WebView2: one folder, the same arguments (`webview`). The top bar is the only bar; whose buttons
    // it holds is the platform's (`platform::frame`: on Windows the page's own, so no decorations)
    let mut builder = platform::frame(platform::webview(
        WebviewWindowBuilder::new(app, spec.label, WebviewUrl::App(spec.page.into())),
        webview::data_dir(tool),
    ))
    .theme(theme)
    .title(tool.name)
    .inner_size(size.0, size.1)
    .min_inner_size(spec.min_size.0, spec.min_size.1)
    // the window appears at once in the right colour; the work follows
    .visible(spec.visible)
    .background_color(bg)
    .initialization_script(&script);
    if let Some(p) = place {
        // where it was, as large; behind the window in front unless it was the one in front
        builder = builder
            .position(p.position.0, p.position.1)
            .inner_size(p.size.0, p.size.1)
            .maximized(p.maximized)
            .focused(p.focused);
    }
    let window = match builder.build() {
        Ok(w) => w,
        Err(e) => {
            let row = lock(&state.files).remove(spec.label);
            drop(row);
            return Err(e);
        }
    };
    platform::paint_frame(&window, dark);
    platform::prepare_frame(&window);
    webview::watch(&window, spec.page, spec.size, spec.min_size, kept, spec.visible);
    state
        .window_shown_ms
        .compare_exchange(0, platform::ms_since_process_start() as u64, Ordering::Relaxed, Ordering::Relaxed)
        .ok();
    Ok(window)
}

/// The room for windows on the screen a new window goes to (the one the window in front is on, else the main one), in
/// points: below the menu bar, beside the Dock.
#[cfg(target_os = "macos")]
fn room(app: &AppHandle) -> Option<(f64, f64)> {
    let front = if app.webview_windows().is_empty() { None } else { front_window(app) };
    let monitor =
        front.and_then(|w| w.current_monitor().ok().flatten()).or_else(|| app.primary_monitor().ok().flatten())?;
    let (s, area) = (monitor.scale_factor(), monitor.work_area());
    Some((area.size.width as f64 / s, area.size.height as f64 / s))
}

/// A document window was resized: on the Mac its size, as the person left it (not zoomed, full screen or in the Dock),
/// is the next window's. Kept with the tool's settings, written when the window goes.
#[cfg(target_os = "macos")]
fn keep_size(window: &tauri::Window) {
    let state = window.state::<Base>();
    if !lock(&state.sized).contains(window.label()) {
        return;
    }
    let left_as_is = !window.is_maximized().unwrap_or(true)
        && !window.is_fullscreen().unwrap_or(true)
        && !window.is_minimized().unwrap_or(true);
    let (Ok(size), Ok(scale)) = (window.inner_size(), window.scale_factor()) else { return };
    if !left_as_is || size.width == 0 || size.height == 0 {
        return;
    }
    let text = format!("{}x{}", (size.width as f64 / scale).round(), (size.height as f64 / scale).round());
    state.settings.lock().unwrap().values.insert(WINDOW_SIZE.to_string(), text);
}

fn frame_state(window: &tauri::Window) -> serde_json::Value {
    // the Mac: the system's own full screen as AppKit has it, from the moment it starts going in to the moment it starts
    // coming out (`platform::full_screen`), never Tao's idea of it, which is ahead of the window and can stay wrong
    #[cfg(target_os = "macos")]
    let full = platform::full_screen(window).unwrap_or(false);
    #[cfg(not(target_os = "macos"))]
    let full = window.is_fullscreen().unwrap_or(false);
    serde_json::json!({ "maximized": window.is_maximized().unwrap_or(false), "full": full })
}

/// The tool's builder with the family already in it: the font address, the shared state, asking before closing, the
/// frame's state, and the window. The tool adds its own state, protocols and `invoke_handler(handler![…])`, then runs.
/// The program keeps one file, in its one window (`main`); a tool that keeps a file per window builds with
/// `builder_per_window`.
pub fn builder(tool: &'static Tool) -> tauri::Builder<tauri::Wry> {
    start(tool, None, |app, start| open_window(app, start).map(|_| ()))
}

/// `builder`, for a tool that keeps a file per window: on the Mac a file that arrives while every window has one opens in
/// a new window of this program (`arrive`), and the Dock icon opens an empty one; on Windows each file
/// is a program of its own with one window, and this behaves as `builder` does. The base keeps each window's file (`opened_in`, `saved_in`, `current_in`),
/// and the tool keeps its own state per window too (`PerWindow`), dropping it in `each.forget`.
pub fn builder_per_window(tool: &'static Tool, each: FileWindows) -> tauri::Builder<tauri::Wry> {
    start(tool, Some(each), |app, start| open_file_window(app, start).map(|_| ()))
}

/// `builder`, for a program that opens its own windows (DotAny itself, Text): `open` runs once at the start, with the
/// file the program was started with, and opens them with `open_window_as`.
pub fn builder_with(
    tool: &'static Tool,
    open: impl Fn(&AppHandle, Option<String>) -> tauri::Result<()> + Send + 'static,
) -> tauri::Builder<tauri::Wry> {
    start(tool, None, open)
}

fn start(
    tool: &'static Tool,
    each: Option<FileWindows>,
    open: impl Fn(&AppHandle, Option<String>) -> tauri::Result<()> + Send + 'static,
) -> tauri::Builder<tauri::Wry> {
    let boot = Instant::now();
    fonts::start_reading();
    let since_process_start_ms = platform::ms_since_process_start();
    let data_dir = settings::data_dir(tool);
    // a panic is written down in the tool's folder before the program goes (crash.log)
    crash::install(tool.name, env!("CARGO_PKG_VERSION"), data_dir.clone());
    let mut loaded = settings::load(&data_dir);
    // the recent list and the family's settings are shared: each brought in once, then read from there
    let family_dir = family::join(tool.data_env, &mut loaded.values);
    family::import_recents(&family_dir, tool.folder, &loaded.recents);
    let measure = std::env::var_os("DOTANY_MEASURE").map(PathBuf::from);
    let start_path = std::env::args_os().skip(1).find(|a| !a.to_string_lossy().starts_with("--")).map(|a| {
        let p = PathBuf::from(a);
        std::path::absolute(&p).unwrap_or(p)
    });
    let start_text = start_path.map(|p| p.to_string_lossy().into_owned());
    webview::sweep_old(tool);
    // the Mac's menu bar, its words in the family's language (Windows has none: `menu` gives the builder back as it was)
    let menu_lang = |app: &AppHandle| lock(&app.state::<Base>().settings).values.get("dotany-lang").cloned();
    platform::menu(tauri::Builder::default(), menu_lang)
        // a window whose WebView went comes back before the program could end for want of one
        .plugin(webview::plugin())
        // not on the main thread, so a font on its way never holds up anything else
        .register_asynchronous_uri_scheme_protocol("dotfont", |_ctx, request, responder| {
            let path = request.uri().path().to_string();
            std::thread::spawn(move || responder.respond(fonts::respond(&path)));
        })
        .manage(Base {
            tool,
            settings: Mutex::new(loaded),
            data_dir,
            family_dir,
            family_watch: Mutex::default(),
            files: Mutex::default(),
            each,
            closing: Mutex::default(),
            window_shown_ms: AtomicU64::new(0),
            boot,
            since_process_start_ms,
            measure,
            shown: AtomicBool::new(false),
            #[cfg(target_os = "macos")]
            opener: Mutex::new(None),
            #[cfg(target_os = "macos")]
            early: Mutex::new(Some(Vec::new())),
            #[cfg(target_os = "macos")]
            early_new: Mutex::default(),
            #[cfg(target_os = "macos")]
            sized: Mutex::default(),
        })
        .on_window_event(|window, event| {
            let label = window.label();
            match event {
                WindowEvent::CloseRequested { api, .. } => {
                    // the Mac's Quit learns whether this is its own request or the person closing a window (`quit`)
                    platform::close_requested(label);
                    let state = window.state::<Base>();
                    // a quit given up asks no window that opened meanwhile to close later
                    if !platform::quitting() {
                        state.forget_held();
                    }
                    if !lock(&state.closing).contains(label) {
                        // the page answers: straight away with nothing unsaved, or with the three-answer prompt. Only this
                        // window's page: a program with several windows asks in the one being closed
                        api.prevent_close();
                        let _ = window.emit_to(label, "close-requested", ());
                        // the Mac's Quit reaching a window whose page can't hear it yet (it has only just opened):
                        // asked again once it can (`page_ready`), or the quit would wait for it for ever
                        if platform::quitting() {
                            lock(&state.files).hold_close(label);
                        }
                    }
                }
                WindowEvent::Resized(_) => {
                    let _ = window.emit_to(label, "frame-state", frame_state(window));
                    #[cfg(target_os = "macos")]
                    {
                        keep_size(window);
                        platform::refresh_menu(window.app_handle());
                    }
                }
                // the Mac's Enter Full Screen says what it does to the window in front
                #[cfg(target_os = "macos")]
                WindowEvent::Focused(_) => platform::refresh_menu(window.app_handle()),
                WindowEvent::Destroyed => {
                    // the window's row and the tool's own state for it go with it; first, as a window made again where it
                    // was (its WebView went: `webview` below) starts a row of its own
                    let app = window.app_handle();
                    let state = window.state::<Base>();
                    lock(&state.closing).remove(label);
                    // the size it was left at is kept for the next window (the Mac's)
                    #[cfg(target_os = "macos")]
                    {
                        platform::forget_page(label);
                        if lock(&state.sized).remove(label) {
                            let settings = state.settings.lock().unwrap();
                            settings::store(&state.data_dir, &settings);
                        }
                    }
                    let row = lock(&state.files).remove(label);
                    drop(row);
                    if let Some(each) = &state.each {
                        (each.forget)(app, label);
                    }
                    // the Mac's Quit asks the next window
                    platform::window_gone(app, label);
                    #[cfg(target_os = "macos")]
                    platform::refresh_menu(app);
                }
                _ => {}
            }
            // the window's WebView: its memory in the background, and a window on its way back
            webview::on_window_event(window, event);
        })
        .setup(move |app| {
            crash::set_version(&app.package_info().version.to_string());
            watch_family(app.handle());
            #[cfg(target_os = "macos")]
            testdrive::start(app.handle());
            // the trackpad's smart zoom reaches the page (smart_zoom.rs)
            #[cfg(target_os = "macos")]
            smart_zoom::watch(app.handle());
            open(app.handle(), start_text.clone())?;
            #[cfg(target_os = "macos")]
            {
                let state = app.state::<Base>();
                // a click on the Dock icon with no window left opens one again (`reopen`)
                *lock(&state.opener) = Some(Box::new(open));
                // the Dock icon's Quit and logging out ask about unsaved work as ⌘Q does
                platform::guard_system_quit(app.handle());
                // the Window menu lists the open windows
                platform::menu_ready(app.handle());
                // files that came before the window did go where any file goes now, and new documents asked for
                let early = lock(&state.early).take().unwrap_or_default();
                if !early.is_empty() {
                    hand_files(app.handle(), early);
                }
                let news = std::mem::take(&mut *lock(&state.early_new));
                for format in news {
                    start_new(app.handle(), format);
                }
            }
            Ok(())
        })
}

// ---------- running: what the system hands a program that is already open ----------

/// What the Mac hands a running program: files opened in Finder, dropped on the Dock icon or chosen in Open With (they
/// come as an event, never on the command line), a click on the Dock icon when no window is open, and the last window
/// closing (the program stays, as Mac programs do; ⌘Q quits). On Windows each file starts a process of its own, so this
/// is only Tauri's `run`. A tool ends its builder with `.run_family(tauri::generate_context!())`.
pub trait RunFamily {
    /// Files that arrive go to the window's page if it has none open, otherwise to another window of their own.
    fn run_family(self, context: tauri::Context<tauri::Wry>) -> tauri::Result<()>;
    /// The same, but the tool says where files that arrive go (Text: a window each; DotAny itself: the converter).
    fn run_family_with(
        self,
        context: tauri::Context<tauri::Wry>,
        arrive: impl Fn(&AppHandle, Vec<PathBuf>) + Send + Sync + 'static,
    ) -> tauri::Result<()>;
    /// `run_family`, and `on_exit` runs as the program ends (Recorder finishes its recording).
    fn run_family_then(
        self,
        context: tauri::Context<tauri::Wry>,
        on_exit: impl Fn(&AppHandle) + Send + 'static,
    ) -> tauri::Result<()>;
}

impl RunFamily for tauri::Builder<tauri::Wry> {
    fn run_family(self, context: tauri::Context<tauri::Wry>) -> tauri::Result<()> {
        run(self, context, arrive, |_| {})
    }

    fn run_family_with(
        self,
        context: tauri::Context<tauri::Wry>,
        arrive: impl Fn(&AppHandle, Vec<PathBuf>) + Send + Sync + 'static,
    ) -> tauri::Result<()> {
        run(self, context, arrive, |_| {})
    }

    fn run_family_then(
        self,
        context: tauri::Context<tauri::Wry>,
        on_exit: impl Fn(&AppHandle) + Send + 'static,
    ) -> tauri::Result<()> {
        run(self, context, arrive, on_exit)
    }
}

/// On Windows what `tauri::Builder::run` does, and `on_exit` at `RunEvent::Exit`; the rest is the Mac's.
fn run(
    builder: tauri::Builder<tauri::Wry>,
    context: tauri::Context<tauri::Wry>,
    arrive: impl Fn(&AppHandle, Vec<PathBuf>) + Send + Sync + 'static,
    on_exit: impl Fn(&AppHandle) + Send + 'static,
) -> tauri::Result<()> {
    // where files go, also for the File menu's Open… and a window's open panel (`hand_files`)
    #[cfg(target_os = "macos")]
    let _ = ARRIVE.set(std::sync::Arc::new(arrive));
    #[cfg(not(target_os = "macos"))]
    let _ = arrive;
    let app = builder.build(context)?;
    app.run(move |app, event| match event {
        // files opened in Finder or from the Dock icon's menu, and the tool's own address (DotAny's New)
        #[cfg(target_os = "macos")]
        tauri::RunEvent::Opened { urls } => {
            let paths: Vec<PathBuf> =
                urls.iter().filter(|u| u.scheme() == "file").filter_map(|u| u.to_file_path().ok()).collect();
            if !paths.is_empty() {
                hand_files(app, paths);
            }
            for format in urls.iter().filter_map(new_asked) {
                start_new(app, format);
            }
        }
        #[cfg(target_os = "macos")]
        tauri::RunEvent::Reopen { has_visible_windows: false, .. } => reopen(app),
        // after Tauri's own development-build Dock icon: the bundle's icon, as macOS draws it
        #[cfg(target_os = "macos")]
        tauri::RunEvent::Ready => platform::use_bundle_icon(),
        #[cfg(target_os = "macos")]
        tauri::RunEvent::ExitRequested { code: None, api, .. }
            if platform::STAYS_WITHOUT_WINDOWS && !platform::quitting() =>
        {
            api.prevent_exit()
        }
        tauri::RunEvent::Exit => on_exit(app),
        _ => {}
    });
    Ok(())
}

/// Where files go when the tool doesn't say, each in turn: a file already open in a window brings that window forward; a
/// window with nothing open and nothing on its way takes it (its page once it can: `page_ready`); otherwise a tool with a
/// file per window opens it in a new window of this program, cascaded from the one in front. A tool built with `builder`
/// keeps one file for the program and opens it in another copy of the program (`platform::open_another`). Only the Mac
/// hands a running program files.
#[cfg(target_os = "macos")]
fn arrive(app: &AppHandle, paths: Vec<PathBuf>) {
    let state = app.state::<Base>();
    for path in paths {
        let open = lock(&state.files).files();
        if let Some((label, _)) = open.iter().find(|(_, p)| same_file(p, &path)) {
            bring(app, label);
            continue;
        }
        let free = lock(&state.files).free();
        if let Some(label) = free {
            hand_over(app, &label, path);
            bring(app, &label);
            continue;
        }
        if state.per_window() {
            let _ = open_file_window(app, Some(path.to_string_lossy().into_owned()));
        } else {
            platform::open_another(&path);
        }
    }
}

#[cfg(not(target_os = "macos"))]
fn arrive(_app: &AppHandle, _paths: Vec<PathBuf>) {}

/// A window brought forward, out of the Dock if it was there.
#[cfg(target_os = "macos")]
fn bring(app: &AppHandle, label: &str) {
    if let Some(w) = app.get_webview_window(label) {
        platform::bring_forward(&w);
    }
}

/// Where files go, as the tool says (`run_family`'s `arrive`, or the tool's own): set as the program starts.
#[cfg(target_os = "macos")]
type Arrive = std::sync::Arc<dyn Fn(&AppHandle, Vec<PathBuf>) + Send + Sync>;
#[cfg(target_os = "macos")]
static ARRIVE: std::sync::OnceLock<Arrive> = std::sync::OnceLock::new();

/// Files for the program (opened in Finder, chosen in the File menu's Open… or a window's open panel): before the first
/// window, kept until the program has made it (`start`'s setup); then where the tool says, on the main thread.
#[cfg(target_os = "macos")]
pub(crate) fn hand_files(app: &AppHandle, paths: Vec<PathBuf>) {
    let state = app.state::<Base>();
    {
        let mut early = lock(&state.early);
        if let Some(list) = early.as_mut() {
            list.extend(paths);
            return;
        }
    }
    let Some(arrive) = ARRIVE.get().cloned() else { return };
    if platform::on_main_thread() {
        arrive(app, paths);
    } else {
        let a = app.clone();
        let _ = app.run_on_main_thread(move || arrive(&a, paths));
    }
}

/// The file chosen in a window's open panel goes where a file from Finder goes: the window opens it itself (Some) only
/// when it holds nothing and the file isn't open in another window; otherwise it brings that window forward, or opens in
/// a new one (`hand_files`), and this window stays as it is.
#[cfg(target_os = "macos")]
pub(crate) fn chosen_in(app: &AppHandle, label: &str, path: PathBuf) -> Option<PathBuf> {
    let state = app.state::<Base>();
    let (open, here) = {
        let files = lock(&state.files);
        (files.files(), files.can_take(label))
    };
    if here && !open.iter().any(|(_, p)| same_file(p, &path)) {
        return Some(path);
    }
    hand_files(app, vec![path]);
    None
}

/// A new document asked for by the tool's own address, `dotany-<tool>://new` (DotAny's New on the Mac:
/// `dotany-sheets://new?format=xlsx`): the kind, or "" for the tool's own choice. Other addresses are nothing here.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn new_asked(url: &tauri::Url) -> Option<String> {
    if !url.scheme().starts_with("dotany") || url.host_str() != Some("new") {
        return None;
    }
    let format = url.query_pairs().find(|(k, _)| k == "format").map(|(_, v)| v.into_owned()).unwrap_or_default();
    // a kind is an extension's letters, nothing more
    Some(format.trim_start_matches('.').chars().filter(|c| c.is_ascii_alphanumeric()).collect())
}

/// A new document, as DotAny's New asks: in an empty window of this program, or a new one (a tool with a file per
/// window); a program that opens its own windows (Text) is asked in the window in front, as its own New is, and makes
/// it in another window if that one holds a file. A tool that makes no new files opens an empty window.
#[cfg(target_os = "macos")]
fn start_new(app: &AppHandle, format: String) {
    let state = app.state::<Base>();
    if lock(&state.early).is_some() {
        lock(&state.early_new).push(format);
        return;
    }
    if state.per_window() {
        let free = lock(&state.files).free();
        match free {
            Some(label) => {
                bring(app, &label);
                tell_page(app, &label, "new-file", format.into());
            }
            None => drop(open_file_window_new(app, None, Some(format))),
        }
        return;
    }
    to_page(app, "new-file", format.into());
}

/// Something the menu bar or the system asks of a page (Settings, About, New): the window in front, brought forward, or
/// with none open a new empty one, which hears it once its page is ready.
#[cfg(target_os = "macos")]
pub(crate) fn to_page(app: &AppHandle, event: &str, payload: serde_json::Value) {
    let front = if app.webview_windows().is_empty() { None } else { front_window(app) };
    let label = match front {
        Some(w) => {
            bring(app, w.label());
            w.label().to_string()
        }
        None => {
            let state = app.state::<Base>();
            if let Some(open) = lock(&state.opener).as_ref() {
                let _ = open(app, None);
            };
            let newest = lock(&state.files).newest();
            let Some(label) = newest else { return };
            label
        }
    };
    tell_page(app, &label, event, payload);
}

/// `event` for one window's page: now if it is ready, else once it is (`page_ready`).
#[cfg(target_os = "macos")]
fn tell_page(app: &AppHandle, label: &str, event: &str, payload: serde_json::Value) {
    let now = lock(&app.state::<Base>().files).tell(label, event, payload);
    if let Some((event, payload)) = now {
        let _ = app.emit_to(label, &event, payload);
    }
}

/// Hands `path` to this window's page, as a file that arrives goes to an empty window: now if its page is ready, else
/// when it is (`page_ready`). False when the window can't take one (it has a file, or one is on its way to it). For a
/// tool that opens its own windows and says where files go itself (Text), so a file that starts the program, or comes
/// after the Dock icon opened an empty window, fills that window rather than open another beside it.
pub fn hand_over(app: &AppHandle, label: &str, path: PathBuf) -> bool {
    let state = app.state::<Base>();
    let now = {
        let mut files = lock(&state.files);
        if !files.can_take(label) {
            return false;
        }
        files.hand(label, path)
    };
    if let Some(path) = now {
        let _ = app.emit_to(label, "open-path", path.to_string_lossy());
    }
    true
}

/// This window's page is ready: a file handed to it before it was is handed over now. (Only the Mac hands files over.)
/// Also the Mac's Quit, if it asked the window to close before its page could hear it.
pub(crate) fn page_ready(app: &AppHandle, label: &str) {
    let state = app.state::<Base>();
    let (waiting, told, close) = {
        let mut files = lock(&state.files);
        (files.ready(label), files.told(label), files.ready_to_close(label))
    };
    if let Some(path) = waiting {
        let _ = app.emit_to(label, "open-path", path.to_string_lossy());
    }
    // what the Mac's menu bar or DotAny asked of it meanwhile (Settings, a new document)
    for (event, payload) in told {
        let _ = app.emit_to(label, &event, payload);
    }
    if close {
        let _ = app.emit_to(label, "close-requested", ());
    }
}

/// No window left and the Dock icon clicked: a window again, empty. A window kept hidden (DotAny started at login,
/// out of the Dock) comes forward instead, and the program takes its place in the Dock.
#[cfg(target_os = "macos")]
fn reopen(app: &AppHandle) {
    if let Some(w) = app.webview_windows().into_values().find(|w| !w.is_visible().unwrap_or(true)) {
        let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
        return;
    }
    if !app.webview_windows().is_empty() {
        return;
    }
    let state = app.state::<Base>();
    // (a statement: the lock's guard goes before `state` does)
    if let Some(open) = lock(&state.opener).as_ref() {
        let _ = open(app, None);
    };
}

/// `tauri::generate_handler!` with the family's commands already listed. The tool adds its own after them.
#[macro_export]
macro_rules! handler {
    ($($cmd:path),* $(,)?) => {
        ::tauri::generate_handler![
            ::dotany_base::commands::set_setting, ::dotany_base::commands::forget_recent, ::dotany_base::commands::can_open,
            ::dotany_base::commands::pick_open, ::dotany_base::commands::pick_save, ::dotany_base::commands::default_folder,
            ::dotany_base::commands::neighbour, ::dotany_base::commands::close_document, ::dotany_base::commands::keep_mine,
            ::dotany_base::commands::set_frame, ::dotany_base::commands::window_control, ::dotany_base::commands::window_menu,
            ::dotany_base::commands::set_snap_button, ::dotany_base::commands::set_title, ::dotany_base::commands::window_ready,
            ::dotany_base::commands::startup_done, ::dotany_base::commands::close_window, ::dotany_base::commands::print_window,
            ::dotany_base::commands::set_menu_words, ::dotany_base::commands::set_edited,
            $($cmd),*
        ]
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_window_opens_at_the_size_one_was_last_left_at_and_always_fits() {
        let (default, min, room) = ((1180.0, 800.0), (480.0, 420.0), Some((1440.0, 875.0)));
        // nothing kept yet: the tool's own size
        assert_eq!(fitted_size(None, default, min, room), (1180.0, 800.0));
        // the size the person left a window at
        assert_eq!(fitted_size(Some("900x600"), default, min, room), (900.0, 600.0));
        // left large on a bigger display: no larger than this screen's room for windows
        assert_eq!(fitted_size(Some("2400x1500"), default, min, room), (1440.0, 875.0));
        // never smaller than the tool's smallest, whatever was kept
        assert_eq!(fitted_size(Some("200x100"), default, min, room), (480.0, 420.0));
        // what can't be read is as if nothing were kept
        for bad in ["", "x", "900", "900x", "-5x600", "NaNx600", "infx600", "900×600"] {
            assert_eq!(fitted_size(Some(bad), default, min, room), (1180.0, 800.0), "{bad:?}");
        }
        // the tool's own size fits too, on a small screen; with no screen known, as it is
        assert_eq!(fitted_size(None, default, min, Some((1024.0, 700.0))), (1024.0, 700.0));
        assert_eq!(fitted_size(Some("2400x1500"), default, min, None), (2400.0, 1500.0));
    }

    #[test]
    fn only_the_tools_own_address_asks_for_a_new_document() {
        let new = |s: &str| new_asked(&tauri::Url::parse(s).unwrap());
        assert_eq!(new("dotany-sheets://new?format=xlsx"), Some("xlsx".into()));
        assert_eq!(new("dotany-text://new?format=.md"), Some("md".into()));
        assert_eq!(new("dotany-text://new"), Some(String::new()));
        assert_eq!(new("dotany-text://new?format=md%22%3Balert(1)"), Some("mdalert1".into()));
        assert_eq!(new("dotany://convert?file=a"), None);
        assert_eq!(new("https://new/?format=md"), None);
        assert_eq!(new("file:///home/me/new"), None);
    }
}
