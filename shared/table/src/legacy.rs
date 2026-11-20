//! XLS, XLSB, ODS and FODS, read only: calamine reads the values
//! and the formulas, and they reach the page as the same cells it reads from XLSX. Nothing is ever written back in these
//! formats, and macros are never read, let alone run. FODS (LibreOffice's single-file OpenDocument spreadsheet) is the
//! same XML as an ODS's content.xml, so it goes to calamine's ODS reader inside a package made in memory.
//!
//! The hub's converter reads XLSX and XLSM here too (`xlsx_workbook`). Sheets reads those in its page and never calls it,
//! so calamine's XLSX reader isn't linked into Sheets.
//!
//! A cell is a string as the file holds it, `{ n, fmt }` for a number, or `{ f, c, fmt }` for a formula with the result
//! the file came with. Empty cells are null.

use calamine::{Data, Ods, Range, Reader, Xls, Xlsb, Xlsx, open_workbook_from_rs};
use serde_json::{Value, json};
use std::io::{Cursor, Write};

fn value_of(d: &Data) -> Value {
    match d {
        Data::Empty => Value::Null,
        Data::String(s) => json!(s),
        Data::Int(i) => json!({ "n": i }),
        Data::Float(f) => json!({ "n": f }),
        Data::Bool(b) => json!(if *b { "TRUE" } else { "FALSE" }),
        Data::DateTime(t) => json!({ "n": t.as_f64(), "fmt": if t.is_duration() { "" } else { "date" } }),
        Data::DateTimeIso(s) => match iso_serial(s) {
            Some(n) => json!({ "n": n, "fmt": "date" }),
            None => json!(s),
        },
        Data::DurationIso(s) => json!(s),
        Data::Error(e) => json!(e.to_string()),
    }
}

/// "YYYY-MM-DD" or "YYYY-MM-DDThh:mm:ss" as the day number spreadsheets count from 1899-12-30.
pub(crate) fn iso_serial(s: &str) -> Option<f64> {
    let (date, time) = s.split_once('T').unwrap_or((s, ""));
    let mut p = date.split('-').map(|x| x.parse::<i64>());
    let (y, m, d) = (p.next()?.ok()?, p.next()?.ok()?, p.next()?.ok()?);
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    // days from 1970-01-01 (Howard Hinnant's civil calendar), then the 25569 days to 1899-12-30
    let y2 = if m <= 2 { y - 1 } else { y };
    let era = y2.div_euclid(400);
    let yoe = y2 - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146097 + doe - 719468;
    let mut t = 0.0;
    if !time.is_empty() {
        let mut q = time.trim_end_matches('Z').split(':').map(|x| x.parse::<f64>().unwrap_or(0.0));
        t = (q.next().unwrap_or(0.0) * 3600.0 + q.next().unwrap_or(0.0) * 60.0 + q.next().unwrap_or(0.0)) / 86400.0;
    }
    Some((days + 25569) as f64 + t)
}

/// An ODS formula (`of:=SUM([.A1:.A3];[$Sheet2.B1])`) as the page writes formulas (`=SUM(A1:A3,Sheet2!B1)`).
fn ods_formula(f: &str) -> String {
    let f = f.strip_prefix("of:").or_else(|| f.strip_prefix("msoxl:")).unwrap_or(f);
    let f = f.strip_prefix('=').unwrap_or(f);
    // functions newer than OpenFormula carry Microsoft's prefix (COM.MICROSOFT.CONCAT)
    let f = f.replace("COM.MICROSOFT.", "");
    let mut out = String::from("=");
    let mut chars = f.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            // a named range: $$Revenue
            '$' if chars.peek() == Some(&'$') => {
                chars.next();
            }
            '"' => {
                out.push('"');
                for d in chars.by_ref() {
                    out.push(d);
                    if d == '"' {
                        break;
                    }
                }
            }
            ';' => out.push(','),
            '[' => {
                let mut inner = String::new();
                for d in chars.by_ref() {
                    if d == ']' {
                        break;
                    }
                    inner.push(d);
                }
                out.push_str(&ods_ref(&inner));
            }
            other => out.push(other),
        }
    }
    out
}

fn ods_ref(inner: &str) -> String {
    // each end is [$]sheet.cell or .cell; the sheet may be quoted
    let ends: Vec<(Option<String>, String)> = inner
        .split(':')
        .map(|part| {
            let at = part.rfind('.').unwrap_or(0);
            let (sheet, cell) = part.split_at(at);
            let cell = cell.trim_start_matches('.').to_string();
            let sheet = sheet.trim_start_matches('$');
            (if sheet.is_empty() { None } else { Some(sheet.to_string()) }, cell)
        })
        .collect();
    let sheet = ends.first().and_then(|e| e.0.clone()).map(|s| {
        let bare = s.trim_matches('\'');
        if bare.chars().all(|c| c.is_alphanumeric() || c == '_') { bare.to_string() } else { format!("'{bare}'") }
    });
    let cells: Vec<String> = ends.into_iter().map(|e| e.1).collect();
    match sheet {
        Some(s) => format!("{s}!{}", cells.join(":")),
        None => cells.join(":"),
    }
}

/// calamine 0.38 reads an XLS formula's references to the same sheet correctly, but not to other sheets: PtgRef3d
/// shifts the column by two bits and loses the $ flags, PtgArea3d takes the sheet by the wrong index; and functions
/// newer than the format come out as `User(_xlfn.…)` or `[PtgNameX]`. Such a formula would be wrong if worked out or
/// saved, so it isn't used. Text in quotes doesn't count.
/// A formula that uses one of the workbook's defined names isn't used either: calamine reads the names through the same
/// references.
fn xls_formula_trusted(f: &str, names: &[String]) -> bool {
    let mut outside = String::new();
    let mut quoted = false;
    for c in f.chars() {
        if c == '"' {
            quoted = !quoted;
        } else if !quoted {
            outside.push(c);
        }
    }
    if outside.contains('!')
        || outside.contains("User(")
        || outside.contains("[PtgNameX]")
        || outside.contains("_xlfn.")
        || outside.contains("#REF")
    {
        return false;
    }
    let words: Vec<String> =
        outside.split(|c: char| !(c.is_alphanumeric() || c == '_' || c == '.')).map(str::to_ascii_lowercase).collect();
    !names.iter().any(|n| words.contains(&n.to_ascii_lowercase()))
}

/// An ODS defined name's range (`Sales.$F$2:Sales.$F$13`, `$'My Sheet'.$A$1`) as the page writes ranges.
fn ods_range(r: &str) -> Option<String> {
    let f = ods_formula(&format!("=[{}]", r.trim_start_matches('[').trim_end_matches(']')));
    let f = f.strip_prefix('=')?;
    (f.contains('!') && !f.contains('[')).then(|| f.to_string())
}

fn sheet_json(name: &str, values: Range<Data>, formulas: Option<Range<String>>, ods: bool, names: &[String]) -> Value {
    let mut rows: Vec<Vec<Value>> = Vec::new();
    let put = |rows: &mut Vec<Vec<Value>>, r: usize, c: usize, v: Value| {
        if rows.len() <= r {
            rows.resize(r + 1, Vec::new());
        }
        let row = &mut rows[r];
        if row.len() <= c {
            row.resize(c + 1, Value::Null);
        }
        row[c] = v;
    };
    if let Some((r0, c0)) = values.start() {
        for (r, c, d) in values.cells() {
            let v = value_of(d);
            if !v.is_null() {
                put(&mut rows, r0 as usize + r, c0 as usize + c, v);
            }
        }
    }
    if let Some(f) = formulas {
        if let Some((r0, c0)) = f.start() {
            for (r, c, text) in f.cells() {
                // an XLS formula calamine can't write faithfully stays what the file says it came to: a value
                if text.is_empty() || (!ods && !xls_formula_trusted(text, names)) {
                    continue;
                }
                let (r, c) = (r0 as usize + r, c0 as usize + c);
                let cached = rows.get(r).and_then(|row| row.get(c)).cloned().unwrap_or(Value::Null);
                let fmt = cached.get("fmt").cloned().unwrap_or(json!(""));
                let source = if ods { ods_formula(text) } else { format!("={}", text.trim_start_matches('=')) };
                put(&mut rows, r, c, json!({ "f": source, "c": cached, "fmt": fmt }));
            }
        }
    }
    json!({ "name": name, "rows": rows })
}

/// `formulas`: whether the file's formulas are read at all (XLSB's aren't, see `workbook`).
fn read<R: Reader<Cursor<Vec<u8>>>>(mut book: R, ods: bool, formulas: bool) -> Result<Value, String> {
    let mut sheets = Vec::new();
    let defined = book.defined_names().to_vec();
    let names: Vec<String> = defined.iter().map(|(n, _)| n.clone()).collect();
    // ODS names go into a new workbook with the formulas that use them; XLS names aren't trusted (see above)
    let carried: Vec<Value> = if ods {
        defined.iter().filter_map(|(n, r)| ods_range(r).map(|r| json!([n, r]))).collect()
    } else {
        Vec::new()
    };
    for meta in book.sheets_metadata().to_vec() {
        // chart sheets and dialog sheets have no cells to show
        if !matches!(meta.typ, calamine::SheetType::WorkSheet) {
            continue;
        }
        let values = book.worksheet_range(&meta.name).map_err(|e| format!("{e:?}"))?;
        let formulas = if formulas { book.worksheet_formula(&meta.name).ok() } else { None };
        sheets.push(sheet_json(&meta.name, values, formulas, ods, &names));
    }
    if sheets.is_empty() {
        return Err("no sheets".into());
    }
    Ok(json!({ "sheets": sheets, "names": carried }))
}

fn read_xlsx(mut book: Xlsx<Cursor<Vec<u8>>>) -> Result<Value, String> {
    let mut sheets = Vec::new();
    for meta in book.sheets_metadata().to_vec() {
        if !matches!(meta.typ, calamine::SheetType::WorkSheet) {
            continue;
        }
        let values = book.worksheet_range(&meta.name).map_err(|e| format!("{e:?}"))?;
        let mut sheet = sheet_json(&meta.name, values, None, false, &[]);
        // an XLSX formula is already written as Excel writes it: it goes across as it is, with the value it came to
        if let Ok(f) = book.worksheet_formula(&meta.name) {
            if let (Some((r0, c0)), Some(rows)) = (f.start(), sheet["rows"].as_array_mut()) {
                for (r, c, text) in f.cells() {
                    if text.is_empty() {
                        continue;
                    }
                    let (r, c) = (r0 as usize + r, c0 as usize + c);
                    if rows.len() <= r {
                        rows.resize(r + 1, json!([]));
                    }
                    let row = rows[r].as_array_mut().unwrap();
                    if row.len() <= c {
                        row.resize(c + 1, Value::Null);
                    }
                    let cached = row[c].clone();
                    let fmt = cached.get("fmt").cloned().unwrap_or(json!(""));
                    row[c] = json!({ "f": format!("={}", text.trim_start_matches('=')), "c": cached, "fmt": fmt });
                }
            }
        }
        sheets.push(sheet);
    }
    if sheets.is_empty() {
        return Err("no sheets".into());
    }
    Ok(json!({ "sheets": sheets, "names": [] }))
}

const ODS_MIME: &str = "application/vnd.oasis.opendocument.spreadsheet";

/// Whether these bytes are a FODS: an OpenDocument XML document that says it is a spreadsheet.
pub fn looks_fods(bytes: &[u8]) -> bool {
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(4096)]);
    let head = head.trim_start_matches('\u{feff}').trim_start();
    head.starts_with('<') && head.contains("office:document") && head.contains(&format!("\"{ODS_MIME}\""))
}

/// A FODS as the ODS calamine reads: its XML is the package's content.xml.
fn fods_package(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let xml = bytes.strip_prefix(&[0xEF, 0xBB, 0xBF]).unwrap_or(bytes);
    let stored = zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let mut z = zip::ZipWriter::new(Cursor::new(Vec::with_capacity(xml.len() + 1024)));
    let manifest = r#"<?xml version="1.0" encoding="UTF-8"?><manifest:manifest xmlns:manifest="urn:oasis:names:tc:opendocument:xmlns:manifest:1.0"><manifest:file-entry manifest:full-path="content.xml" manifest:media-type="text/xml"/></manifest:manifest>"#;
    for (name, data) in
        [("mimetype", ODS_MIME.as_bytes()), ("META-INF/manifest.xml", manifest.as_bytes()), ("content.xml", xml)]
    {
        z.start_file(name, stored).map_err(|e| e.to_string())?;
        z.write_all(data).map_err(|e| e.to_string())?;
    }
    Ok(z.finish().map_err(|e| e.to_string())?.into_inner())
}

/// The workbook as the page's cells. `kind` is "xls", "xlsb", "ods" or "fods".
pub fn workbook(kind: &str, bytes: Vec<u8>) -> Result<Value, String> {
    // a damaged file must never take the window down with it
    std::panic::catch_unwind(move || match kind {
        "xls" => {
            read(open_workbook_from_rs::<Xls<_>, _>(Cursor::new(bytes)).map_err(|e| format!("{e:?}"))?, false, true)
        }
        // calamine 0.38 reads XLSB's formulas wrongly: a column's shared formula (=D2*E2 down a column) isn't there at
        // all, and references come out as other columns ($BTRP$2 for $D$2). Every XLSB cell is the value Excel stored.
        "xlsb" => {
            read(open_workbook_from_rs::<Xlsb<_>, _>(Cursor::new(bytes)).map_err(|e| format!("{e:?}"))?, false, false)
        }
        "fods" => read(
            open_workbook_from_rs::<Ods<_>, _>(Cursor::new(fods_package(&bytes)?)).map_err(|e| format!("{e:?}"))?,
            true,
            true,
        ),
        _ => read(open_workbook_from_rs::<Ods<_>, _>(Cursor::new(bytes)).map_err(|e| format!("{e:?}"))?, true, true),
    })
    .unwrap_or_else(|_| Err("panicked".into()))
}

/// An XLSX or XLSM workbook as the page's cells, for the converter: Sheets reads these in its page. calamine reads their
/// formulas as written, so they are trusted like ODS's.
pub fn xlsx_workbook(bytes: Vec<u8>) -> Result<Value, String> {
    // a damaged file must never take the converter down with it
    std::panic::catch_unwind(move || {
        read_xlsx(open_workbook_from_rs::<Xlsx<_>, _>(Cursor::new(bytes)).map_err(|e| format!("{e:?}"))?)
    })
    .unwrap_or_else(|_| Err("panicked".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ods_formulas_read_as_the_page_writes_them() {
        assert_eq!(ods_formula("of:=SUM([.A1:.A3])"), "=SUM(A1:A3)");
        assert_eq!(ods_formula("of:=IF([.B2]>0;\"a;b\";[$Sheet2.C3])"), "=IF(B2>0,\"a;b\",Sheet2!C3)");
        assert_eq!(ods_formula("of:=[$'My Sheet'.A1:.B2]"), "='My Sheet'!A1:B2");
    }

    #[test]
    fn xls_formulas_with_other_sheets_are_not_trusted() {
        let none: [String; 0] = [];
        assert!(xls_formula_trusted("D2*E2", &none));
        assert!(xls_formula_trusted("IF(A1>0,\"yes!\",\"no\")", &none));
        assert!(!xls_formula_trusted("SUM(Sales!$M$2:$M$13)", &none));
        assert!(!xls_formula_trusted("User(_xlfn.CONCAT,A1,B1)", &none));
        assert!(!xls_formula_trusted("SUM(RevenueRange)", &["RevenueRange".to_string()]));
        assert_eq!(
            ods_formula("of:=COM.MICROSOFT.CONCAT([.A1];\"-\")+SUM($$Revenue)"),
            "=CONCAT(A1,\"-\")+SUM(Revenue)"
        );
        assert_eq!(ods_range("Sales.$F$2:Sales.$F$13").as_deref(), Some("Sales!$F$2:$F$13"));
    }

    #[test]
    fn iso_dates_count_from_1899_12_30() {
        assert_eq!(iso_serial("1900-03-01"), Some(61.0));
        assert_eq!(iso_serial("2000-01-02"), Some(36527.0));
        assert_eq!(iso_serial("2000-01-02T12:00:00"), Some(36527.5));
        assert_eq!(iso_serial("not a date"), None);
    }
}

#[cfg(test)]
mod real_files {
    use super::*;

    /// The folder of real workbooks these tests read, where there is one.
    fn files() -> Option<std::path::PathBuf> {
        crate::real_file("../../sheets/test/files")
    }

    fn read(dir: &std::path::Path, name: &str) -> Value {
        let kind = name.rsplit('.').next().unwrap();
        workbook(kind, std::fs::read(dir.join(name)).unwrap()).unwrap()
    }

    #[test]
    fn excel_xls_and_ods_carry_values_and_formulas() {
        // the same Excel workbook as XLS, XLSB and ODS, and the ODS's XML as one FODS document
        let Some(dir) = files() else { return };
        for name in ["excel-old.xls", "excel-binary.xlsb", "excel-saved.ods", "excel-saved.fods"] {
            let m = read(&dir, name);
            let sheets = m["sheets"].as_array().unwrap();
            let names: Vec<&str> = sheets.iter().map(|s| s["name"].as_str().unwrap()).collect();
            assert_eq!(names, ["Sales", "Pivot", "Formulas", "Notes"], "{name}");
            let sales = &sheets[0]["rows"];
            assert_eq!(sales[0][1], "Region", "{name}");
            if name.ends_with(".xlsb") {
                // XLSB's formulas aren't read (see `workbook`): the value Excel stored
                assert_eq!(sales[1][5], json!({ "n": 37.5 }), "{name}");
            } else {
                assert_eq!(sales[1][5]["f"], "=D2*E2", "{name}");
                assert_eq!(sales[1][5]["c"]["n"], 37.5, "{name}");
            }
            assert_eq!(sales[1][0]["fmt"], "date", "{name}");
            assert_eq!(sheets[3]["rows"][3][0], "007", "{name}: a code stays text");
            // a formula on another sheet: the value Excel worked out, and, from XLS, no formula calamine would garble
            let b2 = &sheets[2]["rows"][1][1];
            if name.ends_with(".xls") || name.ends_with(".xlsb") {
                assert_eq!(b2["n"], 153.0, "{name}");
                assert!(b2.get("f").is_none(), "{name}: {b2}");
            } else {
                assert_eq!(b2["f"], "=SUM(Sales!D2:D13)", "{name}");
            }
        }
    }

    #[test]
    fn fods_and_xlsb_are_known_by_their_bytes() {
        let Some(dir) = files() else { return };
        assert!(looks_fods(&std::fs::read(dir.join("excel-saved.fods")).unwrap()));
        assert!(!looks_fods(
            b"<?xml version=\"1.0\"?><office:document office:mimetype=\"application/vnd.oasis.opendocument.text\">"
        ));
        assert!(matches!(
            crate::package::look(&std::fs::read(dir.join("excel-binary.xlsb")).unwrap()),
            crate::package::Package::Xlsb
        ));
        // a FODS cut short gives whatever calamine makes of it, or an error, never a crash; junk is an error
        let _ = workbook("fods", b"<office:document office:mimetype=\"application/vnd.oasis.opendocument.spreadsheet\"><table:table table:name=\"A\"><table:table-row><table:table-cell office:value-type=\"float\" office:value=\"1\">".to_vec());
        assert!(
            workbook("fods", b"\x00\x01 not xml at all <<<".to_vec())
                .map(|m| m["sheets"].as_array().unwrap().is_empty())
                .unwrap_or(true)
        );
        assert!(workbook("xlsb", b"PKnonsense".to_vec()).is_err());
    }

    #[test]
    fn libreoffice_files_open() {
        let Some(dir) = files() else { return };
        for name in [
            "libreoffice/basic-cell-content.ods",
            "libreoffice/formula-across-sheets.ods",
            "libreoffice/chart-import-basic.ods",
            "libreoffice/pivot-table-basic.ods",
            "libreoffice/date-time-functions.ods",
            "libreoffice/formula-reference.xls",
            "libreoffice/embedded-chart.xls",
        ] {
            let m = read(&dir, name);
            assert!(!m["sheets"].as_array().unwrap().is_empty(), "{name}");
        }
        let m = read(&dir, "libreoffice/formula-across-sheets.ods");
        let f: Vec<String> = m["sheets"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|s| {
                s["rows"].as_array().unwrap().iter().flat_map(|r| {
                    r.as_array().unwrap().iter().filter_map(|c| c["f"].as_str().map(str::to_string)).collect::<Vec<_>>()
                })
            })
            .take(6)
            .collect();
        println!("{f:?}");
        assert!(f.iter().any(|x| x.contains('!')), "{f:?}");
    }
}
