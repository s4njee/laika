//! S07: a visible, resumable way to leave Laika.
//!
//! Planning snapshots the live catalog on its owning thread. `run` owns only
//! ordinary data and filesystem paths, so the app can execute it in the
//! background. Every regular file is content-verified after writing. Re-running
//! into the same bundle reuses files whose hashes already match and repairs the
//! rest.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

use crate::catalog::{Catalog, Collection, DbPhoto, hash_file};
use crate::edit::{self, Edit};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExitBundleOptions {
    /// Put verified byte-for-byte originals inside `Originals/`.
    pub copy_originals: bool,
    /// The app consumes this flag after the portable portion finishes and
    /// adds full-resolution developed JPEGs through its normal renderer.
    pub rendered_jpegs: bool,
}

#[derive(Clone, Debug)]
pub struct ExitBundlePhoto {
    pub photo: DbPhoto,
    pub edit: Edit,
    pub authorship: crate::xmp::Authorship,
}

#[derive(Clone, Debug, Serialize)]
pub struct ExitBundleCollection {
    pub id: i64,
    pub name: String,
    pub title: String,
    pub description: String,
    pub quick: bool,
    pub smart: bool,
    pub criteria_json: String,
    pub members: Vec<i64>,
}

#[derive(Clone, Debug)]
pub struct ExitBundlePlan {
    pub destination: PathBuf,
    pub catalog_name: String,
    pub catalog_identity: String,
    pub created_at: String,
    pub catalog_snapshot: PathBuf,
    pub options: ExitBundleOptions,
    pub photos: Vec<ExitBundlePhoto>,
    pub collections: Vec<ExitBundleCollection>,
}

#[derive(Clone, Debug, Default)]
pub struct ExitBundleProgress {
    pub done: usize,
    pub total: usize,
    pub current: String,
}

#[derive(Clone, Debug, Default)]
pub struct ExitBundleReport {
    pub path: PathBuf,
    pub photos: usize,
    pub sidecars: usize,
    pub originals_copied: usize,
    pub files_written: usize,
    pub files_reused: usize,
    pub bytes: u64,
    pub missing_originals: Vec<String>,
    pub errors: Vec<String>,
    pub complete: bool,
}

#[derive(Clone, Debug, Serialize)]
struct FileRecord {
    path: String,
    blake3: String,
    bytes: u64,
    kind: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ResumeState {
    format: String,
    version: u32,
    catalog_identity: String,
    catalog_name: String,
    created_at: String,
    options: ExitBundleOptions,
}

struct RemoveDirOnDrop(PathBuf);

impl Drop for RemoveDirOnDrop {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).ok();
    }
}

impl Catalog {
    /// Freeze everything needed for an exit bundle. SQLite stays on this
    /// thread; the returned plan is safe to move into a background worker.
    pub fn plan_exit_bundle(
        &self,
        parent: &Path,
        options: ExitBundleOptions,
    ) -> Result<ExitBundlePlan, String> {
        std::fs::create_dir_all(parent).map_err(|e| format!("create {}: {e}", parent.display()))?;
        let catalog_name = self.catalog_name();
        let db_identity = self.db_path().to_string_lossy();
        let catalog_identity = blake3::hash(db_identity.as_bytes()).to_hex().to_string();
        let base = format!(
            "{}-leave-laika",
            crate::sync::sanitize_catalog(&catalog_name)
        );
        let wanted = parent.join(&base);
        let mut destination = wanted.clone();
        let mut created_at = crate::sidecar::now_iso();
        if wanted.exists() {
            let state = std::fs::read(wanted.join(".laika-export-state.json"))
                .ok()
                .and_then(|b| serde_json::from_slice::<ResumeState>(&b).ok());
            if let Some(state) = state.filter(|s| s.catalog_identity == catalog_identity) {
                created_at = state.created_at;
            } else {
                destination = crate::import::unique_dest_name(parent, &base);
            }
        }

        let snapshot_dir = std::env::temp_dir().join(format!(
            "laika-exit-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let catalog_snapshot = self.backup_db(&snapshot_dir)?;
        let edits: HashMap<i64, Edit> = self.load_all_edits().into_iter().collect();
        let keywords = self.keywords_by_photo();
        let photos = self.all_photos();
        let bundle_photos = photos
            .iter()
            .map(|photo| ExitBundlePhoto {
                photo: photo.clone(),
                edit: edits.get(&photo.id).cloned().unwrap_or_else(Edit::new),
                authorship: self.photo_authorship(photo.id),
            })
            .collect();
        let collections = self
            .collections()
            .into_iter()
            .map(|collection| {
                let members = collection_members(self, &collection, &photos, &keywords);
                ExitBundleCollection {
                    id: collection.id,
                    name: collection.name,
                    title: collection.title,
                    description: collection.description,
                    quick: collection.quick,
                    smart: collection.smart,
                    criteria_json: collection.criteria_json,
                    members,
                }
            })
            .collect();
        Ok(ExitBundlePlan {
            destination,
            catalog_name,
            catalog_identity,
            created_at,
            catalog_snapshot,
            options,
            photos: bundle_photos,
            collections,
        })
    }
}

fn collection_members(
    catalog: &Catalog,
    collection: &Collection,
    photos: &[DbPhoto],
    keywords: &HashMap<i64, Vec<String>>,
) -> Vec<i64> {
    if !collection.smart {
        return catalog.album_order(collection.id);
    }
    let Ok(filters) = serde_json::from_str::<crate::state::Filters>(&collection.criteria_json)
    else {
        return Vec::new();
    };
    photos
        .iter()
        .filter(|p| filters.matches_db(p, keywords.get(&p.id).map(Vec::as_slice).unwrap_or(&[])))
        .map(|p| p.id)
        .collect()
}

impl ExitBundlePlan {
    pub fn total_steps(&self) -> usize {
        5 + self.photos.len() * if self.options.copy_originals { 2 } else { 1 }
            + self.collections.len()
    }

    /// Write or resume the bundle. A cancellation or failed original leaves
    /// the folder intact; running the same export again verifies and reuses
    /// completed files before continuing.
    pub fn run(
        self,
        cancel: &AtomicBool,
        progress: impl Fn(ExitBundleProgress),
    ) -> Result<ExitBundleReport, String> {
        // The catalog snapshot is only a handoff from the SQLite-owning
        // thread. Clean it up on success, pause, or any early error.
        let _snapshot_cleanup = RemoveDirOnDrop(
            self.catalog_snapshot
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .to_path_buf(),
        );
        let mut report = ExitBundleReport {
            path: self.destination.clone(),
            photos: self.photos.len(),
            ..Default::default()
        };
        let mut records = Vec::new();
        let mut done = 0usize;
        let total = self.total_steps();
        let step = |done: usize, current: &str| {
            progress(ExitBundleProgress {
                done,
                total,
                current: current.to_string(),
            })
        };
        std::fs::create_dir_all(&self.destination)
            .map_err(|e| format!("create {}: {e}", self.destination.display()))?;
        let state = ResumeState {
            format: "laika-leave-bundle".to_string(),
            version: 1,
            catalog_identity: self.catalog_identity.clone(),
            catalog_name: self.catalog_name.clone(),
            created_at: self.created_at.clone(),
            options: self.options,
        };
        let state_bytes = serde_json::to_vec_pretty(&state).map_err(|e| e.to_string())?;
        write_verified(
            &self.destination,
            Path::new(".laika-export-state.json"),
            &state_bytes,
            "resume-state",
            &mut records,
            &mut report,
        )?;

        check_cancel(cancel)?;
        step(done, "catalog");
        copy_verified(
            &self.destination,
            Path::new("Catalog/catalog.db"),
            &self.catalog_snapshot,
            None,
            "catalog",
            &mut records,
            &mut report,
        )?;
        done += 1;

        let mut photo_manifest = Vec::with_capacity(self.photos.len());
        for item in &self.photos {
            check_cancel(cancel)?;
            let photo = &item.photo;
            step(done, &format!("XMP · {}", photo.filename));
            let portable_filename = safe_name(&photo.filename);
            let side_name = if photo.is_raw() {
                Path::new(&portable_filename)
                    .with_extension("xmp")
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("photo.xmp")
                    .to_string()
            } else {
                format!("{}.xmp", portable_filename)
            };
            let side_rel = PathBuf::from("Sidecars")
                .join(photo.id.to_string())
                .join(&side_name);
            let existing = std::fs::read(crate::sidecar::path_for(&photo.path)).ok();
            let params = effective_params(&item.edit);
            let history: Vec<String> = item
                .edit
                .history
                .iter()
                .take(item.edit.cursor)
                .map(|h| h.label.clone())
                .collect();
            let xmp = crate::sidecar::merge(
                existing.as_deref(),
                &params,
                photo.rating,
                &history,
                None,
                &item.authorship,
                &item.edit.geom,
                crate::sidecar::WriteScope::default(),
                &self.created_at,
            )?;
            write_verified(
                &self.destination,
                &side_rel,
                &xmp,
                "xmp",
                &mut records,
                &mut report,
            )?;
            report.sidecars += 1;
            // When originals are included, put a second verified XMP beside
            // each copy so Lightroom/darktable can import Originals/ directly.
            if self.options.copy_originals {
                let adjacent_rel = PathBuf::from("Originals")
                    .join(photo.id.to_string())
                    .join(&side_name);
                write_verified(
                    &self.destination,
                    &adjacent_rel,
                    &xmp,
                    "xmp-adjacent",
                    &mut records,
                    &mut report,
                )?;
            }
            done += 1;

            let original_rel = PathBuf::from("Originals")
                .join(photo.id.to_string())
                .join(&portable_filename);
            let mut copied = None;
            if self.options.copy_originals {
                check_cancel(cancel)?;
                step(done, &format!("Original · {}", photo.filename));
                if Path::new(&photo.path).is_file() {
                    match copy_verified(
                        &self.destination,
                        &original_rel,
                        Path::new(&photo.path),
                        Some(&photo.blake3),
                        "original",
                        &mut records,
                        &mut report,
                    ) {
                        Ok(()) => {
                            report.originals_copied += 1;
                            copied = Some(path_text(&original_rel));
                        }
                        Err(e) => report.errors.push(format!("{}: {e}", photo.filename)),
                    }
                } else {
                    report.missing_originals.push(photo.path.clone());
                }
                done += 1;
            } else if !Path::new(&photo.path).is_file() {
                report.missing_originals.push(photo.path.clone());
            }
            photo_manifest.push(serde_json::json!({
                "id": photo.id,
                "filename": photo.filename,
                "original_path": photo.path,
                "original_blake3": photo.blake3,
                "captured_at": photo.captured_at,
                "captured_original": photo.captured_orig,
                "capture_offset_minutes": photo.capture_offset_min,
                "camera": photo.camera,
                "lens": photo.lens,
                "focal_length_mm": photo.focal_mm,
                "aperture": photo.aperture,
                "shutter": photo.shutter,
                "iso": photo.iso,
                "width": photo.width,
                "height": photo.height,
                "remote_backup_key": photo.remote_key,
                "rating": photo.rating,
                "picked": photo.picked,
                "rejected": photo.rejected,
                "label": item.authorship.label,
                "keywords": item.authorship.keywords,
                "xmp": path_text(&side_rel),
                "original_copy": copied,
            }));
        }

        check_cancel(cancel)?;
        step(done, "metadata.csv");
        let csv = metadata_csv(&self.photos);
        write_verified(
            &self.destination,
            Path::new("metadata.csv"),
            csv.as_bytes(),
            "metadata-csv",
            &mut records,
            &mut report,
        )?;
        done += 1;

        step(done, "collections");
        let collections_bytes = serde_json::to_vec_pretty(&self.collections)
            .map_err(|e| format!("collections JSON: {e}"))?;
        write_verified(
            &self.destination,
            Path::new("collections.json"),
            &collections_bytes,
            "collections-json",
            &mut records,
            &mut report,
        )?;
        done += 1;
        for collection in &self.collections {
            check_cancel(cancel)?;
            step(done, &format!("Collection · {}", collection.name));
            write_collection_links(&self, collection, &mut records, &mut report)?;
            done += 1;
        }

        step(done, "README");
        let readme = readme(self.options);
        write_verified(
            &self.destination,
            Path::new("README.md"),
            readme.as_bytes(),
            "instructions",
            &mut records,
            &mut report,
        )?;
        let manifest = serde_json::json!({
            "format": "laika-leave-bundle",
            "version": 1,
            "catalog_name": self.catalog_name,
            "created_at": self.created_at,
            "options": self.options,
            "complete": report.errors.is_empty()
                && report.missing_originals.is_empty()
                && !self.options.rendered_jpegs,
            "photos": photo_manifest,
            "collections": self.collections,
            "files": records,
            "missing_originals": report.missing_originals,
            "errors": report.errors,
        });
        let manifest_bytes =
            serde_json::to_vec_pretty(&manifest).map_err(|e| format!("manifest JSON: {e}"))?;
        // The manifest cannot hash itself; COMPLETE contains its verified hash.
        crate::xmp::write_atomic(
            &self.destination.join("manifest.json").to_string_lossy(),
            &manifest_bytes,
        )?;
        let manifest_hash = hash_file(&self.destination.join("manifest.json"))?;
        // A requested rendered set is finalized by the GUI renderer. Until
        // then VERIFY.txt must not claim the bundle is complete.
        let complete = report.errors.is_empty()
            && report.missing_originals.is_empty()
            && !self.options.rendered_jpegs;
        let marker = format!(
            "manifest.blake3={manifest_hash}\ncomplete={}\n",
            if complete { "true" } else { "false" }
        );
        crate::xmp::write_atomic(
            &self.destination.join("VERIFY.txt").to_string_lossy(),
            marker.as_bytes(),
        )?;
        report.complete = complete;
        step(
            total,
            if complete {
                "Complete"
            } else {
                "Needs attention"
            },
        );
        Ok(report)
    }
}

fn check_cancel(cancel: &AtomicBool) -> Result<(), String> {
    if cancel.load(Ordering::Relaxed) {
        Err("export paused — choose the same destination to resume".to_string())
    } else {
        Ok(())
    }
}

fn effective_params(edit: &Edit) -> [f32; edit::PARAM_COUNT] {
    let mut p = edit.params;
    let d = edit::defaults();
    if !edit.curve_on {
        p[edit::CURVE_RANGE].copy_from_slice(&d[edit::CURVE_RANGE]);
    }
    if !edit.hsl_on {
        p[edit::HSL_RANGE].copy_from_slice(&d[edit::HSL_RANGE]);
    }
    if !edit.detail_on {
        p[edit::DETAIL_RANGE].copy_from_slice(&d[edit::DETAIL_RANGE]);
    }
    if !edit.optics_on {
        p[edit::OPTICS_RANGE].copy_from_slice(&d[edit::OPTICS_RANGE]);
    }
    if !edit.effects_on {
        for i in edit::EFFECTS_PARAMS {
            p[i] = d[i];
        }
    }
    if !edit.grading_on {
        p[edit::GRADING_RANGE].copy_from_slice(&d[edit::GRADING_RANGE]);
    }
    p
}

fn write_verified(
    root: &Path,
    rel: &Path,
    bytes: &[u8],
    kind: &str,
    records: &mut Vec<FileRecord>,
    report: &mut ExitBundleReport,
) -> Result<(), String> {
    let dest = root.join(rel);
    let expected = blake3::hash(bytes).to_hex().to_string();
    if dest.is_file() && hash_file(&dest).ok().as_deref() == Some(expected.as_str()) {
        report.files_reused += 1;
    } else {
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create {}: {e}", parent.display()))?;
        }
        crate::xmp::write_atomic(&dest.to_string_lossy(), bytes)?;
        let got = hash_file(&dest)?;
        if got != expected {
            return Err(format!("verify {}: {got} != {expected}", dest.display()));
        }
        report.files_written += 1;
        report.bytes += bytes.len() as u64;
    }
    records.push(FileRecord {
        path: path_text(rel),
        blake3: expected,
        bytes: bytes.len() as u64,
        kind: kind.to_string(),
    });
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn copy_verified(
    root: &Path,
    rel: &Path,
    source: &Path,
    expected: Option<&str>,
    kind: &str,
    records: &mut Vec<FileRecord>,
    report: &mut ExitBundleReport,
) -> Result<(), String> {
    let source_hash = hash_file(source)?;
    if let Some(expected) = expected
        && source_hash != expected
    {
        return Err(format!(
            "source changed: catalog hash {expected}, file hash {source_hash}"
        ));
    }
    let dest = root.join(rel);
    let bytes = std::fs::metadata(source).map(|m| m.len()).unwrap_or(0);
    if dest.is_file() && hash_file(&dest).ok().as_deref() == Some(source_hash.as_str()) {
        report.files_reused += 1;
    } else {
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create {}: {e}", parent.display()))?;
        }
        let tmp = dest.with_extension(format!(
            "{}.part-{}",
            dest.extension().and_then(|e| e.to_str()).unwrap_or("file"),
            std::process::id()
        ));
        let mut input =
            std::fs::File::open(source).map_err(|e| format!("read {}: {e}", source.display()))?;
        let mut output =
            std::fs::File::create(&tmp).map_err(|e| format!("write {}: {e}", tmp.display()))?;
        let mut buf = vec![0u8; 1 << 20];
        loop {
            let n = input.read(&mut buf).map_err(|e| e.to_string())?;
            if n == 0 {
                break;
            }
            output.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        }
        output.sync_all().map_err(|e| e.to_string())?;
        drop(output);
        let got = hash_file(&tmp)?;
        if got != source_hash {
            std::fs::remove_file(&tmp).ok();
            return Err(format!("verify {}: {got} != {source_hash}", tmp.display()));
        }
        std::fs::rename(&tmp, &dest).map_err(|e| {
            std::fs::remove_file(&tmp).ok();
            format!("finish {}: {e}", dest.display())
        })?;
        report.files_written += 1;
        report.bytes += bytes;
    }
    records.push(FileRecord {
        path: path_text(rel),
        blake3: source_hash,
        bytes,
        kind: kind.to_string(),
    });
    Ok(())
}

fn metadata_csv(photos: &[ExitBundlePhoto]) -> String {
    let mut out = "id,filename,original_path,blake3,captured_at,captured_original,capture_offset_minutes,camera,lens,focal_length_mm,aperture,shutter,iso,width,height,rating,picked,rejected,label,title,caption,headline,location,creator,copyright,rights,contact,gps,keywords\n".to_string();
    for item in photos {
        let p = &item.photo;
        let a = &item.authorship;
        let values = vec![
            p.id.to_string(),
            p.filename.clone(),
            p.path.clone(),
            p.blake3.clone(),
            p.captured_at.clone(),
            p.captured_orig.clone(),
            p.capture_offset_min.to_string(),
            p.camera.clone(),
            p.lens.clone(),
            p.focal_mm.clone(),
            p.aperture.clone(),
            p.shutter.clone(),
            p.iso.clone(),
            p.width.to_string(),
            p.height.to_string(),
            p.rating.to_string(),
            p.picked.to_string(),
            p.rejected.to_string(),
            a.label.clone(),
            a.title.clone(),
            a.caption.clone(),
            a.headline.clone(),
            a.location.clone(),
            a.creator.clone(),
            a.copyright.clone(),
            a.rights_usage.clone(),
            a.contact.clone(),
            a.gps.clone(),
            a.keywords.join(" | "),
        ];
        out.push_str(
            &values
                .into_iter()
                .map(|v| csv(&v))
                .collect::<Vec<_>>()
                .join(","),
        );
        out.push('\n');
    }
    out
}

fn csv(value: &str) -> String {
    if value.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

fn safe_name(name: &str) -> String {
    let out = crate::template::sanitize_segment(name.trim());
    if out.is_empty() {
        "Collection".to_string()
    } else {
        out.chars().take(100).collect()
    }
}

fn write_collection_links(
    plan: &ExitBundlePlan,
    collection: &ExitBundleCollection,
    records: &mut Vec<FileRecord>,
    report: &mut ExitBundleReport,
) -> Result<(), String> {
    let dir = plan.destination.join("Collections").join(format!(
        "{}-{}",
        collection.id,
        safe_name(&collection.name)
    ));
    std::fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    let by_id: HashMap<i64, &DbPhoto> =
        plan.photos.iter().map(|p| (p.photo.id, &p.photo)).collect();
    for (index, id) in collection.members.iter().enumerate() {
        let Some(photo) = by_id.get(id) else { continue };
        let name = format!("{:05}-{}", index + 1, safe_name(&photo.filename));
        let link = dir.join(name);
        let target = if plan.options.copy_originals {
            // The alias sits two levels below the bundle root, so a relative
            // target keeps the whole bundle movable after export.
            PathBuf::from("../..")
                .join("Originals")
                .join(id.to_string())
                .join(safe_name(&photo.filename))
        } else {
            PathBuf::from(&photo.path)
        };
        #[cfg(unix)]
        {
            use std::os::unix::fs::symlink;
            let wanted = target.to_string_lossy().to_string();
            if link.exists() || link.symlink_metadata().is_ok() {
                if std::fs::read_link(&link).ok().as_deref() == Some(target.as_path()) {
                    report.files_reused += 1;
                } else {
                    std::fs::remove_file(&link).map_err(|e| e.to_string())?;
                    symlink(&target, &link)
                        .map_err(|e| format!("alias {}: {e}", link.display()))?;
                    report.files_written += 1;
                }
            } else {
                symlink(&target, &link).map_err(|e| format!("alias {}: {e}", link.display()))?;
                report.files_written += 1;
            }
            if std::fs::read_link(&link).ok().as_deref() != Some(target.as_path()) {
                return Err(format!("verify alias {}", link.display()));
            }
            records.push(FileRecord {
                path: path_text(link.strip_prefix(&plan.destination).unwrap_or(&link)),
                blake3: blake3::hash(wanted.as_bytes()).to_hex().to_string(),
                bytes: wanted.len() as u64,
                kind: "collection-alias".to_string(),
            });
        }
        #[cfg(not(unix))]
        {
            let rel = link
                .strip_prefix(&plan.destination)
                .unwrap_or(&link)
                .with_extension("txt");
            write_verified(
                &plan.destination,
                &rel,
                target.to_string_lossy().as_bytes(),
                "collection-link",
                records,
                report,
            )?;
        }
    }
    Ok(())
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().replace('\\', "/")
}

fn readme(options: ExitBundleOptions) -> String {
    format!(
        "# Leaving Laika\n\nThis bundle is deliberately made from ordinary files. `manifest.json` maps every photo to its original path, BLAKE3 hash, exported XMP, metadata, and collections. `VERIFY.txt` pins the manifest hash. Re-run Export Everything into the same parent folder to verify and resume it.\n\n## Contents\n\n- `Catalog/catalog.db`: an integrity-checked Laika SQLite snapshot.\n- `Sidecars/`: one Adobe-compatible XMP packet per photo. Ratings, color-label names, keywords, descriptive metadata, GPS, crop, and supported `crs:` edits live here. Unknown valid properties from an existing sidecar are retained. Laika masks, heals, camera looks, and panel history remain in the catalog and are not claimed to render identically elsewhere.\n- `metadata.csv`: spreadsheet-friendly metadata for every photo.\n- `collections.json`: albums, smart criteria, descriptions, and ordered member IDs.\n- `Collections/`: Finder-visible symbolic aliases in album order.\n- `Originals/`: {}\n- `Rendered JPEGs/`: {}\n\n## Lightroom Classic\n\nIf originals were included, import `Originals/` directly: verified XMP copies are already beside the matching files. Otherwise place each file from `Sidecars/` beside the original named by `manifest.json`, then use Synchronize Folder / Read Metadata from Files. This preserves standard ratings, label names, keywords, IPTC fields, GPS, and supported Camera Raw settings. Collection membership is in `collections.json`; Lightroom does not import arbitrary collection JSON automatically.\n\n## darktable\n\nImport `Originals/` directly when it is present; otherwise place the exported XMP beside each original first. darktable reads standard ratings and keywords; develop rendering is not expected to match Adobe or Laika.\n\n## Capture One\n\nImport the originals, then use Load Metadata for standard rating, color-tag, keyword, and IPTC fields. Camera Raw `crs:` develop settings generally do not translate.\n\n## Apple Photos\n\nImport originals or rendered JPEGs. Photos does not consume these sidecars as a complete develop recipe; use `metadata.csv` as the audit record.\n",
        if options.copy_originals {
            "verified byte-for-byte copies, grouped by Laika photo ID"
        } else {
            "not included; use the original paths and hashes in manifest.json"
        },
        if options.rendered_jpegs {
            "requested; full-resolution developed JPEGs are added by the Laika app"
        } else {
            "not requested"
        }
    )
}

/// Add/refresh the optional rendered-JPEG records, then verify every file
/// already named by the manifest and rewrite the completion marker. This is
/// separate because full rendering lives in the GUI/export crate, not core.
pub fn finalize_rendered_jpegs(
    bundle: &Path,
    expected: usize,
    render_errors: &[String],
) -> Result<(usize, u64, bool), String> {
    let manifest_path = bundle.join("manifest.json");
    let mut manifest: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&manifest_path).map_err(|e| format!("read manifest: {e}"))?,
    )
    .map_err(|e| format!("parse manifest: {e}"))?;
    let files = manifest
        .get_mut("files")
        .and_then(|v| v.as_array_mut())
        .ok_or("manifest has no files")?;
    files.retain(|record| record["kind"] != "rendered-jpeg" && record["kind"] != "rendered-xmp");
    let mut rendered = 0usize;
    let mut bytes = 0u64;
    let render_dir = bundle.join("Rendered JPEGs");
    if render_dir.is_dir() {
        let mut stack = vec![render_dir];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir)
                .map_err(|e| format!("read {}: {e}", dir.display()))?
                .flatten()
            {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if !path.is_file() {
                    continue;
                }
                let rel = path.strip_prefix(bundle).unwrap_or(&path);
                let len = entry.metadata().map(|m| m.len()).unwrap_or(0);
                let kind = if path.extension().and_then(|e| e.to_str()) == Some("xmp") {
                    "rendered-xmp"
                } else {
                    rendered += 1;
                    "rendered-jpeg"
                };
                files.push(serde_json::json!({
                    "path": path_text(rel),
                    "blake3": hash_file(&path)?,
                    "bytes": len,
                    "kind": kind,
                }));
                bytes += len;
            }
        }
    }
    // Re-verify regular records. Alias records hash their link target text,
    // not the target file bytes, and were already checked when created.
    let mut errors = render_errors.to_vec();
    for record in files.iter() {
        if record["kind"] == "collection-alias" {
            let Some(rel) = record["path"].as_str() else {
                continue;
            };
            let Some(expected_hash) = record["blake3"].as_str() else {
                continue;
            };
            let path = bundle.join(rel);
            match std::fs::read_link(&path) {
                Ok(target)
                    if blake3::hash(target.to_string_lossy().as_bytes())
                        .to_hex()
                        .as_str()
                        == expected_hash => {}
                Ok(target) => errors.push(format!(
                    "{}: alias target changed to {}",
                    path.display(),
                    target.display()
                )),
                Err(e) => errors.push(format!("read alias {}: {e}", path.display())),
            }
            continue;
        }
        let Some(rel) = record["path"].as_str() else {
            continue;
        };
        let Some(expected_hash) = record["blake3"].as_str() else {
            continue;
        };
        let path = bundle.join(rel);
        match hash_file(&path) {
            Ok(got) if got == expected_hash => {}
            Ok(got) => errors.push(format!("{}: {got} != {expected_hash}", path.display())),
            Err(e) => errors.push(e),
        }
    }
    if rendered != expected {
        errors.push(format!("rendered {rendered} of {expected} requested JPEGs"));
    }
    let missing = manifest
        .get("missing_originals")
        .and_then(|v| v.as_array())
        .is_some_and(|v| !v.is_empty());
    let prior = manifest
        .get("errors")
        .and_then(|v| v.as_array())
        .is_some_and(|v| !v.is_empty());
    let complete = errors.is_empty() && !missing && !prior;
    manifest["rendered_jpegs"] = serde_json::json!(rendered);
    manifest["render_errors"] = serde_json::json!(errors);
    manifest["complete"] = serde_json::json!(complete);
    let bytes_json = serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?;
    crate::xmp::write_atomic(&manifest_path.to_string_lossy(), &bytes_json)?;
    let manifest_hash = hash_file(&manifest_path)?;
    let marker = format!(
        "manifest.blake3={manifest_hash}\ncomplete={}\n",
        if complete { "true" } else { "false" }
    );
    crate::xmp::write_atomic(
        &bundle.join("VERIFY.txt").to_string_lossy(),
        marker.as_bytes(),
    )?;
    Ok((rendered, bytes, complete))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workdir() -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "laika-exit-bundle-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn exports_every_photo_xmp_metadata_collections_and_resumes() {
        let dir = workdir();
        let originals = dir.join("photos");
        std::fs::create_dir_all(&originals).unwrap();
        let a = originals.join("a.jpg");
        let b = originals.join("b.jpg");
        image::RgbImage::from_pixel(32, 24, image::Rgb([20, 40, 60]))
            .save(&a)
            .unwrap();
        image::RgbImage::from_pixel(32, 24, image::Rgb([60, 40, 20]))
            .save(&b)
            .unwrap();
        let cat = Catalog::open(&dir.join("catalog.db"), "Exit Test", &dir).unwrap();
        let cache = dir.join("cache");
        let aid = cat.import_file(&a, &cache).unwrap().unwrap();
        let bid = cat.import_file(&b, &cache).unwrap().unwrap();
        cat.set_rating(aid, 4).unwrap();
        cat.set_keywords(aid, &["Places > Coast".to_string()])
            .unwrap();
        let album = cat.create_collection("Portfolio / Coast").unwrap();
        cat.add_to_collection(album, &[bid, aid]).unwrap();
        let foreign = br#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:xmp="http://ns.adobe.com/xap/1.0/" xmlns:other="https://example.test/" xmp:Rating="2" other:keep="yes"/></rdf:RDF></x:xmpmeta>"#;
        std::fs::write(crate::sidecar::path_for(&a.to_string_lossy()), foreign).unwrap();

        let options = ExitBundleOptions {
            copy_originals: true,
            rendered_jpegs: false,
        };
        let plan = cat.plan_exit_bundle(&dir.join("out"), options).unwrap();
        let destination = plan.destination.clone();
        let first = plan.run(&AtomicBool::new(false), |_| {}).unwrap();
        assert!(first.complete, "{:?}", first.errors);
        assert_eq!(
            (first.photos, first.sidecars, first.originals_copied),
            (2, 2, 2)
        );
        assert!(destination.join("metadata.csv").is_file());
        assert!(destination.join("collections.json").is_file());
        assert!(destination.join("README.md").is_file());
        assert!(destination.join("VERIFY.txt").is_file());
        let manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(destination.join("manifest.json")).unwrap())
                .unwrap();
        assert_eq!(manifest["photos"].as_array().unwrap().len(), 2);
        assert!(manifest["photos"][0].get("iso").is_some());
        assert!(
            manifest["photos"][0]
                .get("capture_offset_minutes")
                .is_some()
        );
        let xmp =
            std::fs::read_to_string(destination.join(format!("Sidecars/{aid}/a.jpg.xmp"))).unwrap();
        assert!(xmp.contains("other:keep=\"yes\""));
        assert!(xmp.contains("xmp:Rating=\"4\""));
        assert!(xmp.contains("Places|Coast"));
        assert_eq!(
            hash_file(&destination.join(format!("Originals/{aid}/a.jpg"))).unwrap(),
            hash_file(&a).unwrap()
        );
        assert!(
            destination
                .join(format!("Originals/{aid}/a.jpg.xmp"))
                .is_file()
        );
        let collections: serde_json::Value =
            serde_json::from_slice(&std::fs::read(destination.join("collections.json")).unwrap())
                .unwrap();
        assert!(collections.as_array().unwrap().iter().any(|c| {
            c["name"] == "Portfolio / Coast" && c["members"] == serde_json::json!([bid, aid])
        }));

        // Same catalog + destination resumes and verifies instead of duplicating.
        let second = cat
            .plan_exit_bundle(&dir.join("out"), options)
            .unwrap()
            .run(&AtomicBool::new(false), |_| {})
            .unwrap();
        assert_eq!(second.path, destination);
        assert!(second.files_reused > 0);
        assert_eq!(
            std::fs::read_dir(dir.join("out")).unwrap().count(),
            1,
            "resume does not make a second bundle"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rendered_finalize_and_pause_keep_verification_honest() {
        let dir = workdir();
        let original = dir.join("photo.jpg");
        image::RgbImage::from_pixel(16, 12, image::Rgb([8, 16, 32]))
            .save(&original)
            .unwrap();
        let cat = Catalog::open(&dir.join("catalog.db"), "Resume Test", &dir).unwrap();
        cat.import_file(&original, &dir.join("cache"))
            .unwrap()
            .unwrap();
        let options = ExitBundleOptions {
            copy_originals: false,
            rendered_jpegs: true,
        };
        let paused = AtomicBool::new(true);
        let plan = cat.plan_exit_bundle(&dir.join("out"), options).unwrap();
        let bundle = plan.destination.clone();
        assert!(plan.run(&paused, |_| {}).is_err());
        assert!(bundle.join(".laika-export-state.json").is_file());

        let report = cat
            .plan_exit_bundle(&dir.join("out"), options)
            .unwrap()
            .run(&AtomicBool::new(false), |_| {})
            .unwrap();
        assert_eq!(report.path, bundle);
        assert!(!report.complete, "rendered output has not been finalized");
        std::fs::create_dir_all(bundle.join("Rendered JPEGs")).unwrap();
        std::fs::write(
            bundle.join("Rendered JPEGs/photo-00001.jpg"),
            b"jpeg fixture",
        )
        .unwrap();
        let (rendered, _, complete) = finalize_rendered_jpegs(&bundle, 1, &[]).unwrap();
        assert_eq!(rendered, 1);
        assert!(complete);
        assert!(
            std::fs::read_to_string(bundle.join("VERIFY.txt"))
                .unwrap()
                .contains("complete=true")
        );

        // A later mutation is detected by the same final verifier.
        std::fs::write(bundle.join("metadata.csv"), b"tampered").unwrap();
        let (_, _, complete) = finalize_rendered_jpegs(&bundle, 1, &[]).unwrap();
        assert!(!complete);
        std::fs::remove_dir_all(&dir).ok();
    }
}
