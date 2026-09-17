//! G19–G21: static gallery sites — manifest and diff, the HTML/CSS/JS
//! renderer, and the incremental build.

pub mod build;
pub mod html;
pub mod manifest;

pub use build::{BuildOpts, BuildProgress, BuildReport, SitePhoto, build, preview_diff};
pub use manifest::{BuildDiff, Manifest};

#[cfg(test)]
mod tests;
