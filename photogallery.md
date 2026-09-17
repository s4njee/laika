# Laika photo gallery builder — epics and stories

Updated: 2026-09-15. All items below are open.

## Scope of this document

`design_handoff_gallery_builder/` specifies a **WYSIWYG photo gallery builder**:
a three-pane editor (library tray, live grid canvas, contextual inspector), a
layout/template picker, a theme and typography panel, and the published gallery
a visitor sees. Laika already promises static HTML galleries (`plan.md` Phase 5,
`backlog.md` U25, `backlogv2.md` V29/V30) but today `laika-export` is a five-line
placeholder and the Publish module renders the Library grid with a modal form.

This document turns the handoff into epics and stories that fit the existing
crates, controls, and backlog conventions. Where a story replaces or narrows
an existing backlog item it says so. Numbering is G01–G28. Format, priorities,
and release gates follow `backlog.md`.

This is a product backlog grounded in the current source and the handoff, not a
claim of live-app verification.

Design reference: `design_handoff_gallery_builder/README.md` (tokens, dimensions,
interaction spec) and `Gallery Builder Mockups.dc.html` (views `1a`–`1d` are the
core dark flow; `2a`/`2b` are exploration only and are not scheduled here).

## Relationship to the existing backlogs

| Existing item | What happens to it |
| --- | --- |
| `plan.md` Phase 5 (Publish modal, command log, askama masonry build, wrangler deploy) | Superseded by G04 (editor replaces the modal), G19–G20 (build), G23 (deploy). The command-log preview is dropped; the build/deploy log lives in the publish sheet (G23). |
| U25 — Finish static galleries and publishing | Becomes the umbrella for Epics D and E. Its acceptance criteria are carried by G19, G22, G23, G24. |
| V29 — Gallery theme system | The eight handoff **layouts** (G02, G11) are grid templates, not themes. V29's "theme" becomes the Page-tab theme model (G15) plus the output renderer (G20). V29's user theme directory and `minijinja` stay in V29 and depend on G20. |
| V30 — Albums | Gallery membership and order are stored per gallery (G01), independent of collections. When U13/V30 land, "New gallery from collection" (G05) reads album order and captions. |
| V27/V28 — formats, watermarks | G19 emits JPEG only via the U10 render path. AVIF/WebP `<picture>` sources and watermarking arrive through V27/V28; G20 leaves the hooks. |
| U01 — truthful controls | Applies throughout. The handoff's sample content ("48 photos", "Draft · saved 2m ago", "mirakawa.photo") never ships as defaults (G25). |

## Current baseline

| Area | Implemented foundation | Gap this backlog addresses |
| --- | --- | --- |
| Publish module | `Module::Publish` exists; the module tab renders the Library view plus `publish_modal` (title/slug fields, template index, size chips, four toggles); `PublishForm` in `state.rs`; slug derivation is tested | No gallery entity, no per-photo placement, no canvas, no inspector, no build |
| Gallery storage | SQLite catalog with `schema_version` and `run_migrations` | No `galleries` / `gallery_photos` tables; captions and alt text have no home |
| Rendering | `laika_develop::Renderer::render_export` renders full-res with acknowledged crop; U10 export writes resized JPEGs atomically with progress, cancel, retry | No per-gallery derivative sizes, no manifest, no incremental rebuild, no HTML |
| Controls | `text_input.rs` (caret, selection, clipboard, undo, validators), `slider`, `toggle`, `segmented`, `chip`, `modal_shell` (1020px), section headers, hover tooltips, `?` overlay | No drag-and-drop between panes, no resize handles, no grid canvas, no card picker, no swatch control |
| Input | GPUI `on_drag` / `on_drop` / `on_drag_move` / `ExternalPaths` available in the pinned `gpui-pre 0.3.2`; window-level `MouseMoveEvent` already drives crop drag | Nothing uses `on_drag` yet |
| Fonts | IBM Plex Sans 400/500/600, Plex Mono 400/500 embedded from `assets/fonts` | No Plex Sans **Light 300** (the handoff lede and "Light display" pairing need it); no woff2 for the published page |
| Undo | U14 persisted develop history; U04 text-field undo | No undo stack for placement, resize, caption, or template changes |
| Jobs | U21 job center for import/export with cancel and retry | No build or deploy job kind |
| Preview | No embedded web view (no `wry`/`webview` in `Cargo.lock`) | Preview must open in the system browser (G22) |

Source anchors: `crates/laika-app/src/main.rs` (`publish_modal` ~L11993,
`Module::Publish` ~L13608, `start_export` ~L4169, `export_one_file` ~L4315),
`crates/laika-core/src/state.rs` (`PublishForm`), `crates/laika-core/src/catalog.rs`
(`run_migrations`, `photos` schema), `crates/laika-develop/src/lib.rs`
(`render_export`), `crates/laika-export/src/lib.rs`, `crates/laika-app/src/theme.rs`,
`crates/laika-app/src/controls/`.

## Design reconciliation

The handoff is drawn as a standalone web app ("Aperture") with its own dark
palette and a `#080147` accent. Its README instructs implementers to recreate
the designs "using the codebase's established patterns, component library, and
tokens." Applied to Laika:

- **Editor chrome uses Laika tokens** (`theme.rs`): `BG_CHROME`, `BG_PANEL`,
  `BG_CANVAS`, `TEXT_*`, hairlines, and the green accent `#00C227` /
  `#007B12` for selection outlines, snap guides, drop targets, active chips,
  and the Publish button. This resolves the README's "known issue" (the
  `#080147` accent is unreadable on dark chrome) by not using it in chrome.
  The wordmark stays `LAIKA`; the Publish module tab is the entry point.
- **Handoff dimensions are kept**: 248px tray, 300px inspector (348px on the
  Page tab), 34px status bar, 760px page at the default zoom, 12px gutter,
  1020px picker modal (matches `layout::DIALOG_W`). The top bar stays Laika's
  46px bar; the gallery name, status chip, breakpoint control, Preview, and
  Publish live in the Publish module's 40px toolbar row instead of a 58px bar.
- **Published-gallery tokens come from the handoff** and become the default
  theme (G15): page `#121517`, ink `#F2EFE6`, accent `#080147`, accent text on
  dark `#9B93E8`, selection tint `#5B4BC9`, Plex Sans/Mono, type scale from the
  README's table. The theme is data, so the editor canvas renders the same
  values the visitor will see.
- **Type scale**: editor labels use Laika's sizes (11–12.5px); the canvas page
  and the published page use the handoff scale (34px canvas title, 48px theme
  screen, 56px published). The 54px slider readout versus 48px canvas conflict
  in the README is resolved by making the canvas render the slider value (G16).
- **Icons**: text glyphs (`✕`, `+`, `⇧`, `⌘`) as in the rest of Laika.

## Priorities and delivery order

- **P0 — A gallery a photographer can build and open:** model, layout engine,
  editor shell, placement, build, local preview.
- **P1 — Finish the designed surface:** template picker, theme panel, focal
  point, breakpoints, empty and error states, deploy.
- **P2 — Depth and validation:** palette sampling, incremental publish,
  performance gates, accessibility, photographer validation.

| Milestone | Exit condition | Items |
| --- | --- | --- |
| 1. Model and engine | Galleries persist; the layout engine is pure, tested, and reflows deterministically; undo works on the model | G01–G03 |
| 2. Editor core | Create a gallery from a selection, drag photos onto the page, resize, caption, and see it saved | G04–G09, G13 |
| 3. Layouts, theme, states | Switch templates, set type and palette, check phone/tablet, and every empty/error state is honest | G10–G12, G14–G18 |
| 4. Build and publish | Build to disk offline, open the result in a browser, deploy with a working URL, republish only what changed | G19–G24 |
| 5. Trust and validation | No fabricated status, tests cover engine and build, gates met, photographers complete the script | G25–G28 |

Milestone 1 has no UI and can start immediately. Milestone 2 stories G05–G08
touch the same `main.rs` render path and should be sequenced, not parallelized.

## Epic A — Gallery model, layout engine, and history (`laika-core`)

### [ ] G01 — Persist galleries, membership, and per-photo gallery data

- **Problem:** `PublishForm` holds a title, slug, template index, sizes, and
  four booleans in memory. Nothing records which photos are in a gallery, where
  they sit, or what their captions are.
- **Deliver:** A `gallery` module in `laika-core` and two tables added through
  `run_migrations`:
  - `galleries(id, title, subtitle, eyebrow, slug UNIQUE, status 'draft'|'published',
    template_id, columns, gutter, default_ratio, theme_json, sizes_json,
    allow_downloads, strip_gps, site_name, meta_line, created_at, updated_at,
    last_build_at, last_build_dir, last_deploy_at, last_deploy_url)`.
  - `gallery_photos(gallery_id, photo_id, position, col, row, span_x, span_y,
    caption, alt_text, focal_x, focal_y, fit 'fill'|'fit', open_full_size,
    PRIMARY KEY(gallery_id, photo_id))`. `col`/`row` are NULL while unplaced.
  - Typed structs (`Gallery`, `GalleryPhoto`, `Placement`, `Theme`) with
    `serde` for `theme_json`, CRUD on `Catalog`, and a `slug_is_free` check.
  - Deleting a gallery removes rows only; originals, edits, and cache are
    untouched. Removing a photo from the catalog (U17 remove/trash) cascades
    out of galleries and reports the affected gallery names.
  - Migrate `PublishForm` fields into `Gallery`; keep `derive_slug`.
- **Done when:** Create, rename, reslug, reorder, place, caption, and delete
  round-trip through a fresh catalog in tests. Opening a catalog written before
  this migration upgrades without touching `photos`. Two galleries cannot share
  a slug; the error names the other gallery.
- **Depends on:** V07/V08 migration machinery (present as `run_migrations`).
  **Touchpoints:** `catalog.rs`, new `laika-core/src/gallery.rs`, `state.rs`.

### [ ] G02 — Pure grid layout engine with the eight templates

- **Problem:** The canvas, the picker previews, the breakpoint toggle, and the
  HTML renderer all need the same answer to "where does each photo go".
- **Deliver:** A pure, UI-free `layout` submodule:
  - `Template` definitions for the eight handoff layouts: Mixed spans (3 col,
    variable), Square grid (3 col 1:1), Editorial rows (alternating widths),
    Single column, Contact sheet (4 col, tight gutter), Masonry pairs (2 col,
    mixed ratio), Hero + grid (lead image, then 3 col), Filmstrip (horizontal
    rows). Each declares columns, gutter, default ratio, a default-span rule
    by index, a category (Uniform grid / Editorial / Single column / Contact
    sheet), and a miniature diagram spec for the picker card.
  - `flow(photos, template) -> Vec<Placement>`: fills cells in order, skipping
    occupied cells, honoring per-photo span overrides, never overlapping.
  - `reflow_after(change)`: siblings shift when a span changes; the changed
    photo keeps its cell when it fits, else moves to the next fit.
  - `apply_template(existing, template)`: preserves photo order, captions, alt
    text, focal, fit; resets span overrides; returns the new placements.
  - `at_breakpoint(placements, Breakpoint)`: desktop = template columns,
    tablet = min(columns, 2), phone = 1; spans clamp to the column count;
    stored placements are untouched.
  - `snap(x, column_edges, threshold_px)` and `quantize_span(drag_px, cell_px)`
    for resize; `nearest_free_cell(point)` for drop targets; `pixel_size(span)`
    for the size badge at a given page width.
  - Deterministic, no floats in stored cells, and total-order stable.
- **Done when:** Property tests show no overlap and no lost photo for random
  orders, spans, and templates; applying then re-applying a template is
  idempotent; breakpoint reflow never mutates the desktop layout; a 500-photo
  flow runs under 5 ms in debug.
- **Depends on:** G01 types. **Touchpoints:** `laika-core/src/gallery/layout.rs`.

### [ ] G03 — Gallery history: undo/redo and autosave

- **Problem:** U14 gives Develop a persisted history; U04 gives text fields
  local undo. The editor needs one undoable stream across placement, resize,
  caption, alt text, focal, span, theme, and template changes.
- **Deliver:** A `GalleryHistory` of typed commands with inverse, coalescing
  for continuous drags (one entry per drag, per slider gesture, per text
  commit), a 200-entry cap, `⌘Z` / `⇧⌘Z` routing that respects text-field
  focus (a focused field consumes undo first, as U04 already does), and a
  visible "Undo apply layout" affordance after a template switch. Autosave
  debounced at 1.5 s trailing, flushed on module switch, gallery switch, and
  window close through the U02 flush hook, surfaced in the existing top-bar
  save pill as unsaved / saved / failed-with-reason. History is in-memory per
  session (persisting it is out of scope; say so in the `?` overlay).
- **Done when:** Any sequence of 50 random commands undone then redone
  reproduces the same model (test). A failed catalog write shelves the gallery
  in memory, shows the reason, and retries from the pill, like U02.
- **Depends on:** G01, U02, U14 patterns. **Touchpoints:** `gallery.rs`, save
  pill handlers in `main.rs`.

## Epic B — Editor shell and placement (`laika-app`)

### [ ] G04 — Publish module becomes the gallery editor shell

- **Problem:** `Module::Publish` renders the Library and opens a modal. The
  handoff is a full-module, three-pane editor.
- **Deliver:** Replace the Publish module body with `grid-template-rows:
  toolbar 40 / 1fr / status 34` and `grid-template-columns: 248 / 1fr / 300`
  (inspector 348 when the Page tab is active). Toolbar: gallery name (500
  13.5px, click to rename in a `text_input` field), status chip (`Draft` /
  `Saving…` / `Saved 2m ago` / `Published <date>`, real values only),
  breakpoint `segmented` (Desktop / Tablet / Phone), `Preview` (outlined) and
  `Publish` (accent fill) buttons. Status bar: `N of M placed` · `Row r · cell c`
  · right-aligned hint `Hold ⇧ to keep ratio · ⌘Z undo`. A gallery switcher
  in the toolbar lists galleries by `updated_at` with a `New gallery` item.
  Remove `publish_modal` and the "Publish gallery" routing into it; the Library
  right-rail button now runs G05. Minimum window stays 1440×900; the editor is
  desktop-only, as the README states.
- **Done when:** Switching to Publish with no gallery shows the G14 empty
  state, with one shows its editor, and switching galleries flushes autosave.
  No sample text appears anywhere. The layout holds at 1440×900 with no
  overflow into the rails.
- **Depends on:** G01, G03. **Touchpoints:** `main.rs` module routing, `theme.rs`
  `layout` constants, `controls/segmented.rs`.

### [ ] G05 — Create a gallery from a selection or collection; manage galleries

- **Deliver:** "Publish gallery" in the Library right rail and `New gallery`
  in the editor create a draft from the current visible selection (U03's
  `targets()` scope) with the Mixed spans template, all photos unplaced,
  title empty, slug empty. A gallery can also be created from a collection
  once U13 lands (reads V30 order and captions when present). Add photos
  later by selecting in Library and choosing "Add to gallery ›". Rename,
  duplicate, and delete (confirm dialog that states originals are not
  deleted). Video and rejected photos are refused with a status note, not
  silently dropped; RAW+JPEG pairs (V05) contribute the shown side.
- **Done when:** Creating from 200 selected photos takes under 200 ms and
  lands in the editor with the tray showing `200 photos`, `Unplaced 200`.
  Deleting a gallery leaves catalog counts and cache untouched (test).
- **Depends on:** G01, G04, U03. **Touchpoints:** Library rail footer, `main.rs`.

### [ ] G06 — Library tray

- **Deliver:** 248px tray per `1a`: eyebrow `Library` + real count; filter
  chips `All` / `Unplaced N` / `Picks` (Picks = `picked` flag; active chip is
  `TEXT_PRIMARY` fill with dark text); 2-column 1:1 thumbnail grid, 8px gap,
  index badge bottom-left (Mono 500 8px), selected thumbnail with 2px accent
  outline at 1px offset and a 14px accent dot top-right, hover lifts 1px with
  a light border. Thumbnails come from the existing U21 cache (viewport-first
  loading, single-flight). Placed photos show a small placed mark; unplaced
  ones are the drag sources for G08. Bottom import zone: dashed 7px-radius
  box, `Drop photos to import` / `JPEG, RAW · up to 60 MB`; OS file drop
  (`ExternalPaths`) routes into the U05 import dialog with those paths, and
  imported photos join the gallery unplaced. The zone lists only formats
  `laika-raw` decodes (no HEIC claim until it decodes).
- **Done when:** Filters and counts match the model; scrolling 2,000
  thumbnails stays smooth (U21 gate); dropping three NEFs from Finder imports
  them and adds them to the tray.
- **Depends on:** G04, G05, U05, U21. **Touchpoints:** `main.rs` tray render,
  thumbnail cache, import dialog entry.

### [ ] G07 — Canvas page rendering

- **Deliver:** Center pane on `BG_CANVAS`: canvas toolbar (breadcrumb
  `Galleries / <title>`, right `GRID n COL` Mono 10px, `SNAP ON|OFF` toggle,
  zoom readout) and a centered page card (760px at 100%, `theme.page`
  background, padding 38px 42px 0, page shadow `0 2px 20px rgba(0,0,0,.45)`).
  Page content: eyebrow + title row (title 600 34px, year eyebrow right-
  aligned), lede (300 13px/1.6, max 44ch; falls back to 400 until G18 adds
  Light), then the CSS-grid equivalent built from G02 placements: each
  placed photo is an image element from the 2048 preview cropped by `fit`
  and `focal` (object-position math in Rust), captions under tiles when the
  theme says so, corner radius from the theme, an empty-cell drop target
  (accent tint, 1.5px dashed) while dragging. Zoom: fit-to-width default,
  50–200% via `⌘+`/`⌘−`/`⌘0` and the readout. Scroll the page, not the
  canvas chrome. Snap guides: 1px accent lines at column edges at 50%
  opacity, shown only during drag/resize.
- **Done when:** A 48-photo gallery renders at 60 fps while scrolling and
  zooming on the reference Mac; the canvas and the G20 HTML agree on cell
  positions for every template (shared engine, snapshot test on placements).
- **Depends on:** G02, G04, U07 zoom patterns. **Touchpoints:** `main.rs`
  canvas render, `zoom.rs`.

### [ ] G08 — Drag from tray to canvas, move within the page, and drop rules

- **Deliver:** GPUI `on_drag` on tray thumbnails and placed tiles with a
  ghost preview; `on_drag_move` over the page computes `nearest_free_cell`
  and highlights it (accent tint fill, dashed border, `DROP HERE`); `on_drop`
  places at the template's default span or moves the tile and reflows
  siblings; dragging a tile off the page to the tray unplaces it. Snap
  threshold 8px at 100% zoom, scaled with zoom. Esc cancels a drag and
  restores the model. Multi-select drag from the tray places in tray order.
  Every drop is one history entry. Pointer flow uses the same
  `window.on_mouse_event` pattern as crop drag for the resize case in G09.
- **Done when:** Drop never overlaps or loses a photo (engine tests plus a
  UI walkthrough); dragging 20 photos one by one onto a 3-column page fills
  cells left-to-right, top-to-bottom; Esc mid-drag leaves the page unchanged.
- **Depends on:** G02, G06, G07. **Touchpoints:** `main.rs` drag handlers.

### [ ] G09 — Selection, corner handles, resize, and the size badge

- **Deliver:** Click a tile or tray thumbnail to select (2px accent outline
  at 3px offset, four 7px square corner handles, floating size badge above
  the top-left corner reading `2 × 1 · 640 × 320` from `pixel_size`).
  Dragging a handle quantizes span to whole columns/rows live, `⇧` keeps the
  ratio, guides show, siblings reflow on release; status bar shows `Row r ·
  cell c`. Selecting in the tray scrolls the canvas to the tile and vice
  versa; the inspector switches to the Photo tab. Delete/Backspace unplaces
  the selected tile (never removes it from the gallery or catalog); `⌘⌫`
  removes it from the gallery after a status confirmation.
- **Done when:** Resizing `1×1` → `2×2` on a 3-column page moves the
  displaced neighbours predictably (matches engine test); the badge updates
  every frame during a drag; keyboard focus never leaks into photo commands
  when a caption field is active (U03/U04 rules).
- **Depends on:** G07, G08. **Touchpoints:** `main.rs`.

### [ ] G10 — Inspector: Photo tab

- **Deliver:** 300px inspector with tabs Photo / Layout / Page (active tab
  indicator `inset 0 -2px 0 TEXT_PRIMARY`). Photo tab, 18px gap: file row
  (56px thumbnail, filename 500 12.5px, `W × H · size` Mono 11px, real values
  from `photos`); **Span** 4-up group `1×1` / `2×1` / `2×2` / `Full`; **Crop &
  focal point** 96px preview with rule-of-thirds overlay and a draggable 16px
  focal handle storing normalized x/y, plus `Fill` / `Fit` / `Reset`;
  **Caption** and **Alt text** `text_input` fields (alt placeholder "Describe
  this photo for screen readers…", Enter commits, Esc reverts); toggle "Open
  full size on click". With nothing selected the tab shows a short "Select a
  photo on the page or in the library" note. Multi-selection applies span,
  fit, and the toggle to all selected and shows mixed-state captions as
  "— mixed —".
- **Done when:** Focal drag updates the canvas tile's crop live and persists;
  captions typed here appear on the canvas and in the G20 HTML byte-for-byte
  (HTML-escaped); the file row never shows placeholder numbers.
- **Depends on:** G09, U04 fields. **Touchpoints:** `main.rs` inspector,
  `controls/text_input.rs`, new `controls/focal_picker.rs`.

### [ ] G11 — Inspector: Layout tab and the layout/template picker modal

- **Problem:** `1a` designs only the Photo tab; `2a`/`2b` show Columns,
  Gutter, and Ratio on a Layout tab. The picker `1b` is fully designed.
- **Deliver:** Layout tab: current template name with a `Choose layout…`
  button, `Columns` (2–6) and `Gutter` (0–32px) value rows with the U04
  numeric entry, `Default ratio` segmented (`3:2`, `4:5`, `1:1`, `Original`),
  and `Snap to grid` toggle. Picker modal in `modal_shell` (1020px): header
  "Choose a layout" + sub "Your N photos flow into the grid. You can move any
  of them afterwards.", category chips (`All 8`, Uniform grid, Editorial,
  Single column, Contact sheet), 4-column card grid with miniature diagrams
  drawn from each template's diagram spec, `CURRENT` badge, footer note
  "Switching layouts keeps your captions and photo order.", `Cancel` /
  `Apply layout`. Apply runs `apply_template` in one history entry and shows
  "Undo apply layout" in the status bar for 8 s. Focus is trapped like the
  other modals.
- **Done when:** Applying each of the eight templates to the same gallery
  preserves order, captions, alt text, and focal (test); the card diagrams
  match the README descriptions; column/gutter changes reflow the canvas
  live.
- **Depends on:** G02, G03, G07. **Touchpoints:** `main.rs`, `controls/modal.rs`,
  new `controls/template_card.rs`.

### [ ] G12 — Breakpoint preview

- **Deliver:** The toolbar segmented control re-renders the canvas through
  `at_breakpoint`: Tablet narrows the page to 640px and 2 columns, Phone to
  390px and 1 column, with the same theme. Stored placements do not change;
  the inspector Span group is disabled on Tablet/Phone with the reason
  "Spans are set on Desktop". The status chip reads `Previewing phone`.
- **Done when:** Toggling Desktop → Phone → Desktop leaves the model
  byte-identical (test); the phone rendering matches the G20 HTML at 390px.
- **Depends on:** G02, G07. **Touchpoints:** `main.rs`.

### [ ] G13 — Keyboard map, focus scopes, and shortcut help

- **Deliver:** `⌘Z`/`⇧⌘Z` (G03), arrows move selection between cells (`⇧`
  extends), `⌘A` selects all placed tiles, `Esc` clears selection or cancels a
  drag, `1`–`4` set span `1×1`/`2×1`/`2×2`/`Full`, `⌘L` opens the picker,
  `⌘P` preview, `⇧⌘P` publish, `Tab` cycles inspector fields. All additions
  documented in the `?` overlay and available as visible controls (U03
  rule). Text-entry focus scopes from U04 apply; modals trap focus.
- **Done when:** Every shortcut has a visible equivalent; no shortcut fires
  while a field or modal owns focus (test on the routing function).
- **Depends on:** G09, G10, G11. **Touchpoints:** `main.rs` key routing,
  `?` overlay.

### [ ] G14 — Empty, loading, and error states

- **Problem:** The README lists these as "not designed, needs a pass".
- **Deliver:** Publish module with no galleries: centered card "No galleries
  yet" with `New gallery from selection` (disabled with reason when nothing
  is selected) and `Import photos`. Empty tray: "This gallery has no photos"
  with `Add from Library`. Empty page: dashed page-sized target "Drag photos
  here or press ⌘L to flow them into a layout" and a `Flow all N` button.
  `Unplaced` with zero results: "Everything is placed". Missing originals
  (U17 offline set) render the tile with the existing missing badge and
  block publish for that photo with a per-photo reason. Thumbnail loading
  shows the cache's placeholder, never a fake image.
- **Done when:** Each state is reachable in a fresh catalog and matches the
  editor tokens; no state offers a control that does nothing.
- **Depends on:** G04–G08, U17. **Touchpoints:** `main.rs`.

## Epic C — Theme and typography (Page tab)

### [ ] G15 — Theme model and the Page tab inspector

- **Deliver:** `Theme { type_pairing, title_size_px, palette: {page, canvas,
  ink, accent, extras[]}, show_captions, hover_zoom, corner_radius_px }` in
  `theme_json` with the handoff defaults. Page tab (inspector widens to
  348px), 22px gap: **Gallery** text fields (title, eyebrow, subtitle/lede,
  slug with `derive_slug` auto-fill until hand-edited, site name, meta line);
  **Type pairing** three selectable specimen rows (`Aa` 600 23px + name +
  descriptor, active row 1.5px accent border + `IN USE` Mono 9px): Plex Sans
  display, Plex Sans + Mono, Light display; **Title size** slider 32–72px
  with Mono readout and numeric entry; **Palette** 46px 6px-radius swatches
  (page, canvas, ink, accent) with a dashed `+` add slot opening a hex field
  (validated), caption "Sampled from your photos" only after G17 has run;
  **Behaviour** toggles Show captions, Hover zoom, and `Rounded corners`
  value row 0–24px.
- **Done when:** Every theme value changes the canvas immediately (G16) and
  persists; the slug field refuses collisions with the G01 message; a
  contrast check warns inline when accent-on-page text falls under 4.5:1 and
  proposes the README's `#9B93E8` text variant.
- **Depends on:** G01, G04, U04, `slider`/`toggle` controls. **Touchpoints:**
  `gallery.rs` theme types, `main.rs` inspector, new `controls/swatch.rs`.

### [ ] G16 — Live canvas reflects the theme

- **Deliver:** The G07 page reads `Theme` for page/canvas/ink colors, title
  size (the slider value, resolving the README's 54 vs 48 note), type
  pairing (family/weight per role), caption visibility, corner radius, and
  accent eyebrow color (`#9B93E8`-style on-dark text variant when the accent
  fails contrast). The `1c` layout (820px page, 52px 56px padding, 2-column
  4:5 tiles with captions) is the canvas at the `Single column`/`Masonry
  pairs` templates with captions on; no separate screen is built.
- **Done when:** Snapshot tests of the canvas layout tree for the three
  pairings and two title sizes match stored expectations; the same values
  feed G20 so the browser rendering differs only by font hinting.
- **Depends on:** G07, G15. **Touchpoints:** `main.rs` canvas render.

### [ ] G17 — Palette sampling from the gallery's photos

- **Deliver:** A background job that samples dominant colors from the placed
  photos' 512px previews (median cut or k-means on a downsample, in
  `laika-core`), proposes up to five swatches, and never overwrites a swatch
  the user edited. Runs on demand from a `Sample from photos` button and
  after the first placement. Results are ranked by coverage and contrast
  against the page color.
- **Done when:** Sampling a 48-photo gallery finishes under 1 s on the
  reference Mac; proposed swatches differ from each other by ΔE > 10;
  re-running after edits keeps user swatches.
- **Depends on:** G15, U21 job patterns. **Touchpoints:** `laika-core`, `main.rs`.

### [ ] G18 — Embed Plex Sans Light and ship woff2 for the published page

- **Problem:** The lede (300) and the "Light display" pairing need a weight
  Laika does not embed; the published page must load fonts without external
  requests (V29 rule).
- **Deliver:** Add `IBMPlexSans-Light.ttf` to `assets/fonts` and register it at
  startup; add woff2 subsets of Sans 300/400/500/600 and Mono 400/500 under
  `assets/fonts/web` and copy them into every build. Record the OFL license
  file alongside.
- **Done when:** The canvas lede renders at 300; the built page loads with the
  network offline and shows no fallback font (checked in three browsers).
- **Depends on:** none. **Touchpoints:** `assets/fonts`, font registration in
  `main.rs`, `laika-export` asset bundling.

## Epic D — Static site generation (`laika-export`)

### [ ] G19 — Gallery build pipeline: derivatives, manifest, and jobs

- **Deliver:** `laika_export::build(gallery, photos, opts, progress) ->
  Result<BuildReport>`: for each placed photo render through the U10 path
  (`render_export`, acknowledged edit and crop, sRGB, no EXIF) at each size in
  `sizes_json` (default 640/1280/2048, never upscaled), JPEG quality 86,
  written as `img/<hash>-<w>.jpg` where `<hash>` is the photo's blake3 plus an
  edit fingerprint. Build into a temp directory and swap atomically into
  `~/Pictures/Laika Galleries/<slug>/` (or a user-chosen folder, persisted per
  gallery). Write `manifest.json` (photo ids, hashes, sizes, placements,
  theme, build time, Laika version). Incremental: unchanged hashes are hard-
  linked or copied from the previous build, so a caption edit does not re-
  encode 48 images. Runs as a U21 job kind with progress per photo, cancel
  (finishes the current file), a bounded failure list, and retry. Offline
  originals fail per photo with a reason; the build completes for the rest
  and says so. `strip_gps` is honored by construction (no EXIF in pixels;
  say so in the sheet). A pre-build estimate shows count and approximate
  bytes from the previous build's actual sizes, "no estimate yet" before
  that.
- **Done when:** Building the three fixture NEFs in a temp catalog produces
  the expected file tree and manifest (integration test); a second build
  with one caption change re-encodes zero images; cancel leaves the previous
  build intact.
- **Depends on:** G01, G02, U10, U21. **Touchpoints:** `laika-export`
  (`build.rs`, `manifest.rs`), `Cargo.toml` deps (`serde`, `image`,
  `laika-develop`).

### [ ] G20 — HTML, CSS, and JavaScript renderer for the published gallery

- **Deliver:** Compiled `askama` templates (per `plan.md`; V29 adds a
  runtime engine for user themes) producing `index.html` and one CSS block
  that renders `1d`: site header (site name 600 17px; nav omitted unless the
  user adds links in the Page tab), title block (`minmax(0,1fr) 300px`, 56px
  gap, eyebrow Mono 11.5px .2em, title 600 56px/1.04, description 300
  13.5px/1.7, meta line at .45), and the photo grid from G02 placements as CSS
  grid with `grid-column: span n` / `grid-row: span n`, 20px gap, per-photo
  `<img>` with `srcset`/`sizes`, `alt`, `loading="lazy"`, `object-fit` from
  `fit`, `object-position` from `focal`, captions 400 11.5px at .5 when
  enabled, corner radius from the theme. Responsive: 3 col → 2 col at
  ≤1024px → 1 col at ≤640px using the same clamping rule as `at_breakpoint`.
  Theme values become CSS custom properties. Relative paths only; works from
  `file://` and any subpath; zero external requests; fonts from G18. Hooks for
  V27 `<picture>` sources and V28 watermark are left as template slots.
  HTML-escape every user string; test with `<`, `&`, quotes, and emoji.
- **Done when:** The build of a fixture gallery validates with an HTML
  checker, matches a stored golden `index.html` (normalized), opens correctly
  in Safari, Chrome, and Firefox at 1440, 1024, and 390px widths, and the
  cell positions equal the canvas's for every template.
- **Depends on:** G02, G15, G18, G19. **Touchpoints:** `laika-export/templates/`,
  `Cargo.toml` (`askama`).

### [ ] G21 — Lightbox, hover zoom, and download behaviour

- **Deliver:** A small dependency-free script: tiles with `open_full_size`
  open a lightbox showing the 2048 derivative, with `←`/`→`/`Esc`, focus
  trapping, swipe on touch, and the caption; `hover_zoom` applies a 1.03
  scale transition on pointer devices only; `allow_downloads` adds a download
  link to the largest derivative (originals are never copied into a build
  in this story). `password_protect` cannot be enforced by static files, so
  the toggle renders disabled with "needs a host that supports access control"
  until a deploy target provides it (U01 rule); the stored flag is kept.
- **Done when:** Lightbox is fully keyboard operable and announces the
  caption to VoiceOver; disabling JavaScript still shows the grid and links
  to full-size images; no console errors in three browsers.
- **Depends on:** G20. **Touchpoints:** `laika-export/templates/gallery.js`.

### [ ] G22 — Preview in the browser

- **Problem:** Laika has no embedded web view.
- **Deliver:** `Preview` (toolbar, `⌘P`) runs G19 into a per-gallery temp
  directory at the 1280 size only, then opens `index.html` in the default
  browser via the platform opener (extend `import::reveal_in_manager`'s
  sibling). Repeated previews rebuild only changed items. The status chip
  reads `Building preview… n/N` and the job appears in the U21 job center
  with cancel. A preview never touches the publish directory.
- **Done when:** First preview of 48 photos completes under 30 s warm cache
  on the reference Mac and subsequent previews after a caption change under
  2 s; the opened page matches the canvas at Desktop and the phone width.
- **Depends on:** G19, G20. **Touchpoints:** `laika-core/src/import.rs`
  opener, `main.rs`.

### [ ] G23 — Publish: build to folder and deploy with a working URL

- **Deliver:** `Publish` (`⇧⌘P`) opens a sheet over the editor: destination
  (`Folder only` with the path and Reveal; `Cloudflare Pages` via `wrangler
  pages deploy <dir> --project-name <project>`, project name persisted;
  S3-compatible via the existing `object_store` settings from U24 as a later
  option, shown disabled with reason until U24 lands), the pre-build
  estimate, `Build` and `Build & deploy`. Wrangler is detected on open;
  missing wrangler disables deploy with the install hint (plan.md fallback).
  Deploy streams stdout/stderr into a log pane, keeps the sheet open with
  progress, records `last_deploy_at` / `last_deploy_url`, flips status to
  `published`, and shows the URL as a copyable, clickable line. Failure
  keeps the build on disk and offers retry; the log is copyable.
- **Done when:** A build-only publish works with the network off; a deploy
  against a test project yields a URL that serves the gallery; a failed
  deploy leaves `last_build_dir` intact and the error visible.
- **Depends on:** G19, G20, G21. **Touchpoints:** `laika-export/deploy.rs`,
  `main.rs` publish sheet, `galleries` columns.

### [ ] G24 — Republish only what changed and show what will ship

- **Deliver:** Before publish, a diff against the last manifest lists photos
  added, removed, re-encoded (edit fingerprint changed), and text-only
  changes, with counts in the sheet ("3 new images, 45 unchanged, page
  text changed"). Deploy uploads the whole directory (wrangler handles
  dedupe); the local build reuses derivatives per G19. Photos unplaced since
  the last build are removed from the output so nothing stale leaks.
- **Done when:** After editing one photo's exposure, the diff reports one
  re-encode and the build re-renders one photo (test on the manifest diff).
- **Depends on:** G19, G23. **Touchpoints:** `laika-export/manifest.rs`.

## Epic E — Trust, quality, and validation

### [ ] G25 — Truthfulness pass over the editor (U01 for galleries)

- **Deliver:** Audit every control and label in Epics B–D: no sample gallery
  names, counts, timestamps, URLs, or file sizes; status chip states derive
  from the save pill and job states; disabled controls carry a reason
  tooltip; `?` overlay lists the gallery shortcuts; the README's alternates
  (`2a`, `2b`) are explicitly not shipped.
- **Done when:** A fresh catalog walk-through finds zero fabricated values;
  the checklist is recorded in this file's result block.
- **Depends on:** G04–G23.

### [ ] G26 — Automated tests for engine, history, and build

- **Deliver:** Unit tests: templates flow/reflow/apply (G02 properties),
  breakpoint clamp, snap and quantize math, history undo/redo round-trip,
  theme serde defaults, slug collisions, HTML escaping. Integration test in
  `laika-core/tests` or `laika-export/tests`: import fixtures into a temp
  catalog, create a gallery, place, caption, build, assert the file tree,
  manifest, and golden HTML; second build re-encodes nothing. CI runs them on
  macOS and Linux (existing workflow).
- **Done when:** `cargo test --workspace` covers every G-story's "Done when"
  that is testable without a window; golden HTML updates require an explicit
  env flag.
- **Depends on:** G02, G03, G19, G20.

### [ ] G27 — Performance and memory gates for the editor and build

- **Deliver:** Measure and record (hardware, build, cache state) against:

| Scenario | Target |
| --- | --- |
| Drag a tile across a 48-photo page | 60 fps sustained; drop-target update ≤ 16 ms |
| Resize with live badge | Input-to-visible p95 ≤ 50 ms |
| Open a 500-photo gallery in the editor | Interactive within 1 s warm cache |
| Apply a template to 500 photos | Under 50 ms including canvas re-layout |
| Build 200 photos at three sizes, warm decode | Under 4 minutes on the reference Mac; UI stays responsive (no stall > 100 ms) |
| Preview rebuild after a caption edit | Under 2 s |

- **Done when:** Numbers are recorded in this file with the method; misses
  become follow-up items rather than silent regressions.
- **Depends on:** G07–G09, G19, G22, U21.

### [ ] G28 — Accessibility, output validation, and photographer script

- **Deliver:** Publish warns when any placed photo lacks alt text and lists
  them (publish still allowed, warning stated); contrast check from G15
  blocks nothing but is visible; lightbox keyboard/VoiceOver checks; built
  page validated at three widths in three browsers; a photographer script
  in the U23 style: create a gallery from a shoot, flow a template, adjust
  five photos, set a theme, preview, publish to a folder, reopen the app and
  find the gallery intact. Feed friction back into this file.
- **Done when:** Two photographers complete the script without facilitator
  help; the built gallery passes an automated accessibility scan with no
  critical issues.
- **Depends on:** G14, G21, G23, G25.

## Open decisions and assumptions

- **Chrome tokens:** editor uses Laika's palette and green accent; the
  handoff's `#080147` accent is a published-gallery default only (see Design
  reconciliation). Revisit only if the product wants a distinct Publish look.
- **Site header nav** (`Galleries / About / Prints` in `1d`) has no source in
  Laika. G20 omits nav unless links are entered on the Page tab; a multi-
  gallery site index is out of scope here.
- **Meta line** ("48 photographs · Sapporo, Otaru, Shirahama") is a free-text
  field with the count auto-prefixed; place names are not derived from EXIF.
- **HEIC** is not claimed in the import zone until `laika-raw` decodes it.
- **Password protection** stays a stored, disabled toggle (G21) until a host
  with access control is a deploy target.
- **History persistence** across restarts is out of scope (G03); autosave
  makes the model durable, not the undo stack.
- **Formats beyond JPEG, watermarks, user themes** are owned by V27, V28, and
  V29 and plug into the G20 template slots.
- **Alternates `2a`/`2b`** are not scheduled.

## Release gates and validation

Follow the `backlog.md` gates for responsiveness during background work and
for save/restart recovery; add the G27 table above for editor and build
scenarios. Test with the three fixture NEFs plus JPEG and portrait fixtures,
Unicode captions and slugs, a gallery of one photo, a gallery of 500, an
offline original, a read-only output folder, and a missing `wrangler` binary.
Screenshots alone cannot verify placement; use the engine snapshot tests and
the golden HTML.
