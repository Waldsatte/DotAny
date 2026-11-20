//! The .docx zip package. Parts we don't change are copied in their compressed form, untouched.

use std::collections::BTreeMap;
use std::io::{Cursor, Read, Write};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

pub fn read_part(package: &[u8], name: &str) -> Option<Vec<u8>> {
    let mut zip = ZipArchive::new(Cursor::new(package)).ok()?;
    let mut file = zip.by_name(name).ok()?;
    let mut out = Vec::with_capacity(file.size() as usize);
    file.read_to_end(&mut out).ok()?;
    Some(out)
}

pub fn part_names(package: &[u8]) -> Option<Vec<String>> {
    let zip = ZipArchive::new(Cursor::new(package)).ok()?;
    Some(zip.file_names().filter_map(|n| n.ok().map(|n| n.into_owned())).collect())
}

/// Encrypted or otherwise unreadable packages come back false.
pub fn is_plain_zip(package: &[u8]) -> bool {
    let Ok(mut zip) = ZipArchive::new(Cursor::new(package)) else { return false };
    for i in 0..zip.len() {
        let Ok(file) = zip.by_index_raw(i) else { return false };
        if file.encrypted() {
            return false;
        }
    }
    true
}

/// A new package: every part keeps its place and its bytes, except the ones given here. New ones go at the end.
pub fn rewrite(package: &[u8], changed: &BTreeMap<String, Vec<u8>>) -> Result<Vec<u8>, String> {
    let mut zip = ZipArchive::new(Cursor::new(package)).map_err(|e| e.to_string())?;
    let mut out = ZipWriter::new(Cursor::new(Vec::with_capacity(package.len() + 4096)));
    let deflate = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for i in 0..zip.len() {
        let raw = zip.by_index_raw(i).map_err(|e| e.to_string())?;
        let name = raw.name().map_err(|e| e.to_string())?.into_owned();
        match changed.get(&name) {
            Some(bytes) => {
                drop(raw);
                out.start_file(name.as_str(), deflate).map_err(|e| e.to_string())?;
                out.write_all(bytes).map_err(|e| e.to_string())?;
            }
            None => out.raw_copy_file(raw).map_err(|e| e.to_string())?,
        }
    }
    let existing: Vec<String> = zip.file_names().filter_map(|n| n.ok().map(|n| n.into_owned())).collect();
    for (name, bytes) in changed {
        if !existing.contains(name) {
            out.start_file(name.as_str(), deflate).map_err(|e| e.to_string())?;
            out.write_all(bytes).map_err(|e| e.to_string())?;
        }
    }
    Ok(out.finish().map_err(|e| e.to_string())?.into_inner())
}

/// A package from nothing, for a new document.
pub fn create(parts: &[(&str, Vec<u8>)]) -> Result<Vec<u8>, String> {
    let mut out = ZipWriter::new(Cursor::new(Vec::new()));
    let deflate = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    for (name, bytes) in parts {
        out.start_file(*name, deflate).map_err(|e| e.to_string())?;
        out.write_all(bytes).map_err(|e| e.to_string())?;
    }
    Ok(out.finish().map_err(|e| e.to_string())?.into_inner())
}
