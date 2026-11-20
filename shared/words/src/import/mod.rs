//! Other word-processor formats, only ever read: Word 97–2003 (.doc), RTF and OpenDocument text (.odt).
//!
//! Each reader turns the file into the editor's blocks: paragraphs with their runs, headings, lists, tables and pictures.
//! Those blocks become a new Word package in memory, which is what the window shows, read-only, and what Save As writes.
//! The original file is never written. What a reader can't carry over (footnotes, comments, text boxes, links, merged
//! cells and the like) is never guessed at: the file says so with `missing`, and the page tells the person.

/// Something can't be shown or carried over; where it was noticed is kept for tests and for finding out why.
macro_rules! lack {
    ($sink:expr) => {
        $sink.lack(concat!(file!(), ":", line!()))
    };
}

mod doc;
mod odt;
mod rtf;

use crate::docx::{self, Block, Cell, Docx, ListRef, Opened, Para, Row, Run};

/// Which reader a file needs, from its first bytes; the name only decides that it opens read-only.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Doc,
    Rtf,
    Odt,
    /// A Word 2007+ file under an older name.
    Docx,
}

pub fn sniff(bytes: &[u8]) -> Option<Kind> {
    if bytes.starts_with(b"{\\rtf") {
        return Some(Kind::Rtf);
    }
    if bytes.starts_with(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]) {
        return Some(Kind::Doc);
    }
    if bytes.starts_with(b"PK\x03\x04") {
        let names = docx::part_names_of(bytes)?;
        if names.iter().any(|n| n == "content.xml") {
            return Some(Kind::Odt);
        }
        if names.iter().any(|n| n == "word/document.xml") {
            return Some(Kind::Docx);
        }
    }
    None
}

/// A file of a kind Text doesn't read (a Word file from Word 95 or older, or an encrypted one), rather than a damaged one.
pub fn is_unsupported_kind(bytes: &[u8]) -> bool {
    sniff(bytes) == Some(Kind::Doc) && doc::is_unsupported_kind(bytes)
}

/// The file extensions that open here and are never written back.
pub fn is_read_only_format(ext: &str) -> bool {
    ["doc", "rtf", "odt"].iter().any(|e| e.eq_ignore_ascii_case(ext))
}

/// What a reader found.
#[derive(Debug, Default)]
pub struct Imported {
    pub blocks: Vec<Block>,
    /// Something in the file couldn't be shown and won't be in a saved copy.
    pub missing: bool,
    /// Where in the readers each missing thing was noticed.
    pub why: Vec<&'static str>,
}

pub fn read(bytes: &[u8]) -> Option<(Kind, Imported)> {
    let kind = sniff(bytes)?;
    let found = match kind {
        Kind::Rtf => rtf::read(bytes)?,
        Kind::Doc => doc::read(bytes)?,
        Kind::Odt => odt::read(bytes)?,
        Kind::Docx => return Some((kind, Imported::default())),
    };
    Some((kind, found))
}

/// The file as a Word document in memory, read-only. None if no reader can take it.
pub fn open(bytes: &[u8], source: &str) -> Option<(Docx, Opened)> {
    let (kind, found) = read(bytes)?;
    let (mut word, mut opened) = if kind == Kind::Docx {
        // the package itself is what a saved copy holds, byte for byte, so nothing in it is lost
        let (word, mut opened) = docx::open(bytes.to_vec()).ok()?;
        opened.has_locked = false;
        (word, opened)
    } else {
        let (blank, _) = docx::blank().ok()?;
        let made = blank.prepare(&found.blocks, None).ok()?;
        let (word, mut opened) = docx::open(made.next.bytes).ok()?;
        opened.has_locked = found.missing;
        (word, opened)
    };
    word.read_only = Some("format".into());
    opened.read_only = Some("format".into());
    opened.source = Some(source.to_lowercase());
    Some((word, opened))
}

// ---------- building blocks, shared by the readers ----------

/// How a run looks, without its words.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Look {
    pub b: bool,
    pub i: bool,
    pub u: bool,
    pub sz: Option<u32>,
    pub color: Option<String>,
}

/// Paragraph properties a reader collects; spacing and indents are in twips and travel as Word properties the toolbar
/// doesn't show.
#[derive(Clone, Debug, Default)]
pub(crate) struct ParaLook {
    pub heading: Option<u32>,
    pub align: Option<&'static str>,
    pub line: Option<u32>,
    pub before: Option<i32>,
    pub after: Option<i32>,
    pub left: Option<i32>,
    pub right: Option<i32>,
    pub first: Option<i32>,
    pub list: Option<(&'static str, u32, u32)>,
}

impl ParaLook {
    fn apply(&self, p: &mut Para) {
        p.style = self.heading.filter(|l| (1..=3).contains(l)).map(|l| format!("h{l}"));
        p.align = self.align.map(str::to_string);
        p.line = self.line.filter(|l| *l > 0 && *l <= 2400);
        let mut ppr = String::new();
        if self.before.is_some() || self.after.is_some() {
            ppr.push_str(&format!(
                "<w:spacing w:before=\"{}\" w:after=\"{}\"/>",
                self.before.unwrap_or(0).clamp(0, 31680),
                self.after.unwrap_or(0).clamp(0, 31680)
            ));
        }
        let ind = [("w:left", self.left), ("w:right", self.right)]
            .iter()
            .filter_map(|(k, v)| v.filter(|v| *v != 0).map(|v| format!(" {k}=\"{}\"", v.clamp(-31680, 31680))))
            .collect::<String>();
        let first = match self.first {
            Some(f) if f > 0 => format!(" w:firstLine=\"{}\"", f.min(31680)),
            Some(f) if f < 0 => format!(" w:hanging=\"{}\"", (-f).min(31680)),
            _ => String::new(),
        };
        // a list brings its own indents
        if (!ind.is_empty() || !first.is_empty()) && self.list.is_none() {
            ppr.push_str(&format!("<w:ind{ind}{first}/>"));
        }
        p.ppr = ppr;
        // a new list in the Word file has one kind at every level, so bullets and numbers inside one list go apart;
        // the numbers still count on across a bulleted level between them
        p.list = self.list.map(|(kind, level, key)| ListRef {
            kind: kind.into(),
            level: level.min(8),
            num: None,
            new: Some(key.saturating_mul(2) + u32::from(kind == "bullet")),
        });
    }
}

/// Collects paragraphs, tables and pictures in reading order.
#[derive(Default)]
pub(crate) struct Sink {
    pub blocks: Vec<Block>,
    pub missing: bool,
    pub why: Vec<&'static str>,
    runs: Vec<Run>,
    /// A picture already stood for this paragraph, so an empty end to it adds nothing.
    had_picture: bool,
    rows: Vec<Row>,
    cells: Vec<Cell>,
    cell_paras: Vec<Para>,
}

impl Sink {
    pub fn lack(&mut self, at: &'static str) {
        self.missing = true;
        if self.why.len() < 32 {
            self.why.push(at);
        }
    }

    pub fn text(&mut self, words: &str, look: &Look) {
        if words.is_empty() {
            return;
        }
        let same =
            |r: &Run| r.b == look.b && r.i == look.i && r.u == look.u && r.sz == look.sz && r.color == look.color;
        match self.runs.last_mut() {
            Some(last) if same(last) => last.t.push_str(words),
            _ => self.runs.push(Run {
                t: words.to_string(),
                b: look.b,
                i: look.i,
                u: look.u,
                sz: look.sz,
                color: look.color.clone(),
                extra: None,
            }),
        }
    }

    pub fn has_words(&self) -> bool {
        self.runs.iter().any(|r| !r.t.is_empty())
    }

    fn take_para(&mut self, look: &ParaLook) -> Para {
        let mut p = Para { runs: std::mem::take(&mut self.runs), ..Default::default() };
        look.apply(&mut p);
        p
    }

    /// The end of a paragraph; inside a table it belongs to the cell being filled.
    pub fn end_para(&mut self, look: &ParaLook, in_table: bool) {
        if in_table {
            let mut p = self.take_para(look);
            p.list = None;
            p.style = None;
            self.cell_paras.push(p);
            return;
        }
        self.close_table();
        if self.had_picture && self.runs.is_empty() {
            self.had_picture = false;
            return;
        }
        self.had_picture = false;
        let p = self.take_para(look);
        self.blocks.push(Block::Para(p));
    }

    pub fn end_cell(&mut self, look: &ParaLook) {
        if self.has_words() || self.cell_paras.is_empty() {
            self.end_para(look, true);
        } else {
            self.runs.clear();
        }
        let paras = std::mem::take(&mut self.cell_paras);
        self.cells.push(Cell { paras, tcpr: String::new(), head: false });
    }

    pub fn end_row(&mut self) {
        if !self.cell_paras.is_empty() || self.has_words() {
            self.end_cell(&ParaLook::default());
        }
        let cells = std::mem::take(&mut self.cells);
        if !cells.is_empty() {
            self.rows.push(Row { cells, trpr: String::new() });
        }
    }

    pub fn in_table(&self) -> bool {
        !self.rows.is_empty() || !self.cells.is_empty() || !self.cell_paras.is_empty()
    }

    /// Rows of different widths are filled out with empty cells; that the layout differs is said.
    pub fn close_table(&mut self) {
        if !self.cells.is_empty() || !self.cell_paras.is_empty() {
            self.end_row();
        }
        if self.rows.is_empty() {
            return;
        }
        let mut rows = std::mem::take(&mut self.rows);
        let width = rows.iter().map(|r| r.cells.len()).max().unwrap_or(1);
        for r in &mut rows {
            if r.cells.len() < width {
                lack!(self);
                r.cells.resize_with(width, || Cell { paras: vec![Para::default()], tcpr: String::new(), head: false });
            }
            for c in &mut r.cells {
                if c.paras.is_empty() {
                    c.paras.push(Para::default());
                }
            }
        }
        self.blocks.push(Block::Table { id: None, rows, tblpr: String::new(), grid: String::new() });
    }

    /// A picture is a block of its own: words before it end their paragraph there. Inside a table it can't be shown.
    pub fn picture(&mut self, bytes: &[u8], width: Option<u32>, height: Option<u32>, look: &ParaLook, in_table: bool) {
        let Some(mime) = picture_type(bytes) else {
            lack!(self);
            return;
        };
        if in_table {
            lack!(self);
            return;
        }
        self.close_table();
        if self.has_words() {
            let p = self.take_para(look);
            self.blocks.push(Block::Para(p));
        }
        self.runs.clear();
        let src = format!("data:{mime};base64,{}", base64(bytes));
        let size = |v: Option<u32>| v.filter(|v| *v > 0 && *v < 20000);
        self.blocks.push(Block::Image {
            id: None,
            rel: None,
            src: Some(src),
            alt: String::new(),
            width: size(width),
            height: size(height),
        });
        self.had_picture = true;
    }

    pub fn finish(mut self, look: &ParaLook) -> Imported {
        if self.in_table() {
            self.close_table();
        }
        if self.has_words() {
            self.end_para(look, false);
        }
        // a document that ends with an empty paragraph after its last words: Word's own last mark, not content
        while matches!(self.blocks.last(), Some(Block::Para(p)) if p.runs.is_empty() && p.list.is_none() && p.style.is_none())
            && self.blocks.len() > 1
        {
            self.blocks.pop();
        }
        Imported { blocks: self.blocks, missing: self.missing, why: self.why }
    }
}

/// Only what the editor and Word both show; anything else is reported missing by the caller.
fn picture_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else {
        None
    }
}

fn base64(bytes: &[u8]) -> String {
    const ABC: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n =
            (chunk[0] as u32) << 16 | (*chunk.get(1).unwrap_or(&0) as u32) << 8 | *chunk.get(2).unwrap_or(&0) as u32;
        out.push(ABC[(n >> 18) as usize & 63] as char);
        out.push(ABC[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { ABC[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { ABC[n as usize & 63] as char } else { '=' });
    }
    out
}

/// A Windows code page as an encoding; None for ones with no decoder here, which then count as missing.
pub(crate) fn code_page(cp: u32) -> Option<&'static encoding_rs::Encoding> {
    let label: &[u8] = match cp {
        0 | 1252 => b"windows-1252",
        1250 => b"windows-1250",
        1251 => b"windows-1251",
        1253 => b"windows-1253",
        1254 => b"windows-1254",
        1255 => b"windows-1255",
        1256 => b"windows-1256",
        1257 => b"windows-1257",
        1258 => b"windows-1258",
        874 => b"windows-874",
        932 => b"shift_jis",
        936 => b"gbk",
        949 => b"euc-kr",
        950 => b"big5",
        866 => b"ibm866",
        10000 => b"macintosh",
        65001 => b"utf-8",
        _ => return None,
    };
    encoding_rs::Encoding::for_label(label)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_standard() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }
}
