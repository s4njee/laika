//! Apple Photos sync: settings dialog, left-rail status, background runs.
//! Photos becomes the home of each synced original; Laika's own file goes
//! to the Trash once Photos' copy is verified.
//! Planning and bookkeeping live in `laika_core::apple_photos`; this file
//! only drives them. Catalog work stays on the UI thread, and only the
//! osascript batches run in the background.

use super::*;
use laika_core::apple_photos::{self as ap, FavoriteRule, Outcome, PhotosSettings, Scope};

/// Jobs per osascript run: small enough for steady progress and cheap
/// interruption, large enough to keep one Photos session warm.
const BATCH: usize = 8;
/// Automatic sync check interval.
const AUTO_SECS: u64 = 45;

#[derive(Default)]
pub(crate) struct AppleSync {
    pub settings: PhotosSettings,
    pub open: bool,
    pub running: bool,
    pub done: usize,
    pub total: usize,
    /// Catalog photos whose original now lives in Photos.
    pub linked: usize,
    /// Jobs waiting (imports + updates).
    pub pending: usize,
    pub failed: usize,
    /// Originals moved into Photos this run.
    pub moved: usize,
    /// Originals Photos stored but Laika couldn't verify (kept in Laika).
    pub kept: usize,
    pub error: String,
    pub last_ok: Option<Instant>,
    /// Photos that failed this session; automatic runs skip them so an
    /// unsupported file doesn't relaunch Photos every interval.
    pub skip: HashSet<i64>,
    pub loop_started: bool,
    /// What a running sync is doing (Photos → Laika phases).
    pub phase: String,
    /// Mirrored Photos folder/album tree.
    pub albums: Vec<laika_core::catalog::PhotosAlbumNode>,
    /// Lazily loaded members of the album being filtered.
    pub album_members: RefCell<Option<(String, HashSet<i64>)>>,
    /// Collapsed Photos folders in the sidebar.
    pub collapsed: HashSet<String>,
    pub pending_ingest: Option<crate::photos_ingest::PendingIngest>,
    pub last_ingest: Option<Instant>,
    /// Last library read: originals only in iCloud, unsupported files.
    pub not_local: usize,
    pub unsupported: usize,
    /// Library files that failed to import this session; automatic runs
    /// skip them (a manual sync retries).
    pub failed_paths: HashSet<String>,
    /// When the automatic loop started (no background reads right after
    /// launch) and when it last looked for library changes.
    pub loop_started_at: Option<Instant>,
    pub last_check: Option<Instant>,
    /// A background change check is in flight.
    pub checking: bool,
    /// Held while a sync runs (no App Nap throttling).
    pub awake: Option<crate::activity::KeepAwake>,
}

impl Laika {
    pub(crate) fn load_apple_sync(&mut self) {
        let settings = self
            .catalog
            .as_ref()
            .map(|c| c.photos_settings())
            .unwrap_or_default();
        // A run in flight stops itself at the catalog check; keep its
        // flag so a second run can't overlap it.
        self.apple = AppleSync {
            settings,
            loop_started: self.apple.loop_started,
            running: self.apple.running,
            awake: self.apple.awake.take(),
            collapsed: std::mem::take(&mut self.apple.collapsed),
            failed_paths: std::mem::take(&mut self.apple.failed_paths),
            loop_started_at: self.apple.loop_started_at,
            ..Default::default()
        };
        self.reload_apple_albums();
        self.refresh_apple_counts();
    }

    fn apple_plan(&self) -> Vec<ap::Job> {
        let Some(cat) = self.catalog.as_ref() else {
            return Vec::new();
        };
        ap::plan(
            &self.apple_candidates(),
            &cat.photos_links(),
            &cat.photos_forced(),
            &self.apple.settings,
        )
    }

    /// Planner input from the library already in memory — no catalog
    /// re-read, no per-photo stat (the offline probe already knows), one
    /// root lookup. This runs on the UI thread on every automatic check.
    fn apple_candidates(&self) -> Vec<ap::Candidate> {
        let root = self
            .catalog
            .as_ref()
            .map(|c| c.root_path())
            .unwrap_or_default();
        self.photos
            .iter()
            .map(|p| {
                let folder = laika_core::catalog::Catalog::folder_under(&p.path, &root);
                ap::Candidate {
                    id: p.id,
                    path: p.path.clone(),
                    folder: if folder == "(root)" {
                        String::new()
                    } else {
                        folder
                    },
                    is_raw: p.is_raw(),
                    is_video: p.duration_ms > 0
                        || laika_raw::media_kind(std::path::Path::new(&p.path))
                            == Some(laika_raw::MediaKind::Video),
                    exists: !self.offline.contains(&p.id),
                    blake3: p.blake3.clone(),
                    rating: p.rating,
                    picked: p.picked,
                    rejected: p.rejected,
                    title: p.title.clone(),
                    caption: p.caption.clone(),
                    keywords: self.photo_keywords.get(&p.id).cloned().unwrap_or_default(),
                }
            })
            .collect()
    }

    pub(crate) fn refresh_apple_counts(&mut self) {
        if self.catalog.is_none() {
            return;
        }
        self.apple.linked = self
            .photos
            .iter()
            .filter(|p| ap::in_library(&p.path))
            .count();
        self.apple.pending = if self.apple.settings.enabled {
            self.apple_plan().len()
        } else {
            0
        };
    }

    pub(crate) fn open_apple(&mut self, cx: &mut Context<Self>) {
        self.close_modals(cx);
        self.return_focus = self.focused_param;
        self.apple.open = true;
        self.refresh_apple_counts();
        cx.notify();
    }

    fn update_apple_settings(
        &mut self,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut PhotosSettings),
    ) {
        change(&mut self.apple.settings);
        if let Some(cat) = self.catalog.as_ref() {
            cat.set_photos_settings(&self.apple.settings);
        }
        self.refresh_apple_counts();
        cx.notify();
    }

    pub(crate) fn commit_apple_album(
        &mut self,
        buf: &str,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let name = buf.trim();
        if name.is_empty() {
            return Err("give the album a name".to_string());
        }
        if name.chars().count() > 80 {
            return Err("keep the album name under 80 characters".to_string());
        }
        let name = name.to_string();
        self.update_apple_settings(cx, |s| s.album = name);
        Ok(())
    }

    /// Right-click "Add to Apple Photos": sync these regardless of scope.
    pub(crate) fn add_to_apple(&mut self, ids: Vec<i64>, cx: &mut Context<Self>) {
        if let Some(cat) = self.catalog.as_ref() {
            cat.add_photos_forced(&ids);
        }
        for id in &ids {
            self.apple.skip.remove(id);
        }
        if !self.apple.settings.enabled {
            self.open_apple(cx);
            return;
        }
        self.status_note = format!(
            "adding {} photo{} to Apple Photos",
            ids.len(),
            if ids.len() == 1 { "" } else { "s" }
        );
        self.start_apple_push(true, cx);
    }

    /// Laika → Photos: run every pending job. `manual` retries photos that
    /// failed earlier this session.
    pub(crate) fn start_apple_push(&mut self, manual: bool, cx: &mut Context<Self>) {
        if self.apple.running || self.catalog.is_none() || !self.apple.settings.enabled {
            return;
        }
        if manual {
            self.apple.skip.clear();
        }
        let skip = self.apple.skip.clone();
        let jobs: Vec<ap::Job> = self
            .apple_plan()
            .into_iter()
            .filter(|j| !j.photo_ids.iter().any(|id| skip.contains(id)))
            .collect();
        self.apple.error.clear();
        self.apple.failed = 0;
        self.apple.moved = 0;
        self.apple.kept = 0;
        if jobs.is_empty() {
            self.apple.last_ok = Some(Instant::now());
            self.refresh_apple_counts();
            cx.notify();
            return;
        }
        let db = self
            .catalog
            .as_ref()
            .map(|c| c.db_path().to_path_buf())
            .unwrap_or_default();
        self.apple.running = true;
        self.apple.awake = Some(crate::activity::KeepAwake::begin(
            "Syncing with Apple Photos",
        ));
        if manual {
            self.hud_hidden = false;
        }
        self.apple.done = 0;
        self.apple.total = jobs.len();
        eprintln!("[photos] syncing {} job(s)", jobs.len());
        cx.notify();
        cx.spawn(async move |entity, cx| {
            for chunk in jobs.chunks(BATCH) {
                let batch = chunk.to_vec();
                let run = batch.clone();
                let res = cx
                    .background_spawn(async move { ap::run_blocking(&run) })
                    .await;
                let keep_going = entity
                    .update(cx, |this, cx| {
                        let same_catalog = this
                            .catalog
                            .as_ref()
                            .is_some_and(|c| c.db_path() == db.as_path());
                        if !same_catalog {
                            return false;
                        }
                        this.apple.done += batch.len();
                        let ok = match res {
                            Ok(outcomes) => this.record_apple_batch(&batch, outcomes),
                            Err(e) => {
                                eprintln!("[photos] run failed: {e}");
                                this.apple.error = e;
                                false
                            }
                        };
                        cx.notify();
                        ok
                    })
                    .unwrap_or(false);
                if !keep_going {
                    break;
                }
            }
            entity
                .update(cx, |this, cx| {
                    this.apple.running = false;
                    this.apple.awake = None;
                    let plural = |n: usize| if n == 1 { "" } else { "s" };
                    if this.apple.error.is_empty() {
                        this.apple.last_ok = Some(Instant::now());
                        if this.apple.failed > 0 {
                            this.apple.error = format!(
                                "{} photo{} couldn't be added — see the log for details",
                                this.apple.failed,
                                plural(this.apple.failed)
                            );
                        } else if this.apple.kept > 0 {
                            this.apple.error = format!(
                                "{} original{} couldn't be verified in Photos and stayed in Laika",
                                this.apple.kept,
                                plural(this.apple.kept)
                            );
                        }
                    }
                    if this.apple.moved > 0 {
                        this.status_note = format!(
                            "moved {} original{} into Apple Photos (Laika's files are in the Trash)",
                            this.apple.moved,
                            plural(this.apple.moved)
                        );
                        this.refresh_photos(cx);
                    }
                    this.refresh_apple_counts();
                    cx.notify();
                })
                .ok();
        })
        .detach();
    }

    /// Save links and hand verified originals over to Photos. Returns
    /// false when the run must stop (Photos isn't copying files).
    fn record_apple_batch(&mut self, batch: &[ap::Job], outcomes: Vec<Outcome>) -> bool {
        let Some(cat) = self.catalog.as_ref() else {
            return false;
        };
        let mut trash: Vec<PathBuf> = Vec::new();
        let mut keep_going = true;
        for (job, outcome) in batch.iter().zip(outcomes) {
            match outcome {
                Outcome::Done(ids) => {
                    if let Err(e) = cat.record_photos_job(job, &ids) {
                        eprintln!("[photos] link not saved: {e}");
                    }
                }
                Outcome::Placed {
                    ids,
                    originals,
                    copied,
                } => {
                    if let Err(e) = cat.record_photos_job(job, &ids) {
                        eprintln!("[photos] link not saved: {e}");
                    }
                    if !copied {
                        // Referenced import: Photos points at Laika's file.
                        // Keep it and stop before any more are referenced.
                        self.apple.error = "Photos didn't copy the photo into its library. In \
                             Photos › Settings › General, turn on “Copy items to the Photos \
                             library”, then sync again."
                            .to_string();
                        keep_going = false;
                        continue;
                    }
                    for ((id, old), new) in job.photo_ids.iter().zip(&job.paths).zip(originals) {
                        if ap::in_library(old) {
                            continue;
                        }
                        let Some(new) = new else {
                            eprintln!("[photos] couldn't verify Photos' copy of {old}; kept");
                            self.apple.kept += 1;
                            self.apple.skip.insert(*id);
                            continue;
                        };
                        match cat.adopt_photos_original(*id, std::path::Path::new(&new)) {
                            Ok(()) => {
                                self.apple.moved += 1;
                                trash.push(PathBuf::from(old));
                                let side = laika_core::xmp::sidecar_path(old);
                                if std::path::Path::new(&side).exists() {
                                    trash.push(PathBuf::from(side));
                                }
                            }
                            Err(e) => {
                                eprintln!("[photos] couldn't adopt {new}: {e}");
                                self.apple.kept += 1;
                                self.apple.skip.insert(*id);
                            }
                        }
                    }
                }
                Outcome::Missing => {
                    // Deleted in Photos: forget it. Originals still outside
                    // Photos are imported again on the next run.
                    cat.forget_photos_links(&job.photo_ids);
                }
                Outcome::Failed(msg) => {
                    eprintln!("[photos] {:?} failed: {msg}", job.paths);
                    self.apple.failed += 1;
                    self.apple.skip.extend(job.photo_ids.iter().copied());
                }
            }
        }
        // The catalog already points at Photos' copies, so a failed trash
        // only leaves a stray file behind — never a missing original.
        trash.sort();
        trash.dedup();
        if !trash.is_empty() {
            if let Err(e) = laika_core::import::trash_files(&trash) {
                eprintln!("[photos] moving Laika's copies to the Trash failed: {e}");
                self.status_note = format!("couldn't move Laika's copies to the Trash: {e}");
            }
        }
        keep_going
    }

    /// Automatic sync: check for work every `AUTO_SECS` while enabled.
    pub(crate) fn kick_apple_loop(&mut self, cx: &mut Context<Self>) {
        if self.apple.loop_started {
            return;
        }
        self.apple.loop_started = true;
        self.apple.loop_started_at = Some(Instant::now());
        cx.spawn(async move |entity, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_secs(AUTO_SECS))
                    .await;
                let alive = entity
                    .update(cx, |this, cx| {
                        if this.apple.running {
                            return;
                        }
                        let s = &this.apple.settings;
                        let quiet = this.import.is_none() && this.apple.error.is_empty();
                        if quiet && this.apple_check_due() {
                            this.check_library_changes(cx);
                        } else if s.enabled && s.auto && quiet {
                            this.start_apple_push(false, cx);
                        } else if s.enabled {
                            this.refresh_apple_counts();
                            cx.notify();
                        }
                    })
                    .is_ok();
                if !alive {
                    break;
                }
            }
        })
        .detach();
    }

    fn apple_status(&self) -> (String, bool) {
        let a = &self.apple;
        if !a.settings.enabled && !self.apple_ingest_on() {
            ("Off".to_string(), false)
        } else if a.running && !a.phase.is_empty() {
            (a.phase.clone(), false)
        } else if a.running {
            (
                format!("Syncing {} of {}", a.done.min(a.total), a.total),
                false,
            )
        } else if !a.error.is_empty() {
            (a.error.clone(), true)
        } else if a.pending > 0 {
            (
                format!("{} in Photos · {} to sync", a.linked, a.pending),
                false,
            )
        } else {
            match a.last_ok {
                Some(t) => (
                    format!("{} in Photos · synced {}", a.linked, Self::relative_time(t)),
                    false,
                ),
                None => (format!("{} in Photos", a.linked), false),
            }
        }
    }

    /// Compact block under Backup in the left rail footer.
    pub(crate) fn apple_footer(&self, cx: &mut Context<Self>) -> Div {
        let _p = crate::ProfSpan("apple_footer", Instant::now());
        let (status, error) = self.apple_status();
        let a = &self.apple;
        let small = |id: &'static str, label: &'static str| {
            div()
                .id(id)
                .px(px(8.))
                .py(px(4.))
                .rounded(px(3.))
                .border_1()
                .border_color(border_control())
                .font_family(SANS)
                .text_size(px(10.))
                .text_color(rgb(TEXT_SECONDARY))
                .hover(|s| s.bg(rgb(bg_row_hover())))
                .child(label)
        };
        div()
            .pt(px(9.))
            .border_t_1()
            .border_color(hairline())
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .flex()
                    .items_baseline()
                    .justify_between()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_none()
                            .text_size(px(12.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(TEXT_SECONDARY))
                            .child("Apple Photos"),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .text_size(px(11.))
                            .text_color(rgb(if error { 0xE56060 } else { TEXT_DIM }))
                            .child(status),
                    ),
            )
            .when(a.running, |d| {
                d.child(
                    div().h(px(3.)).rounded(px(2.)).bg(rgb(track())).child(
                        div()
                            .h(px(3.))
                            .w(relative(a.done as f32 / a.total.max(1) as f32))
                            .rounded(px(2.))
                            .bg(rgb(progress_fill())),
                    ),
                )
            })
            .child(
                div()
                    .flex()
                    .gap(px(6.))
                    .when(
                        (a.settings.enabled || self.apple_ingest_on()) && !a.running,
                        |d| {
                            d.child(small("apple-sync-now", "Sync now").on_click(
                                cx.listener(|this, _, _, cx| this.start_apple_sync(true, cx)),
                            ))
                        },
                    )
                    .child(
                        small(
                            "apple-settings",
                            if a.settings.enabled || self.apple_ingest_on() {
                                "Settings"
                            } else {
                                "Set up"
                            },
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            if this.apple.open {
                                this.close_modals(cx);
                            } else {
                                this.open_apple(cx);
                            }
                        })),
                    ),
            )
    }

    pub(crate) fn apple_modal(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let s = &self.apple.settings;
        let a = &self.apple;
        let chip = |id: (&'static str, usize), label: String, on: bool| {
            div()
                .id(id)
                .px(px(12.))
                .py(px(5.))
                .rounded(px(4.))
                .border_1()
                .border_color::<Hsla>(if on {
                    rgb(accent_line()).into()
                } else {
                    border_control()
                })
                .text_size(px(12.))
                .text_color(rgb(if on { TEXT_PRIMARY } else { TEXT_MUTED }))
                .hover(|st| st.bg(rgb(bg_row_hover())))
                .child(label)
        };
        let row = |label: &'static str| {
            div().flex().items_center().gap(px(8.)).child(
                div()
                    .w(px(120.))
                    .flex_none()
                    .text_size(px(12.))
                    .text_color(rgb(TEXT_DIM))
                    .child(label),
            )
        };
        let hint = |text: &'static str| {
            div()
                .text_size(px(11.5))
                .line_height(relative(1.4))
                .text_color(rgb(TEXT_DIM))
                .child(text)
        };
        let section = |label: &'static str| {
            div()
                .pt(px(4.))
                .text_size(px(11.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(rgb(TEXT_MUTED))
                .child(label)
        };

        // Step 1: the Photos setting that makes imports references.
        let confirmed = s.enabled;
        let setup = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .p(px(12.))
            .rounded(px(6.))
            .bg(rgb(bg_well()))
            .border_1()
            .border_color::<Hsla>(if confirmed {
                hairline()
            } else {
                rgb(WARNING).into()
            })
            .child(
                div()
                    .text_size(px(12.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(rgb(TEXT_PRIMARY))
                    .child("Photos keeps the only copy"),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .line_height(relative(1.45))
                    .text_color(rgb(TEXT_SECONDARY))
                    .child(
                        "Laika imports each photo into Photos, checks that Photos' copy matches \
                         the original byte for byte, then uses that copy and moves its own file \
                         to the Trash. Anything that doesn't match stays in Laika. Before you \
                         start: keep “Copy items to the Photos library” on in Photos › Settings › \
                         General (the default), and if iCloud Photos is on, choose “Download \
                         Originals to this Mac”. Laika needs Full Disk Access to read the \
                         library and checks for it before moving anything.",
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .child(self.check_row(
                        ("apple-confirm", 0),
                        "Move originals into Photos",
                        confirmed,
                        "Nothing is sent to Photos until this is on",
                        cx,
                        |this, cx| {
                            this.update_apple_settings(cx, |s| s.enabled = !s.enabled);
                            if this.apple.settings.enabled {
                                this.apple.error.clear();
                            }
                        },
                    ))
                    .child(
                        div()
                            .id("apple-open-photos")
                            .flex_none()
                            .px(px(10.))
                            .py(px(5.))
                            .rounded(px(4.))
                            .border_1()
                            .border_color(border_control())
                            .text_size(px(12.))
                            .text_color(rgb(TEXT_SECONDARY))
                            .hover(|st| st.bg(rgb(bg_row_hover())))
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Err(e) = std::process::Command::new("/usr/bin/open")
                                    .args(["-a", "Photos"])
                                    .spawn()
                                {
                                    this.apple.error = format!("couldn't open Photos: {e}");
                                    cx.notify();
                                }
                            }))
                            .child("Open Photos"),
                    ),
            );

        let scope = row("Photos to sync")
            .child(
                chip(("apple-scope", 0), "All".into(), s.scope == Scope::All).on_click(
                    cx.listener(|this, _, _, cx| {
                        this.update_apple_settings(cx, |s| s.scope = Scope::All)
                    }),
                ),
            )
            .child(
                chip(
                    ("apple-scope", 1),
                    "Picked".into(),
                    s.scope == Scope::Picked,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.update_apple_settings(cx, |s| s.scope = Scope::Picked)
                })),
            )
            .child(
                chip(("apple-scope", 2), "Rated".into(), s.scope == Scope::Rated).on_click(
                    cx.listener(|this, _, _, cx| {
                        this.update_apple_settings(cx, |s| s.scope = Scope::Rated)
                    }),
                ),
            );
        let mut stars = row("At least");
        for n in 1..=5u8 {
            stars = stars.child(
                chip(
                    ("apple-stars", n as usize),
                    format!("{n}\u{2605}"),
                    s.min_rating.clamp(1, 5) == n,
                )
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.update_apple_settings(cx, |s| s.min_rating = n)
                })),
            );
        }
        let albums = row("Albums")
            .child(
                chip(("apple-albums", 0), "One album".into(), !s.by_folder).on_click(cx.listener(
                    |this, _, _, cx| this.update_apple_settings(cx, |s| s.by_folder = false),
                )),
            )
            .child(
                chip(("apple-albums", 1), "One per folder".into(), s.by_folder).on_click(
                    cx.listener(|this, _, _, cx| {
                        this.update_apple_settings(cx, |s| s.by_folder = true)
                    }),
                ),
            );
        let favorites = row("Favorites")
            .child(
                chip(
                    ("apple-fav", 0),
                    "Picked".into(),
                    s.favorites == FavoriteRule::Picked,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.update_apple_settings(cx, |s| s.favorites = FavoriteRule::Picked)
                })),
            )
            .child(
                chip(
                    ("apple-fav", 1),
                    "5 stars".into(),
                    s.favorites == FavoriteRule::FiveStars,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.update_apple_settings(cx, |s| s.favorites = FavoriteRule::FiveStars)
                })),
            )
            .child(
                chip(
                    ("apple-fav", 2),
                    "Don't set".into(),
                    s.favorites == FavoriteRule::Off,
                )
                .on_click(cx.listener(|this, _, _, cx| {
                    this.update_apple_settings(cx, |s| s.favorites = FavoriteRule::Off)
                })),
            );

        let (status, error) = self.apple_status();
        let ingest_on = s.ingest_all || s.mirror_albums;
        let can_sync = (confirmed || ingest_on) && !a.running;
        let footer =
            div()
                .flex()
                .items_center()
                .justify_between()
                .gap(px(12.))
                .pt(px(4.))
                .child(
                    div()
                        .min_w_0()
                        .text_size(px(12.))
                        .line_height(relative(1.4))
                        .text_color(rgb(if error { 0xE56060 } else { TEXT_TERTIARY }))
                        .child(status),
                )
                .child(
                    div()
                        .flex()
                        .flex_none()
                        .gap(px(8.))
                        .child(
                            div()
                                .id("apple-close")
                                .px(px(12.))
                                .py(px(6.))
                                .rounded(px(5.))
                                .border_1()
                                .border_color(border_control())
                                .text_size(px(12.))
                                .text_color(rgb(TEXT_SECONDARY))
                                .hover(|st| st.bg(rgb(bg_row_hover())))
                                .on_click(cx.listener(|this, _, _, cx| this.close_modals(cx)))
                                .child("Close"),
                        )
                        .child(
                            div()
                                .id("apple-sync")
                                .px(px(12.))
                                .py(px(6.))
                                .rounded(px(5.))
                                .text_size(px(12.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .when(can_sync, |d| {
                                    d.bg(rgb(accent_fill()))
                                        .text_color(rgb(accent_on_fill()))
                                        .hover(|st| st.bg(rgb(accent_fill_hover())))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.start_apple_sync(true, cx)
                                        }))
                                })
                                .when(!can_sync, |d| {
                                    d.bg(rgb(bg_segment_active())).text_color(rgb(TEXT_DIMMER))
                                })
                                .child(if a.running {
                                    "Syncing…".to_string()
                                } else if ingest_on && confirmed {
                                    "Sync with Photos".to_string()
                                } else if ingest_on {
                                    "Bring in from Photos".to_string()
                                } else if a.pending > 0 {
                                    format!("Sync {} to Photos", a.pending)
                                } else {
                                    "Sync now".to_string()
                                }),
                        ),
                );

        let max_h = (window.viewport_size().height.as_f32() - 60.).max(320.);
        modal::modal_shell_w(
            div()
                .id("apple-dialog")
                .max_h(px(max_h))
                .overflow_y_scroll()
                .flex()
                .flex_col()
                .gap(px(10.))
                .p(px(22.))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(3.))
                        .child(
                            div()
                                .text_size(px(15.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(rgb(TEXT_PRIMARY))
                                .child("Apple Photos"),
                        )
                        .child(hint(
                            "Photos is the home for your originals. Laika brings its photos and \
                             albums in without copying them, and can move Laika's own originals \
                             into Photos.",
                        )),
                )
                .child(section("From Photos"))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .child(self.check_row(
                            ("apple-ingest", 0),
                            "Bring in every photo from Photos (no copies)",
                            s.ingest_all,
                            "Laika catalogs the originals inside your Photos library",
                            cx,
                            |this, cx| {
                                this.update_apple_settings(cx, |s| s.ingest_all = !s.ingest_all)
                            },
                        ))
                        .child(self.check_row(
                            ("apple-albums-mirror", 0),
                            "Recreate Photos albums and folders in the sidebar",
                            s.mirror_albums,
                            "Albums stay managed in Photos; Laika mirrors them",
                            cx,
                            |this, cx| {
                                this.update_apple_settings(cx, |s| {
                                    s.mirror_albums = !s.mirror_albums
                                })
                            },
                        )),
                )
                .when(ingest_on, |d| {
                    d.child(hint(
                        "Photos' title, caption, keywords and favorite (as a pick) come along. \
                         Laika needs Full Disk Access to read the library, and originals that \
                         are only in iCloud are skipped — choose “Download Originals to this \
                         Mac” in Photos to include them.",
                    ))
                })
                .when(a.not_local + a.unsupported > 0, |d| {
                    d.child(
                        div()
                            .text_size(px(11.5))
                            .text_color(rgb(WARNING))
                            .child(format!(
                                "Last read skipped {} only in iCloud and {} Laika can't open.",
                                a.not_local, a.unsupported
                            )),
                    )
                })
                .child(section("To Photos"))
                .child(setup)
                .child(section("What to sync"))
                .child(scope)
                .when(s.scope == Scope::Rated, |d| d.child(stars))
                .child(albums)
                .child(self.backup_field(
                    text_input::FieldId::PhotosAlbum,
                    if s.by_folder {
                        "Photos folder"
                    } else {
                        "Album name"
                    },
                    s.album_name(),
                    "Laika",
                    cx,
                ))
                .child(favorites)
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .child(self.check_row(
                            ("apple-meta", 0),
                            "Send titles, captions and keywords",
                            s.metadata,
                            "Keyword hierarchies become their last level",
                            cx,
                            |this, cx| this.update_apple_settings(cx, |s| s.metadata = !s.metadata),
                        ))
                        .child(self.check_row(
                            ("apple-videos", 0),
                            "Include videos",
                            s.videos,
                            "Movies sync like photos",
                            cx,
                            |this, cx| this.update_apple_settings(cx, |s| s.videos = !s.videos),
                        ))
                        .child(self.check_row(
                            ("apple-auto", 0),
                            "Sync changes automatically while Laika is open",
                            s.auto,
                            "Checks for new or changed photos every 45 seconds",
                            cx,
                            |this, cx| this.update_apple_settings(cx, |s| s.auto = !s.auto),
                        )),
                )
                .child(section("Good to know"))
                .child(hint(
                    "Develop edits, ratings and history stay in Laika — Photos shows the \
                     unedited original. RAW+JPEG pairs become one photo in Photos.",
                ))
                .child(hint(
                    "Moved photos appear under the Apple Photos folder. Laika never renames, \
                     moves or deletes files inside the Photos library; delete them in Photos.",
                ))
                .child(hint(
                    "Removing a photo from Laika, or from what's synced, leaves it in Photos. \
                     Right-click photos to add them by hand.",
                ))
                .child(footer),
            600.,
        )
    }
}
