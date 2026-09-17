//! V28: watermarks — text (SVG typesetting) or graphic (PNG/SVG),
//! anchored with margins, scaled proportionally to the output long
//! edge, composited with opacity. Pure placement + compositing math
//! (unit-tested); rasterization goes through resvg (pure Rust).
//!
//! Honest limits: text needs system fonts (fontdb); unknown families
//! fall back silently — check the output files, which carry exactly
//! what the dialog produces. Graphic SVGs render without external
//! image references.

/// V28: watermark configuration (persisted inside export presets).
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct WatermarkSpec {
    /// Off / Text / Graphic.
    pub mode: WatermarkMode,
    pub text: String,
    /// Font family (falls back to system default when missing).
    pub font: String,
    /// Text size, percent of output long edge.
    pub font_pct: u32,
    /// sRGB text color.
    pub color: [u8; 3],
    /// Opacity 0..100.
    pub opacity: u8,
    /// Soft offset shadow behind text.
    pub shadow: bool,
    /// Graphic file (PNG/JPEG/SVG).
    pub graphic_path: String,
    /// Anchor 0..8 (TL,T,TR,L,C,R,BL,B,BR).
    pub anchor: u8,
    /// Margin in output pixels.
    pub margin_px: u32,
    /// Graphic long edge, percent of output long edge.
    pub scale_pct: u32,
}

impl Default for WatermarkSpec {
    fn default() -> Self {
        Self {
            mode: WatermarkMode::Off,
            text: String::new(),
            font: "monospace".to_string(),
            font_pct: 4,
            color: [255, 255, 255],
            opacity: 80,
            shadow: true,
            graphic_path: String::new(),
            anchor: 8,
            margin_px: 24,
            scale_pct: 15,
        }
    }
}

/// V28: watermark kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum WatermarkMode {
    #[default]
    Off,
    Text,
    Graphic,
}

impl WatermarkMode {
    pub fn label(self) -> &'static str {
        match self {
            WatermarkMode::Off => "Off",
            WatermarkMode::Text => "Text",
            WatermarkMode::Graphic => "Graphic",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "Text" => WatermarkMode::Text,
            "Graphic" => WatermarkMode::Graphic,
            _ => WatermarkMode::Off,
        }
    }
}

/// V28: anchor grid factors (x, y in 0..1).
pub fn anchor_factors(anchor: u8) -> (f32, f32) {
    let ax = [0., 0.5, 1.];
    let a = (anchor.min(8) / 3) as usize;
    let b = (anchor.min(8) % 3) as usize;
    (ax[b], ax[a])
}

/// V28: top-left placement, clamped inside the frame. Symmetric by
/// construction — portrait and landscape place identically.
pub fn place(
    base_w: u32,
    base_h: u32,
    wm_w: u32,
    wm_h: u32,
    anchor: u8,
    margin_px: u32,
) -> (i32, i32) {
    let (fx, fy) = anchor_factors(anchor);
    // Free travel may be 0 (watermark exactly as wide as the frame): a
    // floor of 1 would shift it a pixel and clip its last column.
    let avail_w = base_w.saturating_sub(wm_w) as f32;
    let avail_h = base_h.saturating_sub(wm_h) as f32;
    let m = margin_px as f32;
    // Margin pins both ends: TL = m, BR = avail − m, center = avail/2.
    let x = (m + fx * (avail_w - 2. * m)).clamp(0., avail_w);
    let y = (m + fy * (avail_h - 2. * m)).clamp(0., avail_h);
    (x.round() as i32, y.round() as i32)
}

/// V28: straight-alpha composite of `wm` onto `base` at (x, y) with a
/// master opacity. Out-of-frame pixels clip (never panics).
pub fn composite(base: &mut image::RgbaImage, wm: &image::RgbaImage, x: i32, y: i32, opacity: u8) {
    let master = (opacity.min(100) as f32) / 100.;
    if master <= 0. {
        return;
    }
    let (bw, bh) = (base.width() as i32, base.height() as i32);
    for (wy, row) in wm.rows().enumerate() {
        let by = y + wy as i32;
        if !(0..bh).contains(&by) {
            continue;
        }
        for (wx, px) in row.enumerate() {
            let bx = x + wx as i32;
            if !(0..bw).contains(&bx) {
                continue;
            }
            let sa = (px[3] as f32 / 255.) * master;
            if sa <= 0. {
                continue;
            }
            let dst = base.get_pixel_mut(bx as u32, by as u32);
            // Straight-alpha "over": destination color counts by its own
            // alpha (identical to a plain lerp on opaque bases).
            let da = dst[3] as f32 / 255.;
            let oa = sa + da * (1. - sa);
            for c in 0..3 {
                dst[c] = ((px[c] as f32 * sa + dst[c] as f32 * da * (1. - sa)) / oa)
                    .round()
                    .clamp(0., 255.) as u8;
            }
            dst[3] = (oa * 255.).round() as u8;
        }
    }
}

/// V28: validate the configuration before a run writes anything.
/// Off always passes; Text needs non-blank copy; Graphic must load.
pub fn validate(spec: &WatermarkSpec) -> Result<(), String> {
    match spec.mode {
        WatermarkMode::Off => Ok(()),
        WatermarkMode::Text => {
            if spec.text.trim().is_empty() {
                return Err("watermark text is empty — type copy or switch it off".to_string());
            }
            Ok(())
        }
        WatermarkMode::Graphic => {
            if spec.graphic_path.trim().is_empty() {
                return Err("watermark graphic has no file — pick one or switch it off".to_string());
            }
            // Probe at a tiny size: proves the file parses without
            // paying full-scale rasterization per run.
            load_graphic(&spec.graphic_path, 256, 15).map(|_| ())
        }
    }
}

/// V28: stamp the watermark onto output pixels (post-resize, so sizes
/// track the exported file). Returns whether anything was stamped.
/// Off never touches pixels; empty rasterizations stamp nothing.
pub fn apply(base: &mut image::RgbaImage, spec: &WatermarkSpec) -> Result<bool, String> {
    if spec.mode == WatermarkMode::Off {
        return Ok(false);
    }
    let long = base.width().max(base.height());
    if long == 0 {
        return Err("empty image".to_string());
    }
    let wm = match spec.mode {
        WatermarkMode::Off => return Ok(false),
        WatermarkMode::Text => {
            if spec.text.trim().is_empty() {
                return Err("watermark text is empty".to_string());
            }
            let px = ((long as u64 * spec.font_pct.clamp(1, 20) as u64) / 100).clamp(8, 512) as u32;
            rasterize_svg(text_svg(spec, px).as_bytes())?
        }
        WatermarkMode::Graphic => load_graphic(&spec.graphic_path, long, spec.scale_pct)?,
    };
    if wm.width() <= 1 && wm.height() <= 1 && wm.get_pixel(0, 0)[3] == 0 {
        // Nothing rendered (e.g. missing fonts): report, don't fake it.
        return Ok(false);
    }
    let (x, y) = place(
        base.width(),
        base.height(),
        wm.width(),
        wm.height(),
        spec.anchor,
        spec.margin_px,
    );
    composite(base, &wm, x, y, spec.opacity);
    Ok(true)
}

fn escape_xml(s: &str) -> String {
    // Quotes too: the font family lands inside an attribute value.
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// V28: typeset one line of text as SVG (single line; newlines become
/// spaces). Size in px for the target output.
pub fn text_svg(spec: &WatermarkSpec, px: u32) -> String {
    // Control characters (newlines, tabs, NUL…) are not valid XML text:
    // they become spaces instead of breaking the SVG parse.
    let text: String = spec
        .text
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let text = escape_xml(text.trim());
    let (r, g, b) = (spec.color[0], spec.color[1], spec.color[2]);
    let family: String = spec
        .font
        .trim()
        .chars()
        .filter(|c| !c.is_control())
        .collect();
    let family = escape_xml(&family);
    // Generous canvas: text metrics come from rasterization, trimmed after.
    let w = px.saturating_mul(32).max(64);
    let h = (px as f32 * 1.8).ceil() as u32 + 8;
    let shadow = if spec.shadow {
        let off = ((px as f32 * 0.06).ceil() as u32).max(1);
        format!(
            "<text x=\"{}\" y=\"{}\" font-family=\"{}\" font-size=\"{}\" fill=\"black\" fill-opacity=\"0.6\">{}</text>",
            4 + off,
            4 + px + off,
            family,
            px,
            text
        )
    } else {
        String::new()
    };
    format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\">\
         {shadow}\
         <text x=\"4\" y=\"{}\" font-family=\"{}\" font-size=\"{}\" fill=\"rgb({},{},{})\">{}</text>\
         </svg>",
        4 + px,
        family,
        px,
        r,
        g,
        b,
        text
    )
}

/// V28: rasterize SVG bytes to straight-alpha RGBA at its document size.
pub fn rasterize_svg(svg: &[u8]) -> Result<image::RgbaImage, String> {
    let text = std::str::from_utf8(svg).map_err(|_| "svg is not UTF-8".to_string())?;
    // Scanning system fonts costs far more than a watermark render; load
    // them once per process instead of once per exported file.
    static FONTS: std::sync::OnceLock<std::sync::Arc<resvg::usvg::fontdb::Database>> =
        std::sync::OnceLock::new();
    let fontdb = FONTS
        .get_or_init(|| {
            let mut db = resvg::usvg::fontdb::Database::new();
            db.load_system_fonts();
            std::sync::Arc::new(db)
        })
        .clone();
    let opt = resvg::usvg::Options {
        fontdb,
        ..Default::default()
    };
    let tree = resvg::usvg::Tree::from_str(text, &opt).map_err(|e| format!("svg parse: {e:?}"))?;
    let size = tree.size();
    let (w, h) = (size.width() as u32, size.height() as u32);
    if w == 0 || h == 0 {
        return Err("svg has no size".to_string());
    }
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h).ok_or("pixmap failed".to_string())?;
    resvg::render(
        &tree,
        resvg::tiny_skia::Transform::identity(),
        &mut pixmap.as_mut(),
    );
    // tiny-skia is premultiplied RGBA: unpremultiply into straight
    // alpha, trimming fully transparent borders for tight placement.
    let data = pixmap.data();
    let mut min_x = w;
    let mut min_y = h;
    let mut max_x = 0u32;
    let mut max_y = 0u32;
    for y in 0..h {
        for x in 0..w {
            let i = (y as usize * w as usize + x as usize) * 4;
            if data[i + 3] > 0 {
                min_x = min_x.min(x);
                min_y = min_y.min(y);
                max_x = max_x.max(x);
                max_y = max_y.max(y);
            }
        }
    }
    if max_x < min_x || max_y < min_y {
        // No ink (missing fonts render nothing): 1px transparent tile.
        // Callers treat empty as "nothing to stamp", never an error —
        // the preview shows the same absence exports get.
        return Ok(image::RgbaImage::from_pixel(
            1,
            1,
            image::Rgba([0, 0, 0, 0]),
        ));
    }
    let (tw, th) = (max_x - min_x + 1, max_y - min_y + 1);
    let mut out = image::RgbaImage::new(tw, th);
    for y in 0..th {
        for x in 0..tw {
            let i = ((min_y + y) as usize * w as usize + (min_x + x) as usize) * 4;
            let (r, g, b, a) = (data[i], data[i + 1], data[i + 2], data[i + 3]);
            let (r, g, b) = if a == 0 {
                (0, 0, 0)
            } else {
                (
                    ((r as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8,
                    ((g as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8,
                    ((b as u32 * 255 + a as u32 / 2) / a as u32).min(255) as u8,
                )
            };
            out.put_pixel(x, y, image::Rgba([r, g, b, a]));
        }
    }
    Ok(out)
}

/// V28: load a graphic watermark (PNG/JPEG via image, SVG via resvg),
/// scaled so its long edge is `scale_pct`% of the output long edge.
/// Missing/unreadable files explain themselves (the run validates
/// before writing anything).
pub fn load_graphic(path: &str, out_long: u32, scale_pct: u32) -> Result<image::RgbaImage, String> {
    let bytes = std::fs::read(path).map_err(|_| format!("watermark graphic missing: {path}"))?;
    // Lossy: a 200-byte cut through a multi-byte character (or a BOM'd
    // prolog) must not hide the `<svg` sniff.
    let head = String::from_utf8_lossy(&bytes[..bytes.len().min(200)]);
    let is_svg = path.to_lowercase().ends_with(".svg") || head.contains("<svg");
    let img = if is_svg {
        rasterize_svg(&bytes)?
    } else {
        image::load_from_memory(&bytes)
            .map_err(|_| format!("watermark graphic unreadable: {path}"))?
            .to_rgba8()
    };
    if img.width() == 0 || img.height() == 0 {
        return Err(format!("watermark graphic is empty: {path}"));
    }
    let target = ((out_long as u64 * scale_pct.max(1) as u64) / 100).max(1) as u32;
    let (w, h) = (img.width(), img.height());
    // Scale to exactly the target in both directions: small logos grow,
    // oversized graphics shrink (Lanczos either way).
    let (nw, nh) = if w >= h {
        (
            target,
            ((h as u64 * target as u64) / w as u64).max(1) as u32,
        )
    } else {
        (
            ((w as u64 * target as u64) / h as u64).max(1) as u32,
            target,
        )
    };
    if img.pixels().all(|p| p[3] == 255) {
        return Ok(image::imageops::resize(
            &img,
            nw,
            nh,
            image::imageops::FilterType::Lanczos3,
        ));
    }
    // Resample premultiplied: straight-alpha filtering bleeds the color of
    // fully transparent pixels (black after SVG unpremultiply) into logo
    // edges as dark halos.
    let pre = image::Rgba32FImage::from_fn(w, h, |x, y| {
        let p = img.get_pixel(x, y);
        let a = p[3] as f32 / 255.;
        image::Rgba([
            p[0] as f32 / 255. * a,
            p[1] as f32 / 255. * a,
            p[2] as f32 / 255. * a,
            a,
        ])
    });
    let small = image::imageops::resize(&pre, nw, nh, image::imageops::FilterType::Lanczos3);
    Ok(image::RgbaImage::from_fn(nw, nh, |x, y| {
        let p = small.get_pixel(x, y);
        let a = p[3];
        let un = |c: f32| {
            if a > 0. {
                ((c / a).clamp(0., 1.) * 255.).round() as u8
            } else {
                0
            }
        };
        image::Rgba([un(p[0]), un(p[1]), un(p[2]), (a * 255.).round() as u8])
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn anchors_place_symmetrically() {
        // Portrait and landscape place identically from every edge.
        for (bw, bh) in [(1000u32, 800u32), (800u32, 1000u32)] {
            assert_eq!(place(bw, bh, 100, 40, 0, 10), (10, 10));
            assert_eq!(
                place(bw, bh, 100, 40, 8, 10),
                (bw as i32 - 110, bh as i32 - 50)
            );
            assert_eq!(
                place(bw, bh, 100, 40, 4, 10),
                ((bw as i32 - 100) / 2, (bh as i32 - 40) / 2)
            );
            // Oversize watermarks clamp inside, never negative.
            let (x, y) = place(100, 100, 400, 400, 8, 10);
            assert!(x >= 0 && y >= 0);
        }
        // Margins pin to edges.
        assert_eq!(place(1000, 800, 100, 40, 2, 0), (900, 0));
        // Full-width watermark at a right anchor stays fully in frame.
        assert_eq!(place(100, 50, 100, 10, 8, 0), (0, 40));
    }

    #[test]
    fn composite_opacity_and_clipping() {
        let mut base = image::RgbaImage::from_pixel(10, 10, image::Rgba([0, 0, 0, 255]));
        let wm = image::RgbaImage::from_pixel(4, 4, image::Rgba([255, 255, 255, 255]));
        composite(&mut base, &wm, 2, 2, 0);
        assert_eq!(base.get_pixel(3, 3), &image::Rgba([0, 0, 0, 255]));
        composite(&mut base, &wm, 2, 2, 100);
        assert_eq!(base.get_pixel(3, 3), &image::Rgba([255, 255, 255, 255]));
        // Half opacity blends; off-frame placement clips.
        let mut base = image::RgbaImage::from_pixel(10, 10, image::Rgba([0, 0, 0, 255]));
        composite(&mut base, &wm, 2, 2, 50);
        assert_eq!(base.get_pixel(3, 3), &image::Rgba([128, 128, 128, 255]));
        composite(&mut base, &wm, 8, 8, 100);
        assert_eq!(base.get_pixel(9, 9), &image::Rgba([255, 255, 255, 255]));
    }

    #[test]
    fn svg_pipeline_renders_shapes_without_fonts() {
        // Rectangles need no fonts: deterministic rasterization.
        let svg = b"<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"20\" height=\"10\"><rect width=\"20\" height=\"10\" fill=\"red\"/></svg>";
        let img = rasterize_svg(svg).expect("rect rasterizes");
        assert_eq!((img.width(), img.height()), (20, 10));
        assert_eq!(img.get_pixel(5, 5), &image::Rgba([255, 0, 0, 255]));
        // Text escaping is structural (glyphs depend on system fonts).
        let spec = WatermarkSpec {
            mode: WatermarkMode::Text,
            text: "A&B <tag>".into(),
            ..Default::default()
        };
        let svg = text_svg(&spec, 24);
        assert!(svg.contains("A&amp;B &lt;tag&gt;"));
        assert!(svg.contains("font-size=\"24\""));
        // Text rasterizes to the right ballpark (missing fonts still Ok).
        let img = rasterize_svg(svg.as_bytes()).expect("text svg parses");
        assert!(img.width() > 0 && img.height() > 0);
    }

    #[test]
    fn validate_and_apply_gate_the_run() {
        // Empty text explains itself before anything writes.
        let empty = WatermarkSpec {
            mode: WatermarkMode::Text,
            ..Default::default()
        };
        assert!(validate(&empty).is_err());
        assert!(validate(&WatermarkSpec::default()).is_ok());
        // Off never touches pixels.
        let mut base = image::RgbaImage::from_pixel(200, 100, image::Rgba([10, 10, 10, 255]));
        assert!(!apply(&mut base, &WatermarkSpec::default()).expect("off applies"));
        assert_eq!(base.get_pixel(100, 50), &image::Rgba([10, 10, 10, 255]));
        // Stamped text changes pixels (shape rasterization is covered above).
        let spec = WatermarkSpec {
            mode: WatermarkMode::Text,
            text: "© Laika".into(),
            font_pct: 10,
            opacity: 100,
            anchor: 8,
            margin_px: 4,
            ..Default::default()
        };
        assert!(validate(&spec).is_ok());
        let stamped = apply(&mut base, &spec).expect("text applies");
        assert!(stamped);
        let changed = base
            .pixels()
            .any(|p| p[0] != 10 || p[1] != 10 || p[2] != 10);
        assert!(changed, "stamped pixels must differ from the base");
    }

    #[test]
    fn graphic_load_validates_loudly() {
        assert!(load_graphic("/nonexistent/wm.png", 1000, 15).is_err());
        let dir = std::env::temp_dir().join("laika-wm-test");
        std::fs::create_dir_all(&dir).unwrap();
        let png = dir.join("wm.png");
        image::RgbImage::from_pixel(200, 100, image::Rgb([9u8, 9, 9]))
            .save(&png)
            .unwrap();
        // 15% of 1000 = 150 long edge.
        let img = load_graphic(&png.to_string_lossy(), 1000, 15).expect("png loads");
        assert_eq!((img.width(), img.height()), (150, 75));
        // SVG graphics render too.
        let svg = dir.join("wm.svg");
        std::fs::write(&svg, "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"40\" height=\"40\"><rect width=\"40\" height=\"40\" fill=\"blue\"/></svg>").unwrap();
        let img = load_graphic(&svg.to_string_lossy(), 1000, 10).expect("svg loads");
        assert_eq!((img.width(), img.height()), (100, 100));
        // Transparent surroundings must not darken a white logo's edges.
        let logo = dir.join("logo.png");
        image::RgbaImage::from_fn(40, 40, |x, _| {
            if x < 20 {
                image::Rgba([255, 255, 255, 255])
            } else {
                image::Rgba([0, 0, 0, 0])
            }
        })
        .save(&logo)
        .unwrap();
        let img = load_graphic(&logo.to_string_lossy(), 1000, 3).expect("png loads");
        for p in img.pixels().filter(|p| p[3] > 16) {
            assert!(p[0] > 240, "dark fringe: {p:?}");
        }
        // Quotes and control characters never break the SVG document.
        let spec = WatermarkSpec {
            mode: WatermarkMode::Text,
            text: "a\tb\nc".into(),
            font: "My \"Font\"".into(),
            ..Default::default()
        };
        assert!(rasterize_svg(text_svg(&spec, 16).as_bytes()).is_ok());
        std::fs::remove_dir_all(&dir).ok();
    }
}
