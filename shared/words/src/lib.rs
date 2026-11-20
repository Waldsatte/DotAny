//! Word files as DotAny Text reads and writes them, kept once: Text uses this crate, and so does the hub's converter
//! (`shared/convert`), so a fix here reaches both.
//! - `docx`: Word documents opened into the editor's blocks and saved back, with nothing the editor can't represent
//!   ever lost;
//! - `import`: Word 97–2003 (.doc), RTF and OpenDocument text (.odt), only ever read, each turned into the Word document
//!   Text shows for it.

pub mod docx;
pub mod import;

/// Markdown links to pictures beside the file, read back as file names (dotany-base's `beside`, the same file).
#[path = "../../rust/src/beside.rs"]
#[allow(dead_code)]
mod beside;
