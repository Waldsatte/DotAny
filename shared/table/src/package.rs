//! Writing XLSX. The page hands over only the parts it rewrote (a changed sheet, the workbook when a sheet's references
//! moved) and the parts that go (calcChain.xml, a macro project); every other part of the file it was opened from is
//! copied as it was, compressed bytes and all. Charts, pivot tables, pictures, styles: none of them is ever rewritten.
//! A new workbook (from CSV, XLS or ODS, or a new sheet) has no file to start from: then the page sends every part.

use std::collections::{BTreeMap, HashSet};
use std::io::{Cursor, Write};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

fn deflated() -> SimpleFileOptions {
    SimpleFileOptions::default().compression_method(CompressionMethod::Deflated)
}

/// The new file. `base` is the file it was opened from, if it was a package.
pub fn build(
    base: Option<&[u8]>,
    parts: &BTreeMap<String, String>,
    remove: &HashSet<String>,
) -> Result<Vec<u8>, String> {
    let mut out = ZipWriter::new(Cursor::new(Vec::new()));
    let mut written = HashSet::new();
    if let Some(base) = base {
        let mut zip = ZipArchive::new(Cursor::new(base)).map_err(|e| e.to_string())?;
        for i in 0..zip.len() {
            let name = zip.by_index_raw(i).map_err(|e| e.to_string())?.name().map_err(|e| e.to_string())?.into_owned();
            if remove.contains(&name) {
                continue;
            }
            // a rewritten part keeps its place in the package
            if let Some(text) = parts.get(&name) {
                out.start_file(name.as_str(), deflated()).map_err(|e| e.to_string())?;
                out.write_all(text.as_bytes()).map_err(|e| e.to_string())?;
            } else {
                let file = zip.by_index_raw(i).map_err(|e| e.to_string())?;
                out.raw_copy_file(file).map_err(|e| e.to_string())?;
            }
            written.insert(name);
        }
    }
    // the rest are new; [Content_Types].xml leads a new package, as Excel writes it
    let mut fresh: Vec<&String> = parts.keys().filter(|k| !written.contains(*k) && !remove.contains(*k)).collect();
    fresh.sort_by_key(|k| (k.as_str() != "[Content_Types].xml", k.as_str() != "_rels/.rels", k.to_string()));
    for name in fresh {
        out.start_file(name.as_str(), deflated()).map_err(|e| e.to_string())?;
        out.write_all(parts[name].as_bytes()).map_err(|e| e.to_string())?;
    }
    Ok(out.finish().map_err(|e| e.to_string())?.into_inner())
}

/// What the page needs to know of a ZIP before it reads one: is it a workbook, and does it carry macros.
pub enum Package {
    Xlsx {
        macros: bool,
    },
    /// Excel's binary workbook: the same package with its parts in binary records (xl/workbook.bin)
    Xlsb,
    Ods,
    Other,
}

pub fn look(bytes: &[u8]) -> Package {
    let Ok(zip) = ZipArchive::new(Cursor::new(bytes)) else { return Package::Other };
    let names: Vec<String> = zip.file_names().filter_map(|n| n.ok().map(|n| n.into_owned())).collect();
    if names.iter().any(|n| n.eq_ignore_ascii_case("xl/workbook.xml")) {
        let macros = names.iter().any(|n| n.to_ascii_lowercase().ends_with("vbaproject.bin"));
        return Package::Xlsx { macros };
    }
    if names.iter().any(|n| n.eq_ignore_ascii_case("xl/workbook.bin")) {
        return Package::Xlsb;
    }
    if names.iter().any(|n| n == "content.xml") && names.iter().any(|n| n == "mimetype") {
        return Package::Ods;
    }
    Package::Other
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;

    fn zip_of(files: &[(&str, &str)]) -> Vec<u8> {
        let mut w = ZipWriter::new(Cursor::new(Vec::new()));
        for (n, t) in files {
            w.start_file(*n, deflated()).unwrap();
            w.write_all(t.as_bytes()).unwrap();
        }
        w.finish().unwrap().into_inner()
    }

    #[test]
    fn untouched_parts_are_copied_exactly_and_rewritten_ones_keep_their_place() {
        let base = zip_of(&[
            ("[Content_Types].xml", "types"),
            ("xl/charts/chart1.xml", "<chart/>"),
            ("xl/worksheets/sheet1.xml", "old"),
            ("xl/calcChain.xml", "chain"),
        ]);
        let parts = BTreeMap::from([("xl/worksheets/sheet1.xml".to_string(), "new".to_string())]);
        let remove = HashSet::from(["xl/calcChain.xml".to_string()]);
        let out = build(Some(&base), &parts, &remove).unwrap();
        let mut a = ZipArchive::new(Cursor::new(&base[..])).unwrap();
        let mut b = ZipArchive::new(Cursor::new(&out[..])).unwrap();
        assert_eq!(b.len(), 3);
        assert_eq!(b.by_index(0).unwrap().name().unwrap(), "[Content_Types].xml");
        assert_eq!(b.by_index(2).unwrap().name().unwrap(), "xl/worksheets/sheet1.xml");
        // the chart's compressed bytes are the same bytes
        let raw = |z: &mut ZipArchive<Cursor<&[u8]>>| {
            let f = z.by_name("xl/charts/chart1.xml").unwrap();
            (f.crc32(), f.compressed_size(), 0)
        };
        let (ca, sa, _) = raw(&mut a);
        let (cb, sb, _) = raw(&mut b);
        assert_eq!((ca, sa), (cb, sb));
        let mut s = String::new();
        b.by_name("xl/worksheets/sheet1.xml").unwrap().read_to_string(&mut s).unwrap();
        assert_eq!(s, "new");
    }
}
