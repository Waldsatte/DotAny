//! DBF tables (dBase III and IV, FoxPro, Visual FoxPro, and the attribute table of a shapefile), read only: never
//! written, as XLS isn't. The format is a header, a row of field descriptions and fixed-width records.
//! Text is in the code page the header names (its language driver byte); a file that names none is read the way a CSV
//! is (valid UTF-8, or chardetng's guess), which is how the many GBK files from old Chinese systems come out right.
//! Memo fields live in a second file beside the table (.dbt or .fpt); the caller reads it when it is there
//! (`memo_beside`), and without it a memo cell is empty.
//!
//! Cells are what `legacy` gives the page: a string, `{ n }` for a number, `{ n, fmt: "date" }` for a date, "TRUE" or
//! "FALSE" for a logical field, null when empty. The first row is the field names, marked as the header.

use crate::text;
use encoding_rs::Encoding;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// The most a cell holds: Excel's own limit, so whatever is shown can be saved as XLSX.
pub const CELL_CHARS: usize = 32767;

const VERSIONS: [u8; 14] = [0x02, 0x03, 0x04, 0x05, 0x30, 0x31, 0x32, 0x43, 0x63, 0x83, 0x8B, 0xCB, 0xF5, 0xFB];

fn foxpro(version: u8) -> bool {
    matches!(version, 0x30 | 0x31 | 0x32 | 0xF5 | 0xFB)
}

fn visual_foxpro(version: u8) -> bool {
    matches!(version, 0x30 | 0x31 | 0x32)
}

struct Field {
    name: Vec<u8>,
    kind: u8,
    len: usize,
    /// Visual FoxPro's own columns (_NullFlags): never shown
    system: bool,
}

struct Table {
    version: u8,
    records: usize,
    header_len: usize,
    record_len: usize,
    driver: u8,
    fields: Vec<Field>,
}

fn u16le(b: &[u8], at: usize) -> usize {
    u16::from_le_bytes([b[at], b[at + 1]]) as usize
}

fn u32le(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]])
}

fn header(b: &[u8]) -> Option<Table> {
    if b.len() < 33 || !VERSIONS.contains(&b[0]) {
        return None;
    }
    // the date of the last change: some writers leave it at zero, none writes a thirteenth month
    if b[2] > 12 || b[3] > 31 {
        return None;
    }
    let (records, header_len, record_len) = (u32le(b, 4) as usize, u16le(b, 8), u16le(b, 10));
    if header_len < 33 || header_len > b.len() || record_len < 1 {
        return None;
    }
    let mut fields = Vec::new();
    let mut at = 32;
    let mut width = 1;
    loop {
        if at >= header_len {
            return None;
        }
        if b[at] == 0x0D {
            break;
        }
        if at + 32 > header_len {
            return None;
        }
        let d = &b[at..at + 32];
        let name: Vec<u8> = d[..11].iter().take_while(|c| **c != 0).copied().collect();
        let kind = d[11].to_ascii_uppercase();
        let len = match kind {
            // a character field longer than 255 keeps the high byte where the decimals would be (Clipper, FoxPro)
            b'C' => d[16] as usize | (d[17] as usize) << 8,
            _ => d[16] as usize,
        };
        if !kind.is_ascii_graphic() || len == 0 {
            return None;
        }
        fields.push(Field { name, kind, len, system: visual_foxpro(b[0]) && d[18] & 0x01 != 0 });
        width += len;
        at += 32;
    }
    // the fields must fill the record exactly, after its one-byte deleted mark
    if fields.is_empty() || width != record_len {
        return None;
    }
    Some(Table { version: b[0], records, header_len, record_len, driver: b[29], fields })
}

/// Whether these bytes are a DBF table.
pub fn looks(bytes: &[u8]) -> bool {
    header(bytes).is_some()
}

// the upper halves of the two DOS code pages encoding_rs doesn't carry (from Python's cp437 and cp850 codecs)
const CP437: &str = "\u{c7}\u{fc}\u{e9}\u{e2}\u{e4}\u{e0}\u{e5}\u{e7}\u{ea}\u{eb}\u{e8}\u{ef}\u{ee}\u{ec}\u{c4}\u{c5}\u{c9}\u{e6}\u{c6}\u{f4}\u{f6}\u{f2}\u{fb}\u{f9}\u{ff}\u{d6}\u{dc}\u{a2}\u{a3}\u{a5}\u{20a7}\u{192}\u{e1}\u{ed}\u{f3}\u{fa}\u{f1}\u{d1}\u{aa}\u{ba}\u{bf}\u{2310}\u{ac}\u{bd}\u{bc}\u{a1}\u{ab}\u{bb}\u{2591}\u{2592}\u{2593}\u{2502}\u{2524}\u{2561}\u{2562}\u{2556}\u{2555}\u{2563}\u{2551}\u{2557}\u{255d}\u{255c}\u{255b}\u{2510}\u{2514}\u{2534}\u{252c}\u{251c}\u{2500}\u{253c}\u{255e}\u{255f}\u{255a}\u{2554}\u{2569}\u{2566}\u{2560}\u{2550}\u{256c}\u{2567}\u{2568}\u{2564}\u{2565}\u{2559}\u{2558}\u{2552}\u{2553}\u{256b}\u{256a}\u{2518}\u{250c}\u{2588}\u{2584}\u{258c}\u{2590}\u{2580}\u{3b1}\u{df}\u{393}\u{3c0}\u{3a3}\u{3c3}\u{b5}\u{3c4}\u{3a6}\u{398}\u{3a9}\u{3b4}\u{221e}\u{3c6}\u{3b5}\u{2229}\u{2261}\u{b1}\u{2265}\u{2264}\u{2320}\u{2321}\u{f7}\u{2248}\u{b0}\u{2219}\u{b7}\u{221a}\u{207f}\u{b2}\u{25a0}\u{a0}";
const CP850: &str = "\u{c7}\u{fc}\u{e9}\u{e2}\u{e4}\u{e0}\u{e5}\u{e7}\u{ea}\u{eb}\u{e8}\u{ef}\u{ee}\u{ec}\u{c4}\u{c5}\u{c9}\u{e6}\u{c6}\u{f4}\u{f6}\u{f2}\u{fb}\u{f9}\u{ff}\u{d6}\u{dc}\u{f8}\u{a3}\u{d8}\u{d7}\u{192}\u{e1}\u{ed}\u{f3}\u{fa}\u{f1}\u{d1}\u{aa}\u{ba}\u{bf}\u{ae}\u{ac}\u{bd}\u{bc}\u{a1}\u{ab}\u{bb}\u{2591}\u{2592}\u{2593}\u{2502}\u{2524}\u{c1}\u{c2}\u{c0}\u{a9}\u{2563}\u{2551}\u{2557}\u{255d}\u{a2}\u{a5}\u{2510}\u{2514}\u{2534}\u{252c}\u{251c}\u{2500}\u{253c}\u{e3}\u{c3}\u{255a}\u{2554}\u{2569}\u{2566}\u{2560}\u{2550}\u{256c}\u{a4}\u{f0}\u{d0}\u{ca}\u{cb}\u{c8}\u{131}\u{cd}\u{ce}\u{cf}\u{2518}\u{250c}\u{2588}\u{2584}\u{a6}\u{cc}\u{2580}\u{d3}\u{df}\u{d4}\u{d2}\u{f5}\u{d5}\u{b5}\u{fe}\u{de}\u{da}\u{db}\u{d9}\u{fd}\u{dd}\u{af}\u{b4}\u{ad}\u{b1}\u{2017}\u{be}\u{b6}\u{a7}\u{f7}\u{b8}\u{b0}\u{a8}\u{b7}\u{b9}\u{b3}\u{b2}\u{25a0}\u{a0}";

/// How the bytes become text: a WHATWG encoding, or one of the two DOS code pages.
#[derive(Clone, Copy)]
enum Code {
    Enc(&'static Encoding),
    Dos(&'static str),
}

impl Code {
    fn text(self, b: &[u8]) -> String {
        match self {
            Code::Enc(e) => e.decode_without_bom_handling(b).0.into_owned(),
            Code::Dos(table) => b
                .iter()
                .map(|&c| if c < 0x80 { c as char } else { table.chars().nth(c as usize - 0x80).unwrap_or('\u{fffd}') })
                .collect(),
        }
    }
}

/// The language driver byte (dBase, FoxPro and Esri use the same numbers) as a code page.
fn code_of_driver(driver: u8) -> Option<Code> {
    let label: &str = match driver {
        0x01 | 0x09 | 0x0B | 0x0D | 0x0F | 0x11 | 0x15 | 0x18 | 0x19 | 0x1B => return Some(Code::Dos(CP437)),
        0x02 | 0x0A | 0x0E | 0x10 | 0x12 | 0x14 | 0x16 | 0x1A | 0x1D | 0x25 | 0x37 | 0x40 => {
            return Some(Code::Dos(CP850));
        }
        0x03 | 0x57 | 0x58 | 0x59 => "windows-1252",
        0x04 => "macintosh",
        0x26 | 0x65 => "ibm866",
        0x13 | 0x7B => "shift_jis",
        0x4D | 0x7A => "gbk",
        0x4E | 0x79 => "euc-kr",
        0x4F | 0x78 => "big5",
        0x50 | 0x7C => "windows-874",
        0x7D => "windows-1255",
        0x7E => "windows-1256",
        0x96 => "x-mac-cyrillic",
        0xC8 => "windows-1250",
        0xC9 => "windows-1251",
        0xCA => "windows-1254",
        0xCB => "windows-1253",
        0xCC => "windows-1257",
        _ => return None,
    };
    Encoding::for_label(label.as_bytes()).map(Code::Enc)
}

fn trim(b: &[u8]) -> &[u8] {
    let end = b.iter().rposition(|c| *c != b' ' && *c != 0).map_or(0, |i| i + 1);
    let start = b[..end].iter().position(|c| *c != b' ').unwrap_or(end);
    &b[start..end]
}

/// Text no longer than a cell holds.
pub(crate) fn capped(s: String) -> String {
    match s.char_indices().nth(CELL_CHARS) {
        Some((at, _)) => s[..at].to_string(),
        None => s,
    }
}

fn number(b: &[u8]) -> Value {
    let t = String::from_utf8_lossy(trim(b)).replace(',', ".");
    // an empty numeric field, or one that overflowed its width (written as stars)
    match t.parse::<f64>() {
        Ok(n) if n.is_finite() => json!({ "n": n }),
        _ => Value::Null,
    }
}

fn date(b: &[u8]) -> Value {
    let t = trim(b);
    if t.len() != 8 || !t.iter().all(u8::is_ascii_digit) {
        return if t.is_empty() { Value::Null } else { json!(String::from_utf8_lossy(t)) };
    }
    let s = std::str::from_utf8(t).unwrap_or("");
    match crate::legacy::iso_serial(&format!("{}-{}-{}", &s[..4], &s[4..6], &s[6..])) {
        Some(n) if n > 0.0 => json!({ "n": n, "fmt": "date" }),
        _ => json!(s),
    }
}

/// A memo's text in the file beside the table: FoxPro's .fpt or dBase's .dbt.
fn memo_text(memo: &[u8], block: usize, version: u8) -> Option<Vec<u8>> {
    if block == 0 || memo.len() < 512 {
        return None;
    }
    if foxpro(version) {
        let size = (u16::from_be_bytes([memo[6], memo[7]]) as usize).max(1);
        let at = block.checked_mul(size)?;
        let head = memo.get(at..at.checked_add(8)?)?;
        // type 1 is text; 0 a picture and 2 an OLE object, which have nothing to show
        if u32::from_be_bytes([head[0], head[1], head[2], head[3]]) != 1 {
            return None;
        }
        let len = u32::from_be_bytes([head[4], head[5], head[6], head[7]]) as usize;
        return Some(memo.get(at + 8..(at + 8).saturating_add(len).min(memo.len()))?.to_vec());
    }
    // dBase IV: blocks of the size its header says, each starting FF FF 08 00 and its length; dBase III: 512 bytes,
    // the text running to an end-of-file mark
    let size = match u16le(memo, 20) {
        n if n > 0 && matches!(version, 0x8B | 0xCB) => n,
        _ => 512,
    };
    let rest = memo.get(block.checked_mul(size)?..)?;
    if rest.starts_with(&[0xFF, 0xFF, 0x08, 0x00]) && rest.len() >= 8 {
        let len = (u32le(rest, 4) as usize).saturating_sub(8);
        return Some(rest[8..8usize.saturating_add(len).min(rest.len())].to_vec());
    }
    let end = rest.iter().position(|c| *c == 0x1A).unwrap_or(rest.len());
    Some(rest[..end].to_vec())
}

/// The file beside a table that holds its memo fields, if there is one: table.fpt or table.dbt, in any case.
pub fn memo_beside(path: &Path) -> Option<PathBuf> {
    let dir = path.parent()?;
    let stem = path.file_stem()?.to_string_lossy().to_lowercase();
    std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).find(|p| {
        let ext = p.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
        (ext == "fpt" || ext == "dbt")
            && p.file_stem().is_some_and(|s| s.to_string_lossy().to_lowercase() == stem)
            && p.is_file()
    })
}

/// The table as the page's cells, one sheet called `name`. `memo` is the file beside it (`memo_beside`), if any.
pub fn workbook(bytes: &[u8], memo: Option<&[u8]>, name: &str) -> Result<Value, String> {
    // a damaged file must never take the window down with it
    std::panic::catch_unwind(|| read(bytes, memo, name)).unwrap_or_else(|_| Err("panicked".into()))
}

fn read(b: &[u8], memo: Option<&[u8]>, name: &str) -> Result<Value, String> {
    let t = header(b).ok_or("not a DBF table")?;
    // a file cut short keeps the records it has
    let records = t.records.min(b.len().saturating_sub(t.header_len) / t.record_len);
    let record = |i: usize| &b[t.header_len + i * t.record_len..t.header_len + (i + 1) * t.record_len];
    let code = code_of_driver(t.driver).unwrap_or_else(|| {
        // no code page named: guessed from the names and the text fields, as a CSV's encoding is found
        let mut sample: Vec<u8> = t.fields.iter().flat_map(|f| f.name.iter().copied().chain([b' '])).collect();
        'all: for i in 0..records {
            let mut at = 1;
            for f in &t.fields {
                if matches!(f.kind, b'C' | b'V') {
                    sample.extend_from_slice(trim(&record(i)[at..at + f.len]));
                    sample.push(b' ');
                }
                at += f.len;
            }
            if sample.len() > 65536 {
                break 'all;
            }
        }
        Code::Enc(Encoding::for_label(text::decode(&sample, None).enc.as_bytes()).unwrap_or(encoding_rs::UTF_8))
    });
    let mut rows: Vec<Vec<Value>> = Vec::with_capacity(records + 1);
    rows.push(t.fields.iter().filter(|f| !f.system).map(|f| json!(code.text(&f.name))).collect());
    for i in 0..records {
        let r = record(i);
        // a deleted record stays in the file until the table is packed; dBase doesn't show it
        if r[0] == b'*' {
            continue;
        }
        let mut row = Vec::with_capacity(t.fields.len());
        let mut at = 1;
        for f in &t.fields {
            let raw = &r[at..at + f.len];
            at += f.len;
            if !f.system {
                row.push(cell(f, raw, &t, memo, code));
            }
        }
        while row.last().is_some_and(Value::is_null) {
            row.pop();
        }
        rows.push(row);
    }
    Ok(json!({ "sheets": [{ "name": name, "rows": rows, "header": true }], "names": [] }))
}

fn cell(f: &Field, raw: &[u8], t: &Table, memo: Option<&[u8]>, code: Code) -> Value {
    match f.kind {
        b'C' | b'V' => {
            let s = trim(raw);
            if s.is_empty() { Value::Null } else { json!(capped(code.text(s))) }
        }
        b'N' | b'F' => number(raw),
        b'D' => date(raw),
        b'L' => match raw.first().map(u8::to_ascii_uppercase) {
            Some(b'T' | b'Y') => json!("TRUE"),
            Some(b'F' | b'N') => json!("FALSE"),
            _ => Value::Null,
        },
        // whole numbers, and Visual FoxPro's autoincrement
        b'I' | b'+' if f.len == 4 => json!({ "n": i32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]) }),
        // currency: an integer in ten-thousandths
        b'Y' if f.len == 8 => json!({ "n": i64::from_le_bytes(raw.try_into().unwrap_or([0; 8])) as f64 / 10000.0 }),
        // Visual FoxPro's double; in dBase IV and later B is a binary memo
        b'B' if visual_foxpro(t.version) && f.len == 8 => match f64::from_le_bytes(raw.try_into().unwrap_or([0; 8])) {
            n if n.is_finite() => json!({ "n": n }),
            _ => Value::Null,
        },
        // dBase 7's double, stored so that it sorts as bytes
        b'O' if f.len == 8 => {
            let mut x: [u8; 8] = raw.try_into().unwrap_or([0; 8]);
            if x[0] & 0x80 != 0 {
                x[0] &= 0x7F;
            } else {
                x.iter_mut().for_each(|c| *c = !*c);
            }
            match f64::from_be_bytes(x) {
                n if n.is_finite() && raw.iter().any(|c| *c != 0) => json!({ "n": n }),
                _ => Value::Null,
            }
        }
        // FoxPro's date and time: a Julian day and milliseconds since midnight (day 2415019 is 1899-12-30)
        b'T' if f.len == 8 => match (u32le(raw, 0) as i64, u32le(raw, 4) as f64) {
            (0, _) => Value::Null,
            (day, ms) => json!({ "n": (day - 2_415_019) as f64 + ms / 86_400_000.0, "fmt": "date" }),
        },
        b'M' => {
            let block = if f.len == 4 {
                u32le(raw, 0) as usize
            } else {
                std::str::from_utf8(trim(raw)).ok().and_then(|s| s.parse().ok()).unwrap_or(0)
            };
            match memo.and_then(|m| memo_text(m, block, t.version)) {
                Some(b) => {
                    let s = code.text(trim(&b));
                    let s = s.trim_end_matches(['\u{1a}', '\0']);
                    if s.is_empty() { Value::Null } else { json!(capped(s.to_string())) }
                }
                None => Value::Null,
            }
        }
        // pictures, OLE objects, binary memos: nothing a cell can show
        _ => Value::Null,
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A DBF table as the format describes it, from fields (name, type, length, decimals) and rows of raw bytes.
    pub(crate) fn table(version: u8, driver: u8, fields: &[(&str, u8, u8, u8)], rows: &[(&[u8], bool)]) -> Vec<u8> {
        let record_len = 1 + fields.iter().map(|f| f.2 as usize).sum::<usize>();
        // Visual FoxPro's header has 263 more bytes after the fields, for a database's path
        let header_len = 32 + 32 * fields.len() + 1 + if visual_foxpro(version) { 263 } else { 0 };
        let mut b = vec![version, 126, 11, 14];
        b.extend((rows.len() as u32).to_le_bytes());
        b.extend((header_len as u16).to_le_bytes());
        b.extend((record_len as u16).to_le_bytes());
        b.resize(29, 0);
        b.push(driver);
        b.resize(32, 0);
        for (name, kind, len, dec) in fields {
            let mut d = name.as_bytes().to_vec();
            d.resize(11, 0);
            d.push(*kind);
            d.resize(16, 0);
            d.extend([*len, *dec]);
            d.resize(32, 0);
            b.extend(d);
        }
        b.push(0x0D);
        b.resize(header_len, 0);
        for (r, deleted) in rows {
            b.push(if *deleted { b'*' } else { b' ' });
            assert_eq!(r.len(), record_len - 1);
            b.extend_from_slice(r);
        }
        b.push(0x1A);
        b
    }

    #[test]
    fn fields_become_cells() {
        let b = table(
            0x03,
            0x03,
            &[
                ("NAME", b'C', 6, 0),
                ("QTY", b'N', 3, 0),
                ("PRICE", b'N', 8, 2),
                ("DAY", b'D', 8, 0),
                ("OK", b'L', 1, 0),
            ],
            &[
                (b"Lamp    3  12.50 20261103T", false),
                (b"Gone    1   1.00 20260101F", true),
                (b"      ***                ?", false),
            ],
        );
        assert!(looks(&b));
        let m = workbook(&b, None, "stock").unwrap();
        let s = &m["sheets"][0];
        assert_eq!(s["name"], "stock");
        assert_eq!(s["header"], true);
        assert_eq!(s["rows"][0], json!(["NAME", "QTY", "PRICE", "DAY", "OK"]));
        assert_eq!(s["rows"][1], json!(["Lamp", { "n": 3.0 }, { "n": 12.5 }, { "n": 46329.0, "fmt": "date" }, "TRUE"]));
        // the deleted record isn't shown; overflowed and empty fields are empty cells
        assert_eq!(s["rows"].as_array().unwrap().len(), 3);
        assert_eq!(s["rows"][2], json!([]));
    }

    #[test]
    fn code_pages_named_or_guessed() {
        let gbk = |s: &str| {
            let mut r = encoding_rs::GBK.encode(s).0.into_owned();
            r.resize(10, b' ');
            r
        };
        // 0x7A names GBK
        let m = workbook(&table(0x03, 0x7A, &[("CITY", b'C', 10, 0)], &[(&gbk("北京市"), false)]), None, "t").unwrap();
        assert_eq!(m["sheets"][0]["rows"][1][0], "北京市");
        // no driver: guessed from the text
        let many: Vec<Vec<u8>> = ["北京市", "上海市", "广州市", "深圳市", "杭州市", "南京市", "成都市", "武汉市"]
            .iter()
            .map(|c| gbk(c))
            .collect();
        let rows: Vec<(&[u8], bool)> = many.iter().map(|r| (r.as_slice(), false)).collect();
        let m = workbook(&table(0x03, 0, &[("CITY", b'C', 10, 0)], &rows), None, "t").unwrap();
        assert_eq!(m["sheets"][0]["rows"][4][0], "深圳市");
        // DOS Western Europe, which encoding_rs doesn't carry
        let m = workbook(
            &table(0x03, 0x02, &[("WORD", b'C', 5, 0)], &[(&[b'C', 0x82, b'p', b'e', b' '], false)]),
            None,
            "t",
        )
        .unwrap();
        assert_eq!(m["sheets"][0]["rows"][1][0], "Cépe");
    }

    #[test]
    fn not_a_table() {
        assert!(!looks(b"just some text that is long enough to have a header of thirty-two bytes"));
        assert!(workbook(b"\x03\x01\x01\x01", None, "t").is_err());
        // the fields don't add up to the record
        let mut b = table(0x03, 0x03, &[("A", b'C', 4, 0)], &[(b"abcd", false)]);
        b[10] = 9;
        assert!(!looks(&b));
        // cut short: the records that are there
        let b = table(0x03, 0x03, &[("A", b'C', 4, 0)], &[(b"abcd", false), (b"efgh", false)]);
        let m = workbook(&b[..b.len() - 6], None, "t").unwrap();
        assert_eq!(m["sheets"][0]["rows"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn foxpro_memo_and_binary_fields() {
        // a FoxPro 2 table with a memo in block 1 of an .fpt with 64-byte blocks
        let b = table(0xF5, 0x03, &[("ID", b'N', 3, 0), ("NOTE", b'M', 10, 0)], &[(b"  1         1", false)]);
        let mut fpt = vec![0u8; 512];
        fpt[6..8].copy_from_slice(&64u16.to_be_bytes());
        fpt[64..68].copy_from_slice(&1u32.to_be_bytes());
        fpt[68..72].copy_from_slice(&5u32.to_be_bytes());
        fpt[72..77].copy_from_slice(b"hello");
        let m = workbook(&b, Some(&fpt), "t").unwrap();
        assert_eq!(m["sheets"][0]["rows"][1], json!([{ "n": 1.0 }, "hello"]));
        // without the memo file the cell is empty
        let m = workbook(&b, None, "t").unwrap();
        assert_eq!(m["sheets"][0]["rows"][1], json!([{ "n": 1.0 }]));
        // Visual FoxPro: integer, currency, double, date-time
        let mut r = Vec::new();
        r.extend(42i32.to_le_bytes());
        r.extend(1_234_500i64.to_le_bytes());
        r.extend(2.5f64.to_le_bytes());
        r.extend(2_461_348u32.to_le_bytes());
        r.extend(43_200_000u32.to_le_bytes());
        let b = table(
            0x30,
            0x03,
            &[("I", b'I', 4, 0), ("Y", b'Y', 8, 4), ("B", b'B', 8, 0), ("T", b'T', 8, 0)],
            &[(&r, false)],
        );
        let m = workbook(&b, None, "t").unwrap();
        assert_eq!(
            m["sheets"][0]["rows"][1],
            json!([{ "n": 42 }, { "n": 123.45 }, { "n": 2.5 }, { "n": 46329.5, "fmt": "date" }])
        );
    }

    #[test]
    fn sheets_sample_files() {
        // a dBase table in GBK, and a FoxPro table with its memo file
        let Some(dir) = crate::real_file("../../sheets/test/files") else { return };
        let gbk = std::fs::read(dir.join("dbase-gbk.dbf")).unwrap();
        let m = workbook(&gbk, None, "dbase-gbk").unwrap();
        let rows = &m["sheets"][0]["rows"];
        assert_eq!(rows[0], json!(["CODE", "NAME", "KIND", "QTY", "PRICE", "BOUGHT", "INSTOCK"]));
        assert_eq!(
            rows[1],
            json!(["A001", "电热水壶", "厨房电器", { "n": 3.0 }, { "n": 129.0 }, { "n": 46027.0, "fmt": "date" }, "TRUE"])
        );
        assert_eq!(rows[5][1], "地毯（羊毛）");
        assert_eq!(rows.as_array().unwrap().len(), 7, "the deleted record isn't shown");
        assert_eq!(rows[6], json!(["C021", "置物架", "家居", { "n": 7.0 }, { "n": 88.0 }]));
        let path = dir.join("foxpro-memo.dbf");
        let memo = std::fs::read(memo_beside(&path).unwrap()).unwrap();
        let m = workbook(&std::fs::read(&path).unwrap(), Some(&memo), "foxpro-memo").unwrap();
        let rows = &m["sheets"][0]["rows"];
        assert_eq!(
            rows[1],
            json!([{ "n": 1.0 }, "Müller GmbH", { "n": 1520.5 }, "Delivered on time. Customer asked for an invoice."])
        );
        assert_eq!(rows[2][3], "Café order — paid in cash.");
        assert_eq!(rows[3], json!([{ "n": 3.0 }, "Smith & Co", { "n": 0.0 }]));
    }

    #[test]
    fn long_text_is_cut_to_a_cell() {
        assert_eq!(capped("ab".repeat(20000)).chars().count(), CELL_CHARS);
        assert_eq!(capped("北京".into()), "北京");
    }
}
