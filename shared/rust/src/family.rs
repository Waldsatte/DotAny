//! What the whole family shares between programs, in `%APPDATA%\DotAny\Shared`: one list of recent files that every
//! tool writes to and DotAny (the hub) reads, and the settings every tool follows: appearance, the top bar's labels,
//! sound, language and autosave.
//! Each program changes these files under a lock, so two tools saving at once never lose each other's line.
//!
//! The folder is `DOTANY_FAMILY_DATA` when set. When it isn't, but the tool's own data folder was moved (a test), it is
//! `family` inside that folder, so a test never reaches the real list.

use crate::atomic::atomic_write;
use crate::watcher::{self, FileWatch};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// The settings every tool follows: every setting of the family's own (appearance, the top bar's labels, sound,
/// language, and Text's autosave, which any tool that saves in place would follow). A tool's own settings stay its own:
/// Recorder's microphone, Video's volume, Text's recent formats, PDF's dimmed pages, Books' reading choices.
pub const SHARED_KEYS: [&str; 5] = ["dotany-theme", "dotany-sound", "dotany-lang", "dotany-labels", "dotany-autosave"];
/// The keys that were shared first: a settings file that exists but has no `SEEDED` entry already holds every
/// program's choice of these.
const FIRST_KEYS: [&str; 3] = ["dotany-theme", "dotany-sound", "dotany-lang"];
/// In the shared settings, the keys some program has already brought in or someone has set, comma-separated, so a
/// choice made since is never overwritten by an older one a tool still keeps.
pub const SEEDED: &str = "dotany-shared-seeded";
/// Enough for every tool to keep its own eight.
pub const MAX_RECENT: usize = 96;

pub fn is_shared(key: &str) -> bool {
    SHARED_KEYS.contains(&key)
}

/// One file somebody opened or saved: where it is, which tool (its settings folder name: "Audio", "3D"), and when.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Recent {
    pub path: String,
    pub tool: String,
    /// Milliseconds since 1970; 0 for what a tool had before the list was shared.
    #[serde(default)]
    pub at: u64,
}

#[derive(Default, Serialize, Deserialize, Clone, Debug)]
pub struct Recents {
    #[serde(default)]
    pub files: Vec<Recent>,
    /// The tools whose own old lists were already brought in, so a list cleared in DotAny stays cleared.
    #[serde(default)]
    pub imported: Vec<String>,
}

pub fn dir(tool_env: &str) -> PathBuf {
    if let Some(d) = std::env::var_os("DOTANY_FAMILY_DATA") {
        return PathBuf::from(d);
    }
    if let Some(d) = std::env::var_os(tool_env) {
        return PathBuf::from(d).join("family");
    }
    crate::platform::data_dir("Shared")
}

fn recents_file(dir: &Path) -> PathBuf {
    dir.join("recents.json")
}

pub fn settings_file(dir: &Path) -> PathBuf {
    dir.join("settings.json")
}

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

fn read<T: for<'a> Deserialize<'a> + Default>(path: &Path) -> Option<T> {
    let raw = std::fs::read(path).ok()?;
    match serde_json::from_slice(&raw) {
        Ok(v) => Some(v),
        Err(_) => {
            // set aside, never overwritten blindly
            let _ = std::fs::rename(path, path.with_extension("broken.json"));
            Some(T::default())
        }
    }
}

fn write<T: Serialize>(path: &Path, value: &T) {
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(raw) = serde_json::to_vec_pretty(value) {
        let _ = atomic_write(path, &raw);
    }
}

/// Changes a shared file under the family's lock: read, change, write, as one step among all the programs.
fn change<T: for<'a> Deserialize<'a> + Serialize + Default, R>(path: &Path, f: impl FnOnce(&mut T) -> R) -> R {
    let _lock = path.parent().and_then(|d| crate::platform::lock_file(&d.join(".lock")));
    let mut value: T = read(path).unwrap_or_default();
    let r = f(&mut value);
    write(path, &value);
    r
}

// ---------- recent files ----------

pub fn recents(dir: &Path) -> Recents {
    read(&recents_file(dir)).unwrap_or_default()
}

/// The newest of one tool's files, as its own empty window and file menu list them.
pub fn recents_of(dir: &Path, tool: &str, n: usize) -> Vec<String> {
    recents(dir).files.into_iter().filter(|r| r.tool == tool).take(n).map(|r| r.path).collect()
}

/// `path` was opened or saved in `tool`, just now: first on the list, and nowhere else on it.
pub fn add_recent(dir: &Path, tool: &str, path: &str) {
    change(&recents_file(dir), |list: &mut Recents| {
        list.files.retain(|r| !crate::platform::same_name(r.path.as_ref(), path.as_ref()));
        list.files.insert(0, Recent { path: path.to_string(), tool: tool.to_string(), at: now_ms() });
        list.files.truncate(MAX_RECENT);
    });
}

pub fn forget_recent(dir: &Path, path: &str) {
    change(&recents_file(dir), |list: &mut Recents| list.files.retain(|r| r.path != path));
}

pub fn clear_recents(dir: &Path) {
    change(&recents_file(dir), |list: &mut Recents| list.files.clear());
}

/// Once per tool: the list it kept before the list was shared joins the end of the shared one.
pub fn import_recents(dir: &Path, tool: &str, own: &[String]) {
    change(&recents_file(dir), |list: &mut Recents| {
        if list.imported.iter().any(|t| t == tool) {
            return;
        }
        list.imported.push(tool.to_string());
        for p in own {
            if !list.files.iter().any(|r| crate::platform::same_name(r.path.as_ref(), p.as_ref())) {
                list.files.push(Recent { path: p.clone(), tool: tool.to_string(), at: 0 });
            }
        }
        list.files.truncate(MAX_RECENT);
    });
}

// ---------- settings every tool follows ----------

/// The shared settings, or None before any program wrote them.
pub fn settings(dir: &Path) -> Option<BTreeMap<String, String>> {
    let path = settings_file(dir);
    if !path.is_file() {
        return None;
    }
    read(&path)
}

/// Sets one shared setting (None takes it away: "follow the system", or the default). From then on no program's older
/// choice is brought in over it.
pub fn set_setting(dir: &Path, key: &str, value: Option<&str>) {
    change(&settings_file(dir), |values: &mut BTreeMap<String, String>| {
        let mut seeded = seeded_keys(values, true);
        if !seeded.iter().any(|k| k == key) {
            seeded.push(key.to_string());
        }
        values.insert(SEEDED.to_string(), seeded.join(","));
        match value {
            Some(v) => {
                values.insert(key.to_string(), v.to_string());
            }
            None => {
                values.remove(key);
            }
        }
    });
}

fn seeded_keys(values: &BTreeMap<String, String>, existed: bool) -> Vec<String> {
    match values.get(SEEDED) {
        Some(list) => list.split(',').filter(|k| !k.is_empty()).map(str::to_string).collect(),
        None if existed => FIRST_KEYS.iter().map(|k| k.to_string()).collect(),
        None => Vec::new(),
    }
}

/// Each program brings its own choices as it starts: a shared key that no program has brought in and nobody has set
/// yet takes this tool's value, so no choice is lost when a setting becomes shared; one already brought in or set is
/// left alone.
pub fn seed_settings(dir: &Path, own: &BTreeMap<String, String>) {
    let path = settings_file(dir);
    let _ = std::fs::create_dir_all(dir);
    let _lock = crate::platform::lock_file(&dir.join(".lock"));
    let existed = path.exists();
    let mut values: BTreeMap<String, String> = if existed { read(&path).unwrap_or_default() } else { BTreeMap::new() };
    let mut seeded = seeded_keys(&values, existed);
    let mut changed = !existed;
    for k in SHARED_KEYS {
        if seeded.iter().any(|s| s == k) {
            continue;
        }
        if values.contains_key(k) {
            seeded.push(k.to_string());
            changed = true;
        } else if let Some(v) = own.get(k) {
            values.insert(k.to_string(), v.clone());
            seeded.push(k.to_string());
            changed = true;
        }
    }
    if changed {
        values.insert(SEEDED.to_string(), seeded.join(","));
        write(&path, &values);
    }
}

/// Brings the shared settings into a tool's own: every shared key as the family has it (absent is "follow the
/// system", or the default). Returns the keys that changed, with their new values.
pub fn follow(own: &mut BTreeMap<String, String>, shared: &BTreeMap<String, String>) -> Vec<(String, Option<String>)> {
    let mut changed = Vec::new();
    for k in SHARED_KEYS {
        let now = shared.get(k).cloned();
        if own.get(k) != now.as_ref() {
            match &now {
                Some(v) => own.insert(k.to_string(), v.clone()),
                None => own.remove(k),
            };
            changed.push((k.to_string(), now));
        }
    }
    changed
}

/// A tool that keeps its own settings joins the family's as it starts: it brings its choices in, then takes the
/// family's. Returns the shared folder, for `set_setting` and `watch_settings`.
pub fn join(tool_env: &str, own: &mut BTreeMap<String, String>) -> PathBuf {
    let dir = dir(tool_env);
    seed_settings(&dir, own);
    if let Some(shared) = settings(&dir) {
        follow(own, &shared);
    }
    dir
}

/// The shared settings as a page wants them: every shared key, with null for "follow the system".
pub fn page_values(own: &BTreeMap<String, String>) -> serde_json::Value {
    SHARED_KEYS.iter().map(|k| (k.to_string(), own.get(*k).cloned().into())).collect::<serde_json::Map<_, _>>().into()
}

/// Calls back when another program changes the shared settings or the shared list.
pub fn watch(
    dir: &Path,
    on_settings: impl Fn() + Send + 'static,
    on_recents: impl Fn() + Send + 'static,
) -> Vec<FileWatch> {
    let _ = std::fs::create_dir_all(dir);
    [watcher::watch(&settings_file(dir), on_settings), watcher::watch(&recents_file(dir), on_recents)]
        .into_iter()
        .flatten()
        .collect()
}

/// Calls back when another program changes the shared settings (for a tool that keeps its own recent list).
pub fn watch_settings(dir: &Path, on_settings: impl Fn() + Send + 'static) -> Option<FileWatch> {
    let _ = std::fs::create_dir_all(dir);
    watcher::watch(&settings_file(dir), on_settings)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn own(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }
    fn fresh(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("dotany-family-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn a_file_from_before_brings_in_only_the_newly_shared_keys() {
        let dir = fresh("before");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(settings_file(&dir), r#"{"dotany-theme":"dark"}"#).unwrap();
        seed_settings(&dir, &own(&[("dotany-sound", "on"), ("dotany-labels", "off"), ("recorder-mic", "x")]));
        let s = settings(&dir).unwrap();
        assert_eq!(s.get("dotany-theme").map(String::as_str), Some("dark"));
        assert_eq!(s.get("dotany-sound"), None, "sound was shared already: the family's choice (none) stands");
        assert_eq!(
            s.get("dotany-labels").map(String::as_str),
            Some("off"),
            "labels weren't: this tool's choice comes in"
        );
        assert_eq!(s.get("recorder-mic"), None, "a tool's own setting stays its own");
        // a second tool's older choice doesn't overwrite it
        seed_settings(&dir, &own(&[("dotany-labels", "on")]));
        assert_eq!(settings(&dir).unwrap().get("dotany-labels").map(String::as_str), Some("off"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn what_someone_set_is_never_brought_in_again() {
        let dir = fresh("set");
        seed_settings(&dir, &BTreeMap::new());
        set_setting(&dir, "dotany-autosave", None);
        seed_settings(&dir, &own(&[("dotany-autosave", "on")]));
        assert_eq!(settings(&dir).unwrap().get("dotany-autosave"), None, "turned off, stays off");
        // a key nobody has set or brought in yet takes the next tool's choice
        seed_settings(&dir, &own(&[("dotany-labels", "off")]));
        assert_eq!(settings(&dir).unwrap().get("dotany-labels").map(String::as_str), Some("off"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn follow_reports_what_changed() {
        let mut mine = own(&[("dotany-labels", "off"), ("dotany-theme", "dark"), ("video-volume", "0.5")]);
        let changed = follow(&mut mine, &own(&[("dotany-theme", "dark"), ("dotany-sound", "on")]));
        assert_eq!(
            changed,
            vec![("dotany-sound".to_string(), Some("on".to_string())), ("dotany-labels".to_string(), None)]
        );
        assert_eq!(mine, own(&[("dotany-sound", "on"), ("dotany-theme", "dark"), ("video-volume", "0.5")]));
    }
}
