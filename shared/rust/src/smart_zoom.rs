//! The trackpad's smart zoom (a double tap with two fingers), for the page. WKWebView keeps that gesture for its own
//! page zoom, which the family's windows don't use, and then passes it on to nobody: the page never hears of it (pinch
//! and rotate do reach the page, as WebKit's `gesturechange`). So the program watches for it and tells the window's page
//! where it was: `smart-zoom` with the point in the page's pixels. What it does is the page's (`shared/web/gestures.js`:
//! Image toggles fit and 100%, PDF fit width and back, as Preview does).

use std::ptr::NonNull;
use std::sync::OnceLock;

use block2::RcBlock;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSEvent, NSEventMask};
use tauri::{AppHandle, Emitter, Manager};

static APP: OnceLock<AppHandle> = OnceLock::new();

/// Once, as the program starts (on the main thread).
pub fn watch(app: &AppHandle) {
    if APP.set(app.clone()).is_err() {
        return;
    }
    let block = RcBlock::new(|event: NonNull<NSEvent>| -> *mut NSEvent {
        // SAFETY: AppKit hands the monitor a live event, on the main thread
        let e = unsafe { event.as_ref() };
        if let (Some(app), Some(mtm)) = (APP.get(), MainThreadMarker::new()) {
            if let Some(ns) = e.window(mtm) {
                let height = ns.contentView().map(|v| v.frame().size.height).unwrap_or(0.0);
                let at = e.locationInWindow();
                let mine = objc2::rc::Retained::as_ptr(&ns) as *mut std::ffi::c_void;
                if let Some(w) = app.webview_windows().into_values().find(|w| w.ns_window().ok() == Some(mine)) {
                    let _ = app.emit_to(w.label(), "smart-zoom", serde_json::json!({ "x": at.x, "y": height - at.y }));
                }
            }
        }
        // the event goes on unchanged: nothing else in the window uses it
        event.as_ptr()
    });
    // SAFETY: the block has the signature AppKit asks for; the monitor is kept for the program's life
    let monitor = unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(NSEventMask::SmartMagnify, &block) };
    std::mem::forget(monitor);
}
