//! The pictures beside this one, in the order a person expects: 2 before 10, ignoring case. The family's
//! (dotany_base::folder), asked about pictures.

use crate::formats;
use std::path::{Path, PathBuf};

pub use dotany_base::folder::natural;

/// Every picture we can open in this file's folder, in order.
pub fn pictures_beside(path: &Path) -> Vec<PathBuf> {
    dotany_base::folder::files_beside(path, formats::allowed_name)
}

/// The next picture along, wrapping at the ends. A file that has just disappeared still has a place: where its name would sort.
pub fn neighbour(path: &Path, step: i32) -> Option<PathBuf> {
    dotany_base::folder::neighbour(path, step, formats::allowed_name)
}

pub fn count_beside(path: &Path) -> usize {
    dotany_base::folder::count_beside(path, formats::allowed_name)
}
