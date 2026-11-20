//! What a release carries and what an install leaves behind, as JSON. The packer writes a `Release` into the installer;
//! an install writes a `Record` to `<root>\install.json`. Each release publishes the same `Release` as `manifest.json`
//! beside the installer, so the hub can compare versions and file hashes without a format of its own.

use serde::{Deserialize, Serialize};

pub const SCHEMA: u32 = 1;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Release {
    pub schema: u32,
    pub product: String,
    pub version: String,
    /// When the payload was packed, UTC, `YYYY-MM-DDTHH:MM:SSZ`.
    pub built: String,
    pub components: Vec<Component>,
    /// Every file once, in the order it sits in the packed stream.
    pub files: Vec<FileEntry>,
    /// Which file types an install makes DotAny's default. See `ClaimRules`.
    #[serde(default)]
    pub claim: ClaimRules,
}

/// Which of the types the tools register an install makes DotAny's default (`claim` in installer/payload.json): a type
/// in `never` is only listed under Open with; one in `polite` becomes ours only while no other program is its default,
/// for this person or for the whole computer; every other type becomes ours, and the program the person had comes back
/// when DotAny goes. A choice made in Windows (`UserChoice`) always comes first. See `reg::registration`.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ClaimRules {
    #[serde(default)]
    pub never: Vec<String>,
    #[serde(default)]
    pub polite: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Component {
    pub id: String,
    /// The card's name: "Text". Empty for shared parts, which have no card.
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub version: String,
    /// The program, relative to the install folder: "dotany-text.exe".
    #[serde(default)]
    pub exe: Option<String>,
    /// The hub: always installed.
    #[serde(default)]
    pub required: bool,
    /// Installed once for every tool that needs it (the fonts).
    #[serde(default)]
    pub shared: bool,
    /// A stand-in rather than the real program. dotany-pack always writes false: a program that isn't built stops it.
    #[serde(default)]
    pub placeholder: bool,
    /// Relative paths of this component's files; a file two components list (libmp3lame.dll) is installed once.
    pub files: Vec<String>,
    /// Extensions without dots that this tool registers to open.
    #[serde(default)]
    pub formats: Vec<String>,
    /// Shared components this one needs.
    #[serde(default)]
    pub needs: Vec<String>,
    /// Keys under HKCU the tool itself may have made (Archive's right-click menu); removed with the tool.
    #[serde(default)]
    pub cleanup: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FileEntry {
    pub path: String,
    pub size: u64,
    pub sha256: String,
    /// Where the file starts in the unpacked stream.
    pub offset: u64,
}

impl Release {
    pub fn component(&self, id: &str) -> Option<&Component> {
        self.components.iter().find(|c| c.id == id)
    }
    pub fn file(&self, path: &str) -> Option<&FileEntry> {
        self.files.iter().find(|f| f.path == path)
    }
    /// The tools a person can pick, in card order: everything that has a card, the hub first.
    pub fn programs(&self) -> impl Iterator<Item = &Component> {
        self.components.iter().filter(|c| !c.shared)
    }
    /// The chosen programs plus the hub and what they need, in stream order.
    pub fn resolve(&self, chosen: &[String]) -> Vec<&Component> {
        let mut want: Vec<&str> = Vec::new();
        for c in self.programs() {
            if c.required || chosen.iter().any(|x| x == &c.id) {
                want.push(&c.id);
                want.extend(c.needs.iter().map(String::as_str));
            }
        }
        self.components.iter().filter(|c| want.contains(&c.id.as_str())).collect()
    }
    /// The files of these components, once each, in stream order.
    pub fn files_of(&self, components: &[&Component]) -> Vec<&FileEntry> {
        self.files.iter().filter(|f| components.iter().any(|c| c.files.contains(&f.path))).collect()
    }
}

/// `<root>\install.json`: exactly what this install put on the computer, so uninstalling takes away that and no more.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct Record {
    pub schema: u32,
    pub product: String,
    pub version: String,
    pub root: String,
    pub installed: String,
    pub components: Vec<InstalledComponent>,
    pub files: Vec<InstalledFile>,
    pub registry: Vec<RegItem>,
    /// Keys under HKCU that this install created to hold one of its values, and may remove again once they are empty.
    pub created_keys: Vec<String>,
    /// Keys a tool may have made itself (Archive's right-click menu); removed if they are there.
    #[serde(default)]
    pub tool_keys: Vec<String>,
    pub shortcuts: Vec<String>,
    pub autostart: bool,
    /// Folders above the install folder that the first install had to make; removed again once empty.
    #[serde(default)]
    pub made_dirs: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct InstalledComponent {
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub exe: Option<String>,
    pub version: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct InstalledFile {
    pub path: String,
    pub size: u64,
    pub sha256: String,
}

/// One thing written under HKCU. A tree is ours alone; a value sits in a key other programs share.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "lowercase")]
pub enum RegItem {
    Tree { key: String },
    Value { key: String, name: String },
}

/// Round MiB the way the cards show them: never 0 for something that is there.
pub fn mb(bytes: u64) -> u64 {
    if bytes == 0 { 0 } else { ((bytes as f64) / 1048576.0).round().max(1.0) as u64 }
}
