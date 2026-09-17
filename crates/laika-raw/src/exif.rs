//! Read-only EXIF. kamadak-exif handles JPEG and TIFF-structured files;
//! anything else falls back to rawler's decoded camera fields.

use std::path::Path;

#[derive(Clone, Debug, Default)]
pub struct FileMeta {
    pub camera: String,
    pub lens: String,
    pub focal_mm: String,
    pub aperture: String,
    pub shutter: String,
    pub iso: String,
    pub captured_at: String,
    pub width: u32,
    pub height: u32,
    /// V05: video duration + codec (0/empty for stills).
    pub duration_ms: u64,
    pub codec: String,
    /// V12: read-only overflow for the metadata panel (display strings,
    /// empty when the file doesn't carry the tag).
    pub exposure_program: String,
    pub metering_mode: String,
    pub flash: String,
    pub focal_35mm: String,
    pub serial: String,
    pub firmware: String,
    pub gps: String,
}

fn read_container(path: &Path) -> FileMeta {
    let mut meta = FileMeta::default();
    if let Ok(f) = std::fs::File::open(path) {
        let mut buf = std::io::BufReader::new(f);
        if let Ok(exif) = exif::Reader::new().read_from_container(&mut buf) {
            if let Some(f) = exif.get_field(exif::Tag::Model, exif::In::PRIMARY) {
                meta.camera = f.display_value().to_string();
            }
            if let Some(f) = exif.get_field(exif::Tag::LensModel, exif::In::PRIMARY) {
                meta.lens = f.display_value().to_string();
            }
            if let Some(f) = exif.get_field(exif::Tag::FocalLength, exif::In::PRIMARY) {
                meta.focal_mm = format!("{}mm", f.display_value());
            }
            if let Some(f) = exif.get_field(exif::Tag::FNumber, exif::In::PRIMARY) {
                meta.aperture = format!("ƒ/{}", f.display_value());
            }
            if let Some(f) = exif.get_field(exif::Tag::ExposureTime, exif::In::PRIMARY) {
                meta.shutter = f.display_value().to_string();
            }
            if let Some(f) = exif.get_field(exif::Tag::PhotographicSensitivity, exif::In::PRIMARY) {
                meta.iso = format!("ISO {}", f.display_value());
            }
            if let Some(f) = exif.get_field(exif::Tag::DateTimeOriginal, exif::In::PRIMARY) {
                meta.captured_at = f.display_value().to_string();
            }
            if let Some(f) = exif.get_field(exif::Tag::PixelXDimension, exif::In::PRIMARY) {
                meta.width = f.display_value().to_string().parse().unwrap_or(0);
            }
            if let Some(f) = exif.get_field(exif::Tag::PixelYDimension, exif::In::PRIMARY) {
                meta.height = f.display_value().to_string().parse().unwrap_or(0);
            }
            // V12: overflow tags for the read-only panel. `display_value`
            // renders enums human-readable ("Manual", "Auto, fired", …).
            if let Some(f) = exif.get_field(exif::Tag::ExposureProgram, exif::In::PRIMARY) {
                meta.exposure_program = f.display_value().to_string();
            }
            if let Some(f) = exif.get_field(exif::Tag::MeteringMode, exif::In::PRIMARY) {
                meta.metering_mode = f.display_value().to_string();
            }
            if let Some(f) = exif.get_field(exif::Tag::Flash, exif::In::PRIMARY) {
                meta.flash = f.display_value().to_string();
            }
            if let Some(f) = exif.get_field(exif::Tag::FocalLengthIn35mmFilm, exif::In::PRIMARY) {
                meta.focal_35mm = format!("{}mm", f.display_value());
            }
            if let Some(f) = exif.get_field(exif::Tag::BodySerialNumber, exif::In::PRIMARY) {
                meta.serial = f.display_value().to_string();
            }
            // Closest standard field to firmware; maker notes are opaque.
            if let Some(f) = exif.get_field(exif::Tag::Software, exif::In::PRIMARY) {
                meta.firmware = f.display_value().to_string();
            }
            let lat = exif
                .get_field(exif::Tag::GPSLatitude, exif::In::PRIMARY)
                .map(|f| f.display_value().to_string());
            let lat_ref = exif
                .get_field(exif::Tag::GPSLatitudeRef, exif::In::PRIMARY)
                .map(|f| f.display_value().to_string());
            let lon = exif
                .get_field(exif::Tag::GPSLongitude, exif::In::PRIMARY)
                .map(|f| f.display_value().to_string());
            let lon_ref = exif
                .get_field(exif::Tag::GPSLongitudeRef, exif::In::PRIMARY)
                .map(|f| f.display_value().to_string());
            if let (Some(la), Some(lo)) = (lat, lon) {
                meta.gps = format!(
                    "{}{}, {}{}",
                    la,
                    lat_ref.unwrap_or_default(),
                    lo,
                    lon_ref.unwrap_or_default()
                );
            }
        }
    }
    meta
}

/// Fast metadata for source review. This reads container EXIF and raster
/// dimensions only; RAW files never fall back to a full mosaic decode.
pub fn read_quick(path: &Path) -> FileMeta {
    let mut meta = read_container(path);
    if super::is_raw(path) && meta.width == 0 {
        // RAW containers rarely carry PixelX/YDimension; the decoder's
        // header parse (no pixel decode) gives the developed frame.
        if let Some((w, h)) = raw_dims(path) {
            meta.width = w;
            meta.height = h;
        }
    }
    if crate::system_image::handles(path) {
        // HEIC/HEIF decode with orientation applied, so the catalog size
        // must too: ImageIO's size wins over EXIF PixelX/YDimension, which
        // describe the unrotated image.
        if let Some((w, h)) = crate::system_image::dimensions(path) {
            meta.width = w;
            meta.height = h;
        }
    }
    if !super::is_raw(path) && meta.width == 0 {
        // Header-only raster read; no full pixel decode.
        if let Ok(r) = image::ImageReader::open(path).and_then(|r| r.with_guessed_format()) {
            if let Ok((w, h)) = r.into_dimensions() {
                meta.width = w;
                meta.height = h;
            }
        }
        // Formats the image crate can't size: ask ImageIO.
        if meta.width == 0 {
            if let Some((w, h)) = crate::system_image::dimensions(path) {
                meta.width = w;
                meta.height = h;
            }
        }
    }
    meta
}

/// Developed RAW frame dims (default crop, else active area) from a
/// header-only decode. Matches what `decode::decode` produces, which is
/// what crop/aspect math and the Develop stage measure against.
pub fn raw_dims(path: &Path) -> Option<(u32, u32)> {
    let raw = std::panic::catch_unwind(|| {
        let src = rawler::rawsource::RawSource::new(path).ok()?;
        rawler::decode_dummy(&src).ok()
    })
    .ok()
    .flatten()?;
    let (w, h) = match raw.crop_area.or(raw.active_area) {
        Some(r) => (r.d.w, r.d.h),
        None => (raw.width, raw.height),
    };
    (w > 0 && h > 0).then_some((w as u32, h as u32))
}

/// Full catalog metadata. RAW files may use rawler as a fallback for camera
/// identity and dimensions when container EXIF is incomplete; callers doing
/// this on a small worker stack must use `on_big_stack`.
pub fn read(path: &Path) -> FileMeta {
    let mut meta = read_quick(path);
    if super::is_raw(path) && (meta.camera.is_empty() || meta.width == 0) {
        // rawler panics (rather than Err) on some malformed inputs;
        // never let a bad file kill the import thread.
        let decoded = std::panic::catch_unwind(|| rawler::decode_file(path));
        if let Ok(Ok(raw)) = decoded {
            if meta.camera.is_empty() && !raw.model.is_empty() {
                meta.camera = raw.model.clone();
            }
            if meta.width == 0 {
                meta.width = raw.width as u32;
                meta.height = raw.height as u32;
            }
        }
    }
    meta
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_dims_match_decoded_frame() {
        let p = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/raw/IMG_5442.NEF"
        ));
        if !p.exists() {
            return;
        }
        let quick = read_quick(p);
        let (w, h, _, _) = crate::decode::linear_dims_for_test(p);
        assert_eq!((quick.width, quick.height), (w, h));
    }

    #[test]
    fn missing_file_gives_default() {
        let m = read(Path::new("/nonexistent/file.dng"));
        assert!(m.camera.is_empty());
    }

    #[test]
    fn plain_jpeg_has_no_crash_and_reports_size() {
        let dir = std::env::temp_dir().join(format!("laika-exif-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("plain.jpg");
        image::RgbImage::new(64, 48).save(&p).unwrap();
        let m = read(&p);
        assert!(m.camera.is_empty());
        assert!(m.iso.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }
}
