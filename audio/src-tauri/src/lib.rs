pub mod app;
pub mod decode;
pub mod engine;
pub mod export;
pub mod fx;
pub mod platform;
pub mod store;
// what a file is, the old sounds, tracker modules, each format's writer, sample rates and tags: shared with the hub's
// converter (shared/sound), and LAME's and libFLAC's doors with Recorder too (shared/sound-encode). Audio's own code and
// its tests reach them here, by these short names
pub use dotany_sound::{encode, flac, formats, lame, module, oldsound, rate, tags};
// the family's atomic save, as Audio's own code and its tests reach it
pub use dotany_base::atomic;
