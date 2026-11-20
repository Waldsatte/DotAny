//! The Tauri side of DotAny Sheets: what makes this tool itself, and its commands. Everything else is dotany-base.
//!
//! A file is looked at here first: its format by its bytes, not only its name. XLSX and XLSM reach the page as bytes and
//! the page reads them (src/xlsx.js), keeping each cell's own XML so a save rewrites only what changed. XLS, XLSB, ODS
//! and FODS are read here by calamine (`legacy`), DBF tables by `dbf` and SQLite databases by `sqlite`, and all of them
//! are only ever viewed. CSV and TSV are decoded here (`text`), so the encoding that reads them is the one that writes
//! them back. Those modules and `package` are dotany-table's (`shared/table`), which the hub's converter uses too.
//!
//! Sheets keeps a file per window (`dotany_base::builder_per_window`): on the Mac one program shows each workbook in a
//! window of its own; on Windows each workbook is a program of its own, with one window.

use dotany_base::RunFamily;
use dotany_base::atomic::{WriteFailure, atomic_write};
use dotany_base::{Base, FileWindows, PerWindow, Tool, file_name, lock, same_file};
use dotany_table::{dbf, legacy, package, sqlite, text};
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Manager, State, WebviewWindow};

fn any_bytes(_: &Path) -> bool {
    true
}

/// The window's workbook closed, or the window went: the file it was read from, and one being opened, go with it.
fn forget(app: &AppHandle, label: &str) {
    app.state::<Docs>().remove(label);
}

/// Sheets keeps a file per window and says so in `FileWindows` (`forget`); this is never called.
fn nothing(_: &AppHandle) {}

pub static TOOL: Tool = Tool {
    name: "DotAny Sheets",
    folder: "Sheets",
    data_env: "DOTANY_SHEETS_DATA",
    user_dir: "Documents",
    opens_files: true,
    extensions: &["xlsx", "csv", "tsv", "xlsm", "xls", "ods", "xlsb", "fods", "dbf", "sqlite", "sqlite3", "db"],
    sniff: any_bytes,
    size: (1180.0, 800.0),
    min_size: (480.0, 420.0),
    on_close: nothing,
};

/// A file as read from disk, and what it turned out to be.
#[derive(Clone)]
struct Held {
    bytes: Arc<Vec<u8>>,
    kind: &'static str,
    model: Option<Arc<Value>>,
}

/// A window's open file, and the one being opened: it becomes the open one only once the page has read it. Another
/// window of the program (the Mac) has its own; nothing here is seen by it.
#[derive(Default)]
struct Doc {
    open: Mutex<Option<Held>>,
    pending: Mutex<Option<(PathBuf, Held)>>,
}

/// Each window's `Doc`, by its label.
type Docs = PerWindow<Doc>;

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
enum Opened {
    Ok {
        path: String,
        dir: String,
        name: String,
        size: u64,
        kind: &'static str,
        readonly: bool,
    },
    Refused,
    Missing,
    Damaged,
    /// A workbook with a password: its contents are encrypted and Sheets doesn't open those.
    Locked,
}

fn ext_of(name: &str) -> String {
    name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default()
}

const CFB: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

/// An encrypted Office file is a compound file holding "EncryptionInfo" (its name is in UTF-16 in the directory).
fn encrypted(bytes: &[u8]) -> bool {
    let needle: Vec<u8> = "EncryptionInfo".encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
    bytes.starts_with(&CFB) && bytes.windows(needle.len()).any(|w| w == needle.as_slice())
}

/// What the bytes are, whatever the name says. Read-only is the name's or the bytes' old format, whichever applies.
pub fn judge(name: &str, bytes: &[u8]) -> Result<(&'static str, bool), &'static str> {
    let ext = ext_of(name);
    let old_name = matches!(ext.as_str(), "xls" | "ods" | "xlsm");
    if encrypted(bytes) {
        return Err("locked");
    }
    // a database: only under a database's name (.db is also Windows' thumbnail cache and many programs' own files)
    if sqlite::looks(bytes) {
        return if is_database(&ext) { Ok(("sqlite", true)) } else { Err("damaged") };
    }
    if bytes.starts_with(b"PK\x03\x04") {
        return match package::look(bytes) {
            package::Package::Xlsx { macros } if macros || ext == "xlsm" => Ok(("xlsm", true)),
            package::Package::Xlsx { .. } if matches!(ext.as_str(), "xlsx" | "xls") => Ok(("xlsx", old_name)),
            package::Package::Xlsb if matches!(ext.as_str(), "xlsb" | "xlsx" | "xls") => Ok(("xlsb", true)),
            package::Package::Ods if ext == "ods" => Ok(("ods", true)),
            _ => Err("damaged"),
        };
    }
    if ext == "dbf" {
        return if dbf::looks(bytes) { Ok(("dbf", true)) } else { Err("damaged") };
    }
    if ext == "fods" {
        return if legacy::looks_fods(bytes) { Ok(("fods", true)) } else { Err("damaged") };
    }
    if bytes.starts_with(&CFB) {
        return if ext == "xls" { Ok(("xls", true)) } else { Err("damaged") };
    }
    // text: CSV and TSV, and the many "xls" files that systems export as plain tab- or comma-separated text
    if matches!(ext.as_str(), "csv" | "tsv" | "xls") {
        let head = &bytes[..bytes.len().min(65536)];
        let utf16 = head.starts_with(&[0xFF, 0xFE]) || head.starts_with(&[0xFE, 0xFF]);
        if !utf16 && head.iter().filter(|b| **b == 0).count() > head.len() / 100 + 2 {
            return Err("damaged");
        }
        let tabs = head.iter().filter(|b| **b == b'\t').count();
        let commas = head.iter().filter(|b| **b == b',').count();
        let kind = if ext == "tsv" || (ext == "xls" && tabs >= commas) { "tsv" } else { "csv" };
        return Ok((kind, old_name));
    }
    Err("damaged")
}

fn is_database(ext: &str) -> bool {
    matches!(ext, "sqlite" | "sqlite3" | "db")
}

/// Whether a file's first bytes say it is a SQLite database, without reading the rest.
fn database_at(path: &Path) -> bool {
    use std::io::Read;
    let mut head = [0u8; 16];
    fs::File::open(path).and_then(|mut f| f.read_exact(&mut head)).is_ok() && sqlite::looks(&head)
}

#[tauri::command(async)]
fn open_file(window: WebviewWindow, docs: State<Docs>, path: String) -> Opened {
    let path = std::path::absolute(&path).unwrap_or_else(|_| PathBuf::from(&path));
    if !path.is_file() {
        return Opened::Missing;
    }
    let name = file_name(&path);
    if !TOOL.allowed_name(&name) {
        return Opened::Refused;
    }
    // a database is read where it is, a table at a time and only so much of each (sqlite.rs), never all into memory
    let database = is_database(&ext_of(&name)) && database_at(&path);
    let Ok(bytes) = (if database { Ok(Vec::new()) } else { fs::read(&path) }) else { return Opened::Damaged };
    let judged = if database { Ok(("sqlite", true)) } else { judge(&name, &bytes) };
    let (kind, readonly) = match judged {
        Ok(k) => k,
        Err("locked") => return Opened::Locked,
        Err(_) => return Opened::Damaged,
    };
    let model = match kind {
        "xls" | "ods" | "xlsb" | "fods" => legacy::workbook(kind, bytes.clone()),
        "dbf" => {
            let memo = dbf::memo_beside(&path).and_then(|m| fs::read(m).ok());
            let stem = name.rsplit_once('.').map_or(name.as_str(), |(s, _)| s);
            dbf::workbook(&bytes, memo.as_deref(), stem)
        }
        "sqlite" => sqlite::workbook(&path, sqlite::VIEW),
        _ => Ok(Value::Null),
    };
    let model = match model {
        Ok(Value::Null) => None,
        Ok(m) => Some(Arc::new(m)),
        Err(_) => return Opened::Damaged,
    };
    let size = if database { fs::metadata(&path).map(|m| m.len()).unwrap_or(0) } else { bytes.len() as u64 };
    // the window went while its file was being read: nothing keeps it
    if window.app_handle().get_webview_window(window.label()).is_none() {
        return Opened::Missing;
    }
    *lock(&docs.of(window.label()).pending) = Some((path.clone(), Held { bytes: Arc::new(bytes), kind, model }));
    Opened::Ok {
        dir: path.parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default(),
        name,
        path: path.to_string_lossy().into_owned(),
        size,
        kind,
        readonly,
    }
}

fn held(docs: &Docs, window: &WebviewWindow) -> Option<Held> {
    let doc = docs.get(window.label())?;
    if let Some((_, h)) = lock(&doc.pending).as_ref() {
        return Some(h.clone());
    }
    let open = lock(&doc.open).clone();
    open
}

/// The workbook being opened, as bytes, for the page to read (XLSX, XLSM).
#[tauri::command(async)]
fn sheet_bytes(window: WebviewWindow, docs: State<Docs>) -> tauri::ipc::Response {
    tauri::ipc::Response::new(held(&docs, &window).map(|h| h.bytes.as_ref().clone()).unwrap_or_default())
}

/// The workbook being opened, as cells (XLS, XLSB, ODS, FODS, DBF, SQLite).
#[tauri::command(async)]
fn sheet_model(window: WebviewWindow, docs: State<Docs>) -> Value {
    held(&docs, &window).and_then(|h| h.model).map(|m| m.as_ref().clone()).unwrap_or(Value::Null)
}

/// The text being opened, or the open one read again in another encoding (CSV, TSV).
#[tauri::command(async)]
fn sheet_text(window: WebviewWindow, docs: State<Docs>, enc: Option<String>) -> Option<text::Decoded> {
    held(&docs, &window).map(|h| text::decode(&h.bytes, enc.as_deref()))
}

/// The page has read it: it's the open file now, watched and on the recent list.
#[tauri::command(async)]
fn sheet_opened(window: WebviewWindow, docs: State<Docs>, path: String) {
    let Some(doc) = docs.get(window.label()) else { return };
    let mut pending = lock(&doc.pending);
    if let Some((p, h)) = pending.take() {
        if p.to_string_lossy() == path {
            *lock(&doc.open) = Some(h);
            dotany_base::opened_in(window.app_handle(), window.label(), &p);
        }
    }
}

/// The page couldn't read it: nothing changes.
#[tauri::command(async)]
fn sheet_failed(window: WebviewWindow, docs: State<Docs>) {
    if let Some(doc) = docs.get(window.label()) {
        *lock(&doc.pending) = None;
    }
}

/// A new workbook: nothing on disk yet, nothing to copy from.
#[tauri::command(async)]
fn sheet_new(window: WebviewWindow, docs: State<Docs>) {
    let doc = docs.of(window.label());
    *lock(&doc.pending) = None;
    *lock(&doc.open) = Some(Held { bytes: Arc::new(Vec::new()), kind: "new", model: None });
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
enum Saved {
    Saved { path: String, name: String, size: u64 },
    Nothing,
    Failed { kind: String, detail: String },
}

fn failed(kind: WriteFailure, detail: String) -> Saved {
    let kind =
        serde_json::to_value(&kind).ok().and_then(|v| v.as_str().map(str::to_string)).unwrap_or_else(|| "other".into());
    Saved::Failed { kind, detail }
}

/// Writes `bytes` to `path`; `adopt` then makes the written file the window's own (recent list, open file, watcher). A
/// copy of the filtered and sorted view (`export`) is only written: the window stays on the file it has open.
fn put(path: String, bytes: &[u8], export: bool, adopt: impl FnOnce(&Path)) -> Saved {
    let target = PathBuf::from(&path);
    if let Err(e) = atomic_write(&target, bytes) {
        return failed(e.kind, e.detail);
    }
    if !export {
        adopt(&target);
    }
    Saved::Saved { name: file_name(&target), path, size: bytes.len() as u64 }
}

fn write(window: &WebviewWindow, path: String, bytes: &[u8], export: bool) -> Saved {
    let (app, label) = (window.app_handle(), window.label());
    put(path, bytes, export, |target| {
        dotany_base::saved_in(app, label, target);
        // the window moves to what was written; the watcher with it
        let state = app.state::<Base>();
        if !state.current_in(label).is_some_and(|p| same_file(&p, target)) {
            dotany_base::opened_in(app, label, target);
        }
    })
}

/// XLSX: the parts the page rewrote, over the file it was opened from (`fresh`: a new package, nothing to copy;
/// `export`: a copy of the filtered and sorted view, which is not the open file).
#[tauri::command(async)]
fn save_xlsx(
    window: WebviewWindow,
    docs: State<Docs>,
    path: String,
    parts: BTreeMap<String, String>,
    remove: Vec<String>,
    fresh: bool,
    export: Option<bool>,
) -> Saved {
    let Some(doc) = docs.get(window.label()) else { return Saved::Nothing };
    let Some(h) = lock(&doc.open).clone() else { return Saved::Nothing };
    let base = if fresh || !matches!(h.kind, "xlsx" | "xlsm") { None } else { Some(h.bytes.as_slice()) };
    let remove: HashSet<String> = remove.into_iter().collect();
    match package::build(base, &parts, &remove) {
        Ok(bytes) => write(&window, path, &bytes, export == Some(true)),
        Err(detail) => Saved::Failed { kind: "other".into(), detail },
    }
}

/// CSV, TSV: the text in the encoding asked for. A character that encoding can't hold stops the save. `export`: as for
/// `save_xlsx`.
#[tauri::command(async)]
fn save_text(window: WebviewWindow, path: String, text: String, enc: String, bom: bool, export: Option<bool>) -> Saved {
    match text::encode(&text, &enc, bom) {
        Ok(bytes) => write(&window, path, &bytes, export == Some(true)),
        Err(()) => Saved::Failed { kind: "encoding".into(), detail: enc },
    }
}

pub fn run() {
    dotany_base::builder_per_window(&TOOL, FileWindows { forget, extra: dotany_base::no_extra })
        .manage(Docs::default())
        .invoke_handler(dotany_base::handler![
            open_file,
            sheet_bytes,
            sheet_model,
            sheet_text,
            sheet_opened,
            sheet_failed,
            sheet_new,
            save_xlsx,
            save_text
        ])
        .run_family(tauri::generate_context!())
        .expect("error while running DotAny Sheets");
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The folder of real workbooks the test below reads, where there is one: otherwise that test ends at once.
    fn real_files() -> Option<PathBuf> {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../test/files");
        if dir.is_dir() {
            return Some(dir);
        }
        eprintln!("skipped: {} is not here", dir.display());
        None
    }

    /// A copy of the filtered and sorted view is written and nothing else: the window's file, its watcher and the
    /// recent list are what `adopt` changes, and an export never reaches it. A save of the file itself does.
    #[test]
    fn an_export_is_only_written() {
        let dir = std::env::temp_dir().join(format!("dotany-sheets-export-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let (copy, own) = (dir.join("copy.csv"), dir.join("own.csv"));
        let adopted = std::cell::RefCell::new(Vec::<PathBuf>::new());
        let adopt = |p: &Path| adopted.borrow_mut().push(p.to_path_buf());

        let r = put(copy.to_string_lossy().into_owned(), b"a,b\n", true, adopt);
        assert!(matches!(r, Saved::Saved { size: 4, .. }));
        assert_eq!(fs::read(&copy).unwrap(), b"a,b\n", "the copy is on disk");
        assert!(adopted.borrow().is_empty(), "and the program's idea of the open file is untouched");

        let r = put(own.to_string_lossy().into_owned(), b"1,2\n", false, adopt);
        assert!(matches!(r, Saved::Saved { size: 4, .. }));
        assert_eq!(*adopted.borrow(), [own.clone()], "a save of the file itself is the open file now");

        // a write that fails adopts nothing either way
        let missing = dir.join("no-such-folder").join("x.csv");
        let r = put(missing.to_string_lossy().into_owned(), b"x", false, adopt);
        assert!(matches!(r, Saved::Failed { .. }));
        assert_eq!(adopted.borrow().len(), 1);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_bytes_decide_not_the_name() {
        assert_eq!(judge("a.csv", b"a,b\n1,2\n"), Ok(("csv", false)));
        assert_eq!(judge("a.xls", b"a\tb\n1\t2\n"), Ok(("tsv", true)));
        assert_eq!(judge("a.csv", &[0x50, 0x4B, 0x03, 0x04, 0, 0]), Err("damaged"));
        assert_eq!(judge("a.xlsx", b"just text"), Err("damaged"));
        let mut cfb = CFB.to_vec();
        cfb.extend("EncryptionInfo".encode_utf16().flat_map(|u| u.to_le_bytes()));
        assert_eq!(judge("a.xlsx", &cfb), Err("locked"));
        assert_eq!(judge("a.xls", &CFB), Ok(("xls", true)));
    }

    #[test]
    fn new_formats_by_their_bytes() {
        let Some(files) = real_files() else { return };
        let read = |n: &str| fs::read(files.join(n)).unwrap();
        assert_eq!(judge("a.xlsb", &read("excel-binary.xlsb")), Ok(("xlsb", true)));
        assert_eq!(judge("a.fods", &read("excel-saved.fods")), Ok(("fods", true)));
        assert_eq!(judge("a.dbf", &read("dbase-gbk.dbf")), Ok(("dbf", true)));
        for n in ["a.sqlite", "a.sqlite3", "a.db"] {
            assert_eq!(judge(n, &read("sqlite-shop.sqlite")), Ok(("sqlite", true)), "{n}");
        }
        // under another name they are what the name says, and that isn't what they are
        assert_eq!(judge("a.xlsx", &read("sqlite-shop.sqlite")), Err("damaged"));
        assert_eq!(judge("a.ods", &read("excel-binary.xlsb")), Err("damaged"));
        assert_eq!(judge("a.dbf", b"a,b\n1,2\n"), Err("damaged"));
        assert_eq!(judge("a.fods", b"<html></html>"), Err("damaged"));
        // Thumbs.db is a compound file, not a database
        assert_eq!(judge("Thumbs.db", &CFB), Err("damaged"));
        assert!(database_at(&files.join("sqlite-shop.sqlite")));
        assert!(!database_at(&files.join("dbase-gbk.dbf")));
    }
}
