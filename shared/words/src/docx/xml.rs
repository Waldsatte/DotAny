//! A light view of an XML part: every element knows the exact bytes it came from, so anything we don't change can be
//! written back as it was. Nothing here decodes or normalizes what it keeps.

use quick_xml::Reader;
use quick_xml::events::Event;

#[derive(Debug, Clone)]
pub struct El {
    pub name: String,
    /// The raw attributes, as written.
    pub attrs: String,
    pub start: usize,
    pub end: usize,
    /// Where the children start and end; the same point for an empty element.
    pub inner_start: usize,
    pub inner_end: usize,
    pub kids: Vec<Node>,
}

#[derive(Debug, Clone)]
pub enum Node {
    El(El),
    Text(usize, usize),
    Other(usize, usize),
}

impl El {
    pub fn raw<'a>(&self, src: &'a str) -> &'a str {
        &src[self.start..self.end]
    }
    pub fn inner<'a>(&self, src: &'a str) -> &'a str {
        &src[self.inner_start..self.inner_end]
    }
    pub fn els(&self) -> impl Iterator<Item = &El> {
        self.kids.iter().filter_map(|k| if let Node::El(e) = k { Some(e) } else { None })
    }
    pub fn child(&self, name: &str) -> Option<&El> {
        self.els().find(|e| e.name == name)
    }
    pub fn attr(&self, name: &str) -> Option<String> {
        attr_in(&self.attrs, name)
    }
    /// Any element of this name, at any depth.
    pub fn has_deep(&self, name: &str) -> bool {
        self.els().any(|e| e.name == name || e.has_deep(name))
    }
    /// The element's own text, entities resolved.
    pub fn text(&self, src: &str) -> String {
        let mut out = String::new();
        for k in &self.kids {
            if let Node::Text(a, b) = k {
                out.push_str(&unescape(&src[*a..*b]));
            }
        }
        out
    }
}

pub fn attr_in(attrs: &str, name: &str) -> Option<String> {
    let mut rest = attrs;
    loop {
        let eq = rest.find('=')?;
        let key = rest[..eq].trim();
        let after = rest[eq + 1..].trim_start();
        let quote = after.chars().next()?;
        if quote != '"' && quote != '\'' {
            return None;
        }
        let close = after[1..].find(quote)?;
        if key == name {
            return Some(unescape(&after[1..1 + close]));
        }
        rest = &after[close + 2..];
    }
}

pub fn unescape(s: &str) -> String {
    if !s.contains('&') {
        return s.to_string();
    }
    let mut out = String::with_capacity(s.len());
    let mut rest = s;
    while let Some(at) = rest.find('&') {
        out.push_str(&rest[..at]);
        rest = &rest[at..];
        let Some(end) = rest.find(';') else {
            out.push_str(rest);
            return out;
        };
        let name = &rest[1..end];
        match name {
            "amp" => out.push('&'),
            "lt" => out.push('<'),
            "gt" => out.push('>'),
            "quot" => out.push('"'),
            "apos" => out.push('\''),
            _ => {
                let code = name
                    .strip_prefix("#x")
                    .or_else(|| name.strip_prefix("#X"))
                    .map(|h| u32::from_str_radix(h, 16))
                    .or_else(|| name.strip_prefix('#').map(|d| d.parse()));
                match code.and_then(|c| c.ok()).and_then(char::from_u32) {
                    Some(c) => out.push(c),
                    None => out.push_str(&rest[..=end]),
                }
            }
        }
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}

pub fn escape_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            // XML 1.0 can't hold these at all
            c if (c as u32) < 0x20 && c != '\t' && c != '\n' && c != '\r' => {}
            '\u{FFFE}' | '\u{FFFF}' => {}
            c => out.push(c),
        }
    }
    out
}

pub fn escape_attr(s: &str) -> String {
    escape_text(s).replace('"', "&quot;")
}

/// The whole part as a tree. None if it isn't well-formed.
pub fn parse(src: &str) -> Option<El> {
    let mut reader = Reader::from_str(src);
    reader.config_mut().check_end_names = true;
    reader.config_mut().trim_text(false);
    let mut stack: Vec<El> = Vec::new();
    let mut root: Option<El> = None;
    loop {
        let start = reader.buffer_position() as usize;
        let event = reader.read_event().ok()?;
        let end = reader.buffer_position() as usize;
        match event {
            Event::Start(e) => {
                let name = std::str::from_utf8(e.name().as_ref()).ok()?.to_string();
                let attrs = attrs_of(&src[start..end], &name, false)?;
                stack.push(El { name, attrs, start, end, inner_start: end, inner_end: end, kids: Vec::new() });
            }
            Event::Empty(e) => {
                let name = std::str::from_utf8(e.name().as_ref()).ok()?.to_string();
                let attrs = attrs_of(&src[start..end], &name, true)?;
                let el = El { name, attrs, start, end, inner_start: end, inner_end: end, kids: Vec::new() };
                match stack.last_mut() {
                    Some(p) => p.kids.push(Node::El(el)),
                    None => root = Some(el),
                }
            }
            Event::End(_) => {
                let mut el = stack.pop()?;
                el.inner_end = start;
                el.end = end;
                match stack.last_mut() {
                    Some(p) => p.kids.push(Node::El(el)),
                    None => root = Some(el),
                }
            }
            Event::Text(_) | Event::CData(_) | Event::GeneralRef(_) => {
                if let Some(p) = stack.last_mut() {
                    // text and references between them are one stretch of text
                    if let Some(Node::Text(_, b)) = p.kids.last_mut() {
                        *b = end;
                    } else {
                        p.kids.push(Node::Text(start, end));
                    }
                }
            }
            Event::Eof => break,
            _ => {
                if let Some(p) = stack.last_mut() {
                    p.kids.push(Node::Other(start, end));
                }
            }
        }
    }
    if !stack.is_empty() {
        return None;
    }
    root
}

fn attrs_of(tag: &str, name: &str, empty: bool) -> Option<String> {
    let body = tag.strip_prefix('<')?;
    let body = body.strip_suffix('>')?;
    let body = if empty { body.strip_suffix('/')? } else { body };
    Some(body.strip_prefix(name)?.to_string())
}
