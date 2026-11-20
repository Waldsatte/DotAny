//! The two things this tool asks of the system itself: open a file in its usual program, and show a folder.

#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
use std::path::Path;
#[cfg(windows)]
use windows_sys::Win32::UI::Shell::ShellExecuteW;
#[cfg(windows)]
use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

#[cfg(windows)]
fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
    s.encode_wide().chain(Some(0)).collect()
}

#[cfg(windows)]
fn shell(verb: &str, target: &Path) -> bool {
    let v: Vec<u16> = verb.encode_utf16().chain(Some(0)).collect();
    let t = wide(target.as_os_str());
    // above 32 is success
    unsafe {
        ShellExecuteW(std::ptr::null_mut(), v.as_ptr(), t.as_ptr(), std::ptr::null(), std::ptr::null(), SW_SHOWNORMAL)
            as isize
            > 32
    }
}

/// The file in the program Windows uses for it.
#[cfg(windows)]
pub fn open_with_default(file: &Path) -> bool {
    shell("open", file)
}

/// The folder in File Explorer.
#[cfg(windows)]
pub fn show_folder(dir: &Path) -> bool {
    dir.is_dir() && shell("explore", dir)
}

/// The Mac's `open`: the file in the program the Mac uses for it, a folder in the Finder. It asks the system, as a
/// double-click in the Finder does, and comes back at once.
#[cfg(target_os = "macos")]
fn open(target: &Path) -> bool {
    std::process::Command::new("/usr/bin/open").arg(target).status().is_ok_and(|s| s.success())
}

/// The file in the program the Mac uses for it.
#[cfg(target_os = "macos")]
pub fn open_with_default(file: &Path) -> bool {
    open(file)
}

/// Show in Finder: the Finder opens the folder that holds it, with it chosen, as every Mac program's Show in Finder
/// does (`NSWorkspace.activateFileViewerSelectingURLs`).
#[cfg(target_os = "macos")]
pub fn show_folder(dir: &Path) -> bool {
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::{NSArray, NSString, NSURL};
    if !dir.exists() {
        return false;
    }
    let url = NSURL::fileURLWithPath(&NSString::from_str(&dir.to_string_lossy()));
    NSWorkspace::sharedWorkspace().activateFileViewerSelectingURLs(&NSArray::from_retained_slice(&[url]));
    true
}

/// A name as the Mac's own programs write it for others: composed (NFC). The Mac's disks and libarchive's RAR reader
/// hand names over decomposed (NFD: é as e and a separate accent), which Windows shows as two letters and doesn't find
/// by the name typed; DotAny shows and writes every name composed.
#[cfg(target_os = "macos")]
pub fn nfc(name: &str) -> String {
    if name.is_ascii() {
        return name.to_string();
    }
    #[allow(unused_unsafe)]
    let composed = unsafe { objc2_foundation::NSString::from_str(name).precomposedStringWithCanonicalMapping() };
    composed.to_string()
}

/// What the Mac leaves beside a person's files and no one wants packed: the Finder's `.DS_Store` and the `._` files that
/// carry a file's extended attributes on disks that can't (and in zips made by the Finder, under `__MACOSX`).
#[cfg(target_os = "macos")]
pub fn mac_litter(name: &str) -> bool {
    name == ".DS_Store" || name.starts_with("._")
}

/// A zip made on a Mac carries `__MACOSX/`, the extended attributes of its files as `._` files: not the person's files,
/// so never listed or extracted.
#[cfg(target_os = "macos")]
pub fn mac_attributes(path: &str) -> bool {
    path.split(['/', '\\']).next() == Some("__MACOSX")
}
