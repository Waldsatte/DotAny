//! The Windows side of the platform module.

use crate::atomic::WriteFailure;
use std::cell::Cell;
use std::ffi::OsStr;
use std::fs::{File, OpenOptions};
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use tauri::{Emitter, WebviewWindow};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_HIDDEN, FILE_ATTRIBUTE_NORMAL, FILE_ATTRIBUTE_READONLY, GetFileAttributesW, INVALID_FILE_ATTRIBUTES,
    MOVEFILE_WRITE_THROUGH, MoveFileExW, REPLACEFILE_IGNORE_MERGE_ERRORS, ReplaceFileW, SetFileAttributesW,
};
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{TME_LEAVE, TME_NONCLIENT, TRACKMOUSEEVENT, TrackMouseEvent};
use windows_sys::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, EnableMenuItem, GetParent, GetPropW, GetSystemMenu, HTMAXBUTTON, HWND_TOP,
    IDC_HAND, LWA_ALPHA, LoadCursorW, MF_BYCOMMAND, MF_ENABLED, MF_GRAYED, PostMessageW, RegisterClassW, RemovePropW,
    SC_MAXIMIZE, SC_MOVE, SC_RESTORE, SC_SIZE, SWP_NOACTIVATE, SetCursor, SetLayeredWindowAttributes, SetPropW,
    SetWindowPos, TPM_RETURNCMD, TrackPopupMenu, WM_NCDESTROY, WM_NCHITTEST, WM_NCLBUTTONDBLCLK, WM_NCLBUTTONDOWN,
    WM_NCLBUTTONUP, WM_NCMOUSELEAVE, WM_NCMOUSEMOVE, WM_SETCURSOR, WM_SYSCOMMAND, WNDCLASSW, WS_CHILD, WS_EX_LAYERED,
    WS_EX_NOACTIVATE, WS_VISIBLE,
};

// the family's one WebView2: its folder and arguments, its crash event, its memory level
mod webview;
pub use self::webview::*;

fn wide(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(std::iter::once(0)).collect()
}

// ---------- files ----------

pub fn write_failure(err: &io::Error) -> WriteFailure {
    match err.raw_os_error() {
        Some(32) | Some(33) | Some(1175) | Some(1176) | Some(1177) => WriteFailure::Locked,
        Some(39) | Some(112) => WriteFailure::DiskFull,
        Some(5) => WriteFailure::Denied,
        Some(2) | Some(3) => WriteFailure::Missing,
        _ => WriteFailure::Other,
    }
}

pub fn is_read_only(path: &Path) -> bool {
    let attrs = unsafe { GetFileAttributesW(wide(path).as_ptr()) };
    attrs != INVALID_FILE_ATTRIBUTES && attrs & FILE_ATTRIBUTE_READONLY != 0
}

/// A new file nobody should notice while it exists.
pub fn create_temp(path: &Path) -> io::Result<File> {
    OpenOptions::new().write(true).create_new(true).attributes(FILE_ATTRIBUTE_HIDDEN).open(path)
}

/// ReplaceFile keeps the old file's attributes, permissions, creation time and streams.
pub fn replace_file(target: &Path, temp: &Path) -> io::Result<()> {
    let (t, r) = (wide(target), wide(temp));
    let mut last = io::Error::from_raw_os_error(0);
    // virus scanners and indexers often hold a file for a moment after it changes
    for attempt in 0..6 {
        let ok = unsafe {
            ReplaceFileW(
                t.as_ptr(),
                r.as_ptr(),
                std::ptr::null(),
                REPLACEFILE_IGNORE_MERGE_ERRORS,
                std::ptr::null(),
                std::ptr::null(),
            )
        };
        if ok != 0 {
            return Ok(());
        }
        last = io::Error::last_os_error();
        match last.raw_os_error() {
            Some(32) | Some(33) | Some(5) | Some(1175) if attempt < 5 => {
                std::thread::sleep(std::time::Duration::from_millis(40))
            }
            _ => break,
        }
    }
    Err(last)
}

/// The temp file becomes a new file, no longer hidden.
pub fn place_file(target: &Path, temp: &Path) -> io::Result<()> {
    let (t, r) = (wide(target), wide(temp));
    let ok = unsafe { MoveFileExW(r.as_ptr(), t.as_ptr(), MOVEFILE_WRITE_THROUGH) };
    if ok == 0 {
        return Err(io::Error::last_os_error());
    }
    unsafe { SetFileAttributesW(t.as_ptr(), FILE_ATTRIBUTE_NORMAL) };
    Ok(())
}

/// A lock several programs take turns on: the file opened with nobody else allowed in. Held until dropped; after a
/// second of waiting, carries on without it rather than hang.
pub fn lock_file(path: &Path) -> Option<File> {
    use std::os::windows::fs::OpenOptionsExt;
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    for _ in 0..100 {
        match OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .share_mode(0)
            .attributes(FILE_ATTRIBUTE_HIDDEN)
            .open(path)
        {
            Ok(f) => return Some(f),
            Err(_) => std::thread::sleep(std::time::Duration::from_millis(10)),
        }
    }
    None
}

/// File names here ignore case.
pub fn same_name(a: &OsStr, b: &OsStr) -> bool {
    a.eq_ignore_ascii_case(b)
}

// ---------- per-user folders ----------

/// Each tool keeps its settings in its own folder: `%APPDATA%\DotAny\<folder>`.
pub fn data_dir(folder: &str) -> PathBuf {
    let base = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("DotAny").join(folder)
}

/// One of the user's own folders, by its name under the profile ("Documents", "Music", "Pictures", "Videos"), or the
/// profile itself if that folder isn't there.
pub fn user_dir(name: &str) -> PathBuf {
    let home = std::env::var_os("USERPROFILE").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    let dir = home.join(name);
    if dir.is_dir() { dir } else { home }
}

// ---------- the window's frame ----------

pub fn system_is_dark() -> bool {
    use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, RRF_RT_REG_DWORD, RegGetValueW};
    let w = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let (key, name) = (w(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"), w("AppsUseLightTheme"));
    let (mut value, mut size) = (1u32, 4u32);
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_DWORD,
            std::ptr::null_mut(),
            &mut value as *mut u32 as *mut _,
            &mut size,
        )
    };
    status == 0 && value == 0
}

/// The system's dark or light frame, matching the window.
pub fn paint_frame(window: &WebviewWindow, dark: bool) {
    use windows_sys::Win32::Graphics::Dwm::{DWMWA_USE_IMMERSIVE_DARK_MODE, DwmSetWindowAttribute};
    let Ok(hwnd) = window.hwnd() else { return };
    let hwnd = hwnd.0 as *mut core::ffi::c_void;
    let mode: i32 = dark as i32;
    unsafe {
        DwmSetWindowAttribute(hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE as u32, &mode as *const i32 as *const _, 4);
    }
}

// The maximize button's snap layouts: Windows shows them when the window under the pointer answers that it is a maximize button.
// The page sits in another process and would answer for itself, so a child window nearly clear of paint lies over the button.
// It says what it is, and takes the pointer and the click itself, the way Windows Terminal does.
struct Snap {
    window: WebviewWindow,
    cover: HWND,
    over: Cell<bool>,
    pressed: Cell<bool>,
}

const SNAP_PROP: &str = "DotAnySnap";

fn snap_of(hwnd: HWND) -> *const Snap {
    let name: Vec<u16> = SNAP_PROP.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe { GetPropW(hwnd, name.as_ptr()) as *const Snap }
}

unsafe extern "system" fn cover_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM) -> LRESULT {
    let snap = snap_of(GetParent(hwnd));
    if snap.is_null() {
        return DefWindowProcW(hwnd, msg, w, l);
    }
    let snap = &*snap;
    let set_over = |over: bool| {
        if snap.over.replace(over) != over {
            let _ = snap.window.emit_to(snap.window.label(), "snap-hover", over);
        }
    };
    match msg {
        WM_NCHITTEST => return HTMAXBUTTON as LRESULT,
        // the same hand as the page's buttons beside it
        WM_SETCURSOR => {
            SetCursor(LoadCursorW(std::ptr::null_mut(), IDC_HAND));
            return 1;
        }
        WM_NCMOUSEMOVE => {
            if !snap.over.get() {
                let mut track = TRACKMOUSEEVENT {
                    cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE | TME_NONCLIENT,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                TrackMouseEvent(&mut track);
            }
            set_over(true);
            return 0;
        }
        WM_NCMOUSELEAVE => {
            set_over(false);
            snap.pressed.set(false);
            return 0;
        }
        WM_NCLBUTTONDOWN | WM_NCLBUTTONDBLCLK => {
            snap.pressed.set(true);
            return 0;
        }
        WM_NCLBUTTONUP => {
            if snap.pressed.replace(false) {
                let _ = snap.window.emit_to(snap.window.label(), "snap-click", ());
                let _ = if snap.window.is_maximized().unwrap_or(false) {
                    snap.window.unmaximize()
                } else {
                    snap.window.maximize()
                };
            }
            return 0;
        }
        _ => {}
    }
    DefWindowProcW(hwnd, msg, w, l)
}

unsafe extern "system" fn frame_proc(hwnd: HWND, msg: u32, w: WPARAM, l: LPARAM, _id: usize, data: usize) -> LRESULT {
    if msg == WM_NCDESTROY {
        let name: Vec<u16> = SNAP_PROP.encode_utf16().chain(std::iter::once(0)).collect();
        RemovePropW(hwnd, name.as_ptr());
        RemoveWindowSubclass(hwnd, Some(frame_proc), 1);
        let result = DefSubclassProc(hwnd, msg, w, l);
        drop(Box::from_raw(data as *mut Snap));
        return result;
    }
    DefSubclassProc(hwnd, msg, w, l)
}

/// Runs once for each new window.
pub fn prepare_frame(window: &WebviewWindow) {
    let window = window.clone();
    let inner = window.clone();
    let _ = window.run_on_main_thread(move || unsafe {
        let Ok(hwnd) = inner.hwnd() else { return };
        let hwnd = hwnd.0 as HWND;
        let class: Vec<u16> = "DotAnySnapCover".encode_utf16().chain(std::iter::once(0)).collect();
        let instance = GetModuleHandleW(std::ptr::null());
        let mut info: WNDCLASSW = std::mem::zeroed();
        info.lpfnWndProc = Some(cover_proc);
        info.hInstance = instance;
        info.lpszClassName = class.as_ptr();
        RegisterClassW(&info);
        let cover = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_NOACTIVATE,
            class.as_ptr(),
            std::ptr::null(),
            WS_CHILD | WS_VISIBLE,
            0,
            0,
            0,
            0,
            hwnd,
            std::ptr::null_mut(),
            instance,
            std::ptr::null(),
        );
        if cover.is_null() {
            // without it the page's own button still works; only the snap layouts are missing
            return;
        }
        SetLayeredWindowAttributes(cover, 0, 1, LWA_ALPHA);
        let snap = Box::into_raw(Box::new(Snap {
            window: inner.clone(),
            cover,
            over: Cell::new(false),
            pressed: Cell::new(false),
        }));
        let name: Vec<u16> = SNAP_PROP.encode_utf16().chain(std::iter::once(0)).collect();
        SetPropW(hwnd, name.as_ptr(), snap as *mut core::ffi::c_void);
        SetWindowSubclass(hwnd, Some(frame_proc), 1, snap as usize);
    });
}

/// Where the maximize button is, in pixels from the top left of the window.
pub fn set_snap_button(window: &WebviewWindow, x: i32, y: i32, w: i32, h: i32) {
    let inner = window.clone();
    let _ = window.run_on_main_thread(move || unsafe {
        let Ok(hwnd) = inner.hwnd() else { return };
        let snap = snap_of(hwnd.0 as HWND);
        if snap.is_null() {
            return;
        }
        SetWindowPos((*snap).cover, HWND_TOP, x, y, w, h, SWP_NOACTIVATE);
    });
}

/// The system's own window menu, for a right click on the top bar.
pub fn show_system_menu(window: &WebviewWindow, x: i32, y: i32) {
    let inner = window.clone();
    let _ = window.run_on_main_thread(move || unsafe {
        let Ok(hwnd) = inner.hwnd() else { return };
        let hwnd = hwnd.0 as HWND;
        let menu = GetSystemMenu(hwnd, 0);
        if menu.is_null() {
            return;
        }
        let maximized = inner.is_maximized().unwrap_or(false);
        let state = |on: bool| if on { MF_ENABLED } else { MF_GRAYED };
        EnableMenuItem(menu, SC_RESTORE, MF_BYCOMMAND | state(maximized));
        EnableMenuItem(menu, SC_MAXIMIZE, MF_BYCOMMAND | state(!maximized));
        EnableMenuItem(menu, SC_MOVE, MF_BYCOMMAND | state(!maximized));
        EnableMenuItem(menu, SC_SIZE, MF_BYCOMMAND | state(!maximized));
        let chosen = TrackPopupMenu(menu, TPM_RETURNCMD, x, y, 0, hwnd, std::ptr::null());
        if chosen != 0 {
            PostMessageW(hwnd, WM_SYSCOMMAND, chosen as WPARAM, 0);
        }
    });
}

/// The top bar is the only bar: it holds the window's buttons and moves the window.
pub fn frame<'a, R: tauri::Runtime, M: tauri::Manager<R>>(
    builder: tauri::webview::WebviewWindowBuilder<'a, R, M>,
) -> tauri::webview::WebviewWindowBuilder<'a, R, M> {
    builder.decorations(false)
}

/// Windows shows unsaved work with the top bar's dot alone.
pub fn set_edited(_window: &WebviewWindow, _edited: bool) {}

// ---------- the menu bar and the program's life ----------

/// Windows has no menu bar: everything is in the window's own menu.
pub fn menu(
    builder: tauri::Builder<tauri::Wry>,
    _chosen: impl Fn(&tauri::AppHandle) -> Option<String> + Send + 'static,
) -> tauri::Builder<tauri::Wry> {
    builder
}

pub fn set_menu_words(_app: &tauri::AppHandle, _words: &std::collections::HashMap<String, String>) {}

/// Windows ends a program when its last window closes.
pub const STAYS_WITHOUT_WINDOWS: bool = false;

pub fn quitting() -> bool {
    false
}

/// The Mac's Quit closes the windows one by one; Windows has no Quit, and a program's window closing ends it.
pub fn close_requested(_label: &str) {}

pub fn window_gone(_app: &tauri::AppHandle, _label: &str) {}

/// A program here never has a second window for a file (each file is a program of its own): a window made hidden to be
/// placed is only shown.
pub fn cascade(window: &WebviewWindow, _from: &WebviewWindow) {
    let _ = window.show();
}

/// Another window for a file: on Windows a program of its own, as every file is. (Only the Mac hands a running program
/// files, so nothing calls this on Windows.)
pub fn open_another(path: &Path) {
    if let Ok(me) = std::env::current_exe() {
        let _ = std::process::Command::new(me).arg(path).spawn();
    }
}

// ---------- measuring ----------

pub fn ms_since_process_start() -> f64 {
    use windows_sys::Win32::Foundation::FILETIME;
    use windows_sys::Win32::System::SystemInformation::GetSystemTimeAsFileTime;
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
    let zero = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
    let (mut created, mut a, mut b, mut c, mut now) = (zero, zero, zero, zero, zero);
    let ok = unsafe { GetProcessTimes(GetCurrentProcess(), &mut created, &mut a, &mut b, &mut c) };
    if ok == 0 {
        return 0.0;
    }
    unsafe { GetSystemTimeAsFileTime(&mut now) };
    let to = |f: FILETIME| ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64;
    (to(now).saturating_sub(to(created))) as f64 / 10_000.0
}

// ---------- the family ----------

/// Another DotAny tool, if it is installed beside this one.
pub fn sibling_tool(name: &str) -> Option<PathBuf> {
    let me = std::env::current_exe().ok()?;
    let there = me.parent()?.join(format!("{name}.exe"));
    if there.is_file() { Some(there) } else { None }
}

/// Starts a program on a file and lets it go.
pub fn launch(program: &Path, file: &Path) -> bool {
    std::process::Command::new(program).arg(file).spawn().is_ok()
}

// ---------- the system's browser ----------

/// Hands a web address to the system's browser (`links::open_web_link` has already checked it is http or https).
pub fn open_url(url: &str) -> bool {
    let wide = |s: &str| s.encode_utf16().chain(std::iter::once(0)).collect::<Vec<u16>>();
    let (verb, target) = (wide("open"), wide(url));
    // SAFETY: two NUL-terminated wide strings; ShellExecuteW returns a value above 32 when it worked
    let done = unsafe {
        windows_sys::Win32::UI::Shell::ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            target.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL,
        )
    };
    done as isize > 32
}
