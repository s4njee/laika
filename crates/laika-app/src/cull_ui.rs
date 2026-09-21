//! S10: cull before the import finishes. The import review grid becomes a
//! culling surface — rate, flag, and label straight from the card's
//! embedded previews, look closer in a Loupe (100% on the camera JPEG),
//! and import only the keepers. Marks are applied once, as each photo
//! lands in the catalog.

use super::*;
use std::path::Path;

/// Largest long edge kept for the Loupe's fit image.
const FIT_EDGE: u32 = 2560;
/// Fit images kept around the current photo.
const FIT_CACHE: usize = 12;
/// Photos ahead/behind the current one prefetched for the Loupe.
const PREFETCH: usize = 3;

/// A culling decision made before import.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct Cull {
    pub rating: u8,
    /// 1 pick, -1 reject, 0 none.
    pub flag: i8,
    pub label: u8,
}

impl Cull {
    pub fn is_empty(&self) -> bool {
        *self == Cull::default()
    }
}

pub(crate) struct CullUi {
    /// Source file → decision (survives closing and reopening the dialog).
    pub marks: HashMap<PathBuf, Cull>,
    /// Entry index under the cursor.
    pub focus: Option<usize>,
    pub loupe: bool,
    /// 100% view centered on this normalized point.
    pub zoom: Option<(f32, f32)>,
    pub fit: HashMap<usize, Arc<RenderImage>>,
    pub full: Option<(usize, Arc<RenderImage>)>,
    pub loading: bool,
    pub full_loading: Option<usize>,
    /// Import options.
    pub skip_rejects: bool,
    pub picks_only: bool,
    pub stage_box: Rc<std::cell::Cell<Bounds<Pixels>>>,
    pub grid_box: Rc<std::cell::Cell<Bounds<Pixels>>>,
}

impl Default for CullUi {
    fn default() -> Self {
        Self {
            marks: HashMap::new(),
            focus: None,
            loupe: false,
            zoom: None,
            fit: HashMap::new(),
            full: None,
            loading: false,
            full_loading: None,
            skip_rejects: true,
            picks_only: false,
            stage_box: Rc::new(std::cell::Cell::new(Bounds {
                origin: point(px(0.), px(0.)),
                size: size(px(0.), px(0.)),
            })),
            grid_box: Rc::new(std::cell::Cell::new(Bounds {
                origin: point(px(0.), px(0.)),
                size: size(px(0.), px(0.)),
            })),
        }
    }
}

/// RGBA → BGRA buffer ready for a RenderImage (built on the UI thread).
fn to_bgra(img: image::DynamicImage, max_edge: u32) -> Option<image::RgbaImage> {
    let img = if max_edge > 0 && img.width().max(img.height()) > max_edge {
        img.resize(max_edge, max_edge, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let rgba = img.to_rgba8();
    let (w, h) = (rgba.width(), rgba.height());
    let mut px = rgba.into_raw();
    rgba_to_bgra_in_place(&mut px);
    image::RgbaImage::from_raw(w, h, px)
}

fn render(buf: image::RgbaImage) -> Arc<RenderImage> {
    Arc::new(RenderImage::new(smallvec::smallvec![image::Frame::new(
        buf
    )]))
}

impl Laika {
    fn cull_entries_len(&self) -> usize {
        self.import_dialog
            .as_ref()
            .map(|d| d.entries.len())
            .unwrap_or(0)
    }

    fn cull_path(&self, i: usize) -> Option<PathBuf> {
        self.import_dialog
            .as_ref()?
            .entries
            .get(i)
            .map(|e| e.path.clone())
    }

    pub(crate) fn cull_mark(&self, path: &Path) -> Cull {
        self.cull.marks.get(path).copied().unwrap_or_default()
    }

    fn set_mark(&mut self, f: impl FnOnce(&mut Cull), cx: &mut Context<Self>) {
        let Some(i) = self.cull.focus else {
            return;
        };
        let Some(path) = self.cull_path(i) else {
            return;
        };
        let mut m = self.cull_mark(&path);
        f(&mut m);
        if m.is_empty() {
            self.cull.marks.remove(&path);
        } else {
            self.cull.marks.insert(path, m);
        }
        if self.auto_advance {
            self.cull_move(1, cx);
        }
        cx.notify();
    }

    pub(crate) fn cull_focus(&mut self, i: usize, cx: &mut Context<Self>) {
        if i >= self.cull_entries_len() {
            return;
        }
        self.cull.focus = Some(i);
        if self.cull.full.as_ref().is_some_and(|(f, _)| *f != i) {
            if let Some((_, old)) = self.cull.full.take() {
                self.stale.push(old);
            }
        }
        if self.cull.loupe {
            self.kick_cull_load(cx);
            if self.cull.zoom.is_some() {
                self.kick_cull_full(i, cx);
            }
        }
        cx.notify();
    }

    fn cull_move(&mut self, delta: isize, cx: &mut Context<Self>) {
        let n = self.cull_entries_len();
        if n == 0 {
            return;
        }
        let cur = self.cull.focus.unwrap_or(0) as isize;
        let next = (cur + delta).clamp(0, n as isize - 1) as usize;
        self.cull_focus(next, cx);
    }

    pub(crate) fn cull_open_loupe(&mut self, i: usize, cx: &mut Context<Self>) {
        self.cull.loupe = true;
        self.cull.zoom = None;
        self.cull_focus(i, cx);
    }

    fn cull_toggle_zoom(&mut self, at: (f32, f32), cx: &mut Context<Self>) {
        let Some(i) = self.cull.focus else { return };
        if self.cull.zoom.is_some() {
            self.cull.zoom = None;
        } else {
            self.cull.zoom = Some(at);
            self.kick_cull_full(i, cx);
        }
        cx.notify();
    }

    /// Culling keys in the import review. True when handled.
    pub(crate) fn cull_key(&mut self, key: &str, shift: bool, cx: &mut Context<Self>) -> bool {
        let reviewing = self
            .import_dialog
            .as_ref()
            .is_some_and(|d| d.stage == ImportStage::Review);
        if !reviewing || self.field.is_some() || self.cull_entries_len() == 0 {
            return false;
        }
        let gw = self.cull.grid_box.get().size.width.as_f32();
        let cols = if gw > 10. {
            ((gw + 8.) / 140.).floor().max(1.) as isize
        } else {
            4
        };
        match key {
            "left" => self.cull_move(-1, cx),
            "right" => self.cull_move(1, cx),
            "up" if !self.cull.loupe => self.cull_move(-cols, cx),
            "down" if !self.cull.loupe => self.cull_move(cols, cx),
            "0" | "1" | "2" | "3" | "4" | "5" => {
                let r = key.parse::<u8>().unwrap_or(0);
                self.set_mark(|m| m.rating = r, cx);
            }
            "p" => self.set_mark(|m| m.flag = if m.flag == 1 { 0 } else { 1 }, cx),
            "x" => self.set_mark(|m| m.flag = if m.flag == -1 { 0 } else { -1 }, cx),
            "u" => self.set_mark(|m| m.flag = 0, cx),
            "6" | "7" | "8" | "9" => {
                let l = laika_core::labels::label_for_key(key).unwrap_or(0);
                self.set_mark(|m| m.label = if m.label == l { 0 } else { l }, cx);
            }
            "e" | "space" => {
                let i = self.cull.focus.unwrap_or(0);
                if self.cull.loupe && key == "space" {
                    self.cull_toggle_zoom((0.5, 0.5), cx);
                } else {
                    self.cull_open_loupe(i, cx);
                }
            }
            "z" if self.cull.loupe => self.cull_toggle_zoom((0.5, 0.5), cx),
            "g" => {
                self.cull.loupe = false;
                self.cull.zoom = None;
                cx.notify();
            }
            "escape" if self.cull.loupe => {
                self.cull.loupe = false;
                self.cull.zoom = None;
                cx.notify();
            }
            // Enter would start the import — never from inside the Loupe.
            "enter" if self.cull.loupe => {}
            "a" if !shift => {
                self.auto_advance = !self.auto_advance;
                self.status_note = if self.auto_advance {
                    "auto-advance on".to_string()
                } else {
                    "auto-advance off".to_string()
                };
                cx.notify();
            }
            _ => return false,
        }
        true
    }

    // ---- loading ----------------------------------------------------------------------

    /// Loupe fit images for the current photo and its neighbours, nearest
    /// first. Single flight; the queue re-reads the focus each step.
    fn kick_cull_load(&mut self, cx: &mut Context<Self>) {
        if self.cull.loading {
            return;
        }
        self.cull.loading = true;
        cx.spawn(async move |entity, cx| {
            loop {
                let next = entity
                    .update(cx, |this, _| {
                        let focus = this.cull.focus?;
                        let n = this.cull_entries_len();
                        let mut order = vec![focus];
                        for d in 1..=PREFETCH {
                            if focus + d < n {
                                order.push(focus + d);
                            }
                            if focus >= d {
                                order.push(focus - d);
                            }
                        }
                        let i = order.into_iter().find(|i| !this.cull.fit.contains_key(i))?;
                        Some((i, this.cull_path(i)?))
                    })
                    .ok()
                    .flatten();
                let Some((i, path)) = next else { break };
                let t = Instant::now();
                let buf = cx
                    .background_spawn(async move {
                        laika_raw::on_big_stack(move || {
                            laika_raw::preview::cull_image(&path, 2048)
                                .ok()
                                .and_then(|img| to_bgra(img, FIT_EDGE))
                        })
                        .unwrap_or(None)
                    })
                    .await;
                let ok = entity
                    .update(cx, |this, cx| {
                        let Some(buf) = buf else {
                            // Unreadable: remember an empty slot so the loop moves on.
                            if let Some(small) = this
                                .review_thumbs
                                .iter()
                                .find(|p| p.entry_index == i)
                                .and_then(|p| p.image.clone())
                            {
                                this.cull.fit.insert(i, small);
                            }
                            return true;
                        };
                        if t.elapsed().as_millis() > 400 {
                            eprintln!("[cull] loupe image {i} took {} ms", t.elapsed().as_millis());
                        }
                        this.cull.fit.insert(i, render(buf));
                        // Evict whatever is farthest from the cursor.
                        let focus = this.cull.focus.unwrap_or(i);
                        while this.cull.fit.len() > FIT_CACHE {
                            let Some(far) = this
                                .cull
                                .fit
                                .keys()
                                .copied()
                                .max_by_key(|k| k.abs_diff(focus))
                            else {
                                break;
                            };
                            if let Some(img) = this.cull.fit.remove(&far) {
                                this.stale.push(img);
                            }
                        }
                        cx.notify();
                        this.cull.loupe
                    })
                    .unwrap_or(false);
                if !ok {
                    break;
                }
            }
            entity.update(cx, |this, _| this.cull.loading = false).ok();
        })
        .detach();
    }

    /// The camera JPEG at full size for 100% (current photo only).
    fn kick_cull_full(&mut self, i: usize, cx: &mut Context<Self>) {
        if self.cull.full.as_ref().is_some_and(|(f, _)| *f == i)
            || self.cull.full_loading == Some(i)
        {
            return;
        }
        let Some(path) = self.cull_path(i) else {
            return;
        };
        self.cull.full_loading = Some(i);
        cx.spawn(async move |entity, cx| {
            let buf = cx
                .background_spawn(async move {
                    laika_raw::on_big_stack(move || {
                        laika_raw::preview::cull_image(&path, 0)
                            .ok()
                            .and_then(|img| to_bgra(img, 0))
                    })
                    .unwrap_or(None)
                })
                .await;
            entity
                .update(cx, |this, cx| {
                    this.cull.full_loading = None;
                    if this.cull.focus != Some(i) {
                        return;
                    }
                    if let Some(buf) = buf {
                        if let Some((_, old)) = this.cull.full.replace((i, render(buf))) {
                            this.stale.push(old);
                        }
                    }
                    cx.notify();
                })
                .ok();
        })
        .detach();
    }

    /// Drop Loupe images when the review closes.
    pub(crate) fn cull_release(&mut self) {
        for (_, img) in self.cull.fit.drain() {
            self.stale.push(img);
        }
        if let Some((_, img)) = self.cull.full.take() {
            self.stale.push(img);
        }
        self.cull.loupe = false;
        self.cull.zoom = None;
        self.cull.focus = None;
    }

    // ---- import --------------------------------------------------------------------------

    /// Entries the import will bring in, honoring the cull options.
    pub(crate) fn cull_filter(
        &self,
        entries: Vec<laika_core::import::ScanEntry>,
    ) -> Vec<laika_core::import::ScanEntry> {
        entries
            .into_iter()
            .filter(|e| {
                let m = self.cull_mark(&e.path);
                if self.cull.picks_only {
                    m.flag == 1
                } else {
                    !(self.cull.skip_rejects && m.flag == -1)
                }
            })
            .collect()
    }

    /// Apply a photo's cull marks as it lands in the catalog — once: the
    /// mark is consumed.
    pub(crate) fn apply_cull_marks(&mut self, id: i64, source: &Path) {
        let Some(m) = self.cull.marks.remove(source) else {
            return;
        };
        let Some(cat) = self.catalog.as_ref() else {
            return;
        };
        let mut ok = true;
        if m.rating > 0 {
            ok &= cat.set_rating(id, m.rating).is_ok();
        }
        if m.flag != 0 {
            ok &= cat.set_flag(id, m.flag == 1, m.flag == -1).is_ok();
        }
        if m.label > 0 {
            ok &= cat.set_label(id, m.label).is_ok();
        }
        if !ok {
            eprintln!("[cull] couldn't save marks for {}", source.display());
        }
        self.refresh_photo_row(id);
        self.request_sidecar(id, false);
    }

    // ---- view ------------------------------------------------------------------------

    /// Stars / flag / label strip for a review cell or the Loupe.
    pub(crate) fn cull_badges(m: Cull, size: f32) -> Div {
        let mut row = div().flex().items_center().gap(px(4.));
        if m.flag == 1 {
            row = row.child(
                div()
                    .text_size(px(size))
                    .text_color(rgb(accent_line()))
                    .child("⚑"),
            );
        } else if m.flag == -1 {
            row = row.child(
                div()
                    .text_size(px(size))
                    .text_color(rgb(0xE56060))
                    .child("✕"),
            );
        }
        if m.rating > 0 {
            row = row.child(
                div()
                    .text_size(px(size))
                    .text_color(rgb(0xE9C46A))
                    .child("★".repeat(m.rating as usize)),
            );
        }
        if m.label > 0 {
            row = row.child(collections::label_swatch(m.label, size * 0.7));
        }
        row
    }

    pub(crate) fn cull_loupe_view(&self, cx: &mut Context<Self>) -> Div {
        let Some(d) = self.import_dialog.as_ref() else {
            return div();
        };
        let i = self.cull.focus.unwrap_or(0);
        let Some(entry) = d.entries.get(i) else {
            return div();
        };
        let n = d.entries.len();
        let m = self.cull_mark(&entry.path);
        let name = entry
            .path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let small = self
            .review_thumbs
            .iter()
            .find(|p| p.entry_index == i)
            .and_then(|p| p.image.clone());
        let fit = self.cull.fit.get(&i).cloned().or(small);
        let slot = self.cull.stage_box.clone();
        let mut stage = div()
            .id("cull-stage")
            .relative()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .rounded(px(4.))
            .bg(rgb(0x0B0B0A))
            .child(crate::gallery_ui::meter(slot))
            .on_click(cx.listener(|this, ev: &ClickEvent, _, cx| {
                let b = this.cull.stage_box.get();
                let (w, h) = (
                    b.size.width.as_f32().max(1.),
                    b.size.height.as_f32().max(1.),
                );
                let p = ev.position();
                let u = ((p.x.as_f32() - b.origin.x.as_f32()) / w).clamp(0., 1.);
                let v = ((p.y.as_f32() - b.origin.y.as_f32()) / h).clamp(0., 1.);
                this.cull_toggle_zoom((u, v), cx);
            }));
        match (
            self.cull.zoom,
            self.cull.full.as_ref().filter(|(f, _)| *f == i),
        ) {
            (Some((u, v)), Some((_, full))) => {
                // 100%: one camera pixel per screen pixel, centered on the
                // clicked point.
                let sz = full.size(0);
                let scale = self.view_scale.get().max(1.);
                let (iw, ih) = (sz.width.0 as f32 / scale, sz.height.0 as f32 / scale);
                let b = self.cull.stage_box.get();
                let (bw, bh) = (b.size.width.as_f32(), b.size.height.as_f32());
                let left = (bw / 2. - u * iw).min(0.).max(bw - iw);
                let top = (bh / 2. - v * ih).min(0.).max(bh - ih);
                stage = stage.child(
                    div()
                        .absolute()
                        .left(px(if iw < bw { (bw - iw) / 2. } else { left }))
                        .top(px(if ih < bh { (bh - ih) / 2. } else { top }))
                        .w(px(iw))
                        .h(px(ih))
                        .child(img(ImageSource::Render(full.clone())).size_full()),
                );
            }
            _ => {
                if let Some(img_) = fit {
                    stage = stage.child(
                        img(ImageSource::Render(img_))
                            .absolute()
                            .top_0()
                            .left_0()
                            .size_full()
                            .object_fit(ObjectFit::Contain),
                    );
                }
            }
        }
        let zoom_note = match (self.cull.zoom, self.cull.full_loading) {
            (Some(_), Some(_)) => "100% · loading the camera JPEG…",
            (Some(_), None) => "100% of the camera JPEG · click or Z to fit",
            _ => "Fit · click or Z for 100%",
        };
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        div()
                            .id("cull-back")
                            .px(px(8.))
                            .py(px(4.))
                            .rounded(px(3.))
                            .border_1()
                            .border_color(border_control())
                            .text_size(sp(10.5))
                            .text_color(rgb(TEXT_SECONDARY))
                            .hover(|s| s.bg(rgb(bg_row_hover())))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.cull.loupe = false;
                                this.cull.zoom = None;
                                cx.notify();
                            }))
                            .child("◂ Grid"),
                    )
                    .child(
                        div()
                            .text_size(sp(11.5))
                            .text_color(rgb(TEXT_PRIMARY))
                            .child(name),
                    )
                    .child(Self::cull_badges(m, 12.))
                    .child(div().flex_1())
                    .when(entry.previously_imported, |dd| {
                        dd.child(
                            div()
                                .text_size(sp(10.))
                                .text_color(rgb(TEXT_DIMMER))
                                .child("ALREADY IMPORTED"),
                        )
                    })
                    .when(!entry.previously_imported, |dd| {
                        dd.child(
                            div()
                                .text_size(sp(10.))
                                .text_color(rgb(0xE0B870))
                                .child("NOT IMPORTED YET"),
                        )
                    })
                    .child(
                        div()
                            .text_size(sp(10.5))
                            .text_color(rgb(TEXT_DIM))
                            .child(format!("{} / {n}", i + 1)),
                    ),
            )
            .child(stage)
            .child(
                div()
                    .flex()
                    .gap(px(14.))
                    .text_size(sp(10.))
                    .text_color(rgb(TEXT_DIM))
                    .child(zoom_note)
                    .child(div().flex_1())
                    .child(
                        "← → move · 0–5 rate · P X U flag · 6–9 label · A auto-advance · G grid",
                    ),
            )
    }
}
