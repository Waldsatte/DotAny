//! An SVG is a program as much as a picture. What reaches the page is rebuilt here from the parts that only draw:
//! no scripts, no event handlers, nothing that fetches, no other documents embedded. The file on disk is never changed by this.
//! The page then shows it as an image, which on its own forbids scripts and requests, under a policy that forbids them again.

#![allow(deprecated)] // unescape_value is the plain reading we want here

use quick_xml::events::{BytesStart, Event};
use quick_xml::{Reader, Writer};
use serde::Serialize;

const DROP_ELEMENTS: [&str; 11] =
    ["script", "foreignobject", "iframe", "embed", "object", "audio", "video", "canvas", "handler", "listener", "set"];
const SAFE_DATA: [&str; 5] =
    ["data:image/png", "data:image/jpeg", "data:image/gif", "data:image/webp", "data:image/bmp"];

#[derive(Serialize, Clone, Debug)]
pub struct Svg {
    pub text: String,
    pub w: u32,
    pub h: u32,
    pub removed: u32,
}

fn local(name: &[u8]) -> String {
    let n = String::from_utf8_lossy(name).to_lowercase();
    n.rsplit(':').next().unwrap_or("").to_string()
}

fn safe_link(v: &str) -> bool {
    let v = v.trim();
    v.starts_with('#') || SAFE_DATA.iter().any(|d| v.to_lowercase().starts_with(d))
}

/// A url(...) is fine only if it points inside this file, and an @import never is.
fn safe_style(v: &str) -> bool {
    let low = v.to_lowercase();
    if low.contains("@import") || low.contains("expression(") || low.contains("javascript:") {
        return false;
    }
    let mut rest = low.as_str();
    while let Some(i) = rest.find("url(") {
        let inner = rest[i + 4..].trim_start().trim_start_matches(['"', '\'']).trim_start();
        if !inner.starts_with('#') && !SAFE_DATA.iter().any(|d| inner.starts_with(d)) {
            return false;
        }
        rest = &rest[i + 4..];
    }
    true
}

fn clean(e: &BytesStart, removed: &mut u32) -> Result<BytesStart<'static>, ()> {
    let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
    let mut out = BytesStart::new(name);
    for a in e.attributes().with_checks(false) {
        let a = a.map_err(|_| ())?;
        let key = local(a.key.as_ref());
        let value = a.unescape_value().map_err(|_| ())?.into_owned();
        let bad = key.starts_with("on")
            || ((key == "href" || key == "src") && !safe_link(&value))
            || (key == "style" && !safe_style(&value))
            || (key != "style" && value.to_lowercase().contains("url(") && !safe_style(&value));
        if bad {
            *removed += 1;
            continue;
        }
        out.push_attribute((String::from_utf8_lossy(a.key.as_ref()).as_ref(), value.as_str()));
    }
    Ok(out)
}

/// The size it asks to be: its width and height when they're real lengths, else its viewBox, else a plain default.
fn length(v: &str) -> Option<f64> {
    let v = v.trim();
    let digits = v.find(|c: char| !(c.is_ascii_digit() || c == '.' || c == '-')).unwrap_or(v.len());
    let n: f64 = v[..digits].parse().ok()?;
    let k = match v[digits..].trim() {
        "" | "px" => 1.0,
        "pt" => 96.0 / 72.0,
        "pc" => 16.0,
        "in" => 96.0,
        "cm" => 96.0 / 2.54,
        "mm" => 96.0 / 25.4,
        _ => return None,
    };
    if n > 0.0 { Some(n * k) } else { None }
}

fn root_size(e: &BytesStart) -> (u32, u32) {
    let get = |n: &str| {
        e.attributes()
            .with_checks(false)
            .flatten()
            .find(|a| local(a.key.as_ref()) == n)
            .and_then(|a| a.unescape_value().ok().map(|v| v.into_owned()))
    };
    let (mut w, mut h) = (get("width").and_then(|v| length(&v)), get("height").and_then(|v| length(&v)));
    if w.is_none() || h.is_none() {
        if let Some(vb) = get("viewbox") {
            let n: Vec<f64> = vb.split([' ', ',']).filter(|s| !s.is_empty()).filter_map(|s| s.parse().ok()).collect();
            if n.len() == 4 && n[2] > 0.0 && n[3] > 0.0 {
                let aspect = n[3] / n[2];
                match (w, h) {
                    (Some(a), None) => h = Some(a * aspect),
                    (None, Some(b)) => w = Some(b / aspect),
                    _ => {
                        w = Some(n[2]);
                        h = Some(n[3]);
                    }
                }
            }
        }
    }
    let (w, h) = (w.unwrap_or(800.0), h.unwrap_or(600.0));
    ((w.round() as u32).clamp(1, 16384), (h.round() as u32).clamp(1, 16384))
}

pub fn sanitize(bytes: &[u8]) -> Option<Svg> {
    let text = std::str::from_utf8(bytes.strip_prefix(b"\xEF\xBB\xBF").unwrap_or(bytes)).ok()?;
    let mut reader = Reader::from_str(text);
    reader.config_mut().check_end_names = true;
    let mut writer = Writer::new(Vec::with_capacity(text.len()));
    let (mut skipping, mut in_style, mut removed) = (0usize, false, 0u32);
    let mut size = None;
    let mut depth = 0usize;
    loop {
        let ev = reader.read_event().ok()?;
        match ev {
            Event::Eof => break,
            // DOCTYPE is where entity bombs and outside references live; it draws nothing
            Event::DocType(_) | Event::PI(_) | Event::Comment(_) => {
                if matches!(ev, Event::DocType(_)) {
                    removed += 1;
                }
            }
            Event::Decl(d) => {
                writer.write_event(Event::Decl(d)).ok()?;
            }
            Event::Start(e) => {
                let n = local(e.name().as_ref());
                if depth == 0 {
                    if n != "svg" {
                        return None;
                    }
                    size = Some(root_size(&e));
                }
                depth += 1;
                if skipping > 0 || DROP_ELEMENTS.contains(&n.as_str()) {
                    if skipping == 0 {
                        removed += 1;
                    }
                    skipping += 1;
                    continue;
                }
                in_style = n == "style";
                let cleaned = clean(&e, &mut removed).ok()?;
                writer.write_event(Event::Start(cleaned)).ok()?;
            }
            Event::Empty(e) => {
                let n = local(e.name().as_ref());
                if depth == 0 {
                    if n != "svg" {
                        return None;
                    }
                    size = Some(root_size(&e));
                }
                if skipping > 0 {
                    continue;
                }
                if DROP_ELEMENTS.contains(&n.as_str()) {
                    removed += 1;
                    continue;
                }
                let cleaned = clean(&e, &mut removed).ok()?;
                writer.write_event(Event::Empty(cleaned)).ok()?;
            }
            Event::End(e) => {
                depth = depth.saturating_sub(1);
                if skipping > 0 {
                    skipping -= 1;
                    continue;
                }
                in_style = false;
                writer.write_event(Event::End(e)).ok()?;
            }
            Event::Text(t) => {
                if skipping > 0 {
                    continue;
                }
                let raw = String::from_utf8_lossy(t.as_ref()).into_owned();
                if in_style && !safe_style(&raw) {
                    removed += 1;
                    continue;
                }
                writer.write_event(Event::Text(t)).ok()?;
            }
            Event::CData(t) => {
                if skipping > 0 {
                    continue;
                }
                let raw = String::from_utf8_lossy(t.as_ref()).into_owned();
                if in_style && !safe_style(&raw) {
                    removed += 1;
                    continue;
                }
                writer.write_event(Event::CData(t)).ok()?;
            }
            other => {
                if skipping == 0 {
                    writer.write_event(other).ok()?;
                }
            }
        }
    }
    if depth != 0 {
        return None;
    }
    let (w, h) = size?;
    Some(Svg { text: String::from_utf8(writer.into_inner()).ok()?, w, h, removed })
}
