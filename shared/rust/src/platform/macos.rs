//! The macOS side of the platform module: the same functions as `windows.rs`, the Mac way.
//!
//! The window keeps the system's own frame here: the red, yellow and green buttons sit in the top
//! bar's left, drawn by the system over the page, so the Windows frame work (`paint_frame`, `prepare_frame`,
//! `set_snap_button`, `show_system_menu`) has nothing to do. The menu bar is the system's too, with only the app's own
//! menu and the edit menu (`menu`).

use crate::atomic::WriteFailure;
use std::ffi::{CString, OsStr};
use std::fs::{File, OpenOptions};
use std::io;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use tauri::menu::{IsMenuItem, Menu, MenuEvent, MenuItem, PredefinedMenuItem, Submenu};
use tauri::webview::WebviewWindowBuilder;
use tauri::{AppHandle, Emitter, Manager, Runtime, TitleBarStyle, WebviewWindow, Wry};

fn c_path(path: &Path) -> io::Result<CString> {
    CString::new(path.as_os_str().as_bytes()).map_err(|_| io::Error::from(io::ErrorKind::InvalidInput))
}

// ---------- files ----------

pub fn write_failure(err: &io::Error) -> WriteFailure {
    match err.raw_os_error() {
        Some(libc::EBUSY) | Some(libc::ETXTBSY) => WriteFailure::Locked,
        Some(libc::ENOSPC) | Some(libc::EDQUOT) => WriteFailure::DiskFull,
        Some(libc::EACCES) | Some(libc::EPERM) | Some(libc::EROFS) => WriteFailure::Denied,
        Some(libc::ENOENT) | Some(libc::ENOTDIR) => WriteFailure::Missing,
        _ => WriteFailure::Other,
    }
}

/// Read-only the way Finder means it: no write permission for this process, or Locked (the locked flag).
pub fn is_read_only(path: &Path) -> bool {
    let Ok(c) = c_path(path) else { return false };
    let mut st: libc::stat = unsafe { std::mem::zeroed() };
    if unsafe { libc::stat(c.as_ptr(), &mut st) } != 0 {
        return false;
    }
    if st.st_flags & (libc::UF_IMMUTABLE | libc::SF_IMMUTABLE) != 0 {
        return true;
    }
    unsafe { libc::access(c.as_ptr(), libc::W_OK) != 0 }
}

/// A new file nobody should notice while it exists: its name starts with a dot (`atomic::TEMP_PREFIX`), which Finder
/// doesn't show, and nobody else can read it until it takes the old file's permissions.
pub fn create_temp(path: &Path) -> io::Result<File> {
    OpenOptions::new().write(true).create_new(true).mode(0o600).open(path)
}

/// The new file takes the old one's place in one step (rename), keeping what Windows' ReplaceFile keeps: the old file's
/// permissions, owner, access list and extended attributes (Finder tags, comments), and its creation date.
pub fn replace_file(target: &Path, temp: &Path) -> io::Result<()> {
    let (t, r) = (c_path(target)?, c_path(temp)?);
    // what the old file is besides its bytes; never its times, so the new file is plainly newer
    unsafe {
        libc::copyfile(t.as_ptr(), r.as_ptr(), std::ptr::null_mut(), libc::COPYFILE_SECURITY | libc::COPYFILE_XATTR)
    };
    if let Ok(created) = std::fs::metadata(target).and_then(|m| m.created()) {
        set_created(&r, created);
    }
    std::fs::rename(temp, target)
}

/// The creation date Finder shows, as the old file had it.
fn set_created(path: &CString, when: std::time::SystemTime) {
    let Ok(since) = when.duration_since(std::time::UNIX_EPOCH) else { return };
    let mut attrs: libc::attrlist = unsafe { std::mem::zeroed() };
    attrs.bitmapcount = libc::ATTR_BIT_MAP_COUNT;
    attrs.commonattr = libc::ATTR_CMN_CRTIME;
    let mut time =
        libc::timespec { tv_sec: since.as_secs() as libc::time_t, tv_nsec: since.subsec_nanos() as libc::c_long };
    unsafe {
        libc::setattrlist(
            path.as_ptr(),
            &mut attrs as *mut _ as *mut libc::c_void,
            &mut time as *mut _ as *mut libc::c_void,
            std::mem::size_of::<libc::timespec>(),
            0,
        );
    }
}

/// The temp file becomes a new file, with the permissions a new file would have.
pub fn place_file(target: &Path, temp: &Path) -> io::Result<()> {
    let mask = unsafe {
        let m = libc::umask(0o022);
        libc::umask(m);
        m
    };
    let _ = std::fs::set_permissions(temp, std::os::unix::fs::PermissionsExt::from_mode(0o666 & !(mask as u32)));
    // rename would quietly replace a file that appeared in the meantime; this one refuses, as MoveFileEx does
    let (t, r) = (c_path(target)?, c_path(temp)?);
    if unsafe { libc::renamex_np(r.as_ptr(), t.as_ptr(), libc::RENAME_EXCL) } == 0 {
        return Ok(());
    }
    let err = io::Error::last_os_error();
    // a disk that can't refuse that way (some network and FAT disks)
    match err.raw_os_error() {
        Some(libc::ENOTSUP) | Some(libc::EINVAL) if !target.exists() => std::fs::rename(temp, target),
        _ => Err(err),
    }
}

/// A lock several programs take turns on: an exclusive flock on the file. Held until dropped; after a second of
/// waiting, carries on without it rather than hang.
pub fn lock_file(path: &Path) -> Option<File> {
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let file = OpenOptions::new().read(true).write(true).create(true).truncate(false).open(path).ok()?;
    for _ in 0..100 {
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
            return Some(file);
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    None
}

/// File names here ignore case, as the Mac's own disks do unless made otherwise, and the two ways Unicode can write the
/// same letter (é as one character or as e and its accent: NFC and NFD). The Mac's disks treat them as one name, and a
/// name typed in one program and one read from Finder or an archive can come in either.
pub fn same_name(a: &OsStr, b: &OsStr) -> bool {
    if a.eq_ignore_ascii_case(b) {
        return true;
    }
    // only names with letters beyond ASCII can differ in their Unicode form or their case beyond A to Z
    if a.is_ascii() && b.is_ascii() {
        return false;
    }
    let (Some(a), Some(b)) = (a.to_str(), b.to_str()) else { return false };
    fold(a) == fold(b)
}

/// A name as the Mac's disks compare it: decomposed (NFD), and in lower case.
fn fold(name: &str) -> String {
    use objc2_foundation::NSString;
    NSString::from_str(name).decomposedStringWithCanonicalMapping().to_string().to_lowercase()
}

/// The Mac's `fsync` leaves the bytes in the disk's own cache, where a power cut loses them: `F_FULLFSYNC` asks the disk
/// to put them down for good before the new file takes the old one's name (`atomic`). A disk that can't (some network
/// disks) has had the ordinary `fsync` already.
pub fn full_sync(file: &File) {
    unsafe { libc::fcntl(file.as_raw_fd(), libc::F_FULLFSYNC) };
}

// ---------- per-user folders ----------

fn home() -> PathBuf {
    std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

/// Each tool keeps its settings in its own folder: `~/Library/Application Support/DotAny/<folder>`.
pub fn data_dir(folder: &str) -> PathBuf {
    home().join("Library").join("Application Support").join("DotAny").join(folder)
}

/// One of the user's own folders, by its Windows name ("Documents", "Music", "Pictures", "Videos"), or the home folder
/// if it isn't there. The Mac calls Videos "Movies".
pub fn user_dir(name: &str) -> PathBuf {
    let home = home();
    let dir = home.join(if name == "Videos" { "Movies" } else { name });
    if dir.is_dir() { dir } else { home }
}

// ---------- the window's frame ----------

/// The system's light or dark, as System Settings › Appearance has it now (also when set to Auto).
pub fn system_is_dark() -> bool {
    use objc2_foundation::{NSUserDefaults, ns_string};
    #[allow(unused_unsafe)]
    let style = unsafe { NSUserDefaults::standardUserDefaults().stringForKey(ns_string!("AppleInterfaceStyle")) };
    style.is_some_and(|s| s.to_string().eq_ignore_ascii_case("dark"))
}

/// Nothing to paint: the frame is the system's, and it follows the system's light or dark by itself. Pinning it to the
/// page's choice would also pin the page's `prefers-color-scheme`, and System chosen later would stop following.
pub fn paint_frame(_window: &WebviewWindow, _dark: bool) {}

/// The system's own buttons go on the top bar (`buttons`), and the system's tabs are turned off: with the system
/// setting "Prefer tabs when opening documents" they would stack our windows as tabs and push a tab bar into the top
/// bar. And the page is a window, not a web page: a pinch never magnifies the whole of it, top bar and all (a tool that
/// zooms its own work hears the pinch itself), and a two-finger swipe never goes "back" to a page before it.
pub fn prepare_frame(window: &WebviewWindow) {
    with_ns_window(window, |w| {
        w.setTabbingMode(objc2_app_kit::NSWindowTabbingMode::Disallowed);
        buttons::keep(w);
    });
    full_screen_early(window);
    first_click::watch(window);
    let _ = window.with_webview(|wv| {
        // SAFETY: Tauri hands out the window's own WKWebView, on the main thread
        if let Some(web) = unsafe { (wv.inner() as *const objc2_web_kit::WKWebView).as_ref() } {
            unsafe {
                web.setAllowsMagnification(false);
                web.setAllowsBackForwardNavigationGestures(false);
            }
        }
    });
}

/// The windows on their way into full screen, by address: from `NSWindowWillEnterFullScreenNotification`, when AppKit
/// does not yet mark them full screen, until they are in (`full_screen_early`).
static ENTERING: std::sync::Mutex<Vec<usize>> = std::sync::Mutex::new(Vec::new());

/// Whether the window is in the system's full screen as its page should see it: from the moment it starts going in until
/// the moment it starts coming out. Read from AppKit (its own mark, with the windows on their way in or out), not from
/// Tao's `is_fullscreen`: Tao's is what it last asked AppKit for, set before AppKit has done it, and it said "out" while
/// the window had gone straight back in. On the main thread (the window's events are).
pub fn full_screen(window: &tauri::Window) -> Option<bool> {
    let ptr = window.ns_window().ok()?;
    // SAFETY: Tauri hands out the window's own NSWindow, alive while the window is; read on the main thread
    let w = unsafe { (ptr as *const objc2_app_kit::NSWindow).as_ref() }?;
    Some(page_full(w))
}

fn page_full(w: &objc2_app_kit::NSWindow) -> bool {
    let key = std::ptr::from_ref(w) as usize;
    crate::lock(&ENTERING).contains(&key)
        || (w.styleMask().contains(objc2_app_kit::NSWindowStyleMask::FullScreen)
            && !crate::lock(&buttons::LEAVING).contains(&key))
}

/// The page hears it as the window starts to go into full screen or out of it, not only once the system's animation has
/// ended (the window's size changes, Tauri's `frame-state`, come only after the window is out): a top bar that steps aside only in full screen is back in the window as it comes out. And it hears it again
/// once the window is in or out, whichever way it went (the page's button, ⎋, the green button, the Window menu, ⌃⌘F,
/// Mission Control), so the page ends where the window is.
fn full_screen_early(window: &WebviewWindow) {
    use objc2_app_kit::{
        NSWindowDidEnterFullScreenNotification, NSWindowDidExitFullScreenNotification,
        NSWindowWillEnterFullScreenNotification, NSWindowWillExitFullScreenNotification,
    };
    use objc2_foundation::{NSNotification, NSNotificationCenter};
    let win = window.clone();
    with_ns_window(window, move |w| {
        let key = std::ptr::from_ref(w) as usize;
        for (note, step) in [
            (unsafe { NSWindowWillEnterFullScreenNotification }, 0),
            (unsafe { NSWindowWillExitFullScreenNotification }, 1),
            (unsafe { NSWindowDidExitFullScreenNotification }, 2),
            (unsafe { NSWindowDidEnterFullScreenNotification }, 3),
        ] {
            let page = win.clone();
            let block = block2::RcBlock::new(move |n: std::ptr::NonNull<NSNotification>| {
                // the buttons are held again from the moment the window starts out of full screen (`buttons::held`)
                let mut leaving = crate::lock(&buttons::LEAVING);
                leaving.retain(|k| *k != key);
                if step == 1 {
                    leaving.push(key);
                }
                drop(leaving);
                let mut entering = crate::lock(&ENTERING);
                entering.retain(|k| *k != key);
                if step == 0 {
                    entering.push(key);
                }
                drop(entering);
                // SAFETY: the notification's object is the window it was posted for, on the main thread
                let w = unsafe { n.as_ref() }.object().and_then(|o| o.downcast::<objc2_app_kit::NSWindow>().ok());
                if let (2, Some(w)) = (step, &w) {
                    buttons::place(w);
                }
                if let (2 | 3, Some(w)) = (step, &w) {
                    keys_back(w);
                }
                let full = w.map_or(step == 0 || step == 3, |w| page_full(&w));
                let _ =
                    page.emit_to(page.label(), "frame-state", serde_json::json!({ "maximized": false, "full": full }));
            });
            // SAFETY: the block takes only the notification; the observer lives as long as the program, as the
            // buttons' do
            let token = unsafe {
                NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
                    Some(note),
                    Some(w),
                    None,
                    &block,
                )
            };
            std::mem::forget(token);
        }
    });
}

/// Keys reach the page again once the window is in or out of full screen. Tao, as the window comes out, sets its style
/// back in a block it queues on the main queue and makes its own view the first responder there, so the window kept
/// every key from the page until it was clicked (after ⎋, the page's button and the green button alike). After Tao's block has run (two turns of the main queue), the window's web view takes the keys back, if the
/// window itself or Tao's view has them.
fn keys_back(w: &objc2_app_kit::NSWindow) {
    use objc2::Message;
    use objc2::rc::Retained;
    use objc2_foundation::NSOperationQueue;
    let window = w.retain();
    let first_turn = block2::RcBlock::new(move || {
        let window = window.clone();
        let second_turn = block2::RcBlock::new(move || {
            let content = window.contentView();
            let lost = match window.firstResponder() {
                None => true,
                Some(r) => {
                    let r = Retained::as_ptr(&r).cast::<std::ffi::c_void>();
                    r == Retained::as_ptr(&window).cast()
                        || content.as_ref().is_some_and(|c| r == Retained::as_ptr(c).cast())
                }
            };
            if let (true, Some(web)) = (lost, content.as_deref().and_then(web_view_in)) {
                let web: &objc2_app_kit::NSResponder = &web;
                window.makeFirstResponder(Some(web));
            }
        });
        // SAFETY: the main queue runs the block on the main thread, where this is
        unsafe { NSOperationQueue::mainQueue().addOperationWithBlock(&second_turn) };
    });
    // SAFETY: as above
    unsafe { NSOperationQueue::mainQueue().addOperationWithBlock(&first_turn) };
}

/// The window's WKWebView, among the views in `view`.
fn web_view_in(view: &objc2_app_kit::NSView) -> Option<objc2::rc::Retained<objc2_app_kit::NSView>> {
    use objc2::ClassType;
    use objc2::runtime::NSObjectProtocol;
    view.subviews().iter().find_map(|sub| {
        if sub.isKindOfClass(objc2_web_kit::WKWebView::class()) { Some(sub.clone()) } else { web_view_in(&sub) }
    })
}

/// The top bar is the window's toolbar, and on the Mac a toolbar's buttons in a window behind act on the click that
/// brings the window forward; everything else in the window (the work area, side panels, the dock at the bottom) only
/// comes forward on that click, so nothing in a document changes from a click meant to pick a window.
///
/// AppKit asks the view under a click in a window that is not in front whether the click may act
/// (`acceptsFirstMouse:`), and wry's WebView answers one fixed yes or no for the whole page. Its class's answer is
/// replaced here by one that looks at where the click is: yes over the parts of the top bar the page has reported as
/// showing and on top (`shared/web/window-frame.js`, the `dotany-first-click` event: the bar's rectangle, or nothing while
/// the bar is hidden or covered), never below `TOP_BAR`, no anywhere else. A refused press never reaches the page, and
/// neither does its release (AppKit sends neither). Over the bar's empty part the press goes to Tauri's drag region, which
/// moves the window when the pointer moves and does nothing else, so a background window can be dragged by its bar.
mod first_click {
    use super::TOP_BAR;
    use objc2::runtime::{AnyClass, AnyObject, Bool, Imp, Sel};
    use objc2_app_kit::{NSEvent, NSView};
    use std::sync::{Mutex, Once};
    use tauri::{Listener, Manager, WebviewWindow};

    /// Each window's reported parts: its label, its NSWindow (as an address, only compared), the rectangles in points
    /// from the window's top-left ([x, y, width, height]).
    static AREAS: Mutex<Vec<(String, usize, Vec<[f64; 4]>)>> = Mutex::new(Vec::new());

    pub fn watch(window: &WebviewWindow) {
        static LISTEN: Once = Once::new();
        LISTEN.call_once(|| {
            let app = window.app_handle().clone();
            window.app_handle().listen_any("dotany-first-click", move |event| {
                let Ok(v) = serde_json::from_str::<serde_json::Value>(event.payload()) else { return };
                let Some(label) = v["label"].as_str() else { return };
                let Some(ptr) = app.get_webview_window(label).and_then(|w| w.ns_window().ok()) else { return };
                let rects: Vec<[f64; 4]> = v["rects"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|r| {
                                let n: Vec<f64> = r.as_array()?.iter().filter_map(|x| x.as_f64()).collect();
                                // only ever the top bar's band
                                let (x, y, w, h) = (n.first()?, n.get(1)?.max(0.0), *n.get(2)?, *n.get(3)?);
                                let bottom = (n[1] + h).min(TOP_BAR);
                                (w > 0.0 && bottom > y).then_some([*x, y, w, bottom - y])
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                let mut areas = crate::lock(&AREAS);
                areas.retain(|(l, _, _)| l != label);
                areas.push((label.to_string(), ptr as usize, rects));
            });
        });
        static REPLACE: Once = Once::new();
        let _ = window.with_webview(|wv| {
            REPLACE.call_once(|| {
                // SAFETY: the WebView is alive in this call; its class is wry's WKWebView subclass, which has its own
                // acceptsFirstMouse:, whose implementation is swapped for one of the same signature
                unsafe {
                    let class: *const AnyClass = objc2::ffi::object_getClass(wv.inner() as *const AnyObject);
                    let Some(class) = class.as_ref() else { return };
                    if let Some(method) = class.instance_method(objc2::sel!(acceptsFirstMouse:)) {
                        let imp: unsafe extern "C-unwind" fn(&AnyObject, Sel, *mut NSEvent) -> Bool =
                            accepts_first_mouse;
                        method.set_implementation(std::mem::transmute::<_, Imp>(imp));
                    }
                }
            });
        });
    }

    pub fn forget(label: &str) {
        crate::lock(&AREAS).retain(|(l, _, _)| l != label);
    }

    unsafe extern "C-unwind" fn accepts_first_mouse(this: &AnyObject, _sel: Sel, event: *mut NSEvent) -> Bool {
        // SAFETY: AppKit calls this on the WebView (an NSView) with the press it asks about, on the main thread
        let (view, Some(event)) = (unsafe { &*(this as *const AnyObject as *const NSView) }, unsafe { event.as_ref() })
        else {
            return Bool::NO;
        };
        let Some(window) = view.window() else { return Bool::NO };
        let p = event.locationInWindow();
        let height = window.contentView().map(|v| v.frame().size.height).unwrap_or(0.0);
        let (x, y) = (p.x, height - p.y);
        let me = objc2::rc::Retained::as_ptr(&window) as usize;
        let yes = y >= 0.0
            && y < TOP_BAR
            && crate::lock(&AREAS).iter().any(|(_, w, rects)| {
                *w == me && rects.iter().any(|r| x >= r[0] && x < r[0] + r[2] && y >= r[1] && y < r[1] + r[3])
            });
        Bool::new(yes)
    }
}

/// The family's top bar's height in points: `.head`'s `height` in `shared/web/family.css`. The page draws the bar and
/// Rust places the system's buttons on its middle line, so the two must say the same number (the tests read both and fail when they differ).
pub const TOP_BAR: f64 = 52.0;
/// The top bar's own left margin (`.head`'s `max(20px, …)`): where the red button's left edge starts.
pub const TOP_BAR_LEFT: f64 = 20.0;

/// The file a window shows, for the system: ⌘-click (or Control-click) on its title in Mission Control and the Window
/// menu shows the folders it is in, and the Dock knows the window by it. None when the window has no file.
pub fn set_represented(window: &WebviewWindow, path: Option<String>) {
    with_ns_window(window, move |w| {
        let url = path
            .filter(|p| !p.is_empty() && Path::new(p).is_absolute())
            .map(|p| objc2_foundation::NSURL::fileURLWithPath(&objc2_foundation::NSString::from_str(&p)));
        w.setRepresentedURL(url.as_deref());
    });
}

/// What a double-click on a window's title bar does, as System Settings › Desktop & Dock › "Double-click a window's
/// title bar to" is set (`AppleActionOnDoubleClick`, read each time, as AppKit reads it for its own title bars).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TitleAction {
    Zoom,
    Minimize,
    Fill,
    Nothing,
}

/// The setting's word (`Maximize`, `Minimize`, `Fill`, `None`) as an action; `miniaturize` is the older switch
/// (`AppleMiniaturizeOnDoubleClick`), which a Mac set before the four choices still has. With neither, the Mac's own
/// default, `Maximize` (Zoom).
pub fn title_action(setting: Option<&str>, miniaturize: bool) -> TitleAction {
    match setting {
        Some("Minimize") => TitleAction::Minimize,
        Some("None") => TitleAction::Nothing,
        Some("Fill") => TitleAction::Fill,
        Some("Maximize") => TitleAction::Zoom,
        _ if miniaturize => TitleAction::Minimize,
        _ => TitleAction::Zoom,
    }
}

/// A double-click on the top bar's empty parts: what the system setting says, each time.
pub fn title_double_click(window: &WebviewWindow) {
    with_ns_window(window, |w| {
        use objc2_foundation::{NSUserDefaults, ns_string};
        let defaults = NSUserDefaults::standardUserDefaults();
        let setting = defaults.stringForKey(ns_string!("AppleActionOnDoubleClick")).map(|s| s.to_string());
        let miniaturize = defaults.boolForKey(ns_string!("AppleMiniaturizeOnDoubleClick"));
        match title_action(setting.as_deref(), miniaturize) {
            TitleAction::Zoom => w.performZoom(None),
            TitleAction::Minimize => w.performMiniaturize(None),
            TitleAction::Nothing => {}
            TitleAction::Fill => fill(w),
        }
    });
}

/// Fill: the window takes the whole of the screen's room for windows, as the Window menu's Fill does. That item is the
/// system's own (AppKit adds it to the window menu); it is chosen if it is there, so the window goes exactly where the
/// system's Fill puts it, tiling margins and all.
fn fill(w: &objc2_app_kit::NSWindow) {
    let Some(mtm) = objc2::MainThreadMarker::new() else { return };
    let app = objc2_app_kit::NSApplication::sharedApplication(mtm);
    if let Some(menu) = app.windowsMenu() {
        for item in menu.itemArray().iter() {
            let Some(action) = item.action() else { continue };
            if FILL_ACTIONS.contains(&action.name().to_str().unwrap_or_default()) {
                w.makeKeyWindow();
                unsafe { app.sendAction_to_from(action, None, None) };
                return;
            }
        }
    }
    if let Some(screen) = w.screen() {
        w.setFrame_display_animate(screen.visibleFrame(), true, true);
    }
}

/// The Window menu's Fill, by its action, as AppKit names it (macOS 15 and later).
const FILL_ACTIONS: [&str; 2] = ["_zoomFill:", "zoomFill:"];

/// Runs `f` with the window's NSWindow, on the main thread (where AppKit wants it).
fn with_ns_window(window: &WebviewWindow, f: impl FnOnce(&objc2_app_kit::NSWindow) + Send + 'static) {
    let w = window.clone();
    let _ = window.run_on_main_thread(move || {
        if let Ok(ptr) = w.ns_window() {
            // SAFETY: Tauri hands out the window's own NSWindow, alive while the window is
            if let Some(ns) = unsafe { (ptr as *const objc2_app_kit::NSWindow).as_ref() } {
                f(ns);
            }
        }
    });
}

/// Unsaved work is a dot in the red close button, as on every Mac (the top bar's dot stays too).
pub fn set_edited(window: &WebviewWindow, edited: bool) {
    with_ns_window(window, move |w| w.setDocumentEdited(edited));
}

/// A window brought forward, as when its file is opened again from Finder: out of the Dock if it was there, in front,
/// taking the keys, the program in front too. (Tauri's own leaves a window in the Dock where it is.)
pub fn bring_forward(window: &WebviewWindow) {
    with_ns_window(window, |w| {
        if w.isMiniaturized() {
            w.deminiaturize(None);
        }
        w.makeKeyAndOrderFront(None);
        if let Some(mtm) = objc2::MainThreadMarker::new() {
            #[allow(deprecated)]
            objc2_app_kit::NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
        }
    });
}

/// Windows' snap layouts have no counterpart: the green button is the system's own.
pub fn set_snap_button(_window: &WebviewWindow, _x: i32, _y: i32, _w: i32, _h: i32) {}

/// The Mac has no window menu on the title bar.
pub fn show_system_menu(_window: &WebviewWindow, _x: i32, _y: i32) {}

/// The top bar is the only bar, as on Windows, but the system keeps its frame: its title bar lies under the page and
/// the title is hidden. The three buttons are put on the top bar by `buttons`, not by tao's `traffic_light_position`.
pub fn frame<'a, R: Runtime, M: Manager<R>>(builder: WebviewWindowBuilder<'a, R, M>) -> WebviewWindowBuilder<'a, R, M> {
    builder.title_bar_style(TitleBarStyle::Overlay).hidden_title(true)
}

/// The system's red, yellow and green buttons on the top bar: the red one's left edge at the bar's margin
/// (`TOP_BAR_LEFT`), the system's own spacing between the three, and their middle on the bar's middle line
/// (`TOP_BAR / 2`). Their size and spacing are read from the buttons AppKit made (14pt, 23pt apart on macOS 27), never
/// written down here.
///
/// The title bar's container is made as high as the top bar and the buttons are set where they go, in the window's own
/// coordinates. AppKit lays the title bar out again whenever it likes (a resize, a new title, the unsaved dot, the window
/// drawn dark or light, leaving full screen) and puts the buttons back at its own place, (9, 9) from the top-left on
/// macOS 27; each time it moves a button or the container they are put back at once (`NSViewFrameDidChangeNotification`).
/// Putting them back moves them, and AppKit answers some of those moves by laying them out again in the same call: that
/// answer is not answered again (`PLACING`), or the two would go on until the stack ran out.
///
/// tao's `traffic_light_position` does not hold: it moves the buttons only when its own
/// view redraws, which the WebView over it seldom lets happen (after a new title they were back at AppKit's (9, 9)), and
/// its `y` is not a distance from the top (y = 18 puts their middle at 16, y = 38 at 36).
///
/// On the Mac a top bar does not step aside in a window: the system's buttons are not part of the family's
/// bar, so a bar that slid away could not take them along. They are always on the bar.
pub mod buttons {
    use super::{TOP_BAR, TOP_BAR_LEFT};
    use objc2::Message;
    use objc2::rc::Retained;
    use objc2_app_kit::{
        NSButton, NSView, NSViewFrameDidChangeNotification, NSWindow, NSWindowButton, NSWindowStyleMask,
    };
    use objc2_foundation::{NSNotification, NSNotificationCenter, NSOperationQueue, NSPoint, NSRect, NSSize};
    use std::ptr::NonNull;
    use std::sync::Mutex;

    fn three(w: &NSWindow) -> Option<[Retained<NSButton>; 3]> {
        Some([
            w.standardWindowButton(NSWindowButton::CloseButton)?,
            w.standardWindowButton(NSWindowButton::MiniaturizeButton)?,
            w.standardWindowButton(NSWindowButton::ZoomButton)?,
        ])
    }

    /// The view the buttons are in, and the title bar's container around it.
    fn holders(button: &NSButton) -> Option<(Retained<NSView>, Retained<NSView>)> {
        // SAFETY: views AppKit made for the window, on the main thread
        let bar = unsafe { button.superview() }?;
        let container = unsafe { bar.superview() }?;
        Some((bar, container))
    }

    thread_local! {
        /// Moving the container posts its own notification: that one is not answered again.
        static PLACING: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
        /// How many times in a row placing has been tried again after AppKit undid it.
        static AGAIN: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
    }

    /// Puts the buttons on the bar; nothing moves when they are there already.
    pub fn place(w: &NSWindow) {
        // full screen shows them in the system's own strip that slides down: they are the system's there
        if w.styleMask().contains(NSWindowStyleMask::FullScreen) || PLACING.get() {
            return;
        }
        let Some(buttons) = three(w) else { return };
        let Some((bar, container)) = holders(&buttons[0]) else { return };
        // each button keeps its distance from the window's top whatever happens to the title bar's height below it
        // (AppKit sets its own height for a frame now and then while it animates a resize: the buttons rode up with it)
        for b in &buttons {
            let pinned = objc2_app_kit::NSAutoresizingMaskOptions::ViewMinYMargin
                | objc2_app_kit::NSAutoresizingMaskOptions::ViewMaxXMargin;
            if b.autoresizingMask() != pinned {
                b.setAutoresizingMask(pinned);
            }
        }
        let height = w.frame().size.height;
        // the system's spacing, as AppKit lays them out next to each other: read before they are first moved (after that
        // AppKit may have put back some and not others)
        let spacing = *SPACING.get_or_init(|| buttons[1].frame().origin.x - buttons[0].frame().origin.x);
        let size = buttons[0].frame().size;
        // where each goes, in the window (AppKit counts from the bottom); the title bar's container must reach down to
        // the bar's bottom so they are inside it
        let want = |i: usize| {
            NSRect::new(NSPoint::new(TOP_BAR_LEFT + i as f64 * spacing, height - (TOP_BAR + size.height) / 2.0), size)
        };
        let c = container.frame();
        let off = |b: &NSButton, i: usize| {
            let now = b.convertRect_toView(b.bounds(), None);
            (now.origin.x - want(i).origin.x).abs() > 0.01 || (now.origin.y - want(i).origin.y).abs() > 0.01
        };
        let short = (c.size.height - TOP_BAR).abs() > 0.01 || (c.origin.y - (height - TOP_BAR)).abs() > 0.01;
        if !short && !buttons.iter().enumerate().any(|(i, b)| off(b, i)) {
            return;
        }
        PLACING.set(true);
        if short {
            container
                .setFrame(NSRect::new(NSPoint::new(c.origin.x, height - TOP_BAR), NSSize::new(c.size.width, TOP_BAR)));
        }
        for (i, b) in buttons.iter().enumerate() {
            b.setFrameOrigin(bar.convertRect_fromView(want(i), None).origin);
        }
        PLACING.set(false);
        MOVES.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        // AppKit put them back while they were being placed: once more, after it has finished (a few times at most, so
        // the two never chase each other)
        if buttons.iter().enumerate().any(|(i, b)| off(b, i)) && AGAIN.get() < 3 {
            AGAIN.set(AGAIN.get() + 1);
            let window = w.retain();
            let later = block2::RcBlock::new(move || place(&window));
            // SAFETY: the main queue runs the block on the main thread, where this is
            unsafe { NSOperationQueue::mainQueue().addOperationWithBlock(&later) };
        } else {
            AGAIN.set(0);
        }
    }

    /// The distance from one button's left edge to the next one's, as AppKit lays them out (23pt on macOS 27).
    pub static SPACING: std::sync::OnceLock<f64> = std::sync::OnceLock::new();

    /// How many times the buttons have been put back (the test driver reports it: a count that keeps growing while
    /// nothing happens would be AppKit and this answering each other without end).
    pub static MOVES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

    /// Where each of a window's own views the base holds belongs, in its superview, whatever AppKit asks for: the three
    /// buttons on the bar, the title bar's container as tall as the bar. None for any other view, a window in full
    /// screen, or a window that is not this program's.
    fn held(view: &NSView, asked: NSRect) -> Option<NSRect> {
        if PLACING.get() {
            return None;
        }
        let w = view.window()?;
        let key = Retained::as_ptr(&w) as usize;
        // (on its way out of full screen the window gets its title bar back while it still says full screen: held from then)
        let full = w.styleMask().contains(NSWindowStyleMask::FullScreen) && !crate::lock(&LEAVING).contains(&key);
        if full || !crate::lock(&KEPT).contains(&key) {
            return None;
        }
        let buttons = three(&w)?;
        let height = w.frame().size.height;
        let me = std::ptr::from_ref(view) as usize;
        if let Some(i) = buttons.iter().position(|b| Retained::as_ptr(b) as usize == me) {
            let spacing = *SPACING.get()?;
            let size = buttons[i].frame().size;
            let y = height - (TOP_BAR + size.height) / 2.0;
            let want = NSRect::new(NSPoint::new(TOP_BAR_LEFT + i as f64 * spacing, y), size);
            // SAFETY: the button's own superview, on the main thread
            let sup = unsafe { view.superview() }?;
            let r = sup.convertRect_fromView(want, None);
            return Some(NSRect::new(r.origin, asked.size));
        }
        let (bar, container) = holders(&buttons[0])?;
        // the view the buttons are in fills the container, whatever height AppKit lays it out at for a moment
        if Retained::as_ptr(&bar) as usize == me {
            let c = container.frame();
            return Some(NSRect::new(NSPoint::new(asked.origin.x, 0.0), NSSize::new(asked.size.width, c.size.height)));
        }
        if Retained::as_ptr(&container) as usize == me {
            return Some(NSRect::new(
                NSPoint::new(asked.origin.x, height - TOP_BAR),
                NSSize::new(asked.size.width, TOP_BAR),
            ));
        }
        None
    }

    /// How far, in points, the buttons are from where they belong now (0 when they are there): what a person would see
    /// out of place (the title bar's container around them is clear). None in full screen, where they are the system's.
    /// For the test driver's sampling.
    pub fn deviation(w: &NSWindow) -> Option<f64> {
        if w.styleMask().contains(NSWindowStyleMask::FullScreen) {
            return None;
        }
        let buttons = three(w)?;
        let height = w.frame().size.height;
        let spacing = *SPACING.get()?;
        let mut worst = 0f64;
        for (i, b) in buttons.iter().enumerate() {
            let r = b.convertRect_toView(b.bounds(), None);
            let (x, y) = (TOP_BAR_LEFT + i as f64 * spacing, height - (TOP_BAR + r.size.height) / 2.0);
            worst = worst.max((r.origin.x - x).abs()).max((r.origin.y - y).abs());
        }
        Some(worst)
    }

    /// The windows on their way out of full screen, by address (`super::full_screen_early`).
    pub static LEAVING: Mutex<Vec<usize>> = Mutex::new(Vec::new());

    /// The windows whose buttons are held (`held`), by address.
    static KEPT: Mutex<Vec<usize>> = Mutex::new(Vec::new());
    /// AppKit's own implementations of what is held, by class and selector name.
    static APPKIT_FRAME: Mutex<Vec<(usize, &'static str, usize)>> = Mutex::new(Vec::new());

    fn appkit_frame(this: &NSView, sel: &'static str) -> Option<usize> {
        let mut class: *const objc2::runtime::AnyClass = objc2::runtime::AnyObject::class(this);
        let list = crate::lock(&APPKIT_FRAME);
        // the nearest class up the chain that was held
        while let Some(c) = unsafe { class.as_ref() } {
            if let Some(&(_, _, imp)) = list.iter().find(|(k, s, _)| *k == class as usize && *s == sel) {
                return Some(imp);
            }
            class = c.superclass().map_or(std::ptr::null(), |s| s as *const _);
        }
        None
    }

    unsafe extern "C-unwind" fn set_frame(this: &NSView, sel: objc2::runtime::Sel, asked: NSRect) {
        let r = held(this, asked).unwrap_or(asked);
        let Some(imp) = appkit_frame(this, "setFrame:") else { return };
        // SAFETY: AppKit's own setFrame: for this class
        unsafe {
            let f: unsafe extern "C-unwind" fn(&NSView, objc2::runtime::Sel, NSRect) = std::mem::transmute(imp);
            f(this, sel, r)
        }
    }

    unsafe extern "C-unwind" fn set_frame_origin(this: &NSView, sel: objc2::runtime::Sel, asked: NSPoint) {
        let p = held(this, NSRect::new(asked, this.frame().size)).map_or(asked, |r| r.origin);
        let Some(imp) = appkit_frame(this, "setFrameOrigin:") else { return };
        // SAFETY: AppKit's own setFrameOrigin: for this class
        unsafe {
            let f: unsafe extern "C-unwind" fn(&NSView, objc2::runtime::Sel, NSPoint) = std::mem::transmute(imp);
            f(this, sel, p)
        }
    }

    unsafe extern "C-unwind" fn set_frame_size(this: &NSView, sel: objc2::runtime::Sel, asked: NSSize) {
        let s = held(this, NSRect::new(this.frame().origin, asked)).map_or(asked, |r| r.size);
        let Some(imp) = appkit_frame(this, "setFrameSize:") else { return };
        // SAFETY: AppKit's own setFrameSize: for this class
        unsafe {
            let f: unsafe extern "C-unwind" fn(&NSView, objc2::runtime::Sel, NSSize) = std::mem::transmute(imp);
            f(this, sel, s)
        }
    }

    unsafe extern "C-unwind" fn set_frame_origin_once(
        this: &NSView,
        sel: objc2::runtime::Sel,
        asked: NSPoint,
        again: objc2::runtime::Bool,
    ) {
        let p = held(this, NSRect::new(asked, this.frame().size)).map_or(asked, |r| r.origin);
        let Some(imp) = appkit_frame(this, "setFrameOrigin:ignoreRentry:") else { return };
        // SAFETY: AppKit's own setFrameOrigin:ignoreRentry: for this class
        unsafe {
            let f: unsafe extern "C-unwind" fn(&NSView, objc2::runtime::Sel, NSPoint, objc2::runtime::Bool) =
                std::mem::transmute(imp);
            f(this, sel, p, again)
        }
    }

    /// AppKit lays the title bar out again in its own passes (a zoom, a live resize, a new title), and a button it has
    /// moved stays where it put it until it is put back: a frame or more at AppKit's place shows (the green button
    /// jumping up during a double-click's zoom). So the move itself is held, not corrected after
    /// it: each view class the buttons and the container are made of answers `setFrame:` and `setFrameOrigin:` with
    /// where the view belongs (`held`) before AppKit's own sets it. Views of other windows, and of this program's in full screen,
    /// pass through untouched. The theme frame's own pass that puts the buttons at the system's place
    /// (`_tileTitlebarAndRedisplay:` → `_updateButtonPositions`, after a new title and whenever the theme frame changes
    /// size, among them as the window comes out of full screen) moves each button with the button's
    /// `setFrameOrigin:ignoreRentry:`, which hands the point to NSView's `setFrameOrigin:` past the button's own: that one
    /// is held too. Without it the buttons went to the system's place and the frame-change
    /// notice put them back after it, sometimes only on a later turn of the main queue (`place`), and on leaving full
    /// screen a display frame could fall in between.
    fn hold_frames(w: &NSWindow, views: &[&NSView]) {
        crate::lock(&KEPT).push(std::ptr::from_ref(w) as usize);
        for v in views {
            // SAFETY: the view's own class (not KVO's stand-in); methods of AppKit's own signatures added to it or put in
            // place of its own, AppKit's kept to be called on
            unsafe {
                let class: *const objc2::runtime::AnyClass = objc2::msg_send![*v, class];
                let Some(cls) = class.as_ref() else { continue };
                let frame: unsafe extern "C-unwind" fn(&NSView, objc2::runtime::Sel, NSRect) = set_frame;
                let origin: unsafe extern "C-unwind" fn(&NSView, objc2::runtime::Sel, NSPoint) = set_frame_origin;
                let size: unsafe extern "C-unwind" fn(&NSView, objc2::runtime::Sel, NSSize) = set_frame_size;
                let once: unsafe extern "C-unwind" fn(&NSView, objc2::runtime::Sel, NSPoint, objc2::runtime::Bool) =
                    set_frame_origin_once;
                for (name, sel, imp) in [
                    ("setFrame:", objc2::sel!(setFrame:), std::mem::transmute::<_, objc2::runtime::Imp>(frame)),
                    (
                        "setFrameOrigin:",
                        objc2::sel!(setFrameOrigin:),
                        std::mem::transmute::<_, objc2::runtime::Imp>(origin),
                    ),
                    ("setFrameSize:", objc2::sel!(setFrameSize:), std::mem::transmute::<_, objc2::runtime::Imp>(size)),
                    (
                        "setFrameOrigin:ignoreRentry:",
                        objc2::sel!(setFrameOrigin:ignoreRentry:),
                        std::mem::transmute::<_, objc2::runtime::Imp>(once),
                    ),
                ] {
                    let mut list = crate::lock(&APPKIT_FRAME);
                    if list.iter().any(|(k, s, _)| *k == class as usize && *s == name) {
                        continue;
                    }
                    let Some(m) = cls.instance_method(sel) else { continue };
                    let theirs = m.implementation();
                    if theirs as usize == imp as usize {
                        continue;
                    }
                    let types = objc2::ffi::method_getTypeEncoding(m);
                    if objc2::ffi::class_addMethod(class as *mut _, sel, imp, types).as_bool() {
                        // added to this class: AppKit's is the one it had from its superclass
                        list.push((class as usize, name, theirs as usize));
                    } else if let Some(own) = cls.instance_method(sel) {
                        let old = own.set_implementation(imp);
                        list.push((class as usize, name, old as usize));
                    }
                }
            }
        }
    }

    /// Places the window's buttons now, and again each time AppKit moves one of them or the title bar.
    pub fn keep(w: &NSWindow) {
        place(w);
        if let Some(buttons) = three(w) {
            if let Some((bar, container)) = holders(&buttons[0]) {
                hold_frames(w, &[&buttons[0], &buttons[1], &buttons[2], &bar, &container]);
            }
        }
        let Some(buttons) = three(w) else { return };
        let containers: Vec<Retained<NSView>> = buttons.iter().filter_map(|b| holders(b)).map(|(_, c)| c).collect();
        let block = block2::RcBlock::new(|note: NonNull<NSNotification>| {
            // SAFETY: the notification AppKit posts for one of the views watched below, on the main thread
            let view = unsafe { note.as_ref() }.object().and_then(|o| o.downcast::<NSView>().ok());
            if let Some(w) = view.and_then(|v| v.window()) {
                place(&w);
            }
        });
        let mut views: Vec<&NSView> = buttons.iter().map(|b| -> &NSView { b }).collect();
        for c in &containers {
            if !views.iter().any(|v| std::ptr::eq(*v, &**c)) {
                views.push(c);
            }
        }
        for v in views {
            v.setPostsFrameChangedNotifications(true);
            // SAFETY: the block takes only the notification. The observer is kept for the program's life: a window's
            // views go with the window, and nothing is posted for them after that
            let token = unsafe {
                NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
                    Some(NSViewFrameDidChangeNotification),
                    Some(v),
                    None,
                    &block,
                )
            };
            std::mem::forget(token);
        }
    }
}

// ---------- the menu bar ----------

/// The menu bar's items: id, the Chinese the page looks up in the family's tables (`shared/web/lang`), and the English it
/// says until the page has sent its words. `{name}` is the program's name. The page sends every item in its language when
/// it starts and whenever the language changes (`set_menu_words`, from `shared/web/window-frame.js`, which lists the same
/// Chinese); Rust never translates.
const WORDS: &[(&str, &str, &str)] = &[
    ("about", "关于 {name}::菜单栏", "About {name}"),
    ("settings", "设置…::菜单栏", "Settings…"),
    ("services", "服务::菜单栏", "Services"),
    ("hide", "隐藏 {name}::菜单栏", "Hide {name}"),
    ("hide_others", "隐藏其他::菜单栏", "Hide Others"),
    ("show_all", "全部显示::菜单栏", "Show All"),
    ("quit", "退出 {name}::菜单栏", "Quit {name}"),
    ("edit", "编辑::菜单栏", "Edit"),
    ("undo", "撤销::菜单栏", "Undo"),
    ("redo", "重做::菜单栏", "Redo"),
    ("cut", "剪切::菜单栏", "Cut"),
    ("copy", "拷贝::菜单栏", "Copy"),
    ("paste", "粘贴::菜单栏", "Paste"),
    ("select_all", "全选::菜单栏", "Select All"),
    ("window", "窗口::菜单栏", "Window"),
    ("minimize", "最小化::菜单栏", "Minimize"),
    ("zoom", "缩放::菜单栏", "Zoom"),
    ("fullscreen", "进入全屏幕::菜单栏", "Enter Full Screen"),
    ("unfullscreen", "退出全屏幕::菜单栏", "Exit Full Screen"),
    ("bring_all", "前置全部窗口::菜单栏", "Bring All to Front"),
    ("close", "关闭::菜单栏", "Close"),
    // the File (文件) menu (`file_menu`)
    ("file", "文件::菜单栏", "File"),
    ("new", "新建::菜单栏", "New"),
    ("open", "打开…::菜单栏", "Open…"),
    ("open_recent", "打开最近使用::菜单栏", "Open Recent"),
    ("clear_recent", "清除菜单::菜单栏", "Clear Menu"),
];

/// The system's first language is Chinese: until the page speaks, the menu bar says the Chinese, else the English.
fn system_is_chinese() -> bool {
    use objc2_foundation::NSLocale;
    #[allow(unused_unsafe)]
    let first = unsafe { NSLocale::preferredLanguages().firstObject() }.map(|l| l.to_string()).unwrap_or_default();
    first.to_ascii_lowercase().starts_with("zh")
}

type SetText = Box<dyn Fn(&str) + Send + Sync>;

/// Every item of the menu bar that has words, by id, so the page's words can reach each one.
#[derive(Default)]
struct MenuParts {
    items: Vec<(&'static str, SetText)>,
    /// The Window menu, to become the system's window menu once the bar is in place.
    windows: Option<Submenu<Wry>>,
    /// The File menu, for a program that opens documents (`file_menu`), and its New, put in once a page says its tool
    /// makes new files (`set_menu_words`).
    file: Option<FileMenu>,
    /// Enter Full Screen ⌃⌘F, which reads Exit Full Screen while the window in front is in full screen
    /// (`refresh_menu`), with the page's two words for it.
    full: Option<(MenuItem<Wry>, std::sync::Mutex<(String, String)>)>,
}

struct FileMenu {
    menu: Submenu<Wry>,
    new: MenuItem<Wry>,
    has_new: std::sync::atomic::AtomicBool,
    /// Open Recent: the system's recent documents, as items of ours (`refresh_recent`), a gap, then Clear Menu.
    recent: Submenu<Wry>,
    clear: MenuItem<Wry>,
    gap: PredefinedMenuItem<Wry>,
    shown: std::sync::Mutex<Vec<(MenuItem<Wry>, PathBuf)>>,
}

fn word(zh: bool, id: &str, name: &str) -> String {
    let (_, z, e) = WORDS.iter().find(|(i, ..)| *i == id).expect("a menu word");
    // the Chinese key's part after :: tells the translator where it is, and is never shown
    let z = z.split("::").next().unwrap_or(z);
    (if zh { z } else { e }).replace("{name}", name)
}

/// Whether Finder hands this program kinds of documents: its Info.plist lists them (`CFBundleDocumentTypes`, written by
/// `shared/scripts/mac-plist.mjs`; a debug build carries the same Info.plist inside it). Every tool's does but
/// Recorder's (the WAVs it opens are Audio's in Finder). The system keeps recent documents only of kinds listed there, so
/// a program without them shows the family's own recent list in Open Recent instead (`refresh_recent`). Whether a
/// program has the File menu at all is its `Tool::opens_files`.
fn has_document_types() -> bool {
    use objc2_foundation::{NSBundle, ns_string};
    NSBundle::mainBundle().objectForInfoDictionaryKey(ns_string!("CFBundleDocumentTypes")).is_some()
}

/// The app's menu, the File menu, the edit menu and the window menu: the least a Mac program that opens documents has
/// everything else is in the window's own menu. About and Settings open what
/// the top bar's Settings opens (in a window opened for it if none is); Quit closes each window the way its red button
/// does, so unsaved work is asked about first; Undo and Redo go to the page in front, which undoes in a text field or
/// in the tool's own work. The rest are the system's own items.
pub fn menu(
    builder: tauri::Builder<Wry>,
    chosen: impl Fn(&AppHandle) -> Option<String> + Send + 'static,
) -> tauri::Builder<Wry> {
    builder
        .menu(move |app| {
            let zh = match chosen(app) {
                Some(l) if !l.is_empty() => l.starts_with("zh"),
                _ => system_is_chinese(),
            };
            let name = app.package_info().name.clone();
            let w = |id: &str| word(zh, id, &name);
            let mut parts = MenuParts::default();
            let mut own = |id: &'static str, key: Option<&str>| -> tauri::Result<MenuItem<Wry>> {
                let item = MenuItem::with_id(app, id, w(id), true, key)?;
                let i = item.clone();
                parts.items.push((
                    id,
                    Box::new(move |t: &str| {
                        let _ = i.set_text(t);
                    }),
                ));
                Ok(item)
            };
            let about = own("about", None)?;
            let settings = own("settings", Some("Cmd+,"))?;
            let quit = own("quit", Some("Cmd+Q"))?;
            let undo = own("undo", Some("Cmd+Z"))?;
            let redo = own("redo", Some("Shift+Cmd+Z"))?;
            let file_items = if app.state::<crate::Base>().tool.opens_files {
                Some((own("new", Some("Cmd+N"))?, own("open", Some("Cmd+O"))?, own("clear_recent", None)?))
            } else {
                None
            };
            type Make = fn(&AppHandle, Option<&str>) -> tauri::Result<PredefinedMenuItem<Wry>>;
            let mut system = |id: &'static str, make: Make| -> tauri::Result<PredefinedMenuItem<Wry>> {
                let item = make(app, Some(&w(id)))?;
                let i = item.clone();
                parts.items.push((
                    id,
                    Box::new(move |t: &str| {
                        let _ = i.set_text(t);
                    }),
                ));
                Ok(item)
            };
            let services = system("services", PredefinedMenuItem::services)?;
            let hide = system("hide", PredefinedMenuItem::hide)?;
            let hide_others = system("hide_others", PredefinedMenuItem::hide_others)?;
            let show_all = system("show_all", PredefinedMenuItem::show_all)?;
            let cut = system("cut", PredefinedMenuItem::cut)?;
            let copy = system("copy", PredefinedMenuItem::copy)?;
            let paste = system("paste", PredefinedMenuItem::paste)?;
            let select_all = system("select_all", PredefinedMenuItem::select_all)?;
            let minimize = system("minimize", PredefinedMenuItem::minimize)?;
            let zoom = system("zoom", PredefinedMenuItem::maximize)?;
            let bring_all = system("bring_all", PredefinedMenuItem::bring_all_to_front)?;
            // ours, not AppKit's toggleFullScreen: item (whose words would be the system's language, not the page's, and
            // beside which AppKit adds a second, hidden one)
            let fullscreen = MenuItem::with_id(app, "fullscreen", w("fullscreen"), false, Some("Ctrl+Cmd+F"))?;
            let close = system("close", PredefinedMenuItem::close_window)?;
            let gap = || PredefinedMenuItem::separator(app);
            let (g1, g2, g3, g4, g5, g6, g7) = (gap()?, gap()?, gap()?, gap()?, gap()?, gap()?, gap()?);
            let own_items: [&dyn IsMenuItem<Wry>; 11] =
                [&about, &g1, &settings, &g2, &services, &g3, &hide, &hide_others, &show_all, &g4, &quit];
            let edit_items: [&dyn IsMenuItem<Wry>; 7] = [&undo, &redo, &g5, &cut, &copy, &paste, &select_all];
            // the app's menu is named by the system, after the program
            let own_menu = Submenu::with_items(app, &name, true, &own_items)?;
            let edit = Submenu::with_items(app, w("edit"), true, &edit_items)?;
            // Close is in the File menu where there is one, as Apple puts it; else the window menu keeps it
            let file = match file_items {
                Some((new, open, clear)) => Some(file_menu(app, &w, &mut parts, new, open, clear, &close)?),
                None => None,
            };
            let window = match file {
                Some(_) => {
                    Submenu::with_items(app, w("window"), true, &[&minimize, &zoom, &fullscreen, &g6, &bring_all])?
                }
                None => Submenu::with_items(
                    app,
                    w("window"),
                    true,
                    &[&minimize, &zoom, &fullscreen, &g6, &bring_all, &g7, &close],
                )?,
            };
            parts.full = Some((fullscreen, std::sync::Mutex::new((w("fullscreen"), w("unfullscreen")))));
            // the system's window menu (it adds the open windows by itself): only once the bar is the program's menu bar, as
            // the program starts (`menu_ready`); said here, it would find no menu bar yet
            parts.windows = Some(window.clone());
            for (id, sub) in [("edit", edit.clone()), ("window", window.clone())] {
                parts.items.push((
                    id,
                    Box::new(move |t: &str| {
                        let _ = sub.set_text(t);
                    }),
                ));
            }
            let bar = match &file {
                Some(f) => Menu::with_items(app, &[&own_menu, &f.menu, &edit, &window])?,
                None => Menu::with_items(app, &[&own_menu, &edit, &window])?,
            };
            parts.file = file;
            app.manage(parts);
            Ok(bar)
        })
        .on_menu_event(menu_chosen)
        // the page of a window ended (WebKit's web process went): the window comes back with its file (`watch_webview`)
        .on_web_content_process_terminate(|webview| page_gone(webview.label()))
}

/// The File menu every Mac document program has, between the program's menu and Edit (TextEdit's and Preview's first
/// items): New ⌘N (only for a tool that makes new files: put in when its page says so), Open… ⌘O, Open Recent with the
/// system's own list of this program's recent documents and its Clear Menu, then Close ⌘W.
fn file_menu(
    app: &AppHandle,
    w: &dyn Fn(&str) -> String,
    parts: &mut MenuParts,
    new: MenuItem<Wry>,
    open: MenuItem<Wry>,
    clear: MenuItem<Wry>,
    close: &PredefinedMenuItem<Wry>,
) -> tauri::Result<FileMenu> {
    // the system fills this one with the recent documents and keeps Clear Menu at its end (`menu_ready`)
    let recent = Submenu::with_id_and_items(app, "open_recent", w("open_recent"), true, &[&clear])?;
    let gap = PredefinedMenuItem::separator(app)?;
    let menu = Submenu::with_items(app, w("file"), true, &[&open, &recent, &gap, close])?;
    for (id, sub) in [("file", menu.clone()), ("open_recent", recent.clone())] {
        parts.items.push((
            id,
            Box::new(move |t: &str| {
                let _ = sub.set_text(t);
            }),
        ));
    }
    Ok(FileMenu {
        menu,
        new,
        has_new: std::sync::atomic::AtomicBool::new(false),
        recent,
        clear,
        gap: PredefinedMenuItem::separator(app)?,
        shown: Default::default(),
    })
}

/// A Mac program stays open when its last window closes; only Quit (⌘Q) ends it.
pub const STAYS_WITHOUT_WINDOWS: bool = true;
static QUIT: std::sync::Mutex<crate::quit::Quit> = std::sync::Mutex::new(crate::quit::Quit::new());

/// Quit is under way: the program ends when its last window has gone.
pub fn quitting() -> bool {
    crate::lock(&QUIT).on()
}

/// Quit (⌘Q, and the test driver's): the windows close one by one, each brought forward and closed as its red button
/// closes it. Those with unsaved work go first, the one in front first, so each asks about its own work in turn while
/// the rest are still open; Cancel in any stops the quit there with every other window as it was (`crate::quit`).
/// Then the rest close, and the program ends after the last.
pub fn quit(app: &AppHandle) {
    if app.webview_windows().is_empty() {
        app.exit(0);
        return;
    }
    ask_next(app, None);
}

fn ask_next(app: &AppHandle, gone: Option<&str>) {
    let windows: Vec<WebviewWindow> = app.webview_windows().into_values().filter(|w| Some(w.label()) != gone).collect();
    let edited: Vec<bool> = {
        let state = app.state::<crate::Base>();
        let files = crate::lock(&state.files);
        windows.iter().map(|w| files.edited(w.label())).collect()
    };
    let front = |w: &WebviewWindow| w.is_focused().unwrap_or(false);
    let pick = |want: bool| {
        let all: Vec<&WebviewWindow> =
            windows.iter().zip(&edited).filter(|(_, e)| **e == want).map(|(w, _)| w).collect();
        all.iter().find(|w| front(w)).or(all.first()).copied()
    };
    // none left: the program ends as the last one goes, quitting still on (and a logout waiting for it may go on)
    let Some(w) = pick(true).or_else(|| pick(false)) else {
        answer_system(true);
        return;
    };
    crate::lock(&QUIT).ask(w.label());
    let _ = w.unminimize();
    let _ = w.set_focus();
    let _ = w.close();
}

/// The program, for a quit the system asks for (`guard_system_quit`).
static PROGRAM: std::sync::OnceLock<AppHandle> = std::sync::OnceLock::new();
/// The system asked whether the program may end (logging out, restarting, the Dock icon's Quit) and was told "later":
/// it waits for the answer (`answer_system`), as it waits for TextEdit's while TextEdit asks about each unsaved document.
static SYSTEM_WAITS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The answer the system is waiting for, if it is: yes when every window has closed, no when a question was answered
/// Cancel. A logout then goes on, or is called off by the system, as with TextEdit.
fn answer_system(yes: bool) {
    if !SYSTEM_WAITS.swap(false, std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    // after the window event that led here has been handled: a yes ends the program from inside AppKit
    if let Some(app) = PROGRAM.get() {
        let _ = app.run_on_main_thread(move || {
            if let Some(mtm) = objc2::MainThreadMarker::new() {
                objc2_app_kit::NSApplication::sharedApplication(mtm).replyToApplicationShouldTerminate(yes);
            }
        });
    }
}

/// A quit that comes from outside the program (the Dock icon's Quit, logging out, restarting, `quit app` from a script)
/// goes the way ⌘Q goes: AppKit asks the program's delegate first; while a window is open the answer is "later"
/// (`NSTerminateLater`, what an NSDocument program such as TextEdit answers while it asks about unsaved documents), and
/// `quit` closes the windows one by one, asking about unsaved work. The system waits: when the last window has gone the
/// answer is yes and the logout goes on; Cancel in any window's question answers no, and the system calls the logout
/// off.
/// Tauri's delegate (tao's `TaoAppDelegateParent`) doesn't answer that question, so AppKit would end the program at once,
/// with whatever was unsaved: the answer is added to it here, as the program starts.
pub fn guard_system_quit(app: &AppHandle) {
    use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
    let _ = PROGRAM.set(app.clone());
    /// NSApplicationTerminateReply: 0 cancel, 1 now, 2 later.
    unsafe extern "C-unwind" fn should_terminate(_this: &AnyObject, _sel: Sel, _sender: *mut AnyObject) -> usize {
        let Some(app) = PROGRAM.get() else { return 1 };
        if app.webview_windows().is_empty() {
            return 1;
        }
        SYSTEM_WAITS.store(true, std::sync::atomic::Ordering::Relaxed);
        if !quitting() {
            quit(app);
        }
        2
    }
    let Some(mtm) = objc2::MainThreadMarker::new() else { return };
    let Some(class) = AnyClass::get(c"TaoAppDelegateParent") else { return };
    // SAFETY: the function has the method's signature (NSUInteger, self, _cmd, sender), as its type string says
    unsafe {
        let imp: Imp = std::mem::transmute(
            should_terminate as unsafe extern "C-unwind" fn(&AnyObject, Sel, *mut AnyObject) -> usize,
        );
        objc2::ffi::class_addMethod(
            class as *const AnyClass as *mut AnyClass,
            objc2::sel!(applicationShouldTerminate:),
            imp,
            c"Q@:@".as_ptr(),
        );
    }
    // AppKit learns what its delegate answers when the delegate is set: set again, it asks
    let ns_app = objc2_app_kit::NSApplication::sharedApplication(mtm);
    if let Some(delegate) = ns_app.delegate() {
        ns_app.setDelegate(None);
        ns_app.setDelegate(Some(&*delegate));
    }
}

/// A window is asked to close: the quit's own request, or the person closing one (which gives a quit up).
pub fn close_requested(label: &str) {
    let was = quitting();
    crate::lock(&QUIT).close_requested(label);
    if was && !quitting() {
        answer_system(false);
    }
}

/// The question a window asked as it was closing was answered Cancel (its page says so): a quit under way stops there,
/// every window as it is, and a logout waiting for the program is called off.
pub fn close_given_up(label: &str) {
    if crate::lock(&QUIT).given_up(label) {
        answer_system(false);
    }
}

/// A window has gone: if it was the one the quit asked, the next is asked.
pub fn window_gone(app: &AppHandle, label: &str) {
    first_click::forget(label);
    let next = crate::lock(&QUIT).gone(label);
    if next {
        ask_next(app, Some(label));
    }
}

/// A new document window, made hidden, goes where the Mac puts the next one, cascaded down and right from `from` (the
/// window in front), the way AppKit's own document programs place theirs; then it comes forward.
pub fn cascade(window: &WebviewWindow, from: &WebviewWindow) {
    let (new, from) = (window.clone(), from.clone());
    let _ = window.run_on_main_thread(move || {
        let ns = |w: &WebviewWindow| w.ns_window().ok().map(|p| p as *const objc2_app_kit::NSWindow);
        let (Some(new_ptr), Some(from_ptr)) = (ns(&new), ns(&from)) else {
            let _ = new.show();
            return;
        };
        // SAFETY: Tauri hands out the windows' own NSWindows, alive while the windows are, on the main thread
        let (Some(new_ns), Some(from_ns)) = (unsafe { new_ptr.as_ref() }, unsafe { from_ptr.as_ref() }) else {
            let _ = new.show();
            return;
        };
        // where the window after `from` goes (not moving `from` itself, other than to keep it on the screen)
        let next = from_ns.cascadeTopLeftFromPoint(objc2_foundation::NSPoint::new(0.0, 0.0));
        new_ns.cascadeTopLeftFromPoint(next);
        new_ns.makeKeyAndOrderFront(None);
    });
}

/// The window that takes the keys (the one in front, of this program in front), if any.
fn key_window(app: &AppHandle) -> Option<WebviewWindow> {
    app.webview_windows().into_values().find(|w| w.is_focused().unwrap_or(false))
}

fn menu_chosen(app: &AppHandle, event: MenuEvent) {
    match event.id().as_ref() {
        "quit" => quit(app),
        // the window in front only: with none in front there is nothing to undo, as in any Mac program
        id @ ("undo" | "redo") => {
            if let Some(w) = key_window(app) {
                let _ = w.emit_to(w.label(), "app-menu", id);
            }
        }
        // the window in front shows them (brought back if it was in the Dock); with no window, one opens for them, as
        // TextEdit's and Preview's do
        id @ ("about" | "settings" | "new") => crate::to_page(app, "app-menu", id.into()),
        // the window in front asks, as its own Open… does (a sheet on it; the file goes where a file from Finder goes);
        // with no window, the system's open panel on its own
        "open" => open_from_menu(app, key_window(app)),
        // the window in front into full screen, or out of it
        "fullscreen" => {
            if let Some(w) = key_window(app) {
                let _ = w.set_fullscreen(!w.is_fullscreen().unwrap_or(false));
            }
        }
        // without document types the list shown is the family's own of this program's files: it is cleared, here and in
        // the window's own file menu (the same list), as TextEdit's Clear Menu clears TextEdit's
        "clear_recent" if !has_document_types() => {
            let state = app.state::<crate::Base>();
            for path in crate::family::recents_of(&state.family_dir, state.tool.folder, usize::MAX) {
                crate::family::forget_recent(&state.family_dir, &path);
            }
            let mut settings = crate::lock(&state.settings);
            settings.recents.clear();
            crate::settings::store(&state.data_dir, &settings);
            drop(settings);
            let _ = app.emit("recents", state.recents());
            refresh_recent(app);
        }
        "clear_recent" => {
            if let Some(mtm) = objc2::MainThreadMarker::new() {
                unsafe {
                    objc2_app_kit::NSDocumentController::sharedDocumentController(mtm).clearRecentDocuments(None)
                };
            }
            refresh_recent(app);
        }
        // a recent document: where a file from Finder goes
        id if id.starts_with("recent:") => {
            let path = app.try_state::<MenuParts>().and_then(|m| {
                let f = m.file.as_ref()?;
                let shown = crate::lock(&f.shown);
                shown.iter().find(|(i, _)| i.id().as_ref() == id).map(|(_, p)| p.clone())
            });
            if let Some(p) = path {
                crate::hand_files(app, vec![p]);
            }
        }
        _ => {}
    }
}

/// Open… in the menu bar: the system's open panel, a sheet on the window in front (as that window's own Open… is), or a
/// panel of its own with no window open (as TextEdit's); what is chosen goes where a file from Finder goes. In a tool
/// with a file per window the window in front opens it itself if it holds nothing (`crate::chosen_in`); a program that
/// keeps its own windows (Text) says where it goes (its `arrive`).
fn open_from_menu(app: &AppHandle, front: Option<WebviewWindow>) {
    let state = app.state::<crate::Base>();
    let exts = state.tool.extensions;
    let dir = match &front {
        Some(w) => state.start_folder_in(w.label()),
        None => state.start_folder(),
    };
    let per_window = state.per_window();
    let (app, label) = (app.clone(), front.as_ref().map(|w| w.label().to_string()));
    open_panel(front.as_ref(), exts, dir, move |path| {
        let Some(p) = path else { return };
        match label.filter(|_| per_window) {
            Some(label) => {
                if let Some(here) = crate::chosen_in(&app, &label, p) {
                    let _ = app.emit_to(label.as_str(), "open-path", here.to_string_lossy());
                }
            }
            None => crate::hand_files(&app, vec![p]),
        }
    });
}

// ---------- the open and save panels ----------

/// With `DOTANY_TEST` set, the open and save panels answer from a file instead of being shown, so that an automated
/// test can open and save: `DOTANY_TEST_PICK_OPEN` and `DOTANY_TEST_PICK_SAVE` name a file holding the path to
/// answer (nothing in it: Cancel).
/// Unset, or the file saying SHOW, the real panel shows, as for a person.
fn test_answer(var: &str) -> Option<Option<PathBuf>> {
    std::env::var_os("DOTANY_TEST")?;
    let file = std::env::var_os(var)?;
    let said = std::fs::read_to_string(file).unwrap_or_default();
    let said = said.trim();
    if said == "SHOW" {
        return None;
    }
    Some((!said.is_empty()).then(|| PathBuf::from(said)))
}

/// The NSWindow of a window, on the main thread.
fn ns_of(window: &WebviewWindow) -> Option<objc2::rc::Retained<objc2_app_kit::NSWindow>> {
    let ptr = window.ns_window().ok()? as *mut objc2_app_kit::NSWindow;
    // SAFETY: Tauri hands out the window's own NSWindow, alive while the window is
    unsafe { objc2::rc::Retained::retain(ptr) }
}

/// The kinds of file a panel offers, by extension (none: every file).
fn allow(panel: &objc2_app_kit::NSSavePanel, exts: &[&str]) {
    if exts.is_empty() {
        return;
    }
    let list: Vec<objc2::rc::Retained<objc2_foundation::NSString>> =
        exts.iter().map(|e| objc2_foundation::NSString::from_str(e)).collect();
    #[allow(deprecated)]
    panel.setAllowedFileTypes(Some(&objc2_foundation::NSArray::from_retained_slice(&list)));
}

fn folder_url(dir: &Path) -> objc2::rc::Retained<objc2_foundation::NSURL> {
    objc2_foundation::NSURL::fileURLWithPath_isDirectory(
        &objc2_foundation::NSString::from_str(&dir.to_string_lossy()),
        true,
    )
}

/// The system's open panel, for one file of the kinds `exts` names, starting in `dir`. On a window it is that window's
/// sheet, as a Mac program asks about one window's file: only that window waits for it, the others go on. With no
/// window it is a panel of its own. `done` hears the answer, on the main thread.
pub fn open_panel(
    on: Option<&WebviewWindow>,
    exts: &'static [&'static str],
    dir: Option<PathBuf>,
    done: impl FnOnce(Option<PathBuf>) + Send + 'static,
) {
    open_panel_for(on, exts, dir, Pick::File, move |p| done(p.into_iter().next()));
}

/// What an open panel picks: one file, several at once, or a folder.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Pick {
    File,
    Files,
    Folder,
}

/// The open panel as `open_panel`, for what `pick` says (a tool's own pickers: pages to add, a folder to extract to…);
/// `done` hears what was chosen, nothing for Cancel. In a test the answer file holds a path a line.
pub fn open_panel_for(
    on: Option<&WebviewWindow>,
    exts: &'static [&'static str],
    dir: Option<PathBuf>,
    pick: Pick,
    done: impl FnOnce(Vec<PathBuf>) + Send + 'static,
) {
    if let Some(answer) = test_answer("DOTANY_TEST_PICK_OPEN") {
        let said = answer.map(|a| a.to_string_lossy().into_owned()).unwrap_or_default();
        done(said.lines().map(str::trim).filter(|l| !l.is_empty()).map(PathBuf::from).collect());
        return;
    }
    let on = on.cloned();
    let run = move || {
        let Some(mtm) = objc2::MainThreadMarker::new() else { return };
        let panel = objc2_app_kit::NSOpenPanel::openPanel(mtm);
        panel.setCanChooseFiles(pick != Pick::Folder);
        panel.setCanChooseDirectories(pick == Pick::Folder);
        panel.setCanCreateDirectories(pick == Pick::Folder);
        panel.setAllowsMultipleSelection(pick == Pick::Files);
        allow(&panel, exts);
        if let Some(d) = dir.filter(|d| d.is_dir()) {
            panel.setDirectoryURL(Some(&folder_url(&d)));
        }
        let done = std::sync::Mutex::new(Some(done));
        let answer = panel.clone();
        let handler = block2::RcBlock::new(move |response: objc2_app_kit::NSModalResponse| {
            let Some(done) = crate::lock(&done).take() else { return };
            let paths = match response == objc2_app_kit::NSModalResponseOK {
                true => answer.URLs().iter().filter_map(|u| u.path()).map(|p| PathBuf::from(p.to_string())).collect(),
                false => vec![],
            };
            done(paths);
        });
        match on.as_ref().and_then(ns_of) {
            Some(window) => panel.beginSheetModalForWindow_completionHandler(&window, &handler),
            None => panel.beginWithCompletionHandler(&handler),
        }
    };
    match on_main_thread() {
        true => run(),
        false => {
            if let Some(app) = PROGRAM.get() {
                let _ = app.run_on_main_thread(run);
            }
        }
    }
}

/// `open_panel_for` on the window a command came from, waiting for the answer: for a `#[tauri::command(async)]`, which
/// runs off the main thread (on it, this would wait for itself).
pub fn ask_open(on: &WebviewWindow, exts: &'static [&'static str], dir: Option<PathBuf>, pick: Pick) -> Vec<PathBuf> {
    let (tx, rx) = std::sync::mpsc::channel();
    open_panel_for(Some(on), exts, dir, pick, move |p| {
        let _ = tx.send(p);
    });
    rx.recv().unwrap_or_default()
}

/// The system's save panel, as the window's sheet: the name to start with, the kind it is saved as (`ext`, with its
/// dot), the folder it starts in. `done` hears the answer, on the main thread.
pub fn save_panel(
    on: &WebviewWindow,
    dir: Option<PathBuf>,
    name: String,
    ext: String,
    done: impl FnOnce(Option<PathBuf>) + Send + 'static,
) {
    if let Some(answer) = test_answer("DOTANY_TEST_PICK_SAVE") {
        done(answer);
        return;
    }
    let on = on.clone();
    let _ = on.clone().run_on_main_thread(move || {
        let Some(mtm) = objc2::MainThreadMarker::new() else { return };
        let panel = objc2_app_kit::NSSavePanel::savePanel(mtm);
        if let Some(e) = ext.strip_prefix('.').filter(|e| !e.is_empty()) {
            allow(&panel, &[e]);
        }
        if let Some(d) = dir.map(PathBuf::from).filter(|d| d.is_dir()) {
            panel.setDirectoryURL(Some(&folder_url(&d)));
        }
        panel.setNameFieldStringValue(&objc2_foundation::NSString::from_str(&name));
        panel.setCanCreateDirectories(true);
        let done = std::sync::Mutex::new(Some(done));
        let answer = panel.clone();
        let handler = block2::RcBlock::new(move |response: objc2_app_kit::NSModalResponse| {
            let Some(done) = crate::lock(&done).take() else { return };
            let path = (response == objc2_app_kit::NSModalResponseOK)
                .then(|| answer.URL().and_then(|u| u.path()).map(|p| PathBuf::from(p.to_string())))
                .flatten();
            done(path);
        });
        match ns_of(&on) {
            Some(window) => panel.beginSheetModalForWindow_completionHandler(&window, &handler),
            None => panel.beginWithCompletionHandler(&handler),
        }
    });
}

/// Whether this runs on the main thread, where AppKit wants its windows and panels.
pub fn on_main_thread() -> bool {
    objc2::MainThreadMarker::new().is_some()
}

// ---------- recent documents ----------

/// The file goes on the system's list of this program's recent documents too (the family keeps its own as well): the
/// File menu's Open Recent and the Dock icon's menu list them, as for every Mac program. The system keeps only kinds of
/// file the program says it opens (its Info.plist).
pub fn note_recent(app: &AppHandle, path: &Path) {
    let path = path.to_path_buf();
    let app = app.clone();
    let _ = app.clone().run_on_main_thread(move || {
        let Some(mtm) = objc2::MainThreadMarker::new() else { return };
        let url =
            objc2_foundation::NSURL::fileURLWithPath(&objc2_foundation::NSString::from_str(&path.to_string_lossy()));
        objc2_app_kit::NSDocumentController::sharedDocumentController(mtm).noteNewRecentDocumentURL(&url);
        refresh_recent(&app);
    });
}

/// The family's recent list changed (a file forgotten, cleared, or opened in another window or program): Open Recent
/// follows it where it shows that list.
pub fn family_recents_changed(app: &AppHandle) {
    if has_document_types() {
        return;
    }
    let a = app.clone();
    let _ = app.run_on_main_thread(move || refresh_recent(&a));
}

/// The menu bar is the program's now (Tauri puts it in place as the program is built, before its setup): the Window
/// menu becomes the system's window menu, which lists every open window by its title, and Open Recent shows the
/// system's recent documents. Asked for earlier, AppKit has no menu bar to find the window menu in.
pub fn menu_ready(app: &AppHandle) {
    let Some(parts) = app.try_state::<MenuParts>() else { return };
    if let Some(window) = parts.windows.clone() {
        let _ = window.set_as_windows_menu_for_nsapp();
    }
    refresh_recent(app);
}

/// Open Recent lists the system's recent documents of this program (the list the Dock icon's menu shows, newest
/// first), each by its name with its icon as Finder shows it, then Clear Menu, which is off while the list is empty, as
/// in TextEdit. (AppKit fills such a menu by itself only for a menu made in a nib, as TextEdit's is.) A program Finder
/// hands no kinds of file (Recorder) has no such list from the system: the family's list of its files stands in, the
/// one its window's own file menu shows. On the main thread.
pub fn refresh_recent(app: &AppHandle) {
    let Some(mtm) = objc2::MainThreadMarker::new() else { return };
    let Some(parts) = app.try_state::<MenuParts>() else { return };
    let Some(f) = &parts.file else { return };
    let paths: Vec<PathBuf> = if has_document_types() {
        let urls = objc2_app_kit::NSDocumentController::sharedDocumentController(mtm).recentDocumentURLs();
        urls.iter().filter_map(|u| u.path().map(|p| PathBuf::from(p.to_string()))).collect()
    } else {
        app.state::<crate::Base>().recents().into_iter().map(PathBuf::from).collect()
    };
    let mut shown = crate::lock(&f.shown);
    if shown.iter().map(|(_, p)| p).eq(paths.iter()) {
        return;
    }
    for (item, _) in shown.drain(..) {
        let _ = f.recent.remove(&item);
    }
    let _ = f.recent.remove(&f.gap);
    for (i, path) in paths.iter().enumerate() {
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let Ok(item) = MenuItem::with_id(app, format!("recent:{i}"), name, true, None::<&str>) else { continue };
        let _ = f.recent.insert(&item, i);
        shown.push((item, path.clone()));
    }
    if !shown.is_empty() {
        let _ = f.recent.insert(&f.gap, shown.len());
    }
    let _ = f.clear.set_enabled(!shown.is_empty());
    // each with the icon Finder shows for it, small, as AppKit's own recent menus have them
    let icons: Vec<String> = shown.iter().map(|(_, p)| p.to_string_lossy().into_owned()).collect();
    drop(shown);
    let Some(menu) = objc2_app_kit::NSApplication::sharedApplication(mtm)
        .mainMenu()
        .and_then(|bar| bar.itemAtIndex(1))
        .and_then(|file| file.submenu())
        .and_then(|file| file.itemArray().iter().find_map(|i| i.submenu()))
    else {
        return;
    };
    let workspace = objc2_app_kit::NSWorkspace::sharedWorkspace();
    for (i, path) in icons.iter().enumerate() {
        if let Some(item) = menu.itemAtIndex(i as isize) {
            let icon = workspace.iconForFile(&objc2_foundation::NSString::from_str(path));
            icon.setSize(objc2_foundation::NSSize::new(16.0, 16.0));
            item.setImage(Some(&icon));
        }
    }
}

/// The window in front changed, or went into full screen or out of it: Enter Full Screen says what it would do to it
/// now, and is off with no window in front, as in every Mac program.
pub fn refresh_menu(app: &AppHandle) {
    let Some(m) = app.try_state::<MenuParts>() else { return };
    let Some((item, said)) = &m.full else { return };
    let front = key_window(app);
    let full = front.as_ref().is_some_and(|w| w.is_fullscreen().unwrap_or(false));
    let (enter, leave) = crate::lock(said).clone();
    let _ = item.set_text(if full { leave } else { enter });
    let _ = item.set_enabled(front.is_some());
}

/// The page's words for the menu bar, by item id (`WORDS`), in its language. `new` comes only from a tool that makes new
/// files: New goes into the File menu then, first, as in TextEdit's.
pub fn set_menu_words(app: &AppHandle, words: &std::collections::HashMap<String, String>) {
    let Some(m) = app.try_state::<MenuParts>() else { return };
    for (id, set) in &m.items {
        if let Some(t) = words.get(*id) {
            set(t);
        }
    }
    if let Some((_, said)) = &m.full {
        if let (Some(enter), Some(leave)) = (words.get("fullscreen"), words.get("unfullscreen")) {
            *crate::lock(said) = (enter.clone(), leave.clone());
        }
    }
    refresh_menu(app);
    if let Some(f) = &m.file {
        if words.contains_key("new") && !f.has_new.swap(true, std::sync::atomic::Ordering::Relaxed) {
            let _ = f.menu.prepend(&f.new);
        }
    }
}

// ---------- measuring ----------

pub fn ms_since_process_start() -> f64 {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    let got = unsafe {
        libc::proc_pidinfo(
            std::process::id() as libc::c_int,
            libc::PROC_PIDTBSDINFO,
            0,
            &mut info as *mut _ as *mut libc::c_void,
            size,
        )
    };
    if got != size {
        return 0.0;
    }
    let started =
        std::time::UNIX_EPOCH + std::time::Duration::new(info.pbi_start_tvsec, info.pbi_start_tvusec as u32 * 1000);
    std::time::SystemTime::now().duration_since(started).map(|d| d.as_secs_f64() * 1000.0).unwrap_or(0.0)
}

// ---------- the family ----------

/// The `.app` this program runs in, if it is one: `…/DotAny Text.app` for `…/DotAny Text.app/Contents/MacOS/dotany-text`.
fn bundle_of(exe: &Path) -> Option<&Path> {
    let bundle = exe.parent()?.parent()?.parent()?;
    bundle.extension().is_some_and(|e| e == "app").then_some(bundle)
}

/// A program running from its `.app` shows the bundle's own icon in the Dock, which macOS draws as it draws every other
/// program's (`Contents/Resources`, from `src-tauri/icons/icon.icns`). Tauri's development builds set the Dock icon from
/// the program's embedded icon as they start (`RunEvent::Ready`); this gives the Dock back to the bundle. A bare build,
/// run without a bundle, keeps the embedded one (the same Mac icon, `icons/icon.icns`): it has no other.
pub fn use_bundle_icon() {
    let Some(mtm) = objc2::MainThreadMarker::new() else { return };
    if std::env::current_exe().ok().is_some_and(|exe| bundle_of(&exe).is_some()) {
        // SAFETY: nil is AppKit's documented way back to the bundle's own icon
        unsafe { objc2_app_kit::NSApplication::sharedApplication(mtm).setApplicationIconImage(None) };
    }
}

/// Another DotAny tool, if it is installed beside this one: its `.app` in the same folder (DotAny as installed), or its
/// program in the same folder (a build).
pub fn sibling_tool(name: &str) -> Option<PathBuf> {
    let me = std::env::current_exe().ok()?;
    if let Some(dir) = bundle_of(&me).and_then(Path::parent) {
        return std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).find(|p| {
            p.extension().is_some_and(|e| e == "app") && p.join("Contents").join("MacOS").join(name).is_file()
        });
    }
    let there = me.parent()?.join(name);
    if there.is_file() { Some(there) } else { None }
}

/// Another window for a file, while this program's window holds one, for a tool built with `builder`, which keeps one
/// file for the program: another copy of the program (`open -n`). A tool with a file per window opens a window of its
/// own instead (`builder_per_window`).
pub fn open_another(path: &Path) {
    let Ok(me) = std::env::current_exe() else { return };
    match bundle_of(&me) {
        Some(app) => drop(std::process::Command::new("/usr/bin/open").arg("-n").arg("-a").arg(app).arg(path).spawn()),
        None => drop(std::process::Command::new(me).arg(path).spawn()),
    }
}

/// Starts a program on a file and lets it go. A `.app` is started the system's way, so a second start reaches the
/// one already open.
pub fn launch(program: &Path, file: &Path) -> bool {
    if program.extension().is_some_and(|e| e == "app") {
        return std::process::Command::new("/usr/bin/open").arg("-a").arg(program).arg(file).spawn().is_ok();
    }
    std::process::Command::new(program).arg(file).spawn().is_ok()
}

// ---------- the system's browser ----------

/// Hands a web address to the system's browser (`links::open_web_link` has already checked it is http or https).
pub fn open_url(url: &str) -> bool {
    std::process::Command::new("/usr/bin/open").arg(url).spawn().is_ok()
}

// ---------- the WebView ----------
// Windows shares one WebView2 among the family's programs and lets a window in the background give back memory
// (`windows/webview.rs`). The Mac's WKWebView has neither a browser to share nor a memory level to set: each window has
// its own WebView, kept by WebKit in `~/Library/WebKit/<bundle id>`, and the rest does nothing here.

/// Where `crate::webview::data_dir` points when no test moved things. WKWebView doesn't take a folder (`webview` leaves
/// it unused); this is only a place that is the family's.
pub fn shared_webview_dir() -> PathBuf {
    home().join("Library").join("Caches").join("DotAny").join("WebView")
}

/// WebView2's arguments have no counterpart in WKWebView.
pub fn browser_args() -> String {
    String::new()
}

/// The window's own WKWebView, as Tauri makes it: no folder or arguments to give it.
pub fn webview<'a, R: Runtime, M: Manager<R>>(
    builder: WebviewWindowBuilder<'a, R, M>,
    _dir: PathBuf,
) -> WebviewWindowBuilder<'a, R, M> {
    builder
}

/// What went: the browser every window shares (Windows only), or only this window's page.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Gone {
    Browser,
    Page,
}

/// What to do when a window's page ends, by the window's label (`watch_webview`).
type OnGone = Box<dyn Fn(Gone) + Send>;
static PAGES: std::sync::LazyLock<std::sync::Mutex<std::collections::HashMap<String, OnGone>>> =
    std::sync::LazyLock::new(Default::default);

/// A window's page ending: WebKit runs each window's page in a process of its own, and when that process ends (it
/// crashed, or the system took it back for memory) the window would stay white. It comes back instead, as on Windows:
/// made again where it was, with its file (`webview`). Sound has no counterpart here.
pub fn watch_webview(
    window: &WebviewWindow,
    gone: impl Fn(Gone) + Send + 'static,
    _sound: impl Fn(bool) + Send + 'static,
) {
    crate::lock(&PAGES).insert(window.label().to_string(), Box::new(gone));
}

/// WebKit says the window's page process ended (Tauri's `on_web_content_process_terminate`, set in `menu`).
fn page_gone(label: &str) {
    let pages = crate::lock(&PAGES);
    if let Some(gone) = pages.get(label) {
        gone(Gone::Page);
    }
}

/// The window has gone for good: nothing to bring back.
pub fn forget_page(label: &str) {
    crate::lock(&PAGES).remove(label);
}

/// Nothing to lower: answered as done, so the window isn't asked again and again.
pub fn rest(
    _window: &WebviewWindow,
    _still: impl Fn() -> bool + Send + 'static,
    done: impl Fn(bool) + Send + Sync + 'static,
) {
    done(true);
}

pub fn wake(_window: &WebviewWindow) {}

/// Where a zoomed or minimized window goes back to: AppKit keeps no such place for us, so a window coming back zoomed is
/// made zoomed again (`webview`), and one in the Dock where it is now.
pub fn normal_place(_window: &WebviewWindow) -> Option<(tauri::PhysicalPosition<i32>, tauri::PhysicalSize<u32>)> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_name_is_the_same_in_either_unicode_form_and_either_case() {
        let same = |a: &str, b: &str| same_name(a.as_ref(), b.as_ref());
        // é as one character (NFC, as typed) and as e with its accent (NFD, as the Mac's disks and some archives give it)
        assert!(same("Caf\u{e9}.txt", "Cafe\u{301}.txt"));
        assert!(same("caf\u{e9}.TXT", "CAFE\u{301}.txt"));
        assert!(same("Notes.TXT", "notes.txt"));
        assert!(same("\u{304c}.md", "\u{304b}\u{3099}.md"), "が in either form");
        assert!(!same("Cafe.txt", "Caf\u{e9}.txt"));
        assert!(!same("notes.txt", "notes.md"));
    }

    #[test]
    fn a_double_click_on_the_title_bar_does_what_the_setting_says() {
        assert_eq!(title_action(Some("Maximize"), false), TitleAction::Zoom);
        assert_eq!(title_action(Some("Minimize"), false), TitleAction::Minimize);
        assert_eq!(title_action(Some("Fill"), false), TitleAction::Fill);
        assert_eq!(title_action(Some("None"), true), TitleAction::Nothing);
        // the older switch, on a Mac set before the four choices
        assert_eq!(title_action(None, true), TitleAction::Minimize);
        assert_eq!(title_action(None, false), TitleAction::Zoom);
    }
}
