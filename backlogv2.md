# Laika backlog v2 — basic catalog, storage, views, tools, and delivery features

Updated: 2026-09-15. All items below are open.

## Scope of this document

`backlog.md` (U01–U27) covers trust, persistence, selection, culling, zoom, the
first crop tool, Basic editing, JPEG export, Compare/Survey, search, collections,
history, batch editing, presets, missing files, the first advanced panels, local
adjustments, color management, performance, accessibility, backup, gallery
deployment, and Lightroom migration.

This backlog adds the **everyday Lightroom Classic features that U01–U27 do not
describe**: memory-card ingest, organized on-disk storage, catalog lifecycle and
portability, metadata basics, alternative Library views (dense thumbnails, a
chrome-free wall, a timeline, a table), a deeper crop and geometry tool, the
Effects/Color Grading/Calibration panels, multi-format export and watermarking,
gallery themes, and app preferences. Where a story touches a U-item it says so
and defines only the new surface.

Numbering is V01–V32. Format, priorities, and release gates follow `backlog.md`.
This is a product backlog grounded in the current source and `plan.md`, not a
claim of live-app verification.

## Current baseline relevant to this backlog

| Area | Implemented foundation | Gap this backlog addresses |
| --- | --- | --- |
| Ingest | Folder picker, recursive scan, blake3, EXIF, 512/2048 previews, idempotent by path | No card/device awareness, verified copy, second copy, rename or folder templates, metadata or develop presets on import, RAW+JPEG/video handling |
| Storage | Originals stay where they were; cache under the app support dir | No catalog-managed destination layout; no folder synchronize; no DNG option |
| Catalog | One SQLite file (`catalogs`, `photos`, `keywords`, `edits`, `sync_queue`, `sync_settings`), WAL | No schema version/migrations, single implicit catalog, no open/switch/recent, no catalog export/import/merge, absolute paths |
| Metadata | Ratings, pick/reject, flat keywords, EXIF read | No IPTC editing, metadata presets, color labels, capture-time correction, rotation, hierarchical keywords |
| Views | Grid (3–12 columns), Loupe, fixed cell overlay, rails at fixed widths | No compact/expanded cells, no flush chrome-free wall, no timeline, no table view, no slideshow, no second window |
| Develop | Twelve Basic sliders, aspect-only crop, seven presets | No Dehaze, Effects (vignette/grain), Color Grading, B&W mix, Calibration, perspective/Upright, Auto Tone, external editor |
| Export | Full-resolution render to RGBA8; `laika-export` placeholder | No AVIF/WebP/PNG/JPEG XL/TIFF encoders, per-format options, watermark editor, export presets, post-export actions |
| Galleries | One askama masonry template planned; other two templates render masonry | No theme system, theme preview/options, responsive multi-format images, album ordering/cover/captions |
| App | No preferences window, no diagnostics, no first-run flow | Preferences, logging/About/onboarding |

Source anchors: `crates/laika-core/src/catalog.rs`, `crates/laika-core/src/edit.rs`,
`crates/laika-core/src/xmp.rs`, `crates/laika-raw/`, `crates/laika-develop/`,
`crates/laika-export/src/lib.rs`, `crates/laika-app/src/main.rs`, and
`crates/laika-app/src/controls/`.

## Priorities and delivery order

- **P0 — Basics a photographer expects on day one:** card ingest, organized
  storage, a catalog that can be opened/moved/upgraded, real export formats,
  dense and chrome-free browsing.
- **P1 — Everyday depth:** metadata, labels, timeline, table view, deeper crop,
  Effects and Color Grading, watermarks and export presets, gallery themes.
- **P2 — Extended workflow:** catalog merge, smart previews, DNG, external
  editor, second display, diagnostics.

| Milestone | Exit condition | Items |
| --- | --- | --- |
| A. Ingest and store a shoot properly | Card-to-disk copy is verified and organized; imported files carry the photographer's metadata | V01–V05 |
| B. Own the catalog | Catalogs can be created, opened, moved, upgraded, and checked without data loss | V07–V09 |
| C. Deliver in the right format | Any selection exports to JPEG/PNG/WebP/AVIF/JPEG XL/TIFF with correct color, metadata, and watermark | V27–V28 |
| D. Browse the way you want | Thumbnail wall, timeline, table, slideshow, and info overlays are all usable and remembered | V16–V20 |
| E. Finish the image | Perspective-aware crop, Dehaze, vignette, grain, color grading, B&W, calibration | V22–V25 |
| F. Organize and annotate | IPTC, presets, labels, quick collection, capture time, rotation, albums | V12–V15, V30 |
| G. Publish with style | Multiple gallery themes with preview, options, and responsive output | V29 |
| H. Extend | Catalog export/import/merge, duplicates, DNG, external editor, second window, preferences, diagnostics | V06, V10, V11, V21, V26, V31, V32 |

Milestones A–C are P0. D–G are P1. H is P2 except V31 (P1).

## P0 — Basics a photographer expects on day one

### [x] V01 — Import from memory cards and cameras with verified copy

**Result: done 2026-09-15.** Same verification as U05 (shared engine).
- Mounted volumes detected (`/Volumes` minus boot device on macOS,
  `/media` + `/run/media` on Linux) with label, free/total capacity via
  statvfs, photo count, and capture date range in the source picker.
- Card scan prefers `DCIM`, skips `MISC`/dot-dirs; plain folders scan
  fully. PTP/MTP devices appear only where the platform mounts them as
  folders — raw PTP needs a camera library and stays out of scope.
- Every copy hashes source bytes during streaming and re-hashes the
  written file: mismatch retries once, then fails loudly per file.
  Placement is atomic (`.part` + rename); pulled cards leave no partials
  and no row without a verified file (insert happens only after prepare).
- Optional second destination verified independently in the same pass;
  its failure is recorded without failing the primary.
- `card_history` remembers hashes per volume id; New-photos-only (default
  on for cards) plus Don't-import-duplicates (default on) make reinserted
  cards import only new frames. `import_batches` journals each run.
- Eject/unmount on completion when enabled (diskutil/umount/udisksctl),
  off-thread and never fatal. Footer and dialog show throughput, ETA, and
  the current file throughout.

- **Problem:** U05 adds a “Copy to folder” mode but does not define how cards are
  found, how copies are verified, or what happens when a card is pulled mid-copy.
- **Deliver:** Detect mounted removable volumes and PTP/MTP devices where the
  platform exposes them as folders; list them as sources with card name, capacity,
  photo count, and date range. Scan `DCIM` and vendor layouts (`100NIKON`,
  `PRIVATE/M4ROOT`, `MISC` skipped). Copy each file to the destination, hash the
  source and the written copy with blake3, and only then insert into the catalog.
  Offer a **second copy** destination (backup drive) written in the same pass.
  Remember imported hashes per card serial/volume ID so “Don't import suspected
  duplicates” is on by default and “New photos only” skips everything seen before.
  Eject/unmount on completion when the option is enabled. Show throughput,
  remaining time, and per-file status.
- **Done when:** Pulling the card mid-copy leaves no half-written file in the
  destination and no catalog row without a verified file. Reinserting the same card
  imports only new frames. A hash mismatch is reported per file and the file is
  retried, not silently skipped. Source files on the card are never modified or
  deleted. Two-destination imports verify both copies independently.
- **Depends on:** U05. **Touchpoints:** Import dialog, `laika-core` import job,
  new `import_sources` and `card_history` tables.

### [x] V02 — Destination folder structure and file renaming templates

**Result: done 2026-09-15.** `cargo fmt`, full workspace tests (66 total:
11 new template/relocate tests plus updated import tests) and
`cargo check --workspace --all-targets` pass, no new warnings.
- New `laika-core/src/template.rs`: token parser (`{yyyy} {yy} {mm} {dd}
  {date} {camera} {shoot} {catalog} {batch} {original} {seq[:width]}`,
  `{{`/`}}` escapes), date parts from capture time with mtime fallback,
  per-file sequence with start number and padding, sanitization that keeps
  Unicode/spaces and replaces only filesystem-invalid characters, empty
  results and unknown tokens as explained errors.
- Copy imports resolve destination subfolders + filenames per file
  (sequence follows scan order); the second copy mirrors the layout; the
  review shows the exact first-three placed paths, sanitized, before
  anything is written. Presets ship built-in, save by name, and the last
  used is the default (plus a persistent Custom row); "Destination only"
  and "Original names" rows cover the flat cases.
- Rename-later (toolbar button + `F2` + dialog over the visible-scope
  targets): same engine, same preview, sidecar follows the original with
  rollback, catalog/previews (hash-keyed)/queue (id-keyed) stay valid, and
  both files re-enqueue so backup converges to the new keys — old remote
  keys are orphaned, never deleted.
- Gates: 10,000 rendered paths stay unique (test); collisions suffix and
  report in both flows (tests); Unicode camera/shoot text survives
  (test); an upsert bug that wrote preset names into values was caught by
  the new tests and fixed.
- Deliberately later: metadata presets (V03), folder sync/moves (V04).

- **Deliver:** A token-based **folder template** for copy imports
  (`{yyyy}/{yyyy}-{mm}-{dd}`, `{yyyy}/{mm}/{shoot}`, `{camera}/{yyyy}-{mm}-{dd}`,
  custom) and a **rename template** (`{date}_{seq:4}_{original}`, `{shoot}-{seq}`,
  `{camera}{seq}`) with a live preview of the first three resulting paths. Tokens:
  date parts from capture time, original name, sequence with padding and start
  number, camera model, custom text, catalog name, import batch. Templates are
  saved, named, and reusable; the last used one is the default. Rename also works
  later on selected catalog photos and renames the sidecar with the original.
- **Done when:** Ten thousand files import into dated folders with no collisions;
  a collision appends a suffix and reports it. Renaming keeps the catalog, sidecar,
  previews, and sync queue consistent. Invalid characters for the target file
  system are replaced and shown before import starts. Unicode camera names and
  paths survive on macOS and Linux.
- **Depends on:** U04, V01. **Touchpoints:** Template engine in `laika-core`,
  Import dialog, Library rename action.

### [x] V03 — Apply metadata and develop presets during import

**Result: done 2026-09-15.** `cargo fmt`, workspace tests (58 in the
affected crates: 6 new validator/roundtrip/adopt/shift tests) and
`cargo check --workspace --all-targets` pass, no new warnings.
- Review dialog gains a collapsible Metadata section: preset picker with
  save-as, inline creator/copyright/rights/contact/keywords fields (U04
  editors with validation), develop preset picker over the built-ins, and
  a decimal-hours capture offset. Preset selections are remembered per
  run; picking never discards typed input.
- Storage: authorship columns on photos (with an idempotent
  prototype-era upgrade), `metadata_presets`, `import_defaults`, and
  keyword replace/set helpers. Sidecars carry `dc:`/`xmpRights:`/
  `laika:Contact` flat attributes (bags/structs stay V12 work) with
  unknown preservation intact; heals and param edits ride authorship
  along so rewrites never wipe it.
- Application order per file: offset (catalog time only, EXIF untouched,
  original kept for V14) → additive metadata/keywords → develop preset
  params unless an external sidecar was adopted → sidecar written at
  import. The preset lands as Import-baseline + Preset history steps,
  undoable in session. Panel shows creator/copyright/rights/contact when
  set and corrected time alongside the original.
- Scoped honestly to open dependencies: built-in develop presets stand in
  for U16-managed ones, flat keywords for V12 hierarchy, import-only
  metadata editing (no library editor yet).

- **Deliver:** Import options to apply a **metadata preset** (creator, copyright,
  rights usage, contact, keywords), a **develop preset** (from U16), a keyword
  list, and a **capture-time offset** for cameras with a wrong clock or time zone.
  Presets are chosen per import and remembered. Values are written to the catalog
  and sidecar at import time, not baked into the original.
- **Done when:** Every file in an import shows the creator and copyright in the
  metadata panel and in exported files' XMP. The develop preset appears as the
  first history step and is undoable. The time offset shifts capture time for
  sorting and folder templates but leaves EXIF in the original untouched.
- **Depends on:** U16, V01, V12. **Touchpoints:** Import dialog, `xmp.rs`, edit
  history.

### [x] V04 — Folder synchronize, watched folders, and on-disk folder operations

**Result: done 2026-09-15.** `cargo fmt`, workspace tests (84 total: 5
new folder/sync/watch tests) and `cargo check --workspace --all-targets`
pass, no new warnings.
- Folder tree acts: create (validated names), same-parent rename moving
  the directory plus every descendant row in one transaction (LIKE-safe,
  char-counted prefix rewrite), empty-only delete, and a scoped action
  panel (Synchronize, Watch toggle, Rename, New subfolder, Delete) with
  offline counts on rows.
- Synchronize Folder diffs disk vs catalog (new/missing/externally
  changed sidecars) with a review dialog and per-category toggles; new
  files reuse the headless import engine, missing rows drop
  (files already gone), sidecars re-adopt; non-empty folder delete forces
  the explicit Remove-from-catalog vs Move-to-Trash choice.
- Watched folders persist in the DB and poll every 5s: unknown folders
  baseline silently, only size/mtime-stable files older than the settle
  window import, catalogued paths never re-enter, runs defer while an
  import is active. Polling, not inotify — no new dependency (registry
  was unreachable) and safe against partial writes by construction.
- Drag photos between folders: threshold-disambiguated drag from grid
  cells, live drop highlight via measured row bounds, Esc cancels,
  same-dir drops no-op, cross-volume moves reuse the verified copier.
- Gates: Finder-added files sync without duplicates; non-empty delete
  refuses; folder rename updates all descendants atomically with
  sidecars riding along; polled imports skip growing files (tested).
- Honest limits: no background drive watcher UI beyond Recheck (drives
  surface as offline badges + Missing filter), no PTP beyond mounted
  folders, Linux Trash/Reveal need `gio`/`xdg-open`.

- **Problem:** U17 covers relinking and catalog-managed move/rename of photos.
  Folders themselves cannot be created, renamed, watched, or reconciled with disk.
- **Deliver:** Create, rename, and delete-empty folders from the folder tree,
  changing the file system and the catalog together. **Synchronize Folder**
  compares disk with the catalog and offers to import new files, remove missing
  entries, and re-read changed sidecars, with a review list before applying.
  Optional **watched folders** import new files automatically (for tethered
  capture apps and phone sync folders). Drag photos between folders to move them.
- **Done when:** Adding a file with Finder and synchronizing imports it without
  duplicating existing photos. Removing a folder with photos requires an explicit
  Remove from Catalog or Move to Trash choice. Watched folders never import a file
  still being written. Renaming a folder updates every descendant path in one
  transaction.
- **Depends on:** U17, V01. **Touchpoints:** Folder rail, `catalog.rs`, file watcher.

### [x] V05 — RAW+JPEG pairs, video, and non-photo files

**Result: done 2026-09-15, except in-Loupe playback (see below).**
`cargo fmt`, workspace tests (76 in affected crates: container parser,
pairing, video import, overrides) and
`cargo check --workspace --all-targets` pass, no new warnings.
- Pairs are a view over one-per-file rows (sync, sidecars, history stay
  per file): same folder + stem, RAW + raster, videos excluded. Grouped
  cells show an `R+J` badge; the toggle (Loupe bar + rail) switches the
  displayed side and its primary, so Develop and ratings follow. Ratings,
  flags, and import keywords apply to both sides; toggling the persisted
  preference re-groups instantly with no reimport.
- Videos import with a pure-std MP4/MOV parse (duration, dimensions,
  codec fourcc, capture time with mtime fallback) plus best-effort
  posters (QuickLook/`ffmpeg`, 20 s timeout, `LAIKA_NO_POSTER` escape
  hatch). No-poster videos import fine and tile the placeholder with a
  `VIDEO m:ss` badge. Loupe shows duration/codec/dims with Play and an
  explicit Develop exclusion (guarded with a reason); export copies the
  original per U10's future dialog.
- Unsupported strays are counted with samples at scan, shown in review,
  and journaled — sidecars/dotfiles excluded, shoots never abort.
- Gates: one cell per capture; pair rating/flags hit both rows; videos
  sort by capture time; corrupt containers import as empty-metadata rows.
- **Deferred with reason: in-Loupe playback.** No decoder exists in the
  dependency tree (and the registry is unreachable); Play shells to the
  system player instead. A `symphonia`/`mp4` + software-decode frame
  pipeline is the honest future fix.

- **Deliver:** Treat `NEF+JPG` (same stem, same folder) as one photo with the RAW
  primary by default and a preference to import JPEGs as separate photos. Show a
  pair badge and allow switching the displayed file. Import video files (MOV, MP4)
  with a poster frame, duration, and codec metadata; play them in Loupe; exclude
  them from Develop and gallery builds unless the theme supports video. Skip and
  report unsupported or non-image files without aborting the import.
- **Done when:** A card with RAW+JPEG pairs produces one grid cell per capture;
  ratings and keywords apply to the pair. Toggling the preference re-groups without
  reimport. Videos sort by capture time and export as copies of the original.
- **Depends on:** U05, V01. **Touchpoints:** Import scanner, photo model, Loupe.

### [x] V07 — Catalog lifecycle: create, open, switch, recent, and relocate

**Result: done 2026-09-15, except an About window (V32).** `cargo fmt`,
workspace tests (80 in affected crates: relative-path, bundle-move,
lock, recents tests) and `cargo check --workspace --all-targets` pass,
no new warnings.
- Relative paths: rows under the catalog root store relatively (absolute
  otherwise); reads always resolve, so every path consumer works
  unchanged. Inserts, relink, move, and rename all store portable form;
  folder rename rewrites both stored forms in one transaction.
- Moved bundles reopen fully online: stored roots that resolve nothing
  yield to the db directory when it resolves, with edits and history
  intact (tested). Rail shows the name; the Manage modal shows path/size.
- Advisory `<db>.lock` (pid/host): second opens refuse with holder
  details instead of risking corruption; stale same-host locks are taken
  over; the guard drops on switch and exit. A locked-out launch stays
  usable-but-empty with an explanation, never a half-open catalog.
- Manage modal hub: New (folder + name, duplicateDetected), Open
  (validated), Recent (existing-first, missing marked), launch preference
  (last/ask/fixed + fixed picker), plus the existing backup/rebuild/
  recheck/integrity tools. Switches flush saves first and refuse
  mid-sync; restore reuses the same adopt path (rescan + settings +
  queue reload included).
- Gates: moved-folder reopen shows every photo online; second instance
  never corrupts; switches flush first.
- Honest limits: no About window yet (V32: path/size live in Manage
  instead); no catalog rename/duplicate/delete actions (not requested).

- **Problem:** The catalog is a single implicit file; users cannot pick a
  location, keep separate catalogs, or move one between machines.
- **Deliver:** New Catalog with a chosen location and name, Open Catalog, Recent
  Catalogs, and a preference for which catalog loads at launch (last used, a fixed
  one, or ask). Store photo paths **relative to the catalog root** when the photos
  live under it and absolute otherwise, so a catalog folder moved with its photos
  still resolves. Hold an advisory lock file so a second instance opens read-only
  with an explanation. Show the catalog path and size in About and in the left
  rail header.
- **Done when:** Moving a catalog folder with its photos to another disk and
  opening it shows every photo online. Opening a catalog already open elsewhere
  never corrupts it. Switching catalogs flushes pending saves (U02) first.
- **Depends on:** U02. **Touchpoints:** `catalog.rs`, app startup, left rail.

### [x] V08 — Schema versioning, migrations, integrity check, and optimize

**Result: done 2026-09-16.** `cargo fmt`, `cargo test --workspace` (114 tests,
including 5 new V08 pins: mid-era upgrade, rollback naming, newer-schema
refusal, damage report, optimize) and `cargo check --workspace --all-targets`
pass, no new warnings.
- `schema_version` + ordered migrations wired into open: version gate
  first (garbage refused via magic bytes, newer schemas refuse naming
  the required version + this Laika's), automatic pre-migration backup
  beside the db (skipped for fresh files), each step in its own
  transaction, version stamped after. v1 = baseline tables + authorship
  columns; v2 = primary columns + capture/hash/rating/flag/sync/path/
  keyword indexes.
- Mid-era catalogs upgrade with ratings, flags, sync state, keywords,
  and edits intact (NULL-tolerant row mapping — legacy NULLs surface as
  relinkable rows instead of silently vanishing); failed steps name
  themselves and roll back with the backup kept.
- `PRAGMA foreign_keys=ON` set with an honest note (schema predates FK
  clauses; orphan scans enforce instead). Integrity report adds orphan
  previews to the existing page + row checks; damage names the latest
  backup with a Restore pointer. Optimize (VACUUM + ANALYZE +
  checkpoint, journal restored) runs off the UI thread on its own
  connection with busy retry messaging.
- Honest limits: no automatic lens-style FK rebuilds (rightly out of
  scope); Manage-modal pointer flow code-reviewed only.

- **Deliver:** A `schema_version` table and an ordered migration list; each
  migration runs in one transaction after an automatic backup copy of the catalog.
  Refuse to open a newer schema with a clear message naming the required Laika
  version. Add the missing indexes (capture time, folder prefix, hash, rating,
  flags, sync state) and `PRAGMA foreign_keys`. Provide **Check Integrity**
  (`PRAGMA integrity_check`, orphaned edits/keywords, previews without photos) and
  **Optimize Catalog** (`VACUUM`, `ANALYZE`, `wal_checkpoint`) with progress.
- **Done when:** A prototype-era catalog upgrades without losing ratings, flags,
  edits, or sync state. A failed migration leaves the pre-migration file intact and
  reports the step that failed. Integrity check on a deliberately damaged copy
  reports the damage and offers the last backup.
- **Depends on:** V07. **Touchpoints:** `catalog.rs`, migrations module, Catalog menu.

### [x] V09 — Preview and cache management, including smart previews

**Result: done 2026-09-16.** `cargo fmt`, `cargo test --workspace` (117 tests:
bridge cache, audit eviction/TTL/pinning, 1:1 key match) and
`cargo check --workspace --all-targets` pass, no new warnings.
- Smart previews (the cached 2048 linear, RAW via decode cache, rasters
  via the sRGB bridge which now shares that cache): built on demand for
  the selection (online stills, sequential, cancellable); offline
  originals with one edit fully in Develop (params persist, sidecars
  shelve-and-retry) under a SMART badge with an edited-from-smart state;
  reconnecting re-decodes the primary from the original with identical
  params. Without a smart file the refusal names both ways back.
- 1:1 previews: full developed JPEG + key (tone/geometry/split/dims)
  built on demand; Loupe 100% serves fresh keys without any decode
  (offline included) — any drift falls back to the GPU path, never
  stale pixels. TTL + over-cap eviction (expired then oldest).
- Cache home in Manage: usage line (background audit), relocatable
  folder, cap + TTL steppers (persisted app-level for V31 to adopt),
  smart/1:1 builders with progress + cancel, orphan prune. Eviction
  never touches 512/2048 or offline smart files; the cap governs the
  evictable class (sequential writes exceed it by at most one file).
- Export at smart-preview resolution with a disclosed report line when
  the original is offline (missing smart file fails with the fix).
- Honest limits: pointer flows code-reviewed only — verify in-app;
  no per-photo smart freshness UI beyond the badge/note.

- **Problem:** U21 bounds caches and U17 offers a cache rebuild. Users cannot
  choose which previews exist or edit while originals are offline.
- **Deliver:** Build Standard (2048) and 1:1 previews for a selection on demand;
  discard 1:1 previews after a configurable period; set cache location and size
  cap in preferences with current usage shown. Add **smart previews**: a
  lossy-compressed, demosaiced, reduced-size linear image (DNG-like or Laika's own
  container) that Develop can edit when the original is offline, with a badge and
  an “edited from smart preview” state that re-renders from the original when it
  returns.
- **Done when:** Disconnecting the drive still allows slider edits and export at
  smart-preview resolution with a warning. Reconnecting produces full-resolution
  renders with identical parameters. Cache never exceeds its cap by more than one
  in-flight file; eviction never removes a smart preview for an offline original.
- **Depends on:** U17, U21. **Touchpoints:** Preview cache, `laika-raw`, Develop.

### [x] V16 — Thumbnail cell styles, info overlays, and badges

**Result: done 2026-09-16.** `cargo fmt`, `cargo test --workspace` (118 tests,
including overlay/badge persistence pins) and `cargo check --workspace --all-targets`
pass, no new warnings.
- Compact (image-only + corner flag dot) and expanded (overlay + badges)
  styles; overlay cycle `J` (none / filename+capture / camera+exposure,
  empties dropped, never dangling separators); nine per-badge toggles
  (flag, rating, label, crop, edit, keywords, pair, video, sync).
- Badges read live state (crop box/geometry, tone divergence, keyword
  rows, pick/reject, sync dot); offline stays unconditional (safety,
  not a badge). Label toggle persists dormant until V13 provides data.
- 3–20 columns (slider remapped, persisted); past 10 columns badges
  collapse to flag + rating by rule so rows never overflow. Hover always
  reveals the full filename; overlay keeps the mono type tokens on the
  existing dark bar for light-thumbnail legibility.
- Style, overlay, badges, and columns persist per catalog and reload on
  switch/launch.
- Honest limits: Loupe label untouched (grid scope); pointer flow
  code-reviewed only — verify in-app.

- **Deliver:** Compact and expanded cell styles; an overlay cycle (`J`) showing
  none, filename and capture time, or camera/exposure; per-cell badges for
  flag, rating, label, crop, edit, keyword, pair, video, and sync state that can
  be individually turned off. Thumbnail size and cell style are remembered per
  catalog. Hover shows the full filename without truncation.
- **Done when:** Cells never overflow their row at any size from 3 to 20 columns.
  Badges are legible at the smallest size or hidden by rule, not clipped. Overlay
  text uses the type tokens from the design handoff and remains readable on light
  thumbnails.
- **Depends on:** U03. **Touchpoints:** Grid cell, `AppState.thumb_size`, theme.

### [x] V17 — Flush thumbnail wall with no chrome

**Result: done 2026-09-16.** `cargo fmt`, `cargo test --workspace` (119 tests,
including wall-layout/navigation pins) and `cargo check --workspace --all-targets`
pass, no new warnings.
- Wall view (`W` toggles, returns to last view; segmented control too):
  justified rows from pure layout math (tested: exact-width rows, equal
  heights, left-aligned tail, 2000-item cap with an explicit note),
  zero gaps, no padding/borders/overlays/badges. Selection is an inset
  accent ring; hover reveals filenames via the shared tip; clicks, folder
  drags, ratings, flags, and auto-advance all ride the existing transitions.
- Arrows follow the visual layout (left/right walk order, up/down keep x
  across rows, shift extends); leaving the wall keeps primary/selection.
- `Shift+Tab` hides rails/toolbar/filmstrip/status, `F` fullscreens,
  `L` cycles normal/dim (filmstrip+status off)/out (all chrome off +
  pure black). Works in every view, not just the wall.
- Honest limits: pointer/chrome flow code-reviewed only — verify in-app
  (including the 200-photo zero-gap gate and resize reflow); wall renders
  without virtualization (cap documented); grid scroll position restores
  best-effort via the shared list handle.

- **Deliver:** A **wall** mode that removes cell padding, borders, overlays, and
  badges, and lays thumbnails out edge to edge in **justified rows** (each row
  scaled so aspect-preserved thumbnails fill the width exactly, last row
  left-aligned) or a fixed square grid with center crop. `Shift+Tab` hides all
  rails, toolbar, filmstrip, and status bar; `F` enters full screen; `L` cycles
  lights dim/out. Selection is shown by a thin inset ring, keyboard navigation
  follows the visual row layout, and rating/flag keys still work. Hover reveals
  a minimal overlay only when the pointer is still.
- **Done when:** A 200-photo shoot fills a 1440 × 900 window with zero gaps and no
  visible UI; resizing re-flows without a reload. Mixed portrait and landscape
  rows are equal height. All culling shortcuts from U06 work identically in wall
  mode. Leaving wall mode restores the previous layout and scroll position.
- **Depends on:** U03, U06, U22. **Touchpoints:** Grid layout, app shell visibility
  state, keyboard map.

### [x] V27 — Export to AVIF, WebP, PNG, JPEG, JPEG XL, TIFF, and Original

**Result: done 2026-09-16, with stated encoder-boundary cuts.** `cargo fmt`,
`cargo test --workspace` (123 tests: PNG-lossless round-trip, JPEG
size-cap convergence, container magics, AVIF ftyp smoke) and
`cargo check --workspace --all-targets` pass, no new warnings. Live Metal
render → JPEG/PNG/WebP/TIFF verified (exiftool: correct type+dims, zero
embedded EXIF/GPS/XMP; WebP eyeballed sane); that run caught and fixed a
real RGBA→JPEG encode bug.
- Format chooser in the Export dialog (settings persist): JPEG (quality
  + MB cap via quality search), PNG (Fast/Default/Best, 8-bit), lossless
  WebP, AVIF (quality, speed 1–10 with per-effort timing logged, 8/10-bit),
  uncompressed 8-bit TIFF, Original (source copy + sidecar). Videos always
  copy originals. Per-format options show only for their format.
- Same pipeline truth (tone/crop/mirrors, acknowledged values), resize
  without upscale, atomic writes, naming/collision/sidecar policies
  unchanged and per-format; failures report per file and continue;
  sequential background loop (encoders parallelize internally — rav1e
  threads — keeping memory and cancel predictable).
- Honest cuts, all stated in UI + `formats.rs` module docs: sRGB only
  (P3/Adobe/ProPhoto need vendored ICC blobs + container writers);
  WebP lossless-only (no libwebp offline); PNG/TIFF 8-bit (pipeline is
  RGBA8); TIFF uncompressed; JPEG quality-only; JPEG XL dimmed (jxl-oxide
  decodes only, jpegxl-rs needs libjxl + network); no embedded EXIF —
  authorship rides the sidecar; 16-bit TIFF / lossless-JXL round-trips
  unverifiable here (lossless PNG stands in).

- **Problem:** U10 defines the export dialog and JPEG output; U20 mentions TIFF.
  No other format is specified.
- **Deliver:** A format chooser with per-format options:
  - **JPEG:** quality, chroma subsampling (4:2:0/4:4:4), progressive, target file
    size limit.
  - **PNG:** 8/16-bit, compression level.
  - **WebP:** lossy quality or lossless, effort.
  - **AVIF:** quality, speed/effort, 8/10-bit, 4:2:0/4:4:4.
  - **JPEG XL:** distance/quality or lossless, effort, 8/16-bit float.
  - **TIFF:** 8/16-bit, none/LZW/ZIP compression.
  - **Original:** copy the original file plus sidecar, no rendering.
  Every rendered format embeds the chosen ICC profile (sRGB, Display P3, Adobe
  RGB, ProPhoto RGB) and the selected EXIF/XMP/IPTC subset in its native container
  (APP1 for JPEG, `iCCP`/`eXIf`/`iTXt` for PNG, `ICCP`/`EXIF`/`XMP` chunks for
  WebP, `colr`/`Exif`/`mime` boxes for AVIF, `icc`/`exif`/`xml` boxes for JXL,
  TIFF tags). Use `image`, `ravif`/`rav1e`, `jpegxl-rs`, and `webp` crates;
  document build requirements per platform. Encode on the background pool with
  bounded parallelism.
- **Done when:** A 16-bit TIFF and a lossless JXL round-trip pixel-identical
  through the pipeline. AVIF and WebP output open in Safari, Chrome, Firefox,
  Preview, and GIMP with correct color and orientation. Metadata policy (all,
  copyright only, none, strip GPS) is honored in every format and verified by
  `exiftool` in tests. File-size-limited JPEGs land under the limit. An encoder
  failure for one format reports and continues.
- **Depends on:** U10, U20. **Touchpoints:** `laika-export`, Export dialog, tests
  with fixture images.

### [x] V28 — Export presets, watermarks, and post-export actions

**Result: done 2026-09-16.** `cargo fmt`, `cargo test --workspace`
(131 tests) and `cargo check --workspace --all-targets` pass, no new
warnings. Watermark output verified live on Metal renders (text +
graphic placement, exiftool-clean); dialog/preset orchestration is
code-reviewed only (no headless GUI harness) — manually verify the
preset rows, multi-run button, and post-action lines in the app.
- Watermarks (`laika-export::watermark`, resvg, pure Rust): text
  (copy, font, size % of long edge, white/black, opacity, shadow) or
  graphic (PNG/JPEG/SVG), 3×3 anchor, px margin, proportional scaling;
  stamped post-resize. Unit gates: anchor symmetry, opacity/clipping,
  font-free SVG rasterization, validate/apply, channel-order regression
  caught by test. Missing graphic / empty text blocks the run before
  anything writes; Original copies and videos refuse/explain stamping.
- Presets: `export_presets` table (migration v3 — presets travel with
  the catalog), folder groups, save (upsert)/apply/delete, full dialog
  snapshot as JSON (dest, naming, quality, long edge, meta, collision,
  format + opts, watermark, post-action, script). Corrupt bodies name
  themselves and never block other presets.
- Multi-run: run-set toggles, `Export N presets` button, per-preset
  numbering restarts (three presets → three output sets), failures
  preset-qualified in one progress list, retries replay each preset's
  settings. Post-actions per preset frozen at start: Reveal, Open
  (system handler, first 10 — stated cap), Script (executable-bit
  checked pre-run, exit status appended). Export with Previous
  (Shift+E) replays the dialog's values on the current selection.
- Deferred: watermark preview on the current photo — the export dialog
  has no pixel-preview surface (naming preview only); verify
  watermarks on the output files instead.

- **Deliver:** Named **export presets** grouped in folders (Web 2048 AVIF, Client
  full JPEG, Print 16-bit TIFF, …) that capture every dialog value including the
  rename template from V02 and the folder template. Run **several presets at
  once** from one selection. A **watermark editor**: text (font, size, opacity,
  color, shadow) or graphic (PNG/SVG), anchor, margins, proportional scaling,
  and per-preset choice; preview on the current photo. **Post-export actions:**
  reveal, open in an application, or run a chosen script with the file list.
  **Export with Previous** repeats the last export without the dialog.
- **Done when:** Selecting 50 photos and running three presets produces three
  correctly named output sets with one progress list. Watermark placement is
  identical on portrait and landscape frames and scales with output size. A
  missing watermark graphic blocks the export with a clear message before any
  file is written.
- **Depends on:** U10, V02, V27. **Touchpoints:** Export dialog, preset storage,
  `laika-export`.

## P1 — Everyday depth

### [x] V12 — Editable metadata panel, metadata presets, and hierarchical keywords

**Result: done 2026-09-16.** `cargo fmt`, `cargo test --workspace`
(139 tests) and `cargo check --workspace --all-targets` pass, no new
warnings. Core verified by execution (XMP structures, hierarchy ops,
JPEG embed read back by exiftool on a live render); rail/manager UI is
code-reviewed only — manually verify fields, `<mixed>`, chips, and the
manager in the running app.
- Right rail: editable Title/Caption (2000 chars)/Headline/Creator/
  Copyright/Rights/Contact/Location over the scope (targets else
  primary), `<mixed>` on disagreement, batch empty commits never clear
  (stated per tip). Keyword union chips click-to-remove (excluded dimmed
  with ⊘), add box unions, manager entry. Rail preset chips + save box
  (`<mixed>` saves blank, stated). EXIF section shows non-empty
  program/metering/flash/35mm/serial/firmware/GPS.
- Keyword manager (K): 300-row capped tree with depth indent, include
  toggles, select; rename/merge-into/delete with undo over affected
  assignments; synonyms add/remove; sets save-from-scope/apply/delete;
  text import/export with loud errors and a byte-identical round-trip.
- Batch engine: catalog writes on the UI thread, sidecars in the
  background with pre-collected inputs (acknowledged edits mirrored,
  never defaults-over-data), one undo step per action; Cmd+Z/Shift+Cmd+Z
  route to the metadata stack when last (edit sites clear the flag).
  Structural ops (rename/merge/delete/include) snapshot and refresh the
  same way; pure-text ops skip undo honestly.
- Sidecars: standard Alt/Bag/Seq (`dc:`, `photoshop:Headline`,
  `Iptc4xmpCore:Location`, `xmpRights:`, `lr:hierarchicalSubject`);
  legacy flat sidecars still parse and upgrade on rewrite; full paths
  win over leaves. JPEG exports embed the packet (caption+copyright
  verified via exiftool); other formats keep the no-embed rule.
- Deferred: multi-line captions — FieldEdit is single-line by design
  (2000-char one line); full EXIF beyond the seven overflow fields
  (maker notes are opaque to kamadak-exif).

- **Deliver:** Edit title, caption, headline, creator, copyright, rights usage
  terms, contact, location fields, and user-defined keywords in the right rail
  for one photo or a batch (mixed values shown as `<mixed>`). Save and apply
  **metadata presets**. Show full read-only EXIF (exposure program, metering,
  flash, focal length 35 mm equivalent, lens ID, serial, firmware, GPS). Support
  **hierarchical keywords** (`Places > Portugal > Lisbon`), synonyms, export
  inclusion flags, keyword sets, and import/export of keyword lists as text.
  Write `dc:`, `photoshop:`, `xmpRights:`, `Iptc4xmpCore:` and `lr:hierarchicalSubject`
  to the sidecar; preserve existing fields.
- **Done when:** Batch-applying a copyright to 500 photos completes in the
  background with one undo step. Lightroom and darktable read the resulting
  sidecar keywords and captions. Keyword hierarchy survives rename and merge.
  Exported JPEGs carry the caption and copyright.
- **Depends on:** U02, U04, U13. **Touchpoints:** Right rail, `xmp.rs`, keywords
  schema.

### [x] V13 — Color labels, quick collection, and target collection

**Result: done 2026-09-17.** `cargo fmt` and `cargo test --workspace`
pass (laika-core 164, including the new label and collection tests), no
new warnings. Verified live in the isolated e2e app (release): the v6→v7
migration with its automatic backup; keys 6 and 8 labeling; B adding to
the Quick Collection; the red label chip filtering 9,427 photos to 1; the
Quick Collection filter; Save as… creating a collection and clearing the
Quick Collection; everything intact after a relaunch.
- Schema v7: `photos.label` (0 none, 1–5 Red/Yellow/Green/Blue/Purple)
  and `collections` + `collection_items` (the Quick Collection is the one
  `kind='quick'` row, created on demand). Removing a photo drops its
  memberships; restore-after-remove keeps the label.
- Labels: 6–9 set red/yellow/green/blue and pressing again clears
  (Lightroom's toggle); the context menu sets any of the five or clears;
  pairs get the label together; each change is one undo step (labels
  ride `Snap`, history and named snapshots). Slideshow keys 6–9 label
  the slide on screen.
- Names: editable per catalog in Settings → Color labels (distinct,
  blank restores the default). Sidecars write the name as `xmp:Label`
  (Lightroom stores and reads the name). Reading maps this catalog's
  names first, then Lightroom's stock names; unknown text such as
  "Select" is carried untouched and never overwrites a label. Renaming
  rewrites the sidecars of photos with that label. Photos inside an Apple
  Photos library get no sidecar (unchanged rule).
- Filters: label chips (no label + five colors, multi-select) combine
  with rating, flag and every other filter and clear with Clear; collection
  scope sits beside folder and album scope. Saved filter presets from before
  V13 still load.
- Collections (left rail): Quick Collection plus saved collections with
  counts; ○/● chooses the target collection that B and the context menu's
  "Add to …" use (persisted per catalog, falls back to Quick). Viewing a
  collection offers Save as… and Clear (Quick) or Rename…, Remove
  selected and Delete (saved); New creates a collection from the
  selection. Names commit on Enter only.
- Badges: label swatch in expanded cells (every column count), beside the
  flag dot in compact cells, and on filmstrip thumbnails; the Loupe label
  line and slideshow toast name the label. The V16 badge toggle now
  controls it.
- Also fixed: at launch the V22 overlay/aspect presets never loaded
  (only a catalog switch loaded them); they now load with the cell prefs.
- Limits: B / Add to / Remove from collection aren't undo steps yet
  (they apply immediately; B again reverses); Lightroom reading the
  label was verified against the sidecar text, not by opening Lightroom;
  context-menu label/collection items were code-reviewed only (the test
  harness can't open context menus).


- **Deliver:** Five color labels (`6`–`9` and a fifth via menu), label filter
  chips, editable label names per catalog, and a label badge in cells and
  filmstrip. A **Quick Collection** toggled with `B`, a **target collection**
  chooser so `B` adds to any collection, and Save Quick Collection as a
  collection. Labels are written to `xmp:Label` for interoperability.
- **Done when:** Labels survive restart and appear in Lightroom after sidecar
  read. Quick Collection holds photos across folders and is cleared explicitly.
  Filtering by label combines with rating and flag filters from U12.
- **Depends on:** U03, U12, U13. **Touchpoints:** Keyboard map, filters, XMP.

### [ ] V14 — Capture time correction and time zone shift

- **Deliver:** Edit Capture Time for a selection: set to a specific time, shift by
  hours/minutes/seconds, or adjust by the difference between the primary photo's
  current and corrected time. Optionally write the corrected value to the sidecar
  only, or (for JPEG/DNG) into the original after an explicit warning with a
  backup. Show original capture time alongside corrected time in metadata.
- **Done when:** Two cameras with a 1 h 02 m offset sort correctly after one
  shift on each camera's photos. Undo restores the original time. Folder and
  rename templates use the corrected time.
- **Depends on:** U04, U14. **Touchpoints:** Metadata panel, sort, templates.

### [x] V15 — Rotate, flip in Library, and Delete Rejected Photos

**Result: done 2026-09-16.** `cargo fmt`, `cargo test --workspace`
(144 tests) and `cargo check --workspace --all-targets` pass, no new
warnings. Rotation verified live on Metal (pure + composed
rotate/crop/tilt renders, dims swap exact); Library keys, flips, and
the delete flows are code-reviewed only — manually verify shortcuts,
confirm buttons, and undo in the running app.
- Orientation in the edit model: `CropGeom.rotation` (quarter turns CW)
  with rect-transmuting `rotated_cw/ccw` (flips conjugated, straighten
  untouched), EXIF 1–8 mapping both ways, `display_dims`, and
  rotation-aware `crop_sample`/`constrain_crop` (source bounds checked).
  Unit gates: 4-spin return, CW/CCW identity, all-eight round-trip,
  in-frame rotated corners.
- Renderer: `CropRender.rotation` rides the `o.z` uniform slot;
  `crop_target` swaps axes on odd turns; WGSL unrotates after
  flip/crop/derotate (aspect-correct); `DetailKey` carries rotation so
  stale orientations never show. CPU formula mirrors the shader.
- Sidecar: always-explicit `tiff:Orientation` + `laika:Rotation`;
  explicit geometry wins on read (no double rotation), foreign
  Orientation-only files derive rotation/flips. Thumbs/Loupe/Develop/
  export all flow through the same geom, so all follow.
- Library: Cmd+[/Cmd+] rotate, [/] flip H/V over the scope (targets else
  primary), one undo step each via the edit history; open crop boxes
  transmute along. Cmd+Delete/Backspace counts rejected (never picked)
  and confirms with Remove and Trash side by side; Remove snapshots full
  rows (photo+edits+keywords+named snapshots) and undoes/redoes exactly
  (path-resolved, id-shift safe, missing originals skipped loudly).

- **Deliver:** Rotate left/right (`Cmd+[` / `Cmd+]`) and flip on selected photos in
  Library, stored as orientation in the edit model and `tiff:Orientation` in the
  sidecar, applied in thumbnails, Loupe, Develop, and export. **Delete Rejected
  Photos** (`Cmd+Delete`) shows the count and offers Remove from Catalog or Move to
  Trash per U17.
- **Done when:** Rotation of a portrait RAW composes with a later crop and
  straighten without a double rotation. Deleting rejected never touches picked or
  unflagged photos, and the action is undoable for Remove from Catalog.
- **Depends on:** U08, U17. **Touchpoints:** Edit model, renderer orientation, keyboard map.

### [x] V18 — Timeline view for the catalog and albums

**Result: done 2026-09-16, with two U13-owned cuts.** `cargo fmt`,
`cargo test --workspace` (149 tests) and
`cargo check --workspace --all-targets` pass, no new warnings. Core
verified by execution (grouping, gap re-runs, collapse, 50k scale
gate); the view itself is code-reviewed only — manually verify rows,
scrubber drags, jumps, and collapse in the running app.
- `laika-core::timeline` (pure): positional EXIF-stamp parsing, civil
  year/month/day sections, capture-gap events (configurable 1–168h,
  default 4) with deterministic date+folder suggested names, undated
  tail, collapse-aware flatten to uniform rows, month/day row lookup.
  Unit gates: ordering, wedding-day segmentation at 4h vs 12h, collapse
  without re-query, malformed stamps, 50k clustered photos
  (build ~75ms, flatten ~1ms — Timeline-only cost, proportionate to
  grid's per-frame cell work).
- View (T): year/month/day banners with counts, ranges, and chevrons;
  event banners; thumb rows reusing `grid_cell` (selection, culling,
  badges, overlays identical); month scrubber with density bars and
  click/drag-to-month in one frame on cached thumbs; Jump to Date
  (YYYY-MM-DD/MM); gap chips persisted per catalog; folder scope via the
  existing filters; capture order always (other filters apply).
- Deferred: events→album/collection and album scope — U13 owns
  collections/albums (no such tables exist); suggested names are shown
  on event banners. Pairs unfold in timeline (no hidden photos).

- **Deliver:** A **Timeline** view (`T`) that groups photos into collapsible
  year, month, and day sections with headers showing counts and date range, a
  vertical **scrubber** with a density strip and month labels, Jump to Date, and
  smooth scrolling across a 50,000-photo catalog through indexed capture-time
  queries. Auto-segment by capture gaps (configurable, default 4 hours) into
  **events** that can be turned into an album or collection with a suggested
  name from date and folder. Timeline applies to the whole catalog, a folder, or
  an album; within an album it uses the album's capture order.
- **Done when:** Dragging the scrubber to a month lands on that month within one
  frame using cached thumbnails. Collapsing a year hides its rows without
  re-query. The gap segmentation for a wedding day yields sensible events and can
  be re-run with a different gap. Selection and culling shortcuts work exactly as
  in Grid.
- **Depends on:** U03, U12, U21. **Touchpoints:** Library view segmented control,
  catalog queries, grid virtualization.

### [ ] V19 — Table view with sortable metadata columns

- **Deliver:** A **List** view with a small thumbnail and configurable columns
  (filename, capture time, camera, lens, focal length, aperture, shutter, ISO,
  rating, flag, label, dimensions, file size, format, folder, keywords, sync
  state). Click headers to sort, drag to reorder and resize, choose columns, and
  copy selected rows as tab-separated text. Inline-edit rating, label, and title.
- **Done when:** Sorting by ISO across 10,000 rows completes within the U12
  filter budget. Column layout persists per catalog. Keyboard navigation and
  selection match Grid behavior.
- **Depends on:** U04, U12. **Touchpoints:** Library views, catalog queries.

### [x] V20 — Loupe info overlay and simple slideshow

**Result: done 2026-09-17.** `cargo fmt`, `cargo test --workspace`
(including 6 new `slideshow` core tests + fit math) pass. Verified live
in the isolated e2e app (release, M1 Max, 9,427-photo catalog): info
overlay, full-screen entry/exit, letterboxed fit, auto-advance, cross-fade,
rating on the slide only, Esc back to Loupe on that photo; the user
confirmed every playback key by hand.
- `laika-core::slideshow` (pure): `LoupeInfo` cycle, `{token}` line
  templates (16 tokens) that drop empty values with their separators,
  per-catalog `SlideshowPrefs`, scope (multi-selection in visible order,
  else everything shown — folder, Photos album, filters) and stepping.
- Loupe `I` / toolbar Info button: none → file line → exposure line,
  both lines editable in Settings (empty restores the default).
- Slideshow (⌘Enter or ▶ Slideshow in Grid/Loupe/Wall/Timeline):
  enters full screen (restored on exit), interval 2–30 s, loop, 450 ms
  cross-fade, caption from title; Space pauses, arrows step, 0–5 and
  P/X/U mark only the slide on screen (never the selection, no
  auto-advance), Esc returns to Loupe keeping a selection that contains
  it. Controls appear on pointer move; toasts confirm marks.
- Previews: the 2048 shows at once; when the screen is sharper than 2048,
  a screen-sized render replaces it (fresh 1:1 cache, else GPU full
  render) for the current then next slide, one at a time, at most three
  kept. Auto-advance waits up to 2.5 s for the next preview.
- Frame budget: while playing only the slideshow is drawn (no grid,
  Develop, or top bar behind it).
- Honest limits: videos show their poster only; the 100-photo
  no-frame-drop gate was not measured (playback looked smooth, but no
  frame timing was recorded).


- **Deliver:** Loupe overlay (`I`) cycling none, filename/capture/dimensions, and
  exposure/camera/lens, with a customizable line format. A **Slideshow** of the
  current selection or album in full screen: interval, manual advance, loop,
  optional caption from title, fade transition, and `Esc` to exit. Shows the
  2048 preview immediately and swaps to a 1:1 render when available.
- **Done when:** The slideshow runs on a 100-photo album without frame drops on
  the reference hardware; rating and flag keys work during playback; exiting
  returns to the same photo in Loupe.
- **Depends on:** U07. **Touchpoints:** Loupe, full-screen state, preview loading.

### [x] V22 — Crop and geometry: perspective, straighten line, overlays, and numeric entry

**Result: done 2026-09-17, with stated limits.** `cargo fmt` and
`cargo test --workspace` pass (laika-core 160, laika-app 29,
laika-develop 12), no new warnings. Verified live in the isolated e2e app
(release, M1 Max): Transform panel, Auto and Guided Upright, straighten
line, O overlay cycling, typed W, X swap, output-size chip.
- Perspective model (`laika-core::upright`): Lightroom-shaped
  Vertical/Horizontal keystone (plane tilt re-projected at a normal
  focal length, center fixed), Rotate, Aspect, Scale, X/Y Offset as one
  homography; the renderer samples its inverse. Seven Transform sliders
  live in the params array (undo, 1:1/derivative cache keys, and
  `crs:PerspectiveVertical/Horizontal/Rotate/Aspect/Scale/X/Y` for free;
  legacy 63-wide rows pad). Upright mode, solved correction, guides and
  Constrain ride `CropGeom` (`laika:UprightMode/UprightAuto/
  UprightGuides/ConstrainCrop`). Values round-trip; Lightroom rendering
  equivalence is not claimed.
- Shader: one warp stage between straighten and unrotate, so preview,
  1:1, derivatives, slideshow and export share it; uncovered corners
  are white when Constrain is off, dark in the crop tool. GPU tests pin
  the WGSL against the CPU mirror, and an end-to-end gate detects a
  synthetic keystoned, rolled building, solves Full Upright, renders the
  export, and re-detects every long edge within 0.6° of its axis.
- Upright: `laika-core::lines` finds straight edges (Hough in ±40° bands
  around each axis, least-squares refit, 15 ms at 1024×683 release) on
  the cached 2048 editing linear. Level rotates only, Vertical adds
  vertical keystone, Full adds horizontal, Auto is Full pulled toward
  small corrections, Guided solves from 2–4 drawn lines (click a line to
  remove it; each change is one undo step).
- Constrain to image: warp-aware corner check (the covered region stays
  convex), re-centering when the box center itself is uncovered; a
  200-case randomized sweep never leaves an empty corner.
- Crop tool: Straighten line tool (or ⌘-drag) with live angle readout and
  ±0.1° fine steps; overlays Thirds, Golden ratio, Diagonals, Triangle,
  Golden spiral, 2×2 and N×N grid (O cycles the chosen set, ⇧O flips,
  set and grid size persist per catalog); outside area dim/hide/show;
  X/Y/W/H fields in pixels or percent (width/height keep an aspect lock);
  user aspect presets (save current ratio, remove) persisted per catalog;
  X swaps portrait/landscape; arrows nudge 1 px, Shift 10 px. The dims
  chip shows the output size ("2799 × 2099 of 3000 × 2250"). Reset
  geometry resets crop, straighten, Upright and Transform as one step,
  keeping orientation and tone. Paste Settings never carries Transform.
- Also fixed: Detail and Effects sliders now share Basic's inset (their
  readouts clipped at the rail edge).
- Limits: local masks (U19) don't exist yet, so "masks follow the
  corrected geometry" has nothing to apply to; closing the crop tool
  returns to Fit on the cropped frame rather than an animated zoom;
  Auto's line finder works on unedited camera-linear pixels, so busy
  organic scenes may yield only a small or no correction (it says so).


- **Problem:** U08 ships the draggable crop with aspect lock, rotate/straighten,
  flip, and a grid overlay. Everyday geometry correction still needs more.
- **Deliver:**
  - **Upright/perspective:** Auto (level), Level, Vertical, Full, and Guided
    (draw 2–4 lines) modes; manual vertical/horizontal/rotate/scale/aspect/offset
    sliders; **Constrain to image** that shrinks the crop to exclude transparent
    corners after any rotation or perspective change.
  - **Straighten tool:** draw a line along a horizon or edge to set the angle,
    with an angle readout and fine-scrub.
  - **Overlays:** thirds, golden ratio, diagonals, golden spiral, triangle, 2×2,
    and custom grid; cycle with `O`, flip orientation with `Shift+O`, choose the
    set to cycle through, and dim or hide the area outside the crop.
  - **Numeric entry and presets:** width/height/x/y fields in pixels or percent,
    a user-editable list of aspect presets (including saving the current crop's
    ratio), `X` swaps orientation, arrow keys nudge by 1 px and `Shift` by 10.
  - Zoom to crop when the tool closes; show the output pixel size in the
    dimensions chip; per-image reset for geometry separate from tone edits.
- **Done when:** A tilted architectural shot corrects to vertical lines with
  Guided mode and exports with the same geometry as the preview. Constrain to
  image never leaves an empty corner. Numeric entry and drag agree to the pixel.
  Perspective parameters serialize to `crs:PerspectiveVertical` and friends so
  U26 migration can map them, while Laika-only fields stay in the `laika:`
  namespace. Local masks from U19 follow the corrected geometry.
- **Depends on:** U08, U14. **Touchpoints:** Edit schema, `xmp.rs`, renderer warp
  pass, crop UI.

### [ ] V23 — Dehaze and the Effects panel: post-crop vignette and grain

- **Deliver:** Add **Dehaze** to Basic. Add an **Effects** panel with
  **Post-Crop Vignetting** (amount, midpoint, roundness, feather, highlights;
  styles highlight priority, color priority, paint overlay) applied after the
  crop from V22, and **Grain** (amount, size, roughness) using a seeded
  procedural noise that renders identically at preview and export resolution.
  Panel enable/bypass and reset, history steps, preset inclusion, and selective
  batch sync per U15/U16.
- **Done when:** Vignette follows the crop rectangle, not the original frame, and
  respects orientation from V15. Grain at export matches the 1:1 preview in
  structure and strength. Values write to `crs:PostCropVignette*`, `crs:Grain*`,
  and `crs:Dehaze` for interoperability.
- **Depends on:** U09, U14, U18, V22. **Touchpoints:** WGSL stages, `Params`,
  right rail, XMP.

### [ ] V24 — Color Grading, Black & White mix, and Calibration

- **Deliver:** **Color Grading** with shadows/midtones/highlights/global wheels
  (hue, saturation, luminance), blending and balance, and a split-toning
  compatibility mapping. **Treatment: Black & White** with a B&W mix panel
  (eight channel sliders and Auto), keeping color data for return to Color.
  **Calibration** with process version display, shadow tint, and red/green/blue
  primary hue and saturation. Each panel has bypass, reset, history, and preset
  inclusion.
- **Done when:** A three-way grade renders within the U09 tolerance between
  preview and export. Switching to B&W and back restores the color treatment and
  HSL values. Calibration changes apply before Basic in the pipeline so
  white-balance behavior is unchanged. Values write to `crs:ColorGrade*`,
  `crs:ConvertToGrayscale`, `crs:GrayMixer*`, and `crs:CameraCalibration*`.
- **Depends on:** U09, U18, U20. **Touchpoints:** WGSL pipeline, `Params`,
  right rail, XMP.

### [ ] V25 — Develop panel ergonomics: Auto Tone, Solo mode, panel state, Reset All

- **Deliver:** **Auto Tone** for exposure/contrast/highlights/shadows/whites/
  blacks from histogram analysis, as one history step. **Solo mode** so opening
  a panel collapses the others; panel open/closed state remembered per catalog.
  **Reset All**, per-panel reset, and reset to import defaults. `Alt`-drag for
  fine slider control and `Shift+double-click` for auto on a single slider where
  Auto applies. **Match Total Exposures** for a selection relative to the primary.
- **Done when:** Auto Tone on the fixture NEFs produces a usable starting point
  without clipping warnings. Solo mode never hides the panel a user just opened.
  Reset to defaults leaves crop and geometry untouched unless included.
- **Depends on:** U04, U09, U14. **Touchpoints:** Right rail panels, histogram
  analysis, keyboard map.

### [ ] V29 — Gallery theme system with multiple static HTML themes

- **Problem:** U25 finishes configuration, preview, and deployment for one
  masonry template; the form's other two templates still render masonry.
- **Deliver:** A **theme** abstraction in `laika-export` with at least five
  built-in themes: **Masonry** (existing), **Justified rows**, **Square grid**,
  **Story** (single column with large images and captions), and **Slideshow**
  (full-screen with thumbnails strip). A **theme picker** with live preview
  rendered from the album's actual first photos. Common options: title,
  description, cover photo, accent color, light/dark/auto, typography preset,
  caption source (title, caption, filename, none), EXIF line, ordering (album
  order, capture time, rating), sizes, download originals, watermark, strip GPS.
  Output uses `<picture>` with AVIF and WebP plus JPEG fallback via V27, a
  keyboard-navigable lightbox, lazy loading, relative paths that work from
  `file://` and any subpath, no external requests, and an optional `index.json`
  manifest. Support a **user theme directory** with a runtime template engine
  (`minijinja`) and a documented variable set; built-in themes stay compiled.
- **Done when:** Building the same album with each theme opens correctly in
  Safari, Chrome, and Firefox on desktop and phone widths. Theme switch preserves
  album options. Total output size and photo count show before build. A broken
  user theme reports the template error line without leaving partial output.
- **Depends on:** U10, U25, V27, V30. **Touchpoints:** `laika-export` templates,
  Publish form, theme options schema.

### [x] V30 — Albums: manual ordering, cover, description, and captions

**Result: done 2026-09-17, with stated limits.** `cargo fmt` and
`cargo test --workspace` pass (laika-core 169, including a 200-photo
reorder that survives reopen), no new warnings. Verified live in the
isolated e2e app (release): v7→v8 migration with backup, building a
7-photo album with B, dragging the last photo to the front, an album
caption that left the photo's title empty, and the order intact after a
relaunch.
- Schema v8: `collection_items.position` and `.caption`,
  `collections.cover_photo_id`, `.title`, `.description`. Existing
  members keep the order they were added in. New members append; removing
  one (or the photo) leaves the rest in order; a cover that leaves the
  album reads as none. Saving the Quick Collection keeps its order.
- Order: pure `laika-core::album` (block moves keep their own order,
  unknown ids ignored, stays a permutation over 500 random moves of 200).
  Every collection shows in the new **Album** sort (selecting a collection
  switches to it; leaving switches back to date). In Grid, dragging photos
  (pairs together) onto a cell makes them take that cell's slot — after it
  when moving forward, before it when moving backward — with a bar on that
  side, so every slot (including the first cell of any row) is reachable;
  a folder under the pointer still wins. Slideshow plays in album order.
  Export and rename now number files in on-screen order (previously
  catalog id order), so album order carries into `{seq}`.
- Album panel (left rail, while viewing a collection): gallery title
  (empty uses the collection name), description, cover (use selected /
  clear, with thumbnail), and a one-click switch to album order.
- Captions: an "In album" field under Caption in the right rail edits the
  album-only caption over the selection (`<mixed>` on disagreement, batch
  empty leaves captions unchanged); "use as photo title" copies it to the
  photo's own title only when chosen. Album captions win in the slideshow
  caption and in exported JPEG captions when exporting from the album.
  Publish prefills its title from the album.
- Limits: the gallery build (V29) doesn't exist yet, so "the default gallery
  order" and "captions the gallery shows" are ready in the catalog API
  (`album_order`, `album_captions`, cover, title, description) but not yet
  rendered by a gallery; Timeline stays chronological (it groups by capture
  date); drag-to-reorder is Grid only (the Wall has no reorder); the
  description is a single-line field.


- **Problem:** U13 adds collections. Galleries and slideshows also need a
  curated order, a cover, and text.
- **Deliver:** Any collection can act as an **album**: drag to reorder with a
  custom order persisted per album, set a cover photo, edit title and
  description, and edit per-photo captions inline without changing the
  photo's global title unless chosen. Album order is available as a sort in
  Grid, Timeline, and Slideshow, and is the default gallery order in V29.
- **Done when:** Reordering 200 photos persists across restart and export.
  Removing a photo from an album keeps the remaining order. Captions edited in
  the album are the ones the gallery shows.
- **Depends on:** U13, U04. **Touchpoints:** Collections schema (`position`,
  `cover_photo_id`, `caption`), Grid drag, Publish form.

### [x] V31 — Preferences window and complete menu bar

**Result: done 2026-09-17, with stated limits.** `cargo fmt` and
`cargo test --workspace` pass (laika-core 172, laika-app 30), no new
warnings. Verified live in the isolated e2e app (release): the native menu
bar with every group, View › Timeline and Settings… from the menus, the
Preferences tabs, Filmstrip size Large visibly enlarging the Loupe
filmstrip and persisting to `library.json`, and the in-window menu bar
(forced on macOS with `LAIKA_INWINDOW_MENU=1`) running View › Timeline.
- Storage: `AppPrefs` inside `library.json` in the app support directory
  (serde defaults, clamped on read); Reset to Defaults restores
  preferences, appearance and cache limits, keeping recents, the launch
  catalog and the cache location. Per-catalog settings (label names,
  Loupe info lines, slideshow, the catalog's own grid choices and pair
  grouping) sit under "This catalog" or say so.
- General: catalog at launch (last / ask / fixed + chooser); language and
  version shown.
- File Handling: cache location, cap and 1:1 TTL; import dialog defaults
  (add vs copy, skip duplicates, new only, eject); RAW+JPEG grouping for
  catalogs that haven't chosen; sidecar policy (write, or catalog only —
  no writes or heals, external sidecar edits still read).
- Interface: default columns, cell style, overlay and badge set for
  catalogs without their own; hide panels on entering the wall; filmstrip
  size (small/medium/large, applied live).
- External Editing: application, TIFF or JPEG, file-name template; new
  ⌘E / File › Edit in External Editor renders the selection next to the
  originals and opens the files in that application.
- Performance: GPU preference (high performance / low power) with the
  adapter in use; import worker count (auto or 1–32; the env var still
  wins and is flagged); thumbnails decoded at once.
- Menus: one command table (70+ commands in Laika/File/Edit/Photo/
  Develop/View/Help, submenus for rating, flag, label, Upright, catalog,
  backup, Apple Photos) drives the native macOS menu bar and an
  in-window bar elsewhere; every title shows its shortcut. The keyboard's
  view/mode keys now run the same commands. ⌘Q, ⌘, and ⌘H are native key
  equivalents; the other keys stay on the window's handler so text fields
  and dialogs keep them.
- Limits: English is the only language; there is no update service to
  check (version shown instead); external editing is 8-bit sRGB with no
  re-import round-trip (V26); the GPU preference applies on the next
  launch and only matters with two GPUs; smart/1:1 preview builds stay
  sequential; Preferences is an in-window panel rather than a separate
  OS window; the Linux menu bar was exercised on macOS only.


- **Deliver:** A Preferences window with **General** (catalog at launch, language,
  update check), **File Handling** (cache location and cap from V09, import
  defaults, RAW+JPEG policy, sidecar policy), **Interface** (thumbnail defaults,
  badge set, overlay format, wall mode defaults, filmstrip height), **External
  Editing** (application, format, color space, bit depth, naming), **Performance**
  (GPU selection, decode threads, preview generation concurrency), and **Reset
  to Defaults**. A native menu bar (macOS) and in-window menu (Linux) exposing
  every command with its shortcut, including the U03 shortcuts and all V-items.
- **Done when:** Every preference has an effect the user can observe and persists
  in the app support directory, not the catalog, except per-catalog ones which
  say so. No command exists only as a keyboard shortcut.
- **Depends on:** U03, U04. **Touchpoints:** App shell, settings store, menu
  definitions.

## P2 — Extended workflow

### [ ] V06 — Convert to DNG on import or later

- **Deliver:** Optional **Copy as DNG** during import and **Convert to DNG** for a
  selection, using rawler's DNG writer with lossless compression, embedded
  original preview, and optional embedded original RAW. Keep or delete the
  original after a verified conversion. Sidecar edits move to the DNG.
- **Done when:** A converted NEF opens in Laika with identical decode results and
  in Lightroom as a valid DNG. Conversion errors keep the original and report.
- **Depends on:** V01, U20. **Touchpoints:** `laika-raw`, import job.

### [ ] V10 — Export as catalog and import from another catalog with merge

- **Problem:** U26 mentions a portable Laika bundle for migration; nothing defines
  the format or how two Laika catalogs merge (laptop shoot into the main desktop
  catalog).
- **Deliver:** **Export as Catalog** for a selection: a folder containing a new
  SQLite catalog, sidecars, previews, optional smart previews (V09), and optional
  copies of originals with relative paths. **Import from Catalog**: choose the
  bundle, see a summary (new photos, photos already present by hash, changed
  edits/metadata), pick per-category rules (add, replace metadata and edits,
  keep both as a virtual copy, skip), choose whether to copy originals into a
  destination template from V02, and get a report. Hash identity, not path,
  decides “already present”.
- **Done when:** A shoot edited on a laptop merges into the desktop catalog with
  edits, ratings, labels, keywords, albums, and history intact and no duplicate
  photos. Conflicts are shown before writing. The bundle opens directly as a
  standalone catalog too.
- **Depends on:** V07, V08, V09, U14. **Touchpoints:** `catalog.rs`, bundle
  format, import UI.

### [ ] V11 — Find duplicates across the catalog

- **Deliver:** A **Find Duplicates** command that groups photos by blake3 (exact
  copies) and, optionally, by embedded-preview perceptual hash (near copies such
  as the same frame exported twice or a RAW and its DNG). Show groups in a Survey-
  like view with the file path, size, and edit status, mark a keeper per group by
  rule (largest, oldest, most edited, in a chosen folder), and stack or remove the
  rest per U13/U17.
- **Done when:** Exact duplicates in a 10,000-photo catalog list in under the
  U12 filter budget. Near-duplicate grouping never auto-removes anything. Removal
  respects the U17 distinction between catalog and disk.
- **Depends on:** U11, U13, U17. **Touchpoints:** Catalog query, perceptual hash
  column, Survey view.

### [ ] V21 — Second window and secondary display

- **Deliver:** A second window showing Grid, Loupe, Compare, or a locked photo,
  following the main window's selection or locked, with full-screen on a second
  display. Shortcuts route to the focused window; culling keys affect the main
  selection.
- **Done when:** Grid on the laptop and Loupe on the external monitor stay in sync
  during culling with no extra latency beyond the U-gates. Closing the second
  window keeps its layout for next time.
- **Depends on:** U03, U07, U22. **Touchpoints:** GPUI window management, `AppState`.

### [ ] V26 — Edit in external editor with round-trip

- **Deliver:** **Edit In…** renders the current edit to a 16-bit TIFF or PNG in the
  chosen color space (or passes the original), opens it in the configured
  application, watches the file, imports the result next to the original when it
  changes, and stacks it with the original. Options: edit original, edit a copy
  with Laika adjustments, naming suffix.
- **Done when:** A round-trip through GIMP or Affinity Photo returns a file that
  appears in the catalog with metadata copied from the source and shows up
  stacked. The exported intermediate honors V27 metadata and profile choices.
- **Depends on:** V27, V31, U13. **Touchpoints:** Export render, file watcher, stacks.

### [x] V32 — Diagnostics, logging, About, and first-run onboarding

**Result: done 2026-09-17.** `cargo fmt`, workspace tests pass (new:
`logging` unit tests for redaction/rotation/timestamps, a `crash_report`
integration test, and the existing suites), verified live in a fresh
isolated HOME.
- Logging (`laika-core/src/logging.rs`): stderr is teed through a pipe, so
  every existing `[area]` line lands in
  `~/Library/Application Support/Laika/logs/laika.log` with a UTC
  timestamp, still echoed to the terminal. Rotates at 5 MB, keeps 4
  (`laika.1.log`…). `LAIKA_LOG=0` disables. Each launch writes a start
  line (version, OS, arch, pid).
- Support coverage: failures that were silent now log with file and
  reason — export per-file failures and a run summary, Develop decode
  failures (path, camera, type), gallery builds and per-photo failures,
  wrangler deploy output and failures. Import, sync, rename/move, and
  Apple Photos already logged.
- Credentials never reach disk: the S3 secret is registered for scrubbing
  when loaded (keychain or `LAIKA_S3_SECRET`); every line is also scrubbed
  of URL userinfo, `secret|password|token|authorization|access_key=…`
  values, and AWS key ids.
- About (Laika/Help menu and Preferences → General): version, git build
  and date, OS, GPU adapter, catalog path/size/count and root, cache,
  rawler/wgpu/gpui versions, log path, crash-report state; Copy
  Diagnostics, Show Log, Show Crash Reports. Help menu also has Show Log
  in Finder and Welcome Screen.
- Crash reports (opt-in toggle in About and Preferences, persisted in
  `library.json`): the panic hook always logs the panic; when on it writes
  `logs/crash-<time>.txt` with the message, location, backtrace, the
  diagnostics block, and the last 200 log lines, all scrubbed. Nothing is
  sent anywhere.
- First run: a welcome screen when the library has never been welcomed
  and the open catalog is empty (existing libraries are marked welcomed
  silently). It explains local originals and the catalog, shows the
  catalog path with Create Catalog Elsewhere (the V07 folder + name
  flow), Try with sample photos (copies the bundled fixture NEFs to
  `~/Pictures/Laika Samples` and imports them), Import your photos, and
  Skip (Esc also skips).
- Gates: a fresh HOME reached a populated 3-photo grid about 1 s after
  clicking Try with sample photos (import 0.4 s); logs of that run show
  the start, migration, catalog open, samples, and import lines.
- Honest limits: the samples ship from `Resources/samples` in a bundle,
  but no bundling script copies them there yet (development builds use
  `fixtures/raw`, and the button disables with a reason when neither
  exists). Log lines are the existing free-text `[area]` lines, not
  key/value records. Panics in the render thread are logged and
  reported like any other; a hard crash (signal, not a panic) writes no
  report.

- **Deliver:** Structured logs with rotation in the app support directory, a
  **Show Log** action, an About window with version, build, catalog path, GPU,
  and rawler/wgpu versions, and an opt-in crash report file the user can attach
  to an issue. A first-run screen that explains local originals, offers Create
  Catalog with a location picker, and a one-click import of the bundled fixture
  RAWs so the app is never empty on first launch.
- **Done when:** A support request can be answered from the log without asking
  for reproduction steps for import, export, sync, and decode failures. Logs
  never contain credentials. First run reaches a populated grid in under a minute
  on the reference hardware.
- **Depends on:** U01, V07. **Touchpoints:** Logging setup, app shell, import.

## Release gates and validation

These extend the gates in `backlog.md`; they do not replace them. Record the
same hardware, build, and cache-state context with each measurement.

| Scenario | Acceptance target |
| --- | --- |
| Card import, 64 GB, ~2,000 RAW files | Copy runs at drive-limited speed; every file hashed at both ends; card removal mid-copy leaves no partial file or orphan row |
| Second-copy import | Both destinations verified; a failure on one destination is reported per file and does not fail the other |
| Folder/rename template preview | Preview updates within 100 ms of a template edit for the first three files |
| Catalog open with migration | A 10,000-photo prototype catalog migrates in under 10 seconds with a backup written first |
| Integrity check / Optimize | Progress visible; UI remains responsive; results list orphaned rows and previews |
| Wall mode reflow | Window resize re-lays 2,000 justified thumbnails within one frame at 60 fps using cached sizes |
| Timeline scrub | Jump to any month in a 50,000-photo catalog within 150 ms p95 using indexed capture-time queries |
| Table sort | Sort 10,000 rows by any column within 300 ms |
| Multi-format export, 50 photos at 2048 px | All formats produce files that open in the listed viewers with the correct profile, orientation, and metadata policy; AVIF/JXL encode time recorded per effort level |
| Gallery build with each theme | Output opens from `file://` and a subpath with no console errors; all images load via `<picture>` fallbacks |
| Perspective correction | Preview and export geometry agree to within one pixel at export resolution on fixture images |
| Vignette/grain fidelity | Export matches the 1:1 preview within the U09 rendering tolerance |
| Catalog merge | Two catalogs with 30 overlapping photos merge with zero duplicates and every conflict shown before writing |

Add fixtures: a real SD card image with `DCIM` and vendor folders, RAW+JPEG
pairs, a short video, a JPEG with a wrong camera clock, a tilted architectural
frame, and an existing Lightroom-written sidecar with labels and hierarchical
keywords. Verify exported metadata with `exiftool` in automated tests and confirm
format compatibility in the listed viewers by hand.

## Benchmark references

Adobe's Lightroom Classic documentation for
[importing from a camera or card](https://helpx.adobe.com/lightroom-classic/help/import-photos-camera-card-reader.html),
[file renaming and folder organization](https://helpx.adobe.com/lightroom-classic/help/rename-photos.html),
[catalog management](https://helpx.adobe.com/lightroom-classic/help/create-catalogs.html),
[export settings and formats](https://helpx.adobe.com/lightroom-classic/help/export-files-hard-disk.html),
and the [Develop module reference](https://helpx.adobe.com/lightroom-classic/help/develop-module-basics.html)
define the expected behaviors that this backlog adapts. These guide feature
shape; Laika's acceptance targets are product proposals based on the source
review. Consulted 2026-09-15.
