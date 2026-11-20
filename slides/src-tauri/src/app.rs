//! The Tauri side of DotAny Slides: what makes this tool itself, reading a presentation's bytes and writing what the page
//! made, a PDF printed by WebView2 itself, a picture of a slide, and the films and sounds inside a slide that WebView2
//! won't play (WMV, AVI, WMA), played by Windows. Reading and writing the presentation itself happens in the page
//! (deck.js, edit.js); everything else is dotany-base. On the Mac WKWebView prints the PDF and takes the pictures
//! (`mac.rs`), an AVI is repackaged into a film WKWebView plays, and WMV and WMA are not played.
//!
//! Slides keeps a file per window (`dotany_base::builder_per_window`): on the Mac one program shows each presentation in
//! a window of its own; on Windows each presentation is a program of its own, with one window. The deck, its pictures,
//! films and sounds, and a running show with its presenter's view all live in the window's page; what this side holds
//! for a window is the films and sounds Windows is playing in it (`Media`). A show is the window's own: it fills that
//! window's screen (`set_full`), and on the Mac another window's presentation stays a window of its own, to edit
//! meanwhile. On a Mac with more than one screen the show goes to the other screen and its presenter's view is a window
//! of its own on the Mac's screen (`presenter_open`, `mac::open_presenter`), which belongs to the show's window.

use dotany_base::RunFamily;
use dotany_base::atomic::{WriteFailure, atomic_write};
use dotany_base::{Base, FileWindows, PerWindow, Tool, file_name, lock};
use serde::Serialize;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
#[cfg(windows)]
use std::sync::mpsc::channel;
#[cfg(windows)]
use std::time::Duration;
use tauri::ipc::{InvokeBody, Request, Response};
use tauri::{AppHandle, Manager, State, WebviewWindow};

#[cfg(windows)]
use crate::media_foundation as mf;
#[cfg(target_os = "macos")]
use crate::media_macos as mf;

/// A presentation is a ZIP. A compound file is let through too: that's a pptx with a password, which the page names as such.
fn looks_like_a_deck(path: &Path) -> bool {
    let mut head = [0u8; 8];
    let Ok(mut f) = fs::File::open(path) else { return false };
    if f.read_exact(&mut head).is_err() {
        return false;
    }
    head.starts_with(b"PK\x03\x04") || head == [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]
}

/// The window's presentation closed, or the window went: what Windows was playing in it stops.
fn forget(app: &AppHandle, label: &str) {
    // the Mac: a show's presenter's window goes with its document window; when it goes by itself (closed, ⌘W, ⌘Q),
    // the show ends, as Esc ends it
    #[cfg(target_os = "macos")]
    {
        use tauri::Emitter;
        match crate::mac::owner_of(label) {
            Some(owner) => {
                let _ = app.emit_to(owner, "presenter-do", "gone");
                return;
            }
            None => crate::mac::close_presenter(app, label),
        }
    }
    let Some(media) = app.state::<Medias>().remove(label) else { return };
    let gone: Vec<Playing> = lock(&media.playing).drain(..).collect();
    for p in gone {
        p.close(app, label);
    }
}

/// Slides keeps a file per window and says so in `FileWindows` (`forget`); this is never called.
fn nothing(_: &AppHandle) {}

pub static TOOL: Tool = Tool {
    name: "DotAny Slides",
    folder: "Slides",
    data_env: "DOTANY_SLIDES_DATA",
    user_dir: "Documents",
    opens_files: true,
    // pptm, ppsm and potm are only read: the window is read-only and a save is a new pptx. The old binary ppt isn't
    // opened at all: no code that reads it is compiled in
    extensions: &["pptx", "ppsx", "potx", "pptm", "ppsm", "potm"],
    sniff: looks_like_a_deck,
    size: (1180.0, 800.0),
    min_size: (480.0, 420.0),
    on_close: nothing,
};

/// The presentation's bytes, whole, straight to the page. Answers "missing", "refused" or "damaged" when it can't.
#[tauri::command(async)]
fn read_deck(path: String) -> Result<Response, String> {
    let path = std::path::absolute(&path).unwrap_or_else(|_| PathBuf::from(&path));
    if !path.is_file() {
        return Err("missing".into());
    }
    if !TOOL.allowed_name(&file_name(&path)) {
        return Err("refused".into());
    }
    let bytes = fs::read(&path).map_err(|_| "damaged".to_string())?;
    Ok(Response::new(bytes))
}

#[derive(Serialize)]
struct Opened {
    status: &'static str,
    path: String,
    dir: String,
    name: String,
    size: u64,
}

/// The page has read the presentation: it's now the open file, watched, and first in the recent list.
#[tauri::command(async)]
fn adopt_deck(window: WebviewWindow, path: String) -> Opened {
    let path = std::path::absolute(&path).unwrap_or_else(|_| PathBuf::from(&path));
    dotany_base::opened_in(window.app_handle(), window.label(), &path);
    Opened {
        status: "ok",
        dir: path.parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default(),
        name: file_name(&path),
        size: fs::metadata(&path).map(|m| m.len()).unwrap_or(0),
        path: path.to_string_lossy().into_owned(),
    }
}

fn unescape(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        let hex = |c: u8| (c as char).to_digit(16).map(|d| d as u8);
        if b[i] == b'%' && i + 2 < b.len() {
            if let (Some(h), Some(l)) = (hex(b[i + 1]), hex(b[i + 2])) {
                out.push(h * 16 + l);
                i += 3;
                continue;
            }
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
enum Saved {
    Saved { path: String, name: String, size: u64 },
    Failed { kind: WriteFailure, detail: String },
}

fn header(request: &Request, k: &str) -> Option<String> {
    request.headers().get(k).and_then(|v| v.to_str().ok()).map(unescape)
}

/// The bytes the page made, written whole to the path in the `path` header. A PDF or a picture isn't a file this tool
/// opens, so it doesn't go on the recent list (header `export: 1`).
#[tauri::command(async)]
fn save_bytes(window: WebviewWindow, request: Request) -> Saved {
    let Some(path) = header(&request, "path") else {
        return Saved::Failed { kind: WriteFailure::Other, detail: "no path".into() };
    };
    let InvokeBody::Raw(bytes) = request.body() else {
        return Saved::Failed { kind: WriteFailure::Other, detail: "no bytes".into() };
    };
    let target = PathBuf::from(&path);
    if let Err(e) = atomic_write(&target, bytes) {
        return Saved::Failed { kind: e.kind, detail: e.detail };
    }
    if header(&request, "export").as_deref() != Some("1") {
        dotany_base::saved_in(window.app_handle(), window.label(), &target);
    }
    Saved::Saved { name: file_name(&target), path, size: bytes.len() as u64 }
}

/// A file of this program's own in the system's temporary folder: a PDF on its way, or a film Windows plays.
fn temp_file(ext: &str) -> PathBuf {
    static N: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("dotany-slides-{}", std::process::id()));
    let _ = fs::create_dir_all(&dir);
    dir.join(format!("{n}.{ext}"))
}

/// The page as it is laid out for printing (only the slides chosen, one to a sheet the slide's own size), printed to a PDF
/// by WebView2 and then written to `path` the family's way. `width` and `height` are the sheet's, in inches.
#[tauri::command(async)]
fn print_pdf(window: WebviewWindow, path: String, width: f64, height: f64) -> Saved {
    let temp = temp_file("pdf");
    let printed = print_to(&window, &temp, width, height);
    let result = match printed.and_then(|_| fs::read(&temp).map_err(|e| e.to_string())) {
        Ok(bytes) if bytes.starts_with(b"%PDF") => {
            let target = PathBuf::from(&path);
            match atomic_write(&target, &bytes) {
                Ok(()) => Saved::Saved { name: file_name(&target), path, size: bytes.len() as u64 },
                Err(e) => Saved::Failed { kind: e.kind, detail: e.detail },
            }
        }
        Ok(_) => Saved::Failed { kind: WriteFailure::Other, detail: "not a pdf".into() },
        Err(detail) => Saved::Failed { kind: WriteFailure::Other, detail },
    };
    let _ = fs::remove_file(&temp);
    result
}

#[cfg(windows)]
fn print_to(window: &WebviewWindow, temp: &Path, width: f64, height: f64) -> Result<(), String> {
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_PRINT_ORIENTATION_PORTRAIT, ICoreWebView2_7, ICoreWebView2Environment6,
    };
    use webview2_com::PrintToPdfCompletedHandler;
    use windows::core::{HSTRING, Interface};
    let (tx, rx) = channel::<Result<(), String>>();
    let target = HSTRING::from(temp.as_os_str());
    window
        .with_webview(move |w| {
            let tx2 = tx.clone();
            let started = (|| unsafe {
                let env: ICoreWebView2Environment6 = w.environment().cast().map_err(|e| e.to_string())?;
                let settings = env.CreatePrintSettings().map_err(|e| e.to_string())?;
                settings.SetOrientation(COREWEBVIEW2_PRINT_ORIENTATION_PORTRAIT).map_err(|e| e.to_string())?;
                settings.SetPageWidth(width).map_err(|e| e.to_string())?;
                settings.SetPageHeight(height).map_err(|e| e.to_string())?;
                for set in [
                    ICoreWebView2PrintSettingsMargins::Top,
                    ICoreWebView2PrintSettingsMargins::Bottom,
                    ICoreWebView2PrintSettingsMargins::Left,
                    ICoreWebView2PrintSettingsMargins::Right,
                ] {
                    set.apply(&settings).map_err(|e| e.to_string())?;
                }
                settings.SetScaleFactor(1.0).map_err(|e| e.to_string())?;
                settings.SetShouldPrintBackgrounds(true).map_err(|e| e.to_string())?;
                settings.SetShouldPrintHeaderAndFooter(false).map_err(|e| e.to_string())?;
                settings.SetShouldPrintSelectionOnly(false).map_err(|e| e.to_string())?;
                let core: ICoreWebView2_7 =
                    w.controller().CoreWebView2().map_err(|e| e.to_string())?.cast().map_err(|e| e.to_string())?;
                let handler = PrintToPdfCompletedHandler::create(Box::new(move |res, ok| {
                    let _ = tx2.send(match (res, ok) {
                        (Ok(()), true) => Ok(()),
                        (Err(e), _) => Err(e.to_string()),
                        _ => Err("print failed".into()),
                    });
                    Ok(())
                }));
                core.PrintToPdf(&target, &settings, &handler).map_err(|e| e.to_string())
            })();
            if let Err(e) = started {
                let _ = tx.send(Err(e));
            }
        })
        .map_err(|e| e.to_string())?;
    rx.recv_timeout(Duration::from_secs(300)).map_err(|_| "slow".to_string())?
}

/// On the Mac WKWebView prints the same print layout to the PDF (`mac::print_to`).
#[cfg(target_os = "macos")]
fn print_to(window: &WebviewWindow, temp: &Path, width: f64, height: f64) -> Result<(), String> {
    crate::mac::print_to(window, temp, width, height)
}

/// The Mac: the slides as PNG pictures `pixels` wide, one to each of `paths`, from the page's print layout as `print_pdf`
/// prints it (`mac::pdf_pictures` says why not the page's own SVG picture). `width` and `height` are the slide's, in
/// inches; the page lays each slide on a sheet a pixel taller, so nothing of it is cut off. Answers the first picture's
/// save, or the first failure; rejects when the pictures couldn't be made at all.
#[cfg(target_os = "macos")]
#[tauri::command(async)]
fn print_png(window: WebviewWindow, paths: Vec<String>, width: f64, height: f64, pixels: f64) -> Result<Saved, String> {
    let temp = temp_file("pdf");
    let made = print_to(&window, &temp, width, height + 1.0 / 96.0)
        .and_then(|_| fs::read(&temp).map_err(|e| e.to_string()))
        .and_then(|pdf| crate::mac::pdf_pictures(&pdf, width * 72.0, height * 72.0, pixels));
    let _ = fs::remove_file(&temp);
    let pictures = made?;
    if pictures.len() != paths.len() {
        return Err(format!("{} sheets for {} slides", pictures.len(), paths.len()));
    }
    let mut first = None;
    for (path, png) in paths.into_iter().zip(pictures) {
        let target = PathBuf::from(&path);
        let saved = match atomic_write(&target, &png) {
            Ok(()) => Saved::Saved { name: file_name(&target), path, size: png.len() as u64 },
            Err(e) => return Ok(Saved::Failed { kind: e.kind, detail: e.detail }),
        };
        first.get_or_insert(saved);
    }
    first.ok_or_else(|| "no slides".into())
}

/// The four margins, each zero: the slide fills its sheet.
#[cfg(windows)]
enum ICoreWebView2PrintSettingsMargins {
    Top,
    Bottom,
    Left,
    Right,
}

#[cfg(windows)]
impl ICoreWebView2PrintSettingsMargins {
    unsafe fn apply(
        &self,
        s: &webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2PrintSettings,
    ) -> windows::core::Result<()> {
        match self {
            Self::Top => s.SetMarginTop(0.0),
            Self::Bottom => s.SetMarginBottom(0.0),
            Self::Left => s.SetMarginLeft(0.0),
            Self::Right => s.SetMarginRight(0.0),
        }
    }
}

/// A picture of part of the page, as Chromium itself draws it, at `scale` times its size on the page: the page puts the
/// slide there first. Answers the PNG, base64.
#[tauri::command(async)]
fn capture(window: WebviewWindow, x: f64, y: f64, width: f64, height: f64, scale: f64) -> Result<String, String> {
    // the Mac: WKWebView's own snapshot of the same part of the page (`mac::capture`)
    #[cfg(target_os = "macos")]
    return crate::mac::capture(&window, x, y, width, height, scale);
    #[cfg(windows)]
    {
        let params = serde_json::json!({
            "format": "png",
            "clip": { "x": x, "y": y, "width": width, "height": height, "scale": scale },
            "captureBeyondViewport": true,
            "fromSurface": true,
        })
        .to_string();
        let answer = devtools(&window, "Page.captureScreenshot", &params)?;
        let v: serde_json::Value = serde_json::from_str(&answer).map_err(|e| e.to_string())?;
        v.get("data").and_then(|d| d.as_str()).map(str::to_string).ok_or_else(|| "no picture".into())
    }
}

#[cfg(windows)]
fn devtools(window: &WebviewWindow, method: &str, params: &str) -> Result<String, String> {
    use webview2_com::CallDevToolsProtocolMethodCompletedHandler;
    use windows::core::HSTRING;
    let (tx, rx) = channel::<Result<String, String>>();
    let (method, params) = (HSTRING::from(method), HSTRING::from(params));
    window
        .with_webview(move |w| {
            let tx2 = tx.clone();
            let started = (|| unsafe {
                let core = w.controller().CoreWebView2().map_err(|e| e.to_string())?;
                let handler = CallDevToolsProtocolMethodCompletedHandler::create(Box::new(move |res, json| {
                    let _ = tx2.send(res.map(|_| json).map_err(|e| e.to_string()));
                    Ok(())
                }));
                core.CallDevToolsProtocolMethod(&method, &params, &handler).map_err(|e| e.to_string())
            })();
            if let Err(e) = started {
                let _ = tx.send(Err(e));
            }
        })
        .map_err(|e| e.to_string())?;
    rx.recv_timeout(Duration::from_secs(60)).map_err(|_| "slow".to_string())?
}

/// The window full screen for the show, and back: the window itself.
#[tauri::command]
fn set_full(window: WebviewWindow, on: bool) {
    // the Mac: with a screen plugged in, the show goes there first, and comes back after (`mac::show_screen`); the
    // full screen is the system's own, in a space of its own, as the green button makes it
    #[cfg(target_os = "macos")]
    {
        use std::collections::HashMap;
        static WAS: Mutex<Option<HashMap<String, objc2_foundation::NSRect>>> = Mutex::new(None);
        if on {
            if let Some(was) = crate::mac::move_for_show(&window) {
                lock(&WAS).get_or_insert_default().insert(window.label().to_string(), was);
            }
        } else if let Some(was) = lock(&WAS).get_or_insert_default().remove(window.label()) {
            let _ = window.set_fullscreen(false);
            // the window leaves full screen in the system's own motion before it can move back
            let w = window.clone();
            std::thread::spawn(move || {
                for _ in 0..40 {
                    std::thread::sleep(std::time::Duration::from_millis(100));
                    if !w.is_fullscreen().unwrap_or(false) {
                        break;
                    }
                }
                std::thread::sleep(std::time::Duration::from_millis(300));
                crate::mac::move_back(&w, was);
            });
            return;
        }
    }
    let _ = window.set_fullscreen(on);
}

/// The Mac with more than one screen: the presenter's view in a window of its own on the presenter's screen, for this
/// window's show (`mac::open_presenter`); false with one screen.
#[cfg(target_os = "macos")]
#[tauri::command(async)]
fn presenter_open(window: WebviewWindow) -> bool {
    crate::mac::open_presenter(&window)
}

/// The show's page tells its presenter's window what to show: the slide, the next one, the notes, the words.
#[cfg(target_os = "macos")]
#[tauri::command]
fn presenter_send(window: WebviewWindow, state: serde_json::Value) {
    use tauri::Emitter;
    let _ = window.emit_to(format!("{}{}", window.label(), crate::mac::PRESENTER), "presenter-state", state);
}

/// The presenter's window asks the show's page to do something: a key, a button, "ready".
#[cfg(target_os = "macos")]
#[tauri::command]
fn presenter_do(window: WebviewWindow, action: String) {
    use tauri::Emitter;
    if let Some(owner) = crate::mac::owner_of(window.label()) {
        let _ = window.emit_to(owner, "presenter-do", action);
    }
}

/// The show ended: its presenter's window goes.
#[cfg(target_os = "macos")]
#[tauri::command]
fn presenter_close(window: WebviewWindow) {
    crate::mac::close_presenter(window.app_handle(), window.label());
}

/// A web address a slide links to, opened by the system's browser. The family's check (`links::web_address`): http and
/// https with a host, and nothing a command line could split; nothing else a file names is run.
#[tauri::command]
fn open_link(url: String) {
    dotany_base::links::open_web_link(&url);
}

const PICTURES: &[&str] = &["png", "jpg", "jpeg", "gif", "bmp"];
const MEDIA: &[&str] = &["mp4", "m4v", "mov", "wmv", "avi", "mp3", "wav", "m4a", "wma", "ogg", "flac"];

/// The system's open dialog for something to put on a slide: a picture, or a film or sound. `label` names the filter.
#[tauri::command(async)]
fn pick_insert(window: WebviewWindow, kind: String, label: String) -> Option<String> {
    let exts = if kind == "image" { PICTURES } else { MEDIA };
    // the Mac: the family's open panel, as this window's sheet (and a test's answer in its place)
    #[cfg(target_os = "macos")]
    {
        let _ = label;
        let folder = window.state::<Base>().current_in(window.label()).and_then(|p| p.parent().map(Path::to_path_buf));
        let picked = dotany_base::platform::ask_open(&window, exts, folder, dotany_base::platform::Pick::File);
        picked.into_iter().next().map(|p| p.to_string_lossy().into_owned())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let mut dialog = rfd::FileDialog::new().set_parent(&window).add_filter(label, exts);
        let folder = window.state::<Base>().current_in(window.label()).and_then(|p| p.parent().map(Path::to_path_buf));
        if let Some(dir) = folder {
            dialog = dialog.set_directory(dir);
        }
        dialog.pick_file().map(|p| p.to_string_lossy().into_owned())
    }
}

/// The bytes of a picture, film or sound chosen to go on a slide. Only those kinds, by name.
#[tauri::command(async)]
fn read_insert(path: String) -> Result<Response, String> {
    let p = PathBuf::from(&path);
    let ext = p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    if !PICTURES.contains(&ext.as_str()) && !MEDIA.contains(&ext.as_str()) {
        return Err("refused".into());
    }
    fs::read(&p).map(Response::new).map_err(|_| "damaged".into())
}

// ---------- films and sounds in a slide that WebView2 won't play: Windows plays them ----------

struct Playing {
    gen: u64,
    film: mf::SystemFilm,
    shared: mf::Shared,
    file: PathBuf,
}

impl Playing {
    /// Stops playing and lets go of the memory its pictures went through, in the window it was playing in (`label`). A
    /// window that has gone takes that memory with it: it is left to go with the program.
    fn close(self, app: &AppHandle, label: &str) {
        self.film.close();
        match app.get_webview_window(label) {
            Some(w) => mf::unshare(&w, self.shared),
            None => std::mem::forget(self.shared),
        }
        let _ = fs::remove_file(&self.file);
    }
}

/// The films and sounds Windows is playing in one window. Another window of the program has its own.
#[derive(Default)]
struct Media {
    playing: Mutex<Vec<Playing>>,
}

/// Each window's `Media`, by its label.
type Medias = PerWindow<Media>;

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
enum SystemOpened {
    Ok { facts: mf::SystemFacts },
    Refused,
    Nodecoder,
}

/// Starts Windows playing the film or sound in the body (its extension in the `ext` header); its pictures arrive in the
/// page as shared memory tagged `gen`. The bytes are put in a file of their own for the time it plays, then removed.
#[tauri::command(async)]
fn media_open(window: WebviewWindow, state: State<Medias>, request: Request) -> SystemOpened {
    let gen: u64 = header(&request, "gen").and_then(|g| g.parse().ok()).unwrap_or(0);
    let ext: String =
        header(&request, "ext").unwrap_or_default().chars().filter(|c| c.is_ascii_alphanumeric()).take(5).collect();
    let InvokeBody::Raw(bytes) = request.body() else { return SystemOpened::Refused };
    let file = temp_file(if ext.is_empty() { "bin" } else { &ext });
    if fs::write(&file, bytes).is_err() {
        return SystemOpened::Refused;
    }
    {
        let film = match mf::SystemFilm::open(&file) {
            Ok(f) => f,
            Err(e) => {
                let _ = fs::remove_file(&file);
                return if e == mf::SystemError::NoDecoder { SystemOpened::Nodecoder } else { SystemOpened::Refused };
            }
        };
        let facts = film.facts;
        let Some((shared, at)) = mf::share(&window, facts.surface_len(), gen) else {
            film.close();
            let _ = fs::remove_file(&file);
            return SystemOpened::Refused;
        };
        film.send(mf::Command::Surface(at, facts.surface_len()));
        lock(&state.of(window.label()).playing).push(Playing { gen, film, shared, file });
        SystemOpened::Ok { facts }
    }
}

#[tauri::command]
fn media_control(
    window: WebviewWindow,
    state: State<Medias>,
    gen: u64,
    action: String,
    value: Option<f64>,
    id: Option<u32>,
) {
    {
        let Some(media) = state.get(window.label()) else { return };
        let playing = lock(&media.playing);
        let Some(p) = playing.iter().find(|p| p.gen == gen) else { return };
        let v = value.unwrap_or(0.0);
        p.film.send(match action.as_str() {
            "play" => mf::Command::Play,
            "pause" => mf::Command::Pause,
            "seek" => mf::Command::Seek(v, id.unwrap_or(0)),
            "volume" => mf::Command::Volume(v),
            "rate" => mf::Command::Rate(v),
            _ => return,
        });
    }
}

/// The page has let go of the memory: playing stops, the memory is freed and the file goes.
#[tauri::command(async)]
fn media_close(window: WebviewWindow, state: State<Medias>, gen: u64) {
    {
        let Some(media) = state.get(window.label()) else { return };
        let found = {
            let mut playing = lock(&media.playing);
            playing.iter().position(|p| p.gen == gen).map(|i| playing.remove(i))
        };
        if let Some(p) = found {
            p.close(window.app_handle(), window.label());
        }
    }
}

/// The Mac: a film in a slide that WKWebView won't play (AVI), repackaged for it; the new film's bytes. Answers
/// "windows" for WMV and WMA, which nothing on a Mac plays, and "cannot" for what can't be repackaged.
#[cfg(target_os = "macos")]
#[tauri::command(async)]
fn media_repack(request: Request) -> Result<Response, String> {
    let ext: String =
        header(&request, "ext").unwrap_or_default().chars().filter(|c| c.is_ascii_alphanumeric()).take(5).collect();
    let ext = ext.to_ascii_lowercase();
    let InvokeBody::Raw(bytes) = request.body() else { return Err("cannot".into()) };
    match crate::mac::repack(bytes, &ext, &temp_file(if ext.is_empty() { "bin" } else { &ext })) {
        Ok((film, _)) => Ok(Response::new(film)),
        Err(crate::mac::NotPlayed::Windows) => Err("windows".into()),
        Err(crate::mac::NotPlayed::Cannot) => Err("cannot".into()),
    }
}

/// How many films and sounds Windows is playing for this window. For the tests.
#[tauri::command]
fn media_count(window: WebviewWindow, state: State<Medias>) -> usize {
    state.get(window.label()).map_or(0, |media| lock(&media.playing).len())
}

pub fn run() {
    let builder = dotany_base::builder_per_window(&TOOL, FileWindows { forget, extra: dotany_base::no_extra })
        .manage(Medias::default());
    // the Mac's own commands besides (`media_repack`, `print_png`)
    #[cfg(target_os = "macos")]
    let builder = builder.invoke_handler(dotany_base::handler![
        read_deck,
        adopt_deck,
        save_bytes,
        print_pdf,
        print_png,
        capture,
        set_full,
        open_link,
        pick_insert,
        read_insert,
        media_open,
        media_control,
        media_close,
        media_count,
        media_repack,
        presenter_open,
        presenter_send,
        presenter_do,
        presenter_close
    ]);
    #[cfg(windows)]
    let builder = builder.invoke_handler(dotany_base::handler![
        read_deck,
        adopt_deck,
        save_bytes,
        print_pdf,
        capture,
        set_full,
        open_link,
        pick_insert,
        read_insert,
        media_open,
        media_control,
        media_close,
        media_count
    ]);
    builder.run_family(tauri::generate_context!()).expect("error while running DotAny Slides");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decks_are_known_by_their_first_bytes() {
        let dir = std::env::temp_dir().join(format!("dotany-slides-sniff-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let zip = dir.join("a.pptx");
        fs::write(&zip, b"PK\x03\x04rest").unwrap();
        let cfb = dir.join("b.ppt");
        fs::write(&cfb, [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1, 0]).unwrap();
        let text = dir.join("c.pptx");
        fs::write(&text, b"hello world").unwrap();
        assert!(looks_like_a_deck(&zip));
        assert!(looks_like_a_deck(&cfb));
        assert!(!looks_like_a_deck(&text));
        assert_eq!(unescape("%E4%B8%AD.pptx"), "中.pptx");
        fs::remove_dir_all(&dir).ok();
    }
}
