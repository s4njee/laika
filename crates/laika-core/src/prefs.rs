//! V31: app-wide preferences, stored in `library.json` in the app support
//! directory (never in a catalog). Per-catalog settings stay in each
//! catalog; the Preferences window labels those.

/// Sidecar policy: write XMP next to originals, or keep edits in the
/// catalog only.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum SidecarPolicy {
    #[default]
    Always,
    Never,
}

/// Which GPU wgpu should prefer (only matters with more than one).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum GpuPreference {
    #[default]
    HighPerformance,
    LowPower,
}

/// Filmstrip size (Develop and Loupe).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum FilmstripSize {
    Small,
    #[default]
    Medium,
    Large,
}

impl FilmstripSize {
    /// Scale applied to the filmstrip height and cell size.
    pub fn scale(self) -> f32 {
        match self {
            FilmstripSize::Small => 0.75,
            FilmstripSize::Medium => 1.0,
            FilmstripSize::Large => 1.4,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            FilmstripSize::Small => "Small",
            FilmstripSize::Medium => "Medium",
            FilmstripSize::Large => "Large",
        }
    }
}

/// External editor file format (the renderer writes 8-bit sRGB).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum EditorFormat {
    #[default]
    Tiff,
    Jpeg,
}

#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct AppPrefs {
    // File handling — import dialog defaults.
    pub import_copy: bool,
    pub import_skip_duplicates: bool,
    pub import_new_only: bool,
    pub import_eject: bool,
    /// RAW+JPEG grouping for catalogs that haven't chosen yet.
    pub group_pairs_default: bool,
    pub sidecars: SidecarPolicy,
    // Interface — defaults for catalogs without their own choice.
    pub default_columns: u8,
    pub default_cell_style: String,
    pub default_overlay: String,
    pub default_badges: String,
    /// Entering the wall also hides the rails and toolbars.
    pub wall_hides_chrome: bool,
    pub filmstrip: FilmstripSize,
    // External editing.
    pub editor_app: String,
    pub editor_format: EditorFormat,
    pub editor_naming: String,
    // Performance.
    pub gpu: GpuPreference,
    /// Import worker threads (0 = automatic).
    pub import_workers: u8,
    /// Thumbnails decoded in parallel per batch.
    pub thumb_concurrency: u8,
    /// V32: write a crash report beside the log (opt-in; never sent).
    pub crash_reports: bool,
}

impl Default for AppPrefs {
    fn default() -> Self {
        Self {
            import_copy: false,
            import_skip_duplicates: true,
            import_new_only: true,
            import_eject: false,
            group_pairs_default: true,
            sidecars: SidecarPolicy::Always,
            default_columns: 6,
            default_cell_style: "expanded".to_string(),
            default_overlay: "file".to_string(),
            default_badges: String::new(),
            wall_hides_chrome: false,
            filmstrip: FilmstripSize::Medium,
            editor_app: String::new(),
            editor_format: EditorFormat::Tiff,
            editor_naming: "{original}-edit".to_string(),
            gpu: GpuPreference::HighPerformance,
            import_workers: 0,
            thumb_concurrency: 8,
            crash_reports: false,
        }
    }
}

impl AppPrefs {
    /// Clamp hand-edited or older values into range.
    pub fn sanitized(mut self) -> Self {
        self.default_columns = self.default_columns.clamp(3, 20);
        self.import_workers = self.import_workers.min(32);
        self.thumb_concurrency = self.thumb_concurrency.clamp(1, 32);
        if self.editor_naming.trim().is_empty() {
            self.editor_naming = Self::default().editor_naming;
        }
        self
    }

    /// Import workers to use: an explicit preference, else automatic
    /// (cores minus two, 2..=8).
    pub fn import_worker_count(&self, cores: usize) -> usize {
        if self.import_workers > 0 {
            self.import_workers as usize
        } else {
            cores.saturating_sub(2).clamp(2, 8)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn older_files_load_with_defaults_and_values_clamp() {
        let p: AppPrefs = serde_json::from_str("{}").unwrap();
        assert_eq!(p, AppPrefs::default());
        let p: AppPrefs = serde_json::from_str(
            r#"{"default_columns": 99, "thumb_concurrency": 0, "editor_naming": " "}"#,
        )
        .unwrap();
        let p = p.sanitized();
        assert_eq!((p.default_columns, p.thumb_concurrency), (20, 1));
        assert_eq!(p.editor_naming, "{original}-edit");
        let back: AppPrefs = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn worker_count_prefers_the_setting() {
        let mut p = AppPrefs::default();
        assert_eq!(p.import_worker_count(10), 8);
        assert_eq!(p.import_worker_count(3), 2);
        p.import_workers = 3;
        assert_eq!(p.import_worker_count(10), 3);
    }
}
