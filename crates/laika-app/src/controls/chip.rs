//! Filter / keyword / size chip. `padding: 4px 9px` (filter) or `3px 7px`
//! (keyword), `border-radius: 2-3px`. Active filter: green border + green text.

use crate::theme::*;
use gpui_kit::prelude::*;
use gpui_kit::*;

pub fn filter_chip(label: &str, active: bool) -> Div {
    div()
        .px(px(9.))
        .py(px(4.))
        .rounded(px(3.))
        .border_1()
        .border_color::<Hsla>(if active {
            rgb(accent_line()).into()
        } else {
            border_control()
        })
        .text_size(sp(10.5))
        .text_color(rgb(if active {
            accent_line()
        } else {
            TEXT_SECONDARY
        }))
        .hover(|s| s.border_color(border_strong()))
        .child(label.to_string())
}
