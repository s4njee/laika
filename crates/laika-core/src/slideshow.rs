//! V20: Loupe info overlay and slideshow — the pure parts (cycle order,
//! line templates, playback scope and stepping, persisted prefs). The app
//! only lays these out and drives the timer.

use std::collections::BTreeSet;

/// Loupe info overlay cycle (`I`): nothing, the file line, or the
/// exposure line. Remembered per catalog.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LoupeInfo {
    #[default]
    None,
    File,
    Exposure,
}

impl LoupeInfo {
    pub fn parse(s: &str) -> Self {
        match s {
            "file" => LoupeInfo::File,
            "exposure" => LoupeInfo::Exposure,
            _ => LoupeInfo::None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            LoupeInfo::None => "none",
            LoupeInfo::File => "file",
            LoupeInfo::Exposure => "exposure",
        }
    }

    /// `I` order: none → file → exposure → none.
    pub fn cycle(self) -> Self {
        match self {
            LoupeInfo::None => LoupeInfo::File,
            LoupeInfo::File => LoupeInfo::Exposure,
            LoupeInfo::Exposure => LoupeInfo::None,
        }
    }
}

pub const DEFAULT_FILE_LINE: &str = "{filename} · {captured} · {dims}";
pub const DEFAULT_EXPOSURE_LINE: &str = "{exposure} · {focal} · {camera} · {lens}";

/// Every token a line template understands, for the settings hint.
pub const TOKENS: [&str; 16] = [
    "filename",
    "captured",
    "date",
    "time",
    "dims",
    "megapixels",
    "camera",
    "lens",
    "focal",
    "aperture",
    "shutter",
    "iso",
    "exposure",
    "rating",
    "flag",
    "title",
];

/// Display values for one photo (raw catalog strings are fine; EXIF
/// ASCII quotes are stripped here).
#[derive(Clone, Debug, Default)]
pub struct InfoFields {
    pub filename: String,
    pub captured_at: String,
    pub width: u32,
    pub height: u32,
    pub camera: String,
    pub lens: String,
    pub focal: String,
    pub aperture: String,
    pub shutter: String,
    pub iso: String,
    pub rating: u8,
    pub picked: bool,
    pub rejected: bool,
    pub title: String,
}

impl InfoFields {
    fn token(&self, name: &str) -> Option<String> {
        let clean = |s: &str| s.trim().trim_matches('"').trim().to_string();
        let captured = clean(&self.captured_at);
        let (date, time) = match captured.split_once(' ') {
            Some((d, t)) => (d.to_string(), t.to_string()),
            None => (captured.clone(), String::new()),
        };
        let dims = if self.width > 0 && self.height > 0 {
            format!("{}×{}", self.width, self.height)
        } else {
            String::new()
        };
        Some(match name {
            "filename" => clean(&self.filename),
            "captured" => captured,
            // `captured` split at the space: `2026-08-25` / `11:15:56`.
            "date" => date,
            "time" => time,
            "dims" => dims,
            "megapixels" => {
                let mp = self.width as f64 * self.height as f64 / 1_000_000.;
                if mp > 0. {
                    format!("{mp:.1} MP")
                } else {
                    String::new()
                }
            }
            "camera" => clean(&self.camera),
            "lens" => clean(&self.lens),
            "focal" => clean(&self.focal),
            "aperture" => clean(&self.aperture),
            "shutter" => clean(&self.shutter),
            "iso" => clean(&self.iso),
            "exposure" => [
                clean(&self.aperture),
                clean(&self.shutter),
                clean(&self.iso),
            ]
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" · "),
            "rating" => "★".repeat(self.rating.min(5) as usize),
            "flag" => {
                if self.picked {
                    "Picked".to_string()
                } else if self.rejected {
                    "Rejected".to_string()
                } else {
                    String::new()
                }
            }
            "title" => self.title.trim().to_string(),
            _ => return None,
        })
    }
}

/// Punctuation that only joins values. Literal text made of these alone
/// disappears next to an empty value, so lines never dangle a separator.
fn is_joiner(s: &str) -> bool {
    s.chars()
        .all(|c| c.is_whitespace() || "·•|,;:/-–—()[]".contains(c))
}

enum Part {
    Text(String),
    Value(String),
}

/// Fill a line template. `{token}` becomes the photo's value; unknown
/// tokens stay literal (so typos are visible); empty values drop along
/// with the joiner text around them.
pub fn info_line(template: &str, f: &InfoFields) -> String {
    let mut parts: Vec<Part> = Vec::new();
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        let Some(close) = rest[open..].find('}').map(|c| open + c) else {
            break;
        };
        if open > 0 {
            parts.push(Part::Text(rest[..open].to_string()));
        }
        let name = &rest[open + 1..close];
        match f.token(name.trim()) {
            Some(v) => parts.push(Part::Value(v)),
            None => parts.push(Part::Text(rest[open..=close].to_string())),
        }
        rest = &rest[close + 1..];
    }
    if !rest.is_empty() {
        parts.push(Part::Text(rest.to_string()));
    }

    // Empty values go; joiner text that now touches another joiner or an
    // edge goes with them.
    let mut kept: Vec<Part> = Vec::new();
    for part in parts {
        match part {
            Part::Value(v) if v.is_empty() => {}
            Part::Text(t) => {
                if let Some(Part::Text(prev)) = kept.last_mut() {
                    if is_joiner(prev) && is_joiner(&t) {
                        // Two joiners met across a dropped value: keep one.
                        continue;
                    }
                    prev.push_str(&t);
                } else {
                    kept.push(Part::Text(t));
                }
            }
            v => kept.push(v),
        }
    }
    while matches!(kept.first(), Some(Part::Text(t)) if is_joiner(t)) {
        kept.remove(0);
    }
    while matches!(kept.last(), Some(Part::Text(t)) if is_joiner(t)) {
        kept.pop();
    }
    let mut out = String::new();
    for part in kept {
        match part {
            Part::Text(t) | Part::Value(t) => out.push_str(&t),
        }
    }
    out.trim().to_string()
}

/// Slideshow settings, persisted per catalog.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SlideshowPrefs {
    pub interval_secs: u32,
    pub looped: bool,
    pub fade: bool,
    pub caption: bool,
}

impl Default for SlideshowPrefs {
    fn default() -> Self {
        Self {
            interval_secs: 5,
            looped: true,
            fade: true,
            caption: true,
        }
    }
}

pub const INTERVALS: [u32; 6] = [2, 3, 5, 8, 15, 30];
pub const FADE_MS: u64 = 450;

impl SlideshowPrefs {
    /// Stored as `interval=5,loop=1,fade=1,caption=1`; unknown or broken
    /// parts keep their defaults.
    pub fn parse(s: &str) -> Self {
        let mut out = Self::default();
        for part in s.split(',') {
            let Some((k, v)) = part.split_once('=') else {
                continue;
            };
            let on = v.trim() == "1";
            match k.trim() {
                "interval" => {
                    if let Ok(n) = v.trim().parse::<u32>() {
                        out.interval_secs = n.clamp(1, 60);
                    }
                }
                "loop" => out.looped = on,
                "fade" => out.fade = on,
                "caption" => out.caption = on,
                _ => {}
            }
        }
        out
    }

    pub fn serialize(&self) -> String {
        format!(
            "interval={},loop={},fade={},caption={}",
            self.interval_secs, self.looped as u8, self.fade as u8, self.caption as u8
        )
    }
}

/// What plays: a multi-photo selection (in visible order), otherwise
/// everything visible — the folder, album, or filter on screen. Starts
/// on the primary when it is part of the show.
pub fn slide_scope(
    ordered: &[i64],
    selection: &BTreeSet<i64>,
    primary: Option<i64>,
) -> (Vec<i64>, usize) {
    let picked: Vec<i64> = ordered
        .iter()
        .copied()
        .filter(|id| selection.contains(id))
        .collect();
    let ids = if picked.len() > 1 {
        picked
    } else {
        ordered.to_vec()
    };
    let start = primary
        .and_then(|p| ids.iter().position(|&x| x == p))
        .unwrap_or(0);
    (ids, start)
}

/// Next slide index for a step, or `None` when a non-looping show runs
/// off either end.
pub fn step(idx: usize, dir: i32, len: usize, looped: bool) -> Option<usize> {
    if len == 0 {
        return None;
    }
    let next = idx as i64 + dir as i64;
    if (0..len as i64).contains(&next) {
        Some(next as usize)
    } else if looped {
        Some(next.rem_euclid(len as i64) as usize)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nikon() -> InfoFields {
        InfoFields {
            filename: "DSC_0042.NEF".into(),
            captured_at: "2026-08-25 11:15:56".into(),
            width: 6016,
            height: 4016,
            camera: "\"NIKON D600\"".into(),
            lens: "".into(),
            focal: "50mm".into(),
            aperture: "ƒ/1.4".into(),
            shutter: "1/160".into(),
            iso: "ISO 2000".into(),
            rating: 3,
            picked: true,
            rejected: false,
            title: "".into(),
        }
    }

    #[test]
    fn cycle_and_parse_round_trip() {
        let mut m = LoupeInfo::None;
        let mut seen = Vec::new();
        for _ in 0..3 {
            m = m.cycle();
            seen.push(m);
            assert_eq!(LoupeInfo::parse(m.label()), m);
        }
        assert_eq!(
            seen,
            [LoupeInfo::File, LoupeInfo::Exposure, LoupeInfo::None]
        );
        assert_eq!(LoupeInfo::parse("garbage"), LoupeInfo::None);
    }

    #[test]
    fn default_lines_fill_and_drop_empties() {
        let f = nikon();
        assert_eq!(
            info_line(DEFAULT_FILE_LINE, &f),
            "DSC_0042.NEF · 2026-08-25 11:15:56 · 6016×4016"
        );
        // No lens: its value and one joiner vanish, quotes are stripped.
        assert_eq!(
            info_line(DEFAULT_EXPOSURE_LINE, &f),
            "ƒ/1.4 · 1/160 · ISO 2000 · 50mm · NIKON D600"
        );
    }

    #[test]
    fn templates_never_dangle_separators() {
        let f = nikon();
        assert_eq!(info_line("{title} · {filename}", &f), "DSC_0042.NEF");
        assert_eq!(info_line("{filename} | {lens}", &f), "DSC_0042.NEF");
        assert_eq!(info_line("{filename} ({lens})", &f), "DSC_0042.NEF");
        assert_eq!(info_line("{lens} · {title}", &f), "");
        assert_eq!(
            info_line("{date} at {time} · {megapixels}", &f),
            "2026-08-25 at 11:15:56 · 24.2 MP"
        );
        assert_eq!(info_line("{rating} {flag}", &f), "★★★ Picked");
        // Literal words stay; unknown tokens stay visible as typed.
        assert_eq!(
            info_line("Shot on {camera}, {nope}", &f),
            "Shot on NIKON D600, {nope}"
        );
        assert_eq!(info_line("no tokens", &f), "no tokens");
        assert_eq!(info_line("{unclosed", &f), "{unclosed");
    }

    #[test]
    fn prefs_round_trip_and_clamp() {
        let p = SlideshowPrefs {
            interval_secs: 8,
            looped: false,
            fade: true,
            caption: false,
        };
        assert_eq!(SlideshowPrefs::parse(&p.serialize()), p);
        assert_eq!(SlideshowPrefs::parse(""), SlideshowPrefs::default());
        assert_eq!(SlideshowPrefs::parse("interval=900").interval_secs, 60);
        assert_eq!(SlideshowPrefs::parse("interval=x,loop=0").interval_secs, 5);
        assert!(!SlideshowPrefs::parse("interval=x,loop=0").looped);
    }

    #[test]
    fn scope_prefers_a_multi_selection_in_visible_order() {
        let ordered = [10, 11, 12, 13, 14];
        let sel: BTreeSet<i64> = [14, 11, 99].into_iter().collect();
        // 99 is hidden by the filter: never played.
        assert_eq!(slide_scope(&ordered, &sel, Some(14)), (vec![11, 14], 1));
        // A single selected photo plays everything visible from itself.
        let one: BTreeSet<i64> = [12].into_iter().collect();
        assert_eq!(slide_scope(&ordered, &one, Some(12)), (ordered.to_vec(), 2));
        // Primary outside the show starts at the beginning.
        assert_eq!(slide_scope(&ordered, &sel, Some(10)).1, 0);
        assert_eq!(slide_scope(&[], &sel, None), (vec![], 0));
    }

    #[test]
    fn stepping_loops_or_stops() {
        assert_eq!(step(0, 1, 3, false), Some(1));
        assert_eq!(step(2, 1, 3, false), None);
        assert_eq!(step(2, 1, 3, true), Some(0));
        assert_eq!(step(0, -1, 3, true), Some(2));
        assert_eq!(step(0, -1, 3, false), None);
        assert_eq!(step(0, 1, 0, true), None);
        assert_eq!(step(0, 1, 1, true), Some(0));
    }
}
