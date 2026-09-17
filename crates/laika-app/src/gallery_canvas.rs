//! G05–G09, G12, G14, G16: galleries home, editor toolbar, tray, and the
//! canvas page (placement, drag and drop, selection handles, resize,
//! breakpoint preview) rendered with the gallery's own theme.

use laika_core::gallery::layout::{self, Breakpoint, Cell as GridCell};
use laika_core::gallery::{Fit, Gallery, Status, TypePairing};

use super::gallery_ui::*;
use super::*;

/// Canvas type scale relative to the published page (760px page vs the
/// ~1264px content width at 1440).
const CANVAS_SCALE: f32 = 0.6;
const CAPTION_H: f32 = 19.;

/// Page layout on the canvas at one breakpoint.
pub(crate) struct PageGeom {
    pub cols: u8,
    pub col_w: f32,
    pub row_h: f32,
    pub gutter: f32,
    pub row_y: Vec<f32>,
    pub extra: Vec<f32>,
    /// (photo index, cell, span_x, span_y) in reading order.
    pub tiles: Vec<(usize, GridCell, u8, u8)>,
    pub height: f32,
}

impl PageGeom {
    pub fn new(g: &Gallery, bp: Breakpoint, spare_rows: u16) -> Self {
        let cols = bp.columns(g.columns);
        let inner = page_width(bp) - 2. * page_pad(bp);
        let gutter = g.gutter as f32;
        let (col_w, row_h) = layout::metrics(inner, cols, gutter, g.ratio);
        let tiles = g.layout_at(bp);
        let cells: Vec<(GridCell, u8, u8)> = tiles.iter().map(|t| (t.1, t.2, t.3)).collect();
        let rows = layout::row_count(&cells) + spare_rows;
        let mut extra = vec![0.; rows as usize];
        if g.theme.show_captions {
            for (i, c, _, sy) in &tiles {
                let last = (c.row + *sy as u16).saturating_sub(1) as usize;
                if !g.photos[*i].caption.trim().is_empty() && last < extra.len() {
                    extra[last] = CAPTION_H;
                }
            }
        }
        let mut row_y = Vec::with_capacity(rows as usize);
        let mut y = 0.;
        for e in &extra {
            row_y.push(y);
            y += row_h + e + gutter;
        }
        Self {
            cols,
            col_w,
            row_h,
            gutter,
            row_y,
            extra,
            tiles,
            height: (y - gutter).max(0.),
        }
    }

    fn y_of(&self, row: u16) -> f32 {
        let r = row as usize;
        match self.row_y.get(r) {
            Some(y) => *y,
            None => {
                let last = self.row_y.len();
                let base = if last == 0 { 0. } else { self.height + self.gutter };
                base + (r - last) as f32 * (self.row_h + self.gutter)
            }
        }
    }

    /// (x, y, w, image h) of a tile.
    pub fn rect(&self, cell: GridCell, sx: u8, sy: u8) -> (f32, f32, f32, f32) {
        let sx = sx.max(1) as f32;
        let sy = sy.max(1) as f32;
        (
            cell.col as f32 * (self.col_w + self.gutter),
            self.y_of(cell.row),
            sx * self.col_w + (sx - 1.) * self.gutter,
            sy * self.row_h + (sy - 1.) * self.gutter,
        )
    }

    pub fn cell_at(&self, x: f32, y: f32) -> GridCell {
        let col = ((x.max(0.)) / (self.col_w + self.gutter)).floor() as i64;
        let col = col.clamp(0, self.cols as i64 - 1) as u16;
        let mut row = self.row_y.len();
        for (r, top) in self.row_y.iter().enumerate() {
            if y < top + self.row_h + self.extra[r] + self.gutter {
                row = r;
                break;
            }
        }
        if row == self.row_y.len() {
            let past = (y - (self.height + self.gutter)).max(0.);
            row += (past / (self.row_h + self.gutter)).floor() as usize;
        }
        GridCell { col, row: row as u16 }
    }
}

pub(crate) fn page_width(bp: Breakpoint) -> f32 {
    bp.page_width()
}

pub(crate) fn page_pad(bp: Breakpoint) -> f32 {
    match bp {
        Breakpoint::Desktop => 42.,
        Breakpoint::Tablet => 34.,
        Breakpoint::Phone => 18.,
    }
}

/// Published pixel size of a tile at 1440px wide (for the size badge).
fn published_px(g: &Gallery, sx: u8, sy: u8) -> (u32, u32) {
    let cols = g.columns.max(1) as f32;
    let gap = g.gutter as f32 * 5. / 3.;
    let col = (1264. - (cols - 1.) * gap) / cols;
    let row = col / g.ratio.max(0.1);
    (
        (sx as f32 * col + (sx as f32 - 1.) * gap).round() as u32,
        (sy as f32 * row + (sy as f32 - 1.) * gap).round() as u32,
    )
}

fn small_button(id: impl Into<ElementId>, label: &str) -> Stateful<Div> {
    div()
        .id(id)
        .px(px(9.))
        .py(px(5.))
        .rounded(px(4.))
        .border_1()
        .border_color(border_control())
        .font_family(SANS)
        .text_size(px(11.))
        .text_color(rgb(TEXT_SECONDARY))
        .hover(|s| s.bg(rgb(bg_row_hover())))
        .child(label.to_string())
}

fn accent_button(id: impl Into<ElementId>, label: &str) -> Stateful<Div> {
    div()
        .id(id)
        .px(px(13.))
        .py(px(5.))
        .rounded(px(4.))
        .bg(rgb(accent_fill()))
        .font_family(SANS)
        .font_weight(FontWeight::SEMIBOLD)
        .text_size(px(11.))
        .text_color(rgb(accent_on_fill()))
        .hover(|s| s.bg(rgb(accent_fill_hover())))
        .child(label.to_string())
}

fn eyebrow(text: &str) -> Div {
    div()
        .font_family(PLEX_MONO)
        .font_weight(FontWeight::MEDIUM)
        .text_size(px(9.5))
        .text_color(rgb(TEXT_DIM))
        .child(text.to_uppercase())
}

/// A photo drawn into a box honoring fit and the focal point.
pub(crate) fn fitted_image(image: Arc<RenderImage>, w: f32, h: f32, fit: Fit, focal: (f32, f32)) -> Div {
    let sz = image.size(0);
    let (iw, ih) = (sz.width.0.max(1) as f32, sz.height.0.max(1) as f32);
    let s = match fit {
        Fit::Fill => (w / iw).max(h / ih),
        Fit::Fit => (w / iw).min(h / ih),
    };
    let (dw, dh) = (iw * s, ih * s);
    let (left, top) = match fit {
        Fit::Fill => (-(dw - w) * focal.0.clamp(0., 1.), -(dh - h) * focal.1.clamp(0., 1.)),
        Fit::Fit => ((w - dw) / 2., (h - dh) / 2.),
    };
    div()
        .absolute()
        .left(px(left))
        .top(px(top))
        .w(px(dw))
        .h(px(dh))
        .child(img(ImageSource::Render(image)).size_full())
}

impl Laika {
    /// The Publish module body.
    pub(crate) fn publish_module(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        self.ensure_gallery_fonts(cx);
        if self.catalog.is_none() {
            return div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .bg(rgb(bg_canvas()))
                .text_size(px(12.))
                .text_color(rgb(TEXT_DIM))
                .child("Open a catalog to build galleries");
        }
        if self.gal.current.is_none() {
            return self.galleries_home(cx);
        }
        let body = div()
            .flex_1()
            .min_h_0()
            .flex()
            .child(self.gallery_tray(cx))
            .child(self.gallery_canvas(window, cx))
            .child(self.gallery_inspector(window, cx));
        let mut root = div()
            .relative()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(self.gallery_toolbar(cx))
            .child(body)
            .child(self.gallery_status_line());
        if self.gal.picker.is_some() {
            root = root.child(self.layout_picker(cx));
        }
        if self.gal.publish.is_some() {
            root = root.child(self.publish_sheet(cx));
        }
        root
    }

    // ---- home ------------------------------------------------------------------

    fn galleries_home(&self, cx: &mut Context<Self>) -> Div {
        let sel = self.state.selection.len();
        let collections: Vec<(i64, String, usize)> = self
            .coll
            .list
            .iter()
            .filter(|c| c.count > 0)
            .map(|c| (c.id, c.display_title().to_string(), c.count))
            .collect();
        let mut cards = div().flex().flex_wrap().gap(px(18.));
        for s in &self.gal.list {
            if let Some(pid) = s.cover_photo_id {
                self.thumb_seen.borrow_mut().push(pid);
            }
            let id = s.id;
            let cover = s
                .cover_photo_id
                .and_then(|pid| self.thumbs.get(&pid))
                .map(|t| t.image.clone());
            let confirming = self.gal.confirm_delete == Some(id);
            let status = match s.status {
                Status::Draft => "Draft",
                Status::Published => "Published",
            };
            cards = cards.child(
                div()
                    .id(("gal-card", id as usize))
                    .w(px(220.))
                    .flex()
                    .flex_col()
                    .rounded(px(6.))
                    .overflow_hidden()
                    .bg(rgb(bg_panel()))
                    .border_1()
                    .border_color(hairline())
                    .hover(|d| d.border_color(border_strong()))
                    .on_click(cx.listener(move |this, _, _, cx| this.open_gallery(id, cx)))
                    .child(
                        div()
                            .relative()
                            .h(px(140.))
                            .overflow_hidden()
                            .bg(rgb(bg_well()))
                            .children(cover.map(|im| fitted_image(im, 220., 140., Fit::Fill, (0.5, 0.5)))),
                    )
                    .child(
                        div()
                            .p(px(10.))
                            .flex()
                            .flex_col()
                            .gap(px(3.))
                            .child(
                                div()
                                    .text_size(px(12.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(rgb(TEXT_PRIMARY))
                                    .child(if s.title.trim().is_empty() {
                                        "Untitled gallery".to_string()
                                    } else {
                                        s.title.clone()
                                    }),
                            )
                            .child(
                                div()
                                    .text_size(px(10.5))
                                    .text_color(rgb(TEXT_DIM))
                                    .child(format!(
                                        "{} photo{} · {status}",
                                        s.photo_count,
                                        if s.photo_count == 1 { "" } else { "s" }
                                    )),
                            )
                            .child(
                                div()
                                    .flex()
                                    .gap(px(6.))
                                    .pt(px(6.))
                                    .child(
                                        small_button(("gal-dup", id as usize), "Duplicate").on_click(
                                            cx.listener(move |this, _, _, cx| {
                                                cx.stop_propagation();
                                                if let Some(cat) = this.catalog.as_ref() {
                                                    match cat.duplicate_gallery(id) {
                                                        Ok(_) => this.status_note = "gallery duplicated".to_string(),
                                                        Err(e) => this.status_note = e,
                                                    }
                                                }
                                                this.load_galleries();
                                                cx.notify();
                                            }),
                                        ),
                                    )
                                    .child(
                                        small_button(
                                            ("gal-del", id as usize),
                                            if confirming { "Confirm delete" } else { "Delete" },
                                        )
                                        .when(confirming, |d| d.text_color(rgb(0xE56060)))
                                        .on_hover(self.tip("Deletes the gallery (photos and built folders stay)"))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            cx.stop_propagation();
                                            if this.gal.confirm_delete == Some(id) {
                                                if let Some(cat) = this.catalog.as_ref() {
                                                    match cat.delete_gallery(id) {
                                                        Ok(()) => this.status_note = "gallery deleted".to_string(),
                                                        Err(e) => this.status_note = e,
                                                    }
                                                }
                                                this.gal.confirm_delete = None;
                                                this.load_galleries();
                                            } else {
                                                this.gal.confirm_delete = Some(id);
                                            }
                                            cx.notify();
                                        })),
                                    ),
                            ),
                    ),
            );
        }
        let empty = self.gal.list.is_empty();
        div()
            .flex_1()
            .min_h_0()
            .bg(rgb(bg_canvas()))
            .child(
                div()
                    .id("galleries-home")
                    .size_full()
                    .overflow_y_scroll()
                    .p(px(34.))
                    .flex()
                    .flex_col()
                    .gap(px(20.))
                    .child(
                        div()
                            .flex()
                            .items_end()
                            .gap(px(14.))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(4.))
                                    .child(eyebrow("Publish"))
                                    .child(
                                        div()
                                            .text_size(px(22.))
                                            .font_weight(FontWeight::SEMIBOLD)
                                            .text_color(rgb(TEXT_PRIMARY))
                                            .child("Galleries"),
                                    ),
                            )
                            .child(div().flex_1())
                            .child(
                                small_button("gal-new-empty", "New empty gallery").on_click(cx.listener(
                                    |this, _, _, cx| this.new_gallery("Untitled gallery", Vec::new(), cx),
                                )),
                            )
                            .child(
                                div()
                                    .relative()
                                    .child(small_button("gal-new-coll", "From collection…").on_click(
                                        cx.listener(|this, _, _, cx| {
                                            this.gal.from_collection_open = !this.gal.from_collection_open;
                                            cx.notify();
                                        }),
                                    ))
                                    .when(self.gal.from_collection_open, |d| {
                                        d.child(
                                            div()
                                                .absolute()
                                                .top(px(30.))
                                                .right(px(0.))
                                                .w(px(240.))
                                                .p(px(4.))
                                                .rounded(px(5.))
                                                .bg(rgb(bg_panel()))
                                                .border_1()
                                                .border_color(border_control())
                                                .flex()
                                                .flex_col()
                                                .when(collections.is_empty(), |d| {
                                                    d.child(
                                                        div()
                                                            .p(px(8.))
                                                            .text_size(px(11.))
                                                            .text_color(rgb(TEXT_DIM))
                                                            .child("No collections with photos yet"),
                                                    )
                                                })
                                                .children(collections.iter().map(|(cid, name, n)| {
                                                    let cid = *cid;
                                                    div()
                                                        .id(("gal-from-coll", cid as usize))
                                                        .flex()
                                                        .justify_between()
                                                        .px(px(8.))
                                                        .py(px(6.))
                                                        .rounded(px(3.))
                                                        .text_size(px(11.5))
                                                        .text_color(rgb(TEXT_SECONDARY))
                                                        .hover(|s| s.bg(rgb(bg_row_hover())))
                                                        .on_click(cx.listener(move |this, _, _, cx| {
                                                            this.new_gallery_from_collection(cid, cx)
                                                        }))
                                                        .child(name.clone())
                                                        .child(
                                                            div()
                                                                .text_color(rgb(TEXT_DIM))
                                                                .child(n.to_string()),
                                                        )
                                                })),
                                        )
                                    }),
                            )
                            .child(
                                accent_button(
                                    "gal-new-sel",
                                    &if sel == 0 {
                                        "New gallery from selection".to_string()
                                    } else {
                                        format!("New gallery from {sel} selected")
                                    },
                                )
                                .when(sel == 0, |d| d.opacity(0.5))
                                .on_hover(self.tip("Select photos in the Library first"))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    if this.state.selection.is_empty() {
                                        this.status_note =
                                            "select photos in the Library, then come back to Publish".to_string();
                                        cx.notify();
                                        return;
                                    }
                                    let title = this
                                        .viewed_collection()
                                        .map(|c| c.display_title().to_string())
                                        .unwrap_or_else(|| "Untitled gallery".to_string());
                                    let ids = this.state.selection.iter().copied().collect();
                                    this.new_gallery(&title, ids, cx);
                                })),
                            ),
                    )
                    .when(empty, |d| {
                        d.child(
                            div()
                                .mt(px(40.))
                                .flex()
                                .flex_col()
                                .items_center()
                                .gap(px(8.))
                                .child(
                                    div()
                                        .text_size(px(15.))
                                        .text_color(rgb(TEXT_SECONDARY))
                                        .child("No galleries yet"),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.5))
                                        .text_color(rgb(TEXT_DIM))
                                        .child("Select photos in the Library or pick a collection, then lay them out on a page you can publish as a website."),
                                ),
                        )
                    })
                    .child(cards),
            )
    }

    // ---- toolbar and status ----------------------------------------------------------

    fn gallery_toolbar(&self, cx: &mut Context<Self>) -> Div {
        let g = self.gal.current.as_ref().expect("open gallery");
        let status = match (&self.gal.save_error, self.gal.last_saved) {
            (Some(_), _) => ("not saved".to_string(), 0xE56060),
            (None, Some(t)) => (
                format!(
                    "{} · saved {}",
                    if g.status == Status::Published { "Published" } else { "Draft" },
                    Self::relative_time(t)
                ),
                accent_line(),
            ),
            (None, None) => (
                if g.status == Status::Published { "Published" } else { "Draft" }.to_string(),
                TEXT_TERTIARY,
            ),
        };
        let bp = self.gal.breakpoint;
        let segments = [Breakpoint::Desktop, Breakpoint::Tablet, Breakpoint::Phone];
        let building = self.gal.build.as_ref().map(|b| b.label());
        div()
            .h(px(theme::layout::TOOLBAR))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(10.))
            .px(px(12.))
            .bg(rgb(bg_chrome()))
            .border_b_1()
            .border_color(hairline())
            .child(
                small_button("gal-back", "‹ Galleries").on_click(cx.listener(|this, _, _, cx| {
                    this.close_gallery();
                    this.load_galleries();
                    cx.notify();
                })),
            )
            .child(
                div()
                    .text_size(px(13.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(rgb(TEXT_PRIMARY))
                    .max_w(px(280.))
                    .overflow_hidden()
                    .child(if g.title.trim().is_empty() {
                        "Untitled gallery".to_string()
                    } else {
                        g.title.clone()
                    }),
            )
            .child(
                div()
                    .px(px(8.))
                    .py(px(3.))
                    .rounded(px(10.))
                    .bg(rgb(bg_chip()))
                    .text_size(px(10.5))
                    .text_color(rgb(status.1))
                    .child(status.0),
            )
            .children(building.map(|label| {
                div()
                    .text_size(px(10.5))
                    .text_color(rgb(TEXT_TERTIARY))
                    .child(label)
            }))
            .child(div().flex_1())
            .child(
                small_button("gal-undo", "Undo")
                    .when(!self.gal.history.can_undo(), |d| d.opacity(0.45))
                    .on_hover(self.tip("Undo the last gallery change (⌘Z)"))
                    .on_click(cx.listener(|this, _, _, cx| this.gal_undo(cx))),
            )
            .child(
                small_button("gal-redo", "Redo")
                    .when(!self.gal.history.can_redo(), |d| d.opacity(0.45))
                    .on_hover(self.tip("Redo (⇧⌘Z)"))
                    .on_click(cx.listener(|this, _, _, cx| this.gal_redo(cx))),
            )
            .child(
                div()
                    .flex()
                    .p(px(2.))
                    .rounded(px(5.))
                    .bg(rgb(bg_segment_shell()))
                    .children(segments.iter().map(|s| {
                        let s = *s;
                        let on = s == bp;
                        div()
                            .id(("gal-bp", s as usize))
                            .px(px(10.))
                            .py(px(3.))
                            .rounded(px(4.))
                            .text_size(px(11.))
                            .text_color(rgb(if on { TEXT_PRIMARY } else { TEXT_MUTED }))
                            .when(on, |d| d.bg(rgb(bg_segment_active())))
                            .hover(|d| d.text_color(rgb(TEXT_PRIMARY)))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.gal.breakpoint = s;
                                cx.notify();
                            }))
                            .child(s.label())
                    })),
            )
            .child(
                small_button("gal-layout", "Layout…")
                    .on_hover(self.tip("Choose the grid template photos flow into"))
                    .on_click(cx.listener(|this, _, _, cx| this.open_picker(cx))),
            )
            .child(
                small_button("gal-preview", "Preview")
                    .on_hover(self.tip("Build a quick copy and open it in your browser (⌘P)"))
                    .on_click(cx.listener(|this, _, _, cx| this.preview_gallery(cx))),
            )
            .child(
                accent_button("gal-publish", "Publish")
                    .on_hover(self.tip("Build the site to a folder, optionally deploy (⇧⌘P)"))
                    .on_click(cx.listener(|this, _, _, cx| this.open_publish_sheet(cx))),
            )
    }

    fn gallery_status_line(&self) -> Div {
        let g = self.gal.current.as_ref().expect("open gallery");
        let placed = g.placed_count();
        let at = self
            .gal
            .selected
            .and_then(|id| g.index_of(id))
            .and_then(|i| g.photos[i].cell)
            .map(|c| format!("Row {} · cell {}", c.row + 1, c.col + 1))
            .unwrap_or_default();
        let hint = match self.gal.breakpoint {
            Breakpoint::Desktop => "Drag to place · corners resize, hold ⇧ to keep ratio · ⌫ removes from page · ⌘Z undo",
            _ => "Preview only — switch to Desktop to arrange",
        };
        div()
            .h(px(26.))
            .flex_none()
            .flex()
            .items_center()
            .gap(px(18.))
            .px(px(14.))
            .bg(rgb(bg_chrome()))
            .border_t_1()
            .border_color(hairline())
            .text_size(px(10.5))
            .text_color(rgb(TEXT_DIM))
            .child(format!("{placed} of {} placed", g.photos.len()))
            .child(at)
            .child(
                div()
                    .min_w_0()
                    .overflow_hidden()
                    .text_color(rgb(TEXT_SECONDARY))
                    .child(self.status_note.clone()),
            )
            .child(div().flex_1())
            .child(hint)
    }

    // ---- tray ------------------------------------------------------------------------

    fn gallery_tray(&self, cx: &mut Context<Self>) -> Div {
        let g = self.gal.current.as_ref().expect("open gallery");
        let unplaced = g.photos.iter().filter(|p| p.cell.is_none()).count();
        let order: std::collections::HashMap<usize, usize> = g
            .layout_at(Breakpoint::Desktop)
            .iter()
            .enumerate()
            .map(|(n, (i, ..))| (*i, n + 1))
            .collect();
        let filter = self.gal.tray_filter;
        let shown: Vec<usize> = (0..g.photos.len())
            .filter(|i| match filter {
                TrayFilter::All => true,
                TrayFilter::Unplaced => g.photos[*i].cell.is_none(),
                TrayFilter::Picks => self.find(g.photos[*i].photo_id).is_some_and(|p| p.picked),
            })
            .collect();
        let chip = |label: String, f: TrayFilter| {
            let on = filter == f;
            div()
                .id(("gal-tray-filter", f as usize))
                .px(px(8.))
                .py(px(3.))
                .rounded(px(10.))
                .text_size(px(10.5))
                .when(on, |d| d.bg(rgb(TEXT_PRIMARY)).text_color(rgb(bg_chrome())))
                .when(!on, |d| {
                    d.border_1()
                        .border_color(border_control())
                        .text_color(rgb(TEXT_SECONDARY))
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.gal.tray_filter = f;
                    cx.notify();
                }))
                .child(label)
        };
        let dragging = self.gal.drag.is_some_and(|d| d.moved && !d.from_tray);
        let cell_w = (248. - 24. - 8. - 6.) / 2.;
        let mut grid = div().flex().flex_wrap().gap(px(8.));
        for i in shown.iter().copied() {
            let p = &g.photos[i];
            let pid = p.photo_id;
            self.thumb_seen.borrow_mut().push(pid);
            let selected = self.gal.selected == Some(pid);
            let thumb = self.thumbs.get(&pid).map(|t| t.image.clone());
            let lifted = self.gal.drag.is_some_and(|d| d.moved && d.photo == pid);
            grid = grid.child(
                div()
                    .id(("gal-tray", pid as usize))
                    .relative()
                    .w(px(cell_w))
                    .h(px(cell_w))
                    .rounded(px(3.))
                    .overflow_hidden()
                    .bg(rgb(bg_well()))
                    .when(p.cell.is_some(), |d| d.opacity(if lifted { 0.3 } else { 0.55 }))
                    .when(lifted && p.cell.is_none(), |d| d.opacity(0.3))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                            let pos = (ev.position.x.as_f32(), ev.position.y.as_f32());
                            this.gal.drag = Some(GalDrag {
                                photo: pid,
                                from_tray: true,
                                start: pos,
                                pos,
                                moved: false,
                            });
                            cx.notify();
                        }),
                    )
                    .children(thumb.map(|im| fitted_image(im, cell_w, cell_w, Fit::Fill, (0.5, 0.5))))
                    .child(
                        div()
                            .absolute()
                            .left(px(4.))
                            .bottom(px(4.))
                            .px(px(4.))
                            .rounded(px(2.))
                            .bg(rgba(0x000000A0))
                            .font_family(PLEX_MONO)
                            .text_size(px(8.5))
                            .text_color(rgb(0xF2EFE6))
                            .child(order.get(&i).map(|n| n.to_string()).unwrap_or_else(|| "–".to_string())),
                    )
                    .when(selected, |d| {
                        d.child(
                            div()
                                .absolute()
                                .inset_0()
                                .rounded(px(3.))
                                .border_2()
                                .border_color(rgb(accent_line())),
                        )
                    }),
            );
        }
        let sel = self.state.selection.len();
        div()
            .w(px(248.))
            .flex_none()
            .flex()
            .flex_col()
            .bg(rgb(bg_panel()))
            .border_r_1()
            .border_color(hairline())
            .relative()
            .child(meter(self.gal.tray_box.clone()))
            .child(
                div()
                    .px(px(12.))
                    .pt(px(12.))
                    .pb(px(8.))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .child(eyebrow("In this gallery"))
                            .child(
                                div()
                                    .text_size(px(10.5))
                                    .text_color(rgb(TEXT_DIM))
                                    .child(format!("{} photos", g.photos.len())),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(5.))
                            .child(chip("All".to_string(), TrayFilter::All))
                            .child(chip(format!("Unplaced {unplaced}"), TrayFilter::Unplaced))
                            .child(chip("Picks".to_string(), TrayFilter::Picks)),
                    ),
            )
            .child(
                div()
                    .id("gal-tray-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(px(12.))
                    .pb(px(12.))
                    .when(shown.is_empty(), |d| {
                        d.child(
                            div()
                                .pt(px(20.))
                                .text_size(px(11.))
                                .text_color(rgb(TEXT_DIM))
                                .child(match filter {
                                    TrayFilter::All => "No photos yet",
                                    TrayFilter::Unplaced => "Every photo is on the page",
                                    TrayFilter::Picks => "No picks in this gallery",
                                }),
                        )
                    })
                    .child(grid),
            )
            .child(
                div()
                    .p(px(12.))
                    .border_t_1()
                    .border_color(hairline())
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        div()
                            .id("gal-add-sel")
                            .flex()
                            .justify_center()
                            .py(px(10.))
                            .rounded(px(7.))
                            .border_1()
                            .border_color(if dragging {
                                Hsla::from(rgb(accent_line()))
                            } else {
                                border_control()
                            })
                            .text_size(px(11.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(rgb(if sel > 0 { TEXT_PRIMARY } else { TEXT_DIM }))
                            .hover(|d| d.bg(rgb(bg_row_hover())))
                            .on_click(cx.listener(|this, _, _, cx| this.gal_add_selection(cx)))
                            .child(if dragging {
                                "Drop here to take it off the page".to_string()
                            } else if sel > 0 {
                                format!("Add {sel} selected from Library")
                            } else {
                                "Add photos: select them in Library".to_string()
                            }),
                    ),
            )
    }

    // ---- canvas -----------------------------------------------------------------------

    fn gallery_canvas(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let g = self.gal.current.as_ref().expect("open gallery");
        let bp = self.gal.breakpoint;
        let t = &g.theme;
        let desktop = bp == Breakpoint::Desktop;
        let dragging = self.gal.drag.filter(|d| d.moved);
        let spare = if desktop { if dragging.is_some() { 3 } else { 1 } } else { 0 };
        let geom = PageGeom::new(g, bp, spare);
        let page_w = page_width(bp);
        let pad = page_pad(bp);
        let ink = t.ink;
        let (title_family, title_weight) = match t.pairing {
            TypePairing::LightDisplay => (PLEX_SANS, FontWeight::NORMAL),
            _ => (PLEX_SANS, FontWeight::SEMIBOLD),
        };
        let caption_family = if t.pairing == TypePairing::SansMono { PLEX_MONO } else { PLEX_SANS };
        let title_px = match bp {
            Breakpoint::Phone => (t.title_size as f32 * CANVAS_SCALE).min(26.),
            _ => t.title_size as f32 * CANVAS_SCALE,
        };
        let radius = t.corner_radius as f32 * CANVAS_SCALE;

        let mut grid = div()
            .relative()
            .w(px(page_w - 2. * pad))
            .h(px(geom.height.max(geom.row_h)))
            .child(meter(self.gal.grid_box.clone()));

        // Drop ghost.
        if let Some(d) = dragging {
            if desktop {
                if let Some((cell, sx, sy)) = self.gal_drop_target(&d, d.pos) {
                    let (x, y, w, h) = geom.rect(cell, sx, sy);
                    grid = grid.child(
                        div()
                            .absolute()
                            .left(px(x))
                            .top(px(y))
                            .w(px(w))
                            .h(px(h))
                            .rounded(px(radius))
                            .bg(accent_wash_hsla())
                            .border_2()
                            .border_color(rgb(accent_line()))
                            .flex()
                            .items_center()
                            .justify_center()
                            .font_family(PLEX_MONO)
                            .text_size(px(10.))
                            .text_color(rgb(accent_line()))
                            .child("DROP HERE"),
                    );
                }
            }
        }

        for (i, cell, sx, sy) in geom.tiles.iter().copied() {
            let p = &g.photos[i];
            let pid = p.photo_id;
            self.thumb_seen.borrow_mut().push(pid);
            let (x, y, w, h) = geom.rect(cell, sx, sy);
            let selected = self.gal.selected == Some(pid);
            let lifted = dragging.is_some_and(|d| d.photo == pid);
            let thumb = self.thumbs.get(&pid).map(|t| t.image.clone());
            let caption = (t.show_captions && !p.caption.trim().is_empty()).then(|| p.caption.trim().to_string());
            let mut tile = div()
                .id(("gal-tile", pid as usize))
                .absolute()
                .left(px(x))
                .top(px(y))
                .w(px(w))
                .h(px(h))
                .rounded(px(radius))
                .overflow_hidden()
                .bg(rgb(t.canvas))
                .when(lifted, |d| d.opacity(0.35))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                        let pos = (ev.position.x.as_f32(), ev.position.y.as_f32());
                        this.gal.drag = Some(GalDrag {
                            photo: pid,
                            from_tray: false,
                            start: pos,
                            pos,
                            moved: false,
                        });
                        cx.notify();
                    }),
                );
            tile = match thumb {
                Some(im) => tile.child(fitted_image(im, w, h, p.fit, p.focal)),
                None => tile.child(
                    div()
                        .size_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_size(px(9.))
                        .text_color(rgba((ink << 8) | 0x55))
                        .child(self.find(pid).map(|r| r.filename.clone()).unwrap_or_default()),
                ),
            };
            grid = grid.child(tile);
            if let Some(c) = caption {
                grid = grid.child(
                    div()
                        .absolute()
                        .left(px(x))
                        .top(px(y + h + 5.))
                        .w(px(w))
                        .h(px(CAPTION_H - 5.))
                        .overflow_hidden()
                        .font_family(caption_family)
                        .text_size(px(9.5))
                        .text_color(rgba((ink << 8) | 0x80))
                        .child(c),
                );
            }
            if selected && !lifted {
                grid = grid.child(self.selection_chrome(g, pid, cell, sx, sy, (x, y, w, h), desktop, window, cx));
            }
        }

        let empty = g.photos.is_empty();
        let accent_text = t.accent_text();
        let page = div()
            .w(px(page_w))
            .flex_none()
            .bg(rgb(t.page))
            .pt(px(38.))
            .px(px(pad))
            .pb(px(40.))
            .shadow(vec![gpui_kit::BoxShadow {
                color: rgba(0x00000073).into(),
                offset: point(px(0.), px(2.)),
                blur_radius: px(20.),
                spread_radius: px(0.),
                inset: false,
            }])
            .flex()
            .flex_col()
            .gap(px(8.))
            .text_color(rgb(ink))
            .when(!g.site_name.trim().is_empty(), |d| {
                d.child(
                    div()
                        .pb(px(12.))
                        .mb(px(8.))
                        .border_b_1()
                        .border_color(rgba((ink << 8) | 0x1F))
                        .font_family(PLEX_SANS)
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_size(px(11.))
                        .child(g.site_name.clone()),
                )
            })
            .child(
                div()
                    .flex()
                    .items_end()
                    .justify_between()
                    .gap(px(16.))
                    .child(
                        div()
                            .font_family(title_family)
                            .font_weight(title_weight)
                            .text_size(px(title_px))
                            .line_height(relative(1.06))
                            .child(if g.title.trim().is_empty() {
                                "Untitled gallery".to_string()
                            } else {
                                g.title.clone()
                            }),
                    )
                    .when(!g.eyebrow.trim().is_empty(), |d| {
                        d.child(
                            div()
                                .flex_none()
                                .pb(px(6.))
                                .font_family(PLEX_MONO)
                                .text_size(px(8.5))
                                .text_color(rgb(accent_text))
                                .child(g.eyebrow.to_uppercase()),
                        )
                    }),
            )
            .when(!g.subtitle.trim().is_empty(), |d| {
                d.child(
                    div()
                        .max_w(px(400.))
                        .font_family(PLEX_SANS)
                        .text_size(px(11.))
                        .line_height(relative(1.6))
                        .text_color(rgba((ink << 8) | 0xCC))
                        .child(g.subtitle.clone()),
                )
            })
            .child(
                div()
                    .pb(px(14.))
                    .font_family(PLEX_SANS)
                    .text_size(px(9.))
                    .text_color(rgba((ink << 8) | 0x73))
                    .child(laika_export::site::html::meta_text(g.placed_count(), &g.meta_line)),
            )
            .when(empty, |d| {
                d.child(
                    div()
                        .h(px(220.))
                        .flex()
                        .flex_col()
                        .items_center()
                        .justify_center()
                        .gap(px(6.))
                        .border_1()
                        .border_color(rgba((ink << 8) | 0x33))
                        .rounded(px(6.))
                        .child(div().text_size(px(13.)).child("No photos yet"))
                        .child(
                            div()
                                .text_size(px(11.))
                                .text_color(rgba((ink << 8) | 0x99))
                                .child("Select photos in the Library, then use Add selected in the tray"),
                        ),
                )
            })
            .when(!empty && g.placed_count() == 0 && dragging.is_none(), |d| {
                d.child(
                    div()
                        .py(px(8.))
                        .text_size(px(11.))
                        .text_color(rgba((ink << 8) | 0x99))
                        .child("Drag photos from the tray onto the page"),
                )
            })
            .when(!empty, |d| d.child(grid));

        div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .bg(rgb(bg_canvas()))
            .child(
                div()
                    .h(px(30.))
                    .flex_none()
                    .flex()
                    .items_center()
                    .gap(px(14.))
                    .px(px(16.))
                    .border_b_1()
                    .border_color(hairline())
                    .font_family(PLEX_MONO)
                    .text_size(px(10.))
                    .text_color(rgb(TEXT_DIM))
                    .child(format!(
                        "{} · GRID {} COL",
                        layout::template(&g.template).name.to_uppercase(),
                        geom.cols
                    ))
                    .child(div().flex_1())
                    .child(format!("{} · {}px", bp.label().to_uppercase(), page_w as u32)),
            )
            .child(
                div()
                    .id("gal-canvas-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .track_scroll(&self.gal.canvas_scroll)
                    .on_click(cx.listener(|this, _, _, cx| {
                        // Clicks on the page background clear the selection.
                        let just_ended = this
                            .gal
                            .gesture_end
                            .is_some_and(|t| t.elapsed().as_millis() < 400);
                        if !just_ended && this.gal.drag.is_none() && this.gal.resize.is_none() {
                            this.gal.selected = None;
                            cx.notify();
                        }
                    }))
                    .child(div().w_full().flex().justify_center().py(px(28.)).child(page)),
            )
    }

    /// Outline, corner handles, and the size badge for the selected tile.
    #[allow(clippy::too_many_arguments)]
    fn selection_chrome(
        &self,
        g: &Gallery,
        pid: i64,
        cell: GridCell,
        sx: u8,
        sy: u8,
        (x, y, w, h): (f32, f32, f32, f32),
        editable: bool,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Div {
        let (pw, ph) = published_px(g, sx, sy);
        let accent = accent_line();
        let mut chrome = div()
            .absolute()
            .left(px(x - 3.))
            .top(px(y - 3.))
            .w(px(w + 6.))
            .h(px(h + 6.))
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .border_2()
                    .border_color(rgb(accent))
                    .rounded(px(2.)),
            )
            .child(
                div()
                    .absolute()
                    .left(px(0.))
                    .top(px(-20.))
                    .px(px(5.))
                    .py(px(2.))
                    .rounded(px(3.))
                    .bg(rgb(accent_fill()))
                    .font_family(PLEX_MONO)
                    .text_size(px(10.))
                    .text_color(rgb(accent_on_fill()))
                    .child(format!("{sx} × {sy} · {pw} × {ph}")),
            );
        if editable {
            for (right, bottom) in [(false, false), (true, false), (false, true), (true, true)] {
                let hx = if right { w + 6. - 5. } else { -2. };
                let hy = if bottom { h + 6. - 5. } else { -2. };
                let anchor = GridCell {
                    col: if right { cell.col } else { cell.col + sx as u16 - 1 },
                    row: if bottom { cell.row } else { cell.row + sy as u16 - 1 },
                };
                chrome = chrome.child(
                    div()
                        .id(("gal-handle", (right as usize) * 2 + bottom as usize))
                        .absolute()
                        .left(px(hx))
                        .top(px(hy))
                        .size(px(9.))
                        .bg(rgb(0xF2EFE6))
                        .border_2()
                        .border_color(rgb(accent))
                        .cursor(if right == bottom {
                            CursorStyle::ResizeUpLeftDownRight
                        } else {
                            CursorStyle::ResizeUpRightDownLeft
                        })
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                                cx.stop_propagation();
                                let Some(start) = this.gal.current.clone() else {
                                    return;
                                };
                                this.gal.drag = None;
                                this.gal.resize = Some(GalResize {
                                    photo: pid,
                                    right,
                                    bottom,
                                    anchor,
                                    start_span: (sx, sy),
                                    start,
                                    last: None,
                                });
                                cx.notify();
                            }),
                        ),
                );
            }
        }
        chrome
    }

    /// Where a drag would land: (top-left cell, span) on the Desktop grid.
    pub(crate) fn gal_drop_target(&self, d: &GalDrag, pos: (f32, f32)) -> Option<(GridCell, u8, u8)> {
        let g = self.gal.current.as_ref()?;
        let b = self.gal.grid_box.get();
        let geom = PageGeom::new(g, Breakpoint::Desktop, 3);
        let (ox, oy) = (b.origin.x.as_f32(), b.origin.y.as_f32());
        let (lx, ly) = (pos.0 - ox, pos.1 - oy);
        let width = b.size.width.as_f32();
        if width <= 1. || lx < -20. || lx > width + 20. || ly < -30. || ly > geom.height + 3. * (geom.row_h + geom.gutter) {
            return None;
        }
        let i = g.index_of(d.photo)?;
        let p = &g.photos[i];
        let cols = g.columns.max(1);
        let (sx, sy) = if p.cell.is_none() {
            layout::template(&g.template).span_for(g.placed_count(), cols)
        } else {
            (p.span_x.min(cols), p.span_y)
        };
        let mut at = geom.cell_at(lx, ly.max(0.));
        if let (false, Some(tile)) = (d.from_tray, p.cell) {
            // Keep the grab point: offset by where inside the tile it began.
            let s = geom.cell_at(d.start.0 - ox, (d.start.1 - oy).max(0.));
            let dc = s.col.saturating_sub(tile.col);
            let dr = s.row.saturating_sub(tile.row);
            at = GridCell {
                col: at.col.saturating_sub(dc),
                row: at.row.saturating_sub(dr),
            };
        }
        at.col = at.col.min(cols.saturating_sub(sx) as u16);
        Some((at, sx, sy))
    }

    pub(crate) fn gal_drop_cell(&self, d: &GalDrag, pos: (f32, f32)) -> Option<GridCell> {
        self.gal_drop_target(d, pos).map(|t| t.0)
    }

    /// Corner-handle resize: the rectangle between the fixed corner and
    /// the pointer's cell, applied from the drag-start model.
    pub(crate) fn gal_resize_to(&mut self, pos: (f32, f32), shift: bool, cx: &mut Context<Self>) {
        let Some(r) = self.gal.resize.as_ref() else {
            return;
        };
        let b = self.gal.grid_box.get();
        let geom = PageGeom::new(&r.start, Breakpoint::Desktop, 3);
        let p = geom.cell_at(pos.0 - b.origin.x.as_f32(), (pos.1 - b.origin.y.as_f32()).max(0.));
        let cols = r.start.columns.max(1) as u16;
        let a = r.anchor;
        let (mut x0, mut x1) = if r.right { (a.col, p.col.max(a.col)) } else { (p.col.min(a.col), a.col) };
        let (mut y0, mut y1) = if r.bottom { (a.row, p.row.max(a.row)) } else { (p.row.min(a.row), a.row) };
        x1 = x1.min(cols - 1);
        x0 = x0.min(x1);
        let mut sx = (x1 - x0 + 1) as u8;
        let mut sy = (y1 - y0 + 1).min(8) as u8;
        if shift {
            let (ssx, ssy) = r.start_span;
            sy = ((sx as f32 * ssy as f32 / ssx.max(1) as f32).round() as u8).clamp(1, 8);
            if r.bottom {
                y1 = y0 + sy as u16 - 1;
            } else {
                y0 = y1 + 1 - sy as u16;
            }
            let _ = y1;
        }
        sx = sx.clamp(1, cols as u8);
        let target = (GridCell { col: x0, row: y0 }, sx, sy);
        if r.last == Some(target) {
            return;
        }
        let (id, first, mut g) = (r.photo, r.last.is_none(), r.start.clone());
        g.place(id, target.0);
        g.set_span(id, sx, sy);
        if first {
            let before = r.start.clone();
            self.gal.history.record(&before, "Resize", None);
        }
        if let Some(r) = self.gal.resize.as_mut() {
            r.last = Some(target);
        }
        self.gal.current = Some(g);
        self.gal_save();
        cx.notify();
    }

    pub(crate) fn gal_slider_to(&mut self, kind: GalSlider, pos: (f32, f32), cx: &mut Context<Self>) {
        let b = match kind {
            GalSlider::TitleSize => self.gal.title_box.get(),
            GalSlider::Radius => self.gal.radius_box.get(),
            GalSlider::Gutter => self.gal.gutter_box.get(),
            GalSlider::Focal => self.gal.focal_box.get(),
        };
        let (w, h) = (b.size.width.as_f32(), b.size.height.as_f32());
        if w <= 1. {
            return;
        }
        let fx = ((pos.0 - b.origin.x.as_f32()) / w).clamp(0., 1.);
        let fy = ((pos.1 - b.origin.y.as_f32()) / h.max(1.)).clamp(0., 1.);
        match kind {
            GalSlider::TitleSize => {
                let v = (32. + fx * 40.).round() as u8;
                self.gal_edit("Title size", Some("title-size"), |g| g.theme.title_size = v, cx);
            }
            GalSlider::Radius => {
                let v = (fx * 24.).round() as u8;
                self.gal_edit("Rounded corners", Some("radius"), |g| g.theme.corner_radius = v, cx);
            }
            GalSlider::Gutter => {
                let v = (fx * 32.).round() as u8;
                self.gal_edit("Gutter", Some("gutter"), |g| g.gutter = v, cx);
            }
            GalSlider::Focal => {
                let Some(id) = self.gal.selected else {
                    return;
                };
                let (fx, fy) = ((fx * 100.).round() / 100., (fy * 100.).round() / 100.);
                self.gal_edit("Focal point", Some("focal"), |g| {
                    if let Some(i) = g.index_of(id) {
                        g.photos[i].focal = (fx, fy);
                    }
                }, cx);
            }
        }
    }
}

fn accent_wash_hsla() -> Hsla {
    let mut c: Hsla = rgb(accent_line()).into();
    c.a = 0.14;
    c
}
