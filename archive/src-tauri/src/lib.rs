pub mod app;
mod extract;
mod pack;
mod platform;
#[cfg(windows)]
mod shellmenu;
// what's inside an archive and reading it back out: dotany-unpack (shared/unpack), which the hub's converter reads with too;
// and the archive's "came from the internet" mark, given to what is written out of it
use dotany_unpack::{mark, source};

/// The Mac's counterpart of the right-click menu: two Quick Actions in the Finder, Open with DotAny Archive for the
/// archives this tool reads and Compress with DotAny Archive for any file or folder (`dotany_base::quickaction`). They
/// hand the files over the way a drop on the Dock icon does. Compress hands over, for each chosen item, a small request
/// file of ours naming it (`pack_request`), so each goes into a new archive of its own in a new window, even an archive,
/// even one already open, as `--pack` does on Windows.
#[cfg(target_os = "macos")]
mod shellmenu {
    use dotany_base::quickaction::{QuickAction, add, present, remove, shell_quote, type_of_ext};
    use std::path::{Path, PathBuf};

    const ARCHIVES: &[&str] = &[
        "zip", "7z", "rar", "tar", "gz", "tgz", "bz2", "tbz2", "tbz", "xz", "txz", "zst", "tzst", "lzh", "lha", "cpio",
        "ar", "deb", "rpm",
    ];
    const OPEN: &str = "archive-open";
    const PACK: &str = "archive-pack";

    /// Where Compress writes its requests: `<n>.dotany-pack`, each naming one item.
    fn requests() -> PathBuf {
        dotany_base::settings::data_dir(&crate::app::TOOL).join("pack-requests")
    }

    /// A request Compress handed over: the item it names, and the request is gone. None for any other file.
    pub fn pack_request(path: &Path) -> Option<PathBuf> {
        // the same folder however it is spelt (the system hands /private/var/… for the /var/… of a temporary folder)
        let here = |p: &Path| std::fs::canonicalize(p).ok();
        if !path.extension().is_some_and(|e| e == "dotany-pack")
            || path.parent().and_then(here).is_none()
            || path.parent().and_then(here) != here(&requests())
        {
            return None;
        }
        let named = std::fs::read_to_string(path).ok()?;
        let _ = std::fs::remove_file(path);
        let named = named.trim_end_matches('\n');
        (!named.is_empty()).then(|| PathBuf::from(named))
    }

    pub fn register(_exe: &Path, open_label: &str, pack_label: &str) -> bool {
        let mut types: Vec<String> = ARCHIVES.iter().filter_map(|e| type_of_ext(e)).collect();
        types.sort();
        types.dedup();
        let dir = shell_quote(&requests().to_string_lossy());
        let before = format!(
            "/bin/mkdir -p {dir}\nreqs=()\nfor f in \"$@\"; do r={dir}/$(/usr/bin/uuidgen).dotany-pack; \
             print -r -- \"$f\" > \"$r\"; reqs+=(\"$r\"); done\nset -- \"${{reqs[@]}}\"\n"
        );
        let opened = add(&QuickAction { kind: OPEN, label: open_label, types, before: String::new() });
        let packed = add(&QuickAction { kind: PACK, label: pack_label, types: vec!["public.item".into()], before });
        if !(opened && packed) {
            unregister();
            return false;
        }
        true
    }

    /// Takes away exactly the two workflows `register` writes.
    pub fn unregister() -> bool {
        remove(OPEN) & remove(PACK)
    }

    pub fn registered(_exe: &Path) -> bool {
        present(OPEN) && present(PACK)
    }
}

#[cfg(not(target_os = "macos"))]
#[cfg(not(windows))]
mod shellmenu {
    use std::path::Path;

    pub fn register(_exe: &Path, _open_label: &str, _pack_label: &str) -> bool {
        false
    }

    pub fn unregister() -> bool {
        true
    }

    pub fn registered(_exe: &Path) -> bool {
        false
    }
}
