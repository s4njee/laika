//! laika-raw: rawler wrapper.
//!
//! Phase 2: embedded preview extraction (no demosaic), EXIF read, resize to
//! derivative sizes. Full demosaic (`decode -> LinearImage`) lands in Phase 3.

pub mod decode;
pub mod exif;
pub mod preview;
pub mod system_image;

use std::path::Path;

/// RAW extensions handled via rawler (embedded preview, demosaic later).
pub const RAW_EXTS: &[&str] = &[
    "dng", "arw", "raf", "cr3", "cr2", "nef", "rw2", "orf", "pef", "srw", "rwl",
];

/// Rendered formats imported as-is (no rawler involved).
/// HEIC/HEIF decode through macOS ImageIO (`system_image`).
pub const RASTER_EXTS: &[&str] = &["jpg", "jpeg", "tif", "tiff", "png", "heic", "heif", "hif"];

/// V05: video containers (metadata parsed in-house, no decoder dependency).
pub const VIDEO_EXTS: &[&str] = &["mov", "mp4", "m4v"];

pub mod video;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaKind {
    Raw,
    Raster,
    Video,
}

/// V05: media kind by extension (`None` = unsupported).
pub fn media_kind(path: &Path) -> Option<MediaKind> {
    match ext_of(path).as_deref() {
        Some(e) if RAW_EXTS.contains(&e) => Some(MediaKind::Raw),
        Some(e) if RASTER_EXTS.contains(&e) => Some(MediaKind::Raster),
        Some(e) if VIDEO_EXTS.contains(&e) => Some(MediaKind::Video),
        _ => None,
    }
}

pub fn ext_of(path: &Path) -> Option<String> {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
}

pub fn is_raw(path: &Path) -> bool {
    ext_of(path)
        .as_deref()
        .is_some_and(|e| RAW_EXTS.contains(&e))
}

pub fn is_supported(path: &Path) -> bool {
    media_kind(path).is_some()
}

/// Run rawler decode work on a thread with a large stack. GPUI's background
/// pool threads are too small for NEF decompression (stack overflow).
pub fn on_big_stack<T: Send + 'static>(
    f: impl FnOnce() -> T + Send + 'static,
) -> Result<T, String> {
    std::thread::Builder::new()
        .name("laika-decode".into())
        .stack_size(64 << 20)
        .spawn(move || {
            user_initiated_thread();
            f()
        })
        .map_err(|e| e.to_string())?
        .join()
        .map_err(|_| "decode thread failed".to_string())
}

/// Mark the calling thread user-initiated (macOS QoS): decodes the user
/// is waiting on run on performance cores instead of being scheduled as
/// background work.
pub fn user_initiated_thread() {
    #[cfg(target_os = "macos")]
    {
        const QOS_CLASS_USER_INITIATED: u32 = 0x19;
        unsafe extern "C" {
            fn pthread_set_qos_class_self_np(qos_class: u32, relative_priority: i32) -> i32;
        }
        // SAFETY: affects only the calling thread's scheduling class.
        unsafe {
            pthread_set_qos_class_self_np(QOS_CLASS_USER_INITIATED, 0);
        }
    }
}

/// Recursive scan for supported files, sorted for stable import order.
pub fn scan_dir(root: &Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    // Symlinked directories are followed, but each real directory is
    // walked once (a link to an ancestor must not loop forever).
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
                stack.push(p);
            } else if p.is_file() && is_supported(&p) {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ext_sets() {
        assert!(is_raw(Path::new("a.DNG")));
        assert!(is_raw(Path::new("a.arw")));
        assert!(!is_raw(Path::new("a.jpg")));
        assert!(is_supported(Path::new("a.jpg")));
        assert!(is_supported(Path::new("a.CR3")));
        assert!(!is_supported(Path::new("a.xmp")));
        assert!(!is_supported(Path::new("a")));
        // V05: video containers are supported stills-adjacent citizens.
        assert!(is_supported(Path::new("a.MP4")));
        assert!(is_supported(Path::new("a.mov")));
        assert_eq!(media_kind(Path::new("a.nef")), Some(MediaKind::Raw));
        assert_eq!(media_kind(Path::new("a.jpg")), Some(MediaKind::Raster));
        assert_eq!(media_kind(Path::new("a.mp4")), Some(MediaKind::Video));
        assert_eq!(media_kind(Path::new("a.txt")), None);
    }

    #[test]
    fn scan_finds_nested_supported_only() {
        let root = std::env::temp_dir().join(format!("laika-scan-{}", std::process::id()));
        let sub = root.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        for name in ["a.dng", "b.JPG", "c.txt", "sub/d.arw", "sub/e.xmp"] {
            std::fs::write(root.join(name), b"data").unwrap();
        }
        let mut found = scan_dir(&root);
        let mut names: Vec<String> = found
            .drain(..)
            .map(|p| p.strip_prefix(&root).unwrap().to_str().unwrap().into())
            .collect();
        names.sort();
        assert_eq!(names, ["a.dng", "b.JPG", "sub/d.arw"]);
        // A symlink back to an ancestor must not loop forever.
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&root, sub.join("loop")).unwrap();
            assert_eq!(scan_dir(&root).len(), 3);
        }
        std::fs::remove_dir_all(&root).ok();
    }
}
