# Handoff: Photo Gallery Builder (WYSIWYG)

## Overview
A desktop web app for photographers and personal users to build and publish photo galleries. Editing is **grid-based WYSIWYG**: photos snap into a responsive column grid; each photo can span cells. The editor uses classic three-pane chrome — left photo library tray, center live canvas, right contextual inspector.

Four screens are covered: gallery editor, layout/template picker, theme & typography settings, published gallery. Two alternate visual treatments of the editor are included as exploration.

## About the Design Files
The files in this bundle are **design references created in HTML** — prototypes showing intended look and structure, not production code to copy. The task is to **recreate these designs in the target codebase's existing environment** (React, Vue, SwiftUI, native, etc.) using its established patterns, component library, and tokens. If no environment exists yet, pick the most appropriate framework and implement there.

Photo tiles in the mocks are CSS gradient **placeholders** standing in for real images. No real photography was used.

## Fidelity
**High-fidelity (hifi)** for layout, color, and typography: exact hex values, type sizes, and spacing are specified below and should be matched. **Static** — no interaction logic is implemented in the prototype; the interaction section below is specification, not demonstrated behavior.

## Design Tokens

### Color — dark theme (primary, used by all four core screens)
| Token | Value | Use |
|---|---|---|
| `surface/app` | `#17191C` | Outer app shell background |
| `surface/chrome` | `#1B1E21` | Top bar, left tray, right inspector, status bar, modal body |
| `surface/canvas` | `#0F1113` | Canvas area surrounding the gallery page |
| `surface/page` | `#121517` | The gallery page itself; also input fields, cards, template thumbnails |
| `surface/footer` | `#1B1E21` | Modal footer |
| `surface/browserbar` | `#1F2225` | Browser chrome in published preview |
| `ink/primary` | `#F2EFE6` | Titles, primary labels, active tab text, slider fill |
| `ink/secondary` | `rgba(242,239,230,.55–.72)` | Body copy, secondary labels |
| `ink/tertiary` | `rgba(242,239,230,.4–.5)` | Section eyebrows, meta, placeholder text |
| `line/hairline` | `rgba(242,239,230,.1–.12)` | Panel dividers, card borders |
| `line/control` | `rgba(242,239,230,.16–.22)` | Button and input borders |
| `accent` | `#080147` | Primary buttons, selection outline, active chips, focal/snap indicators |
| `accent/hover` | `#050030` | Accent link hover |
| `accent/tint` | `rgba(8,1,71,.08–.5)` | Accent fills at low opacity (drop target, chips) |
| `accent/on-dark-text` | `#9B93E8` | Accent-colored **text** on dark panels (eyebrows, "SNAP ON", draft chip) — `#080147` is unreadable as text on near-black |
| `accent/label-ink` | `#F2EFE6` | Text sitting on an accent fill |
| `photo/placeholder` | linear-gradients of `#A9B2B0→#66726F`, `#D6CCBC→#A89C8B`, `#9AA3A8→#6B747A`, `#C9C2B4→#9C9485`, `#8E9A98→#5F6A6B` | Stand-ins for photos |

**Known issue to resolve in implementation:** `#080147` is very close in value to the dark chrome. Selection outlines and the Publish button read faintly. Recommended fix: keep `#080147` as the fill but add a `rgba(242,239,230,.22)` 1px border to accent buttons, and use a lighter same-hue tint (`#5B4BC9` or similar) for selection outlines.

### Color — light alternate (`2b` Editorial paper only)
`#F7F4EC` chrome · `#EDE7DB` canvas · `#FFFDF8` page · `#22262B` ink · `rgba(34,38,43,.4–.7)` secondary ink · `#080147` accent.

### Typography
- **All type: IBM Plex Sans** (weights 300, 400, 500, 600). Headers are sans, not serif.
- **IBM Plex Mono** (400, 500) for eyebrows, numeric readouts, dimensions, keyboard hints.
- Scale in use:

| Role | Spec |
|---|---|
| Wordmark ("Aperture") | 600 19px Plex Sans |
| Published gallery title | 600 56px/1.04, letter-spacing −.018em |
| Theme screen page title | 600 48px/1.06, letter-spacing −.015em |
| Editor canvas page title | 600 34px, letter-spacing −.01em |
| Modal title | 600 27px |
| Type-specimen "Aa" | 600 23px |
| Published nav name | 600 17px |
| Page subtitle / lede | 300 13–15px, line-height 1.6–1.7 |
| Inspector row label | 400 12.5px |
| Primary button | 500 12px |
| Secondary button | 400 12px |
| Chip | 400–500 11px |
| Section eyebrow | 500 10px Plex Mono, letter-spacing .13em, uppercase |
| Published eyebrow | 400 11.5px Plex Mono, letter-spacing .2em, uppercase |
| Caption (page + inspector) | 400 11.5px |
| Meta / file info | 400 11px Plex Mono |

### Spacing, radius, shadow
- Spacing steps used: 4, 5, 6, 8, 9, 12, 14, 16, 18, 20, 22, 26, 30, 34, 46, 56 px.
- Radius: `3px` badges · `5px` inputs and small buttons · `6px` primary buttons, swatches · `7px` cards, drop zones · `8px` template cards · `10px` app shell · `12px` modal · `12–13px` pill chips · `50%` toggle knobs and focal handle. Photos have **0 radius** by default (adjustable in theme panel).
- Shadow: app shell `0 2px 6px rgba(0,0,0,.08)` · gallery page `0 2px 20px rgba(0,0,0,.45)` · modal `0 24px 60px rgba(0,0,0,.62)` · toggle segment `0 1px 2px rgba(0,0,0,.06)`.
- Active tab indicator: `box-shadow: inset 0 -2px 0 #F2EFE6`.

## Screens / Views

### 1. Gallery editor (`1a`)
**Purpose:** place, resize and annotate photos on the gallery page.

**Layout** — 1440×900 shell, `grid-template-rows: 58px 1fr 34px`; middle row `grid-template-columns: 248px 1fr 300px`.

**Top bar (58px, `surface/chrome`, bottom hairline):** wordmark · 1px×20px divider · gallery name (500 13.5px) + status chip ("Draft · saved 2m ago", accent-tint background, `accent/on-dark-text`) · right side: breakpoint segmented control (Desktop / Tablet / Phone; track `rgba(242,239,230,.06)`, 6px radius, 3px pad; active segment `surface/page` + shadow) · "Preview" (outlined, 7px 14px) · "Publish" (accent fill, 7px 16px).

**Left tray (248px):**
- Header row: eyebrow "Library" + "48 photos".
- Filter chips: `All` (active, `ink/primary` fill with dark text), `Unplaced 11`, `Picks` (outlined pills).
- 2-column thumbnail grid, 8px gap, 1:1 tiles, numeric index badge bottom-left (500 8px Mono). Selected thumbnail: 2px accent outline, 1px offset, plus a 14px accent dot top-right.
- Bottom, above a hairline: dashed 7px-radius import zone — "Drop photos to import" (500 12px) / "JPEG, HEIC, RAW · up to 60 MB" (400 11px tertiary).

**Canvas (center, `surface/canvas`):**
- Canvas toolbar (9px 18px, bottom hairline): breadcrumb "Home / Hokkaidō, February" · right: `GRID 12 COL` (Mono 10px tertiary), `SNAP ON` (Mono 10px, `accent/on-dark-text`), zoom "92%".
- Page: max-width 760px, centered, `surface/page`, padding 38px 42px 0, page shadow.
- Page content: title (600 34px) with "2026" Mono eyebrow right-aligned on the same baseline row; lede 300 13px/1.6, max-width 44ch; then a 3-column grid, 12px gap.
- Grid contents: a 2×1 spanning photo (selected), three 1×1 photos, one empty drop target (accent-tint fill, 1.5px dashed accent, centered `DROP HERE` Mono 10px), one full-width 3×1 photo.
- **Selection affordance:** 2px accent outline at 3px offset, four 7px square corner handles (`#F2EFE6` fill, 2px accent border) at the corners, and a floating size badge above the top-left corner: accent fill, `F2EFE6` text, Mono 10px — `2 × 1 · 640 × 320`.
- **Snap guides:** two 1px vertical lines at the 1/3 and 2/3 column boundaries, `rgba(8,1,71,.5)`, spanning full page height.

**Right inspector (300px):** three tabs — Photo (active) / Layout / Page.
Photo tab contents, 18px gap:
1. File row: 56px square thumbnail + `DSCF4180.jpg` (500 12.5px) + `4032 × 2688 · 8.4 MB` (Mono 11px).
2. **Span** — 4-up button group: `1×1`, `2×1` (active, accent fill), `2×2`, `Full`.
3. **Crop & focal point** — 96px-tall photo preview with rule-of-thirds overlay (`rgba(255,253,248,.35)` 1px lines) and a draggable 16px circular focal handle (2px light border + dark 1px halo). Below: `Fill` / `Fit` / `Reset`.
4. **Caption** — filled input, value "Shirahama, 06:40". **Alt text** — empty input, placeholder "Describe this photo for screen readers…".
5. Toggle row: "Open full size on click" — 34×19 track, accent when on, 15px light knob.

**Status bar (34px):** "14 of 48 placed" · "Row 1 · cell 1" · right-aligned "Hold ⇧ to keep ratio · ⌘Z undo".

### 2. Layout & template picker (`1b`)
**Purpose:** choose the grid template the photos flow into.

Modal (1020px wide, 12px radius, `surface/chrome`) centered over the editor. Backdrop: the editor at `blur(1.5px)` / `opacity .5`, plus a `rgba(0,0,0,.58)` scrim.

- Header (26px 30px): "Choose a layout" (600 27px) + sub "Your 48 photos flow into the grid. You can move any of them afterwards." (400 12.5px secondary); close ✕ right.
- Category chips: `All 9` (active) · Uniform grid · Editorial · Single column · Contact sheet.
- 4-column card grid, 16px gap. Each card: `surface/page`, 1px hairline border, 8px radius, a miniature diagram of the layout in flat tones (16px pad, 4–6px gaps), then name (500 12.5px) + description (400 11px tertiary).
- Eight templates: **Mixed spans** (3 col, variable — marked CURRENT with 2px accent border and an accent `CURRENT` badge) · **Square grid** (3 col, 1:1) · **Editorial rows** (alternating widths) · **Single column** (centered, captions below) · **Contact sheet** (4 col, tight gutter) · **Masonry pairs** (2 col, mixed ratio) · **Hero + grid** (lead image then 3 col) · **Filmstrip** (horizontal scroll rows).
- Footer (16px 30px, top hairline): "Switching layouts keeps your captions and photo order." · `Cancel` (outlined) · `Apply layout` (accent fill).

### 3. Theme & typography (`1c`)
**Purpose:** set the published gallery's type, palette, and photo behavior.

Layout: 58px top bar, then `grid-template-columns: 1fr 348px` — large live canvas left, inspector on the **Page** tab right.

Canvas page (max-width 820px, padding 52px 56px 0): accent eyebrow `TRAVEL · 2026` (Mono 12px, .2em tracking, `accent/on-dark-text`) · title 600 48px/1.06 max-width 18ch · lede 300 15px/1.65 max-width 52ch · 2-column photo grid (18px gap, 4:5 tiles) with captions below each (400 11.5px secondary).

Inspector (348px), 22px gap:
1. **Type pairing** — three selectable rows, each: large "Aa" specimen + name + descriptor, `surface/page` fill, 7px radius. Active row has 1.5px accent border and an `IN USE` Mono 9px accent label. Options: *Plex Sans display / tight, all weights* (in use) · *Plex Sans + Mono / technical captions* · *Light display / airy, low contrast*.
2. **Title size** — slider (2px track, 11px knob with 2px `ink/primary` border), fill at 62%, readout "54 px" (Mono 11.5px). Note: the canvas renders 48px; reconcile in implementation.
3. **Palette** — five 46px 6px-radius swatches: page `#121517` (selected, 1.5px accent border), canvas `#0F1113`, ink `#F2EFE6`, accent `#080147`, and a dashed "+" add slot. Caption: "Sampled from your photos".
4. **Behaviour** — toggles: "Show captions" (on), "Hover zoom" (off); plus a value row "Rounded corners — 0 px".

### 4. Published gallery (`1d`)
**Purpose:** what a visitor sees.

1440×900, `surface/page` background, wrapped in a minimal browser bar (`surface/browserbar`, three `#3C4145` dots, URL pill `mirakawa.photo/hokkaido-february` in Mono 11px).

Content padding 0 88px:
- Site header (26px vertical, bottom hairline): site name "Jun Mirakawa" (600 17px) · nav Galleries (active) / About / Prints (400 12px, inactive at .55 opacity), 26px gap.
- Title block: `grid-template-columns: minmax(0,1fr) 300px`, 56px gap, 46px top padding. Left: accent eyebrow `TRAVEL · 2026` + title 600 56px/1.04. Right: description 300 13.5px/1.7 + meta "48 photographs · Sapporo, Otaru, Shirahama" (400 11.5px, .45 opacity).
- Photo grid: 3 columns, 20px gap. First item spans 2 columns at 2:1 with caption below; remaining items 1:1; captions 400 11.5px at .5 opacity, 9px top margin.

### Alternates (exploration only)
- **`2a` Dark studio** — 1040×600 crop of the editor: `#15171A` chrome, `#1D2023` canvas, `#0E1011` page, `#080147` accent. Serif wordmark and title retained (Instrument Serif). Narrower panels (200 / 232).
- **`2b` Editorial paper** — light theme, borderless: no panel dividers, Mono uppercase tracking-heavy labels instead of buttons, values set in serif, selected photo marked by a 1px ink outline rather than an accent outline. Panels 190 / 236.

If you only implement one direction, implement the dark core flow (`1a`–`1d`).

## Interactions & Behavior
Specification — not implemented in the prototype.

- **Drag from tray to canvas:** dragging a library thumbnail shows the nearest valid cell as a drop target (accent-tint fill, dashed accent border). On drop, the photo occupies that cell at the template's default span.
- **Snap:** while dragging or resizing, show 1px accent guides at column boundaries, `rgba(8,1,71,.5)`. Snap threshold ~8px. `⇧` constrains aspect ratio.
- **Select:** click a photo on canvas or in the tray → accent outline + corner handles + size badge; inspector switches to the Photo tab; the tray thumbnail gets the accent dot.
- **Resize:** drag a corner handle; span quantizes to whole columns/rows; the size badge updates live (`2 × 1 · 640 × 320`).
- **Span buttons** set span directly; siblings reflow.
- **Focal point:** drag the circular handle over the crop preview; stored as normalized x/y and applied as `object-position` when the photo is cropped.
- **Template apply:** photos reflow into the new template preserving order, captions, and alt text; per-photo span overrides are reset. Show an undo affordance.
- **Breakpoint toggle** re-renders the canvas at tablet/phone column counts; it does not change stored layout.
- **Autosave** every few seconds; status chip reflects "Saving…" / "Draft · saved Nm ago".
- **Undo/redo** `⌘Z` / `⇧⌘Z` across placement, resize, caption, and template changes.
- **Hover states** (to define in implementation): tray thumbnail lifts 1px + 1px light border; canvas photo shows a faint light outline; outlined buttons brighten border to `rgba(242,239,230,.34)`; accent button darkens ~6%.
- **Empty states** (not designed, needs a pass): empty library, empty gallery page, unplaced-photos filter with zero results.
- **Loading / error** (not designed): upload progress per photo, failed-upload retry, publish failure.
- **Responsive:** the editor is desktop-only (min ~1280px). The published gallery must be fully responsive — 3 col → 2 col → 1 col.

## State Management
- `gallery`: id, title, subtitle, eyebrow, slug, status (`draft` | `published`), updatedAt.
- `photos[]`: id, filename, width, height, bytes, url, caption, altText, focal {x, y}, fit (`fill` | `fit`), openFullSize, placed (bool).
- `layout`: templateId, columns, gutter, defaultRatio, and per-photo `{photoId, col, row, spanX, spanY}` cells.
- `theme`: typePairingId, titleSize, palette[], showCaptions, hoverZoom, cornerRadius.
- `editor` (transient): selectedPhotoId, activeTab (`photo` | `layout` | `page`), breakpoint, zoom, dragState, snapGuides[], history stack.
- Data needs: list/upload photos (multipart, progress events), read/write gallery + layout + theme, publish (writes the public page).

## Assets
None. All photo positions are CSS gradient placeholders; all icons are text glyphs (`✕`, `+`, `⇧`, `⌘`) — substitute the codebase's icon set. Fonts are Google Fonts **IBM Plex Sans** (300/400/500/600) and **IBM Plex Mono** (400/500); use the codebase's own font loading. Instrument Serif appears only in alternate `2a`.

Replace the placeholders with real photography before any user-facing build.

## Files
- `Gallery Builder Mockups.dc.html` — all six views. Turn 1 (lower section) holds the four core dark screens `1a`–`1d`; turn 2 (upper section) holds alternates `2a` and `2b`. Each view's wrapper carries its id (`1a`, `1b`, …).
- `support.js` — runtime for the prototype file. Not part of the design; not needed in the target codebase.
