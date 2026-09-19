//! Buttons: primary accent fill (on-fill text, lighter fill on hover),
//! outline (`1px rgba(255,255,255,.14)`, hover brighter border).

use crate::theme::*;
use gpui_kit::prelude::*;
use gpui_kit::*;

pub fn primary(label: &str) -> Div {
    div()
        .flex()
        .justify_center()
        .px(px(16.))
        .py(px(9.))
        .rounded(px(3.))
        .bg(rgb(accent_fill()))
        .hover(|s| s.bg(rgb(accent_fill_hover())))
        .text_color(rgb(accent_on_fill()))
        .font_weight(FontWeight::SEMIBOLD)
        .text_size(sp(11.5))
        .child(label.to_string())
}

pub fn outline(label: &str) -> Div {
    div()
        .flex()
        .justify_center()
        .px(px(16.))
        .py(px(9.))
        .rounded(px(3.))
        .border_1()
        .border_color(border_strong())
        .hover(|s| s.border_color(rgba(0xFFFFFF3D)))
        .text_color(rgb(TEXT_SECONDARY))
        .font_weight(FontWeight::MEDIUM)
        .text_size(sp(11.))
        .child(label.to_string())
}
