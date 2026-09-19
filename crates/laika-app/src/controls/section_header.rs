//! Section header: system font, 11px semibold, title case, muted — quiet
//! labels that group rows without shouting. Active variant (Develop Basic,
//! modified panels) in green. `padding: 16px 14px 6px`.

use crate::theme::*;
use gpui_kit::prelude::*;
use gpui_kit::*;

pub fn section_header(text: &str, active: bool, _window: &mut Window) -> Div {
    div()
        .px(px(14.))
        .pt(px(16.))
        .pb(px(6.))
        .font_family(SANS)
        .text_size(sp(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(if active { accent_line() } else { TEXT_MUTED }))
        .child(text.to_string())
}
