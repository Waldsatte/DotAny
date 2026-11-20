//! The window and what the page asks of it. The page owns the four steps and their pacing; this side does the work:
//! staging in a thread while the page draws the ticks, then the commit when the page has drawn the last one.

use crate::engine::{self, Choice, Failure, Staged};
use crate::manifest::{Record, mb};
use crate::payload::Payload;
use crate::reg::Hive;
use crate::system;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use tauri::webview::WebviewWindowBuilder;
use tauri::{AppHandle, Emitter, Manager, State, WebviewUrl, WindowEvent};

pub enum Mode {
    Install(Payload),
    /// The hub's Install and Update: this release over the install in the plan's folder, its tools plus any added.
    Update(Payload, Choice),
    Uninstall(Record),
}

#[derive(Default)]
enum Job {
    #[default]
    Idle,
    Staging(JoinHandle<Result<Staged, Failure>>),
    Done(Record),
}

pub struct Setup {
    mode: Mode,
    hive: Hive,
    fonts: Vec<(String, Vec<u8>)>,
    job: Mutex<Job>,
    choice: Mutex<Option<Choice>>,
    cancel: Arc<AtomicBool>,
    /// Held while a commit runs; closing the window waits for it rather than cut it off.
    committing: Mutex<()>,
}

const FONT_FILES: [&str; 4] = [
    "fonts/HarmonyOS_SansSC_Regular.ttf",
    "fonts/HarmonyOS_SansSC_Bold.ttf",
    "fonts/StackSansText-wght.ttf",
    "fonts/StackSansHeadline-wght.ttf",
];

/// The family's fonts, and the fonts of the one language this run speaks, if it needs its own (Traditional Chinese,
/// Japanese, Korean).
fn font_files() -> Vec<&'static str> {
    let own: &[&str] = match crate::words::system_lang() {
        "zh-Hant" => &["fonts/HarmonyOS_Sans_TC.ttf"],
        "ja" => &["fonts/NotoSansJP-DotAny-Regular.ttf", "fonts/NotoSansJP-DotAny-Bold.ttf"],
        "ko" => &["fonts/Pretendard-Regular.otf", "fonts/Pretendard-Bold.otf"],
        _ => &[],
    };
    FONT_FILES.iter().chain(own).copied().collect()
}

fn read_fonts(mode: &Mode) -> Vec<(String, Vec<u8>)> {
    let files = font_files();
    match mode {
        Mode::Install(p) | Mode::Update(p, _) => p.read_front(&files).unwrap_or_default(),
        Mode::Uninstall(r) => files
            .iter()
            .filter_map(|f| std::fs::read(Path::new(&r.root).join(f)).ok().map(|d| (f.to_string(), d)))
            .collect(),
    }
}

pub fn run(mode: Mode, temp: PathBuf) {
    let fonts = read_fonts(&mode);
    let setup = Setup {
        mode,
        hive: Hive::from_env(),
        fonts,
        job: Mutex::default(),
        choice: Mutex::default(),
        cancel: Arc::default(),
        committing: Mutex::default(),
    };
    let temp_after = temp.clone();
    let dark = match std::env::var("DOTANY_SETUP_THEME").as_deref() {
        Ok("dark") => true,
        Ok("light") => false,
        _ => system::system_is_dark(),
    };
    tauri::Builder::default()
        .register_asynchronous_uri_scheme_protocol("dotfont", |ctx, request, responder| {
            let name = format!("fonts{}", request.uri().path());
            let app = ctx.app_handle().clone();
            std::thread::spawn(move || {
                let setup = app.state::<Setup>();
                let found = setup.fonts.iter().find(|(n, _)| n == &name).map(|(_, d)| d.clone());
                let builder = tauri::http::Response::builder().header("Access-Control-Allow-Origin", "*").header("Content-Type", if name.ends_with(".otf") { "font/otf" } else { "font/ttf" });
                responder.respond(match found {
                    Some(d) => builder.status(200).body(d).unwrap(),
                    None => builder.status(404).body(Vec::new()).unwrap(),
                });
            });
        })
        .manage(setup)
        .invoke_handler(tauri::generate_handler![boot, need, pick_folder, install, cancel, commit, set_autostart, open_hub, uninstall, minimize, quit, ready, open_now])
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { .. } = event {
                stop(&window.state::<Setup>());
            }
        })
        .setup(move |app| {
            let bg = if dark { tauri::window::Color(0x16, 0x16, 0x16, 255) } else { tauri::window::Color(0xEC, 0xEB, 0xE7, 255) };
            let theme = std::env::var("DOTANY_SETUP_THEME").ok();
            // the language and its table, chosen once in Rust (words.rs); Chinese needs no table
            let lang = crate::words::system_lang();
            let script = format!(
                "window.__SETUP__={};",
                json!({ "theme": theme, "lang": lang, "words": serde_json::from_str::<Value>(crate::words::table(lang)).unwrap_or_default(), "en": serde_json::from_str::<Value>(crate::words::table("en")).unwrap_or_default(), "test": std::env::var_os("DOTANY_SETUP_HIVE").is_some() })
            );
            WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
                .title("DotAny")
                .inner_size(720.0, 480.0)
                .resizable(false)
                .maximizable(false)
                .decorations(false)
                .center()
                // shown by the page once its fonts are in, so nothing is drawn twice
                .visible(false)
                .background_color(bg)
                .theme(Some(if dark { tauri::Theme::Dark } else { tauri::Theme::Light }))
                .data_directory(temp.join("webview"))
                .initialization_script(&script)
                .build()?;
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("the installer's window")
        .run(move |_, event| {
            if let tauri::RunEvent::Exit = event {
                // the WebView's data and, for the uninstaller, its own copy: gone once this process is
                system::remove_after_exit(&temp_after);
            }
        });
}

/// Cancels whatever is under way and leaves nothing behind. A commit is never interrupted; it takes a moment.
fn stop(setup: &Setup) {
    setup.cancel.store(true, Ordering::Relaxed);
    // a commit that panicked has ended all the same: closing and quitting still work, and `commit` still refuses
    let _wait = setup.committing.lock().unwrap_or_else(PoisonError::into_inner);
    let job = std::mem::take(&mut *setup.job.lock().unwrap());
    let staged = match job {
        Job::Staging(handle) => handle.join().ok().and_then(|r| r.ok()),
        Job::Done(r) => {
            *setup.job.lock().unwrap() = Job::Done(r);
            None
        }
        Job::Idle => None,
    };
    if let Some(s) = staged {
        engine::discard(&s.root, &s.made);
    }
}

fn payload<'a>(setup: &'a Setup) -> Result<&'a Payload, String> {
    match &setup.mode {
        Mode::Install(p) | Mode::Update(p, _) => Ok(p),
        Mode::Uninstall(_) => Err("not an installer".into()),
    }
}

fn open_list(payload: &Payload, root: &Path) -> Vec<Value> {
    crate::open_in(payload, root).into_iter().map(|(id, name)| json!({ "id": id, "name": name })).collect()
}

/// The update mode: the DotAny programs still open, which the person is asked to close first.
#[tauri::command]
fn open_now(setup: State<Setup>) -> Vec<Value> {
    match &setup.mode {
        Mode::Update(p, plan) => open_list(p, &plan.root),
        _ => Vec::new(),
    }
}

#[tauri::command]
fn boot(setup: State<Setup>) -> Value {
    match &setup.mode {
        Mode::Install(p) => {
            let release = &p.release;
            let sizes = engine::sizes(p);
            let existing = engine::existing(&setup.hive);
            let programs: Vec<Value> = release
                .programs()
                .map(|c| {
                    let size = sizes.per_program.iter().find(|(id, _)| id == &c.id).map(|(_, m)| *m).unwrap_or(0);
                    json!({ "id": c.id, "name": c.name, "mb": size, "required": c.required, "placeholder": c.placeholder })
                })
                .collect();
            json!({
                "mode": "install",
                "version": release.version,
                "programs": programs,
                "full": sizes.full,
                "root": existing.as_ref().map(|r| r.root.clone()).unwrap_or_else(|| system::default_root().display().to_string()),
                "existing": existing.map(|r| json!({ "version": r.version, "programs": r.components.iter().map(|c| c.id.clone()).collect::<Vec<_>>() })),
            })
        }
        Mode::Update(p, plan) => {
            let release = &p.release;
            let programs: Vec<Value> =
                release.programs().map(|c| json!({ "id": c.id, "name": c.name, "required": c.required })).collect();
            let ids: Vec<&String> =
                release.programs().filter(|c| c.required || plan.programs.contains(&c.id)).map(|c| &c.id).collect();
            let autostart =
                Record::load(&plan.root).or_else(|| engine::existing(&setup.hive)).map(|r| r.autostart).unwrap_or(true);
            json!({ "mode": "update", "version": release.version, "programs": programs, "ids": ids, "root": plan.root.display().to_string(), "open": open_list(p, &plan.root), "autostart": autostart })
        }
        Mode::Uninstall(r) => {
            let programs: Vec<Value> = r
                .components
                .iter()
                .filter(|c| c.exe.is_some())
                .map(|c| json!({ "id": c.id, "name": c.name }))
                .collect();
            json!({ "mode": "uninstall", "version": r.version, "programs": programs, "mb": mb(r.files.iter().map(|f| f.size).sum()), "root": r.root })
        }
    }
}

#[tauri::command]
fn need(setup: State<Setup>, programs: Vec<String>) -> Result<u64, String> {
    Ok(mb(engine::need_bytes(payload(&setup)?, &programs)))
}

/// The system's own folder picker. A folder that isn't already called DotAny gets one inside it.
#[tauri::command]
async fn pick_folder(app: AppHandle, current: String) -> Option<String> {
    let window = app.get_webview_window("main")?;
    let start = Path::new(&current).parent().filter(|p| p.exists()).map(Path::to_path_buf);
    let (tx, rx) = std::sync::mpsc::channel();
    let w = window.clone();
    window
        .run_on_main_thread(move || {
            let mut dialog = rfd::FileDialog::new().set_parent(&w);
            if let Some(s) = start {
                dialog = dialog.set_directory(s);
            }
            let _ = tx.send(dialog.pick_folder());
        })
        .ok()?;
    let picked = tauri::async_runtime::spawn_blocking(move || rx.recv().ok().flatten()).await.ok()??;
    let named = picked.file_name().map(|n| n.to_string_lossy().eq_ignore_ascii_case("DotAny")).unwrap_or(false);
    let root = if named { picked } else { picked.join("DotAny") };
    Some(root.display().to_string())
}

/// Starts staging. The page hears `staged` with each component's id, then `staging-done`.
#[tauri::command]
fn install(app: AppHandle, setup: State<Setup>, root: String, programs: Vec<String>) -> Result<(), String> {
    payload(&setup)?;
    stop(&setup);
    setup.cancel.store(false, Ordering::Relaxed);
    let choice = Choice { root: PathBuf::from(root), programs };
    *setup.choice.lock().unwrap() = Some(choice.clone());
    let cancel = setup.cancel.clone();
    let handle = std::thread::spawn(move || {
        let setup = app.state::<Setup>();
        let Ok(p) = payload(&setup) else { unreachable!() };
        let result = engine::stage_with(
            p,
            &choice,
            &cancel,
            |id| {
                let _ = app.emit("staged", id);
            },
            |done, total| {
                let _ = app.emit("unpacked", json!({ "done": done, "total": total }));
            },
        );
        let _ = app.emit(
            "staging-done",
            match &result {
                Ok(_) => json!({ "ok": true }),
                Err(f) => json!({ "ok": false, "failure": f }),
            },
        );
        result
    });
    *setup.job.lock().unwrap() = Job::Staging(handle);
    Ok(())
}

#[tauri::command]
async fn cancel(app: AppHandle) {
    let _ = tauri::async_runtime::spawn_blocking(move || stop(&app.state::<Setup>())).await;
}

/// Puts the staged install in place. Answers how many programs and how many MB, or why it couldn't.
#[tauri::command]
async fn commit(app: AppHandle, lang: String, autostart: bool) -> Result<Value, Failure> {
    tauri::async_runtime::spawn_blocking(move || {
        let setup = app.state::<Setup>();
        let _hold = setup.committing.lock().unwrap();
        let job = std::mem::take(&mut *setup.job.lock().unwrap());
        let staged = match job {
            Job::Staging(handle) => {
                handle.join().map_err(|_| Failure::Other { detail: "staging stopped".into() })??
            }
            _ => return Err(Failure::Other { detail: "nothing staged".into() }),
        };
        let p = payload(&setup).map_err(|detail| Failure::Other { detail })?;
        let choice = setup.choice.lock().unwrap().clone().unwrap();
        let record = engine::commit(p, staged, &choice, &setup.hive, &lang, autostart)?;
        let programs = record.components.iter().filter(|c| c.exe.is_some()).count();
        let bytes: u64 = record.files.iter().map(|f| f.size).sum();
        let ids: Vec<String> = record.components.iter().map(|c| c.id.clone()).collect();
        *setup.job.lock().unwrap() = Job::Done(record);
        Ok(json!({ "programs": programs, "mb": mb(bytes), "ids": ids }))
    })
    .await
    .map_err(|e| Failure::Other { detail: e.to_string() })?
}

fn installed_root(setup: &Setup) -> Option<PathBuf> {
    match &*setup.job.lock().unwrap() {
        Job::Done(r) => Some(PathBuf::from(&r.root)),
        _ => None,
    }
}

#[tauri::command]
fn set_autostart(setup: State<Setup>, on: bool) -> Result<(), String> {
    let root = installed_root(&setup).ok_or("not installed")?;
    engine::set_autostart(&setup.hive, &root, on).map_err(|e| e.to_string())
}

#[tauri::command]
fn open_hub(app: AppHandle, setup: State<Setup>) {
    if let Some(root) = installed_root(&setup) {
        system::start(&root.join(engine::HUB), &[]);
    }
    app.exit(0);
}

#[tauri::command]
async fn uninstall(app: AppHandle, remove_settings: bool) -> Result<Value, Failure> {
    tauri::async_runtime::spawn_blocking(move || {
        let setup = app.state::<Setup>();
        let Mode::Uninstall(record) = &setup.mode else {
            return Err(Failure::Other { detail: "not an uninstaller".into() });
        };
        let removed = engine::uninstall(record, &setup.hive, remove_settings)?;
        Ok(json!({ "programs": removed.programs, "mb": mb(removed.bytes), "left": removed.left }))
    })
    .await
    .map_err(|e| Failure::Other { detail: e.to_string() })?
}

#[tauri::command]
fn minimize(app: AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.minimize();
    }
}

#[tauri::command]
async fn quit(app: AppHandle) {
    let a = app.clone();
    let _ = tauri::async_runtime::spawn_blocking(move || stop(&a.state::<Setup>())).await;
    app.exit(0);
}

#[tauri::command]
fn ready(app: AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
    }
}
