//! Taking a paragraph's old words out of a page. The words are found the way a reader draws them: the text state is
//! followed through the content, each string's place on the page worked out, and a string whose middle falls inside a
//! paragraph being replaced is swapped for a plain move of the same width. Everything after it on the line stays exactly
//! where it was; nothing else in the content changes.

use crate::geom::Mat;
use lopdf::content::{Content, Operation};
use lopdf::{Dictionary, Document, Object};
use std::collections::HashMap;

/// How wide a font's codes are, read once per font.
struct Metrics {
    /// bytes per code: 1, 2, or mixed (a lead byte at or above 0x81 starts two)
    bytes: u8,
    widths: HashMap<u32, f32>,
    default: f32,
    /// glyph space to text space: 1/1000 for most, the FontMatrix for Type3
    scale: f32,
}

fn num(o: &Object) -> Option<f32> {
    match o {
        Object::Integer(i) => Some(*i as f32),
        Object::Real(r) => Some(*r),
        _ => None,
    }
}

fn deref<'a>(doc: &'a Document, o: &'a Object) -> &'a Object {
    doc.dereference(o).map(|(_, o)| o).unwrap_or(o)
}

fn metrics(doc: &Document, font: &Dictionary) -> Metrics {
    let subtype = font.get(b"Subtype").and_then(|o| o.as_name()).unwrap_or(b"");
    if subtype == b"Type0" {
        let enc = font.get(b"Encoding").map(|o| deref(doc, o)).ok();
        let enc_name =
            enc.and_then(|o| o.as_name().ok()).map(|n| String::from_utf8_lossy(n).into_owned()).unwrap_or_default();
        let bytes = if enc_name.is_empty()
            || enc_name.starts_with("Identity")
            || enc_name.contains("UCS2")
            || enc_name.contains("UTF16")
        {
            2
        } else if enc_name.contains("EUC")
            || enc_name.contains("GBK")
            || enc_name.contains("RKSJ")
            || enc_name.contains("B5")
            || enc_name.contains("ETen")
            || enc_name.contains("HKscs")
            || enc_name.contains("KSC")
            || enc_name.contains("GB-")
        {
            3
        } else {
            2
        };
        let identity = enc_name.is_empty() || enc_name.starts_with("Identity");
        let mut m = Metrics { bytes, widths: HashMap::new(), default: 1000.0, scale: 0.001 };
        let desc = font
            .get(b"DescendantFonts")
            .map(|o| deref(doc, o))
            .ok()
            .and_then(|o| o.as_array().ok())
            .and_then(|a| a.first())
            .map(|o| deref(doc, o))
            .and_then(|o| o.as_dict().ok());
        if let Some(d) = desc {
            if let Some(dw) = d.get(b"DW").ok().and_then(num) {
                m.default = dw;
            }
            // a CID is only the code itself with Identity; otherwise only the default width is known
            if identity {
                if let Ok(w) = d.get(b"W").map(|o| deref(doc, o)).and_then(|o| o.as_array()) {
                    let mut i = 0;
                    while i + 1 < w.len() {
                        let first = num(deref(doc, &w[i])).unwrap_or(0.0) as u32;
                        match deref(doc, &w[i + 1]) {
                            Object::Array(list) => {
                                for (k, v) in list.iter().enumerate() {
                                    m.widths.insert(first + k as u32, num(deref(doc, v)).unwrap_or(m.default));
                                }
                                i += 2;
                            }
                            last => {
                                let (Some(lastc), Some(v)) =
                                    (num(last), w.get(i + 2).map(|o| deref(doc, o)).and_then(num))
                                else {
                                    break;
                                };
                                for c in first..=(lastc as u32).min(first + 65535) {
                                    m.widths.insert(c, v);
                                }
                                i += 3;
                            }
                        }
                    }
                }
            }
        }
        return m;
    }
    let mut m = Metrics { bytes: 1, widths: HashMap::new(), default: 500.0, scale: 0.001 };
    let base = font
        .get(b"BaseFont")
        .and_then(|o| o.as_name())
        .map(|n| String::from_utf8_lossy(n).to_lowercase())
        .unwrap_or_default();
    if base.contains("courier") {
        m.default = 600.0;
    }
    if let Some(d) = font.get(b"FontDescriptor").map(|o| deref(doc, o)).ok().and_then(|o| o.as_dict().ok()) {
        if let Some(mw) = d.get(b"MissingWidth").ok().and_then(num) {
            if mw > 0.0 {
                m.default = mw;
            }
        }
    }
    if subtype == b"Type3" {
        if let Ok(fm) = font.get(b"FontMatrix").map(|o| deref(doc, o)).and_then(|o| o.as_array()) {
            m.scale = fm.first().map(|o| deref(doc, o)).and_then(num).unwrap_or(0.001);
        }
    }
    let first = font.get(b"FirstChar").ok().and_then(num).unwrap_or(0.0) as u32;
    if let Ok(w) = font.get(b"Widths").map(|o| deref(doc, o)).and_then(|o| o.as_array()) {
        for (k, v) in w.iter().enumerate() {
            m.widths.insert(first + k as u32, num(deref(doc, v)).unwrap_or(m.default));
        }
    }
    m
}

impl Metrics {
    /// Each code in the string with its width in glyph units, and whether it is the single-byte space.
    fn codes(&self, s: &[u8]) -> Vec<(f32, bool)> {
        let mut out = Vec::new();
        let mut i = 0;
        while i < s.len() {
            let (code, n) = match self.bytes {
                1 => (s[i] as u32, 1),
                3 if s[i] < 0x81 => (s[i] as u32, 1),
                _ if i + 1 < s.len() => (((s[i] as u32) << 8) | s[i + 1] as u32, 2),
                _ => (s[i] as u32, 1),
            };
            let w = self.widths.get(&code).copied().unwrap_or(if self.bytes == 3 && n == 1 {
                self.default / 2.0
            } else {
                self.default
            });
            out.push((w, n == 1 && code == 32));
            i += n;
        }
        out
    }
}

#[derive(Clone)]
struct TextState {
    ctm: Mat,
    tc: f32,
    tw: f32,
    th: f32,
    tl: f32,
    tfs: f32,
    rise: f32,
    font: Vec<u8>,
}

/// The page's own content and its fonts; `inside` says, for a point in user space, whether it belongs to a paragraph
/// being replaced. Answers the new content and how many strings were taken out of each paragraph (by index).
pub fn cut(
    doc: &Document,
    content: &[u8],
    fonts: &Dictionary,
    inside: &dyn Fn(f32, f32) -> Option<usize>,
    blocks: usize,
) -> Option<(Vec<u8>, Vec<usize>)> {
    let parsed = Content::decode(content).ok()?;
    let mut cache: HashMap<Vec<u8>, Metrics> = HashMap::new();
    let mut st = TextState { ctm: Mat::ID, tc: 0.0, tw: 0.0, th: 1.0, tl: 0.0, tfs: 0.0, rise: 0.0, font: Vec::new() };
    let mut stack: Vec<TextState> = Vec::new();
    let (mut tm, mut tlm) = (Mat::ID, Mat::ID);
    let mut found = vec![0usize; blocks];
    let mut out: Vec<Operation> = Vec::with_capacity(parsed.operations.len());
    let f = |ops: &[Object], i: usize| ops.get(i).and_then(num).unwrap_or(0.0);

    for op in parsed.operations {
        let o = &op.operands;
        match op.operator.as_str() {
            "q" => stack.push(st.clone()),
            "Q" => {
                if let Some(s) = stack.pop() {
                    st = s;
                }
            }
            "cm" if o.len() == 6 => st.ctm = Mat([f(o, 0), f(o, 1), f(o, 2), f(o, 3), f(o, 4), f(o, 5)]).then(&st.ctm),
            "BT" => {
                tm = Mat::ID;
                tlm = Mat::ID;
            }
            "Tc" => st.tc = f(o, 0),
            "Tw" => st.tw = f(o, 0),
            "Tz" => st.th = f(o, 0) / 100.0,
            "TL" => st.tl = f(o, 0),
            "Ts" => st.rise = f(o, 0),
            "Tf" => {
                st.font = o.first().and_then(|n| n.as_name().ok()).map(|n| n.to_vec()).unwrap_or_default();
                st.tfs = f(o, 1);
            }
            "Td" => {
                tlm = Mat::translate(f(o, 0), f(o, 1)).then(&tlm);
                tm = tlm;
            }
            "TD" => {
                st.tl = -f(o, 1);
                tlm = Mat::translate(f(o, 0), f(o, 1)).then(&tlm);
                tm = tlm;
            }
            "Tm" if o.len() == 6 => {
                tlm = Mat([f(o, 0), f(o, 1), f(o, 2), f(o, 3), f(o, 4), f(o, 5)]);
                tm = tlm;
            }
            "T*" => {
                tlm = Mat::translate(0.0, -st.tl).then(&tlm);
                tm = tlm;
            }
            _ => {}
        }
        let show = matches!(op.operator.as_str(), "Tj" | "TJ" | "'" | "\"");
        if !show {
            out.push(op);
            continue;
        }
        // ' and " move to the next line first; " also sets the spacing
        if op.operator == "'" || op.operator == "\"" {
            if op.operator == "\"" {
                st.tw = f(o, 0);
                st.tc = f(o, 1);
                out.push(Operation::new("Tw", vec![o[0].clone()]));
                out.push(Operation::new("Tc", vec![o.get(1).cloned().unwrap_or(Object::Integer(0))]));
            }
            tlm = Mat::translate(0.0, -st.tl).then(&tlm);
            tm = tlm;
            out.push(Operation::new("T*", vec![]));
        }
        if !cache.contains_key(&st.font) {
            let fd = fonts.get(&st.font).map(|o| deref(doc, o)).ok().and_then(|o| o.as_dict().ok());
            let m = fd.map(|d| metrics(doc, d)).unwrap_or(Metrics {
                bytes: 1,
                widths: HashMap::new(),
                default: 500.0,
                scale: 0.001,
            });
            cache.insert(st.font.clone(), m);
        }
        let m = &cache[&st.font];
        let items: Vec<Object> = match op.operator.as_str() {
            "TJ" => o.first().and_then(|a| a.as_array().ok()).cloned().unwrap_or_default(),
            "\"" => o.get(2).cloned().into_iter().collect(),
            _ => o.first().cloned().into_iter().collect(),
        };
        let mut kept: Vec<Object> = Vec::new();
        let mut any_cut = false;
        for item in items {
            match &item {
                Object::String(s, _) => {
                    let mut w = 0.0;
                    for (gw, space) in m.codes(s) {
                        w += (gw * m.scale * st.tfs + st.tc + if space { st.tw } else { 0.0 }) * st.th;
                    }
                    // where the string's middle sits on the page, on its baseline
                    let mid = Mat::translate(w / 2.0, st.rise).then(&tm).then(&st.ctm);
                    let hit = inside(mid.0[4], mid.0[5]);
                    if let (Some(b), true) = (hit, st.tfs != 0.0 && st.th != 0.0) {
                        found[b] += 1;
                        any_cut = true;
                        kept.push(Object::Real(-w * 1000.0 / (st.tfs * st.th)));
                    } else {
                        kept.push(item.clone());
                    }
                    tm = Mat::translate(w, 0.0).then(&tm);
                }
                other => {
                    let n = num(other).unwrap_or(0.0);
                    tm = Mat::translate(-n / 1000.0 * st.tfs * st.th, 0.0).then(&tm);
                    kept.push(item.clone());
                }
            }
        }
        if !any_cut {
            // as it was, minus the line move already written for ' and "
            match op.operator.as_str() {
                "'" | "\"" => out.push(Operation::new("Tj", vec![items_first(&op)])),
                _ => out.push(op),
            }
            continue;
        }
        // neighbouring numbers become one
        let mut merged: Vec<Object> = Vec::new();
        for k in kept {
            if let (Some(Object::Real(a)), Some(b)) =
                (merged.last().cloned(), num(&k).filter(|_| !matches!(k, Object::String(..))))
            {
                merged.pop();
                merged.push(Object::Real(a + b));
            } else {
                merged.push(k);
            }
        }
        out.push(Operation::new("TJ", vec![Object::Array(merged)]));
    }
    let bytes = Content { operations: out }.encode().ok()?;
    Some((bytes, found))
}

fn items_first(op: &Operation) -> Object {
    match op.operator.as_str() {
        "\"" => op.operands.get(2).cloned().unwrap_or(Object::String(vec![], lopdf::StringFormat::Literal)),
        _ => op.operands.first().cloned().unwrap_or(Object::String(vec![], lopdf::StringFormat::Literal)),
    }
}
