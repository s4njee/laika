//! U11: Library Compare and Survey review views.

use std::cell::Cell;
use std::collections::HashSet;
use std::rc::Rc;

use super::*;

pub(crate) struct ReviewUi {
    pub candidate: Option<i64>,
    pub survey_hidden: HashSet<i64>,
    pub left_bounds: Rc<Cell<Bounds<Pixels>>>,
    pub right_bounds: Rc<Cell<Bounds<Pixels>>>,
}

impl Default for ReviewUi {
    fn default() -> Self {
        Self {
            candidate: None,
            survey_hidden: HashSet::new(),
            left_bounds: Rc::new(Cell::new(Bounds::default())),
            right_bounds: Rc::new(Cell::new(Bounds::default())),
        }
    }
}

impl Laika {
    pub(crate) fn open_compare(&mut self, cx: &mut Context<Self>) {
        self.flush_saves();
        self.state.active_module = Module::Library;
        let ids = self.ordered_ids();
        if self.state.primary.is_none() {
            if let Some(first) = ids.first().copied() {
                self.select_navigate(first, SelectMode::Set, cx);
            }
        }
        let primary = self.state.primary;
        let selected_candidate = ids
            .iter()
            .copied()
            .find(|id| Some(*id) != primary && self.state.selection.contains(id));
        self.review.candidate = selected_candidate.or_else(|| {
            let at = primary.and_then(|id| ids.iter().position(|p| *p == id))?;
            ids.get(at + 1)
                .copied()
                .or_else(|| at.checked_sub(1).and_then(|i| ids.get(i).copied()))
        });
        self.view = ViewMode::Compare;
        self.prev_view = ViewMode::Compare;
        self.zoom_center = (0.5, 0.5);
        if self.review.candidate.is_none() {
            self.status_note = "Compare needs at least two visible photos".to_string();
        }
        if let Some(id) = primary {
            self.kick_large_load(id, cx);
        }
        if let Some(id) = self.review.candidate {
            self.kick_large_load(id, cx);
        }
        cx.notify();
    }

    pub(crate) fn open_survey(&mut self, cx: &mut Context<Self>) {
        self.flush_saves();
        self.state.active_module = Module::Library;
        self.review.survey_hidden.clear();
        self.view = ViewMode::Survey;
        self.prev_view = ViewMode::Survey;
        if self.survey_ids().len() < 2 {
            self.status_note = "Select two or more photos for Survey".to_string();
        }
        cx.notify();
    }

    pub(crate) fn set_compare_candidate(&mut self, id: i64, cx: &mut Context<Self>) {
        if Some(id) == self.state.primary {
            return;
        }
        self.review.candidate = Some(id);
        self.kick_large_load(id, cx);
        cx.notify();
    }

    fn step_compare_candidate(&mut self, delta: isize, cx: &mut Context<Self>) {
        let ids: Vec<i64> = self
            .ordered_ids()
            .into_iter()
            .filter(|id| Some(*id) != self.state.primary)
            .collect();
        if ids.is_empty() {
            return;
        }
        let current = self
            .review
            .candidate
            .and_then(|id| ids.iter().position(|p| *p == id))
            .unwrap_or(0);
        let next = (current as isize + delta).rem_euclid(ids.len() as isize) as usize;
        self.set_compare_candidate(ids[next], cx);
    }

    fn swap_compare(&mut self, cx: &mut Context<Self>) {
        let (Some(select), Some(candidate)) = (self.state.primary, self.review.candidate) else {
            return;
        };
        self.select_navigate(candidate, SelectMode::Set, cx);
        self.review.candidate = Some(select);
        self.kick_large_load(select, cx);
        cx.notify();
    }

    fn survey_ids(&self) -> Vec<i64> {
        self.ordered_ids()
            .into_iter()
            .filter(|id| {
                self.state.selection.contains(id) && !self.review.survey_hidden.contains(id)
            })
            .collect()
    }

    /// One compare/survey star click updates exactly that photo, without
    /// replacing the selection or rating a paired neighbor.
    fn rate_review_photo(&mut self, photo_id: i64, rating: u8, cx: &mut Context<Self>) {
        let before = self.snap_current(photo_id);
        let Some(photo) = self.photos.iter_mut().find(|p| p.id == photo_id) else {
            return;
        };
        if photo.rating == rating {
            return;
        }
        photo.rating = rating.min(5);
        self.record_step(photo_id, "Rating", &rating.to_string(), before);
        self.last_batch = vec![photo_id];
        self.redo_batch.clear();
        self.last_was_meta = false;
        self.last_was_remove = false;
        if self.persist_rating(photo_id) {
            self.status_note = format!("rated {} stars", rating.min(5));
        }
        self.request_sidecar(photo_id, false);
        self.kick_save_timer(cx);
        self.state.photos = self.photos.iter().map(as_photo).collect();
        cx.notify();
    }

    fn review_stars(&self, photo_id: i64, rating: u8, cx: &mut Context<Self>) -> Div {
        let mut row = div().flex().items_center().gap(px(2.));
        for star in 1..=5u8 {
            row =
                row.child(
                    div()
                        .id((
                            "review-star",
                            (photo_id as usize).wrapping_mul(8) + star as usize,
                        ))
                        .role(Role::Button)
                        .aria_label(format!("Rate {star} stars"))
                        .px(px(2.))
                        .text_size(sp(15.))
                        .text_color(rgb(if star <= rating { WARNING } else { TEXT_DIMMER }))
                        .hover(|s| s.text_color(rgb(TEXT_PRIMARY)))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.rate_review_photo(photo_id, star, cx)
                        }))
                        .child("★"),
                );
        }
        row
    }

    fn compare_pane(
        &self,
        photo_id: Option<i64>,
        label: &'static str,
        bounds: Rc<Cell<Bounds<Pixels>>>,
        cx: &mut Context<Self>,
    ) -> Div {
        let photo = photo_id.and_then(|id| self.find(id));
        let image = photo_id.and_then(|id| {
            self.large
                .get(&id)
                .cloned()
                .or_else(|| self.thumbs.get(&id).map(|t| t.image.clone()))
        });
        let image_el: AnyElement = match (photo, image) {
            (Some(p), Some(image)) => {
                let max_edge = p.width.max(p.height).max(1) as f32;
                let scale = (2048. / max_edge).min(1.);
                let (nw, nh) = (
                    (p.width.max(1) as f32 * scale).max(1.),
                    (p.height.max(1) as f32 * scale).max(1.),
                );
                let b = bounds.get();
                let (vw, vh) = (b.size.width.as_f32(), b.size.height.as_f32());
                if vw > 1. && vh > 1. {
                    let g = zoom::stage_geom(
                        self.zoom,
                        self.zoom_center,
                        vw,
                        vh,
                        nw,
                        nh,
                        self.view_scale.get().max(0.01),
                    );
                    div()
                        .absolute()
                        .left(px(g.off_x))
                        .top(px(g.off_y))
                        .w(px(g.disp_w))
                        .h(px(g.disp_h))
                        .child(img(ImageSource::Render(image)).size_full())
                        .into_any_element()
                } else {
                    img(ImageSource::Render(image))
                        .size_full()
                        .object_fit(ObjectFit::Contain)
                        .into_any_element()
                }
            }
            _ => div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(rgb(TEXT_DIM))
                .child("Choose another visible photo")
                .into_any_element(),
        };
        let meter = bounds.clone();
        let press_bounds = bounds.clone();
        let info = photo.map(|p| (p.filename.clone(), p.rating));
        div()
            .flex_1()
            .min_w_0()
            .h_full()
            .flex()
            .flex_col()
            .border_1()
            .border_color(hairline())
            .child(
                div()
                    .flex_none()
                    .h(px(34.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(10.))
                    .bg(rgb(bg_chrome()))
                    .child(
                        div()
                            .font_family(SANS)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_size(sp(10.))
                            .text_color(rgb(accent_line()))
                            .child(label),
                    )
                    .child(
                        div()
                            .flex_1()
                            .truncate()
                            .text_size(sp(10.5))
                            .text_color(rgb(TEXT_SECONDARY))
                            .child(info.as_ref().map(|i| i.0.clone()).unwrap_or_default()),
                    )
                    .children(
                        info.map(|(_, rating)| self.review_stars(photo_id.unwrap(), rating, cx)),
                    ),
            )
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .bg(rgb(bg_canvas()))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                            this.viewport_box.set(press_bounds.get());
                            this.zoom_press_start(
                                (ev.position.x.as_f32(), ev.position.y.as_f32()),
                                cx,
                            );
                        }),
                    )
                    .child(
                        canvas(move |b, _, _| meter.set(b), |_, _, _, _| {})
                            .absolute()
                            .size_full(),
                    )
                    .child(image_el),
            )
    }

    fn review_button(
        &self,
        id: &'static str,
        text: &'static str,
        tip: &'static str,
    ) -> Stateful<Div> {
        div()
            .id(id)
            .role(Role::Button)
            .px(px(8.))
            .py(px(4.))
            .rounded(px(3.))
            .border_1()
            .border_color(border_control())
            .text_size(sp(10.5))
            .text_color(rgb(TEXT_SECONDARY))
            .hover(|s| s.bg(rgb(bg_row_hover())))
            .on_hover(self.tip(tip))
            .child(text)
    }

    pub(crate) fn compare_center(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        self.view_scale.set(window.scale_factor());
        let zoom_label = match self.zoom {
            zoom::ZoomLevel::Fit => "Fit",
            zoom::ZoomLevel::Full => "1:1 preview",
            zoom::ZoomLevel::Double => "2× preview",
        };
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .child(self.view_toolbar(window, cx))
            .child(
                div()
                    .flex_none()
                    .h(px(38.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .px(px(10.))
                    .border_b_1()
                    .border_color(hairline())
                    .child(
                        self.review_button("compare-prev", "← Candidate", "Previous candidate")
                            .on_click(
                                cx.listener(|this, _, _, cx| this.step_compare_candidate(-1, cx)),
                            ),
                    )
                    .child(
                        self.review_button("compare-next", "Candidate →", "Next candidate")
                            .on_click(
                                cx.listener(|this, _, _, cx| this.step_compare_candidate(1, cx)),
                            ),
                    )
                    .child(
                        self.review_button("compare-swap", "Swap", "Swap Select and Candidate")
                            .on_click(cx.listener(|this, _, _, cx| this.swap_compare(cx))),
                    )
                    .child(div().flex_1())
                    .child(
                        self.review_button("compare-zoom", zoom_label, "Linked zoom: Fit, 1:1, 2×")
                            .on_click(cx.listener(|this, _, _, cx| this.cycle_zoom(cx))),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .gap(px(6.))
                    .p(px(6.))
                    .child(self.compare_pane(
                        self.state.primary,
                        "SELECT",
                        self.review.left_bounds.clone(),
                        cx,
                    ))
                    .child(self.compare_pane(
                        self.review.candidate,
                        "CANDIDATE",
                        self.review.right_bounds.clone(),
                        cx,
                    )),
            )
            .children(self.filmstrip_strip(cx))
            .when(!self.chrome_minimal(), |d| d.child(self.status_bar(cx)))
    }

    pub(crate) fn survey_center(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let ids = self.survey_ids();
        let mut cards = div()
            .flex()
            .flex_wrap()
            .content_start()
            .gap(px(8.))
            .p(px(10.));
        for photo_id in &ids {
            let Some(photo) = self.find(*photo_id) else {
                continue;
            };
            let image = self
                .large
                .get(photo_id)
                .cloned()
                .or_else(|| self.thumbs.get(photo_id).map(|t| t.image.clone()));
            let id = *photo_id;
            cards = cards.child(
                div()
                    .w(px(230.))
                    .h(px(260.))
                    .flex()
                    .flex_col()
                    .rounded(px(3.))
                    .border_1()
                    .border_color(if Some(id) == self.state.primary {
                        rgb(accent_line()).into()
                    } else {
                        hairline()
                    })
                    .overflow_hidden()
                    .child(
                        div()
                            .relative()
                            .flex_1()
                            .min_h_0()
                            .bg(rgb(bg_canvas()))
                            .children(image.map(|image| {
                                img(ImageSource::Render(image))
                                    .size_full()
                                    .object_fit(ObjectFit::Contain)
                            })),
                    )
                    .child(
                        div()
                            .flex_none()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .p(px(7.))
                            .bg(rgb(bg_chrome()))
                            .child(
                                div()
                                    .truncate()
                                    .text_size(sp(10.5))
                                    .text_color(rgb(TEXT_SECONDARY))
                                    .child(photo.filename.clone()),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .child(self.review_stars(id, photo.rating, cx))
                                    .child(div().flex_1())
                                    .child(
                                        div()
                                            .id(("survey-remove", id as usize))
                                            .role(Role::Button)
                                            .px(px(8.))
                                            .py(px(4.))
                                            .rounded(px(3.))
                                            .border_1()
                                            .border_color(border_control())
                                            .text_size(sp(10.5))
                                            .text_color(rgb(TEXT_SECONDARY))
                                            .hover(|s| s.bg(rgb(bg_row_hover())))
                                            .on_hover(self.tip(
                                                "Hide from Survey only; catalog and selection stay intact",
                                            ))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.review.survey_hidden.insert(id);
                                                cx.notify();
                                            }))
                                            .child("Remove from view"),
                                    ),
                            ),
                    ),
            );
        }
        let empty = ids.is_empty();
        div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_col()
            .child(self.view_toolbar(window, cx))
            .child(
                div()
                    .flex_none()
                    .h(px(34.))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(10.))
                    .border_b_1()
                    .border_color(hairline())
                    .child(format!("Survey · {} visible", ids.len()))
                    .child(div().flex_1())
                    .child(
                        self.review_button(
                            "survey-restore",
                            "Restore hidden",
                            "Restore photos removed from this Survey view",
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.review.survey_hidden.clear();
                            cx.notify();
                        })),
                    ),
            )
            .child(
                div()
                    .id("survey-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .when(empty, |d| {
                        d.flex()
                            .items_center()
                            .justify_center()
                            .text_color(rgb(TEXT_DIM))
                            .child("Select two or more photos, then open Survey (N)")
                    })
                    .when(!empty, |d| d.child(cards)),
            )
            .when(!self.chrome_minimal(), |d| d.child(self.status_bar(cx)))
    }
}
