//! G01: gallery persistence (tables from migration v9).

use rusqlite::{OptionalExtension, params};

use super::{Catalog, chrono_stamp};
use crate::gallery::layout::{self, Cell};
use crate::gallery::{Fit, Gallery, GalleryPhoto, Status, Theme, validate_slug};

/// One row in the gallery list.
#[derive(Clone, Debug, PartialEq)]
pub struct GallerySummary {
    pub id: i64,
    pub title: String,
    pub slug: String,
    pub status: Status,
    pub photo_count: usize,
    /// First photo in tray order (the list thumbnail).
    pub cover_photo_id: Option<i64>,
    pub updated_at: String,
    pub last_deploy_url: String,
}

const GALLERY_COLS: &str = "id, title, subtitle, eyebrow, slug, status, template_id, columns, gutter, ratio,
     theme_json, sizes_json, allow_downloads, strip_gps, site_name, meta_line, output_dir,
     deploy_project, created_at, updated_at, last_build_at, last_build_dir, last_deploy_at,
     last_deploy_url";

impl Catalog {
    /// A new, empty draft (template "mixed"). Titles need not be unique.
    pub fn create_gallery(&self, title: &str) -> Result<i64, String> {
        let now = chrono_stamp();
        let g = Gallery {
            title: title.trim().to_string(),
            ..Gallery::default()
        };
        self.conn
            .execute(
                "INSERT INTO galleries(title, template_id, columns, gutter, ratio, theme_json,
                   sizes_json, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
                params![
                    g.title,
                    g.template,
                    g.columns,
                    g.gutter,
                    g.ratio,
                    serde_json::to_string(&g.theme).unwrap_or_default(),
                    serde_json::to_string(&g.sizes).unwrap_or_default(),
                    now
                ],
            )
            .map_err(|e| format!("create gallery: {e}"))?;
        Ok(self.conn.last_insert_rowid())
    }

    /// A gallery seeded from a collection/album: its order, title,
    /// description (as subtitle) and album captions are copied — later
    /// album edits don't flow into the gallery.
    pub fn create_gallery_from_collection(&self, collection: i64) -> Result<i64, String> {
        let c = self
            .collections()
            .into_iter()
            .find(|c| c.id == collection)
            .ok_or_else(|| "that collection no longer exists".to_string())?;
        let id = self.create_gallery(&c.display_title())?;
        let mut g = self.load_gallery(id)?;
        g.subtitle = c.description.clone();
        let order = self.album_order(collection);
        g.add_photos(&order);
        let captions = self.album_captions(collection);
        for p in &mut g.photos {
            if let Some(cap) = captions.get(&p.photo_id) {
                p.caption = cap.clone();
            }
        }
        let tpl = g.template.clone();
        g.apply_template(&tpl);
        self.save_gallery(&g)?;
        Ok(id)
    }

    /// Summaries, most recently edited first.
    pub fn galleries(&self) -> Vec<GallerySummary> {
        let mut stmt = match self.conn.prepare(
            "SELECT g.id, g.title, g.slug, g.status, g.updated_at, g.last_deploy_url,
                    (SELECT count(*) FROM gallery_photos gp JOIN photos p ON p.id = gp.photo_id
                      WHERE gp.gallery_id = g.id),
                    (SELECT gp.photo_id FROM gallery_photos gp JOIN photos p ON p.id = gp.photo_id
                      WHERE gp.gallery_id = g.id ORDER BY gp.position LIMIT 1)
               FROM galleries g
              ORDER BY CAST(g.updated_at AS INTEGER) DESC, g.id DESC",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        stmt.query_map([], |r| {
            Ok(GallerySummary {
                id: r.get(0)?,
                title: r.get(1)?,
                slug: r.get(2)?,
                status: Status::parse(&r.get::<_, String>(3)?),
                updated_at: r.get(4)?,
                last_deploy_url: r.get(5)?,
                photo_count: r.get::<_, i64>(6)? as usize,
                cover_photo_id: r.get(7)?,
            })
        })
        .map(|rows| rows.flatten().collect())
        .unwrap_or_default()
    }

    pub fn load_gallery(&self, id: i64) -> Result<Gallery, String> {
        let mut g = self
            .conn
            .query_row(
                &format!("SELECT {GALLERY_COLS} FROM galleries WHERE id = ?1"),
                [id],
                |r| {
                    let theme: String = r.get(10)?;
                    let sizes: String = r.get(11)?;
                    Ok(Gallery {
                        id: r.get(0)?,
                        title: r.get(1)?,
                        subtitle: r.get(2)?,
                        eyebrow: r.get(3)?,
                        slug: r.get(4)?,
                        status: Status::parse(&r.get::<_, String>(5)?),
                        template: r.get(6)?,
                        columns: r.get::<_, i64>(7)?.clamp(1, 6) as u8,
                        gutter: r.get::<_, i64>(8)?.clamp(0, 64) as u8,
                        ratio: r.get::<_, f64>(9)? as f32,
                        theme: serde_json::from_str::<Theme>(&theme).unwrap_or_default(),
                        sizes: serde_json::from_str(&sizes)
                            .unwrap_or_else(|_| vec![640, 1280, 2048]),
                        allow_downloads: r.get::<_, i64>(12)? != 0,
                        strip_gps: r.get::<_, i64>(13)? != 0,
                        site_name: r.get(14)?,
                        meta_line: r.get(15)?,
                        output_dir: r.get(16)?,
                        deploy_project: r.get(17)?,
                        created_at: r.get(18)?,
                        updated_at: r.get(19)?,
                        last_build_at: r.get(20)?,
                        last_build_dir: r.get(21)?,
                        last_deploy_at: r.get(22)?,
                        last_deploy_url: r.get(23)?,
                        photos: Vec::new(),
                    })
                },
            )
            .optional()
            .map_err(|e| format!("load gallery: {e}"))?
            .ok_or_else(|| "that gallery no longer exists".to_string())?;
        // Photos removed from the catalog drop out (the JOIN).
        let mut stmt = self
            .conn
            .prepare(
                "SELECT gp.photo_id, gp.col, gp.row, gp.span_x, gp.span_y, gp.caption, gp.alt_text,
                        gp.focal_x, gp.focal_y, gp.fit, gp.open_full
                   FROM gallery_photos gp JOIN photos p ON p.id = gp.photo_id
                  WHERE gp.gallery_id = ?1
                  ORDER BY gp.position, gp.photo_id",
            )
            .map_err(|e| format!("load gallery: {e}"))?;
        let rows = stmt
            .query_map([id], |r| {
                let col: Option<i64> = r.get(1)?;
                let row: Option<i64> = r.get(2)?;
                Ok(GalleryPhoto {
                    photo_id: r.get(0)?,
                    cell: col.zip(row).map(|(c, w)| Cell {
                        col: c.max(0) as u16,
                        row: w.max(0) as u16,
                    }),
                    span_x: r.get::<_, i64>(3)?.clamp(1, 6) as u8,
                    span_y: r.get::<_, i64>(4)?.clamp(1, 8) as u8,
                    caption: r.get(5)?,
                    alt_text: r.get(6)?,
                    focal: (
                        (r.get::<_, f64>(7)? as f32).clamp(0., 1.),
                        (r.get::<_, f64>(8)? as f32).clamp(0., 1.),
                    ),
                    fit: if r.get::<_, String>(9)? == "fit" {
                        Fit::Fit
                    } else {
                        Fit::Fill
                    },
                    open_full_size: r.get::<_, i64>(10)? != 0,
                })
            })
            .map_err(|e| format!("load gallery: {e}"))?;
        g.photos = rows.flatten().collect();
        repair_layout(&mut g);
        Ok(g)
    }

    /// Who owns a slug (other than `except`).
    pub fn slug_owner(&self, slug: &str, except: i64) -> Option<(i64, String)> {
        self.conn
            .query_row(
                "SELECT id, title FROM galleries WHERE slug = ?1 AND id <> ?2",
                params![slug, except],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .ok()
    }

    /// Persist the whole gallery in one transaction; stamps `updated_at`.
    /// An empty slug is allowed on drafts; a non-empty one must be valid
    /// and unused.
    pub fn save_gallery(&self, g: &Gallery) -> Result<String, String> {
        if !g.slug.is_empty() {
            validate_slug(&g.slug)?;
            if let Some((_, title)) = self.slug_owner(&g.slug, g.id) {
                let who = if title.is_empty() { "another gallery".to_string() } else { title };
                return Err(format!("{who} already uses the address {}", g.slug));
            }
        }
        if g.title.chars().count() > 200 {
            return Err("keep gallery titles under 200 characters".to_string());
        }
        let now = chrono_stamp();
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|e| format!("save gallery: {e}"))?;
        let n = tx
            .execute(
                "UPDATE galleries SET title=?1, subtitle=?2, eyebrow=?3, slug=?4, status=?5,
                   template_id=?6, columns=?7, gutter=?8, ratio=?9, theme_json=?10, sizes_json=?11,
                   allow_downloads=?12, strip_gps=?13, site_name=?14, meta_line=?15, output_dir=?16,
                   deploy_project=?17, updated_at=?18, last_build_at=?19, last_build_dir=?20,
                   last_deploy_at=?21, last_deploy_url=?22
                 WHERE id = ?23",
                params![
                    g.title,
                    g.subtitle,
                    g.eyebrow,
                    g.slug,
                    g.status.key(),
                    g.template,
                    g.columns,
                    g.gutter,
                    g.ratio,
                    serde_json::to_string(&g.theme).unwrap_or_default(),
                    serde_json::to_string(&g.sizes).unwrap_or_default(),
                    g.allow_downloads as i64,
                    g.strip_gps as i64,
                    g.site_name,
                    g.meta_line,
                    g.output_dir,
                    g.deploy_project,
                    now,
                    g.last_build_at,
                    g.last_build_dir,
                    g.last_deploy_at,
                    g.last_deploy_url,
                    g.id
                ],
            )
            .map_err(|e| format!("save gallery: {e}"))?;
        if n == 0 {
            return Err("that gallery no longer exists".to_string());
        }
        tx.execute("DELETE FROM gallery_photos WHERE gallery_id = ?1", [g.id])
            .map_err(|e| format!("save gallery: {e}"))?;
        {
            let mut ins = tx
                .prepare(
                    "INSERT OR IGNORE INTO gallery_photos(gallery_id, photo_id, position, col, row,
                       span_x, span_y, caption, alt_text, focal_x, focal_y, fit, open_full)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                )
                .map_err(|e| format!("save gallery: {e}"))?;
            for (i, p) in g.photos.iter().enumerate() {
                ins.execute(params![
                    g.id,
                    p.photo_id,
                    i as i64,
                    p.cell.map(|c| c.col as i64),
                    p.cell.map(|c| c.row as i64),
                    p.span_x,
                    p.span_y,
                    p.caption,
                    p.alt_text,
                    p.focal.0,
                    p.focal.1,
                    if p.fit == Fit::Fit { "fit" } else { "fill" },
                    p.open_full_size as i64
                ])
                .map_err(|e| format!("save gallery: {e}"))?;
            }
        }
        tx.commit().map_err(|e| format!("save gallery: {e}"))?;
        Ok(now)
    }

    /// Delete a gallery (photos stay; built folders on disk stay).
    pub fn delete_gallery(&self, id: i64) -> Result<(), String> {
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|e| format!("delete gallery: {e}"))?;
        tx.execute("DELETE FROM gallery_photos WHERE gallery_id = ?1", [id])
            .and_then(|_| tx.execute("DELETE FROM galleries WHERE id = ?1", [id]))
            .map_err(|e| format!("delete gallery: {e}"))?;
        tx.commit().map_err(|e| format!("delete gallery: {e}"))
    }

    /// Copy a gallery as a new draft (no slug, no build/deploy history).
    pub fn duplicate_gallery(&self, id: i64) -> Result<i64, String> {
        let src = self.load_gallery(id)?;
        let new_id = self.create_gallery(&format!("{} copy", src.title).trim().to_string())?;
        let copy = Gallery {
            id: new_id,
            title: format!("{} copy", src.title).trim().to_string(),
            slug: String::new(),
            status: Status::Draft,
            created_at: String::new(),
            updated_at: String::new(),
            last_build_at: String::new(),
            last_build_dir: String::new(),
            last_deploy_at: String::new(),
            last_deploy_url: String::new(),
            ..src
        };
        self.save_gallery(&copy)?;
        Ok(new_id)
    }
}

/// Stored layouts can go stale (a photo left the catalog, an older build
/// wrote overlapping cells): re-flow placed photos in reading order when
/// anything overlaps or overflows, so the editor never shows a broken grid.
fn repair_layout(g: &mut Gallery) {
    let cols = g.columns.max(1);
    let mut seen = std::collections::HashSet::new();
    let mut broken = false;
    for p in g.photos.iter().filter(|p| p.cell.is_some()) {
        let c = p.cell.unwrap();
        if c.col as u32 + p.span_x as u32 > cols as u32 {
            broken = true;
            break;
        }
        for dy in 0..p.span_y as u16 {
            for dx in 0..p.span_x as u16 {
                if !seen.insert((c.col + dx, c.row + dy)) {
                    broken = true;
                }
            }
        }
    }
    if !broken {
        return;
    }
    let cells: Vec<Option<Cell>> = g.photos.iter().map(|p| p.cell).collect();
    let order = layout::reading_order(&cells);
    let slots: Vec<layout::Slot> = order
        .iter()
        .map(|&k| layout::Slot {
            span_x: g.photos[k].span_x.min(cols),
            span_y: g.photos[k].span_y,
            cell: None,
        })
        .collect();
    let flowed = layout::flow(&slots, cols);
    for (k, c) in order.into_iter().zip(flowed) {
        g.photos[k].span_x = g.photos[k].span_x.min(cols);
        g.photos[k].cell = Some(c);
    }
}
