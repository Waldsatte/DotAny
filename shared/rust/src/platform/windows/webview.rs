//! The family's one WebView2. Every DotAny program starts its WebView2 in one
//! folder with the same arguments, byte for byte, so the second program joins the first one's browser instead of
//! starting a browser of its own. Each window hears when that browser, or its own page, goes; and a window in the
//! background can let go of memory (WebView2's `MemoryUsageTargetLevel`). What to do about either is `crate::webview`'s.

use std::path::PathBuf;
use std::sync::Arc;
use tauri::WebviewWindow;
use tauri::webview::WebviewWindowBuilder;
use webview2_com::Microsoft::Web::WebView2::Win32::{
    COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW,
    COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL, COREWEBVIEW2_PROCESS_FAILED_KIND,
    COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED, COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED,
    ICoreWebView2_8, ICoreWebView2_19,
};
use webview2_com::{
    ExecuteScriptCompletedHandler, IsDocumentPlayingAudioChangedEventHandler, ProcessFailedEventHandler,
};
use windows::core::{BOOL, HSTRING, Interface};

/// wry's own defaults (no mini menu over selected text, no SmartScreen), written out here so that no program can
/// differ: a program whose arguments differ by one byte from the running browser's can't join it, and its window
/// stays empty. So changing them means changing the folder's name too (`shared_webview_dir`): a program from a newer
/// install would otherwise meet a browser an older one still runs.
const BROWSER_ARGS: &str = "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection";

/// The arguments every program gives its WebView2. An automated test reaches the pages through DevTools: with
/// `DOTANY_TEST` and `DOTANY_DEBUG_PORT` both set, and only then, the port is added here in the same words for every
/// program, so two programs under one test still share a browser.
pub fn browser_args() -> String {
    args_for(std::env::var_os("DOTANY_TEST").is_some(), std::env::var("DOTANY_DEBUG_PORT").ok().as_deref())
}

/// No `--remote-allow-origins`: Chromium then refuses a DevTools WebSocket whose request carries an `Origin` header
/// (unless it is the port's own address). A web page always sends one, so no page open in a browser can reach the
/// port; a test's driver is a program, not a page, and sends none, so it is let in.
fn args_for(test: bool, port: Option<&str>) -> String {
    match port.filter(|_| test).and_then(|p| p.trim().parse::<u16>().ok()) {
        Some(port) => format!("{BROWSER_ARGS} --remote-debugging-port={port}"),
        None => BROWSER_ARGS.to_string(),
    }
}

/// The one WebView2 folder of the whole family: `%LOCALAPPDATA%\DotAny\WebView2`. It is a cache, nothing of the
/// user's: no program keeps anything in the browser's storage.
pub fn shared_webview_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    base.join("DotAny").join("WebView2")
}

/// A window's WebView2 in `dir`, with the family's arguments.
pub fn webview<'a, R: tauri::Runtime, M: tauri::Manager<R>>(
    builder: WebviewWindowBuilder<'a, R, M>,
    dir: PathBuf,
) -> WebviewWindowBuilder<'a, R, M> {
    builder.data_directory(dir).additional_browser_args(&browser_args())
}

/// What went: the browser every window of the family shares, or only this window's page.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Gone {
    Browser,
    Page,
}

/// Calls `gone` when the browser or this window's page ends without being asked to (WebView2's ProcessFailed; a
/// window can't be used again after either), and `sound` when the page starts or stops playing sound.
pub fn watch_webview(
    window: &WebviewWindow,
    gone: impl Fn(Gone) + Send + 'static,
    sound: impl Fn(bool) + Send + 'static,
) {
    let _ = window.with_webview(move |w| unsafe {
        let Ok(core) = w.controller().CoreWebView2() else { return };
        let mut token = 0i64;
        let failed = ProcessFailedEventHandler::create(Box::new(move |_, args| {
            let mut kind = COREWEBVIEW2_PROCESS_FAILED_KIND::default();
            if let Some(args) = args {
                args.ProcessFailedKind(&mut kind)?;
            }
            if kind == COREWEBVIEW2_PROCESS_FAILED_KIND_BROWSER_PROCESS_EXITED {
                gone(Gone::Browser);
            } else if kind == COREWEBVIEW2_PROCESS_FAILED_KIND_RENDER_PROCESS_EXITED {
                gone(Gone::Page);
            }
            // the GPU and helper processes come back by themselves; a page that hangs is left alone
            Ok(())
        }));
        let _ = core.add_ProcessFailed(&failed, &mut token);
        if let Ok(core) = core.cast::<ICoreWebView2_8>() {
            let heard = IsDocumentPlayingAudioChangedEventHandler::create(Box::new(move |sender, _| {
                let mut on = BOOL::default();
                if let Some(s) = sender.and_then(|s| s.cast::<ICoreWebView2_8>().ok()) {
                    s.IsDocumentPlayingAudio(&mut on)?;
                }
                sound(on.as_bool());
                Ok(())
            }));
            let _ = core.add_IsDocumentPlayingAudioChanged(&heard, &mut token);
        }
    });
}

/// What the page answers when asked whether it may rest: `app.quiet`, the same question a button's sound asks
/// (Video while a film plays, Recorder while it records). shared/web/shared.js puts it on the window.
const BUSY: &str = "!!(window.__DOTANY_QUIET__ && window.__DOTANY_QUIET__())";

/// Lets the window's page go down to WebView2's low memory level, unless it is playing sound or says it is busy.
/// `still` is asked once more on the main thread just before; `done` hears whether it went down.
pub fn rest(
    window: &WebviewWindow,
    still: impl Fn() -> bool + Send + 'static,
    done: impl Fn(bool) + Send + Sync + 'static,
) {
    let done = Arc::new(done);
    let answer = done.clone();
    let sent = window.with_webview(move |w| unsafe {
        let Ok(core) = w.controller().CoreWebView2() else { return done(false) };
        if let Ok(core) = core.cast::<ICoreWebView2_8>() {
            let mut on = BOOL::default();
            if core.IsDocumentPlayingAudio(&mut on).is_ok() && on.as_bool() {
                return done(false);
            }
        }
        let low = core.clone();
        let then = done.clone();
        let asked = ExecuteScriptCompletedHandler::create(Box::new(move |result, json| {
            let idle = result.is_ok() && json == "false";
            then(idle && still() && set_level(&low, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW));
            Ok(())
        }));
        if core.ExecuteScript(&HSTRING::from(BUSY), &asked).is_err() {
            done(false);
        }
    });
    if sent.is_err() {
        answer(false);
    }
}

/// Back to WebView2's normal memory level.
pub fn wake(window: &WebviewWindow) {
    let _ = window.with_webview(|w| unsafe {
        if let Ok(core) = w.controller().CoreWebView2() {
            set_level(&core, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL);
        }
    });
}

fn set_level(
    core: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2,
    level: COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL,
) -> bool {
    unsafe { core.cast::<ICoreWebView2_19>().and_then(|c| c.SetMemoryUsageTargetLevel(level)).is_ok() }
}

/// Where the window goes when it is restored: its outer place before it was minimized or maximized. Windows keeps it
/// in work-area coordinates (beside a taskbar on the left or top they differ from the screen's).
pub fn normal_place(window: &WebviewWindow) -> Option<(tauri::PhysicalPosition<i32>, tauri::PhysicalSize<u32>)> {
    use windows_sys::Win32::Graphics::Gdi::{
        GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromWindow,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetWindowPlacement, WINDOWPLACEMENT};
    let hwnd = window.hwnd().ok()?.0 as windows_sys::Win32::Foundation::HWND;
    unsafe {
        let mut placement: WINDOWPLACEMENT = std::mem::zeroed();
        placement.length = std::mem::size_of::<WINDOWPLACEMENT>() as u32;
        if GetWindowPlacement(hwnd, &mut placement) == 0 {
            return None;
        }
        let mut info: MONITORINFO = std::mem::zeroed();
        info.cbSize = std::mem::size_of::<MONITORINFO>() as u32;
        let (dx, dy) = if GetMonitorInfoW(MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST), &mut info) != 0 {
            (info.rcWork.left - info.rcMonitor.left, info.rcWork.top - info.rcMonitor.top)
        } else {
            (0, 0)
        };
        let r = placement.rcNormalPosition;
        let (w, h) = ((r.right - r.left).max(0) as u32, (r.bottom - r.top).max(0) as u32);
        Some((tauri::PhysicalPosition::new(r.left + dx, r.top + dy), tauri::PhysicalSize::new(w, h)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_debugging_port_opens_only_in_a_test() {
        assert_eq!(args_for(false, None), BROWSER_ARGS);
        assert_eq!(args_for(true, None), BROWSER_ARGS);
        // the port alone, outside a test, opens nothing
        assert_eq!(args_for(false, Some("9444")), BROWSER_ARGS);
        assert_eq!(args_for(true, Some("9444")), format!("{BROWSER_ARGS} --remote-debugging-port=9444"));
        assert_eq!(args_for(true, Some(" 9444 ")), format!("{BROWSER_ARGS} --remote-debugging-port=9444"));
        assert_eq!(args_for(true, Some("9444 --remote-allow-origins=*")), BROWSER_ARGS);
        assert_eq!(args_for(true, Some("no")), BROWSER_ARGS);
    }

    #[test]
    fn no_web_page_is_let_in() {
        assert!(!args_for(true, Some("9444")).contains("remote-allow-origins"));
    }
}
