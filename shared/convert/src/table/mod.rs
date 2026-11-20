//! Tables: xlsx, csv and tsv both ways; xls, xlsb, ods, fods, xlsm, dbf and SQLite databases are only read. As DotAny
//! Sheets does it:
//! - `text` decodes CSV and TSV (the encoding is found the way Sheets finds it), `legacy` reads workbooks with calamine,
//!   `dbf` reads dBase and FoxPro tables and `sqlite` reads databases (a sheet per table); all of them come from the
//!   dotany-table crate (`shared/table`), which Sheets uses too. calamine reads XLSX here as well, where Sheets reads it
//!   in its page. A database is read where it is, never written, and one too large for an XLSX (or for memory:
//!   `sqlite::CONVERT`) is refused as too large, never converted in part;
//! - a new XLSX is written the way Sheets writes one (`freshXlsx` and `cellXml` in `sheets/src/xlsx.js`): every sheet,
//!   values and formulas, dates as dates, no other formatting; `package` (also from dotany-table) writes the zip;
//! - XLSM to XLSX keeps the whole workbook and leaves the macros behind, as Sheets' Save As does (`xlsx.js`);
//! - CSV and TSV hold one sheet: from a workbook it is the first worksheet, its values, as the hub's note says
//!   ("Exports the first sheet only, without formatting.").

use crate::out::Outputs;
use crate::{Reason, Step};
use dotany_table::{dbf, legacy, package, sqlite, text};
use serde_json::Value;
use std::collections::{BTreeMap, HashSet};
use std::io::{Cursor, Read};
use std::path::Path;

/// What a file is by its bytes: a zip workbook, an old Excel file, a FODS, a DBF table, or text.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    Xlsx { macros: bool },
    Xlsb,
    Ods,
    Fods,
    Xls,
    Dbf,
    Text,
    Locked,
}

fn sniff(b: &[u8], ext: &str) -> Kind {
    if b.starts_with(b"PK") {
        return match package::look(b) {
            package::Package::Xlsx { macros } => Kind::Xlsx { macros },
            package::Package::Xlsb => Kind::Xlsb,
            package::Package::Ods => Kind::Ods,
            package::Package::Other => Kind::Text,
        };
    }
    if b.starts_with(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]) {
        // an encrypted workbook of any kind is a compound file with this stream
        let locked = cfb::CompoundFile::open(Cursor::new(b)).map(|c| c.exists("/EncryptionInfo")).unwrap_or(false);
        return if locked { Kind::Locked } else { Kind::Xls };
    }
    if legacy::looks_fods(b) {
        return Kind::Fods;
    }
    // a DBF header is only a few bytes of numbers: believed under its own name only
    if ext == "dbf" && dbf::looks(b) {
        return Kind::Dbf;
    }
    Kind::Text
}

fn is_database(ext: &str) -> bool {
    matches!(ext, "sqlite" | "sqlite3" | "db")
}

/// A database's sheets, or why there are none: one cut to fit is too large to convert.
fn database(p: &Path) -> Result<Book, Reason> {
    let mut head = [0u8; 16];
    crate::open(p)?.read_exact(&mut head).map_err(|_| Reason::Damaged)?;
    // Thumbs.db and the many other .db files that aren't SQLite
    if !sqlite::looks(&head) {
        return Err(Reason::Damaged);
    }
    let m = sqlite::workbook(p, sqlite::CONVERT).map_err(|_| Reason::Damaged)?;
    if m["sheets"].as_array().is_some_and(|s| s.iter().any(|s| s.get("cut").is_some())) {
        return Err(Reason::TooLarge);
    }
    Ok(from_model(m))
}

/// A sheet as Sheets' page holds it: rows of cells, each a string, `{n, fmt}` or `{f, c, fmt}`, or null.
struct Sheet {
    name: String,
    rows: Vec<Vec<Value>>,
}

struct Book {
    sheets: Vec<Sheet>,
    /// [[name, "Sheet!$A$1:$B$2"], …]
    names: Vec<(String, String)>,
}

fn from_model(v: Value) -> Book {
    let sheets = v["sheets"]
        .as_array()
        .map(|a| {
            a.iter()
                .map(|s| Sheet {
                    name: s["name"].as_str().unwrap_or("Sheet").to_string(),
                    rows: s["rows"]
                        .as_array()
                        .map(|r| r.iter().map(|row| row.as_array().cloned().unwrap_or_default()).collect())
                        .unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_default();
    let names = v["names"]
        .as_array()
        .map(|a| a.iter().filter_map(|p| Some((p[0].as_str()?.to_string(), p[1].as_str()?.to_string()))).collect())
        .unwrap_or_default();
    Book { sheets, names }
}

// ---------- CSV and TSV: the delimiter is guessed from the first line; the parser follows Sheets' `parseDelimited` (`sheets/src/sheet.js`) ----------

fn sniff_delimiter(text: &str, tsv_name: bool) -> char {
    if tsv_name {
        return '\t';
    }
    let first = text.chars().take(4000).collect::<String>();
    let first = first.split(['\n', '\r']).next().unwrap_or("");
    let count = |d: char| first.split(d).count();
    [',', '\t', ';'].into_iter().fold(',', |a, b| if count(b) > count(a) { b } else { a })
}

fn parse_delimited(text: &str, d: char) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let (mut cell, mut q, mut quoted) = (String::new(), false, false);
    let mut it = text.chars().peekable();
    while let Some(c) = it.next() {
        if q {
            if c == '"' {
                if it.peek() == Some(&'"') {
                    cell.push('"');
                    it.next();
                } else {
                    q = false;
                }
            } else {
                cell.push(c);
            }
            continue;
        }
        if c == '"' && cell.is_empty() && !quoted {
            q = true;
            quoted = true;
        } else if c == d {
            row.push(std::mem::take(&mut cell));
            quoted = false;
        } else if c == '\r' || c == '\n' {
            if c == '\r' && it.peek() == Some(&'\n') {
                it.next();
            }
            row.push(std::mem::take(&mut cell));
            quoted = false;
            rows.push(std::mem::take(&mut row));
        } else {
            cell.push(c);
        }
    }
    if !cell.is_empty() || quoted || !row.is_empty() {
        row.push(cell);
        rows.push(row);
    }
    rows
}

fn quote_field(s: &str, d: char) -> String {
    if s.contains(d) || s.contains('"') || s.contains('\n') || s.contains('\r') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn write_delimited(rows: &[Vec<String>], d: char) -> String {
    let mut out = String::new();
    for row in rows {
        let line: Vec<String> = row.iter().map(|v| quote_field(v, d)).collect();
        out.push_str(&line.join(&d.to_string()));
        out.push_str("\r\n");
    }
    out
}

// ---------- values as text: what a cell shows, without its formatting ----------

/// A day number counted from 1899-12-30, as a date (and a time, when it has one).
fn date_text(n: f64) -> String {
    let days = n.floor() as i64 - 25569;
    // Howard Hinnant's civil_from_days, the reverse of legacy::iso_serial
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    let secs = ((n - n.floor()) * 86400.0).round() as i64;
    if secs == 0 {
        format!("{y:04}-{m:02}-{d:02}")
    } else {
        format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}", secs / 3600 % 24, secs / 60 % 60, secs % 60)
    }
}

fn number_text(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        // the shortest text that reads back as the same number, as a spreadsheet would show it at full width
        let s = format!("{n}");
        if s.len() > 17 { format!("{n:.15}").trim_end_matches('0').trim_end_matches('.').to_string() } else { s }
    }
}

fn shown(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Object(o) => {
            if o.contains_key("f") {
                return shown(&o["c"]);
            }
            let n = o.get("n").and_then(Value::as_f64).unwrap_or(0.0);
            match o.get("fmt").and_then(Value::as_str) {
                Some("date") => date_text(n),
                _ => number_text(n),
            }
        }
        other => other.to_string(),
    }
}

// ---------- a new XLSX ----------

const CT_XLSX: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml";

fn esc(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => o.push_str("&amp;"),
            '<' => o.push_str("&lt;"),
            '>' => o.push_str("&gt;"),
            '"' => o.push_str("&quot;"),
            // characters XML can't hold at all
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            c => o.push(c),
        }
    }
    o
}

fn col_name(mut i: usize) -> String {
    let mut s = Vec::new();
    i += 1;
    while i > 0 {
        let m = (i - 1) % 26;
        s.push(b'A' + m as u8);
        i = (i - m - 1) / 26;
    }
    s.reverse();
    String::from_utf8(s).unwrap()
}

/// "a number someone typed is a number; '007' and a long ID stay words": /^-?(0|[1-9]\d{0,14})(\.\d+)?$/
fn typed_number(x: &str) -> bool {
    let s = x.strip_prefix('-').unwrap_or(x);
    let (int, frac) = match s.split_once('.') {
        Some((a, b)) => (a, Some(b)),
        None => (s, None),
    };
    let int_ok = int == "0"
        || (!int.is_empty() && int.len() <= 15 && !int.starts_with('0') && int.bytes().all(|b| b.is_ascii_digit()));
    let frac_ok = frac.is_none_or(|f| !f.is_empty() && f.bytes().all(|b| b.is_ascii_digit()));
    int_ok && frac_ok
}

fn fmt_style(x: &Value) -> Option<&'static str> {
    match x.get("fmt").and_then(Value::as_str) {
        Some("date") => Some("1"),
        Some("%") => Some("2"),
        Some(",") => Some("3"),
        _ => None,
    }
}

fn cell_xml(r: &str, x: &Value) -> String {
    let s = fmt_style(x);
    let sa = s.map(|s| format!(" s=\"{s}\"")).unwrap_or_default();
    match x {
        Value::Null => String::new(),
        Value::String(t) if t.is_empty() => String::new(),
        Value::String(t) => {
            if typed_number(t) {
                return format!("<c r=\"{r}\"{sa}><v>{t}</v></c>");
            }
            let sp = if t.starts_with(char::is_whitespace) || t.ends_with(char::is_whitespace) || t.contains('\n') {
                " xml:space=\"preserve\""
            } else {
                ""
            };
            format!("<c r=\"{r}\"{sa} t=\"inlineStr\"><is><t{sp}>{}</t></is></c>", esc(t))
        }
        Value::Object(o) if !o.contains_key("f") => format!(
            "<c r=\"{r}\"{sa}><v>{}</v></c>",
            o.get("n").and_then(Value::as_f64).map(number_text).unwrap_or_default()
        ),
        Value::Object(o) => {
            let f = format!("<f>{}</f>", esc(o["f"].as_str().unwrap_or("").trim_start_matches('=')));
            // the result the file came with; the spreadsheet that opens it works it out again (fullCalcOnLoad)
            match &o["c"] {
                Value::Object(c) => format!(
                    "<c r=\"{r}\"{sa}>{f}<v>{}</v></c>",
                    c.get("n").and_then(Value::as_f64).map(number_text).unwrap_or_default()
                ),
                Value::String(t) if t.starts_with('#') => format!("<c r=\"{r}\"{sa} t=\"e\">{f}<v>{}</v></c>", esc(t)),
                Value::String(t) if t == "TRUE" || t == "FALSE" => {
                    format!("<c r=\"{r}\"{sa} t=\"b\">{f}<v>{}</v></c>", (t == "TRUE") as u8)
                }
                Value::String(t) => format!("<c r=\"{r}\"{sa} t=\"str\">{f}<v>{}</v></c>", esc(t)),
                _ => format!("<c r=\"{r}\"{sa}>{f}</c>"),
            }
        }
        other => format!("<c r=\"{r}\"{sa} t=\"inlineStr\"><is><t>{}</t></is></c>", esc(&other.to_string())),
    }
}

fn sheet_data(rows: &[Vec<Value>]) -> (String, String) {
    let mut out = String::from("<sheetData>");
    let (mut max_r, mut max_c) = (0usize, 0usize);
    for (r, row) in rows.iter().enumerate() {
        let mut cells = String::new();
        for (c, x) in row.iter().enumerate() {
            let xml = cell_xml(&format!("{}{}", col_name(c), r + 1), x);
            if !xml.is_empty() {
                cells.push_str(&xml);
                max_c = max_c.max(c);
            }
        }
        if cells.is_empty() {
            continue;
        }
        max_r = r;
        out.push_str(&format!("<row r=\"{}\">{cells}</row>", r + 1));
    }
    out.push_str("</sheetData>");
    let dim = if max_r > 0 || max_c > 0 { format!("A1:{}{}", col_name(max_c), max_r + 1) } else { "A1".into() };
    (out, dim)
}

fn fresh_xlsx(book: &Book) -> Result<Vec<u8>, String> {
    let mut used = HashSet::new();
    let mut sheet_name = |n: &str| {
        let clean: String = n.chars().map(|c| if "[]:*?/\\".contains(c) { ' ' } else { c }).collect();
        let base: String = clean.trim_matches('\'').chars().take(31).collect::<String>().trim().to_string();
        let base = if base.is_empty() { "Sheet".to_string() } else { base };
        let (mut s, mut k) = (base.clone(), 1);
        while used.contains(&s.to_lowercase()) {
            k += 1;
            s = format!("{} ({k})", base.chars().take(27).collect::<String>());
        }
        used.insert(s.to_lowercase());
        s
    };
    let mut parts: BTreeMap<String, String> = BTreeMap::new();
    let head = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\"?>\n";
    let mut sheets = String::new();
    for (i, s) in book.sheets.iter().enumerate() {
        let (xml, dim) = sheet_data(&s.rows);
        parts.insert(
            format!("xl/worksheets/sheet{}.xml", i + 1),
            format!("{head}<worksheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><dimension ref=\"{dim}\"/><sheetViews><sheetView workbookViewId=\"0\"{}/></sheetViews><sheetFormatPr defaultRowHeight=\"15\"/>{xml}</worksheet>", if i == 0 { " tabSelected=\"1\"" } else { "" }),
        );
        sheets.push_str(&format!(
            "<sheet name=\"{}\" sheetId=\"{}\" r:id=\"rId{}\"/>",
            esc(&sheet_name(&s.name)),
            i + 1,
            i + 1
        ));
    }
    let n = book.sheets.len();
    let overrides: String = (0..n).map(|i| format!("<Override PartName=\"/xl/worksheets/sheet{}.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml\"/>", i + 1)).collect();
    parts.insert("[Content_Types].xml".into(), format!("{head}<Types xmlns=\"http://schemas.openxmlformats.org/package/2006/content-types\"><Default Extension=\"rels\" ContentType=\"application/vnd.openxmlformats-package.relationships+xml\"/><Default Extension=\"xml\" ContentType=\"application/xml\"/><Override PartName=\"/xl/workbook.xml\" ContentType=\"{CT_XLSX}\"/>{overrides}<Override PartName=\"/xl/styles.xml\" ContentType=\"application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml\"/></Types>"));
    parts.insert("_rels/.rels".into(), format!("{head}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\"><Relationship Id=\"rId1\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument\" Target=\"xl/workbook.xml\"/></Relationships>"));
    let names = if book.names.is_empty() {
        String::new()
    } else {
        format!(
            "<definedNames>{}</definedNames>",
            book.names
                .iter()
                .map(|(n, r)| format!("<definedName name=\"{}\">{}</definedName>", esc(n), esc(r)))
                .collect::<String>()
        )
    };
    parts.insert("xl/workbook.xml".into(), format!("{head}<workbook xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\" xmlns:r=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships\"><workbookPr/><bookViews><workbookView/></bookViews><sheets>{sheets}</sheets>{names}<calcPr calcId=\"191029\" fullCalcOnLoad=\"1\"/></workbook>"));
    let rels: String = (0..n).map(|i| format!("<Relationship Id=\"rId{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet\" Target=\"worksheets/sheet{}.xml\"/>", i + 1, i + 1)).collect();
    parts.insert("xl/_rels/workbook.xml.rels".into(), format!("{head}<Relationships xmlns=\"http://schemas.openxmlformats.org/package/2006/relationships\">{rels}<Relationship Id=\"rId{}\" Type=\"http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles\" Target=\"styles.xml\"/></Relationships>", n + 1));
    parts.insert("xl/styles.xml".into(), format!("{head}<styleSheet xmlns=\"http://schemas.openxmlformats.org/spreadsheetml/2006/main\"><numFmts count=\"1\"><numFmt numFmtId=\"164\" formatCode=\"yyyy\\-mm\\-dd\"/></numFmts><fonts count=\"1\"><font><sz val=\"11\"/><name val=\"Calibri\"/><family val=\"2\"/></font></fonts><fills count=\"2\"><fill><patternFill patternType=\"none\"/></fill><fill><patternFill patternType=\"gray125\"/></fill></fills><borders count=\"1\"><border><left/><right/><top/><bottom/><diagonal/></border></borders><cellStyleXfs count=\"1\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\"/></cellStyleXfs><cellXfs count=\"4\"><xf numFmtId=\"0\" fontId=\"0\" fillId=\"0\" borderId=\"0\" xfId=\"0\"/><xf numFmtId=\"164\" fontId=\"0\" fillId=\"0\" borderId=\"0\" xfId=\"0\" applyNumberFormat=\"1\"/><xf numFmtId=\"10\" fontId=\"0\" fillId=\"0\" borderId=\"0\" xfId=\"0\" applyNumberFormat=\"1\"/><xf numFmtId=\"4\" fontId=\"0\" fillId=\"0\" borderId=\"0\" xfId=\"0\" applyNumberFormat=\"1\"/></cellXfs><cellStyles count=\"1\"><cellStyle name=\"Normal\" xfId=\"0\" builtinId=\"0\"/></cellStyles></styleSheet>"));
    package::build(None, &parts, &HashSet::new())
}

// ---------- XLSM to XLSX: macros left behind ----------

/// Every `<tag …/>` element whose text matches `drop`, taken out.
fn without(xml: &str, tag: &str, drop: impl Fn(&str) -> bool) -> String {
    let open = format!("<{tag}");
    let mut out = String::with_capacity(xml.len());
    let mut rest = xml;
    while let Some(at) = rest.find(&open) {
        let after = &rest[at + open.len()..];
        // the element's own name must end here: <Default, not <DefaultX
        if !after.starts_with(|c: char| c.is_whitespace() || c == '/' || c == '>') {
            out.push_str(&rest[..at + open.len()]);
            rest = after;
            continue;
        }
        let Some(end) = rest[at..].find("/>") else { break };
        let el = &rest[at..at + end + 2];
        out.push_str(&rest[..at]);
        if !drop(el) {
            out.push_str(el);
        }
        rest = &rest[at + end + 2..];
    }
    out.push_str(rest);
    out
}

fn drop_macros(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e| e.to_string())?;
    let names: Vec<String> = zip.file_names().filter_map(|n| n.ok().map(|n| n.into_owned())).collect();
    let text = |zip: &mut zip::ZipArchive<Cursor<&[u8]>>, n: &str| -> Result<String, String> {
        let mut s = String::new();
        zip.by_name(n).map_err(|e| e.to_string())?.read_to_string(&mut s).map_err(|e| e.to_string())?;
        Ok(s)
    };
    let mut remove = HashSet::new();
    for n in &names {
        let low = n.to_ascii_lowercase();
        let file = low.rsplit('/').next().unwrap_or("");
        if (low.starts_with("xl/vba") && (low.ends_with(".bin") || low.ends_with(".xml")) && !low[3..].contains('/'))
            || (low.starts_with("xl/_rels/vba") && low.ends_with(".rels"))
        {
            remove.insert(n.clone());
        }
        let _ = file;
    }
    let mut parts = BTreeMap::new();
    let rels = names
        .iter()
        .find(|n| n.eq_ignore_ascii_case("xl/_rels/workbook.xml.rels"))
        .cloned()
        .ok_or("no workbook relationships")?;
    let r = text(&mut zip, &rels)?;
    let vba = ["/vbaProject\"", "/vbaProjectSignature\"", "/vbaProjectSignatureAgile\"", "/vbaProjectSignatureV3\""];
    parts.insert(rels, without(&r, "Relationship", |el| vba.iter().any(|v| el.contains(v))));
    let ct = text(&mut zip, "[Content_Types].xml")?;
    let ct = without(&ct, "Override", |el| el.to_ascii_lowercase().contains("partname=\"/xl/vba"));
    let ct = without(&ct, "Default", |el| el.contains("Extension=\"bin\"") && el.contains("vbaProject"));
    parts.insert(
        "[Content_Types].xml".to_string(),
        ct.replace("application/vnd.ms-excel.sheet.macroEnabled.main+xml", CT_XLSX),
    );
    package::build(Some(bytes), &parts, &remove)
}

// ---------- the conversion ----------

fn read_book(kind: Kind, bytes: Vec<u8>, p: &Path) -> Result<(Book, Option<text::Decoded>), Reason> {
    let tsv_name = crate::ext_of(p) == "tsv";
    let calamine =
        |kind: &str, bytes: Vec<u8>| legacy::workbook(kind, bytes).map(from_model).map_err(|_| Reason::Damaged);
    match kind {
        Kind::Locked => Err(Reason::Locked),
        Kind::Xlsx { .. } => Ok((from_model(legacy::xlsx_workbook(bytes).map_err(|_| Reason::Damaged)?), None)),
        Kind::Ods => Ok((calamine("ods", bytes)?, None)),
        Kind::Fods => Ok((calamine("fods", bytes)?, None)),
        Kind::Xls => Ok((calamine("xls", bytes)?, None)),
        Kind::Xlsb => Ok((calamine("xlsb", bytes)?, None)),
        Kind::Dbf => {
            // its memo fields, when the file that holds them is beside it
            let memo = dbf::memo_beside(p).and_then(|m| std::fs::read(m).ok());
            Ok((
                from_model(dbf::workbook(&bytes, memo.as_deref(), &crate::stem_of(p)).map_err(|_| Reason::Damaged)?),
                None,
            ))
        }
        Kind::Text => {
            if bytes.contains(&0) && !bytes.starts_with(&[0xFF, 0xFE]) && !bytes.starts_with(&[0xFE, 0xFF]) {
                return Err(Reason::Damaged);
            }
            let d = text::decode(&bytes, None);
            let delim = sniff_delimiter(&d.text, tsv_name);
            let rows = parse_delimited(&d.text, delim)
                .into_iter()
                .map(|r| r.into_iter().map(Value::String).collect())
                .collect();
            Ok((Book { sheets: vec![Sheet { name: "Sheet1".into(), rows }], names: Vec::new() }, Some(d)))
        }
    }
}

pub fn convert(p: &Path, target: &str, out: &mut Outputs, step: Step) -> Result<(), Reason> {
    let ext = crate::ext_of(p);
    // a database is read where it is, a table at a time, never all into memory
    if is_database(&ext) {
        step(0.1);
        let book = std::panic::catch_unwind(|| database(p)).unwrap_or(Err(Reason::Damaged))?;
        step(0.6);
        return write(&book, None, target, out, step);
    }
    let bytes = crate::read_all(p)?;
    let kind = sniff(&bytes, &ext);
    // a file named like a workbook that is really something else
    let fits = match ext.as_str() {
        "xlsx" | "xlsm" => matches!(kind, Kind::Xlsx { .. } | Kind::Xlsb | Kind::Locked),
        "xlsb" => matches!(kind, Kind::Xlsb | Kind::Locked),
        "ods" => matches!(kind, Kind::Ods | Kind::Locked),
        "fods" => kind == Kind::Fods,
        "dbf" => kind == Kind::Dbf,
        _ => true,
    };
    if !fits {
        return Err(Reason::Damaged);
    }
    step(0.1);
    if target == "xlsx" {
        if let Kind::Xlsx { .. } = kind {
            // an XLSM: the whole workbook, without its macros
            let made = drop_macros(&bytes).map_err(|_| Reason::Damaged)?;
            step(0.9);
            return out.write("xlsx", &made);
        }
    }
    let (book, decoded) = std::panic::catch_unwind(|| read_book(kind, bytes, p)).unwrap_or(Err(Reason::Damaged))?;
    step(0.6);
    write(&book, decoded, target, out, step)
}

fn write(
    book: &Book,
    decoded: Option<text::Decoded>,
    target: &str,
    out: &mut Outputs,
    step: Step,
) -> Result<(), Reason> {
    // a database with no tables has nothing to write
    if book.sheets.is_empty() {
        return Err(Reason::Damaged);
    }
    let made = match target {
        "xlsx" => fresh_xlsx(book).map_err(|_| Reason::Damaged)?,
        _ => {
            let d = if target == "tsv" { '\t' } else { ',' };
            let first = book.sheets.first().ok_or(Reason::Damaged)?;
            let rows: Vec<Vec<String>> = first.rows.iter().map(|r| r.iter().map(shown).collect()).collect();
            let text = write_delimited(&rows, d);
            // a CSV or TSV keeps the encoding it came in; from a workbook it is UTF-8 with the mark Excel looks for
            match decoded.filter(|d| d.enc != "utf-16le" && d.enc != "utf-16be") {
                Some(src) => text::encode(&text, &src.enc, src.bom)
                    .or_else(|_| text::encode(&text, "utf-8", true))
                    .map_err(|_| Reason::Damaged)?,
                None => text::encode(&text, "utf-8", true).map_err(|_| Reason::Damaged)?,
            }
        }
    };
    step(0.9);
    out.write(target, &made)
}

pub fn estimate(p: &Path, size: u64, target: &str) -> u64 {
    let ext = crate::ext_of(p);
    match (ext.as_str(), target) {
        ("xlsm", "xlsx") => size,
        // a zip of XML holds about a fifth of what its cells say; its first sheet's text is roughly twice the file
        ("xlsx" | "xlsm" | "ods", _) => (size as f64 * 1.6) as u64,
        // XLS keeps its cells uncompressed, about as large as their text
        ("xls", "xlsx") => (size as f64 * 0.35) as u64,
        ("xls", _) => (size as f64 * 0.6) as u64,
        // XLSB's records are about a third the size of the XML they become, zipped alike
        ("xlsb", _) => (size as f64 * 1.3) as u64,
        // FODS is the XML an ODS zips: its cells' text is a small part of it
        ("fods", "xlsx") => (size as f64 * 0.15) as u64 + 2500,
        ("fods", _) => (size as f64 * 0.25) as u64,
        // DBF's fixed-width records, padded with spaces; a database's pages, indexes and free space included
        ("dbf" | "sqlite" | "sqlite3" | "db", "xlsx") => (size as f64 * 0.3) as u64 + 2500,
        ("dbf" | "sqlite" | "sqlite3" | "db", _) => (size as f64 * 0.6) as u64,
        // text into a workbook: each cell's XML, then zipped
        (_, "xlsx") => (size as f64 * 0.45) as u64 + 2500,
        _ => size,
    }
}

/// Words in a text file, in the encoding Sheets finds for it (a BOM, valid UTF-8, or chardetng's guess).
pub fn decode_text(b: &[u8]) -> String {
    text::decode(b, None).text
}
