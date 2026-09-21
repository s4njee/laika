# Laika backlog v3 — the switching case

Updated: 2026-09-18. All items below are open.

## Scope of this document

`backlog.md` (U01–U27) made Laika trustworthy for one shoot: persistence,
culling, Basic editing, export, and the first organization and backup features.
`backlogv2.md` (V01–V32) added the everyday Lightroom Classic features: card
ingest, catalog lifecycle, metadata, alternate views, crop and geometry, export
formats, preferences, and diagnostics. `photogallery.md` (G01–G28) covers the
gallery builder.

This backlog answers a different question: **why would a working photographer
leave Lightroom Classic for Laika?** Feature parity alone won't convince them.
Someone with ten years of catalog, presets, and muscle memory switches when:

1. **Leaving Lightroom costs an afternoon, not a month.** Their catalog,
   collections, edits, and presets come across. What doesn't is listed, not
   discovered later.
2. **Laika is clearly better at something they do every day.** It has to be
   faster, more private, more automatable, or more in their control.
3. **They can't be locked in again.** The catalog and edits stay readable
   without Laika, and leaving Laika is as easy as arriving.
4. **Installing it doesn't feel risky.** Releases are signed and notarized,
   updates are safe, and it runs on the machines they own.

Every item here serves one of those four reasons. Items that only reach parity
belong in the earlier backlogs, and this document references them instead of
restating them. It includes the parity gaps that stop a switch outright
(Section E), because it is dishonest to promise easy switching while hiding
them.

Numbering is S01–S30. Format, priorities, and release gates follow `backlog.md`;
each item also carries a **Why switch** line naming the reason it serves. This is
a product backlog grounded in the current source, not a claim of live-app
verification.

## The pitch, and where it is not yet true

| Lightroom Classic pain | Laika's answer | Honest status today | Items |
| --- | --- | --- | --- |
| Subscription; the catalog is useful only inside Adobe's app | One-time local app; catalog is plain SQLite plus XMP sidecars beside originals | Sidecars and SQLite exist; format undocumented; no "leave Laika" export | S06–S08 |
| Migrating away from anything is a project | Import the `.lrcat` directly with a report of what came across | Only XMP sidecars are read (U26 plans sidecar-level migration) | S01–S05 |
| Slow previews, slow culling, beachballs on big catalogs | GPU pipeline, embedded-preview culling, published benchmarks | Fast in practice; never measured against Lightroom | S09–S11 |
| Cloud-tied AI and Adobe account sign-in | On-device culling assist and search; no account, no telemetry | Nothing yet; no statement or audit of network use | S12–S14, S26 |
| Plug-in SDK is Lua, dated, and limited | CLI, rules, and sandboxed extensions | Headless `--import` and `--sync-now` flags only | S18–S20 |
| One catalog, one machine; mobile needs Creative Cloud | Multi-Mac sync over your own storage; phone culling over the LAN | Backup to S3/SFTP/SMB, not catalog sync | S21–S22 |
| macOS and Windows only | macOS, then Linux | macOS only; CI once built Linux (plan.md) | S25 |
| Map module needs Google and the internet | Local map with opt-in tiles, offline place names | Map module on `exp`: clusters, filters, EXIF/storage rail, place-on-map, opt-in Esri tiles | S15–S17 |
| — (things Lightroom does that Laika doesn't) | Close the gaps that stop a switch | Lens profiles, virtual copies, masks/heal (U19), color management (U20) | S08, S27–S30 |

## Current baseline relevant to this backlog

| Area | Implemented foundation | Gap this backlog addresses |
| --- | --- | --- |
| Migration | XMP sidecars read and written with `crs:` develop fields; foreign attributes preserved | No `.lrcat` reader; no Adobe preset import; no coexistence rules; no render-difference report |
| Portability | SQLite catalog, `import_defaults`, migrations to schema v10, sidecars beside originals | No published schema or edit spec, no reference renderer, no export-everything bundle |
| Speed | wgpu develop, virtualized grid, background jobs, scan fingerprints skip known card files | No comparative benchmark; culling waits for import to finish |
| Intelligence | None | No on-device model runtime, embeddings, or quality scores |
| Places | `geo.rs` (projection, clustering, fit, places), `set_gps`, XMP `exif:GPSLatitude/Longitude`, Map module | No tracklogs, place names, private zones |
| Automation | Headless `--import` and `--sync-now` | No CLI surface, rules engine, or extension API |
| Devices | One catalog per machine; S3/SFTP/SMB backup; smart previews | No catalog sync, no companion culling |
| Distribution | Ad-hoc signed `.app`/`.dmg` from GitHub Actions; nightly and tagged releases | Not notarized; no updater; no Linux build; no license file |

Source anchors: `crates/laika-core/src/{catalog.rs,xmp.rs,edit.rs,geo.rs,import.rs,sync.rs}`,
`crates/laika-app/src/{main.rs,map_view.rs,diagnostics.rs}`,
`scripts/bundle-macos.sh`, and `.github/workflows/release.yml`.

## Priorities and delivery order

- **P0 — Make switching possible:** migrate a Lightroom catalog, stay free to
  leave, install without fear.
- **P1 — Make switching worth it:** measured speed, on-device intelligence,
  automation, places.
- **P2 — Make staying delightful:** multiple devices, extensions, Linux.

| Milestone | Exit condition | Items |
| --- | --- | --- |
| A. Arrive | A 20,000-photo Lightroom catalog imports in one pass with a report, and Lightroom still works on the same files | S01–S05, S08 |
| B. Trust the exit | Laika's catalog and edits are documented, re-renderable without the app, and exportable in one step | S06–S07 |
| C. Install without fear | Notarized, auto-updating builds; a verifiable no-telemetry promise; a license | S23, S24, S26 |
| D. Prove it's faster | Published, reproducible numbers against Lightroom on the same machine and shoot | S09–S11 |
| E. Remove deal-breakers | Lens profiles, virtual copies, HDR output, and a clear status for masks and color | S27–S30 (plus U19, U20) |
| F. Beat it daily | Culling assist, natural-language search, places, CLI, and rules | S12–S20 |
| G. Everywhere you work | Two Macs, a phone on the LAN, and Linux | S21, S22, S25 |

Milestones A–C are P0. D–F are P1. G is P2.

## Section A — Arrive in an afternoon

### [ ] S01 — Import a Lightroom Classic catalog directly

- **Deliver:** Open a `.lrcat` and import it without Lightroom running. Work
  from a copied snapshot and never write to Adobe's file. Read folders and root
  paths (with relink when volumes differ), ratings, flags, color labels
  (mapping Lightroom label names to Laika's `LabelNames`), hierarchical
  keywords with synonyms and export flags, and the collection set / collection
  tree. Also read smart collections where their criteria map (report the rest),
  stacks, virtual copies (S08), edited capture times, IPTC fields, GPS, and the
  latest develop settings plus the named snapshots. A dry-run screen shows
  counts per category, a per-photo table of skipped or approximate settings, and
  estimated time before anything is written. Import runs as a cancellable
  background job with a resumable checkpoint.
- **Done when:**
  - Three real catalogs of 1k, 20k, and 100k photos import from Lightroom
    Classic 12–14 fixtures; each photo's ratings, flags, labels, keywords,
    collections, and stacks match the source.
  - Any develop settings Laika can't render are listed by name, and the develop
    report totals equal the photo count.
  - The `.lrcat` is byte-identical afterwards, and re-running the import updates
    records in place without duplicating them.
- **Why switch:** reason 1. This replaces a month of manual migration with a
  report.
- **Depends on:** U13, U26 (sidecar parity and the report format), V07, V08.
  **Touchpoints:** new `laika-core::lrcat`, import job queue, catalog
  migrations for virtual copies and stacks.

**Progress 2026-09-18: Lightroom (cloud) libraries done; Classic `.lrcat` still open.**
The first real library available was a Lightroom desktop `.lrlibrary`, not a
Classic catalog, so that reader shipped first. The shared model (links,
report, UI) is where the `.lrcat` reader will plug in.

- **Reader:** `laika-core::lightroom` and `msgpack`.
  - It copies `Managed Catalog.mcat` (SQLite, one MessagePack document per
    asset, album, and link) and reads the copy.
  - It reads assets (file name, size, SHA-256, capture date, rating, pick or
    reject, title, location, the develop-settings blob), albums with folder
    ancestry and custom order, stacks, people, virtual copies, and face
    regions.
  - Titles that are just file names are dropped.
  - Camera Raw settings come from `settings/<sha>`, parsed with the new
    `xmp::parse(bytes)`.
  - `unsupported_settings` names what Laika doesn't render. It reads only the
    photo's own settings, never a profile look's built-in curve.
- **Matching:** `find_originals` finds originals by content (size, then
  SHA-256 using `sha2` 0.11 hardware SHA), so renamed or moved files match.
  - It searches the catalog's photos, the originals Lightroom keeps locally,
    and any folders the user adds.
  - Originals found inside Lightroom's package are copied to
    `~/Pictures/Lightroom Originals` rather than referenced there.
- **Apply:** schema v11 adds `lightroom_links`. `resolve_lightroom` and
  `apply_lightroom` then apply everything in one transaction.
  - By default only blank values are filled; "Replace" overwrites. Values
    Laika already has are counted as kept, unless they already equal
    Lightroom's.
  - Albums become collections in Lightroom's order. Names are prefixed with
    parent folders only when two albums share a name.
  - Re-running uses the links: no re-hashing, no duplicates, no reapplication.
- **UI:** File → Import from Lightroom….
  - Steps: pick library → summary (stats, what won't come across, where to
    search, album checklist, develop/replace toggles) → matching progress →
    review (in catalog / to add / to copy / missing) → import → done.
  - The report is written to the log folder, listing every original not
    found.
- **Tests:** MessagePack decoding, the unsupported-settings report,
  file-name titles, content matching, and an end-to-end
  apply/re-apply/replace test.
- **Live test:** a 10,637-asset library against a 9,434-photo test catalog.
  - Reading took 0.8 s. Matching read 7,214 files in 32 s: 7,233 photos were
    already in the catalog, 48 were copied and added, and 3,356 were
    cloud-only.
  - Applying took 0.4 s: 131 locations, 69 develop settings, and 31
    collections (17,646 memberships).
  - Re-import matched in 2.4 s and changed nothing.
- **Still open:**
  - The Classic `.lrcat` reader (needs a real fixture).
  - Keywords (none in the test library).
  - Color labels and edited capture times.
  - Smart-album translation, stacks, and virtual copies (S08) — all reported,
    not imported.
  - Cloud-only originals and settings, which can only be listed.

### [ ] S02 — Import Lightroom presets and profiles

- **Deliver:** Import Adobe develop presets (`.xmp`, and legacy
  `.lrtemplate` Lua tables), keeping their group structure, amount sliders
  where present, and "include" masks. Import metadata presets and filename
  templates where tokens map (V02 templates). Each preset shows a badge:
  complete, approximate (named settings), or unsupported (profile-dependent or
  mask-dependent). Camera and creative profiles are listed as unsupported with
  the nearest Laika look suggested, never silently substituted.
- **Done when:** Adobe's bundled preset groups and three third-party packs
  import, and each preset's badge matches its actual rendered coverage. Applying
  an approximate preset records one history step and names what it skipped.
- **Why switch:** reason 1. Presets are the most personal thing a Lightroom
  user owns.
- **Depends on:** U16, U26, V02.

**Progress 2026-09-18: develop and metadata presets import; filename templates and real third-party packs still open.**

- **Parser:** `laika-core::presets`.
  - It reads Adobe `.xmp` presets (name and group from their Alt
    structures, the amount flag, top-level settings only) and Lightroom
    Classic `.lrtemplate` files through a small Lua table reader (Develop
    and Metadata types; `$$$/…=` titles are cleaned).
  - Presets are sparse: only the settings a file includes are stored. So
    Adobe's "include" choices hold, and included zeros still reset values.
  - Coverage is **Complete**, **Approximate** or **Unsupported**. Every gap is
    named: black & white mix, profiles, lens profile, RGB channel curves,
    grain size, masks, and so on.
  - Two settings are approximated, and the approximation is named:
    - Point curves are sampled onto Laika's 20/40/60/80 % curve. That's exact
      only when the curve already sits on those points or is straight.
    - Black & white conversion, including Adobe's stubbed B&W and
      Monochrome looks, becomes Saturation −100.
  - Profiles (`PresetType="Look"`) are never imported; each gets a suggested
    Laika look. Other template types are listed as not imported.
- **Storage:** schema v12 adds `develop_presets`. The same group and name
  replaces an older version; the identical file (by SHA-256) counts as
  already imported.
- **UI:**
  - Develop → Presets lists Laika's looks, then the imported groups
    (collapsed, each with a two-step ✕ to remove).
  - Approximate presets carry ≈, and hovering shows the gaps.
  - Applying a preset changes only its included settings, records one
    history step, and the status line names what was skipped.
  - Presets with an amount slider get 50–150 % chips that re-apply from the
    same base.
  - File → Import Presets… (and "Import…" in the rail) finds Camera Raw
    settings, Lightroom Classic develop and metadata presets, and the
    presets inside an installed Lightroom or Lightroom Classic app. It can
    also take any files or folder.
  - A review screen lists every file with its badge and the named gaps.
    Unsupported presets start unchecked; profiles are summarized.
  - Imported presets also appear in the photo import dialog's develop-preset
    list (`user:<id>` keys).
- **Live test:**
  - The 72 Adobe presets bundled with Lightroom (990 files including
    profiles) all parse: 16 complete, 47 approximate, 9 unsupported.
  - A test pack (2 `.lrtemplate` develop presets, 1 metadata preset, 1 user
    `.xmp`) imported with correct badges and stored values.
  - B&W Flat rendered black and white; at 50 % it halved its changes.
  - Re-importing reported the 63 already there, and removing a group worked.
- **Still open:**
  - Filename templates (no real Lightroom Classic example to map tokens from).
  - Testing against real third-party packs.
  - An amount slider instead of chips.
  - Saving and renaming your own presets (U16).

### [ ] S03 — Work beside Lightroom on the same files

- **Deliver:** A coexistence mode for photographers who switch gradually.
  - Detect when Lightroom wrote a sidecar (via `xmp:CreatorTool` and
    `crs:` history), and never drop Adobe-only fields or history on rewrite.
  - Show a "last written by" indicator per photo.
  - Offer a per-catalog policy: *Laika leads*, *Lightroom leads* (Laika
    read-only for develop settings), or *ask on conflict*.
  - A conflict screen shows both versions of rating, keywords, and develop
    settings, and lets the user pick one per field.
- **Done when:**
  - Alternating edits between Lightroom and Laika on the same 200 photos for
    ten rounds loses no metadata in either app.
  - Every change Laika can't represent is preserved byte-for-byte.
  - *Lightroom leads* never writes a develop field.
- **Why switch:** reason 1. Trial without commitment is how switching actually
  happens.
- **Depends on:** U02 (safe sidecar merge), U26.

**Progress 2026-09-18: built and tested with a simulated Lightroom Classic writer; not yet tried against Lightroom Classic itself.**

- **Shared file names** (`laika-core::sidecar`):
  - Raw files use Adobe's `IMG_0001.xmp` when that file exists, or when the
    catalog shares files with Lightroom. JPEG/HEIC keep `IMG_0001.JPG.xmp`,
    since Adobe embeds their XMP and Laika never touches originals.
  - Keyword paths are written in Lightroom's `Places|Portugal|Lisbon` form,
    and both forms are read. Previously each app read the other's
    hierarchies as one long keyword.
- **Merge writes:** every sidecar write now rewrites only the fields Laika
  changed. Everything else is carried byte-for-byte: Adobe-only settings,
  nested structures (RGB curves, profile looks, masks, `xmpMM:History`) and
  other namespaces.
  - Before this, a Laika rewrite dropped every nested element and flattened
    curves Laika only half-understands.
  - Laika stamps `laika:Modified` and bumps `xmp:MetadataDate`, so Lightroom
    notices the change.
- **Last writer:** worked out from `laika:Modified` against
  `xmp:MetadataDate` and the last `xmpMM:History` agent. It appears as a
  "Sidecar" row in the Library rail, for example "Lightroom Classic 13.0 ·
  5 min ago", with "· conflict" when one is open.
- **Policy per catalog**, under Settings → File Handling → Sidecars:
  - *Newest wins*: the previous behaviour, and the default.
  - *Laika leads*, *Lightroom leads*, *Ask on conflict*: sidecars merge field
    by field.
    - The merge is three-way against a stored baseline (schema v13:
      `sidecar_baseline`, `sidecar_conflicts`), across rating, label,
      keywords, title & caption, creator & copyright, GPS, and develop.
    - A field only one side changed flows across; only real conflicts need a
      policy decision.
    - *Lightroom leads* never writes develop, crop or orientation fields.
- **Watching for changes:**
  - Shared sidecars are checked every 12 s; file stats run off the UI thread.
  - A write that finds the file changed outside Laika merges first, so a
    Lightroom change made moments earlier is never silently overwritten.
- **Conflicts:**
  - A top-bar "N to review" pill (and File → Review Sidecar Conflicts…)
    opens *Changed in Both Apps*. It shows Laika's and the sidecar's version
    of each field, with per-field and all-at-once choices.
  - Conflicted photos aren't rewritten until you decide.
- **Tests:**
  - A 200-photo × 10-round alternating simulation of Lightroom and Laika
    edits: Lightroom's changes arrive, Laika's reach the file, and the RGB
    curve, look, extra namespace and lens setting stay byte-identical while
    the history only grows.
  - Policy outcomes for real conflicts, and Lightroom-leads keeping develop
    out of the file.
  - Unit tests for the merge writer, last-writer detection and naming.
- **Live test (test app):**
  - A Lightroom-style `IMG_5443.xmp` was adopted within 12 s.
  - The rail showed "Lightroom Classic 13.0".
  - Laika's rating change reached the file with every foreign element intact.
  - A simulated Lightroom rating change followed by an immediate Laika
    change produced one conflict. Resolving it with "Keep Laika's" wrote ★★
    and kept Lightroom's history.
- **Still open:**
  - Testing against Lightroom Classic itself (not installed here).
  - Per-field choices inside develop (it's one group today).
  - Migrating older Laika-style `IMG.NEF.xmp` files when switching to shared
    names; they're left in place.

### [ ] S04 — "Coming from Lightroom" mode

- **Deliver:** A first-run option (and a Preferences switch) that does four
  things:
  - Applies a Lightroom-compatible keyboard map (G/E/D/R/Q/W/C/N, ⌘'
    virtual copy, ⌘⇧C/V, `\` before/after, `[`/`]` rotate).
  - Renames nothing but adds Lightroom terms as search aliases in the command
    palette and menus ("Spot Removal" → Heal, marked *not yet available*).
  - Shows a one-page concept map: Catalog, Collections, Virtual Copies,
    Snapshots, and Publish Services mapped to their Laika equivalents.
  - Links the migration report from S01.
- **Done when:** A Lightroom user performs the U23 task script with no shortcut
  lookups for the twenty most common actions. Searching any Lightroom menu
  command name finds either the Laika command or an honest "not available".
- **Why switch:** reason 1. Muscle memory is the hidden switching cost.
- **Depends on:** V31 (command table), U23.

**Progress 2026-09-18: built; not yet tried by a Lightroom user.**

- **Keyboard map** (app-wide): Settings → Interface → Keyboard, the welcome
  screen's "I'm coming from Lightroom", or the switch on the guide page.
  - **W** white balance picker, **R** crop (again to close), **⌥⌘1/2/3**
    Library / Develop / Map, **⌥⌘5/7** Slideshow / Publish, **⇧⌘C / ⇧⌘V**
    copy / paste settings, **⇧⌘E** export, **⇧⌘I** import, **⌘R** show in
    Finder, **⌘K** keywords, **⇧⌘U** auto white balance, **Space** zoom,
    **Tab** hide panels. G, E, D, P, X, U, 0–5, 6–9, B, L, F, \\, Y and
    ⌘[ ⌘] already matched Lightroom.
  - **Q, K and M** (in Develop), **⇧M, V, C, N, ⌘', ⌘U** and ⌥⌘4/6 explain
    what Laika doesn't have yet, in a brief toast over the workspace,
    instead of doing nothing.
  - Menu titles show the active map, and the menu bar rebuilds when it
    changes. Wall loses its W key under this map; it stays in View.
- **Command palette** (⇧⌘P, Help → Find a Command…) searches every Laika
  command, plus about 190 Lightroom Classic menu, tool and panel names.
  - Each name either runs its Laika command (shown as "Lightroom: …") or
    answers "not yet" with the reason and the backlog item (U11, U19, S08,
    …).
  - Tests check that every Lightroom name reaches its command or its answer,
    and that word-prefix queries ("del rej") work.
- **Guide page** (Help → Laika for Lightroom Users) maps 21 Lightroom
  concepts to Laika, marked *same*, *partly* or *not yet*. It links to Import
  from Lightroom…, Import Presets…, and the last S01 import report.
- **Live test (test app):**
  - The guide rendered, and the switch changed the menus (Crop Tool R, White
    Balance Picker W, Library ⌥⌘1).
  - R opened and closed crop; ⌥⌘2 opened Develop; Q showed the Spot Removal
    toast.
  - ⇧⌘P "virt" answered Create Virtual Copy → not yet (S08); "grid" + Enter
    ran Grid. The map persisted across a restart.
- **Still open:**
  - The U23 task script with real Lightroom users.
  - Surfacing the Lightroom names in the native Help-menu search (only the
    palette knows them).
  - Lightroom's view-specific keys (T toolbar, H, Lightroom's O/⇧O variants).

### [ ] S05 — Rendering difference report

- **Deliver:** For migrated photos, render Laika's result and compare it with
  Lightroom's output. The reference is either a user-provided export folder or
  the embedded 1:1 preview in `Previews.lrdata` where present. Report per-photo
  ΔE2000 (mean and 95th percentile) and a heat-map overlay in Loupe, and sort
  the migrated set worst-first. Suggest the cause when it can be attributed
  (profile, lens correction, masks, unsupported panel).
- **Done when:** On a 500-photo migrated shoot the report completes in under ten
  minutes, flags every photo with masks or profiles, and the photographer can
  jump from a flagged photo to its approximate settings in one click.
- **Why switch:** reasons 1 and 3. It says honestly where edits won't look the
  same, instead of letting the user discover it on client work.
- **Depends on:** S01, U20.

## Section B — Own everything, forever

### [ ] S06 — Documented catalog and edit format with a reference renderer

- **Deliver:**
  - Publish `docs/format/` with the SQLite schema per version, the meaning
    of every develop parameter (units, ranges, order of operations), and the
    XMP mapping, with a stability promise: formats only gain fields, and old
    versions stay readable.
  - Ship `laika-render`, a small CLI in the release bundle that renders any
    photo from a catalog or sidecar to TIFF/JPEG without the GUI.
  - Golden-image tests pin the renderer.
- **Done when:**
  - A catalog written by every released schema version renders identically
    with the current `laika-render` (within tolerance).
  - A third party can write a new-version-compatible reader from the docs alone
    (validated by a reviewer who has not read the source).
- **Why switch:** reason 3. "Your edits are readable without us" is the
  anti-lock-in promise.
- **Depends on:** V08, S18 (shares the CLI crate).

**Progress 2026-09-18: format contract and reference renderer shipped; independent-doc review still open.**

- `docs/format/` now documents the additive compatibility promise, schema
  versions 0–13, durable tables and edit JSON, all 70 develop parameters,
  geometry and pipeline order, and the complete XMP mapping.
- The new `laika-render` workspace binary opens catalogs read-only (it never
  migrates them), accepts photo IDs or exact paths, reads explicit or adjacent
  sidecars, and renders full-resolution JPEG/TIFF through the same RAW decoder,
  GPU pipeline, compatibility layer, and encoders as the app.
- The macOS bundle and release workflow build and include `laika-render`.
- Tests cover legacy edit widths, unknown additive JSON fields, panel bypasses,
  read-only access for every schema version, CLI validation, and the existing
  tolerance-based RAW/raster GPU goldens. A live 6016×4016 NEF-to-JPEG render
  through an XMP edit passed on Metal.
- Still open: have a reviewer who has not read the source implement a reader
  from `docs/format/` and record any ambiguities they find.

### [ ] S07 — Leave-Laika export bundle

- **Deliver:** File → Export Everything writes a folder containing:
  - an originals manifest (paths, hashes, and capture data), with optional
    copies;
  - an XMP sidecar for every photo, using Adobe-compatible `crs:` names wherever
    they map;
  - collections and albums as JSON and as folders of Finder aliases;
  - the metadata as CSV;
  - optional rendered JPEGs;
  - a README explaining how to bring it into Lightroom, darktable, Capture One,
    and Apple Photos.
- **Done when:** The bundle of a 20k-photo catalog imports into Lightroom with
  ratings, labels, keywords, and supported edits intact (verified on fixtures),
  and into darktable with ratings and keywords intact. It is resumable and
  verifies every hash it writes.
- **Why switch:** reason 3. Nobody trusts an exit they can't see.
- **Depends on:** V10 (catalog export), S06.

**Progress 2026-09-19: product workflow shipped; external 20k-app verification remains.**

- File → Export Everything now produces a stable, resumable bundle with an
  integrity-checked catalog snapshot, per-photo Adobe-compatible XMP (including
  preserved foreign fields), originals manifest, metadata CSV, collection JSON,
  ordered Finder-visible aliases, and importer-specific README.
- Optional originals are copied atomically and checked against their catalog
  BLAKE3 values; adjacent XMP copies make the `Originals/` tree directly
  importable. Optional developed JPEGs use the normal full-resolution renderer.
- The progress dialog supports pause/resume. Every regular output is re-hashed,
  the manifest is pinned by `VERIFY.txt`, and completion stays false for missing,
  changed, failed, paused, or not-yet-rendered files.
- Automated fixtures cover XMP metadata and edit mapping, foreign-field
  preservation, album order, original copying, pause/resume/reuse, rendered-set
  finalization, and tamper detection. The acceptance run through Lightroom and
  darktable with the specified 20k-photo catalog is still outstanding.

### [ ] S08 — Virtual copies and version diff

- **Deliver:** Virtual copies were deferred from U14 because a copy needs its
  own catalog row, edits, history, and sidecar naming. This item delivers them:
  - create, rename, and delete copies, with a master/copy badge and grouping
    in a stack;
  - independent ratings and collections per copy;
  - a develop-settings diff between any two copies or snapshots (a
    parameter-level list plus a side-by-side view);
  - export naming that includes the copy name;
  - a sidecar convention that round-trips with S01/S07.
- **Done when:** Three copies of one RAW (color, B&W, crop) survive restart,
  export independently, import from Lightroom virtual copies, and deleting a
  copy never touches the original file.
- **Why switch:** reason 1. A migration that flattens virtual copies loses work.
- **Depends on:** U13 (stacks), U14.

## Section C — Faster than Lightroom, and provably so

### [ ] S09 — Published speed benchmark against Lightroom Classic

- **Deliver:** A scripted, reproducible benchmark on a public test shoot
  (2,000 NEF + 500 CR3 + 500 HEIC under a permissive license). It measures:
  - import to first usable grid;
  - time to 1:1 preview;
  - cull latency (next photo plus rating, p50 and p95);
  - Basic slider drag latency;
  - export of 500 full-size JPEGs;
  - catalog open time at 100k photos.

  Laika's side runs in CI on a fixed Apple silicon runner and fails the build on
  a >10% regression. Lightroom's side is a documented manual protocol re-run
  each release on the same machine. Results go in the README with the hardware
  and versions stated.
- **Done when:** Two independent people reproduce Laika's numbers within 10%.
  The README states wins *and* losses, with no cherry-picked metrics.
- **Why switch:** reason 2. "Faster" is only persuasive when measured.
- **Depends on:** U21, V09.

### [ ] S10 — Cull before import finishes

- **Deliver:** Point Laika at a card or folder and start culling at once, from
  embedded JPEG previews, before any copy or hashing completes. Ratings, flags,
  labels, and rejects made during this pass carry into the import. Rejects can be
  excluded from the copy entirely. The review grid from the SD-scan work
  becomes a full culling surface (Loupe, 100% zoom on the embedded preview,
  keyboard rating) with a clear "not yet imported" state.
- **Done when:** On a full 64 GB card, the first photo is shown within 2 s of
  mount. Culling 1,000 photos never waits on I/O. The resulting import copies
  only keepers when asked, and applies every rating exactly once.
- **Why switch:** reason 2. This is the Photo Mechanic step that Lightroom
  users currently pay for separately.
- **Depends on:** V01, U06, the scan fingerprint cache (`source_fingerprints`).

**Progress 2026-09-19: built and tested on a 48-NEF test card; not yet timed on a full 64 GB card.**

- **Culling in the review** (`cull_ui.rs`): the SD-card review grid is now a
  culling surface.
  - Click to select, arrows to move (↑↓ by row), **0–5** rate, **P / X / U**
    flag, **6–9** label, **A** auto-advance.
  - Double-click or **E** opens a Loupe; **Z**, Space or a click toggles
    100% at that point; **G** or Esc returns to the grid. Enter never
    starts the import from inside the Loupe.
  - Cells show ⚑ / ✕ / stars / label, rejects dim, and the Loupe header says
    **NOT IMPORTED YET**.
- **Images come straight from the camera's embedded JPEG**
  (`preview::cull_image`), never a RAW decode.
  - The Loupe loads the smallest embedded JPEG at least 2048 px (capped at
    2560 px), plus ±3 neighbours prefetched nearest-first; 12 are cached
    around the cursor.
  - 100% uses the largest embedded JPEG, which is full size on Nikon.
- **Import:**
  - "Leave rejected photos on the card" (on by default) and "Import picked
    photos only"; the Import button count follows them.
  - Marks survive closing and reopening the dialog. Each photo's marks are
    applied as it's inserted (`Prepared.source` maps a copy back to its card
    file), then consumed, so every rating lands once.
  - Sidecars are written.
- **Live test:** a disk-image card with 48 NEFs.
  - Scan took 0.9 s.
  - 5★ pick, reject, 3★ green, reject, 4★ pick set in grid and Loupe; no
    Loupe image took more than 400 ms while stepping through.
  - The import brought in 46 at 426 MB/s. The two rejects stayed on the
    card, and every mark matched in the catalog.
- **Tests:** `cull_images_come_from_the_camera_jpeg` (≥ 2048 px for the
  Loupe, largest for 100%, no RAW decode). The rest was verified live.
- **Still open:**
  - The 64 GB / 1,000-photo timings: first photo within 2 s of mount, and
    never waiting on I/O across 1,000 photos.
  - Panning at 100% (it's click-to-center today).
  - A larger, dedicated culling window: the Loupe shares the review dialog
    with the options column.
  - Culling from a plain folder uses the same path but wasn't tried.

### [ ] S11 — Never wait: a performance budget per interaction

- **Deliver:** A tracked budget table (like U21's gates) extended to every
  switching-critical interaction. Examples:
  - opening a 100k catalog in ≤ 2 s;
  - search results in ≤ 150 ms;
  - switching modules in ≤ 100 ms;
  - opening Map with 50k geotagged photos in ≤ 300 ms.

  Add an in-app "slow frame" recorder that captures the job and view names
  whenever a frame exceeds 100 ms, feeding the diagnostics log (V32). Fix every
  budget breach found on the 100k fixture.
- **Done when:** The 100k fixture meets every budget on an M1 base machine, and
  the recorder shows no frame over 250 ms during the S09 cull script.
- **Why switch:** reason 2.
- **Depends on:** U21, V32, S09.

## Section D — Private, on-device intelligence

### [ ] S12 — Culling assistant

- **Deliver:** For each photo, compute on-device:
  - a focus/sharpness score (at the detected subject, else the center);
  - a closed-eye / blink flag for faces;
  - a motion-blur estimate;
  - highlight/shadow clipping;
  - burst grouping by time and visual similarity, with a suggested best frame.

  Results show as filterable badges ("soft", "eyes closed", "best of 6"), never
  as automatic rejects. Scores are explainable: hovering shows the measured
  value and the region used. Scoring runs in the background at import and uses
  no network.
- **Done when:**
  - On a labeled 2,000-photo event set, "soft" flags have ≥ 90% precision.
  - Best-of-burst matches the photographer's pick in ≥ 70% of bursts.
  - Scoring 2,000 photos takes ≤ 5 minutes on M1 without blocking culling.
- **Why switch:** reasons 2 and 3. Lightroom has no culling assist, and cloud
  culling tools upload your photos.
- **Depends on:** U06, U11, V05. **Relation:** U27 lists AI features as
  candidates; this promotes one with measurable criteria.

### [ ] S13 — Natural-language search, on device

- **Deliver:** Compute image embeddings locally with a CLIP-class model via
  Core ML/ONNX, downloaded once after opt-in and verified by checksum. Search
  queries like "dog on the beach at sunset" and combine them with existing
  filters (camera, date, rating, place). An index-status line shows progress and
  size. "Find similar" is available from any photo. Nothing leaves the machine.
- **Done when:**
  - Indexing 20k photos takes ≤ 30 minutes on M1 in the background.
  - A query returns in ≤ 300 ms, and the top-20 precision on a published query
    set is ≥ 0.7.
  - Turning the feature off deletes the model and index.
- **Why switch:** reason 2. Finding photos is where catalogs fail over years.
- **Depends on:** U12, S11.

### [ ] S14 — Suggested keywords you approve

- **Deliver:** Keyword suggestions from S13's embeddings, mapped onto the
  user's existing keyword hierarchy first and a small built-in vocabulary
  second. Suggestions appear as ghost chips in the metadata panel, and you can
  accept or reject them one by one or in bulk for a selection. Nothing is
  written without acceptance, and rejected suggestions are remembered per photo.
- **Done when:** On a 1,000-photo set, accepting all suggestions adds no
  keyword the photographer marks as wrong more than 10% of the time, and every
  accepted keyword round-trips through XMP.
- **Why switch:** reason 2.
- **Depends on:** S13, V12.

## Section E — Places (following the Map module)

The Map module now exists on `exp`. It has a local graticule, opt-in Esri tiles,
clusters, date and camera filters, rectangle selection, an EXIF and storage
rail, place/move/remove location, and a sidecar round trip. These items finish
the places story.

### [ ] S15 — Geotag from GPX tracklogs

- **Deliver:** Load one or more GPX/FIT/KML tracks, draw them on the map, and
  match photos by capture time. The user sets a camera clock offset or
  time-zone shift (shared with V14), then previews matched photos and their
  distances from the track before applying. Only photos without a location
  change by default; an option overwrites existing locations too.
- **Done when:** A day of Garmin and phone tracks tags 300 photos from a camera
  that was 1 h 02 m off, with all positions within 20 m of the track and one
  undo step.
- **Why switch:** reason 1. Lightroom users who geotag rely on this.
- **Depends on:** V14, Map module.

### [ ] S16 — Offline place names and place search

- **Deliver:** Bundle a compact reverse-geocoding dataset (GeoNames cities
  ≥ 1,000 population plus admin regions, ≤ 20 MB). It powers:
  - human names in the Map rail's Places list, replacing coordinates;
  - IPTC City/State/Country suggestions for located photos (accept per batch);
  - place-name search in Library ("Kyoto", "Texas").

  No network lookups are made.
- **Done when:** Places shows names for 99% of located photos in the test
  catalog, place search returns within 150 ms, and suggestions never overwrite
  existing IPTC location fields without confirmation.
- **Why switch:** reasons 2 and 3. Lightroom's address lookup sends
  coordinates to an online service.
- **Depends on:** Map module, V12.

### [ ] S17 — Private locations

- **Deliver:** Draw private zones (home, family) on the map. Photos inside a
  zone keep their location in the catalog but have GPS and IPTC location
  stripped from exports, galleries, and shared links automatically. A
  per-export override exists, and zones show on the map as hatched areas.
- **Done when:** Exporting and publishing a mixed set never emits coordinates
  within a zone (verified with exiftool in tests), while photos outside zones
  keep them when the export policy allows.
- **Why switch:** reason 3.
- **Depends on:** V28, G19.

## Section F — Automate and extend

### [ ] S18 — `laika` command-line interface

- **Deliver:** A `laika` CLI, installed from the app, that talks to a catalog
  directly (or through the running app via its lock and socket). It offers
  `import`, `export --preset`, `query` (filters and search to JSON), `apply-preset`,
  `set` (rating, flag, label, keywords, GPS), `render`, `backup`, and `verify`.
  Output is stable JSON, and exit codes are documented.
- **Done when:** Every subcommand has an integration test, runs safely while the
  GUI is open (no lock violations, and the app reflects changes live), and a
  shell loop can tag and export 1,000 photos without touching the GUI.
- **Why switch:** reason 2. Lightroom has no scripting outside its plug-in
  SDK.
- **Depends on:** V07, V28, S06.

### [ ] S19 — Rules and automations

- **Deliver:** A rules editor with triggers, conditions, and actions.
  - Triggers: *imported from card/camera/folder*, *rated/flagged/labeled*,
    *added to collection*, *on schedule*.
  - Conditions: camera, lens, keyword, place, time.
  - Actions: apply preset, add keywords, add to collection, export with
    preset, back up, run a CLI command.

  Each rule has a dry-run preview and an activity log, and every action is
  undoable through history.
- **Done when:**
  - "Imports from the Z8 get the wedding preset and go to *Inbox*" and "5★
    exports to ~/Portfolio" run unattended for a week of test use.
  - Disabled rules never fire, and a failing action reports and retries
    without blocking the queue.
- **Why switch:** reason 2.
- **Depends on:** S18, U15, V03.

### [ ] S20 — Sandboxed extensions

- **Deliver:** A WebAssembly extension API with explicit capabilities
  (network domains, folders). Its first extension points are export
  destinations (e.g. Immich, SmugMug, Flickr, a WebDAV folder) and metadata
  sources. The UI declares its fields; the host renders them. An extension
  never sees originals unless granted. Ship two reference extensions.
- **Done when:** A third-party extension installs from a file, requests and
  receives only its declared capabilities, survives an app update, and cannot
  read outside granted folders (tested).
- **Why switch:** reason 2. It replaces Lightroom's Lua publish services with
  something safer.
- **Depends on:** S18, V28.

## Section G — Every device you own, without a cloud account

### [ ] S21 — One catalog across two Macs

- **Deliver:** Sync a catalog between machines through storage the user already
  has (the U24 S3/SFTP/SMB destinations). It uses an append-only operation log
  with per-field last-writer-wins and an explicit conflict list. Smart previews
  travel so a laptop can edit without the originals, and a clear status shows
  what's pending on each side. Originals move only when asked.
- **Done when:** A laptop in the field and a desktop at home edit disjoint and
  overlapping photos offline for a week. After reconnecting, no edit is lost,
  and every conflict is shown once and resolved explicitly.
- **Why switch:** reason 2. This is Lightroom Classic's single-machine limit
  without Creative Cloud.
- **Depends on:** U24, V09, V10.

### [ ] S22 — Cull from a phone or tablet on your network

- **Deliver:** Laika serves a small web app over the LAN, paired with a QR code
  and a short-lived token and using TLS with a pinned self-signed certificate.
  It shows smart previews with swipe rating, flags, labels, and keywords. Changes
  sync back live, and the feature turns off automatically when the app is idle.
  No relay, no account.
- **Done when:** An iPad on the same Wi-Fi culls 500 photos with changes
  visible on the Mac within 1 s. An unpaired device can't reach any photo.
  Disabling the feature closes the port (tested).
- **Why switch:** reasons 2 and 3. Mobile culling without uploading a shoot.
- **Depends on:** V09, S18.

## Section H — Install without fear

### [ ] S23 — Signed, notarized, self-updating releases

- **Deliver:** Developer ID signing, notarization, and stapling in
  `release.yml`, with secrets in GitHub environments. Add an update check
  against a signed appcast: opt-in, showing release notes, with delta or full
  downloads, verified signatures, and one-click rollback to the previous
  version. Remove the right-click → Open instruction from the README, and decide
  on a universal or Apple silicon-only build.
- **Done when:** A fresh Mac opens the downloaded DMG with no Gatekeeper
  warning. The app updates from N-1 to N and rolls back while preserving the
  catalog. A tampered appcast or binary is refused.
- **Why switch:** reason 4.
- **Depends on:** V32.

### [ ] S24 — License, pricing, and an honest comparison page

- **Deliver:**
  - Choose and add a license (none exists in the repository).
  - State the pricing model plainly.
  - Add a "Laika and Lightroom" page to the README and docs: a feature table
    with ✓ / partial / ✗ from this backlog's status, the S09 benchmark, the
    migration steps, and what you give up.
  - Keep the page generated from a checked-in data file so it can't drift.
- **Done when:** Every ✓ on the page links to a passing test or a verified
  release note, and the page updates in the same PR that changes a feature's
  status.
- **Why switch:** reasons 3 and 4.
- **Depends on:** S09.

### [ ] S25 — Linux build

- **Deliver:** Build and package for Linux (AppImage first, then Flatpak) using
  GPUI's Linux backend. Handle XDG paths (already sketched in `plan.md`), use a
  portal-based file chooser, and document Vulkan requirements for wgpu. Apple
  Photos features are hidden there. Nightly Linux builds run in CI (fixing the
  current `nasm` failure in `ci.yml`).
- **Done when:** Ubuntu LTS and Fedora testers import, cull, develop, export,
  and back up the test shoot, and the release page carries a Linux artifact.
- **Why switch:** reason 4. Lightroom does not run on Linux at all.
- **Depends on:** S23 (release pipeline), U22.

### [ ] S26 — No telemetry, and proof of it

- **Deliver:** A written network policy: Laika connects only to destinations
  the user configured (backup, deploy, extensions) and to opt-in services (map
  tiles, update check, model download). A Network Activity panel lists every
  connection made this session and why. An automated egress test runs the app's
  workflows under a proxy and fails on any unexpected host.
- **Done when:** The egress test is in CI. With all opt-ins off, a full
  import → edit → export session makes zero network connections, and the panel
  shows each opt-in connection when it is enabled.
- **Why switch:** reasons 3 and 4.
- **Depends on:** V32.

## Section I — Remove the deal-breakers

These are parity gaps that stop a switch outright. Local adjustments and healing
(U19) and color management (U20) remain in `backlog.md`, and S24's comparison
page must show their status plainly until they ship.

### [ ] S27 — Automatic lens profile corrections

- **Deliver:** Look up lensfun profiles (bundled database, updatable) from EXIF
  make/model/lens/focal/aperture. Apply distortion, vignetting, and TCA with a
  per-photo on/off, amount sliders, and a manual profile picker for unmatched
  lenses. Record the profile in the sidecar. This closes the "no lens profiles"
  limit recorded on U18.
- **Done when:** The 50 most common lenses in the fixture set match
  automatically. Corrections render identically in preview and export, and
  unmatched lenses say so instead of silently doing nothing.
- **Why switch:** reason 1. Lightroom users expect corrections to be on by
  default.
- **Depends on:** U18, U20.

### [ ] S28 — HDR editing and HDR export

- **Deliver:** Edit in extended range on XDR/HDR displays with a visible HDR
  headroom indicator and an SDR preview toggle. Export ISO gain-map JPEG and
  HDR AVIF alongside SDR, and let the gallery builder serve HDR where browsers
  support it.
- **Done when:** An HDR export displays correctly in Safari and macOS Photos,
  falls back to the SDR base elsewhere, and the SDR rendition matches Laika's
  SDR export.
- **Why switch:** reason 1. HDR is now expected from a modern raw developer.
- **Depends on:** U20, V27, G19.

### [ ] S29 — Camera support promise and fallback

- **Deliver:** A published support matrix generated from rawler's list and
  Laika's fixtures, with a "new camera" process: report form → sample upload
  (opt-in, to a location the user chooses) → tracked issue. For unsupported
  raws, fall back to the embedded JPEG with a clear banner, instead of failing
  to import.
- **Done when:** Every camera on the matrix has a fixture that renders in CI,
  and an unsupported raw imports, culls, and exports from its embedded preview
  with the limitation labeled everywhere.
- **Why switch:** reasons 1 and 4. A missing camera is an instant deal-breaker.
- **Depends on:** U20, V05.

### [ ] S30 — Print to paper and PDF contact sheets

- **Deliver:** A minimal print path: single photo and contact-sheet layouts,
  page size and margins, a sharpening-for-print amount, a color-managed print
  through the macOS print dialog, and PDF output of contact sheets for clients.
- **Done when:** A single photo and a 4×5 contact sheet print on A4 and Letter
  with correct margins and color from a soft-proofed profile, and a PDF sheet
  shows filenames and ratings.
- **Why switch:** reason 1. Occasional printing shouldn't require keeping
  Lightroom around.
- **Depends on:** U20 (soft proofing). **Relation:** promotes "printing" from
  U27 with a deliberately small scope.

## Release gates and validation

- **Switch drill (gate for Milestones A–C):** Five Lightroom Classic users each
  migrate their own catalog (≥ 10k photos) using only S01, S02, S04, and the
  README. They then work one real shoot end to end in Laika and export it.
  - Pass: four of five finish without facilitator help.
  - No one loses a rating, keyword, collection, or original.
  - Every setting that did not transfer appears in the report.
- **Exit drill (gate for Milestone B):** Take a catalog built only in Laika,
  export it with S07, and import it into Lightroom and darktable. Ratings,
  keywords, labels, and supported edits must arrive intact.
- **Fresh-Mac install (gate for Milestone C):** Download from the release page
  on a clean macOS install, with no Gatekeeper prompt, a working update, and
  zero network egress with opt-ins off.
- **Benchmark gate (Milestone D onward):** S09 runs on every release tag.
  Regressions over 10% block the tag unless waived in the release notes.
- **Privacy gate (all milestones):** S26's egress test passes. Any new network
  feature is opt-in and appears in the Network Activity panel.
- **Honesty gate (all milestones):** the S24 comparison page is regenerated.
  No item is marked done without its "Done when" verified on a real build, as
  in earlier backlogs.

## Benchmark references

- Lightroom Classic catalog (`.lrcat`, SQLite) and `Previews.lrdata`, plus
  Adobe Camera Raw `crs:` XMP and preset formats (`.xmp`, legacy
  `.lrtemplate`).
- Lightroom Classic's Map module (tracklogs, saved locations, address lookup)
  as the places baseline.
- Photo Mechanic's ingest-and-cull-first workflow as the S10 baseline.
- darktable and Capture One as exit targets for S07.
- lensfun for S27, GeoNames for S16, and ISO 21496-1 gain maps for S28.
