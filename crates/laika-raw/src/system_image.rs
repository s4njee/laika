//! Formats the `image` crate can't read (HEIC/HEIF), decoded by macOS
//! ImageIO. Pixels come back as 8-bit sRGB (Display P3 and other profiles
//! are color-converted by CoreGraphics) with the file's EXIF orientation
//! applied, so what Laika shows matches Photos and Finder.

use std::path::Path;

/// Extensions routed to the system decoder.
pub const SYSTEM_EXTS: &[&str] = &["heic", "heif", "hif"];

/// Rasters by content, not extension (a `.jpeg` that is really WebP),
/// falling back to ImageIO for anything the `image` crate can't decode.
pub fn open_raster(path: &Path, max_edge: Option<u32>) -> Result<image::DynamicImage, String> {
    if handles(path) {
        return decode_rgb8(path, max_edge).map(image::DynamicImage::ImageRgb8);
    }
    let crate_decode = image::ImageReader::open(path)
        .map_err(|e| format!("open {}: {e}", path.display()))
        .and_then(|r| {
            r.with_guessed_format()
                .map_err(|e| format!("open {}: {e}", path.display()))
        })
        .and_then(|r| {
            r.decode()
                .map_err(|e| format!("decode {}: {e}", path.display()))
        });
    match crate_decode {
        Ok(img) => Ok(img),
        Err(e) if cfg!(target_os = "macos") => decode_rgb8(path, max_edge)
            .map(image::DynamicImage::ImageRgb8)
            .map_err(|_| e),
        Err(e) => Err(e),
    }
}

pub fn handles(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .is_some_and(|e| SYSTEM_EXTS.contains(&e.as_str()))
}

#[cfg(target_os = "macos")]
mod ffi {
    #![allow(non_upper_case_globals, non_snake_case)]
    use std::ffi::c_void;

    pub type CFTypeRef = *const c_void;
    pub type CFStringRef = *const c_void;
    pub type CFDictionaryRef = *const c_void;
    pub type CFURLRef = *const c_void;
    pub type CFNumberRef = *const c_void;
    pub type CFBooleanRef = *const c_void;
    pub type CGImageSourceRef = *const c_void;
    pub type CGImageRef = *const c_void;
    pub type CGColorSpaceRef = *const c_void;
    pub type CGContextRef = *mut c_void;

    #[repr(C)]
    pub struct CGPoint {
        pub x: f64,
        pub y: f64,
    }
    #[repr(C)]
    pub struct CGSize {
        pub width: f64,
        pub height: f64,
    }
    #[repr(C)]
    pub struct CGRect {
        pub origin: CGPoint,
        pub size: CGSize,
    }

    #[repr(C)]
    pub struct CFDictionaryKeyCallBacks {
        _private: [u8; 0],
    }
    #[repr(C)]
    pub struct CFDictionaryValueCallBacks {
        _private: [u8; 0],
    }

    pub const kCFNumberSInt32Type: isize = 3;
    pub const kCGImageAlphaNoneSkipLast: u32 = 5;

    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        pub static kCFBooleanTrue: CFBooleanRef;
        pub static kCFTypeDictionaryKeyCallBacks: CFDictionaryKeyCallBacks;
        pub static kCFTypeDictionaryValueCallBacks: CFDictionaryValueCallBacks;
        pub fn CFRelease(cf: CFTypeRef);
        pub fn CFURLCreateFromFileSystemRepresentation(
            allocator: CFTypeRef,
            buffer: *const u8,
            len: isize,
            is_directory: u8,
        ) -> CFURLRef;
        pub fn CFNumberCreate(
            allocator: CFTypeRef,
            the_type: isize,
            value_ptr: *const c_void,
        ) -> CFNumberRef;
        pub fn CFNumberGetValue(number: CFNumberRef, the_type: isize, value_ptr: *mut c_void)
        -> u8;
        pub fn CFDictionaryCreate(
            allocator: CFTypeRef,
            keys: *const CFTypeRef,
            values: *const CFTypeRef,
            num_values: isize,
            key_callbacks: *const CFDictionaryKeyCallBacks,
            value_callbacks: *const CFDictionaryValueCallBacks,
        ) -> CFDictionaryRef;
        pub fn CFDictionaryGetValue(dict: CFDictionaryRef, key: CFTypeRef) -> CFTypeRef;
    }

    #[link(name = "ImageIO", kind = "framework")]
    unsafe extern "C" {
        pub static kCGImageSourceCreateThumbnailFromImageAlways: CFStringRef;
        pub static kCGImageSourceCreateThumbnailWithTransform: CFStringRef;
        pub static kCGImageSourceThumbnailMaxPixelSize: CFStringRef;
        pub static kCGImagePropertyPixelWidth: CFStringRef;
        pub static kCGImagePropertyPixelHeight: CFStringRef;
        pub static kCGImagePropertyOrientation: CFStringRef;
        pub fn CGImageSourceCreateWithURL(
            url: CFURLRef,
            options: CFDictionaryRef,
        ) -> CGImageSourceRef;
        pub fn CGImageSourceCreateThumbnailAtIndex(
            source: CGImageSourceRef,
            index: usize,
            options: CFDictionaryRef,
        ) -> CGImageRef;
        pub fn CGImageSourceCopyPropertiesAtIndex(
            source: CGImageSourceRef,
            index: usize,
            options: CFDictionaryRef,
        ) -> CFDictionaryRef;
    }

    #[link(name = "CoreGraphics", kind = "framework")]
    unsafe extern "C" {
        pub static kCGColorSpaceSRGB: CFStringRef;
        pub fn CGImageGetWidth(image: CGImageRef) -> usize;
        pub fn CGImageGetHeight(image: CGImageRef) -> usize;
        pub fn CGColorSpaceCreateWithName(name: CFStringRef) -> CGColorSpaceRef;
        pub fn CGBitmapContextCreate(
            data: *mut c_void,
            width: usize,
            height: usize,
            bits_per_component: usize,
            bytes_per_row: usize,
            space: CGColorSpaceRef,
            bitmap_info: u32,
        ) -> CGContextRef;
        pub fn CGContextDrawImage(ctx: CGContextRef, rect: CGRect, image: CGImageRef);
        pub fn CGContextRelease(ctx: CGContextRef);
    }
}

/// Releases a CoreFoundation object when dropped.
#[cfg(target_os = "macos")]
struct Owned(ffi::CFTypeRef);

#[cfg(target_os = "macos")]
impl Drop for Owned {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: created by a CF "Create/Copy" call we own.
            unsafe { ffi::CFRelease(self.0) };
        }
    }
}

#[cfg(target_os = "macos")]
fn open_source(path: &Path) -> Result<Owned, String> {
    use std::os::unix::ffi::OsStrExt;
    let bytes = path.as_os_str().as_bytes();
    // SAFETY: plain CF constructors with a valid byte buffer.
    unsafe {
        let url = Owned(ffi::CFURLCreateFromFileSystemRepresentation(
            std::ptr::null(),
            bytes.as_ptr(),
            bytes.len() as isize,
            0,
        ));
        if url.0.is_null() {
            return Err(format!("bad path {}", path.display()));
        }
        let src = Owned(ffi::CGImageSourceCreateWithURL(url.0, std::ptr::null()));
        if src.0.is_null() {
            return Err(format!("macOS can't open {}", path.display()));
        }
        Ok(src)
    }
}

#[cfg(target_os = "macos")]
fn number_at(dict: ffi::CFDictionaryRef, key: ffi::CFStringRef) -> Option<i32> {
    // SAFETY: `dict` is a live properties dictionary; values are CFNumbers.
    unsafe {
        let v = ffi::CFDictionaryGetValue(dict, key);
        if v.is_null() {
            return None;
        }
        let mut out: i32 = 0;
        (ffi::CFNumberGetValue(v, ffi::kCFNumberSInt32Type, &mut out as *mut i32 as *mut _) != 0)
            .then_some(out)
    }
}

/// Displayed size (orientation applied) without decoding pixels.
#[cfg(target_os = "macos")]
pub fn dimensions(path: &Path) -> Option<(u32, u32)> {
    let src = open_source(path).ok()?;
    // SAFETY: live image source; the copied dictionary is owned.
    let props =
        Owned(unsafe { ffi::CGImageSourceCopyPropertiesAtIndex(src.0, 0, std::ptr::null()) });
    if props.0.is_null() {
        return None;
    }
    let (w, h) = unsafe {
        (
            number_at(props.0, ffi::kCGImagePropertyPixelWidth)?,
            number_at(props.0, ffi::kCGImagePropertyPixelHeight)?,
        )
    };
    let orientation = unsafe { number_at(props.0, ffi::kCGImagePropertyOrientation) }.unwrap_or(1);
    let (w, h) = (w.max(0) as u32, h.max(0) as u32);
    // Orientations 5–8 swap width and height.
    Some(if (5..=8).contains(&orientation) {
        (h, w)
    } else {
        (w, h)
    })
}

/// Decode to 8-bit sRGB with orientation applied. `max_edge` asks ImageIO
/// for a downscaled decode (much faster for previews); `None` is full size.
#[cfg(target_os = "macos")]
pub fn decode_rgb8(path: &Path, max_edge: Option<u32>) -> Result<image::RgbImage, String> {
    let src = open_source(path)?;
    let edge = match max_edge {
        Some(e) => e as i32,
        None => {
            let (w, h) = dimensions(path)
                .ok_or_else(|| format!("macOS can't read the size of {}", path.display()))?;
            w.max(h).max(1) as i32
        }
    };
    // SAFETY: CF objects are created here and released by `Owned`; the
    // bitmap buffer outlives the context that draws into it.
    unsafe {
        let edge_num = Owned(ffi::CFNumberCreate(
            std::ptr::null(),
            ffi::kCFNumberSInt32Type,
            &edge as *const i32 as *const _,
        ));
        let keys = [
            ffi::kCGImageSourceCreateThumbnailFromImageAlways,
            ffi::kCGImageSourceCreateThumbnailWithTransform,
            ffi::kCGImageSourceThumbnailMaxPixelSize,
        ];
        let values = [ffi::kCFBooleanTrue, ffi::kCFBooleanTrue, edge_num.0];
        let options = Owned(ffi::CFDictionaryCreate(
            std::ptr::null(),
            keys.as_ptr(),
            values.as_ptr(),
            keys.len() as isize,
            &ffi::kCFTypeDictionaryKeyCallBacks,
            &ffi::kCFTypeDictionaryValueCallBacks,
        ));
        let image = Owned(ffi::CGImageSourceCreateThumbnailAtIndex(
            src.0, 0, options.0,
        ));
        if image.0.is_null() {
            return Err(format!("macOS couldn't decode {}", path.display()));
        }
        let (w, h) = (
            ffi::CGImageGetWidth(image.0),
            ffi::CGImageGetHeight(image.0),
        );
        if w == 0 || h == 0 {
            return Err(format!("empty image {}", path.display()));
        }
        let space = Owned(ffi::CGColorSpaceCreateWithName(ffi::kCGColorSpaceSRGB));
        let mut rgbx = vec![0u8; w * h * 4];
        let ctx = ffi::CGBitmapContextCreate(
            rgbx.as_mut_ptr() as *mut _,
            w,
            h,
            8,
            w * 4,
            space.0,
            ffi::kCGImageAlphaNoneSkipLast,
        );
        if ctx.is_null() {
            return Err("couldn't create a bitmap context".to_string());
        }
        ffi::CGContextDrawImage(
            ctx,
            ffi::CGRect {
                origin: ffi::CGPoint { x: 0., y: 0. },
                size: ffi::CGSize {
                    width: w as f64,
                    height: h as f64,
                },
            },
            image.0,
        );
        ffi::CGContextRelease(ctx);
        let rgb: Vec<u8> = rgbx
            .chunks_exact(4)
            .flat_map(|p| [p[0], p[1], p[2]])
            .collect();
        image::RgbImage::from_raw(w as u32, h as u32, rgb)
            .ok_or_else(|| "bitmap size mismatch".to_string())
    }
}

#[cfg(not(target_os = "macos"))]
pub fn dimensions(_path: &Path) -> Option<(u32, u32)> {
    None
}

#[cfg(not(target_os = "macos"))]
pub fn decode_rgb8(path: &Path, _max_edge: Option<u32>) -> Result<image::RgbImage, String> {
    Err(format!(
        "{} needs the macOS image decoder (HEIC/HEIF)",
        path.display()
    ))
}

#[cfg(test)]
mod content_tests {
    use super::*;

    /// A WebP saved with a `.jpeg` extension (seen in Photos libraries)
    /// decodes by content.
    #[test]
    fn mislabeled_webp_decodes() {
        let dir = std::env::temp_dir().join(format!("laika-webp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let webp = dir.join("real.webp");
        image::RgbImage::from_pixel(40, 30, image::Rgb([10, 200, 30]))
            .save(&webp)
            .unwrap();
        let fake = dir.join("photo.jpeg");
        std::fs::rename(&webp, &fake).unwrap();
        let img = open_raster(&fake, None).unwrap();
        assert_eq!((img.width(), img.height()), (40, 30));
        let meta = crate::exif::read_quick(&fake);
        assert_eq!((meta.width, meta.height), (40, 30));
        std::fs::remove_dir_all(&dir).ok();
    }
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;

    /// Opt-in: decode real files listed in `LAIKA_HEIC_SAMPLES` (one path per
    /// line) and report sizes and timings.
    #[test]
    fn decodes_real_samples() {
        let Ok(list) = std::env::var("LAIKA_HEIC_SAMPLES") else {
            return;
        };
        for line in std::fs::read_to_string(list).unwrap().lines() {
            let p = Path::new(line.trim());
            let t = std::time::Instant::now();
            let dims = dimensions(p);
            let t_dims = t.elapsed();
            let t = std::time::Instant::now();
            let preview = decode_rgb8(p, Some(2048)).unwrap();
            let t_preview = t.elapsed();
            let t = std::time::Instant::now();
            let full = decode_rgb8(p, None).unwrap();
            let t_full = t.elapsed();
            eprintln!(
                "{dims:?} in {t_dims:?}; preview {}x{} in {t_preview:?}; full {}x{} in {t_full:?}",
                preview.width(),
                preview.height(),
                full.width(),
                full.height()
            );
            assert_eq!(Some((full.width(), full.height())), dims);
            let t = std::time::Instant::now();
            let (small, large) = crate::preview::import_derivatives(p).unwrap();
            eprintln!(
                "import derivatives ({} + {} bytes) in {:?}",
                small.len(),
                large.len(),
                t.elapsed()
            );
        }
    }

    /// Encode a fixture HEIC with `sips` (ImageIO) — rotated, in P3 — and
    /// read it back through the decoder.
    #[test]
    fn heic_round_trip_with_orientation() {
        let dir = std::env::temp_dir().join(format!("laika-heic-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("src.png");
        // 60×40, left half red, right half blue.
        let mut img = image::RgbImage::new(60, 40);
        for (x, _, p) in img.enumerate_pixels_mut() {
            *p = if x < 30 {
                image::Rgb([220, 20, 20])
            } else {
                image::Rgb([20, 20, 220])
            };
        }
        img.save(&png).unwrap();
        let heic = dir.join("shot.heic");
        let ok = std::process::Command::new("/usr/bin/sips")
            .args(["-s", "format", "heic"])
            .arg(&png)
            .arg("--out")
            .arg(&heic)
            .output()
            .is_ok_and(|o| o.status.success());
        if !ok || !heic.exists() {
            // HEIC encoding unavailable on this machine.
            std::fs::remove_dir_all(&dir).ok();
            return;
        }
        assert!(handles(&heic));
        assert_eq!(dimensions(&heic), Some((60, 40)));
        let full = decode_rgb8(&heic, None).unwrap();
        assert_eq!((full.width(), full.height()), (60, 40));
        let left = full.get_pixel(5, 20);
        let right = full.get_pixel(55, 20);
        assert!(left[0] > 150 && left[2] < 90, "left {left:?}");
        assert!(right[2] > 150 && right[0] < 90, "right {right:?}");

        let small = decode_rgb8(&heic, Some(30)).unwrap();
        assert_eq!(small.width().max(small.height()), 30);

        // Rotate 90° via EXIF orientation only (pixels unchanged).
        let rotated = dir.join("rotated.heic");
        let ok = std::process::Command::new("/usr/bin/sips")
            .args(["-r", "90"])
            .arg(&heic)
            .arg("--out")
            .arg(&rotated)
            .output()
            .is_ok_and(|o| o.status.success());
        if ok && rotated.exists() {
            assert_eq!(dimensions(&rotated), Some((40, 60)));
            let r = decode_rgb8(&rotated, None).unwrap();
            assert_eq!((r.width(), r.height()), (40, 60));
        }
        std::fs::remove_dir_all(&dir).ok();
    }
}
