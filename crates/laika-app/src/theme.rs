//! Design tokens from `design_handoff_laika_ui/README.md`, re-tinted from
//! the handoff's warm greys to neutral greys with a slight cool cast.
//! Surfaces and the accent are runtime-switchable (Settings → Appearance)
//! and read through functions; text greys stay constant.

use gpui_kit::{Hsla, rgba};
use std::sync::atomic::{AtomicU8, AtomicU32, Ordering};

/// Surface palette. Grey is the default (neutral with a slight cool
/// cast); Black drops every surface to near-black for dark rooms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Appearance {
    Grey,
    Black,
}

impl Appearance {
    pub const ALL: [Appearance; 2] = [Appearance::Grey, Appearance::Black];

    pub fn key(self) -> &'static str {
        match self {
            Appearance::Grey => "grey",
            Appearance::Black => "black",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Appearance::Grey => "Grey",
            Appearance::Black => "Black",
        }
    }

    pub fn parse(s: &str) -> Self {
        match s {
            "black" => Appearance::Black,
            _ => Appearance::Grey,
        }
    }
}

struct Surfaces {
    chrome: u32,
    canvas: u32,
    app: u32,
    panel: u32,
    panel_deep: u32,
    well: u32,
    row_active: u32,
    row_hover: u32,
    tab_active: u32,
    segment_active: u32,
    segment_shell: u32,
    segment_hover: u32,
    chip: u32,
    track: u32,
    track_off: u32,
    detent: u32,
    hist_bar: u32,
    hist_bar_library: u32,
}

const GREY: Surfaces = Surfaces {
    chrome: 0x0D0E0F,
    canvas: 0x090A0B,
    app: 0x121314,
    panel: 0x171819,
    panel_deep: 0x131415,
    well: 0x101112,
    row_active: 0x222426,
    row_hover: 0x1C1E20,
    tab_active: 0x212325,
    segment_active: 0x2C2E30,
    segment_shell: 0x1B1D1F,
    segment_hover: 0x252729,
    chip: 0x212325,
    track: 0x252729,
    track_off: 0x292B2D,
    detent: 0x3A3C3E,
    hist_bar: 0x616467,
    hist_bar_library: 0x585B5E,
};

const BLACK: Surfaces = Surfaces {
    chrome: 0x000000,
    canvas: 0x000000,
    app: 0x050505,
    panel: 0x0A0A0A,
    panel_deep: 0x070707,
    well: 0x000000,
    row_active: 0x1A1A1A,
    row_hover: 0x141414,
    tab_active: 0x181818,
    segment_active: 0x242424,
    segment_shell: 0x111111,
    segment_hover: 0x1C1C1C,
    chip: 0x161616,
    track: 0x222222,
    track_off: 0x262626,
    detent: 0x383838,
    hist_bar: 0x5E5E5E,
    hist_bar_library: 0x555555,
};

static APPEARANCE: AtomicU8 = AtomicU8::new(0);
static ACCENT: AtomicU32 = AtomicU32::new(ACCENT_DEFAULT);

pub fn appearance() -> Appearance {
    match APPEARANCE.load(Ordering::Relaxed) {
        1 => Appearance::Black,
        _ => Appearance::Grey,
    }
}

pub fn set_appearance(a: Appearance) {
    APPEARANCE.store(a as u8, Ordering::Relaxed);
}

/// (chrome, panel, row) swatches of a palette, for the Settings preview.
pub fn preview(a: Appearance) -> (u32, u32, u32) {
    let s = match a {
        Appearance::Grey => &GREY,
        Appearance::Black => &BLACK,
    };
    (s.canvas, s.panel, s.segment_active)
}

fn surfaces() -> &'static Surfaces {
    match appearance() {
        Appearance::Grey => &GREY,
        Appearance::Black => &BLACK,
    }
}

pub fn bg_chrome() -> u32 {
    surfaces().chrome
}
pub fn bg_canvas() -> u32 {
    surfaces().canvas
}
pub fn bg_app() -> u32 {
    surfaces().app
}
pub fn bg_panel() -> u32 {
    surfaces().panel
}
pub fn bg_panel_deep() -> u32 {
    surfaces().panel_deep
}
pub fn bg_well() -> u32 {
    surfaces().well
}
pub fn bg_row_active() -> u32 {
    surfaces().row_active
}
pub fn bg_row_hover() -> u32 {
    surfaces().row_hover
}
pub fn bg_tab_active() -> u32 {
    surfaces().tab_active
}
pub fn bg_segment_active() -> u32 {
    surfaces().segment_active
}
pub fn bg_segment_shell() -> u32 {
    surfaces().segment_shell
}
pub fn bg_segment_hover() -> u32 {
    surfaces().segment_hover
}
pub fn bg_chip() -> u32 {
    surfaces().chip
}
pub fn track() -> u32 {
    surfaces().track
}
pub fn track_off() -> u32 {
    surfaces().track_off
}
pub fn detent() -> u32 {
    surfaces().detent
}
pub fn hist_bar() -> u32 {
    surfaces().hist_bar
}
pub fn hist_bar_library() -> u32 {
    surfaces().hist_bar_library
}

pub const TEXT_PRIMARY: u32 = 0xDEE4EA;
pub const TEXT_SECONDARY: u32 = 0xB1B6BB;
pub const TEXT_TERTIARY: u32 = 0x9DA1A5;
pub const TEXT_MUTED: u32 = 0x808488;
pub const TEXT_DIM: u32 = 0x66696C;
pub const TEXT_DIMMER: u32 = 0x74777A;
pub const WARNING: u32 = 0xE5C860;

/// Accent choices offered in Settings: (label, line color). Any other
/// `#RRGGBB` is accepted as a custom accent.
pub const ACCENT_DEFAULT: u32 = 0x00C227;
pub const ACCENT_PRESETS: [(&str, u32); 8] = [
    ("Green", ACCENT_DEFAULT),
    ("Blue", 0x3D8BFF),
    ("Teal", 0x1FB8A8),
    ("Violet", 0x9A7BFF),
    ("Pink", 0xF0609E),
    ("Orange", 0xF08A3C),
    ("Gold", 0xD9AE3A),
    ("Silver", 0xB8C0C8),
];

/// The accent's line color (text, rules, active states). Fills and the
/// on-fill text derive from it so any hue stays legible.
pub fn accent() -> u32 {
    ACCENT.load(Ordering::Relaxed)
}

pub fn set_accent(line: u32) {
    ACCENT.store(line & 0xFFFFFF, Ordering::Relaxed);
}

fn scale(c: u32, k: f32) -> u32 {
    let ch = |shift: u32| ((((c >> shift) & 0xFF) as f32 * k).round().min(255.) as u32) << shift;
    ch(16) | ch(8) | ch(0)
}

fn toward_white(c: u32, t: f32) -> u32 {
    let ch = |shift: u32| {
        let v = ((c >> shift) & 0xFF) as f32;
        ((v + (255. - v) * t).round() as u32) << shift
    };
    ch(16) | ch(8) | ch(0)
}

pub fn accent_line() -> u32 {
    accent()
}
pub fn accent_wash() -> u32 {
    accent()
}
pub fn accent_fill() -> u32 {
    match accent() {
        ACCENT_DEFAULT => 0x007B12,
        c => scale(c, 0.63),
    }
}
pub fn accent_fill_hover() -> u32 {
    match accent() {
        ACCENT_DEFAULT => 0x00921A,
        c => scale(c, 0.75),
    }
}
pub fn accent_on_fill() -> u32 {
    match accent() {
        ACCENT_DEFAULT => 0xDFFFE4,
        c => toward_white(c, 0.88),
    }
}
pub fn progress_fill() -> u32 {
    accent_fill()
}

/// `#RRGGBB` / `RRGGBB` / `#RGB` → color.
pub fn parse_hex(s: &str) -> Option<u32> {
    let h = s.trim().trim_start_matches('#');
    let full: String = match h.len() {
        3 => h.chars().flat_map(|c| [c, c]).collect(),
        6 => h.to_string(),
        _ => return None,
    };
    u32::from_str_radix(&full, 16).ok()
}

pub fn hex(c: u32) -> String {
    format!("#{:06X}", c & 0xFFFFFF)
}

pub fn hairline() -> Hsla {
    rgba(0xFFFFFF12).into()
}
pub fn border_control() -> Hsla {
    rgba(0xFFFFFF1A).into()
}
pub fn border_strong() -> Hsla {
    rgba(0xFFFFFF24).into()
}
pub fn scrim() -> Hsla {
    rgba(0x08090AC7).into()
}

/// The macOS system UI font (SF Pro). Numbers render with tabular figures
/// app-wide (set at the window root) so counts and slider readouts never
/// jitter; there is no separate monospace face.
pub const SANS: &str = ".SystemUIFont";

/// (family, weight-name, size px, tracking em) per README typography table.
pub mod type_style {
    pub const WORDMARK: (&str, &str, f32, f32) = ("mono", "semibold", 13., 0.22);
    pub const MODULE_TAB: (&str, &str, f32, f32) = ("sans", "medium", 11., 0.09);
    pub const SECTION_HEADER: (&str, &str, f32, f32) = ("mono", "medium", 9.5, 0.14);
    pub const LIST_ITEM: f32 = 11.5;
    pub const SLIDER_LABEL: f32 = 11.;
    pub const NUMERIC: f32 = 10.5;
    pub const COUNT: f32 = 10.;
}

/// Hover/transition durations from the README: 120ms ease-out on
/// hover/background, 160ms panel expand. GPUI has no CSS transitions;
/// keep state flips instant and mechanical, no animated slider drag.
pub mod motion {
    pub const HOVER_MS: u64 = 120;
    pub const PANEL_MS: u64 = 160;
}

/// Fixed dimensions from the README.
pub mod layout {
    pub const TOP_BAR: f32 = 46.;
    pub const TOOLBAR: f32 = 40.;
    pub const DEVELOP_TOOLBAR: f32 = 38.;
    pub const STATUS_BAR: f32 = 28.;
    pub const FILMSTRIP: f32 = 96.;
    pub const FILM_CELL_W: f32 = 106.;
    pub const FILM_CELL_H: f32 = 72.;
    pub const RAIL_LIBRARY_LEFT: f32 = 226.;
    pub const RAIL_LIBRARY_RIGHT: f32 = 290.;
    pub const RAIL_DEVELOP_LEFT: f32 = 210.;
    pub const RAIL_DEVELOP_RIGHT: f32 = 306.;
    pub const DIALOG_W: f32 = 1020.;
    pub const HIST_H: f32 = 78.;
    pub const HIST_H_LIBRARY: f32 = 74.;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_parsing() {
        assert_eq!(parse_hex("#3D8BFF"), Some(0x3D8BFF));
        assert_eq!(parse_hex("e0457b"), Some(0xE0457B));
        assert_eq!(parse_hex("#fff"), Some(0xFFFFFF));
        assert_eq!(parse_hex("#12345"), None);
        assert_eq!(parse_hex("zzzzzz"), None);
        assert_eq!(hex(0x00C227), "#00C227");
        assert_eq!(
            Appearance::parse(Appearance::Black.key()),
            Appearance::Black
        );
        assert_eq!(Appearance::parse(""), Appearance::Grey);
    }

    #[test]
    fn derived_accent_stays_legible() {
        assert_eq!(scale(0x3D8BFF, 0.63), 0x2658A1);
        assert_eq!(toward_white(0x000000, 0.88), 0xE0E0E0);
    }
}
