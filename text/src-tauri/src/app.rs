//! The Tauri side of DotAny Text: windows, one open document per window, and the commands the page calls. The window
//! itself, settings and recent files, the watcher, fonts, atomic saves and the platform layer are dotany-base. Text is
//! the one program of the family with several windows in one process, so it keeps each window's document itself and
//! tells the family only what it opened and saved (`dotany_base::remember_recent`). Word files (`docx`) and the doc, rtf
//! and odt it only reads (`import`) are dotany-words' (`shared/words`), which the hub's converter uses too.

use crate::art;
use crate::document::{self, Eol, EolStyle, Loaded, Overrides};
use crate::encoding::{self, TextEncoding};
use crate::long::{FindHit, Level, Line, LongDoc};
use dotany_base::RunFamily;
use dotany_base::atomic::{WriteFailure, atomic_write};
use dotany_base::watcher::{self, FileWatch};
use dotany_base::{Base, PerWindow, Tool, WindowSpec, lock, platform};
use dotany_words::docx::{self, Block, Docx};
use dotany_words::import;
use serde::Serialize;
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow, WindowEvent};

fn any_bytes(_: &Path) -> bool {
    true
}

fn nothing_to_drop(_: &AppHandle) {}

/// What makes Text itself, for the family. Text answers opening, its dialogs and closing a file per window, itself.
pub static TOOL: Tool = Tool {
    name: "DotAny Text",
    folder: "Text",
    data_env: "DOTANY_TEXT_DATA",
    user_dir: "Documents",
    opens_files: true,
    extensions: &[],
    sniff: any_bytes,
    size: (1120.0, 780.0),
    min_size: (480.0, 420.0),
    on_close: nothing_to_drop,
};

/// Above this a file only opens in the long scroll.
pub const SIZE_LIMIT: u64 = 20 * 1024 * 1024;
const SAMPLE: u64 = 4 * 1024 * 1024;

type Meta = (u64, Option<SystemTime>);

#[derive(Default)]
struct Doc {
    path: Option<PathBuf>,
    loaded: Option<Loaded>,
    long: Option<Arc<LongDoc>>,
    docx: Option<Docx>,
    /// A doc, rtf or odt file's own bytes: shown through `docx`, never written.
    source: Option<Vec<u8>>,
    baseline: Option<Meta>,
    /// Told the page about an outside change and have not heard back yet.
    announced: Option<Meta>,
    must_write: bool,
    watch: Option<FileWatch>,
}

struct AppState {
    /// Each window's document, by the window's label (the family's `PerWindow`, as a tool with a file per window keeps
    /// its own state). Text keeps where the file is and the watcher itself (`Doc`), not in the family's table.
    docs: PerWindow<Mutex<Doc>>,
    seq: AtomicU64,
}

impl AppState {
    fn doc(&self, label: &str) -> Arc<Mutex<Doc>> {
        self.docs.of(label)
    }
}

fn meta_of(path: &Path) -> Option<Meta> {
    fs::metadata(path).ok().map(|m| (m.len(), m.modified().ok()))
}

// ---------- what a command sends back ----------

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
enum Opened {
    Text {
        path: String,
        name: String,
        text: String,
        encoding: String,
        certain: bool,
        lossy: bool,
        eol: String,
        size: u64,
    },
    Long {
        path: String,
        name: String,
        encoding: String,
        certain: bool,
        eol: String,
        size: u64,
        large: bool,
    },
    Docx {
        path: String,
        name: String,
        size: u64,
        #[serde(flatten)]
        doc: docx::Opened,
    },
    /// DOS text art (.nfo, .diz, .ans), read from code page 437: shown, never written back (`art`).
    Art {
        path: String,
        name: String,
        text: String,
        /// "nfo" or "ans"
        kind: String,
        size: u64,
    },
    Binary,
    /// A format Text reads, but this file can't be read: damaged, or not what its name says.
    Damaged,
    Missing,
    Failed {
        detail: String,
    },
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
enum Saved {
    Saved { path: String, name: String, encoding: String, eol: String },
    Nochange { path: String, name: String, encoding: String, eol: String },
    Unmappable { chars: Vec<String>, encoding: String },
    Conflict { deleted: bool },
    Exists,
    Copied,
    Exported,
    Nopath,
    Written { path: String, name: String, assigned: Vec<(u32, u32)> },
    Unchanged { path: String, name: String },
    Failed { kind: WriteFailure, detail: String },
}

fn file_name(path: &Path) -> String {
    path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
}

// ---------- opening ----------

fn is_log(path: &Path) -> bool {
    path.extension().map(|e| e.eq_ignore_ascii_case("log")).unwrap_or(false)
}

fn is_word(path: &Path) -> bool {
    path.extension().map(|e| e.eq_ignore_ascii_case("docx")).unwrap_or(false)
}

/// doc, rtf and odt: opened read-only; a change is saved as a new Word file.
fn read_only_format(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_string_lossy().to_lowercase();
    import::is_read_only_format(&ext).then_some(ext)
}

fn start_watch(app: &AppHandle, label: &str, path: &Path) -> Option<FileWatch> {
    let (app, label) = (app.clone(), label.to_string());
    watcher::watch(path, move || file_touched(&app, &label))
}

fn scan_long(app: &AppHandle, label: &str, doc: Arc<LongDoc>) {
    let (app, label) = (app.clone(), label.to_string());
    std::thread::spawn(move || {
        let _ = doc.scan(|p| {
            let _ = app.emit_to(label.as_str(), "long-progress", p);
        });
    });
}

fn read_head(path: &Path, len: u64) -> std::io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    fs::File::open(path)?.take(len).read_to_end(&mut buf)?;
    Ok(buf)
}

fn open_into(app: &AppHandle, state: &AppState, label: &str, path: PathBuf) -> Opened {
    let Ok(meta) = fs::metadata(&path) else { return Opened::Missing };
    if !meta.is_file() {
        return Opened::Missing;
    }
    let size = meta.len();
    let path_text = path.to_string_lossy().into_owned();
    let name = file_name(&path);
    let reply;
    let mut new_doc =
        Doc { path: Some(path.clone()), baseline: Some((size, meta.modified().ok())), ..Default::default() };
    if is_word(&path) {
        let bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(e) => return Opened::Failed { detail: e.to_string() },
        };
        // an empty file, as Explorer's New > Word document makes, is a blank document; it's written as a whole one when saved
        let opened = if bytes.is_empty() { docx::blank().map_err(|_| ()) } else { docx::open(bytes).map_err(|_| ()) };
        let Ok((word, doc)) = opened else { return Opened::Damaged };
        new_doc.docx = Some(word);
        reply = Opened::Docx { path: path_text.clone(), name, size, doc };
    } else if let Some(ext) = read_only_format(&path) {
        let bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(e) => return Opened::Failed { detail: e.to_string() },
        };
        let Some((word, doc)) = import::open(&bytes, &ext) else {
            return if import::is_unsupported_kind(&bytes) { Opened::Binary } else { Opened::Damaged };
        };
        new_doc.docx = Some(word);
        new_doc.source = Some(bytes);
        reply = Opened::Docx { path: path_text.clone(), name, size, doc };
    } else if let Some(kind) = art::kind_of(&path).filter(|_| size <= SIZE_LIMIT) {
        let bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(e) => return Opened::Failed { detail: e.to_string() },
        };
        reply = Opened::Art {
            path: path_text.clone(),
            name,
            text: art::decode(&bytes, kind),
            kind: kind.to_string(),
            size,
        };
    } else if size > SIZE_LIMIT || is_log(&path) {
        let head = match read_head(&path, SAMPLE) {
            Ok(h) => h,
            Err(e) => return Opened::Failed { detail: e.to_string() },
        };
        let Some(found) = encoding::detect(&head, size <= SAMPLE) else { return Opened::Binary };
        let (raw, _) = encoding::decode(&head, found.encoding);
        let long = LongDoc::new(&path, found.encoding, size);
        new_doc.long = Some(long.clone());
        reply = Opened::Long {
            path: path_text.clone(),
            name,
            encoding: found.encoding.label(),
            certain: found.certain,
            eol: document::eol_style(&raw).label().to_string(),
            size,
            large: size > SIZE_LIMIT,
        };
        scan_long(app, label, long);
    } else {
        let bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(e) => return Opened::Failed { detail: e.to_string() },
        };
        let Some(mut loaded) = document::load(bytes) else { return Opened::Binary };
        let text = std::mem::take(&mut loaded.text);
        reply = Opened::Text {
            path: path_text.clone(),
            name,
            text,
            encoding: loaded.encoding.label(),
            certain: loaded.certain,
            lossy: loaded.lossy,
            eol: loaded.eol.label().to_string(),
            size,
        };
        new_doc.loaded = Some(loaded);
    }
    new_doc.watch = start_watch(app, label, &path);
    let slot = state.doc(label);
    let mut doc = lock(&slot);
    if let Some(old) = &doc.long {
        old.stop();
    }
    *doc = new_doc;
    drop(doc);
    remember(app, state, &path_text);
    reply
}

/// First on the family's recent list (DotAny's Recent too) and on Text's own; every window hears it.
fn remember(app: &AppHandle, _state: &AppState, path: &str) {
    dotany_base::remember_recent(app, Path::new(path));
}

#[tauri::command(async)]
fn open_path(app: AppHandle, window: WebviewWindow, state: State<AppState>, path: String) -> Opened {
    let path = std::path::absolute(&path).unwrap_or_else(|_| PathBuf::from(&path));
    open_into(&app, &state, window.label(), path)
}

/// The open dialog, with no filter: Text opens any file that is text. It starts where this window's file is.
#[tauri::command(async)]
fn pick_open(window: WebviewWindow, state: State<AppState>, base: State<Base>) -> Option<String> {
    // the Mac: the family's open panel, as the window's sheet like every other tool's (and a test's answer in its place)
    #[cfg(target_os = "macos")]
    {
        let (tx, rx) = std::sync::mpsc::channel();
        dotany_base::platform::open_panel(Some(&window), &[], start_folder(&state, &base, window.label()), move |p| {
            let _ = tx.send(p);
        });
        rx.recv().ok().flatten().map(|p| p.to_string_lossy().into_owned())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let mut dialog = rfd::FileDialog::new().set_parent(&window);
        if let Some(dir) = start_folder(&state, &base, window.label()) {
            dialog = dialog.set_directory(dir);
        }
        dialog.pick_file().map(|p| p.to_string_lossy().into_owned())
    }
}

fn start_folder(state: &AppState, base: &Base, label: &str) -> Option<PathBuf> {
    if let Some(p) = lock(&state.doc(label)).path.clone() {
        return p.parent().map(Path::to_path_buf);
    }
    base.recents().first().and_then(|p| Path::new(p).parent().map(Path::to_path_buf))
}

/// Where Save As starts: this window's file's folder, else the last file's folder, else Documents.
#[tauri::command(async)]
fn default_folder(window: WebviewWindow, state: State<AppState>, base: State<Base>) -> String {
    if let Some(dir) = start_folder(&state, &base, window.label()).filter(|d| d.is_dir()) {
        return dir.to_string_lossy().into_owned();
    }
    platform::user_dir(TOOL.user_dir).to_string_lossy().into_owned()
}

/// A blank document that exists nowhere yet.
#[tauri::command]
fn new_document(window: WebviewWindow, state: State<AppState>) {
    let slot = state.doc(window.label());
    let mut doc = lock(&slot);
    if let Some(old) = &doc.long {
        old.stop();
    }
    *doc = Doc { loaded: Some(document::fresh_document()), ..Default::default() };
}

#[tauri::command]
fn close_document(window: WebviewWindow, state: State<AppState>) {
    let slot = state.doc(window.label());
    let mut doc = lock(&slot);
    if let Some(old) = &doc.long {
        old.stop();
    }
    *doc = Doc::default();
}

// ---------- reading again: another encoding, or the new version ----------

#[derive(Serialize)]
struct Redecoded {
    text: String,
    encoding: String,
    lossy: bool,
    eol: String,
}

/// Always decodes the original bytes, so trying a wrong encoding costs nothing.
#[tauri::command(async)]
fn reopen_as(
    app: AppHandle,
    window: WebviewWindow,
    state: State<AppState>,
    label: String,
) -> Result<Option<Redecoded>, String> {
    let enc = TextEncoding::from_label(&label).ok_or("unknown encoding")?;
    let slot = state.doc(window.label());
    let mut doc = lock(&slot);
    if let Some(old) = doc.long.clone() {
        old.stop();
        let path = old.path.clone();
        let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        let long = LongDoc::new(&path, enc, size);
        doc.long = Some(long.clone());
        scan_long(&app, window.label(), long);
        return Ok(None);
    }
    let old = doc.loaded.as_ref().ok_or("nothing open")?;
    let mut loaded = document::read_as(old.bytes.clone(), enc, false);
    let text = std::mem::take(&mut loaded.text);
    let reply = Redecoded { text, encoding: enc.label(), lossy: loaded.lossy, eol: loaded.eol.label().to_string() };
    doc.loaded = Some(loaded);
    Ok(Some(reply))
}

#[tauri::command(async)]
fn reload_document(app: AppHandle, window: WebviewWindow, state: State<AppState>) -> Opened {
    let path = lock(&state.doc(window.label())).path.clone();
    match path {
        Some(p) => open_into(&app, &state, window.label(), p),
        None => Opened::Missing,
    }
}

/// "Keep mine": what is on disk now is accepted as the thing we will overwrite.
#[tauri::command(async)]
fn keep_mine(window: WebviewWindow, state: State<AppState>) {
    let slot = state.doc(window.label());
    let mut doc = lock(&slot);
    doc.baseline = doc.path.as_deref().and_then(meta_of);
    doc.announced = None;
    // an unchanged-looking save must still write, because the disk now holds someone else's text
    doc.must_write = true;
}

// ---------- another program changed the file ----------

fn file_touched(app: &AppHandle, label: &str) {
    let state = app.state::<AppState>();
    let slot = state.doc(label);
    let mut doc = lock(&slot);
    let Some(path) = doc.path.clone() else { return };
    let now = meta_of(&path);
    if now == doc.baseline || (now.is_some() && now == doc.announced) {
        return;
    }
    if let Some(long) = doc.long.clone() {
        let Some((size, _)) = now else { return };
        doc.baseline = now;
        if size >= long.progress().scanned {
            // a growing log: read the new part
            scan_long(app, label, long);
            let _ = app.emit_to(label, "long-grew", ());
        } else {
            long.stop();
            let fresh = LongDoc::new(&path, long.encoding(), size);
            doc.long = Some(fresh.clone());
            scan_long(app, label, fresh);
            let _ = app.emit_to(label, "long-reset", ());
        }
        return;
    }
    let known = doc.loaded.as_ref().map(|l| &l.bytes).or(doc.source.as_ref()).or(doc.docx.as_ref().map(|w| &w.bytes));
    if let (Some(_), Some(known)) = (now, known) {
        // touched but not changed
        if fs::read(&path).map(|b| &b == known).unwrap_or(false) {
            doc.baseline = now;
            doc.announced = None;
            return;
        }
    }
    if doc.announced.is_some() && now.is_none() {
        return;
    }
    doc.announced = now.or(Some((0, None)));
    let _ = app.emit_to(label, "file-changed", now.is_none());
}

// ---------- saving ----------

fn saved_reply(doc: &Doc, changed: bool) -> Saved {
    let path = doc.path.as_deref().unwrap_or(Path::new(""));
    let loaded = doc.loaded.as_ref();
    let (path, name, encoding, eol) = (
        path.to_string_lossy().into_owned(),
        file_name(path),
        loaded.map(|l| l.encoding.label()).unwrap_or_default(),
        loaded.map(|l| l.eol.label().to_string()).unwrap_or_default(),
    );
    if changed { Saved::Saved { path, name, encoding, eol } } else { Saved::Nochange { path, name, encoding, eol } }
}

fn overrides(encoding: Option<String>, eol: Option<String>) -> Overrides {
    Overrides {
        encoding: encoding.as_deref().and_then(TextEncoding::from_label),
        eol: eol.as_deref().and_then(Eol::from_label),
    }
}

fn after_write(doc: &mut Doc, path: &Path, bytes: Vec<u8>, over: Overrides) {
    let enc = over.encoding.or(doc.loaded.as_ref().map(|l| l.encoding)).unwrap_or(TextEncoding::UTF8);
    let mut fresh = document::read_as(bytes, enc, true);
    fresh.text = String::new();
    if let EolStyle::None = fresh.eol {
        if let Some(old) = doc.loaded.as_ref() {
            fresh.eol = old.eol;
        }
    }
    doc.loaded = Some(fresh);
    doc.baseline = meta_of(path);
    doc.announced = None;
    doc.must_write = false;
}

#[tauri::command(async)]
fn save_document(
    window: WebviewWindow,
    state: State<AppState>,
    text: String,
    encoding: Option<String>,
    eol: Option<String>,
) -> Saved {
    let slot = state.doc(window.label());
    let mut doc = lock(&slot);
    let Some(path) = doc.path.clone() else { return Saved::Nopath };
    let Some(loaded) = doc.loaded.as_ref() else { return Saved::Nopath };
    let over = overrides(encoding, eol);
    let bytes = match document::render(loaded, &text, over) {
        Ok(b) => b,
        Err(chars) => {
            return Saved::Unmappable {
                chars: chars.into_iter().map(String::from).collect(),
                encoding: over.encoding.unwrap_or(loaded.encoding).label(),
            };
        }
    };
    let disk = meta_of(&path);
    if disk != doc.baseline {
        let same = fs::read(&path).map(|b| b == loaded.bytes).unwrap_or(false);
        if !same {
            return Saved::Conflict { deleted: disk.is_none() };
        }
    }
    if disk.is_some() && bytes == loaded.bytes && !doc.must_write {
        // nothing would change: the disk is not touched at all
        return saved_reply(&doc, false);
    }
    match atomic_write(&path, &bytes) {
        Ok(()) => {
            after_write(&mut doc, &path, bytes, over);
            saved_reply(&doc, true)
        }
        Err(e) => Saved::Failed { kind: e.kind, detail: e.detail },
    }
}

#[tauri::command(async)]
#[allow(clippy::too_many_arguments)]
fn save_document_as(
    app: AppHandle,
    window: WebviewWindow,
    state: State<AppState>,
    path: String,
    text: String,
    overwrite: bool,
    encoding: Option<String>,
    eol: Option<String>,
) -> Saved {
    let target = PathBuf::from(&path);
    let slot = state.doc(window.label());
    let mut doc = lock(&slot);
    let is_same = doc.path.as_deref().map(|p| same_file(p, &target)).unwrap_or(false);
    if target.exists() && !overwrite && !is_same {
        return Saved::Exists;
    }
    let Some(loaded) = doc.loaded.as_ref() else { return Saved::Nopath };
    let over = overrides(encoding, eol);
    let bytes = match document::render(loaded, &text, over) {
        Ok(b) => b,
        Err(chars) => {
            return Saved::Unmappable {
                chars: chars.into_iter().map(String::from).collect(),
                encoding: over.encoding.unwrap_or(loaded.encoding).label(),
            };
        }
    };
    if let Err(e) = atomic_write(&target, &bytes) {
        return Saved::Failed { kind: e.kind, detail: e.detail };
    }
    doc.path = Some(target.clone());
    after_write(&mut doc, &target, bytes, over);
    doc.watch = start_watch(&app, window.label(), &target);
    let reply = saved_reply(&doc, true);
    drop(doc);
    remember(&app, &state, &path);
    reply
}

fn same_file(a: &Path, b: &Path) -> bool {
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => x == y,
        _ => false,
    }
}

// ---------- Word ----------

#[tauri::command(async)]
fn new_word_document(window: WebviewWindow, state: State<AppState>) -> Result<docx::Opened, String> {
    let (word, opened) = docx::blank()?;
    let slot = state.doc(window.label());
    let mut doc = lock(&slot);
    if let Some(old) = &doc.long {
        old.stop();
    }
    *doc = Doc { docx: Some(word), ..Default::default() };
    Ok(opened)
}

#[tauri::command(async)]
fn save_word(window: WebviewWindow, state: State<AppState>, blocks: Vec<Block>) -> Saved {
    let slot = state.doc(window.label());
    let mut doc = lock(&slot);
    let Some(path) = doc.path.clone() else { return Saved::Nopath };
    let Some(word) = doc.docx.as_ref() else { return Saved::Nopath };
    if word.read_only.is_some() {
        return Saved::Failed { kind: WriteFailure::Other, detail: "read only".into() };
    }
    let disk = meta_of(&path);
    if disk != doc.baseline {
        let same = fs::read(&path).map(|b| b == word.bytes).unwrap_or(false);
        if !same {
            return Saved::Conflict { deleted: disk.is_none() };
        }
    }
    let base = path.parent().map(Path::to_path_buf);
    let done = match word.prepare(&blocks, base.as_deref()) {
        Ok(d) => d,
        Err(e) => return Saved::Failed { kind: WriteFailure::Other, detail: e },
    };
    let name = file_name(&path);
    let text = path.to_string_lossy().into_owned();
    if !done.changed && disk.is_some() && !doc.must_write {
        // nothing would change: the disk is not touched at all
        doc.docx = Some(done.next);
        return Saved::Unchanged { path: text, name };
    }
    match atomic_write(&path, &done.next.bytes) {
        Ok(()) => {
            doc.docx = Some(done.next);
            doc.baseline = meta_of(&path);
            doc.announced = None;
            doc.must_write = false;
            Saved::Written { path: text, name, assigned: done.assigned }
        }
        Err(e) => Saved::Failed { kind: e.kind, detail: e.detail },
    }
}

/// The same document under another name; this window follows it.
#[tauri::command(async)]
fn save_word_as(
    app: AppHandle,
    window: WebviewWindow,
    state: State<AppState>,
    path: String,
    blocks: Vec<Block>,
    overwrite: bool,
) -> Saved {
    let target = PathBuf::from(&path);
    let slot = state.doc(window.label());
    let mut doc = lock(&slot);
    let is_same = doc.path.as_deref().map(|p| same_file(p, &target)).unwrap_or(false);
    if target.exists() && !overwrite && !is_same {
        return Saved::Exists;
    }
    let Some(word) = doc.docx.as_ref() else { return Saved::Nopath };
    let base = target.parent().map(Path::to_path_buf);
    let done = match word.prepare(&blocks, base.as_deref()) {
        Ok(d) => d,
        Err(e) => return Saved::Failed { kind: WriteFailure::Other, detail: e },
    };
    if let Err(e) = atomic_write(&target, &done.next.bytes) {
        return Saved::Failed { kind: e.kind, detail: e.detail };
    }
    doc.docx = Some(done.next);
    doc.path = Some(target.clone());
    doc.baseline = meta_of(&target);
    doc.announced = None;
    doc.must_write = false;
    doc.watch = start_watch(&app, window.label(), &target);
    drop(doc);
    remember(&app, &state, &path);
    Saved::Written { name: file_name(&target), path, assigned: done.assigned }
}

/// A new Word file from a document in another format. This window stays on the file it has.
#[tauri::command(async)]
fn export_word(
    app: AppHandle,
    state: State<AppState>,
    path: String,
    blocks: Vec<Block>,
    overwrite: bool,
    base: Option<String>,
) -> Saved {
    let target = PathBuf::from(&path);
    if target.exists() && !overwrite {
        return Saved::Exists;
    }
    let (blank, _) = match docx::blank() {
        Ok(b) => b,
        Err(e) => return Saved::Failed { kind: WriteFailure::Other, detail: e },
    };
    let done = match blank.prepare(&blocks, base.as_deref().map(Path::new)) {
        Ok(d) => d,
        Err(e) => return Saved::Failed { kind: WriteFailure::Other, detail: e },
    };
    if let Err(e) = atomic_write(&target, &done.next.bytes) {
        return Saved::Failed { kind: e.kind, detail: e.detail };
    }
    remember(&app, &state, &path);
    Saved::Exported
}

/// A doc, rtf or odt file as a Word document: the one shown, exactly. The original is never the target.
#[tauri::command(async)]
fn save_converted_as(
    app: AppHandle,
    window: WebviewWindow,
    state: State<AppState>,
    path: String,
    overwrite: bool,
) -> Saved {
    let target = PathBuf::from(&path);
    let slot = state.doc(window.label());
    let doc = lock(&slot);
    let (Some(word), Some(_)) = (doc.docx.as_ref(), doc.source.as_ref()) else { return Saved::Nopath };
    if doc.path.as_deref().is_some_and(|p| same_file(p, &target) || p == target) {
        return Saved::Failed { kind: WriteFailure::Other, detail: "the original file".into() };
    }
    if target.exists() && !overwrite {
        return Saved::Exists;
    }
    if let Err(e) = atomic_write(&target, &word.bytes) {
        return Saved::Failed { kind: e.kind, detail: e.detail };
    }
    drop(doc);
    remember(&app, &state, &path);
    Saved::Exported
}

/// A new text file from a document in another format, in UTF-8.
#[tauri::command(async)]
fn export_text(app: AppHandle, state: State<AppState>, path: String, text: String, overwrite: bool) -> Saved {
    let target = PathBuf::from(&path);
    if target.exists() && !overwrite {
        return Saved::Exists;
    }
    if let Err(e) = atomic_write(&target, text.as_bytes()) {
        return Saved::Failed { kind: e.kind, detail: e.detail };
    }
    remember(&app, &state, &path);
    Saved::Exported
}

/// A Word document as Markdown, with its pictures. `text` is the page's Markdown, each picture still written with the
/// address the page shows it by (`docx:rId5`, or a data URL for one pasted in); `pictures` are those addresses, each once,
/// in the document's order. Each is written beside the Markdown as `<its name> 图1.png`… (`word` is the language's
/// `图{n}`), under a name nothing has, and the Markdown links to it by that name (`dotany_base::beside`, as the hub's
/// converter does). A picture that can't be read is left as it was. This window stays on the file it has.
#[tauri::command(async)]
#[allow(clippy::too_many_arguments)]
fn export_markdown(
    app: AppHandle,
    window: WebviewWindow,
    state: State<AppState>,
    path: String,
    text: String,
    pictures: Vec<String>,
    word: String,
    overwrite: bool,
) -> Saved {
    use dotany_base::beside;
    let target = PathBuf::from(&path);
    if target.exists() && !overwrite {
        return Saved::Exists;
    }
    if target.parent().is_none() {
        return Saved::Nopath;
    }
    let found: Vec<beside::Picture> = {
        let slot = state.doc(window.label());
        let doc = lock(&slot);
        pictures
            .into_iter()
            .filter_map(|src| {
                let (bytes, kind) = if let Some(rel) = src.strip_prefix("docx:") {
                    doc.docx.as_ref()?.media(rel)?
                } else if src.starts_with("data:") {
                    docx::data_url(&src)?
                } else {
                    return None;
                };
                Some(beside::Picture { ext: beside::picture_ext(&kind)?, src, bytes })
            })
            .collect()
    };
    match beside::write_markdown(&target, &text, &found, &word, atomic_write) {
        Ok(_) => {}
        Err(beside::WriteError::Picture(e)) => {
            return Saved::Failed { kind: platform::write_failure(&e), detail: e.to_string() };
        }
        Err(beside::WriteError::Markdown(e)) => return Saved::Failed { kind: e.kind, detail: e.detail },
    }
    remember(&app, &state, &path);
    Saved::Exported
}

/// Save As from the long scroll: a straight copy of the bytes, however large the file is.
#[tauri::command(async)]
fn copy_long_as(window: WebviewWindow, state: State<AppState>, path: String, overwrite: bool) -> Saved {
    let target = PathBuf::from(&path);
    let Some(from) = lock(&state.doc(window.label())).path.clone() else { return Saved::Nopath };
    if same_file(&from, &target) {
        return Saved::Copied;
    }
    if target.exists() && !overwrite {
        return Saved::Exists;
    }
    let Some(dir) = target.parent() else { return Saved::Nopath };
    let temp = dir.join(format!("{}{}.tmp", dotany_base::atomic::TEMP_PREFIX, std::process::id()));
    if let Err(e) = fs::copy(&from, &temp).and_then(|_| fs::rename(&temp, &target)) {
        let _ = fs::remove_file(&temp);
        return Saved::Failed { kind: platform::write_failure(&e), detail: e.to_string() };
    }
    Saved::Copied
}

// ---------- the long scroll ----------

fn long_of(state: &AppState, label: &str) -> Option<Arc<LongDoc>> {
    lock(&state.doc(label)).long.clone()
}

#[tauri::command(async)]
fn long_lines(window: WebviewWindow, state: State<AppState>, start: u64, count: u64) -> Result<Vec<Line>, String> {
    let doc = long_of(&state, window.label()).ok_or("no long document")?;
    doc.fetch(start, count.min(2000)).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn long_state(window: WebviewWindow, state: State<AppState>) -> Result<crate::long::Progress, String> {
    Ok(long_of(&state, window.label()).ok_or("no long document")?.progress())
}

#[tauri::command(async)]
fn long_filter(app: AppHandle, window: WebviewWindow, state: State<AppState>, level: String) -> Result<(), String> {
    let doc = long_of(&state, window.label()).ok_or("no long document")?;
    let level = match level.as_str() {
        "warn" => Level::Warn,
        "error" => Level::Error,
        _ => Level::All,
    };
    let label = window.label().to_string();
    std::thread::spawn(move || {
        let _ = doc.apply_filter(level, |p| {
            let _ = app.emit_to(label.as_str(), "long-progress", p);
        });
    });
    Ok(())
}

#[tauri::command(async)]
#[allow(clippy::too_many_arguments)]
fn long_find(
    app: AppHandle,
    window: WebviewWindow,
    state: State<AppState>,
    seq: u64,
    query: String,
    match_case: bool,
    whole_word: bool,
    regex: bool,
) -> Result<(), String> {
    let doc = long_of(&state, window.label()).ok_or("no long document")?;
    let label = window.label().to_string();
    std::thread::spawn(move || {
        let _ = doc.start_find(seq, &query, match_case, whole_word, regex, |status| {
            let _ = app.emit_to(label.as_str(), "long-find", status);
        });
    });
    Ok(())
}

#[tauri::command(async)]
fn long_find_step(
    window: WebviewWindow,
    state: State<AppState>,
    from: u64,
    forward: bool,
    current: Option<usize>,
) -> Result<Option<FindHit>, String> {
    Ok(long_of(&state, window.label()).ok_or("no long document")?.find_step(from, forward, current))
}

/// For printing a log that is small enough to hold; a larger one is refused by the page first.
#[tauri::command(async)]
fn long_text(window: WebviewWindow, state: State<AppState>) -> Result<String, String> {
    let doc = long_of(&state, window.label()).ok_or("no long document")?;
    doc.read_all().map_err(|e| e.to_string())
}

// ---------- settings ----------

/// A family setting (appearance, labels, sound, language, autosave) goes to the family, so every program follows it;
/// every setting goes to Text's own file; and Text's other windows hear it at once.
#[tauri::command(async)]
fn set_setting(app: AppHandle, window: WebviewWindow, key: String, value: Option<String>) {
    dotany_base::commands::set_setting(app.state::<Base>(), key.clone(), value.clone());
    let _ = app.emit("setting", serde_json::json!({ "from": window.label(), "key": key, "value": value }));
}

/// Local pictures in a Markdown file are shown through the asset protocol, picture files only (`picture_allowed`). The
/// document's own path is the one this window holds, never one the page sends.
#[tauri::command(async)]
fn allow_images(app: AppHandle, window: WebviewWindow, state: State<AppState>, paths: Vec<String>) {
    const IMAGES: [&str; 8] = ["png", "jpg", "jpeg", "gif", "webp", "svg", "bmp", "avif"];
    let document = lock(&state.doc(window.label())).path.clone();
    for path in paths {
        let p = Path::new(&path);
        let is_image = p.extension().map(|e| IMAGES.iter().any(|x| e.eq_ignore_ascii_case(x))).unwrap_or(false);
        if is_image && picture_allowed(&path, document.as_deref()) {
            let _ = app.asset_protocol_scope().allow_file(p);
        }
    }
}

/// A picture on this computer always; one on another computer (`names_another_machine`) only when the document itself
/// was opened from a network share and the picture lies on that same `\\server\share`: the person chose that share, so
/// reading from it again tells no one anything new. Never a device or verbatim path (`\\?\`, `\\.\`, `\??\`).
fn picture_allowed(picture: &str, document: Option<&Path>) -> bool {
    if !names_another_machine(picture) {
        return true;
    }
    match (share_of(picture), document.and_then(|d| share_of(&d.to_string_lossy()))) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

/// `\\server\share` of a path on a network share, lower-cased, as Windows reads the path once it is made absolute
/// (either slash; `..` never climbs above the share). The server part is compared whole, so `server@SSL` and
/// `server@80` are servers of their own. None for anything else, device and verbatim paths included; and on other
/// systems, where `//server/share` is not a network path.
#[cfg(windows)]
fn share_of(path: &str) -> Option<(String, String)> {
    let b = path.as_bytes();
    let sep = |c: u8| c == b'\\' || c == b'/';
    // two separators first (`\??\` has one), and not `\\?\` or `\\.\`
    let two = b.len() >= 2 && sep(b[0]) && sep(b[1]);
    let device = b.len() >= 4 && matches!(b[2], b'?' | b'.') && sep(b[3]);
    if !two || device {
        return None;
    }
    let abs = std::path::absolute(path).ok()?;
    match abs.components().next() {
        Some(std::path::Component::Prefix(p)) => match p.kind() {
            std::path::Prefix::UNC(server, share) => {
                Some((server.to_string_lossy().to_lowercase(), share.to_string_lossy().to_lowercase()))
            }
            _ => None,
        },
        _ => None,
    }
}

#[cfg(not(windows))]
fn share_of(_: &str) -> Option<(String, String)> {
    None
}

/// A path to another computer or to a device rather than to a file on this one: `\\host\share\…` and `//host/share/…`
/// (`\\host@80\…` is WebDAV), `\\?\…`, `\\.\…` and `\??\…`. Anything that so much as looks at such a path makes Windows
/// connect to that server, over SMB or WebDAV, and sign in to it with the user's account: a document that names one
/// would tell whoever sent it that it was opened, and hand over the user's NTLM hash. The scope itself looks at a
/// path when it is allowed (it resolves it), so the refusal comes before. The path is judged as written and again once
/// made absolute, so that `..`, doubled separators or either slash do not turn one into the other. A drive letter
/// mapped to a network share is the user's own doing and is not told apart.
fn names_another_machine(path: &str) -> bool {
    fn starts_remote(s: &str) -> bool {
        let sep = |c: u8| c == b'\\' || c == b'/';
        let b = s.as_bytes();
        (b.len() >= 2 && sep(b[0]) && sep(b[1])) || (b.len() >= 4 && sep(b[0]) && &b[1..3] == b"??" && sep(b[3]))
    }
    if starts_remote(path) {
        return true;
    }
    // on Windows `absolute` is GetFullPathNameW: it reads the text only, and never the disk or the network
    let Ok(abs) = std::path::absolute(path) else { return true };
    if starts_remote(&abs.to_string_lossy()) {
        return true;
    }
    #[cfg(windows)]
    if let Some(std::path::Component::Prefix(p)) = abs.components().next() {
        return !matches!(p.kind(), std::path::Prefix::Disk(_));
    }
    false
}

// ---------- windows ----------

#[derive(Serialize, Default)]
struct StartArgs {
    path: Option<String>,
    format: Option<String>,
}

/// A window of the family's, with a document of its own. `new` is DotAny's New (--new, or --new=md for one format).
fn open_window(app: &AppHandle, start: StartArgs, new: Option<String>) -> tauri::Result<WebviewWindow> {
    let n = app.state::<AppState>().seq.fetch_add(1, Ordering::Relaxed);
    let label = format!("w{n}");
    let extra = serde_json::json!({ "start": start, "new": new });
    // on the Mac cascaded from the window in front, as document windows are; Windows places them as it always has
    dotany_base::open_document_window(
        app,
        WindowSpec {
            label: &label,
            page: "index.html",
            size: TOOL.size,
            min_size: TOOL.min_size,
            visible: true,
            extra,
        },
    )
}

/// A window whose WebView went comes back with the file it had open (dotany-base's `webview`); a new document that was
/// never saved comes back new.
fn reopen(app: &AppHandle, label: &str, extra: &mut serde_json::Value) {
    let state = app.state::<AppState>();
    let slot = state.docs.get(label);
    let path = slot.and_then(|d| lock(&d).path.clone());
    if let (Some(path), Some(extra)) = (path, extra.as_object_mut()) {
        extra.insert(
            "start".into(),
            serde_json::json!(StartArgs { path: Some(path.to_string_lossy().into_owned()), format: None }),
        );
        extra.insert("new".into(), serde_json::Value::Null);
    }
}

/// A window with no document at all: nothing opened, nothing new started in it.
fn holds_nothing(app: &AppHandle, label: &str) -> bool {
    app.state::<AppState>().docs.get(label).map_or(true, |d| {
        let d = lock(&d);
        d.path.is_none() && d.loaded.is_none() && d.docx.is_none() && d.long.is_none()
    })
}

/// New while a file is open starts a new window.
#[tauri::command(async)]
fn new_window(app: AppHandle, format: Option<String>, path: Option<String>) -> Result<(), String> {
    open_window(&app, StartArgs { path, format }, None).map(|_| ()).map_err(|e| e.to_string())
}

pub fn run() {
    // started by DotAny's New: --new, or --new=md for one of the formats New… offers
    let new = std::env::args_os().skip(1).find_map(|a| {
        let a = a.to_string_lossy();
        if a == "--new" { Some(String::new()) } else { a.strip_prefix("--new=").map(str::to_string) }
    });
    dotany_base::set_reopen(reopen);
    dotany_base::builder_with(&TOOL, move |app, path| {
        // the Mac: a file that started Text came before this and has its window already, so no empty one beside it
        // (Windows starts here with no window, always)
        if path.is_none() && new.is_none() && !app.webview_windows().is_empty() {
            return Ok(());
        }
        let format = new.clone().filter(|f| !f.is_empty() && path.is_none());
        open_window(app, StartArgs { path, format }, new.clone()).map(|_| ())
    })
    .manage(AppState { docs: PerWindow::default(), seq: AtomicU64::new(1) })
    // a window's document goes with it
    .on_window_event(|window, event| {
        if let WindowEvent::Destroyed = event {
            let state = window.state::<AppState>();
            let slot = state.docs.remove(window.label());
            if let Some(slot) = slot {
                if let Some(long) = &lock(&slot).long {
                    long.stop();
                }
            }
        }
    })
    // the pictures inside a Word file, for showing
    .register_asynchronous_uri_scheme_protocol("dotmedia", |ctx, request, responder| {
        let app = ctx.app_handle().clone();
        // the page's address encodes the separator between window and picture
        let path = request.uri().path().trim_start_matches('/').replace("%2F", "/");
        std::thread::spawn(move || {
            let state = app.state::<AppState>();
            let mut parts = path.splitn(2, '/');
            let (label, rel) = (parts.next().unwrap_or(""), parts.next().unwrap_or(""));
            let found = lock(&state.doc(label)).docx.as_ref().and_then(|d| d.media(rel));
            let builder = tauri::http::Response::builder().header("Access-Control-Allow-Origin", "*");
            let response = match found {
                Some((bytes, mime)) => builder.status(200).header("Content-Type", mime).body(bytes).unwrap(),
                None => builder.status(404).body(Vec::new()).unwrap(),
            };
            responder.respond(response);
        });
    })
    // the family's window commands; opening, the open dialog, settings and closing a file are Text's own, per window
    .invoke_handler(tauri::generate_handler![
        dotany_base::commands::forget_recent,
        dotany_base::commands::pick_save,
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
        new_word_document,
        save_word,
        save_word_as,
        export_word,
        save_converted_as,
        export_text,
        export_markdown,
        open_path,
        pick_open,
        default_folder,
        new_document,
        close_document,
        reopen_as,
        reload_document,
        keep_mine,
        save_document,
        save_document_as,
        copy_long_as,
        long_lines,
        long_state,
        long_filter,
        long_find,
        long_find_step,
        long_text,
        set_setting,
        allow_images,
        new_window
    ])
    .run_family_with(tauri::generate_context!(), |app, paths| {
        // the Mac hands over files opened in Finder: a file already open brings its window forward; an empty window
        // takes one (the window a file starts Text with, or the one the Dock icon opened); every other opens in a
        // window of its own, as New while a file is open
        for p in paths {
            let windows = app.webview_windows();
            let state = app.state::<AppState>();
            let showing = windows.iter().find(|(l, _)| {
                state.docs.get(l).is_some_and(|d| lock(&d).path.as_deref().is_some_and(|q| same_file(q, &p)))
            });
            if let Some((_, w)) = showing {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
                continue;
            }
            let empty = windows.keys().find(|l| holds_nothing(app, l)).cloned();
            if empty.is_some_and(|label| dotany_base::hand_over(app, &label, p.clone())) {
                continue;
            }
            let _ = open_window(app, StartArgs { path: Some(p.to_string_lossy().into_owned()), format: None }, None);
        }
    })
    .expect("error while running DotAny Text");
}

#[cfg(test)]
mod tests {
    use super::{names_another_machine, picture_allowed};
    use std::path::Path;

    #[test]
    fn a_local_document_never_gets_a_network_picture() {
        #[cfg(windows)]
        let local = Some(Path::new(r"C:\Docs\notes\a.md"));
        #[cfg(not(windows))]
        let local = Some(Path::new("/Docs/notes/a.md"));
        for doc in [local, None] {
            assert!(picture_allowed("pics/x.png", doc));
            for p in [r"\\nas\docs\x.png", "//nas/docs/x.png", r"\\?\UNC\nas\docs\x.png"] {
                assert!(!picture_allowed(p, doc), "{p} for {doc:?}");
            }
        }
    }

    #[cfg(windows)]
    #[test]
    fn a_document_on_a_share_gets_pictures_from_that_share_only() {
        let doc = Some(Path::new(r"\\nas\docs\notes\a.md"));
        for p in [
            r"\\nas\docs\notes\pics\x.png",
            r"\\NAS\Docs\x.png",
            "//nas/docs/x.png",
            r"\/nas\docs/x.png",
            // `..` stops at the share's root
            r"\\nas\docs\notes\..\..\..\x.png",
            r"C:\Docs\x.png",
        ] {
            assert!(picture_allowed(p, doc), "{p}");
        }
        for p in [
            r"\\other\docs\x.png",
            r"\\nas\private\x.png",
            r"\\nas@SSL\docs\x.png",
            r"\\nas@80\docs\x.png",
            r"\\nas@SSL@443\docs\x.png",
            r"\\?\UNC\nas\docs\x.png",
            r"\\.\UNC\nas\docs\x.png",
            r"\??\UNC\nas\docs\x.png",
        ] {
            assert!(!picture_allowed(p, doc), "{p}");
        }
        // a server with a port or SSL is its own server, the same rule
        let dav = Some(Path::new(r"\\nas@SSL\docs\a.md"));
        assert!(picture_allowed(r"\\NAS@ssl\docs\x.png", dav));
        assert!(!picture_allowed(r"\\nas\docs\x.png", dav));
        // a document opened by a device or verbatim path gets no network picture
        assert!(!picture_allowed(r"\\nas\docs\x.png", Some(Path::new(r"\\?\UNC\nas\docs\a.md"))));
    }

    #[test]
    fn pictures_on_other_computers_and_devices_are_refused() {
        for p in [
            r"\\host\share\x.png",
            "//host/share/x.png",
            r"/\host\share\x.png",
            r"\/host/share/x.png",
            r"\\host@80\dav\x.png",
            r"\\host@SSL@443\dav\x.png",
            r"\\?\C:\x.png",
            r"\\?\UNC\host\share\x.png",
            r"\\.\C:\x.png",
            r"\\.\pipe\x.png",
            r"\??\C:\x.png",
            r"\??\UNC\host\share\x.png",
            "/??/C:/x.png",
            "",
        ] {
            assert!(names_another_machine(p), "{p} is refused");
        }
    }

    #[test]
    fn ordinary_pictures_still_pass() {
        #[cfg(windows)]
        let paths = [
            r"C:\Docs\notes\pics\x.png",
            "C:/Docs/notes/pics/x.png",
            r"C:\docs\..\pics\x.png",
            // `..` stops at the drive's root, and doubled separators count as one: these stay on C:
            r"C:\x\..\..\\host\share\x.png",
            r"C:\x\..\..\\\\host\share\x.png",
            "C:/x/../..//host/share/x.png",
            r"\Docs\notes\x.png",
            r"pics\x.png",
            "../pics/x.png",
        ];
        #[cfg(not(windows))]
        let paths = ["/Docs/notes/pics/x.png", "/Docs/notes/docs/../pics/x.png", "pics/x.png", "../pics/x.png"];
        for p in paths {
            assert!(!names_another_machine(p), "{p} is a file on this computer");
        }
    }
}
