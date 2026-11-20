//! The Tauri side of DotAny Archive: what makes this tool itself, and its commands. Everything else is dotany-base.
//! The page keeps the archive's contents as a list of paths and does the editing (add, delete, rename, undo); this side
//! lists archives, reads entries out, writes new archives and extracts, always streaming from the archive on disk.
//!
//! Archive keeps a file per window (`dotany_base::builder_per_window`): on the Mac one program shows each archive in a
//! window of its own; on Windows each archive is a program of its own, with one window. Each window has its own archive
//! and its own job (an extraction, a save, a check): a long job in one window keeps running and reports its progress to
//! that window only while the person works in another, and stops when its window closes.

use crate::extract::{self, ExtractError};
use crate::pack::{self, Format, Level, Node, PackError};
use crate::shellmenu;
use crate::source::{self, Entry, EntryError, Kind, OpenError, Source};
use dotany_base::RunFamily;
use dotany_base::atomic::{WriteError, WriteFailure, atomic_produce};
use dotany_base::{FileWindows, PerWindow, Tool, file_name, lock};
use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};
use tauri::{AppHandle, Emitter, Manager, State, WebviewWindow};

fn archive_bytes(path: &Path) -> bool {
    source::sniff_kind(path).is_some()
}

/// The window's archive closed, or the window went: a job still running in it stops (the page has already asked it to
/// when it closes its file; a window that goes asks it here), and what it held goes. Nothing waits for the job: it holds
/// its own share of what it works on, and finds it stopped.
fn forget(app: &AppHandle, label: &str) {
    if let Some(doc) = app.state::<Docs>().remove(label) {
        doc.stop.store(true, Ordering::Relaxed);
    }
}

/// Archive keeps a file per window and says so in `FileWindows` (`forget`); this is never called.
fn nothing(_: &AppHandle) {}

/// Nothing from disk is open in this window any more (a new archive): this waits for a job still holding the archive.
fn put_down(doc: &Doc) {
    *lock(&doc.source) = None;
}

pub static TOOL: Tool = Tool {
    name: "DotAny Archive",
    folder: "Archive",
    data_env: "DOTANY_ARCHIVE_DATA",
    user_dir: "Documents",
    opens_files: true,
    extensions: &[
        "zip", "7z", "rar", "tar", "gz", "tgz", "bz2", "tbz2", "tbz", "xz", "txz", "zst", "tzst", "lzh", "lha", "cpio",
        "ar", "deb", "rpm",
    ],
    sniff: archive_bytes,
    size: (1180.0, 800.0),
    min_size: (480.0, 420.0),
    on_close: nothing,
};

/// A window's open archive, and the one job (extract, save, check) that may be running on it. Another window of the
/// program (the Mac) has its own; nothing here is seen by it.
#[derive(Default)]
struct Doc {
    source: Mutex<Option<Source>>,
    stop: Arc<AtomicBool>,
}

/// Each window's `Doc`, by its label.
type Docs = PerWindow<Doc>;

/// `--pack <path>` from the right-click menu: that path is to be put in a new archive, even if it is an archive. Per
/// program: it comes on the command line, so only ever for the first window (Windows' right-click menu starts a program
/// of its own for it).
static PACK: Mutex<Option<PathBuf>> = Mutex::new(None);

fn kind_name(k: Kind) -> &'static str {
    k.name()
}

#[derive(Serialize)]
pub struct Item {
    /// where it is on disk
    pub from: String,
    /// where it goes in the archive, from the dropped item's own name down
    pub rel: String,
    pub dir: bool,
    pub size: u64,
    pub u: Option<i64>,
}

fn unix(t: std::io::Result<SystemTime>) -> Option<i64> {
    t.ok()?.duration_since(SystemTime::UNIX_EPOCH).ok().map(|d| d.as_secs() as i64)
}

/// Files and folders from disk, folders walked all the way down, links never followed.
pub(crate) fn walk_items(paths: &[String]) -> Vec<Item> {
    let mut out = Vec::new();
    fn visit(p: &Path, rel: String, out: &mut Vec<Item>) {
        let Ok(m) = std::fs::symlink_metadata(p) else { return };
        if m.file_type().is_symlink() {
            return;
        }
        if m.is_dir() {
            out.push(Item {
                from: p.to_string_lossy().into_owned(),
                rel: rel.clone(),
                dir: true,
                size: 0,
                u: unix(m.modified()),
            });
            let Ok(list) = std::fs::read_dir(p) else { return };
            let mut list: Vec<_> = list.flatten().collect();
            list.sort_by_key(|e| e.file_name());
            // the Mac's .DS_Store and ._ files are the Finder's, not the person's: never packed
            #[cfg(target_os = "macos")]
            list.retain(|e| !crate::platform::mac_litter(&e.file_name().to_string_lossy()));
            for e in list {
                visit(&e.path(), format!("{rel}/{}", e.file_name().to_string_lossy()), out);
            }
        } else if m.is_file() {
            out.push(Item {
                from: p.to_string_lossy().into_owned(),
                rel,
                dir: false,
                size: m.len(),
                u: unix(m.modified()),
            });
        }
    }
    for p in paths {
        let path = PathBuf::from(p);
        visit(&path, file_name(&path), &mut out);
    }
    out
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
        entries: Vec<Entry>,
    },
    /// not an archive (or `--pack`): a new archive holding it
    Fresh {
        dir: String,
        name: String,
        items: Vec<Item>,
    },
    Locked,
    Wrong,
    Missing,
    Damaged,
}

/// The Mac: an archive as a Mac person expects to see it. A zip made on a Mac carries `__MACOSX/` (its files' extended
/// attributes): never listed, so never extracted or written again. Names are composed (NFC), as the Mac's own programs
/// show and write them; libarchive gives RAR's decomposed there. Elsewhere the archive is as it is.
pub(crate) fn tidy(src: &mut Source) {
    #[cfg(target_os = "macos")]
    {
        src.entries.retain(|e| !e.n.as_deref().is_some_and(crate::platform::mac_attributes));
        for e in &mut src.entries {
            if let Some(n) = e.n.as_mut() {
                *n = crate::platform::nfc(n);
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = src;
}

#[tauri::command(async)]
fn open_file(window: WebviewWindow, docs: State<Docs>, path: String, password: Option<String>) -> Opened {
    let (app, label) = (window.app_handle(), window.label());
    let path = std::path::absolute(&path).unwrap_or_else(|_| PathBuf::from(&path));
    let pack = {
        let mut p = lock(&PACK);
        if p.as_deref().is_some_and(|q| dotany_base::same_file(q, &path)) { p.take().is_some() } else { false }
    };
    // the Mac's Quick Action Compress with DotAny Archive hands over a request naming the item: it goes into a new
    // archive
    #[cfg(target_os = "macos")]
    let (path, pack) = match crate::shellmenu::pack_request(&path) {
        Some(item) => (item, true),
        None => (path, pack),
    };
    if !path.exists() {
        return Opened::Missing;
    }
    if pack || path.is_dir() || source::sniff_kind(&path).is_none() {
        // a folder, a file that isn't an archive, or "compress" from the right-click menu: a new archive named after it
        if !path.is_dir() && !pack && TOOL.allowed_name(&file_name(&path)) {
            return Opened::Damaged;
        }
        let stem = if path.is_dir() {
            file_name(&path)
        } else {
            path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default()
        };
        if let Some(doc) = docs.get(label) {
            put_down(&doc);
        }
        return Opened::Fresh {
            dir: path.parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default(),
            name: format!("{stem}.zip"),
            items: walk_items(&[path.to_string_lossy().into_owned()]),
        };
    }
    let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
    match Source::open(&path, password.as_deref()) {
        Ok(mut src) => {
            tidy(&mut src);
            // the window went while its archive was being read: nothing keeps it
            if app.get_webview_window(label).is_none() {
                return Opened::Missing;
            }
            let entries = src.entries.clone();
            let kind = kind_name(src.kind);
            *lock(&docs.of(label).source) = Some(src);
            dotany_base::opened_in(app, label, &path);
            Opened::Ok {
                dir: path.parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default(),
                name: file_name(&path),
                path: path.to_string_lossy().into_owned(),
                size,
                kind,
                entries,
            }
        }
        Err(OpenError::Locked) => Opened::Locked,
        Err(OpenError::WrongPassword) => Opened::Wrong,
        Err(OpenError::Damaged) => Opened::Damaged,
    }
}

/// A new, empty archive: nothing from disk is open any more.
#[tauri::command]
fn forget_source(window: WebviewWindow, docs: State<Docs>) {
    if let Some(doc) = docs.get(window.label()) {
        put_down(&doc);
    }
}

#[tauri::command(async)]
fn stat_paths(paths: Vec<String>) -> Vec<Item> {
    walk_items(&paths)
}

#[tauri::command(async)]
fn pick_files(window: WebviewWindow, start: Option<String>) -> Vec<String> {
    // the Mac: the family's open panel, as this window's sheet (and a test's answer in its place)
    #[cfg(target_os = "macos")]
    {
        let picked = mac_pick(&window, start, dotany_base::platform::Pick::Files);
        picked.into_iter().map(|p| p.to_string_lossy().into_owned()).collect()
    }
    #[cfg(not(target_os = "macos"))]
    {
        let mut d = rfd::FileDialog::new().set_parent(&window);
        if let Some(s) = start.filter(|s| Path::new(s).is_dir()) {
            d = d.set_directory(s);
        }
        d.pick_files().unwrap_or_default().into_iter().map(|p| p.to_string_lossy().into_owned()).collect()
    }
}

#[tauri::command(async)]
fn pick_folder(window: WebviewWindow, start: Option<String>) -> Option<String> {
    // the Mac: the family's open panel for a folder, as this window's sheet
    #[cfg(target_os = "macos")]
    {
        let picked = mac_pick(&window, start, dotany_base::platform::Pick::Folder);
        picked.into_iter().next().map(|p| p.to_string_lossy().into_owned())
    }
    #[cfg(not(target_os = "macos"))]
    {
        let mut d = rfd::FileDialog::new().set_parent(&window);
        if let Some(s) = start.filter(|s| Path::new(s).is_dir()) {
            d = d.set_directory(s);
        }
        d.pick_folder().map(|p| p.to_string_lossy().into_owned())
    }
}

/// Any kind of file (or a folder), starting where the page says.
#[cfg(target_os = "macos")]
fn mac_pick(window: &WebviewWindow, start: Option<String>, pick: dotany_base::platform::Pick) -> Vec<PathBuf> {
    dotany_base::platform::ask_open(window, &[], start.map(PathBuf::from), pick)
}

/// One entry's bytes, up to `limit`, for the preview panel.
#[tauri::command(async)]
fn read_entry(
    window: WebviewWindow,
    docs: State<Docs>,
    i: usize,
    password: Option<String>,
    limit: u64,
) -> Result<tauri::ipc::Response, String> {
    let doc = docs.of(window.label());
    let mut guard = lock(&doc.source);
    let src = guard.as_mut().ok_or("closed")?;
    match src.read_one(i, password.as_deref(), limit) {
        Ok(v) => Ok(tauri::ipc::Response::new(v)),
        Err(EntryError::Password) => Err("password".into()),
        Err(EntryError::Unsupported) => Err("unsupported".into()),
        Err(EntryError::Damaged) => Err("damaged".into()),
    }
}

/// The first `limit` bytes of a file added from disk, for the preview panel.
#[tauri::command(async)]
fn read_local(path: String, limit: u64) -> Result<tauri::ipc::Response, String> {
    use std::io::Read;
    let f = std::fs::File::open(&path).map_err(|e| e.to_string())?;
    let mut v = Vec::new();
    f.take(limit).read_to_end(&mut v).map_err(|e| e.to_string())?;
    Ok(tauri::ipc::Response::new(v))
}

/// Is this the password? Reads one locked entry to its end, so its CRC answers too.
#[tauri::command(async)]
fn try_password(window: WebviewWindow, docs: State<Docs>, i: usize, password: String) -> &'static str {
    let doc = docs.of(window.label());
    let mut guard = lock(&doc.source);
    let Some(src) = guard.as_mut() else { return "damaged" };
    // read to its end without keeping it: the CRC answers, and a large entry costs no memory
    match src.copy_one(i, Some(&password), &mut std::io::sink()) {
        Ok(_) => "ok",
        Err(EntryError::Password) => "wrong",
        Err(EntryError::Unsupported) => "unsupported",
        Err(EntryError::Damaged) => "wrong",
    }
}

#[derive(Serialize, Clone)]
struct Progress {
    done: u64,
    total: u64,
    name: String,
}

/// Sends the progress line about twelve times a second while `job` runs, to the window whose job it is only: another
/// window of the program (the Mac) has its own job, or none.
fn with_progress<T>(window: &WebviewWindow, total: u64, job: impl FnOnce(&AtomicU64, &dyn Fn(&str)) -> T) -> T {
    let (app, label) = (window.app_handle(), window.label().to_string());
    let done = Arc::new(AtomicU64::new(0));
    let name = Arc::new(Mutex::new(String::new()));
    let running = Arc::new(AtomicBool::new(true));
    let ticker = {
        let (app, label, done, name, running) =
            (app.clone(), label.clone(), done.clone(), name.clone(), running.clone());
        std::thread::spawn(move || {
            while running.load(Ordering::Relaxed) {
                let _ = app.emit_to(
                    label.as_str(),
                    "progress",
                    Progress { done: done.load(Ordering::Relaxed), total, name: lock(&name).clone() },
                );
                std::thread::sleep(Duration::from_millis(80));
            }
        })
    };
    let now = |n: &str| *lock(&name) = n.to_string();
    let out = job(&done, &now);
    running.store(false, Ordering::Relaxed);
    let _ = ticker.join();
    let _ = app.emit_to(
        label.as_str(),
        "progress",
        Progress { done: done.load(Ordering::Relaxed), total, name: String::new() },
    );
    out
}

fn total_of(src: Option<&Source>, nodes: &[Node]) -> u64 {
    let sizes: std::collections::HashMap<usize, u64> =
        src.map(|s| s.entries.iter().map(|e| (e.i, e.s)).collect()).unwrap_or_default();
    nodes
        .iter()
        .filter(|n| !n.dir)
        .map(|n| match (n.i, n.add.as_deref()) {
            (Some(i), _) => sizes.get(&i).copied().unwrap_or(0),
            (None, Some(p)) => std::fs::metadata(p).map(|m| m.len()).unwrap_or(0),
            _ => 0,
        })
        .sum()
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
enum Extracted {
    Done(extract::Outcome),
    Cancelled,
    Failed { kind: WriteFailure, detail: String },
}

#[tauri::command(async)]
fn extract_to(
    window: WebviewWindow,
    docs: State<Docs>,
    nodes: Vec<Node>,
    location: String,
    folder: Option<String>,
    password: Option<String>,
) -> Extracted {
    let doc = docs.of(window.label());
    doc.stop.store(false, Ordering::Relaxed);
    let stop = doc.stop.clone();
    let mut guard = lock(&doc.source);
    let total = total_of(guard.as_ref(), &nodes);
    // the Mac keeps what was packed runnable, and makes a zip's symbolic links links again
    #[cfg(target_os = "macos")]
    let modes = guard.as_ref().map(|s| s.unix_modes()).unwrap_or_default();
    #[cfg(not(target_os = "macos"))]
    let modes = extract::Modes::new();
    let r = with_progress(&window, total, |done, now| {
        extract::extract(
            Path::new(&location),
            folder.as_deref(),
            guard.as_mut(),
            password.as_deref(),
            &nodes,
            &modes,
            done,
            now,
            &stop,
        )
    });
    match r {
        Ok(out) => Extracted::Done(out),
        Err(ExtractError::Cancelled) => Extracted::Cancelled,
        Err(ExtractError::Io(e)) => {
            let w = WriteError::from(e);
            Extracted::Failed { kind: w.kind, detail: w.detail }
        }
    }
}

/// Stops this window's job; another window's goes on.
#[tauri::command]
fn cancel_job(window: WebviewWindow, docs: State<Docs>) {
    if let Some(doc) = docs.get(window.label()) {
        doc.stop.store(true, Ordering::Relaxed);
    }
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "lowercase")]
enum Saved {
    Saved { path: String, name: String, size: u64, kind: &'static str, entries: Vec<Entry> },
    Failed { kind: String, detail: String },
}

fn fail(kind: &str, detail: impl ToString) -> Saved {
    Saved::Failed { kind: kind.to_string(), detail: detail.to_string() }
}

#[tauri::command(async)]
#[allow(clippy::too_many_arguments)]
fn save_archive(
    window: WebviewWindow,
    docs: State<Docs>,
    path: String,
    format: Format,
    level: Level,
    password: Option<String>,
    read_password: Option<String>,
    nodes: Vec<Node>,
) -> Saved {
    let target = PathBuf::from(&path);
    // RAR is read only: a save is always one of the four formats this program writes, never RAR
    if target.extension().is_some_and(|e| e.eq_ignore_ascii_case("rar")) {
        return fail("other", "rar is never written");
    }
    let (app, label) = (window.app_handle(), window.label());
    let doc = docs.of(label);
    doc.stop.store(false, Ordering::Relaxed);
    let stop = doc.stop.clone();
    let mut guard = lock(&doc.source);
    let total = total_of(guard.as_ref(), &nodes);
    let mut packed: Option<PackError> = None;
    let written = with_progress(&window, total, |done, now| {
        atomic_produce(&target, |temp| {
            pack::write_archive(
                temp,
                format,
                level,
                password.as_deref(),
                read_password.as_deref(),
                guard.as_mut(),
                &nodes,
                done,
                now,
                &stop,
            )
            .map_err(|e| {
                let w = match &e {
                    PackError::Io(io) => WriteError::from(std::io::Error::new(io.kind(), io.to_string())),
                    _ => WriteError::from("pack".to_string()),
                };
                packed = Some(e);
                w
            })
        })
    });
    if let Err(e) = written {
        return match packed {
            Some(PackError::Password) => fail("password", "password"),
            Some(PackError::Damaged(name)) => fail("damaged", name),
            Some(PackError::Cancelled) => fail("cancelled", ""),
            _ => Saved::Failed {
                kind: serde_json::to_value(&e.kind)
                    .ok()
                    .and_then(|v| v.as_str().map(str::to_string))
                    .unwrap_or_else(|| "other".into()),
                detail: e.detail,
            },
        };
    }
    dotany_base::saved_in(app, label, &target);
    // the window moves to the new archive: what's listed now comes from it
    match Source::open(&target, password.as_deref().filter(|p| !p.is_empty())) {
        Ok(mut src) => {
            tidy(&mut src);
            let entries = src.entries.clone();
            let kind = kind_name(src.kind);
            *guard = Some(src);
            drop(guard);
            // unless the window has put its archive down, or gone, while this was written: then it holds something else
            // now, or nothing
            if docs.get(label).is_some_and(|now| Arc::ptr_eq(&now, &doc)) {
                dotany_base::opened_in(app, label, &target);
            }
            Saved::Saved {
                name: file_name(&target),
                size: std::fs::metadata(&target).map(|m| m.len()).unwrap_or(0),
                path,
                kind,
                entries,
            }
        }
        Err(_) => fail("other", "written but not readable"),
    }
}

/// About how large the new archive will be. Entries keep roughly the share they have now; files from disk are
/// sampled and compressed for real.
#[tauri::command(async)]
fn estimate_save(window: WebviewWindow, docs: State<Docs>, format: Format, level: Level, nodes: Vec<Node>) -> u64 {
    use std::io::{Read, Write};
    let doc = docs.of(window.label());
    let guard = lock(&doc.source);
    let src = guard.as_ref();
    let by_i: std::collections::HashMap<usize, &Entry> =
        src.map(|s| s.entries.iter().map(|e| (e.i, e)).collect()).unwrap_or_default();
    let stored = src.is_some_and(|s| s.kind == Kind::Tar(None));
    let (mut raw, mut known, mut disk) = (0u64, 0f64, 0u64);
    let (mut sample_in, mut sample_out) = (0usize, 0usize);
    for n in nodes.iter().filter(|n| !n.dir) {
        if let Some(e) = n.i.and_then(|i| by_i.get(&i)) {
            raw += e.s;
            known += e.s as f64 * if stored || e.s == 0 { 0.6 } else { (e.c as f64 / e.s as f64).min(1.0) };
        } else if let Some(p) = n.add.as_deref() {
            let size = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
            raw += size;
            disk += size;
            // a sample of what was added, compressed for real
            if sample_in < 8 << 20 {
                if let Ok(f) = std::fs::File::open(p) {
                    let mut buf = Vec::new();
                    let _ = f.take(256 << 10).read_to_end(&mut buf);
                    let mut z = flate2::write::DeflateEncoder::new(Vec::new(), flate2::Compression::new(6));
                    let _ = z.write_all(&buf);
                    sample_out += z.finish().map(|v| v.len()).unwrap_or(buf.len());
                    sample_in += buf.len();
                }
            }
        }
    }
    let ratio = if sample_in > 0 { sample_out as f64 / sample_in as f64 } else { 0.6 };
    let compressed = (known + disk as f64 * ratio)
        * match level {
            Level::Fast => 1.08,
            Level::Std => 1.0,
            Level::Tight => 0.96,
        };
    let entries = nodes.len() as f64;
    let names: f64 = nodes.iter().map(|n| n.path.len() as f64).sum();
    let size = match format {
        Format::Zip => compressed + entries * 76.0 + names * 2.0 + 22.0,
        Format::SevenZ => compressed * 0.86 + entries * 24.0 + names * 2.0 + 64.0,
        Format::TarGz => compressed * 0.98 + entries * 40.0 + 20.0,
        Format::Tar => raw as f64 + entries * 512.0 + 1024.0,
    };
    size.round() as u64
}

#[derive(Serialize)]
struct Checked {
    /// entries whose data is broken or whose checksum doesn't match
    bad: Vec<usize>,
    /// encrypted entries that weren't checked: no password yet, or RAR's encryption
    locked: usize,
    /// the archive itself couldn't be read to its end
    broken: bool,
}

/// Every entry read to its end, so each one's checksum answers.
#[tauri::command(async)]
fn check_archive(window: WebviewWindow, docs: State<Docs>, password: Option<String>) -> Checked {
    let doc = docs.of(window.label());
    doc.stop.store(false, Ordering::Relaxed);
    let stop = doc.stop.clone();
    let mut guard = lock(&doc.source);
    let Some(src) = guard.as_mut() else { return Checked { bad: vec![], locked: 0, broken: true } };
    let files: HashSet<usize> = src.entries.iter().filter(|e| !e.d).map(|e| e.i).collect();
    let total: u64 = src.entries.iter().map(|e| e.s).sum();
    let mut out = Checked { bad: vec![], locked: 0, broken: false };
    let mut seen = HashSet::new();
    let walked = with_progress(&window, total, |done, _| {
        src.walk(password.as_deref(), &|i| files.contains(&i), &mut |i, data| {
            seen.insert(i);
            match data {
                Ok(r) => {
                    let mut counted = pack::Counted { inner: r, done, stop: &stop };
                    match std::io::copy(&mut counted, &mut std::io::sink()) {
                        Ok(_) => {}
                        Err(e) if e.kind() == std::io::ErrorKind::Interrupted => return Ok(false),
                        Err(_) => out.bad.push(i),
                    }
                }
                Err(EntryError::Password) | Err(EntryError::Unsupported) => out.locked += 1,
                Err(EntryError::Damaged) => out.bad.push(i),
            }
            Ok(true)
        })
    });
    if walked.is_err() {
        out.broken = true;
        // what the broken archive never reached counts as damaged too
        let mut rest: Vec<usize> = files.iter().filter(|i| !seen.contains(i)).copied().collect();
        rest.sort();
        out.bad.extend(rest);
    }
    out
}

/// What is never opened from inside an archive in the system's own program: on Windows, what runs, installs, links
/// elsewhere or plugs into the system when it is opened (programs, scripts, installers, shortcuts, help files that run
/// script, console snap-ins, Excel add-ins, search and library descriptions, troubleshooting packs, disk images that
/// mount), and Office documents whose kind is to carry macros.
const RUNNABLE: &[&str] = &[
    "exe",
    "com",
    "bat",
    "cmd",
    "ps1",
    "vbs",
    "vbe",
    "js",
    "jse",
    "wsf",
    "wsh",
    "msi",
    "msp",
    "scr",
    "pif",
    "lnk",
    "url",
    "hta",
    "cpl",
    "reg",
    "jar",
    "appx",
    "msix",
    "application",
    "inf",
    "sct",
    "psm1",
    "chm",
    "msc",
    "xll",
    "appref-ms",
    "settingcontent-ms",
    "library-ms",
    "search-ms",
    "diagcab",
    "iso",
    "img",
    "vhd",
    "vhdx",
    "docm",
    "xlsm",
    "pptm",
    "dotm",
    "xltm",
    "xlam",
    "ppam",
];

/// And what the Mac runs or installs when it is opened: Terminal's scripts, installers, Automator's and AppleScript's,
/// its link files, programs and the bundles that plug into the system.
#[cfg(target_os = "macos")]
const MAC_RUNNABLE: &[&str] = &[
    "command",
    "tool",
    "terminal",
    "pkg",
    "mpkg",
    "workflow",
    "action",
    "scpt",
    "scptd",
    "applescript",
    "webloc",
    "inetloc",
    "fileloc",
    "app",
    "prefpane",
    "saver",
    "osax",
    "kext",
    "plugin",
];

/// Is the file at `path` of a kind never opened from inside an archive? Judged on the name of the file that is opened,
/// as the system will read it.
fn never_opened(path: &Path) -> bool {
    let ext = path.extension().map(|e| e.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    #[cfg(target_os = "macos")]
    if MAC_RUNNABLE.contains(&ext.as_str()) {
        return true;
    }
    RUNNABLE.contains(&ext.as_str())
}

/// Opens one entry in the DotAny tool for its kind when it's installed beside this one, else in the system's own
/// program: written to a temporary folder first, under its last name only, with the archive's mark. `add` is a file
/// added from disk and not yet saved into the archive, opened where it is. What `never_opened` names is refused.
#[tauri::command(async)]
fn open_entry(
    window: WebviewWindow,
    docs: State<Docs>,
    i: Option<usize>,
    add: Option<String>,
    name: String,
    tool: Option<String>,
    password: Option<String>,
) -> &'static str {
    let sibling = tool.as_deref().and_then(dotany_base::platform::sibling_tool);
    let file = match (i, add) {
        (_, Some(p)) => {
            let p = PathBuf::from(p);
            if sibling.is_none() && never_opened(&p) {
                return "refused";
            }
            p
        }
        (Some(i), None) => {
            let Some(leaf) = extract::leaf_name(&name) else { return "damaged" };
            if sibling.is_none() && never_opened(Path::new(&leaf)) {
                return "refused";
            }
            let doc = docs.of(window.label());
            let mut guard = lock(&doc.source);
            let Some(src) = guard.as_mut() else { return "damaged" };
            let n = std::process::id() as u64 * 1000
                + (SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)
                    .map(|d| d.subsec_micros() as u64)
                    .unwrap_or(0)
                    % 1000);
            let dir = std::env::temp_dir().join("DotAny Archive").join(n.to_string());
            let file = dir.join(&leaf);
            if file.parent() != Some(dir.as_path()) || std::fs::create_dir_all(&dir).is_err() {
                return "damaged";
            }
            if let Err(e) = extract::entry_to_file(src, i, password.as_deref(), &file) {
                let _ = std::fs::remove_dir_all(&dir);
                return match e {
                    EntryError::Password => "password",
                    EntryError::Unsupported => "unsupported",
                    EntryError::Damaged => "damaged",
                };
            }
            file
        }
        _ => return "damaged",
    };
    let ok = match sibling {
        Some(exe) => dotany_base::platform::launch(&exe, &file),
        None => crate::platform::open_with_default(&file),
    };
    if ok { "ok" } else { "refused" }
}

/// The DotAny tools installed beside this one, by name.
#[tauri::command(async)]
fn siblings() -> Vec<&'static str> {
    [
        "dotany-image",
        "dotany-text",
        "dotany-video",
        "dotany-audio",
        "dotany-pdf",
        "dotany-books",
        "dotany-sheets",
        "dotany-slides",
        "dotany-3d",
    ]
    .into_iter()
    .filter(|n| dotany_base::platform::sibling_tool(n).is_some())
    .collect()
}

#[tauri::command(async)]
fn reveal(path: String) -> bool {
    crate::platform::show_folder(Path::new(&path))
}

#[tauri::command(async)]
fn shell_menu() -> bool {
    std::env::current_exe().map(|e| shellmenu::registered(&e)).unwrap_or(false)
}

#[tauri::command(async)]
fn set_shell_menu(on: bool, open_label: String, pack_label: String) -> bool {
    let Ok(exe) = std::env::current_exe() else { return false };
    if on { shellmenu::register(&exe, &open_label, &pack_label) } else { shellmenu::unregister() }
}

pub fn run() {
    // "compress" from the right-click menu: the path after --pack goes into a new archive
    let args: Vec<String> = std::env::args().collect();
    if let Some(at) = args.iter().position(|a| a == "--pack") {
        if let Some(p) = args.get(at + 1) {
            let p = PathBuf::from(p);
            *lock(&PACK) = Some(std::path::absolute(&p).unwrap_or(p));
        }
    }
    dotany_base::builder_per_window(&TOOL, FileWindows { forget, extra: dotany_base::no_extra })
        .manage(Docs::default())
        .invoke_handler(dotany_base::handler![
            open_file,
            forget_source,
            stat_paths,
            pick_files,
            pick_folder,
            read_entry,
            read_local,
            try_password,
            extract_to,
            cancel_job,
            save_archive,
            estimate_save,
            check_archive,
            open_entry,
            siblings,
            reveal,
            shell_menu,
            set_shell_menu
        ])
        .run_family(tauri::generate_context!())
        .expect("error while running DotAny Archive");
}

#[cfg(test)]
mod tests {
    use super::never_opened;
    use crate::extract::leaf_name;
    use std::path::Path;

    #[test]
    fn programs_links_disk_images_and_macro_documents_are_never_opened() {
        for name in [
            "setup.exe",
            "run.BAT",
            "x.lnk",
            "help.chm",
            "console.msc",
            "addin.xll",
            "app.appref-ms",
            "s.settingcontent-ms",
            "l.library-ms",
            "q.search-ms",
            "fix.diagcab",
            "disk.iso",
            "disk.img",
            "disk.vhd",
            "disk.VHDX",
            "report.docm",
            "book.xlsm",
            "deck.pptm",
            "t.dotm",
            "t.xltm",
            "a.xlam",
            "a.ppam",
        ] {
            assert!(never_opened(Path::new(name)), "{name}");
        }
        for name in ["report.docx", "book.xlsx", "deck.pptx", "photo.png", "notes.txt", "README", "a.pdf"] {
            assert!(!never_opened(Path::new(name)), "{name}");
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_macs_runnable_kinds_are_never_opened_there() {
        for name in ["run.command", "setup.pkg", "x.app", "s.scpt", "link.webloc"] {
            assert!(never_opened(Path::new(name)), "{name}");
        }
    }

    /// The kind judged is the kind of the file written, whatever the name the page sends: Windows drops trailing dots
    /// and spaces, and only the last part of a name is used.
    #[test]
    fn the_kind_judged_is_the_kind_of_the_file_written() {
        for name in ["x.exe.", "x.exe ", "x.exe . .", "a/b/x.exe", r"..\..\x.cmd", "notes.txt/../x.lnk"] {
            let leaf = leaf_name(name).unwrap();
            assert!(never_opened(Path::new(&leaf)), "{name} is written as {leaf}");
        }
    }
}
