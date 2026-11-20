//! Tables as DotAny Sheets reads and writes them, kept once: Sheets uses this crate, and so does the hub's converter
//! (`shared/convert`), so a fix here reaches both.
//! - `text`: CSV and TSV, which encoding the bytes are in, and writing them back the same way;
//! - `package`: what a ZIP is (a workbook, with or without macros, or a binary one), and writing an XLSX that copies
//!   every part it doesn't rewrite;
//! - `legacy`: XLS, XLSB, ODS and FODS read by calamine into the page's cells;
//! - `dbf`: dBase and FoxPro tables, read by the reader in this crate;
//! - `sqlite`: SQLite databases, a sheet per table, opened so that nothing is ever written.
//!
//! Everything but CSV, TSV and XLSX is only ever read: the page shows it, and saving makes an XLSX (or CSV, TSV).

pub mod dbf;
pub mod legacy;
pub mod package;
pub mod sqlite;
pub mod text;

/// A real file or folder kept outside this crate that a unit test reads (a path from this crate's folder): its path, or
/// None, after saying so, where it is not there. The test then ends without checking anything.
#[cfg(test)]
pub(crate) fn real_file(relative: &str) -> Option<std::path::PathBuf> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
    if path.exists() {
        return Some(path);
    }
    eprintln!("skipped: {} is not here", path.display());
    None
}
