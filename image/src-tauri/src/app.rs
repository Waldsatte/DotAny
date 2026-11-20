//! The Tauri side of DotAny Image: what makes this tool itself, the picture open in each window, and its commands. The
//! window, settings, recent files, the watcher, fonts and the platform layer are dotany-base. Image keeps a file per
//! window (`dotany_base::builder_per_window`): on the Mac one program shows each picture in a window of its own; on
//! Windows each picture is a program of its own, with one window.

use crate::decode::{Body, OpenError, Picture};
use crate::exif::Info;
use crate::export::{self, Marks, Plan, Request, Route};
use crate::folder;
use crate::formats::{self, Kind};
use crate::ops::{Adjust, Geo, Rect, Tile};
use crate::store;
use dotany_base::RunFamily;
use dotany_base::atomic::{WriteFailure, atomic_write};
use dotany_base::{Base, FileWindows, PerWindow, Tool, file_name, lock, platform, same_file};
use serde::Serialize;
use std::borrow::Cow;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use tauri::{AppHandle, Manager, State, WebviewWindow};

type Early = (PathBuf, JoinHandle<Result<Picture, OpenError>>);

/// A look at the first bytes: is this really a picture we read?
fn sniff_head(path: &Path) -> formats::Sniff {
    let mut head = Vec::new();
    let Ok(f) = fs::File::open(path) else { return formats::Sniff::Damaged };
    if f.take(1 << 20).read_to_end(&mut head).is_err() {
        return formats::Sniff::Damaged;
    }
    formats::sniff(&head, &file_name(path))
}

fn is_picture(path: &Path) -> bool {
    matches!(sniff_head(path), formats::Sniff::Is(_))
}

/// The window's picture closed, or the window went: what it held goes, and a picture still being read stops.
fn forget(app: &AppHandle, label: &str) {
    if let Some(w) = app.state::<AppState>().windows.remove(label) {
        if let Some(old) = &lock(&w.doc).picture {
            old.stop();
        }
    }
}

/// Image keeps a picture per window and says so in `FileWindows` (`forget`); this is never called.
fn nothing(_: &AppHandle) {}

pub static TOOL: Tool = Tool {
    name: "DotAny Image",
    folder: "Image",
    data_env: "DOTANY_IMAGE_DATA",
    user_dir: "Pictures",
    opens_files: true,
    extensions: &formats::ALLOWED_EXT,
    sniff: is_picture,
    size: (1180.0, 800.0),
    min_size: (480.0, 420.0),
    on_close: nothing,
};

/// The picture on screen. Where it lives, and whether another program has been at it, is the family's (`Base`).
#[derive(Default)]
struct Doc {
    picture: Option<Arc<Picture>>,
    /// What a view address must name, so a late answer can't show the previous picture.
    gen: u64,
}

/// What one window holds: its picture, what is drawn on it on its way to a save, and its newest estimate. Another window
/// of the program (the Mac) has its own; nothing here is seen by it.
#[derive(Default)]
struct Win {
    doc: Mutex<Doc>,
    marks: Mutex<Marks>,
    estimates: AtomicU64,
}

/// What the program holds: each window's own (`windows`), and what every window shares because it belongs to no one
/// picture: the folder large pictures are paged out to (each picture's file there is its own), the picture from the
/// command line being read before the window asks for it (only ever the first window's), and the count that names each
/// picture, so a view address names one picture in the whole program.
struct AppState {
    windows: PerWindow<Win>,
    /// Where large pictures are paged out while they're open.
    temp: PathBuf,
    /// The picture from the command line, already being read while the window gets ready.
    early: Mutex<Option<Early>>,
    gens: AtomicU64,
}

impl AppState {
    fn win(&self, window: &WebviewWindow) -> Arc<Win> {
        self.windows.of(window.label())
    }

    fn picture(&self, window: &WebviewWindow) -> Option<Arc<Picture>> {
        lock(&self.win(window).doc).picture.clone()
    }
}

// ---------- opening ----------

#[derive(Serialize)]
struct SlotInfo {
    w: u32,
    h: u32,
    alpha: bool,
    moving: bool,
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
enum Opened {
    Ok {
        gen: u64,
        path: Option<String>,
        dir: Option<String>,
        name: String,
        kind: Kind,
        label: &'static str,
        size: u64,
        slots: Vec<SlotInfo>,
        info: Info,
        details: bool,
        svg: Option<String>,
        wide: bool,
        text_tool: bool,
        beside: usize,
    },
    Refused,
    Damaged,
    Toolarge,
    Missing,
    Failed {
        detail: String,
    },
}

fn reply(state: &AppState, window: &WebviewWindow, pic: Picture, path: Option<PathBuf>, name: String) -> Opened {
    let (app, label) = (window.app_handle(), window.label());
    // the window went while its picture was being read: nothing keeps it
    if app.get_webview_window(label).is_none() {
        pic.stop();
        return Opened::Missing;
    }
    let pic = Arc::new(pic);
    let slots = pic
        .slots
        .iter()
        .map(|s| SlotInfo { w: s.w, h: s.h, alpha: s.alpha, moving: matches!(s.body, Body::Frames(_)) })
        .collect();
    let gen = state.gens.fetch_add(1, Ordering::Relaxed) + 1;
    let opened = Opened::Ok {
        gen,
        path: path.as_ref().map(|p| p.to_string_lossy().into_owned()),
        dir: path.as_ref().and_then(|p| p.parent()).map(|d| d.to_string_lossy().into_owned()),
        name,
        kind: pic.kind,
        label: pic.kind.label(),
        size: pic.source.len() as u64,
        slots,
        info: pic.info.clone(),
        details: pic.meta.has_details(),
        svg: pic.svg.as_ref().map(|s| s.text.clone()),
        wide: pic.colour.is_some(),
        text_tool: path.is_some() && pic.kind == Kind::Svg && platform::sibling_tool("dotany-text").is_some(),
        beside: path.as_deref().map(folder::count_beside).unwrap_or(0),
    };
    // a picture with no file (pasted) leaves the family nothing to watch or step from, and a file handed over on the Mac
    // opens beside it rather than over it
    if path.is_none() {
        dotany_base::untitled_in(app, label);
    }
    let win = state.windows.of(label);
    {
        let mut doc = lock(&win.doc);
        if let Some(old) = &doc.picture {
            old.stop();
        }
        *doc = Doc { picture: Some(pic), gen };
    }
    *lock(&win.marks) = Marks::new();
    if let Some(p) = &path {
        dotany_base::opened_in(app, label, p);
    }
    opened
}

fn failed_open(e: OpenError) -> Opened {
    match e {
        OpenError::Refused => Opened::Refused,
        OpenError::Damaged => Opened::Damaged,
        OpenError::TooLarge => Opened::Toolarge,
    }
}

fn read_picture(path: &Path, temp: &Path) -> Result<Picture, OpenError> {
    let bytes = fs::read(path).map_err(|_| OpenError::Damaged)?;
    Picture::open(bytes, &file_name(path), temp)
}

fn open_into(state: &AppState, window: &WebviewWindow, path: PathBuf) -> Opened {
    let early = {
        let mut slot = lock(&state.early);
        match slot.take() {
            Some((p, handle)) if p == path => Some(handle),
            other => {
                *slot = other;
                None
            }
        }
    };
    let result = match early {
        Some(handle) => handle.join().unwrap_or(Err(OpenError::Damaged)),
        None => {
            let Ok(meta) = fs::metadata(&path) else { return Opened::Missing };
            if !meta.is_file() {
                return Opened::Missing;
            }
            match fs::read(&path) {
                Ok(bytes) => Picture::open(bytes, &file_name(&path), &state.temp),
                Err(e) => return Opened::Failed { detail: e.to_string() },
            }
        }
    };
    if !path.exists() {
        return Opened::Missing;
    }
    match result {
        Ok(pic) => {
            let name = file_name(&path);
            reply(state, window, pic, Some(path), name)
        }
        Err(e) => failed_open(e),
    }
}

#[tauri::command(async)]
fn open_path(window: WebviewWindow, state: State<AppState>, path: String) -> Opened {
    let path = std::path::absolute(&path).unwrap_or_else(|_| PathBuf::from(&path));
    open_into(&state, &window, path)
}

/// A quick look at the first bytes, before anything unsaved is asked about. Image's own: a camera RAW or HEIC named
/// like a picture of ours is refused ("This file format isn't supported yet."), not called damaged.
#[tauri::command(async)]
fn can_open(path: String) -> &'static str {
    let p = Path::new(&path);
    if fs::File::open(p).is_err() {
        return "missing";
    }
    match sniff_head(p) {
        formats::Sniff::Is(_) => "ok",
        formats::Sniff::Refused => "refused",
        formats::Sniff::Damaged => "damaged",
    }
}

/// A picture that came from the clipboard: it has no file yet. The name comes in the first line of the body.
#[tauri::command(async)]
fn open_pasted(window: WebviewWindow, state: State<AppState>, request: tauri::ipc::Request) -> Opened {
    let tauri::ipc::InvokeBody::Raw(body) = request.body() else { return Opened::Damaged };
    let Some(cut) = body.iter().position(|b| *b == b'\n') else { return Opened::Damaged };
    let name = String::from_utf8_lossy(&body[..cut]).into_owned();
    match Picture::open(body[cut + 1..].to_vec(), &name, &state.temp) {
        Ok(pic) => reply(&state, &window, pic, None, name),
        Err(e) => failed_open(e),
    }
}

#[tauri::command(async)]
fn beside_count(window: WebviewWindow, base: State<Base>) -> usize {
    base.current_in(window.label()).as_deref().map(folder::count_beside).unwrap_or(0)
}

#[derive(Serialize)]
struct FramesState {
    count: usize,
    done: bool,
    delays: Vec<u32>,
    alpha: bool,
    failed_early: bool,
}

/// How far reading a moving picture has got, and how long each frame lasts.
#[tauri::command(async)]
fn frames_state(window: WebviewWindow, state: State<AppState>, slot: usize) -> Option<FramesState> {
    let pic = state.picture(&window)?;
    let f = pic.frames(slot)?;
    Some(FramesState {
        count: f.count(),
        done: f.done(),
        delays: f.delays(),
        alpha: f.alpha.load(Ordering::Relaxed),
        failed_early: false,
    })
}

// ---------- pixels for the page ----------

/// Raw pixels with a little header: width, height, whether any are see-through. On their own address, away from the main thread.
fn pixel_response(img: Option<(u32, u32, Vec<u8>)>) -> tauri::http::Response<Cow<'static, [u8]>> {
    let builder = tauri::http::Response::builder()
        .header("Access-Control-Allow-Origin", "*")
        .header("Content-Type", "application/octet-stream");
    match img {
        Some((w, h, body)) => builder
            .status(200)
            .header("X-Width", w)
            .header("X-Height", h)
            .header("Access-Control-Expose-Headers", "X-Width, X-Height")
            .body(Cow::Owned(body))
            .unwrap(),
        None => builder.status(404).body(Cow::Borrowed(&[][..])).unwrap(),
    }
}

/// The pixels of the picture in the window whose page asked (`label`, the WebView's own): a window never sees another's.
fn serve_pixels(app: &AppHandle, label: &str, path: &str) -> Option<(u32, u32, Vec<u8>)> {
    let state = app.state::<AppState>();
    // the address arrives percent-encoded; only digits, signs and slashes are ever in it
    let path = path.replace("%2F", "/");
    let parts: Vec<&str> = path.trim_matches('/').split('/').collect();
    let num = |i: usize| parts.get(i).and_then(|p| p.parse::<i64>().ok());
    let gen = num(1)? as u64;
    let pic = {
        let win = state.windows.get(label)?;
        let doc = lock(&win.doc);
        if doc.gen != gen {
            return None;
        }
        doc.picture.clone()?
    };
    match *parts.first()? {
        // view/gen/slot/frame/max
        "view" => {
            let img = pic.view(num(2)? as usize, num(3)? as usize, num(4)? as u32)?;
            Some((img.w, img.h, img.rgba.clone()))
        }
        // region/gen/slot/frame/x/y/w/h/out_w/out_h/g0/g1/g2/g3/exposure/contrast/saturation/temperature/highlights/shadows
        "region" => {
            let rect = Rect { x: num(4)? as u32, y: num(5)? as u32, w: num(6)? as u32, h: num(7)? as u32 };
            let geo = Geo([num(10)? as i8, num(11)? as i8, num(12)? as i8, num(13)? as i8]);
            let adj = Adjust {
                exposure: num(14)? as i32,
                contrast: num(15)? as i32,
                saturation: num(16)? as i32,
                temperature: num(17)? as i32,
                highlights: num(18)? as i32,
                shadows: num(19)? as i32,
            };
            let img = export::region(
                &pic,
                num(2)? as usize,
                num(3)? as usize,
                geo,
                &adj,
                rect,
                num(8)? as u32,
                num(9)? as u32,
            )?;
            Some((img.w, img.h, img.rgba))
        }
        // lut/gen
        "lut" => {
            let c = pic.colour.as_ref()?;
            Some((crate::colour::LUT_SIDE as u32, crate::colour::LUT_SIDE as u32, c.table()))
        }
        _ => None,
    }
}

// ---------- another program changed the file ----------

/// The family's watcher asks before it says anything: a picture only touched (its time changed, its bytes not) isn't news.
fn same_bytes(app: &AppHandle, label: &str, path: &Path) -> bool {
    let win = app.state::<AppState>().windows.get(label);
    let pic = win.and_then(|w| lock(&w.doc).picture.clone());
    pic.is_some_and(|pic| fs::read(path).map(|b| b == *pic.source).unwrap_or(false))
}

#[tauri::command(async)]
fn reload_document(window: WebviewWindow, state: State<AppState>, base: State<Base>) -> Opened {
    match base.current_in(window.label()) {
        Some(p) => open_into(&state, &window, p),
        None => Opened::Missing,
    }
}

// ---------- saving ----------

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
enum Saved {
    Saved { path: String, name: String, size: u64, whole: bool, route: Route },
    Exists,
    Nothing,
    Failed { kind: WriteFailure, detail: String },
    Toolarge,
}

/// The newest question wins: an estimate nobody is waiting for any more is abandoned. Each window's own questions: one
/// window's estimate never abandons another's.
#[tauri::command(async)]
fn estimate_save(window: WebviewWindow, state: State<AppState>, seq: u64, request: Request) -> Option<Plan> {
    let win = state.win(&window);
    win.estimates.store(seq, Ordering::Relaxed);
    std::thread::sleep(std::time::Duration::from_millis(60));
    if win.estimates.load(Ordering::Relaxed) != seq {
        return None;
    }
    let pic = lock(&win.doc).picture.clone()?;
    export::estimate(&pic, &request).ok()
}

/// What a save loses, said at once: the estimate's own notes, without encoding anything, so the card's quiet line can
/// change with its format in the same motion as its options. The size follows with estimate_save.
#[tauri::command(async)]
fn save_notes(window: WebviewWindow, state: State<AppState>, request: Request) -> Option<Vec<&'static str>> {
    let pic = state.picture(&window)?;
    Some(export::notes(&pic, &request, export::route(&pic, &request)))
}

/// A piece of the drawing layer on its way to a save: 3 numbers (slot, x, y) and then a PNG.
#[tauri::command(async)]
fn put_tile(window: WebviewWindow, state: State<AppState>, request: tauri::ipc::Request) -> bool {
    let tauri::ipc::InvokeBody::Raw(body) = request.body() else { return false };
    if body.len() < 12 {
        return false;
    }
    let n = |i: usize| u32::from_le_bytes([body[i * 4], body[i * 4 + 1], body[i * 4 + 2], body[i * 4 + 3]]);
    let (slot, x, y) = (n(0) as usize, n(1), n(2));
    let Ok((rgba, w, h, _)) = crate::decode::read_png(&body[12..]) else { return false };
    lock(&state.win(&window).marks).entry(slot).or_default().push(Tile { x, y, w, h, rgba });
    true
}

#[tauri::command(async)]
fn clear_tiles(window: WebviewWindow, state: State<AppState>) {
    lock(&state.win(&window).marks).clear();
}

#[tauri::command(async)]
fn save_as(
    window: WebviewWindow,
    state: State<AppState>,
    base: State<Base>,
    path: String,
    overwrite: bool,
    request: Request,
) -> Saved {
    let (app, label) = (window.app_handle(), window.label());
    let target = PathBuf::from(&path);
    if target.exists() && !overwrite {
        return Saved::Exists;
    }
    let win = state.win(&window);
    let Some(pic) = lock(&win.doc).picture.clone() else { return Saved::Nothing };
    let need = request.w as u64 * request.h as u64 * 8;
    if need > crate::memory::available() {
        return Saved::Toolarge;
    }
    let how = export::route(&pic, &request);
    let mut marks = std::mem::take(&mut *lock(&win.marks));
    let bytes =
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| export::produce(&pic, &request, &mut marks))) {
            Ok(Ok(b)) => b,
            Ok(Err(e)) if e.starts_with("too large") => return Saved::Toolarge,
            Ok(Err(e)) => return Saved::Failed { kind: WriteFailure::Other, detail: e },
            Err(_) => return Saved::Failed { kind: WriteFailure::Other, detail: "the encoder stopped".into() },
        };
    let size = bytes.len() as u64;
    if let Err(e) = atomic_write(&target, &bytes) {
        return Saved::Failed { kind: e.kind, detail: e.detail };
    }
    let whole = export::is_whole(&pic, &request);
    let on_open_file = base.current_in(label).is_some_and(|p| same_file(&p, &target));
    if whole && !on_open_file {
        // the whole picture went into the new file: the window follows it there
        dotany_base::opened_in(app, label, &target);
    } else {
        dotany_base::saved_in(app, label, &target);
    }
    Saved::Saved { path: path.clone(), name: file_name(&target), size, whole, route: how }
}

// ---------- the family ----------

/// Opens the source of an SVG in DotAny Text. Only offered when Text is really there.
#[tauri::command(async)]
fn open_in_text(window: WebviewWindow, base: State<Base>) -> bool {
    let Some(path) = base.current_in(window.label()) else { return false };
    let Some(tool) = platform::sibling_tool("dotany-text") else { return false };
    platform::launch(&tool, &path)
}

/// What every window's page finds besides the family's own: it draws with the GPU unless `DOTANY_NOGL` is set, which
/// tries the path for computers without one.
fn page_extra() -> serde_json::Value {
    serde_json::json!({ "nogl": std::env::var_os("DOTANY_NOGL").is_some() })
}

pub fn run() {
    let start_path = std::env::args_os().skip(1).find(|a| !a.to_string_lossy().starts_with("--")).map(|a| {
        let p = PathBuf::from(a);
        std::path::absolute(&p).unwrap_or(p)
    });
    let temp = dotany_base::settings::data_dir(&TOOL).join("tmp");
    store::sweep(&temp);
    // reading starts now, so the picture is ready by the time the page asks for it
    let early = start_path.map(|p| {
        let (path, dir) = (p.clone(), temp.clone());
        (p, std::thread::spawn(move || read_picture(&path, &dir)))
    });
    dotany_base::set_same_bytes_check(same_bytes);
    dotany_base::builder_per_window(&TOOL, FileWindows { forget, extra: page_extra })
        .register_asynchronous_uri_scheme_protocol("dotpix", |ctx, request, responder| {
            let path = request.uri().path().to_string();
            let (app, label) = (ctx.app_handle().clone(), ctx.webview_label().to_string());
            std::thread::spawn(move || {
                let body = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| serve_pixels(&app, &label, &path)))
                    .unwrap_or(None);
                responder.respond(pixel_response(body));
            });
        })
        .manage(AppState { windows: PerWindow::default(), temp, early: Mutex::new(early), gens: AtomicU64::new(0) })
        // the family's commands, but Image answers `can_open` itself (dotany_base::handler! would list the family's)
        .invoke_handler(tauri::generate_handler![
            dotany_base::commands::set_setting,
            dotany_base::commands::forget_recent,
            dotany_base::commands::pick_open,
            dotany_base::commands::pick_save,
            dotany_base::commands::default_folder,
            dotany_base::commands::neighbour,
            dotany_base::commands::close_document,
            dotany_base::commands::keep_mine,
            dotany_base::commands::set_frame,
            dotany_base::commands::window_control,
            dotany_base::commands::window_menu,
            dotany_base::commands::set_snap_button,
            dotany_base::commands::set_title,
            dotany_base::commands::window_ready,
            dotany_base::commands::startup_done,
            dotany_base::commands::close_window,
            dotany_base::commands::print_window,
            dotany_base::commands::set_menu_words,
            dotany_base::commands::set_edited,
            open_path,
            can_open,
            open_pasted,
            beside_count,
            frames_state,
            reload_document,
            estimate_save,
            save_notes,
            put_tile,
            clear_tiles,
            save_as,
            open_in_text
        ])
        .run_family(tauri::generate_context!())
        .expect("error while running DotAny Image");
}
