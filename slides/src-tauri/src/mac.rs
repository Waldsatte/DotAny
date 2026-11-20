//! What Slides asks of the Mac where Windows asks WebView2 or Media Foundation:
//!
//! - a PDF of the slides: WKWebView prints the page's print layout (one slide to a sheet the slide's own size, the same
//!   `@media print` rules WebView2 prints on Windows) to a file, through the system's print operation without its panel
//!   (`print_to`). Not `createPDF`: that draws one page of the screen as it is, where the print operation lays the deck
//!   out sheet by sheet, as WebView2 does, with the text as text and the links as links;
//! - the slides as PNG pictures: that same PDF, each sheet drawn by CoreGraphics at the width asked (`pdf_pictures`).
//!   Not the page's own way (an SVG picture of the slide drawn onto a canvas, `picture.js`): WebKit lays out the HTML
//!   inside such a picture without its viewBox's scale, and draws the pictures inside it only some while after the
//!   picture says it has loaded, with nothing to wait for;
//! - a picture of part of the page: WKWebView's own snapshot (`capture`), at the scale asked (2 on a Retina screen);
//! - a film in a slide that WKWebView won't play (AVI): repackaged with the family's `dotany-repack` (`repack`); WMV and
//!   WMA aren't played on the Mac at all;
//! - where a show goes when the Mac has more than one screen (`show_screen`).

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, NSObject};
use objc2::{AnyThread, DefinedClass, MainThreadMarker, define_class, msg_send, sel};
use objc2_app_kit::{
    NSBitmapImageFileType, NSBitmapImageRep, NSImage, NSPrintInfo, NSPrintJobSavingURL, NSPrintOperation,
    NSPrintSaveJob, NSPrintingPaginationMode, NSScreen, NSWindow,
};
use objc2_core_foundation::{CFData, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGBitmapContextCreateImage, CGColorSpace, CGContext, CGDataProvider, CGImageAlphaInfo,
    CGInterpolationQuality, CGPDFBox, CGPDFDocument, CGPDFPage, kCGColorSpaceSRGB,
};
use objc2_foundation::{NSDictionary, NSError, NSNumber, NSPoint, NSRect, NSSize, NSString, NSURL};
use objc2_web_kit::{WKSnapshotConfiguration, WKWebView};
use std::ffi::c_void;
use std::path::Path;
use std::sync::Mutex;
use std::sync::mpsc::{Sender, channel};
use std::time::Duration;
use tauri::WebviewWindow;

/// The WKWebView of the window, on the main thread.
fn with_webkit(
    window: &WebviewWindow,
    f: impl FnOnce(&WKWebView, &NSWindow, MainThreadMarker) + Send + 'static,
) -> bool {
    let w = window.clone();
    window
        .with_webview(move |wv| {
            let mtm = MainThreadMarker::new().expect("the main thread");
            let web = unsafe { (wv.inner() as *const WKWebView).as_ref() };
            let ns = w.ns_window().ok().and_then(|p| unsafe { (p as *const NSWindow).as_ref() });
            if let (Some(web), Some(ns)) = (web, ns) {
                f(web, ns, mtm);
            }
        })
        .is_ok()
}

/// The page's print layout printed to `temp`, on sheets `width` × `height` inches with no margins.
pub fn print_to(window: &WebviewWindow, temp: &Path, width: f64, height: f64) -> Result<(), String> {
    let _ = std::fs::remove_file(temp);
    let url = temp.to_string_lossy().into_owned();
    let (tx, rx) = channel::<bool>();
    let started = with_webkit(window, move |web, ns, mtm| {
        // the slides' backgrounds and colours as the window shows them, as WebView2 prints them on Windows
        // (ShouldPrintBackgrounds): without it WebKit leaves out every CSS background and darkens light text for white paper
        unsafe { web.configuration().preferences().setShouldPrintBackgrounds(true) };
        let info = NSPrintInfo::init(NSPrintInfo::alloc());
        info.setPaperSize(NSSize::new(width * 72.0, height * 72.0));
        info.setTopMargin(0.0);
        info.setBottomMargin(0.0);
        info.setLeftMargin(0.0);
        info.setRightMargin(0.0);
        info.setHorizontalPagination(NSPrintingPaginationMode::Clip);
        info.setVerticalPagination(NSPrintingPaginationMode::Automatic);
        info.setHorizontallyCentered(false);
        info.setVerticallyCentered(false);
        unsafe {
            info.setJobDisposition(NSPrintSaveJob);
            let file = NSURL::fileURLWithPath(&NSString::from_str(&url));
            info.dictionary().insert(NSPrintJobSavingURL, &*file);
        }
        let op: Retained<NSPrintOperation> = unsafe { web.printOperationWithPrintInfo(&info) };
        op.setShowsPrintPanel(false);
        op.setShowsProgressPanel(false);
        // WKWebView's printing view is laid out at the web view's size; without this it prints nothing
        if let Some(view) = op.view() {
            view.setFrame(web.bounds());
        }
        // the operation runs on the main thread's loop; its file is there, growing, before it ends: it says when it has
        // ended (`Printed`), and only then is the file whole
        let printed = Printed::new(tx);
        let delegate: &AnyObject = &printed;
        unsafe {
            op.runOperationModalForWindow_delegate_didRunSelector_contextInfo(
                ns,
                Some(delegate),
                Some(sel!(printOperationDidRun:success:contextInfo:)),
                std::ptr::null_mut(),
            );
        }
        PRINTED.with(|p| *p.borrow_mut() = Some(printed));
        let _ = mtm;
    });
    if !started {
        return Err("no web view".into());
    }
    match rx.recv_timeout(Duration::from_secs(300)) {
        Ok(true) if temp.is_file() => Ok(()),
        Ok(_) => Err("the print operation wrote nothing".into()),
        Err(_) => Err("slow".into()),
    }
}

/// What the print operation calls when it has ended (`printOperationDidRun:success:contextInfo:`).
struct PrintedIvars {
    done: Mutex<Option<Sender<bool>>>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and Printed has no Drop of its own.
    #[unsafe(super(NSObject))]
    #[name = "DotAnySlidesPrinted"]
    #[ivars = PrintedIvars]
    struct Printed;

    impl Printed {
        #[unsafe(method(printOperationDidRun:success:contextInfo:))]
        fn did_run(&self, _operation: *mut NSPrintOperation, success: Bool, _info: *mut c_void) {
            if let Some(done) = dotany_base::lock(&self.ivars().done).take() {
                let _ = done.send(success.as_bool());
            }
        }
    }
);

impl Printed {
    fn new(done: Sender<bool>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(PrintedIvars { done: Mutex::new(Some(done)) });
        unsafe { msg_send![super(this), init] }
    }
}

thread_local! {
    // the operation doesn't keep its delegate: the last one stays here, on the main thread, until the next print
    static PRINTED: std::cell::RefCell<Option<Retained<Printed>>> = const { std::cell::RefCell::new(None) };
}

/// Every sheet of a PDF the print operation wrote, as a PNG `pixels` wide: the `width` × `height` points at the top of
/// the sheet, drawn by CoreGraphics on white, in sRGB as a canvas of the page's own would be.
pub fn pdf_pictures(pdf: &[u8], width: f64, height: f64, pixels: f64) -> Result<Vec<Vec<u8>>, String> {
    let data = CFData::from_buffer(pdf);
    let provider = CGDataProvider::with_cf_data(Some(&data)).ok_or("no pdf")?;
    let doc = CGPDFDocument::with_provider(Some(&provider)).ok_or("not a pdf")?;
    let srgb = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB })).ok_or("no sRGB")?;
    let k = pixels / width;
    let (w, h) = (pixels.round().max(1.0) as usize, (height * k).round().max(1.0) as usize);
    let mut out = Vec::new();
    for n in 1..=CGPDFDocument::number_of_pages(Some(&doc)) {
        let page = CGPDFDocument::page(Some(&doc), n).ok_or("no page")?;
        let sheet = CGPDFPage::box_rect(Some(&page), CGPDFBox::MediaBox);
        let bitmap = unsafe {
            CGBitmapContextCreate(std::ptr::null_mut(), w, h, 8, 0, Some(&srgb), CGImageAlphaInfo::PremultipliedLast.0)
        }
        .ok_or("no bitmap")?;
        let c = Some(&*bitmap);
        CGContext::set_rgb_fill_color(c, 1.0, 1.0, 1.0, 1.0);
        CGContext::fill_rect(c, CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(w as f64, h as f64)));
        CGContext::set_interpolation_quality(c, CGInterpolationQuality::High);
        // a PDF's y goes up from the bottom of the sheet: the sheet's top edge is the picture's top row
        CGContext::translate_ctm(c, 0.0, h as f64);
        CGContext::scale_ctm(c, k, k);
        CGContext::translate_ctm(c, -sheet.origin.x, -(sheet.origin.y + sheet.size.height));
        CGContext::draw_pdf_page(c, Some(&page));
        let image = CGBitmapContextCreateImage(c).ok_or("no image")?;
        let rep = NSBitmapImageRep::initWithCGImage(NSBitmapImageRep::alloc(), &image);
        let png = unsafe { rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new()) }
            .ok_or("no png")?;
        out.push(png.to_vec());
    }
    Ok(out)
}

/// A picture of part of the page, as WebKit draws it, `scale` pixels to the page's point; the PNG, base64.
pub fn capture(window: &WebviewWindow, x: f64, y: f64, width: f64, height: f64, scale: f64) -> Result<String, String> {
    let (tx, rx) = channel::<Option<Vec<u8>>>();
    // WebKit draws at the screen's own scale: asked for a width in points that comes out at `scale` per point
    let screen = window.scale_factor().unwrap_or(2.0).max(1.0);
    let started = with_webkit(window, move |web, _, mtm| {
        let config = unsafe { WKSnapshotConfiguration::new(mtm) };
        unsafe {
            config.setRect(NSRect::new(NSPoint::new(x, y), NSSize::new(width, height)));
            config.setSnapshotWidth(Some(&NSNumber::new_f64(width * scale / screen)));
            config.setAfterScreenUpdates(true);
        }
        let tx = Mutex::new(Some(tx));
        let done = RcBlock::new(move |image: *mut NSImage, _error: *mut NSError| {
            let Some(tx) = dotany_base::lock(&tx).take() else { return };
            let png = unsafe { image.as_ref() }
                .and_then(|i| i.TIFFRepresentation())
                .and_then(|tiff| NSBitmapImageRep::imageRepWithData(&tiff))
                .and_then(|rep| unsafe {
                    rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new())
                })
                .map(|data| data.to_vec());
            let _ = tx.send(png);
        });
        unsafe { web.takeSnapshotWithConfiguration_completionHandler(Some(&config), &done) };
    });
    if !started {
        return Err("no web view".into());
    }
    let png = rx.recv_timeout(Duration::from_secs(60)).map_err(|_| "slow".to_string())?.ok_or("no picture")?;
    Ok(base64(&png))
}

fn base64(data: &[u8]) -> String {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            out.push(if i <= c.len() { A[(n >> (18 - 6 * i) & 63) as usize] as char } else { '=' });
        }
    }
    out
}

/// Why a film or sound in a slide isn't played on the Mac.
#[derive(Debug, PartialEq)]
pub enum NotPlayed {
    /// WMV, WMA: nothing on a Mac plays them.
    Windows,
    /// The system has nothing for what's inside, or it is damaged.
    Cannot,
}

/// A film in a slide that WKWebView won't play, repackaged for it: the new film's bytes, and its type.
pub fn repack(bytes: &[u8], ext: &str, temp: &Path) -> Result<(Vec<u8>, &'static str), NotPlayed> {
    if matches!(ext, "wmv" | "wma" | "asf") {
        return Err(NotPlayed::Windows);
    }
    std::fs::write(temp, bytes).map_err(|_| NotPlayed::Cannot)?;
    let made = (|| {
        let r = dotany_repack::repack(temp).ok()?;
        let mut f = std::fs::File::open(temp).ok()?;
        Some((r.to_vec(&mut f).ok()?, r.mime()))
    })();
    let _ = std::fs::remove_file(temp);
    made.ok_or(NotPlayed::Cannot)
}

// ---------- the show on a second screen ----------

/// A screen as the show's placement sees it: AppKit's frame (points, from the bottom left of the main screen).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Screen {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    /// The Mac's own display (a MacBook's), not one plugged in.
    pub builtin: bool,
    /// The one with the menu bar.
    pub main: bool,
}

/// Where the show goes with the Mac's displays extended: on a screen the audience sees (one that
/// isn't the Mac's own, the largest if there are several), the presenter's own screen left to the presenter. With one
/// screen, or when every screen is the Mac's own, the show stays on the screen the window is on (`None`). With only
/// screens plugged in, the show goes to the largest that hasn't the menu bar.
pub fn show_screen(screens: &[Screen]) -> Option<usize> {
    if screens.len() < 2 {
        return None;
    }
    let area = |s: &Screen| s.w * s.h;
    let pick = |ok: &dyn Fn(&Screen) -> bool| {
        screens
            .iter()
            .enumerate()
            .filter(|(_, s)| ok(s))
            .max_by(|a, b| area(a.1).total_cmp(&area(b.1)).then(b.0.cmp(&a.0)))
            .map(|(i, _)| i)
    };
    if screens.iter().any(|s| s.builtin) { pick(&|s| !s.builtin) } else { pick(&|s| !s.main) }
}

/// With more than one screen, where the show goes and where the presenter's view goes: the show
/// on `show_screen`'s choice, the presenter's view on the Mac's own screen; with none of its own (a Mac mini, a MacBook
/// closed), on the one with the menu bar; if that is the show's too, the largest other one. `None` with one screen: the
/// show and its presenter's view share the window, as on Windows.
pub fn show_screens(screens: &[Screen]) -> Option<(usize, usize)> {
    let show = show_screen(screens)?;
    let others = || screens.iter().enumerate().filter(move |(i, _)| *i != show);
    let presenter = others()
        .find(|(_, s)| s.builtin)
        .or_else(|| others().find(|(_, s)| s.main))
        .or_else(|| others().max_by(|a, b| (a.1.w * a.1.h).total_cmp(&(b.1.w * b.1.h)).then(b.0.cmp(&a.0))))
        .map(|(i, _)| i)?;
    Some((show, presenter))
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGDisplayIsBuiltin(display: u32) -> u32;
}

fn screens(mtm: MainThreadMarker) -> Vec<Screen> {
    let list = NSScreen::screens(mtm);
    let main = NSScreen::mainScreen(mtm).map(|m| m.frame());
    let mut found: Vec<Screen> = list
        .iter()
        .map(|s| {
            let f = s.frame();
            let number = s
                .deviceDescription()
                .objectForKey(&NSString::from_str("NSScreenNumber"))
                .and_then(|n| n.downcast::<NSNumber>().ok())
                .map_or(0, |n| n.unsignedIntValue());
            Screen {
                x: f.origin.x,
                y: f.origin.y,
                w: f.size.width,
                h: f.size.height,
                builtin: unsafe { CGDisplayIsBuiltin(number) } != 0,
                main: main.is_some_and(|m| m.origin.x == f.origin.x && m.origin.y == f.origin.y),
            }
        })
        .collect();
    // with DOTANY_TEST set and DOTANY_TEST_SCREENS=two, a Mac with one screen gets a made-up second screen plugged in,
    // where the first one is, so the two-window show runs (both windows in full screen on the one real screen, each in
    // a space of its own); only where the windows go is left unproved
    let imitated =
        std::env::var_os("DOTANY_TEST").is_some() && std::env::var("DOTANY_TEST_SCREENS").is_ok_and(|v| v == "two");
    if let Some(first) = found.first().copied().filter(|_| imitated) {
        found.push(Screen { builtin: false, main: false, ..first });
    }
    found
}

/// Before the show goes full screen: the window moves to the screen the show belongs on, if that isn't where it is.
/// Answers where it was, so the end of the show brings it back.
pub fn move_for_show(window: &WebviewWindow) -> Option<NSRect> {
    let (tx, rx) = channel::<Option<NSRect>>();
    let w = window.clone();
    window
        .run_on_main_thread(move || {
            let mtm = MainThreadMarker::new().expect("the main thread");
            let ns = w.ns_window().ok().and_then(|p| unsafe { (p as *const NSWindow).as_ref() });
            let list = screens(mtm);
            let moved = ns.zip(show_screen(&list)).and_then(|(ns, i)| {
                let to = list[i];
                let was = ns.frame();
                let inside = was.origin.x >= to.x
                    && was.origin.x < to.x + to.w
                    && was.origin.y >= to.y
                    && was.origin.y < to.y + to.h;
                if inside {
                    return None;
                }
                let size = NSSize::new(was.size.width.min(to.w), was.size.height.min(to.h));
                let at = NSPoint::new(to.x + (to.w - size.width) / 2.0, to.y + (to.h - size.height) / 2.0);
                ns.setFrame_display(NSRect::new(at, size), true);
                Some(was)
            });
            let _ = tx.send(moved);
        })
        .ok()?;
    rx.recv_timeout(Duration::from_secs(5)).ok().flatten()
}

// ---------- the presenter's view on a screen of its own ----------

/// A presenter's window is its document window's label and this (`main-2-presenter`): it belongs to that window.
pub const PRESENTER: &str = "-presenter";

/// The document window a presenter's window belongs to.
pub fn owner_of(label: &str) -> Option<&str> {
    label.strip_suffix(PRESENTER).filter(|o| !o.is_empty())
}

/// With more than one screen: a window of the same program for the presenter's view of `window`'s show, full screen on
/// the screen `show_screens` gives it, beside the show on the other (two windows, each in the full screen
/// of its own screen, which "Displays have separate Spaces", the Mac's default, allows). Its page is this program's page,
/// told whose presenter's view it is (main.js `presenterOf`); everything it shows comes from the show's page. False with
/// one screen: the show keeps its presenter's view in its own window, as on Windows. Not on the main thread.
pub fn open_presenter(window: &WebviewWindow) -> bool {
    use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};
    let app = window.app_handle();
    let label = format!("{}{PRESENTER}", window.label());
    if app.get_webview_window(&label).is_some() {
        return true;
    }
    let (tx, rx) = channel::<Option<Screen>>();
    let asked = window.run_on_main_thread(move || {
        let mtm = MainThreadMarker::new().expect("the main thread");
        let list = screens(mtm);
        let _ = tx.send(show_screens(&list).map(|(_, p)| list[p]));
    });
    if asked.is_err() {
        return false;
    }
    let Some(to) = rx.recv_timeout(Duration::from_secs(5)).ok().flatten() else { return false };
    let boot = serde_json::json!({ "settings": {}, "presenterOf": window.label() });
    let built = WebviewWindowBuilder::new(app, &label, WebviewUrl::App("index.html".into()))
        .title(window.title().unwrap_or_default())
        .visible(false)
        .background_color(tauri::window::Color(0, 0, 0, 255))
        .initialization_script(format!("window.__DOTANY__={boot};{}", dotany_base::test_hook()))
        .build();
    let Ok(presenter) = built else { return false };
    // on its screen first: a window goes full screen on the screen it is on
    let p = presenter.clone();
    let (tx, rx) = channel::<()>();
    let _ = presenter.run_on_main_thread(move || {
        if let Some(ns) = p.ns_window().ok().and_then(|n| unsafe { (n as *const NSWindow).as_ref() }) {
            let size = NSSize::new((to.w * 0.8).min(1280.0), (to.h * 0.8).min(800.0));
            let at = NSPoint::new(to.x + (to.w - size.width) / 2.0, to.y + (to.h - size.height) / 2.0);
            ns.setFrame_display(NSRect::new(at, size), true);
            // the Window menu lists the documents; this window is part of one, not one of its own
            ns.setExcludedFromWindowsMenu(true);
        }
        let _ = tx.send(());
    });
    let _ = rx.recv_timeout(Duration::from_secs(5));
    let _ = presenter.show();
    // one full-screen move at a time: AppKit lets a second wait or drop while the show's window is still on its way
    let settled = |w: &WebviewWindow, full: bool| {
        for _ in 0..40 {
            if w.is_fullscreen().unwrap_or(!full) == full {
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        false
    };
    settled(window, true);
    // the green button's own action, on the window in front: AppKit drops it now and then while another full-screen
    // move is under way (seen with both windows on one screen), so it is asked again until the window is there.
    // Tauri's `set_fullscreen` can't ask again: it takes the first ask as done
    for _ in 0..4 {
        let p = presenter.clone();
        let _ = presenter.run_on_main_thread(move || {
            if let Some(ns) = p.ns_window().ok().and_then(|n| unsafe { (n as *const NSWindow).as_ref() }) {
                if !ns.styleMask().contains(objc2_app_kit::NSWindowStyleMask::FullScreen) {
                    ns.makeKeyAndOrderFront(None);
                    ns.toggleFullScreen(None);
                }
            }
        });
        if settled(&presenter, true) {
            break;
        }
    }
    // its page takes the keys at once (a presenter's remote is a keyboard), without a click first
    let _ = presenter.with_webview(|wv| {
        let web = unsafe { (wv.inner() as *const objc2_app_kit::NSView).as_ref() };
        let ns = unsafe { (wv.ns_window() as *const NSWindow).as_ref() };
        if let (Some(web), Some(ns)) = (web, ns) {
            ns.makeFirstResponder(Some(&**web));
        }
    });
    true
}

/// The show's presenter's window, if it has one, goes (the show ended, or its window went).
pub fn close_presenter(app: &tauri::AppHandle, owner: &str) {
    use tauri::Manager;
    if let Some(w) = app.get_webview_window(&format!("{owner}{PRESENTER}")) {
        let _ = w.destroy();
    }
}

/// After the show: the window back where it was.
pub fn move_back(window: &WebviewWindow, was: NSRect) {
    let w = window.clone();
    let _ = window.run_on_main_thread(move || {
        if let Some(ns) = w.ns_window().ok().and_then(|p| unsafe { (p as *const NSWindow).as_ref() }) {
            ns.setFrame_display(was, true);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(w: f64, h: f64, builtin: bool, main: bool) -> Screen {
        Screen { x: 0.0, y: 0.0, w, h, builtin, main }
    }

    #[test]
    fn one_screen_keeps_the_show_where_it_is() {
        assert_eq!(show_screen(&[]), None);
        assert_eq!(show_screen(&[screen(1470.0, 956.0, true, true)]), None);
    }

    #[test]
    fn a_projector_beside_a_macbook_gets_the_show() {
        let s = [screen(1470.0, 956.0, true, true), screen(1920.0, 1080.0, false, false)];
        assert_eq!(show_screen(&s), Some(1));
        // the projector made the main screen (the menu bar dragged there): still the projector
        let s = [screen(1470.0, 956.0, true, false), screen(1280.0, 720.0, false, true)];
        assert_eq!(show_screen(&s), Some(1));
        // listed first, smaller than the MacBook's own
        let s = [screen(1024.0, 768.0, false, false), screen(1470.0, 956.0, true, true)];
        assert_eq!(show_screen(&s), Some(0));
    }

    #[test]
    fn two_screens_plugged_in_the_larger_gets_it() {
        let s = [
            screen(1470.0, 956.0, true, true),
            screen(1920.0, 1080.0, false, false),
            screen(3840.0, 2160.0, false, false),
        ];
        assert_eq!(show_screen(&s), Some(2));
        let s = [
            screen(1920.0, 1080.0, false, false),
            screen(1920.0, 1080.0, false, false),
            screen(1470.0, 956.0, true, true),
        ];
        assert_eq!(show_screen(&s), Some(0));
    }

    #[test]
    fn a_mac_without_its_own_screen_shows_on_the_one_without_the_menu_bar() {
        let s = [screen(2560.0, 1440.0, false, true), screen(1920.0, 1080.0, false, false)];
        assert_eq!(show_screen(&s), Some(1));
    }

    #[test]
    fn the_presenter_keeps_the_macs_own_screen() {
        // one screen: the show keeps its presenter's view in its own window
        assert_eq!(show_screens(&[screen(1470.0, 956.0, true, true)]), None);
        // a MacBook and a projector: the show there, the presenter's view on the MacBook
        let s = [screen(1470.0, 956.0, true, true), screen(1920.0, 1080.0, false, false)];
        assert_eq!(show_screens(&s), Some((1, 0)));
        // the projector made the main screen: the same
        let s = [screen(1470.0, 956.0, true, false), screen(1280.0, 720.0, false, true)];
        assert_eq!(show_screens(&s), Some((1, 0)));
        // two plugged in beside the MacBook: the show on the larger, the presenter on the MacBook, not on the other
        let s = [
            screen(1920.0, 1080.0, false, false),
            screen(1470.0, 956.0, true, true),
            screen(3840.0, 2160.0, false, false),
        ];
        assert_eq!(show_screens(&s), Some((2, 1)));
    }

    #[test]
    fn without_its_own_screen_the_presenter_gets_the_menu_bar() {
        let s = [screen(2560.0, 1440.0, false, true), screen(1920.0, 1080.0, false, false)];
        assert_eq!(show_screens(&s), Some((1, 0)));
        let s = [
            screen(1280.0, 720.0, false, false),
            screen(2560.0, 1440.0, false, false),
            screen(1920.0, 1080.0, false, true),
        ];
        assert_eq!(show_screens(&s), Some((1, 2)));
    }

    #[test]
    fn a_presenters_window_knows_whose_it_is() {
        assert_eq!(owner_of("main-presenter"), Some("main"));
        assert_eq!(owner_of("main-3-presenter"), Some("main-3"));
        assert_eq!(owner_of("main-3"), None);
        assert_eq!(owner_of("-presenter"), None);
    }

    #[test]
    fn base64_as_the_page_reads_it() {
        assert_eq!(base64(b"Man"), "TWFu");
        assert_eq!(base64(b"Ma"), "TWE=");
        assert_eq!(base64(b"M"), "TQ==");
    }
}
