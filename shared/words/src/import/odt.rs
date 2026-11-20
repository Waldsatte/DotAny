//! OpenDocument text (.odt): `content.xml` with its automatic styles, and the named styles in `styles.xml`.
//! Paragraphs and headings, spans (bold, italic, underline, size, color), alignment, spacing and line height, lists,
//! tables, sections and the words of an index, and PNG, JPEG or GIF pictures from the package. Notes, comments, links,
//! text boxes, objects, merged cells, nested tables and tracked changes count as missing.

use super::{Imported, Look, ParaLook, Sink};
use crate::docx::{self, xml};
use std::collections::HashMap;
use xml::{El, Node};

const MAX_DEPTH: usize = 200;
/// Repeated rows and columns as written by spreadsheets-turned-documents can be huge; past this they are cut.
const MAX_REPEAT: usize = 200;

#[derive(Clone, Default, Debug)]
struct Style {
    parent: Option<String>,
    b: Option<bool>,
    i: Option<bool>,
    u: Option<bool>,
    sz: Option<u32>,
    color: Option<String>,
    align: Option<&'static str>,
    line: Option<u32>,
    before: Option<i32>,
    after: Option<i32>,
    left: Option<i32>,
    right: Option<i32>,
    first: Option<i32>,
    outline: Option<u32>,
    list_style: Option<String>,
    hidden: Option<bool>,
}

struct Reader<'a> {
    src: &'a str,
    bytes: &'a [u8],
    styles: HashMap<String, Style>,
    defaults: Style,
    /// List style name to each level's kind.
    lists: HashMap<String, Vec<&'static str>>,
    sink: Sink,
    next_key: u32,
    last_list: HashMap<String, u32>,
    list_styles: Vec<String>,
}

pub fn read(bytes: &[u8]) -> Option<Imported> {
    let names = docx::part_names_of(bytes)?;
    // an encrypted package lists its parts but can't be read
    let manifest = part_text(bytes, "META-INF/manifest.xml").unwrap_or_default();
    if manifest.contains("encryption-data") {
        return None;
    }
    if let Some(m) = part_text(bytes, "mimetype") {
        if !m.trim().starts_with("application/vnd.oasis.opendocument.text") {
            return None;
        }
    }
    let content = part_text(bytes, "content.xml")?;
    let root = xml::parse(&content)?;
    let named =
        names.iter().any(|n| n == "styles.xml").then(|| part_text(bytes, "styles.xml")).flatten().unwrap_or_default();
    let mut r = Reader {
        src: &content,
        bytes,
        styles: HashMap::new(),
        defaults: Style::default(),
        lists: HashMap::new(),
        sink: Sink::default(),
        next_key: 1,
        last_list: HashMap::new(),
        list_styles: Vec::new(),
    };
    if let Some(sroot) = xml::parse(&named) {
        r.read_styles(&sroot, &named);
        // headers and footers live with the page styles; words in them won't be in a saved copy
        let pages = sroot.child("office:master-styles");
        if pages.is_some_and(|m| {
            m.els()
                .flat_map(|p| p.els())
                .any(|hf| hf.name.starts_with("style:header") || hf.name.starts_with("style:footer"))
                && has_words(m, &named, 0)
        }) {
            lack!(r.sink);
        }
    }
    r.read_styles(&root, &content);
    let body = root.child("office:body")?.child("office:text")?;
    if body.has_deep("text:tracked-changes") {
        lack!(r.sink);
    }
    r.blocks(body, 0, None, false);
    Some(r.sink.finish(&ParaLook::default()))
}

fn part_text(bytes: &[u8], name: &str) -> Option<String> {
    docx::read_part(bytes, name)
        .and_then(|b| String::from_utf8(b).ok())
        .map(|s| s.trim_start_matches('\u{feff}').to_string())
}

/// Lengths in twips: "1.5cm", "0.25in", "12pt", "10mm".
fn twips(v: &str) -> Option<i32> {
    let v = v.trim();
    let (num, unit) = v.split_at(v.find(|c: char| c.is_ascii_alphabetic() || c == '%')?);
    let n: f64 = num.trim().parse().ok()?;
    let t = match unit {
        "cm" => n * 1440.0 / 2.54,
        "mm" => n * 144.0 / 2.54,
        "in" | "inch" => n * 1440.0,
        "pt" => n * 20.0,
        "pc" => n * 240.0,
        "px" => n * 15.0,
        _ => return None,
    };
    t.is_finite().then_some(t.round().clamp(-100_000.0, 100_000.0) as i32)
}

impl Reader<'_> {
    fn read_styles(&mut self, root: &El, src: &str) {
        for group in root.els().filter(|e| matches!(e.name.as_str(), "office:styles" | "office:automatic-styles")) {
            for st in group.els() {
                match st.name.as_str() {
                    "style:style" => {
                        let Some(name) = st.attr("style:name") else { continue };
                        let s = self.style_of(st);
                        self.styles.insert(name, s);
                    }
                    "style:default-style" if st.attr("style:family").as_deref() == Some("paragraph") => {
                        self.defaults = self.style_of(st)
                    }
                    "text:list-style" => {
                        let Some(name) = st.attr("style:name") else { continue };
                        let mut levels = vec!["bullet"; 10];
                        for l in st.els() {
                            let level =
                                l.attr("text:level").and_then(|v| v.parse::<usize>().ok()).unwrap_or(1).clamp(1, 10)
                                    - 1;
                            levels[level] = match l.name.as_str() {
                                "text:list-level-style-number"
                                    if l.attr("style:num-format").is_some_and(|f| !f.is_empty()) =>
                                {
                                    "ordered"
                                }
                                "text:list-level-style-number" => "none",
                                "text:list-level-style-image" | "text:list-level-style-bullet" => "bullet",
                                _ => continue,
                            };
                        }
                        self.lists.insert(name, levels);
                    }
                    _ => {}
                }
            }
        }
        let _ = src;
    }

    fn style_of(&self, st: &El) -> Style {
        let mut s = Style {
            parent: st.attr("style:parent-style-name"),
            list_style: st.attr("style:list-style-name"),
            ..Default::default()
        };
        s.outline = st.attr("style:default-outline-level").and_then(|v| v.parse().ok()).filter(|v| *v > 0);
        if let Some(t) = st.child("style:text-properties") {
            s.b = t.attr("fo:font-weight").map(|w| w == "bold" || w.parse::<u32>().is_ok_and(|n| n >= 600));
            s.i = t.attr("fo:font-style").map(|v| v == "italic" || v == "oblique");
            s.u = t.attr("style:text-underline-style").map(|v| v != "none");
            s.sz = t.attr("fo:font-size").and_then(|v| twips(&v)).filter(|t| *t > 0).map(|t| (t / 10) as u32);
            s.hidden = t.attr("text:display").map(|d| d == "none");
            s.color = t
                .attr("fo:color")
                .and_then(|c| c.strip_prefix('#').map(|h| h.to_uppercase()))
                .filter(|h| h.len() == 6 && h.chars().all(|c| c.is_ascii_hexdigit()));
        }
        if let Some(p) = st.child("style:paragraph-properties") {
            s.align = p.attr("fo:text-align").and_then(|a| match a.as_str() {
                "center" => Some("center"),
                "end" | "right" => Some("right"),
                "justify" => Some("justify"),
                "start" | "left" => Some("left"),
                _ => None,
            });
            s.line = p
                .attr("fo:line-height")
                .and_then(|v| v.strip_suffix('%').and_then(|n| n.trim().parse::<f64>().ok()))
                .map(|pc| (pc * 2.4).round() as u32);
            s.before = p.attr("fo:margin-top").and_then(|v| twips(&v));
            s.after = p.attr("fo:margin-bottom").and_then(|v| twips(&v));
            s.left = p.attr("fo:margin-left").and_then(|v| twips(&v));
            s.right = p.attr("fo:margin-right").and_then(|v| twips(&v));
            s.first = p.attr("fo:text-indent").and_then(|v| twips(&v));
        }
        s
    }

    /// A style with everything it inherits, nearest first.
    fn resolved(&self, name: Option<&str>) -> Style {
        let mut chain = Vec::new();
        let mut at = name.map(str::to_string);
        while let Some(n) = at {
            if chain.len() > 32 {
                break;
            }
            let Some(s) = self.styles.get(&n) else { break };
            chain.push(s);
            at = s.parent.clone();
        }
        let mut out = self.defaults.clone();
        for s in chain.iter().rev() {
            macro_rules! take {
                ($($f:ident),*) => { $( if s.$f.is_some() { out.$f = s.$f.clone(); } )* };
            }
            take!(b, i, u, sz, color, align, line, before, after, left, right, first, outline, list_style, hidden);
        }
        out
    }

    fn look_of(s: &Style) -> Look {
        Look {
            b: s.b.unwrap_or(false),
            i: s.i.unwrap_or(false),
            u: s.u.unwrap_or(false),
            sz: s.sz,
            color: s.color.clone().filter(|c| c != "000000"),
        }
    }

    fn blocks(&mut self, parent: &El, depth: usize, list: Option<(&'static str, u32, u32)>, in_table: bool) {
        if depth > MAX_DEPTH {
            lack!(self.sink);
            return;
        }
        for el in parent.els() {
            match el.name.as_str() {
                "text:p" | "text:h" => self.paragraph(el, list, in_table),
                "text:list" => self.list(el, depth, list, in_table),
                "table:table" => {
                    if in_table {
                        lack!(self.sink);
                        self.blocks_of_table(el, depth);
                    } else {
                        self.table(el, depth);
                    }
                }
                "text:section"
                | "text:index-body"
                | "text:table-of-content"
                | "text:alphabetical-index"
                | "text:illustration-index"
                | "text:table-index"
                | "text:object-index"
                | "text:user-index"
                | "text:bibliography"
                | "text:soft-page-break" => self.blocks(el, depth + 1, list, in_table),
                "text:index-title" => self.blocks(el, depth + 1, list, in_table),
                "office:forms"
                | "text:sequence-decls"
                | "text:variable-decls"
                | "text:user-field-decls"
                | "text:dde-connection-decls"
                | "text:tracked-changes"
                | "table:table-of-content-source"
                | "text:table-of-content-source" => {}
                "draw:frame" | "draw:a" => self.frame(el, &ParaLook::default(), in_table),
                n if n.ends_with("-source") => {}
                _ => lack!(self.sink),
            }
        }
    }

    /// Nested tables can't be shown as tables: their words stand in the cell, one paragraph after another.
    fn blocks_of_table(&mut self, t: &El, depth: usize) {
        for row in rows_of(t) {
            for cell in row.els() {
                self.blocks(cell, depth + 1, None, true);
            }
        }
    }

    fn list(&mut self, el: &El, depth: usize, outer: Option<(&'static str, u32, u32)>, in_table: bool) {
        if depth > MAX_DEPTH {
            lack!(self.sink);
            return;
        }
        // a nested list without a style of its own follows the one around it
        let style = el.attr("text:style-name").or_else(|| outer.and(self.list_styles.last().cloned()));
        let level = outer.map(|o| o.1 + 1).unwrap_or(0);
        let key = match outer {
            Some(o) => o.2,
            None => {
                let continues = el.attr("text:continue-numbering").as_deref() == Some("true")
                    || el.attr("text:continue-list").is_some();
                let name = style.clone().unwrap_or_default();
                match self.last_list.get(&name) {
                    Some(k) if continues => *k,
                    _ => {
                        self.next_key += 1;
                        self.last_list.insert(name, self.next_key);
                        self.next_key
                    }
                }
            }
        };
        let k = style
            .as_ref()
            .and_then(|s| self.lists.get(s))
            .and_then(|l| l.get(level as usize).copied())
            .unwrap_or(outer.map(|o| o.0).unwrap_or("bullet"));
        self.list_styles.push(style.unwrap_or_default());
        for item in el.els() {
            match item.name.as_str() {
                "text:list-item" => {
                    let this = if k == "none" { None } else { Some((k, level, key)) };
                    // a list item's first paragraph carries the number; the rest are plain paragraphs of the item
                    let mut first = true;
                    for c in item.els() {
                        match c.name.as_str() {
                            "text:p" | "text:h" => {
                                self.paragraph(c, if first { this } else { None }, in_table);
                                first = false;
                            }
                            "text:list" => self.list(c, depth + 1, Some(this.unwrap_or((k, level, key))), in_table),
                            _ => self.blocks(&wrap(c), depth + 1, None, in_table),
                        }
                    }
                }
                "text:list-header" => self.blocks(item, depth + 1, None, in_table),
                _ => {}
            }
        }
        self.list_styles.pop();
    }

    fn paragraph(&mut self, p: &El, list: Option<(&'static str, u32, u32)>, in_table: bool) {
        let style = self.resolved(p.attr("text:style-name").as_deref());
        let mut look = ParaLook {
            align: style.align.filter(|a| *a != "left"),
            line: style.line,
            before: Some(style.before.unwrap_or(0)),
            after: Some(style.after.unwrap_or(0)),
            left: style.left,
            right: style.right,
            first: style.first,
            list,
            ..Default::default()
        };
        if p.name == "text:h" {
            let level = p.attr("text:outline-level").and_then(|v| v.parse::<u32>().ok()).or(style.outline).unwrap_or(1);
            look.heading = Some(level).filter(|l| (1..=3).contains(l));
        } else if let Some(o) = style.outline {
            look.heading = Some(o).filter(|l| (1..=3).contains(l));
        }
        let base = Self::look_of(&style);
        if style.hidden != Some(true) {
            self.inline(p, &base, &look, in_table, 0);
        }
        self.sink.end_para(&look, in_table);
    }

    fn inline(&mut self, el: &El, look: &Look, para: &ParaLook, in_table: bool, depth: usize) {
        if depth > MAX_DEPTH {
            lack!(self.sink);
            return;
        }
        for k in &el.kids {
            match k {
                Node::Text(a, b) => {
                    let t = xml::unescape(&self.src[*a..*b]);
                    // white space in ODF text collapses like HTML's
                    let t: String = collapse(&t);
                    self.sink.text(&t, look);
                }
                Node::El(e) => match e.name.as_str() {
                    "text:span" => {
                        let s = e.attr("text:style-name").map(|n| self.resolved(Some(&n)));
                        // hidden by its style: not shown, as in any other program
                        if s.as_ref().is_some_and(|s| s.hidden == Some(true)) {
                            continue;
                        }
                        let mut inner = look.clone();
                        if let Some(s) = s {
                            inner.b = s.b.unwrap_or(inner.b);
                            inner.i = s.i.unwrap_or(inner.i);
                            inner.u = s.u.unwrap_or(inner.u);
                            inner.sz = s.sz.or(inner.sz);
                            if s.color.is_some() {
                                inner.color = s.color.clone().filter(|c| c != "000000");
                            }
                        }
                        self.inline(e, &inner, para, in_table, depth + 1);
                    }
                    "text:s" => {
                        let n = e.attr("text:c").and_then(|v| v.parse::<usize>().ok()).unwrap_or(1).clamp(1, 1000);
                        self.sink.text(&" ".repeat(n), look);
                    }
                    "text:tab" => self.sink.text("\t", look),
                    "text:line-break" => self.sink.text("\n", look),
                    "text:a" => {
                        lack!(self.sink);
                        self.inline(e, look, para, in_table, depth + 1);
                    }
                    "text:note" | "office:annotation" | "draw:text-box" | "draw:object" | "draw:object-ole"
                    | "draw:custom-shape" | "draw:rect" | "draw:ellipse" | "draw:line" | "draw:g" | "draw:control"
                    | "text:ruby-text" => lack!(self.sink),
                    "office:annotation-end"
                    | "text:bookmark"
                    | "text:bookmark-start"
                    | "text:bookmark-end"
                    | "text:reference-mark"
                    | "text:reference-mark-start"
                    | "text:reference-mark-end"
                    | "text:soft-page-break"
                    | "text:change"
                    | "text:change-start"
                    | "text:change-end"
                    | "text:alphabetical-index-mark"
                    | "text:alphabetical-index-mark-start"
                    | "text:alphabetical-index-mark-end"
                    | "text:toc-mark"
                    | "text:toc-mark-start"
                    | "text:toc-mark-end"
                    | "text:user-index-mark"
                    | "text:user-index-mark-start"
                    | "text:user-index-mark-end" => {}
                    "draw:frame" | "draw:a" => self.frame(e, para, in_table),
                    // shapes, measures and 3D: nothing a document view can show
                    n if n.starts_with("draw:")
                        || n.starts_with("dr3d:")
                        || n.starts_with("form:")
                        || n.starts_with("svg:") =>
                    {
                        lack!(self.sink)
                    }
                    // fields, ruby base, meta and the like: the words they show
                    _ => self.inline(e, look, para, in_table, depth + 1),
                },
                Node::Other(..) => {}
            }
        }
    }

    fn frame(&mut self, f: &El, para: &ParaLook, in_table: bool) {
        if f.name == "draw:a" {
            lack!(self.sink);
            for c in f.els().filter(|c| c.name == "draw:frame") {
                self.frame(c, para, in_table);
            }
            return;
        }
        let Some(img) = f.child("draw:image") else {
            lack!(self.sink);
            return;
        };
        let href = img.attr("xlink:href").unwrap_or_default();
        let bytes = if href.is_empty() {
            img.child("office:binary-data").map(|b| b.text(self.src)).and_then(|t| decode64(&t))
        } else {
            docx::read_part(self.bytes, href.trim_start_matches("./"))
        };
        let Some(bytes) = bytes else {
            lack!(self.sink);
            return;
        };
        let px = |v: Option<String>| v.and_then(|v| twips(&v)).filter(|t| *t > 0).map(|t| (t / 15) as u32);
        self.sink.picture(&bytes, px(f.attr("svg:width")), px(f.attr("svg:height")), para, in_table);
    }

    fn table(&mut self, t: &El, depth: usize) {
        self.sink.close_table();
        for row in rows_of(t) {
            let repeat = row
                .attr("table:number-rows-repeated")
                .and_then(|v| v.parse::<usize>().ok())
                .unwrap_or(1)
                .clamp(1, MAX_REPEAT);
            for _ in 0..repeat {
                for cell in row.els() {
                    let span = |a: &str| cell.attr(a).and_then(|v| v.parse::<usize>().ok()).unwrap_or(1);
                    if cell.name == "table:covered-table-cell"
                        || span("table:number-columns-spanned") > 1
                        || span("table:number-rows-spanned") > 1
                    {
                        lack!(self.sink);
                    }
                    if cell.name != "table:table-cell" && cell.name != "table:covered-table-cell" {
                        continue;
                    }
                    let times = span("table:number-columns-repeated").clamp(1, MAX_REPEAT);
                    for _ in 0..times {
                        self.blocks(cell, depth + 1, None, true);
                        self.sink.end_cell(&ParaLook { after: Some(0), before: Some(0), ..Default::default() });
                    }
                }
                self.sink.end_row();
            }
        }
        self.sink.close_table();
    }
}

fn has_words(e: &El, src: &str, depth: usize) -> bool {
    depth < MAX_DEPTH
        && e.kids.iter().any(|k| match k {
            Node::Text(a, b) => xml::unescape(&src[*a..*b]).chars().any(char::is_alphanumeric),
            Node::El(c) => has_words(c, src, depth + 1),
            Node::Other(..) => false,
        })
}

fn rows_of(t: &El) -> Vec<&El> {
    let mut out = Vec::new();
    for e in t.els() {
        match e.name.as_str() {
            "table:table-row" => out.push(e),
            "table:table-header-rows" | "table:table-rows" | "table:table-row-group" => out.extend(rows_of(e)),
            _ => {}
        }
    }
    out
}

/// A lone element seen as a parent of itself, for walking it like a body.
fn wrap(e: &El) -> El {
    El {
        name: String::new(),
        attrs: String::new(),
        start: e.start,
        end: e.end,
        inner_start: e.start,
        inner_end: e.end,
        kids: vec![Node::El(e.clone())],
    }
}

fn collapse(t: &str) -> String {
    let mut out = String::with_capacity(t.len());
    let mut space = false;
    for c in t.chars() {
        if matches!(c, ' ' | '\t' | '\n' | '\r') {
            if !space {
                out.push(' ');
            }
            space = true;
        } else {
            out.push(c);
            space = false;
        }
    }
    out
}

fn decode64(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let (mut buf, mut bits) = (0u32, 0u32);
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' | b'\n' | b'\r' | b' ' | b'\t' => continue,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lengths() {
        assert_eq!(twips("2.54cm"), Some(1440));
        assert_eq!(twips("12pt"), Some(240));
        assert_eq!(twips("0.5in"), Some(720));
        assert_eq!(twips("bad"), None);
    }

    #[test]
    fn white_space_collapses() {
        assert_eq!(collapse("a  \n b"), "a b");
    }
}
