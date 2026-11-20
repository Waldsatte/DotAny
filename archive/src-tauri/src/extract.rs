//! Extracting: every name is made safe before anything touches the disk. A name that climbs out with `..` is skipped,
//! never honoured; drive letters, colons and Windows' reserved names are made harmless; nothing that already exists is
//! overwritten or written into, because every top-level item gets a new name when its name is taken. Every file written
//! out of a marked archive gets the archive's "came from the internet" mark (`crate::mark`).

use crate::mark::Mark;
use crate::pack::{Counted, Node};
use crate::source::{EntryError, Source};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{self, BufWriter, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8", "COM9", "LPT1", "LPT2",
    "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9", "CONIN$", "CONOUT$",
];

/// One part of a name, as Windows can hold it. None for `..`, which is never followed.
pub fn safe_part(p: &str) -> Option<String> {
    if p == ".." {
        return None;
    }
    let mut s: String = p
        .chars()
        .map(|c| if c < ' ' || matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*') { '_' } else { c })
        .collect();
    // Windows drops trailing dots and spaces, which would turn "a." into "a"
    while s.ends_with('.') || s.ends_with(' ') {
        s.pop();
    }
    if s.is_empty() {
        s.push('_');
    }
    let stem = s.split('.').next().unwrap_or("").to_ascii_uppercase();
    if RESERVED.contains(&stem.trim_end()) {
        s.insert(0, '_');
    }
    Some(s)
}

/// The name one entry is written under in a folder of its own (opening it): the last part of `name` only, made safe, so
/// the file is always directly inside that folder. None for `..`.
pub fn leaf_name(name: &str) -> Option<String> {
    safe_part(name.rsplit(['/', '\\']).next().unwrap_or(""))
}

/// Entry `i`, straight from the archive into a new file `file`, never whole in memory, with the archive's mark.
pub fn entry_to_file(src: &mut Source, i: usize, password: Option<&str>, file: &Path) -> Result<(), EntryError> {
    let f = File::create(file).map_err(|_| EntryError::Damaged)?;
    let mut w = BufWriter::with_capacity(1 << 20, f);
    src.copy_one(i, password, &mut w).and_then(|_| w.flush().map_err(|_| EntryError::Damaged))?;
    drop(w);
    Mark::of(src.path()).put(file);
    Ok(())
}

/// A name inside the archive as parts that can only ever land inside the target folder.
pub fn safe_parts(path: &str) -> Option<Vec<String>> {
    let mut out = Vec::new();
    for p in path.split(['/', '\\']) {
        if p.is_empty() || p == "." {
            continue;
        }
        out.push(safe_part(p)?);
    }
    (!out.is_empty()).then_some(out)
}

/// "name (2).txt" for a file, "name (2)" for a folder, until the name is free.
pub fn unique(dir: &Path, name: &str, is_dir: bool, taken: &HashSet<String>) -> String {
    let free = |n: &str| !dir.join(n).exists() && !taken.contains(&n.to_lowercase());
    if free(name) {
        return name.to_string();
    }
    let (stem, ext) = match name.rfind('.') {
        Some(at) if !is_dir && at > 0 => (&name[..at], &name[at..]),
        _ => (name, ""),
    };
    (2..).map(|k| format!("{stem} ({k}){ext}")).find(|n| free(n)).unwrap()
}

fn inside(base: &Path, p: &Path) -> bool {
    p.starts_with(base)
        && p.strip_prefix(base).is_ok_and(|rest| rest.components().all(|c| matches!(c, Component::Normal(_))))
}

#[derive(Serialize, Default, Debug)]
pub struct Outcome {
    /// where it all went: the new folder, or the chosen location when nothing was wrapped
    pub target: String,
    pub files: usize,
    /// names that weren't safe (`..`) and were left out
    pub skipped: usize,
    /// entries whose data was broken or whose CRC didn't match: their partial files are removed
    pub bad: Vec<String>,
    /// encrypted entries left out because there was no password, or RAR's encryption
    pub locked: usize,
}

pub enum ExtractError {
    Cancelled,
    Io(io::Error),
}

/// The Unix permissions entries were packed with, by index (`Source::unix_modes`): the Mac keeps a program runnable and
/// makes a zip's or 7z's symbolic links links again. Windows has none to give.
pub type Modes = HashMap<usize, u32>;

/// A symbolic link's mode bits.
#[cfg(unix)]
const S_IFLNK: u32 = 0o120000;

/// The links an extraction met, made only once every file and folder is written (so nothing is ever written through
/// one), and kept only when they lead inside the target folder (`make_links`).
#[cfg(unix)]
fn link_text(r: &mut dyn io::Read) -> Option<String> {
    let mut v = Vec::new();
    io::Read::read_to_end(&mut io::Read::take(r, 4097), &mut v).ok()?;
    (v.len() <= 4096).then(|| String::from_utf8(v).ok()).flatten()
}

/// Every folder from `base` down to `dir` is a real one, not a link: a link made in a folder reached through another
/// link could lead somewhere the check of its own text doesn't see.
#[cfg(unix)]
fn real_folders(base: &Path, dir: &Path) -> bool {
    let Ok(rest) = dir.strip_prefix(base) else { return false };
    let mut at = base.to_path_buf();
    for c in rest.components() {
        let Component::Normal(name) = c else { return false };
        at.push(name);
        match fs::symlink_metadata(&at) {
            Ok(m) if m.is_dir() && !m.file_type().is_symlink() => {}
            _ => return false,
        }
    }
    true
}

/// Makes the links, then keeps only those that, followed as the system follows them, lead to something inside `base`:
/// one with an absolute target, one in a folder reached through a link, one leading out or nowhere, or round in a
/// loop, goes (counted as skipped). Taking one away can leave another leading nowhere, so this runs until none goes.
#[cfg(unix)]
fn make_links(base: &Path, links: Vec<(PathBuf, String)>, made: &mut Vec<PathBuf>, out: &mut Outcome) {
    let mut kept: Vec<PathBuf> = Vec::new();
    for (at, to) in links {
        let target = Path::new(&to);
        let fine = !to.is_empty()
            && !to.contains('\0')
            && !target.is_absolute()
            && at.parent().is_some_and(|p| real_folders(base, p))
            && fs::symlink_metadata(&at).is_err();
        if !fine || std::os::unix::fs::symlink(target, &at).is_err() {
            out.skipped += 1;
            continue;
        }
        made.push(at.clone());
        kept.push(at);
    }
    let Ok(root) = fs::canonicalize(base) else { return };
    loop {
        let before = kept.len();
        kept.retain(|l| {
            let inside = fs::canonicalize(l).is_ok_and(|p| p.starts_with(&root));
            if !inside {
                let _ = fs::remove_file(l);
                out.skipped += 1;
            }
            inside
        });
        if kept.len() == before {
            break;
        }
    }
    made.retain(|p| fs::symlink_metadata(p).is_ok());
    out.files += kept.len();
}

/// A file packed runnable stays runnable: the executable bits it was packed with are added to what it was written with.
#[cfg(unix)]
fn keep_runnable(target: &Path, mode: Option<u32>) {
    use std::os::unix::fs::PermissionsExt;
    let Some(x) = mode.map(|m| m & 0o111).filter(|x| *x != 0) else { return };
    if let Ok(m) = fs::metadata(target) {
        let mut p = m.permissions();
        p.set_mode((p.mode() & 0o777) | x);
        let _ = fs::set_permissions(target, p);
    }
}

impl From<io::Error> for ExtractError {
    fn from(e: io::Error) -> Self {
        if e.kind() == io::ErrorKind::Interrupted {
            return ExtractError::Cancelled;
        }
        ExtractError::Io(e)
    }
}

/// Extracts `nodes` into `location`, wrapped in a new folder named `folder` when one is given. `modes`: the entries'
/// Unix permissions, where the archive has them (the Mac's; Windows passes none).
#[allow(clippy::too_many_arguments)]
pub fn extract(
    location: &Path,
    folder: Option<&str>,
    source: Option<&mut Source>,
    password: Option<&str>,
    nodes: &[Node],
    modes: &Modes,
    done: &AtomicU64,
    now: &dyn Fn(&str),
    stop: &AtomicBool,
) -> Result<Outcome, ExtractError> {
    #[cfg(not(unix))]
    let _ = modes;
    let mark = source.as_deref().map(|s| Mark::of(s.path())).unwrap_or_default();
    // links, made last (`make_links`)
    #[cfg(unix)]
    let mut links: Vec<(PathBuf, String)> = Vec::new();
    let mut out = Outcome::default();
    let location = std::path::absolute(location)?;
    fs::create_dir_all(&location)?;
    // everything this run makes, so a cancel can take it back
    let mut made: Vec<PathBuf> = Vec::new();
    let base = match folder.and_then(safe_part) {
        Some(name) => {
            let name = unique(&location, &name, true, &HashSet::new());
            let dir = location.join(name);
            fs::create_dir(&dir)?;
            made.push(dir.clone());
            dir
        }
        None => location.clone(),
    };
    out.target = base.to_string_lossy().into_owned();
    // top-level names are renamed once when they're taken, and everything below follows
    let mut tops: HashMap<String, String> = HashMap::new();
    let mut taken: HashSet<String> = HashSet::new();
    let mut plan: Vec<(usize, PathBuf)> = Vec::new();
    for (k, n) in nodes.iter().enumerate() {
        let Some(mut parts) = safe_parts(&n.path) else {
            out.skipped += 1;
            continue;
        };
        let top_dir = n.dir || parts.len() > 1;
        let key = parts[0].to_lowercase();
        let top = tops.entry(key).or_insert_with(|| {
            let free = unique(&base, &parts[0], top_dir, &taken);
            taken.insert(free.to_lowercase());
            free
        });
        parts[0] = top.clone();
        let target: PathBuf = parts.iter().fold(base.clone(), |p, s| p.join(s));
        if !inside(&base, &target) {
            out.skipped += 1;
            continue;
        }
        plan.push((k, target));
    }
    let undo = |made: &[PathBuf]| {
        for p in made.iter().rev() {
            if p.is_dir() {
                let _ = fs::remove_dir_all(p);
            } else {
                let _ = fs::remove_file(p);
            }
        }
    };
    let result = (|| -> Result<(), ExtractError> {
        let mkdir = |dir: &Path, made: &mut Vec<PathBuf>| -> io::Result<()> {
            let mut missing = Vec::new();
            let mut at = Some(dir);
            while let Some(d) = at {
                if d.exists() || d == base {
                    break;
                }
                missing.push(d.to_path_buf());
                at = d.parent();
            }
            for d in missing.into_iter().rev() {
                fs::create_dir(&d)?;
                made.push(d);
            }
            Ok(())
        };
        for (k, target) in &plan {
            if nodes[*k].dir {
                mkdir(target, &mut made)?;
            }
        }
        let wanted: HashMap<usize, PathBuf> = plan
            .iter()
            .filter(|(k, _)| !nodes[*k].dir && nodes[*k].i.is_some())
            .map(|(k, t)| (nodes[*k].i.unwrap(), t.clone()))
            .collect();
        if let Some(src) = source {
            if !wanted.is_empty() {
                let mut failure: Option<ExtractError> = None;
                src.walk(password, &|i| wanted.contains_key(&i), &mut |i, data| {
                    let target = &wanted[&i];
                    match data {
                        Ok(r) => {
                            if let Some(parent) = target.parent() {
                                if let Err(e) = mkdir(parent, &mut made) {
                                    failure = Some(e.into());
                                    return Ok(false);
                                }
                            }
                            // a link (a zip's or 7z's from a Mac or Linux): its data is where it leads; made at the end
                            #[cfg(unix)]
                            if modes.get(&i).is_some_and(|m| m & 0o170000 == S_IFLNK) {
                                match link_text(r) {
                                    Some(to) => links.push((target.clone(), to)),
                                    None => out.skipped += 1,
                                }
                                return Ok(true);
                            }
                            let file = match File::create(target) {
                                Ok(f) => f,
                                Err(e) => {
                                    failure = Some(e.into());
                                    return Ok(false);
                                }
                            };
                            made.push(target.clone());
                            let mut w = BufWriter::with_capacity(1 << 20, file);
                            now(target.file_name().map(|n| n.to_string_lossy()).as_deref().unwrap_or(""));
                            let mut counted = Counted { inner: r, done, stop };
                            let copied = io::copy(&mut counted, &mut w).and_then(|_| w.flush());
                            drop(w);
                            match copied {
                                Ok(()) => {
                                    out.files += 1;
                                    mark.put(target);
                                    #[cfg(unix)]
                                    keep_runnable(target, modes.get(&i).copied());
                                }
                                Err(e) if e.kind() == io::ErrorKind::Interrupted || stop.load(Ordering::Relaxed) => {
                                    failure = Some(ExtractError::Cancelled);
                                    return Ok(false);
                                }
                                Err(e)
                                    if matches!(
                                        e.kind(),
                                        io::ErrorKind::StorageFull | io::ErrorKind::PermissionDenied
                                    ) =>
                                {
                                    failure = Some(ExtractError::Io(e));
                                    return Ok(false);
                                }
                                // broken data or a CRC that doesn't match: the half-written file goes
                                Err(_) => {
                                    let _ = fs::remove_file(target);
                                    made.retain(|p| p != target);
                                    out.bad.push(
                                        target
                                            .strip_prefix(&base)
                                            .map(|p| p.to_string_lossy().replace('\\', "/"))
                                            .unwrap_or_default(),
                                    );
                                }
                            }
                            Ok(true)
                        }
                        Err(EntryError::Password) | Err(EntryError::Unsupported) => {
                            out.locked += 1;
                            Ok(true)
                        }
                        Err(EntryError::Damaged) => {
                            out.bad.push(
                                target
                                    .strip_prefix(&base)
                                    .map(|p| p.to_string_lossy().replace('\\', "/"))
                                    .unwrap_or_default(),
                            );
                            Ok(true)
                        }
                    }
                })
                .map_err(|e| {
                    if stop.load(Ordering::Relaxed) { ExtractError::Cancelled } else { ExtractError::Io(e) }
                })?;
                if let Some(f) = failure {
                    return Err(f);
                }
            }
        }
        // files added from disk and not yet saved into the archive: copied as they are, each with its own mark
        for (k, target) in &plan {
            let n = &nodes[*k];
            let (false, None, Some(from)) = (n.dir, n.i, n.add.as_deref()) else { continue };
            if let Some(parent) = target.parent() {
                mkdir(parent, &mut made)?;
            }
            let mut f = File::open(from)?;
            let file = File::create(target)?;
            made.push(target.clone());
            let mut w = BufWriter::new(file);
            now(target.file_name().map(|n| n.to_string_lossy()).as_deref().unwrap_or(""));
            let mut counted = Counted { inner: &mut f, done, stop };
            io::copy(&mut counted, &mut w)?;
            w.flush()?;
            drop(w);
            Mark::of(Path::new(from)).put(target);
            out.files += 1;
        }
        #[cfg(unix)]
        make_links(&base, std::mem::take(&mut links), &mut made, &mut out);
        Ok(())
    })();
    match result {
        Ok(()) => Ok(out),
        Err(e) => {
            undo(&made);
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use zip::write::SimpleFileOptions;

    /// A folder of its own for one test, gone when it ends.
    struct Scratch(PathBuf);
    impl Scratch {
        fn new(name: &str) -> Scratch {
            let d = std::env::temp_dir().join(format!("dotany-archive-extract-{name}-{}", std::process::id()));
            let _ = fs::remove_dir_all(&d);
            fs::create_dir_all(&d).unwrap();
            Scratch(d)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    enum Item<'a> {
        File(&'a str, &'a [u8], Option<u32>),
        #[allow(dead_code)]
        Link(&'a str, &'a str),
    }

    fn zip_of(path: &Path, items: &[Item]) {
        let mut z = zip::ZipWriter::new(File::create(path).unwrap());
        for it in items {
            match it {
                Item::File(name, data, mode) => {
                    let mut o = SimpleFileOptions::default();
                    if let Some(m) = mode {
                        o = o.unix_permissions(*m);
                    }
                    z.start_file(*name, o).unwrap();
                    z.write_all(data).unwrap();
                }
                Item::Link(name, to) => z.add_symlink(*name, *to, SimpleFileOptions::default()).unwrap(),
            }
        }
        z.finish().unwrap();
    }

    /// Every entry, as the page would send them back: its own name, its index.
    fn nodes_of(src: &Source) -> Vec<Node> {
        src.entries
            .iter()
            .map(|e| Node {
                path: e.n.clone().unwrap_or_default(),
                dir: e.d,
                i: Some(e.i),
                add: None,
                u: None,
                t: None,
            })
            .collect()
    }

    fn run(src: &mut Source, to: &Path, modes: &Modes) -> Outcome {
        let nodes = nodes_of(src);
        let (done, stop) = (AtomicU64::new(0), AtomicBool::new(false));
        match extract(to, None, Some(src), None, &nodes, modes, &done, &|_| {}, &stop) {
            Ok(o) => o,
            Err(_) => panic!("extract failed"),
        }
    }

    /// Everything under `dir`, as paths from it, links not followed.
    fn tree(dir: &Path) -> Vec<String> {
        fn walk(base: &Path, at: &Path, out: &mut Vec<String>) {
            for e in fs::read_dir(at).unwrap().flatten() {
                let p = e.path();
                out.push(p.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/"));
                if fs::symlink_metadata(&p).unwrap().is_dir() {
                    walk(base, &p, out);
                }
            }
        }
        let mut out = Vec::new();
        walk(dir, dir, &mut out);
        out.sort();
        out
    }

    #[test]
    fn names_that_climb_out_never_leave_the_chosen_folder() {
        let s = Scratch::new("climb");
        let zip = s.0.join("evil.zip");
        zip_of(
            &zip,
            &[
                Item::File("../evil1.txt", b"x", None),
                Item::File("a/../../evil2.txt", b"x", None),
                Item::File("..\\evil3.txt", b"x", None),
                Item::File("C:/evil4.txt", b"x", None),
                Item::File("/abs/evil5.txt", b"x", None),
                Item::File("ok.txt", b"ok", None),
            ],
        );
        let sandbox = s.0.join("sandbox");
        let to = sandbox.join("inside");
        fs::create_dir_all(&to).unwrap();
        let mut src = Source::open(&zip, None).unwrap();
        let out = run(&mut src, &to, &Modes::new());
        assert!(out.skipped >= 3, "the .. names are skipped: {out:?}");
        assert_eq!(fs::read_dir(&sandbox).unwrap().count(), 1, "nothing next to the chosen folder");
        let all = tree(&to);
        assert!(!all.iter().any(|p| p.contains("evil1") || p.contains("evil2") || p.contains("evil3")), "{all:?}");
        assert!(all.contains(&"ok.txt".to_string()), "{all:?}");
        assert!(!s.0.join("evil1.txt").exists() && !s.0.join("evil2.txt").exists());
    }

    #[cfg(unix)]
    fn mode_of(p: &Path) -> u32 {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(p).unwrap().permissions().mode() & 0o777
    }

    #[cfg(unix)]
    #[test]
    fn a_runnable_file_stays_runnable() {
        let s = Scratch::new("runnable");
        let zip = s.0.join("tools.zip");
        zip_of(
            &zip,
            &[
                Item::File("bin/run.sh", b"#!/bin/sh\necho hi\n", Some(0o755)),
                Item::File("read.txt", b"words", Some(0o644)),
                Item::File("made-on-windows.txt", b"words", None),
            ],
        );
        let mut src = Source::open(&zip, None).unwrap();
        let modes = src.unix_modes();
        let to = s.0.join("out");
        let out = run(&mut src, &to, &modes);
        assert_eq!(out.files, 3);
        assert_eq!(mode_of(&to.join("bin/run.sh")) & 0o111, 0o111, "runnable");
        assert_eq!(mode_of(&to.join("read.txt")) & 0o111, 0, "not runnable");
        assert_eq!(mode_of(&to.join("made-on-windows.txt")) & 0o111, 0);
        // and a TAR's
        let tar = s.0.join("tools.tar");
        {
            let mut b = tar::Builder::new(File::create(&tar).unwrap());
            for (name, mode) in [("run", 0o750), ("doc", 0o640)] {
                let mut h = tar::Header::new_gnu();
                h.set_path(name).unwrap();
                h.set_size(2);
                h.set_mode(mode);
                h.set_cksum();
                b.append(&h, &b"hi"[..]).unwrap();
            }
            b.finish().unwrap();
        }
        let mut src = Source::open(&tar, None).unwrap();
        let modes = src.unix_modes();
        let to = s.0.join("out-tar");
        run(&mut src, &to, &modes);
        assert_ne!(mode_of(&to.join("run")) & 0o100, 0);
        assert_eq!(mode_of(&to.join("doc")) & 0o111, 0);
    }

    #[cfg(unix)]
    #[test]
    fn links_come_back_as_links_but_only_those_that_lead_inside() {
        let s = Scratch::new("links");
        let zip = s.0.join("bundle.zip");
        zip_of(
            &zip,
            &[
                // a framework's, as a Mac zips an .app
                Item::File("App.app/Versions/A/lib", b"the library", Some(0o755)),
                Item::Link("App.app/Versions/Current", "A"),
                Item::Link("App.app/lib", "Versions/Current/lib"),
                // and every way out
                Item::Link("abs", "/etc/passwd"),
                Item::Link("up", "../outside.txt"),
                Item::Link("deep/up2", "../../outside.txt"),
                Item::Link("nowhere", "missing/file"),
                Item::Link("loop1", "loop2"),
                Item::Link("loop2", "loop1"),
                // a link to the folder itself leads inside; one named like a folder that a later entry needs as a
                // folder is never made, so nothing is written or linked through it (`here` is a real folder, and
                // `here/esc` leads to the extraction's own folder)
                Item::Link("self", "."),
                Item::Link("here", "."),
                Item::Link("here/esc", ".."),
                Item::Link("here/out", "../.."),
                // a link whose name is a folder already written: never replaces it, never written through
                Item::File("dir/payload", b"p", None),
                Item::Link("dir", "../outside-dir"),
            ],
        );
        let sandbox = s.0.join("sandbox");
        let to = sandbox.join("inside");
        fs::create_dir_all(&to).unwrap();
        fs::write(sandbox.join("outside.txt"), "outside").unwrap();
        let mut src = Source::open(&zip, None).unwrap();
        let modes = src.unix_modes();
        let out = run(&mut src, &to, &modes);
        let link = |p: &str| fs::symlink_metadata(to.join(p)).is_ok_and(|m| m.file_type().is_symlink());
        assert!(link("App.app/Versions/Current") && link("App.app/lib") && link("self"), "{:?}", tree(&to));
        assert_eq!(fs::read(to.join("App.app/lib")).unwrap(), b"the library");
        assert_eq!(fs::read_link(to.join("App.app/Versions/Current")).unwrap(), Path::new("A"));
        assert!(to.join("here").is_dir() && !link("here") && link("here/esc"), "{:?}", tree(&to));
        for bad in ["abs", "up", "deep/up2", "nowhere", "loop1", "loop2", "here/out"] {
            assert!(fs::symlink_metadata(to.join(bad)).is_err(), "{bad} was made: {:?}", tree(&to));
        }
        assert!(to.join("dir").is_dir() && !link("dir"), "the folder stays a folder");
        assert!(out.skipped >= 8, "{out:?}");
        // nothing anywhere leads out of the folder
        let root = fs::canonicalize(&to).unwrap();
        for p in tree(&to) {
            if let Ok(real) = fs::canonicalize(to.join(&p)) {
                assert!(real.starts_with(&root), "{p} leads to {}", real.display());
            }
        }
        assert_eq!(fs::read_to_string(sandbox.join("outside.txt")).unwrap(), "outside");
        assert_eq!(fs::read_dir(&sandbox).unwrap().count(), 2, "nothing new beside the folder");
    }

    /// RAR's names come decomposed from libarchive on the Mac: shown composed, as the Finder shows them.
    #[cfg(target_os = "macos")]
    #[test]
    fn rar_names_are_shown_composed() {
        let sample = Path::new(env!("CARGO_MANIFEST_DIR")).join("../test/fixtures/rar_unicode.rar");
        if !sample.exists() {
            eprintln!("skipped: {} is not here", sample.display());
            return;
        }
        let mut src = Source::open(&sample, None).unwrap();
        let before: Vec<String> = src.entries.iter().filter_map(|e| e.n.clone()).collect();
        crate::app::tidy(&mut src);
        let after: Vec<String> = src.entries.iter().filter_map(|e| e.n.clone()).collect();
        assert_eq!(before.len(), after.len());
        assert!(after.iter().all(|n| crate::platform::nfc(n) == *n), "{after:?}");
        // the archive has at least one name that decomposes (a kana with its voicing mark, or an accent)
        assert_ne!(before, after, "libarchive gave decomposed names here: {before:?}");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn the_macs_own_files_are_neither_listed_nor_packed_and_names_are_composed() {
        let s = Scratch::new("mac");
        // a zip as the Finder makes one: __MACOSX beside, a name decomposed
        let zip = s.0.join("from-a-mac.zip");
        zip_of(
            &zip,
            &[
                Item::File("cafe\u{301}.txt", b"coffee", None),
                Item::File("__MACOSX/._cafe\u{301}.txt", b"attributes", None),
                Item::File("__MACOSX/sub/._x", b"attributes", None),
            ],
        );
        let mut src = Source::open(&zip, None).unwrap();
        crate::app::tidy(&mut src);
        let names: Vec<String> = src.entries.iter().filter_map(|e| e.n.clone()).collect();
        assert_eq!(names, ["caf\u{e9}.txt"], "composed, and no __MACOSX");
        let to = s.0.join("out");
        run(&mut src, &to, &Modes::new());
        assert_eq!(tree(&to), ["caf\u{e9}.txt"]);
        // packing a folder: the Finder's .DS_Store and ._ files stay out, and names go in composed
        let dir = s.0.join("pack");
        fs::create_dir_all(dir.join("sub")).unwrap();
        for f in ["a.txt", ".DS_Store", "._a.txt", "sub/.DS_Store", "sub/re\u{301}sume\u{301}.txt"] {
            fs::write(dir.join(f), "x").unwrap();
        }
        let items = crate::app::walk_items(&[dir.to_string_lossy().into_owned()]);
        let rel: Vec<&str> = items.iter().map(|i| i.rel.as_str()).collect();
        assert!(!rel.iter().any(|r| r.contains(".DS_Store") || r.contains("/._")), "{rel:?}");
        let nodes: Vec<Node> = items
            .iter()
            .map(|i| Node { path: i.rel.clone(), dir: i.dir, i: None, add: Some(i.from.clone()), u: None, t: None })
            .collect();
        let packed = s.0.join("packed.zip");
        let (done, stop) = (AtomicU64::new(0), AtomicBool::new(false));
        let written = crate::pack::write_archive(
            &packed,
            crate::pack::Format::Zip,
            crate::pack::Level::Std,
            None,
            None,
            None,
            &nodes,
            &done,
            &|_| {},
            &stop,
        );
        assert!(written.is_ok());
        let mut z = zip::ZipArchive::new(File::open(&packed).unwrap()).unwrap();
        let raw: Vec<String> =
            (0..z.len()).map(|i| String::from_utf8(z.by_index_raw(i).unwrap().name_raw().to_vec()).unwrap()).collect();
        assert!(raw.contains(&"pack/sub/r\u{e9}sum\u{e9}.txt".to_string()), "the name as Windows reads it: {raw:?}");
        assert_eq!(raw.len(), 4, "pack/, pack/sub/, a.txt, the résumé: {raw:?}");
    }

    #[test]
    fn an_entry_opened_on_its_own_is_written_directly_inside_its_folder() {
        for (name, leaf) in [
            ("a.txt", "a.txt"),
            ("dir/a.txt", "a.txt"),
            (r"..\..\Startup\a.txt", "a.txt"),
            ("../../a.txt", "a.txt"),
            ("C:a.txt", "C_a.txt"),
            ("a.txt:x.exe", "a.txt_x.exe"),
            ("a.txt.", "a.txt"),
            ("", "_"),
        ] {
            assert_eq!(leaf_name(name).as_deref(), Some(leaf), "{name}");
            let dir = Path::new("temp");
            assert_eq!(dir.join(leaf).parent(), Some(dir), "{name}");
        }
        assert_eq!(leaf_name(".."), None);
        assert_eq!(leaf_name("a/.."), None);
    }

    /// `path:Zone.Identifier`, Windows' mark of a download.
    #[cfg(windows)]
    fn zone(path: &Path) -> Option<Vec<u8>> {
        let mut s = path.as_os_str().to_owned();
        s.push(":Zone.Identifier");
        fs::read(s).ok()
    }

    #[cfg(windows)]
    fn set_zone(path: &Path, v: &[u8]) {
        let mut s = path.as_os_str().to_owned();
        s.push(":Zone.Identifier");
        fs::write(s, v).unwrap();
    }

    #[cfg(windows)]
    const ZONE: &[u8] =
        b"[ZoneTransfer]\r\nZoneId=3\r\nReferrerUrl=https://example.com/\r\nHostUrl=https://example.com/a.zip\r\n";

    #[cfg(windows)]
    #[test]
    fn what_a_marked_archive_gives_carries_its_mark() {
        let s = Scratch::new("zone");
        let zip = s.0.join("download.zip");
        zip_of(&zip, &[Item::File("a.txt", b"a", None), Item::File("sub/b.docm", b"b", None)]);
        set_zone(&zip, ZONE);
        let mut src = Source::open(&zip, None).unwrap();
        // extracting everything
        let to = s.0.join("out");
        assert_eq!(run(&mut src, &to, &Modes::new()).files, 2);
        assert_eq!(zone(&to.join("a.txt")).as_deref(), Some(ZONE));
        assert_eq!(zone(&to.join("sub/b.docm")).as_deref(), Some(ZONE));
        assert_eq!(zone(&to.join("sub")), None, "folders are not marked");
        // one entry, as opening it writes it
        let one = s.0.join("one.txt");
        let i = src.entries.iter().find(|e| e.n.as_deref() == Some("a.txt")).unwrap().i;
        entry_to_file(&mut src, i, None, &one).unwrap();
        assert_eq!(fs::read(&one).unwrap(), b"a");
        assert_eq!(zone(&one).as_deref(), Some(ZONE));
        // a file added from disk keeps its own mark, or none
        let (marked, plain) = (s.0.join("marked.txt"), s.0.join("plain.txt"));
        fs::write(&marked, "m").unwrap();
        fs::write(&plain, "p").unwrap();
        let own = b"[ZoneTransfer]\r\nZoneId=3\r\n";
        set_zone(&marked, own);
        let nodes: Vec<Node> = [&marked, &plain]
            .iter()
            .map(|p| Node {
                path: p.file_name().unwrap().to_string_lossy().into_owned(),
                dir: false,
                i: None,
                add: Some(p.to_string_lossy().into_owned()),
                u: None,
                t: None,
            })
            .collect();
        let added = s.0.join("added");
        let (done, stop) = (AtomicU64::new(0), AtomicBool::new(false));
        assert!(extract(&added, None, Some(&mut src), None, &nodes, &Modes::new(), &done, &|_| {}, &stop).is_ok());
        assert_eq!(zone(&added.join("marked.txt")).as_deref(), Some(&own[..]));
        assert_eq!(zone(&added.join("plain.txt")), None);
    }

    #[cfg(windows)]
    #[test]
    fn an_unmarked_archive_gives_no_mark() {
        let s = Scratch::new("nozone");
        let zip = s.0.join("made-here.zip");
        zip_of(&zip, &[Item::File("a.txt", b"a", None)]);
        let mut src = Source::open(&zip, None).unwrap();
        let to = s.0.join("out");
        run(&mut src, &to, &Modes::new());
        assert_eq!(zone(&to.join("a.txt")), None);
        let one = s.0.join("one.txt");
        let i = src.entries[0].i;
        entry_to_file(&mut src, i, None, &one).unwrap();
        assert_eq!(zone(&one), None);
    }

    /// The Mac's mark, `com.apple.quarantine`: read and written with `xattr`, the system's own command.
    #[cfg(target_os = "macos")]
    #[test]
    fn what_a_quarantined_archive_gives_is_quarantined() {
        use std::process::Command;
        let attr = |p: &Path| {
            let out = Command::new("xattr").args(["-p", "com.apple.quarantine"]).arg(p).output().unwrap();
            out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string())
        };
        let s = Scratch::new("quarantine");
        let zip = s.0.join("download.zip");
        zip_of(&zip, &[Item::File("a.txt", b"a", None)]);
        let value = "0083;6745a1b2;Safari;1F2E3D4C-0000-0000-0000-000000000000";
        assert!(
            Command::new("xattr").args(["-w", "com.apple.quarantine", value]).arg(&zip).status().unwrap().success()
        );
        let mut src = Source::open(&zip, None).unwrap();
        let to = s.0.join("out");
        run(&mut src, &to, &Modes::new());
        assert_eq!(attr(&to.join("a.txt")).as_deref(), Some(value));
        let one = s.0.join("one.txt");
        let i = src.entries[0].i;
        entry_to_file(&mut src, i, None, &one).unwrap();
        assert_eq!(attr(&one).as_deref(), Some(value));
        // and an archive without it gives none
        let plain = s.0.join("plain.zip");
        zip_of(&plain, &[Item::File("b.txt", b"b", None)]);
        let mut src = Source::open(&plain, None).unwrap();
        let to = s.0.join("out2");
        run(&mut src, &to, &Modes::new());
        assert_eq!(attr(&to.join("b.txt")), None);
    }
}
