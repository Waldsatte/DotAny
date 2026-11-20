//! DotAny's installer. One program, three ways it runs:
//!
//! - **With a payload** (DotAny-Setup.exe): the installer's four steps, or with `--quiet` a silent install.
//! - **Without one** (`<root>\uninstall.exe`, the same program cut before the payload): the uninstaller. It first copies
//!   itself to %TEMP% and runs from there, because a program can't delete itself.
//!
//! Quiet use, for administrators and the tests:
//!
//! ```text
//! DotAny-Setup.exe --quiet [--root <folder>] [--full | --tools text,audio] [--no-autostart] [--log <file>]
//! uninstall.exe --quiet [--remove-settings] [--log <file>]
//! ```
//!
//! **Update mode**, which the hub starts for Install and Update (hub/src-tauri/src/update.rs):
//!
//! ```text
//! DotAny-Setup.exe --update [--root <folder>] [--tools text,audio] [--quiet] [--log <file>]
//! ```
//!
//! It brings the install in `--root` (else the one the registry knows, else the default folder) to this release: every
//! tool already there, plus `--tools`. Open DotAny programs are named and have to be closed first (the hub itself is
//! ended and opened again, it has no unsaved work). There is no choosing step; when it is done, DotAny opens. With
//! `--quiet` an open program ends it with code 4, and DotAny is only started again if it was running.
//!
//! Exit codes: 0 done, 1 cancelled, 2 not enough space, 3 not allowed to write there, 4 a DotAny program is open,
//! 5 anything else. `--log` writes the result as JSON.

pub mod app;
pub mod engine;
pub mod manifest;
pub mod payload;
pub mod reg;
pub mod system;
pub mod words;

use engine::{Choice, Failure};
use manifest::Record;
use payload::Payload;
use reg::Hive;
use serde_json::json;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

fn args() -> Vec<String> {
    std::env::args().collect()
}
fn flag(name: &str) -> bool {
    args().iter().any(|a| a == name)
}
fn value(name: &str) -> Option<String> {
    let a = args();
    a.iter().position(|x| x == name).and_then(|i| a.get(i + 1).cloned())
}

fn code(f: &Failure) -> i32 {
    match f {
        Failure::Cancelled => 1,
        Failure::Space { .. } => 2,
        Failure::Denied => 3,
        Failure::Busy { .. } => 4,
        Failure::Other { .. } => 5,
    }
}

fn log(result: serde_json::Value) {
    if let Some(path) = value("--log") {
        let _ = std::fs::write(path, serde_json::to_vec_pretty(&result).unwrap());
    }
}

/// This run's own folder under %TEMP%. Folders an earlier run couldn't remove (it was ended, not closed) go first.
fn temp_dir(kind: &str) -> PathBuf {
    if let Ok(list) = std::fs::read_dir(std::env::temp_dir()) {
        for e in list.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            let pid = name
                .strip_prefix("DotAny-Setup-")
                .or_else(|| name.strip_prefix("DotAny-Uninstall-"))
                .and_then(|p| p.parse::<u32>().ok());
            if let Some(pid) = pid {
                if pid != std::process::id() && !system::is_running(pid) {
                    let _ = std::fs::remove_dir_all(e.path());
                }
            }
        }
    }
    let dir = std::env::temp_dir().join(format!("DotAny-{kind}-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&dir);
    dir
}

/// Where Microsoft hands out the WebView2 Runtime (the Evergreen bootstrapper).
const WEBVIEW2_URL: &str = "https://go.microsoft.com/fwlink/p/?LinkId=2124703";

/// Every DotAny window, this one included, is drawn by Microsoft Edge WebView2. Windows 11 and an up-to-date Windows
/// 10 have it; some older or trimmed systems (Windows 10 LTSC 2019, Server 2019, slimmed images) don't, and then no
/// window can open at all. `DOTANY_SETUP_NO_WEBVIEW2` makes a test behave as if it were missing.
pub fn webview2_present() -> bool {
    std::env::var_os("DOTANY_SETUP_NO_WEBVIEW2").is_none() && tauri::webview_version().is_ok()
}

fn open_url(url: &str) {
    let _ = std::process::Command::new("explorer.exe").arg(url).spawn();
}

/// Says what's missing in a plain system message, instead of ending without a word, and offers the download page.
fn explain_missing_webview2() {
    let (text, download, close) = words::say(words::system_lang(), words::Say::NoWebView2);
    let answer = rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Warning)
        .set_title("DotAny")
        .set_description(text)
        .set_buttons(rfd::MessageButtons::OkCancelCustom(download.into(), close.into()))
        .show();
    if answer == rfd::MessageDialogResult::Ok || answer == rfd::MessageDialogResult::Custom(download.into()) {
        open_url(WEBVIEW2_URL);
    }
}

/// The uninstaller without WebView2: its window can't open, so it asks in a system message and removes DotAny the
/// quiet way, keeping the settings.
fn uninstall_without_window(record: &Record) -> i32 {
    let lang = words::system_lang();
    let (ask, go, keep) = words::say(lang, words::Say::UninstallWithoutWindow);
    let answer = rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Warning)
        .set_title("DotAny")
        .set_description(ask)
        .set_buttons(rfd::MessageButtons::OkCancelCustom(go.into(), keep.into()))
        .show();
    if !(answer == rfd::MessageDialogResult::Ok || answer == rfd::MessageDialogResult::Custom(go.into())) {
        return 1;
    }
    let (result, said) = match engine::uninstall(record, &Hive::from_env(), false) {
        Ok(_) => (0, words::say(lang, words::Say::Uninstalled).0.to_string()),
        Err(Failure::Busy { names }) => {
            (4, words::say(lang, words::Say::StillOpen).0.replace("{}", &names.join(words::list_sep(lang))))
        }
        Err(f) => (code(&f), words::say(lang, words::Say::CouldntUninstall).0.to_string()),
    };
    rfd::MessageDialog::new().set_title("DotAny").set_description(said).show();
    result
}

pub fn run() -> i32 {
    let me = std::env::current_exe().expect("this program's path");
    match Payload::open(&me) {
        Ok(Some(payload)) => {
            if flag("--update") {
                let plan = update_plan(&payload);
                if flag("--quiet") {
                    quiet_update(&payload, plan)
                } else if !webview2_present() {
                    explain_missing_webview2();
                    log(json!({ "ok": false, "detail": "the Microsoft Edge WebView2 Runtime is missing" }));
                    5
                } else {
                    engine::OPEN_HUB_AFTER.store(true, std::sync::atomic::Ordering::Relaxed);
                    app::run(app::Mode::Update(payload, plan), temp_dir("Setup"));
                    0
                }
            } else if flag("--quiet") {
                quiet_install(&payload)
            } else if !webview2_present() {
                explain_missing_webview2();
                log(json!({ "ok": false, "detail": "the Microsoft Edge WebView2 Runtime is missing" }));
                5
            } else {
                app::run(app::Mode::Install(payload), temp_dir("Setup"));
                0
            }
        }
        Ok(None) => run_uninstaller(&me),
        Err(e) => {
            rfd::MessageDialog::new()
                .set_title("DotAny")
                .set_description(words::say(words::system_lang(), words::Say::Damaged).0)
                .show();
            log(json!({ "ok": false, "detail": e.to_string() }));
            5
        }
    }
}

fn quiet_install(payload: &Payload) -> i32 {
    let hive = Hive::from_env();
    let root = value("--root")
        .map(PathBuf::from)
        .or_else(|| engine::existing(&hive).map(|r| PathBuf::from(r.root)))
        .unwrap_or_else(system::default_root);
    let programs: Vec<String> = if let Some(list) = value("--tools") {
        list.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect()
    } else {
        payload.release.programs().map(|c| c.id.clone()).collect()
    };
    let choice = Choice { root, programs };
    let cancel = AtomicBool::new(false);
    let result = engine::stage(payload, &choice, &cancel, |_| {}).and_then(|staged| {
        engine::commit(payload, staged, &choice, &hive, words::system_lang(), !flag("--no-autostart"))
    });
    match result {
        Ok(record) => {
            // a quiet install goes ahead without WebView2 (an administrator may add it afterwards), but says so
            log(
                json!({ "ok": true, "root": record.root, "programs": record.components.iter().map(|c| &c.id).collect::<Vec<_>>(), "webview2": webview2_present() }),
            );
            0
        }
        Err(f) => {
            log(json!({ "ok": false, "failure": f }));
            code(&f)
        }
    }
}

/// What the update mode installs: where, and which tools.
pub fn update_plan(payload: &Payload) -> Choice {
    let hive = Hive::from_env();
    let existing = engine::existing(&hive);
    let root = value("--root")
        .map(PathBuf::from)
        .or_else(|| existing.as_ref().map(|r| PathBuf::from(&r.root)))
        .unwrap_or_else(system::default_root);
    // the install in that folder: every tool it has stays, at this release's version
    let record = Record::load(&root).or(existing.filter(|r| engine::same_dir(Path::new(&r.root), &root)));
    let mut programs: Vec<String> = record
        .map(|r| r.components.iter().filter(|c| c.exe.is_some()).map(|c| c.id.clone()).collect())
        .unwrap_or_default();
    if let Some(list) = value("--tools") {
        programs.extend(list.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()));
    }
    let release = &payload.release;
    let chosen: Vec<String> =
        release.programs().filter(|c| !c.required && programs.contains(&c.id)).map(|c| c.id.clone()).collect();
    Choice { root, programs: chosen }
}

/// The open DotAny programs in the update's folder, by component (id, name).
pub fn open_in(payload: &Payload, root: &Path) -> Vec<(String, String)> {
    engine::open_programs(root)
        .into_iter()
        .map(|file| {
            match payload
                .release
                .components
                .iter()
                .find(|c| c.exe.as_deref().map(|e| e.eq_ignore_ascii_case(&file)).unwrap_or(false))
            {
                Some(c) => (c.id.clone(), c.name.clone()),
                None => (String::new(), file),
            }
        })
        .collect()
}

fn quiet_update(payload: &Payload, choice: Choice) -> i32 {
    let open = open_in(payload, &choice.root);
    if !open.is_empty() {
        let f = Failure::Busy {
            names: open.into_iter().map(|(id, n)| if id.is_empty() { n } else { format!("DotAny {n}") }).collect(),
        };
        log(json!({ "ok": false, "failure": f }));
        return code(&f);
    }
    let hive = Hive::from_env();
    // starting with Windows stays as the person left it
    let autostart = Record::load(&choice.root).or_else(|| engine::existing(&hive)).map(|r| r.autostart).unwrap_or(true);
    let cancel = AtomicBool::new(false);
    match engine::stage(payload, &choice, &cancel, |_| {})
        .and_then(|staged| engine::commit(payload, staged, &choice, &hive, words::system_lang(), autostart))
    {
        Ok(record) => {
            log(
                json!({ "ok": true, "root": record.root, "version": record.version, "programs": record.components.iter().map(|c| &c.id).collect::<Vec<_>>() }),
            );
            0
        }
        Err(f) => {
            log(json!({ "ok": false, "failure": f }));
            code(&f)
        }
    }
}

/// The uninstaller runs from a copy in %TEMP%, so it can take its own folder away.
fn run_uninstaller(me: &Path) -> i32 {
    let root = match value("--from") {
        Some(r) => PathBuf::from(r),
        None => {
            let here = me.parent().unwrap_or(Path::new(".")).to_path_buf();
            if Record::load(&here).is_none() {
                return 5;
            }
            let dir = temp_dir("Uninstall");
            let copy = dir.join(engine::UNINSTALLER);
            if std::fs::copy(me, &copy).is_err() {
                return 5;
            }
            let mut rest: Vec<String> = args().into_iter().skip(1).collect();
            rest.push("--from".into());
            rest.push(here.display().to_string());
            return match std::process::Command::new(&copy).args(&rest).spawn() {
                Ok(_) => 0,
                Err(_) => 5,
            };
        }
    };
    let Some(record) = Record::load(&root) else {
        log(json!({ "ok": false, "detail": "no install.json" }));
        return 5;
    };
    // the copy in %TEMP% goes once this process has
    let own = me.parent().map(Path::to_path_buf).filter(|d| d.starts_with(std::env::temp_dir()));
    // the uninstall.exe it was started from may still be closing
    std::thread::sleep(std::time::Duration::from_millis(300));
    let code = if flag("--quiet") {
        match engine::uninstall(&record, &Hive::from_env(), flag("--remove-settings")) {
            Ok(r) => {
                log(json!({ "ok": true, "programs": r.programs, "left": r.left }));
                0
            }
            Err(f) => {
                log(json!({ "ok": false, "failure": f }));
                code(&f)
            }
        }
    } else if !webview2_present() {
        uninstall_without_window(&record)
    } else {
        app::run(app::Mode::Uninstall(record), own.clone().unwrap_or_else(|| temp_dir("Uninstall")));
        0
    };
    if let Some(dir) = own {
        system::remove_after_exit(&dir);
    }
    code
}
