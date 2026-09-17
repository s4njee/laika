//! V22: perspective correction ("Transform" / Upright).
//!
//! All geometry here lives in DISPLAY space (source after quarter turns,
//! before flips) using centered coordinates: for a display uv `(u, v)`
//! and display aspect `a = dw / dh`, `x = (u - 0.5) * a`, `y = v - 0.5`
//! (y down). The forward transform `F` maps source → corrected; the
//! renderer samples with its inverse `G` (corrected → source), row-major
//! 3×3, which is what `laika_develop::CropRender::warp` carries.
//!
//! The user-facing controls are Lightroom-shaped: Vertical / Horizontal
//! keystone (±100), Rotate (±10°), Aspect (±100), Scale (50..150) and
//! X/Y offset (±100). Upright modes (Auto, Level, Vertical, Full, Guided)
//! solve keystone + rotate from straight lines and store that correction
//! separately; it adds to the manual sliders.

use crate::lines::Segment;

pub type Mat3 = [f64; 9];

pub const IDENTITY: Mat3 = [1., 0., 0., 0., 1., 0., 0., 0., 1.];

/// Degrees of tilt per slider unit (±100 → ±30° plane tilt).
const DEG_PER_UNIT: f64 = 0.3;

pub fn mul(a: &Mat3, b: &Mat3) -> Mat3 {
    let mut m = [0.; 9];
    for r in 0..3 {
        for c in 0..3 {
            m[r * 3 + c] = (0..3).map(|k| a[r * 3 + k] * b[k * 3 + c]).sum();
        }
    }
    m
}

pub fn invert(m: &Mat3) -> Option<Mat3> {
    let [a, b, c, d, e, f, g, h, i] = *m;
    let co = [
        e * i - f * h,
        -(d * i - f * g),
        d * h - e * g,
        -(b * i - c * h),
        a * i - c * g,
        -(a * h - b * g),
        b * f - c * e,
        -(a * f - c * d),
        a * e - b * d,
    ];
    let det = a * co[0] + b * co[1] + c * co[2];
    if !det.is_finite() || det.abs() < 1e-12 {
        return None;
    }
    // Adjugate is the transpose of the cofactor matrix.
    Some([
        co[0] / det,
        co[3] / det,
        co[6] / det,
        co[1] / det,
        co[4] / det,
        co[7] / det,
        co[2] / det,
        co[5] / det,
        co[8] / det,
    ])
}

/// Apply a homography to a centered point (None at the horizon).
pub fn apply(m: &Mat3, x: f64, y: f64) -> Option<(f64, f64)> {
    let w = m[6] * x + m[7] * y + m[8];
    if w.abs() < 1e-9 {
        return None;
    }
    Some((
        (m[0] * x + m[1] * y + m[2]) / w,
        (m[3] * x + m[4] * y + m[5]) / w,
    ))
}

fn translate(tx: f64, ty: f64) -> Mat3 {
    [1., 0., tx, 0., 1., ty, 0., 0., 1.]
}

fn scale(sx: f64, sy: f64) -> Mat3 {
    [sx, 0., 0., 0., sy, 0., 0., 0., 1.]
}

fn rotate_deg(deg: f64) -> Mat3 {
    let (s, c) = deg.to_radians().sin_cos();
    // y is down: positive degrees turn the picture clockwise as viewed.
    [c, -s, 0., s, c, 0., 0., 0., 1.]
}

/// Manual + solved correction in slider units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Transform {
    pub vertical: f32,
    pub horizontal: f32,
    pub rotate: f32,
    pub aspect: f32,
    pub scale: f32,
    pub offset_x: f32,
    pub offset_y: f32,
}

impl Default for Transform {
    fn default() -> Self {
        Self {
            vertical: 0.,
            horizontal: 0.,
            rotate: 0.,
            aspect: 0.,
            scale: 100.,
            offset_x: 0.,
            offset_y: 0.,
        }
    }
}

impl Transform {
    pub fn is_identity(&self) -> bool {
        *self == Self::default()
    }

    /// Manual sliders plus an Upright solution (keystone + rotate).
    pub fn with_auto(mut self, auto: [f32; 3]) -> Self {
        self.vertical += auto[0];
        self.horizontal += auto[1];
        self.rotate += auto[2];
        self
    }
}

/// Keystone only: tilt the picture plane (vertical about the x axis,
/// horizontal about the y axis) and re-project with a normal-lens focal
/// length, keeping the frame center fixed.
fn keystone(vertical: f64, horizontal: f64, aspect: f64) -> Mat3 {
    if vertical == 0. && horizontal == 0. {
        return IDENTITY;
    }
    let f = aspect.hypot(1.);
    // Negative Vertical widens the top (fixes a building shot from below);
    // positive Horizontal enlarges the right side.
    let tv = (-vertical * DEG_PER_UNIT).to_radians();
    let th = (horizontal * DEG_PER_UNIT).to_radians();
    let (sv, cv) = tv.sin_cos();
    let (sh, ch) = th.sin_cos();
    let rx: Mat3 = [1., 0., 0., 0., cv, -sv, 0., sv, cv];
    let ry: Mat3 = [ch, 0., sh, 0., 1., 0., -sh, 0., ch];
    let r = mul(&ry, &rx);
    let k = scale(f, f);
    let k_inv = scale(1. / f, 1. / f);
    let p = mul(&k, &mul(&r, &k_inv));
    // Keep the frame center where it was.
    match apply(&p, 0., 0.) {
        Some((cx, cy)) => mul(&translate(-cx, -cy), &p),
        None => p,
    }
}

/// Forward map F: source centered → corrected centered.
pub fn forward(t: &Transform, aspect: f32) -> Mat3 {
    let a = aspect.max(1e-3) as f64;
    let p = keystone(t.vertical as f64, t.horizontal as f64, a);
    let rot = rotate_deg(t.rotate as f64);
    // Aspect ±100 stretches up to √2 along one axis, shrinking the other.
    let k = 2f64.powf(t.aspect as f64 / 200.);
    let asp = scale(k, 1. / k);
    let s = (t.scale as f64 / 100.).max(0.05);
    let sc = scale(s, s);
    let off = translate(
        t.offset_x as f64 / 100. * a / 2.,
        t.offset_y as f64 / 100. / 2.,
    );
    mul(&off, &mul(&sc, &mul(&asp, &mul(&rot, &p))))
}

/// Render matrix G (corrected → source), identity for no correction.
pub fn warp_matrix(t: &Transform, aspect: f32) -> [f32; 9] {
    if t.is_identity() {
        return IDENTITY.map(|v| v as f32);
    }
    invert(&forward(t, aspect))
        .unwrap_or(IDENTITY)
        .map(|v| v as f32)
}

/// Display-uv helpers around the centered convention.
pub fn to_centered(u: f64, v: f64, aspect: f64) -> (f64, f64) {
    ((u - 0.5) * aspect, v - 0.5)
}

pub fn from_centered(x: f64, y: f64, aspect: f64) -> (f64, f64) {
    (x / aspect.max(1e-9) + 0.5, y + 0.5)
}

/// Warp a display uv through G (corrected → source), as the shader does.
pub fn warp_uv(g: &[f32; 9], u: f32, v: f32, aspect: f32) -> Option<(f32, f32)> {
    let a = aspect.max(1e-3) as f64;
    let m = g.map(|x| x as f64);
    let (x, y) = to_centered(u as f64, v as f64, a);
    let (sx, sy) = apply(&m, x, y)?;
    let (su, sv) = from_centered(sx, sy, a);
    Some((su as f32, sv as f32))
}

// ---- Upright modes ----------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum UprightMode {
    #[default]
    Off,
    Auto,
    Level,
    Vertical,
    Full,
    Guided,
}

impl UprightMode {
    pub const ALL: [UprightMode; 6] = [
        UprightMode::Off,
        UprightMode::Auto,
        UprightMode::Level,
        UprightMode::Vertical,
        UprightMode::Full,
        UprightMode::Guided,
    ];

    pub fn label(self) -> &'static str {
        match self {
            UprightMode::Off => "Off",
            UprightMode::Auto => "Auto",
            UprightMode::Level => "Level",
            UprightMode::Vertical => "Vertical",
            UprightMode::Full => "Full",
            UprightMode::Guided => "Guided",
        }
    }

    pub fn code(self) -> u8 {
        self as u8
    }

    pub fn from_code(c: u8) -> Self {
        Self::ALL.get(c as usize).copied().unwrap_or_default()
    }

    pub fn parse(s: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|m| m.label().eq_ignore_ascii_case(s.trim()))
            .unwrap_or_default()
    }
}

/// A straight edge in display uv (pre-correction), with a weight.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Line {
    pub u0: f32,
    pub v0: f32,
    pub u1: f32,
    pub v1: f32,
    pub weight: f32,
}

impl Line {
    /// Angle from horizontal in degrees on screen (display pixels).
    fn screen_angle(&self, aspect: f64) -> f64 {
        let dx = (self.u1 - self.u0) as f64 * aspect;
        let dy = (self.v1 - self.v0) as f64;
        dy.atan2(dx).to_degrees()
    }

    pub fn is_vertical(&self, aspect: f32) -> bool {
        let a = self.screen_angle(aspect as f64).abs();
        (45. ..=135.).contains(&a)
    }
}

/// Detected segments (analysis pixels of the SOURCE image, pre-rotation)
/// → display-uv lines for a quarter-turn `rotation`.
pub fn segments_to_lines(segs: &[Segment], w: usize, h: usize, rotation: u8) -> Vec<Line> {
    let (w, h) = (w.max(1) as f32, h.max(1) as f32);
    let to_display = |x: f32, y: f32| {
        let (su, sv) = (x / w, y / h);
        match rotation % 4 {
            1 => (1. - sv, su),
            2 => (1. - su, 1. - sv),
            3 => (sv, 1. - su),
            _ => (su, sv),
        }
    };
    segs.iter()
        .map(|s| {
            let (u0, v0) = to_display(s.x0, s.y0);
            let (u1, v1) = to_display(s.x1, s.y1);
            Line {
                u0,
                v0,
                u1,
                v1,
                weight: s.length().max(1.),
            }
        })
        .collect()
}

/// Which keystone parameters a mode may move (rotate always moves).
fn free_params(mode: UprightMode) -> (bool, bool) {
    match mode {
        UprightMode::Off | UprightMode::Level => (false, false),
        UprightMode::Vertical => (true, false),
        UprightMode::Auto | UprightMode::Full | UprightMode::Guided => (true, true),
    }
}

/// Signed deviation (degrees) of a corrected line from its target axis.
fn residual(line: &Line, vertical: bool, f: &Mat3, aspect: f64) -> Option<f64> {
    let (x0, y0) = to_centered(line.u0 as f64, line.v0 as f64, aspect);
    let (x1, y1) = to_centered(line.u1 as f64, line.v1 as f64, aspect);
    let (a0, b0) = apply(f, x0, y0)?;
    let (a1, b1) = apply(f, x1, y1)?;
    let (dx, dy) = (a1 - a0, b1 - b0);
    if dx.hypot(dy) < 1e-9 {
        return None;
    }
    let mut ang = if vertical {
        dx.atan2(dy).to_degrees()
    } else {
        dy.atan2(dx).to_degrees()
    };
    // Direction-free: fold into (-90, 90].
    while ang > 90. {
        ang -= 180.;
    }
    while ang <= -90. {
        ang += 180.;
    }
    Some(ang)
}

/// Result of an Upright solve.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Solution {
    /// Vertical, Horizontal, Rotate (slider units / degrees).
    pub auto: [f32; 3],
    /// Lines that informed it (after outlier rejection).
    pub used: usize,
    /// Mean absolute residual before and after, degrees.
    pub before_deg: f32,
    pub after_deg: f32,
}

/// Solve keystone + rotate so vertical-ish lines become vertical and
/// horizontal-ish lines horizontal. `Level` rotates only, `Vertical`
/// adds vertical keystone, `Full`/`Guided` add horizontal keystone, and
/// `Auto` is Full with a pull toward small corrections so busy scenes
/// never swing wildly. Returns None when there is nothing to go on.
pub fn solve(mode: UprightMode, lines: &[Line], aspect: f32) -> Option<Solution> {
    if mode == UprightMode::Off {
        return None;
    }
    let a = aspect.max(1e-3) as f64;
    // Lines far from both axes (roofs, diagonals) are not evidence.
    let classified: Vec<(Line, bool)> = lines
        .iter()
        .filter(|l| l.weight.is_finite() && l.weight > 0.)
        .filter_map(|l| {
            let ang = l.screen_angle(a).abs();
            let dev_v = (ang - 90.).abs();
            let dev_h = ang.min(180. - ang);
            let tol = if mode == UprightMode::Guided {
                45.
            } else {
                35.
            };
            if dev_v <= tol {
                Some((*l, true))
            } else if dev_h <= tol {
                Some((*l, false))
            } else {
                None
            }
        })
        .collect();
    if classified.is_empty() {
        return None;
    }
    let (mut free_v, mut free_h) = free_params(mode);
    let verticals = classified.iter().filter(|(_, v)| *v).count();
    let horizontals = classified.len() - verticals;
    // Keystone needs two lines of its family; one line only levels.
    if verticals < 2 {
        free_v = false;
    }
    if horizontals < 2 {
        free_h = false;
    }
    let reg = match mode {
        UprightMode::Auto => 0.0025,
        _ => 0.00002,
    };
    let limit = match mode {
        UprightMode::Auto => 60.,
        _ => 100.,
    };
    let total_w: f64 = classified.iter().map(|(l, _)| l.weight as f64).sum();
    let transform = |p: [f64; 3]| {
        forward(
            &Transform {
                vertical: p[0] as f32,
                horizontal: p[1] as f32,
                rotate: p[2] as f32,
                ..Transform::default()
            },
            aspect,
        )
    };
    // Robust weights: Huber on the residual, scale 1.5°.
    let cost = |p: [f64; 3], weights: &[f64]| -> f64 {
        let f = transform(p);
        let mut c = 0.;
        for ((l, v), w) in classified.iter().zip(weights) {
            let r = residual(l, *v, &f, a).unwrap_or(45.);
            c += w * r * r;
        }
        c / total_w.max(1e-9) + reg * (p[0] * p[0] + p[1] * p[1])
    };
    let mean_abs = |p: [f64; 3]| -> f64 {
        let f = transform(p);
        let mut s = 0.;
        for (l, v) in &classified {
            s += l.weight as f64 * residual(l, *v, &f, a).unwrap_or(45.).abs();
        }
        s / total_w.max(1e-9)
    };
    let mut p = [0f64; 3];
    let before = mean_abs(p);
    let mut weights: Vec<f64> = classified.iter().map(|(l, _)| l.weight as f64).collect();
    let free = [free_v, free_h, true];
    for _round in 0..4 {
        // Coordinate-wise Newton steps with a backtracking line search:
        // three parameters and smooth cost, so this converges quickly.
        for _ in 0..60 {
            let mut moved = false;
            for k in 0..3 {
                if !free[k] {
                    continue;
                }
                let h = if k == 2 { 0.02 } else { 0.05 };
                let c0 = cost(p, &weights);
                let mut pp = p;
                pp[k] += h;
                let cp = cost(pp, &weights);
                pp[k] = p[k] - h;
                let cm = cost(pp, &weights);
                let g = (cp - cm) / (2. * h);
                let curv = (cp - 2. * c0 + cm) / (h * h);
                let mut step = if curv > 1e-9 {
                    -g / curv
                } else {
                    -g.signum() * h
                };
                let cap = if k == 2 { 10. } else { 20. };
                step = step.clamp(-cap, cap);
                for _ in 0..12 {
                    let mut cand = p;
                    cand[k] = (p[k] + step).clamp(-limit, limit);
                    if k == 2 {
                        cand[k] = cand[k].clamp(-45., 45.);
                    }
                    if cost(cand, &weights) < c0 - 1e-12 {
                        if (cand[k] - p[k]).abs() > 1e-6 {
                            moved = true;
                        }
                        p = cand;
                        break;
                    }
                    step /= 2.;
                }
            }
            if !moved {
                break;
            }
        }
        // Reweight: lines that still disagree lose influence.
        let f = transform(p);
        for (i, (l, v)) in classified.iter().enumerate() {
            let r = residual(l, *v, &f, a).unwrap_or(45.).abs();
            let huber = if r <= 1.5 { 1. } else { 1.5 / r };
            weights[i] = l.weight as f64 * huber;
        }
    }
    let f = transform(p);
    let used = classified
        .iter()
        .filter(|(l, v)| residual(l, *v, &f, a).is_some_and(|r| r.abs() <= 3.))
        .count();
    Some(Solution {
        auto: [p[0] as f32, p[1] as f32, p[2] as f32],
        used,
        before_deg: before as f32,
        after_deg: mean_abs(p) as f32,
    })
}

// ---- persisted state ----------------------------------------------------------

/// Upright state carried with the crop geometry.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Upright {
    /// `UprightMode::code()`.
    #[serde(default)]
    pub mode: u8,
    /// Solved Vertical, Horizontal, Rotate — added to the manual sliders.
    #[serde(default)]
    pub auto: [f32; 3],
    /// Guided lines in display uv: x0, y0, x1, y1 (up to four).
    #[serde(default)]
    pub guides: [[f32; 4]; 4],
    #[serde(default)]
    pub n_guides: u8,
    /// Shrink the crop so corrected frames never show empty corners.
    #[serde(default = "crate::edit::flag_on")]
    pub constrain: bool,
}

impl Default for Upright {
    fn default() -> Self {
        Self {
            mode: 0,
            auto: [0.; 3],
            guides: [[0.; 4]; 4],
            n_guides: 0,
            constrain: true,
        }
    }
}

impl Upright {
    pub fn mode(&self) -> UprightMode {
        UprightMode::from_code(self.mode)
    }

    pub fn guide_lines(&self) -> Vec<Line> {
        self.guides[..(self.n_guides.min(4) as usize)]
            .iter()
            .map(|g| Line {
                u0: g[0],
                v0: g[1],
                u1: g[2],
                v1: g[3],
                weight: 1.,
            })
            .collect()
    }

    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// `laika:UprightGuides` text: `u0,v0,u1,v1;…` with 4 decimals.
    pub fn guides_text(&self) -> String {
        self.guides[..(self.n_guides.min(4) as usize)]
            .iter()
            .map(|g| {
                g.iter()
                    .map(|v| format!("{v:.4}"))
                    .collect::<Vec<_>>()
                    .join(",")
            })
            .collect::<Vec<_>>()
            .join(";")
    }

    /// Parse guide text; malformed entries are skipped, never fatal.
    pub fn set_guides_text(&mut self, s: &str) {
        self.n_guides = 0;
        for part in s.split(';') {
            let nums: Vec<f32> = part
                .split(',')
                .filter_map(|t| t.trim().parse::<f32>().ok())
                .filter(|v| v.is_finite())
                .collect();
            if nums.len() == 4 && (self.n_guides as usize) < 4 {
                self.guides[self.n_guides as usize] = [
                    nums[0].clamp(-1., 2.),
                    nums[1].clamp(-1., 2.),
                    nums[2].clamp(-1., 2.),
                    nums[3].clamp(-1., 2.),
                ];
                self.n_guides += 1;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: f32 = 1.5;

    fn close(a: f64, b: f64, tol: f64) -> bool {
        (a - b).abs() <= tol
    }

    #[test]
    fn invert_round_trips() {
        let t = Transform {
            vertical: -35.,
            horizontal: 20.,
            rotate: 3.,
            aspect: 15.,
            scale: 110.,
            offset_x: 10.,
            offset_y: -5.,
        };
        let f = forward(&t, A);
        let g = invert(&f).unwrap();
        let id = mul(&f, &g);
        for (x, y) in id.iter().zip(IDENTITY) {
            assert!(close(*x, y, 1e-9), "{id:?}");
        }
        assert_eq!(
            warp_matrix(&Transform::default(), A),
            IDENTITY.map(|v| v as f32)
        );
    }

    #[test]
    fn sign_conventions_match_the_sliders() {
        let a = A as f64;
        let corner = |t: &Transform, u: f64, v: f64| {
            let (x, y) = to_centered(u, v, a);
            apply(&forward(t, A), x, y).unwrap()
        };
        // Negative Vertical widens the top edge relative to the bottom.
        let t = Transform {
            vertical: -40.,
            ..Transform::default()
        };
        let top = corner(&t, 1., 0.).0 - corner(&t, 0., 0.).0;
        let bottom = corner(&t, 1., 1.).0 - corner(&t, 0., 1.).0;
        assert!(top > bottom * 1.05, "top {top} bottom {bottom}");
        // Positive Horizontal makes the right edge taller.
        let t = Transform {
            horizontal: 40.,
            ..Transform::default()
        };
        let right = corner(&t, 1., 1.).1 - corner(&t, 1., 0.).1;
        let left = corner(&t, 0., 1.).1 - corner(&t, 0., 0.).1;
        assert!(right > left * 1.05, "right {right} left {left}");
        // Keystone keeps the frame center fixed.
        let c = corner(
            &Transform {
                vertical: 70.,
                horizontal: -50.,
                ..Transform::default()
            },
            0.5,
            0.5,
        );
        assert!(close(c.0, 0., 1e-9) && close(c.1, 0., 1e-9), "{c:?}");
        // Positive Rotate turns content clockwise: the right midpoint moves down.
        let t = Transform {
            rotate: 5.,
            ..Transform::default()
        };
        assert!(corner(&t, 1., 0.5).1 > 0.01);
        // Scale 50 halves distances; offsets move content right/down.
        let t = Transform {
            scale: 50.,
            offset_x: 100.,
            offset_y: 100.,
            ..Transform::default()
        };
        let p = corner(&t, 1., 1.);
        assert!(
            close(p.0, a / 4. + a / 2., 1e-9) && close(p.1, 0.25 + 0.5, 1e-9),
            "{p:?}"
        );
    }

    #[test]
    fn warp_uv_matches_the_matrix() {
        let t = Transform {
            vertical: -25.,
            rotate: 2.,
            ..Transform::default()
        };
        let f = forward(&t, A);
        let g = warp_matrix(&t, A);
        // A source point pushed forward and pulled back lands where it began.
        let (x, y) = to_centered(0.2, 0.3, A as f64);
        let (cx, cy) = apply(&f, x, y).unwrap();
        let (cu, cv) = from_centered(cx, cy, A as f64);
        let (su, sv) = warp_uv(&g, cu as f32, cv as f32, A).unwrap();
        assert!(
            close(su as f64, 0.2, 1e-4) && close(sv as f64, 0.3, 1e-4),
            "{su} {sv}"
        );
    }

    /// Lines of a synthetic scene: true verticals/horizontals pushed
    /// through a known distortion (the inverse of `truth`'s correction).
    fn distorted_scene(truth: &Transform, verticals: &[f64], horizontals: &[f64]) -> Vec<Line> {
        let a = A as f64;
        let g = invert(&forward(truth, A)).unwrap();
        let mut out = Vec::new();
        let mut push = |p0: (f64, f64), p1: (f64, f64)| {
            let (x0, y0) = apply(&g, p0.0, p0.1).unwrap();
            let (x1, y1) = apply(&g, p1.0, p1.1).unwrap();
            let (u0, v0) = from_centered(x0, y0, a);
            let (u1, v1) = from_centered(x1, y1, a);
            out.push(Line {
                u0: u0 as f32,
                v0: v0 as f32,
                u1: u1 as f32,
                v1: v1 as f32,
                weight: 100.,
            });
        };
        for &x in verticals {
            push((x, -0.4), (x, 0.4));
        }
        for &y in horizontals {
            push((-0.6, y), (0.6, y));
        }
        out
    }

    fn assert_straightened(mode: UprightMode, lines: &[Line], tol: f64) -> Solution {
        let sol = solve(mode, lines, A).expect("solution");
        let t = Transform::default().with_auto(sol.auto);
        let f = forward(&t, A);
        for l in lines {
            let vertical = l.is_vertical(A);
            let r = residual(l, vertical, &f, A as f64).unwrap();
            assert!(
                r.abs() <= tol,
                "{mode:?} residual {r} for {l:?} with {sol:?}"
            );
        }
        sol
    }

    #[test]
    fn guided_corrects_a_tilted_building() {
        // Shot from below and slightly to the side, camera rolled 1.5°.
        let truth = Transform {
            vertical: -38.,
            horizontal: 12.,
            rotate: 1.5,
            ..Transform::default()
        };
        let lines = distorted_scene(&truth, &[-0.5, 0.45], &[-0.3, 0.35]);
        let sol = assert_straightened(UprightMode::Guided, &lines, 0.1);
        assert!(sol.before_deg > 1.5 && sol.after_deg < 0.1, "{sol:?}");
        assert!((sol.auto[0] - truth.vertical).abs() < 1.5, "{sol:?}");
    }

    #[test]
    fn modes_limit_what_they_move() {
        let truth = Transform {
            vertical: -30.,
            horizontal: 15.,
            rotate: -2.,
            ..Transform::default()
        };
        let lines = distorted_scene(&truth, &[-0.55, -0.1, 0.5], &[-0.25, 0.3]);
        let level = solve(UprightMode::Level, &lines, A).unwrap();
        assert_eq!((level.auto[0], level.auto[1]), (0., 0.));
        let vertical = solve(UprightMode::Vertical, &lines, A).unwrap();
        assert_eq!(vertical.auto[1], 0.);
        assert!(vertical.auto[0] < -10., "{vertical:?}");
        assert_straightened(UprightMode::Full, &lines, 0.15);
        // Auto is Full, pulled toward smaller corrections.
        let auto = solve(UprightMode::Auto, &lines, A).unwrap();
        let full = solve(UprightMode::Full, &lines, A).unwrap();
        assert!(
            auto.auto[0].abs() <= full.auto[0].abs() + 0.01,
            "{auto:?} {full:?}"
        );
        assert!(auto.after_deg < auto.before_deg);
    }

    #[test]
    fn solver_handles_thin_evidence() {
        assert!(solve(UprightMode::Auto, &[], A).is_none());
        assert!(solve(UprightMode::Off, &[], A).is_none());
        // A single tilted vertical can only be leveled.
        let one = [Line {
            u0: 0.5,
            v0: 0.1,
            u1: 0.52,
            v1: 0.9,
            weight: 50.,
        }];
        let sol = solve(UprightMode::Guided, &one, A).unwrap();
        assert_eq!((sol.auto[0], sol.auto[1]), (0., 0.));
        assert!(sol.after_deg < 0.05, "{sol:?}");
        // A diagonal is ignored rather than forced to an axis.
        let diag = [Line {
            u0: 0.1,
            v0: 0.1,
            u1: 0.6,
            v1: 0.85,
            weight: 50.,
        }];
        assert!(solve(UprightMode::Full, &diag, A).is_none());
    }

    #[test]
    fn upright_state_round_trips() {
        let mut u = Upright::default();
        assert!(u.constrain && u.is_default());
        u.set_guides_text("0.1,0.2,0.1,0.9; bad; 0.8,0.1,0.75,0.95;1,2,3");
        assert_eq!(u.n_guides, 2);
        let mut v = Upright::default();
        v.set_guides_text(&u.guides_text());
        assert_eq!(v.guides, u.guides);
        let json = serde_json::to_string(&u).unwrap();
        assert_eq!(serde_json::from_str::<Upright>(&json).unwrap(), u);
        // Legacy JSON without the field keeps constrain on.
        let legacy: Upright = serde_json::from_str("{}").unwrap();
        assert!(legacy.constrain);
        assert_eq!(UprightMode::parse("vertical"), UprightMode::Vertical);
        assert_eq!(UprightMode::from_code(9), UprightMode::Off);
    }

    #[test]
    fn segments_rotate_into_display_space() {
        let seg = Segment {
            x0: 0.,
            y0: 0.,
            x1: 100.,
            y1: 0.,
            strength: 1.,
        };
        // Top edge of the source becomes the right edge after a CW turn.
        let l = segments_to_lines(&[seg], 100, 50, 1)[0];
        assert_eq!((l.u0, l.v0, l.u1, l.v1), (1., 0., 1., 1.));
        let l = segments_to_lines(&[seg], 100, 50, 3)[0];
        assert_eq!((l.u0, l.v0, l.u1, l.v1), (0., 1., 0., 0.));
    }
}
