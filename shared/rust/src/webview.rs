//! The family's one WebView: what the base does when it goes, and how a window in the background lets go of memory.
//!
//! - **One browser for every program.** Each program's WebView keeps its data in one folder,
//!   `platform::shared_webview_dir()`, with the same arguments (`platform::browser_args`), so the second program joins
//!   the first one's browser, which saves memory and opening time. `DOTANY_WEBVIEW_DATA` moves the folder; a test that
//!   moved a tool's own data folder gets one inside it, so tests stay apart.
//! - **A window comes back.** When that browser goes, every window of every program goes with it; when a page goes,
//!   only its window. Either way the window is made again where it was, as large, with the file it had open
//!   (`set_reopen` for a tool that keeps a file per window). What wasn't saved is lost.
//! - **A window in the background gives back memory.** Minimized or hidden for a few seconds, or behind other windows
//!   for two minutes, its page goes down to WebView2's low memory level, unless it is playing sound or the page says
//!   it is busy (`app.quiet`: Video playing, Recorder recording); it is back to normal the moment it comes forward.

use crate::{Tool, WindowSpec, open_window_at, platform};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager, PhysicalPosition, PhysicalSize, Runtime, WebviewWindow, WindowEvent};

/// Where this program's WebView keeps its data: the family's folder, unless a test moved things.
pub fn data_dir(tool: &Tool) -> PathBuf {
    if let Some(d) = std::env::var_os("DOTANY_WEBVIEW_DATA") {
        return PathBuf::from(d);
    }
    if let Some(d) = std::env::var_os(tool.data_env) {
        return PathBuf::from(d).join("webview");
    }
    platform::shared_webview_dir()
}

/// Earlier versions kept a WebView folder of each tool's own, `<its data folder>\webview` (a cache of some tens of MB).
/// Nothing in it is the user's; it is removed 20 seconds after the start, so the removal never slows the start.
pub(crate) fn sweep_old(tool: &Tool) {
    if std::env::var_os(tool.data_env).is_some() || std::env::var_os("DOTANY_WEBVIEW_DATA").is_some() {
        return;
    }
    let old = platform::data_dir(tool.folder).join("webview");
    if old.is_dir() {
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(20));
            let _ = std::fs::remove_dir_all(old);
        });
    }
}

// ---------- a window comes back ----------

/// What a window was made from, to make it again.
#[derive(Clone)]
struct Kept {
    page: String,
    size: (f64, f64),
    min_size: (f64, f64),
    extra: serde_json::Value,
}

/// Where and how a window was, to make it again there.
#[derive(Clone)]
pub(crate) struct Place {
    /// Logical pixels, for the builder; the physical ones are put back exactly once it is made.
    pub position: (f64, f64),
    pub size: (f64, f64),
    pub maximized: bool,
    pub focused: bool,
    exact: Option<(PhysicalPosition<i32>, PhysicalSize<u32>)>,
    minimized: bool,
}

/// A window on its way back: what it was made from, and where and how it was.
#[derive(Clone)]
struct Coming {
    kept: Kept,
    place: Place,
    visible: bool,
}

static KEPT: LazyLock<Mutex<HashMap<String, Kept>>> = LazyLock::new(Mutex::default);
static COMING: LazyLock<Mutex<HashMap<String, Coming>>> = LazyLock::new(Mutex::default);
/// When each window last came back: a browser that keeps going isn't followed forever.
static RETURNS: LazyLock<Mutex<HashMap<String, Vec<Instant>>>> = LazyLock::new(Mutex::default);
/// Windows that came back behind others: their page's `window_ready` doesn't bring them forward.
static BEHIND: LazyLock<Mutex<Vec<String>>> = LazyLock::new(Mutex::default);

/// A tool that keeps its own documents (Text) says what a window had open: it changes the window's `extra` so the page
/// opens that file again. Without it, `extra.start` becomes the window's open file (`Base::current_in`).
static REOPEN: std::sync::OnceLock<fn(&AppHandle, &str, &mut serde_json::Value)> = std::sync::OnceLock::new();

pub fn set_reopen(reopen: fn(&AppHandle, &str, &mut serde_json::Value)) {
    let _ = REOPEN.set(reopen);
}

/// Called by `open_window_as` for every window, once it is made.
pub(crate) fn watch(
    window: &WebviewWindow,
    page: &str,
    size: (f64, f64),
    min_size: (f64, f64),
    extra: serde_json::Value,
    visible: bool,
) {
    let label = window.label().to_string();
    KEPT.lock().unwrap().insert(label.clone(), Kept { page: page.to_string(), size, min_size, extra });
    let (a, b) = (window.clone(), window.clone());
    platform::watch_webview(window, move |gone| went(&a, gone), move |on| heard(&b, on));
    REST.lock().unwrap().insert(label, Rest { away_since: Some(Instant::now()), ..Rest::default() });
    // made hidden (DotAny at login): it rests from the start; otherwise once it has stayed behind
    later(window, if visible { AWAY } else { HIDDEN });
}

/// The browser or the page went. Not inside WebView2's own event: the window and its WebView are about to be dropped.
fn went(window: &WebviewWindow, _gone: platform::Gone) {
    let w = window.clone();
    std::thread::spawn(move || {
        let inner = w.clone();
        let _ = w.run_on_main_thread(move || make_again(&inner));
    });
}

fn make_again(window: &WebviewWindow) {
    let label = window.label().to_string();
    if COMING.lock().unwrap().contains_key(&label) {
        return;
    }
    {
        // three times in a minute: the browser can't stay up, and the window stays as it is
        let mut returns = RETURNS.lock().unwrap();
        let list = returns.entry(label.clone()).or_default();
        list.retain(|t| t.elapsed() < Duration::from_secs(60));
        if list.len() >= 3 {
            return;
        }
        list.push(Instant::now());
    }
    let Some(mut kept) = KEPT.lock().unwrap().get(&label).cloned() else { return };
    let app = window.app_handle();
    match REOPEN.get() {
        Some(reopen) => reopen(app, &label, &mut kept.extra),
        None => {
            if let Some(extra) = kept.extra.as_object_mut().filter(|e| e.contains_key("start")) {
                if let Some(path) = app.state::<crate::Base>().current_in(&label) {
                    extra.insert("start".into(), path.to_string_lossy().into_owned().into());
                    extra.insert("new".into(), serde_json::Value::Null);
                } else {
                    extra.insert("start".into(), serde_json::Value::Null);
                }
            }
        }
    }
    let maximized = window.is_maximized().unwrap_or(false);
    let minimized = window.is_minimized().unwrap_or(false);
    let exact = if maximized || minimized {
        // where it goes when restored; the outer size less the frame, if it has one
        let frame = match (window.outer_size(), window.inner_size()) {
            (Ok(o), Ok(i)) if !minimized => (o.width.saturating_sub(i.width), o.height.saturating_sub(i.height)),
            _ => (0, 0),
        };
        platform::normal_place(window)
            .map(|(p, s)| (p, PhysicalSize::new(s.width.saturating_sub(frame.0), s.height.saturating_sub(frame.1))))
    } else {
        match (window.outer_position(), window.inner_size()) {
            (Ok(p), Ok(s)) => Some((p, s)),
            _ => None,
        }
    };
    let scale = window.scale_factor().unwrap_or(1.0);
    let (position, size) = match exact {
        Some((p, s)) => ((p.x as f64 / scale, p.y as f64 / scale), (s.width as f64 / scale, s.height as f64 / scale)),
        None => ((0.0, 0.0), kept.size),
    };
    let place = Place { position, size, maximized, focused: window.is_focused().unwrap_or(false), exact, minimized };
    let coming = Coming { kept, place, visible: window.is_visible().unwrap_or(true) };
    COMING.lock().unwrap().insert(label, coming);
    // its Destroyed brings it back (`on_window_event`); until then the program doesn't end for want of a window (`plugin`)
    let _ = window.destroy();
}

fn come_back(app: AppHandle, label: String) {
    // stays on the list until the new window is made: meanwhile the program must not end for want of a window
    let Some(c) = COMING.lock().unwrap().get(&label).cloned() else { return };
    // made where it was, shown without taking the front from the window that has it (unless it had it); a minimized
    // window is made hidden and minimized at once
    let visible = c.visible && !c.place.minimized;
    let spec = WindowSpec {
        label: &label,
        page: &c.kept.page,
        size: c.kept.size,
        min_size: c.kept.min_size,
        visible,
        extra: c.kept.extra.clone(),
    };
    if !c.place.focused {
        BEHIND.lock().unwrap().push(label.clone());
    }
    let made = open_window_at(&app, spec, Some(&c.place));
    COMING.lock().unwrap().remove(&label);
    match made {
        Ok(w) => {
            // exactly: the builder's logical pixels went through one monitor's scale
            if let (Some((pos, size)), false) = (c.place.exact, c.place.maximized) {
                let _ = w.set_position(pos);
                let _ = w.set_size(size);
            }
            if c.visible && c.place.minimized {
                let _ = w.minimize();
            }
        }
        Err(_) => {
            BEHIND.lock().unwrap().retain(|l| l != &label);
            // nothing to come back to: the program ends as it would have
            if app.webview_windows().is_empty() {
                app.exit(0);
            }
        }
    }
}

/// The window came back behind others: its page's `window_ready` leaves it there. Asked once.
pub(crate) fn stays_behind(label: &str) -> bool {
    let mut behind = BEHIND.lock().unwrap();
    let was = behind.iter().any(|l| l == label);
    behind.retain(|l| l != label);
    was
}

/// Keeps the program running while a window is on its way back: its last window gone, Tauri would end it.
pub(crate) fn plugin<R: Runtime>() -> tauri::plugin::TauriPlugin<R> {
    tauri::plugin::Builder::new("dotany-webview")
        .on_event(|_, event| {
            if let tauri::RunEvent::ExitRequested { code: None, api, .. } = event {
                if !COMING.lock().unwrap().is_empty() {
                    api.prevent_exit();
                }
            }
        })
        .build()
}

// ---------- a window in the background gives back memory ----------

/// Minimized, or made hidden: rests after this long.
const HIDDEN: Duration = Duration::from_secs(3);
/// Behind other windows, still in sight: rests after this long. `DOTANY_REST_AFTER_MS` changes it for a test.
const AWAY: Duration = Duration::from_secs(120);
/// Busy (sound playing, Video's film, Recorder recording): asked again after this long.
const AGAIN: Duration = Duration::from_secs(30);

fn away_after() -> Duration {
    std::env::var("DOTANY_REST_AFTER_MS").ok().and_then(|v| v.parse().ok()).map(Duration::from_millis).unwrap_or(AWAY)
}

#[derive(Default)]
struct Rest {
    /// Every change of mind counts one up; a check that finds another number was overtaken.
    turn: u64,
    low: bool,
    away_since: Option<Instant>,
}

static REST: LazyLock<Mutex<HashMap<String, Rest>>> = LazyLock::new(Mutex::default);

/// From the base's window events, for every window.
pub(crate) fn on_window_event(window: &tauri::Window, event: &WindowEvent) {
    let Some(w) = window.app_handle().get_webview_window(window.label()) else {
        if let WindowEvent::Destroyed = event {
            gone_for_good(window.app_handle(), window.label());
        }
        return;
    };
    match event {
        // a window out of sight can still be told it has focus (DotAny at login, never shown): it keeps resting
        WindowEvent::Focused(true) if out_of_sight(&w) => later(&w, HIDDEN),
        WindowEvent::Focused(true) => forward(&w),
        WindowEvent::Focused(false) => {
            if let Some(r) = REST.lock().unwrap().get_mut(w.label()) {
                r.away_since = Some(Instant::now());
            }
            later(&w, AWAY);
        }
        WindowEvent::Resized(_) => {
            if w.is_minimized().unwrap_or(false) {
                later(&w, HIDDEN);
            } else if w.is_focused().unwrap_or(false) && !out_of_sight(&w) {
                forward(&w);
            }
        }
        WindowEvent::Destroyed => gone_for_good(window.app_handle(), window.label()),
        _ => {}
    }
}

fn gone_for_good(app: &AppHandle, label: &str) {
    REST.lock().unwrap().remove(label);
    if COMING.lock().unwrap().contains_key(label) {
        // made again off the main thread: a window made inside a window event can wait forever for its WebView
        let (app, label) = (app.clone(), label.to_string());
        std::thread::spawn(move || come_back(app, label));
    } else {
        KEPT.lock().unwrap().remove(label);
    }
}

/// A program showed a hidden window itself (DotAny at login, opened again): back to normal at once. Windows may
/// already count that window as focused, and then no focus event comes to say so.
pub fn shown(window: &WebviewWindow) {
    forward(window);
}

/// The window is in front again: back to normal at once.
fn forward(window: &WebviewWindow) {
    let was_low = {
        let mut rest = REST.lock().unwrap();
        let Some(r) = rest.get_mut(window.label()) else { return };
        r.turn += 1;
        r.away_since = None;
        std::mem::replace(&mut r.low, false)
    };
    if was_low {
        platform::wake(window);
    }
}

/// Sound started or stopped. Started while resting: back to normal; stopped: it may rest again soon.
fn heard(window: &WebviewWindow, on: bool) {
    let was_low = {
        let mut rest = REST.lock().unwrap();
        let Some(r) = rest.get_mut(window.label()) else { return };
        if on {
            r.turn += 1;
        }
        on && std::mem::replace(&mut r.low, false)
    };
    if was_low {
        platform::wake(window);
    }
    if !on {
        later(window, HIDDEN);
    }
}

/// Looks again after `wait`, unless something changes meanwhile.
fn later(window: &WebviewWindow, wait: Duration) {
    let wait = if wait == AWAY { away_after() } else { wait };
    let turn = {
        let mut rest = REST.lock().unwrap();
        let Some(r) = rest.get_mut(window.label()) else { return };
        r.turn += 1;
        r.turn
    };
    let w = window.clone();
    std::thread::spawn(move || {
        std::thread::sleep(wait);
        try_rest(&w, turn);
    });
}

fn current(label: &str, turn: u64) -> bool {
    REST.lock().unwrap().get(label).is_some_and(|r| r.turn == turn && !r.low)
}

/// Minimized or hidden: whatever Windows says about focus, nobody is looking at it
fn out_of_sight(window: &WebviewWindow) -> bool {
    window.is_minimized().unwrap_or(false) || !window.is_visible().unwrap_or(true)
}

fn try_rest(window: &WebviewWindow, turn: u64) {
    let label = window.label().to_string();
    if !current(&label, turn) {
        return;
    }
    if !out_of_sight(window) {
        let behind =
            REST.lock().unwrap().get(&label).and_then(|r| r.away_since).map_or(true, |t| t.elapsed() >= away_after());
        if window.is_focused().unwrap_or(true) || !behind {
            return;
        }
    }
    let (check, done) = (label.clone(), label);
    let w = window.clone();
    platform::rest(
        window,
        move || current(&check, turn),
        move |rested| {
            if rested {
                if let Some(r) = REST.lock().unwrap().get_mut(&done) {
                    r.low = true;
                }
            } else if current(&done, turn) {
                // busy, or the page didn't answer: ask again in a while
                later(&w, AGAIN);
            }
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool(env: &'static str) -> Tool {
        fn yes(_: &std::path::Path) -> bool {
            true
        }
        fn nothing(_: &AppHandle) {}
        Tool {
            name: "T",
            folder: "T",
            data_env: env,
            user_dir: "Documents",
            opens_files: true,
            extensions: &[],
            sniff: yes,
            size: (1.0, 1.0),
            min_size: (1.0, 1.0),
            on_close: nothing,
        }
    }

    #[test]
    fn every_program_shares_one_folder_unless_a_test_moved_things() {
        // one test, in order: the variables are the process's own
        std::env::remove_var("DOTANY_WEBVIEW_DATA");
        std::env::remove_var("DOTANY_WVTEST_A_DATA");
        std::env::remove_var("DOTANY_WVTEST_B_DATA");
        let (a, b) = (tool("DOTANY_WVTEST_A_DATA"), tool("DOTANY_WVTEST_B_DATA"));
        assert_eq!(data_dir(&a), data_dir(&b));
        assert_eq!(data_dir(&a), platform::shared_webview_dir());
        std::env::set_var("DOTANY_WVTEST_A_DATA", r"C:\t\a");
        // joined, not written out: the separator is the system's
        assert_eq!(data_dir(&a), PathBuf::from(r"C:\t\a").join("webview"));
        assert_eq!(data_dir(&b), platform::shared_webview_dir());
        std::env::set_var("DOTANY_WEBVIEW_DATA", r"C:\t\shared");
        assert_eq!(data_dir(&a), PathBuf::from(r"C:\t\shared"));
        assert_eq!(data_dir(&b), PathBuf::from(r"C:\t\shared"));
        std::env::remove_var("DOTANY_WEBVIEW_DATA");
        std::env::remove_var("DOTANY_WVTEST_A_DATA");
    }

    // WebView2's arguments; the Mac's WKWebView takes none
    #[cfg(windows)]
    #[test]
    fn every_program_gives_the_same_arguments() {
        std::env::remove_var("DOTANY_DEBUG_PORT");
        assert_eq!(platform::browser_args(), "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection");
    }
}
