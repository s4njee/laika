//! Toggle: 26x14 pill, radius 8. On = accent-fill track + 10px on-fill knob at
//! left 14px; off = track #2E2B27 + #7E786E knob at left 2px.

use crate::theme::*;
use gpui_kit::prelude::*;
use gpui_kit::*;

pub fn toggle(on: bool, label: &str) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(9.))
        .child(
            div()
                .relative()
                .w(px(26.))
                .h(px(14.))
                .rounded(px(8.))
                .bg(rgb(if on { accent_fill() } else { track_off() }))
                .child(
                    div()
                        .absolute()
                        .top(px(2.))
                        .left(px(if on { 14. } else { 2. }))
                        .size(px(10.))
                        .rounded_full()
                        .bg(rgb(if on { accent_on_fill() } else { TEXT_DIMMER })),
                ),
        )
        .child(
            div()
                .text_size(sp(11.5))
                .text_color(rgb(TEXT_SECONDARY))
                .child(label.to_string()),
        )
}
