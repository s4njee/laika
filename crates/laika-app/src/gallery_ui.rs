//! G04/G05/G03: the Publish module is the gallery editor — a galleries
//! home, then tray · canvas · inspector over one `Gallery` model with
//! snapshot undo and autosave. Canvas and tray live in `gallery_canvas`,
//! the inspector and layout picker in `gallery_inspector`.

use std::cell::Cell as StdCell;
use std::rc::Rc;

use laika_core::catalog::GallerySummary;
use laika_core::gallery::layout::{self, Breakpoint, Cell as GridCell};
use laika_core::gallery::{self, Gallery, History};

use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum TrayFilter {
    #[default]
    All,
    Unplaced,
    Picks,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum InspectorTab {
    #[default]
    Photo,
    Layout,
    Page,
}

/// A photo being dragged from the tray or the page.
#[derive(Clone, Copy, Debug)]
pub(crate) struct GalDrag {
    pub photo: i64,
    pub from_tray: bool,
    pub start: (f32, f32),
    pub pos: (f32, f32),
    pub moved: bool,
}

/// A corner-handle resize, re-applied from the drag-start model each move.
#[derive(Clone, Debug)]
pub(crate) struct GalResize {
    pub photo: i64,
    pub right: bool,
    pub bottom: bool,
    /// The fixed corner cell (inclusive), opposite the dragged handle.
    pub anchor: GridCell,
    pub start_span: (u8, u8),
    pub start: Gallery,
    pub last: Option<(GridCell, u8, u8)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GalSlider {
    TitleSize,
    Radius,
    Gutter,
    Focal,
}

pub(crate) struct Picker {
    pub choice: &'static str,
    pub category: Option<layout::Category>,
}

/// Swatch being edited in the Page tab: 0 page, 1 canvas, 2 ink,
/// 3 accent, 4.. extras, `usize::MAX` a new extra.
pub(crate) const NEW_SWATCH: usize = usize::MAX;

pub(crate) struct GalleryUi {
    pub list: Vec<GallerySummary>,
    pub current: Option<Gallery>,
    pub history: History,
    pub selected: Option<i64>,
    pub tray_filter: TrayFilter,
    pub tab: InspectorTab,
    pub breakpoint: Breakpoint,
    pub drag: Option<GalDrag>,
    pub resize: Option<GalResize>,
    pub slider: Option<GalSlider>,
    pub picker: Option<Picker>,
    pub swatch: Option<usize>,
    pub last_saved: Option<Instant>,
    pub save_error: Option<String>,
    /// Home: which gallery awaits a second click to delete.
    pub confirm_delete: Option<i64>,
    pub from_collection_open: bool,
    /// Measured each paint for hit tests (window coordinates).
    pub grid_box: Rc<StdCell<Bounds<Pixels>>>,
    pub tray_box: Rc<StdCell<Bounds<Pixels>>>,
    pub title_box: Rc<StdCell<Bounds<Pixels>>>,
    pub radius_box: Rc<StdCell<Bounds<Pixels>>>,
    pub gutter_box: Rc<StdCell<Bounds<Pixels>>>,
    pub focal_box: Rc<StdCell<Bounds<Pixels>>>,
    pub canvas_scroll: ScrollHandle,
    pub fonts_loaded: bool,
    /// When a gallery gesture last ended: the click that follows a drop
    /// must not clear the selection.
    pub gesture_end: Option<Instant>,
    pub build: Option<crate::gallery_publish::BuildRun>,
    pub publish: Option<crate::gallery_publish::PublishSheet>,
}

impl Default for GalleryUi {
    fn default() -> Self {
        let zero = || {
            Rc::new(StdCell::new(Bounds {
                origin: point(px(0.), px(0.)),
                size: size(px(0.), px(0.)),
            }))
        };
        Self {
            list: Vec::new(),
            current: None,
            history: History::default(),
            selected: None,
            tray_filter: TrayFilter::All,
            tab: InspectorTab::Photo,
            breakpoint: Breakpoint::Desktop,
            drag: None,
            resize: None,
            slider: None,
            picker: None,
            swatch: None,
            last_saved: None,
            save_error: None,
            confirm_delete: None,
            from_collection_open: false,
            grid_box: zero(),
            tray_box: zero(),
            title_box: zero(),
            radius_box: zero(),
            gutter_box: zero(),
            focal_box: zero(),
            canvas_scroll: ScrollHandle::new(),
            fonts_loaded: false,
            gesture_end: None,
            build: None,
            publish: None,
        }
    }
}

/// Published-page font families (registered from `assets/fonts`).
pub(crate) const PLEX_SANS: &str = "IBM Plex Sans";
pub(crate) const PLEX_MONO: &str = "IBM Plex Mono";

pub(crate) fn in_bounds(b: Bounds<Pixels>, pos: (f32, f32)) -> bool {
    let (x, y) = (b.origin.x.as_f32(), b.origin.y.as_f32());
    b.size.width.as_f32() > 1.
        && pos.0 >= x
        && pos.1 >= y
        && pos.0 <= x + b.size.width.as_f32()
        && pos.1 <= y + b.size.height.as_f32()
}

/// Bounds meter: records an element's window bounds each paint.
pub(crate) fn meter(slot: Rc<StdCell<Bounds<Pixels>>>) -> impl IntoElement {
    canvas(
        move |b, _, _| {
            slot.set(b);
        },
        |_, _, _, _| {},
    )
    .absolute()
    .size_full()
}

impl Laika {
    // ---- loading ----------------------------------------------------------------

    /// Register the Plex faces once so the canvas previews published type.
    pub(crate) fn ensure_gallery_fonts(&mut self, cx: &mut Context<Self>) {
        if self.gal.fonts_loaded {
            return;
        }
        self.gal.fonts_loaded = true;
        let fonts: Vec<std::borrow::Cow<'static, [u8]>> = vec![
            std::borrow::Cow::Borrowed(include_bytes!("../../../assets/fonts/IBMPlexSans-Regular.ttf")),
            std::borrow::Cow::Borrowed(include_bytes!("../../../assets/fonts/IBMPlexSans-Medium.ttf")),
            std::borrow::Cow::Borrowed(include_bytes!("../../../assets/fonts/IBMPlexSans-SemiBold.ttf")),
            std::borrow::Cow::Borrowed(include_bytes!("../../../assets/fonts/IBMPlexMono-Regular.ttf")),
            std::borrow::Cow::Borrowed(include_bytes!("../../../assets/fonts/IBMPlexMono-Medium.ttf")),
        ];
        if let Err(e) = cx.text_system().add_fonts(fonts) {
            eprintln!("[gallery] fonts: {e}");
        }
    }

    pub(crate) fn load_galleries(&mut self) {
        self.gal.list = self
            .catalog
            .as_ref()
            .map(|c| c.galleries())
            .unwrap_or_default();
        // A gallery deleted elsewhere (or a catalog switch) closes.
        if let Some(g) = self.gal.current.as_ref() {
            if !self.gal.list.iter().any(|s| s.id == g.id) {
                self.close_gallery();
            }
        }
    }

    pub(crate) fn open_gallery(&mut self, id: i64, cx: &mut Context<Self>) {
        let Some(cat) = self.catalog.as_ref() else {
            return;
        };
        match cat.load_gallery(id) {
            Ok(g) => {
                self.gal.selected = g
                    .layout_at(Breakpoint::Desktop)
                    .first()
                    .map(|(i, ..)| g.photos[*i].photo_id);
                self.gal.current = Some(g);
                self.gal.history.clear();
                self.gal.save_error = None;
                self.gal.last_saved = None;
                self.gal.breakpoint = Breakpoint::Desktop;
                self.gal.swatch = None;
                self.gal.picker = None;
                self.gal.publish = None;
                self.gal.confirm_delete = None;
            }
            Err(e) => self.status_note = e,
        }
        cx.notify();
    }

    pub(crate) fn close_gallery(&mut self) {
        self.gal.current = None;
        self.gal.history.clear();
        self.gal.selected = None;
        self.gal.drag = None;
        self.gal.resize = None;
        self.gal.picker = None;
        self.gal.publish = None;
        self.gal.slider = None;
    }

    // ---- commands ------------------------------------------------------------------

    /// Apply one undoable change to the open gallery and autosave.
    /// `coalesce` merges continuous gestures into one undo step.
    pub(crate) fn gal_edit(
        &mut self,
        label: &str,
        coalesce: Option<&str>,
        f: impl FnOnce(&mut Gallery),
        cx: &mut Context<Self>,
    ) {
        let Some(g) = self.gal.current.as_mut() else {
            return;
        };
        let before = g.clone();
        f(g);
        if *g == before {
            return;
        }
        self.gal.history.record(&before, label, coalesce);
        self.gal_save();
        cx.notify();
    }

    /// Autosave: the whole model in one transaction (small and fast).
    pub(crate) fn gal_save(&mut self) {
        let (Some(cat), Some(g)) = (self.catalog.as_ref(), self.gal.current.as_mut()) else {
            return;
        };
        match cat.save_gallery(g) {
            Ok(stamp) => {
                g.updated_at = stamp;
                self.gal.last_saved = Some(Instant::now());
                self.gal.save_error = None;
            }
            Err(e) => {
                self.gal.save_error = Some(e.clone());
                self.status_note = format!("gallery not saved — {e}");
            }
        }
        if let Some(s) = self
            .gal
            .current
            .as_ref()
            .and_then(|g| self.gal.list.iter_mut().find(|s| s.id == g.id).map(|s| (s, g)))
        {
            let (sum, g) = s;
            sum.title = g.title.clone();
            sum.slug = g.slug.clone();
            sum.status = g.status;
            sum.photo_count = g.photos.len();
            sum.cover_photo_id = g.photos.first().map(|p| p.photo_id);
            sum.updated_at = g.updated_at.clone();
        }
    }

    pub(crate) fn gal_undo(&mut self, cx: &mut Context<Self>) {
        let Some(g) = self.gal.current.as_ref() else {
            return;
        };
        match self.gal.history.undo(g) {
            Some((prev, label)) => {
                self.gal.current = Some(prev);
                self.gal_save();
                self.status_note = format!("undid {label}");
                self.gal_fix_selection();
            }
            None => self.status_note = "nothing to undo in this gallery".to_string(),
        }
        cx.notify();
    }

    pub(crate) fn gal_redo(&mut self, cx: &mut Context<Self>) {
        let Some(g) = self.gal.current.as_ref() else {
            return;
        };
        match self.gal.history.redo(g) {
            Some((next, label)) => {
                self.gal.current = Some(next);
                self.gal_save();
                self.status_note = format!("redid {label}");
                self.gal_fix_selection();
            }
            None => self.status_note = "nothing to redo".to_string(),
        }
        cx.notify();
    }

    fn gal_fix_selection(&mut self) {
        let keep = self
            .gal
            .current
            .as_ref()
            .zip(self.gal.selected)
            .is_some_and(|(g, id)| g.index_of(id).is_some());
        if !keep {
            self.gal.selected = None;
        }
    }

    // ---- creating galleries -------------------------------------------------------

    fn unique_slug(&self, base: &str, except: i64) -> String {
        let Some(cat) = self.catalog.as_ref() else {
            return base.to_string();
        };
        if base.is_empty() || cat.slug_owner(base, except).is_none() {
            return base.to_string();
        }
        (2..1000)
            .map(|n| format!("{base}-{n}"))
            .find(|s| cat.slug_owner(s, except).is_none())
            .unwrap_or_default()
    }

    /// New gallery: from the given photos (in visible order), placed with
    /// the default template, opened on the Page tab to name it.
    pub(crate) fn new_gallery(&mut self, title: &str, photos: Vec<i64>, cx: &mut Context<Self>) {
        let Some(cat) = self.catalog.as_ref() else {
            self.status_note = "open a catalog first".to_string();
            cx.notify();
            return;
        };
        let id = match cat.create_gallery(title) {
            Ok(id) => id,
            Err(e) => {
                self.status_note = e;
                cx.notify();
                return;
            }
        };
        let ordered = self.in_visible_order(photos);
        let slug = self.unique_slug(&gallery::derive_slug(title), id);
        if let Some(cat) = self.catalog.as_ref() {
            if let Ok(mut g) = cat.load_gallery(id) {
                g.add_photos(&ordered);
                g.slug = slug;
                let tpl = g.template.clone();
                g.apply_template(&tpl);
                if let Err(e) = cat.save_gallery(&g) {
                    self.status_note = e;
                }
            }
        }
        self.load_galleries();
        self.open_gallery(id, cx);
        self.gal.tab = if ordered.is_empty() {
            InspectorTab::Page
        } else {
            InspectorTab::Photo
        };
        self.status_note = format!(
            "new gallery with {} photo{}",
            ordered.len(),
            if ordered.len() == 1 { "" } else { "s" }
        );
    }

    pub(crate) fn new_gallery_from_collection(&mut self, cid: i64, cx: &mut Context<Self>) {
        let Some(cat) = self.catalog.as_ref() else {
            return;
        };
        match cat.create_gallery_from_collection(cid) {
            Ok(id) => {
                // Give it a free web address from its title.
                if let Ok(mut g) = cat.load_gallery(id) {
                    g.slug = self.unique_slug(&gallery::derive_slug(&g.title), id);
                    if let Some(cat) = self.catalog.as_ref() {
                        cat.save_gallery(&g).ok();
                    }
                }
                self.load_galleries();
                self.open_gallery(id, cx);
                self.status_note = "gallery created from the album's order and captions".to_string();
            }
            Err(e) => self.status_note = e,
        }
        self.gal.from_collection_open = false;
        cx.notify();
    }

    /// Add the Library selection to the open gallery (placed at the end).
    pub(crate) fn gal_add_selection(&mut self, cx: &mut Context<Self>) {
        let ids = self.in_visible_order(self.state.selection.iter().copied().collect());
        if ids.is_empty() {
            self.status_note =
                "select photos in the Library first, then add them here".to_string();
            cx.notify();
            return;
        }
        let mut added = 0;
        self.gal_edit(
            "Add photos",
            None,
            |g| {
                added = g.add_photos(&ids);
                for id in &ids {
                    g.place_next(*id);
                }
            },
            cx,
        );
        self.status_note = if added == 0 {
            "those photos are already in this gallery".to_string()
        } else {
            format!("added {added} photo{}", if added == 1 { "" } else { "s" })
        };
        cx.notify();
    }

    // ---- fields ------------------------------------------------------------------------

    pub(crate) fn gallery_field_text(&self, id: text_input::FieldId) -> String {
        use text_input::FieldId as F;
        let Some(g) = self.gal.current.as_ref() else {
            return String::new();
        };
        let photo = self.gal.selected.and_then(|pid| g.index_of(pid)).map(|i| &g.photos[i]);
        match id {
            F::GalleryTitle => g.title.clone(),
            F::GalleryEyebrow => g.eyebrow.clone(),
            F::GallerySubtitle => g.subtitle.clone(),
            F::GallerySlug => g.slug.clone(),
            F::GallerySite => g.site_name.clone(),
            F::GalleryMeta => g.meta_line.clone(),
            F::GalleryCaption => photo.map(|p| p.caption.clone()).unwrap_or_default(),
            F::GalleryAlt => photo.map(|p| p.alt_text.clone()).unwrap_or_default(),
            F::GalleryHex => {
                let t = &g.theme;
                let c = match self.gal.swatch {
                    Some(0) => t.page,
                    Some(1) => t.canvas,
                    Some(2) => t.ink,
                    Some(3) | Some(NEW_SWATCH) | None => t.accent,
                    Some(n) => t.extras.get(n - 4).copied().unwrap_or(t.accent),
                };
                if self.gal.swatch == Some(NEW_SWATCH) {
                    String::new()
                } else {
                    format!("#{:06X}", c)
                }
            }
            F::GalleryOutputDir => g.output_dir.clone(),
            F::GalleryProject => g.deploy_project.clone(),
            _ => String::new(),
        }
    }

    pub(crate) fn commit_gallery_field(
        &mut self,
        id: text_input::FieldId,
        buf: &str,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        use text_input::FieldId as F;
        let Some(g) = self.gal.current.as_ref() else {
            return Err("open a gallery first".to_string());
        };
        let gid = g.id;
        let text = buf.trim().to_string();
        let long = |max: usize, what: &str| -> Result<(), String> {
            if text.chars().count() > max {
                Err(format!("keep the {what} under {max} characters"))
            } else {
                Ok(())
            }
        };
        match id {
            F::GalleryTitle => {
                long(200, "title")?;
                // The web address follows the title until it's hand-edited.
                let follows = g.slug.is_empty() || g.slug == gallery::derive_slug(&g.title);
                let slug = follows.then(|| self.unique_slug(&gallery::derive_slug(&text), gid));
                self.gal_edit("Title", None, |g| {
                    g.title = text;
                    if let Some(s) = slug {
                        g.slug = s;
                    }
                }, cx);
            }
            F::GallerySlug => {
                if !text.is_empty() {
                    gallery::validate_slug(&text)?;
                    if let Some((_, title)) = self.catalog.as_ref().and_then(|c| c.slug_owner(&text, gid)) {
                        let who = if title.is_empty() { "another gallery".to_string() } else { format!("“{title}”") };
                        return Err(format!("{who} already uses {text}"));
                    }
                }
                self.gal_edit("Web address", None, |g| g.slug = text, cx);
                self.refresh_publish_diff_pub();
            }
            F::GalleryEyebrow => {
                long(80, "eyebrow")?;
                self.gal_edit("Eyebrow", None, |g| g.eyebrow = text, cx);
            }
            F::GallerySubtitle => {
                long(600, "lede")?;
                self.gal_edit("Lede", None, |g| g.subtitle = text, cx);
            }
            F::GallerySite => {
                long(80, "site name")?;
                self.gal_edit("Site name", None, |g| g.site_name = text, cx);
            }
            F::GalleryMeta => {
                long(160, "meta line")?;
                self.gal_edit("Meta line", None, |g| g.meta_line = text, cx);
            }
            F::GalleryCaption | F::GalleryAlt => {
                long(2000, if id == F::GalleryCaption { "caption" } else { "alt text" })?;
                let pid = self.gal.selected.ok_or("select a photo first")?;
                let is_caption = id == F::GalleryCaption;
                self.gal_edit(if is_caption { "Caption" } else { "Alt text" }, None, |g| {
                    if let Some(i) = g.index_of(pid) {
                        if is_caption {
                            g.photos[i].caption = text;
                        } else {
                            g.photos[i].alt_text = text;
                        }
                    }
                }, cx);
            }
            F::GalleryHex => {
                if text.is_empty() && self.gal.swatch == Some(NEW_SWATCH) {
                    self.gal.swatch = None;
                    return Ok(());
                }
                let c = theme::parse_hex(&text).ok_or("use a hex color like #121517")?;
                let slot = self.gal.swatch.ok_or("pick a swatch first")?;
                self.gal_edit("Palette", None, |g| {
                    let t = &mut g.theme;
                    match slot {
                        0 => t.page = c,
                        1 => t.canvas = c,
                        2 => t.ink = c,
                        3 => t.accent = c,
                        NEW_SWATCH => {
                            if t.extras.len() < 8 {
                                t.extras.push(c);
                            }
                        }
                        n => {
                            if let Some(x) = t.extras.get_mut(n - 4) {
                                *x = c;
                            }
                        }
                    }
                }, cx);
                if slot == NEW_SWATCH {
                    self.gal.swatch = None;
                }
            }
            F::GalleryOutputDir => {
                let expanded = if let Some(rest) = text.strip_prefix("~/") {
                    std::env::var("HOME").map(|h| format!("{h}/{rest}")).unwrap_or(text.clone())
                } else {
                    text.clone()
                };
                if !expanded.is_empty() && !std::path::Path::new(&expanded).is_absolute() {
                    return Err("use a full folder path, like ~/Sites/hokkaido".to_string());
                }
                self.gal_edit("Output folder", None, |g| g.output_dir = expanded, cx);
                self.refresh_publish_diff_pub();
            }
            F::GalleryProject => {
                if !text.is_empty()
                    && !text.chars().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
                {
                    return Err("Pages projects use lowercase letters, digits and dashes".to_string());
                }
                long(58, "project name")?;
                self.gal_edit("Pages project", None, |g| g.deploy_project = text, cx);
            }
            _ => {}
        }
        Ok(())
    }

    // ---- keyboard ------------------------------------------------------------------

    /// Publish-module keys. Returns true when consumed.
    pub(crate) fn gallery_key(&mut self, key: &str, shift: bool, cmd: bool, cx: &mut Context<Self>) -> bool {
        if self.state.active_module != Module::Publish {
            return false;
        }
        if self.gal.publish.is_some() {
            if key == "escape" {
                self.close_publish_sheet(cx);
                return true;
            }
            return !cmd;
        }
        if let Some(p) = self.gal.picker.as_mut() {
            let visible: Vec<&'static str> = layout::TEMPLATES
                .iter()
                .filter(|t| p.category.is_none_or(|c| c == t.category))
                .map(|t| t.id)
                .collect();
            let at = visible.iter().position(|id| *id == p.choice).unwrap_or(0);
            match key {
                "escape" => self.gal.picker = None,
                "enter" => self.apply_picker(cx),
                "right" if at + 1 < visible.len() => p.choice = visible[at + 1],
                "left" if at > 0 => p.choice = visible[at - 1],
                "down" if at + 4 < visible.len() => p.choice = visible[at + 4],
                "up" if at >= 4 => p.choice = visible[at - 4],
                _ => {}
            }
            cx.notify();
            return true;
        }
        if self.gal.current.is_none() {
            return false;
        }
        if cmd {
            match key {
                "z" if shift => self.gal_redo(cx),
                "z" => self.gal_undo(cx),
                "p" if shift => self.open_publish_sheet(cx),
                "p" => self.preview_gallery(cx),
                _ => return false,
            }
            return true;
        }
        let editable = self.gal.breakpoint == Breakpoint::Desktop;
        let sel = self.gal.selected;
        match key {
            "escape" => {
                if self.gal.drag.take().is_some() || self.gal.resize.is_some() {
                    if let Some(r) = self.gal.resize.take() {
                        self.gal.current = Some(r.start);
                        self.gal_save();
                    }
                } else if self.gal.selected.take().is_none() {
                    return false;
                }
                cx.notify();
            }
            "delete" | "backspace" => {
                let Some(id) = sel else {
                    return true;
                };
                if shift {
                    self.gal_edit("Remove from gallery", None, |g| {
                        g.remove_photos(&[id]);
                    }, cx);
                    self.gal.selected = None;
                } else {
                    self.gal_edit("Remove from page", None, |g| g.unplace(id), cx);
                }
            }
            "left" | "right" | "up" | "down" if editable => {
                let Some(id) = sel else {
                    return true;
                };
                let Some(cell) = self
                    .gal
                    .current
                    .as_ref()
                    .and_then(|g| g.index_of(id).and_then(|i| g.photos[i].cell))
                else {
                    return true;
                };
                let (c, r) = (cell.col as i32, cell.row as i32);
                let (c, r) = match key {
                    "left" => (c - 1, r),
                    "right" => (c + 1, r),
                    "up" => (c, r - 1),
                    _ => (c, r + 1),
                };
                if c < 0 || r < 0 {
                    return true;
                }
                let cols = self.gal.current.as_ref().map(|g| g.columns).unwrap_or(3) as i32;
                let sx = self
                    .gal
                    .current
                    .as_ref()
                    .and_then(|g| g.index_of(id).map(|i| g.photos[i].span_x))
                    .unwrap_or(1) as i32;
                if c + sx > cols {
                    return true;
                }
                let to = GridCell { col: c as u16, row: r as u16 };
                self.gal_edit("Move", Some("arrow-move"), |g| g.place(id, to), cx);
            }
            "1" | "2" | "3" | "4" if editable => {
                if let Some(id) = sel {
                    self.gal_set_span_preset(id, key.parse::<u8>().unwrap_or(1), cx);
                }
            }
            _ => return false,
        }
        true
    }

    /// Span presets: 1 = 1×1, 2 = 2×1, 3 = 2×2, 4 = full width.
    pub(crate) fn gal_set_span_preset(&mut self, id: i64, preset: u8, cx: &mut Context<Self>) {
        let cols = self.gal.current.as_ref().map(|g| g.columns).unwrap_or(3);
        let (sx, sy) = match preset {
            1 => (1, 1),
            2 => (2, 1),
            3 => (2, 2),
            _ => (cols, 1),
        };
        let label = format!("Span {}×{}", sx.min(cols), sy);
        self.gal_edit(&label, None, |g| {
            if g.index_of(id).is_some_and(|i| g.photos[i].cell.is_none()) {
                g.place_next(id);
            }
            g.set_span(id, sx.min(cols), sy)
        }, cx);
    }

    // ---- pointer --------------------------------------------------------------------

    /// Window mouse-move while a gallery gesture is active. True = handled.
    pub(crate) fn gallery_mouse_move(&mut self, pos: (f32, f32), pressed: bool, shift: bool, cx: &mut Context<Self>) -> bool {
        if self.state.active_module != Module::Publish {
            return false;
        }
        if let Some(kind) = self.gal.slider {
            if !pressed {
                self.gallery_mouse_up(pos, cx);
                return true;
            }
            self.gal_slider_to(kind, pos, cx);
            return true;
        }
        if self.gal.resize.is_some() {
            if !pressed {
                self.gallery_mouse_up(pos, cx);
                return true;
            }
            self.gal_resize_to(pos, shift, cx);
            return true;
        }
        if let Some(mut d) = self.gal.drag {
            if !pressed {
                self.gal.drag = None;
                cx.notify();
                return true;
            }
            d.pos = pos;
            if !d.moved && (d.start.0 - pos.0).abs() + (d.start.1 - pos.1).abs() > 5. {
                d.moved = true;
            }
            self.gal.drag = Some(d);
            if d.moved {
                cx.notify();
            }
            return true;
        }
        false
    }

    pub(crate) fn gallery_mouse_up(&mut self, pos: (f32, f32), cx: &mut Context<Self>) -> bool {
        if let Some(kind) = self.gal.slider.take() {
            self.gal.gesture_end = Some(Instant::now());
            self.gal.history.seal();
            if kind == GalSlider::Focal {
                self.status_note = "focal point set".to_string();
            }
            cx.notify();
            return true;
        }
        if let Some(r) = self.gal.resize.take() {
            self.gal.gesture_end = Some(Instant::now());
            self.gal.history.seal();
            if let Some((_, sx, sy)) = r.last {
                self.status_note = format!("span {sx} × {sy}");
            }
            cx.notify();
            return true;
        }
        let Some(d) = self.gal.drag.take() else {
            return false;
        };
        self.gal.gesture_end = Some(Instant::now());
        if !d.moved {
            // A click selects (tray or page).
            self.gal.selected = Some(d.photo);
            self.gal.tab = InspectorTab::Photo;
            cx.notify();
            return true;
        }
        if self.gal.breakpoint != Breakpoint::Desktop {
            self.status_note = "switch to Desktop to arrange photos".to_string();
            cx.notify();
            return true;
        }
        if let Some(cell) = self.gal_drop_cell(&d, pos) {
            let id = d.photo;
            let label = if d.from_tray { "Place photo" } else { "Move photo" };
            self.gal_edit(label, None, |g| g.place(id, cell), cx);
            self.gal.selected = Some(id);
        } else if !d.from_tray && in_bounds(self.gal.tray_box.get(), pos) {
            let id = d.photo;
            self.gal_edit("Remove from page", None, |g| g.unplace(id), cx);
        }
        cx.notify();
        true
    }
}
