//! laika-core: catalog (sqlite), photo model, edit params, xmp, exif, sync queue.
//! Phase 1: AppState-adjacent types live here so later phases can grow the DB
//! without touching UI code. Full SQLite storage lands in Phase 2.

pub mod album;
pub mod apple_photos;
pub mod catalog;
pub mod edit;
pub mod gallery;
pub mod import;
pub mod labels;
pub mod lines;
pub mod logging;
pub mod pairs;
pub mod photo;
pub mod prefs;
pub mod slideshow;
pub mod state;
pub mod sync;
pub mod template;
pub mod timeline;
pub mod upright;
pub mod xmp;
