//! V13: color labels (Lightroom's five) — names per catalog, sidecar text,
//! filter bits. Label values: 0 = none, 1..=5 = Red, Yellow, Green, Blue,
//! Purple (keys 6–9 set the first four; Purple comes from the menu).

pub const LABEL_COUNT: usize = 5;

/// Lightroom's default label set; also the fallback when reading
/// sidecars written with the stock names.
pub const DEFAULT_NAMES: [&str; LABEL_COUNT] = ["Red", "Yellow", "Green", "Blue", "Purple"];

/// Swatch colors (sRGB hex) for badges and chips.
pub const COLORS: [u32; LABEL_COUNT] = [0xE0524F, 0xE8C547, 0x5DBB63, 0x4F8FE0, 0xA46BD8];

/// Keyboard: `6`..`9` → labels 1..4 (Lightroom).
pub fn label_for_key(key: &str) -> Option<u8> {
    match key {
        "6" => Some(1),
        "7" => Some(2),
        "8" => Some(3),
        "9" => Some(4),
        _ => None,
    }
}

/// Per-catalog label names (editable; blank entries fall back).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LabelNames(pub [String; LABEL_COUNT]);

impl Default for LabelNames {
    fn default() -> Self {
        Self(DEFAULT_NAMES.map(str::to_string))
    }
}

impl LabelNames {
    /// Stored as the five names joined by `|`.
    pub fn parse(s: &str) -> Self {
        let mut out = Self::default();
        for (i, part) in s.split('|').take(LABEL_COUNT).enumerate() {
            let t = part.trim();
            if !t.is_empty() {
                out.0[i] = t.chars().take(40).collect();
            }
        }
        out
    }

    pub fn serialize(&self) -> String {
        self.0
            .iter()
            .map(|n| n.replace('|', "/"))
            .collect::<Vec<_>>()
            .join("|")
    }

    /// Display name for a label value (empty for none).
    pub fn name(&self, label: u8) -> &str {
        match label {
            1..=5 => &self.0[label as usize - 1],
            _ => "",
        }
    }

    /// Set one name; blank restores the default. Names must stay distinct
    /// so sidecar text maps back to exactly one label.
    pub fn set(&mut self, label: u8, name: &str) -> Result<(), String> {
        if !(1..=5).contains(&label) {
            return Err("no such label".to_string());
        }
        let i = label as usize - 1;
        let t = name.trim();
        let t = if t.is_empty() { DEFAULT_NAMES[i] } else { t };
        if t.chars().count() > 40 {
            return Err("keep label names under 40 characters".to_string());
        }
        if t.contains('|') {
            return Err("label names can't contain |".to_string());
        }
        if self
            .0
            .iter()
            .enumerate()
            .any(|(j, n)| j != i && n.eq_ignore_ascii_case(t))
        {
            return Err(format!("another label is already named {t}"));
        }
        self.0[i] = t.to_string();
        Ok(())
    }

    /// `xmp:Label` text → label value. The catalog's own names win, then
    /// Lightroom's stock names; unknown text (e.g. "Select") is None so
    /// the caller can leave the photo untouched.
    pub fn from_xmp(&self, text: &str) -> Option<u8> {
        let t = text.trim();
        if t.is_empty() {
            return Some(0);
        }
        if let Some(i) = self.0.iter().position(|n| n.eq_ignore_ascii_case(t)) {
            return Some(i as u8 + 1);
        }
        DEFAULT_NAMES
            .iter()
            .position(|n| n.eq_ignore_ascii_case(t))
            .map(|i| i as u8 + 1)
    }
}

/// Filter bitmask: bit `label` (0 = no label, 1..=5 = colors). Empty = all.
pub fn matches_mask(mask: u8, label: u8) -> bool {
    mask == 0 || mask & (1 << label.min(5)) != 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_round_trip_and_stay_distinct() {
        let mut n = LabelNames::default();
        assert_eq!(n.name(1), "Red");
        assert_eq!(n.name(0), "");
        n.set(2, "To print").unwrap();
        assert!(n.set(3, "to PRINT").is_err());
        assert!(n.set(3, "a|b").is_err());
        n.set(5, "  ").unwrap();
        assert_eq!(n.name(5), "Purple");
        assert_eq!(LabelNames::parse(&n.serialize()), n);
        assert_eq!(LabelNames::parse(""), LabelNames::default());
        assert_eq!(LabelNames::parse("A||C").name(2), "Yellow");
    }

    #[test]
    fn xmp_text_maps_back() {
        let mut n = LabelNames::default();
        n.set(2, "Review").unwrap();
        assert_eq!(n.from_xmp("Review"), Some(2));
        // Stock Lightroom names still read after a rename.
        assert_eq!(n.from_xmp("yellow"), Some(2));
        assert_eq!(n.from_xmp("Purple"), Some(5));
        assert_eq!(n.from_xmp(""), Some(0));
        assert_eq!(n.from_xmp("Select"), None);
    }

    #[test]
    fn keys_and_mask() {
        assert_eq!(label_for_key("6"), Some(1));
        assert_eq!(label_for_key("9"), Some(4));
        assert_eq!(label_for_key("5"), None);
        assert!(matches_mask(0, 3));
        assert!(matches_mask(0b1010, 1));
        assert!(!matches_mask(0b1010, 2));
        assert!(matches_mask(0b0001, 0));
    }
}
