//! The Tauri side of the starter: what makes this tool itself, and its two commands. Everything else is dotany-base.
//! A new tool replaces the extensions, `Doc` and the two commands with its own reading and writing. It keeps a file per
//! window, as every tool does (shared/README.md, "A file per window"): each command works on the window that asked.

use dotany_base::RunFamily;
use dotany_base::atomic::{WriteFailure, atomic_write};
use dotany_base::{FileWindows, PerWindow, Tool, file_name, lock};
use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Manager, State, WebviewWindow};

fn any_bytes(_: &Path) -> bool {
    true
}

/// The window's file closed, or the window went: what it held goes.
fn forget(app: &AppHandle, label: &str) {
    app.state::<Docs>().remove(label);
}

/// The starter keeps a file per window and says so in `FileWindows` (`forget`); this is never called.
fn nothing(_: &AppHandle) {}

pub static TOOL: Tool = Tool {
    name: "DotAny Starter",
    folder: "Starter",
    data_env: "DOTANY_STARTER_DATA",
    user_dir: "Documents",
    opens_files: true,
    extensions: &["txt", "md"],
    sniff: any_bytes,
    size: (1180.0, 800.0),
    min_size: (480.0, 420.0),
    on_close: nothing,
};

/// The open file as this tool holds it, in each window. The starter only keeps its bytes, so a save is a faithful copy.
type Docs = PerWindow<Mutex<Option<Vec<u8>>>>;

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
enum Opened {
    Ok { path: String, dir: String, name: String, size: u64 },
    Refused,
    Missing,
    Damaged,
}

#[tauri::command(async)]
fn open_file(window: WebviewWindow, docs: State<Docs>, path: String) -> Opened {
    let path = std::path::absolute(&path).unwrap_or_else(|_| PathBuf::from(&path));
    if !path.is_file() {
        return Opened::Missing;
    }
    if !TOOL.allowed_name(&file_name(&path)) {
        return Opened::Refused;
    }
    let Ok(bytes) = fs::read(&path) else { return Opened::Damaged };
    let size = bytes.len() as u64;
    *lock(&docs.of(window.label())) = Some(bytes);
    dotany_base::opened_in(window.app_handle(), window.label(), &path);
    Opened::Ok {
        dir: path.parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default(),
        name: file_name(&path),
        path: path.to_string_lossy().into_owned(),
        size,
    }
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
enum Saved {
    Saved { path: String, name: String, size: u64 },
    Nothing,
    Failed { kind: WriteFailure, detail: String },
}

/// The new file's size, exactly.
#[tauri::command(async)]
fn estimate_save(window: WebviewWindow, docs: State<Docs>) -> Option<u64> {
    lock(&docs.of(window.label())).as_ref().map(|b| b.len() as u64)
}

#[tauri::command(async)]
fn save_file(window: WebviewWindow, docs: State<Docs>, path: String) -> Saved {
    let Some(bytes) = lock(&docs.of(window.label())).clone() else { return Saved::Nothing };
    let target = PathBuf::from(&path);
    if let Err(e) = atomic_write(&target, &bytes) {
        return Saved::Failed { kind: e.kind, detail: e.detail };
    }
    dotany_base::saved_in(window.app_handle(), window.label(), &target);
    Saved::Saved { name: file_name(&target), path, size: bytes.len() as u64 }
}

pub fn run() {
    dotany_base::builder_per_window(&TOOL, FileWindows { forget, extra: dotany_base::no_extra })
        .manage(Docs::default())
        .invoke_handler(dotany_base::handler![open_file, estimate_save, save_file])
        .run_family(tauri::generate_context!())
        .expect("error while running DotAny Starter");
}
