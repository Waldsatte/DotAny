//! The system right-click menu: only for this user (HKCU\Software\Classes), only when the user turns it on in settings,
//! and only these keys of our own, so turning it off takes away exactly what was added. Archives get "open"; any file or
//! folder gets "compress". Windows 11 shows both under "Show more options".
//! Tests point DOTANY_ARCHIVE_SHELL_ROOT at a throwaway key under HKCU instead of Software\Classes.

use std::path::Path;
use windows_sys::Win32::Foundation::ERROR_SUCCESS;
use windows_sys::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ, RegCloseKey, RegCreateKeyExW,
    RegDeleteTreeW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};
use windows_sys::Win32::UI::Shell::{SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify};

pub const ARCHIVES: &[&str] = &[
    "zip", "7z", "rar", "tar", "gz", "tgz", "bz2", "tbz2", "tbz", "xz", "txz", "zst", "tzst", "lzh", "lha", "cpio",
    "ar", "deb", "rpm",
];
const OPEN: &str = "DotAnyArchive.Open";
const PACK: &str = "DotAnyArchive.Pack";

fn root() -> (String, bool) {
    match std::env::var("DOTANY_ARCHIVE_SHELL_ROOT") {
        Ok(r) if !r.is_empty() => (r, false),
        _ => ("Software\\Classes".to_string(), true),
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// Every key this program adds, with its label and command.
fn keys(exe: &Path, open_label: &str, pack_label: &str) -> Vec<(String, String, String)> {
    let (root, _) = root();
    let exe = exe.to_string_lossy();
    let mut out: Vec<(String, String, String)> = ARCHIVES
        .iter()
        .map(|e| {
            (
                format!("{root}\\SystemFileAssociations\\.{e}\\shell\\{OPEN}"),
                open_label.to_string(),
                format!("\"{exe}\" \"%1\""),
            )
        })
        .collect();
    for kind in ["*", "Directory"] {
        out.push((
            format!("{root}\\{kind}\\shell\\{PACK}"),
            pack_label.to_string(),
            format!("\"{exe}\" --pack \"%1\""),
        ));
    }
    out
}

unsafe fn set(key: HKEY, name: Option<&str>, value: &str) -> bool {
    let v = wide(value);
    let n = name.map(wide);
    RegSetValueExW(
        key,
        n.as_ref().map_or(std::ptr::null(), |n| n.as_ptr()),
        0,
        REG_SZ,
        v.as_ptr() as *const u8,
        (v.len() * 2) as u32,
    ) == ERROR_SUCCESS
}

unsafe fn create(path: &str) -> Option<HKEY> {
    let mut key: HKEY = std::ptr::null_mut();
    let p = wide(path);
    (RegCreateKeyExW(
        HKEY_CURRENT_USER,
        p.as_ptr(),
        0,
        std::ptr::null(),
        REG_OPTION_NON_VOLATILE,
        KEY_WRITE,
        std::ptr::null(),
        &mut key,
        std::ptr::null_mut(),
    ) == ERROR_SUCCESS)
        .then_some(key)
}

unsafe fn read(path: &str) -> Option<String> {
    let mut key: HKEY = std::ptr::null_mut();
    let p = wide(path);
    if RegOpenKeyExW(HKEY_CURRENT_USER, p.as_ptr(), 0, KEY_READ, &mut key) != ERROR_SUCCESS {
        return None;
    }
    let mut buf = vec![0u16; 2048];
    let mut len = (buf.len() * 2) as u32;
    let mut kind = 0u32;
    let r = RegQueryValueExW(key, std::ptr::null(), std::ptr::null(), &mut kind, buf.as_mut_ptr() as *mut u8, &mut len);
    RegCloseKey(key);
    if r != ERROR_SUCCESS {
        return None;
    }
    let n = (len as usize / 2).min(buf.len());
    Some(String::from_utf16_lossy(&buf[..n]).trim_end_matches('\0').to_string())
}

fn notify(real: bool) {
    if real {
        unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED as i32, SHCNF_IDLIST, std::ptr::null(), std::ptr::null()) };
    }
}

/// Adds the two verbs, in the interface's language.
pub fn register(exe: &Path, open_label: &str, pack_label: &str) -> bool {
    let mut ok = true;
    for (path, label, command) in keys(exe, open_label, pack_label) {
        unsafe {
            match create(&path) {
                Some(k) => {
                    ok &= set(k, None, &label);
                    ok &= set(k, Some("Icon"), &format!("\"{}\",0", exe.to_string_lossy()));
                    RegCloseKey(k);
                }
                None => ok = false,
            }
            match create(&format!("{path}\\command")) {
                Some(k) => {
                    ok &= set(k, None, &command);
                    RegCloseKey(k);
                }
                None => ok = false,
            }
        }
    }
    notify(root().1);
    ok
}

/// Takes away exactly the keys `register` adds, and nothing else.
pub fn unregister() -> bool {
    let mut ok = true;
    for (path, _, _) in keys(Path::new(""), "", "") {
        let p = wide(&path);
        let r = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, p.as_ptr()) };
        let gone = unsafe { read(&path) }.is_none();
        ok &= r == ERROR_SUCCESS || gone;
    }
    notify(root().1);
    ok
}

/// On when every key is there and points at this program.
pub fn registered(exe: &Path) -> bool {
    keys(exe, "", "")
        .iter()
        .all(|(path, _, command)| unsafe { read(&format!("{path}\\command")) }.as_deref() == Some(command.as_str()))
}
