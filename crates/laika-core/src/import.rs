//! Card-aware import engine (U05 + V01).
//!
//! - [`detect_volumes`]: mounted removable volumes (cards, drives) with
//!   capacity, for the import source picker. PTP/MTP devices appear only
//!   where the platform mounts them as folders; raw PTP needs a camera
//!   library and is out of scope.
//! - [`scan_source`]: file enumeration with `DCIM` preference and vendor
//!   junk (`MISC`, dot-dirs) skipped on cards.
//! - [`copy_verified`]: chunked copy with blake3 at both ends, atomic
//!   placement, one retry on mismatch, partial cleanup, cancellation.
//! - [`eject_volume`]: unmount after import when enabled.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// A mounted volume offered as an import source.
#[derive(Clone, Debug)]
pub struct Volume {
    pub mount_path: PathBuf,
    pub label: String,
    pub total_bytes: u64,
    pub avail_bytes: u64,
}

/// Stable-enough identity for duplicate memory: label plus canonical mount.
pub fn volume_id(v: &Volume) -> String {
    let mount = v
        .mount_path
        .canonicalize()
        .unwrap_or_else(|_| v.mount_path.clone())
        .to_string_lossy()
        .to_string();
    format!("{} :: {mount}", v.label)
}

fn same_device(a: &Path, b: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;
    match (std::fs::metadata(a), std::fs::metadata(b)) {
        (Ok(ma), Ok(mb)) => ma.dev() == mb.dev(),
        _ => false,
    }
}

fn capacity(mount: &Path) -> (u64, u64) {
    use std::ffi::CString;
    let Ok(c) = CString::new(mount.as_os_str().as_encoded_bytes()) else {
        return (0, 0);
    };
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: statvfs writes a valid struct on success; zeroed on failure.
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        return (0, 0);
    }
    (
        st.f_blocks as u64 * st.f_frsize as u64,
        st.f_bavail as u64 * st.f_frsize as u64,
    )
}

fn push_volume(out: &mut Vec<Volume>, mount: PathBuf) {
    if !mount.is_dir() {
        return;
    }
    let label = mount
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("volume")
        .to_string();
    let (total, avail) = capacity(&mount);
    out.push(Volume {
        mount_path: mount,
        label,
        total_bytes: total,
        avail_bytes: avail,
    });
}

/// Jennings' rule, minus the boot volume: every mounted volume that is not
/// the system device is offered (cards, USB drives, disk images).
pub fn detect_volumes() -> Vec<Volume> {
    let mut out = Vec::new();
    #[cfg(target_os = "macos")]
    {
        if let Ok(entries) = std::fs::read_dir("/Volumes") {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() && !same_device(&p, Path::new("/")) {
                    push_volume(&mut out, p);
                }
            }
        }
    }
    #[cfg(target_os = "linux")]
    {
        if let Ok(user) = std::env::var("USER") {
            for base in [format!("/media/{user}"), format!("/run/media/{user}")] {
                if let Ok(entries) = std::fs::read_dir(&base) {
                    for entry in entries.flatten() {
                        let p = entry.path();
                        if p.is_dir() {
                            push_volume(&mut out, p);
                        }
                    }
                }
            }
        }
    }
    out.sort_by(|a, b| a.label.cmp(&b.label));
    out
}

/// Card root for scanning: the `DCIM` subtree when present (still-photo
/// area), otherwise the whole volume.
pub fn card_scan_root(mount: &Path) -> PathBuf {
    let dcim = mount.join("DCIM");
    if dcim.is_dir() {
        dcim
    } else {
        mount.to_path_buf()
    }
}

fn skip_dir(name: &str) -> bool {
    name == "MISC" || name.starts_with('.')
}

/// Enumerate supported files. Card mode prefers `DCIM` and skips vendor
/// junk; folder mode is the plain recursive scan.
pub fn scan_source(root: &Path, card_mode: bool) -> Vec<PathBuf> {
    let base = if card_mode {
        card_scan_root(root)
    } else {
        root.to_path_buf()
    };
    let mut out = Vec::new();
    let mut stack = vec![base];
    // Symlinked directories are followed, each real directory once: a link
    // to an ancestor must not spin the scan forever.
    let mut visited = std::collections::HashSet::new();
    while let Some(dir) = stack.pop() {
        if !visited.insert(std::fs::canonicalize(&dir).unwrap_or_else(|_| dir.clone())) {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                let skip =
                    card_mode && p.file_name().and_then(|n| n.to_str()).is_some_and(skip_dir);
                if !skip {
                    stack.push(p);
                }
            } else if p.is_file() && laika_raw::is_supported(&p) {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

/// One enumerated file with review metadata (size + dates for the dialog).
#[derive(Clone, Debug)]
pub struct ScanEntry {
    pub path: PathBuf,
    pub size: u64,
    pub mtime_secs: i64,
    pub captured_at: String,
    pub camera: String,
    pub is_raw: bool,
    /// Content identity used to compare the source with the active catalog.
    pub content_hash: String,
    /// Filled by the app after the scan is compared with catalog/card history.
    pub previously_imported: bool,
}

fn scan_entry_metadata(p: &Path) -> ScanEntry {
    let (size, mtime_secs) = std::fs::metadata(p)
        .map(|m| {
            let t = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            (m.len(), t)
        })
        .unwrap_or((0, 0));
    let meta = laika_raw::exif::read_quick(p);
    ScanEntry {
        path: p.to_path_buf(),
        size,
        mtime_secs,
        captured_at: meta.captured_at,
        camera: meta.camera,
        is_raw: laika_raw::is_raw(p),
        content_hash: String::new(),
        previously_imported: false,
    }
}

/// Read one file's lightweight review metadata without streaming the whole
/// source. Card review uses this so import does not read every RAW twice.
pub fn scan_entry_quick(p: &Path) -> ScanEntry {
    scan_entry_metadata(p)
}

/// Read one file's review metadata and definitive content identity.
pub fn scan_entry(p: &Path) -> ScanEntry {
    let mut entry = scan_entry_metadata(p);
    entry.content_hash = crate::catalog::hash_file(p).unwrap_or_default();
    entry
}

/// Read per-file review metadata with cancellation between files.
pub fn scan_entries(paths: &[PathBuf], cancel: &AtomicBool) -> Vec<ScanEntry> {
    paths
        .iter()
        .take_while(|_| !cancel.load(Ordering::Relaxed))
        .map(|p| scan_entry(p))
        .collect()
}

/// Date range over capture time (falling back to file mtime) for review.
pub fn date_range(entries: &[ScanEntry]) -> Option<(String, String)> {
    let mut days: Vec<String> = entries
        .iter()
        .map(|e| {
            // `get`, not `[..10]`: a non-ASCII (corrupt) EXIF date must not
            // panic on a char boundary.
            if let Some(day) = e.captured_at.get(..10) {
                day.replace(':', "-")
            } else if e.mtime_secs > 0 {
                let days = e.mtime_secs / 86400;
                format!("epoch+{days}d")
            } else {
                String::new()
            }
        })
        .filter(|d| !d.is_empty())
        .collect();
    if days.is_empty() {
        return None;
    }
    days.sort();
    let first = days.first().cloned().unwrap();
    let last = days.last().cloned().unwrap();
    Some((first, last))
}

/// Outcome of one verified copy.
#[derive(Clone, Debug)]
pub struct CopyOutcome {
    /// Final placed path (collision suffix applied when needed).
    pub dest_path: PathBuf,
    /// True when the final name differs from the requested one.
    pub renamed: bool,
    pub bytes: u64,
    /// Hash computed while streaming the source. Callers can persist this
    /// instead of rereading the newly verified destination.
    pub content_hash: String,
}

/// Copy `src` into `dest_dir` with blake3 verified at both ends, placed
/// atomically. `filename` overrides the source file name (template output);
/// `None` keeps the source name. A hash mismatch retries the copy once,
/// then fails loudly — never silently. Partial files are always removed,
/// so a pulled card can leave no half-written file behind. `cancel` is
/// honored per chunk.
pub fn copy_verified(
    src: &Path,
    dest_dir: &Path,
    filename: Option<&str>,
    cancel: &AtomicBool,
    bytes_done: &AtomicU64,
) -> Result<CopyOutcome, String> {
    let src_name = src
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| format!("bad filename: {}", src.display()))?;
    let requested = filename.unwrap_or(src_name);
    if requested.is_empty() || requested == "." || requested == ".." {
        return Err(format!("refusing empty filename for {}", src.display()));
    }
    std::fs::create_dir_all(dest_dir).map_err(|e| format!("create {}: {e}", dest_dir.display()))?;
    let dest_path = unique_dest_name(dest_dir, requested);
    let renamed = dest_path
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| n != requested);
    let tmp = dest_dir.join(format!(
        ".part-{}-{}",
        std::process::id(),
        dest_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file")
    ));
    let mut last_err = String::new();
    for attempt in 0..2 {
        let _ = std::fs::remove_file(&tmp);
        match copy_once(src, &tmp, cancel, bytes_done) {
            Ok(src_hash) => match verify_copy(&tmp, &src_hash) {
                Ok(bytes) => {
                    std::fs::rename(&tmp, &dest_path)
                        .map_err(|e| format!("place {}: {e}", dest_path.display()))?;
                    return Ok(CopyOutcome {
                        dest_path,
                        renamed,
                        bytes,
                        content_hash: src_hash,
                    });
                }
                Err(e) => {
                    last_err = e;
                    let _ = std::fs::remove_file(&tmp);
                }
            },
            Err(e) => {
                last_err = e;
                let _ = std::fs::remove_file(&tmp);
                if last_err.starts_with("cancelled") || attempt == 1 {
                    break;
                }
            }
        }
        if cancel.load(Ordering::Relaxed) {
            let _ = std::fs::remove_file(&tmp);
            return Err("cancelled".to_string());
        }
    }
    let _ = std::fs::remove_file(&tmp);
    Err(last_err)
}

/// Stream-copy `src` to `tmp`, hashing source bytes as they are read.
fn copy_once(
    src: &Path,
    tmp: &Path,
    cancel: &AtomicBool,
    bytes_done: &AtomicU64,
) -> Result<String, String> {
    use std::io::{Read, Write};
    let mut fin = std::fs::File::open(src).map_err(|e| format!("read {}: {e}", src.display()))?;
    let mut fout =
        std::fs::File::create(tmp).map_err(|e| format!("write {}: {e}", tmp.display()))?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0u8; 1024 * 1024];
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Err("cancelled".to_string());
        }
        let n = fin
            .read(&mut buf)
            .map_err(|e| format!("read {}: {e}", src.display()))?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        fout.write_all(&buf[..n])
            .map_err(|e| format!("write {}: {e}", tmp.display()))?;
        bytes_done.fetch_add(n as u64, Ordering::Relaxed);
    }
    fout.flush()
        .map_err(|e| format!("write {}: {e}", tmp.display()))?;
    Ok(hasher.finalize().to_hex().to_string())
}

/// Hash the written file and compare with the source hash.
fn verify_copy(tmp: &Path, src_hash: &str) -> Result<u64, String> {
    let bytes = std::fs::metadata(tmp)
        .map(|m| m.len())
        .map_err(|e| format!("stat {}: {e}", tmp.display()))?;
    let mut hasher = blake3::Hasher::new();
    let mut f = std::fs::File::open(tmp).map_err(|e| format!("read {}: {e}", tmp.display()))?;
    std::io::copy(&mut f, &mut hasher).map_err(|e| format!("read {}: {e}", tmp.display()))?;
    let dest_hash = hasher.finalize().to_hex().to_string();
    if dest_hash != src_hash {
        return Err(format!(
            "hash mismatch after copy (src {src_hash} != copy {dest_hash})"
        ));
    }
    Ok(bytes)
}

/// Collision-free destination name: `name.jpg`, `name-2.jpg`, …
pub fn unique_dest_name(dest_dir: &Path, filename: &str) -> PathBuf {
    let first = dest_dir.join(filename);
    if !first.exists() {
        return first;
    }
    let (stem, ext) = match filename.rsplit_once('.') {
        Some((s, e)) => (s.to_string(), format!(".{e}")),
        None => (filename.to_string(), String::new()),
    };
    for n in 2..100000 {
        let cand = dest_dir.join(format!("{stem}-{n}{ext}"));
        if !cand.exists() {
            return cand;
        }
    }
    first
}

/// U17: move files to the OS Trash (recoverable delete). Backs the
/// Move-to-Trash action; failures name the reason. No extra dependency:
/// Finder on macOS, `gio trash` on Linux (with an honest error when gio
/// is absent).
pub fn trash_files(paths: &[PathBuf]) -> Result<(), String> {
    if paths.is_empty() {
        return Ok(());
    }
    #[cfg(target_os = "macos")]
    {
        for p in paths {
            let posix = p
                .to_string_lossy()
                .replace('\\', "\\\\")
                .replace('"', "\\\"");
            let script =
                format!(r#"tell application "Finder" to delete (POSIX file "{posix}" as alias)"#);
            let out = std::process::Command::new("osascript")
                .arg("-e")
                .arg(&script)
                .output()
                .map_err(|e| format!("move to trash: {e}"))?;
            if !out.status.success() {
                return Err(format!(
                    "move to trash failed for {}: {}",
                    p.display(),
                    String::from_utf8_lossy(&out.stderr).trim()
                ));
            }
        }
        Ok(())
    }
    #[cfg(target_os = "linux")]
    {
        let out = std::process::Command::new("gio")
            .arg("trash")
            .args(paths)
            .output()
            .map_err(|_| {
                "move to trash needs `gio` (glib2) — install it or remove from catalog instead"
                    .to_string()
            })?;
        if out.status.success() {
            Ok(())
        } else {
            Err(format!(
                "move to trash failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ))
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = paths;
        Err("move to trash is not supported on this platform".to_string())
    }
}

/// V05: play a video in the system player (Loupe has no decoder).
pub fn play_file(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let cmd = ("open", Vec::<String>::new());
    #[cfg(target_os = "linux")]
    let cmd = ("xdg-open", Vec::<String>::new());
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let cmd: (&str, Vec<String>) = ("", Vec::new());
    if cmd.0.is_empty() {
        return Err("playback is not supported on this platform".to_string());
    }
    std::process::Command::new(cmd.0)
        .args(&cmd.1)
        .arg(path)
        .status()
        .map_err(|e| format!("player failed to start ({e}) — is one installed?"))
        .and_then(|s| {
            if s.success() {
                Ok(())
            } else {
                Err("player exited with an error".to_string())
            }
        })
}

/// U17: reveal one file in Finder / the file manager.
pub fn reveal_in_manager(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("-R")
            .arg(path)
            .status()
            .map_err(|e| format!("reveal: {e}"))
            .and_then(|s| {
                if s.success() {
                    Ok(())
                } else {
                    Err("reveal failed".to_string())
                }
            })
    }
    #[cfg(target_os = "linux")]
    {
        let dir = path.parent().unwrap_or(Path::new("."));
        std::process::Command::new("xdg-open")
            .arg(dir)
            .status()
            .map_err(|e| format!("reveal (is xdg-open installed?): {e}"))
            .and_then(|s| {
                if s.success() {
                    Ok(())
                } else {
                    Err("reveal failed".to_string())
                }
            })
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = path;
        Err("reveal is not supported on this platform".to_string())
    }
}

/// V05: count-and-sample unsupported non-photo files next to a scan
/// (sidecars and dotfiles excluded — they are companions, not strays).
/// Separate walk from [`scan_source`] so every caller stays total.
pub fn count_unsupported(root: &Path, card_mode: bool) -> (usize, Vec<String>) {
    let base = if card_mode {
        card_scan_root(root)
    } else {
        root.to_path_buf()
    };
    let mut count = 0;
    let mut samples = Vec::new();
    let mut stack = vec![base];
    let mut visited = std::collections::HashSet::new();
    while let Some(dir) = stack.pop() {
        if !visited.insert(std::fs::canonicalize(&dir).unwrap_or_else(|_| dir.clone())) {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                let skip =
                    card_mode && p.file_name().and_then(|n| n.to_str()).is_some_and(skip_dir);
                if !skip {
                    stack.push(p);
                }
                continue;
            }
            if !p.is_file() {
                continue;
            }
            let name = p
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_string();
            if name.starts_with('.') || name.ends_with(".xmp") {
                continue;
            }
            if laika_raw::is_supported(&p) {
                continue;
            }
            count += 1;
            if samples.len() < 5 {
                samples.push(name);
            }
        }
    }
    (count, samples)
}

/// V04: a file is safe to auto-import only when two consecutive polls
/// agree on size and mtime and it is older than the settle window —
/// tethered apps and sync folders must never ingest a partial write.
pub fn file_stable(
    size_now: u64,
    mtime_now: i64,
    size_prev: u64,
    mtime_prev: i64,
    now_secs: i64,
) -> bool {
    const SETTLE_SECS: i64 = 15;
    size_now == size_prev && mtime_now == mtime_prev && now_secs - mtime_now >= SETTLE_SECS
}

/// Unmount a volume after import when the option is enabled. Failures are
/// reported, never fatal.
pub fn eject_volume(mount: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    {
        let out = std::process::Command::new("diskutil")
            .arg("unmount")
            .arg(mount)
            .output()
            .map_err(|e| format!("eject helper failed: {e}"))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(format!(
                "diskutil unmount failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            ))
        }
    }
    #[cfg(target_os = "linux")]
    {
        for cmd in ["udisksctl", "umount"] {
            let args: &[&str] = if cmd == "udisksctl" {
                &["unmount", "-p"]
            } else {
                &[]
            };
            let mut full: Vec<&str> = args.to_vec();
            full.push(&mount.to_string_lossy());
            if let Ok(out) = std::process::Command::new(cmd).args(&full).output() {
                if out.status.success() {
                    return Ok(());
                }
            }
        }
        Err(format!("could not unmount {}", mount.display()))
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        let _ = mount;
        Err("eject is not supported on this platform".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use std::sync::atomic::AtomicU64;

    fn workdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("laika-imp-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn no_cancel() -> AtomicBool {
        AtomicBool::new(false)
    }

    #[test]
    fn card_layout_prefers_dcim_and_skips_junk() {
        let dir = workdir("card");
        for p in [
            "DCIM/100NIKON/DSC_1.jpg",
            "DCIM/101NIKON/DSC_2.jpg",
            "PRIVATE/M4ROOT/CLIP001.mp4",
            "MISC/skip.jpg",
            ".hidden/a.jpg",
            "loose.jpg",
        ] {
            let f = dir.join(p);
            std::fs::create_dir_all(f.parent().unwrap()).unwrap();
            std::fs::write(&f, b"img").unwrap();
        }
        // Card mode: DCIM subtree only, MISC/dot dirs skipped.
        let mut found: Vec<String> = scan_source(&dir, true)
            .iter()
            .map(|p| p.strip_prefix(&dir).unwrap().to_string_lossy().to_string())
            .collect();
        found.sort();
        assert_eq!(
            found,
            ["DCIM/100NIKON/DSC_1.jpg", "DCIM/101NIKON/DSC_2.jpg"]
        );
        // Folder mode: everything supported, videos included (V05).
        let mut all: Vec<String> = scan_source(&dir, false)
            .iter()
            .map(|p| p.strip_prefix(&dir).unwrap().to_string_lossy().to_string())
            .collect();
        all.sort();
        assert!(all.contains(&"loose.jpg".to_string()));
        assert!(all.contains(&"MISC/skip.jpg".to_string()));
        assert!(all.contains(&"PRIVATE/M4ROOT/CLIP001.mp4".to_string()));
        // A symlink to an ancestor must not loop the walk forever.
        std::os::unix::fs::symlink(dir.join("DCIM"), dir.join("DCIM/100NIKON/loop")).unwrap();
        assert_eq!(scan_source(&dir, true).len(), 2);
        assert_eq!(scan_source(&dir, false).len(), 6);
        let _ = count_unsupported(&dir, false);
        // A non-ASCII capture string never panics the date range.
        let _ = date_range(&[ScanEntry {
            path: PathBuf::new(),
            size: 0,
            mtime_secs: 0,
            captured_at: "2024:01:0é 00:00".into(),
            camera: String::new(),
            is_raw: false,
            content_hash: String::new(),
            previously_imported: false,
        }]);
        std::fs::remove_file(dir.join("DCIM/100NIKON/loop")).unwrap();
        // No DCIM anywhere: card mode falls back to the whole tree.
        std::fs::remove_dir_all(dir.join("DCIM")).unwrap();
        let back = scan_source(&dir, true);
        assert!(back.iter().any(|p| p.ends_with("loose.jpg")));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn verified_copy_roundtrips_and_suffixes_collisions() {
        let dir = workdir("copy");
        let src = dir.join("src");
        let dst = dir.join("dst");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("a.jpg"), b"0123456789abcdef").unwrap();
        let bytes = AtomicU64::new(0);
        let o1 = copy_verified(&src.join("a.jpg"), &dst, None, &no_cancel(), &bytes).unwrap();
        assert!(!o1.renamed);
        assert_eq!(
            o1.content_hash,
            crate::catalog::hash_file(&o1.dest_path).unwrap()
        );
        assert_eq!(std::fs::read(&o1.dest_path).unwrap(), b"0123456789abcdef");
        assert_eq!(bytes.load(Ordering::Relaxed), 16);
        // Collision appends a suffix and reports it.
        let bytes2 = AtomicU64::new(0);
        let o2 = copy_verified(&src.join("a.jpg"), &dst, None, &no_cancel(), &bytes2).unwrap();
        assert!(o2.renamed);
        assert!(o2.dest_path.ends_with("a-2.jpg"));
        // No strays left behind.
        let parts: Vec<_> = std::fs::read_dir(&dst)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with(".part-"))
            .collect();
        assert!(parts.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn copy_honors_template_filename_override() {
        let dir = workdir("tplname");
        let src = dir.join("src");
        let dst = dir.join("dst");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("DSC_1.jpg"), b"0123456789abcdef").unwrap();
        let o = copy_verified(
            &src.join("DSC_1.jpg"),
            &dst,
            Some("2026-06-14_0007_DSC_1.jpg"),
            &no_cancel(),
            &AtomicU64::new(0),
        )
        .unwrap();
        assert!(!o.renamed);
        assert!(o.dest_path.ends_with("2026-06-14_0007_DSC_1.jpg"));
        assert_eq!(std::fs::read(&o.dest_path).unwrap(), b"0123456789abcdef");
        assert!(
            copy_verified(
                &src.join("x.jpg"),
                &dst,
                Some(""),
                &no_cancel(),
                &AtomicU64::new(0)
            )
            .is_err()
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn copy_failure_leaves_no_partial_and_no_row_material() {
        let dir = workdir("fail");
        let dst = dir.join("dst");
        std::fs::create_dir_all(&dst).unwrap();
        // Missing source: read fails, nothing placed.
        let err = copy_verified(
            &dir.join("gone.jpg"),
            &dst,
            None,
            &no_cancel(),
            &AtomicU64::new(0),
        )
        .expect_err("must fail");
        assert!(err.contains("gone.jpg"), "{err}");
        // Unwritable destination (missing parent chain under a file).
        std::fs::write(dir.join("blocker"), b"x").unwrap();
        let bad = dir.join("blocker").join("sub");
        let err = copy_verified(
            &dir.join("blocker"),
            &bad,
            None,
            &no_cancel(),
            &AtomicU64::new(0),
        );
        assert!(err.is_err());
        let parts: Vec<_> = std::fs::read_dir(&dst)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().starts_with(".part-"))
            .collect();
        assert!(parts.is_empty());
        // Cancelled copies clean up too.
        std::fs::write(dir.join("big.jpg"), vec![7u8; 64]).unwrap();
        let cancel = AtomicBool::new(true);
        let err = copy_verified(
            &dir.join("big.jpg"),
            &dst,
            None,
            &cancel,
            &AtomicU64::new(0),
        )
        .expect_err("cancelled");
        assert_eq!(err, "cancelled");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn stability_gate_rejects_partial_writes() {
        // V04: size/mtime must agree across polls and beat the window.
        assert!(file_stable(100, 1000, 100, 1000, 1015));
        assert!(!file_stable(100, 1000, 100, 1000, 1014)); // too fresh
        assert!(!file_stable(120, 1000, 100, 1000, 2000)); // still growing
        assert!(!file_stable(100, 1200, 100, 1000, 2000)); // touched
    }

    #[test]
    fn eject_reports_instead_of_pretending() {
        let err = eject_volume(Path::new("/nonexistent-volume-xyz")).expect_err("must fail");
        assert!(!err.is_empty());
    }

    #[test]
    fn volumes_and_ids_are_sane() {
        // Environment-dependent; must never crash and labels are non-empty.
        for v in detect_volumes() {
            assert!(!v.label.is_empty());
            assert!(!volume_id(&v).is_empty());
        }
        let fake = Volume {
            mount_path: PathBuf::from("/Volumes/CARD"),
            label: "CARD".into(),
            total_bytes: 1,
            avail_bytes: 1,
        };
        assert!(volume_id(&fake).starts_with("CARD :: "));
    }

    #[test]
    fn scan_entries_carry_review_metadata() {
        let dir = workdir("entries");
        std::fs::write(dir.join("a.jpg"), b"img").unwrap();
        std::fs::write(dir.join("note.txt"), b"x").unwrap();
        let paths = scan_source(&dir, false);
        assert_eq!(paths.len(), 1);
        let entries = scan_entries(&paths, &no_cancel());
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].size, 3);
        assert!(!entries[0].is_raw);
        assert_eq!(entries[0].content_hash.len(), 64);
        assert!(!entries[0].previously_imported);
        // Cancellation stops the walk.
        let cancel = AtomicBool::new(true);
        assert!(scan_entries(&paths, &cancel).is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn raw_scan_entries_avoid_full_decode() {
        let raw = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/raw/IMG_5442.NEF");
        assert!(raw.is_file(), "missing RAW regression fixture");
        let entries = scan_entries(&[raw], &no_cancel());
        assert_eq!(entries.len(), 1);
        assert!(entries[0].is_raw);
        assert_eq!(entries[0].content_hash.len(), 64);
    }
}
