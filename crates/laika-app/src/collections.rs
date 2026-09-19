//! V13: color labels (keys 6–9, menu for all five), label filter chips,
//! editable label names, the Quick Collection (B), a target collection
//! that B adds to, and saved collections in the left rail.

use std::collections::{HashMap, HashSet};

use laika_core::catalog::{Collection, PhotoStack};
use laika_core::labels::{self, LabelNames};

use super::*;

/// What the collection name field does when committed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum NameMode {
    #[default]
    Closed,
    /// New collection (with the targeted photos, if any).
    New,
    /// Save the currently active filters as a dynamic collection.
    NewSmart,
    /// Save the Quick Collection under a name (then clear it).
    SaveQuick,
    Rename(i64),
}

#[derive(Default)]
pub(crate) struct CollState {
    pub names: LabelNames,
    pub list: Vec<Collection>,
    pub quick: Option<i64>,
    /// Where `B` adds; None = the Quick Collection.
    pub target: Option<i64>,
    pub quick_members: HashSet<i64>,
    pub stacks: Vec<PhotoStack>,
    pub stack_members: HashMap<i64, Vec<i64>>,
    pub stack_by_photo: HashMap<i64, i64>,
    /// Filtered collection's members (lazy; dropped on any change).
    pub members: RefCell<Option<(i64, HashSet<i64>)>>,
    pub name_mode: NameMode,
    /// Last typed name (committed by Enter).
    pub name_draft: String,
    /// V30: cached album order / captions for the viewed collection.
    pub order: RefCell<Option<(i64, Vec<i64>)>>,
    pub captions: RefCell<Option<(i64, HashMap<i64, String>)>>,
    /// V30: grid insertion slot while a reorder drag hovers.
    pub drop_at: Rc<Cell<Option<usize>>>,
    /// Collection rows' visible window bounds, stamped with the frame
    /// they were painted in (photo drops hit-test against the latest).
    pub row_bounds: Rc<RefCell<HashMap<i64, (u64, Bounds<Pixels>)>>>,
    /// The collection row a photo drag is over.
    pub drop_hover: Rc<Cell<Option<i64>>>,
    /// Photo context menu: the "Add to Collection" submenu is open.
    pub menu_open: bool,
}

/// A round color swatch for a label (hollow ring for "no label").
pub(crate) fn label_swatch(label: u8, size: f32) -> Div {
    let d = div().size(px(size)).rounded_full().flex_none();
    match label {
        1..=5 => d.bg(rgb(labels::COLORS[label as usize - 1])),
        _ => d.border_1().border_color(rgba(0xFFFFFF66)),
    }
}

impl Laika {
    /// Reload names, collections and the target for the open catalog.
    pub(crate) fn load_collections(&mut self) {
        let Some(cat) = self.catalog.as_ref() else {
            self.coll = CollState::default();
            return;
        };
        self.coll.names = cat.label_names();
        self.coll.quick = cat.quick_collection_id().ok();
        self.coll.list = cat.collections();
        self.coll.target = cat
            .get_import_default("target_collection")
            .parse::<i64>()
            .ok()
            .filter(|id| {
                self.coll
                    .list
                    .iter()
                    .any(|c| c.id == *id && !c.quick && !c.smart)
            });
        self.coll.quick_members = self
            .coll
            .quick
            .map(|q| cat.collection_members(q))
            .unwrap_or_default();
        self.coll.stacks = cat.photo_stacks();
        self.coll.stack_members.clear();
        self.coll.stack_by_photo.clear();
        for stack in &self.coll.stacks {
            let members = cat.stack_members(stack.id);
            for photo_id in &members {
                self.coll.stack_by_photo.insert(*photo_id, stack.id);
            }
            self.coll.stack_members.insert(stack.id, members);
        }
        self.coll.members.replace(None);
        self.coll.order.replace(None);
        self.coll.captions.replace(None);
        // A filter on a collection that no longer exists would hide
        // everything with no way to see why.
        if let Some(cid) = self.state.filters.collection {
            if !self.coll.list.iter().any(|c| c.id == cid) {
                self.state.filters.collection = None;
                self.state.filters.collection_name.clear();
            }
        }
        if let Some(cid) = self.state.filters.smart_collection {
            if !self.coll.list.iter().any(|c| c.id == cid && c.smart) {
                self.state.filters.smart_collection = None;
                self.state.filters.smart_collection_name.clear();
            }
        }
        // G05: the galleries list follows the open catalog.
        self.load_galleries();
    }

    /// The collection row under a window position during a photo drag.
    pub(crate) fn collection_at(&self, pos: (f32, f32)) -> Option<i64> {
        let map = self.coll.row_bounds.borrow();
        let latest = map.values().map(|(g, _)| *g).max()?;
        map.iter()
            .filter(|(_, (g, _))| *g == latest)
            .find(|(_, (_, b))| {
                let (x, y) = (b.origin.x.as_f32(), b.origin.y.as_f32());
                pos.0 >= x
                    && pos.1 >= y
                    && pos.0 <= x + b.size.width.as_f32()
                    && pos.1 <= y + b.size.height.as_f32()
            })
            .map(|(id, _)| *id)
    }

    /// Photos dropped on a collection row join it (pairs travel together).
    pub(crate) fn drop_photos_on_collection(
        &mut self,
        ids: Vec<i64>,
        cid: i64,
        cx: &mut Context<Self>,
    ) {
        if self.coll.list.iter().any(|c| c.id == cid && c.smart) {
            self.status_note = "smart collections update from their criteria".to_string();
            cx.notify();
            return;
        }
        let ids = self.expand_pair_targets(&ids);
        let Some(cat) = self.catalog.as_ref() else {
            return;
        };
        let name = self
            .coll
            .list
            .iter()
            .find(|c| c.id == cid)
            .map(|c| c.name.clone())
            .unwrap_or_default();
        self.status_note = match cat.add_to_collection(cid, &ids) {
            Ok(0) => format!("already in {name}"),
            Ok(n) => format!("added {n} photo{} to {name}", if n == 1 { "" } else { "s" }),
            Err(e) => e,
        };
        self.refresh_collections(cx);
    }

    /// Context menu: add the targets to a collection, or take them out
    /// when every target is already in it.
    pub(crate) fn toggle_targets_in(&mut self, cid: i64, cx: &mut Context<Self>) {
        let ids = self.expand_pair_targets(&self.targets());
        if !ids.is_empty() && ids.iter().all(|pid| self.in_collection(cid, *pid)) {
            self.remove_targets_from(cid, cx);
        } else {
            self.add_targets_to(cid, cx);
        }
    }

    /// Context menu entry point for a new collection from the targets.
    pub(crate) fn new_collection_from_menu(&mut self, cx: &mut Context<Self>) {
        self.open_name_field(NameMode::New, cx);
    }

    fn refresh_collections(&mut self, cx: &mut Context<Self>) {
        self.load_collections();
        // Membership feeds the filtered order and timeline memos.
        self.photos_rev.set(self.photos_rev.get().wrapping_add(1));
        cx.notify();
    }

    /// The collection the library is filtered to.
    pub(crate) fn viewed_collection(&self) -> Option<&Collection> {
        let id = self.state.filters.collection?;
        self.coll.list.iter().find(|c| c.id == id)
    }

    pub(crate) fn target_collection(&self) -> Option<&Collection> {
        let id = self.coll.target.or(self.coll.quick)?;
        self.coll.list.iter().find(|c| c.id == id && !c.smart)
    }

    pub(crate) fn stack_for_photo(&self, photo_id: i64) -> Option<&PhotoStack> {
        let id = self.coll.stack_by_photo.get(&photo_id)?;
        self.coll.stacks.iter().find(|s| s.id == *id)
    }

    pub(crate) fn collapsed_stack_hidden(&self) -> HashSet<i64> {
        let mut hidden = HashSet::new();
        for stack in self.coll.stacks.iter().filter(|s| s.collapsed) {
            if let Some(members) = self.coll.stack_members.get(&stack.id) {
                hidden.extend(members.iter().copied().filter(|id| *id != stack.cover));
            }
        }
        hidden
    }

    fn smart_count(&self, collection: &Collection) -> usize {
        serde_json::from_str::<laika_core::state::Filters>(&collection.criteria_json)
            .ok()
            .map(|filters| {
                self.photos
                    .iter()
                    .filter(|photo| self.photo_matches_filters(&filters, photo))
                    .count()
            })
            .unwrap_or(0)
    }

    /// Membership test for the collection filter.
    pub(crate) fn in_collection(&self, cid: i64, pid: i64) -> bool {
        if Some(cid) == self.coll.quick {
            return self.coll.quick_members.contains(&pid);
        }
        let mut cache = self.coll.members.borrow_mut();
        if cache.as_ref().is_none_or(|(id, _)| *id != cid) {
            let members = self
                .catalog
                .as_ref()
                .map(|c| c.collection_members(cid))
                .unwrap_or_default();
            *cache = Some((cid, members));
        }
        cache.as_ref().is_some_and(|(_, m)| m.contains(&pid))
    }

    // ---- labels ------------------------------------------------------------------

    /// Set a label on the targets (pairs included). When every target
    /// already carries it, the label clears instead (Lightroom's toggle).
    pub(crate) fn apply_label(&mut self, label: u8, cx: &mut Context<Self>) {
        let anchor = self.state.primary;
        let anchor_idx = anchor.and_then(|id| self.ordered_ids().iter().position(|&x| x == id));
        let ids = self.expand_pair_targets(&self.targets());
        if ids.is_empty() {
            self.status_note = "no visible photos targeted".to_string();
            cx.notify();
            return;
        }
        let label = label.min(5);
        let all_have = ids
            .iter()
            .all(|id| self.find(*id).is_some_and(|p| p.label == label));
        let value = if all_have { 0 } else { label };
        let befores: Vec<(i64, edit::Snap)> = ids
            .iter()
            .map(|pid| (*pid, self.snap_current(*pid)))
            .collect();
        for p in self.photos.iter_mut().filter(|p| ids.contains(&p.id)) {
            p.label = value;
        }
        let name = if value == 0 {
            "none".to_string()
        } else {
            self.coll.names.name(value).to_string()
        };
        for (pid, before) in &befores {
            self.record_step(*pid, "Label", &name, before.clone());
        }
        self.last_batch = ids.clone();
        self.last_was_meta = false;
        self.last_was_remove = false;
        self.redo_batch.clear();
        let mut failed = 0;
        for id in &ids {
            if !self.persist_rating(*id) {
                failed += 1;
            }
        }
        if failed > 0 {
            self.status_note = self
                .save_error
                .clone()
                .unwrap_or_else(|| "save failed".to_string());
        } else {
            self.status_note = if value == 0 {
                format!("label cleared on {}", ids.len())
            } else {
                format!("labeled {} {}", ids.len(), name)
            };
        }
        // xmp:Label converges like ratings.
        for pid in &ids {
            self.request_sidecar(*pid, false);
        }
        if !self.sidecar_pending.is_empty() {
            self.kick_save_timer(cx);
        }
        self.state.photos = self.photos.iter().map(as_photo).collect();
        self.settle_after_cull(anchor, anchor_idx, cx);
        cx.notify();
    }

    /// Rename a label; photos carrying it rewrite their sidecars so
    /// `xmp:Label` follows.
    pub(crate) fn commit_label_name(
        &mut self,
        label: u8,
        buf: &str,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let mut names = self.coll.names.clone();
        names.set(label, buf)?;
        if names == self.coll.names {
            return Ok(());
        }
        if let Some(cat) = self.catalog.as_ref() {
            cat.set_label_names(&names);
        }
        self.coll.names = names;
        let affected: Vec<i64> = self
            .photos
            .iter()
            .filter(|p| p.label == label)
            .map(|p| p.id)
            .collect();
        for pid in &affected {
            self.request_sidecar(*pid, false);
        }
        if !self.sidecar_pending.is_empty() {
            self.kick_save_timer(cx);
        }
        cx.notify();
        Ok(())
    }

    /// Label filter chips: "none" ring + five swatches, multi-select.
    pub(crate) fn label_filter_chips(&self, cx: &mut Context<Self>) -> Div {
        let mask = self.state.filters.labels;
        let mut row = div().flex().items_center().gap(px(3.));
        for label in 0..=5u8 {
            let on = mask & (1 << label) != 0;
            let tip: &'static str = match label {
                0 => "Show photos with no color label",
                1 => "Show red-labeled photos (6)",
                2 => "Show yellow-labeled photos (7)",
                3 => "Show green-labeled photos (8)",
                4 => "Show blue-labeled photos (9)",
                _ => "Show purple-labeled photos",
            };
            row = row.child(
                div()
                    .id(("label-filter", label as usize))
                    .size(px(20.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(4.))
                    .border_1()
                    .border_color::<Hsla>(if on {
                        rgb(TEXT_PRIMARY).into()
                    } else {
                        gpui_kit::transparent_black()
                    })
                    .hover(|s| s.bg(rgb(bg_row_hover())))
                    .on_hover(self.tip(tip))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.state.filters.labels ^= 1 << label;
                        cx.notify();
                    }))
                    .child(label_swatch(label, 10.)),
            );
        }
        row
    }

    /// Context-menu row: all five labels plus clear, for the target.
    pub(crate) fn label_menu_row(&self, pid: i64, cx: &mut Context<Self>) -> Div {
        let current = self.find(pid).map(|p| p.label).unwrap_or(0);
        let mut row = div()
            .flex()
            .items_center()
            .gap(px(2.))
            .px(px(8.))
            .py(px(4.));
        for label in 0..=5u8 {
            let on = current == label;
            row = row.child(
                div()
                    .id(("ctx-label", label as usize))
                    .size(px(26.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(3.))
                    .when(on, |d| d.bg(rgb(bg_segment_active())))
                    .hover(|s| s.bg(rgb(bg_row_hover())))
                    .on_hover(self.tip(if label == 0 {
                        "Clear label"
                    } else {
                        "Set color label"
                    }))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.close_context_menu();
                        this.menu_target(pid, cx);
                        if label == 0 {
                            // Clearing is never a toggle.
                            let cur = this.find(pid).map(|p| p.label).unwrap_or(0);
                            if cur != 0 {
                                this.apply_label(cur, cx);
                            }
                        } else {
                            this.apply_label(label, cx);
                        }
                        cx.notify();
                    }))
                    .child(label_swatch(label, 12.)),
            );
        }
        row
    }

    // ---- collections ---------------------------------------------------------------

    /// `B`: toggle the targets in the target collection (Quick Collection
    /// unless another is chosen). Adds when any target is missing.
    pub(crate) fn toggle_in_target(&mut self, cx: &mut Context<Self>) {
        let Some(target) = self.target_collection().cloned() else {
            self.status_note = "no catalog is open".to_string();
            cx.notify();
            return;
        };
        let ids = self.expand_pair_targets(&self.targets());
        if ids.is_empty() {
            self.status_note = "no visible photos targeted".to_string();
            cx.notify();
            return;
        }
        let members = if target.quick {
            self.coll.quick_members.clone()
        } else {
            self.catalog
                .as_ref()
                .map(|c| c.collection_members(target.id))
                .unwrap_or_default()
        };
        let all_in = ids.iter().all(|id| members.contains(id));
        let Some(cat) = self.catalog.as_ref() else {
            return;
        };
        let result = if all_in {
            cat.remove_from_collection(target.id, &ids)
                .map(|n| format!("removed {n} from {}", target.name))
        } else {
            cat.add_to_collection(target.id, &ids)
                .map(|n| format!("added {n} to {}", target.name))
        };
        self.status_note = match result {
            Ok(note) => note,
            Err(e) => e,
        };
        self.refresh_collections(cx);
    }

    /// Context menu: add the targets to a specific collection.
    pub(crate) fn add_targets_to(&mut self, cid: i64, cx: &mut Context<Self>) {
        let ids = self.expand_pair_targets(&self.targets());
        let Some(cat) = self.catalog.as_ref() else {
            return;
        };
        let name = self
            .coll
            .list
            .iter()
            .find(|c| c.id == cid)
            .map(|c| c.name.clone())
            .unwrap_or_default();
        self.status_note = match cat.add_to_collection(cid, &ids) {
            Ok(n) => format!("added {n} to {name}"),
            Err(e) => e,
        };
        self.refresh_collections(cx);
    }

    /// Remove the targets from the collection being viewed.
    fn remove_targets_from(&mut self, cid: i64, cx: &mut Context<Self>) {
        let ids = self.expand_pair_targets(&self.targets());
        let Some(cat) = self.catalog.as_ref() else {
            return;
        };
        self.status_note = match cat.remove_from_collection(cid, &ids) {
            Ok(n) => format!("removed {n} from this collection"),
            Err(e) => e,
        };
        self.refresh_collections(cx);
    }

    fn set_target_collection(&mut self, cid: Option<i64>, cx: &mut Context<Self>) {
        self.coll.target = cid.filter(|id| {
            Some(*id) != self.coll.quick && self.coll.list.iter().any(|c| c.id == *id && !c.smart)
        });
        if let Some(cat) = self.catalog.as_ref() {
            cat.set_import_default(
                "target_collection",
                &self
                    .coll
                    .target
                    .map(|id| id.to_string())
                    .unwrap_or_default(),
            );
        }
        let name = self
            .target_collection()
            .map(|c| c.name.clone())
            .unwrap_or_default();
        self.status_note = format!("B now adds to {name}");
        cx.notify();
    }

    /// Commit the collection name field for the current mode.
    pub(crate) fn commit_collection_name(
        &mut self,
        buf: &str,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let mode = self.coll.name_mode;
        let Some(cat) = self.catalog.as_ref() else {
            return Err("no catalog is open".to_string());
        };
        let note = match mode {
            NameMode::Closed => return Ok(()),
            NameMode::New => {
                let id = cat.create_collection(buf)?;
                let ids = self.expand_pair_targets(&self.targets());
                let n = if ids.is_empty() {
                    0
                } else {
                    cat.add_to_collection(id, &ids)?
                };
                format!("created {} with {n} photos", buf.trim())
            }
            NameMode::NewSmart => {
                let mut criteria = self.state.filters.clone();
                criteria.smart_collection = None;
                criteria.smart_collection_name.clear();
                let json = serde_json::to_string(&criteria)
                    .map_err(|e| format!("save smart collection: {e}"))?;
                cat.create_smart_collection(buf, &json)?;
                format!("saved smart collection {}", buf.trim())
            }
            NameMode::SaveQuick => {
                let id = cat.save_quick_collection(buf)?;
                let n = cat.collection_members(id).len();
                format!(
                    "saved {n} photos as {} · Quick Collection cleared",
                    buf.trim()
                )
            }
            NameMode::Rename(id) => {
                cat.rename_collection(id, buf)?;
                if self.state.filters.collection == Some(id) {
                    self.state.filters.collection_name = buf.trim().to_string();
                }
                format!("renamed to {}", buf.trim())
            }
        };
        self.coll.name_mode = NameMode::Closed;
        self.status_note = note;
        self.refresh_collections(cx);
        Ok(())
    }

    pub(crate) fn open_name_field(&mut self, mode: NameMode, cx: &mut Context<Self>) {
        self.coll.name_mode = mode;
        self.coll.name_draft.clear();
        self.focus_field(text_input::FieldId::CollectionName, cx);
        cx.notify();
    }

    pub(crate) fn create_stack_from_selection(&mut self, cx: &mut Context<Self>) {
        let selected: HashSet<i64> = self.targets().into_iter().collect();
        let ids: Vec<i64> = self
            .ordered_ids()
            .into_iter()
            .filter(|id| selected.contains(id))
            .collect();
        let Some(cat) = self.catalog.as_ref() else {
            return;
        };
        self.status_note = match cat.create_stack(&ids) {
            Ok(_) => format!("stacked {} photos", ids.len()),
            Err(e) => e,
        };
        self.refresh_collections(cx);
    }

    pub(crate) fn toggle_primary_stack(&mut self, cx: &mut Context<Self>) {
        let Some(photo_id) = self.state.primary else {
            self.status_note = "select a stacked photo first".to_string();
            cx.notify();
            return;
        };
        let Some(stack) = self.stack_for_photo(photo_id).cloned() else {
            self.status_note = "the selected photo is not in a stack".to_string();
            cx.notify();
            return;
        };
        if let Some(cat) = self.catalog.as_ref() {
            self.status_note = match cat.set_stack_collapsed(stack.id, !stack.collapsed) {
                Ok(()) => if stack.collapsed {
                    "stack expanded"
                } else {
                    "stack collapsed"
                }
                .to_string(),
                Err(e) => e,
            };
        }
        self.refresh_collections(cx);
    }

    pub(crate) fn unstack_primary(&mut self, cx: &mut Context<Self>) {
        let Some(photo_id) = self.state.primary else {
            self.status_note = "select a stacked photo first".to_string();
            cx.notify();
            return;
        };
        if let Some(cat) = self.catalog.as_ref() {
            self.status_note = match cat.unstack_photo(photo_id) {
                Ok(0) => "the selected photo is not in a stack".to_string(),
                Ok(n) => format!("unstacked {n} photos; originals were not changed"),
                Err(e) => e,
            };
        }
        self.refresh_collections(cx);
    }

    pub(crate) fn stack_raw_jpeg_pairs(&mut self, cx: &mut Context<Self>) {
        let pairs = self.pair_rows();
        let mut made = 0;
        let mut skipped = 0;
        if let Some(cat) = self.catalog.as_ref() {
            for pair in pairs {
                if self.coll.stack_by_photo.contains_key(&pair.raw_id)
                    || self.coll.stack_by_photo.contains_key(&pair.jpeg_id)
                {
                    skipped += 1;
                    continue;
                }
                if cat.create_stack(&[pair.raw_id, pair.jpeg_id]).is_ok() {
                    made += 1;
                }
            }
        }
        self.status_note = format!(
            "created {made} RAW/JPEG stack{}{}",
            if made == 1 { "" } else { "s" },
            if skipped > 0 {
                format!(" · skipped {skipped} already stacked")
            } else {
                String::new()
            }
        );
        self.refresh_collections(cx);
    }

    fn stacks_section(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let primary_stack = self.state.primary.and_then(|id| self.stack_for_photo(id));
        let count: usize = self.coll.stacks.iter().map(|s| s.count).sum();
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
        div()
            .flex()
            .flex_col()
            .child(section_header::section_header(
                &format!("Stacks · {} / {count}", self.coll.stacks.len()),
                primary_stack.is_some(),
                window,
            ))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(4.))
                    .px(px(14.))
                    .child(
                        button("stack-selected", "Stack selected", "Group selected photos")
                            .on_click(
                                cx.listener(|this, _, _, cx| this.create_stack_from_selection(cx)),
                            ),
                    )
                    .child(
                        button(
                            "stack-toggle",
                            if primary_stack.is_some_and(|s| s.collapsed) {
                                "Expand"
                            } else {
                                "Collapse"
                            },
                            "Expand or collapse the selected photo's stack",
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.toggle_primary_stack(cx))),
                    )
                    .child(
                        button("stack-remove", "Unstack", "Remove only the stack grouping")
                            .on_click(cx.listener(|this, _, _, cx| this.unstack_primary(cx))),
                    )
                    .child(
                        button(
                            "stack-pairs",
                            "RAW/JPEG pairs",
                            "Create stacks from detected pairs",
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.stack_raw_jpeg_pairs(cx))),
                    ),
            )
    }

    /// Left rail: Quick Collection, saved collections, the target marker,
    /// and actions for the collection being viewed.
    pub(crate) fn collections_section(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let selected = self.state.filters.collection;
        let smart_selected = self.state.filters.smart_collection;
        let target = self.target_collection().map(|c| c.id);
        let mut rows = div().flex().flex_col().gap(px(1.)).px(px(6.));
        for (i, c) in self.coll.list.iter().enumerate() {
            let active = if c.smart {
                smart_selected == Some(c.id)
            } else {
                selected == Some(c.id)
            };
            let is_target = target == Some(c.id);
            let (cid, name, smart) = (c.id, c.name.clone(), c.smart);
            let count = if c.smart {
                self.smart_count(c)
            } else {
                c.count
            };
            let drop_hot = self.coll.drop_hover.get() == Some(cid);
            let bounds_map = self.coll.row_bounds.clone();
            let frame = self.render_gen.get();
            rows = rows.child(
                div()
                    .relative()
                    .flex()
                    .items_center()
                    .rounded(px(3.))
                    .border_1()
                    .border_color::<Hsla>(if drop_hot {
                        rgb(accent_line()).into()
                    } else {
                        rgba(0x00000000).into()
                    })
                    // Photo-drop target: record the row's visible bounds
                    // (clipped to the scrolling rail).
                    .child(
                        canvas(
                            move |b, window, _| {
                                let mask = window.content_mask().bounds;
                                let visible = b.intersect(&mask);
                                let mut map = bounds_map.borrow_mut();
                                if visible.size.width.as_f32() > 1.
                                    && visible.size.height.as_f32() > 1.
                                {
                                    map.insert(cid, (frame, visible));
                                } else {
                                    map.remove(&cid);
                                }
                            },
                            |_, _, _, _| {},
                        )
                        .absolute()
                        .size_full(),
                    )
                    .child(
                        div()
                            .id(("collection", i))
                            .flex_1()
                            .min_w_0()
                            .rounded(px(3.))
                            .on_hover(self.tip(if c.smart {
                                "Load this smart collection's saved criteria"
                            } else {
                                "Filter to this collection · drop photos to add"
                            }))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if smart {
                                    if this.state.filters.smart_collection == Some(cid) {
                                        this.state.filters = Default::default();
                                    } else if let Some(saved) =
                                        this.coll.list.iter().find(|c| c.id == cid).and_then(|c| {
                                            serde_json::from_str(&c.criteria_json).ok()
                                        })
                                    {
                                        this.state.filters = saved;
                                        this.state.filters.smart_collection = Some(cid);
                                        this.state.filters.smart_collection_name = name.clone();
                                    }
                                } else {
                                    let f = &mut this.state.filters;
                                    f.smart_collection = None;
                                    f.smart_collection_name.clear();
                                    if f.collection == Some(cid) {
                                        f.collection = None;
                                        f.collection_name.clear();
                                        if f.sort.field == laika_core::state::SortField::Album {
                                            f.sort.field = laika_core::state::SortField::Captured;
                                        }
                                    } else {
                                        f.collection = Some(cid);
                                        f.collection_name = name.clone();
                                        this.enter_album_sort();
                                    }
                                }
                                this.coll.members.replace(None);
                                cx.notify();
                            }))
                            .child(list_row::list_row(
                                &c.name,
                                &count.to_string(),
                                if active {
                                    accent_line()
                                } else if c.quick || c.smart {
                                    WARNING
                                } else {
                                    0x424446
                                },
                                active,
                            )),
                    )
                    .child(
                        div()
                            .id(("collection-target", i))
                            .w(px(22.))
                            .h(px(22.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(3.))
                            .font_family(SANS)
                            .text_size(sp(11.))
                            .text_color(rgb(if is_target {
                                accent_line()
                            } else {
                                TEXT_DIMMER
                            }))
                            .hover(|s| s.bg(rgb(bg_row_hover())))
                            .on_hover(self.tip(if c.smart {
                                "Smart collections cannot be collection targets"
                            } else if is_target {
                                "Target collection — B adds here"
                            } else {
                                "Make this the target collection for B"
                            }))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if !smart {
                                    this.set_target_collection(Some(cid), cx);
                                }
                            }))
                            .child(if c.smart {
                                "◆"
                            } else if is_target {
                                "●"
                            } else {
                                "○"
                            }),
                    ),
            );
        }
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
        let viewing = selected
            .or(smart_selected)
            .and_then(|id| self.coll.list.iter().find(|c| c.id == id));
        let mut actions = div()
            .flex()
            .flex_wrap()
            .gap(px(4.))
            .px(px(14.))
            .pt(px(4.))
            .child(
                button(
                    "collection-new",
                    "New",
                    "New collection from the selected photos",
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.open_name_field(NameMode::New, cx);
                })),
            )
            .child(
                button(
                    "collection-smart-new",
                    "Smart",
                    "Save the current filter criteria as a smart collection",
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.open_name_field(NameMode::NewSmart, cx);
                })),
            );
        match viewing {
            Some(c) if c.quick => {
                actions = actions
                    .child(
                        button(
                            "quick-save",
                            "Save as…",
                            "Save the Quick Collection as a collection (then clear it)",
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.open_name_field(NameMode::SaveQuick, cx);
                        })),
                    )
                    .child(
                        button("quick-clear", "Clear", "Empty the Quick Collection").on_click(
                            cx.listener(|this, _, _, cx| {
                                if let (Some(cat), Some(q)) =
                                    (this.catalog.as_ref(), this.coll.quick)
                                {
                                    this.status_note = match cat.clear_collection(q) {
                                        Ok(n) => format!("Quick Collection cleared ({n})"),
                                        Err(e) => e,
                                    };
                                }
                                this.refresh_collections(cx);
                            }),
                        ),
                    )
                    .child(
                        button(
                            "quick-remove",
                            "Remove selected",
                            "Take the selection out (B)",
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(q) = this.coll.quick {
                                this.remove_targets_from(q, cx);
                            }
                        })),
                    );
            }
            Some(c) if !c.smart => {
                let cid = c.id;
                actions = actions
                    .child(
                        button("collection-rename", "Rename…", "Rename this collection").on_click(
                            cx.listener(move |this, _, _, cx| {
                                this.open_name_field(NameMode::Rename(cid), cx);
                            }),
                        ),
                    )
                    .child(
                        button(
                            "collection-remove",
                            "Remove selected",
                            "Take the selected photos out of this collection",
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.remove_targets_from(cid, cx);
                        })),
                    )
                    .child(
                        button(
                            "collection-delete",
                            "Delete",
                            "Delete this collection (photos stay in the catalog)",
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(cat) = this.catalog.as_ref() {
                                this.status_note = match cat.delete_collection(cid) {
                                    Ok(()) => "collection deleted".to_string(),
                                    Err(e) => e,
                                };
                            }
                            if this.coll.target == Some(cid) {
                                this.set_target_collection(None, cx);
                            }
                            this.state.filters.collection = None;
                            this.state.filters.collection_name.clear();
                            this.refresh_collections(cx);
                        })),
                    );
            }
            Some(c) => {
                let cid = c.id;
                actions = actions
                    .child(
                        button(
                            "collection-rename",
                            "Rename…",
                            "Rename this smart collection",
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.open_name_field(NameMode::Rename(cid), cx);
                        })),
                    )
                    .child(
                        button(
                            "collection-delete",
                            "Delete",
                            "Delete this smart collection (photos stay in the catalog)",
                        )
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(cat) = this.catalog.as_ref() {
                                this.status_note = match cat.delete_collection(cid) {
                                    Ok(()) => "smart collection deleted".to_string(),
                                    Err(e) => e,
                                };
                            }
                            this.state.filters = Default::default();
                            this.refresh_collections(cx);
                        })),
                    );
            }
            None => {}
        }
        let mut col = div()
            .flex()
            .flex_col()
            .child(section_header::section_header(
                "Collections",
                selected.is_some(),
                window,
            ))
            .child(rows)
            .child(actions);
        // V30: the viewed collection is also an album.
        if let Some(c) = viewing.filter(|c| !c.smart) {
            col = col.child(self.album_panel(c, cx));
        }
        if self.coll.name_mode != NameMode::Closed {
            let hint = match self.coll.name_mode {
                NameMode::New => "New collection name…",
                NameMode::NewSmart => "Smart collection name…",
                NameMode::SaveQuick => "Save Quick Collection as…",
                NameMode::Rename(_) => "Rename to…",
                NameMode::Closed => "",
            };
            col = col.child(
                div().px(px(14.)).pt(px(6.)).child(
                    self.field_cell(
                        text_input::FieldId::CollectionName,
                        div()
                            .font_family(SANS)
                            .text_size(sp(10.5))
                            .text_color(rgb(TEXT_DIMMER))
                            .child(hint),
                        false,
                        "Type a name, Enter saves, Esc cancels",
                        cx,
                    ),
                ),
            );
        }
        col.child(self.stacks_section(window, cx))
    }
}
