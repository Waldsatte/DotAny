//! Everything recorded so far: one WAV per take in the recordings folder, with a small JSON beside it for its name, when
//! it was made and its marks. A take that was still recording when the program stopped short is brought back intact.

use crate::wav;
use dotany_base::atomic::atomic_write;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{AppHandle, Manager};

/// Where takes go: the user's Music folder, where they can find them. Tests move the settings folder, and the takes with it.
pub fn folder(app: &AppHandle) -> PathBuf {
    let base = app.state::<dotany_base::Base>();
    let dir = if std::env::var_os(base.tool.data_env).is_some() {
        base.data_dir.join("takes")
    } else {
        dotany_base::platform::user_dir("Music").join("DotAny Recorder")
    };
    let _ = fs::create_dir_all(&dir);
    dir
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Meta {
    pub name: String,
    /// Milliseconds since 1970, when recording began.
    pub created: u64,
    /// Seconds from the start.
    #[serde(default)]
    pub marks: Vec<f64>,
    /// Still being written. Left true by a program that stopped short.
    #[serde(default)]
    pub recording: bool,
    /// Brought back after the program stopped short.
    #[serde(default)]
    pub recovered: bool,
    /// Not all of what was heard is in the file: a write to the disk failed, or the disk fell so far behind that sound
    /// had to be dropped (`dropped` says how many samples).
    #[serde(default)]
    pub incomplete: bool,
    #[serde(default)]
    pub dropped: u64,
}

pub fn now_ms() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

pub fn meta_path(wav: &Path) -> PathBuf {
    wav.with_extension("json")
}

pub fn read_meta(wav: &Path) -> Option<Meta> {
    serde_json::from_slice(&fs::read(meta_path(wav)).ok()?).ok()
}

pub fn write_meta(wav: &Path, meta: &Meta) -> Result<(), String> {
    atomic_write(&meta_path(wav), &serde_json::to_vec_pretty(meta).unwrap()).map_err(|e| e.detail)
}

/// Is this file one of ours, in the recordings folder?
pub fn is_take(app: &AppHandle, path: &Path) -> bool {
    let dir = folder(app);
    path.parent().is_some_and(|p| dotany_base::same_file(p, &dir))
        && path.extension().is_some_and(|e| e.eq_ignore_ascii_case("wav"))
}

/// A file name for a new take: its name, made safe, and numbered if the name is taken.
pub fn new_path(dir: &Path, name: &str) -> PathBuf {
    let safe: String =
        name.chars().map(|c| if "\\/:*?\"<>|".contains(c) || c.is_control() { '-' } else { c }).collect();
    let safe = safe.trim().trim_end_matches('.');
    let safe = if safe.is_empty() { "Recording" } else { safe };
    let taken = |stem: &str| dir.join(format!("{stem}.wav")).exists() || dir.join(format!("{stem}.json")).exists();
    let mut stem = safe.to_string();
    let mut n = 2;
    while taken(&stem) {
        stem = format!("{safe} ({n})");
        n += 1;
    }
    dir.join(format!("{stem}.wav"))
}

#[derive(Serialize, Clone, Debug)]
pub struct Row {
    pub path: String,
    pub name: String,
    pub created: u64,
    pub rate: u32,
    pub frames: u64,
    pub marks: Vec<f64>,
    pub recovered: bool,
    pub incomplete: bool,
    pub live: bool,
}

fn files(dir: &Path, ext: &str) -> Vec<PathBuf> {
    let Ok(read) = fs::read_dir(dir) else { return Vec::new() };
    read.flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext)))
        .filter(|p| !dotany_base::file_name(p).starts_with(dotany_base::atomic::TEMP_PREFIX))
        .collect()
}

/// Every take, newest first. `live` is the one being recorded right now, if any.
pub fn list(app: &AppHandle, live: Option<&Path>) -> Vec<Row> {
    let mut rows: Vec<Row> = files(&folder(app), "wav")
        .into_iter()
        .filter_map(|p| {
            let meta = read_meta(&p)?;
            let is_live = live.is_some_and(|l| dotany_base::same_file(l, &p));
            let info = wav::open_info(&p);
            Some(Row {
                name: meta.name,
                created: meta.created,
                rate: info.map_or(48000, |i| i.rate),
                frames: info.map_or(0, |i| i.frames()),
                marks: meta.marks,
                recovered: meta.recovered,
                incomplete: meta.incomplete,
                live: is_live,
                path: p.to_string_lossy().into_owned(),
            })
        })
        .collect();
    rows.sort_by(|a, b| b.created.cmp(&a.created).then_with(|| b.path.cmp(&a.path)));
    rows
}

/// Takes the last run left mid-recording come back whole: the header is brought up to the sound that reached the disk,
/// and the take says it was found again. One still held by another Recorder window is left to it.
pub fn recover(app: &AppHandle) -> Vec<String> {
    let dir = folder(app);
    let mut found = Vec::new();
    for p in files(&dir, "wav") {
        let meta = read_meta(&p);
        if meta.as_ref().is_some_and(|m| !m.recording) {
            continue;
        }
        // another window writing it holds it shut to writers; then it isn't ours to touch
        if wav::repair(&p).is_err() {
            continue;
        }
        let mut meta = meta.unwrap_or_else(|| Meta {
            name: p.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
            created: fs::metadata(&p)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_millis() as u64),
            ..Default::default()
        });
        meta.recording = false;
        meta.recovered = true;
        if write_meta(&p, &meta).is_ok() {
            found.push(p.to_string_lossy().into_owned());
        }
    }
    found
}

/// Deletes takes, but only takes: files outside the recordings folder are never touched here.
pub fn delete(app: &AppHandle, paths: &[String], live: Option<&Path>) -> Vec<String> {
    let mut gone = Vec::new();
    for p in paths.iter().map(PathBuf::from) {
        if !is_take(app, &p) || live.is_some_and(|l| dotany_base::same_file(l, &p)) {
            continue;
        }
        if remove_take(&p) {
            gone.push(p.to_string_lossy().into_owned());
        }
    }
    gone
}

/// The sound first; its JSON only once the sound is gone. A WAV another program holds (a player, OneDrive) stays
/// with its JSON, so it stays in the list and can be deleted later, instead of becoming a file nobody lists.
fn remove_take(p: &Path) -> bool {
    let wav_gone = fs::remove_file(p).is_ok() || !p.exists();
    if wav_gone {
        let _ = fs::remove_file(meta_path(p));
    }
    wav_gone
}

// Windows' sharing modes: on the Mac another program holding the file never stops it going
#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn a_take_whose_sound_is_held_keeps_its_record() {
        use std::os::windows::fs::OpenOptionsExt;
        let dir = std::env::temp_dir().join(format!("dotany-recorder-delete-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let wav = dir.join("take.wav");
        fs::write(&wav, wav::header(48000, 1, 0)).unwrap();
        fs::write(meta_path(&wav), b"{}").unwrap();
        // another program has it open and shares nothing, as a player can
        let held = fs::OpenOptions::new().read(true).share_mode(0).open(&wav).unwrap();
        assert!(!remove_take(&wav));
        assert!(wav.exists() && meta_path(&wav).exists(), "both stay");
        drop(held);
        assert!(remove_take(&wav));
        assert!(!wav.exists() && !meta_path(&wav).exists(), "both go");
        let _ = fs::remove_dir_all(&dir);
    }
}
