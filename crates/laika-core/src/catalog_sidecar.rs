//! S03: working beside Lightroom on the same files.
//!
//! When a sidecar changes on disk, Laika compares three versions of each
//! field group — the file (theirs), the catalog (ours), and the baseline
//! (both as last synchronized) — so only real conflicts need a decision:
//!
//! - only Lightroom changed a field → Laika adopts it;
//! - only Laika changed it → Laika's value stays (and is written back);
//! - both changed it differently → the catalog's policy decides:
//!   *Laika leads*, *Lightroom leads*, or *ask* (recorded as a conflict).
//!
//! *Lightroom leads* also keeps Laika from ever writing develop fields.

use std::collections::BTreeMap;

use rusqlite::params;
use serde::{Deserialize, Serialize};

use super::{Catalog, EditJson, PhotoMeta, chrono_stamp};
use crate::edit::{CropGeom, PARAM_COUNT};

/// How the catalog shares sidecars with Lightroom.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LightroomPolicy {
    /// The newer file or catalog wins as a whole (Laika's behavior before S03).
    #[default]
    Newest,
    /// Field by field; on a real conflict Laika's value wins.
    LaikaLeads,
    /// Field by field; Lightroom wins conflicts and Laika never writes
    /// develop settings.
    LightroomLeads,
    /// Field by field; conflicts wait for the photographer.
    Ask,
}

impl LightroomPolicy {
    pub const ALL: [LightroomPolicy; 4] = [
        LightroomPolicy::Newest,
        LightroomPolicy::LaikaLeads,
        LightroomPolicy::LightroomLeads,
        LightroomPolicy::Ask,
    ];

    pub fn key(self) -> &'static str {
        match self {
            LightroomPolicy::Newest => "newest",
            LightroomPolicy::LaikaLeads => "laika",
            LightroomPolicy::LightroomLeads => "lightroom",
            LightroomPolicy::Ask => "ask",
        }
    }

    pub fn from_key(s: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|p| p.key() == s)
            .unwrap_or_default()
    }

    pub fn label(self) -> &'static str {
        match self {
            LightroomPolicy::Newest => "Newest wins",
            LightroomPolicy::LaikaLeads => "Laika leads",
            LightroomPolicy::LightroomLeads => "Lightroom leads",
            LightroomPolicy::Ask => "Ask on conflict",
        }
    }

    pub fn detail(self) -> &'static str {
        match self {
            LightroomPolicy::Newest => {
                "Whichever changed a photo last replaces the other's values."
            }
            LightroomPolicy::LaikaLeads => {
                "Changes merge field by field; when both apps change the same field, Laika's value stays."
            }
            LightroomPolicy::LightroomLeads => {
                "Changes merge field by field; Lightroom wins conflicts, and Laika never writes develop settings to the sidecar."
            }
            LightroomPolicy::Ask => {
                "Changes merge field by field; when both apps change the same field, you choose."
            }
        }
    }

    /// Whether field-by-field merging (and Adobe sidecar names) apply.
    pub fn shares(self) -> bool {
        self != LightroomPolicy::Newest
    }

    /// Laika may write develop fields to sidecars.
    pub fn writes_develop(self) -> bool {
        self != LightroomPolicy::LightroomLeads
    }
}

/// Field groups merged independently.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SideGroup {
    Rating,
    Label,
    Keywords,
    Description,
    Rights,
    Location,
    Develop,
}

impl SideGroup {
    pub const ALL: [SideGroup; 7] = [
        SideGroup::Rating,
        SideGroup::Label,
        SideGroup::Keywords,
        SideGroup::Description,
        SideGroup::Rights,
        SideGroup::Location,
        SideGroup::Develop,
    ];

    pub fn key(self) -> &'static str {
        match self {
            SideGroup::Rating => "rating",
            SideGroup::Label => "label",
            SideGroup::Keywords => "keywords",
            SideGroup::Description => "description",
            SideGroup::Rights => "rights",
            SideGroup::Location => "location",
            SideGroup::Develop => "develop",
        }
    }

    pub fn from_key(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|g| g.key() == s)
    }

    pub fn label(self) -> &'static str {
        match self {
            SideGroup::Rating => "Rating",
            SideGroup::Label => "Color label",
            SideGroup::Keywords => "Keywords",
            SideGroup::Description => "Title & caption",
            SideGroup::Rights => "Creator & copyright",
            SideGroup::Location => "GPS location",
            SideGroup::Develop => "Develop settings",
        }
    }
}

/// The sidecar-visible state of one photo, normalized for comparison.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct SideFields {
    pub rating: u8,
    pub label: String,
    pub keywords: Vec<String>,
    pub title: String,
    pub caption: String,
    pub headline: String,
    pub location: String,
    pub creator: String,
    pub copyright: String,
    pub rights: String,
    pub contact: String,
    pub gps: Option<[i64; 2]>,
    /// Rounded (×1000) so float noise never reads as a change.
    pub params: Vec<i64>,
    pub geom: [i64; 7],
}

fn round_params(p: &[f32; PARAM_COUNT]) -> Vec<i64> {
    p.iter().map(|v| (v * 1000.).round() as i64).collect()
}

fn round_geom(g: &CropGeom) -> [i64; 7] {
    [
        (g.rect[0] * 10_000.).round() as i64,
        (g.rect[1] * 10_000.).round() as i64,
        (g.rect[2] * 10_000.).round() as i64,
        (g.rect[3] * 10_000.).round() as i64,
        (g.angle * 100.).round() as i64,
        (g.flip_h as i64) | ((g.flip_v as i64) << 1),
        g.rotation as i64,
    ]
}

fn norm_keywords(k: &[String]) -> Vec<String> {
    let mut v: Vec<String> = k
        .iter()
        .map(|s| Catalog::canon_keyword_path(s))
        .filter(|s| !s.is_empty())
        .collect();
    v.sort_by_key(|s| s.to_lowercase());
    v.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    v
}

fn norm_gps(s: &str) -> Option<[i64; 2]> {
    crate::geo::parse_gps(s).map(|(a, b)| [(a * 1e5).round() as i64, (b * 1e5).round() as i64])
}

impl SideFields {
    fn group_eq(&self, other: &SideFields, g: SideGroup) -> bool {
        match g {
            SideGroup::Rating => self.rating == other.rating,
            SideGroup::Label => self.label.eq_ignore_ascii_case(&other.label),
            SideGroup::Keywords => self.keywords == other.keywords,
            SideGroup::Description => {
                (&self.title, &self.caption, &self.headline, &self.location)
                    == (
                        &other.title,
                        &other.caption,
                        &other.headline,
                        &other.location,
                    )
            }
            SideGroup::Rights => {
                (&self.creator, &self.copyright, &self.rights, &self.contact)
                    == (
                        &other.creator,
                        &other.copyright,
                        &other.rights,
                        &other.contact,
                    )
            }
            SideGroup::Location => self.gps == other.gps,
            SideGroup::Develop => self.params == other.params && self.geom == other.geom,
        }
    }

    /// Copy one group's values from `src`.
    fn take_group(&mut self, src: &SideFields, g: SideGroup) {
        match g {
            SideGroup::Rating => self.rating = src.rating,
            SideGroup::Label => self.label = src.label.clone(),
            SideGroup::Keywords => self.keywords = src.keywords.clone(),
            SideGroup::Description => {
                self.title = src.title.clone();
                self.caption = src.caption.clone();
                self.headline = src.headline.clone();
                self.location = src.location.clone();
            }
            SideGroup::Rights => {
                self.creator = src.creator.clone();
                self.copyright = src.copyright.clone();
                self.rights = src.rights.clone();
                self.contact = src.contact.clone();
            }
            SideGroup::Location => self.gps = src.gps,
            SideGroup::Develop => {
                self.params = src.params.clone();
                self.geom = src.geom;
            }
        }
    }

    /// Short human text for a group (conflict screen).
    pub fn describe(&self, g: SideGroup, other: Option<&SideFields>) -> String {
        let or_none = |s: &str| {
            if s.trim().is_empty() {
                "—".to_string()
            } else {
                s.to_string()
            }
        };
        match g {
            SideGroup::Rating => {
                if self.rating == 0 {
                    "No rating".to_string()
                } else {
                    "★".repeat(self.rating as usize)
                }
            }
            SideGroup::Label => or_none(&self.label),
            SideGroup::Keywords => {
                if self.keywords.is_empty() {
                    "No keywords".to_string()
                } else {
                    self.keywords.join(", ")
                }
            }
            SideGroup::Description => {
                let mut parts = Vec::new();
                for (k, v) in [
                    ("Title", &self.title),
                    ("Caption", &self.caption),
                    ("Headline", &self.headline),
                    ("Location", &self.location),
                ] {
                    if !v.is_empty() {
                        parts.push(format!("{k}: {v}"));
                    }
                }
                if parts.is_empty() {
                    "—".to_string()
                } else {
                    parts.join(" · ")
                }
            }
            SideGroup::Rights => {
                let mut parts = Vec::new();
                for (k, v) in [
                    ("Creator", &self.creator),
                    ("Copyright", &self.copyright),
                    ("Usage", &self.rights),
                    ("Contact", &self.contact),
                ] {
                    if !v.is_empty() {
                        parts.push(format!("{k}: {v}"));
                    }
                }
                if parts.is_empty() {
                    "—".to_string()
                } else {
                    parts.join(" · ")
                }
            }
            SideGroup::Location => match self.gps {
                Some([a, b]) => crate::geo::display_gps(a as f64 / 1e5, b as f64 / 1e5),
                None => "No location".to_string(),
            },
            SideGroup::Develop => {
                // Name the sliders that differ from the other version.
                let mut parts = Vec::new();
                if let Some(o) = other {
                    for (i, (a, b)) in self.params.iter().zip(o.params.iter()).enumerate() {
                        if a != b && i < PARAM_COUNT {
                            let def = &crate::edit::PARAMS[i];
                            parts.push(format!(
                                "{} {}",
                                def.label,
                                crate::edit::format(i, *a as f32 / 1000.)
                            ));
                        }
                    }
                    if self.geom != o.geom {
                        parts.push("crop/rotation".to_string());
                    }
                }
                if parts.is_empty() {
                    "Develop settings".to_string()
                } else if parts.len() > 5 {
                    format!("{} and {} more", parts[..5].join(", "), parts.len() - 5)
                } else {
                    parts.join(", ")
                }
            }
        }
    }
}

/// One unresolved conflict.
#[derive(Clone, Debug, PartialEq)]
pub struct SidecarConflict {
    pub photo_id: i64,
    pub group: SideGroup,
    pub ours: SideFields,
    pub theirs: SideFields,
    /// Who wrote the file ("Lightroom Classic 13.0").
    pub writer: String,
}

/// What one merge did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MergeOutcome {
    pub adopted: Vec<SideGroup>,
    /// Groups where Laika's value stands and must be written back.
    pub kept: Vec<SideGroup>,
    pub conflicts: Vec<SideGroup>,
}

impl MergeOutcome {
    pub fn needs_write(&self) -> bool {
        !self.kept.is_empty()
    }
}

impl Catalog {
    pub fn lightroom_policy(&self) -> LightroomPolicy {
        LightroomPolicy::from_key(&self.get_import_default("lightroom_policy"))
    }

    pub fn set_lightroom_policy(&self, p: LightroomPolicy) {
        self.set_import_default("lightroom_policy", p.key());
    }

    /// Develop params of one photo (defaults when unedited).
    fn params_of(&self, id: i64) -> [f32; PARAM_COUNT] {
        self.conn
            .query_row(
                "SELECT params_json FROM edits WHERE photo_id = ?1",
                [id],
                |r| r.get::<_, String>(0),
            )
            .ok()
            .and_then(|j| {
                serde_json::from_str::<EditJson>(&j)
                    .map(|e| e.params)
                    .or_else(|_| serde_json::from_str::<Vec<f32>>(&j))
                    .ok()
            })
            .and_then(|v| crate::edit::pad_params(&v))
            .unwrap_or_else(crate::edit::defaults)
    }

    /// The catalog's side of the comparison.
    pub fn catalog_fields(&self, id: i64) -> Option<SideFields> {
        let p = self.photo_by_id(id)?;
        let meta = self.photo_meta(id);
        Some(SideFields {
            rating: p.rating,
            label: self.label_names().name(p.label).to_string(),
            keywords: norm_keywords(&self.photo_keywords(id)),
            title: meta.title.trim().to_string(),
            caption: meta.caption.trim().to_string(),
            headline: meta.headline.trim().to_string(),
            location: meta.location.trim().to_string(),
            creator: meta.creator.trim().to_string(),
            copyright: meta.copyright.trim().to_string(),
            rights: meta.rights.trim().to_string(),
            contact: meta.contact.trim().to_string(),
            gps: norm_gps(&p.exif_gps),
            params: round_params(&self.params_of(id)),
            geom: round_geom(&self.geom_of(id)),
        })
    }

    /// The file's side of the comparison.
    pub fn sidecar_fields(&self, side: &crate::xmp::Sidecar) -> SideFields {
        let names = self.label_names();
        let label = match side.label.as_deref() {
            None => String::new(),
            Some(t) => match names.from_xmp(t) {
                Some(v) => names.name(v).to_string(),
                // Text Laika doesn't know (e.g. "Select"): not a Laika label.
                None => String::new(),
            },
        };
        SideFields {
            rating: side.rating.unwrap_or(0),
            label,
            keywords: norm_keywords(&side.keywords),
            title: side.title.trim().to_string(),
            caption: side.caption.trim().to_string(),
            headline: side.headline.trim().to_string(),
            location: side.location.trim().to_string(),
            creator: side.creator.trim().to_string(),
            copyright: side.copyright.trim().to_string(),
            rights: side.rights_usage.trim().to_string(),
            contact: side.contact.trim().to_string(),
            gps: norm_gps(&side.gps),
            params: round_params(&side.params),
            geom: round_geom(&side.geom.unwrap_or_default()),
        }
    }

    fn baseline(&self, id: i64) -> Option<SideFields> {
        self.conn
            .query_row(
                "SELECT fields_json FROM sidecar_baseline WHERE photo_id = ?1",
                [id],
                |r| r.get::<_, String>(0),
            )
            .ok()
            .and_then(|j| serde_json::from_str(&j).ok())
    }

    fn store_baseline(&self, id: i64, f: &SideFields) -> Result<(), String> {
        let j = serde_json::to_string(f).map_err(|e| e.to_string())?;
        self.conn
            .execute(
                "INSERT INTO sidecar_baseline(photo_id, fields_json, updated_at) VALUES (?1,?2,?3)
                 ON CONFLICT(photo_id) DO UPDATE SET fields_json = ?2, updated_at = ?3",
                params![id, j, chrono_stamp()],
            )
            .map(|_| ())
            .map_err(|e| format!("sidecar baseline: {e}"))
    }

    /// Record what the sidecar holds now as the synchronized state
    /// (after Laika writes it or adopts it). Groups with an open conflict
    /// keep their old baseline.
    pub fn record_sidecar_baseline(&self, id: i64, photo_path: &str) {
        let Some(side) = crate::xmp::read(photo_path) else {
            return;
        };
        let mut fields = self.sidecar_fields(&side);
        let open = self.conflict_groups(id);
        if !open.is_empty() {
            if let Some(old) = self.baseline(id) {
                for g in open {
                    fields.take_group(&old, g);
                }
            }
        }
        self.store_baseline(id, &fields).ok();
    }

    fn conflict_groups(&self, id: i64) -> Vec<SideGroup> {
        let mut stmt = match self
            .conn
            .prepare("SELECT grp FROM sidecar_conflicts WHERE photo_id = ?1")
        {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        stmt.query_map([id], |r| r.get::<_, String>(0))
            .map(|rows| {
                rows.flatten()
                    .filter_map(|g| SideGroup::from_key(&g))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn has_sidecar_conflict(&self, id: i64) -> bool {
        !self.conflict_groups(id).is_empty()
    }

    pub fn sidecar_conflict_count(&self) -> usize {
        self.conn
            .query_row(
                "SELECT count(DISTINCT photo_id) FROM sidecar_conflicts",
                [],
                |r| r.get::<_, i64>(0),
            )
            .unwrap_or(0) as usize
    }

    pub fn sidecar_conflicts(&self) -> Vec<SidecarConflict> {
        let mut stmt = match self.conn.prepare(
            "SELECT photo_id, grp, ours_json, theirs_json, writer FROM sidecar_conflicts
              ORDER BY photo_id, grp",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        })
        .map(|rows| {
            rows.flatten()
                .filter_map(|(id, g, o, t, w)| {
                    Some(SidecarConflict {
                        photo_id: id,
                        group: SideGroup::from_key(&g)?,
                        ours: serde_json::from_str(&o).ok()?,
                        theirs: serde_json::from_str(&t).ok()?,
                        writer: w,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
    }

    /// Write one group's values from `f` into the catalog.
    fn apply_group(
        &self,
        id: i64,
        g: SideGroup,
        f: &SideFields,
        side: Option<&crate::xmp::Sidecar>,
    ) -> Result<(), String> {
        match g {
            SideGroup::Rating => self.set_rating(id, f.rating),
            SideGroup::Label => {
                let v = self.label_names().from_xmp(&f.label).unwrap_or(0);
                self.set_label(id, v)
            }
            SideGroup::Keywords => self.set_keywords(id, &f.keywords),
            SideGroup::Description | SideGroup::Rights => {
                let mut meta = self.photo_meta(id);
                if g == SideGroup::Description {
                    meta.title = f.title.clone();
                    meta.caption = f.caption.clone();
                    meta.headline = f.headline.clone();
                    meta.location = f.location.clone();
                } else {
                    meta.creator = f.creator.clone();
                    meta.copyright = f.copyright.clone();
                    meta.rights = f.rights.clone();
                    meta.contact = f.contact.clone();
                }
                self.set_photo_meta(id, &PhotoMeta { ..meta })
            }
            SideGroup::Location => {
                self.set_gps(id, f.gps.map(|[a, b]| (a as f64 / 1e5, b as f64 / 1e5)))
            }
            SideGroup::Develop => {
                // Exact values come from the file when we have it; the
                // stored snapshot is rounded.
                let (params, geom) = match side {
                    Some(s) => (s.params, s.geom.unwrap_or_default()),
                    None => {
                        let mut p = crate::edit::defaults();
                        for (i, v) in f.params.iter().enumerate().take(PARAM_COUNT) {
                            p[i] = *v as f32 / 1000.;
                        }
                        let mut g = self.geom_of(id);
                        g.rect = [
                            f.geom[0] as f32 / 10_000.,
                            f.geom[1] as f32 / 10_000.,
                            f.geom[2] as f32 / 10_000.,
                            f.geom[3] as f32 / 10_000.,
                        ];
                        g.angle = f.geom[4] as f32 / 100.;
                        g.flip_h = f.geom[5] & 1 != 0;
                        g.flip_v = f.geom[5] & 2 != 0;
                        g.rotation = f.geom[6] as u8;
                        (p, g)
                    }
                };
                self.save_params(
                    id,
                    &crate::edit::Edit {
                        params,
                        history: Vec::new(),
                        cursor: 0,
                        crop: None,
                        geom,
                        curve_on: true,
                        hsl_on: true,
                        detail_on: true,
                        optics_on: true,
                        effects_on: true,
                        grading_on: true,
                        locals: Default::default(),
                        camera_profile: Default::default(),
                    },
                )
            }
        }
    }

    /// Merge a sidecar another app changed, field group by field group.
    /// Returns None when there is no baseline yet (first contact — the
    /// caller falls back to whole-file adoption).
    pub fn merge_external_sidecar(
        &self,
        id: i64,
        photo_path: &str,
        policy: LightroomPolicy,
    ) -> Result<Option<MergeOutcome>, String> {
        let Some(base) = self.baseline(id) else {
            return Ok(None);
        };
        let Some(side) = crate::xmp::read(photo_path) else {
            return Ok(Some(MergeOutcome::default()));
        };
        let theirs = self.sidecar_fields(&side);
        let ours = self.catalog_fields(id).ok_or("photo not in catalog")?;
        let writer = std::fs::read(crate::xmp::sidecar_path(photo_path))
            .ok()
            .and_then(|b| crate::sidecar::last_writer(&b))
            .map(|w| w.app)
            .unwrap_or_else(|| "another app".to_string());
        let mut out = MergeOutcome::default();
        let mut new_base = base.clone();
        let open = self.conflict_groups(id);
        for g in SideGroup::ALL {
            let t_changed = !theirs.group_eq(&base, g);
            let o_changed = !ours.group_eq(&base, g);
            if !t_changed {
                // Laika's own change (if any) goes out on the next write —
                // except develop while Lightroom leads.
                if o_changed && !(g == SideGroup::Develop && !policy.writes_develop()) {
                    out.kept.push(g);
                }
                continue;
            }
            if !o_changed || ours.group_eq(&theirs, g) {
                self.apply_group(id, g, &theirs, Some(&side))?;
                new_base.take_group(&theirs, g);
                out.adopted.push(g);
                continue;
            }
            // Both changed, differently.
            let decide = match policy {
                LightroomPolicy::LaikaLeads | LightroomPolicy::Newest => Some(false),
                LightroomPolicy::LightroomLeads => Some(true),
                LightroomPolicy::Ask => None,
            };
            match decide {
                Some(true) => {
                    self.apply_group(id, g, &theirs, Some(&side))?;
                    new_base.take_group(&theirs, g);
                    out.adopted.push(g);
                }
                Some(false) => {
                    new_base.take_group(&theirs, g);
                    out.kept.push(g);
                }
                None => {
                    if !open.contains(&g) {
                        self.conn
                            .execute(
                                "INSERT INTO sidecar_conflicts(photo_id, grp, ours_json, theirs_json, writer, detected_at)
                                 VALUES (?1,?2,?3,?4,?5,?6)
                                 ON CONFLICT(photo_id, grp) DO UPDATE SET ours_json = ?3, theirs_json = ?4,
                                   writer = ?5, detected_at = ?6",
                                params![
                                    id,
                                    g.key(),
                                    serde_json::to_string(&ours).map_err(|e| e.to_string())?,
                                    serde_json::to_string(&theirs).map_err(|e| e.to_string())?,
                                    writer,
                                    chrono_stamp()
                                ],
                            )
                            .map_err(|e| format!("record conflict: {e}"))?;
                    }
                    out.conflicts.push(g);
                }
            }
        }
        self.store_baseline(id, &new_base)?;
        let mtime = crate::xmp::sidecar_mtime(photo_path).unwrap_or(0);
        self.remember_mtime(id, mtime)?;
        Ok(Some(out))
    }

    /// Resolve one conflict: `take_theirs` adopts the file's values, else
    /// Laika's stand (and are written back once the photo has no other
    /// open conflicts). Returns true when the photo has none left.
    pub fn resolve_sidecar_conflict(
        &self,
        id: i64,
        g: SideGroup,
        take_theirs: bool,
    ) -> Result<bool, String> {
        let Some(c) = self
            .sidecar_conflicts()
            .into_iter()
            .find(|c| c.photo_id == id && c.group == g)
        else {
            return Ok(!self.has_sidecar_conflict(id));
        };
        if take_theirs {
            let path = self.photo_by_id(id).map(|p| p.path).unwrap_or_default();
            let side = crate::xmp::read(&path);
            // Use the file's exact values if it still matches the conflict.
            let exact = side
                .as_ref()
                .filter(|s| self.sidecar_fields(s).group_eq(&c.theirs, g));
            self.apply_group(id, g, &c.theirs, exact)?;
        }
        let mut base = self.baseline(id).unwrap_or_default();
        base.take_group(&c.theirs, g);
        self.store_baseline(id, &base)?;
        self.conn
            .execute(
                "DELETE FROM sidecar_conflicts WHERE photo_id = ?1 AND grp = ?2",
                params![id, g.key()],
            )
            .map_err(|e| format!("resolve conflict: {e}"))?;
        Ok(!self.has_sidecar_conflict(id))
    }

    /// The sidecar changed since Laika last wrote or merged it.
    pub fn sidecar_changed_externally(&self, id: i64, photo_path: &str) -> bool {
        match (
            crate::xmp::sidecar_mtime(photo_path),
            self.sidecar_written_at(id),
        ) {
            (Some(m), Some(w)) => m > w + 1,
            _ => false,
        }
    }

    fn remember_mtime(&self, id: i64, mtime: i64) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT INTO sidecar_state(photo_id, written_mtime) VALUES (?1,?2)
                 ON CONFLICT(photo_id) DO UPDATE SET written_mtime = ?2",
                params![id, mtime.to_string()],
            )
            .map(|_| ())
            .map_err(|e| format!("save sidecar state: {e}"))
    }

    /// Photos whose sidecar may have changed since Laika last saw it:
    /// (id, photo path, last seen mtime). Stat the paths off the UI thread
    /// with [`changed_since`].
    pub fn sidecar_watch_list(&self) -> Vec<(i64, String, Option<i64>)> {
        let seen: BTreeMap<i64, i64> = {
            let mut stmt = match self
                .conn
                .prepare("SELECT photo_id, written_mtime FROM sidecar_state")
            {
                Ok(s) => s,
                Err(_) => return Vec::new(),
            };
            stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))
                .map(|rows| {
                    rows.flatten()
                        .filter_map(|(id, m)| m.parse::<i64>().ok().map(|m| (id, m)))
                        .collect()
                })
                .unwrap_or_default()
        };
        self.all_photos()
            .into_iter()
            .filter(|p| !crate::apple_photos::in_library(&p.path))
            .map(|p| (p.id, p.path.clone(), seen.get(&p.id).copied()))
            .collect()
    }
}

/// Of a watch list, the photos whose sidecar is newer than last seen (or
/// newly appeared). Pure file stats — safe on a background thread.
pub fn changed_since(list: &[(i64, String, Option<i64>)]) -> Vec<(i64, String)> {
    list.iter()
        .filter_map(|(id, path, seen)| {
            let m = crate::xmp::sidecar_mtime(path)?;
            match seen {
                Some(s) if m <= s + 1 => None,
                _ => Some((*id, path.clone())),
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sidecar::{Item, parse_doc};

    fn jpeg(path: &std::path::Path, shade: u8) {
        image::RgbImage::from_pixel(24, 16, image::Rgb([shade, 90, 140]))
            .save(path)
            .unwrap();
    }

    /// Stand-in for Lightroom Classic writing a sidecar: edits attribute
    /// values and keywords in place, appends a history event, bumps
    /// MetadataDate — and keeps everything it doesn't touch.
    fn lightroom_writes(photo: &str, rating: u8, exposure: &str, keyword: &str, when: &str) {
        let path = &crate::xmp::sidecar_path(photo);
        let raw = std::fs::read_to_string(path).unwrap();
        let mut doc = parse_doc(raw.as_bytes()).unwrap();
        let set = |doc: &mut crate::sidecar::Doc, key: &str, val: &str| {
            if let Some(i) = doc
                .items
                .iter_mut()
                .find(|i| matches!(i, Item::Attr { key: k, .. } if k == key))
            {
                *i = Item::Attr {
                    key: key.into(),
                    raw: val.into(),
                };
            } else {
                doc.items.push(Item::Attr {
                    key: key.into(),
                    raw: val.into(),
                });
            }
        };
        set(&mut doc, "xmp:Rating", &rating.to_string());
        set(&mut doc, "crs:Exposure2012", exposure);
        set(&mut doc, "xmp:MetadataDate", when);
        doc.items.retain(|i| !matches!(i, Item::Elem { key, .. } if key == "dc:subject" || key == "lr:hierarchicalSubject"));
        doc.items.push(Item::Elem {
            key: "dc:subject".into(),
            raw: format!("<dc:subject><rdf:Bag><rdf:li>{keyword}</rdf:li></rdf:Bag></dc:subject>"),
        });
        let event = format!(
            "<rdf:li stEvt:action=\"saved\" stEvt:when=\"{when}\" stEvt:softwareAgent=\"Adobe Photoshop Lightroom Classic 13.0 (Macintosh)\"/>"
        );
        for i in doc.items.iter_mut() {
            if let Item::Elem { key, raw } = i {
                if key == "xmpMM:History" {
                    *raw = raw.replace("</rdf:Seq>", &format!("{event}</rdf:Seq>"));
                }
            }
        }
        let mut out = String::from(
            "<x:xmpmeta xmlns:x=\"adobe:ns:meta/\" x:xmptk=\"Adobe XMP Core 7.0\"><rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\"><rdf:Description rdf:about=\"\"",
        );
        for (p, u) in &doc.decls {
            if p != "x" && p != "rdf" {
                out.push_str(&format!(" xmlns:{p}=\"{u}\""));
            }
        }
        for i in &doc.items {
            if let Item::Attr { key, raw } = i {
                out.push_str(&format!(" {key}=\"{raw}\""));
            }
        }
        out.push('>');
        for i in &doc.items {
            if let Item::Elem { raw, .. } = i {
                out.push_str(raw);
            }
        }
        out.push_str("</rdf:Description></rdf:RDF></x:xmpmeta>");
        std::fs::write(path, out).unwrap();
        // Coarse mtimes: make sure the change is visible.
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
        let f = std::fs::File::options().write(true).open(path).unwrap();
        f.set_modified(later).unwrap();
    }

    /// Laika writes its catalog values for a photo (like the app does).
    fn laika_writes(cat: &Catalog, id: i64, path: &str, policy: LightroomPolicy) {
        let p = cat.photo_by_id(id).unwrap();
        let auth = cat.photo_authorship(id);
        crate::xmp::write_scoped(
            path,
            &cat.params_of(id),
            p.rating,
            &[],
            None,
            &auth,
            &cat.geom_of(id),
            crate::sidecar::WriteScope {
                develop: policy.writes_develop(),
            },
        )
        .unwrap();
        cat.remember_sidecar_write(id, crate::xmp::sidecar_mtime(path).unwrap())
            .unwrap();
    }

    const FOREIGN: [&str; 3] = ["crs:ToneCurvePV2012Red", "crs:Look", "xmpMM:History"];

    fn seed(dir: &std::path::Path, n: usize) -> (Catalog, Vec<(i64, String)>) {
        let cat = Catalog::open(&dir.join("c.db"), "c", dir).unwrap();
        let mut photos = Vec::new();
        for i in 0..n {
            let path = dir.join(format!("p{i}.jpg"));
            jpeg(&path, (i % 250) as u8);
            let id = cat.import_file(&path, &dir.join("cache")).unwrap().unwrap();
            let path = path.to_string_lossy().into_owned();
            // Lightroom made the sidecar first, with things Laika can't show.
            std::fs::write(
                crate::xmp::sidecar_path(&path),
                crate::sidecar::tests::ADOBE,
            )
            .unwrap();
            // First contact adopts it whole and records the baseline.
            cat.apply_sidecar(id, &path).unwrap();
            photos.push((id, path));
        }
        (cat, photos)
    }

    fn foreign_elements(path: &str) -> Vec<String> {
        let doc = parse_doc(&std::fs::read(crate::xmp::sidecar_path(path)).unwrap()).unwrap();
        FOREIGN
            .iter()
            .filter_map(|k| {
                doc.items.iter().find_map(|i| match i {
                    Item::Elem { key, raw } if key == k => Some(raw.clone()),
                    _ => None,
                })
            })
            .collect()
    }

    #[test]
    fn two_hundred_photos_ten_rounds_lose_nothing() {
        let dir = std::env::temp_dir().join(format!("laika-coexist-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        let (cat, photos) = seed(&dir, 200);
        cat.set_lightroom_policy(LightroomPolicy::LaikaLeads);
        let look_before: Vec<Vec<String>> =
            photos.iter().map(|(_, p)| foreign_elements(p)).collect();
        let exposure = crate::xmp::PARAM_KEYS
            .iter()
            .find(|(k, _)| *k == "Exposure2012")
            .unwrap()
            .1;
        for round in 0..10 {
            // Lightroom edits every photo: rating, exposure, a keyword.
            for (k, (_, path)) in photos.iter().enumerate() {
                let when = format!("2026-09-{:02}T10:{:02}:00Z", 10 + round, k % 60);
                lightroom_writes(
                    path,
                    (round % 5) as u8 + 1,
                    &format!("+{}.{:02}", round % 3, k % 100),
                    &format!("lr{round}"),
                    &when,
                );
            }
            for (id, path) in &photos {
                let r = cat
                    .merge_external_sidecar(*id, path, cat.lightroom_policy())
                    .unwrap()
                    .unwrap();
                assert!(r.conflicts.is_empty());
                let f = cat.catalog_fields(*id).unwrap();
                assert_eq!(
                    f.rating,
                    (round % 5) as u8 + 1,
                    "Lightroom's rating arrived"
                );
                assert_eq!(f.keywords, vec![format!("lr{round}")]);
            }
            // Laika edits: title and exposure (different fields than Lightroom's rating).
            for (k, (id, path)) in photos.iter().enumerate() {
                let mut meta = cat.photo_meta(*id);
                meta.title = format!("Round {round} photo {k}");
                cat.set_photo_meta(*id, &meta).unwrap();
                let mut params = cat.params_of(*id);
                params[exposure] = -1.0 - round as f32 / 10.;
                cat.save_params(
                    *id,
                    &crate::edit::Edit {
                        params,
                        geom: cat.geom_of(*id),
                        ..crate::edit::Edit::default()
                    },
                )
                .unwrap();
                laika_writes(&cat, *id, path, LightroomPolicy::LaikaLeads);
                let side = crate::xmp::read(path).unwrap();
                assert_eq!(
                    side.title,
                    format!("Round {round} photo {k}"),
                    "Laika's title in the file"
                );
                assert!((side.params[exposure] - params[exposure]).abs() < 1e-3);
                assert_eq!(
                    side.keywords,
                    vec![format!("lr{round}")],
                    "Lightroom's keyword kept"
                );
            }
        }
        // Everything Laika can't represent survived ten rounds unchanged
        // (the history only grew).
        for ((_, path), before) in photos.iter().zip(look_before.iter()) {
            let after = foreign_elements(path);
            assert_eq!(after[0], before[0], "RGB curve byte-for-byte");
            assert_eq!(after[1], before[1], "profile look byte-for-byte");
            assert!(
                after[2].len() > before[2].len()
                    && after[2].starts_with(&before[2][..before[2].len() - 30])
            );
            let doc = parse_doc(&std::fs::read(crate::xmp::sidecar_path(path)).unwrap()).unwrap();
            assert!(doc.items.iter().any(|i| matches!(i, Item::Attr { key, raw } if key == "other:Tag" && raw == "keep &amp; me")));
            assert!(doc.items.iter().any(|i| matches!(i, Item::Attr { key, raw } if key == "crs:LensProfileEnable" && raw == "1")));
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn policies_decide_real_conflicts() {
        let dir = std::env::temp_dir().join(format!("laika-conflict-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        let (cat, photos) = seed(&dir, 3);
        let (a, pa) = &photos[0];
        let (b, pb) = &photos[1];
        let (c, pc) = &photos[2];
        // Both apps change the rating (and Lightroom the exposure).
        for (id, _) in &photos {
            cat.set_rating(*id, 1).unwrap();
        }
        for (_, p) in &photos {
            lightroom_writes(p, 5, "+2.00", "Lisbon", "2026-09-20T10:00:00Z");
        }

        // Laika leads: Laika's rating stays and goes back out.
        let r = cat
            .merge_external_sidecar(*a, pa, LightroomPolicy::LaikaLeads)
            .unwrap()
            .unwrap();
        assert_eq!(r.kept, vec![SideGroup::Rating]);
        assert!(
            r.adopted.contains(&SideGroup::Develop),
            "only Lightroom touched develop"
        );
        assert_eq!(cat.photo_by_id(*a).unwrap().rating, 1);

        // Lightroom leads: Lightroom's rating wins.
        let r = cat
            .merge_external_sidecar(*b, pb, LightroomPolicy::LightroomLeads)
            .unwrap()
            .unwrap();
        assert!(r.adopted.contains(&SideGroup::Rating));
        assert_eq!(cat.photo_by_id(*b).unwrap().rating, 5);

        // Ask: recorded, nothing changed, resolvable per field.
        let r = cat
            .merge_external_sidecar(*c, pc, LightroomPolicy::Ask)
            .unwrap()
            .unwrap();
        assert_eq!(r.conflicts, vec![SideGroup::Rating]);
        assert_eq!(cat.photo_by_id(*c).unwrap().rating, 1);
        assert!(cat.has_sidecar_conflict(*c));
        let list = cat.sidecar_conflicts();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].writer, "Lightroom Classic 13.0");
        assert_eq!(list[0].ours.describe(SideGroup::Rating, None), "★");
        assert_eq!(list[0].theirs.describe(SideGroup::Rating, None), "★★★★★");
        assert!(
            cat.resolve_sidecar_conflict(*c, SideGroup::Rating, true)
                .unwrap()
        );
        assert_eq!(cat.photo_by_id(*c).unwrap().rating, 5);
        assert_eq!(cat.sidecar_conflict_count(), 0);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn lightroom_leads_keeps_laika_develop_edits_out_of_the_file() {
        let dir = std::env::temp_dir().join(format!("laika-lrleads-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        let (cat, photos) = seed(&dir, 1);
        let (id, path) = &photos[0];
        let before: Vec<Item> = parse_doc(&std::fs::read(crate::xmp::sidecar_path(path)).unwrap())
            .unwrap()
            .items
            .into_iter()
            .filter(|i| matches!(i, Item::Attr { key, .. } | Item::Elem { key, .. } if key.starts_with("crs:")))
            .collect();
        let mut params = cat.params_of(*id);
        params[2] = -3.0;
        cat.save_params(
            *id,
            &crate::edit::Edit {
                params,
                ..crate::edit::Edit::default()
            },
        )
        .unwrap();
        cat.set_rating(*id, 4).unwrap();
        laika_writes(&cat, *id, path, LightroomPolicy::LightroomLeads);
        let after: Vec<Item> = parse_doc(&std::fs::read(crate::xmp::sidecar_path(path)).unwrap())
            .unwrap()
            .items
            .into_iter()
            .filter(|i| matches!(i, Item::Attr { key, .. } | Item::Elem { key, .. } if key.starts_with("crs:")))
            .collect();
        assert_eq!(before, after, "no develop field written");
        assert_eq!(
            crate::xmp::read(path).unwrap().rating,
            Some(4),
            "metadata still written"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
