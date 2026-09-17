//! Apple Photos → Laika: bring every Photos original into the catalog in
//! place (no copy) and mirror Photos' folder/album tree in the sidebar.
//!
//! A run reads the library through Photos' scripting (bulk properties and
//! the album tree), indexes the originals folder, and hands new files to
//! the regular import pipeline in add-in-place mode. When that import
//! finishes, each photo is linked to its media item and takes Photos'
//! title, caption, keywords and favorite where Laika has none.

use super::*;
use laika_core::apple_photos as ap;

/// Automatic library checks: at most this often, never in the first
/// minutes after launch, only while Photos is already open (Laika never
/// launches it in the background), and a full read only when the library's
/// database changed since the last one.
const INGEST_EVERY_SECS: u64 = 600;
const STARTUP_GRACE_SECS: u64 = 120;
const KEY_LAST_INGEST: &str = "apple_photos_last_ingest";
const KEY_STAMP: &str = "apple_photos_library_stamp";

/// Work waiting on the import pipeline.
pub(crate) struct PendingIngest {
    /// (path, media item id, content hash) for every planned file.
    pub files: Vec<(String, String, String)>,
    pub items: HashMap<String, ap::LibraryItem>,
    /// New files still to import (taken when the import starts).
    pub entries: Option<Vec<laika_core::import::ScanEntry>>,
    /// Send Laika's photos to Photos afterwards.
    pub then_push: bool,
    /// Library change marker taken before the read (saved on success).
    pub stamp: String,
}

impl Laika {
    pub(crate) fn apple_ingest_on(&self) -> bool {
        let s = &self.apple.settings;
        s.ingest_all || s.mirror_albums
    }

    /// Sync button: Photos → Laika first (when on), then Laika → Photos.
    pub(crate) fn start_apple_sync(&mut self, manual: bool, cx: &mut Context<Self>) {
        if self.apple.running || self.catalog.is_none() {
            return;
        }
        if self.apple_ingest_on() {
            self.start_apple_ingest(manual, cx);
        } else if self.apple.settings.enabled {
            self.start_apple_push(manual, cx);
        } else {
            self.open_apple(cx);
        }
    }

    pub(crate) fn reload_apple_albums(&mut self) {
        self.apple.albums = self
            .catalog
            .as_ref()
            .map(|c| c.photos_album_tree())
            .unwrap_or_default();
        self.apple.album_members.replace(None);
        // A selected album that no longer exists stops filtering.
        if let Some(id) = self.state.filters.album.clone() {
            if !self.apple.albums.iter().any(|a| a.id == id) {
                self.state.filters.album = None;
                self.state.filters.album_name.clear();
            }
        }
    }

    /// Membership test for the album filter (members load lazily and are
    /// dropped whenever links or albums change).
    pub(crate) fn in_apple_album(&self, album_id: &str, photo_id: i64) -> bool {
        let mut cache = self.apple.album_members.borrow_mut();
        if cache.as_ref().is_none_or(|(id, _)| id != album_id) {
            let members = self
                .catalog
                .as_ref()
                .map(|c| c.photos_album_members(album_id))
                .unwrap_or_default();
            *cache = Some((album_id.to_string(), members));
        }
        cache.as_ref().is_some_and(|(_, m)| m.contains(&photo_id))
    }

    fn start_apple_ingest(&mut self, manual: bool, cx: &mut Context<Self>) {
        if self.import.is_some() {
            if manual {
                self.apple.error =
                    "Finish the current import, then sync with Photos again.".to_string();
                cx.notify();
            }
            return;
        }
        if manual {
            self.apple.failed_paths.clear();
        }
        let skip_paths = self.apple.failed_paths.clone();
        let settings = self.apple.settings.clone();
        let db = self
            .catalog
            .as_ref()
            .map(|c| c.db_path().to_path_buf())
            .unwrap_or_default();
        self.apple.running = true;
        self.apple.awake = Some(crate::activity::KeepAwake::begin(
            "Syncing with Apple Photos",
        ));
        self.hud_hidden = false;
        self.apple.error.clear();
        self.apple.phase = "Reading your Photos library…".to_string();
        self.apple.done = 0;
        self.apple.total = 0;
        cx.notify();
        cx.spawn(async move |entity, cx| {
            let read = cx
                .background_spawn(async move {
                    let library = ap::library_path()?;
                    ap::check_library(&library)?;
                    // Stamp first: changes made during the read show up as
                    // a new stamp next time.
                    let stamp = ap::library_stamp(&library);
                    let dump = ap::dump_library_blocking(false)?;
                    let originals = ap::index_originals(&library);
                    Ok::<_, String>((dump, originals, stamp))
                })
                .await;
            let planned = entity
                .update(cx, |this, cx| {
                    let same = this
                        .catalog
                        .as_ref()
                        .is_some_and(|c| c.db_path() == db.as_path());
                    if !same {
                        this.apple.running = false;
                        this.apple.awake = None;
                        return None;
                    }
                    let (dump, originals, stamp) = match read {
                        Ok(r) => r,
                        Err(e) => {
                            eprintln!("[photos] library read failed: {e}");
                            this.apple.error = e;
                            this.apple.running = false;
                            this.apple.awake = None;
                            this.apple.phase.clear();
                            cx.notify();
                            return None;
                        }
                    };
                    let cat = this.catalog.as_ref()?;
                    if settings.mirror_albums {
                        if let Err(e) = cat.replace_photos_albums(&dump) {
                            eprintln!("[photos] albums not saved: {e}");
                        }
                    }
                    let linked: HashSet<String> =
                        cat.photos_links().into_iter().map(|l| l.item_id).collect();
                    let known: HashSet<String> =
                        this.photos.iter().map(|p| p.path.clone()).collect();
                    let mut plan =
                        ap::plan_ingest(&dump, &originals, &linked, &known, settings.ingest_all);
                    plan.files.retain(|f| !skip_paths.contains(&f.path));
                    // Developer aid: cap a run (testing against big libraries).
                    if let Some(limit) = std::env::var("LAIKA_PHOTOS_INGEST_LIMIT")
                        .ok()
                        .and_then(|v| v.parse::<usize>().ok())
                    {
                        plan.files.truncate(limit);
                    }
                    eprintln!(
                        "[photos] library: {} items, {} albums/folders, {} files to bring in \
                         ({} new), {} only in iCloud, {} unsupported",
                        dump.items.len(),
                        dump.containers.len(),
                        plan.files.len(),
                        plan.files.iter().filter(|f| !f.known).count(),
                        plan.not_local,
                        plan.unsupported
                    );
                    this.apple.not_local = plan.not_local;
                    this.apple.unsupported = plan.unsupported;
                    this.reload_apple_albums();
                    this.apple.total = plan.files.iter().filter(|f| !f.known).count();
                    if !plan.files.is_empty() {
                        this.apple.phase = "Reading titles and keywords from Photos…".to_string();
                    }
                    cx.notify();
                    Some((plan.files, stamp))
                })
                .ok()
                .flatten();
            let Some((files, stamp)) = planned else {
                return;
            };

            // Metadata only for items Laika doesn't have yet (the bulk
            // properties are the slow part of reading a library).
            let mut new_ids: Vec<String> = files.iter().map(|f| f.item_id.clone()).collect();
            new_ids.sort();
            new_ids.dedup();
            let meta = cx
                .background_spawn(async move { ap::item_metadata_blocking(&new_ids) })
                .await;
            let items: HashMap<String, ap::LibraryItem> = match meta {
                Ok(list) => list.into_iter().map(|i| (i.id.clone(), i)).collect(),
                Err(e) => {
                    // Links are made once; stop rather than bring photos in
                    // without their titles and keywords.
                    eprintln!("[photos] metadata read failed: {e}");
                    entity
                        .update(cx, |this, cx| {
                            this.apple.error = e;
                            this.apple.running = false;
                            this.apple.awake = None;
                            this.apple.phase.clear();
                            cx.notify();
                        })
                        .ok();
                    return;
                }
            };

            // The import workers hash and read each file in parallel; here
            // only sizes are needed (throughput). Duplicates are matched by
            // hash when linking.
            let fresh: Vec<PathBuf> = files
                .iter()
                .filter(|f| !f.known)
                .map(|f| PathBuf::from(&f.path))
                .collect();
            let entries: Vec<laika_core::import::ScanEntry> = cx
                .background_spawn(async move {
                    fresh
                        .into_iter()
                        .map(|path| {
                            let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                            laika_core::import::ScanEntry {
                                is_raw: laika_raw::is_raw(&path),
                                path,
                                size,
                                mtime_secs: 0,
                                captured_at: String::new(),
                                camera: String::new(),
                                content_hash: String::new(),
                                previously_imported: false,
                            }
                        })
                        .collect()
                })
                .await;
            let files: Vec<(String, String, String)> = files
                .into_iter()
                .map(|f| (f.path, f.item_id, String::new()))
                .collect();
            entity
                .update(cx, |this, cx| {
                    this.apple.pending_ingest = Some(PendingIngest {
                        files,
                        items,
                        entries: (!entries.is_empty()).then_some(entries),
                        then_push: settings.enabled,
                        stamp,
                    });
                    this.continue_apple_ingest(cx);
                })
                .ok();
        })
        .detach();
    }

    /// Start the pending import, or finish when nothing is left to import.
    /// Also called when any import run completes.
    pub(crate) fn continue_apple_ingest(&mut self, cx: &mut Context<Self>) {
        let Some(pending) = self.apple.pending_ingest.as_mut() else {
            return;
        };
        let Some(entries) = pending.entries.take() else {
            self.finish_apple_ingest(cx);
            return;
        };
        if self.import.is_some() {
            // Someone else's import is running; retry when it completes.
            pending.entries = Some(entries);
            self.apple.phase = "Waiting for the current import…".to_string();
            cx.notify();
            return;
        }
        let n = entries.len();
        self.apple.phase = format!("Adding {n} photos from Photos");
        let cfg = JobCfg {
            source_label: "Apple Photos".to_string(),
            volume_id: None,
            eject_mount: None,
            eject: false,
            dest: None,
            second: None,
            skip_dup: true,
            new_only: false,
            started_stamp: import_stamp(),
            folder_template: String::new(),
            rename_template: None,
            shoot: String::new(),
            seq_start: 1,
            catalog_name: self.catalog_name.clone(),
            batch: import_stamp(),
            meta_creator: String::new(),
            meta_copyright: String::new(),
            meta_rights: String::new(),
            meta_contact: String::new(),
            meta_keywords: Vec::new(),
            dev_params: None,
            offset_min: 0,
        };
        self.status_note = format!("adding {n} photos from Apple Photos (no copies)");
        self.start_import_run(entries, cfg, (0, Vec::new()), cx);
    }

    fn finish_apple_ingest(&mut self, cx: &mut Context<Self>) {
        let Some(pending) = self.apple.pending_ingest.take() else {
            return;
        };
        let linked = match self.catalog.as_ref() {
            Some(cat) => {
                cat.apply_photos_ingest(&pending.files, &pending.items, &self.apple.settings)
            }
            None => Ok(0),
        };
        let mut changed = false;
        match linked {
            Ok(n) if n > 0 => {
                eprintln!("[photos] linked {n} photos from Photos");
                self.status_note = format!("{n} photos from Apple Photos are now in Laika");
                changed = true;
            }
            Ok(_) => {}
            Err(e) => self.apple.error = format!("couldn't link photos from Photos: {e}"),
        }
        self.apple.running = false;
        self.apple.awake = None;
        self.apple.phase.clear();
        self.apple.last_ingest = Some(Instant::now());
        self.apple.last_check = Some(Instant::now());
        self.apple.last_ok = Some(Instant::now());
        if let Some(cat) = self.catalog.as_ref() {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            cat.set_import_default(KEY_LAST_INGEST, &now.to_string());
            if self.apple.error.is_empty() {
                cat.set_import_default(KEY_STAMP, &pending.stamp);
            }
        }
        // The import (if any) already refreshed the library; a run that
        // linked nothing new changes nothing on screen.
        if changed {
            self.refresh_photos(cx);
        }
        // Files that didn't make it into the catalog (damaged, unreadable)
        // wait for a manual sync instead of retrying every interval.
        let cataloged: HashSet<&str> = self.photos.iter().map(|p| p.path.as_str()).collect();
        let failed: Vec<String> = pending
            .files
            .iter()
            .filter(|(path, _, _)| !cataloged.contains(path.as_str()))
            .map(|(path, _, _)| path.clone())
            .collect();
        self.apple.failed_paths.extend(failed);
        self.reload_apple_albums();
        self.refresh_apple_counts();
        cx.notify();
        if pending.then_push && self.apple.error.is_empty() {
            self.start_apple_push(false, cx);
        }
    }

    /// Time for an automatic change check (cheap gates only; the Photos
    /// process and library checks run in the background).
    pub(crate) fn apple_check_due(&self) -> bool {
        let a = &self.apple;
        if !self.apple_ingest_on() || !a.settings.auto || a.checking || a.running {
            return false;
        }
        if a.loop_started_at
            .is_some_and(|t| t.elapsed().as_secs() < STARTUP_GRACE_SECS)
        {
            return false;
        }
        if a.last_check
            .is_some_and(|t| t.elapsed().as_secs() < INGEST_EVERY_SECS)
        {
            return false;
        }
        // The last full read is remembered across launches.
        let last_ingest = self
            .catalog
            .as_ref()
            .and_then(|c| c.get_import_default(KEY_LAST_INGEST).parse::<u64>().ok())
            .unwrap_or(0);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        now.saturating_sub(last_ingest) >= INGEST_EVERY_SECS
    }

    /// Off the UI thread: is Photos open, and did its library change since
    /// the last read? Only then run a full (slow) read.
    pub(crate) fn check_library_changes(&mut self, cx: &mut Context<Self>) {
        let saved = self
            .catalog
            .as_ref()
            .map(|c| c.get_import_default(KEY_STAMP))
            .unwrap_or_default();
        self.apple.checking = true;
        self.apple.last_check = Some(Instant::now());
        cx.spawn(async move |entity, cx| {
            let changed = cx
                .background_spawn(async move {
                    if !photos_running() {
                        return false;
                    }
                    match ap::library_path() {
                        Ok(lib) => ap::library_stamp(&lib) != saved,
                        Err(_) => false,
                    }
                })
                .await;
            entity
                .update(cx, |this, cx| {
                    this.apple.checking = false;
                    let quiet = this.import.is_none() && this.apple.error.is_empty();
                    if changed && quiet && !this.apple.running {
                        eprintln!("[photos] library changed since the last read; syncing");
                        this.start_apple_sync(false, cx);
                    }
                })
                .ok();
        })
        .detach();
    }

    /// Sidebar section: the mirrored folder/album tree (albums filter the
    /// grid; folders collapse).
    pub(crate) fn apple_albums_section(&self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let _p = crate::ProfSpan("apple_albums_section", Instant::now());
        if self.apple.albums.is_empty() {
            return div();
        }
        let selected = self.state.filters.album.clone();
        let mut hidden_under: Option<usize> = None;
        let mut rows = div().flex().flex_col().gap(px(1.)).px(px(6.));
        for (i, node) in self.apple.albums.iter().enumerate() {
            // Skip descendants of a collapsed folder.
            if let Some(depth) = hidden_under {
                if node.depth > depth {
                    continue;
                }
                hidden_under = None;
            }
            let collapsed = node.is_folder && self.apple.collapsed.contains(&node.id);
            if collapsed {
                hidden_under = Some(node.depth);
            }
            let active = selected.as_deref() == Some(node.id.as_str());
            let id = node.id.clone();
            let name = node.name.clone();
            let is_folder = node.is_folder;
            let label = if is_folder {
                format!("{} {}", if collapsed { "▸" } else { "▾" }, node.name)
            } else {
                node.name.clone()
            };
            let count = if is_folder {
                String::new()
            } else {
                node.count.to_string()
            };
            rows =
                rows.child(
                    div()
                        .id(("photos-album", i))
                        .rounded(px(3.))
                        .on_hover(self.tip(if is_folder {
                            "Photos folder · click to expand or collapse"
                        } else {
                            "Filter to this Photos album"
                        }))
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if is_folder {
                                if !this.apple.collapsed.remove(&id) {
                                    this.apple.collapsed.insert(id.clone());
                                }
                            } else {
                                let f = &mut this.state.filters;
                                if f.album.as_deref() == Some(id.as_str()) {
                                    f.album = None;
                                    f.album_name.clear();
                                } else {
                                    f.album = Some(id.clone());
                                    f.album_name = name.clone();
                                }
                            }
                            cx.notify();
                        }))
                        .child(div().pl(px(node.depth.min(6) as f32 * 12.)).child(
                            list_row::list_row(
                                &label,
                                &count,
                                if active {
                                    accent_line()
                                } else if is_folder {
                                    bg_segment_active()
                                } else {
                                    0x424446
                                },
                                active,
                            ),
                        )),
                );
        }
        div()
            .flex()
            .flex_col()
            .child(section_header::section_header(
                "Photos Albums",
                selected.is_some(),
                window,
            ))
            .child(rows)
    }
}

/// Photos is already open (automatic runs never launch it).
fn photos_running() -> bool {
    std::process::Command::new("/usr/bin/pgrep")
        .args(["-x", "Photos"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}
