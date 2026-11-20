//! What DotAny asks of the Mac: opening at login (`SMAppService`), the Finder's Quick Action
//! Convert with DotAny (`dotany_base::quickaction`), which program opens a kind of file (Launch Services) and
//! choosing it (`NSWorkspace.setDefaultApplication`), a tool's version (its `Info.plist`), which tools run from
//! DotAny's folder (`NSWorkspace.runningApplications`).

use objc2::msg_send;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject};
use objc2_foundation::{NSString, NSURL};
use std::ffi::{c_char, c_void};
use std::path::{Path, PathBuf};

extern "C" {
    fn dlopen(path: *const c_char, mode: i32) -> *mut c_void;
}

/// Loads one of the system's frameworks into the program, once, so its classes can be found by name.
fn load(framework: &std::ffi::CStr) {
    // SAFETY: a NUL-terminated path; RTLD_LAZY. A framework already loaded is only counted again.
    unsafe { dlopen(framework.as_ptr(), 1) };
}

// ---------- opening at login: SMAppService.mainApp (macOS 13) ----------

/// The login item that is DotAny itself, as System Settings › General › Login Items (Login Items & Extensions since
/// macOS 15) lists it. None for a program not run from its `.app` (a build folder), which the system can't open at
/// login.
fn login_item() -> Option<Retained<AnyObject>> {
    dotany_base::quickaction::this_app()?;
    load(c"/System/Library/Frameworks/ServiceManagement.framework/ServiceManagement");
    let class = AnyClass::get(c"SMAppService")?;
    // SAFETY: a class method that answers an autoreleased object, retained here
    unsafe { msg_send![class, mainAppService] }
}

/// SMAppServiceStatus: 0 not registered, 1 enabled, 2 requires approval, 3 not found.
fn login_status() -> isize {
    // SAFETY: `status` is an NSInteger property of SMAppService
    login_item().map_or(0, |s| unsafe { msg_send![&*s, status] })
}

/// On when the system will open DotAny at login: what System Settings shows, read each time, so a switch turned off
/// there shows off here.
pub fn autostart() -> bool {
    login_status() == 1
}

pub fn set_autostart(on: bool) -> bool {
    let Some(s) = login_item() else { return false };
    let mut err: *mut AnyObject = std::ptr::null_mut();
    // SAFETY: both take an NSError** and answer a BOOL
    let done: bool = unsafe {
        if on {
            msg_send![&*s, registerAndReturnError: &mut err as *mut *mut AnyObject]
        } else {
            msg_send![&*s, unregisterAndReturnError: &mut err as *mut *mut AnyObject]
        }
    };
    done && autostart() == on
}

/// Whether the system opened DotAny because it is a login item: the "open application" Apple event that started it
/// says so (`keyAELaunchedAsLogInItem`). Asked as DotAny starts. With `DOTANY_TEST` set, `DOTANY_TEST_AT_LOGIN` makes it answer yes.
pub fn started_at_login() -> bool {
    if std::env::var_os("DOTANY_TEST").is_some() && std::env::var_os("DOTANY_TEST_AT_LOGIN").is_some() {
        return true;
    }
    let four = |s: &[u8; 4]| u32::from_be_bytes(*s);
    let Some(class) = AnyClass::get(c"NSAppleEventManager") else { return false };
    // SAFETY: AppKit's own singletons and descriptors; a missing one is nil, checked before use
    unsafe {
        let manager: *mut AnyObject = msg_send![class, sharedAppleEventManager];
        if manager.is_null() {
            return false;
        }
        let event: *mut AnyObject = msg_send![manager, currentAppleEvent];
        if event.is_null() {
            return false;
        }
        let id: u32 = msg_send![event, eventID];
        if id != four(b"oapp") {
            return false;
        }
        let prop: *mut AnyObject = msg_send![event, paramDescriptorForKeyword: four(b"prop")];
        if prop.is_null() {
            return false;
        }
        let value: u32 = msg_send![prop, enumCodeValue];
        value == four(b"lgit")
    }
}

// ---------- the Finder's Quick Action Convert with DotAny ----------

const CONVERT: &str = "hub-convert";

pub fn shell_menu(_exts: &[&str]) -> bool {
    dotany_base::quickaction::present(CONVERT)
}

/// Offered for the kinds of file the converter takes, by the system's names for them.
pub fn set_shell_menu(exts: &[&str], on: bool, label: &str) -> bool {
    use dotany_base::quickaction::{QuickAction, add, remove, type_of_ext};
    if !on {
        return remove(CONVERT);
    }
    let mut types: Vec<String> = exts.iter().filter_map(|e| type_of_ext(e)).collect();
    types.sort();
    types.dedup();
    add(&QuickAction { kind: CONVERT, label, types, before: String::new() })
}

// ---------- which program opens a kind of file: Launch Services says, and DotAny chooses ----------

type CFStringRef = *const c_void;
type CFArrayRef = *const c_void;

const UTF8: u32 = 0x0800_0100;
const ALL_ROLES: u32 = 0xFFFF_FFFF;

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFStringCreateWithBytes(
        alloc: *const c_void,
        bytes: *const u8,
        len: isize,
        encoding: u32,
        ext: u8,
    ) -> CFStringRef;
    fn CFStringGetCString(s: CFStringRef, buf: *mut c_char, size: isize, encoding: u32) -> u8;
    fn CFArrayGetCount(a: CFArrayRef) -> isize;
    fn CFArrayGetValueAtIndex(a: CFArrayRef, i: isize) -> *const c_void;
    fn CFRelease(cf: *const c_void);
}

#[link(name = "CoreServices", kind = "framework")]
extern "C" {
    fn UTTypeCreatePreferredIdentifierForTag(class: CFStringRef, tag: CFStringRef, within: CFStringRef) -> CFStringRef;
    fn LSCopyDefaultRoleHandlerForContentType(uti: CFStringRef, role: u32) -> CFStringRef;
    fn LSCopyAllRoleHandlersForContentType(uti: CFStringRef, role: u32) -> CFArrayRef;
}

/// A CoreFoundation object of our own, released when dropped.
struct Cf(*const c_void);

impl Cf {
    fn new(s: &str) -> Option<Cf> {
        // SAFETY: the bytes and their length; a null allocator is the default one
        let r = unsafe { CFStringCreateWithBytes(std::ptr::null(), s.as_ptr(), s.len() as isize, UTF8, 0) };
        (!r.is_null()).then_some(Cf(r))
    }
}

/// A CFString's text (one we are only lent, or our own).
fn text_of(s: CFStringRef) -> Option<String> {
    let mut buf = vec![0u8; 512];
    // SAFETY: a buffer of that size, which the call fills with a NUL-terminated string or refuses
    let ok = unsafe { CFStringGetCString(s, buf.as_mut_ptr() as *mut c_char, buf.len() as isize, UTF8) };
    if ok == 0 {
        return None;
    }
    let end = buf.iter().position(|b| *b == 0)?;
    String::from_utf8(buf[..end].to_vec()).ok()
}

impl Drop for Cf {
    fn drop(&mut self) {
        // SAFETY: an object this side made or was given to own (a "Create" or "Copy" call's)
        unsafe { CFRelease(self.0) };
    }
}

/// The system's type identifier for files ending `.ext`.
fn uti_of(ext: &str) -> Option<Cf> {
    let (class, tag) = (Cf::new("public.filename-extension")?, Cf::new(ext)?);
    // SAFETY: two strings of ours; what comes back is ours to release
    let uti = unsafe { UTTypeCreatePreferredIdentifierForTag(class.0, tag.0, std::ptr::null()) };
    (!uti.is_null()).then_some(Cf(uti))
}

/// The identifier of the program Launch Services opens `.ext` with now.
fn handler_of(ext: &str) -> Option<String> {
    let uti = uti_of(ext)?;
    // SAFETY: a type identifier of ours; what comes back, if anything, is ours to release
    let h = unsafe { LSCopyDefaultRoleHandlerForContentType(uti.0, ALL_ROLES) };
    if h.is_null() {
        return None;
    }
    let h = Cf(h);
    text_of(h.0)
}

/// Every program that says it opens `.ext`, by identifier.
fn handlers_of(ext: &str) -> Vec<String> {
    let Some(uti) = uti_of(ext) else { return Vec::new() };
    // SAFETY: as above; the array and its strings are the array's, read while it lives
    unsafe {
        let all = LSCopyAllRoleHandlersForContentType(uti.0, ALL_ROLES);
        if all.is_null() {
            return Vec::new();
        }
        let all = Cf(all);
        (0..CFArrayGetCount(all.0)).filter_map(|i| text_of(CFArrayGetValueAtIndex(all.0, i))).collect()
    }
}

fn is_ours(id: &str) -> bool {
    id.to_ascii_lowercase().starts_with("com.dotany.")
}

/// Whether a DotAny program opens `.ext` now: the program Launch Services gives that kind of file, by its bundle
/// identifier (every DotAny program's starts com.dotany.).
pub fn opens_with_dotany(ext: &str) -> bool {
    handler_of(ext).is_some_and(|id| is_ours(&id))
}

/// Windows sends the person to its own settings to choose; the Mac lets DotAny choose (`set_default`), and the page has
/// no button that would come here. Nothing is opened.
pub fn open_default_apps(_app: Option<&str>) -> String {
    String::new()
}

/// Where a program with this identifier is, as Launch Services knows it.
fn app_with_id(id: &str) -> Option<Retained<NSURL>> {
    let class = AnyClass::get(c"NSWorkspace")?;
    let id = NSString::from_str(id);
    // SAFETY: NSWorkspace's shared instance; the answer is nil or an NSURL
    unsafe {
        let ws: *mut AnyObject = msg_send![class, sharedWorkspace];
        msg_send![ws, URLForApplicationWithBundleIdentifier: &*id]
    }
}

/// Makes the program at `app` open `.ext`, as Get Info › Open With › Change All… does: `NSWorkspace`'s
/// `setDefaultApplicationAtURL:toOpenContentType:completionHandler:` (macOS 12), and waits for its answer, up to `wait`.
///
/// How the system behaves: giving a kind to one of DotAny's programs is answered at once; giving it
/// back to another maker's program (Preview) is answered only after the system has asked the person to confirm (its
/// dialog, CoreServicesUIAgent's), and Launch Services' older `LSSetDefaultRoleHandlerForContentType` is quietly
/// ignored for that. So turning DotAny off for a kind waits for the person's answer to the system's question; the page
/// then shows what the system says.
fn choose(app: &NSURL, ext: &str, wait: std::time::Duration) -> Result<(), String> {
    load(c"/System/Library/Frameworks/UniformTypeIdentifiers.framework/UniformTypeIdentifiers");
    let (Some(ws_class), Some(ut_class)) = (AnyClass::get(c"NSWorkspace"), AnyClass::get(c"UTType")) else {
        return Err("no NSWorkspace or UTType".into());
    };
    let ext = NSString::from_str(ext);
    let (tx, rx) = std::sync::mpsc::channel::<Result<(), String>>();
    let done = block2::RcBlock::new(move |err: *mut AnyObject| {
        let said = if err.is_null() {
            Ok(())
        } else {
            // SAFETY: an NSError the system hands the block, alive while it runs
            let text: Option<Retained<NSString>> = unsafe { msg_send![err, localizedDescription] };
            Err(text.map_or_else(|| "an error".into(), |t| t.to_string()))
        };
        let _ = tx.send(said);
    });
    // SAFETY: a UTType made from the extension (never nil: an undeclared one is a dynamic type), the program's file
    // URL, and a block that only sends on a channel, called once on a queue of the system's
    unsafe {
        let kind: *mut AnyObject = msg_send![ut_class, typeWithFilenameExtension: &*ext];
        if kind.is_null() {
            return Err("no type".into());
        }
        let ws: *mut AnyObject = msg_send![ws_class, sharedWorkspace];
        let _: () = msg_send![ws, setDefaultApplicationAtURL: app, toOpenContentType: kind, completionHandler: &*done];
    }
    rx.recv_timeout(wait).unwrap_or_else(|_| Err("no answer".into()))
}

/// What opened each kind before DotAny took it, kept so turning DotAny off gives it back: `{ "fbx": "com.apple.Preview" }`.
fn before_file(data_dir: &Path) -> PathBuf {
    data_dir.join("default-apps-before.json")
}

fn read_before(data_dir: &Path) -> serde_json::Map<String, serde_json::Value> {
    std::fs::read(before_file(data_dir)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

/// DotAny's `tool` (its `.app`) opens `.ext` (`on`), or the program that opened it before does again: the one DotAny
/// took it from, else the first other program that says it opens it. A kind nothing else opens stays DotAny's (there
/// is nobody to give it to). Answers whether DotAny opens it now.
pub fn set_default(tool: Option<&Path>, ext: &str, on: bool, data_dir: &Path) -> bool {
    let now = handler_of(ext);
    if on {
        let Some(app) = tool.filter(|t| t.extension().is_some_and(|e| e == "app")) else {
            return opens_with_dotany(ext);
        };
        if let Some(id) = now.as_deref().filter(|id| !is_ours(id)) {
            let mut before = read_before(data_dir);
            before.insert(ext.to_string(), id.into());
            let _ = std::fs::create_dir_all(data_dir);
            let _ = dotany_base::atomic::atomic_write(
                &before_file(data_dir),
                serde_json::to_vec_pretty(&before).unwrap_or_default().as_slice(),
            );
        }
        let at = NSURL::fileURLWithPath(&NSString::from_str(&app.to_string_lossy()));
        let _ = choose(&at, ext, std::time::Duration::from_secs(15));
    } else if now.as_deref().is_some_and(is_ours) {
        let remembered = read_before(data_dir).get(ext).and_then(|v| v.as_str()).map(str::to_string);
        let others = handlers_of(ext).into_iter().filter(|id| !is_ours(id));
        // one Launch Services still knows of (a program thrown away since is skipped)
        let back = remembered.into_iter().chain(others).find_map(|id| app_with_id(&id));
        if let Some(app) = back {
            // the system asks the person first: as long as they take
            let _ = choose(&app, ext, std::time::Duration::from_secs(600));
        }
    }
    // Launch Services answers for the change a moment after it is made
    for _ in 0..30 {
        if opens_with_dotany(ext) == on {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    opens_with_dotany(ext)
}

// ---------- the version in a tool's .app ----------

/// The version in the tool's `.app` (its Info.plist's CFBundleShortVersionString); a program run from a build folder has
/// none.
pub fn version_of(path: &Path) -> Option<String> {
    let app = path.ancestors().find(|a| a.extension().is_some_and(|e| e == "app"))?;
    let plist = std::fs::read_to_string(app.join("Contents").join("Info.plist")).ok()?;
    dotany_base::quickaction::plist_string(&plist, "CFBundleShortVersionString")
}

// ---------- programs running from DotAny's .apps (before an update replaces them) ----------

/// The programs running now from these .apps, by name as the Dock shows them (`localizedName`), each once: the
/// system's list of running applications (`NSWorkspace.runningApplications`), matched by each one's bundle URL, not by
/// name or identifier, so a DotAny tool run from elsewhere doesn't count. This DotAny itself never counts.
pub fn open_from(apps: &[PathBuf]) -> Vec<String> {
    let real = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let wanted: Vec<PathBuf> = apps.iter().map(|a| real(a)).collect();
    let mut names: Vec<String> = Vec::new();
    let Some(class) = AnyClass::get(c"NSWorkspace") else { return names };
    let me = std::process::id() as i32;
    // SAFETY: NSWorkspace's shared instance and its array of NSRunningApplication, read while retained here; a missing
    // bundle URL or name is nil, checked before use
    unsafe {
        let ws: *mut AnyObject = msg_send![class, sharedWorkspace];
        if ws.is_null() {
            return names;
        }
        let Some(list): Option<Retained<AnyObject>> = msg_send![ws, runningApplications] else { return names };
        let count: usize = msg_send![&*list, count];
        for i in 0..count {
            let r: *mut AnyObject = msg_send![&*list, objectAtIndex: i];
            if r.is_null() {
                continue;
            }
            let pid: i32 = msg_send![r, processIdentifier];
            let gone: bool = msg_send![r, isTerminated];
            if pid == me || gone {
                continue;
            }
            let url: Option<Retained<NSURL>> = msg_send![r, bundleURL];
            let Some(path) = url.and_then(|u| u.path()) else { continue };
            let at = real(Path::new(&path.to_string()));
            let Some(i) = wanted.iter().position(|w| *w == at) else { continue };
            let shown: Option<Retained<NSString>> = msg_send![r, localizedName];
            let name = shown
                .map(|n| n.to_string())
                .filter(|n| !n.is_empty())
                .unwrap_or_else(|| apps[i].file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default());
            if !names.contains(&name) {
                names.push(name);
            }
        }
    }
    names
}

// ---------- one DotAny at a time ----------

/// The Mac itself keeps one DotAny.app open: opening it again, or handing it files, reaches the one that runs (the
/// family's `RunFamily` takes those as `Reopen` and `Opened`), so there is nothing to hold and nobody to knock.
pub struct Single;

pub fn single(_data_dir: &Path, _inbox: &Path, _args: &[String]) -> Result<Single, ()> {
    Ok(Single)
}

pub fn wait_for_knock(_s: &Single) -> bool {
    false
}
