//! The Tauri side of DotAny PDF: what makes this tool itself, the open file's bytes, the PDFs and pictures added in the
//! pages view, and writing what the window holds (save.rs), a Word document (docx.rs) or a picture of a page. Reading
//! and drawing the pages happens in the window, with PDF.js; everything else is dotany-base. PDF keeps a file per window
//! (`dotany_base::builder_per_window`): on the Mac one program shows each PDF in a window of its own; on Windows each PDF
//! is a program of its own, with one window.

use crate::save::{self, Kind, Plan, Source};
use crate::{docx, embed};
use dotany_base::RunFamily;
use dotany_base::atomic::{WriteFailure, atomic_write};
use dotany_base::{Base, FileWindows, PerWindow, Tool, file_name, lock};
use serde::Serialize;
use std::collections::HashMap;
use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::ipc::{InvokeBody, Request, Response};
use tauri::{AppHandle, Manager, State, WebviewWindow};

/// A PDF says so in its first kilobyte.
fn looks_like_a_pdf(path: &Path) -> bool {
    let mut head = vec![0u8; 1024];
    let Ok(mut f) = fs::File::open(path) else { return false };
    let n = f.read(&mut head).unwrap_or(0);
    head[..n].windows(5).any(|w| w == b"%PDF-")
}

/// The window's PDF closed, or the window went: its file, what was added to it, the pictures drawn for a save and the
/// last file made all go with it.
fn forget(app: &AppHandle, label: &str) {
    app.state::<Held>().windows.remove(label);
}

/// PDF keeps a file per window and says so in `FileWindows` (`forget`); this is never called.
fn nothing(_: &AppHandle) {}

pub static TOOL: Tool = Tool {
    name: "DotAny PDF",
    folder: "PDF",
    data_env: "DOTANY_PDF_DATA",
    user_dir: "Documents",
    opens_files: true,
    extensions: &["pdf"],
    sniff: looks_like_a_pdf,
    size: (1180.0, 800.0),
    min_size: (480.0, 420.0),
    on_close: nothing,
};

/// What one window holds besides its page: its open file (source 0, with its password) and what was added to it, the
/// pictures its page drew for a save, and the last file made, so a save after an estimate is the very bytes the estimate
/// measured. Another window of the program (the Mac) has its own; nothing here is seen by it.
#[derive(Default)]
struct Win {
    sources: Mutex<HashMap<u32, Source>>,
    next: Mutex<u32>,
    blobs: Mutex<HashMap<String, Vec<u8>>>,
    made: Mutex<Option<(u64, Arc<Vec<u8>>)>>,
}

/// What the program holds: each window's own (`windows`), and the font files new words are written in. Those belong to
/// no one PDF: they are the family's own fonts, the same bytes whichever window hands them over, so they are kept once
/// and a second window finds them already there (`fonts_held`).
#[derive(Default)]
struct Held {
    windows: PerWindow<Win>,
    fonts: Mutex<embed::Files>,
}

impl Held {
    fn win(&self, window: &WebviewWindow) -> Arc<Win> {
        self.windows.of(window.label())
    }
}

fn absolute(path: &str) -> PathBuf {
    std::path::absolute(path).unwrap_or_else(|_| PathBuf::from(path))
}

/// Whether the window is still there: a file read for a window that has gone meanwhile is kept by nothing (its row went
/// with it, and a new one would stay until the program quits, a long time on the Mac).
fn alive(window: &WebviewWindow) -> bool {
    window.app_handle().get_webview_window(window.label()).is_some()
}

/// The file's bytes, whole, straight to the page; kept here as the window's open file until the page has read it.
#[tauri::command(async)]
fn read_pdf(window: WebviewWindow, held: State<Held>, path: String) -> Result<Response, String> {
    let path = absolute(&path);
    if !path.is_file() {
        return Err("missing".into());
    }
    if !TOOL.allowed_name(&file_name(&path)) || !looks_like_a_pdf(&path) {
        return Err("refused".into());
    }
    let bytes = fs::read(&path).map_err(|_| "damaged".to_string())?;
    if !alive(&window) {
        return Err("missing".into());
    }
    let win = held.win(&window);
    let mut s = lock(&win.sources);
    s.clear();
    s.insert(0, Source { bytes: Arc::new(bytes.clone()), kind: Kind::Pdf, password: None });
    *lock(&win.next) = 1;
    lock(&win.blobs).clear();
    *lock(&win.made) = None;
    Ok(Response::new(bytes))
}

#[derive(Serialize)]
struct Opened {
    status: &'static str,
    path: String,
    dir: String,
    name: String,
    size: u64,
    rights: save::Rights,
}

/// The page has read it, with this password if it needed one: now it's the open file, watched and first in the recent
/// list. Answers what the file allows.
#[tauri::command(async)]
fn adopt_pdf(window: WebviewWindow, held: State<Held>, path: String, password: Option<String>) -> Opened {
    let path = absolute(&path);
    let rights = {
        let win = held.win(&window);
        let mut s = lock(&win.sources);
        match s.get_mut(&0) {
            Some(src) => {
                src.password = password.filter(|p| !p.is_empty());
                save::rights(src)
            }
            None => save::Rights {
                encrypted: false,
                modify: false,
                copy: false,
                annotate: false,
                fill: false,
                assemble: false,
                writable: false,
            },
        }
    };
    dotany_base::opened_in(window.app_handle(), window.label(), &path);
    Opened {
        status: "ok",
        dir: path.parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default(),
        name: file_name(&path),
        size: fs::metadata(&path).map(|m| m.len()).unwrap_or(0),
        path: path.to_string_lossy().into_owned(),
        rights,
    }
}

#[derive(Serialize)]
struct Added {
    id: u32,
    name: String,
    kind: &'static str,
    size: u64,
}

fn add_paths(window: &WebviewWindow, held: &Held, paths: Vec<PathBuf>) -> Vec<Added> {
    let mut out = Vec::new();
    if !alive(window) {
        return out;
    }
    let held = held.win(window);
    for p in paths {
        let Ok(bytes) = fs::read(&p) else { continue };
        let name = file_name(&p);
        let kind = if bytes.windows(5).take(1024).any(|w| w == b"%PDF-") {
            Kind::Pdf
        } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) || bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            Kind::Image
        } else {
            out.push(Added { id: 0, name, kind: "refused", size: 0 });
            continue;
        };
        let mut next = lock(&held.next);
        let id = *next;
        *next += 1;
        let size = bytes.len() as u64;
        let k = if matches!(kind, Kind::Pdf) { "pdf" } else { "image" };
        lock(&held.sources).insert(id, Source { bytes: Arc::new(bytes), kind, password: None });
        out.push(Added { id, name, kind: k, size });
    }
    out
}

/// The system's open dialog, several at once: PDFs, JPG and PNG to add as pages.
#[tauri::command(async)]
fn pick_sources(window: WebviewWindow, held: State<Held>, label: String) -> Vec<Added> {
    // the Mac: the family's open panel, as this window's sheet (and a test's answer in its place)
    #[cfg(target_os = "macos")]
    {
        let _ = label;
        let folder = window.state::<Base>().current_in(window.label()).and_then(|p| p.parent().map(Path::to_path_buf));
        let paths = dotany_base::platform::ask_open(
            &window,
            &["pdf", "jpg", "jpeg", "png"],
            folder,
            dotany_base::platform::Pick::Files,
        );
        add_paths(&window, &held, paths)
    }
    #[cfg(not(target_os = "macos"))]
    pick_sources_windows(window, held, label)
}

#[cfg(not(target_os = "macos"))]
fn pick_sources_windows(window: WebviewWindow, held: State<Held>, label: String) -> Vec<Added> {
    let mut dialog = rfd::FileDialog::new().set_parent(&window).add_filter(&label, &["pdf", "jpg", "jpeg", "png"]);
    let folder = window.state::<Base>().current_in(window.label()).and_then(|p| p.parent().map(Path::to_path_buf));
    if let Some(dir) = folder {
        dialog = dialog.set_directory(dir);
    }
    let Some(paths) = dialog.pick_files() else { return vec![] };
    add_paths(&window, &held, paths)
}

/// Files dropped on the pages view, or named by the tests.
#[tauri::command(async)]
fn add_sources(window: WebviewWindow, held: State<Held>, paths: Vec<String>) -> Vec<Added> {
    add_paths(&window, &held, paths.iter().map(|p| absolute(p)).collect())
}

#[tauri::command(async)]
fn read_source(window: WebviewWindow, held: State<Held>, id: u32) -> Result<Response, String> {
    let win = held.win(&window);
    let s = lock(&win.sources);
    s.get(&id).map(|src| Response::new(src.bytes.to_vec())).ok_or_else(|| "missing".into())
}

#[tauri::command(async)]
fn source_password(window: WebviewWindow, held: State<Held>, id: u32, password: String) {
    if let Some(s) = lock(&held.win(&window).sources).get_mut(&id) {
        s.password = Some(password).filter(|p| !p.is_empty());
    }
}

/// A picture for a signature: the system's open dialog, then its bytes.
#[tauri::command(async)]
fn pick_picture(window: WebviewWindow, label: String) -> Result<Response, String> {
    // the Mac: the family's open panel, as this window's sheet
    #[cfg(target_os = "macos")]
    let Some(p) = ({
        let _ = label;
        dotany_base::platform::ask_open(&window, &["png", "jpg", "jpeg"], None, dotany_base::platform::Pick::File)
            .into_iter()
            .next()
    }) else {
        return Err("none".into());
    };
    #[cfg(not(target_os = "macos"))]
    let Some(p) = rfd::FileDialog::new().add_filter(&label, &["png", "jpg", "jpeg"]).set_parent(&window).pick_file()
    else {
        return Err("none".into());
    };
    fs::read(&p).map(Response::new).map_err(|_| "damaged".into())
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

fn header(request: &Request, k: &str) -> Option<String> {
    request.headers().get(k).and_then(|v| v.to_str().ok()).map(unescape)
}

/// A picture the page drew for a save (a redacted page, a page for Word), kept under the id in the `id` header.
#[tauri::command(async)]
fn stash(window: WebviewWindow, held: State<Held>, request: Request) -> bool {
    let (Some(id), InvokeBody::Raw(bytes)) = (header(&request, "id"), request.body()) else { return false };
    lock(&held.win(&window).blobs).insert(id, bytes.clone());
    true
}

#[tauri::command(async)]
fn drop_stash(window: WebviewWindow, held: State<Held>) {
    if let Some(win) = held.windows.get(window.label()) {
        lock(&win.blobs).clear();
    }
}

/// The font files the program already has, handed over once by the first window that saves new words.
#[tauri::command(async)]
fn put_font(held: State<Held>, request: Request) -> bool {
    let (Some(name), InvokeBody::Raw(bytes)) = (header(&request, "name"), request.body()) else { return false };
    lock(&held.fonts).insert(name, Arc::new(bytes.clone()));
    true
}

#[tauri::command(async)]
fn fonts_held(held: State<Held>) -> Vec<String> {
    lock(&held.fonts).keys().cloned().collect()
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
enum Saved {
    Saved { path: String, name: String, size: u64 },
    Failed { kind: WriteFailure, detail: String },
    Unreadable { detail: String },
}

fn key_of(plan: &str) -> u64 {
    let mut h = DefaultHasher::new();
    plan.hash(&mut h);
    h.finish()
}

/// The file the plan makes, from the last estimate if it was the same plan.
fn make(held: &Held, window: &WebviewWindow, plan_json: &str) -> Result<Arc<Vec<u8>>, save::Fail> {
    let win = held.win(window);
    let key = key_of(plan_json);
    if let Some((k, bytes)) = lock(&win.made).as_ref() {
        if *k == key {
            return Ok(bytes.clone());
        }
    }
    let plan: Plan = serde_json::from_str(plan_json).map_err(|e| save::Fail::Other(e.to_string()))?;
    let bytes = {
        let sources = lock(&win.sources);
        let blobs = lock(&win.blobs);
        let fonts = lock(&held.fonts);
        // a PDF that can't be read can still be read by other programs: it goes on panicking only in this one save
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| save::build(&plan, &sources, &blobs, &fonts)))
            .map_err(|_| save::Fail::Damaged("panic".into()))??
    };
    let bytes = Arc::new(bytes);
    *lock(&win.made) = Some((key, bytes.clone()));
    Ok(bytes)
}

/// The new file's size, exactly: the file is made once and kept for the save.
#[tauri::command(async)]
fn estimate_pdf(window: WebviewWindow, held: State<Held>, plan: String) -> Option<u64> {
    make(&held, &window, &plan).ok().map(|b| b.len() as u64)
}

/// The plan, written to `path`. `keep` is true when the window stays on the open file (pages taken out of it).
#[tauri::command(async)]
fn save_pdf(window: WebviewWindow, held: State<Held>, path: String, plan: String) -> Saved {
    let bytes = match make(&held, &window, &plan) {
        Ok(b) => b,
        Err(save::Fail::Damaged(d)) => return Saved::Unreadable { detail: d },
        Err(save::Fail::Other(d)) => return Saved::Failed { kind: WriteFailure::Other, detail: d },
    };
    let target = PathBuf::from(&path);
    if let Err(e) = atomic_write(&target, &bytes) {
        return Saved::Failed { kind: e.kind, detail: e.detail };
    }
    dotany_base::saved_in(window.app_handle(), window.label(), &target);
    Saved::Saved { name: file_name(&target), path, size: bytes.len() as u64 }
}

/// Word, from what the page read off each page.
#[tauri::command(async)]
fn save_docx(window: WebviewWindow, held: State<Held>, path: String, book: String) -> Saved {
    let book: docx::Book = match serde_json::from_str(&book) {
        Ok(b) => b,
        Err(e) => return Saved::Failed { kind: WriteFailure::Other, detail: e.to_string() },
    };
    let bytes = {
        let win = held.win(&window);
        let blobs = lock(&win.blobs);
        match docx::write(&book, &blobs) {
            Ok(b) => b,
            Err(e) => return Saved::Failed { kind: WriteFailure::Other, detail: e },
        }
    };
    let target = PathBuf::from(&path);
    if let Err(e) = atomic_write(&target, &bytes) {
        return Saved::Failed { kind: e.kind, detail: e.detail };
    }
    Saved::Saved { name: file_name(&target), path, size: bytes.len() as u64 }
}

/// A picture of a page, made by the page, written whole to the path in the `path` header. It isn't a file this tool
/// opens, so it doesn't go on the recent list.
#[tauri::command(async)]
fn save_bytes(request: Request) -> Saved {
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
    Saved::Saved { name: file_name(&target), path, size: bytes.len() as u64 }
}

/// How much memory this process holds right now, so that a test can check how a large file is handled.
#[tauri::command]
fn memory() -> u64 {
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
        use windows_sys::Win32::System::Threading::GetCurrentProcess;
        let mut c: PROCESS_MEMORY_COUNTERS = std::mem::zeroed();
        c.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
        if GetProcessMemoryInfo(GetCurrentProcess(), &mut c, c.cb) != 0 {
            return c.WorkingSetSize as u64;
        }
    }
    // the Mac: the process's footprint, what Activity Monitor shows as its memory (`phys_footprint`, the same figure
    // `task_info` gives for TASK_VM_INFO), from the kernel's usage record of this process
    #[cfg(target_os = "macos")]
    {
        #[repr(C)]
        struct Usage {
            uuid: [u8; 16],
            user_time: u64,
            system_time: u64,
            idle_wakeups: u64,
            interrupt_wakeups: u64,
            pageins: u64,
            wired: u64,
            resident: u64,
            phys_footprint: u64,
            started: u64,
            exited: u64,
        }
        unsafe extern "C" {
            fn proc_pid_rusage(pid: i32, flavor: i32, buffer: *mut Usage) -> i32;
        }
        let mut u: Usage = unsafe { std::mem::zeroed() };
        // RUSAGE_INFO_V0
        if unsafe { proc_pid_rusage(std::process::id() as i32, 0, &mut u) } == 0 {
            return u.phys_footprint;
        }
    }
    0
}

pub fn run() {
    dotany_base::builder_per_window(&TOOL, FileWindows { forget, extra: dotany_base::no_extra })
        .manage(Held::default())
        .invoke_handler(dotany_base::handler![
            read_pdf,
            adopt_pdf,
            pick_sources,
            add_sources,
            read_source,
            source_password,
            pick_picture,
            stash,
            drop_stash,
            put_font,
            fonts_held,
            estimate_pdf,
            save_pdf,
            save_docx,
            save_bytes,
            memory,
            dotany_base::commands::open_web_link
        ])
        .run_family(tauri::generate_context!())
        .expect("error while running DotAny PDF");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pdfs_are_known_by_their_first_bytes() {
        let dir = std::env::temp_dir().join(format!("dotany-pdf-sniff-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a.pdf");
        fs::write(&a, b"%PDF-1.7\n%\xe2\xe3").unwrap();
        let b = dir.join("b.pdf");
        fs::write(&b, b"junk before %PDF-1.4 header").unwrap();
        let c = dir.join("c.pdf");
        fs::write(&c, b"PK\x03\x04 not a pdf").unwrap();
        assert!(looks_like_a_pdf(&a));
        assert!(looks_like_a_pdf(&b));
        assert!(!looks_like_a_pdf(&c));
        assert_eq!(unescape("%E4%B8%AD.pdf"), "中.pdf");
        fs::remove_dir_all(&dir).ok();
    }
}
