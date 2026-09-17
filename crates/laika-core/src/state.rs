//! `AppState` model (plan.md Phase 1). Pure data + pure transitions so the
//! GPUI view stays thin; unit tests pin the filter/selection semantics.

use std::collections::{BTreeSet, HashMap};

use crate::edit::Edit;
use crate::photo::{Photo, SyncState};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Module {
    #[default]
    Library,
    Develop,
    Publish,
}

/// U12: flag dimension (mutually exclusive by construction).
/// U06 adds `Unflagged` (neither picked nor rejected) for culling.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum FlagFilter {
    #[default]
    All,
    Picked,
    Rejected,
    Unflagged,
}

/// U12: file-type dimension (mutually exclusive by construction).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum FileType {
    #[default]
    All,
    Raw,
    Raster,
}

/// U12: sort field + direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum SortField {
    #[default]
    Captured,
    Filename,
    Rating,
    /// V30: the viewed collection's custom order (applied by the app;
    /// without a collection it reads as capture order).
    Album,
}

/// U12: sort direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum SortDir {
    #[default]
    Asc,
    Desc,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub struct SortSpec {
    pub field: SortField,
    pub dir: SortDir,
}

#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Filters {
    pub min_stars: u8,
    pub flag: FlagFilter,
    pub file_type: FileType,
    pub unsynced_only: bool,
    /// U06: show only photos with no star rating yet (culling).
    #[serde(default)]
    pub unrated_only: bool,
    /// U12: substring search over filename, camera, lens, dates, keywords.
    pub search: String,
    /// U17: show only photos whose originals are absent from disk.
    pub missing_only: bool,
    /// U12: exact camera / lens (from catalog distinct lists).
    pub camera: Option<String>,
    pub lens: Option<String>,
    /// U12: capture-date window, `YYYY-MM-DD`.
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    /// U12: relative-folder scope (`"(root)"` = catalog root top level).
    pub folder: Option<String>,
    /// Mirrored Apple Photos album id (membership is resolved by the app).
    #[serde(default)]
    pub album: Option<String>,
    /// The album's name, for labels.
    #[serde(default)]
    pub album_name: String,
    /// V13: color label bitmask (bit 0 = no label, 1..=5 = colors);
    /// 0 = any. Combines with rating and flag filters.
    #[serde(default)]
    pub labels: u8,
    /// V13: collection scope (membership resolved by the app).
    #[serde(default)]
    pub collection: Option<i64>,
    #[serde(default)]
    pub collection_name: String,
    pub sort: SortSpec,
}

impl Filters {
    /// Anything beyond the neutral browse state.
    pub fn is_active(&self) -> bool {
        self.min_stars > 0
            || self.flag != FlagFilter::All
            || self.file_type != FileType::All
            || self.unsynced_only
            || self.unrated_only
            || !self.search.trim().is_empty()
            || self.missing_only
            || self.camera.is_some()
            || self.lens.is_some()
            || self.date_from.is_some()
            || self.date_to.is_some()
            || self.folder.is_some()
            || self.album.is_some()
            || self.labels != 0
            || self.collection.is_some()
    }

    /// Human list of active constraints (empty-results explanation).
    pub fn active_labels(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.min_stars > 0 {
            out.push(format!("★{}+", self.min_stars));
        }
        match self.flag {
            FlagFilter::Picked => out.push("picked".to_string()),
            FlagFilter::Rejected => out.push("rejected".to_string()),
            FlagFilter::Unflagged => out.push("unflagged".to_string()),
            FlagFilter::All => {}
        }
        match self.file_type {
            FileType::Raw => out.push("RAW".to_string()),
            FileType::Raster => out.push("raster".to_string()),
            FileType::All => {}
        }
        if self.unsynced_only {
            out.push("unsynced".to_string());
        }
        if self.unrated_only {
            out.push("unrated".to_string());
        }
        if self.missing_only {
            out.push("missing".to_string());
        }
        if !self.search.trim().is_empty() {
            out.push(format!("“{}”", self.search.trim()));
        }
        if let Some(c) = self.camera.as_ref() {
            out.push(format!("camera {c}"));
        }
        if let Some(l) = self.lens.as_ref() {
            out.push(format!("lens {l}"));
        }
        if self.date_from.is_some() || self.date_to.is_some() {
            out.push(format!(
                "{}…{}",
                self.date_from.as_deref().unwrap_or("…"),
                self.date_to.as_deref().unwrap_or("…")
            ));
        }
        if let Some(f) = self.folder.as_ref() {
            out.push(format!("folder {f}"));
        }
        if self.album.is_some() {
            out.push(format!("album {}", self.album_name));
        }
        if self.labels != 0 {
            let names = ["no label", "red", "yellow", "green", "blue", "purple"];
            let picked: Vec<&str> = (0..6)
                .filter(|b| self.labels & (1 << b) != 0)
                .map(|b| names[b])
                .collect();
            out.push(format!("label {}", picked.join("/")));
        }
        if self.collection.is_some() {
            out.push(format!("collection {}", self.collection_name));
        }
        out
    }

    pub fn matches(&self, p: &Photo, is_raw: bool) -> bool {
        if p.rating < self.min_stars {
            return false;
        }
        if self.unrated_only && p.rating != 0 {
            return false;
        }
        match self.flag {
            FlagFilter::Picked if !p.picked => return false,
            FlagFilter::Rejected if !p.rejected => return false,
            FlagFilter::Unflagged if p.picked || p.rejected => return false,
            _ => {}
        }
        match self.file_type {
            FileType::Raw if !is_raw => return false,
            FileType::Raster if is_raw => return false,
            _ => {}
        }
        if self.unsynced_only && !matches!(p.sync, SyncState::Pending | SyncState::Failed) {
            return false;
        }
        if !crate::labels::matches_mask(self.labels, p.label) {
            return false;
        }
        true
    }

    /// Full match against a catalog row: base fields plus search, camera,
    /// lens, date window, and folder scope.
    pub fn matches_db(&self, p: &crate::catalog::DbPhoto, keywords: &[String]) -> bool {
        let probe = Photo {
            id: p.id,
            filename: p.filename.clone(),
            rating: p.rating,
            picked: p.picked,
            rejected: p.rejected,
            label: p.label,
            sync: p.sync,
            tint: (0, 0),
        };
        if !self.matches(&probe, p.is_raw()) {
            return false;
        }
        if let Some(c) = self.camera.as_ref() {
            if &p.camera != c {
                return false;
            }
        }
        if let Some(l) = self.lens.as_ref() {
            if &p.lens != l {
                return false;
            }
        }
        if self.date_from.is_some() || self.date_to.is_some() {
            let day = normalize_day(&p.captured_at);
            let Some(day) = day else {
                return false;
            };
            if let Some(from) = self.date_from.as_ref() {
                if &day < from {
                    return false;
                }
            }
            if let Some(to) = self.date_to.as_ref() {
                if &day > to {
                    return false;
                }
            }
        }
        if !self.search.trim().is_empty() {
            let hay = search_haystack(&p.filename, &p.camera, &p.lens, &p.captured_at, keywords);
            if !hay.contains(&self.search.trim().to_lowercase()) {
                return false;
            }
        }
        true
    }

    /// Comparator over catalog rows for the sort spec. Filename always
    /// breaks ties so the order is total and stable.
    pub fn compare_db(
        &self,
        a: &crate::catalog::DbPhoto,
        b: &crate::catalog::DbPhoto,
    ) -> std::cmp::Ordering {
        use std::cmp::Ordering;
        let ord = match self.sort.field {
            SortField::Captured => a.captured_at.cmp(&b.captured_at),
            SortField::Filename => a.filename.cmp(&b.filename),
            SortField::Rating => a.rating.cmp(&b.rating),
            SortField::Album => a.captured_at.cmp(&b.captured_at),
        };
        let ord = if ord == Ordering::Equal && self.sort.field != SortField::Filename {
            a.filename.cmp(&b.filename)
        } else {
            ord
        };
        match self.sort.dir {
            SortDir::Asc => ord,
            SortDir::Desc => ord.reverse(),
        }
    }
}

/// V16: thumbnail cell style (remembered per catalog).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CellStyle {
    Compact,
    #[default]
    Expanded,
}

impl CellStyle {
    pub fn parse(s: &str) -> Self {
        match s {
            "compact" => CellStyle::Compact,
            _ => CellStyle::Expanded,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            CellStyle::Compact => "compact",
            CellStyle::Expanded => "expanded",
        }
    }
}

/// V16: grid overlay cycle (`J`): nothing, filename + capture time, or
/// camera + exposure. Remembered per catalog.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum OverlayMode {
    None,
    #[default]
    File,
    Exif,
}

impl OverlayMode {
    pub fn parse(s: &str) -> Self {
        match s {
            "none" => OverlayMode::None,
            "exif" => OverlayMode::Exif,
            _ => OverlayMode::File,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            OverlayMode::None => "none",
            OverlayMode::File => "file",
            OverlayMode::Exif => "exif",
        }
    }

    /// `J` order: none → file → exif → none.
    pub fn cycle(self) -> Self {
        match self {
            OverlayMode::None => OverlayMode::File,
            OverlayMode::File => OverlayMode::Exif,
            OverlayMode::Exif => OverlayMode::None,
        }
    }
}

/// V16: per-cell badges, each toggled on its own. `label` is dormant
/// until color labels land (V13) — the toggle already persists.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct BadgeSet {
    pub flag: bool,
    pub rating: bool,
    pub label: bool,
    pub crop: bool,
    pub edit: bool,
    pub keywords: bool,
    pub pair: bool,
    pub video: bool,
    pub sync: bool,
}

impl BadgeSet {
    pub fn all_on() -> Self {
        Self {
            flag: true,
            rating: true,
            label: true,
            crop: true,
            edit: true,
            keywords: true,
            pair: true,
            video: true,
            sync: true,
        }
    }

    /// Comma names, `all`/`none` shortcuts; unknown tokens ignored.
    pub fn parse(s: &str) -> Self {
        if s.trim().is_empty() {
            return Self::all_on();
        }
        if s.trim() == "none" {
            return Self {
                flag: false,
                rating: false,
                label: false,
                crop: false,
                edit: false,
                keywords: false,
                pair: false,
                video: false,
                sync: false,
            };
        }
        if s.trim() == "all" {
            return Self::all_on();
        }
        let mut out = Self::all_on();
        // Start from all-off only when the string names badges: a bare
        // unknown string keeps everything (never blank the grid).
        let known = [
            "flag", "rating", "label", "crop", "edit", "keywords", "pair", "video", "sync",
        ];
        let parts: Vec<&str> = s.split(',').map(|t| t.trim()).collect();
        if !parts.iter().any(|t| known.contains(t)) {
            return out;
        }
        out = Self {
            flag: false,
            rating: false,
            label: false,
            crop: false,
            edit: false,
            keywords: false,
            pair: false,
            video: false,
            sync: false,
        };
        for t in parts {
            match t {
                "flag" => out.flag = true,
                "rating" => out.rating = true,
                "label" => out.label = true,
                "crop" => out.crop = true,
                "edit" => out.edit = true,
                "keywords" => out.keywords = true,
                "pair" => out.pair = true,
                "video" => out.video = true,
                "sync" => out.sync = true,
                _ => {}
            }
        }
        out
    }

    /// Canonical serialization (stable order).
    pub fn serialize(&self) -> String {
        let mut out = Vec::new();
        if self.flag {
            out.push("flag");
        }
        if self.rating {
            out.push("rating");
        }
        if self.label {
            out.push("label");
        }
        if self.crop {
            out.push("crop");
        }
        if self.edit {
            out.push("edit");
        }
        if self.keywords {
            out.push("keywords");
        }
        if self.pair {
            out.push("pair");
        }
        if self.video {
            out.push("video");
        }
        if self.sync {
            out.push("sync");
        }
        out.join(",")
    }
}

/// V16: overlay text lines for one photo (pure; the cell only lays out).
/// File mode: filename + capture time. Exif mode: camera (+ lens) +
/// exposure triplet assembled from non-empty parts only.
pub fn overlay_lines(
    mode: OverlayMode,
    filename: &str,
    captured_at: &str,
    camera: &str,
    lens: &str,
    aperture: &str,
    shutter: &str,
    iso: &str,
) -> Vec<String> {
    match mode {
        OverlayMode::None => Vec::new(),
        // Capture time only: filenames are noise on photo cells (they stay
        // in the hover tip and the metadata panel). The persisted mode
        // name is kept so saved preferences still load.
        OverlayMode::File => {
            let _ = filename;
            if captured_at.trim().is_empty() {
                Vec::new()
            } else {
                vec![captured_at.trim().to_string()]
            }
        }
        OverlayMode::Exif => {
            let mut out = Vec::new();
            // EXIF ASCII values are stored quoted ("NIKON D600").
            let unquote = |s: &str| s.trim().trim_matches('"').trim().to_string();
            let mut cam = unquote(camera);
            if !unquote(lens).is_empty() {
                if !cam.is_empty() {
                    cam.push_str(" · ");
                }
                cam.push_str(&unquote(lens));
            }
            if !cam.is_empty() {
                out.push(cam);
            }
            let parts: Vec<&str> = [aperture.trim(), shutter.trim(), iso.trim()]
                .into_iter()
                .filter(|t| !t.is_empty())
                .collect();
            if !parts.is_empty() {
                out.push(parts.join(" · "));
            }
            out
        }
    }
}

/// Lowercased haystack for substring search (U12).
pub fn search_haystack(
    filename: &str,
    camera: &str,
    lens: &str,
    captured_at: &str,
    keywords: &[String],
) -> String {
    let mut s = format!("{filename} {camera} {lens} {captured_at}");
    for k in keywords {
        s.push(' ');
        s.push_str(k);
    }
    s.to_lowercase()
}

/// `YYYY:MM:DD …` or `YYYY-MM-DD…` → `YYYY-MM-DD`; else None.
pub fn normalize_day(captured_at: &str) -> Option<String> {
    // `get` (not slicing): a multi-byte char inside the first 10 bytes
    // reads as undated instead of panicking.
    let norm = captured_at.get(..10)?.replace(':', "-");
    let parts: Vec<&str> = norm.split('-').collect();
    // Fixed-width parts only: the result is compared lexicographically
    // against `YYYY-MM-DD` window bounds.
    if parts.len() == 3
        && parts[0].len() == 4
        && parts[1].len() == 2
        && parts[2].len() == 2
        && parts.iter().all(|p| p.bytes().all(|b| b.is_ascii_digit()))
    {
        Some(norm)
    } else {
        None
    }
}

#[derive(Clone, Debug)]
pub struct PublishForm {
    pub title: String,
    pub slug: String,
    pub template: usize,
    pub sizes: Vec<u32>,
    pub watermark: bool,
    pub allow_downloads: bool,
    pub strip_gps: bool,
    pub password_protect: bool,
}

impl Default for PublishForm {
    /// No sample identity: a fresh form carries no gallery title or slug.
    /// (U01 — nothing here may imply user content that does not exist.)
    fn default() -> Self {
        Self {
            title: String::new(),
            slug: String::new(),
            template: 0,
            sizes: vec![640, 1280, 2048],
            watermark: true,
            allow_downloads: true,
            strip_gps: true,
            password_protect: false,
        }
    }
}

impl PublishForm {
    /// Lowercase, spaces/punctuation to `-`, collapse runs, trim dashes.
    pub fn derive_slug(title: &str) -> String {
        let mut out = String::with_capacity(title.len());
        let mut dash = false;
        for c in title.chars() {
            if c.is_ascii_alphanumeric() {
                out.push(c.to_ascii_lowercase());
                dash = false;
            } else if !dash {
                out.push('-');
                dash = true;
            }
        }
        out.trim_matches('-').to_string()
    }

    pub fn command_preview(&self) -> String {
        format!(
            "→ laika build --gallery {} --sizes {}\n→ laika deploy --target cloudflare-pages --project studio-galleries",
            if self.slug.is_empty() {
                "gallery"
            } else {
                &self.slug
            },
            self.sizes
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
                .join(","),
        )
    }
}

#[derive(Debug, Default)]
pub struct AppState {
    pub active_module: Module,
    pub photos: Vec<Photo>,
    pub selection: BTreeSet<i64>,
    pub primary: Option<i64>,
    /// U03: range anchor is independent of the primary photo, so repeated
    /// shift-clicks extend from a fixed point instead of drifting.
    pub anchor: Option<i64>,
    pub filters: Filters,
    pub thumb_columns: u8,
    pub edits: HashMap<i64, Edit>,
    pub clipboard: Option<[f32; crate::edit::PARAM_COUNT]>,
    pub publish: PublishForm,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            thumb_columns: 6,
            ..Self::default()
        }
    }

    pub fn filtered(&self, is_raw: impl Fn(i64) -> bool) -> Vec<&Photo> {
        self.photos
            .iter()
            .filter(|p| self.filters.matches(p, is_raw(p.id)))
            .collect()
    }

    pub fn select_click(&mut self, id: i64, shift: bool, cmd: bool, ordered_ids: &[i64]) {
        if shift {
            // U03: extend from the fixed anchor (falling back to the primary
            // when there is none); the anchor does not move, so repeated
            // shift-clicks grow one stable range.
            let anchor = self.anchor.or(self.primary);
            if let Some(a) = anchor {
                let ai = ordered_ids.iter().position(|&x| x == a);
                let bi = ordered_ids.iter().position(|&x| x == id);
                if let (Some(a), Some(b)) = (ai, bi) {
                    let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
                    self.selection.extend(ordered_ids[lo..=hi].iter().copied());
                    self.primary = Some(id);
                    return;
                }
            }
            self.selection.insert(id);
            self.primary = Some(id);
            self.anchor = Some(id);
        } else if cmd {
            if !self.selection.insert(id) {
                self.selection.remove(&id);
            }
            self.primary = Some(id);
            self.anchor = Some(id);
        } else {
            self.selection.clear();
            self.selection.insert(id);
            self.primary = Some(id);
            self.anchor = Some(id);
        }
    }

    pub fn selected_count(&self) -> usize {
        self.selection.len()
    }

    pub fn edit(&mut self, id: i64) -> &mut Edit {
        self.edits.entry(id).or_default()
    }

    pub fn set_columns(&mut self, cols: u8) {
        // V16: 3..20 columns (badges collapse by rule past 10).
        self.thumb_columns = cols.clamp(3, 20);
    }

    /// V16: full badges only at readable widths.
    pub fn full_badges(&self) -> bool {
        self.thumb_columns <= 10
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn photo(id: i64, rating: u8, picked: bool, sync: SyncState) -> Photo {
        Photo {
            id,
            filename: format!("DSC_{id}.dng"),
            rating,
            picked,
            rejected: false,
            label: 0,
            sync,
            tint: (0, 0),
        }
    }

    #[test]
    fn filters_and_combined() {
        let p = photo(1, 4, true, SyncState::Pending);
        let f = Filters {
            min_stars: 3,
            flag: FlagFilter::Picked,
            file_type: FileType::All,
            unsynced_only: true,
            ..Default::default()
        };
        assert!(f.matches(&p, true));
        assert!(!f.matches(&photo(2, 2, true, SyncState::Pending), true));
        assert!(!f.matches(&photo(3, 4, false, SyncState::Pending), true));
        assert!(!f.matches(&photo(4, 4, true, SyncState::Synced), true));
    }

    #[test]
    fn flag_and_file_type_are_exclusive() {
        // U12: enum dimensions cannot contradict themselves.
        let mut p = photo(1, 4, true, SyncState::Local);
        p.rejected = true;
        let f = Filters {
            flag: FlagFilter::Rejected,
            ..Default::default()
        };
        assert!(f.matches(&p, true));
        assert!(!f.matches(&photo(2, 4, true, SyncState::Local), true));
        let f = Filters {
            file_type: FileType::Raster,
            ..Default::default()
        };
        assert!(f.matches(&p, false));
        assert!(!f.matches(&p, true));
        let f = Filters {
            file_type: FileType::Raw,
            ..Default::default()
        };
        assert!(f.matches(&p, true));
        assert!(!f.matches(&p, false));
    }

    #[test]
    fn u06_unrated_and_unflagged_filters() {
        // U06: culling filters isolate the undecided photos.
        let mut undecided = photo(1, 0, false, SyncState::Local);
        undecided.rejected = false;
        let mut rated = photo(2, 4, false, SyncState::Local);
        rated.rejected = false;
        let mut picked = photo(3, 0, true, SyncState::Local);
        let mut rejected = photo(4, 0, false, SyncState::Local);
        rejected.rejected = true;

        let f = Filters {
            unrated_only: true,
            ..Default::default()
        };
        assert!(f.is_active());
        assert!(f.matches(&undecided, true));
        assert!(!f.matches(&rated, true));
        assert!(f.active_labels().contains(&"unrated".to_string()));

        let f = Filters {
            flag: FlagFilter::Unflagged,
            ..Default::default()
        };
        assert!(f.is_active());
        assert!(f.matches(&undecided, true));
        assert!(f.matches(&rated, true));
        assert!(!f.matches(&picked, true));
        assert!(!f.matches(&rejected, true));
        assert!(f.active_labels().contains(&"unflagged".to_string()));

        // Old preset JSON without the new field still reads (defaults off).
        let old: Filters = serde_json::from_str(
            r#"{"min_stars":3,"flag":"Picked","file_type":"All","unsynced_only":false,
                "search":"","missing_only":false,"camera":null,"lens":null,
                "date_from":null,"date_to":null,"folder":null,
                "sort":{"field":"Captured","dir":"Asc"}}"#,
        )
        .expect("old presets stay readable");
        assert!(!old.unrated_only);
        assert_eq!(old.flag, FlagFilter::Picked);
    }

    #[test]
    fn v16_overlay_lines_and_badge_persistence() {
        // File (date) mode: capture time only, never the filename.
        assert_eq!(
            super::overlay_lines(
                super::OverlayMode::File,
                "a.nef",
                "2026:01:02 10:00:00",
                "",
                "",
                "",
                "",
                ""
            ),
            vec!["2026:01:02 10:00:00".to_string()]
        );
        assert!(
            super::overlay_lines(super::OverlayMode::File, "a.nef", "", "", "", "", "", "")
                .is_empty()
        );
        assert!(
            super::overlay_lines(
                super::OverlayMode::None,
                "a.nef",
                "t",
                "c",
                "l",
                "f",
                "s",
                "i"
            )
            .is_empty()
        );
        // Exif mode: camera (+ lens) then the exposure triplet, with
        // empties dropped instead of dangling separators.
        assert_eq!(
            super::overlay_lines(
                super::OverlayMode::Exif,
                "a.nef",
                "",
                "Nikon",
                "24-70",
                "f/2.8",
                "",
                "400"
            ),
            vec!["Nikon · 24-70".to_string(), "f/2.8 · 400".to_string()]
        );
        assert!(
            super::overlay_lines(super::OverlayMode::Exif, "a", "", "", "", "", "", "").is_empty()
        );
        // Cycle order + persistence round-trip.
        assert_eq!(super::OverlayMode::None.cycle(), super::OverlayMode::File);
        assert_eq!(super::OverlayMode::File.cycle(), super::OverlayMode::Exif);
        assert_eq!(super::OverlayMode::Exif.cycle(), super::OverlayMode::None);
        assert_eq!(
            super::CellStyle::parse("compact"),
            super::CellStyle::Compact
        );
        assert_eq!(super::CellStyle::parse("bogus"), super::CellStyle::Expanded);
        let full = super::BadgeSet::all_on();
        assert_eq!(super::BadgeSet::parse(&full.serialize()), full);
        assert_eq!(super::BadgeSet::parse("").serialize(), full.serialize());
        let mut partial = full;
        partial.crop = false;
        partial.video = false;
        assert_eq!(super::BadgeSet::parse(&partial.serialize()), partial);
        assert!(!super::BadgeSet::parse("none").flag);
        assert!(super::AppState::new().full_badges());
        let mut st = super::AppState::new();
        st.set_columns(99);
        assert_eq!(st.thumb_columns, 20);
        assert!(!st.full_badges());
    }

    #[test]
    fn search_haystack_matches_case_insensitively() {
        let hay = search_haystack(
            "DSC_4412.NEF",
            "Nikon D600",
            "24-70mm",
            "2026:06:14",
            &["Lisbon".into()],
        );
        for q in [
            "dsc_4412",
            "nikon",
            "24-70",
            "2026:06",
            "lisbon",
            "nef nikon",
        ] {
            assert!(hay.contains(q), "{q} not in {hay}");
        }
        assert!(!hay.contains("canon"));
        assert_eq!(
            normalize_day("2026:06:14 18:42:00"),
            Some("2026-06-14".to_string())
        );
        assert_eq!(normalize_day("garbage"), None);
        assert_eq!(normalize_day("2026:06:1é 18:42:00"), None);
        assert_eq!(normalize_day("2026-6-014 18:42"), None);
    }

    #[test]
    fn matches_db_covers_camera_lens_dates_search() {
        use crate::catalog::DbPhoto;
        let mut p = DbPhoto {
            id: 1,
            catalog_id: 1,
            path: "/arc/a.nef".into(),
            filename: "a.nef".into(),
            blake3: String::new(),
            captured_at: "2026:06:14 10:00:00".into(),
            camera: "Nikon D600".into(),
            lens: "24-70mm".into(),
            focal_mm: String::new(),
            aperture: String::new(),
            shutter: String::new(),
            iso: String::new(),
            width: 0,
            height: 0,
            rating: 4,
            picked: false,
            rejected: false,
            sync: SyncState::Local,
            remote_key: String::new(),
            creator: String::new(),
            copyright: String::new(),
            rights: String::new(),
            contact: String::new(),
            captured_orig: String::new(),
            capture_offset_min: 0,
            duration_ms: 0,
            codec: String::new(),
            ..Default::default()
        };
        let kw = vec!["Lisbon".to_string()];
        let mut f = Filters::default();
        assert!(f.matches_db(&p, &kw));
        f.camera = Some("Canon".into());
        assert!(!f.matches_db(&p, &kw));
        f.camera = Some("Nikon D600".into());
        assert!(f.matches_db(&p, &kw));
        f.date_from = Some("2026-06-15".into());
        assert!(!f.matches_db(&p, &kw));
        f.date_from = None;
        f.date_to = Some("2026-06-13".into());
        assert!(!f.matches_db(&p, &kw));
        f.date_to = None;
        f.search = "lisbon".into();
        assert!(f.matches_db(&p, &kw));
        f.search = "canon".into();
        assert!(!f.matches_db(&p, &kw));
        f.search.clear();
        // Undated photos drop out of date windows, honestly.
        p.captured_at.clear();
        f.date_from = Some("2020-01-01".into());
        assert!(!f.matches_db(&p, &kw));
    }

    #[test]
    fn sort_comparator_orders_and_ties() {
        use crate::catalog::DbPhoto;
        let mk = |id: i64, name: &str, cap: &str, rating: u8| DbPhoto {
            id,
            catalog_id: 1,
            path: format!("/arc/{name}"),
            filename: name.into(),
            blake3: String::new(),
            captured_at: cap.into(),
            camera: String::new(),
            lens: String::new(),
            focal_mm: String::new(),
            aperture: String::new(),
            shutter: String::new(),
            iso: String::new(),
            width: 0,
            height: 0,
            rating,
            picked: false,
            rejected: false,
            sync: SyncState::Local,
            remote_key: String::new(),
            creator: String::new(),
            copyright: String::new(),
            rights: String::new(),
            contact: String::new(),
            captured_orig: String::new(),
            capture_offset_min: 0,
            duration_ms: 0,
            codec: String::new(),
            ..Default::default()
        };
        let a = mk(1, "b.nef", "2026:06:14 10:00:00", 5);
        let b = mk(2, "a.nef", "2026:06:14 10:00:00", 1);
        let f = Filters::default(); // captured asc, filename tiebreak
        use std::cmp::Ordering::*;
        assert_eq!(f.compare_db(&a, &b), Greater); // same time → b by name
        let mut f = Filters::default();
        f.sort.field = SortField::Rating;
        f.sort.dir = SortDir::Desc;
        assert_eq!(f.compare_db(&a, &b), Less); // 5 before 1
        f.sort.field = SortField::Filename;
        f.sort.dir = SortDir::Asc;
        assert_eq!(f.compare_db(&a, &b), Greater);
        // Serde round-trip for saved presets.
        let json = serde_json::to_string(&f).unwrap();
        let back: Filters = serde_json::from_str(&json).unwrap();
        assert_eq!(back.sort.field, SortField::Filename);
    }

    #[test]
    fn unsynced_matches_pending_and_failed() {
        let f = Filters {
            unsynced_only: true,
            ..Default::default()
        };
        assert!(f.matches(&photo(1, 0, false, SyncState::Pending), true));
        assert!(f.matches(&photo(2, 0, false, SyncState::Failed), true));
        assert!(!f.matches(&photo(3, 0, false, SyncState::Synced), true));
        assert!(!f.matches(&photo(4, 0, false, SyncState::Local), true));
    }

    #[test]
    fn selection_click_shift_cmd() {
        let mut s = AppState::new();
        let ids = vec![1, 2, 3, 4];
        s.select_click(2, false, false, &ids);
        assert!(s.selection.contains(&2) && s.selection.len() == 1);
        s.select_click(4, true, false, &ids);
        assert_eq!(s.selection, BTreeSet::from([2, 3, 4]));
        s.select_click(3, true, false, &ids);
        assert!(s.selection.contains(&3));
        let mut s2 = AppState::new();
        s2.select_click(1, false, false, &ids);
        s2.select_click(3, false, true, &ids);
        assert_eq!(s2.selection, BTreeSet::from([1, 3]));
        s2.select_click(1, false, true, &ids);
        assert_eq!(s2.selection, BTreeSet::from([3]));
    }

    #[test]
    fn range_anchor_stays_fixed_across_shifts() {
        // U03: repeated shift-clicks extend from one fixed anchor.
        let mut s = AppState::new();
        let ids = vec![1, 2, 3, 4, 5];
        s.select_click(2, false, false, &ids);
        assert_eq!(s.anchor, Some(2));
        s.select_click(4, true, false, &ids);
        assert_eq!(s.selection, BTreeSet::from([2, 3, 4]));
        assert_eq!(s.anchor, Some(2));
        assert_eq!(s.primary, Some(4));
        s.select_click(5, true, false, &ids);
        assert_eq!(s.selection, BTreeSet::from([2, 3, 4, 5]));
        assert_eq!(s.anchor, Some(2));
        // A plain click resets the anchor.
        s.select_click(5, false, false, &ids);
        assert_eq!(s.anchor, Some(5));
        assert_eq!(s.selection, BTreeSet::from([5]));
    }

    #[test]
    fn shift_without_anchor_falls_back_to_primary() {
        let mut s = AppState::new();
        let ids = vec![1, 2, 3, 4];
        s.primary = Some(1);
        s.select_click(3, true, false, &ids);
        assert_eq!(s.selection, BTreeSet::from([1, 2, 3]));
    }

    #[test]
    fn slug_derivation() {
        assert_eq!(
            PublishForm::derive_slug("Mori & Tan — 14 June 2026"),
            "mori-tan-14-june-2026"
        );
        assert_eq!(PublishForm::derive_slug("  Portfolio  "), "portfolio");
        assert_eq!(PublishForm::derive_slug("a---b"), "a-b");
    }

    #[test]
    fn publish_defaults_carry_no_sample_identity() {
        // U01: a fresh form must not imply gallery content that doesn't exist.
        let f = PublishForm::default();
        assert!(f.title.is_empty());
        assert!(f.slug.is_empty());
    }
}
