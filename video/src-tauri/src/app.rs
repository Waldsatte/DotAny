//! The Tauri side of DotAny Video: the film open in each window and the commands the page calls. The window, settings
//! and the recent list, the watcher, fonts and the platform layer are the family's (dotany-base). Video keeps a file per
//! window (`dotany_base::builder_per_window`): on the Mac one program shows each film in a window of its own, and each
//! plays on its own in its own page; on Windows each film is a program of its own, with one window.

use crate::formats::{self, Container};
use crate::media::{Cover, Edit, Facts, Media, Request};
use crate::platform;
use crate::stream;
use dotany_base::RunFamily;
use dotany_base::atomic::{WriteFailure, atomic_write_with};
use dotany_base::{Base, FileWindows, Meta, PerWindow, Tool, file_name, lock, meta_of};
use serde::Serialize;
use std::borrow::Cow;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tauri::ipc::Response;
use tauri::{AppHandle, Manager, State, WebviewWindow};

fn is_film(path: &Path) -> bool {
    matches!(formats::sniff_file(path), Ok(Some(_)))
}

/// The window's film is closed: nothing of it, or of one on its way, is kept. A film the system plays for it stays until
/// the page lets it go (`system_close`). A window that has gone takes everything with it, the films
/// the system was playing for it too: they stop and their memory is freed.
fn forget(app: &AppHandle, label: &str) {
    let state = app.state::<AppState>();
    if app.get_webview_window(label).is_none() {
        let gone = state.windows.remove(label);
        drop(gone);
        return;
    }
    let Some(w) = state.windows.get(label) else { return };
    *lock(&w.pending) = None;
    *lock(&w.doc) = Doc::default();
}

/// Video keeps a film per window and says so in `FileWindows` (`forget`); this is never called.
fn nothing(_: &AppHandle) {}

pub static TOOL: Tool = Tool {
    name: "DotAny Video",
    folder: "Video",
    data_env: "DOTANY_VIDEO_DATA",
    user_dir: "Videos",
    opens_files: true,
    extensions: &formats::VIDEO_EXTENSIONS,
    sniff: is_film,
    size: (1180.0, 800.0),
    min_size: (480.0, 420.0),
    on_close: nothing,
};

const LARGEST_COVER: u64 = 64 << 20;
const LARGEST_SUBTITLE: u64 = 32 << 20;

#[derive(Default)]
struct Doc {
    media: Option<Arc<Media>>,
    path: Option<PathBuf>,
    container: Option<Container>,
    /// What an address must name, so a late request can't read the previous film.
    gen: u64,
    /// What the file was when it was read. The plan for a save is only true of that file. Whether another program has
    /// been at it since is the family's (`Base`).
    read_as: Option<Meta>,
    new_cover: Option<Cover>,
    /// The Mac: the film as the player gets it, when it isn't the file itself (an MKV or AVI repackaged).
    #[cfg(target_os = "macos")]
    repacked: Option<Arc<dotany_repack::Repacked>>,
    /// The Mac: a WebM or MKV the player gets as it is, but with these elements (start, end) seen as Void: its
    /// attachments, which WKWebView's WebM player can't get past (`Matroska::attachment_places`).
    #[cfg(target_os = "macos")]
    hidden: Vec<(u64, u64)>,
}

/// A film that has been read but hasn't yet shown it can play.
struct Pending {
    gen: u64,
    doc: Doc,
    text: String,
}

/// A film the system is playing, and the memory its pictures go to.
struct Playing {
    gen: u64,
    film: platform::SystemFilm,
    shared: platform::Shared,
}

/// What one window holds: its film, the one on its way, the films the system plays for its page, and whether it is
/// saving. Another window of the program (the Mac) has its own of each and sees none of these.
#[derive(Default)]
struct Win {
    doc: Mutex<Doc>,
    /// Usually one; two for a moment while a new film shows it can play before the old one goes.
    systems: Mutex<Vec<Playing>>,
    pending: Mutex<Option<Pending>>,
    saving: AtomicBool,
}

/// What the program holds: each window's own (`windows`), and the count that names each film, so an address names one
/// film in the whole program.
struct AppState {
    windows: PerWindow<Win>,
    gens: AtomicU64,
}

impl AppState {
    fn win(&self, window: &WebviewWindow) -> Arc<Win> {
        self.windows.of(window.label())
    }
}

// ---------- opening ----------

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
enum Opened {
    Ok {
        gen: u64,
        path: String,
        dir: Option<String>,
        name: String,
        size: u64,
        facts: Facts,
        beside: usize,
    },
    Refused,
    /// A film format this program plays, but this file isn't one: empty, damaged, or not what its name says.
    Damaged,
    Missing,
    Failed {
        detail: String,
    },
}

fn read_into(state: &AppState, win: &Win, path: PathBuf) -> Opened {
    let Ok(meta) = fs::metadata(&path) else { return Opened::Missing };
    if !meta.is_file() {
        return Opened::Missing;
    }
    let name = file_name(&path);
    if !formats::allowed_name(&name) {
        return Opened::Refused;
    }
    let kind = match formats::sniff_file(&path) {
        Ok(Some(k)) => k,
        Ok(None) => return Opened::Damaged,
        Err(e) => return Opened::Failed { detail: e.to_string() },
    };
    let media = Arc::new(Media::open(&path, kind));
    let facts = media.facts();
    // the Mac plays every film in the page, an MKV or AVI repackaged for it: none is the system's
    #[cfg(target_os = "macos")]
    let (facts, repacked) = (Facts { system: false, ..facts }, platform::for_player(&path, kind));
    #[cfg(target_os = "macos")]
    let hidden = match media.as_ref() {
        Media::Mkv(m) if repacked.is_none() => m.attachment_places().to_vec(),
        _ => Vec::new(),
    };
    let gen = state.gens.fetch_add(1, Ordering::Relaxed) + 1;
    let text = path.to_string_lossy().into_owned();
    let dir = path.parent().map(|d| d.to_string_lossy().into_owned());
    let beside = dotany_base::folder::count_beside(&path, |n| TOOL.allowed_name(n));
    let now = meta_of(&path);
    let doc = Doc {
        media: Some(media),
        path: Some(path),
        container: Some(kind),
        gen,
        read_as: now,
        new_cover: None,
        #[cfg(target_os = "macos")]
        repacked,
        #[cfg(target_os = "macos")]
        hidden,
    };
    *lock(&win.pending) = Some(Pending { gen, doc, text: text.clone() });
    Opened::Ok { gen, path: text, dir, name, size: meta.len(), facts, beside }
}

#[tauri::command(async)]
fn open_path(window: WebviewWindow, state: State<AppState>, path: String) -> Opened {
    let path = std::path::absolute(&path).unwrap_or_else(|_| PathBuf::from(&path));
    read_into(&state, &state.win(&window), path)
}

/// The film has drawn its first picture: now it's the window's open film.
#[tauri::command(async)]
fn commit_open(window: WebviewWindow, state: State<AppState>, gen: u64) -> bool {
    let win = state.win(&window);
    let Some(next) = lock(&win.pending).take_if(|p| p.gen == gen) else { return false };
    *lock(&win.doc) = next.doc;
    // watched, stepped from and remembered from now on
    dotany_base::opened_in(window.app_handle(), window.label(), Path::new(&next.text));
    true
}

/// A quick look, before anything unsaved is asked about. Video's own: a subtitle file goes to the open film.
#[tauri::command(async)]
fn can_open(path: String) -> &'static str {
    let p = Path::new(&path);
    if !p.is_file() {
        return "missing";
    }
    let name = file_name(p);
    if formats::subtitle_name(&name) {
        return "subtitle";
    }
    if !formats::allowed_name(&name) {
        return "refused";
    }
    // the name says it's a film this program plays; bytes that aren't one can't be read, which isn't an unsupported format
    match formats::sniff_file(p) {
        Ok(Some(_)) => "ok",
        _ => "damaged",
    }
}

#[tauri::command(async)]
fn pick_subtitle(window: WebviewWindow, base: State<Base>, label: String) -> Option<String> {
    // the Mac: the family's open panel, as this window's sheet (and a test's answer in its place)
    #[cfg(target_os = "macos")]
    {
        let _ = label;
        mac_pick(&window, &base, &formats::SUBTITLE_EXTENSIONS)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let mut dialog = rfd::FileDialog::new().set_parent(&window).add_filter(label, &formats::SUBTITLE_EXTENSIONS);
        if let Some(dir) = base.start_folder_in(window.label()) {
            dialog = dialog.set_directory(dir);
        }
        dialog.pick_file().map(|p| p.to_string_lossy().into_owned())
    }
}

/// One file of these kinds, starting where this window's film is.
#[cfg(target_os = "macos")]
fn mac_pick(window: &WebviewWindow, base: &Base, exts: &'static [&'static str]) -> Option<String> {
    let dir = base.start_folder_in(window.label());
    let picked = dotany_base::platform::ask_open(window, exts, dir, dotany_base::platform::Pick::File);
    picked.into_iter().next().map(|p| p.to_string_lossy().into_owned())
}

/// The file as it is; which characters it holds is the page's question.
#[tauri::command(async)]
fn read_subtitle(path: String) -> Result<Response, String> {
    let p = Path::new(&path);
    if !formats::subtitle_name(&file_name(p)) {
        return Err("not a subtitle file".into());
    }
    if fs::metadata(p).map_err(|e| e.to_string())?.len() > LARGEST_SUBTITLE {
        return Err("too large".into());
    }
    fs::read(p).map(Response::new).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn pick_cover(window: WebviewWindow, base: State<Base>, label: String) -> Option<String> {
    // the Mac: the family's open panel, as this window's sheet
    #[cfg(target_os = "macos")]
    {
        let _ = label;
        mac_pick(&window, &base, &["jpg", "jpeg", "png"])
    }
    #[cfg(not(target_os = "macos"))]
    {
        let mut dialog = rfd::FileDialog::new().set_parent(&window).add_filter(label, &["jpg", "jpeg", "png"]);
        if let Some(dir) = base.start_folder_in(window.label()) {
            dialog = dialog.set_directory(dir);
        }
        dialog.pick_file().map(|p| p.to_string_lossy().into_owned())
    }
}

/// A picture chosen as the cover: held here until a save, and handed back so the page can show it.
#[tauri::command(async)]
fn load_cover(window: WebviewWindow, state: State<AppState>, path: String) -> Result<Response, String> {
    if fs::metadata(&path).map_err(|e| e.to_string())?.len() > LARGEST_COVER {
        return Err("too large".into());
    }
    let cover = Cover::from_bytes(fs::read(&path).map_err(|e| e.to_string())?).ok_or("not a JPG or PNG")?;
    let bytes = cover.bytes.clone();
    lock(&state.win(&window).doc).new_cover = Some(cover);
    Ok(Response::new(bytes))
}

/// The film's current picture, which the page made into a JPG.
#[tauri::command(async)]
fn set_cover(window: WebviewWindow, state: State<AppState>, request: tauri::ipc::Request) -> bool {
    let tauri::ipc::InvokeBody::Raw(body) = request.body() else { return false };
    let Some(cover) = Cover::from_bytes(body.clone()) else { return false };
    lock(&state.win(&window).doc).new_cover = Some(cover);
    true
}

#[tauri::command(async)]
fn read_cover(window: WebviewWindow, state: State<AppState>) -> Response {
    let found = {
        let win = state.win(&window);
        let doc = lock(&win.doc);
        doc.media.clone().zip(doc.path.clone())
    };
    Response::new(found.and_then(|(media, path)| media.cover(&path)).unwrap_or_default())
}

// ---------- the film, for the player ----------

/// The film of the window whose page asked (`label`, the WebView's own), or the one on its way to it: a window never
/// gets another's.
fn film_response(
    app: &AppHandle,
    label: &str,
    path: &str,
    range: Option<&str>,
) -> tauri::http::Response<Cow<'static, [u8]>> {
    let gen = path.trim_matches('/').parse::<u64>().ok();
    #[cfg(target_os = "macos")]
    if let Some(answer) = repacked_response(app, label, gen, range) {
        return answer;
    }
    let named = |doc: &Doc| match (&doc.path, doc.container) {
        (Some(p), Some(k)) if gen == Some(doc.gen) => Some((p.clone(), k)),
        _ => None,
    };
    let found = app.state::<AppState>().windows.get(label).and_then(|win| {
        let open = named(&lock(&win.doc));
        open.or_else(|| lock(&win.pending).as_ref().and_then(|p| named(&p.doc)))
    });
    match found {
        Some((file, kind)) => stream::respond(&file, kind.mime(), range),
        None => stream::refused(404),
    }
}

/// The Mac: the window's film as the player gets it when that isn't the file byte for byte: repackaged, or a WebM or
/// MKV with its attachments hidden.
#[cfg(target_os = "macos")]
fn repacked_response(
    app: &AppHandle,
    label: &str,
    gen: Option<u64>,
    range: Option<&str>,
) -> Option<tauri::http::Response<Cow<'static, [u8]>>> {
    let named = |doc: &Doc| match (&doc.path, doc.container) {
        (Some(p), Some(k)) if gen == Some(doc.gen) && (doc.repacked.is_some() || !doc.hidden.is_empty()) => {
            Some((p.clone(), k, doc.repacked.clone(), doc.hidden.clone()))
        }
        _ => None,
    };
    let win = app.state::<AppState>().windows.get(label)?;
    let open = named(&lock(&win.doc));
    let (file, kind, film, hidden) = open.or_else(|| lock(&win.pending).as_ref().and_then(|p| named(&p.doc)))?;
    Some(match film {
        Some(film) => stream::respond_repacked(&file, &film, range),
        None => stream::respond_hiding(&file, kind.mime(), range, &hidden),
    })
}

// ---------- films the system plays: AVI and WMV ----------

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
enum SystemOpened {
    Ok { facts: platform::SystemFacts },
    Refused,
    Nodecoder,
}

fn system_path(win: &Win, gen: u64) -> Option<PathBuf> {
    let named = |doc: &Doc| match (&doc.path, doc.container) {
        (Some(p), Some(k)) if doc.gen == gen && k.by_system() => Some(p.clone()),
        _ => None,
    };
    let found = named(&lock(&win.doc));
    found.or_else(|| lock(&win.pending).as_ref().and_then(|p| named(&p.doc)))
}

/// Starts the system playing film `gen` of this window; its pictures arrive in the page as shared memory.
#[tauri::command(async)]
fn system_open(window: WebviewWindow, state: State<AppState>, gen: u64) -> SystemOpened {
    let win = state.win(&window);
    let Some(path) = system_path(&win, gen) else { return SystemOpened::Refused };
    let film = match platform::SystemFilm::open(&path) {
        Ok(f) => f,
        Err(platform::SystemError::NoDecoder) => return SystemOpened::Nodecoder,
        Err(platform::SystemError::Refused) => return SystemOpened::Refused,
    };
    let facts = film.facts;
    let Some((shared, at)) = platform::share(&window, facts.surface_len(), gen) else {
        film.close();
        return SystemOpened::Refused;
    };
    film.send(platform::Command::Surface(at, facts.surface_len()));
    lock(&win.systems).push(Playing { gen, film, shared });
    SystemOpened::Ok { facts }
}

#[tauri::command]
fn system_control(
    window: WebviewWindow,
    state: State<AppState>,
    gen: u64,
    action: String,
    value: Option<f64>,
    id: Option<u32>,
) {
    let win = state.win(&window);
    let systems = lock(&win.systems);
    let Some(p) = systems.iter().find(|p| p.gen == gen) else { return };
    let v = value.unwrap_or(0.0);
    p.film.send(match action.as_str() {
        "play" => platform::Command::Play,
        "pause" => platform::Command::Pause,
        "seek" => platform::Command::Seek(v, id.unwrap_or(0)),
        "volume" => platform::Command::Volume(v),
        "rate" => platform::Command::Rate(v),
        _ => return,
    });
}

/// The page has let go of the memory: the film stops, then the memory is freed.
#[tauri::command(async)]
fn system_close(window: WebviewWindow, state: State<AppState>, gen: u64) {
    let found = {
        let win = state.win(&window);
        let mut systems = lock(&win.systems);
        systems.iter().position(|p| p.gen == gen).map(|i| systems.remove(i))
    };
    if let Some(p) = found {
        p.film.close();
        platform::unshare(&window, p.shared);
    }
}

/// How many films the system is playing for this window: one at most once a new film has settled. For the tests.
#[tauri::command]
fn system_count(window: WebviewWindow, state: State<AppState>) -> usize {
    lock(&state.win(&window).systems).len()
}

// ---------- saving ----------

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
enum Saved {
    Saved { path: String, name: String, size: u64 },
    Busy,
    Nothing,
    Changed,
    Failed { kind: WriteFailure, detail: String },
}

fn edit_from(doc: &Doc, request: &Request) -> Edit {
    Edit {
        from_key: request.from_key,
        to_key: request.to_key,
        title: request.title.clone(),
        cover: if request.cover { doc.new_cover.clone() } else { None },
    }
}

/// The new file's size, exactly: the plan is the file, before it's written.
#[tauri::command(async)]
fn estimate_save(window: WebviewWindow, state: State<AppState>, request: Request) -> Option<u64> {
    let (media, path, edit) = {
        let win = state.win(&window);
        let doc = lock(&win.doc);
        (doc.media.clone()?, doc.path.clone()?, edit_from(&doc, &request))
    };
    media.plan(&path, &edit).ok().map(|p| p.size)
}

struct ClearOnDrop<'a>(&'a AtomicBool);
impl Drop for ClearOnDrop<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Relaxed);
    }
}

/// One save at a time in each window; another window's save is its own.
#[tauri::command(async)]
fn save_as(window: WebviewWindow, state: State<AppState>, path: String, request: Request) -> Saved {
    let win = state.win(&window);
    if win.saving.swap(true, Ordering::Relaxed) {
        return Saved::Busy;
    }
    let _done = ClearOnDrop(&win.saving);
    let target = PathBuf::from(&path);
    let (media, source, read_as, edit) = {
        let doc = lock(&win.doc);
        let (Some(m), Some(p)) = (doc.media.clone(), doc.path.clone()) else { return Saved::Nothing };
        (m, p, doc.read_as, edit_from(&doc, &request))
    };
    // the plan says where samples are in the file as it was read
    if meta_of(&source) != read_as {
        return Saved::Changed;
    }
    let plan = match media.plan(&source, &edit) {
        Ok(p) => p,
        Err(detail) => return Saved::Failed { kind: WriteFailure::Other, detail },
    };
    let mut from = match File::open(&source) {
        Ok(f) => f,
        Err(e) => return Saved::Failed { kind: WriteFailure::Missing, detail: e.to_string() },
    };
    let fail_at: Option<u64> = std::env::var("DOTANY_VIDEO_FAIL_WRITE").ok().and_then(|v| v.parse().ok());
    let written = atomic_write_with(&target, &[], &mut |file, _| {
        plan.write_until(&mut from, file, fail_at)?;
        // a source that changed while being copied has given us a mix of two files
        if meta_of(&source) != read_as {
            return Err(std::io::Error::other("the source file changed during the save"));
        }
        Ok(())
    });
    drop(from);
    if let Err(e) = written {
        return Saved::Failed { kind: e.kind, detail: e.detail };
    }
    let size = plan.size;
    drop(plan);
    // the program's own write isn't news to the watcher, and it goes on the recent list
    dotany_base::saved_in(window.app_handle(), window.label(), &target);
    Saved::Saved { path: path.clone(), name: file_name(&target), size }
}

/// What every window's page finds besides the family's own: a test may ask for films that don't start playing as they
/// open, so its pictures hold still.
fn page_extra() -> serde_json::Value {
    let test = std::env::var_os("DOTANY_TEST").is_some();
    let still = test && std::env::var_os("DOTANY_VIDEO_TEST_STILL").is_some_and(|v| !v.is_empty());
    serde_json::json!({ "still": still })
}

pub fn run() {
    dotany_base::builder_per_window(&TOOL, FileWindows { forget, extra: page_extra })
        // the asking window's film and nothing else, a stretch at a time, off the main thread
        .register_asynchronous_uri_scheme_protocol("dotfilm", |ctx, request, responder| {
            let path = request.uri().path().to_string();
            let range = request.headers().get("range").and_then(|v| v.to_str().ok()).map(str::to_string);
            let (app, label) = (ctx.app_handle().clone(), ctx.webview_label().to_string());
            std::thread::spawn(move || responder.respond(film_response(&app, &label, &path, range.as_deref())));
        })
        .manage(AppState { windows: PerWindow::default(), gens: AtomicU64::new(0) })
        // the family's commands, but Video answers `can_open` itself (a subtitle file goes to the film), so it lists them here
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
            commit_open,
            can_open,
            pick_subtitle,
            read_subtitle,
            pick_cover,
            load_cover,
            set_cover,
            read_cover,
            estimate_save,
            save_as,
            system_open,
            system_control,
            system_close,
            system_count
        ])
        .run_family(tauri::generate_context!())
        .expect("error while running DotAny Video");
}
