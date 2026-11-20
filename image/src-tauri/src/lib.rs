pub mod app;
pub mod colour;
pub mod decode;
pub mod encode;
pub mod export;
pub mod folder;
pub mod memory;
pub mod store;
pub mod svg;
// what a file is, what a picture carries besides pixels, the work on pixels, and the PCX and ILBM readers: dotany-picture
// (shared/picture), which the hub's converter uses too. They keep their names here (crate::exif and so on); Image's own
// tests reach the first four as dotany_image_lib::exif and so on
pub use dotany_picture::{exif, formats, metadata, ops};
use dotany_picture::{ilbm, pcx};
// the family's atomic save, as Image's own tests reach it
pub use dotany_base::atomic;
