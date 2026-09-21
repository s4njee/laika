//! Embedded preview extraction (no demosaic) + derivative resize.
//!
//! RAW files go through rawler's thumbnail/preview path, which reads the
//! embedded JPEG without touching the mosaic data. Rasters load directly.

use std::{
    io::{Cursor, Read},
    path::Path,
};

use image::DynamicImage;
use rawler::decoders::RawDecodeParams;

use super::is_raw;

/// Long edges written to the derivative cache.
pub const PREVIEW_SMALL: u32 = 512;
pub const PREVIEW_LARGE: u32 = 2048;

/// Enough of the file to cover the review JPEGs used by common cameras,
/// without reading an entire multi-megabyte RAW mosaic from a card.
const EMBEDDED_JPEG_SCAN_LIMIT: u64 = 8 * 1024 * 1024;
const REVIEW_JPEG_SCAN_LIMIT: u64 = 512 * 1024;

/// Find a camera-generated JPEG inside the beginning of a RAW container.
///
/// rawler's thumbnail API decodes the 24 MP `JpgFromRaw` for some Nikon
/// files even though the same NEF carries a 570 px `PreviewImage`. Scanning
/// JPEG SOI markers lets the import dialog select the smallest useful image
/// and turns a multi-second operation into a small sequential read.
fn embedded_jpeg_candidates(
    path: &Path,
    limit: u64,
) -> Option<(Vec<u8>, Vec<(u64, u32, u32, usize)>)> {
    let file = std::fs::File::open(path).ok()?;
    let mut data = Vec::new();
    file.take(limit).read_to_end(&mut data).ok()?;

    let mut candidates = Vec::new();
    for offset in 0..data.len().saturating_sub(3) {
        if data[offset..].starts_with(&[0xff, 0xd8, 0xff]) {
            let reader = image::ImageReader::with_format(
                Cursor::new(&data[offset..]),
                image::ImageFormat::Jpeg,
            );
            if let Ok((width, height)) = reader.into_dimensions() {
                let long = width.max(height);
                // Tiny EXIF thumbnails look poor in the review grid. The
                // upper bound filters accidental marker matches cheaply.
                if (256..=20_000).contains(&long) && width > 0 && height > 0 {
                    candidates.push((width as u64 * height as u64, width, height, offset));
                }
            }
        }
    }
    candidates.sort_unstable();
    Some((data, candidates))
}

fn decode_embedded_candidate(data: &[u8], offset: usize) -> Option<DynamicImage> {
    image::load_from_memory_with_format(&data[offset..], image::ImageFormat::Jpeg).ok()
}

fn embedded_review_image(path: &Path) -> Option<DynamicImage> {
    let (data, candidates) = embedded_jpeg_candidates(path, REVIEW_JPEG_SCAN_LIMIT)
        .filter(|(_, candidates)| !candidates.is_empty())
        .or_else(|| embedded_jpeg_candidates(path, EMBEDDED_JPEG_SCAN_LIMIT))?;
    candidates.into_iter().find_map(|(_, _, _, offset)| {
        image::load_from_memory_with_format(&data[offset..], image::ImageFormat::Jpeg).ok()
    })
}

/// S10: a camera-embedded image at least `edge` px on its long side (the
/// smallest such JPEG, else the largest there is) for culling straight
/// from a card — no RAW decode. `edge = 0` asks for the largest (100%).
pub fn cull_image(path: &Path, edge: u32) -> Result<DynamicImage, String> {
    if is_raw(path) {
        if let Some((data, candidates)) =
            embedded_jpeg_candidates(path, EMBEDDED_JPEG_SCAN_LIMIT).filter(|(_, c)| !c.is_empty())
        {
            let pick = if edge == 0 {
                candidates.iter().rev().collect::<Vec<_>>()
            } else {
                candidates
                    .iter()
                    .filter(|(_, w, h, _)| (*w).max(*h) >= edge)
                    .chain(candidates.iter().rev())
                    .collect::<Vec<_>>()
            };
            if let Some(image) = pick
                .into_iter()
                .find_map(|(_, _, _, offset)| decode_embedded_candidate(&data, *offset))
            {
                return Ok(image);
            }
        }
        return load_preview(path);
    }
    crate::system_image::open_raster(path, (edge > 0).then_some(edge))
}

/// Small JPEG for the import-review grid. Prefer the camera's embedded
/// review image and retain rawler as a compatibility fallback.
pub fn review_jpeg(path: &Path, edge: u32) -> Result<Vec<u8>, String> {
    if is_raw(path) {
        if let Some(image) = embedded_review_image(path) {
            return Ok(derivative_jpeg(&image, edge));
        }
    }
    load_preview(path).map(|image| derivative_jpeg(&image, edge))
}

/// Import derivatives from embedded camera JPEGs. Unlike rawler's Nikon
/// thumbnail path, this never expands the RAW mosaic and decodes only the
/// JPEG sizes needed for the 512 px grid and 2048 px Fit preview.
pub fn import_derivatives(path: &Path) -> Result<(Vec<u8>, Vec<u8>), String> {
    if is_raw(path) {
        if let Some((data, candidates)) = embedded_jpeg_candidates(path, EMBEDDED_JPEG_SCAN_LIMIT) {
            let small = candidates.iter().find_map(|(_, _, _, offset)| {
                decode_embedded_candidate(&data, *offset)
                    .map(|image| derivative_jpeg(&image, PREVIEW_SMALL))
            });
            let large = candidates
                .iter()
                .filter(|(_, width, height, _)| width.max(height) >= &PREVIEW_LARGE)
                .find_map(|(_, _, _, offset)| {
                    decode_embedded_candidate(&data, *offset).map(|image| {
                        derivative_jpeg_filtered(
                            &image,
                            PREVIEW_LARGE,
                            image::imageops::FilterType::Triangle,
                        )
                    })
                });
            if let Some(small) = small {
                // Cameras without a large embedded JPEG still get a usable
                // Fit image; native detail continues to read the original.
                let large = large.unwrap_or_else(|| small.clone());
                return Ok((small, large));
            }
        }
    }
    let image = load_preview(path)?;
    Ok((
        derivative_jpeg(&image, PREVIEW_SMALL),
        derivative_jpeg(&image, PREVIEW_LARGE),
    ))
}

/// Load the best cheap image for `path`: embedded preview for RAW,
/// decoded file for rasters. Never demosaics.
pub fn load_preview(path: &Path) -> Result<DynamicImage, String> {
    if is_raw(path) {
        let params = RawDecodeParams::default();
        rawler::analyze::extract_thumbnail_pixels(path, &params)
            .or_else(|_| rawler::analyze::extract_preview_pixels(path, &params))
            .map_err(|e| format!("preview for {}: {e}", path.display()))
    } else {
        // Previews top out at 2048 px (ImageIO decodes at that size).
        crate::system_image::open_raster(path, Some(PREVIEW_LARGE))
    }
}

/// Resize so the long edge is `edge` px (never upscale), as RGB8 JPEG bytes.
pub fn derivative_jpeg(img: &DynamicImage, edge: u32) -> Vec<u8> {
    derivative_jpeg_filtered(img, edge, image::imageops::FilterType::Lanczos3)
}

fn derivative_jpeg_filtered(
    img: &DynamicImage,
    edge: u32,
    filter: image::imageops::FilterType,
) -> Vec<u8> {
    let (w, h) = (img.width(), img.height());
    let long = w.max(h).max(1);
    let rgb = if long > edge {
        let (nw, nh) = if w >= h {
            (edge, ((h as u64 * edge as u64) / w as u64).max(1) as u32)
        } else {
            (((w as u64 * edge as u64) / h as u64).max(1) as u32, edge)
        };
        img.resize_exact(nw, nh, filter).into_rgb8()
    } else {
        img.to_rgb8()
    };
    let (w, h) = (rgb.width(), rgb.height());
    let mut buf = Vec::new();
    let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 86);
    enc.encode(&rgb.into_raw(), w, h, image::ExtendedColorType::Rgb8)
        .expect("jpeg encode to memory");
    buf
}

/// 48-bin luminance histogram over decoded JPEG bytes (sqrt-normalized,
/// matching the Phase 0 spike's shape).
pub fn histogram48(jpeg: &[u8]) -> [f32; 48] {
    let mut hist = [0f32; 48];
    let Ok(img) = image::load_from_memory(jpeg) else {
        return hist;
    };
    let rgb = img.into_rgb8();
    let w = rgb.width() as usize;
    if w == 0 {
        return hist;
    }
    // Every third pixel of every third row (stride, not per-pixel skips).
    for row in rgb.as_raw().chunks_exact(w * 3).step_by(3) {
        for px in row.chunks_exact(3).step_by(3) {
            let l = 0.0722 * px[2] as f32 + 0.7152 * px[1] as f32 + 0.2126 * px[0] as f32;
            hist[((l / 256.) * 48.).min(47.) as usize] += 1.;
        }
    }
    let max = hist.iter().cloned().fold(1., f32::max);
    for b in &mut hist {
        *b = (*b / max).sqrt();
    }
    hist
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(path: &Path, w: u32, h: u32) {
        let mut img = image::RgbImage::new(w, h);
        for (x, y, p) in img.enumerate_pixels_mut() {
            *p = image::Rgb([(x * 255 / w) as u8, (y * 255 / h) as u8, 128]);
        }
        img.save(path).unwrap();
    }

    #[test]
    fn cull_images_come_from_the_camera_jpeg() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/raw/IMG_5442.NEF");
        if !path.exists() {
            return;
        }
        let t = std::time::Instant::now();
        let fit = cull_image(&path, 2048).expect("loupe image");
        let full = cull_image(&path, 0).expect("100% image");
        // The NEF's full-size embedded JPEG serves both: at least 2048 px
        // for the Loupe, the largest for 100%.
        assert!(
            fit.width().max(fit.height()) >= 2048,
            "{}x{}",
            fit.width(),
            fit.height()
        );
        assert!(full.width() * full.height() >= fit.width() * fit.height());
        assert!(
            t.elapsed().as_secs() < 5,
            "no RAW decode: {:?}",
            t.elapsed()
        );
        // Raster files decode at the requested size.
        let dir = std::env::temp_dir().join(format!("laika-cull-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let jpg = dir.join("a.jpg");
        gradient(&jpg, 3000, 2000);
        assert_eq!(cull_image(&jpg, 0).unwrap().width(), 3000);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn bundled_nef_uses_small_embedded_review() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/raw/IMG_5442.NEF");
        if !path.exists() {
            return;
        }
        let embedded = embedded_review_image(&path).expect("small embedded NEF review");
        assert!(embedded.width().max(embedded.height()) <= 1024);
        assert!(embedded.width().max(embedded.height()) >= 256);
        let jpeg = review_jpeg(&path, PREVIEW_SMALL).expect("embedded NEF review");
        let image = image::load_from_memory(&jpeg).expect("review JPEG decodes");
        assert!(image.width().max(image.height()) <= PREVIEW_SMALL);
        assert!(image.width().max(image.height()) >= 256);
        let (small, large) = import_derivatives(&path).expect("embedded import derivatives");
        let small = image::load_from_memory(&small).expect("small derivative decodes");
        let large = image::load_from_memory(&large).expect("large derivative decodes");
        assert_eq!(small.width().max(small.height()), PREVIEW_SMALL);
        assert_eq!(large.width().max(large.height()), PREVIEW_LARGE);
    }

    #[test]
    fn derivatives_keep_aspect_without_upscale() {
        let dir = std::env::temp_dir().join(format!("laika-prev-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let src = dir.join("wide.jpg");
        gradient(&src, 900, 300);
        let img = load_preview(&src).unwrap();
        assert_eq!((img.width(), img.height()), (900, 300));

        let small = derivative_jpeg(&img, 512);
        let d = image::load_from_memory(&small).unwrap();
        assert_eq!((d.width(), d.height()), (512, 170));

        let noscale = derivative_jpeg(&img, 2048);
        let d = image::load_from_memory(&noscale).unwrap();
        assert_eq!((d.width(), d.height()), (900, 300));

        let h = histogram48(&small);
        assert!(h.iter().any(|&b| b > 0.5));
        assert!((h.iter().cloned().fold(0., f32::max) - 1.0).abs() < 1e-5);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn histogram_empty_on_garbage() {
        assert_eq!(histogram48(b"not a jpeg"), [0.; 48]);
    }

    /// DNG carrying only an embedded thumbnail (no raw data): proves the
    /// preview path reads embedded JPEGs without demosaicing.
    #[test]
    fn dng_embedded_thumbnail_roundtrip() {
        let dir = std::env::temp_dir().join(format!("laika-dng-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.dng");
        let mut gradient = image::RgbImage::new(320, 200);
        for (x, y, p) in gradient.enumerate_pixels_mut() {
            *p = image::Rgb([(x * 255 / 320) as u8, (y * 255 / 200) as u8, 128]);
        }
        let dynimg = DynamicImage::ImageRgb8(gradient);
        {
            let f = std::fs::File::create(&path).unwrap();
            let mut w = rawler::dng::writer::DngWriter::new(f, [1, 4, 0, 0]).unwrap();
            w.thumbnail(&dynimg).unwrap();
            w.close().unwrap();
        }
        assert!(is_raw(&path));
        let back = load_preview(&path).unwrap();
        assert!(
            back.width() >= 100 && back.height() >= 50,
            "{}x{}",
            back.width(),
            back.height()
        );
        let rgb = back.to_rgb8();
        let tl = rgb.get_pixel(8, 8);
        let br = rgb.get_pixel(rgb.width() - 9, rgb.height() - 9);
        assert!(
            br[0] > tl[0] + 40,
            "red gradient survives: {tl:?} -> {br:?}"
        );
        let small = derivative_jpeg(&back, PREVIEW_SMALL);
        let d = image::load_from_memory(&small).unwrap();
        assert!(d.width() <= PREVIEW_SMALL);
        std::fs::remove_dir_all(&dir).ok();
    }
}
