# Catalog format

Laika catalogs are ordinary SQLite 3 files. No application-defined collation,
extension, encryption, trigger, or virtual table is required. The database uses
SQLite's normal rollback/WAL behavior; copy a live catalog through SQLite's
backup API or while Laika is closed.

## Schema revisions

`schema_version` contains exactly one integer row. Version 0 is the unstamped
prototype shape. Migrations run in increasing order and are additive.

| Version | Additions |
|---:|---|
| 0 | Unstamped core: `catalogs`, `photos`, `keywords`, `edits` |
| 1 | Complete baseline tables listed below; descriptive/photo compatibility columns |
| 2 | Primary photo fields and indexes for capture time, hash, rating, flags, sync, path, and keywords |
| 3 | `export_presets` |
| 4 | Descriptive/EXIF photo columns; metadata-preset title/caption/headline/location; `keyword_nodes`, `keyword_synonyms`, `keyword_sets` |
| 5 | `photos_links`, `photos_include` for Apple Photos |
| 6 | `photos_links.origin`; `photos_containers`, `photos_album_items` |
| 7 | `photos.label`; `collections`, `collection_items` |
| 8 | Album order/caption on `collection_items`; cover/title/description on `collections` |
| 9 | `galleries`, `gallery_photos` |
| 10 | `source_fingerprints` |
| 11 | `lightroom_links` |
| 12 | `develop_presets` |
| 13 | `sidecar_baseline`, `sidecar_conflicts` |
| 14 | Smart-collection criteria; `photo_stacks`, `photo_stack_items` |
| 15 | `keyword_suggestions` with persistent accept/reject decisions |

The current writer is version 15. A reader interested only in photos and
develop settings needs only `photos`, `edits`, and the rules below; all other
tables can be ignored safely.

## Core tables (version 1)

### `catalogs`

`id INTEGER PRIMARY KEY`, `name TEXT NOT NULL`, `root_path TEXT NOT NULL`.
Photo rows refer to `catalogs.id` through `catalog_id`.

### `photos`

| Columns | Meaning |
|---|---|
| `id`, `catalog_id` | Stable SQLite IDs |
| `path`, `filename` | Absolute original path and display name; `path` is unique |
| `blake3` | Lowercase BLAKE3 digest of the original |
| `captured_at`, `captured_orig`, `capture_offset_min` | Display capture time, original capture time, and applied minute offset |
| `camera`, `lens`, `focal_mm`, `aperture`, `shutter`, `iso` | Imported EXIF display strings |
| `width`, `height` | Native pixel dimensions |
| `rating` | Integer 0–5 |
| `picked`, `rejected` | Boolean integers; normally mutually exclusive |
| `label` | 0 none, 1 red, 2 yellow, 3 green, 4 blue, 5 purple |
| `sync_state`, `remote_key` | Backup state and provider object key |
| `imported_at` | Unix-seconds text |
| `creator`, `copyright`, `rights`, `contact` | Authorship text |
| `title`, `caption`, `headline`, `location` | Descriptive metadata |
| `exif_program`, `exif_metering`, `exif_flash`, `exif_focal35`, `exif_serial`, `exif_firmware`, `exif_gps` | Read-only imported EXIF overflow |
| `duration_ms`, `codec` | Video metadata; zero/empty for stills |

Later photo columns have empty/zero defaults, so a projection should use
`COALESCE` when it needs to read prototype catalogs.

### `keywords`

`photo_id INTEGER NOT NULL`, `keyword TEXT NOT NULL`. There is one row per
assigned path. Hierarchy uses ` > ` between levels.

### `edits`

`photo_id INTEGER PRIMARY KEY`, `params_json TEXT`, `history_json TEXT`,
`cursor INTEGER`, `updated_at TEXT` (Unix seconds).

Current `params_json` is this JSON object:

```json
{
  "params": [70 numbers],
  "crop": null,
  "geom": {
    "rect": [0.0, 0.0, 1.0, 1.0],
    "angle": 0.0,
    "flip_h": false,
    "flip_v": false,
    "rotation": 0,
    "upright": {
      "mode": 0,
      "auto": [0.0, 0.0, 0.0],
      "guides": [[0.0, 0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 0.0], [0.0, 0.0, 0.0, 0.0]],
      "n_guides": 0,
      "constrain": true
    }
  },
  "curve_on": true,
  "hsl_on": true,
  "detail_on": true,
  "optics_on": true,
  "effects_on": true,
  "grading_on": true
}
```

`crop` is the old aspect-ratio hint and does not affect pixels. `rect` is
normalized `[x,y,width,height]` in post-quarter-turn display space. `angle` is
clockwise degrees in −45…45. `rotation` is clockwise quarter turns modulo 4.
Upright `mode` is 0 off, 1 auto, 2 level, 3 vertical, 4 full, 5 guided;
`auto` is solved vertical, horizontal, rotate; guides are normalized display
coordinates `[x0,y0,x1,y1]` and only the first `n_guides` (maximum four) count.

Legacy `params_json` is a bare JSON number array. Recognized widths are 12,
46, 49, 63, and 70; missing trailing values take current defaults.

`history_json` is an array, oldest first. Each item has `label`, `value`,
`params`, `crop`, `geom`, the six `*_on` flags, and non-render fields `rating`,
`picked`, `rejected`, `color_label`. Missing fields take the same defaults as
the current object. `cursor` is clamped to 0…array length. If cursor is nonzero,
geometry and panel flags come from item `cursor - 1`; otherwise they come from
`params_json`. `params_json.params` is always the live pixel array.

### Other version-1 tables

| Table | Columns / purpose |
|---|---|
| `sync_queue` | `photo_id, kind, state, attempts, error`; pending backup work |
| `sync_settings` | string `key PRIMARY KEY, value` |
| `sidecar_state` | `photo_id PRIMARY KEY, written_mtime` Unix-seconds text |
| `card_history` | `(file_hash, volume)` primary key, `imported_at` |
| `import_batches` | import journal: source, volume, mode, timestamps and counts |
| `name_templates` | `(kind,name)` primary key, `value`, `last_used` |
| `metadata_presets` | name plus authorship, descriptive fields, keywords, `last_used` |
| `import_defaults` | string `key PRIMARY KEY, value` |
| `filter_presets` | `name PRIMARY KEY, filters_json, last_used` |
| `snapshots` | `id, photo_id, name, state_json, created_at`; state JSON has the history-item shape |
| `watched_folders` | `path PRIMARY KEY, added_at` |

## Added tables

| Version | Table | Columns / purpose |
|---:|---|---|
| 3 | `export_presets` | `name PRIMARY KEY, folder, body` (JSON body) |
| 4 | `keyword_nodes` | `path PRIMARY KEY, include` |
| 4 | `keyword_synonyms` | `(path,synonym)` primary key |
| 4 | `keyword_sets` | `name PRIMARY KEY, paths` |
| 5 | `photos_links` | `photo_id PRIMARY KEY, item_id, meta_hash, album, synced_at`; v6 adds `origin` |
| 5 | `photos_include` | `photo_id PRIMARY KEY` |
| 6 | `photos_containers` | `id PRIMARY KEY, parent, name, kind, position` |
| 6 | `photos_album_items` | `album_id, item_id, position` |
| 7 | `collections` | `id PRIMARY KEY, name, kind, created_at`; v8 adds `cover_photo_id,title,description` |
| 7 | `collection_items` | `(collection_id,photo_id)` primary key, `added_at`; v8 adds `position,caption` |
| 9 | `galleries` | gallery identity/status, layout, theme/sizes JSON, download/GPS policy, build/deploy fields |
| 9 | `gallery_photos` | `(gallery_id,photo_id)` primary key, position/grid span/caption/alt/focal/fit/open fields |
| 10 | `source_fingerprints` | `(source,rel_path)` primary key, size, mtime, hash, capture/camera |
| 11 | `lightroom_links` | `(library,kind,lr_id)` primary key, `laika_id,linked_at` |
| 12 | `develop_presets` | `id,name,grp,values_json,supports_amount,skipped_json,approx_json,source,digest,created_at`; unique `(grp,name)` |
| 13 | `sidecar_baseline` | `photo_id PRIMARY KEY, fields_json, updated_at` |
| 13 | `sidecar_conflicts` | `(photo_id,grp)` primary key, ours/theirs JSON, writer, detected_at |
| 14 | `photo_stacks` | `id PRIMARY KEY, collapsed, created_at` |
| 14 | `photo_stack_items` | `(stack_id,photo_id)` primary key, unique photo membership and position |
| 15 | `keyword_suggestions` | `(photo_id,keyword)` primary key, score, source, state (`pending`, `accepted`, or `rejected`) |

SQLite foreign-key clauses are intentionally absent in versions 0–15. IDs
still have the relationships named above; integrity checks report orphans.
