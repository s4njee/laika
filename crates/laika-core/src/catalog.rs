//! SQLite catalog (single file per catalog) + import pipeline.
//!
//! Import runs file-by-file: blake3 → EXIF → embedded preview → 512/2048
//! derivatives in the cache → row insert. The caller decides threading; this
//! module stays synchronous and testable.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use rusqlite::{Connection, params};

use crate::photo::SyncState;

pub const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS catalogs(id INTEGER PRIMARY KEY, name TEXT NOT NULL, root_path TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS photos(
  id INTEGER PRIMARY KEY, catalog_id INTEGER NOT NULL, path TEXT NOT NULL UNIQUE, filename TEXT NOT NULL,
  blake3 TEXT NOT NULL, captured_at TEXT, camera TEXT, lens TEXT, focal_mm TEXT, aperture TEXT,
  shutter TEXT, iso TEXT, width INTEGER, height INTEGER,
  rating INTEGER DEFAULT 0, picked INTEGER DEFAULT 0, rejected INTEGER DEFAULT 0,
  sync_state TEXT DEFAULT 'local', remote_key TEXT, imported_at TEXT,
  creator TEXT DEFAULT '', copyright TEXT DEFAULT '', rights TEXT DEFAULT '', contact TEXT DEFAULT '',
  captured_orig TEXT DEFAULT '', capture_offset_min INTEGER DEFAULT 0,
  duration_ms INTEGER DEFAULT 0, codec TEXT DEFAULT ''
);
CREATE TABLE IF NOT EXISTS keywords(photo_id INTEGER NOT NULL, keyword TEXT NOT NULL);
CREATE TABLE IF NOT EXISTS edits(photo_id INTEGER PRIMARY KEY, params_json TEXT, history_json TEXT, cursor INTEGER, updated_at TEXT);
CREATE TABLE IF NOT EXISTS sync_queue(photo_id INTEGER NOT NULL, kind TEXT, state TEXT, attempts INTEGER DEFAULT 0, error TEXT);
CREATE INDEX IF NOT EXISTS idx_photos_catalog ON photos(catalog_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_sync_queue_job ON sync_queue(photo_id, kind);
CREATE TABLE IF NOT EXISTS sync_settings(key TEXT PRIMARY KEY, value TEXT);
-- U02: mtime (epoch secs) this app last wrote each sidecar. Lets rescan tell
-- our own writes apart from external edits and stale files.
CREATE TABLE IF NOT EXISTS sidecar_state(photo_id INTEGER PRIMARY KEY, written_mtime TEXT);
-- U05/V01: import memory. card_history remembers verified hashes per volume
-- so reinserted cards import only new frames; import_batches journals runs.
CREATE TABLE IF NOT EXISTS card_history(file_hash TEXT NOT NULL, volume TEXT NOT NULL, imported_at TEXT NOT NULL,
  PRIMARY KEY(file_hash, volume));
CREATE TABLE IF NOT EXISTS import_batches(id INTEGER PRIMARY KEY, source TEXT NOT NULL, volume TEXT NOT NULL,
  mode TEXT NOT NULL, started_at TEXT NOT NULL, finished_at TEXT NOT NULL,
  imported INTEGER DEFAULT 0, skipped_dup INTEGER DEFAULT 0, failed INTEGER DEFAULT 0);
-- V02: named folder/rename templates; last_used picks the default.
CREATE TABLE IF NOT EXISTS name_templates(kind TEXT NOT NULL, name TEXT NOT NULL, value TEXT NOT NULL,
  last_used TEXT NOT NULL DEFAULT '0', PRIMARY KEY(kind, name));
-- V03: named metadata presets (creator/copyright/rights/contact/keywords).
CREATE TABLE IF NOT EXISTS metadata_presets(name TEXT PRIMARY KEY, creator TEXT NOT NULL DEFAULT '',
  copyright TEXT NOT NULL DEFAULT '', rights TEXT NOT NULL DEFAULT '',
  contact TEXT NOT NULL DEFAULT '', keywords TEXT NOT NULL DEFAULT '', last_used TEXT NOT NULL DEFAULT '0');
-- V03: per-run import defaults (preset selections).
CREATE TABLE IF NOT EXISTS import_defaults(key TEXT PRIMARY KEY, value TEXT NOT NULL DEFAULT '');
-- U12: saved filter sets (JSON) with last-used ordering.
CREATE TABLE IF NOT EXISTS filter_presets(name TEXT PRIMARY KEY, filters_json TEXT NOT NULL,
  last_used TEXT NOT NULL DEFAULT '0');
-- U14: named full-state snapshots per photo (local-only treatments).
CREATE TABLE IF NOT EXISTS snapshots(id INTEGER PRIMARY KEY, photo_id INTEGER NOT NULL,
  name TEXT NOT NULL, state_json TEXT NOT NULL, created_at TEXT NOT NULL);
CREATE INDEX IF NOT EXISTS idx_snapshots_photo ON snapshots(photo_id);
-- V04: watched folders for automatic import (polled, stability-gated).
CREATE TABLE IF NOT EXISTS watched_folders(path TEXT PRIMARY KEY, added_at TEXT NOT NULL);
";

#[derive(Clone, Debug, Default)]
pub struct DbPhoto {
    pub id: i64,
    pub catalog_id: i64,
    pub path: String,
    pub filename: String,
    pub blake3: String,
    pub captured_at: String,
    pub camera: String,
    pub lens: String,
    pub focal_mm: String,
    pub aperture: String,
    pub shutter: String,
    pub iso: String,
    pub width: u32,
    pub height: u32,
    pub rating: u8,
    pub picked: bool,
    pub rejected: bool,
    pub sync: SyncState,
    pub remote_key: String,
    /// V03/V12: IPTC-ish authorship (empty until applied at import).
    pub creator: String,
    pub copyright: String,
    pub rights: String,
    pub contact: String,
    /// V03/V14: pre-offset capture time + applied minutes (0 = none).
    pub captured_orig: String,
    pub capture_offset_min: i32,
    /// V12: descriptive metadata (right-rail editable, empty until set).
    pub title: String,
    pub caption: String,
    pub headline: String,
    pub location: String,
    /// V12: read-only EXIF overflow (display strings, import-filled).
    pub exif_program: String,
    pub exif_metering: String,
    pub exif_flash: String,
    pub exif_focal35: String,
    pub exif_serial: String,
    pub exif_firmware: String,
    pub exif_gps: String,
    /// V05: video duration + codec (0/empty for stills).
    pub duration_ms: i64,
    pub codec: String,
}

impl DbPhoto {
    pub fn is_raw(&self) -> bool {
        laika_raw::is_raw(Path::new(&self.path))
    }

    pub fn cache_key(&self) -> &str {
        &self.blake3
    }
}

/// V15: full row snapshot for undoable Remove (photo + persisted
/// edits + keywords + named snapshots).
#[derive(Clone, Debug)]
pub struct RemovedPhoto {
    pub photo: DbPhoto,
    pub params_json: Option<String>,
    pub history_json: Option<String>,
    pub cursor: i64,
    pub keywords: Vec<String>,
    /// (name, state_json) pairs.
    pub snapshots: Vec<(String, String)>,
}

/// U17: batch folder-relink outcome.
#[derive(Clone, Debug, Default)]
pub struct FolderRelinkReport {
    pub linked: usize,
    pub missing: Vec<String>,
    pub mismatched: Vec<String>,
}

/// U17: validated backup file summary.
#[derive(Clone, Debug)]
pub struct BackupProbe {
    pub photos: i64,
    pub bytes: u64,
}

/// V04: folder synchronize comparison.
#[derive(Clone, Debug, Default)]
pub struct FolderDiff {
    /// Supported files on disk with no catalog row.
    pub new_files: Vec<std::path::PathBuf>,
    /// Catalog rows whose files are gone: (photo id, filename).
    pub missing: Vec<(i64, String)>,
    /// Rows whose sidecars changed externally: (photo id, filename).
    pub changed: Vec<(i64, String)>,
    /// The folder itself was unreadable.
    pub error: Option<String>,
}

pub struct Catalog {
    conn: Connection,
    pub id: i64,
    db_path: PathBuf,
}

/// V03: named metadata preset (authorship + keywords).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MetadataPreset {
    pub name: String,
    pub title: String,
    pub caption: String,
    pub headline: String,
    pub creator: String,
    pub copyright: String,
    pub rights: String,
    pub contact: String,
    pub location: String,
    /// Comma-separated keyword list as typed.
    pub keywords: String,
}

/// V12: the right-rail editable descriptive block (batch panels diff
/// field-by-field on this; `<mixed>` marks disagreement).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PhotoMeta {
    pub title: String,
    pub caption: String,
    pub headline: String,
    pub creator: String,
    pub copyright: String,
    pub rights: String,
    pub contact: String,
    pub location: String,
}

/// V07: advisory catalog lock. One writer per catalog file: the second
/// opener gets holder details instead of risking corruption. Stale locks
/// (dead pid, same host) are taken over with a note.
#[derive(Debug)]
pub struct CatalogLock {
    path: PathBuf,
}

impl CatalogLock {
    /// Lock file holding `pid\nhostname\ntimestamp`.
    pub fn acquire(db_path: &Path) -> Result<Self, String> {
        let path = lock_path(db_path);
        if path.exists() {
            if let Some(holder) = read_lock(&path) {
                // Stale when the holder is dead on this machine. Hostnames
                // drift (network changes turn `mbp.local` into `MacBookPro`),
                // so on a local disk — which no other machine can have open
                // — a dead pid is enough; network shares keep the host check.
                let this_machine = same_host(&holder.host, &this_host())
                    || path.parent().is_some_and(on_local_volume);
                if this_machine && !pid_alive(holder.pid) {
                    eprintln!(
                        "[catalog] taking over stale lock from pid {} on {}",
                        holder.pid, holder.host
                    );
                    let _ = std::fs::remove_file(&path);
                } else {
                    return Err(format!(
                        "catalog is open elsewhere (pid {} on {}) — close it first",
                        holder.pid, holder.host
                    ));
                }
            } else {
                // Unreadable lock: safest to refuse as well.
                return Err(
                    "catalog lock file is unreadable — remove it if no Laika runs".to_string(),
                );
            }
        }
        let content = format!(
            "{}\n{}\n{}",
            std::process::id(),
            this_host(),
            chrono_sys_secs()
        );
        // create_new: two instances launching together must not both pass
        // the exists() check above and each believe they hold the lock.
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::AlreadyExists {
                    "catalog is open elsewhere (lock just taken) — close it first".to_string()
                } else {
                    format!("lock catalog: {e}")
                }
            })?;
        {
            use std::io::Write;
            if let Err(e) = f.write_all(content.as_bytes()) {
                let _ = std::fs::remove_file(&path);
                return Err(format!("lock catalog: {e}"));
            }
        }
        Ok(Self { path })
    }

    /// Release early for catalog switches (Drop covers the rest).
    pub fn release(self) {}
}

impl Drop for CatalogLock {
    fn drop(&mut self) {
        // Only remove our own lock (pid match): never unlink a successor.
        if let Some(holder) = read_lock(&self.path) {
            if holder.pid == std::process::id() && holder.host == this_host() {
                let _ = std::fs::remove_file(&self.path);
            }
        }
    }
}

fn lock_path(db_path: &Path) -> PathBuf {
    let mut s = db_path.to_string_lossy().to_string();
    s.push_str(".lock");
    PathBuf::from(s)
}

struct LockHolder {
    pid: u32,
    host: String,
}

fn read_lock(path: &Path) -> Option<LockHolder> {
    let content = std::fs::read_to_string(path).ok()?;
    let mut lines = content.lines();
    Some(LockHolder {
        pid: lines.next()?.parse().ok()?,
        host: lines.next()?.to_string(),
    })
}

fn this_host() -> String {
    let mut buf = [0i8; 256];
    // SAFETY: gethostname writes at most `len` bytes into a valid buffer.
    let ok = unsafe { libc::gethostname(buf.as_mut_ptr(), buf.len()) == 0 };
    if !ok {
        return "unknown".to_string();
    }
    let len = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    buf[..len].iter().map(|&c| c as u8 as char).collect()
}

/// Hostname equality tolerant of `.local`/domain suffixes and case.
fn same_host(a: &str, b: &str) -> bool {
    let short = |h: &str| h.split('.').next().unwrap_or(h).to_ascii_lowercase();
    short(a) == short(b)
}

/// True when `dir` is on a locally attached volume (not NFS/SMB/AFP).
#[cfg(target_os = "macos")]
fn on_local_volume(dir: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;
    let dir = if dir.as_os_str().is_empty() {
        Path::new(".")
    } else {
        dir
    };
    let Ok(c) = std::ffi::CString::new(dir.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: statfs fills the zeroed struct for a valid C path.
    let mut st: libc::statfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statfs(c.as_ptr(), &mut st) } != 0 {
        return false;
    }
    st.f_flags & (libc::MNT_LOCAL as u32) != 0
}

#[cfg(not(target_os = "macos"))]
fn on_local_volume(_dir: &Path) -> bool {
    false
}

fn pid_alive(pid: u32) -> bool {
    // Out-of-range pids would go negative and address a process group.
    if pid == 0 || pid > i32::MAX as u32 {
        return false;
    }
    // SAFETY: kill(pid, 0) performs no action, only error checking.
    let r = unsafe { libc::kill(pid as i32, 0) };
    if r == 0 {
        return true;
    }
    // EPERM means the process exists but belongs to another user.
    std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

fn chrono_sys_secs() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default()
}

/// One row of the mirrored Apple Photos folder/album tree.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PhotosAlbumNode {
    pub id: String,
    pub parent: String,
    pub name: String,
    pub is_folder: bool,
    pub depth: usize,
    /// Catalog photos in the album (0 for folders).
    pub count: usize,
}

/// V07: app-level library state (recents + launch preference), stored
/// beside the catalogs, never inside one.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct LibraryState {
    #[serde(default)]
    pub recent: Vec<String>,
    #[serde(default = "default_startup")]
    pub startup: StartupMode,
    #[serde(default)]
    pub fixed: String,
    /// V09: preview cache location override (empty = default dir).
    #[serde(default)]
    pub cache_dir: String,
    /// V09: cache cap in MB (evictable class: 1:1 previews).
    #[serde(default = "default_cache_cap")]
    pub cache_cap_mb: u64,
    /// V09: 1:1 preview TTL in days (0 = keep until over cap).
    #[serde(default = "default_cache_ttl")]
    pub cache_ttl_days: u64,
    /// UI surface palette key ("grey" | "black"; empty = grey).
    #[serde(default)]
    pub appearance: String,
    /// UI accent as `#RRGGBB` (empty = the default green).
    #[serde(default)]
    pub accent: String,
}

fn default_cache_cap() -> u64 {
    2048
}

fn default_cache_ttl() -> u64 {
    30
}

fn default_startup() -> StartupMode {
    StartupMode::Last
}

/// V07: which catalog loads at launch.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum StartupMode {
    Last,
    Ask,
    Fixed,
}

impl Default for LibraryState {
    fn default() -> Self {
        Self {
            recent: Vec::new(),
            startup: StartupMode::Last,
            fixed: String::new(),
            cache_dir: String::new(),
            cache_cap_mb: default_cache_cap(),
            cache_ttl_days: default_cache_ttl(),
            appearance: String::new(),
            accent: String::new(),
        }
    }
}

impl LibraryState {
    fn path(base: &Path) -> PathBuf {
        base.join("library.json")
    }

    pub fn read(base: &Path) -> Self {
        std::fs::read(Self::path(base))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    fn write(&self, base: &Path) -> Result<(), String> {
        std::fs::create_dir_all(base).map_err(|e| e.to_string())?;
        let json = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(Self::path(base), json).map_err(|e| e.to_string())
    }

    /// Most-recent-first, deduped, capped at 8. Missing files stay listed
    /// (drives come back) but sort after existing ones.
    pub fn push_recent(&mut self, db_path: &Path, base: &Path) {
        let s = db_path.to_string_lossy().to_string();
        self.recent.retain(|p| p != &s);
        self.recent.insert(0, s);
        self.recent.truncate(8);
        if let Err(e) = self.write(base) {
            eprintln!("[catalog] recent list not saved: {e}");
        }
    }

    pub fn set_startup(&mut self, mode: StartupMode, fixed: &str, base: &Path) {
        self.startup = mode;
        self.fixed = fixed.to_string();
        if let Err(e) = self.write(base) {
            eprintln!("[catalog] launch preference not saved: {e}");
        }
    }

    /// V09: cache policy (empty dir keeps the current location).
    pub fn set_cache(&mut self, dir: &str, cap_mb: u64, ttl_days: u64, base: &Path) {
        if !dir.is_empty() {
            self.cache_dir = dir.to_string();
        }
        self.cache_cap_mb = cap_mb.clamp(256, 65536);
        self.cache_ttl_days = ttl_days.min(365);
        if let Err(e) = self.write(base) {
            eprintln!("[catalog] cache settings not saved: {e}");
        }
    }

    /// App appearance (palette key + accent hex), app-wide.
    pub fn set_appearance(&mut self, appearance: &str, accent: &str, base: &Path) {
        self.appearance = appearance.to_string();
        self.accent = accent.to_string();
        if let Err(e) = self.write(base) {
            eprintln!("[catalog] appearance not saved: {e}");
        }
    }

    /// Recents with existing files first (missing drives sink).
    pub fn ordered(&self) -> Vec<String> {
        let (mut here, mut gone): (Vec<_>, Vec<_>) = self
            .recent
            .iter()
            .cloned()
            .partition(|p| Path::new(p).exists());
        here.append(&mut gone);
        here
    }
}

/// Outcome of reconciling one sidecar against the catalog (U02).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SidecarApply {
    /// No sidecar file next to the original.
    Missing,
    /// Sidecar adopted (no local edits to protect).
    Applied,
    /// Sidecar changed externally since our last write — adopted + reported.
    AppliedExternal,
    /// Sidecar older than acknowledged catalog values — catalog wins.
    Stale,
    /// Sidecar exists but is unreadable — catalog values kept.
    Corrupt,
}

/// Startup/import reconciliation summary.
#[derive(Clone, Debug, Default)]
pub struct RescanReport {
    pub applied: usize,
    pub external: usize,
    pub healed: usize,
    pub error: Option<String>,
}

/// V08: SQLite magic bytes ("SQLite format 3\0").
fn is_sqlite_file(path: &Path) -> bool {
    std::fs::File::open(path)
        .and_then(|mut f| {
            use std::io::Read;
            let mut magic = [0u8; 16];
            f.read_exact(&mut magic)?;
            Ok(magic)
        })
        .map(|m| &m == b"SQLite format 3\0")
        .unwrap_or(false)
}

/// V08: ordered schema migrations. Each runs in its own transaction
/// after a pre-migration backup copy; a failed step names itself and the
/// transaction rolls back, so the file stays intact.
/// V08-mini helper shared by fresh opens and the v1 migration.
fn ensure_photo_columns(conn: &rusqlite::Connection) -> Result<(), String> {
    for (col, ddl) in [
        ("creator", "TEXT DEFAULT ''"),
        ("copyright", "TEXT DEFAULT ''"),
        ("rights", "TEXT DEFAULT ''"),
        ("contact", "TEXT DEFAULT ''"),
        ("captured_orig", "TEXT DEFAULT ''"),
        ("capture_offset_min", "INTEGER DEFAULT 0"),
        ("duration_ms", "INTEGER DEFAULT 0"),
        ("codec", "TEXT DEFAULT ''"),
        // V12: descriptive metadata (right-rail editable).
        ("title", "TEXT DEFAULT ''"),
        ("caption", "TEXT DEFAULT ''"),
        ("headline", "TEXT DEFAULT ''"),
        ("location", "TEXT DEFAULT ''"),
        // V12: read-only EXIF overflow (display strings, import-filled).
        ("exif_program", "TEXT DEFAULT ''"),
        ("exif_metering", "TEXT DEFAULT ''"),
        ("exif_flash", "TEXT DEFAULT ''"),
        ("exif_focal35", "TEXT DEFAULT ''"),
        ("exif_serial", "TEXT DEFAULT ''"),
        ("exif_firmware", "TEXT DEFAULT ''"),
        ("exif_gps", "TEXT DEFAULT ''"),
    ] {
        let exists: bool = conn
            .prepare("SELECT 1 FROM pragma_table_info('photos') WHERE name = ?1")
            .and_then(|mut s| s.query_row([col], |_| Ok(())))
            .is_ok();
        if !exists {
            conn.execute_batch(&format!("ALTER TABLE photos ADD COLUMN {col} {ddl}"))
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

pub mod migrations {
    use super::{SCHEMA, ensure_photo_columns};
    use rusqlite::Connection;

    pub struct Migration {
        pub version: u32,
        pub name: &'static str,
        pub apply: fn(&Connection) -> Result<(), String>,
    }

    fn migrate_v1(conn: &Connection) -> Result<(), String> {
        conn.execute_batch(SCHEMA)
            .map_err(|e| format!("baseline tables: {e}"))?;
        ensure_photo_columns(conn)
    }

    /// V08: columns the minimal prototype era predates (ratings, flags,
    /// sync state). Existing data is never touched — ADD COLUMN only.
    fn ensure_primary_columns(conn: &Connection) -> Result<(), String> {
        for (col, ddl) in [
            ("rating", "INTEGER DEFAULT 0"),
            ("picked", "INTEGER DEFAULT 0"),
            ("rejected", "INTEGER DEFAULT 0"),
            ("sync_state", "TEXT DEFAULT 'local'"),
            ("remote_key", "TEXT"),
            ("imported_at", "TEXT"),
        ] {
            let exists: bool = conn
                .prepare("SELECT 1 FROM pragma_table_info('photos') WHERE name = ?1")
                .and_then(|mut s| s.query_row([col], |_| Ok(())))
                .is_ok();
            if !exists {
                conn.execute_batch(&format!("ALTER TABLE photos ADD COLUMN {col} {ddl}"))
                    .map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }

    fn migrate_v2(conn: &Connection) -> Result<(), String> {
        // Columns first (indexes below must resolve on legacy tables),
        // then the query indexes: capture time, folder prefix, hash,
        // rating, flags, sync state, path, keywords.
        ensure_primary_columns(conn)?;
        ensure_photo_columns(conn)?;
        conn.execute_batch(
            "CREATE INDEX IF NOT EXISTS idx_photos_captured ON photos(captured_at);
             CREATE INDEX IF NOT EXISTS idx_photos_blake3 ON photos(blake3);
             CREATE INDEX IF NOT EXISTS idx_photos_rating ON photos(rating);
             CREATE INDEX IF NOT EXISTS idx_photos_flags ON photos(picked, rejected);
             CREATE INDEX IF NOT EXISTS idx_photos_sync ON photos(sync_state);
             CREATE INDEX IF NOT EXISTS idx_photos_path ON photos(path);
             CREATE INDEX IF NOT EXISTS idx_keywords_photo ON keywords(photo_id);",
        )
        .map_err(|e| format!("query indexes: {e}"))
    }

    /// V12: descriptive columns land via `ensure_photo_columns`
    /// (ADD-only, legacy-safe); presets gain the new fields and the
    /// keyword hierarchy tables appear empty.
    fn migrate_v4(conn: &Connection) -> Result<(), String> {
        super::ensure_photo_columns(conn)?;
        for (col, ddl) in [
            ("title", "TEXT DEFAULT ''"),
            ("caption", "TEXT DEFAULT ''"),
            ("headline", "TEXT DEFAULT ''"),
            ("location", "TEXT DEFAULT ''"),
        ] {
            let exists: bool = conn
                .prepare("SELECT 1 FROM pragma_table_info('metadata_presets') WHERE name = ?1")
                .and_then(|mut s| s.query_row([col], |_| Ok(())))
                .is_ok();
            if !exists {
                conn.execute_batch(&format!(
                    "ALTER TABLE metadata_presets ADD COLUMN {col} {ddl}"
                ))
                .map_err(|e| e.to_string())?;
            }
        }
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS keyword_nodes(
               path TEXT PRIMARY KEY,
               include INTEGER NOT NULL DEFAULT 1
             );
             CREATE TABLE IF NOT EXISTS keyword_synonyms(
               path TEXT NOT NULL, synonym TEXT NOT NULL,
               PRIMARY KEY(path, synonym)
             );
             CREATE TABLE IF NOT EXISTS keyword_sets(
               name TEXT PRIMARY KEY, paths TEXT NOT NULL DEFAULT ''
             );",
        )
        .map_err(|e| format!("keyword hierarchy: {e}"))
    }

    /// V28: named export presets (settings JSON in `body`, folder
    /// grouping for the dialog). ADD-only: existing catalogs gain the
    /// empty table, presets travel with the catalog file.
    fn migrate_v3(conn: &Connection) -> Result<(), String> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS export_presets(
               name TEXT PRIMARY KEY,
               folder TEXT NOT NULL DEFAULT '',
               body TEXT NOT NULL DEFAULT ''
             );",
        )
        .map_err(|e| format!("export presets: {e}"))
    }

    /// Apple Photos sync: which media item each photo became, and photos
    /// added to the sync by hand regardless of scope.
    fn migrate_v5(conn: &Connection) -> Result<(), String> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS photos_links(
               photo_id INTEGER PRIMARY KEY,
               item_id TEXT NOT NULL,
               meta_hash TEXT NOT NULL DEFAULT '',
               album TEXT NOT NULL DEFAULT '',
               synced_at TEXT NOT NULL DEFAULT ''
             );
             CREATE TABLE IF NOT EXISTS photos_include(photo_id INTEGER PRIMARY KEY);",
        )
        .map_err(|e| format!("apple photos links: {e}"))
    }

    /// Apple Photos → Laika: mirrored folder/album tree, album membership
    /// by media item, and where each link came from.
    fn migrate_v6(conn: &Connection) -> Result<(), String> {
        let has_origin = conn
            .prepare("SELECT 1 FROM pragma_table_info('photos_links') WHERE name = 'origin'")
            .and_then(|mut s| s.query_row([], |_| Ok(())))
            .is_ok();
        if !has_origin {
            conn.execute_batch(
                "ALTER TABLE photos_links ADD COLUMN origin TEXT NOT NULL DEFAULT 'laika'",
            )
            .map_err(|e| format!("photos link origin: {e}"))?;
        }
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS photos_containers(
               id TEXT PRIMARY KEY,
               parent TEXT NOT NULL DEFAULT '',
               name TEXT NOT NULL,
               kind TEXT NOT NULL,
               position INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS photos_album_items(
               album_id TEXT NOT NULL,
               item_id TEXT NOT NULL,
               position INTEGER NOT NULL
             );
             CREATE INDEX IF NOT EXISTS photos_album_items_album ON photos_album_items(album_id);
             CREATE INDEX IF NOT EXISTS photos_links_item ON photos_links(item_id);",
        )
        .map_err(|e| format!("photos albums: {e}"))
    }

    pub const MIGRATIONS: &[Migration] = &[
        Migration {
            version: 1,
            name: "baseline tables, photo columns",
            apply: migrate_v1,
        },
        Migration {
            version: 2,
            name: "primary columns, query indexes",
            apply: migrate_v2,
        },
        Migration {
            version: 3,
            name: "export presets table",
            apply: migrate_v3,
        },
        Migration {
            version: 4,
            name: "descriptive metadata, EXIF overflow, keyword hierarchy",
            apply: migrate_v4,
        },
        Migration {
            version: 5,
            name: "apple photos links",
            apply: migrate_v5,
        },
        Migration {
            version: 6,
            name: "apple photos albums",
            apply: migrate_v6,
        },
    ];

    /// Highest schema this build opens. Bump with every `MIGRATIONS` entry.
    pub const APP_SCHEMA_VERSION: u32 = 6;

    /// Schema version of an open db (0 = pre-versioning prototype era).
    pub fn read_version(conn: &Connection, db_path: &std::path::Path) -> Result<u32, String> {
        conn.query_row("SELECT count(*) FROM sqlite_master", [], |r| {
            r.get::<_, i64>(0)
        })
        .map_err(|_| format!("not a SQLite database: {}", db_path.display()))?;
        conn.query_row("SELECT version FROM schema_version LIMIT 1", [], |r| {
            r.get(0)
        })
        .or(Ok(0))
    }

    /// Run every migration newer than `from`, one transaction each.
    /// Returns the new version. Testable with custom slices.
    pub fn run_migrations(
        conn: &mut Connection,
        from: u32,
        migrations: &[Migration],
    ) -> Result<u32, String> {
        let mut version = from;
        for m in migrations.iter().filter(|m| m.version > from) {
            let tx = conn
                .transaction()
                .map_err(|e| format!("migration {} ({}): {e}", m.version, m.name))?;
            if let Err(e) = (m.apply)(&tx) {
                // Transaction drops uncommitted: the file stays intact.
                return Err(format!("migration {} ({}): {e}", m.version, m.name));
            }
            tx.commit()
                .map_err(|e| format!("migration {} ({}): {e}", m.version, m.name))?;
            version = m.version;
        }
        Ok(version)
    }

    /// Stamp the current version after migrating.
    pub fn stamp_version(conn: &Connection, version: u32) -> Result<(), String> {
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS schema_version(version INTEGER);
             DELETE FROM schema_version;",
        )
        .map_err(|e| format!("stamp schema version: {e}"))?;
        conn.execute("INSERT INTO schema_version(version) VALUES (?1)", [version])
            .map(|_| ())
            .map_err(|e| format!("stamp schema version: {e}"))
    }
}

/// V08: structured integrity report — page verdict, per-table
/// orphans, and preview cache dirs without photos.
#[derive(Clone, Debug, Default)]
pub struct IntegrityReport {
    /// First `integrity_check` row ("ok" or the damage).
    pub pages: String,
    pub orphan_edits: i64,
    pub orphan_keywords: i64,
    pub orphan_sidecar_state: i64,
    pub orphan_sync_queue: i64,
    pub orphan_snapshots: i64,
    /// Preview cache dirs without photos (capped sample).
    pub orphan_previews: Vec<String>,
    /// One-line UI summary.
    pub line: String,
}

impl IntegrityReport {
    pub fn orphans(&self) -> i64 {
        self.orphan_edits
            + self.orphan_keywords
            + self.orphan_sidecar_state
            + self.orphan_sync_queue
            + self.orphan_snapshots
    }

    pub fn damaged(&self) -> bool {
        self.pages != "ok"
    }
}

/// V09: cached 1:1 preview key (sidecar JSON next to `preview-11.jpg`).
/// Matches only when tone, geometry, split, and dims all agree — any
/// edit invalidates, never shows stale pixels as native detail.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct DetailKey {
    pub values: Vec<f32>,
    pub rect: [f32; 4],
    pub angle: f32,
    pub flip_h: bool,
    pub flip_v: bool,
    /// V15: absent in old rows → unrotated (rotation never shows stale).
    #[serde(default)]
    pub rotation: u8,
    pub split: f32,
    pub w: u32,
    pub h: u32,
    /// V22: perspective warp; absent in old rows → none (identity).
    #[serde(default)]
    pub warp: Vec<f32>,
}

impl DetailKey {
    /// V22: the stored warp matches (empty = identity).
    pub fn warp_matches(&self, warp: &[f32; 9]) -> bool {
        if self.warp.is_empty() {
            *warp == crate::edit::WARP_IDENTITY
        } else {
            self.warp.as_slice() == warp.as_slice()
        }
    }
}

/// V09: does a cached 1:1 key match the live render state?
pub fn detail_key_matches(
    key: &DetailKey,
    values: &[f32],
    rect: [f32; 4],
    angle: f32,
    flip_h: bool,
    flip_v: bool,
    rotation: u8,
    split: f32,
    w: u32,
    h: u32,
) -> bool {
    key.values == values
        && key.rect == rect
        && key.angle == angle
        && key.flip_h == flip_h
        && key.flip_v == flip_v
        && key.rotation == rotation
        && key.split == split
        && key.w == w
        && key.h == h
}

/// V09: cache audit outcome (one directory walk).
#[derive(Clone, Debug, Default)]
pub struct CacheAudit {
    pub bytes: u64,
    pub files: usize,
    /// 1:1 previews evicted (expired or oldest-first over cap).
    pub evicted: Vec<String>,
}

/// V09: audit the preview cache against a cap with TTL eviction.
/// One walk; evicts expired 1:1s, then oldest 1:1s while over cap.
/// Never touches 512/2048 derivatives or smart-preview linears of
/// offline originals (offline editing depends on them); the cap
/// governs the evictable class, so protected files may exceed it.
/// `known` = referenced hashes, `offline` = hashes whose originals
/// are missing, `now_secs` = unix time for TTL math.
pub fn audit_cache(
    cache_dir: &Path,
    known: &std::collections::HashSet<String>,
    offline: &std::collections::HashSet<String>,
    cap_bytes: u64,
    ttl_days: u64,
    now_secs: u64,
) -> CacheAudit {
    struct Entry {
        hash: String,
        bytes: u64,
        mtime: u64,
    }
    let mut ones: Vec<Entry> = Vec::new();
    let mut audit = CacheAudit::default();
    let rd = std::fs::read_dir(cache_dir)
        .map(|rd| rd.filter_map(|e| e.ok()).collect::<Vec<_>>())
        .unwrap_or_default();
    for e in rd {
        let name = e.file_name().to_string_lossy().to_string();
        // Top-level strays count toward usage (never evicted).
        if !e.path().is_dir() {
            if let Ok(m) = e.metadata() {
                audit.files += 1;
                audit.bytes += m.len();
            }
            continue;
        }
        if name.len() != 64 || !name.bytes().all(|b| b.is_ascii_hexdigit()) {
            // Non-hash dirs still count toward usage (never evicted).
            if let Ok(files) = std::fs::read_dir(e.path()) {
                for f in files.filter_map(|f| f.ok()) {
                    if let Ok(m) = f.metadata() {
                        audit.files += 1;
                        audit.bytes += m.len();
                    }
                }
            }
            continue;
        }
        let dir = e.path();
        if let Ok(files) = std::fs::read_dir(&dir) {
            for f in files.filter_map(|f| f.ok()) {
                if let Ok(m) = f.metadata() {
                    audit.files += 1;
                    audit.bytes += m.len();
                }
            }
        }
        // Evictable: a 1:1 of a known, online photo. Track its own
        // bytes (re-walked below via metadata for exactness).
        if known.contains(&name) && !offline.contains(&name) {
            let p = dir.join("preview-11.jpg");
            if let Ok(m) = std::fs::metadata(&p) {
                let mt = m
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                ones.push(Entry {
                    hash: name,
                    bytes: m.len(),
                    mtime: mt,
                });
            }
        }
    }
    // Expired first (any age past TTL), then oldest while over cap.
    let ttl_secs = ttl_days.saturating_mul(86_400);
    let mut evict: Vec<Entry> = Vec::new();
    let mut kept: Vec<Entry> = Vec::new();
    for e in ones {
        if ttl_secs > 0 && now_secs.saturating_sub(e.mtime) > ttl_secs {
            evict.push(e);
        } else {
            kept.push(e);
        }
    }
    kept.sort_by_key(|e| e.mtime);
    // Expired evictions already free their bytes: only the remainder over
    // the cap may cost fresh (unexpired) previews.
    let expired_bytes: u64 = evict.iter().map(|e| e.bytes).sum();
    let mut over = audit
        .bytes
        .saturating_sub(expired_bytes)
        .saturating_sub(cap_bytes);
    let mut idx = 0;
    while over > 0 && idx < kept.len() {
        over = over.saturating_sub(kept[idx].bytes);
        let e = Entry {
            hash: std::mem::take(&mut kept[idx].hash),
            bytes: kept[idx].bytes,
            mtime: kept[idx].mtime,
        };
        evict.push(e);
        idx += 1;
    }
    for e in &evict {
        let p = cache_dir.join(&e.hash).join("preview-11.jpg");
        if std::fs::remove_file(&p).is_ok() {
            // Drop the stale key beside it (best effort).
            std::fs::remove_file(cache_dir.join(&e.hash).join("preview-11.json")).ok();
            audit.bytes = audit.bytes.saturating_sub(e.bytes);
            audit.evicted.push(e.hash.clone());
        }
    }
    audit
}

/// V08: optimize outcome for the UI.
#[derive(Clone, Copy, Debug, Default)]
pub struct OptimizeReport {
    pub before: u64,
    pub after: u64,
    /// Signed (negative = the file grew, e.g. fresh ANALYZE stats).
    pub reclaimed: i64,
}
impl Catalog {
    pub fn open(db_path: &Path, name: &str, root: &Path) -> Result<Self, String> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        // V08: refuse non-databases with a clear message (a lazy open
        // succeeds on garbage; the magic check does not lie).
        if db_path.exists() && !is_sqlite_file(db_path) {
            return Err(format!("not a SQLite database: {}", db_path.display()));
        }
        let mut conn = Connection::open(db_path).map_err(|e| e.to_string())?;
        // V08: foreign-key enforcement for future constrained tables
        // (the current schema predates FK clauses, so today this guards
        // nothing — orphan checks in `integrity_check` enforce instead).
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000; PRAGMA foreign_keys=ON;",
        )
        .map_err(|e| e.to_string())?;
        // V08: version gate before touching anything. Garbage files
        // refuse here (lazy open succeeds; the first query does not).
        let from = migrations::read_version(&conn, db_path)?;
        if from > migrations::APP_SCHEMA_VERSION {
            return Err(format!(
                "this catalog needs schema v{from}, but this Laika (v{}) opens up to v{} — update Laika to open it",
                env!("CARGO_PKG_VERSION"),
                migrations::APP_SCHEMA_VERSION,
            ));
        }
        if from < migrations::APP_SCHEMA_VERSION {
            // Fresh files (no tables yet) need no backup of nothing.
            let tables: i64 = conn
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE type='table'",
                    [],
                    |r| r.get(0),
                )
                .unwrap_or(0);
            if tables > 0 {
                // Automatic pre-migration backup (same filesystem, fast).
                // A failed step reports itself; the backup stays regardless.
                let backup_dir = db_path
                    .parent()
                    .map(|d| d.join("backups"))
                    .unwrap_or_else(|| PathBuf::from("backups"));
                std::fs::create_dir_all(&backup_dir).map_err(|e| format!("backup dir: {e}"))?;
                conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);").ok();
                let stamp = chrono_stamp();
                let backup = backup_dir.join(format!("laika-pre-v{from}-{stamp}.db"));
                std::fs::copy(db_path, &backup)
                    .map_err(|e| format!("pre-migration backup: {e}"))?;
                eprintln!("[catalog] pre-migration backup at {}", backup.display());
            }
            let new_version = migrations::run_migrations(&mut conn, from, migrations::MIGRATIONS)?;
            migrations::stamp_version(&conn, new_version)?;
            eprintln!("[catalog] migrated v{from} → v{new_version}");
        }
        let root_str = root.to_string_lossy().to_string();
        conn.execute(
            "INSERT INTO catalogs(name, root_path) SELECT ?1, ?2 WHERE NOT EXISTS (SELECT 1 FROM catalogs)",
            params![name, root_str],
        )
        .map_err(|e| e.to_string())?;
        let id: i64 = conn
            .query_row("SELECT id FROM catalogs LIMIT 1", [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        let mut this = Self {
            conn,
            id,
            db_path: db_path.to_path_buf(),
        };
        // V07: a catalog folder moved with its photos keeps resolving — if
        // the stored root serves nothing but the db's own directory does,
        // adopt the directory as the new root.
        this.maybe_adopt_root();
        Ok(this)
    }

    /// Filesystem path of the live catalog file (backups, size display).
    pub fn db_path(&self) -> &Path {
        &self.db_path
    }

    /// Display name stored in the catalog row.
    pub fn stored_name(&self) -> String {
        self.conn
            .query_row("SELECT name FROM catalogs LIMIT 1", [], |r| r.get(0))
            .unwrap_or_else(|_| "Laika".to_string())
    }

    /// Stored form of an absolute path: relative when under the catalog
    /// root (portable bundles), absolute otherwise (Lightroom-style).
    fn stored_form(&self, abs: &Path) -> String {
        let root = self.root_path();
        if root.is_empty() {
            return abs.to_string_lossy().to_string();
        }
        match abs.strip_prefix(&root) {
            Ok(rel) if !rel.as_os_str().is_empty() => rel.to_string_lossy().to_string(),
            _ => abs.to_string_lossy().to_string(),
        }
    }

    /// Resolve a stored path back to absolute.
    fn resolve_stored(&self, stored: &str) -> String {
        let p = Path::new(stored);
        if p.is_absolute() {
            return stored.to_string();
        }
        let root = self.root_path();
        if root.is_empty() {
            return stored.to_string();
        }
        Path::new(&root).join(p).to_string_lossy().to_string()
    }

    /// Adopt the db's directory as root when the stored root resolves
    /// nothing but the directory does (moved catalog folder).
    fn maybe_adopt_root(&mut self) {
        let stored_root: String = self.root_path();
        let rows: Vec<String> = self
            .conn
            .prepare("SELECT path FROM photos WHERE catalog_id = ?1 LIMIT 200")
            .and_then(|mut s| {
                s.query_map([self.id], |r| r.get(0))?
                    .collect::<Result<Vec<_>, _>>()
            })
            .unwrap_or_default();
        if rows.is_empty() {
            return;
        }
        let resolves = |root: &str, limit: usize| -> usize {
            rows.iter()
                .take(limit)
                .filter(|p| {
                    let abs = if Path::new(p).is_absolute() {
                        (*p).clone()
                    } else if root.is_empty() {
                        return false;
                    } else {
                        Path::new(root).join(p).to_string_lossy().to_string()
                    };
                    Path::new(&abs).exists()
                })
                .count()
        };
        if resolves(&stored_root, 200) > 0 {
            return;
        }
        let dir = self
            .db_path
            .parent()
            .map(|d| d.to_string_lossy().to_string())
            .unwrap_or_default();
        if !dir.is_empty() && resolves(&dir, 200) > 0 {
            eprintln!("[catalog] root moved {stored_root} → {dir}; adopting");
            self.conn
                .execute("UPDATE catalogs SET root_path = ?1", [dir])
                .ok();
        }
    }

    pub fn photo_count(&self) -> usize {
        self.conn
            .query_row(
                "SELECT COUNT(*) FROM photos WHERE catalog_id = ?1",
                [self.id],
                |r| r.get::<_, i64>(0),
            )
            .unwrap_or(0) as usize
    }

    pub fn already_imported(&self, path: &str) -> bool {
        if self
            .conn
            .query_row("SELECT 1 FROM photos WHERE path = ?1", [path], |_| Ok(()))
            .is_ok()
        {
            return true;
        }
        // V07: rows may store the relative form of an absolute path.
        let stored = self.stored_form(Path::new(path));
        stored != path
            && self
                .conn
                .query_row("SELECT 1 FROM photos WHERE path = ?1", [stored], |_| Ok(()))
                .is_ok()
    }

    /// Insert one file: hash → EXIF → previews → cache → insert.
    /// Returns the row id, or `Ok(None)` if already imported.
    pub fn import_file(&self, path: &Path, cache_dir: &Path) -> Result<Option<i64>, String> {
        // V07: portable rows store root-relative paths when possible.
        let path_str = self.stored_form(path);
        if self.already_imported(&path.to_string_lossy()) {
            return Ok(None);
        }
        let hash = hash_file(path)?;
        // V05: videos never touch the still preview/EXIF path.
        if laika_raw::media_kind(path) == Some(laika_raw::MediaKind::Video) {
            let prepared = prepare_video_file(path, &hash)?;
            return self.insert_prepared(&prepared, cache_dir);
        }
        let meta = laika_raw::exif::read(path);
        let img = laika_raw::preview::load_preview(path)?;
        let small = laika_raw::preview::derivative_jpeg(&img, laika_raw::preview::PREVIEW_SMALL);
        let large = laika_raw::preview::derivative_jpeg(&img, laika_raw::preview::PREVIEW_LARGE);
        let dir = cache_dir.join(&hash);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        std::fs::write(dir.join("preview-512.jpg"), &small).map_err(|e| e.to_string())?;
        std::fs::write(dir.join("preview-2048.jpg"), &large).map_err(|e| e.to_string())?;

        let filename = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        let now = chrono_stamp();
        self.conn
            .execute(
                "INSERT INTO photos(catalog_id, path, filename, blake3, captured_at, camera, lens,
                 focal_mm, aperture, shutter, iso, width, height, sync_state, imported_at,
                 duration_ms, codec, exif_program, exif_metering, exif_flash, exif_focal35,
                 exif_serial, exif_firmware, exif_gps)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,'local',?14,?15,?16,
                 ?17,?18,?19,?20,?21,?22,?23)",
                params![
                    self.id,
                    path_str,
                    filename,
                    hash,
                    meta.captured_at,
                    meta.camera,
                    meta.lens,
                    meta.focal_mm,
                    meta.aperture,
                    meta.shutter,
                    meta.iso,
                    meta.width,
                    meta.height,
                    now,
                    meta.duration_ms as i64,
                    meta.codec.clone(),
                    meta.exposure_program,
                    meta.metering_mode,
                    meta.flash,
                    meta.focal_35mm,
                    meta.serial,
                    meta.firmware,
                    meta.gps,
                ],
            )
            .map_err(|e| e.to_string())?;
        Ok(Some(self.conn.last_insert_rowid()))
    }

    pub fn all_photos(&self) -> Vec<DbPhoto> {
        let mut stmt = self.conn.prepare(
            "SELECT id, catalog_id, path, filename, blake3, captured_at, camera, lens, focal_mm,
             aperture, shutter, iso, width, height, rating, picked, rejected, sync_state, remote_key,
             creator, copyright, rights, contact, captured_orig, capture_offset_min,
             duration_ms, codec, title, caption, headline, location,
             exif_program, exif_metering, exif_flash, exif_focal35,
             exif_serial, exif_firmware, exif_gps
             FROM photos WHERE catalog_id = ?1 ORDER BY captured_at, filename",
        );
        let Ok(stmt) = stmt.as_mut() else {
            return Vec::new();
        };
        let rows = stmt.query_map([self.id], row_to_photo);
        let Ok(rows) = rows else { return Vec::new() };
        // V07: rows may store root-relative paths; the app always sees
        // absolute ones, so every path consumer keeps working unchanged.
        let root = self.root_path();
        rows.flatten()
            .map(|mut p| {
                let stored = Path::new(&p.path);
                if !stored.is_absolute() && !root.is_empty() {
                    p.path = Path::new(&root).join(stored).to_string_lossy().to_string();
                }
                p
            })
            .collect()
    }

    /// Backfill frame dims for rows imported before RAW dims were read
    /// (0×0). Crop, aspect locks and the Develop stage measure against them.
    pub fn set_dims(&self, id: i64, width: u32, height: u32) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE photos SET width = ?1, height = ?2 WHERE id = ?3",
                params![width, height, id],
            )
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// Fetch one catalog row for an incremental UI update. Background
    /// imports use this so a newly processed photo can appear immediately
    /// without re-querying and rebuilding the entire library on every file.
    pub fn photo_by_id(&self, id: i64) -> Option<DbPhoto> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT id, catalog_id, path, filename, blake3, captured_at, camera, lens, focal_mm,
                 aperture, shutter, iso, width, height, rating, picked, rejected, sync_state, remote_key,
                 creator, copyright, rights, contact, captured_orig, capture_offset_min,
                 duration_ms, codec, title, caption, headline, location,
                 exif_program, exif_metering, exif_flash, exif_focal35,
                 exif_serial, exif_firmware, exif_gps
                 FROM photos WHERE catalog_id = ?1 AND id = ?2",
            )
            .ok()?;
        let mut photo = stmt.query_row(params![self.id, id], row_to_photo).ok()?;
        // V07: expose an absolute path even when the movable catalog stores
        // originals relative to its root.
        let stored = Path::new(&photo.path);
        let root = self.root_path();
        if !stored.is_absolute() && !root.is_empty() {
            photo.path = Path::new(&root).join(stored).to_string_lossy().to_string();
        }
        Some(photo)
    }

    /// U02: catalog write failures are returned, never discarded — the UI
    /// turns them into an explicit save-error state with retry.
    pub fn set_rating(&self, id: i64, rating: u8) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE photos SET rating = ?1 WHERE id = ?2",
                params![rating, id],
            )
            .map(|_| ())
            .map_err(|e| format!("save rating: {e}"))
    }

    pub fn set_flag(&self, id: i64, picked: bool, rejected: bool) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE photos SET picked = ?1, rejected = ?2 WHERE id = ?3",
                params![picked as u8, rejected as u8, id],
            )
            .map(|_| ())
            .map_err(|e| format!("save flag: {e}"))
    }

    pub fn set_sync(&self, id: i64, state: SyncState) {
        let s = match state {
            SyncState::Local => "local",
            SyncState::Pending => "pending",
            SyncState::Synced => "synced",
            SyncState::Failed => "failed",
        };
        self.conn
            .execute(
                "UPDATE photos SET sync_state = ?1 WHERE id = ?2",
                params![s, id],
            )
            .ok();
    }

    /// Insert a file prepared on a background thread (previews already
    /// rendered). Writes the derivative cache, then the row.
    pub fn insert_prepared(
        &self,
        f: &PreparedFile,
        cache_dir: &Path,
    ) -> Result<Option<i64>, String> {
        if self.already_imported(&f.path_str) {
            return Ok(None);
        }
        let stored_path = self.stored_form(Path::new(&f.path_str));
        let dir = cache_dir.join(&f.hash);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        // V05: videos without a poster write no derivatives; the UI falls
        // back to its placeholder tile (never a fake or zero-byte file).
        if !f.small_jpeg.is_empty() {
            std::fs::write(dir.join("preview-512.jpg"), &f.small_jpeg)
                .map_err(|e| e.to_string())?;
        }
        if !f.large_jpeg.is_empty() {
            std::fs::write(dir.join("preview-2048.jpg"), &f.large_jpeg)
                .map_err(|e| e.to_string())?;
        }

        let filename = Path::new(&f.path_str)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        let now = chrono_stamp();
        let m = &f.meta;
        self.conn
            .execute(
                "INSERT INTO photos(catalog_id, path, filename, blake3, captured_at, camera, lens,
                 focal_mm, aperture, shutter, iso, width, height, sync_state, imported_at,
                 duration_ms, codec, exif_program, exif_metering, exif_flash, exif_focal35,
                 exif_serial, exif_firmware, exif_gps)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,'local',?14,?15,?16,
                 ?17,?18,?19,?20,?21,?22,?23)",
                params![
                    self.id,
                    stored_path,
                    filename,
                    f.hash,
                    m.captured_at,
                    m.camera,
                    m.lens,
                    m.focal_mm,
                    m.aperture,
                    m.shutter,
                    m.iso,
                    m.width,
                    m.height,
                    now,
                    m.duration_ms as i64,
                    m.codec.clone(),
                    m.exposure_program,
                    m.metering_mode,
                    m.flash,
                    m.focal_35mm,
                    m.serial,
                    m.firmware,
                    m.gps,
                ],
            )
            .map_err(|e| e.to_string())?;
        Ok(Some(self.conn.last_insert_rowid()))
    }

    /// Persist one photo's full edit state: params, crop, durable history
    /// and cursor (U14). History beyond the cap is trimmed oldest-first
    /// with the baseline pinned (see `Edit::push_snap`).
    pub fn save_params(&self, photo_id: i64, edit: &crate::edit::Edit) -> Result<(), String> {
        // Non-finite values serialize as JSON null, which fails to read back
        // and silently drops the whole row on the next load.
        let defaults = crate::edit::defaults();
        let json = serde_json::to_string(&EditJson {
            params: edit
                .params
                .iter()
                .zip(defaults)
                .map(|(v, d)| if v.is_finite() { *v } else { d })
                .collect(),
            crop: edit.crop,
            geom: edit.geom,
            curve_on: edit.curve_on,
            hsl_on: edit.hsl_on,
            detail_on: edit.detail_on,
            optics_on: edit.optics_on,
            effects_on: edit.effects_on,
            grading_on: edit.grading_on,
        })
        .unwrap_or_default();
        let history = crate::edit::encode_history(&edit.history);
        let now = chrono_stamp();
        self.conn
            .execute(
                "INSERT INTO edits(photo_id, params_json, history_json, cursor, updated_at)
                 VALUES (?1,?2,?3,?4,?5)
                 ON CONFLICT(photo_id) DO UPDATE SET params_json = ?2, history_json = ?3,
                   cursor = ?4, updated_at = ?5",
                params![photo_id, json, history, edit.cursor as i64, now],
            )
            .map(|_| ())
            .map_err(|e| format!("save edits: {e}"))
    }

    /// U14: load full edit states (params, crop, durable history, cursor).
    /// Legacy rows (bare-array params, empty history) still read.
    pub fn load_all_edits(&self) -> Vec<(i64, crate::edit::Edit)> {
        let mut out = Vec::new();
        let mut stmt = match self
            .conn
            .prepare("SELECT photo_id, params_json, history_json, cursor FROM edits")
        {
            Ok(s) => s,
            Err(_) => return out,
        };
        let rows = match stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                r.get::<_, Option<i64>>(3)?.unwrap_or(0),
            ))
        }) {
            Ok(r) => r,
            Err(_) => return out,
        };
        for (pid, pjson, hjson, cursor) in rows.flatten() {
            // Current object form, falling back to the Phase 2 bare array.
            let (params, crop, stored) = if let Ok(e) = serde_json::from_str::<EditJson>(&pjson) {
                let stored = (
                    e.geom,
                    e.curve_on,
                    e.hsl_on,
                    e.detail_on,
                    e.optics_on,
                    e.effects_on,
                    e.grading_on,
                );
                (e.params, e.crop, Some(stored))
            } else if let Ok(params) = serde_json::from_str::<Vec<f32>>(&pjson) {
                (params, None, None)
            } else {
                continue;
            };
            let Some(arr) = crate::edit::pad_params(&params) else {
                continue;
            };
            let mut history = crate::edit::decode_history(&hjson);
            // Clamp the cursor into the loaded history (never past the tip).
            let cursor = (cursor.max(0) as usize).min(history.len());
            // U08/U18: live geometry + panel flags follow the history
            // tip (undo position), falling back to stored columns.
            let tip = history.get(cursor.saturating_sub(1));
            // (History-less rows — e.g. an adopted sidecar — keep the
            // geometry and flags stored in params_json.)
            let live_geom = tip
                .map(|st: &crate::edit::HistoryStep| st.geom)
                .or(stored.map(|s| s.0))
                .unwrap_or_default();
            let (curve_on, hsl_on, detail_on, optics_on, effects_on, grading_on) = tip
                .map(|st| {
                    (
                        st.curve_on,
                        st.hsl_on,
                        st.detail_on,
                        st.optics_on,
                        st.effects_on,
                        st.grading_on,
                    )
                })
                .or(stored.map(|s| (s.1, s.2, s.3, s.4, s.5, s.6)))
                .unwrap_or((true, true, true, true, true, true));
            out.push((
                pid,
                crate::edit::Edit {
                    params: arr,
                    history,
                    cursor,
                    crop,
                    geom: live_geom,
                    curve_on,
                    hsl_on,
                    detail_on,
                    optics_on,
                    effects_on,
                    grading_on,
                },
            ));
        }
        out
    }

    pub fn load_all_params(&self) -> Vec<(i64, Vec<f32>, Option<f32>)> {
        self.load_all_edits()
            .into_iter()
            .map(|(pid, e)| (pid, e.params.to_vec(), e.crop))
            .collect()
    }

    /// Outcome of reconciling one sidecar against the catalog (U02).
    /// Stale sidecars never overwrite newer catalog values; external edits
    /// are detected and reported instead of silently winning or losing.
    pub fn apply_sidecar(&self, id: i64, path: &str) -> Result<SidecarApply, String> {
        let side_path = crate::xmp::sidecar_path(path);
        let mtime = crate::xmp::sidecar_mtime(path);
        let Some(side) = crate::xmp::read(path) else {
            if std::path::Path::new(&side_path).exists() {
                return Ok(SidecarApply::Corrupt);
            }
            return Ok(SidecarApply::Missing);
        };
        let mtime = mtime.unwrap_or(0);
        let db_updated = self.edits_updated_at(id);
        let written = self.sidecar_written_at(id);
        // No local edits: adopt the sidecar (fresh import or first sight).
        let take_sidecar = match (db_updated, written) {
            (None, _) => true,
            (Some(_), Some(w)) => mtime > w + 1,
            (Some(updated), None) => mtime + 2 >= updated,
        };
        if !take_sidecar {
            return Ok(SidecarApply::Stale);
        }
        let external = matches!((db_updated, written), (Some(_), Some(w)) if mtime > w + 1);
        if side.has_tone {
            // Adopted sidecars start a fresh in-memory lineage; the rescan
            // report tells the user they came from outside. Rating-only
            // foreign rewrites leave catalog tone alone (U18).
            let arr = side.params;
            let geom = side.geom.unwrap_or_else(|| self.geom_of(id));
            self.save_params(
                id,
                &crate::edit::Edit {
                    params: arr,
                    history: Vec::new(),
                    cursor: 0,
                    crop: self.crop_of(id),
                    geom,
                    curve_on: true,
                    hsl_on: true,
                    detail_on: true,
                    optics_on: true,
                    effects_on: true,
                    grading_on: true,
                },
            )?;
        }
        if let Some(rating) = side.rating {
            self.set_rating(id, rating)?;
        }
        // Adopt carried authorship field-by-field: a sidecar that only
        // sets the creator must not blank catalog copyright.
        let cur = self.photo_authorship(id);
        // V12: adopt carried descriptive fields field-by-field (a sidecar
        // that only sets the caption must not blank catalog copyright).
        // Note the asymmetry: `cur` holds included keywords only, so a
        // sidecar keyword list replaces the full assigned set.
        let pick = |side_v: &str, cur_v: &str| {
            if side_v.is_empty() {
                cur_v.to_string()
            } else {
                side_v.to_string()
            }
        };
        let merged = crate::xmp::Authorship {
            title: pick(&side.title, &cur.title),
            caption: pick(&side.caption, &cur.caption),
            headline: pick(&side.headline, &cur.headline),
            creator: pick(&side.creator, &cur.creator),
            copyright: pick(&side.copyright, &cur.copyright),
            rights_usage: pick(&side.rights_usage, &cur.rights_usage),
            contact: pick(&side.contact, &cur.contact),
            location: pick(&side.location, &cur.location),
            keywords: if side.keywords.is_empty() {
                self.photo_keywords(id)
            } else {
                side.keywords.clone()
            },
        };
        let got_auth = !side.title.is_empty()
            || !side.caption.is_empty()
            || !side.headline.is_empty()
            || !side.creator.is_empty()
            || !side.copyright.is_empty()
            || !side.rights_usage.is_empty()
            || !side.contact.is_empty()
            || !side.location.is_empty()
            || !side.keywords.is_empty();
        if got_auth && merged != cur {
            self.set_photo_meta(
                id,
                &PhotoMeta {
                    title: merged.title.clone(),
                    caption: merged.caption.clone(),
                    headline: merged.headline.clone(),
                    creator: merged.creator.clone(),
                    copyright: merged.copyright.clone(),
                    rights: merged.rights_usage.clone(),
                    contact: merged.contact.clone(),
                    location: merged.location.clone(),
                },
            )?;
            self.set_keywords(id, &merged.keywords)?;
        }
        self.remember_sidecar_write(id, mtime)?;
        Ok(if external {
            SidecarApply::AppliedExternal
        } else {
            SidecarApply::Applied
        })
    }

    /// Heals a stale sidecar by rewriting it from the acknowledged catalog
    /// values (crash-recovery convergence). The original is never touched.
    pub fn heal_sidecar(
        &self,
        id: i64,
        path: &str,
        params: &[f32; crate::edit::PARAM_COUNT],
        rating: u8,
        history: &[String],
        preset: Option<&str>,
    ) -> Result<(), String> {
        // The heal rewrites acknowledged values; authorship and geometry
        // ride along so neither is wiped by convergence.
        let auth = self.photo_authorship(id);
        let geom = self.geom_of(id);
        crate::xmp::write(path, params, rating, history, preset, &auth, &geom)?;
        let mtime = crate::xmp::sidecar_mtime(path).unwrap_or(0);
        self.remember_sidecar_write(id, mtime)
    }

    /// Authorship block for one photo (sidecar rewrites + panel
    /// display). V12: full descriptive fields, export-included keywords.
    pub fn photo_authorship(&self, id: i64) -> crate::xmp::Authorship {
        let meta = self.photo_meta(id);
        crate::xmp::Authorship {
            title: meta.title,
            caption: meta.caption,
            headline: meta.headline,
            creator: meta.creator,
            copyright: meta.copyright,
            rights_usage: meta.rights,
            contact: meta.contact,
            location: meta.location,
            keywords: self.export_keyword_paths(id),
        }
    }

    /// Epoch secs of the last acknowledged params save, if any.
    pub fn edits_updated_at(&self, id: i64) -> Option<i64> {
        self.conn
            .query_row(
                "SELECT updated_at FROM edits WHERE photo_id = ?1",
                [id],
                |r| r.get::<_, String>(0),
            )
            .ok()?
            .parse::<i64>()
            .ok()
    }

    /// Record the mtime this app last wrote for a sidecar.
    pub fn remember_sidecar_write(&self, id: i64, mtime: i64) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT INTO sidecar_state(photo_id, written_mtime) VALUES (?1,?2)
                 ON CONFLICT(photo_id) DO UPDATE SET written_mtime = ?2",
                params![id, mtime.to_string()],
            )
            .map(|_| ())
            .map_err(|e| format!("save sidecar state: {e}"))
    }

    fn sidecar_written_at(&self, id: i64) -> Option<i64> {
        self.conn
            .query_row(
                "SELECT written_mtime FROM sidecar_state WHERE photo_id = ?1",
                [id],
                |r| r.get::<_, String>(0),
            )
            .ok()?
            .parse::<i64>()
            .ok()
    }

    fn crop_of(&self, id: i64) -> Option<f32> {
        self.conn
            .query_row(
                "SELECT params_json FROM edits WHERE photo_id = ?1",
                [id],
                |r| r.get::<_, String>(0),
            )
            .ok()
            .and_then(|j| serde_json::from_str::<EditJson>(&j).ok())
            .and_then(|e| e.crop)
    }

    /// U08: live geometry for one photo (history tip via `load_all_edits`
    /// semantics, legacy column fallback).
    pub fn geom_of(&self, id: i64) -> crate::edit::CropGeom {
        self.conn
            .query_row(
                "SELECT params_json FROM edits WHERE photo_id = ?1",
                [id],
                |r| r.get::<_, String>(0),
            )
            .ok()
            .and_then(|j| serde_json::from_str::<EditJson>(&j).ok())
            .map(|e| e.geom)
            .unwrap_or_default()
    }

    /// One pass over the catalog reconciling every sidecar found next to an
    /// original. Runs at startup and after import. Stale sidecars are healed
    /// from the catalog so acknowledged saves survive crashes.
    pub fn rescan_sidecars(&self) -> RescanReport {
        let mut report = RescanReport::default();
        let params_by_photo: std::collections::HashMap<i64, Vec<f32>> = self
            .load_all_params()
            .into_iter()
            .map(|(pid, v, _)| (pid, v))
            .collect();
        for p in self.all_photos() {
            match self.apply_sidecar(p.id, &p.path) {
                Ok(SidecarApply::Applied) => report.applied += 1,
                Ok(SidecarApply::AppliedExternal) => {
                    report.applied += 1;
                    report.external += 1;
                }
                Ok(SidecarApply::Stale) => {
                    // Heal only when the catalog holds params for this photo;
                    // otherwise there is nothing to converge. History rides
                    // along so the heal is lossless.
                    if let Some(v) = params_by_photo.get(&p.id) {
                        if let Some(arr) = crate::edit::pad_params(v) {
                            let old = crate::xmp::read(&p.path);
                            let history: Vec<String> =
                                old.as_ref().map(|s| s.history.clone()).unwrap_or_default();
                            let preset = old.as_ref().and_then(|s| s.preset.clone());
                            match self.heal_sidecar(
                                p.id,
                                &p.path,
                                &arr,
                                p.rating,
                                &history,
                                preset.as_deref(),
                            ) {
                                Ok(()) => report.healed += 1,
                                Err(e) => report.error = Some(e),
                            }
                        }
                    }
                }
                Ok(SidecarApply::Corrupt) => {
                    report.error = Some(format!(
                        "unreadable sidecar next to {} — catalog values kept",
                        p.filename
                    ));
                }
                Ok(SidecarApply::Missing) => {}
                Err(e) => report.error = Some(e),
            }
        }
        report
    }

    // ---- sync settings + queue --------------------------------------------------

    pub fn load_sync_settings(&self) -> crate::sync::SyncSettings {
        let get = |k: &str| -> String {
            self.conn
                .query_row("SELECT value FROM sync_settings WHERE key = ?1", [k], |r| {
                    r.get(0)
                })
                .unwrap_or_default()
        };
        let mut s = crate::sync::SyncSettings {
            endpoint: get("endpoint"),
            bucket: get("bucket"),
            region: if get("region").is_empty() {
                "us-east-1".into()
            } else {
                get("region")
            },
            access_key: get("access_key"),
            sftp_host: get("sftp_host"),
            sftp_port: get("sftp_port"),
            sftp_user: get("sftp_user"),
            sftp_path: get("sftp_path"),
            sftp_identity: get("sftp_identity"),
            share_kind: if get("share_kind") == "nfs" {
                crate::sync::ShareKind::Nfs
            } else {
                crate::sync::ShareKind::Smb
            },
            share_server: get("share_server"),
            share_name: get("share_name"),
            share_path: get("share_path"),
            ..Default::default()
        };
        // Catalogs from before target selection keep backing up to S3.
        s.target =
            crate::sync::BackupTarget::from_key(&get("target")).unwrap_or(if s.s3_configured() {
                crate::sync::BackupTarget::S3
            } else {
                crate::sync::BackupTarget::Sftp
            });
        s
    }

    pub fn save_sync_settings(&self, s: &crate::sync::SyncSettings) -> Result<(), String> {
        for (k, v) in [
            ("target", s.target.key()),
            ("endpoint", s.endpoint.as_str()),
            ("bucket", s.bucket.as_str()),
            ("region", s.region.as_str()),
            ("access_key", s.access_key.as_str()),
            ("sftp_host", s.sftp_host.as_str()),
            ("sftp_port", s.sftp_port.as_str()),
            ("sftp_user", s.sftp_user.as_str()),
            ("sftp_path", s.sftp_path.as_str()),
            ("sftp_identity", s.sftp_identity.as_str()),
            ("share_kind", s.share_kind.key()),
            ("share_server", s.share_server.as_str()),
            ("share_name", s.share_name.as_str()),
            ("share_path", s.share_path.as_str()),
        ] {
            self.conn
                .execute(
                    "INSERT INTO sync_settings(key, value) VALUES (?1,?2)
                     ON CONFLICT(key) DO UPDATE SET value = ?2",
                    rusqlite::params![k, v],
                )
                .map_err(|e| format!("save backup settings: {e}"))?;
        }
        Ok(())
    }

    /// U05/V01: content-hash duplicate check across renames and moves.
    pub fn hash_exists(&self, hash: &str) -> bool {
        self.conn
            .query_row("SELECT 1 FROM photos WHERE blake3 = ?1", [hash], |_| Ok(()))
            .is_ok()
    }

    /// Remember one verified import for per-card duplicate memory.
    pub fn record_card_import(&self, hash: &str, volume: &str) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT OR IGNORE INTO card_history(file_hash, volume, imported_at) VALUES (?1,?2,?3)",
                params![hash, volume, chrono_stamp()],
            )
            .map(|_| ())
            .map_err(|e| format!("record card import: {e}"))
    }

    /// True when this hash was already imported from this volume.
    pub fn card_has_hash(&self, hash: &str, volume: &str) -> bool {
        self.conn
            .query_row(
                "SELECT 1 FROM card_history WHERE file_hash = ?1 AND volume = ?2",
                params![hash, volume],
                |_| Ok(()),
            )
            .is_ok()
    }

    /// Journal one finished import run (diagnostics; best-effort caller).
    pub fn record_import_batch(
        &self,
        source: &str,
        volume: &str,
        mode: &str,
        started_at: &str,
        imported: usize,
        skipped_dup: usize,
        failed: usize,
    ) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT INTO import_batches(source, volume, mode, started_at, finished_at, imported, skipped_dup, failed)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",
                params![
                    source,
                    volume,
                    mode,
                    started_at,
                    chrono_stamp(),
                    imported as i64,
                    skipped_dup as i64,
                    failed as i64
                ],
            )
            .map(|_| ())
            .map_err(|e| format!("record import batch: {e}"))
    }

    // ---- V02 name templates + relocate ----------------------------------------

    /// Seed built-in folder/rename presets (first call only wins the
    /// defaults race; user edits are never overwritten).
    pub fn seed_templates(&self) {
        let now = chrono_stamp();
        for (kind, presets, first) in [
            (
                "folder",
                crate::template::FOLDER_PRESETS.as_slice(),
                "Dated",
            ),
            (
                "rename",
                crate::template::RENAME_PRESETS.as_slice(),
                "Date + sequence",
            ),
        ] {
            for (name, value) in presets {
                let stamp = if *name == first {
                    now.clone()
                } else {
                    "0".to_string()
                };
                self.conn
                    .execute(
                        "INSERT OR IGNORE INTO name_templates(kind, name, value, last_used)
                         VALUES (?1,?2,?3,?4)",
                        params![kind, name, value, stamp],
                    )
                    .ok();
            }
        }
    }

    /// Saved templates of a kind, most-recently-used first.
    pub fn list_templates(&self, kind: &str) -> Vec<(String, String)> {
        self.seed_templates();
        let mut stmt = match self.conn.prepare(
            "SELECT name, value FROM name_templates WHERE kind = ?1
             ORDER BY last_used DESC, name",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        stmt.query_map([kind], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default()
    }

    /// Save (or refresh) a template and make it the default.
    pub fn save_template(&self, kind: &str, name: &str, value: &str) -> Result<(), String> {
        self.seed_templates();
        self.conn
            .execute(
                "INSERT INTO name_templates(kind, name, value, last_used) VALUES (?1,?2,?3,?4)
                 ON CONFLICT(kind, name) DO UPDATE SET value = excluded.value, last_used = excluded.last_used",
                params![kind, name, value, chrono_stamp()],
            )
            .map(|_| ())
            .map_err(|e| format!("save template: {e}"))
    }

    /// Mark a preset used (it becomes the default).
    pub fn touch_template(&self, kind: &str, name: &str) {
        self.conn
            .execute(
                "UPDATE name_templates SET last_used = ?1 WHERE kind = ?2 AND name = ?3",
                params![chrono_stamp(), kind, name],
            )
            .ok();
    }

    // ---- V03 metadata presets, photo metadata, keywords -----------------------

    /// One metadata preset: authorship plus keywords.
    pub fn list_metadata_presets(&self) -> Vec<MetadataPreset> {
        let mut stmt = match self.conn.prepare(
            "SELECT name, title, caption, headline, creator, copyright, rights, contact,
             location, keywords
             FROM metadata_presets ORDER BY last_used DESC, name",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        stmt.query_map([], |r| {
            let opt = |i: usize| -> rusqlite::Result<String> {
                Ok(r.get::<_, Option<String>>(i)?.unwrap_or_default())
            };
            Ok(MetadataPreset {
                name: r.get(0)?,
                title: opt(1)?,
                caption: opt(2)?,
                headline: opt(3)?,
                creator: opt(4)?,
                copyright: opt(5)?,
                rights: opt(6)?,
                contact: opt(7)?,
                location: opt(8)?,
                keywords: opt(9)?,
            })
        })
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default()
    }

    pub fn save_metadata_preset(&self, p: &MetadataPreset) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT INTO metadata_presets(name, title, caption, headline, creator, copyright, rights,
                 contact, location, keywords, last_used)
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)
                 ON CONFLICT(name) DO UPDATE SET title = excluded.title, caption = excluded.caption,
                   headline = excluded.headline, creator = excluded.creator,
                   copyright = excluded.copyright, rights = excluded.rights,
                   contact = excluded.contact, location = excluded.location,
                   keywords = excluded.keywords, last_used = excluded.last_used",
                params![
                    p.name,
                    p.title,
                    p.caption,
                    p.headline,
                    p.creator,
                    p.copyright,
                    p.rights,
                    p.contact,
                    p.location,
                    p.keywords,
                    chrono_stamp()
                ],
            )
            .map(|_| ())
            .map_err(|e| format!("save metadata preset: {e}"))
    }

    pub fn touch_metadata_preset(&self, name: &str) {
        self.conn
            .execute(
                "UPDATE metadata_presets SET last_used = ?1 WHERE name = ?2",
                params![chrono_stamp(), name],
            )
            .ok();
    }

    /// V12: delete a metadata preset by name (no-op when missing).
    pub fn delete_metadata_preset(&self, name: &str) {
        self.conn
            .execute("DELETE FROM metadata_presets WHERE name = ?1", [name])
            .ok();
    }

    /// Import-default key/value (preset selections remembered per run).
    pub fn get_import_default(&self, key: &str) -> String {
        self.conn
            .query_row(
                "SELECT value FROM import_defaults WHERE key = ?1",
                [key],
                |r| r.get(0),
            )
            .unwrap_or_default()
    }

    pub fn set_import_default(&self, key: &str, value: &str) {
        self.conn
            .execute(
                "INSERT INTO import_defaults(key, value) VALUES (?1,?2)
                 ON CONFLICT(key) DO UPDATE SET value = ?2",
                params![key, value],
            )
            .ok();
    }

    // ---- Apple Photos sync ------------------------------------------------------

    pub fn photos_settings(&self) -> crate::apple_photos::PhotosSettings {
        crate::apple_photos::PhotosSettings::from_json(&self.get_import_default("apple_photos"))
    }

    pub fn set_photos_settings(&self, s: &crate::apple_photos::PhotosSettings) {
        self.set_import_default("apple_photos", &s.to_json());
    }

    /// Every photo in the planner's terms (paths absolute, keywords
    /// attached, existence checked).
    pub fn photos_candidates(&self) -> Vec<crate::apple_photos::Candidate> {
        let mut keywords: HashMap<i64, Vec<String>> = HashMap::new();
        if let Ok(mut q) = self.conn.prepare("SELECT photo_id, keyword FROM keywords") {
            if let Ok(rows) = q.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))
            {
                for (id, k) in rows.flatten() {
                    keywords.entry(id).or_default().push(k);
                }
            }
        }
        let root = self.root_path();
        self.all_photos()
            .into_iter()
            .map(|p| {
                let folder = Self::folder_under(&p.path, &root);
                crate::apple_photos::Candidate {
                    id: p.id,
                    is_raw: p.is_raw(),
                    is_video: p.duration_ms > 0
                        || laika_raw::media_kind(Path::new(&p.path))
                            == Some(laika_raw::MediaKind::Video),
                    exists: Path::new(&p.path).is_file(),
                    blake3: p.blake3.clone(),
                    folder: if folder == "(root)" {
                        String::new()
                    } else {
                        folder
                    },
                    rating: p.rating,
                    picked: p.picked,
                    rejected: p.rejected,
                    title: p.title,
                    caption: p.caption,
                    keywords: keywords.remove(&p.id).unwrap_or_default(),
                    path: p.path,
                }
            })
            .collect()
    }

    pub fn photos_links(&self) -> Vec<crate::apple_photos::Link> {
        let Ok(mut q) = self
            .conn
            .prepare("SELECT photo_id, item_id, meta_hash, album, origin FROM photos_links")
        else {
            return Vec::new();
        };
        q.query_map([], |r| {
            Ok(crate::apple_photos::Link {
                photo_id: r.get(0)?,
                item_id: r.get(1)?,
                meta_hash: r.get(2)?,
                album: r.get(3)?,
                from_photos: r.get::<_, String>(4)? == "photos",
            })
        })
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default()
    }

    /// Record a finished job. A pair that Photos kept as two items maps
    /// member-to-item in order; otherwise every member shares the item.
    pub fn record_photos_job(
        &self,
        job: &crate::apple_photos::Job,
        item_ids: &[String],
    ) -> Result<(), String> {
        let Some(first) = item_ids.first() else {
            return Err("no media item".to_string());
        };
        let now = chrono_sys_secs();
        for (i, id) in job.photo_ids.iter().enumerate() {
            let item = if item_ids.len() == job.photo_ids.len() {
                &item_ids[i]
            } else {
                first
            };
            self.conn
                .execute(
                    "INSERT INTO photos_links(photo_id, item_id, meta_hash, album, synced_at)
                     VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT(photo_id) DO UPDATE SET item_id = ?2, meta_hash = ?3,
                       album = ?4, synced_at = ?5",
                    params![id, item, job.meta_hash, job.album_key(), now],
                )
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// Replace the mirrored folder/album tree with a fresh dump.
    pub fn replace_photos_albums(
        &self,
        dump: &crate::apple_photos::LibraryDump,
    ) -> Result<(), String> {
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|e| e.to_string())?;
        tx.execute_batch("DELETE FROM photos_containers; DELETE FROM photos_album_items;")
            .map_err(|e| e.to_string())?;
        {
            let mut c = tx
                .prepare(
                    "INSERT OR REPLACE INTO photos_containers(id, parent, name, kind, position)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                )
                .map_err(|e| e.to_string())?;
            let mut m = tx
                .prepare(
                    "INSERT INTO photos_album_items(album_id, item_id, position) VALUES (?1, ?2, ?3)",
                )
                .map_err(|e| e.to_string())?;
            for (pos, k) in dump.containers.iter().enumerate() {
                let kind = match k.kind {
                    crate::apple_photos::ContainerKind::Folder => "folder",
                    crate::apple_photos::ContainerKind::Album => "album",
                };
                c.execute(params![k.id, k.parent, k.name, kind, pos as i64])
                    .map_err(|e| e.to_string())?;
                for (i, item) in k.items.iter().enumerate() {
                    m.execute(params![k.id, item, i as i64])
                        .map_err(|e| e.to_string())?;
                }
            }
        }
        tx.commit().map_err(|e| e.to_string())
    }

    /// The mirrored tree in Photos' order, with depth and how many of
    /// each album's photos are in the catalog.
    pub fn photos_album_tree(&self) -> Vec<PhotosAlbumNode> {
        let rows: Vec<(String, String, String, String)> = self
            .conn
            .prepare("SELECT id, parent, name, kind FROM photos_containers ORDER BY position")
            .and_then(|mut q| {
                q.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
                    .map(|rows| rows.flatten().collect())
            })
            .unwrap_or_default();
        let counts: HashMap<String, usize> = self
            .conn
            .prepare(
                "SELECT a.album_id, COUNT(DISTINCT l.photo_id) FROM photos_album_items a
                 JOIN photos_links l ON l.item_id = a.item_id
                 JOIN photos p ON p.id = l.photo_id
                 GROUP BY a.album_id",
            )
            .and_then(|mut q| {
                q.query_map([], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as usize))
                })
                .map(|rows| rows.flatten().collect())
            })
            .unwrap_or_default();
        let parents: HashMap<String, String> =
            rows.iter().map(|r| (r.0.clone(), r.1.clone())).collect();
        rows.into_iter()
            .map(|(id, parent, name, kind)| {
                let mut depth = 0;
                let mut up = parent.clone();
                while let Some(p) = parents.get(&up) {
                    depth += 1;
                    if depth > 32 {
                        break;
                    }
                    up = p.clone();
                }
                PhotosAlbumNode {
                    count: counts.get(&id).copied().unwrap_or(0),
                    is_folder: kind == "folder",
                    id,
                    parent,
                    name,
                    depth,
                }
            })
            .collect()
    }

    /// Catalog photos in one mirrored album.
    pub fn photos_album_members(&self, album_id: &str) -> HashSet<i64> {
        self.conn
            .prepare(
                "SELECT l.photo_id FROM photos_album_items a
                 JOIN photos_links l ON l.item_id = a.item_id
                 WHERE a.album_id = ?1",
            )
            .and_then(|mut q| {
                q.query_map([album_id], |r| r.get::<_, i64>(0))
                    .map(|rows| rows.flatten().collect())
            })
            .unwrap_or_default()
    }

    /// After cataloging library files: link each to its media item and
    /// take Photos' title, caption, keywords and favorite where Laika has
    /// nothing yet. Files skipped as duplicates link through their content
    /// hash. Returns how many photos were linked.
    pub fn apply_photos_ingest(
        &self,
        files: &[(String, String, String)],
        items: &HashMap<String, crate::apple_photos::LibraryItem>,
        settings: &crate::apple_photos::PhotosSettings,
    ) -> Result<usize, String> {
        use crate::apple_photos::FavoriteRule;
        if files.is_empty() {
            return Ok(0);
        }
        let linked: HashSet<i64> = self.photos_links().iter().map(|l| l.photo_id).collect();
        let now = chrono_sys_secs();
        let mut newly: Vec<(i64, String)> = Vec::new();
        for (path, item_id, hash) in files {
            // Rows may hold the root-relative form of the path.
            let stored = self.stored_form(Path::new(path));
            let by_path: Option<i64> = self
                .conn
                .query_row(
                    "SELECT id FROM photos WHERE path = ?1 OR path = ?2 LIMIT 1",
                    params![path, stored],
                    |r| r.get(0),
                )
                .ok();
            let id = by_path.or_else(|| {
                // Not cataloged under this path (a duplicate skipped at
                // import, or a failed file): match by content.
                let hash = if hash.is_empty() {
                    hash_file(Path::new(path)).unwrap_or_default()
                } else {
                    hash.clone()
                };
                (!hash.is_empty())
                    .then(|| {
                        self.conn
                            .query_row(
                                "SELECT id FROM photos WHERE blake3 = ?1 LIMIT 1",
                                [&hash],
                                |r| r.get(0),
                            )
                            .ok()
                    })
                    .flatten()
            });
            let Some(id) = id else { continue };
            if linked.contains(&id) || newly.iter().any(|(n, _)| *n == id) {
                continue;
            }
            if let Some(item) = items.get(item_id) {
                // Show Photos' original name, not the library's UUID file
                // name (a pair's RAW keeps its own extension).
                if !item.filename.is_empty() && crate::apple_photos::in_library(path) {
                    let is_primary = Path::new(path)
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .is_some_and(|s| !s.contains('_'));
                    let name = if is_primary {
                        item.filename.clone()
                    } else {
                        let stem = Path::new(&item.filename)
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or(&item.filename);
                        let ext = Path::new(path)
                            .extension()
                            .and_then(|e| e.to_str())
                            .unwrap_or("")
                            .to_ascii_uppercase();
                        format!("{stem}.{ext}")
                    };
                    self.conn
                        .execute(
                            "UPDATE photos SET filename = ?1 WHERE id = ?2",
                            params![name, id],
                        )
                        .ok();
                }
                let p = self.photo_by_id(id);
                if let Some(p) = p.as_ref() {
                    if p.title.is_empty() && !item.name.is_empty() {
                        self.conn
                            .execute(
                                "UPDATE photos SET title = ?1 WHERE id = ?2",
                                params![item.name, id],
                            )
                            .ok();
                    }
                    if p.caption.is_empty() && !item.description.is_empty() {
                        self.conn
                            .execute(
                                "UPDATE photos SET caption = ?1 WHERE id = ?2",
                                params![item.description, id],
                            )
                            .ok();
                    }
                    if item.favorite {
                        match settings.favorites {
                            FavoriteRule::Picked if !p.rejected => {
                                self.set_flag(id, true, false).ok();
                            }
                            FavoriteRule::FiveStars if p.rating < 5 => {
                                self.set_rating(id, 5).ok();
                            }
                            _ => {}
                        }
                    }
                }
                if !item.keywords.is_empty() {
                    let mut kws = self.photo_keywords(id);
                    for k in &item.keywords {
                        if !kws
                            .iter()
                            .any(|x| Self::keyword_leaf(x).eq_ignore_ascii_case(k))
                        {
                            kws.push(k.clone());
                        }
                    }
                    self.set_keywords(id, &kws).ok();
                }
            }
            newly.push((id, item_id.clone()));
        }
        if newly.is_empty() {
            return Ok(0);
        }
        // Hashes after metadata landed, so nothing pushes straight back.
        let hashes = crate::apple_photos::meta_hashes(&self.photos_candidates(), settings);
        for (id, item_id) in &newly {
            self.conn
                .execute(
                    "INSERT INTO photos_links(photo_id, item_id, meta_hash, album, synced_at, origin)
                     VALUES (?1, ?2, ?3, '', ?4, 'photos')
                     ON CONFLICT(photo_id) DO NOTHING",
                    params![id, item_id, hashes.get(id).cloned().unwrap_or_default(), now],
                )
                .map_err(|e| e.to_string())?;
        }
        Ok(newly.len())
    }

    /// Point a photo at its verified original inside the Photos library.
    /// The catalog filename stays the camera/import name.
    pub fn adopt_photos_original(&self, id: i64, library_file: &Path) -> Result<(), String> {
        if !library_file.is_file() {
            return Err(format!("nothing at {}", library_file.display()));
        }
        self.conn
            .execute(
                "UPDATE photos SET path = ?1 WHERE id = ?2",
                params![library_file.to_string_lossy().to_string(), id],
            )
            .map(|_| ())
            .map_err(|e| format!("adopt original: {e}"))
    }

    /// Forget links whose media item was deleted in Photos (re-imports
    /// on the next sync if still in scope).
    pub fn forget_photos_links(&self, photo_ids: &[i64]) {
        for id in photo_ids {
            self.conn
                .execute("DELETE FROM photos_links WHERE photo_id = ?1", [id])
                .ok();
        }
    }

    pub fn photos_forced(&self) -> HashSet<i64> {
        let Ok(mut q) = self.conn.prepare("SELECT photo_id FROM photos_include") else {
            return HashSet::new();
        };
        q.query_map([], |r| r.get::<_, i64>(0))
            .map(|rows| rows.flatten().collect())
            .unwrap_or_default()
    }

    pub fn add_photos_forced(&self, photo_ids: &[i64]) {
        for id in photo_ids {
            self.conn
                .execute(
                    "INSERT OR IGNORE INTO photos_include(photo_id) VALUES (?1)",
                    [id],
                )
                .ok();
        }
    }

    /// V28: named export presets — full dialog snapshots as JSON.
    /// Listed (folder, name) for the dialog; bodies parse at apply
    /// time so one corrupt preset never blocks the rest.
    pub fn list_export_presets(&self) -> Vec<(String, String)> {
        self.conn
            .prepare("SELECT name, folder FROM export_presets ORDER BY folder, name")
            .and_then(|mut s| {
                s.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
                    .and_then(|rows| rows.collect::<Result<Vec<_>, _>>())
            })
            .unwrap_or_default()
    }

    pub fn get_export_preset(&self, name: &str) -> Option<(String, String)> {
        self.conn
            .query_row(
                "SELECT folder, body FROM export_presets WHERE name = ?1",
                [name],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok()
    }

    pub fn save_export_preset(&self, name: &str, folder: &str, body: &str) {
        self.conn
            .execute(
                "INSERT INTO export_presets(name, folder, body) VALUES (?1,?2,?3)
                 ON CONFLICT(name) DO UPDATE SET folder = ?2, body = ?3",
                params![name, folder, body],
            )
            .ok();
    }

    pub fn delete_export_preset(&self, name: &str) {
        self.conn
            .execute("DELETE FROM export_presets WHERE name = ?1", [name])
            .ok();
    }

    /// Write authorship metadata for one photo (import time, not baked in).
    pub fn set_photo_metadata(
        &self,
        id: i64,
        creator: &str,
        copyright: &str,
        rights: &str,
        contact: &str,
    ) -> Result<(), String> {
        let mut meta = self.photo_meta(id);
        meta.creator = creator.to_string();
        meta.copyright = copyright.to_string();
        meta.rights = rights.to_string();
        meta.contact = contact.to_string();
        self.set_photo_meta(id, &meta)
    }

    /// V12: the right-rail editable descriptive block as data.
    #[allow(clippy::too_many_arguments)]
    pub fn set_photo_meta(&self, id: i64, meta: &PhotoMeta) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE photos SET title = ?1, caption = ?2, headline = ?3, creator = ?4,
                 copyright = ?5, rights = ?6, contact = ?7, location = ?8 WHERE id = ?9",
                params![
                    meta.title,
                    meta.caption,
                    meta.headline,
                    meta.creator,
                    meta.copyright,
                    meta.rights,
                    meta.contact,
                    meta.location,
                    id
                ],
            )
            .map(|_| ())
            .map_err(|e| format!("save metadata: {e}"))
    }

    /// V12: read the editable block back (batch panels diff on this).
    pub fn photo_meta(&self, id: i64) -> PhotoMeta {
        self.conn
            .query_row(
                "SELECT title, caption, headline, creator, copyright, rights, contact, location
                 FROM photos WHERE id = ?1",
                [id],
                |r| {
                    let opt = |i: usize| -> rusqlite::Result<String> {
                        Ok(r.get::<_, Option<String>>(i)?.unwrap_or_default())
                    };
                    Ok(PhotoMeta {
                        title: opt(0)?,
                        caption: opt(1)?,
                        headline: opt(2)?,
                        creator: opt(3)?,
                        copyright: opt(4)?,
                        rights: opt(5)?,
                        contact: opt(6)?,
                        location: opt(7)?,
                    })
                },
            )
            .unwrap_or_default()
    }

    /// V12: export-included keyword paths for one photo (nodes never
    /// registered — e.g. pre-V12 assignments — read as included).
    pub fn export_keyword_paths(&self, id: i64) -> Vec<String> {
        let assigned = self.photo_keywords(id);
        if assigned.is_empty() {
            return Vec::new();
        }
        let nodes: std::collections::HashMap<String, bool> =
            self.keyword_nodes().into_iter().collect();
        assigned
            .into_iter()
            .filter(|p| nodes.get(p).copied().unwrap_or(true))
            .collect()
    }

    /// Replace a photo's keyword set (typed text resolves synonyms,
    /// new paths auto-create their nodes — Lightroom behavior).
    pub fn set_keywords(&self, id: i64, keywords: &[String]) -> Result<(), String> {
        self.conn
            .execute("DELETE FROM keywords WHERE photo_id = ?1", [id])
            .map_err(|e| format!("save keywords: {e}"))?;
        let mut seen = Vec::new();
        for kw in keywords {
            let canon = self.resolve_keyword(kw);
            if canon.is_empty() || seen.contains(&canon) {
                continue;
            }
            seen.push(canon.clone());
            self.ensure_keyword_node(&canon);
            self.conn
                .execute(
                    "INSERT INTO keywords(photo_id, keyword) VALUES (?1,?2)",
                    params![id, canon],
                )
                .map_err(|e| format!("save keywords: {e}"))?;
        }
        Ok(())
    }

    pub fn photo_keywords(&self, id: i64) -> Vec<String> {
        let mut stmt = match self
            .conn
            .prepare("SELECT keyword FROM keywords WHERE photo_id = ?1 ORDER BY keyword")
        {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        stmt.query_map([id], |r| r.get(0))
            .map(|rows| rows.flatten().collect())
            .unwrap_or_default()
    }

    /// V03: split a typed keyword list on commas/semicolons, trimmed,
    /// deduplicated, order-preserving.
    pub fn split_keywords(s: &str) -> Vec<String> {
        let mut out = Vec::new();
        for part in s.split([',', ';']) {
            let kw = part.trim().to_string();
            if !kw.is_empty() && !out.contains(&kw) {
                out.push(kw);
            }
        }
        out
    }

    /// V12: normalize a keyword path — `A>B >  C` becomes `A > B > C`.
    /// Empty segments drop; a blank path normalizes to empty.
    pub fn canon_keyword_path(s: &str) -> String {
        s.split('>')
            .map(|p| p.trim())
            .filter(|p| !p.is_empty())
            .collect::<Vec<_>>()
            .join(" > ")
    }

    /// V12: leaf display name (`Lisbon` for `Places > Portugal > Lisbon`).
    pub fn keyword_leaf(path: &str) -> &str {
        path.rsplit(" > ").next().unwrap_or(path)
    }

    /// V12: ancestor paths, shallowest first.
    pub fn keyword_parents(path: &str) -> Vec<String> {
        let segs: Vec<&str> = path.split(" > ").collect();
        (1..segs.len()).map(|n| segs[..n].join(" > ")).collect()
    }

    /// V12: ensure a node and its ancestors exist (include defaults on).
    pub fn ensure_keyword_node(&self, path: &str) {
        let path = Self::canon_keyword_path(path);
        if path.is_empty() {
            return;
        }
        for ancestor in Self::keyword_parents(&path).into_iter().chain([path]) {
            self.conn
                .execute(
                    "INSERT INTO keyword_nodes(path, include) VALUES (?1, 1)
                     ON CONFLICT(path) DO NOTHING",
                    [ancestor],
                )
                .ok();
        }
    }

    /// V12: all nodes (path, include) ordered for the tree.
    pub fn keyword_nodes(&self) -> Vec<(String, bool)> {
        self.conn
            .prepare("SELECT path, include FROM keyword_nodes ORDER BY path")
            .and_then(|mut s| {
                s.query_map([], |r| {
                    let p: String = r.get(0)?;
                    let i: i64 = r.get(1)?;
                    Ok((p, i != 0))
                })
                .and_then(|rows| rows.collect::<Result<Vec<_>, _>>())
            })
            .unwrap_or_default()
    }

    /// V12: export-inclusion flag (excluded keywords stay catalog-only).
    pub fn set_keyword_include(&self, path: &str, include: bool) {
        self.ensure_keyword_node(path);
        self.conn
            .execute(
                "UPDATE keyword_nodes SET include = ?1 WHERE path = ?2",
                params![if include { 1 } else { 0 }, Self::canon_keyword_path(path)],
            )
            .ok();
    }

    /// V12: rename a node — the subtree, synonyms, sets, and assigned
    /// photo keywords all follow, so hierarchy survives rename.
    pub fn rename_keyword_node(&self, old: &str, new: &str) -> Result<(), String> {
        let old = Self::canon_keyword_path(old);
        let new = Self::canon_keyword_path(new);
        if old.is_empty() || new.is_empty() {
            return Err("keyword path is empty".to_string());
        }
        if new == old || new.starts_with(&format!("{old} > ")) {
            return Err("keyword cannot move into itself".to_string());
        }
        // Ancestors of the target exist; the target itself may exist —
        // renaming onto it merges (UPDATE OR IGNORE + leftover delete).
        for ancestor in Self::keyword_parents(&new) {
            self.ensure_keyword_node(&ancestor);
        }
        let subtree: Vec<String> = self
            .keyword_nodes()
            .into_iter()
            .map(|(p, _)| p)
            .filter(|p| *p == old || p.starts_with(&format!("{old} > ")))
            .collect();
        for p in &subtree {
            let renamed = format!("{new}{}", &p[old.len()..]);
            for (table, col) in [
                ("keyword_nodes", "path"),
                ("keywords", "keyword"),
                ("keyword_synonyms", "path"),
            ] {
                self.conn
                    .execute(
                        &format!("UPDATE OR IGNORE {table} SET {col} = ?1 WHERE {col} = ?2"),
                        params![renamed, p],
                    )
                    .map_err(|e| format!("rename keyword: {e}"))?;
            }
        }
        // Leftovers are merge-into-existing rows: drop them so no stale
        // source path survives alongside the target.
        for p in &subtree {
            for (table, col) in [
                ("keyword_nodes", "path"),
                ("keywords", "keyword"),
                ("keyword_synonyms", "path"),
            ] {
                self.conn
                    .execute(&format!("DELETE FROM {table} WHERE {col} = ?1"), [p])
                    .map_err(|e| format!("rename keyword: {e}"))?;
            }
        }
        // Assignments carry no unique constraint: collapse the
        // merge-created (photo, keyword) duplicates.
        self.conn
            .execute(
                "DELETE FROM keywords WHERE rowid NOT IN
                 (SELECT MIN(rowid) FROM keywords GROUP BY photo_id, keyword)",
                [],
            )
            .map_err(|e| format!("rename keyword: {e}"))?;
        // Sets store `;`-joined paths: rewrite whole-list membership.
        for (name, paths) in self.list_keyword_sets() {
            let rewritten: Vec<String> = paths
                .into_iter()
                .map(|p| {
                    if p == old || p.starts_with(&format!("{old} > ")) {
                        format!("{new}{}", &p[old.len()..])
                    } else {
                        p
                    }
                })
                .collect();
            self.save_keyword_set(&name, &rewritten);
        }
        self.conn
            .execute("DELETE FROM keyword_nodes WHERE path = ?1", [old])
            .map_err(|e| format!("rename keyword: {e}"))?;
        Ok(())
    }

    /// V12: merge one node into another — photo assignments and synonyms
    /// move, the source node goes away.
    pub fn merge_keyword_nodes(&self, from: &str, into: &str) -> Result<(), String> {
        let from = Self::canon_keyword_path(from);
        let into = Self::canon_keyword_path(into);
        if from.is_empty() || into.is_empty() || from == into {
            return Err("keyword merge needs two different paths".to_string());
        }
        self.ensure_keyword_node(&into);
        // Move assignments (dedupe: a photo with both keeps one).
        let ids: Vec<i64> = self
            .conn
            .prepare("SELECT photo_id FROM keywords WHERE keyword = ?1")
            .and_then(|mut s| {
                s.query_map([from.as_str()], |r| r.get(0))
                    .and_then(|rows| rows.collect::<Result<Vec<_>, _>>())
            })
            .unwrap_or_default();
        for id in ids {
            let mut kws = self.photo_keywords(id);
            kws.retain(|k| *k != from);
            if !kws.contains(&into) {
                kws.push(into.clone());
            }
            self.set_keywords(id, &kws)?;
        }
        // Move synonyms.
        let syns: Vec<String> = self
            .conn
            .prepare("SELECT synonym FROM keyword_synonyms WHERE path = ?1")
            .and_then(|mut s| {
                s.query_map([from.as_str()], |r| r.get(0))
                    .and_then(|rows| rows.collect::<Result<Vec<_>, _>>())
            })
            .unwrap_or_default();
        for syn in syns {
            self.conn
                .execute(
                    "INSERT INTO keyword_synonyms(path, synonym) VALUES (?1,?2)
                     ON CONFLICT(path, synonym) DO NOTHING",
                    params![into, syn],
                )
                .map_err(|e| format!("merge keywords: {e}"))?;
        }
        self.delete_keyword_node(&from);
        Ok(())
    }

    /// V12: delete a node subtree — synonyms go, photo assignments drop
    /// the removed paths, sets drop them too.
    pub fn delete_keyword_node(&self, path: &str) {
        let path = Self::canon_keyword_path(path);
        if path.is_empty() {
            return;
        }
        let prefix = format!("{path} > ");
        let doomed: Vec<String> = self
            .keyword_nodes()
            .into_iter()
            .map(|(p, _)| p)
            .filter(|p| *p == path || p.starts_with(&prefix))
            .collect();
        for p in &doomed {
            self.conn
                .execute("DELETE FROM keyword_nodes WHERE path = ?1", [p])
                .ok();
            self.conn
                .execute("DELETE FROM keyword_synonyms WHERE path = ?1", [p])
                .ok();
            self.conn
                .execute("DELETE FROM keywords WHERE keyword = ?1", [p])
                .ok();
        }
        for (name, paths) in self.list_keyword_sets() {
            let kept: Vec<String> = paths
                .into_iter()
                .filter(|p| *p != path && !p.starts_with(&prefix))
                .collect();
            self.save_keyword_set(&name, &kept);
        }
    }

    /// V12: synonyms resolve to their canonical path on assignment.
    pub fn add_keyword_synonym(&self, path: &str, synonym: &str) {
        let (path, synonym) = (Self::canon_keyword_path(path), synonym.trim().to_string());
        if path.is_empty() || synonym.is_empty() {
            return;
        }
        self.ensure_keyword_node(&path);
        self.conn
            .execute(
                "INSERT INTO keyword_synonyms(path, synonym) VALUES (?1,?2)
                 ON CONFLICT(path, synonym) DO NOTHING",
                params![path, synonym],
            )
            .ok();
    }

    pub fn remove_keyword_synonym(&self, path: &str, synonym: &str) {
        self.conn
            .execute(
                "DELETE FROM keyword_synonyms WHERE path = ?1 AND synonym = ?2",
                params![Self::canon_keyword_path(path), synonym.trim()],
            )
            .ok();
    }

    pub fn synonyms_for(&self, path: &str) -> Vec<String> {
        self.conn
            .prepare("SELECT synonym FROM keyword_synonyms WHERE path = ?1 ORDER BY synonym")
            .and_then(|mut s| {
                s.query_map([Self::canon_keyword_path(path)], |r| r.get(0))
                    .and_then(|rows| rows.collect::<Result<Vec<_>, _>>())
            })
            .unwrap_or_default()
    }

    /// V12: resolve typed text — exact synonym (case-insensitive) maps
    /// to its canonical path, otherwise the normalized path itself.
    pub fn resolve_keyword(&self, s: &str) -> String {
        let typed = s.trim().to_string();
        if typed.is_empty() {
            return String::new();
        }
        let found: Option<String> = self
            .conn
            // Cached: runs once per keyword inside `set_keywords` loops.
            .prepare_cached(
                "SELECT path FROM keyword_synonyms WHERE synonym = ?1 COLLATE NOCASE LIMIT 1",
            )
            .and_then(|mut q| q.query_row([typed.as_str()], |r| r.get(0)))
            .ok();
        found.unwrap_or_else(|| Self::canon_keyword_path(&typed))
    }

    /// V12: named keyword sets (one-click multi-apply in the rail).
    pub fn save_keyword_set(&self, name: &str, paths: &[String]) {
        if name.trim().is_empty() {
            return;
        }
        let joined = paths
            .iter()
            .map(|p| Self::canon_keyword_path(p))
            .filter(|p| !p.is_empty())
            .collect::<Vec<_>>()
            .join(";");
        self.conn
            .execute(
                "INSERT INTO keyword_sets(name, paths) VALUES (?1,?2)
                 ON CONFLICT(name) DO UPDATE SET paths = ?2",
                params![name.trim(), joined],
            )
            .ok();
    }

    pub fn delete_keyword_set(&self, name: &str) {
        self.conn
            .execute("DELETE FROM keyword_sets WHERE name = ?1", [name])
            .ok();
    }

    pub fn list_keyword_sets(&self) -> Vec<(String, Vec<String>)> {
        self.conn
            .prepare("SELECT name, paths FROM keyword_sets ORDER BY name")
            .and_then(|mut s| {
                s.query_map([], |r| {
                    let name: String = r.get(0)?;
                    let paths: String = r.get(1)?;
                    Ok((
                        name,
                        paths
                            .split(';')
                            .map(|p| p.to_string())
                            .filter(|p| !p.is_empty())
                            .collect(),
                    ))
                })
                .and_then(|rows| rows.collect::<Result<Vec<_>, _>>())
            })
            .unwrap_or_default()
    }

    /// V12: keyword list as text — one node per line, `!path` excludes
    /// from export, `path = syn, syn` carries synonyms, `@set: a; b`
    /// carries sets. Round-trips through `import_keywords_text`.
    pub fn export_keywords_text(&self) -> String {
        let mut lines = Vec::new();
        for (path, include) in self.keyword_nodes() {
            let mut line = if include {
                path.clone()
            } else {
                format!("!{path}")
            };
            let syns = self.synonyms_for(&path);
            if !syns.is_empty() {
                line.push_str(&format!(" = {}", syns.join(", ")));
            }
            lines.push(line);
        }
        for (name, paths) in self.list_keyword_sets() {
            lines.push(format!("@{name}: {}", paths.join("; ")));
        }
        lines.join("\n") + "\n"
    }

    /// V12: parse the text format back. Returns (nodes, synonyms).
    /// Unknown lines explain themselves; sets replace by name.
    pub fn import_keywords_text(&self, text: &str) -> Result<(usize, usize), String> {
        let mut nodes = 0;
        let mut syns = 0;
        for (n, raw) in text.lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some(rest) = line.strip_prefix('@') {
                let (name, paths) = rest
                    .split_once(':')
                    .ok_or_else(|| format!("line {}: set needs `name: paths`", n + 1))?;
                if name.trim().is_empty() {
                    return Err(format!("line {}: set needs a name", n + 1));
                }
                let ps: Vec<String> = paths
                    .split(';')
                    .map(|p| Self::canon_keyword_path(p))
                    .filter(|p| !p.is_empty())
                    .collect();
                self.save_keyword_set(name.trim(), &ps);
                continue;
            }
            let (path_part, syn_part) = match line.split_once('=') {
                Some((p, s)) => (p.trim(), Some(s)),
                None => (line, None),
            };
            let (path, include) = match path_part.strip_prefix('!') {
                Some(p) => (Self::canon_keyword_path(p), false),
                None => (Self::canon_keyword_path(path_part), true),
            };
            if path.is_empty() {
                return Err(format!("line {}: empty keyword path", n + 1));
            }
            self.ensure_keyword_node(&path);
            self.set_keyword_include(&path, include);
            nodes += 1;
            if let Some(syn_list) = syn_part {
                for syn in syn_list.split(',') {
                    let syn = syn.trim();
                    if syn.is_empty() {
                        continue;
                    }
                    self.add_keyword_synonym(&path, syn);
                    syns += 1;
                }
            }
        }
        Ok((nodes, syns))
    }

    /// V03: shift an EXIF-style capture time (`YYYY:MM:DD HH:MM:SS`) by
    /// whole minutes. Garbage passes through unchanged; the original file
    /// EXIF is never touched (only the catalog value moves).
    pub fn shift_captured_at(at: &str, minutes: i32) -> String {
        if minutes == 0 {
            return at.to_string();
        }
        let mut parts = at.split(' ');
        let (date, time) = match (parts.next(), parts.next()) {
            (Some(d), Some(t)) => (d, t),
            _ => return at.to_string(),
        };
        let d: Vec<&str> = date.split(':').collect();
        let t: Vec<&str> = time.split(':').collect();
        if d.len() != 3 || t.len() < 2 {
            return at.to_string();
        }
        let nums: Option<Vec<i64>> = d
            .iter()
            .chain(t.iter().take(3))
            .map(|s| s.parse::<i64>().ok())
            .collect();
        let mut n = match nums {
            Some(v) if v.len() >= 5 => v,
            _ => return at.to_string(),
        };
        while n.len() < 6 {
            n.push(0);
        }
        let (y, mo, dd, h, mi, s) = (n[0], n[1], n[2], n[3], n[4], n[5]);
        if !(1..=9999).contains(&y) || !(1..=12).contains(&mo) || !(1..=31).contains(&dd) {
            return at.to_string();
        }
        // Days from civil date; minutes since midnight; shift; convert back.
        let y0 = if mo <= 2 { y - 1 } else { y };
        let era = if y0 >= 0 { y0 } else { y0 - 399 } / 400;
        let yoe = y0 - era * 400;
        let mp = (mo + 9) % 12;
        let doy = (153 * mp + 2) / 5 + dd - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        let days = era * 146097 + doe - 719468;
        let total_min = days * 1440 + h * 60 + mi + minutes as i64;
        let days2 = total_min.div_euclid(1440);
        let min2 = total_min.rem_euclid(1440);
        let z = days2 + 719468;
        let era2 = if z >= 0 { z } else { z - 146096 } / 146097;
        let doe2 = z - era2 * 146097;
        let yoe2 = (doe2 - doe2 / 1460 + doe2 / 36524 - doe2 / 146096) / 365;
        let y2 = yoe2 + era2 * 400;
        let doy2 = doe2 - (365 * yoe2 + yoe2 / 4 - yoe2 / 100);
        let mp2 = (5 * doy2 + 2) / 153;
        let d2 = doy2 - (153 * mp2 + 2) / 5 + 1;
        let m2 = if mp2 < 10 { mp2 + 3 } else { mp2 - 9 };
        let y2 = if m2 <= 2 { y2 + 1 } else { y2 };
        format!(
            "{:04}:{:02}:{:02} {:02}:{:02}:{:02}",
            y2,
            m2,
            d2,
            min2 / 60,
            min2 % 60,
            s
        )
    }

    /// Apply a capture-time offset in minutes: `captured_at` becomes the
    /// shifted time (sorting, templates), the file EXIF is untouched, and
    /// the pre-offset value is kept for V14.
    pub fn apply_capture_offset(
        &self,
        id: i64,
        shifted: &str,
        orig: &str,
        minutes: i32,
    ) -> Result<(), String> {
        self.conn
            .execute(
                "UPDATE photos SET captured_at = ?1, captured_orig = ?2, capture_offset_min = ?3 WHERE id = ?4",
                params![shifted, orig, minutes, id],
            )
            .map(|_| ())
            .map_err(|e| format!("save capture time: {e}"))
    }

    /// Move one catalogued photo (and its sidecar) to a new filename in the
    /// same directory. Previews stay valid (hash-keyed) and sync rows are
    /// photo-id-keyed; the caller re-enqueues uploads so backup converges
    /// to the new keys. Returns the new path.
    pub fn relocate_photo(&self, id: i64, new_filename: &str) -> Result<String, String> {
        let (stored, filename): (String, String) = self
            .conn
            .query_row(
                "SELECT path, filename FROM photos WHERE id = ?1",
                [id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .map_err(|e| format!("find photo: {e}"))?;
        // V07: rows may store root-relative paths; fs ops need absolute.
        let path = self.resolve_stored(&stored);
        if new_filename == filename {
            return Ok(path);
        }
        // Originals kept by Apple Photos are never renamed on disk; the
        // catalog name (display, export templates) still changes.
        if crate::apple_photos::in_library(&path) {
            self.conn
                .execute(
                    "UPDATE photos SET filename = ?1 WHERE id = ?2",
                    params![new_filename, id],
                )
                .map_err(|e| format!("rename: {e}"))?;
            return Ok(path);
        }
        let old_path = Path::new(&path);
        let dir = old_path
            .parent()
            .ok_or_else(|| "photo has no parent directory".to_string())?;
        let dest = crate::import::unique_dest_name(dir, new_filename);
        std::fs::rename(old_path, &dest)
            .map_err(|e| format!("rename {}: {e}", old_path.display()))?;
        // The sidecar follows the original when present; a missing sidecar
        // is fine (it will be rewritten on the next edit).
        let old_side = crate::xmp::sidecar_path(&path);
        let mut moved_side: Option<String> = None;
        if Path::new(&old_side).exists() {
            let new_side = crate::xmp::sidecar_path(&dest.to_string_lossy());
            if let Err(e) = std::fs::rename(&old_side, &new_side) {
                // Roll back the original so catalog and disk agree.
                let _ = std::fs::rename(&dest, old_path);
                return Err(format!("rename sidecar: {e}"));
            }
            moved_side = Some(new_side);
        }
        let abs_path = dest.to_string_lossy().to_string();
        let new_name = dest
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(new_filename)
            .to_string();
        // V07: keep rows portable.
        let new_path = self.stored_form(Path::new(&abs_path));
        if let Err(e) = self.conn.execute(
            "UPDATE photos SET path = ?1, filename = ?2 WHERE id = ?3",
            params![new_path, new_name, id],
        ) {
            // The row still names the old file: put disk back to match
            // (e.g. UNIQUE(path) clash with a missing row at `dest`).
            if let Some(new_side) = moved_side {
                let _ = std::fs::rename(&new_side, &old_side);
            }
            let _ = std::fs::rename(&dest, old_path);
            return Err(format!("save rename: {e}"));
        }
        Ok(abs_path)
    }

    /// Enqueue one job; no-op when already queued or claimed. Marks the
    /// photo pending (new work supersedes a previous failure).
    pub fn enqueue(&self, photo_id: i64, kind: &str) {
        // A leftover failed row for (photo, kind) must be re-queued: plain
        // INSERT OR IGNORE would keep it failed forever while the photo
        // reads pending.
        self.conn
            .execute(
                "INSERT INTO sync_queue(photo_id, kind, state) VALUES (?1,?2,'queued')
                 ON CONFLICT(photo_id, kind) DO UPDATE SET state = 'queued', error = ''
                 WHERE sync_queue.state = 'failed'",
                rusqlite::params![photo_id, kind],
            )
            .ok();
        self.conn
            .execute(
                "UPDATE photos SET sync_state = 'pending' WHERE id = ?1",
                [photo_id],
            )
            .ok();
    }

    /// Catalog display name (S3 key prefix source).
    pub fn catalog_name(&self) -> String {
        self.conn
            .query_row("SELECT name FROM catalogs LIMIT 1", [], |r| r.get(0))
            .unwrap_or_else(|_| "laika".to_string())
    }

    /// Rewrite rows under one absolute prefix into stored form
    /// (relative when under root). Keeps the catalog portable after folder
    /// operations that must write absolute prefixes in SQL.
    pub fn normalize_prefix(&self, abs_prefix: &str) -> usize {
        let like = format!(
            "{}%",
            abs_prefix
                .replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        );
        let rows: Vec<(i64, String)> = self
            .conn
            .prepare("SELECT id, path FROM photos WHERE path LIKE ?1 ESCAPE '\\'")
            .and_then(|mut s| {
                s.query_map([like], |r| {
                    Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
                })
                .map(|rows| rows.flatten().collect())
            })
            .unwrap_or_default();
        let mut changed = 0;
        for (id, abs) in rows {
            let stored = self.stored_form(Path::new(&abs));
            if stored != abs
                && self
                    .conn
                    .execute(
                        "UPDATE photos SET path = ?1 WHERE id = ?2",
                        params![stored, id],
                    )
                    .is_ok()
            {
                changed += 1;
            }
        }
        changed
    }

    /// Resolve one photo's upload inputs on the UI thread (owns the DB).
    /// Returns `(local_path, hash, captured_at)`. Sidecar hashes are read
    /// from the current `.xmp` file; originals use the import-time blake3.
    pub fn job_inputs(&self, photo_id: i64, kind: &str) -> Option<(PathBuf, String, String)> {
        let (stored, hash, captured): (String, String, String) = self
            .conn
            .query_row(
                "SELECT path, blake3, COALESCE(captured_at,'') FROM photos WHERE id = ?1",
                [photo_id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .ok()?;
        // V07: rows may store root-relative paths; transfers need absolute.
        let path = self.resolve_stored(&stored);
        if kind == "sidecar" {
            let side = crate::xmp::sidecar_path(&path);
            let h = hash_file(Path::new(&side)).ok()?;
            Some((PathBuf::from(side), h, captured))
        } else {
            Some((PathBuf::from(path), hash, captured))
        }
    }

    /// Backups are per destination: when the destination changes, photos
    /// verified at the old one are queued again (originals + existing
    /// sidecars) for the new one. Returns true when a rebase happened.
    pub fn rebase_backup_destination(&self, dest: &str) -> bool {
        let stored = self.get_import_default("backup_dest");
        if stored == dest {
            return false;
        }
        self.set_import_default("backup_dest", dest);
        if stored.is_empty() {
            // First destination recorded: existing states belong to it.
            return false;
        }
        self.conn.execute("DELETE FROM sync_queue", []).ok();
        self.conn
            .execute(
                "UPDATE photos SET sync_state = 'local', remote_key = ''",
                [],
            )
            .ok();
        let rows: Vec<(i64, String)> = self
            .conn
            .prepare("SELECT id, path FROM photos")
            .and_then(|mut st| {
                st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                    .collect::<Result<Vec<_>, _>>()
            })
            .unwrap_or_default();
        for (id, stored_path) in rows {
            let path = self.resolve_stored(&stored_path);
            if Path::new(&crate::xmp::sidecar_path(&path)).exists() {
                self.enqueue(id, "sidecar");
            }
        }
        true
    }

    /// Enqueue originals for every photo not yet verified. Returns new rows.
    pub fn enqueue_unsynced(&self) -> usize {
        // Set-based: a per-row INSERT + UPDATE loop was two autocommit
        // write transactions (fsyncs) per photo on first backup.
        let n = self
            .conn
            .execute(
                "INSERT OR IGNORE INTO sync_queue(photo_id, kind, state)
                 SELECT id, 'original', 'queued' FROM photos
                 WHERE sync_state IN ('local','pending','failed')",
                [],
            )
            .unwrap_or(0);
        self.conn
            .execute(
                "UPDATE photos SET sync_state = 'pending'
                 WHERE sync_state IN ('local','pending','failed')",
                [],
            )
            .ok();
        // Crash recovery: claimed-but-never-finished goes back to queued.
        self.conn
            .execute(
                "UPDATE sync_queue SET state = 'queued' WHERE state = 'claimed'",
                [],
            )
            .ok();
        n
    }

    /// Atomically claim one queued job (multi-worker safe under WAL).
    pub fn claim_job(&self) -> Option<crate::sync::SyncJob> {
        let row: Option<(i64, i64, String)> = self
            .conn
            .query_row(
                "UPDATE sync_queue SET state = 'claimed'
                 WHERE rowid = (SELECT rowid FROM sync_queue WHERE state = 'queued' ORDER BY rowid LIMIT 1)
                 RETURNING rowid, photo_id, kind",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .ok();
        row.map(|(rowid, photo_id, kind)| crate::sync::SyncJob {
            rowid,
            photo_id,
            kind,
        })
    }

    pub fn complete_job(&self, job: &crate::sync::SyncJob, remote_key: Option<&str>) {
        self.conn
            .execute("DELETE FROM sync_queue WHERE rowid = ?1", [job.rowid])
            .ok();
        // Only the last outstanding job verifies the photo: a sidecar that
        // finishes while the original is still queued (or failed) must not
        // flip the photo to synced.
        let (remaining, failed): (i64, i64) = self
            .conn
            .query_row(
                "SELECT COUNT(*), COALESCE(SUM(state = 'failed'), 0)
                 FROM sync_queue WHERE photo_id = ?1",
                [job.photo_id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap_or((0, 0));
        let state = if remaining == 0 {
            crate::photo::SyncState::Synced
        } else if failed > 0 {
            crate::photo::SyncState::Failed
        } else {
            crate::photo::SyncState::Pending
        };
        self.set_sync(job.photo_id, state);
        if job.kind == "original" {
            if let Some(key) = remote_key {
                self.conn
                    .execute(
                        "UPDATE photos SET remote_key = ?1 WHERE id = ?2",
                        rusqlite::params![key, job.photo_id],
                    )
                    .ok();
            }
        }
    }

    pub fn fail_job(&self, job: &crate::sync::SyncJob, error: &str) {
        self.conn
            .execute(
                "UPDATE sync_queue SET state = 'failed', attempts = attempts + 1, error = ?1 WHERE rowid = ?2",
                rusqlite::params![error, job.rowid],
            )
            .ok();
        self.set_sync(job.photo_id, crate::photo::SyncState::Failed);
    }

    /// Re-queue failures (all, or one photo). Returns re-queued rows.
    pub fn retry_failed(&self, photo_id: Option<i64>) -> usize {
        let n = match photo_id {
            Some(id) => self
                .conn
                .execute(
                    "UPDATE sync_queue SET state = 'queued', error = '' WHERE photo_id = ?1 AND state = 'failed'",
                    [id],
                )
                .unwrap_or(0),
            None => self
                .conn
                .execute("UPDATE sync_queue SET state = 'queued', error = '' WHERE state = 'failed'", [])
                .unwrap_or(0),
        };
        match photo_id {
            None => {
                self.conn
                    .execute(
                        "UPDATE photos SET sync_state = 'pending' WHERE sync_state = 'failed'",
                        [],
                    )
                    .ok();
            }
            // Single-photo retry: the photo must stop reading failed too.
            Some(id) if n > 0 => {
                self.conn
                    .execute(
                        "UPDATE photos SET sync_state = 'pending' WHERE id = ?1 AND sync_state = 'failed'",
                        [id],
                    )
                    .ok();
            }
            Some(_) => {}
        }
        n as usize
    }

    pub fn queue_depth(&self) -> (usize, usize) {
        let count = |state: &str| -> usize {
            self.conn
                .query_row(
                    "SELECT COUNT(*) FROM sync_queue WHERE state = ?1",
                    [state],
                    |r| r.get::<_, i64>(0),
                )
                .unwrap_or(0) as usize
        };
        (count("queued") + count("claimed"), count("failed"))
    }

    pub fn set_remote_key(&self, id: i64, key: &str) {
        self.conn
            .execute(
                "UPDATE photos SET remote_key = ?1 WHERE id = ?2",
                rusqlite::params![key, id],
            )
            .ok();
    }

    /// Distinct keywords with usage counts (U13 owns editing; empty until
    /// keywords are actually assigned — never sample content).
    pub fn keywords(&self) -> Vec<(String, usize)> {
        let mut stmt = match self
            .conn
            .prepare("SELECT keyword, COUNT(*) FROM keywords GROUP BY keyword ORDER BY keyword")
        {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        stmt.query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as usize))
        })
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default()
    }

    /// Folders derived from path prefixes relative to the catalog root.
    pub fn folders(&self) -> Vec<(String, usize)> {
        let root: String = self
            .conn
            .query_row("SELECT root_path FROM catalogs LIMIT 1", [], |r| r.get(0))
            .unwrap_or_default();
        let mut counts = std::collections::BTreeMap::new();
        let photos = self.all_photos();
        for p in &photos {
            if crate::apple_photos::in_library(&p.path) {
                *counts.entry(Self::PHOTOS_FOLDER.to_string()).or_insert(0) += 1;
                continue;
            }
            // Component-wise prefix: root `/pics` must not claim `/pics2/…`.
            let rel = Path::new(&p.path)
                .strip_prefix(&root)
                .ok()
                .and_then(|r| r.to_str())
                .unwrap_or(&p.path);
            let rel = rel.trim_start_matches(['/', '\\']);
            let folder = Path::new(rel)
                .parent()
                .and_then(|d| d.to_str())
                .filter(|d| !d.is_empty())
                .unwrap_or("(root)");
            *counts.entry(folder.to_string()).or_insert(0) += 1;
        }
        counts.into_iter().collect()
    }

    // ---- U12 folder scope, metadata browser, saved presets ---------------------

    /// Catalog root for relative folder math (U12 scope matching).
    pub fn root_path(&self) -> String {
        self.conn
            .query_row("SELECT root_path FROM catalogs LIMIT 1", [], |r| r.get(0))
            .unwrap_or_default()
    }

    /// Folder label for photos whose originals live in Apple Photos.
    pub const PHOTOS_FOLDER: &'static str = "Apple Photos";

    fn refuse_photos_library(dir: &Path) -> Result<(), String> {
        let s = dir.to_string_lossy();
        if crate::apple_photos::in_library(&format!("{s}/")) || s == Self::PHOTOS_FOLDER {
            return Err("Apple Photos manages that folder".to_string());
        }
        Ok(())
    }

    /// Relative folder of one photo path (`"(root)"` at the top level).
    pub fn folder_of(&self, path: &str) -> String {
        Self::folder_under(path, &self.root_path())
    }

    /// `folder_of` with the catalog root already in hand (loops over many
    /// photos read the root once instead of querying it per photo).
    pub fn folder_under(path: &str, root: &str) -> String {
        if crate::apple_photos::in_library(path) {
            return Self::PHOTOS_FOLDER.to_string();
        }
        // Component-wise prefix (matches `folders`): root `/pics` must not
        // claim `/pics2/…`.
        let rel = Path::new(path)
            .strip_prefix(root)
            .ok()
            .and_then(|r| r.to_str())
            .unwrap_or(path);
        let rel = rel.trim_start_matches(['/', '\\']);
        Path::new(rel)
            .parent()
            .and_then(|d| d.to_str())
            .filter(|d| !d.is_empty())
            .unwrap_or("(root)")
            .to_string()
    }

    /// Distinct non-empty cameras with counts, most common first.
    pub fn distinct_cameras(&self) -> Vec<(String, usize)> {
        self.distinct_meta("camera")
    }

    /// Distinct non-empty lenses with counts, most common first.
    pub fn distinct_lenses(&self) -> Vec<(String, usize)> {
        self.distinct_meta("lens")
    }

    fn distinct_meta(&self, col: &str) -> Vec<(String, usize)> {
        // Column name is caller-fixed, never user input.
        let mut stmt = match self.conn.prepare(&format!(
            "SELECT {col}, COUNT(*) FROM photos WHERE catalog_id = ?1 AND {col} != ''
             GROUP BY {col} ORDER BY COUNT(*) DESC, {col} LIMIT 10"
        )) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        stmt.query_map([self.id], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)? as usize))
        })
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default()
    }

    /// Saved filter presets, most-recently-used first.
    pub fn list_filter_presets(&self) -> Vec<(String, String)> {
        let mut stmt = match self
            .conn
            .prepare("SELECT name, filters_json FROM filter_presets ORDER BY last_used DESC, name")
        {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .map(|rows| rows.flatten().collect())
            .unwrap_or_default()
    }

    pub fn save_filter_preset(&self, name: &str, filters_json: &str) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT INTO filter_presets(name, filters_json, last_used) VALUES (?1,?2,?3)
                 ON CONFLICT(name) DO UPDATE SET filters_json = excluded.filters_json,
                   last_used = excluded.last_used",
                params![name, filters_json, chrono_stamp()],
            )
            .map(|_| ())
            .map_err(|e| format!("save filter preset: {e}"))
    }

    pub fn delete_filter_preset(&self, name: &str) {
        self.conn
            .execute("DELETE FROM filter_presets WHERE name = ?1", [name])
            .ok();
    }

    // ---- U17 missing files, relink, move, remove, backup ------------------------

    /// Ids whose originals are absent from disk (external drive off, moved
    /// files). Pure filesystem probe, safe to run on demand.
    pub fn missing_ids(&self) -> Vec<i64> {
        self.all_photos()
            .into_iter()
            .filter(|p| !Path::new(&p.path).exists())
            .map(|p| p.id)
            .collect()
    }

    /// Point one photo at a new location. The file must exist and its
    /// blake3 must match the catalog record — a different file under the
    /// same name is refused with both hashes named.
    pub fn relink_photo(&self, id: i64, new_path: &Path) -> Result<(), String> {
        if !new_path.exists() {
            return Err(format!("nothing at {}", new_path.display()));
        }
        let stored: String = self
            .conn
            .query_row("SELECT blake3 FROM photos WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .map_err(|_| format!("photo {id} is not in the catalog"))?;
        let actual = hash_file(new_path)?;
        if actual != stored {
            return Err(format!(
                "content differs (catalog {}… vs file {}…) — reimport instead",
                &stored[..12.min(stored.len())],
                &actual[..12.min(actual.len())]
            ));
        }
        let filename = new_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        // V07: keep rows portable.
        let new_str = self.stored_form(new_path);
        self.conn
            .execute(
                "UPDATE photos SET path = ?1, filename = ?2 WHERE id = ?3",
                params![new_str, filename, id],
            )
            .map(|_| ())
            .map_err(|e| format!("relink: {e}"))
    }

    /// Batch-relink every missing photo under `old_abs` into `new_root`,
    /// matching relative paths and verifying each hash. Present files are
    /// untouched; nothing is reimported.
    pub fn relink_folder(&self, old_abs: &Path, new_root: &Path) -> FolderRelinkReport {
        let mut report = FolderRelinkReport::default();
        for p in self.all_photos() {
            // Component-wise: relinking `/Volumes/A` must not sweep in
            // `/Volumes/AB/…` rows (reported as bogus "missing").
            let Ok(rel) = Path::new(&p.path).strip_prefix(old_abs) else {
                continue;
            };
            if Path::new(&p.path).exists() {
                continue;
            }
            let candidate = new_root.join(rel);
            if !candidate.exists() {
                report.missing.push(p.filename.clone());
                continue;
            }
            match self.relink_photo(p.id, &candidate) {
                Ok(()) => report.linked += 1,
                Err(e) => report.mismatched.push(format!("{}: {e}", p.filename)),
            }
        }
        report
    }

    /// Catalog-managed move of one photo into another directory: same-name
    /// fast path via rename, verified copy + original delete across volumes
    /// (reuses the import verifier), sidecar follows in both cases.
    /// Same-directory moves are no-ops. Returns the new path.
    pub fn move_photo(&self, id: i64, dest_dir: &Path) -> Result<String, String> {
        let stored: String = self
            .conn
            .query_row("SELECT path FROM photos WHERE id = ?1", [id], |r| r.get(0))
            .map_err(|_| format!("photo {id} is not in the catalog"))?;
        // V07: rows may store root-relative paths; fs ops need absolute.
        let path = self.resolve_stored(&stored);
        if crate::apple_photos::in_library(&path) {
            return Err("stored in Apple Photos — Photos manages this file".to_string());
        }
        let old_path = PathBuf::from(&path);
        if !old_path.exists() {
            return Err(format!("original is missing: {}", old_path.display()));
        }
        if old_path.parent().is_some_and(|par| par == dest_dir) {
            return Ok(path);
        }
        let filename = old_path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| "bad filename".to_string())?;
        std::fs::create_dir_all(dest_dir)
            .map_err(|e| format!("create {}: {e}", dest_dir.display()))?;
        let dest = crate::import::unique_dest_name(dest_dir, filename);
        match std::fs::rename(&old_path, &dest) {
            Ok(()) => {}
            Err(e) if e.raw_os_error() == Some(libc::EXDEV) => {
                // Cross-volume: verified copy, then delete the originals.
                use std::sync::atomic::{AtomicBool, AtomicU64};
                let cancel = AtomicBool::new(false);
                let bytes = AtomicU64::new(0);
                crate::import::copy_verified(
                    &old_path,
                    dest_dir,
                    dest.file_name().and_then(|n| n.to_str()),
                    &cancel,
                    &bytes,
                )
                .map_err(|e| format!("copy for move: {e}"))?;
                let placed = dest_dir.join(
                    dest.file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or(filename),
                );
                std::fs::remove_file(&old_path).map_err(|e| {
                    format!(
                        "remove old original (copy is safe at {}): {e}",
                        placed.display()
                    )
                })?;
            }
            Err(e) => return Err(format!("move {}: {e}", old_path.display())),
        }
        // Sidecar follows when present.
        let old_side = crate::xmp::sidecar_path(&path);
        if Path::new(&old_side).exists() {
            let new_side = crate::xmp::sidecar_path(&dest.to_string_lossy());
            // Cross-volume moves reach here with the original already
            // copied + deleted: a plain rename would EXDEV every time and
            // the rollback rename below cannot undo that.
            let moved = match std::fs::rename(&old_side, &new_side) {
                Err(e) if e.raw_os_error() == Some(libc::EXDEV) => {
                    std::fs::copy(&old_side, &new_side)
                        .and_then(|_| std::fs::remove_file(&old_side))
                }
                other => other,
            };
            if let Err(e) = moved {
                let _ = std::fs::rename(&dest, &old_path);
                return Err(format!("move sidecar: {e}"));
            }
        }
        let abs_str = dest.to_string_lossy().to_string();
        let new_name = dest
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or(filename)
            .to_string();
        // V07: keep rows portable.
        let new_str = self.stored_form(Path::new(&abs_str));
        self.conn
            .execute(
                "UPDATE photos SET path = ?1, filename = ?2 WHERE id = ?3",
                params![new_str, new_name, id],
            )
            .map(|_| ())
            .map_err(|e| format!("save move: {e}"))?;
        Ok(abs_str)
    }

    /// Remove photo rows (photo, edits, keywords, sidecar state, queue,
    /// snapshots). Files on disk are untouched — use Trash for those.
    pub fn remove_photo(&self, id: i64) -> Result<(), String> {
        for table in [
            "photos",
            "edits",
            "sidecar_state",
            "sync_queue",
            "snapshots",
        ] {
            let col = if table == "photos" { "id" } else { "photo_id" };
            self.conn
                .execute(&format!("DELETE FROM {table} WHERE {col} = ?1"), [id])
                .map_err(|e| format!("remove from catalog: {e}"))?;
        }
        self.conn
            .execute("DELETE FROM keywords WHERE photo_id = ?1", [id])
            .map(|_| ())
            .map_err(|e| format!("remove from catalog: {e}"))
    }

    /// V15: full row snapshot for undoable Remove (photo + persisted
    /// edits + keywords + named snapshots). Sidecar/sync-queue state
    /// rebuilds on next scan — rows, not caches, are what undo restores.
    pub fn snapshot_removed(&self, id: i64) -> Option<RemovedPhoto> {
        // Indexed single-row read (a full `all_photos` scan per removed id
        // made batch removes quadratic).
        let photo = self.photo_by_id(id)?;
        // Never-edited photos have no `edits` row: that is "no edits", not
        // "no snapshot" (callers only remove photos they could snapshot).
        let (params_json, history_json, cursor) = self
            .conn
            .query_row(
                "SELECT params_json, history_json, cursor FROM edits WHERE photo_id = ?1",
                [id],
                |r| {
                    Ok((
                        r.get::<_, Option<String>>(0)?,
                        r.get::<_, Option<String>>(1)?,
                        r.get::<_, Option<i64>>(2)?.unwrap_or(0),
                    ))
                },
            )
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok((None, None, 0)),
                other => Err(other),
            })
            .ok()?;
        let keywords = self.photo_keywords(id);
        let snapshots: Vec<(String, String)> = self
            .conn
            .prepare("SELECT name, state_json FROM snapshots WHERE photo_id = ?1 ORDER BY id")
            .and_then(|mut s| {
                s.query_map([id], |r| Ok((r.get(0)?, r.get(1)?)))
                    .and_then(|rows| rows.collect::<Result<Vec<_>, _>>())
            })
            .unwrap_or_default();
        Some(RemovedPhoto {
            photo,
            params_json,
            history_json,
            cursor,
            keywords,
            snapshots,
        })
    }

    /// V15: restore a snapshot (Remove undo). Reuses the original id when
    /// free; otherwise inserts fresh (a reimport may own it now) and
    /// returns the final id. Missing originals restore as missing rows —
    /// callers skip those loudly instead.
    pub fn restore_removed(&self, r: &RemovedPhoto) -> Result<i64, String> {
        let taken: bool = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM photos WHERE id = ?1",
                [r.photo.id],
                |row| row.get::<_, i64>(0),
            )
            .map(|n| n > 0)
            .unwrap_or(true);
        let sync = match r.photo.sync {
            SyncState::Local => "local",
            SyncState::Pending => "pending",
            SyncState::Synced => "synced",
            SyncState::Failed => "failed",
        };
        let p = &r.photo;
        // Snapshots carry the resolved absolute path; V07 rows store the
        // root-relative form so restored photos stay portable.
        let stored_path = self.stored_form(Path::new(&p.path));
        let id: i64 = if taken {
            self.conn
                .execute(
                    "INSERT INTO photos(catalog_id, path, filename, blake3, captured_at, camera, lens,
                     focal_mm, aperture, shutter, iso, width, height, rating, picked, rejected,
                     sync_state, remote_key, creator, copyright, rights, contact, captured_orig,
                     capture_offset_min, duration_ms, codec, title, caption, headline, location,
                     exif_program, exif_metering, exif_flash, exif_focal35, exif_serial,
                     exif_firmware, exif_gps)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,
                     ?20,?21,?22,?23,?24,?25,?26,?27,?28,?29,?30,?31,?32,?33,?34,?35,?36,?37)",
                    params![
                        self.id, stored_path, p.filename, p.blake3, p.captured_at, p.camera, p.lens,
                        p.focal_mm, p.aperture, p.shutter, p.iso, p.width, p.height, p.rating,
                        p.picked as u8, p.rejected as u8, sync, p.remote_key, p.creator,
                        p.copyright, p.rights, p.contact, p.captured_orig, p.capture_offset_min,
                        p.duration_ms, p.codec, p.title, p.caption, p.headline, p.location,
                        p.exif_program, p.exif_metering, p.exif_flash, p.exif_focal35, p.exif_serial,
                        p.exif_firmware, p.exif_gps,
                    ],
                )
                .map_err(|e| format!("restore photo: {e}"))?;
            self.conn.last_insert_rowid()
        } else {
            self.conn
                .execute(
                    "INSERT INTO photos(id, catalog_id, path, filename, blake3, captured_at, camera, lens,
                     focal_mm, aperture, shutter, iso, width, height, rating, picked, rejected,
                     sync_state, remote_key, creator, copyright, rights, contact, captured_orig,
                     capture_offset_min, duration_ms, codec, title, caption, headline, location,
                     exif_program, exif_metering, exif_flash, exif_focal35, exif_serial,
                     exif_firmware, exif_gps)
                     VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,
                     ?19,?20,?21,?22,?23,?24,?25,?26,?27,?28,?29,?30,?31,?32,?33,?34,?35,
                     ?36,?37,?38)",
                    params![
                        p.id, self.id, stored_path, p.filename, p.blake3, p.captured_at, p.camera,
                        p.lens, p.focal_mm, p.aperture, p.shutter, p.iso, p.width, p.height,
                        p.rating, p.picked as u8, p.rejected as u8, sync, p.remote_key, p.creator,
                        p.copyright, p.rights, p.contact, p.captured_orig, p.capture_offset_min,
                        p.duration_ms, p.codec, p.title, p.caption, p.headline, p.location,
                        p.exif_program, p.exif_metering, p.exif_flash, p.exif_focal35, p.exif_serial,
                        p.exif_firmware, p.exif_gps,
                    ],
                )
                .map_err(|e| format!("restore photo: {e}"))?;
            p.id
        };
        // Legacy rows carry params with a NULL history: still restore them.
        if let Some(pj) = r.params_json.clone() {
            let hj = r.history_json.clone();
            self.conn
                .execute(
                    "INSERT INTO edits(photo_id, params_json, history_json, cursor, updated_at)
                     VALUES (?1,?2,?3,?4,?5)
                     ON CONFLICT(photo_id) DO UPDATE SET params_json = ?2, history_json = ?3,
                     cursor = ?4, updated_at = ?5",
                    params![id, pj, hj, r.cursor, chrono_stamp()],
                )
                .map_err(|e| format!("restore edits: {e}"))?;
        }
        self.set_keywords(id, &r.keywords)?;
        for (name, state_json) in &r.snapshots {
            self.conn
                .execute(
                    "INSERT INTO snapshots(photo_id, name, state_json, created_at)
                     VALUES (?1,?2,?3,?4)",
                    params![id, name, state_json, chrono_stamp()],
                )
                .map_err(|e| format!("restore snapshots: {e}"))?;
        }
        Ok(id)
    }

    /// U21: drop one photo's preview cache dir when no remaining row
    /// references its hash. Cache dirs are content-addressed (blake3),
    /// so a hash shared with another photo (duplicate content) survives.
    pub fn drop_preview_cache(&self, cache_dir: &Path, hash: &str) {
        if hash.is_empty() {
            return;
        }
        let still: i64 = self
            .conn
            .query_row(
                "SELECT COUNT(*) FROM photos WHERE blake3 = ?1",
                [hash],
                |r| r.get(0),
            )
            .unwrap_or(1);
        if still == 0 {
            std::fs::remove_dir_all(cache_dir.join(hash)).ok();
        }
    }

    /// U21: prune cache dirs no photo references (stale removals, old
    /// V08: preview cache dirs no photo references (cap 64 samples).
    pub fn list_orphan_previews(&self, cache_dir: &Path) -> Vec<String> {
        let known: std::collections::HashSet<String> = self
            .conn
            .prepare("SELECT DISTINCT blake3 FROM photos")
            .and_then(|mut s| {
                s.query_map([], |r| r.get::<_, String>(0))?
                    .collect::<Result<std::collections::HashSet<String>, _>>()
            })
            .unwrap_or_default();
        let entries = std::fs::read_dir(cache_dir)
            .map(|rd| rd.filter_map(|e| e.ok()).collect::<Vec<_>>())
            .unwrap_or_default();
        let mut orphans = Vec::new();
        for e in entries {
            let name = e.file_name().to_string_lossy().to_string();
            if e.path().is_dir()
                && !known.contains(&name)
                && name.len() == 64
                && name.bytes().all(|b| b.is_ascii_hexdigit())
            {
                orphans.push(name);
                if orphans.len() >= 64 {
                    break;
                }
            }
        }
        orphans
    }

    /// U21: prune cache dirs no photo references (stale removals, old
    /// bugs). Returns removed count. Loops the capped listing until
    /// clean (a failed removal stops the sweep).
    pub fn prune_preview_cache(&self, cache_dir: &Path) -> usize {
        let mut removed = 0;
        loop {
            let batch = self.list_orphan_previews(cache_dir);
            if batch.is_empty() {
                break;
            }
            let mut failed = false;
            for name in batch {
                if std::fs::remove_dir_all(cache_dir.join(&name)).is_ok() {
                    removed += 1;
                } else {
                    failed = true;
                    break;
                }
            }
            if failed {
                break;
            }
        }
        removed
    }

    /// Timestamped backup copy beside the live file (after a WAL
    /// checkpoint so the single file is complete). Returns its path.
    pub fn backup_db(&self, backup_dir: &Path) -> Result<PathBuf, String> {
        self.conn
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .map_err(|e| format!("checkpoint: {e}"))?;
        std::fs::create_dir_all(backup_dir)
            .map_err(|e| format!("create {}: {e}", backup_dir.display()))?;
        let stamp = chrono_stamp();
        let dest = backup_dir.join(format!("laika-{stamp}.db"));
        std::fs::copy(&self.db_path, &dest).map_err(|e| format!("copy backup: {e}"))?;
        Ok(dest)
    }

    /// Validate a backup file before restoring it: readable SQLite with an
    /// intact page structure and the catalog tables present.
    pub fn probe_backup(path: &Path) -> Result<BackupProbe, String> {
        use rusqlite::OpenFlags;
        let conn = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(|e| format!("open {}: {e}", path.display()))?;
        let verdict: String = conn
            .query_row("PRAGMA integrity_check", [], |r| r.get(0))
            .map_err(|e| format!("integrity check failed: {e}"))?;
        if verdict != "ok" {
            return Err(format!("integrity check failed: {verdict}"));
        }
        for table in ["catalogs", "photos", "edits"] {
            // COUNT (not LIMIT 1): empty tables are valid, unreadable
            // ones are damage.
            conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| {
                r.get::<_, i64>(0)
            })
            .map_err(|e| {
                let missing: bool = conn
                    .prepare("SELECT 1 FROM sqlite_master WHERE name = ?1")
                    .and_then(|mut s| s.query_row([table], |_| Ok(())))
                    .is_err();
                if missing {
                    format!("not a Laika catalog (missing {table})")
                } else {
                    format!("damaged catalog ({table} unreadable: {e})")
                }
            })?;
        }
        let photos: i64 = conn
            .query_row("SELECT COUNT(*) FROM photos", [], |r| r.get(0))
            .unwrap_or(0);
        let bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        Ok(BackupProbe { photos, bytes })
    }

    /// V08: full check (pages, orphan rows, orphan previews). Page
    /// damage short-circuits — row counts on a torn file would lie.
    pub fn integrity_report(&self, cache_dir: &Path) -> IntegrityReport {
        let pages: String = self
            .conn
            .query_row("PRAGMA integrity_check", [], |r| r.get(0))
            .unwrap_or_else(|e| format!("check failed: {e}"));
        let mut rep = IntegrityReport {
            pages,
            ..Default::default()
        };
        if rep.damaged() {
            rep.line = format!("integrity FAILED: {}", rep.pages);
            return rep;
        }
        let orphans = |table: &str, col: &str| -> i64 {
            self.conn
                .query_row(
                    &format!(
                        "SELECT COUNT(*) FROM {table} WHERE {col} NOT IN (SELECT id FROM photos)"
                    ),
                    [],
                    |r| r.get(0),
                )
                .unwrap_or(0)
        };
        rep.orphan_edits = orphans("edits", "photo_id");
        rep.orphan_keywords = orphans("keywords", "photo_id");
        rep.orphan_sidecar_state = orphans("sidecar_state", "photo_id");
        rep.orphan_sync_queue = orphans("sync_queue", "photo_id");
        rep.orphan_snapshots = orphans("snapshots", "photo_id");
        rep.orphan_previews = self.list_orphan_previews(cache_dir);
        rep.line = if rep.orphans() == 0 && rep.orphan_previews.is_empty() {
            "ok — no orphaned rows or previews".to_string()
        } else {
            format!(
                "ok, {} orphaned row{} + {} orphaned preview{}",
                rep.orphans(),
                if rep.orphans() == 1 { "" } else { "s" },
                rep.orphan_previews.len(),
                if rep.orphan_previews.len() == 1 {
                    ""
                } else {
                    "s"
                },
            )
        };
        rep
    }

    /// V08: optimize on a standalone connection (the UI calls this off
    /// the main thread — the live catalog keeps its own connection).
    /// VACUUM rebuilds (reclaims freelist), ANALYZE refreshes the
    /// planner, checkpoint truncates the WAL. Journal mode is restored
    /// defensively; a busy catalog reports instead of waiting forever.
    pub fn optimize_db(db_path: &Path) -> Result<OptimizeReport, String> {
        let before = std::fs::metadata(db_path).map(|m| m.len()).unwrap_or(0);
        let conn = Connection::open(db_path).map_err(|e| format!("open: {e}"))?;
        conn.execute_batch("PRAGMA busy_timeout=5000;")
            .map_err(|e| e.to_string())?;
        conn.execute_batch("VACUUM;")
            .map_err(|e| format!("vacuum: {e} (catalog busy — retry when idle)"))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; ANALYZE; PRAGMA wal_checkpoint(TRUNCATE);")
            .map_err(|e| format!("analyze: {e}"))?;
        drop(conn);
        let after = std::fs::metadata(db_path).map(|m| m.len()).unwrap_or(0);
        Ok(OptimizeReport {
            before,
            after,
            reclaimed: before as i64 - after as i64,
        })
    }

    /// Full integrity report: page structure plus orphaned rows that
    /// reference missing photos.
    pub fn integrity_check(&self) -> Result<String, String> {
        let verdict: String = self
            .conn
            .query_row("PRAGMA integrity_check", [], |r| r.get(0))
            .map_err(|e| format!("integrity check failed: {e}"))?;
        if verdict != "ok" {
            return Err(format!("integrity check failed: {verdict}"));
        }
        let mut orphans = 0;
        for (table, col) in [
            ("edits", "photo_id"),
            ("keywords", "photo_id"),
            ("sidecar_state", "photo_id"),
            ("sync_queue", "photo_id"),
            ("snapshots", "photo_id"),
        ] {
            let n: i64 = self
                .conn
                .query_row(
                    &format!(
                        "SELECT COUNT(*) FROM {table} WHERE {col} NOT IN (SELECT id FROM photos)"
                    ),
                    [],
                    |r| r.get(0),
                )
                .unwrap_or(0);
            orphans += n;
        }
        if orphans > 0 {
            Ok(format!("ok, {orphans} orphaned rows"))
        } else {
            Ok("ok — no orphaned rows".to_string())
        }
    }

    /// Restore from a validated backup: current file is preserved first,
    /// then replaced. The caller reopens the catalog afterwards.
    pub fn restore_db(&self, backup: &Path, backup_dir: &Path) -> Result<(), String> {
        Self::probe_backup(backup)?;
        self.conn
            .execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .map_err(|e| format!("checkpoint: {e}"))?;
        std::fs::create_dir_all(backup_dir)
            .map_err(|e| format!("create {}: {e}", backup_dir.display()))?;
        let stamp = chrono_stamp();
        let safety = backup_dir.join(format!("laika-pre-restore-{stamp}.db"));
        std::fs::copy(&self.db_path, &safety)
            .map_err(|e| format!("preserve current catalog: {e}"))?;
        std::fs::copy(backup, &self.db_path).map_err(|e| format!("restore catalog: {e}"))?;
        Ok(())
    }

    // ---- V04 folders: create/rename/delete/sync/watch --------------------------

    /// Validate a new folder name (no separators, non-empty, fits the fs).
    pub fn check_folder_name(name: &str) -> Result<(), String> {
        let t = name.trim();
        if t.is_empty() {
            return Err("name the folder first".to_string());
        }
        if t.len() > 200 {
            return Err("keep folder names under 200 characters".to_string());
        }
        if t.contains(['/', '\\', '\0']) || t.chars().any(|c| c.is_control()) {
            return Err("folder names can't contain / or \\".to_string());
        }
        if ["..", "."].contains(&t) {
            return Err(format!("{t:?} is not a usable folder name"));
        }
        Ok(())
    }

    /// Create a subfolder on disk (appears in the tree on next refresh).
    pub fn create_folder(&self, parent: &Path, name: &str) -> Result<PathBuf, String> {
        Self::refuse_photos_library(parent)?;
        Self::check_folder_name(name)?;
        let dir = parent.join(name.trim());
        if dir.exists() {
            return Err(format!("{} already exists", dir.display()));
        }
        std::fs::create_dir_all(&dir).map_err(|e| format!("create folder: {e}"))?;
        Ok(dir)
    }

    /// Rename a folder, updating every descendant catalog path in one
    /// transaction. Sidecars ride along on disk (same directory move).
    /// Same-parent renames only — relocation is Move's job.
    pub fn rename_folder(&self, old_abs: &Path, new_name: &str) -> Result<PathBuf, String> {
        Self::refuse_photos_library(old_abs)?;
        Self::check_folder_name(new_name)?;
        if !old_abs.is_dir() {
            return Err(format!("folder is gone: {}", old_abs.display()));
        }
        let parent = old_abs
            .parent()
            .ok_or_else(|| "folder has no parent".to_string())?;
        let new_abs = parent.join(new_name.trim());
        if new_abs.exists() {
            return Err(format!("{} already exists", new_abs.display()));
        }
        std::fs::rename(old_abs, &new_abs).map_err(|e| format!("rename folder: {e}"))?;
        // One transaction for every descendant row: prefix rewrite.
        // Rows may store either form, so both prefixes are rewritten.
        let old_prefix = format!("{}/", old_abs.to_string_lossy());
        let new_prefix = format!("{}/", new_abs.to_string_lossy());
        let root = self.root_path();
        let rel_old = old_abs
            .strip_prefix(&root)
            .ok()
            .filter(|r| !r.as_os_str().is_empty())
            .map(|r| format!("{}/", r.to_string_lossy()));
        let rel_new = new_abs
            .strip_prefix(&root)
            .ok()
            .filter(|r| !r.as_os_str().is_empty())
            .map(|r| format!("{}/", r.to_string_lossy()));
        // LIKE metacharacters in real paths must match literally.
        let esc = |s: &str| {
            s.replace('\\', "\\\\")
                .replace('%', "\\%")
                .replace('_', "\\_")
        };
        if let Err(e) = self.conn.execute_batch("BEGIN IMMEDIATE;") {
            // Catalog untouched: put the directory back so rows still resolve.
            let _ = std::fs::rename(&new_abs, old_abs);
            return Err(format!("rename folder: {e}"));
        }
        let mut rewrite = |old: &str, new: &str| -> Result<(), String> {
            self.conn
                .execute(
                    // LIKE is ASCII case-insensitive: the exact substr()
                    // check keeps `/p/trip/…` rows when renaming `/p/Trip`
                    // on case-sensitive volumes.
                    "UPDATE photos SET path = ?1 || substr(path, ?2) WHERE path LIKE ?3 ESCAPE '\\'
                     AND substr(path, 1, ?2 - 1) = ?4",
                    // substr() counts characters, not bytes (Unicode paths).
                    rusqlite::params![
                        new,
                        old.chars().count() as i64 + 1,
                        format!("{}%", esc(old)),
                        old
                    ],
                )
                .map(|_| ())
                .map_err(|e| format!("rename folder: {e}"))
        };
        let updated = rewrite(&old_prefix, &new_prefix)
            .and_then(|_| match (&rel_old, &rel_new) {
                (Some(o), Some(n)) => rewrite(o, n),
                _ => Ok(()),
            })
            // A failed COMMIT must roll back too, or the connection stays
            // inside an open transaction and later writes never persist.
            .and_then(|_| {
                self.conn
                    .execute_batch("COMMIT;")
                    .map_err(|e| format!("rename folder: {e}"))
            });
        match updated {
            Ok(()) => {
                // Keep the rewritten rows portable.
                self.normalize_prefix(&new_prefix.trim_end_matches('/'));
            }
            Err(e) => {
                let _ = self.conn.execute_batch("ROLLBACK;");
                // Best effort: roll the directory back too.
                let _ = std::fs::rename(&new_abs, old_abs);
                return Err(e);
            }
        }
        Ok(new_abs)
    }

    /// Delete an empty folder from disk. Refuses non-empty directories so
    /// photo removal always goes through the explicit Remove/Trash flows.
    pub fn delete_folder(&self, dir: &Path) -> Result<(), String> {
        Self::refuse_photos_library(dir)?;
        if !dir.is_dir() {
            return Err(format!("folder is gone: {}", dir.display()));
        }
        let mut entries = std::fs::read_dir(dir)
            .map_err(|e| format!("read folder: {e}"))?
            .peekable();
        if entries.peek().is_some() {
            return Err("folder still has files — remove or trash its photos first".to_string());
        }
        std::fs::remove_dir(dir).map_err(|e| format!("delete folder: {e}"))?;
        Ok(())
    }

    /// Compare one disk subtree against the catalog for Synchronize:
    /// new files, missing entries, and externally changed sidecars.
    pub fn folder_diff(&self, dir: &Path) -> FolderDiff {
        let mut diff = FolderDiff::default();
        if !dir.is_dir() {
            diff.error = Some(format!("folder is gone: {}", dir.display()));
            return diff;
        }
        let dir_s = dir.to_string_lossy().to_string();
        let under = |p: &str| p == dir_s || p.starts_with(&format!("{dir_s}/"));
        let on_disk = laika_raw::scan_dir(dir);
        let known: std::collections::HashMap<String, (i64, String)> = self
            .all_photos()
            .into_iter()
            .filter(|p| under(&p.path))
            .map(|p| (p.path.clone(), (p.id, p.blake3.clone())))
            .collect();
        for path in &on_disk {
            let s = path.to_string_lossy().to_string();
            if !known.contains_key(&s) {
                diff.new_files.push(path.clone());
            }
        }
        // Hash lookup: a linear scan per known row (with a lossy string
        // allocation per comparison) was O(rows × files) on big folders.
        let on_disk_set: std::collections::HashSet<String> = on_disk
            .iter()
            .map(|p| p.to_string_lossy().to_string())
            .collect();
        for (path, (id, _)) in &known {
            if !on_disk_set.contains(path) {
                let name = Path::new(path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("?")
                    .to_string();
                diff.missing.push((*id, name));
            }
        }
        // Changed sidecars: newer than our last write (or than the last
        // acknowledged save when we never recorded one).
        for (path, (id, _)) in &known {
            if !Path::new(path).exists() {
                continue;
            }
            let mtime = crate::xmp::sidecar_mtime(path).unwrap_or(0);
            if mtime == 0 {
                continue;
            }
            let changed = match self.sidecar_written_at(*id) {
                Some(w) => mtime > w + 1,
                None => match self.edits_updated_at(*id) {
                    Some(u) => mtime + 2 >= u,
                    None => false,
                },
            };
            if changed {
                let name = Path::new(path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("?")
                    .to_string();
                diff.changed.push((*id, name));
            }
        }
        diff.new_files.sort();
        diff.missing.sort();
        diff.changed.sort();
        diff
    }

    /// Watched folders, in added order.
    pub fn list_watched(&self) -> Vec<String> {
        let mut stmt = match self
            .conn
            .prepare("SELECT path FROM watched_folders ORDER BY added_at, path")
        {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        stmt.query_map([], |r| r.get(0))
            .map(|rows| rows.flatten().collect())
            .unwrap_or_default()
    }

    pub fn watch_folder(&self, path: &Path) -> Result<(), String> {
        if !path.is_dir() {
            return Err(format!("not a folder: {}", path.display()));
        }
        self.conn
            .execute(
                "INSERT OR IGNORE INTO watched_folders(path, added_at) VALUES (?1,?2)",
                params![path.to_string_lossy().to_string(), chrono_stamp()],
            )
            .map(|_| ())
            .map_err(|e| format!("watch folder: {e}"))
    }

    pub fn unwatch_folder(&self, path: &Path) {
        self.conn
            .execute(
                "DELETE FROM watched_folders WHERE path = ?1",
                [path.to_string_lossy().to_string()],
            )
            .ok();
    }

    // ---- U14 snapshots ----------------------------------------------------------

    /// Saved snapshot (full photo state as JSON).
    pub fn save_snapshot(
        &self,
        photo_id: i64,
        name: &str,
        snap: &crate::edit::Snap,
    ) -> Result<i64, String> {
        let json = serde_json::to_string(&crate::edit::HistoryJson {
            label: name.into(),
            value: String::new(),
            params: snap.params.to_vec(),
            crop: snap.crop,
            geom: snap.geom,
            curve_on: snap.curve_on,
            hsl_on: snap.hsl_on,
            detail_on: snap.detail_on,
            optics_on: snap.optics_on,
            effects_on: snap.effects_on,
            grading_on: snap.grading_on,
            rating: snap.rating,
            picked: snap.picked,
            rejected: snap.rejected,
        })
        .map_err(|e| format!("save snapshot: {e}"))?;
        // Same-name snapshots replace (one row per treatment name).
        self.conn
            .execute(
                "DELETE FROM snapshots WHERE photo_id = ?1 AND name = ?2",
                params![photo_id, name],
            )
            .map_err(|e| format!("save snapshot: {e}"))?;
        self.conn
            .execute(
                "INSERT INTO snapshots(photo_id, name, state_json, created_at)
                 VALUES (?1,?2,?3,?4)",
                params![photo_id, name, json, chrono_stamp()],
            )
            .map_err(|e| format!("save snapshot: {e}"))?;
        Ok(self.conn.last_insert_rowid())
    }

    /// Snapshots for one photo, oldest first: (row id, name, state).
    pub fn list_snapshots(&self, photo_id: i64) -> Vec<(i64, String, crate::edit::Snap)> {
        let mut stmt = match self
            .conn
            .prepare("SELECT id, name, state_json FROM snapshots WHERE photo_id = ?1 ORDER BY id")
        {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let rows = match stmt.query_map([photo_id], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
            ))
        }) {
            Ok(r) => r,
            Err(_) => return Vec::new(),
        };
        rows.flatten()
            .filter_map(|(id, name, json)| {
                let j: crate::edit::HistoryJson = serde_json::from_str(&json).ok()?;
                let Some(params) = crate::edit::pad_params(&j.params) else {
                    return None;
                };
                Some((
                    id,
                    name,
                    crate::edit::Snap {
                        params,
                        crop: j.crop,
                        geom: j.geom,
                        curve_on: j.curve_on,
                        hsl_on: j.hsl_on,
                        detail_on: j.detail_on,
                        optics_on: j.optics_on,
                        effects_on: j.effects_on,
                        grading_on: j.grading_on,
                        rating: j.rating,
                        picked: j.picked,
                        rejected: j.rejected,
                    },
                ))
            })
            .collect()
    }

    pub fn delete_snapshot(&self, id: i64) {
        self.conn
            .execute("DELETE FROM snapshots WHERE id = ?1", [id])
            .ok();
    }
}

fn row_to_photo(r: &rusqlite::Row) -> rusqlite::Result<DbPhoto> {
    // V08: every column but the identity reads NULL-tolerant — legacy
    // files carry NULLs where modern inserts write empty strings, and
    // dropping those rows would lose photos silently.
    let opt = |i: usize| -> String {
        r.get::<_, Option<String>>(i)
            .unwrap_or_default()
            .unwrap_or_default()
    };
    let opt_i = |i: usize| -> i64 { r.get::<_, Option<i64>>(i).unwrap_or_default().unwrap_or(0) };
    let sync_str = opt(17);
    Ok(DbPhoto {
        id: r.get(0)?,
        catalog_id: r.get(1)?,
        path: opt(2),
        filename: opt(3),
        blake3: opt(4),
        captured_at: opt(5),
        camera: opt(6),
        lens: opt(7),
        focal_mm: opt(8),
        aperture: opt(9),
        shutter: opt(10),
        iso: opt(11),
        width: opt_i(12) as u32,
        height: opt_i(13) as u32,
        rating: opt_i(14) as u8,
        picked: opt_i(15) != 0,
        rejected: opt_i(16) != 0,
        sync: match sync_str.as_str() {
            "pending" => SyncState::Pending,
            "synced" => SyncState::Synced,
            "failed" => SyncState::Failed,
            _ => SyncState::Local,
        },
        remote_key: r.get::<_, Option<String>>(18)?.unwrap_or_default(),
        creator: r.get::<_, Option<String>>(19)?.unwrap_or_default(),
        copyright: r.get::<_, Option<String>>(20)?.unwrap_or_default(),
        rights: r.get::<_, Option<String>>(21)?.unwrap_or_default(),
        contact: r.get::<_, Option<String>>(22)?.unwrap_or_default(),
        captured_orig: r.get::<_, Option<String>>(23)?.unwrap_or_default(),
        capture_offset_min: r.get::<_, Option<i64>>(24)?.unwrap_or(0) as i32,
        duration_ms: r.get::<_, Option<i64>>(25)?.unwrap_or(0),
        codec: r.get::<_, Option<String>>(26)?.unwrap_or_default(),
        // V12: new columns read NULL-tolerant (legacy rows predate them).
        title: r.get::<_, Option<String>>(27)?.unwrap_or_default(),
        caption: r.get::<_, Option<String>>(28)?.unwrap_or_default(),
        headline: r.get::<_, Option<String>>(29)?.unwrap_or_default(),
        location: r.get::<_, Option<String>>(30)?.unwrap_or_default(),
        exif_program: r.get::<_, Option<String>>(31)?.unwrap_or_default(),
        exif_metering: r.get::<_, Option<String>>(32)?.unwrap_or_default(),
        exif_flash: r.get::<_, Option<String>>(33)?.unwrap_or_default(),
        exif_focal35: r.get::<_, Option<String>>(34)?.unwrap_or_default(),
        exif_serial: r.get::<_, Option<String>>(35)?.unwrap_or_default(),
        exif_firmware: r.get::<_, Option<String>>(36)?.unwrap_or_default(),
        exif_gps: r.get::<_, Option<String>>(37)?.unwrap_or_default(),
    })
}

/// A file whose hashing, EXIF read and preview renders finished on a
/// background thread; the UI thread owns cache writes + DB insert.
pub struct PreparedFile {
    pub path_str: String,
    pub hash: String,
    pub meta: laika_raw::exif::FileMeta,
    pub small_jpeg: Vec<u8>,
    pub large_jpeg: Vec<u8>,
}

/// V05: prepare a video file without any still decoder: container metadata
/// for the row, best-effort poster derivatives, empty JPEGs when no poster
/// helper answers (the UI falls back to its placeholder tile).
pub fn prepare_video_file(path: &Path, hash: &str) -> Result<PreparedFile, String> {
    let vm = laika_raw::video::read(path).unwrap_or_default();
    let mut meta = laika_raw::exif::FileMeta::default();
    meta.captured_at = if vm.captured_at.is_empty() {
        mtime_exif(path)
    } else {
        vm.captured_at
    };
    meta.width = vm.width;
    meta.height = vm.height;
    meta.duration_ms = vm.duration_ms;
    meta.codec = vm.codec;
    let (small_jpeg, large_jpeg) = match laika_raw::video::poster_for(path) {
        Some(poster) => match image::load_from_memory(&poster) {
            Ok(img) => (
                laika_raw::preview::derivative_jpeg(&img, laika_raw::preview::PREVIEW_SMALL),
                laika_raw::preview::derivative_jpeg(&img, laika_raw::preview::PREVIEW_LARGE),
            ),
            Err(_) => (Vec::new(), Vec::new()),
        },
        None => (Vec::new(), Vec::new()),
    };
    Ok(PreparedFile {
        path_str: path.to_string_lossy().to_string(),
        hash: hash.to_string(),
        meta,
        small_jpeg,
        large_jpeg,
    })
}

/// V05: file mtime in EXIF shape (`YYYY:MM:DD HH:MM:SS`, UTC) for rows
/// whose container carries no capture time (videos sort by it).
pub fn mtime_exif(path: &Path) -> String {
    let secs = std::fs::metadata(path)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    if secs <= 0 {
        return String::new();
    }
    let days = secs.div_euclid(86400);
    let sec = secs.rem_euclid(86400);
    let z = days + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    if m <= 2 {
        y += 1;
    }
    format!(
        "{y:04}:{m:02}:{d:02} {:02}:{:02}:{:02}",
        sec / 3600,
        sec / 60 % 60,
        sec % 60
    )
}

/// blake3 hex of the whole file.
pub fn hash_file(path: &Path) -> Result<String, String> {
    let mut hasher = blake3::Hasher::new();
    let mut f = std::fs::File::open(path).map_err(|e| e.to_string())?;
    std::io::copy(&mut f, &mut hasher).map_err(|e| e.to_string())?;
    Ok(hasher.finalize().to_hex().to_string())
}

fn chrono_stamp() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default()
}

/// Edit payload stored in the `edits` table (object form; Phase 2 wrote a
/// bare params array, which still reads).
#[derive(serde::Serialize, serde::Deserialize)]
struct EditJson {
    #[serde(default)]
    params: Vec<f32>,
    #[serde(default)]
    crop: Option<f32>,
    /// U08: absent in old rows → full-frame default.
    #[serde(default)]
    geom: crate::edit::CropGeom,
    /// U18: absent in old rows → active.
    #[serde(default = "crate::edit::flag_on")]
    curve_on: bool,
    #[serde(default = "crate::edit::flag_on")]
    hsl_on: bool,
    #[serde(default = "crate::edit::flag_on")]
    detail_on: bool,
    #[serde(default = "crate::edit::flag_on")]
    optics_on: bool,
    #[serde(default = "crate::edit::flag_on")]
    effects_on: bool,
    #[serde(default = "crate::edit::flag_on")]
    grading_on: bool,
}

/// Default catalog + cache locations (macOS app-support, XDG cache elsewhere).
pub fn default_dirs() -> (PathBuf, PathBuf) {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    if cfg!(target_os = "macos") {
        (
            PathBuf::from(format!(
                "{home}/Library/Application Support/Laika/catalog.db"
            )),
            PathBuf::from(format!("{home}/Library/Application Support/Laika/cache")),
        )
    } else {
        let base = std::env::var("XDG_CACHE_HOME").unwrap_or_else(|_| format!("{home}/.cache"));
        (
            PathBuf::from(format!("{base}/laika/catalog.db")),
            PathBuf::from(format!("{base}/laika/cache")),
        )
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn backup_destination_change_requeues() {
        let dir = workdir("dest-rebase");
        jpeg(&dir.join("a.jpg"), 32, 24);
        let cat = Catalog::open(&dir.join("c.db"), "c", &dir).unwrap();
        let id = cat
            .import_file(&dir.join("a.jpg"), &dir.join("cache"))
            .unwrap()
            .unwrap();
        cat.set_sync(id, crate::photo::SyncState::Synced);
        assert!(!cat.rebase_backup_destination("/Volumes/nas"));
        assert!(!cat.rebase_backup_destination("/Volumes/nas"));
        assert!(cat.rebase_backup_destination("sftp://box/srv"));
        let p = cat.photo_by_id(id).unwrap();
        assert_eq!(p.sync, crate::photo::SyncState::Local);
        assert_eq!(cat.enqueue_unsynced(), 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn stale_lock_survives_hostname_drift() {
        assert!(super::same_host("mbp.local", "MBP"));
        assert!(!super::same_host("mbp.local", "MacBookPro"));
        let dir = std::env::temp_dir().join(format!("laika-lock-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = dir.join("c.db");
        // A dead pid recorded under an old hostname must not lock us out
        // of a catalog on a local disk.
        std::fs::write(super::lock_path(&db), "999999\nold-hostname.local\n0").unwrap();
        if cfg!(target_os = "macos") {
            let lock = super::CatalogLock::acquire(&db).expect("stale lock taken over");
            drop(lock);
            assert!(!super::lock_path(&db).exists());
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    use super::*;

    fn workdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("laika-cat-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn jpeg(path: &Path, w: u32, h: u32) {
        let mut img = image_like(w, h);
        img.save(path).unwrap();
    }

    fn filetime_set(path: &Path, secs: u64) {
        use std::time::{Duration, UNIX_EPOCH};
        let t = filetime::FileTime::from_system_time(UNIX_EPOCH + Duration::from_secs(secs));
        filetime::set_file_mtime(path, t).unwrap();
    }

    fn image_like(w: u32, h: u32) -> image::DynamicImage {
        image::DynamicImage::ImageRgb8(image::RgbImage::from_fn(w, h, |x, y| {
            image::Rgb([(x % 256) as u8, (y % 256) as u8, 128])
        }))
    }

    #[test]
    fn import_roundtrip_and_filters() {
        let dir = workdir("rt");
        let sub = dir.join("shoot");
        std::fs::create_dir_all(&sub).unwrap();
        jpeg(&sub.join("a.jpg"), 120, 80);
        jpeg(&dir.join("b.jpg"), 60, 40);
        std::fs::write(dir.join("note.txt"), b"x").unwrap();

        let found = laika_raw::scan_dir(&dir);
        assert_eq!(found.len(), 2);

        let cat = Catalog::open(&dir.join("cat.db"), "Test", &dir).unwrap();
        let cache = dir.join("cache");
        let mut ids = Vec::new();
        for p in &found {
            ids.push(cat.import_file(p, &cache).unwrap());
        }
        assert!(ids.iter().all(|i| i.is_some()));
        assert_eq!(cat.photo_count(), 2);
        // Re-import is a no-op.
        assert_eq!(cat.import_file(&found[0], &cache).unwrap(), None);

        let photos = cat.all_photos();
        assert_eq!(photos.len(), 2);
        assert!(photos.iter().all(|p| p.blake3.len() == 64));
        for photo in &photos {
            let incremental = cat.photo_by_id(photo.id).expect("row by id");
            assert_eq!(incremental.id, photo.id);
            assert_eq!(incremental.path, photo.path);
            assert_eq!(incremental.blake3, photo.blake3);
        }
        assert!(cat.photo_by_id(i64::MAX).is_none());
        assert!(
            photos
                .iter()
                .all(|p| cache.join(&p.blake3).join("preview-512.jpg").exists())
        );
        assert!(
            photos
                .iter()
                .all(|p| cache.join(&p.blake3).join("preview-2048.jpg").exists())
        );

        cat.set_rating(photos[0].id, 4).unwrap();
        cat.set_flag(photos[0].id, true, false).unwrap();
        cat.set_sync(photos[0].id, SyncState::Pending);
        let photos = cat.all_photos();
        let first = photos.iter().find(|p| p.rating == 4).unwrap();
        assert!(first.picked && matches!(first.sync, SyncState::Pending));

        let folders = cat.folders();
        assert!(folders.iter().any(|(n, c)| n == "shoot" && *c == 1));
        assert!(folders.iter().any(|(n, c)| n == "(root)" && *c == 1));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn hash_stable() {
        let dir = workdir("hash");
        let p = dir.join("f.bin");
        std::fs::write(&p, b"hello").unwrap();
        assert_eq!(hash_file(&p).unwrap(), hash_file(&p).unwrap());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn card_history_and_hash_duplicates() {
        let dir = workdir("hist");
        let sub = dir.join("shoot");
        std::fs::create_dir_all(&sub).unwrap();
        jpeg(&sub.join("a.jpg"), 64, 48);
        let cat = Catalog::open(&dir.join("cat.db"), "Test", &dir).unwrap();
        let cache = dir.join("cache");
        let id = cat
            .import_file(&sub.join("a.jpg"), &cache)
            .unwrap()
            .unwrap();
        let hash = cat
            .all_photos()
            .into_iter()
            .find(|p| p.id == id)
            .unwrap()
            .blake3;
        assert!(cat.hash_exists(&hash));
        assert!(!cat.hash_exists(&"0".repeat(64)));
        // Same bytes under a new name are the same photo by content.
        std::fs::copy(sub.join("a.jpg"), sub.join("a-renamed.jpg")).unwrap();
        assert!(cat.hash_exists(&hash_file(&sub.join("a-renamed.jpg")).unwrap()));
        // Per-volume memory.
        assert!(!cat.card_has_hash(&hash, "CARD :: /Volumes/CARD"));
        cat.record_card_import(&hash, "CARD :: /Volumes/CARD")
            .unwrap();
        assert!(cat.card_has_hash(&hash, "CARD :: /Volumes/CARD"));
        assert!(!cat.card_has_hash(&hash, "OTHER :: /Volumes/OTHER"));
        // Batch journal lands.
        cat.record_import_batch(
            "/Volumes/CARD",
            "CARD :: /Volumes/CARD",
            "copy",
            "0",
            2,
            1,
            0,
        )
        .unwrap();
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn templates_seed_list_and_reuse() {
        // V02: builtins seed once, use refreshes the default, customs save.
        let dir = workdir("tpl");
        let cat = Catalog::open(&dir.join("cat.db"), "Test", &dir).unwrap();
        let folders = cat.list_templates("folder");
        assert_eq!(folders.len(), 3);
        assert_eq!(folders[0].0, "Dated"); // seeded default first
        let renames = cat.list_templates("rename");
        assert_eq!(renames.len(), 3);
        cat.touch_template("folder", "Camera days");
        // last_used has second granularity; the sleep keeps the recency
        // assertion (last used is the default) deterministic.
        std::thread::sleep(std::time::Duration::from_millis(1100));
        cat.save_template("folder", "Custom", "{yyyy}/x").unwrap();
        let folders = cat.list_templates("folder");
        assert_eq!(folders[0].0, "Custom");
        assert_eq!(folders.iter().filter(|(n, _)| n == "Custom").count(), 1);
        // Re-saving keeps one row and refreshes the value.
        cat.save_template("folder", "Custom", "{yyyy}/y").unwrap();
        let folders = cat.list_templates("folder");
        let custom = folders.iter().find(|(n, _)| n == "Custom").unwrap();
        assert_eq!(custom.1, "{yyyy}/y");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn relocate_keeps_everything_consistent() {
        // V02: original + sidecar move together; params, rating, queue rows
        // and previews (hash-keyed) stay valid.
        let dir = workdir("reloc");
        let sub = dir.join("shoot");
        std::fs::create_dir_all(&sub).unwrap();
        jpeg(&sub.join("a.jpg"), 64, 48);
        let cat = Catalog::open(&dir.join("cat.db"), "Test", &dir).unwrap();
        let cache = dir.join("cache");
        let id = cat
            .import_file(&sub.join("a.jpg"), &cache)
            .unwrap()
            .unwrap();
        let before_hash = hash_file(&sub.join("a.jpg")).unwrap();
        let mut params = vec![0f32; crate::edit::PARAM_COUNT];
        params[2] = 0.5;
        let arr: [f32; crate::edit::PARAM_COUNT] = params.try_into().unwrap();
        cat.save_params(
            id,
            &crate::edit::Edit {
                params: arr,
                history: Vec::new(),
                cursor: 0,
                crop: None,
                geom: crate::edit::CropGeom::default(),
                curve_on: true,
                hsl_on: true,
                detail_on: true,
                optics_on: true,
                effects_on: true,
                grading_on: true,
            },
        )
        .unwrap();
        cat.set_rating(id, 4).unwrap();
        let photo = sub.join("a.jpg").to_string_lossy().to_string();
        crate::xmp::write(
            &photo,
            &arr,
            4,
            &[],
            None,
            &crate::xmp::Authorship::default(),
            &crate::edit::CropGeom::default(),
        )
        .unwrap();
        cat.enqueue(id, "original");

        let new_path = cat.relocate_photo(id, "2026-06-14_0001_a.jpg").unwrap();
        assert_eq!(
            std::path::Path::new(&new_path),
            sub.join("2026-06-14_0001_a.jpg")
        );
        // Disk: original moved with identical bytes, sidecar followed.
        assert!(!sub.join("a.jpg").exists());
        assert_eq!(hash_file(Path::new(&new_path)).unwrap(), before_hash);
        assert!(std::path::Path::new(&format!("{new_path}.xmp")).exists());
        // DB: path + filename updated, content rows intact.
        let p = cat.all_photos().into_iter().find(|p| p.id == id).unwrap();
        assert_eq!(p.path, new_path);
        assert_eq!(p.rating, 4);
        assert_eq!(p.blake3, before_hash);
        let loaded: Vec<f32> = cat
            .load_all_params()
            .into_iter()
            .find(|(pid, _, _)| *pid == id)
            .map(|(_, v, _)| v)
            .unwrap();
        assert_eq!(loaded[2], 0.5);
        // Sync queue row survived (photo-id keyed).
        assert_eq!(cat.queue_depth(), (1, 0));
        // Sidecar still parses against the new path.
        assert_eq!(crate::xmp::read(&new_path).unwrap().rating, Some(4));
        // No-op rename returns the same path.
        assert_eq!(
            cat.relocate_photo(id, "2026-06-14_0001_a.jpg").unwrap(),
            new_path
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn relocate_collision_suffixes_and_reports() {
        let dir = workdir("relocc");
        let sub = dir.join("shoot");
        std::fs::create_dir_all(&sub).unwrap();
        jpeg(&sub.join("a.jpg"), 64, 48);
        jpeg(&sub.join("b.jpg"), 32, 24);
        let cat = Catalog::open(&dir.join("cat.db"), "Test", &dir).unwrap();
        let cache = dir.join("cache");
        let a = cat
            .import_file(&sub.join("a.jpg"), &cache)
            .unwrap()
            .unwrap();
        let _ = cat
            .import_file(&sub.join("b.jpg"), &cache)
            .unwrap()
            .unwrap();
        // b.jpg already occupies the target: a.jpg takes a suffixed name.
        let new_path = cat.relocate_photo(a, "b.jpg").unwrap();
        assert!(new_path.ends_with("b-2.jpg"), "{new_path}");
        assert!(sub.join("b-2.jpg").exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn apple_photos_links_drive_the_plan() {
        use crate::apple_photos::{JobKind, PhotosSettings, plan};
        let dir = workdir("apple-photos");
        std::fs::create_dir_all(dir.join("shoot")).unwrap();
        jpeg(&dir.join("shoot/a.jpg"), 32, 24);
        let cat = Catalog::open(&dir.join("c.db"), "c", &dir).unwrap();
        let id = cat
            .import_file(&dir.join("shoot/a.jpg"), &dir.join("cache"))
            .unwrap()
            .unwrap();
        let s = PhotosSettings {
            by_folder: true,
            ..Default::default()
        };
        cat.set_photos_settings(&s);
        assert_eq!(cat.photos_settings(), s);

        let c = cat.photos_candidates();
        assert_eq!(c.len(), 1);
        assert_eq!(c[0].folder, "shoot");
        assert!(c[0].exists);
        let jobs = plan(&c, &cat.photos_links(), &cat.photos_forced(), &s);
        assert_eq!(jobs[0].kind, JobKind::Import);
        cat.record_photos_job(&jobs[0], &["X/L0/001".to_string()])
            .unwrap();
        // Linked but still in Laika's folder: the move is retried.
        let again = plan(&c, &cat.photos_links(), &HashSet::new(), &s);
        assert_eq!(again[0].kind, JobKind::Update);
        assert!(again[0].needs_move());

        // Photos' verified copy becomes the original; it is read-only to
        // Laika from here on.
        let lib = dir.join("Photos Library.photoslibrary/originals/X");
        std::fs::create_dir_all(&lib).unwrap();
        let copy = lib.join("X.jpeg");
        std::fs::copy(dir.join("shoot/a.jpg"), &copy).unwrap();
        cat.adopt_photos_original(id, &copy).unwrap();
        let p = cat.photo_by_id(id).unwrap();
        assert_eq!(p.filename, "a.jpg");
        assert_eq!(cat.folder_of(&p.path), Catalog::PHOTOS_FOLDER);
        assert!(
            cat.folders()
                .iter()
                .any(|(f, n)| f == "Apple Photos" && *n == 1)
        );
        assert!(cat.move_photo(id, &dir.join("elsewhere")).is_err());
        assert!(cat.rename_folder(&lib, "nope").is_err());
        assert_eq!(cat.relocate_photo(id, "harbor.jpg").unwrap(), p.path);
        assert!(copy.exists());
        assert_eq!(cat.photo_by_id(id).unwrap().filename, "harbor.jpg");
        crate::xmp::write(
            &p.path,
            &crate::edit::defaults(),
            3,
            &[],
            None,
            &Default::default(),
            &Default::default(),
        )
        .unwrap();
        assert!(!Path::new(&crate::xmp::sidecar_path(&p.path)).exists());
        let c = cat.photos_candidates();
        assert!(plan(&c, &cat.photos_links(), &HashSet::new(), &s).is_empty());

        // A keyword change becomes an update of the same media item.
        cat.set_keywords(id, &["Harbor".to_string()]).unwrap();
        let c = cat.photos_candidates();
        let jobs = plan(&c, &cat.photos_links(), &HashSet::new(), &s);
        assert_eq!(jobs[0].kind, JobKind::Update);
        assert_eq!(jobs[0].item_ids, vec!["X/L0/001".to_string()]);
        assert!(!jobs[0].needs_move());

        // Deleted in Photos → forgotten → never re-imported from inside
        // the library.
        cat.forget_photos_links(&[id]);
        assert!(plan(&c, &cat.photos_links(), &HashSet::new(), &s).is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn photos_albums_and_ingest_links() {
        use crate::apple_photos::*;
        let dir = workdir("photos-albums");
        let lib = dir.join("L.photoslibrary/originals/A");
        std::fs::create_dir_all(&lib).unwrap();
        jpeg(&lib.join("AAA.jpeg"), 32, 24);
        jpeg(&dir.join("dupe.jpg"), 40, 30);
        std::fs::copy(dir.join("dupe.jpg"), lib.join("ABC.jpeg")).unwrap();
        let cat = Catalog::open(&dir.join("c.db"), "c", &dir).unwrap();
        let cache = dir.join("cache");
        let a = cat
            .import_file(&lib.join("AAA.jpeg"), &cache)
            .unwrap()
            .unwrap();
        // Laika already has this content elsewhere: links by hash.
        let dupe = cat
            .import_file(&dir.join("dupe.jpg"), &cache)
            .unwrap()
            .unwrap();

        let dump = LibraryDump {
            items: vec![
                LibraryItem {
                    id: "AAA/L0/001".to_string(),
                    filename: "IMG_0042.JPG".to_string(),
                    name: "Harbor".to_string(),
                    description: "Morning".to_string(),
                    favorite: true,
                    keywords: vec!["sea".to_string()],
                    ..Default::default()
                },
                LibraryItem {
                    id: "ABC/L0/001".to_string(),
                    ..Default::default()
                },
            ],
            containers: vec![
                Container {
                    id: "F1".to_string(),
                    parent: String::new(),
                    name: "Trips".to_string(),
                    kind: ContainerKind::Folder,
                    items: vec![],
                },
                Container {
                    id: "A1".to_string(),
                    parent: "F1".to_string(),
                    name: "Lisbon".to_string(),
                    kind: ContainerKind::Album,
                    items: vec!["AAA/L0/001".to_string(), "ABC/L0/001".to_string()],
                },
            ],
        };
        cat.replace_photos_albums(&dump).unwrap();
        let items: HashMap<String, LibraryItem> = dump
            .items
            .iter()
            .map(|i| (i.id.clone(), i.clone()))
            .collect();
        let s = PhotosSettings::default();
        let files = vec![
            (
                lib.join("AAA.jpeg").to_string_lossy().to_string(),
                "AAA/L0/001".to_string(),
                String::new(),
            ),
            (
                lib.join("ABC.jpeg").to_string_lossy().to_string(),
                "ABC/L0/001".to_string(),
                hash_file(&lib.join("ABC.jpeg")).unwrap(),
            ),
        ];
        assert_eq!(cat.apply_photos_ingest(&files, &items, &s).unwrap(), 2);
        // Idempotent.
        assert_eq!(cat.apply_photos_ingest(&files, &items, &s).unwrap(), 0);

        let p = cat.photo_by_id(a).unwrap();
        assert_eq!(
            (p.title.as_str(), p.caption.as_str(), p.picked),
            ("Harbor", "Morning", true)
        );
        assert_eq!(p.filename, "IMG_0042.JPG");
        assert_eq!(cat.photo_keywords(a), vec!["sea".to_string()]);

        let tree = cat.photos_album_tree();
        assert_eq!(tree.len(), 2);
        assert!(tree[0].is_folder);
        assert_eq!(
            (tree[1].name.as_str(), tree[1].depth, tree[1].count),
            ("Lisbon", 1, 2)
        );
        let members = cat.photos_album_members("A1");
        assert!(members.contains(&a) && members.contains(&dupe));

        // Ingested links start in sync: nothing pushes back to Photos, and
        // library files are never imported again.
        let on = PhotosSettings {
            enabled: true,
            ..Default::default()
        };
        let jobs = plan(
            &cat.photos_candidates(),
            &cat.photos_links(),
            &HashSet::new(),
            &on,
        );
        assert!(jobs.iter().all(|j| j.photo_ids != vec![a]), "{jobs:?}");
        // The duplicate still lives in Laika's folder, so with sync on it
        // is moved into Photos (one copy).
        assert!(
            jobs.iter()
                .any(|j| j.photo_ids == vec![dupe] && j.needs_move())
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn keywords_empty_until_assigned() {
        // U01: nothing assigned → no keywords, never sample content.
        let dir = workdir("kw");
        let cat = Catalog::open(&dir.join("cat.db"), "Test", &dir).unwrap();
        assert!(cat.keywords().is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn v04_folder_ops_and_sync_diff() {
        // V04: create/rename/delete folders, diff, watched list.
        let dir = workdir("v04");
        let shoot = dir.join("shoot");
        std::fs::create_dir_all(&shoot).unwrap();
        jpeg(&shoot.join("a.jpg"), 64, 48);
        jpeg(&shoot.join("b.jpg"), 32, 24);
        let cat = Catalog::open(&dir.join("cat.db"), "Test", &dir).unwrap();
        let cache = dir.join("cache");
        let a = cat
            .import_file(&shoot.join("a.jpg"), &cache)
            .unwrap()
            .unwrap();

        // Create + validation.
        assert!(Catalog::check_folder_name("a/b").is_err());
        assert!(Catalog::check_folder_name("  ").is_err());
        let sub = cat.create_folder(&shoot, " selects ").unwrap();
        assert!(sub.is_dir());
        assert!(cat.create_folder(&shoot, "selects").is_err());

        // Diff: b.jpg is new (never imported), nothing missing/changed.
        let diff = cat.folder_diff(&shoot);
        assert!(diff.error.is_none());
        assert_eq!(diff.new_files.len(), 1);
        assert!(diff.new_files[0].ends_with("b.jpg"));
        assert!(diff.missing.is_empty() && diff.changed.is_empty());

        // Rename moves the directory and every descendant row at once,
        // sidecars riding along on disk.
        let renamed = cat.rename_folder(&shoot, "ceremony").unwrap();
        assert!(!shoot.exists());
        assert_eq!(
            cat.all_photos()
                .into_iter()
                .find(|p| p.id == a)
                .unwrap()
                .path,
            renamed.join("a.jpg").to_string_lossy().to_string()
        );
        // Diff follows the rename: b.jpg (never imported) is new,
        // nothing is missing.
        let diff = cat.folder_diff(&renamed);
        assert_eq!(diff.new_files.len(), 1);
        assert!(diff.missing.is_empty());

        // Non-empty folders refuse deletion (Remove/Trash flows own that).
        assert!(cat.delete_folder(&renamed).is_err());
        let moved_sub = renamed.join("selects");
        assert!(
            cat.delete_folder(&moved_sub).is_ok(),
            "empty selects/ must delete"
        );
        assert!(!moved_sub.exists());

        // Watched list round-trips; missing dirs refuse watching.
        assert!(cat.list_watched().is_empty());
        cat.watch_folder(&renamed).unwrap();
        assert!(cat.watch_folder(&dir.join("gone")).is_err());
        assert_eq!(
            cat.list_watched(),
            vec![renamed.to_string_lossy().to_string()]
        );
        cat.unwatch_folder(&renamed);
        assert!(cat.list_watched().is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn v04_sync_diff_spots_new_missing_changed() {
        let dir = workdir("v04d");
        let shoot = dir.join("shoot");
        std::fs::create_dir_all(&shoot).unwrap();
        jpeg(&shoot.join("a.jpg"), 64, 48);
        let cat = Catalog::open(&dir.join("cat.db"), "Test", &dir).unwrap();
        let cache = dir.join("cache");
        let a = cat
            .import_file(&shoot.join("a.jpg"), &cache)
            .unwrap()
            .unwrap();
        // New file appears in diff.
        jpeg(&shoot.join("b.jpg"), 32, 24);
        // Write + record a sidecar, then change it externally.
        let photo = shoot.join("a.jpg").to_string_lossy().to_string();
        crate::xmp::write(
            &photo,
            &[0f32; crate::edit::PARAM_COUNT],
            0,
            &[],
            None,
            &crate::xmp::Authorship::default(),
            &crate::edit::CropGeom::default(),
        )
        .unwrap();
        cat.remember_sidecar_write(a, crate::xmp::sidecar_mtime(&photo).unwrap())
            .unwrap();
        crate::xmp::write(
            &photo,
            &[1f32; crate::edit::PARAM_COUNT],
            3,
            &[],
            None,
            &crate::xmp::Authorship::default(),
            &crate::edit::CropGeom::default(),
        )
        .unwrap();
        set_mtime(
            std::path::Path::new(&crate::xmp::sidecar_path(&photo)),
            now_secs() + 100,
        );
        let diff = cat.folder_diff(&shoot);
        assert_eq!(diff.new_files.len(), 1);
        assert!(diff.missing.is_empty());
        assert_eq!(diff.changed.len(), 1);
        assert_eq!(diff.changed[0].0, a);
        // Removing the original flips it to missing.
        std::fs::remove_file(shoot.join("a.jpg")).unwrap();
        let diff = cat.folder_diff(&shoot);
        assert_eq!(diff.missing.len(), 1);
        assert_eq!(diff.missing[0].0, a);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn v05_video_imports_with_container_metadata() {
        // V05: a hand-built MP4 row lands with duration/dims/codec and a
        // capture time, without any decoder, and survives reload.
        // Posters are hermetic here (helpers off); production tries them.
        unsafe {
            std::env::set_var("LAIKA_NO_POSTER", "1");
        }
        let dir = workdir("v05v");
        std::fs::create_dir_all(&dir).unwrap();
        let mp4 = dir.join("clip.mp4");
        std::fs::write(&mp4, minimal_mp4()).unwrap();
        let cat = Catalog::open(&dir.join("cat.db"), "Test", &dir).unwrap();
        let cache = dir.join("cache");
        let id = cat.import_file(&mp4, &cache).unwrap().unwrap();
        let p = cat.all_photos().into_iter().find(|p| p.id == id).unwrap();
        assert_eq!(p.duration_ms, 42150);
        assert_eq!((p.width, p.height), (1920, 1080));
        assert_eq!(p.codec, "avc1");
        // No container timestamp → file-mtime fallback (sortable).
        assert_eq!(p.captured_at.len(), 19);
        // Reload keeps every video field.
        drop(cat);
        let cat = Catalog::open(&dir.join("cat.db"), "Test", &dir).unwrap();
        let p = &cat.all_photos()[0];
        assert_eq!((p.duration_ms, p.codec.as_str()), (42150, "avc1"));
        // Garbage with an mp4 extension imports as an empty-metadata row
        // rather than failing the shoot.
        std::fs::write(dir.join("bad.mp4"), b"not a movie").unwrap();
        let id2 = cat
            .import_file(&dir.join("bad.mp4"), &cache)
            .unwrap()
            .unwrap();
        let q = cat.all_photos().into_iter().find(|p| p.id == id2).unwrap();
        assert_eq!(q.duration_ms, 0);
        assert!(q.codec.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Minimal ftyp+moov (mvhd 1000Hz/42150 + 1920x1080 avc1 trak).
    fn minimal_mp4() -> Vec<u8> {
        fn bx(tag: &[u8; 4], payload: &[u8]) -> Vec<u8> {
            let mut v = Vec::new();
            v.extend_from_slice(&((8 + payload.len()) as u32).to_be_bytes());
            v.extend_from_slice(tag);
            v.extend_from_slice(payload);
            v
        }
        let mut mvhd = vec![0u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        mvhd.extend_from_slice(&1000u32.to_be_bytes());
        mvhd.extend_from_slice(&42150u32.to_be_bytes());
        mvhd.extend_from_slice(&[0u8; 80]);
        let mut tkhd = vec![0u8, 0, 0, 0];
        tkhd.extend_from_slice(&[0u8; 4 + 4 + 4 + 4 + 4 + 8 + 2 + 2 + 2 + 2 + 36]);
        tkhd.extend_from_slice(&((1920u32 << 16).to_be_bytes()));
        tkhd.extend_from_slice(&((1080u32 << 16).to_be_bytes()));
        let mut stsd = vec![0, 0, 0, 0];
        stsd.extend_from_slice(&1u32.to_be_bytes());
        stsd.extend_from_slice(&86u32.to_be_bytes());
        stsd.extend_from_slice(b"avc1");
        stsd.extend_from_slice(&[0u8; 78]);
        let minf = bx(b"minf", &bx(b"stbl", &bx(b"stsd", &stsd)));
        let mdia = bx(b"mdia", &bx(b"minf", &minf[8..]));
        let mut trak_payload = bx(b"tkhd", &tkhd);
        trak_payload.extend_from_slice(&mdia);
        let mut moov = bx(b"mvhd", &mvhd);
        moov.extend_from_slice(&bx(b"trak", &trak_payload));
        let mut file = bx(b"ftyp", b"isom\0\0\0\0isom");
        file.extend_from_slice(&bx(b"moov", &moov));
        file
    }

    #[test]
    fn v05_pairs_group_by_folder_and_stem() {
        // V05: RAW+JPEG rows group per capture; toggling the preference is
        // a view concern (rows untouched).
        let dir = workdir("v05p");
        let sub = dir.join("shoot");
        std::fs::create_dir_all(&sub).unwrap();
        // Grouping runs over hand-built rows (import needs decodable
        // content, which fake RAWs lack).
        let rows = vec![
            (1, sub.join("a.nef").to_string_lossy().to_string(), true),
            (2, sub.join("a.jpg").to_string_lossy().to_string(), false),
            (3, sub.join("b.nef").to_string_lossy().to_string(), true),
        ];
        let pairs = crate::pairs::find_pairs(&rows);
        assert_eq!(pairs.len(), 1);
        assert_eq!(pairs[0].raw_id, 1);
        assert_eq!(pairs[0].jpeg_id, 2);
        assert_eq!(crate::pairs::sibling_of(&pairs, 1), Some(2));
        assert_eq!(crate::pairs::sibling_of(&pairs, 2), Some(1));
        assert_eq!(crate::pairs::sibling_of(&pairs, 3), None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn v07_relative_storage_and_moved_bundle() {
        // V07: rows under the root store relatively; a moved catalog
        // folder reopens with every photo online and edits intact.
        let dir = workdir("v07");
        let shoot = dir.join("shoot");
        std::fs::create_dir_all(&shoot).unwrap();
        jpeg(&shoot.join("a.jpg"), 64, 48);
        let db = dir.join("cat.db");
        let id = {
            let cat = Catalog::open(&db, "Test", &dir).unwrap();
            let cache = dir.join("cache");
            let id = cat
                .import_file(&shoot.join("a.jpg"), &cache)
                .unwrap()
                .unwrap();
            cat.set_rating(id, 4).unwrap();
            // Stored relative, resolved absolute.
            let stored: String = cat
                .conn
                .query_row("SELECT path FROM photos WHERE id = ?1", [id], |r| r.get(0))
                .unwrap();
            assert_eq!(stored, "shoot/a.jpg");
            assert!(cat.all_photos()[0].path.ends_with("shoot/a.jpg"));
            assert!(cat.already_imported(&shoot.join("a.jpg").to_string_lossy()));
            id
        };
        // Move the whole bundle elsewhere and reopen.
        let dir2 = std::env::temp_dir().join(format!("laika-cat-v07b-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir2);
        std::fs::create_dir_all(&dir2).unwrap();
        let moved = dir2.join("bundle");
        std::fs::rename(&dir, &moved).unwrap();
        let cat = Catalog::open(&moved.join("cat.db"), "Test", &moved).unwrap();
        assert!(cat.missing_ids().is_empty());
        let p = &cat.all_photos()[0];
        assert_eq!(p.rating, 4);
        assert_eq!(p.id, id);
        // Outside-root files stay absolute.
        let outside = dir2.join("elsewhere.jpg");
        jpeg(&outside, 32, 24);
        let cache = dir2.join("cache");
        let oid = cat.import_file(&outside, &cache).unwrap().unwrap();
        let stored: String = cat
            .conn
            .query_row("SELECT path FROM photos WHERE id = ?1", [oid], |r| r.get(0))
            .unwrap();
        assert!(std::path::Path::new(&stored).is_absolute());
        std::fs::remove_dir_all(&dir2).ok();
    }

    #[test]
    fn v07_lock_second_instance_and_stale_takeover() {
        let dir = workdir("v07lock");
        let db = dir.join("cat.db");
        std::fs::write(&db, b"").unwrap();
        let _guard = CatalogLock::acquire(&db).expect("first lock");
        let err = CatalogLock::acquire(&db).expect_err("second lock refuses");
        assert!(err.contains("close it first"), "{err}");
        drop(_guard);
        // Released locks open cleanly (Drop unlinks our own pid).
        assert!(!db.with_extension("db.lock").exists());
        let _guard2 = CatalogLock::acquire(&db).expect("re-acquire after release");
        // Stale lock from a dead pid is taken over.
        std::fs::write(db.with_extension("db.lock"), "1\nnonexistent-host-xyz\n0").unwrap();
        // Different host: refused even for dead pids (can't verify).
        assert!(CatalogLock::acquire(&db).is_err());
        std::fs::write(
            db.with_extension("db.lock"),
            format!("{}\n{}\n0", 4000000000u32, super::this_host()),
        )
        .unwrap();
        // Absurd pid: kill() ESRCH/EPERM ambiguity — accept either outcome
        // without asserting; the live-pid path is covered above.
        let _ = CatalogLock::acquire(&db);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn v07_recents_dedupe_cap_and_order() {
        let dir = workdir("v07r");
        let mut lib = LibraryState::default();
        for i in 0..10 {
            lib.push_recent(std::path::Path::new(&format!("/vol/c{i}.db")), &dir);
        }
        assert_eq!(lib.recent.len(), 8);
        assert_eq!(lib.recent[0], "/vol/c9.db");
        lib.push_recent(std::path::Path::new("/vol/c5.db"), &dir);
        assert_eq!(lib.recent[0], "/vol/c5.db");
        assert_eq!(lib.recent.len(), 8);
        // Persisted + reread.
        let back = LibraryState::read(&dir);
        assert_eq!(back.recent, lib.recent);
        assert_eq!(back.startup, StartupMode::Last);
        lib.set_startup(StartupMode::Ask, "", &dir);
        assert_eq!(LibraryState::read(&dir).startup, StartupMode::Ask);
        // Missing files sink below existing ones.
        std::fs::write(dir.join("here.db"), b"").unwrap();
        lib.recent = vec![
            "/gone/a.db".into(),
            dir.join("here.db").to_string_lossy().to_string(),
        ];
        let ordered = lib.ordered();
        assert!(ordered[0].ends_with("here.db"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn v09_audit_evicts_expired_then_oldest_and_pins_offline() {
        use std::collections::HashSet;
        // V09: TTL expiry beats age; over-cap evicts oldest first;
        // offline smart previews and unknown dirs never go.
        let dir = workdir("v09audit");
        let h = |n: &str| format!("{n:0<64}");
        let (online_new, online_old, expired, off) = (h("a"), h("b"), h("c"), h("d"));
        for name in [
            &online_new,
            &online_old,
            &expired,
            &off,
            &"zzzap".to_string(),
        ] {
            let d = dir.join(name.as_str());
            std::fs::create_dir_all(&d).unwrap();
            std::fs::write(d.join("preview-11.jpg"), vec![0u8; 1000]).unwrap();
            std::fs::write(d.join("preview-512.jpg"), vec![0u8; 100]).unwrap();
        }
        // mtimes: expired is ancient, online_old older than online_new.
        filetime_set(&dir.join(expired.as_str()).join("preview-11.jpg"), 100);
        filetime_set(&dir.join(online_old.as_str()).join("preview-11.jpg"), 200);
        filetime_set(&dir.join(online_new.as_str()).join("preview-11.jpg"), 300);
        let known: HashSet<String> = [&online_new, &online_old, &expired, &off]
            .into_iter()
            .cloned()
            .collect();
        let offline: HashSet<String> = [&off].into_iter().cloned().collect();
        // TTL 0-days... use ttl that expires only the ancient file:
        // now=400, ttl covering 100..200 boundary via days is coarse, so
        // pass ttl_days=0 (disabled) first for the cap path, then test
        // expiry separately below.
        let a = super::audit_cache(&dir, &known, &offline, 10_000_000, 0, 400);
        assert!(a.evicted.is_empty(), "{:?}", a.evicted);
        assert_eq!(a.files, 10);
        // Cap just under total bytes (5500): oldest online 1:1 goes,
        // its 512 companion survives (never evictable).
        let a = super::audit_cache(&dir, &known, &offline, 4500, 0, 400);
        assert_eq!(a.evicted, vec![expired.clone()], "{:?}", a.evicted);
        assert!(dir.join(expired.as_str()).join("preview-512.jpg").exists());
        // TTL expiry: ttl_days=1 with now far future expires everything
        // online (`expired` already went to the cap test above).
        let a = super::audit_cache(&dir, &known, &offline, 10_000_000, 1, 100_000_000);
        assert_eq!(a.evicted.len(), 2, "{:?}", a.evicted);
        assert!(a.evicted.contains(&online_new), "{:?}", a.evicted);
        assert!(a.evicted.contains(&online_old), "{:?}", a.evicted);
        assert!(!a.evicted.contains(&off), "{:?}", a.evicted);
        assert!(dir.join(off.as_str()).join("preview-11.jpg").exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn v09_detail_key_matches_exactly_core() {
        let key = super::DetailKey {
            warp: Vec::new(),
            values: vec![1., 2., 3.],
            rect: [0., 0., 1., 1.],
            angle: 0.,
            flip_h: false,
            flip_v: false,
            rotation: 0,
            split: 0.,
            w: 100,
            h: 50,
        };
        assert!(super::detail_key_matches(
            &key,
            &[1., 2., 3.],
            [0., 0., 1., 1.],
            0.,
            false,
            false,
            0,
            0.,
            100,
            50
        ));
        // Rotation drift invalidates (never shows stale orientation).
        assert!(!super::detail_key_matches(
            &key,
            &[1., 2., 3.],
            [0., 0., 1., 1.],
            0.,
            false,
            false,
            1,
            0.,
            100,
            50
        ));
        // Any drift — tone, geometry, split, dims — invalidates.
        assert!(!super::detail_key_matches(
            &key,
            &[1., 2., 4.],
            [0., 0., 1., 1.],
            0.,
            false,
            false,
            0,
            0.,
            100,
            50
        ));
        assert!(!super::detail_key_matches(
            &key,
            &[1., 2., 3.],
            [0., 0., 0.5, 1.],
            0.,
            false,
            false,
            0,
            0.,
            100,
            50
        ));
        assert!(!super::detail_key_matches(
            &key,
            &[1., 2., 3.],
            [0., 0., 1., 1.],
            1.,
            false,
            false,
            0,
            0.,
            100,
            50
        ));
        assert!(!super::detail_key_matches(
            &key,
            &[1., 2., 3.],
            [0., 0., 1., 1.],
            0.,
            true,
            false,
            0,
            0.,
            100,
            50
        ));
        assert!(!super::detail_key_matches(
            &key,
            &[1., 2., 3.],
            [0., 0., 1., 1.],
            0.,
            false,
            false,
            0,
            0.5,
            100,
            50
        ));
        assert!(!super::detail_key_matches(
            &key,
            &[1., 2., 3.],
            [0., 0., 1., 1.],
            0.,
            false,
            false,
            0,
            0.,
            200,
            50
        ));
    }

    #[test]
    fn v08_mid_era_upgrade_preserves_everything() {
        // V08: a mid-prototype catalog (ratings/flags/sync but no V03
        // columns, no version stamp) opens with all data intact.
        let dir = workdir("v08up");
        let db = dir.join("cat.db");
        {
            let conn = Connection::open(&db).unwrap();
            conn.execute_batch(
                "CREATE TABLE catalogs(id INTEGER PRIMARY KEY, name TEXT, root_path TEXT);
                 INSERT INTO catalogs(name, root_path) VALUES ('P', '/shoot');
                 CREATE TABLE photos(id INTEGER PRIMARY KEY, catalog_id INTEGER, path TEXT UNIQUE,
                 filename TEXT, blake3 TEXT, captured_at TEXT, camera TEXT, lens TEXT, focal_mm TEXT,
                 aperture TEXT, shutter TEXT, iso TEXT, width INTEGER, height INTEGER,
                 rating INTEGER DEFAULT 0, picked INTEGER DEFAULT 0, rejected INTEGER DEFAULT 0,
                 sync_state TEXT DEFAULT 'local', remote_key TEXT, imported_at TEXT);
                 INSERT INTO photos(catalog_id, path, filename, blake3, captured_at, rating, picked, sync_state)
                 VALUES (1, '/shoot/a.nef', 'a.nef', 'h1', '2026:01:02 10:00:00', 4, 1, 'synced');
                 CREATE TABLE keywords(photo_id INTEGER, keyword TEXT);
                 INSERT INTO keywords VALUES (1, 'Lisbon');
                 CREATE TABLE edits(photo_id INTEGER PRIMARY KEY, params_json TEXT, history_json TEXT, cursor INTEGER, updated_at TEXT);",
            )
            .unwrap();
        }
        let cat = Catalog::open(&db, "P", &dir).expect("mid-era upgrades");
        // Ratings, flags, sync state, keywords survive.
        let photos = cat.all_photos();
        assert_eq!(photos.len(), 1);
        assert_eq!(photos[0].rating, 4);
        assert!(photos[0].picked);
        assert!(photos[0].sync == crate::photo::SyncState::Synced);
        assert_eq!(cat.photo_keywords(1), vec!["Lisbon".to_string()]);
        // Version stamped + pre-migration backup written beside it.
        let v: u32 = cat
            .conn
            .query_row("SELECT version FROM schema_version LIMIT 1", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(v, super::migrations::APP_SCHEMA_VERSION);
        let backups: Vec<_> = std::fs::read_dir(dir.join("backups"))
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert_eq!(backups.len(), 1);
        assert!(
            backups[0]
                .file_name()
                .to_string_lossy()
                .starts_with("laika-pre-v0-")
        );
        // Indexes exist now.
        let idx: i64 = cat
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name LIKE 'idx_photos_%'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(idx >= 5, "{idx}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn v12_keyword_hierarchy_rename_merge_synonyms_sets_text() {
        // V12 gate: canon/parents, auto-parents, rename + merge survival,
        // synonym resolution, sets, and the text interchange round-trip.
        let dir = workdir("v12kw");
        let db = dir.join("cat.db");
        let cat = Catalog::open(&db, "Test", &dir).unwrap();
        assert_eq!(Catalog::canon_keyword_path("A>B >  C"), "A > B > C");
        assert_eq!(Catalog::keyword_leaf("A > B"), "B");
        assert_eq!(
            Catalog::keyword_parents("A > B > C"),
            vec!["A".to_string(), "A > B".to_string()]
        );
        // Assignment auto-creates nodes with ancestors.
        cat.set_keywords(1, &["Places > Portugal > Lisbon".to_string()])
            .unwrap();
        let nodes = cat.keyword_nodes();
        assert_eq!(nodes.len(), 3);
        assert!(nodes.iter().all(|(_, inc)| *inc));
        // Synonyms resolve on assignment (case-insensitive).
        cat.add_keyword_synonym("Birds > Owl", "night bird");
        assert_eq!(cat.resolve_keyword("Night Bird"), "Birds > Owl");
        assert_eq!(cat.resolve_keyword("new thing"), "new thing");
        cat.set_keywords(2, &["night bird".to_string()]).unwrap();
        assert_eq!(cat.photo_keywords(2), vec!["Birds > Owl".to_string()]);
        // Rename carries the subtree + assignments.
        cat.rename_keyword_node("Places > Portugal", "Travel > PT")
            .unwrap();
        assert_eq!(
            cat.photo_keywords(1),
            vec!["Travel > PT > Lisbon".to_string()]
        );
        assert!(cat.rename_keyword_node("", "x").is_err());
        // Merge moves assignments + synonyms, drops the source.
        cat.merge_keyword_nodes("Birds > Owl", "Travel > PT > Lisbon")
            .unwrap();
        assert_eq!(
            cat.photo_keywords(2),
            vec!["Travel > PT > Lisbon".to_string()]
        );
        assert_eq!(cat.resolve_keyword("night bird"), "Travel > PT > Lisbon");
        assert!(cat.keyword_nodes().iter().all(|(p, _)| p != "Birds > Owl"));
        assert!(cat.merge_keyword_nodes("a", "a").is_err());
        // Exclusion + sets + text round-trip.
        cat.set_keyword_include("Travel", false);
        cat.save_keyword_set("trip", &["Travel > PT > Lisbon".to_string()]);
        let text = cat.export_keywords_text();
        assert!(text.contains("!Travel\n"), "{text}");
        assert!(text.contains("= night bird"), "{text}");
        assert!(text.contains("@trip: Travel > PT > Lisbon"), "{text}");
        let dir2 = workdir("v12kw2");
        let cat2 = Catalog::open(&dir2.join("cat.db"), "Test", &dir2).unwrap();
        let (nodes_n, syns_n) = cat2.import_keywords_text(&text).unwrap();
        assert!(nodes_n >= 3 && syns_n >= 1);
        assert_eq!(cat2.export_keywords_text(), text);
        assert!(cat2.import_keywords_text("@bad line").is_err());
        // Delete drops the subtree + assignments + set members.
        cat.delete_keyword_node("Travel > PT");
        assert!(cat.photo_keywords(1).is_empty());
        assert!(cat.list_keyword_sets()[0].1.is_empty());
        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&dir2).ok();
    }

    #[test]
    fn v12_photo_meta_and_presets_carry_descriptive_fields() {
        // V12 gate: title/caption/headline/location persist on photos
        // and presets, and sidecar adoption is field-by-field.
        let dir = workdir("v12meta");
        let db = dir.join("cat.db");
        let cat = Catalog::open(&db, "Test", &dir).unwrap();
        let sub = dir.join("pics");
        std::fs::create_dir_all(&sub).unwrap();
        jpeg(&sub.join("a.jpg"), 32, 24);
        let id = cat
            .import_file(&sub.join("a.jpg"), &dir.join("cache"))
            .unwrap()
            .unwrap();
        let meta = PhotoMeta {
            title: "Dawn".into(),
            caption: "Harbor light".into(),
            headline: "H".into(),
            creator: "Ada".into(),
            copyright: "© Ada".into(),
            rights: "Editorial".into(),
            contact: "ada@x.io".into(),
            location: "Lisbon".into(),
        };
        cat.set_photo_meta(id, &meta).unwrap();
        assert_eq!(cat.photo_meta(id), meta);
        assert_eq!(cat.photo_authorship(id).caption, "Harbor light");
        cat.save_metadata_preset(&MetadataPreset {
            name: "Full".into(),
            title: "T".into(),
            caption: "C".into(),
            headline: "H".into(),
            creator: "Ada".into(),
            copyright: "©".into(),
            rights: "R".into(),
            contact: "c".into(),
            location: "L".into(),
            keywords: "a".into(),
        })
        .unwrap();
        let presets = cat.list_metadata_presets();
        assert_eq!(presets.len(), 1);
        assert_eq!(presets[0].caption, "C");
        assert_eq!(presets[0].location, "L");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn v15_remove_snapshot_restores_rows_edits_keywords() {
        // V15 gate: snapshot → remove → restore brings back the photo row,
        // persisted edits, keywords, and named snapshots (same id when free).
        let dir = workdir("v15rm");
        let db = dir.join("cat.db");
        let cat = Catalog::open(&db, "Test", &dir).unwrap();
        let sub = dir.join("pics");
        std::fs::create_dir_all(&sub).unwrap();
        jpeg(&sub.join("a.jpg"), 32, 24);
        let id = cat
            .import_file(&sub.join("a.jpg"), &dir.join("cache"))
            .unwrap()
            .unwrap();
        cat.set_rating(id, 4).unwrap();
        cat.set_keywords(id, &["Owl".to_string()]).unwrap();
        cat.save_metadata_preset(&MetadataPreset {
            name: "x".into(),
            ..Default::default()
        })
        .unwrap();
        // Persist an edit + a named snapshot so restore has shape.
        let mut e = crate::edit::Edit::new();
        e.push("Exposure", "+1", crate::edit::defaults());
        cat.save_params(id, &e).unwrap();
        let snap = crate::edit::Snap {
            params: crate::edit::defaults(),
            crop: None,
            geom: crate::edit::CropGeom::default(),
            curve_on: true,
            hsl_on: true,
            detail_on: true,
            optics_on: true,
            effects_on: true,
            grading_on: true,
            rating: 4,
            picked: false,
            rejected: false,
        };
        cat.save_snapshot(id, "s1", &snap).unwrap();
        let saved = cat.snapshot_removed(id).expect("snapshot");
        assert_eq!(saved.keywords, vec!["Owl".to_string()]);
        assert_eq!(saved.snapshots.len(), 1);
        cat.remove_photo(id).unwrap();
        assert!(cat.snapshot_removed(id).is_none());
        let back = cat.restore_removed(&saved).unwrap();
        assert_eq!(back, id);
        let photos = cat.all_photos();
        assert_eq!(photos.len(), 1);
        assert_eq!(photos[0].rating, 4);
        assert_eq!(cat.photo_keywords(id), vec!["Owl".to_string()]);
        assert_eq!(cat.list_snapshots(id).len(), 1);
        assert_eq!(cat.load_all_edits().len(), 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn v28_export_presets_round_trip() {
        // V28 gate: save/list/get/delete + upsert, folders order first.
        let dir = workdir("v28presets");
        let db = dir.join("cat.db");
        let cat = Catalog::open(&db, "Test", &dir).unwrap();
        assert!(cat.list_export_presets().is_empty());
        cat.save_export_preset("Client JPEG", "Client", "{\"q\":90}");
        cat.save_export_preset("Web AVIF", "", "{\"q\":80}");
        cat.save_export_preset("Print TIFF", "Client", "{\"q\":0}");
        let list = cat.list_export_presets();
        assert_eq!(list.len(), 3);
        // Empty folder sorts first, then folder groups alphabetically.
        assert_eq!(list[0].0, "Web AVIF");
        assert_eq!(list[1], ("Client JPEG".to_string(), "Client".to_string()));
        let (folder, body) = cat.get_export_preset("Client JPEG").expect("get");
        assert_eq!((folder.as_str(), body.as_str()), ("Client", "{\"q\":90}"));
        // Upsert overwrites folder + body.
        cat.save_export_preset("Client JPEG", "Final", "{\"q\":95}");
        assert_eq!(
            cat.get_export_preset("Client JPEG").expect("get2").0,
            "Final"
        );
        assert!(cat.get_export_preset("Missing").is_none());
        cat.delete_export_preset("Web AVIF");
        assert_eq!(cat.list_export_presets().len(), 2);
        cat.delete_export_preset("Missing"); // no-op, never errors
        // Survives reopen (v3 table is real, not a temp).
        drop(cat);
        let cat = Catalog::open(&db, "Test", &dir).unwrap();
        assert_eq!(cat.list_export_presets().len(), 2);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn v08_failed_migration_rolls_back_and_names_the_step() {
        use super::migrations::{Migration, run_migrations};
        let dir = workdir("v08fail");
        let db = dir.join("cat.db");
        let mut conn = Connection::open(&db).unwrap();
        fn ok_step(conn: &Connection) -> Result<(), String> {
            conn.execute_batch("CREATE TABLE kept(x TEXT);")
                .map_err(|e| e.to_string())
        }
        fn bad_step(_conn: &Connection) -> Result<(), String> {
            Err("boom".to_string())
        }
        let steps = [
            Migration {
                version: 1,
                name: "good step",
                apply: ok_step,
            },
            Migration {
                version: 2,
                name: "bad step",
                apply: bad_step,
            },
        ];
        let err = run_migrations(&mut conn, 0, &steps).expect_err("must fail");
        assert!(err.contains('2') && err.contains("bad step"), "{err}");
        // Committed prefix stays; the failed step left nothing behind.
        let kept: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE name='kept'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(kept, 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn v08_newer_schema_refuses_with_version() {
        let dir = workdir("v08new");
        let db = dir.join("cat.db");
        let cat = Catalog::open(&db, "P", &dir).unwrap();
        cat.conn
            .execute("UPDATE schema_version SET version = 99", [])
            .unwrap();
        drop(cat);
        let err = match Catalog::open(&db, "P", &dir) {
            Ok(_) => panic!("newer refuses"),
            Err(e) => e,
        };
        assert!(err.contains("99"), "{err}");
        assert!(err.contains("update Laika"), "{err}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn v08_garbage_file_refuses() {
        let dir = workdir("v08garbage");
        let db = dir.join("cat.db");
        std::fs::write(&db, b"this is not sqlite at all").unwrap();
        let err = match Catalog::open(&db, "P", &dir) {
            Ok(_) => panic!("garbage refuses"),
            Err(e) => e,
        };
        assert!(err.contains("not a SQLite database"), "{err}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn v08_integrity_reports_damage_and_optimize_reclaims() {
        let dir = workdir("v08health");
        let db = dir.join("cat.db");
        let cat = Catalog::open(&db, "P", &dir).unwrap();
        cat.conn
            .execute_batch(
                &((0..500)
                    .map(|i| format!("INSERT INTO photos(catalog_id, path, filename, blake3) VALUES (1, '/f{i}.nef', 'f{i}.nef', 'x');"))
                    .collect::<Vec<_>>()
                    .join("")),
            )
            .unwrap();
        cat.conn
            .execute_batch("DELETE FROM photos WHERE id % 2 = 0;")
            .unwrap();
        let oreport = Catalog::optimize_db(&db).expect("optimize works");
        assert!(oreport.after <= oreport.before, "{oreport:?}");
        // Damage a COPY (never the live file): truncating halfway is
        // deterministic damage — integrity must report it.
        let bad = dir.join("bad.db");
        std::fs::copy(&db, &bad).unwrap();
        {
            let bytes = std::fs::read(&bad).unwrap();
            std::fs::write(&bad, &bytes[..bytes.len() / 2]).unwrap();
        }
        let err = Catalog::probe_backup(&bad).expect_err("damage reports");
        assert!(
            err.contains("integrity check failed") || err.contains("damaged catalog"),
            "{err}"
        );
        // A healthy file probes clean with counts.
        let probe = Catalog::probe_backup(&db).expect("healthy probes");
        assert!(probe.photos > 0);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn u21_preview_cache_drop_and_prune() {
        // U21: removing a photo frees its cache dir; shared hashes
        // survive; prune sweeps the rest.
        let dir = workdir("u21cache");
        let db = dir.join("cat.db");
        let cat = Catalog::open(&db, "P", &dir).unwrap();
        let cache = dir.join("cache");
        for h in ["aaa", "bbb"] {
            std::fs::create_dir_all(cache.join(h)).unwrap();
            std::fs::write(cache.join(h).join("preview-512.jpg"), b"x").unwrap();
        }
        cat.conn
            .execute_batch(
                "INSERT INTO photos(catalog_id, path, filename, blake3, captured_at, camera, lens, focal_mm, aperture, shutter, iso, width, height, rating, sync_state) VALUES (1, '/a.jpg', 'a.jpg', 'aaa', '', '', '', '', '', '', '', 0, 0, 0, 'local');",
            )
            .unwrap();
        // Referenced hash survives a drop request…
        cat.drop_preview_cache(&cache, "aaa");
        assert!(cache.join("aaa").exists());
        // …and vanishes once unreferenced.
        cat.conn.execute("DELETE FROM photos", []).unwrap();
        cat.drop_preview_cache(&cache, "aaa");
        assert!(!cache.join("aaa").exists());
        // Prune takes unknown hash dirs, keeps referenced ones.
        std::fs::create_dir_all(cache.join("bbb")).unwrap();
        std::fs::create_dir_all(cache.join("notahashdir")).unwrap();
        assert_eq!(cat.prune_preview_cache(&cache), 0);
        std::fs::create_dir_all(cache.join("c".repeat(64).as_str())).unwrap();
        assert_eq!(cat.prune_preview_cache(&cache), 1);
        assert!(cache.join("bbb").exists());
        assert!(cache.join("notahashdir").exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn u21_filter_budget_10k() {
        // U21 gate: find/filter a 10,000-photo catalog within 300 ms
        // after input settles (measured here, debug build included).
        let dir = workdir("u21budget");
        let db = dir.join("cat.db");
        let cat = Catalog::open(&db, "P", &dir).unwrap();
        cat.conn
            .execute_batch(
                &((0..10_000)
                    .map(|i| {
                        format!(
                            "INSERT INTO photos(catalog_id, path, filename, blake3, captured_at, camera, lens, focal_mm, aperture, shutter, iso, width, height, rating, sync_state) VALUES (1, '/shoot/img_{i:05}.nef', 'img_{i:05}.nef', 'hash{i:05}', '2026:01:02 10:00:00', '', '', '', '', '', '', 0, 0, {}, 'local');",
                            i % 6
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("")),
            )
            .unwrap();
        let photos = cat.all_photos();
        assert_eq!(photos.len(), 10_000);
        let t0 = std::time::Instant::now();
        let mut f = crate::state::Filters::default();
        f.min_stars = 3;
        f.search = "img_00".to_string();
        let mut hits: Vec<&DbPhoto> = photos.iter().filter(|p| f.matches_db(p, &[])).collect();
        hits.sort_by(|a, b| f.compare_db(a, b));
        let dt = t0.elapsed();
        eprintln!("u21: filter+sort 10k = {dt:?} (hits={})", hits.len());
        assert!(dt.as_millis() < 300, "filter budget blown: {dt:?}");
        assert!(!hits.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn u12_browser_lists_and_filter_presets() {
        // U12: folder scope math, preset roundtrip.
        let dir = workdir("u12");
        let sub = dir.join("shoot");
        std::fs::create_dir_all(&sub).unwrap();
        jpeg(&sub.join("a.jpg"), 64, 48);
        jpeg(&dir.join("b.jpg"), 32, 24);
        let cat = Catalog::open(&dir.join("cat.db"), "Test", &dir).unwrap();
        let cache = dir.join("cache");
        cat.import_file(&sub.join("a.jpg"), &cache).unwrap();
        cat.import_file(&dir.join("b.jpg"), &cache).unwrap();
        assert_eq!(cat.folder_of(&sub.join("a.jpg").to_string_lossy()), "shoot");
        assert_eq!(
            cat.folder_of(&dir.join("b.jpg").to_string_lossy()),
            "(root)"
        );
        assert!(cat.list_filter_presets().is_empty());
        cat.save_filter_preset("keepers", r#"{"min_stars":3}"#)
            .unwrap();
        let presets = cat.list_filter_presets();
        assert_eq!(presets.len(), 1);
        assert_eq!(presets[0].0, "keepers");
        cat.delete_filter_preset("keepers");
        assert!(cat.list_filter_presets().is_empty());
        // Component-wise root prefix: a sibling dir sharing the root's
        // name prefix is not "under" the root.
        let sibling = format!("{}2/x/c.jpg", dir.to_string_lossy());
        assert_ne!(cat.folder_of(&sibling), "2/x");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn remove_snapshot_covers_unedited_and_sync_state_tracks_all_jobs() {
        let dir = workdir("fixpass");
        jpeg(&dir.join("a.jpg"), 32, 24);
        let cat = Catalog::open(&dir.join("cat.db"), "Test", &dir).unwrap();
        let id = cat
            .import_file(&dir.join("a.jpg"), &dir.join("cache"))
            .unwrap()
            .unwrap();
        // Never-edited photos (no `edits` row) still snapshot for Remove.
        let snap = cat.snapshot_removed(id).expect("unedited photo snapshots");
        assert!(snap.params_json.is_none());
        // Sidecar finishing first must not verify the photo while its
        // original is still outstanding.
        cat.enqueue(id, "original");
        cat.enqueue(id, "sidecar");
        let first = cat.claim_job().unwrap();
        let second = cat.claim_job().unwrap();
        let (orig, side) = if first.kind == "original" {
            (first, second)
        } else {
            (second, first)
        };
        cat.fail_job(&orig, "outage");
        cat.complete_job(&side, None);
        assert_eq!(cat.photo_by_id(id).unwrap().sync, SyncState::Failed);
        // Re-enqueueing new work supersedes the failure.
        cat.enqueue(id, "original");
        assert_eq!(cat.queue_depth(), (1, 0));
        let job = cat.claim_job().unwrap();
        cat.complete_job(&job, Some("k"));
        assert_eq!(cat.photo_by_id(id).unwrap().sync, SyncState::Synced);
        // History-less edit rows keep their stored geometry on reload.
        let mut e = crate::edit::Edit::new();
        e.geom.rect = [0.1, 0.1, 0.5, 0.5];
        cat.save_params(id, &e).unwrap();
        let loaded = cat.load_all_edits();
        assert_eq!(loaded[0].1.geom.rect, [0.1, 0.1, 0.5, 0.5]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_relink_move_remove_roundtrip() {
        // U17: offline detection, hash-verified relink, managed move,
        // catalog removal — originals byte-identical throughout.
        let dir = workdir("u17");
        let shoot = dir.join("shoot");
        std::fs::create_dir_all(&shoot).unwrap();
        jpeg(&shoot.join("a.jpg"), 64, 48);
        jpeg(&shoot.join("b.jpg"), 32, 24);
        let pristine_a = std::fs::read(shoot.join("a.jpg")).unwrap();
        let cat = Catalog::open(&dir.join("cat.db"), "Test", &dir).unwrap();
        let cache = dir.join("cache");
        let a = cat
            .import_file(&shoot.join("a.jpg"), &cache)
            .unwrap()
            .unwrap();
        let b = cat
            .import_file(&shoot.join("b.jpg"), &cache)
            .unwrap()
            .unwrap();
        let hash_a = hash_file(&shoot.join("a.jpg")).unwrap();
        let hash_b = hash_file(&shoot.join("b.jpg")).unwrap();
        cat.set_rating(a, 5).unwrap();
        assert!(cat.missing_ids().is_empty());

        // Simulate a moved shoot: whole folder goes elsewhere.
        let moved = dir.join("moved");
        std::fs::create_dir_all(&moved).unwrap();
        std::fs::rename(shoot.join("a.jpg"), moved.join("a.jpg")).unwrap();
        std::fs::rename(shoot.join("b.jpg"), moved.join("b.jpg")).unwrap();
        let mut missing = cat.missing_ids();
        missing.sort();
        assert_eq!(missing, vec![a, b]);

        // Batch relink repairs descendants without reimporting.
        let report = cat.relink_folder(&shoot, &moved);
        assert_eq!(report.linked, 2);
        assert!(report.missing.is_empty() && report.mismatched.is_empty());
        assert!(cat.missing_ids().is_empty());
        assert_eq!(cat.photo_count(), 2);
        // Edits survived the outage untouched.
        let kept = cat.all_photos().into_iter().find(|p| p.id == a).unwrap();
        assert_eq!(kept.rating, 5);
        assert_eq!(kept.blake3, hash_a);

        // Wrong-content file under the same name is refused, with both
        // hashes named; the true bytes restore cleanly afterwards.
        std::fs::write(moved.join("a.jpg"), b"different-bytes").unwrap();
        let err = cat
            .relink_photo(a, &moved.join("a.jpg"))
            .expect_err("hash mismatch must refuse");
        assert!(err.contains("differs"), "{err}");
        std::fs::write(moved.join("a.jpg"), &pristine_a).unwrap();
        cat.relink_photo(a, &moved.join("a.jpg")).unwrap();

        // Managed move keeps bytes identical and the row current.
        let dest = dir.join("sorted");
        let new_path = cat.move_photo(b, &dest).unwrap();
        assert!(new_path.ends_with("sorted/b.jpg"), "{new_path}");
        assert_eq!(hash_file(Path::new(&new_path)).unwrap(), hash_b);
        assert!(!moved.join("b.jpg").exists());
        assert_eq!(
            cat.all_photos()
                .into_iter()
                .find(|p| p.id == b)
                .unwrap()
                .path,
            new_path
        );

        // Remove-from-catalog drops every row, never the files.
        cat.remove_photo(a).unwrap();
        assert!(cat.all_photos().iter().all(|p| p.id != a));
        assert!(moved.join("a.jpg").exists());
        assert_eq!(cat.missing_ids().len(), 0);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn backup_restore_and_cache_safety() {
        // U17: backup preserves metadata/history; restore recovers them;
        // wiping the preview cache never touches originals.
        let dir = workdir("u17b");
        let sub = dir.join("shoot");
        std::fs::create_dir_all(&sub).unwrap();
        jpeg(&sub.join("a.jpg"), 64, 48);
        let db = dir.join("cat.db");
        let backups = dir.join("backups");
        let before_hash;
        {
            let cat = Catalog::open(&db, "Test", &dir).unwrap();
            let cache = dir.join("cache");
            let id = cat
                .import_file(&sub.join("a.jpg"), &cache)
                .unwrap()
                .unwrap();
            before_hash = hash_file(&sub.join("a.jpg")).unwrap();
            let mut e = crate::edit::Edit::new();
            let mut p1 = crate::edit::defaults();
            p1[2] = 0.5;
            e.push_snap(
                "Exposure",
                "+0.50",
                crate::edit::Snap {
                    params: p1,
                    crop: None,
                    geom: crate::edit::CropGeom::default(),
                    curve_on: true,
                    hsl_on: false,
                    detail_on: true,
                    optics_on: true,
                    effects_on: true,
                    grading_on: true,
                    rating: 0,
                    picked: false,
                    rejected: false,
                },
            );
            cat.save_params(id, &e).unwrap();
            cat.set_rating(id, 4).unwrap();
            cat.set_keywords(id, &["x".to_string()]).unwrap();
            assert_eq!(cat.integrity_check().unwrap(), "ok — no orphaned rows");
            let dest = cat.backup_db(&backups).unwrap();
            assert!(dest.exists());
            let probe = Catalog::probe_backup(&dest).unwrap();
            assert_eq!(probe.photos, 1);
            // Mutate after the backup.
            cat.set_rating(id, 1).unwrap();
            cat.restore_db(&dest, &backups).unwrap();
        }
        // Reopen like the app does after restore: values are back.
        let cat = Catalog::open(&db, "Test", &dir).unwrap();
        let p = &cat.all_photos()[0];
        assert_eq!(p.rating, 4);
        assert_eq!(cat.photo_keywords(p.id), vec!["x"]);
        let edits = cat.load_all_edits();
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].1.history.len(), 1);
        assert_eq!(edits[0].1.history[0].label, "Exposure");
        // Cache rebuild: wipe derivatives, originals byte-identical.
        std::fs::remove_dir_all(dir.join("cache")).ok();
        assert_eq!(hash_file(&sub.join("a.jpg")).unwrap(), before_hash);
        assert!(cat.missing_ids().is_empty());
        // A non-catalog file is refused as a restore source.
        let bogus = dir.join("bogus.db");
        std::fs::write(&bogus, b"not sqlite").unwrap();
        assert!(Catalog::probe_backup(&bogus).is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn metadata_presets_save_list_and_default() {
        // V03: presets persist, last used first, import defaults remember.
        let dir = workdir("mdp");
        let cat = Catalog::open(&dir.join("cat.db"), "Test", &dir).unwrap();
        assert!(cat.list_metadata_presets().is_empty());
        cat.save_metadata_preset(&MetadataPreset {
            name: "Studio".into(),
            creator: "Ada".into(),
            copyright: "© 2026 Ada".into(),
            rights: "Editorial".into(),
            contact: "ada@example.com".into(),
            keywords: "wedding, Lisbon".into(),
            ..Default::default()
        })
        .unwrap();
        cat.save_metadata_preset(&MetadataPreset {
            name: "Personal".into(),
            ..Default::default()
        })
        .unwrap();
        let presets = cat.list_metadata_presets();
        assert_eq!(presets.len(), 2);
        assert_eq!(presets[0].name, "Personal"); // last saved first
        assert_eq!(presets[1].keywords, "wedding, Lisbon");
        cat.touch_metadata_preset("Studio");
        // Second granularity: deterministic only across ticks; just require
        // both present and touch not to error.
        assert_eq!(cat.list_metadata_presets().len(), 2);
        assert_eq!(cat.get_import_default("meta"), "");
        cat.set_import_default("meta", "Studio");
        assert_eq!(cat.get_import_default("meta"), "Studio");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn photo_metadata_keywords_and_shift() {
        // V03: authorship + keywords round-trip per photo; capture shift
        // moves catalog time, never the file.
        let dir = workdir("mdph");
        let sub = dir.join("shoot");
        std::fs::create_dir_all(&sub).unwrap();
        jpeg(&sub.join("a.jpg"), 64, 48);
        let cat = Catalog::open(&dir.join("cat.db"), "Test", &dir).unwrap();
        let cache = dir.join("cache");
        let id = cat
            .import_file(&sub.join("a.jpg"), &cache)
            .unwrap()
            .unwrap();
        cat.set_photo_metadata(id, "Ada", "© 2026", "Editorial", "ada@x.com")
            .unwrap();
        cat.set_keywords(id, &["b".to_string(), "a".to_string(), "b".to_string()])
            .unwrap();
        let auth = cat.photo_authorship(id);
        assert_eq!(auth.creator, "Ada");
        assert_eq!(auth.copyright, "© 2026");
        // Keywords dedupe on split, not on store (store is literal).
        assert_eq!(cat.photo_keywords(id), vec!["a", "b"]);
        assert_eq!(Catalog::split_keywords("a, b;a,, c "), vec!["a", "b", "c"]);
        // Capture shift: +90 min rolls the hour, keeps EXIF shape.
        assert_eq!(
            Catalog::shift_captured_at("2026:06:14 23:30:00", 90),
            "2026:06:15 01:00:00"
        );
        assert_eq!(
            Catalog::shift_captured_at("2026:01:01 00:15:00", -30),
            "2025:12:31 23:45:00"
        );
        assert_eq!(
            Catalog::shift_captured_at("2026:06:14 10:00:00", 0),
            "2026:06:14 10:00:00"
        );
        assert_eq!(Catalog::shift_captured_at("garbage", 60), "garbage");
        cat.apply_capture_offset(id, "2026:06:15 01:00:00", "2026:06:14 23:30:00", 90)
            .unwrap();
        let p = cat.all_photos().into_iter().find(|p| p.id == id).unwrap();
        assert_eq!(p.captured_at, "2026:06:15 01:00:00");
        assert_eq!(p.captured_orig, "2026:06:14 23:30:00");
        assert_eq!(p.capture_offset_min, 90);
        // Prototype-era catalogs gain the columns on open (mini-migration).
        let old = dir.join("old.db");
        {
            let conn = rusqlite::Connection::open(&old).unwrap();
            conn.execute_batch(
                "CREATE TABLE catalogs(id INTEGER PRIMARY KEY, name TEXT, root_path TEXT);
                 CREATE TABLE photos(id INTEGER PRIMARY KEY, catalog_id INTEGER, path TEXT UNIQUE,
                 filename TEXT, blake3 TEXT, captured_at TEXT, camera TEXT, lens TEXT, focal_mm TEXT,
                 aperture TEXT, shutter TEXT, iso TEXT, width INTEGER, height INTEGER);",
            )
            .unwrap();
        }
        let legacy = Catalog::open(&old, "Old", &dir).unwrap();
        assert!(legacy.all_photos().is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    fn set_mtime(path: &Path, secs: i64) {
        let t = filetime::FileTime::from_unix_time(secs, 0);
        filetime::set_file_mtime(path, t).unwrap();
    }

    fn now_secs() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap()
    }

    #[test]
    fn stale_sidecar_never_overwrites_catalog() {
        // U02: acknowledged DB values beat an older sidecar; rescan heals
        // the sidecar instead of clobbering the catalog.
        let dir = workdir("stale");
        let sub = dir.join("shoot");
        std::fs::create_dir_all(&sub).unwrap();
        jpeg(&sub.join("a.jpg"), 64, 48);
        let cat = Catalog::open(&dir.join("cat.db"), "Test", &dir).unwrap();
        let cache = dir.join("cache");
        let id = cat
            .import_file(&sub.join("a.jpg"), &cache)
            .unwrap()
            .unwrap();
        let before = hash_file(&sub.join("a.jpg")).unwrap();

        // Acknowledged save: rating 5 + warm params in the DB.
        let mut warm = vec![0f32; crate::edit::PARAM_COUNT];
        warm[0] = 7000.;
        cat.save_params(
            id,
            &crate::edit::Edit {
                params: warm.try_into().unwrap(),
                history: Vec::new(),
                cursor: 0,
                crop: None,
                geom: crate::edit::CropGeom::default(),
                curve_on: true,
                hsl_on: true,
                detail_on: true,
                optics_on: true,
                effects_on: true,
                grading_on: true,
            },
        )
        .unwrap();
        cat.set_rating(id, 5).unwrap();

        // A stale sidecar from before the save (cold params, rating 1).
        let mut cold = vec![0f32; crate::edit::PARAM_COUNT];
        cold[0] = 4000.;
        let photo = sub.join("a.jpg").to_string_lossy().to_string();
        crate::xmp::write(
            &photo,
            &cold.try_into().unwrap(),
            1,
            &[],
            None,
            &crate::xmp::Authorship::default(),
            &crate::edit::CropGeom::default(),
        )
        .unwrap();
        set_mtime(
            Path::new(&crate::xmp::sidecar_path(&photo)),
            now_secs() - 100,
        );

        let outcome = cat.apply_sidecar(id, &photo).unwrap();
        assert_eq!(outcome, SidecarApply::Stale);
        // Catalog values intact.
        let p = cat.all_photos().into_iter().find(|p| p.id == id).unwrap();
        assert_eq!(p.rating, 5);
        // Heal converges the sidecar to the acknowledged values.
        let report = cat.rescan_sidecars();
        assert_eq!(report.healed, 1);
        let side = crate::xmp::read(&photo).unwrap();
        assert_eq!(side.rating, Some(5));
        assert_eq!(side.params[0], 7000.);
        // The original is byte-identical through all of it.
        assert_eq!(hash_file(&sub.join("a.jpg")).unwrap(), before);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn external_sidecar_change_wins_and_reports() {
        // U02: an edit made outside Laika after our last write is adopted.
        let dir = workdir("ext");
        let sub = dir.join("shoot");
        std::fs::create_dir_all(&sub).unwrap();
        jpeg(&sub.join("a.jpg"), 64, 48);
        let cat = Catalog::open(&dir.join("cat.db"), "Test", &dir).unwrap();
        let cache = dir.join("cache");
        let id = cat
            .import_file(&sub.join("a.jpg"), &cache)
            .unwrap()
            .unwrap();
        let photo = sub.join("a.jpg").to_string_lossy().to_string();

        // Our acknowledged state, sidecar written by us.
        cat.save_params(id, &crate::edit::Edit::default()).unwrap();
        cat.set_rating(id, 2).unwrap();
        crate::xmp::write(
            &photo,
            &[0f32; crate::edit::PARAM_COUNT],
            2,
            &[],
            None,
            &crate::xmp::Authorship::default(),
            &crate::edit::CropGeom::default(),
        )
        .unwrap();
        cat.remember_sidecar_write(id, crate::xmp::sidecar_mtime(&photo).unwrap())
            .unwrap();

        // External editor bumps the rating afterwards.
        crate::xmp::write(
            &photo,
            &[0f32; crate::edit::PARAM_COUNT],
            4,
            &[],
            None,
            &crate::xmp::Authorship::default(),
            &crate::edit::CropGeom::default(),
        )
        .unwrap();
        set_mtime(
            Path::new(&crate::xmp::sidecar_path(&photo)),
            now_secs() + 100,
        );
        let outcome = cat.apply_sidecar(id, &photo).unwrap();
        assert_eq!(outcome, SidecarApply::AppliedExternal);
        let p = cat.all_photos().into_iter().find(|p| p.id == id).unwrap();
        assert_eq!(p.rating, 4);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn authored_sidecar_adopts_metadata_fieldwise() {
        // V03: external authorship merges into the catalog without blanking
        // fields the sidecar doesn't set.
        let dir = workdir("adoptmd");
        let sub = dir.join("shoot");
        std::fs::create_dir_all(&sub).unwrap();
        jpeg(&sub.join("a.jpg"), 64, 48);
        let cat = Catalog::open(&dir.join("cat.db"), "Test", &dir).unwrap();
        let cache = dir.join("cache");
        let id = cat
            .import_file(&sub.join("a.jpg"), &cache)
            .unwrap()
            .unwrap();
        cat.set_photo_metadata(id, "", "© Mine", "", "").unwrap();
        let photo = sub.join("a.jpg").to_string_lossy().to_string();
        let auth = crate::xmp::Authorship {
            creator: "Ada".into(),
            copyright: String::new(),
            rights_usage: String::new(),
            contact: String::new(),
            keywords: vec!["x".into()],
            ..Default::default()
        };
        crate::xmp::write(
            &photo,
            &[0f32; crate::edit::PARAM_COUNT],
            0,
            &[],
            None,
            &auth,
            &Default::default(),
        )
        .unwrap();
        let outcome = cat.apply_sidecar(id, &photo).unwrap();
        assert_eq!(outcome, SidecarApply::Applied);
        let back = cat.photo_authorship(id);
        assert_eq!(back.creator, "Ada");
        assert_eq!(back.copyright, "© Mine");
        assert_eq!(back.keywords, vec!["x"]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn corrupt_sidecar_keeps_catalog_values() {
        let dir = workdir("corrupt");
        let sub = dir.join("shoot");
        std::fs::create_dir_all(&sub).unwrap();
        jpeg(&sub.join("a.jpg"), 64, 48);
        let cat = Catalog::open(&dir.join("cat.db"), "Test", &dir).unwrap();
        let cache = dir.join("cache");
        let id = cat
            .import_file(&sub.join("a.jpg"), &cache)
            .unwrap()
            .unwrap();
        cat.save_params(id, &crate::edit::Edit::default()).unwrap();
        cat.set_rating(id, 3).unwrap();
        let photo = sub.join("a.jpg").to_string_lossy().to_string();
        std::fs::write(crate::xmp::sidecar_path(&photo), b"<not xml at all").unwrap();
        let outcome = cat.apply_sidecar(id, &photo).unwrap();
        assert_eq!(outcome, SidecarApply::Corrupt);
        let p = cat.all_photos().into_iter().find(|p| p.id == id).unwrap();
        assert_eq!(p.rating, 3);
        std::fs::remove_dir_all(&dir).ok();
    }
    #[test]
    fn history_persists_with_cursor_and_crop() {
        // U14: restart restores params, crop, full steps, and cursor.
        let dir = workdir("hist14");
        let sub = dir.join("shoot");
        std::fs::create_dir_all(&sub).unwrap();
        jpeg(&sub.join("a.jpg"), 64, 48);
        let db = dir.join("cat.db");
        let id = {
            let cat = Catalog::open(&db, "Test", &dir).unwrap();
            let cache = dir.join("cache");
            let id = cat
                .import_file(&sub.join("a.jpg"), &cache)
                .unwrap()
                .unwrap();
            let mut e = crate::edit::Edit::new();
            let base = crate::edit::Snap {
                params: crate::edit::defaults(),
                crop: None,
                geom: crate::edit::CropGeom::default(),
                curve_on: true,
                hsl_on: true,
                detail_on: true,
                optics_on: true,
                effects_on: true,
                grading_on: true,
                rating: 0,
                picked: false,
                rejected: false,
            };
            e.ensure_baseline(base);
            let mut p1 = crate::edit::defaults();
            p1[2] = 0.5;
            e.push_snap(
                "Exposure",
                "+0.50",
                crate::edit::Snap {
                    params: p1,
                    crop: Some(1.5),
                    geom: crate::edit::CropGeom::default(),
                    curve_on: true,
                    hsl_on: true,
                    detail_on: false,
                    optics_on: true,
                    effects_on: true,
                    grading_on: true,
                    rating: 4,
                    picked: true,
                    rejected: false,
                },
            );
            cat.save_params(id, &e).unwrap();
            id
        };
        // Reopen like a restart and reload.
        let cat = Catalog::open(&db, "Test", &dir).unwrap();
        let loaded: Vec<crate::edit::Edit> = cat
            .load_all_edits()
            .into_iter()
            .filter_map(|(pid, e)| (pid == id).then_some(e))
            .collect();
        assert_eq!(loaded.len(), 1);
        let e = &loaded[0];
        assert_eq!(e.params[2], 0.5);
        assert_eq!(e.crop, Some(1.5));
        assert_eq!(e.cursor, 2);
        assert_eq!(e.history.len(), 2);
        assert_eq!(e.history[0].label, "Import");
        assert_eq!(e.history[1].label, "Exposure");
        assert_eq!(e.history[1].rating, 4);
        assert!(e.history[1].picked);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn snapshots_save_list_restore_and_replace() {
        // U14: named full-state treatments round-trip; same name replaces.
        let dir = workdir("snap");
        let cat = Catalog::open(&dir.join("cat.db"), "Test", &dir).unwrap();
        let snap = crate::edit::Snap {
            params: crate::edit::defaults(),
            crop: Some(1.0),
            geom: crate::edit::CropGeom {
                rect: [0., 0., 1., 0.5],
                angle: 0.,
                flip_h: false,
                flip_v: false,
                rotation: 0,
                upright: Default::default(),
            },
            curve_on: false,
            hsl_on: true,
            detail_on: true,
            optics_on: false,
            effects_on: false,
            grading_on: false,
            rating: 5,
            picked: true,
            rejected: false,
        };
        let row = cat.save_snapshot(7, "Warm", &snap).unwrap();
        assert!(row > 0);
        let list = cat.list_snapshots(7);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].1, "Warm");
        assert_eq!(list[0].2.rating, 5);
        assert_eq!(list[0].2.crop, Some(1.0));
        // U18: bypass flags round-trip with the snapshot.
        assert!(!list[0].2.curve_on && !list[0].2.optics_on);
        assert!(!list[0].2.effects_on && !list[0].2.grading_on);
        assert!(list[0].2.hsl_on && list[0].2.detail_on);
        assert!(cat.list_snapshots(8).is_empty());
        // Same name replaces instead of duplicating.
        let snap2 = crate::edit::Snap { rating: 1, ..snap };
        cat.save_snapshot(7, "Warm", &snap2).unwrap();
        let list = cat.list_snapshots(7);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].2.rating, 1);
        cat.delete_snapshot(list[0].0);
        assert!(cat.list_snapshots(7).is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }
}
