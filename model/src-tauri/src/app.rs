//! The Tauri side of DotAny 3D: what makes this tool itself, reading a model and the files it names beside it, and
//! writing what the page made. Reading the formats themselves happens in the page, with three.js; everything else is
//! dotany-base. 3D keeps a file per window (`dotany_base::builder_per_window`): on the Mac one program shows each model
//! in a window of its own, each drawn by its own page; on Windows each model is a program of its own, with one window.

use dotany_base::RunFamily;
use dotany_base::atomic::{WriteFailure, atomic_write};
use dotany_base::{FileWindows, PerWindow, Tool, file_name, lock};
use serde::Serialize;
use std::borrow::Cow;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;
use tauri::ipc::{InvokeBody, Request, Response};
use tauri::{AppHandle, Manager, State, WebviewWindow};

fn any_bytes(_: &Path) -> bool {
    true
}

/// The window's model is closed: the files beside it are no longer served to it. A window that has gone takes its row
/// with it.
fn forget(app: &AppHandle, label: &str) {
    let docs = app.state::<Docs>();
    if app.get_webview_window(label).is_none() {
        let gone = docs.0.remove(label);
        drop(gone);
        return;
    }
    if let Some(doc) = docs.0.get(label) {
        *lock(&doc.0) = None;
    }
}

/// 3D keeps a model per window and says so in `FileWindows` (`forget`); this is never called.
fn nothing(_: &AppHandle) {}

pub static TOOL: Tool = Tool {
    name: "DotAny 3D",
    folder: "3D",
    data_env: "DOTANY_MODEL_DATA",
    user_dir: "Documents",
    opens_files: true,
    // FBX is only read, never written; so are PLY, OFF, G-code and VOX
    extensions: &["glb", "gltf", "obj", "stl", "3mf", "fbx", "ply", "off", "gcode", "gco", "nc", "vox"],
    sniff: any_bytes,
    size: (1180.0, 800.0),
    min_size: (480.0, 420.0),
    on_close: nothing,
};

/// The folder of the model a window is reading or showing: the files it names beside itself come from here, and only
/// from here.
#[derive(Default)]
struct Doc(Mutex<Option<PathBuf>>);

/// Each window's model; another window of the program (the Mac) has its own.
#[derive(Default)]
struct Docs(PerWindow<Doc>);

/// The model's bytes, whole, straight to the page. Answers "missing", "refused" or "damaged" when it can't.
#[tauri::command(async)]
fn read_model(window: WebviewWindow, docs: State<Docs>, path: String) -> Result<Response, String> {
    let path = std::path::absolute(&path).unwrap_or_else(|_| PathBuf::from(&path));
    if !path.is_file() {
        return Err("missing".into());
    }
    if !TOOL.allowed_name(&file_name(&path)) {
        return Err("refused".into());
    }
    let bytes = fs::read(&path).map_err(|_| "damaged".to_string())?;
    *lock(&docs.0.of(window.label()).0) = path.parent().map(Path::to_path_buf);
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

/// The page has read the model: it's now the window's open file, watched, and first in the recent list.
#[tauri::command(async)]
fn adopt_model(window: WebviewWindow, path: String) -> Opened {
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

/// A name a model gives for a file beside it, as a path inside its folder. A name that reaches outside the folder, or an
/// absolute path from the computer it was made on, is looked for by its last part only, in the folder itself.
fn beside(dir: &Path, name: &str) -> Option<PathBuf> {
    let name = name.replace('\\', "/");
    let inside: Option<PathBuf> = {
        let rel = Path::new(&name);
        let plain = rel.components().all(|c| matches!(c, Component::Normal(_) | Component::CurDir));
        plain.then(|| dir.join(rel))
    };
    let last = name.rsplit('/').next().unwrap_or("").to_string();
    let candidates = inside.into_iter().chain((!last.is_empty()).then(|| dir.join(&last)));
    let root = fs::canonicalize(dir).ok()?;
    for p in candidates {
        if let Ok(real) = fs::canonicalize(&p) {
            if real.starts_with(&root) && real.is_file() {
                return Some(real);
            }
        }
    }
    None
}

/// Which of these names are there, beside the model.
#[tauri::command(async)]
fn have_beside(window: WebviewWindow, docs: State<Docs>, names: Vec<String>) -> Vec<bool> {
    let dir = lock(&docs.0.of(window.label()).0).clone();
    names.iter().map(|n| dir.as_deref().and_then(|d| beside(d, n)).is_some()).collect()
}

/// `http://dotmodel.localhost/<name>`: a file the model names beside itself, for the loaders to fetch like any address.
/// Only from beside the model of the window whose page asked (`label`, the WebView's own).
fn serve(app: &AppHandle, label: &str, uri_path: &str) -> tauri::http::Response<Cow<'static, [u8]>> {
    let doc = app.state::<Docs>().0.get(label);
    let dir = doc.and_then(|d| lock(&d.0).clone());
    let name = unescape(uri_path.trim_start_matches('/'));
    let found = dir.as_deref().and_then(|d| beside(d, &name)).and_then(|p| fs::read(p).ok());
    let builder = tauri::http::Response::builder().header("Access-Control-Allow-Origin", "*");
    match found {
        Some(bytes) => builder.status(200).header("Content-Type", kind_of(&name)).body(Cow::Owned(bytes)).unwrap(),
        None => builder.status(404).body(Cow::Borrowed(&[][..])).unwrap(),
    }
}

fn kind_of(name: &str) -> &'static str {
    match name.rsplit('.').next().map(str::to_ascii_lowercase).as_deref() {
        Some("png") => "image/png",
        Some("jpg" | "jpeg") => "image/jpeg",
        Some("webp") => "image/webp",
        Some("bmp") => "image/bmp",
        Some("gif") => "image/gif",
        Some("mtl") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
enum Saved {
    Saved { path: String, name: String, size: u64 },
    Failed { kind: WriteFailure, detail: String },
}

/// The bytes the page made, written whole to the path in the `path` header. A picture of the model isn't a file this
/// tool opens, so it doesn't go on the recent list (header `picture: 1`).
#[tauri::command(async)]
fn save_model(window: WebviewWindow, request: Request) -> Saved {
    let header = |k: &str| request.headers().get(k).and_then(|v| v.to_str().ok()).map(unescape);
    let Some(path) = header("path") else {
        return Saved::Failed { kind: WriteFailure::Other, detail: "no path".into() };
    };
    let InvokeBody::Raw(bytes) = request.body() else {
        return Saved::Failed { kind: WriteFailure::Other, detail: "no bytes".into() };
    };
    let target = PathBuf::from(&path);
    if let Err(e) = atomic_write(&target, bytes) {
        return Saved::Failed { kind: e.kind, detail: e.detail };
    }
    if header("picture").as_deref() != Some("1") {
        dotany_base::saved_in(window.app_handle(), window.label(), &target);
    }
    Saved::Saved { name: file_name(&target), path, size: bytes.len() as u64 }
}

pub fn run() {
    dotany_base::builder_per_window(&TOOL, FileWindows { forget, extra: dotany_base::no_extra })
        .manage(Docs::default())
        .register_asynchronous_uri_scheme_protocol("dotmodel", |ctx, request, responder| {
            let (app, label) = (ctx.app_handle().clone(), ctx.webview_label().to_string());
            let path = request.uri().path().to_string();
            std::thread::spawn(move || responder.respond(serve(&app, &label, &path)));
        })
        .invoke_handler(dotany_base::handler![read_model, adopt_model, have_beside, save_model])
        .run_family(tauri::generate_context!())
        .expect("error while running DotAny 3D");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_stay_inside_the_folder() {
        let dir = std::env::temp_dir().join(format!("dotany-model-beside-{}", std::process::id()));
        fs::create_dir_all(dir.join("tex")).unwrap();
        fs::write(dir.join("a.bin"), b"1").unwrap();
        fs::write(dir.join("tex").join("b.png"), b"2").unwrap();
        assert!(beside(&dir, "a.bin").is_some());
        assert!(beside(&dir, "tex/b.png").is_some());
        assert!(beside(&dir, "tex\\b.png").is_some());
        // an absolute path from another computer: its last part, here
        assert!(beside(&dir, "C:\\Users\\artist\\a.bin").is_some());
        // reaching out finds only what's here by that last name
        assert!(beside(&dir, "../a.bin").is_some());
        assert!(beside(&dir, "../../Windows/win.ini").is_none());
        assert!(beside(&dir, "missing.png").is_none());
        assert_eq!(unescape("my%20model.bin"), "my model.bin");
        assert_eq!(unescape("%E4%B8%AD.png"), "中.png");
        fs::remove_dir_all(&dir).ok();
    }
}
