//! G19/G24: the build manifest, the page fingerprint, and the diff that
//! tells the publish sheet (and the incremental build) what will change.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use laika_core::gallery::Gallery;
use serde::{Deserialize, Serialize};

pub const MANIFEST_FILE: &str = "manifest.json";
pub const MANIFEST_VERSION: u32 = 1;

/// One derivative written for a photo.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Derivative {
    pub width: u32,
    pub height: u32,
    /// Relative to the build root (`img/<key>-<w>.jpg`).
    pub file: String,
    pub bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ManifestPhoto {
    pub photo_id: i64,
    /// Source content hash + edit fingerprint (see [`photo_key`]).
    pub key: String,
    pub files: Vec<Derivative>,
    /// Desktop placement (col, row, span_x, span_y), 0-based.
    pub placement: (u16, u16, u8, u8),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u32,
    pub laika_version: String,
    pub gallery_id: i64,
    pub slug: String,
    pub title: String,
    /// Unix seconds.
    pub built_at: String,
    /// Fingerprint of everything on the page except pixels.
    pub page_hash: String,
    /// Derivative widths requested (before the no-upscale clamp).
    pub sizes: Vec<u32>,
    pub photos: Vec<ManifestPhoto>,
}

impl Manifest {
    pub fn read(dir: &Path) -> Option<Manifest> {
        let bytes = std::fs::read(dir.join(MANIFEST_FILE)).ok()?;
        let m: Manifest = serde_json::from_slice(&bytes).ok()?;
        (m.version == MANIFEST_VERSION).then_some(m)
    }

    pub fn total_bytes(&self) -> u64 {
        self.photos
            .iter()
            .flat_map(|p| p.files.iter())
            .map(|f| f.bytes)
            .sum()
    }
}

/// Derivative file stem for a photo: the source's content hash plus the
/// edit fingerprint, so re-edits get new names and unchanged photos keep
/// theirs (browser caches and hosts dedupe on that).
pub fn photo_key(source_hash: &str, edit_key: &str) -> String {
    let h = blake3::hash(format!("{source_hash}|{edit_key}").as_bytes());
    h.to_hex()[..16].to_string()
}

/// Fingerprint of the page's text, theme and layout (not pixels).
pub fn page_hash(g: &Gallery) -> String {
    let photos: Vec<serde_json::Value> = g
        .photos
        .iter()
        .filter(|p| p.cell.is_some())
        .map(|p| {
            serde_json::json!([
                p.photo_id,
                p.cell,
                p.span_x,
                p.span_y,
                p.caption,
                p.alt_text,
                [p.focal.0, p.focal.1],
                p.fit,
                p.open_full_size,
            ])
        })
        .collect();
    let v = serde_json::json!({
        "title": g.title, "subtitle": g.subtitle, "eyebrow": g.eyebrow,
        "site": g.site_name, "meta": g.meta_line, "template": g.template,
        "columns": g.columns, "gutter": g.gutter, "ratio": g.ratio,
        "theme": g.theme, "downloads": g.allow_downloads, "sizes": g.sizes,
        "photos": photos, "renderer": super::html::RENDERER_VERSION,
    });
    blake3::hash(v.to_string().as_bytes()).to_hex()[..16].to_string()
}

/// What a build will change relative to the last one.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BuildDiff {
    /// No previous build to compare against.
    pub first_build: bool,
    pub added: Vec<i64>,
    pub removed: Vec<i64>,
    /// In both builds, but the pixels changed (edit or source changed).
    pub reencoded: Vec<i64>,
    pub unchanged: usize,
    pub page_changed: bool,
}

impl BuildDiff {
    pub fn is_empty(&self) -> bool {
        !self.first_build
            && self.added.is_empty()
            && self.removed.is_empty()
            && self.reencoded.is_empty()
            && !self.page_changed
    }

    /// "3 new images, 1 re-rendered, 45 unchanged, page text changed".
    pub fn summary(&self) -> String {
        if self.first_build {
            return format!("first build · {} images", self.added.len());
        }
        let mut parts = Vec::new();
        let plural = |n: usize, one: &str, many: &str| {
            format!("{n} {}", if n == 1 { one } else { many })
        };
        if !self.added.is_empty() {
            parts.push(plural(self.added.len(), "new image", "new images"));
        }
        if !self.reencoded.is_empty() {
            parts.push(format!("{} re-rendered", self.reencoded.len()));
        }
        if !self.removed.is_empty() {
            parts.push(format!("{} removed", self.removed.len()));
        }
        parts.push(format!("{} unchanged", self.unchanged));
        if self.page_changed {
            parts.push("page changed".to_string());
        }
        if self.is_empty() {
            return "nothing changed since the last build".to_string();
        }
        parts.join(", ")
    }
}

/// Compare the planned photos (id, key) and page hash with a manifest.
pub fn diff(prev: Option<&Manifest>, planned: &[(i64, String)], page_hash: &str) -> BuildDiff {
    let Some(prev) = prev else {
        return BuildDiff {
            first_build: true,
            added: planned.iter().map(|p| p.0).collect(),
            page_changed: true,
            ..Default::default()
        };
    };
    let old: HashMap<i64, &str> = prev
        .photos
        .iter()
        .map(|p| (p.photo_id, p.key.as_str()))
        .collect();
    let now: HashSet<i64> = planned.iter().map(|p| p.0).collect();
    let mut d = BuildDiff {
        page_changed: prev.page_hash != page_hash,
        ..Default::default()
    };
    for (id, key) in planned {
        match old.get(id) {
            None => d.added.push(*id),
            Some(k) if *k != key => d.reencoded.push(*id),
            Some(_) => d.unchanged += 1,
        }
    }
    d.removed = prev
        .photos
        .iter()
        .map(|p| p.photo_id)
        .filter(|id| !now.contains(id))
        .collect();
    d
}

/// Pre-build size estimate from the previous build's actual bytes per
/// photo; None before any build ("no estimate yet").
pub fn estimate_bytes(prev: Option<&Manifest>, photo_count: usize) -> Option<u64> {
    let prev = prev?;
    if prev.photos.is_empty() {
        return None;
    }
    Some(prev.total_bytes() / prev.photos.len() as u64 * photo_count as u64)
}
