//! Archives as DotAny Archive reads them, kept once: Archive uses this crate, and so does the hub's converter
//! (`shared/convert`), so a fix here reaches both.
//! - `source`: what's inside an archive, and reading it back out entry by entry, as a stream;
//! - `codec`: gzip, bzip2, xz and zstd, around one file, a TAR or a cpio, undone in Rust;
//! - `libarchive`: RAR, LHA, cpio and ar through libarchive's readers, built from vendor/ by build.rs;
//! - `mark`: the system's "came from the internet" mark an archive carries, given to each file written out of it.
//!
//! Everything here only reads archives. Writing ZIP, 7z and TAR is each program's own (Archive's `pack.rs`, the
//! converter's `pack/write.rs`); RAR, LHA, cpio and ar are never written: no code for that is compiled in.

pub mod codec;
mod libarchive;
pub mod mark;
pub mod source;
