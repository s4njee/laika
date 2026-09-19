//! S01: apply a Lightroom library to this catalog (links from migration
//! v11). Reading the library lives in `crate::lightroom`.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};

use rusqlite::params;

use super::{Catalog, chrono_stamp};
use crate::lightroom::{Flag, Library};

/// What to bring across.
#[derive(Clone, Debug, PartialEq)]
pub struct LightroomOptions {
    /// Album ids to turn into collections.
    pub albums: HashSet<String>,
    pub develop: bool,
    /// Replace values Laika already has (default: only fill blanks).
    pub overwrite: bool,
}

/// Where each Lightroom asset's original is.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LightroomResolution {
    /// Asset id → photo already in this catalog.
    pub in_catalog: HashMap<String, i64>,
    /// Asset id → file to add to the catalog first.
    pub to_import: HashMap<String, PathBuf>,
    /// Asset ids with no original found.
    pub missing: Vec<String>,
}

/// The migration report.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LightroomReport {
    pub library: String,
    pub photos: usize,
    pub linked: usize,
    pub missing: usize,
    pub ratings: usize,
    pub flags: usize,
    pub titles: usize,
    /// Photos whose keyword set gained or replaced Lightroom keywords.
    pub keywords: usize,
    pub locations: usize,
    pub develop: usize,
    /// Values Laika already had and kept (fill-blanks mode).
    pub kept: usize,
    pub collections_created: usize,
    pub collections_updated: usize,
    pub collection_members: usize,
    /// Setting name → photos where Laika applied it only approximately
    /// or not at all.
    pub unsupported: BTreeMap<String, usize>,
    /// Edited photos whose Lightroom settings are only in Adobe's cloud.
    pub develop_unavailable: usize,
    pub virtual_copies: usize,
    pub stacks: usize,
    pub smart_albums: usize,
    pub people: usize,
    pub faces: usize,
    /// File names of assets whose originals were not found (first 500).
    pub missing_files: Vec<String>,
    /// Photos whose metadata or settings changed (sidecars to rewrite).
    pub changed: Vec<i64>,
}

impl LightroomReport {
    /// Plain-text report for the log folder.
    pub fn to_text(&self) -> String {
        let mut t = String::new();
        let mut line = |s: String| {
            t.push_str(&s);
            t.push('\n');
        };
        line(format!("Lightroom import — {}", self.library));
        line(String::new());
        line(format!(
            "Photos: {} in Lightroom · {} linked to Laika · {} originals not found",
            self.photos, self.linked, self.missing
        ));
        line(format!(
            "Applied: {} ratings · {} flags · {} titles · {} keyword sets · {} locations · {} develop settings",
            self.ratings, self.flags, self.titles, self.keywords, self.locations, self.develop
        ));
        if self.kept > 0 {
            line(format!(
                "Kept {} values Laika already had (import again with “Replace” to take Lightroom's)",
                self.kept
            ));
        }
        line(format!(
            "Collections: {} created · {} updated · {} memberships",
            self.collections_created, self.collections_updated, self.collection_members
        ));
        line(String::new());
        line("Not imported".to_string());
        let mut not = |n: usize, what: &str| {
            if n > 0 {
                line(format!("  {n} {what}"));
            }
        };
        not(
            self.develop_unavailable,
            "edited photos whose settings are only in Adobe's cloud (open them once in Lightroom to download)",
        );
        not(
            self.virtual_copies,
            "virtual copies (Laika has no virtual copies yet)",
        );
        not(
            self.stacks,
            "stacks (grouping only; the photos are imported)",
        );
        not(
            self.smart_albums,
            "smart albums (criteria can't be translated yet)",
        );
        not(self.people, "people (face groups)");
        not(self.faces, "photos with detected face regions");
        if !self.unsupported.is_empty() {
            line(String::new());
            line("Develop settings Laika doesn't render yet (photos affected)".to_string());
            for (k, n) in &self.unsupported {
                line(format!("  {k}: {n}"));
            }
        }
        if !self.missing_files.is_empty() {
            line(String::new());
            line(format!(
                "Originals not found ({}{})",
                self.missing,
                if self.missing > self.missing_files.len() {
                    ", first 500 listed"
                } else {
                    ""
                }
            ));
            for f in &self.missing_files {
                line(format!("  {f}"));
            }
        }
        t
    }
}

impl Catalog {
    /// Lightroom id → Laika id for one library and kind ("asset"/"album").
    pub fn lightroom_links(&self, library: &str, kind: &str) -> HashMap<String, i64> {
        let mut stmt = match self
            .conn
            .prepare("SELECT lr_id, laika_id FROM lightroom_links WHERE library = ?1 AND kind = ?2")
        {
            Ok(s) => s,
            Err(_) => return HashMap::new(),
        };
        stmt.query_map(params![library, kind], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?))
        })
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default()
    }

    fn link_lightroom(
        &self,
        library: &str,
        kind: &str,
        lr_id: &str,
        laika_id: i64,
    ) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT INTO lightroom_links(library, kind, lr_id, laika_id, linked_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(library, kind, lr_id) DO UPDATE SET laika_id = ?4, linked_at = ?5",
                params![library, kind, lr_id, laika_id, chrono_stamp()],
            )
            .map(|_| ())
            .map_err(|e| format!("lightroom link: {e}"))
    }

    /// The photo stored at a path (absolute or root-relative).
    pub fn photo_id_for_path(&self, path: &Path) -> Option<i64> {
        let abs = path.to_string_lossy().to_string();
        let stored = self.stored_form(path);
        self.conn
            .query_row(
                "SELECT id FROM photos WHERE catalog_id = ?1 AND (path = ?2 OR path = ?3) LIMIT 1",
                params![self.id, abs, stored],
                |r| r.get(0),
            )
            .ok()
    }

    /// Assets already linked by an earlier import whose photo still
    /// exists (they need no matching).
    pub fn lightroom_linked_assets(&self, library: &str) -> HashMap<String, i64> {
        let live: HashSet<i64> = self.all_photos().iter().map(|p| p.id).collect();
        self.lightroom_links(library, "asset")
            .into_iter()
            .filter(|(_, id)| live.contains(id))
            .collect()
    }

    /// Place every asset: linked before, found among catalog photos, a
    /// file to add, or missing. `found` maps SHA-256 → file.
    pub fn resolve_lightroom(
        &self,
        lib: &Library,
        found: &HashMap<String, PathBuf>,
    ) -> LightroomResolution {
        let linked = self.lightroom_linked_assets(&lib.catalog_id);
        let mut out = LightroomResolution::default();
        for a in &lib.assets {
            if let Some(id) = linked.get(&a.id) {
                out.in_catalog.insert(a.id.clone(), *id);
                continue;
            }
            match found.get(&a.sha256) {
                Some(path) => match self.photo_id_for_path(path) {
                    Some(id) => {
                        out.in_catalog.insert(a.id.clone(), id);
                    }
                    None => {
                        out.to_import.insert(a.id.clone(), path.clone());
                    }
                },
                None => out.missing.push(a.id.clone()),
            }
        }
        out
    }

    /// Apply ratings, flags, titles, locations, develop settings, and
    /// albums to the photos each asset resolved to. `photo_of` maps asset
    /// id → photo id (after any new files were imported). One transaction.
    pub fn apply_lightroom(
        &self,
        lib: &Library,
        photo_of: &HashMap<String, i64>,
        opts: &LightroomOptions,
    ) -> Result<LightroomReport, String> {
        let mut report = LightroomReport {
            library: lib.path.display().to_string(),
            photos: lib.assets.len(),
            virtual_copies: lib.stats.virtual_copies,
            stacks: lib.stats.stacks,
            smart_albums: lib.stats.smart_albums,
            people: lib.stats.people,
            faces: lib.stats.with_faces,
            ..Default::default()
        };
        self.conn
            .execute_batch("BEGIN")
            .map_err(|e| format!("lightroom import: {e}"))?;
        let result = self.apply_lightroom_inner(lib, photo_of, opts, &mut report);
        match result {
            Ok(()) => {
                self.conn
                    .execute_batch("COMMIT")
                    .map_err(|e| format!("lightroom import: {e}"))?;
                report.changed.sort_unstable();
                report.changed.dedup();
                Ok(report)
            }
            Err(e) => {
                self.conn.execute_batch("ROLLBACK").ok();
                Err(e)
            }
        }
    }

    fn apply_lightroom_inner(
        &self,
        lib: &Library,
        photo_of: &HashMap<String, i64>,
        opts: &LightroomOptions,
        report: &mut LightroomReport,
    ) -> Result<(), String> {
        let library = lib.catalog_id.as_str();
        let overwrite = opts.overwrite;
        // Current develop params, to tell "Laika already has Lightroom's
        // settings" (an earlier import) from a real difference.
        let current_edits: HashMap<i64, [f32; crate::edit::PARAM_COUNT]> = if opts.develop {
            self.load_all_edits()
                .into_iter()
                .map(|(id, e)| (id, e.params))
                .collect()
        } else {
            HashMap::new()
        };
        for a in &lib.assets {
            let Some(&pid) = photo_of.get(&a.id) else {
                report.missing += 1;
                if report.missing_files.len() < 500 {
                    report.missing_files.push(if a.captured_at.is_empty() {
                        a.file_name.clone()
                    } else {
                        format!("{} ({})", a.file_name, a.captured_at)
                    });
                }
                continue;
            };
            self.link_lightroom(library, "asset", &a.id, pid)?;
            report.linked += 1;
            // A virtual copy shares its master's original; its own
            // settings would overwrite the master's.
            if a.copy_of.is_some() {
                continue;
            }
            let Some(photo) = self.photo_by_id(pid) else {
                continue;
            };
            let mut changed = false;
            if a.rating > 0 && photo.rating != a.rating {
                if overwrite || photo.rating == 0 {
                    self.set_rating(pid, a.rating)?;
                    report.ratings += 1;
                    changed = true;
                } else {
                    report.kept += 1;
                }
            }
            let flag = (a.flag == Flag::Pick, a.flag == Flag::Reject);
            if a.flag != Flag::None && (photo.picked, photo.rejected) != flag {
                if overwrite || (!photo.picked && !photo.rejected) {
                    self.set_flag(pid, flag.0, flag.1)?;
                    report.flags += 1;
                    changed = true;
                } else {
                    report.kept += 1;
                }
            }
            let mut meta = self.photo_meta(pid);
            let mut meta_changed = false;
            if !a.title.is_empty() && meta.title != a.title {
                if overwrite || meta.title.trim().is_empty() {
                    meta.title = a.title.clone();
                    meta_changed = true;
                    report.titles += 1;
                } else {
                    report.kept += 1;
                }
            }
            if let Some(loc) = &a.location {
                let current = crate::geo::parse_gps(&photo.exif_gps);
                let same = current.is_some_and(|(la, lo)| {
                    (la - loc.lat).abs() < 1e-5 && (lo - loc.lon).abs() < 1e-5
                });
                if same {
                    // Already there (usually from the file's own EXIF).
                } else if overwrite || current.is_none() {
                    self.set_gps(pid, Some((loc.lat, loc.lon)))?;
                    report.locations += 1;
                    changed = true;
                } else {
                    report.kept += 1;
                }
                let label = loc.label();
                if !label.is_empty()
                    && (overwrite || meta.location.trim().is_empty())
                    && meta.location != label
                {
                    meta.location = label;
                    meta_changed = true;
                }
            }
            if meta_changed {
                self.set_photo_meta(pid, &meta)?;
                changed = true;
            }
            if !a.keywords.is_empty() {
                let current = self.photo_keywords(pid);
                let wanted = if overwrite {
                    a.keywords.clone()
                } else {
                    let mut merged = current.clone();
                    for keyword in &a.keywords {
                        if !merged.contains(keyword) {
                            merged.push(keyword.clone());
                        }
                    }
                    merged
                };
                if wanted != current {
                    self.set_keywords(pid, &wanted)?;
                    report.keywords += 1;
                    changed = true;
                }
            }
            if opts.develop {
                if let Some(sha) = &a.develop {
                    match lib.develop_packet(sha) {
                        None => report.develop_unavailable += 1,
                        Some(packet) => {
                            let side = crate::xmp::parse(&packet);
                            let has_edits = self.edits_updated_at(pid).is_some();
                            let same = match (&side, current_edits.get(&pid)) {
                                (Some(side), Some(cur)) => side
                                    .params
                                    .iter()
                                    .zip(cur.iter())
                                    .all(|(a, b)| (a - b).abs() < 1e-4),
                                _ => false,
                            };
                            if same {
                                // Already applied by an earlier import.
                            } else if has_edits && !overwrite {
                                report.kept += 1;
                            } else if let Some(side) = side {
                                if side.has_tone || side.geom.is_some() {
                                    self.save_params(
                                        pid,
                                        &crate::edit::Edit {
                                            params: side.params,
                                            history: Vec::new(),
                                            cursor: 0,
                                            crop: self.crop_of(pid),
                                            geom: side.geom.unwrap_or_else(|| self.geom_of(pid)),
                                            curve_on: true,
                                            hsl_on: true,
                                            detail_on: true,
                                            optics_on: true,
                                            effects_on: true,
                                            grading_on: true,
                                            locals: Default::default(),
                                            camera_profile: Default::default(),
                                        },
                                    )?;
                                    report.develop += 1;
                                    changed = true;
                                    for u in crate::lightroom::unsupported_settings(&packet) {
                                        *report.unsupported.entry(u).or_insert(0) += 1;
                                    }
                                }
                            }
                        }
                    }
                }
            }
            if changed {
                report.changed.push(pid);
            }
        }

        // Albums → collections.
        let names = lib.collection_names();
        let links = self.lightroom_links(library, "album");
        let existing: HashMap<i64, String> = self
            .collections()
            .into_iter()
            .filter(|c| !c.quick)
            .map(|c| (c.id, c.name))
            .collect();
        for album in lib.importable_albums() {
            if !opts.albums.contains(&album.id) {
                continue;
            }
            let members: Vec<i64> = {
                let mut seen = HashSet::new();
                album
                    .members
                    .iter()
                    .filter_map(|a| photo_of.get(a).copied())
                    .filter(|p| seen.insert(*p))
                    .collect()
            };
            if members.is_empty() {
                continue;
            }
            let cid = match links.get(&album.id).filter(|id| existing.contains_key(id)) {
                Some(&id) => {
                    report.collections_updated += 1;
                    id
                }
                None => {
                    let base = names
                        .get(&album.id)
                        .cloned()
                        .unwrap_or_else(|| album.name.clone());
                    let taken = |n: &str| existing.values().any(|e| e.eq_ignore_ascii_case(n));
                    let mut name = base.clone();
                    let mut i = 1;
                    while taken(&name) || self.create_collection_check(&name).is_err() {
                        name = if i == 1 {
                            format!("{} (Lightroom)", truncate(&base, 66))
                        } else {
                            format!("{} (Lightroom {i})", truncate(&base, 62))
                        };
                        i += 1;
                        if i > 50 {
                            return Err(format!("couldn't name a collection for {}", album.name));
                        }
                    }
                    let id = self.create_collection(&name)?;
                    report.collections_created += 1;
                    id
                }
            };
            self.link_lightroom(library, "album", &album.id, cid)?;
            report.collection_members += self.add_to_collection(cid, &members)?;
        }
        Ok(())
    }

    fn create_collection_check(&self, name: &str) -> Result<String, String> {
        self.check_collection_name(name, None)
    }
}

fn truncate(s: &str, n: usize) -> String {
    s.chars().take(n).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lightroom::{Album, AlbumKind, Asset, Location, Media, Stats};

    fn workdir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("laika-lr-{name}-{}", std::process::id()));
        std::fs::remove_dir_all(&d).ok();
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn jpeg(path: &Path, w: u32, shade: u8) {
        let img = image::RgbImage::from_pixel(w, 20, image::Rgb([shade, 80, 120]));
        img.save(path).unwrap();
    }

    fn asset(id: &str, file: &Path) -> Asset {
        Asset {
            id: id.to_string(),
            media: Media::Image,
            file_name: file.file_name().unwrap().to_string_lossy().into_owned(),
            file_size: std::fs::metadata(file).unwrap().len(),
            sha256: crate::lightroom::sha256_file(file).unwrap(),
            captured_at: "2024-05-01T10:00:00".to_string(),
            rating: 0,
            flag: Flag::None,
            title: String::new(),
            keywords: Vec::new(),
            location: None,
            develop: None,
            copy_of: None,
            faces: 0,
        }
    }

    #[test]
    fn lightroom_library_applies_once_and_fills_only_blanks() {
        let dir = workdir("apply");
        let lr_dir = dir.join("Lib.lrlibrary").join("cat1");
        std::fs::create_dir_all(lr_dir.join("settings")).unwrap();
        let photos = dir.join("photos");
        std::fs::create_dir_all(&photos).unwrap();
        let (a_path, b_path, c_path) = (
            photos.join("a.jpg"),
            photos.join("b.jpg"),
            photos.join("c.jpg"),
        );
        jpeg(&a_path, 30, 10);
        jpeg(&b_path, 31, 20);
        jpeg(&c_path, 32, 30);
        let cat = Catalog::open(&dir.join("c.db"), "c", &dir).unwrap();
        let cache = dir.join("cache");
        let a_id = cat.import_file(&a_path, &cache).unwrap().unwrap();
        let b_id = cat.import_file(&b_path, &cache).unwrap().unwrap();
        // Laika already rated b: fill-blanks keeps it.
        cat.set_rating(b_id, 2).unwrap();

        let settings = "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\
            <rdf:Description xmlns:crs=\"http://ns.adobe.com/camera-raw-settings/1.0/\" crs:Exposure2012=\"+1.25\" crs:LensProfileEnable=\"1\"/>\
            </rdf:RDF></x:xmpmeta>";
        let sha_settings = "ab".repeat(32);
        std::fs::write(lr_dir.join("settings").join(&sha_settings), settings).unwrap();

        let mut a = asset("A", &a_path);
        a.rating = 4;
        a.flag = Flag::Pick;
        a.title = "Harbor at dawn".to_string();
        a.keywords = vec![
            "Places > Portugal > Lisbon".to_string(),
            "Harbor".to_string(),
        ];
        a.location = Some(Location {
            lat: 33.0348,
            lon: -96.8915,
            city: "Lewisville".to_string(),
            state: "Texas".to_string(),
            ..Default::default()
        });
        a.develop = Some(sha_settings.clone());
        let mut b = asset("B", &b_path);
        b.rating = 5;
        let c = asset("C", &c_path);
        let mut gone = asset("GONE", &c_path);
        gone.sha256 = "cd".repeat(32);
        gone.file_name = "lost.nef".to_string();
        let lib = Library {
            path: dir.join("Lib.lrlibrary"),
            dir: lr_dir,
            catalog_id: "cat1".to_string(),
            assets: vec![a, b, c, gone],
            albums: vec![
                Album {
                    id: "trip".to_string(),
                    name: "Trip".to_string(),
                    kind: AlbumKind::Album,
                    parent: None,
                    members: vec!["C".to_string(), "A".to_string(), "GONE".to_string()],
                    custom_order: true,
                },
                Album {
                    id: "smart".to_string(),
                    name: "Smart".to_string(),
                    kind: AlbumKind::Smart,
                    parent: None,
                    members: vec![],
                    custom_order: false,
                },
            ],
            stats: Stats::default(),
        };

        // Match by content: a and b are in the catalog; c is a new file.
        let found: HashMap<String, PathBuf> = [&a_path, &b_path, &c_path]
            .iter()
            .map(|p| (crate::lightroom::sha256_file(p).unwrap(), p.to_path_buf()))
            .collect();
        let res = cat.resolve_lightroom(&lib, &found);
        assert_eq!(res.in_catalog.len(), 2);
        assert_eq!(res.to_import.get("C"), Some(&c_path));
        assert_eq!(res.missing, vec!["GONE".to_string()]);
        let c_id = cat.import_file(&c_path, &cache).unwrap().unwrap();
        let mut photo_of = res.in_catalog.clone();
        photo_of.insert("C".to_string(), c_id);

        let opts = LightroomOptions {
            albums: ["trip".to_string()].into_iter().collect(),
            develop: true,
            overwrite: false,
        };
        let report = cat.apply_lightroom(&lib, &photo_of, &opts).unwrap();
        assert_eq!(report.linked, 3);
        assert_eq!(report.missing, 1);
        assert_eq!(
            report.missing_files,
            vec!["lost.nef (2024-05-01T10:00:00)".to_string()]
        );
        assert_eq!(
            (
                report.ratings,
                report.flags,
                report.titles,
                report.keywords,
                report.locations
            ),
            (1, 1, 1, 1, 1)
        );
        assert_eq!(report.develop, 1);
        assert_eq!(report.kept, 1, "b's Laika rating kept");
        assert_eq!(report.unsupported.get("Lens profile correction"), Some(&1));

        let pa = cat.photo_by_id(a_id).unwrap();
        assert_eq!((pa.rating, pa.picked), (4, true));
        assert!(pa.exif_gps.starts_with("33.0348"));
        assert_eq!(cat.photo_meta(a_id).title, "Harbor at dawn");
        assert_eq!(cat.photo_meta(a_id).location, "Lewisville, Texas");
        assert_eq!(
            cat.photo_keywords(a_id),
            vec!["Harbor", "Places > Portugal > Lisbon"]
        );
        assert_eq!(cat.photo_by_id(b_id).unwrap().rating, 2);
        let exposure = crate::xmp::PARAM_KEYS
            .iter()
            .find(|(k, _)| *k == "Exposure2012")
            .unwrap()
            .1;
        let edits: HashMap<i64, crate::edit::Edit> = cat.load_all_edits().into_iter().collect();
        assert!((edits[&a_id].params[exposure] - 1.25).abs() < 1e-4);

        // Album order follows Lightroom; missing members drop out.
        let trip = cat
            .collections()
            .into_iter()
            .find(|c| c.name == "Trip")
            .unwrap();
        assert_eq!(cat.album_order(trip.id), vec![c_id, a_id]);

        // Importing again changes nothing and reuses the collection.
        let res2 = cat.resolve_lightroom(&lib, &HashMap::new());
        assert_eq!(res2.in_catalog.len(), 3, "links replace matching");
        let again = cat.apply_lightroom(&lib, &res2.in_catalog, &opts).unwrap();
        assert_eq!(
            (
                again.ratings,
                again.flags,
                again.titles,
                again.keywords,
                again.develop
            ),
            (0, 0, 0, 0, 0)
        );
        assert_eq!(
            again.kept, 1,
            "only b's differing rating; a's imported settings aren't a conflict"
        );
        assert_eq!(
            (again.collections_created, again.collections_updated),
            (0, 1)
        );
        assert_eq!(again.collection_members, 0);
        assert_eq!(
            cat.collections()
                .iter()
                .filter(|c| c.name.starts_with("Trip"))
                .count(),
            1
        );

        // Replace mode takes Lightroom's values.
        let replace = LightroomOptions {
            overwrite: true,
            ..opts
        };
        let r3 = cat
            .apply_lightroom(&lib, &res2.in_catalog, &replace)
            .unwrap();
        assert_eq!(r3.ratings, 1);
        assert_eq!(cat.photo_by_id(b_id).unwrap().rating, 5);
        std::fs::remove_dir_all(&dir).ok();
    }
}
