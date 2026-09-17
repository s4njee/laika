//! V22: crop and geometry tools beyond U08 — the Transform panel
//! (Upright modes, manual perspective sliders, constrain to image),
//! Guided lines, the straighten line tool, composition overlays, numeric
//! crop entry, user aspect presets, and nudging.
//!
//! Geometry math lives in `laika_core::{upright, edit}`; this file wires
//! it to the Develop stage and rail.

use laika_core::upright::{self, UprightMode};

use super::*;

/// Composition overlays drawn inside the crop box.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Overlay {
    Thirds,
    Golden,
    Diagonals,
    Triangle,
    Spiral,
    Grid2x2,
    Custom,
}

impl Overlay {
    pub const ALL: [Overlay; 7] = [
        Overlay::Thirds,
        Overlay::Golden,
        Overlay::Diagonals,
        Overlay::Triangle,
        Overlay::Spiral,
        Overlay::Grid2x2,
        Overlay::Custom,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Overlay::Thirds => "Thirds",
            Overlay::Golden => "Golden ratio",
            Overlay::Diagonals => "Diagonals",
            Overlay::Triangle => "Triangle",
            Overlay::Spiral => "Golden spiral",
            Overlay::Grid2x2 => "2×2",
            Overlay::Custom => "Grid",
        }
    }

    fn bit(self) -> u8 {
        1 << (self as u8)
    }
}

/// Outside-the-box treatment while cropping.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Outside {
    Dim,
    Hide,
    Show,
}

impl Outside {
    pub fn next(self) -> Self {
        match self {
            Outside::Dim => Outside::Hide,
            Outside::Hide => Outside::Show,
            Outside::Show => Outside::Dim,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Outside::Dim => "Outside: dim",
            Outside::Hide => "Outside: hide",
            Outside::Show => "Outside: show",
        }
    }
}

/// Polylines (box-normalized 0..1 points) for an overlay inside a
/// `bw × bh` px box. Pure so it can be tested; diagonals and triangles
/// use true 45°/perpendicular lines in pixels, not in unit space.
pub(crate) fn overlay_lines(
    kind: Overlay,
    orient: u8,
    grid_n: u8,
    bw: f32,
    bh: f32,
) -> Vec<Vec<(f32, f32)>> {
    let (bw, bh) = (bw.max(1.), bh.max(1.));
    let mut out: Vec<Vec<(f32, f32)>> = Vec::new();
    let grid = |fs: &[f32], out: &mut Vec<Vec<(f32, f32)>>| {
        for &f in fs {
            out.push(vec![(f, 0.), (f, 1.)]);
            out.push(vec![(0., f), (1., f)]);
        }
    };
    match kind {
        Overlay::Thirds => grid(&[1. / 3., 2. / 3.], &mut out),
        Overlay::Golden => grid(&[0.381_966, 0.618_034], &mut out),
        Overlay::Grid2x2 => grid(&[0.5], &mut out),
        Overlay::Custom => {
            let n = grid_n.clamp(2, 20) as usize;
            let fs: Vec<f32> = (1..n).map(|k| k as f32 / n as f32).collect();
            grid(&fs, &mut out);
        }
        Overlay::Diagonals => {
            // 45° (in pixels) from each corner, clipped to the box.
            let s = bh.min(bw);
            let (dx, dy) = (s / bw, s / bh);
            out.push(vec![(0., 0.), (dx, dy)]);
            out.push(vec![(1., 0.), (1. - dx, dy)]);
            out.push(vec![(0., 1.), (dx, 1. - dy)]);
            out.push(vec![(1., 1.), (1. - dx, 1. - dy)]);
        }
        Overlay::Triangle => {
            // Main diagonal plus perpendiculars from the other corners.
            out.push(vec![(0., 0.), (1., 1.)]);
            let foot = |px: f32, py: f32| {
                // Project (px, py) px onto the diagonal (0,0)→(bw,bh).
                let t = (px * bw + py * bh) / (bw * bw + bh * bh);
                (t, t)
            };
            let (t1, _) = foot(bw, 0.);
            out.push(vec![(1., 0.), (t1, t1)]);
            let (t2, _) = foot(0., bh);
            out.push(vec![(0., 1.), (t2, t2)]);
        }
        Overlay::Spiral => {
            let phi = 0.618_034_f32;
            let (mut x, mut y, mut w, mut h) = (0f32, 0f32, 1f32, 1f32);
            let mut pts: Vec<(f32, f32)> = Vec::new();
            for i in 0..12 {
                let (cx, cy, rx, ry, a0) = match i % 4 {
                    0 => {
                        let s = w * phi;
                        let arc = (x + s, y + h, s, h, 180f32);
                        x += s;
                        w -= s;
                        arc
                    }
                    1 => {
                        let s = h * phi;
                        let arc = (x, y + s, w, s, 270.);
                        y += s;
                        h -= s;
                        arc
                    }
                    2 => {
                        let s = w * phi;
                        let arc = (x + w - s, y, s, h, 0.);
                        w -= s;
                        arc
                    }
                    _ => {
                        let s = h * phi;
                        let arc = (x + w, y + h - s, w, s, 90.);
                        h -= s;
                        arc
                    }
                };
                for k in 0..=12 {
                    let a = (a0 + 90. * k as f32 / 12.).to_radians();
                    pts.push((cx + rx * a.cos(), cy + ry * a.sin()));
                }
            }
            out.push(pts);
        }
    }
    // Orientation: bit 0 mirrors x, bit 1 mirrors y (Shift+O cycles).
    let (mx, my) = (orient & 1 == 1, orient & 2 == 2);
    if mx || my {
        for line in &mut out {
            for p in line.iter_mut() {
                if mx {
                    p.0 = 1. - p.0;
                }
                if my {
                    p.1 = 1. - p.1;
                }
            }
        }
    }
    out
}

/// A stage drag in window coordinates (start, current).
pub(crate) type Stroke = ((f32, f32), (f32, f32));

pub(crate) struct GeoUi {
    pub overlay: Overlay,
    /// Enabled overlays for `O` cycling (bitmask of `Overlay::bit`).
    pub overlay_set: u8,
    pub overlay_orient: u8,
    pub grid_n: u8,
    pub outside: Outside,
    pub overlay_menu: bool,
    /// Straighten line tool (also Cmd-drag while cropping).
    pub straighten_tool: bool,
    pub straighten: Option<Stroke>,
    /// Guided Upright: pointer draws guide lines on the stage.
    pub guide_draw: bool,
    pub guide: Option<Stroke>,
    /// Numeric crop fields in pixels (else percent).
    pub units_px: bool,
    /// User aspect presets (label, width/height), per catalog.
    pub aspects: Vec<(String, f32)>,
    /// Detected edges per photo: (segments, analysis w, analysis h).
    pub lines: HashMap<i64, (Vec<laika_core::lines::Segment>, usize, usize)>,
    pub analyzing: Option<(i64, UprightMode)>,
    pub note: String,
}

impl Default for GeoUi {
    fn default() -> Self {
        Self {
            overlay: Overlay::Thirds,
            overlay_set: Overlay::ALL.iter().fold(0, |m, o| m | o.bit()),
            overlay_orient: 0,
            grid_n: 6,
            outside: Outside::Dim,
            overlay_menu: false,
            straighten_tool: false,
            straighten: None,
            guide_draw: false,
            guide: None,
            units_px: true,
            aspects: Vec::new(),
            lines: HashMap::new(),
            analyzing: None,
            note: String::new(),
        }
    }
}

/// `"Name=1.25;5:4=1.25"` ↔ presets. Malformed entries are skipped.
pub(crate) fn parse_aspects(s: &str) -> Vec<(String, f32)> {
    s.split(';')
        .filter_map(|part| {
            let (name, v) = part.rsplit_once('=')?;
            let v = v.trim().parse::<f32>().ok()?;
            (v.is_finite() && (0.1..=10.).contains(&v) && !name.trim().is_empty())
                .then(|| (name.trim().to_string(), v))
        })
        .take(12)
        .collect()
}

pub(crate) fn serialize_aspects(list: &[(String, f32)]) -> String {
    list.iter()
        .map(|(n, v)| format!("{}={v:.5}", n.replace([';', '='], " ")))
        .collect::<Vec<_>>()
        .join(";")
}

/// A readable name for a pixel ratio: small integer pairs when exact
/// enough (`5:4`), else two decimals.
pub(crate) fn ratio_label(r: f32) -> String {
    for d in 1..=16u32 {
        let n = (r * d as f32).round();
        if n >= 1. && ((n / d as f32) - r).abs() < 0.0015 {
            return format!("{}:{}", n as u32, d);
        }
    }
    format!("{r:.2}")
}

/// Straighten from a drawn line: the angle change that brings a line at
/// `(dx, dy)` screen pixels to its nearest axis (flips mirror the view).
pub(crate) fn straighten_delta(dx: f32, dy: f32, flip_h: bool, flip_v: bool) -> Option<f32> {
    if dx.hypot(dy) < 8. {
        return None;
    }
    let mut a = dy.atan2(dx).to_degrees();
    // Fold into (-45, 45] around the nearest axis.
    while a > 45. {
        a -= 90.;
    }
    while a <= -45. {
        a += 90.;
    }
    if flip_h != flip_v {
        a = -a;
    }
    Some(-a)
}

impl Laika {
    // ---- Transform panel ------------------------------------------------------

    /// Rows are appended to the panel column itself (like Detail and
    /// Effects) so sliders share the rail's exact insets.
    pub(crate) fn transform_body(&self, col: Div, cx: &mut Context<Self>) -> Div {
        let geom = self
            .state
            .primary
            .map(|id| self.acknowledged_geom(id))
            .unwrap_or_default();
        let mode = geom.upright.mode();
        let mut chips = div().flex().flex_wrap().gap(px(4.)).px(px(14.)).pb(px(6.));
        for m in UprightMode::ALL {
            let on = m == mode;
            let busy = self.geo.analyzing.is_some_and(|(_, bm)| bm == m);
            chips = chips.child(
                div()
                    .id(("upright", m as usize))
                    .px(px(7.))
                    .py(px(3.))
                    .rounded(px(3.))
                    .font_family(SANS)
                    .text_size(px(10.))
                    .text_color(rgb(if on { TEXT_PRIMARY } else { TEXT_DIM }))
                    .when(on, |d| d.bg(rgb(bg_segment_active())))
                    .hover(|s| s.bg(rgb(bg_row_hover())))
                    .on_hover(self.tip(match m {
                        UprightMode::Off => "No automatic correction",
                        UprightMode::Auto => "Balanced level + perspective from detected lines",
                        UprightMode::Level => "Rotate so horizons are level",
                        UprightMode::Vertical => "Level plus vertical perspective",
                        UprightMode::Full => "Level, vertical and horizontal perspective",
                        UprightMode::Guided => {
                            "Draw 2–4 lines on the photo that should be straight"
                        }
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.set_upright_mode(m, cx);
                    }))
                    .child(if busy {
                        format!("{}…", m.label())
                    } else {
                        m.label().to_string()
                    }),
            );
        }
        let note = if !self.geo.note.is_empty() {
            self.geo.note.clone()
        } else {
            match mode {
                UprightMode::Off => "manual sliders only".to_string(),
                UprightMode::Guided => format!(
                    "{} guide{} · {}",
                    geom.upright.n_guides,
                    if geom.upright.n_guides == 1 { "" } else { "s" },
                    if self.geo.guide_draw {
                        "drag on the photo to draw · click a line to remove · Esc done"
                    } else {
                        "click Guided to draw more"
                    }
                ),
                _ => format!(
                    "{} · V {:+.0} · H {:+.0} · R {:+.1}°",
                    mode.label(),
                    geom.upright.auto[0],
                    geom.upright.auto[1],
                    geom.upright.auto[2]
                ),
            }
        };
        let constrain = geom.upright.constrain;
        let mut body = col
            .child(
                div()
                    .px(px(14.))
                    .pb(px(4.))
                    .font_family(SANS)
                    .text_size(px(10.))
                    .text_color(rgb(TEXT_DIMMER))
                    .child("Upright".to_string()),
            )
            .child(chips)
            .child(
                div()
                    .px(px(14.))
                    .pb(px(8.))
                    .font_family(SANS)
                    .text_size(px(10.))
                    .text_color(rgb(TEXT_DIM))
                    .child(note),
            );
        let mut sliders = div().px(px(14.)).flex().flex_col();
        for i in edit::TRANSFORM_RANGE {
            sliders = sliders.child(self.slider_row(i, slider::KNOB, cx));
        }
        body = body.child(sliders);
        body.child(
            div()
                .flex()
                .items_center()
                .gap(px(8.))
                .px(px(14.))
                .pt(px(6.))
                .child(
                    div()
                        .id("constrain-crop")
                        .on_hover(self.tip("Shrink the crop so no empty corners show"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            let Some(pid) = this.state.primary else {
                                return;
                            };
                            let on = !this.acknowledged_geom(pid).upright.constrain;
                            this.commit_geom(
                                pid,
                                move |g| g.upright.constrain = on,
                                "Constrain to image",
                                if on { "on" } else { "off" },
                                cx,
                            );
                        }))
                        .child(chip::filter_chip("Constrain to image", constrain)),
                )
                .child(
                    div()
                        .id("reset-geometry")
                        .on_hover(self.tip(
                            "Reset crop, straighten, Upright and Transform (keeps rotation, flips and tone)",
                        ))
                        .on_click(cx.listener(|this, _, _, cx| this.reset_geometry(cx)))
                        .child(chip::filter_chip("Reset geometry", false)),
                ),
        )
    }

    /// One undo step that changes a photo's geometry (and an open crop
    /// draft the same way, so the tool never shows stale geometry).
    pub(crate) fn commit_geom(
        &mut self,
        pid: i64,
        f: impl Fn(&mut edit::CropGeom),
        label: &str,
        value: &str,
        cx: &mut Context<Self>,
    ) {
        let before = self.snap_current(pid);
        f(&mut self.state.edit(pid).geom);
        if Some(pid) == self.state.primary {
            if let Some(d) = self.crop_draft.as_mut() {
                f(d);
            }
        }
        self.record_step(pid, label, value, before);
        self.last_batch = vec![pid];
        self.last_was_meta = false;
        self.last_was_remove = false;
        self.redo_batch.clear();
        self.edits_dirty = true;
        self.persist_edits();
        self.request_sidecar(pid, true);
        if !self.sidecar_pending.is_empty() {
            self.kick_save_timer(cx);
        }
        self.submit_dev(cx);
        self.sync_derived(pid, cx);
        cx.notify();
    }

    /// Crop, straighten, Upright and the Transform sliders back to
    /// defaults as one step (orientation and tone stay).
    pub(crate) fn reset_geometry(&mut self, cx: &mut Context<Self>) {
        let Some(pid) = self.state.primary else {
            return;
        };
        let before = self.snap_current(pid);
        let def = edit::defaults();
        {
            let e = self.state.edit(pid);
            e.geom.rect = [0., 0., 1., 1.];
            e.geom.angle = 0.;
            e.geom.upright = Default::default();
            e.crop = None;
            for i in edit::TRANSFORM_RANGE {
                e.params[i] = def[i];
            }
        }
        for i in edit::TRANSFORM_RANGE {
            self.values[i] = def[i];
        }
        if let Some(d) = self.crop_draft.as_mut() {
            d.rect = [0., 0., 1., 1.];
            d.angle = 0.;
            d.upright = Default::default();
            self.crop_aspect_draft = None;
        }
        self.geo.guide_draw = false;
        self.geo.note.clear();
        self.record_step(pid, "Reset geometry", "", before);
        self.last_batch = vec![pid];
        self.last_was_meta = false;
        self.last_was_remove = false;
        self.redo_batch.clear();
        self.edits_dirty = true;
        self.persist_edits();
        self.request_sidecar(pid, true);
        if !self.sidecar_pending.is_empty() {
            self.kick_save_timer(cx);
        }
        self.submit_dev(cx);
        self.sync_derived(pid, cx);
        cx.notify();
    }

    // ---- Upright ---------------------------------------------------------------

    fn display_dims_of(&self, pid: i64, geom: &edit::CropGeom) -> (f32, f32) {
        let (fw, fh) = self.geom_frame(pid);
        geom.display_dims(fw, fh)
    }

    pub(crate) fn set_upright_mode(&mut self, mode: UprightMode, cx: &mut Context<Self>) {
        let Some(pid) = self.state.primary else {
            return;
        };
        self.geo.note.clear();
        match mode {
            UprightMode::Off => {
                self.geo.guide_draw = false;
                self.commit_geom(
                    pid,
                    |g| {
                        g.upright.mode = UprightMode::Off.code();
                        g.upright.auto = [0.; 3];
                    },
                    "Upright",
                    "Off",
                    cx,
                );
            }
            UprightMode::Guided => {
                self.geo.guide_draw = true;
                if self.crop_open {
                    self.cancel_crop(cx);
                }
                if self.zoom != zoom::ZoomLevel::Fit {
                    self.set_zoom(zoom::ZoomLevel::Fit, Some((0.5, 0.5)), cx);
                }
                self.solve_guided(pid, "Upright", "Guided", cx);
            }
            _ => {
                if self.geo.lines.contains_key(&pid) {
                    self.apply_upright_solution(pid, mode, cx);
                } else {
                    self.kick_line_analysis(pid, mode, cx);
                }
            }
        }
    }

    /// Solve from cached edges and commit, or explain why nothing moved.
    fn apply_upright_solution(&mut self, pid: i64, mode: UprightMode, cx: &mut Context<Self>) {
        self.geo.guide_draw = false;
        let geom = self.acknowledged_geom(pid);
        let (dw, dh) = self.display_dims_of(pid, &geom);
        let Some((segs, w, h)) = self.geo.lines.get(&pid) else {
            return;
        };
        let lines = upright::segments_to_lines(segs, *w, *h, geom.rotation);
        match upright::solve(mode, &lines, dw / dh.max(1.)) {
            Some(sol) => {
                self.geo.note = format!(
                    "{} · {} lines · {:.1}° → {:.1}° off axis",
                    mode.label(),
                    sol.used,
                    sol.before_deg,
                    sol.after_deg
                );
                self.commit_geom(
                    pid,
                    move |g| {
                        g.upright.mode = mode.code();
                        g.upright.auto = sol.auto;
                    },
                    "Upright",
                    mode.label(),
                    cx,
                );
            }
            None => {
                self.geo.note = format!(
                    "{}: no straight edges found — left unchanged (try Guided)",
                    mode.label()
                );
                cx.notify();
            }
        }
    }

    /// Detect straight edges on the cached 2048 editing linear (decoded
    /// once), off the UI thread.
    fn kick_line_analysis(&mut self, pid: i64, mode: UprightMode, cx: &mut Context<Self>) {
        if self.geo.analyzing.is_some() {
            return;
        }
        let Some(photo) = self.find(pid).cloned() else {
            return;
        };
        if laika_raw::media_kind(std::path::Path::new(&photo.path))
            == Some(laika_raw::MediaKind::Video)
        {
            self.geo.note = "Upright needs a still photo".to_string();
            cx.notify();
            return;
        }
        self.geo.analyzing = Some((pid, mode));
        self.geo.note = "finding straight edges…".to_string();
        let cache = self.cache_dir.clone();
        cx.spawn(async move |entity, cx| {
            let found = cx
                .background_spawn(async move {
                    laika_raw::on_big_stack(move || {
                        let path = std::path::Path::new(&photo.path);
                        let img = laika_raw::decode::decode_editing(path, &photo.blake3, &cache)
                            .or_else(|_| laika_raw::decode::linear_from_raster(path, Some(2048)))?;
                        let (w, h) = (img.width as usize, img.height as usize);
                        let mut rgb = Vec::with_capacity(w * h * 3);
                        for y in 0..img.height {
                            for x in 0..img.width {
                                rgb.extend_from_slice(&img.pixel_f32(x, y));
                            }
                        }
                        let (lum, lw, lh, _) =
                            laika_core::lines::luminance_for_analysis(&rgb, w, h, 1024);
                        let segs = laika_core::lines::detect_segments(&lum, lw, lh);
                        Ok::<_, String>((segs, lw, lh))
                    })?
                })
                .await;
            entity
                .update(cx, |this, cx| {
                    this.geo.analyzing = None;
                    match found {
                        Ok(result) => {
                            this.geo.lines.insert(pid, result);
                            if this.state.primary == Some(pid) {
                                this.apply_upright_solution(pid, mode, cx);
                            }
                        }
                        Err(e) => {
                            this.geo.note = format!("Upright failed: {e}");
                        }
                    }
                    cx.notify();
                })
                .ok();
        })
        .detach();
        cx.notify();
    }

    fn solve_guided(&mut self, pid: i64, label: &str, value: &str, cx: &mut Context<Self>) {
        let aspect = self.display_aspect(pid);
        self.commit_geom(pid, move |g| resolve_guided(g, aspect), label, value, cx);
    }

    // ---- stage mapping ---------------------------------------------------------

    /// Window point → output uv over the displayed (cropped) render.
    fn stage_uv(&self, pos: (f32, f32)) -> Option<(f32, f32)> {
        let g = self.stage_geom()?;
        if self.render_zoom() != zoom::ZoomLevel::Fit {
            return None;
        }
        let b = self.viewport_box.get();
        Some(zoom::to_image(
            pos.0 - b.origin.x.as_f32(),
            pos.1 - b.origin.y.as_f32(),
            &g,
        ))
    }

    /// Output uv → pre-correction display uv for the primary.
    fn output_to_display(&self, pid: i64, uv: (f32, f32)) -> Option<(f32, f32)> {
        let geom = self.acknowledged_geom(pid);
        let (dw, dh) = self.display_dims_of(pid, &geom);
        let rg = self.render_geom(pid);
        let (x, y) = edit::crop_sample_warp(
            uv.0,
            uv.1,
            rg.rect,
            rg.angle_rad.to_degrees(),
            rg.flip_h,
            rg.flip_v,
            0,
            dw,
            dh,
            &rg.warp,
        )?;
        Some((x / dw, y / dh))
    }

    /// Guide endpoints on screen (stage-relative px), for drawing and
    /// hit-testing.
    fn guide_screen_lines(&self) -> Vec<((f32, f32), (f32, f32))> {
        let (Some(pid), Some(g)) = (self.state.primary, self.stage_geom()) else {
            return Vec::new();
        };
        let geom = self.acknowledged_geom(pid);
        if geom.upright.n_guides == 0 {
            return Vec::new();
        }
        let (dw, dh) = self.display_dims_of(pid, &geom);
        let rg = self.render_geom(pid);
        let to_screen = |u: f32, v: f32| {
            edit::display_to_output(
                u,
                v,
                rg.rect,
                rg.angle_rad.to_degrees(),
                rg.flip_h,
                rg.flip_v,
                dw,
                dh,
                &rg.warp,
            )
            .map(|(ou, ov)| (g.off_x + ou * g.disp_w, g.off_y + ov * g.disp_h))
        };
        geom.upright
            .guide_lines()
            .iter()
            .filter_map(|l| Some((to_screen(l.u0, l.v0)?, to_screen(l.u1, l.v1)?)))
            .collect()
    }

    /// Guided: a press removes a guide under the pointer, else starts one.
    pub(crate) fn guide_press(&mut self, pos: (f32, f32), cx: &mut Context<Self>) -> bool {
        if !self.geo.guide_draw || self.crop_open {
            return false;
        }
        let Some(pid) = self.state.primary else {
            return false;
        };
        if self.stage_uv(pos).is_none() {
            self.geo.note = "Guided lines need Fit zoom".to_string();
            cx.notify();
            return true;
        }
        let b = self.viewport_box.get();
        let local = (pos.0 - b.origin.x.as_f32(), pos.1 - b.origin.y.as_f32());
        let hit = self
            .guide_screen_lines()
            .iter()
            .position(|(a, c)| dist_to_segment(local, *a, *c) <= 6.);
        if let Some(k) = hit {
            let aspect = self.display_aspect(pid);
            self.commit_geom(
                pid,
                move |g| {
                    let n = g.upright.n_guides as usize;
                    if k < n {
                        g.upright.guides.copy_within(k + 1..n, k);
                        g.upright.n_guides -= 1;
                    }
                    resolve_guided(g, aspect);
                },
                "Guided line",
                "removed",
                cx,
            );
            return true;
        }
        self.geo.guide = Some((pos, pos));
        cx.notify();
        true
    }

    pub(crate) fn guide_release(&mut self, cx: &mut Context<Self>) {
        let Some((a, b)) = self.geo.guide.take() else {
            return;
        };
        let Some(pid) = self.state.primary else {
            return;
        };
        if (a.0 - b.0).hypot(a.1 - b.1) < 12. {
            cx.notify();
            return;
        }
        let (Some(ua), Some(ub)) = (self.stage_uv(a), self.stage_uv(b)) else {
            cx.notify();
            return;
        };
        let (Some(da), Some(db)) = (
            self.output_to_display(pid, ua),
            self.output_to_display(pid, ub),
        ) else {
            cx.notify();
            return;
        };
        let aspect = self.display_aspect(pid);
        // One undo step: the line and the correction it produces.
        self.commit_geom(
            pid,
            move |g| {
                let up = &mut g.upright;
                if up.n_guides >= 4 {
                    // Oldest out: four is Lightroom's limit too.
                    up.guides.copy_within(1..4, 0);
                    up.n_guides = 3;
                }
                up.guides[up.n_guides as usize] = [da.0, da.1, db.0, db.1];
                up.n_guides += 1;
                resolve_guided(g, aspect);
            },
            "Guided line",
            "added",
            cx,
        );
    }

    fn display_aspect(&self, pid: i64) -> f32 {
        let geom = self.acknowledged_geom(pid);
        let (dw, dh) = self.display_dims_of(pid, &geom);
        dw / dh.max(1.)
    }

    /// Guides + the line being drawn, over the Develop stage.
    pub(crate) fn guides_overlay(&self) -> Option<AnyElement> {
        if self.crop_open || self.state.active_module != Module::Develop {
            return None;
        }
        let pid = self.state.primary?;
        let mode = self.acknowledged_geom(pid).upright.mode();
        if !(self.geo.guide_draw || (mode == UprightMode::Guided && self.geo.guide.is_some())) {
            return None;
        }
        let lines = self.guide_screen_lines();
        let b = self.viewport_box.get();
        let live = self.geo.guide.map(|(a, c)| {
            (
                (a.0 - b.origin.x.as_f32(), a.1 - b.origin.y.as_f32()),
                (c.0 - b.origin.x.as_f32(), c.1 - b.origin.y.as_f32()),
            )
        });
        Some(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| {
                    use gpui_kit::PathBuilder;
                    let o = bounds.origin;
                    let pt = |p: (f32, f32)| gpui_kit::point(o.x + px(p.0), o.y + px(p.1));
                    let stroke = |segs: &[((f32, f32), (f32, f32))],
                                  w: f32,
                                  color: u32,
                                  window: &mut Window| {
                        if segs.is_empty() {
                            return;
                        }
                        let mut path = PathBuilder::stroke(px(w));
                        for (a, c) in segs {
                            path.move_to(pt(*a));
                            path.line_to(pt(*c));
                        }
                        if let Ok(p) = path.build() {
                            window.paint_path(p, rgb(color));
                        }
                    };
                    // Dark halo under a bright line reads on any photo.
                    stroke(&lines, 3., 0x000000, window);
                    stroke(&lines, 1.5, 0xFFD24A, window);
                    if let Some(l) = live {
                        stroke(&[l], 3., 0x000000, window);
                        stroke(&[l], 1.5, 0xFFFFFF, window);
                    }
                },
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .into_any_element(),
        )
    }

    // ---- straighten tool -------------------------------------------------------

    /// Straighten press while cropping (tool on, or Cmd held).
    pub(crate) fn straighten_press(
        &mut self,
        pos: (f32, f32),
        cmd: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.crop_open || !(self.geo.straighten_tool || cmd) {
            return false;
        }
        self.geo.straighten = Some((pos, pos));
        cx.notify();
        true
    }

    pub(crate) fn straighten_release(&mut self, cx: &mut Context<Self>) {
        let Some((a, b)) = self.geo.straighten.take() else {
            return;
        };
        let (fh, fv) = self
            .crop_draft
            .map(|d| (d.flip_h, d.flip_v))
            .unwrap_or((false, false));
        match straighten_delta(b.0 - a.0, b.1 - a.1, fh, fv) {
            Some(delta) => {
                self.draft_mut(|d| d.angle = ((d.angle + delta) * 100.).round() / 100., cx);
                let angle = self.crop_draft.map(|d| d.angle).unwrap_or(0.);
                self.status_note = format!("straightened to {angle:+.2}° — Enter applies");
            }
            None => cx.notify(),
        }
    }

    /// The straighten line with its live angle readout.
    pub(crate) fn straighten_overlay(&self) -> Option<AnyElement> {
        let (a, c) = self.geo.straighten?;
        let b = self.viewport_box.get();
        let (ox, oy) = (b.origin.x.as_f32(), b.origin.y.as_f32());
        let (la, lc) = ((a.0 - ox, a.1 - oy), (c.0 - ox, c.1 - oy));
        let (fh, fv) = self
            .crop_draft
            .map(|d| (d.flip_h, d.flip_v))
            .unwrap_or((false, false));
        let readout = straighten_delta(c.0 - a.0, c.1 - a.1, fh, fv)
            .map(|d| format!("{:+.2}°", -d))
            .unwrap_or_default();
        Some(
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .child(
                    canvas(
                        |_, _, _| {},
                        move |bounds, _, window, _| {
                            use gpui_kit::PathBuilder;
                            let o = bounds.origin;
                            let pt = |p: (f32, f32)| gpui_kit::point(o.x + px(p.0), o.y + px(p.1));
                            for (w, color) in [(3., 0x000000), (1.5, 0xFFFFFF)] {
                                let mut path = PathBuilder::stroke(px(w));
                                path.move_to(pt(la));
                                path.line_to(pt(lc));
                                if let Ok(p) = path.build() {
                                    window.paint_path(p, rgb(color));
                                }
                            }
                        },
                    )
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full(),
                )
                .child(
                    div()
                        .absolute()
                        .left(px(lc.0 + 12.))
                        .top(px(lc.1 + 10.))
                        .px(px(6.))
                        .py(px(2.))
                        .rounded(px(3.))
                        .bg(rgba(0x000000B3))
                        .font_family(SANS)
                        .text_size(px(10.5))
                        .text_color(rgb(0xFFFFFF))
                        .child(readout),
                )
                .into_any_element(),
        )
    }

    // ---- overlays, nudging, orientation swap ----------------------------------

    /// `O`: next enabled overlay; `Shift+O`: next orientation.
    pub(crate) fn cycle_overlay(&mut self, shift: bool, cx: &mut Context<Self>) {
        if shift {
            self.geo.overlay_orient = (self.geo.overlay_orient + 1) % 4;
            self.status_note = format!("overlay orientation {}", self.geo.overlay_orient + 1);
        } else {
            self.crop_grid = true;
            let i = Overlay::ALL
                .iter()
                .position(|o| *o == self.geo.overlay)
                .unwrap_or(0);
            let next = (1..=Overlay::ALL.len())
                .map(|k| Overlay::ALL[(i + k) % Overlay::ALL.len()])
                .find(|o| self.geo.overlay_set & o.bit() != 0);
            if let Some(o) = next {
                self.geo.overlay = o;
            }
            self.status_note = format!("overlay: {}", self.geo.overlay.label());
        }
        self.save_geo_prefs();
        cx.notify();
    }

    /// Paint the current overlay inside the crop box (stage px).
    pub(crate) fn overlay_element(&self, bw: f32, bh: f32) -> AnyElement {
        let lines = overlay_lines(
            self.geo.overlay,
            self.geo.overlay_orient,
            self.geo.grid_n,
            bw,
            bh,
        );
        canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                use gpui_kit::PathBuilder;
                let o = bounds.origin;
                let (w, h) = (bounds.size.width.as_f32(), bounds.size.height.as_f32());
                // Dark halo, then a light line: legible on bright and dark
                // photos alike.
                for (width, color) in [(2.5, 0x00000066u32), (1., 0xFFFFFFB0)] {
                    let mut path = PathBuilder::stroke(px(width));
                    for line in &lines {
                        for (k, p) in line.iter().enumerate() {
                            let q = gpui_kit::point(o.x + px(p.0 * w), o.y + px(p.1 * h));
                            if k == 0 {
                                path.move_to(q);
                            } else {
                                path.line_to(q);
                            }
                        }
                    }
                    if let Ok(p) = path.build() {
                        window.paint_path(p, rgba(color));
                    }
                }
            },
        )
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .into_any_element()
    }

    /// Arrow keys while cropping: move the box 1 px (Shift: 10 px) in
    /// output pixels, screen direction (flips mirrored).
    pub(crate) fn nudge_crop(&mut self, dx: i32, dy: i32, big: bool, cx: &mut Context<Self>) {
        let Some(pid) = self.state.primary else {
            return;
        };
        let Some(d) = self.crop_draft else {
            return;
        };
        let (dw, dh) = self.display_dims_of(pid, &d);
        let step = if big { 10. } else { 1. };
        let sx = if d.flip_h { -1. } else { 1. };
        let sy = if d.flip_v { -1. } else { 1. };
        let (mx, my) = (
            dx as f32 * step * sx / dw.max(1.),
            dy as f32 * step * sy / dh.max(1.),
        );
        self.draft_mut(
            |g| {
                g.rect[0] = (g.rect[0] + mx).clamp(0., 1. - g.rect[2]);
                g.rect[1] = (g.rect[1] + my).clamp(0., 1. - g.rect[3]);
            },
            cx,
        );
    }

    /// `X`: swap the crop box between landscape and portrait (and the
    /// aspect lock with it), around its center.
    pub(crate) fn swap_crop_orientation(&mut self, cx: &mut Context<Self>) {
        let Some(pid) = self.state.primary else {
            return;
        };
        let Some(d) = self.crop_draft else {
            return;
        };
        let (dw, dh) = self.display_dims_of(pid, &d);
        if let Some(a) = self.crop_aspect_draft {
            self.crop_aspect_draft = Some(1. / a.max(1e-3));
        }
        self.draft_mut(
            |g| {
                let [x, y, w, h] = g.rect;
                let (cx0, cy0) = (x + w / 2., y + h / 2.);
                // Same pixel extents, swapped: w_px' = h_px, h_px' = w_px.
                let mut nw = h * dh / dw.max(1.);
                let mut nh = w * dw / dh.max(1.);
                // Fit into the frame keeping the swapped ratio.
                let fit = (1. / nw.max(1e-6)).min(1. / nh.max(1e-6)).min(1.);
                nw *= fit;
                nh *= fit;
                g.rect = [
                    (cx0 - nw / 2.).clamp(0., 1. - nw),
                    (cy0 - nh / 2.).clamp(0., 1. - nh),
                    nw,
                    nh,
                ];
            },
            cx,
        );
    }

    // ---- numeric entry + aspect presets -----------------------------------------

    /// Current draft rect as field text (pixels of the display frame, or
    /// percent).
    pub(crate) fn crop_field_text(&self, k: usize) -> String {
        let (Some(pid), Some(d)) = (self.state.primary, self.crop_draft) else {
            return String::new();
        };
        let (dw, dh) = self.display_dims_of(pid, &d);
        let v = d.rect[k];
        if self.geo.units_px {
            let dim = if k % 2 == 0 { dw } else { dh };
            format!("{}", (v * dim).round() as i64)
        } else {
            format!("{:.1}", v * 100.)
        }
    }

    /// Commit a typed X/Y/W/H. Width/height keep an active aspect lock
    /// by moving the other side.
    pub(crate) fn commit_crop_field(
        &mut self,
        k: usize,
        buf: &str,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let (Some(pid), Some(d)) = (self.state.primary, self.crop_draft) else {
            return Err("open the crop tool first".to_string());
        };
        let t = buf.trim().trim_end_matches(['%', 'x']).trim();
        let n: f32 = t
            .parse()
            .ok()
            .filter(|v: &f32| v.is_finite())
            .ok_or_else(|| "type a number".to_string())?;
        let (dw, dh) = self.display_dims_of(pid, &d);
        let dim = if k % 2 == 0 { dw } else { dh };
        let norm = if self.geo.units_px {
            n / dim.max(1.)
        } else {
            n / 100.
        };
        let aspect = self
            .crop_aspect_draft
            .map(|a| self.norm_aspect(pid, a, d.rotation));
        self.draft_mut(
            |g| match k {
                0 => g.rect[0] = norm.clamp(0., 1. - g.rect[2]),
                1 => g.rect[1] = norm.clamp(0., 1. - g.rect[3]),
                2 => {
                    g.rect[2] = norm.clamp(0.02, 1. - g.rect[0]);
                    if let Some(a) = aspect {
                        g.rect[3] = (g.rect[2] / a).min(1. - g.rect[1]);
                    }
                }
                _ => {
                    g.rect[3] = norm.clamp(0.02, 1. - g.rect[1]);
                    if let Some(a) = aspect {
                        g.rect[2] = (g.rect[3] * a).min(1. - g.rect[0]);
                    }
                }
            },
            cx,
        );
        Ok(())
    }

    /// Save the draft's pixel ratio as a named preset.
    pub(crate) fn save_aspect_preset(&mut self, cx: &mut Context<Self>) {
        let (Some(pid), Some(d)) = (self.state.primary, self.crop_draft) else {
            return;
        };
        let (dw, dh) = self.display_dims_of(pid, &d);
        let r = (d.rect[2] * dw) / (d.rect[3] * dh).max(1e-3);
        if !(0.1..=10.).contains(&r) {
            return;
        }
        let label = ratio_label(r);
        if self.geo.aspects.iter().any(|(n, _)| *n == label)
            || CROP_ASPECTS.iter().any(|(n, _)| *n == label)
        {
            self.status_note = format!("{label} is already a preset");
        } else {
            self.geo.aspects.push((label.clone(), r));
            if self.geo.aspects.len() > 12 {
                self.geo.aspects.remove(0);
            }
            self.save_geo_prefs();
            self.status_note = format!("saved {label} as an aspect preset");
        }
        cx.notify();
    }

    pub(crate) fn load_geo_prefs(&mut self) {
        let Some(cat) = self.catalog.as_ref() else {
            return;
        };
        self.geo.aspects = parse_aspects(&cat.get_import_default("crop_aspect_presets"));
        let prefs = cat.get_import_default("crop_overlay");
        let mut parts = prefs.split(',');
        if let Some(i) = parts.next().and_then(|v| v.parse::<usize>().ok()) {
            if let Some(o) = Overlay::ALL.get(i) {
                self.geo.overlay = *o;
            }
        }
        if let Some(set) = parts.next().and_then(|v| v.parse::<u8>().ok()) {
            if set != 0 {
                self.geo.overlay_set = set & 0x7F;
            }
        }
        if let Some(n) = parts.next().and_then(|v| v.parse::<u8>().ok()) {
            self.geo.grid_n = n.clamp(2, 20);
        }
        if let Some(o) = parts.next().and_then(|v| v.parse::<u8>().ok()) {
            self.geo.overlay_orient = o % 4;
        }
    }

    pub(crate) fn save_geo_prefs(&self) {
        let Some(cat) = self.catalog.as_ref() else {
            return;
        };
        cat.set_import_default("crop_aspect_presets", &serialize_aspects(&self.geo.aspects));
        cat.set_import_default(
            "crop_overlay",
            &format!(
                "{},{},{},{}",
                self.geo.overlay as usize,
                self.geo.overlay_set,
                self.geo.grid_n,
                self.geo.overlay_orient
            ),
        );
    }

    /// Second crop row: straighten tool, fine angle, numeric box, units,
    /// overlays, outside treatment, user aspect presets.
    pub(crate) fn crop_row_extra(&self, cx: &mut Context<Self>) -> Div {
        let chip_btn = |id: &'static str, label: String, on: bool| {
            div()
                .id(id)
                .px(px(8.))
                .py(px(4.))
                .rounded(px(3.))
                .border_1()
                .border_color::<Hsla>(if on {
                    rgb(accent_line()).into()
                } else {
                    border_control()
                })
                .font_family(SANS)
                .text_size(px(10.5))
                .text_color(rgb(if on { accent_line() } else { TEXT_TERTIARY }))
                .hover(|s| s.bg(rgb(bg_row_hover())))
                .child(label)
        };
        let label = |t: &'static str| {
            div()
                .font_family(SANS)
                .text_size(px(10.5))
                .text_color(rgb(TEXT_DIM))
                .child(t.to_string())
        };
        let field = |this: &Self, id: text_input::FieldId, k: usize, cx: &mut Context<Self>| {
            this.field_cell(
                id,
                div()
                    .min_w(px(38.))
                    .font_family(SANS)
                    .text_size(px(10.5))
                    .text_color(rgb(TEXT_SECONDARY))
                    .child(this.crop_field_text(k)),
                false,
                "Type a value, Enter applies",
                cx,
            )
        };
        let mut row1 = div()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(6.))
            .child(
                chip_btn(
                    "straighten-tool",
                    "Straighten".to_string(),
                    self.geo.straighten_tool,
                )
                .on_hover(self.tip("Draw along a horizon or edge to level it (or ⌘-drag)"))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.geo.straighten_tool = !this.geo.straighten_tool;
                    cx.notify();
                })),
            )
            .child(
                chip_btn("angle-fine-down", "−0.1°".to_string(), false)
                    .on_hover(self.tip("Fine straighten counter-clockwise"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.draft_mut(|d| d.angle = ((d.angle - 0.1) * 10.).round() / 10., cx);
                    })),
            )
            .child(
                chip_btn("angle-fine-up", "+0.1°".to_string(), false)
                    .on_hover(self.tip("Fine straighten clockwise"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.draft_mut(|d| d.angle = ((d.angle + 0.1) * 10.).round() / 10., cx);
                    })),
            )
            .child(div().w(px(1.)).h(px(16.)).bg(rgba(0xFFFFFF14)))
            .child(label("X"))
            .child(field(self, text_input::FieldId::CropX, 0, cx))
            .child(label("Y"))
            .child(field(self, text_input::FieldId::CropY, 1, cx))
            .child(label("W"))
            .child(field(self, text_input::FieldId::CropW, 2, cx))
            .child(label("H"))
            .child(field(self, text_input::FieldId::CropH, 3, cx))
            .child(
                chip_btn(
                    "crop-units",
                    if self.geo.units_px { "px" } else { "%" }.to_string(),
                    false,
                )
                .on_hover(self.tip("Numeric box in pixels or percent"))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.geo.units_px = !this.geo.units_px;
                    cx.notify();
                })),
            )
            .child(
                chip_btn("crop-swap", "Swap ⤢".to_string(), false)
                    .on_hover(self.tip("Portrait ↔ landscape (X)"))
                    .on_click(cx.listener(|this, _, _, cx| this.swap_crop_orientation(cx))),
            )
            .child(div().w(px(1.)).h(px(16.)).bg(rgba(0xFFFFFF14)))
            .child(
                chip_btn(
                    "crop-overlay",
                    format!("Overlay: {}", self.geo.overlay.label()),
                    self.geo.overlay_menu,
                )
                .on_hover(self.tip("Choose overlays (O cycles, ⇧O flips)"))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.geo.overlay_menu = !this.geo.overlay_menu;
                    cx.notify();
                })),
            )
            .child(
                chip_btn("crop-outside", self.geo.outside.label().to_string(), false)
                    .on_hover(self.tip("Dim, hide, or show the area outside the crop"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.geo.outside = this.geo.outside.next();
                        cx.notify();
                    })),
            );
        // User presets, then save.
        for (i, (name, r)) in self.geo.aspects.iter().enumerate() {
            let on = self.crop_aspect_draft.is_some_and(|a| (a - r).abs() < 1e-3);
            let r = *r;
            row1 = row1.child(
                div()
                    .id(("user-aspect", i))
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .px(px(8.))
                    .py(px(4.))
                    .rounded(px(3.))
                    .border_1()
                    .border_color::<Hsla>(if on {
                        rgb(accent_line()).into()
                    } else {
                        border_control()
                    })
                    .font_family(SANS)
                    .text_size(px(10.5))
                    .text_color(rgb(if on { accent_line() } else { TEXT_TERTIARY }))
                    .hover(|s| s.bg(rgb(bg_row_hover())))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.set_crop_aspect(Some(r), cx);
                    }))
                    .child(name.clone())
                    .child(
                        div()
                            .id(("user-aspect-remove", i))
                            .text_color(rgb(TEXT_DIMMER))
                            .hover(|s| s.text_color(rgb(0xE56060)))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                cx.stop_propagation();
                                if i < this.geo.aspects.len() {
                                    this.geo.aspects.remove(i);
                                    this.save_geo_prefs();
                                    cx.notify();
                                }
                            }))
                            .child("×"),
                    ),
            );
        }
        row1 = row1.child(
            chip_btn("save-aspect", "+ Save ratio".to_string(), false)
                .on_hover(self.tip("Save the current box ratio as a preset"))
                .on_click(cx.listener(|this, _, _, cx| this.save_aspect_preset(cx))),
        );
        let mut col = div().flex().flex_col().gap(px(6.)).child(row1);
        if self.geo.overlay_menu {
            let mut menu = div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(6.))
                .child(label("Cycle with O:"));
            for (i, o) in Overlay::ALL.iter().copied().enumerate() {
                let enabled = self.geo.overlay_set & o.bit() != 0;
                menu = menu.child(
                    div()
                        .id(("overlay-kind", i))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let set = this.geo.overlay_set ^ o.bit();
                            // Never empty: the shown overlay stays in.
                            if set != 0 {
                                this.geo.overlay_set = set;
                            }
                            this.geo.overlay = o;
                            this.crop_grid = true;
                            this.save_geo_prefs();
                            cx.notify();
                        }))
                        .child(chip::filter_chip(o.label(), enabled)),
                );
            }
            menu = menu
                .child(label("Grid"))
                .child(
                    chip_btn("grid-less", "−".to_string(), false).on_click(cx.listener(
                        |this, _, _, cx| {
                            this.geo.grid_n = (this.geo.grid_n - 1).max(2);
                            this.save_geo_prefs();
                            cx.notify();
                        },
                    )),
                )
                .child(label_owned(format!(
                    "{}×{}",
                    self.geo.grid_n, self.geo.grid_n
                )))
                .child(
                    chip_btn("grid-more", "+".to_string(), false).on_click(cx.listener(
                        |this, _, _, cx| {
                            this.geo.grid_n = (this.geo.grid_n + 1).min(20);
                            this.save_geo_prefs();
                            cx.notify();
                        },
                    )),
                )
                .child(
                    chip_btn("overlay-flip", "Flip ⇧O".to_string(), false)
                        .on_click(cx.listener(|this, _, _, cx| this.cycle_overlay(true, cx))),
                );
            col = col.child(menu);
        }
        col
    }
}

/// Guided mode on, with the correction solved from the current guides
/// (none or one line of a family simply solves less).
fn resolve_guided(g: &mut edit::CropGeom, aspect: f32) {
    g.upright.mode = UprightMode::Guided.code();
    g.upright.auto = upright::solve(UprightMode::Guided, &g.upright.guide_lines(), aspect)
        .map(|s| s.auto)
        .unwrap_or([0.; 3]);
}

fn label_owned(t: String) -> Div {
    div()
        .font_family(SANS)
        .text_size(px(10.5))
        .text_color(rgb(TEXT_SECONDARY))
        .child(t)
}

fn dist_to_segment(p: (f32, f32), a: (f32, f32), b: (f32, f32)) -> f32 {
    let (vx, vy) = (b.0 - a.0, b.1 - a.1);
    let len2 = vx * vx + vy * vy;
    let t = if len2 > 0. {
        (((p.0 - a.0) * vx + (p.1 - a.1) * vy) / len2).clamp(0., 1.)
    } else {
        0.
    };
    let (cx, cy) = (a.0 + t * vx, a.1 + t * vy);
    (p.0 - cx).hypot(p.1 - cy)
}

#[cfg(test)]
mod tests {
    // Not `super::*`: the app prelude's gpui `test` macro shadows #[test].
    use super::{
        Overlay, dist_to_segment, overlay_lines, parse_aspects, ratio_label, serialize_aspects,
        straighten_delta,
    };

    #[test]
    fn overlays_stay_in_the_box_and_orient() {
        for kind in Overlay::ALL {
            for orient in 0..4 {
                let lines = overlay_lines(kind, orient, 6, 600., 400.);
                assert!(!lines.is_empty(), "{kind:?}");
                for line in &lines {
                    for p in line {
                        assert!(
                            (-1e-3..=1.001).contains(&p.0) && (-1e-3..=1.001).contains(&p.1),
                            "{kind:?} {orient} {p:?}"
                        );
                    }
                }
            }
        }
        // Diagonals are 45° in pixels on a 3:2 box.
        let d = overlay_lines(Overlay::Diagonals, 0, 6, 600., 400.);
        let (a, b) = (d[0][0], d[0][1]);
        assert!((((b.0 - a.0) * 600.) - ((b.1 - a.1) * 400.)).abs() < 1e-3);
        // Spiral is continuous and starts at a corner.
        let s = &overlay_lines(Overlay::Spiral, 0, 6, 600., 400.)[0];
        assert!(
            (s[0].0 - 0.).abs() < 1e-4 && (s[0].1 - 1.).abs() < 1e-4,
            "{:?}",
            s[0]
        );
        for w in s.windows(2) {
            assert!((w[1].0 - w[0].0).hypot(w[1].1 - w[0].1) < 0.2);
        }
        // Mirroring flips the spiral's start corner.
        let m = &overlay_lines(Overlay::Spiral, 1, 6, 600., 400.)[0];
        assert!((m[0].0 - 1.).abs() < 1e-4);
        // Triangle perpendicular meets the diagonal at a right angle (px).
        let t = overlay_lines(Overlay::Triangle, 0, 6, 600., 400.);
        let (p, q) = (t[1][0], t[1][1]);
        let dot = (q.0 - p.0) * 600. * 600. + (q.1 - p.1) * 400. * 400.;
        assert!(dot.abs() < 1e-2, "{dot}");
        assert_eq!(overlay_lines(Overlay::Custom, 0, 4, 1., 1.).len(), 6);
    }

    #[test]
    fn straighten_levels_to_the_nearest_axis() {
        // A horizon falling 5° to the right needs −5°.
        let dy = 5f32.to_radians().tan() * 300.;
        assert!((straighten_delta(300., dy, false, false).unwrap() + 5.).abs() < 1e-3);
        // A near-vertical edge whose top sits right of its bottom is also
        // turned clockwise, so it needs −3° too; leaning left needs +3°.
        let dx = -(3f32.to_radians().tan()) * 300.;
        let d = straighten_delta(dx, 300., false, false).unwrap();
        assert!((d + 3.).abs() < 1e-3, "{d}");
        let d = straighten_delta(-dx, 300., false, false).unwrap();
        assert!((d - 3.).abs() < 1e-3, "{d}");
        // A mirrored view reverses the correction; tiny drags do nothing.
        assert!((straighten_delta(300., dy, true, false).unwrap() - 5.).abs() < 1e-3);
        assert!(straighten_delta(3., 1., false, false).is_none());
    }

    #[test]
    fn aspect_presets_round_trip() {
        let list = vec![("5:4".to_string(), 1.25), ("Pano".to_string(), 3.)];
        assert_eq!(parse_aspects(&serialize_aspects(&list)), list);
        assert!(parse_aspects("bad;x=nan;y=99;=2").is_empty());
        assert_eq!(ratio_label(1.25), "5:4");
        assert_eq!(ratio_label(0.8), "4:5");
        assert_eq!(ratio_label(16. / 9.), "16:9");
        assert_eq!(ratio_label(1.2345), "1.23");
    }

    #[test]
    fn segment_distance() {
        assert_eq!(dist_to_segment((5., 3.), (0., 0.), (10., 0.)), 3.);
        assert_eq!(dist_to_segment((-4., 3.), (0., 0.), (10., 0.)), 5.);
    }
}
