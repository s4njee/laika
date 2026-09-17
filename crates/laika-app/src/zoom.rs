//! U07: shared image-viewport zoom model for Loupe and Develop.
//!
//! Pure geometry over logical pixels: no GPUI types, unit-tested here.
//! The view layer measures its viewport, reads the window scale factor,
//! and positions one exact-aspect image with [`stage_geom`].

/// Zoom level, sticky across photos until the user changes it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum ZoomLevel {
    /// Whole image visible, never upscaled past native.
    #[default]
    Fit,
    /// One image pixel per device pixel.
    Full,
    /// Two device pixels per image pixel (focus checks).
    Double,
}

impl ZoomLevel {
    pub fn label(self) -> &'static str {
        match self {
            ZoomLevel::Fit => "Fit",
            ZoomLevel::Full => "100%",
            ZoomLevel::Double => "200%",
        }
    }

    /// Keyboard/mouse cycle: Fit → 100% → 200% → Fit.
    pub fn cycle(self) -> Self {
        match self {
            ZoomLevel::Fit => ZoomLevel::Full,
            ZoomLevel::Full => ZoomLevel::Double,
            ZoomLevel::Double => ZoomLevel::Fit,
        }
    }

    /// Wheel/pinch step in one direction, clamped at the ends.
    pub fn step(self, dir: i32) -> Self {
        match (self, dir.signum()) {
            (ZoomLevel::Fit, -1) => ZoomLevel::Fit,
            (ZoomLevel::Double, 1) => ZoomLevel::Double,
            (_, 0) => self,
            (ZoomLevel::Fit, _) => ZoomLevel::Full,
            (ZoomLevel::Full, 1) => ZoomLevel::Double,
            (ZoomLevel::Full, _) => ZoomLevel::Fit,
            (ZoomLevel::Double, _) => ZoomLevel::Full,
        }
    }
}

/// Placed image: full-image display size plus its top-left offset in
/// viewport coordinates. The view renders one exact-aspect `img` at
/// `(off_x, off_y, disp_w, disp_h)` inside an `overflow_hidden` stage —
/// before/after stay aligned because the transform is uniform.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StageGeom {
    pub disp_w: f32,
    pub disp_h: f32,
    pub off_x: f32,
    pub off_y: f32,
}

/// Contain `nw × nh` native pixels in a `vw × vh` viewport, never
/// upscaling past native size (logical px = native / scale at 100%).
pub fn fit_size(vw: f32, vh: f32, nw: f32, nh: f32, scale: f32) -> (f32, f32) {
    if vw <= 0. || vh <= 0. || nw <= 0. || nh <= 0. {
        return (0., 0.);
    }
    let native_w = nw / scale.max(0.01);
    let native_h = nh / scale.max(0.01);
    let s = (vw / native_w).min(vh / native_h).min(1.);
    (native_w * s, native_h * s)
}

/// Full display size of the image at `level` (logical px).
pub fn zoom_size(level: ZoomLevel, vw: f32, vh: f32, nw: f32, nh: f32, scale: f32) -> (f32, f32) {
    let scale = scale.max(0.01);
    match level {
        ZoomLevel::Fit => fit_size(vw, vh, nw, nh, scale),
        ZoomLevel::Full => (nw / scale, nh / scale),
        ZoomLevel::Double => (2. * nw / scale, 2. * nh / scale),
    }
}

/// Top-left offset placing `center` (image-normalized) in the viewport.
/// A smaller-than-viewport image centers; a larger one clamps so the
/// viewport never shows past the image edge.
pub fn zoom_offset(center: (f32, f32), disp_w: f32, disp_h: f32, vw: f32, vh: f32) -> (f32, f32) {
    let axis = |c: f32, d: f32, v: f32| {
        if d <= v {
            (v - d) / 2.
        } else {
            (-(c * d - v / 2.)).clamp(v - d, 0.)
        }
    };
    (axis(center.0, disp_w, vw), axis(center.1, disp_h, vh))
}

/// Combined helper: size + offset for the stage.
pub fn stage_geom(
    level: ZoomLevel,
    center: (f32, f32),
    vw: f32,
    vh: f32,
    nw: f32,
    nh: f32,
    scale: f32,
) -> StageGeom {
    let (disp_w, disp_h) = zoom_size(level, vw, vh, nw, nh, scale);
    let (off_x, off_y) = zoom_offset(center, disp_w, disp_h, vw, vh);
    StageGeom {
        disp_w,
        disp_h,
        off_x,
        off_y,
    }
}

/// Window point → image-normalized point under the current geometry.
pub fn to_image(px: f32, py: f32, g: &StageGeom) -> (f32, f32) {
    if g.disp_w <= 0. || g.disp_h <= 0. {
        return (0.5, 0.5);
    }
    (
        ((px - g.off_x) / g.disp_w).clamp(0., 1.),
        ((py - g.off_y) / g.disp_h).clamp(0., 1.),
    )
}

/// Clamp a center so the viewport stays covered (or centered when the
/// image is smaller than the viewport).
pub fn clamp_center(c: (f32, f32), disp_w: f32, disp_h: f32, vw: f32, vh: f32) -> (f32, f32) {
    let axis = |c: f32, d: f32, v: f32| {
        if d <= v {
            0.5
        } else {
            c.clamp((v / 2.) / d, 1. - (v / 2.) / d)
        }
    };
    (axis(c.0, disp_w, vw), axis(c.1, disp_h, vh))
}

/// New center keeping `cursor` (image-normalized) stable across a level
/// change — zoom centers around the inspected point.
pub fn zoom_at_point(
    cursor: (f32, f32),
    center: (f32, f32),
    old_size: (f32, f32),
    new_size: (f32, f32),
    vw: f32,
    vh: f32,
) -> (f32, f32) {
    if new_size.0 <= 0. || new_size.1 <= 0. {
        return center;
    }
    let kx = if old_size.0 > 0. {
        old_size.0 / new_size.0
    } else {
        1.
    };
    let ky = if old_size.1 > 0. {
        old_size.1 / new_size.1
    } else {
        1.
    };
    clamp_center(
        (
            cursor.0 + (center.0 - cursor.0) * kx,
            cursor.1 + (center.1 - cursor.1) * ky,
        ),
        new_size.0,
        new_size.1,
        vw,
        vh,
    )
}

/// Pan by a window-space drag delta (px).
pub fn pan_by(
    center: (f32, f32),
    dx: f32,
    dy: f32,
    disp_w: f32,
    disp_h: f32,
    vw: f32,
    vh: f32,
) -> (f32, f32) {
    if disp_w <= 0. || disp_h <= 0. {
        return center;
    }
    clamp_center(
        (center.0 - dx / disp_w, center.1 - dy / disp_h),
        disp_w,
        disp_h,
        vw,
        vh,
    )
}

/// U08: crop interaction — moving the box or one of 8 resize handles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CropHandle {
    Move,
    /// Drag outside the box: straighten about the frame center.
    Rotate,
    NW,
    N,
    NE,
    E,
    SE,
    S,
    SW,
    W,
}

/// Screen handle → rect-space handle when the frame view is mirrored
/// (flips apply in output space, the rect lives in unflipped space).
pub fn mirror_handle(h: CropHandle, flip_h: bool, flip_v: bool) -> CropHandle {
    use CropHandle::*;
    let h = if flip_h {
        match h {
            NW => NE,
            NE => NW,
            W => E,
            E => W,
            SW => SE,
            SE => SW,
            o => o,
        }
    } else {
        h
    };
    if flip_v {
        match h {
            NW => SW,
            SW => NW,
            N => S,
            S => N,
            NE => SE,
            SE => NE,
            o => o,
        }
    } else {
        h
    }
}

/// U08: handle/move hit test in stage pixels. Handles win within 7 px of
/// a grip; the box interior moves; outside is None (clicks pass through).
pub fn crop_handle_at(px: f32, py: f32, bx: f32, by: f32, bw: f32, bh: f32) -> Option<CropHandle> {
    const R: f32 = 7.;
    let near = |x: f32, y: f32| (px - x).abs() <= R && (py - y).abs() <= R;
    if near(bx, by) {
        return Some(CropHandle::NW);
    }
    if near(bx + bw, by) {
        return Some(CropHandle::NE);
    }
    if near(bx, by + bh) {
        return Some(CropHandle::SW);
    }
    if near(bx + bw, by + bh) {
        return Some(CropHandle::SE);
    }
    if near(bx + bw / 2., by) {
        return Some(CropHandle::N);
    }
    if near(bx + bw / 2., by + bh) {
        return Some(CropHandle::S);
    }
    if near(bx, by + bh / 2.) {
        return Some(CropHandle::W);
    }
    if near(bx + bw, by + bh / 2.) {
        return Some(CropHandle::E);
    }
    if px >= bx && px <= bx + bw && py >= by && py <= by + bh {
        return Some(CropHandle::Move);
    }
    None
}

/// U08: drag a normalized crop rect by a normalized delta. Corners and
/// edges honor the aspect lock (None = free); the moving edge follows
/// the pointer while the opposite anchor stays fixed. Result is
/// sanitized (min size, inside the frame) — angle constraining happens
/// in the app via `constrain_crop`.
pub fn drag_crop_rect(
    mode: CropHandle,
    rect: [f32; 4],
    dx: f32,
    dy: f32,
    aspect: Option<f32>,
) -> [f32; 4] {
    const MIN: f32 = 0.02;
    let (x, y, w, h) = (rect[0], rect[1], rect[2].max(MIN), rect[3].max(MIN));
    let (mut l, mut t, mut r, mut b) = (x, y, x + w, y + h);
    match mode {
        CropHandle::Rotate => return rect,
        CropHandle::Move => {
            // `clamp` panics when min > max: a stored width a hair over
            // 1.0 (float round-trip) must not crash the drag.
            let nx = (x + dx).clamp(0., (1. - w).max(0.));
            let ny = (y + dy).clamp(0., (1. - h).max(0.));
            return [nx, ny, w, h];
        }
        CropHandle::NW => {
            l += dx;
            t += dy;
        }
        CropHandle::N => t += dy,
        CropHandle::NE => {
            r += dx;
            t += dy;
        }
        CropHandle::E => r += dx,
        CropHandle::SE => {
            r += dx;
            b += dy;
        }
        CropHandle::S => b += dy,
        CropHandle::SW => {
            l += dx;
            b += dy;
        }
        CropHandle::W => l += dx,
    }
    // Order + minimums before the aspect pass.
    if r - l < MIN {
        if matches!(mode, CropHandle::NW | CropHandle::W | CropHandle::SW) {
            l = r - MIN;
        } else {
            r = l + MIN;
        }
    }
    if b - t < MIN {
        if matches!(mode, CropHandle::NW | CropHandle::N | CropHandle::NE) {
            t = b - MIN;
        } else {
            b = t + MIN;
        }
    }
    if let Some(a) = aspect.filter(|a| *a > 0.) {
        // Pointer-led axis wins ties by delta magnitude (like Lightroom):
        // dragging mostly sideways drives the width.
        let wide = match mode {
            CropHandle::N | CropHandle::S => false,
            CropHandle::E | CropHandle::W => true,
            _ => dx.abs() >= dy.abs(),
        };
        // Opposite (fixed) anchor per mode.
        let anchor_l = matches!(mode, CropHandle::NE | CropHandle::E | CropHandle::SE);
        let anchor_r = matches!(mode, CropHandle::NW | CropHandle::W | CropHandle::SW);
        let anchor_t = matches!(mode, CropHandle::SW | CropHandle::S | CropHandle::SE);
        let anchor_b = matches!(mode, CropHandle::NW | CropHandle::N | CropHandle::NE);
        if wide {
            let nw = (r - l).clamp(MIN, 1.);
            let nh = nw / a;
            // Horizontal edges land where the pointer put them; the
            // fixed side stays.
            if !anchor_l && !anchor_r {
                // Edge handle: keep the dragged edge, derive around it.
                if matches!(mode, CropHandle::W) {
                    r = (l + nw).min(1.);
                    l = r - nw;
                } else {
                    l = (r - nw).max(0.);
                    r = l + nw;
                }
            } else if anchor_l {
                r = l + nw;
            } else {
                l = r - nw;
            }
            if anchor_t {
                b = t + nh;
            } else if anchor_b {
                t = b - nh;
            } else {
                // Edge handle: center on the box, contain pass shifts in.
                let cy = (t + b) / 2.;
                t = cy - nh / 2.;
                b = cy + nh / 2.;
            }
        } else {
            let nh = (b - t).clamp(MIN, 1.);
            let nw = nh * a;
            if !anchor_t && !anchor_b {
                if matches!(mode, CropHandle::N) {
                    b = (t + nh).min(1.);
                    t = b - nh;
                } else {
                    t = (b - nh).max(0.);
                    b = t + nh;
                }
            } else if anchor_t {
                b = t + nh;
            } else {
                t = b - nh;
            }
            if anchor_l {
                r = l + nw;
            } else if anchor_r {
                l = r - nw;
            } else {
                let cx = (l + r) / 2.;
                l = cx - nw / 2.;
                r = cx + nw / 2.;
            }
        }
    }
    // Contain: shift inside, then shrink what still overflows.
    if l < 0. {
        r -= l;
        l = 0.;
    }
    if t < 0. {
        b -= t;
        t = 0.;
    }
    if r > 1. {
        l -= r - 1.;
        r = 1.;
    }
    if b > 1. {
        t -= b - 1.;
        b = 1.;
    }
    let mut out = [l.max(0.), t.max(0.), (r - l).max(MIN), (b - t).max(MIN)];
    if out[0] + out[2] > 1. {
        out[2] = 1. - out[0];
    }
    if out[1] + out[3] > 1. {
        out[3] = 1. - out[1];
    }
    out
}

/// V17: thumbnail wall — justified rows, zero gaps. Pure geometry over
/// logical pixels (unit-tested here); the view measures its container
/// and lays out rows, reflowing on resize without reloads.
pub const WALL_MAX_ITEMS: usize = 2000;

#[derive(Clone, Debug, PartialEq)]
pub struct WallCell {
    pub id: i64,
    pub w: f32,
    pub h: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WallRow {
    pub cells: Vec<WallCell>,
    pub height: f32,
}

/// Lay out `(id, aspect)` items into justified rows for `container_w`.
/// Every non-final row fills the width exactly at one shared height.
/// Rows taller than 2.5× target shed their last item (rare panoramas);
/// the final row stays left-aligned at target height. Returns rows plus
/// the truncated count (cap keeps 10k catalogs paintable).
pub fn wall_layout(items: &[(i64, f32)], container_w: f32, target_h: f32) -> (Vec<WallRow>, usize) {
    let mut rows = Vec::new();
    if container_w <= 1. || target_h <= 1. {
        return (rows, 0);
    }
    let n = items.len().min(WALL_MAX_ITEMS);
    let truncated = items.len() - n;
    let mut cur: Vec<(i64, f32)> = Vec::new();
    let mut flush = |row: &mut Vec<(i64, f32)>, rows: &mut Vec<WallRow>, last: bool| {
        if row.is_empty() {
            return;
        }
        let sum: f32 = row.iter().map(|(_, a)| a.max(0.05)).sum();
        let h = if last {
            target_h
        } else {
            container_w / sum.max(0.01)
        };
        rows.push(WallRow {
            cells: row
                .iter()
                .map(|(id, a)| WallCell {
                    id: *id,
                    w: h * a.max(0.05),
                    h,
                })
                .collect(),
            height: h,
        });
        row.clear();
    };
    for (id, aspect) in items.iter().take(n) {
        cur.push((*id, *aspect));
        let sum: f32 = cur.iter().map(|(_, a)| a.max(0.05)).sum();
        // Flush implies height = container/sum ≤ target (sums only
        // trigger past container/target), so rows never tower — short
        // rows simply stay open and gather more cells.
        if sum * target_h >= container_w {
            let mut row = std::mem::take(&mut cur);
            flush(&mut row, &mut rows, false);
        }
    }
    flush(&mut cur, &mut rows, true);
    (rows, truncated)
}

/// Visual x-center of every cell (row, center px) for arrow navigation.
pub fn wall_positions(rows: &[WallRow]) -> Vec<(i64, usize, f32)> {
    let mut out = Vec::new();
    for (ri, row) in rows.iter().enumerate() {
        let mut x = 0.;
        for c in &row.cells {
            out.push((c.id, ri, x + c.w / 2.));
            x += c.w;
        }
    }
    out
}

/// V17: wall arrow step — left/right walk the visual order, up/down
/// keep the x position across rows. Returns the landed id.
pub fn wall_move(rows: &[WallRow], from: i64, dx: i32, dy: i32) -> Option<i64> {
    let pos = wall_positions(rows);
    let i = pos.iter().position(|(id, _, _)| *id == from)?;
    if dx != 0 {
        let j = (i as i32 + dx).clamp(0, pos.len() as i32 - 1) as usize;
        return Some(pos[j].0);
    }
    if dy != 0 {
        let (_, row, x) = pos[i];
        let target = (row as i32 + dy).clamp(0, rows.len() as i32 - 1) as usize;
        if target == row {
            return Some(from);
        }
        // Nearest center in the target row (ties go left, stable).
        let mut best = &rows[target].cells[0];
        let mut bx = best.w / 2.;
        let mut acc = 0.;
        for c in &rows[target].cells {
            let cx = acc + c.w / 2.;
            if (cx - x).abs() < (bx - x).abs() {
                best = c;
                bx = cx;
            }
            acc += c.w;
        }
        return Some(best.id);
    }
    Some(from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_contains_without_upscale() {
        // Landscape native in a square viewport: width-bound.
        let (w, h) = fit_size(800., 800., 6000., 4000., 2.);
        assert!((w - 800.).abs() < 0.01);
        assert!((h - 800. * 4000. / 6000.).abs() < 0.5);
        // A tiny image is never upscaled past native.
        let (w, h) = fit_size(2000., 2000., 400., 300., 1.);
        assert!((w - 400.).abs() < 0.01 && (h - 300.).abs() < 0.01);
        // Degenerate inputs stay zero, never NaN.
        assert_eq!(fit_size(0., 800., 6000., 4000., 2.), (0., 0.));
    }

    #[test]
    fn zoom_sizes_respect_display_scale() {
        // 100% = native device pixels: logical size divides by scale.
        let (w, h) = zoom_size(ZoomLevel::Full, 800., 600., 6000., 4000., 2.);
        assert!((w - 3000.).abs() < 0.01 && (h - 2000.).abs() < 0.01);
        let (w, _) = zoom_size(ZoomLevel::Double, 800., 600., 6000., 4000., 2.);
        assert!((w - 6000.).abs() < 0.01);
    }

    #[test]
    fn offsets_center_small_and_clamp_large() {
        // Smaller than viewport: centered.
        assert_eq!(
            zoom_offset((0.2, 0.9), 400., 300., 800., 600.),
            (200., 150.)
        );
        // Larger: middle center puts the middle on screen…
        let (x, y) = zoom_offset((0.5, 0.5), 3000., 2000., 800., 600.);
        assert!((x - (800. - 3000.) / 2.).abs() < 0.01);
        assert!((y - (600. - 2000.) / 2.).abs() < 0.01);
        // …and corners clamp exactly to the edges.
        assert_eq!(zoom_offset((0., 0.), 3000., 2000., 800., 600.), (0., 0.));
        assert_eq!(
            zoom_offset((1., 1.), 3000., 2000., 800., 600.),
            (800. - 3000., 600. - 2000.)
        );
    }

    #[test]
    fn wheel_keeps_cursor_stable() {
        // Zooming 100% → 200% with the cursor on the middle keeps it there,
        // and a corner cursor pulls the center toward that corner.
        let mid = zoom_at_point(
            (0.5, 0.5),
            (0.5, 0.5),
            (3000., 2000.),
            (6000., 4000.),
            800.,
            600.,
        );
        assert!((mid.0 - 0.5).abs() < 1e-6 && (mid.1 - 0.5).abs() < 1e-6);
        let corner = zoom_at_point(
            (0.9, 0.9),
            (0.5, 0.5),
            (3000., 2000.),
            (6000., 4000.),
            800.,
            600.,
        );
        assert!(corner.0 > 0.5 && corner.1 > 0.5);
        // Round-trip through to_image lands back on the cursor.
        let g = stage_geom(ZoomLevel::Double, corner, 800., 600., 6000., 4000., 2.);
        let back = to_image(g.off_x + 0.9 * g.disp_w, g.off_y + 0.9 * g.disp_h, &g);
        assert!((back.0 - 0.9).abs() < 1e-4 && (back.1 - 0.9).abs() < 1e-4);
    }

    #[test]
    fn v17_wall_fills_rows_and_navigates_visually() {
        // Mixed aspects at 1440 wide: every non-final row is exact.
        let items: Vec<(i64, f32)> = (0..12)
            .map(|i| (i, if i % 3 == 0 { 0.67 } else { 1.5 }))
            .collect();
        let (rows, truncated) = wall_layout(&items, 1440., 180.);
        assert_eq!(truncated, 0);
        assert!(rows.len() >= 2);
        for r in &rows[..rows.len() - 1] {
            let w: f32 = r.cells.iter().map(|c| c.w).sum();
            assert!((w - 1440.).abs() < 1.0, "{w}");
            // Equal height within the row (mixed orientations align).
            for c in &r.cells {
                assert!((c.h - r.height).abs() < 0.01);
            }
        }
        // Last row keeps target height, left-aligned (underfull ok).
        let last = rows.last().unwrap();
        assert!((last.height - 180.).abs() < 0.01);
        // Cap truncates with a count.
        let big: Vec<(i64, f32)> = (0..2500).map(|i| (i, 1.5)).collect();
        let (rows, truncated) = wall_layout(&big, 1440., 180.);
        assert_eq!(truncated, 500);
        assert_eq!(rows.iter().map(|r| r.cells.len()).sum::<usize>(), 2000);
        // Navigation: right walks order, down keeps x.
        let (rows, _) = wall_layout(&[(1, 1.), (2, 1.), (3, 1.), (4, 1.)], 250., 100.);
        assert_eq!(rows.len(), 2);
        assert_eq!(wall_move(&rows, 1, 1, 0), Some(2));
        assert_eq!(wall_move(&rows, 2, 1, 0), Some(3));
        assert_eq!(wall_move(&rows, 1, 0, 1), Some(4));
        // (row two is left-aligned: x=50 lands nearest cell one)
        assert_eq!(wall_move(&rows, 4, 0, -1), Some(1));
        assert_eq!(wall_move(&rows, 1, -1, 0), Some(1));
        assert_eq!(wall_move(&rows, 99, 1, 0), None);
    }

    #[test]
    fn u08_crop_handles_and_drag() {
        let (bx, by, bw, bh) = (100., 100., 200., 100.);
        assert_eq!(
            crop_handle_at(100., 100., bx, by, bw, bh),
            Some(CropHandle::NW)
        );
        assert_eq!(
            crop_handle_at(300., 200., bx, by, bw, bh),
            Some(CropHandle::SE)
        );
        assert_eq!(
            crop_handle_at(200., 100., bx, by, bw, bh),
            Some(CropHandle::N)
        );
        assert_eq!(
            crop_handle_at(200., 150., bx, by, bw, bh),
            Some(CropHandle::Move)
        );
        assert_eq!(crop_handle_at(10., 10., bx, by, bw, bh), None);
        // Free corner drag moves its edges, anchor fixed.
        // Rotate drags never move the box; mirrored frames swap handles.
        assert_eq!(
            drag_crop_rect(CropHandle::Rotate, [0.1, 0.2, 0.3, 0.4], 0.5, 0.5, None),
            [0.1, 0.2, 0.3, 0.4]
        );
        assert_eq!(mirror_handle(CropHandle::NW, true, false), CropHandle::NE);
        assert_eq!(mirror_handle(CropHandle::NW, false, true), CropHandle::SW);
        assert_eq!(mirror_handle(CropHandle::NW, true, true), CropHandle::SE);
        assert_eq!(mirror_handle(CropHandle::N, true, false), CropHandle::N);
        assert_eq!(
            mirror_handle(CropHandle::Move, true, true),
            CropHandle::Move
        );
        let r = drag_crop_rect(CropHandle::SE, [0.1, 0.1, 0.5, 0.5], 0.1, 0.1, None);
        assert!((r[0] - 0.1).abs() < 1e-6 && (r[1] - 0.1).abs() < 1e-6);
        assert!((r[2] - 0.6).abs() < 1e-6 && (r[3] - 0.6).abs() < 1e-6);
        // Locked corner keeps the aspect around the fixed anchor.
        let r = drag_crop_rect(CropHandle::SE, [0.1, 0.1, 0.4, 0.4], 0.2, 0.0, Some(2.));
        assert!((r[0] - 0.1).abs() < 1e-5 && (r[1] - 0.1).abs() < 1e-5);
        assert!((r[2] / r[3] - 2.).abs() < 1e-4, "{r:?}");
        // Move clamps inside the frame.
        let r = drag_crop_rect(CropHandle::Move, [0.5, 0.5, 0.4, 0.4], 0.5, 0.5, None);
        assert_eq!(r, [0.6, 0.6, 0.4, 0.4]);
        // Collapse clamps at the minimum, never inverts.
        let r = drag_crop_rect(CropHandle::W, [0.2, 0.2, 0.3, 0.3], 0.9, 0., None);
        assert!(r[2] >= 0.02 && r[0] + r[2] <= 1.0 + 1e-6);
    }

    #[test]
    fn cycle_and_step_cover_all_levels() {
        assert_eq!(ZoomLevel::Fit.cycle(), ZoomLevel::Full);
        assert_eq!(ZoomLevel::Full.cycle(), ZoomLevel::Double);
        assert_eq!(ZoomLevel::Double.cycle(), ZoomLevel::Fit);
        assert_eq!(ZoomLevel::Fit.step(-1), ZoomLevel::Fit);
        assert_eq!(ZoomLevel::Double.step(1), ZoomLevel::Double);
        assert_eq!(ZoomLevel::Full.step(1), ZoomLevel::Double);
        assert_eq!(ZoomLevel::Full.step(-1), ZoomLevel::Fit);
    }
}
