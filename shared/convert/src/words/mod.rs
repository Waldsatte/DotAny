//! Words: docx, md and txt both ways; doc, rtf and odt are only read. What DotAny Text's Save As does, without its window:
//! - `docx` (Word files) and `import` (doc, rtf, odt) come from the dotany-words crate (`shared/words`), which Text
//!   builds from. A doc, rtf or odt becomes the very Word document Text shows for it;
//! - `md` is Text's article view for Markdown and plain words (`text/src/markdown.js`, `text/src/docx-view.js`) in Rust.
//!
//! The rules are Text's: Word to Markdown keeps headings, lists, tables and bold and italic, and drops colours, sizes and
//! line spacing; its pictures are written beside the Markdown as `<name> 图1.png`… and linked by relative paths (`beside`,
//! the same code as Text's Save As); to plain text it keeps only the words, a line per paragraph and table cells between
//! tabs; a text file becomes Word a paragraph per line, and Markdown as it is. A Markdown file Text would only show as
//! code (it holds something the article view can't show safely) goes to Word the same way a text file does: a paragraph
//! per line.

mod locked;
mod md;

use crate::beside;
use crate::out::Outputs;
use crate::{Options, Reason, Step};
use dotany_words::{docx, import};
use std::collections::HashSet;
use std::path::Path;

enum Source {
    /// Word's blocks and the document they came from (its pictures); for doc, rtf and odt the Word document Text makes
    /// of them.
    Word {
        blocks: Vec<docx::Block>,
        word: Box<docx::Docx>,
        made: bool,
    },
    Markdown(String),
    Text(String),
}

fn locked(b: &[u8]) -> bool {
    b.starts_with(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1])
        && cfb::CompoundFile::open(std::io::Cursor::new(b)).map(|c| c.exists("/EncryptionInfo")).unwrap_or(false)
}

/// Text as a person wrote it: the encoding found as Sheets and Text find it, line endings as the editor holds them.
fn words_of(b: &[u8]) -> Result<String, Reason> {
    let utf16 = b.starts_with(&[0xFF, 0xFE]) || b.starts_with(&[0xFE, 0xFF]);
    if !utf16 && b.contains(&0) {
        return Err(Reason::Damaged);
    }
    let d = crate::table::decode_text(b);
    Ok(d.replace("\r\n", "\n").replace('\r', "\n"))
}

fn read(p: &Path, b: Vec<u8>) -> Result<Source, Reason> {
    let ext = crate::ext_of(p);
    match ext.as_str() {
        "md" => return Ok(Source::Markdown(words_of(&b)?)),
        "txt" => return Ok(Source::Text(words_of(&b)?)),
        _ => {}
    }
    if locked(&b) {
        return Err(Reason::Locked);
    }
    // a damaged reader must never take the program down
    // Text's Word reader finds the document wherever the package says it is, not only at word/document.xml
    let word = ext == "docx" && b.starts_with(b"PK");
    std::panic::catch_unwind(move || match import::sniff(&b) {
        _ if word => {
            let (word, opened) = docx::open(b).map_err(|_| Reason::Damaged)?;
            Ok(Source::Word { blocks: opened.blocks, word: Box::new(word), made: false })
        }
        Some(import::Kind::Docx) => {
            let (word, opened) = docx::open(b).map_err(|_| Reason::Damaged)?;
            Ok(Source::Word { blocks: opened.blocks, word: Box::new(word), made: false })
        }
        Some(_) => {
            let (word, opened) = import::open(&b, &ext).ok_or(Reason::Damaged)?;
            Ok(Source::Word { blocks: opened.blocks, word: Box::new(word), made: true })
        }
        None => Err(Reason::Damaged),
    })
    .unwrap_or(Err(Reason::Damaged))
}

/// A new Word document from the article's blocks, as Text's export_word makes it. Pictures named in Markdown are found beside it.
fn word_from(blocks: &[docx::Block], dir: Option<&Path>) -> Result<Vec<u8>, Reason> {
    let (blank, _) = docx::blank().map_err(|_| Reason::Damaged)?;
    Ok(blank.prepare(blocks, dir).map_err(|_| Reason::Damaged)?.next.bytes)
}

/// Word to Markdown (`lang`: the language of the interface that asked, for the words a block it cannot carry is written
/// as). Each picture the document shows (once, however often it is shown) is written beside the Markdown
/// under `<the Markdown's name> 图1.png`…, a name free in the folder, and linked by it; the Markdown's own name is chosen
/// first so the pictures can carry it.
fn word_to_markdown(
    p: &Path,
    blocks: &[docx::Block],
    word: &docx::Docx,
    template: &str,
    lang: &str,
    out: &mut Outputs,
) -> Result<(), Reason> {
    let mut doc = md::blocks_to_doc(blocks, lang);
    let mut rels: Vec<String> = Vec::new();
    md::each_image(&mut doc, &mut |src| {
        if let Some(rel) = src.strip_prefix("docx:") {
            if !rels.iter().any(|r| r == rel) {
                rels.push(rel.to_string());
            }
        }
        Some(src.to_string())
    });
    if rels.is_empty() {
        return out.write("md", md::serialize_markdown(&doc).as_bytes());
    }
    let pictures: Vec<(String, Vec<u8>, &str)> = rels
        .into_iter()
        .filter_map(|rel| {
            let (bytes, mime) = word.media(&rel)?;
            Some((rel, bytes, beside::picture_ext(&mime)?))
        })
        .collect();
    let dir = p.parent().unwrap_or(Path::new(""));
    let md_name = beside::free_name(dir, out.stem(), "md", &HashSet::new());
    let md_stem = md_name.strip_suffix(".md").unwrap_or(&md_name).to_string();
    let exts: Vec<&str> = pictures.iter().map(|p| p.2).collect();
    let names = beside::picture_names(dir, &md_stem, template, &exts, &HashSet::from([md_name.to_lowercase()]));
    md::each_image(&mut doc, &mut |src| match src.strip_prefix("docx:") {
        // one that can't be read out of the file keeps its address, as Text's Save As keeps it
        Some(rel) => {
            Some(pictures.iter().position(|p| p.0 == rel).map_or_else(|| src.to_string(), |i| beside::link(&names[i])))
        }
        None => Some(src.to_string()),
    });
    out.write_named(&md_stem, "md", md::serialize_markdown(&doc).as_bytes())?;
    for ((_, bytes, ext), name) in pictures.iter().zip(&names) {
        out.write_beside(name.strip_suffix(&format!(".{ext}")).unwrap_or(name), ext, bytes)?;
    }
    Ok(())
}

/// `Options::picture` names the pictures Word to Markdown writes beside the Markdown; `Options::lang` is the language
/// of the words written for a block that can't be carried.
pub fn convert(p: &Path, target: &str, opts: &Options, out: &mut Outputs, step: Step) -> Result<(), Reason> {
    let (picture, lang) = (opts.picture.as_str(), opts.lang.as_str());
    let bytes = crate::read_all(p)?;
    step(0.1);
    let source = read(p, bytes)?;
    step(0.5);
    let dir = p.parent();
    let made: Vec<u8> = match (source, target) {
        (Source::Word { word, made: true, .. }, "docx") => word.bytes,
        (Source::Word { blocks, word, .. }, "md") => {
            word_to_markdown(p, &blocks, &word, picture, lang, out)?;
            step(0.9);
            return Ok(());
        }
        (Source::Word { blocks, .. }, _) => md::doc_to_plain(&md::blocks_to_doc(&blocks, lang)).into_bytes(),
        (Source::Markdown(text), "docx") => {
            let doc = md::parse_markdown(&text).unwrap_or_else(|| md::parse_text(&text));
            word_from(&md::doc_to_blocks(&doc), dir)?
        }
        (Source::Markdown(text), _) => match md::parse_markdown(&text) {
            Some(doc) => md::doc_to_plain(&doc).into_bytes(),
            // shown as code, it already is plain text
            None => text.into_bytes(),
        },
        (Source::Text(text), "docx") => word_from(&md::doc_to_blocks(&md::parse_text(&text)), dir)?,
        // the words stay as they are and only the extension changes; they are written as Text writes them
        (Source::Text(text), _) => md::serialize_text(&md::parse_text(&text)).into_bytes(),
    };
    step(0.9);
    out.write(target, &made)
}

pub fn estimate(p: &Path, size: u64, target: &str) -> u64 {
    let ext = crate::ext_of(p);
    match (ext.as_str(), target) {
        // a Word document is a zip of its words; an empty one is about 4 KB
        ("md" | "txt", "docx") => size / 3 + 4096,
        ("md" | "txt", _) => size,
        // Markdown carries the pictures beside it, as they are in the zip; the words are about a third of the rest
        ("docx", "md") => {
            let pictures = pictures_in(p);
            pictures.1 + size.saturating_sub(pictures.0) / 3
        }
        // the words of a Word file are about a third of the zip, more where it holds no pictures
        ("docx" | "odt", _) => size / 3,
        ("doc", "docx") => size / 3 + 4096,
        ("rtf", "docx") => size / 6 + 4096,
        ("doc", _) => size / 4,
        ("rtf", _) => size / 8,
        _ => size,
    }
}

/// The pictures a Word file keeps under word/media: (their size in the zip, their size written out).
fn pictures_in(p: &Path) -> (u64, u64) {
    let Ok(f) = std::fs::File::open(p) else { return (0, 0) };
    let Ok(mut zip) = zip::ZipArchive::new(std::io::BufReader::new(f)) else { return (0, 0) };
    let mut sizes = (0, 0);
    for i in 0..zip.len() {
        let Ok(e) = zip.by_index_raw(i) else { continue };
        let Ok(name) = e.name() else { continue };
        if name.starts_with("word/media/") && beside::picture_ext(&name).is_some() {
            sizes.0 += e.compressed_size();
            sizes.1 += e.size();
        }
    }
    sizes
}
