//! Word documents, under one rule: nothing the editor can't represent is ever lost.
//!
//! The package stays as it is except for `word/document.xml` and, only when an edit needs them, styles, numbering,
//! relationships and media. Within `document.xml` every paragraph and table remembers its exact bytes, and an unedited one
//! is written back byte for byte.
//!
//! What a paragraph can hold and still be edited: text, tabs, line breaks, the toolbar's formatting, and properties
//! the toolbar doesn't know about, which are carried along unread. Things with no width of their own (bookmarks, comment ranges, comment
//! references, page breaks) stay in an edited paragraph at the same place in its words. Anything else in a paragraph
//! (tracked changes, fields, links, footnote references, text boxes, equations, section breaks and so on) makes that paragraph a
//! locked block: shown, never changed, and never removed. Tables with merged cells or anything unusual inside are locked too.
//!
//! If a promise can't be kept for the whole file, it opens read-only: Save As copies it exactly.

mod package;
mod write;
pub mod xml;

use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
pub use write::{Prepared, data_url};
use xml::{El, Node};

pub const WML: &str = "http://schemas.openxmlformats.org/wordprocessingml/2006/main";
const WML_STRICT: &str = "http://purl.oclc.org/ooxml/wordprocessingml/main";
const DOCUMENT: &str = "word/document.xml";

// ---------- what the editor sees ----------

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Run {
    pub t: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub b: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub i: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub u: bool,
    /// Half-points, as Word counts them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sz: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    /// Index into the document's table of run properties the toolbar doesn't know about.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub extra: Option<usize>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ListRef {
    pub kind: String,
    pub level: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub num: Option<u32>,
    /// A list made here that has no number yet; paragraphs with the same key belong together.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new: Option<u32>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct Para {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub align: Option<String>,
    /// Line spacing as a multiple, times 240 as Word counts it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub list: Option<ListRef>,
    pub runs: Vec<Run>,
    /// Paragraph properties the toolbar doesn't know about, unread.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub ppr: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Cell {
    pub paras: Vec<Para>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tcpr: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub head: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Row {
    pub cells: Vec<Cell>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub trpr: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Block {
    Para(Para),
    Table {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<u32>,
        rows: Vec<Row>,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        tblpr: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        grid: String,
    },
    Image {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        id: Option<u32>,
        /// The relationship inside the file, for showing it.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rel: Option<String>,
        /// A picture new to this file: data URL or local path.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        src: Option<String>,
        #[serde(default)]
        alt: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        width: Option<u32>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        height: Option<u32>,
    },
    Locked {
        id: u32,
        /// paragraph, table, picture or other
        kind: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        style: Option<String>,
        #[serde(default, skip_serializing_if = "Vec::is_empty")]
        rows: Vec<Vec<String>>,
    },
}

#[derive(Serialize, Clone, Debug)]
pub struct Opened {
    pub blocks: Vec<Block>,
    /// The run-property table: a run's `extra` points here.
    pub extras: Vec<String>,
    /// Why this file can only be read and copied, if it can't be edited.
    pub read_only: Option<String>,
    /// Something in the body is shown but can't be edited, or isn't shown at all.
    pub has_locked: bool,
    /// The format this was read from when the file isn't a Word document itself: it is shown, never written back.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

// ---------- what we remember about the file ----------

#[derive(Clone, Debug)]
pub(crate) struct Stored {
    /// The exact bytes this block came from, or was last written as.
    raw: String,
    /// What stood before it in the body that has no block of its own: spacing, body-level bookmarks.
    lead: String,
    model: Option<Block>,
    /// The paragraph tag's own attributes.
    p_attrs: String,
    /// Zero-width things, with where they sit in the paragraph's words (in characters).
    anchors: Vec<(usize, String)>,
    is_locked: bool,
}

#[derive(Clone, Debug)]
pub struct Docx {
    pub bytes: Vec<u8>,
    pub read_only: Option<String>,
    pub(crate) main: String,
    pub(crate) head: String,
    pub(crate) tail: String,
    pub(crate) order: Vec<u32>,
    pub(crate) stored: HashMap<u32, Stored>,
    pub(crate) extras: Vec<String>,
    pub(crate) styles: Styles,
    pub(crate) numbering: Numbering,
    pub(crate) rels: HashMap<String, String>,
    pub(crate) next_id: u32,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Styles {
    /// Style id to heading level.
    heading_of: HashMap<String, u32>,
    /// Heading level to style id.
    id_of: BTreeMap<u32, String>,
    /// Whether the styles part was read at all (set when it parses; nothing reads it back).
    known: bool,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Numbering {
    /// (numId, level) to "bullet" or "ordered".
    kinds: HashMap<(u32, u32), String>,
    nums: Vec<u32>,
    abstracts: Vec<u32>,
}

pub enum OpenError {
    NotDocx,
}

pub fn open(bytes: Vec<u8>) -> Result<(Docx, Opened), OpenError> {
    if !package::is_plain_zip(&bytes) {
        return Err(OpenError::NotDocx);
    }
    // the document is wherever the package says it is; nearly always the usual place
    let main = main_part(&bytes).unwrap_or_else(|| DOCUMENT.to_string());
    let Some(raw) = package::read_part(&bytes, &main) else { return Err(OpenError::NotDocx) };
    let Ok(whole) = String::from_utf8(raw) else { return Err(OpenError::NotDocx) };
    let bom = if whole.starts_with('\u{feff}') { "\u{feff}" } else { "" };
    let src = whole[bom.len()..].to_string();
    let Some(root) = xml::parse(&src) else { return Err(OpenError::NotDocx) };
    let styles = text_part(&bytes, "word/styles.xml").map(|s| read_styles(&s)).unwrap_or_default();
    let numbering = text_part(&bytes, "word/numbering.xml").map(|s| read_numbering(&s)).unwrap_or_default();
    let rels = text_part(&bytes, "word/_rels/document.xml.rels").map(|s| read_rels(&s)).unwrap_or_default();
    let settings = text_part(&bytes, "word/settings.xml").unwrap_or_default();
    let mut read_only = whole_file_problem(&root, &settings);
    if main != DOCUMENT {
        read_only = Some("layout".into());
    }

    let mut reader = Reading {
        src: &src,
        styles: &styles,
        numbering: &numbering,
        rels: &rels,
        extras: Vec::new(),
        extra_index: HashMap::new(),
        has_quiet: false,
    };
    let Some(body) = root.child("w:body") else { return Err(OpenError::NotDocx) };

    let mut docx = Docx {
        bytes,
        read_only: None,
        main: main.clone(),
        head: format!("{bom}{}", &src[..body.inner_start]),
        tail: String::new(),
        order: Vec::new(),
        stored: HashMap::new(),
        extras: Vec::new(),
        styles: styles.clone(),
        numbering: numbering.clone(),
        rels: rels.clone(),
        next_id: 1,
    };
    let mut blocks: Vec<Block> = Vec::new();
    // the last sectPr and whatever follows it stay with the end of the file
    let final_sect = body.kids.iter().rposition(|k| matches!(k, Node::El(e) if e.name == "w:sectPr"));
    let tail_from = final_sect
        .map(|i| if let Node::El(e) = &body.kids[i] { e.start } else { body.inner_end })
        .unwrap_or(body.inner_end);
    let mut lead = String::new();
    for (i, kid) in body.kids.iter().enumerate() {
        if final_sect.is_some_and(|f| i >= f) {
            break;
        }
        let Node::El(el) = kid else {
            let (a, b) = match kid {
                Node::Text(a, b) | Node::Other(a, b) => (*a, *b),
                Node::El(_) => unreachable!(),
            };
            lead.push_str(&src[a..b]);
            continue;
        };
        if is_quiet_body(&el.name) {
            lead.push_str(el.raw(&src));
            reader.has_quiet = true;
            continue;
        }
        let id = docx.next_id;
        docx.next_id += 1;
        let (block, stored) = reader.block(el, id, std::mem::take(&mut lead));
        docx.order.push(id);
        docx.stored.insert(id, stored);
        blocks.push(block);
    }
    docx.tail = format!("{lead}{}", &src[tail_from..]);
    settle_lists(&mut blocks, &mut docx.stored);
    docx.extras = reader.extras.clone();
    let has_locked = reader.has_quiet || docx.stored.values().any(|s| s.is_locked || !s.anchors.is_empty());

    // the promise itself, checked: nothing edited means the same bytes
    if write::same_as_read(&docx, &whole).is_none() && read_only.is_none() {
        read_only = Some("rebuild".into());
    }
    docx.read_only = read_only.clone();
    let extras = docx.extras.clone();
    Ok((docx, Opened { blocks, extras, read_only, has_locked, source: None }))
}

/// A blank document, with only what a blank document needs.
pub fn blank() -> Result<(Docx, Opened), String> {
    let bytes = write::blank_package()?;
    open(bytes).map_err(|_| "blank package".to_string())
}

/// A part as text, without the byte-order mark some producers put first.
fn text_part(bytes: &[u8], name: &str) -> Option<String> {
    package::read_part(bytes, name)
        .and_then(|b| String::from_utf8(b).ok())
        .map(|s| s.trim_start_matches('\u{feff}').to_string())
}

fn main_part(bytes: &[u8]) -> Option<String> {
    let rels = text_part(bytes, "_rels/.rels")?;
    let root = xml::parse(&rels)?;
    let main = root.els().find(|r| r.attr("Type").is_some_and(|t| t.ends_with("/officeDocument")))?;
    Some(main.attr("Target")?.trim_start_matches('/').to_string())
}

fn whole_file_problem(root: &El, settings: &str) -> Option<String> {
    let w = root.attr("xmlns:w");
    if w.as_deref() == Some(WML_STRICT) {
        return Some("strict".into());
    }
    if w.as_deref() != Some(WML) || root.name != "w:document" {
        return Some("prefix".into());
    }
    if settings.contains("<w:trackRevisions")
        && !settings.contains("<w:trackRevisions w:val=\"0\"")
        && !settings.contains("<w:trackRevisions w:val=\"false\"")
    {
        return Some("tracking".into());
    }
    if let Some(at) = settings.find("<w:documentProtection") {
        let tag = &settings[at..at + settings[at..].find('>').unwrap_or(0)];
        if tag.contains("w:enforcement=\"1\"")
            || tag.contains("w:enforcement=\"true\"")
            || tag.contains("w:enforcement=\"on\"")
        {
            return Some("protected".into());
        }
    }
    None
}

fn read_styles(src: &str) -> Styles {
    let mut out = Styles::default();
    let Some(root) = xml::parse(src) else { return out };
    out.known = true;
    for st in root.els().filter(|e| e.name == "w:style") {
        if st.attr("w:type").as_deref() != Some("paragraph") {
            continue;
        }
        let Some(id) = st.attr("w:styleId") else { continue };
        let name = st.child("w:name").and_then(|n| n.attr("w:val")).unwrap_or_default().to_lowercase();
        let level = match name.as_str() {
            "heading 1" => Some(1),
            "heading 2" => Some(2),
            "heading 3" => Some(3),
            _ => None,
        };
        if let Some(l) = level {
            out.heading_of.insert(id.clone(), l);
            out.id_of.entry(l).or_insert(id);
        }
    }
    out
}

fn read_numbering(src: &str) -> Numbering {
    let mut out = Numbering::default();
    let Some(root) = xml::parse(src) else { return out };
    let mut abstracts: HashMap<u32, HashMap<u32, String>> = HashMap::new();
    for a in root.els().filter(|e| e.name == "w:abstractNum") {
        let Some(id) = a.attr("w:abstractNumId").and_then(|v| v.parse().ok()) else { continue };
        out.abstracts.push(id);
        let mut levels = HashMap::new();
        for l in a.els().filter(|e| e.name == "w:lvl") {
            let Some(il) = l.attr("w:ilvl").and_then(|v| v.parse().ok()) else { continue };
            let fmt = l.child("w:numFmt").and_then(|f| f.attr("w:val")).unwrap_or_default();
            levels.insert(il, if fmt == "bullet" { "bullet".to_string() } else { "ordered".to_string() });
        }
        abstracts.insert(id, levels);
    }
    for n in root.els().filter(|e| e.name == "w:num") {
        let Some(id) = n.attr("w:numId").and_then(|v| v.parse().ok()) else { continue };
        out.nums.push(id);
        let Some(a) = n.child("w:abstractNumId").and_then(|e| e.attr("w:val")).and_then(|v| v.parse::<u32>().ok())
        else {
            continue;
        };
        if let Some(levels) = abstracts.get(&a) {
            for (l, k) in levels {
                out.kinds.insert((id, *l), k.clone());
            }
        }
    }
    out
}

fn read_rels(src: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    if let Some(root) = xml::parse(src) {
        for r in root.els().filter(|e| e.name == "Relationship") {
            if let (Some(id), Some(target)) = (r.attr("Id"), r.attr("Target")) {
                out.insert(id, target);
            }
        }
    }
    out
}

fn is_quiet_body(name: &str) -> bool {
    matches!(
        name,
        "w:bookmarkStart"
            | "w:bookmarkEnd"
            | "w:proofErr"
            | "w:permStart"
            | "w:permEnd"
            | "w:commentRangeStart"
            | "w:commentRangeEnd"
            | "w:moveFromRangeStart"
            | "w:moveFromRangeEnd"
            | "w:moveToRangeStart"
            | "w:moveToRangeEnd"
            | "w:customXmlInsRangeStart"
            | "w:customXmlInsRangeEnd"
            | "w:customXmlDelRangeStart"
            | "w:customXmlDelRangeEnd"
            | "w:customXmlMoveFromRangeStart"
            | "w:customXmlMoveFromRangeEnd"
            | "w:customXmlMoveToRangeStart"
            | "w:customXmlMoveToRangeEnd"
    )
}

// ---------- reading blocks ----------

pub(crate) struct Reading<'a> {
    pub(crate) src: &'a str,
    pub(crate) styles: &'a Styles,
    pub(crate) numbering: &'a Numbering,
    pub(crate) rels: &'a HashMap<String, String>,
    pub(crate) extras: Vec<String>,
    pub(crate) extra_index: HashMap<String, usize>,
    pub(crate) has_quiet: bool,
}

const DISPLAYABLE: [&str; 6] = ["png", "jpg", "jpeg", "gif", "bmp", "webp"];

impl Reading<'_> {
    fn block(&mut self, el: &El, id: u32, lead: String) -> (Block, Stored) {
        let raw = el.raw(self.src).to_string();
        let plain = |model, p_attrs, anchors| Stored {
            raw: raw.clone(),
            lead: lead.clone(),
            model,
            p_attrs,
            anchors,
            is_locked: false,
        };
        match el.name.as_str() {
            "w:p" => {
                if let Some(image) = self.image(el, id) {
                    return (image.clone(), plain(Some(image), String::new(), Vec::new()));
                }
                if let Some((para, anchors)) = self.para(el, id) {
                    // writing it anew must read back the same, or it can't be trusted to be edited
                    if write::regenerates(&para, &anchors, &self.extras, self.styles, self.numbering, el.attrs.as_str())
                    {
                        let block = Block::Para(para);
                        return (block.clone(), plain(Some(block), el.attrs.clone(), anchors));
                    }
                }
                let text = visible_text(el, self.src);
                let style = self.heading_style(el);
                let kind = if text.is_empty() && el.has_deep("w:drawing") { "picture" } else { "paragraph" };
                let block = Block::Locked { id, kind: kind.into(), text, style, rows: Vec::new() };
                (block, Stored { is_locked: true, ..plain(None, String::new(), Vec::new()) })
            }
            "w:tbl" => {
                if let Some(mut block) = self.table(el) {
                    if let Block::Table { id: slot, .. } = &mut block {
                        *slot = Some(id);
                    }
                    return (block.clone(), plain(Some(block), String::new(), Vec::new()));
                }
                let rows = el
                    .els()
                    .filter(|e| e.name == "w:tr")
                    .map(|tr| tr.els().filter(|c| c.name == "w:tc").map(|c| visible_lines(c, self.src)).collect())
                    .collect();
                let block = Block::Locked { id, kind: "table".into(), text: String::new(), style: None, rows };
                (block, Stored { is_locked: true, ..plain(None, String::new(), Vec::new()) })
            }
            _ => {
                let text = visible_lines(el, self.src);
                let block = Block::Locked { id, kind: "other".into(), text, style: None, rows: Vec::new() };
                (block, Stored { is_locked: true, ..plain(None, String::new(), Vec::new()) })
            }
        }
    }

    fn heading_style(&self, p: &El) -> Option<String> {
        let id = p.child("w:pPr")?.child("w:pStyle")?.attr("w:val")?;
        self.styles.heading_of.get(&id).map(|l| format!("h{l}"))
    }

    /// A paragraph holding one inline picture and nothing else.
    fn image(&self, p: &El, id: u32) -> Option<Block> {
        let mut pic = None;
        for k in &p.kids {
            let Node::El(e) = k else { continue };
            match e.name.as_str() {
                "w:pPr" => {
                    if e.has_deep("w:sectPr") || e.has_deep("w:pPrChange") {
                        return None;
                    }
                }
                "w:r" => {
                    for c in e.els() {
                        match c.name.as_str() {
                            "w:rPr" => {}
                            "w:drawing" if pic.is_none() => pic = Some(c),
                            _ => return None,
                        }
                    }
                }
                "w:proofErr" => {}
                _ => return None,
            }
        }
        let drawing = pic?;
        let inline = drawing.child("wp:inline")?;
        let rel = find_attr(inline, "a:blip", "r:embed")?;
        let target = self.rels.get(&rel)?;
        let ext = target.rsplit('.').next().unwrap_or("").to_lowercase();
        if !DISPLAYABLE.contains(&ext.as_str()) {
            return None;
        }
        let alt =
            inline.child("wp:docPr").and_then(|d| d.attr("descr").or_else(|| d.attr("title"))).unwrap_or_default();
        Some(Block::Image { id: Some(id), rel: Some(rel), src: None, alt, width: None, height: None })
    }

    fn extra_id(&mut self, raw: String) -> Option<usize> {
        if raw.is_empty() {
            return None;
        }
        if let Some(i) = self.extra_index.get(&raw) {
            return Some(*i);
        }
        let i = self.extras.len();
        self.extras.push(raw.clone());
        self.extra_index.insert(raw, i);
        Some(i)
    }

    fn run_props(&mut self, rpr: Option<&El>) -> Option<Run> {
        let mut run = Run::default();
        let Some(rpr) = rpr else { return Some(run) };
        let mut extra = String::new();
        for k in &rpr.kids {
            let Node::El(e) = k else { continue };
            let val = e.attr("w:val");
            let on = !matches!(val.as_deref(), Some("0") | Some("false") | Some("off"));
            match e.name.as_str() {
                "w:rPrChange" => return None,
                "w:b" if on => run.b = true,
                "w:i" if on => run.i = true,
                "w:u"
                    if val.as_deref() == Some("single")
                        && e.attr("w:color").is_none()
                        && e.attr("w:themeColor").is_none() =>
                {
                    run.u = true
                }
                "w:sz" if val.as_deref().and_then(|v| v.parse::<u32>().ok()).is_some() => {
                    run.sz = val.and_then(|v| v.parse().ok())
                }
                "w:color" if e.attr("w:themeColor").is_none() && val.as_deref().is_some_and(|v| v != "auto") => {
                    run.color = val
                }
                _ => extra.push_str(e.raw(self.src)),
            }
        }
        run.extra = self.extra_id(extra);
        Some(run)
    }

    fn para_props(&mut self, ppr: Option<&El>) -> Option<Para> {
        let mut para = Para::default();
        let Some(ppr) = ppr else { return Some(para) };
        let mut extra = String::new();
        for k in &ppr.kids {
            let Node::El(e) = k else { continue };
            let val = e.attr("w:val");
            match e.name.as_str() {
                "w:sectPr" | "w:pPrChange" => return None,
                "w:pStyle" => match val.as_deref().and_then(|v| self.styles.heading_of.get(v)) {
                    Some(l) => para.style = Some(format!("h{l}")),
                    None => extra.push_str(e.raw(self.src)),
                },
                "w:jc" => match val.as_deref() {
                    Some("center") => para.align = Some("center".into()),
                    Some("right") => para.align = Some("right".into()),
                    Some("both") => para.align = Some("justify".into()),
                    _ => extra.push_str(e.raw(self.src)),
                },
                "w:spacing" => {
                    let rule = e.attr("w:lineRule");
                    match (e.attr("w:line").and_then(|v| v.parse::<u32>().ok()), rule.as_deref()) {
                        (Some(line), Some("auto")) => {
                            para.line = Some(line);
                            // before and after stay as they were
                            let rest = xml_attrs_without(&e.attrs, &["w:line", "w:lineRule"]);
                            if !rest.trim().is_empty() {
                                extra.push_str(&format!("<w:spacing{rest}/>"));
                            }
                        }
                        _ => extra.push_str(e.raw(self.src)),
                    }
                }
                "w:numPr" => {
                    let num = e.child("w:numId").and_then(|n| n.attr("w:val")).and_then(|v| v.parse::<u32>().ok());
                    let level = e
                        .child("w:ilvl")
                        .and_then(|n| n.attr("w:val"))
                        .and_then(|v| v.parse::<u32>().ok())
                        .unwrap_or(0);
                    match num.and_then(|n| self.numbering.kinds.get(&(n, level)).map(|k| (n, k))) {
                        Some((n, k)) if e.els().all(|c| c.name == "w:numId" || c.name == "w:ilvl") => {
                            para.list = Some(ListRef { kind: k.clone(), level, num: Some(n), new: None })
                        }
                        _ => extra.push_str(e.raw(self.src)),
                    }
                }
                _ => extra.push_str(e.raw(self.src)),
            }
        }
        para.ppr = extra;
        Some(para)
    }

    /// The paragraph, and what in it has no width. None if it holds anything else.
    pub(crate) fn para(&mut self, p: &El, id: u32) -> Option<(Para, Vec<(usize, String)>)> {
        if p.kids.iter().any(|k| matches!(k, Node::Other(..))) {
            return None;
        }
        let mut para = self.para_props(p.child("w:pPr"))?;
        para.id = Some(id);
        let mut anchors = Vec::new();
        let mut offset = 0usize;
        for k in &p.kids {
            let Node::El(e) = k else { continue };
            match e.name.as_str() {
                "w:pPr" | "w:proofErr" => {}
                "w:bookmarkStart"
                | "w:bookmarkEnd"
                | "w:commentRangeStart"
                | "w:commentRangeEnd"
                | "w:permStart"
                | "w:permEnd" => anchors.push((offset, e.raw(self.src).to_string())),
                "w:r" => {
                    let rpr = e.child("w:rPr");
                    let mut run = self.run_props(rpr)?;
                    let mut quiet_run = false;
                    for c in &e.kids {
                        let Node::El(c) = c else {
                            if matches!(c, Node::Other(..)) {
                                return None;
                            }
                            continue;
                        };
                        match c.name.as_str() {
                            "w:rPr" | "w:lastRenderedPageBreak" => {}
                            "w:t" => {
                                if c.kids.iter().any(|k| !matches!(k, Node::Text(..))) {
                                    return None;
                                }
                                let words = c.text(self.src);
                                if words.contains(['\n', '\t', '\r']) {
                                    return None;
                                }
                                run.t.push_str(&words);
                            }
                            "w:tab" => run.t.push('\t'),
                            "w:cr" => run.t.push('\n'),
                            "w:br"
                                if c.attr("w:type").is_none_or(|v| v == "textWrapping")
                                    && c.attr("w:clear").is_none() =>
                            {
                                run.t.push('\n')
                            }
                            "w:br" | "w:commentReference" => quiet_run = true,
                            _ => return None,
                        }
                    }
                    if quiet_run {
                        // a page break or comment mark on its own run keeps its place; mixed with words it can't be split safely
                        if !run.t.is_empty() {
                            return None;
                        }
                        anchors.push((offset, e.raw(self.src).to_string()));
                        continue;
                    }
                    offset += run.t.chars().count();
                    if run.t.is_empty() {
                        continue;
                    }
                    match para.runs.last_mut() {
                        Some(last) if same_look(last, &run) => last.t.push_str(&run.t),
                        _ => para.runs.push(run),
                    }
                }
                _ => return None,
            }
        }
        Some((para, anchors))
    }

    fn table(&mut self, t: &El) -> Option<Block> {
        let mut rows = Vec::new();
        let mut tblpr = String::new();
        let mut grid = String::new();
        let mut width = None;
        for k in &t.kids {
            let Node::El(e) = k else {
                if matches!(k, Node::Other(..)) {
                    return None;
                }
                continue;
            };
            match e.name.as_str() {
                "w:tblPr" => tblpr = e.inner(self.src).to_string(),
                "w:tblGrid" => grid = e.raw(self.src).to_string(),
                "w:tr" => {
                    let mut cells = Vec::new();
                    let mut trpr = String::new();
                    for c in e.els() {
                        match c.name.as_str() {
                            "w:trPr" => {
                                if c.has_deep("w:ins") || c.has_deep("w:del") || c.has_deep("w:trPrChange") {
                                    return None;
                                }
                                trpr = c.inner(self.src).to_string();
                            }
                            "w:tc" => cells.push(self.cell(c)?),
                            _ => return None,
                        }
                    }
                    if width.is_some_and(|w| w != cells.len()) || cells.is_empty() {
                        return None;
                    }
                    width = Some(cells.len());
                    rows.push(Row { cells, trpr });
                }
                _ => return None,
            }
        }
        if rows.is_empty() {
            return None;
        }
        let first_is_head = rows[0].trpr.contains("<w:tblHeader") || tblpr.contains("w:firstRow=\"1\"");
        if first_is_head {
            for c in &mut rows[0].cells {
                c.head = true;
            }
        }
        Some(Block::Table { id: None, rows, tblpr, grid })
    }

    fn cell(&mut self, c: &El) -> Option<Cell> {
        let mut paras = Vec::new();
        let mut tcpr = String::new();
        for k in &c.kids {
            let Node::El(e) = k else {
                if matches!(k, Node::Other(..)) {
                    return None;
                }
                continue;
            };
            match e.name.as_str() {
                "w:tcPr" => {
                    if e.child("w:gridSpan").is_some()
                        || e.child("w:vMerge").is_some()
                        || e.child("w:hMerge").is_some()
                        || e.has_deep("w:tcPrChange")
                        || e.has_deep("w:cellIns")
                        || e.has_deep("w:cellDel")
                    {
                        return None;
                    }
                    tcpr = e.inner(self.src).to_string();
                }
                "w:p" => {
                    let (mut para, anchors) = self.para(e, 0)?;
                    if !anchors.is_empty() {
                        return None;
                    }
                    para.id = None;
                    paras.push(para);
                }
                _ => return None,
            }
        }
        if paras.is_empty() {
            return None;
        }
        Some(Cell { paras, tcpr, head: false })
    }
}

/// The editor nests lists by level: a run of paragraphs that jumps levels stays plain paragraphs with its numbering untouched,
/// and so do list paragraphs inside table cells.
fn settle_lists(blocks: &mut [Block], stored: &mut HashMap<u32, Stored>) {
    let unlist = |p: &mut Para| {
        if let Some(l) = p.list.take() {
            let num = l.num.unwrap_or(0);
            p.ppr = format!("<w:numPr><w:ilvl w:val=\"{}\"/><w:numId w:val=\"{num}\"/></w:numPr>{}", l.level, p.ppr);
        }
    };
    // a numbered heading is a heading: its numbering stays, unread
    for b in blocks.iter_mut() {
        if let Block::Para(p) = b {
            if p.style.is_some() && p.list.is_some() {
                unlist(p);
                if let Some(s) = p.id.and_then(|id| stored.get_mut(&id)) {
                    s.model = Some(Block::Para(p.clone()));
                }
            }
        }
    }
    let mut i = 0;
    while i < blocks.len() {
        let is_list = |b: &Block| matches!(b, Block::Para(Para { list: Some(_), .. }));
        if !is_list(&blocks[i]) {
            if let Block::Table { rows, .. } = &mut blocks[i] {
                for c in rows.iter_mut().flat_map(|r| r.cells.iter_mut()) {
                    c.paras.iter_mut().for_each(&unlist);
                }
            }
            if let Block::Table { id: Some(id), .. } = &blocks[i] {
                if let Some(s) = stored.get_mut(id) {
                    s.model = Some(blocks[i].clone());
                }
            }
            i += 1;
            continue;
        }
        let mut end = i;
        let mut regular = true;
        let mut prev: i64 = -1;
        while end < blocks.len() && is_list(&blocks[end]) {
            if let Block::Para(Para { list: Some(l), .. }) = &blocks[end] {
                if l.level as i64 > prev + 1 {
                    regular = false;
                }
                prev = l.level as i64;
            }
            end += 1;
        }
        if !regular {
            for b in &mut blocks[i..end] {
                if let Block::Para(p) = b {
                    unlist(p);
                    if let Some(s) = p.id.and_then(|id| stored.get_mut(&id)) {
                        s.model = Some(Block::Para(p.clone()));
                    }
                }
            }
        }
        i = end;
    }
}

fn same_look(a: &Run, b: &Run) -> bool {
    a.b == b.b && a.i == b.i && a.u == b.u && a.sz == b.sz && a.color == b.color && a.extra == b.extra
}

fn xml_attrs_without(attrs: &str, names: &[&str]) -> String {
    let mut out = String::new();
    let mut rest = attrs;
    while let Some(eq) = rest.find('=') {
        let key = rest[..eq].trim().to_string();
        let after = rest[eq + 1..].trim_start();
        let Some(quote) = after.chars().next() else { break };
        let Some(close) = after[1..].find(quote) else { break };
        let value = &after[..close + 2];
        if !names.contains(&key.as_str()) {
            out.push_str(&format!(" {key}={value}"));
        }
        rest = &after[close + 2..];
    }
    out
}

fn find_attr(el: &El, name: &str, attr: &str) -> Option<String> {
    for e in el.els() {
        if e.name == name {
            if let Some(v) = e.attr(attr) {
                return Some(v);
            }
        }
        if let Some(v) = find_attr(e, name, attr) {
            return Some(v);
        }
    }
    None
}

/// The words a person sees in something we won't edit.
fn visible_text(el: &El, src: &str) -> String {
    let mut out = String::new();
    gather(el, src, &mut out);
    out
}

fn visible_lines(el: &El, src: &str) -> String {
    let mut out = String::new();
    let mut first = true;
    for p in all_paragraphs(el) {
        if !first {
            out.push('\n');
        }
        first = false;
        gather(p, src, &mut out);
    }
    out
}

fn all_paragraphs(el: &El) -> Vec<&El> {
    let mut list = Vec::new();
    for e in el.els() {
        if e.name == "w:p" {
            list.push(e);
        } else {
            list.extend(all_paragraphs(e));
        }
    }
    list
}

fn gather(el: &El, src: &str, out: &mut String) {
    for e in el.els() {
        match e.name.as_str() {
            "w:t" => out.push_str(&e.text(src)),
            "w:tab" => out.push('\t'),
            "w:br" | "w:cr" => out.push('\n'),
            // deleted words, field codes and alternatives aren't what a reader sees
            "w:del" | "w:moveFrom" | "w:instrText" | "w:delInstrText" | "w:pPr" | "w:rPr" | "mc:Fallback"
            | "w:drawing" | "w:pict" | "w:object" => {}
            _ => gather(e, src, out),
        }
    }
}

// ---------- the file's own pictures ----------

impl Docx {
    /// A picture in the file, for showing.
    pub fn media(&self, rel: &str) -> Option<(Vec<u8>, String)> {
        let target = self.rels.get(rel)?;
        let part = if let Some(abs) = target.strip_prefix('/') { abs.to_string() } else { format!("word/{target}") };
        let bytes = package::read_part(&self.bytes, &part)?;
        let ext = part.rsplit('.').next().unwrap_or("").to_lowercase();
        let mime = match ext.as_str() {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            "bmp" => "image/bmp",
            "webp" => "image/webp",
            _ => "application/octet-stream",
        };
        Some((bytes, mime.to_string()))
    }
    pub fn part_names(&self) -> Vec<String> {
        package::part_names(&self.bytes).unwrap_or_default()
    }
}

pub fn part_names_of(bytes: &[u8]) -> Option<Vec<String>> {
    package::part_names(bytes)
}

pub fn read_part(bytes: &[u8], name: &str) -> Option<Vec<u8>> {
    package::read_part(bytes, name)
}
