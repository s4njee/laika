//! CSS-style letter-spacing. GPUI's `TextStyle` has no tracking field, so
//! this shapes the line once (kerning intact) and paints each glyph shifted
//! right. Carried from the Phase 0 spike (`spikes/p0/src/tracked.rs`).

use gpui_kit::*;

pub struct Tracked {
    pub width: Pixels,
}

pub fn tracked(
    text: impl Into<SharedString>,
    family: &'static str,
    weight: FontWeight,
    size: f32,
    tracking_em: f32,
    color: impl Into<Hsla>,
    window: &mut Window,
) -> (Div, Tracked) {
    let text: SharedString = text.into();
    let color = color.into();
    let font_size = px(size);
    let run = TextRun {
        len: text.len(),
        font: Font {
            weight,
            ..font(family)
        },
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let line = window
        .text_system()
        .shape_line(text, font_size, &[run], None);
    let tracking = px(size * tracking_em);
    let glyphs: usize = line.runs.iter().map(|r| r.glyphs.len()).sum();
    let width = line.width + tracking * glyphs as f32;
    let line_height = px((size * 1.3).ceil());

    let el = div().w(width).h(line_height).flex_none().child(
        canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                let pad = (line_height - line.ascent - line.descent) / 2.;
                let baseline = bounds.origin + point(px(0.), pad + line.ascent);
                let mut i = 0usize;
                for run in line.runs.iter() {
                    for g in &run.glyphs {
                        let origin =
                            baseline + point(g.position.x + tracking * i as f32, g.position.y);
                        window
                            .paint_glyph(origin, run.font_id, g.id, font_size, color)
                            .ok();
                        i += 1;
                    }
                }
            },
        )
        .size_full(),
    );
    (el, Tracked { width })
}

/// Shorthand when the caller doesn't need the measured width.
pub fn tr(
    text: impl Into<SharedString>,
    family: &'static str,
    weight: FontWeight,
    size: f32,
    tracking_em: f32,
    color: impl Into<Hsla>,
    window: &mut Window,
) -> Div {
    tracked(text, family, weight, size, tracking_em, color, window).0
}
