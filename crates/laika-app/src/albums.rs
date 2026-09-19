//! V30: every collection is an album — a custom order (the Album sort,
//! drag to reorder in Grid), a cover, a gallery title and description,
//! and per-photo captions that live in the album, not on the photo.

use std::collections::HashMap;

use laika_core::state::SortField;

use super::*;

impl Laika {
    /// Album order for a collection (cached until membership or order
    /// changes; `load_collections` drops it).
    pub(crate) fn album_order_of(&self, cid: i64) -> Vec<i64> {
        let mut cache = self.coll.order.borrow_mut();
        if cache.as_ref().is_none_or(|(id, _)| *id != cid) {
            let order = self
                .catalog
                .as_ref()
                .map(|c| c.album_order(cid))
                .unwrap_or_default();
            *cache = Some((cid, order));
        }
        cache.as_ref().map(|(_, o)| o.clone()).unwrap_or_default()
    }

    fn album_captions_of(&self, cid: i64) -> HashMap<i64, String> {
        let mut cache = self.coll.captions.borrow_mut();
        if cache.as_ref().is_none_or(|(id, _)| *id != cid) {
            let caps = self
                .catalog
                .as_ref()
                .map(|c| c.album_captions(cid))
                .unwrap_or_default();
            *cache = Some((cid, caps));
        }
        cache.as_ref().map(|(_, c)| c.clone()).unwrap_or_default()
    }

    /// The album caption for a photo in the collection being viewed.
    pub(crate) fn album_caption(&self, pid: i64) -> Option<String> {
        let cid = self.state.filters.collection?;
        self.album_captions_of(cid).get(&pid).cloned()
    }

    /// The viewed collection, when Grid shows it in album order — the only
    /// state where dragging reorders.
    pub(crate) fn album_reorder_target(&self) -> Option<i64> {
        let f = &self.state.filters;
        (self.state.active_module == Module::Library
            && self.view == ViewMode::Grid
            && f.sort.field == SortField::Album
            && f.sort.dir == laika_core::state::SortDir::Asc)
            .then_some(f.collection)
            .flatten()
    }

    /// Selecting a collection in the rail shows it in album order.
    pub(crate) fn enter_album_sort(&mut self) {
        self.state.filters.sort.field = SortField::Album;
        self.state.filters.sort.dir = laika_core::state::SortDir::Asc;
    }

    /// The grid cell under a window point (index into the visible cells);
    /// past the last cell reads as the last cell (drop at the end).
    pub(crate) fn grid_insert_index(&self, pos: (f32, f32)) -> Option<usize> {
        let b = self.grid_box.get();
        let (bw, bh) = (b.size.width.as_f32(), b.size.height.as_f32());
        let (lx, ly) = (pos.0 - b.origin.x.as_f32(), pos.1 - b.origin.y.as_f32());
        if bw <= 1. || lx < 0. || ly < 0. || lx > bw || ly > bh {
            return None;
        }
        let count = self.grid_photos().len();
        if count == 0 {
            return None;
        }
        let (cols, _, pitch, row_h, scroll) = self.grid_metrics();
        let x = lx - Self::GRID_PAD;
        let y = ly - Self::GRID_PAD - scroll;
        let row = (y / row_h).floor().max(0.) as usize;
        let col = (x / pitch).floor().clamp(0., cols as f32 - 1.) as usize;
        Some((row * cols + col).min(count - 1))
    }

    /// (columns, cell width, cell pitch, row height, scroll offset).
    fn grid_metrics(&self) -> (usize, f32, f32, f32, f32) {
        let bw = self.grid_box.get().size.width.as_f32();
        let cols = self.state.thumb_columns.max(3) as usize;
        let inner = (bw - 2. * Self::GRID_PAD).max(cols as f32);
        let cell_w = ((inner - Self::GRID_GAP * (cols as f32 - 1.)) / cols as f32).max(1.);
        let scroll = self.list_handle.0.borrow().base_handle.offset().y.as_f32();
        (
            cols,
            cell_w,
            cell_w + Self::GRID_GAP,
            self.grid_row_height(),
            scroll,
        )
    }

    /// Photos being dragged (pairs together) and where they would go when
    /// dropped on cell `target`: (forward, insert-before id).
    fn reorder_plan(&self, target: usize) -> Option<(Vec<i64>, bool, Option<i64>)> {
        let (dragged, _) = self.drag_from.as_ref()?;
        let moving = self.expand_pair_targets(dragged);
        let cells: Vec<i64> = self.grid_photos().iter().map(|p| p.id).collect();
        let (forward, before) = laika_core::album::drop_before(&cells, &moving, target)?;
        Some((moving, forward, before))
    }

    /// The accent bar beside the target cell: on its right when the drag
    /// moves forward (photos land after it), on its left when backward.
    pub(crate) fn album_insert_marker(&self) -> Option<Div> {
        self.album_reorder_target()?;
        let target = self.coll.drop_at.get()?;
        let (_, forward, _) = self.reorder_plan(target)?;
        if self.grid_box.get().size.width.as_f32() <= 1. {
            return None;
        }
        let (cols, cell_w, pitch, row_h, scroll) = self.grid_metrics();
        let (row, col) = (target / cols, target % cols);
        let left = Self::GRID_PAD + col as f32 * pitch;
        let x = if forward {
            left + cell_w + Self::GRID_GAP / 2.
        } else {
            left - Self::GRID_GAP / 2.
        };
        let y = Self::GRID_PAD + row as f32 * row_h + scroll;
        Some(
            div()
                .absolute()
                .left(px(x - 1.5))
                .top(px(y))
                .w(px(3.))
                .h(px((row_h - Self::GRID_GAP).max(8.)))
                .rounded(px(2.))
                .bg(rgb(accent_line())),
        )
    }

    /// Drop a reorder drag: the dragged photos take the target cell's slot.
    pub(crate) fn drop_album_reorder(&mut self, dragged: Vec<i64>, cx: &mut Context<Self>) {
        let Some(cid) = self.album_reorder_target() else {
            return;
        };
        let Some(target) = self.coll.drop_at.take() else {
            return;
        };
        let moving = self.expand_pair_targets(&dragged);
        let cells: Vec<i64> = self.grid_photos().iter().map(|p| p.id).collect();
        let Some((_, before)) = laika_core::album::drop_before(&cells, &moving, target) else {
            return;
        };
        let Some(cat) = self.catalog.as_ref() else {
            return;
        };
        match cat.move_in_album(cid, &moving, before) {
            Ok(_) => {
                self.status_note = format!(
                    "moved {} photo{}",
                    moving.len(),
                    if moving.len() == 1 { "" } else { "s" }
                );
            }
            Err(e) => self.status_note = e,
        }
        self.coll.order.replace(None);
        self.photos_rev.set(self.photos_rev.get().wrapping_add(1));
        cx.notify();
    }

    // ---- album panel ---------------------------------------------------------------

    /// Title, description, cover and order controls for the viewed album.
    pub(crate) fn album_panel(
        &self,
        c: &laika_core::catalog::Collection,
        cx: &mut Context<Self>,
    ) -> Div {
        let cid = c.id;
        let cover_thumb = c
            .cover
            .and_then(|pid| self.thumbs.get(&pid))
            .map(|t| t.image.clone());
        let in_album_sort = self.state.filters.sort.field == SortField::Album;
        let button = |id: &'static str, label: &'static str, tip: &'static str| {
            div()
                .id(id)
                .px(px(7.))
                .py(px(3.))
                .rounded(px(3.))
                .border_1()
                .border_color(border_control())
                .font_family(SANS)
                .text_size(sp(10.))
                .text_color(rgb(TEXT_SECONDARY))
                .hover(|s| s.bg(rgb(bg_row_hover())))
                .on_hover(self.tip(tip))
                .child(label)
        };
        let label = |t: &'static str| {
            div()
                .w(px(62.))
                .flex_none()
                .font_family(SANS)
                .text_size(sp(10.5))
                .text_color(rgb(TEXT_DIM))
                .child(t)
        };
        let text = |v: &str, placeholder: &str| {
            div()
                .font_family(SANS)
                .text_size(sp(10.5))
                .text_color(rgb(if v.is_empty() {
                    TEXT_DIMMER
                } else {
                    TEXT_SECONDARY
                }))
                .child(if v.is_empty() {
                    placeholder.to_string()
                } else {
                    v.to_string()
                })
        };
        div()
            .flex()
            .flex_col()
            .gap(px(5.))
            .px(px(14.))
            .pt(px(8.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(label("Title"))
                    .child(self.field_cell(
                        text_input::FieldId::AlbumTitle,
                        text(&c.title, &c.name),
                        false,
                        "Gallery title (empty uses the collection name)",
                        cx,
                    )),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(label("About"))
                    .child(self.field_cell(
                        text_input::FieldId::AlbumDescription,
                        text(&c.description, "Description…"),
                        false,
                        "Album description (Enter applies)",
                        cx,
                    )),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(label("Cover"))
                    .child(match cover_thumb {
                        Some(image) => div()
                            .w(px(42.))
                            .h(px(28.))
                            .rounded(px(2.))
                            .overflow_hidden()
                            .child(
                                img(ImageSource::Render(image))
                                    .size_full()
                                    .object_fit(ObjectFit::Cover),
                            )
                            .into_any_element(),
                        None => text(
                            "",
                            if c.cover.is_some() {
                                "(loading)"
                            } else {
                                "first photo"
                            },
                        )
                        .into_any_element(),
                    })
                    .child(
                        button(
                            "album-cover",
                            "Use selected",
                            "Make the selected photo the cover",
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let Some(pid) = this.state.primary else {
                                this.status_note = "select a photo for the cover".to_string();
                                cx.notify();
                                return;
                            };
                            if !this.album_order_of(cid).contains(&pid) {
                                this.status_note = "the cover must be in this album".to_string();
                                cx.notify();
                                return;
                            }
                            if let Some(cat) = this.catalog.as_ref() {
                                this.status_note = match cat.set_album_cover(cid, Some(pid)) {
                                    Ok(()) => "cover set".to_string(),
                                    Err(e) => e,
                                };
                            }
                            this.load_collections();
                            cx.notify();
                        })),
                    )
                    .when(c.cover.is_some(), |d| {
                        d.child(
                            button("album-cover-clear", "×", "Use the first photo as the cover")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Some(cat) = this.catalog.as_ref() {
                                        cat.set_album_cover(cid, None).ok();
                                    }
                                    this.load_collections();
                                    cx.notify();
                                })),
                        )
                    }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(label("Order"))
                    .child(if in_album_sort {
                        text("album order · drag photos to reorder", "").into_any_element()
                    } else {
                        button(
                            "album-sort",
                            "Sort by album order",
                            "Show this album in its custom order",
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.enter_album_sort();
                            cx.notify();
                        }))
                        .into_any_element()
                    }),
            )
    }

    /// Commit the album title / description fields.
    pub(crate) fn commit_album_text(
        &mut self,
        title: Option<&str>,
        description: Option<&str>,
    ) -> Result<(), String> {
        let cid = self
            .state
            .filters
            .collection
            .ok_or_else(|| "open an album first".to_string())?;
        let cur = self
            .coll
            .list
            .iter()
            .find(|c| c.id == cid)
            .cloned()
            .ok_or_else(|| "that album no longer exists".to_string())?;
        let cat = self.catalog.as_ref().ok_or("no catalog is open")?;
        cat.set_album_text(
            cid,
            title.unwrap_or(&cur.title),
            description.unwrap_or(&cur.description),
        )?;
        self.load_collections();
        Ok(())
    }

    // ---- album captions ------------------------------------------------------------

    /// Right-rail row: the album caption for the scope, `<mixed>` when the
    /// scope disagrees. Only while viewing an album.
    pub(crate) fn album_caption_row(&self, cx: &mut Context<Self>) -> Option<Div> {
        let cid = self.state.filters.collection?;
        let scope = self.meta_scope();
        if scope.is_empty() {
            return None;
        }
        let caps = self.album_captions_of(cid);
        let order = self.album_order_of(cid);
        let members: Vec<i64> = scope
            .iter()
            .copied()
            .filter(|id| order.contains(id))
            .collect();
        if members.is_empty() {
            return None;
        }
        let first = caps.get(&members[0]).cloned().unwrap_or_default();
        let value = if members
            .iter()
            .all(|id| caps.get(id).cloned().unwrap_or_default() == first)
        {
            first
        } else {
            "<mixed>".to_string()
        };
        let copyable = !value.is_empty() && value != "<mixed>";
        Some(
            div()
                .flex()
                .flex_col()
                .gap(px(3.))
                .child(self.meta_field_row_with(
                    "In album",
                    text_input::FieldId::AlbumCaption,
                    value,
                    "Caption shown for this photo in the album only (Enter applies)",
                    cx,
                ))
                .when(copyable, |d| {
                    d.child(
                        div().pl(px(72.)).child(
                            div()
                                .id("album-caption-to-title")
                                .font_family(SANS)
                                .text_size(sp(10.))
                                .text_color(rgb(TEXT_DIM))
                                .hover(|s| s.text_color(rgb(TEXT_SECONDARY)))
                                .on_hover(self.tip(
                                    "Also set it as the photo's own title (sidecars and every view)",
                                ))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    let Some(cid) = this.state.filters.collection else {
                                        return;
                                    };
                                    let caps = this.album_captions_of(cid);
                                    let scope = this.meta_scope();
                                    let text = scope
                                        .iter()
                                        .find_map(|id| caps.get(id).cloned())
                                        .unwrap_or_default();
                                    if text.is_empty() {
                                        return;
                                    }
                                    if let Err(e) =
                                        this.commit_meta_field(&text, "title", |p, v| p.title = Some(v), cx)
                                    {
                                        this.status_note = e;
                                    }
                                    cx.notify();
                                }))
                                .child("use as photo title"),
                        ),
                    )
                }),
        )
    }

    /// Commit an album caption over the scope's members. In a batch an
    /// empty value leaves captions unchanged (the metadata rule).
    pub(crate) fn commit_album_caption(&mut self, buf: &str) -> Result<(), String> {
        let cid = self
            .state
            .filters
            .collection
            .ok_or_else(|| "open an album first".to_string())?;
        if buf.trim() == "<mixed>" {
            return Ok(());
        }
        let order = self.album_order_of(cid);
        let members: Vec<i64> = self
            .meta_scope()
            .into_iter()
            .filter(|id| order.contains(id))
            .collect();
        if members.is_empty() {
            return Err("select photos in this album".to_string());
        }
        if members.len() > 1 && buf.trim().is_empty() {
            return Ok(());
        }
        let cat = self.catalog.as_ref().ok_or("no catalog is open")?;
        for pid in &members {
            cat.set_album_caption(cid, *pid, buf)?;
        }
        self.coll.captions.replace(None);
        self.status_note = format!(
            "album caption set on {} photo{}",
            members.len(),
            if members.len() == 1 { "" } else { "s" }
        );
        Ok(())
    }
}
