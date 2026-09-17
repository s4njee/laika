//! Laika Phase 0 spike (plan.md): Plex fonts with letter-spacing, the slider control, gradient
//! fills, and a wgpu develop pass shown through GPUI by readback.
//!
//! `laika-p0` opens the Develop-shaped window. `laika-p0 --bench 10` sweeps sliders every frame for
//! that many seconds, prints one JSON line of timings and quits. `--size 1800x1200` sets the
//! preview render size.

use std::borrow::Cow;
use std::cell::Cell;
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::StreamExt;
use gpui_kit::prelude::*;
use gpui_kit::*;

mod develop;
mod params;
mod tokens;
mod tracked;

use params::PARAMS;
use tokens::*;
use tracked::{tr, tracked};

const WARM_UP: Duration = Duration::from_secs(1);
const STATS_WINDOW: Duration = Duration::from_millis(750);

const PRESETS: [(&str, u32, u32, [f32; 4]); 7] = [
    // name, swatch from, swatch to, (temperature, contrast, vibrance, saturation)
    ("Neutral RAW", 0x8A8178, 0x2A2723, [5480., 0., 0., 0.]),
    ("Portra warm", 0xC99A6B, 0x3A2C21, [5480., 12., 18., 0.]),
    ("Ceremony low-light", 0x6E7C8C, 0x1B1F24, [4900., -10., 10., -8.]),
    ("Golden hour", 0xE0A85A, 0x40301C, [6600., 18., 30., 6.]),
    ("Mono contrast", 0xE4E0DA, 0x131211, [5480., 40., 0., -100.]),
    ("Reception tungsten", 0xC4795A, 0x2A1C16, [3600., 8., 12., 0.]),
    ("Editorial cool", 0x8FA3AE, 0x1E2428, [4700., 24., -10., -12.]),
];

const FILM: [(u32, u32); 8] = [
    (0xC99A6B, 0x2A1F18),
    (0x8FA3AE, 0x1C2226),
    (0xE0A85A, 0x3A2A18),
    (0x6E7C8C, 0x171B20),
    (0xB08A70, 0x241C16),
    (0xD8C2A0, 0x3A332A),
    (0x7A6A5C, 0x151210),
    (0xC4795A, 0x2A1C16),
];

#[derive(Default)]
struct Samples {
    gpu: Vec<f32>,
    copy: Vec<f32>,
    draw: Vec<f32>,
    present: Vec<f32>,
    interval: Vec<f32>,
    latency: Vec<f32>,
    previews: u32,
    last_present: Option<Instant>,
}

impl Samples {
    fn clear(&mut self) {
        let last = self.last_present;
        *self = Samples::default();
        self.last_present = last;
    }
}

fn pct(v: &[f32], p: f32) -> f32 {
    if v.is_empty() {
        return 0.;
    }
    let mut s = v.to_vec();
    s.sort_by(|a, b| a.partial_cmp(b).unwrap());
    s[((s.len() - 1) as f32 * p).round() as usize]
}

struct Bench {
    seconds: f32,
    started: Instant,
    samples: Samples,
}

struct Spike {
    values: [f32; 12],
    focus: FocusHandle,
    focused_param: usize,
    dragging: Option<usize>,
    track_bounds: Vec<Rc<Cell<Bounds<Pixels>>>>,
    canvas_bounds: Rc<Cell<Bounds<Pixels>>>,
    split: f32,
    split_before_hold: Option<f32>,
    preset: usize,

    renderer: develop::Renderer,
    adapter: SharedString,
    render_size: (u32, u32),
    skip_upload: bool,
    seq: u64,
    submitted: VecDeque<(u64, Instant)>,
    shown: Option<Arc<RenderImage>>,
    stale: Vec<Arc<RenderImage>>,
    histogram: [f32; 48],

    collector: FrameTimingCollector,
    live: Samples,
    live_started: Instant,
    summary: SharedString,
    bench: Option<Bench>,
    launched: Instant,
    first_preview: Option<Duration>,
}

impl Spike {
    fn submit(&mut self) {
        self.seq += 1;
        let now = Instant::now();
        self.submitted.push_back((self.seq, now));
        while self.submitted.len() > 120 {
            self.submitted.pop_front();
        }
        self.renderer.submit(develop::Job { values: self.values, split: self.split, seq: self.seq });
    }

    fn set_value(&mut self, i: usize, v: f32, cx: &mut Context<Self>) {
        let v = params::snap(i, v);
        if v != self.values[i] {
            self.values[i] = v;
            self.submit();
            cx.notify();
        }
    }

    fn drag_to(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(i) = self.dragging else { return };
        let b = self.track_bounds[i].get();
        if b.size.width <= px(0.) {
            return;
        }
        let frac = ((position.x - b.left()) / b.size.width).clamp(0., 1.);
        let d = PARAMS[i];
        self.set_value(i, d.min + frac * (d.max - d.min), cx);
    }

    fn receive(&mut self, frame: develop::Rendered, cx: &mut Context<Self>) {
        let now = Instant::now();
        while let Some(&(seq, at)) = self.submitted.front() {
            if seq > frame.seq {
                break;
            }
            self.submitted.pop_front();
            if seq == frame.seq {
                let ms = now.duration_since(at).as_secs_f32() * 1000.;
                self.live.latency.push(ms);
                if let Some(b) = self.bench.as_mut().filter(|b| b.started.elapsed() >= WARM_UP) {
                    b.samples.latency.push(ms);
                }
            }
        }
        self.first_preview.get_or_insert_with(|| self.launched.elapsed());
        self.live.gpu.push(frame.gpu_ms);
        self.live.copy.push(frame.copy_ms);
        self.live.previews += 1;
        if let Some(b) = self.bench.as_mut().filter(|b| b.started.elapsed() >= WARM_UP) {
            b.samples.gpu.push(frame.gpu_ms);
            b.samples.copy.push(frame.copy_ms);
            b.samples.previews += 1;
        }
        self.histogram = frame.histogram;

        let buffer = image::RgbaImage::from_raw(frame.width, frame.height, frame.bgra)
            .expect("buffer matches size");
        let image = Arc::new(RenderImage::new(smallvec::smallvec![image::Frame::new(buffer)]));
        // --skip-upload keeps the first image, so draw_ms can be compared with and without atlas uploads.
        if !self.skip_upload || self.shown.is_none() {
            if let Some(old) = self.shown.replace(image) {
                self.stale.push(old);
            }
        }
        cx.notify();
    }

    fn collect_timings(&mut self) {
        let steady = self.bench.as_ref().is_some_and(|b| b.started.elapsed() >= WARM_UP);
        for event in self.collector.collect_unseen() {
            match event {
                FrameEvent::Draw(t) => {
                    let ms = t.draw_duration().as_secs_f32() * 1000.;
                    self.live.draw.push(ms);
                    if steady {
                        self.bench.as_mut().unwrap().samples.draw.push(ms);
                    }
                }
                FrameEvent::Present(t) => {
                    let ms = t.present_end.duration_since(t.present_start).as_secs_f32() * 1000.;
                    self.live.present.push(ms);
                    if let Some(last) = self.live.last_present {
                        let iv = t.present_start.duration_since(last).as_secs_f32() * 1000.;
                        // Idle gaps aren't frame intervals.
                        if iv < 250. {
                            self.live.interval.push(iv);
                            if steady {
                                self.bench.as_mut().unwrap().samples.interval.push(iv);
                            }
                        }
                    }
                    self.live.last_present = Some(t.present_start);
                    if steady {
                        self.bench.as_mut().unwrap().samples.present.push(ms);
                    }
                }
            }
        }

        let elapsed = self.live_started.elapsed();
        if elapsed >= STATS_WINDOW && !self.live.interval.is_empty() {
            let s = &self.live;
            self.summary = format!(
                "preview {:.0} fps · gpu p50 {:.1} / p99 {:.1} ms · copy p50 {:.1} ms · latency p50 {:.1} ms · draw p50 {:.1} ms · present p50 {:.1} ms · frame interval p50 {:.1} / p99 {:.1} ms",
                s.previews as f32 / elapsed.as_secs_f32(),
                pct(&s.gpu, 0.5),
                pct(&s.gpu, 0.99),
                pct(&s.copy, 0.5),
                pct(&s.latency, 0.5),
                pct(&s.draw, 0.5),
                pct(&s.present, 0.5),
                pct(&s.interval, 0.5),
                pct(&s.interval, 0.99),
            )
            .into();
            self.live.clear();
            self.live_started = Instant::now();
        } else if elapsed >= STATS_WINDOW {
            self.live.clear();
            self.live_started = Instant::now();
        }
    }

    fn tick_bench(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(bench) = self.bench.as_ref() else { return };
        let elapsed = bench.started.elapsed();
        if elapsed.as_secs_f32() >= bench.seconds {
            let b = self.bench.take().unwrap();
            let steady = (elapsed - WARM_UP).as_secs_f32();
            let s = &b.samples;
            let stat = |name: &str, v: &[f32]| {
                format!(
                    "\"{name}\":{{\"n\":{},\"p50\":{:.2},\"p99\":{:.2},\"max\":{:.2}}}",
                    v.len(),
                    pct(v, 0.5),
                    pct(v, 0.99),
                    v.iter().cloned().fold(0., f32::max)
                )
            };
            println!(
                "{{\"spike\":\"p0\",\"adapter\":{:?},\"render_size\":\"{}x{}\",\"scale_factor\":{},\"first_preview_ms\":{:.0},\"preview_fps\":{:.1},\"upload\":{},{},{},{},{},{},{}}}",
                self.adapter.to_string(),
                self.render_size.0,
                self.render_size.1,
                window.scale_factor(),
                self.first_preview.unwrap_or_default().as_secs_f32() * 1000.,
                s.previews as f32 / steady,
                !self.skip_upload,
                stat("copy_ms", &s.copy),
                stat("gpu_ms", &s.gpu),
                stat("latency_ms", &s.latency),
                stat("draw_ms", &s.draw),
                stat("present_ms", &s.present),
                stat("frame_interval_ms", &s.interval),
            );
            cx.quit();
            return;
        }

        // A continuous two-handed drag: exposure and highlights sweep every frame.
        let t = elapsed.as_secs_f32();
        self.values[2] = params::snap(2, 0.35 + 1.5 * (t * 2.1).sin());
        self.values[4] = params::snap(4, -40. + 50. * (t * 1.3).cos());
        self.submit();
        cx.notify();
        cx.on_next_frame(window, |this, window, cx| this.tick_bench(window, cx));
    }

    // ---- view pieces ------------------------------------------------------------------------

    fn section_header(&self, text: &str, color: u32, window: &mut Window) -> Div {
        div().px(px(14.)).pt(px(14.)).pb(px(8.)).child(tr(
            text.to_uppercase(),
            MONO,
            FontWeight::MEDIUM,
            9.5,
            0.14,
            rgb(color),
            window,
        ))
    }

    fn top_bar(&self, window: &mut Window) -> Div {
        let tabs = ["LIBRARY", "DEVELOP", "MAP", "PUBLISH"];
        div()
            .h(px(46.))
            .flex_none()
            .flex()
            .items_center()
            .px(px(14.))
            .bg(rgb(BG_CHROME))
            .border_b_1()
            .border_color(hairline())
            .child(
                div()
                    .w(px(226.))
                    .flex()
                    .items_center()
                    .gap(px(9.))
                    .child(div().size(px(9.)).rounded_full().bg(rgb(ACCENT_LINE)))
                    .child(tr("LAIKA", MONO, FontWeight::SEMIBOLD, 13., 0.22, rgb(TEXT_PRIMARY), window)),
            )
            .child(div().flex().gap(px(2.)).children(tabs.iter().enumerate().map(|(i, name)| {
                let active = i == 1;
                div()
                    .px(px(13.))
                    .py(px(6.))
                    .rounded(px(4.))
                    .when(active, |d| d.bg(rgb(BG_TAB_ACTIVE)))
                    .child(tr(
                        *name,
                        SANS,
                        FontWeight::MEDIUM,
                        11.,
                        0.09,
                        rgb(if active { TEXT_PRIMARY } else { TEXT_MUTED }),
                        window,
                    ))
            })))
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .font_family(MONO)
                    .text_size(px(10.5))
                    .child(
                        div()
                            .text_color(rgb(TEXT_MUTED))
                            .child("DSC_4418.dng · edits stored locally · .xmp sidecar"),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .px(px(10.))
                            .py(px(5.))
                            .border_1()
                            .border_color(rgba(0xFFFFFF17))
                            .rounded(px(4.))
                            .text_color(rgb(TEXT_TERTIARY))
                            .child(div().size(px(6.)).rounded_full().bg(rgb(WARNING)))
                            .child("edit queued for sync"),
                    ),
            )
    }

    fn left_rail(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let (wordmark, wordmark_m) =
            tracked("LAIKA", MONO, FontWeight::SEMIBOLD, 13., 0.22, rgb(TEXT_PRIMARY), window);
        let (header, header_m) =
            tracked("TONE CURVE", MONO, FontWeight::MEDIUM, 9.5, 0.14, rgb(TEXT_DIM), window);
        let per_char = |text: &str, size: f32, em: f32, weight: FontWeight, color: u32| {
            div()
                .flex()
                .font_family(MONO)
                .font_weight(weight)
                .text_size(px(size))
                .text_color(rgb(color))
                .children(text.chars().map(move |c| div().mr(px(size * em)).child(c.to_string())))
        };
        let label = |s: &'static str| {
            div().w(px(58.)).font_family(MONO).text_size(px(9.)).text_color(rgb(TEXT_DIM)).child(s)
        };
        let row = || div().flex().items_center().h(px(22.)).px(px(14.));

        div()
            .w(px(210.))
            .flex_none()
            .flex()
            .flex_col()
            .bg(rgb(BG_PANEL))
            .border_r_1()
            .border_color(hairline())
            .child(self.section_header("Presets", TEXT_DIM, window))
            .child(div().flex().flex_col().gap(px(1.)).px(px(6.)).children(
                PRESETS.iter().enumerate().map(|(i, (name, from, to, _))| {
                    let active = i == self.preset;
                    div()
                        .id(("preset", i))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .px(px(8.))
                        .py(px(5.))
                        .rounded(px(3.))
                        .text_size(px(11.5))
                        .text_color(rgb(if active { TEXT_PRIMARY } else { TEXT_SECONDARY }))
                        .when(active, |d| d.bg(rgb(BG_ROW_ACTIVE)))
                        .when(!active, |d| d.hover(|s| s.bg(rgb(BG_ROW_HOVER))))
                        .on_click(cx.listener(move |this, _, _, cx| this.apply_preset(i, cx)))
                        .child(
                            div().w(px(18.)).h(px(12.)).rounded(px(1.)).bg(linear_gradient(
                                135.,
                                linear_color_stop(rgb(*from), 0.),
                                linear_color_stop(rgb(*to), 1.),
                            )),
                        )
                        .child(*name)
                })
            ))
            .child(self.section_header("Tracking check", ACCENT_LINE, window))
            .child(row().child(label("none")).child(
                div()
                    .font_family(MONO)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_size(px(13.))
                    .child("LAIKA"),
            ))
            .child(row().child(label("glyph")).child(wordmark))
            .child(row().child(label("per-char")).child(per_char(
                "LAIKA",
                13.,
                0.22,
                FontWeight::SEMIBOLD,
                TEXT_PRIMARY,
            )))
            .child(row().child(label("none")).child(
                div()
                    .font_family(MONO)
                    .font_weight(FontWeight::MEDIUM)
                    .text_size(px(9.5))
                    .text_color(rgb(TEXT_DIM))
                    .child("TONE CURVE"),
            ))
            .child(row().child(label("glyph")).child(header))
            .child(row().child(label("per-char")).child(per_char(
                "TONE CURVE",
                9.5,
                0.14,
                FontWeight::MEDIUM,
                TEXT_DIM,
            )))
            .child(
                div()
                    .px(px(14.))
                    .pt(px(6.))
                    .font_family(MONO)
                    .text_size(px(9.))
                    .text_color(rgb(TEXT_DIMMER))
                    .child(format!(
                        "widths {:.1}px · {:.1}px",
                        f32::from(wordmark_m.width),
                        f32::from(header_m.width)
                    )),
            )
            .child(div().flex_1())
            .child(
                div()
                    .border_t_1()
                    .border_color(hairline())
                    .px(px(14.))
                    .py(px(12.))
                    .font_family(MONO)
                    .text_size(px(10.))
                    .line_height(relative(1.6))
                    .text_color(rgb(TEXT_DIM))
                    .child("non-destructive")
                    .child("original untouched on disk"),
            )
    }

    fn apply_preset(&mut self, i: usize, cx: &mut Context<Self>) {
        self.preset = i;
        let [temp, contrast, vib, sat] = PRESETS[i].3;
        self.values[0] = temp;
        self.values[3] = contrast;
        self.values[10] = vib;
        self.values[11] = sat;
        self.submit();
        cx.notify();
    }

    fn center(&self, window: &mut Window) -> Div {
        // Letterbox a 3:2 rect into last frame's canvas bounds.
        let area = self.canvas_bounds.get();
        let (aw, ah) = (f32::from(area.size.width), f32::from(area.size.height));
        let (iw, ih) = if aw / ah.max(1.) > 1.5 { (ah * 1.5, ah) } else { (aw, aw / 1.5) };
        let (ix, iy) = ((aw - iw) / 2., (ah - ih) / 2.);
        let cell = self.canvas_bounds.clone();

        let corner = |text: &'static str, window: &mut Window| {
            tr(text, MONO, FontWeight::MEDIUM, 9.5, 0.12, rgba(0xFFFFFF9E), window)
        };

        let photo = div()
            .absolute()
            .left(px(ix))
            .top(px(iy))
            .w(px(iw))
            .h(px(ih))
            .bg(rgb(BG_APP))
            .when_some(self.shown.clone(), |d, image| {
                d.child(img(ImageSource::Render(image)).size_full().object_fit(ObjectFit::Fill))
            })
            .when(self.split > 0. && self.split < 1., |d| {
                d.child(
                    div()
                        .absolute()
                        .top_0()
                        .bottom_0()
                        .left(relative(self.split))
                        .w(px(1.))
                        .bg(rgba(0x00C227BF)),
                )
            })
            .child(div().absolute().left(px(14.)).top(px(12.)).child(corner("BEFORE", window)))
            .child(div().absolute().right(px(14.)).top(px(12.)).child(corner("AFTER", window)))
            .child(
                div()
                    .absolute()
                    .right(px(12.))
                    .bottom(px(12.))
                    .px(px(8.))
                    .py(px(4.))
                    .bg(rgba(0x0000008C))
                    .font_family(MONO)
                    .text_size(px(9.5))
                    .text_color(rgb(TEXT_SECONDARY))
                    .child(format!("6048 × 4024 · preview {}×{}", self.render_size.0, self.render_size.1)),
            );

        let segment = |text: &'static str, active: bool, window: &mut Window| {
            div()
                .px(px(9.))
                .py(px(4.))
                .rounded(px(3.))
                .when(active, |d| d.bg(rgb(BG_SEGMENT_ACTIVE)))
                .child(tr(
                    text,
                    MONO,
                    FontWeight::MEDIUM,
                    10.5,
                    0.06,
                    rgb(if active { TEXT_PRIMARY } else { TEXT_MUTED }),
                    window,
                ))
        };

        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .bg(rgb(BG_CANVAS))
            .child(
                div().flex_1().min_h_0().p(px(26.)).child(
                    div()
                        .relative()
                        .size_full()
                        .child(canvas(move |b, _, _| cell.set(b), |_, _, _, _| {}).absolute().size_full())
                        .child(photo),
                ),
            )
            .child(
                div()
                    .h(px(38.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(4.))
                    .px(px(16.))
                    .bg(rgb(BG_CHROME))
                    .border_t_1()
                    .border_color(hairline())
                    .child(segment("CROP", true, window))
                    .child(segment("HEAL", false, window))
                    .child(segment("MASK", false, window))
                    .child(segment("RED-EYE", false, window))
                    .child(div().mx(px(10.)).w(px(1.)).h(px(18.)).bg(rgba(0xFFFFFF14)))
                    .child(
                        div()
                            .font_family(MONO)
                            .text_size(px(10.5))
                            .text_color(rgb(TEXT_DIM))
                            .child("FIT · 1:1 · 2:1"),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .font_family(MONO)
                            .text_size(px(10.5))
                            .text_color(rgb(TEXT_DIM))
                            .child("\\ before/after · Y split · ←→ nudge · ⇧ ×10"),
                    ),
            )
            .child(
                div()
                    .h(px(96.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .px(px(10.))
                    .bg(rgb(BG_APP))
                    .border_t_1()
                    .border_color(hairline())
                    .overflow_hidden()
                    .children((0..16).map(|i| {
                        let (a, b) = FILM[i % FILM.len()];
                        div()
                            .relative()
                            .flex_none()
                            .w(px(106.))
                            .h(px(72.))
                            .rounded(px(2.))
                            .border_1()
                            .border_color::<Hsla>(if i == 7 { rgb(ACCENT_LINE).into() } else { rgba(0xFFFFFF14).into() })
                            .bg(linear_gradient(160., linear_color_stop(rgb(a), 0.), linear_color_stop(rgb(b), 1.)))
                            .child(
                                div()
                                    .absolute()
                                    .left(px(5.))
                                    .bottom(px(3.))
                                    .font_family(MONO)
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_size(px(8.5))
                                    .text_color(rgba(0xFFFFFFBF))
                                    .child(format!("_{}", 4411 + i)),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .right(px(5.))
                                    .bottom(px(3.))
                                    .text_size(px(8.5))
                                    .text_color(rgb(WARNING))
                                    .child("★★★"),
                            )
                    })),
            )
    }

    fn slider(&self, i: usize, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let d = PARAMS[i];
        let v = self.values[i];
        let modified = params::is_modified(i, v);
        let frac = ((v - d.min) / (d.max - d.min)).clamp(0., 1.);
        let accent = rgb(if modified { ACCENT_LINE } else { TEXT_PRIMARY });
        let cell = self.track_bounds[i].clone();
        let focused = i == self.focused_param;

        div()
            .id(("slider", i))
            .flex()
            .flex_col()
            .gap(px(5.))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                    window.focus(&this.focus, cx);
                    this.focused_param = i;
                    if ev.click_count >= 2 {
                        this.dragging = None;
                        this.set_value(i, PARAMS[i].default, cx);
                    } else {
                        this.dragging = Some(i);
                        this.drag_to(ev.position, cx);
                    }
                    cx.notify();
                }),
            )
            .child(
                div()
                    .flex()
                    .justify_between()
                    .items_center()
                    .child(
                        div()
                            .text_size(px(11.))
                            .text_color(rgb(if focused { TEXT_PRIMARY } else { TEXT_TERTIARY }))
                            .child(d.label),
                    )
                    .child(
                        div()
                            .font_family(MONO)
                            .text_size(px(10.5))
                            .text_color(accent)
                            .child(params::format(i, v)),
                    ),
            )
            .child(
                div()
                    .relative()
                    .h(px(10.))
                    .child(canvas(move |b, _, _| cell.set(b), |_, _, _, _| {}).absolute().size_full())
                    .child(div().absolute().left_0().right_0().top(px(4.)).h(px(2.)).bg(rgb(TRACK)))
                    .child(div().absolute().left(relative(0.5)).top(px(2.)).w(px(1.)).h(px(6.)).bg(rgb(DETENT)))
                    .child(
                        div()
                            .absolute()
                            .left(relative(frac))
                            .top_0()
                            .ml(px(-5.))
                            .size(px(10.))
                            .rounded_full()
                            .bg(accent),
                    ),
            )
    }

    fn right_rail(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let collapsed = |name: &'static str, window: &mut Window| {
            div()
                .flex()
                .items_center()
                .justify_between()
                .px(px(14.))
                .py(px(11.))
                .border_t_1()
                .border_color(hairline())
                .child(tr(name, MONO, FontWeight::MEDIUM, 9.5, 0.14, rgb(TEXT_DIM), window))
                .child(div().font_family(MONO).text_size(px(11.)).text_color(rgb(TEXT_DIM)).child("+"))
        };

        div()
            .w(px(306.))
            .flex_none()
            .flex()
            .flex_col()
            .bg(rgb(BG_PANEL))
            .border_l_1()
            .border_color(hairline())
            .child(
                div().px(px(14.)).py(px(12.)).border_b_1().border_color(hairline()).child(
                    div()
                        .h(px(78.))
                        .p(px(5.))
                        .flex()
                        .items_end()
                        .gap(px(1.))
                        .bg(rgb(BG_WELL))
                        .border_1()
                        .border_color(hairline())
                        .rounded(px(3.))
                        .children(self.histogram.iter().map(|h| {
                            div()
                                .flex_1()
                                .h(relative(h.max(0.02)))
                                .rounded_t(px(1.))
                                .bg(rgb(HIST_BAR))
                        })),
                ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .pr(px(14.))
                    .child(self.section_header("Basic", ACCENT_LINE, window))
                    .child(
                        div()
                            .id("reset")
                            .font_family(MONO)
                            .text_size(px(10.))
                            .text_color(rgb(TEXT_DIM))
                            .hover(|s| s.text_color(rgb(TEXT_SECONDARY)))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.values = PARAMS.map(|d| d.default);
                                this.submit();
                                cx.notify();
                            }))
                            .child("reset"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .px(px(14.))
                    .pb(px(14.))
                    .children((0..12).map(|i| self.slider(i, cx))),
            )
            .child(collapsed("TONE CURVE", window))
            .child(collapsed("COLOR MIX", window))
            .child(collapsed("DETAIL", window))
            .child(collapsed("OPTICS", window))
            .child(div().flex_1())
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .px(px(14.))
                    .py(px(12.))
                    .border_t_1()
                    .border_color(hairline())
                    .text_size(px(11.))
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .justify_center()
                            .py(px(8.))
                            .rounded(px(3.))
                            .border_1()
                            .border_color(border_strong())
                            .text_color(rgb(TEXT_SECONDARY))
                            .font_weight(FontWeight::MEDIUM)
                            .child("Copy settings"),
                    )
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .justify_center()
                            .py(px(8.))
                            .rounded(px(3.))
                            .bg(rgb(ACCENT_FILL))
                            .text_color(rgb(ACCENT_ON_FILL))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child("Paste to 14"),
                    ),
            )
    }

    fn status_bar(&self) -> Div {
        div()
            .h(px(28.))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(16.))
            .px(px(14.))
            .bg(rgb(BG_CHROME))
            .border_t_1()
            .border_color(hairline())
            .font_family(MONO)
            .text_size(px(10.))
            .text_color(rgb(TEXT_DIM))
            .child(div().text_color(rgb(ACCENT_LINE)).child(if self.summary.is_empty() {
                SharedString::from("drag a slider to measure")
            } else {
                self.summary.clone()
            }))
            .child(div().flex_1())
            .child(self.adapter.clone())
    }
}

impl Render for Spike {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        for old in self.stale.drain(..) {
            window.drop_image(old).ok();
        }
        self.collect_timings();

        let entity = cx.entity();
        let drag_listener = canvas(
            |_, _, _| {},
            move |_, _, window, _| {
                let e = entity.clone();
                window.on_mouse_event(move |ev: &MouseMoveEvent, phase, _, cx| {
                    if phase != DispatchPhase::Bubble {
                        return;
                    }
                    e.update(cx, |this, cx| {
                        if this.dragging.is_none() {
                            return;
                        }
                        if ev.pressed_button == Some(MouseButton::Left) {
                            this.drag_to(ev.position, cx);
                        } else {
                            this.dragging = None;
                        }
                    });
                });
                let e = entity.clone();
                window.on_mouse_event(move |_: &MouseUpEvent, phase, _, cx| {
                    if phase == DispatchPhase::Bubble {
                        e.update(cx, |this, _| this.dragging = None);
                    }
                });
            },
        )
        .absolute()
        .size_full();

        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(BG_CHROME))
            .text_color(rgb(TEXT_PRIMARY))
            .font_family(SANS)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _, cx| {
                let i = this.focused_param;
                let mult = if ev.keystroke.modifiers.shift { 10. } else { 1. };
                match ev.keystroke.key.as_str() {
                    "left" => this.set_value(i, this.values[i] - PARAMS[i].step * mult, cx),
                    "right" => this.set_value(i, this.values[i] + PARAMS[i].step * mult, cx),
                    "up" => {
                        this.focused_param = i.saturating_sub(1);
                        cx.notify();
                    }
                    "down" => {
                        this.focused_param = (i + 1).min(11);
                        cx.notify();
                    }
                    "y" => {
                        this.split = if this.split > 0. { 0. } else { 0.38 };
                        this.submit();
                        cx.notify();
                    }
                    "\\" if this.split_before_hold.is_none() => {
                        this.split_before_hold = Some(this.split);
                        this.split = 1.;
                        this.submit();
                        cx.notify();
                    }
                    _ => {}
                }
            }))
            .on_key_up(cx.listener(|this, ev: &KeyUpEvent, _, cx| {
                if ev.keystroke.key == "\\" {
                    if let Some(split) = this.split_before_hold.take() {
                        this.split = split;
                        this.submit();
                        cx.notify();
                    }
                }
            }))
            .child(self.top_bar(window))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .child(self.left_rail(window, cx))
                    .child(self.center(window))
                    .child(self.right_rail(window, cx)),
            )
            .child(self.status_bar())
            .child(drag_listener)
    }
}

fn arg<T: std::str::FromStr>(name: &str) -> Option<T> {
    std::env::args().skip_while(|a| a != name).nth(1).and_then(|s| s.parse().ok())
}

fn main() {
    let launched = Instant::now();
    let bench_seconds: Option<f32> = arg("--bench");
    let skip_upload = std::env::args().any(|a| a == "--skip-upload");
    let render_size = arg::<String>("--size")
        .and_then(|s| {
            let (w, h) = s.split_once('x')?;
            Some((w.parse().ok()?, h.parse().ok()?))
        })
        .unwrap_or((1800u32, 1200u32));

    gpui_kit::application().run(move |cx| {
        set_trace_enabled(true);
        cx.text_system()
            .add_fonts(vec![
                Cow::Borrowed(include_bytes!("../assets/fonts/IBMPlexSans-Regular.ttf")),
                Cow::Borrowed(include_bytes!("../assets/fonts/IBMPlexSans-Medium.ttf")),
                Cow::Borrowed(include_bytes!("../assets/fonts/IBMPlexSans-SemiBold.ttf")),
                Cow::Borrowed(include_bytes!("../assets/fonts/IBMPlexMono-Regular.ttf")),
                Cow::Borrowed(include_bytes!("../assets/fonts/IBMPlexMono-Medium.ttf")),
            ])
            .expect("bundled Plex fonts load");

        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(Bounds::centered(
                None,
                size(px(1440.), px(900.)),
                cx,
            ))),
            window_min_size: Some(size(px(1440.), px(900.))),
            ..Default::default()
        };

        cx.open_window(options, move |window, cx| {
            cx.new(|cx: &mut Context<Spike>| {
                let (tx, mut rx) = futures::channel::mpsc::unbounded::<develop::Rendered>();
                let (renderer, adapter) =
                    develop::Renderer::spawn(render_size.0, render_size.1, move |frame| {
                        tx.unbounded_send(frame).ok();
                    })
                    .expect("wgpu adapter");

                cx.spawn(async move |this, cx| {
                    while let Some(mut frame) = rx.next().await {
                        // Latest wins if several arrived while the UI was busy.
                        while let Ok(Some(next)) = rx.try_next() {
                            frame = next;
                        }
                        if this.update(cx, |this, cx| this.receive(frame, cx)).is_err() {
                            break;
                        }
                    }
                })
                .detach();

                let focus = cx.focus_handle();
                window.focus(&focus, cx);

                let mut spike = Spike {
                    values: params::initial(),
                    focus,
                    focused_param: 2,
                    dragging: None,
                    track_bounds: (0..12).map(|_| Rc::new(Cell::new(Bounds::default()))).collect(),
                    canvas_bounds: Rc::new(Cell::new(Bounds::default())),
                    split: 0.38,
                    split_before_hold: None,
                    preset: 1,
                    renderer,
                    adapter: adapter.into(),
                    render_size,
                    skip_upload,
                    seq: 0,
                    submitted: VecDeque::new(),
                    shown: None,
                    stale: Vec::new(),
                    histogram: [0.; 48],
                    collector: FrameTimingCollector::new(),
                    live: Samples::default(),
                    live_started: Instant::now(),
                    summary: SharedString::default(),
                    bench: bench_seconds.map(|seconds| Bench {
                        seconds,
                        started: Instant::now(),
                        samples: Samples::default(),
                    }),
                    launched,
                    first_preview: None,
                };
                spike.submit();
                if spike.bench.is_some() {
                    cx.on_next_frame(window, |this: &mut Spike, window, cx| this.tick_bench(window, cx));
                }
                spike
            })
        })
        .expect("open window");
        cx.activate(true);
    });
}
