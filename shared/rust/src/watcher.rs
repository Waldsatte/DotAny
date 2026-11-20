//! Notices when another program touches the open file. Watches the folder, because many programs save by
//! replacing the file; bursts of events settle for a moment before the file is looked at.

use crate::atomic::TEMP_PREFIX;
use notify::{Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

const SETTLE: Duration = Duration::from_millis(120);

pub struct FileWatch {
    _watcher: RecommendedWatcher,
}

pub fn watch(path: &Path, on_change: impl Fn() + Send + 'static) -> Option<FileWatch> {
    let dir = path.parent()?.to_path_buf();
    let name = path.file_name()?.to_os_string();
    let (tx, rx) = mpsc::channel::<()>();
    let mut watcher = RecommendedWatcher::new(
        move |res: notify::Result<notify::Event>| {
            let Ok(event) = res else { return };
            if matches!(event.kind, EventKind::Access(_)) {
                return;
            }
            let concerns = event.paths.iter().any(|p: &PathBuf| match p.file_name() {
                Some(n) => !n.to_string_lossy().starts_with(TEMP_PREFIX) && crate::platform::same_name(n, &name),
                None => false,
            });
            if concerns {
                let _ = tx.send(());
            }
        },
        Config::default(),
    )
    .ok()?;
    watcher.watch(&dir, RecursiveMode::NonRecursive).ok()?;
    // ends when the watcher is dropped and the channel closes
    std::thread::spawn(move || {
        while rx.recv().is_ok() {
            while rx.recv_timeout(SETTLE).is_ok() {}
            on_change();
        }
    });
    Some(FileWatch { _watcher: watcher })
}
