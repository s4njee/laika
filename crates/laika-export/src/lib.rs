//! Export encoders, watermarks, and static gallery sites.

pub mod formats;
pub mod site;
pub mod watermark;

pub use formats::{
    ExportFormat, ExportFormatOpts, OutputSharpening, embed_xmp_jpeg, encode_pixels, output_sharpen,
};
pub use watermark::{WatermarkMode, WatermarkSpec};

pub fn gallery_dir(slug: &str) -> String {
    format!("~/Pictures/Laika Galleries/{slug}/")
}
