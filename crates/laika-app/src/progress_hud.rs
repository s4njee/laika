//! Floating progress card for long background work (imports, Apple Photos
//! sync). It sits above the status bar without blocking the library, shows
//! count, rate and time left, and can be hidden (the footer keeps the same
//! numbers) or cancel an import.

use super::*;

const HUD_W: f32 = 460.;

/// `12345` → `12,345`.
fn grouped(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn eta(secs: f32) -> String {
    if secs < 90. {
        format!("~{:.0}s left", secs.max(1.))
    } else if secs < 5400. {
        format!("~{:.0} min left", secs / 60.)
    } else {
        format!("~{:.1} h left", secs / 3600.)
    }
}

impl Laika {
    pub(crate) fn progress_hud(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<Stateful<Div>> {
        if self.hud_hidden {
            return None;
        }
        // (title, detail, fraction or None for indeterminate, can cancel)
        let (title, detail, frac, cancellable) = if let Some(p) = self.import.as_ref() {
            let source = if p.source_label == "Apple Photos" {
                "Adding photos from Apple Photos".to_string()
            } else {
                format!("Importing from {}", p.source_label)
            };
            let copying = p.transferred < p.total && p.mode_label == "copy";
            if copying {
                let moved = p.copy_bytes.load(Ordering::Relaxed);
                let secs = p.started.elapsed().as_secs_f32().max(0.1);
                let mb_s = moved as f32 / 1024. / 1024. / secs;
                (
                    source,
                    format!(
                        "Copying {} of {} · {:.0} MB/s",
                        grouped(p.transferred + 1),
                        grouped(p.total),
                        mb_s
                    ),
                    Some(p.transferred as f32 / p.total.max(1) as f32),
                    true,
                )
            } else {
                let secs = p
                    .processing_started
                    .unwrap_or(p.started)
                    .elapsed()
                    .as_secs_f32()
                    .max(0.1);
                let rate = p.done as f32 / secs;
                let mut detail = format!("{} of {}", grouped(p.done), grouped(p.total));
                if p.done >= 3 && rate > 0. {
                    detail.push_str(&format!(" · {rate:.0}/s"));
                    if p.total > p.done {
                        detail.push_str(&format!(" · {}", eta((p.total - p.done) as f32 / rate)));
                    }
                }
                let issues = p.failed + p.skipped;
                if issues > 0 {
                    detail.push_str(&format!(" · {} skipped or failed", grouped(issues)));
                }
                (
                    source,
                    detail,
                    Some(p.done as f32 / p.total.max(1) as f32),
                    true,
                )
            }
        } else if self.apple.running {
            let a = &self.apple;
            let detail = if !a.phase.is_empty() {
                a.phase.clone()
            } else {
                format!("Sending {} of {} to Photos", a.done.min(a.total), a.total)
            };
            let frac =
                (a.phase.is_empty() && a.total > 0).then(|| a.done as f32 / a.total.max(1) as f32);
            ("Syncing with Apple Photos".to_string(), detail, frac, false)
        } else {
            return None;
        };

        let vw = window.viewport_size().width.as_f32();
        let button = |id: &'static str, label: &'static str, danger: bool| {
            div()
                .id(id)
                .role(Role::Button)
                .aria_label(label)
                .px(px(10.))
                .py(px(4.))
                .rounded(px(4.))
                .border_1()
                .border_color(border_control())
                .text_size(sp(11.))
                .text_color(rgb(if danger { 0xE56060 } else { TEXT_SECONDARY }))
                .hover(|s| s.bg(rgb(bg_row_hover())))
                .child(label)
        };
        let bar = div()
            .id("progress-hud-bar")
            .role(Role::ProgressIndicator)
            .aria_label(title.clone())
            .when_some(frac, |d, f| d.aria_numeric_value(f as f64 * 100.))
            .aria_min_numeric_value(0.)
            .aria_max_numeric_value(100.)
            .h(px(4.))
            .rounded(px(2.))
            .bg(rgb(track()))
            .overflow_hidden()
            .child(match frac {
                Some(f) => div()
                    .h_full()
                    .w(relative(f.clamp(0., 1.)))
                    .rounded(px(2.))
                    .bg(rgb(progress_fill())),
                // Indeterminate: a dim full bar reads as "working".
                None => div()
                    .h_full()
                    .w_full()
                    .bg(rgba((accent_fill() << 8) | 0x55)),
            });
        Some(
            div()
                .id("progress-hud")
                .occlude()
                .absolute()
                // Above the filmstrip in Develop, above the status bar elsewhere.
                .bottom(px(
                    if self.state.active_module == Module::Develop
                        && self.library.prefs.filmstrip_visible
                        && !self.chrome_minimal()
                    {
                        layout::FILMSTRIP() + layout::DEVELOP_TOOLBAR + 12.
                    } else {
                        layout::STATUS_BAR + 14.
                    },
                ))
                .left(px(((vw - HUD_W) / 2.).max(8.)))
                .w(px(HUD_W))
                .p(px(14.))
                .flex()
                .flex_col()
                .gap(px(9.))
                .rounded(px(10.))
                .bg(rgb(bg_chrome()))
                .border_1()
                .border_color(border_control())
                .shadow_lg()
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(10.))
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_size(sp(12.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(TEXT_PRIMARY))
                                .child(title),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_none()
                                .gap(px(6.))
                                .child(button("hud-hide", "Hide", false).on_click(cx.listener(
                                    |this, _, _, cx| {
                                        this.hud_hidden = true;
                                        cx.notify();
                                    },
                                )))
                                .when(cancellable, |d| {
                                    d.child(button("hud-cancel", "Cancel", true).on_click(
                                        cx.listener(|this, _, _, cx| this.cancel_import(cx)),
                                    ))
                                }),
                        ),
                )
                .child(bar)
                .child(
                    div()
                        .text_size(sp(11.5))
                        .text_color(rgb(TEXT_DIM))
                        .truncate()
                        .child(detail),
                ),
        )
    }
}
