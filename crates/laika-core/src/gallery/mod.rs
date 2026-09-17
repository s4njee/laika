//! G01/G03: galleries — the model the Publish editor edits and the site
//! builder renders. Layout math lives in [`layout`]; persistence in
//! `catalog.rs` (tables `galleries`, `gallery_photos`).

pub mod layout;

use layout::{Breakpoint, Cell, Slot};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Status {
    #[default]
    Draft,
    Published,
}

impl Status {
    pub fn key(self) -> &'static str {
        match self {
            Status::Draft => "draft",
            Status::Published => "published",
        }
    }

    pub fn parse(s: &str) -> Self {
        if s == "published" {
            Status::Published
        } else {
            Status::Draft
        }
    }
}

/// How a photo fills its tile.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Fit {
    /// Crop to fill (focal point decides what stays).
    #[default]
    Fill,
    /// Letterbox the whole photo inside the tile.
    Fit,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum TypePairing {
    /// Plex Sans display — tight, all weights.
    #[default]
    SansDisplay,
    /// Plex Sans + Mono — technical captions.
    SansMono,
    /// Light display — airy, low contrast.
    LightDisplay,
}

impl TypePairing {
    pub const ALL: [TypePairing; 3] = [
        TypePairing::SansDisplay,
        TypePairing::SansMono,
        TypePairing::LightDisplay,
    ];

    pub fn name(self) -> &'static str {
        match self {
            TypePairing::SansDisplay => "Plex Sans display",
            TypePairing::SansMono => "Plex Sans + Mono",
            TypePairing::LightDisplay => "Light display",
        }
    }

    pub fn descriptor(self) -> &'static str {
        match self {
            TypePairing::SansDisplay => "tight, all weights",
            TypePairing::SansMono => "technical captions",
            TypePairing::LightDisplay => "airy, low contrast",
        }
    }
}

/// The published page's look (defaults from the design handoff).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Theme {
    pub pairing: TypePairing,
    pub title_size: u8,
    pub page: u32,
    pub canvas: u32,
    pub ink: u32,
    pub accent: u32,
    /// Extra palette swatches the user added.
    pub extras: Vec<u32>,
    pub show_captions: bool,
    pub hover_zoom: bool,
    pub corner_radius: u8,
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            pairing: TypePairing::SansDisplay,
            title_size: 56,
            page: 0x121517,
            canvas: 0x0F1113,
            ink: 0xF2EFE6,
            accent: 0x080147,
            extras: Vec::new(),
            show_captions: true,
            hover_zoom: false,
            corner_radius: 0,
        }
    }
}

impl Theme {
    /// WCAG relative luminance of an sRGB hex.
    pub fn luminance(rgb: u32) -> f64 {
        let ch = |v: u32| {
            let c = v as f64 / 255.;
            if c <= 0.03928 {
                c / 12.92
            } else {
                ((c + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * ch((rgb >> 16) & 0xFF) + 0.7152 * ch((rgb >> 8) & 0xFF) + 0.0722 * ch(rgb & 0xFF)
    }

    pub fn contrast(a: u32, b: u32) -> f64 {
        let (la, lb) = (Self::luminance(a), Self::luminance(b));
        let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
        (hi + 0.05) / (lo + 0.05)
    }

    /// Accent color for text on the page: the accent itself when it reads
    /// (≥ 4.5:1), else the same hue with moderated saturation, lightened
    /// (or darkened on light pages) until it reads comfortably (≥ 6:1) —
    /// the handoff's #9B93E8 for #080147 on #121517.
    pub fn accent_text(&self) -> u32 {
        if Self::contrast(self.accent, self.page) >= 4.5 {
            return self.accent;
        }
        let (h, s, _) = rgb_to_hsl(self.accent);
        let s = s.min(0.66);
        let dark_page = Self::luminance(self.page) < 0.18;
        for step in 0..=100 {
            let t = step as f64 / 100.;
            let l = if dark_page { t } else { 1. - t };
            let c = hsl_to_rgb(h, s, l);
            if Self::contrast(c, self.page) >= 6.0 {
                return c;
            }
        }
        self.ink
    }
}

fn rgb_to_hsl(rgb: u32) -> (f64, f64, f64) {
    let r = ((rgb >> 16) & 0xFF) as f64 / 255.;
    let g = ((rgb >> 8) & 0xFF) as f64 / 255.;
    let b = (rgb & 0xFF) as f64 / 255.;
    let (max, min) = (r.max(g).max(b), r.min(g).min(b));
    let l = (max + min) / 2.;
    if max == min {
        return (0., 0., l);
    }
    let d = max - min;
    let s = if l > 0.5 { d / (2. - max - min) } else { d / (max + min) };
    let h = if max == r {
        (g - b) / d + if g < b { 6. } else { 0. }
    } else if max == g {
        (b - r) / d + 2.
    } else {
        (r - g) / d + 4.
    };
    (h / 6., s, l)
}

fn hsl_to_rgb(h: f64, s: f64, l: f64) -> u32 {
    let hue = |p: f64, q: f64, mut t: f64| {
        if t < 0. {
            t += 1.;
        }
        if t > 1. {
            t -= 1.;
        }
        if t < 1. / 6. {
            p + (q - p) * 6. * t
        } else if t < 0.5 {
            q
        } else if t < 2. / 3. {
            p + (q - p) * (2. / 3. - t) * 6.
        } else {
            p
        }
    };
    let (r, g, b) = if s == 0. {
        (l, l, l)
    } else {
        let q = if l < 0.5 { l * (1. + s) } else { l + s - l * s };
        let p = 2. * l - q;
        (hue(p, q, h + 1. / 3.), hue(p, q, h), hue(p, q, h - 1. / 3.))
    };
    let c = |v: f64| (v * 255.).round().clamp(0., 255.) as u32;
    (c(r) << 16) | (c(g) << 8) | c(b)
}

/// One photo's membership and presentation in a gallery.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct GalleryPhoto {
    pub photo_id: i64,
    /// Desktop placement; None while in the tray only.
    pub cell: Option<Cell>,
    pub span_x: u8,
    pub span_y: u8,
    pub caption: String,
    pub alt_text: String,
    /// Normalized focal point (0..1), used as object-position.
    pub focal: (f32, f32),
    pub fit: Fit,
    pub open_full_size: bool,
}

impl GalleryPhoto {
    pub fn new(photo_id: i64) -> Self {
        Self {
            photo_id,
            cell: None,
            span_x: 1,
            span_y: 1,
            caption: String::new(),
            alt_text: String::new(),
            focal: (0.5, 0.5),
            fit: Fit::Fill,
            open_full_size: true,
        }
    }

    fn slot(&self) -> Slot {
        Slot {
            span_x: self.span_x,
            span_y: self.span_y,
            cell: self.cell,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Gallery {
    pub id: i64,
    pub title: String,
    pub subtitle: String,
    pub eyebrow: String,
    pub slug: String,
    pub status: Status,
    pub template: String,
    pub columns: u8,
    pub gutter: u8,
    /// Row ratio (width / height of one cell).
    pub ratio: f32,
    pub theme: Theme,
    pub sizes: Vec<u32>,
    pub allow_downloads: bool,
    pub strip_gps: bool,
    pub site_name: String,
    pub meta_line: String,
    pub output_dir: String,
    pub deploy_project: String,
    pub created_at: String,
    pub updated_at: String,
    pub last_build_at: String,
    pub last_build_dir: String,
    pub last_deploy_at: String,
    pub last_deploy_url: String,
    /// Tray order (also the reading order new placements follow).
    pub photos: Vec<GalleryPhoto>,
}

impl Default for Gallery {
    fn default() -> Self {
        let t = layout::template("mixed");
        Self {
            id: 0,
            title: String::new(),
            subtitle: String::new(),
            eyebrow: String::new(),
            slug: String::new(),
            status: Status::Draft,
            template: t.id.to_string(),
            columns: t.columns,
            gutter: t.gutter,
            ratio: t.ratio,
            theme: Theme::default(),
            sizes: vec![640, 1280, 2048],
            allow_downloads: false,
            strip_gps: true,
            site_name: String::new(),
            meta_line: String::new(),
            output_dir: String::new(),
            deploy_project: String::new(),
            created_at: String::new(),
            updated_at: String::new(),
            last_build_at: String::new(),
            last_build_dir: String::new(),
            last_deploy_at: String::new(),
            last_deploy_url: String::new(),
            photos: Vec::new(),
        }
    }
}

/// Lowercase ASCII words joined by `-` (non-ASCII letters drop).
pub fn derive_slug(title: &str) -> String {
    crate::state::PublishForm::derive_slug(title)
}

/// A slug the web will accept: lowercase letters, digits, and dashes.
pub fn validate_slug(slug: &str) -> Result<(), String> {
    if slug.is_empty() {
        return Err("set a web address (slug) first".to_string());
    }
    if slug.len() > 80 {
        return Err("keep the slug under 80 characters".to_string());
    }
    if !slug
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        || slug.starts_with('-')
        || slug.ends_with('-')
    {
        return Err("use lowercase letters, digits and dashes (like hokkaido-2026)".to_string());
    }
    Ok(())
}

impl Gallery {
    pub fn index_of(&self, photo_id: i64) -> Option<usize> {
        self.photos.iter().position(|p| p.photo_id == photo_id)
    }

    pub fn placed_count(&self) -> usize {
        self.photos.iter().filter(|p| p.cell.is_some()).count()
    }

    /// Add photos to the tray (duplicates ignored). Returns how many joined.
    pub fn add_photos(&mut self, ids: &[i64]) -> usize {
        let mut n = 0;
        for id in ids {
            if self.index_of(*id).is_none() {
                self.photos.push(GalleryPhoto::new(*id));
                n += 1;
            }
        }
        n
    }

    /// Remove photos from the gallery entirely (tray included).
    pub fn remove_photos(&mut self, ids: &[i64]) -> usize {
        let before = self.photos.len();
        self.photos.retain(|p| !ids.contains(&p.photo_id));
        before - self.photos.len()
    }

    fn slots(&self) -> Vec<Slot> {
        self.photos.iter().map(|p| p.slot()).collect()
    }

    fn set_cells(&mut self, cells: Vec<Option<Cell>>) {
        for (p, c) in self.photos.iter_mut().zip(cells) {
            p.cell = c;
        }
    }

    /// Drop a photo onto a cell (placing it from the tray, or moving it).
    pub fn place(&mut self, photo_id: i64, cell: Cell) {
        let Some(i) = self.index_of(photo_id) else {
            return;
        };
        if self.photos[i].cell.is_none() {
            let t = layout::template(&self.template);
            let order = self.placed_count();
            let (sx, sy) = t.span_for(order, self.columns);
            self.photos[i].span_x = sx;
            self.photos[i].span_y = sy;
        }
        let cells = layout::place_at(&self.slots(), i, cell, self.columns);
        self.set_cells(cells);
    }

    /// Place at the first free cell after everything already placed.
    pub fn place_next(&mut self, photo_id: i64) {
        let Some(i) = self.index_of(photo_id) else {
            return;
        };
        if self.photos[i].cell.is_some() {
            return;
        }
        let t = layout::template(&self.template);
        let (sx, sy) = t.span_for(self.placed_count(), self.columns);
        self.photos[i].span_x = sx;
        self.photos[i].span_y = sy;
        // Flow only placed photos plus this one, in reading order.
        let mut order: Vec<usize> = layout::reading_order(
            &self.photos.iter().map(|p| p.cell).collect::<Vec<_>>(),
        );
        order.push(i);
        let slots: Vec<Slot> = order.iter().map(|&k| self.photos[k].slot()).collect();
        let cells = layout::flow(&slots, self.columns);
        for (k, c) in order.into_iter().zip(cells) {
            self.photos[k].cell = Some(c);
        }
    }

    /// Back to the tray (stays in the gallery).
    pub fn unplace(&mut self, photo_id: i64) {
        if let Some(i) = self.index_of(photo_id) {
            self.photos[i].cell = None;
        }
    }

    pub fn set_span(&mut self, photo_id: i64, span_x: u8, span_y: u8) {
        let Some(i) = self.index_of(photo_id) else {
            return;
        };
        let cells = layout::set_span(&self.slots(), i, span_x, span_y, self.columns);
        self.photos[i].span_x = span_x.clamp(1, self.columns.max(1));
        self.photos[i].span_y = span_y.clamp(1, 8);
        self.set_cells(cells);
    }

    /// Switch template: order, captions, alt text, focal and fit stay;
    /// spans reset to the template's rule; every photo is placed.
    pub fn apply_template(&mut self, id: &str) {
        let t = layout::template(id);
        self.template = t.id.to_string();
        self.columns = t.columns;
        self.gutter = t.gutter;
        self.ratio = t.ratio;
        self.theme.show_captions = t.captions;
        self.reflow_all();
    }

    /// Re-flow every photo in tray order with the template's spans (used
    /// by apply and by column changes).
    pub fn reflow_all(&mut self) {
        let t = *layout::template(&self.template);
        let placed = layout::apply_template(self.photos.len(), &t, self.columns);
        for (p, (c, sx, sy)) in self.photos.iter_mut().zip(placed) {
            p.cell = Some(c);
            p.span_x = sx;
            p.span_y = sy;
        }
    }

    /// Change the column count, keeping placed photos in reading order.
    pub fn set_columns(&mut self, columns: u8) {
        let columns = columns.clamp(1, 6);
        if columns == self.columns {
            return;
        }
        self.columns = columns;
        let order = layout::reading_order(&self.photos.iter().map(|p| p.cell).collect::<Vec<_>>());
        let slots: Vec<Slot> = order
            .iter()
            .map(|&k| Slot {
                span_x: self.photos[k].span_x.min(columns),
                span_y: self.photos[k].span_y,
                cell: None,
            })
            .collect();
        let cells = layout::flow(&slots, columns);
        for (k, c) in order.into_iter().zip(cells) {
            let p = &mut self.photos[k];
            p.span_x = p.span_x.min(columns);
            p.cell = Some(c);
        }
    }

    /// Placed photos at a breakpoint: (photo index, cell, span_x, span_y).
    pub fn layout_at(&self, bp: Breakpoint) -> Vec<(usize, Cell, u8, u8)> {
        let slots = self.slots();
        if bp == Breakpoint::Desktop {
            let cells: Vec<Option<Cell>> = slots.iter().map(|s| s.cell).collect();
            return layout::reading_order(&cells)
                .into_iter()
                .map(|i| {
                    let p = &self.photos[i];
                    (i, p.cell.expect("placed"), p.span_x.min(self.columns), p.span_y)
                })
                .collect();
        }
        layout::at_breakpoint(&slots, self.columns, bp)
    }
}

/// G03: per-session undo for gallery edits — whole-model snapshots (a
/// gallery is small), coalesced for continuous gestures, capped.
#[derive(Default)]
pub struct History {
    undo: Vec<(Gallery, String)>,
    redo: Vec<(Gallery, String)>,
    /// Key of the last recorded command, for coalescing (e.g. one entry per
    /// drag or per slider gesture).
    last_key: Option<String>,
}

pub const HISTORY_CAP: usize = 200;

impl History {
    /// Record the state *before* a change. A `coalesce` key equal to the
    /// previous one merges into that entry.
    pub fn record(&mut self, before: &Gallery, label: &str, coalesce: Option<&str>) {
        if coalesce.is_some() && coalesce.map(str::to_string) == self.last_key {
            return;
        }
        self.undo.push((before.clone(), label.to_string()));
        if self.undo.len() > HISTORY_CAP {
            self.undo.remove(0);
        }
        self.redo.clear();
        self.last_key = coalesce.map(str::to_string);
    }

    /// End a coalescing gesture so the next change starts a new entry.
    pub fn seal(&mut self) {
        self.last_key = None;
    }

    pub fn undo(&mut self, current: &Gallery) -> Option<(Gallery, String)> {
        let (prev, label) = self.undo.pop()?;
        self.redo.push((current.clone(), label.clone()));
        self.last_key = None;
        Some((prev, label))
    }

    pub fn redo(&mut self, current: &Gallery) -> Option<(Gallery, String)> {
        let (next, label) = self.redo.pop()?;
        self.undo.push((current.clone(), label.clone()));
        self.last_key = None;
        Some((next, label))
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn last_label(&self) -> Option<&str> {
        self.undo.last().map(|(_, l)| l.as_str())
    }

    pub fn clear(&mut self) {
        self.undo.clear();
        self.redo.clear();
        self.last_key = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gallery(n: i64) -> Gallery {
        let mut g = Gallery::default();
        g.add_photos(&(1..=n).collect::<Vec<_>>());
        g
    }

    #[test]
    fn apply_keeps_order_captions_alt_and_focal() {
        let mut g = gallery(12);
        g.photos[3].caption = "Harbor <dawn> & gulls".into();
        g.photos[3].alt_text = "Boats at sunrise".into();
        g.photos[3].focal = (0.2, 0.8);
        g.photos[3].fit = Fit::Fit;
        for t in layout::TEMPLATES {
            g.apply_template(t.id);
            assert_eq!(g.placed_count(), 12, "{}", t.id);
            let ids: Vec<i64> = g.photos.iter().map(|p| p.photo_id).collect();
            assert_eq!(ids, (1..=12).collect::<Vec<_>>());
            assert_eq!(g.photos[3].caption, "Harbor <dawn> & gulls");
            assert_eq!(g.photos[3].alt_text, "Boats at sunrise");
            assert_eq!((g.photos[3].focal, g.photos[3].fit), ((0.2, 0.8), Fit::Fit));
            // Reading order follows tray order.
            let order: Vec<usize> = g.layout_at(Breakpoint::Desktop).iter().map(|r| r.0).collect();
            assert_eq!(order, (0..12).collect::<Vec<_>>(), "{}", t.id);
        }
    }

    #[test]
    fn placing_moving_and_unplacing() {
        let mut g = gallery(5);
        g.apply_template("square");
        g.unplace(3);
        assert_eq!(g.placed_count(), 4);
        g.place_next(3);
        assert_eq!(g.placed_count(), 5);
        g.place(5, Cell { col: 0, row: 0 });
        assert_eq!(g.photos[4].cell, Some(Cell { col: 0, row: 0 }));
        let placed: Vec<Cell> = g.photos.iter().filter_map(|p| p.cell).collect();
        let unique: std::collections::HashSet<Cell> = placed.iter().copied().collect();
        assert_eq!(unique.len(), 5);
        g.set_span(5, 3, 1);
        assert_eq!((g.photos[4].span_x, g.photos[4].span_y), (3, 1));
        g.set_columns(2);
        assert!(g.photos.iter().all(|p| p.span_x <= 2 && p.cell.unwrap().col as u8 + p.span_x <= 2));
        // Toggling breakpoints never mutates the model.
        let before = g.clone();
        let _ = g.layout_at(Breakpoint::Phone);
        let _ = g.layout_at(Breakpoint::Tablet);
        assert_eq!(g, before);
        assert_eq!(g.remove_photos(&[1, 99]), 1);
        assert_eq!(g.add_photos(&[2, 6]), 1);
    }

    #[test]
    fn history_round_trips_and_coalesces() {
        let mut g = gallery(6);
        g.apply_template("mixed");
        let mut h = History::default();
        let mut states = vec![g.clone()];
        for i in 0..50 {
            h.record(&g, "edit", None);
            match i % 3 {
                0 => g.set_span(1 + (i % 6) as i64, 1 + (i % 2) as u8, 1),
                1 => g.photos[(i % 6) as usize].caption = format!("c{i}"),
                _ => g.place((i % 6) as i64 + 1, Cell { col: (i % 3) as u16, row: (i % 4) as u16 }),
            }
            states.push(g.clone());
        }
        for k in (0..50).rev() {
            let (prev, _) = h.undo(&g).unwrap();
            g = prev;
            assert_eq!(g, states[k]);
        }
        assert!(h.undo(&g).is_none());
        for k in 1..=50 {
            let (next, _) = h.redo(&g).unwrap();
            g = next;
            assert_eq!(g, states[k]);
        }
        // One entry per drag.
        let mut h = History::default();
        for _ in 0..10 {
            h.record(&g, "Move", Some("drag-1"));
        }
        h.seal();
        h.record(&g, "Caption", None);
        assert_eq!(h.undo.len(), 2);
    }

    #[test]
    fn slugs_and_contrast() {
        assert!(validate_slug("hokkaido-2026").is_ok());
        assert!(validate_slug("").is_err());
        assert!(validate_slug("Bad Slug").is_err());
        assert!(validate_slug("-x").is_err());
        let t = Theme::default();
        assert!(Theme::contrast(t.accent, t.page) < 4.5);
        let text = t.accent_text();
        assert!(Theme::contrast(text, t.page) >= 4.5, "{text:06X}");
        // Near the handoff's #9B93E8.
        let (r, g, b) = ((text >> 16) & 0xFF, (text >> 8) & 0xFF, text & 0xFF);
        assert!(r.abs_diff(0x9B) < 24 && g.abs_diff(0x93) < 24 && b.abs_diff(0xE8) < 24, "{text:06X}");
        // Light pages darken instead.
        let light = Theme { page: 0xFFFDF8, accent: 0xE8E0FF, ..Theme::default() };
        assert!(Theme::contrast(light.accent_text(), light.page) >= 4.5);
        let json = serde_json::to_string(&t).unwrap();
        assert_eq!(serde_json::from_str::<Theme>(&json).unwrap(), t);
        assert_eq!(serde_json::from_str::<Theme>("{}").unwrap(), t);
    }
}
