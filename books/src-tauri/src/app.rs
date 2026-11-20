//! The Tauri side of DotAny Books: what makes this tool itself, and its commands. Everything else is dotany-base.
//!
//! A book is read here only far enough to know it may be opened: the format by its bytes, and a lock by the flags in its
//! header. Nothing is ever decrypted, and a locked book never reaches the page. The page then
//! gets the bytes and lays the book out itself (src/readers.js).
//!
//! Books keeps a file per window (`dotany_base::builder_per_window`): on the Mac one program shows each book in a window
//! of its own; on Windows each book is a program of its own, with one window. Where each book was left and its bookmarks
//! are the program's settings, one list for every book; each window's page keeps a copy of it and hears another
//! window's change as it is made (`set_setting`), so neither puts back what the other had before.

use dotany_base::atomic::{WriteFailure, atomic_write};
use dotany_base::{Base, FileWindows, PerWindow, Tool, file_name, lock};
use dotany_base::{RunFamily, commands};
use serde::Serialize;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};

fn any_bytes(_: &Path) -> bool {
    true
}

/// The window's book is closed, or the window went: nothing of it, or of one on its way, is kept.
fn forget(app: &AppHandle, label: &str) {
    let docs = app.state::<Docs>();
    if app.get_webview_window(label).is_none() {
        let gone = docs.0.remove(label);
        drop(gone);
        return;
    }
    if let Some(doc) = docs.0.get(label) {
        *lock(&doc.open) = None;
        *lock(&doc.pending) = None;
    }
}

/// Books keeps a book per window and says so in `FileWindows` (`forget`); this is never called.
fn nothing(_: &AppHandle) {}

pub static TOOL: Tool = Tool {
    name: "DotAny Books",
    folder: "Books",
    data_env: "DOTANY_BOOKS_DATA",
    user_dir: "Documents",
    opens_files: true,
    // txt opens here on purpose, though double-clicking one belongs to DotAny Text. cbt, cb7 and fbz are the other ways
    // comics and FB2 come packed; a book.fb2.zip is opened too (kind_of), though .zip itself belongs to DotAny Archive
    extensions: &["epub", "fb2", "cbz", "djvu", "djv", "mobi", "prc", "txt", "cbt", "cb7", "fbz"],
    sniff: any_bytes,
    size: (1180.0, 800.0),
    min_size: (480.0, 420.0),
    on_close: nothing,
};

/// The book on a window's screen, and the one being opened: it becomes the open one only once the page has read it.
#[derive(Default)]
struct Doc {
    open: Mutex<Option<Arc<Vec<u8>>>>,
    pending: Mutex<Option<Pending>>,
}

/// Each window's book; another window of the program (the Mac) has its own.
#[derive(Default)]
struct Docs(PerWindow<Doc>);

impl Docs {
    fn of(&self, window: &WebviewWindow) -> Arc<Doc> {
        self.0.of(window.label())
    }
}

/// A book on its way to the page: its file's bytes (what Save As copies), and what the page reads when that isn't the
/// same (a CBT's or CB7's pages, as a plain ZIP: comics.rs).
struct Pending {
    path: PathBuf,
    bytes: Arc<Vec<u8>>,
    view: Option<Vec<u8>>,
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
enum Opened {
    Ok {
        path: String,
        dir: String,
        name: String,
        size: u64,
        kind: &'static str,
    },
    Refused,
    Missing,
    Damaged,
    /// Locked by DRM: refused, never opened and never decrypted.
    Drm,
}

/// What a book's bytes say about it: its format, whether it may be opened, and whether it's whole enough to try.
#[derive(Debug, PartialEq)]
pub enum Verdict {
    Book(&'static str),
    Locked,
    Damaged,
}

fn kind_of(name: &str) -> Option<&'static str> {
    if name.to_ascii_lowercase().ends_with(".fb2.zip") {
        return Some("fb2zip");
    }
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase())?;
    Some(match ext.as_str() {
        "epub" => "epub",
        "fb2" => "fb2",
        "cbz" => "cbz",
        "cbt" => "cbt",
        "cb7" => "cb7",
        "fbz" => "fbz",
        "djvu" | "djv" => "djvu",
        "mobi" | "prc" => "mobi",
        "txt" => "txt",
        _ => return None,
    })
}

fn zip_of(bytes: &[u8]) -> Option<zip::ZipArchive<Cursor<&[u8]>>> {
    zip::ZipArchive::new(Cursor::new(bytes)).ok()
}

/// EPUB: a lock is `META-INF/rights.xml`, a Readium LCP licence, or anything in `encryption.xml` beyond fonts.
/// Obfuscated fonts are allowed, so those books open as usual.
fn epub(bytes: &[u8]) -> Verdict {
    let Some(mut zip) = zip_of(bytes) else { return Verdict::Damaged };
    let has = |zip: &zip::ZipArchive<Cursor<&[u8]>>, name: &str| zip.file_names().any(|n| n.eq_ignore_ascii_case(name));
    if has(&zip, "META-INF/rights.xml") || has(&zip, "META-INF/license.lcpl") {
        return Verdict::Locked;
    }
    if !has(&zip, "META-INF/container.xml") {
        return Verdict::Damaged;
    }
    if has(&zip, "META-INF/encryption.xml") {
        // a list of encrypted files is a few kilobytes; one that inflates past a megabyte is not a real one
        let read = zip
            .by_name("META-INF/encryption.xml")
            .ok()
            .and_then(|mut f| dotany_base::limits::read_up_to(&mut f, ENCRYPTION_XML_LIMIT).ok());
        let Some(text) = read.and_then(|b| String::from_utf8(b).ok()) else {
            return Verdict::Damaged;
        };
        if !only_fonts_obfuscated(&text) {
            return Verdict::Locked;
        }
    }
    Verdict::Book("epub")
}

const ENCRYPTION_XML_LIMIT: u64 = 1 << 20;

/// Every `Algorithm="…"` in encryption.xml is one of the two font obfuscations (IDPF's and Adobe's). The file is the
/// book's own: the value may be unquoted or in any script, so it is cut on characters, never on bytes.
pub fn only_fonts_obfuscated(xml: &str) -> bool {
    let mut rest = xml;
    while let Some(at) = rest.find("Algorithm=") {
        rest = &rest[at + 10..];
        let value = match rest.chars().next() {
            Some(q @ ('"' | '\'')) => rest[q.len_utf8()..].split(q).next().unwrap_or(""),
            // unquoted: up to a space or the end of the tag
            _ => rest.split(|c: char| c.is_whitespace() || c == '>').next().unwrap_or(""),
        };
        if !(value.contains("obfuscation") || value.contains("ns.adobe.com/pdf/enc#RC")) {
            return false;
        }
        rest = &rest[rest.chars().next().map_or(0, char::len_utf8)..];
    }
    true
}

/// MOBI: a PalmDB whose first record carries the encryption type at byte 12. Anything but 0 is a lock.
pub fn mobi(bytes: &[u8]) -> Verdict {
    if bytes.len() < 86 || !(&bytes[60..68] == b"BOOKMOBI" || &bytes[60..68] == b"TEXtREAd") {
        return Verdict::Damaged;
    }
    let first = u32::from_be_bytes([bytes[78], bytes[79], bytes[80], bytes[81]]) as usize;
    if first + 16 > bytes.len() {
        return Verdict::Damaged;
    }
    let lock = u16::from_be_bytes([bytes[first + 12], bytes[first + 13]]);
    if lock != 0 {
        return Verdict::Locked;
    }
    Verdict::Book("mobi")
}

/// CBZ: pictures in a ZIP. A password-protected archive can't be read, so it counts as damaged.
fn cbz(bytes: &[u8]) -> Verdict {
    let Some(mut zip) = zip_of(bytes) else { return Verdict::Damaged };
    for i in 0..zip.len() {
        match zip.by_index_raw(i) {
            Ok(f) if f.encrypted() => return Verdict::Damaged,
            Ok(_) => {}
            Err(_) => return Verdict::Damaged,
        }
    }
    Verdict::Book("cbz")
}

/// An FB2 inside a ZIP (.fbz, .fb2.zip, or a .fb2 that is really zipped): the page reads the .fb2 in it.
fn zipped_fb2(bytes: &[u8], kind: &'static str) -> Verdict {
    match zip_of(bytes) {
        Some(zip) if zip.file_names().any(|n| n.to_ascii_lowercase().ends_with(".fb2")) => Verdict::Book(kind),
        _ => Verdict::Damaged,
    }
}

/// FB2: one XML file, or the same inside a ZIP.
fn fb2(bytes: &[u8]) -> Verdict {
    if bytes.starts_with(b"PK") {
        return zipped_fb2(bytes, "fb2");
    }
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(4096)]);
    if head.contains("<FictionBook") || head.trim_start_matches('\u{feff}').trim_start().starts_with("<?xml") {
        Verdict::Book("fb2")
    } else {
        Verdict::Damaged
    }
}

pub fn judge(kind: &'static str, bytes: &[u8]) -> Verdict {
    match kind {
        "epub" => epub(bytes),
        "mobi" => mobi(bytes),
        "cbz" => cbz(bytes),
        "fb2" => fb2(bytes),
        "fbz" | "fb2zip" => zipped_fb2(bytes, kind),
        // the pictures are taken out when it opens (open_file); here only the start is looked at
        "cbt" if bytes.len() >= 512 => Verdict::Book("cbt"),
        "cbt" => Verdict::Damaged,
        "cb7" if bytes.starts_with(b"7z\xbc\xaf\x27\x1c") => Verdict::Book("cb7"),
        "cb7" => Verdict::Damaged,
        "djvu" if bytes.starts_with(b"AT&TFORM") => Verdict::Book("djvu"),
        "djvu" => Verdict::Damaged,
        _ => Verdict::Book(kind),
    }
}

#[tauri::command(async)]
fn open_file(window: WebviewWindow, docs: State<Docs>, path: String) -> Opened {
    let path = std::path::absolute(&path).unwrap_or_else(|_| PathBuf::from(&path));
    if !path.is_file() {
        return Opened::Missing;
    }
    let name = file_name(&path);
    let Some(kind) = kind_of(&name).filter(|k| *k == "fb2zip" || TOOL.allowed_name(&name)) else {
        return Opened::Refused;
    };
    let Ok(bytes) = fs::read(&path) else { return Opened::Damaged };
    let kind = match judge(kind, &bytes) {
        Verdict::Book(k) => k,
        Verdict::Locked => return Opened::Drm,
        Verdict::Damaged => return Opened::Damaged,
    };
    // a CBT's or CB7's pages, unpacked here and read by the page as a ZIP; one with none, or too many, isn't opened
    let view = match kind {
        "cbt" => crate::comics::tar_pages(&bytes).ok(),
        "cb7" => crate::comics::sevenz_pages(&bytes).ok(),
        _ => None,
    };
    if matches!(kind, "cbt" | "cb7") && view.is_none() {
        return Opened::Damaged;
    }
    let size = bytes.len() as u64;
    *lock(&docs.of(&window).pending) = Some(Pending { path: path.clone(), bytes: Arc::new(bytes), view });
    Opened::Ok {
        dir: path.parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default(),
        name,
        path: path.to_string_lossy().into_owned(),
        size,
        kind,
    }
}

/// The book being opened in this window, as bytes, for its page to read.
#[tauri::command(async)]
fn book_bytes(window: WebviewWindow, docs: State<Docs>) -> tauri::ipc::Response {
    let doc = docs.of(&window);
    let bytes = lock(&doc.pending)
        .as_ref()
        .map(|p| p.view.clone().unwrap_or_else(|| p.bytes.as_ref().clone()))
        .unwrap_or_default();
    tauri::ipc::Response::new(bytes)
}

/// The page has read it: it's the window's open book now, watched and on the recent list.
#[tauri::command(async)]
fn book_opened(window: WebviewWindow, docs: State<Docs>, path: String) {
    let doc = docs.of(&window);
    let mut pending = lock(&doc.pending);
    if let Some(p) = pending.take() {
        if p.path.to_string_lossy() == path {
            *lock(&doc.open) = Some(p.bytes);
            dotany_base::opened_in(window.app_handle(), window.label(), &p.path);
        }
    }
}

/// The page couldn't read it: nothing changes.
#[tauri::command(async)]
fn book_failed(window: WebviewWindow, docs: State<Docs>) {
    *lock(&docs.of(&window).pending) = None;
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
enum Saved {
    Saved { path: String, name: String, size: u64 },
    Nothing,
    Failed { kind: WriteFailure, detail: String },
}

/// The copy's size, exactly: a book is saved as it is.
#[tauri::command(async)]
fn estimate_save(window: WebviewWindow, docs: State<Docs>) -> Option<u64> {
    lock(&docs.of(&window).open).as_ref().map(|b| b.len() as u64)
}

#[tauri::command(async)]
fn save_file(window: WebviewWindow, docs: State<Docs>, path: String) -> Saved {
    let Some(bytes) = lock(&docs.of(&window).open).clone() else { return Saved::Nothing };
    let target = PathBuf::from(&path);
    if let Err(e) = atomic_write(&target, &bytes) {
        return Saved::Failed { kind: e.kind, detail: e.detail };
    }
    dotany_base::saved_in(window.app_handle(), window.label(), &target);
    Saved::Saved { name: file_name(&target), path, size: bytes.len() as u64 }
}

/// The family's, and every window of the program hears it at once with the window it came from: a page keeps its own
/// copy of where each book was left and of the bookmarks, and takes another window's change into it (src/main.js).
#[tauri::command(async)]
fn set_setting(window: WebviewWindow, key: String, value: Option<String>) {
    let app = window.app_handle();
    commands::set_setting(app.state::<Base>(), key.clone(), value.clone());
    let _ = app.emit("setting", serde_json::json!({ "from": window.label(), "key": key, "value": value }));
}

pub fn run() {
    dotany_base::builder_per_window(&TOOL, FileWindows { forget, extra: dotany_base::no_extra })
        .manage(Docs::default())
        // the family's commands, but Books answers `set_setting` itself (dotany_base::handler! would list the family's)
        .invoke_handler(tauri::generate_handler![
            set_setting,
            commands::forget_recent,
            commands::can_open,
            commands::pick_open,
            commands::pick_save,
            commands::default_folder,
            commands::neighbour,
            commands::close_document,
            commands::keep_mine,
            commands::set_frame,
            commands::window_control,
            commands::window_menu,
            commands::set_snap_button,
            commands::set_title,
            commands::window_ready,
            commands::startup_done,
            commands::close_window,
            commands::print_window,
            commands::set_menu_words,
            commands::set_edited,
            open_file,
            book_bytes,
            book_opened,
            book_failed,
            estimate_save,
            save_file
        ])
        .run_family(tauri::generate_context!())
        .expect("error while running DotAny Books");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn palm(lock: u16) -> Vec<u8> {
        let mut b = vec![0u8; 78];
        b[60..68].copy_from_slice(b"BOOKMOBI");
        b[76..78].copy_from_slice(&1u16.to_be_bytes());
        b.extend_from_slice(&88u32.to_be_bytes());
        b.extend_from_slice(&[0, 0, 0, 0, 0, 0]);
        let mut r0 = vec![0u8; 32];
        r0[12..14].copy_from_slice(&lock.to_be_bytes());
        b.extend_from_slice(&r0);
        b
    }

    #[test]
    fn mobi_lock_is_read_from_the_header() {
        assert_eq!(mobi(&palm(0)), Verdict::Book("mobi"));
        assert_eq!(mobi(&palm(2)), Verdict::Locked);
        assert_eq!(mobi(b"not a book"), Verdict::Damaged);
    }

    #[test]
    fn font_obfuscation_is_not_a_lock() {
        assert!(only_fonts_obfuscated(r#"<EncryptionMethod Algorithm="http://www.idpf.org/2008/obfuscation"/>"#));
        assert!(only_fonts_obfuscated(r#"<EncryptionMethod Algorithm='http://ns.adobe.com/pdf/enc#RC'/>"#));
        assert!(!only_fonts_obfuscated(
            r#"<EncryptionMethod Algorithm="http://www.w3.org/2001/04/xmlenc#aes128-cbc"/>"#
        ));
    }

    #[test]
    fn an_algorithm_in_any_script_or_unquoted_is_read_not_a_panic() {
        // the first character after `Algorithm=` is three bytes long: cutting at byte 1 would panic
        assert!(!only_fonts_obfuscated("<EncryptionMethod Algorithm=中文加密/>"));
        assert!(!only_fonts_obfuscated("<EncryptionMethod Algorithm=“中文”/>"));
        assert!(!only_fonts_obfuscated("<EncryptionMethod Algorithm=\"中文\"/>"));
        assert!(only_fonts_obfuscated("<EncryptionMethod Algorithm=http://www.idpf.org/2008/obfuscation/>"));
        assert!(!only_fonts_obfuscated("<EncryptionMethod Algorithm="));
        assert!(!only_fonts_obfuscated("<EncryptionMethod Algorithm=é"));
    }

    fn epub_with_encryption(xml: &[u8]) -> Vec<u8> {
        use std::io::Write;
        let mut z = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let o = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        z.start_file("META-INF/container.xml", o).unwrap();
        z.write_all(b"<container/>").unwrap();
        z.start_file("META-INF/encryption.xml", o).unwrap();
        z.write_all(xml).unwrap();
        z.finish().unwrap().into_inner()
    }

    #[test]
    fn an_encryption_list_that_inflates_past_a_megabyte_is_not_read_whole() {
        // 64 MB of spaces, a few dozen kilobytes in the file
        let book = epub_with_encryption(&vec![b' '; 64 << 20]);
        assert!(book.len() < 1 << 20);
        assert!(matches!(epub(&book), Verdict::Damaged));
        // the ordinary kinds still read
        let fonts = epub_with_encryption(
            br#"<encryption><EncryptionMethod Algorithm="http://www.idpf.org/2008/obfuscation"/></encryption>"#,
        );
        assert!(matches!(epub(&fonts), Verdict::Book("epub")));
        let chinese = epub_with_encryption("<encryption><EncryptionMethod Algorithm=中文/></encryption>".as_bytes());
        assert!(matches!(epub(&chinese), Verdict::Locked));
    }
}
