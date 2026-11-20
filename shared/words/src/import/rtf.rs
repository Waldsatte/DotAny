//! RTF, read the way Word writes it and most other programs follow: paragraphs and their runs (bold, italic, underline,
//! size, color), alignment and spacing, headings from the style sheet, Word's list tables and the older `\pn` numbering,
//! tables, and PNG, JPEG or GIF pictures. Field results are shown as the words they display. Headers, footers, footnotes,
//! comments, shapes, objects, equations, links, merged and nested table cells and tracked changes count as missing.

use super::{Imported, Look, ParaLook, Sink, code_page};
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Dest {
    Text,
    Skip,
    /// Words that exist but aren't shown here: any letter in them means something is missing.
    Aside,
    Fonts,
    Colors,
    Styles,
    Lists,
    Overrides,
    Pn,
    FieldCode,
    Picture,
}

#[derive(Clone, Debug)]
struct Group {
    dest: Dest,
    look: Look,
    hidden: bool,
    deleted: bool,
    font: i32,
    uc: usize,
}

#[derive(Default)]
struct Picture {
    kind: Option<&'static str>,
    hex: Vec<u8>,
    raw: Vec<u8>,
    nibble: Option<u8>,
    w: u32,
    h: u32,
    w_goal: u32,
    h_goal: u32,
    scale_x: u32,
    scale_y: u32,
}

#[derive(Default)]
struct ListDef {
    id: i64,
    levels: Vec<u32>,
}

struct Reader<'a> {
    src: &'a [u8],
    at: usize,
    sink: Sink,
    stack: Vec<Group>,
    para: ParaLook,
    style: Option<i64>,
    outline: Option<u32>,
    ls: Option<i64>,
    ilvl: u32,
    pn: Option<(&'static str, u32)>,
    intbl: bool,
    itap: u32,
    default_cp: u32,
    default_font: i32,
    font_cp: HashMap<i32, u32>,
    font_def: i32,
    colors: Vec<Option<String>>,
    color_parts: [Option<u8>; 3],
    style_names: HashMap<i64, String>,
    style_outline: HashMap<i64, u32>,
    style_def: Option<i64>,
    style_name: String,
    lists: Vec<ListDef>,
    overrides: HashMap<i64, i64>,
    override_ls: Option<i64>,
    override_list: Option<i64>,
    field_code: String,
    pict: Option<Picture>,
    bytes: Vec<u8>,
    units: Vec<u16>,
    skip_after_u: usize,
    /// The key for a run of `\pn` paragraphs that belong together.
    pn_key: Option<u32>,
    next_key: u32,
}

const MAX_DEPTH: usize = 4000;

pub fn read(src: &[u8]) -> Option<Imported> {
    if !src.starts_with(b"{\\rtf") {
        return None;
    }
    let mut r = Reader {
        src,
        at: 0,
        sink: Sink::default(),
        stack: Vec::new(),
        para: ParaLook::default(),
        style: None,
        outline: None,
        ls: None,
        ilvl: 0,
        pn: None,
        intbl: false,
        itap: 0,
        default_cp: 1252,
        default_font: 0,
        font_cp: HashMap::new(),
        font_def: 0,
        colors: Vec::new(),
        color_parts: [None; 3],
        style_names: HashMap::new(),
        style_outline: HashMap::new(),
        style_def: None,
        style_name: String::new(),
        lists: Vec::new(),
        overrides: HashMap::new(),
        override_ls: None,
        override_list: None,
        field_code: String::new(),
        pict: None,
        bytes: Vec::new(),
        units: Vec::new(),
        skip_after_u: 0,
        pn_key: None,
        next_key: 1,
    };
    r.run()?;
    let look = r.para_look();
    Some(r.sink.finish(&look))
}

impl Reader<'_> {
    fn group(&self) -> &Group {
        self.stack.last().expect("inside the document group")
    }
    fn group_mut(&mut self) -> &mut Group {
        self.stack.last_mut().expect("inside the document group")
    }

    fn run(&mut self) -> Option<()> {
        while self.at < self.src.len() {
            let c = self.src[self.at];
            match c {
                b'{' => {
                    self.at += 1;
                    self.flush();
                    let top = self.stack.last().cloned().unwrap_or(Group {
                        dest: Dest::Text,
                        look: Look { sz: Some(24), ..Default::default() },
                        hidden: false,
                        deleted: false,
                        font: 0,
                        uc: 1,
                    });
                    if self.stack.len() >= MAX_DEPTH {
                        return None;
                    }
                    self.stack.push(top);
                    self.skip_after_u = 0;
                }
                b'}' => {
                    self.at += 1;
                    self.flush();
                    self.end_group();
                    self.skip_after_u = 0;
                    if self.stack.is_empty() {
                        return Some(());
                    }
                }
                b'\\' => {
                    self.at += 1;
                    self.control();
                }
                b'\r' | b'\n' => self.at += 1,
                _ => {
                    self.at += 1;
                    if self.stack.is_empty() {
                        return None;
                    }
                    if self.skip_after_u > 0 {
                        self.skip_after_u -= 1;
                        continue;
                    }
                    self.byte(c);
                }
            }
        }
        // a file cut short: what was read stands
        self.flush();
        Some(())
    }

    /// A byte of text, in the current destination.
    fn byte(&mut self, c: u8) {
        match self.group().dest {
            Dest::Picture => self.hex_digit(c),
            Dest::Colors if c == b';' => {
                let [r, g, b] = self.color_parts;
                self.colors.push(if r.is_some() || g.is_some() || b.is_some() {
                    Some(format!("{:02X}{:02X}{:02X}", r.unwrap_or(0), g.unwrap_or(0), b.unwrap_or(0)))
                } else {
                    None
                });
                self.color_parts = [None; 3];
            }
            Dest::Styles => {
                if c == b';' {
                    self.flush();
                    if let Some(s) = self.style_def.take() {
                        self.style_names.insert(s, std::mem::take(&mut self.style_name).trim().to_lowercase());
                    }
                    self.style_name.clear();
                } else {
                    self.bytes.push(c);
                }
            }
            Dest::Text | Dest::FieldCode | Dest::Aside => self.bytes.push(c),
            _ => {}
        }
    }

    fn hex_digit(&mut self, c: u8) {
        let Some(p) = self.pict.as_mut() else { return };
        let v = match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            b'A'..=b'F' => c - b'A' + 10,
            _ => return,
        };
        match p.nibble.take() {
            Some(hi) => p.hex.push(hi << 4 | v),
            None => p.nibble = Some(v),
        }
    }

    fn control(&mut self) {
        let Some(&c) = self.src.get(self.at) else { return };
        if !c.is_ascii_alphabetic() {
            self.at += 1;
            self.symbol(c);
            return;
        }
        let start = self.at;
        while self.at < self.src.len() && self.src[self.at].is_ascii_alphabetic() && self.at - start < 32 {
            self.at += 1;
        }
        let word = std::str::from_utf8(&self.src[start..self.at]).unwrap_or("").to_string();
        let neg = self.src.get(self.at) == Some(&b'-');
        if neg {
            self.at += 1;
        }
        let num_start = self.at;
        while self.at < self.src.len() && self.src[self.at].is_ascii_digit() && self.at - num_start < 10 {
            self.at += 1;
        }
        let param = if self.at > num_start {
            std::str::from_utf8(&self.src[num_start..self.at])
                .ok()
                .and_then(|n| n.parse::<i64>().ok())
                .map(|n| if neg { -n } else { n })
        } else {
            None
        };
        if self.src.get(self.at) == Some(&b' ') {
            self.at += 1;
        }
        if word == "bin" {
            let n = param.unwrap_or(0).max(0) as usize;
            let end = (self.at + n).min(self.src.len());
            if self.group().dest == Dest::Picture {
                if let Some(p) = self.pict.as_mut() {
                    p.raw.extend_from_slice(&self.src[self.at..end]);
                }
            }
            self.at = end;
            return;
        }
        if self.skip_after_u > 0 && word != "u" {
            self.skip_after_u -= 1;
            return;
        }
        if self.stack.is_empty() {
            return;
        }
        self.word(&word, param);
    }

    fn symbol(&mut self, c: u8) {
        match c {
            b'\'' => {
                let hex = self
                    .src
                    .get(self.at..self.at + 2)
                    .and_then(|h| std::str::from_utf8(h).ok())
                    .and_then(|h| u8::from_str_radix(h, 16).ok());
                self.at += 2.min(self.src.len() - self.at);
                if self.skip_after_u > 0 {
                    self.skip_after_u -= 1;
                    return;
                }
                if let Some(b) = hex {
                    self.byte(b);
                }
            }
            b'*' => {
                // an optional destination: kept only if this reader knows it
                let save = self.at;
                let known = self.peek_word();
                if !matches!(known.as_str(), "shppict" | "fldinst" | "pn" | "listtable" | "listoverridetable" | "ud") {
                    if matches!(
                        known.as_str(),
                        "moMath" | "moMathPara" | "shpinst" | "footnote" | "annotation" | "atrfstart"
                    ) {
                        lack!(self.sink);
                    }
                    self.flush();
                    if let Some(g) = self.stack.last_mut() {
                        g.dest = Dest::Skip;
                    }
                }
                self.at = save;
            }
            b'\\' | b'{' | b'}' => {
                if self.skip_after_u > 0 {
                    self.skip_after_u -= 1;
                    return;
                }
                self.byte(c)
            }
            b'~' => self.chars("\u{a0}"),
            b'_' => self.chars("\u{2011}"),
            b'-' => {}
            b'\r' | b'\n' => self.word("par", None),
            b'\t' => self.chars("\t"),
            b':' | b'|' => {}
            _ => {}
        }
    }

    fn peek_word(&self) -> String {
        let mut i = self.at;
        while self.src.get(i).is_some_and(|c| c.is_ascii_whitespace()) {
            i += 1;
        }
        if self.src.get(i) != Some(&b'\\') {
            return String::new();
        }
        let start = i + 1;
        let mut end = start;
        while self.src.get(end).is_some_and(|c| c.is_ascii_alphabetic()) {
            end += 1;
        }
        String::from_utf8_lossy(&self.src[start..end]).into_owned()
    }

    fn set_dest(&mut self, dest: Dest) {
        self.flush();
        self.group_mut().dest = dest;
    }

    fn word(&mut self, word: &str, param: Option<i64>) {
        let dest = self.group().dest;
        let n = param.unwrap_or(1);
        let on = param != Some(0);
        if dest == Dest::Skip {
            return;
        }
        // where the words go
        match word {
            "fonttbl" => return self.set_dest(Dest::Fonts),
            "colortbl" => return self.set_dest(Dest::Colors),
            "stylesheet" => return self.set_dest(Dest::Styles),
            "listtable" => return self.set_dest(Dest::Lists),
            "listoverridetable" => return self.set_dest(Dest::Overrides),
            "info" | "pntext" | "listtext" | "nonshppict" | "nonesttables" | "xmlnstbl" | "rsidtbl" | "generator"
            | "themedata" | "colorschememapping" | "latentstyles" | "datastore" | "revtbl" | "pgdsctbl" | "fchars"
            | "lchars" | "upr" | "template" | "userprops" | "docvar" | "pnseclvl" | "fldtype" | "datafield"
            | "mmathPr" | "panose" | "falt" | "fontemb" | "fontfile" | "leveltext" | "levelnumbers" | "listname"
            | "bkmkstart" | "bkmkend" | "atnid" | "atnauthor" | "atntime" | "atnref" | "atndate" | "atnicn"
            | "atrfstart" | "atrfend" | "objdata" | "objclass" | "objname" | "wgrffmtfilter" | "factoidname"
            | "ftnsep" | "ftnsepc" | "ftncn" | "aftnsep" | "aftnsepc" | "aftncn" => return self.set_dest(Dest::Skip),
            "header" | "footer" | "headerl" | "headerr" | "headerf" | "footerl" | "footerr" | "footerf"
            | "footnote" | "annotation" => return self.set_dest(Dest::Aside),
            "shp" | "object" | "shpgrp" | "do" | "mmath" => {
                lack!(self.sink);
                return self.set_dest(Dest::Skip);
            }
            "shppict" | "ud" => return,
            "pn" => return self.set_dest(Dest::Pn),
            "fldinst" => {
                self.flush();
                self.field_code.clear();
                return self.set_dest(Dest::FieldCode);
            }
            "pict" => {
                self.set_dest(Dest::Picture);
                self.pict = Some(Picture { scale_x: 100, scale_y: 100, ..Default::default() });
                return;
            }
            _ => {}
        }
        match dest {
            Dest::Fonts => {
                match word {
                    "f" => self.font_def = n as i32,
                    "fcharset" => {
                        let cp = charset_cp(n as u32, self.default_cp);
                        self.font_cp.insert(self.font_def, cp);
                    }
                    "cpg" => {
                        self.font_cp.insert(self.font_def, n as u32);
                    }
                    _ => {}
                }
                return;
            }
            Dest::Colors => {
                let v = Some(n.clamp(0, 255) as u8);
                match word {
                    "red" => self.color_parts[0] = v,
                    "green" => self.color_parts[1] = v,
                    "blue" => self.color_parts[2] = v,
                    _ => {}
                }
                return;
            }
            Dest::Styles => {
                match word {
                    "s" => self.style_def = Some(n),
                    "cs" | "ds" | "ts" | "tsrowd" => self.style_def = None,
                    "outlinelevel" => {
                        if let Some(s) = self.style_def {
                            self.style_outline.insert(s, n as u32);
                        }
                    }
                    _ => {}
                }
                return;
            }
            Dest::Lists => {
                match word {
                    "list" => self.lists.push(ListDef::default()),
                    "listlevel" => {
                        if let Some(l) = self.lists.last_mut() {
                            l.levels.push(0);
                        }
                    }
                    "levelnfc" | "levelnfcn" => {
                        if let Some(v) = self.lists.last_mut().and_then(|l| l.levels.last_mut()) {
                            *v = n as u32;
                        }
                    }
                    "listid" => {
                        if let Some(l) = self.lists.last_mut() {
                            l.id = n;
                        }
                    }
                    _ => {}
                }
                return;
            }
            Dest::Overrides => {
                match word {
                    "listoverride" => {
                        self.override_list = None;
                        self.override_ls = None;
                    }
                    "listid" => self.override_list = Some(n),
                    "ls" => self.override_ls = Some(n),
                    _ => {}
                }
                if let (Some(ls), Some(id)) = (self.override_ls, self.override_list) {
                    self.overrides.insert(ls, id);
                }
                return;
            }
            Dest::Pn => {
                match word {
                    "pnlvlblt" => self.pn = Some(("bullet", 0)),
                    "pnlvlbody" => self.pn = Some(("ordered", 0)),
                    "pnlvl" => self.pn = Some(("ordered", (n.clamp(1, 9) - 1) as u32)),
                    _ => {}
                }
                return;
            }
            Dest::Picture => {
                let Some(p) = self.pict.as_mut() else { return };
                let v = n.clamp(0, 1_000_000) as u32;
                match word {
                    "pngblip" => p.kind = Some("png"),
                    "jpegblip" => p.kind = Some("jpeg"),
                    "emfblip" | "wmetafile" | "macpict" | "dibitmap" | "wbitmap" | "pmmetafile" => {
                        p.kind = Some("other")
                    }
                    "picw" => p.w = v,
                    "pich" => p.h = v,
                    "picwgoal" => p.w_goal = v,
                    "pichgoal" => p.h_goal = v,
                    "picscalex" => p.scale_x = v,
                    "picscaley" => p.scale_y = v,
                    _ => {}
                }
                return;
            }
            Dest::FieldCode | Dest::Aside => {
                if dest == Dest::FieldCode {
                    self.text_word(word, n);
                }
                return;
            }
            _ => {}
        }
        // the document's own words and their look
        match word {
            "ansi" => self.default_cp = 1252,
            "mac" => self.default_cp = 10000,
            "pc" => self.default_cp = 437,
            "pca" => self.default_cp = 850,
            "ansicpg" => self.default_cp = n as u32,
            "deff" => self.default_font = n as i32,
            "par" => self.end_para(),
            "sect" | "page" | "column" => {}
            "pard" => {
                self.flush();
                self.para = ParaLook::default();
                self.style = None;
                self.outline = None;
                self.ls = None;
                self.ilvl = 0;
                self.pn = None;
                self.intbl = false;
                self.itap = 0;
            }
            "plain" => {
                self.flush();
                let font = self.default_font;
                let g = self.group_mut();
                g.look = Look { sz: Some(24), ..Default::default() };
                g.hidden = false;
                g.font = font;
            }
            "b" => self.look(|l| l.b = on),
            "i" => self.look(|l| l.i = on),
            "ul" | "uld" | "uldash" | "uldashd" | "uldashdd" | "uldb" | "ulth" | "ulw" | "ulwave" => {
                self.look(|l| l.u = on)
            }
            "ulnone" => self.look(|l| l.u = false),
            "fs" => self.look(|l| l.sz = Some(n.clamp(2, 3276) as u32)),
            "cf" => {
                let color = self.colors.get(n.max(0) as usize).cloned().flatten();
                self.look(|l| l.color = color.filter(|c| c != "000000"));
            }
            "f" => {
                self.flush();
                self.group_mut().font = n as i32;
            }
            "v" => {
                self.flush();
                self.group_mut().hidden = on;
            }
            "deleted" => {
                self.flush();
                lack!(self.sink);
                self.group_mut().deleted = on;
            }
            "revised" => lack!(self.sink),
            "uc" => self.group_mut().uc = n.clamp(0, 16) as usize,
            "qc" => self.para.align = Some("center"),
            "qr" => self.para.align = Some("right"),
            "qj" => self.para.align = Some("justify"),
            "ql" => self.para.align = None,
            "sl" => self.para.line = if n > 0 { Some(n as u32) } else { None },
            "slmult" => {
                if param == Some(0) {
                    // an exact height is not a multiple: kept out
                    self.para.line = None;
                }
            }
            "sb" => self.para.before = Some(n as i32),
            "sa" => self.para.after = Some(n as i32),
            "li" | "lin" => self.para.left = Some(n as i32),
            "ri" | "rin" => self.para.right = Some(n as i32),
            "fi" => self.para.first = Some(n as i32),
            "s" => self.style = Some(n),
            "outlinelevel" => self.outline = Some(n as u32),
            "ls" => self.ls = Some(n),
            "ilvl" => self.ilvl = n.clamp(0, 8) as u32,
            "intbl" => self.intbl = true,
            "itap" => {
                self.itap = n.max(0) as u32;
                self.intbl = n > 0;
                if n > 1 {
                    lack!(self.sink);
                }
            }
            "cell" => {
                self.flush();
                let look = self.para_look();
                self.sink.end_cell(&look);
            }
            "nestcell" | "nestrow" => {
                self.flush();
                let look = self.para_look();
                self.sink.end_para(&look, true);
            }
            "row" => {
                self.flush();
                self.sink.end_row();
            }
            "trowd" => {}
            "clmgf" | "clmrg" | "clvmgf" | "clvmrg" => lack!(self.sink),
            "field" => {}
            "fldrslt" => {
                let code = self.field_code.trim_start().to_uppercase();
                // a link's target, and a symbol drawn from a symbol font, can't be carried over
                if code.starts_with("HYPERLINK") || code.starts_with("SYMBOL") {
                    lack!(self.sink);
                }
            }
            _ => self.text_word(word, n),
        }
    }

    /// Control words that stand for a character.
    fn text_word(&mut self, word: &str, n: i64) {
        let s = match word {
            "tab" => "\t",
            "line" | "lbr" => "\n",
            "emdash" => "\u{2014}",
            "endash" => "\u{2013}",
            "bullet" => "\u{2022}",
            "lquote" => "\u{2018}",
            "rquote" => "\u{2019}",
            "ldblquote" => "\u{201C}",
            "rdblquote" => "\u{201D}",
            "emspace" => "\u{2003}",
            "enspace" => "\u{2002}",
            "qmspace" => "\u{2005}",
            "zwj" => "\u{200D}",
            "zwnj" => "\u{200C}",
            "zwbo" => "\u{200B}",
            "u" => {
                self.flush_bytes();
                let unit = if n < 0 { (n + 65536) as u16 } else { n.min(65535) as u16 };
                self.units.push(unit);
                self.skip_after_u = self.group().uc;
                return;
            }
            _ => return,
        };
        self.chars(s);
    }

    fn look(&mut self, f: impl FnOnce(&mut Look)) {
        self.flush();
        f(&mut self.group_mut().look);
    }

    fn chars(&mut self, s: &str) {
        self.flush();
        self.put(s);
    }

    /// Words, decoded, into wherever they belong.
    fn put(&mut self, s: &str) {
        if s.is_empty() {
            return;
        }
        let Some(g) = self.stack.last() else { return };
        match g.dest {
            Dest::Text if !g.hidden && !g.deleted => {
                let look = g.look.clone();
                self.sink.text(s, &look);
            }
            Dest::FieldCode => self.field_code.push_str(s),
            Dest::Styles => self.style_name.push_str(s),
            Dest::Aside => {
                if s.chars().any(|c| c.is_alphanumeric()) {
                    lack!(self.sink);
                }
            }
            _ => {}
        }
    }

    fn flush_bytes(&mut self) {
        if self.bytes.is_empty() {
            return;
        }
        let bytes = std::mem::take(&mut self.bytes);
        let font = self.stack.last().map(|g| g.font).unwrap_or(0);
        let cp = self.font_cp.get(&font).copied().unwrap_or(self.default_cp);
        let text = match code_page(cp) {
            Some(enc) => enc.decode_without_bom_handling(&bytes).0.into_owned(),
            None => {
                if bytes.iter().any(|b| *b >= 0x80) {
                    lack!(self.sink);
                }
                bytes.iter().map(|b| if *b < 0x80 { *b as char } else { '\u{FFFD}' }).collect()
            }
        };
        self.put(&text);
    }

    fn flush_units(&mut self) {
        if self.units.is_empty() {
            return;
        }
        let units = std::mem::take(&mut self.units);
        let text: String = char::decode_utf16(units).map(|c| c.unwrap_or('\u{FFFD}')).collect();
        self.put(&text);
    }

    fn flush(&mut self) {
        self.flush_units();
        self.flush_bytes();
    }

    fn end_group(&mut self) {
        let Some(g) = self.stack.pop() else { return };
        if g.dest == Dest::Picture && self.stack.last().is_none_or(|p| p.dest != Dest::Picture) {
            if let Some(p) = self.pict.take() {
                self.picture(p);
            }
        }
    }

    fn picture(&mut self, p: Picture) {
        // only a picture in the words is shown; one inside a shape or object was already counted as missing
        if self.stack.last().is_none_or(|g| g.dest != Dest::Text) {
            return;
        }
        let bytes = if p.raw.is_empty() { p.hex } else { p.raw };
        if p.kind.is_none() || p.kind == Some("other") || bytes.is_empty() {
            lack!(self.sink);
            return;
        }
        let px = |goal: u32, plain: u32, scale: u32| -> Option<u32> {
            let base = if goal > 0 { goal / 15 } else { plain };
            (base > 0).then(|| base * scale.clamp(1, 1000) / 100)
        };
        let (w, h) = (px(p.w_goal, p.w, p.scale_x), px(p.h_goal, p.h, p.scale_y));
        let look = self.para_look();
        let in_table = self.intbl;
        self.sink.picture(&bytes, w, h, &look, in_table);
    }

    fn para_look(&mut self) -> ParaLook {
        let mut look = self.para.clone();
        if look.line.is_some_and(|l| l == 240) && look.before.is_none() && look.after.is_none() {
            look.after = Some(0);
        }
        if look.after.is_none() {
            look.after = Some(0);
        }
        look.heading = self.style.and_then(|s| {
            let name = self.style_names.get(&s)?;
            let level = name
                .strip_prefix("heading ")
                .and_then(|l| l.trim().parse::<u32>().ok())
                .or_else(|| self.style_outline.get(&s).map(|o| o + 1))?;
            (1..=3).contains(&level).then_some(level)
        });
        if look.heading.is_none() && self.style.is_none() {
            look.heading = self.outline.map(|o| o + 1).filter(|l| (1..=3).contains(l));
        }
        look.list = self.list_ref();
        look
    }

    fn list_ref(&mut self) -> Option<(&'static str, u32, u32)> {
        let def = self.ls.and_then(|ls| self.overrides.get(&ls)).and_then(|id| self.lists.iter().find(|l| l.id == *id));
        if let (Some(ls), Some(def)) = (self.ls, def) {
            self.pn_key = None;
            let nfc = *def.levels.get(self.ilvl as usize).or(def.levels.first())?;
            let kind = match nfc {
                23 => "bullet",
                255 => return None,
                _ => "ordered",
            };
            return Some((kind, self.ilvl, 1000 + ls.clamp(0, 1_000_000) as u32));
        }
        let (kind, level) = self.pn?;
        let key = *self.pn_key.get_or_insert_with(|| {
            self.next_key += 1;
            self.next_key
        });
        Some((kind, level, key))
    }

    fn end_para(&mut self) {
        self.flush();
        let look = self.para_look();
        if look.list.is_none() || self.pn.is_none() {
            if self.pn.is_none() && self.ls.is_none() {
                self.pn_key = None;
            }
        }
        let in_table = self.intbl;
        self.sink.end_para(&look, in_table);
    }
}

/// A font's character set as a code page.
fn charset_cp(charset: u32, default: u32) -> u32 {
    match charset {
        0 | 2 => 1252,
        1 => default,
        77 => 10000,
        128 => 932,
        129 => 949,
        130 => 1361,
        134 => 936,
        136 => 950,
        161 => 1253,
        162 => 1254,
        163 => 1258,
        177 => 1255,
        178 => 1256,
        186 => 1257,
        204 => 1251,
        222 => 874,
        238 => 1250,
        254 => 437,
        255 => 850,
        _ => default,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docx::Block;

    fn paras(src: &str) -> Vec<String> {
        read(src.as_bytes())
            .unwrap()
            .blocks
            .iter()
            .map(|b| match b {
                Block::Para(p) => p.runs.iter().map(|r| r.t.as_str()).collect::<String>(),
                Block::Table { rows, .. } => format!(
                    "[{}]",
                    rows.iter()
                        .map(|r| r
                            .cells
                            .iter()
                            .map(|c| c
                                .paras
                                .iter()
                                .flat_map(|p| p.runs.iter().map(|r| r.t.as_str()))
                                .collect::<String>())
                            .collect::<Vec<_>>()
                            .join("|"))
                        .collect::<Vec<_>>()
                        .join("/")
                ),
                Block::Image { .. } => "<img>".into(),
                Block::Locked { .. } => "<locked>".into(),
            })
            .collect()
    }

    #[test]
    fn words_runs_and_escapes() {
        let src = r"{\rtf1\ansi\ansicpg1252\deff0{\fonttbl{\f0 Arial;}{\f1\fcharset134 SimSun;}}{\colortbl;\red255\green0\blue0;}
\pard Plain {\b bold} {\i\cf1 red} caf\'e9 \u20013?\u25991? {\f1\'d6\'d0}\par
\pard\qc Center\line next\tab tab\par}";
        let found = read(src.as_bytes()).unwrap();
        assert!(!found.missing);
        let Block::Para(p) = &found.blocks[0] else { panic!() };
        let text: String = p.runs.iter().map(|r| r.t.as_str()).collect();
        assert_eq!(text, "Plain bold red café 中文 中");
        assert!(p.runs.iter().any(|r| r.b && r.t == "bold"));
        assert!(p.runs.iter().any(|r| r.i && r.color.as_deref() == Some("FF0000")));
        let Block::Para(q) = &found.blocks[1] else { panic!() };
        assert_eq!(q.align.as_deref(), Some("center"));
        assert_eq!(q.runs[0].t, "Center\nnext\ttab");
    }

    #[test]
    fn tables_and_what_is_missing() {
        assert_eq!(
            paras(
                r"{\rtf1\ansi \pard before\par \trowd\cellx1000\cellx2000\pard\intbl a\cell b\cell\row \trowd\cellx1000\cellx2000\pard\intbl c\cell d\cell\row \pard after\par}"
            ),
            vec!["before", "[ab/cd]".replace("ab", "a|b").replace("cd", "c|d").as_str(), "after"]
        );
        let r = read(br"{\rtf1\ansi words{\footnote\pard note}\par}").unwrap();
        assert!(r.missing);
        let r = read(br"{\rtf1\ansi {\field{\*\fldinst HYPERLINK http://x}{\fldrslt link}}\par}").unwrap();
        assert!(r.missing);
        assert_eq!(paras(r"{\rtf1\ansi {\field{\*\fldinst DATE}{\fldrslt 2026}}\par}"), vec!["2026"]);
        assert_eq!(paras(r"{\rtf1\ansi {\*\unknown skipped}shown\par}"), vec!["shown"]);
    }

    #[test]
    fn not_rtf_and_cut_short() {
        assert!(read(b"plain text").is_none());
        assert_eq!(paras(r"{\rtf1\ansi cut short {\b bold"), vec!["cut short bold"]);
        let deep = format!("{{\\rtf1 {}x{}}}", "{".repeat(5000), "}".repeat(5000));
        assert!(read(deep.as_bytes()).is_none());
    }
}
