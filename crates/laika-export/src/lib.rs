//! Placeholder: gallery build + wrangler deploy land in Phase 5.

pub mod formats;
pub mod watermark;

pub use formats::{ExportFormat, ExportFormatOpts, embed_xmp_jpeg, encode_pixels};
pub use watermark::{WatermarkMode, WatermarkSpec};

pub fn gallery_dir(slug: &str) -> String {
    format!("~/Pictures/Laika Galleries/{slug}/")
}
