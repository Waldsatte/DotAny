//! Everything DotAny writes to the registry, all under HKEY_CURRENT_USER: DotAny installs for one person and asks for
//! no administrator rights. Tests set DOTANY_SETUP_HIVE to a throwaway key (`Software\DotAnySetupTest\<id>`); every
//! path below is then written inside it instead, so a test never touches the real keys.
//!
//! What gets written for each tool, so Windows lists it under "Open with" and on the Default apps page, and opens the
//! types it takes in it:
//! - `Software\Classes\DotAny.<Tool>.<ext>`: a ProgID with an icon and an open command (ours alone).
//! - `Software\Classes\.<ext>\OpenWithProgids`: one value naming that ProgID (the key is shared with other programs).
//! - `Software\Classes\.<ext>`'s default value = that ProgID, by the release's claim rules (`claim` in payload.json).
//!   A type in `never` (html, htm…) is only listed. One in `polite` (code and
//!   data files) becomes ours only while neither HKCU nor HKLM names another program there. Every other type becomes
//!   ours: a program the person had in HKCU waits in the same key, value `DotAny.Before`, and is put back when DotAny
//!   goes; one set for the whole computer (HKLM) needs nothing, since ours only stands over it. A choice the person made
//!   in Windows (`UserChoice`) always comes first, and is never touched. Put back or removed only while the value still
//!   names us; a program that took the type after us keeps it, through upgrades too.
//! - `Software\Classes\Applications\dotany-<tool>.exe`: the program's name and the types it supports (ours alone).
//! - `Software\DotAny\<Tool>\Capabilities` and a value in `Software\RegisteredApplications`, for the Default apps page.
//! - `Software\Microsoft\Windows\CurrentVersion\Uninstall\DotAny`: the entry in Settings → Apps.
//! - `Software\Microsoft\Windows\CurrentVersion\Run`, value `DotAny`: start the hub with Windows, if switched on.
//! - `Software\Microsoft\Windows NT\CurrentVersion\Fonts`, one value per HarmonyOS Sans file (`DotAny <file> (TrueType)`):
//!   they become the person's own Windows fonts where they are, in `<root>\fonts` beside their licence, without a copy.
//!   Every DotAny page then uses the one copy Windows keeps for all programs instead of loading the files itself. The
//!   values are ours alone, so a font the person installed is never touched.

use crate::manifest::{ClaimRules, Component, RegItem};
use std::io;
use std::path::Path;
use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, KEY_READ, KEY_WRITE, REG_DWORD, REG_NONE, RegDisposition};
use winreg::{RegKey, RegValue};

pub const UNINSTALL: &str = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\DotAny";
pub const RUN: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
pub const OURS: &str = r"Software\DotAny";
pub const REGISTERED: &str = r"Software\RegisteredApplications";
pub const CLASSES: &str = r"Software\Classes";
pub const FONTS: &str = r"Software\Microsoft\Windows NT\CurrentVersion\Fonts";

/// A family font that becomes a Windows font: HarmonyOS Sans SC, 17 MB, which every page would otherwise load for
/// itself, and the fonts one language needs (HarmonyOS Sans TC, Noto Sans JP DotAny, Pretendard). Stack Sans stays a
/// font only the pages load (0.26 MB, nothing to save), so it never sits in the person's font menus beside a Stack Sans
/// of their own.
pub fn is_font(path: &str) -> bool {
    let p = path.replace('\\', "/").to_ascii_lowercase();
    let Some(file) = p.strip_prefix("fonts/") else { return false };
    ["harmonyos", "notosansjp-dotany-", "pretendard-"].iter().any(|f| file.starts_with(f))
        && (file.ends_with(".ttf") || file.ends_with(".otf"))
}

/// The value that makes one font file a Windows font for this person.
pub fn font_value(path: &str) -> String {
    let file = path.replace('\\', "/");
    let stem = file.rsplit('/').next().unwrap_or(&file);
    let (stem, ext) = stem.rsplit_once('.').unwrap_or((stem, ""));
    format!("DotAny {stem} ({})", if ext.eq_ignore_ascii_case("otf") { "OpenType" } else { "TrueType" })
}

/// In a type's own key while we are its default: the program the person had before, put back when we go.
pub const BEFORE: &str = "DotAny.Before";

#[derive(Clone, Debug)]
pub enum Data {
    Sz(String),
    Dword(u32),
    /// A value with no data, as OpenWithProgids wants.
    Empty,
}

#[derive(Clone, Debug)]
pub struct Write {
    /// Relative to the tree's key for a tree, or the whole key for a value.
    pub key: String,
    /// "" is the key's default value.
    pub name: String,
    pub data: Data,
}

#[derive(Clone, Debug)]
pub enum Op {
    Tree { key: String, values: Vec<Write> },
    Value(Write),
}

impl Op {
    pub fn item(&self) -> RegItem {
        match self {
            Op::Tree { key, .. } => RegItem::Tree { key: key.clone() },
            Op::Value(w) => RegItem::Value { key: w.key.clone(), name: w.name.clone() },
        }
    }
}

/// Where keys are written: HKCU itself, or a test's throwaway key inside it.
#[derive(Clone)]
pub struct Hive {
    prefix: String,
}

impl Hive {
    pub fn from_env() -> Hive {
        let prefix = std::env::var("DOTANY_SETUP_HIVE").unwrap_or_default().trim_matches('\\').to_string();
        Hive { prefix }
    }
    pub fn is_test(&self) -> bool {
        !self.prefix.is_empty()
    }
    pub fn path(&self, key: &str) -> String {
        if self.prefix.is_empty() { key.to_string() } else { format!("{}\\{}", self.prefix, key) }
    }
    fn hkcu() -> RegKey {
        RegKey::predef(HKEY_CURRENT_USER)
    }
    pub fn exists(&self, key: &str) -> bool {
        Self::hkcu().open_subkey_with_flags(self.path(key), KEY_READ).is_ok()
    }
    pub fn read(&self, key: &str, name: &str) -> Option<String> {
        Self::hkcu().open_subkey_with_flags(self.path(key), KEY_READ).ok()?.get_value::<String, _>(name).ok()
    }
    /// A value the whole computer has, in HKLM, which is only ever read. A test reads `<its key>-machine` in HKCU
    /// instead, so what this computer happens to have can't change a test, and a test can play a system default.
    pub fn read_machine(&self, key: &str, name: &str) -> Option<String> {
        let (hive, path) = if self.prefix.is_empty() {
            (RegKey::predef(HKEY_LOCAL_MACHINE), key.to_string())
        } else {
            (Self::hkcu(), format!("{}-machine\\{}", self.prefix, key))
        };
        hive.open_subkey_with_flags(path, KEY_READ).ok()?.get_value::<String, _>(name).ok()
    }
    pub fn has_value(&self, key: &str, name: &str) -> bool {
        Self::hkcu()
            .open_subkey_with_flags(self.path(key), KEY_READ)
            .map(|k| k.get_raw_value(name).is_ok())
            .unwrap_or(false)
    }

    fn set(key: &RegKey, name: &str, data: &Data) -> io::Result<()> {
        match data {
            Data::Sz(s) => key.set_value(name, s),
            Data::Dword(d) => key.set_raw_value(name, &RegValue { vtype: REG_DWORD, bytes: d.to_le_bytes().to_vec() }),
            Data::Empty => key.set_raw_value(name, &RegValue { vtype: REG_NONE, bytes: vec![] }),
        }
    }

    /// Creates the keys above `key` that aren't there yet, and says which: they are ours to remove once empty.
    fn make_parents(&self, key: &str) -> io::Result<Vec<String>> {
        let mut created = Vec::new();
        let parts: Vec<&str> = key.split('\\').collect();
        for n in 1..parts.len() {
            let partial = parts[..n].join("\\");
            let (_, how) = Self::hkcu().create_subkey(self.path(&partial))?;
            if matches!(how, RegDisposition::REG_CREATED_NEW_KEY) {
                created.push(partial);
            }
        }
        Ok(created)
    }

    /// Writes one op. Returns the keys it had to create to hold it (outermost first), which are ours to remove later.
    pub fn apply(&self, op: &Op) -> io::Result<Vec<String>> {
        match op {
            Op::Tree { key, values } => {
                // a tree is ours alone: what was there from an earlier install goes, so nothing stale is left inside
                self.remove_tree(key)?;
                let created = self.make_parents(key)?;
                let (base, _) = Self::hkcu().create_subkey(self.path(key))?;
                for w in values {
                    let k = if w.key.is_empty() {
                        base.open_subkey_with_flags("", KEY_WRITE)?
                    } else {
                        base.create_subkey(&w.key)?.0
                    };
                    Self::set(&k, &w.name, &w.data)?;
                }
                Ok(created)
            }
            Op::Value(w) => {
                let mut created = self.make_parents(&w.key)?;
                let (key, how) = Self::hkcu().create_subkey(self.path(&w.key))?;
                if matches!(how, RegDisposition::REG_CREATED_NEW_KEY) {
                    created.push(w.key.clone());
                }
                Self::set(&key, &w.name, &w.data)?;
                Ok(created)
            }
        }
    }

    pub fn remove_tree(&self, key: &str) -> io::Result<()> {
        match Self::hkcu().delete_subkey_all(self.path(key)) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            other => other,
        }
    }

    pub fn remove_value(&self, key: &str, name: &str) -> io::Result<()> {
        let Ok(k) = Self::hkcu().open_subkey_with_flags(self.path(key), KEY_WRITE) else { return Ok(()) };
        match k.delete_value(name) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            other => other,
        }
    }

    /// Removes a key we created, but only once nothing else lives in it.
    pub fn remove_if_empty(&self, key: &str) -> bool {
        let Ok(k) = Self::hkcu().open_subkey_with_flags(self.path(key), KEY_READ) else { return true };
        let empty = k.enum_keys().next().is_none() && k.enum_values().next().is_none();
        drop(k);
        empty && Self::hkcu().delete_subkey(self.path(key)).is_ok()
    }

    pub fn remove(&self, item: &RegItem) -> io::Result<()> {
        match item {
            RegItem::Tree { key } => self.remove_tree(key),
            // a file type's default: put back the program it had before us, while it is still ours; a program that
            // has taken it since keeps it
            RegItem::Value { key, name } if name.is_empty() && is_type_key(key) => {
                if !self.read(key, "").is_some_and(|v| is_ours(&v)) {
                    return self.remove_value(key, BEFORE);
                }
                match self.read(key, BEFORE).filter(|v| !v.is_empty()) {
                    Some(before) => Self::set(&Self::hkcu().create_subkey(self.path(key))?.0, "", &Data::Sz(before))?,
                    None => self.remove_value(key, "")?,
                }
                self.remove_value(key, BEFORE)
            }
            // what waits there goes with the default above, whichever of the two is removed first
            RegItem::Value { key, name }
                if name == BEFORE && is_type_key(key) && self.read(key, "").is_some_and(|v| is_ours(&v)) =>
            {
                Ok(())
            }
            RegItem::Value { key, name } => self.remove_value(key, name),
        }
    }

    /// Removes the created keys that are empty now, deepest first, keeping the ones still in `keep`. Answers the ones
    /// still there, in their first order.
    pub fn sweep(&self, created: &[String], keep: &[String]) -> Vec<String> {
        let mut sorted: Vec<&String> = created.iter().collect();
        sorted.sort_by_key(|k| std::cmp::Reverse(k.matches('\\').count()));
        let mut gone = Vec::new();
        for key in sorted {
            if !keep.contains(key) && self.remove_if_empty(key) {
                gone.push(key.clone());
            }
        }
        created.iter().filter(|k| !gone.contains(k)).cloned().collect()
    }
}

/// One of our ProgIDs: `DotAny.<Tool>.<ext>`.
pub fn is_ours(progid: &str) -> bool {
    progid.starts_with("DotAny.")
}

/// A `polite` type becomes ours only while no other program is its default: not for this person (HKCU) and not for the
/// whole computer (HKLM). An empty value, or one of ours, names nobody else.
pub fn may_claim(user: Option<&str>, machine: Option<&str>) -> bool {
    [user, machine].into_iter().flatten().all(|v| v.is_empty() || is_ours(v))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Claim {
    Never,
    Polite,
    Take,
}

pub fn claim_for(rules: &ClaimRules, ext: &str) -> Claim {
    let has = |list: &[String]| list.iter().any(|e| e.eq_ignore_ascii_case(ext));
    if has(&rules.never) {
        Claim::Never
    } else if has(&rules.polite) {
        Claim::Polite
    } else {
        Claim::Take
    }
}

/// What to write for a type's default, from what is there now: None to leave it, or the ProgID and the program to keep
/// in `DotAny.Before`. `had_it`: the earlier install (an upgrade) was its default, so a program there now took it from
/// us, and keeps it. `before`: what already waits in the key from that earlier install.
pub fn claim_write(
    claim: Claim,
    user: Option<&str>,
    machine: Option<&str>,
    before: Option<&str>,
    had_it: bool,
) -> Option<Option<String>> {
    let someone = user.filter(|v| !v.is_empty() && !is_ours(v));
    match claim {
        Claim::Never => None,
        Claim::Polite => may_claim(user, machine).then_some(None),
        Claim::Take if someone.is_some() && had_it => None,
        Claim::Take => Some(someone.or(before.filter(|v| !v.is_empty())).map(String::from)),
    }
}

// `Software\Classes\.<ext>`: whose default value says which program opens that type
fn is_type_key(key: &str) -> bool {
    key.strip_prefix(CLASSES)
        .and_then(|k| k.strip_prefix("\\."))
        .is_some_and(|ext| !ext.is_empty() && !ext.contains('\\'))
}

fn sz(key: &str, name: &str, value: impl Into<String>) -> Write {
    Write { key: key.to_string(), name: name.to_string(), data: Data::Sz(value.into()) }
}

pub fn progid(c: &Component, ext: &str) -> String {
    format!("DotAny.{}.{}", c.name.replace(' ', ""), ext)
}

pub struct Facts<'a> {
    pub root: &'a Path,
    pub version: &'a str,
    /// the language the registry's file-type names are written in (words.rs CODES)
    pub lang: &'a str,
    pub size_kb: u32,
    pub date: &'a str,
}

/// Everything one install writes, for these components. The Run value is separate: see `autostart`. `had`: what the
/// earlier install wrote, on an upgrade.
pub fn registration(
    hive: &Hive,
    facts: &Facts,
    components: &[&Component],
    rules: &ClaimRules,
    had: &[RegItem],
) -> Vec<Op> {
    let root = facts.root.display().to_string();
    let mut ops = Vec::new();
    let mut ours = vec![sz("", "InstallLocation", &root), sz("", "Version", facts.version)];
    for c in components {
        for f in c.files.iter().filter(|f| is_font(f)) {
            ops.push(Op::Value(sz(FONTS, &font_value(f), facts.root.join(f.replace('/', "\\")).display().to_string())));
        }
    }
    for c in components {
        let Some(exe) = &c.exe else { continue };
        if c.formats.is_empty() {
            continue;
        }
        let path = facts.root.join(exe).display().to_string();
        let open = format!("\"{path}\" \"%1\"");
        let icon = format!("\"{path}\",0");
        let app = format!("DotAny {}", c.name);
        for ext in &c.formats {
            let id = progid(c, ext);
            let kind = crate::words::file_kind(facts.lang, ext);
            ops.push(Op::Tree {
                key: format!(r"{CLASSES}\{id}"),
                values: vec![sz("", "", kind), sz("DefaultIcon", "", &icon), sz(r"shell\open\command", "", &open)],
            });
            ops.push(Op::Value(Write {
                key: format!(r"{CLASSES}\.{ext}\OpenWithProgids"),
                name: id.clone(),
                data: Data::Empty,
            }));
            let type_key = format!(r"{CLASSES}\.{ext}");
            let had_it = had.contains(&RegItem::Value { key: type_key.clone(), name: String::new() });
            let (user, machine, before) =
                (hive.read(&type_key, ""), hive.read_machine(&type_key, ""), hive.read(&type_key, BEFORE));
            if let Some(keep) =
                claim_write(claim_for(rules, ext), user.as_deref(), machine.as_deref(), before.as_deref(), had_it)
            {
                if let Some(keep) = keep {
                    ops.push(Op::Value(sz(&type_key, BEFORE, keep)));
                }
                ops.push(Op::Value(sz(&type_key, "", id)));
            }
        }
        let mut apps =
            vec![sz("", "FriendlyAppName", &app), sz("DefaultIcon", "", &icon), sz(r"shell\open\command", "", &open)];
        apps.extend(c.formats.iter().map(|e| sz("SupportedTypes", &format!(".{e}"), "")));
        ops.push(Op::Tree { key: format!(r"{CLASSES}\Applications\{exe}"), values: apps });
        let caps = format!(r"{}\Capabilities", c.name);
        ours.push(sz(&caps, "ApplicationName", &app));
        ours.push(sz(&caps, "ApplicationDescription", &app));
        ours.push(sz(&caps, "ApplicationIcon", &icon));
        ours.extend(c.formats.iter().map(|e| sz(&format!(r"{caps}\FileAssociations"), &format!(".{e}"), progid(c, e))));
        ops.push(Op::Value(sz(REGISTERED, &app, hive.path(&format!(r"{OURS}\{caps}")))));
    }
    ops.push(Op::Tree { key: OURS.to_string(), values: ours });
    let uninstaller = facts.root.join("uninstall.exe").display().to_string();
    ops.push(Op::Tree {
        key: UNINSTALL.to_string(),
        values: vec![
            sz("", "DisplayName", "DotAny"),
            sz("", "DisplayVersion", facts.version),
            sz("", "Publisher", "DotAny"),
            sz("", "DisplayIcon", format!("\"{uninstaller}\",0")),
            sz("", "InstallLocation", &root),
            sz("", "InstallDate", facts.date.replace('-', "")),
            sz("", "UninstallString", format!("\"{uninstaller}\"")),
            sz("", "QuietUninstallString", format!("\"{uninstaller}\" --quiet")),
            Write { key: String::new(), name: "EstimatedSize".into(), data: Data::Dword(facts.size_kb) },
            Write { key: String::new(), name: "NoModify".into(), data: Data::Dword(1) },
            Write { key: String::new(), name: "NoRepair".into(), data: Data::Dword(1) },
        ],
    });
    ops
}

/// Start the hub with Windows. `--autostart` asks it to get ready without a window; the hub's own settings switch
/// writes the same value.
pub fn autostart(root: &Path) -> Op {
    Op::Value(sz(RUN, "DotAny", format!("\"{}\" --autostart", root.join("dotany.exe").display())))
}

/// Tells Explorer that file types changed, so icons and "Open with" catch up.
pub fn announce() {
    use windows_sys::Win32::UI::Shell::{SHCNE_ASSOCCHANGED, SHCNF_IDLIST, SHChangeNotify};
    unsafe { SHChangeNotify(SHCNE_ASSOCCHANGED as i32, SHCNF_IDLIST, std::ptr::null(), std::ptr::null()) };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn harmonyos_sans_becomes_the_persons_windows_font_where_it_is() {
        let fonts: Component = serde_json::from_value(serde_json::json!({
            "id": "fonts", "shared": true,
            "files": ["fonts/HarmonyOS_SansSC_Regular.ttf", "fonts/StackSansText-wght.ttf", "fonts/OFL-StackSans.txt"]
        }))
        .unwrap();
        let root = Path::new(r"E:\Home\x\AppData\Local\Programs\DotAny");
        let facts = Facts { root, version: "1.0.0", lang: "zh", size_kb: 1, date: "2000-01-02" };
        let values: Vec<(String, String)> =
            registration(&Hive { prefix: String::new() }, &facts, &[&fonts], &rules(), &[])
                .into_iter()
                .filter_map(|op| match op {
                    Op::Value(Write { key, name, data: Data::Sz(path) }) if key == FONTS => Some((name, path)),
                    _ => None,
                })
                .collect();
        assert_eq!(
            values,
            vec![(
                "DotAny HarmonyOS_SansSC_Regular (TrueType)".to_string(),
                r"E:\Home\x\AppData\Local\Programs\DotAny\fonts\HarmonyOS_SansSC_Regular.ttf".to_string()
            ),]
        );
        assert!(is_font("fonts/HarmonyOS_SansSC_Bold.ttf") && !is_font("fonts/StackSansHeadline-wght.ttf"));
        assert!(!is_font("fonts/LICENSE-HarmonyOS-Sans.txt") && !is_font("dotany.exe"));
        // the fonts one language needs, and not their licences
        for f in [
            "fonts/HarmonyOS_Sans_TC.ttf",
            "fonts/NotoSansJP-DotAny-Regular.ttf",
            "fonts/NotoSansJP-DotAny-Bold.ttf",
            "fonts/Pretendard-Regular.otf",
            "fonts/Pretendard-Bold.otf",
        ] {
            assert!(is_font(f), "{f}");
        }
        assert!(
            !is_font("fonts/OFL-NotoSansJP.txt")
                && !is_font("fonts/OFL-Pretendard.txt")
                && !is_font("Pretendard-Bold.otf")
        );
        assert_eq!(font_value("fonts/Pretendard-Bold.otf"), "DotAny Pretendard-Bold (OpenType)");
        assert_eq!(font_value("fonts/HarmonyOS_Sans_TC.ttf"), "DotAny HarmonyOS_Sans_TC (TrueType)");
    }

    fn rules() -> ClaimRules {
        ClaimRules { never: vec!["html".into(), "nc".into()], polite: vec!["json".into(), "xml".into(), "yaml".into()] }
    }

    #[test]
    fn three_kinds_of_type() {
        let r = rules();
        assert_eq!(claim_for(&r, "html"), Claim::Never);
        assert_eq!(claim_for(&r, "HTML"), Claim::Never);
        assert_eq!(claim_for(&r, "json"), Claim::Polite);
        assert_eq!(claim_for(&r, "mod"), Claim::Take);
        assert_eq!(claim_for(&ClaimRules::default(), "html"), Claim::Take, "a release without lists takes everything");
    }

    #[test]
    fn a_polite_type_is_ours_only_while_nobody_else_is_its_default() {
        assert!(may_claim(None, None));
        assert!(may_claim(Some(""), Some("")));
        assert!(may_claim(Some("DotAny.Text.json"), None));
        assert!(may_claim(None, Some("DotAny.Text.json")));
        assert!(!may_claim(Some("VSCode.json"), None));
        assert!(!may_claim(None, Some("Windows.XmlFile")));
        assert!(!may_claim(Some("DotAny.Text.json"), Some("Windows.XmlFile")));
        assert!(!may_claim(Some(""), Some("SomeoneElse.json")));
    }

    #[test]
    fn what_each_kind_writes() {
        // never: nothing, whoever has it
        assert_eq!(claim_write(Claim::Never, None, None, None, false), None);
        assert_eq!(claim_write(Claim::Never, Some("DotAny.Text.html"), None, None, true), None);
        // polite: only with nobody else on either side, and nothing to keep
        assert_eq!(claim_write(Claim::Polite, None, None, None, false), Some(None));
        assert_eq!(claim_write(Claim::Polite, Some("VSCode.json"), None, None, false), None);
        assert_eq!(claim_write(Claim::Polite, None, Some("Windows.XmlFile"), None, false), None);
        // take: always; the person's program in HKCU is kept, the computer's in HKLM is only stood over
        assert_eq!(claim_write(Claim::Take, None, None, None, false), Some(None));
        assert_eq!(claim_write(Claim::Take, Some(""), None, None, false), Some(None));
        assert_eq!(claim_write(Claim::Take, Some("VLC.mod"), None, None, false), Some(Some("VLC.mod".into())));
        assert_eq!(claim_write(Claim::Take, None, Some("Windows.IsoFile"), None, false), Some(None));
        assert_eq!(
            claim_write(Claim::Take, Some("VLC.mod"), Some("Windows.ModFile"), None, false),
            Some(Some("VLC.mod".into()))
        );
        // an upgrade: still ours, and what waited keeps waiting
        assert_eq!(
            claim_write(Claim::Take, Some("DotAny.Audio.mod"), None, Some("VLC.mod"), true),
            Some(Some("VLC.mod".into()))
        );
        assert_eq!(claim_write(Claim::Take, Some("DotAny.Audio.mod"), None, None, true), Some(None));
        // an upgrade over a program that took it from us since: it keeps it
        assert_eq!(claim_write(Claim::Take, Some("Later.mod"), None, Some("VLC.mod"), true), None);
        // a type the earlier install didn't have: taken, keeping the program there
        assert_eq!(claim_write(Claim::Take, Some("Later.mod"), None, None, false), Some(Some("Later.mod".into())));
    }

    // install, upgrade and uninstall over a test hive, as the engine runs them: every kind of type, every kind of
    // default there before
    #[test]
    fn an_install_takes_its_types_and_the_uninstall_puts_the_earlier_programs_back() {
        let prefix = format!(r"Software\DotAnySetupTest\unit-claim-{}", std::process::id());
        let hive = Hive { prefix: prefix.clone() };
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let class = |ext: &str| format!(r"{CLASSES}\.{ext}");
        let put = |side: &str, ext: &str, value: &str| {
            hkcu.create_subkey(format!(r"{prefix}{side}\{}", class(ext)))
                .unwrap()
                .0
                .set_value("", &value.to_string())
                .unwrap()
        };
        put("", "voc", "Someone.voc"); // the person's program, in HKCU
        put("-machine", "au", "Windows.AuFile"); // the computer's, in HKLM
        put("", "it", ""); // an empty value
        put("", "xm", "DotAny.Audio.xm"); // ours already
        put("", "json", "VSCode.json"); // polite, the person's
        put("-machine", "xml", "Windows.XmlFile"); // polite, the computer's
        put("", "html", "MSEdgeHTM"); // never

        let c: Component = serde_json::from_value(serde_json::json!({
            "id": "audio", "name": "Audio", "exe": "dotany-audio.exe", "files": ["dotany-audio.exe"],
            "formats": ["mod", "voc", "au", "it", "xm", "json", "xml", "yaml", "html", "nc"]
        }))
        .unwrap();
        let r = rules();
        let facts =
            Facts { root: Path::new(r"C:\DotAny"), version: "1.0.0", lang: "en", size_kb: 1, date: "2000-01-02" };
        let default = |ext: &str| hive.read(&class(ext), "");
        let waiting = |ext: &str| hive.read(&class(ext), BEFORE);
        let ours = |ext: &str| Some(format!("DotAny.Audio.{ext}"));
        let s = |v: &str| Some(v.to_string());
        // as engine::commit: what the earlier install had and this one doesn't goes, then this one is written
        let install = |had: &[RegItem]| -> Vec<RegItem> {
            let ops = registration(&hive, &facts, &[&c], &r, had);
            let items: Vec<RegItem> = ops.iter().map(Op::item).collect();
            for item in had.iter().filter(|i| !items.contains(i)) {
                hive.remove(item).unwrap();
            }
            for op in &ops {
                hive.apply(op).unwrap();
            }
            items
        };

        let checked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let first = install(&[]);
            for ext in ["mod", "voc", "au", "it", "xm"] {
                assert_eq!(default(ext), ours(ext), ".{ext} is ours");
            }
            assert_eq!(waiting("voc"), s("Someone.voc"), "the person's program waits");
            for ext in ["mod", "au", "it", "xm", "yaml"] {
                assert_eq!(waiting(ext), None, ".{ext} has nothing waiting");
            }
            assert_eq!(default("yaml"), ours("yaml"), "polite, and nobody has it");
            assert_eq!(default("json"), s("VSCode.json"), "polite: the person's program stays");
            assert_eq!(default("xml"), None, "polite: the computer's program stays the default");
            assert_eq!(default("html"), s("MSEdgeHTM"), "never");
            assert_eq!(default("nc"), None, "never");

            // a program takes .mod after the install: the upgrade leaves it there, and .voc's program keeps waiting
            put("", "mod", "Later.mod");
            let second = install(&first);
            assert_eq!(default("mod"), s("Later.mod"));
            assert_eq!((default("voc"), waiting("voc")), (ours("voc"), s("Someone.voc")));
            assert_eq!(default("au"), ours("au"));

            // the uninstall, in the record's order and the other way round
            for order in [false, true] {
                // the second round over a fresh install, its record removed back to front
                let mut items = if order { install(&[]) } else { install(&second) };
                if order {
                    items.reverse();
                }
                for item in &items {
                    hive.remove(item).unwrap();
                }
                assert_eq!((default("voc"), waiting("voc")), (s("Someone.voc"), None), "the person's program is back");
                assert_eq!(default("mod"), s("Later.mod"), "the one that took it keeps it");
                for ext in ["au", "it", "xm", "yaml"] {
                    assert_eq!(default(ext), None, ".{ext}: ours is gone");
                    assert_eq!(waiting(ext), None);
                }
                assert_eq!(
                    hive.read_machine(&class("au"), ""),
                    s("Windows.AuFile"),
                    "the computer's was never touched"
                );
                assert_eq!((default("json"), default("html")), (s("VSCode.json"), s("MSEdgeHTM")));
                // the person's program in place again, for the second round
                put("", "voc", "Someone.voc");
            }
        }));
        let _ = hkcu.delete_subkey_all(&prefix);
        let _ = hkcu.delete_subkey_all(format!("{prefix}-machine"));
        let _ = hkcu.delete_subkey(r"Software\DotAnySetupTest"); // only goes while nothing else is in it
        if let Err(e) = checked {
            std::panic::resume_unwind(e);
        }
    }
}
