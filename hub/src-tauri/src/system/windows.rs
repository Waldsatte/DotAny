//! What DotAny asks of Windows: starting with Windows, the right-click menu, which program opens a kind of file, the
//! version written into each tool's exe, and one DotAny at a time. Only the current user's part of the registry is
//! ever written, and only keys of our own, so turning a switch off takes away exactly what turning it on added.
//!
//! These variables point the program at throwaway keys under HKCU, for an automated test: DOTANY_HUB_RUN_KEY (Run), DOTANY_HUB_SHELL_ROOT (Software\Classes for
//! the menu), DOTANY_HUB_ASSOC_ROOT (where file types are looked up). With DOTANY_TEST set and no key given, the real
//! keys are never touched.

use std::path::{Path, PathBuf};
use windows_sys::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, ERROR_SUCCESS, GetLastError, HANDLE};
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CLASSES_ROOT, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey,
    RegCreateKeyExW, RegDeleteTreeW, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

fn testing() -> bool {
    std::env::var_os("DOTANY_TEST").is_some()
}

/// A key from the environment, else the real one; in a test without one, a harmless key of our own.
fn key_or(env: &str, real: &str, test: &str) -> (String, bool) {
    match std::env::var(env) {
        Ok(k) if !k.is_empty() => (k, false),
        _ if testing() => (test.to_string(), false),
        _ => (real.to_string(), true),
    }
}

// ---------- the registry, as little of it as we need ----------

pub fn read(root: HKEY, path: &str, name: Option<&str>) -> Option<String> {
    let mut key: HKEY = std::ptr::null_mut();
    let p = wide(path);
    unsafe {
        if RegOpenKeyExW(root, p.as_ptr(), 0, KEY_READ, &mut key) != ERROR_SUCCESS {
            return None;
        }
        let n = name.map(wide);
        let mut buf = vec![0u16; 2048];
        let mut len = (buf.len() * 2) as u32;
        let mut kind = 0u32;
        let r = RegQueryValueExW(
            key,
            n.as_ref().map_or(std::ptr::null(), |n| n.as_ptr()),
            std::ptr::null(),
            &mut kind,
            buf.as_mut_ptr() as *mut u8,
            &mut len,
        );
        RegCloseKey(key);
        if r != ERROR_SUCCESS {
            return None;
        }
        let n = (len as usize / 2).min(buf.len());
        Some(String::from_utf16_lossy(&buf[..n]).trim_end_matches('\0').to_string())
    }
}

fn write(path: &str, name: Option<&str>, value: &str) -> bool {
    let mut key: HKEY = std::ptr::null_mut();
    let p = wide(path);
    unsafe {
        if RegCreateKeyExW(
            HKEY_CURRENT_USER,
            p.as_ptr(),
            0,
            std::ptr::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            std::ptr::null(),
            &mut key,
            std::ptr::null_mut(),
        ) != ERROR_SUCCESS
        {
            return false;
        }
        let v = wide(value);
        let n = name.map(wide);
        let ok = RegSetValueExW(
            key,
            n.as_ref().map_or(std::ptr::null(), |n| n.as_ptr()),
            0,
            REG_SZ,
            v.as_ptr() as *const u8,
            (v.len() * 2) as u32,
        ) == ERROR_SUCCESS;
        RegCloseKey(key);
        ok
    }
}

fn delete_value(path: &str, name: &str) {
    let mut key: HKEY = std::ptr::null_mut();
    let p = wide(path);
    unsafe {
        if RegOpenKeyExW(HKEY_CURRENT_USER, p.as_ptr(), 0, KEY_WRITE, &mut key) == ERROR_SUCCESS {
            let n = wide(name);
            RegDeleteValueW(key, n.as_ptr());
            RegCloseKey(key);
        }
    }
}

fn delete_tree(path: &str) {
    let p = wide(path);
    unsafe {
        RegDeleteTreeW(HKEY_CURRENT_USER, p.as_ptr());
    }
}

fn exe() -> PathBuf {
    std::env::current_exe().unwrap_or_default()
}

// ---------- starting with Windows: DotAny only, never the tools ----------

const RUN_NAME: &str = "DotAny";

fn run_key() -> String {
    key_or("DOTANY_HUB_RUN_KEY", r"Software\Microsoft\Windows\CurrentVersion\Run", r"Software\DotAnyTest\Run").0
}

pub fn autostart() -> bool {
    read(HKEY_CURRENT_USER, &run_key(), Some(RUN_NAME)).is_some()
}

pub fn set_autostart(on: bool) -> bool {
    let key = run_key();
    if on {
        write(&key, Some(RUN_NAME), &format!("\"{}\" --autostart", exe().display()))
    } else {
        delete_value(&key, RUN_NAME);
        true
    }
}

// ---------- the right-click menu: Convert with DotAny on the kinds of file it can convert ----------

const VERB: &str = "DotAnyConvert";

fn shell_root() -> (String, bool) {
    key_or("DOTANY_HUB_SHELL_ROOT", r"Software\Classes", r"Software\DotAnyTest\Classes")
}

fn verb_keys(exts: &[&str]) -> Vec<String> {
    let (root, _) = shell_root();
    exts.iter().map(|e| format!(r"{root}\SystemFileAssociations\.{e}\shell\{VERB}")).collect()
}

fn notify_shell(real: bool) {
    use windows_sys::Win32::UI::Shell::{SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify};
    if real {
        unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED as i32, SHCNF_IDLIST, std::ptr::null(), std::ptr::null()) };
    }
}

pub fn shell_menu(exts: &[&str]) -> bool {
    verb_keys(exts).first().is_some_and(|k| read(HKEY_CURRENT_USER, &format!(r"{k}\command"), None).is_some())
}

/// Adds the verb in the interface's language; several files chosen together go to one window.
pub fn set_shell_menu(exts: &[&str], on: bool, label: &str) -> bool {
    let (_, real) = shell_root();
    let mut ok = true;
    for k in verb_keys(exts) {
        if on {
            ok &= write(&k, Some("MUIVerb"), label);
            ok &= write(&k, Some("Icon"), &format!("\"{}\",0", exe().display()));
            ok &= write(&k, Some("MultiSelectModel"), "Player");
            ok &= write(&format!(r"{k}\command"), None, &format!("\"{}\" --convert \"%1\"", exe().display()));
        } else {
            delete_tree(&k);
        }
    }
    notify_shell(real);
    ok
}

// ---------- which program opens a kind of file: Windows decides, and DotAny only looks ----------

fn is_ours(s: &str) -> bool {
    let s = s.to_ascii_lowercase();
    s.contains("dotany")
}

/// Whether a DotAny program opens `.ext` now: the user's own choice first, then the system's.
pub fn opens_with_dotany(ext: &str) -> bool {
    if let Ok(root) = std::env::var("DOTANY_HUB_ASSOC_ROOT") {
        let Some(prog) = read(HKEY_CURRENT_USER, &format!(r"{root}\.{ext}"), None) else { return false };
        return is_ours(&prog)
            || read(HKEY_CURRENT_USER, &format!(r"{root}\{prog}\shell\open\command"), None)
                .is_some_and(|c| is_ours(&c));
    }
    if testing() {
        return false;
    }
    let prog = read(
        HKEY_CURRENT_USER,
        &format!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\.{ext}\UserChoice"),
        Some("ProgId"),
    )
    .or_else(|| read(HKEY_CLASSES_ROOT, &format!(".{ext}"), None));
    let Some(prog) = prog.filter(|p| !p.is_empty()) else { return false };
    is_ours(&prog) || read(HKEY_CLASSES_ROOT, &format!(r"{prog}\shell\open\command"), None).is_some_and(|c| is_ours(&c))
}

/// Windows' own Default apps page, where the person confirms: one program's page when `app` names it (as it is
/// registered in RegisteredApplications), else the whole list. Windows shows the whole list by itself for a name it
/// doesn't know, and on a build that doesn't take the name. Tests only learn what would have opened.
pub fn open_default_apps(app: Option<&str>) -> String {
    let page = default_apps_page(app);
    if !testing() {
        open_link(&page);
    }
    page
}

fn default_apps_page(app: Option<&str>) -> String {
    match app {
        Some(name) => format!("ms-settings:defaultapps?registeredAppUser={}", escape(name)),
        None => "ms-settings:defaultapps".to_string(),
    }
}

/// A value in a link's query: letters, digits and - . _ ~ as they are, every other byte as %XX ("DotAny 3D" is
/// "DotAny%203D").
fn escape(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

/// Opened as a link, the way Settings takes a page and its query. Handed to explorer.exe instead, the query is lost and
/// a Settings window already open doesn't move.
fn open_link(uri: &str) {
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    let (verb, file) = (wide("open"), wide(uri));
    unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            file.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        );
    }
}

// ---------- the version written into a tool's exe ----------

pub fn version_of(path: &Path) -> Option<String> {
    use windows_sys::Win32::Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VS_FIXEDFILEINFO, VerQueryValueW,
    };
    let p: Vec<u16> = path.as_os_str().to_string_lossy().encode_utf16().chain(Some(0)).collect();
    unsafe {
        let size = GetFileVersionInfoSizeW(p.as_ptr(), std::ptr::null_mut());
        if size == 0 {
            return None;
        }
        let mut buf = vec![0u8; size as usize];
        if GetFileVersionInfoW(p.as_ptr(), 0, size, buf.as_mut_ptr() as *mut _) == 0 {
            return None;
        }
        let mut info: *mut core::ffi::c_void = std::ptr::null_mut();
        let mut len = 0u32;
        let root = wide("\\");
        if VerQueryValueW(buf.as_ptr() as *const _, root.as_ptr(), &mut info, &mut len) == 0 || info.is_null() {
            return None;
        }
        let f = &*(info as *const VS_FIXEDFILEINFO);
        Some(format!("{}.{}.{}", f.dwFileVersionMS >> 16, f.dwFileVersionMS & 0xffff, f.dwFileVersionLS >> 16))
    }
}

// ---------- one DotAny at a time ----------

/// Held for as long as this DotAny runs. A second start hands its arguments over and leaves.
pub struct Single {
    _mutex: HANDLE,
    pub wake: HANDLE,
}
unsafe impl Send for Single {}
unsafe impl Sync for Single {}

fn names(data_dir: &Path) -> (Vec<u16>, Vec<u16>) {
    // one per data folder, so a test never meets the DotAny someone is using
    let key = data_dir.to_string_lossy().to_lowercase();
    let mut h: u64 = 0xcbf29ce484222325;
    for b in key.bytes() {
        h = (h ^ b as u64).wrapping_mul(0x100000001b3);
    }
    (wide(&format!("Local\\DotAnyHub-{h:016x}")), wide(&format!("Local\\DotAnyHubWake-{h:016x}")))
}

/// Ok when this is the first DotAny; Err when another one runs (and has been woken with what this one was asked).
pub fn single(data_dir: &Path, inbox: &Path, args: &[String]) -> Result<Single, ()> {
    use windows_sys::Win32::System::Threading::{CreateEventW, CreateMutexW, SetEvent};
    let (m, e) = names(data_dir);
    unsafe {
        let mutex = CreateMutexW(std::ptr::null(), 1, m.as_ptr());
        let first = GetLastError() != ERROR_ALREADY_EXISTS;
        let wake = CreateEventW(std::ptr::null(), 0, 0, e.as_ptr());
        if first {
            return Ok(Single { _mutex: mutex, wake });
        }
        let _ = std::fs::create_dir_all(inbox);
        let name = format!("{}-{}.json", dotany_base::family::now_ms(), std::process::id());
        let tmp = inbox.join(format!("~{name}"));
        if std::fs::write(&tmp, serde_json::to_vec(args).unwrap_or_default()).is_ok() {
            let _ = std::fs::rename(&tmp, inbox.join(name));
        }
        allow_front();
        SetEvent(wake);
        CloseHandle(wake);
        CloseHandle(mutex);
        Err(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tools_own_default_apps_page() {
        assert_eq!(default_apps_page(None), "ms-settings:defaultapps");
        assert_eq!(default_apps_page(Some("DotAny Text")), "ms-settings:defaultapps?registeredAppUser=DotAny%20Text");
        assert_eq!(default_apps_page(Some("DotAny 3D")), "ms-settings:defaultapps?registeredAppUser=DotAny%203D");
        assert_eq!(escape("a&b=c/d"), "a%26b%3Dc%2Fd");
        assert_eq!(escape("文"), "%E6%96%87");
    }
}

/// Waits for the next start to knock; true when one did.
pub fn wait_for_knock(s: &Single) -> bool {
    use windows_sys::Win32::System::Threading::{INFINITE, WaitForSingleObject};
    unsafe { WaitForSingleObject(s.wake, INFINITE) == 0 }
}

/// The DotAny already running may bring its window to the front: the one knocking has the focus to give.
fn allow_front() {
    use windows_sys::Win32::UI::WindowsAndMessaging::{ASFW_ANY, AllowSetForegroundWindow};
    unsafe {
        AllowSetForegroundWindow(ASFW_ANY);
    }
}
