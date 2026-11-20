//! Installing in two steps, so cancelling never leaves anything behind and an upgrade never leaves a half-new install:
//!
//! 1. **Stage:** unpack the chosen files into `<root>\.dotany-staging`, checking each one's hash. Nothing else is
//!    touched yet. Cancel or a failure removes the folder, and the install folder too if this install made it.
//! 2. **Commit:** move what's there aside into `<root>\.dotany-old`, move the new files in, write the registry, the
//!    Start menu and `install.json`, then drop the old files. A failure here puts the old files back.
//!
//! A running program can be renamed but not deleted, so an open tool doesn't stop an upgrade: its old file waits in
//! `.dotany-old` until the next install or the uninstaller clears it. The hub has no unsaved work, so it is ended and,
//! after an upgrade, started again.

use crate::manifest::{Component, InstalledComponent, InstalledFile, Record, RegItem, Release, SCHEMA, mb};
use crate::payload::Payload;
use crate::reg::{self, Facts, Hive, Op};
use crate::system;
use serde::Serialize;
use std::collections::HashSet;
use std::fs;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

pub const STAGING: &str = ".dotany-staging";
pub const OLD: &str = ".dotany-old";
pub const RECORD: &str = "install.json";
pub const UNINSTALLER: &str = "uninstall.exe";
pub const HUB: &str = "dotany.exe";

/// Set by the update mode (the hub's Install and Update): after the commit DotAny opens with its window, whether or not
/// it was running. Otherwise a hub that was running is started again as it was, quietly.
pub static OPEN_HUB_AFTER: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum Failure {
    Cancelled,
    /// Not enough room: what's needed and what's free, in bytes.
    Space {
        need: u64,
        free: u64,
    },
    /// Windows wouldn't let us write there.
    Denied,
    /// These programs are open and have to be closed first.
    Busy {
        names: Vec<String>,
    },
    Other {
        detail: String,
    },
}

impl From<io::Error> for Failure {
    fn from(e: io::Error) -> Failure {
        match (e.kind(), e.raw_os_error()) {
            (io::ErrorKind::Interrupted, _) => Failure::Cancelled,
            (io::ErrorKind::PermissionDenied, _) | (_, Some(5)) => Failure::Denied,
            // ERROR_DISK_FULL, ERROR_HANDLE_DISK_FULL
            (_, Some(112)) | (_, Some(39)) => Failure::Space { need: 0, free: 0 },
            _ => Failure::Other { detail: e.to_string() },
        }
    }
}

/// What the person chose: where, and which tools (the hub comes anyway).
#[derive(Clone, Debug)]
pub struct Choice {
    pub root: PathBuf,
    pub programs: Vec<String>,
}

pub struct Staged {
    pub root: PathBuf,
    pub dir: PathBuf,
    /// Folders this install created for the root, outermost first; removed again if the install doesn't go ahead.
    pub made: Vec<PathBuf>,
    pub files: Vec<InstalledFile>,
}

/// How many bytes the choice puts on the disk.
pub fn need_bytes(payload: &Payload, programs: &[String]) -> u64 {
    let release = &payload.release;
    release.files_of(&release.resolve(programs)).iter().map(|f| f.size).sum::<u64>() + payload.program_len
}

fn make_dirs(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut missing = Vec::new();
    let mut at = Some(root);
    while let Some(p) = at {
        if p.exists() {
            break;
        }
        missing.push(p.to_path_buf());
        at = p.parent();
    }
    missing.reverse();
    fs::create_dir_all(root)?;
    Ok(missing)
}

/// Removes the staging folder and any folder this install made, leaving the computer as it was.
pub fn discard(root: &Path, made: &[PathBuf]) {
    let _ = fs::remove_dir_all(root.join(STAGING));
    for dir in made.iter().rev() {
        let _ = fs::remove_dir(dir);
    }
}

fn test_delay(cancel: &AtomicBool) {
    // tests slow staging down to cancel in the middle of it
    let ms: u64 = std::env::var("DOTANY_SETUP_TEST_DELAY_MS").ok().and_then(|v| v.parse().ok()).unwrap_or(0);
    let until = std::time::Instant::now() + std::time::Duration::from_millis(ms);
    while std::time::Instant::now() < until && !cancel.load(Ordering::Relaxed) {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}

/// Step 1. `done` hears each component's id once all its files are unpacked.
pub fn stage(
    payload: &Payload,
    choice: &Choice,
    cancel: &AtomicBool,
    done: impl FnMut(&str),
) -> Result<Staged, Failure> {
    stage_with(payload, choice, cancel, done, |_, _| {})
}

/// Step 1, saying how far it is as it goes: `bytes` hears how many of the files' bytes are unpacked, of how many, after
/// each file. The window's progress line follows it.
pub fn stage_with(
    payload: &Payload,
    choice: &Choice,
    cancel: &AtomicBool,
    mut done: impl FnMut(&str),
    mut bytes: impl FnMut(u64, u64),
) -> Result<Staged, Failure> {
    let release = &payload.release;
    let components = release.resolve(&choice.programs);
    let files = release.files_of(&components);
    let total: u64 = files.iter().map(|f| f.size).sum();
    let mut unpacked = 0u64;
    let need = need_bytes(payload, &choice.programs);
    if let Some(free) = system::free_space(&choice.root) {
        // a little room to spare: the old files sit beside the new ones for a moment during an upgrade
        if free < need + 16 * 1048576 {
            return Err(Failure::Space { need, free });
        }
    }
    let made = make_dirs(&choice.root)?;
    let root = choice.root.clone();
    let dir = root.join(STAGING);
    let result = (|| -> Result<Vec<InstalledFile>, Failure> {
        if dir.exists() {
            fs::remove_dir_all(&dir)?;
        }
        fs::create_dir_all(&dir)?;
        system::hide(&dir);
        let program = payload.program()?;
        fs::write(dir.join(UNINSTALLER), &program)?;
        let mut staged =
            vec![InstalledFile { path: UNINSTALLER.into(), size: program.len() as u64, sha256: sha(&program) }];
        let mut stream = payload.stream()?;
        let mut announced: HashSet<&str> = HashSet::new();
        let keep_going = || !cancel.load(Ordering::Relaxed);
        for entry in files {
            if cancel.load(Ordering::Relaxed) {
                return Err(Failure::Cancelled);
            }
            let to = dir.join(&entry.path);
            if let Some(parent) = to.parent() {
                fs::create_dir_all(parent)?;
            }
            let mut out = BufWriter::with_capacity(1 << 20, fs::File::create(&to)?);
            let hash = stream.copy_entry(entry, &mut out, &keep_going)?;
            out.flush()?;
            drop(out);
            if hash != entry.sha256 {
                return Err(Failure::Other { detail: format!("{} is damaged in the installer", entry.path) });
            }
            staged.push(InstalledFile { path: entry.path.clone(), size: entry.size, sha256: hash });
            test_delay(cancel);
            unpacked += entry.size;
            bytes(unpacked, total);
            for c in &components {
                if !announced.contains(c.id.as_str()) && c.files.iter().all(|p| staged.iter().any(|s| &s.path == p)) {
                    announced.insert(&c.id);
                    done(&c.id);
                }
            }
        }
        if cancel.load(Ordering::Relaxed) {
            return Err(Failure::Cancelled);
        }
        Ok(staged)
    })();
    match result {
        Ok(files) => Ok(Staged { root, dir, made, files }),
        Err(e) => {
            discard(&choice.root, &made);
            Err(e)
        }
    }
}

fn sha(data: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    crate::payload::hex(&Sha256::digest(data))
}

impl Record {
    pub fn load(root: &Path) -> Option<Record> {
        let text = fs::read(root.join(RECORD)).ok()?;
        serde_json::from_slice(&text).ok()
    }
    pub fn save(&self) -> io::Result<()> {
        let root = PathBuf::from(&self.root);
        let temp = root.join(format!("{RECORD}.new"));
        fs::write(&temp, serde_json::to_vec_pretty(self).unwrap())?;
        fs::rename(&temp, root.join(RECORD))
    }
}

/// The install already on this computer, if any: where the registry says, or else the default folder.
pub fn existing(hive: &Hive) -> Option<Record> {
    if let Some(root) = hive.read(reg::OURS, "InstallLocation") {
        if let Some(r) = Record::load(Path::new(&root)) {
            return Some(r);
        }
    }
    Record::load(&system::default_root())
}

fn rollback(moves: &[(PathBuf, PathBuf)]) {
    for (from, to) in moves.iter().rev() {
        let _ = fs::rename(to, from);
    }
}

/// A path written the way Windows compares it: `/` as `\`, no trailing separator, no `\\?\`, one case.
fn folded(p: &Path) -> String {
    let s = p.as_os_str().to_string_lossy().replace('/', "\\");
    let s = s.strip_prefix(r"\\?\UNC\").map(|r| format!(r"\\{r}")).unwrap_or(s);
    let s = s.strip_prefix(r"\\?\").map(str::to_string).unwrap_or(s);
    s.trim_end_matches('\\').to_lowercase()
}

/// Whether two install folders are the same place on the disk. Windows paths ignore case, and one folder can be
/// written several ways (`c:\apps\dotany`, `C:\Apps\DotAny\`, a SUBST drive): the folder is resolved when it exists,
/// and the words are compared the way Windows compares them either way.
pub fn same_dir(a: &Path, b: &Path) -> bool {
    if folded(a) == folded(b) {
        return true;
    }
    match (fs::canonicalize(a), fs::canonicalize(b)) {
        (Ok(x), Ok(y)) => folded(&x) == folded(&y),
        _ => false,
    }
}

/// The Start menu shortcut an installed component has, by the same rule `shortcut_name` uses.
fn installed_shortcut_name(c: &InstalledComponent) -> String {
    if c.exe.as_deref().map(|e| e.eq_ignore_ascii_case(HUB)).unwrap_or(false) {
        "DotAny.lnk".to_string()
    } else {
        format!("DotAny {}.lnk", c.name)
    }
}

/// Puts the earlier install's registration back after this one failed part-way: what this install wrote goes, then
/// the earlier install's keys are written again from its record (start with Windows too, when it had that), and its
/// Start menu shortcuts come back. `items` and `created` are what this install wrote; `shortcuts` the ones it made.
/// Both failure branches of `commit` go through here, so neither can forget a step.
fn restore_old(
    hive: &Hive,
    release: &Release,
    old: Option<&Record>,
    lang: &str,
    items: &[RegItem],
    created: &[String],
    shortcuts: &[String],
) {
    for item in items {
        let _ = hive.remove(item);
    }
    let kept: Vec<String> = old.map(|o| o.shortcuts.clone()).unwrap_or_default();
    for s in shortcuts {
        if !kept.contains(s) {
            let _ = fs::remove_file(s);
        }
    }
    if let Some(old) = old {
        let ids: Vec<String> = old.components.iter().map(|c| c.id.clone()).collect();
        let back = release.resolve(&ids);
        let old_root = PathBuf::from(&old.root);
        let facts = Facts { root: &old_root, version: &old.version, lang, size_kb: 0, date: &old.installed };
        for op in reg::registration(hive, &facts, &back, &release.claim, &old.registry) {
            let _ = hive.apply(&op);
        }
        if old.autostart {
            let _ = hive.apply(&reg::autostart(&old_root));
        }
        let menu = system::start_menu();
        for c in old.components.iter().filter(|c| c.exe.is_some()) {
            let lnk = menu.join(installed_shortcut_name(c));
            let s = lnk.display().to_string();
            if old.shortcuts.contains(&s) {
                let _ = system::shortcut(&lnk, &old_root.join(c.exe.as_ref().unwrap()));
            }
        }
    }
    hive.sweep(created, &[]);
}

fn end_hub(roots: &[&Path]) -> bool {
    let mut ended = false;
    for root in roots {
        for (pid, path) in system::running_under(root) {
            if path.file_name().map(|n| n.eq_ignore_ascii_case(HUB)).unwrap_or(false) {
                // asked to close first (its page has nothing unsaved), then ended with its WebView2
                system::end(pid, std::time::Duration::from_secs(5));
                ended = true;
            }
        }
    }
    ended
}

fn shortcut_name(c: &Component) -> String {
    if c.required { "DotAny.lnk".to_string() } else { format!("DotAny {}.lnk", c.name) }
}

/// Step 2.
pub fn commit(
    payload: &Payload,
    staged: Staged,
    choice: &Choice,
    hive: &Hive,
    lang: &str,
    autostart: bool,
) -> Result<Record, Failure> {
    let release = &payload.release;
    let components = release.resolve(&choice.programs);
    let old = existing(hive);
    let root = staged.root.clone();
    let old_root = old.as_ref().map(|o| PathBuf::from(&o.root));
    // the earlier install is in this same folder, however its record wrote it: then nothing is cleaned from "an old place"
    let same_place = old_root.as_ref().map(|o| same_dir(o, &root)).unwrap_or(true);
    let mut roots: Vec<&Path> = vec![&root];
    if let Some(o) = &old_root {
        roots.push(o);
    }
    let hub_was_running = end_hub(&roots);

    let backup = root.join(OLD);
    let _ = fs::remove_dir_all(&backup);
    let mut moves: Vec<(PathBuf, PathBuf)> = Vec::new();
    // the family fonts are Windows fonts while installed (reg::FONTS), and Windows holds a font it has loaded: one that
    // is the same as the one there stays put, one that changes is let go first and taken up again after the commit
    let real = !hive.is_test();
    let mut fonts_placed: Vec<PathBuf> = Vec::new();
    let swap = (|| -> io::Result<()> {
        fs::create_dir_all(&backup)?;
        system::hide(&backup);
        for f in &staged.files {
            let target = root.join(&f.path);
            let new = staged.dir.join(&f.path);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            if reg::is_font(&f.path) {
                if fs::read(&target).map(|d| sha(&d) == f.sha256).unwrap_or(false) {
                    continue;
                }
                if real && target.exists() {
                    system::fonts_changed(std::slice::from_ref(&target), false);
                }
                fonts_placed.push(target.clone());
            }
            if target.exists() {
                let aside = backup.join(&f.path);
                if let Some(parent) = aside.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::rename(&target, &aside)?;
                moves.push((target.clone(), aside));
            }
            fs::rename(&new, &target)?;
            moves.push((new, target));
        }
        // what the last install had and this one doesn't (a tool left out this time) goes aside as well
        if let Some(old) = &old {
            let keep: HashSet<String> = staged.files.iter().map(|f| folded(&root.join(&f.path))).collect();
            for f in &old.files {
                let p = if same_place { root.join(&f.path) } else { Path::new(&old.root).join(&f.path) };
                if keep.contains(&folded(&p)) || !p.exists() {
                    continue;
                }
                if p.starts_with(&root) {
                    if real && reg::is_font(&f.path) {
                        system::fonts_changed(std::slice::from_ref(&p), false);
                    }
                    let aside = backup.join(&f.path);
                    if let Some(parent) = aside.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    fs::rename(&p, &aside)?;
                    moves.push((p, aside));
                }
            }
        }
        Ok(())
    })();
    // after a failure the old fonts are back where they were: Windows takes them up again
    let fonts_back = |placed: &[PathBuf]| {
        if real {
            system::fonts_changed(&placed.iter().filter(|p| p.exists()).cloned().collect::<Vec<_>>(), true);
        }
    };
    if let Err(e) = swap {
        rollback(&moves);
        fonts_back(&fonts_placed);
        let _ = fs::remove_dir_all(&backup);
        discard(&root, &staged.made);
        return Err(e.into());
    }

    let size: u64 = staged.files.iter().map(|f| f.size).sum();
    let date = system::today();
    let facts = Facts { root: &root, version: &release.version, lang, size_kb: (size / 1024) as u32, date: &date };
    let had = old.as_ref().map(|o| o.registry.clone()).unwrap_or_default();
    let mut ops = reg::registration(hive, &facts, &components, &release.claim, &had);
    if autostart {
        ops.push(reg::autostart(&root));
    }
    let items: Vec<RegItem> = ops.iter().map(Op::item).collect();
    let tool_keys: Vec<String> = components.iter().flat_map(|c| c.cleanup.iter().cloned()).collect();
    let mut created = old.as_ref().map(|o| o.created_keys.clone()).unwrap_or_default();
    // the fonts Windows has to take up once this commit stands: the ones just put in place, and any not yet a Windows font
    let mut fonts_new = fonts_placed.clone();
    for f in staged.files.iter().filter(|f| reg::is_font(&f.path)) {
        let p = root.join(&f.path);
        if !fonts_new.contains(&p) && !hive.has_value(reg::FONTS, &reg::font_value(&f.path)) {
            fonts_new.push(p);
        }
    }
    let written = (|| -> io::Result<()> {
        if let Some(old) = &old {
            for item in &old.registry {
                if !items.contains(item) {
                    hive.remove(item)?;
                }
            }
            for key in &old.tool_keys {
                if !tool_keys.contains(key) {
                    hive.remove_tree(key)?;
                }
            }
        }
        for op in &ops {
            for key in hive.apply(op)? {
                if !created.contains(&key) {
                    created.push(key);
                }
            }
        }
        Ok(())
    })();
    if let Err(e) = written {
        // the files go back; the keys this install wrote go; the earlier install's keys are written again from its record
        rollback(&moves);
        let _ = fs::remove_dir_all(&backup);
        restore_old(hive, release, old.as_ref(), lang, &items, &created, &[]);
        fonts_back(&fonts_placed);
        discard(&root, &staged.made);
        return Err(e.into());
    }
    let created = hive.sweep(&created, &[]);

    // the Start menu: one folder, the hub and each tool
    let menu = system::start_menu();
    let mut shortcuts = Vec::new();
    for c in components.iter().filter(|c| c.exe.is_some()) {
        let lnk = menu.join(shortcut_name(c));
        if system::shortcut(&lnk, &root.join(c.exe.as_ref().unwrap())).is_ok() {
            shortcuts.push(lnk.display().to_string());
        }
    }
    if let Some(old) = &old {
        for s in &old.shortcuts {
            if !shortcuts.contains(s) {
                let _ = fs::remove_file(s);
            }
        }
    }

    let record = Record {
        schema: SCHEMA,
        product: release.product.clone(),
        version: release.version.clone(),
        root: root.display().to_string(),
        installed: system::now(),
        components: components
            .iter()
            .map(|c| InstalledComponent {
                id: c.id.clone(),
                name: c.name.clone(),
                exe: c.exe.clone(),
                version: c.version.clone(),
            })
            .collect(),
        files: staged.files.clone(),
        registry: items,
        created_keys: created,
        tool_keys,
        shortcuts,
        autostart,
        // the install folder itself goes with its files; only the folders above it are kept here
        made_dirs: match old.as_ref().filter(|_| same_place) {
            Some(o) => o.made_dirs.clone(),
            None => staged.made.iter().filter(|d| !same_dir(d, &root)).map(|d| d.display().to_string()).collect(),
        },
    };
    if let Err(e) = record.save() {
        // without the record the uninstaller wouldn't know what to take away, so this install doesn't stand: the
        // files go back and the earlier install is registered again, exactly as when the registry couldn't be written
        let _ = fs::remove_file(root.join(format!("{RECORD}.new")));
        rollback(&moves);
        let _ = fs::remove_dir_all(&backup);
        restore_old(hive, release, old.as_ref(), lang, &record.registry, &record.created_keys, &record.shortcuts);
        fonts_back(&fonts_placed);
        discard(&root, &staged.made);
        return Err(e.into());
    }

    // tidy up: the old files (a running one stays behind, hidden, until next time), the staging folder, an old place
    let _ = fs::remove_dir_all(&backup);
    let _ = fs::remove_dir_all(&staged.dir);
    if let (Some(old), Some(old_root)) = (&old, &old_root) {
        if !same_place {
            remove_files(old_root, old.files.iter().map(|f| f.path.as_str()));
            let _ = fs::remove_file(old_root.join(RECORD));
            let _ = fs::remove_dir_all(old_root.join(OLD));
            let _ = fs::remove_dir(old_root);
        }
    }
    if real {
        system::fonts_changed(&fonts_new, true);
    }
    reg::announce();
    if OPEN_HUB_AFTER.load(Ordering::Relaxed) {
        system::start(&root.join(HUB), &[]);
    } else if hub_was_running {
        system::start(&root.join(HUB), &["--autostart"]);
    }
    Ok(record)
}

/// Removes files and then the folders they leave empty, never anything else.
fn remove_files<'a>(root: &Path, files: impl Iterator<Item = &'a str>) -> Vec<PathBuf> {
    let mut left = Vec::new();
    let mut dirs: Vec<PathBuf> = Vec::new();
    for f in files {
        let p = root.join(f);
        if p.exists() && fs::remove_file(&p).is_err() {
            left.push(p.clone());
        }
        let mut at = p.parent();
        while let Some(d) = at {
            if d == root || !d.starts_with(root) {
                break;
            }
            if !dirs.iter().any(|x| x == d) {
                dirs.push(d.to_path_buf());
            }
            at = d.parent();
        }
    }
    dirs.sort_by_key(|d| std::cmp::Reverse(d.components().count()));
    for d in dirs {
        let _ = fs::remove_dir(d);
    }
    left
}

/// Switches starting with Windows on or off for an install, and keeps its record true to it.
pub fn set_autostart(hive: &Hive, root: &Path, on: bool) -> io::Result<()> {
    let mut record = Record::load(root).ok_or(io::ErrorKind::NotFound)?;
    let op = reg::autostart(root);
    let item = op.item();
    if on {
        for key in hive.apply(&op)? {
            if !record.created_keys.contains(&key) {
                record.created_keys.push(key);
            }
        }
        if !record.registry.contains(&item) {
            record.registry.push(item);
        }
    } else {
        hive.remove(&item)?;
        record.registry.retain(|i| i != &item);
        record.created_keys = hive.sweep(&record.created_keys, &[]);
    }
    record.autostart = on;
    record.save()
}

/// What the uninstaller reports: how many programs went, and anything Windows wouldn't let go of.
#[derive(Debug, Serialize)]
pub struct Removed {
    pub programs: usize,
    pub bytes: u64,
    pub left: Vec<String>,
}

/// The programs open from `root` that have to be closed by the person first, by file name (dotany-text.exe), once
/// each: the hub (no unsaved work, ended by the commit) and this program are left out.
pub fn open_programs(root: &Path) -> Vec<String> {
    let me = std::process::id();
    let mut files: Vec<String> = system::running_under(root)
        .into_iter()
        .filter(|(pid, path)| {
            *pid != me
                && !path
                    .file_name()
                    .map(|n| n.eq_ignore_ascii_case(HUB) || n.eq_ignore_ascii_case(UNINSTALLER))
                    .unwrap_or(false)
        })
        .filter_map(|(_, path)| path.file_name().map(|n| n.to_string_lossy().to_string()))
        .collect();
    files.sort();
    files.dedup();
    files
}

/// Takes away exactly what `record` says this install put there. The person's own files are never touched; their
/// settings only when they ask.
pub fn uninstall(record: &Record, hive: &Hive, remove_settings: bool) -> Result<Removed, Failure> {
    let root = PathBuf::from(&record.root);
    let busy: Vec<String> = open_programs(&root)
        .into_iter()
        .map(|file| {
            record
                .components
                .iter()
                .find(|c| c.exe.as_deref().map(|e| e.eq_ignore_ascii_case(&file)).unwrap_or(false))
                .map(|c| format!("DotAny {}", c.name))
                .unwrap_or(file)
        })
        .collect();
    if !busy.is_empty() {
        return Err(Failure::Busy { names: busy });
    }
    end_hub(&[&root]);

    for item in &record.registry {
        let _ = hive.remove(item);
    }
    for key in &record.tool_keys {
        let _ = hive.remove_tree(key);
    }
    // the hub's own settings switch may have turned start-with-Windows on after the install left it off: the value is
    // ours to remove either way while it starts this install's dotany.exe
    if let Some(now) = hive.read(reg::RUN, "DotAny") {
        if now.to_ascii_lowercase().contains(&root.join(HUB).display().to_string().to_ascii_lowercase()) {
            let _ = hive.remove_value(reg::RUN, "DotAny");
        }
    }
    hive.sweep(&record.created_keys, &[]);
    for s in &record.shortcuts {
        let _ = fs::remove_file(s);
    }
    let _ = fs::remove_dir(system::start_menu());

    // the fonts stop being Windows fonts (their values went with the registry above) and Windows lets go of the files.
    // This is the whole family going: a tool left out by Custom keeps the fonts, which every DotAny program needs
    if !hive.is_test() {
        let fonts: Vec<PathBuf> =
            record.files.iter().filter(|f| reg::is_font(&f.path)).map(|f| root.join(&f.path)).collect();
        system::fonts_changed(&fonts, false);
    }
    let mut left: Vec<String> = remove_files(&root, record.files.iter().map(|f| f.path.as_str()))
        .iter()
        .map(|p| p.display().to_string())
        .collect();
    let _ = fs::remove_dir_all(root.join(OLD));
    let _ = fs::remove_dir_all(root.join(STAGING));
    let _ = fs::remove_file(root.join(RECORD));
    let _ = fs::remove_dir(&root);
    for dir in record.made_dirs.iter().rev() {
        let _ = fs::remove_dir(dir);
    }
    if root.join(OLD).exists() {
        left.push(root.join(OLD).display().to_string());
    }
    if remove_settings {
        let dir = system::settings_dir();
        if dir.exists() && fs::remove_dir_all(&dir).is_err() {
            left.push(dir.display().to_string());
        }
    }
    reg::announce();
    let programs = record.components.iter().filter(|c| c.exe.is_some()).count();
    Ok(Removed { programs, bytes: record.files.iter().map(|f| f.size).sum(), left })
}

/// Sizes for the page, in whole MB the way the cards show them.
#[derive(Serialize)]
pub struct Sizes {
    pub full: u64,
    pub per_program: Vec<(String, u64)>,
}

pub fn sizes(payload: &Payload) -> Sizes {
    let release: &Release = &payload.release;
    let all: Vec<String> = release.programs().map(|c| c.id.clone()).collect();
    let per_program = release
        .programs()
        .map(|c| (c.id.clone(), mb(c.files.iter().filter_map(|p| release.file(p)).map(|f| f.size).sum())))
        .collect();
    Sizes { full: mb(need_bytes(payload, &all)), per_program }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn one_folder_written_two_ways_is_the_same_place() {
        assert!(same_dir(Path::new(r"c:\apps\dotany"), Path::new(r"C:\Apps\DotAny")));
        assert!(same_dir(Path::new("C:\\Apps\\DotAny\\"), Path::new("C:/Apps/DotAny")));
        assert!(same_dir(Path::new(r"\\?\C:\Apps\DotAny"), Path::new(r"c:\apps\dotany")));
        assert!(!same_dir(Path::new(r"C:\Apps\DotAny"), Path::new(r"C:\Apps\DotAny2")));
        assert!(!same_dir(Path::new(r"C:\Apps\DotAny"), Path::new(r"E:\Apps\DotAny")));
    }

    #[test]
    fn an_existing_folder_is_resolved_before_comparing() {
        let dir = std::env::temp_dir().join(format!("dotany-same-dir-{}", std::process::id()));
        fs::create_dir_all(dir.join("Inner")).unwrap();
        let a = dir.join("Inner");
        let b = dir.join("x").join("..").join("inner");
        fs::create_dir_all(dir.join("x")).unwrap();
        assert!(same_dir(&a, &b));
        let _ = fs::remove_dir_all(&dir);
    }
}
