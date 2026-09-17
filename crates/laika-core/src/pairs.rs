//! V05: RAW+JPEG pairing as a view over individual catalog rows.
//!
//! Rows stay one-per-file (sync, sidecars, and history are per file, and
//! toggling the preference re-groups without reimporting). A pair is two
//! rows in the same directory whose stems match case-insensitively with
//! one RAW side and one raster side. Videos never pair.

/// Group key for one capture: `dir + lowercase stem`. `None` when the
/// path cannot pair (unsupported extension or no stem).
pub fn pair_key(path: &str) -> Option<(String, String)> {
    let p = std::path::Path::new(path);
    let stem = p.file_stem()?.to_str()?.to_lowercase();
    // Apple Photos stores a pair's RAW as `<UUID>_4.<ext>` beside
    // `<UUID>.<ext>`.
    let stem = match stem.strip_suffix("_4") {
        Some(base) if crate::apple_photos::in_library(path) => base.to_string(),
        _ => stem,
    };
    if stem.is_empty() {
        return None;
    }
    let ext = p
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .unwrap_or_default();
    if !laika_raw::RAW_EXTS.contains(&ext.as_str())
        && !laika_raw::RASTER_EXTS.contains(&ext.as_str())
    {
        return None;
    }
    let dir = p
        .parent()
        .and_then(|d| d.to_str())
        .unwrap_or_default()
        .to_string();
    Some((dir, stem))
}

/// One resolved pair: the RAW row plus its JPEG-side row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pair {
    pub raw_id: i64,
    pub jpeg_id: i64,
}

/// Fold rows into complete pairs. `rows` is (id, path, is_raw).
pub fn find_pairs(rows: &[(i64, String, bool)]) -> Vec<Pair> {
    let mut by_key: std::collections::HashMap<String, (Option<i64>, Option<i64>)> =
        std::collections::HashMap::new();
    for (id, path, is_raw) in rows {
        let Some((dir, stem)) = pair_key(path) else {
            continue;
        };
        let key = format!("{dir}\0{stem}");
        let slot = by_key.entry(key).or_insert((None, None));
        if *is_raw {
            slot.0 = Some(*id);
        } else {
            slot.1 = Some(*id);
        }
    }
    by_key
        .into_values()
        .filter_map(|(raw, jpeg)| match (raw, jpeg) {
            (Some(r), Some(j)) => Some(Pair {
                raw_id: r,
                jpeg_id: j,
            }),
            _ => None,
        })
        .collect()
}

/// Sibling row id for a photo in a pair, if any.
pub fn sibling_of(pairs: &[Pair], id: i64) -> Option<i64> {
    pairs.iter().find_map(|p| {
        if p.raw_id == id {
            Some(p.jpeg_id)
        } else if p.jpeg_id == id {
            Some(p.raw_id)
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairs_match_dir_and_stem_case_insensitively() {
        assert_eq!(
            pair_key("/a/DCIM/DSC_1.NEF"),
            Some(("/a/DCIM".into(), "dsc_1".into()))
        );
        // Videos and sidecars never pair.
        assert_eq!(pair_key("/a/DCIM/MVI_1.MP4"), None);
        assert_eq!(pair_key("/a/DCIM/DSC_1.xmp"), None);
        assert_eq!(pair_key("/a/DCIM/.nef"), None);
    }

    #[test]
    fn grouping_needs_both_sides_in_one_folder() {
        let rows = vec![
            (1, "/s/a.nef".into(), true),
            (2, "/s/a.jpg".into(), false),
            (3, "/s/b.nef".into(), true),  // no jpeg side
            (4, "/s/c.jpg".into(), false), // no raw side
            (5, "/t/a.nef".into(), true),  // different folder
            (6, "/t/a.jpg".into(), false),
            (7, "/s/A.JPG".into(), false), // case-insensitive dup side
        ];
        let mut pairs = find_pairs(&rows);
        pairs.sort_by_key(|p| p.raw_id);
        // /s/a has two jpeg candidates: last one wins, still one pair.
        assert_eq!(pairs.len(), 2);
        assert!(pairs.iter().any(|p| p.raw_id == 1));
        assert!(pairs.iter().any(|p| p.raw_id == 5));
        assert_eq!(
            sibling_of(&pairs, 1),
            pairs.iter().find(|p| p.raw_id == 1).map(|p| p.jpeg_id)
        );
        assert_eq!(sibling_of(&pairs, 9), None);
    }
}
