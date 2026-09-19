//! U15: selective develop copy/paste, relative Quick Develop, Apply Previous,
//! and explicit Auto Sync. Geometry and local adjustments are never implicit.

use laika_core::presets::{SETTING_GROUPS, default_setting_mask};

use super::*;

#[derive(Clone)]
pub(crate) struct CopyDialog {
    pub source_id: i64,
    pub source_name: String,
    pub source: [f32; edit::PARAM_COUNT],
    pub include: [bool; edit::PARAM_COUNT],
    pub target_count: usize,
    pub source_locals: edit::LocalEdits,
    pub include_locals: bool,
}

pub(crate) struct BatchUi {
    pub include: [bool; edit::PARAM_COUNT],
    pub source_id: Option<i64>,
    pub source_name: String,
    pub auto_sync: bool,
    pub copy: Option<CopyDialog>,
    pub source_locals: edit::LocalEdits,
    pub include_locals: bool,
}

impl Default for BatchUi {
    fn default() -> Self {
        Self {
            include: default_setting_mask(),
            source_id: None,
            source_name: String::new(),
            auto_sync: false,
            copy: None,
            source_locals: Default::default(),
            include_locals: false,
        }
    }
}

impl Laika {
    pub(crate) fn quick_adjust_row(&self, param: usize, cx: &mut Context<Self>) -> Div {
        let delta = if param == 2 { 1.0 / 3.0 } else { 5.0 };
        let label = |d: f32| {
            if param == 2 {
                if d < 0. {
                    "−⅓".to_string()
                } else {
                    "+⅓".to_string()
                }
            } else {
                format!("{d:+.0}")
            }
        };
        div()
            .flex()
            .items_center()
            .gap(px(5.))
            .child(
                div()
                    .flex_1()
                    .text_size(sp(9.5))
                    .text_color(rgb(TEXT_DIM))
                    .child(format!("Relative {}", edit::PARAMS[param].label)),
            )
            .children([-delta, delta].into_iter().enumerate().map(|(k, amount)| {
                div()
                    .id(("quick-relative", param * 2 + k))
                    .min_w(px(34.))
                    .px(px(7.))
                    .py(px(3.))
                    .rounded(px(3.))
                    .border_1()
                    .border_color(border_control())
                    .text_size(sp(10.))
                    .text_color(rgb(TEXT_SECONDARY))
                    .text_center()
                    .hover(|s| s.bg(rgb(bg_row_hover())))
                    .on_click(
                        cx.listener(move |this, _, _, cx| this.quick_adjust(param, amount, cx)),
                    )
                    .child(label(amount))
            }))
    }

    pub(crate) fn open_copy_settings(&mut self, pid: i64, cx: &mut Context<Self>) {
        let source = if Some(pid) == self.state.primary {
            self.values
        } else {
            self.state
                .edits
                .get(&pid)
                .map(|e| e.params)
                .unwrap_or_else(edit::defaults)
        };
        let source_name = self
            .find(pid)
            .map(|p| p.filename.clone())
            .unwrap_or_else(|| format!("photo {pid}"));
        self.batch.copy = Some(CopyDialog {
            source_id: pid,
            source_name,
            source,
            include: self.batch.include,
            target_count: self.targets().into_iter().filter(|id| *id != pid).count(),
            source_locals: self
                .state
                .edits
                .get(&pid)
                .map(|e| e.locals.clone())
                .unwrap_or_default(),
            include_locals: false,
        });
        cx.notify();
    }

    fn confirm_copy_settings(&mut self, cx: &mut Context<Self>) {
        let Some(dialog) = self.batch.copy.take() else {
            return;
        };
        let count = dialog.include.iter().filter(|on| **on).count();
        if count == 0 && !dialog.include_locals {
            self.status_note = "choose at least one setting to copy".to_string();
            self.batch.copy = Some(dialog);
            cx.notify();
            return;
        }
        self.state.clipboard = Some(dialog.source);
        self.batch.include = dialog.include;
        self.batch.source_id = Some(dialog.source_id);
        self.batch.source_name = dialog.source_name.clone();
        self.batch.source_locals = dialog.source_locals;
        self.batch.include_locals = dialog.include_locals;
        self.status_note = format!(
            "copied {count} settings from {} — {} target{} selected",
            dialog.source_name,
            dialog.target_count,
            if dialog.target_count == 1 { "" } else { "s" }
        );
        cx.notify();
    }

    pub(crate) fn copy_settings_modal(&self, cx: &mut Context<Self>) -> Div {
        let Some(dialog) = self.batch.copy.as_ref() else {
            return div();
        };
        let mut groups = div().flex().flex_col().gap(px(5.));
        for (index, group) in SETTING_GROUPS.iter().enumerate() {
            let on = group.range.clone().all(|i| dialog.include[i]);
            groups = groups.child(
                div()
                    .id(("copy-setting-group", index))
                    .flex()
                    .items_center()
                    .gap(px(9.))
                    .px(px(8.))
                    .py(px(6.))
                    .rounded(px(4.))
                    .hover(|s| s.bg(rgb(bg_row_hover())))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if let Some(copy) = this.batch.copy.as_mut() {
                            for i in SETTING_GROUPS[index].range.clone() {
                                copy.include[i] = !on;
                            }
                        }
                        cx.notify();
                    }))
                    .child(toggle::toggle(on, ""))
                    .child(
                        div()
                            .flex_1()
                            .text_size(sp(12.))
                            .text_color(rgb(TEXT_SECONDARY))
                            .child(group.name),
                    )
                    .when(!group.default_on, |d| {
                        d.child(
                            div()
                                .text_size(sp(10.))
                                .text_color(rgb(0xC8A15A))
                                .child("off by default"),
                        )
                    }),
            );
        }
        let included = dialog.include.iter().filter(|on| **on).count();
        modal::modal_shell_w(
            div()
                .p(px(24.))
                .flex()
                .flex_col()
                .gap(px(14.))
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
                                .child("Copy Settings"),
                        )
                        .child(
                            div()
                                .id("copy-settings-close")
                                .text_color(rgb(TEXT_DIM))
                                .hover(|s| s.text_color(rgb(TEXT_PRIMARY)))
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.batch.copy = None;
                                    cx.notify();
                                }))
                                .child("✕"),
                        ),
                )
                .child(
                    div()
                        .p(px(10.))
                        .rounded(px(4.))
                        .bg(rgb(bg_well()))
                        .text_size(sp(11.5))
                        .text_color(rgb(TEXT_SECONDARY))
                        .child(format!(
                            "Source: {}  ·  Targets: {}",
                            dialog.source_name, dialog.target_count
                        )),
                )
                .child(groups)
                .child(
                    div()
                        .id("copy-local-masks")
                        .flex()
                        .items_center()
                        .gap(px(9.))
                        .px(px(8.))
                        .py(px(6.))
                        .rounded(px(4.))
                        .hover(|s| s.bg(rgb(bg_row_hover())))
                        .on_click(cx.listener(|this, _, _, cx| {
                            if let Some(copy) = this.batch.copy.as_mut() {
                                copy.include_locals = !copy.include_locals;
                            }
                            cx.notify();
                        }))
                        .child(toggle::toggle(dialog.include_locals, ""))
                        .child(
                            div()
                                .flex_1()
                                .text_size(sp(11.5))
                                .text_color(rgb(TEXT_SECONDARY))
                                .child("Local masks and spot heals (explicit)"),
                        )
                        .child(
                            div()
                                .text_size(sp(10.))
                                .text_color(rgb(0xC8A15A))
                                .child("off by default"),
                        ),
                )
                .child(
                    div()
                        .text_size(sp(10.5))
                        .text_color(rgb(TEXT_DIM))
                        .child("Crop is never included. Transform and locals are opt-in."),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .child(
                            div()
                                .flex_1()
                                .text_size(sp(11.))
                                .text_color(rgb(TEXT_DIM))
                                .child(format!("{included} of {} settings", edit::PARAM_COUNT)),
                        )
                        .child(
                            div()
                                .id("copy-settings-confirm")
                                .on_click(
                                    cx.listener(|this, _, _, cx| this.confirm_copy_settings(cx)),
                                )
                                .child(button::primary("Copy")),
                        ),
                ),
            520.,
        )
    }

    /// Apply a delta to every selected photo. Each photo retains its own base.
    pub(crate) fn quick_adjust(&mut self, param: usize, delta: f32, cx: &mut Context<Self>) {
        let ids = self.targets();
        if ids.is_empty() || param >= edit::PARAM_COUNT {
            self.status_note = "no visible photos targeted".to_string();
            cx.notify();
            return;
        }
        let before: Vec<_> = ids
            .iter()
            .map(|pid| (*pid, self.snap_current(*pid)))
            .collect();
        for pid in &ids {
            let e = self.state.edit(*pid);
            let d = edit::PARAMS[param];
            e.params[param] = (e.params[param] + delta).clamp(d.min, d.max);
        }
        for (pid, snap) in before {
            self.record_step(pid, "Quick Develop", edit::PARAMS[param].label, snap);
        }
        let failed = self.finish_batch_edit(&ids, cx);
        self.status_note = if failed == 0 {
            format!(
                "{} {:+} on {} photo{}",
                edit::PARAMS[param].label,
                delta,
                ids.len(),
                if ids.len() == 1 { "" } else { "s" }
            )
        } else {
            format!(
                "updated {} photo{} with {failed} save failure{} — retry available",
                ids.len(),
                if ids.len() == 1 { "" } else { "s" },
                if failed == 1 { "" } else { "s" }
            )
        };
        cx.notify();
    }

    pub(crate) fn apply_previous_settings(&mut self, cx: &mut Context<Self>) {
        let Some(primary) = self.state.primary else {
            self.status_note = "select a photo first".to_string();
            cx.notify();
            return;
        };
        let order = self.ordered_ids();
        let Some(pos) = order.iter().position(|id| *id == primary) else {
            return;
        };
        let Some(source_id) = pos.checked_sub(1).and_then(|i| order.get(i)).copied() else {
            self.status_note = "the first photo has no previous settings".to_string();
            cx.notify();
            return;
        };
        let source = self
            .state
            .edits
            .get(&source_id)
            .map(|e| e.params)
            .unwrap_or_else(edit::defaults);
        self.state.clipboard = Some(source);
        self.batch.source_id = Some(source_id);
        self.batch.source_name = self
            .find(source_id)
            .map(|p| p.filename.clone())
            .unwrap_or_default();
        self.paste_settings(cx);
    }

    pub(crate) fn toggle_auto_sync(&mut self, cx: &mut Context<Self>) {
        self.batch.auto_sync = !self.batch.auto_sync;
        self.status_note = if self.batch.auto_sync {
            format!(
                "Auto Sync on — edits mirror to {} targets",
                self.targets().len().saturating_sub(1)
            )
        } else {
            "Auto Sync off".to_string()
        };
        cx.notify();
    }

    pub(crate) fn auto_sync_after_primary(
        &mut self,
        source: i64,
        before_source: [f32; edit::PARAM_COUNT],
        label: &str,
        value: &str,
        cx: &mut Context<Self>,
    ) {
        if !self.batch.auto_sync {
            return;
        }
        let changed: Vec<usize> = (0..edit::PARAM_COUNT)
            .filter(|i| self.batch.include[*i] && self.values[*i] != before_source[*i])
            .collect();
        if changed.is_empty() {
            return;
        }
        let source_values = self.values;
        let others: Vec<i64> = self
            .targets()
            .into_iter()
            .filter(|pid| *pid != source)
            .collect();
        let mut failed = 0usize;
        for pid in &others {
            let before = self.snap_current(*pid);
            let target = self.state.edit(*pid);
            for i in &changed {
                target.params[*i] = source_values[*i];
            }
            self.record_step(*pid, label, value, before);
            if !self.persist_photo(*pid) {
                failed += 1;
            }
            self.request_sidecar(*pid, false);
            self.sync_derived(*pid, cx);
        }
        if !others.is_empty() {
            self.last_batch = std::iter::once(source).chain(others).collect();
        }
        if failed > 0 {
            self.status_note = format!(
                "Auto Sync updated with {failed} save failure{} — retry available",
                if failed == 1 { "" } else { "s" }
            );
        }
    }

    fn finish_batch_edit(&mut self, ids: &[i64], cx: &mut Context<Self>) -> usize {
        self.last_batch = ids.to_vec();
        self.last_was_meta = false;
        self.last_was_remove = false;
        self.redo_batch.clear();
        if let Some(pid) = self.state.primary.filter(|pid| ids.contains(pid)) {
            self.values = self.state.edit(pid).params;
        }
        self.edits_dirty = true;
        let mut failed = 0usize;
        for pid in ids {
            if !self.persist_photo(*pid) {
                failed += 1;
            }
            self.request_sidecar(*pid, false);
            self.sync_derived(*pid, cx);
        }
        if !self.sidecar_pending.is_empty() {
            self.kick_save_timer(cx);
        }
        self.submit_dev(cx);
        self.pump_sync(cx);
        failed
    }
}
