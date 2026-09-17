//! Full RAW decode to linear camera RGB (Phase 3).
//!
//! `decode` runs rawler's demosaic without white balance, color calibration
//! or tone curve, so the wgpu pipeline owns the whole look. Output is half
//! floats, 3 channels, normalized so the sensor white is ~1.0.
//!
//! `decode_editing` returns a long-edge-2048 version for interactive work and
//! caches it at `<cache>/<hash>/linear-2048.f16`; full resolution is decoded
//! on demand for export.

use std::path::Path;

use rawler::decoders::RawDecodeParams;

#[derive(Clone, Debug)]
pub struct LinearImage {
    pub width: u32,
    pub height: u32,
    /// Half-float RGB, 3 per pixel, row-major, linear camera space.
    pub rgb_f16: Vec<u16>,
    /// As-shot white-balance multipliers (multiply, as rawler applies them).
    pub wb_as_shot: [f32; 3],
    /// Camera → linear sRGB, built with rawler's own chain
    /// (`normalize(xyz2cam × sRGB→XYZ)` then pseudo-inverse).
    pub cam_to_xyz: [[f32; 3]; 3],
    pub black: f32,
    pub white: f32,
    pub camera: String,
    /// True for already-rendered sources (rasters through the sRGB
    /// bridge): the develop shader's default look is then the identity,
    /// so an unedited render matches the original file.
    pub display_referred: bool,
}

impl LinearImage {
    pub fn pixel_f32(&self, x: u32, y: u32) -> [f32; 3] {
        let i = (y as usize * self.width as usize + x as usize) * 3;
        [
            half::f16::from_bits(self.rgb_f16[i]).to_f32(),
            half::f16::from_bits(self.rgb_f16[i + 1]).to_f32(),
            half::f16::from_bits(self.rgb_f16[i + 2]).to_f32(),
        ]
    }
}

const LINEAR_STEPS: &[rawler::imgop::develop::ProcessingStep] = &[
    rawler::imgop::develop::ProcessingStep::Rescale,
    rawler::imgop::develop::ProcessingStep::Demosaic,
    rawler::imgop::develop::ProcessingStep::FujiRotate,
    rawler::imgop::develop::ProcessingStep::CropActiveArea,
    rawler::imgop::develop::ProcessingStep::CropDefault,
];

fn linear_floats(path: &Path) -> Result<(u32, u32, Vec<f32>, rawler::RawImage), String> {
    let raw = rawler::decode_file(path).map_err(|e| format!("decode {}: {e}", path.display()))?;
    let dev = rawler::imgop::develop::RawDevelop::new_with(LINEAR_STEPS);
    let inter = dev
        .develop_intermediate(&raw)
        .map_err(|e| format!("demosaic {}: {e}", path.display()))?;
    match inter {
        rawler::imgop::develop::Intermediate::ThreeColor(px) => {
            let dim = px.dim();
            Ok((dim.w as u32, dim.h as u32, px.into_flatten(), raw))
        }
        _ => Err(format!("unsupported pixel layout for {}", path.display())),
    }
}

#[cfg(test)]
pub(crate) fn linear_dims_for_test(path: &Path) -> (u32, u32, (), ()) {
    let (w, h, _, _) = linear_floats(path).expect("decode");
    (w, h, (), ())
}

fn finish(
    width: u32,
    height: u32,
    flat: Vec<f32>,
    raw: &rawler::RawImage,
    _params: &RawDecodeParams,
) -> LinearImage {
    let rgb_f16 = f16_bits(&flat);
    let wb = raw.wb_coeffs;
    let wb_as_shot = [
        if wb[0].is_finite() && wb[0] > 0. {
            wb[0]
        } else {
            1.
        },
        if wb[1].is_finite() && wb[1] > 0. {
            wb[1]
        } else {
            1.
        },
        if wb[2].is_finite() && wb[2] > 0. {
            wb[2]
        } else {
            1.
        },
    ];
    // Prefer the D65 camera matrix; `cam_to_xyz_normalized` reads a
    // deprecated field that is zeroed for many files (NaN out).
    let order = [
        rawler::imgop::xyz::Illuminant::D65,
        rawler::imgop::xyz::Illuminant::A,
        rawler::imgop::xyz::Illuminant::Daylight,
        rawler::imgop::xyz::Illuminant::Flash,
        rawler::imgop::xyz::Illuminant::D50,
    ];
    // Same chain as rawler's Calibrate step: normalize(xyz2cam × sRGB→XYZ),
    // then pseudo-inverse gives camera → linear sRGB.
    let mut cam_to_xyz = [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]];
    if let Some((_, m)) = raw.color_matrix_find_first(order) {
        if m.len() == 9 && m.iter().all(|v| v.is_finite()) {
            use rawler::imgop::matrix::{multiply, normalize, pseudo_inverse};
            use rawler::imgop::xyz::SRGB_TO_XYZ_D65;
            let xyz2cam = [[m[0], m[1], m[2]], [m[3], m[4], m[5]], [m[6], m[7], m[8]]];
            let rgb2cam = normalize(multiply(&xyz2cam, &SRGB_TO_XYZ_D65));
            let back = pseudo_inverse(rgb2cam);
            if back.iter().flatten().all(|v| v.is_finite()) {
                cam_to_xyz = back;
            }
        }
    }
    LinearImage {
        width,
        height,
        rgb_f16,
        wb_as_shot,
        cam_to_xyz,
        black: 0.0,
        white: 1.0,
        camera: raw.model.clone(),
        display_referred: false,
    }
}

/// Full-resolution linear decode. Budget: under 1.5 s for 24 MP (release).
pub fn decode(path: &Path) -> Result<LinearImage, String> {
    let params = RawDecodeParams::default();
    let (w, h, flat, raw) = linear_floats(path)?;
    Ok(finish(w, h, flat, &raw, &params))
}

/// Editing-size decode (long edge 2048), cached on disk.
pub fn decode_editing(path: &Path, hash: &str, cache_dir: &Path) -> Result<LinearImage, String> {
    let cached = cache_dir.join(hash).join("linear-2048.f16");
    if let Ok(mut img) = read_cache(&cached) {
        // Caches written before the header carried the flag: a raster's
        // entry can only have come from the sRGB bridge.
        img.display_referred |= !crate::is_raw(path);
        return Ok(img);
    }
    let params = RawDecodeParams::default();
    let (w, h, flat, raw) = linear_floats(path)?;
    let (w2, h2, flat2) = downscale_linear(flat, w, h, 2048);
    let img = finish(w2, h2, flat2, &raw, &params);
    write_cache(&cached, &img).ok();
    Ok(img)
}

/// U08: share the editing-size cache with bridge decodes (same file,
/// same format — one source, one cache entry).
pub fn write_editing_cache(img: &LinearImage, hash: &str, cache_dir: &Path) {
    write_cache(&cache_dir.join(hash).join("linear-2048.f16"), img).ok();
}

/// U08: raster bridge — sRGB originals (JPEG/TIFF/PNG) as linear pipeline
/// sources so crops, tone, and detail share one render path for every
/// format. Inverse-compands to linear, neutral white balance, identity
/// camera matrix, marked display-referred so default params render the
/// original unchanged. `edge` downscales like the editing path; `None`
/// keeps native resolution.
pub fn linear_from_raster(path: &Path, edge: Option<u32>) -> Result<LinearImage, String> {
    let rgb8 = crate::system_image::open_raster(path, None)?.into_rgb8();
    let (w, h) = (rgb8.width(), rgb8.height());
    if w == 0 || h == 0 {
        return Err(format!("empty image {}", path.display()));
    }
    // sRGB EOTF inverse → scene-linear (256-entry table: one `powf` per
    // code value instead of three per pixel).
    let lut: [f32; 256] = std::array::from_fn(|c| {
        let v = c as f32 / 255.;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    });
    let flat: Vec<f32> = rgb8.as_raw().iter().map(|&c| lut[c as usize]).collect();
    drop(rgb8);
    let (w, h, flat) = match edge {
        Some(e) => downscale_linear(flat, w, h, e),
        None => (w, h, flat),
    };
    let rgb_f16 = f16_bits(&flat);
    Ok(LinearImage {
        width: w,
        height: h,
        rgb_f16,
        wb_as_shot: [1., 1., 1.],
        cam_to_xyz: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
        black: 0.0,
        white: 1.0,
        camera: "sRGB raster".into(),
        display_referred: true,
    })
}

/// f32 → half-float bits in one vectorized pass (per-element
/// `f16::from_f32` re-runs CPU feature detection for every sample).
fn f16_bits(flat: &[f32]) -> Vec<u16> {
    use half::slice::{HalfBitsSliceExt, HalfFloatSliceExt};
    let mut out = vec![0u16; flat.len()];
    out.reinterpret_cast_mut::<half::f16>()
        .convert_from_f32_slice(flat);
    out
}

/// Tent-filter taps (first source index, normalized weights) mapping `src`
/// samples onto `dst`: the same kernel and support as `image`'s Triangle.
fn tent_taps(src: u32, dst: u32) -> Vec<(usize, Vec<f32>)> {
    let ratio = src as f32 / dst as f32;
    let sratio = ratio.max(1.);
    (0..dst)
        .map(|o| {
            let center = (o as f32 + 0.5) * ratio;
            let left = ((center - sratio).floor() as i64).clamp(0, src as i64 - 1);
            let right = ((center + sratio).ceil() as i64).clamp(left + 1, src as i64);
            let c = center - 0.5;
            let mut ws: Vec<f32> = (left..right)
                .map(|i| {
                    let x = ((i as f32 - c) / sratio).abs();
                    if x < 1. { 1. - x } else { 0. }
                })
                .collect();
            let sum: f32 = ws.iter().sum();
            if sum > 0. {
                ws.iter_mut().for_each(|w| *w /= sum);
            }
            (left as usize, ws)
        })
        .collect()
}

/// Separable tent downscale of linear RGB. Unlike `image::imageops::resize`
/// (which clamps float samples to 0..1), this keeps super-white highlights
/// and demosaic undershoot, so the editing source matches full-res export.
fn downscale_linear(flat: Vec<f32>, w: u32, h: u32, edge: u32) -> (u32, u32, Vec<f32>) {
    let long = w.max(h);
    if long <= edge || w == 0 || h == 0 {
        return (w, h, flat);
    }
    let (nw, nh) = if w >= h {
        (edge, ((h as u64 * edge as u64) / w as u64).max(1) as u32)
    } else {
        (((w as u64 * edge as u64) / h as u64).max(1) as u32, edge)
    };
    let (wu, nwu) = (w as usize, nw as usize);
    assert_eq!(flat.len(), wu * h as usize * 3, "demosaic dims");
    // Vertical pass: h rows → nh rows at source width.
    let mut tall = vec![0f32; wu * nh as usize * 3];
    for (orow, (top, ws)) in tall.chunks_exact_mut(wu * 3).zip(tent_taps(h, nh)) {
        for (k, wt) in ws.into_iter().enumerate() {
            let s = (top + k) * wu * 3;
            for (d, v) in orow.iter_mut().zip(&flat[s..s + wu * 3]) {
                *d += wt * v;
            }
        }
    }
    drop(flat);
    // Horizontal pass: w columns → nw columns.
    let taps = tent_taps(w, nw);
    let mut out = vec![0f32; nwu * nh as usize * 3];
    for (orow, srow) in out.chunks_exact_mut(nwu * 3).zip(tall.chunks_exact(wu * 3)) {
        for (opx, (left, ws)) in orow.chunks_exact_mut(3).zip(&taps) {
            let mut acc = [0f32; 3];
            for (k, wt) in ws.iter().enumerate() {
                let s = (left + k) * 3;
                acc[0] += wt * srow[s];
                acc[1] += wt * srow[s + 1];
                acc[2] += wt * srow[s + 2];
            }
            opx.copy_from_slice(&acc);
        }
    }
    (nw, nh, out)
}

const CACHE_MAGIC: &[u8; 8] = b"LAIKAF16";
/// Same layout; the source is display-referred (see `LinearImage`).
const CACHE_MAGIC_DISPLAY: &[u8; 8] = b"LAIKAD16";
/// Magic + width + height + 3 WB + 9 matrix floats.
const CACHE_HEADER: usize = 8 + 4 + 4 + 12 + 36;

fn write_cache(path: &Path, img: &LinearImage) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut buf = Vec::with_capacity(CACHE_HEADER + img.rgb_f16.len() * 2);
    buf.extend_from_slice(if img.display_referred {
        CACHE_MAGIC_DISPLAY
    } else {
        CACHE_MAGIC
    });
    buf.extend_from_slice(&img.width.to_le_bytes());
    buf.extend_from_slice(&img.height.to_le_bytes());
    buf.extend_from_slice(&img.wb_as_shot[0].to_le_bytes());
    buf.extend_from_slice(&img.wb_as_shot[1].to_le_bytes());
    buf.extend_from_slice(&img.wb_as_shot[2].to_le_bytes());
    for m in &img.cam_to_xyz {
        for v in m {
            buf.extend_from_slice(&v.to_le_bytes());
        }
    }
    buf.resize(CACHE_HEADER + img.rgb_f16.len() * 2, 0);
    for (d, px) in buf[CACHE_HEADER..].chunks_exact_mut(2).zip(&img.rgb_f16) {
        d.copy_from_slice(&px.to_le_bytes());
    }
    // Write-then-rename: a concurrent reader (or a crash mid-write) never
    // sees a partially written cache under the final name.
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(format!(".tmp-{}-{seq}", std::process::id()));
    let tmp = std::path::PathBuf::from(tmp);
    std::fs::write(&tmp, buf)
        .and_then(|_| std::fs::rename(&tmp, path))
        .map_err(|e| {
            std::fs::remove_file(&tmp).ok();
            e.to_string()
        })
}

fn read_cache(path: &Path) -> Result<LinearImage, String> {
    let buf = std::fs::read(path).map_err(|e| e.to_string())?;
    if buf.len() < CACHE_HEADER {
        return Err("bad cache".into());
    }
    let display_referred = match &buf[..8] {
        m if m == CACHE_MAGIC => false,
        m if m == CACHE_MAGIC_DISPLAY => true,
        _ => return Err("bad cache".into()),
    };
    let u32_at = |o: usize| u32::from_le_bytes(buf[o..o + 4].try_into().unwrap());
    let f32_at = |o: usize| f32::from_le_bytes(buf[o..o + 4].try_into().unwrap());
    let (w, h) = (u32_at(8), u32_at(12));
    // Validate the header against the payload before allocating: a
    // truncated or corrupt file must re-decode, never panic or mis-size.
    let samples = (w as usize)
        .checked_mul(h as usize)
        .and_then(|n| n.checked_mul(3));
    match samples {
        Some(n) if n > 0 && n.checked_mul(2) == Some(buf.len() - CACHE_HEADER) => {}
        _ => return Err("cache size mismatch".into()),
    }
    let wb_as_shot = [f32_at(16), f32_at(20), f32_at(24)];
    let mut cam_to_xyz = [[0f32; 3]; 3];
    for (i, m) in cam_to_xyz.iter_mut().enumerate() {
        for (j, v) in m.iter_mut().enumerate() {
            *v = f32_at(28 + (i * 3 + j) * 4);
        }
    }
    let rgb_f16: Vec<u16> = buf[CACHE_HEADER..]
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    Ok(LinearImage {
        width: w,
        height: h,
        rgb_f16,
        wb_as_shot,
        cam_to_xyz,
        black: 0.0,
        white: 1.0,
        camera: String::new(),
        display_referred,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nef() -> Option<PathBuf> {
        let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/raw/IMG_5442.NEF");
        p.exists().then_some(p)
    }

    #[test]
    fn v09_raster_bridge_caches_editing_source() {
        // V09: the sRGB bridge writes the shared editing cache, so a
        // raster smart preview survives for offline editing/export.
        let dir = std::env::temp_dir().join(format!("laika-bridge-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let jpg = dir.join("a.jpg");
        image::RgbImage::from_pixel(64, 48, image::Rgb([200u8, 100, 50]))
            .save(&jpg)
            .unwrap();
        let img = linear_from_raster(&jpg, Some(2048)).expect("bridge decodes");
        assert_eq!((img.width, img.height), (64, 48));
        assert!(img.display_referred);
        write_editing_cache(&img, "h", &dir);
        // The flag survives the cache, and decode_editing serves the hit.
        let back = decode_editing(&jpg, "h", &dir).expect("cache hit");
        assert!(back.display_referred);
        assert_eq!(back.rgb_f16, img.rgb_f16);
        // Legacy (pre-flag) raster caches are still display-referred by
        // file type; a RAW path's scene-referred entry stays scene-referred.
        let p = dir.join("h").join("linear-2048.f16");
        let mut legacy = std::fs::read(&p).unwrap();
        legacy[..8].copy_from_slice(CACHE_MAGIC);
        std::fs::write(&p, &legacy).unwrap();
        assert!(!read_cache(&p).unwrap().display_referred);
        assert!(decode_editing(&jpg, "h", &dir).unwrap().display_referred);
        assert!(
            !decode_editing(&dir.join("a.nef"), "h", &dir)
                .unwrap()
                .display_referred
        );
        // Full-res bridge keeps native dims; editing size caps at 2048.
        let big = image::RgbImage::from_pixel(3000, 2000, image::Rgb([10u8, 20, 30]));
        big.save(&dir.join("b.jpg")).unwrap();
        let full = linear_from_raster(&dir.join("b.jpg"), None).expect("full bridge");
        assert_eq!((full.width, full.height), (3000, 2000));
        let small = linear_from_raster(&dir.join("b.jpg"), Some(2048)).expect("capped");
        assert_eq!((small.width, small.height), (2048, 1365));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn cache_roundtrip() {
        let img = LinearImage {
            width: 4,
            height: 2,
            rgb_f16: (0..24)
                .map(|i| half::f16::from_f32(i as f32 / 24.).to_bits())
                .collect(),
            wb_as_shot: [2.2, 1.0, 1.3],
            cam_to_xyz: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
            black: 0.0,
            white: 1.0,
            camera: "Test".into(),
            display_referred: false,
        };
        let dir = std::env::temp_dir().join(format!("laika-f16-{}", std::process::id()));
        let p = dir.join("linear-2048.f16");
        write_cache(&p, &img).unwrap();
        let back = read_cache(&p).unwrap();
        assert_eq!((back.width, back.height), (4, 2));
        assert_eq!(back.rgb_f16, img.rgb_f16);
        assert_eq!(back.wb_as_shot, img.wb_as_shot);
        assert_eq!(back.cam_to_xyz, img.cam_to_xyz);
        assert!(!back.display_referred);
        assert_eq!(
            back.pixel_f32(3, 1)[0],
            half::f16::from_f32(21. / 24.).to_f32()
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn cache_rejects_truncated_or_corrupt_headers() {
        let img = LinearImage {
            width: 4,
            height: 2,
            rgb_f16: vec![7; 24],
            wb_as_shot: [1.0; 3],
            cam_to_xyz: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
            black: 0.0,
            white: 1.0,
            camera: String::new(),
            display_referred: false,
        };
        let dir = std::env::temp_dir().join(format!("laika-f16-bad-{}", std::process::id()));
        let p = dir.join("linear-2048.f16");
        write_cache(&p, &img).unwrap();
        let good = std::fs::read(&p).unwrap();
        // Truncated payload (odd and even byte counts).
        std::fs::write(&p, &good[..good.len() - 1]).unwrap();
        assert!(read_cache(&p).is_err());
        std::fs::write(&p, &good[..good.len() - 2]).unwrap();
        assert!(read_cache(&p).is_err());
        // Header claims enormous dimensions: no huge allocation, no panic.
        let mut huge = good.clone();
        huge[8..12].copy_from_slice(&u32::MAX.to_le_bytes());
        huge[12..16].copy_from_slice(&u32::MAX.to_le_bytes());
        std::fs::write(&p, &huge).unwrap();
        assert!(read_cache(&p).is_err());
        // Zero-sized header.
        let mut zero = good[..CACHE_HEADER].to_vec();
        zero[8..12].copy_from_slice(&0u32.to_le_bytes());
        std::fs::write(&p, &zero).unwrap();
        assert!(read_cache(&p).is_err());
        std::fs::write(&p, &good).unwrap();
        assert_eq!(read_cache(&p).unwrap().rgb_f16, img.rgb_f16);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn downscale_matches_triangle_and_keeps_highlights() {
        let (w, h) = (301u32, 157u32);
        let flat: Vec<f32> = (0..w * h * 3)
            .map(|i| ((i * 7919) % 1000) as f32 / 1000.)
            .collect();
        let src: image::ImageBuffer<image::Rgb<f32>, Vec<f32>> =
            image::ImageBuffer::from_raw(w, h, flat.clone()).unwrap();
        let reference =
            image::imageops::resize(&src, 64, 33, image::imageops::FilterType::Triangle);
        let (nw, nh, out) = downscale_linear(flat, w, h, 64);
        assert_eq!((nw, nh), (64, 33));
        for (a, b) in out.iter().zip(reference.as_raw()) {
            assert!((a - b).abs() < 1e-4, "{a} vs {b}");
        }
        // Super-white linear values survive (image's resize clamps at 1.0).
        let bright = vec![4.0f32; 300 * 200 * 3];
        let (_, _, out) = downscale_linear(bright, 300, 200, 100);
        assert!(out.iter().all(|v| (v - 4.0).abs() < 1e-3));
    }

    #[test]
    fn decode_nef_linear() {
        let Some(path) = nef() else { return };
        let t = std::time::Instant::now();
        let img = decode(&path).expect("nef decodes");
        eprintln!(
            "full decode: {:?} ({}x{})",
            t.elapsed(),
            img.width,
            img.height
        );
        assert!(img.width >= 6000 && img.height >= 4000);
        assert_eq!(img.rgb_f16.len(), (img.width * img.height * 3) as usize);
        assert!(img.wb_as_shot.iter().all(|&v| v > 0.5 && v < 4.));
        // Linear data: mostly mid values, some highlights near/above white.
        let mut max = 0f32;
        let mut mean = 0f64;
        let n = 200_000;
        for i in (0..img.rgb_f16.len()).step_by(img.rgb_f16.len() / n) {
            let v = half::f16::from_bits(img.rgb_f16[i]).to_f32();
            // PPG demosaic overshoots slightly below black; large negatives
            // or NaN would mean corrupt data.
            assert!(v.is_finite() && v >= -0.1, "sane value, got {v}");
            max = max.max(v);
            mean += (v.max(0.)) as f64;
        }
        mean /= n as f64;
        eprintln!("sampled max={max:.2} mean={mean:.3}");
        assert!(max > 0.5, "highlights present");
        assert!(mean > 0.005 && mean < 0.8, "non-blank exposure range");
    }

    #[test]
    fn decode_editing_caches() {
        let Some(path) = nef() else { return };
        let dir = std::env::temp_dir().join(format!("laika-edit-{}.ignore", std::process::id()));
        let t = std::time::Instant::now();
        let img = decode_editing(&path, "testhash", &dir).expect("editing decode");
        let first = t.elapsed();
        assert!(img.width.max(img.height) <= 2048);
        let t = std::time::Instant::now();
        let img2 = decode_editing(&path, "testhash", &dir).expect("cached decode");
        let second = t.elapsed();
        eprintln!("editing first={first:?} cached={second:?}");
        assert_eq!(img.rgb_f16, img2.rgb_f16);
        // Cache hit must avoid re-decode, not hit a fixed latency: the Pi
        // reads the ~17 MB file in ~0.6 s off SD in debug builds.
        assert!(second.as_millis() < 2000, "cache hit is fast");
        std::fs::remove_dir_all(&dir).ok();
    }

    use std::path::PathBuf;
}
