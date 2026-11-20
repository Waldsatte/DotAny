//! The places on the computer and the few Windows calls the installer needs. Tests move every place with an
//! environment variable, so they never touch the real Start menu, settings or Programs folder.

use std::io;
use std::path::{Path, PathBuf};

fn env_path(name: &str) -> Option<PathBuf> {
    std::env::var_os(name).filter(|v| !v.is_empty()).map(PathBuf::from)
}

/// Per user, no administrator prompt: `%LOCALAPPDATA%\Programs\DotAny`, where VS Code, Discord and others put theirs.
pub fn default_root() -> PathBuf {
    if let Some(p) = env_path("DOTANY_SETUP_ROOT") {
        return p;
    }
    let local = env_path("LOCALAPPDATA").unwrap_or_else(std::env::temp_dir);
    local.join("Programs").join("DotAny")
}

/// The Start menu's own folder for DotAny.
pub fn start_menu() -> PathBuf {
    if let Some(p) = env_path("DOTANY_SETUP_START_MENU") {
        return p.join("DotAny");
    }
    let roaming = env_path("APPDATA").unwrap_or_else(std::env::temp_dir);
    roaming.join(r"Microsoft\Windows\Start Menu\Programs\DotAny")
}

/// Where every DotAny tool keeps its settings, recent files and WebView data: `%APPDATA%\DotAny`.
pub fn settings_dir() -> PathBuf {
    env_path("DOTANY_SETUP_APPDATA").or_else(|| env_path("APPDATA")).unwrap_or_else(std::env::temp_dir).join("DotAny")
}

/// A Start menu shortcut.
pub fn shortcut(lnk: &Path, target: &Path) -> io::Result<()> {
    let mut link = mslnk::ShellLink::new(target).map_err(|e| io::Error::other(e.to_string()))?;
    link.set_working_dir(target.parent().map(|p| p.display().to_string()));
    link.set_icon_location(Some(target.display().to_string()));
    if let Some(dir) = lnk.parent() {
        std::fs::create_dir_all(dir)?;
    }
    link.create_lnk(lnk).map_err(|e| io::Error::other(e.to_string()))
}

/// Free space on the drive that `path` is on (or would be on once its folders exist).
pub fn free_space(path: &Path) -> Option<u64> {
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let mut probe = Some(path);
    while let Some(p) = probe {
        if p.exists() {
            let wide: Vec<u16> = p.as_os_str().encode_wide_z();
            let mut free = 0u64;
            let ok =
                unsafe { GetDiskFreeSpaceExW(wide.as_ptr(), &mut free, std::ptr::null_mut(), std::ptr::null_mut()) };
            return (ok != 0).then_some(free);
        }
        probe = p.parent();
    }
    None
}

trait Wide {
    fn encode_wide_z(&self) -> Vec<u16>;
}
impl Wide for std::ffi::OsStr {
    fn encode_wide_z(&self) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        self.encode_wide().chain(Some(0)).collect()
    }
}

/// Programs running from inside `root`, by process id and path.
pub fn running_under(root: &Path) -> Vec<(u32, PathBuf)> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
    };
    let Ok(root) = std::fs::canonicalize(root) else { return vec![] };
    let mut out = Vec::new();
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return out;
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut more = Process32FirstW(snap, &mut entry) != 0;
        while more {
            let pid = entry.th32ProcessID;
            let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
            if !handle.is_null() {
                let mut buf = [0u16; 1024];
                let mut len = buf.len() as u32;
                if QueryFullProcessImageNameW(handle, 0, buf.as_mut_ptr(), &mut len) != 0 {
                    let path = PathBuf::from(String::from_utf16_lossy(&buf[..len as usize]));
                    if std::fs::canonicalize(&path).map(|p| p.starts_with(&root)).unwrap_or(false) {
                        out.push((pid, path));
                    }
                }
                CloseHandle(handle);
            }
            more = Process32NextW(snap, &mut entry) != 0;
        }
        CloseHandle(snap);
    }
    out
}

/// Is this process still there?
pub fn is_running(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return false;
        }
        let mut code = 0u32;
        let alive = GetExitCodeProcess(handle, &mut code) != 0 && code == STILL_ACTIVE as u32;
        CloseHandle(handle);
        alive
    }
}

/// A process, opened to be ended: while this handle is open, Windows gives its number to no other process.
fn open_to_end(pid: u32) -> Option<windows_sys::Win32::Foundation::HANDLE> {
    use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE};
    let handle = unsafe { OpenProcess(PROCESS_TERMINATE | PROCESS_SYNCHRONIZE, 0, pid) };
    (!handle.is_null()).then_some(handle)
}

/// Ends an opened process and waits for it to go; the handle stays open.
fn end_opened(handle: windows_sys::Win32::Foundation::HANDLE) {
    use windows_sys::Win32::System::Threading::{TerminateProcess, WaitForSingleObject};
    unsafe {
        TerminateProcess(handle, 0);
        WaitForSingleObject(handle, 3000);
    }
}

/// Ends one process and waits for it to go.
fn terminate(pid: u32) {
    if let Some(handle) = open_to_end(pid) {
        end_opened(handle);
        unsafe { windows_sys::Win32::Foundation::CloseHandle(handle) };
    }
}

/// When a process started, in Windows' file time: None when it can't be asked.
fn started(pid: u32) -> Option<u64> {
    use windows_sys::Win32::Foundation::{CloseHandle, FILETIME};
    use windows_sys::Win32::System::Threading::{GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return None;
        }
        let mut t: [FILETIME; 4] = std::mem::zeroed();
        let [created, exited, kernel, user] = &mut t;
        let ok = GetProcessTimes(handle, created, exited, kernel, user) != 0;
        CloseHandle(handle);
        ok.then(|| ((t[0].dwHighDateTime as u64) << 32) | t[0].dwLowDateTime as u64)
    }
}

/// Every process started under `pid`, at any depth, with its file name: a DotAny program's WebView2 processes.
/// A process keeps the number of a parent that has ended, and Windows soon gives that number to another process: one
/// that started before `pid` (or before the process it seems to hang under) is some other program's, never ours to end.
fn descendants(pid: u32) -> Vec<(u32, String)> {
    use windows_sys::Win32::Foundation::{CloseHandle, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW, TH32CS_SNAPPROCESS,
    };
    let mut all: Vec<(u32, u32, String)> = Vec::new();
    unsafe {
        let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
        if snap == INVALID_HANDLE_VALUE {
            return Vec::new();
        }
        let mut entry: PROCESSENTRY32W = std::mem::zeroed();
        entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
        let mut more = Process32FirstW(snap, &mut entry) != 0;
        while more {
            let n = entry.szExeFile.iter().position(|c| *c == 0).unwrap_or(entry.szExeFile.len());
            all.push((entry.th32ProcessID, entry.th32ParentProcessID, String::from_utf16_lossy(&entry.szExeFile[..n])));
            more = Process32NextW(snap, &mut entry) != 0;
        }
        CloseHandle(snap);
    }
    let mut out: Vec<(u32, String)> = Vec::new();
    let mut parents = vec![(pid, started(pid))];
    while let Some((p, since)) = parents.pop() {
        for (child, parent, name) in &all {
            if *parent != p || *child == pid || out.iter().any(|(c, _)| c == child) {
                continue;
            }
            let child_since = started(*child);
            if matches!((child_since, since), (Some(c), Some(s)) if c < s) {
                continue;
            }
            out.push((*child, name.clone()));
            parents.push((*child, child_since));
        }
    }
    out
}

/// Asks a DotAny program's windows to close, as their close button does: the program then ends by itself, its
/// WebView2 with it. Answers how many windows were asked.
fn ask_to_close(pid: u32) -> usize {
    use windows_sys::Win32::Foundation::{BOOL, HWND, LPARAM};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClassNameW, GetWindowThreadProcessId, PostMessageW, WM_CLOSE,
    };
    struct Ask {
        pid: u32,
        asked: usize,
    }
    unsafe extern "system" fn each(hwnd: HWND, data: LPARAM) -> BOOL {
        let ask = &mut *(data as *mut Ask);
        let mut owner = 0u32;
        GetWindowThreadProcessId(hwnd, &mut owner);
        if owner == ask.pid {
            let mut class = [0u16; 64];
            let n = GetClassNameW(hwnd, class.as_mut_ptr(), class.len() as i32).max(0) as usize;
            // the program's own windows, not the helper windows its runtime makes
            if String::from_utf16_lossy(&class[..n]) == "Tauri Window" && PostMessageW(hwnd, WM_CLOSE, 0, 0) != 0 {
                ask.asked += 1;
            }
        }
        1
    }
    let mut ask = Ask { pid, asked: 0 };
    unsafe { EnumWindows(Some(each), &mut ask as *mut Ask as LPARAM) };
    ask.asked
}

/// Ends a DotAny program the way that gives everything back: first it is asked to close; if it hasn't gone within
/// `wait`, the program is ended and then every process under it (its WebView2). Ending only the program and leaving its
/// WebView2 to go by itself leaves some of Windows' committed memory held until the next restart. Used for every
/// program the installer ends.
pub fn end(pid: u32, wait: std::time::Duration) {
    let tree = descendants(pid);
    if ask_to_close(pid) > 0 {
        let until = std::time::Instant::now() + wait;
        while is_running(pid) && std::time::Instant::now() < until {
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
    if is_running(pid) {
        // The program before its WebView2: a DotAny program whose WebView2 browser ends while it still runs makes its
        // window again (shared/rust's webview.rs), with a new browser that would be nobody's once the program is gone,
        // and would stay. Held open meanwhile, its number stays its own while what is under it is read (again: the
        // tree may have grown while it was asked).
        let held = open_to_end(pid);
        if let Some(handle) = held {
            end_opened(handle);
        }
        for (child, _) in descendants(pid) {
            terminate(child);
        }
        if let Some(handle) = held {
            unsafe { windows_sys::Win32::Foundation::CloseHandle(handle) };
        }
    }
    // its WebView2 normally goes with it; any left behind goes now (only WebView2's own, in case a number was reused)
    std::thread::sleep(std::time::Duration::from_millis(300));
    for (child, name) in tree {
        if name.eq_ignore_ascii_case("msedgewebview2.exe") && is_running(child) {
            terminate(child);
        }
    }
}

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Removes a folder a moment after this program has gone: a running program can't delete itself, and WebView2 lets go
/// of its folder a little after the window closes.
pub fn remove_after_exit(dir: &Path) {
    use std::os::windows::process::CommandExt;
    let line = format!("ping -n 4 127.0.0.1 >nul & rmdir /s /q \"{}\"", dir.display());
    let _ = std::process::Command::new("cmd.exe")
        .raw_arg("/d /c \"")
        .raw_arg(&line)
        .raw_arg("\"")
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
}

/// What a program started by a test's installer (DOTANY_SETUP_HIVE set) is told, unless the test told it already, so
/// it writes only to the test's own keys and folders: its Run value and right-click menu go under `<hive>-hub`, its data
/// into the test's settings folder. Without this, a hub started from a test's install would read and write the real Run
/// key and the person's own data folder.
fn test_env() -> Vec<(&'static str, String)> {
    let hive = std::env::var("DOTANY_SETUP_HIVE").unwrap_or_default();
    let hive = hive.trim_matches('\\');
    if hive.is_empty() {
        return vec![];
    }
    let wanted = [
        ("DOTANY_TEST", "1".to_string()),
        ("DOTANY_HUB_RUN_KEY", format!(r"{hive}-hub\Run")),
        ("DOTANY_HUB_SHELL_ROOT", format!(r"{hive}-hub\Classes")),
        ("DOTANY_HUB_DATA", settings_dir().join("Hub").display().to_string()),
    ];
    wanted.into_iter().filter(|(k, _)| std::env::var_os(k).is_none()).collect()
}

/// Starts a program and lets it go. It gets none of this program's standard handles: whoever started the installer
/// and waits on its output (a script, a test) would otherwise wait for DotAny too, as long as it runs.
pub fn start(program: &Path, args: &[&str]) -> bool {
    use std::process::Stdio;
    // Windows hands a new program every handle marked inheritable, not only the three it is given: the handles this
    // program was started with stop being inheritable first
    use windows_sys::Win32::Foundation::{HANDLE_FLAG_INHERIT, SetHandleInformation};
    use windows_sys::Win32::System::Console::{GetStdHandle, STD_ERROR_HANDLE, STD_INPUT_HANDLE, STD_OUTPUT_HANDLE};
    for which in [STD_INPUT_HANDLE, STD_OUTPUT_HANDLE, STD_ERROR_HANDLE] {
        unsafe {
            let h = GetStdHandle(which);
            if !h.is_null() && h as isize != -1 {
                SetHandleInformation(h, HANDLE_FLAG_INHERIT, 0);
            }
        }
    }
    std::process::Command::new(program)
        .args(args)
        .envs(test_env())
        .current_dir(program.parent().unwrap_or(Path::new(".")))
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .is_ok()
}

/// Hides a folder of our own (the staging and old-files folders), so it doesn't catch the eye if left a moment.
pub fn hide(path: &Path) {
    use windows_sys::Win32::Storage::FileSystem::{FILE_ATTRIBUTE_HIDDEN, SetFileAttributesW};
    let wide = path.as_os_str().encode_wide_z();
    unsafe { SetFileAttributesW(wide.as_ptr(), FILE_ATTRIBUTE_HIDDEN) };
}

/// Fonts that just became (or stopped being) this person's Windows fonts: Windows is told for the running session, and
/// every program is told the font list changed. The registry values (`reg::FONTS`) are what lasts past signing out; this
/// only saves waiting for the next sign-in. `added` false lets go of the files, so they can be moved or deleted.
pub fn fonts_changed(files: &[PathBuf], added: bool) {
    use windows_sys::Win32::Graphics::Gdi::{AddFontResourceW, RemoveFontResourceW};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        HWND_BROADCAST, SMTO_ABORTIFHUNG, SendMessageTimeoutW, WM_FONTCHANGE,
    };
    if files.is_empty() {
        return;
    }
    for f in files {
        let wide = f.as_os_str().encode_wide_z();
        unsafe {
            if added {
                AddFontResourceW(wide.as_ptr());
            } else {
                // loaded more than once (an earlier install and the sign-in both), so let go until Windows says no more
                for _ in 0..8 {
                    if RemoveFontResourceW(wide.as_ptr()) == 0 {
                        break;
                    }
                }
            }
        }
    }
    unsafe { SendMessageTimeoutW(HWND_BROADCAST, WM_FONTCHANGE, 0, 0, SMTO_ABORTIFHUNG, 1000, std::ptr::null_mut()) };
}

/// Is Windows set to dark apps?
pub fn system_is_dark() -> bool {
    use winreg::enums::HKEY_CURRENT_USER;
    winreg::RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize")
        .and_then(|k| k.get_value::<u32, _>("AppsUseLightTheme"))
        .map(|v| v == 0)
        .unwrap_or(false)
}

/// Today, `YYYY-MM-DD`, and now as UTC `YYYY-MM-DDTHH:MM:SSZ`, without a date library.
pub fn today() -> String {
    now()[..10].to_string()
}
pub fn now() -> String {
    let secs =
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as i64;
    let (days, rest) = (secs.div_euclid(86400), secs.rem_euclid(86400));
    // civil from days, Howard Hinnant's algorithm
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", rest / 3600, rest % 3600 / 60, rest % 60)
}
