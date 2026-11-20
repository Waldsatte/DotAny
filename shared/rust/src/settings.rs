//! Settings and the recent-files list, in one small JSON file. A broken file is set aside, never overwritten blindly.

use crate::atomic::atomic_write;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const MAX_RECENT: usize = 8;

#[derive(Default, Serialize, Deserialize, Clone, Debug)]
pub struct Settings {
    #[serde(default)]
    pub values: BTreeMap<String, String>,
    #[serde(default)]
    pub recents: Vec<String>,
}

/// Where this tool keeps its settings: `DOTANY_<TOOL>_DATA` when set (tests use a throwaway folder), else the user's.
pub fn data_dir(tool: &crate::Tool) -> PathBuf {
    if let Some(dir) = std::env::var_os(tool.data_env) {
        return PathBuf::from(dir);
    }
    crate::platform::data_dir(tool.folder)
}

fn file(dir: &Path) -> PathBuf {
    dir.join("settings.json")
}

pub fn load(dir: &Path) -> Settings {
    let path = file(dir);
    let Ok(raw) = std::fs::read(&path) else { return Settings::default() };
    match serde_json::from_slice(&raw) {
        Ok(s) => s,
        Err(_) => {
            let _ = std::fs::rename(&path, dir.join("settings.broken.json"));
            Settings::default()
        }
    }
}

pub fn store(dir: &Path, settings: &Settings) {
    let _ = std::fs::create_dir_all(dir);
    if let Ok(raw) = serde_json::to_vec_pretty(settings) {
        let _ = atomic_write(&file(dir), &raw);
    }
}

pub fn add_recent(settings: &mut Settings, path: &str) {
    settings.recents.retain(|p| !crate::platform::same_name(p.as_ref(), path.as_ref()));
    settings.recents.insert(0, path.to_string());
    settings.recents.truncate(MAX_RECENT);
}
