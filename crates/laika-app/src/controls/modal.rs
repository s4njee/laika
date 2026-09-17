//! Modal: 78% scrim + 1020px centered dialog on the panel surface,
//! hairline-control border, radius 6px, single allowed shadow
//! `0 30px 80px rgba(0,0,0,.6)`. The scrim occludes, so clicks and hovers
//! never reach the views behind a dialog.

use crate::theme::*;
use gpui_kit::prelude::*;
use gpui_kit::*;

pub fn modal_shell(content: impl IntoElement) -> Div {
    modal_shell_w(content, layout::DIALOG_W)
}

/// Same shell at a custom width (compact settings dialogs).
pub fn modal_shell_w(content: impl IntoElement, width: f32) -> Div {
    div()
        .absolute()
        .size_full()
        .flex()
        .items_center()
        .justify_center()
        .bg(scrim())
        .occlude()
        .child(
            div()
                .w(px(width))
                .bg(rgb(bg_panel()))
                .border_1()
                .border_color(border_control())
                .rounded(px(6.))
                .shadow(vec![gpui_kit::BoxShadow {
                    color: rgba(0x00000099).into(),
                    offset: point(px(0.), px(30.)),
                    blur_radius: px(80.),
                    spread_radius: px(0.),
                    inset: false,
                }])
                .child(content),
        )
}
