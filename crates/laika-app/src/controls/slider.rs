//! Slider control (README shared spec + spike behavior).
//! Row: label left (Sans 11px #A8A29A), value right (Mono 10.5px, green when
//! modified). Track 2px #2A2723, detent 1x6, knob 10px (9px library variant).
//! Interactions: drag knob, scrub anywhere on row, double-click resets,
//! arrows nudge (shift x10) — wired by the owning view.

use crate::theme::*;
use gpui_kit::prelude::*;
use gpui_kit::*;
use laika_core::edit::ParamDef;

pub const KNOB: f32 = 10.;
pub const KNOB_LIBRARY: f32 = 9.;

pub struct SliderProps {
    pub frac: f32,
    pub modified: bool,
    pub knob: f32,
}

pub fn slider_chrome(
    def: &ParamDef,
    value: impl IntoElement,
    props: &SliderProps,
    focused: bool,
) -> Div {
    let accent = rgb(if props.modified {
        accent_line()
    } else {
        TEXT_PRIMARY
    });
    div()
        .flex()
        .flex_col()
        .gap(px(5.))
        .child(
            div()
                .flex()
                .justify_between()
                .items_center()
                .child(
                    div()
                        .text_size(sp(11.))
                        .text_color(rgb(if focused { TEXT_PRIMARY } else { TEXT_TERTIARY }))
                        .child(def.label),
                )
                .child(value),
        )
        .child(
            div()
                .relative()
                .h(px(props.knob))
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .right_0()
                        .top(px(4.))
                        .h(px(2.))
                        .bg(rgb(track())),
                )
                .child(
                    div()
                        .absolute()
                        .left(relative(0.5))
                        .top(px(2.))
                        .w(px(1.))
                        .h(px(6.))
                        .bg(rgb(detent())),
                )
                .child(
                    div()
                        .absolute()
                        .left(relative(props.frac))
                        .top_0()
                        .ml(px(-props.knob / 2.))
                        .size(px(props.knob))
                        .rounded_full()
                        .bg(accent),
                ),
        )
}

pub fn frac_of(def: &ParamDef, v: f32) -> f32 {
    ((v - def.min) / (def.max - def.min)).clamp(0., 1.)
}
