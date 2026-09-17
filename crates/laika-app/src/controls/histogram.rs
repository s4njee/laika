//! Histogram: 78px (74px library), `#121110`, hairline border, radius 3px,
//! `padding: 5px`. 48 flex-1 bars, 1px gap, top radius 1px.

use crate::theme::*;
use gpui_kit::prelude::*;
use gpui_kit::*;

pub fn histogram(values: &[f32; 48], library: bool) -> Div {
    div()
        .h(px(if library {
            layout::HIST_H_LIBRARY
        } else {
            layout::HIST_H
        }))
        .p(px(5.))
        .flex()
        .items_end()
        .gap(px(1.))
        .bg(rgb(bg_well()))
        .border_1()
        .border_color(hairline())
        .rounded(px(3.))
        .children(values.iter().map(|h| {
            div()
                .flex_1()
                .h(relative(h.clamp(0.02, 1.)))
                .rounded_t(px(1.))
                .bg(rgb(if library {
                    hist_bar_library()
                } else {
                    hist_bar()
                }))
        }))
}
