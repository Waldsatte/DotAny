//! A text file in memory: what was read, how it was written, and how an edited text goes back to bytes.
//!
//! Line endings. The editors work in `\n` only. A file with one kind of ending keeps it for every line. A file
//! with mixed endings keeps each unchanged line's own ending: on save the new text is compared line by line with
//! the old, unchanged lines take their old ending, replaced lines take the ending of the line they replace, and
//! new lines take the ending of the line above (or the file's most common ending at the very top). Choosing LF or
//! CRLF in the status line converts the whole file; that is the only thing that ever rewrites other lines.

use crate::encoding::{self, TextEncoding};
use similar::{Algorithm, DiffOp, capture_diff_slices};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Eol {
    Lf,
    Crlf,
    Cr,
}

impl Eol {
    pub fn text(self) -> &'static str {
        match self {
            Eol::Lf => "\n",
            Eol::Crlf => "\r\n",
            Eol::Cr => "\r",
        }
    }
    pub fn from_label(label: &str) -> Option<Eol> {
        match label {
            "LF" => Some(Eol::Lf),
            "CRLF" => Some(Eol::Crlf),
            "CR" => Some(Eol::Cr),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EolStyle {
    None,
    Uniform(Eol),
    Mixed { most: Eol },
}

impl EolStyle {
    pub fn label(self) -> &'static str {
        match self {
            EolStyle::None | EolStyle::Uniform(Eol::Lf) => "LF",
            EolStyle::Uniform(Eol::Crlf) => "CRLF",
            EolStyle::Uniform(Eol::Cr) => "CR",
            EolStyle::Mixed { .. } => "Mixed",
        }
    }
}

/// Every line and the ending after it; the last line has none.
fn split_lines(text: &str) -> (Vec<&str>, Vec<Eol>) {
    let bytes = text.as_bytes();
    let (mut lines, mut eols) = (Vec::new(), Vec::new());
    let (mut start, mut i) = (0, 0);
    while let Some(off) = memchr::memchr2(b'\n', b'\r', &bytes[i..]) {
        let at = i + off;
        lines.push(&text[start..at]);
        if bytes[at] == b'\n' {
            eols.push(Eol::Lf);
            i = at + 1;
        } else if bytes.get(at + 1) == Some(&b'\n') {
            eols.push(Eol::Crlf);
            i = at + 2;
        } else {
            eols.push(Eol::Cr);
            i = at + 1;
        }
        start = i;
    }
    lines.push(&text[start..]);
    (lines, eols)
}

pub fn eol_style(text: &str) -> EolStyle {
    let (_, eols) = split_lines(text);
    let count = |k: Eol| eols.iter().filter(|e| **e == k).count();
    let (lf, crlf, cr) = (count(Eol::Lf), count(Eol::Crlf), count(Eol::Cr));
    let kinds = (lf > 0) as u8 + (crlf > 0) as u8 + (cr > 0) as u8;
    let most = if crlf >= lf && crlf >= cr {
        Eol::Crlf
    } else if lf >= cr {
        Eol::Lf
    } else {
        Eol::Cr
    };
    match kinds {
        0 => EolStyle::None,
        1 => EolStyle::Uniform(most),
        _ => EolStyle::Mixed { most },
    }
}

pub fn normalize_eol(text: &str) -> String {
    if !text.contains('\r') {
        return text.to_string();
    }
    text.replace("\r\n", "\n").replace('\r', "\n")
}

fn join_uniform(text: &str, eol: Eol) -> String {
    if eol == Eol::Lf { text.to_string() } else { text.replace('\n', eol.text()) }
}

/// Puts an edited `\n` text back on the old text's line endings, keeping every unchanged line's own.
pub fn merge_mixed(old_raw: &str, new_text: &str, most: Eol) -> String {
    let (old_lines, old_eols) = split_lines(old_raw);
    let new_lines: Vec<&str> = new_text.split('\n').collect();
    let mut eols: Vec<Eol> = vec![most; new_lines.len()];
    let old_eol = |i: usize| old_eols.get(i).copied();
    let mut previous: Option<Eol> = None;
    for op in capture_diff_slices(Algorithm::Myers, &old_lines, &new_lines) {
        match op {
            DiffOp::Equal { old_index, new_index, len } => {
                for k in 0..len {
                    let e = old_eol(old_index + k).or(previous).unwrap_or(most);
                    eols[new_index + k] = e;
                    previous = Some(e);
                }
            }
            DiffOp::Replace { old_index, old_len, new_index, new_len } => {
                for k in 0..new_len {
                    let from = if k < old_len { old_eol(old_index + k) } else { None };
                    let e = from.or(previous).unwrap_or(most);
                    eols[new_index + k] = e;
                    previous = Some(e);
                }
            }
            DiffOp::Insert { new_index, new_len, .. } => {
                for k in 0..new_len {
                    let e = previous.unwrap_or(most);
                    eols[new_index + k] = e;
                    previous = Some(e);
                }
            }
            DiffOp::Delete { .. } => {}
        }
    }
    let mut out = String::with_capacity(new_text.len() + new_lines.len());
    for (i, line) in new_lines.iter().enumerate() {
        out.push_str(line);
        if i + 1 < new_lines.len() {
            out.push_str(eols[i].text());
        }
    }
    out
}

#[derive(Debug, Clone)]
pub struct Loaded {
    /// The exact bytes on disk.
    pub bytes: Vec<u8>,
    pub encoding: TextEncoding,
    /// False: opens read-only with the encoding notice.
    pub certain: bool,
    /// True: some bytes can't be read or written back in this encoding, so editing would damage the file.
    pub lossy: bool,
    pub eol: EolStyle,
    /// The text with `\n` line endings, as the editors see it.
    pub text: String,
}

pub fn load(bytes: Vec<u8>) -> Option<Loaded> {
    let detected = encoding::detect(&bytes, true)?;
    Some(read_as(bytes, detected.encoding, detected.certain))
}

pub fn read_as(bytes: Vec<u8>, enc: TextEncoding, certain: bool) -> Loaded {
    let (raw, had_errors) = encoding::decode(&bytes, enc);
    let exact = !had_errors && encoding::is_exact(&bytes, enc);
    let eol = eol_style(&raw);
    let text = normalize_eol(&raw);
    Loaded { bytes, encoding: enc, certain: certain && exact, lossy: !exact, eol, text }
}

pub fn fresh_document() -> Loaded {
    Loaded {
        bytes: Vec::new(),
        encoding: TextEncoding::UTF8,
        certain: true,
        lossy: false,
        eol: EolStyle::None,
        text: String::new(),
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Overrides {
    pub encoding: Option<TextEncoding>,
    pub eol: Option<Eol>,
}

/// The bytes a save would write. The only failure is a character the encoding cannot hold; nothing is substituted.
pub fn render(doc: &Loaded, text: &str, over: Overrides) -> Result<Vec<u8>, Vec<char>> {
    let enc = over.encoding.unwrap_or(doc.encoding);
    let raw = match (over.eol, doc.eol) {
        (Some(eol), _) => join_uniform(text, eol),
        (None, EolStyle::None) => text.to_string(),
        (None, EolStyle::Uniform(eol)) => join_uniform(text, eol),
        (None, EolStyle::Mixed { most }) => {
            let (old_raw, _) = encoding::decode(&doc.bytes, doc.encoding);
            merge_mixed(&old_raw, text, most)
        }
    };
    encoding::encode(&raw, enc)
}
