//! S04: "Coming from Lightroom" — a Lightroom Classic keyboard map, a
//! command palette that understands Lightroom's menu names (answering
//! honestly when Laika has no equivalent yet), and a one-page guide that
//! maps Lightroom concepts to Laika's.

use super::*;
use commands::Command as C;

/// Where a Lightroom command name leads in Laika.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Target {
    Cmd(C),
    /// Not in Laika yet — why, and where it's planned.
    Missing(&'static str),
}

use Target::{Cmd, Missing};

/// Lightroom Classic menu command names (and a few panel/tool names) →
/// Laika. Every entry either runs a Laika command or says plainly that
/// Laika doesn't have it yet.
pub(crate) const LIGHTROOM_TERMS: &[(&str, Target)] = &[
    // File
    ("New Catalog", Cmd(C::NewCatalog)),
    ("Open Catalog", Cmd(C::OpenCatalog)),
    ("Open Recent", Cmd(C::ManageCatalog)),
    ("Optimize Catalog", Cmd(C::ManageCatalog)),
    ("Import Photos and Video", Cmd(C::ImportPhotos)),
    ("Import from Another Catalog", Cmd(C::ImportLightroom)),
    (
        "Tethered Capture",
        Missing("Tethered capture isn't in Laika (considered in U27)."),
    ),
    (
        "Auto Import",
        Missing(
            "Auto import from a watched folder is planned with rules (S19); folder Watch exists in the left rail.",
        ),
    ),
    ("Export", Cmd(C::Export)),
    ("Export with Previous", Cmd(C::ExportPrevious)),
    ("Export with Preset", Cmd(C::Export)),
    ("Export as Catalog", Cmd(C::ExportEverything)),
    (
        "Plug-in Manager",
        Missing("Laika has no plug-ins yet; sandboxed extensions are planned (S20)."),
    ),
    (
        "Plug-in Extras",
        Missing("Laika has no plug-ins yet (S20)."),
    ),
    ("Show Quick Collection", Cmd(C::AddToTarget)),
    ("Save Quick Collection", Cmd(C::AddToTarget)),
    ("Clear Quick Collection", Cmd(C::AddToTarget)),
    ("Set Quick Collection as Target", Cmd(C::AddToTarget)),
    ("Page Setup", Missing("Printing isn't in Laika yet (S30).")),
    ("Print", Missing("Printing isn't in Laika yet (S30).")),
    // Edit
    ("Undo", Cmd(C::Undo)),
    ("Redo", Cmd(C::Redo)),
    ("Select All", Cmd(C::SelectAll)),
    ("Select None", Cmd(C::Deselect)),
    ("Deselect Active Photo", Cmd(C::Deselect)),
    (
        "Select Flagged Photos",
        Missing("Use the Picked filter above the grid, then Select All."),
    ),
    ("Catalog Settings", Cmd(C::ManageCatalog)),
    ("Preferences", Cmd(C::Preferences)),
    (
        "Identity Plate Setup",
        Missing("Laika has no identity plate."),
    ),
    ("Edit Watermarks", Cmd(C::Export)),
    // Library
    ("New Collection", Cmd(C::NewCollection)),
    ("New Smart Collection", Cmd(C::NewSmartCollection)),
    (
        "New Collection Set",
        Missing("Collection sets aren't in Laika yet; imported sets become name prefixes (U13)."),
    ),
    ("New Folder", Cmd(C::MoveToFolder)),
    ("Find", Cmd(C::Find)),
    ("Enable Filters", Cmd(C::Find)),
    ("Library Filter", Cmd(C::Find)),
    ("Rename Photo", Cmd(C::Rename)),
    (
        "Convert Photos to DNG",
        Missing("DNG conversion isn't in Laika yet (V06)."),
    ),
    ("Find Missing Photos", Cmd(C::RelinkMissing)),
    ("Synchronize Folder", Cmd(C::RelinkMissing)),
    ("Build Standard-Sized Previews", Cmd(C::BuildSmartPreviews)),
    ("Build 1:1 Previews", Cmd(C::BuildOneToOnePreviews)),
    ("Build Smart Previews", Cmd(C::BuildSmartPreviews)),
    ("Discard 1:1 Previews", Cmd(C::ManageCatalog)),
    ("Discard Smart Previews", Cmd(C::ManageCatalog)),
    // Photo
    ("Add to Quick Collection", Cmd(C::AddToTarget)),
    ("Open in Loupe", Cmd(C::ViewLoupe)),
    ("Open in Compare", Cmd(C::ViewCompare)),
    ("Open in Survey", Cmd(C::ViewSurvey)),
    (
        "Lock to Second Window",
        Missing("A second window isn't in Laika yet (V21)."),
    ),
    ("Show in Finder", Cmd(C::RevealInFinder)),
    ("Go to Folder in Library", Cmd(C::RevealInFinder)),
    ("Edit in Adobe Photoshop", Cmd(C::EditExternal)),
    ("Edit In", Cmd(C::EditExternal)),
    (
        "Open as Smart Object in Photoshop",
        Missing("Laika sends a TIFF to your external editor instead (Edit in External Editor)."),
    ),
    (
        "Photo Merge",
        Missing("HDR and panorama merges aren't in Laika (considered in U27)."),
    ),
    (
        "HDR",
        Missing("HDR merge isn't in Laika (considered in U27)."),
    ),
    (
        "Panorama",
        Missing("Panorama merge isn't in Laika (considered in U27)."),
    ),
    (
        "Enhance",
        Missing("AI Enhance / Denoise isn't in Laika (considered in U27)."),
    ),
    (
        "Denoise",
        Missing("AI Denoise isn't in Laika; the Detail panel has noise reduction (U27)."),
    ),
    (
        "Super Resolution",
        Missing("Super Resolution isn't in Laika (U27)."),
    ),
    ("Group into Stack", Cmd(C::CreateStack)),
    ("Unstack", Cmd(C::Unstack)),
    (
        "People",
        Missing("Face recognition isn't in Laika (considered in U27, on-device only)."),
    ),
    (
        "Create Virtual Copy",
        Missing(
            "Virtual copies aren't in Laika yet (S08). Snapshots in Develop keep alternative treatments meanwhile.",
        ),
    ),
    (
        "Set Copy as Master",
        Missing("Virtual copies aren't in Laika yet (S08)."),
    ),
    ("Rotate Left", Cmd(C::RotateLeft)),
    ("Rotate Right", Cmd(C::RotateRight)),
    ("Rotate Left (CCW)", Cmd(C::RotateLeft)),
    ("Rotate Right (CW)", Cmd(C::RotateRight)),
    ("Flip Horizontal", Cmd(C::FlipHorizontal)),
    ("Flip Vertical", Cmd(C::FlipVertical)),
    ("Set Flag", Cmd(C::Pick)),
    ("Flagged", Cmd(C::Pick)),
    ("Unflagged", Cmd(C::Unflag)),
    ("Rejected", Cmd(C::Reject)),
    ("Set Rating", Cmd(C::Rating(3))),
    ("Set Color Label", Cmd(C::Label(1))),
    ("Auto Advance", Cmd(C::AutoAdvance)),
    ("Set Keyword", Cmd(C::KeywordManager)),
    ("Add Keywords", Cmd(C::KeywordManager)),
    ("Keywording", Cmd(C::KeywordManager)),
    ("Keyword List", Cmd(C::KeywordManager)),
    (
        "Edit Capture Time",
        Missing(
            "Capture time correction isn't in Laika yet (V14); import can shift times by a fixed offset.",
        ),
    ),
    (
        "Revert Capture Time to Original",
        Missing("Capture time correction isn't in Laika yet (V14)."),
    ),
    (
        "Update DNG Preview & Metadata",
        Missing("Laika never writes into originals, DNG included."),
    ),
    ("Read Metadata from File", Cmd(C::RelinkMissing)),
    (
        "Save Metadata to File",
        Missing(
            "Laika writes .xmp sidecars automatically (Preferences → File Handling → Sidecars).",
        ),
    ),
    (
        "Remove Photo",
        Missing("Right-click a photo → Remove from Catalog."),
    ),
    (
        "Remove Photo from Catalog",
        Missing("Right-click a photo → Remove from Catalog."),
    ),
    ("Delete Rejected Photos", Cmd(C::DeleteRejected)),
    // Metadata
    (
        "Copy Metadata",
        Missing("Metadata presets in the right rail apply the same fields to a selection."),
    ),
    (
        "Paste Metadata",
        Missing("Metadata presets in the right rail apply the same fields to a selection."),
    ),
    (
        "Sync Metadata",
        Missing(
            "Select photos and edit the right rail's metadata — changes apply to every selected photo.",
        ),
    ),
    ("Edit Metadata Presets", Cmd(C::ImportPresets)),
    ("Import Keywords", Cmd(C::KeywordManager)),
    ("Export Keywords", Cmd(C::KeywordManager)),
    ("Purge Unused Keywords", Cmd(C::KeywordManager)),
    // View
    ("Go to Grid", Cmd(C::ViewGrid)),
    ("Grid", Cmd(C::ViewGrid)),
    ("Go to Loupe", Cmd(C::ViewLoupe)),
    ("Loupe", Cmd(C::ViewLoupe)),
    ("Go to Compare", Cmd(C::ViewCompare)),
    ("Go to Survey", Cmd(C::ViewSurvey)),
    (
        "Go to People",
        Missing("Face recognition isn't in Laika (U27)."),
    ),
    ("Zoom In", Cmd(C::ZoomCycle)),
    ("Zoom Out", Cmd(C::ZoomFit)),
    ("Toggle Zoom View", Cmd(C::ZoomCycle)),
    ("Grid View Style", Cmd(C::CellOverlay)),
    ("Cycle Grid View Style", Cmd(C::CellOverlay)),
    ("Loupe Info", Cmd(C::LoupeInfo)),
    ("Cycle Info Display", Cmd(C::LoupeInfo)),
    ("View Options", Cmd(C::Preferences)),
    (
        "Hide Toolbar",
        Missing("Laika's toolbar can't be hidden separately; Shift-Tab hides all panels."),
    ),
    ("Lights Out", Cmd(C::Lights)),
    ("Screen Mode", Cmd(C::FullScreen)),
    ("Full Screen Preview", Cmd(C::FullScreen)),
    ("Hide All Panels", Cmd(C::HideChrome)),
    ("Toggle Side Panels", Cmd(C::HideChrome)),
    // Window / modules
    ("Library", Cmd(C::Library)),
    ("Develop", Cmd(C::Develop)),
    ("Map", Cmd(C::Map)),
    ("Book", Missing("Books aren't in Laika.")),
    ("Slideshow", Cmd(C::Slideshow)),
    ("Impromptu Slideshow", Cmd(C::Slideshow)),
    ("Web", Cmd(C::Publish)),
    ("Publish Services", Cmd(C::Publish)),
    // Develop
    (
        "New Snapshot",
        Missing("Name a snapshot in Develop's left rail (Snapshots)."),
    ),
    (
        "New Preset",
        Missing("Saving your own presets is coming (U16); Import Presets brings Lightroom's in."),
    ),
    ("Import Develop Presets", Cmd(C::ImportPresets)),
    ("Reset", Cmd(C::ResetGeometry)),
    (
        "Auto Tone",
        Missing("Auto Tone isn't in Laika yet (V25); Auto white balance is."),
    ),
    ("Auto White Balance", Cmd(C::AutoWhiteBalance)),
    ("White Balance Selector", Cmd(C::WhiteBalancePicker)),
    ("White Balance Tool", Cmd(C::WhiteBalancePicker)),
    (
        "Convert to Black & White",
        Missing(
            "A Black & White mix isn't in Laika yet (V24); the Mono contrast preset or Saturation −100 comes close.",
        ),
    ),
    (
        "Treatment: Black & White",
        Missing("A Black & White mix isn't in Laika yet (V24)."),
    ),
    ("Copy Settings", Cmd(C::CopySettings)),
    ("Paste Settings", Cmd(C::PasteSettings)),
    ("Paste Settings from Previous", Cmd(C::PasteSettings)),
    ("Sync Settings", Cmd(C::PasteSettings)),
    (
        "Enable Auto Sync",
        Missing("Auto Sync isn't in Laika yet (U15); copy and paste settings instead."),
    ),
    (
        "Match Total Exposures",
        Missing("Match Total Exposures isn't in Laika yet."),
    ),
    ("Crop", Cmd(C::CropTool)),
    ("Crop Overlay", Cmd(C::CropTool)),
    ("Crop Tool", Cmd(C::CropTool)),
    ("Straighten", Cmd(C::CropTool)),
    ("Cycle Crop Guide Overlay", Cmd(C::CropOverlay)),
    ("Spot Removal", Cmd(C::Develop)),
    ("Healing", Cmd(C::Develop)),
    (
        "Content-Aware Remove",
        Missing("Healing isn't in Laika yet (U19)."),
    ),
    (
        "Red Eye Correction",
        Missing("Red-eye correction isn't in Laika yet (U19)."),
    ),
    ("Masking", Cmd(C::Develop)),
    ("Graduated Filter", Cmd(C::Develop)),
    ("Linear Gradient", Cmd(C::Develop)),
    ("Radial Filter", Cmd(C::Develop)),
    ("Radial Gradient", Cmd(C::Develop)),
    ("Adjustment Brush", Cmd(C::Develop)),
    (
        "Select Subject",
        Missing("AI masks aren't in Laika (considered in U27)."),
    ),
    (
        "Select Sky",
        Missing("AI masks aren't in Laika (considered in U27)."),
    ),
    ("Before/After", Cmd(C::BeforeAfter)),
    ("Before & After Left/Right", Cmd(C::BeforeAfter)),
    (
        "Show Clipping",
        Missing("Use the clipping Overlay button under the histogram in Develop."),
    ),
    ("Upright", Cmd(C::Upright(1))),
    ("Transform", Cmd(C::Upright(1))),
    (
        "Lens Corrections",
        Missing(
            "Automatic lens profiles aren't in Laika yet (S27); Optics has manual distortion and CA.",
        ),
    ),
    (
        "Enable Profile Corrections",
        Missing("Automatic lens profiles aren't in Laika yet (S27)."),
    ),
    ("Remove Chromatic Aberration", Cmd(C::Develop)),
    ("Profile Browser", Cmd(C::Develop)),
    (
        "Calibration",
        Missing("Calibration isn't in Laika yet (V24)."),
    ),
    (
        "Soft Proofing",
        Missing("Soft proofing isn't in Laika yet (U20)."),
    ),
    ("History", Cmd(C::Develop)),
    ("Snapshots", Cmd(C::Develop)),
    ("Tone Curve", Cmd(C::Develop)),
    ("HSL / Color", Cmd(C::Develop)),
    ("Color Grading", Cmd(C::Develop)),
    ("Split Toning", Cmd(C::Develop)),
    ("Detail", Cmd(C::Develop)),
    ("Effects", Cmd(C::Develop)),
    ("Map module", Cmd(C::Map)),
    (
        "Tracklog",
        Missing("GPX tracklog geotagging is planned (S15); place photos by hand in Map meanwhile."),
    ),
    ("Keyboard Shortcuts", Cmd(C::Shortcuts)),
];

/// One palette result.
#[derive(Clone, Debug)]
pub(crate) struct PaletteHit {
    pub title: String,
    /// "Lightroom: Spot Removal" when found through a Lightroom name.
    pub via: Option<&'static str>,
    pub key: &'static str,
    pub target: Target,
}

#[derive(Default)]
pub(crate) struct PaletteUi {
    pub open: bool,
    pub selected: usize,
}

#[derive(Default)]
pub(crate) struct GuideUi {
    pub open: bool,
    /// Brief notice over the workspace (keys that explain themselves).
    pub toast: Option<(String, Instant)>,
}

const TOAST_SECS: u64 = 4;

fn score(hay: &str, needle: &str) -> Option<i32> {
    let h = hay.to_lowercase();
    let n = needle.trim().to_lowercase();
    if n.is_empty() {
        return Some(0);
    }
    if h == n {
        return Some(1000);
    }
    if h.starts_with(&n) {
        return Some(800 - h.len() as i32);
    }
    if h.split(|c: char| !c.is_alphanumeric())
        .any(|w| w.starts_with(&n))
    {
        return Some(600 - h.len() as i32);
    }
    if h.contains(&n) {
        return Some(400 - h.len() as i32);
    }
    // Every word of the query starts a word of the title ("sp rem").
    let words: Vec<&str> = h
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();
    let all = n
        .split_whitespace()
        .all(|q| words.iter().any(|w| w.starts_with(q)));
    all.then(|| 300 - h.len() as i32)
}

/// Commands and Lightroom names matching a query, best first.
pub(crate) fn palette_hits(query: &str) -> Vec<PaletteHit> {
    let mut hits: Vec<(i32, PaletteHit)> = Vec::new();
    for c in commands::all_commands() {
        let (title, key) = c.title();
        if let Some(s) = score(&title, query) {
            hits.push((
                s + 5,
                PaletteHit {
                    title,
                    via: None,
                    key,
                    target: Cmd(c),
                },
            ));
        }
    }
    for (term, target) in LIGHTROOM_TERMS {
        let Some(s) = score(term, query) else {
            continue;
        };
        let (title, key) = match target {
            Cmd(c) => c.title(),
            Missing(_) => (term.to_string(), ""),
        };
        // A Lightroom name that lands on a command already listed under
        // its own name adds nothing.
        if let Cmd(c) = target {
            if hits.iter().any(|(_, h)| {
                matches!(h.target, Cmd(x) if x == *c)
                    && h.via.is_none()
                    && score(&h.title, query).is_some()
            }) {
                continue;
            }
        }
        hits.push((
            s,
            PaletteHit {
                title,
                via: Some(term),
                key,
                target: *target,
            },
        ));
    }
    hits.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.title.cmp(&b.1.title)));
    // One row per Laika command (the best-matching name wins); each
    // "not yet" answer keeps its own row.
    let mut seen = std::collections::HashSet::new();
    hits.into_iter()
        .map(|(_, h)| h)
        .filter(|h| match h.target {
            Cmd(_) => seen.insert(h.title.clone()),
            Missing(_) => seen.insert(format!("missing:{}", h.via.unwrap_or(&h.title))),
        })
        .take(if query.trim().is_empty() { 12 } else { 40 })
        .collect()
}

impl Laika {
    // ---- keyboard map -------------------------------------------------------------

    /// Lightroom Classic keys (when that map is on). True when handled.
    pub(crate) fn lightroom_key(
        &mut self,
        key: &str,
        cmd: bool,
        shift: bool,
        alt: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !commands::lightroom_keys() {
            return false;
        }
        let develop = self.state.active_module == Module::Develop;
        let missing = |this: &mut Self, what: &str, cx: &mut Context<Self>| {
            let why = LIGHTROOM_TERMS
                .iter()
                .find(|(t, _)| t.eq_ignore_ascii_case(what))
                .and_then(|(_, t)| match t {
                    Missing(w) => Some(*w),
                    _ => None,
                })
                .unwrap_or("That isn't in Laika yet.");
            this.toast(format!("{what}: {why}"), cx);
        };
        let run = |this: &mut Self, c: C, window: &mut Window, cx: &mut Context<Self>| {
            this.run_command(c, window, cx);
            cx.notify();
        };
        if cmd && alt {
            match key {
                "1" => run(self, C::Library, window, cx),
                "2" => run(self, C::Develop, window, cx),
                "3" => run(self, C::Map, window, cx),
                "4" => missing(self, "Book", cx),
                "5" => run(self, C::Slideshow, window, cx),
                "6" => missing(self, "Print", cx),
                "7" => run(self, C::Publish, window, cx),
                _ => return false,
            }
            return true;
        }
        if cmd && shift {
            match key {
                "c" => run(self, C::CopySettings, window, cx),
                "v" => run(self, C::PasteSettings, window, cx),
                "e" => run(self, C::Export, window, cx),
                "i" => run(self, C::ImportPhotos, window, cx),
                "u" => run(self, C::AutoWhiteBalance, window, cx),
                _ => return false,
            }
            return true;
        }
        if cmd {
            match key {
                "'" => missing(self, "Create Virtual Copy", cx),
                "r" => run(self, C::RevealInFinder, window, cx),
                "k" => run(self, C::KeywordManager, window, cx),
                "u" => missing(self, "Auto Tone", cx),
                _ => return false,
            }
            return true;
        }
        if self.crop_open {
            // The crop tool keeps its own keys (O, X, arrows, Enter, Esc);
            // R toggles it off like Lightroom.
            if key == "r" {
                run(self, C::CropTool, window, cx);
                return true;
            }
            return false;
        }
        match key {
            "w" => run(self, C::WhiteBalancePicker, window, cx),
            "r" => run(self, C::CropTool, window, cx),
            "q" => missing(self, "Spot Removal", cx),
            "k" if develop => missing(self, "Adjustment Brush", cx),
            "m" if develop && shift => missing(self, "Radial Filter", cx),
            "m" if develop => missing(self, "Graduated Filter", cx),
            "v" => missing(self, "Convert to Black & White", cx),
            "c" => run(self, C::ViewCompare, window, cx),
            "n" => run(self, C::ViewSurvey, window, cx),
            "space" => run(self, C::ZoomCycle, window, cx),
            "tab" if !shift => run(self, C::HideChrome, window, cx),
            _ => return false,
        }
        true
    }

    /// Show a brief notice over whatever module is open.
    pub(crate) fn toast(&mut self, text: String, cx: &mut Context<Self>) {
        self.status_note = text.clone();
        let until = Instant::now() + std::time::Duration::from_secs(TOAST_SECS);
        self.guide.toast = Some((text, until));
        cx.spawn(async move |entity, cx| {
            cx.background_executor()
                .timer(std::time::Duration::from_secs(TOAST_SECS))
                .await;
            entity
                .update(cx, |this, cx| {
                    if this
                        .guide
                        .toast
                        .as_ref()
                        .is_some_and(|(_, u)| Instant::now() >= *u)
                    {
                        this.guide.toast = None;
                        cx.notify();
                    }
                })
                .ok();
        })
        .detach();
        cx.notify();
    }

    pub(crate) fn toast_overlay(&self) -> Option<Div> {
        let (text, _) = self.guide.toast.as_ref()?;
        Some(
            div()
                .absolute()
                .left_0()
                .right_0()
                .bottom(px(150.))
                .flex()
                .justify_center()
                .child(
                    div()
                        .max_w(px(620.))
                        .px(px(16.))
                        .py(px(10.))
                        .rounded(px(6.))
                        .bg(rgba(0x0F0E0DF0))
                        .border_1()
                        .border_color(rgb(0xC8A15A))
                        .text_size(sp(12.))
                        .text_color(rgb(TEXT_PRIMARY))
                        .child(text.clone()),
                ),
        )
    }

    pub(crate) fn set_lightroom_keys(&mut self, on: bool, cx: &mut Context<Self>) {
        commands::set_lightroom_keys(on);
        self.set_prefs(move |p| p.lightroom_keys = on, cx);
        commands::refresh_native_menus(cx);
        self.status_note = if on {
            "Lightroom keyboard shortcuts on — W white balance, R crop, ⌥⌘1–3 modules, ⇧⌘C/V copy and paste settings".to_string()
        } else {
            "Laika keyboard shortcuts".to_string()
        };
        cx.notify();
    }

    // ---- command palette -------------------------------------------------------------

    pub(crate) fn open_palette(&mut self, cx: &mut Context<Self>) {
        self.close_modals(cx);
        self.palette = PaletteUi {
            open: true,
            selected: 0,
        };
        self.focus_field(text_input::FieldId::Palette, cx);
        cx.notify();
    }

    fn palette_query(&self) -> String {
        self.field
            .as_ref()
            .filter(|f| f.id == text_input::FieldId::Palette)
            .map(|f| f.buffer.clone())
            .unwrap_or_default()
    }

    /// Palette keys: arrows pick, Enter runs, Esc closes; the rest edit
    /// the query.
    pub(crate) fn palette_key(
        &mut self,
        key: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> bool {
        if !self.palette.open {
            return false;
        }
        let n = palette_hits(&self.palette_query()).len();
        match key {
            "up" => self.palette.selected = self.palette.selected.saturating_sub(1),
            "down" => self.palette.selected = (self.palette.selected + 1).min(n.saturating_sub(1)),
            "enter" => {
                let i = self.palette.selected;
                self.palette_run(i, window, cx);
            }
            "escape" => {
                self.palette.open = false;
                self.defocus_field();
            }
            _ => return false,
        }
        cx.notify();
        true
    }

    fn palette_run(&mut self, i: usize, window: &mut Window, cx: &mut Context<Self>) {
        let hits = palette_hits(&self.palette_query());
        let Some(hit) = hits.get(i).cloned() else {
            return;
        };
        match hit.target {
            Cmd(c) => {
                self.palette.open = false;
                self.defocus_field();
                self.run_command(c, window, cx);
            }
            Missing(why) => {
                self.palette.open = false;
                self.defocus_field();
                self.toast(format!("{}: {why}", hit.via.unwrap_or(&hit.title)), cx);
            }
        }
        cx.notify();
    }

    pub(crate) fn palette_modal(&self, cx: &mut Context<Self>) -> Div {
        let q = self.palette_query();
        let hits = palette_hits(&q);
        let sel = self.palette.selected.min(hits.len().saturating_sub(1));
        let mut list = div()
            .id("palette-list")
            .max_h(px(420.))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(1.));
        for (i, h) in hits.iter().enumerate() {
            let on = i == sel;
            let (sub, sub_color) = match (&h.target, h.via) {
                (Missing(why), _) => (why.to_string(), 0xC8A15A),
                (Cmd(_), Some(via)) if !via.eq_ignore_ascii_case(&h.title) => {
                    (format!("Lightroom: {via}"), TEXT_DIM)
                }
                _ => (String::new(), TEXT_DIM),
            };
            list = list.child(
                div()
                    .id(("palette-hit", i))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .px(px(10.))
                    .py(px(6.))
                    .rounded(px(4.))
                    .when(on, |d| d.bg(rgb(bg_row_active())))
                    .hover(|s| s.bg(rgb(bg_row_hover())))
                    .on_click(
                        cx.listener(move |this, _, window, cx| this.palette_run(i, window, cx)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(sp(12.))
                                    .text_color(rgb(if matches!(h.target, Missing(_)) {
                                        TEXT_TERTIARY
                                    } else {
                                        TEXT_PRIMARY
                                    }))
                                    .child(h.title.clone()),
                            )
                            .when(!sub.is_empty(), |d| {
                                d.child(
                                    div()
                                        .text_size(sp(10.5))
                                        .text_color(rgb(sub_color))
                                        .child(sub),
                                )
                            }),
                    )
                    .when(matches!(h.target, Missing(_)), |d| {
                        d.child(
                            div()
                                .text_size(sp(10.))
                                .text_color(rgb(0xC8A15A))
                                .child("not yet"),
                        )
                    })
                    .when(!h.key.is_empty(), |d| {
                        d.child(
                            div()
                                .text_size(sp(10.5))
                                .text_color(rgb(TEXT_DIM))
                                .child(h.key),
                        )
                    }),
            );
        }
        let content = div()
            .p(px(16.))
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(self.field_cell(
                text_input::FieldId::Palette,
                input_box("Type a command — Laika or Lightroom names both work", q.is_empty()),
                false,
                "Type to search commands; ↑↓ choose, Enter runs, Esc closes",
                cx,
            ))
            .child(if hits.is_empty() {
                div()
                    .px(px(10.))
                    .text_size(sp(11.5))
                    .text_color(rgb(TEXT_DIM))
                    .child("No command by that name.")
            } else {
                div().child(list)
            })
            .child(div().text_size(sp(10.5)).text_color(rgb(TEXT_DIM)).child(
                "Lightroom command names work too; ones Laika doesn't have yet say so and where they're planned.",
            ));
        modal::modal_shell_w(content, 560.)
    }

    // ---- guide ----------------------------------------------------------------------

    pub(crate) fn open_lightroom_guide(&mut self, cx: &mut Context<Self>) {
        self.close_modals(cx);
        self.guide.open = true;
        cx.notify();
    }

    /// Newest Lightroom import report in the log folder (S01).
    fn last_lightroom_report() -> Option<PathBuf> {
        let dir = laika_core::logging::log_dir()?;
        std::fs::read_dir(dir)
            .ok()?
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with("lightroom-import-"))
            })
            .max()
    }

    pub(crate) fn lightroom_guide_modal(&self, cx: &mut Context<Self>) -> Div {
        // (Lightroom, Laika, status: 0 same, 1 partial, 2 not yet)
        const ROWS: &[(&str, &str, u8)] = &[
            (
                "Catalog (.lrcat)",
                "Catalog — one SQLite file; File → Catalog",
                0,
            ),
            (
                "Import dialog",
                "Add/Import Photos — cards, folders, copy or add in place",
                0,
            ),
            (
                "Folders",
                "Folders in the Library's left rail (move, rename, synchronize)",
                0,
            ),
            (
                "Collections",
                "Collections, with manual order, covers and captions",
                0,
            ),
            (
                "Collection sets",
                "Not yet — imported sets become name prefixes",
                2,
            ),
            ("Smart collections", "Saved filter presets come closest", 1),
            (
                "Quick Collection · target (B)",
                "Quick Collection and target collection (B)",
                0,
            ),
            (
                "Flags · stars · color labels",
                "The same keys: P X U, 0–5, 6–9",
                0,
            ),
            (
                "Keywords (hierarchical)",
                "Keyword Manager (K), paths like Places › Lisbon",
                0,
            ),
            (
                "Stacks",
                "Not yet — RAW+JPEG pairs are grouped automatically",
                2,
            ),
            (
                "Virtual copies",
                "Not yet — use named Snapshots for alternatives",
                2,
            ),
            (
                "Develop presets",
                "Presets in Develop — import yours with Import Presets…",
                1,
            ),
            (
                "Snapshots · History",
                "Snapshots and History in Develop's left rail",
                0,
            ),
            ("Masks · healing · red eye", "Not yet", 2),
            (
                "Profiles · lens profiles",
                "Not yet — Laika renders its own base look",
                2,
            ),
            (
                "XMP sidecars",
                "Written beside originals; shares Lightroom's files field by field",
                0,
            ),
            (
                "Publish Services · Web",
                "Publish — static gallery sites to a folder or Cloudflare",
                1,
            ),
            ("Map module", "Map — local by default, street map opt-in", 0),
            ("Slideshow", "Slideshow (⌘↩)", 0),
            ("Book · Print", "Not in Laika", 2),
            ("Smart Previews", "Smart previews for offline editing", 0),
        ];
        let lr_keys = commands::lightroom_keys();
        let report = Self::last_lightroom_report();
        let chip = |status: u8| {
            let (t, c) = match status {
                0 => ("same", accent_line()),
                1 => ("partly", 0xC8A15A),
                _ => ("not yet", TEXT_DIM),
            };
            div()
                .w(px(52.))
                .flex_none()
                .text_size(sp(10.))
                .text_color(rgb(c))
                .child(t)
        };
        let table = div()
            .id("guide-table")
            .max_h(px(380.))
            .overflow_y_scroll()
            .p(px(10.))
            .rounded(px(5.))
            .bg(rgb(bg_well()))
            .flex()
            .flex_col()
            .gap(px(6.))
            .children(ROWS.iter().map(|(lr, laika, st)| {
                div()
                    .flex()
                    .gap(px(10.))
                    .child(
                        div()
                            .w(px(180.))
                            .flex_none()
                            .text_size(sp(11.))
                            .text_color(rgb(TEXT_TERTIARY))
                            .child(*lr),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .text_size(sp(11.))
                            .text_color(rgb(TEXT_SECONDARY))
                            .child(*laika),
                    )
                    .child(chip(*st))
            }));
        let button = |id: &'static str, label: &str| {
            div()
                .id(id)
                .px(px(10.))
                .py(px(6.))
                .rounded(px(4.))
                .border_1()
                .border_color(border_control())
                .text_size(sp(11.))
                .text_color(rgb(TEXT_SECONDARY))
                .hover(|s| s.bg(rgb(bg_row_hover())))
                .child(label.to_string())
        };
        let content = div()
            .p(px(24.))
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_size(sp(16.))
                            .text_color(rgb(TEXT_PRIMARY))
                            .child("Laika for Lightroom Users"),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .id("guide-close")
                            .text_size(sp(15.))
                            .text_color(rgb(TEXT_DIM))
                            .hover(|d| d.text_color(rgb(TEXT_PRIMARY)))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.guide.open = false;
                                cx.notify();
                            }))
                            .child("✕"),
                    ),
            )
            .child(div().text_size(sp(12.)).text_color(rgb(TEXT_SECONDARY)).child(
                "Laika keeps Lightroom's ideas where they fit and says plainly where it doesn't have something yet. Library and Develop work the way you expect; your originals are never changed.",
            ))
            .child(table)
            .child(
                div()
                    .id("guide-keys")
                    .on_click(cx.listener(move |this, _, _, cx| this.set_lightroom_keys(!lr_keys, cx)))
                    .child(toggle::toggle(
                        lr_keys,
                        "Use Lightroom's keyboard shortcuts (W white balance, R crop, ⌥⌘1–3 modules, ⇧⌘C/V settings)",
                    )),
            )
            .child(div().text_size(sp(10.5)).text_color(rgb(TEXT_DIM)).child(
                "⇧⌘P finds any command by its Laika or Lightroom name. Keys for tools Laika doesn't have (Q, K, M in Develop) explain instead of doing nothing.",
            ))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(8.))
                    .child(button("guide-import", "Import from Lightroom…").on_click(
                        cx.listener(|this, _, _, cx| {
                            this.guide.open = false;
                            this.open_lightroom_import(cx);
                        }),
                    ))
                    .child(button("guide-presets", "Import Presets…").on_click(cx.listener(
                        |this, _, _, cx| {
                            this.guide.open = false;
                            this.open_preset_import(cx);
                        },
                    )))
                    .when_some(report, |d, p| {
                        d.child(button("guide-report", "Show Last Import Report").on_click(
                            cx.listener(move |this, _, _, cx| {
                                if let Err(e) = laika_core::import::reveal_in_manager(&p) {
                                    this.status_note = e;
                                }
                                cx.notify();
                            }),
                        ))
                    })
                    .child(div().flex_1())
                    .child(
                        div()
                            .id("guide-done")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.guide.open = false;
                                cx.notify();
                            }))
                            .child(button::primary("Done")),
                    ),
            );
        modal::modal_shell_w(content, 680.)
    }
}

#[cfg(test)]
mod tests {
    use super::{LIGHTROOM_TERMS, Target, palette_hits};
    use crate::commands::Command as C;

    #[test]
    fn lightroom_names_always_find_something() {
        // Every Lightroom name reaches its Laika command (possibly listed
        // under Laika's own title) or its honest "not yet" answer.
        for (term, target) in LIGHTROOM_TERMS {
            let hits = palette_hits(term);
            let found = hits.iter().any(|h| match (h.target, target) {
                (Target::Cmd(a), Target::Cmd(b)) => a == *b,
                (Target::Missing(a), Target::Missing(b)) => a == *b,
                _ => false,
            });
            assert!(found, "{term} doesn't reach its target");
        }
    }

    #[test]
    fn implemented_local_tools_reach_develop_and_crop_runs() {
        let hits = palette_hits("spot removal");
        assert!(
            hits.iter()
                .any(|h| matches!(h.target, Target::Cmd(C::Develop)))
        );
        let hits = palette_hits("crop");
        assert!(
            hits.iter()
                .any(|h| matches!(h.target, Target::Cmd(C::CropTool)))
        );
        let hits = palette_hits("copy settings");
        assert!(
            matches!(hits[0].target, Target::Cmd(C::CopySettings)),
            "{:?}",
            hits[0]
        );
        // Word-prefix queries.
        assert!(
            palette_hits("del rej")
                .iter()
                .any(|h| matches!(h.target, Target::Cmd(C::DeleteRejected)))
        );
    }
}
