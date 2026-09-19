//! S01: File → Import from Lightroom. Read a Lightroom library, find its
//! originals by content, add the ones Laika doesn't have, then bring
//! ratings, flags, titles, locations, develop settings, and albums across
//! with a report of everything that couldn't come.
//!
//! Reading and matching run in the background; catalog writes stay on the
//! UI thread like every other import.

use std::sync::Mutex;
use std::sync::atomic::AtomicBool;

use laika_core::catalog::{LightroomOptions, LightroomReport, LightroomResolution};
use laika_core::lightroom::{self as lr, Library, MatchProgress};

use super::*;
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub(crate) enum LrStage {
    #[default]
    Pick,
    Reading,
    Summary,
    Matching,
    Review,
    Importing,
    Done,
}

#[derive(Default)]
pub(crate) struct LrUi {
    pub open: bool,
    pub stage: LrStage,
    pub libraries: Vec<PathBuf>,
    pub path: Option<PathBuf>,
    pub lib: Option<Arc<Library>>,
    pub error: String,
    /// Extra folders to search for originals.
    pub folders: Vec<PathBuf>,
    pub albums_on: HashSet<String>,
    pub develop: bool,
    pub overwrite: bool,
    pub progress: Arc<Mutex<MatchProgress>>,
    pub cancel: Arc<AtomicBool>,
    pub resolution: Option<LightroomResolution>,
    /// Asset id → file being imported (copied originals point at the copy).
    pub importing: HashMap<String, PathBuf>,
    pub copied: usize,
    pub report: Option<LightroomReport>,
    pub report_path: Option<PathBuf>,
}

/// Where originals found inside Lightroom's package are copied, so Laika
/// never references files Lightroom may purge.
fn copy_destination() -> PathBuf {
    laika_core::platform::pictures_dir().join("Lightroom Originals")
}

/// `~/Pictures/…` for display.
fn tilde(p: &Path) -> String {
    let home = laika_core::platform::home_dir();
    match p.strip_prefix(&home) {
        Ok(rest) if !home.as_os_str().is_empty() => format!("~/{}", rest.display()),
        _ => p.display().to_string(),
    }
}

fn grouped(n: usize) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn plural(n: usize, one: &str, many: &str) -> String {
    format!("{} {}", grouped(n), if n == 1 { one } else { many })
}

/// Copy a file to `dir`, keeping its name unless taken.
fn copy_unique(src: &Path, dir: &Path, name: &str) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    let name = if name.trim().is_empty() {
        src.file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "original".to_string())
    } else {
        name.to_string()
    };
    let (stem, ext) = match name.rsplit_once('.') {
        Some((s, e)) => (s.to_string(), format!(".{e}")),
        None => (name.clone(), String::new()),
    };
    let src_len = std::fs::metadata(src).map(|m| m.len()).ok();
    for i in 0..1000 {
        let candidate = if i == 0 {
            dir.join(&name)
        } else {
            dir.join(format!("{stem}-{i}{ext}"))
        };
        if candidate.exists() {
            // Same file already copied by an earlier run: reuse it.
            if std::fs::metadata(&candidate).map(|m| m.len()).ok() == src_len
                && lr::sha256_file(&candidate).ok() == lr::sha256_file(src).ok()
            {
                return Ok(candidate);
            }
            continue;
        }
        std::fs::copy(src, &candidate).map_err(|e| format!("copy {name}: {e}"))?;
        return Ok(candidate);
    }
    Err(format!("couldn't find a free name for {name}"))
}

impl Laika {
    pub(crate) fn open_lightroom_import(&mut self, cx: &mut Context<Self>) {
        if self.catalog.is_none() {
            self.status_note = "open a catalog first".to_string();
            cx.notify();
            return;
        }
        // A finished or running import keeps its state when reopened.
        if matches!(
            self.lr.stage,
            LrStage::Importing | LrStage::Matching | LrStage::Reading
        ) {
            self.lr.open = true;
            cx.notify();
            return;
        }
        self.close_modals(cx);
        let home = laika_core::platform::home_dir();
        self.lr = LrUi {
            open: true,
            libraries: lr::default_libraries(&home),
            develop: true,
            ..Default::default()
        };
        if let [only] = self.lr.libraries.as_slice() {
            let only = only.clone();
            self.lr_read(only, cx);
        }
        cx.notify();
    }

    fn lr_choose(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: true,
            multiple: false,
            prompt: Some("Import this Lightroom library".into()),
        });
        cx.spawn(async move |entity, cx| {
            let path = rx
                .await
                .ok()
                .and_then(|r| r.ok())
                .flatten()
                .and_then(|p| p.into_iter().next());
            if let Some(path) = path {
                entity.update(cx, |this, cx| this.lr_read(path, cx)).ok();
            }
        })
        .detach();
    }

    fn lr_add_folder(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: true,
            prompt: Some("Search this folder for originals".into()),
        });
        cx.spawn(async move |entity, cx| {
            let paths = rx
                .await
                .ok()
                .and_then(|r| r.ok())
                .flatten()
                .unwrap_or_default();
            entity
                .update(cx, |this, cx| {
                    for p in paths {
                        if !this.lr.folders.contains(&p) {
                            this.lr.folders.push(p);
                        }
                    }
                    cx.notify();
                })
                .ok();
        })
        .detach();
    }

    fn lr_read(&mut self, path: PathBuf, cx: &mut Context<Self>) {
        if path.extension().is_some_and(|e| e == "lrcat") {
            self.lr.error = "Lightroom Classic catalogs (.lrcat) aren't supported yet — this reads Lightroom libraries (.lrlibrary)".to_string();
            cx.notify();
            return;
        }
        self.lr.path = Some(path.clone());
        self.lr.stage = LrStage::Reading;
        self.lr.error.clear();
        cx.notify();
        cx.spawn(async move |entity, cx| {
            let t = Instant::now();
            let read = cx
                .background_spawn(async move { Library::open(&path) })
                .await;
            entity
                .update(cx, |this, cx| {
                    if this.lr.stage != LrStage::Reading {
                        return;
                    }
                    match read {
                        Ok(lib) => {
                            eprintln!(
                                "[lightroom] read {} assets, {} albums in {:.1}s",
                                lib.assets.len(),
                                lib.albums.len(),
                                t.elapsed().as_secs_f32()
                            );
                            this.lr.albums_on = lib
                                .importable_albums()
                                .iter()
                                .map(|a| a.id.clone())
                                .collect();
                            this.lr.lib = Some(Arc::new(lib));
                            this.lr.stage = LrStage::Summary;
                        }
                        Err(e) => {
                            eprintln!("[lightroom] read failed: {e}");
                            this.lr.error = e;
                            this.lr.stage = LrStage::Pick;
                        }
                    }
                    cx.notify();
                })
                .ok();
        })
        .detach();
    }

    /// Hash candidate files against the originals Laika doesn't know yet.
    fn lr_find_originals(&mut self, cx: &mut Context<Self>) {
        let (Some(lib), Some(cat)) = (self.lr.lib.clone(), self.catalog.as_ref()) else {
            return;
        };
        let linked = cat.lightroom_linked_assets(&lib.catalog_id);
        let wanted: Vec<(String, u64)> = lib
            .assets
            .iter()
            .filter(|a| !linked.contains_key(&a.id))
            .map(|a| (a.sha256.clone(), a.file_size))
            .collect();
        let catalog_files: Vec<PathBuf> =
            self.photos.iter().map(|p| PathBuf::from(&p.path)).collect();
        let folders = self.lr.folders.clone();
        self.lr.stage = LrStage::Matching;
        self.lr.error.clear();
        self.lr.cancel = Arc::new(AtomicBool::new(false));
        self.lr.progress = Arc::new(Mutex::new(MatchProgress::default()));
        let cancel = self.lr.cancel.clone();
        let progress = self.lr.progress.clone();
        let done = Arc::new(AtomicBool::new(false));
        let done_bg = done.clone();
        let workers = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4)
            .clamp(2, 8);
        cx.notify();
        // Progress repaint while hashing.
        cx.spawn({
            let done = done.clone();
            async move |entity, cx| {
                while !done.load(std::sync::atomic::Ordering::Relaxed) {
                    cx.background_executor()
                        .timer(std::time::Duration::from_millis(200))
                        .await;
                    if entity.update(cx, |_, cx| cx.notify()).is_err() {
                        return;
                    }
                }
            }
        })
        .detach();
        cx.spawn(async move |entity, cx| {
            let t = Instant::now();
            let lib_bg = lib.clone();
            let found = cx
                .background_spawn(async move {
                    let mut candidates = catalog_files;
                    candidates.extend(lib_bg.local_originals());
                    for f in &folders {
                        candidates.extend(lr::walk_media(f, false));
                    }
                    let found = lr::find_originals(&wanted, candidates, workers, &cancel, &|p| {
                        if let Ok(mut g) = progress.lock() {
                            *g = p;
                        }
                    });
                    done_bg.store(true, std::sync::atomic::Ordering::Relaxed);
                    found
                })
                .await;
            done.store(true, std::sync::atomic::Ordering::Relaxed);
            entity
                .update(cx, |this, cx| {
                    if this.lr.stage != LrStage::Matching {
                        return;
                    }
                    if this.lr.cancel.load(std::sync::atomic::Ordering::Relaxed) {
                        this.lr.stage = LrStage::Summary;
                        cx.notify();
                        return;
                    }
                    let Some(cat) = this.catalog.as_ref() else { return };
                    let res = cat.resolve_lightroom(&lib, &found);
                    eprintln!(
                        "[lightroom] matched {} originals in {:.1}s: {} in catalog, {} to add, {} missing",
                        found.len(),
                        t.elapsed().as_secs_f32(),
                        res.in_catalog.len(),
                        res.to_import.len(),
                        res.missing.len()
                    );
                    this.lr.resolution = Some(res);
                    this.lr.stage = LrStage::Review;
                    cx.notify();
                })
                .ok();
        })
        .detach();
    }

    /// Add the files Laika doesn't have (copying any from inside
    /// Lightroom's package first), then apply.
    fn lr_start_import(&mut self, cx: &mut Context<Self>) {
        let (Some(lib), Some(res)) = (self.lr.lib.clone(), self.lr.resolution.clone()) else {
            return;
        };
        if self.import.is_some() {
            self.lr.error = "Another import is running — try again when it finishes.".to_string();
            cx.notify();
            return;
        }
        self.flush_saves();
        self.lr.stage = LrStage::Importing;
        self.lr.error.clear();
        cx.notify();
        let names: HashMap<String, String> = lib
            .assets
            .iter()
            .map(|a| (a.id.clone(), a.file_name.clone()))
            .collect();
        let package = lib.path.clone();
        let to_import = res.to_import.clone();
        cx.spawn(async move |entity, cx| {
            let placed = cx
                .background_spawn(async move {
                    let dest = copy_destination();
                    let mut out: HashMap<String, PathBuf> = HashMap::new();
                    let mut copied = 0;
                    let mut errors = Vec::new();
                    for (asset, path) in to_import {
                        if path.starts_with(&package) {
                            let name = names.get(&asset).cloned().unwrap_or_default();
                            match copy_unique(&path, &dest, &name) {
                                Ok(p) => {
                                    copied += 1;
                                    out.insert(asset, p);
                                }
                                Err(e) => errors.push(e),
                            }
                        } else {
                            out.insert(asset, path);
                        }
                    }
                    (out, copied, errors)
                })
                .await;
            entity
                .update(cx, |this, cx| {
                    let (files, copied, errors) = placed;
                    for e in &errors {
                        eprintln!("[lightroom] {e}");
                    }
                    this.lr.copied = copied;
                    this.lr.importing = files.clone();
                    let mut paths: Vec<PathBuf> = files.into_values().collect();
                    paths.sort();
                    paths.dedup();
                    if paths.is_empty() {
                        this.lr_apply(cx);
                        return;
                    }
                    let entries: Vec<laika_core::import::ScanEntry> = paths
                        .into_iter()
                        .map(|path| laika_core::import::ScanEntry {
                            is_raw: laika_raw::is_raw(&path),
                            size: std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0),
                            path,
                            mtime_secs: 0,
                            captured_at: String::new(),
                            camera: String::new(),
                            content_hash: String::new(),
                            previously_imported: false,
                        })
                        .collect();
                    let n = entries.len();
                    let cfg = JobCfg {
                        source_label: "Lightroom".to_string(),
                        volume_id: None,
                        eject_mount: None,
                        eject: false,
                        dest: None,
                        second: None,
                        skip_dup: false,
                        new_only: false,
                        started_stamp: import_stamp(),
                        folder_template: String::new(),
                        rename_template: None,
                        shoot: String::new(),
                        seq_start: 1,
                        catalog_name: this.catalog_name.clone(),
                        batch: import_stamp(),
                        meta_creator: String::new(),
                        meta_copyright: String::new(),
                        meta_rights: String::new(),
                        meta_contact: String::new(),
                        meta_keywords: Vec::new(),
                        dev_params: None,
                        offset_min: 0,
                    };
                    this.status_note =
                        format!("adding {} from Lightroom", plural(n, "photo", "photos"));
                    this.start_import_run(entries, cfg, (0, Vec::new()), cx);
                    if this.import.is_none() {
                        // The run couldn't start (nothing to do); apply now.
                        this.lr_apply(cx);
                    }
                })
                .ok();
        })
        .detach();
    }

    /// Called whenever an import run completes.
    pub(crate) fn continue_lightroom_import(&mut self, cx: &mut Context<Self>) {
        if self.lr.stage == LrStage::Importing && self.import.is_none() {
            self.lr_apply(cx);
        }
    }

    fn lr_apply(&mut self, cx: &mut Context<Self>) {
        let (Some(lib), Some(res)) = (self.lr.lib.clone(), self.lr.resolution.clone()) else {
            return;
        };
        let Some(cat) = self.catalog.as_ref() else {
            return;
        };
        let mut photo_of = res.in_catalog.clone();
        let mut not_added = 0;
        for (asset, path) in &self.lr.importing {
            match cat.photo_id_for_path(path) {
                Some(id) => {
                    photo_of.insert(asset.clone(), id);
                }
                None => not_added += 1,
            }
        }
        let opts = LightroomOptions {
            albums: self.lr.albums_on.clone(),
            develop: self.lr.develop,
            overwrite: self.lr.overwrite,
        };
        let t = Instant::now();
        match cat.apply_lightroom(&lib, &photo_of, &opts) {
            Ok(report) => {
                eprintln!(
                    "[lightroom] applied in {:.1}s: {} linked, {} missing, {} collections",
                    t.elapsed().as_secs_f32(),
                    report.linked,
                    report.missing,
                    report.collections_created + report.collections_updated
                );
                let mut text = report.to_text();
                if self.lr.copied > 0 {
                    text.push_str(&format!(
                        "\n{} copied out of Lightroom's library to {}\n",
                        plural(self.lr.copied, "original was", "originals were"),
                        tilde(&copy_destination())
                    ));
                }
                if not_added > 0 {
                    text.push_str(&format!(
                        "\n{} couldn't be added (see the import report)\n",
                        plural(not_added, "file", "files")
                    ));
                }
                self.lr.report_path = laika_core::logging::log_dir().and_then(|d| {
                    let stamp = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0);
                    let p = d.join(format!("lightroom-import-{stamp}.txt"));
                    std::fs::write(&p, &text).ok().map(|_| p)
                });
                let changed = report.changed.clone();
                self.lr.report = Some(report);
                self.lr.stage = LrStage::Done;
                self.load_edits_from_db();
                self.refresh_photos(cx);
                for pid in changed {
                    self.request_sidecar(pid, false);
                    self.sync_derived(pid, cx);
                }
                self.status_note = "Lightroom import finished".to_string();
            }
            Err(e) => {
                eprintln!("[lightroom] apply failed: {e}");
                self.lr.error = format!("Nothing was changed: {e}");
                self.lr.stage = LrStage::Review;
            }
        }
        cx.notify();
    }

    // ---- view -------------------------------------------------------------------

    pub(crate) fn lightroom_modal(&self, cx: &mut Context<Self>) -> Div {
        let header = div()
            .flex()
            .items_center()
            .gap(px(10.))
            .child(
                div()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_size(sp(16.))
                    .text_color(rgb(TEXT_PRIMARY))
                    .child("Import from Lightroom"),
            )
            .child(div().flex_1())
            .child(
                div()
                    .id("lr-close")
                    .text_size(sp(15.))
                    .text_color(rgb(TEXT_DIM))
                    .hover(|d| d.text_color(rgb(TEXT_PRIMARY)))
                    .on_click(cx.listener(|this, _, _, cx| this.lr_close(cx)))
                    .child("✕"),
            );
        let body = match self.lr.stage {
            LrStage::Pick => self.lr_pick(cx),
            LrStage::Reading => self.lr_busy(
                "Reading the Lightroom library…",
                "Laika works from a copy of Lightroom's catalog — Lightroom's files are never changed.",
                None,
                cx,
            ),
            LrStage::Summary => self.lr_summary(cx),
            LrStage::Matching => {
                let p = self.lr.progress.lock().map(|g| *g).unwrap_or_default();
                let frac = if p.total == 0 { 0. } else { p.checked as f32 / p.total as f32 };
                self.lr_busy(
                    &format!(
                        "Checking {} of {} files · {} found",
                        grouped(p.checked),
                        grouped(p.total),
                        plural(p.found, "original", "originals")
                    ),
                    "Only files whose size matches a Lightroom original are read.",
                    Some(frac),
                    cx,
                )
            }
            LrStage::Review => self.lr_review(cx),
            LrStage::Importing => self.lr_busy(
                "Adding photos to the catalog…",
                "Progress shows at the bottom of the window. Lightroom's ratings, albums and settings are applied when it finishes.",
                None,
                cx,
            ),
            LrStage::Done => self.lr_done(cx),
        };
        let content = div()
            .p(px(24.))
            .flex()
            .flex_col()
            .gap(px(14.))
            .max_h(px(720.))
            .child(header)
            .child(body)
            .when(!self.lr.error.is_empty(), |d| {
                d.child(
                    div()
                        .text_size(sp(11.))
                        .text_color(rgb(0xE56060))
                        .child(self.lr.error.clone()),
                )
            });
        modal::modal_shell_w(content, 640.)
    }

    fn lr_close(&mut self, cx: &mut Context<Self>) {
        if self.lr.stage == LrStage::Matching {
            self.lr
                .cancel
                .store(true, std::sync::atomic::Ordering::Relaxed);
            self.lr.stage = LrStage::Summary;
        }
        if self.lr.stage == LrStage::Reading {
            self.lr.stage = LrStage::Pick;
        }
        self.lr.open = false;
        cx.notify();
    }

    fn lr_button(id: &'static str, label: &str) -> Stateful<Div> {
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
    }

    fn lr_primary(id: &'static str, label: &str) -> Stateful<Div> {
        div().id(id).child(button::primary(label))
    }

    fn lr_note(text: impl Into<SharedString>) -> Div {
        div()
            .text_size(sp(11.))
            .text_color(rgb(TEXT_DIM))
            .child(text.into())
    }

    fn lr_pick(&self, cx: &mut Context<Self>) -> Div {
        let mut list = div().flex().flex_col().gap(px(6.));
        for (i, p) in self.lr.libraries.iter().enumerate() {
            let path = p.clone();
            list = list.child(
                div()
                    .id(("lr-lib", i))
                    .p(px(10.))
                    .rounded(px(5.))
                    .bg(rgb(bg_well()))
                    .hover(|s| s.bg(rgb(bg_row_hover())))
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .on_click(cx.listener(move |this, _, _, cx| this.lr_read(path.clone(), cx)))
                    .child(
                        div()
                            .text_size(sp(12.))
                            .text_color(rgb(TEXT_PRIMARY))
                            .child(
                                p.file_name()
                                    .map(|n| n.to_string_lossy().into_owned())
                                    .unwrap_or_default(),
                            ),
                    )
                    .child(Self::lr_note(tilde(p))),
            );
        }
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(
                div()
                    .text_size(sp(12.))
                    .text_color(rgb(TEXT_SECONDARY))
                    .child("Bring your Lightroom library's ratings, flags, titles, locations, develop settings and albums into this catalog. Lightroom's files are never changed."),
            )
            .child(if self.lr.libraries.is_empty() {
                Self::lr_note("No Lightroom library found in ~/Pictures.")
            } else {
                list
            })
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(div().flex_1())
                    .child(Self::lr_button("lr-choose", "Choose Library…").on_click(cx.listener(|this, _, _, cx| this.lr_choose(cx)))),
            )
    }

    fn lr_busy(&self, title: &str, note: &str, frac: Option<f32>, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                div()
                    .text_size(sp(12.5))
                    .text_color(rgb(TEXT_PRIMARY))
                    .child(title.to_string()),
            )
            .child(
                div()
                    .h(px(4.))
                    .rounded(px(2.))
                    .bg(rgb(bg_well()))
                    .overflow_hidden()
                    .child(
                        div()
                            .h_full()
                            .w(relative(frac.unwrap_or(0.35).clamp(0.02, 1.)))
                            .bg(rgb(accent_line())),
                    ),
            )
            .child(Self::lr_note(note.to_string()))
            .when(self.lr.stage == LrStage::Matching, |d| {
                d.child(div().flex().child(div().flex_1()).child(
                    Self::lr_button("lr-cancel", "Stop").on_click(cx.listener(|this, _, _, cx| {
                        this.lr
                            .cancel
                            .store(true, std::sync::atomic::Ordering::Relaxed);
                        cx.notify();
                    })),
                ))
            })
    }

    fn lr_stat_grid(rows: Vec<(String, String)>) -> Div {
        div()
            .p(px(12.))
            .rounded(px(5.))
            .bg(rgb(bg_well()))
            .flex()
            .flex_wrap()
            .gap_y(px(6.))
            .children(rows.into_iter().map(|(k, v)| {
                div()
                    .w(relative(0.5))
                    .flex()
                    .gap(px(8.))
                    .child(
                        div()
                            .w(px(120.))
                            .flex_none()
                            .text_size(sp(11.))
                            .text_color(rgb(TEXT_DIM))
                            .child(k),
                    )
                    .child(
                        div()
                            .text_size(sp(11.))
                            .text_color(rgb(TEXT_SECONDARY))
                            .child(v),
                    )
            }))
    }

    fn lr_summary(&self, cx: &mut Context<Self>) -> Div {
        let Some(lib) = self.lr.lib.as_ref() else {
            return div();
        };
        let st = &lib.stats;
        let albums = lib.importable_albums();
        let names = lib.collection_names();
        let stats = Self::lr_stat_grid(vec![
            ("Photos".into(), grouped(st.images)),
            ("Videos".into(), grouped(st.videos)),
            ("Albums".into(), grouped(albums.len())),
            ("Rated".into(), grouped(st.rated)),
            ("Flagged".into(), grouped(st.flagged)),
            ("With a location".into(), grouped(st.located)),
            (
                "Edited".into(),
                format!(
                    "{} ({} on this Mac)",
                    grouped(st.edited),
                    grouped(st.edits_local)
                ),
            ),
            (
                "Kept locally".into(),
                plural(st.local_originals, "original", "originals"),
            ),
        ]);
        let not_coming: Vec<String> = [
            (st.people, "people"),
            (st.stacks, "stacks"),
            (st.smart_albums, "smart albums"),
            (st.virtual_copies, "virtual copies"),
        ]
        .iter()
        .filter(|(n, _)| *n > 0)
        .map(|(n, w)| format!("{} {w}", grouped(*n)))
        .collect();

        let mut sources = div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(Self::lr_note(format!(
                "• This catalog — {}",
                plural(self.photos.len(), "photo", "photos")
            )))
            .child(Self::lr_note(format!(
                "• Lightroom's local originals — {} (copied to {})",
                grouped(st.local_originals),
                tilde(&copy_destination())
            )));
        for (i, f) in self.lr.folders.iter().enumerate() {
            sources = sources.child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(Self::lr_note(format!("• {}", tilde(f))))
                    .child(
                        div()
                            .id(("lr-folder-remove", i))
                            .text_size(sp(11.))
                            .text_color(rgb(TEXT_DIM))
                            .hover(|s| s.text_color(rgb(TEXT_PRIMARY)))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if i < this.lr.folders.len() {
                                    this.lr.folders.remove(i);
                                }
                                cx.notify();
                            }))
                            .child("✕"),
                    ),
            );
        }

        let all_on = albums.iter().all(|a| self.lr.albums_on.contains(&a.id));
        let mut sorted: Vec<(&lr::Album, String)> = albums
            .iter()
            .map(|a| {
                (
                    *a,
                    names.get(&a.id).cloned().unwrap_or_else(|| a.name.clone()),
                )
            })
            .collect();
        sorted.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));
        let album_list = div()
            .id("lr-albums")
            .max_h(px(170.))
            .overflow_y_scroll()
            .p(px(8.))
            .rounded(px(5.))
            .bg(rgb(bg_well()))
            .flex()
            .flex_col()
            .gap(px(2.))
            .children(sorted.into_iter().enumerate().map(|(i, (a, name))| {
                let on = self.lr.albums_on.contains(&a.id);
                let id = a.id.clone();
                div()
                    .id(("lr-album", i))
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .px(px(4.))
                    .py(px(2.))
                    .rounded(px(3.))
                    .hover(|s| s.bg(rgb(bg_row_hover())))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if !this.lr.albums_on.remove(&id) {
                            this.lr.albums_on.insert(id.clone());
                        }
                        cx.notify();
                    }))
                    .child(
                        div()
                            .size(px(12.))
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
                            .truncate()
                            .text_size(sp(11.))
                            .text_color(rgb(if on { TEXT_SECONDARY } else { TEXT_DIM }))
                            .child(name),
                    )
                    .child(
                        div()
                            .text_size(sp(10.5))
                            .text_color(rgb(TEXT_DIM))
                            .child(grouped(a.members.len())),
                    )
            }));

        let develop = self.lr.develop;
        let overwrite = self.lr.overwrite;
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(Self::lr_note(tilde(&lib.path)))
            .child(stats)
            .when(!not_coming.is_empty(), |d| {
                d.child(Self::lr_note(format!(
                    "Not imported: {}. The report lists them.",
                    not_coming.join(" · ")
                )))
            })
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(div().text_size(sp(12.)).text_color(rgb(TEXT_PRIMARY)).child("Where are the originals?"))
                    .child(Self::lr_note(
                        "Lightroom keeps most originals in Adobe's cloud. Laika finds your copies by content, so renamed or moved files still match. It searches:",
                    ))
                    .child(sources)
                    .child(
                        div().flex().child(
                            Self::lr_button("lr-add-folder", "Add Folder…")
                                .on_click(cx.listener(|this, _, _, cx| this.lr_add_folder(cx))),
                        ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .text_size(sp(12.))
                            .text_color(rgb(TEXT_PRIMARY))
                            .child(format!("Albums → collections ({} of {})", grouped(self.lr.albums_on.len()), grouped(albums.len()))),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .id("lr-albums-all")
                            .text_size(sp(10.5))
                            .text_color(rgb(TEXT_DIM))
                            .hover(|s| s.text_color(rgb(TEXT_PRIMARY)))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if all_on {
                                    this.lr.albums_on.clear();
                                } else if let Some(lib) = this.lr.lib.as_ref() {
                                    this.lr.albums_on =
                                        lib.importable_albums().iter().map(|a| a.id.clone()).collect();
                                }
                                cx.notify();
                            }))
                            .child(if all_on { "Select none" } else { "Select all" }),
                    ),
            )
            .child(album_list)
            .child(
                div()
                    .id("lr-develop")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.lr.develop = !develop;
                        cx.notify();
                    }))
                    .child(toggle::toggle(develop, "Bring develop settings (Laika renders them its own way)")),
            )
            .child(
                div()
                    .id("lr-overwrite")
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.lr.overwrite = !overwrite;
                        cx.notify();
                    }))
                    .child(toggle::toggle(
                        overwrite,
                        "Replace ratings, flags, titles, locations and edits Laika already has",
                    )),
            )
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(Self::lr_button("lr-back", "Choose Another…").on_click(cx.listener(|this, _, _, cx| {
                        this.lr.stage = LrStage::Pick;
                        cx.notify();
                    })))
                    .child(div().flex_1())
                    .child(
                        Self::lr_primary("lr-find", "Find Originals")
                            .on_click(cx.listener(|this, _, _, cx| this.lr_find_originals(cx))),
                    ),
            )
    }

    fn lr_review(&self, cx: &mut Context<Self>) -> Div {
        let (Some(lib), Some(res)) = (self.lr.lib.as_ref(), self.lr.resolution.as_ref()) else {
            return div();
        };
        let inside = res
            .to_import
            .values()
            .filter(|p| p.starts_with(&lib.path))
            .count();
        let folders = res.to_import.len() - inside;
        let n_albums = self.lr.albums_on.len();
        let mut rows = div()
            .p(px(12.))
            .rounded(px(5.))
            .bg(rgb(bg_well()))
            .flex()
            .flex_col()
            .gap(px(6.));
        let row = |label: String, n: usize, color: u32| {
            div()
                .flex()
                .gap(px(10.))
                .child(
                    div()
                        .w(px(70.))
                        .flex_none()
                        .text_size(sp(12.))
                        .text_color(rgb(color))
                        .child(grouped(n)),
                )
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .text_size(sp(11.5))
                        .text_color(rgb(TEXT_SECONDARY))
                        .child(label),
                )
        };
        rows = rows.child(row(
            "already in this catalog — updated in place".into(),
            res.in_catalog.len(),
            accent_line(),
        ));
        if folders > 0 {
            rows = rows.child(row(
                "found in your folders — added where they are".into(),
                folders,
                accent_line(),
            ));
        }
        if inside > 0 {
            rows = rows.child(row(
                format!(
                    "in Lightroom's library — copied to {}",
                    tilde(&copy_destination())
                ),
                inside,
                accent_line(),
            ));
        }
        rows = rows.child(row(
            "not found on this Mac — listed in the report".into(),
            res.missing.len(),
            if res.missing.is_empty() {
                TEXT_DIM
            } else {
                0xE0A050
            },
        ));
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(rows)
            .when(!res.missing.is_empty(), |d| {
                d.child(Self::lr_note(
                    "Originals that live only in Adobe's cloud can't be matched. Download them from Lightroom (Export → Original), add that folder, and search again — photos already matched are remembered.",
                ))
            })
            .child(Self::lr_note(format!(
                "Then: ratings, flags, titles, keywords and locations{} are applied, and {} become collections. {}",
                if self.lr.develop { ", develop settings" } else { "" },
                plural(n_albums, "album", "albums"),
                if self.lr.overwrite {
                    "Values Laika already has are replaced."
                } else {
                    "Values Laika already has are kept."
                }
            )))
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(Self::lr_button("lr-review-back", "Back").on_click(cx.listener(|this, _, _, cx| {
                        this.lr.stage = LrStage::Summary;
                        cx.notify();
                    })))
                    .child(div().flex_1())
                    .child(
                        Self::lr_primary(
                            "lr-import",
                            &format!("Import {}", plural(res.in_catalog.len() + res.to_import.len(), "Photo", "Photos")),
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.lr_start_import(cx))),
                    ),
            )
    }

    fn lr_done(&self, cx: &mut Context<Self>) -> Div {
        let Some(r) = self.lr.report.as_ref() else {
            return div();
        };
        let stats = Self::lr_stat_grid(vec![
            ("Linked".into(), grouped(r.linked)),
            ("Not found".into(), grouped(r.missing)),
            ("Ratings".into(), grouped(r.ratings)),
            ("Flags".into(), grouped(r.flags)),
            ("Titles".into(), grouped(r.titles)),
            ("Keyword sets".into(), grouped(r.keywords)),
            ("Locations".into(), grouped(r.locations)),
            ("Develop settings".into(), grouped(r.develop)),
            (
                "Collections".into(),
                format!(
                    "{} new · {} updated",
                    grouped(r.collections_created),
                    grouped(r.collections_updated)
                ),
            ),
        ]);
        let unsupported: Vec<String> = r
            .unsupported
            .iter()
            .map(|(k, n)| format!("{k} ({})", grouped(*n)))
            .collect();
        let path = self.lr.report_path.clone();
        div()
            .flex()
            .flex_col()
            .gap(px(12.))
            .child(stats)
            .when(r.kept > 0, |d| {
                d.child(Self::lr_note(format!(
                    "Kept {} Laika already had. Import again with “Replace” on to take Lightroom's.",
                    plural(r.kept, "value", "values")
                )))
            })
            .when(!unsupported.is_empty(), |d| {
                d.child(Self::lr_note(format!(
                    "Develop settings Laika doesn't render yet: {}.",
                    unsupported.join(", ")
                )))
            })
            .when(r.develop_unavailable > 0, |d| {
                d.child(Self::lr_note(format!(
                    "{} edited in Lightroom have settings only in Adobe's cloud.",
                    plural(r.develop_unavailable, "photo", "photos")
                )))
            })
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .when_some(path, |d, p| {
                        d.child(Self::lr_button("lr-report", "Show Report").on_click(cx.listener(move |this, _, _, cx| {
                            if let Err(e) = laika_core::import::reveal_in_manager(&p) {
                                this.lr.error = e;
                            }
                            cx.notify();
                        })))
                    })
                    .child(div().flex_1())
                    .child(Self::lr_primary("lr-done", "Done").on_click(cx.listener(|this, _, _, cx| {
                        this.lr = LrUi::default();
                        cx.notify();
                    }))),
            )
    }
}
