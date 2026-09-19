//! Single-line text field: `padding: 8px 10px`, `#121110`,
//! `1px rgba(255,255,255,.1)`, radius 3px, 12px. URL variant shows a dim
//! prefix with the editable slug in green mono.

use crate::theme::*;
use gpui_kit::prelude::*;
use gpui_kit::*;

pub fn text_field(value: &str) -> Div {
    div()
        .px(px(10.))
        .py(px(8.))
        .bg(rgb(bg_well()))
        .border_1()
        .border_color(border_control())
        .rounded(px(3.))
        .text_size(sp(12.))
        .text_color(rgb(TEXT_PRIMARY))
        .child(value.to_string())
}

pub fn url_field(prefix: &str, slug: &str) -> Div {
    div()
        .flex()
        .px(px(10.))
        .py(px(8.))
        .bg(rgb(bg_well()))
        .border_1()
        .border_color(border_control())
        .rounded(px(3.))
        .font_family(SANS)
        .text_size(sp(11.5))
        .child(div().text_color(rgb(TEXT_DIM)).child(prefix.to_string()))
        .child(div().text_color(rgb(accent_line())).child(slug.to_string()))
}
