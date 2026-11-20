//! Check for Updates (looking for a newer release) and Install (installing a tool, or updating): where a release comes
//! from, what is believed about it, and what is checked before anything is started.
//!
//! **Where from** (`PLACES`): a release is published in two places with the same files by the same names, the site
//! (`SITE`) and GitHub (`LATEST`), and they are asked in that order, because GitHub's downloads are often slow or out of
//! reach in mainland China. Which place a file came from changes nothing about trusting it: the signature and the
//! sums below are checked the same way for every place. A place that doesn't answer (it can't be reached, it took too
//! long, it hasn't the file, it answers something that isn't a release) passes the turn to the next, and so does one
//! whose files don't pass a check: a broken or tampered copy is never taken, and never stands in the way of a good one.
//! A place with another one after it is waited for briefly (`net::QUICK`), so a site that doesn't answer costs
//! seconds. When no place gives what is needed, the answer is the one that says the most: a file that failed a check
//! (failed), then an answer that wasn't a release (unreadable), then no answer (offline).
//!
//! **Checking** reads a place's `SHA256SUMS.txt`, its signature and `manifest.json` (plain downloads of a few KB, not
//! GitHub's API, which allows 60 calls an hour per address), and believes the versions in the manifest only when the
//! list is signed by a trusted key and names that manifest (`release_at`, the same reading a download starts with), so
//! a place that has been tampered with can neither offer a version nor claim there is none. It compares each part's
//! version with what is on this computer (`check` says how the places are weighed). It runs
//! when the person presses Check for Updates, and on its own at most once a day while DotAny runs. Nothing is ever
//! installed without being asked: with Auto-download updates on, a newer version is downloaded and checked by itself
//! (`fetch_by_itself`), then waits as `ready` (Update ready to install) for Update; with it off only Update downloads
//! it.
//!
//! **Getting** (Install a tool, or Update) downloads the whole `DotAny-Setup.exe` (on the Mac, `DotAny-mac.zip`) into
//! the `DotAny-update` folder of the system's temporary folder, named `.part` until it has passed, in this order:
//!
//! 1. the Ed25519 signature on `SHA256SUMS.txt` (`SHA256SUMS.txt.sig`, minisign's format) by one of `TRUSTED_KEYS`;
//! 2. `manifest.json`'s SHA-256 against that list, so the versions read from it are ours too;
//! 3. no part goes back to a lower version than the one installed;
//! 4. `DotAny-Setup.exe`'s SHA-256 against that list.
//!
//! All four come from one place; when any of them doesn't come or doesn't pass, the next place is tried from 1. (The
//! sums bind the files together, so they could even be mixed, but one place at a time is simpler to follow.)
//!
//! Then the installer starts in its update mode (`--update`), which asks the person to close open DotAny programs and
//! opens DotAny again when it is done. Only https, and only the places' own hosts (dotany.app, github.com) and GitHub's
//! download hosts (*.githubusercontent.com).
//!
//! **A place that lags** (the site not yet carrying a release GitHub has, or the other way round): a release lower than
//! what is installed is never offered or installed, so a place answering one is passed over and the next one asked.
//! A check takes the first signed release with something newer; a place that has only what is installed doesn't end
//! the check, the next place is asked too (briefly, as there is an answer in hand), so a site that lags, or answers
//! the installed version on purpose, can't keep a newer signed release on GitHub from being seen. An update (not
//! Install of a tool) needs a release with something newer than what is installed: a place without one is passed
//! over, so the update a check found on GitHub is not swapped for the site's older copy.
//!
//! Tests: `DOTANY_UPDATE_URL` points at a release served on this computer (http://127.0.0.1:…/, nothing else is taken;
//! several, comma-separated, stand for several places in that order),
//! and only together with it `DOTANY_UPDATE_TEST_KEY` adds a test key; `DOTANY_UPDATE_LAUNCHED` names a file where the
//! installer's command line is written as it starts.

use crate::net::{self, Fail, Url};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::Write;
use std::path::{Path, PathBuf};

/// The site's copy of the latest release: the same files as GitHub's, by the same names, at this address followed by
/// the file's name. Its host is let through, and nothing wider.
pub const SITE: &str = "https://dotany.app/releases/latest/";

/// Where the latest published release is on GitHub: the redirect `releases/latest/download/` follows to the newest one.
pub const LATEST: &str = "https://github.com/Waldsatte/DotAny/releases/latest/download/";

/// Where a release is looked for, in this order.
pub const PLACES: &[&str] = &[SITE, LATEST];

/// The public halves of the keys that sign releases (minisign form). More than one may be here at once, so a new key
/// can come in with one release and the old one go with a later one. The secret half is never in the repository or its
/// build service: each release's SHA256SUMS.txt is signed apart from GitHub (.github/scripts/sign-update.mjs), so
/// write access to the repository alone can't make an update the hub accepts.
pub const TRUSTED_KEYS: &[&str] = &["RWRTNXn4HhYmDLLt259wSyXBPb3H1itR3a+ApiZcvVPp3uX3J00Rjkca"];

/// The manifest's own format (installer/src-tauri/src/manifest.rs). A release in a format DotAny doesn't know is left
/// alone.
const SCHEMA: u32 = 1;
#[cfg(not(target_os = "macos"))]
const SETUP: &str = "DotAny-Setup.exe";
/// The Mac's: the DotAny folder of .apps, zipped by `ditto -c -k --keepParent` (links, permissions and signatures kept).
#[cfg(target_os = "macos")]
const SETUP: &str = "DotAny-mac.zip";
const MAX_SETUP: u64 = 512 << 20;

#[derive(Deserialize)]
pub struct Manifest {
    pub schema: u32,
    pub version: String,
    pub components: Vec<Part>,
}

#[derive(Deserialize)]
pub struct Part {
    pub id: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub shared: bool,
}

/// What the page is told. `state`: idle, checking, latest, newer, offline (no answer), unreadable (an answer, but not a
/// release DotAny can read), downloading, failed (the download didn't pass), installing (the installer is running).
#[derive(Serialize, Clone, Debug, Default, PartialEq)]
pub struct Status {
    pub state: String,
    /// When the last check that got an answer was, ms since 1970; 0 for never.
    pub checked: u64,
    /// The release's version.
    pub version: String,
    /// Installed parts the release has a higher version of: "hub" for DotAny itself, else the tool's key.
    pub newer: Vec<Newer>,
    /// The tools the release carries, so Install is only offered for those.
    pub offered: Vec<String>,
    /// Downloading: 0 to 1.
    pub pct: f64,
    /// Downloading and installing: the parts it is for.
    pub parts: Vec<String>,
    /// The Mac: Update was pressed while these DotAny programs (their names, as the Dock shows them) ran from the .apps
    /// it would replace; nothing was replaced and the version stays `ready`.
    #[cfg(target_os = "macos")]
    pub open: Vec<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Newer {
    pub id: String,
    pub from: String,
    pub to: String,
}

/// Why a check or a download stopped, as the page says it.
#[derive(Debug, Clone, PartialEq)]
pub enum Problem {
    Offline,
    Unreadable,
    Failed,
    /// The Mac: programs from the .apps an update would replace are open (their names); nothing was replaced.
    #[cfg(target_os = "macos")]
    Open(Vec<String>),
}

impl Problem {
    pub fn state(&self) -> &'static str {
        match self {
            Problem::Offline => "offline",
            Problem::Unreadable => "unreadable",
            Problem::Failed => "failed",
            // what was downloaded and checked still waits
            #[cfg(target_os = "macos")]
            Problem::Open(_) => "ready",
        }
    }
}

fn from_net(f: Fail) -> Problem {
    match f {
        Fail::Offline(_) => Problem::Offline,
        Fail::Refused(_) | Fail::Stopped => Problem::Unreadable,
    }
}

/// Of two reasons no place gave a release, the one that says more: something that failed a check, then an answer
/// that wasn't a release, then no answer.
fn worse(a: Option<Problem>, b: Problem) -> Problem {
    let rank = |p: &Problem| match p {
        Problem::Offline => 0,
        Problem::Unreadable => 1,
        Problem::Failed => 2,
        #[cfg(target_os = "macos")]
        Problem::Open(_) => 3,
    };
    match a {
        Some(a) if rank(&a) >= rank(&b) => a,
        _ => b,
    }
}

// ---------- where from, and what to trust ----------

pub struct Source {
    /// The places, each ending in "/", in the order they are asked.
    pub bases: Vec<String>,
    /// A test release on this computer: http and that host are let through.
    pub local: bool,
    pub keys: Vec<String>,
    /// How long a place with another one after it is waited for; the last place is waited for with `net::PATIENT`.
    pub quick: net::Patience,
}

pub(crate) fn is_local(host: &str) -> bool {
    host == "127.0.0.1" || host == "localhost"
}

/// `DOTANY_UPDATE_URL`'s places: one or more addresses, comma-separated, each on this computer; else none.
fn local_bases(list: &str) -> Option<Vec<String>> {
    let bases: Vec<String> = list
        .split(',')
        .map(str::trim)
        .filter(|b| !b.is_empty())
        .map(|b| if b.ends_with('/') { b.to_string() } else { format!("{b}/") })
        .collect();
    // only a server on this computer can stand in for the site or GitHub
    (!bases.is_empty() && bases.iter().all(|b| net::parse(b).is_some_and(|u| is_local(&u.host)))).then_some(bases)
}

impl Source {
    pub fn from_env() -> Source {
        let mut keys: Vec<String> = TRUSTED_KEYS.iter().map(|k| k.to_string()).collect();
        let test = std::env::var("DOTANY_UPDATE_URL").ok().and_then(|b| local_bases(&b));
        let Some(bases) = test else {
            let bases = PLACES.iter().map(|p| p.to_string()).collect();
            return Source { bases, local: false, keys, quick: net::QUICK };
        };
        if let Ok(k) = std::env::var("DOTANY_UPDATE_TEST_KEY") {
            keys.push(k);
        }
        Source { bases, local: true, keys, quick: net::QUICK }
    }

    /// https to a place's own host (the site's, github.com) or GitHub's download hosts; http only to a test server here.
    pub fn allowed(&self, u: &Url) -> bool {
        let place = |h: &str| PLACES.iter().any(|p| net::parse(p).is_some_and(|p| p.host == h));
        (u.secure && (place(&u.host) || u.host.ends_with(".githubusercontent.com")))
            || (self.local && is_local(&u.host))
    }

    /// How long the `i`th place is waited for.
    fn patience(&self, i: usize) -> net::Patience {
        if i + 1 < self.bases.len() { self.quick } else { net::PATIENT }
    }
}

fn agent() -> String {
    format!("DotAny/{}", env!("CARGO_PKG_VERSION"))
}

// ---------- versions ----------

/// 1.10.0 is above 1.9.2; anything after the numbers (-beta) is ignored.
pub fn above(a: &str, b: &str) -> bool {
    let nums =
        |s: &str| -> Vec<u64> { s.split(['.', '-', '+']).take(3).map(|p| p.trim().parse().unwrap_or(0)).collect() };
    let (mut x, mut y) = (nums(a), nums(b));
    x.resize(3, 0);
    y.resize(3, 0);
    x > y
}

pub fn read_manifest(bytes: &[u8]) -> Result<Manifest, Problem> {
    let m: Manifest = serde_json::from_slice(bytes).map_err(|_| Problem::Unreadable)?;
    if m.schema != SCHEMA || m.version.trim().is_empty() {
        return Err(Problem::Unreadable);
    }
    Ok(m)
}

/// The release against what is installed (id, version). A part the release has lower is not newer; one that isn't
/// installed is offered, if it is a tool.
pub fn compare(m: &Manifest, installed: &[(String, String)]) -> (Vec<Newer>, Vec<String>) {
    let mut newer = Vec::new();
    for (id, have) in installed {
        if let Some(p) = m.components.iter().find(|p| &p.id == id) {
            if above(&p.version, have) {
                newer.push(Newer { id: id.clone(), from: have.clone(), to: p.version.clone() });
            }
        }
    }
    let offered = m.components.iter().filter(|p| !p.shared && p.id != "hub").map(|p| p.id.clone()).collect();
    (newer, offered)
}

/// A part the release has at a lower version than the one installed: never installed over it.
fn lower(m: &Manifest, installed: &[(String, String)]) -> bool {
    installed.iter().any(|(id, have)| m.components.iter().any(|p| &p.id == id && above(have, &p.version)))
}

// ---------- checking ----------

pub struct Found {
    pub manifest: Vec<u8>,
    pub status: Status,
}

pub fn status_of(manifest: &Manifest, installed: &[(String, String)], checked: u64) -> Status {
    let (newer, offered) = compare(manifest, installed);
    Status {
        state: if newer.is_empty() { "latest" } else { "newer" }.into(),
        checked,
        version: manifest.version.clone(),
        newer,
        offered,
        ..Status::default()
    }
}

/// A check that found a newer version starts its download by itself when Auto-download updates is on. Only then: never
/// while one is downloading, waiting (`ready`) or going in, never after a failed one (the next check, or Update, tries
/// again), never with nothing newer.
pub fn fetch_by_itself(s: &Status, auto: bool) -> bool {
    auto && s.state == "newer" && !s.newer.is_empty()
}

/// A check's answer while a downloaded version waits (`ready`): the same version still the newest keeps it waiting (and
/// downloaded); any other answer stands, and what waited is let go.
pub fn after_check(was: &Status, next: Status) -> Status {
    if was.state == "ready" && next.state == "newer" && was.version == next.version {
        Status { checked: next.checked, ..was.clone() }
    } else if was.state == "ready" && matches!(next.state.as_str(), "offline" | "unreadable") {
        // a check that couldn't reach the release says nothing about what is already here and checked
        Status { checked: was.checked, ..was.clone() }
    } else {
        next
    }
}

/// What a check believes, place by place, each place's release only once it is signed (`release_at`):
///
/// - something newer than what is installed: the answer, at once;
/// - nothing newer: kept, and the next place is still asked, so a place that lags (or lies about being up to date)
///   can't hide a newer signed release in the next one; with this answer in hand the next place is waited for only
///   briefly (`Source::quick`), and when it gives nothing better this is the answer (Up to date);
/// - lower than what is installed: the place lags, and is the answer only when no place has better.
///
/// When no place gives a signed release, the reason that says most, but a check never says "Verification failed" (that
/// sentence is about a downloaded file): a release that failed its signature or its sums is one that couldn't be read.
pub fn check(src: &Source, installed: &[(String, String)], now: u64) -> Result<Found, Problem> {
    let (mut kept, mut lagging, mut why) = (None::<Found>, None::<Found>, None);
    for (i, base) in src.bases.iter().enumerate() {
        let wait = if kept.is_some() { src.quick } else { src.patience(i) };
        match release_at(src, base, wait) {
            Ok(r) => {
                let found = Found { status: status_of(&r.manifest, installed, now), manifest: r.bytes };
                if lower(&r.manifest, installed) {
                    lagging.get_or_insert(found);
                } else if found.status.state == "newer" {
                    return Ok(found);
                } else {
                    kept.get_or_insert(found);
                }
            }
            Err(p) => why = Some(worse(why, p)),
        }
    }
    kept.or(lagging).ok_or(match why {
        Some(Problem::Failed) => Problem::Unreadable,
        other => other.unwrap_or(Problem::Offline),
    })
}

/// One place's release, trusted: its `SHA256SUMS.txt` signed by a trusted key, and its `manifest.json` the one that list
/// names. A check and a download both read a place through this, so a version is never believed on less than what an
/// update would be installed on.
pub struct Release {
    /// `SHA256SUMS.txt`.
    pub sums: String,
    /// `manifest.json` as it came, and read.
    pub bytes: Vec<u8>,
    pub manifest: Manifest,
}

/// The three small files of one place (a few KB), in this order: the list, its signature, the manifest. Something that
/// isn't a signature at all (a web page served with 200 for a file the place doesn't have, or no `.sig` because the
/// release was never signed) is no release (`Unreadable`); a signature by no trusted key, or a manifest the list
/// doesn't name, is `Failed`.
pub fn release_at(src: &Source, base: &str, wait: net::Patience) -> Result<Release, Problem> {
    let a = agent();
    let small = |file: &str| net::get_small(&a, &format!("{base}{file}"), &|u| src.allowed(u), wait).map_err(from_net);
    let sums = small("SHA256SUMS.txt")?;
    let sig = String::from_utf8_lossy(&small("SHA256SUMS.txt.sig")?).into_owned();
    if minisign_verify::Signature::decode(&sig).is_err() {
        return Err(Problem::Unreadable);
    }
    if !signed(&sums, &sig, &src.keys) {
        return Err(Problem::Failed);
    }
    let sums = String::from_utf8_lossy(&sums).into_owned();
    let bytes = small("manifest.json")?;
    if sum_of(&sums, "manifest.json").as_deref() != Some(sha256(&bytes).as_str()) {
        return Err(Problem::Failed);
    }
    let manifest = read_manifest(&bytes)?;
    Ok(Release { sums, bytes, manifest })
}

// ---------- the signature and the sums ----------

/// `SHA256SUMS.txt` is ours: signed by one of the trusted keys.
pub fn signed(sums: &[u8], sig: &str, keys: &[String]) -> bool {
    let Ok(sig) = minisign_verify::Signature::decode(sig) else { return false };
    keys.iter()
        .filter_map(|k| minisign_verify::PublicKey::from_base64(k).ok())
        .any(|k| k.verify(sums, &sig, false).is_ok())
}

/// The SHA-256 the list gives a file (`sha256sum`'s lines: `<hex>  <name>`, or `<hex> *<name>`).
pub fn sum_of(sums: &str, file: &str) -> Option<String> {
    sums.lines().find_map(|l| {
        let (hex, name) = l.trim().split_once(char::is_whitespace)?;
        let name = name.trim_start().trim_start_matches('*');
        (name == file && hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit()))
            .then(|| hex.to_ascii_lowercase())
    })
}

pub fn sha256(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

// ---------- getting ----------

/// The `DotAny-update` folder of the system's temporary folder (`%TEMP%` on Windows).
pub fn folder() -> PathBuf {
    std::env::temp_dir().join("DotAny-update")
}

/// Downloads, checks and returns the installer, ready to start, from the first place whose files pass (module docs).
/// `adding`: a tool not installed yet is wanted (Install), so a release at the installed versions will do; otherwise
/// (Update) the release must have something newer than what is installed. The file goes in `into` (`folder()`).
/// `progress` hears 0 to 1 (from the start again if a place fails half-way and the next one is tried).
pub fn download(
    src: &Source,
    installed: &[(String, String)],
    adding: bool,
    into: &Path,
    progress: &mut dyn FnMut(f64),
) -> Result<PathBuf, Problem> {
    let mut why = None;
    for (i, base) in src.bases.iter().enumerate() {
        match from_place(src, base, src.patience(i), installed, adding, into, progress) {
            Ok(done) => return Ok(done),
            Err(Miss::Next(p)) => why = Some(worse(why, p)),
            // this computer couldn't keep the file: no other place helps with that
            Err(Miss::Here(p)) => return Err(p),
        }
    }
    Err(why.unwrap_or(Problem::Offline))
}

/// Why one place's files weren't taken: try the next place, or stop (the trouble is on this computer).
enum Miss {
    Next(Problem),
    Here(Problem),
}

fn from_place(
    src: &Source,
    base: &str,
    wait: net::Patience,
    installed: &[(String, String)],
    adding: bool,
    dir: &Path,
    progress: &mut dyn FnMut(f64),
) -> Result<PathBuf, Miss> {
    use Miss::{Here, Next};
    let a = agent();
    let ok = |u: &Url| src.allowed(u);
    // 1. the list and its signature; 2. the manifest is the one the list names, so its versions are ours
    let Release { sums, manifest: m, .. } = release_at(src, base, wait).map_err(Next)?;
    // 3. never a step back; and an update only from a place that has one (a place that lags is passed over)
    if lower(&m, installed) || (!adding && compare(&m, installed).0.is_empty()) {
        return Err(Next(Problem::Unreadable));
    }
    let want = sum_of(&sums, SETUP).ok_or(Next(Problem::Failed))?;
    // 4. the installer itself
    std::fs::create_dir_all(dir).map_err(|_| Here(Problem::Failed))?;
    let part = dir.join(format!("{SETUP}.part"));
    let mut done = dir.join(SETUP);
    // the last update's installer may still be running; then this one gets a name of its own
    if done.exists() && std::fs::remove_file(&done).is_err() {
        done = dir.join(format!("DotAny-Setup-{}.exe", std::process::id()));
        let _ = std::fs::remove_file(&done);
    }
    let file = std::fs::File::create(&part).map_err(|_| Here(Problem::Failed))?;
    let mut out = std::io::BufWriter::new(file);
    let mut hash = Sha256::new();
    // told every 1% or 100 ms, whichever comes first, so a download that stalls still shows where it stopped
    let (mut got, mut said, mut said_pct) = (0u64, std::time::Instant::now(), 0.0f64);
    let mut wrote = true;
    // the installer itself is always waited for with `net::PATIENT`: this place has already answered with a signed list
    let fetched = net::get(&a, &format!("{base}{SETUP}"), &ok, MAX_SETUP, net::PATIENT, &mut |piece, total| {
        hash.update(piece);
        if out.write_all(piece).is_err() {
            wrote = false;
            return false;
        }
        got += piece.len() as u64;
        if let Some(t) = total.filter(|t| *t > 0) {
            let pct = (got as f64 / t as f64).min(1.0);
            if pct - said_pct >= 0.01 || said.elapsed().as_millis() >= 100 {
                (said, said_pct) = (std::time::Instant::now(), pct);
                progress(pct);
            }
        }
        true
    });
    let flushed = out.flush().is_ok() && wrote;
    drop(out);
    let gone = |e: Miss| {
        let _ = std::fs::remove_file(&part);
        e
    };
    if !flushed {
        return Err(gone(Here(Problem::Failed)));
    }
    fetched.map_err(|f| gone(Next(from_net(f))))?;
    if hex(&hash.finalize()) != want {
        return Err(gone(Next(Problem::Failed)));
    }
    progress(1.0);
    std::fs::rename(&part, &done).map_err(|_| gone(Here(Problem::Failed)))?;
    remember(&done, &want);
    Ok(done)
}

/// The installers this DotAny downloaded and checked, with the SHA-256 they passed with. A downloaded installer may wait
/// in the temporary folder for days (`ready`) before it is started, and anything running as this user can write there,
/// so it is hashed again right before it starts (`still_as_checked`). Kept in memory only: after a restart nothing
/// counts as downloaded (and nothing is `ready`), so nothing is started without being downloaded and checked again.
static CHECKED: std::sync::Mutex<Vec<(PathBuf, String)>> = std::sync::Mutex::new(Vec::new());

pub(crate) fn remember(file: &Path, sha: &str) {
    let mut list = CHECKED.lock().unwrap_or_else(|e| e.into_inner());
    list.retain(|(p, _)| p != file);
    list.push((file.to_path_buf(), sha.to_string()));
}

/// `file` is one this DotAny downloaded and checked, and it still has exactly the bytes it passed with. A file that has
/// changed since is deleted, so it is downloaded again next time.
pub fn still_as_checked(file: &Path) -> bool {
    let want =
        CHECKED.lock().unwrap_or_else(|e| e.into_inner()).iter().find(|(p, _)| p == file).map(|(_, s)| s.clone());
    let Some(want) = want else { return false };
    let mut hash = Sha256::new();
    let read = std::fs::File::open(file).and_then(|mut f| std::io::copy(&mut f, &mut hash));
    if read.is_ok() && hex(&hash.finalize()) == want {
        return true;
    }
    CHECKED.lock().unwrap_or_else(|e| e.into_inner()).retain(|(p, _)| p != file);
    let _ = std::fs::remove_file(file);
    false
}

/// Starts the installer in its update mode: the install in `root`, plus `add` (a tool not installed yet). Only an
/// installer that is still the one downloaded and checked (`still_as_checked`).
pub fn launch(setup: &Path, root: &Path, add: &[String]) -> bool {
    if !still_as_checked(setup) {
        return false;
    }
    let mut args: Vec<String> = vec!["--update".into(), "--root".into(), root.display().to_string()];
    if !add.is_empty() {
        args.push("--tools".into());
        args.push(add.join(","));
    }
    if let Ok(log) = std::env::var("DOTANY_UPDATE_LAUNCHED") {
        let _ = std::fs::write(
            log,
            serde_json::to_vec(&serde_json::json!({ "program": setup, "args": args })).unwrap_or_default(),
        );
    }
    // let go of DotAny's standard handles: the installer outlives this DotAny, which it ends and opens again
    use std::process::Stdio;
    std::process::Command::new(setup)
        .args(&args)
        .current_dir(setup.parent().unwrap_or(Path::new(".")))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .is_ok()
}

/// The Mac's update: the package (`DotAny-mac.zip`, the DotAny folder of .apps) is downloaded and checked in the
/// background like Windows' installer (`download`), then waits as `ready` until the person says Update; then each .app
/// that is in DotAny's folder now is replaced by the package's, DotAny itself always, and DotAny opens again. A tool
/// the person dragged to the Trash stays away: the Mac has no installer that chooses tools, so the update brings what
/// is there up to date and nothing more.
///
/// Replacing: the package is unpacked by the system's `ditto` into a folder beside DotAny's (the same disk, so every
/// move is a rename), each new .app must say it is one of ours (`com.dotany.`), and the old ones are moved aside, not
/// deleted, until every new one is in place; any step that fails puts back what was moved.
///
/// **Open programs**: before anything is moved, the programs running from the .apps that would be replaced are asked
/// for (`open`: the system's list of running applications, by each one's bundle URL); the hub's own .app doesn't count,
/// as it opens again by itself. Those are exactly the ones whose .app the package also carries: a tool in the folder that
/// the package hasn't stays as it is, running or not, and a tool not in the folder isn't brought. When any is open,
/// nothing is replaced and nothing is quit (`Problem::Open` with their names): the package stays downloaded and checked,
/// and the page asks the person to close them first, as Windows' installer does in its update mode.
///
/// This relies on two behaviours of the system that only a signed build shows: a Developer ID signed and notarised
/// .app put in place this way opens without the system asking (it does for apps Sparkle replaces the same way), and
/// a login item whose .app was replaced keeps working (the system follows the bundle's identifier).
#[cfg(target_os = "macos")]
pub mod mac {
    use super::Problem;
    use std::path::{Path, PathBuf};

    /// The .apps the package holds, at its top or one folder down (`DotAny/`), by name.
    fn apps_in(dir: &Path) -> Vec<PathBuf> {
        let list = |d: &Path| -> Vec<PathBuf> {
            std::fs::read_dir(d)
                .map(|r| r.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "app")).collect())
                .unwrap_or_default()
        };
        let mut apps = list(dir);
        if apps.is_empty() {
            for sub in std::fs::read_dir(dir).map(|r| r.flatten().map(|e| e.path()).collect()).unwrap_or(Vec::new()) {
                if sub.is_dir() && !sub.is_symlink() {
                    apps.extend(list(&sub));
                }
            }
        }
        apps.sort();
        apps
    }

    /// One of ours: a real folder (not a link) whose Info.plist names a `com.dotany.` identifier.
    fn ours(app: &Path) -> bool {
        !app.is_symlink()
            && std::fs::read_to_string(app.join("Contents").join("Info.plist"))
                .ok()
                .and_then(|p| dotany_base::quickaction::plist_string(&p, "CFBundleIdentifier"))
                .is_some_and(|id| id.starts_with("com.dotany."))
    }

    /// Which of these .apps (in DotAny's folder) have a program running from them now: their names.
    pub type Open<'a> = &'a dyn Fn(&[PathBuf]) -> Vec<String>;

    /// Puts the package's .apps in `root` (DotAny's folder): those already there, and `hub` (DotAny's own, by name)
    /// always. Answers the names put in place; `Problem::Open` when a program from one of them other than `hub` runs.
    pub fn install(package: &Path, root: &Path, hub: &str, open: Open<'_>) -> Result<Vec<String>, Problem> {
        // the package may have waited days since it was checked
        if !super::still_as_checked(package) {
            return Err(Problem::Failed);
        }
        let work = root.join(format!(".DotAny-update-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&work);
        let (fresh, aside) = (work.join("new"), work.join("old"));
        std::fs::create_dir_all(&fresh).map_err(|_| Problem::Failed)?;
        std::fs::create_dir_all(&aside).map_err(|_| Problem::Failed)?;
        let done = place(package, root, hub, &fresh, &aside, open);
        let _ = std::fs::remove_dir_all(&work);
        done
    }

    fn place(
        package: &Path,
        root: &Path,
        hub: &str,
        fresh: &Path,
        aside: &Path,
        open: Open<'_>,
    ) -> Result<Vec<String>, Problem> {
        let unpacked = std::process::Command::new("/usr/bin/ditto")
            .args(["-x", "-k"])
            .arg(package)
            .arg(fresh)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if !unpacked {
            return Err(Problem::Failed);
        }
        let apps = apps_in(fresh);
        if apps.is_empty() || !apps.iter().all(|a| ours(a)) {
            return Err(Problem::Failed);
        }
        let name = |p: &Path| p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        if !apps.iter().any(|a| name(a) == hub) {
            return Err(Problem::Failed);
        }
        let wanted: Vec<&PathBuf> = apps.iter().filter(|a| name(a) == hub || root.join(name(a)).exists()).collect();
        // a program running from one of those: nothing moves until the person has closed it
        let tools: Vec<PathBuf> = wanted.iter().filter(|a| name(a) != hub).map(|a| root.join(name(a))).collect();
        let running = open(&tools);
        if !running.is_empty() {
            return Err(Problem::Open(running));
        }
        // each step, so a failure puts every one back: (where it is now, the old one moved aside, if any)
        let mut moved: Vec<(PathBuf, Option<PathBuf>)> = Vec::new();
        let undo = |moved: &[(PathBuf, Option<PathBuf>)], from: &Path| {
            for (at, old) in moved.iter().rev() {
                let _ = std::fs::rename(at, from.join(at.file_name().unwrap_or_default()));
                if let Some(old) = old {
                    let _ = std::fs::rename(old, at);
                }
            }
        };
        for app in wanted {
            let at = root.join(name(app));
            let old = if at.exists() || at.is_symlink() {
                let to = aside.join(name(app));
                if std::fs::rename(&at, &to).is_err() {
                    undo(&moved, fresh);
                    return Err(Problem::Failed);
                }
                Some(to)
            } else {
                None
            };
            if std::fs::rename(app, &at).is_err() {
                if let Some(old) = &old {
                    let _ = std::fs::rename(old, &at);
                }
                undo(&moved, fresh);
                return Err(Problem::Failed);
            }
            moved.push((at, old));
        }
        Ok(moved.iter().map(|(at, _)| name(at)).collect())
    }

    /// DotAny opens again from `app` once this one has ended (the caller ends it next). A test's
    /// `DOTANY_UPDATE_LAUNCHED` file gets what would be opened, and the test opens it itself: the system's `open` would
    /// start it without the test's settings (its own data folder, no test driver), as the person's DotAny.
    pub fn reopen(app: &Path) -> bool {
        if let Ok(log) = std::env::var("DOTANY_UPDATE_LAUNCHED") {
            return std::fs::write(
                log,
                serde_json::to_vec(&serde_json::json!({ "program": app, "args": ["reopen"] })).unwrap_or_default(),
            )
            .is_ok();
        }
        use std::process::Stdio;
        std::process::Command::new("/bin/sh")
            .arg("-c")
            .arg("while /bin/kill -0 \"$1\" 2>/dev/null; do /bin/sleep 0.2; done; exec /usr/bin/open \"$2\"")
            .arg("sh")
            .arg(std::process::id().to_string())
            .arg(app)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .is_ok()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        /// A folder of its own for one test, gone when it ends.
        struct Scratch(PathBuf);
        impl Scratch {
            fn new(name: &str) -> Scratch {
                let d = std::env::temp_dir().join(format!("dotany-hub-update-{name}-{}", std::process::id()));
                let _ = std::fs::remove_dir_all(&d);
                std::fs::create_dir_all(&d).unwrap();
                Scratch(d)
            }
        }
        impl Drop for Scratch {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }

        /// The least .app this needs: an Info.plist with an identifier and a version, and a program.
        fn app(dir: &Path, name: &str, id: &str, version: &str) {
            let c = dir.join(format!("{name}.app")).join("Contents");
            std::fs::create_dir_all(c.join("MacOS")).unwrap();
            std::fs::write(
                c.join("Info.plist"),
                format!(
                    "<plist><dict><key>CFBundleIdentifier</key><string>{id}</string>\
                     <key>CFBundleShortVersionString</key><string>{version}</string></dict></plist>"
                ),
            )
            .unwrap();
            std::fs::write(c.join("MacOS").join("run"), format!("#!/bin/sh\necho {version}\n")).unwrap();
            // a link inside, as a framework has: it must come through as a link
            std::os::unix::fs::symlink("MacOS/run", c.join("current")).unwrap();
        }

        fn version(root: &Path, name: &str) -> String {
            let p = std::fs::read_to_string(root.join(format!("{name}.app/Contents/Info.plist"))).unwrap();
            dotany_base::quickaction::plist_string(&p, "CFBundleShortVersionString").unwrap()
        }

        /// Nothing runs.
        fn none(_: &[PathBuf]) -> Vec<String> {
            Vec::new()
        }

        fn zip(from: &Path, to: &Path) {
            assert!(
                std::process::Command::new("/usr/bin/ditto")
                    .args(["-c", "-k", "--keepParent"])
                    .arg(from)
                    .arg(to)
                    .status()
                    .unwrap()
                    .success()
            );
            // as if downloaded and checked: install takes only such a package
            crate::update::remember(to, &crate::update::sha256(&std::fs::read(to).unwrap()));
        }

        #[test]
        fn the_apps_there_are_replaced_and_one_taken_away_stays_away() {
            let s = Scratch::new("place");
            let root = s.0.join("DotAny");
            std::fs::create_dir_all(&root).unwrap();
            app(&root, "DotAny", "com.dotany.hub", "1.0.0");
            app(&root, "DotAny Text", "com.dotany.text", "1.0.0");
            std::fs::write(root.join("notes.txt"), "the person's own").unwrap();
            let release = s.0.join("release").join("DotAny");
            std::fs::create_dir_all(&release).unwrap();
            for (n, id) in
                [("DotAny", "com.dotany.hub"), ("DotAny Text", "com.dotany.text"), ("DotAny Image", "com.dotany.image")]
            {
                app(&release, n, id, "1.1.0");
            }
            let package = s.0.join("DotAny-mac.zip");
            zip(&release, &package);
            let placed = install(&package, &root, "DotAny.app", &none).unwrap();
            assert_eq!(placed, ["DotAny Text.app", "DotAny.app"]);
            assert_eq!(version(&root, "DotAny"), "1.1.0");
            assert_eq!(version(&root, "DotAny Text"), "1.1.0");
            assert!(!root.join("DotAny Image.app").exists(), "a tool that wasn't there isn't brought");
            assert_eq!(std::fs::read_to_string(root.join("notes.txt")).unwrap(), "the person's own");
            let link = root.join("DotAny.app/Contents/current");
            assert!(link.is_symlink(), "links inside an .app stay links");
            assert_eq!(std::fs::read_link(&link).unwrap(), Path::new("MacOS/run"));
            let names: Vec<String> =
                std::fs::read_dir(&root).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into()).collect();
            assert!(!names.iter().any(|n| n.starts_with(".DotAny-update")), "nothing left beside: {names:?}");
        }

        #[test]
        fn an_open_tool_that_would_be_replaced_stops_it_before_anything_moves() {
            let s = Scratch::new("open");
            let root = s.0.join("DotAny");
            std::fs::create_dir_all(&root).unwrap();
            app(&root, "DotAny", "com.dotany.hub", "1.0.0");
            app(&root, "DotAny Text", "com.dotany.text", "1.0.0");
            // in the folder, but not in the release: left as it is, so it doesn't count
            app(&root, "DotAny Video", "com.dotany.video", "1.0.0");
            let release = s.0.join("release").join("DotAny");
            std::fs::create_dir_all(&release).unwrap();
            for (n, id) in
                [("DotAny", "com.dotany.hub"), ("DotAny Text", "com.dotany.text"), ("DotAny Image", "com.dotany.image")]
            {
                app(&release, n, id, "1.1.0");
            }
            let package = s.0.join("DotAny-mac.zip");
            zip(&release, &package);
            let asked = std::cell::RefCell::new(Vec::new());
            let text_open = |apps: &[PathBuf]| {
                asked.borrow_mut().extend(apps.iter().cloned());
                apps.iter().filter(|a| a.ends_with("DotAny Text.app")).map(|_| "DotAny Text".to_string()).collect()
            };
            let r = install(&package, &root, "DotAny.app", &text_open);
            assert_eq!(r, Err(Problem::Open(vec!["DotAny Text".into()])));
            assert_eq!(
                *asked.borrow(),
                [root.join("DotAny Text.app")],
                "only what would be replaced, not DotAny itself"
            );
            assert_eq!(version(&root, "DotAny"), "1.0.0");
            assert_eq!(version(&root, "DotAny Text"), "1.0.0");
            assert!(!root.join("DotAny Image.app").exists());
            assert_eq!(std::fs::read_dir(&root).unwrap().count(), 3, "nothing left beside");
            assert!(package.exists(), "the package itself stays, for the next 更新");
            // closed: the same package goes in
            let placed = install(&package, &root, "DotAny.app", &none).unwrap();
            assert_eq!(placed, ["DotAny Text.app", "DotAny.app"]);
            assert_eq!(version(&root, "DotAny Text"), "1.1.0");
        }

        #[test]
        fn a_package_that_isnt_ours_changes_nothing() {
            let s = Scratch::new("refuse");
            let root = s.0.join("DotAny");
            std::fs::create_dir_all(&root).unwrap();
            app(&root, "DotAny", "com.dotany.hub", "1.0.0");
            app(&root, "DotAny Text", "com.dotany.text", "1.0.0");
            // one stranger among ours
            let release = s.0.join("release");
            std::fs::create_dir_all(&release).unwrap();
            app(&release, "DotAny", "com.dotany.hub", "1.1.0");
            app(&release, "DotAny Text", "com.example.text", "6.6.6");
            let package = s.0.join("bad.zip");
            zip(&release, &package);
            assert_eq!(install(&package, &root, "DotAny.app", &none), Err(Problem::Failed));
            // and one without DotAny itself, and one that isn't a zip at all
            let release2 = s.0.join("release2");
            std::fs::create_dir_all(&release2).unwrap();
            app(&release2, "DotAny Text", "com.dotany.text", "1.1.0");
            let package2 = s.0.join("nohub.zip");
            zip(&release2, &package2);
            assert_eq!(install(&package2, &root, "DotAny.app", &none), Err(Problem::Failed));
            let junk = s.0.join("junk.zip");
            std::fs::write(&junk, b"not a zip").unwrap();
            assert_eq!(install(&junk, &root, "DotAny.app", &none), Err(Problem::Failed));
            assert_eq!(version(&root, "DotAny"), "1.0.0");
            assert_eq!(version(&root, "DotAny Text"), "1.0.0");
            assert_eq!(std::fs::read_dir(&root).unwrap().count(), 2, "nothing left beside");
        }

        #[test]
        fn a_step_that_fails_puts_back_what_was_moved() {
            let s = Scratch::new("undo");
            let root = s.0.join("DotAny");
            std::fs::create_dir_all(&root).unwrap();
            app(&root, "DotAny", "com.dotany.hub", "1.0.0");
            app(&root, "DotAny Text", "com.dotany.text", "1.0.0");
            let release = s.0.join("release");
            std::fs::create_dir_all(&release).unwrap();
            app(&release, "DotAny", "com.dotany.hub", "1.1.0");
            app(&release, "DotAny Text", "com.dotany.text", "1.1.0");
            let package = s.0.join("DotAny-mac.zip");
            zip(&release, &package);
            // DotAny.app (second in order) can't be moved aside: its folder may not be renamed out of a locked parent;
            // a stand-in: DotAny.app made a file the rename can't replace a folder with
            let fresh = s.0.join("fresh");
            let aside = s.0.join("aside");
            std::fs::create_dir_all(&fresh).unwrap();
            std::fs::create_dir_all(aside.join("DotAny.app").join("x")).unwrap();
            let r = place(&package, &root, "DotAny.app", &fresh, &aside, &none);
            assert_eq!(r, Err(Problem::Failed));
            assert_eq!(version(&root, "DotAny"), "1.0.0");
            assert_eq!(version(&root, "DotAny Text"), "1.0.0", "the one already replaced is put back");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RELEASE: &str = r#"{"schema":1,"product":"DotAny","version":"1.1.0","built":"x","components":[
        {"id":"fonts","shared":true,"files":[]},{"id":"hub","version":"1.1.0","files":[]},{"id":"text","version":"1.0.0","files":[]},
        {"id":"image","version":"1.2.0","files":[]},{"id":"video","version":"1.0.0","files":[]}],"files":[]}"#;

    #[test]
    fn versions() {
        assert!(above("1.10.0", "1.9.2"));
        assert!(above("1.0.1", "1.0"));
        assert!(!above("1.0.0", "1.0.0"));
        assert!(!above("0.9.9", "1.0.0"));
        assert!(!above("1.0.0-beta", "1.0.0"));
    }

    #[test]
    fn what_is_newer_and_what_is_offered() {
        let m = read_manifest(RELEASE.as_bytes()).unwrap();
        let installed = vec![
            ("hub".to_string(), "1.0.0".to_string()),
            ("text".to_string(), "1.0.0".to_string()),
            ("image".to_string(), "1.0.0".to_string()),
        ];
        let (newer, offered) = compare(&m, &installed);
        assert_eq!(
            newer.iter().map(|n| (n.id.as_str(), n.to.as_str())).collect::<Vec<_>>(),
            vec![("hub", "1.1.0"), ("image", "1.2.0")]
        );
        assert_eq!(offered, vec!["text", "image", "video"]);
        assert!(!lower(&m, &installed));
        assert!(lower(&m, &[("text".to_string(), "2.0.0".to_string())]));
        assert_eq!(status_of(&m, &installed, 5).state, "newer");
        assert_eq!(
            read_manifest(RELEASE.replace("\"schema\":1", "\"schema\":2").as_bytes()).err(),
            Some(Problem::Unreadable)
        );
        assert_eq!(read_manifest(b"<html>").err(), Some(Problem::Unreadable));
    }

    #[test]
    fn a_newer_version_downloads_by_itself_only_when_asked_to_and_only_once() {
        let m = read_manifest(RELEASE.as_bytes()).unwrap();
        let installed = vec![("hub".to_string(), "1.0.0".to_string())];
        let newer = status_of(&m, &installed, 5);
        assert!(fetch_by_itself(&newer, true));
        assert!(!fetch_by_itself(&newer, false), "自动下载更新 off: a check only checks");
        let latest = status_of(&m, &[("hub".to_string(), "1.1.0".to_string())], 5);
        assert_eq!(latest.state, "latest");
        assert!(!fetch_by_itself(&latest, true));
        for busy in ["checking", "downloading", "ready", "installing", "failed", "offline", "unreadable", "idle"] {
            assert!(!fetch_by_itself(&Status { state: busy.into(), ..newer.clone() }, true), "{busy}");
        }
    }

    #[test]
    fn what_waits_downloaded_keeps_waiting_while_it_is_the_newest() {
        let m = read_manifest(RELEASE.as_bytes()).unwrap();
        let installed = vec![("hub".to_string(), "1.0.0".to_string())];
        let ready =
            Status { state: "ready".into(), pct: 1.0, parts: vec!["hub".into()], ..status_of(&m, &installed, 5) };
        // the same release again: still ready, with the new check's time
        let again = after_check(&ready, status_of(&m, &installed, 9));
        assert_eq!((again.state.as_str(), again.checked, again.pct), ("ready", 9, 1.0));
        // no answer: nothing about what is here changes
        let off = after_check(&ready, Status { state: "offline".into(), ..ready.clone() });
        assert_eq!(off.state, "ready");
        // a newer release than the one waiting: that one is newer now, and what waited goes
        let m2 = read_manifest(
            RELEASE.replace("\"version\":\"1.1.0\",\"built\"", "\"version\":\"1.2.0\",\"built\"").as_bytes(),
        )
        .unwrap();
        let moved = after_check(&ready, status_of(&m2, &installed, 9));
        assert_eq!((moved.state.as_str(), moved.version.as_str()), ("newer", "1.2.0"));
        // installed meanwhile some other way: the latest
        let done = after_check(&ready, status_of(&m, &[("hub".to_string(), "1.1.0".to_string())], 9));
        assert_eq!(done.state, "latest");
        // without anything waiting, a check's answer is the answer
        let plain = status_of(&m, &installed, 9);
        assert_eq!(after_check(&status_of(&m, &installed, 5), plain.clone()), plain);
    }

    #[test]
    fn sums() {
        let list = "aa  nope\n0123456789abcdef0123456789abcdef0123456789abcdef0123456789ABCDEF  DotAny-Setup.exe\n1111111111111111111111111111111111111111111111111111111111111111 *manifest.json\n";
        assert_eq!(
            sum_of(list, "DotAny-Setup.exe").as_deref(),
            Some("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")
        );
        assert_eq!(
            sum_of(list, "manifest.json").as_deref(),
            Some("1111111111111111111111111111111111111111111111111111111111111111")
        );
        assert_eq!(sum_of(list, "nope"), None);
        assert_eq!(sha256(b"abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    // made by .github/scripts/sign-update.mjs with a throwaway key, so the two sides are known to agree
    const TEST_KEY: &str = "RWRZAus8cWHG4CIeyp7kxHY1OGwoqLSOGTYwDmbDlTQokLFqwHKKWikY";
    const TEST_SIG: &str = "untrusted comment: DotAny update signature
RURZAus8cWHG4Gb4JG0JI2ASj4xvu0OOuYXuPuAS0d2vs0vW8b4+kSx/BBB1k5fLM4GdcMZ424jgfqbDuwCzYQeM4szGd2vUKwY=
trusted comment: DotAny test
5KoU2DSq5fr03rQIQOMPUWJvg+qr0qCHjJA+ym8tFgdx/WEO77si58o2WgpNHrZRDsSS//44r4SjzIlv6fmKDA==
";

    #[test]
    fn signatures() {
        let keys = vec![TRUSTED_KEYS[0].to_string(), TEST_KEY.to_string()];
        assert!(signed(b"hello\n", TEST_SIG, &keys));
        assert!(!signed(b"hello!\n", TEST_SIG, &keys));
        assert!(!signed(b"hello\n", TEST_SIG, &[TRUSTED_KEYS[0].to_string()]));
        assert!(!signed(b"hello\n", &TEST_SIG.replace("DotAny test", "DotAny tost"), &keys));
        assert!(!signed(b"hello\n", "garbage", &keys));
        for k in TRUSTED_KEYS {
            assert!(minisign_verify::PublicKey::from_base64(k).is_ok());
        }
    }

    #[test]
    fn only_the_places_or_a_local_test() {
        let real = Source {
            bases: PLACES.iter().map(|p| p.to_string()).collect(),
            local: false,
            keys: vec![],
            quick: net::QUICK,
        };
        let ok = |s: &Source, u: &str| s.allowed(&net::parse(u).unwrap());
        assert!(ok(&real, "https://github.com/x"));
        assert!(ok(&real, "https://objects.githubusercontent.com/x"));
        assert!(ok(&real, "https://dotany.app/releases/latest/manifest.json"));
        assert!(!ok(&real, "http://github.com/x"));
        assert!(!ok(&real, "http://dotany.app/x"), "https only, the site too");
        assert!(!ok(&real, "https://github.com.evil.example/x"));
        assert!(!ok(&real, "https://evilgithubusercontent.com/x"));
        assert!(!ok(&real, "https://dotany.app.evil.example/x"));
        assert!(!ok(&real, "https://evildotany.app/x"));
        assert!(!ok(&real, "https://files.dotany.app/x"), "the site's host and nothing wider");
        assert!(!ok(&real, "http://127.0.0.1:9/x"));
        let test = Source { bases: vec!["http://127.0.0.1:9/".into()], local: true, keys: vec![], quick: net::QUICK };
        assert!(ok(&test, "http://127.0.0.1:9/x"));
        assert!(!ok(&test, "http://example.com/x"));
        // the site first, then GitHub; the last one is waited for patiently
        assert_eq!(PLACES, &[SITE, LATEST]);
        assert_eq!((real.patience(0), real.patience(1)), (net::QUICK, net::PATIENT));
        assert_eq!(test.patience(0), net::PATIENT, "one place alone is waited for patiently");
    }

    #[test]
    fn a_test_release_is_one_place_or_several_all_on_this_computer() {
        assert_eq!(local_bases("http://127.0.0.1:5/x"), Some(vec!["http://127.0.0.1:5/x/".to_string()]));
        assert_eq!(
            local_bases("http://127.0.0.1:5/a/, http://localhost:6/b/"),
            Some(vec!["http://127.0.0.1:5/a/".to_string(), "http://localhost:6/b/".to_string()])
        );
        assert_eq!(local_bases("http://127.0.0.1:5/a/,https://dotany.app/releases/latest/"), None);
        assert_eq!(local_bases("https://example.com/"), None);
        assert_eq!(local_bases(" , "), None);
    }

    #[test]
    fn the_reason_that_says_most() {
        assert_eq!(worse(None, Problem::Offline), Problem::Offline);
        assert_eq!(worse(Some(Problem::Offline), Problem::Unreadable), Problem::Unreadable);
        assert_eq!(worse(Some(Problem::Failed), Problem::Offline), Problem::Failed);
        assert_eq!(worse(Some(Problem::Unreadable), Problem::Failed), Problem::Failed);
    }

    /// Several places, each a release served on this computer. Net only fetches on Windows and the Mac.
    #[cfg(any(windows, target_os = "macos"))]
    mod places {
        use super::super::*;
        use std::io::{Read, Write};
        use std::sync::{Arc, Mutex};
        use std::time::{Duration, Instant};

        // SHA256SUMS.txt of `release(v)`, signed by .github/scripts/sign-update.mjs with a throwaway key (KEY); BAD is
        // 1.1.0's signed by another throwaway key
        const KEY: &str = "RWQv/2d9TcL09+dRSF5BQt7/TZqHccYqwVfiMmDR8YuYfdbCy6EVCkoY";
        const SIG_100: &str = "untrusted comment: DotAny update signature
RUQv/2d9TcL090MPattPZFP8WqdSulduSKGz2QQ4PKorz2NdGngc2h+jwvWn7ZbSd+w0+Ubm09hSMuBLxUaUQ6jYUM6u6rHOYgs=
trusted comment: DotAny test
yn78XL8EOkC8cIv2jDC1xeh7wGyWZZyUaPrQfvPVf0nOaIcSawiHgwLl435ZP3A26PA8J9l/j424GDJAowG4Dg==
";
        const SIG_110: &str = "untrusted comment: DotAny update signature
RUQv/2d9TcL099eqOz6hhg+3cjR+GOE9/QZCCN7srmzSYR62/0JATo4IxD0SIkUx62vr5Rd5Q7O2Qjt9pOYR0DO1JoaHgwzZJgU=
trusted comment: DotAny test
sSDRkecK1421E8o0iqHhbQk9QqZARCfePIcDzu2/XM4bZgAU0DXCBx2j1gkJu9L9oKCSc+WPy2gQmA9QP4mzBw==
";
        const SIG_BAD: &str = "untrusted comment: DotAny update signature
RURCRmAAtNRA/PSMHVp1XpMUJPBLrd+ZoGjwpX+q85KF2PZ0ZQEDkU77PJ+3jj1lYl2wL19OSPCORbf17jG5d7IVoOkARK2foQ8=
trusted comment: DotAny test
CO2AuyRPlMoqNKqDA3oxu/MqM/IinURm4M/sjsq0Ox2crzo8y09LDXZLyHI7I60bT5l2Q+uqupyg6pk1MQDeDA==
";

        fn manifest(v: &str) -> Vec<u8> {
            format!(
                r#"{{"schema":1,"product":"DotAny","version":"{v}","built":"x","components":[{{"id":"hub","version":"{v}","files":[]}},{{"id":"text","version":"1.0.0","files":[]}}],"files":[]}}"#
            )
            .into_bytes()
        }

        fn setup(v: &str) -> Vec<u8> {
            format!("DotAny {v} installer").into_bytes()
        }

        type Files = Vec<(&'static str, Vec<u8>)>;

        /// A whole release, its list signed by `sig`, the installer under both systems' names.
        fn release(v: &str, sig: &str) -> Files {
            let (m, s) = (manifest(v), setup(v));
            let sums = format!(
                "{}  DotAny-Setup.exe\n{}  DotAny-mac.zip\n{}  manifest.json\n",
                sha256(&s),
                sha256(&s),
                sha256(&m)
            );
            vec![
                ("manifest.json", m),
                ("SHA256SUMS.txt", sums.into_bytes()),
                ("SHA256SUMS.txt.sig", sig.as_bytes().to_vec()),
                ("DotAny-Setup.exe", s.clone()),
                ("DotAny-mac.zip", s),
            ]
        }

        /// One place: its address, and the files asked of it.
        struct Place {
            base: String,
            hits: Arc<Mutex<Vec<String>>>,
        }

        impl Place {
            fn asked(&self) -> Vec<String> {
                self.hits.lock().unwrap().clone()
            }
        }

        /// A place serving `files` by name (anything else: 404); `silent`: it takes the connection and never answers.
        fn serve(files: Files, silent: bool) -> Place {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let base = format!("http://127.0.0.1:{}/", listener.local_addr().unwrap().port());
            let hits = Arc::new(Mutex::new(Vec::new()));
            let (files, heard) = (Arc::new(files), hits.clone());
            std::thread::spawn(move || {
                for conn in listener.incoming().flatten() {
                    let (files, heard) = (files.clone(), heard.clone());
                    std::thread::spawn(move || {
                        let mut conn = conn;
                        let (mut head, mut buf) = (Vec::new(), [0u8; 1024]);
                        while !head.windows(4).any(|w| w == b"\r\n\r\n") {
                            match conn.read(&mut buf) {
                                Ok(0) | Err(_) => return,
                                Ok(n) => head.extend_from_slice(&buf[..n]),
                            }
                        }
                        let line = String::from_utf8_lossy(&head).into_owned();
                        let name = line.split_whitespace().nth(1).unwrap_or("/").trim_start_matches('/').to_string();
                        heard.lock().unwrap().push(name.clone());
                        if silent {
                            std::thread::sleep(Duration::from_secs(20));
                            return;
                        }
                        let _ = match files.iter().find(|(n, _)| *n == name) {
                            Some((_, body)) => write!(
                                conn,
                                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                                body.len()
                            )
                            .and_then(|_| conn.write_all(body)),
                            None => conn
                                .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"),
                        };
                    });
                }
            });
            Place { base, hits }
        }

        /// A place where nothing answers.
        fn down() -> Place {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let base = format!("http://127.0.0.1:{}/", listener.local_addr().unwrap().port());
            drop(listener);
            Place { base, hits: Arc::new(Mutex::new(Vec::new())) }
        }

        fn places(list: &[&Place]) -> Source {
            Source {
                bases: list.iter().map(|p| p.base.clone()).collect(),
                local: true,
                keys: vec![KEY.into()],
                quick: net::Patience { connect_ms: 1000, idle_ms: 1000 },
            }
        }

        fn have(v: &str) -> Vec<(String, String)> {
            vec![("hub".to_string(), v.to_string()), ("text".to_string(), "1.0.0".to_string())]
        }

        /// A folder of its own for one test, gone when it ends.
        struct Scratch(PathBuf);
        impl Scratch {
            fn new(name: &str) -> Scratch {
                let d = std::env::temp_dir().join(format!("dotany-hub-places-{name}-{}", std::process::id()));
                let _ = std::fs::remove_dir_all(&d);
                Scratch(d)
            }
            fn left(&self) -> Vec<String> {
                std::fs::read_dir(&self.0)
                    .map(|r| r.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
                    .unwrap_or_default()
            }
        }
        impl Drop for Scratch {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }

        fn get(src: &Source, installed: &[(String, String)], adding: bool, dir: &Scratch) -> Result<Vec<u8>, Problem> {
            download(src, installed, adding, &dir.0, &mut |_| {}).map(|p| std::fs::read(p).unwrap())
        }

        #[test]
        fn the_first_place_down_the_second_is_used() {
            let (site, github) = (down(), serve(release("1.1.0", SIG_110), false));
            let src = places(&[&site, &github]);
            let found = check(&src, &have("1.0.0"), 7).unwrap();
            assert_eq!((found.status.state.as_str(), found.status.version.as_str()), ("newer", "1.1.0"));
            assert_eq!(found.manifest, manifest("1.1.0"));
            let dir = Scratch::new("down");
            assert_eq!(get(&src, &have("1.0.0"), false, &dir), Ok(setup("1.1.0")));
            assert_eq!(dir.left(), [SETUP], "the installer, and nothing half-written");
            // the second answers, so it was asked for every file; nothing came from the first
            // (the check reads the same three small files the download does, and believes them only signed)
            assert_eq!(
                github.asked(),
                [
                    "SHA256SUMS.txt",
                    "SHA256SUMS.txt.sig",
                    "manifest.json",
                    "SHA256SUMS.txt",
                    "SHA256SUMS.txt.sig",
                    "manifest.json",
                    SETUP
                ]
            );
        }

        #[test]
        fn both_places_down_is_no_network_as_with_one() {
            let src = places(&[&down(), &down()]);
            assert_eq!(check(&src, &have("1.0.0"), 7).err(), Some(Problem::Offline));
            let dir = Scratch::new("both");
            assert_eq!(get(&src, &have("1.0.0"), false, &dir), Err(Problem::Offline));
            assert!(dir.left().is_empty());
            // a place that answers only 404 (a site that carries no releases): an answer that isn't a release
            let empty = serve(vec![], false);
            let src = places(&[&empty, &down()]);
            assert_eq!(check(&src, &have("1.0.0"), 7).err(), Some(Problem::Unreadable));
        }

        #[test]
        fn a_bad_signature_is_never_taken_and_the_next_place_is() {
            let (site, github) = (serve(release("1.1.0", SIG_BAD), false), serve(release("1.1.0", SIG_110), false));
            let src = places(&[&site, &github]);
            let dir = Scratch::new("badsig");
            assert_eq!(get(&src, &have("1.0.0"), false, &dir), Ok(setup("1.1.0")));
            assert_eq!(site.asked(), ["SHA256SUMS.txt", "SHA256SUMS.txt.sig"], "stopped at the signature");
            assert!(github.asked().contains(&SETUP.to_string()));
            // with no good place after it: failed, even when the next one doesn't answer at all
            let dir = Scratch::new("badsig-alone");
            assert_eq!(get(&places(&[&site]), &have("1.0.0"), false, &dir), Err(Problem::Failed));
            assert_eq!(get(&places(&[&site, &down()]), &have("1.0.0"), false, &dir), Err(Problem::Failed));
            assert!(dir.left().is_empty());
        }

        #[test]
        fn an_installer_that_isnt_the_listed_one_is_thrown_away_and_the_next_place_is_used() {
            let mut tampered = release("1.1.0", SIG_110);
            for f in tampered.iter_mut().filter(|(n, _)| n.starts_with("DotAny-")) {
                f.1 = b"something else".to_vec();
            }
            let (site, github) = (serve(tampered, false), serve(release("1.1.0", SIG_110), false));
            let dir = Scratch::new("tampered");
            assert_eq!(get(&places(&[&site, &github]), &have("1.0.0"), false, &dir), Ok(setup("1.1.0")));
            assert!(site.asked().contains(&SETUP.to_string()), "the first place's was downloaded, and refused");
            assert_eq!(dir.left(), [SETUP]);
            // a manifest that isn't the listed one: the same
            let mut other = release("1.1.0", SIG_110);
            other[0].1 = manifest("9.0.0");
            let site = serve(other, false);
            assert_eq!(get(&places(&[&site, &github]), &have("1.0.0"), false, &dir), Ok(setup("1.1.0")));
            assert!(!site.asked().contains(&SETUP.to_string()));
        }

        #[test]
        fn a_place_older_than_what_is_installed_is_passed_over() {
            let (site, github) = (serve(release("1.0.0", SIG_100), false), serve(release("1.1.0", SIG_110), false));
            let src = places(&[&site, &github]);
            // 1.0.5 is installed: the site's 1.0.0 lags, GitHub's 1.1.0 is the answer
            let found = check(&src, &have("1.0.5"), 7).unwrap();
            assert_eq!((found.status.state.as_str(), found.status.version.as_str()), ("newer", "1.1.0"));
            let dir = Scratch::new("older");
            assert_eq!(get(&src, &have("1.0.5"), false, &dir), Ok(setup("1.1.0")));
            assert!(!site.asked().contains(&SETUP.to_string()), "never a step back");
            // with GitHub out of reach: the site's answer stands for the check (nothing newer), and nothing is installed
            let src = places(&[&site, &down()]);
            let found = check(&src, &have("1.0.5"), 7).unwrap();
            assert_eq!((found.status.state.as_str(), found.status.version.as_str()), ("latest", "1.0.0"));
            assert_eq!(get(&src, &have("1.0.5"), false, &dir), Err(Problem::Unreadable));
            assert_eq!(get(&src, &have("1.0.5"), true, &dir), Err(Problem::Unreadable), "安装 too");
        }

        #[test]
        fn a_site_at_the_installed_version_cannot_hide_a_newer_signed_release_on_github() {
            let (site, github) = (serve(release("1.0.0", SIG_100), false), serve(release("1.1.0", SIG_110), false));
            let src = places(&[&site, &github]);
            // the site says nothing newer, signed and all: GitHub is still asked, and its newer release is the answer
            let found = check(&src, &have("1.0.0"), 7).unwrap();
            assert_eq!((found.status.state.as_str(), found.status.version.as_str()), ("newer", "1.1.0"));
            assert_eq!(found.manifest, manifest("1.1.0"));
            assert_eq!(site.asked(), ["SHA256SUMS.txt", "SHA256SUMS.txt.sig", "manifest.json"]);
            assert_eq!(github.asked(), ["SHA256SUMS.txt", "SHA256SUMS.txt.sig", "manifest.json"]);
            // GitHub out of reach: the site's signed answer stands
            let found = check(&places(&[&site, &down()]), &have("1.0.0"), 7).unwrap();
            assert_eq!((found.status.state.as_str(), found.status.version.as_str()), ("latest", "1.0.0"));
            // a newer release on the site is the answer at once: GitHub isn't asked
            let (site, github) = (serve(release("1.1.0", SIG_110), false), serve(release("1.0.0", SIG_100), false));
            let found = check(&places(&[&site, &github]), &have("1.0.0"), 7).unwrap();
            assert_eq!(found.status.version, "1.1.0");
            assert!(github.asked().is_empty());
        }

        #[test]
        fn an_answer_in_hand_waits_only_briefly_for_the_next_place() {
            let (site, github) = (serve(release("1.0.0", SIG_100), false), serve(vec![], true));
            let t = Instant::now();
            let found = check(&places(&[&site, &github]), &have("1.0.0"), 7).unwrap();
            assert_eq!(found.status.state, "latest");
            // GitHub is the last place, waited for `net::PATIENT` (30 s without a byte) when it is the only hope; with the
            // site's answer in hand, the
            // short wait (1 s here, and about 3 s WinHTTP adds when it gives up)
            assert!(t.elapsed() < Duration::from_secs(15), "took {:?}", t.elapsed());
            assert_eq!(github.asked(), ["SHA256SUMS.txt"]);
        }

        #[test]
        fn a_newer_release_unsigned_or_wrongly_signed_is_passed_over() {
            let github = serve(release("1.0.0", SIG_100), false);
            let unsigned: Files =
                release("1.1.0", SIG_110).into_iter().filter(|(n, _)| *n != "SHA256SUMS.txt.sig").collect();
            // signed by a key that isn't trusted; a signature that is ours but for another list (1.0.0's); a list
            // signed rightly with a manifest it doesn't name; no signature at all (a draft published unsigned)
            let mut other_manifest = release("1.1.0", SIG_110);
            other_manifest[0].1 = manifest("9.0.0");
            let mut other_list = release("1.1.0", SIG_110);
            other_list[2].1 = SIG_100.as_bytes().to_vec();
            for (name, files) in [
                ("bad key", release("1.1.0", SIG_BAD)),
                ("another list's", other_list),
                ("unnamed manifest", other_manifest),
                ("unsigned", unsigned),
            ] {
                let site = serve(files, false);
                let found = check(&places(&[&site, &github]), &have("1.0.0"), 7).unwrap();
                assert_eq!((found.status.state.as_str(), found.status.version.as_str()), ("latest", "1.0.0"), "{name}");
                assert!(!github.asked().is_empty(), "{name}: GitHub asked");
                // alone, it is no release (a check never says "Verification failed"), and nothing is downloaded from it
                assert_eq!(check(&places(&[&site]), &have("1.0.0"), 7).err(), Some(Problem::Unreadable), "{name}");
                let dir = Scratch::new("unsigned");
                assert!(get(&places(&[&site]), &have("1.0.0"), false, &dir).is_err(), "{name}");
                assert!(!site.asked().contains(&SETUP.to_string()), "{name}: the installer never asked for");
                assert!(dir.left().is_empty(), "{name}");
            }
        }

        #[test]
        fn a_web_page_served_for_every_file_is_no_release() {
            // what a site may answer for a file it hasn't: its own page, with 200
            let page = b"<!doctype html><html><body>Not here</body></html>".to_vec();
            let files: Files =
                ["SHA256SUMS.txt", "SHA256SUMS.txt.sig", "manifest.json", "DotAny-Setup.exe", "DotAny-mac.zip"]
                    .into_iter()
                    .map(|n| (n, page.clone()))
                    .collect();
            let site = serve(files, false);
            assert_eq!(check(&places(&[&site]), &have("1.0.0"), 7).err(), Some(Problem::Unreadable));
            assert_eq!(check(&places(&[&site, &down()]), &have("1.0.0"), 7).err(), Some(Problem::Unreadable));
            let github = serve(release("1.1.0", SIG_110), false);
            let src = places(&[&site, &github]);
            assert_eq!(check(&src, &have("1.0.0"), 7).unwrap().status.version, "1.1.0");
            let dir = Scratch::new("page");
            assert_eq!(get(&src, &have("1.0.0"), false, &dir), Ok(setup("1.1.0")));
            assert!(!site.asked().contains(&SETUP.to_string()));
        }

        #[test]
        fn an_installer_changed_after_its_check_is_not_started() {
            let github = serve(release("1.1.0", SIG_110), false);
            let dir = Scratch::new("changed");
            let got = download(&places(&[&github]), &have("1.0.0"), false, &dir.0, &mut |_| {}).unwrap();
            assert!(still_as_checked(&got));
            assert!(still_as_checked(&got), "and again: checking reads, it doesn't take");
            // days later, something has written to it
            std::fs::write(&got, b"something else").unwrap();
            assert!(!launch(&got, &dir.0, &[]), "not started");
            assert!(!got.exists(), "and thrown away, so it is downloaded again");
            // a file this DotAny never downloaded is never started either
            let other = dir.0.join("other.exe");
            std::fs::write(&other, setup("1.1.0")).unwrap();
            assert!(!still_as_checked(&other));
        }

        #[test]
        fn a_place_at_the_installed_version_is_passed_over_for_an_update() {
            let (site, github) = (serve(release("1.0.0", SIG_100), false), serve(release("1.1.0", SIG_110), false));
            let src = places(&[&site, &github]);
            // Update (a check found 1.1.0 on GitHub): the site's 1.0.0 isn't taken for it
            let dir = Scratch::new("lags");
            assert_eq!(get(&src, &have("1.0.0"), false, &dir), Ok(setup("1.1.0")));
            assert!(!site.asked().contains(&SETUP.to_string()));
            // Install a tool: the site's release at the installed versions will do
            let (site, github) = (serve(release("1.0.0", SIG_100), false), serve(release("1.1.0", SIG_110), false));
            assert_eq!(get(&places(&[&site, &github]), &have("1.0.0"), true, &dir), Ok(setup("1.0.0")));
            assert!(github.asked().is_empty());
        }

        #[test]
        fn a_first_place_that_never_answers_costs_only_its_short_wait() {
            let (site, github) = (serve(vec![], true), serve(release("1.1.0", SIG_110), false));
            let src = places(&[&site, &github]);
            let t = Instant::now();
            let found = check(&src, &have("1.0.0"), 7).unwrap();
            assert_eq!(found.status.version, "1.1.0");
            let dir = Scratch::new("silent");
            assert_eq!(get(&src, &have("1.0.0"), false, &dir), Ok(setup("1.1.0")));
            // two short waits of 1 s (the check, the download's list); WinHTTP adds about 3 s to each when it gives up
            // on a connection that never answered (8 s in all here). The patient wait alone would be 30 s.
            assert!(t.elapsed() < Duration::from_secs(20), "took {:?}", t.elapsed());
            assert_eq!(site.asked(), ["SHA256SUMS.txt", "SHA256SUMS.txt"]);
        }
    }
}
