//! G10, G11, G15: the inspector (Photo / Layout / Page tabs) and the
//! layout & template picker.

use laika_core::gallery::layout::{self, Category, TEMPLATES};
use laika_core::gallery::{Fit, Gallery, Theme, TypePairing};

use super::gallery_canvas::fitted_image;
use super::gallery_ui::*;
use super::*;

fn section(title: &str) -> Div {
    div()
        .pt(px(4.))
        .font_family(PLEX_MONO)
        .font_weight(FontWeight::MEDIUM)
        .text_size(px(9.5))
        .text_color(rgb(TEXT_DIM))
        .child(title.to_uppercase())
}

fn row_label(t: &str) -> Div {
    div()
        .w(px(78.))
        .flex_none()
        .text_size(px(11.5))
        .text_color(rgb(TEXT_TERTIARY))
        .child(t.to_string())
}

fn value_text(v: &str, placeholder: &str) -> Div {
    div()
        .flex_1()
        .min_w_0()
        .px(px(7.))
        .py(px(5.))
        .rounded(px(4.))
        .bg(rgb(bg_well()))
        .overflow_hidden()
        .text_size(px(11.5))
        .text_color(rgb(if v.is_empty() {
            TEXT_DIMMER
        } else {
            TEXT_SECONDARY
        }))
        .child(if v.is_empty() {
            placeholder.to_string()
        } else {
            v.to_string()
        })
}

fn seg_button(id: impl Into<ElementId>, label: &str, on: bool) -> Stateful<Div> {
    div()
        .id(id)
        .flex_1()
        .flex()
        .justify_center()
        .py(px(5.))
        .rounded(px(4.))
        .text_size(px(11.))
        .when(on, |d| {
            d.bg(rgb(accent_fill())).text_color(rgb(accent_on_fill()))
        })
        .when(!on, |d| {
            d.border_1()
                .border_color(border_control())
                .text_color(rgb(TEXT_SECONDARY))
                .hover(|d| d.bg(rgb(bg_row_hover())))
        })
        .child(label.to_string())
}

fn hexs(c: u32) -> String {
    format!("#{:06X}", c & 0xFF_FFFF)
}

impl Laika {
    pub(crate) fn gallery_inspector(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let tab = self.gal.tab;
        let width = if tab == InspectorTab::Page {
            348.
        } else {
            300.
        };
        let tabs = [
            ("Photo", InspectorTab::Photo),
            ("Layout", InspectorTab::Layout),
            ("Page", InspectorTab::Page),
        ];
        let body = match tab {
            InspectorTab::Photo => self.photo_tab(cx),
            InspectorTab::Layout => self.layout_tab(cx),
            InspectorTab::Page => self.page_tab(window, cx),
        };
        div()
            .w(px(width))
            .flex_none()
            .flex()
            .flex_col()
            .bg(rgb(bg_panel()))
            .border_l_1()
            .border_color(hairline())
            .child(
                div()
                    .h(px(36.))
                    .flex_none()
                    .flex()
                    .border_b_1()
                    .border_color(hairline())
                    .children(tabs.iter().map(|(name, t)| {
                        let t = *t;
                        let on = t == tab;
                        div()
                            .id(("gal-tab", t as usize))
                            .flex_1()
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(px(11.5))
                            .text_color(rgb(if on { TEXT_PRIMARY } else { TEXT_MUTED }))
                            .when(on, |d| d.border_b_2().border_color(rgb(TEXT_PRIMARY)))
                            .hover(|d| d.text_color(rgb(TEXT_PRIMARY)))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.gal.tab = t;
                                this.gal.swatch = None;
                                cx.notify();
                            }))
                            .child(*name)
                    })),
            )
            .child(
                div()
                    .id("gal-inspector-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(div().p(px(14.)).flex().flex_col().gap(px(14.)).child(body)),
            )
    }

    fn field_row(
        &self,
        label: &str,
        id: text_input::FieldId,
        value: &str,
        placeholder: &str,
        tip: &'static str,
        cx: &mut Context<Self>,
    ) -> Div {
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(row_label(label))
            .child(div().flex_1().min_w_0().flex().child(self.field_cell(
                id,
                value_text(value, placeholder),
                false,
                tip,
                cx,
            )))
    }

    fn toggle_row(
        &self,
        id: &'static str,
        on: bool,
        label: &str,
        f: fn(&mut Gallery),
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let label_s = label.to_string();
        div()
            .id(id)
            .on_click(cx.listener(move |this, _, _, cx| {
                this.gal_edit(&label_s, None, f, cx);
            }))
            .child(toggle::toggle(on, label))
    }

    /// A 0..1 track with a knob; dragging drives `kind`.
    fn gal_slider_row(
        &self,
        label: &str,
        readout: String,
        frac: f32,
        kind: GalSlider,
        cx: &mut Context<Self>,
    ) -> Div {
        let slot = match kind {
            GalSlider::TitleSize => self.gal.title_box.clone(),
            GalSlider::Radius => self.gal.radius_box.clone(),
            _ => self.gal.gutter_box.clone(),
        };
        let frac = frac.clamp(0., 1.);
        div()
            .flex()
            .items_center()
            .gap(px(8.))
            .child(row_label(label))
            .child(
                div()
                    .id(("gal-slider", kind as usize))
                    .flex_1()
                    .h(px(18.))
                    .relative()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                            this.gal.slider = Some(kind);
                            this.gal.history.seal();
                            this.gal_slider_to(
                                kind,
                                (ev.position.x.as_f32(), ev.position.y.as_f32()),
                                cx,
                            );
                        }),
                    )
                    .child(
                        div()
                            .absolute()
                            .left(px(0.))
                            .right(px(0.))
                            .top(px(8.))
                            .h(px(2.))
                            .bg(rgb(track()))
                            .child(meter(slot)),
                    )
                    .child(
                        div()
                            .absolute()
                            .left(relative(0.))
                            .top(px(8.))
                            .h(px(2.))
                            .w(relative(frac))
                            .bg(rgb(TEXT_SECONDARY)),
                    )
                    .child(
                        div()
                            .absolute()
                            .left(relative(frac))
                            .top(px(3.5))
                            .ml(px(-5.5))
                            .size(px(11.))
                            .rounded_full()
                            .bg(rgb(bg_panel()))
                            .border_2()
                            .border_color(rgb(TEXT_PRIMARY)),
                    ),
            )
            .child(
                div()
                    .w(px(40.))
                    .flex_none()
                    .flex()
                    .justify_end()
                    .font_family(PLEX_MONO)
                    .text_size(px(11.))
                    .text_color(rgb(TEXT_SECONDARY))
                    .child(readout),
            )
    }

    // ---- Photo tab --------------------------------------------------------------

    fn photo_tab(&self, cx: &mut Context<Self>) -> Div {
        let g = self.gal.current.as_ref().expect("open gallery");
        let Some((i, p)) = self
            .gal
            .selected
            .and_then(|id| g.index_of(id).map(|i| (i, &g.photos[i])))
        else {
            return div()
                .pt(px(30.))
                .flex()
                .flex_col()
                .items_center()
                .gap(px(6.))
                .text_size(px(11.5))
                .text_color(rgb(TEXT_DIM))
                .child("No photo selected")
                .child("Click a photo on the page or in the tray");
        };
        let _ = i;
        let pid = p.photo_id;
        let row = self.find(pid);
        let thumb = self.thumbs.get(&pid).map(|t| t.image.clone());
        let cols = g.columns;
        let (sx, sy) = (p.span_x, p.span_y);
        let placed = p.cell.is_some();
        let presets = [
            ("1×1", 1u8, sx == 1 && sy == 1),
            ("2×1", 2, sx == 2 && sy == 1),
            ("2×2", 3, sx == 2 && sy == 2),
            ("Full", 4, sx == cols && sy == 1 && cols > 2),
        ];

        // Focal preview: the whole photo fitted, thirds, and the handle.
        let (bw, bh) = (272., 120.);
        let focal = p.focal;
        let fit = p.fit;
        let preview = match thumb.clone() {
            Some(im) => {
                let sz = im.size(0);
                let (iw, ih) = (sz.width.0.max(1) as f32, sz.height.0.max(1) as f32);
                let s = (bw / iw).min(bh / ih);
                let (dw, dh) = (iw * s, ih * s);
                let (ox, oy) = ((bw - dw) / 2., (bh - dh) / 2.);
                let line = || div().absolute().bg(rgba(0xFFFDF859));
                div()
                    .relative()
                    .w(px(bw))
                    .h(px(bh))
                    .rounded(px(4.))
                    .bg(rgb(bg_well()))
                    .overflow_hidden()
                    .child(
                        div()
                            .id("gal-focal")
                            .absolute()
                            .left(px(ox))
                            .top(px(oy))
                            .w(px(dw))
                            .h(px(dh))
                            .cursor(CursorStyle::Crosshair)
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, ev: &MouseDownEvent, _, cx| {
                                    this.gal.slider = Some(GalSlider::Focal);
                                    this.gal.history.seal();
                                    this.gal_slider_to(
                                        GalSlider::Focal,
                                        (ev.position.x.as_f32(), ev.position.y.as_f32()),
                                        cx,
                                    );
                                }),
                            )
                            .child(meter(self.gal.focal_box.clone()))
                            .child(fitted_image(im, dw, dh, Fit::Fit, (0.5, 0.5)))
                            .child(line().left(relative(1. / 3.)).top_0().bottom_0().w(px(1.)))
                            .child(line().left(relative(2. / 3.)).top_0().bottom_0().w(px(1.)))
                            .child(line().top(relative(1. / 3.)).left_0().right_0().h(px(1.)))
                            .child(line().top(relative(2. / 3.)).left_0().right_0().h(px(1.)))
                            .child(
                                div()
                                    .absolute()
                                    .left(px(focal.0 * dw - 8.))
                                    .top(px(focal.1 * dh - 8.))
                                    .size(px(16.))
                                    .rounded_full()
                                    .border_2()
                                    .border_color(rgb(0xF2EFE6))
                                    .shadow(vec![gpui_kit::BoxShadow {
                                        color: rgba(0x000000CC).into(),
                                        offset: point(px(0.), px(0.)),
                                        blur_radius: px(1.),
                                        spread_radius: px(1.),
                                        inset: false,
                                    }]),
                            ),
                    )
            }
            None => div()
                .w(px(bw))
                .h(px(bh))
                .rounded(px(4.))
                .bg(rgb(bg_well()))
                .flex()
                .items_center()
                .justify_center()
                .text_size(px(10.5))
                .text_color(rgb(TEXT_DIM))
                .child("Loading preview…"),
        };

        div()
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(
                div()
                    .flex()
                    .gap(px(10.))
                    .child(
                        div()
                            .relative()
                            .size(px(56.))
                            .flex_none()
                            .rounded(px(3.))
                            .overflow_hidden()
                            .bg(rgb(bg_well()))
                            .children(
                                thumb.map(|im| fitted_image(im, 56., 56., Fit::Fill, (0.5, 0.5))),
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(3.))
                            .child(
                                div()
                                    .text_size(px(12.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(TEXT_PRIMARY))
                                    .overflow_hidden()
                                    .child(row.map(|r| r.filename.clone()).unwrap_or_default()),
                            )
                            .child(
                                div()
                                    .font_family(PLEX_MONO)
                                    .text_size(px(10.5))
                                    .text_color(rgb(TEXT_DIM))
                                    .child(
                                        row.map(|r| format!("{} × {}", r.width, r.height))
                                            .unwrap_or_default(),
                                    ),
                            )
                            .child(
                                div()
                                    .text_size(px(10.5))
                                    .text_color(rgb(if placed { TEXT_DIM } else { WARNING }))
                                    .child(match p.cell {
                                        Some(c) => format!(
                                            "on the page · row {} · cell {}",
                                            c.row + 1,
                                            c.col + 1
                                        ),
                                        None => "in the tray, not on the page".to_string(),
                                    }),
                            ),
                    ),
            )
            .child(section("Span"))
            .child(
                div()
                    .flex()
                    .gap(px(5.))
                    .children(presets.iter().map(|(label, n, on)| {
                        let n = *n;
                        seg_button(("gal-span", n as usize), label, *on && placed).on_click(
                            cx.listener(move |this, _, _, cx| this.gal_set_span_preset(pid, n, cx)),
                        )
                    })),
            )
            .child(section("Crop & focal point"))
            .child(preview)
            .child(
                div()
                    .flex()
                    .gap(px(5.))
                    .child(
                        seg_button("gal-fill", "Fill", fit == Fit::Fill).on_click(cx.listener(
                            move |this, _, _, cx| {
                                this.gal_edit(
                                    "Fill",
                                    None,
                                    |g| {
                                        if let Some(i) = g.index_of(pid) {
                                            g.photos[i].fit = Fit::Fill;
                                        }
                                    },
                                    cx,
                                )
                            },
                        )),
                    )
                    .child(
                        seg_button("gal-fit", "Fit", fit == Fit::Fit).on_click(cx.listener(
                            move |this, _, _, cx| {
                                this.gal_edit(
                                    "Fit",
                                    None,
                                    |g| {
                                        if let Some(i) = g.index_of(pid) {
                                            g.photos[i].fit = Fit::Fit;
                                        }
                                    },
                                    cx,
                                )
                            },
                        )),
                    )
                    .child(
                        seg_button("gal-focal-reset", "Reset", false).on_click(cx.listener(
                            move |this, _, _, cx| {
                                this.gal_edit(
                                    "Reset focal point",
                                    None,
                                    |g| {
                                        if let Some(i) = g.index_of(pid) {
                                            g.photos[i].focal = (0.5, 0.5);
                                            g.photos[i].fit = Fit::Fill;
                                        }
                                    },
                                    cx,
                                )
                            },
                        )),
                    ),
            )
            .child(section("Text"))
            .child(self.field_row(
                "Caption",
                text_input::FieldId::GalleryCaption,
                &p.caption,
                "Shown under the photo",
                "Caption on the published page (Enter applies)",
                cx,
            ))
            .child(self.field_row(
                "Alt text",
                text_input::FieldId::GalleryAlt,
                &p.alt_text,
                "Describe this photo for screen readers…",
                "Read aloud by screen readers; the caption is used when empty",
                cx,
            ))
            .child(
                div()
                    .id("gal-open-full")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.gal_edit(
                            "Open full size",
                            None,
                            |g| {
                                if let Some(i) = g.index_of(pid) {
                                    g.photos[i].open_full_size = !g.photos[i].open_full_size;
                                }
                            },
                            cx,
                        )
                    }))
                    .child(toggle::toggle(p.open_full_size, "Open full size on click")),
            )
            .child(
                div()
                    .flex()
                    .gap(px(6.))
                    .pt(px(4.))
                    .child(
                        seg_button(
                            "gal-unplace",
                            if placed {
                                "Remove from page"
                            } else {
                                "Place on page"
                            },
                            false,
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let placed = this
                                .gal
                                .current
                                .as_ref()
                                .and_then(|g| g.index_of(pid).map(|i| g.photos[i].cell.is_some()))
                                .unwrap_or(false);
                            if placed {
                                this.gal_edit("Remove from page", None, |g| g.unplace(pid), cx);
                            } else {
                                this.gal_edit("Place photo", None, |g| g.place_next(pid), cx);
                            }
                        })),
                    )
                    .child(
                        seg_button("gal-remove", "Remove from gallery", false).on_click(
                            cx.listener(move |this, _, _, cx| {
                                this.gal_edit(
                                    "Remove from gallery",
                                    None,
                                    |g| {
                                        g.remove_photos(&[pid]);
                                    },
                                    cx,
                                );
                                this.gal.selected = None;
                            }),
                        ),
                    ),
            )
    }

    // ---- Layout tab ---------------------------------------------------------------

    fn layout_tab(&self, cx: &mut Context<Self>) -> Div {
        let g = self.gal.current.as_ref().expect("open gallery");
        let t = layout::template(&g.template);
        let ratios: [(&str, f32); 5] = [
            ("2:1", 2.0),
            ("16:9", 16. / 9.),
            ("3:2", 1.5),
            ("1:1", 1.0),
            ("4:5", 0.8),
        ];
        let sizes = [640u32, 1280, 2048, 3200];
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(section("Template"))
            .child(
                div()
                    .flex()
                    .gap(px(10.))
                    .items_center()
                    .child(self.template_diagram(t, 72., 54.))
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .gap(px(3.))
                            .child(
                                div()
                                    .text_size(px(12.5))
                                    .text_color(rgb(TEXT_PRIMARY))
                                    .child(t.name),
                            )
                            .child(
                                div()
                                    .text_size(px(10.5))
                                    .text_color(rgb(TEXT_DIM))
                                    .child(t.description),
                            ),
                    ),
            )
            .child(
                seg_button("gal-change-layout", "Change layout…", false)
                    .on_click(cx.listener(|this, _, _, cx| this.open_picker(cx))),
            )
            .child(section("Grid"))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(row_label("Columns"))
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .gap(px(4.))
                            .children((1..=6u8).map(|n| {
                                seg_button(("gal-cols", n as usize), &n.to_string(), g.columns == n)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.gal_edit(
                                            &format!("{n} columns"),
                                            None,
                                            |g| g.set_columns(n),
                                            cx,
                                        )
                                    }))
                            })),
                    ),
            )
            .child(self.gal_slider_row(
                "Gutter",
                format!("{} px", g.gutter),
                g.gutter as f32 / 32.,
                GalSlider::Gutter,
                cx,
            ))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(row_label("Row shape"))
                    .child(div().flex_1().flex().gap(px(4.)).children(
                        ratios.iter().enumerate().map(|(k, (label, r))| {
                            let r = *r;
                            let on = (g.ratio - r).abs() < 0.01;
                            seg_button(("gal-ratio", k), label, on).on_click(cx.listener(
                                move |this, _, _, cx| {
                                    this.gal_edit("Row shape", None, |g| g.ratio = r, cx)
                                },
                            ))
                        }),
                    )),
            )
            .child(
                seg_button(
                    "gal-reflow",
                    "Re-flow every photo with this template",
                    false,
                )
                .on_hover(
                    self.tip("Places all photos in order with the template's spans (undoable)"),
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.gal_edit("Re-flow", None, |g| g.reflow_all(), cx)
                })),
            )
            .child(section("Published images"))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(row_label("Sizes"))
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .gap(px(4.))
                            .children(sizes.iter().map(|s| {
                                let s = *s;
                                let on = g.sizes.contains(&s);
                                seg_button(("gal-size", s as usize), &s.to_string(), on).on_click(
                                    cx.listener(move |this, _, _, cx| {
                                        let only = this
                                            .gal
                                            .current
                                            .as_ref()
                                            .is_some_and(|g| g.sizes == [s]);
                                        if only {
                                            this.status_note =
                                                "keep at least one image size".to_string();
                                            cx.notify();
                                            return;
                                        }
                                        this.gal_edit(
                                            "Image sizes",
                                            None,
                                            |g| {
                                                if let Some(k) =
                                                    g.sizes.iter().position(|x| *x == s)
                                                {
                                                    g.sizes.remove(k);
                                                } else {
                                                    g.sizes.push(s);
                                                    g.sizes.sort_unstable();
                                                }
                                            },
                                            cx,
                                        )
                                    }),
                                )
                            })),
                    ),
            )
            .child(
                div()
                    .text_size(px(10.5))
                    .text_color(rgb(TEXT_DIM))
                    .child("Long edge in pixels; never upscaled. JPEG, sRGB, no EXIF or GPS."),
            )
            .child(self.toggle_row(
                "gal-downloads",
                g.allow_downloads,
                "Allow downloads",
                |g| g.allow_downloads = !g.allow_downloads,
                cx,
            ))
            .child(
                div()
                    .id("gal-password")
                    .opacity(0.45)
                    .on_hover(self.tip("Needs a host that supports access control"))
                    .child(toggle::toggle(false, "Password protect")),
            )
    }

    /// Miniature of a template's first eight tiles.
    pub(crate) fn template_diagram(&self, t: &layout::Template, w: f32, h: f32) -> Div {
        let placed = layout::apply_template(8, t, t.columns);
        let pad = 6.;
        let gap = 3.;
        let inner_w = w - 2. * pad;
        let (col_w, row_h) = layout::metrics(inner_w, t.columns, gap, t.ratio);
        let tones = [0xA9B2B0u32, 0xD6CCBC, 0x9AA3A8, 0xC9C2B4, 0x8E9A98];
        let mut d = div()
            .relative()
            .w(px(w))
            .h(px(h))
            .flex_none()
            .rounded(px(4.))
            .overflow_hidden()
            .bg(rgb(0x121517));
        for (k, (c, sx, sy)) in placed.into_iter().enumerate() {
            let (x, y, tw, th) = layout::tile_rect(c, sx, sy, col_w, row_h, gap);
            if y > h {
                continue;
            }
            d = d.child(
                div()
                    .absolute()
                    .left(px(pad + x))
                    .top(px(pad + y))
                    .w(px(tw))
                    .h(px(th))
                    .bg(rgb(tones[k % tones.len()]))
                    .opacity(0.8),
            );
        }
        d
    }

    // ---- Page tab -------------------------------------------------------------------

    fn page_tab(&self, _window: &mut Window, cx: &mut Context<Self>) -> Div {
        let g = self.gal.current.as_ref().expect("open gallery");
        let t = &g.theme;
        use text_input::FieldId as F;
        let mut swatches: Vec<(usize, &str, u32)> = vec![
            (0, "page", t.page),
            (1, "canvas", t.canvas),
            (2, "ink", t.ink),
            (3, "accent", t.accent),
        ];
        for (k, c) in t.extras.iter().enumerate() {
            swatches.push((4 + k, "extra", *c));
        }
        let editing = self.gal.swatch;
        let low_contrast = Theme::contrast(t.accent, t.page) < 4.5;
        let accent_text = t.accent_text();
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(section("Gallery"))
            .child(self.field_row(
                "Title",
                F::GalleryTitle,
                &g.title,
                "Untitled gallery",
                "Page title (Enter applies)",
                cx,
            ))
            .child(self.field_row(
                "Eyebrow",
                F::GalleryEyebrow,
                &g.eyebrow,
                "TRAVEL · 2026",
                "Small line above the title",
                cx,
            ))
            .child(self.field_row(
                "Lede",
                F::GallerySubtitle,
                &g.subtitle,
                "A sentence about these photos",
                "Introduction beside the title",
                cx,
            ))
            .child(self.field_row(
                "Web address",
                F::GallerySlug,
                &g.slug,
                "hokkaido-2026",
                "Folder and URL name: lowercase letters, digits and dashes",
                cx,
            ))
            .child(self.field_row(
                "Site name",
                F::GallerySite,
                &g.site_name,
                "Your name",
                "Shown in the page header",
                cx,
            ))
            .child(self.field_row(
                "Meta line",
                F::GalleryMeta,
                &g.meta_line,
                "Sapporo, Otaru",
                "After the photo count, like “48 photographs · Sapporo”",
                cx,
            ))
            .child(section("Type pairing"))
            .children(TypePairing::ALL.iter().map(|p| {
                let p = *p;
                let on = t.pairing == p;
                let (fam, weight) = match p {
                    TypePairing::SansDisplay => (PLEX_SANS, FontWeight::SEMIBOLD),
                    TypePairing::SansMono => (PLEX_MONO, FontWeight::MEDIUM),
                    TypePairing::LightDisplay => (PLEX_SANS, FontWeight::NORMAL),
                };
                div()
                    .id(("gal-pairing", p as usize))
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .px(px(12.))
                    .py(px(9.))
                    .rounded(px(7.))
                    .bg(rgb(t.page))
                    .border_1()
                    .when(on, |d| d.border_color(rgb(accent_line())))
                    .when(!on, |d| {
                        d.border_color(hairline())
                            .hover(|d| d.border_color(border_strong()))
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.gal_edit("Type pairing", None, |g| g.theme.pairing = p, cx)
                    }))
                    .child(
                        div()
                            .w(px(40.))
                            .font_family(fam)
                            .font_weight(weight)
                            .text_size(px(23.))
                            .text_color(rgb(t.ink))
                            .child("Aa"),
                    )
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(rgb(t.ink))
                                    .child(p.name()),
                            )
                            .child(
                                div()
                                    .text_size(px(10.5))
                                    .text_color(rgba((t.ink << 8) | 0x80))
                                    .child(p.descriptor()),
                            ),
                    )
                    .when(on, |d| {
                        d.child(
                            div()
                                .font_family(PLEX_MONO)
                                .text_size(px(9.))
                                .text_color(rgb(accent_line()))
                                .child("IN USE"),
                        )
                    })
            }))
            .child(self.gal_slider_row(
                "Title size",
                format!("{} px", t.title_size),
                (t.title_size as f32 - 32.) / 40.,
                GalSlider::TitleSize,
                cx,
            ))
            .child(section("Palette"))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(8.))
                    .children(swatches.iter().map(|(k, name, c)| {
                        let k = *k;
                        let on = editing == Some(k);
                        div()
                            .id(("gal-swatch", k))
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap(px(3.))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.gal.swatch = if this.gal.swatch == Some(k) {
                                    None
                                } else {
                                    Some(k)
                                };
                                if this.gal.swatch.is_some() {
                                    this.focus_field(text_input::FieldId::GalleryHex, cx);
                                }
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .size(px(46.))
                                    .rounded(px(6.))
                                    .bg(rgb(*c))
                                    .border_1()
                                    .when(on, |d| d.border_2().border_color(rgb(accent_line())))
                                    .when(!on, |d| d.border_color(border_control())),
                            )
                            .child(
                                div()
                                    .text_size(px(9.5))
                                    .text_color(rgb(TEXT_DIM))
                                    .child(name.to_string()),
                            )
                    }))
                    .child(
                        div()
                            .id("gal-swatch-add")
                            .flex()
                            .flex_col()
                            .items_center()
                            .gap(px(3.))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.gal.swatch = Some(NEW_SWATCH);
                                this.focus_field(text_input::FieldId::GalleryHex, cx);
                                cx.notify();
                            }))
                            .child(
                                div()
                                    .size(px(46.))
                                    .rounded(px(6.))
                                    .border_1()
                                    .border_color(border_strong())
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .text_size(px(18.))
                                    .text_color(rgb(TEXT_DIM))
                                    .child("+"),
                            )
                            .child(
                                div()
                                    .text_size(px(9.5))
                                    .text_color(rgb(TEXT_DIM))
                                    .child("add"),
                            ),
                    ),
            )
            .when_some(editing, |d, k| {
                let current = match k {
                    0 => t.page,
                    1 => t.canvas,
                    2 => t.ink,
                    3 => t.accent,
                    NEW_SWATCH => t.accent,
                    n => t.extras.get(n - 4).copied().unwrap_or(0),
                };
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .child(
                            self.field_row(
                                if k == NEW_SWATCH { "New color" } else { "Hex" },
                                F::GalleryHex,
                                &hexs(current),
                                "#RRGGBB",
                                "Type a hex color like #121517 (Enter applies)",
                                cx,
                            )
                            .flex_1(),
                        )
                        .when(k >= 4 && k != NEW_SWATCH, |d| {
                            d.child(
                                seg_button("gal-swatch-del", "Remove", false)
                                    .flex_none()
                                    .px(px(8.))
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.gal_edit(
                                            "Remove color",
                                            None,
                                            |g| {
                                                if k - 4 < g.theme.extras.len() {
                                                    g.theme.extras.remove(k - 4);
                                                }
                                            },
                                            cx,
                                        );
                                        this.gal.swatch = None;
                                        this.defocus_field();
                                    })),
                            )
                        }),
                )
            })
            .when(low_contrast, |d| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(6.))
                        .text_size(px(10.5))
                        .text_color(rgb(TEXT_TERTIARY))
                        .child(
                            div()
                                .flex_none()
                                .size(px(10.))
                                .rounded(px(2.))
                                .bg(rgb(accent_text)),
                        )
                        .child(div().flex_1().min_w_0().child(format!(
                            "Accent reads poorly as text on the page; eyebrows use {} instead",
                            hexs(accent_text)
                        ))),
                )
            })
            .child(section("Behaviour"))
            .child(self.toggle_row(
                "gal-captions",
                t.show_captions,
                "Show captions",
                |g| g.theme.show_captions = !g.theme.show_captions,
                cx,
            ))
            .child(self.toggle_row(
                "gal-hover-zoom",
                t.hover_zoom,
                "Hover zoom",
                |g| g.theme.hover_zoom = !g.theme.hover_zoom,
                cx,
            ))
            .child(self.gal_slider_row(
                "Corners",
                format!("{} px", t.corner_radius),
                t.corner_radius as f32 / 24.,
                GalSlider::Radius,
                cx,
            ))
    }

    // ---- layout picker (1b) -----------------------------------------------------------

    pub(crate) fn open_picker(&mut self, cx: &mut Context<Self>) {
        let Some(g) = self.gal.current.as_ref() else {
            return;
        };
        self.gal.picker = Some(Picker {
            choice: layout::template(&g.template).id,
            category: None,
        });
        cx.notify();
    }

    pub(crate) fn apply_picker(&mut self, cx: &mut Context<Self>) {
        let Some(p) = self.gal.picker.take() else {
            return;
        };
        let name = layout::template(p.choice).name;
        self.gal_edit(
            &format!("Layout: {name}"),
            None,
            |g| g.apply_template(p.choice),
            cx,
        );
        self.status_note = format!("layout set to {name} — captions and order kept");
        cx.notify();
    }

    pub(crate) fn layout_picker(&self, cx: &mut Context<Self>) -> Div {
        let g = self.gal.current.as_ref().expect("open gallery");
        let p = self.gal.picker.as_ref().expect("picker open");
        let current = layout::template(&g.template).id;
        let cats: [(Option<Category>, String); 5] = [
            (None, format!("All {}", TEMPLATES.len())),
            (
                Some(Category::Uniform),
                Category::Uniform.label().to_string(),
            ),
            (
                Some(Category::Editorial),
                Category::Editorial.label().to_string(),
            ),
            (
                Some(Category::SingleColumn),
                Category::SingleColumn.label().to_string(),
            ),
            (
                Some(Category::ContactSheet),
                Category::ContactSheet.label().to_string(),
            ),
        ];
        let visible: Vec<&layout::Template> = TEMPLATES
            .iter()
            .filter(|t| p.category.is_none_or(|c| c == t.category))
            .collect();
        let card_w = (1020. - 62. - 3. * 16.) / 4. - 1.;
        let content = div()
            .flex()
            .flex_col()
            .child(
                div()
                    .px(px(30.))
                    .pt(px(26.))
                    .pb(px(14.))
                    .flex()
                    .justify_between()
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(div().text_size(px(22.)).font_weight(FontWeight::SEMIBOLD).text_color(rgb(TEXT_PRIMARY)).child("Choose a layout"))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(rgb(TEXT_TERTIARY))
                                    .child(format!(
                                        "Your {} photos flow into the grid. You can move any of them afterwards.",
                                        g.photos.len()
                                    )),
                            ),
                    )
                    .child(
                        div()
                            .id("gal-picker-close")
                            .text_size(px(15.))
                            .text_color(rgb(TEXT_DIM))
                            .hover(|d| d.text_color(rgb(TEXT_PRIMARY)))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.gal.picker = None;
                                cx.notify();
                            }))
                            .child("✕"),
                    ),
            )
            .child(
                div().px(px(30.)).pb(px(16.)).flex().gap(px(6.)).children(cats.iter().enumerate().map(|(k, (c, label))| {
                    let c = *c;
                    let on = p.category == c;
                    div()
                        .id(("gal-picker-cat", k))
                        .px(px(10.))
                        .py(px(4.))
                        .rounded(px(12.))
                        .text_size(px(11.))
                        .when(on, |d| d.bg(rgb(TEXT_PRIMARY)).text_color(rgb(bg_panel())))
                        .when(!on, |d| d.border_1().border_color(border_control()).text_color(rgb(TEXT_SECONDARY)))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(p) = this.gal.picker.as_mut() {
                                p.category = c;
                            }
                            cx.notify();
                        }))
                        .child(label.clone())
                })),
            )
            .child(
                div().px(px(30.)).pb(px(22.)).flex().flex_wrap().gap(px(16.)).children(visible.iter().map(|t| {
                    let id = t.id;
                    let chosen = p.choice == id;
                    div()
                        .id(("gal-picker-card", TEMPLATES.iter().position(|x| x.id == id).unwrap_or(0)))
                        .w(px(card_w))
                        .flex()
                        .flex_col()
                        .rounded(px(8.))
                        .bg(rgb(bg_panel_deep()))
                        .border_1()
                        .when(chosen, |d| d.border_2().border_color(rgb(accent_line())))
                        .when(!chosen, |d| d.border_color(hairline()).hover(|d| d.border_color(border_strong())))
                        .on_click(cx.listener(move |this, ev: &ClickEvent, _, cx| {
                            if let Some(p) = this.gal.picker.as_mut() {
                                p.choice = id;
                            }
                            if ev.click_count() >= 2 {
                                this.apply_picker(cx);
                            }
                            cx.notify();
                        }))
                        .child(
                            div()
                                .relative()
                                .p(px(12.))
                                .child(self.template_diagram(t, card_w - 24., 118.))
                                .when(id == current, |d| {
                                    d.child(
                                        div()
                                            .absolute()
                                            .top(px(18.))
                                            .right(px(18.))
                                            .px(px(5.))
                                            .py(px(1.))
                                            .rounded(px(3.))
                                            .bg(rgb(accent_fill()))
                                            .font_family(PLEX_MONO)
                                            .text_size(px(9.))
                                            .text_color(rgb(accent_on_fill()))
                                            .child("CURRENT"),
                                    )
                                }),
                        )
                        .child(
                            div()
                                .px(px(12.))
                                .pb(px(12.))
                                .flex()
                                .flex_col()
                                .gap(px(2.))
                                .child(div().text_size(px(12.5)).font_weight(FontWeight::MEDIUM).text_color(rgb(TEXT_PRIMARY)).child(t.name))
                                .child(div().text_size(px(11.)).text_color(rgb(TEXT_DIM)).child(t.description)),
                        )
                })),
            )
            .child(
                div()
                    .px(px(30.))
                    .py(px(16.))
                    .border_t_1()
                    .border_color(hairline())
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(div().flex_1().text_size(px(11.5)).text_color(rgb(TEXT_TERTIARY)).child("Switching layouts keeps your captions and photo order."))
                    .child(
                        div()
                            .id("gal-picker-cancel")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.gal.picker = None;
                                cx.notify();
                            }))
                            .child(button::outline("Cancel")),
                    )
                    .child(
                        div()
                            .id("gal-picker-apply")
                            .on_click(cx.listener(|this, _, _, cx| this.apply_picker(cx)))
                            .child(button::primary("Apply layout")),
                    ),
            );
        modal::modal_shell(content)
    }
}
