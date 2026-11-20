//! DotAny Text's article view, in Rust: `text/src/docx-view.js` (Word blocks to the document and back, plain words) and
//! `text/src/markdown.js` (Markdown to the document and back, plain text), ported line for line so the converter writes
//! what Text's Save As writes. The document is ProseMirror's in Text; here it is the same shape as plain data.
//!
//! Text's rule carries over: a Markdown file is only read as an article when every part of it is something the article
//! view can really show; anything else (code blocks, quotes, HTML, …) is not guessed at. `parse_markdown` says None then.

use super::docx::{Block, Cell, ListRef, Para, Row, Run};
use regex::Regex;
use std::sync::OnceLock;

// ---------- the document (text/src/schema.js) ----------

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Marks {
    pub strong: bool,
    pub em: bool,
    pub underline: bool,
    /// px, as the page shows it: half-points × 2/3
    pub size: Option<String>,
    pub color: Option<String>,
    pub rx: Option<usize>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Inline {
    Text(String, Marks),
    Break(Marks),
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Textblock {
    /// 0 for a paragraph, 1–3 for a heading
    pub level: u32,
    pub align: Option<String>,
    pub lh: Option<u32>,
    pub pid: Option<u32>,
    pub ppr: String,
    pub kids: Vec<Inline>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct TCell {
    pub head: bool,
    pub tcpr: String,
    pub kids: Vec<Node>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Node {
    Text(Textblock),
    Bullet { marker: String, loose: bool, num: Option<u32>, items: Vec<Vec<Node>> },
    Ordered { start: u64, loose: bool, num: Option<u32>, items: Vec<Vec<Node>> },
    Image { src: String, alt: String, iid: Option<u32>, rel: Option<String> },
    Table { tid: Option<u32>, tblpr: String, grid: String, rows: Vec<(String, Vec<TCell>)> },
    Locked { lid: u32, kind: String, text: String, rows: Option<Vec<Vec<String>>> },
}

fn para(kids: Vec<Inline>) -> Node {
    Node::Text(Textblock { kids: join_texts(kids), ..Default::default() })
}

/// ProseMirror joins neighbouring text with the same marks into one node.
fn join_texts(kids: Vec<Inline>) -> Vec<Inline> {
    let mut out: Vec<Inline> = Vec::new();
    for k in kids {
        if let (Some(Inline::Text(t, m)), Inline::Text(t2, m2)) = (out.last_mut(), &k) {
            if m == m2 {
                t.push_str(t2);
                continue;
            }
        }
        if let Inline::Text(t, _) = &k {
            if t.is_empty() {
                continue;
            }
        }
        out.push(k);
    }
    out
}

/// What a node reads as, as ProseMirror's textContent: a line break is its leafText, a new line; a locked block its label.
fn text_content(n: &Node) -> String {
    match n {
        Node::Text(t) => t.kids.iter().map(|k| if let Inline::Text(s, _) = k { s.as_str() } else { "\n" }).collect(),
        Node::Bullet { items, .. } | Node::Ordered { items, .. } => items.iter().flatten().map(text_content).collect(),
        Node::Image { .. } => String::new(),
        Node::Table { rows, .. } => {
            rows.iter().flat_map(|r| r.1.iter()).flat_map(|c| c.kids.iter()).map(text_content).collect()
        }
        Node::Locked { text, .. } => text.clone(),
    }
}

// ---------- docx-view.js: Word blocks to the document ----------

const COLOR_HEX: [(&str, &str); 4] = [("gray", "75716B"), ("red", "B3261E"), ("blue", "1F4FA8"), ("green", "2C6E49")];

fn js_number(v: f64) -> String {
    if v.fract() == 0.0 { format!("{}", v as i64) } else { format!("{v}") }
}

fn inline(runs: &[Run]) -> Vec<Inline> {
    let mut out = Vec::new();
    for run in runs {
        let marks = Marks {
            strong: run.b,
            em: run.i,
            underline: run.u,
            size: run.sz.map(|sz| js_number(sz as f64 * 2.0 / 3.0)),
            color: run.color.as_ref().map(|c| {
                COLOR_HEX.iter().find(|(_, h)| h == c).map(|(n, _)| n.to_string()).unwrap_or_else(|| format!("#{c}"))
            }),
            rx: run.extra,
        };
        let pieces: Vec<&str> = run.t.split('\n').collect();
        for (i, piece) in pieces.iter().enumerate() {
            if !piece.is_empty() {
                out.push(Inline::Text(piece.to_string(), marks.clone()));
            }
            if i < pieces.len() - 1 {
                out.push(Inline::Break(marks.clone()));
            }
        }
    }
    join_texts(out)
}

fn textblock(p: &Para) -> Node {
    let level = p.style.as_ref().and_then(|s| s.get(1..2)).and_then(|d| d.parse().ok()).unwrap_or(0);
    Node::Text(Textblock {
        level,
        align: p.align.clone(),
        lh: p.line,
        pid: p.id,
        ppr: p.ppr.clone(),
        kids: inline(&p.runs),
    })
}

fn nest(paras: &[&Para], from: usize, level: u32) -> (Vec<Node>, usize) {
    let mut lists = Vec::new();
    let mut i = from;
    while i < paras.len() && paras[i].list.as_ref().is_some_and(|l| l.level >= level) {
        let head = paras[i].list.clone().unwrap();
        let mut items = Vec::new();
        while i < paras.len() {
            let Some(l) = &paras[i].list else { break };
            if !(l.level >= level && (l.level > level || (l.kind == head.kind && l.num == head.num))) {
                break;
            }
            let mut kids = vec![textblock(paras[i])];
            i += 1;
            if i < paras.len() && paras[i].list.as_ref().is_some_and(|l| l.level > level) {
                let (inner, next) = nest(paras, i, level + 1);
                kids.extend(inner);
                i = next;
            }
            items.push(kids);
        }
        lists.push(if head.kind == "bullet" {
            Node::Bullet { marker: "-".into(), loose: false, num: head.num, items }
        } else {
            Node::Ordered { start: 1, loose: false, num: head.num, items }
        });
    }
    (lists, i)
}

/// What a locked block shows: its words, or what it is, in the language of the interface that asked (`lang`, see
/// `locked::label`).
fn locked_label(text: &str, kind: &str, rows: &[Vec<String>], lang: &str) -> String {
    if !text.is_empty() {
        text.to_string()
    } else if kind == "picture" {
        super::locked::label(lang, true).into()
    } else if !rows.is_empty() {
        String::new()
    } else {
        super::locked::label(lang, false).into()
    }
}

pub fn blocks_to_doc(blocks: &[Block], lang: &str) -> Vec<Node> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < blocks.len() {
        if let Block::Para(p) = &blocks[i] {
            if p.list.is_some() {
                let mut run = Vec::new();
                while let Some(Block::Para(q)) = blocks.get(i) {
                    if q.list.is_none() {
                        break;
                    }
                    run.push(q);
                    i += 1;
                }
                out.extend(nest(&run, 0, 0).0);
                continue;
            }
        }
        match &blocks[i] {
            Block::Para(p) => out.push(textblock(p)),
            Block::Image { id, rel, src, alt, .. } => out.push(Node::Image {
                src: src.clone().unwrap_or_else(|| format!("docx:{}", rel.clone().unwrap_or_default())),
                alt: alt.clone(),
                iid: *id,
                rel: rel.clone(),
            }),
            Block::Locked { id, kind, text, rows, .. } => out.push(Node::Locked {
                lid: *id,
                kind: kind.clone(),
                text: locked_label(text, kind, rows, lang),
                rows: if rows.is_empty() { None } else { Some(rows.clone()) },
            }),
            Block::Table { id, rows, tblpr, grid } => out.push(Node::Table {
                tid: *id,
                tblpr: tblpr.clone(),
                grid: grid.clone(),
                rows: rows
                    .iter()
                    .map(|r| {
                        (
                            r.trpr.clone(),
                            r.cells
                                .iter()
                                .map(|c| TCell {
                                    head: c.head,
                                    tcpr: c.tcpr.clone(),
                                    kids: c.paras.iter().map(textblock).collect(),
                                })
                                .collect(),
                        )
                    })
                    .collect(),
            }),
        }
        i += 1;
    }
    if out.is_empty() {
        out.push(para(Vec::new()));
    }
    out
}

// ---------- docx-view.js: the document to Word blocks ----------

fn runs_of(t: &Textblock) -> Vec<Run> {
    let mut runs: Vec<Run> = Vec::new();
    let mut add = |text: &str, m: &Marks| {
        let run = Run {
            t: text.to_string(),
            b: m.strong,
            i: m.em,
            u: m.underline,
            sz: m.size.as_ref().and_then(|px| px.parse::<f64>().ok()).map(|px| (px * 1.5).round() as u32),
            color: m.color.as_ref().map(|c| {
                COLOR_HEX
                    .iter()
                    .find(|(n, _)| n == c)
                    .map(|(_, h)| h.to_string())
                    .unwrap_or_else(|| c.trim_start_matches('#').to_string())
            }),
            extra: m.rx,
        };
        if let Some(last) = runs.last_mut() {
            if last.b == run.b
                && last.i == run.i
                && last.u == run.u
                && last.sz == run.sz
                && last.color == run.color
                && last.extra == run.extra
            {
                last.t.push_str(&run.t);
                return;
            }
        }
        runs.push(run);
    };
    for k in &t.kids {
        match k {
            Inline::Text(s, m) => add(s, m),
            Inline::Break(m) => add("\n", m),
        }
    }
    runs
}

fn para_of(t: &Textblock, list: Option<ListRef>) -> Para {
    Para {
        id: t.pid,
        style: (t.level > 0).then(|| format!("h{}", t.level)),
        align: t.align.clone(),
        line: t.lh,
        list,
        runs: runs_of(t),
        ppr: t.ppr.clone(),
    }
}

fn flat_cell(kids: &[Node]) -> Vec<Para> {
    let mut out = Vec::new();
    fn walk(n: &Node, out: &mut Vec<Para>) {
        match n {
            Node::Text(t) => out.push(para_of(t, None)),
            Node::Bullet { items, .. } | Node::Ordered { items, .. } => {
                items.iter().flatten().for_each(|k| walk(k, out))
            }
            Node::Table { rows, .. } => {
                rows.iter().flat_map(|r| r.1.iter()).flat_map(|c| c.kids.iter()).for_each(|k| walk(k, out))
            }
            _ => {}
        }
    }
    kids.iter().for_each(|k| walk(k, &mut out));
    if out.is_empty() {
        out.push(Para::default());
    }
    out
}

pub fn doc_to_blocks(doc: &[Node]) -> Vec<Block> {
    let mut out = Vec::new();
    let mut next_key = 1u32;
    fn block(n: &Node, out: &mut Vec<Block>, next_key: &mut u32) {
        match n {
            Node::Text(t) => out.push(Block::Para(para_of(t, None))),
            Node::Bullet { .. } | Node::Ordered { .. } => walk_list(n, 0, out, next_key),
            Node::Image { src, alt, iid, rel } => out.push(Block::Image {
                id: *iid,
                rel: rel.clone(),
                src: if rel.is_some() { None } else { Some(src.clone()) },
                alt: alt.clone(),
                width: None,
                height: None,
            }),
            Node::Locked { lid, kind, .. } => out.push(Block::Locked {
                id: *lid,
                kind: kind.clone(),
                text: String::new(),
                style: None,
                rows: Vec::new(),
            }),
            Node::Table { tid, tblpr, grid, rows } => {
                out.push(Block::Table {
                    id: *tid,
                    tblpr: tblpr.clone(),
                    grid: grid.clone(),
                    rows: rows
                        .iter()
                        .map(|(trpr, cells)| Row {
                            trpr: trpr.clone(),
                            cells: cells
                                .iter()
                                .map(|c| Cell { paras: flat_cell(&c.kids), tcpr: c.tcpr.clone(), head: c.head })
                                .collect(),
                        })
                        .collect(),
                });
            }
        }
    }
    fn walk_list(n: &Node, level: u32, out: &mut Vec<Block>, next_key: &mut u32) {
        let (kind, num, items) = match n {
            Node::Bullet { num, items, .. } => ("bullet", *num, items),
            Node::Ordered { num, items, .. } => ("ordered", *num, items),
            _ => return,
        };
        let r = ListRef {
            kind: kind.into(),
            level,
            num,
            new: if num.is_none() {
                let k = *next_key;
                *next_key += 1;
                Some(k)
            } else {
                None
            },
        };
        for item in items {
            let mut first = true;
            for kid in item {
                match kid {
                    Node::Text(t) if first => {
                        out.push(Block::Para(para_of(t, Some(r.clone()))));
                        first = false;
                    }
                    Node::Bullet { .. } | Node::Ordered { .. } => walk_list(kid, level + 1, out, next_key),
                    other => block(other, out, next_key),
                }
            }
        }
    }
    for n in doc {
        block(n, &mut out, &mut next_key);
    }
    out
}

/// docToPlain: one line per paragraph, table cells separated by tabs.
pub fn doc_to_plain(doc: &[Node]) -> String {
    let mut lines = Vec::new();
    fn walk(n: &Node, lines: &mut Vec<String>) {
        match n {
            Node::Text(t) => lines.push(
                t.kids
                    .iter()
                    .map(|k| match k {
                        Inline::Text(s, _) => s.clone(),
                        Inline::Break(_) => "\n".into(),
                    })
                    .collect(),
            ),
            Node::Table { rows, .. } => {
                for (_, cells) in rows {
                    lines.push(
                        cells
                            .iter()
                            .map(|c| c.kids.iter().map(text_content).collect::<String>())
                            .collect::<Vec<_>>()
                            .join("\t"),
                    );
                }
            }
            Node::Locked { text, rows, .. } => match rows {
                Some(rs) => rs.iter().for_each(|r| lines.push(r.join("\t"))),
                None => lines.push(text.clone()),
            },
            Node::Image { .. } => {}
            Node::Bullet { items, .. } | Node::Ordered { items, .. } => {
                items.iter().flatten().for_each(|k| walk(k, lines))
            }
        }
    }
    doc.iter().for_each(|n| walk(n, &mut lines));
    lines.join("\n") + "\n"
}

// ---------- markdown.js: plain text ----------

pub fn parse_text(text: &str) -> Vec<Node> {
    text.split('\n')
        .map(|line| para(if line.is_empty() { Vec::new() } else { vec![Inline::Text(line.into(), Marks::default())] }))
        .collect()
}

pub fn serialize_text(doc: &[Node]) -> String {
    doc.iter().map(text_content).collect::<Vec<_>>().join("\n")
}

// ---------- markdown.js: one line of text ----------

fn re(cell: &'static OnceLock<Regex>, src: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(src).unwrap())
}

static LINK: OnceLock<Regex> = OnceLock::new();
static BARE_URL: OnceLock<Regex> = OnceLock::new();

/// The length in characters of a link or bare URL starting at `i`, or 0.
fn token_at(s: &[char], i: usize) -> usize {
    let c = s.get(i);
    let r = match c {
        Some('[') => re(
            &LINK,
            r#"^\[(?:[^\[\]\\\n]|\\.)*\]\((?:<[^<>\n]*>|(?:[^\s()\\]|\\.|\((?:[^\s()\\]|\\.)*\))*)(?:[ \t]+(?:"(?:[^"\\\n]|\\.)*"|'(?:[^'\\\n]|\\.)*'))?\)"#,
        ),
        Some('h') => re(&BARE_URL, r"^https?://[^\s<>\[\]]+"),
        _ => return 0,
    };
    let rest: String = s[i..].iter().collect();
    r.find(&rest).map(|m| rest[..m.end()].chars().count()).unwrap_or(0)
}

fn link_ranges(s: &[char]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < s.len() {
        if s[i] == '\\' {
            i += 2;
            continue;
        }
        let n = token_at(s, i);
        if n > 0 {
            out.push((i, i + n));
            i += n;
        } else {
            i += 1;
        }
    }
    out
}

const HOLE: char = '\u{E000}';

fn without_links(s: &[char]) -> Vec<char> {
    let mut out = Vec::new();
    let mut at = 0;
    for (a, b) in link_ranges(s) {
        out.extend_from_slice(&s[at..a]);
        out.push(HOLE);
        at = b;
    }
    out.extend_from_slice(&s[at..]);
    out
}

fn is_punct(c: char) -> bool {
    matches!(c, '!'..='/' | ':'..='@' | '['..='`' | '{'..='~')
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric()
}

struct Foreign;
type Res<T> = Result<T, Foreign>;

/// s.replace(/\\./g, '')
fn unescaped(s: &[char]) -> String {
    let mut out = String::new();
    let mut i = 0;
    while i < s.len() {
        if s[i] == '\\' && i + 1 < s.len() {
            i += 2;
            continue;
        }
        out.push(s[i]);
        i += 1;
    }
    out
}

static REF_LINK: OnceLock<Regex> = OnceLock::new();
static HTML: OnceLock<Regex> = OnceLock::new();
static ENTITY: OnceLock<Regex> = OnceLock::new();

fn check_inline(raw: &[char]) -> Res<()> {
    let s = without_links(raw);
    let u = unescaped(&s);
    if u.contains(&format!("!{HOLE}")) || u.contains('`') {
        return Err(Foreign);
    }
    if re(&REF_LINK, r"!\[|\[[^\]]*\]\s*[(\[]|\[\^").is_match(&u)
        || re(&HTML, r"<[a-zA-Z/!?]").is_match(&u)
        || re(&ENTITY, r"&(#\d+|#[xX][0-9a-fA-F]+|[a-zA-Z][a-zA-Z0-9]*);").is_match(&u)
        || u.contains("~~")
    {
        return Err(Foreign);
    }
    let st: String = s.iter().collect();
    if st.ends_with('\\') && !st.ends_with("\\\\") {
        return Err(Foreign);
    }
    // s.replace(/\\./g, 'x')
    let mut plain = Vec::new();
    let mut i = 0;
    while i < s.len() {
        if s[i] == '\\' && i + 1 < s.len() {
            plain.push('x');
            i += 2;
            continue;
        }
        plain.push(s[i]);
        i += 1;
    }
    let mut i = 0;
    while i < plain.len() {
        if plain[i] != '_' {
            i += 1;
            continue;
        }
        let mut a = i;
        while a > 0 && plain[a - 1] == '_' {
            a -= 1;
        }
        let mut b = i;
        while b + 1 < plain.len() && plain[b + 1] == '_' {
            b += 1;
        }
        let before = if a > 0 { Some(plain[a - 1]) } else { None };
        let after = plain.get(b + 1).copied();
        // inside a word it is just a character (snake_case); at a word's edge it would be emphasis
        let inside = before.is_some_and(is_word) && after.is_some_and(is_word);
        if !inside && (after.is_some_and(|c| !c.is_whitespace()) || before.is_some_and(|c| !c.is_whitespace())) {
            return Err(Foreign);
        }
        i = b + 1;
    }
    Ok(())
}

/// [text, strong, em] pieces, with escapes resolved.
fn runs_of_md(s: &[char]) -> Vec<(String, bool, bool)> {
    let mut out: Vec<(String, bool, bool)> = Vec::new();
    fn push(out: &mut Vec<(String, bool, bool)>, t: &str, strong: bool, em: bool) {
        if t.is_empty() {
            return;
        }
        if let Some(l) = out.last_mut() {
            if l.1 == strong && l.2 == em {
                l.0.push_str(t);
                return;
            }
        }
        out.push((t.to_string(), strong, em));
    }
    fn walk(s: &[char], strong: bool, em: bool, out: &mut Vec<(String, bool, bool)>) {
        let mut i = 0;
        let mut text = String::new();
        while i < s.len() {
            let c = s[i];
            if c == '\\' && i + 1 < s.len() && is_punct(s[i + 1]) {
                text.push(s[i + 1]);
                i += 2;
                continue;
            }
            let link = token_at(s, i);
            if link > 0 {
                text.extend(&s[i..i + link]);
                i += link;
                continue;
            }
            if c != '*' {
                text.push(c);
                i += 1;
                continue;
            }
            let mut n = 1;
            while s.get(i + n) == Some(&'*') {
                n += 1;
            }
            let next = s.get(i + n);
            let close = if next.is_some_and(|c| !c.is_whitespace()) { find_close(s, i + n, n.min(3)) } else { None };
            let Some(at) = close else {
                text.extend(&s[i..i + n]);
                i += n;
                continue;
            };
            push(out, &text, strong, em);
            text.clear();
            let inner = &s[i + n..at];
            let k = n.min(3);
            let lead: String = s[i..i + n - k].iter().collect();
            push(out, &lead, strong, em);
            if k == 3 {
                walk(inner, true, true, out);
            } else if k == 2 {
                walk(inner, true, em, out);
            } else {
                walk(inner, strong, true, out);
            }
            i = at + k;
        }
        push(out, &text, strong, em);
    }
    walk(s, false, false, &mut out);
    out
}

fn find_close(s: &[char], from: usize, k: usize) -> Option<usize> {
    let mut i = from;
    while i < s.len() {
        if s[i] == '\\' {
            i += 2;
            continue;
        }
        let link = token_at(s, i);
        if link > 0 {
            i += link;
            continue;
        }
        if s[i] != '*' {
            i += 1;
            continue;
        }
        let mut n = 1;
        while s.get(i + n) == Some(&'*') {
            n += 1;
        }
        let closes = i > 0 && !s[i - 1].is_whitespace();
        if closes && n == k {
            return Some(i);
        }
        if closes && n > k && n == 3 {
            return Some(i + n - k);
        }
        i += n;
    }
    None
}

fn inline_nodes(s: &str) -> Res<Vec<Inline>> {
    let chars: Vec<char> = s.chars().collect();
    check_inline(&chars)?;
    let nodes: Vec<Inline> = join_texts(
        runs_of_md(&chars)
            .into_iter()
            .map(|(t, strong, em)| Inline::Text(t, Marks { strong, em, ..Default::default() }))
            .collect(),
    );
    // stars have many tricky readings; only the plain, unambiguous way of writing them is trusted
    if s.contains('*') && inline_text(&nodes, false) != s {
        return Err(Foreign);
    }
    Ok(nodes)
}

static LINK_LIKE: OnceLock<Regex> = OnceLock::new();

fn escape_text(t: &str, cell: bool) -> String {
    let t: Vec<char> = t.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < t.len() {
        let c = t[i];
        let prev = if i > 0 { Some(t[i - 1]) } else { None };
        let next = t.get(i + 1).copied();
        let link = if c == '\\' { 0 } else { token_at(&t, i) };
        if link > 0 {
            out.extend(&t[i..i + link]);
            i += link;
            continue;
        }
        let rest = || t[i..].iter().collect::<String>();
        match c {
            '\\' => out.push_str(if next.is_none_or(is_punct) { "\\\\" } else { "\\" }),
            '*' => out.push_str(if prev.is_some_and(char::is_whitespace) && next.is_some_and(char::is_whitespace) {
                "*"
            } else {
                "\\*"
            }),
            '`' => out.push_str("\\`"),
            '[' => out.push_str(if re(&LINK_LIKE, r"^\[[^\]]*\]\s*[(\[:]").is_match(&rest()) || next == Some('^') {
                "\\["
            } else {
                "["
            }),
            '!' if next == Some('[') => out.push_str("\\!"),
            '<' => out.push_str(if next.is_some_and(|n| n.is_ascii_alphabetic() || "/!?".contains(n)) {
                "\\<"
            } else {
                "<"
            }),
            '&' => out.push_str(
                if re(&ENTITY, r"&(#\d+|#[xX][0-9a-fA-F]+|[a-zA-Z][a-zA-Z0-9]*);")
                    .find(&rest())
                    .is_some_and(|m| m.start() == 0)
                {
                    "\\&"
                } else {
                    "&"
                },
            ),
            '~' if next == Some('~') || prev == Some('~') => out.push_str("\\~"),
            '_' => out.push_str(if prev.is_some_and(is_word) && next.is_some_and(is_word) {
                "_"
            } else if prev.is_some_and(|p| !p.is_whitespace()) || next.is_some_and(|n| !n.is_whitespace()) {
                "\\_"
            } else {
                "_"
            }),
            '|' if cell => out.push_str("\\|"),
            c => out.push(c),
        }
        i += 1;
    }
    out
}

static START_HASH: OnceLock<Regex> = OnceLock::new();
static START_DASH: OnceLock<Regex> = OnceLock::new();
static START_NUM: OnceLock<Regex> = OnceLock::new();
static START_RULE: OnceLock<Regex> = OnceLock::new();

fn text_start(s: &str) -> String {
    // JS's lookahead (?=\s|$) becomes a check on what follows the match
    let follows_space = |s: &str, end: usize| s[end..].chars().next().is_none_or(char::is_whitespace);
    let mut s = s.to_string();
    if let Some(m) = re(&START_HASH, r"^#{1,6}").find(&s) {
        if follows_space(&s, m.end()) {
            s = format!("\\{s}");
        }
    }
    if let Some(m) = re(&START_DASH, r"^[-+]").find(&s) {
        if follows_space(&s, m.end()) {
            s = format!("\\{s}");
        }
    }
    if let Some(c) = re(&START_NUM, r"^(\d{1,9})([.)])").captures(&s) {
        let end = c.get(0).unwrap().end();
        if follows_space(&s, end) {
            s = format!("{}\\{}{}", &c[1], &c[2], &s[end..]);
        }
    }
    if s.starts_with('>') {
        s = format!("\\{s}");
    }
    if let Some(c) = re(&START_RULE, r"^(-{3,}|=+)\s*$").captures(&s) {
        s = format!("\\{}", &s[c.get(1).unwrap().start()..]);
    }
    s
}

fn inline_text(kids: &[Inline], cell: bool) -> String {
    let mut open: Vec<&'static str> = Vec::new();
    let mut out = String::new();
    let delims = |k: Option<&Inline>| -> Vec<&'static str> {
        let m = match k {
            Some(Inline::Text(_, m)) | Some(Inline::Break(m)) => m,
            None => return Vec::new(),
        };
        let mut d = Vec::new();
        if m.strong {
            d.push("**");
        }
        if m.em {
            d.push("*");
        }
        d
    };
    fn close_to(open: &mut Vec<&'static str>, out: &mut String, keep: usize) {
        while open.len() > keep {
            out.push_str(open.pop().unwrap());
        }
    }
    let common = |open: &[&str], want: &[&str]| {
        let mut n = 0;
        while n < open.len() && want.contains(&open[n]) {
            n += 1;
        }
        n
    };
    for (i, child) in kids.iter().enumerate() {
        let want = delims(Some(child));
        let text = if let Inline::Text(t, _) = child { t.as_str() } else { "" };
        // /^(\s*)(.*?)(\s*)$/s
        let lead_end = text.len() - text.trim_start().len();
        let core = text.trim();
        let trail_start = if core.is_empty() { text.len() } else { lead_end + core.len() };
        let (lead, mid, trail) = (&text[..lead_end.min(trail_start)], core, &text[trail_start..]);
        let c = common(&open, &want);
        close_to(&mut open, &mut out, c);
        out.push_str(lead);
        if !mid.is_empty() {
            for d in want.iter().filter(|d| !open.contains(d)).copied().collect::<Vec<_>>() {
                out.push_str(d);
                open.push(d);
            }
            out.push_str(&escape_text(mid, cell));
        }
        if !trail.is_empty() {
            let next = delims(kids.get(i + 1));
            let c = common(&open, &next);
            close_to(&mut open, &mut out, c);
            out.push_str(trail);
        }
    }
    close_to(&mut open, &mut out, 0);
    out
}

fn line_of(kids: &[Inline]) -> String {
    text_start(inline_text(kids, false).trim())
}

// ---------- markdown.js: blocks to Markdown ----------

fn cell_text(c: &TCell) -> String {
    c.kids
        .iter()
        .map(|b| {
            if let Node::Text(t) = b {
                inline_text(&t.kids, true).trim().to_string()
            } else {
                text_content(b).replace('|', "\\|")
            }
        })
        .collect::<Vec<_>>()
        .join("<br>")
}

static TASK: OnceLock<Regex> = OnceLock::new();

fn block_text(node: &Node, indent: &str) -> String {
    match node {
        Node::Text(t) if t.level == 0 => line_of(&t.kids),
        Node::Text(t) => format!("{} {}", "#".repeat(t.level as usize), inline_text(&t.kids, false).trim()),
        Node::Image { src, alt, .. } => format!("![{}]({src})", escape_text(alt, false)),
        Node::Bullet { .. } | Node::Ordered { .. } => {
            let (ordered, start, marker, loose, items) = match node {
                Node::Bullet { marker, loose, items, .. } => (false, 0, marker.clone(), *loose, items),
                Node::Ordered { start, loose, items, .. } => (true, *start, String::new(), *loose, items),
                _ => unreachable!(),
            };
            let mut lines = Vec::new();
            for (i, item) in items.iter().enumerate() {
                let mark = if ordered { format!("{}. ", start + i as u64) } else { format!("{marker} ") };
                let pad = format!("{indent}{}", " ".repeat(mark.chars().count()));
                let mut first = true;
                let mut body = Vec::new();
                for b in item {
                    let text = block_text(b, &pad);
                    if first {
                        let fixed = re(&TASK, r"^\[([ xX])\]").replace(&text, "\\[$1]").into_owned();
                        body.push(format!("{indent}{mark}{fixed}"));
                        first = false;
                    } else if matches!(b, Node::Bullet { .. } | Node::Ordered { .. }) {
                        body.push(text);
                    } else {
                        body.push(text.split('\n').map(|l| format!("{pad}{l}")).collect::<Vec<_>>().join("\n"));
                    }
                }
                lines.push(body.join("\n"));
            }
            lines.join(if loose { "\n\n" } else { "\n" })
        }
        Node::Table { rows, .. } => {
            let rows: Vec<Vec<String>> = rows.iter().map(|(_, cells)| cells.iter().map(cell_text).collect()).collect();
            let line = |cells: &[String]| format!("| {} |", cells.join(" | "));
            let Some(head) = rows.first() else { return String::new() };
            let mut out = vec![line(head), line(&head.iter().map(|_| "---".to_string()).collect::<Vec<_>>())];
            out.extend(rows[1..].iter().map(|r| line(r)));
            out.join("\n")
        }
        // Text writes a locked table as nothing at all; its words are known, so here they go in as a plain table
        Node::Locked { rows: Some(rows), .. } if !rows.is_empty() => {
            let line = |cells: &[String]| {
                format!(
                    "| {} |",
                    cells.iter().map(|c| escape_text(&c.replace('\n', " "), true)).collect::<Vec<_>>().join(" | ")
                )
            };
            let width = rows.iter().map(Vec::len).max().unwrap_or(1);
            let pad = |r: &Vec<String>| {
                let mut r = r.clone();
                r.resize(width, String::new());
                r
            };
            let mut out = vec![line(&pad(&rows[0])), line(&vec!["---".to_string(); width])];
            out.extend(rows[1..].iter().map(|r| line(&pad(r))));
            out.join("\n")
        }
        other => text_content(other),
    }
}

/// Every picture the document shows at block level (in lists too), in its order: `f` gets its address and gives the
/// new one, or None to leave the picture out.
pub fn each_image(doc: &mut Vec<Node>, f: &mut dyn FnMut(&str) -> Option<String>) {
    doc.retain_mut(|n| match n {
        Node::Image { src, .. } => match f(src) {
            Some(new) => {
                *src = new;
                true
            }
            None => false,
        },
        Node::Bullet { items, .. } | Node::Ordered { items, .. } => {
            for item in items.iter_mut() {
                each_image(item, f);
            }
            true
        }
        _ => true,
    });
}

/// serializeMarkdown(doc, null): new Markdown for every block, a blank line between blocks.
pub fn serialize_markdown(doc: &[Node]) -> String {
    let parts: Vec<String> = doc
        .iter()
        .filter(|n| !matches!(n, Node::Text(t) if t.level == 0 && t.kids.is_empty()))
        .map(|n| block_text(n, ""))
        .collect();
    if parts.is_empty() {
        return String::new();
    }
    parts.join("\n\n") + "\n"
}

// ---------- markdown.js: Markdown to blocks ----------

static LIST: OnceLock<Regex> = OnceLock::new();
static DELIM: OnceLock<Regex> = OnceLock::new();

fn list_re() -> &'static Regex {
    re(&LIST, r"^(?:([-*+])|(\d{1,9})\.) (.*)$")
}

fn delim_re() -> &'static Regex {
    re(&DELIM, r"^\|?\s*-+\s*(\|\s*-+\s*)*\|?$")
}

fn split_row(line: &str) -> Vec<String> {
    let mut s = line.trim();
    if let Some(r) = s.strip_prefix('|') {
        s = r;
    }
    if s.ends_with('|') && !s.ends_with("\\|") {
        s = &s[..s.len() - 1];
    }
    let chars: Vec<char> = s.chars().collect();
    let mut cells = Vec::new();
    let mut cur = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' && i + 1 < chars.len() {
            cur.push(chars[i]);
            cur.push(chars[i + 1]);
            i += 2;
            continue;
        }
        if chars[i] == '|' {
            cells.push(std::mem::take(&mut cur));
        } else {
            cur.push(chars[i]);
        }
        i += 1;
    }
    cells.push(cur);
    cells.into_iter().map(|c| c.trim().to_string()).collect()
}

static BLOCK_START: OnceLock<Regex> = OnceLock::new();
static DEEP: OnceLock<Regex> = OnceLock::new();
static RULE: OnceLock<Regex> = OnceLock::new();
static SETEXT: OnceLock<Regex> = OnceLock::new();
static LINKDEF: OnceLock<Regex> = OnceLock::new();
static OTHER_LIST: OnceLock<Regex> = OnceLock::new();

fn check_line(l: &str) -> Res<()> {
    let bad = l.ends_with(char::is_whitespace)
        || l.starts_with(char::is_whitespace)
        || re(&BLOCK_START, r"^(>|```|~~~|<|:::|\$\$)").is_match(l)
        || re(&DEEP, r"^(#{4,6})(\s|$)").is_match(l)
        || re(&RULE, r"^(?:-\s*(?:-\s*){2,}|\*\s*(?:\*\s*){2,}|_\s*(?:_\s*){2,})$").is_match(l)
        || re(&SETEXT, r"^(=+|-+)$").is_match(l)
        || re(&LINKDEF, r"^\[[^\]]+\]:").is_match(l)
        || re(&OTHER_LIST, r"^\d{1,9}\)\s").is_match(l);
    if bad { Err(Foreign) } else { Ok(()) }
}

fn indent_of(n: usize) -> String {
    " ".repeat(n)
}

/// lines[i].slice(indent), by characters
fn after(line: &str, n: usize) -> &str {
    let at = line.char_indices().nth(n).map(|(b, _)| b).unwrap_or(line.len());
    &line[at..]
}

fn parse_list(lines: &[&str], start: usize, indent: usize) -> Res<(Node, usize)> {
    let first = list_re().captures(after(lines[start], indent)).ok_or(Foreign)?;
    let ordered = first.get(1).is_none();
    let marker = first.get(1).map(|m| m.as_str().to_string()).unwrap_or_default();
    let first_num: u64 = first.get(2).map(|m| m.as_str().parse().unwrap_or(1)).unwrap_or(1);
    let mut items = Vec::new();
    let mut i = start;
    let mut loose: Option<bool> = None;
    let pad = indent_of(indent);
    loop {
        let m = list_re().captures(after(lines[i], indent)).ok_or(Foreign)?;
        let text = m.get(3).unwrap().as_str();
        if text.starts_with("[ ]") || text.starts_with("[x]") || text.starts_with("[X]") {
            return Err(Foreign);
        }
        if text.is_empty() || text.ends_with(char::is_whitespace) || text.starts_with(char::is_whitespace) {
            return Err(Foreign);
        }
        let content_indent = indent + (lines[i].chars().count() - indent - text.chars().count());
        let cpad = indent_of(content_indent);
        let mut kids = vec![para(inline_nodes(text)?)];
        i += 1;
        while i < lines.len()
            && lines[i].starts_with(&cpad)
            && after(lines[i], content_indent).chars().next() != Some(' ')
            && !lines[i].is_empty()
        {
            if !list_re().is_match(after(lines[i], content_indent)) {
                return Err(Foreign);
            }
            let (node, end) = parse_list(lines, i, content_indent)?;
            kids.push(node);
            i = end;
        }
        items.push(kids);
        if i >= lines.len() {
            break;
        }
        if !lines[i].is_empty() {
            let peek = if lines[i].starts_with(&pad) { list_re().captures(after(lines[i], indent)) } else { None };
            if let Some(peek) = peek.filter(|_| after(lines[i], indent).chars().next() != Some(' ')) {
                if if ordered {
                    peek.get(1).is_some()
                } else {
                    peek.get(1).map(|m| m.as_str()) != Some(marker.as_str())
                } {
                    return Err(Foreign);
                }
                if loose == Some(true) {
                    return Err(Foreign);
                }
                loose = Some(false);
                continue;
            }
            if indent > 0 && !lines[i].starts_with(&pad) {
                break;
            }
            if indent > 0 && list_re().is_match(lines[i].trim_start()) {
                break;
            }
            return Err(Foreign);
        }
        // one blank line, then another item of the same list: a loose list
        let mut j = i;
        while j < lines.len() && lines[j].is_empty() {
            j += 1;
        }
        let again =
            if j < lines.len() && lines[j].starts_with(&pad) && after(lines[j], indent).chars().next() != Some(' ') {
                list_re().captures(after(lines[j], indent))
            } else {
                None
            };
        let same = again.as_ref().is_some_and(|a| {
            if ordered { a.get(1).is_none() } else { a.get(1).map(|m| m.as_str()) == Some(marker.as_str()) }
        });
        if same {
            if j - i != 1 || indent > 0 || loose == Some(false) {
                return Err(Foreign);
            }
            loose = Some(true);
            i = j;
            continue;
        }
        if j < lines.len() && lines[j].starts_with(' ') {
            return Err(Foreign);
        }
        break;
    }
    let loose = loose.unwrap_or(false);
    let node = if ordered {
        Node::Ordered { start: first_num, loose, num: None, items }
    } else {
        Node::Bullet { marker, loose, num: None, items }
    };
    Ok((node, i))
}

static HEADING: OnceLock<Regex> = OnceLock::new();
static HEADING_CLOSE: OnceLock<Regex> = OnceLock::new();
static PIC: OnceLock<Regex> = OnceLock::new();
static REMOTE: OnceLock<Regex> = OnceLock::new();

fn parse_block(lines: &[&str], i: usize) -> Res<(Node, usize)> {
    let l = lines[i];
    check_line(l)?;
    if let Some(h) = re(&HEADING, r"^(#{1,3}) (.+)$").captures(l) {
        let body = h.get(2).unwrap().as_str();
        if re(&HEADING_CLOSE, r"\s#+$").is_match(body) || body.starts_with(char::is_whitespace) {
            return Err(Foreign);
        }
        return Ok((
            Node::Text(Textblock {
                level: h[1].len() as u32,
                kids: join_texts(inline_nodes(body)?),
                ..Default::default()
            }),
            i + 1,
        ));
    }
    if !l.is_empty() && l.len() <= 3 && l.chars().all(|c| c == '#') {
        return Err(Foreign);
    }
    if list_re().is_match(l) {
        return parse_list(lines, i, 0);
    }
    if let Some(p) = re(&PIC, r"^!\[([^\]\\]*)\]\(([^()\s]+)\)$").captures(l) {
        if re(&REMOTE, r"(?i)^(https?:)?//").is_match(&p[2]) {
            return Err(Foreign);
        }
        if i + 1 < lines.len() && !lines[i + 1].is_empty() {
            return Err(Foreign);
        }
        return Ok((Node::Image { alt: p[1].to_string(), src: p[2].to_string(), iid: None, rel: None }, i + 1));
    }
    if l.contains('|') && i + 1 < lines.len() && delim_re().is_match(lines[i + 1]) && lines[i + 1].contains('-') {
        let head = split_row(l);
        let width = head.len();
        if split_row(lines[i + 1]).len() != width {
            return Err(Foreign);
        }
        let cell = |head: bool, s: &str| -> Res<TCell> {
            Ok(TCell { head, tcpr: String::new(), kids: vec![para(inline_nodes(s)?)] })
        };
        let mut rows = vec![(String::new(), head.iter().map(|c| cell(true, c)).collect::<Res<Vec<_>>>()?)];
        let mut j = i + 2;
        while j < lines.len() && !lines[j].is_empty() {
            if !lines[j].contains('|') || lines[j].ends_with(char::is_whitespace) {
                return Err(Foreign);
            }
            let cells = split_row(lines[j]);
            if cells.len() != width {
                return Err(Foreign);
            }
            rows.push((String::new(), cells.iter().map(|c| cell(false, c)).collect::<Res<Vec<_>>>()?));
            j += 1;
        }
        return Ok((Node::Table { tid: None, tblpr: String::new(), grid: String::new(), rows }, j));
    }
    if delim_re().is_match(l) && l.contains('-') {
        return Err(Foreign);
    }
    if i + 1 < lines.len() && !lines[i + 1].is_empty() {
        return Err(Foreign);
    }
    Ok((para(inline_nodes(l)?), i + 1))
}

/// The article view of a Markdown file, or None: Text would show this file in its code view, where it is only text.
pub fn parse_markdown(src: &str) -> Option<Vec<Node>> {
    if src.contains('\r') || src.contains('\0') {
        return None;
    }
    let lines: Vec<&str> = src.split('\n').collect();
    let mut i = 0;
    while i < lines.len() && lines[i].is_empty() {
        i += 1;
    }
    if i == lines.len() {
        return Some(vec![para(Vec::new())]);
    }
    if lines[i] == "---" && i == 0 {
        return None;
    }
    let mut nodes = Vec::new();
    while i < lines.len() {
        let (node, end) = parse_block(&lines, i).ok()?;
        // new Markdown for this block must mean the same thing, or the file's meaning could change
        let again_text = block_text(&node, "");
        let again_lines: Vec<&str> = again_text.split('\n').collect();
        let (again, _) = parse_block(&again_lines, 0).ok()?;
        if again != node {
            return None;
        }
        let mut j = end;
        while j < lines.len() && lines[j].is_empty() {
            j += 1;
        }
        nodes.push(node);
        i = j;
    }
    Some(nodes)
}
