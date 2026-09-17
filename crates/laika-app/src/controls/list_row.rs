//! List row: chip + name + count; active/hover backgrounds.
//! `padding: 5px 8px`, radius 3px, 8px gap. Active `#272420`, hover `#201E1B`.

use crate::theme::*;
use gpui_kit::prelude::*;
use gpui_kit::*;

pub fn list_row(name: &str, count: &str, chip: u32, active: bool) -> Div {
    div()
        .flex()
        .items_center()
        .gap(px(8.))
        .px(px(8.))
        .py(px(5.))
        .rounded(px(4.))
        .when(active, |d| d.bg(rgb(bg_row_active())))
        .when(!active, |d| d.hover(|s| s.bg(rgb(bg_row_hover()))))
        .child(div().size(px(5.)).rounded(px(1.)).bg(rgb(chip)))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_size(px(12.))
                .text_color(rgb(if active { TEXT_PRIMARY } else { TEXT_SECONDARY }))
                .child(name.to_string()),
        )
        .child(
            div()
                .font_family(SANS)
                .flex_none()
                .text_size(px(11.))
                .text_color(rgb(TEXT_DIM))
                .child(count.to_string()),
        )
}
