//! The hub's converter. Files go in, the same thing comes out in another format, saved beside each original under a name
//! that is free (`name (2).ext`), and the original is never opened for writing. Each family's reading and writing is the
//! tool's own, without the window around it, from the crate the tool itself builds from: pictures from Image
//! (dotany-picture), sound from Audio (dotany-sound), tables from Sheets (dotany-table), words from Text (dotany-words),
//! archives from Archive (dotany-unpack). Where one of those crates doesn't hold a piece, the converter keeps its own
//! copy; README.md says which. What a tool only does in its page (Slides drawing a deck, PDF.js reading a page, three.js
//! reading a model) is either done here another real way or left out; README.md says which.

/// The pictures of a Word file turned into Markdown, named and linked as Text's Save As does (shared with dotany-base).
#[path = "../../rust/src/beside.rs"]
#[allow(dead_code)]
mod beside;
mod model;
mod out;
mod pack;
mod pdf;
mod picture;
mod sound;
mod table;
mod words;

pub mod notices;

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub use notices::{Notice, notices};

/// What a group of files is. The hub's page calls them by these names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Family {
    Picture,
    Sound,
    Table,
    /// Slides: pptx, ppsx, potx.
    Page,
    Pdf,
    Words,
    Model,
    Pack,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Targets {
    pub family: Family,
    /// In the order the hub shows them; the first is where it starts.
    pub formats: Vec<String>,
}

/// Why nothing can be offered: the hub's three answers ("Video conversion isn't supported yet." / "These file types
/// cannot be converted together." / "This file format cannot be converted yet.").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Refusal {
    Video,
    Mixed,
    Unsupported,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Tone {
    /// Standard: 128 kbps
    Normal,
    /// High: 192 kbps
    #[default]
    High,
    /// Maximum: 320 kbps
    Top,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum Pack {
    Fast,
    #[default]
    Normal,
    Small,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Options {
    /// 10–100, for jpg and webp.
    pub quality: u8,
    /// For mp3.
    pub tone: Tone,
    /// For zip and 7z.
    pub pack: Pack,
    /// Word to Markdown: what a picture written beside the Markdown is called, in the language on screen, `{n}` its
    /// number (`图{n}` makes `report 图1.png`, `Figure {n}` makes `report Figure 1.png`).
    pub picture: String,
    /// Word to Markdown and to text: the interface language's code (`zh`, `zh-Hant`, `en`, `ja`, `ko`, `es-419`,
    /// `pt-BR`, `fr`, `de`), for the words written where a picture or another block can't be carried. Any other code is
    /// English.
    pub lang: String,
}

impl Default for Options {
    fn default() -> Self {
        // the values the hub's page starts with
        Options { quality: 85, tone: Tone::High, pack: Pack::Normal, picture: "图{n}".into(), lang: "en".into() }
    }
}

/// Why a conversion stopped. Outputs finished before it are kept and listed in `done`; the one being written is removed.
#[derive(Debug)]
pub struct ConvertError {
    pub reason: Reason,
    /// The file it stopped on.
    pub file: PathBuf,
    /// Outputs already finished, beside their originals.
    pub done: Vec<PathBuf>,
}

#[derive(Debug, PartialEq, Eq, Clone, Serialize)]
#[serde(rename_all = "lowercase", tag = "kind", content = "detail")]
pub enum Reason {
    /// The files can't go to that format at all (what `targets` would have said).
    Refused(Refusal),
    /// The file isn't what its name says, or is broken partway.
    Damaged,
    /// The file couldn't be read: gone, in use, or no permission.
    Unreadable,
    /// The file has a password (a workbook, an archive); the converter doesn't ask for one.
    Locked,
    /// The folder beside it couldn't be written: read only, full, or no permission.
    Unwritable,
    /// Too large for the memory there is, or for the format (a WAV past 4 GB).
    TooLarge,
    /// A piece of the program is missing: LAME's DLL beside the program, or Windows' own PDF reader.
    Missing(String),
}

impl std::fmt::Display for ConvertError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?} at {}", self.reason, self.file.display())
    }
}
impl std::error::Error for ConvertError {}

// ---------- names ----------

/// The extension in lower case; a compressed tar counts as one (`.tar.gz` and `.tgz` are "tar.gz").
pub(crate) fn ext_of(p: &Path) -> String {
    let name = p.file_name().map(|n| n.to_string_lossy().to_lowercase()).unwrap_or_default();
    for (ends, ext) in [
        (".tar.gz", "tar.gz"),
        (".tgz", "tar.gz"),
        (".tar.bz2", "tar.bz2"),
        (".tbz2", "tar.bz2"),
        (".tbz", "tar.bz2"),
        (".tar.xz", "tar.xz"),
        (".txz", "tar.xz"),
        (".tar.zst", "tar.zst"),
        (".tzst", "tar.zst"),
    ] {
        if name.ends_with(ends) {
            return ext.into();
        }
    }
    p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default()
}

/// The name without its extension, two-part ones included.
pub(crate) fn stem_of(p: &Path) -> String {
    let name = p.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let lower = name.to_lowercase();
    for two in [".tar.gz", ".tar.bz2", ".tar.xz", ".tar.zst"] {
        if lower.ends_with(two) {
            return name[..name.len() - two.len()].to_string();
        }
    }
    match name.rfind('.') {
        Some(i) if i > 0 => name[..i].to_string(),
        _ => name,
    }
}

/// The formats each family can be written as, in the order the hub shows them.
fn writes(f: Family) -> &'static [&'static str] {
    match f {
        Family::Picture => &["png", "jpg", "webp", "tiff", "bmp"],
        Family::Sound => &["mp3", "flac", "wav", "aiff", "ogg"],
        Family::Table => &["xlsx", "csv", "tsv"],
        Family::Page => &["pdf", "png"],
        Family::Pdf => &["docx", "png"],
        Family::Words => &["docx", "md", "txt"],
        Family::Model => &["glb", "obj", "stl"],
        Family::Pack => &["zip", "7z", "tar"],
    }
}

/// What a file is by its name, and the format it already is (jpeg is jpg, tif is tiff, aif is aiff).
fn family_of(ext: &str) -> Option<(Family, &'static str)> {
    Some(match ext {
        "png" => (Family::Picture, "png"),
        "jpg" | "jpeg" | "jfif" => (Family::Picture, "jpg"),
        "webp" => (Family::Picture, "webp"),
        "tif" | "tiff" => (Family::Picture, "tiff"),
        "bmp" => (Family::Picture, "bmp"),
        "gif" => (Family::Picture, "gif"),
        "ico" => (Family::Picture, "ico"),
        // only read, as DotAny Image reads them
        "tga" => (Family::Picture, "tga"),
        "qoi" => (Family::Picture, "qoi"),
        "pbm" => (Family::Picture, "pbm"),
        "pgm" => (Family::Picture, "pgm"),
        "ppm" => (Family::Picture, "ppm"),
        "pnm" => (Family::Picture, "pnm"),
        "pam" => (Family::Picture, "pam"),
        "ff" | "farbfeld" => (Family::Picture, "ff"),
        "pcx" => (Family::Picture, "pcx"),
        "iff" | "ilbm" | "lbm" => (Family::Picture, "iff"),
        "mp3" => (Family::Sound, "mp3"),
        "flac" => (Family::Sound, "flac"),
        "wav" => (Family::Sound, "wav"),
        "aif" | "aiff" => (Family::Sound, "aiff"),
        "ogg" | "oga" => (Family::Sound, "ogg"),
        // read as Audio reads them, never written
        "mod" => (Family::Sound, "mod"),
        "xm" => (Family::Sound, "xm"),
        "s3m" => (Family::Sound, "s3m"),
        "it" => (Family::Sound, "it"),
        "au" | "snd" => (Family::Sound, "au"),
        "voc" => (Family::Sound, "voc"),
        "xlsx" => (Family::Table, "xlsx"),
        "xlsm" => (Family::Table, "xlsm"),
        "xls" => (Family::Table, "xls"),
        "ods" => (Family::Table, "ods"),
        "xlsb" => (Family::Table, "xlsb"),
        "fods" => (Family::Table, "fods"),
        "dbf" => (Family::Table, "dbf"),
        "sqlite" | "sqlite3" | "db" => (Family::Table, "sqlite"),
        "csv" => (Family::Table, "csv"),
        "tsv" | "tab" => (Family::Table, "tsv"),
        "pptx" => (Family::Page, "pptx"),
        "ppsx" => (Family::Page, "ppsx"),
        "potx" => (Family::Page, "potx"),
        "pdf" => (Family::Pdf, "pdf"),
        "docx" => (Family::Words, "docx"),
        "md" | "markdown" => (Family::Words, "md"),
        "txt" => (Family::Words, "txt"),
        "doc" => (Family::Words, "doc"),
        "rtf" => (Family::Words, "rtf"),
        "odt" => (Family::Words, "odt"),
        "glb" => (Family::Model, "glb"),
        "gltf" => (Family::Model, "gltf"),
        "obj" => (Family::Model, "obj"),
        "stl" => (Family::Model, "stl"),
        "3mf" => (Family::Model, "3mf"),
        "ply" => (Family::Model, "ply"),
        "off" => (Family::Model, "off"),
        "zip" => (Family::Pack, "zip"),
        "7z" => (Family::Pack, "7z"),
        "tar" => (Family::Pack, "tar"),
        "tar.gz" => (Family::Pack, "tar.gz"),
        "tar.bz2" => (Family::Pack, "tar.bz2"),
        "tar.xz" => (Family::Pack, "tar.xz"),
        "tar.zst" => (Family::Pack, "tar.zst"),
        "gz" => (Family::Pack, "gz"),
        "bz2" => (Family::Pack, "bz2"),
        "xz" => (Family::Pack, "xz"),
        "zst" => (Family::Pack, "zst"),
        "rar" => (Family::Pack, "rar"),
        "lzh" | "lha" => (Family::Pack, "lzh"),
        "cpio" => (Family::Pack, "cpio"),
        "ar" => (Family::Pack, "ar"),
        "deb" => (Family::Pack, "deb"),
        "rpm" => (Family::Pack, "rpm"),
        _ => return None,
    })
}

/// What DotAny Video opens. Films are never converted.
const VIDEO: &[&str] =
    &["mp4", "m4v", "mov", "mkv", "webm", "avi", "wmv", "asf", "flv", "ts", "mts", "m2ts", "3gp", "mpg", "mpeg", "ogv"];

/// Whether this build can write a family at all. Slides' decks are drawn only by Slides' own page, so none is offered:
/// see README.md.
fn family_works(f: Family) -> bool {
    match f {
        Family::Page => false,
        Family::Pdf => pdf::available(),
        _ => true,
    }
}

/// Formats a family can be turned into from this particular input: a PDF's pages become pictures, but its words only when
/// it is not a scan (that is decided on converting, not here).
fn formats_from(f: Family) -> Vec<&'static str> {
    match f {
        Family::Pdf => pdf::targets(),
        _ => writes(f).to_vec(),
    }
}

/// What the files can become. A format is offered when at least one file isn't already in it; a file already in the
/// chosen format is left alone when converting.
pub fn targets(paths: &[PathBuf]) -> Result<Targets, Refusal> {
    if paths.is_empty() {
        return Err(Refusal::Unsupported);
    }
    let exts: Vec<String> = paths.iter().map(|p| ext_of(p)).collect();
    if exts.iter().all(|e| VIDEO.contains(&e.as_str())) {
        return Err(Refusal::Video);
    }
    let mut family = None;
    for e in &exts {
        let Some((f, _)) = family_of(e) else { return Err(Refusal::Unsupported) };
        match family {
            None => family = Some(f),
            Some(g) if g != f => return Err(Refusal::Mixed),
            _ => {}
        }
    }
    let family = family.unwrap();
    if !family_works(family) {
        return Err(Refusal::Unsupported);
    }
    let formats: Vec<String> = formats_from(family)
        .into_iter()
        .filter(|x| exts.iter().any(|e| family_of(e).map(|(_, own)| own) != Some(*x)))
        .map(String::from)
        .collect();
    if formats.is_empty() {
        return Err(Refusal::Unsupported);
    }
    Ok(Targets { family, formats })
}

/// The files that will really be converted: those not already in the target format.
fn work(paths: &[PathBuf], target: &str) -> Vec<PathBuf> {
    paths.iter().filter(|p| family_of(&ext_of(p)).map(|(_, own)| own) != Some(target)).cloned().collect()
}

fn checked<'a>(paths: &'a [PathBuf], target: &str) -> Result<Family, Refusal> {
    let t = targets(paths)?;
    if !t.formats.iter().any(|f| f == target) {
        return Err(Refusal::Unsupported);
    }
    Ok(t.family)
}

/// The size the outputs will have together, in bytes, the way the save card says "About X". Quick: it reads headers,
/// not whole files, so it can be asked again on every move of the quality slider.
pub fn estimate(paths: &[PathBuf], target: &str, opts: &Options) -> u64 {
    let Ok(family) = checked(paths, target) else { return 0 };
    work(paths, target)
        .iter()
        .map(|p| {
            let size = std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
            match family {
                Family::Picture => picture::estimate(p, size, target, opts),
                Family::Sound => sound::estimate(p, size, target, opts),
                Family::Table => table::estimate(p, size, target),
                Family::Words => words::estimate(p, size, target),
                Family::Model => model::estimate(p, size, target),
                Family::Pack => pack::estimate(p, size, target, opts),
                Family::Pdf => pdf::estimate(p, size, target),
                Family::Page => 0,
            }
        })
        .sum()
}

/// One file's conversion. `out` hands out the names beside the original and takes the finished bytes; `progress` is
/// this file's own share, 0 to 1.
pub(crate) type Step<'a> = &'a dyn Fn(f32);

/// Converts every file not already in `target`, one after the other. `progress` goes from 0 to 1 over all of them.
/// Returns the outputs in the order made. On a damaged or unreadable file it stops there: what that file had written is
/// removed, earlier outputs stay and are listed in the error.
pub fn convert(
    paths: &[PathBuf],
    target: &str,
    opts: &Options,
    progress: impl Fn(f32),
) -> Result<Vec<PathBuf>, ConvertError> {
    let family = checked(paths, target).map_err(|r| ConvertError {
        reason: Reason::Refused(r),
        file: paths.first().cloned().unwrap_or_default(),
        done: Vec::new(),
    })?;
    let todo = work(paths, target);
    // each file's share follows its size, so one huge file among small ones doesn't make the line stand still
    let sizes: Vec<f64> =
        todo.iter().map(|p| std::fs::metadata(p).map(|m| m.len() as f64).unwrap_or(0.0).max(1.0)).collect();
    let total: f64 = sizes.iter().sum();
    let mut done: Vec<PathBuf> = Vec::new();
    let mut before = 0.0;
    progress(0.0);
    for (p, size) in todo.iter().zip(&sizes) {
        let share = size / total;
        let step = |f: f32| progress(((before + share * f.clamp(0.0, 1.0) as f64) / 1.0).min(1.0) as f32);
        let mut sink = out::Outputs::new(p);
        let result = match family {
            Family::Picture => picture::convert(p, target, opts, &mut sink, &step),
            Family::Sound => sound::convert(p, target, opts, &mut sink, &step),
            Family::Table => table::convert(p, target, &mut sink, &step),
            Family::Words => words::convert(p, target, opts, &mut sink, &step),
            Family::Model => model::convert(p, target, &mut sink, &step),
            Family::Pack => pack::convert(p, target, opts, &mut sink, &step),
            Family::Pdf => pdf::convert(p, target, &mut sink, &step),
            Family::Page => Err(Reason::Refused(Refusal::Unsupported)),
        };
        match result {
            Ok(()) => done.extend(sink.commit()),
            Err(reason) => {
                sink.abandon();
                return Err(ConvertError { reason, file: p.clone(), done });
            }
        }
        before += share;
        progress(before.min(1.0) as f32);
    }
    progress(1.0);
    Ok(done)
}

/// Whether the source can be read at all: the error a missing or locked file gives.
pub(crate) fn read_all(p: &Path) -> Result<Vec<u8>, Reason> {
    std::fs::read(p).map_err(|_| Reason::Unreadable)
}

pub(crate) fn open(p: &Path) -> Result<std::fs::File, Reason> {
    std::fs::File::open(p).map_err(|_| Reason::Unreadable)
}
