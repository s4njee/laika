# Handoff: Laika — Desktop Photo Catalog & Editor (UI Mockups)

## Overview
Laika is a local-first desktop photo catalog and RAW editor for working photographers (weddings, editorial) and serious hobbyists. It differs from comparable tools in three ways, and the UI is built to make those differences visible:

1. **Local-first.** Originals and edits live on the user's disk. Edits are non-destructive and written as `.xmp` sidecars.
2. **Syncs to object storage.** A background queue mirrors the catalog to S3 (or compatible). Sync state is surfaced per-photo, per-catalog, and globally.
3. **Generates static HTML galleries.** Export can build a self-contained HTML gallery plus resized derivatives and deploy it to Cloudflare Pages / similar.

This bundle covers four screens: Library, Develop, Publish, and Map & Metadata.

## About the Design Files
The files in this bundle are **design references created in HTML** — prototypes showing intended look and layout, not production code to copy directly. The task is to **recreate these designs in the target codebase's existing environment** (Electron/React, Tauri, SwiftUI, Qt, etc.) using its established patterns, component library, and styling approach. If no environment exists yet, choose the most appropriate framework for a cross-platform desktop app and implement the designs there.

`Laika.dc.html` is a single design-board file: it renders all four screens side by side on a canvas, each wrapped in a labelled card. The wrapper chrome (the `.dv-turn` / `.dv-opt` / `.dv-card` elements and the light `#e8e5df` page background) is **presentation scaffolding for review only** — it is not part of the product. Everything inside a `.dv-card` is the product.

## Fidelity
**High-fidelity.** Final colors, typography, spacing, and density are specified here and should be matched closely. Two deliberate exceptions:

- **Photographs are placeholders.** Every thumbnail, loupe image, and gallery preview tile is a CSS linear-gradient standing in for a real photo. Replace with real image rendering.
- **The map basemap is a placeholder.** The Map screen shows an abstract shape-and-grid stand-in, explicitly labelled `BASEMAP PLACEHOLDER`. Replace with a real tile provider (MapLibre GL / Mapbox / Leaflet) styled dark to match.

Interactions were not prototyped — these are static mockups. The Interactions section below describes intended behavior.

---

## Screens / Views

All four screens are designed at **1440 × 900 logical px** — the minimum supported window size. Layout is a fixed-height app shell: a top chrome bar, a middle row that consumes remaining height, and (where present) a bottom bar. Side panels are fixed-width; the center column is fluid (`flex: 1; min-width: 0`).

### Shared: Top chrome bar
- Height **46px**, flex row, `padding: 0 14px`, background `#0F0E0D`, bottom border `1px solid rgba(255,255,255,.07)`.
- **Left, 226px wide** (aligns with the left panel below it): a 9px `#00C227` circle, the wordmark `LAIKA` in IBM Plex Mono 600 / 13px / `letter-spacing: .22em`, and on Library only a version tag `0.9.4` in Plex Mono 400 / 9px / `#6F6A62`.
- **Module tabs**: `LIBRARY · DEVELOP · MAP · PUBLISH`. Each is `padding: 6px 13px`, `border-radius: 4px`, IBM Plex Sans 500 / 11px / `letter-spacing: .09em`. Inactive `#8B857C` on transparent; active `#E9E5DE` on `#26241F`.
- **Right**: status pills. Each is `padding: 5px 10px`, `border: 1px solid rgba(255,255,255,.09)`, `border-radius: 4px`, Plex Mono 400 / 10.5px / `#A8A29A`, optionally preceded by a 6px status dot.
  - Library: `s3://laika-archive · synced 2m ago` (green dot `#00C227`), `LOCAL 2.1 TB / 4 TB`, then a 24px circular avatar (`#3A3630`, `1px solid rgba(255,255,255,.12)`).
  - Develop: plain text `DSC_4418.dng · edits stored locally · .xmp sidecar` (Plex Mono 10.5px `#8B857C`), then a pill `edit queued for sync` with an amber dot `#E5C860`.
  - Map: plain text `1,104 of 1,284 geotagged`.

### Shared: Panel section headers
Every panel subsection opens with a header: IBM Plex Mono 500 / 9.5px / `letter-spacing: .14em` / uppercase / `#6F6A62`, `padding: 14px 14px 8px` (18px top when following another group). The one active/highlighted header (Develop's `BASIC`) uses `#00C227`.

### Shared: Slider control
The single most repeated control. Structure:
- Row: label left (Plex Sans 400 / 11px / `#A8A29A`), value right (Plex Mono 400 / 10.5px). Value is `#E9E5DE` at default, `#00C227` when the parameter has been modified from its default. `margin-bottom: 5px`.
- Track: 2px tall, `#2A2723`, full width, `position: relative`.
- Center detent: 1px × 6px tick at `left: 50%`, `top: -2px`, `#413C36`.
- Knob: 10px circle (9px in the Library quick-develop variant), centered on the track via `margin: -5px 0 0 -5px`. `#E9E5DE` at default, `#00C227` when modified.
- Vertical gap between sliders: **10px** (9px in Library quick-develop).

### Shared: Histogram
78px tall (74px in Library), background `#121110`, `1px solid rgba(255,255,255,.07)`, `border-radius: 3px`, `padding: 5px`. Inside: 48 flex-1 bars, `gap: 1px`, `border-radius: 1px 1px 0 0`, height as a percentage. Bar color `#6B655C` in Develop, `#615C54` in Library. Under the Library histogram sits a shot-data row: `ƒ/1.8 · 1/400 · ISO 400 · 35mm`, space-between, Plex Mono 9.5px `#6F6A62`.

### Shared: Filmstrip (Develop, Map)
96px tall, `border-top: 1px solid rgba(255,255,255,.07)`, background `#141312`, horizontal flex, `gap: 6px`, `padding: 0 10px`, `overflow: hidden` (horizontally scrollable in the real app). Each cell is **106 × 72px**, `border-radius: 2px`, `1px solid rgba(255,255,255,.08)` — the current photo is `1px solid #00C227`. Overlays: filename fragment bottom-left (Plex Mono 500 / 8.5px / `rgba(255,255,255,.75)`), star rating bottom-right (`#E5C860`, Develop only).

---

### 1. Library (`#1a` in the board)
**Purpose:** browse, filter, rate, and cull the catalog; see at a glance what is and is not backed up.

**Layout:** 226px left rail · fluid center · 290px right rail.

**Left rail** (`#1A1816`, right border `rgba(255,255,255,.07)`), top to bottom:
- `CATALOGS` — rows at `padding: 5px 8px`, `border-radius: 3px`, 8px gap. Each row: a 5px square color chip (`#00C227` active, `#4A453D` inactive), name (Plex Sans 11.5px; `#E9E5DE` active, `#A8A29A` inactive), right-aligned count (Plex Mono 10px `#6F6A62`). Active row background `#272420`. Content: Weddings 2026 / 1284 · Editorial / 612 · Personal / 3097.
- `FOLDERS` — same row shape but with a disclosure glyph (`▾` open, `▸` closed, `·` leaf) in a 9px centered span, Plex Mono 9px `#55504A`. Names `#BDB7AE`. Content: 2026-06-14 Mori–Tan / 842 · 2026-05-30 Okonkwo / 196 · 2026-05-11 Bhatt / 141 · 2026-04 Engagements / 64 · 2026-03 Scouting / 41.
- `COLLECTIONS` — Selects — Mori/Tan / 84 · Client proofs / 210 · Portfolio / 36.
- **Sync queue footer**, pinned to bottom, `border-top: 1px solid rgba(255,255,255,.07)`, `padding: 12px 14px`, 9px gap: a header row `SYNC QUEUE` / `12` (count in `#00C227`), a 3px progress bar (`#2A2723` track, `#007B12` fill at 72%, `border-radius: 2px`), and a caption `uploading DSC_4412.dng · 18 MB/s` (Plex Mono 10px `#6F6A62`).

**Center — toolbar** (40px, bottom border):
- View segmented control: `GRID / LOUPE / COMPARE` in a `#1F1D1A` pill (`border-radius: 4px`, `padding: 2px`); each segment `padding: 4px 9px`, Plex Mono 500 / 10.5px / `letter-spacing: .06em`; active on `#332F29` with `#E9E5DE`, inactive `#8B857C`.
- 1px × 18px divider `rgba(255,255,255,.08)`.
- Filter chips, 6px gap: `★ 3+`, `Picked`, `RAW only`, `Unsynced`. Each `padding: 4px 9px`, `border-radius: 3px`, 10.5px. Inactive `1px solid rgba(255,255,255,.1)` / `#BDB7AE`; active (`Picked`) `1px solid #00C227` / `#00C227`.
- Right: `SORT capture time` (Plex Mono 10.5px `#6F6A62`), divider, then a `SIZE` thumbnail-scale slider — 70px × 3px track `#2A2723`, `#7E786E` fill to 58%, 9px `#E9E5DE` knob.

**Center — grid:** `display: grid`, `grid-template-columns: repeat(6, 1fr)`, `gap: 10px`, `padding: 12px`, `align-content: start`. Each cell: `border-radius: 2px`, `1px solid rgba(255,255,255,.07)`, `overflow: hidden`, image at `aspect-ratio: 3/2`. Overlay bar across the bottom, 20px tall, `padding: 0 6px`, `background: linear-gradient(transparent, rgba(0,0,0,.82))`: filename (Plex Mono 500 / 9px / `#C9C4BB`), spacer, star rating (Plex Mono 9px / `#E5C860`), and a 5px sync dot — `#00C227` synced, `#E5C860` pending.

**Center — status bar:** 28px, `#0F0E0D`, top border, `padding: 0 14px`, 16px gap, Plex Mono 10px `#6F6A62`: `1,284 photos` · `14 selected` (in `#00C227`) · `412 picked` · `1,272 synced · 12 pending`; right-aligned breadcrumb `Weddings 2026 / 2026-06-14 Mori–Tan`.

**Right rail** (290px, `#1A1816`, left border): histogram + shot data → `QUICK DEVELOP` (Exposure +0.35 / Contrast +12 / Highlights −40 / Shadows +28) → `METADATA` key/value list (File, Captured, Camera, Lens, Size, Local, Remote) → `KEYWORDS` as chips (`padding: 3px 7px`, `#26241F`, `border-radius: 2px`, 10.5px `#BDB7AE`, 5px gap: ceremony, golden hour, Mori–Tan, Presidio) → pinned action footer with a primary **Publish gallery** button (flex 1, `#007B12` on `#DFFFE4` text, `border-radius: 3px`, Plex Sans 600 / 11px) and a secondary **Export** button (`1px solid rgba(255,255,255,.14)`, `#BDB7AE`).

Metadata rows are space-between with a 10px gap: key in Plex Sans 11px `#6F6A62`, value right-aligned in Plex Mono 10.5px `#BDB7AE`, 4–5px row gap.

### 2. Develop (`#1b`)
**Purpose:** non-destructive RAW editing of one photo with fast movement through the shoot.

**Layout:** 210px left rail · fluid center (canvas + toolbar + filmstrip) · 306px right rail. Page background `#0F0E0D`; the canvas area is darker still, `#0B0A0A`.

**Left rail:**
- `PRESETS` — rows at `padding: 5px 8px` with an 18 × 12px gradient swatch (`border-radius: 1px`) previewing the look, then the name (11.5px). Active row on `#272420` with `#E9E5DE`; others `#BDB7AE`. Content and swatch gradients: Neutral RAW `#8A8178→#2A2723` · **Portra warm** `#C99A6B→#3A2C21` (active) · Ceremony low-light `#6E7C8C→#1B1F24` · Golden hour `#E0A85A→#40301C` · Mono contrast `#E4E0DA→#131211` · Reception tungsten `#C4795A→#2A1C16` · Editorial cool `#8FA3AE→#1E2428`.
- `HISTORY` — space-between rows, 6px gap, `padding: 0 14px`: action name (11px) and value (Plex Mono 10px `#6F6A62`). Newest first, newest in `#00C227`: Paste settings / now · Shadows / +28 · Highlights / −40 · Temperature / 5480 K · Crop 3:2 / — · Portra warm / preset · Import / 18:42 (dimmed to `#7E786E`).
- Pinned footer note, top border, `padding: 12px 14px`, Plex Mono 10px `#6F6A62`, `line-height: 1.6`: "non-destructive / original untouched on disk".

**Center — canvas:** the photo is centered in a `flex: 1` area with `padding: 26px`, `min-width: 0`, `min-height: 0`, `overflow: hidden`; the image is `height: 100%; max-width: 100%; max-height: 100%; aspect-ratio: 3/2` so it letterboxes rather than pushing into the panels. It is shown in **split before/after**: the left 38% carries the unedited render and a right edge of `1px solid rgba(0,194,39,.75)` as the divider. Corner labels `BEFORE` / `AFTER` in Plex Mono 500 / 9.5px / `letter-spacing: .12em` / `rgba(255,255,255,.62)` at 12–14px inset. Bottom-right chip on `rgba(0,0,0,.55)`, `padding: 4px 8px`, Plex Mono 9.5px: dimensions `6048 × 4024`.

**Center — tool bar** (38px, `#0F0E0D`, top border, `padding: 0 16px`): tool buttons `CROP / HEAL / MASK / RED-EYE` (same segment styling as the view control, 4px gap, `CROP` active on `#332F29`), divider, zoom presets `FIT · 1:1 · 2:1` (Plex Mono 10.5px `#6F6A62`), and a right-aligned shortcut hint `\\ before/after · Y split · P pick`.

**Center — filmstrip:** shared spec above; photo 8 of 16 is current (green border).

**Right rail:** histogram (in a 12px/14px block with a bottom border) → `BASIC` header with a `reset` affordance on the right (Plex Mono 10px `#6F6A62`) → 12 sliders → four collapsed panel headers, each its own `border-top` row at `padding: 11px 14px` with a `+` on the right: `TONE CURVE`, `COLOR MIX`, `DETAIL`, `OPTICS` → pinned footer with two equal buttons: **Copy settings** (outline) and **Paste to 14** (primary green; the number reflects the current multi-selection).

Slider values, in order — modified ones (green) marked `*`: Temperature 5480 K, Tint +6, Exposure +0.35`*`, Contrast +12, Highlights −40`*`, Shadows +28`*`, Whites +8, Blacks −14, Texture +10, Clarity +4, Vibrance +18, Saturation 0.

### 3. Publish (`#1c`)
**Purpose:** export a selection, and in particular build and deploy a static HTML gallery. This is the screen that carries Laika's differentiator.

**Layout:** a modal over the dimmed Library. The backdrop is the photo grid at `opacity: .22` (8 columns) under a `rgba(10,9,9,.78)` scrim. The dialog is **1020px wide**, centered, `#1A1816`, `1px solid rgba(255,255,255,.1)`, `border-radius: 6px`, `box-shadow: 0 30px 80px rgba(0,0,0,.6)`, height driven by content.

**Dialog left column, 250px** (`#151412`, right border, `padding: 16px 0`):
- Title `Publish 84 photos` (Plex Sans 600 / 12px), `padding: 0 16px 12px`.
- `DESTINATION` — radio rows at `padding: 7px 9px`, 9px gap, with a 5px dot (`#00C227` selected, `#4A453D` not) and 11.5px label. Selected row on `#272420`. Options: **Static gallery** (selected), Local disk, S3 bucket, Client proof link.
- `SAVED RECIPES` — plain 11.5px `#BDB7AE` rows, 7px gap: Wedding proofs — 2048px · Portfolio — full res · Instagram — 1350 sq.
- Pinned footer, top border: `est. 612 MB` / `≈ 2 min 10 s` (Plex Mono 10px `#6F6A62`, `line-height: 1.7`).

**Dialog right column:**
- Header, `padding: 18px 22px 0`: `Static gallery` (Plex Sans 600 / 15px) with an inline explainer `builds HTML + resized JPEGs, deploys to your host` (Plex Mono 11px `#6F6A62`), 10px gap, baseline-aligned.
- A **2-column form grid**, `gap: 18px 26px`:
  - **GALLERY TITLE** — text field: `padding: 8px 10px`, `#121110`, `1px solid rgba(255,255,255,.1)`, `border-radius: 3px`, 12px. Value `Mori & Tan — 14 June 2026`.
  - **URL** — same field shell, Plex Mono 11.5px, showing the host prefix `galleries.studio.com/` in `#6F6A62` and the editable slug `mori-tan` in `#00C227`.
  - **TEMPLATE** — three equal-width option buttons, 8px gap, `padding: 9px`, centered 11px: **Masonry** selected (`1px solid #00C227`, `#00C227`), Full-bleed and Contact sheet unselected (`rgba(255,255,255,.1)`, `#A8A29A`).
  - **SIZES EMITTED** — multi-select chips, 6px gap, `padding: 6px 10px`, Plex Mono 10.5px: 640, 1280, 2048 on (green outline + green text); 4096, AVIF off (`rgba(255,255,255,.1)`, `#6F6A62`).
- **OPTIONS** column (left) + **PREVIEW** (right), 26px gap:
  - Toggles: 26 × 14px pill, `border-radius: 8px`; on = track `#007B12` with a 10px `#DFFFE4` knob at `left: 14px`; off = track `#2E2B27` with a `#7E786E` knob at `left: 2px`. Label 11.5px `#BDB7AE`, 9px gap, 9px row gap. Watermark **on**, Allow downloads **on**, Password protect off, Strip GPS from EXIF **on**, Lazy-load AVIF off.
  - Preview box: 128px tall, `#0F0E0D`, `1px solid rgba(255,255,255,.08)`, `border-radius: 3px`, `padding: 9px`, 7px gap. A title line `MORI & TAN — 14 JUNE 2026` (Plex Mono 500 / 9px / `letter-spacing: .1em` / `#7E786E`) above a 6 × 2 thumbnail grid with 5px gaps.
- **Command log**, `margin: 18px 22px 0`, `padding: 10px 12px`, `#0F0E0D`, `1px solid rgba(255,255,255,.08)`, `border-radius: 3px`, Plex Mono 10.5px / `line-height: 1.75` / `#7E786E`, each line prefixed by a green `→`. This is a live echo of the equivalent CLI invocation — it should update as the form changes:
  ```
  laika build --gallery mori-tan --sizes 640,1280,2048
  laika deploy --target cloudflare-pages --project studio-galleries
  ```
- **Footer**, top border, `padding: 16px 22px`, 12px gap: `last deploy 3 days ago · 612 files` (Plex Mono 10.5px `#6F6A62`), spacer, **Build only** (outline, `padding: 9px 16px`), **Build & deploy** (primary green, `padding: 9px 20px`, Plex Sans 600 / 11.5px).

### 4. Map & Metadata (`#1d`)
**Purpose:** find photos by place and date, and inspect complete EXIF plus storage provenance.

**Layout:** 226px left rail · fluid map + filmstrip · 306px right rail.

**Left rail:** `PLACES` list (San Francisco / 612 selected, Presidio / 284, Marin Headlands / 141, Oakland / 48, Point Reyes / 19) → `DATE RANGE` with two date labels (Plex Mono 10.5px `#A8A29A`) over a dual-handle range slider (2px `#2A2723` track, `#00C227` selected span from 22% to 70%, two 9px `#E9E5DE` handles) → `CAMERA` facet counts (Sony A7 IV / 802, Leica Q3 / 302, Fujifilm X100VI / 180).

**Center — map:** background `#101413` with a 56px graticule drawn as two 1px `rgba(255,255,255,.035)` linear-gradient grids. Landmass placeholders are two soft blobs (`#181D1B` / `#1A1F1C`, `1px solid rgba(255,255,255,.05)`, irregular `border-radius`). Photo **clusters** are circles sized by count (24–52px), `background: rgba(0,194,39,.18)`, `1px solid #00C227`, count centered in Plex Mono 500 / 10px / `#00C227`. Top-left a labelled placeholder chip; bottom-right a 26px `+` / `−` zoom stack on `rgba(15,14,13,.9)` with `1px solid rgba(255,255,255,.1)`.

**Center — filmstrip:** shared spec, without star overlays.

**Right rail:** a photo preview block (3:2, gradient placeholder, `1px solid rgba(255,255,255,.07)`) with the filename beneath in Plex Mono 500 / 11px → `EXIF` key/value list of 14 rows (Camera, Lens, Focal, Aperture, Shutter, ISO, Metering, White bal., Captured, GPS, Altitude, Artist, Copyright, Rating) → `STORAGE` block with three rows: Local `/Volumes/Archive/2026` (value in `#00C227` — it is on-disk and available), Remote `s3://laika-archive`, Checksum `blake3 · verified`.

---

## Interactions & Behavior
Not prototyped; intended behavior:

- **Module tabs** switch the whole workspace. Keyboard: `G` Library grid, `E` Loupe, `D` Develop, `C` Compare.
- **Grid**: click selects, shift-click range, cmd/ctrl-click adds. Selection count drives the status bar and the Develop **Paste to N** button. `P` picks, `X` rejects, `1–5` sets stars, `0` clears.
- **Filter chips** are independent toggles, AND-combined. `Unsynced` is the one that matters for the local-first story — it filters to photos whose remote state is pending or failed.
- **Thumbnail size slider** changes the grid's column count live (range roughly 3–12 columns).
- **Sliders**: drag the knob, or scrub anywhere on the row; double-click the knob or the value resets to default; click the value to type an exact number. Arrow keys nudge, shift-arrow ×10. The value and knob turn `#00C227` the moment the value differs from default, and revert to `#E9E5DE` on reset — this is the only signal that a parameter has been touched, so it must be exact.
- **Before/after**: hold `\\` for full-frame before; `Y` toggles the persistent split shown in the mock. The split divider is draggable.
- **History** is click-to-revert; clicking an older entry previews that state, and a subsequent edit truncates the stack after it.
- **Presets**: hover a preset to preview it on the canvas; click to apply as a history step.
- **Sync**: the queue footer and the per-photo dot update from the background uploader. Amber `#E5C860` = pending or in-flight; green `#00C227` = verified remote copy. Failure state (not mocked) should use a red and be clickable through to a retry.
- **Publish**: changing any field re-renders the command log immediately. **Build only** writes the gallery to a local directory and reveals it; **Build & deploy** runs the build then the deploy, and should transition the footer to a progress/log state (not mocked) rather than closing the dialog.
- **Map**: clicking a cluster zooms and filters the filmstrip; drag-select on the map filters. Selecting a photo with no GPS should offer manual placement by drag.
- **Hover states** (not mocked): list rows and grid cells lift to `#201E1B`; toolbar segments to `#2A2723`; primary buttons lighten to `#00921A`; outline buttons take `border-color: rgba(255,255,255,.24)`.
- **Transitions**: keep them short and mechanical — 120ms `ease-out` on hover/background changes, no motion on slider drag. Panel expand/collapse 160ms.

## State Management
- `activeModule`: library | develop | map | publish
- `activeCatalog`, `activeFolder`, `activeCollection` — the source scope for the grid
- `filters`: `{ minStars, picked, rawOnly, unsynced, dateRange, places[], cameras[] }`
- `sort`: field + direction (default capture time, ascending)
- `selection`: ordered id list; `primaryId` is the one shown in Develop and in the metadata rail
- `thumbSize`: grid column count
- `edits[photoId]`: the parameter map backing the sliders, plus `defaults` for diff detection (drives the green modified state) and an `historyStack` with a cursor
- `clipboard`: copied edit settings for Copy/Paste
- `syncState[photoId]`: local | pending | synced | failed; plus a global `syncQueue` (length, current file, throughput, progress)
- `publishForm`: destination, title, slug, template, sizes[], toggles, plus derived `estimatedBytes` / `estimatedDuration` / `commandPreview`
- `lastDeploy`: timestamp + file count per gallery

Data needs: catalog index and EXIF read from local storage (SQLite or similar); thumbnails from a local derivative cache; remote object listing and checksums for sync reconciliation; deploy status from the host's API.

## Design Tokens

**Colors**
| Token | Hex | Use |
|---|---|---|
| bg/chrome | `#0F0E0D` | top bar, status bar, toolbars, inset wells |
| bg/canvas | `#0B0A0A` | Develop image canvas |
| bg/app | `#141312` | center column, filmstrip |
| bg/panel | `#1A1816` | left and right rails |
| bg/panel-deep | `#151412` | Publish dialog left column |
| bg/well | `#121110` | histogram, form fields |
| bg/row-active | `#272420` | selected list row |
| bg/tab-active | `#26241F` | active module tab |
| bg/segment-active | `#332F29` | active toolbar segment |
| bg/segment-shell | `#1F1D1A` | segmented-control container |
| bg/chip | `#26241F` | keyword chips |
| track | `#2A2723` | slider tracks, progress track |
| track/off | `#2E2B27` | toggle off |
| detent | `#413C36` | slider center tick |
| text/primary | `#E9E5DE` | |
| text/secondary | `#BDB7AE` | list items |
| text/tertiary | `#A8A29A` | slider labels, pill text |
| text/muted | `#8B857C` | inactive tabs |
| text/dim | `#6F6A62` | section headers, keys, counts |
| text/dimmer | `#7E786E` | log text, off-state knob |
| accent/fill | `#007B12` | primary buttons, toggle on, progress fill |
| accent/on-fill | `#DFFFE4` | text on accent fill |
| accent/line | `#00C227` | accent text, active borders, status dots, modified values |
| accent/wash | `rgba(0,194,39,.18)` | map cluster fill |
| warning | `#E5C860` | pending sync, star ratings |
| border/hairline | `rgba(255,255,255,.07)` | panel dividers |
| border/control | `rgba(255,255,255,.1)` | inputs, chips, map controls |
| border/strong | `rgba(255,255,255,.14)` | secondary buttons |
| scrim | `rgba(10,9,9,.78)` | modal backdrop |

Accessibility note: `#00C227` on `#141312` clears 4.5:1. `#DFFFE4` on `#007B12` clears 4.5:1. Do not use `#007B12` as a text color on dark backgrounds, and do not use `#00C227` as a fill behind dark text.

**Typography** — IBM Plex Sans (400/500/600) for UI text; IBM Plex Mono (400/500) for all numerics, filenames, paths, section headers, and status text. The mono/sans split is load-bearing: anything machine-generated is mono.
| Role | Spec |
|---|---|
| Wordmark | Mono 600 / 13px / `.22em` |
| Module tab | Sans 500 / 11px / `.09em` |
| Section header | Mono 500 / 9.5px / `.14em` / uppercase |
| List item | Sans 400 / 11.5px |
| Slider label | Sans 400 / 11px |
| Numeric value | Mono 400 / 10.5px |
| Metadata key | Sans 400 / 11px |
| Metadata value | Mono 400 / 10.5px |
| Count / caption | Mono 400 / 10px |
| Thumbnail overlay | Mono 500 / 8.5–9px |
| Dialog title | Sans 600 / 15px |
| Button | Sans 500–600 / 11–11.5px |

**Spacing** — 4px base. Panel padding `14px` horizontal; dialog padding `22px`; grid gap `10px`; filmstrip gap `6px`; chip gap `5–6px`; slider stack gap `10px`; list row gap `1px`.

**Radius** — `1px` swatches · `2px` thumbnails and chips · `3px` rows, buttons, fields · `4px` tabs and pills · `6px` dialog · `50%` dots, knobs, avatars, map clusters.

**Elevation** — one shadow only: `0 30px 80px rgba(0,0,0,.6)` on the modal. Everything else separates with hairline borders and background steps. Keep it that way; this UI reads as flat panels, not cards.

**Fixed dimensions** — top bar 46px · toolbar 40px · Develop tool bar 38px · status bar 28px · filmstrip 96px (cell 106 × 72) · left rail 226px (210px in Develop) · right rail 290px (306px in Develop and Map) · dialog 1020px · histogram 74–78px.

## Assets
- **Fonts:** IBM Plex Sans and IBM Plex Mono, loaded from Google Fonts in the mock. Ship them self-hosted in a desktop app. Both are OFL-licensed.
- **Photographs:** none. Every image is a CSS gradient placeholder generated in the mock's logic (`TONES` array + `g()` helper). Supply real assets.
- **Basemap:** none. Placeholder only — see Fidelity.
- **Icons:** the mock deliberately uses no icon set; affordances are text, geometric dots, squares, and disclosure glyphs (`▾ ▸ · + −`). If the target app has an icon library, introducing icons is a real design decision — check before doing it, because the density and the type-driven look depend on their absence.

## Files
- `Laika.dc.html` — the design board containing all four screens. Open it in a browser. Screens are marked `1a` Library, `1b` Develop, `1c` Publish, `1d` Map & metadata. Ignore the light-background review chrome around each card.
- `support.js` — runtime required for `Laika.dc.html` to render. Not product code; do not port it.
