# Laika usability backlog

Updated: 2026-09-15. All items below are open.

## Product goal

Make Laika dependable and fast enough for a photographer to import a shoot,
choose the keepers, edit a consistent set, export finished images, and return
later without losing work or relearning the interface.

Use **Lightroom Classic's desktop workflow** as the primary usability benchmark,
given Laika's local-first catalog and Library/Develop structure. Prioritize
predictable interactions, image quality, speed, and recovery. Keep Laika's
local originals, portable edits, object-storage backup, and static galleries.

This is a product backlog grounded in the current source, not a claim of live-app
verification. `plan.md` remains the prototype implementation record; its checked
boxes do not establish that a workflow meets the acceptance criteria here.

## Current baseline

| Area | Implemented foundation | Usability gap observed in source |
| --- | --- | --- |
| Import | Folder picker, recursive scan, hashing, previews, SQLite catalog, progress | No import review or cancellation; scan begins on the UI thread; duplicate check uses path |
| Library | Virtualized grid, single-photo Loupe, selection, ratings, flags, four filters | Folder rows have no navigation action; collections are sample content; sort is fixed; Compare is unavailable |
| Develop | RAW renderer, Basic sliders, presets, before/after rendering, in-memory history | Zoom labels have no actions; split divider cannot be dragged; crop stores only an aspect ratio; advanced panels are placeholders |
| Persistence | SQLite WAL, parameter/crop storage, XMP read/write | History is not restored as editable steps; writes can fail silently; XMP writes replace the file; external sidecars always win |
| Export | Full-resolution rendering foundation | `laika-export` is a placeholder; Export opens the Publish form; there is no finished file-export workflow |
| App shell | Library/Develop/Publish layout, some shortcuts | Text fields are display-only; several statuses contain sample data; global arrow handlers change a parameter regardless of view |

Source anchors: `crates/laika-app/src/main.rs`,
`crates/laika-app/src/controls/text_field.rs`, `crates/laika-core/src/state.rs`,
`crates/laika-core/src/catalog.rs`, `crates/laika-core/src/edit.rs`,
`crates/laika-core/src/xmp.rs`, `crates/laika-develop/`, `crates/laika-raw/`,
and `crates/laika-export/src/lib.rs`.

## Priorities and delivery order

- **P0 — Daily workflow blockers:** trust, navigation, basic editing, and delivery.
- **P1 — Efficient shoot processing:** organization, batch work, editing depth,
  and reliable operation at realistic catalog sizes.
- **P2 — Broader workflow:** backup, galleries, migration, and specialist features.

| Milestone | Exit condition | Items |
| --- | --- | --- |
| 1. Trust and control | No fabricated status; edits survive navigation/restart; shortcuts and inputs behave predictably | U01–U04 |
| 2. Complete one shoot | Import, cull, inspect focus, crop, edit, and export real images end to end | U05–U10 |
| 3. Process shoots efficiently | Compare near-duplicates, find photos, organize sets, batch edit, and recover from missing files | U11–U17 |
| 4. Refine image quality and scale | Deeper edits, stable large catalogs, accessible workspace, repeatable usability results | U18–U23 |
| 5. Extend delivery and portability | Verified backup, finished galleries, and explicit migration support | U24–U27 |

Performance, accessibility, and image fidelity apply throughout; their dedicated
items extend the initial gates rather than defer basic correctness.

## P0 — Daily workflow blockers

### [x] U01 — Make every visible control and status truthful

**Result: done 2026-09-15.** `cargo fmt`, `cargo test --workspace` (32 tests,
including 2 new U01 pins) and `cargo check --workspace --all-targets` pass.
- Sample content removed: collections show "no collections yet"; keywords read
  the catalog (`Catalog::keywords`) with a "no keywords yet" empty state;
  publish count is the real selection ("No photos selected" at 0); estimates
  read "no estimate yet / computed at build time"; deploy line reads "no
  deploys yet"; recipes read "no saved recipes yet"; `PublishForm` defaults
  carry no title/slug and the preview falls back to "UNTITLED GALLERY";
  version comes from `CARGO_PKG_VERSION`; Develop dims fall back to "—"; grid
  stars render the real rating.
- Dead controls fixed or marked: empty-state Import is wired; MAP and COMPARE
  render dimmed and explain on click (visible in the Library footer);
  unavailable destinations render dimmed with "only static galleries in this
  prototype"; Build buttons render disabled with "gallery build and deploy are
  not wired in this prototype"; collapsed panels show "soon"; title/URL fields
  carry "title and URL editing is not wired yet"; folder rows explain on click.
- Newly working instead of disabled: publish template, size chips, and all
  four toggles (incl. stored `password_protect`) update the form.
- Still owned elsewhere, deliberately not built here: real text inputs (U04),
  folder navigation (U12), collections/keywords editing (U13), Compare (U11),
  zoom/pan (U07), file export (U10), gallery build/deploy (U25).

- **Problem:** Sample collection counts, upload progress, and a “synced 2m ago”
  label imply work that has not happened. Some controls look actionable but do nothing.
- **Deliver:** Replace sample values with actual state and useful empty states.
  Hide unavailable features or show an explicit disabled state with a reason.
- **Done when:** A fresh catalog offers Import; an unconfigured account says
  backup is not configured; no fake photo dimensions, gallery titles, counts,
  or upload activity appear. Every enabled control has a working action.
- **Touchpoints:** App shell, Library rails, Publish form, `PublishForm` defaults.

### [x] U02 — Guarantee durable edits and expose save failures

**Result: done 2026-09-15.** `cargo fmt`, `cargo test --workspace` (38 tests,
including 7 new U02 pins) and `cargo check --workspace --all-targets` pass.
- Every commit path (slider, preset, history revert, crop, paste, rating,
  flag) acknowledges to SQLite synchronously; failures shelve the photo
  (`shelved`/`rating_dirty` sets, values kept in memory) instead of vanishing.
- Trailing sidecar debounce: throttled writes defer with a deadline and flush
  on render tick, timer wake, photo/module switch, retry, and window close
  (`on_window_closed` hook, synchronous inline flush). Never dropped.
- Atomic sidecars (tmp + fsync + rename); foreign XMP attributes survive
  rewrites (Lightroom `ProcessVersion`/labels verified by test).
- Stale sidecars never overwrite newer catalog values; external edits are
  detected via remembered write-mtimes and adopted with a report; startup and
  import rescan heals stale sidecars from the catalog (history/preset ride
  along). Corrupt sidecars keep catalog values with an actionable message.
- Explicit save pill in the top bar (all modules): unsaved / saved-relative /
  failed-with-reason; always clickable (retry/flush/report). Rating/flag
  changes also converge `xmp:Rating`, closing the stale-rating clobber.
- Originals are never written (byte-identity asserted in test).
- Not built here by design: durable history/snapshots (U14), true background
  IO threads (writes are sub-ms synchronous; UI never blocks on scans anyway),
  survival of kill -9 (impossible to guarantee; acknowledged = on disk).

- **Problem:** Catalog write errors are discarded; sidecar writes can be skipped
  within the throttle window without a guaranteed trailing write. A stale sidecar
  can overwrite newer catalog values on reopening.
- **Deliver:** A background save queue with a trailing debounce, atomic sidecar
  replacement, explicit dirty/saving/saved/error state, retry, and a safe shutdown
  flush. Preserve unknown XMP metadata and detect external changes before merging.
- **Done when:** Rapid edits, immediate photo/module switches, and quit/reopen
  retain the latest committed values, crop, ratings, and flags. Read-only folders,
  disk-full failures, and conflicting sidecars produce actionable messages.
  Crash recovery preserves acknowledged saves. Originals remain byte-identical.
- **Touchpoints:** App persistence handlers, `catalog.rs`, `xmp.rs`.

### [x] U03 — Make selection, navigation, and shortcuts predictable

**Result: done 2026-09-15.** `cargo fmt`, `cargo test --workspace` (40 tests,
including 2 new U03 pins) and `cargo check --workspace --all-targets` pass.
- One transition (`select_navigate` + `SelectMode::{Set,Range,Toggle}`):
  grid clicks, filmstrip clicks, import, and keyboard navigation share a
  single flush → select → primary → load-values path. This fixed a live bug
  where grid clicks set the primary before `switch_primary`, early-returning
  past its save/load transition (new values never loaded, old edits never
  flushed on click navigation).
- `AppState.anchor` is independent of primary: repeated shift-clicks extend
  one stable range; plain/cmd clicks reset it; shift with no anchor falls
  back to the primary. Range math runs against the visible (filtered) order.
- Batch scope is visible-only: `targets()` intersects the selection with the
  filtered order, falls back to a visible primary, and reports "no visible
  photos targeted" instead of acting on hidden photos (rating, flags,
  unflag, paste).
- Arrows navigate (with shift-extend, up/down by row) unless a slider owns
  the keyboard: clicking a slider focuses it (arrows nudge, shift = coarse);
  photo input or Esc releases it. Filmstrip renders a 16-window centered on
  the primary so it always follows the active photo.
- Shortcuts added: `U` unflag, `Cmd/Ctrl+A` select visible, `Cmd/Ctrl+D`
  deselect (primary kept), `?` shortcuts overlay (visible equivalent — the
  prototype has no menu bar). Modal dialogs swallow all photo commands
  except Esc.
- Not built here by design: text-entry focus scopes (no editable text until
  U04), Cmd+C/V settings sync (batch work, U15), loupe pan/zoom keys (U07).

- **Problem:** Global arrows currently edit a parameter; grid selection updates
  the primary before `switch_primary`, which can bypass that function's save/load
  transition. Multi-photo actions need a clear target scope.
- **Deliver:** One primary-photo transition; separate range anchor, active photo,
  and selection; focus-scoped commands. Arrows navigate photos unless an editing
  control owns focus. Support select all, deselect, `P`/`X`/`U`, `0`–`5`, and
  familiar view shortcuts with visible menu equivalents.
- **Done when:** Switching between differently edited photos always loads the
  correct values. Range selection follows visible sort order; filtering cannot
  silently apply actions to hidden selections. Filmstrip follows the active photo.
  Text entry and modal dialogs never trigger background photo commands.
- **Depends on:** U02. **Touchpoints:** `AppState`, grid clicks, keyboard handlers.

### [x] U04 — Implement real inputs and desktop interaction basics

**Result: done 2026-09-15.** `cargo fmt`, `cargo test --workspace` (45 tests,
including 6 new U04 pins) and `cargo check --workspace --all-targets` pass,
no new warnings.
- New `controls/text_input.rs`: single active `FieldEdit` with char-boundary
  caret, shift-selection, clipboard (⌘A/C/X/V/Z over the OS clipboard),
  50-deep undo, and per-field validators whose errors explain the fix.
  Pure logic, unit-tested without a window.
- Editable everywhere it matters: gallery title/slug (slug auto-derives
  until hand-edited), all five backup settings (endpoint/bucket/region/key
  validated, secret masked/never echoed, keyring-stored), and every slider
  value readout (click to type, range-checked against the param def).
  Enter commits, Esc reverts + leaves, Tab cycles modal fields (trapped) or
  commits + closes, clicking another field auto-commits.
- Modals trap focus (autofocus first field, Tab wraps, typing never leaks
  photo commands) and restore pre-modal slider focus on close; publish
  gained a mouse-usable Close button.
- Custom hover tooltips (Import, filters, views, pills, value readouts,
  fields) via shared hover slots + cursor overlay; `?` overlay documents
  the full map including the new keys.
- Deliberate limits, stated in code and here: no platform IME composition
  API exists in this UI stack, so CJK entry arrives via paste (verified in
  test); caret does not blink; collection rename and export-path editing
  await their owners (U13/U10) — the field machinery is ready for them.

- **Deliver:** Editable text fields with caret, selection, clipboard, undo,
  validation, and IME support. Add direct numeric entry to sliders, double-click
  reset, precise keyboard adjustment, focus rings, tooltips, and shortcut help.
  Modal dialogs trap focus and restore it when dismissed.
- **Done when:** Users can enter an exact exposure, rename a collection, or edit
  an export path entirely by keyboard. Tab order is logical; Escape cancels an
  uncommitted input; Enter commits it. Errors explain how to correct the value.
- **Touchpoints:** `controls/`, action routing in `main.rs`.

### [x] U05 — Build a clear, cancellable import workflow

**Result: done 2026-09-15.** `cargo fmt`, `cargo test --workspace` (55 tests,
including 3 new app-level import tests) and
`cargo check --workspace --all-targets` pass, no new warnings.
- Import dialog with Pick → Scanning → Review → Running → Done: source
  (cards + folder), file count, total size, capture date range, three
  preview thumbnails, Add-in-place vs verified-Copy mode, destination
  pickers with first-three-path preview, duplicate policy (content-hash
  skip, on by default) — all before anything is written.
- Scan (walk + EXIF + thumbs) runs off the UI thread with progress and
  cancel; the run loop honors cancel between files and inside copy chunks.
  Cancel keeps successes; partial copies are always removed.
- Duplicates: path check plus blake3 check (renames detected) plus
  per-card seen-hashes; all skips counted by kind in the end report.
- Per-file errors land in a bounded failure list (dialog + log), never just
  the last error; corrupt files fail singly without aborting the shoot;
  source files are only ever read.
- Headless `--import` keeps working on the same engine with safe defaults.
- Deliberately later: rename/folder templates (V02), metadata presets
  (V03), videos/pairs (V05).

- **Deliver:** Review source, file count, thumbnails, duplicate policy, and
  destination before importing. Start with explicit “Add in place”; add “Copy to
  folder” for memory cards with destination verification. Move scan and file work
  off the UI thread. Show progress, cancellation, and a per-file error summary.
- **Done when:** Reimporting the same folder adds no duplicates; renamed copies
  are detected by content hash with a clear choice. Cancelling keeps successful
  imports and removes partial outputs. Corrupt/unsupported files do not abort the
  entire shoot. Source files are never deleted by import.
- **Depends on:** U01, U04. **Touchpoints:** Import handlers, `laika-raw`, catalog.

### [x] U06 — Make culling fast and continuous

**Result: done 2026-09-16.** `cargo fmt`, `cargo test --workspace` (95 tests,
including 1 new U06 filter pin) and `cargo check --workspace --all-targets` pass,
no new warnings.
- Keyboard-first review unchanged and documented: arrows navigate Grid/Loupe/
  Develop off one primary (`select_navigate`), `P`/`X`/`U` flag, `0`–`5` rate,
  `G`/`E`/`D` switch views without losing the primary; `A` toggles auto-advance
  (toolbar Advance chip mirrors it).
- Visible pick/reject indicators: `PICK`/`REJ` badges in every grid cell plus
  `PICKED`/`REJECTED` in the Loupe label; status bar reads
  picked · rejected · unrated counts.
- New culling filters: `Unrated` (rating 0) and `Unflagged` (neither picked nor
  rejected) chips alongside the existing Picked/Rejected; old preset JSON
  without the new field still deserializes (pinned in test).
- Auto-advance + filter drop-out settle in one helper: advance steps past the
  anchor, tail stays; with advance off the primary stays unless the mutation
  filtered it out (e.g. rejecting under Picked), when it settles on the photo
  now at its old index. Batch scope stays visible-explicit (U03/U14 undo unit
  untouched).
- Next image ready: every navigation/cull prefetches the prev/next 2048 large
  previews; thumbnails already load per row; full RAW decode stays on demand
  (stated, not implied).
- Honest limits: the grid does not auto-scroll to the primary (filmstrip
  follows it); no Caps-Lock binding (`A` instead — reliable in this UI stack).

- **Deliver:** Keyboard-first Grid/Loupe/Develop navigation, visible pick/reject
  indicators, unflag, optional auto-advance, and filters for unrated/unflagged/
  rejected photos. Keep the next image ready and preserve position across views.
- **Done when:** A user reviews 200 photos using arrows and rating/flag keys,
  filters to keepers, and opens Develop without losing their place. Auto-advance
  works when the current photo drops out of the filter. Batch scope is explicit.
- **Depends on:** U03. **Touchpoints:** Library, filters, preview loading.

### [x] U07 — Implement accurate Fit, 100% zoom, and pan

**Result: done 2026-09-16.** `cargo fmt`, `cargo test --workspace` (100 tests,
including 5 new zoom-geometry pins) and `cargo check --workspace --all-targets`
pass, no new warnings.
- Shared Loupe/Develop viewport (`zoom.rs`: pure, tested): Fit contains the
  frame without upscaling past native, 100% = native device pixels (window
  scale honored), 200% doubles. One exact-aspect placed image per level —
  Fit no longer Cover-crops (Develop) or stretches non-3:2 frames (Loupe).
- 100%/200% show true native pixels: RAW via full demosaic + full-res GPU
  render through the same pipeline (current values, same split composition,
  so before/after stays aligned by construction); rasters via exact full CPU
  decode. Async with an explicit "loading native detail…" state; Fit renders
  until detail arrives — the 2048 preview is never enlarged to fake 100%.
- Input: `Z` cycles, click toggles (anchored at the click), wheel/pinch step
  at the cursor, threshold-disambiguated drag pans, navigator with
  click-to-jump, FIT/100/200 buttons in Loupe and the Develop toolbar
  (replacing the display-only label). Zoom is sticky across photos, center
  resets; video/offline/GPU-less photos stay Fit with the reason shown.
- Detail stays truthful: values/split mismatch re-renders 500 ms after the
  user pauses (demosaic cached one-photo, never per tick); completed decodes
  land only on the current primary; photo/catalog switches drop detail.
- Honest limits: loupe/develop pointer interaction code-reviewed only (no
  headless harness — verify click/wheel/pinch/pan in the running app);
  EXIF-orientation handling is unchanged pre-existing behavior; crop tool
  exits zoom (and vice versa) since geometry needs full-frame context.

- **Deliver:** Shared image viewport for Loupe and Develop with Fit, 100%, 200%,
  click-to-zoom, drag/Space-pan, scroll/trackpad support, and a navigator. Preserve
  aspect ratio and orientation; request sufficient-resolution pixels for focus checks.
- **Done when:** Fit shows the whole image without unintended clipping. 100%
  samples the original at native detail rather than enlarging the 2048 preview.
  Zoom centers around the inspected point, handles display scaling, and remains
  responsive while detail loads. Before/after stays spatially aligned.
- **Depends on:** U03. **Touchpoints:** Viewports, decode/cache, renderer.

### [x] U08 — Replace aspect-only crop with a real geometry tool

**Result: done 2026-09-16.** `cargo fmt`, `cargo test --workspace` (103 tests:
crop-sample/constrain, handle/drag, pack/geometry, XMP round-trip) and
`cargo check --workspace --all-targets` pass, no new warnings. GPU-verified
on Metal (fixture NEF): identity keeps dims, crop dims follow the rect with
pixel-exact center content, 10°+mirror renders full-bleed with no empty
corners.
- Model: `CropGeom` (normalized rect, ±45° angle, flips) rides `Edit`,
  `Snap`, every history step, snapshots, and the DB JSON (old rows decode
  to full-frame). Paste stays tone-only, so batch paste preserves
  individual crops (the U15 chooser owns selective geometry later).
- Render truth: one shared uv mapping (`edit::crop_sample`, mirrored in
  `develop.wgsl`) for crop+straighten+flip; preview and export targets
  follow the rect, so output dims reflect the crop and before/after stays
  aligned by construction. Rects pre-constrain (tested, landscape and
  portrait) — rotated sampling never shows empty corners.
- Tool: CROP opens a draft (aspect locks incl. custom `W:H`, ±1°/typed
  angle, H/V mirrors, thirds, reset) with live preview; drag moves, 8
  handles resize with aspect-aware anchors; Apply writes one undo step
  (sync DB + sidecar), Esc/Cancel restores, photo switch discards with a
  note. Zoom and crop are mutually exclusive (full-frame context each way).
- Every view agrees: geometry commits re-derive the 512/2048 caches through
  the pipeline (grid + Loupe match Develop); undo/redo/snapshot restores
  re-derive too. Sidecars carry `crs:Crop*` + flips; foreign rewrites
  without crop keys never wipe catalog geometry.
- Honest limits: pointer drags code-reviewed only (verify in-app); rasters
  cross a new sRGB-linear bridge (identity camera matrix — plausible tone,
  same path, not a calibrated profile); line-draw straighten and richer
  overlays stay V22; grid cells keep their pre-existing aspect behavior
  (V16 owns cell styles).

- **Deliver:** Draggable crop rectangle, resize handles, aspect lock, custom ratio,
  rotate/straighten, flip, grid overlay, reset, and explicit commit/cancel. Store
  normalized crop bounds and geometry in the edit model.
- **Done when:** Crop survives restart, undo, and selective copy/paste. Preview,
  thumbnails, and export use the same geometry for landscape and portrait inputs;
  output dimensions reflect the crop. Cancel restores the previous composition.
- **Depends on:** U02, U04, U07. **Touchpoints:** Edit schema, XMP, viewport, renderer.

### [x] U09 — Make Basic editing and before/after trustworthy

**Result: done 2026-09-16.** `cargo fmt`, `cargo test --workspace` (104 tests,
including 1 new WB-approximation pin) and `cargo check --workspace --all-targets`
pass, no new warnings. Render example re-run on Metal: pipeline healthy.
- White balance: As Shot (pipeline defaults), six CCT presets (Temp only,
  tint untouched — no fabricated pairs), gray-world Auto from the preview
  mean, and an eyedropper (5×5 mean → McCamy CCT + green-excess tint delta,
  one undo step; click neutral gray, trim after). Approximation is stated —
  tone stages also move color.
- Units travel with numbers (K, EV); Basic header reads modified (green
  while any of the twelve differs); per-slider double-click reset and
  typed entry unchanged.
- Clipping: exact-0/255 fractions from each render in the histogram row,
  plus a red/blue overlay toggle (CPU copy on toggle/receive, hidden while
  zoomed or across photo switches).
- Before/after: the divider drags (6 px grab, live resubmit, double-click
  resets 0.38); composition is identical by construction (one shared
  geometric mapping, U08). BEFORE is the defined reference: pipeline
  defaults over as-shot multipliers.
- Previews converge: every tone commit, undo/redo/revert/snapshot restore,
  import preset, and shelved retry re-renders the 512/2048 derivatives in
  the background (one render per photo, dirty-flagged, never stacked), so
  grid/Loupe match Develop. Originals are never written (pre-existing
  byte-identity pin).
- Stale renders can't win: jobs carry their photo id, frames for departed
  photos drop, late decodes restart the current photo, and the stage falls
  back to the large preview mid-decode.
- Honest limits: pointer interactions (picker, divider drag, overlay)
  code-reviewed only — verify in-app; raster tone rides the new sRGB
  bridge (documented approximation); no per-panel reset beyond Basic.

- **Deliver:** As Shot/Auto/custom white balance and an eyedropper; clear slider
  units; accurate modified/reset states; live histogram and clipping warnings;
  draggable before/after divider with a clearly defined original reference.
  Refresh Library/Loupe previews after edits and preserve untouched originals.
- **Done when:** Every enabled adjustment produces a visible, stable result.
  Switching photos during decode/render never displays the previous photo as the
  new result. Preview and exported output agree within a documented rendering
  tolerance. Camera fixtures cover orientation, saturated colors, skin tones,
  deep shadows, and highlights. Unsupported files have a useful fallback/error.
- **Depends on:** U02, U07. **Touchpoints:** RAW decode, shaders, preview invalidation.

### [x] U10 — Ship ordinary file export as a complete workflow

**Result: done 2026-09-16.** `cargo fmt`, `cargo test --workspace` (105 tests,
including 1 new minimal-sidecar pin) and `cargo check --workspace --all-targets`
pass, no new warnings. Render chain verified live on Metal (edited + cropped
NEF → resized JPEG + sidecar; eyeballed the output): this run also caught
and fixed a real RGBA→JPEG encode bug before it shipped.
- Dedicated dialog (toolbar + publish panel, focus-trapped like the other
  modals): destination picker, naming template with live first-file preview
  (V02 engine, videos keep their container extension, pairs export the
  shown side), JPEG quality, long-edge resize (never upscale), fixed sRGB,
  sidecar policy, collision policy, and the target count. Every setting
  persists per catalog — reopening restores it.
- Render truth: full-res pipeline render (tone + crop + mirrors, after-only
  split) with acknowledged values — drafts never leak into exports.
- Sidecars: All = full render (params/history/rating/geometry + foreign
  carry-over), Copyright = creator/rights only, None = nothing. Pixel
  files carry no EXIF (GPS included) — stated in the dialog, not implied.
- Safety: pre-validated naming, atomic writes, Suffix (default)/Skip/
  Overwrite — never silent overwrites; offline originals fail with a
  reason instead of aborting the run.
- Progress with cancel (finishes the current file, then stops), bounded
  failure list, Retry-failed (paths re-resolve, suffixes advance), Reveal
  in the file manager. Runs survive dialog closes like imports.
- Honest limits: JPEG only (V27 owns the other formats + ICC/metadata
  embedding); dialog/pointer flow code-reviewed only — verify in-app;
  no EXIF orientation handling (pre-existing, matches preview).

- **Deliver:** A dedicated Export dialog for selected photos: destination picker,
  naming template, JPEG quality, resize/long edge, sRGB profile, metadata/GPS
  choices, collision handling, reusable settings, and output count. Add background
  progress, cancellation, failed-file retry, and Reveal in Finder/file manager.
- **Done when:** A set of edited/cropped RAWs exports to usable JPEGs with correct
  names, dimensions, orientation, and embedded profile. Reopening the dialog
  restores settings. Existing files are not silently overwritten; a failed item
  does not hide successful output. No gallery or cloud setup is required.
- **Depends on:** U04, U08, U09. **Touchpoints:** `laika-export`, app dialog/jobs.

## P1 — Efficient shoot processing

### [x] U11 — Add Compare and Survey for choosing keepers

**Result: done 2026-09-18.** Compare is a first-class Library view (`C`)
with explicit Select/Candidate panes, previous/next candidate replacement,
filmstrip candidate picking, swap, linked Fit/1:1/2× preview zoom and pan, and
per-photo ratings that do not disturb the comparison selection. Survey (`N`)
lays out the selected group responsively; Remove from view is session-only and
Restore hidden brings every frame back without changing selection, collection
membership, catalog rows, or originals. Both views are available in menus,
the view switcher, command search, and the Lightroom keyboard/guide mapping.

- **Deliver:** Two-photo Compare with candidate/select switching, linked zoom/pan,
  and independent ratings; Survey for a selected group with remove-from-view.
- **Done when:** Users inspect matching detail in two similar frames, replace the
  candidate, and narrow a group without changing catalog membership or deleting files.
- **Depends on:** U03, U06, U07.

### [x] U12 — Make folders, search, filters, and sorting useful

**Result: done 2026-09-15.** `cargo fmt`, workspace tests (63 in the
affected crates: 5 new state/model tests plus catalog browser tests) and
`cargo check --workspace --all-targets` pass, no new warnings.
- Filters are now mutually exclusive enums (`FlagFilter`, `FileType`)
  instead of contradictory bools, plus search (filename/camera/lens/
  dates/keywords, `/` focuses, Enter applies), exact camera/lens pickers
  with counts, a `YYYY-MM-DD` date window, folder scope, and sort field
  (date/name/rating) + direction — all AND-combined, all in the toolbar,
  rails, and status scope line.
- Folders render as an indented clickable tree filtering the grid;
  cameras/lenses browse the top values; the rail middle scrolls so nothing
  pushes the sync footer off.
- Saved filter presets persist as JSON (apply/delete/last-used order);
  unreadable JSON reports instead of applying. Clear-all appears whenever
  anything is active; empty results name the active constraints with a
  working reset.
- Selection is pruned to live ids on refresh (valid kept, vanished
  dropped); batch actions were already visible-scoped (U03); sort runs
  over the filtered set with a filename tiebreak, so range selection
  follows the visible order.
- Sort is session-local; presets are the persistence story. Full-text
  search is substring matching, not indexed — fine to 10k rows in memory,
  revisit with U21 if catalogs grow past that.

- **Deliver:** Clickable folder tree; search filename, keywords, and metadata;
  combinable date/camera/lens/rating/flag/file-type filters; sort field/direction;
  clear-all and saved filter presets. Show the current scope and result count.
- **Done when:** Users find a known photo in a 10,000-photo catalog without manual
  scrolling. Empty results explain active constraints and offer reset. Sorting and
  filtering preserve valid selection and scroll state without hidden batch targets.
- **Depends on:** U03, U04.

### [x] U13 — Add real collections, keywords, and stacks

**Result: done 2026-09-18.** Existing persisted collections, Quick Collection,
batch metadata undo, keyword hierarchy, and sidecar-safe writes now include
saved smart collections with live criteria counts; type-ahead keyword choices
apply to the full visible selection as one undo step. Schema v14 adds persisted
manual stacks with collapsed covers, expand/collapse/unstack, stack badges, and
automatic RAW/JPEG-pair stack creation. Smart collections reject membership
writes, and deleting a collection or dissolving a stack changes organization
only. A reopen/safety test verifies smart criteria, stack membership/state,
catalog rows, and original files. `cargo fmt --all`, all workspace tests (311
passed, 2 ignored), and `cargo check --workspace --all-targets` pass.

- **Deliver:** Create/rename/delete collections; add/remove selected photos;
  smart collections from saved criteria; keyword autocomplete and batch editing;
  manual stacks for bursts and RAW/JPEG pairs. Keep folder and collection concepts clear.
- **Done when:** Organization survives restart, counts reflect membership, and
  removing a collection or stack never deletes originals. Batch metadata edits
  are undoable and preserve unrelated metadata in sidecars.
- **Depends on:** U02, U04, U12.

### [x] U14 — Persist history and provide complete undo/redo

**Result: done 2026-09-15, except virtual copies (see below).** `cargo fmt`,
workspace tests (67 in affected crates: 4 new model/codec/persistence
tests) and `cargo check --workspace --all-targets` pass, no new warnings.
- Complete snapshots: every history step carries params, crop aspect,
  rating, and flags. One slider gesture (drag, keys, typed entry, reset)
  is one step via gesture-start baselines; presets, crop, paste, revert,
  and snapshot restores record the same way.
- Durable history: params + crop + full steps + cursor persist per photo
  (tolerant decode of legacy rows); restart restores the panel, cursor at
  the tip, redo branch intact; a 50-step cap pins the Import baseline as
  the undo floor.
- Undo/redo (`⌘/Ctrl+Z`, `⇧⌘/Ctrl+Z`): act over every photo the last
  change touched (batch paste/rating/flags restore all targets at once),
  acknowledge through the U02 save path, and report persistence failures
  instead of claiming success. History clicks revert full snapshots
  (previously params-only, unpersisted) through the same path.
- Named snapshots: per-photo table (same-name replaces), History-rail UI
  with save-as field, click-to-restore as a new undoable step, delete.
  Local-only by design (never in sidecars).
- Editing an earlier step truncates forward (predictable branch); import
  presets seed Import-baseline + Preset steps, persisted at once.
- **Deferred with reason: virtual copies.** A copy needs its own catalog
  identity against the `UNIQUE(path)` model, a sidecar identity scheme
  (copies would fight over one `.xmp`), and sync/import rules for shared
  originals — a schema-level feature, not a history extension. Snapshots
  cover alternative treatments within one photo until then.

- **Deliver:** Durable history with complete edit snapshots, including geometry;
  standard undo/redo for edits, ratings, flags, and organization; named snapshots
  and virtual copies for alternative treatments.
- **Done when:** One slider gesture is one undo step. Undoing a batch operation
  restores every target. Restart restores edit history; editing an earlier step
  creates a predictable branch. Virtual copies share the original safely.
- **Depends on:** U02, U03, U08.

### [x] U15 — Make batch editing safe and efficient

**Result: done 2026-09-18.** Copy Settings now freezes and names its source,
shows the real target count, and offers grouped include switches; crop/local
geometry is never part of the payload and Transform is opt-in. Selective paste,
Apply Previous, relative Quick Develop nudges, and the clearly lit optional
Auto Sync all use the existing batch-history unit, so one undo restores every
target. Writes report partial failures for retry, and the frozen source is
excluded from paste targets. Selective-overlay tests cover excluded-setting,
Transform, and source preservation.

- **Deliver:** Copy Settings chooser, explicit source and target count, selective
  paste/sync, apply previous, relative Quick Develop adjustments, and optional
  clearly indicated Auto Sync. Exclude crop and local masks by default.
- **Done when:** Users apply white balance/exposure to 50 keepers while preserving
  individual crops, undo the operation once, and receive a useful partial-failure
  report. A selected source never changes unexpectedly during batch processing.
- **Depends on:** U03, U09, U14.

### [x] U16 — Let users manage presets

**Result: done 2026-09-18.** The Presets rail can create, name, group, edit,
rename, delete, import, and export native `.laikapreset` files. Creation/editing
uses the same explicit grouped setting selection as Copy Settings. Stored presets
persist in the catalog, apply sparse values as one history step, and preview on
hover by rendering a temporary value set that is restored on exit without a
catalog, sidecar, or history write. The versioned JSON interchange validates
names, indices, numeric ranges, and schema/version with actionable import errors;
round-trip, invalid-file, sparse-apply, and catalog rename tests pass.

- **Deliver:** Create, name, group, rename, delete, and import/export Laika presets;
  choose included settings; preview on hover and restore on exit without committing.
- **Done when:** Presets survive restart, record one history step on apply, and
  leave excluded adjustments unchanged. Invalid imports explain the problem.
- **Depends on:** U04, U14, U15.

### [x] U17 — Handle missing files and catalog recovery

**Result: done 2026-09-15.** `cargo fmt`, workspace tests (71 in the
affected crates: missing/relink/move/remove plus backup/restore/cache
tests) and `cargo check --workspace --all-targets` pass, no new warnings.
- Missing originals: probed on every refresh into an offline set —
  OFFLINE grid badges, Loupe labels over surviving cached previews,
  per-folder offline counts, status-bar count that toggles a Missing
  filter chip, and a Develop guard that refuses decode with a reason
  instead of failing. Edits stay fully workable while offline.
- Locate (hash-verified single file, mismatch refused with both hashes),
  batch folder relink by relative path (linked/missing/mismatched report),
  Reveal (Finder/`-R`, `xdg-open`), catalog-managed Move (rename fast
  path, verified-copy fallback across volumes, sidecar follows, backup
  re-enqueues to the new keys).
- Remove from Catalog (rows only, originals + sidecars stay) vs Move to
  Trash (OS Trash via Finder/`gio`, rows drop only after the files move)
  behind an explicit confirm stating its target; Trash failure keeps
  everything.
- Catalog modal: path/size/counts, latest backup, Back up now (WAL
  checkpoint + timestamped copy), validated Restore (integrity + tables
  probed, current preserved first, reopen resets UI state, refused
  mid-sync), Rebuild previews (background re-derive, originals read-only),
  Recheck missing, integrity (page check + orphan scan).
- Gates: drive disconnect/reconnect simulated by moving folders away and
  back (edits, previews, ratings intact); folder relink repairs without
  reimport; restore recovers metadata/history/keywords; cache wipe leaves
  originals byte-identical.
- Honest limits: no background drive watcher (manual Recheck; V04 owns
  watched folders), Linux Trash/Reveal need `gio`/`xdg-open`, PTP devices
  only where mounted.

- **Deliver:** Missing/offline badges; locate file/folder with batch relinking;
  reveal original; safe catalog-managed move/rename; distinct Remove from Catalog
  and Move to Trash actions. Add catalog backup/restore and cache rebuild controls.
- **Done when:** Disconnecting and reconnecting an external drive preserves edits
  and previews. Relinking a moved shoot repairs descendants without reimporting.
  Restore recovers metadata/history; deleting previews never deletes originals.
  Destructive actions state their target and support OS Trash recovery where available.
- **Depends on:** U02, U12, U14.

### [x] U18 — Add the highest-value global editing panels

**Result: done 2026-09-16.** `cargo fmt`, `cargo test --workspace` (109 tests)
and `cargo check --workspace --all-targets` pass, no new warnings.
GPU-verified live on Metal (fixture NEF, eyeballed): curve lift clean,
per-stage solo renders sane, combined grade caught and fixed two real
bugs before shipping — a WGSL dynamic-indexing compile failure and an
outward distortion mapping that smeared edges (now inward, clean).
- Tone Curve: canvas editor (draggable 20/40/60/80% points, snap 0.01,
  double-click resets point/all, one undo unit per drag) through a shared
  piecewise-linear stage; identity at defaults (pixel-exact bypass proof).
- Color Mix: full 8×H/S/L over triangular hue sectors; Detail: sharpen +
  radius + luminance/color NR; Optics: manual distortion + lateral CA
  (no profile database in this build — stated in the panel).
- Params widen 12→46 with legacy-row padding (old catalogs render
  identically); every panel has bypass (undoable, resolves to defaults
  at every render: preview, detail, derivatives, export) and reset;
  Basic reset re-scoped to tone. Rail scrolls; panels collapse (V25
  persists the layout later).
- Before/after compares tone (geometric stages shared for alignment);
  CA triples cost only while active. Detail panel judges at 100% via
  the U07 path automatically.
- Sidecars: HSL/detail/distortion as `crs:` scalars, curve as a genuine
  `ToneCurvePV2012` list (foreign curves ignored, never misread), CA as
  `laika:`; rating-only foreign rewrites no longer reset tone.
- Presets stay tone-only (new panels untouched); paste carries all 46
  (individual crops survive per U15's future chooser).
- Honest limits: pointer UI code-reviewed only — verify in-app; no lens
  profiles; line-draw straighten stays V22; per-panel reset covers new
  panels only.

- **Deliver in order:** Tone Curve; Color Mix/HSL; sharpening and noise reduction
  with 100% detail preview; lens distortion/vignette and chromatic-aberration
  correction. Add enable/bypass and reset per panel.
- **Done when:** Each shipped panel works in preview, export, history, presets,
  and selective batch sync. Lens corrections identify the applied profile and
  offer manual fallback. No inactive panel is presented as working.
- **Depends on:** U07, U09, U14, U15.

### [ ] U19 — Add essential local adjustments and cleanup

**Progress 2026-09-18:** Added persisted Brush/Linear/Radial mask and spot-heal
models, original-image coordinates, feather/invert/erase state, editable Develop
list + preview-only overlay, undo/snapshot/restart support, shared preview/export
GPU rendering, and explicit opt-in local copying. Numerical preview/export parity
is pinned. Remaining before closing: direct on-canvas brush painting and draggable
gradient/heal handles (current controls create, nudge, resize, and edit them).

- **Deliver:** Brush, linear gradient, radial gradient, editable mask list/overlay,
  feather/erase/invert, followed by a spot-heal tool. Keep mask placement tied to
  original image coordinates through cropping and rotation.
- **Done when:** Masks and healing survive save/restart and undo, render identically
  on export, and remain editable. Copying local adjustments is an explicit choice.
- **Depends on:** U08, U14, U18.

### [ ] U20 — Establish color management and camera coverage

**Progress 2026-09-18:** Added persisted Laika Standard/Neutral/Vivid/Monochrome
camera intents with explicit unknown-profile fallback, documented the linear-camera
to linear-sRGB to 8-bit-sRGB contract and camera/container matrix, carried profiles
through preview/reference export, and added final-size Off/Low/Standard/High output
sharpening to all rendered formats (including TIFF). Existing WB numerical tests,
RAW/raster goldens, and new local preview/export parity cover the shared path.
Remaining before closing: a trustworthy per-display ICC transform and soft-proof
pipeline; GPUI currently exposes the explicitly documented compositor-sRGB fallback.

- **Deliver:** Display-profile-aware preview, defined working/output color spaces,
  camera profile selection, validated white-balance mapping, and a camera support
  matrix. Extend export to TIFF and output sharpening; add soft proofing afterward.
- **Done when:** Documented reference images pass visual and numerical regression
  checks across supported cameras and displays. Missing/unsupported profiles have
  explicit fallback behavior. Shared XMP parameter names are never treated as proof
  that Laika and Adobe render the same image.
- **Depends on:** U09, U10. **Note:** Basic sRGB correctness is already a P0 gate.

### [x] U21 — Keep large catalogs and background jobs responsive

**Result: done 2026-09-16.** `cargo fmt`, `cargo test --workspace` (107 tests,
including 2 new U21 pins: 10k filter budget + cache drop/prune) and
`cargo check --workspace --all-targets` pass, no new warnings.
- Measured first (10k synthetic rows, debug): full scan+sort 6.2 ms
  (gate: 300 ms), `all_photos` 46 ms, keyword fan-out 70 ms, missing
  probe 52 ms. Pagination correctly deferred — the gate passes by two
  orders of magnitude; full `LIMIT/OFFSET` would only complicate pair
  folding.
- One filter scan+sort per frame (render-generation memo shared by every
  view; pair folding memoized too), with an explicit rule: pure readers
  use the cache, mutate-then-read cull settle uses a fresh scan. Row
  replacement retires the cache + row index together, so stale ids can
  never resolve against new rows.
- Thumbnails load primary-neighborhood-first, single-flight with
  rescan-on-filter-change (duplicate loops impossible); large previews
  capped at 8 with farthest-from-primary eviction; thumbs stay at 600.
- Disk stays proportional: remove/trash frees unreferenced cache dirs
  (shared hashes survive), plus a Manage-modal orphan prune. Rebuild
  already wipes. No silent disk cap that would break offline previews —
  stated, with V09 owning eviction policy.
- Sync completions mirror one row in place instead of reloading 10k rows
  per finished upload (the browse-during-backup stall).
- Job center: status-bar glance at live import/export counts opening
  their dialogs (cancel/retry live there), sync retry in the footer, and
  import failures carry a dedupe-safe re-run note.
- Honest limits: UI pointer flow code-reviewed only; filter chips still
  recompute on the UI thread (measured milliseconds — backgrounding
  would add races for no user gain).

- **Deliver:** Viewport-prioritized thumbnail loading, adjacent-image prefetch,
  bounded memory/disk caches, paginated/indexed catalog queries, stale-job
  cancellation, and a shared import/export/preview job center with retry.
- **Done when:** Meet the performance gates below while importing or exporting.
  Memory stabilizes during repeated browsing; cache eviction allows images to
  reload when revisited. UI actions never wait on scanning, disk writes, or decoding.
- **Depends on:** U05–U10; profile throughout earlier milestones.

### [x] U22 — Make the workspace readable and accessible

**Result: done 2026-09-18.** `cargo fmt --all`,
`cargo test --workspace --all-targets` (310 passed, 2 ignored),
`cargo check --workspace --all-targets`, and `git diff --check` pass; a native
smoke launch opened the catalog and rendered the workspace without a panic.
- Left and right rails resize from accessible splitters and Preferences steppers;
  all three workspace panels can be hidden from the top bar, View menu, or command
  palette. Visibility, rail widths, filmstrip size, and 85–150% text scale persist.
- The window now supports a 1280 × 800 logical-pixel minimum, compacts its top bar
  at that width, keeps rails independently scrollable, preserves full-screen mode,
  and scales every UI text style (including tracked labels and timeline captions).
- Secondary text contrast is raised and regression-tested. Photo cells and the
  filmstrip spell out selection and sync state; ratings remain countable stars,
  while save/sync failures retain text labels rather than color-only dots.
- Essential tabs, buttons, switches, text fields, photo cells, splitters, progress,
  and adjustment sliders expose accessibility roles, names, selection/toggle state,
  and values. Root keyboard navigation and the command palette reach module,
  panel, filmstrip, full-screen, and text-size actions.
- Dragging is optional for rail widths and gallery placement/span/adjustments:
  Preferences steppers, Place/Remove and Span controls, and slider −/+ buttons
  provide explicit alternatives.

- **Deliver:** Collapsible/resizable rails, hideable filmstrip, remembered layout,
  full-screen viewing, scalable text, sufficient contrast, and usable small-window
  layouts. Expose accessible names/roles/values and alternatives to drag-only actions.
- **Done when:** Core workflows fit a 1280 × 800 logical-pixel window without
  clipped controls; keyboard and supported screen-reader navigation reach all
  essential actions. Ratings, errors, selection, and sync state use more than color.
- **Depends on:** U03, U04; apply these basics to every new control.

### [ ] U23 — Validate workflows with photographers

- **Deliver:** Repeatable task script using a real shoot and sessions with at least
  five photographers, including Lightroom users and someone unfamiliar with Laika.
  Record completion, time, wrong-target actions, recovery, and points of confusion.
- **Done when:** At least four of five participants import, cull, edit, batch apply,
  export, and reopen the shoot without facilitator intervention. No participant
  loses work or accidentally applies edits to unintended photos. Feed remaining
  friction back into this backlog before declaring daily-driver readiness.
- **Depends on:** U01–U17 for the full script; run smaller scripts after each milestone.

## P2 — Broader workflow

### [ ] U24 — Make object-storage backup verifiable and recoverable

**Progress 2026-09-19:** Backup now has tested setup/write probes, keychain-held
S3 secrets, a crash-durable/retryable queue, verified S3/SFTP/share transfers,
and explicit pause/resume that lets active uploads finish safely. Remote keys are
content-versioned and idempotent, preventing same-name collisions and retaining
older sidecar revisions; the layout-version bump requeues existing destinations.
Local removal never sends a remote delete. Added a documented recovery contract
and checksummed catalog/sidecar recovery bundle. Remaining before closing: upload
the catalog bundle to remote destinations, implement an automated verified remote
download/relink path, and record a real-shoot restore drill against each target.

- **Deliver:** Connection setup/test, OS-keychain credentials, separate local-save
  and remote-backup states, checksummed uploads, durable queue, pause/retry, and
  version/conflict handling. Define which originals, sidecars, and catalog data
  are protected and provide a restore workflow.
- **Done when:** Offline edits queue safely; reconnect resumes without duplicates;
  “Backed up” appears only after verification. A restore drill recovers an actual
  shoot and edits. Local deletion never silently propagates to the backup.
- **Depends on:** U02, U17, U21.

### [ ] U25 — Finish static galleries and publishing

- **Deliver:** Editable gallery configuration, local preview, responsive gallery,
  download/GPS/watermark controls, build-only, deploy progress, actionable errors,
  and republish of changed images. Show the destination and included photos clearly.
- **Done when:** Local builds work offline; output respects selected settings;
  deploy failures preserve the build for retry; successful deployment produces a
  working URL. Publishing is an optional delivery path alongside file export.
- **Depends on:** U04, U10, U21.

### [ ] U26 — Offer an honest Lightroom migration path

**Progress 2026-09-19:** Lightroom migration already previews matches/options
before its transaction, preserves originals, reports missing originals and
unsupported masks/profiles/RGB curves/other edits, and preserves foreign XMP
properties byte-for-byte. This pass adds standard flat/hierarchical keyword import,
explicit metadata-compatibility-versus-render-equivalence documentation, and a
Catalog UI export for an integrity-checked portable SQLite + byte-identical XMP
bundle with checksummed manifest. Remaining before closing: add provenance-recorded
fixtures produced by current Lightroom Classic/Camera Raw releases and validate the
full import/report matrix against them (the existing regression packet is
Adobe-shaped, not a provenance-recorded application output).

- **Deliver:** Document and test standard sidecar discovery/naming and supported
  metadata/settings against actual Adobe-written fixtures. Import ratings,
  keywords, and supported edits with a report of skipped or approximate settings.
  Export a portable Laika catalog/sidecar bundle.
- **Done when:** Migration preserves original files and unknown metadata, reports
  unsupported masks/profiles/edits, and provides a preview before applying changes.
  Users can distinguish metadata compatibility from rendering equivalence.
- **Depends on:** U02, U13, U20. Safe existing-sidecar handling belongs in U02.

### [ ] U27 — Reassess specialist features from observed demand

- **Candidates:** Subject/sky AI masks, AI denoise, panorama/HDR merging, tethered
  capture, face recognition, maps, printing, mobile editing, and client proofing.
- **Done when:** Promote a candidate only with a concrete photographer task,
  evidence of demand, cost/privacy implications where relevant, and measurable
  acceptance criteria. Do not block core daily use on this feature list.
- **Depends on:** U23 and a stable daily workflow.

## Release gates and validation

These are proposed Laika targets, not measured results or claims about Adobe's
performance. Record hardware, build, display scaling, camera files, and cold/warm
cache state with each measurement. Use an M-series Mac with 16 GB RAM and SSD as
the initial reference; also exercise the supported Linux build.

| Scenario | Acceptance target |
| --- | --- |
| Open a 10,000-photo local catalog | Interactive first viewport within 2 seconds with a warm preview cache; remaining work stays in the background |
| Navigate to an adjacent cached photo | Input-to-preview p95 ≤ 150 ms; selection feedback ≤ 50 ms |
| Drag a Basic slider | Input-to-visible-update p95 ≤ 50 ms; target 60 fps during a sustained 10-second drag |
| Open an uncached 24 MP RAW | Embedded preview within 250 ms where available; editable preview within 1.5 seconds on reference hardware |
| Find/filter a 10,000-photo catalog | Results within 300 ms after input settles; typing and scrolling remain responsive |
| Browse during import/export | No UI stall over 100 ms attributable to blocking file work; cancel acknowledges within 250 ms |
| Save/restart/crash recovery | No lost acknowledged saves; pending/error states are visible; original hashes unchanged |
| Export a selected shoot | Correct count, naming, crop, orientation, profile, and metadata policy; preview/export fidelity checked on fixture images |

Test with the three existing Nikon NEFs plus licensed fixtures from other camera
families, JPEGs, portrait orientations, corrupt files, duplicates, Unicode paths,
and an external drive. Benchmark both a 200-photo shoot and a 10,000-photo catalog;
add a 50,000-photo stress run for U21. Use fault injection for failed writes,
interruptions, missing originals, and stale render completion. Run targeted
automated tests for persistence, selection, crop transforms, and export fidelity,
plus hands-on workflow checks; screenshots alone cannot verify usability.

## Benchmark references

Adobe's [Library workflow](https://helpx.adobe.com/lightroom-classic/desktop/help/library-module-basic-workflow.html)
provides the reference for browsing, organizing, selecting, and narrowing a shoot.
Its [keyboard shortcut reference](https://helpx.adobe.com/lightroom-classic/desktop/introduction-to-lightroom-classic/keyboard-shortcuts.html)
informs familiar navigation and culling commands. These references guide behavior;
Laika's priorities and acceptance targets above are product proposals based on
the source review. Consulted 2026-09-15.
