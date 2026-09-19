//! S02: develop presets — Laika's own, and ones imported from Adobe
//! Lightroom / Camera Raw.
//!
//! A preset is sparse: it lists only the settings it includes (Adobe's
//! "include" checkboxes), so applying it leaves everything else alone.
//! Imported presets carry an honest account of coverage: settings Laika
//! renders, settings it approximates (named), and settings it skips.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::edit::PARAM_COUNT;
use serde::{Deserialize, Serialize};

/// User-facing groups used by Copy Settings and Laika preset creation.
/// Crop geometry and local masks are intentionally absent: neither is a
/// develop parameter and neither can be copied accidentally.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettingGroup {
    pub name: &'static str,
    pub range: std::ops::Range<usize>,
    pub default_on: bool,
}

pub const SETTING_GROUPS: [SettingGroup; 9] = [
    SettingGroup {
        name: "White balance",
        range: 0..2,
        default_on: true,
    },
    SettingGroup {
        name: "Tone",
        range: 2..8,
        default_on: true,
    },
    SettingGroup {
        name: "Presence & color",
        range: 8..12,
        default_on: true,
    },
    SettingGroup {
        name: "Tone curve",
        range: 12..16,
        default_on: true,
    },
    SettingGroup {
        name: "Color mix",
        range: 16..40,
        default_on: true,
    },
    SettingGroup {
        name: "Detail",
        range: 40..44,
        default_on: true,
    },
    SettingGroup {
        name: "Optics",
        range: 44..46,
        default_on: true,
    },
    SettingGroup {
        name: "Effects & grading",
        range: 46..63,
        default_on: true,
    },
    // Transform is geometry-adjacent and therefore opt-in.
    SettingGroup {
        name: "Transform",
        range: 63..70,
        default_on: false,
    },
];

pub fn default_setting_mask() -> [bool; PARAM_COUNT] {
    let mut mask = [false; PARAM_COUNT];
    for group in SETTING_GROUPS {
        if group.default_on {
            for i in group.range {
                mask[i] = true;
            }
        }
    }
    mask
}

pub fn sparse_values(
    values: &[f32; PARAM_COUNT],
    include: &[bool; PARAM_COUNT],
) -> BTreeMap<usize, f32> {
    values
        .iter()
        .copied()
        .enumerate()
        .filter(|(i, v)| include[*i] && v.is_finite())
        .collect()
}

/// Overlay only selected develop settings. The source is never mutated and
/// every excluded target value remains byte-for-byte unchanged.
pub fn apply_selected_settings(
    base: &[f32; PARAM_COUNT],
    source: &[f32; PARAM_COUNT],
    include: &[bool; PARAM_COUNT],
) -> [f32; PARAM_COUNT] {
    let mut out = *base;
    for i in 0..PARAM_COUNT {
        if include[i] && source[i].is_finite() {
            let def = &crate::edit::PARAMS[i];
            out[i] = source[i].clamp(def.min, def.max);
        }
    }
    out
}

/// Curve params: Laika's point curve outputs at x = 20/40/60/80 %.
const CURVE: [usize; 4] = [12, 13, 14, 15];
const SATURATION: usize = 11;

#[derive(Clone, Debug, PartialEq, Default)]
pub struct DevelopPreset {
    /// Catalog row id (0 until stored).
    pub id: i64,
    pub name: String,
    pub group: String,
    /// Included settings: param index → value.
    pub values: BTreeMap<usize, f32>,
    /// Adobe's amount slider applies (blend from the photo's settings).
    pub supports_amount: bool,
    /// Settings in the preset that Laika doesn't render.
    pub skipped: Vec<String>,
    /// Settings Laika renders only approximately, and how.
    pub approximations: Vec<String>,
    /// File the preset came from ("" for Laika's own).
    pub source: String,
    /// SHA-256 of the source file (re-import detection).
    pub digest: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Coverage {
    /// Every included setting renders in Laika.
    Complete,
    /// Some settings render, some are approximated or skipped (named).
    Approximate,
    /// Nothing in the preset renders in Laika.
    Unsupported,
}

impl Coverage {
    pub fn label(self) -> &'static str {
        match self {
            Coverage::Complete => "Complete",
            Coverage::Approximate => "Approximate",
            Coverage::Unsupported => "Unsupported",
        }
    }
}

impl DevelopPreset {
    pub fn coverage(&self) -> Coverage {
        if self.values.is_empty() {
            Coverage::Unsupported
        } else if self.skipped.is_empty() && self.approximations.is_empty() {
            Coverage::Complete
        } else {
            Coverage::Approximate
        }
    }

    /// Settings after applying the preset at `amount` (1.0 = as saved).
    /// Only included settings change; amount blends each from `base`.
    pub fn apply(&self, base: &[f32; PARAM_COUNT], amount: f32) -> [f32; PARAM_COUNT] {
        let mut out = *base;
        let amount = if self.supports_amount {
            amount.clamp(0., 2.)
        } else {
            1.
        };
        for (&i, &v) in &self.values {
            if i >= PARAM_COUNT || !v.is_finite() {
                continue;
            }
            let def = &crate::edit::PARAMS[i];
            out[i] = (base[i] + (v - base[i]) * amount).clamp(def.min, def.max);
        }
        out
    }

    /// One line for tooltips and reports: what didn't come across.
    pub fn coverage_note(&self) -> String {
        let mut parts = Vec::new();
        if !self.skipped.is_empty() {
            parts.push(format!("Not rendered: {}", self.skipped.join(", ")));
        }
        if !self.approximations.is_empty() {
            parts.push(format!("Approximated: {}", self.approximations.join("; ")));
        }
        parts.join(". ")
    }
}

const LAIKA_PRESET_FORMAT: &str = "laika-develop-preset";

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct LaikaPresetFile {
    format: String,
    version: u32,
    name: String,
    #[serde(default)]
    group: String,
    settings: BTreeMap<usize, f32>,
}

/// Encode a portable, human-readable Laika preset.
pub fn export_laika_preset(preset: &DevelopPreset) -> Result<Vec<u8>, String> {
    validate_laika_preset(preset)?;
    serde_json::to_vec_pretty(&LaikaPresetFile {
        format: LAIKA_PRESET_FORMAT.to_string(),
        version: 1,
        name: preset.name.trim().to_string(),
        group: preset.group.trim().to_string(),
        settings: preset.values.clone(),
    })
    .map_err(|e| format!("encode Laika preset: {e}"))
}

/// Decode the native `.laikapreset` interchange format. Errors are written
/// for people, because the import review surfaces them verbatim.
pub fn import_laika_preset(bytes: &[u8]) -> Result<DevelopPreset, String> {
    let file: LaikaPresetFile =
        serde_json::from_slice(bytes).map_err(|e| format!("invalid Laika preset JSON: {e}"))?;
    if file.format != LAIKA_PRESET_FORMAT {
        return Err(format!(
            "not a Laika develop preset (expected format {LAIKA_PRESET_FORMAT})"
        ));
    }
    if file.version != 1 {
        return Err(format!(
            "unsupported Laika preset version {} (this app supports version 1)",
            file.version
        ));
    }
    let preset = DevelopPreset {
        name: file.name,
        group: file.group,
        values: file.settings,
        ..Default::default()
    };
    validate_laika_preset(&preset)?;
    Ok(preset)
}

fn validate_laika_preset(preset: &DevelopPreset) -> Result<(), String> {
    if preset.name.trim().is_empty() {
        return Err("Laika preset is missing a name".to_string());
    }
    if preset.values.is_empty() {
        return Err("Laika preset includes no develop settings".to_string());
    }
    for (&i, &value) in &preset.values {
        if i >= PARAM_COUNT {
            return Err(format!("Laika preset contains unknown setting index {i}"));
        }
        if !value.is_finite() {
            return Err(format!("Laika preset setting {i} is not a finite number"));
        }
        let def = &crate::edit::PARAMS[i];
        if value < def.min || value > def.max {
            return Err(format!(
                "Laika preset setting {} is outside its supported range {}…{}",
                def.label, def.min, def.max
            ));
        }
    }
    Ok(())
}

/// Lightroom metadata preset fields Laika has.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct MetadataFields {
    pub name: String,
    pub title: String,
    pub caption: String,
    pub headline: String,
    pub creator: String,
    pub copyright: String,
    pub rights: String,
    pub contact: String,
    pub location: String,
    pub keywords: String,
    /// Fields in the preset Laika has no place for.
    pub skipped: Vec<String>,
}

/// What a preset file turned out to be.
#[derive(Clone, Debug, PartialEq)]
pub enum PresetFile {
    Develop(DevelopPreset),
    Metadata(MetadataFields),
    /// A camera or creative profile: Laika has no profiles; the nearest
    /// built-in look is suggested, never substituted.
    Profile {
        name: String,
        group: String,
        suggestion: String,
    },
    /// Another Lightroom template kind (filename, export, …).
    Other {
        name: String,
        kind: String,
    },
}

/// Places Adobe apps keep presets on this platform, with a label.
pub fn default_sources(home: &Path) -> Vec<(String, PathBuf)> {
    #[cfg(target_os = "windows")]
    {
        let roaming = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join("AppData/Roaming"));
        let mut out = vec![
            (
                "Camera Raw and Lightroom presets".to_string(),
                roaming.join("Adobe/CameraRaw/Settings"),
            ),
            (
                "Lightroom Classic develop presets".to_string(),
                roaming.join("Adobe/Lightroom/Develop Presets"),
            ),
            (
                "Lightroom Classic metadata presets".to_string(),
                roaming.join("Adobe/Lightroom/Metadata Presets"),
            ),
        ];
        if let Some(program_data) = std::env::var_os("PROGRAMDATA") {
            out.push((
                "Shared Adobe Camera Raw presets".to_string(),
                PathBuf::from(program_data).join("Adobe/CameraRaw/Settings"),
            ));
        }
        return out.into_iter().filter(|(_, p)| p.is_dir()).collect();
    }

    #[cfg(target_os = "macos")]
    {
        let support = home.join("Library/Application Support/Adobe");
        let mut out = vec![
            (
                "Camera Raw and Lightroom presets".to_string(),
                support.join("CameraRaw/Settings"),
            ),
            (
                "Lightroom Classic develop presets".to_string(),
                support.join("Lightroom/Develop Presets"),
            ),
            (
                "Lightroom Classic metadata presets".to_string(),
                support.join("Lightroom/Metadata Presets"),
            ),
        ];
        for (label, app) in [
            (
                "Adobe presets from Lightroom",
                "/Applications/Adobe Lightroom CC/Adobe Lightroom.app",
            ),
            (
                "Adobe presets from Lightroom Classic",
                "/Applications/Adobe Lightroom Classic/Adobe Lightroom Classic.app",
            ),
        ] {
            out.push((
                label.to_string(),
                PathBuf::from(app).join("Contents/Resources/Settings/Adobe/Presets"),
            ));
        }
        return out.into_iter().filter(|(_, p)| p.is_dir()).collect();
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let support = home.join(".config/Adobe");
        [
            (
                "Camera Raw and Lightroom presets".to_string(),
                support.join("CameraRaw/Settings"),
            ),
            (
                "Lightroom Classic develop presets".to_string(),
                support.join("Lightroom/Develop Presets"),
            ),
            (
                "Lightroom Classic metadata presets".to_string(),
                support.join("Lightroom/Metadata Presets"),
            ),
        ]
        .into_iter()
        .filter(|(_, p)| p.is_dir())
        .collect()
    }
}

/// Preset files under a path (a file, or a folder searched recursively).
pub fn preset_files(path: &Path) -> Vec<PathBuf> {
    let is_preset = |p: &Path| {
        p.extension().and_then(|e| e.to_str()).is_some_and(|e| {
            e.eq_ignore_ascii_case("xmp")
                || e.eq_ignore_ascii_case("lrtemplate")
                || e.eq_ignore_ascii_case("laikapreset")
        })
    };
    if path.is_file() {
        return if is_preset(path) {
            vec![path.to_path_buf()]
        } else {
            Vec::new()
        };
    }
    let mut out = Vec::new();
    let mut stack = vec![path.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if e.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            match e.file_type() {
                Ok(t) if t.is_dir() => stack.push(p),
                Ok(t) if t.is_file() && is_preset(&p) => out.push(p),
                _ => {}
            }
        }
    }
    out.sort();
    out
}

/// Read one preset file.
pub fn read_file(path: &Path) -> Result<PresetFile, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let digest = crate::lightroom::sha256_bytes(&bytes);
    let folder = path
        .parent()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let stem = path
        .file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default();
    if extension.eq_ignore_ascii_case("laikapreset") {
        let mut preset = import_laika_preset(&bytes)?;
        preset.source = path.display().to_string();
        preset.digest = digest;
        return Ok(PresetFile::Develop(preset));
    }
    let is_template = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("lrtemplate"));
    let mut file = if is_template {
        parse_lrtemplate(&String::from_utf8_lossy(&bytes), &stem, &folder)?
    } else {
        parse_xmp_preset(&bytes, &stem, &folder)?
    };
    if let PresetFile::Develop(p) = &mut file {
        p.source = path.display().to_string();
        p.digest = digest;
    }
    Ok(file)
}

// ---- Adobe XMP presets -----------------------------------------------------------

/// Parse an Adobe develop preset / profile (`.xmp`).
pub fn parse_xmp_preset(
    bytes: &[u8],
    fallback_name: &str,
    folder: &str,
) -> Result<PresetFile, String> {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;

    let mut attrs: Vec<(String, String)> = Vec::new();
    let mut texts: BTreeMap<String, String> = BTreeMap::new();
    let mut curves: BTreeMap<String, Vec<(f32, f32)>> = BTreeMap::new();
    let mut look_name = String::new();
    let mut look_gray = false;
    let mut elements: Vec<String> = Vec::new();
    let mut r = Reader::from_reader(bytes);
    let mut depth = 0usize;
    let mut stack: Vec<String> = Vec::new();
    let mut text = String::new();
    let mut seen_rdf = false;
    loop {
        match r.read_event() {
            Ok(ev @ (Event::Start(_) | Event::Empty(_))) => {
                let (e, empty) = match &ev {
                    Event::Start(e) => (e.clone(), false),
                    Event::Empty(e) => (e.clone(), true),
                    _ => unreachable!(),
                };
                let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                if name == "rdf:RDF" {
                    seen_rdf = true;
                }
                let parent = stack.last().cloned().unwrap_or_default();
                if name == "rdf:Description" {
                    depth += 1;
                }
                let in_look = stack.iter().any(|s| s == "crs:Look");
                for a in e.attributes().flatten() {
                    let key = String::from_utf8_lossy(a.key.as_ref()).into_owned();
                    let val = a.unescape_value().unwrap_or_default().into_owned();
                    if in_look {
                        if key == "crs:Name" && parent == "crs:Look" {
                            look_name = val;
                        } else if key == "crs:ConvertToGrayscale"
                            && val.eq_ignore_ascii_case("true")
                        {
                            look_gray = true;
                        }
                    } else if depth == 1 && name == "rdf:Description" {
                        if let Some(k) = key.strip_prefix("crs:") {
                            attrs.push((k.to_string(), val));
                        }
                    }
                }
                if depth == 1 && !in_look {
                    if let Some(k) = name.strip_prefix("crs:") {
                        if name != "crs:Look" {
                            elements.push(k.to_string());
                        }
                    }
                }
                if !empty {
                    stack.push(name);
                    text.clear();
                } else if name == "rdf:Description" {
                    depth = depth.saturating_sub(1);
                }
            }
            Ok(Event::Text(t)) => text.push_str(&String::from_utf8_lossy(&t)),
            Ok(Event::GeneralRef(g)) => {
                let entity = String::from_utf8_lossy(&g);
                text.push_str(match entity.as_ref() {
                    "amp" => "&",
                    "lt" => "<",
                    "gt" => ">",
                    "quot" => "\"",
                    "apos" => "'",
                    _ => "",
                });
            }
            Ok(Event::End(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                stack.pop();
                let in_look = stack.iter().any(|s| s == "crs:Look");
                if name == "rdf:li" && !in_look {
                    // Owner property: the nearest crs: ancestor.
                    if let Some(owner) = stack.iter().rev().find(|s| s.starts_with("crs:")) {
                        let owner = owner.trim_start_matches("crs:").to_string();
                        if owner.starts_with("ToneCurvePV2012") {
                            let nums: Vec<f32> = text
                                .split(',')
                                .filter_map(|n| n.trim().parse().ok())
                                .collect();
                            if nums.len() == 2 {
                                curves.entry(owner).or_default().push((nums[0], nums[1]));
                            }
                        } else {
                            texts
                                .entry(owner)
                                .or_insert_with(|| text.trim().to_string());
                        }
                    }
                }
                if name == "rdf:Description" {
                    depth = depth.saturating_sub(1);
                }
                text.clear();
            }
            Ok(Event::Eof) => break,
            Err(e) => return Err(format!("not a readable preset: {e}")),
            _ => {}
        }
    }
    if !seen_rdf {
        return Err("not an XMP preset".to_string());
    }
    let get = |k: &str| attrs.iter().find(|(a, _)| a == k).map(|(_, v)| v.as_str());
    let name = texts
        .get("Name")
        .filter(|n| !n.is_empty())
        .cloned()
        .or_else(|| get("Name").map(str::to_string))
        .unwrap_or_else(|| fallback_name.to_string());
    let group = texts
        .get("Group")
        .filter(|g| !g.is_empty())
        .cloned()
        .or_else(|| get("Group").map(str::to_string))
        .unwrap_or_else(|| folder.to_string());
    let preset_type = get("PresetType").unwrap_or("Normal");
    if preset_type == "Look" {
        let suggestion = suggest_look(
            &name,
            look_gray || get("ConvertToGrayscale") == Some("True"),
        );
        return Ok(PresetFile::Profile {
            name,
            group,
            suggestion,
        });
    }
    if get("HasSettings").is_none()
        && attrs.iter().all(|(k, _)| !is_setting_key(k))
        && curves.is_empty()
    {
        return Err("no develop settings in this file".to_string());
    }

    let mut p = DevelopPreset {
        name,
        group,
        supports_amount: get("SupportsAmount").is_some_and(|v| v.eq_ignore_ascii_case("true")),
        ..Default::default()
    };
    let skip = |s: String, p: &mut DevelopPreset| {
        if !p.skipped.contains(&s) {
            p.skipped.push(s);
        }
    };
    let white_balance = get("WhiteBalance");
    for (k, v) in &attrs {
        if let Some((_, idx)) = crate::xmp::PARAM_KEYS.iter().find(|(key, _)| key == k) {
            // Temperature/Tint count only with a custom white balance.
            if matches!(k.as_str(), "Temperature" | "Tint")
                && !matches!(white_balance, Some("Custom") | None)
            {
                continue;
            }
            if let Some(x) = parse_number(v) {
                p.values.insert(*idx, x);
            }
        } else if k == "ConvertToGrayscale" {
            // Handled below (approximated, not skipped).
        } else if let Some(label) = crate::lightroom::attr_label(k, v) {
            skip(label, &mut p);
        }
    }
    for e in &elements {
        if let Some(label) = crate::lightroom::element_label(e) {
            skip(label.to_string(), &mut p);
        }
    }
    if !look_name.is_empty() && !crate::lightroom::is_default_profile(&look_name) {
        skip(format!("Profile: {look_name}"), &mut p);
    }
    for (name, points) in &curves {
        let identity = points.iter().all(|(x, y)| (x - y).abs() < 0.5);
        if name == "ToneCurvePV2012" {
            if !points.is_empty() {
                let (ys, exact) = sample_curve(points);
                for (k, y) in ys.iter().enumerate() {
                    p.values.insert(CURVE[k], *y);
                }
                if !exact && !identity {
                    p.approximations
                        .push("Point tone curve sampled at 20/40/60/80 %".to_string());
                }
            }
        } else if !identity {
            skip("RGB channel tone curves".to_string(), &mut p);
        }
    }
    // Stubbed looks carry only a name: Adobe's B&W / Monochrome profiles
    // convert to grayscale.
    let look_is_mono = {
        let n = look_name.to_lowercase();
        n.starts_with("b&w") || n.contains("monochrome")
    };
    let gray = get("ConvertToGrayscale").is_some_and(|v| v.eq_ignore_ascii_case("true"))
        || look_gray
        || look_is_mono;
    if gray {
        p.values.insert(SATURATION, -100.);
        p.approximations
            .push("Black & white conversion rendered as Saturation −100".to_string());
    }
    Ok(PresetFile::Develop(p))
}

fn is_setting_key(k: &str) -> bool {
    crate::xmp::PARAM_KEYS.iter().any(|(key, _)| *key == k)
        || crate::lightroom::attr_label(k, "1").is_some()
}

fn parse_number(v: &str) -> Option<f32> {
    v.trim()
        .trim_start_matches('+')
        .replace('\u{2212}', "-")
        .parse::<f32>()
        .ok()
        .filter(|x| x.is_finite())
}

/// Sample an Adobe point curve (0–255) at Laika's four control x's.
/// Exact when the curve's own points already sit on those x's (or it's a
/// straight segment through them).
fn sample_curve(points: &[(f32, f32)]) -> ([f32; 4], bool) {
    let mut pts: Vec<(f32, f32)> = points.to_vec();
    pts.sort_by(|a, b| a.0.total_cmp(&b.0));
    let xs = [51., 102., 153., 204.];
    let at = |x: f32| -> f32 {
        if pts.is_empty() {
            return x;
        }
        if x <= pts[0].0 {
            return pts[0].1;
        }
        for w in pts.windows(2) {
            let (a, b) = (w[0], w[1]);
            if x <= b.0 {
                let t = if b.0 > a.0 {
                    (x - a.0) / (b.0 - a.0)
                } else {
                    0.
                };
                return a.1 + (b.1 - a.1) * t;
            }
        }
        pts[pts.len() - 1].1
    };
    let mut out = [0f32; 4];
    for (k, x) in xs.iter().enumerate() {
        out[k] = (at(*x) / 255.).clamp(0., 1.);
    }
    // Exact only when the curve's inner points are Laika's four nodes, or
    // the curve is a straight line; otherwise the samples approximate
    // Adobe's spline.
    let inner: Vec<f32> = pts
        .iter()
        .map(|(x, _)| *x)
        .filter(|x| *x > 0. && *x < 255.)
        .collect();
    let on_grid = inner.len() == 4 && inner.iter().zip(xs).all(|(x, g)| (x - g).abs() < 0.5);
    let collinear = pts.windows(3).all(|w| {
        let (a, b, c) = (w[0], w[1], w[2]);
        ((b.1 - a.1) * (c.0 - a.0) - (c.1 - a.1) * (b.0 - a.0)).abs() < 1.
    });
    (out, collinear || on_grid)
}

/// Nearest Laika built-in look for a profile, by name.
fn suggest_look(name: &str, gray: bool) -> String {
    let n = name.to_lowercase();
    if gray || n.contains("b&w") || n.contains("monochrome") || n.contains("black") {
        "Mono contrast"
    } else if n.contains("vivid") || n.contains("landscape") || n.contains("warm") {
        "Golden hour"
    } else if n.contains("portrait") {
        "Portra warm"
    } else {
        "Neutral RAW"
    }
    .to_string()
}

// ---- Lightroom Classic .lrtemplate (Lua) -------------------------------------------

/// Parse a Lightroom Classic template: `s = { title = "…", type = "Develop",
/// value = { settings = { Exposure2012 = 0.5, … } } }`.
pub fn parse_lrtemplate(
    text: &str,
    fallback_name: &str,
    folder: &str,
) -> Result<PresetFile, String> {
    let start = text.find('{').ok_or("not a Lightroom template")?;
    let mut lua = Lua {
        s: text.as_bytes(),
        i: start,
    };
    let root = lua.value(0)?;
    let get = |v: &serde_json::Value, k: &str| v.get(k).cloned();
    let title = get(&root, "title")
        .and_then(|t| t.as_str().map(clean_lr_string))
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| fallback_name.to_string());
    let kind = get(&root, "type")
        .and_then(|t| t.as_str().map(str::to_string))
        .unwrap_or_default();
    let value = get(&root, "value").unwrap_or(serde_json::Value::Null);
    match kind.as_str() {
        "Develop" => {
            let settings = value
                .get("settings")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            let obj = settings
                .as_object()
                .ok_or("the develop preset has no settings")?;
            let mut p = DevelopPreset {
                name: title,
                group: folder.to_string(),
                // Classic presets have no amount slider.
                supports_amount: false,
                ..Default::default()
            };
            let wb = obj
                .get("WhiteBalance")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            for (k, v) in obj {
                let sval = match v {
                    serde_json::Value::String(s) => s.clone(),
                    serde_json::Value::Bool(b) => if *b { "True" } else { "False" }.to_string(),
                    // Lua numbers arrive as floats; Camera Raw writes
                    // whole numbers without a fraction ("1", not "1.0").
                    serde_json::Value::Number(n) => match n.as_f64() {
                        Some(f) if f.fract() == 0. && f.abs() < 1e15 => format!("{}", f as i64),
                        _ => n.to_string(),
                    },
                    _ => String::new(),
                };
                if let Some((_, idx)) = crate::xmp::PARAM_KEYS.iter().find(|(key, _)| key == k) {
                    if matches!(k.as_str(), "Temperature" | "Tint")
                        && !matches!(wb.as_deref(), Some("Custom") | None)
                    {
                        continue;
                    }
                    if let Some(x) = parse_number(&sval) {
                        p.values.insert(*idx, x);
                    }
                } else if k.starts_with("ToneCurvePV2012") {
                    let nums: Vec<f32> = v
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(|n| n.as_f64())
                                .map(|n| n as f32)
                                .collect()
                        })
                        .unwrap_or_default();
                    let pts: Vec<(f32, f32)> = nums
                        .chunks(2)
                        .filter(|c| c.len() == 2)
                        .map(|c| (c[0], c[1]))
                        .collect();
                    let identity = pts.iter().all(|(x, y)| (x - y).abs() < 0.5);
                    if k == "ToneCurvePV2012" {
                        if !pts.is_empty() {
                            let (ys, exact) = sample_curve(&pts);
                            for (n, y) in ys.iter().enumerate() {
                                p.values.insert(CURVE[n], *y);
                            }
                            if !exact && !identity {
                                p.approximations
                                    .push("Point tone curve sampled at 20/40/60/80 %".to_string());
                            }
                        }
                    } else if !identity && !p.skipped.iter().any(|s| s == "RGB channel tone curves")
                    {
                        p.skipped.push("RGB channel tone curves".to_string());
                    }
                } else if k == "ConvertToGrayscale" && sval == "True" {
                    p.values.insert(SATURATION, -100.);
                    p.approximations
                        .push("Black & white conversion rendered as Saturation −100".to_string());
                } else if let Some(label) = crate::lightroom::attr_label(k, &sval) {
                    if !p.skipped.contains(&label) {
                        p.skipped.push(label);
                    }
                } else if let Some(label) = crate::lightroom::element_label(k) {
                    if v.as_array().is_some_and(|a| !a.is_empty())
                        && !p.skipped.iter().any(|s| s == label)
                    {
                        p.skipped.push(label.to_string());
                    }
                }
            }
            Ok(PresetFile::Develop(p))
        }
        "Metadata" => {
            let obj = value.as_object().ok_or("the metadata preset is empty")?;
            let text = |k: &str| {
                obj.get(k)
                    .and_then(|v| v.as_str())
                    .map(clean_lr_string)
                    .unwrap_or_default()
            };
            let mut m = MetadataFields {
                name: title,
                title: text("title"),
                caption: text("caption"),
                headline: text("headline"),
                creator: text("creator"),
                copyright: text("copyright"),
                rights: text("rightsUsageTerms"),
                contact: {
                    let email = text("creatorEmail");
                    if email.is_empty() {
                        text("creatorUrl")
                    } else {
                        email
                    }
                },
                location: text("location"),
                keywords: text("keywords"),
                skipped: Vec::new(),
            };
            let known = [
                "title",
                "caption",
                "headline",
                "creator",
                "copyright",
                "rightsUsageTerms",
                "creatorEmail",
                "creatorUrl",
                "location",
                "keywords",
                "copyrightState",
                "uuid",
            ];
            for (k, v) in obj {
                let has_value = match v {
                    serde_json::Value::String(s) => !s.is_empty(),
                    serde_json::Value::Null => false,
                    _ => true,
                };
                if has_value && !known.contains(&k.as_str()) {
                    m.skipped.push(k.clone());
                }
            }
            m.skipped.sort();
            Ok(PresetFile::Metadata(m))
        }
        "" => Err("not a Lightroom template".to_string()),
        other => Ok(PresetFile::Other {
            name: title,
            kind: other.to_string(),
        }),
    }
}

/// Lightroom localizes titles as `$$$/Key=Default text`.
fn clean_lr_string(s: &str) -> String {
    match s.strip_prefix("$$$/") {
        Some(rest) => rest
            .split_once('=')
            .map(|(_, v)| v)
            .unwrap_or(rest)
            .to_string(),
        None => s.to_string(),
    }
}

/// Just enough Lua to read a template's table literal.
struct Lua<'a> {
    s: &'a [u8],
    i: usize,
}

impl Lua<'_> {
    fn peek(&self) -> Option<u8> {
        self.s.get(self.i).copied()
    }

    fn skip_ws(&mut self) {
        loop {
            while self.peek().is_some_and(|c| c.is_ascii_whitespace()) {
                self.i += 1;
            }
            if self.s[self.i..].starts_with(b"--") {
                if self.s[self.i + 2..].starts_with(b"[[") {
                    match find(self.s, self.i + 4, b"]]") {
                        Some(end) => self.i = end + 2,
                        None => self.i = self.s.len(),
                    }
                } else {
                    while self.peek().is_some_and(|c| c != b'\n') {
                        self.i += 1;
                    }
                }
                continue;
            }
            break;
        }
    }

    fn value(&mut self, depth: usize) -> Result<serde_json::Value, String> {
        if depth > 64 {
            return Err("template nested too deeply".to_string());
        }
        self.skip_ws();
        match self.peek() {
            Some(b'{') => self.table(depth),
            Some(b'"') | Some(b'\'') => Ok(serde_json::Value::String(self.string()?)),
            Some(b'[') if self.s[self.i..].starts_with(b"[[") => {
                let end = find(self.s, self.i + 2, b"]]").ok_or("unterminated long string")?;
                let v = String::from_utf8_lossy(&self.s[self.i + 2..end]).into_owned();
                self.i = end + 2;
                Ok(serde_json::Value::String(v))
            }
            Some(c) if c == b'-' || c == b'+' || c == b'.' || c.is_ascii_digit() => {
                let start = self.i;
                self.i += 1;
                while self
                    .peek()
                    .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-' | b'+'))
                {
                    self.i += 1;
                }
                let t = String::from_utf8_lossy(&self.s[start..self.i]).into_owned();
                let n: f64 = t
                    .trim_start_matches('+')
                    .parse()
                    .map_err(|_| format!("bad number {t}"))?;
                Ok(serde_json::Number::from_f64(n)
                    .map(serde_json::Value::Number)
                    .unwrap_or(serde_json::Value::Null))
            }
            Some(_) => {
                let word = self.ident();
                match word.as_str() {
                    "true" => Ok(serde_json::Value::Bool(true)),
                    "false" => Ok(serde_json::Value::Bool(false)),
                    "nil" => Ok(serde_json::Value::Null),
                    "" => Err(format!("unexpected character at byte {}", self.i)),
                    // Function calls such as LOC "…": keep the argument.
                    _ => {
                        self.skip_ws();
                        match self.peek() {
                            Some(b'"') | Some(b'\'') | Some(b'{') => self.value(depth + 1),
                            _ => Ok(serde_json::Value::Null),
                        }
                    }
                }
            }
            None => Err("template ended early".to_string()),
        }
    }

    fn ident(&mut self) -> String {
        let start = self.i;
        while self
            .peek()
            .is_some_and(|c| c.is_ascii_alphanumeric() || c == b'_')
        {
            self.i += 1;
        }
        String::from_utf8_lossy(&self.s[start..self.i]).into_owned()
    }

    fn string(&mut self) -> Result<String, String> {
        let quote = self.peek().ok_or("expected a string")?;
        self.i += 1;
        let mut out = Vec::new();
        while let Some(c) = self.peek() {
            self.i += 1;
            if c == quote {
                return Ok(String::from_utf8_lossy(&out).into_owned());
            }
            if c == b'\\' {
                let Some(e) = self.peek() else { break };
                self.i += 1;
                match e {
                    b'n' => out.push(b'\n'),
                    b't' => out.push(b'\t'),
                    b'r' => out.push(b'\r'),
                    d if d.is_ascii_digit() => {
                        let mut n = (d - b'0') as u32;
                        for _ in 0..2 {
                            match self.peek() {
                                Some(x) if x.is_ascii_digit() => {
                                    n = n * 10 + (x - b'0') as u32;
                                    self.i += 1;
                                }
                                _ => break,
                            }
                        }
                        out.push(n.min(255) as u8);
                    }
                    other => out.push(other),
                }
            } else {
                out.push(c);
            }
        }
        Err("unterminated string".to_string())
    }

    fn table(&mut self, depth: usize) -> Result<serde_json::Value, String> {
        self.i += 1; // '{'
        let mut map = serde_json::Map::new();
        let mut list = Vec::new();
        loop {
            self.skip_ws();
            match self.peek() {
                None => return Err("unterminated table".to_string()),
                Some(b'}') => {
                    self.i += 1;
                    break;
                }
                Some(b',') | Some(b';') => {
                    self.i += 1;
                    continue;
                }
                _ => {}
            }
            // key = value | ["key"] = value | value
            let save = self.i;
            let key = if self.peek() == Some(b'[') && !self.s[self.i..].starts_with(b"[[") {
                self.i += 1;
                let k = match self.value(depth + 1)? {
                    serde_json::Value::String(s) => s,
                    other => other.to_string(),
                };
                self.skip_ws();
                if self.peek() != Some(b']') {
                    return Err("expected ]".to_string());
                }
                self.i += 1;
                Some(k)
            } else if self
                .peek()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == b'_')
            {
                let word = self.ident();
                self.skip_ws();
                if self.peek() == Some(b'=') && self.s.get(self.i + 1) != Some(&b'=') {
                    Some(word)
                } else {
                    self.i = save;
                    None
                }
            } else {
                None
            };
            match key {
                Some(k) => {
                    self.skip_ws();
                    if self.peek() != Some(b'=') {
                        return Err("expected =".to_string());
                    }
                    self.i += 1;
                    let v = self.value(depth + 1)?;
                    map.insert(k, v);
                }
                None => list.push(self.value(depth + 1)?),
            }
        }
        if map.is_empty() && !list.is_empty() {
            Ok(serde_json::Value::Array(list))
        } else {
            for (n, v) in list.into_iter().enumerate() {
                map.insert((n + 1).to_string(), v);
            }
            Ok(serde_json::Value::Object(map))
        }
    }
}

fn find(hay: &[u8], from: usize, needle: &[u8]) -> Option<usize> {
    hay.get(from..)?
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|p| p + from)
}

#[cfg(test)]
mod tests {
    use super::*;

    const XMP: &str = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about="" xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
   crs:PresetType="Normal" crs:SupportsAmount="True" crs:Version="10.3"
   crs:GrayMixerRed="-12" crs:Whites2012="+25" crs:Blacks2012="-30" crs:Clarity2012="+20"
   crs:SplitToningBalance="0" crs:WhiteBalance="As Shot" crs:Temperature="6000" crs:HasSettings="True">
   <crs:Name><rdf:Alt><rdf:li xml:lang="x-default">B&amp;W Flat</rdf:li></rdf:Alt></crs:Name>
   <crs:Group><rdf:Alt><rdf:li xml:lang="x-default">B&amp;W</rdf:li></rdf:Alt></crs:Group>
   <crs:ToneCurvePV2012><rdf:Seq><rdf:li>0, 0</rdf:li><rdf:li>64, 50</rdf:li><rdf:li>255, 255</rdf:li></rdf:Seq></crs:ToneCurvePV2012>
   <crs:Look><rdf:Description crs:Name="Adobe Monochrome"><crs:Parameters><rdf:Description crs:ConvertToGrayscale="True">
     <crs:ToneCurvePV2012><rdf:Seq><rdf:li>0, 0</rdf:li><rdf:li>30, 5</rdf:li><rdf:li>255, 255</rdf:li></rdf:Seq></crs:ToneCurvePV2012>
   </rdf:Description></crs:Parameters></rdf:Description></crs:Look>
  </rdf:Description></rdf:RDF></x:xmpmeta>"#;

    fn idx(key: &str) -> usize {
        crate::xmp::PARAM_KEYS
            .iter()
            .find(|(k, _)| *k == key)
            .unwrap()
            .1
    }

    #[test]
    fn adobe_xmp_preset_keeps_only_included_settings_and_names_gaps() {
        let PresetFile::Develop(p) = parse_xmp_preset(XMP.as_bytes(), "file", "folder").unwrap()
        else {
            panic!("develop preset")
        };
        assert_eq!((p.name.as_str(), p.group.as_str()), ("B&W Flat", "B&W"));
        assert!(p.supports_amount);
        assert_eq!(p.values.get(&idx("Whites2012")), Some(&25.));
        assert_eq!(p.values.get(&idx("Blacks2012")), Some(&-30.));
        assert_eq!(
            p.values.get(&idx("SplitToningBalance")),
            Some(&0.),
            "included zeros reset"
        );
        assert!(
            !p.values.contains_key(&idx("Exposure2012")),
            "not included → untouched"
        );
        assert!(
            !p.values.contains_key(&idx("Temperature")),
            "As Shot ignores Temperature"
        );
        // The look's own curve never leaks into the preset's curve.
        assert!(
            (p.values[&13] - (50. + (102. - 64.) * (255. - 50.) / (255. - 64.)) / 255.).abs()
                < 1e-3
        );
        assert_eq!(p.values.get(&idx("Saturation")), Some(&-100.));
        assert!(
            p.skipped.contains(&"Black & white mix".to_string()),
            "{:?}",
            p.skipped
        );
        assert!(
            p.skipped.contains(&"Profile: Adobe Monochrome".to_string()),
            "{:?}",
            p.skipped
        );
        assert_eq!(p.coverage(), Coverage::Approximate);
        assert!(p.coverage_note().contains("Saturation −100"));
    }

    #[test]
    fn amount_blends_only_included_settings() {
        let mut p = DevelopPreset {
            supports_amount: true,
            ..Default::default()
        };
        p.values.insert(2, 1.0); // Exposure +1
        let mut base = crate::edit::defaults();
        base[3] = 30.; // Contrast untouched by the preset
        let half = p.apply(&base, 0.5);
        assert!((half[2] - 0.5).abs() < 1e-6);
        assert_eq!(half[3], 30.);
        let fixed = DevelopPreset {
            supports_amount: false,
            ..p.clone()
        };
        assert!(
            (fixed.apply(&base, 0.5)[2] - 1.0).abs() < 1e-6,
            "no amount slider → as saved"
        );
    }

    #[test]
    fn lightroom_classic_templates_parse() {
        let lua = r#"s = {
            id = "8F4E1C9A",
            internalName = "Punchy",
            title = "$$$/Presets/Punchy=Punchy \"Pop\"",
            type = "Develop",
            value = {
                settings = {
                    Exposure2012 = 0.35,
                    Contrast2012 = 15,
                    WhiteBalance = "Custom",
                    Temperature = 5200,
                    ToneCurvePV2012 = { 0, 0, 51, 40, 102, 95, 153, 160, 204, 214, 255, 255, },
                    ToneCurvePV2012Red = { 0, 0, 255, 255, },
                    LensProfileEnable = 1,
                    ConvertToGrayscale = false,
                    RetouchInfo = { "centerX = 0.5, centerY = 0.5" },
                },
                uuid = "AB12",
            }, -- trailing comment
            version = 0,
        }"#;
        let PresetFile::Develop(p) = parse_lrtemplate(lua, "fallback", "My Pack").unwrap() else {
            panic!("develop")
        };
        assert_eq!(p.name, "Punchy \"Pop\"");
        assert_eq!(p.group, "My Pack");
        assert!((p.values[&idx("Exposure2012")] - 0.35).abs() < 1e-6);
        assert_eq!(p.values[&idx("Temperature")], 5200.);
        assert!((p.values[&12] - 40. / 255.).abs() < 1e-4);
        assert!((p.values[&15] - 214. / 255.).abs() < 1e-4);
        assert!(
            p.approximations.is_empty(),
            "points on Laika's grid are exact: {:?}",
            p.approximations
        );
        assert!(p.skipped.contains(&"Lens profile correction".to_string()));
        assert!(
            p.skipped.contains(&"Healing and spot removal".to_string()),
            "{:?}",
            p.skipped
        );
        assert!(!p.values.contains_key(&idx("Saturation")));

        let meta = r#"s = { title = "Studio", type = "Metadata", value = {
            copyright = "© 2026 Sam", copyrightState = true, creator = "Sam",
            creatorEmail = "sam@example.com", jobIdentifier = "J1", uuid = "X" } }"#;
        let PresetFile::Metadata(m) = parse_lrtemplate(meta, "f", "g").unwrap() else {
            panic!("metadata")
        };
        assert_eq!(
            (m.creator.as_str(), m.contact.as_str()),
            ("Sam", "sam@example.com")
        );
        assert_eq!(m.skipped, vec!["jobIdentifier".to_string()]);

        let other = r#"s = { title = "Date-Seq", type = "Filename", value = { } }"#;
        assert!(matches!(
            parse_lrtemplate(other, "f", "g").unwrap(),
            PresetFile::Other { .. }
        ));
        assert!(parse_lrtemplate("s = { title = ", "f", "g").is_err());
    }

    #[test]
    fn profiles_are_suggested_not_substituted() {
        let look = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
          <rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/" crs:PresetType="Look" crs:ConvertToGrayscale="True">
          <crs:Name><rdf:Alt><rdf:li xml:lang="x-default">B&amp;W 01</rdf:li></rdf:Alt></crs:Name></rdf:Description></rdf:RDF></x:xmpmeta>"#;
        match parse_xmp_preset(look.as_bytes(), "f", "Profiles").unwrap() {
            PresetFile::Profile {
                name, suggestion, ..
            } => {
                assert_eq!(name, "B&W 01");
                assert_eq!(suggestion, "Mono contrast");
            }
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn native_preset_round_trips_and_explains_bad_files() {
        let mut p = DevelopPreset {
            name: "Soft portrait".into(),
            group: "People".into(),
            ..Default::default()
        };
        p.values.insert(2, 0.35);
        p.values.insert(8, -12.0);
        let bytes = export_laika_preset(&p).unwrap();
        let back = import_laika_preset(&bytes).unwrap();
        assert_eq!(back.name, p.name);
        assert_eq!(back.group, p.group);
        assert_eq!(back.values, p.values);

        let bad = br#"{"format":"laika-develop-preset","version":1,"name":"","settings":{}}"#;
        assert!(
            import_laika_preset(bad)
                .unwrap_err()
                .contains("missing a name")
        );
        let future =
            br#"{"format":"laika-develop-preset","version":9,"name":"x","settings":{"2":0.0}}"#;
        assert!(
            import_laika_preset(future)
                .unwrap_err()
                .contains("version 9")
        );
    }

    #[test]
    fn default_copy_mask_excludes_transform() {
        let mask = default_setting_mask();
        assert!(mask[..63].iter().all(|on| *on));
        assert!(mask[63..].iter().all(|on| !*on));
    }

    #[test]
    fn selective_settings_leave_excluded_values_unchanged() {
        let mut source = crate::edit::defaults();
        source[0] = 6200.;
        source[2] = 1.25;
        source[63] = 9.;
        let mut target = crate::edit::defaults();
        target[63] = -4.;
        let out = apply_selected_settings(&target, &source, &default_setting_mask());
        assert_eq!(out[0], 6200.);
        assert_eq!(out[2], 1.25);
        assert_eq!(out[63], -4., "transform is excluded by default");
        assert_eq!(source[63], 9., "copy source is immutable");
    }
}
