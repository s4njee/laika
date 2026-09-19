//! Map & metadata (design 1d): find photos by place and date, inspect
//! EXIF and storage, and place photos that have no location.
//!
//! The basemap is local by default: a latitude/longitude graticule drawn
//! in the app, so nothing leaves the Mac. "Street map" opts in to Esri's
//! Dark Gray Canvas tiles, fetched on demand and cached on disk.

use std::cell::{Cell as StdCell, RefCell};
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;

use laika_core::geo::{self, View};

use super::gallery_ui::{in_bounds, meter};
use super::*;

/// A located photo, parsed once per catalog change.
#[derive(Clone, Debug)]
pub(crate) struct Located {
    pub id: i64,
    pub lat: f64,
    pub lon: f64,
    /// Days since 1970-01-01 from the capture date (None when unknown).
    pub day: Option<i64>,
    pub camera: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum StripMode {
    /// Located photos matching the filters.
    #[default]
    Located,
    /// Photos with no location (to place on the map).
    Unlocated,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum MapGesture {
    /// Background press: pans once it moves; a still press is a click.
    Press {
        start: (f32, f32),
        last: (f32, f32),
        moved: bool,
    },
    /// ⇧-drag: select photos inside a rectangle.
    Select { start: (f32, f32), cur: (f32, f32) },
    /// Date range handle (0 = from, 1 = to).
    Date(u8),
}

type TileKey = (u8, u32, u32);

pub(crate) struct MapUi {
    pub view: View,
    pub fitted: bool,
    pub gesture: Option<MapGesture>,
    /// Cluster / rectangle / place filter over located photos.
    pub filter: Option<(String, HashSet<i64>)>,
    /// Date window as fractions of the located photos' date span.
    pub date: (f32, f32),
    pub camera: Option<String>,
    pub strip: StripMode,
    /// Next click on the map places the selected photos.
    pub placing: bool,
    pub map_box: Rc<StdCell<Bounds<Pixels>>>,
    pub date_box: Rc<StdCell<Bounds<Pixels>>>,
    located: RefCell<(u64, Rc<Vec<Located>>)>,
    tiles: RefCell<HashMap<TileKey, Arc<RenderImage>>>,
    tile_order: RefCell<VecDeque<TileKey>>,
    tile_pending: RefCell<HashSet<TileKey>>,
    tile_failed: RefCell<HashSet<TileKey>>,
    pub restored: bool,
}

impl Default for MapUi {
    fn default() -> Self {
        let zero = || {
            Rc::new(StdCell::new(Bounds {
                origin: point(px(0.), px(0.)),
                size: size(px(0.), px(0.)),
            }))
        };
        Self {
            view: View {
                lat: 20.,
                lon: 0.,
                zoom: 2.,
                width: 900.,
                height: 600.,
            },
            fitted: false,
            gesture: None,
            filter: None,
            date: (0., 1.),
            camera: None,
            strip: StripMode::Located,
            placing: false,
            map_box: zero(),
            date_box: zero(),
            located: RefCell::new((u64::MAX, Rc::new(Vec::new()))),
            tiles: RefCell::new(HashMap::new()),
            tile_order: RefCell::new(VecDeque::new()),
            tile_pending: RefCell::new(HashSet::new()),
            tile_failed: RefCell::new(HashSet::new()),
            restored: false,
        }
    }
}

/// In-memory tile cap (256px RGBA ≈ 260 KB each).
const TILE_CAP: usize = 160;
const TILE_CONCURRENCY: usize = 6;

/// Days since the epoch from `YYYY-MM-DD…` / `YYYY:MM:DD…`.
fn day_of(captured: &str) -> Option<i64> {
    let y: i64 = captured.get(0..4)?.parse().ok()?;
    let m: i64 = captured.get(5..7)?.parse().ok()?;
    let d: i64 = captured.get(8..10)?.parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

fn date_label(day: i64) -> String {
    let z = day + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// Esri Dark Gray Canvas: a basemap and a transparent label layer that
/// are fetched separately and composited (no API key required).
fn tile_url(layer: &str, z: u8, x: u32, y: u32) -> String {
    format!(
        "https://server.arcgisonline.com/ArcGIS/rest/services/Canvas/{layer}/MapServer/tile/{z}/{y}/{x}"
    )
}

const TILE_LAYERS: [(&str, &str); 2] = [
    ("World_Dark_Gray_Base", "jpg"),
    ("World_Dark_Gray_Reference", "png"),
];

/// Read a cached tile, or download it into the cache.
fn fetch_tile(
    dir: &std::path::Path,
    layer: &str,
    ext: &str,
    z: u8,
    x: u32,
    y: u32,
) -> Option<Vec<u8>> {
    let path = dir
        .join(layer)
        .join(z.to_string())
        .join(x.to_string())
        .join(format!("{y}.{ext}"));
    if let Ok(b) = std::fs::read(&path) {
        return Some(b);
    }
    std::fs::create_dir_all(path.parent()?).ok()?;
    let tmp = path.with_extension("part");
    let curl = if cfg!(target_os = "macos") {
        "/usr/bin/curl"
    } else {
        "curl"
    };
    let ok = std::process::Command::new(curl)
        .args(["-sSfL", "--max-time", "12", "-A"])
        .arg(format!(
            "Laika/{} (+https://github.com/s4njee/laika)",
            env!("CARGO_PKG_VERSION")
        ))
        .arg("-o")
        .arg(&tmp)
        .arg(tile_url(layer, z, x, y))
        .status()
        .is_ok_and(|s| s.success());
    if !ok {
        std::fs::remove_file(&tmp).ok();
        return None;
    }
    std::fs::rename(&tmp, &path).ok()?;
    std::fs::read(&path).ok()
}

/// Base tile with labels drawn over it, as BGRA pixels.
fn load_tile(dir: &std::path::Path, z: u8, x: u32, y: u32) -> Option<image::RgbaImage> {
    let (base_layer, base_ext) = TILE_LAYERS[0];
    let base = fetch_tile(dir, base_layer, base_ext, z, x, y)?;
    let mut rgba = image::load_from_memory(&base).ok()?.to_rgba8();
    let (label_layer, label_ext) = TILE_LAYERS[1];
    // Labels are optional: a missing label tile still shows the basemap.
    if let Some(labels) = fetch_tile(dir, label_layer, label_ext, z, x, y)
        .and_then(|b| image::load_from_memory(&b).ok())
    {
        image::imageops::overlay(&mut rgba, &labels.to_rgba8(), 0, 0);
    }
    let (w, h) = (rgba.width(), rgba.height());
    let mut pixels = rgba.into_raw();
    rgba_to_bgra_in_place(&mut pixels);
    image::RgbaImage::from_raw(w, h, pixels)
}

fn mono(text: impl Into<SharedString>, size: f32, color: u32) -> Div {
    div()
        .font_family(crate::gallery_ui::PLEX_MONO)
        .text_size(sp(size))
        .text_color(rgb(color))
        .child(text.into())
}

fn rail_heading(text: &str) -> Div {
    div()
        .px(px(14.))
        .pt(px(14.))
        .pb(px(6.))
        .font_family(crate::gallery_ui::PLEX_MONO)
        .font_weight(FontWeight::MEDIUM)
        .text_size(sp(9.5))
        .text_color(rgb(TEXT_DIM))
        .child(text.to_uppercase())
}

impl Laika {
    // ---- data --------------------------------------------------------------------

    /// Located photos (cached per catalog revision).
    pub(crate) fn map_located(&self) -> Rc<Vec<Located>> {
        let rev = self.photos_rev.get() ^ ((self.photos.len() as u64) << 32);
        {
            let cache = self.map.located.borrow();
            if cache.0 == rev {
                return cache.1.clone();
            }
        }
        let list: Vec<Located> = self
            .photos
            .iter()
            .filter_map(|p| {
                let (lat, lon) = geo::parse_gps(&p.exif_gps)?;
                Some(Located {
                    id: p.id,
                    lat,
                    lon,
                    day: day_of(&p.captured_at),
                    camera: p.camera.trim_matches('"').to_string(),
                })
            })
            .collect();
        let rc = Rc::new(list);
        *self.map.located.borrow_mut() = (rev, rc.clone());
        rc
    }

    fn map_day_span(located: &[Located]) -> Option<(i64, i64)> {
        let days = located.iter().filter_map(|l| l.day);
        let min = days.clone().min()?;
        let max = days.max()?;
        Some((min, max))
    }

    /// Located photos passing the date and camera facets (not the
    /// cluster/rectangle filter — clusters are drawn from these).
    fn map_faceted(&self) -> Vec<Located> {
        let located = self.map_located();
        let span = Self::map_day_span(&located);
        let (f0, f1) = self.map.date;
        let window = span.map(|(a, b)| {
            let len = (b - a) as f32;
            (a + (len * f0).floor() as i64, a + (len * f1).ceil() as i64)
        });
        located
            .iter()
            .filter(|l| match (window, l.day) {
                (Some((a, b)), Some(d)) => d >= a && d <= b,
                (Some(_), None) => f0 <= 0.0 && f1 >= 1.0,
                _ => true,
            })
            .filter(|l| self.map.camera.as_ref().is_none_or(|c| &l.camera == c))
            .cloned()
            .collect()
    }

    /// The filmstrip in the Map module.
    pub(crate) fn map_strip_photos(&self) -> Vec<&DbPhoto> {
        match self.map.strip {
            StripMode::Located => {
                let ids: HashSet<i64> = self
                    .map_faceted()
                    .into_iter()
                    .map(|l| l.id)
                    .filter(|id| self.map.filter.as_ref().is_none_or(|(_, f)| f.contains(id)))
                    .collect();
                let mut v: Vec<&DbPhoto> =
                    self.photos.iter().filter(|p| ids.contains(&p.id)).collect();
                v.sort_by(|a, b| a.captured_at.cmp(&b.captured_at).then(a.id.cmp(&b.id)));
                v
            }
            StripMode::Unlocated => {
                let mut v: Vec<&DbPhoto> = self
                    .photos
                    .iter()
                    .filter(|p| geo::parse_gps(&p.exif_gps).is_none())
                    .collect();
                v.sort_by(|a, b| a.captured_at.cmp(&b.captured_at).then(a.id.cmp(&b.id)));
                v
            }
        }
    }

    fn map_view_now(&self) -> View {
        let b = self.map.map_box.get();
        let mut v = self.map.view;
        if b.size.width.as_f32() > 1. {
            v.width = b.size.width.as_f32() as f64;
            v.height = b.size.height.as_f32() as f64;
        }
        v
    }

    /// Frame every located photo (or the filter) on first open.
    fn map_fit(&mut self, ids: Option<&HashSet<i64>>) {
        let v = self.map_view_now();
        let pts: Vec<(f64, f64)> = self
            .map_faceted()
            .iter()
            .filter(|l| ids.is_none_or(|f| f.contains(&l.id)))
            .map(|l| (l.lat, l.lon))
            .collect();
        if let Some((lat, lon, zoom)) = geo::fit(&pts, v.width, v.height, 60.) {
            self.map.view = View {
                lat,
                lon,
                zoom,
                ..v
            };
        }
        self.map.fitted = true;
    }

    fn map_save_view(&self) {
        if let Some(cat) = self.catalog.as_ref() {
            let v = self.map.view;
            cat.set_import_default(
                "map_view",
                &format!("{:.6},{:.6},{:.3}", v.lat, v.lon, v.zoom),
            );
        }
    }

    pub(crate) fn open_map(&mut self, cx: &mut Context<Self>) {
        self.flush_saves();
        self.state.active_module = Module::Map;
        self.publish_open = false;
        if !self.map.restored {
            self.map.restored = true;
            let saved = self
                .catalog
                .as_ref()
                .map(|c| c.get_import_default("map_view"))
                .unwrap_or_default();
            let parts: Vec<f64> = saved.split(',').filter_map(|p| p.parse().ok()).collect();
            if let [lat, lon, zoom] = parts.as_slice() {
                self.map.view.lat = *lat;
                self.map.view.lon = *lon;
                self.map.view.zoom = zoom.clamp(geo::MIN_ZOOM, geo::MAX_ZOOM);
                self.map.fitted = true;
            }
        }
        cx.notify();
    }

    // ---- actions -----------------------------------------------------------------

    fn map_zoom_by(&mut self, delta: f64, at: Option<(f64, f64)>, cx: &mut Context<Self>) {
        let mut v = self.map_view_now();
        let (sx, sy) = at.unwrap_or((v.width / 2., v.height / 2.));
        v.zoom_at(v.zoom + delta, sx, sy);
        self.map.view = v;
        self.map_save_view();
        cx.notify();
    }

    fn map_set_filter(&mut self, label: String, ids: HashSet<i64>, cx: &mut Context<Self>) {
        self.map.strip = StripMode::Located;
        if let Some(first) = self.map_strip_photos_for(&ids).first().copied() {
            self.map.filter = Some((label, ids));
            self.select_navigate(first, SelectMode::Set, cx);
        } else {
            self.map.filter = Some((label, ids));
        }
        cx.notify();
    }

    fn map_strip_photos_for(&self, ids: &HashSet<i64>) -> Vec<i64> {
        let mut v: Vec<&DbPhoto> = self.photos.iter().filter(|p| ids.contains(&p.id)).collect();
        v.sort_by(|a, b| a.captured_at.cmp(&b.captured_at).then(a.id.cmp(&b.id)));
        v.into_iter().map(|p| p.id).collect()
    }

    /// Place the target photos (selection, else primary) at a coordinate.
    fn map_place_targets(&mut self, lat: f64, lon: f64, cx: &mut Context<Self>) {
        let ids = self.targets();
        if ids.is_empty() {
            self.status_note = "select photos to place first".to_string();
            cx.notify();
            return;
        }
        let Some(cat) = self.catalog.as_ref() else {
            return;
        };
        let value = geo::format_gps(lat, lon);
        let mut placed = 0;
        for id in &ids {
            match cat.set_gps(*id, Some((lat, lon))) {
                Ok(()) => placed += 1,
                Err(e) => self.status_note = e,
            }
        }
        for id in &ids {
            if let Some(&i) = self.photo_index.get(id) {
                if let Some(p) = self.photos.get_mut(i).filter(|p| p.id == *id) {
                    p.exif_gps = value.clone();
                }
            }
            self.request_sidecar(*id, false);
        }
        self.photos_rev.set(self.photos_rev.get().wrapping_add(1));
        self.map.placing = false;
        self.status_note = format!(
            "placed {placed} photo{} at {}",
            if placed == 1 { "" } else { "s" },
            geo::display_gps(lat, lon)
        );
        cx.notify();
    }

    fn map_clear_location(&mut self, cx: &mut Context<Self>) {
        let ids = self.targets();
        let Some(cat) = self.catalog.as_ref() else {
            return;
        };
        let cleared: Vec<i64> = ids
            .iter()
            .copied()
            .filter(|id| cat.set_gps(*id, None).is_ok())
            .collect();
        for id in &cleared {
            {
                if let Some(&i) = self.photo_index.get(id) {
                    if let Some(p) = self.photos.get_mut(i).filter(|p| p.id == *id) {
                        p.exif_gps.clear();
                    }
                }
                self.request_sidecar(*id, false);
            }
        }
        self.photos_rev.set(self.photos_rev.get().wrapping_add(1));
        self.status_note = format!(
            "removed the location from {} photo{} (the original file still holds its own GPS, if any)",
            ids.len(),
            if ids.len() == 1 { "" } else { "s" }
        );
        cx.notify();
    }

    /// Map keys. True when consumed.
    pub(crate) fn map_key(&mut self, key: &str, cx: &mut Context<Self>) -> bool {
        if self.state.active_module != Module::Map {
            return false;
        }
        match key {
            "=" | "+" => self.map_zoom_by(1., None, cx),
            "-" => self.map_zoom_by(-1., None, cx),
            "escape" => {
                if self.map.placing {
                    self.map.placing = false;
                } else if self.map.filter.take().is_none() {
                    return false;
                }
                cx.notify();
            }
            _ => return false,
        }
        true
    }

    // ---- pointer (window-level) ------------------------------------------------------

    pub(crate) fn map_mouse_move(
        &mut self,
        pos: (f32, f32),
        pressed: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.state.active_module != Module::Map {
            return false;
        }
        let Some(g) = self.map.gesture else {
            return false;
        };
        if !pressed {
            self.map_mouse_up(pos, cx);
            return true;
        }
        match g {
            MapGesture::Press { start, last, moved } => {
                let moved = moved || (start.0 - pos.0).abs() + (start.1 - pos.1).abs() > 3.;
                if moved {
                    let mut v = self.map_view_now();
                    v.pan((pos.0 - last.0) as f64, (pos.1 - last.1) as f64);
                    self.map.view = v;
                }
                self.map.gesture = Some(MapGesture::Press {
                    start,
                    last: pos,
                    moved,
                });
            }
            MapGesture::Select { start, .. } => {
                self.map.gesture = Some(MapGesture::Select { start, cur: pos });
            }
            MapGesture::Date(h) => {
                let b = self.map.date_box.get();
                let w = b.size.width.as_f32();
                if w > 1. {
                    let f = ((pos.0 - b.origin.x.as_f32()) / w).clamp(0., 1.);
                    if h == 0 {
                        self.map.date.0 = f.min(self.map.date.1);
                    } else {
                        self.map.date.1 = f.max(self.map.date.0);
                    }
                }
            }
        }
        cx.notify();
        true
    }

    pub(crate) fn map_mouse_up(&mut self, pos: (f32, f32), cx: &mut Context<Self>) -> bool {
        let Some(g) = self.map.gesture.take() else {
            return false;
        };
        let b = self.map.map_box.get();
        let local = |p: (f32, f32)| {
            (
                (p.0 - b.origin.x.as_f32()) as f64,
                (p.1 - b.origin.y.as_f32()) as f64,
            )
        };
        match g {
            MapGesture::Press { moved: true, .. } => self.map_save_view(),
            MapGesture::Press { moved: false, .. } => {
                if self.map.placing && in_bounds(b, pos) {
                    let (sx, sy) = local(pos);
                    let (lat, lon) = self.map_view_now().to_coord(sx, sy);
                    self.map_place_targets(lat, lon, cx);
                }
            }
            MapGesture::Select { start, cur } => {
                let v = self.map_view_now();
                let (a, c) = (local(start), local(cur));
                let (x0, x1) = (a.0.min(c.0), a.0.max(c.0));
                let (y0, y1) = (a.1.min(c.1), a.1.max(c.1));
                let ids: HashSet<i64> = self
                    .map_faceted()
                    .iter()
                    .filter(|l| {
                        let (x, y) = v.to_screen(l.lat, l.lon);
                        x >= x0 && x <= x1 && y >= y0 && y <= y1
                    })
                    .map(|l| l.id)
                    .collect();
                if ids.is_empty() {
                    self.status_note = "no located photos inside that area".to_string();
                } else {
                    let n = ids.len();
                    self.map_set_filter(format!("{n} in selected area"), ids, cx);
                }
            }
            MapGesture::Date(_) => {}
        }
        cx.notify();
        true
    }

    // ---- tiles ------------------------------------------------------------------------

    fn tile_image(&self, key: TileKey) -> Option<Arc<RenderImage>> {
        self.map.tiles.borrow().get(&key).cloned()
    }

    /// Queue visible tiles that are neither loaded nor in flight.
    fn kick_tiles(&self, wanted: &[TileKey], cx: &mut Context<Self>) {
        let dir = self.cache_dir.join("map-tiles").join("esri");
        let mut queued = Vec::new();
        {
            let tiles = self.map.tiles.borrow();
            let mut pending = self.map.tile_pending.borrow_mut();
            let failed = self.map.tile_failed.borrow();
            for key in wanted {
                if pending.len() >= TILE_CONCURRENCY {
                    break;
                }
                if tiles.contains_key(key) || pending.contains(key) || failed.contains(key) {
                    continue;
                }
                pending.insert(*key);
                queued.push(*key);
            }
        }
        for key in queued {
            let (z, x, y) = key;
            let dir = dir.clone();
            cx.spawn(async move |entity, cx| {
                let pixels = cx
                    .background_spawn(async move { load_tile(&dir, z, x, y) })
                    .await;
                let image = pixels.map(|buf| {
                    Arc::new(RenderImage::new(smallvec::smallvec![image::Frame::new(
                        buf
                    )]))
                });
                entity
                    .update(cx, |this, cx| {
                        this.map.tile_pending.borrow_mut().remove(&key);
                        match image {
                            Some(img) => {
                                let mut tiles = this.map.tiles.borrow_mut();
                                let mut order = this.map.tile_order.borrow_mut();
                                tiles.insert(key, img);
                                order.push_back(key);
                                while order.len() > TILE_CAP {
                                    if let Some(old) = order.pop_front() {
                                        if let Some(img) = tiles.remove(&old) {
                                            this.stale.push(img);
                                        }
                                    }
                                }
                            }
                            None => {
                                this.map.tile_failed.borrow_mut().insert(key);
                                eprintln!("[map] tile {z}/{x}/{y} unavailable (offline?)");
                            }
                        }
                        cx.notify();
                    })
                    .ok();
            })
            .detach();
        }
    }

    // ---- view -----------------------------------------------------------------------

    pub(crate) fn map_module(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        self.ensure_gallery_fonts(cx);
        if self.catalog.is_none() {
            return div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .bg(rgb(bg_canvas()))
                .text_size(sp(12.))
                .text_color(rgb(TEXT_DIM))
                .child("Open a catalog to see photos on the map");
        }
        if !self.map.fitted && self.map.map_box.get().size.width.as_f32() > 1. {
            self.map_fit(None);
        }
        let mut row = div().flex_1().min_h_0().flex();
        if self.left_rail_shown() {
            row = row
                .child(self.map_left(window, cx))
                .child(self.rail_resize_handle(RailSide::Left, cx));
        }
        row = row.child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(self.map_toolbar(cx))
                .child(self.map_canvas(window, cx))
                .children(self.filmstrip_strip(cx)),
        );
        if self.right_rail_shown() {
            row = row
                .child(self.rail_resize_handle(RailSide::Right, cx))
                .child(self.map_right(cx));
        }
        row
    }

    fn map_toolbar(&self, cx: &mut Context<Self>) -> Div {
        let located = self.map_located();
        let total = self.photos.len();
        let chip = |id: &'static str, label: String, on: bool| {
            div()
                .id(id)
                .px(px(9.))
                .py(px(4.))
                .rounded(px(3.))
                .border_1()
                .border_color::<Hsla>(if on {
                    rgb(accent_line()).into()
                } else {
                    border_control()
                })
                .text_size(sp(10.5))
                .text_color(rgb(if on { accent_line() } else { TEXT_SECONDARY }))
                .hover(|s| s.border_color(border_strong()))
                .child(label)
        };
        let tiles_on = self.library.prefs.map_tiles;
        div()
            .h(px(layout::TOOLBAR))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(8.))
            .px(px(14.))
            .bg(rgb(bg_chrome()))
            .border_b_1()
            .border_color(hairline())
            .child(
                div()
                    .text_size(sp(11.))
                    .text_color(rgb(TEXT_SECONDARY))
                    .child(format!("{} of {} geotagged", located.len(), total)),
            )
            .when_some(self.map.filter.as_ref(), |d, (label, _)| {
                d.child(
                    chip("map-filter-clear", format!("{label}  ✕"), true)
                        .on_hover(self.tip("Show every located photo again (Esc)"))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.map.filter = None;
                            cx.notify();
                        })),
                )
            })
            .child(div().flex_1())
            .child(
                chip("map-strip-located", "Located".to_string(), self.map.strip == StripMode::Located)
                    .on_hover(self.tip("Filmstrip: located photos matching the filters"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.map.strip = StripMode::Located;
                        this.map.placing = false;
                        cx.notify();
                    })),
            )
            .child(
                chip(
                    "map-strip-unlocated",
                    format!("No location {}", total.saturating_sub(located.len())),
                    self.map.strip == StripMode::Unlocated,
                )
                .on_hover(self.tip("Filmstrip: photos without a location, ready to place"))
                .on_click(cx.listener(|this, _, _, cx| {
                    this.map.strip = StripMode::Unlocated;
                    cx.notify();
                })),
            )
            .child(div().w(px(1.)).h(px(18.)).bg(hairline()))
            .child(
                chip("map-fit", "Fit".to_string(), false)
                    .on_hover(self.tip("Frame every located photo"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        let ids = this.map.filter.as_ref().map(|(_, f)| f.clone());
                        this.map_fit(ids.as_ref());
                        this.map_save_view();
                        cx.notify();
                    })),
            )
            .child(
                chip("map-tiles", format!("Street map {}", if tiles_on { "on" } else { "off" }), tiles_on)
                    .on_hover(self.tip(
                        "Loads map tiles from Esri (OpenStreetMap and partner data) over the internet; off keeps the map fully local",
                    ))
                    .on_click(cx.listener(|this, _, _, cx| {
                        let on = !this.library.prefs.map_tiles;
                        this.map.tile_failed.borrow_mut().clear();
                        this.set_prefs(move |p| p.map_tiles = on, cx);
                    })),
            )
    }

    fn map_canvas(&self, window: &mut Window, cx: &mut Context<Self>) -> Stateful<Div> {
        let v = self.map_view_now();
        let (w, h) = (v.width, v.height);
        let tiles_on = self.library.prefs.map_tiles;
        let mut map = div()
            .id("map-canvas")
            .relative()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .bg(rgb(0x101413))
            .when(self.map.placing, |d| d.cursor(CursorStyle::Crosshair))
            .child(meter(self.map.map_box.clone()))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, ev: &MouseDownEvent, _, cx| {
                    let pos = (ev.position.x.as_f32(), ev.position.y.as_f32());
                    this.map.gesture = Some(if ev.modifiers.shift {
                        MapGesture::Select {
                            start: pos,
                            cur: pos,
                        }
                    } else {
                        MapGesture::Press {
                            start: pos,
                            last: pos,
                            moved: false,
                        }
                    });
                    cx.notify();
                }),
            )
            .on_scroll_wheel(cx.listener(|this, ev: &ScrollWheelEvent, window, cx| {
                let b = this.map.map_box.get();
                let at = (
                    (ev.position.x.as_f32() - b.origin.x.as_f32()) as f64,
                    (ev.position.y.as_f32() - b.origin.y.as_f32()) as f64,
                );
                let dy = ev.delta.pixel_delta(window.line_height()).y.as_f32() as f64;
                if dy.abs() > 0.01 {
                    this.map_zoom_by(dy / 240., Some(at), cx);
                }
            }));

        // Basemap: tiles (opt-in) over a graticule that is always drawn.
        if tiles_on {
            let tiles = v.tiles(if window.scale_factor() > 1.5 { 1. } else { 0. });
            let wanted: Vec<TileKey> = tiles.iter().map(|t| (t.0, t.1, t.2)).collect();
            self.kick_tiles(&wanted, cx);
            for (z, x, y, left, top, size) in tiles {
                if let Some(tile) = self.tile_image((z, x, y)) {
                    map = map.child(
                        div()
                            .absolute()
                            .left(px(left as f32))
                            .top(px(top as f32))
                            .w(px(size as f32 + 0.5))
                            .h(px(size as f32 + 0.5))
                            .child(img(ImageSource::Render(tile)).size_full()),
                    );
                }
            }
        }
        map = map.children(self.graticule(&v, tiles_on));

        // Clusters.
        let faceted = self.map_faceted();
        let pts: Vec<(i64, f64, f64)> = faceted.iter().map(|l| (l.id, l.lat, l.lon)).collect();
        let clusters = geo::cluster(&pts, &v, 56.);
        let filter = self.map.filter.as_ref().map(|(_, f)| f);
        let primary = self.state.primary;
        for (ci, c) in clusters.into_iter().enumerate().take(600) {
            let n = c.ids.len();
            let dim = filter.is_some_and(|f| !c.ids.iter().any(|id| f.contains(id)));
            let has_primary = primary.is_some_and(|p| c.ids.contains(&p));
            let ids: HashSet<i64> = c.ids.iter().copied().collect();
            let (cx0, cy0, lat, lon, zoom) = (c.x as f32, c.y as f32, c.lat, c.lon, v.zoom);
            let el = if n == 1 {
                let pid = c.ids[0];
                self.thumb_seen.borrow_mut().push(pid);
                let thumb = self.thumbs.get(&pid).map(|t| t.image.clone());
                let d = 34.;
                div()
                    .id(("map-photo", ci))
                    .absolute()
                    .left(px(cx0 - d / 2.))
                    .top(px(cy0 - d / 2.))
                    .size(px(d))
                    .rounded_full()
                    .border_2()
                    .border_color(rgb(if has_primary { 0xF2EFE6 } else { accent_line() }))
                    .map(|el| match thumb {
                        Some(im) => el.bg(rgb(0x1A1F1C)).child(
                            img(ImageSource::Render(im))
                                .size_full()
                                .rounded_full()
                                .object_fit(ObjectFit::Cover),
                        ),
                        None => {
                            let (a, b) = self
                                .find(pid)
                                .map(|p| placeholder_tint(&p.blake3))
                                .unwrap_or((0x2A2F2C, 0x1A1F1C));
                            el.bg(linear_gradient(
                                160.,
                                linear_color_stop(rgb(a), 0.),
                                linear_color_stop(rgb(b), 1.),
                            ))
                        }
                    })
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.select_navigate(pid, SelectMode::Set, cx);
                        cx.notify();
                    }))
            } else {
                let d = (24. + (n as f32).log2() * 5.).clamp(24., 52.);
                let mut wash: Hsla = rgb(accent_line()).into();
                wash.a = 0.18;
                div()
                    .id(("map-cluster", ci))
                    .absolute()
                    .left(px(cx0 - d / 2.))
                    .top(px(cy0 - d / 2.))
                    .size(px(d))
                    .rounded_full()
                    .bg(wash)
                    .border_1()
                    .border_color(rgb(if has_primary { 0xF2EFE6 } else { accent_line() }))
                    .flex()
                    .items_center()
                    .justify_center()
                    .font_family(crate::gallery_ui::PLEX_MONO)
                    .font_weight(FontWeight::MEDIUM)
                    .text_size(sp(10.))
                    .text_color(rgb(accent_line()))
                    .child(if n >= 10_000 {
                        format!("{}k", n / 1000)
                    } else {
                        n.to_string()
                    })
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        // Zoom into the cluster and filter the filmstrip.
                        let pts: Vec<(f64, f64)> = this
                            .map_located()
                            .iter()
                            .filter(|l| ids.contains(&l.id))
                            .map(|l| (l.lat, l.lon))
                            .collect();
                        let v = this.map_view_now();
                        match geo::fit(&pts, v.width, v.height, 80.) {
                            Some((la, lo, z)) => {
                                this.map.view = View {
                                    lat: la,
                                    lon: lo,
                                    zoom: z.max(zoom + 1.).min(geo::MAX_ZOOM),
                                    ..v
                                };
                            }
                            None => {
                                this.map.view = View {
                                    lat,
                                    lon,
                                    zoom: (zoom + 2.).min(geo::MAX_ZOOM),
                                    ..v
                                }
                            }
                        }
                        this.map_save_view();
                        let n = ids.len();
                        this.map_set_filter(format!("{n} photos here"), ids.clone(), cx);
                    }))
            };
            map = map.child(el.when(dim, |d| d.opacity(0.35)));
        }

        // Rectangle selection.
        if let Some(MapGesture::Select { start, cur }) = self.map.gesture {
            let b = self.map.map_box.get();
            let (ox, oy) = (b.origin.x.as_f32(), b.origin.y.as_f32());
            let (x0, x1) = (start.0.min(cur.0) - ox, start.0.max(cur.0) - ox);
            let (y0, y1) = (start.1.min(cur.1) - oy, start.1.max(cur.1) - oy);
            let mut wash: Hsla = rgb(accent_line()).into();
            wash.a = 0.1;
            map = map.child(
                div()
                    .absolute()
                    .left(px(x0))
                    .top(px(y0))
                    .w(px(x1 - x0))
                    .h(px(y1 - y0))
                    .bg(wash)
                    .border_1()
                    .border_color(rgb(accent_line())),
            );
        }

        // Overlays: mode chip, zoom stack, attribution.
        let chip_text = if self.map.placing {
            "PLACING — click the map to set the location · Esc cancels".to_string()
        } else if tiles_on {
            format!("STREET MAP · ZOOM {:.1}", v.zoom)
        } else {
            format!("LOCAL MAP · ZOOM {:.1} · ⇧-drag selects", v.zoom)
        };
        let control = |id: &'static str, label: &'static str| {
            div()
                .id(id)
                .size(px(26.))
                .flex()
                .items_center()
                .justify_center()
                .bg(rgba(0x0F0E0DE6))
                .border_1()
                .border_color(border_control())
                .text_size(sp(14.))
                .text_color(rgb(TEXT_SECONDARY))
                .hover(|s| s.text_color(rgb(TEXT_PRIMARY)))
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(label)
        };
        map = map
            .child(
                div()
                    .absolute()
                    .left(px(12.))
                    .top(px(12.))
                    .px(px(8.))
                    .py(px(4.))
                    .rounded(px(3.))
                    .bg(rgba(0x0F0E0DCC))
                    .border_1()
                    .border_color(if self.map.placing { Hsla::from(rgb(accent_line())) } else { border_control() })
                    .child(mono(chip_text, 9.5, if self.map.placing { accent_line() } else { TEXT_TERTIARY })),
            )
            .child(
                div()
                    .absolute()
                    .right(px(12.))
                    .bottom(px(if tiles_on { 26. } else { 12. }))
                    .flex()
                    .flex_col()
                    .child(control("map-zoom-in", "+").on_click(cx.listener(|this, _, _, cx| this.map_zoom_by(1., None, cx))))
                    .child(control("map-zoom-out", "−").on_click(cx.listener(|this, _, _, cx| this.map_zoom_by(-1., None, cx)))),
            )
            .when(tiles_on, |d| {
                d.child(
                    div()
                        .absolute()
                        .right(px(0.))
                        .bottom(px(0.))
                        .px(px(6.))
                        .py(px(2.))
                        .bg(rgba(0x0F0E0DCC))
                        .child(mono("Tiles © Esri — Esri, HERE, Garmin, © OpenStreetMap contributors", 9., TEXT_DIM)),
                )
            })
            .when(faceted.is_empty() && self.map.strip == StripMode::Located, |d| {
                d.child(
                    div()
                        .absolute()
                        .left(px(w as f32 / 2. - 170.))
                        .top(px(h as f32 / 2. - 30.))
                        .w(px(340.))
                        .p(px(12.))
                        .rounded(px(6.))
                        .bg(rgba(0x0F0E0DE6))
                        .border_1()
                        .border_color(border_control())
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .child(div().text_size(sp(12.)).text_color(rgb(TEXT_PRIMARY)).child("No photos with a location"))
                        .child(div().text_size(sp(11.)).text_color(rgb(TEXT_DIM)).child(
                            "Photos with GPS in their EXIF appear here. Choose “No location”, select photos, then Place on map.",
                        )),
                )
            });
        let _ = window;
        map
    }

    /// Latitude/longitude grid lines with labels (the local basemap).
    fn graticule(&self, v: &View, faint: bool) -> Vec<Div> {
        let mut out = Vec::new();
        let (s, wlon, n, elon) = v.bounds();
        let px_per_deg = geo::TILE * 2f64.powf(v.zoom) / 360.;
        let step = [
            90., 45., 30., 15., 10., 5., 2., 1., 0.5, 0.25, 0.1, 0.05, 0.02, 0.01, 0.005, 0.002,
            0.001,
        ]
        .into_iter()
        .rev()
        .find(|st| st * px_per_deg >= 90.)
        .unwrap_or(90.);
        let line = if faint { 0xFFFFFF0A } else { 0xFFFFFF12 };
        let strong = if faint { 0xFFFFFF14 } else { 0xFFFFFF22 };
        let (w, h) = (v.width as f32, v.height as f32);
        // Meridians (handle a view that crosses the antimeridian).
        let span = if elon >= wlon {
            elon - wlon
        } else {
            elon + 360. - wlon
        };
        let mut lon = (wlon / step).floor() * step;
        let mut guard = 0;
        while lon <= wlon + span + step && guard < 400 {
            guard += 1;
            let l = geo::wrap_lon(lon);
            let (x, _) = v.to_screen(0., l);
            if (0. ..=w as f64).contains(&x) {
                out.push(
                    div()
                        .absolute()
                        .left(px(x as f32))
                        .top(px(0.))
                        .w(px(1.))
                        .h(px(h))
                        .bg(rgba(if l.abs() < 1e-9 { strong } else { line })),
                );
                if !faint {
                    out.push(
                        div()
                            .absolute()
                            .left(px(x as f32 + 4.))
                            .bottom(px(4.))
                            .child(mono(
                                format!(
                                    "{:.*}°{}",
                                    decimals(step),
                                    l.abs(),
                                    if l < 0. {
                                        "W"
                                    } else if l > 0. {
                                        "E"
                                    } else {
                                        ""
                                    }
                                ),
                                9.,
                                0x4A504D,
                            )),
                    );
                }
            }
            lon += step;
        }
        let mut lat = (s / step).floor() * step;
        guard = 0;
        while lat <= n + step && guard < 400 {
            guard += 1;
            if lat.abs() <= geo::MAX_LAT {
                let (_, y) = v.to_screen(lat, v.lon);
                if (0. ..=h as f64).contains(&y) {
                    out.push(
                        div()
                            .absolute()
                            .left(px(0.))
                            .top(px(y as f32))
                            .w(px(w))
                            .h(px(1.))
                            .bg(rgba(if lat.abs() < 1e-9 { strong } else { line })),
                    );
                    if !faint {
                        out.push(
                            div()
                                .absolute()
                                .left(px(4.))
                                .top(px(y as f32 + 3.))
                                .child(mono(
                                    format!(
                                        "{:.*}°{}",
                                        decimals(step),
                                        lat.abs(),
                                        if lat < 0. {
                                            "S"
                                        } else if lat > 0. {
                                            "N"
                                        } else {
                                            ""
                                        }
                                    ),
                                    9.,
                                    0x4A504D,
                                )),
                        );
                    }
                }
            }
            lat += step;
        }
        out
    }

    fn map_left(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let located = self.map_located();
        let faceted = self.map_faceted();
        let pts: Vec<(i64, f64, f64)> = faceted.iter().map(|l| (l.id, l.lat, l.lon)).collect();
        let places = geo::places(&pts, 0.5, 8);
        let span = Self::map_day_span(&located);
        let (f0, f1) = self.map.date;
        // Camera facet counts over the date window (ignoring the camera
        // facet itself, so every camera stays choosable).
        let mut cams: HashMap<String, usize> = HashMap::new();
        let window_days = span.map(|(a, b)| {
            let len = (b - a) as f32;
            (a + (len * f0).floor() as i64, a + (len * f1).ceil() as i64)
        });
        for l in located.iter().filter(|l| match (window_days, l.day) {
            (Some((a, b)), Some(d)) => d >= a && d <= b,
            (Some(_), None) => f0 <= 0.0 && f1 >= 1.0,
            _ => true,
        }) {
            if !l.camera.is_empty() {
                *cams.entry(l.camera.clone()).or_insert(0) += 1;
            }
        }
        let mut cams: Vec<(String, usize)> = cams.into_iter().collect();
        cams.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        cams.truncate(8);

        let row = |id: (&'static str, usize), label: String, count: usize, active: bool| {
            div().id(id).px(px(6.)).child(list_row::list_row(
                &label,
                &count.to_string(),
                if active { accent_line() } else { 0x424446 },
                active,
            ))
        };
        let mut col = div()
            .id("map-left-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .child(rail_heading("Places"));
        if places.is_empty() {
            col = col.child(
                div()
                    .px(px(14.))
                    .text_size(sp(11.))
                    .text_color(rgb(TEXT_DIM))
                    .child("No located photos yet"),
            );
        }
        for (i, p) in places.iter().enumerate() {
            let (lat, lon) = (p.lat, p.lon);
            col = col.child(
                row(("map-place", i), p.label.clone(), p.count, false)
                    .on_hover(self.tip("Fly to this area"))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let v = this.map_view_now();
                        this.map.view = View {
                            lat,
                            lon,
                            zoom: 10.,
                            ..v
                        };
                        this.map_save_view();
                        cx.notify();
                    })),
            );
        }
        // Date range.
        col = col.child(rail_heading("Date range"));
        match span {
            Some((a, b)) => {
                let len = (b - a) as f32;
                let d0 = a + (len * f0).floor() as i64;
                let d1 = a + (len * f1).ceil() as i64;
                let slot = self.map.date_box.clone();
                col = col
                    .child(
                        div()
                            .px(px(14.))
                            .flex()
                            .justify_between()
                            .child(mono(date_label(d0), 10.5, 0xA8A29A))
                            .child(mono(date_label(d1), 10.5, 0xA8A29A)),
                    )
                    .child(
                        div()
                            .id("map-date-track")
                            .mx(px(14.))
                            .mt(px(6.))
                            .h(px(16.))
                            .relative()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, ev: &MouseDownEvent, _, cx| {
                                    let b = this.map.date_box.get();
                                    let w = b.size.width.as_f32().max(1.);
                                    let f = ((ev.position.x.as_f32() - b.origin.x.as_f32()) / w)
                                        .clamp(0., 1.);
                                    // Nearest handle takes the drag.
                                    let h = if (f - this.map.date.0).abs()
                                        <= (f - this.map.date.1).abs()
                                    {
                                        0
                                    } else {
                                        1
                                    };
                                    this.map.gesture = Some(MapGesture::Date(h));
                                    this.map_mouse_move(
                                        (ev.position.x.as_f32(), ev.position.y.as_f32()),
                                        true,
                                        cx,
                                    );
                                }),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .left_0()
                                    .right_0()
                                    .top(px(7.))
                                    .h(px(2.))
                                    .bg(rgb(0x2A2723))
                                    .child(meter(slot)),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .left(relative(f0))
                                    .w(relative(f1 - f0))
                                    .top(px(7.))
                                    .h(px(2.))
                                    .bg(rgb(accent_line())),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .left(relative(f0))
                                    .ml(px(-4.5))
                                    .top(px(3.5))
                                    .size(px(9.))
                                    .rounded_full()
                                    .bg(rgb(0xE9E5DE)),
                            )
                            .child(
                                div()
                                    .absolute()
                                    .left(relative(f1))
                                    .ml(px(-4.5))
                                    .top(px(3.5))
                                    .size(px(9.))
                                    .rounded_full()
                                    .bg(rgb(0xE9E5DE)),
                            ),
                    )
                    .when(f0 > 0. || f1 < 1., |d| {
                        d.child(
                            div()
                                .id("map-date-reset")
                                .px(px(14.))
                                .pt(px(4.))
                                .text_size(sp(10.))
                                .text_color(rgb(TEXT_DIM))
                                .hover(|s| s.text_color(rgb(TEXT_SECONDARY)))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.map.date = (0., 1.);
                                    cx.notify();
                                }))
                                .child("show all dates"),
                        )
                    });
            }
            None => {
                col = col.child(
                    div()
                        .px(px(14.))
                        .text_size(sp(11.))
                        .text_color(rgb(TEXT_DIM))
                        .child("No capture dates"),
                );
            }
        }
        // Cameras.
        col = col.child(rail_heading("Camera"));
        for (i, (name, n)) in cams.iter().enumerate() {
            let label = name.clone();
            let active = self.map.camera.as_deref() == Some(name.as_str());
            col = col.child(
                row(("map-camera", i), name.clone(), *n, active)
                    .on_hover(self.tip("Only this camera"))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if this.map.camera.as_deref() == Some(label.as_str()) {
                            this.map.camera = None;
                        } else {
                            this.map.camera = Some(label.clone());
                        }
                        cx.notify();
                    })),
            );
        }
        let _ = window;
        div()
            .w(px(self.left_rail_width))
            .flex_none()
            .flex()
            .flex_col()
            .bg(rgb(bg_panel()))
            .border_r_1()
            .border_color(hairline())
            .child(col)
    }

    fn map_right(&self, cx: &mut Context<Self>) -> Div {
        let p = self.primary_photo();
        let mut col = div()
            .id("map-right-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .flex()
            .flex_col();
        let Some(p) = p else {
            return div()
                .w(px(self.right_rail_width))
                .flex_none()
                .bg(rgb(bg_panel()))
                .border_l_1()
                .border_color(hairline())
                .p(px(14.))
                .text_size(sp(11.))
                .text_color(rgb(TEXT_DIM))
                .child("Select a photo on the map or in the filmstrip");
        };
        self.thumb_seen.borrow_mut().push(p.id);
        let thumb = self.thumbs.get(&p.id).map(|t| t.image.clone());
        let (pw, ph) = (278., 185.);
        let (a, b) = placeholder_tint(&p.blake3);
        col = col.child(
            div()
                .p(px(14.))
                .pb(px(6.))
                .child(
                    div()
                        .relative()
                        .w(px(pw))
                        .h(px(ph))
                        .overflow_hidden()
                        .border_1()
                        .border_color(rgba(0xFFFFFF12))
                        .when(thumb.is_none(), |d| {
                            d.bg(linear_gradient(
                                160.,
                                linear_color_stop(rgb(a), 0.),
                                linear_color_stop(rgb(b), 1.),
                            ))
                        })
                        .children(thumb.map(|im| {
                            crate::gallery_canvas::fitted_image(
                                im,
                                pw,
                                ph,
                                laika_core::gallery::Fit::Fit,
                                (0.5, 0.5),
                            )
                        })),
                )
                .child(
                    div()
                        .pt(px(6.))
                        .child(mono(p.filename.clone(), 11., TEXT_PRIMARY)),
                ),
        );
        let location = geo::parse_gps(&p.exif_gps);
        let stars = if p.rating > 0 {
            "★".repeat(p.rating as usize)
        } else {
            "—".to_string()
        };
        let rows: Vec<(&str, String)> = vec![
            ("Camera", p.camera.trim_matches('"').to_string()),
            ("Lens", p.lens.trim_matches('"').to_string()),
            ("Focal", p.focal_mm.clone()),
            ("Aperture", p.aperture.clone()),
            ("Shutter", p.shutter.clone()),
            ("ISO", p.iso.trim_start_matches("ISO ").to_string()),
            ("Metering", p.exif_metering.clone()),
            ("Flash", p.exif_flash.clone()),
            ("Captured", p.captured_at.clone()),
            (
                "GPS",
                location
                    .map(|(la, lo)| geo::display_gps(la, lo))
                    .unwrap_or_default(),
            ),
            ("Artist", p.creator.clone()),
            ("Copyright", p.copyright.clone()),
            ("Rating", stars),
        ];
        col = col.child(rail_heading("EXIF")).child(
            div()
                .px(px(14.))
                .flex()
                .flex_col()
                .gap(px(5.))
                .children(rows.into_iter().map(|(k, v)| {
                    div()
                        .flex()
                        .justify_between()
                        .gap(px(10.))
                        .child(
                            div()
                                .flex_none()
                                .text_size(sp(11.))
                                .text_color(rgb(TEXT_DIM))
                                .child(k),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .text_size(sp(11.))
                                .text_color(rgb(if v.is_empty() {
                                    TEXT_DIMMER
                                } else {
                                    TEXT_SECONDARY
                                }))
                                .child(if v.is_empty() { "—".to_string() } else { v }),
                        )
                })),
        );
        // Location actions.
        let pid = p.id;
        let button = |id: &'static str, label: &str, primary: bool| {
            div()
                .id(id)
                .px(px(9.))
                .py(px(5.))
                .rounded(px(3.))
                .text_size(sp(11.))
                .when(primary, |d| {
                    d.bg(rgb(accent_fill()))
                        .text_color(rgb(accent_on_fill()))
                        .hover(|s| s.bg(rgb(accent_fill_hover())))
                })
                .when(!primary, |d| {
                    d.border_1()
                        .border_color(border_control())
                        .text_color(rgb(TEXT_SECONDARY))
                        .hover(|s| s.bg(rgb(bg_row_hover())))
                })
                .child(label.to_string())
        };
        let n = self.targets().len().max(1);
        col = col.child(rail_heading("Location")).child(
            div()
                .px(px(14.))
                .flex()
                .flex_wrap()
                .gap(px(6.))
                .child(
                    button(
                        "map-place",
                        &if self.map.placing {
                            "Click the map…".to_string()
                        } else if location.is_some() {
                            format!("Move {n} on map")
                        } else {
                            format!("Place {n} on map")
                        },
                        location.is_none() || self.map.placing,
                    )
                    .on_hover(self.tip("Then click the map where the photo was taken"))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.map.placing = !this.map.placing;
                        cx.notify();
                    })),
                )
                .when(location.is_some(), |d| {
                    let (la, lo) = location.unwrap_or_default();
                    d.child(button("map-center", "Center", false).on_click(cx.listener(
                        move |this, _, _, cx| {
                            let v = this.map_view_now();
                            this.map.view = View {
                                lat: la,
                                lon: lo,
                                zoom: v.zoom.max(13.),
                                ..v
                            };
                            this.map_save_view();
                            cx.notify();
                        },
                    )))
                    .child(
                        button("map-clear-location", "Remove", false)
                            .on_hover(
                                self.tip("Clear the location Laika stores for the selected photos"),
                            )
                            .on_click(cx.listener(|this, _, _, cx| this.map_clear_location(cx))),
                    )
                })
                .child(
                    button("map-open-library", "Show in Library", false).on_click(cx.listener(
                        move |this, _, _, cx| {
                            this.state.active_module = Module::Library;
                            this.select_navigate(pid, SelectMode::Set, cx);
                            cx.notify();
                        },
                    )),
                ),
        );
        // Storage provenance.
        let offline = self.offline.contains(&p.id);
        let remote = if p.remote_key.is_empty() {
            match p.sync {
                SyncState::Pending => "queued for backup".to_string(),
                SyncState::Failed => "backup failed".to_string(),
                _ => "not backed up".to_string(),
            }
        } else {
            p.remote_key.clone()
        };
        let storage = vec![
            (
                "Local",
                p.path.clone(),
                if offline { 0xE56060 } else { accent_line() },
            ),
            (
                "Remote",
                remote,
                if p.sync == SyncState::Synced {
                    accent_line()
                } else {
                    TEXT_SECONDARY
                },
            ),
            (
                "Checksum",
                format!("blake3 · {}", p.blake3.get(..12).unwrap_or(&p.blake3)),
                TEXT_SECONDARY,
            ),
        ];
        col = col.child(rail_heading("Storage")).child(
            div()
                .px(px(14.))
                .pb(px(14.))
                .flex()
                .flex_col()
                .gap(px(5.))
                .children(storage.into_iter().map(|(k, v, color)| {
                    div()
                        .flex()
                        .gap(px(10.))
                        .child(
                            div()
                                .w(px(62.))
                                .flex_none()
                                .text_size(sp(11.))
                                .text_color(rgb(TEXT_DIM))
                                .child(k),
                        )
                        .child(
                            div()
                                .min_w_0()
                                .flex_1()
                                .truncate()
                                .child(mono(v, 10.5, color)),
                        )
                })),
        );
        div()
            .w(px(self.right_rail_width))
            .flex_none()
            .flex()
            .flex_col()
            .bg(rgb(bg_panel()))
            .border_l_1()
            .border_color(hairline())
            .child(col)
    }
}

fn decimals(step: f64) -> usize {
    if step >= 1. {
        0
    } else if step >= 0.1 {
        1
    } else if step >= 0.01 {
        2
    } else {
        3
    }
}

#[cfg(test)]
mod tests {
    use super::{date_label, day_of};

    #[test]
    fn capture_days_round_trip() {
        assert_eq!(day_of("1970-01-01T00:00:00"), Some(0));
        assert_eq!(day_of("2026:09:14 06:51:25"), day_of("2026-09-14"));
        for date in ["2000-02-29", "2016-03-07", "2026-12-31", "1969-07-20"] {
            assert_eq!(date_label(day_of(date).unwrap()), date);
        }
        assert_eq!(day_of(""), None);
        assert_eq!(day_of("2026-13-01"), None);
    }
}
