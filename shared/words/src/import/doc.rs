//! Word 97–2003 (.doc), after Microsoft's published [MS-DOC] description: the main document's words through the piece
//! table, paragraphs and runs from the property pages (bold, italic, underline, size, color), alignment, spacing and
//! indents, headings from built-in heading styles, lists, tables, and inline PNG or JPEG pictures. Field results are
//! shown as the words they display. Files from Word 95 and older, and encrypted ones, are refused. Headers and footers
//! with words, footnotes, endnotes, comments, text boxes, floating shapes, other pictures, links, symbol characters,
//! merged and nested table cells and tracked changes count as missing.

use super::{Imported, Look, ParaLook, Sink};
use std::collections::HashMap;
use std::io::{Cursor, Read};

/// Larger streams than this aren't a document anyone wrote by hand.
const MAX_STREAM: u64 = 512 * 1024 * 1024;

fn u16_at(b: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(at..at + 2)?.try_into().ok()?))
}
fn u32_at(b: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(at..at + 4)?.try_into().ok()?))
}
fn i16_at(b: &[u8], at: usize) -> Option<i16> {
    u16_at(b, at).map(|v| v as i16)
}

/// Stream names are compared without case, as the compound file format says.
fn stream(cf: &mut cfb::CompoundFile<Cursor<&[u8]>>, name: &str) -> Option<Vec<u8>> {
    let name = cf
        .read_root_storage()
        .find(|e| e.name().eq_ignore_ascii_case(name.trim_start_matches('/')))?
        .path()
        .to_path_buf();
    let entry = cf.entry(&name).ok()?;
    if !entry.is_stream() || entry.len() > MAX_STREAM {
        return None;
    }
    let mut s = cf.open_stream(&name).ok()?;
    let mut out = Vec::with_capacity(entry.len() as usize);
    s.read_to_end(&mut out).ok()?;
    Some(out)
}

// ---------- the file information block ----------

struct Fib {
    ccp_text: u32,
    ccp_ftn: u32,
    ccp_hdd: u32,
    ccp_atn: u32,
    ccp_edn: u32,
    ccp_txbx: u32,
    pairs: Vec<(u32, u32)>,
    which_table: bool,
}

impl Fib {
    fn parse(w: &[u8]) -> Option<Fib> {
        if u16_at(w, 0)? != 0xA5EC {
            return None;
        }
        // Word 97 and later; Word 6 and 95 wrote a different layout
        if u16_at(w, 2)? < 0x00C1 {
            return None;
        }
        let flags = u16_at(w, 0x0A)?;
        if flags & 0x0100 != 0 || flags & 0x8000 != 0 {
            return None;
        }
        let csw = u16_at(w, 32)? as usize;
        let lw_at = 34 + csw * 2;
        let cslw = u16_at(w, lw_at)? as usize;
        let lw = |i: usize| if i < cslw { u32_at(w, lw_at + 2 + i * 4) } else { Some(0) };
        let pairs_at = lw_at + 2 + cslw * 4;
        let count = u16_at(w, pairs_at)? as usize;
        let mut pairs = Vec::with_capacity(count);
        for i in 0..count {
            pairs.push((u32_at(w, pairs_at + 2 + i * 8)?, u32_at(w, pairs_at + 6 + i * 8)?));
        }
        Some(Fib {
            ccp_text: lw(3)?,
            ccp_ftn: lw(4)?,
            ccp_hdd: lw(5)?,
            ccp_atn: lw(7)?,
            ccp_edn: lw(8)?,
            ccp_txbx: lw(9)?,
            pairs,
            which_table: flags & 0x0200 != 0,
        })
    }

    fn part<'a>(&self, table: &'a [u8], index: usize) -> Option<&'a [u8]> {
        let (fc, lcb) = *self.pairs.get(index)?;
        if lcb == 0 {
            return None;
        }
        table.get(fc as usize..fc as usize + lcb as usize)
    }

    fn has(&self, index: usize) -> bool {
        self.pairs.get(index).is_some_and(|p| p.1 > 0)
    }
}

const STSHF: usize = 1;
const PLCF_SED: usize = 6;
const PLCF_BTE_CHPX: usize = 12;
const PLCF_BTE_PAPX: usize = 13;
const CLX: usize = 33;
const PLC_SPA_MOM: usize = 40;
const PLF_LST: usize = 73;
const PLF_LFO: usize = 74;

// ---------- properties ----------

/// Each property ("sprm") in a list of them, as (operation, operand).
fn sprms(grpprl: &[u8]) -> Vec<(u16, &[u8])> {
    let mut out = Vec::new();
    let mut at = 0;
    while at + 2 <= grpprl.len() {
        let op = u16::from_le_bytes([grpprl[at], grpprl[at + 1]]);
        at += 2;
        let size = match op >> 13 {
            0 | 1 => 1,
            2 | 4 | 5 => 2,
            3 => 4,
            7 => 3,
            _ => {
                if op == 0xD608 || op == 0xD606 {
                    let Some(cb) = u16_at(grpprl, at) else { break };
                    at += 2;
                    (cb as usize).saturating_sub(1)
                } else if op == 0xC615 && grpprl.get(at) == Some(&255) {
                    // tab stops beyond what one byte can count: the size follows from their own counts
                    let del = *grpprl.get(at + 1).unwrap_or(&0) as usize;
                    let add_at = at + 2 + del * 4;
                    let add = *grpprl.get(add_at).unwrap_or(&0) as usize;
                    at += 1;
                    1 + del * 4 + 1 + add * 3
                } else {
                    let Some(&cb) = grpprl.get(at) else { break };
                    at += 1;
                    cb as usize
                }
            }
        };
        let Some(operand) = grpprl.get(at..at + size) else { break };
        out.push((op, operand));
        at += size;
    }
    out
}

#[derive(Clone, Debug, Default)]
struct Chp {
    b: bool,
    i: bool,
    u: bool,
    hps: u32,
    color: Option<String>,
    hidden: bool,
    deleted: bool,
    inserted: bool,
    special: bool,
    ole: bool,
    pic: Option<u32>,
    istd: Option<u16>,
}

const ICO: [&str; 17] = [
    "", "000000", "0000FF", "00FFFF", "00FF00", "FF00FF", "FF0000", "FFFF00", "FFFFFF", "000080", "008080", "008000",
    "800080", "800000", "808000", "808080", "C0C0C0",
];

fn toggle(v: u8, current: bool) -> bool {
    match v {
        0 => false,
        1 => true,
        0x80 => current,
        0x81 => !current,
        _ => current,
    }
}

impl Chp {
    fn apply(&mut self, grpprl: &[u8]) {
        for (op, v) in sprms(grpprl) {
            let b0 = *v.first().unwrap_or(&0);
            match op {
                0x0835 => self.b = toggle(b0, self.b),
                0x0836 => self.i = toggle(b0, self.i),
                0x2A3E => self.u = b0 != 0,
                0x4A43 => self.hps = u16_at(v, 0).unwrap_or(20) as u32,
                0x2A42 => self.color = ICO.get(b0 as usize).filter(|c| !c.is_empty()).map(|c| c.to_string()),
                0x6870 => {
                    self.color = if v.get(3) == Some(&0xFF) {
                        None
                    } else {
                        Some(format!("{:02X}{:02X}{:02X}", v[0], *v.get(1).unwrap_or(&0), *v.get(2).unwrap_or(&0)))
                    };
                }
                0x083C => self.hidden = b0 != 0,
                0x0800 => self.deleted = b0 != 0,
                0x0801 => self.inserted = b0 != 0,
                0x0855 => self.special = b0 != 0,
                0x080A => self.ole = b0 != 0,
                0x6A03 => self.pic = u32_at(v, 0),
                0x4A30 => self.istd = u16_at(v, 0),
                _ => {}
            }
        }
    }

    fn look(&self) -> Look {
        Look {
            b: self.b,
            i: self.i,
            u: self.u,
            sz: Some(self.hps.clamp(2, 3276)),
            color: self.color.clone().filter(|c| c != "000000"),
        }
    }
}

#[derive(Clone, Debug, Default)]
struct Pap {
    jc: Option<u8>,
    line: Option<u32>,
    exact_line: bool,
    before: Option<i32>,
    after: Option<i32>,
    left: Option<i32>,
    right: Option<i32>,
    first: Option<i32>,
    in_table: bool,
    ttp: bool,
    inner_ttp: bool,
    itap: u32,
    ilvl: u8,
    ilfo: i16,
    merged: bool,
}

impl Pap {
    fn apply(&mut self, grpprl: &[u8]) {
        for (op, v) in sprms(grpprl) {
            let b0 = *v.first().unwrap_or(&0);
            let i16v = || i16_at(v, 0).unwrap_or(0) as i32;
            match op {
                0x2403 | 0x2461 => self.jc = Some(b0),
                0x6412 => {
                    let line = i16_at(v, 0).unwrap_or(240);
                    let mult = i16_at(v, 2).unwrap_or(1);
                    self.exact_line = mult == 0;
                    self.line = (mult == 1 && line > 0).then_some(line as u32);
                }
                0xA413 => self.before = Some(u16_at(v, 0).unwrap_or(0) as i32),
                0xA414 => self.after = Some(u16_at(v, 0).unwrap_or(0) as i32),
                0x840F | 0x845E => self.left = Some(i16v()),
                0x840E | 0x845D => self.right = Some(i16v()),
                0x8411 | 0x8460 => self.first = Some(i16v()),
                0x2416 => self.in_table = b0 != 0,
                0x2417 => self.ttp = b0 != 0,
                0x244C => self.inner_ttp = b0 != 0,
                0x6649 => self.itap = u32_at(v, 0).unwrap_or(0),
                0x260A => self.ilvl = b0.min(8),
                0x460B => self.ilfo = i16_at(v, 0).unwrap_or(0),
                0x5624 | 0xD62B => self.merged = true,
                0xD608 => {
                    // the row's cells: any of them merged with another
                    let cells = b0 as usize;
                    let tc_at = 1 + (cells + 1) * 2;
                    for c in 0..cells {
                        let Some(flags) = u16_at(v, tc_at + c * 20) else { break };
                        if flags & 0x0063 != 0 {
                            self.merged = true;
                        }
                    }
                }
                _ => {}
            }
        }
    }
}

// ---------- styles and lists ----------

#[derive(Clone, Default)]
struct Style {
    sti: u16,
    base: Option<u16>,
    papx: Vec<u8>,
    chpx: Vec<u8>,
    name: String,
}

fn read_styles(stsh: &[u8]) -> Vec<Option<Style>> {
    let mut out = Vec::new();
    let Some(cb_stshi) = u16_at(stsh, 0) else { return out };
    let stshi = &stsh[2..(2 + cb_stshi as usize).min(stsh.len())];
    let cstd = u16_at(stshi, 0).unwrap_or(0) as usize;
    let base_size = u16_at(stshi, 2).unwrap_or(10) as usize;
    let mut at = 2 + cb_stshi as usize;
    for _ in 0..cstd.min(4096) {
        let Some(cb) = u16_at(stsh, at) else { break };
        at += 2;
        let std = stsh.get(at..at + cb as usize).unwrap_or(&[]);
        at += cb as usize;
        if std.len() < base_size + 2 {
            out.push(None);
            continue;
        }
        let w0 = u16_at(std, 0).unwrap_or(0);
        let w1 = u16_at(std, 2).unwrap_or(0);
        let w2 = u16_at(std, 4).unwrap_or(0);
        let stk = w1 & 0x000F;
        let base = w1 >> 4;
        let cupx = (w2 & 0x000F) as usize;
        let mut style = Style { sti: w0 & 0x0FFF, base: (base != 0x0FFF).then_some(base), ..Default::default() };
        let cch = u16_at(std, base_size).unwrap_or(0) as usize;
        let name_end = base_size + 2 + cch * 2;
        let units: Vec<u16> = (0..cch).filter_map(|i| u16_at(std, base_size + 2 + i * 2)).collect();
        style.name = String::from_utf16_lossy(&units).to_lowercase();
        let mut u = name_end + 2;
        if u % 2 == 1 {
            u += 1;
        }
        let mut upx = Vec::new();
        for _ in 0..cupx {
            let Some(len) = u16_at(std, u) else { break };
            let start = u + 2;
            let Some(body) = std.get(start..start + len as usize) else { break };
            upx.push(body);
            u = start + len as usize;
            if u % 2 == 1 {
                u += 1;
            }
        }
        match stk {
            1 => {
                style.papx = upx.first().and_then(|p| p.get(2..)).unwrap_or(&[]).to_vec();
                style.chpx = upx.get(1).copied().unwrap_or(&[]).to_vec();
            }
            2 => style.chpx = upx.first().copied().unwrap_or(&[]).to_vec(),
            _ => {}
        }
        out.push(Some(style));
    }
    out
}

/// Each list's levels as number formats, and each list override's list id.
fn read_lists(fib: &Fib, table: &[u8]) -> (HashMap<i32, Vec<u8>>, Vec<i32>) {
    let mut lists = HashMap::new();
    let mut lfos = Vec::new();
    if let Some((fc, lcb)) = fib.pairs.get(PLF_LST).copied().filter(|p| p.1 > 0) {
        let base = fc as usize;
        let count = i16_at(table, base).unwrap_or(0).max(0) as usize;
        let mut lvl_at = base + 2 + count * 28;
        let _ = lcb;
        for i in 0..count.min(4096) {
            let lstf = base + 2 + i * 28;
            let Some(id) = u32_at(table, lstf) else { break };
            let simple = table.get(lstf + 26).is_some_and(|f| f & 1 != 0);
            let levels = if simple { 1 } else { 9 };
            let mut nfcs = Vec::new();
            for _ in 0..levels {
                let Some(&nfc) = table.get(lvl_at + 4) else { break };
                let chpx = *table.get(lvl_at + 24).unwrap_or(&0) as usize;
                let papx = *table.get(lvl_at + 25).unwrap_or(&0) as usize;
                let xst = lvl_at + 28 + chpx + papx;
                let cch = u16_at(table, xst).unwrap_or(0) as usize;
                nfcs.push(nfc);
                lvl_at = xst + 2 + cch * 2;
            }
            lists.insert(id as i32, nfcs);
        }
    }
    if let Some(part) = fib.part(table, PLF_LFO) {
        let count = u32_at(part, 0).unwrap_or(0) as usize;
        for i in 0..count.min(65536) {
            let Some(lsid) = u32_at(part, 4 + i * 16) else { break };
            lfos.push(lsid as i32);
        }
    }
    (lists, lfos)
}

// ---------- where properties are, by file position ----------

struct Spans<T> {
    starts: Vec<u32>,
    ends: Vec<u32>,
    values: Vec<T>,
}

impl<T> Spans<T> {
    fn at(&self, fc: u32) -> Option<&T> {
        let i = self.starts.partition_point(|s| *s <= fc).checked_sub(1)?;
        (fc < self.ends[i]).then(|| &self.values[i])
    }
}

/// The property pages named by a bin table; each gives a range of file positions and its properties.
fn pages<T>(word: &[u8], bte: Option<&[u8]>, read: impl Fn(&[u8], usize) -> Option<T>) -> Spans<T> {
    let mut list: Vec<(u32, u32, T)> = Vec::new();
    if let Some(bte) = bte {
        let n = (bte.len().saturating_sub(4)) / 8;
        for i in 0..n {
            let Some(pn) = u32_at(bte, (n + 1) * 4 + i * 4) else { break };
            let at = (pn & 0x3F_FFFF) as usize * 512;
            let Some(page) = word.get(at..at + 512) else { continue };
            let crun = page[511] as usize;
            for r in 0..crun {
                let (Some(a), Some(b)) = (u32_at(page, r * 4), u32_at(page, r * 4 + 4)) else { break };
                if let Some(v) = read(page, r) {
                    list.push((a, b, v));
                }
            }
        }
    }
    list.sort_by_key(|x| x.0);
    let mut out = Spans { starts: Vec::new(), ends: Vec::new(), values: Vec::new() };
    for (a, b, v) in list {
        out.starts.push(a);
        out.ends.push(b);
        out.values.push(v);
    }
    out
}

fn chpx_of(page: &[u8], r: usize) -> Option<Vec<u8>> {
    let crun = page[511] as usize;
    let off = *page.get((crun + 1) * 4 + r)? as usize * 2;
    if off == 0 {
        return Some(Vec::new());
    }
    let cb = *page.get(off)? as usize;
    Some(page.get(off + 1..off + 1 + cb)?.to_vec())
}

fn papx_of(page: &[u8], r: usize) -> Option<(u16, Vec<u8>)> {
    let crun = page[511] as usize;
    let off = *page.get((crun + 1) * 4 + r * 13)? as usize * 2;
    if off == 0 {
        return Some((0, Vec::new()));
    }
    let cb = *page.get(off)? as usize;
    let (start, len) = if cb == 0 { (off + 2, *page.get(off + 1)? as usize * 2) } else { (off + 1, cb * 2 - 1) };
    let body = page.get(start..start + len)?;
    Some((u16_at(body, 0)?, body.get(2..).unwrap_or(&[]).to_vec()))
}

// ---------- the words ----------

struct Piece {
    cp: u32,
    end: u32,
    fc: u32,
    compressed: bool,
    prm: u16,
}

fn pieces(clx: &[u8]) -> Option<(Vec<Piece>, Vec<Vec<u8>>)> {
    let mut prcs = Vec::new();
    let mut at = 0;
    while at < clx.len() {
        match clx[at] {
            1 => {
                let cb = i16_at(clx, at + 1)?.max(0) as usize;
                prcs.push(clx.get(at + 3..at + 3 + cb)?.to_vec());
                at += 3 + cb;
            }
            2 => {
                let lcb = u32_at(clx, at + 1)? as usize;
                let plc = clx.get(at + 5..at + 5 + lcb)?;
                let n = lcb.saturating_sub(4) / 12;
                let mut out = Vec::with_capacity(n);
                for i in 0..n {
                    let cp = u32_at(plc, i * 4)?;
                    let end = u32_at(plc, i * 4 + 4)?;
                    let pcd = (n + 1) * 4 + i * 8;
                    let raw = u32_at(plc, pcd + 2)?;
                    let compressed = raw & 0x4000_0000 != 0;
                    let fc = raw & 0x3FFF_FFFF;
                    out.push(Piece {
                        cp,
                        end,
                        fc: if compressed { fc / 2 } else { fc },
                        compressed,
                        prm: u16_at(plc, pcd + 6)?,
                    });
                }
                return Some((out, prcs));
            }
            _ => return None,
        }
    }
    None
}

/// Words up to `limit` characters in, each with its file position and the piece's extra properties.
fn words(word: &[u8], pieces: &[Piece], limit: u32) -> Option<(Vec<u16>, Vec<u32>, Vec<usize>)> {
    let mut chars = Vec::new();
    let mut fcs = Vec::new();
    let mut from = Vec::new();
    for (pi, p) in pieces.iter().enumerate() {
        if p.cp >= limit {
            break;
        }
        let n = p.end.min(limit).saturating_sub(p.cp) as usize;
        for k in 0..n {
            let (c, fc) = if p.compressed {
                let fc = p.fc as usize + k;
                (cp1252(*word.get(fc)?), fc as u32)
            } else {
                let fc = p.fc as usize + k * 2;
                (u16_at(word, fc)?, fc as u32)
            };
            chars.push(c);
            fcs.push(fc);
            from.push(pi);
        }
    }
    Some((chars, fcs, from))
}

fn cp1252(b: u8) -> u16 {
    if (0x80..0xA0).contains(&b) {
        let one = [b];
        let (s, _, _) = encoding_rs::WINDOWS_1252.decode(&one);
        s.encode_utf16().next().unwrap_or(b as u16)
    } else {
        b as u16
    }
}

// ---------- reading ----------

/// A whole Word file of a kind this reader doesn't take (Word 95 and older, or encrypted), as opposed to a damaged one.
pub fn is_unsupported_kind(bytes: &[u8]) -> bool {
    let Ok(mut cf) = cfb::CompoundFile::open(Cursor::new(bytes)) else { return false };
    let Some(word) = stream(&mut cf, "/WordDocument") else { return false };
    if u16_at(&word, 0) != Some(0xA5EC) {
        return false;
    }
    let old = u16_at(&word, 2).is_some_and(|n| n < 0x00C1);
    let locked = u16_at(&word, 0x0A).is_some_and(|f| f & 0x0100 != 0 || f & 0x8000 != 0);
    old || locked
}

pub fn read(bytes: &[u8]) -> Option<Imported> {
    let mut cf = cfb::CompoundFile::open(Cursor::new(bytes)).ok()?;
    let word = stream(&mut cf, "/WordDocument")?;
    let fib = Fib::parse(&word)?;
    let table = stream(&mut cf, if fib.which_table { "/1Table" } else { "/0Table" })?;
    let data = stream(&mut cf, "/Data").unwrap_or_default();
    let (pieces, prcs) = pieces(fib.part(&table, CLX)?)?;
    let total = fib.ccp_text.checked_add(fib.ccp_ftn)?.checked_add(fib.ccp_hdd)?;
    let (chars, fcs, from) = words(&word, &pieces, total)?;
    if chars.len() < fib.ccp_text as usize {
        return None;
    }
    let styles = fib.part(&table, STSHF).map(read_styles).unwrap_or_default();
    let (lists, lfos) = read_lists(&fib, &table);
    let chpx = pages(&word, fib.part(&table, PLCF_BTE_CHPX), chpx_of);
    let papx = pages(&word, fib.part(&table, PLCF_BTE_PAPX), papx_of);

    let mut r =
        Doc { styles, lists, lfos, data, sink: Sink::default(), style_chp: HashMap::new(), style_pap: HashMap::new() };
    if fib.ccp_ftn > 0 || fib.ccp_atn > 0 || fib.ccp_edn > 0 || fib.ccp_txbx > 0 || fib.has(PLC_SPA_MOM) {
        lack!(r.sink);
    }
    // headers and footers that say something
    let hdd_from = (fib.ccp_text + fib.ccp_ftn) as usize;
    if fib.ccp_hdd > 0 && visible(&chars[hdd_from.min(chars.len())..]).chars().any(|c| c.is_alphanumeric()) {
        lack!(r.sink);
    }

    let main = fib.ccp_text as usize;
    // where sections end: a 0x0C there is a section mark, which ends a paragraph; anywhere else it is a page break
    let section_marks: std::collections::HashSet<usize> = fib
        .part(&table, PLCF_SED)
        .map(|plc| {
            let n = plc.len().saturating_sub(4) / 16;
            (1..=n).filter_map(|i| u32_at(plc, i * 4)).map(|cp| (cp as usize).saturating_sub(1)).collect()
        })
        .unwrap_or_default();
    let mut fields: Vec<(bool, String)> = Vec::new();
    let mut pending: Vec<u16> = Vec::new();
    let mut pending_look = Look::default();
    // the paragraph the next characters belong to: where it ends, and its style
    let mut para_to: Option<(usize, u16)> = None;
    for at in 0..main {
        let c = chars[at];
        let piece = &pieces[from[at]];
        let prm = prm_grpprl(piece.prm, &prcs);
        // the paragraph this character belongs to: its mark decides
        if c == 0x0C && !section_marks.contains(&at) {
            continue;
        }
        if c == 0x0D || c == 0x07 || c == 0x0C {
            r.flush(&mut pending, &pending_look);
            let mark_end = at;
            let (istd, grpprl) = papx.at(fcs[mark_end]).cloned().unwrap_or((0, Vec::new()));
            let mut pap = r.style_pap(istd);
            pap.apply(&grpprl);
            if let Some(g) = prm {
                pap.apply(g);
            }
            let look = r.para_look(istd, &pap);
            if pap.merged || pap.itap > 1 {
                lack!(r.sink);
            }
            let in_table = pap.in_table || pap.itap > 0;
            match c {
                0x07 if in_table && (pap.ttp || pap.itap == 1 && pap.inner_ttp) => r.sink.end_row(),
                0x07 if in_table => r.sink.end_cell(&look),
                // a section mark on a line of its own is not a paragraph
                0x0C if !r.sink.has_words() => {}
                _ => r.sink.end_para(&look, in_table),
            }
            para_to = None;
            continue;
        }
        // the look of this character
        let (end, istd_para) = *para_to.get_or_insert_with(|| {
            let end = para_end(&chars, at, main);
            (end, papx.at(fcs[end]).map(|p| p.0).unwrap_or(0))
        });
        let mut chp = r.style_chp(istd_para);
        let direct = chpx.at(fcs[at]).cloned().unwrap_or_default();
        if let Some(cs) = Chp::char_style(&direct) {
            r.char_style(cs, &mut chp);
        }
        chp.apply(&direct);
        if let Some(g) = prm {
            chp.apply(g);
        }
        // fields: their code is hidden, their result is shown
        match c {
            0x13 => {
                r.flush(&mut pending, &pending_look);
                fields.push((true, String::new()));
                continue;
            }
            0x14 => {
                r.flush(&mut pending, &pending_look);
                if let Some(f) = fields.last_mut() {
                    f.0 = false;
                    let code = f.1.trim_start().to_uppercase();
                    if code.starts_with("HYPERLINK") || code.starts_with("SYMBOL") {
                        lack!(r.sink);
                    }
                }
                continue;
            }
            0x15 => {
                r.flush(&mut pending, &pending_look);
                fields.pop();
                continue;
            }
            _ => {}
        }
        if let Some(f) = fields.iter_mut().rev().find(|f| f.0) {
            if let Some(ch) = char::from_u32(c as u32) {
                f.1.push(ch);
            }
            continue;
        }
        if chp.deleted || chp.inserted {
            lack!(r.sink);
        }
        if chp.hidden || chp.deleted {
            continue;
        }
        if chp.special {
            match c {
                0x01 if !chp.ole => {
                    r.flush(&mut pending, &pending_look);
                    let (istd, grpprl) = papx.at(fcs[end]).cloned().unwrap_or((0, Vec::new()));
                    let mut pap = r.style_pap(istd);
                    pap.apply(&grpprl);
                    let look = r.para_look(istd, &pap);
                    r.picture(chp.pic, &look, pap.in_table || pap.itap > 0);
                }
                // a field's own markers inside a result, or the page number Word draws
                0x01 | 0x08 | 0x02 | 0x05 | 0x28 | 0x29 | 0x03 | 0x04 => lack!(r.sink),
                _ => {}
            }
            continue;
        }
        let ch = match c {
            0x09 => 0x09,
            0x0B => 0x0A,
            0x1E => 0x2011,
            0x1F | 0x00 => continue,
            c if c < 0x20 => continue,
            c => c,
        };
        let look = chp.look();
        if look != pending_look {
            r.flush(&mut pending, &pending_look);
            pending_look = look;
        }
        pending.push(ch);
    }
    r.flush(&mut pending, &pending_look);
    Some(r.sink.finish(&ParaLook::default()))
}

/// Where the paragraph holding this character ends.
fn para_end(chars: &[u16], at: usize, main: usize) -> usize {
    let mut i = at;
    while i < main && !matches!(chars[i], 0x0D | 0x07) {
        i += 1;
    }
    i.min(main.saturating_sub(1))
}

/// A piece's own properties, when they are a list of them.
fn prm_grpprl(prm: u16, prcs: &[Vec<u8>]) -> Option<&[u8]> {
    if prm & 1 == 1 { prcs.get((prm >> 1) as usize).map(Vec::as_slice) } else { None }
}

/// The words a person sees in a run of characters: no field codes, no control characters.
fn visible(chars: &[u16]) -> String {
    let mut depth_code = 0usize;
    let mut stack: Vec<bool> = Vec::new();
    let mut out = String::new();
    for &c in chars {
        match c {
            0x13 => {
                stack.push(true);
                depth_code += 1;
            }
            0x14 => {
                if let Some(top) = stack.last_mut() {
                    if *top {
                        *top = false;
                        depth_code -= 1;
                    }
                }
            }
            0x15 => {
                if let Some(top) = stack.pop() {
                    if top {
                        depth_code -= 1;
                    }
                }
            }
            c if c < 0x20 => {}
            c if depth_code == 0 => out.extend(char::from_u32(c as u32)),
            _ => {}
        }
    }
    out
}

struct Doc {
    styles: Vec<Option<Style>>,
    lists: HashMap<i32, Vec<u8>>,
    lfos: Vec<i32>,
    data: Vec<u8>,
    sink: Sink,
    style_chp: HashMap<u16, Chp>,
    style_pap: HashMap<u16, Pap>,
}

impl Chp {
    fn char_style(grpprl: &[u8]) -> Option<u16> {
        sprms(grpprl).into_iter().filter(|(op, _)| *op == 0x4A30).last().and_then(|(_, v)| u16_at(v, 0))
    }
}

impl Doc {
    fn chain(&self, istd: u16) -> Vec<&Style> {
        let mut out = Vec::new();
        let mut at = Some(istd);
        while let Some(i) = at {
            if out.len() > 16 {
                break;
            }
            let Some(Some(s)) = self.styles.get(i as usize) else { break };
            out.push(s);
            at = s.base;
        }
        out.reverse();
        out
    }

    fn style_chp(&mut self, istd: u16) -> Chp {
        if let Some(c) = self.style_chp.get(&istd) {
            return c.clone();
        }
        let mut chp = Chp { hps: 20, ..Default::default() };
        for s in self.chain(istd) {
            chp.apply(&s.chpx);
        }
        self.style_chp.insert(istd, chp.clone());
        chp
    }

    fn char_style(&self, istd: u16, chp: &mut Chp) {
        for s in self.chain(istd) {
            chp.apply(&s.chpx);
        }
    }

    fn style_pap(&mut self, istd: u16) -> Pap {
        if let Some(p) = self.style_pap.get(&istd) {
            return p.clone();
        }
        let mut pap = Pap::default();
        for s in self.chain(istd) {
            pap.apply(&s.papx);
        }
        self.style_pap.insert(istd, pap.clone());
        pap
    }

    fn heading_of(&self, istd: u16) -> Option<u32> {
        let s = self.styles.get(istd as usize)?.as_ref()?;
        if (1..=9).contains(&s.sti) {
            return Some(s.sti as u32);
        }
        s.name.strip_prefix("heading ").and_then(|l| l.trim().parse().ok())
    }

    fn para_look(&mut self, istd: u16, pap: &Pap) -> ParaLook {
        let list = if pap.ilfo > 0 {
            let lsid = self.lfos.get(pap.ilfo as usize - 1).copied();
            match lsid.and_then(|id| self.lists.get(&id)) {
                Some(levels) => match levels.get(pap.ilvl as usize).or(levels.first()) {
                    Some(23) => Some(("bullet", pap.ilvl as u32, 1000 + pap.ilfo as u32)),
                    Some(255) => None,
                    Some(_) => Some(("ordered", pap.ilvl as u32, 1000 + pap.ilfo as u32)),
                    None => None,
                },
                None => {
                    lack!(self.sink);
                    None
                }
            }
        } else {
            None
        };
        ParaLook {
            heading: self.heading_of(istd).filter(|l| (1..=3).contains(l)),
            align: match pap.jc {
                Some(1) => Some("center"),
                Some(2) => Some("right"),
                Some(3) | Some(4) => Some("justify"),
                _ => None,
            },
            line: if pap.exact_line { None } else { Some(pap.line.unwrap_or(240)) },
            before: Some(pap.before.unwrap_or(0)),
            after: Some(pap.after.unwrap_or(0)),
            left: pap.left,
            right: pap.right,
            first: pap.first,
            list,
        }
    }

    fn flush(&mut self, pending: &mut Vec<u16>, look: &Look) {
        if pending.is_empty() {
            return;
        }
        let text: String = char::decode_utf16(pending.drain(..)).map(|c| c.unwrap_or('\u{FFFD}')).collect();
        self.sink.text(&text, look);
    }

    /// An inline picture kept in the Data stream: its header, the shape, then the picture itself.
    fn picture(&mut self, at: Option<u32>, look: &ParaLook, in_table: bool) {
        let Some(found) = at.and_then(|fc| picture_at(&self.data, fc as usize)) else {
            lack!(self.sink);
            return;
        };
        let (bytes, w, h) = found;
        self.sink.picture(&bytes, w, h, look, in_table);
    }
}

fn picture_at(data: &[u8], fc: usize) -> Option<(Vec<u8>, Option<u32>, Option<u32>)> {
    let lcb = u32_at(data, fc)? as usize;
    let header = u16_at(data, fc + 4)? as usize;
    let end = fc.checked_add(lcb)?.min(data.len());
    let mm = u16_at(data, fc + 6)?;
    let goal_w = i16_at(data, fc + 0x1C)?.max(0) as u32;
    let goal_h = i16_at(data, fc + 0x1E)?.max(0) as u32;
    let mx = u16_at(data, fc + 0x20)?.max(1) as u32;
    let my = u16_at(data, fc + 0x22)?.max(1) as u32;
    let size = |goal: u32, m: u32| (goal > 0).then(|| goal * m / 1000 / 15);
    let mut at = fc + header;
    if mm == 0x66 {
        at += 1 + *data.get(at)? as usize;
    }
    let region = data.get(at..end)?;
    let bytes = blip_in(region, 0)?;
    Some((bytes, size(goal_w, mx), size(goal_h, my)))
}

/// The first PNG or JPEG picture in a run of drawing records.
fn blip_in(b: &[u8], depth: usize) -> Option<Vec<u8>> {
    if depth > 8 {
        return None;
    }
    let mut at = 0;
    while at + 8 <= b.len() {
        let ver_inst = u16_at(b, at)?;
        let kind = u16_at(b, at + 2)?;
        let len = u32_at(b, at + 4)? as usize;
        let body = b.get(at + 8..(at + 8).checked_add(len)?.min(b.len()))?;
        let inst = ver_inst >> 4;
        match kind {
            0xF007 => {
                let name = *body.get(33).unwrap_or(&0) as usize;
                if let Some(found) = body.get(36 + name..).and_then(|rest| blip_in(rest, depth + 1)) {
                    return Some(found);
                }
            }
            0xF01D | 0xF01E | 0xF02A => {
                let uids = if inst & 1 == 1 { 2 } else { 1 };
                return body.get(16 * uids + 1..).map(<[u8]>::to_vec);
            }
            k if ver_inst & 0x000F == 0x000F && (0xF000..0xF200).contains(&k) => {
                if let Some(found) = blip_in(body, depth + 1) {
                    return Some(found);
                }
            }
            _ => {}
        }
        at += 8 + len;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn property_lists_split_by_their_sizes() {
        // bold on, size 24, a variable one of two bytes, then a cut-off one
        let g = [0x35, 0x08, 0x01, 0x43, 0x4A, 0x18, 0x00, 0x00, 0xC6, 0x02, 0xAA, 0xBB, 0x00, 0x60];
        let s = sprms(&g);
        assert_eq!(s.len(), 3);
        assert_eq!(s[0], (0x0835, &[1u8][..]));
        assert_eq!(s[1], (0x4A43, &[0x18, 0][..]));
        assert_eq!(s[2], (0xC600, &[0xAA, 0xBB][..]));
    }

    #[test]
    fn not_a_word_file() {
        assert!(read(b"not a compound file").is_none());
    }

    #[test]
    fn field_codes_are_not_seen() {
        let s: Vec<u16> = "a\u{13} PAGE \u{14}3\u{15}b".encode_utf16().collect();
        assert_eq!(visible(&s), "a3b");
    }
}
