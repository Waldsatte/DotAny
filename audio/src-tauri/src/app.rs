//! The Tauri side of DotAny Audio: the song open in each window and the commands the page calls. The window, settings
//! and the recent list, the watcher, fonts and the platform layer are the family's (dotany-base). Audio keeps a file
//! per window (`dotany_base::builder_per_window`): on the Mac one program shows each song in a window of its own, and
//! each window plays on its own, as two QuickTime windows do; on Windows each song is a program of its own, with one
//! window.

use crate::decode::{Decoded, OpenError, read_file};
use crate::engine::{Command, Engine, Note};
use crate::export::{self, OnDisk, Plan, Request, Route};
use crate::formats::{self, Kind, Sniff};
use crate::fx::{Recipe, Timeline, level_gain};
use crate::store::{self, Pcm};
use crate::tags::{self, Words};
use dotany_base::RunFamily;
use dotany_base::atomic::WriteFailure;
use dotany_base::{Base, FileWindows, PerWindow, Tool, file_name, lock, meta_of, same_file};
use lofty::tag::Tag;
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};

fn is_song(path: &Path) -> bool {
    matches!(formats::sniff_file(path), Ok(Sniff::Is(_)))
}

/// The window's song is closed: nothing plays, nothing of it is kept, and a song still being read is abandoned. The
/// window keeps its player for its next song. A window that has gone takes its
/// player with it: the player's thread ends and its sound stops.
fn forget(app: &AppHandle, label: &str) {
    let state = app.state::<AppState>();
    if app.get_webview_window(label).is_none() {
        if let Some(w) = state.windows.remove(label) {
            w.opens.fetch_add(1, Ordering::AcqRel);
        }
        return;
    }
    let Some(w) = state.windows.get(label) else { return };
    w.opens.fetch_add(1, Ordering::AcqRel);
    if let Some(e) = &*lock(&w.engine) {
        e.send(Command::Load(None));
    }
    *lock(&w.doc) = None;
}

/// Audio keeps a song per window and says so in `FileWindows` (`forget`); this is never called.
fn nothing(_: &AppHandle) {}

pub static TOOL: Tool = Tool {
    name: "DotAny Audio",
    folder: "Audio",
    data_env: "DOTANY_AUDIO_DATA",
    user_dir: "Music",
    opens_files: true,
    extensions: &formats::NAMES,
    sniff: is_song,
    size: (1180.0, 800.0),
    min_size: (480.0, 420.0),
    on_close: nothing,
};

struct Song {
    pcm: Arc<Pcm>,
    bits: Option<u32>,
    /// The format the sound came from: what can still be said about its quality, whatever has been saved since.
    came_from: Kind,
    disk: OnDisk,
    carry: Option<Tag>,
    new_cover: Option<Vec<u8>>,
}

/// What one window holds: the song on screen, its player, and the numbers by which a newer open and a newer estimate
/// win. Where the song lives, and whether another program has been at it, is the family's (`Base`). Another window of
/// the program (the Mac) has its own of each: playing, pausing or seeking in one never reaches another, and each plays
/// on the same speakers through a stream of its own, as two programs would.
#[derive(Default)]
struct Win {
    doc: Mutex<Option<Song>>,
    engine: Mutex<Option<Engine>>,
    opens: AtomicU64,
    estimates: AtomicU64,
}

/// What the program holds: each window's own (`windows`), and the folder long songs are paged out to, which every
/// window shares because each song's file in it has a name of its own (`store`).
struct AppState {
    windows: PerWindow<Win>,
    temp: PathBuf,
}

impl AppState {
    fn win(&self, window: &WebviewWindow) -> Arc<Win> {
        self.windows.of(window.label())
    }
}

/// The window's player, started the first time it's needed. What it says goes to its own window's page.
fn engine(window: &WebviewWindow, win: &Win) -> Engine {
    let mut slot = lock(&win.engine);
    if let Some(e) = &*slot {
        return e.clone();
    }
    let (handle, label) = (window.app_handle().clone(), window.label().to_string());
    let tap = std::env::var_os("DOTANY_TEST_TAP").map(PathBuf::from);
    let e = Engine::start(
        move |note| {
            let payload = match note {
                Note::At(t) => serde_json::json!({ "at": t, "playing": true }),
                Note::Ended(t) => serde_json::json!({ "at": t, "playing": false, "ended": true }),
                Note::NoDevice => serde_json::json!({ "playing": false, "noDevice": true }),
            };
            let _ = handle.emit_to(label.as_str(), "player", payload);
        },
        tap,
    );
    *slot = Some(e.clone());
    e
}

fn beside(path: &Path) -> usize {
    dotany_base::folder::count_beside(path, |n| TOOL.allowed_name(n))
}

// ---------- opening ----------

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
enum Opened {
    #[serde(rename_all = "camelCase")]
    Ok {
        path: String,
        dir: String,
        name: String,
        kind: Kind,
        size: u64,
        rate: u32,
        channels: usize,
        frames: u64,
        tags: Words,
        beside: usize,
        level: f32,
    },
    Refused,
    Damaged,
    Toolarge,
    /// An M4A, and this Windows can't decode it.
    Nodecoder,
    Missing,
    Abandoned,
}

#[tauri::command(async)]
fn open_path(window: WebviewWindow, state: State<AppState>, path: String) -> Opened {
    let path = std::path::absolute(&path).unwrap_or_else(|_| PathBuf::from(&path));
    match fs::metadata(&path) {
        Ok(m) if m.is_file() => {}
        _ => return Opened::Missing,
    }
    let win = state.win(&window);
    let seq = win.opens.fetch_add(1, Ordering::AcqRel) + 1;
    let decoded =
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| read_file(&path, &state.temp, seq, &win.opens)))
            .unwrap_or(Err(OpenError::Damaged));
    let Decoded { kind, pcm, bits, words: own } = match decoded {
        Ok(d) => d,
        Err(OpenError::Refused) => return Opened::Refused,
        Err(OpenError::Damaged) => return Opened::Damaged,
        Err(OpenError::TooLarge) => return Opened::Toolarge,
        Err(OpenError::NoDecoder) => return Opened::Nodecoder,
        Err(OpenError::Abandoned) => return Opened::Abandoned,
    };
    // the window went while its song was being read: nothing keeps it
    if window.app_handle().get_webview_window(window.label()).is_none() {
        return Opened::Abandoned;
    }
    let (mut words, mut carry) = tags::read(&path);
    // a module's own title and artist, when the file has no tag of its own: shown, and carried into a save
    if let Some(own) = own.filter(|_| carry.is_none()) {
        use lofty::tag::Accessor;
        words.title = own.title().map(|s| s.to_string()).unwrap_or_default();
        words.artist = own.artist().map(|s| s.to_string()).unwrap_or_default();
        carry = Some(own);
    }
    let pcm = Arc::new(pcm);
    let recipe = Recipe::whole(&pcm);
    let level = level_gain(&pcm, &recipe.spans);
    let opened = Opened::Ok {
        dir: path.parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default(),
        name: file_name(&path),
        path: path.to_string_lossy().into_owned(),
        kind,
        size: fs::metadata(&path).map(|m| m.len()).unwrap_or(0),
        rate: pcm.rate,
        channels: pcm.channels,
        frames: pcm.frames,
        tags: words,
        beside: beside(&path),
        level,
    };
    engine(&window, &win).send(Command::Load(Some(pcm.clone())));
    *lock(&win.doc) =
        Some(Song { pcm, bits, came_from: kind, disk: OnDisk { kind, recipe, trusted: true }, carry, new_cover: None });
    dotany_base::opened_in(window.app_handle(), window.label(), &path);
    opened
}

/// A quick look at the first bytes, before anything unsaved is asked about. Audio's own: what the bytes are decides,
/// so raw AAC or Opus named like a song of ours is refused ("This file format isn't supported yet."), not called
/// damaged.
#[tauri::command(async)]
fn can_open(path: String) -> &'static str {
    match formats::sniff_file(Path::new(&path)) {
        Ok(Sniff::Is(_)) => "ok",
        Ok(Sniff::Refused) => "refused",
        Ok(Sniff::Damaged) => "damaged",
        Err(_) => "missing",
    }
}

/// A new cover: only JPEG and PNG, kept here until it's saved into a file.
#[derive(Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
enum Cover {
    Ok { cover: String },
    Cancelled,
    Refused,
}

#[tauri::command(async)]
fn pick_cover(window: WebviewWindow, state: State<AppState>, label: String) -> Cover {
    // the Mac: the family's open panel, as this window's sheet (and a test's answer in its place)
    #[cfg(target_os = "macos")]
    let Some(path) = ({
        let _ = label;
        dotany_base::platform::ask_open(&window, &["jpg", "jpeg", "png"], None, dotany_base::platform::Pick::File)
            .into_iter()
            .next()
    }) else {
        return Cover::Cancelled;
    };
    #[cfg(not(target_os = "macos"))]
    let Some(path) = rfd::FileDialog::new().set_parent(&window).add_filter(label, &["jpg", "jpeg", "png"]).pick_file()
    else {
        return Cover::Cancelled;
    };
    set_cover_from(&state.win(&window), &path)
}

fn set_cover_from(win: &Win, path: &Path) -> Cover {
    let Ok(data) = fs::read(path) else { return Cover::Refused };
    let Some(cover) = tags::data_address(&data) else { return Cover::Refused };
    if let Some(song) = &mut *lock(&win.doc) {
        song.new_cover = Some(data);
    }
    Cover::Ok { cover }
}

/// The same as the dialog's answer, for the tests.
#[tauri::command(async)]
fn set_cover_path(window: WebviewWindow, state: State<AppState>, path: String) -> Cover {
    set_cover_from(&state.win(&window), Path::new(&path))
}

// ---------- sound for the page ----------

fn pcm_now(state: &AppState, window: &WebviewWindow) -> Option<Arc<Pcm>> {
    lock(&state.win(window).doc).as_ref().map(|s| s.pcm.clone())
}

/// The wave: one peak per bar, as raw floats.
#[tauri::command(async)]
fn wave_bars(
    window: WebviewWindow,
    state: State<AppState>,
    spans: Vec<(u64, u64)>,
    from: f64,
    span: f64,
    bars: usize,
) -> tauri::ipc::Response {
    let Some(pcm) = pcm_now(&state, &window) else { return tauri::ipc::Response::new(Vec::new()) };
    let list = Timeline::new(pcm, &spans).bars(from, span, bars.min(16384));
    let mut out = Vec::with_capacity(list.len() * 4);
    for v in list {
        out.extend_from_slice(&v.to_le_bytes());
    }
    tauri::ipc::Response::new(out)
}

#[tauri::command(async)]
fn level_of(window: WebviewWindow, state: State<AppState>, spans: Vec<(u64, u64)>) -> f32 {
    pcm_now(&state, &window).map(|p| level_gain(&p, &spans)).unwrap_or(1.0)
}

#[tauri::command]
fn player_set(window: WebviewWindow, state: State<AppState>, recipe: Recipe, k: f32) {
    engine(&window, &state.win(&window)).send(Command::Set(recipe, k));
}

#[tauri::command]
fn player_play(window: WebviewWindow, state: State<AppState>, from: f64, until: Option<f64>) {
    engine(&window, &state.win(&window)).send(Command::Play { from, until });
}

#[tauri::command]
fn player_pause(window: WebviewWindow, state: State<AppState>) {
    engine(&window, &state.win(&window)).send(Command::Pause);
}

#[tauri::command]
fn player_seek(window: WebviewWindow, state: State<AppState>, at: f64) {
    engine(&window, &state.win(&window)).send(Command::Seek(at));
}

/// What the real device has really taken from this window's player. For tests.
#[tauri::command(async)]
fn player_stats(window: WebviewWindow, state: State<AppState>) -> serde_json::Value {
    let e = engine(&window, &state.win(&window));
    let s = &e.stats;
    serde_json::json!({
        "frames": s.device_frames.load(Ordering::Relaxed),
        "peak": f32::from_bits(s.device_peak.load(Ordering::Relaxed)),
        "rate": s.device_rate.load(Ordering::Relaxed),
        "channels": s.device_channels.load(Ordering::Relaxed),
        "lame": crate::lame::version(),
    })
}

/// "Keep mine": the family's, and what is on disk now is accepted as not ours, so nothing can be copied from it any more.
#[tauri::command(async)]
fn keep_mine(window: WebviewWindow, state: State<AppState>) {
    dotany_base::keep_mine_in(window.app_handle(), window.label());
    if let Some(s) = &mut *lock(&state.win(&window).doc) {
        s.disk.trusted = false;
    }
}

// ---------- saving ----------

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
enum Saved {
    #[serde(rename_all = "camelCase")]
    Saved {
        path: String,
        name: String,
        size: u64,
        route: Route,
        rate: u32,
        channels: usize,
    },
    Nothing,
    Failed {
        kind: WriteFailure,
        detail: String,
    },
    Toolarge,
}

/// The newest question wins: an estimate nobody is waiting for any more is abandoned. Each window's own questions: one
/// window's estimate never abandons another's.
#[tauri::command(async)]
fn estimate_save(
    window: WebviewWindow,
    state: State<AppState>,
    base: State<Base>,
    seq: u64,
    request: Request,
) -> Option<Plan> {
    let (win, label) = (state.win(&window), window.label());
    win.estimates.store(seq, Ordering::Relaxed);
    std::thread::sleep(std::time::Duration::from_millis(60));
    if win.estimates.load(Ordering::Relaxed) != seq {
        return None;
    }
    let size = base.current_in(label).as_deref().and_then(meta_of).map(|m| m.0).unwrap_or(0);
    let (pcm, bits, disk, came_from, cover) = {
        let doc = lock(&win.doc);
        let s = doc.as_ref()?;
        (s.pcm.clone(), s.bits, trust(&base, label, s), s.came_from, s.new_cover.as_ref().map(|c| c.len()).unwrap_or(0))
    };
    Some(export::plan(&pcm, bits, &disk, size, came_from, &request, cover))
}

/// What a save loses, said at once: the estimate's own notes, without encoding anything, so the card's quiet line can
/// change with its format in the same motion as its options. The size follows with estimate_save.
#[tauri::command(async)]
fn save_notes(
    window: WebviewWindow,
    state: State<AppState>,
    base: State<Base>,
    request: Request,
) -> Option<Vec<&'static str>> {
    let win = state.win(&window);
    let doc = lock(&win.doc);
    let s = doc.as_ref()?;
    let cover = s.new_cover.as_ref().map(|c| c.len()).unwrap_or(0);
    Some(export::notes(&s.pcm, s.bits, &trust(&base, window.label(), s), s.came_from, &request, cover))
}

// the window's file really has to be the one remembered before anything is copied from it
fn trust(base: &Base, label: &str, song: &Song) -> OnDisk {
    let mut disk = song.disk.clone();
    disk.trusted = disk.trusted && base.unchanged_since_read_in(label);
    disk
}

#[tauri::command(async)]
fn save_as(window: WebviewWindow, state: State<AppState>, base: State<Base>, path: String, request: Request) -> Saved {
    let (app, label) = (window.app_handle(), window.label());
    let win = state.win(&window);
    let target = PathBuf::from(&path);
    let source = base.current_in(label);
    let (pcm, bits, disk, carry, cover) = {
        let doc = lock(&win.doc);
        let Some(s) = &*doc else { return Saved::Nothing };
        (s.pcm.clone(), s.bits, trust(&base, label, s), s.carry.clone(), s.new_cover.clone())
    };
    let job = export::Job {
        pcm: &pcm,
        bits,
        disk: &disk,
        source: source.as_deref(),
        carry: carry.as_ref(),
        cover: cover.as_deref(),
    };
    let done = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| export::save(&target, &job, &request)));
    let way = match done {
        Ok(Ok(way)) => way,
        Ok(Err(e)) if e.detail.contains("too large") => return Saved::Toolarge,
        Ok(Err(e)) => return Saved::Failed { kind: e.kind, detail: e.detail },
        Err(_) => return Saved::Failed { kind: WriteFailure::Other, detail: "the encoder stopped".into() },
    };
    // the window is on the new file now, and what's in it is what this recipe made
    let (_, new_carry) = tags::read(&target);
    let shape = crate::encode::shape_for(request.kind, pcm.rate, pcm.channels, bits);
    if let Some(s) = &mut *lock(&win.doc) {
        s.disk = OnDisk { kind: request.kind, recipe: request.recipe.clone(), trusted: true };
        s.carry = new_carry;
        s.new_cover = None;
    }
    if source.as_deref().is_some_and(|p| same_file(p, &target)) {
        dotany_base::saved_in(app, label, &target);
    } else {
        // watched, stepped from and remembered there from now on
        dotany_base::opened_in(app, label, &target);
    }
    let size = meta_of(&target).map(|m| m.0).unwrap_or(0);
    let (rate, channels) = if way == Route::Encode { (shape.rate, shape.channels) } else { (pcm.rate, pcm.channels) };
    Saved::Saved { name: file_name(&target), path: path.clone(), size, route: way, rate, channels }
}

pub fn run() {
    let temp = dotany_base::settings::data_dir(&TOOL).join("tmp");
    store::sweep(&temp);
    dotany_base::builder_per_window(&TOOL, FileWindows { forget, extra: dotany_base::no_extra })
        .manage(AppState { windows: PerWindow::default(), temp })
        // the family's commands, but Audio answers `can_open` and `keep_mine` itself (dotany_base::handler! would list the family's)
        .invoke_handler(tauri::generate_handler![
            dotany_base::commands::set_setting,
            dotany_base::commands::forget_recent,
            dotany_base::commands::pick_open,
            dotany_base::commands::pick_save,
            dotany_base::commands::default_folder,
            dotany_base::commands::neighbour,
            dotany_base::commands::close_document,
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
            keep_mine,
            pick_cover,
            set_cover_path,
            wave_bars,
            level_of,
            player_set,
            player_play,
            player_pause,
            player_seek,
            player_stats,
            estimate_save,
            save_notes,
            save_as
        ])
        .run_family(tauri::generate_context!())
        .expect("error while running DotAny Audio");
}
