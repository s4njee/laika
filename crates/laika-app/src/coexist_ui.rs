//! S03: working beside Lightroom on the same files — the per-catalog
//! policy, a background watch for sidecars another app changed, the
//! "written by" row, and the conflict screen. Merging lives in
//! `laika_core::catalog` (`merge_external_sidecar`).

use laika_core::catalog::{LightroomPolicy, SideGroup, SidecarConflict};

use super::*;

/// How often shared sidecars are checked for outside changes.
const WATCH_SECS: u64 = 12;

#[derive(Default)]
pub(crate) struct CoexistUi {
    pub policy: LightroomPolicy,
    pub loaded_for: Option<PathBuf>,
    /// photo → (sidecar mtime, "Lightroom Classic 13.0 · 5 min ago").
    pub writer_cache: RefCell<HashMap<i64, (i64, String)>>,
    pub conflict_count: usize,
    pub conflicts_open: bool,
    pub conflicts: Vec<SidecarConflict>,
    pub watching: bool,
    pub scanning: bool,
    /// Writes held back until an outside change is merged first.
    pub deferred: HashSet<i64>,
    pub check_soon: bool,
}

fn ago(secs: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let d = (now - secs).max(0);
    match d {
        0..=59 => "just now".to_string(),
        60..=3599 => format!("{} min ago", d / 60),
        3600..=86_399 => format!("{} h ago", d / 3600),
        _ => laika_core::sidecar::iso_from_epoch(secs)[..10].to_string(),
    }
}

impl Laika {
    /// Per-catalog sharing state (policy, sidecar names, open conflicts).
    pub(crate) fn ensure_coexist(&mut self, cx: &mut Context<Self>) {
        let Some(cat) = self.catalog.as_ref() else {
            return;
        };
        let db = cat.db_path().to_path_buf();
        if self.coexist.loaded_for.as_ref() != Some(&db) {
            self.coexist.policy = cat.lightroom_policy();
            laika_core::sidecar::set_adobe_naming(self.coexist.policy.shares());
            self.coexist.conflict_count = cat.sidecar_conflict_count();
            self.coexist.writer_cache.borrow_mut().clear();
            self.coexist.loaded_for = Some(db);
        }
        if std::mem::take(&mut self.coexist.check_soon) {
            self.check_shared_sidecars(cx);
        }
        if !self.coexist.watching {
            self.coexist.watching = true;
            cx.spawn(async move |entity, cx| {
                loop {
                    cx.background_executor()
                        .timer(std::time::Duration::from_secs(WATCH_SECS))
                        .await;
                    if entity
                        .update(cx, |this, cx| this.check_shared_sidecars(cx))
                        .is_err()
                    {
                        return;
                    }
                }
            })
            .detach();
        }
    }

    pub(crate) fn set_lightroom_policy(&mut self, p: LightroomPolicy, cx: &mut Context<Self>) {
        if self.catalog.is_none() {
            return;
        }
        self.flush_saves();
        let Some(cat) = self.catalog.as_ref() else {
            return;
        };
        cat.set_lightroom_policy(p);
        self.coexist.policy = p;
        laika_core::sidecar::set_adobe_naming(p.shares());
        self.status_note = format!("sidecars: {} — {}", p.label(), p.detail());
        cx.notify();
        if p.shares() {
            self.check_shared_sidecars(cx);
        }
    }

    /// Stat shared sidecars off the UI thread; merge the changed ones.
    pub(crate) fn check_shared_sidecars(&mut self, cx: &mut Context<Self>) {
        if !self.coexist.policy.shares() || self.coexist.scanning || self.import.is_some() {
            if self.coexist.scanning || self.import.is_some() {
                // Try again on the next frame after this scan / import.
                self.coexist.check_soon = !self.coexist.deferred.is_empty();
            }
            return;
        }
        let Some(cat) = self.catalog.as_ref() else {
            return;
        };
        let db = cat.db_path().to_path_buf();
        let list = cat.sidecar_watch_list();
        self.coexist.scanning = true;
        cx.spawn(async move |entity, cx| {
            let changed = cx
                .background_spawn(async move { laika_core::catalog::changed_since(&list) })
                .await;
            entity
                .update(cx, |this, cx| {
                    this.coexist.scanning = false;
                    let same = this
                        .catalog
                        .as_ref()
                        .is_some_and(|c| c.db_path() == db.as_path());
                    if same && !changed.is_empty() {
                        this.merge_changed_sidecars(changed, cx);
                    }
                })
                .ok();
        })
        .detach();
    }

    fn merge_changed_sidecars(&mut self, changed: Vec<(i64, String)>, cx: &mut Context<Self>) {
        // Never merge underneath unsaved edits.
        self.flush_saves();
        let Some(cat) = self.catalog.as_ref() else {
            return;
        };
        let policy = self.coexist.policy;
        let (mut adopted, mut conflicts, mut kept) = (0usize, 0usize, Vec::new());
        let mut develop_changed = Vec::new();
        for (id, path) in &changed {
            match cat.merge_external_sidecar(*id, path, policy) {
                Ok(Some(out)) => {
                    if !out.adopted.is_empty() {
                        adopted += 1;
                    }
                    if out.adopted.contains(&SideGroup::Develop) {
                        develop_changed.push(*id);
                    }
                    conflicts += out.conflicts.len();
                    if out.needs_write() {
                        kept.push(*id);
                    }
                }
                // First contact: adopt whole, as before sharing.
                Ok(None) => match cat.apply_sidecar(*id, path) {
                    Ok(
                        laika_core::catalog::SidecarApply::Applied
                        | laika_core::catalog::SidecarApply::AppliedExternal,
                    ) => {
                        adopted += 1;
                        develop_changed.push(*id);
                    }
                    Ok(_) => {}
                    Err(e) => eprintln!("[sidecar] {e}"),
                },
                Err(e) => eprintln!("[sidecar] merge {path}: {e}"),
            }
        }
        self.coexist.conflict_count = cat.sidecar_conflict_count();
        eprintln!(
            "[sidecar] {} changed outside Laika: {adopted} merged, {conflicts} conflicts, {} to write back",
            changed.len(),
            kept.len()
        );
        self.coexist.writer_cache.borrow_mut().clear();
        if adopted > 0 || conflicts > 0 {
            self.load_edits_from_db();
            self.refresh_photos(cx);
            for id in &develop_changed {
                self.sync_derived(*id, cx);
            }
            if develop_changed
                .iter()
                .any(|id| Some(*id) == self.state.primary)
                && self.state.active_module == Module::Develop
            {
                if let Some(pid) = self.state.primary {
                    self.open_develop_for(pid, cx);
                }
            }
            self.status_note = if conflicts > 0 {
                format!(
                    "{} changed in both apps — review from the top bar",
                    self.coexist.conflict_count
                )
            } else {
                format!(
                    "{adopted} photo{} updated from Lightroom",
                    if adopted == 1 { "" } else { "s" }
                )
            };
        }
        // Laika's own changes (kept, or held back for this merge) go out now.
        let deferred: Vec<i64> = self.coexist.deferred.drain().collect();
        for id in kept.into_iter().chain(deferred) {
            let conflicted = self
                .catalog
                .as_ref()
                .is_some_and(|c| c.has_sidecar_conflict(id));
            if !conflicted {
                self.request_sidecar(id, true);
            }
        }
        cx.notify();
    }

    /// "Lightroom Classic 13.0 · 5 min ago" for the photo's sidecar.
    pub(crate) fn sidecar_writer_label(&self, pid: i64, photo_path: &str) -> String {
        let Some(mtime) = laika_core::xmp::sidecar_mtime(photo_path) else {
            return "none".to_string();
        };
        if let Some((m, label)) = self.coexist.writer_cache.borrow().get(&pid) {
            if *m == mtime {
                return label.clone();
            }
        }
        let label = std::fs::read(laika_core::xmp::sidecar_path(photo_path))
            .ok()
            .and_then(|b| laika_core::sidecar::last_writer(&b))
            .map(|w| format!("{} · {}", w.app, ago(w.when.unwrap_or(mtime))))
            .unwrap_or_else(|| "unreadable".to_string());
        let label = if self
            .catalog
            .as_ref()
            .is_some_and(|c| c.has_sidecar_conflict(pid))
        {
            format!("{label} · conflict")
        } else {
            label
        };
        self.coexist
            .writer_cache
            .borrow_mut()
            .insert(pid, (mtime, label.clone()));
        label
    }

    // ---- conflicts ---------------------------------------------------------------------

    pub(crate) fn open_conflicts(&mut self, cx: &mut Context<Self>) {
        self.close_modals(cx);
        let Some(cat) = self.catalog.as_ref() else {
            return;
        };
        self.coexist.conflicts = cat.sidecar_conflicts();
        self.coexist.conflicts_open = true;
        cx.notify();
    }

    fn resolve_conflicts(
        &mut self,
        which: Vec<(i64, SideGroup)>,
        take_theirs: bool,
        cx: &mut Context<Self>,
    ) {
        let Some(cat) = self.catalog.as_ref() else {
            return;
        };
        let mut done = Vec::new();
        let mut develop = Vec::new();
        for (id, g) in which {
            match cat.resolve_sidecar_conflict(id, g, take_theirs) {
                Ok(clear) => {
                    if clear {
                        done.push(id);
                    }
                    if take_theirs && g == SideGroup::Develop {
                        develop.push(id);
                    }
                }
                Err(e) => self.status_note = e,
            }
        }
        self.coexist.conflicts = cat.sidecar_conflicts();
        self.coexist.conflict_count = cat.sidecar_conflict_count();
        if self.coexist.conflicts.is_empty() {
            self.coexist.conflicts_open = false;
        }
        self.coexist.writer_cache.borrow_mut().clear();
        self.load_edits_from_db();
        self.refresh_photos(cx);
        for id in develop {
            self.sync_derived(id, cx);
        }
        // Photos with nothing left open get Laika's merged sidecar.
        done.sort_unstable();
        done.dedup();
        for id in done {
            self.request_sidecar(id, true);
        }
        cx.notify();
    }

    /// Top-bar pill while photos wait for a decision.
    pub(crate) fn conflicts_pill(&self, cx: &mut Context<Self>) -> Option<Stateful<Div>> {
        (self.coexist.conflict_count > 0).then(|| {
            let n = self.coexist.conflict_count;
            div()
                .id("sidecar-conflicts")
                .mr(px(8.))
                .flex()
                .items_center()
                .gap(px(6.))
                .px(px(10.))
                .py(px(5.))
                .rounded(px(4.))
                .border_1()
                .border_color(rgb(0xC8A15A))
                .font_family(SANS)
                .text_size(sp(10.5))
                .text_color(rgb(0xE0B870))
                .hover(|s| s.bg(rgb(bg_row_hover())))
                .on_hover(self.tip(
                    "Photos changed in both Laika and Lightroom — choose which values to keep",
                ))
                .on_click(cx.listener(|this, _, _, cx| this.open_conflicts(cx)))
                .child(div().size(px(6.)).rounded_full().bg(rgb(0xE0B870)))
                .child(format!("{n} to review"))
        })
    }

    pub(crate) fn conflicts_modal(&self, cx: &mut Context<Self>) -> Div {
        let list = &self.coexist.conflicts;
        let button = |id: ElementId, label: &str, primary: bool| {
            div()
                .id(id)
                .px(px(9.))
                .py(px(5.))
                .rounded(px(4.))
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
        let all: Vec<(i64, SideGroup)> = list.iter().map(|c| (c.photo_id, c.group)).collect();
        let all2 = all.clone();
        let mut rows = div()
            .id("conflict-list")
            .max_h(px(460.))
            .overflow_y_scroll()
            .flex()
            .flex_col()
            .gap(px(8.));
        let mut last_photo = None;
        for (i, c) in list.iter().enumerate() {
            if last_photo != Some(c.photo_id) {
                let name = self
                    .find(c.photo_id)
                    .map(|p| p.filename.clone())
                    .unwrap_or_else(|| format!("photo {}", c.photo_id));
                rows = rows.child(
                    div()
                        .pt(px(6.))
                        .text_size(sp(12.))
                        .text_color(rgb(TEXT_PRIMARY))
                        .child(name),
                );
                last_photo = Some(c.photo_id);
            }
            let (id, g) = (c.photo_id, c.group);
            let side = |label: String, text: String| {
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .child(
                        div()
                            .text_size(sp(10.))
                            .text_color(rgb(TEXT_DIM))
                            .child(label),
                    )
                    .child(
                        div()
                            .text_size(sp(11.))
                            .text_color(rgb(TEXT_SECONDARY))
                            .child(text),
                    )
            };
            rows = rows.child(
                div()
                    .p(px(10.))
                    .rounded(px(5.))
                    .bg(rgb(bg_well()))
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .child(
                        div()
                            .text_size(sp(11.))
                            .text_color(rgb(TEXT_TERTIARY))
                            .child(g.label()),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(14.))
                            .child(side(
                                "In Laika".to_string(),
                                c.ours.describe(g, Some(&c.theirs)),
                            ))
                            .child(side(
                                format!("In the sidecar ({})", c.writer),
                                c.theirs.describe(g, Some(&c.ours)),
                            )),
                    )
                    .child(
                        div()
                            .flex()
                            .gap(px(6.))
                            .child(div().flex_1())
                            .child(
                                button(
                                    ElementId::Name(format!("keep-ours-{i}").into()),
                                    "Keep Laika's",
                                    false,
                                )
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.resolve_conflicts(vec![(id, g)], false, cx)
                                    },
                                )),
                            )
                            .child(
                                button(
                                    ElementId::Name(format!("take-theirs-{i}").into()),
                                    "Use the sidecar's",
                                    false,
                                )
                                .on_click(cx.listener(
                                    move |this, _, _, cx| {
                                        this.resolve_conflicts(vec![(id, g)], true, cx)
                                    },
                                )),
                            ),
                    ),
            );
        }
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
                            .child("Changed in Both Apps"),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .id("conflicts-close")
                            .text_size(sp(15.))
                            .text_color(rgb(TEXT_DIM))
                            .hover(|d| d.text_color(rgb(TEXT_PRIMARY)))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.coexist.conflicts_open = false;
                                cx.notify();
                            }))
                            .child("✕"),
                    ),
            )
            .child(div().text_size(sp(11.5)).text_color(rgb(TEXT_DIM)).child(
                "These fields changed in Laika and in another app since they last agreed. Pick a version for each — nothing else in the sidecar changes, and Laika doesn't rewrite these photos' sidecars until you decide.",
            ))
            .child(rows)
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(button("conflicts-all-ours".into(), "Keep Laika's for All", false).on_click(
                        cx.listener(move |this, _, _, cx| this.resolve_conflicts(all.clone(), false, cx)),
                    ))
                    .child(div().flex_1())
                    .child(button("conflicts-all-theirs".into(), "Use the Sidecars' for All", true).on_click(
                        cx.listener(move |this, _, _, cx| this.resolve_conflicts(all2.clone(), true, cx)),
                    )),
            );
        modal::modal_shell_w(content, 640.)
    }

    /// Preferences → General: the per-catalog sharing policy.
    pub(crate) fn coexist_prefs(&self, cx: &mut Context<Self>) -> Div {
        let now = self.coexist.policy;
        let mut chips = div().flex().flex_wrap().gap(px(6.));
        for (i, p) in LightroomPolicy::ALL.into_iter().enumerate() {
            chips = chips.child(
                div()
                    .id(("lr-policy", i))
                    .px(px(9.))
                    .py(px(5.))
                    .rounded(px(4.))
                    .border_1()
                    .border_color::<Hsla>(if p == now {
                        rgb(accent_line()).into()
                    } else {
                        border_control()
                    })
                    .text_size(sp(11.))
                    .text_color(rgb(if p == now {
                        accent_line()
                    } else {
                        TEXT_SECONDARY
                    }))
                    .hover(|s| s.bg(rgb(bg_row_hover())))
                    .on_click(cx.listener(move |this, _, _, cx| this.set_lightroom_policy(p, cx)))
                    .child(p.label()),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .child(
                div()
                    .text_size(sp(11.5))
                    .text_color(rgb(TEXT_SECONDARY))
                    .child("When Lightroom edits the same photos (this catalog)"),
            )
            .child(chips)
            .child(div().text_size(sp(10.5)).text_color(rgb(TEXT_DIM)).child(format!(
                "{} {}",
                now.detail(),
                if now.shares() {
                    "Raw files use Lightroom's sidecar name (IMG_0001.xmp), settings Laika can't show are kept exactly as Lightroom wrote them, and changes are picked up while Laika is open."
                } else {
                    "Choose one of the other options to share sidecars with Lightroom field by field."
                }
            )))
    }
}
