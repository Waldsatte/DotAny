//! Writing: unedited blocks as their original bytes, edited ones anew, with every property the toolbar doesn't know about
//! carried along and put in the place the file format requires.

use super::xml::{self, El, Node, escape_attr, escape_text};
use super::{Block, Cell, Docx, ListRef, Para, Row, Run, Stored, Styles, WML, package};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

const PPR_ORDER: [&str; 36] = [
    "pStyle",
    "keepNext",
    "keepLines",
    "pageBreakBefore",
    "framePr",
    "widowControl",
    "numPr",
    "suppressLineNumbers",
    "pBdr",
    "shd",
    "tabs",
    "suppressAutoHyphens",
    "kinsoku",
    "wordWrap",
    "overflowPunct",
    "topLinePunct",
    "autoSpaceDE",
    "autoSpaceDN",
    "bidi",
    "adjustRightInd",
    "snapToGrid",
    "spacing",
    "ind",
    "contextualSpacing",
    "mirrorIndents",
    "suppressOverlap",
    "jc",
    "textDirection",
    "textAlignment",
    "textboxTightWrap",
    "outlineLvl",
    "divId",
    "cnfStyle",
    "rPr",
    "sectPr",
    "pPrChange",
];
const RPR_ORDER: [&str; 39] = [
    "rStyle",
    "rFonts",
    "b",
    "bCs",
    "i",
    "iCs",
    "caps",
    "smallCaps",
    "strike",
    "dstrike",
    "outline",
    "shadow",
    "emboss",
    "imprint",
    "noProof",
    "snapToGrid",
    "vanish",
    "webHidden",
    "color",
    "spacing",
    "w",
    "kern",
    "position",
    "sz",
    "szCs",
    "highlight",
    "u",
    "effect",
    "bdr",
    "shd",
    "fitText",
    "vertAlign",
    "rtl",
    "cs",
    "em",
    "lang",
    "eastAsianLayout",
    "specVanish",
    "oMath",
];

/// Children of a property list, as (local name, raw bytes).
fn children_of(raw: &str) -> Vec<(String, String)> {
    if raw.is_empty() {
        return Vec::new();
    }
    let wrapped = format!("<x>{raw}</x>");
    let Some(root) = xml::parse(&wrapped) else { return vec![("?".into(), raw.to_string())] };
    root.els()
        .map(|e| (e.name.rsplit(':').next().unwrap_or(&e.name).to_string(), e.raw(&wrapped).to_string()))
        .collect()
}

/// In the order the format requires; things we don't recognize stay after what they followed.
fn ordered(order: &[&str], mut items: Vec<(String, String)>) -> String {
    let rank = |name: &str| order.iter().position(|o| *o == name);
    let mut last = 0usize;
    let mut keyed: Vec<(usize, String)> = Vec::with_capacity(items.len());
    for (name, raw) in items.drain(..) {
        if let Some(r) = rank(&name) {
            last = r;
        }
        keyed.push((rank(&name).unwrap_or(last), raw));
    }
    keyed.sort_by_key(|k| k.0);
    keyed.into_iter().map(|k| k.1).collect()
}

pub struct Context<'a> {
    pub extras: &'a [String],
    pub styles: &'a Styles,
    /// New list keys to the numbers given to them.
    pub new_lists: &'a HashMap<u32, u32>,
}

fn style_id(styles: &Styles, name: &str) -> String {
    let level: u32 = name[1..].parse().unwrap_or(1);
    styles.id_of.get(&level).cloned().unwrap_or_else(|| format!("Heading{level}"))
}

fn para_props(para: &Para, ctx: &Context) -> String {
    let mut items = children_of(&para.ppr);
    let gone = |items: &mut Vec<(String, String)>, name: &str| items.retain(|(n, _)| n != name);
    if let Some(style) = &para.style {
        gone(&mut items, "pStyle");
        items.push(("pStyle".into(), format!("<w:pStyle w:val=\"{}\"/>", escape_attr(&style_id(ctx.styles, style)))));
    }
    if let Some(list) = &para.list {
        let num = list.num.or_else(|| list.new.and_then(|k| ctx.new_lists.get(&k).copied()));
        if let Some(num) = num {
            gone(&mut items, "numPr");
            items.push((
                "numPr".into(),
                format!("<w:numPr><w:ilvl w:val=\"{}\"/><w:numId w:val=\"{num}\"/></w:numPr>", list.level),
            ));
        }
    }
    if let Some(line) = para.line {
        // before and after stay as they were
        let rest = items
            .iter()
            .find(|(n, _)| n == "spacing")
            .map(|(_, raw)| {
                raw.trim_start_matches("<w:spacing").trim_end_matches('>').trim_end_matches('/').to_string()
            })
            .unwrap_or_default();
        gone(&mut items, "spacing");
        items.push(("spacing".into(), format!("<w:spacing{rest} w:line=\"{line}\" w:lineRule=\"auto\"/>")));
    }
    if let Some(align) = &para.align {
        gone(&mut items, "jc");
        let val = if align == "justify" { "both" } else { align.as_str() };
        items.push(("jc".into(), format!("<w:jc w:val=\"{}\"/>", escape_attr(val))));
    }
    if items.is_empty() {
        return String::new();
    }
    format!("<w:pPr>{}</w:pPr>", ordered(&PPR_ORDER, items))
}

fn run_props(run: &Run, ctx: &Context) -> String {
    let mut items = run.extra.and_then(|i| ctx.extras.get(i)).map(|raw| children_of(raw)).unwrap_or_default();
    let set = |items: &mut Vec<(String, String)>, name: &str, raw: String| {
        items.retain(|(n, _)| n != name);
        items.push((name.to_string(), raw));
    };
    if run.b {
        set(&mut items, "b", "<w:b/>".into());
    }
    if run.i {
        set(&mut items, "i", "<w:i/>".into());
    }
    if run.u {
        set(&mut items, "u", "<w:u w:val=\"single\"/>".into());
    }
    if let Some(sz) = run.sz {
        set(&mut items, "sz", format!("<w:sz w:val=\"{sz}\"/>"));
    }
    if let Some(color) = &run.color {
        set(&mut items, "color", format!("<w:color w:val=\"{}\"/>", escape_attr(color)));
    }
    if items.is_empty() {
        return String::new();
    }
    format!("<w:rPr>{}</w:rPr>", ordered(&RPR_ORDER, items))
}

fn run_xml(text: &str, props: &str, out: &mut String) {
    if text.is_empty() {
        return;
    }
    out.push_str("<w:r>");
    out.push_str(props);
    let mut piece = String::new();
    let flush = |piece: &mut String, out: &mut String| {
        if !piece.is_empty() {
            out.push_str("<w:t xml:space=\"preserve\">");
            out.push_str(&escape_text(piece));
            out.push_str("</w:t>");
            piece.clear();
        }
    };
    for c in text.chars() {
        match c {
            '\t' => {
                flush(&mut piece, out);
                out.push_str("<w:tab/>");
            }
            '\n' => {
                flush(&mut piece, out);
                out.push_str("<w:br/>");
            }
            c => piece.push(c),
        }
    }
    flush(&mut piece, out);
    out.push_str("</w:r>");
}

/// A paragraph, anew. Anchors go at their place in its words.
pub fn para_xml(para: &Para, p_attrs: &str, anchors: &[(usize, String)], ctx: &Context) -> String {
    let mut out = format!("<w:p{p_attrs}>");
    out.push_str(&para_props(para, ctx));
    let mut anchors = anchors.iter().peekable();
    let mut at = 0usize;
    for run in &para.runs {
        let props = run_props(run, ctx);
        let chars: Vec<char> = run.t.chars().collect();
        let mut from = 0usize;
        while let Some((offset, raw)) = anchors.peek() {
            if *offset > at + chars.len() || (*offset == at + chars.len() && !chars.is_empty()) {
                break;
            }
            let cut = offset.saturating_sub(at).min(chars.len());
            if cut > from {
                run_xml(&chars[from..cut].iter().collect::<String>(), &props, &mut out);
                from = cut;
            }
            out.push_str(raw);
            anchors.next();
        }
        run_xml(&chars[from..].iter().collect::<String>(), &props, &mut out);
        at += chars.len();
    }
    for (_, raw) in anchors {
        out.push_str(raw);
    }
    out.push_str("</w:p>");
    out
}

const NEW_TABLE_PROPS: &str = "<w:tblW w:w=\"5000\" w:type=\"pct\"/><w:tblBorders><w:top w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/><w:left w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/><w:bottom w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/><w:right w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/><w:insideH w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/><w:insideV w:val=\"single\" w:sz=\"4\" w:space=\"0\" w:color=\"auto\"/></w:tblBorders><w:tblLook w:firstRow=\"1\"/>";

fn table_xml(rows: &[Row], tblpr: &str, grid: &str, ctx: &Context) -> String {
    let columns = rows.iter().map(|r| r.cells.len()).max().unwrap_or(1).max(1);
    let same_grid = !grid.is_empty() && grid.matches("<w:gridCol").count() == columns;
    let width = 9000 / columns;
    let mut out = String::from("<w:tbl><w:tblPr>");
    out.push_str(if tblpr.is_empty() { NEW_TABLE_PROPS } else { tblpr });
    out.push_str("</w:tblPr>");
    if same_grid {
        out.push_str(grid);
    } else {
        out.push_str("<w:tblGrid>");
        for _ in 0..columns {
            out.push_str(&format!("<w:gridCol w:w=\"{width}\"/>"));
        }
        out.push_str("</w:tblGrid>");
    }
    for row in rows {
        out.push_str("<w:tr>");
        let head_new = row.trpr.is_empty() && row.cells.first().is_some_and(|c| c.head);
        if !row.trpr.is_empty() || head_new {
            out.push_str("<w:trPr>");
            out.push_str(if head_new { "<w:tblHeader/>" } else { &row.trpr });
            out.push_str("</w:trPr>");
        }
        for cell in &row.cells {
            out.push_str(&cell_xml(cell, width, ctx));
        }
        out.push_str("</w:tr>");
    }
    out.push_str("</w:tbl>");
    out
}

fn cell_xml(cell: &Cell, width: usize, ctx: &Context) -> String {
    let mut out = String::from("<w:tc><w:tcPr>");
    if cell.tcpr.is_empty() {
        out.push_str(&format!("<w:tcW w:w=\"{width}\" w:type=\"dxa\"/>"));
    } else {
        out.push_str(&cell.tcpr);
    }
    out.push_str("</w:tcPr>");
    if cell.paras.is_empty() {
        out.push_str("<w:p/>");
    }
    for para in &cell.paras {
        let mut para = para.clone();
        // a header cell made here reads as a header anywhere
        if cell.head && cell.tcpr.is_empty() {
            for run in &mut para.runs {
                run.b = true;
            }
        }
        out.push_str(&para_xml(&para, "", &[], ctx));
    }
    out.push_str("</w:tc>");
    out
}

/// Keeps every anchor: words the edit left alone keep theirs in place, and one inside the changed words moves to where they start.
fn remap(old: &str, new: &str, anchors: &[(usize, String)]) -> Vec<(usize, String)> {
    let (a, b): (Vec<char>, Vec<char>) = (old.chars().collect(), new.chars().collect());
    let n = a.len().min(b.len());
    let mut pre = 0;
    while pre < n && a[pre] == b[pre] {
        pre += 1;
    }
    let mut suf = 0;
    while suf < n - pre && a[a.len() - 1 - suf] == b[b.len() - 1 - suf] {
        suf += 1;
    }
    anchors
        .iter()
        .map(|(o, raw)| {
            let o = *o;
            let moved = if o <= pre {
                o
            } else if o >= a.len() - suf {
                b.len() - (a.len() - o)
            } else {
                pre
            };
            (moved, raw.clone())
        })
        .collect()
}

fn para_text(p: &Para) -> String {
    p.runs.iter().map(|r| r.t.as_str()).collect()
}

/// Nothing edited: the same bytes, or None if that isn't so.
pub fn same_as_read(docx: &Docx, src: &str) -> Option<()> {
    let mut out = docx.head.clone();
    for id in &docx.order {
        let s = docx.stored.get(id)?;
        out.push_str(&s.lead);
        out.push_str(&s.raw);
    }
    out.push_str(&docx.tail);
    (out == src).then_some(())
}

/// What a paragraph reads back as once written anew must be what it was.
pub fn regenerates(
    para: &Para,
    anchors: &[(usize, String)],
    reader_extras: &[String],
    styles: &Styles,
    numbering: &super::Numbering,
    p_attrs: &str,
) -> bool {
    let empty = HashMap::new();
    let ctx = Context { extras: reader_extras, styles, new_lists: &empty };
    let written = para_xml(para, p_attrs, anchors, &ctx);
    let wrapped = format!("<x>{written}</x>");
    let Some(root) = xml::parse(&wrapped) else { return false };
    let Some(p) = root.child("w:p") else { return false };
    let rels = HashMap::new();
    let mut again = super::Reading {
        src: &wrapped,
        styles,
        numbering,
        rels: &rels,
        extras: reader_extras.to_vec(),
        extra_index: reader_extras.iter().enumerate().map(|(i, s)| (s.clone(), i)).collect(),
        has_quiet: false,
    };
    let Some((back, back_anchors)) = again.para(p, para.id.unwrap_or(0)) else { return false };
    normalized(&back) == normalized(para) && back_anchors == anchors && again.extras.len() == reader_extras.len()
}

/// Extra properties may come back in file order, which is the same set of things.
fn normalized(p: &Para) -> Para {
    let mut p = p.clone();
    let mut items: Vec<String> = children_of(&p.ppr).into_iter().map(|c| c.1).collect();
    items.sort();
    p.ppr = items.concat();
    p
}

pub struct Prepared {
    pub next: Docx,
    pub changed: bool,
    /// New list keys and the numbers they were given, for the page to keep.
    pub assigned: Vec<(u32, u32)>,
}

impl Docx {
    /// What the file becomes with these blocks. Nothing on disk or in memory changes until the caller takes `next`.
    pub fn prepare(&self, blocks: &[Block], base_dir: Option<&Path>) -> Result<Prepared, String> {
        // a file with no blocks shows one empty paragraph to type in; left empty, it isn't part of the file
        let blocks = match blocks {
            [Block::Para(p)]
                if self.order.is_empty()
                    && p.id.is_none()
                    && p.runs.is_empty()
                    && p.style.is_none()
                    && p.align.is_none()
                    && p.line.is_none()
                    && p.list.is_none() =>
            {
                &[]
            }
            _ => blocks,
        };
        let present: HashSet<u32> = blocks.iter().filter_map(block_id).collect();
        // the guarantee, enforced here too: a locked block can't be left out
        for (id, s) in &self.stored {
            if s.is_locked && !present.contains(id) {
                return Err("a locked block is missing".into());
            }
        }
        let mut parts = Parts::new(self);
        let new_lists = parts.allocate_lists(blocks)?;
        parts.need_headings(blocks);
        let styles = parts.styles_after();
        let ctx = Context { extras: &self.extras, styles: &styles, new_lists: &new_lists };

        // what was attached to a removed block moves to the next one that remains
        let mut orphan_lead: HashMap<u32, String> = HashMap::new();
        let mut orphan_anchors: HashMap<u32, Vec<String>> = HashMap::new();
        let mut late_lead = String::new();
        let mut late_anchors: Vec<String> = Vec::new();
        for id in &self.order {
            if present.contains(id) {
                let lead = std::mem::take(&mut late_lead);
                if !lead.is_empty() {
                    orphan_lead.insert(*id, lead);
                }
                let list = std::mem::take(&mut late_anchors);
                if !list.is_empty() {
                    orphan_anchors.insert(*id, list);
                }
            } else if let Some(s) = self.stored.get(id) {
                late_lead.push_str(&s.lead);
                late_anchors.extend(s.anchors.iter().map(|a| a.1.clone()));
            }
        }

        let mut body = String::new();
        let mut order = Vec::new();
        let mut stored = HashMap::new();
        let mut used: HashSet<u32> = HashSet::new();
        let mut pending_anchors: Vec<String> = Vec::new();
        for block in blocks {
            let id = block_id(block).filter(|i| self.stored.contains_key(i) && used.insert(*i));
            let old = id.and_then(|i| self.stored.get(&i));
            let lead = old.map(|s| s.lead.clone()).unwrap_or_default();
            let extra_lead = id.and_then(|i| orphan_lead.get(&i)).cloned().unwrap_or_default();
            if let Some(i) = id {
                if let Some(list) = orphan_anchors.get(&i) {
                    pending_anchors.extend(list.iter().cloned());
                }
            }
            let lead = format!("{extra_lead}{lead}");
            body.push_str(&lead);
            let same = old.is_some_and(|s| s.is_locked || s.model.as_ref() == Some(&clean(block)));
            let (raw, entry) = if same && pending_anchors.is_empty() || (same && !matches!(block, Block::Para(_))) {
                let s = old.unwrap();
                (s.raw.clone(), Stored { lead: lead.clone(), ..s.clone() })
            } else {
                match block {
                    Block::Para(p) => {
                        let mut anchors: Vec<(usize, String)> = pending_anchors.drain(..).map(|raw| (0, raw)).collect();
                        if let Some(s) = old {
                            let before = if let Some(Block::Para(m)) = &s.model { para_text(m) } else { String::new() };
                            anchors.extend(remap(&before, &para_text(p), &s.anchors));
                        }
                        let attrs = old.map(|s| s.p_attrs.as_str()).unwrap_or("");
                        let raw = para_xml(p, attrs, &anchors, &ctx);
                        (
                            raw.clone(),
                            Stored {
                                raw,
                                lead: lead.clone(),
                                model: Some(clean(block)),
                                p_attrs: attrs.to_string(),
                                anchors,
                                is_locked: false,
                            },
                        )
                    }
                    Block::Table { rows, tblpr, grid, .. } => {
                        let raw = table_xml(rows, tblpr, grid, &ctx);
                        (
                            raw.clone(),
                            Stored {
                                raw,
                                lead: lead.clone(),
                                model: Some(clean(block)),
                                p_attrs: String::new(),
                                anchors: Vec::new(),
                                is_locked: false,
                            },
                        )
                    }
                    Block::Image { src, alt, width, height, .. } => {
                        let raw = parts.picture(src.as_deref().unwrap_or(""), alt, *width, *height, base_dir)?;
                        (
                            raw.clone(),
                            Stored {
                                raw,
                                lead: lead.clone(),
                                model: Some(clean(block)),
                                p_attrs: String::new(),
                                anchors: Vec::new(),
                                is_locked: false,
                            },
                        )
                    }
                    Block::Locked { .. } => return Err("unknown locked block".into()),
                }
            };
            body.push_str(&raw);
            if let Some(i) = id {
                order.push(i);
                stored.insert(i, entry);
            }
        }
        // nothing after to carry them: they get a paragraph of their own
        let leftover: String = pending_anchors.iter().chain(late_anchors.iter()).cloned().collect();
        if !late_lead.is_empty() {
            body.push_str(&late_lead);
        }
        if !leftover.is_empty() {
            body.push_str(&format!("<w:p>{leftover}</w:p>"));
        }
        // Word wants a paragraph after a table at the very end of the body
        if matches!(blocks.last(), Some(Block::Table { id, .. }) if id.is_none() || order.last().is_some_and(|l| Some(l) != self.order.last()))
        {
            body.push_str("<w:p/>");
        }
        let document = format!("{}{}{}", self.head, body, self.tail);
        let changed_doc = package::read_part(&self.bytes, &self.main).is_none_or(|b| b != document.as_bytes());
        if changed_doc {
            parts.set(&self.main, document.into_bytes());
        }
        let assigned: Vec<(u32, u32)> = new_lists.iter().map(|(k, v)| (*k, *v)).collect();
        let (bytes, changed) = parts.finish()?;
        let mut next = self.clone();
        next.bytes = bytes;
        next.order = order;
        next.stored = stored;
        next.refresh();
        Ok(Prepared { next, changed, assigned })
    }

    fn refresh(&mut self) {
        if let Some(s) = package::read_part(&self.bytes, "word/styles.xml").and_then(|b| String::from_utf8(b).ok()) {
            self.styles = super::read_styles(&s);
        }
        if let Some(s) = package::read_part(&self.bytes, "word/numbering.xml").and_then(|b| String::from_utf8(b).ok()) {
            self.numbering = super::read_numbering(&s);
        }
        if let Some(s) =
            package::read_part(&self.bytes, "word/_rels/document.xml.rels").and_then(|b| String::from_utf8(b).ok())
        {
            self.rels = super::read_rels(&s);
        }
    }
}

fn block_id(b: &Block) -> Option<u32> {
    match b {
        Block::Para(p) => p.id,
        Block::Table { id, .. } | Block::Image { id, .. } => *id,
        Block::Locked { id, .. } => Some(*id),
    }
}

/// The copy of a block kept to compare the next save against: as it is, with its id (a table's cells never have one).
fn clean(b: &Block) -> Block {
    b.clone()
}

// ---------- the other parts an edit can need ----------

struct Parts<'a> {
    docx: &'a Docx,
    changed: BTreeMap<String, Vec<u8>>,
    new_headings: Vec<u32>,
}

fn text_part(bytes: &[u8], name: &str) -> Option<String> {
    package::read_part(bytes, name).and_then(|b| String::from_utf8(b).ok())
}

fn insert_before(text: &str, closing: &str, what: &str) -> Option<String> {
    let at = text.rfind(closing)?;
    Some(format!("{}{}{}", &text[..at], what, &text[at..]))
}

impl<'a> Parts<'a> {
    fn new(docx: &'a Docx) -> Self {
        Parts { docx, changed: BTreeMap::new(), new_headings: Vec::new() }
    }

    fn current(&self, name: &str) -> Option<String> {
        match self.changed.get(name) {
            Some(b) => String::from_utf8(b.clone()).ok(),
            None => text_part(&self.docx.bytes, name),
        }
    }

    fn set(&mut self, name: &str, bytes: Vec<u8>) {
        self.changed.insert(name.to_string(), bytes);
    }

    fn content_type_override(&mut self, part: &str, kind: &str) -> Result<(), String> {
        let types = self.current("[Content_Types].xml").ok_or("no content types")?;
        if types.contains(&format!("PartName=\"/{part}\"")) {
            return Ok(());
        }
        let next =
            insert_before(&types, "</Types>", &format!("<Override PartName=\"/{part}\" ContentType=\"{kind}\"/>"))
                .ok_or("bad content types")?;
        self.set("[Content_Types].xml", next.into_bytes());
        Ok(())
    }

    fn content_type_default(&mut self, ext: &str, kind: &str) -> Result<(), String> {
        let types = self.current("[Content_Types].xml").ok_or("no content types")?;
        let lower = types.to_lowercase();
        if lower.contains(&format!("extension=\"{ext}\"")) {
            return Ok(());
        }
        let at = types
            .find("<Default")
            .or_else(|| types.find("<Override"))
            .or_else(|| types.rfind("</Types>"))
            .ok_or("bad content types")?;
        let next = format!("{}<Default Extension=\"{ext}\" ContentType=\"{kind}\"/>{}", &types[..at], &types[at..]);
        self.set("[Content_Types].xml", next.into_bytes());
        Ok(())
    }

    fn add_relationship(&mut self, kind: &str, target: &str) -> Result<String, String> {
        const RELS: &str = "word/_rels/document.xml.rels";
        let rels = self.current(RELS).unwrap_or_else(|| "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"></Relationships>".to_string());
        if let Some(at) = rels.find(&format!("Target=\"{target}\"")) {
            let head = &rels[..at];
            if let Some(tag) = head.rfind("<Relationship ") {
                if let Some(id) = xml::attr_in(&rels[tag + 13..at + target.len() + 9], "Id") {
                    return Ok(id);
                }
            }
        }
        let mut n = 1000;
        while rels.contains(&format!("\"rIdDA{n}\"")) {
            n += 1;
        }
        let id = format!("rIdDA{n}");
        let next = insert_before(
            &rels,
            "</Relationships>",
            &format!("<Relationship Id=\"{id}\" Type=\"{kind}\" Target=\"{target}\"/>"),
        )
        .ok_or("bad relationships")?;
        self.set(RELS, next.into_bytes());
        Ok(id)
    }

    fn need_headings(&mut self, blocks: &[Block]) {
        let mut wanted = HashSet::new();
        each_para(blocks, &mut |p| {
            if let Some(level) = p.style.as_deref().and_then(|s| s[1..].parse::<u32>().ok()) {
                wanted.insert(level);
            }
        });
        for level in wanted {
            if !self.docx.styles.id_of.contains_key(&level) {
                self.new_headings.push(level);
            }
        }
        self.new_headings.sort();
        if self.new_headings.is_empty() {
            return;
        }
        const SIZES: [u32; 3] = [48, 32, 26];
        let mut defs = String::new();
        for level in &self.new_headings {
            let sz = SIZES[(*level as usize - 1).min(2)];
            defs.push_str(&format!("<w:style w:type=\"paragraph\" w:styleId=\"Heading{level}\"><w:name w:val=\"heading {level}\"/><w:qFormat/><w:pPr><w:keepNext/><w:outlineLvl w:val=\"{}\"/></w:pPr><w:rPr><w:b/><w:sz w:val=\"{sz}\"/></w:rPr></w:style>", level - 1));
        }
        let styles = self.current("word/styles.xml").and_then(|s| insert_before(&s, "</w:styles>", &defs));
        let text = styles.unwrap_or_else(|| format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:styles xmlns:w=\"{WML}\">{defs}</w:styles>"));
        let existed = package::read_part(&self.docx.bytes, "word/styles.xml").is_some();
        self.set("word/styles.xml", text.into_bytes());
        if !existed {
            let _ = self.content_type_override(
                "word/styles.xml",
                "application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml",
            );
            let _ = self.add_relationship(
                "http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles",
                "styles.xml",
            );
        }
    }

    fn styles_after(&self) -> Styles {
        let mut s = self.docx.styles.clone();
        for level in &self.new_headings {
            s.id_of.insert(*level, format!("Heading{level}"));
        }
        s
    }

    fn allocate_lists(&mut self, blocks: &[Block]) -> Result<HashMap<u32, u32>, String> {
        let mut wanted: BTreeMap<u32, String> = BTreeMap::new();
        each_para(blocks, &mut |p| {
            if let Some(ListRef { new: Some(k), kind, level, .. }) = &p.list {
                if *level == 0 || !wanted.contains_key(k) {
                    wanted.entry(*k).or_insert_with(|| kind.clone());
                }
            }
        });
        if wanted.is_empty() {
            return Ok(HashMap::new());
        }
        let existed = package::read_part(&self.docx.bytes, "word/numbering.xml").is_some();
        let mut text = self.current("word/numbering.xml").unwrap_or_else(|| format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n<w:numbering xmlns:w=\"{WML}\"></w:numbering>"));
        let mut next_abs = self.docx.numbering.abstracts.iter().max().map_or(100, |m| m + 1).max(100);
        let mut next_num = self.docx.numbering.nums.iter().max().map_or(100, |m| m + 1).max(100);
        let mut out = HashMap::new();
        for (key, kind) in wanted {
            let (abs_id, num) = (next_abs, next_num);
            next_abs += 1;
            next_num += 1;
            let abs = abstract_list(abs_id, &kind);
            // abstract definitions must come before the numbers
            text = match text.find("<w:num ") {
                Some(at) => format!("{}{}{}", &text[..at], abs, &text[at..]),
                None => insert_before(&text, "</w:numbering>", &abs).ok_or("bad numbering")?,
            };
            let def = format!("<w:num w:numId=\"{num}\"><w:abstractNumId w:val=\"{abs_id}\"/></w:num>");
            // anything after the numbers (a clean-up marker) stays after
            text = match text.find("<w:numIdMacAtCleanup") {
                Some(at) => format!("{}{}{}", &text[..at], def, &text[at..]),
                None => insert_before(&text, "</w:numbering>", &def).ok_or("bad numbering")?,
            };
            out.insert(key, num);
        }
        self.set("word/numbering.xml", text.into_bytes());
        if !existed {
            self.content_type_override(
                "word/numbering.xml",
                "application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml",
            )?;
            self.add_relationship(
                "http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering",
                "numbering.xml",
            )?;
        }
        Ok(out)
    }

    /// A picture new to the file. It gets its part once, however many saves follow.
    fn picture(
        &mut self,
        src: &str,
        alt: &str,
        width: Option<u32>,
        height: Option<u32>,
        base_dir: Option<&Path>,
    ) -> Result<String, String> {
        let (bytes, ext) = picture_bytes(src, base_dir).ok_or("picture can't be read")?;
        let sum = bytes.iter().fold(0xcbf29ce484222325u64, |h, b| (h ^ *b as u64).wrapping_mul(0x100000001b3));
        let part = format!("media/dotany-{sum:016x}.{ext}");
        let full = format!("word/{part}");
        if package::read_part(&self.docx.bytes, &full).is_none() {
            self.set(&full, bytes.clone());
        }
        let mime = match ext.as_str() {
            "jpg" | "jpeg" => "image/jpeg",
            "gif" => "image/gif",
            _ => "image/png",
        };
        self.content_type_default(&ext, mime)?;
        let rel =
            self.add_relationship("http://schemas.openxmlformats.org/officeDocument/2006/relationships/image", &part)?;
        // 6.2 inches at most, which fits a page with normal margins
        let (w, h) = match (width, height) {
            (Some(w), Some(h)) if w > 0 && h > 0 => {
                let scale = (595.0 / w as f64).min(1.0);
                ((w as f64 * scale * 9525.0) as u64, (h as f64 * scale * 9525.0) as u64)
            }
            _ => (4_800_000, 3_000_000),
        };
        let pic_id = 100_000 + (sum % 800_000);
        let alt = escape_attr(alt);
        Ok(format!(
            "<w:p><w:r><w:drawing><wp:inline xmlns:wp=\"http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing\" distT=\"0\" distB=\"0\" distL=\"0\" distR=\"0\"><wp:extent cx=\"{w}\" cy=\"{h}\"/><wp:docPr id=\"{pic_id}\" name=\"Picture {pic_id}\" descr=\"{alt}\"/><a:graphic xmlns:a=\"http://schemas.openxmlformats.org/drawingml/2006/main\"><a:graphicData uri=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><pic:pic xmlns:pic=\"http://schemas.openxmlformats.org/drawingml/2006/picture\"><pic:nvPicPr><pic:cNvPr id=\"{pic_id}\" name=\"Picture {pic_id}\"/><pic:cNvPicPr/></pic:nvPicPr><pic:blipFill><a:blip xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\" r:embed=\"{rel}\"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill><pic:spPr><a:xfrm><a:off x=\"0\" y=\"0\"/><a:ext cx=\"{w}\" cy=\"{h}\"/></a:xfrm><a:prstGeom prst=\"rect\"><a:avLst/></a:prstGeom></pic:spPr></pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r></w:p>"
        ))
    }

    fn finish(self) -> Result<(Vec<u8>, bool), String> {
        if self.changed.is_empty() {
            return Ok((self.docx.bytes.clone(), false));
        }
        Ok((package::rewrite(&self.docx.bytes, &self.changed)?, true))
    }
}

fn each_para(blocks: &[Block], f: &mut dyn FnMut(&Para)) {
    for b in blocks {
        match b {
            Block::Para(p) => f(p),
            Block::Table { rows, .. } => {
                for r in rows {
                    for c in &r.cells {
                        for p in &c.paras {
                            f(p);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn abstract_list(id: u32, kind: &str) -> String {
    let mut levels = String::new();
    for l in 0..9u32 {
        let (fmt, text) = if kind == "bullet" {
            ("bullet".to_string(), ["•", "◦", "▪"][(l % 3) as usize].to_string())
        } else {
            (["decimal", "lowerLetter", "lowerRoman"][(l % 3) as usize].to_string(), format!("%{}.", l + 1))
        };
        levels.push_str(&format!("<w:lvl w:ilvl=\"{l}\"><w:start w:val=\"1\"/><w:numFmt w:val=\"{fmt}\"/><w:lvlText w:val=\"{text}\"/><w:lvlJc w:val=\"left\"/><w:pPr><w:ind w:left=\"{}\" w:hanging=\"360\"/></w:pPr></w:lvl>", 720 * (l + 1)));
    }
    format!(
        "<w:abstractNum w:abstractNumId=\"{id}\"><w:multiLevelType w:val=\"hybridMultilevel\"/>{levels}</w:abstractNum>"
    )
}

fn base64_decode(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut buf, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            b'=' | b'\n' | b'\r' => continue,
            _ => return None,
        };
        buf = (buf << 6) | v as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buf >> bits) as u8);
            buf &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// A picture pasted into the page: its bytes and its type, from a base64 data URL.
pub fn data_url(src: &str) -> Option<(Vec<u8>, String)> {
    let (head, data) = src.strip_prefix("data:")?.split_once(',')?;
    let mut kinds = head.split(';');
    let mime = kinds.next()?.to_ascii_lowercase();
    if !kinds.any(|k| k.eq_ignore_ascii_case("base64")) {
        return None;
    }
    Some((base64_decode(data)?, mime))
}

fn picture_bytes(src: &str, base_dir: Option<&Path>) -> Option<(Vec<u8>, String)> {
    if src.starts_with("data:") {
        let (bytes, mime) = data_url(src)?;
        let ext = match mime.as_str() {
            "image/jpeg" => "jpg",
            "image/gif" => "gif",
            "image/png" => "png",
            _ => return None,
        };
        return Some((bytes, ext.to_string()));
    }
    let path = Path::new(src);
    let full = if path.is_absolute() { path.to_path_buf() } else { base_dir?.join(path) };
    // a link to a picture beside the Markdown spells spaces and brackets as %20, %28… (Word to Markdown writes them so)
    let full = if full.exists() || !src.contains('%') {
        full
    } else {
        let plain = crate::beside::unlink(src);
        let path = Path::new(&plain);
        if path.is_absolute() { path.to_path_buf() } else { base_dir?.join(path) }
    };
    let ext = full.extension()?.to_string_lossy().to_lowercase();
    if !["png", "jpg", "jpeg", "gif"].contains(&ext.as_str()) {
        return None;
    }
    Some((std::fs::read(full).ok()?, ext))
}

// ---------- a new document ----------

pub fn blank_package() -> Result<Vec<u8>, String> {
    let head = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n";
    let types = format!(
        "{head}<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml\"/><Override PartName=\"/word/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml\"/></Types>"
    );
    let rels = format!(
        "{head}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"word/document.xml\"/></Relationships>"
    );
    let doc_rels = format!(
        "{head}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/></Relationships>"
    );
    let document = format!(
        "{head}<w:document xmlns:w=\"{WML}\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><w:body><w:p/><w:sectPr><w:pgSz w:w=\"11906\" w:h=\"16838\"/><w:pgMar w:top=\"1440\" w:right=\"1440\" w:bottom=\"1440\" w:left=\"1440\" w:header=\"708\" w:footer=\"708\" w:gutter=\"0\"/></w:sectPr></w:body></w:document>"
    );
    let heading = |level: u32, sz: u32| {
        format!(
            "<w:style w:type=\"paragraph\" w:styleId=\"Heading{level}\"><w:name w:val=\"heading {level}\"/><w:basedOn w:val=\"Normal\"/><w:next w:val=\"Normal\"/><w:qFormat/><w:pPr><w:keepNext/><w:spacing w:before=\"240\" w:after=\"120\"/><w:outlineLvl w:val=\"{}\"/></w:pPr><w:rPr><w:b/><w:sz w:val=\"{sz}\"/></w:rPr></w:style>",
            level - 1
        )
    };
    let styles = format!(
        "{head}<w:styles xmlns:w=\"{WML}\"><w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii=\"Calibri\" w:hAnsi=\"Calibri\" w:eastAsia=\"Microsoft YaHei\" w:cs=\"Calibri\"/><w:sz w:val=\"24\"/><w:szCs w:val=\"24\"/></w:rPr></w:rPrDefault><w:pPrDefault><w:pPr><w:spacing w:after=\"160\" w:line=\"276\" w:lineRule=\"auto\"/></w:pPr></w:pPrDefault></w:docDefaults><w:style w:type=\"paragraph\" w:default=\"1\" w:styleId=\"Normal\"><w:name w:val=\"Normal\"/><w:qFormat/></w:style>{}{}{}</w:styles>",
        heading(1, 48),
        heading(2, 32),
        heading(3, 26)
    );
    package::create(&[
        ("[Content_Types].xml", types.into_bytes()),
        ("_rels/.rels", rels.into_bytes()),
        ("word/document.xml", document.into_bytes()),
        ("word/_rels/document.xml.rels", doc_rels.into_bytes()),
        ("word/styles.xml", styles.into_bytes()),
    ])
}

#[allow(unused)]
fn _node_kinds(_: &El, _: &Node) {}
