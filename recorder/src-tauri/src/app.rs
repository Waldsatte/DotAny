//! The Tauri side of DotAny Recorder: what makes this tool itself, and its commands. Everything else is dotany-base.
//! A take is a WAV in the recordings folder, written while it is recorded (capture.rs). The open file is the take
//! being listened to; Save As copies it out as WAV, FLAC or MP3 (encode.rs).
//!
//! Recorder keeps a file per window (`dotany_base::builder_per_window`): on the Mac one program shows each take in a
//! window of its own; on Windows each is a program of its own, with one window. There is one microphone and one
//! recording at a time for the whole program, made by the window that started it: only that window pauses, marks or
//! stops it, and it ends whole when that window closes or the program quits. Another window meanwhile lists, plays,
//! marks, saves and deletes finished takes; it can't start a second recording (`Busy`), nor stop, cut or delete the one
//! being made, nor open its file while it is being written.

use crate::capture::{self, Live, Mic, Recording};
use crate::encode::{self, Kind, Quality};
use crate::{stream, takes, wav};
use dotany_base::RunFamily;
use dotany_base::atomic::{WriteError, WriteFailure, atomic_produce};
use dotany_base::{FileWindows, PerWindow, Tool, file_name, lock};
use serde::Serialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State, WebviewWindow};

fn is_pcm_wav(path: &Path) -> bool {
    wav::open_info(path).is_some()
}

/// The window's take is put down: the player lets go of it. A window that has gone takes its row with it, and a
/// recording it was making ends there, whole, as it does when the program ends.
fn forget(app: &AppHandle, label: &str) {
    if app.get_webview_window(label).is_none() {
        let gone = app.state::<Docs>().0.remove(label);
        drop(gone);
        let mine = lock(&app.state::<Rec>().0).take_if(|t| t.by == label);
        if let Some(t) = mine {
            let _ = t.recording.finish();
        }
        return;
    }
    if let Some(doc) = app.state::<Docs>().0.get(label) {
        *lock(&doc.0) = None;
    }
}

/// Recorder keeps a take per window and says so in `FileWindows` (`forget`); this is never called.
fn nothing(_: &AppHandle) {}

pub static TOOL: Tool = Tool {
    name: "DotAny Recorder",
    folder: "Recorder",
    data_env: "DOTANY_RECORDER_DATA",
    user_dir: "Music",
    opens_files: true,
    extensions: &["wav"],
    sniff: is_pcm_wav,
    size: (1180.0, 800.0),
    min_size: (480.0, 420.0),
    on_close: nothing,
};

/// The file being listened to. `gen` changes with every open, so the player never plays an earlier file from its cache.
struct Open {
    path: PathBuf,
    gen: u64,
}

/// What one window is listening to; another window of the program (the Mac) has its own.
#[derive(Default)]
struct Doc(Mutex<Option<Open>>);

#[derive(Default)]
struct Docs(PerWindow<Doc>);

impl Docs {
    fn of(&self, window: &WebviewWindow) -> std::sync::Arc<Doc> {
        self.0.of(window.label())
    }
}

/// The take being recorded and the window recording it.
struct Taking {
    by: String,
    recording: Recording,
}

/// The take being recorded, if any: one for the whole program, as there is one microphone.
#[derive(Default)]
struct Rec(Mutex<Option<Taking>>);

/// One count for the whole program, so a take's address names one take anywhere.
static GEN: AtomicU64 = AtomicU64::new(0);
/// Takes a stopped-short run left behind are brought back once, by the first list any window asks for.
static RECOVERED: OnceLock<Vec<String>> = OnceLock::new();

fn live_path(rec: &Rec) -> Option<PathBuf> {
    lock(&rec.0).as_ref().map(|t| t.recording.path().to_path_buf())
}

/// The window recording the take at `path`, if `path` is the take being recorded.
fn recording_by(rec: &Rec, path: &Path) -> Option<String> {
    lock(&rec.0).as_ref().filter(|t| dotany_base::same_file(t.recording.path(), path)).map(|t| t.by.clone())
}

// ---------- opening ----------

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
enum Opened {
    Ok {
        path: String,
        dir: String,
        name: String,
        size: u64,
        /// The take's own name, which can differ from its file's.
        title: String,
        rate: u32,
        channels: u16,
        frames: u64,
        peaks: Vec<f32>,
        marks: Vec<f64>,
        created: u64,
        take: bool,
        live: bool,
        gen: u64,
    },
    Refused,
    Missing,
    Damaged,
}

fn open_at(window: &WebviewWindow, path: &Path) -> Opened {
    let (app, label) = (window.app_handle(), window.label());
    let rec = app.state::<Rec>();
    let take = takes::is_take(app, path);
    let meta = if take { takes::read_meta(path) } else { None };
    let dir = path.parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
    match recording_by(&rec, path) {
        Some(by) if by == label => {
            // the take being recorded: what the page needs to show it, without reading a file that's still growing
            let r = lock(&rec.0);
            let Some(t) = r.as_ref() else { return Opened::Missing };
            return Opened::Ok {
                path: path.to_string_lossy().into_owned(),
                dir,
                name: file_name(path),
                size: 0,
                title: t.recording.meta.name.clone(),
                rate: t.recording.rate,
                channels: 1,
                frames: 0,
                peaks: Vec::new(),
                marks: Vec::new(),
                created: t.recording.meta.created,
                take: true,
                live: true,
                gen: 0,
            };
        }
        Some(by) => {
            // another window is recording it (the Mac): it can't be read while it grows, and that window comes forward
            if let Some(w) = app.get_webview_window(&by) {
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
            return Opened::Damaged;
        }
        None => {}
    }
    let Ok((info, peaks)) = wav::peaks(path) else { return Opened::Damaged };
    let gen = GEN.fetch_add(1, Ordering::Relaxed) + 1;
    *lock(&app.state::<Docs>().of(window).0) = Some(Open { path: path.to_path_buf(), gen });
    dotany_base::opened_in(app, label, path);
    let fallback = path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    Opened::Ok {
        path: path.to_string_lossy().into_owned(),
        dir,
        name: file_name(path),
        size: std::fs::metadata(path).map(|m| m.len()).unwrap_or(0),
        title: meta.as_ref().map_or(fallback, |m| m.name.clone()),
        rate: info.rate,
        channels: info.channels,
        frames: info.frames(),
        peaks,
        marks: meta.as_ref().map_or_else(Vec::new, |m| m.marks.clone()),
        created: meta.as_ref().map_or(0, |m| m.created),
        take,
        live: false,
        gen,
    }
}

#[tauri::command(async)]
fn open_file(window: WebviewWindow, rec: State<Rec>, path: String) -> Opened {
    let path = std::path::absolute(&path).unwrap_or_else(|_| PathBuf::from(&path));
    if !path.is_file() {
        return Opened::Missing;
    }
    if !TOOL.allowed_name(&file_name(&path)) {
        return Opened::Refused;
    }
    if !is_pcm_wav(&path) && recording_by(&rec, &path).is_none() {
        return Opened::Refused;
    }
    open_at(&window, &path)
}

// ---------- the list ----------

#[derive(Serialize)]
struct Takes {
    folder: String,
    rows: Vec<takes::Row>,
}

/// Every take, newest first. The first time, takes a stopped-short run left behind are brought back first.
#[tauri::command(async)]
fn list_takes(app: AppHandle, rec: State<Rec>) -> Takes {
    RECOVERED.get_or_init(|| takes::recover(&app));
    let live = live_path(&rec);
    Takes { folder: takes::folder(&app).to_string_lossy().into_owned(), rows: takes::list(&app, live.as_deref()) }
}

/// Deletes takes; never the one being recorded, nor one another window has open (the Mac): those stay in the list, as a
/// take another program holds does.
#[tauri::command(async)]
fn delete_takes(window: WebviewWindow, rec: State<Rec>, docs: State<Docs>, paths: Vec<String>) -> Vec<String> {
    let app = window.app_handle();
    let live = live_path(&rec);
    let held = |p: &str| {
        app.webview_windows().keys().filter(|l| l.as_str() != window.label()).any(|l| {
            let doc = docs.0.get(l);
            doc.is_some_and(|d| lock(&d.0).as_ref().is_some_and(|o| dotany_base::same_file(Path::new(p), &o.path)))
        })
    };
    let paths: Vec<String> = paths.into_iter().filter(|p| !held(p)).collect();
    // the player lets go of a take before it goes
    let doc = docs.of(&window);
    let mut open = lock(&doc.0);
    if open.as_ref().is_some_and(|o| paths.iter().any(|p| dotany_base::same_file(Path::new(p), &o.path))) {
        *open = None;
    }
    drop(open);
    takes::delete(app, &paths, live.as_deref())
}

/// Save As names the take too. Only the name it's listed by changes; the file stays where it is.
#[tauri::command(async)]
fn rename_take(app: AppHandle, path: String, name: String) -> bool {
    let p = PathBuf::from(&path);
    if !takes::is_take(&app, &p) {
        return false;
    }
    let Some(mut meta) = takes::read_meta(&p) else { return false };
    meta.name = name;
    takes::write_meta(&p, &meta).is_ok()
}

// ---------- recording ----------

#[tauri::command(async)]
fn list_mics() -> Vec<Mic> {
    capture::mics()
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
enum Started {
    Ok {
        path: String,
        title: String,
        rate: u32,
        created: u64,
    },
    /// No microphone to record from, or it couldn't be opened.
    Nomic,
    /// A recording is already being made, in this window or another (the Mac).
    Busy,
    Failed {
        detail: String,
    },
}

#[tauri::command(async)]
fn start_recording(
    window: WebviewWindow,
    rec: State<Rec>,
    name: String,
    mic: Option<String>,
    on_data: Channel<Live>,
) -> Started {
    let (app, label) = (window.app_handle(), window.label());
    // the Mac asks the person first (the first time), and a no means silence: the take doesn't start
    #[cfg(target_os = "macos")]
    if std::env::var_os("DOTANY_RECORDER_TEST_TONE").is_none() && lock(&rec.0).is_none() && !crate::mic_mac::allowed() {
        return Started::Failed { detail: "the system doesn't let this program hear the microphone".into() };
    }
    let mut slot = lock(&rec.0);
    if slot.is_some() {
        return Started::Busy;
    }
    // what was open in this window is put down: it now shows the take being made
    dotany_base::close_file_in(app, label);
    let path = takes::new_path(&takes::folder(app), &name);
    match capture::start(path, name, mic, on_data) {
        Ok(r) => {
            let out = Started::Ok {
                path: r.path().to_string_lossy().into_owned(),
                title: r.meta.name.clone(),
                rate: r.rate,
                created: r.meta.created,
            };
            *slot = Some(Taking { by: label.to_string(), recording: r });
            out
        }
        Err(e) if e == "nomic" => Started::Nomic,
        Err(detail) => Started::Failed { detail },
    }
}

/// Pauses or resumes the recording, from the window making it only.
#[tauri::command(async)]
fn pause_recording(window: WebviewWindow, rec: State<Rec>, paused: bool) -> u64 {
    let slot = lock(&rec.0);
    let Some(t) = slot.as_ref().filter(|t| t.by == window.label()) else { return 0 };
    t.recording.shared.paused.store(paused, Ordering::Release);
    t.recording.shared.frames.load(Ordering::Acquire)
}

/// A mark: now, in the window recording; at `at` seconds in a window listening to a take.
#[tauri::command(async)]
fn add_mark(window: WebviewWindow, rec: State<Rec>, docs: State<Docs>, at: Option<f64>) -> Option<Vec<f64>> {
    if let Some(t) = lock(&rec.0).as_mut().filter(|t| t.by == window.label()) {
        return Some(t.recording.mark());
    }
    let path = lock(&docs.of(&window).0).as_ref().map(|o| o.path.clone())?;
    if !takes::is_take(window.app_handle(), &path) {
        return None;
    }
    let mut meta = takes::read_meta(&path)?;
    meta.marks.push(at?.max(0.0));
    meta.marks.sort_by(|a, b| a.total_cmp(b));
    takes::write_meta(&path, &meta).ok()?;
    Some(meta.marks)
}

/// Stops and closes the take with its final header, from the window making it only. The page then opens it to be
/// listened to. Answers its path.
#[tauri::command(async)]
fn stop_recording(window: WebviewWindow, rec: State<Rec>) -> Option<String> {
    let t = lock(&rec.0).take_if(|t| t.by == window.label())?;
    let path = t.recording.path().to_string_lossy().into_owned();
    let _ = t.recording.finish();
    Some(path)
}

/// When the program ends with a take still recording, it's closed properly on the way out.
fn finish_on_exit(app: &AppHandle) {
    if let Some(t) = lock(&app.state::<Rec>().0).take() {
        let _ = t.recording.finish();
    }
}

// ---------- listening: the open take, a stretch at a time, to the page's <audio> ----------

/// The take open in the window whose page asked (`label`, the WebView's own): a window never plays another's.
fn take_response(
    app: &AppHandle,
    label: &str,
    path: &str,
    range: Option<&str>,
) -> tauri::http::Response<std::borrow::Cow<'static, [u8]>> {
    let gen = path.trim_matches('/').parse::<u64>().ok();
    let doc = app.state::<Docs>().0.get(label);
    let file = doc.and_then(|d| lock(&d.0).as_ref().filter(|o| Some(o.gen) == gen).map(|o| o.path.clone()));
    match file {
        Some(f) => stream::respond(&f, "audio/wav", range),
        None => stream::refused(404),
    }
}

// ---------- saving ----------

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
enum Saved {
    Saved { path: String, name: String, size: u64 },
    Nothing,
    Failed { kind: WriteFailure, detail: String },
}

#[tauri::command(async)]
fn save_file(window: WebviewWindow, docs: State<Docs>, path: String, fmt: Kind, quality: Quality) -> Saved {
    let Some(from) = lock(&docs.of(&window).0).as_ref().map(|o| o.path.clone()) else { return Saved::Nothing };
    let target = PathBuf::from(&path);
    let done = atomic_produce(&target, |temp| encode::write(&from, temp, fmt, quality).map_err(WriteError::from));
    if let Err(e) = done {
        return Saved::Failed { kind: e.kind, detail: e.detail };
    }
    dotany_base::saved_in(window.app_handle(), window.label(), &target);
    Saved::Saved { name: file_name(&target), size: std::fs::metadata(&target).map(|m| m.len()).unwrap_or(0), path }
}

// ---------- DotAny Audio, for editing ----------

/// Opens a take in DotAny Audio when it is installed beside this program: `path`, one row of the list (a finished
/// take, never the one being recorded), or with none the one open in this window.
#[tauri::command(async)]
fn open_in_audio(window: WebviewWindow, rec: State<Rec>, docs: State<Docs>, path: Option<String>) -> &'static str {
    let app = window.app_handle();
    let path = match path {
        Some(p) => {
            let p = PathBuf::from(p);
            if !takes::is_take(app, &p)
                || !p.is_file()
                || live_path(&rec).is_some_and(|l| dotany_base::same_file(&l, &p))
            {
                return "nothing";
            }
            p
        }
        None => match lock(&docs.of(&window).0).as_ref().map(|o| o.path.clone()) {
            Some(p) => p,
            None => return "nothing",
        },
    };
    match dotany_base::platform::sibling_tool("dotany-audio") {
        Some(audio) if dotany_base::platform::launch(&audio, &path) => "ok",
        Some(_) => "failed",
        None => "missing",
    }
}

pub fn run() {
    dotany_base::builder_per_window(&TOOL, FileWindows { forget, extra: dotany_base::no_extra })
        // the asking window's take and nothing else, off the main thread
        .register_asynchronous_uri_scheme_protocol("take", |ctx, request, responder| {
            let path = request.uri().path().to_string();
            let range = request.headers().get("range").and_then(|v| v.to_str().ok()).map(str::to_string);
            let (app, label) = (ctx.app_handle().clone(), ctx.webview_label().to_string());
            std::thread::spawn(move || responder.respond(take_response(&app, &label, &path, range.as_deref())));
        })
        .manage(Docs::default())
        .manage(Rec::default())
        .invoke_handler(dotany_base::handler![
            open_file,
            list_takes,
            delete_takes,
            rename_take,
            list_mics,
            start_recording,
            pause_recording,
            add_mark,
            stop_recording,
            save_file,
            open_in_audio
        ])
        .run_family_then(tauri::generate_context!(), finish_on_exit)
        .expect("error while running DotAny Recorder");
}
