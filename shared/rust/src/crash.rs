//! When a program panics, what happened is kept in `crash.log` in its own settings folder (`%APPDATA%\DotAny\<Tool>`):
//! the time, the program and its version, the panic's message and place, and a backtrace. The file keeps about the
//! last 256 KB; older entries are dropped whole. Then the panic goes on as it would have without the log: in an async command or a
//! thread of the program's own it ends only that (a lock it held: `locks`); on the main thread it ends the program.
//! Nothing is shown and nothing leaves the computer.

use std::path::PathBuf;
use std::sync::{Mutex, Once};
use std::time::{SystemTime, UNIX_EPOCH};

const CAP: usize = 256 * 1024;
const MARK: &str = "==== ";

struct Who {
    name: String,
    version: String,
    file: PathBuf,
}

static WHO: Mutex<Option<Who>> = Mutex::new(None);
static HOOK: Once = Once::new();

/// Installs the hook once per process; later calls only change the name, version or folder.
pub fn install(name: &str, version: &str, data_dir: PathBuf) {
    if let Ok(mut who) = WHO.lock() {
        *who = Some(Who { name: name.to_string(), version: version.to_string(), file: data_dir.join("crash.log") });
    }
    HOOK.call_once(|| {
        let before = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            // never a second panic from in here: every step may fail quietly
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| record(info)));
            before(info);
        }));
    });
    // only for the tests: a panic on a thread of its own, to see it written down while the program carries on
    if std::env::var_os("DOTANY_TEST").is_some() && std::env::var_os("DOTANY_TEST_PANIC").is_some_and(|v| !v.is_empty())
    {
        let _ = std::thread::Builder::new()
            .name("test-panic".into())
            .spawn(|| panic!("DOTANY_TEST_PANIC: a panic asked for by a test"));
    }
}

/// The version, once the program knows it (a tool on the base learns it from Tauri as it starts).
pub fn set_version(version: &str) {
    if let Ok(mut who) = WHO.lock() {
        if let Some(w) = who.as_mut() {
            w.version = version.to_string();
        }
    }
}

fn record(info: &std::panic::PanicHookInfo) {
    let Some((name, version, file)) =
        WHO.try_lock().ok().and_then(|w| w.as_ref().map(|w| (w.name.clone(), w.version.clone(), w.file.clone())))
    else {
        return;
    };
    let message = info
        .payload()
        .downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| info.payload().downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "(no message)".into());
    let place = info
        .location()
        .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
        .unwrap_or_else(|| "(unknown)".into());
    let thread = std::thread::current().name().unwrap_or("(unnamed)").to_string();
    let trace = std::backtrace::Backtrace::force_capture();
    let entry = format!(
        "{MARK}{} {name} {version}\npanic on thread '{thread}' at {place}:\n{message}\nbacktrace:\n{trace}\n",
        now()
    );
    append(&file, &entry);
}

fn append(file: &std::path::Path, entry: &str) {
    if let Some(d) = file.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let mut all = std::fs::read_to_string(file).unwrap_or_default();
    all.push_str(entry);
    let all = trim(&all, CAP);
    let _ = std::fs::write(file, all);
}

/// The newest entries that fit in `cap` bytes; a single entry larger than that keeps its end.
fn trim(all: &str, cap: usize) -> &str {
    if all.len() <= cap {
        return all;
    }
    let from = all.len() - cap;
    // the first entry that starts within the last `cap` bytes
    let mut at = from;
    while !all.is_char_boundary(at) {
        at += 1;
    }
    match all[at..].find(&format!("\n{MARK}")) {
        Some(i) => &all[at + i + 1..],
        None => &all[at..],
    }
}

/// The time in UTC, as `YYYY-MM-DD hh:mm:ssZ`.
fn now() -> String {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let (days, rem) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    // days since 1970 to a civil date (Howard Hinnant's algorithm)
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}Z", rem / 3600, rem % 3600 / 60, rem % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_newest_whole_entries() {
        let one = format!("{MARK}a\n{}\n", "x".repeat(100));
        let two = format!("{MARK}b\n{}\n", "y".repeat(100));
        let all = format!("{one}{two}");
        assert_eq!(trim(&all, 1000), all);
        assert_eq!(trim(&all, 150), two);
    }

    #[test]
    fn a_panic_is_written_down_and_then_goes_on() {
        let dir = std::env::temp_dir().join(format!("dotany-crash-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        install("DotAny Test", "9.9.9", dir.clone());
        let r = std::thread::Builder::new().name("worker".into()).spawn(|| panic!("the reason, 文本")).unwrap().join();
        assert!(r.is_err(), "the panic still happens");
        let log = std::fs::read_to_string(dir.join("crash.log")).unwrap();
        assert!(log.starts_with(MARK) && log.contains("DotAny Test 9.9.9"), "{log}");
        assert!(log.contains("panic on thread 'worker' at ") && log.contains("crash.rs:"), "{log}");
        assert!(log.contains("the reason, 文本") && log.contains("backtrace:"), "{log}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_file_stays_small() {
        let dir = std::env::temp_dir().join(format!("dotany-crash-cap-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let file = dir.join("crash.log");
        for i in 0..400 {
            append(&file, &format!("{MARK}{i}\n{}\n", "z".repeat(1000)));
        }
        let log = std::fs::read_to_string(&file).unwrap();
        assert!(log.len() <= CAP && log.len() > CAP - 2000, "{}", log.len());
        assert!(log.starts_with(MARK) && log.contains(&format!("{MARK}399\n")) && !log.contains(&format!("{MARK}1\n")));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_date() {
        let s = now();
        assert!(s.len() == 20 && s.ends_with('Z') && s.starts_with("20"), "{s}");
    }
}
