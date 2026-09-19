//! S02: develop preset storage (migration v12).

use std::collections::BTreeMap;

use rusqlite::params;

use super::{Catalog, chrono_stamp};
use crate::presets::DevelopPreset;

/// What saving an imported preset did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PresetSave {
    Added(i64),
    /// Same group and name, different file: replaced in place.
    Replaced(i64),
    /// This exact file was imported before.
    Unchanged(i64),
}

impl Catalog {
    pub fn develop_presets(&self) -> Vec<DevelopPreset> {
        let mut stmt = match self.conn.prepare(
            "SELECT id, name, grp, values_json, supports_amount, skipped_json, approx_json,
                    source, digest
               FROM develop_presets ORDER BY grp COLLATE NOCASE, name COLLATE NOCASE",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        stmt.query_map([], |r| {
            let values: BTreeMap<usize, f32> =
                serde_json::from_str(&r.get::<_, String>(3)?).unwrap_or_default();
            Ok(DevelopPreset {
                id: r.get(0)?,
                name: r.get(1)?,
                group: r.get(2)?,
                values,
                supports_amount: r.get::<_, i64>(4)? != 0,
                skipped: serde_json::from_str(&r.get::<_, String>(5)?).unwrap_or_default(),
                approximations: serde_json::from_str(&r.get::<_, String>(6)?).unwrap_or_default(),
                source: r.get(7)?,
                digest: r.get(8)?,
            })
        })
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default()
    }

    pub fn develop_preset(&self, id: i64) -> Option<DevelopPreset> {
        self.develop_presets().into_iter().find(|p| p.id == id)
    }

    /// Store a preset. Same group + name replaces it (a re-imported or
    /// updated file); the identical file is left alone.
    pub fn save_develop_preset(&self, p: &DevelopPreset) -> Result<PresetSave, String> {
        let name = p.name.trim();
        if name.is_empty() {
            return Err("name the preset".to_string());
        }
        let existing: Option<(i64, String)> = self
            .conn
            .query_row(
                "SELECT id, digest FROM develop_presets WHERE grp = ?1 AND name = ?2",
                params![p.group.trim(), name],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok();
        if let Some((id, digest)) = &existing {
            if !p.digest.is_empty() && *digest == p.digest {
                return Ok(PresetSave::Unchanged(*id));
            }
        }
        let values = serde_json::to_string(&p.values).map_err(|e| e.to_string())?;
        let skipped = serde_json::to_string(&p.skipped).map_err(|e| e.to_string())?;
        let approx = serde_json::to_string(&p.approximations).map_err(|e| e.to_string())?;
        match existing {
            Some((id, _)) => {
                self.conn
                    .execute(
                        "UPDATE develop_presets SET values_json = ?1, supports_amount = ?2,
                           skipped_json = ?3, approx_json = ?4, source = ?5, digest = ?6
                         WHERE id = ?7",
                        params![
                            values,
                            p.supports_amount as i64,
                            skipped,
                            approx,
                            p.source,
                            p.digest,
                            id
                        ],
                    )
                    .map_err(|e| format!("save preset: {e}"))?;
                Ok(PresetSave::Replaced(id))
            }
            None => {
                self.conn
                    .execute(
                        "INSERT INTO develop_presets(name, grp, values_json, supports_amount,
                           skipped_json, approx_json, source, digest, created_at)
                         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                        params![
                            name,
                            p.group.trim(),
                            values,
                            p.supports_amount as i64,
                            skipped,
                            approx,
                            p.source,
                            p.digest,
                            chrono_stamp()
                        ],
                    )
                    .map_err(|e| format!("save preset: {e}"))?;
                Ok(PresetSave::Added(self.conn.last_insert_rowid()))
            }
        }
    }

    pub fn delete_develop_preset(&self, id: i64) -> Result<(), String> {
        self.conn
            .execute("DELETE FROM develop_presets WHERE id = ?1", [id])
            .map(|_| ())
            .map_err(|e| format!("delete preset: {e}"))
    }

    /// Update one stored preset, including its name/group. Unlike import,
    /// this addresses the row by id so rename never leaves the old row behind.
    pub fn update_develop_preset(&self, id: i64, p: &DevelopPreset) -> Result<(), String> {
        let name = p.name.trim();
        if name.is_empty() {
            return Err("name the preset".to_string());
        }
        let collision: Option<i64> = self
            .conn
            .query_row(
                "SELECT id FROM develop_presets WHERE grp = ?1 AND name = ?2 AND id <> ?3",
                params![p.group.trim(), name, id],
                |r| r.get(0),
            )
            .ok();
        if collision.is_some() {
            return Err(format!(
                "a preset named {name:?} already exists in this group"
            ));
        }
        let values = serde_json::to_string(&p.values).map_err(|e| e.to_string())?;
        let skipped = serde_json::to_string(&p.skipped).map_err(|e| e.to_string())?;
        let approx = serde_json::to_string(&p.approximations).map_err(|e| e.to_string())?;
        let changed = self
            .conn
            .execute(
                "UPDATE develop_presets SET name=?1, grp=?2, values_json=?3,
                   supports_amount=?4, skipped_json=?5, approx_json=?6,
                   source=?7, digest=?8 WHERE id=?9",
                params![
                    name,
                    p.group.trim(),
                    values,
                    p.supports_amount as i64,
                    skipped,
                    approx,
                    p.source,
                    p.digest,
                    id
                ],
            )
            .map_err(|e| format!("update preset: {e}"))?;
        if changed == 0 {
            Err("preset no longer exists".to_string())
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presets_save_replace_and_skip_identical_files() {
        let dir = std::env::temp_dir().join(format!("laika-presets-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        let cat = Catalog::open(&dir.join("c.db"), "c", &dir).unwrap();
        let mut p = DevelopPreset {
            name: "Punchy".into(),
            group: "Pack".into(),
            supports_amount: true,
            skipped: vec!["Lens profile correction".into()],
            digest: "a".into(),
            ..Default::default()
        };
        p.values.insert(2, 0.5);
        let PresetSave::Added(id) = cat.save_develop_preset(&p).unwrap() else {
            panic!("added")
        };
        assert_eq!(
            cat.save_develop_preset(&p).unwrap(),
            PresetSave::Unchanged(id)
        );
        p.digest = "b".into();
        p.values.insert(3, 20.);
        assert_eq!(
            cat.save_develop_preset(&p).unwrap(),
            PresetSave::Replaced(id)
        );
        let back = cat.develop_preset(id).unwrap();
        assert_eq!(back.values.get(&3), Some(&20.));
        assert!(back.supports_amount);
        assert_eq!(back.skipped, vec!["Lens profile correction".to_string()]);
        let mut renamed = back.clone();
        renamed.name = "Portrait".into();
        renamed.group = "People".into();
        cat.update_develop_preset(id, &renamed).unwrap();
        let back = cat.develop_preset(id).unwrap();
        assert_eq!(
            (back.name.as_str(), back.group.as_str()),
            ("Portrait", "People")
        );
        cat.delete_develop_preset(id).unwrap();
        assert!(cat.develop_presets().is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }
}
