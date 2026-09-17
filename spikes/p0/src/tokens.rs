//! Design tokens from design_handoff_laika_ui/README.md.
#![allow(dead_code)]

use gpui_kit::{Hsla, rgba};

pub const BG_CHROME: u32 = 0x0F0E0D;
pub const BG_CANVAS: u32 = 0x0B0A0A;
pub const BG_APP: u32 = 0x141312;
pub const BG_PANEL: u32 = 0x1A1816;
pub const BG_WELL: u32 = 0x121110;
pub const BG_ROW_ACTIVE: u32 = 0x272420;
pub const BG_ROW_HOVER: u32 = 0x201E1B;
pub const BG_TAB_ACTIVE: u32 = 0x26241F;
pub const BG_SEGMENT_ACTIVE: u32 = 0x332F29;
pub const TRACK: u32 = 0x2A2723;
pub const DETENT: u32 = 0x413C36;
pub const HIST_BAR: u32 = 0x6B655C;
pub const TEXT_PRIMARY: u32 = 0xE9E5DE;
pub const TEXT_SECONDARY: u32 = 0xBDB7AE;
pub const TEXT_TERTIARY: u32 = 0xA8A29A;
pub const TEXT_MUTED: u32 = 0x8B857C;
pub const TEXT_DIM: u32 = 0x6F6A62;
pub const TEXT_DIMMER: u32 = 0x7E786E;
pub const ACCENT_FILL: u32 = 0x007B12;
pub const ACCENT_ON_FILL: u32 = 0xDFFFE4;
pub const ACCENT_LINE: u32 = 0x00C227;
pub const WARNING: u32 = 0xE5C860;

pub fn hairline() -> Hsla {
    rgba(0xFFFFFF12).into()
}
pub fn border_control() -> Hsla {
    rgba(0xFFFFFF1A).into()
}
pub fn border_strong() -> Hsla {
    rgba(0xFFFFFF24).into()
}

pub const SANS: &str = "IBM Plex Sans";
pub const MONO: &str = "IBM Plex Mono";
