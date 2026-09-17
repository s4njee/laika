//! Apple Photos sync (macOS): Photos becomes the home of the originals.
//!
//! Each photo goes to Photos through its AppleScript `import` command as a
//! normal, copied import (Photos' default "Copy items to the Photos
//! library"). Laika then finds Photos' copy inside the library package
//! (`originals/<first char>/<UUID>.<ext>`; a RAW+JPEG pair's RAW is stored
//! beside it as `<UUID>_4.<ext>` — matched by hash, never by name), checks
//! it is byte-identical to
//! the catalog's blake3, points the catalog row at it, and moves its own
//! file (and sidecar) to the Trash. Anything that can't be verified keeps
//! Laika's file, so a photo is never left without an original.
//!
//! Files inside a Photos library are read-only to Laika: no sidecars,
//! renames, moves or trashing (see `in_library`). Edits, ratings and
//! keywords stay in the catalog, and title, caption, keywords and favorite
//! are pushed to the media item whenever they change. Photos' scripting
//! cannot delete media items, so removing a photo from Laika leaves it in
//! Photos.

use crate::pairs;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Which catalog photos go to Photos. Rejected photos never do.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Scope {
    #[default]
    All,
    Picked,
    Rated,
}

/// What marks a media item as a Photos favorite.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum FavoriteRule {
    #[default]
    Picked,
    FiveStars,
    Off,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct PhotosSettings {
    /// Bring every photo in the Photos library into the catalog (in
    /// place, no copy).
    pub ingest_all: bool,
    /// Mirror Photos' folders and albums in the sidebar (and bring in
    /// their photos even when `ingest_all` is off).
    pub mirror_albums: bool,
    /// Push changes automatically while Laika is open.
    pub auto: bool,
    /// The user turned sync on (and agreed originals move into Photos).
    /// Nothing is sent to Photos before this is true.
    pub enabled: bool,
    pub scope: Scope,
    /// Minimum stars for `Scope::Rated` (1..=5).
    pub min_rating: u8,
    /// Album name, or the Photos folder name when `by_folder` is on.
    pub album: String,
    /// One album per catalog folder, grouped in a Photos folder.
    pub by_folder: bool,
    pub favorites: FavoriteRule,
    /// Title, caption and keywords.
    pub metadata: bool,
    pub videos: bool,
}

impl Default for PhotosSettings {
    fn default() -> Self {
        Self {
            ingest_all: true,
            mirror_albums: true,
            // Off until the first run is started by hand.
            auto: false,
            enabled: false,
            scope: Scope::All,
            min_rating: 3,
            album: "Laika".to_string(),
            by_folder: false,
            favorites: FavoriteRule::Picked,
            metadata: true,
            videos: true,
        }
    }
}

impl PhotosSettings {
    pub fn from_json(s: &str) -> Self {
        serde_json::from_str(s).unwrap_or_default()
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }

    pub fn album_name(&self) -> &str {
        match self.album.trim() {
            "" => "Laika",
            a => a,
        }
    }
}

/// One catalog photo as the planner sees it.
#[derive(Clone, Debug, Default)]
pub struct Candidate {
    pub id: i64,
    /// Absolute path.
    pub path: String,
    /// Catalog-relative folder ("" at the root).
    pub folder: String,
    pub is_raw: bool,
    pub is_video: bool,
    pub exists: bool,
    /// Catalog content hash (blake3 of the whole file).
    pub blake3: String,
    pub rating: u8,
    pub picked: bool,
    pub rejected: bool,
    pub title: String,
    pub caption: String,
    /// Keyword paths (`Places > Portugal > Lisbon`).
    pub keywords: Vec<String>,
}

/// A remembered photo → media item mapping.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Link {
    pub photo_id: i64,
    pub item_id: String,
    pub meta_hash: String,
    /// `folder/album` the item was placed in.
    pub album: String,
    /// The photo came from Photos (ingested), not sent there by Laika:
    /// never filed into Laika's albums.
    pub from_photos: bool,
}

/// Properties Laika sets on a media item. `None` leaves Photos' value.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Meta {
    pub name: Option<String>,
    pub description: Option<String>,
    pub keywords: Option<Vec<String>>,
    pub favorite: Option<bool>,
}

impl Meta {
    fn hash(&self) -> String {
        let text = format!(
            "{:?}\u{1f}{:?}\u{1f}{:?}\u{1f}{:?}",
            self.name, self.description, self.keywords, self.favorite
        );
        blake3::hash(text.as_bytes()).to_hex()[..16].to_string()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JobKind {
    Import,
    Update,
}

/// One unit of work: a single photo, or a RAW+JPEG pair that Photos
/// keeps as one media item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Job {
    pub kind: JobKind,
    pub photo_ids: Vec<i64>,
    /// Current path of each photo (same order as `photo_ids`).
    pub paths: Vec<String>,
    /// Catalog blake3 of each photo (same order).
    pub hashes: Vec<String>,
    /// Existing media items (updates only).
    pub item_ids: Vec<String>,
    /// Photos folder ("" = top level).
    pub folder: String,
    pub album: String,
    pub meta: Meta,
    pub meta_hash: String,
    /// Add the item to the album (new item, or album setting changed).
    pub place: bool,
}

impl Job {
    pub fn album_key(&self) -> String {
        album_key(&self.folder, &self.album)
    }

    /// Members still stored outside Photos (their originals move after
    /// this job).
    pub fn needs_move(&self) -> bool {
        self.paths.iter().any(|p| !in_library(p))
    }
}

/// True for files inside any Photos library package. Laika never writes,
/// renames, moves or trashes these.
pub fn in_library(path: &str) -> bool {
    path.contains(".photoslibrary/")
}

fn album_key(folder: &str, album: &str) -> String {
    if folder.is_empty() {
        album.to_string()
    } else {
        format!("{folder}/{album}")
    }
}

/// Everything that needs doing, in catalog order. `forced` photos are
/// synced regardless of scope (added by hand).
pub fn plan(
    candidates: &[Candidate],
    links: &[Link],
    forced: &HashSet<i64>,
    settings: &PhotosSettings,
) -> Vec<Job> {
    let by_id: HashMap<i64, &Candidate> = candidates.iter().map(|c| (c.id, c)).collect();
    let links: HashMap<i64, &Link> = links.iter().map(|l| (l.photo_id, l)).collect();
    let rows: Vec<(i64, String, bool)> = candidates
        .iter()
        .map(|c| (c.id, c.path.clone(), c.is_raw))
        .collect();
    let mut group_of: HashMap<i64, Vec<i64>> = HashMap::new();
    for p in pairs::find_pairs(&rows) {
        group_of.insert(p.raw_id, vec![p.raw_id, p.jpeg_id]);
        group_of.insert(p.jpeg_id, vec![p.raw_id, p.jpeg_id]);
    }

    let mut seen = HashSet::new();
    let mut jobs = Vec::new();
    for c in candidates {
        if !seen.insert(c.id) {
            continue;
        }
        let ids = group_of.get(&c.id).cloned().unwrap_or_else(|| vec![c.id]);
        seen.extend(ids.iter().copied());
        let members: Vec<&Candidate> = ids.iter().filter_map(|i| by_id.get(i).copied()).collect();
        let wanted = members
            .iter()
            .any(|m| forced.contains(&m.id) || in_scope(m, settings));
        if !wanted {
            continue;
        }

        let linked_album = members
            .iter()
            .filter_map(|m| links.get(&m.id))
            .map(|l| l.album.clone())
            .next();
        let all_in_library = members.iter().all(|m| in_library(&m.path));
        let (folder, album) =
            if let (true, true, Some(k)) = (settings.by_folder, all_in_library, linked_album) {
                // Moved originals no longer have a catalog folder; they stay
                // in the album they were filed into.
                match k.split_once('/') {
                    Some((f, a)) => (f.to_string(), a.to_string()),
                    None => (String::new(), k),
                }
            } else if settings.by_folder {
                let f = c.folder.trim_matches('/');
                (
                    settings.album_name().to_string(),
                    if f.is_empty() {
                        "Catalog root".to_string()
                    } else {
                        f.to_string()
                    },
                )
            } else {
                (String::new(), settings.album_name().to_string())
            };
        let meta = group_meta(&members, settings);
        let meta_hash = meta.hash();
        let key = album_key(&folder, &album);

        let mut item_ids: Vec<String> = members
            .iter()
            .filter_map(|m| links.get(&m.id))
            .map(|l| l.item_id.clone())
            .filter(|i| !i.is_empty())
            .collect();
        item_ids.dedup();

        let photo_ids: Vec<i64> = members.iter().map(|m| m.id).collect();
        let paths: Vec<String> = members.iter().map(|m| m.path.clone()).collect();
        let hashes: Vec<String> = members.iter().map(|m| m.blake3.clone()).collect();

        if item_ids.is_empty() {
            // New to Photos: every file on disk and none already inside a
            // library (an unlinked library file was deleted in Photos —
            // importing it into itself would be wrong).
            if members.iter().any(|m| !m.exists || in_library(&m.path)) {
                continue;
            }
            jobs.push(Job {
                kind: JobKind::Import,
                photo_ids,
                paths,
                hashes,
                item_ids,
                folder,
                album,
                meta,
                meta_hash,
                place: true,
            });
            continue;
        }

        let fully_linked = members.iter().all(|m| links.contains_key(&m.id));
        let stale = members
            .iter()
            .filter_map(|m| links.get(&m.id))
            .any(|l| l.meta_hash != meta_hash);
        let moved = members
            .iter()
            .filter_map(|m| links.get(&m.id))
            .any(|l| !l.from_photos && l.album != key);
        // Linked but still outside Photos: a previous move didn't verify.
        let unmoved = members.iter().any(|m| m.exists && !in_library(&m.path));
        if fully_linked && !stale && !moved && !unmoved {
            continue;
        }
        jobs.push(Job {
            kind: JobKind::Update,
            photo_ids,
            paths,
            hashes,
            item_ids,
            folder,
            album,
            meta,
            meta_hash,
            place: moved,
        });
    }
    jobs
}

fn in_scope(c: &Candidate, s: &PhotosSettings) -> bool {
    if c.rejected || (c.is_video && !s.videos) {
        return false;
    }
    match s.scope {
        Scope::All => true,
        Scope::Picked => c.picked,
        Scope::Rated => c.rating >= s.min_rating.clamp(1, 5),
    }
}

/// The planner's metadata hash for every photo (pairs share one), so
/// freshly ingested links start in sync instead of pushing back to Photos.
pub fn meta_hashes(candidates: &[Candidate], settings: &PhotosSettings) -> HashMap<i64, String> {
    let by_id: HashMap<i64, &Candidate> = candidates.iter().map(|c| (c.id, c)).collect();
    let rows: Vec<(i64, String, bool)> = candidates
        .iter()
        .map(|c| (c.id, c.path.clone(), c.is_raw))
        .collect();
    let mut out = HashMap::new();
    for p in pairs::find_pairs(&rows) {
        let members: Vec<&Candidate> = [p.raw_id, p.jpeg_id]
            .iter()
            .filter_map(|i| by_id.get(i).copied())
            .collect();
        let h = group_meta(&members, settings).hash();
        out.insert(p.raw_id, h.clone());
        out.insert(p.jpeg_id, h);
    }
    for c in candidates {
        out.entry(c.id)
            .or_insert_with(|| group_meta(&[c], settings).hash());
    }
    out
}

fn group_meta(members: &[&Candidate], s: &PhotosSettings) -> Meta {
    let first = |f: fn(&Candidate) -> &str| {
        members
            .iter()
            .map(|m| f(m).trim())
            .find(|v| !v.is_empty())
            .unwrap_or("")
            .to_string()
    };
    let favorite = match s.favorites {
        FavoriteRule::Picked => Some(members.iter().any(|m| m.picked)),
        FavoriteRule::FiveStars => Some(members.iter().any(|m| m.rating >= 5)),
        FavoriteRule::Off => None,
    };
    if !s.metadata {
        return Meta {
            favorite,
            ..Meta::default()
        };
    }
    // Photos keywords are flat: send leaf names.
    let mut keywords: Vec<String> = members
        .iter()
        .flat_map(|m| m.keywords.iter())
        .map(|k| k.rsplit('>').next().unwrap_or(k).trim().to_string())
        .filter(|k| !k.is_empty())
        .collect();
    keywords.sort();
    keywords.dedup();
    Meta {
        name: Some(first(|m| &m.title)),
        description: Some(first(|m| &m.caption)),
        keywords: Some(keywords),
        favorite,
    }
}

// ---- AppleScript bridge ------------------------------------------------------

/// Outcome of one job, keyed by its index in the batch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    /// Media item ids now holding the photo(s).
    Done(Vec<String>),
    /// Done, plus where each photo's original lives in Photos (same order
    /// as `photo_ids`; `None` = keep Laika's file). `copied` is false when
    /// Photos stored no original at all — its copy setting is off.
    Placed {
        ids: Vec<String>,
        originals: Vec<Option<String>>,
        copied: bool,
    },
    /// The linked media item no longer exists (deleted in Photos).
    Missing,
    Failed(String),
}

const SEP: char = '\u{1f}';

fn clean(s: &str) -> String {
    s.chars()
        .map(|c| match c {
            '\t' | '\n' | '\r' | SEP => ' ',
            c => c,
        })
        .collect()
}

fn join(items: &[String]) -> String {
    items
        .iter()
        .map(|s| clean(s))
        .collect::<Vec<_>>()
        .join(&SEP.to_string())
}

/// One tab-separated manifest line per job (fields documented in
/// `SCRIPT`'s `runJob`).
pub fn manifest(jobs: &[Job]) -> String {
    let flag = |b: bool| if b { "1" } else { "0" };
    let mut out = String::new();
    for (i, j) in jobs.iter().enumerate() {
        let fields = [
            match j.kind {
                JobKind::Import => "I".to_string(),
                JobKind::Update => "U".to_string(),
            },
            i.to_string(),
            clean(&j.folder),
            clean(&j.album),
            join(&j.item_ids),
            join(&j.paths),
            flag(j.meta.name.is_some()).to_string(),
            clean(j.meta.name.as_deref().unwrap_or("")),
            flag(j.meta.description.is_some()).to_string(),
            clean(j.meta.description.as_deref().unwrap_or("")),
            flag(j.meta.keywords.is_some()).to_string(),
            join(j.meta.keywords.as_deref().unwrap_or(&[])),
            match j.meta.favorite {
                Some(true) => "1".to_string(),
                Some(false) => "0".to_string(),
                None => String::new(),
            },
            flag(j.place).to_string(),
        ];
        out.push_str(&fields.join("\t"));
        out.push('\n');
    }
    out
}

/// Parse the script's `index \t status \t ids \t message` lines.
pub fn parse_output(out: &str, count: usize) -> Vec<Outcome> {
    let mut results = vec![Outcome::Failed("no result from Photos".to_string()); count];
    for line in out.lines() {
        let f: Vec<&str> = line.splitn(4, '\t').collect();
        let Some(i) = f.first().and_then(|s| s.trim().parse::<usize>().ok()) else {
            continue;
        };
        if i >= count {
            continue;
        }
        let ids = || -> Vec<String> {
            f.get(2)
                .map(|s| {
                    s.split(SEP)
                        .map(|x| x.trim().to_string())
                        .filter(|x| !x.is_empty())
                        .collect()
                })
                .unwrap_or_default()
        };
        results[i] = match f.get(1).map(|s| s.trim()) {
            Some("ok") if !ids().is_empty() => Outcome::Done(ids()),
            Some("missing") => Outcome::Missing,
            _ => Outcome::Failed(f.get(3).map(|s| s.trim()).unwrap_or("").to_string()),
        };
    }
    results
}

/// Turn osascript failures into something a person can act on.
pub fn friendly_error(stderr: &str) -> String {
    let e = stderr.trim();
    if e.contains("-1743") {
        "Laika isn't allowed to control Photos. Allow it in System Settings › Privacy & \
         Security › Automation, then sync again."
            .to_string()
    } else if e.contains("-600") || e.contains("-609") {
        "Photos quit or couldn't be opened — open Photos and sync again.".to_string()
    } else if e.contains("-1712") {
        "Photos took too long to respond — sync again to continue.".to_string()
    } else if e.is_empty() {
        "Photos reported an unknown error".to_string()
    } else {
        e.lines().last().unwrap_or(e).to_string()
    }
}

/// Map a per-job AppleScript error to a short message.
pub fn friendly_job_error(msg: &str) -> String {
    if msg.contains("-43") {
        "file not found".to_string()
    } else if msg.starts_with("9001") {
        "Photos didn't import it (unsupported file type?)".to_string()
    } else if msg.is_empty() {
        "Photos reported an error".to_string()
    } else {
        msg.to_string()
    }
}

/// Run a batch against Photos, then find and verify each moved photo's
/// original in the library. Blocking; call off the UI thread.
#[cfg(target_os = "macos")]
pub fn run_blocking(jobs: &[Job]) -> Result<Vec<Outcome>, String> {
    if jobs.is_empty() {
        return Ok(Vec::new());
    }
    // Pre-flight before anything is imported: the library must be found
    // and readable, or no original could ever be verified.
    let library = library_path()?;
    check_library(&library)?;

    let dir = std::env::temp_dir().join(format!("laika-photos-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let script = dir.join("sync.applescript");
    let list = dir.join("manifest.tsv");
    std::fs::write(&script, SCRIPT).map_err(|e| e.to_string())?;
    std::fs::write(&list, manifest(jobs)).map_err(|e| e.to_string())?;
    let out = std::process::Command::new("/usr/bin/osascript")
        .arg(&script)
        .arg(&list)
        .output()
        .map_err(|e| format!("couldn't run osascript: {e}"))?;
    std::fs::remove_file(&list).ok();
    if !out.status.success() {
        return Err(friendly_error(&String::from_utf8_lossy(&out.stderr)));
    }
    let outcomes = parse_output(&String::from_utf8_lossy(&out.stdout), jobs.len());
    Ok(jobs
        .iter()
        .zip(outcomes)
        .map(|(job, o)| match o {
            Outcome::Failed(m) => Outcome::Failed(friendly_job_error(&m)),
            Outcome::Done(ids) if job.needs_move() => {
                let (originals, copied) =
                    locate_originals(&library, job, &ids, std::time::Duration::from_secs(10));
                Outcome::Placed {
                    ids,
                    originals,
                    copied,
                }
            }
            o => o,
        })
        .collect())
}

/// The library Photos has open, from the bookmark in its preferences;
/// falls back to the default location.
#[cfg(target_os = "macos")]
pub fn library_path() -> Result<std::path::PathBuf, String> {
    if let Some(p) = resolve_library_bookmark() {
        return Ok(p);
    }
    let home = std::env::var("HOME").unwrap_or_default();
    let fallback = std::path::Path::new(&home).join("Pictures/Photos Library.photoslibrary");
    if fallback.is_dir() {
        return Ok(fallback);
    }
    Err("couldn't find your Photos library — open Photos once, then sync again".to_string())
}

#[cfg(target_os = "macos")]
fn resolve_library_bookmark() -> Option<std::path::PathBuf> {
    use std::process::{Command, Stdio};
    let prefs = Command::new("/usr/bin/defaults")
        .args(["export", "com.apple.Photos", "-"])
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let mut plutil = Command::new("/usr/bin/plutil")
        .args([
            "-extract",
            "IPXDefaultLibraryURLBookmark",
            "raw",
            "-o",
            "-",
            "-",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    {
        use std::io::Write;
        plutil.stdin.take()?.write_all(&prefs.stdout).ok()?;
    }
    let b64 = plutil.wait_with_output().ok()?;
    let b64 = String::from_utf8(b64.stdout).ok()?.trim().to_string();
    if b64.is_empty() {
        return None;
    }
    // Bookmark data resolves through Foundation; JXA is the shortest path.
    let out = Command::new("/usr/bin/osascript")
        .args(["-l", "JavaScript", "-e", RESOLVE_BOOKMARK, &b64])
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let path = String::from_utf8(out.stdout).ok()?.trim().to_string();
    let p = std::path::PathBuf::from(path);
    (p.extension().is_some_and(|e| e == "photoslibrary") && p.is_dir()).then_some(p)
}

#[cfg(target_os = "macos")]
const RESOLVE_BOOKMARK: &str = r#"ObjC.import("Foundation");
function run(argv) {
  var d = $.NSData.alloc.initWithBase64EncodedStringOptions(argv[0], 0);
  var u = $.NSURL.URLByResolvingBookmarkDataOptionsRelativeToURLBookmarkDataIsStaleError(d, 256 | 512, $(), null, null);
  return u.isNil() ? "" : ObjC.unwrap(u.path);
}"#;

/// The library's originals must be listable (macOS protects the package;
/// Laika needs Full Disk Access to read it).
pub fn check_library(library: &std::path::Path) -> Result<(), String> {
    match std::fs::read_dir(library.join("originals")) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => Err(
            "Laika can't read your Photos library. Turn on Laika in System Settings › Privacy \
             & Security › Full Disk Access, then sync again."
                .to_string(),
        ),
        Err(e) => Err(format!(
            "couldn't open {}: {e}",
            library.join("originals").display()
        )),
    }
}

/// Cheap change marker for a Photos library: modification times of its
/// database files. Unchanged since the last read means nothing to bring in.
pub fn library_stamp(library: &std::path::Path) -> String {
    let db = library.join("database");
    ["Photos.sqlite", "Photos.sqlite-wal"]
        .iter()
        .map(|f| {
            std::fs::metadata(db.join(f))
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_millis().to_string())
                .unwrap_or_default()
        })
        .collect::<Vec<_>>()
        .join(":")
}

/// Media item id (`UUID/L0/001`) → asset UUID.
fn asset_uuid(item_id: &str) -> &str {
    item_id.split('/').next().unwrap_or(item_id)
}

/// Find each job member's original among the files Photos stored for the
/// job's media items, matching by content hash. Photos can finish writing
/// shortly after `import` returns, so this waits up to `max_wait` for them.
pub fn locate_originals(
    library: &std::path::Path,
    job: &Job,
    item_ids: &[String],
    max_wait: std::time::Duration,
) -> (Vec<Option<String>>, bool) {
    let started = std::time::Instant::now();
    let mut files: Vec<std::path::PathBuf> = Vec::new();
    loop {
        files.clear();
        for id in item_ids {
            let uuid = asset_uuid(id);
            let Some(first) = uuid.chars().next() else {
                continue;
            };
            let dir = library
                .join("originals")
                .join(first.to_ascii_uppercase().to_string());
            if let Ok(entries) = std::fs::read_dir(&dir) {
                for e in entries.flatten() {
                    let name = e.file_name().to_string_lossy().to_string();
                    if name.starts_with(uuid) && !name.ends_with(".aae") {
                        files.push(e.path());
                    }
                }
            }
        }
        // Enough files for every member that still needs a home, or a
        // few seconds have passed.
        let wanted = job.paths.iter().filter(|p| !in_library(p)).count();
        let waited = started.elapsed();
        if files.len() >= wanted
            || (waited >= max_wait / 4 && !files.is_empty())
            || waited >= max_wait
        {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
    let copied = !files.is_empty();
    let hashes: Vec<(std::path::PathBuf, String)> = files
        .into_iter()
        .filter_map(|f| crate::catalog::hash_file(&f).ok().map(|h| (f, h)))
        .collect();
    let originals = job
        .paths
        .iter()
        .zip(&job.hashes)
        .map(|(path, hash)| {
            if in_library(path) || hash.is_empty() {
                return None;
            }
            hashes
                .iter()
                .find(|(_, h)| h == hash)
                .map(|(f, _)| f.to_string_lossy().to_string())
        })
        .collect();
    (originals, copied)
}

#[cfg(not(target_os = "macos"))]
pub fn run_blocking(_jobs: &[Job]) -> Result<Vec<Outcome>, String> {
    Err("Apple Photos sync is only available on macOS".to_string())
}

/// Reads the manifest (path in argv), runs each job inside one Photos
/// session, and prints one result line per job. Per-job errors are
/// reported and skipped; an authorization failure (-1743) aborts the run.
pub const SCRIPT: &str = r#"
on run argv
	set manifestText to read (POSIX file (item 1 of argv)) as «class utf8»
	set sep to character id 31
	set results to {}
	repeat with ln in (paragraphs of manifestText)
		set ln to ln as text
		if ln is not "" then
			set f to my splitText(ln, tab)
			set jobIndex to item 2 of f
			try
				set r to my runJob(f, sep)
				set end of results to jobIndex & tab & r & tab
			on error errMsg number errNum
				if errNum is -1743 then error errMsg number errNum
				set end of results to jobIndex & tab & "error" & tab & tab & errNum & " " & my cleanText(errMsg)
			end try
		end if
	end repeat
	return my joinText(results, linefeed)
end run

-- Fields: 1 op (I/U), 2 index, 3 folder, 4 album, 5 item ids, 6 paths,
-- 7/8 set name + name, 9/10 set description + description,
-- 11/12 set keywords + keywords, 13 favorite ("", "1", "0"), 14 place.
on runJob(f, sep)
	set op to item 1 of f
	set theAlbum to missing value
	if op is "I" or (item 14 of f) is "1" then set theAlbum to my ensureAlbum(item 4 of f, item 3 of f)
	if op is "I" then
		set fileList to my fileRefs(my splitText(item 6 of f, sep))
		tell application "Photos"
			with timeout of 3600 seconds
				set mediaItems to import fileList into theAlbum skip check duplicates true
			end timeout
		end tell
		if mediaItems is missing value then error "Photos imported nothing" number 9001
		if (count of mediaItems) is 0 then error "Photos imported nothing" number 9001
	else
		set mediaItems to {}
		tell application "Photos"
			repeat with theId in my splitText(item 5 of f, sep)
				set theId to theId as text
				if theId is not "" then
					if not (exists media item id theId) then return "missing" & tab
					set end of mediaItems to media item id theId
				end if
			end repeat
			if (item 14 of f) is "1" then add mediaItems to theAlbum
		end tell
	end if
	set ids to {}
	tell application "Photos"
		repeat with mi in mediaItems
			set mi to contents of mi
			if (item 7 of f) is "1" then set name of mi to (item 8 of f)
			if (item 9 of f) is "1" then set description of mi to (item 10 of f)
			if (item 11 of f) is "1" then
				set kws to my splitText(item 12 of f, sep)
				if kws is {""} then set kws to {}
				try
					set keywords of mi to kws
				on error
					set keywords of mi to missing value
				end try
			end if
			if (item 13 of f) is "1" then set favorite of mi to true
			if (item 13 of f) is "0" then set favorite of mi to false
			set end of ids to (id of mi)
		end repeat
	end tell
	return "ok" & tab & my joinText(ids, sep)
end runJob

on ensureAlbum(albumName, folderName)
	tell application "Photos"
		if folderName is "" then
			repeat with a in (every album whose name is albumName)
				set a to contents of a
				if my isTopLevel(a) then return a
			end repeat
			return make new album named albumName
		end if
		set theFolder to missing value
		repeat with d in (every folder whose name is folderName)
			set d to contents of d
			if my isTopLevel(d) then set theFolder to d
		end repeat
		if theFolder is missing value then set theFolder to make new folder named folderName
		repeat with a in (every album of theFolder whose name is albumName)
			return contents of a
		end repeat
		return make new album named albumName at theFolder
	end tell
end ensureAlbum

on isTopLevel(c)
	tell application "Photos"
		try
			set p to parent of c
		on error
			return true
		end try
		return p is missing value
	end tell
end isTopLevel

on fileRefs(pathList)
	set refs to {}
	repeat with p in pathList
		set end of refs to (POSIX file (p as text)) as alias
	end repeat
	return refs
end fileRefs

on splitText(t, d)
	set saved to AppleScript's text item delimiters
	set AppleScript's text item delimiters to d
	set parts to text items of t
	set AppleScript's text item delimiters to saved
	return parts
end splitText

on joinText(parts, d)
	set saved to AppleScript's text item delimiters
	set AppleScript's text item delimiters to d
	set t to parts as text
	set AppleScript's text item delimiters to saved
	return t
end joinText

on cleanText(t)
	return my joinText(my splitText(t as text, {tab, linefeed, return}), " ")
end cleanText
"#;

// ---- Photos → Laika: library contents and albums ---------------------------

/// One media item as Photos reports it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LibraryItem {
    pub id: String,
    pub filename: String,
    pub name: String,
    pub description: String,
    pub favorite: bool,
    pub keywords: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContainerKind {
    Folder,
    Album,
}

/// A folder or album, in Photos' order (parents precede children).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Container {
    pub id: String,
    /// Parent folder id ("" = top level).
    pub parent: String,
    pub name: String,
    pub kind: ContainerKind,
    /// Media item ids (albums only), in album order.
    pub items: Vec<String>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct LibraryDump {
    pub items: Vec<LibraryItem>,
    pub containers: Vec<Container>,
}

/// Parse `DUMP_SCRIPT` output.
pub fn parse_dump(out: &str) -> LibraryDump {
    let list = |s: Option<&&str>| -> Vec<String> {
        s.map(|s| {
            s.split(SEP)
                .map(|x| x.trim().to_string())
                .filter(|x| !x.is_empty())
                .collect()
        })
        .unwrap_or_default()
    };
    let text = |s: Option<&&str>| s.map(|s| s.to_string()).unwrap_or_default();
    let mut dump = LibraryDump::default();
    for line in out.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        match f.first().copied() {
            Some("M") if f.len() >= 2 && !f[1].is_empty() => dump.items.push(LibraryItem {
                id: f[1].to_string(),
                filename: text(f.get(2)),
                name: text(f.get(3)),
                description: text(f.get(4)),
                favorite: f.get(5) == Some(&"1"),
                keywords: list(f.get(6)),
            }),
            Some(k @ ("F" | "A")) if f.len() >= 4 && !f[1].is_empty() => {
                dump.containers.push(Container {
                    id: f[1].to_string(),
                    parent: f[2].to_string(),
                    name: f[3].to_string(),
                    kind: if k == "F" {
                        ContainerKind::Folder
                    } else {
                        ContainerKind::Album
                    },
                    items: list(f.get(4)),
                })
            }
            _ => {}
        }
    }
    dump
}

/// Originals Photos stores for each asset UUID: the primary file
/// (`<UUID>.<ext>`) and a RAW+JPEG pair's RAW (`<UUID>_4.<ext>`). Live
/// Photo movies (`_3`) and adjustment data (`.aae`) are not photos Laika
/// catalogs.
pub fn index_originals(library: &std::path::Path) -> HashMap<String, Vec<std::path::PathBuf>> {
    let mut index: HashMap<String, Vec<std::path::PathBuf>> = HashMap::new();
    let Ok(dirs) = std::fs::read_dir(library.join("originals")) else {
        return index;
    };
    for dir in dirs.flatten() {
        let Ok(files) = std::fs::read_dir(dir.path()) else {
            continue;
        };
        for f in files.flatten() {
            let name = f.file_name().to_string_lossy().to_string();
            let Some((stem, ext)) = name.rsplit_once('.') else {
                continue;
            };
            if ext.eq_ignore_ascii_case("aae") {
                continue;
            }
            let (uuid, suffix) = match stem.split_once('_') {
                Some((u, s)) => (u, Some(s)),
                None => (stem, None),
            };
            let primary = suffix.is_none();
            let pair_raw = suffix == Some("4")
                && laika_raw::RAW_EXTS.contains(&ext.to_ascii_lowercase().as_str());
            if primary || pair_raw {
                let entry = index.entry(uuid.to_string()).or_default();
                // Primary first so it maps as the item's main file.
                if primary {
                    entry.insert(0, f.path());
                } else {
                    entry.push(f.path());
                }
            }
        }
    }
    index
}

/// One library file to catalog, with the media item it belongs to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IngestFile {
    pub path: String,
    pub item_id: String,
    /// Already in the catalog (only needs linking).
    pub known: bool,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IngestPlan {
    pub files: Vec<IngestFile>,
    /// Items already linked to catalog photos (nothing to do).
    pub present: usize,
    /// Originals not on this Mac (iCloud "Optimize Mac Storage").
    pub not_local: usize,
    /// Files Laika can't open (unsupported formats).
    pub unsupported: usize,
}

/// Which library files still need cataloging. With `all` off, only
/// photos that are in some album are brought in.
pub fn plan_ingest(
    dump: &LibraryDump,
    originals: &HashMap<String, Vec<std::path::PathBuf>>,
    linked_items: &HashSet<String>,
    known_paths: &HashSet<String>,
    all: bool,
) -> IngestPlan {
    let in_albums: HashSet<&str> = dump
        .containers
        .iter()
        .flat_map(|c| c.items.iter().map(|i| i.as_str()))
        .collect();
    let mut plan = IngestPlan::default();
    for item in &dump.items {
        if !all && !in_albums.contains(item.id.as_str()) {
            continue;
        }
        if linked_items.contains(&item.id) {
            plan.present += 1;
            continue;
        }
        let Some(files) = originals.get(asset_uuid(&item.id)) else {
            plan.not_local += 1;
            continue;
        };
        let mut any = false;
        for f in files {
            let path = f.to_string_lossy().to_string();
            if laika_raw::media_kind(f).is_none() {
                continue;
            }
            any = true;
            plan.files.push(IngestFile {
                known: known_paths.contains(&path),
                path,
                item_id: item.id.clone(),
            });
        }
        if !any {
            plan.unsupported += 1;
        }
    }
    plan
}

/// Above this many new items, metadata is fetched in bulk for the whole
/// library (≈1 min on 10k items) instead of item by item.
pub const BULK_META_THRESHOLD: usize = 300;

#[cfg(target_os = "macos")]
fn run_script(name: &str, text: &str, args: &[&str]) -> Result<String, String> {
    let dir = std::env::temp_dir().join(format!("laika-photos-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let script = dir.join(format!("{name}.applescript"));
    std::fs::write(&script, text).map_err(|e| e.to_string())?;
    let out = std::process::Command::new("/usr/bin/osascript")
        .arg(&script)
        .args(args)
        .output()
        .map_err(|e| format!("couldn't run osascript: {e}"))?;
    if !out.status.success() {
        return Err(friendly_error(&String::from_utf8_lossy(&out.stderr)));
    }
    Ok(String::from_utf8_lossy(&out.stdout).to_string())
}

/// Read every media item and the folder/album tree. With `metadata` off
/// only ids come back (seconds instead of a minute). Blocking.
#[cfg(target_os = "macos")]
pub fn dump_library_blocking(metadata: bool) -> Result<LibraryDump, String> {
    let mode = if metadata { "meta" } else { "ids" };
    run_script("dump", DUMP_SCRIPT, &[mode]).map(|o| parse_dump(&o))
}

/// Title, description, keywords and favorite for specific items. Blocking.
#[cfg(target_os = "macos")]
pub fn item_metadata_blocking(ids: &[String]) -> Result<Vec<LibraryItem>, String> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    if ids.len() > BULK_META_THRESHOLD {
        let wanted: HashSet<&String> = ids.iter().collect();
        return dump_library_blocking(true).map(|d| {
            d.items
                .into_iter()
                .filter(|i| wanted.contains(&i.id))
                .collect()
        });
    }
    let dir = std::env::temp_dir().join(format!("laika-photos-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let list = dir.join("items.txt");
    std::fs::write(&list, ids.join("\n")).map_err(|e| e.to_string())?;
    let out = run_script("meta", META_SCRIPT, &[&list.to_string_lossy()]);
    std::fs::remove_file(&list).ok();
    out.map(|o| parse_dump(&o).items)
}

#[cfg(not(target_os = "macos"))]
pub fn dump_library_blocking(_metadata: bool) -> Result<LibraryDump, String> {
    Err("Apple Photos sync is only available on macOS".to_string())
}

#[cfg(not(target_os = "macos"))]
pub fn item_metadata_blocking(_ids: &[String]) -> Result<Vec<LibraryItem>, String> {
    Err("Apple Photos sync is only available on macOS".to_string())
}

/// `M` lines for the item ids listed in the file at argv 1; items that no
/// longer exist are skipped.
pub const META_SCRIPT: &str = r#"
property pIds : {}
property outLines : {}

on run argv
	set outLines to {}
	set sep to character id 31
	set pIds to paragraphs of (read (POSIX file (item 1 of argv)) as «class utf8»)
	tell application "Photos"
		with timeout of 3600 seconds
			repeat with i from 1 to count of my pIds
				set theId to item i of my pIds
				if theId is not "" and (exists media item id theId) then
					set mi to media item id theId
					set fav to "0"
					if favorite of mi is true then set fav to "1"
					set end of my outLines to "M" & tab & theId & tab & my clean(filename of mi) & tab & my clean(name of mi) & tab & my clean(description of mi) & tab & fav & tab & my joinList(keywords of mi, sep)
				end if
			end repeat
		end timeout
	end tell
	return my joinText(my outLines, linefeed)
end run

on clean(v)
	if v is missing value then return ""
	set t to v as text
	if t contains tab or t contains linefeed or t contains return or t contains (character id 31) then
		return my joinText(my splitText(t, {tab, linefeed, return, character id 31}), " ")
	end if
	return t
end clean

on joinList(v, sep)
	if v is missing value then return ""
	if (count of v) is 0 then return ""
	set parts to {}
	repeat with x in v
		set end of parts to my clean(contents of x)
	end repeat
	return my joinText(parts, sep)
end joinList

on splitText(t, d)
	set saved to AppleScript's text item delimiters
	set AppleScript's text item delimiters to d
	set parts to text items of t
	set AppleScript's text item delimiters to saved
	return parts
end splitText

on joinText(parts, d)
	set saved to AppleScript's text item delimiters
	set AppleScript's text item delimiters to d
	set t to parts as text
	set AppleScript's text item delimiters to saved
	return t
end joinText
"#;

/// Prints `M` lines (id, filename, title, description, favorite, keywords)
/// for every media item, then `F`/`A` lines (id, parent id, name, album
/// item ids) walking folders depth-first. Properties are fetched in bulk:
/// one Apple event per property, not per item.
pub const DUMP_SCRIPT: &str = r#"
-- Bulk results live in properties: `item i of my pList` is constant time,
-- while indexing a large local list is quadratic in AppleScript.
property outLines : {}
property pIds : {}
property pFns : {}
property pNames : {}
property pDescs : {}
property pFavs : {}
property pKws : {}

on run argv
	set outLines to {}
	set sep to character id 31
	set wantMeta to true
	if (count of argv) > 0 then set wantMeta to ((item 1 of argv) is not "ids")
	tell application "Photos"
		with timeout of 3600 seconds
			set pIds to id of every media item
			if wantMeta then
				set pFns to filename of every media item
				set pNames to name of every media item
				set pDescs to description of every media item
				set pFavs to favorite of every media item
				set pKws to keywords of every media item
			end if
			set favId to ""
			try
				set favId to id of favorites album
			end try
			set tops to containers
		end timeout
	end tell
	set n to count of my pIds
	repeat with i from 1 to n
		if wantMeta then
			set fav to "0"
			if (item i of my pFavs) is true then set fav to "1"
			set end of my outLines to "M" & tab & (item i of my pIds) & tab & my clean(item i of my pFns) & tab & my clean(item i of my pNames) & tab & my clean(item i of my pDescs) & tab & fav & tab & my joinList(item i of my pKws, sep)
		else
			set end of my outLines to "M" & tab & (item i of my pIds)
		end if
	end repeat
	repeat with c in tops
		my walk(contents of c, "", favId, sep)
	end repeat
	return my joinText(my outLines, linefeed)
end run

on walk(c, parentId, favId, sep)
	tell application "Photos"
		with timeout of 3600 seconds
			set cid to id of c
			set cname to name of c
			set isFolder to (class of c is folder)
			set isAlbum to (class of c is album)
		end timeout
	end tell
	if cid is favId then return
	if isFolder then
		set end of my outLines to "F" & tab & cid & tab & parentId & tab & my clean(cname)
		tell application "Photos" to set kids to containers of c
		repeat with kid in kids
			my walk(contents of kid, cid, favId, sep)
		end repeat
	else if isAlbum then
		tell application "Photos"
			with timeout of 3600 seconds
				set mids to id of every media item of c
			end timeout
		end tell
		set end of my outLines to "A" & tab & cid & tab & parentId & tab & my clean(cname) & tab & my joinText(mids, sep)
	end if
end walk

on clean(v)
	if v is missing value then return ""
	set t to v as text
	if t contains tab or t contains linefeed or t contains return or t contains (character id 31) then
		return my joinText(my splitText(t, {tab, linefeed, return, character id 31}), " ")
	end if
	return t
end clean

on joinList(v, sep)
	if v is missing value then return ""
	if (count of v) is 0 then return ""
	set parts to {}
	repeat with x in v
		set end of parts to my clean(contents of x)
	end repeat
	return my joinText(parts, sep)
end joinList

on splitText(t, d)
	set saved to AppleScript's text item delimiters
	set AppleScript's text item delimiters to d
	set parts to text items of t
	set AppleScript's text item delimiters to saved
	return parts
end splitText

on joinText(parts, d)
	set saved to AppleScript's text item delimiters
	set AppleScript's text item delimiters to d
	set t to parts as text
	set AppleScript's text item delimiters to saved
	return t
end joinText
"#;

#[cfg(test)]
mod tests {
    use super::*;

    fn cand(id: i64, path: &str) -> Candidate {
        Candidate {
            id,
            path: path.to_string(),
            folder: "2026/2026-09-12".to_string(),
            is_raw: path.ends_with(".NEF"),
            exists: true,
            blake3: format!("hash-{id}"),
            ..Default::default()
        }
    }

    fn link(photo_id: i64, item: &str, hash: &str, album: &str) -> Link {
        Link {
            photo_id,
            item_id: item.to_string(),
            meta_hash: hash.to_string(),
            album: album.to_string(),
            from_photos: false,
        }
    }

    #[test]
    fn new_photos_import_and_pairs_travel_together() {
        let c = vec![
            cand(1, "/p/a.NEF"),
            cand(2, "/p/a.JPG"),
            cand(3, "/p/b.JPG"),
        ];
        let jobs = plan(&c, &[], &HashSet::new(), &PhotosSettings::default());
        assert_eq!(jobs.len(), 2);
        assert_eq!(jobs[0].kind, JobKind::Import);
        let mut ids = jobs[0].photo_ids.clone();
        ids.sort();
        assert_eq!(ids, vec![1, 2]);
        assert_eq!(jobs[0].paths.len(), 2);
        assert_eq!(jobs[0].album, "Laika");
        assert_eq!(jobs[1].photo_ids, vec![3]);
    }

    #[test]
    fn scope_rejects_and_forced_additions() {
        let mut a = cand(1, "/p/a.JPG");
        a.rejected = true;
        let mut b = cand(2, "/p/b.JPG");
        b.rating = 2;
        let mut c = cand(3, "/p/c.JPG");
        c.rating = 4;
        let d = cand(4, "/p/d.JPG");
        let s = PhotosSettings {
            scope: Scope::Rated,
            min_rating: 3,
            ..Default::default()
        };
        let forced: HashSet<i64> = [4].into_iter().collect();
        let jobs = plan(&[a, b, c, d], &[], &forced, &s);
        let ids: Vec<i64> = jobs.iter().flat_map(|j| j.photo_ids.clone()).collect();
        assert_eq!(ids, vec![3, 4]);
    }

    #[test]
    fn library_files_are_never_reimported_and_unmoved_links_retry() {
        let s = PhotosSettings::default();
        // Unlinked file inside the library: deleted in Photos → skip.
        let gone = cand(
            1,
            "/u/Pictures/Photos Library.photoslibrary/originals/A/A1.jpeg",
        );
        assert!(plan(&[gone.clone()], &[], &HashSet::new(), &s).is_empty());

        // Linked and already moved: nothing to do.
        let hash = plan(&[cand(1, "/p/a.JPG")], &[], &HashSet::new(), &s)[0]
            .meta_hash
            .clone();
        let linked = [link(1, "A1/L0/001", &hash, "Laika")];
        assert!(plan(&[gone], &linked, &HashSet::new(), &s).is_empty());

        // Linked but still in Laika's folder: retry the move.
        let jobs = plan(&[cand(1, "/p/a.JPG")], &linked, &HashSet::new(), &s);
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].kind, JobKind::Update);
        assert!(jobs[0].needs_move());
        assert_eq!(jobs[0].hashes, vec!["hash-1".to_string()]);
    }

    #[test]
    fn originals_are_matched_by_content_not_name() {
        let root = std::env::temp_dir().join(format!("laika-lib-{}", std::process::id()));
        let lib = root.join("Test.photoslibrary");
        let dir = lib.join("originals/9");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("9ABC-1.jpeg"), b"jpeg bytes").unwrap();
        std::fs::write(dir.join("9ABC-1_1_201_a.nef"), b"raw bytes").unwrap();
        std::fs::write(dir.join("9ABC-1_5.aae"), b"raw bytes").unwrap();
        std::fs::write(dir.join("9FFF-2.jpeg"), b"raw bytes").unwrap();
        let hash = |b: &[u8]| blake3::hash(b).to_hex().to_string();
        let job = Job {
            kind: JobKind::Import,
            photo_ids: vec![1, 2, 3],
            paths: vec![
                "/p/a.NEF".to_string(),
                "/p/a.JPG".to_string(),
                "/p/c.JPG".to_string(),
            ],
            hashes: vec![hash(b"raw bytes"), hash(b"jpeg bytes"), hash(b"other")],
            item_ids: vec![],
            folder: String::new(),
            album: "Laika".to_string(),
            meta: Meta::default(),
            meta_hash: String::new(),
            place: true,
        };
        let (originals, copied) = locate_originals(
            &lib,
            &job,
            &["9ABC-1/L0/001".to_string()],
            std::time::Duration::ZERO,
        );
        assert!(copied);
        assert_eq!(
            originals,
            vec![
                Some(dir.join("9ABC-1_1_201_a.nef").to_string_lossy().to_string()),
                Some(dir.join("9ABC-1.jpeg").to_string_lossy().to_string()),
                None,
            ]
        );
        assert!(check_library(&lib).is_ok());

        // Nothing stored for the item: Photos referenced instead of copying.
        let (none, copied) = locate_originals(
            &lib,
            &job,
            &["7777/L0/001".to_string()],
            std::time::Duration::ZERO,
        );
        assert!(!copied);
        assert!(none.iter().all(|o| o.is_none()));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn missing_files_are_not_imported() {
        let mut a = cand(1, "/p/a.JPG");
        a.exists = false;
        assert!(plan(&[a], &[], &HashSet::new(), &PhotosSettings::default()).is_empty());
    }

    #[test]
    fn linked_photos_update_only_when_something_changed() {
        let s = PhotosSettings::default();
        let mut a = cand(1, "/p/a.JPG");
        a.title = "Harbor".to_string();
        let first = plan(std::slice::from_ref(&a), &[], &HashSet::new(), &s);
        let hash = first[0].meta_hash.clone();
        let linked = [link(1, "ITEM/L0/001", &hash, "Laika")];
        // Still in Laika's folder: the move is retried.
        assert_eq!(
            plan(std::slice::from_ref(&a), &linked, &HashSet::new(), &s).len(),
            1
        );
        a.path = "/u/Photos Library.photoslibrary/originals/I/ITEM.jpeg".to_string();
        assert!(plan(std::slice::from_ref(&a), &linked, &HashSet::new(), &s).is_empty());

        a.keywords = vec!["Places > Portugal > Lisbon".to_string()];
        let jobs = plan(std::slice::from_ref(&a), &linked, &HashSet::new(), &s);
        assert_eq!(jobs.len(), 1);
        assert_eq!(jobs[0].kind, JobKind::Update);
        assert_eq!(jobs[0].item_ids, vec!["ITEM/L0/001".to_string()]);
        assert_eq!(jobs[0].meta.keywords, Some(vec!["Lisbon".to_string()]));
        assert!(!jobs[0].place);

        // Switching to folder albums re-places the existing item.
        let by_folder = PhotosSettings {
            by_folder: true,
            ..Default::default()
        };
        let mut outside = a.clone();
        outside.path = "/p/a.JPG".to_string();
        let moved = plan(&[outside], &linked, &HashSet::new(), &by_folder);
        assert_eq!(moved.len(), 1);
        assert_eq!(moved[0].album_key(), "Laika/2026/2026-09-12");
        assert!(moved[0].place);
        // Once in Photos the catalog folder is gone: the album stays put.
        a.keywords.clear();
        a.folder = "Apple Photos".to_string();
        assert!(plan(&[a], &linked, &HashSet::new(), &by_folder).is_empty());
    }

    #[test]
    fn favorites_follow_the_rule_and_metadata_can_be_off() {
        let mut a = cand(1, "/p/a.JPG");
        a.rating = 5;
        a.title = "t".to_string();
        let s = PhotosSettings {
            favorites: FavoriteRule::FiveStars,
            metadata: false,
            ..Default::default()
        };
        let jobs = plan(&[a], &[], &HashSet::new(), &s);
        assert_eq!(
            jobs[0].meta,
            Meta {
                favorite: Some(true),
                ..Meta::default()
            }
        );
    }

    #[test]
    fn manifest_escapes_separators_and_output_parses() {
        let job = Job {
            kind: JobKind::Import,
            photo_ids: vec![1],
            paths: vec!["/p/a b.JPG".to_string()],
            hashes: vec!["h".to_string()],
            item_ids: vec![],
            folder: String::new(),
            album: "Laika".to_string(),
            meta: Meta {
                name: Some("Tab\there".to_string()),
                description: Some("line\nbreak".to_string()),
                keywords: Some(vec!["a".to_string(), "b".to_string()]),
                favorite: Some(false),
            },
            meta_hash: String::new(),
            place: true,
        };
        let m = manifest(std::slice::from_ref(&job));
        let fields: Vec<&str> = m.trim_end_matches('\n').split('\t').collect();
        assert_eq!(fields.len(), 14);
        assert_eq!(fields[7], "Tab here");
        assert_eq!(fields[9], "line break");
        assert_eq!(fields[11], "a\u{1f}b");
        assert_eq!(fields[12], "0");

        let out = "0\tok\tA/L0/001\u{1f}B/L0/001\t\n1\tmissing\t\n2\terror\t\t-43 File not found\n";
        assert_eq!(
            parse_output(out, 4),
            vec![
                Outcome::Done(vec!["A/L0/001".to_string(), "B/L0/001".to_string()]),
                Outcome::Missing,
                Outcome::Failed("-43 File not found".to_string()),
                Outcome::Failed("no result from Photos".to_string()),
            ]
        );
        assert_eq!(friendly_job_error("-43 File not found"), "file not found");
        assert!(friendly_error("execution error: Not authorized (-1743)").contains("Automation"));
    }

    #[test]
    fn library_stamp_changes_when_the_database_does() {
        let lib =
            std::env::temp_dir().join(format!("laika-stamp-{}.photoslibrary", std::process::id()));
        std::fs::create_dir_all(lib.join("database")).unwrap();
        std::fs::write(lib.join("database/Photos.sqlite"), b"a").unwrap();
        let first = library_stamp(&lib);
        assert_eq!(first, library_stamp(&lib));
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(lib.join("database/Photos.sqlite-wal"), b"b").unwrap();
        assert_ne!(first, library_stamp(&lib));
        std::fs::remove_dir_all(&lib).ok();
    }

    #[test]
    fn dump_parses_items_and_tree() {
        let out = "M\tAAA/L0/001\tIMG_1.HEIC\tBeach\t\t1\tsea\u{1f}sun\n\
                   M\tBBB/L0/001\tIMG_2.JPG\t\t\t0\t\n\
                   F\tF1/L0/020\t\tTrips\n\
                   A\tA1/L0/040\tF1/L0/020\tPortugal\tAAA/L0/001\u{1f}BBB/L0/001\n\
                   A\tA2/L0/040\t\tEmpty\t\n";
        let d = parse_dump(out);
        assert_eq!(d.items.len(), 2);
        assert!(d.items[0].favorite);
        assert_eq!(d.items[0].keywords, vec!["sea", "sun"]);
        assert_eq!(d.items[1].name, "");
        assert_eq!(d.containers.len(), 3);
        assert_eq!(d.containers[0].kind, ContainerKind::Folder);
        assert_eq!(d.containers[1].parent, "F1/L0/020");
        assert_eq!(d.containers[1].items.len(), 2);
        assert!(d.containers[2].items.is_empty());
    }

    #[test]
    fn ingest_indexes_originals_and_skips_what_it_cant_use() {
        let root = std::env::temp_dir().join(format!("laika-ingest-{}", std::process::id()));
        let lib = root.join("L.photoslibrary");
        let a = lib.join("originals/A");
        let b = lib.join("originals/B");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        for f in [
            a.join("AAA.jpeg"),
            a.join("AAA_4.nef"),
            a.join("AAA_5.aae"),
            b.join("BBB.heic"),
            b.join("BBB_3.mov"),
            b.join("BCD.xyz"),
        ] {
            std::fs::write(f, b"x").unwrap();
        }
        let index = index_originals(&lib);
        assert_eq!(index["AAA"], vec![a.join("AAA.jpeg"), a.join("AAA_4.nef")]);
        assert_eq!(index["BBB"], vec![b.join("BBB.heic")]);

        let item = |id: &str| LibraryItem {
            id: id.to_string(),
            ..Default::default()
        };
        let dump = LibraryDump {
            items: vec![
                item("AAA/L0/001"),
                item("BBB/L0/001"),
                item("BCD/L0/001"),
                item("CCC/L0/001"),
                item("DDD/L0/001"),
            ],
            containers: vec![Container {
                id: "A1".to_string(),
                parent: String::new(),
                name: "Trip".to_string(),
                kind: ContainerKind::Album,
                items: vec!["BBB/L0/001".to_string()],
            }],
        };
        let linked: HashSet<String> = ["DDD/L0/001".to_string()].into_iter().collect();
        let plan = plan_ingest(&dump, &index, &linked, &HashSet::new(), true);
        let paths: Vec<&str> = plan.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths.len(), 3, "{paths:?}");
        assert!(plan.files.iter().all(|f| f.item_id != "CCC/L0/001"));
        assert_eq!((plan.present, plan.not_local, plan.unsupported), (1, 1, 1));

        // Albums only: just the album's photo.
        let only = plan_ingest(&dump, &index, &linked, &HashSet::new(), false);
        assert_eq!(only.files.len(), 1);
        assert_eq!(only.files[0].item_id, "BBB/L0/001");

        // Cataloged but unlinked files are only linked, not imported again.
        let known: HashSet<String> = plan.files.iter().map(|f| f.path.clone()).collect();
        let again = plan_ingest(&dump, &index, &linked, &known, true);
        assert_eq!(again.files.len(), 3);
        assert!(again.files.iter().all(|f| f.known));

        // The pair's RAW groups with its JPEG.
        let pk = |p: &std::path::Path| crate::pairs::pair_key(&p.to_string_lossy()).unwrap();
        assert_eq!(pk(&a.join("AAA.jpeg")), pk(&a.join("AAA_4.nef")));
        std::fs::remove_dir_all(&root).ok();
    }

    /// Read-only check against this Mac's real library (opt-in).
    #[cfg(target_os = "macos")]
    #[test]
    fn finds_the_open_library() {
        if std::env::var("LAIKA_PHOTOS_LIBRARY_TEST").is_err() {
            return;
        }
        let lib = library_path().unwrap();
        eprintln!("library: {}", lib.display());
        check_library(&lib).unwrap();
    }

    /// Live round trip against the real Photos library (opt-in; imports
    /// the files in `LAIKA_PHOTOS_LIVE_DIR` into the album "Laika Test").
    #[cfg(target_os = "macos")]
    #[test]
    fn live_import_and_verify() {
        let Ok(dir) = std::env::var("LAIKA_PHOTOS_LIVE_DIR") else {
            return;
        };
        let dir = std::path::Path::new(&dir);
        let file = |n: &str| dir.join(n).to_string_lossy().to_string();
        let hash = |p: &str| crate::catalog::hash_file(std::path::Path::new(p)).unwrap();
        let meta = |name: &str| Meta {
            name: Some(name.to_string()),
            description: Some("Laika live test — safe to delete".to_string()),
            keywords: Some(vec!["laika-test".to_string()]),
            favorite: Some(false),
        };
        let job = |ids: Vec<i64>, paths: Vec<String>, name: &str| Job {
            kind: JobKind::Import,
            photo_ids: ids,
            hashes: paths.iter().map(|p| hash(p)).collect(),
            paths,
            item_ids: vec![],
            folder: String::new(),
            album: "Laika Test".to_string(),
            meta: meta(name),
            meta_hash: String::new(),
            place: true,
        };
        let jobs = vec![
            job(vec![1], vec![file("laika-live-single.jpg")], "Laika single"),
            job(
                vec![2, 3],
                vec![file("IMG_5443.NEF"), file("IMG_5443.JPG")],
                "Laika pair",
            ),
        ];
        let out = run_blocking(&jobs).unwrap();
        eprintln!("outcomes: {out:#?}");
        let mut first_item = String::new();
        for (job, o) in jobs.iter().zip(&out) {
            match o {
                Outcome::Placed {
                    ids,
                    originals,
                    copied,
                } => {
                    assert!(*copied, "Photos didn't copy {:?}", job.paths);
                    if first_item.is_empty() {
                        first_item = ids[0].clone();
                    }
                    for (p, o) in job.paths.iter().zip(originals) {
                        eprintln!("{p} -> {o:?}");
                    }
                }
                other => panic!("{:?}: {other:?}", job.paths),
            }
        }
        // The single JPEG must have been found and verified.
        match &out[0] {
            Outcome::Placed { originals, .. } => assert!(originals[0].is_some()),
            _ => unreachable!(),
        }

        // Metadata update of an existing item, then a missing-item probe.
        let mut update = jobs[0].clone();
        update.kind = JobKind::Update;
        update.item_ids = vec![first_item];
        update.paths = vec![];
        update.hashes = vec![];
        update.place = false;
        update.meta.keywords = Some(vec!["laika-test".to_string(), "updated".to_string()]);
        let mut gone = update.clone();
        gone.item_ids = vec!["00000000-0000-0000-0000-000000000000/L0/001".to_string()];
        let out = run_blocking(&[update, gone]).unwrap();
        eprintln!("update outcomes: {out:#?}");
        assert!(matches!(out[0], Outcome::Done(_)));
        assert_eq!(out[1], Outcome::Missing);
    }

    /// Read-only: dump the real library and plan an ingest (opt-in).
    #[cfg(target_os = "macos")]
    #[test]
    fn live_dump_and_plan() {
        if std::env::var("LAIKA_PHOTOS_DUMP_TEST").is_err() {
            return;
        }
        let t = std::time::Instant::now();
        let lib = library_path().unwrap();
        let dump = dump_library_blocking(false).unwrap();
        let read = t.elapsed();
        let t2 = std::time::Instant::now();
        let sample: Vec<String> = dump.items.iter().take(50).map(|i| i.id.clone()).collect();
        let meta = item_metadata_blocking(&sample).unwrap();
        eprintln!(
            "metadata for {} items in {:?} ({} with keywords)",
            meta.len(),
            t2.elapsed(),
            meta.iter().filter(|m| !m.keywords.is_empty()).count()
        );
        assert_eq!(meta.len(), sample.len());
        assert!(meta.iter().all(|m| !m.filename.is_empty()));
        let index = index_originals(&lib);
        let plan = plan_ingest(&dump, &index, &HashSet::new(), &HashSet::new(), true);
        let albums = dump
            .containers
            .iter()
            .filter(|c| c.kind == ContainerKind::Album)
            .count();
        let in_albums: usize = dump.containers.iter().map(|c| c.items.len()).sum();
        let titled = dump.items.iter().filter(|i| !i.name.is_empty()).count();
        let kw = dump.items.iter().filter(|i| !i.keywords.is_empty()).count();
        let fav = dump.items.iter().filter(|i| i.favorite).count();
        eprintln!(
            "read {:?}; items {}, folders {}, albums {} ({} memberships); titled {}, keywords {}, \
             favorites {}; originals indexed {}; plan files {}, not local {}, unsupported {}; total {:?}",
            read,
            dump.items.len(),
            dump.containers.len() - albums,
            albums,
            in_albums,
            titled,
            kw,
            fav,
            index.len(),
            plan.files.len(),
            plan.not_local,
            plan.unsupported,
            t.elapsed()
        );
        let unknown: usize = dump
            .containers
            .iter()
            .flat_map(|c| c.items.iter())
            .filter(|i| !dump.items.iter().any(|m| &m.id == *i))
            .count();
        eprintln!("album members not in the item list: {unknown}");
        assert!(!dump.items.is_empty());
    }

    /// The script must compile against Photos' scripting dictionary.
    /// `osacompile` reads the dictionary without launching Photos.
    #[cfg(target_os = "macos")]
    #[test]
    fn script_compiles() {
        if !std::path::Path::new("/System/Applications/Photos.app").exists() {
            return;
        }
        let dir = std::env::temp_dir().join(format!("laika-photos-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for (name, text) in [
            ("sync", SCRIPT),
            ("dump", DUMP_SCRIPT),
            ("meta", META_SCRIPT),
        ] {
            let src = dir.join(format!("{name}.applescript"));
            std::fs::write(&src, text).unwrap();
            let out = std::process::Command::new("/usr/bin/osacompile")
                .arg("-o")
                .arg(dir.join(format!("{name}.scpt")))
                .arg(&src)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{name}: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        std::fs::remove_dir_all(&dir).ok();
    }
}
