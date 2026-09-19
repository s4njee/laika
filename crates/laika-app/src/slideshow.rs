//! V20: full-screen slideshow and the Loupe info overlay.
//!
//! A show plays a multi-photo selection, or everything visible (folder,
//! album, filters). Each slide appears from its cached 2048 preview at
//! once, then swaps to a screen-resolution render from the full pipeline
//! (the fresh 1:1 cache when it matches, else a GPU render) when that is
//! sharper. Only the slideshow draws while it plays; rating and flag keys
//! land on the slide on screen; Esc returns to Loupe on that photo.

use std::collections::BTreeSet;
use std::time::Duration;

use laika_core::slideshow::{self as ss, InfoFields, LoupeInfo, SlideshowPrefs};

use super::*;

/// How long the controls stay after the pointer stops.
const CONTROLS_MS: u64 = 2500;
/// A slide waits this long at most for the next preview before moving on
/// to a placeholder (a missing cache must never stall the show).
const WAIT_FOR_NEXT_MS: u64 = 2500;

pub(crate) struct Show {
    ids: Vec<i64>,
    idx: usize,
    playing: bool,
    /// When the current slide started counting toward the interval.
    shown_at: Instant,
    /// Outgoing slide, fading out over the incoming one.
    fade_from: Option<(Arc<RenderImage>, Instant)>,
    /// Non-looping show reached its last slide while playing.
    ended: bool,
    entered_fullscreen: bool,
    controls_until: Instant,
    controls_shown: bool,
    toast: Option<(String, Instant)>,
    return_selection: BTreeSet<i64>,
    token: u64,
}

#[derive(Default)]
pub(crate) struct SlideState {
    pub prefs: SlideshowPrefs,
    pub show: Option<Show>,
    token: u64,
    /// Screen-resolution renders for the slides around the current one.
    hi: HashMap<i64, Arc<RenderImage>>,
    hi_loading: Option<i64>,
    hi_failed: HashSet<i64>,
    /// Physical window size, refreshed every slideshow frame.
    screen_px: (u32, u32),
}

impl SlideState {
    pub fn active(&self) -> bool {
        self.show.is_some()
    }
}

/// Fit `(w, h)` inside `(bw, bh)`, never enlarging.
fn fit_within(w: u32, h: u32, bw: u32, bh: u32) -> (u32, u32) {
    if w == 0 || h == 0 || bw == 0 || bh == 0 {
        return (w, h);
    }
    let s = (bw as f64 / w as f64).min(bh as f64 / h as f64).min(1.);
    (
        ((w as f64 * s).round() as u32).max(1),
        ((h as f64 * s).round() as u32).max(1),
    )
}

/// RGBA pixels → a GPUI image no larger than the screen.
fn screen_image(rgba: Vec<u8>, w: u32, h: u32, screen: (u32, u32)) -> Option<Arc<RenderImage>> {
    let buf = image::RgbaImage::from_raw(w, h, rgba)?;
    let (nw, nh) = fit_within(w, h, screen.0, screen.1);
    let buf = if (nw, nh) == (w, h) {
        buf
    } else {
        image::imageops::resize(&buf, nw, nh, image::imageops::FilterType::Triangle)
    };
    let mut px = buf.into_raw();
    rgba_to_bgra_in_place(&mut px);
    let buf = image::RgbaImage::from_raw(nw, nh, px)?;
    Some(Arc::new(RenderImage::new(smallvec::smallvec![
        image::Frame::new(buf)
    ])))
}

impl Laika {
    // ---- Loupe info overlay --------------------------------------------------

    pub(crate) fn info_fields(p: &DbPhoto) -> InfoFields {
        InfoFields {
            filename: p.filename.clone(),
            captured_at: p.captured_at.clone(),
            width: p.width,
            height: p.height,
            camera: p.camera.clone(),
            lens: p.lens.clone(),
            focal: p.focal_mm.clone(),
            aperture: p.aperture.clone(),
            shutter: p.shutter.clone(),
            iso: p.iso.clone(),
            rating: p.rating,
            picked: p.picked,
            rejected: p.rejected,
            title: p.title.clone(),
        }
    }

    /// The Loupe's info line for the primary, if the overlay is on and
    /// the template produces anything.
    pub(crate) fn loupe_info_line(&self) -> Option<String> {
        let template = match self.loupe_info {
            LoupeInfo::None => return None,
            LoupeInfo::File => &self.info_file_line,
            LoupeInfo::Exposure => &self.info_exposure_line,
        };
        let p = self.primary_photo()?;
        let line = ss::info_line(template, &Self::info_fields(p));
        (!line.is_empty()).then_some(line)
    }

    pub(crate) fn loupe_info_el(&self) -> Option<Div> {
        let line = self.loupe_info_line()?;
        Some(
            div()
                .absolute()
                .left(px(36.))
                .top(px(34.))
                .max_w(px(640.))
                .px(px(8.))
                .py(px(5.))
                .rounded(px(4.))
                .bg(rgba(0x0000008C))
                .font_family(SANS)
                .text_size(sp(12.))
                .text_color(rgba(0xFFFFFFE6))
                .truncate()
                .child(line),
        )
    }

    /// `I`: none → file line → exposure line.
    pub(crate) fn cycle_loupe_info(&mut self, cx: &mut Context<Self>) {
        self.loupe_info = self.loupe_info.cycle();
        self.save_cell_prefs();
        let name = match self.loupe_info {
            LoupeInfo::None => "off",
            LoupeInfo::File => "file line",
            LoupeInfo::Exposure => "exposure line",
        };
        self.status_note = if self.view == ViewMode::Loupe {
            format!("Loupe info: {name}")
        } else {
            format!("Loupe info: {name} (shows in Loupe, E)")
        };
        cx.notify();
    }

    // ---- lifecycle -------------------------------------------------------------

    /// ⌘Enter / Slideshow button: play the selection, or everything visible.
    pub(crate) fn start_slideshow(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.slides.active() {
            return;
        }
        let ordered = self.ordered_ids();
        let (ids, start) = ss::slide_scope(&ordered, &self.state.selection, self.state.primary);
        if ids.is_empty() {
            self.status_note = "nothing to play — no visible photos".to_string();
            cx.notify();
            return;
        }
        self.close_modals(cx);
        if self.field.is_some() {
            self.revert_field();
            self.defocus_field();
        }
        if self.crop_open {
            self.cancel_crop(cx);
        }
        self.flush_saves();
        self.context_menu = None;
        self.tooltip = None;
        self.hover_tip.set(None);
        let entered_fullscreen = !window.is_fullscreen();
        if entered_fullscreen {
            window.toggle_fullscreen();
        }
        self.slides.token = self.slides.token.wrapping_add(1);
        let token = self.slides.token;
        let now = Instant::now();
        self.slides.hi_failed.clear();
        self.slides.show = Some(Show {
            ids,
            idx: start,
            playing: true,
            shown_at: now,
            fade_from: None,
            ended: false,
            entered_fullscreen,
            controls_until: now + Duration::from_millis(CONTROLS_MS),
            controls_shown: true,
            toast: None,
            return_selection: self.state.selection.clone(),
            token,
        });
        self.slide_preload(cx);
        cx.spawn(async move |entity, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                let alive = entity
                    .update(cx, |this, cx| this.slideshow_tick(token, cx))
                    .unwrap_or(false);
                if !alive {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }

    /// Esc: back to Loupe on the photo that was showing. A multi-photo
    /// selection that contains it survives.
    pub(crate) fn exit_slideshow(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(show) = self.slides.show.take() else {
            return;
        };
        self.slides.token = self.slides.token.wrapping_add(1);
        if show.entered_fullscreen && window.is_fullscreen() {
            window.toggle_fullscreen();
        }
        for (_, image) in self.slides.hi.drain() {
            self.stale.push(image);
        }
        if let Some((image, _)) = show.fade_from {
            self.stale.push(image);
        }
        self.slides.hi_loading = None;
        let id = show.ids[show.idx];
        if self.find(id).is_some() {
            self.select_navigate(id, SelectMode::Set, cx);
            if show.return_selection.len() > 1 && show.return_selection.contains(&id) {
                self.state.selection = show.return_selection;
            }
        }
        self.state.active_module = Module::Library;
        self.view = ViewMode::Loupe;
        self.prev_view = ViewMode::Loupe;
        self.publish_open = false;
        self.wb_pick = false;
        cx.notify();
    }

    /// Timer: advance on the interval, expire controls and toasts.
    /// Returns false once this show is over (the loop stops).
    fn slideshow_tick(&mut self, token: u64, cx: &mut Context<Self>) -> bool {
        let interval = Duration::from_secs(self.slides.prefs.interval_secs.max(1) as u64);
        let looped = self.slides.prefs.looped;
        let Some(show) = self.slides.show.as_mut() else {
            return false;
        };
        if show.token != token {
            return false;
        }
        let now = Instant::now();
        let mut dirty = false;
        if show.controls_shown && now >= show.controls_until {
            show.controls_shown = false;
            dirty = true;
        }
        if show.toast.as_ref().is_some_and(|(_, until)| now >= *until) {
            show.toast = None;
            dirty = true;
        }
        let current = show.ids[show.idx];
        let next = ss::step(show.idx, 1, show.ids.len(), looped).map(|i| show.ids[i]);
        let (playing, ended, shown_at) = (show.playing, show.ended, show.shown_at);
        if playing && !ended {
            if !self.large.contains_key(&current) && now.duration_since(shown_at) < interval {
                // Count the interval from when the photo actually appears.
                if let Some(show) = self.slides.show.as_mut() {
                    show.shown_at = now;
                }
            } else if now.duration_since(shown_at) >= interval {
                let next_ready = next.is_none_or(|n| self.large.contains_key(&n));
                let waited = now.duration_since(shown_at)
                    >= interval + Duration::from_millis(WAIT_FOR_NEXT_MS);
                if next_ready || waited {
                    self.slide_step(1, false, cx);
                    dirty = false;
                }
            }
        }
        if dirty {
            cx.notify();
        }
        true
    }

    /// Move one slide. `manual` steps reset the interval and never end a
    /// show; at a non-looping edge they only say so.
    fn slide_step(&mut self, dir: i32, manual: bool, cx: &mut Context<Self>) {
        let prefs = self.slides.prefs;
        let Some(show) = self.slides.show.as_ref() else {
            return;
        };
        let len = show.ids.len();
        match ss::step(show.idx, dir, len, prefs.looped) {
            Some(i) if i != show.idx || len == 1 => {
                let outgoing = self.slide_image(show.ids[show.idx]);
                let now = Instant::now();
                let show = self.slides.show.as_mut().expect("checked above");
                if let Some((old, _)) = show.fade_from.take() {
                    self.stale.push(old);
                }
                show.fade_from = if prefs.fade && i != show.idx {
                    outgoing.map(|img| (img, now))
                } else {
                    None
                };
                show.idx = i;
                show.shown_at = now;
                show.ended = false;
                self.slide_preload(cx);
            }
            _ => {
                let now = Instant::now();
                let show = self.slides.show.as_mut().expect("checked above");
                if manual {
                    let edge = if dir > 0 { "last" } else { "first" };
                    show.toast = Some((
                        format!("{edge} photo · loop is off"),
                        now + Duration::from_millis(1400),
                    ));
                    show.shown_at = now;
                } else {
                    show.ended = true;
                    show.playing = false;
                    show.controls_shown = true;
                    show.controls_until = now;
                    show.toast = Some((
                        "End of slideshow · Space plays again · Esc exits".to_string(),
                        now + Duration::from_millis(4000),
                    ));
                }
            }
        }
        cx.notify();
    }

    fn slide_toggle_play(&mut self, cx: &mut Context<Self>) {
        let Some(show) = self.slides.show.as_mut() else {
            return;
        };
        let now = Instant::now();
        if show.ended {
            // Play again from the top.
            show.ended = false;
            show.playing = true;
            show.shown_at = now;
            if show.idx != 0 {
                show.idx = 0;
                self.slide_preload(cx);
            }
        } else {
            show.playing = !show.playing;
            show.shown_at = now;
            show.toast = Some((
                if show.playing { "Playing" } else { "Paused" }.to_string(),
                now + Duration::from_millis(900),
            ));
        }
        cx.notify();
    }

    fn slide_current(&self) -> Option<i64> {
        self.slides.show.as_ref().map(|s| s.ids[s.idx])
    }

    /// Best image on hand for a slide: the screen render, else the 2048.
    fn slide_image(&self, id: i64) -> Option<Arc<RenderImage>> {
        self.slides
            .hi
            .get(&id)
            .or_else(|| self.large.get(&id))
            .cloned()
    }

    /// Previews around the current slide, and the sharper render for the
    /// current slide then the next. Renders outside that window retire.
    fn slide_preload(&mut self, cx: &mut Context<Self>) {
        let looped = self.slides.prefs.looped;
        let Some(show) = self.slides.show.as_ref() else {
            return;
        };
        let len = show.ids.len();
        let at = |d: i32| ss::step(show.idx, d, len, looped).map(|i| show.ids[i]);
        let cur = show.ids[show.idx];
        let (next, next2, prev) = (at(1), at(2), at(-1));
        for id in [Some(cur), next, prev, next2].into_iter().flatten() {
            self.kick_large_load(id, cx);
        }
        let keep: HashSet<i64> = [Some(cur), next, prev].into_iter().flatten().collect();
        let drop: Vec<i64> = self
            .slides
            .hi
            .keys()
            .copied()
            .filter(|id| !keep.contains(id))
            .collect();
        for id in drop {
            if let Some(image) = self.slides.hi.remove(&id) {
                self.stale.push(image);
            }
        }
        for id in [Some(cur), next].into_iter().flatten() {
            if self.slide_hi_wanted(id) {
                self.kick_slide_hi(id, cx);
                break;
            }
        }
    }

    /// Worth a full render: a still whose on-screen size beats the 2048.
    fn slide_hi_wanted(&self, id: i64) -> bool {
        if self.slides.hi.contains_key(&id) || self.slides.hi_failed.contains(&id) {
            return false;
        }
        let Some(p) = self.find(id) else {
            return false;
        };
        if laika_raw::media_kind(std::path::Path::new(&p.path)) == Some(laika_raw::MediaKind::Video)
        {
            return false;
        }
        let (sw, sh) = self.slides.screen_px;
        let geom = self.committed_render_geom(id);
        let (w, h) = laika_develop::crop_target(p.width, p.height, geom.rect, geom.rotation);
        let (dw, dh) = fit_within(w, h, sw, sh);
        dw.max(dh) > laika_raw::preview::PREVIEW_LARGE + 64
    }

    fn kick_slide_hi(&mut self, id: i64, cx: &mut Context<Self>) {
        if self.slides.hi_loading.is_some() {
            return;
        }
        let Some(photo) = self.find(id).cloned() else {
            return;
        };
        let token = self.slides.token;
        let screen = self.slides.screen_px;
        let params = if Some(id) == self.state.primary {
            self.values
        } else {
            self.state
                .edits
                .get(&id)
                .map(|e| e.params)
                .unwrap_or_else(edit::defaults)
        };
        let values = self.effective_values(id, params);
        let geom = self.committed_render_geom(id);
        let (ew, eh) =
            laika_develop::crop_target(photo.width, photo.height, geom.rect, geom.rotation);
        // V09: a fresh 1:1 file serves without any decode.
        let dir = self.cache_dir.join(&photo.blake3);
        let cached_11 = std::fs::read(dir.join("preview-11.json"))
            .ok()
            .and_then(|b| serde_json::from_slice::<laika_core::catalog::DetailKey>(&b).ok())
            .filter(|key| {
                laika_core::catalog::detail_key_matches(
                    key,
                    &values,
                    geom.rect,
                    geom.angle_rad.to_degrees(),
                    geom.flip_h,
                    geom.flip_v,
                    geom.rotation,
                    0.,
                    ew,
                    eh,
                ) && key.warp_matches(&geom.warp)
            })
            .map(|_| dir.join("preview-11.jpg"));
        let renderer = if cached_11.is_some() || self.offline.contains(&id) {
            None
        } else if self.ensure_dev(cx) {
            self.dev.as_ref().map(|d| d.renderer.clone())
        } else {
            None
        };
        if cached_11.is_none() && renderer.is_none() {
            self.slides.hi_failed.insert(id);
            return;
        }
        self.slides.hi_loading = Some(id);
        let path = PathBuf::from(&photo.path);
        cx.spawn(async move |entity, cx| {
            let ready = cx
                .background_spawn(async move {
                    if let Some(jpg) = cached_11 {
                        let rgba = image::open(&jpg).map_err(|e| e.to_string())?.to_rgba8();
                        let (w, h) = rgba.dimensions();
                        return screen_image(rgba.into_raw(), w, h, screen)
                            .ok_or_else(|| "bad pixels".to_string());
                    }
                    let renderer = renderer.expect("checked above");
                    laika_raw::on_big_stack(move || {
                        let linear = if laika_raw::is_raw(&path) {
                            laika_raw::decode::decode(&path)?
                        } else {
                            laika_raw::decode::linear_from_raster(&path, None)?
                        };
                        let frame = renderer.render_export(linear, values, 0., geom)?;
                        screen_image(frame.rgba, frame.width, frame.height, screen)
                            .ok_or_else(|| "bad pixels".to_string())
                    })?
                })
                .await;
            entity
                .update(cx, |this, cx| {
                    if this.slides.token != token {
                        return;
                    }
                    this.slides.hi_loading = None;
                    match ready {
                        Ok(image) => {
                            this.slides.hi.insert(id, image);
                        }
                        Err(e) => {
                            eprintln!("[slideshow] full render failed for {id}: {e}");
                            this.slides.hi_failed.insert(id);
                        }
                    }
                    // Next in line (the slide after the current one).
                    this.slide_preload(cx);
                    cx.notify();
                })
                .ok();
        })
        .detach();
    }

    // ---- input -----------------------------------------------------------------

    /// Keys while a show plays. Everything is swallowed so no library
    /// command fires behind the full-screen view.
    pub(crate) fn slideshow_key(
        &mut self,
        key: &str,
        cmd: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match key {
            "escape" => self.exit_slideshow(window, cx),
            "enter" if cmd => self.exit_slideshow(window, cx),
            "space" => self.slide_toggle_play(cx),
            "right" | "down" | "pagedown" => self.slide_step(1, true, cx),
            "left" | "up" | "pageup" => self.slide_step(-1, true, cx),
            "0" | "1" | "2" | "3" | "4" | "5" if !cmd => {
                let n = key.parse::<u8>().unwrap_or(0);
                self.slide_mark(cx, move |this, cx| this.apply_rating(n, cx));
            }
            "6" | "7" | "8" | "9" if !cmd => {
                if let Some(l) = laika_core::labels::label_for_key(key) {
                    self.slide_mark(cx, move |this, cx| this.apply_label(l, cx));
                }
            }
            "p" if !cmd => self.slide_mark(cx, |this, cx| this.apply_flag(true, cx)),
            "x" if !cmd => self.slide_mark(cx, |this, cx| this.apply_flag(false, cx)),
            "u" if !cmd => self.slide_mark(cx, |this, cx| this.clear_flags(cx)),
            _ => {}
        }
    }

    /// Rating/flag on exactly the slide on screen (never the selection),
    /// with a toast so the key visibly landed.
    fn slide_mark(
        &mut self,
        cx: &mut Context<Self>,
        f: impl FnOnce(&mut Self, &mut Context<Self>),
    ) {
        let Some(id) = self.slide_current() else {
            return;
        };
        self.forced_targets = Some(vec![id]);
        f(self, cx);
        self.forced_targets = None;
        let note = self
            .find(id)
            .map(|p| {
                let stars = if p.rating == 0 {
                    "No rating".to_string()
                } else {
                    "★".repeat(p.rating as usize)
                };
                let flag = if p.picked {
                    " · Picked"
                } else if p.rejected {
                    " · Rejected"
                } else {
                    ""
                };
                let label = if p.label > 0 {
                    format!(" · {}", self.coll.names.name(p.label))
                } else {
                    String::new()
                };
                format!("{stars}{flag}{label}")
            })
            .unwrap_or_default();
        if let Some(show) = self.slides.show.as_mut() {
            show.toast = Some((note, Instant::now() + Duration::from_millis(1400)));
        }
        cx.notify();
    }

    fn slide_poke_controls(&mut self, cx: &mut Context<Self>) {
        if let Some(show) = self.slides.show.as_mut() {
            show.controls_until = Instant::now() + Duration::from_millis(CONTROLS_MS);
            if !show.controls_shown {
                show.controls_shown = true;
                cx.notify();
            }
        }
    }

    pub(crate) fn set_slide_prefs(&mut self, prefs: SlideshowPrefs, cx: &mut Context<Self>) {
        self.slides.prefs = prefs;
        self.save_cell_prefs();
        cx.notify();
    }

    // ---- view ------------------------------------------------------------------

    pub(crate) fn slideshow_view(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let scale = window.scale_factor();
        let vp = window.viewport_size();
        self.slides.screen_px = (
            (vp.width.as_f32() * scale).round() as u32,
            (vp.height.as_f32() * scale).round() as u32,
        );
        // Window-sized boxes in points (placement math needs real sizes).
        let (vw, vh) = (vp.width, vp.height);
        let prefs = self.slides.prefs;
        let now = Instant::now();
        // Finished fades retire here (the image may still be on screen
        // this frame, so it drops next frame via `stale`).
        let mut retired = None;
        if let Some(show) = self.slides.show.as_mut()
            && show
                .fade_from
                .as_ref()
                .is_some_and(|(_, t)| now.duration_since(*t).as_millis() as u64 >= ss::FADE_MS)
        {
            retired = show.fade_from.take();
        }
        if let Some((image, _)) = retired {
            self.stale.push(image);
        }
        let Some(show) = self.slides.show.as_ref() else {
            return div().id("slideshow");
        };
        let id = show.ids[show.idx];
        let photo = self.find(id).cloned();
        let incoming = self.slide_image(id);
        let fade_t = show.fade_from.as_ref().map(|(_, t)| {
            (now.duration_since(*t).as_millis() as f32 / ss::FADE_MS as f32).clamp(0., 1.)
        });
        if fade_t.is_some() {
            window.request_animation_frame();
        }
        let layer = |image: Arc<RenderImage>, opacity: f32| {
            // The same placement as the Loupe: an exact-aspect box at the
            // fitted size (letterboxed on black), filled by the image.
            let sz = image.size(0);
            let (iw, ih) = (sz.width.0.max(1) as f32, sz.height.0.max(1) as f32);
            let (bw, bh) = (vw.as_f32(), vh.as_f32());
            let s = (bw / iw).min(bh / ih);
            let (dw, dh) = (iw * s, ih * s);
            div()
                .absolute()
                .left(px((bw - dw) / 2.))
                .top(px((bh - dh) / 2.))
                .w(px(dw))
                .h(px(dh))
                .opacity(opacity)
                .child(img(ImageSource::Render(image)).size_full())
        };
        let mut stage = div().absolute().top_0().left_0().w(vw).h(vh);
        match (&show.fade_from, fade_t) {
            (Some((old, _)), Some(t)) => {
                // Cross-fade: incoming rises under the outgoing.
                if let Some(image) = incoming.clone() {
                    stage = stage.child(layer(image, t));
                }
                stage = stage.child(layer(old.clone(), 1. - t));
            }
            _ => {
                if let Some(image) = incoming.clone() {
                    stage = stage.child(layer(image, 1.));
                }
            }
        }
        if incoming.is_none() {
            stage = stage.child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .w(vw)
                    .h(vh)
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_size(sp(13.))
                    .text_color(rgba(0xFFFFFF66))
                    .child(
                        photo
                            .as_ref()
                            .map(|p| format!("{} · no preview yet", p.filename))
                            .unwrap_or_else(|| "photo removed".to_string()),
                    ),
            );
        }

        let controls = show.controls_shown || !show.playing;
        let caption = photo
            .as_ref()
            .filter(|_| prefs.caption)
            // V30: inside an album its caption wins over the photo title.
            .map(|p| {
                self.album_caption(p.id)
                    .unwrap_or_else(|| p.title.clone())
                    .trim()
                    .to_string()
            })
            .filter(|t| !t.is_empty());
        let caption_el = caption.map(|text| {
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom(px(if controls { 96. } else { 40. }))
                .flex()
                .justify_center()
                .child(
                    div()
                        .max_w(relative(0.7))
                        .px(px(14.))
                        .py(px(6.))
                        .rounded(px(6.))
                        .bg(rgba(0x00000073))
                        .text_size(sp(17.))
                        .text_color(rgba(0xFFFFFFEB))
                        .child(text),
                )
        });
        let toast_el = show.toast.as_ref().map(|(text, _)| {
            div()
                .absolute()
                .left_0()
                .right_0()
                .top(px(40.))
                .flex()
                .justify_center()
                .child(
                    div()
                        .px(px(14.))
                        .py(px(7.))
                        .rounded(px(18.))
                        .bg(rgba(0x000000B3))
                        .text_size(sp(14.))
                        .text_color(rgba(0xFFFFFFF2))
                        .child(text.clone()),
                )
        });
        let controls_el = controls.then(|| self.slideshow_controls(show, photo.as_ref(), cx));

        div()
            .id("slideshow")
            .absolute()
            .top_0()
            .left_0()
            .w(vw)
            .h(vh)
            .occlude()
            .bg(rgb(0x000000))
            .on_mouse_move(cx.listener(|this, _: &MouseMoveEvent, _, cx| {
                this.slide_poke_controls(cx);
            }))
            .child(stage)
            .children(caption_el)
            .children(toast_el)
            .children(controls_el)
    }

    fn slideshow_controls(
        &self,
        show: &Show,
        photo: Option<&DbPhoto>,
        cx: &mut Context<Self>,
    ) -> Div {
        let prefs = self.slides.prefs;
        let button = |id: &'static str, label: String, on: bool| {
            div()
                .id(id)
                .min_w(px(30.))
                .h(px(28.))
                .px(px(9.))
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(5.))
                .text_size(sp(12.5))
                .text_color(rgba(if on { 0xFFFFFFF2 } else { 0xFFFFFF99 }))
                .when(on, |d| d.bg(rgba(0xFFFFFF1F)))
                .hover(|s| s.bg(rgba(0xFFFFFF2E)))
                .child(label)
        };
        let sep = || div().w(px(1.)).h(px(18.)).bg(rgba(0xFFFFFF26));
        let name = photo.map(|p| p.filename.clone()).unwrap_or_default();
        let marks = photo
            .map(|p| {
                let mut s = "★".repeat(p.rating as usize);
                if p.picked {
                    s.push_str(" ⚑");
                } else if p.rejected {
                    s.push_str(" ✕");
                }
                s
            })
            .unwrap_or_default();
        let next_interval = {
            let i = ss::INTERVALS
                .iter()
                .position(|&s| s == prefs.interval_secs)
                .map(|i| (i + 1) % ss::INTERVALS.len())
                .unwrap_or(2);
            ss::INTERVALS[i]
        };
        div()
            .absolute()
            .left_0()
            .right_0()
            .bottom(px(32.))
            .flex()
            .justify_center()
            .child(
                div()
                    .id("slideshow-controls")
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .px(px(10.))
                    .py(px(6.))
                    .rounded(px(10.))
                    .bg(rgba(0x141414D9))
                    .border_1()
                    .border_color(rgba(0xFFFFFF1A))
                    .font_family(SANS)
                    .child(
                        button("slide-prev", "‹".to_string(), false)
                            .on_click(cx.listener(|this, _, _, cx| this.slide_step(-1, true, cx))),
                    )
                    .child(
                        button(
                            "slide-play",
                            if show.playing { "❚❚" } else { "▶" }.to_string(),
                            false,
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.slide_toggle_play(cx))),
                    )
                    .child(
                        button("slide-next", "›".to_string(), false)
                            .on_click(cx.listener(|this, _, _, cx| this.slide_step(1, true, cx))),
                    )
                    .child(sep())
                    .child(
                        div()
                            .px(px(4.))
                            .text_size(sp(12.))
                            .text_color(rgba(0xFFFFFFCC))
                            .child(format!("{} / {}", show.idx + 1, show.ids.len())),
                    )
                    .child(
                        div()
                            .max_w(px(260.))
                            .truncate()
                            .text_size(sp(12.))
                            .text_color(rgba(0xFFFFFF99))
                            .child(name),
                    )
                    .when(!marks.is_empty(), |d| {
                        d.child(
                            div()
                                .text_size(sp(12.))
                                .text_color(rgb(WARNING))
                                .child(marks),
                        )
                    })
                    .child(sep())
                    .child(
                        button("slide-interval", format!("{}s", prefs.interval_secs), false)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                let mut p = this.slides.prefs;
                                p.interval_secs = next_interval;
                                this.set_slide_prefs(p, cx);
                            })),
                    )
                    .child(
                        button("slide-loop", "Loop".to_string(), prefs.looped).on_click(
                            cx.listener(|this, _, _, cx| {
                                let mut p = this.slides.prefs;
                                p.looped = !p.looped;
                                this.set_slide_prefs(p, cx);
                            }),
                        ),
                    )
                    .child(
                        button("slide-fade", "Fade".to_string(), prefs.fade).on_click(cx.listener(
                            |this, _, _, cx| {
                                let mut p = this.slides.prefs;
                                p.fade = !p.fade;
                                this.set_slide_prefs(p, cx);
                            },
                        )),
                    )
                    .child(
                        button("slide-caption", "Caption".to_string(), prefs.caption).on_click(
                            cx.listener(|this, _, _, cx| {
                                let mut p = this.slides.prefs;
                                p.caption = !p.caption;
                                this.set_slide_prefs(p, cx);
                            }),
                        ),
                    )
                    .child(sep())
                    .child(
                        button("slide-exit", "Exit  Esc".to_string(), false).on_click(
                            cx.listener(|this, _, window, cx| this.exit_slideshow(window, cx)),
                        ),
                    ),
            )
    }

    /// Toolbar entry point shared by Grid, Loupe, Wall and Timeline.
    pub(crate) fn slideshow_button(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        div()
            .id("slideshow-start")
            .px(px(9.))
            .py(px(4.))
            .rounded(px(3.))
            .border_1()
            .border_color(border_control())
            .font_family(SANS)
            .text_size(sp(10.5))
            .text_color(rgb(TEXT_SECONDARY))
            .hover(|s| s.bg(rgb(bg_row_hover())))
            .on_hover(self.tip("Slideshow of the selection, or everything shown (⌘Enter)"))
            .on_click(cx.listener(|this, _, window, cx| this.start_slideshow(window, cx)))
            .child("▶ Slideshow")
    }

    /// Loupe toolbar toggle for the info overlay (same as `I`).
    pub(crate) fn loupe_info_button(&self, cx: &mut Context<Self>) -> Stateful<Div> {
        let on = self.loupe_info != LoupeInfo::None;
        div()
            .id("loupe-info")
            .px(px(9.))
            .py(px(4.))
            .rounded(px(3.))
            .border_1()
            .border_color(border_control())
            .font_family(SANS)
            .text_size(sp(10.5))
            .text_color(rgb(if on { TEXT_PRIMARY } else { TEXT_SECONDARY }))
            .when(on, |d| d.bg(rgb(bg_segment_active())))
            .hover(|s| s.bg(rgb(bg_row_hover())))
            .on_hover(self.tip("Info overlay: off · file · exposure (I)"))
            .on_click(cx.listener(|this, _, _, cx| this.cycle_loupe_info(cx)))
            .child(match self.loupe_info {
                LoupeInfo::None => "Info",
                LoupeInfo::File => "Info · file",
                LoupeInfo::Exposure => "Info · exposure",
            })
    }
}

#[cfg(test)]
mod tests {
    use super::fit_within;

    #[test]
    fn fit_never_enlarges_and_keeps_aspect() {
        assert_eq!(fit_within(6000, 4000, 3024, 1964), (2946, 1964));
        assert_eq!(fit_within(4000, 6000, 3024, 1964), (1309, 1964));
        assert_eq!(fit_within(1000, 800, 3024, 1964), (1000, 800));
        assert_eq!(fit_within(0, 0, 3024, 1964), (0, 0));
    }
}
