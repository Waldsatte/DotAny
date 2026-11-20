//! Print… on the Mac: the window's page printed by WKWebView through the system's print panel, as a
//! sheet on the window, with what WebView2 puts on paper on Windows.
//!
//! Not Tauri's `window.print()` (wry), which prints differently:
//! - it leaves out every CSS background (WebKit's default; WebView2 prints them), so Text's list bullets, drawn as small
//!   squares, would be missing. The base turns them on, as Slides' own PDF does (slides/src-tauri/src/mac.rs);
//! - it sets the program-wide paper's margins to nothing, and WebKit lays the page out 1.25 times as wide as the paper
//!   inside those margins, then shrinks that into the box the page's `@page` margins leave: so the size on paper
//!   depended on the margins, not on the page. The base reads the page's `@page` margins first and gives the print the
//!   margins that make WebKit lay the page out at WebView2's 96 dots to the inch: the type at the size the page asks, a
//!   page as full as on Windows, the `@page` margins on the paper;
//! - CSS's `vh` is the window's height in WebKit's print, where WebView2 takes the page's: the base puts the height
//!   of the page's printed area in `--print-h` (CSS px) for a page that needs it (Image's picture fits the sheet);
//! - WebKit prints a CSS transition where it is at that moment, also one the print's own rules would end: with the
//!   backgrounds on, the family's body, still easing to another theme's colour or back from the last print's white,
//!   came out as a grey box behind Text's words and Image's picture. Before it prints, the base brings every easing
//!   on the page to its end, and in print nothing eases on the Mac (the base's print rule, `NO_EASING`).
//!
//! The paper and its orientation are the system panel's (a person's choices there kept for the next print, as in every
//! Mac program); the page's own `@media print` rules say what is on the paper.
//!
//! In a test the panel can't wait for a person: the driver (`DotAny.printTo`) has the next print go to a PDF file
//! without the panel, the same operation otherwise, and `DotAny.printed` says when it has ended (the file is there,
//! growing, before that).

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, NSObject};
use objc2::{AnyThread, Message, define_class, msg_send, sel};
use objc2_app_kit::{NSPrintInfo, NSPrintJobSavingURL, NSPrintOperation, NSPrintSaveJob, NSWindow};
use objc2_foundation::{NSError, NSString, NSURL};
use objc2_web_kit::WKWebView;
use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::WebviewWindow;

/// A test's next print: to this file, without the panel.
static TO_FILE: Mutex<Option<PathBuf>> = Mutex::new(None);
/// How the last print ended: None while it runs.
static ENDED: Mutex<Option<bool>> = Mutex::new(None);

/// The test driver: the next print goes to `path` (a PDF) without the panel.
pub fn print_next_to(path: PathBuf) {
    *crate::lock(&ENDED) = None;
    *crate::lock(&TO_FILE) = Some(path);
}

/// The test driver: whether the last print has ended, and well.
pub fn ended() -> Option<bool> {
    *crate::lock(&ENDED)
}

/// The base's print rule, put in the page before it prints: what is on paper is where it ends, not on its way there.
const NO_EASING: &str =
    "@media print { *, *::before, *::after { transition: none !important; animation: none !important; } }";

/// The page's `@page` margins in points (top, right, bottom, left), from its print rules, given the paper's size in
/// points; it also keeps the height of the printed area in `--print-h` (CSS px, 96 to the inch), adds the base's print
/// rule (`NO_EASING`) and ends the easings under way (a looping one, the caret's blink, can't end and goes on). 'null'
/// without margins (a string either way: what WebKit hands back for a page's null is not one).
const PAGE_MARGINS: &str = r#"((w, h, rule) => {
  if (!document.getElementById('dotany-print')) {
    const s = document.createElement('style');
    s.id = 'dotany-print';
    s.textContent = rule;
    document.head.appendChild(s);
  }
  for (const a of document.getAnimations()) try { a.finish(); } catch (e) {}
  const pt = (v) => {
    const m = /^(-?\d*\.?\d+)(px|pt|mm|cm|in|q)?$/i.exec(String(v).trim());
    return m ? m[1] * { px: 0.75, pt: 1, mm: 72 / 25.4, cm: 72 / 2.54, in: 72, q: 18 / 25.4 }[(m[2] || 'px').toLowerCase()] : null;
  };
  let page = null;
  const walk = (rules) => {
    for (const r of rules)
      if (r.type === 6) page = r;
      else if (r.cssRules && !(r.media && ![...r.media].some((m) => /^(all|print)$/.test(m.trim())))) walk(r.cssRules);
  };
  for (const s of document.styleSheets) try { walk(s.cssRules); } catch (e) {}
  const m = page ? ['top', 'right', 'bottom', 'left'].map((k) => pt(page.style.getPropertyValue('margin-' + k))) : [];
  if (m.length !== 4 || m.some((v) => v === null)) return 'null';
  document.documentElement.style.setProperty('--print-h', (h - m[0] - m[2]) / 0.75 + 'px');
  return JSON.stringify(m);
})"#;

/// Print…: the system's print panel on the window, then the page on paper.
pub fn print(window: &WebviewWindow) {
    let to_file = crate::lock(&TO_FILE).take();
    let _ = window.with_webview(move |wv| {
        let Some(web) = (unsafe { (wv.inner() as *const WKWebView).as_ref() }) else { return };
        let Some(ns) = web.window() else { return };
        let paper = NSPrintInfo::sharedPrintInfo().paperSize();
        let web2 = web.retain();
        let asked = RcBlock::new(move |value: *mut AnyObject, _error: *mut NSError| {
            let margins = unsafe { (value as *const NSString).as_ref() }
                .and_then(|s| serde_json::from_str::<[f64; 4]>(&s.to_string()).ok());
            run(&web2, &ns, margins, to_file.clone());
        });
        let script = format!("{PAGE_MARGINS}({}, {}, {:?})", paper.width, paper.height, NO_EASING);
        unsafe { web.evaluateJavaScript_completionHandler(&NSString::from_str(&script), Some(&asked)) };
    });
}

/// The print operation itself, on the main thread, the page's `@page` margins known (or not).
fn run(web: &WKWebView, ns: &NSWindow, margins: Option<[f64; 4]>, to_file: Option<PathBuf>) {
    // the page's backgrounds and colours on paper, as WebView2 prints them (ShouldPrintBackgrounds): without it
    // WebKit leaves out every CSS background and darkens light text for white paper
    unsafe { web.configuration().preferences().setShouldPrintBackgrounds(true) };
    let shared = NSPrintInfo::sharedPrintInfo();
    let info = match &to_file {
        // a test's file: a copy, so the program's own print info isn't sent to a file
        Some(path) => {
            let info = unsafe { NSPrintInfo::initWithDictionary(NSPrintInfo::alloc(), &shared.dictionary()) };
            unsafe {
                info.setJobDisposition(NSPrintSaveJob);
                let file = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
                info.dictionary().insert(NSPrintJobSavingURL, &*file);
            }
            info
        }
        None => shared,
    };
    // WebKit lays the page out at 1.25 CSS px to each point of the paper inside the print's margins, then scales it
    // into the box the `@page` margins leave: that paper 16/15 of the box gives 0.75 points to the px, WebView2's 96
    // dots to the inch. Centred both ways, the page's own margins are what is left on each side
    if let Some([top, right, bottom, left]) = margins {
        let paper = info.paperSize();
        let across = ((paper.width - (paper.width - left - right) * 16.0 / 15.0) / 2.0).max(0.0);
        let down = ((paper.height - (paper.height - top - bottom) * 16.0 / 15.0) / 2.0).max(0.0);
        info.setLeftMargin(across);
        info.setRightMargin(across);
        info.setTopMargin(down);
        info.setBottomMargin(down);
        info.setHorizontallyCentered(true);
        info.setVerticallyCentered(true);
    }
    let op: Retained<NSPrintOperation> = unsafe { web.printOperationWithPrintInfo(&info) };
    op.setShowsPrintPanel(to_file.is_none());
    op.setShowsProgressPanel(to_file.is_none());
    // WKWebView's printing view is laid out at the web view's size
    if let Some(view) = op.view() {
        view.setFrame(web.bounds());
    }
    *crate::lock(&ENDED) = None;
    let printed = Printed::new();
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
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements, and Printed has no Drop of its own.
    #[unsafe(super(NSObject))]
    #[name = "DotAnyBasePrinted"]
    struct Printed;

    impl Printed {
        /// What the print operation calls when it has ended (printed, saved, or the panel's Cancel).
        #[unsafe(method(printOperationDidRun:success:contextInfo:))]
        fn did_run(&self, _operation: *mut NSPrintOperation, success: Bool, _info: *mut c_void) {
            *crate::lock(&ENDED) = Some(success.as_bool());
        }
    }
);

impl Printed {
    fn new() -> Retained<Self> {
        let this = Self::alloc().set_ivars(());
        unsafe { msg_send![super(this), init] }
    }
}

thread_local! {
    // the operation doesn't keep its delegate: the last one stays here, on the main thread, until the next print
    static PRINTED: std::cell::RefCell<Option<Retained<Printed>>> = const { std::cell::RefCell::new(None) };
}
