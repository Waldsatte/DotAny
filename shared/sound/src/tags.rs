//! The song's name, artist, album and cover. Editing them never touches a single byte of the sound.

use crate::formats::Kind;
use lofty::config::{ParseOptions, WriteOptions};
use lofty::file::{AudioFile, TaggedFileExt};
use lofty::picture::{MimeType, Picture, PictureType};
use lofty::probe::Probe;
use lofty::tag::{Accessor, Tag, TagExt, TagType};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Serialize, Clone, Default, Debug, PartialEq)]
pub struct Words {
    pub title: String,
    pub artist: String,
    pub album: String,
    /// A data: address, ready for the page. Empty without a cover.
    pub cover: String,
}

/// What the page changed. `None` leaves that field as the file has it.
#[derive(Deserialize, Clone, Default, Debug)]
pub struct Edit {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    /// The new cover was handed over earlier, with `set_cover`.
    #[serde(default)]
    pub cover: bool,
}

impl Edit {
    pub fn is_empty(&self) -> bool {
        self.title.is_none() && self.artist.is_none() && self.album.is_none() && !self.cover
    }
}

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

pub fn base64(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for c in data.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        out.push(B64[(n >> 18) as usize & 63] as char);
        out.push(B64[(n >> 12) as usize & 63] as char);
        out.push(if c.len() > 1 { B64[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if c.len() > 2 { B64[n as usize & 63] as char } else { '=' });
    }
    out
}

pub fn image_mime(data: &[u8]) -> Option<&'static str> {
    if data.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if data.starts_with(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]) {
        Some("image/png")
    } else {
        None
    }
}

pub fn data_address(data: &[u8]) -> Option<String> {
    image_mime(data).map(|m| format!("data:{m};base64,{}", base64(data)))
}

fn open(path: &Path) -> Result<lofty::file::TaggedFile, String> {
    Probe::open(path)
        .map_err(|e| e.to_string())?
        .guess_file_type()
        .map_err(|e| e.to_string())?
        .options(ParseOptions::new())
        .read()
        .map_err(|e| e.to_string())
}

fn front_cover(tag: &Tag) -> Option<&Picture> {
    tag.pictures().iter().find(|p| p.pic_type() == PictureType::CoverFront).or_else(|| tag.pictures().first())
}

/// What's in the file, and the file's whole main tag, to carry across a save in another format.
pub fn read(path: &Path) -> (Words, Option<Tag>) {
    let Ok(file) = open(path) else { return (Words::default(), None) };
    let mut words = Words::default();
    let primary = file.primary_tag().cloned();
    let mut order: Vec<&Tag> = Vec::new();
    if let Some(t) = file.primary_tag() {
        order.push(t);
    }
    order.extend(file.tags().iter().filter(|t| Some(t.tag_type()) != file.primary_tag().map(|p| p.tag_type())));
    for t in &order {
        if words.title.is_empty() {
            words.title = t.title().map(|s| s.to_string()).unwrap_or_default();
        }
        if words.artist.is_empty() {
            words.artist = t.artist().map(|s| s.to_string()).unwrap_or_default();
        }
        if words.album.is_empty() {
            words.album = t.album().map(|s| s.to_string()).unwrap_or_default();
        }
        if words.cover.is_empty() {
            if let Some(p) = front_cover(t) {
                words.cover = data_address(p.data()).unwrap_or_default();
            }
        }
    }
    (words, primary.or_else(|| file.first_tag().cloned()))
}

fn apply(tag: &mut Tag, edit: &Edit, cover: Option<&[u8]>) {
    let mut set = |text: &Option<String>, on: fn(&mut Tag, String), off: fn(&mut Tag)| {
        if let Some(v) = text {
            if v.is_empty() {
                off(tag);
            } else {
                on(tag, v.clone());
            }
        }
    };
    set(&edit.title, |t, v| t.set_title(v), |t| t.remove_title());
    set(&edit.artist, |t, v| t.set_artist(v), |t| t.remove_artist());
    set(&edit.album, |t, v| t.set_album(v), |t| t.remove_album());
    if let (true, Some(data)) = (edit.cover, cover) {
        tag.remove_picture_type(PictureType::CoverFront);
        let mime = if data.starts_with(&[0xFF, 0xD8]) { MimeType::Jpeg } else { MimeType::Png };
        tag.push_picture(Picture::unchecked(data.to_vec()).pic_type(PictureType::CoverFront).mime_type(mime).build());
    }
}

fn options() -> WriteOptions {
    // version 2.3 of ID3 is the one every player reads, Windows Explorer's included
    WriteOptions::default().use_id3v23(true).preferred_padding(0)
}

fn latin1(s: &str) -> bool {
    s.chars().all(|c| (c as u32) < 256)
}

/// Writes tags into a file that's still only a temp file. `carry` is the tag from the file the sound came from.
pub fn write(path: &Path, kind: Kind, carry: Option<&Tag>, edit: &Edit, cover: Option<&[u8]>) -> Result<(), String> {
    if edit.is_empty() && carry.is_none() {
        return Ok(());
    }
    let file = open(path)?;
    let main = file.primary_tag_type();
    let mut tag = file.tag(main).cloned().unwrap_or_else(|| Tag::new(main));
    if let Some(from) = carry {
        let mut from = from.clone();
        from.re_map(main);
        for item in from.items() {
            tag.insert(item.clone());
        }
        for pic in from.pictures() {
            tag.push_picture(pic.clone());
        }
    }
    apply(&mut tag, edit, cover);
    let others: Vec<Tag> = file.tags().iter().filter(|t| t.tag_type() != main).cloned().collect();
    drop(file);
    if tag.is_empty() {
        tag.remove_from_path(path, options()).map_err(|e| e.to_string())?;
    } else {
        tag.save_to_path(path, options()).map_err(|e| e.to_string())?;
    }
    // older tags in the same file keep saying the same thing, or go if they can't
    for mut other in others {
        let t = other.tag_type();
        if !edit.is_empty() {
            let fits = t != TagType::Id3v1
                || [&edit.title, &edit.artist, &edit.album].into_iter().flatten().all(|s| latin1(s) && s.len() <= 30);
            if !fits {
                strip_id3v1(path).map_err(|e| e.to_string())?;
                continue;
            }
            // only words live in these; covers stay in the main tag
            let words_only = Edit { cover: false, ..edit.clone() };
            apply(&mut other, &words_only, None);
            if other.is_empty() {
                other.remove_from_path(path, options()).map_err(|e| e.to_string())?;
            } else {
                other.save_to_path(path, options()).map_err(|e| e.to_string())?;
            }
        }
    }
    let _ = kind;
    Ok(())
}

// the last 128 bytes of the file, when they start with its mark
fn strip_id3v1(path: &Path) -> std::io::Result<()> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::OpenOptions::new().read(true).write(true).open(path)?;
    let len = f.metadata()?.len();
    if len < 128 {
        return Ok(());
    }
    let mut mark = [0u8; 3];
    f.seek(SeekFrom::Start(len - 128))?;
    f.read_exact(&mut mark)?;
    if &mark == b"TAG" {
        f.set_len(len - 128)?;
    }
    Ok(())
}

/// How many bytes the sound itself begins after, for tests. MP3 only.
pub fn sound_offset_mp3(bytes: &[u8]) -> usize {
    if bytes.len() > 10 && &bytes[..3] == b"ID3" {
        let s = (bytes[6] as usize) << 21 | (bytes[7] as usize) << 14 | (bytes[8] as usize) << 7 | bytes[9] as usize;
        return 10 + s;
    }
    0
}

/// The properties lofty reads, for tests.
pub fn duration_ms(path: &Path) -> Option<u128> {
    Some(open(path).ok()?.properties().duration().as_millis())
}
