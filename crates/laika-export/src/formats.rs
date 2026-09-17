//! V27: multi-format still export (JPEG/PNG/WebP/AVIF/TIFF/Original).
//!
//! Rendered pixels arrive as sRGB RGBA8 from the pipeline; each format
//! encodes them with its native options. Honest limits of this build,
//! stated where users meet them:
//! - Color is pipeline sRGB throughout. Display P3 / Adobe RGB /
//!   ProPhoto need vendored ICC blobs plus per-container writers
//!   (TIFF already accepts one via `set_icc_profile`); the chooser
//!   offers sRGB only until then.
//! - WebP is lossless-only (`image-webp` has no lossy encoder; lossy
//!   needs the `libwebp` C library).
//! - PNG/TIFF are 8-bit only (the pipeline renders RGBA8; upconverting
//!   would fake fidelity). TIFF is uncompressed (`image` exposes no
//!   compression knob for TIFF).
//! - JPEG has quality only (no progressive / 4:4:4 / subsampling knobs
//!   in `image`'s encoder); the target-size loop below is ours.
//! - JPEG XL has no offline encoder in this tree (`jxl-oxide` decodes
//!   only; `jpegxl-rs` needs the libjxl C library + network build).
//! - Pixel files carry no EXIF (GPS included) — authorship rides the
//!   `.xmp` sidecar per the dialog's metadata policy, same as U10.
//! - Encoding runs sequentially on the background pool (one file at a
//!   time): rav1e parallelizes internally, and serial keeps memory and
//!   cancellation predictable.
//!
//! Build requirements per platform: none beyond `cargo build` — every
//! encoder here is pure Rust (image, ravif/rav1e). JPEG XL additionally
//! needs a C toolchain + libjxl, which is why it stays dimmed.

/// V27: output formats (Original copies the source file instead).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum ExportFormat {
    #[default]
    Jpeg,
    Png,
    WebP,
    Avif,
    Tiff,
    Original,
}

impl ExportFormat {
    pub const ALL: [ExportFormat; 6] = [
        ExportFormat::Jpeg,
        ExportFormat::Png,
        ExportFormat::WebP,
        ExportFormat::Avif,
        ExportFormat::Tiff,
        ExportFormat::Original,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ExportFormat::Jpeg => "JPEG",
            ExportFormat::Png => "PNG",
            ExportFormat::WebP => "WebP",
            ExportFormat::Avif => "AVIF",
            ExportFormat::Tiff => "TIFF",
            ExportFormat::Original => "Original",
        }
    }

    pub fn ext(self) -> &'static str {
        match self {
            ExportFormat::Jpeg => "jpg",
            ExportFormat::Png => "png",
            ExportFormat::WebP => "webp",
            ExportFormat::Avif => "avif",
            ExportFormat::Tiff => "tif",
            // Original keeps the source extension (set per file).
            ExportFormat::Original => "",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "PNG" => ExportFormat::Png,
            "WebP" => ExportFormat::WebP,
            "AVIF" => ExportFormat::Avif,
            "TIFF" => ExportFormat::Tiff,
            "Original" => ExportFormat::Original,
            _ => ExportFormat::Jpeg,
        }
    }

    /// Renders from pixels (Original copies instead).
    pub fn renders(self) -> bool {
        self != ExportFormat::Original
    }
}

/// V27: per-format options (persisted with the export dialog).
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ExportFormatOpts {
    /// JPEG/AVIF lossy quality 1..100.
    pub quality: u8,
    /// JPEG size cap in MB (0 = off; binary search down to q5).
    pub jpeg_max_mb: u32,
    /// PNG compression 0 fast / 1 default / 2 best.
    pub png_level: u8,
    /// AVIF speed 1 (slow) .. 10 (fast). Default 6.
    pub avif_speed: u8,
    /// AVIF bit depth 8 / 10.
    pub avif_depth: u8,
}

impl Default for ExportFormatOpts {
    fn default() -> Self {
        Self {
            quality: 90,
            jpeg_max_mb: 0,
            png_level: 1,
            avif_speed: 6,
            avif_depth: 8,
        }
    }
}

/// V12: insert (or replace) the XMP APP1 segment in a JPEG so
/// exported files carry title/caption/authorship in the standard
/// packet Lightroom, darktable, and exiftool read. Non-JPEG input or
/// oversize packets pass through untouched — never corrupt output.
pub fn embed_xmp_jpeg(jpeg: &[u8], packet: &[u8]) -> Vec<u8> {
    const XMP_MAGIC: &[u8] = b"http://ns.adobe.com/xap/1.0/\x00";
    if jpeg.len() < 4 || jpeg[0] != 0xFF || jpeg[1] != 0xD8 {
        return jpeg.to_vec();
    }
    if packet.len() + XMP_MAGIC.len() + 2 > 0xFFFF {
        return jpeg.to_vec();
    }
    // Walk segments from after SOI; stop at image data.
    let mut old_range: Option<(usize, usize)> = None;
    let mut pos = 2;
    while pos + 4 <= jpeg.len() {
        if jpeg[pos] != 0xFF {
            break;
        }
        let marker = jpeg[pos + 1];
        if marker == 0xD9 || marker == 0xDA {
            break; // EOI / start of scan — no more headers.
        }
        if marker == 0xD8 || (0xD0..=0xD8).contains(&marker) || marker == 0x01 {
            pos += 2;
            continue;
        }
        let len = u16::from_be_bytes([jpeg[pos + 2], jpeg[pos + 3]]) as usize;
        if len < 2 || pos + 2 + len > jpeg.len() {
            break;
        }
        // Match the magic inside this segment only (a short APP1 must not
        // borrow bytes from the segments after it).
        if marker == 0xE1
            && jpeg[pos + 4..pos + 2 + len].starts_with(XMP_MAGIC)
            && old_range.is_none()
        {
            old_range = Some((pos, pos + 2 + len));
        }
        pos += 2 + len;
    }
    let mut seg = Vec::with_capacity(2 + XMP_MAGIC.len() + packet.len() + 2);
    seg.extend_from_slice(&[0xFF, 0xE1]);
    let seg_len = (XMP_MAGIC.len() + packet.len() + 2) as u16;
    seg.extend_from_slice(&seg_len.to_be_bytes());
    seg.extend_from_slice(XMP_MAGIC);
    seg.extend_from_slice(packet);
    let mut out = Vec::with_capacity(jpeg.len() + seg.len());
    out.extend_from_slice(&jpeg[..2]);
    out.extend_from_slice(&seg);
    match old_range {
        Some((a, b)) => {
            out.extend_from_slice(&jpeg[2..a]);
            out.extend_from_slice(&jpeg[b..]);
        }
        None => out.extend_from_slice(&jpeg[2..]),
    }
    out
}

/// V27: encode rendered sRGB pixels to one format's bytes (+ extension).
/// Pure over pixels: unit-tested without a GPU.
pub fn encode_pixels(
    format: ExportFormat,
    opts: &ExportFormatOpts,
    rgba: &image::RgbaImage,
) -> Result<(Vec<u8>, &'static str), String> {
    let (w, h) = (rgba.width(), rgba.height());
    if w == 0 || h == 0 {
        return Err("empty image".to_string());
    }
    match format {
        ExportFormat::Jpeg => {
            let limit = (opts.jpeg_max_mb as u64).saturating_mul(1024 * 1024);
            let mut q = opts.quality.clamp(1, 100);
            // Flatten once, not once per target-size attempt.
            let rgb = rgb_bytes(rgba);
            loop {
                let bytes = encode_jpeg(&rgb, w, h, q)?;
                if limit == 0 || bytes.len() as u64 <= limit || q <= 5 {
                    return Ok((bytes, "jpg"));
                }
                // Halve toward the q5 floor (at most ~6 encodes).
                q = 5 + (q - 5) / 2;
            }
        }
        ExportFormat::Png => {
            let rgb = rgb_bytes(rgba);
            let compression = match opts.png_level {
                0 => image::codecs::png::CompressionType::Fast,
                2 => image::codecs::png::CompressionType::Best,
                _ => image::codecs::png::CompressionType::Default,
            };
            let mut buf = Vec::new();
            {
                let enc = image::codecs::png::PngEncoder::new_with_quality(
                    &mut buf,
                    compression,
                    image::codecs::png::FilterType::Adaptive,
                );
                use image::{ExtendedColorType, ImageEncoder};
                enc.write_image(&rgb, w, h, ExtendedColorType::Rgb8.into())
                    .map_err(|_| "PNG encode failed".to_string())?;
            }
            Ok((buf, "png"))
        }
        ExportFormat::WebP => {
            // Lossless only (see module docs).
            let mut buf = Vec::new();
            {
                let enc = image::codecs::webp::WebPEncoder::new_lossless(&mut buf);
                use image::ExtendedColorType;
                enc.encode(rgba.as_raw(), w, h, ExtendedColorType::Rgba8)
                    .map_err(|_| "WebP encode failed".to_string())?;
            }
            Ok((buf, "webp"))
        }
        ExportFormat::Avif => {
            let t0 = std::time::Instant::now();
            let pixels: Vec<ravif::RGBA8> = rgba
                .chunks_exact(4)
                .map(|px| ravif::RGBA8::new(px[0], px[1], px[2], px[3]))
                .collect();
            let img = ravif::Img::new(pixels.as_slice(), w as usize, h as usize);
            let depth = if opts.avif_depth >= 10 {
                ravif::BitDepth::Ten
            } else {
                ravif::BitDepth::Eight
            };
            let enc = ravif::Encoder::new()
                .with_quality(opts.quality.clamp(1, 100) as f32)
                .with_speed(opts.avif_speed.clamp(1, 10))
                .with_bit_depth(depth);
            let out = enc
                .encode_rgba(img)
                .map_err(|e| format!("AVIF encode failed: {e:?}"))?;
            // Encode time per effort level (gate observability).
            eprintln!(
                "[export] avif q{} speed{} {}-bit {}x{}: {} ms, {} bytes",
                opts.quality,
                opts.avif_speed,
                if opts.avif_depth >= 10 { 10 } else { 8 },
                w,
                h,
                t0.elapsed().as_millis(),
                out.avif_file.len(),
            );
            Ok((out.avif_file, "avif"))
        }
        ExportFormat::Tiff => {
            // Uncompressed 8-bit sRGB (see module docs).
            let rgb = rgb_bytes(rgba);
            let mut buf = std::io::Cursor::new(Vec::new());
            {
                let enc = image::codecs::tiff::TiffEncoder::new(&mut buf);
                use image::{ExtendedColorType, ImageEncoder};
                enc.write_image(&rgb, w, h, ExtendedColorType::Rgb8)
                    .map_err(|_| "TIFF encode failed".to_string())?;
            }
            Ok((buf.into_inner(), "tif"))
        }
        ExportFormat::Original => Err("originals copy, never render".to_string()),
    }
}

/// Drop alpha (pipeline output is opaque) straight from the RGBA buffer,
/// without cloning the whole image first.
fn rgb_bytes(rgba: &image::RgbaImage) -> Vec<u8> {
    let mut out = Vec::with_capacity(rgba.as_raw().len() / 4 * 3);
    for px in rgba.as_raw().chunks_exact(4) {
        out.extend_from_slice(&px[..3]);
    }
    out
}

/// V27: JPEG at one quality (shared with the target-size loop).
fn encode_jpeg(rgb: &[u8], w: u32, h: u32, quality: u8) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    {
        let mut enc =
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, quality.clamp(1, 100));
        use image::ExtendedColorType;
        enc.encode(rgb, w, h, ExtendedColorType::Rgb8)
            .map_err(|_| "JPEG encode failed".to_string())?;
    }
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient(w: u32, h: u32) -> image::RgbaImage {
        image::RgbaImage::from_fn(w, h, |x, y| {
            image::Rgba([(x % 256) as u8, (y % 256) as u8, ((x + y) % 256) as u8, 255])
        })
    }

    #[test]
    fn png_lossless_round_trips_pixel_identical() {
        // V27 gate, adapted: lossless PNG (JXL has no offline encoder).
        let src = gradient(64, 48);
        let (bytes, ext) = encode_pixels(ExportFormat::Png, &ExportFormatOpts::default(), &src)
            .expect("png encodes");
        assert_eq!(ext, "png");
        assert_eq!(&bytes[0..8], &[137, 80, 78, 71, 13, 10, 26, 10]);
        let back = image::load_from_memory(&bytes)
            .expect("png decodes")
            .to_rgba8();
        assert_eq!(back.dimensions(), src.dimensions());
        assert_eq!(back.as_raw(), src.as_raw());
    }

    #[test]
    fn jpeg_target_size_lands_under_limit() {
        let src = gradient(512, 384);
        let plain = ExportFormatOpts {
            quality: 95,
            ..Default::default()
        };
        let (full, _) = encode_pixels(ExportFormat::Jpeg, &plain, &src).expect("jpeg");
        assert!(!full.is_empty());
        // A 1 MB cap on photographic pixels must converge below it.
        let capped_opts = ExportFormatOpts {
            quality: 95,
            jpeg_max_mb: 1,
            ..Default::default()
        };
        let (capped, _) = encode_pixels(ExportFormat::Jpeg, &capped_opts, &src).expect("capped");
        assert!(capped.len() as u64 <= 1024 * 1024, "{}", capped.len());
        assert!(!capped.is_empty());
    }

    #[test]
    fn containers_have_correct_magic() {
        let src = gradient(32, 24);
        let opts = ExportFormatOpts::default();
        let (jpg, ext) = encode_pixels(ExportFormat::Jpeg, &opts, &src).expect("jpg");
        assert_eq!(ext, "jpg");
        assert_eq!(&jpg[0..2], &[0xFF, 0xD8]);
        let (webp, ext) = encode_pixels(ExportFormat::WebP, &opts, &src).expect("webp");
        assert_eq!(ext, "webp");
        assert_eq!(&webp[0..4], b"RIFF");
        assert_eq!(&webp[8..12], b"WEBP");
        let (tif, ext) = encode_pixels(ExportFormat::Tiff, &opts, &src).expect("tif");
        assert_eq!(ext, "tif");
        assert!(tif.starts_with(b"II*\x00") || tif.starts_with(b"MM\x00*"));
        let empty = image::RgbaImage::new(0, 0);
        assert!(encode_pixels(ExportFormat::Jpeg, &opts, &empty).is_err());
        assert!(encode_pixels(ExportFormat::Original, &opts, &src).is_err());
        assert_eq!(ExportFormat::parse("bogus"), ExportFormat::Jpeg);
    }

    #[test]
    fn xmp_embed_inserts_replaceable_app1_without_harming_pixels() {
        // V12 gate: XMP packet lands in APP1, pixels decode identical,
        // re-embedding replaces (one XMP segment), non-JPEG passes through.
        let src = gradient(32, 24);
        let opts = ExportFormatOpts::default();
        let (jpg, _) = encode_pixels(ExportFormat::Jpeg, &opts, &src).expect("jpeg");
        assert_eq!(&jpg[0..2], &[0xFF, 0xD8]);
        let packet = b"<x:xmpmeta>test-caption</x:xmpmeta>";
        let stamped = embed_xmp_jpeg(&jpg, packet);
        assert!(stamped.starts_with(&[0xFF, 0xD8, 0xFF, 0xE1]));
        assert!(
            stamped.windows(packet.len()).any(|w| w == packet),
            "packet present"
        );
        let back = image::load_from_memory(&stamped)
            .expect("still decodes")
            .to_rgba8();
        // JPEG is lossy: compare against the un-stamped decode, identical.
        let plain = image::load_from_memory(&jpg).expect("decodes").to_rgba8();
        assert_eq!(back.as_raw(), plain.as_raw());
        // Re-embed replaces: exactly one XMP header survives.
        let twice = embed_xmp_jpeg(&stamped, b"<x/>");
        let magic = b"http://ns.adobe.com/xap/1.0/";
        assert_eq!(
            twice.windows(magic.len()).filter(|w| *w == magic).count(),
            1
        );
        // Non-JPEG passes through byte-identical.
        let (png, _) = encode_pixels(ExportFormat::Png, &opts, &src).expect("png");
        assert_eq!(embed_xmp_jpeg(&png, packet), png);
        assert_eq!(embed_xmp_jpeg(b"tiny", packet), b"tiny");
    }

    #[test]
    fn avif_smoke_encodes_ftyp() {
        // rav1e is slow: 16×12 px keeps the suite fast.
        let src = gradient(16, 12);
        let opts = ExportFormatOpts {
            avif_speed: 10,
            ..Default::default()
        };
        let (bytes, ext) = encode_pixels(ExportFormat::Avif, &opts, &src).expect("avif");
        assert_eq!(ext, "avif");
        assert_eq!(&bytes[4..8], b"ftyp");
    }
}
