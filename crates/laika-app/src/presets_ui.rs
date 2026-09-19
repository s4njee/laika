//! S02: develop presets in the Develop rail (Laika's built-ins plus
//! imported groups), the amount control, and File → Import Presets….
//! Parsing and coverage live in `laika_core::presets`.

use laika_core::catalog::PresetSave;
use laika_core::presets::{self as pr, Coverage, DevelopPreset, PresetFile};

use super::*;

/// Key for a stored preset in import defaults and preset pickers.
pub(crate) fn user_key(id: i64) -> String {
    format!("user:{id}")
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum PresetImportStage {
    #[default]
    Pick,
    Scanning,
    Preview,
    Done,
}

#[derive(Clone, Debug)]
pub(crate) struct ImportItem {
    pub file: PresetFile,
    pub include: bool,
}

#[derive(Default)]
pub(crate) struct PresetImport {
    pub stage: PresetImportStage,
    /// (label, folder, preset files found).
    pub sources: Vec<(String, PathBuf, usize)>,
    pub chosen: Vec<PathBuf>,
    pub items: Vec<ImportItem>,
    pub errors: Vec<(PathBuf, String)>,
    pub summary: Vec<String>,
}

#[derive(Default)]
pub(crate) struct PresetUi {
    /// Stored presets (refreshed on catalog open and after imports).
    pub user: Vec<DevelopPreset>,
    pub loaded_for: Option<PathBuf>,
    /// Laika's group is open unless toggled; imported groups start
    /// closed (there can be dozens) and open when toggled.
    pub collapsed: HashSet<String>,
    /// Last stored preset applied: (preset id, photo, params before it).
    pub applied: Option<(i64, i64, [f32; edit::PARAM_COUNT])>,
    pub amount: f32,
    /// Group awaiting a second click to remove.
    pub remove_group: Option<String>,
    /// One preset awaiting a second-click delete confirmation.
    pub remove_preset: Option<i64>,
    pub editor: Option<PresetEditor>,
    /// Non-destructive hover preview: preset, photo, original live values.
    pub preview: Option<(i64, i64, [f32; edit::PARAM_COUNT])>,
    pub import: Option<PresetImport>,
}

pub(crate) struct PresetEditor {
    pub id: Option<i64>,
    pub name: String,
    pub group: String,
    pub source: [f32; edit::PARAM_COUNT],
    pub include: [bool; edit::PARAM_COUNT],
}

fn coverage_badge(c: Coverage) -> Option<(&'static str, u32)> {
    match c {
        Coverage::Complete => None,
        Coverage::Approximate => Some(("≈", 0xC8A15A)),
        Coverage::Unsupported => Some(("✕", 0xE56060)),
    }
}

impl Laika {
    /// Load stored presets once per catalog.
    pub(crate) fn ensure_presets(&mut self) {
        let Some(cat) = self.catalog.as_ref() else {
            return;
        };
        let db = cat.db_path().to_path_buf();
        if self.presets.loaded_for.as_ref() != Some(&db) {
            self.presets.user = cat.develop_presets();
            self.presets.loaded_for = Some(db);
            self.presets.applied = None;
        }
    }

    fn reload_presets(&mut self) {
        self.presets.loaded_for = None;
        self.ensure_presets();
    }

    pub(crate) fn open_preset_editor(&mut self, id: Option<i64>, cx: &mut Context<Self>) {
        let Some(pid) = self.state.primary else {
            self.status_note = "select a photo to save its settings as a preset".to_string();
            cx.notify();
            return;
        };
        self.cancel_preset_preview(cx);
        let mut source = self
            .state
            .edits
            .get(&pid)
            .map(|e| e.params)
            .unwrap_or_else(edit::defaults);
        let (name, group, include) = if let Some(id) = id {
            let Some(p) = self.presets.user.iter().find(|p| p.id == id) else {
                return;
            };
            let mut include = [false; edit::PARAM_COUNT];
            for (&i, &value) in &p.values {
                if i < edit::PARAM_COUNT {
                    include[i] = true;
                    source[i] = value;
                }
            }
            (p.name.clone(), p.group.clone(), include)
        } else {
            (
                "New preset".to_string(),
                "User Presets".to_string(),
                pr::default_setting_mask(),
            )
        };
        self.presets.editor = Some(PresetEditor {
            id,
            name,
            group,
            source,
            include,
        });
        self.focus_field(text_input::FieldId::DevelopPresetName, cx);
        cx.notify();
    }

    fn save_preset_editor(&mut self, cx: &mut Context<Self>) {
        if self.field.is_some() && !self.commit_field(cx) {
            return;
        }
        let Some(editor) = self.presets.editor.as_ref() else {
            return;
        };
        if editor.name.trim().is_empty() {
            self.status_note = "name the preset".to_string();
            cx.notify();
            return;
        }
        let values = pr::sparse_values(&editor.source, &editor.include);
        if values.is_empty() {
            self.status_note = "choose at least one setting for the preset".to_string();
            cx.notify();
            return;
        }
        let preset = DevelopPreset {
            id: editor.id.unwrap_or_default(),
            name: editor.name.trim().to_string(),
            group: editor.group.trim().to_string(),
            values,
            ..Default::default()
        };
        let result = match (self.catalog.as_ref(), editor.id) {
            (Some(cat), Some(id)) => cat.update_develop_preset(id, &preset),
            (Some(cat), None) => cat.save_develop_preset(&preset).map(|_| ()),
            (None, _) => Err("open a catalog first".to_string()),
        };
        match result {
            Ok(()) => {
                let action = if editor.id.is_some() {
                    "updated"
                } else {
                    "created"
                };
                self.status_note = format!("{action} preset {}", preset.name);
                self.presets.collapsed.insert(preset.group.clone());
                self.presets.editor = None;
                self.defocus_field();
                self.reload_presets();
            }
            Err(e) => self.status_note = e,
        }
        cx.notify();
    }

    fn delete_user_preset(&mut self, id: i64, cx: &mut Context<Self>) {
        if self.presets.remove_preset != Some(id) {
            self.presets.remove_preset = Some(id);
            cx.notify();
            return;
        }
        let name = self
            .presets
            .user
            .iter()
            .find(|p| p.id == id)
            .map(|p| p.name.clone())
            .unwrap_or_else(|| "preset".to_string());
        let result = self
            .catalog
            .as_ref()
            .ok_or_else(|| "open a catalog first".to_string())
            .and_then(|cat| cat.delete_develop_preset(id));
        self.presets.remove_preset = None;
        match result {
            Ok(()) => {
                self.reload_presets();
                self.status_note = format!("deleted preset {name}");
            }
            Err(e) => self.status_note = e,
        }
        cx.notify();
    }

    fn export_user_preset(&mut self, id: i64, cx: &mut Context<Self>) {
        let Some(preset) = self.presets.user.iter().find(|p| p.id == id).cloned() else {
            return;
        };
        let bytes = match pr::export_laika_preset(&preset) {
            Ok(bytes) => bytes,
            Err(e) => {
                self.status_note = e;
                cx.notify();
                return;
            }
        };
        let safe: String = preset
            .name
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') {
                    c
                } else {
                    '-'
                }
            })
            .collect();
        let suggested = format!("{}.laikapreset", safe.trim_matches('-'));
        let initial = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let rx = cx.prompt_for_new_path(&initial, Some(&suggested));
        cx.spawn(async move |entity, cx| {
            let path = rx.await.ok().and_then(|r| r.ok()).flatten();
            let Some(path) = path else {
                return;
            };
            let result = cx
                .background_spawn(async move {
                    std::fs::write(&path, bytes)
                        .map(|_| path)
                        .map_err(|e| format!("export preset: {e}"))
                })
                .await;
            entity
                .update(cx, |this, cx| {
                    this.status_note = match result {
                        Ok(path) => format!("exported preset to {}", path.display()),
                        Err(e) => e,
                    };
                    cx.notify();
                })
                .ok();
        })
        .detach();
    }

    fn preview_user_preset(&mut self, id: i64, hovered: bool, cx: &mut Context<Self>) {
        if hovered {
            if self.presets.preview.is_some() {
                return;
            }
            let Some(pid) = self.state.primary else {
                return;
            };
            let Some(preset) = self.presets.user.iter().find(|p| p.id == id).cloned() else {
                return;
            };
            let original = self.values;
            self.presets.preview = Some((id, pid, original));
            self.values = preset.apply(&original, 1.0);
            self.submit_dev(cx);
        } else if self.presets.preview.is_some_and(|(pre, _, _)| pre == id) {
            self.cancel_preset_preview(cx);
        }
    }

    pub(crate) fn cancel_preset_preview(&mut self, cx: &mut Context<Self>) {
        let Some((_, pid, original)) = self.presets.preview.take() else {
            return;
        };
        if self.state.primary == Some(pid) {
            self.values = original;
            self.submit_dev(cx);
        }
    }

    /// Full params for a preset key: a built-in name or `user:<id>`,
    /// applied over defaults (import-time presets).
    pub(crate) fn develop_preset_params_any(
        &self,
        key: &str,
    ) -> Option<([f32; edit::PARAM_COUNT], String)> {
        if let Some(id) = key
            .strip_prefix("user:")
            .and_then(|s| s.parse::<i64>().ok())
        {
            let p = self.presets.user.iter().find(|p| p.id == id)?;
            return Some((p.apply(&edit::defaults(), 1.0), p.name.clone()));
        }
        Self::develop_preset_params(key)
    }

    pub(crate) fn preset_key_exists(&self, key: &str) -> bool {
        self.develop_preset_params_any(key).is_some()
    }

    /// Apply a stored preset to the primary photo: only its included
    /// settings change; one history step.
    pub(crate) fn apply_user_preset(&mut self, id: i64, amount: f32, cx: &mut Context<Self>) {
        self.cancel_preset_preview(cx);
        let Some(preset) = self.presets.user.iter().find(|p| p.id == id).cloned() else {
            return;
        };
        let Some(pid) = self.state.primary else {
            self.status_note = "select a photo to apply a preset".to_string();
            cx.notify();
            return;
        };
        // Re-applying at another amount starts from the same base.
        let base = match self.presets.applied {
            Some((pre, photo, base)) if pre == id && photo == pid => base,
            _ => self
                .state
                .edits
                .get(&pid)
                .map(|e| e.params)
                .unwrap_or_else(edit::defaults),
        };
        let before = self.snap_current(pid);
        self.values = preset.apply(&base, amount);
        self.state.edit(pid).params = self.values;
        let value = if preset.supports_amount && (amount - 1.).abs() > 1e-3 {
            format!("{} {:.0}%", preset.name, amount * 100.)
        } else {
            preset.name.clone()
        };
        self.record_step(pid, "Preset", &value, before);
        self.last_batch = vec![pid];
        self.last_was_meta = false;
        self.last_was_remove = false;
        self.redo_batch.clear();
        self.presets.applied = Some((id, pid, base));
        self.presets.collapsed.insert(preset.group.clone());
        self.presets.amount = amount;
        if let Some(dev) = self.dev.as_mut() {
            dev.last_preset = Some(preset.name.clone());
        }
        self.edits_dirty = true;
        self.submit_dev(cx);
        self.persist_edits();
        self.request_sidecar(pid, false);
        if !self.sidecar_pending.is_empty() {
            self.kick_save_timer(cx);
        }
        self.status_note = match preset.coverage() {
            Coverage::Complete => format!("applied {}", preset.name),
            _ => format!("applied {} — {}", preset.name, preset.coverage_note()),
        };
        cx.notify();
    }

    // ---- Develop rail --------------------------------------------------------------

    pub(crate) fn preset_rail(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let mut col = div().flex().flex_col().gap(px(1.)).px(px(6.));
        let group_header = |id: (&'static str, usize), name: &str, count: usize, open: bool| {
            div()
                .id(id)
                .flex()
                .items_center()
                .gap(px(6.))
                .px(px(8.))
                .pt(px(8.))
                .pb(px(3.))
                .text_size(sp(10.))
                .text_color(rgb(TEXT_DIM))
                .hover(|s| s.text_color(rgb(TEXT_SECONDARY)))
                .child(if open { "▾" } else { "▸" })
                .child(div().flex_1().min_w_0().truncate().child(name.to_string()))
                .child(count.to_string())
        };
        // Laika's own looks.
        let builtin_open = !self.presets.collapsed.contains("");
        col = col.child(
            group_header(
                ("preset-group-laika", 0),
                "Laika",
                PRESETS.len(),
                builtin_open,
            )
            .on_click(cx.listener(|this, _, _, cx| {
                if !this.presets.collapsed.remove("") {
                    this.presets.collapsed.insert(String::new());
                }
                cx.notify();
            })),
        );
        if builtin_open {
            for (i, (name, from, to, _)) in PRESETS.iter().enumerate() {
                let active = i == self.preset && self.presets.applied.is_none();
                col = col.child(
                    div()
                        .id(("preset", i))
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .px(px(8.))
                        .py(px(5.))
                        .rounded(px(3.))
                        .text_size(sp(11.5))
                        .text_color(rgb(if active { TEXT_PRIMARY } else { TEXT_SECONDARY }))
                        .when(active, |d| d.bg(rgb(bg_row_active())))
                        .when(!active, |d| d.hover(|s| s.bg(rgb(bg_row_hover()))))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            this.presets.applied = None;
                            this.apply_preset(i, cx)
                        }))
                        .child(
                            div()
                                .w(px(18.))
                                .h(px(12.))
                                .rounded(px(1.))
                                .bg(linear_gradient(
                                    135.,
                                    linear_color_stop(rgb(*from), 0.),
                                    linear_color_stop(rgb(*to), 1.),
                                )),
                        )
                        .child(*name),
                );
            }
        }
        // Imported / saved groups.
        let mut groups: Vec<(String, Vec<&DevelopPreset>)> = Vec::new();
        for p in &self.presets.user {
            match groups.iter_mut().find(|(g, _)| *g == p.group) {
                Some((_, v)) => v.push(p),
                None => groups.push((p.group.clone(), vec![p])),
            }
        }
        let applied = self
            .presets
            .applied
            .filter(|(_, pid, _)| Some(*pid) == self.state.primary)
            .map(|(id, _, _)| id);
        for (gi, (group, list)) in groups.into_iter().enumerate() {
            let open = self.presets.collapsed.contains(&group);
            let g = group.clone();
            let label = if group.is_empty() {
                "Imported"
            } else {
                group.as_str()
            };
            if self.presets.remove_group.as_deref() == Some(group.as_str()) {
                let g2 = group.clone();
                col = col.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .px(px(8.))
                        .pt(px(8.))
                        .pb(px(3.))
                        .text_size(sp(10.5))
                        .text_color(rgb(TEXT_SECONDARY))
                        .child(
                            div()
                                .flex_1()
                                .child(format!("Remove {} presets?", list.len())),
                        )
                        .child(
                            div()
                                .id(("preset-group-remove-yes", gi))
                                .text_color(rgb(0xE56060))
                                .hover(|s| s.text_color(rgb(TEXT_PRIMARY)))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.remove_preset_group(&g2, cx)
                                }))
                                .child("Remove"),
                        )
                        .child(
                            div()
                                .id(("preset-group-remove-no", gi))
                                .text_color(rgb(TEXT_DIM))
                                .hover(|s| s.text_color(rgb(TEXT_PRIMARY)))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.presets.remove_group = None;
                                    cx.notify();
                                }))
                                .child("Keep"),
                        ),
                );
            } else {
                let g3 = group.clone();
                col = col.child(
                    div()
                        .flex()
                        .items_center()
                        .child(
                            group_header(("preset-group", gi + 1), label, list.len(), open)
                                .flex_1()
                                .min_w_0()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if !this.presets.collapsed.remove(&g) {
                                        this.presets.collapsed.insert(g.clone());
                                    }
                                    cx.notify();
                                })),
                        )
                        .child(
                            div()
                                .id(("preset-group-remove", gi))
                                .pt(px(5.))
                                .px(px(6.))
                                .text_size(sp(10.))
                                .text_color(rgb(TEXT_DIMMER))
                                .hover(|s| s.text_color(rgb(TEXT_PRIMARY)))
                                .on_hover(self.tip("Remove this group of presets"))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.presets.remove_group = Some(g3.clone());
                                    cx.notify();
                                }))
                                .child("✕"),
                        ),
                );
            }
            if !open {
                continue;
            }
            for p in list {
                let id = p.id;
                let active = applied == Some(id);
                let badge = coverage_badge(p.coverage());
                let note = p.coverage_note();
                let tip = match p.coverage() {
                    Coverage::Complete => {
                        format!("{} — everything in this preset renders in Laika", p.name)
                    }
                    Coverage::Approximate => format!("{} — {note}", p.name),
                    Coverage::Unsupported => {
                        format!(
                            "{} — nothing in this preset renders in Laika. {note}",
                            p.name
                        )
                    }
                };
                col =
                    col.child(
                        div()
                            .id(("user-preset", id as usize))
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .pl(px(18.))
                            .pr(px(8.))
                            .py(px(4.))
                            .rounded(px(3.))
                            .text_size(sp(11.5))
                            .text_color(rgb(if active { TEXT_PRIMARY } else { TEXT_SECONDARY }))
                            .when(active, |d| d.bg(rgb(bg_row_active())))
                            .when(!active, |d| d.hover(|s| s.bg(rgb(bg_row_hover()))))
                            .on_hover(cx.listener(move |this, hovered: &bool, _, cx| {
                                this.hover_tip
                                    .set(if *hovered { Some(tip.clone()) } else { None });
                                this.preview_user_preset(id, *hovered, cx);
                                cx.notify();
                            }))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.apply_user_preset(id, 1.0, cx)
                            }))
                            .child(div().flex_1().min_w_0().truncate().child(p.name.clone()))
                            .children(badge.map(|(b, c)| {
                                div().text_size(sp(10.5)).text_color(rgb(c)).child(b)
                            })),
                    );
                let deleting = self.presets.remove_preset == Some(id);
                col = col.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(10.))
                        .pl(px(26.))
                        .pr(px(8.))
                        .pb(px(4.))
                        .text_size(sp(9.5))
                        .text_color(rgb(TEXT_DIM))
                        .child(
                            div()
                                .id(("preset-edit", id as usize))
                                .hover(|s| s.text_color(rgb(TEXT_PRIMARY)))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.open_preset_editor(Some(id), cx)
                                }))
                                .child("Rename / settings"),
                        )
                        .child(
                            div()
                                .id(("preset-export", id as usize))
                                .hover(|s| s.text_color(rgb(TEXT_PRIMARY)))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.export_user_preset(id, cx)
                                }))
                                .child("Export"),
                        )
                        .child(div().flex_1())
                        .child(
                            div()
                                .id(("preset-delete", id as usize))
                                .text_color(rgb(if deleting { 0xE56060 } else { TEXT_DIM }))
                                .hover(|s| s.text_color(rgb(0xE56060)))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.delete_user_preset(id, cx)
                                }))
                                .child(if deleting { "Delete?" } else { "Delete" }),
                        ),
                );
                if active && p.supports_amount {
                    let now = self.presets.amount;
                    col = col.child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(1.))
                            .pl(px(18.))
                            .pr(px(8.))
                            .pb(px(4.))
                            .child(
                                div()
                                    .mr(px(2.))
                                    .text_size(sp(10.))
                                    .text_color(rgb(TEXT_DIM))
                                    .child("Amount"),
                            )
                            .children([0.5f32, 0.75, 1.0, 1.25, 1.5].into_iter().enumerate().map(
                                |(k, a)| {
                                    let on = (now - a).abs() < 1e-3;
                                    div()
                                        .id(("preset-amount", k))
                                        .px(px(3.5))
                                        .py(px(2.))
                                        .rounded(px(3.))
                                        .text_size(sp(10.))
                                        .text_color(rgb(if on {
                                            accent_on_fill()
                                        } else {
                                            TEXT_SECONDARY
                                        }))
                                        .when(on, |d| d.bg(rgb(accent_fill())))
                                        .when(!on, |d| d.hover(|s| s.bg(rgb(bg_row_hover()))))
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.apply_user_preset(id, a, cx)
                                        }))
                                        .child(format!("{:.0}", a * 100.))
                                },
                            )),
                    );
                }
            }
        }
        let _ = window;
        div()
            .flex()
            .flex_col()
            .child(
                div()
                    .flex()
                    .items_center()
                    .child(
                        div()
                            .id("presets-save")
                            .mr(px(10.))
                            .pt(px(10.))
                            .text_size(sp(10.5))
                            .text_color(rgb(TEXT_DIM))
                            .hover(|s| s.text_color(rgb(TEXT_PRIMARY)))
                            .on_hover(self.tip("Create a preset from the current photo"))
                            .on_click(
                                cx.listener(|this, _, _, cx| this.open_preset_editor(None, cx)),
                            )
                            .child("Create…"),
                    )
                    .child(
                        div()
                            .flex_1()
                            .child(section_header::section_header("Presets", false, window)),
                    )
                    .child(
                        div()
                            .id("presets-import")
                            .mr(px(14.))
                            .pt(px(10.))
                            .text_size(sp(10.5))
                            .text_color(rgb(TEXT_DIM))
                            .hover(|s| s.text_color(rgb(TEXT_PRIMARY)))
                            .on_hover(self.tip("Import Lightroom or Camera Raw presets"))
                            .on_click(cx.listener(|this, _, _, cx| this.open_preset_import(cx)))
                            .child("Import…"),
                    ),
            )
            .child(
                div()
                    .id("presets-scroll")
                    .max_h(px(360.))
                    .overflow_y_scroll()
                    .child(col),
            )
    }

    fn remove_preset_group(&mut self, group: &str, cx: &mut Context<Self>) {
        let Some(cat) = self.catalog.as_ref() else {
            return;
        };
        let ids: Vec<i64> = self
            .presets
            .user
            .iter()
            .filter(|p| p.group == group)
            .map(|p| p.id)
            .collect();
        let mut failed = None;
        for id in &ids {
            if let Err(e) = cat.delete_develop_preset(*id) {
                failed = Some(e);
            }
        }
        self.presets.remove_group = None;
        self.reload_presets();
        self.status_note = match failed {
            Some(e) => e,
            None => format!(
                "removed {} preset{} — photos keep the settings they already have",
                ids.len(),
                if ids.len() == 1 { "" } else { "s" }
            ),
        };
        cx.notify();
    }

    pub(crate) fn preset_editor_modal(&self, cx: &mut Context<Self>) -> Div {
        let Some(editor) = self.presets.editor.as_ref() else {
            return div();
        };
        let field = |value: &str, placeholder: &str| {
            div()
                .w_full()
                .px(px(9.))
                .py(px(7.))
                .rounded(px(4.))
                .border_1()
                .border_color(border_control())
                .text_size(sp(11.5))
                .text_color(rgb(if value.is_empty() {
                    TEXT_DIMMER
                } else {
                    TEXT_PRIMARY
                }))
                .child(if value.is_empty() {
                    placeholder.to_string()
                } else {
                    value.to_string()
                })
        };
        let mut groups = div().flex().flex_col().gap(px(4.));
        for (index, group) in pr::SETTING_GROUPS.iter().enumerate() {
            let included = group.range.clone().filter(|i| editor.include[*i]).count();
            let on = included == group.range.len();
            let suffix = if included == 0 {
                String::new()
            } else if on {
                format!("{} settings", group.range.len())
            } else {
                format!("{included} of {} settings", group.range.len())
            };
            groups = groups.child(
                div()
                    .id(("preset-editor-group", index))
                    .flex()
                    .items_center()
                    .gap(px(9.))
                    .px(px(8.))
                    .py(px(5.))
                    .rounded(px(4.))
                    .hover(|s| s.bg(rgb(bg_row_hover())))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(editor) = this.presets.editor.as_mut() {
                            for i in pr::SETTING_GROUPS[index].range.clone() {
                                editor.include[i] = !on;
                            }
                        }
                        cx.notify();
                    }))
                    .child(toggle::toggle(on, ""))
                    .child(
                        div()
                            .flex_1()
                            .text_size(sp(11.5))
                            .text_color(rgb(TEXT_SECONDARY))
                            .child(group.name),
                    )
                    .child(
                        div()
                            .text_size(sp(9.5))
                            .text_color(rgb(TEXT_DIM))
                            .child(suffix),
                    ),
            );
        }
        let editing = editor.id.is_some();
        let included = editor.include.iter().filter(|on| **on).count();
        modal::modal_shell_w(
            div()
                .p(px(24.))
                .flex()
                .flex_col()
                .gap(px(13.))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .child(
                            div()
                                .flex_1()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_size(sp(16.))
                                .text_color(rgb(TEXT_PRIMARY))
                                .child(if editing {
                                    "Edit Preset"
                                } else {
                                    "Create Preset"
                                }),
                        )
                        .child(
                            div()
                                .id("preset-editor-close")
                                .text_color(rgb(TEXT_DIM))
                                .hover(|s| s.text_color(rgb(TEXT_PRIMARY)))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.presets.editor = None;
                                    this.defocus_field();
                                    cx.notify();
                                }))
                                .child("✕"),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .gap(px(10.))
                        .child(
                            div()
                                .flex_1()
                                .flex()
                                .flex_col()
                                .gap(px(4.))
                                .child(
                                    div()
                                        .text_size(sp(10.))
                                        .text_color(rgb(TEXT_DIM))
                                        .child("NAME"),
                                )
                                .child(self.field_cell(
                                    text_input::FieldId::DevelopPresetName,
                                    field(&editor.name, "Preset name"),
                                    false,
                                    "Name this preset",
                                    cx,
                                )),
                        )
                        .child(
                            div()
                                .flex_1()
                                .flex()
                                .flex_col()
                                .gap(px(4.))
                                .child(
                                    div()
                                        .text_size(sp(10.))
                                        .text_color(rgb(TEXT_DIM))
                                        .child("GROUP"),
                                )
                                .child(self.field_cell(
                                    text_input::FieldId::DevelopPresetGroup,
                                    field(&editor.group, "User Presets"),
                                    false,
                                    "Group presets together",
                                    cx,
                                )),
                        ),
                )
                .child(
                    div()
                        .text_size(sp(11.))
                        .text_color(rgb(TEXT_SECONDARY))
                        .child("Include settings"),
                )
                .child(groups)
                .child(div().text_size(sp(10.)).text_color(rgb(TEXT_DIM)).child(
                    "Crop and local masks are not preset settings. Transform starts unchecked.",
                ))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .child(
                            div()
                                .flex_1()
                                .text_size(sp(10.5))
                                .text_color(rgb(TEXT_DIM))
                                .child(format!("{included} settings included")),
                        )
                        .child(
                            div()
                                .id("preset-editor-save")
                                .on_click(cx.listener(|this, _, _, cx| this.save_preset_editor(cx)))
                                .child(button::primary(if editing {
                                    "Save Changes"
                                } else {
                                    "Create Preset"
                                })),
                        ),
                ),
            560.,
        )
    }

    // ---- Import Presets dialog -------------------------------------------------------

    pub(crate) fn open_preset_import(&mut self, cx: &mut Context<Self>) {
        if self.catalog.is_none() {
            self.status_note = "open a catalog first".to_string();
            cx.notify();
            return;
        }
        self.close_modals(cx);
        let home = laika_core::platform::home_dir();
        let sources: Vec<(String, PathBuf, usize)> = pr::default_sources(&home)
            .into_iter()
            .map(|(label, path)| {
                let n = pr::preset_files(&path).len();
                (label, path, n)
            })
            .filter(|(_, _, n)| *n > 0)
            .collect();
        self.presets.import = Some(PresetImport {
            chosen: sources.iter().map(|(_, p, _)| p.clone()).collect(),
            sources,
            ..Default::default()
        });
        cx.notify();
    }

    fn preset_import_choose(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: true,
            multiple: true,
            prompt: Some("Import presets".into()),
        });
        cx.spawn(async move |entity, cx| {
            let paths = rx
                .await
                .ok()
                .and_then(|r| r.ok())
                .flatten()
                .unwrap_or_default();
            if paths.is_empty() {
                return;
            }
            entity
                .update(cx, |this, cx| {
                    if let Some(imp) = this.presets.import.as_mut() {
                        imp.chosen = paths;
                    }
                    this.preset_import_scan(cx);
                })
                .ok();
        })
        .detach();
    }

    fn preset_import_scan(&mut self, cx: &mut Context<Self>) {
        let Some(imp) = self.presets.import.as_mut() else {
            return;
        };
        let chosen = imp.chosen.clone();
        if chosen.is_empty() {
            return;
        }
        imp.stage = PresetImportStage::Scanning;
        cx.notify();
        cx.spawn(async move |entity, cx| {
            let (items, errors) = cx
                .background_spawn(async move {
                    let mut items = Vec::new();
                    let mut errors = Vec::new();
                    let mut seen = HashSet::new();
                    for root in &chosen {
                        for f in pr::preset_files(root) {
                            if !seen.insert(f.clone()) {
                                continue;
                            }
                            match pr::read_file(&f) {
                                Ok(file) => {
                                    let include = match &file {
                                        PresetFile::Develop(p) => {
                                            p.coverage() != Coverage::Unsupported
                                        }
                                        PresetFile::Metadata(_) => true,
                                        _ => false,
                                    };
                                    items.push(ImportItem { file, include });
                                }
                                Err(e) => errors.push((f, e)),
                            }
                        }
                    }
                    (items, errors)
                })
                .await;
            entity
                .update(cx, |this, cx| {
                    if let Some(imp) = this.presets.import.as_mut() {
                        eprintln!(
                            "[presets] scanned {} files ({} unreadable)",
                            items.len(),
                            errors.len()
                        );
                        imp.items = items;
                        imp.errors = errors;
                        imp.stage = PresetImportStage::Preview;
                    }
                    cx.notify();
                })
                .ok();
        })
        .detach();
    }

    fn preset_import_apply(&mut self, cx: &mut Context<Self>) {
        let Some(cat) = self.catalog.as_ref() else {
            return;
        };
        let Some(imp) = self.presets.import.as_mut() else {
            return;
        };
        let (mut added, mut replaced, mut unchanged, mut meta) = (0, 0, 0, 0);
        let mut failed: Vec<String> = Vec::new();
        for item in imp.items.iter().filter(|i| i.include) {
            match &item.file {
                PresetFile::Develop(p) => match cat.save_develop_preset(p) {
                    Ok(PresetSave::Added(_)) => added += 1,
                    Ok(PresetSave::Replaced(_)) => replaced += 1,
                    Ok(PresetSave::Unchanged(_)) => unchanged += 1,
                    Err(e) => failed.push(format!("{}: {e}", p.name)),
                },
                PresetFile::Metadata(m) => {
                    let preset = laika_core::catalog::MetadataPreset {
                        name: m.name.clone(),
                        title: m.title.clone(),
                        caption: m.caption.clone(),
                        headline: m.headline.clone(),
                        creator: m.creator.clone(),
                        copyright: m.copyright.clone(),
                        rights: m.rights.clone(),
                        contact: m.contact.clone(),
                        location: m.location.clone(),
                        keywords: m.keywords.clone(),
                    };
                    match cat.save_metadata_preset(&preset) {
                        Ok(()) => meta += 1,
                        Err(e) => failed.push(format!("{}: {e}", m.name)),
                    }
                }
                _ => {}
            }
        }
        let profiles = imp
            .items
            .iter()
            .filter(|i| matches!(i.file, PresetFile::Profile { .. }))
            .count();
        let others = imp
            .items
            .iter()
            .filter(|i| matches!(i.file, PresetFile::Other { .. }))
            .count();
        let mut summary = vec![format!(
            "{added} develop preset{} added · {replaced} updated · {unchanged} already here",
            if added == 1 { "" } else { "s" }
        )];
        if meta > 0 {
            summary.push(format!(
                "{meta} metadata preset{} added (Library → Metadata presets)",
                if meta == 1 { "" } else { "s" }
            ));
        }
        if profiles > 0 {
            summary.push(format!(
                "{profiles} profiles not imported — Laika has no camera or creative profiles yet"
            ));
        }
        if others > 0 {
            summary.push(format!(
                "{others} other templates (filename, export, …) not imported"
            ));
        }
        if !imp.errors.is_empty() {
            summary.push(format!("{} files couldn't be read", imp.errors.len()));
        }
        summary.extend(failed);
        eprintln!("[presets] {}", summary.join(" | "));
        imp.summary = summary;
        imp.stage = PresetImportStage::Done;
        self.reload_presets();
        cx.notify();
    }

    pub(crate) fn preset_import_modal(&self, cx: &mut Context<Self>) -> Div {
        let Some(imp) = self.presets.import.as_ref() else {
            return div();
        };
        let header = div()
            .flex()
            .items_center()
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_size(sp(16.))
                    .text_color(rgb(TEXT_PRIMARY))
                    .child("Import Presets"),
            )
            .child(div().flex_1())
            .child(
                div()
                    .id("presets-import-close")
                    .text_size(sp(15.))
                    .text_color(rgb(TEXT_DIM))
                    .hover(|d| d.text_color(rgb(TEXT_PRIMARY)))
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.presets.import = None;
                        cx.notify();
                    }))
                    .child("✕"),
            );
        let note = |t: String| div().text_size(sp(11.)).text_color(rgb(TEXT_DIM)).child(t);
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
        let body = match imp.stage {
            PresetImportStage::Pick => {
                let mut list = div().flex().flex_col().gap(px(6.));
                for (i, (label, path, n)) in imp.sources.iter().enumerate() {
                    let on = imp.chosen.contains(path);
                    let p = path.clone();
                    list = list.child(
                        div()
                            .id(("preset-source", i))
                            .flex()
                            .items_center()
                            .gap(px(10.))
                            .p(px(10.))
                            .rounded(px(5.))
                            .bg(rgb(bg_well()))
                            .hover(|s| s.bg(rgb(bg_row_hover())))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if let Some(imp) = this.presets.import.as_mut() {
                                    if let Some(k) = imp.chosen.iter().position(|c| *c == p) {
                                        imp.chosen.remove(k);
                                    } else {
                                        imp.chosen.push(p.clone());
                                    }
                                }
                                cx.notify();
                            }))
                            .child(toggle::toggle(on, ""))
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .flex()
                                    .flex_col()
                                    .child(div().text_size(sp(12.)).text_color(rgb(TEXT_PRIMARY)).child(label.clone()))
                                    .child(note(path.display().to_string())),
                            )
                            .child(div().text_size(sp(11.)).text_color(rgb(TEXT_SECONDARY)).child(format!("{n} files"))),
                    );
                }
                div()
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .child(div().text_size(sp(12.)).text_color(rgb(TEXT_SECONDARY)).child(
                        "Import Laika presets (.laikapreset), Lightroom / Camera Raw develop presets (.xmp, .lrtemplate), and Lightroom Classic metadata presets. Each preset shows how much Laika renders before import.",
                    ))
                    .child(if imp.sources.is_empty() {
                        note("No Adobe presets found in the usual places — choose files or a folder.".to_string())
                    } else {
                        list
                    })
                    .child(
                        div()
                            .flex()
                            .gap(px(8.))
                            .child(button("presets-choose", "Choose Files or Folder…").on_click(
                                cx.listener(|this, _, _, cx| this.preset_import_choose(cx)),
                            ))
                            .child(div().flex_1())
                            .when(!imp.chosen.is_empty(), |d| {
                                d.child(
                                    div()
                                        .id("presets-scan")
                                        .on_click(cx.listener(|this, _, _, cx| this.preset_import_scan(cx)))
                                        .child(button::primary("Review Presets")),
                                )
                            }),
                    )
            }
            PresetImportStage::Scanning => div().child(note("Reading presets…".to_string())),
            PresetImportStage::Preview => self.preset_import_preview(imp, cx),
            PresetImportStage::Done => div()
                .flex()
                .flex_col()
                .gap(px(8.))
                .children(imp.summary.iter().map(|l| {
                    div().text_size(sp(12.)).text_color(rgb(TEXT_SECONDARY)).child(l.clone())
                }))
                .child(note(
                    "Imported presets are in Develop → Presets. ≈ marks presets Laika renders only in part — hover one to see what's missing."
                        .to_string(),
                ))
                .child(
                    div().flex().child(div().flex_1()).child(
                        div()
                            .id("presets-done")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.presets.import = None;
                                cx.notify();
                            }))
                            .child(button::primary("Done")),
                    ),
                ),
        };
        let content = div()
            .p(px(24.))
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(header)
            .child(body);
        modal::modal_shell_w(content, 680.)
    }

    fn preset_import_preview(&self, imp: &PresetImport, cx: &mut Context<Self>) -> Div {
        let (mut complete, mut approx, mut unsupported, mut meta, mut profiles, mut other) =
            (0, 0, 0, 0, 0, 0);
        for i in &imp.items {
            match &i.file {
                PresetFile::Develop(p) => match p.coverage() {
                    Coverage::Complete => complete += 1,
                    Coverage::Approximate => approx += 1,
                    Coverage::Unsupported => unsupported += 1,
                },
                PresetFile::Metadata(_) => meta += 1,
                PresetFile::Profile { .. } => profiles += 1,
                PresetFile::Other { .. } => other += 1,
            }
        }
        let chosen = imp.items.iter().filter(|i| i.include).count();
        let chip = |label: String, color: u32| {
            div()
                .px(px(8.))
                .py(px(3.))
                .rounded(px(3.))
                .bg(rgb(bg_well()))
                .text_size(sp(11.))
                .text_color(rgb(color))
                .child(label)
        };
        let mut list = div()
            .id("preset-preview-list")
            .max_h(px(380.))
            .overflow_y_scroll()
            .p(px(6.))
            .rounded(px(5.))
            .bg(rgb(bg_well()))
            .flex()
            .flex_col();
        let mut last_group = String::new();
        for (i, item) in imp.items.iter().enumerate() {
            let (group, name, cov, detail) = match &item.file {
                PresetFile::Develop(p) => (
                    p.group.clone(),
                    p.name.clone(),
                    Some(p.coverage()),
                    p.coverage_note(),
                ),
                PresetFile::Metadata(m) => (
                    "Metadata presets".to_string(),
                    m.name.clone(),
                    None,
                    if m.skipped.is_empty() {
                        "metadata preset".to_string()
                    } else {
                        format!("metadata preset — not kept: {}", m.skipped.join(", "))
                    },
                ),
                PresetFile::Profile {
                    name,
                    group,
                    suggestion,
                } => (
                    format!("Profiles · {group}"),
                    name.clone(),
                    Some(Coverage::Unsupported),
                    format!("profile — not imported; nearest Laika look: {suggestion}"),
                ),
                PresetFile::Other { name, kind } => (
                    "Other templates".to_string(),
                    name.clone(),
                    Some(Coverage::Unsupported),
                    format!("{kind} template — not imported"),
                ),
            };
            // Profiles are summarized, not listed one by one (there are hundreds).
            if matches!(item.file, PresetFile::Profile { .. }) {
                continue;
            }
            if group != last_group {
                list = list.child(
                    div()
                        .px(px(6.))
                        .pt(px(8.))
                        .pb(px(3.))
                        .text_size(sp(10.))
                        .text_color(rgb(TEXT_DIM))
                        .child(if group.is_empty() {
                            "(no group)".to_string()
                        } else {
                            group.clone()
                        }),
                );
                last_group = group;
            }
            let selectable = matches!(item.file, PresetFile::Develop(_) | PresetFile::Metadata(_));
            let on = item.include;
            let (badge, color) = match cov {
                Some(Coverage::Complete) => ("Complete", accent_line()),
                Some(Coverage::Approximate) => ("Approximate", 0xC8A15A),
                Some(Coverage::Unsupported) => ("Unsupported", 0xE56060),
                None => ("Metadata", TEXT_SECONDARY),
            };
            list = list.child(
                div()
                    .id(("preset-item", i))
                    .flex()
                    .items_start()
                    .gap(px(8.))
                    .px(px(6.))
                    .py(px(4.))
                    .rounded(px(3.))
                    .when(selectable, |d| d.hover(|s| s.bg(rgb(bg_row_hover()))))
                    .when(selectable, |d| {
                        d.on_click(cx.listener(move |this, _, _, cx| {
                            if let Some(imp) = this.presets.import.as_mut() {
                                if let Some(it) = imp.items.get_mut(i) {
                                    it.include = !it.include;
                                }
                            }
                            cx.notify();
                        }))
                    })
                    .child(
                        div()
                            .mt(px(2.))
                            .size(px(12.))
                            .flex_none()
                            .rounded(px(2.))
                            .border_1()
                            .border_color::<Hsla>(if on {
                                rgb(accent_line()).into()
                            } else {
                                border_control()
                            })
                            .when(on, |d| d.bg(rgb(accent_fill())))
                            .flex()
                            .items_center()
                            .justify_center()
                            .text_size(sp(9.))
                            .text_color(rgb(accent_on_fill()))
                            .when(on, |d| d.child("✓")),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .child(
                                div()
                                    .text_size(sp(11.5))
                                    .text_color(rgb(if on { TEXT_PRIMARY } else { TEXT_DIM }))
                                    .child(name),
                            )
                            .when(!detail.is_empty(), |d| {
                                d.child(
                                    div()
                                        .text_size(sp(10.5))
                                        .text_color(rgb(TEXT_DIM))
                                        .child(detail),
                                )
                            }),
                    )
                    .child(
                        div()
                            .flex_none()
                            .text_size(sp(10.5))
                            .text_color(rgb(color))
                            .child(badge),
                    ),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .flex()
                    .flex_wrap()
                    .gap(px(6.))
                    .child(chip(format!("{complete} complete"), accent_line()))
                    .child(chip(format!("{approx} approximate"), 0xC8A15A))
                    .when(unsupported > 0, |d| d.child(chip(format!("{unsupported} unsupported"), 0xE56060)))
                    .when(meta > 0, |d| d.child(chip(format!("{meta} metadata"), TEXT_SECONDARY)))
                    .when(profiles > 0, |d| d.child(chip(format!("{profiles} profiles (not imported)"), TEXT_DIM)))
                    .when(other > 0, |d| d.child(chip(format!("{other} other templates"), TEXT_DIM))),
            )
            .child(div().text_size(sp(11.)).text_color(rgb(TEXT_DIM)).child(
                "Complete presets render fully. Approximate ones apply what Laika can and name the rest. Unsupported ones contain nothing Laika renders, so they start unchecked.",
            ))
            .child(list)
            .when(!imp.errors.is_empty(), |d| {
                d.child(
                    div()
                        .text_size(sp(10.5))
                        .text_color(rgb(0xE0A050))
                        .child(format!(
                            "{} files couldn't be read (e.g. {}: {})",
                            imp.errors.len(),
                            imp.errors[0].0.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default(),
                            imp.errors[0].1
                        )),
                )
            })
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(
                        div()
                            .id("presets-back")
                            .px(px(10.))
                            .py(px(6.))
                            .rounded(px(4.))
                            .border_1()
                            .border_color(border_control())
                            .text_size(sp(11.))
                            .text_color(rgb(TEXT_SECONDARY))
                            .hover(|s| s.bg(rgb(bg_row_hover())))
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(imp) = this.presets.import.as_mut() {
                                    imp.stage = PresetImportStage::Pick;
                                }
                                cx.notify();
                            }))
                            .child("Back"),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .id("presets-import-go")
                            .on_click(cx.listener(|this, _, _, cx| this.preset_import_apply(cx)))
                            .child(button::primary(&format!("Import {chosen} Presets"))),
                    ),
            )
    }
}
