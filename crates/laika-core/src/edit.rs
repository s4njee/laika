//! Twelve Basic develop parameters. Ported from the Phase 0 spike
//! (`spikes/p0/src/params.rs`); `default` is the untouched value (white text),
//! anything else reads as modified (green).

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fmt {
    Kelvin,
    Int,
    Ev,
    /// 0..1 fraction, three decimals (tone-curve points).
    Unit,
    /// Plain one-decimal float (sharpen radius).
    Float,
    /// Hue angle in whole degrees (color grading wheels).
    Hue,
}

#[derive(Clone, Copy, Debug)]
pub struct ParamDef {
    pub label: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub step: f32,
    pub fmt: Fmt,
}

const fn p(label: &'static str, min: f32, max: f32, default: f32, step: f32, fmt: Fmt) -> ParamDef {
    ParamDef {
        label,
        min,
        max,
        default,
        step,
        fmt,
    }
}

pub const PARAMS: [ParamDef; PARAM_COUNT] = [
    p("Temperature", 2000., 8960., 5480., 20., Fmt::Kelvin),
    p("Tint", -150., 150., 6., 1., Fmt::Int),
    p("Exposure", -5., 5., 0., 0.05, Fmt::Ev),
    p("Contrast", -100., 100., 0., 1., Fmt::Int),
    p("Highlights", -100., 100., 0., 1., Fmt::Int),
    p("Shadows", -100., 100., 0., 1., Fmt::Int),
    p("Whites", -100., 100., 0., 1., Fmt::Int),
    p("Blacks", -100., 100., 0., 1., Fmt::Int),
    p("Texture", -100., 100., 0., 1., Fmt::Int),
    p("Clarity", -100., 100., 0., 1., Fmt::Int),
    p("Vibrance", -100., 100., 0., 1., Fmt::Int),
    p("Saturation", -100., 100., 0., 1., Fmt::Int),
    // U18: point-curve outputs at fixed 20/40/60/80% (identity defaults).
    p("Curve 20", 0., 1., 0.2, 0.01, Fmt::Unit),
    p("Curve 40", 0., 1., 0.4, 0.01, Fmt::Unit),
    p("Curve 60", 0., 1., 0.6, 0.01, Fmt::Unit),
    p("Curve 80", 0., 1., 0.8, 0.01, Fmt::Unit),
    // U18: Color Mix hue shifts, red→magenta.
    p("Hue Red", -100., 100., 0., 1., Fmt::Int),
    p("Hue Orange", -100., 100., 0., 1., Fmt::Int),
    p("Hue Yellow", -100., 100., 0., 1., Fmt::Int),
    p("Hue Green", -100., 100., 0., 1., Fmt::Int),
    p("Hue Aqua", -100., 100., 0., 1., Fmt::Int),
    p("Hue Blue", -100., 100., 0., 1., Fmt::Int),
    p("Hue Purple", -100., 100., 0., 1., Fmt::Int),
    p("Hue Magenta", -100., 100., 0., 1., Fmt::Int),
    // U18: Color Mix saturation.
    p("Sat Red", -100., 100., 0., 1., Fmt::Int),
    p("Sat Orange", -100., 100., 0., 1., Fmt::Int),
    p("Sat Yellow", -100., 100., 0., 1., Fmt::Int),
    p("Sat Green", -100., 100., 0., 1., Fmt::Int),
    p("Sat Aqua", -100., 100., 0., 1., Fmt::Int),
    p("Sat Blue", -100., 100., 0., 1., Fmt::Int),
    p("Sat Purple", -100., 100., 0., 1., Fmt::Int),
    p("Sat Magenta", -100., 100., 0., 1., Fmt::Int),
    // U18: Color Mix luminance.
    p("Lum Red", -100., 100., 0., 1., Fmt::Int),
    p("Lum Orange", -100., 100., 0., 1., Fmt::Int),
    p("Lum Yellow", -100., 100., 0., 1., Fmt::Int),
    p("Lum Green", -100., 100., 0., 1., Fmt::Int),
    p("Lum Aqua", -100., 100., 0., 1., Fmt::Int),
    p("Lum Blue", -100., 100., 0., 1., Fmt::Int),
    p("Lum Purple", -100., 100., 0., 1., Fmt::Int),
    p("Lum Magenta", -100., 100., 0., 1., Fmt::Int),
    // U18: Detail.
    p("Sharpen", 0., 150., 0., 1., Fmt::Int),
    p("Radius", 0., 3., 1., 0.1, Fmt::Float),
    p("Lum NR", 0., 100., 0., 1., Fmt::Int),
    p("Color NR", 0., 100., 0., 1., Fmt::Int),
    // U18: Optics (manual; no profile database in this build).
    p("Distortion", -100., 100., 0., 1., Fmt::Int),
    p("Defringe CA", -100., 100., 0., 1., Fmt::Int),
    // Effects (Texture 8 and Clarity 9 also show in the Effects panel).
    p("Dehaze", -100., 100., 0., 1., Fmt::Int),
    p("Vignette", -100., 100., 0., 1., Fmt::Int),
    p("Grain", 0., 100., 0., 1., Fmt::Int),
    // Color Grading (Lightroom semantics): hue/sat/lum per range + global,
    // then Blending (range overlap) and Balance (shadow/highlight split).
    p("Shadow Hue", 0., 359., 0., 1., Fmt::Hue),
    p("Shadow Sat", 0., 100., 0., 1., Fmt::Int),
    p("Shadow Lum", -100., 100., 0., 1., Fmt::Int),
    p("Midtone Hue", 0., 359., 0., 1., Fmt::Hue),
    p("Midtone Sat", 0., 100., 0., 1., Fmt::Int),
    p("Midtone Lum", -100., 100., 0., 1., Fmt::Int),
    p("Highlight Hue", 0., 359., 0., 1., Fmt::Hue),
    p("Highlight Sat", 0., 100., 0., 1., Fmt::Int),
    p("Highlight Lum", -100., 100., 0., 1., Fmt::Int),
    p("Global Hue", 0., 359., 0., 1., Fmt::Hue),
    p("Global Sat", 0., 100., 0., 1., Fmt::Int),
    p("Global Lum", -100., 100., 0., 1., Fmt::Int),
    p("Blending", 0., 100., 50., 1., Fmt::Int),
    p("Balance", -100., 100., 0., 1., Fmt::Int),
    // V22 Transform (Lightroom semantics; Upright's solved correction
    // lives in `CropGeom::upright` and adds to these).
    p("Vertical", -100., 100., 0., 1., Fmt::Int),
    p("Horizontal", -100., 100., 0., 1., Fmt::Int),
    p("Rotate", -10., 10., 0., 0.1, Fmt::Float),
    p("Aspect", -100., 100., 0., 1., Fmt::Int),
    p("Scale", 50., 150., 100., 1., Fmt::Int),
    p("X Offset", -100., 100., 0., 1., Fmt::Int),
    p("Y Offset", -100., 100., 0., 1., Fmt::Int),
];

/// U18 panel index ranges over `values`.
pub const CURVE_RANGE: std::ops::Range<usize> = 12..16;
pub const HUE_RANGE: std::ops::Range<usize> = 16..24;
pub const SAT_RANGE: std::ops::Range<usize> = 24..32;
pub const LUM_RANGE: std::ops::Range<usize> = 32..40;
pub const HSL_RANGE: std::ops::Range<usize> = 16..40;
pub const DETAIL_RANGE: std::ops::Range<usize> = 40..44;
pub const OPTICS_RANGE: std::ops::Range<usize> = 44..46;
/// Dehaze, Vignette, Grain (new slots; the panel also shows 8 and 9).
pub const EFFECTS_RANGE: std::ops::Range<usize> = 46..49;
/// Everything the Effects panel shows and resets.
pub const EFFECTS_PARAMS: [usize; 5] = [8, 9, 46, 47, 48];

/// Color Grading: 3 wheels × (hue, sat, lum) at 49/52/55, global at 58,
/// Blending 61, Balance 62.
pub const GRADING_RANGE: std::ops::Range<usize> = 49..63;

/// V22: Transform sliders (Vertical, Horizontal, Rotate, Aspect, Scale,
/// X/Y Offset) — geometry, so tone copy/paste leaves them alone.
pub const TRANSFORM_RANGE: std::ops::Range<usize> = 63..70;

pub const PARAM_COUNT: usize = 70;

/// U20: rendering intent for the camera data. These are Laika profiles,
/// deliberately not Adobe/DCP profile names. `Standard` preserves the
/// historical rendering exactly.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum CameraProfile {
    Neutral,
    Vivid,
    Monochrome,
    #[default]
    #[serde(other)]
    Standard,
}

/// U19: a local adjustment carried by every mask. Values use the same
/// human-facing units as Basic (EV and +/-100 scales).
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LocalAdjustment {
    #[serde(default)]
    pub exposure: f32,
    #[serde(default)]
    pub contrast: f32,
    #[serde(default)]
    pub saturation: f32,
    #[serde(default)]
    pub temperature: f32,
    #[serde(default)]
    pub tint: f32,
}

impl Default for LocalAdjustment {
    fn default() -> Self {
        Self {
            exposure: 0.,
            contrast: 0.,
            saturation: 0.,
            temperature: 0.,
            tint: 0.,
        }
    }
}

impl LocalAdjustment {
    pub fn sanitized(self) -> Self {
        let finite = |v: f32| if v.is_finite() { v } else { 0. };
        Self {
            exposure: finite(self.exposure).clamp(-5., 5.),
            contrast: finite(self.contrast).clamp(-100., 100.),
            saturation: finite(self.saturation).clamp(-100., 100.),
            temperature: finite(self.temperature).clamp(-100., 100.),
            tint: finite(self.tint).clamp(-100., 100.),
        }
    }
}

/// One pressure-independent brush sample in normalized original-image
/// coordinates. Negative/erase state is explicit so strokes stay editable.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct BrushPoint {
    pub position: [f32; 2],
    pub radius: f32,
    #[serde(default = "default_flow")]
    pub flow: f32,
    #[serde(default)]
    pub erase: bool,
}

fn default_flow() -> f32 {
    1.
}

/// U19 mask geometry. Every coordinate is normalized in the uncropped,
/// unrotated original. The renderer evaluates it after its output->source
/// geometry mapping, so crop/rotate/upright never detach a mask.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum MaskShape {
    Brush {
        points: Vec<BrushPoint>,
    },
    Linear {
        start: [f32; 2],
        end: [f32; 2],
    },
    Radial {
        center: [f32; 2],
        radius: [f32; 2],
        #[serde(default)]
        rotation: f32,
    },
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LocalMask {
    pub id: u64,
    pub name: String,
    pub shape: MaskShape,
    #[serde(default = "default_mask_feather")]
    pub feather: f32,
    #[serde(default)]
    pub invert: bool,
    #[serde(default)]
    pub adjustment: LocalAdjustment,
}

fn default_mask_feather() -> f32 {
    0.5
}

/// U19 non-destructive spot heal: target and donor are original-image
/// coordinates. The operation remains editable and follows geometry.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct HealSpot {
    pub id: u64,
    pub target: [f32; 2],
    pub source: [f32; 2],
    pub radius: f32,
    #[serde(default = "default_mask_feather")]
    pub feather: f32,
}

#[derive(Clone, Debug, Default, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LocalEdits {
    #[serde(default)]
    pub masks: Vec<LocalMask>,
    #[serde(default)]
    pub heals: Vec<HealSpot>,
}

impl LocalEdits {
    pub fn is_empty(&self) -> bool {
        self.masks.is_empty() && self.heals.is_empty()
    }

    /// Stable per-photo IDs without a global counter or wall-clock coupling.
    pub fn next_id(&self) -> u64 {
        self.masks
            .iter()
            .map(|m| m.id)
            .chain(self.heals.iter().map(|h| h.id))
            .max()
            .unwrap_or(0)
            .saturating_add(1)
    }

    pub fn sanitized(&self) -> Self {
        let unit = |v: f32, fallback: f32| {
            if v.is_finite() {
                v.clamp(0., 1.)
            } else {
                fallback
            }
        };
        let point = |p: [f32; 2]| [unit(p[0], 0.5), unit(p[1], 0.5)];
        let masks = self
            .masks
            .iter()
            .cloned()
            .map(|mut m| {
                m.feather = unit(m.feather, default_mask_feather());
                m.adjustment = m.adjustment.sanitized();
                match &mut m.shape {
                    MaskShape::Brush { points } => {
                        for p in points {
                            p.position = point(p.position);
                            p.radius = unit(p.radius, 0.1).max(0.0001);
                            p.flow = unit(p.flow, 1.);
                        }
                    }
                    MaskShape::Linear { start, end } => {
                        *start = point(*start);
                        *end = point(*end);
                    }
                    MaskShape::Radial {
                        center,
                        radius,
                        rotation,
                    } => {
                        *center = point(*center);
                        *radius = [
                            unit(radius[0], 0.25).max(0.0001),
                            unit(radius[1], 0.25).max(0.0001),
                        ];
                        if !rotation.is_finite() {
                            *rotation = 0.;
                        }
                    }
                }
                m
            })
            .collect();
        let heals = self
            .heals
            .iter()
            .cloned()
            .map(|mut h| {
                h.target = point(h.target);
                h.source = point(h.source);
                h.radius = unit(h.radius, 0.04).max(0.0001);
                h.feather = unit(h.feather, default_mask_feather());
                h
            })
            .collect();
        Self { masks, heals }
    }
}

/// V22: the manual Transform sliders as an Upright transform.
pub fn transform_of(params: &[f32; PARAM_COUNT]) -> crate::upright::Transform {
    crate::upright::Transform {
        vertical: params[63],
        horizontal: params[64],
        rotate: params[65],
        aspect: params[66],
        scale: params[67],
        offset_x: params[68],
        offset_y: params[69],
    }
}

/// Pad a stored param vector to full width: the current width passes
/// through; legacy widths (12 Basic-only, 46 pre-Effects, 49 pre-grading,
/// 63 pre-Transform) fill the newer slots with defaults; anything else drops.
pub fn pad_params(v: &[f32]) -> Option<[f32; PARAM_COUNT]> {
    match v.len() {
        PARAM_COUNT | 12 | 46 | 49 | 63 => {
            let mut arr = defaults();
            arr[..v.len()].copy_from_slice(v);
            Some(arr)
        }
        _ => None,
    }
}

pub fn defaults() -> [f32; PARAM_COUNT] {
    PARAMS.map(|d| d.default)
}

pub fn is_modified(i: usize, v: f32) -> bool {
    (v - PARAMS[i].default).abs() > PARAMS[i].step * 0.25
}

pub fn snap(i: usize, v: f32) -> f32 {
    let d = PARAMS[i];
    // NaN survives `clamp` and serializes as JSON null, which makes the
    // whole persisted edit row unreadable — fall back to the default.
    if v.is_nan() {
        return d.default;
    }
    ((v / d.step).round() * d.step).clamp(d.min, d.max)
}

pub fn format(i: usize, v: f32) -> String {
    let sign = |x: f32| {
        if x > 0. {
            "+"
        } else if x < 0. {
            "\u{2212}"
        } else {
            ""
        }
    };
    match PARAMS[i].fmt {
        Fmt::Kelvin => format!("{} K", v.round() as i32),
        Fmt::Int => format!("{}{}", sign(v.round()), v.round().abs() as i32),
        // U09: units travel with the number (Kelvin, EV); ±100 scales
        // are unitless by definition.
        Fmt::Ev => {
            let r = (v * 100.).round() / 100.;
            format!("{}{:.2} EV", sign(r), r.abs())
        }
        Fmt::Unit => format!("{:.3}", v.clamp(0., 1.)),
        Fmt::Float => format!("{:.1}", v),
        Fmt::Hue => format!("{}°", v.round() as i32),
    }
}

/// A photo's develop settings plus undo history.
#[derive(Clone, Debug)]
pub struct Edit {
    pub params: [f32; PARAM_COUNT],
    pub history: Vec<HistoryStep>,
    pub cursor: usize,
    /// Aspect-ratio crop (width/height), None = free.
    pub crop: Option<f32>,
    /// U08: normalized crop bounds + straighten angle + flips.
    pub geom: CropGeom,
    /// U18: per-panel bypass (Basic is always on).
    pub curve_on: bool,
    pub hsl_on: bool,
    pub detail_on: bool,
    pub optics_on: bool,
    pub effects_on: bool,
    pub grading_on: bool,
    /// U19: editable local masks and spot heals.
    pub locals: LocalEdits,
    /// U20: Laika camera rendering profile (not an Adobe profile name).
    pub camera_profile: CameraProfile,
}

/// U09: white-balance presets (Temp Kelvin; tint stays where the user
/// left it except As Shot, which restores the pipeline default).
/// Pairs are standard CCT anchors, not calibrated profiles.
pub const WB_PRESETS: [(&str, f32); 7] = [
    ("As Shot", 5480.),
    ("Daylight", 5500.),
    ("Cloudy", 6500.),
    ("Shade", 7500.),
    ("Tungsten", 3200.),
    ("Fluorescent", 4000.),
    ("Flash", 5500.),
];

/// sRGB byte → scene-linear (exact inverse companding).
pub fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// Correlated color temperature (McCamy) for linear sRGB, plus a tint
/// delta that neutralizes green excess the way the Temp/Tint stages do:
/// the shader scales G by `(1 − tint/300)`, so `+300·(1 − Lum/G)` trims
/// a cast to neutral. One-shot approximation — tone stages also move
/// color, so the sliders stay for trimming. Pure and tested.
pub fn wb_from_linear(r: f32, g: f32, b: f32) -> (f32, f32) {
    // Linear sRGB → XYZ (D65).
    let x = 0.4124564 * r + 0.3575761 * g + 0.1804375 * b;
    let y = 0.2126729 * r + 0.7151522 * g + 0.0721750 * b;
    let z = 0.0193339 * r + 0.1191920 * g + 0.9503041 * b;
    let sum = (x + y + z).max(1e-6);
    let (xc, yc) = (x / sum, y / sum);
    // The denominator is negative for every real white (yc > 0.1858) —
    // guard near-zero preserving the sign, never clamp it away.
    let denom = 0.1858 - yc;
    let denom = if denom.abs() < 1e-6 { 1e-6 } else { denom };
    let n = (xc - 0.3320) / denom;
    let cct = (449. * n.powi(3) + 3525. * n.powi(2) + 6823.3 * n + 5520.33).clamp(2000., 8960.);
    let lum = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let tint = if g > 1e-4 && lum > 1e-4 {
        (300. * (1. - lum / g)).clamp(-150., 150.)
    } else {
        0.
    };
    (cct, tint)
}

/// White balance from developed sRGB bytes (eyedropper sample or frame
/// mean for Auto): inverse-compand, then [`wb_from_linear`]. Assumes a
/// neutral target — garbage in (a saturated patch), approximation out.
pub fn wb_from_sample(r: u8, g: u8, b: u8) -> (f32, f32) {
    wb_from_linear(
        srgb_to_linear(r as f32 / 255.),
        srgb_to_linear(g as f32 / 255.),
        srgb_to_linear(b as f32 / 255.),
    )
}

/// U18: panel bypass default (old rows predate panels — active).
pub fn flag_on() -> bool {
    true
}

/// U08: real geometry — normalized crop rectangle, straighten angle in
/// degrees (-45..45), and mirrors. The rect is always pre-constrained
/// (see [`constrain_crop`]), so renderers can sample it directly.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CropGeom {
    /// Normalized x, y, w, h in current display (post-rotation) space.
    /// Rotating transmutes the rect (see `rotated_cw/ccw`) so it keeps
    /// describing the same image content — never double-applied.
    #[serde(default = "default_rect")]
    pub rect: [f32; 4],
    #[serde(default)]
    pub angle: f32,
    #[serde(default)]
    pub flip_h: bool,
    #[serde(default)]
    pub flip_v: bool,
    /// V15: quarter turns clockwise (0..4). Display dims swap on odd
    /// values; the shader unrotates before crop/flip/straighten.
    #[serde(default)]
    pub rotation: u8,
    /// V22: Upright mode, solved correction, guides, constrain-to-image.
    #[serde(default)]
    pub upright: crate::upright::Upright,
}

fn default_rect() -> [f32; 4] {
    [0., 0., 1., 1.]
}

impl Default for CropGeom {
    fn default() -> Self {
        Self {
            rect: default_rect(),
            angle: 0.,
            flip_h: false,
            flip_v: false,
            rotation: 0,
            upright: Default::default(),
        }
    }
}

impl CropGeom {
    /// True when nothing differs from the full-frame original.
    pub fn is_default(&self) -> bool {
        self.rect == [0., 0., 1., 1.]
            && self.angle == 0.
            && !self.flip_h
            && !self.flip_v
            && self.rotation % 4 == 0
            && self.upright.is_default()
    }

    /// V15: display dims for a source frame (odd rotations swap axes).
    pub fn display_dims(&self, sw: f32, sh: f32) -> (f32, f32) {
        if self.rotation % 4 % 2 == 1 {
            (sh, sw)
        } else {
            (sw, sh)
        }
    }

    /// V15: rotate 90° clockwise in place — rect transmuted to the same
    /// content, flips conjugated (H↔V swap), angle untouched (straighten
    /// lives in display space and composes without doubling).
    pub fn rotated_cw(mut self) -> Self {
        let [x, y, w, h] = self.rect;
        self.rect = [1. - (y + h), x, h, w];
        self.rotation = (self.rotation + 1) % 4;
        std::mem::swap(&mut self.flip_h, &mut self.flip_v);
        self.sanitized()
    }

    /// V15: rotate 90° counter-clockwise in place.
    pub fn rotated_ccw(mut self) -> Self {
        let [x, y, w, h] = self.rect;
        self.rect = [y, 1. - (x + w), h, w];
        self.rotation = (self.rotation + 3) % 4;
        std::mem::swap(&mut self.flip_h, &mut self.flip_v);
        self.sanitized()
    }

    /// V15: EXIF orientation 1..8 for the (rotation, flips) pair.
    /// Render order is rotate-then-flip (flips in display space).
    pub fn orientation(&self) -> u8 {
        // Double flips equal a half turn: fold them into the rotation.
        let r = (self.rotation % 4 + if self.flip_h && self.flip_v { 2 } else { 0 }) % 4;
        let (fh, fv) = (self.flip_h && !self.flip_v, self.flip_v && !self.flip_h);
        match (r, fh, fv) {
            (0, false, false) => 1,
            (0, true, false) => 2,
            (0, false, true) => 4,
            (1, false, false) => 6,
            (1, true, false) => 5,
            (1, false, true) => 7,
            (2, false, false) => 3,
            (2, true, false) => 4,
            (2, false, true) => 2,
            (3, false, false) => 8,
            (3, true, false) => 7,
            (3, false, true) => 5,
            _ => 1,
        }
    }

    /// V15: derive (rotation, flips) from an EXIF orientation value.
    /// Unknown values read as identity, never an error.
    pub fn from_orientation(o: u8) -> (u8, bool, bool) {
        match o {
            2 => (0, true, false),
            3 => (2, false, false),
            4 => (0, false, true),
            5 => (3, false, true),
            6 => (1, false, false),
            7 => (1, false, true),
            8 => (3, false, false),
            _ => (0, false, false),
        }
    }

    /// Sanitize typed/dragged values: rect clamped into the frame with a
    /// minimum size, angle into ±45.
    pub fn sanitized(mut self) -> Self {
        let r = &mut self.rect;
        // Non-finite input (e.g. a sidecar `CropLeft="nan"`) would make
        // `clamp` below panic (NaN bound) — reset those components.
        let full = default_rect();
        for (v, d) in r.iter_mut().zip(full) {
            if !v.is_finite() {
                *v = d;
            }
        }
        if !self.angle.is_finite() {
            self.angle = 0.;
        }
        r[2] = r[2].clamp(0.02, 1.);
        r[3] = r[3].clamp(0.02, 1.);
        r[0] = r[0].clamp(0., 1. - r[2]);
        r[1] = r[1].clamp(0., 1. - r[3]);
        self.angle = self.angle.clamp(-45., 45.);
        self
    }
}

/// U08: output-uv → source-pixel mapping for crop + straighten + flip.
/// `(u, v)` is normalized output space (0..1 over the crop rect);
/// returns source pixels. The WGSL stage mirrors this formula exactly
/// (see `develop.wgsl`), so the unit tests below pin both.
/// Positive angles rotate the picture clockwise as viewed.
/// V15: `rotation` quarter-turns CW; `fw`/`fh` are DISPLAY (post-rotation)
/// frame dims — the tail unrotates into source pixels.
pub fn crop_sample(
    u: f32,
    v: f32,
    rect: [f32; 4],
    angle_deg: f32,
    flip_h: bool,
    flip_v: bool,
    rotation: u8,
    fw: f32,
    fh: f32,
) -> (f32, f32) {
    crop_sample_warp(
        u,
        v,
        rect,
        angle_deg,
        flip_h,
        flip_v,
        rotation,
        fw,
        fh,
        &WARP_IDENTITY,
    )
    .unwrap_or((f32::NAN, f32::NAN))
}

/// V22: identity render warp (row-major 3×3).
pub const WARP_IDENTITY: [f32; 9] = [1., 0., 0., 0., 1., 0., 0., 0., 1.];

/// V22: [`crop_sample`] with the perspective warp `g` (corrected →
/// source, centered display coords; see `upright`) applied between the
/// straighten and the unrotate — the shader's order. None past the
/// warp's horizon.
#[allow(clippy::too_many_arguments)]
pub fn crop_sample_warp(
    u: f32,
    v: f32,
    rect: [f32; 4],
    angle_deg: f32,
    flip_h: bool,
    flip_v: bool,
    rotation: u8,
    fw: f32,
    fh: f32,
    g: &[f32; 9],
) -> Option<(f32, f32)> {
    let (mut x, mut y) = (u, v);
    if flip_h {
        x = 1. - x;
    }
    if flip_v {
        y = 1. - y;
    }
    let sx = (rect[0] + x * rect[2]) * fw;
    let sy = (rect[1] + y * rect[3]) * fh;
    // Derotate content about the frame center.
    let a = angle_deg.to_radians();
    let (s, c) = (a.sin(), a.cos());
    let (px, py) = (fw / 2., fh / 2.);
    let (dx, dy) = (sx - px, sy - py);
    let (mut qx, mut qy) = (px + dx * c + dy * s, py - dx * s + dy * c);
    if *g != WARP_IDENTITY {
        let (wu, wv) = crate::upright::warp_uv(g, qx / fw, qy / fh, fw / fh.max(1e-6))?;
        qx = wu * fw;
        qy = wv * fh;
    }
    // Unrotate into source pixels (source dims derive from display).
    let (sw, sh) = if rotation % 4 % 2 == 1 {
        (fh, fw)
    } else {
        (fw, fh)
    };
    let (nx, ny) = (qx / fw, qy / fh);
    Some(match rotation % 4 {
        1 => (ny * sw, (1. - nx) * sh),
        2 => ((1. - nx) * sw, (1. - ny) * sh),
        3 => ((1. - ny) * sw, nx * sh),
        _ => (qx, qy),
    })
}

/// U08: shrink a rect (keeping center + aspect) until every output
/// corner samples inside the `sw × sh` SOURCE frame — rotated sampling
/// never shows empty corners. Pure; corners suffice (affine map of a
/// convex quad into a convex frame). V15: rotation-aware (display dims
/// derived, bounds checked against source).
pub fn constrain_crop(rect: [f32; 4], angle_deg: f32, rotation: u8, sw: f32, sh: f32) -> [f32; 4] {
    constrain_crop_warp(rect, angle_deg, rotation, sw, sh, &WARP_IDENTITY)
}

/// V22: [`constrain_crop`] through a perspective warp. The covered
/// region is still convex (a homography of the source rectangle), so the
/// four corners decide. When even a sliver around the rect's center is
/// uncovered, the rect re-centers on the corrected image first.
pub fn constrain_crop_warp(
    rect: [f32; 4],
    angle_deg: f32,
    rotation: u8,
    sw: f32,
    sh: f32,
    g: &[f32; 9],
) -> [f32; 4] {
    if sw <= 0. || sh <= 0. {
        return [0., 0., 1., 1.];
    }
    let (fw, fh) = if rotation % 4 % 2 == 1 {
        (sh, sw)
    } else {
        (sw, sh)
    };
    let mut r = rect;
    // Non-finite components would make `clamp` panic (NaN bound).
    for (v, d) in r.iter_mut().zip(default_rect()) {
        if !v.is_finite() {
            *v = d;
        }
    }
    r[2] = r[2].clamp(0.02, 1.);
    r[3] = r[3].clamp(0.02, 1.);
    r[0] = r[0].clamp(0., 1. - r[2]);
    r[1] = r[1].clamp(0., 1. - r[3]);
    let angle = if angle_deg.is_finite() {
        angle_deg.clamp(-45., 45.)
    } else {
        0.
    };
    let inside_rect = |r: [f32; 4], s: f32| -> bool {
        let t = [
            r[0] + r[2] * (1. - s) / 2.,
            r[1] + r[3] * (1. - s) / 2.,
            r[2] * s,
            r[3] * s,
        ];
        [(0., 0.), (1., 0.), (1., 1.), (0., 1.)]
            .iter()
            .all(|(u, v)| {
                crop_sample_warp(*u, *v, t, angle, false, false, rotation, fw, fh, g).is_some_and(
                    |(ix, iy)| ix >= -0.5 && ix <= sw + 0.5 && iy >= -0.5 && iy <= sh + 0.5,
                )
            })
    };
    if !inside_rect(r, 0.02) && *g != WARP_IDENTITY {
        // Center the rect on the corrected image of the source center.
        if let Some(inv) = crate::upright::invert(&g.map(|x| x as f64)) {
            let a = (fw / fh.max(1e-6)) as f64;
            if let Some((cx, cy)) = crate::upright::apply(&inv, 0., 0.) {
                let (cu, cv) = crate::upright::from_centered(cx, cy, a);
                // Undo the straighten about the frame center (display uv).
                let rad = angle.to_radians() as f64;
                let (sn, cs) = (rad.sin(), rad.cos());
                let (dx, dy) = ((cu - 0.5) * fw as f64, (cv - 0.5) * fh as f64);
                let (ox, oy) = (dx * cs - dy * sn, dx * sn + dy * cs);
                let (cu, cv) = (ox / fw as f64 + 0.5, oy / fh as f64 + 0.5);
                r[0] = (cu as f32 - r[2] / 2.).clamp(0., 1. - r[2]);
                r[1] = (cv as f32 - r[3] / 2.).clamp(0., 1. - r[3]);
            }
        }
    }
    let inside = |s: f32| inside_rect(r, s);
    if inside(1.) {
        return r;
    }
    let (mut lo, mut hi) = (0., 1.);
    for _ in 0..24 {
        let mid = (lo + hi) / 2.;
        if inside(mid) {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    let s = lo.max(0.02);
    [
        r[0] + r[2] * (1. - s) / 2.,
        r[1] + r[3] * (1. - s) / 2.,
        r[2] * s,
        r[3] * s,
    ]
}

/// V22: the render warp for a photo — manual Transform sliders plus the
/// Upright solution (when a mode is on), in display space of `fw × fh`
/// (display dims). Identity when nothing is corrected.
pub fn geom_warp(geom: &CropGeom, params: &[f32; PARAM_COUNT], fw: f32, fh: f32) -> [f32; 9] {
    let mut t = transform_of(params);
    if geom.upright.mode() != crate::upright::UprightMode::Off {
        t = t.with_auto(geom.upright.auto);
    }
    if t.is_identity() || fw <= 0. || fh <= 0. {
        return WARP_IDENTITY;
    }
    crate::upright::warp_matrix(&t, fw / fh)
}

/// V22: the crop rect a renderer should use — constrained to the
/// corrected image when "Constrain to image" is on (always without a
/// warp, as before), else only sanitized so corners may show white.
pub fn constrain_geom(
    geom: &CropGeom,
    params: &[f32; PARAM_COUNT],
    sw: f32,
    sh: f32,
) -> ([f32; 4], [f32; 9]) {
    let (fw, fh) = geom.display_dims(sw, sh);
    let g = geom_warp(geom, params, fw, fh);
    if g != WARP_IDENTITY && !geom.upright.constrain {
        return (geom.sanitized().rect, g);
    }
    (
        constrain_crop_warp(geom.rect, geom.angle, geom.rotation, sw, sh, &g),
        g,
    )
}

/// V22: inverse of [`crop_sample_warp`] up to the unrotate — a point in
/// pre-warp DISPLAY uv (e.g. a guide endpoint) to output uv over `rect`
/// (flips applied). None past the warp's horizon. `fw`/`fh` display dims.
#[allow(clippy::too_many_arguments)]
pub fn display_to_output(
    du: f32,
    dv: f32,
    rect: [f32; 4],
    angle_deg: f32,
    flip_h: bool,
    flip_v: bool,
    fw: f32,
    fh: f32,
    g: &[f32; 9],
) -> Option<(f32, f32)> {
    let (mut qu, mut qv) = (du as f64, dv as f64);
    if *g != WARP_IDENTITY {
        let a = (fw / fh.max(1e-6)) as f64;
        let f = crate::upright::invert(&g.map(|x| x as f64))?;
        let (x, y) = crate::upright::to_centered(qu, qv, a);
        let (cx, cy) = crate::upright::apply(&f, x, y)?;
        (qu, qv) = crate::upright::from_centered(cx, cy, a);
    }
    // Re-apply the straighten (inverse of the derotate in crop_sample).
    let rad = angle_deg.to_radians() as f64;
    let (s, c) = (rad.sin(), rad.cos());
    let (dx, dy) = ((qu - 0.5) * fw as f64, (qv - 0.5) * fh as f64);
    let (ox, oy) = (dx * c - dy * s, dx * s + dy * c);
    let (su, sv) = (ox / fw as f64 + 0.5, oy / fh as f64 + 0.5);
    let mut u = ((su - rect[0] as f64) / (rect[2] as f64).max(1e-9)) as f32;
    let mut v = ((sv - rect[1] as f64) / (rect[3] as f64).max(1e-9)) as f32;
    if flip_h {
        u = 1. - u;
    }
    if flip_v {
        v = 1. - v;
    }
    Some((u, v))
}

#[derive(Clone, Debug)]
pub struct HistoryStep {
    pub label: String,
    pub value: String,
    pub params: [f32; PARAM_COUNT],
    /// U14: complete snapshots — geometry, ratings, flags ride every step.
    pub crop: Option<f32>,
    /// U08: crop bounds + angle + flips ride every step too.
    pub geom: CropGeom,
    /// U18: panel bypass rides every step (undoable).
    pub curve_on: bool,
    pub hsl_on: bool,
    pub detail_on: bool,
    pub optics_on: bool,
    pub effects_on: bool,
    pub grading_on: bool,
    pub rating: u8,
    pub picked: bool,
    pub rejected: bool,
    /// V13: color label rides every step (0 = none).
    pub color_label: u8,
    /// U19: complete local edit state makes each undo step deterministic.
    pub locals: LocalEdits,
    /// U20: camera profile changes undo with the rest of Develop.
    pub camera_profile: CameraProfile,
}

/// U14: full restorable photo state for undo steps, snapshots, and reloads.
#[derive(Clone, Debug, PartialEq)]
pub struct Snap {
    pub params: [f32; PARAM_COUNT],
    pub crop: Option<f32>,
    /// U08: geometry restores with every undo/redo/snapshot/paste.
    pub geom: CropGeom,
    /// U18: panel bypass restores too.
    pub curve_on: bool,
    pub hsl_on: bool,
    pub detail_on: bool,
    pub optics_on: bool,
    pub effects_on: bool,
    pub grading_on: bool,
    pub rating: u8,
    pub picked: bool,
    pub rejected: bool,
    /// V13: color label rides every step (0 = none).
    pub color_label: u8,
    pub locals: LocalEdits,
    pub camera_profile: CameraProfile,
}

/// Persisted history rows survive unknown future fields.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct HistoryJson {
    pub label: String,
    pub value: String,
    pub params: Vec<f32>,
    #[serde(default)]
    pub crop: Option<f32>,
    /// U08: absent in pre-geometry rows → full-frame default.
    #[serde(default)]
    pub geom: CropGeom,
    /// U18: absent in pre-panel rows → active.
    #[serde(default = "flag_on")]
    pub curve_on: bool,
    #[serde(default = "flag_on")]
    pub hsl_on: bool,
    #[serde(default = "flag_on")]
    pub detail_on: bool,
    #[serde(default = "flag_on")]
    pub optics_on: bool,
    #[serde(default = "flag_on")]
    pub effects_on: bool,
    #[serde(default = "flag_on")]
    pub grading_on: bool,
    #[serde(default)]
    pub rating: u8,
    #[serde(default)]
    pub picked: bool,
    #[serde(default)]
    pub rejected: bool,
    #[serde(default)]
    pub color_label: u8,
    #[serde(default)]
    pub locals: LocalEdits,
    #[serde(default)]
    pub camera_profile: CameraProfile,
}

impl HistoryStep {
    fn to_json(&self) -> HistoryJson {
        HistoryJson {
            label: self.label.clone(),
            value: self.value.clone(),
            params: self.params.to_vec(),
            crop: self.crop,
            geom: self.geom,
            curve_on: self.curve_on,
            hsl_on: self.hsl_on,
            detail_on: self.detail_on,
            optics_on: self.optics_on,
            effects_on: self.effects_on,
            grading_on: self.grading_on,
            rating: self.rating,
            picked: self.picked,
            rejected: self.rejected,
            color_label: self.color_label,
            locals: self.locals.clone(),
            camera_profile: self.camera_profile,
        }
    }

    fn from_json(j: &HistoryJson) -> Option<HistoryStep> {
        // Legacy 12-wide rows pad new slots with defaults (U18).
        let params = pad_params(&j.params)?;
        Some(HistoryStep {
            label: j.label.clone(),
            value: j.value.clone(),
            params,
            crop: j.crop,
            geom: j.geom,
            curve_on: j.curve_on,
            hsl_on: j.hsl_on,
            detail_on: j.detail_on,
            optics_on: j.optics_on,
            effects_on: j.effects_on,
            grading_on: j.grading_on,
            rating: j.rating,
            picked: j.picked,
            rejected: j.rejected,
            color_label: j.color_label,
            locals: j.locals.clone(),
            camera_profile: j.camera_profile,
        })
    }
}

/// Serialize history for the `edits` row (newest last, capped by push).
pub fn encode_history(history: &[HistoryStep]) -> String {
    serde_json::to_string(&history.iter().map(HistoryStep::to_json).collect::<Vec<_>>())
        .unwrap_or_else(|_| "[]".into())
}

/// Parse history rows; corrupt entries are dropped, never fatal.
pub fn decode_history(json: &str) -> Vec<HistoryStep> {
    serde_json::from_str::<Vec<HistoryJson>>(json)
        .map(|rows| rows.iter().filter_map(HistoryStep::from_json).collect())
        .unwrap_or_default()
}

impl Edit {
    pub fn new() -> Self {
        Self {
            params: defaults(),
            history: Vec::new(),
            cursor: 0,
            crop: None,
            geom: CropGeom::default(),
            curve_on: true,
            hsl_on: true,
            detail_on: true,
            optics_on: true,
            effects_on: true,
            grading_on: true,
            locals: LocalEdits::default(),
            camera_profile: CameraProfile::default(),
        }
    }

    pub fn reset(&mut self, snap: Snap) {
        self.params = defaults();
        let mut base = snap;
        base.params = defaults();
        self.push_snap("Reset", "", base);
    }

    /// U14: one commit = one step with a complete snapshot. Truncates any
    /// redo branch (editing an earlier step forks predictably). The live
    /// state becomes the snapshot (history tip == current values, always).
    pub fn push_snap(&mut self, label: &str, value: &str, snap: Snap) {
        self.history.truncate(self.cursor);
        self.history.push(HistoryStep {
            label: label.into(),
            value: value.into(),
            params: snap.params,
            crop: snap.crop,
            geom: snap.geom,
            curve_on: snap.curve_on,
            hsl_on: snap.hsl_on,
            detail_on: snap.detail_on,
            optics_on: snap.optics_on,
            effects_on: snap.effects_on,
            grading_on: snap.grading_on,
            rating: snap.rating,
            picked: snap.picked,
            rejected: snap.rejected,
            color_label: snap.color_label,
            locals: snap.locals.clone(),
            camera_profile: snap.camera_profile,
        });
        self.params = snap.params;
        self.crop = snap.crop;
        self.geom = snap.geom;
        self.curve_on = snap.curve_on;
        self.hsl_on = snap.hsl_on;
        self.detail_on = snap.detail_on;
        self.optics_on = snap.optics_on;
        self.effects_on = snap.effects_on;
        self.grading_on = snap.grading_on;
        self.locals = snap.locals;
        self.camera_profile = snap.camera_profile;
        self.cursor = self.history.len();
        // Cap depth but pin the baseline: the first step always stays the
        // undo floor, even across restarts.
        while self.history.len() > 50 {
            self.history.remove(1);
            self.cursor = self.cursor.saturating_sub(1).max(1);
        }
    }

    /// U14: legacy entry point — snapshots current flags as cleared.
    /// Prefer push_snap; kept for reset paths that own no photo row.
    pub fn push(&mut self, label: &str, value: &str, params: [f32; PARAM_COUNT]) {
        self.push_snap(
            label,
            value,
            Snap {
                params,
                crop: self.crop,
                geom: self.geom,
                curve_on: self.curve_on,
                hsl_on: self.hsl_on,
                detail_on: self.detail_on,
                optics_on: self.optics_on,
                effects_on: self.effects_on,
                grading_on: self.grading_on,
                rating: 0,
                picked: false,
                rejected: false,
                color_label: 0,
                locals: self.locals.clone(),
                camera_profile: self.camera_profile,
            },
        );
    }

    /// U14: ensure an "Import" baseline so the first real step is undoable.
    /// The baseline snapshot is the pre-edit state supplied by the caller.
    pub fn ensure_baseline(&mut self, snap: Snap) {
        if self.history.is_empty() {
            self.history.push(HistoryStep {
                label: "Import".into(),
                value: String::new(),
                params: snap.params,
                crop: snap.crop,
                geom: snap.geom,
                curve_on: snap.curve_on,
                hsl_on: snap.hsl_on,
                detail_on: snap.detail_on,
                optics_on: snap.optics_on,
                effects_on: snap.effects_on,
                grading_on: snap.grading_on,
                rating: snap.rating,
                picked: snap.picked,
                rejected: snap.rejected,
                color_label: snap.color_label,
                locals: snap.locals.clone(),
                camera_profile: snap.camera_profile,
            });
            self.cursor = 1;
        }
    }

    /// Click-to-revert: restore the snapshot, keep forward steps for redo
    /// until the next edit truncates them. Returns the applied snapshot.
    pub fn revert_to(&mut self, index: usize) -> Option<Snap> {
        let step = self.history.get(index)?.clone();
        self.apply_step(&step);
        self.cursor = index + 1;
        Some(step_snap(&step))
    }

    /// U14: undo one step; None when already at the baseline floor.
    pub fn undo(&mut self) -> Option<Snap> {
        if self.cursor <= 1 || self.history.is_empty() {
            return None;
        }
        self.cursor -= 1;
        let step = self.history.get(self.cursor - 1)?.clone();
        self.apply_step(&step);
        Some(step_snap(&step))
    }

    /// U14: redo one undone step; None when at the tip.
    pub fn redo(&mut self) -> Option<Snap> {
        if self.cursor >= self.history.len() {
            return None;
        }
        let step = self.history.get(self.cursor)?.clone();
        self.apply_step(&step);
        self.cursor += 1;
        Some(step_snap(&step))
    }

    fn apply_step(&mut self, step: &HistoryStep) {
        self.params = step.params;
        self.crop = step.crop;
        self.geom = step.geom;
        self.curve_on = step.curve_on;
        self.hsl_on = step.hsl_on;
        self.detail_on = step.detail_on;
        self.optics_on = step.optics_on;
        self.effects_on = step.effects_on;
        self.grading_on = step.grading_on;
        self.locals = step.locals.clone();
        self.camera_profile = step.camera_profile;
    }
}

impl Default for Edit {
    fn default() -> Self {
        Self::new()
    }
}

/// Snapshot view of one history step.
pub fn step_snap(step: &HistoryStep) -> Snap {
    Snap {
        params: step.params,
        crop: step.crop,
        geom: step.geom,
        curve_on: step.curve_on,
        hsl_on: step.hsl_on,
        detail_on: step.detail_on,
        optics_on: step.optics_on,
        effects_on: step.effects_on,
        grading_on: step.grading_on,
        rating: step.rating,
        picked: step.picked,
        rejected: step.rejected,
        color_label: step.color_label,
        locals: step.locals.clone(),
        camera_profile: step.camera_profile,
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn pad_params_fills_effects_for_pre_effects_rows() {
        let mut legacy = super::defaults()[..46].to_vec();
        legacy[9] = 25.;
        let arr = super::pad_params(&legacy).expect("46-wide rows still load");
        assert_eq!(arr[9], 25.);
        assert_eq!(&arr[46..], &super::defaults()[46..]);
        assert!(super::pad_params(&[0.; 30]).is_none());
        // Pre-grading rows (49 wide) load with neutral wheels, Blending 50.
        let arr = super::pad_params(&super::defaults()[..49]).expect("49-wide rows load");
        assert_eq!(arr[super::GRADING_RANGE.end - 2], 50.);
        assert_eq!(super::format(49, 212.4), "212°");
        assert_eq!(super::PARAMS[super::EFFECTS_PARAMS[2]].label, "Dehaze");
    }

    use super::*;

    #[test]
    fn v15_rotation_transmutes_rect_without_losing_content() {
        // V15 gate: rect tracks the same content across rotations —
        // left-half source becomes top-half display after CW.
        let g = CropGeom {
            rect: [0., 0., 0.5, 1.],
            ..Default::default()
        };
        let cw = g.rotated_cw();
        assert_eq!(cw.rotation, 1);
        assert_eq!(cw.rect, [0., 0., 1., 0.5]);
        // CW then CCW is identity (rect, flips, rotation).
        let back = cw.rotated_ccw();
        assert_eq!(back.rect, g.rect);
        assert_eq!((back.flip_h, back.flip_v), (false, false));
        assert_eq!(back.rotation, 0);
        // Four CW turns return (float drift is sub-pixel: < 1e-6).
        let mut spin = CropGeom {
            rect: [0.1, 0.2, 0.5, 0.4],
            flip_h: true,
            ..Default::default()
        };
        let start = spin;
        for _ in 0..4 {
            spin = spin.rotated_cw();
        }
        for i in 0..4 {
            assert!(
                (spin.rect[i] - start.rect[i]).abs() < 1e-6,
                "{:?}",
                spin.rect
            );
        }
        assert_eq!((spin.flip_h, spin.flip_v), (start.flip_h, start.flip_v));
        assert_eq!(spin.rotation, 0);
        // Flips conjugate (H↔V swap) on quarter turns.
        let f = CropGeom {
            flip_h: true,
            ..Default::default()
        }
        .rotated_cw();
        assert_eq!((f.flip_h, f.flip_v), (false, true));
        // Display dims swap on odd rotations only.
        assert_eq!(g.display_dims(6000., 4000.), (6000., 4000.));
        assert_eq!(cw.display_dims(6000., 4000.), (4000., 6000.));
        assert_eq!(cw.rotated_cw().display_dims(6000., 4000.), (6000., 4000.));
    }

    #[test]
    fn v15_orientation_round_trips_all_eight() {
        // V15 gate: every EXIF orientation maps to a model triple and back.
        for o in 1..=8u8 {
            let (r, fh, fv) = CropGeom::from_orientation(o);
            let g = CropGeom {
                rotation: r,
                flip_h: fh,
                flip_v: fv,
                ..Default::default()
            };
            assert_eq!(g.orientation(), o, "orientation {o}");
        }
        // Garbage reads as identity, never an error.
        assert_eq!(CropGeom::from_orientation(0), (0, false, false));
        assert_eq!(CropGeom::from_orientation(255), (0, false, false));
        // Double flips fold to a half turn for foreign readers.
        let g = CropGeom {
            flip_h: true,
            flip_v: true,
            ..Default::default()
        };
        assert_eq!(g.orientation(), 3);
    }

    #[test]
    fn v15_crop_sample_unrotates_into_source_pixels() {
        // Portrait source 4000×6000, display rotated CW (6000×4000):
        // display center hits source center, corners land in-frame.
        let (cx, cy) = crop_sample(
            0.5,
            0.5,
            [0., 0., 1., 1.],
            0.,
            false,
            false,
            1,
            6000.,
            4000.,
        );
        assert!((cx - 2000.).abs() < 1e-3, "{cx}");
        assert!((cy - 3000.).abs() < 1e-3, "{cy}");
        let (x0, y0) = crop_sample(0., 0., [0., 0., 1., 1.], 0., false, false, 1, 6000., 4000.);
        assert!(
            (x0 - 0.).abs() < 1e-3 && (y0 - 6000.).abs() < 1e-3,
            "{x0},{y0}"
        );
        // R0 is the legacy path, pixel-identical.
        let (lx, ly) = crop_sample(0.25, 0.5, [0., 0., 1., 1.], 0., false, false, 0, 100., 100.);
        assert!((lx - 25.).abs() < 1e-3 && (ly - 50.).abs() < 1e-3);
        // Rotated crop+straighten constrains inside SOURCE bounds.
        for rot in 0..4u8 {
            let r = constrain_crop([0.05, 0.05, 0.9, 0.9], 30., rot, 4000., 6000.);
            for (u, v) in [(0., 0.), (1., 0.), (1., 1.), (0., 1.)] {
                let (fw, fh) = if rot % 2 == 1 {
                    (6000., 4000.)
                } else {
                    (4000., 6000.)
                };
                let (x, y) = crop_sample(u, v, r, 30., false, false, rot, fw, fh);
                assert!(x >= -0.5 && x <= 4000.5, "rot{rot} {u},{v} → {x}");
                assert!(y >= -0.5 && y <= 6000.5, "rot{rot} {u},{v} → {y}");
            }
        }
    }

    #[test]
    fn default_diffing() {
        let d = defaults();
        for i in 0..PARAM_COUNT {
            assert!(!is_modified(i, d[i]), "param {i} default reads untouched");
        }
        assert!(is_modified(2, 0.35));
        assert!(is_modified(4, -40.));
        assert!(!is_modified(2, 0.005));
    }

    #[test]
    fn snap_clamps() {
        assert_eq!(snap(2, 99.), 5.);
        assert_eq!(snap(2, -99.), -5.);
        assert_eq!(snap(0, 5491.), 5500.);
    }

    #[test]
    fn history_truncates_on_branch() {
        let mut e = Edit::new();
        let p = defaults();
        e.push("a", "1", p);
        e.push("b", "2", p);
        e.cursor = 1;
        e.push("c", "3", p);
        assert_eq!(e.history.len(), 2);
        assert_eq!(e.history[1].label, "c");
    }

    #[test]
    fn revert_restores_snapshot() {
        let mut e = Edit::new();
        let mut p1 = defaults();
        p1[2] = 1.0;
        let mut p2 = defaults();
        p2[2] = 2.0;
        e.push("a", "+1", p1);
        e.push("b", "+2", p2);
        e.revert_to(0);
        assert_eq!(e.params[2], 1.0);
        assert_eq!(e.cursor, 1);
    }

    fn snap_of(params: [f32; PARAM_COUNT]) -> Snap {
        Snap {
            params,
            crop: Some(1.5),
            geom: CropGeom {
                rect: [0.1, 0.1, 0.8, 0.8],
                angle: 2.,
                flip_h: true,
                flip_v: false,
                rotation: 0,
                upright: Default::default(),
            },
            curve_on: true,
            hsl_on: false,
            detail_on: true,
            optics_on: true,
            effects_on: true,
            grading_on: true,
            rating: 4,
            picked: true,
            rejected: false,
            color_label: 3,
            locals: LocalEdits::default(),
            camera_profile: CameraProfile::default(),
        }
    }

    #[test]
    fn undo_redo_walk_full_snapshots() {
        // U14: one gesture in, one keypress out — params, crop, rating.
        let mut e = Edit::new();
        let base = defaults();
        e.ensure_baseline(snap_of(base));
        assert_eq!(e.cursor, 1);
        let mut p1 = defaults();
        p1[2] = 0.5;
        e.push_snap("Exposure", "+0.50", snap_of(p1));
        assert_eq!(e.cursor, 2);
        // Undo restores the baseline floor exactly.
        let back = e.undo().expect("undo");
        assert_eq!(back.params, base);
        assert_eq!(back.rating, 4); // baseline carried the photo's rating
        assert_eq!(e.cursor, 1);
        assert!(e.undo().is_none()); // floor: no-op, nothing lost
        assert_eq!(e.cursor, 1);
        // Redo returns to the tip.
        let fwd = e.redo().expect("redo");
        assert_eq!(fwd.params[2], 0.5);
        assert_eq!(e.cursor, 2);
        assert!(e.redo().is_none());
        // Editing from the middle forks: redo branch truncated.
        e.undo();
        let mut p2 = defaults();
        p2[2] = -0.5;
        e.push_snap("Exposure", "-0.50", snap_of(p2));
        assert_eq!(e.history.len(), 2);
        assert!(e.redo().is_none());
    }

    #[test]
    fn local_edits_and_camera_profile_survive_history_restart_and_undo() {
        let mut e = Edit::new();
        let base = snap_of(defaults());
        e.ensure_baseline(base.clone());
        let mut changed = base;
        changed.camera_profile = CameraProfile::Vivid;
        changed.locals.masks.push(LocalMask {
            id: 1,
            name: "Sky".into(),
            shape: MaskShape::Linear {
                start: [0.1, 0.2],
                end: [0.9, 0.8],
            },
            feather: 0.7,
            invert: true,
            adjustment: LocalAdjustment {
                exposure: -0.8,
                ..Default::default()
            },
        });
        e.push_snap("Linear Gradient", "add", changed.clone());
        let json = encode_history(&e.history);
        let history = decode_history(&json);
        assert_eq!(history[1].locals, changed.locals);
        assert_eq!(history[1].camera_profile, CameraProfile::Vivid);

        e.history = history;
        let undone = e.undo().expect("baseline is undoable");
        assert!(undone.locals.is_empty());
        assert_eq!(undone.camera_profile, CameraProfile::Standard);
        let redone = e.redo().expect("local step is redoable");
        assert_eq!(redone.locals, changed.locals);
    }

    #[test]
    fn u08_crop_sample_identity_and_constrain() {
        // Default geometry samples the identity (shader zero-diff).
        for (u, v) in [(0., 0.), (1., 0.), (0.5, 0.5), (1., 1.)] {
            let (x, y) = crop_sample(u, v, [0., 0., 1., 1.], 0., false, false, 0, 6000., 4000.);
            assert!((x - u * 6000.).abs() < 0.01 && (y - v * 4000.).abs() < 0.01);
        }
        // Flips mirror exactly.
        let (x, _) = crop_sample(0.25, 0.5, [0., 0., 1., 1.], 0., true, false, 0, 100., 100.);
        assert!((x - 75.).abs() < 0.01);
        // No angle: rect passes through untouched.
        assert_eq!(
            constrain_crop([0.1, 0.2, 0.5, 0.4], 0., 0, 6000., 4000.),
            [0.1, 0.2, 0.5, 0.4]
        );
        // 30° on a full-frame rect shrinks to fit, keeps center and
        // pixel aspect (normalized w/h stays 1.0 on the 3:2 frame).
        let c = constrain_crop([0., 0., 1., 1.], 30., 0, 6000., 4000.);
        assert!((c[0] + c[2] / 2. - 0.5).abs() < 1e-4);
        assert!(((c[2] * 6000.) / (c[3] * 4000.) - 1.5).abs() < 1e-3);
        assert!(c[2] < 1. && c[3] < 1.);
        // Every constrained corner samples inside the frame (landscape
        // and portrait).
        for (fw, fh) in [(6000., 4000.), (4000., 6000.)] {
            for angle in [-45., -10., 7., 30., 45.] {
                let r = constrain_crop([0.05, 0.05, 0.9, 0.9], angle, 0, fw, fh);
                for (u, v) in [(0., 0.), (1., 0.), (1., 1.), (0., 1.)] {
                    let (x, y) = crop_sample(u, v, r, angle, false, false, 0, fw, fh);
                    assert!(x >= -0.5 && x <= fw + 0.5, "{angle} {u},{v} → {x}");
                    assert!(y >= -0.5 && y <= fh + 0.5, "{angle} {u},{v} → {y}");
                }
            }
        }
        // Sanitizer clamps garbage without NaN.
        let g = CropGeom {
            rect: [-1., 2., 5., 5.],
            angle: 90.,
            flip_h: false,
            flip_v: false,
            rotation: 0,
            upright: Default::default(),
        }
        .sanitized();
        assert_eq!(g.rect, [0., 0., 1., 1.]);
        assert_eq!(g.angle, 45.);
        assert!(CropGeom::default().is_default());
        // NaN never panics (clamp with a NaN bound) and never persists.
        let g = CropGeom {
            rect: [f32::NAN, 0.1, f32::NAN, 0.5],
            angle: f32::NAN,
            ..Default::default()
        }
        .sanitized();
        assert_eq!(g.rect, [0., 0.1, 1., 0.5]);
        assert_eq!(g.angle, 0.);
        let c = constrain_crop([0.1, 0.1, f32::NAN, f32::NAN], f32::NAN, 0, 600., 400.);
        assert!(c.iter().all(|v| v.is_finite()), "{c:?}");
        assert_eq!(snap(2, f32::NAN), PARAMS[2].default);
    }

    #[test]
    fn u09_wb_from_sample_points_the_right_way() {
        // D65 gray lands near 6500 K with no tint (McCamy sanity).
        let (t, tint) = wb_from_sample(186, 186, 186);
        assert!((t - 6504.).abs() < 60., "{t}");
        assert!(tint.abs() < 3., "{tint}");
        // Warm cast (high R) drives Temp down; green cast drives tint up.
        let (warm, _) = wb_from_sample(220, 150, 100);
        let (cool, _) = wb_from_sample(100, 150, 220);
        assert!(warm < cool, "{warm} vs {cool}");
        let (_, green) = wb_from_sample(150, 200, 150);
        let (_, magenta) = wb_from_sample(200, 150, 200);
        assert!(green > 0. && magenta < 0., "{green} {magenta}");
        // Temp snaps and clamps through the param def, not here.
        let (t, _) = wb_from_sample(255, 200, 120);
        assert!((2000.0..=8960.0).contains(&t));
    }

    #[test]
    fn history_codec_roundtrips_and_tolerates_legacy() {
        let mut e = Edit::new();
        let mut p1 = defaults();
        p1[0] = 6500.;
        e.push_snap("Temperature", "6500", snap_of(p1));
        let json = encode_history(&e.history);
        let back = decode_history(&json);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].params[0], 6500.);
        assert_eq!(back[0].crop, Some(1.5));
        assert_eq!(back[0].geom.rect, [0.1, 0.1, 0.8, 0.8]);
        assert_eq!(back[0].geom.angle, 2.);
        assert!(back[0].geom.flip_h);
        assert_eq!(back[0].rating, 4);
        assert!(back[0].picked);
        // Legacy rows (params-only, unknown future fields) decode.
        let legacy = r#"[{"label":"x","value":"","params":[0,0,0,0,0,0,0,0,0,0,0,0],"zzz":1}]"#;
        let back = decode_history(legacy);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].crop, None);
        // Corrupt rows drop, valid siblings survive.
        let mixed = r#"[{"label":"x","value":"","params":[1]}, {"label":"y","value":"","params":[0,0,0,0,0,0,0,0,0,0,0,0]}]"#;
        let back = decode_history(mixed);
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].label, "y");
        assert!(decode_history("not json").is_empty());
        assert!(decode_history("[]").is_empty());
    }

    #[test]
    fn v22_constrain_never_leaves_an_empty_corner() {
        // Deterministic sweep over keystone/rotate/scale/straighten and all
        // quarter turns: every corner of the constrained crop samples source.
        let mut seed = 7u32;
        let mut rnd = |lo: f32, hi: f32| {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            lo + (seed >> 8) as f32 / (1u32 << 24) as f32 * (hi - lo)
        };
        for i in 0..200 {
            let mut params = defaults();
            params[63] = rnd(-100., 100.);
            params[64] = rnd(-100., 100.);
            params[65] = rnd(-10., 10.);
            params[66] = rnd(-60., 60.);
            params[67] = rnd(60., 140.);
            params[68] = rnd(-50., 50.);
            params[69] = rnd(-50., 50.);
            let geom = CropGeom {
                rect: [rnd(0., 0.3), rnd(0., 0.3), rnd(0.4, 0.7), rnd(0.4, 0.7)],
                angle: rnd(-20., 20.),
                rotation: (i % 4) as u8,
                ..CropGeom::default()
            };
            let (sw, sh) = (6000., 4000.);
            let (rect, g) = constrain_geom(&geom, &params, sw, sh);
            assert_ne!(g, WARP_IDENTITY);
            let (fw, fh) = geom.display_dims(sw, sh);
            for (u, v) in [(0., 0.), (1., 0.), (1., 1.), (0., 1.), (0.5, 0.5)] {
                let (x, y) = crop_sample_warp(
                    u,
                    v,
                    rect,
                    geom.angle,
                    false,
                    false,
                    geom.rotation,
                    fw,
                    fh,
                    &g,
                )
                .expect("finite sample");
                assert!(
                    (-0.6..=sw + 0.6).contains(&x) && (-0.6..=sh + 0.6).contains(&y),
                    "case {i}: corner ({u},{v}) -> ({x},{y}) rect {rect:?}"
                );
            }
        }
    }

    #[test]
    fn v22_transform_params_pad_and_warp() {
        let legacy = vec![0.5f32; 63];
        let padded = pad_params(&legacy).unwrap();
        assert_eq!(padded[67], 100.);
        assert!(transform_of(&padded).is_identity());
        let geom = CropGeom::default();
        assert_eq!(geom_warp(&geom, &defaults(), 3., 2.), WARP_IDENTITY);
        // Upright's solution only counts while a mode is on.
        let mut geom = CropGeom::default();
        geom.upright.auto = [-20., 0., 1.];
        assert_eq!(geom_warp(&geom, &defaults(), 3., 2.), WARP_IDENTITY);
        geom.upright.mode = crate::upright::UprightMode::Vertical.code();
        assert_ne!(geom_warp(&geom, &defaults(), 3., 2.), WARP_IDENTITY);
        assert!(!geom.is_default());
        // Unconstrained keeps the rect as drawn.
        geom.upright.constrain = false;
        geom.rect = [0., 0., 1., 1.];
        assert_eq!(
            constrain_geom(&geom, &defaults(), 6000., 4000.).0,
            [0., 0., 1., 1.]
        );
        geom.upright.constrain = true;
        assert!(constrain_geom(&geom, &defaults(), 6000., 4000.).0[2] < 1.);
    }

    #[test]
    fn v22_display_to_output_inverts_the_sampler() {
        let mut params = defaults();
        params[63] = -35.;
        params[64] = 10.;
        params[65] = 2.;
        let geom = CropGeom {
            rect: [0.1, 0.15, 0.7, 0.6],
            angle: 6.,
            flip_h: true,
            ..CropGeom::default()
        };
        let (fw, fh) = (6000., 4000.);
        let g = geom_warp(&geom, &params, fw, fh);
        for (u, v) in [(0.2, 0.3), (0.5, 0.5), (0.9, 0.1)] {
            let (px, py) =
                crop_sample_warp(u, v, geom.rect, geom.angle, true, false, 0, fw, fh, &g).unwrap();
            let (ou, ov) = display_to_output(
                px / fw,
                py / fh,
                geom.rect,
                geom.angle,
                true,
                false,
                fw,
                fh,
                &g,
            )
            .unwrap();
            assert!(
                (ou - u).abs() < 1e-3 && (ov - v).abs() < 1e-3,
                "({u},{v}) -> ({ou},{ov})"
            );
        }
    }
}
