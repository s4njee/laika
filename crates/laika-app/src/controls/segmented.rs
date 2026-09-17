//! Segmented control: `#1F1D1A` pill, `padding: 2px`; each segment
//! `padding: 4px 9px`, Mono 500 / 10.5px / .06em; active `#332F29` + `#E9E5DE`.

use crate::theme::*;
use crate::tracked::tr;
use gpui_kit::prelude::*;
use gpui_kit::*;

pub fn segmented(items: &[&str], active: usize, window: &mut Window) -> Div {
    div()
        .flex()
        .bg(rgb(bg_segment_shell()))
        .rounded(px(4.))
        .p(px(2.))
        .children(items.iter().enumerate().map(|(i, name)| {
            let on = i == active;
            div()
                .px(px(9.))
                .py(px(4.))
                .rounded(px(3.))
                .when(on, |d| d.bg(rgb(bg_segment_active())))
                .child(tr(
                    *name,
                    SANS,
                    FontWeight::MEDIUM,
                    10.5,
                    0.06,
                    rgb(if on { TEXT_PRIMARY } else { TEXT_MUTED }),
                    window,
                ))
        }))
}
