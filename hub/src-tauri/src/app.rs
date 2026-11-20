//! DotAny itself: the family's home. It lists the files every tool opened, starts the tool for one, makes new files in
//! them, converts files into another format, and holds the settings every tool follows. It never opens a file itself:
//! a double-click still goes straight to the tool.
//!
//! One DotAny runs at a time. `dotany.exe --convert a.png b.png` (the right-click menu) opens only the converter, in a
//! small window of its own; `--autostart` (Windows starting) gets the window ready without showing it.

use crate::update::Problem;
use crate::update::{self, Status};
use crate::{convert, feedback, system};
use dotany_base::RunFamily;
use dotany_base::{Base, Tool, WindowSpec, family, file_name};
use serde::Serialize;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Instant;
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};

fn any_bytes(_: &Path) -> bool {
    true
}

fn nothing(_: &AppHandle) {}

/// One of the family's programs, as DotAny knows it: its exe beside DotAny's, its settings folder (the name it writes on
/// the shared recent list), and the kinds of file it opens.
pub struct Member {
    pub key: &'static str,
    pub name: &'static str,
    pub exe: &'static str,
    pub folder: &'static str,
    /// The kinds of file it opens that Default Apps lists, one chip each.
    pub exts: &'static [&'static str],
    /// The rare formats it opens too (each also in its `formats` in installer/payload.json): a dropped or chosen file
    /// goes to it, and the open dialog offers them, but Default Apps has no chip for them.
    pub rare: &'static [&'static str],
}

pub const FAMILY: &[Member] = &[
    Member {
        key: "text",
        name: "Text",
        exe: "dotany-text",
        folder: "Text",
        exts: &["docx", "doc", "rtf", "odt", "md", "txt", "json", "srt"],
        rare: &["nfo", "diz", "ans"],
    },
    Member {
        key: "image",
        name: "Image",
        exe: "dotany-image",
        folder: "Image",
        exts: &["png", "jpg", "jpeg", "webp", "gif", "svg", "bmp", "tiff", "ico"],
        rare: &["tga", "qoi", "pbm", "pgm", "ppm", "pnm", "pam", "ff", "farbfeld", "pcx", "iff", "ilbm", "lbm"],
    },
    Member {
        key: "video",
        name: "Video",
        exe: "dotany-video",
        folder: "Video",
        exts: &["mp4", "mkv", "mov", "webm", "avi", "wmv"],
        rare: &[],
    },
    Member {
        key: "audio",
        name: "Audio",
        exe: "dotany-audio",
        folder: "Audio",
        exts: &["mp3", "flac", "wav", "aiff", "ogg", "m4a"],
        rare: &["mod", "xm", "s3m", "it", "au", "snd", "voc"],
    },
    Member { key: "pdf", name: "PDF", exe: "dotany-pdf", folder: "PDF", exts: &["pdf"], rare: &[] },
    Member {
        key: "archive",
        name: "Archive",
        exe: "dotany-archive",
        folder: "Archive",
        exts: &["zip", "7z", "rar", "tar", "gz"],
        rare: &["tgz", "bz2", "tbz2", "tbz", "xz", "txz", "zst", "tzst", "lzh", "lha", "cpio", "ar", "deb", "rpm"],
    },
    Member {
        key: "sheets",
        name: "Sheets",
        exe: "dotany-sheets",
        folder: "Sheets",
        exts: &["xlsx", "csv", "tsv", "xls", "ods"],
        rare: &["xlsm", "xlsb", "fods", "dbf", "sqlite", "sqlite3", "db"],
    },
    Member {
        key: "slides",
        name: "Slides",
        exe: "dotany-slides",
        folder: "Slides",
        exts: &["pptx", "ppsx", "potx"],
        rare: &["pptm", "ppsm", "potm"],
    },
    Member { key: "recorder", name: "Recorder", exe: "dotany-recorder", folder: "Recorder", exts: &[], rare: &[] },
    Member {
        key: "books",
        name: "Books",
        exe: "dotany-books",
        folder: "Books",
        exts: &["epub", "fb2", "cbz", "djvu", "mobi"],
        rare: &["cbt", "cb7", "fbz"],
    },
    Member {
        key: "model",
        name: "3D",
        exe: "dotany-model",
        folder: "3D",
        exts: &["glb", "gltf", "obj", "stl", "fbx", "3mf"],
        rare: &["ply", "off", "gcode", "gco", "vox"],
    },
];

/// Every name the family's files go by; the converter decides which of them it takes. Not "db": the converter takes a
/// SQLite .db dropped on it, but Thumbs.db and many programs' own .db files would get a right-click entry too.
const KNOWN: &[&str] = &[
    "png", "jpg", "jpeg", "jfif", "webp", "tif", "tiff", "bmp", "gif", "ico", "mp3", "flac", "wav", "aif", "aiff",
    "ogg", "oga", "xlsx", "xlsm", "xls", "xlsb", "ods", "fods", "dbf", "sqlite", "sqlite3", "csv", "tsv", "tab",
    "pptx", "ppsx", "potx", "pdf", "docx", "doc", "rtf", "odt", "md", "markdown", "txt", "glb", "gltf", "obj", "stl",
    "3mf", "fbx", "zip", "7z", "tar", "tgz", "rar", "gz", "bz2", "tbz2", "tbz", "xz", "txz", "zst", "tzst", "lzh",
    "lha", "cpio", "ar", "deb", "rpm", "tga", "qoi", "pbm", "pgm", "ppm", "pnm", "pam", "ff", "farbfeld", "pcx", "iff",
    "ilbm", "lbm", "ply", "off",
];

/// Every kind of file the converter really takes in this build, for the right-click menu and the open dialog: what
/// `convert::targets` offers something for, file by file.
fn convertible() -> &'static [&'static str] {
    static LIST: std::sync::OnceLock<Vec<&'static str>> = std::sync::OnceLock::new();
    LIST.get_or_init(|| {
        KNOWN.iter().copied().filter(|e| convert::targets(&[PathBuf::from(format!("x.{e}"))]).is_ok()).collect()
    })
}

fn can_convert(path: &str) -> bool {
    convert::targets(&[PathBuf::from(path)]).is_ok()
}

fn member(key: &str) -> Option<&'static Member> {
    FAMILY.iter().find(|m| m.key == key)
}

fn ext_of(p: &str) -> String {
    Path::new(p).extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

fn member_for_ext(ext: &str) -> Option<&'static Member> {
    FAMILY.iter().find(|m| m.exts.contains(&ext)).or_else(|| FAMILY.iter().find(|m| m.rare.contains(&ext)))
}

static ALL_EXTS: std::sync::OnceLock<Vec<&'static str>> = std::sync::OnceLock::new();

pub static TOOL: Tool = Tool {
    name: "DotAny",
    folder: "Hub",
    data_env: "DOTANY_HUB_DATA",
    user_dir: "Documents",
    opens_files: false,
    extensions: &[],
    sniff: any_bytes,
    size: (1180.0, 800.0),
    min_size: (640.0, 480.0),
    on_close: nothing,
};

/// Where the tools are: beside DotAny, or DOTANY_HUB_TOOLS_DIR for tests.
fn tools_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("DOTANY_HUB_TOOLS_DIR") {
        return PathBuf::from(d);
    }
    std::env::current_exe().ok().and_then(|p| p.parent().map(Path::to_path_buf)).unwrap_or_default()
}

#[cfg(windows)]
fn exe_of(m: &Member) -> Option<PathBuf> {
    let p = tools_dir().join(format!("{}.exe", m.exe));
    p.is_file().then_some(p)
}

/// The Mac: the tool's `.app` in DotAny's folder, or its program beside DotAny's in a build (the family's
/// `sibling_tool`); a test's folder holds plain programs.
#[cfg(not(windows))]
fn exe_of(m: &Member) -> Option<PathBuf> {
    if std::env::var_os("DOTANY_HUB_TOOLS_DIR").is_some() {
        let p = tools_dir().join(m.exe);
        return p.is_file().then_some(p);
    }
    dotany_base::platform::sibling_tool(m.exe)
}

/// The Mac's New: a tool installed as an `.app` is handed its own address (`dotany-sheets://new?format=xlsx`, as its
/// Info.plist names it after its program) by the system, through `open -a`, so the copy already open takes it (or the one
/// beside DotAny starts with it) and starts the new document in a window. A build that is no `.app`
/// is started with `--new`, as on Windows.
#[cfg(not(windows))]
fn new_on_mac(m: &Member, exe: &Path, ext: &str) -> Option<bool> {
    if !exe.extension().is_some_and(|e| e == "app") {
        return None;
    }
    let address = format!("{}://new?format={}", m.exe, ext.trim_start_matches('.'));
    Some(std::process::Command::new("/usr/bin/open").arg("-a").arg(exe).arg(address).spawn().is_ok())
}

// ---------- the windows ----------

#[derive(Default)]
struct Windows {
    /// the converter window that may still take more files from the same right-click, and since when its page has been
    /// ready (none while it is still loading: it takes every file that comes until then)
    fresh: Mutex<Option<(String, Option<Instant>)>>,
    /// files that came for a window, kept until its page takes them: one may come before the page is there to hear it
    waiting: Mutex<Vec<(String, Vec<String>)>>,
    count: AtomicUsize,
}

/// What a start asks for: `--convert` and the files after it, `--autostart`, or just DotAny.
fn act(app: &AppHandle, args: &[String]) {
    if let Some(at) = args.iter().position(|a| a == "--convert") {
        let files: Vec<String> = args[at + 1..]
            .iter()
            .filter(|a| !a.starts_with("--"))
            .map(|a| std::path::absolute(a).map(|p| p.to_string_lossy().into_owned()).unwrap_or_else(|_| a.clone()))
            .collect();
        if !files.is_empty() {
            open_converter(app, files);
            return;
        }
    }
    let quiet = args.iter().any(|a| a == "--autostart");
    // the Mac: started at login, DotAny keeps out of the Dock until it shows a window
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(if quiet {
        tauri::ActivationPolicy::Accessory
    } else {
        tauri::ActivationPolicy::Regular
    });
    if let Some(w) = app.get_webview_window("main") {
        if !quiet {
            let _ = w.show();
            let _ = w.unminimize();
            let _ = w.set_focus();
            dotany_base::shown(&w);
        }
        return;
    }
    let _ = dotany_base::open_window_as(
        app,
        WindowSpec {
            label: "main",
            page: "index.html",
            size: TOOL.size,
            min_size: TOOL.min_size,
            visible: !quiet,
            extra: json!({ "view": "main", "hidden": quiet }),
        },
    );
}

/// The right-click menu starts one DotAny per chosen file: those that come within a moment of the first join its window.
fn open_converter(app: &AppHandle, files: Vec<String>) {
    // the Mac's Quick Action reaching a DotAny started at login: it takes its place in the Dock with the window
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
    let wins = app.state::<Windows>();
    let mut fresh = wins.fresh.lock().unwrap();
    if let Some((label, ready)) = fresh.as_ref() {
        if ready.map_or(true, |at| at.elapsed().as_millis() < 2500) {
            if let Some(w) = app.get_webview_window(label) {
                wins.waiting.lock().unwrap().push((label.clone(), files));
                let _ = w.emit_to(label.as_str(), "convert-add", ());
                return;
            }
        }
    }
    let label = format!("convert-{}", wins.count.fetch_add(1, Ordering::Relaxed) + 1);
    if let Ok(w) = dotany_base::open_window_as(
        app,
        WindowSpec {
            label: &label,
            page: "index.html",
            size: (560.0, 384.0),
            min_size: (440.0, 340.0),
            visible: true,
            extra: json!({ "view": "convert", "files": files }),
        },
    ) {
        let _ = w.set_focus();
        *fresh = Some((label, None));
    }
}

/// The converter window's page is ready: the files that came for it meanwhile, and from now a moment more for others.
#[tauri::command]
fn convert_waiting(window: WebviewWindow, wins: State<Windows>, ready: bool) -> Vec<String> {
    let label = window.label().to_string();
    if ready {
        if let Some((l, at)) = wins.fresh.lock().unwrap().as_mut() {
            if *l == label && at.is_none() {
                *at = Some(Instant::now());
            }
        }
    }
    let mut waiting = wins.waiting.lock().unwrap();
    let mine = waiting.iter().filter(|(l, _)| *l == label).flat_map(|(_, f)| f.clone()).collect();
    waiting.retain(|(l, _)| *l != label);
    mine
}

// ---------- the family: who is installed, and the shared recent list ----------

#[derive(Serialize)]
struct Installed {
    key: &'static str,
    name: &'static str,
    exts: &'static [&'static str],
    rare: &'static [&'static str],
    installed: bool,
    version: Option<String>,
}

#[tauri::command(async)]
fn hub_tools() -> Vec<Installed> {
    FAMILY
        .iter()
        .map(|m| {
            let exe = exe_of(m);
            Installed {
                key: m.key,
                name: m.name,
                exts: m.exts,
                rare: m.rare,
                installed: exe.is_some(),
                version: exe.as_deref().and_then(system::version_of),
            }
        })
        .collect()
}

#[derive(Serialize)]
struct RecentFile {
    path: String,
    name: String,
    /// the tool that opened it last, by DotAny's key; "" when that tool isn't one DotAny knows
    tool: String,
    at: u64,
    size: u64,
    gone: bool,
    /// the converter takes it: the row offers Convert
    convertible: bool,
}

#[tauri::command(async)]
fn hub_recents(state: State<Base>) -> Vec<RecentFile> {
    family::recents(&state.family_dir)
        .files
        .into_iter()
        .map(|r| {
            let meta = std::fs::metadata(&r.path).ok().filter(|m| m.is_file());
            let tool = FAMILY
                .iter()
                .find(|m| m.folder == r.tool)
                .or_else(|| member_for_ext(&ext_of(&r.path)))
                .map(|m| m.key)
                .unwrap_or("");
            RecentFile {
                name: file_name(Path::new(&r.path)),
                tool: tool.to_string(),
                at: r.at,
                size: meta.as_ref().map_or(0, |m| m.len()),
                gone: meta.is_none(),
                convertible: can_convert(&r.path),
                path: r.path,
            }
        })
        .collect()
}

#[tauri::command(async)]
fn hub_forget(state: State<Base>, path: String) {
    family::forget_recent(&state.family_dir, &path);
}

#[tauri::command(async)]
fn hub_clear(state: State<Base>) {
    family::clear_recents(&state.family_dir);
}

/// Starts `tool` (by key; by the file's kind when empty) on `path`. The tool puts it on the recent list itself.
#[tauri::command(async)]
fn hub_open(path: String, tool: String) -> &'static str {
    if !Path::new(&path).is_file() {
        return "missing";
    }
    let Some(m) = member(&tool).or_else(|| member_for_ext(&ext_of(&path))) else { return "refused" };
    let Some(exe) = exe_of(m) else { return "uninstalled" };
    #[cfg(windows)]
    let started = std::process::Command::new(exe).arg(&path).spawn().is_ok();
    // the Mac: an .app is opened the system's way (`open -a`), so a tool already open takes the file
    #[cfg(not(windows))]
    let started = dotany_base::platform::launch(&exe, Path::new(&path));
    if started { "ok" } else { "failed" }
}

/// Starts `tool` on a new file: `--new=xlsx`. A tool that makes new files starts one; any other opens empty.
#[tauri::command(async)]
fn hub_new(tool: String, ext: String) -> &'static str {
    let Some(m) = member(&tool) else { return "refused" };
    let Some(exe) = exe_of(m) else { return "uninstalled" };
    #[cfg(not(windows))]
    if let Some(started) = new_on_mac(m, &exe, &ext) {
        return if started { "ok" } else { "failed" };
    }
    let arg = if ext.is_empty() { "--new".to_string() } else { format!("--new={}", ext.trim_start_matches('.')) };
    if std::process::Command::new(exe).arg(arg).spawn().is_ok() { "ok" } else { "failed" }
}

/// The system's open dialog, for any kind of file the family opens.
#[tauri::command(async)]
fn hub_pick(window: WebviewWindow, label: String, many: bool) -> Vec<String> {
    let exts = ALL_EXTS.get_or_init(|| {
        let mut v: Vec<&str> = FAMILY
            .iter()
            .flat_map(|m| m.exts.iter().chain(m.rare.iter()).copied())
            .chain(convertible().iter().copied())
            .collect();
        v.sort();
        v.dedup();
        v
    });
    // the Mac: the family's open panel, as the hub window's sheet (and a test's answer in its place)
    #[cfg(target_os = "macos")]
    let picked = {
        use dotany_base::platform::{Pick, ask_open, user_dir};
        let _ = label;
        ask_open(&window, exts.as_slice(), Some(user_dir("Documents")), if many { Pick::Files } else { Pick::File })
    };
    #[cfg(not(target_os = "macos"))]
    let picked = {
        let mut dialog = rfd::FileDialog::new().set_parent(&window).add_filter(label, exts);
        if let Some(dir) = dotany_base::platform::user_dir("Documents").to_str() {
            dialog = dialog.set_directory(dir);
        }
        if many { dialog.pick_files().unwrap_or_default() } else { dialog.pick_file().into_iter().collect() }
    };
    picked.into_iter().map(|p| p.to_string_lossy().into_owned()).collect()
}

#[derive(Serialize)]
struct FileInfo {
    path: String,
    name: String,
    size: u64,
}

#[tauri::command(async)]
fn hub_files(paths: Vec<String>) -> Vec<FileInfo> {
    paths
        .into_iter()
        .filter(|p| Path::new(p).is_file())
        .map(|p| FileInfo {
            name: file_name(Path::new(&p)),
            size: std::fs::metadata(&p).map_or(0, |m| m.len()),
            path: p,
        })
        .collect()
}

// ---------- the converter ----------

fn paths(list: &[String]) -> Vec<PathBuf> {
    list.iter().map(PathBuf::from).collect()
}

#[tauri::command(async)]
fn convert_targets(files: Vec<String>) -> serde_json::Value {
    match convert::targets(&paths(&files)) {
        Ok(t) => json!({ "family": t.family, "formats": t.formats }),
        Err(r) => json!({ "refused": r }),
    }
}

/// Which of these files the converter takes (the recent list's Convert).
#[tauri::command(async)]
fn convert_takes(files: Vec<String>) -> Vec<bool> {
    files.iter().map(|f| can_convert(f)).collect()
}

#[tauri::command(async)]
fn convert_estimate(files: Vec<String>, target: String, quality: u8, tone: String, pack: String) -> u64 {
    convert::estimate(&paths(&files), &target, &convert::options(quality, &tone, &pack, "", ""))
}

/// Writes the new files beside the originals, never over anything, telling the window how far it is. The new files go
/// on the shared recent list under the tool that opens them, those finished before a stop included.
#[tauri::command(async)]
fn convert_run(
    window: WebviewWindow,
    state: State<Base>,
    files: Vec<String>,
    target: String,
    quality: u8,
    tone: String,
    pack: String,
    picture: Option<String>,
    lang: Option<String>,
) -> serde_json::Value {
    let label = window.label().to_string();
    let result = convert::convert(
        &paths(&files),
        &target,
        &convert::options(quality, &tone, &pack, picture.as_deref().unwrap_or(""), lang.as_deref().unwrap_or("")),
        |p| {
            let _ = window.emit_to(label.as_str(), "convert-progress", p);
        },
    );
    let remember = |out: &[PathBuf]| -> Vec<String> {
        let out: Vec<String> = out.iter().map(|p| p.to_string_lossy().into_owned()).collect();
        for p in out.iter().rev() {
            let folder = member_for_ext(&ext_of(p)).map_or("", |m| m.folder);
            family::add_recent(&state.family_dir, folder, p);
        }
        out
    };
    match result {
        Ok(out) => json!({ "status": "done", "files": remember(&out) }),
        // the reason is the crate's own ({ kind, detail }); the page says it in the family's words
        Err(e) => {
            json!({ "status": "failed", "reason": e.reason, "file": file_name(&e.file), "files": remember(&e.done) })
        }
    }
}

// ---------- settings the system holds: starting with it, the file manager's menu, default apps ----------

#[tauri::command(async)]
fn autostart_get() -> bool {
    system::autostart()
}

#[tauri::command(async)]
fn autostart_set(on: bool) -> bool {
    system::set_autostart(on);
    system::autostart()
}

#[tauri::command(async)]
fn shell_menu_get() -> bool {
    system::shell_menu(convertible())
}

#[tauri::command(async)]
fn shell_menu_set(on: bool, label: String) -> bool {
    system::set_shell_menu(convertible(), on, &label);
    system::shell_menu(convertible())
}

/// Which kinds of file a DotAny program opens now, as Windows has it.
#[tauri::command(async)]
fn default_apps() -> serde_json::Map<String, serde_json::Value> {
    FAMILY.iter().flat_map(|m| m.exts.iter()).map(|e| (e.to_string(), system::opens_with_dotany(e).into())).collect()
}

/// The Mac lets DotAny choose: each of `exts` is opened by its tool from now on (`on`), or by the program that opened it
/// before. Answers what the system says now for every kind, as `default_apps` does. Windows changes nothing here (its
/// page sends the person to Windows' own settings) and only answers.
#[tauri::command(async)]
fn default_set(state: State<Base>, exts: Vec<String>, on: bool) -> serde_json::Map<String, serde_json::Value> {
    #[cfg(target_os = "macos")]
    for ext in &exts {
        let Some(m) = FAMILY.iter().find(|m| m.exts.contains(&ext.as_str())) else { continue };
        system::set_default(exe_of(m).as_deref(), ext, on, &state.data_dir);
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (state, exts, on);
    default_apps()
}

/// The name a tool goes by in Windows' Default apps: `DotAny <name>`, as the installer registers it in
/// RegisteredApplications (installer/src-tauri/src/reg.rs, with its component's name in installer/payload.json).
/// The three must agree: for a name that is not registered, Windows shows its whole list instead of the tool's page.
fn registered(m: &Member) -> String {
    format!("DotAny {}", m.name)
}

/// Windows' Default apps: one tool's own page (by its key), or the whole list. A tool that isn't installed has nothing
/// registered, so it never gets a page that would only be the whole list under its name.
#[tauri::command(async)]
fn open_default_apps(tool: Option<String>) -> String {
    let app = tool.as_deref().and_then(member).filter(|m| !m.exts.is_empty() && exe_of(m).is_some()).map(registered);
    system::open_default_apps(app.as_deref())
}

// ---------- feedback (feedback.rs does the work) ----------

/// Send: the text goes to the receiver; the page gets "sent", or why it was not ("offline", "many", "later", "long",
/// "refused") and keeps the text, which also waits in DotAny's folder until it is sent.
#[tauri::command(async)]
fn send_feedback(app: AppHandle, state: State<Base>, text: String, lang: String) -> &'static str {
    let version = app.package_info().version.to_string();
    let agent = format!("DotAny/{version}");
    feedback::send_and_keep(&feedback::Place::from_env(), &agent, &state.data_dir, &text, &version, &lang).word()
}

/// The text a press of Send could not send, to put back in the box.
#[tauri::command(async)]
fn feedback_draft(state: State<Base>) -> Option<String> {
    feedback::draft(&state.data_dir)
}

// ---------- updates: Check for Updates and Install (update.rs does the work) ----------
//
// Once a check finds a newer version, it is downloaded and checked in the background and waits as `ready`
// (Update ready to install) until the person presses Update; nothing is ever installed without that press. With
// Auto-download updates turned off a check only checks, and Update downloads and installs in one go.

#[derive(Default)]
struct Updates {
    status: Mutex<Status>,
    /// a check or a download is under way: a second one waits for nothing and gets the state as it is
    busy: AtomicBool,
    /// what was downloaded and checked by itself, waiting for Update (the state is then `ready`): Windows' installer,
    /// the Mac's package
    ready: Mutex<Option<PathBuf>>,
}

/// Auto-download updates: on unless turned off.
const AUTO_DOWNLOAD: &str = "dotany-hub-auto-download";

fn auto_download(app: &AppHandle) -> bool {
    app.state::<Base>().settings.lock().unwrap().values.get(AUTO_DOWNLOAD).map_or(true, |v| v != "0")
}

#[tauri::command(async)]
fn auto_download_get(app: AppHandle) -> bool {
    auto_download(&app)
}

#[tauri::command(async)]
fn auto_download_set(app: AppHandle, on: bool) -> bool {
    {
        let state = app.state::<Base>();
        let mut settings = state.settings.lock().unwrap();
        settings.values.insert(AUTO_DOWNLOAD.into(), if on { "1" } else { "0" }.into());
        dotany_base::settings::store(&state.data_dir, &settings);
    }
    fetch_quietly(&app);
    auto_download(&app)
}

/// A check has found a newer version and Auto-download updates is on: the download starts by itself, in the background.
fn fetch_quietly(app: &AppHandle) {
    let s = app.state::<Updates>().status.lock().unwrap().clone();
    if !update::fetch_by_itself(&s, auto_download(app)) {
        return;
    }
    let app = app.clone();
    std::thread::spawn(move || {
        fetch(&app, None, false);
    });
}

const DAY_MS: u64 = 86_400_000;
const CHECKED: &str = "dotany-hub-update-checked";

/// What is installed, as the release names it: DotAny itself as "hub", each tool by its key.
fn installed(app: &AppHandle) -> Vec<(String, String)> {
    let mut list = vec![("hub".to_string(), app.package_info().version.to_string())];
    list.extend(
        FAMILY.iter().filter_map(|m| exe_of(m).and_then(|e| system::version_of(&e)).map(|v| (m.key.to_string(), v))),
    );
    list
}

fn kept_manifest(app: &AppHandle) -> PathBuf {
    app.state::<Base>().data_dir.join("update-manifest.json")
}

fn set_status(app: &AppHandle, s: Status) {
    *app.state::<Updates>().status.lock().unwrap() = s.clone();
    let _ = app.emit("update", s);
}

/// The last check's answer, kept with DotAny's settings, read again against what is installed now: within a day of it,
/// a new start knows what that check found without asking again.
fn recall(app: &AppHandle) {
    let state = app.state::<Base>();
    let checked: u64 = state.settings.lock().unwrap().values.get(CHECKED).and_then(|v| v.parse().ok()).unwrap_or(0);
    let Some(m) = std::fs::read(kept_manifest(app)).ok().and_then(|b| update::read_manifest(&b).ok()) else { return };
    if checked > 0 {
        *app.state::<Updates>().status.lock().unwrap() = update::status_of(&m, &installed(app), checked);
    }
}

/// One check, if none is under way. A quiet one (the daily one) leaves the last good answer standing when it fails.
fn run_check(app: &AppHandle, quiet: bool) -> Status {
    let ups = app.state::<Updates>();
    if ups.busy.swap(true, Ordering::SeqCst) {
        return ups.status.lock().unwrap().clone();
    }
    let was = ups.status.lock().unwrap().clone();
    if !quiet {
        set_status(app, Status { state: "checking".into(), ..was.clone() });
    }
    let now = family::now_ms();
    let result = update::check(&update::Source::from_env(), &installed(app), now);
    let next = match result {
        Ok(found) => {
            let state = app.state::<Base>();
            let mut settings = state.settings.lock().unwrap();
            settings.values.insert(CHECKED.into(), now.to_string());
            dotany_base::settings::store(&state.data_dir, &settings);
            let _ = dotany_base::atomic::atomic_write(&kept_manifest(app), &found.manifest);
            found.status
        }
        Err(_) if quiet => was.clone(),
        Err(p) => Status { state: p.state().into(), ..was.clone() },
    };
    // a version already downloaded and waiting stays waiting while it is still the newest
    let next = update::after_check(&was, next);
    // which tools were open is the answer to one Update; a check after it doesn't repeat it
    #[cfg(target_os = "macos")]
    let next = Status { open: Vec::new(), ..next };
    if next.state != "ready" {
        *ups.ready.lock().unwrap() = None;
    }
    set_status(app, next.clone());
    ups.busy.store(false, Ordering::SeqCst);
    fetch_quietly(app);
    next
}

/// At most once a day while DotAny runs, starting a little after it does; never in a test unless a test release is set.
fn daily_check(app: &AppHandle) {
    if std::env::var_os("DOTANY_TEST").is_some() && std::env::var_os("DOTANY_UPDATE_URL").is_none() {
        return;
    }
    let first = std::env::var("DOTANY_UPDATE_FIRST_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(20_000);
    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(first));
        loop {
            let checked: u64 = app
                .state::<Base>()
                .settings
                .lock()
                .unwrap()
                .values
                .get(CHECKED)
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);
            if family::now_ms().saturating_sub(checked) >= DAY_MS {
                run_check(&app, true);
            } else {
                // a newer version an earlier check found (kept from before this start) is fetched too
                fetch_quietly(&app);
            }
            std::thread::sleep(std::time::Duration::from_secs(3600));
        }
    });
}

#[tauri::command]
fn update_status(ups: State<Updates>) -> Status {
    ups.status.lock().unwrap().clone()
}

#[tauri::command(async)]
fn update_check(app: AppHandle) -> Status {
    run_check(&app, false)
}

/// Install (`tool`: one not installed yet) or Update (none) pressed: download, check, and install. A version already
/// downloaded and waiting goes in straight away.
#[tauri::command(async)]
fn update_get(app: AppHandle, tool: Option<String>) -> Status {
    let waiting = app.state::<Updates>().ready.lock().unwrap().is_some();
    if tool.is_none() && waiting {
        return install(&app);
    }
    fetch(&app, tool, true)
}

/// Update once the new version is ready.
#[tauri::command(async)]
fn update_install(app: AppHandle) -> Status {
    install(&app)
}

/// Downloads and checks the release; then installs it when the person asked (`then_install`), or keeps it `ready`.
fn fetch(app: &AppHandle, tool: Option<String>, then_install: bool) -> Status {
    let ups = app.state::<Updates>();
    if ups.busy.swap(true, Ordering::SeqCst) {
        return ups.status.lock().unwrap().clone();
    }
    let was = ups.status.lock().unwrap().clone();
    let add: Vec<String> = tool.into_iter().filter(|t| member(t).is_some()).collect();
    let mut parts: Vec<String> =
        if add.is_empty() { was.newer.iter().map(|n| n.id.clone()).collect() } else { add.clone() };
    // the installer brings every installed part up to the release's version, so those that are behind move too
    for n in &was.newer {
        if !parts.contains(&n.id) {
            parts.push(n.id.clone());
        }
    }
    let base = Status { pct: 0.0, parts: parts.clone(), ..was.clone() };
    set_status(app, Status { state: "downloading".into(), ..base.clone() });
    let src = update::Source::from_env();
    let have = installed(app);
    let a = app.clone();
    let progress_base = base.clone();
    let result = update::download(&src, &have, !add.is_empty(), &update::folder(), &mut |pct| {
        set_status(&a, Status { state: "downloading".into(), pct, ..progress_base.clone() });
    });
    let next = match result {
        Ok(got) if !then_install => {
            *ups.ready.lock().unwrap() = Some(got);
            Status { state: "ready".into(), pct: 1.0, ..base }
        }
        Ok(got) => {
            ups.busy.store(false, Ordering::SeqCst);
            *ups.ready.lock().unwrap() = Some(got);
            return install_with(app, &add);
        }
        Err(p) => Status { state: p.state().into(), pct: 0.0, ..base },
    };
    set_status(app, next.clone());
    ups.busy.store(false, Ordering::SeqCst);
    next
}

fn install(app: &AppHandle) -> Status {
    install_with(app, &[])
}

/// What is downloaded and checked goes in. Windows: the installer starts in its update mode (it asks the person to close
/// the open DotAny programs, closes DotAny and opens it again). The Mac: unless a tool runs from one of the .apps to be
/// replaced (then it waits, still `ready`, naming them: `open`), the .apps are replaced (`update::mac::install`) and
/// DotAny opens again.
fn install_with(app: &AppHandle, add: &[String]) -> Status {
    let ups = app.state::<Updates>();
    let Some(got) = ups.ready.lock().unwrap().clone() else { return ups.status.lock().unwrap().clone() };
    if ups.busy.swap(true, Ordering::SeqCst) {
        return ups.status.lock().unwrap().clone();
    }
    let was = ups.status.lock().unwrap().clone();
    #[cfg(not(target_os = "macos"))]
    let next = {
        if update::launch(&got, &tools_dir(), add) {
            *ups.ready.lock().unwrap() = None;
            Status { state: "installing".into(), pct: 1.0, ..was }
        } else {
            // not started (it changed after its check, and is gone): Update downloads it again rather than retrying it
            *ups.ready.lock().unwrap() = None;
            Status { state: Problem::Failed.state().into(), pct: 0.0, ..was }
        }
    };
    #[cfg(target_os = "macos")]
    let next = {
        let _ = add;
        let me = std::env::current_exe().ok();
        match me.as_deref().and_then(|p| p.ancestors().find(|a| a.extension().is_some_and(|e| e == "app"))) {
            // a DotAny not run from its .app (a build folder) has nothing to replace
            None => Status { state: "failed".into(), ..was },
            Some(hub) => {
                let was = Status { open: Vec::new(), ..was };
                set_status(app, Status { state: "installing".into(), ..was.clone() });
                let root = hub.parent().unwrap_or(Path::new("/"));
                let name = hub.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                let done = update::mac::install(&got, root, &name, &system::open_from);
                // in or not, what was downloaded goes (a failed one is fetched again next time); kept while tools that
                // it would replace are open, for Update once they are closed
                if !matches!(done, Err(Problem::Open(_))) {
                    let _ = std::fs::remove_file(&got);
                    *ups.ready.lock().unwrap() = None;
                }
                match done {
                    Ok(_) => {
                        if update::mac::reopen(&root.join(&name)) {
                            // a moment for the page to say Installing…, then this DotAny ends and the new one opens
                            let a = app.clone();
                            std::thread::spawn(move || {
                                std::thread::sleep(std::time::Duration::from_millis(600));
                                a.exit(0);
                            });
                        }
                        Status { state: "installing".into(), pct: 1.0, ..was }
                    }
                    // nothing replaced, nothing quit: the person closes them, then Update again
                    Err(Problem::Open(names)) => Status { state: "ready".into(), open: names, ..was },
                    Err(p) => Status { state: p.state().into(), ..was },
                }
            }
        }
    };
    set_status(app, next.clone());
    ups.busy.store(false, Ordering::SeqCst);
    next
}

// ---------- the start ----------

// Windows has no first-start step: starting with Windows is the installer's to set first (on, unless the person turned
// it off at its last step or the quiet install was told `--no-autostart`) and the person's from then on, in Settings.
// Turning it on here would undo an install that left it off, and a hub started from a build folder has no install
// behind it to decide anything.

/// The Mac has no installer, so there the first start is the first decision: whether it turns the login item (Open
/// DotAny at login) on. The system then tells the person once that DotAny was added to the login items.
#[cfg(any(target_os = "macos", test))]
fn login_item_wanted(started_before: bool, runs_from_app: bool) -> bool {
    // only a DotAny that runs from its .app can be a login item, so a start from a build folder is left unmarked and
    // the first start from the .app decides; once marked, it never decides again, whatever the person has done since
    !started_before && runs_from_app
}

/// The Mac: the first start from the .app turns the login item on and marks it did. With `DOTANY_TEST` set, the Mac's
/// login items are left alone unless `DOTANY_TEST_FIRST_START` is set as well; whoever sets it takes the item away
/// again itself.
#[cfg(target_os = "macos")]
fn first_start(app: &AppHandle) {
    if std::env::var_os("DOTANY_TEST").is_some() && std::env::var_os("DOTANY_TEST_FIRST_START").is_none() {
        return;
    }
    let state = app.state::<Base>();
    let mut settings = state.settings.lock().unwrap();
    let started = settings.values.contains_key("dotany-hub-started");
    if !login_item_wanted(started, dotany_base::quickaction::this_app().is_some()) {
        return;
    }
    system::set_autostart(true);
    settings.values.insert("dotany-hub-started".into(), "1".into());
    dotany_base::settings::store(&state.data_dir, &settings);
}

pub fn run() {
    let args: Vec<String> = std::env::args_os().skip(1).map(|a| a.to_string_lossy().into_owned()).collect();
    let data_dir = dotany_base::settings::data_dir(&TOOL);
    let inbox = data_dir.join("inbox");
    // another DotAny is running: it has been told what this start was for
    let Ok(single) = system::single(&data_dir, &inbox, &args) else { return };
    let single = std::sync::Arc::new(single);
    // the family calls this again on the Mac when the Dock icon is clicked with no window left (`reopen`): then only the
    // window comes back; the start's own work (the first start, the last check, the daily check, the knocks) is done once
    static STARTED: AtomicBool = AtomicBool::new(false);
    dotany_base::builder_with(&TOOL, move |app, _| {
        let first = !STARTED.swap(true, Ordering::SeqCst);
        if first {
            #[cfg(target_os = "macos")]
            first_start(app);
            recall(app);
            daily_check(app);
        }
        // the Mac opened DotAny as a login item (asked while it starts, as the event that started it is still the
        // current one): it gets ready without showing itself, as `--autostart` does on Windows
        #[cfg(target_os = "macos")]
        let args: Vec<String> =
            args.iter().cloned().chain(system::started_at_login().then(|| "--autostart".to_string())).collect();
        act(app, &args);
        if !first {
            return Ok(());
        }
        let (app, single, inbox) = (app.clone(), single.clone(), inbox.clone());
        std::thread::spawn(move || {
            while system::wait_for_knock(&single) {
                let mut knocks: Vec<PathBuf> = std::fs::read_dir(&inbox)
                    .map(|d| d.flatten().map(|e| e.path()).filter(|p| !file_name(p).starts_with('~')).collect())
                    .unwrap_or_default();
                knocks.sort();
                for k in knocks {
                    let asked: Vec<String> =
                        std::fs::read(&k).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
                    let _ = std::fs::remove_file(&k);
                    act(&app, &asked);
                }
            }
        });
        Ok(())
    })
    .manage(Windows::default())
    .manage(Updates::default())
    .invoke_handler(dotany_base::handler![
        hub_tools,
        hub_recents,
        hub_forget,
        hub_clear,
        hub_open,
        hub_new,
        hub_pick,
        hub_files,
        convert_targets,
        convert_estimate,
        convert_run,
        autostart_get,
        autostart_set,
        shell_menu_get,
        shell_menu_set,
        default_apps,
        default_set,
        open_default_apps,
        send_feedback,
        feedback_draft,
        convert_waiting,
        convert_takes,
        update_status,
        update_check,
        update_get,
        update_install,
        auto_download_get,
        auto_download_set
    ])
    .run_family_with(tauri::generate_context!(), |app, paths| {
        // files handed to DotAny itself on the Mac (dropped on its Dock icon, or the converter's Quick Action) go to the
        // converter, as `--convert` and the files after it do on Windows
        let mut args = vec!["--convert".to_string()];
        args.extend(paths.iter().map(|p| p.to_string_lossy().into_owned()));
        act(app, &args);
    })
    .expect("error while running DotAny");
}

#[cfg(test)]
mod tests {
    use super::login_item_wanted;

    /// The Mac's first start decides once, and only from the .app: never again after it has, and never from a build
    /// folder, whatever the person did to the login item in between.
    #[test]
    fn the_login_item_is_decided_once_from_the_app() {
        assert!(login_item_wanted(false, true));
        assert!(!login_item_wanted(true, true), "the person's later choice stands");
        assert!(!login_item_wanted(false, false), "a build folder can't be a login item, and stays undecided");
        assert!(!login_item_wanted(true, false));
    }
}
