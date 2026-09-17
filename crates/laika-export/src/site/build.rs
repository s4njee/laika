//! G19: build a gallery into a static folder — derivatives, page, script,
//! fonts, manifest — incrementally and atomically.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use laika_core::gallery::Gallery;
use laika_core::gallery::layout::Breakpoint;

use super::html::{self, PagePhoto};
use super::manifest::{self, Derivative, MANIFEST_FILE, MANIFEST_VERSION, Manifest, ManifestPhoto};
use crate::formats::{ExportFormat, ExportFormatOpts, encode_pixels};

/// JPEG quality for gallery derivatives.
pub const QUALITY: u8 = 86;

/// What the app knows about one gallery photo before rendering.
#[derive(Clone, Debug)]
pub struct SitePhoto {
    pub photo_id: i64,
    /// Source content hash (catalog blake3).
    pub source_hash: String,
    /// Fingerprint of the develop settings + crop that shape the pixels.
    pub edit_key: String,
    /// Shown in failure lists.
    pub name: String,
}

pub struct BuildOpts {
    /// Final folder (replaced atomically on success).
    pub out_dir: PathBuf,
    /// Override the gallery's sizes (Preview builds 1280 only).
    pub sizes: Option<Vec<u32>>,
    /// `assets/fonts` to copy in (missing → system font fallback).
    pub fonts_dir: Option<PathBuf>,
    /// "Laika 0.1.0" (generator meta + footer + manifest).
    pub generator: String,
    pub laika_version: String,
    /// Photos rendered/encoded at once (each holds a full-size image).
    pub workers: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildProgress {
    pub done: usize,
    pub total: usize,
    pub current: String,
}

#[derive(Clone, Debug, Default)]
pub struct BuildReport {
    pub out_dir: PathBuf,
    pub photos: usize,
    /// Photos whose pixels were rendered this build.
    pub rendered: usize,
    /// Photos reused from the previous build (no re-encode).
    pub reused: usize,
    /// (name, reason) per photo that failed; the rest still built.
    pub failures: Vec<(String, String)>,
    pub bytes: u64,
    pub diff: manifest::BuildDiff,
    pub manifest: Option<Manifest>,
}

impl BuildReport {
    pub fn summary(&self) -> String {
        let mut s = format!(
            "{} photos · {} rendered, {} reused",
            self.photos, self.rendered, self.reused
        );
        if !self.failures.is_empty() {
            s.push_str(&format!(" · {} failed", self.failures.len()));
        }
        s
    }
}

/// Target widths for an image of `w`×`h`: each requested size that fits
/// the long edge, plus the long edge itself when every size is larger
/// (never upscale). Sizes apply to the long edge.
pub fn derivative_edges(w: u32, h: u32, sizes: &[u32]) -> Vec<u32> {
    let long = w.max(h).max(1);
    let mut out: Vec<u32> = sizes.iter().copied().filter(|s| *s > 0 && *s <= long).collect();
    if sizes.iter().any(|s| *s > long) {
        out.push(long);
    }
    out.sort_unstable();
    out.dedup();
    if out.is_empty() {
        out.push(long);
    }
    out
}

fn resized_dims(w: u32, h: u32, long_edge: u32) -> (u32, u32) {
    if w >= h {
        (long_edge, ((h as u64 * long_edge as u64) / w.max(1) as u64).max(1) as u32)
    } else {
        (((w as u64 * long_edge as u64) / h.max(1) as u64).max(1) as u32, long_edge)
    }
}

/// Placed photos in desktop reading order, with keys — what the diff and
/// the build iterate.
pub fn plan(g: &Gallery, photos: &[SitePhoto]) -> Vec<(usize, SitePhoto, String)> {
    g.layout_at(Breakpoint::Desktop)
        .into_iter()
        .filter_map(|(i, ..)| {
            let id = g.photos[i].photo_id;
            let sp = photos.iter().find(|p| p.photo_id == id)?;
            Some((i, sp.clone(), manifest::photo_key(&sp.source_hash, &sp.edit_key)))
        })
        .collect()
}

/// The G24 diff for the publish sheet, without building anything.
pub fn preview_diff(g: &Gallery, photos: &[SitePhoto], out_dir: &Path) -> manifest::BuildDiff {
    let prev = Manifest::read(out_dir);
    let planned: Vec<(i64, String)> = plan(g, photos)
        .into_iter()
        .map(|(_, p, k)| (p.photo_id, k))
        .collect();
    manifest::diff(prev.as_ref(), &planned, &manifest::page_hash(g))
}

fn link_or_copy(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::hard_link(from, to).or_else(|_| std::fs::copy(from, to).map(|_| ()))
}

/// Build the gallery. `render` returns the fully developed sRGB pixels
/// for a photo (any size ≥ the largest derivative; it is downscaled
/// here). Photos that need pixels run on `opts.workers` threads (decode,
/// resize and encode in parallel; the render closure may serialize its
/// own GPU step). Cancel is checked before each photo starts: a cancelled
/// build removes its staging folder and leaves the previous build
/// untouched.
pub fn build(
    g: &Gallery,
    photos: &[SitePhoto],
    opts: &BuildOpts,
    render: impl Fn(&SitePhoto, u32) -> Result<image::RgbaImage, String> + Sync,
    progress: impl Fn(BuildProgress) + Sync,
    cancel: &AtomicBool,
) -> Result<BuildReport, String> {
    let planned = plan(g, photos);
    if planned.is_empty() {
        return Err("place at least one photo on the page before building".to_string());
    }
    let sizes: Vec<u32> = opts
        .sizes
        .clone()
        .unwrap_or_else(|| g.sizes.clone())
        .into_iter()
        .filter(|s| *s >= 64)
        .collect();
    let sizes = if sizes.is_empty() { vec![1280] } else { sizes };
    let max_size = sizes.iter().copied().max().unwrap_or(1280);

    let out = &opts.out_dir;
    let parent = out
        .parent()
        .ok_or_else(|| "choose a folder inside another folder".to_string())?;
    std::fs::create_dir_all(parent)
        .map_err(|e| format!("can't create {}: {e}", parent.display()))?;
    let name = out
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "gallery".to_string());
    let staging = parent.join(format!(".{name}.building-{}", std::process::id()));
    if staging.exists() {
        std::fs::remove_dir_all(&staging).ok();
    }
    let img_dir = staging.join("img");
    std::fs::create_dir_all(&img_dir)
        .map_err(|e| format!("can't write to {}: {e}", parent.display()))?;
    let cleanup = |e: String| {
        std::fs::remove_dir_all(&staging).ok();
        e
    };

    let prev = Manifest::read(out);
    let page_hash = manifest::page_hash(g);
    let diff = manifest::diff(
        prev.as_ref(),
        &planned
            .iter()
            .map(|(_, p, k)| (p.photo_id, k.clone()))
            .collect::<Vec<_>>(),
        &page_hash,
    );
    let mut report = BuildReport {
        out_dir: out.clone(),
        diff,
        ..Default::default()
    };
    let total = planned.len();
    let fopts = ExportFormatOpts {
        quality: QUALITY,
        ..ExportFormatOpts::default()
    };
    let placement_of = |index: usize| {
        let p = &g.photos[index];
        let cell = p.cell.unwrap_or(laika_core::gallery::layout::Cell { col: 0, row: 0 });
        (cell.col, cell.row, p.span_x, p.span_y)
    };

    // Pass 1 (cheap, in order): reuse unchanged photos from the last build.
    let mut slots: Vec<Option<ManifestPhoto>> = vec![None; total];
    let mut pending: Vec<usize> = Vec::new();
    for (n, (index, sp, key)) in planned.iter().enumerate() {
        let reuse = prev.as_ref().and_then(|m| {
            if m.sizes != sizes {
                return None;
            }
            let old = m.photos.iter().find(|p| p.key == *key)?;
            old.files
                .iter()
                .all(|f| out.join(&f.file).is_file())
                .then_some(old)
        });
        let linked = reuse.filter(|old| {
            old.files
                .iter()
                .all(|f| link_or_copy(&out.join(&f.file), &staging.join(&f.file)).is_ok())
        });
        match linked {
            Some(old) => {
                report.reused += 1;
                slots[n] = Some(ManifestPhoto {
                    photo_id: sp.photo_id,
                    key: key.clone(),
                    files: old.files.clone(),
                    placement: placement_of(*index),
                });
            }
            None => pending.push(n),
        }
    }

    // Pass 2 (parallel): render, resize, encode, write.
    enum Outcome {
        Built(Vec<Derivative>),
        Failed(String),
    }
    let done = AtomicUsize::new(report.reused);
    let next = AtomicUsize::new(0);
    let fatal: Mutex<Option<String>> = Mutex::new(None);
    let results: Mutex<Vec<(usize, Outcome)>> = Mutex::new(Vec::new());
    let report_progress = |current: &str| {
        progress(BuildProgress {
            done: done.load(Ordering::Relaxed),
            total,
            current: current.to_string(),
        });
    };
    report_progress("");
    let workers = opts.workers.clamp(1, 16).min(pending.len().max(1));
    let work = |n: usize| -> Result<Outcome, String> {
        let (_, sp, key) = &planned[n];
        let rgba = match render(sp, max_size) {
            Ok(img) if img.width() > 0 && img.height() > 0 => img,
            Ok(_) => return Ok(Outcome::Failed("rendered an empty image".to_string())),
            Err(e) => return Ok(Outcome::Failed(e)),
        };
        let (w, h) = (rgba.width(), rgba.height());
        let mut files = Vec::new();
        // Largest first; each smaller size resamples the previous one
        // (not the full render) — same quality, a fraction of the work.
        let mut edges = derivative_edges(w, h, &sizes);
        edges.reverse();
        let mut base = rgba;
        for edge in edges {
            let (dw, dh) = resized_dims(w, h, edge);
            if (dw, dh) != (base.width(), base.height()) {
                base = image::imageops::resize(&base, dw, dh, image::imageops::FilterType::Lanczos3);
            }
            let bytes = match encode_pixels(ExportFormat::Jpeg, &fopts, &base) {
                Ok((b, _)) => b,
                Err(e) => return Ok(Outcome::Failed(e)),
            };
            let file = format!("img/{key}-{edge}.jpg");
            std::fs::write(staging.join(&file), &bytes)
                .map_err(|e| format!("write failed — disk full or read-only? ({e})"))?;
            files.push(Derivative {
                width: dw,
                height: dh,
                file,
                bytes: bytes.len() as u64,
            });
        }
        files.reverse();
        Ok(Outcome::Built(files))
    };
    std::thread::scope(|scope| {
        for _ in 0..workers {
            scope.spawn(|| {
                loop {
                    if cancel.load(Ordering::Relaxed) || fatal.lock().map(|f| f.is_some()).unwrap_or(true) {
                        return;
                    }
                    let k = next.fetch_add(1, Ordering::Relaxed);
                    let Some(&n) = pending.get(k) else {
                        return;
                    };
                    report_progress(&planned[n].1.name);
                    match work(n) {
                        Ok(outcome) => {
                            if let Ok(mut r) = results.lock() {
                                r.push((n, outcome));
                            }
                        }
                        Err(e) => {
                            if let Ok(mut f) = fatal.lock() {
                                f.get_or_insert(e);
                            }
                            return;
                        }
                    }
                    done.fetch_add(1, Ordering::Relaxed);
                    report_progress(&planned[n].1.name);
                }
            });
        }
    });
    if let Some(e) = fatal.into_inner().ok().flatten() {
        return Err(cleanup(e));
    }
    let mut outcomes = results.into_inner().unwrap_or_default();
    outcomes.sort_by_key(|(n, _)| *n);
    for (n, outcome) in outcomes {
        let (index, sp, key) = &planned[n];
        match outcome {
            Outcome::Built(files) => {
                report.rendered += 1;
                slots[n] = Some(ManifestPhoto {
                    photo_id: sp.photo_id,
                    key: key.clone(),
                    files,
                    placement: placement_of(*index),
                });
            }
            Outcome::Failed(e) => report.failures.push((sp.name.clone(), e)),
        }
    }
    // Reading order, as planned.
    let written: Vec<(usize, ManifestPhoto)> = slots
        .into_iter()
        .enumerate()
        .filter_map(|(n, m)| m.map(|m| (planned[n].0, m)))
        .collect();
    if cancel.load(Ordering::Relaxed) {
        return Err(cleanup("build cancelled — the previous build is unchanged".to_string()));
    }
    if written.is_empty() {
        let why = report
            .failures
            .first()
            .map(|(n, e)| format!("{n}: {e}"))
            .unwrap_or_default();
        return Err(cleanup(format!("no photo could be built ({why})")));
    }
    progress(BuildProgress {
        done: total,
        total,
        current: "Writing page".to_string(),
    });

    let page: Vec<PagePhoto> = written
        .iter()
        .map(|(i, m)| PagePhoto {
            index: *i,
            files: &m.files,
        })
        .collect();
    let index = html::render_index(g, &page, &opts.generator);
    let assets = staging.join("assets");
    std::fs::create_dir_all(assets.join("fonts")).map_err(|e| cleanup(e.to_string()))?;
    std::fs::write(staging.join("index.html"), index).map_err(|e| cleanup(e.to_string()))?;
    std::fs::write(assets.join("gallery.js"), html::JS).map_err(|e| cleanup(e.to_string()))?;
    if let Some(fonts) = &opts.fonts_dir {
        for f in FONT_FILES {
            let src = fonts.join(f);
            if src.is_file() {
                std::fs::copy(&src, assets.join("fonts").join(f)).map_err(|e| cleanup(e.to_string()))?;
            }
        }
    }

    let manifest = Manifest {
        version: MANIFEST_VERSION,
        laika_version: opts.laika_version.clone(),
        gallery_id: g.id,
        slug: g.slug.clone(),
        title: g.title.clone(),
        built_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs().to_string())
            .unwrap_or_default(),
        page_hash,
        sizes,
        photos: written.into_iter().map(|(_, m)| m).collect(),
    };
    let json = serde_json::to_vec_pretty(&manifest).map_err(|e| cleanup(e.to_string()))?;
    std::fs::write(staging.join(MANIFEST_FILE), json).map_err(|e| cleanup(e.to_string()))?;

    // Swap: old build aside, staging in, old build removed.
    let old = parent.join(format!(".{name}.old-{}", std::process::id()));
    if out.exists() {
        std::fs::rename(out, &old).map_err(|e| cleanup(format!("can't replace {}: {e}", out.display())))?;
    }
    if let Err(e) = std::fs::rename(&staging, out) {
        if old.exists() {
            std::fs::rename(&old, out).ok();
        }
        return Err(cleanup(format!("can't move the build into place: {e}")));
    }
    std::fs::remove_dir_all(&old).ok();

    report.photos = manifest.photos.len();
    report.bytes = manifest.total_bytes();
    report.manifest = Some(manifest);
    Ok(report)
}

pub const FONT_FILES: [&str; 5] = [
    "IBMPlexSans-Regular.ttf",
    "IBMPlexSans-Medium.ttf",
    "IBMPlexSans-SemiBold.ttf",
    "IBMPlexMono-Regular.ttf",
    "IBMPlexMono-Medium.ttf",
];
