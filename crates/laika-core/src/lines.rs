//! V22: straight line segments for Upright.
//!
//! Detects the dominant straight edges (building verticals, horizons,
//! rooflines) of a downscaled luminance image. A separate solver turns the
//! segments into perspective corrections; this module only detects.
//!
//! Pipeline:
//! 1. sanitize (NaN/inf -> 0), light 3x3 binomial blur;
//! 2. Sobel gradients (normalised so a unit step spread over 2 px ≈ 0.5);
//! 3. edge pixels = gradient-direction non-maximum suppression with
//!    sub-pixel (parabolic) localisation, adaptive threshold
//!    (`max(abs floor, 2 × median gradient)`, capped to the strongest 8 % of
//!    pixels);
//! 4. Hough accumulation (0.25° θ bins, 1 px ρ bins, ρ measured from the
//!    image centre) restricted to ±40° around vertical/horizontal, each
//!    edge pixel voting only within ±5° of its own gradient orientation;
//! 5. progressive peak picking (lazy max-heap): for each peak gather inliers
//!    (2.5 px, then 1.5 px, orientation within 15°), refine by weighted total
//!    least squares, split into contiguous runs (gaps ≤ 2 % of min dim), emit
//!    runs ≥ min length, and remove the inliers' votes from the accumulator;
//! 6. merge collinear duplicates / double edges, band filter, sort, cap at 60.
//!
//! Coordinates: pixel `(i, j)` covers `[i, i+1) × [j, j+1)`, so pixel centres
//! are at `(i + 0.5, j + 0.5)`; y points down.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

/// A detected straight edge in pixel coordinates of the analysed image.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Segment {
    pub x0: f32,
    pub y0: f32,
    pub x1: f32,
    pub y1: f32,
    /// Supporting edge evidence (≈ number of aligned edge pixels, gradient-weighted).
    pub strength: f32,
}

impl Segment {
    pub fn length(&self) -> f32 {
        (self.x1 - self.x0).hypot(self.y1 - self.y0)
    }

    /// Angle in degrees of the segment direction in (-90, 90], 0 = horizontal, +90 = vertical, y down.
    pub fn angle_deg(&self) -> f32 {
        let dx = self.x1 - self.x0;
        let dy = self.y1 - self.y0;
        if dx == 0.0 && dy == 0.0 {
            return 0.0;
        }
        let mut a = dy.atan2(dx).to_degrees();
        if a <= -90.0 {
            a += 180.0;
        } else if a > 90.0 {
            a -= 180.0;
        }
        a
    }

    /// true when within `tol_deg` of vertical.
    pub fn is_near_vertical(&self, tol_deg: f32) -> bool {
        90.0 - self.angle_deg().abs() <= tol_deg
    }

    /// true when within `tol_deg` of horizontal.
    pub fn is_near_horizontal(&self, tol_deg: f32) -> bool {
        self.angle_deg().abs() <= tol_deg
    }
}

// ---------------------------------------------------------------------------
// Parameters
// ---------------------------------------------------------------------------

const MAX_SEGMENTS: usize = 60;
/// Accepted deviation from vertical / horizontal.
const BAND_DEG: f32 = 40.0;
/// Hough θ bin width (θ = direction of the line normal).
const THETA_STEP: f32 = 0.25;
const THETA_MIN: f32 = -BAND_DEG; // normals of near-vertical lines: [-40, 40]
const THETA_MAX: f32 = 90.0 + BAND_DEG; // normals of near-horizontal lines: [50, 130]
/// Edge pixels vote only within this distance of their gradient orientation.
const VOTE_TOL_DEG: f32 = 5.0;
/// Inliers must have gradient orientation within this of the line normal.
const INLIER_TOL_DEG: f32 = 15.0;
const GATHER_TOL_WIDE: f32 = 2.5;
const GATHER_TOL: f32 = 1.5;
/// Absolute gradient floor (luminance units per pixel).
const MIN_GRAD: f32 = 0.01;
const NOISE_FACTOR: f32 = 2.0;
const MAX_EDGE_FRACTION: f32 = 0.08;
const MIN_LEN_FRACTION: f32 = 0.06;
const MIN_LEN_PX: f32 = 12.0;
const GAP_FRACTION: f32 = 0.02;
/// A run must have at least this many edge pixels per pixel of length.
const MIN_DENSITY: f32 = 0.35;
const MAX_ATTEMPTS: usize = 1500;
const MAX_RAW_SEGMENTS: usize = 400;
const MERGE_ANGLE_DEG: f32 = 2.0;

const NONE: u32 = u32::MAX;

#[derive(Clone, Copy)]
struct Edge {
    x: f32,
    y: f32,
    /// Gradient (normal) orientation in degrees, folded into [-50, 130).
    phi: f32,
    mag: f32,
    /// Vote weight.
    w: f32,
}

#[derive(PartialEq)]
struct Cand {
    v: f32,
    idx: u32,
}
impl Eq for Cand {}
impl Ord for Cand {
    fn cmp(&self, other: &Self) -> Ordering {
        self.v
            .total_cmp(&other.v)
            .then_with(|| other.idx.cmp(&self.idx))
    }
}
impl PartialOrd for Cand {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Smallest difference between two orientations (mod 180°), in [0, 90].
fn orient_diff(a: f32, b: f32) -> f32 {
    let d = (a - b).rem_euclid(180.0);
    d.min(180.0 - d)
}

fn fold_phi(mut phi: f32) -> f32 {
    while phi < -50.0 {
        phi += 180.0;
    }
    while phi >= 130.0 {
        phi -= 180.0;
    }
    phi
}

/// Luminance plane, row-major, `w * h` values.
pub fn detect_segments(lum: &[f32], w: usize, h: usize) -> Vec<Segment> {
    if w < 8 || h < 8 {
        return Vec::new();
    }
    let n = match w.checked_mul(h) {
        Some(n) if n <= lum.len() && n < (u32::MAX as usize) => n,
        _ => return Vec::new(),
    };

    let img: Vec<f32> = lum[..n]
        .iter()
        .map(|&v| {
            if v.is_finite() {
                v.clamp(-1.0e4, 1.0e4)
            } else {
                0.0
            }
        })
        .collect();

    let blurred = blur3(&img, w, h);
    drop(img);
    let (gx, gy, mag) = sobel(&blurred, w, h);
    drop(blurred);

    let edges = find_edges(&gx, &gy, &mag, w, h);
    drop(gx);
    drop(gy);
    drop(mag);
    if edges.len() < 2 {
        return Vec::new();
    }

    Detector::new(edges, w, h).run()
}

fn blur3(src: &[f32], w: usize, h: usize) -> Vec<f32> {
    let mut tmp = vec![0.0f32; w * h];
    for y in 0..h {
        let row = &src[y * w..(y + 1) * w];
        let out = &mut tmp[y * w..(y + 1) * w];
        for x in 0..w {
            let l = row[x.saturating_sub(1)];
            let r = row[(x + 1).min(w - 1)];
            out[x] = 0.25 * (l + r) + 0.5 * row[x];
        }
    }
    let mut dst = vec![0.0f32; w * h];
    for y in 0..h {
        let up = &tmp[y.saturating_sub(1) * w..];
        let mid = &tmp[y * w..];
        let dn = &tmp[(y + 1).min(h - 1) * w..];
        let out = &mut dst[y * w..(y + 1) * w];
        for x in 0..w {
            out[x] = 0.25 * (up[x] + dn[x]) + 0.5 * mid[x];
        }
    }
    dst
}

fn sobel(p: &[f32], w: usize, h: usize) -> (Vec<f32>, Vec<f32>, Vec<f32>) {
    let mut gx = vec![0.0f32; w * h];
    let mut gy = vec![0.0f32; w * h];
    let mut mag = vec![0.0f32; w * h];
    for y in 0..h {
        let up = &p[y.saturating_sub(1) * w..y.saturating_sub(1) * w + w];
        let mid = &p[y * w..y * w + w];
        let dn = &p[(y + 1).min(h - 1) * w..(y + 1).min(h - 1) * w + w];
        for x in 0..w {
            let xl = x.saturating_sub(1);
            let xr = (x + 1).min(w - 1);
            let sx = (up[xr] + 2.0 * mid[xr] + dn[xr]) - (up[xl] + 2.0 * mid[xl] + dn[xl]);
            let sy = (dn[xl] + 2.0 * dn[x] + dn[xr]) - (up[xl] + 2.0 * up[x] + up[xr]);
            let i = y * w + x;
            gx[i] = sx * 0.125;
            gy[i] = sy * 0.125;
            mag[i] = (gx[i] * gx[i] + gy[i] * gy[i]).sqrt();
        }
    }
    (gx, gy, mag)
}

fn find_edges(gx: &[f32], gy: &[f32], mag: &[f32], w: usize, h: usize) -> Vec<Edge> {
    // Noise estimate: median gradient magnitude over a pixel subsample.
    let mut sample: Vec<f32> = mag.iter().step_by(3).copied().collect();
    let median = if sample.is_empty() {
        0.0
    } else {
        let mid = sample.len() / 2;
        let (_, m, _) = sample.select_nth_unstable_by(mid, |a, b| a.total_cmp(b));
        *m
    };
    drop(sample);
    let thr = MIN_GRAD.max(NOISE_FACTOR * median);

    const TAN_22_5: f32 = 0.414_213_57;
    let at = |x: isize, y: isize| -> f32 {
        if x < 0 || y < 0 || x >= w as isize || y >= h as isize {
            0.0
        } else {
            mag[y as usize * w + x as usize]
        }
    };

    let mut edges = Vec::new();
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let m = mag[i];
            if !(m > thr) {
                continue;
            }
            let ax = gx[i].abs();
            let ay = gy[i].abs();
            let (dx, dy): (isize, isize) = if ay <= ax * TAN_22_5 {
                (1, 0)
            } else if ax <= ay * TAN_22_5 {
                (0, 1)
            } else if gx[i] * gy[i] > 0.0 {
                (1, 1)
            } else {
                (1, -1)
            };
            let (xi, yi) = (x as isize, y as isize);
            let mm = at(xi - dx, yi - dy);
            let mp = at(xi + dx, yi + dy);
            if !(m >= mm && m > mp) {
                continue;
            }
            let inside = xi - dx >= 0
                && xi + dx < w as isize
                && yi - dy >= 0
                && yi + dy < h as isize
                && yi - dy < h as isize
                && yi + dy >= 0;
            let mut t = 0.0;
            if inside {
                let denom = mm - 2.0 * m + mp;
                if denom < -1e-12 {
                    t = (0.5 * (mm - mp) / denom).clamp(-0.5, 0.5);
                }
            }
            let phi = fold_phi(gy[i].atan2(gx[i]).to_degrees());
            edges.push(Edge {
                x: x as f32 + 0.5 + t * dx as f32,
                y: y as f32 + 0.5 + t * dy as f32,
                phi,
                mag: m,
                w: 1.0,
            });
        }
    }

    let max_edges = ((w * h) as f32 * MAX_EDGE_FRACTION) as usize;
    if edges.len() > max_edges.max(16) {
        let k = max_edges.max(16);
        edges.select_nth_unstable_by(k, |a, b| b.mag.total_cmp(&a.mag));
        edges.truncate(k);
        // Restore raster order for determinism / cache friendliness.
        edges.sort_by(|a, b| {
            (a.y as u32, a.x as u32)
                .cmp(&(b.y as u32, b.x as u32))
                .then(a.y.total_cmp(&b.y))
                .then(a.x.total_cmp(&b.x))
        });
    }

    if !edges.is_empty() {
        let mut mags: Vec<f32> = edges.iter().map(|e| e.mag).collect();
        let mid = mags.len() / 2;
        let (_, r, _) = mags.select_nth_unstable_by(mid, |a, b| a.total_cmp(b));
        let reference = r.max(1e-6);
        for e in &mut edges {
            e.w = (e.mag / reference).clamp(0.25, 2.0);
        }
    }
    edges
}

struct Line {
    cx: f32,
    cy: f32,
    dx: f32,
    dy: f32,
}

struct Detector {
    edges: Vec<Edge>,
    used: Vec<bool>,
    map: Vec<u32>,
    w: usize,
    h: usize,
    n_theta: usize,
    n_rho: usize,
    rho_off: f32,
    cos_t: Vec<f32>,
    sin_t: Vec<f32>,
    valid_t: Vec<bool>,
    acc: Vec<f32>,
    ccx: f32,
    ccy: f32,
    min_len: f32,
    max_gap: f32,
}

impl Detector {
    fn new(edges: Vec<Edge>, w: usize, h: usize) -> Self {
        let n_theta = ((THETA_MAX - THETA_MIN) / THETA_STEP).round() as usize + 1;
        let half_diag = 0.5 * (w as f32).hypot(h as f32);
        let rho_off = half_diag.ceil() + 1.0;
        let n_rho = 2 * rho_off as usize + 2;
        let mut cos_t = Vec::with_capacity(n_theta);
        let mut sin_t = Vec::with_capacity(n_theta);
        let mut valid_t = Vec::with_capacity(n_theta);
        for b in 0..n_theta {
            let th = THETA_MIN as f64 + b as f64 * THETA_STEP as f64;
            let r = th.to_radians();
            cos_t.push(r.cos() as f32);
            sin_t.push(r.sin() as f32);
            valid_t.push(th <= BAND_DEG as f64 + 1e-6 || th >= (90.0 - BAND_DEG) as f64 - 1e-6);
        }
        let mut map = vec![NONE; w * h];
        for (k, e) in edges.iter().enumerate() {
            let xi = (e.x.floor() as isize).clamp(0, w as isize - 1) as usize;
            let yi = (e.y.floor() as isize).clamp(0, h as isize - 1) as usize;
            let slot = &mut map[yi * w + xi];
            // Sub-pixel shifts can land two edges in one cell; keep the stronger.
            if *slot == NONE || edges[*slot as usize].mag < e.mag {
                *slot = k as u32;
            }
        }
        let min_dim = w.min(h) as f32;
        let used = vec![false; edges.len()];
        Detector {
            edges,
            used,
            map,
            w,
            h,
            n_theta,
            n_rho,
            rho_off,
            cos_t,
            sin_t,
            valid_t,
            acc: vec![0.0; n_theta * n_rho],
            ccx: w as f32 * 0.5,
            ccy: h as f32 * 0.5,
            min_len: (MIN_LEN_FRACTION * min_dim).max(MIN_LEN_PX),
            max_gap: (GAP_FRACTION * min_dim).max(3.0),
        }
    }

    fn vote(&mut self, k: usize, sign: f32) {
        let e = self.edges[k];
        let ex = e.x - self.ccx;
        let ey = e.y - self.ccy;
        let wv = e.w * sign;
        for base in [e.phi - 180.0, e.phi, e.phi + 180.0] {
            let lo = base - VOTE_TOL_DEG;
            let hi = base + VOTE_TOL_DEG;
            if hi < THETA_MIN || lo > THETA_MAX {
                continue;
            }
            let b0 = ((lo - THETA_MIN) / THETA_STEP).ceil().max(0.0) as usize;
            let b1 =
                (((hi - THETA_MIN) / THETA_STEP).floor() as isize).min(self.n_theta as isize - 1);
            if b1 < b0 as isize {
                continue;
            }
            for b in b0..=b1 as usize {
                if !self.valid_t[b] {
                    continue;
                }
                let r = ex * self.cos_t[b] + ey * self.sin_t[b] + self.rho_off;
                let ri = r.round();
                if ri < 0.0 || ri as usize >= self.n_rho {
                    continue;
                }
                self.acc[b * self.n_rho + ri as usize] += wv;
            }
        }
    }

    /// Unused edge pixels within `tol` px of the line whose gradient
    /// orientation is within `tol_deg` of the line normal.
    fn gather(&self, line: &Line, tol: f32, tol_deg: f32, out: &mut Vec<u32>) {
        out.clear();
        let (nx, ny) = (-line.dy, line.dx);
        let theta_n = ny.atan2(nx).to_degrees();
        let (w, h) = (self.w as isize, self.h as isize);
        let check = |xi: isize, yi: isize, out: &mut Vec<u32>| {
            if xi < 0 || yi < 0 || xi >= w || yi >= h {
                return;
            }
            let k = self.map[yi as usize * self.w + xi as usize];
            if k == NONE || self.used[k as usize] {
                return;
            }
            let e = &self.edges[k as usize];
            let d = ((e.x - line.cx) * nx + (e.y - line.cy) * ny).abs();
            if d <= tol && orient_diff(e.phi, theta_n) <= tol_deg {
                out.push(k);
            }
        };
        if line.dy.abs() >= line.dx.abs() {
            let r = tol / line.dy.abs() + 1.0;
            for yi in 0..h {
                let t = (yi as f32 + 0.5 - line.cy) / line.dy;
                let x = line.cx + t * line.dx;
                let x0 = (x - r - 0.5).floor() as isize;
                let x1 = (x + r - 0.5).ceil() as isize;
                if x1 < 0 || x0 >= w {
                    continue;
                }
                for xi in x0.max(0)..=x1.min(w - 1) {
                    check(xi, yi, out);
                }
            }
        } else {
            let r = tol / line.dx.abs() + 1.0;
            for xi in 0..w {
                let t = (xi as f32 + 0.5 - line.cx) / line.dx;
                let y = line.cy + t * line.dy;
                let y0 = (y - r - 0.5).floor() as isize;
                let y1 = (y + r - 0.5).ceil() as isize;
                if y1 < 0 || y0 >= h {
                    continue;
                }
                for yi in y0.max(0)..=y1.min(h - 1) {
                    check(xi, yi, out);
                }
            }
        }
    }

    /// Weighted total least squares line fit.
    fn fit(&self, idx: &[u32]) -> Option<Line> {
        if idx.len() < 2 {
            return None;
        }
        let (mut sw, mut sx, mut sy) = (0.0f64, 0.0f64, 0.0f64);
        for &k in idx {
            let e = &self.edges[k as usize];
            let wt = e.w as f64;
            sw += wt;
            sx += wt * e.x as f64;
            sy += wt * e.y as f64;
        }
        if sw <= 0.0 {
            return None;
        }
        let (mx, my) = (sx / sw, sy / sw);
        let (mut cxx, mut cxy, mut cyy) = (0.0f64, 0.0f64, 0.0f64);
        for &k in idx {
            let e = &self.edges[k as usize];
            let wt = e.w as f64;
            let (ux, uy) = (e.x as f64 - mx, e.y as f64 - my);
            cxx += wt * ux * ux;
            cxy += wt * ux * uy;
            cyy += wt * uy * uy;
        }
        if cxx + cyy <= 1e-9 {
            return None;
        }
        let a = 0.5 * (2.0 * cxy).atan2(cxx - cyy);
        Some(Line {
            cx: mx as f32,
            cy: my as f32,
            dx: a.cos() as f32,
            dy: a.sin() as f32,
        })
    }

    fn run(mut self) -> Vec<Segment> {
        for k in 0..self.edges.len() {
            self.vote(k, 1.0);
        }

        let min_votes = 0.3 * self.min_len;
        let mut heap: BinaryHeap<Cand> = self
            .acc
            .iter()
            .enumerate()
            .filter(|&(_, &v)| v >= min_votes)
            .map(|(i, &v)| Cand { v, idx: i as u32 })
            .collect();

        let mut raw: Vec<Segment> = Vec::new();
        let mut attempts = 0usize;
        let mut cand: Vec<u32> = Vec::new();
        let mut inl: Vec<u32> = Vec::new();

        while let Some(c) = heap.pop() {
            if attempts >= MAX_ATTEMPTS || raw.len() >= MAX_RAW_SEGMENTS {
                break;
            }
            let cur = self.acc[c.idx as usize];
            if cur < min_votes {
                continue;
            }
            if cur < c.v - 1e-3 {
                heap.push(Cand { v: cur, idx: c.idx });
                continue;
            }
            attempts += 1;

            let b = c.idx as usize / self.n_rho;
            let ri = c.idx as usize % self.n_rho;
            let rho = ri as f32 - self.rho_off;
            let (ct, st) = (self.cos_t[b], self.sin_t[b]);
            let bin_line = Line {
                cx: self.ccx + rho * ct,
                cy: self.ccy + rho * st,
                dx: -st,
                dy: ct,
            };
            self.gather(&bin_line, GATHER_TOL_WIDE, INLIER_TOL_DEG, &mut cand);

            // Pixels to retire: the tight core of the bin plus final inliers.
            let mut remove: Vec<u32> = cand
                .iter()
                .copied()
                .filter(|&k| {
                    let e = &self.edges[k as usize];
                    ((e.x - bin_line.cx) * ct + (e.y - bin_line.cy) * st).abs() <= 0.75
                })
                .collect();

            let mut final_line = None;
            if cand.len() >= 3 {
                if let Some(mut line) = self.fit(&cand) {
                    let mut ok = true;
                    for _ in 0..2 {
                        self.gather(&line, GATHER_TOL, INLIER_TOL_DEG, &mut inl);
                        match self.fit(&inl) {
                            Some(l) => line = l,
                            None => {
                                ok = false;
                                break;
                            }
                        }
                    }
                    if ok {
                        self.gather(&line, GATHER_TOL, INLIER_TOL_DEG, &mut inl);
                        if inl.len() >= 2 {
                            remove.extend_from_slice(&inl);
                            final_line = Some(line);
                        }
                    }
                }
            }

            if let Some(line) = final_line {
                self.emit_runs(&line, &inl, &mut raw);
            }

            remove.sort_unstable();
            remove.dedup();
            for &k in &remove {
                if !self.used[k as usize] {
                    self.used[k as usize] = true;
                    self.vote(k as usize, -1.0);
                }
            }
            let slot = &mut self.acc[c.idx as usize];
            *slot = slot.min(0.0);
        }

        let min_dim = self.w.min(self.h) as f32;
        let dist_tol = (0.004 * min_dim).max(2.5);
        let mut segs = merge_duplicates(raw, dist_tol, 2.0 * self.max_gap);
        segs.retain(|s| {
            let a = s.angle_deg().abs();
            s.length() >= self.min_len && (a <= BAND_DEG + 1e-3 || a >= 90.0 - BAND_DEG - 1e-3)
        });
        for s in &mut segs {
            normalize_endpoints(s);
        }
        segs.sort_by(|a, b| {
            b.strength
                .total_cmp(&a.strength)
                .then(a.x0.total_cmp(&b.x0))
                .then(a.y0.total_cmp(&b.y0))
        });
        segs.truncate(MAX_SEGMENTS);
        segs
    }

    fn emit_runs(&self, line: &Line, inl: &[u32], raw: &mut Vec<Segment>) {
        let mut ts: Vec<(f32, u32)> = inl
            .iter()
            .map(|&k| {
                let e = &self.edges[k as usize];
                ((e.x - line.cx) * line.dx + (e.y - line.cy) * line.dy, k)
            })
            .collect();
        ts.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        let mut start = 0;
        let mut run_idx: Vec<u32> = Vec::new();
        for i in 1..=ts.len() {
            if i < ts.len() && ts[i].0 - ts[i - 1].0 <= self.max_gap {
                continue;
            }
            let run = &ts[start..i];
            start = i;
            let span = run[run.len() - 1].0 - run[0].0;
            if span < self.min_len || (run.len() as f32) < MIN_DENSITY * span || run.len() < 6 {
                continue;
            }
            run_idx.clear();
            run_idx.extend(run.iter().map(|&(_, k)| k));
            let Some(rl) = self.fit(&run_idx) else {
                continue;
            };
            let (mut tmin, mut tmax, mut strength) = (f32::INFINITY, f32::NEG_INFINITY, 0.0f32);
            for &k in &run_idx {
                let e = &self.edges[k as usize];
                let t = (e.x - rl.cx) * rl.dx + (e.y - rl.cy) * rl.dy;
                tmin = tmin.min(t);
                tmax = tmax.max(t);
                strength += e.w;
            }
            raw.push(Segment {
                x0: rl.cx + tmin * rl.dx,
                y0: rl.cy + tmin * rl.dy,
                x1: rl.cx + tmax * rl.dx,
                y1: rl.cy + tmax * rl.dy,
                strength,
            });
        }
    }
}

/// Near-vertical segments run top→bottom (y0 ≤ y1), others left→right (x0 ≤ x1).
fn normalize_endpoints(s: &mut Segment) {
    let flip = if s.angle_deg().abs() >= 45.0 {
        s.y0 > s.y1
    } else {
        s.x0 > s.x1
    };
    if flip {
        std::mem::swap(&mut s.x0, &mut s.x1);
        std::mem::swap(&mut s.y0, &mut s.y1);
    }
}

fn merge_duplicates(mut segs: Vec<Segment>, dist_tol: f32, gap_tol: f32) -> Vec<Segment> {
    segs.sort_by(|a, b| b.strength.total_cmp(&a.strength));
    let cos_tol = MERGE_ANGLE_DEG.to_radians().cos();
    loop {
        let mut merged_any = false;
        let mut i = 0;
        while i < segs.len() {
            let mut j = i + 1;
            while j < segs.len() {
                if let Some(m) = try_merge(&segs[i], &segs[j], dist_tol, gap_tol, cos_tol) {
                    segs[i] = m;
                    segs.remove(j);
                    merged_any = true;
                    j = i + 1;
                } else {
                    j += 1;
                }
            }
            i += 1;
        }
        if !merged_any {
            break;
        }
        segs.sort_by(|a, b| b.strength.total_cmp(&a.strength));
    }
    segs
}

fn unit_dir(s: &Segment) -> Option<(f32, f32, f32)> {
    let (dx, dy) = (s.x1 - s.x0, s.y1 - s.y0);
    let l = dx.hypot(dy);
    if l > 1e-6 {
        Some((dx / l, dy / l, l))
    } else {
        None
    }
}

fn try_merge(
    a: &Segment,
    b: &Segment,
    dist_tol: f32,
    gap_tol: f32,
    cos_tol: f32,
) -> Option<Segment> {
    let (ax, ay, la) = unit_dir(a)?;
    let (mut bx, mut by, lb) = unit_dir(b)?;
    let dot = ax * bx + ay * by;
    if dot.abs() < cos_tol {
        return None;
    }
    let (nx, ny) = (-ay, ax);
    let d0 = ((b.x0 - a.x0) * nx + (b.y0 - a.y0) * ny).abs();
    let d1 = ((b.x1 - a.x0) * nx + (b.y1 - a.y0) * ny).abs();
    if d0.max(d1) > dist_tol {
        return None;
    }
    let tb0 = (b.x0 - a.x0) * ax + (b.y0 - a.y0) * ay;
    let tb1 = (b.x1 - a.x0) * ax + (b.y1 - a.y0) * ay;
    let (tbmin, tbmax) = (tb0.min(tb1), tb0.max(tb1));
    let gap = (tbmin - la).max(-tbmax);
    if gap > gap_tol {
        return None;
    }
    let overlap = (la.min(tbmax) - tbmin.max(0.0)).max(0.0);
    let strength = a.strength + b.strength * (1.0 - overlap / lb).clamp(0.0, 1.0);

    if dot < 0.0 {
        bx = -bx;
        by = -by;
    }
    let (wa, wb) = (a.strength.max(1e-6), b.strength.max(1e-6));
    let (mut dx, mut dy) = (ax * wa + bx * wb, ay * wa + by * wb);
    let dl = dx.hypot(dy);
    if dl <= 1e-9 {
        return None;
    }
    dx /= dl;
    dy /= dl;
    let cx = ((a.x0 + a.x1) * 0.5 * wa + (b.x0 + b.x1) * 0.5 * wb) / (wa + wb);
    let cy = ((a.y0 + a.y1) * 0.5 * wa + (b.y0 + b.y1) * 0.5 * wb) / (wa + wb);
    let (mut tmin, mut tmax) = (f32::INFINITY, f32::NEG_INFINITY);
    for (px, py) in [(a.x0, a.y0), (a.x1, a.y1), (b.x0, b.y0), (b.x1, b.y1)] {
        let t = (px - cx) * dx + (py - cy) * dy;
        tmin = tmin.min(t);
        tmax = tmax.max(t);
    }
    Some(Segment {
        x0: cx + tmin * dx,
        y0: cy + tmin * dy,
        x1: cx + tmax * dx,
        y1: cy + tmax * dy,
        strength,
    })
}

/// Convenience: downscale an interleaved RGB f32 buffer (linear light) to luminance with the long edge ≤ `max_edge`, applying a simple gamma (e.g. powf(1/2.2)) so edges in shadows count; returns (lum, w, h, scale) where scale = analysed px per source px.
///
/// A single uniform `scale` is used for both axes (analysed = source × scale);
/// analysed pixel `i` is the area average of source span `[i/scale, (i+1)/scale)`.
pub fn luminance_for_analysis(
    rgb: &[f32],
    w: usize,
    h: usize,
    max_edge: usize,
) -> (Vec<f32>, usize, usize, f32) {
    let valid = w > 0
        && h > 0
        && w.checked_mul(h)
            .and_then(|n| n.checked_mul(3))
            .is_some_and(|n3| n3 <= rgb.len());
    if !valid {
        return (Vec::new(), 0, 0, 1.0);
    }
    let long = w.max(h);
    let max_edge = max_edge.max(1);
    let s = if long > max_edge {
        max_edge as f64 / long as f64
    } else {
        1.0
    };
    let ow = ((w as f64 * s).round() as usize).max(1);
    let oh = ((h as f64 * s).round() as usize).max(1);

    let lum_at = |i: usize| -> f32 {
        let (r, g, b) = (rgb[3 * i], rgb[3 * i + 1], rgb[3 * i + 2]);
        let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
        if y.is_finite() { y.max(0.0) } else { 0.0 }
    };

    let xw = axis_weights(w, ow, s);
    let yw = axis_weights(h, oh, s);

    // Horizontal pass: h rows × ow.
    let mut tmp = vec![0.0f32; ow * h];
    for y in 0..h {
        let row = y * w;
        for (ox, (x0, ws)) in xw.iter().enumerate() {
            let mut acc = 0.0f32;
            for (k, &wt) in ws.iter().enumerate() {
                acc += wt * lum_at(row + x0 + k);
            }
            tmp[y * ow + ox] = acc;
        }
    }
    // Vertical pass.
    let mut out = vec![0.0f32; ow * oh];
    for (oy, (y0, ws)) in yw.iter().enumerate() {
        let dst = &mut out[oy * ow..(oy + 1) * ow];
        for (k, &wt) in ws.iter().enumerate() {
            let src = &tmp[(y0 + k) * ow..(y0 + k + 1) * ow];
            for (d, &v) in dst.iter_mut().zip(src) {
                *d += wt * v;
            }
        }
    }
    let inv_gamma = 1.0 / 2.2;
    for v in &mut out {
        *v = if v.is_finite() {
            v.max(0.0).powf(inv_gamma)
        } else {
            0.0
        };
    }
    (out, ow, oh, s as f32)
}

/// Box-filter weights: for each output index, (first source index, weights).
fn axis_weights(n_src: usize, n_out: usize, s: f64) -> Vec<(usize, Vec<f32>)> {
    (0..n_out)
        .map(|i| {
            let a = i as f64 / s;
            let b = (i + 1) as f64 / s;
            let j0 = (a.floor() as usize).min(n_src - 1);
            let j1 = ((b.ceil() as usize).max(j0 + 1)).min(n_src);
            let mut ws: Vec<f32> = (j0..j1)
                .map(|j| ((b.min(j as f64 + 1.0) - a.max(j as f64)).max(0.0)) as f32)
                .collect();
            let sum: f32 = ws.iter().sum();
            if sum > 0.0 {
                for v in &mut ws {
                    *v /= sum;
                }
                (j0, ws)
            } else {
                (n_src - 1, vec![1.0])
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- synthetic renderer ------------------------------------------------

    type Poly = Vec<(f32, f32)>;

    /// Anti-aliased convex polygon coverage at a pixel centre.
    fn poly_cov(px: f32, py: f32, poly: &[(f32, f32)]) -> f32 {
        let n = poly.len();
        let mut area2 = 0.0;
        for i in 0..n {
            let (x0, y0) = poly[i];
            let (x1, y1) = poly[(i + 1) % n];
            area2 += x0 * y1 - x1 * y0;
        }
        let sgn = if area2 >= 0.0 { 1.0 } else { -1.0 };
        let mut md = f32::INFINITY;
        for i in 0..n {
            let (x0, y0) = poly[i];
            let (x1, y1) = poly[(i + 1) % n];
            let (ex, ey) = (x1 - x0, y1 - y0);
            let l = ex.hypot(ey);
            if l < 1e-9 {
                continue;
            }
            let d = sgn * (ex * (py - y0) - ey * (px - x0)) / l;
            md = md.min(d);
        }
        (md + 0.5).clamp(0.0, 1.0)
    }

    fn render(w: usize, h: usize, bg: f32, shapes: &[(Poly, f32)]) -> Vec<f32> {
        let mut img = vec![bg; w * h];
        paint(&mut img, w, h, shapes);
        img
    }

    fn paint(img: &mut [f32], w: usize, h: usize, shapes: &[(Poly, f32)]) {
        for (poly, val) in shapes {
            let minx = poly.iter().map(|p| p.0).fold(f32::INFINITY, f32::min) - 1.0;
            let maxx = poly.iter().map(|p| p.0).fold(f32::NEG_INFINITY, f32::max) + 1.0;
            let miny = poly.iter().map(|p| p.1).fold(f32::INFINITY, f32::min) - 1.0;
            let maxy = poly.iter().map(|p| p.1).fold(f32::NEG_INFINITY, f32::max) + 1.0;
            let x0 = minx.max(0.0) as usize;
            let x1 = (maxx.max(0.0) as usize).min(w);
            let y0 = miny.max(0.0) as usize;
            let y1 = (maxy.max(0.0) as usize).min(h);
            for y in y0..y1 {
                for x in x0..x1 {
                    let a = poly_cov(x as f32 + 0.5, y as f32 + 0.5, poly);
                    if a > 0.0 {
                        let p = &mut img[y * w + x];
                        *p = *p * (1.0 - a) + val * a;
                    }
                }
            }
        }
    }

    fn add_noise(img: &mut [f32], sigma: f32, seed: u64) {
        let mut s = seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let mut uni = || {
            s = s
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((s >> 11) as f64 + 0.5) / (1u64 << 53) as f64
        };
        for p in img.iter_mut() {
            let (u1, u2) = (uni(), uni());
            let g = (-2.0 * u1.ln()).sqrt() * (2.0 * std::f64::consts::PI * u2).cos();
            *p += sigma * g as f32;
        }
    }

    fn rect(x0: f32, y0: f32, x1: f32, y1: f32) -> Poly {
        vec![(x0, y0), (x1, y0), (x1, y1), (x0, y1)]
    }

    /// Point on a line through (px, py) with direction angle `deg`, at parameter t.
    fn along(px: f32, py: f32, deg: f32, t: f32) -> (f32, f32) {
        let r = deg.to_radians();
        (px + t * r.cos(), py + t * r.sin())
    }

    /// Half-plane (as a big polygon) on the left side of a directed line.
    fn half_plane(px: f32, py: f32, deg: f32) -> Poly {
        let big = 4000.0;
        let r = deg.to_radians();
        let (nx, ny) = (r.sin(), -r.cos());
        let a = along(px, py, deg, -big);
        let b = along(px, py, deg, big);
        vec![
            a,
            b,
            (b.0 + nx * big, b.1 + ny * big),
            (a.0 + nx * big, a.1 + ny * big),
        ]
    }

    fn angle_diff(a: f32, b: f32) -> f32 {
        let d = (a - b).rem_euclid(180.0);
        d.min(180.0 - d)
    }

    /// Building from below: sides lean ±8°, horizon tilted 2°.
    fn building_scene(w: usize, h: usize) -> (Vec<f32>, [f32; 3]) {
        let t8 = 8.0f32.to_radians().tan();
        let (bl, br) = (w as f32 * 0.2, w as f32 * 0.8);
        let hb = h as f32;
        // Sides pass through (bl, hb) and (br, hb), converging upward.
        let top = -60.0;
        let bot = hb + 200.0;
        let xl = |y: f32| bl + (hb - y) * t8;
        let xr = |y: f32| br - (hb - y) * t8;
        let building = vec![
            (xl(top), top),
            (xr(top), top),
            (xr(bot), bot),
            (xl(bot), bot),
        ];
        let t2 = 2.0f32.to_radians().tan();
        let hy = |x: f32| h as f32 * 0.68 + (x - w as f32 * 0.5) * t2;
        let ground = vec![
            (-100.0, hy(-100.0)),
            (w as f32 + 100.0, hy(w as f32 + 100.0)),
            (w as f32 + 100.0, hb + 300.0),
            (-100.0, hb + 300.0),
        ];
        let img = render(w, h, 0.3, &[(building, 0.75), (ground, 0.12)]);
        // Left side direction (t8, -1) → -82°, right side → 82°, horizon 2°.
        (img, [-82.0, 82.0, 2.0])
    }

    fn check_three(segs: &[Segment], truth: [f32; 3], tol: f32) {
        assert!(segs.len() >= 3, "found {segs:?}");
        let top: Vec<f32> = segs[..3].iter().map(|s| s.angle_deg()).collect();
        for t in truth {
            let best = top
                .iter()
                .map(|&a| angle_diff(a, t))
                .fold(f32::INFINITY, f32::min);
            assert!(
                best <= tol,
                "truth {t}: top angles {top:?}, err {best}\n{segs:?}"
            );
        }
        // Each of the three must be long.
        for s in &segs[..3] {
            assert!(s.length() > 100.0, "short top segment {s:?}");
        }
    }

    // ---- tests -------------------------------------------------------------

    #[test]
    fn segment_angle_convention() {
        let s = |x0, y0, x1, y1| Segment {
            x0,
            y0,
            x1,
            y1,
            strength: 1.0,
        };
        assert_eq!(s(0.0, 0.0, 0.0, 10.0).angle_deg(), 90.0);
        assert_eq!(s(0.0, 10.0, 0.0, 0.0).angle_deg(), 90.0);
        assert_eq!(s(10.0, 0.0, 0.0, 0.0).angle_deg(), 0.0);
        assert!((s(0.0, 0.0, 10.0, 10.0).angle_deg() - 45.0).abs() < 1e-4);
        assert!((s(0.0, 10.0, 10.0, 0.0).angle_deg() + 45.0).abs() < 1e-4);
        assert!(s(0.0, 0.0, 1.0, 20.0).is_near_vertical(5.0));
        assert!(!s(0.0, 0.0, 1.0, 20.0).is_near_horizontal(5.0));
        assert!(s(0.0, 0.0, 20.0, -1.0).is_near_horizontal(5.0));
        assert!((s(0.0, 0.0, 3.0, 4.0).length() - 5.0).abs() < 1e-5);
    }

    #[test]
    fn single_vertical_edge() {
        let (w, h) = (300, 240);
        let img = render(w, h, 0.7, &[(rect(-10.0, -10.0, 150.3, 260.0), 0.2)]);
        let segs = detect_segments(&img, w, h);
        assert_eq!(segs.len(), 1, "{segs:?}");
        let s = segs[0];
        assert!(s.is_near_vertical(0.25), "{s:?} angle {}", s.angle_deg());
        assert!(
            (s.x0 - 150.3).abs() < 0.3 && (s.x1 - 150.3).abs() < 0.3,
            "{s:?}"
        );
        assert!(s.length() >= 0.95 * h as f32, "{s:?}");
        assert!(s.y0 <= s.y1);
    }

    #[test]
    fn clean_edges_angle_accuracy() {
        let (w, h) = (360, 300);
        for &lean in &[0.7f32, 3.7, -11.3, 17.0, -31.0] {
            // Near-vertical edge through centre, direction angle 90 + lean.
            let dir = 90.0 + lean;
            let img = render(w, h, 0.25, &[(half_plane(180.0, 150.0, dir), 0.8)]);
            let segs = detect_segments(&img, w, h);
            assert!(!segs.is_empty(), "lean {lean}");
            let err = angle_diff(segs[0].angle_deg(), dir);
            assert!(err <= 0.25, "lean {lean}: err {err} {segs:?}");
            assert!(segs[0].length() >= 150.0);
        }
        for &tilt in &[1.3f32, -6.1, 24.0] {
            let img = render(w, h, 0.6, &[(half_plane(180.0, 150.0, tilt), 0.1)]);
            let segs = detect_segments(&img, w, h);
            assert!(!segs.is_empty(), "tilt {tilt}");
            let err = angle_diff(segs[0].angle_deg(), tilt);
            assert!(err <= 0.25, "tilt {tilt}: err {err} {segs:?}");
        }
    }

    #[test]
    fn converging_verticals_and_horizon() {
        let (w, h) = (512, 384);
        let (img, truth) = building_scene(w, h);
        let segs = detect_segments(&img, w, h);
        check_three(&segs, truth, 0.3);
        if segs.len() > 3 {
            let weakest = segs[..3]
                .iter()
                .map(|s| s.strength)
                .fold(f32::INFINITY, f32::min);
            assert!(segs[3].strength < 0.5 * weakest, "{segs:?}");
        }
    }

    #[test]
    fn converging_verticals_with_noise() {
        let (w, h) = (512, 384);
        let (mut img, truth) = building_scene(w, h);
        add_noise(&mut img, 0.05, 12345);
        let segs = detect_segments(&img, w, h);
        check_three(&segs, truth, 0.5);
    }

    #[test]
    fn short_edges_end_at_corners() {
        let (w, h) = (480, 360);
        let (x0, y0, x1, y1) = (160.0f32, 120.0f32, 320.0f32, 240.0f32);
        let img = render(w, h, 0.2, &[(rect(x0, y0, x1, y1), 0.8)]);
        let segs = detect_segments(&img, w, h);
        assert_eq!(segs.len(), 4, "{segs:?}");
        let sides = [
            ((x0, y0), (x1, y0)),
            ((x0, y1), (x1, y1)),
            ((x0, y0), (x0, y1)),
            ((x1, y0), (x1, y1)),
        ];
        let d = |a: (f32, f32), b: (f32, f32)| (a.0 - b.0).hypot(a.1 - b.1);
        for (a, b) in sides {
            let best = segs
                .iter()
                .map(|s| {
                    let (p, q) = ((s.x0, s.y0), (s.x1, s.y1));
                    (d(p, a).max(d(q, b))).min(d(p, b).max(d(q, a)))
                })
                .fold(f32::INFINITY, f32::min);
            assert!(
                best <= 3.0,
                "side {a:?}-{b:?}: endpoint err {best}\n{segs:?}"
            );
        }
    }

    #[test]
    fn thin_line_double_edge_merged() {
        let (w, h) = (320, 240);
        let img = render(w, h, 0.7, &[(rect(158.0, -10.0, 160.0, 250.0), 0.1)]);
        let segs = detect_segments(&img, w, h);
        assert_eq!(segs.len(), 1, "{segs:?}");
        assert!(segs[0].is_near_vertical(0.3));
    }

    #[test]
    fn degenerate_inputs() {
        assert!(detect_segments(&[], 0, 0).is_empty());
        assert!(detect_segments(&[0.5; 49], 7, 7).is_empty());
        assert!(detect_segments(&[0.5; 10], 100, 100).is_empty());
        assert!(detect_segments(&vec![0.42; 200 * 150], 200, 150).is_empty());
        let mut img = render(200, 150, 0.2, &[(rect(60.0, 30.0, 140.0, 120.0), 0.9)]);
        for (i, p) in img.iter_mut().enumerate() {
            if i % 7 == 0 {
                *p = f32::NAN;
            } else if i % 101 == 0 {
                *p = f32::INFINITY;
            }
        }
        let _ = detect_segments(&img, 200, 150);
        let _ = detect_segments(&vec![f32::NAN; 64 * 64], 64, 64);
        let (l, w, h, _) = luminance_for_analysis(&[], 0, 0, 1024);
        assert!(l.is_empty() && w == 0 && h == 0);
        let (l, w, h, _) = luminance_for_analysis(&[f32::NAN; 30], 10, 1, 4);
        assert_eq!(l.len(), w * h);
        assert!(l.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn luminance_dims_and_scale() {
        let (w, h) = (2048usize, 1365usize);
        let rgb = vec![0.18f32; w * h * 3];
        let (lum, ow, oh, scale) = luminance_for_analysis(&rgb, w, h, 1024);
        assert_eq!((ow, oh), (1024, 683));
        assert_eq!(lum.len(), ow * oh);
        assert!((scale - 0.5).abs() < 1e-6);
        let expect = 0.18f32.powf(1.0 / 2.2);
        assert!(lum.iter().all(|v| (v - expect).abs() < 1e-4));
        // No upscaling.
        let (_, ow, oh, scale) = luminance_for_analysis(&vec![0.5; 300 * 200 * 3], 300, 200, 1024);
        assert_eq!((ow, oh, scale), (300, 200, 1.0));
    }

    fn timing_scene(w: usize, h: usize) -> Vec<f32> {
        let (base, _) = building_scene(w, h);
        let mut shapes: Vec<(Poly, f32)> = Vec::new();
        for r in 0..8 {
            for c in 0..10 {
                let x = 230.0 + c as f32 * 58.0;
                let y = 40.0 + r as f32 * 52.0;
                shapes.push((
                    rect(x, y, x + 30.0, y + 34.0),
                    0.45 + 0.03 * (r + c) as f32 % 0.3,
                ));
            }
        }
        let mut img = base;
        paint(&mut img, w, h, &shapes);
        add_noise(&mut img, 0.03, 99);
        img
    }

    #[test]
    #[ignore]
    fn timing_1024x683() {
        let (w, h) = (1024, 683);
        let img = timing_scene(w, h);
        let _ = detect_segments(&img, w, h);
        let runs = 5;
        let t = std::time::Instant::now();
        let mut n = 0;
        for _ in 0..runs {
            n = detect_segments(&img, w, h).len();
        }
        let ms = t.elapsed().as_secs_f64() * 1000.0 / runs as f64;
        println!("detect_segments 1024x683: {ms:.1} ms/run, {n} segments");
    }
}
