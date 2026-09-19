//! S01: read an Adobe Lightroom library so a catalog can move into Laika.
//!
//! Lightroom (the cloud-synced desktop app) keeps its library in a
//! `.lrlibrary` package: `<catalog id>/Managed Catalog.mcat` is SQLite
//! holding one MessagePack document per asset, album, and link;
//! `<catalog id>/settings/<sha256>` holds Camera Raw settings as XMP; and
//! `<catalog id>/originals/` holds whichever originals are kept locally
//! (usually few — the rest live in Adobe's cloud).
//!
//! Laika never writes to the package. `open` reads a copied snapshot of the
//! database, and originals are found by content (the SHA-256 Lightroom
//! recorded at import), so renamed or moved files still match.

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use serde_json::Value;
use sha2_hw::Digest;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Media {
    Image,
    Video,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Flag {
    #[default]
    None,
    Pick,
    Reject,
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct Location {
    pub lat: f64,
    pub lon: f64,
    pub sublocation: String,
    pub city: String,
    pub state: String,
    pub country: String,
}

impl Location {
    /// "Castle Hills, Lewisville, Texas, United States".
    pub fn label(&self) -> String {
        [&self.sublocation, &self.city, &self.state, &self.country]
            .iter()
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Asset {
    /// Lightroom's document id.
    pub id: String,
    pub media: Media,
    /// The file name at import (Lightroom may know it by another title).
    pub file_name: String,
    pub file_size: u64,
    /// SHA-256 of the original file (lowercase hex).
    pub sha256: String,
    pub captured_at: String,
    pub rating: u8,
    pub flag: Flag,
    /// `dc:title`, when it is a real title rather than a file name.
    pub title: String,
    /// Standard flat and hierarchical XMP keywords. Hierarchical paths are
    /// normalized to Laika's `Parent > Child` spelling.
    pub keywords: Vec<String>,
    pub location: Option<Location>,
    /// Camera Raw settings blob (`settings/<sha>`), when the photo was
    /// developed away from its defaults.
    pub develop: Option<String>,
    /// Lightroom virtual copy of another asset (same original).
    pub copy_of: Option<String>,
    /// Face regions Lightroom detected (not imported; reported).
    pub faces: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlbumKind {
    Album,
    Folder,
    Smart,
    Other,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Album {
    pub id: String,
    pub name: String,
    pub kind: AlbumKind,
    pub parent: Option<String>,
    /// Member asset ids, in Lightroom's custom order when it has one.
    pub members: Vec<String>,
    pub custom_order: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stats {
    pub images: usize,
    pub videos: usize,
    pub people: usize,
    pub stacks: usize,
    pub stacked: usize,
    pub virtual_copies: usize,
    pub smart_albums: usize,
    pub other_albums: usize,
    pub rated: usize,
    pub flagged: usize,
    pub located: usize,
    pub edited: usize,
    /// Edited photos whose settings blob is on this Mac.
    pub edits_local: usize,
    pub with_faces: usize,
    pub local_originals: usize,
}

#[derive(Clone, Debug)]
pub struct Library {
    /// The `.lrlibrary` package.
    pub path: PathBuf,
    /// `<package>/<catalog id>`.
    pub dir: PathBuf,
    /// Lightroom's catalog id (the link key for re-imports).
    pub catalog_id: String,
    pub assets: Vec<Asset>,
    pub albums: Vec<Album>,
    pub stats: Stats,
}

/// `.lrlibrary` packages in `~/Pictures` (Lightroom's default location).
pub fn default_libraries(home: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(home.join("Pictures"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "lrlibrary") && p.is_dir())
        .collect();
    out.sort();
    out
}

/// Whether a path looks like something `open` can read.
pub fn is_library(path: &Path) -> bool {
    catalog_dir(path).is_some()
}

/// The catalog folder inside a package: the one holding the largest
/// `Managed Catalog.mcat` (packages also carry a small profiles catalog).
fn catalog_dir(package: &Path) -> Option<PathBuf> {
    std::fs::read_dir(package)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter_map(|d| {
            let size = std::fs::metadata(d.join("Managed Catalog.mcat"))
                .ok()?
                .len();
            Some((size, d))
        })
        .max_by_key(|(size, _)| *size)
        .map(|(_, d)| d)
}

fn s(v: &Value, path: &[&str]) -> String {
    let mut cur = v;
    for k in path {
        match cur.get(k) {
            Some(n) => cur = n,
            None => return String::new(),
        }
    }
    match cur {
        Value::String(s) => s.clone(),
        Value::Array(a) => a
            .iter()
            .filter_map(|x| x.as_str())
            .collect::<Vec<_>>()
            .join(", "),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn string_values(value: Option<&Value>) -> Vec<String> {
    fn collect(value: &Value, out: &mut Vec<String>) {
        match value {
            Value::String(s) => {
                let s = s.trim();
                if !s.is_empty() && !out.iter().any(|v| v == s) {
                    out.push(s.to_string());
                }
            }
            Value::Array(values) => {
                for value in values {
                    collect(value, out);
                }
            }
            Value::Object(values) => {
                for value in values.values() {
                    collect(value, out);
                }
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    if let Some(value) = value {
        collect(value, &mut out);
    }
    out
}

/// Lightroom has used both arrays and id-keyed maps for RDF bags. Prefer
/// hierarchical subjects, then retain flat subjects that are not merely a
/// leaf already represented by a hierarchy.
fn keywords(doc: &Value) -> Vec<String> {
    let xmp = doc.get("xmp");
    let hierarchical = xmp
        .and_then(|v| v.get("lr"))
        .and_then(|v| v.get("hierarchicalSubject"))
        .or_else(|| xmp.and_then(|v| v.get("lr:hierarchicalSubject")));
    let flat = xmp
        .and_then(|v| v.get("dc"))
        .and_then(|v| v.get("subject"))
        .or_else(|| xmp.and_then(|v| v.get("dc:subject")));
    let mut out: Vec<String> = string_values(hierarchical)
        .into_iter()
        .map(|s| s.split('|').map(str::trim).collect::<Vec<_>>().join(" > "))
        .filter(|s| !s.is_empty())
        .collect();
    for keyword in string_values(flat) {
        let covered = out.iter().any(|path| {
            path.rsplit(" > ")
                .next()
                .is_some_and(|leaf| leaf == keyword)
        });
        if !covered && !out.contains(&keyword) {
            out.push(keyword);
        }
    }
    out
}

fn is_hex_digest(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Titles Lightroom copies from a file name ("DSC_1380.NEF") are not
/// titles a photographer wrote.
fn looks_like_file_name(t: &str) -> bool {
    let t = t.trim();
    if laika_raw::media_kind(Path::new(t)).is_some() {
        return true;
    }
    match t.rsplit_once('.') {
        Some((stem, ext)) => {
            !stem.is_empty()
                && !stem.contains(' ')
                && (2..=5).contains(&ext.len())
                && ext.chars().all(|c| c.is_ascii_alphanumeric())
        }
        None => false,
    }
}

impl Library {
    /// Read a library package. The database is copied first, so Lightroom
    /// may keep running and its files are never modified.
    pub fn open(package: &Path) -> Result<Library, String> {
        let dir = catalog_dir(package)
            .ok_or_else(|| format!("{} is not a Lightroom library", package.display()))?;
        let snap = std::env::temp_dir().join(format!(
            "laika-lightroom-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&snap).map_err(|e| format!("snapshot folder: {e}"))?;
        let result = (|| {
            for suffix in ["", "-wal", "-shm"] {
                let name = format!("Managed Catalog.mcat{suffix}");
                let from = dir.join(&name);
                if from.exists() {
                    std::fs::copy(&from, snap.join(&name))
                        .map_err(|e| format!("copy the Lightroom catalog: {e}"))?;
                }
            }
            let conn = rusqlite::Connection::open(snap.join("Managed Catalog.mcat"))
                .map_err(|e| format!("open the Lightroom catalog: {e}"))?;
            Self::read(&conn, package, &dir)
        })();
        std::fs::remove_dir_all(&snap).ok();
        result
    }

    fn read(conn: &rusqlite::Connection, package: &Path, dir: &Path) -> Result<Library, String> {
        let mut stmt = conn
            .prepare(
                "SELECT d.fullDocId, d.type, IFNULL(d.subtype, ''), r.content
                   FROM docs d JOIN revs r ON r.sequence = d.winningRevSequence
                  WHERE d.deleted = 0 AND IFNULL(r.deleted, 0) = 0",
            )
            .map_err(|e| format!("this doesn't look like a Lightroom catalog: {e}"))?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    // Stored as BLOB or TEXT depending on the writer.
                    match r.get_ref(3)? {
                        rusqlite::types::ValueRef::Blob(b) | rusqlite::types::ValueRef::Text(b) => {
                            Some(b.to_vec())
                        }
                        _ => None,
                    },
                ))
            })
            .map_err(|e| format!("read the Lightroom catalog: {e}"))?;

        let mut catalog_id = String::new();
        let mut assets = Vec::new();
        let mut albums: Vec<Album> = Vec::new();
        // album id → (asset id, order key)
        let mut members: HashMap<String, Vec<(String, String)>> = HashMap::new();
        let mut stats = Stats::default();
        let mut stack_members = 0usize;
        let mut copies: HashMap<String, Vec<(String, String)>> = HashMap::new();
        for row in rows {
            let (id, kind, subtype, content) =
                row.map_err(|e| format!("read the Lightroom catalog: {e}"))?;
            let Some(content) = content else { continue };
            let Ok(doc) = crate::msgpack::decode(&content) else {
                continue;
            };
            match (kind.as_str(), subtype.as_str()) {
                ("catalog", _) => catalog_id = id,
                ("asset", "image" | "video") => {
                    let a = Self::asset(&id, &subtype, &doc);
                    if let Some(group) = doc
                        .get("copy")
                        .and_then(|c| c.get("copy_id"))
                        .and_then(|c| c.as_str())
                    {
                        copies
                            .entry(group.to_string())
                            .or_default()
                            .push((s(&doc, &["userCreated"]), id.clone()));
                    }
                    assets.push(a);
                }
                ("asset", "person") => stats.people += 1,
                ("asset", "stack") => stats.stacks += 1,
                ("stack_asset", _) => stack_members += 1,
                ("album", sub) => {
                    let kind = match sub {
                        "collection" => AlbumKind::Album,
                        "collection_set" => AlbumKind::Folder,
                        "smart" => AlbumKind::Smart,
                        _ => AlbumKind::Other,
                    };
                    let parent = doc
                        .get("parent")
                        .and_then(|p| p.get("id"))
                        .and_then(|p| p.as_str())
                        .map(str::to_string);
                    albums.push(Album {
                        id,
                        name: s(&doc, &["name"]).trim().to_string(),
                        kind,
                        parent,
                        members: Vec::new(),
                        custom_order: s(&doc, &["assetSortOrder"]).starts_with("custom"),
                    });
                }
                ("album_asset", _) => {
                    let album = s(&doc, &["album", "id"]);
                    let asset = s(&doc, &["asset", "id"]);
                    if !album.is_empty() && !asset.is_empty() {
                        members
                            .entry(album)
                            .or_default()
                            .push((asset, s(&doc, &["order"])));
                    }
                }
                _ => {}
            }
        }
        if assets.is_empty() && albums.is_empty() {
            return Err("the Lightroom library is empty".to_string());
        }

        // Virtual copies: the earliest asset of a group is the master.
        for group in copies.values_mut() {
            group.sort();
            let master = group[0].1.clone();
            for (_, id) in group.iter().skip(1) {
                if let Some(a) = assets.iter_mut().find(|a| a.id == *id) {
                    a.copy_of = Some(master.clone());
                }
            }
        }

        let known: HashMap<&str, &Asset> = assets.iter().map(|a| (a.id.as_str(), a)).collect();
        for album in &mut albums {
            let mut list = members.remove(&album.id).unwrap_or_default();
            list.retain(|(a, _)| known.contains_key(a.as_str()));
            if list.iter().any(|(_, o)| !o.is_empty()) {
                list.sort_by(|a, b| a.1.cmp(&b.1));
            } else {
                list.sort_by(|a, b| {
                    let ca = &known[a.0.as_str()].captured_at;
                    let cb = &known[b.0.as_str()].captured_at;
                    ca.cmp(cb).then(a.0.cmp(&b.0))
                });
            }
            let mut seen = HashSet::new();
            album.members = list
                .into_iter()
                .map(|(a, _)| a)
                .filter(|a| seen.insert(a.clone()))
                .collect();
        }

        let settings = dir.join("settings");
        for a in &assets {
            match a.media {
                Media::Image => stats.images += 1,
                Media::Video => stats.videos += 1,
            }
            stats.rated += (a.rating > 0) as usize;
            stats.flagged += (a.flag != Flag::None) as usize;
            stats.located += a.location.is_some() as usize;
            stats.virtual_copies += a.copy_of.is_some() as usize;
            stats.with_faces += (a.faces > 0) as usize;
            if let Some(sha) = &a.develop {
                stats.edited += 1;
                stats.edits_local += settings.join(sha).is_file() as usize;
            }
        }
        stats.stacked = stack_members;
        stats.smart_albums = albums.iter().filter(|a| a.kind == AlbumKind::Smart).count();
        stats.other_albums = albums.iter().filter(|a| a.kind == AlbumKind::Other).count();
        let lib = Library {
            path: package.to_path_buf(),
            dir: dir.to_path_buf(),
            catalog_id: if catalog_id.is_empty() {
                dir.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default()
            } else {
                catalog_id
            },
            assets,
            albums,
            stats,
        };
        Ok(Library {
            stats: Stats {
                local_originals: lib.local_originals().len(),
                ..lib.stats.clone()
            },
            ..lib
        })
    }

    fn asset(id: &str, subtype: &str, doc: &Value) -> Asset {
        let rating = doc
            .get("ratings")
            .and_then(|r| r.as_object())
            .into_iter()
            .flat_map(|m| m.values())
            .filter_map(|v| v.get("rating").and_then(|r| r.as_u64()))
            .max()
            .unwrap_or(0)
            .min(5) as u8;
        let flag = doc
            .get("reviews")
            .and_then(|r| r.as_object())
            .into_iter()
            .flat_map(|m| m.values())
            .filter_map(|v| v.get("flag").and_then(|f| f.as_str()))
            .map(|f| match f {
                "pick" => Flag::Pick,
                "reject" => Flag::Reject,
                _ => Flag::None,
            })
            .find(|f| *f != Flag::None)
            .unwrap_or_default();
        let location = doc.get("location").and_then(|l| {
            let lat = l.get("latitude")?.as_f64()?;
            let lon = l.get("longitude")?.as_f64()?;
            (lat.abs() <= 90. && lon.abs() <= 180. && (lat != 0. || lon != 0.)).then(|| Location {
                lat,
                lon,
                sublocation: s(l, &["sublocation"]),
                city: s(l, &["city"]),
                state: s(l, &["state"]),
                country: s(l, &["country"]),
            })
        });
        let dev = doc.get("develop");
        let from_defaults = dev
            .and_then(|d| d.get("fromDefaults"))
            .and_then(|f| f.as_bool())
            .unwrap_or(false);
        let develop = dev
            .map(|d| s(d, &["xmpCameraRaw", "sha256"]))
            .filter(|sha| is_hex_digest(sha) && !from_defaults)
            .map(|sha| sha.to_ascii_lowercase());
        let title = s(doc, &["xmp", "dc", "title"]);
        let faces = doc
            .get("xmp")
            .and_then(|x| x.get("mwg-rs"))
            .and_then(|r| r.get("Regions"))
            .and_then(|r| r.get("RegionList"))
            .and_then(|l| l.as_object())
            .map(|l| {
                l.values()
                    .filter(|r| r.get("Type").and_then(|t| t.as_str()) == Some("Face"))
                    .count()
            })
            .unwrap_or(0);
        Asset {
            id: id.to_string(),
            media: if subtype == "video" {
                Media::Video
            } else {
                Media::Image
            },
            file_name: s(doc, &["importSource", "fileName"]),
            file_size: doc
                .get("importSource")
                .and_then(|i| i.get("fileSize"))
                .and_then(|n| n.as_u64())
                .unwrap_or(0),
            sha256: s(doc, &["importSource", "sha256"]).to_ascii_lowercase(),
            captured_at: s(doc, &["captureDate"]),
            rating,
            flag,
            title: if looks_like_file_name(&title) {
                String::new()
            } else {
                title.trim().to_string()
            },
            keywords: keywords(doc),
            location,
            develop,
            copy_of: None,
            faces,
        }
    }

    /// Camera Raw settings for an asset, when the blob is on this Mac.
    pub fn develop_packet(&self, sha: &str) -> Option<Vec<u8>> {
        if !is_hex_digest(sha) {
            return None;
        }
        std::fs::read(self.dir.join("settings").join(sha)).ok()
    }

    /// Originals Lightroom keeps inside the package.
    pub fn local_originals(&self) -> Vec<PathBuf> {
        walk_media(&self.dir.join("originals"), true)
    }

    /// Albums that can become Laika collections (not folders, smart
    /// albums, or empty ones).
    pub fn importable_albums(&self) -> Vec<&Album> {
        self.albums
            .iter()
            .filter(|a| a.kind == AlbumKind::Album && !a.members.is_empty())
            .collect()
    }

    /// Collection names for albums: the album's own name, with parent
    /// folder names prepended only as far as needed to tell apart albums
    /// that share a name ("Albums › RAW" vs "Archive › RAW").
    pub fn collection_names(&self) -> HashMap<String, String> {
        let by_id: HashMap<&str, &Album> = self.albums.iter().map(|a| (a.id.as_str(), a)).collect();
        let chain = |album: &Album| -> Vec<String> {
            let mut parts = vec![album.name.clone()];
            let mut cur = album.parent.as_deref();
            while let Some(pid) = cur {
                let Some(p) = by_id.get(pid) else { break };
                if parts.len() > 16 || p.name.is_empty() {
                    break;
                }
                parts.push(p.name.clone());
                cur = p.parent.as_deref();
            }
            parts
        };
        let albums = self.importable_albums();
        let chains: Vec<(String, Vec<String>)> =
            albums.iter().map(|a| (a.id.clone(), chain(a))).collect();
        let name_at = |parts: &[String], depth: usize| -> String {
            let n = (depth + 1).min(parts.len());
            let mut v: Vec<&str> = parts[..n].iter().map(|s| s.as_str()).collect();
            v.reverse();
            let name = v.join(" › ");
            let name = name.trim();
            if name.is_empty() {
                "Lightroom album".to_string()
            } else {
                name.chars().take(80).collect()
            }
        };
        let mut depth: HashMap<&str, usize> =
            chains.iter().map(|(id, _)| (id.as_str(), 0)).collect();
        for _ in 0..16 {
            let mut groups: HashMap<String, Vec<&str>> = HashMap::new();
            for (id, parts) in &chains {
                groups
                    .entry(name_at(parts, depth[id.as_str()]).to_lowercase())
                    .or_default()
                    .push(id.as_str());
            }
            let mut changed = false;
            for ids in groups.values().filter(|ids| ids.len() > 1) {
                for id in ids {
                    let parts = &chains.iter().find(|(i, _)| i == id).unwrap().1;
                    if depth[id] + 1 < parts.len() {
                        *depth.get_mut(id).unwrap() += 1;
                        changed = true;
                    }
                }
            }
            if !changed {
                break;
            }
        }
        chains
            .iter()
            .map(|(id, parts)| (id.clone(), name_at(parts, depth[id.as_str()])))
            .collect()
    }
}

/// Media files under a folder. Photo library packages are skipped (they
/// are matched through the catalog instead) unless `inside_package`.
pub fn walk_media(root: &Path, inside_package: bool) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name();
            let name = name.to_string_lossy();
            if name.starts_with('.') {
                continue;
            }
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                let package = [".photoslibrary", ".lrlibrary", ".lrdata", ".app"]
                    .iter()
                    .any(|x| name.ends_with(x));
                if !package || inside_package {
                    stack.push(p);
                }
            } else if ft.is_file() && laika_raw::media_kind(&p).is_some() {
                out.push(p);
            }
        }
    }
    out
}

/// SHA-256 of bytes, lowercase hex.
pub fn sha256_bytes(bytes: &[u8]) -> String {
    sha2_hw::Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// SHA-256 of a file, lowercase hex.
pub fn sha256_file(path: &Path) -> Result<String, String> {
    let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut h = sha2_hw::Sha256::new();
    let mut buf = vec![0u8; 1 << 20];
    loop {
        let n = f.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.finalize().iter().map(|b| format!("{b:02x}")).collect())
}

/// Progress while matching originals: files checked, files to check,
/// originals found.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MatchProgress {
    pub checked: usize,
    pub total: usize,
    pub found: usize,
}

/// Find originals among candidate files by size, then SHA-256. Returns
/// digest → file. Only candidates whose size matches a wanted original
/// are read, and once every original of a size is found, later
/// candidates of that size are skipped.
pub fn find_originals(
    wanted: &[(String, u64)],
    candidates: Vec<PathBuf>,
    workers: usize,
    cancel: &AtomicBool,
    progress: &(dyn Fn(MatchProgress) + Sync),
) -> HashMap<String, PathBuf> {
    let mut by_size: HashMap<u64, HashSet<String>> = HashMap::new();
    for (sha, size) in wanted {
        if *size > 0 && is_hex_digest(sha) {
            by_size.entry(*size).or_default().insert(sha.clone());
        }
    }
    let mut seen = HashSet::new();
    let queue: Vec<(PathBuf, u64)> = candidates
        .into_iter()
        .filter(|p| seen.insert(p.clone()))
        .filter_map(|p| {
            let size = std::fs::metadata(&p).ok()?.len();
            by_size.contains_key(&size).then_some((p, size))
        })
        .collect();
    let total = queue.len();
    let remaining = Mutex::new(by_size);
    let found: Mutex<HashMap<String, PathBuf>> = Mutex::new(HashMap::new());
    let next = AtomicUsize::new(0);
    let checked = AtomicUsize::new(0);
    progress(MatchProgress {
        checked: 0,
        total,
        found: 0,
    });
    std::thread::scope(|scope| {
        for _ in 0..workers.clamp(1, 16) {
            scope.spawn(|| {
                loop {
                    if cancel.load(Ordering::Relaxed) {
                        return;
                    }
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some((path, size)) = queue.get(i) else {
                        return;
                    };
                    let open = remaining
                        .lock()
                        .map(|r| r.get(size).is_some_and(|s| !s.is_empty()))
                        .unwrap_or(false);
                    if open {
                        if let Ok(sha) = sha256_file(path) {
                            let hit = remaining
                                .lock()
                                .map(|mut r| r.get_mut(size).is_some_and(|s| s.remove(&sha)))
                                .unwrap_or(false);
                            if hit {
                                if let Ok(mut f) = found.lock() {
                                    f.insert(sha, path.clone());
                                }
                            }
                        }
                    }
                    let done = checked.fetch_add(1, Ordering::Relaxed) + 1;
                    if done % 16 == 0 || done == total {
                        let n = found.lock().map(|f| f.len()).unwrap_or(0);
                        progress(MatchProgress {
                            checked: done,
                            total,
                            found: n,
                        });
                    }
                }
            });
        }
    });
    found.into_inner().unwrap_or_default()
}

/// Camera Raw settings Laika does not render, named for the report. Only
/// settings that differ from Adobe's neutral values are listed.
pub fn unsupported_settings(packet: &[u8]) -> Vec<String> {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;
    let mut out: Vec<String> = Vec::new();
    let mut push = |s: String| {
        if !out.contains(&s) {
            out.push(s);
        }
    };
    let mut r = Reader::from_reader(packet);
    let mut depth = 0usize;
    let mut element: Vec<String> = Vec::new();
    let mut curve: Option<(String, bool)> = None;
    let mut text = String::new();
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                if name == "rdf:Description" {
                    depth += 1;
                }
                let parent = element.last().map(|s| s.as_str());
                if depth == 1 || parent == Some("crs:Look") {
                    attrs(&e, parent, &mut push);
                }
                // Only the photo's own settings count: a profile's look
                // carries its built-in curve in a nested description.
                if let Some(key) = name.strip_prefix("crs:").filter(|_| depth == 1) {
                    match key {
                        "ToneCurvePV2012"
                        | "ToneCurvePV2012Red"
                        | "ToneCurvePV2012Green"
                        | "ToneCurvePV2012Blue" => curve = Some((key.to_string(), false)),
                        _ => {
                            if let Some(label) = element_label(key) {
                                push(label.to_string());
                            }
                        }
                    }
                }
                element.push(name);
                text.clear();
            }
            Ok(Event::Empty(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                if name == "rdf:Description" && depth == 0 {
                    attrs(&e, None, &mut push);
                } else if depth <= 1 {
                    attrs(&e, element.last().map(|s| s.as_str()), &mut push);
                }
            }
            Ok(Event::Text(t)) => text.push_str(&String::from_utf8_lossy(&t)),
            Ok(Event::End(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                if name == "rdf:li" {
                    if let Some((_, bent)) = curve.as_mut() {
                        let nums: Vec<f32> = text
                            .split(',')
                            .filter_map(|n| n.trim().parse().ok())
                            .collect();
                        if nums.len() == 2 && (nums[0] - nums[1]).abs() > 0.5 {
                            *bent = true;
                        }
                    }
                }
                if let Some(key) = name.strip_prefix("crs:").filter(|_| depth == 1) {
                    if let Some((k, bent)) = curve.take() {
                        if k == key && bent {
                            push(if key == "ToneCurvePV2012" {
                                "Point tone curve".to_string()
                            } else {
                                "RGB channel tone curves".to_string()
                            });
                        } else if k != key {
                            curve = Some((k, bent));
                        }
                    }
                }
                if name == "rdf:Description" {
                    depth = depth.saturating_sub(1);
                }
                element.pop();
                text.clear();
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => {}
        }
    }
    out
}

/// Top-level `crs:` attributes, or a `crs:Look` struct's name.
fn attrs(e: &quick_xml::events::BytesStart, parent: Option<&str>, push: &mut impl FnMut(String)) {
    let mut look_name = String::new();
    for a in e.attributes().flatten() {
        let key = String::from_utf8_lossy(a.key.as_ref()).into_owned();
        let val = a.unescape_value().unwrap_or_default().into_owned();
        if parent == Some("crs:Look") {
            if key == "crs:Name" {
                look_name = val;
            }
            continue;
        }
        let Some(k) = key.strip_prefix("crs:") else {
            continue;
        };
        if let Some(label) = attr_label(k, &val) {
            push(label);
        }
    }
    if !look_name.is_empty() && !is_default_profile(&look_name) {
        push(format!("Profile: {look_name}"));
    }
}

pub(crate) fn is_default_profile(name: &str) -> bool {
    matches!(
        name,
        "Adobe Standard"
            | "Adobe Color"
            | "Adobe Default"
            | "Embedded"
            | "Camera Standard"
            | "Default Profile"
            | "Default Color"
    )
}

/// Structures that carry local edits Laika can't reproduce yet.
pub(crate) fn element_label(key: &str) -> Option<&'static str> {
    Some(match key {
        "MaskGroupBasedCorrections"
        | "GradientBasedCorrections"
        | "CircularGradientBasedCorrections"
        | "PaintBasedCorrections" => "Masks and local adjustments",
        "RetouchAreas" | "RetouchInfo" => "Healing and spot removal",
        "RedEyeInfo" => "Red-eye removal",
        "PointColors" => return None,
        _ => return None,
    })
}

pub(crate) fn attr_label(key: &str, val: &str) -> Option<String> {
    // Laika renders these (or they carry no look at all).
    if crate::xmp::PARAM_KEYS.iter().any(|(k, _)| *k == key) {
        return None;
    }
    let num = val.trim_start_matches('+').parse::<f64>().ok();
    let off = |neutral: f64| num.is_some_and(|n| (n - neutral).abs() > 1e-6);
    let label = match key {
        "LensProfileEnable" => (val == "1").then(|| "Lens profile correction".to_string()),
        "CameraProfile" => {
            (!val.is_empty() && !is_default_profile(val)).then(|| format!("Camera profile: {val}"))
        }
        "ConvertToGrayscale" => {
            (val.eq_ignore_ascii_case("true")).then(|| "Black & white conversion".to_string())
        }
        "AutoLateralCA" => (val == "1").then(|| "Remove chromatic aberration".to_string()),
        "PerspectiveUpright" => off(0.).then(|| "Upright".to_string()),
        "HDREditMode" => off(0.).then(|| "HDR editing".to_string()),
        "EnableMaskGroupBasedCorrections" | "EnableRetouch" | "EnableRedEye" => None,
        "ParametricShadows" | "ParametricDarks" | "ParametricLights" | "ParametricHighlights" => {
            off(0.).then(|| "Parametric tone curve".to_string())
        }
        "SharpenDetail" => off(25.).then(|| "Sharpening detail".to_string()),
        "SharpenEdgeMasking" => off(0.).then(|| "Sharpening masking".to_string()),
        "LuminanceNoiseReductionDetail" => off(50.).then(|| "Noise reduction detail".to_string()),
        "LuminanceNoiseReductionContrast" => {
            off(0.).then(|| "Noise reduction contrast".to_string())
        }
        "ColorNoiseReductionDetail" | "ColorNoiseReductionSmoothness" => {
            off(50.).then(|| "Color noise reduction detail".to_string())
        }
        "DefringePurpleAmount" | "DefringeGreenAmount" => off(0.).then(|| "Defringe".to_string()),
        "VignetteAmount" => off(0.).then(|| "Lens vignetting".to_string()),
        "PostCropVignetteMidpoint" | "PostCropVignetteFeather" => {
            off(50.).then(|| "Vignette shape".to_string())
        }
        "PostCropVignetteRoundness" | "PostCropVignetteHighlightContrast" => {
            off(0.).then(|| "Vignette shape".to_string())
        }
        "GrainSize" => off(25.).then(|| "Grain size".to_string()),
        "GrainFrequency" => off(50.).then(|| "Grain roughness".to_string()),
        "ShadowTint" | "RedHue" | "RedSaturation" | "GreenHue" | "GreenSaturation" | "BlueHue"
        | "BlueSaturation" => off(0.).then(|| "Calibration".to_string()),
        "CurveRefineSaturation" => off(100.).then(|| "Curve saturation refine".to_string()),
        k if k.starts_with("GrayMixer") => off(0.).then(|| "Black & white mix".to_string()),
        "AutoTone" | "AutoExposure" | "AutoContrast" | "AutoBrightness" | "AutoShadows" => {
            (val.eq_ignore_ascii_case("true")).then(|| "Auto tone".to_string())
        }
        "WhiteBalance" => (val == "Auto").then(|| "Auto white balance".to_string()),
        "IncrementalTemperature" | "IncrementalTint" => {
            off(0.).then(|| "White balance for JPEGs (incremental)".to_string())
        }
        "LensProfileSetup" => (val == "Auto").then(|| "Lens profile correction".to_string()),
        "EnableCalibration"
        | "EnableColorAdjustments"
        | "EnableDetail"
        | "EnableEffects"
        | "EnableGrayscaleMix"
        | "EnableLensCorrections"
        | "EnableSplitToning"
        | "EnableToneCurve"
        | "EnableTransform"
        | "EnableCircularGradientBasedCorrections"
        | "EnableGradientBasedCorrections"
        | "EnablePaintBasedCorrections" => None,
        "Exposure" | "Contrast" | "Brightness" | "Shadows" | "FillLight" | "HighlightRecovery" => {
            off(0.).then(|| "Legacy process version settings".to_string())
        }
        _ => None,
    };
    label
}

#[cfg(test)]
mod tests {
    use super::*;

    const PACKET: &str = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
 <rdf:Description rdf:about="" xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
   crs:Version="17.1" crs:ProcessVersion="15.4" crs:WhiteBalance="As Shot"
   crs:Exposure2012="-0.59" crs:Contrast2012="+17" crs:LensProfileEnable="1"
   crs:CameraProfile="Adobe Color" crs:SharpenDetail="25" crs:ShadowTint="0"
   crs:CropTop="0.1" crs:CropLeft="0" crs:CropBottom="1" crs:CropRight="0.9" crs:CropAngle="0"
   crs:HasCrop="True" crs:HasSettings="True">
  <crs:ToneCurvePV2012><rdf:Seq><rdf:li>0, 0</rdf:li><rdf:li>255, 255</rdf:li></rdf:Seq></crs:ToneCurvePV2012>
  <crs:ToneCurvePV2012Red><rdf:Seq><rdf:li>0, 0</rdf:li><rdf:li>128, 150</rdf:li><rdf:li>255, 255</rdf:li></rdf:Seq></crs:ToneCurvePV2012Red>
  <crs:Look><rdf:Description crs:Name="Adobe Vivid" crs:Amount="1"><crs:Parameters><rdf:Description>
   <crs:ToneCurvePV2012><rdf:Seq><rdf:li>0, 0</rdf:li><rdf:li>22, 16</rdf:li><rdf:li>255, 255</rdf:li></rdf:Seq></crs:ToneCurvePV2012>
  </rdf:Description></crs:Parameters></rdf:Description></crs:Look>
  <crs:MaskGroupBasedCorrections><rdf:Seq><rdf:li>x</rdf:li></rdf:Seq></crs:MaskGroupBasedCorrections>
 </rdf:Description></rdf:RDF></x:xmpmeta>"#;

    #[test]
    fn unsupported_settings_name_only_what_differs() {
        let got = unsupported_settings(PACKET.as_bytes());
        assert!(
            got.contains(&"Lens profile correction".to_string()),
            "{got:?}"
        );
        assert!(
            got.contains(&"RGB channel tone curves".to_string()),
            "{got:?}"
        );
        assert!(got.contains(&"Profile: Adobe Vivid".to_string()), "{got:?}");
        assert!(
            got.contains(&"Masks and local adjustments".to_string()),
            "{got:?}"
        );
        // Neutral values and settings Laika renders are not reported.
        assert!(
            !got.iter().any(|g| g.contains("Point tone curve")),
            "{got:?}"
        );
        assert!(!got.iter().any(|g| g.contains("Camera profile")), "{got:?}");
        assert!(!got.iter().any(|g| g.contains("Sharpening")), "{got:?}");
        assert!(!got.iter().any(|g| g.contains("Calibration")), "{got:?}");
    }

    #[test]
    fn camera_raw_packets_parse_into_laika_settings() {
        let side = crate::xmp::parse(PACKET.as_bytes()).expect("parses");
        assert!(side.has_tone);
        let exposure = crate::xmp::PARAM_KEYS
            .iter()
            .find(|(k, _)| *k == "Exposure2012")
            .unwrap()
            .1;
        assert!((side.params[exposure] + 0.59).abs() < 1e-4);
        assert!(side.geom.is_some(), "crop carried");
    }

    #[test]
    fn titles_that_are_file_names_are_dropped() {
        assert!(looks_like_file_name("DSC_1380.NEF"));
        assert!(looks_like_file_name("1404.jpeg"));
        assert!(looks_like_file_name("Thortam view.jpeg"));
        assert!(!looks_like_file_name("Sunset over the bay"));
        assert!(!looks_like_file_name("Dr. Smith"));
        assert!(!looks_like_file_name("v2.0 final draft"));
    }

    #[test]
    fn standard_keyword_shapes_prefer_hierarchies_without_losing_flat_terms() {
        let doc = serde_json::json!({
            "xmp": {
                "dc": { "subject": ["Lisbon", "Night", "Harbor"] },
                "lr": { "hierarchicalSubject": {
                    "a": "Places|Portugal|Lisbon",
                    "b": "Time|Night"
                }}
            }
        });
        assert_eq!(
            keywords(&doc),
            vec!["Places > Portugal > Lisbon", "Time > Night", "Harbor"]
        );
    }

    #[test]
    fn originals_are_found_by_content_not_name() {
        let dir = std::env::temp_dir().join(format!("laika-lr-match-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("a/b")).unwrap();
        let wanted_bytes = b"original raw bytes".to_vec();
        std::fs::write(dir.join("a/b/renamed.nef"), &wanted_bytes).unwrap();
        // Same size, different content: never a match.
        std::fs::write(dir.join("a/decoy.nef"), b"0riginal raw bytes").unwrap();
        std::fs::write(dir.join("a/other.jpg"), b"x").unwrap();
        let sha = sha256_file(&dir.join("a/b/renamed.nef")).unwrap();
        let cancel = AtomicBool::new(false);
        let found = find_originals(
            &[(sha.clone(), wanted_bytes.len() as u64)],
            walk_media(&dir, false),
            2,
            &cancel,
            &|_| {},
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[&sha], dir.join("a/b/renamed.nef"));
        std::fs::remove_dir_all(&dir).ok();
    }
}
