//! The Finder's Quick Actions, the Mac's counterpart of the right-click entries a tool adds on Windows: the hub's
//! Convert with DotAny, Archive's Open with DotAny Archive and Compress with DotAny Archive.
//!
//! A Quick Action is an Automator workflow in `~/Library/Services/<label>.workflow`: an `Info.plist` that tells the
//! system its name and which kinds of file it is for (`NSSendFileTypes`, so it is offered for those and no others), and
//! a `document.wflow` with one "Run Shell Script" action that hands the chosen files to the program with `open -a`, the
//! way a drop on its Dock icon does. The program is found by where its `.app` was when the action was written, else by
//! its identifier (it was moved); when neither finds it (DotAny went to the Trash) the action removes itself, so no
//! broken entry stays behind. Each workflow carries a key of ours, `DotAnyQuickAction`, by which it is found and
//! removed again: only what was added is ever taken away.
//!
//! `DOTANY_TEST_SERVICES_DIR` points it at another folder instead of `~/Library/Services`, for an automated test.

use std::ffi::c_void;
use std::path::{Path, PathBuf};

/// One Quick Action as a program asks for it.
pub struct QuickAction<'a> {
    /// What it is, as the workflow records it: `hub-convert`, `archive-open`, `archive-pack`.
    pub kind: &'a str,
    /// Its name in the Quick Actions menu, in the interface's language.
    pub label: &'a str,
    /// The kinds of file it is offered for, as the system's type identifiers.
    pub types: Vec<String>,
    /// Shell lines run before the files are handed over (the chosen files are `"$@"`).
    pub before: String,
}

/// Where the Quick Actions are.
pub fn services_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("DOTANY_TEST_SERVICES_DIR") {
        return PathBuf::from(d);
    }
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"));
    home.join("Library").join("Services")
}

/// The `.app` this program runs in and its identifier; none for a program run from a build folder, which the system
/// can't hand files to.
pub fn this_app() -> Option<(PathBuf, String)> {
    let exe = std::env::current_exe().ok()?;
    let app = exe.parent()?.parent()?.parent()?;
    if !app.extension().is_some_and(|e| e == "app") {
        return None;
    }
    let plist = std::fs::read_to_string(app.join("Contents").join("Info.plist")).ok()?;
    let id = plist_string(&plist, "CFBundleIdentifier")?;
    Some((app.to_path_buf(), id))
}

/// A string value from an XML property list, by its key.
pub fn plist_string(plist: &str, key: &str) -> Option<String> {
    let after = plist.split(&format!("<key>{key}</key>")).nth(1)?;
    let v = after.trim_start().strip_prefix("<string>")?.split("</string>").next()?.trim();
    (!v.is_empty()).then(|| unxml(v))
}

fn xml(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn unxml(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

/// A word for the shell, quoted so nothing in it is read as the shell's.
pub fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

/// The workflows of one kind that are there now.
fn ours(kind: &str) -> Vec<PathBuf> {
    let Ok(list) = std::fs::read_dir(services_dir()) else { return Vec::new() };
    list.flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "workflow"))
        .filter(|p| {
            std::fs::read_to_string(p.join("Contents").join("Info.plist"))
                .ok()
                .and_then(|s| plist_string(&s, "DotAnyQuickAction"))
                .is_some_and(|k| k == kind)
        })
        .collect()
}

/// Whether the Quick Action of this kind is there.
pub fn present(kind: &str) -> bool {
    !ours(kind).is_empty()
}

/// Takes every workflow of this kind away. True when none is left.
pub fn remove(kind: &str) -> bool {
    for p in ours(kind) {
        let _ = std::fs::remove_dir_all(&p);
    }
    refresh();
    ours(kind).is_empty()
}

/// Writes the Quick Action for this program (replacing one of the same kind, say in another language). False when the
/// program isn't an `.app`, or the folder can't be written.
pub fn add(qa: &QuickAction) -> bool {
    let Some((app, id)) = this_app() else { return false };
    for p in ours(qa.kind) {
        let _ = std::fs::remove_dir_all(&p);
    }
    let name = qa.label.replace(['/', ':'], "-");
    let dir = services_dir().join(format!("{name}.workflow"));
    let contents = dir.join("Contents");
    let written = std::fs::create_dir_all(&contents)
        .and_then(|_| std::fs::write(contents.join("Info.plist"), info_plist(qa)))
        .and_then(|_| std::fs::write(contents.join("document.wflow"), document(&script(qa, &app, &id, &dir))));
    if written.is_err() {
        let _ = std::fs::remove_dir_all(&dir);
        return false;
    }
    refresh();
    present(qa.kind)
}

/// What the action runs, the chosen files being its arguments.
pub fn script(qa: &QuickAction, app: &Path, id: &str, me: &Path) -> String {
    let (app, id, me) = (shell_quote(&app.to_string_lossy()), shell_quote(id), shell_quote(&me.to_string_lossy()));
    format!(
        "# DotAny: hands the chosen files to the program, the way a drop on its Dock icon does\n\
         {before}\
         if [ -d {app} ]; then exec /usr/bin/open -a {app} \"$@\"; fi\n\
         if /usr/bin/open -b {id} \"$@\" 2>/dev/null; then exit 0; fi\n\
         # the program is gone: this action goes too\n\
         /bin/rm -rf {me}\n",
        before = qa.before
    )
}

fn info_plist(qa: &QuickAction) -> String {
    let types: String = qa.types.iter().map(|t| format!("\t\t\t\t<string>{}</string>\n", xml(t))).collect();
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>DotAnyQuickAction</key>
	<string>{kind}</string>
	<key>NSServices</key>
	<array>
		<dict>
			<key>NSBackgroundColorName</key>
			<string>background</string>
			<key>NSIconName</key>
			<string>NSActionTemplate</string>
			<key>NSMenuItem</key>
			<dict>
				<key>default</key>
				<string>{label}</string>
			</dict>
			<key>NSMessage</key>
			<string>runWorkflowAsService</string>
			<key>NSRequiredContext</key>
			<dict>
				<key>NSApplicationIdentifier</key>
				<string>com.apple.finder</string>
			</dict>
			<key>NSSendFileTypes</key>
			<array>
{types}			</array>
		</dict>
	</array>
</dict>
</plist>
"#,
        kind = xml(qa.kind),
        label = xml(qa.label),
    )
}

/// A new UUID in the form Automator writes, for the action's own identifiers.
fn uuid() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    let t = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    let x = (t as u64) ^ ((std::process::id() as u64) << 32) ^ N.fetch_add(0x9E37_79B9_7F4A_7C15, Ordering::Relaxed);
    let y = x.wrapping_mul(0x2545_F491_4F6C_DD1D);
    format!(
        "{:08X}-{:04X}-4{:03X}-8{:03X}-{:012X}",
        (x >> 32) as u32,
        (x >> 16) as u16,
        (x & 0xFFF) as u16,
        (y >> 52) as u16 & 0xFFF,
        y & 0xFFFF_FFFF_FFFF
    )
}

/// Automator's document: one "Run Shell Script" action (`/bin/zsh`, the files as arguments) in a Quick Action that takes
/// files and folders in the Finder.
fn document(command: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
	<key>AMApplicationBuild</key>
	<string>528</string>
	<key>AMApplicationVersion</key>
	<string>2.10</string>
	<key>AMDocumentVersion</key>
	<string>2</string>
	<key>actions</key>
	<array>
		<dict>
			<key>action</key>
			<dict>
				<key>AMAccepts</key>
				<dict>
					<key>Container</key>
					<string>List</string>
					<key>Optional</key>
					<true/>
					<key>Types</key>
					<array>
						<string>com.apple.cocoa.string</string>
					</array>
				</dict>
				<key>AMActionVersion</key>
				<string>2.0.3</string>
				<key>AMApplication</key>
				<array>
					<string>Automator</string>
				</array>
				<key>AMParameterProperties</key>
				<dict>
					<key>COMMAND_STRING</key>
					<dict/>
					<key>CheckedForUserDefaultShell</key>
					<dict/>
					<key>inputMethod</key>
					<dict/>
					<key>shell</key>
					<dict/>
					<key>source</key>
					<dict/>
				</dict>
				<key>AMProvides</key>
				<dict>
					<key>Container</key>
					<string>List</string>
					<key>Types</key>
					<array>
						<string>com.apple.cocoa.string</string>
					</array>
				</dict>
				<key>ActionBundlePath</key>
				<string>/System/Library/Automator/Run Shell Script.action</string>
				<key>ActionName</key>
				<string>Run Shell Script</string>
				<key>ActionParameters</key>
				<dict>
					<key>COMMAND_STRING</key>
					<string>{command}</string>
					<key>CheckedForUserDefaultShell</key>
					<true/>
					<key>inputMethod</key>
					<integer>1</integer>
					<key>shell</key>
					<string>/bin/zsh</string>
					<key>source</key>
					<string></string>
				</dict>
				<key>BundleIdentifier</key>
				<string>com.apple.RunShellScript</string>
				<key>CFBundleVersion</key>
				<string>2.0.3</string>
				<key>CanShowSelectedItemsWhenRun</key>
				<false/>
				<key>CanShowWhenRun</key>
				<true/>
				<key>Category</key>
				<array>
					<string>AMCategoryUtilities</string>
				</array>
				<key>Class Name</key>
				<string>RunShellScriptAction</string>
				<key>InputUUID</key>
				<string>{input}</string>
				<key>Keywords</key>
				<array>
					<string>Shell</string>
					<string>Script</string>
					<string>Command</string>
					<string>Run</string>
					<string>Unix</string>
				</array>
				<key>OutputUUID</key>
				<string>{output}</string>
				<key>UUID</key>
				<string>{id}</string>
				<key>UnlocalizedApplications</key>
				<array>
					<string>Automator</string>
				</array>
				<key>arguments</key>
				<dict>
					<key>0</key>
					<dict>
						<key>default value</key>
						<integer>0</integer>
						<key>name</key>
						<string>inputMethod</string>
						<key>required</key>
						<string>0</string>
						<key>type</key>
						<string>0</string>
						<key>uuid</key>
						<string>0</string>
					</dict>
					<key>1</key>
					<dict>
						<key>default value</key>
						<false/>
						<key>name</key>
						<string>CheckedForUserDefaultShell</string>
						<key>required</key>
						<string>0</string>
						<key>type</key>
						<string>0</string>
						<key>uuid</key>
						<string>1</string>
					</dict>
					<key>2</key>
					<dict>
						<key>default value</key>
						<string></string>
						<key>name</key>
						<string>source</string>
						<key>required</key>
						<string>0</string>
						<key>type</key>
						<string>0</string>
						<key>uuid</key>
						<string>2</string>
					</dict>
					<key>3</key>
					<dict>
						<key>default value</key>
						<string></string>
						<key>name</key>
						<string>COMMAND_STRING</string>
						<key>required</key>
						<string>0</string>
						<key>type</key>
						<string>0</string>
						<key>uuid</key>
						<string>3</string>
					</dict>
					<key>4</key>
					<dict>
						<key>default value</key>
						<string>/bin/sh</string>
						<key>name</key>
						<string>shell</string>
						<key>required</key>
						<string>0</string>
						<key>type</key>
						<string>0</string>
						<key>uuid</key>
						<string>4</string>
					</dict>
				</dict>
				<key>isViewVisible</key>
				<integer>1</integer>
				<key>location</key>
				<string>309.000000:305.000000</string>
				<key>nibPath</key>
				<string>/System/Library/Automator/Run Shell Script.action/Contents/Resources/Base.lproj/main.nib</string>
			</dict>
			<key>isViewVisible</key>
			<integer>1</integer>
		</dict>
	</array>
	<key>connectors</key>
	<dict/>
	<key>workflowMetaData</key>
	<dict>
		<key>applicationBundleIDsByPath</key>
		<dict/>
		<key>applicationPaths</key>
		<array/>
		<key>inputTypeIdentifier</key>
		<string>com.apple.Automator.fileSystemObject</string>
		<key>outputTypeIdentifier</key>
		<string>com.apple.Automator.nothing</string>
		<key>presentationMode</key>
		<integer>15</integer>
		<key>processesInput</key>
		<false/>
		<key>serviceApplicationBundleID</key>
		<string>com.apple.finder</string>
		<key>serviceApplicationPath</key>
		<string>/System/Library/CoreServices/Finder.app</string>
		<key>serviceInputTypeIdentifier</key>
		<string>com.apple.Automator.fileSystemObject</string>
		<key>serviceOutputTypeIdentifier</key>
		<string>com.apple.Automator.nothing</string>
		<key>serviceProcessesInput</key>
		<false/>
		<key>systemImageName</key>
		<string>NSActionTemplate</string>
		<key>useAutomaticInputType</key>
		<false/>
		<key>workflowTypeIdentifier</key>
		<string>com.apple.Automator.servicesMenu</string>
	</dict>
</dict>
</plist>
"#,
        command = xml(command),
        input = uuid(),
        output = uuid(),
        id = uuid(),
    )
}

#[link(name = "AppKit", kind = "framework")]
extern "C" {
    /// Tells the system's services list that ours changed, as `pbs -update` does.
    fn NSUpdateDynamicServices();
}

fn refresh() {
    // SAFETY: no arguments; AppKit is loaded in every program of ours
    unsafe { NSUpdateDynamicServices() };
}

// ---------- the system's type identifiers ----------

type CFStringRef = *const c_void;

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFStringCreateWithBytes(a: *const c_void, b: *const u8, n: isize, enc: u32, ext: u8) -> CFStringRef;
    fn CFStringGetCString(s: CFStringRef, buf: *mut std::ffi::c_char, size: isize, enc: u32) -> u8;
    fn CFRelease(cf: *const c_void);
}

#[link(name = "CoreServices", kind = "framework")]
extern "C" {
    fn UTTypeCreatePreferredIdentifierForTag(class: CFStringRef, tag: CFStringRef, within: CFStringRef) -> CFStringRef;
}

fn cf(s: &str) -> CFStringRef {
    // SAFETY: the bytes and their length, UTF-8; a null allocator is the default one
    unsafe { CFStringCreateWithBytes(std::ptr::null(), s.as_ptr(), s.len() as isize, 0x0800_0100, 0) }
}

/// The type identifier the system gives files ending `.ext` (`public.png`; `dyn.…` for one nobody declares).
pub fn type_of_ext(ext: &str) -> Option<String> {
    let (class, tag) = (cf("public.filename-extension"), cf(ext));
    if class.is_null() || tag.is_null() {
        return None;
    }
    // SAFETY: two strings of ours, released here; what comes back is ours to release
    let uti = unsafe {
        let u = UTTypeCreatePreferredIdentifierForTag(class, tag, std::ptr::null());
        CFRelease(class);
        CFRelease(tag);
        u
    };
    if uti.is_null() {
        return None;
    }
    let mut buf = vec![0u8; 512];
    // SAFETY: a buffer of that size, filled with a NUL-terminated string or refused
    let ok = unsafe { CFStringGetCString(uti, buf.as_mut_ptr() as *mut _, buf.len() as isize, 0x0800_0100) };
    // SAFETY: ours, from the Create call
    unsafe { CFRelease(uti) };
    if ok == 0 {
        return None;
    }
    let end = buf.iter().position(|b| *b == 0)?;
    String::from_utf8(buf[..end].to_vec()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_quotes_every_path_and_goes_when_the_program_has() {
        let qa =
            QuickAction { kind: "hub-convert", label: "使用 DotAny 转换", types: vec![], before: String::new() };
        let s = script(
            &qa,
            Path::new("/Applications/DotAny/Rob's DotAny.app"),
            "com.dotany.hub",
            Path::new("/x/a b.workflow"),
        );
        assert!(
            s.contains(r#"if [ -d '/Applications/DotAny/Rob'\''s DotAny.app' ]; then exec /usr/bin/open -a"#),
            "{s}"
        );
        assert!(s.contains("/usr/bin/open -b 'com.dotany.hub' \"$@\""), "{s}");
        assert!(s.ends_with("/bin/rm -rf '/x/a b.workflow'\n"), "{s}");
        let doc = document(&s);
        assert!(doc.contains("Rob&apos;s") || doc.contains("Rob'\\''s"), "the command is in the document");
        assert_eq!(plist_string(&info_plist(&qa), "DotAnyQuickAction").as_deref(), Some("hub-convert"));
        assert_eq!(uuid().len(), 36);
        assert_ne!(uuid(), uuid());
    }
}
