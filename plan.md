# Laika prototype plan

Local-first photo catalog and RAW editor. Desktop, cross-platform, Rust, GPUI.
Three screens: Library, Develop, Publish. The Map screen is dropped; its EXIF and
storage rail folds into Library.

Design reference: `design_handoff_laika_ui/` (README has exact tokens and dimensions).

## Prototype definition of done

A user can:

1. Import a folder of RAW files (DNG, ARW, RAF, CR3) and browse them in the grid.
2. Rate, pick, filter and select photos. Status bar and selection count are live.
3. Open a photo in Develop, drag the twelve Basic sliders with a live preview,
   see before/after split, step through history, copy and paste settings.
4. Have every edit written as an `.xmp` sidecar next to the original and reloaded
   on restart.
5. Sync originals and sidecars to an S3-compatible bucket in the background, with
   per-photo and global sync state shown in the UI.
6. Open Publish, configure a static gallery, build it to a local folder, open it in
   a browser. Deploy via `wrangler` as a subprocess.

Out of scope for the prototype: Map, Compare view, Heal / Mask / Red-eye tools,
Tone Curve / Color Mix / Detail / Optics panels, client proof links, password
protection, Loupe view beyond a single-image display.

## Stack

| Concern | Choice | Notes |
|---|---|---|
| UI | `gpui` | Pin a crates.io version. Tailwind-style API on Taffy flexbox. |
| RAW decode | `rawler` | Pure Rust, no C toolchain. LibRaw via `libraw-rs` is the fallback if camera coverage or demosaic quality is insufficient. |
| Develop pipeline | `wgpu` + WGSL | Runs on a dedicated render thread. Same shaders for preview and export. |
| Image encode | `image` (JPEG), `ravif` (AVIF, later) | libvips is the upgrade if export speed matters. |
| Catalog | `rusqlite` (bundled) | Single file per catalog. |
| Sync | `object_store` | S3, R2, MinIO. MinIO in Docker for local dev. |
| Checksums | `blake3` | Stored per file, verified on sync. |
| EXIF | `kamadak-exif` | Read only. |
| XMP | hand-written writer over `quick-xml` | Camera Raw Settings namespace (`crs:`) so Lightroom and darktable can read the values. |
| Gallery template | `askama` | Compile-time templates, one masonry layout. |
| Async | `tokio` runtime on a background thread | GPUI has its own executor. Sync and IO run on tokio; results are posted back via `cx.spawn`. |
| Fonts | IBM Plex Sans 400/500/600, IBM Plex Mono 400/500 | Embedded with `include_bytes`, registered via the text system at startup. |

## Workspace layout

```
laika/
  Cargo.toml              # workspace
  crates/
    laika-core/           # catalog (sqlite), photo model, edit params, xmp, exif, sync queue
    laika-raw/            # rawler wrapper: decode -> linear RGB f16 + metadata
    laika-develop/        # wgpu pipeline, WGSL shaders, param -> uniform mapping, export render
    laika-export/         # derivatives, gallery build, wrangler deploy
    laika-app/            # gpui app: theme, controls, screens, state
  assets/fonts/
  fixtures/raw/           # a handful of CC0 sample RAWs from raw.pixls.us
  plan.md
```

## Phase 0. Spike (3 days, go / no-go)

Answers the two questions that could send us to Tauri.

- [x] Empty GPUI window, dark background `#0F0E0D`, IBM Plex loaded from bytes.
- [x] Render `LAIKA` wordmark at Mono 600 / 13px / `.22em` tracking and a section
      header at Mono 500 / 9.5px / `.14em`. **Check: does GPUI support letter-spacing?**
      If not, measure whether per-glyph manual spacing is viable, or accept the loss.
- [x] One slider control matching the shared spec (2px track, detent, 10px knob,
      green modified state). Drag, scrub-on-row, double-click reset, arrow nudge.
- [x] Canvas loop: wgpu renders a gradient into a 1440 x 900 texture, uniform
      driven by the slider, readback to CPU, upload to GPUI as an image element.
      **Check: 60fps sustained while dragging, with frame time logged.**
- [x] Linear gradient fills for preset swatches and thumbnail placeholders.

Go if tracking works (or is acceptably approximated) and the canvas loop holds
frame rate. Otherwise switch the UI crate to Tauri and keep everything else.

**Result: passed 2026-09-15.** See `spikes/P0-RESULTS.md`.

## Phase 1. Foundation (1 week)

- [x] Workspace scaffold, CI building on macOS and Linux.
- [x] `theme.rs`: every color token from the README as a named constant.
      Type styles as functions: `wordmark()`, `section_header()`, `list_item()`,
      `numeric()`, `metadata_key()`, etc.
- [x] App shell: 46px top bar with wordmark, module tabs, status pills, avatar.
      Fixed-height middle row. Rails at exact widths (226 / 290 in Library,
      210 / 306 in Develop).
- [x] Controls, each in its own module with the spec dimensions:
  - `slider` (from the spike), `range_slider` deferred
  - `segmented` (GRID / LOUPE / COMPARE, tool bar)
  - `chip` (filter chip, keyword chip, size chip)
  - `toggle` (26 x 14 pill)
  - `text_field` (single line, with prefix text for the URL field)
  - `list_row` (chip, name, count; active / hover backgrounds)
  - `section_header`
  - `button` (primary green, outline)
  - `histogram` (48 bars from a `[f32; 48]`)
  - `modal` (scrim + centered dialog with the single allowed shadow)
- [x] Hover states and 120ms transitions per the README.
- [x] `AppState` model: `active_module`, `selection`, `filters`, `sort`,
      `thumb_size`, `edits`, `clipboard`, `sync_state`, `publish_form`.

**Result: done 2026-09-15.** `cargo fmt --check`, `cargo test --workspace` and
`cargo check --workspace --all-targets` pass on macOS and on the Pi
(`neo.local`, Debian 13 aarch64). Notes: GPUI has no CSS transitions, so hover
flips are instant/mechanical and the 120/160ms durations are documented in
`theme::motion` only; MAP tab is a non-functional placeholder (Map screen was
dropped from the prototype); `sort` is fixed to capture-time ascending.

## Phase 2. Catalog and Library (2 weeks)

Import
- [x] Folder picker, recursive scan for RAW and JPEG extensions.
- [x] Per file, on a background pool: blake3 of the original, EXIF read,
      embedded JPEG preview extracted via rawler (no demosaic), resized to a
      long edge of 512 and 2048 into a derivative cache at
      `~/Library/Application Support/Laika/cache/<hash>/`.
- [x] Insert into SQLite. Import progress visible in the sync queue footer area.

Schema (initial)
```sql
catalogs(id, name, root_path)
photos(id, catalog_id, path, filename, blake3, captured_at, camera, lens,
       focal_mm, aperture, shutter, iso, width, height, rating, picked,
       rejected, sync_state, remote_key, imported_at)
keywords(photo_id, keyword)
edits(photo_id, params_json, history_json, cursor, updated_at)
sync_queue(photo_id, kind, state, attempts, error)
```

Library screen
- [x] Left rail: catalogs, folders (derived from path prefixes), collections
      (static for the prototype), sync queue footer.
- [x] Toolbar: view segmented control, filter chips (`★ 3+`, `Picked`,
      `RAW only`, `Unsynced`) AND-combined, sort label, thumbnail size slider
      driving column count 3 to 12.
- [x] Grid via GPUI `uniform_list` over rows of cells. Cell: 3:2 thumbnail,
      overlay bar with filename, stars, sync dot.
- [x] Selection: click, shift-click range, cmd-click add. `P` pick, `X` reject,
      `1`-`5` stars, `0` clear. `G` / `E` / `D` switch views.
- [x] Status bar with counts and breadcrumb.
- [x] Right rail: histogram from the 512 preview, shot data row, Quick Develop
      (four sliders writing to the same edit params as Develop), metadata list
      including the Storage block (local path, remote key, checksum), keywords.
- [x] Publish gallery and Export buttons wired to open the Publish modal.

**Result: done 2026-09-15.** Import verified end-to-end on macOS
(`laika --import` 12 JPEGs → catalog + 512/2048 cache, idempotent re-import,
reload on restart). `cargo test --workspace` (16 tests) and
`cargo check --workspace --all-targets` pass on macOS and on the Pi
(`neo.local`, Debian 13 aarch64). Notes:
- Cache dir on Linux is `$XDG_CACHE_HOME/laika/cache` (no `~/Library` there).
- `exif::read` only consults rawler for actual RAW files: `decode_file`
  panics (rather than `Err`) on non-RAW input, so the fallback is gated on
  `is_raw` plus `catch_unwind`; rasters get header-only dimensions via
  `image::ImageReader::into_dimensions`. Found via a crash report during the
  smoke test.
- RAW preview path proven by a synthetic DNG roundtrip test (writer →
  `extract_thumbnail_pixels`, no demosaic). No real camera RAW tested yet;
  `fixtures/raw/` is still empty.
- Loupe (`E`) shows the primary's 2048 preview; COMPARE segment and
  collections stay static/deferred. `sort` is still capture-time only.

## Phase 3. Develop (3 weeks)

Decode (`laika-raw`)
- [x] `decode(path) -> LinearImage { width, height, rgb_f16, wb_as_shot,
      cam_to_xyz, black, white }`. Demosaic on CPU once per open; cache the
      result at long edge 2048 for editing and full resolution on demand for export.
- [x] Decode time budget: under 1.5 s for a 24MP file on an M-series laptop.
      Show the 2048 embedded preview instantly while decoding.

Pipeline (`laika-develop`), one WGSL compute or fullscreen pass per stage:
1. White balance. Temperature and tint map to RGB multipliers around as-shot.
   Prototype approximation: temperature scales the R/B ratio, tint scales G.
2. Exposure, `* 2^ev`.
3. Whites and blacks, endpoint remap.
4. Highlights and shadows, luminance-masked soft compression and lift.
5. Contrast, sigmoid around middle gray.
6. Texture and clarity, local contrast from a downsampled blur pass.
   Schedule last; ship without them if the phase runs long.
7. Vibrance and saturation in Oklab.
8. Camera to display transform, tone curve, sRGB encode.

- [x] `Params` struct with `defaults()`; `is_modified(field)` drives the green state.
- [x] Render thread owns the wgpu device. Receives `(image_id, Params)` over a
      channel, coalesces to the latest, renders at preview resolution, reads
      back, posts an image to the UI. Target under 8 ms per frame.
- [x] Before/after split: render both, composite at the split fraction on GPU.
      `\` held shows full before, `Y` toggles split, divider draggable.
- [x] Export render: same pipeline at full resolution to an RGBA8 buffer.

Screen
- [x] Left rail: presets (seven, each a `Params` delta with a gradient swatch),
      hover previews, click applies as a history step. History list newest
      first, click reverts, subsequent edit truncates.
- [x] Center: letterboxed canvas at `padding: 26px`, corner labels, dimensions
      chip. Tool bar with `CROP` only functional (aspect crop, stored in params).
      Zoom presets FIT / 1:1 / 2:1.
- [x] Filmstrip: 96px, 106 x 72 cells, current photo green border, left/right
      arrows move through the selection.
- [x] Right rail: histogram from the rendered preview, `BASIC` header with reset,
      twelve sliders, collapsed panel headers (non-functional), Copy settings /
      Paste to N.
- [x] Copy/paste applies `Params` to every selected photo and writes sidecars.

**Status 2026-09-15 (in progress).** Decode, pipeline, screen wiring and XMP
are implemented against 3 real NEFs (Nikon D600, `fixtures/raw/`). Verified:
full decode 704 ms release (budget 1.5 s), cached editing decode 14 ms,
preview render 1.5–7 ms GPU at 2048 (first frame ~8 ms incl. warmup),
default/warm/split renders eyeballed against rawler's reference. 24 tests
pass on macOS and on the Pi (including NEF decode on aarch64; the cache-hit
latency budget is 2 s there vs 14 ms on the Mac). Notes and deviations:
- `cam_to_xyz_normalized()` returns NaN for these NEFs (reads a zeroed
  deprecated field); the matrix is built from `color_matrix` via rawler's own
  `normalize → pseudo-inverse` chain instead. WB **multiplies** by as-shot
  (my first version divided — green cast, caught visually).
- Vibrance/saturation run in linear, not Oklab.
- Rawler needs a 64 MB stack thread (`laika_raw::on_big_stack`); the GPUI
  pool threads overflow decoding NEFs. Tokio/rayon from the plan never
  materialized — GPUI `background_spawn` + OS threads cover it.
- Zoom presets are display-only; the split divider is display-only (not
  draggable); filmstrip arrows are clicks, not arrow keys; TOCTOU: history
  lives in memory (params persist, history steps don't).
- Not yet done: sustained-60fps drag measurement in the live app, golden
  image test (no GPU on CI), interactive drag-through of Develop.

XMP sidecars
- [x] Write `<photo>.xmp` on every committed edit (debounced 500 ms), using the
      `crs:` namespace: `Temperature`, `Tint`, `Exposure2012`, `Contrast2012`,
      `Highlights2012`, `Shadows2012`, `Whites2012`, `Blacks2012`, `Texture`,
      `Clarity2012`, `Vibrance`, `Saturation`, plus `xmp:Rating` and a
      `laika:` namespace for history and preset name.
- [x] Read sidecars on import and on catalog open. Sidecar wins over the DB.
- [x] Roundtrip test: write, read, params equal.

## Phase 4. Sync (1 week)

- [x] Settings: endpoint, bucket, region, credentials from the OS keychain
      (`keyring` crate). Test with MinIO locally.
- [x] Queue worker on tokio: uploads originals and sidecars, key layout
      `<catalog>/<yyyy>/<yyyy-mm-dd>/<filename>`. Concurrency 3. Throughput
      sampled for the footer caption.
- [x] Verification: HEAD the object, compare stored blake3 (as object metadata)
      with the local hash. `synced` only after verification.
- [x] States per photo: `local`, `pending`, `synced`, `failed`. Amber dot for
      pending, green for synced, red for failed with click-to-retry.
- [x] Sidecar changes re-enqueue the sidecar only.
- [x] `Unsynced` filter chip queries `sync_state IN ('pending','failed')`.
- [x] Top bar pill: `s3://<bucket> · synced <relative time>`.

**Result: done 2026-09-15.** Verified end-to-end against local MinIO
(`laika-minio`, `http://localhost:9000`, bucket `laika-test`):
`crates/laika-core/tests/phase4_e2e.rs` (gated on `LAIKA_SYNC_TEST=1`)
covers enqueue → claim → `upload_blocking` → verify → `complete_job`
(originals + sidecar-only re-enqueue + fail/retry), and the MinIO
`memory_roundtrip`/`minio_roundtrip` unit tests pass. `cargo test --workspace`
(30 tests) and `cargo check --workspace --all-targets` pass. Notes and
deviations:
- No standalone tokio runtime: each transfer runs `upload_blocking` on a
  short-lived current-thread runtime inside a GPUI background task; DB stays
  on the UI thread (claim/resolve → background put → UI complete/fail).
  Concurrency is 3 in-flight uploads.
- Verification is a metadata-only ranged GET (`0..1`) comparing the
  `blake3` object metadata, not a HEAD call — same guarantee, one code path.
- `remote_key` slugifies the catalog prefix (`Weddings` → `weddings`).
- Settings persist in `sync_settings` + keyring; env `LAIKA_S3_*` and
  `--s3-endpoint/--bucket/--region/--key/--secret` overlay them, `--sync-now`
  forces a full re-enqueue. Editing in the UI is read-only until U04 inputs
  land; the sync modal shows values + Test/Sync now/Retry actions.
- Retry is the footer `Retry failed (N)` button, the sync modal, and `r`;
  the red grid dot itself selects (does not retry). Top pill and footer never
  fabricate status (`backup not configured` when unset).

## Phase 5. Publish (1.5 weeks)

- [ ] Modal over the dimmed Library grid at the exact dialog spec.
- [ ] Form: title, URL slug (derived from title, editable), template (Masonry
      functional, other two selectable but render Masonry), sizes chips,
      toggles (watermark, allow downloads, strip GPS functional; others stored).
- [ ] Command log re-renders on every form change:
      `laika build --gallery <slug> --sizes 640,1280,2048`
      `laika deploy --target cloudflare-pages --project <project>`
- [ ] Estimate: bytes from selected sizes times photo count using a fixed
      bytes-per-megapixel constant; duration from a measured encode rate.
- [ ] Build: for each selected photo, render through the develop pipeline at
      full resolution, resize to each emitted size, JPEG encode at quality 86,
      optional watermark text bottom-right, EXIF stripped or GPS removed.
      Write `index.html` from the askama masonry template with a `srcset` per
      photo and a self-contained CSS block. Output to
      `~/Pictures/Laika Galleries/<slug>/`. Reveal in Finder / file manager.
- [ ] Deploy: spawn `wrangler pages deploy <dir> --project-name <project>`,
      stream stdout into the command log area. Footer shows progress instead of
      closing.
- [ ] Record `last_deploy` per gallery and show it in the footer.
- [ ] Build only, Build & deploy buttons.

## Phase 6. Polish (1 week)

- [ ] All hover and active states from the README. 120ms ease-out, 160ms panel
      expand.
- [ ] Keyboard map complete: `G E D C`, `P X 0-5`, `\ Y`, arrows, cmd-C / cmd-V
      for settings in Develop.
- [ ] Window minimum 1440 x 900. Rails fixed, center fluid.
- [ ] Crash-safe catalog writes (WAL mode, transactions per batch).
- [ ] Startup under 1 s to a populated grid on a 3,000 photo catalog.

## Threading model

- **UI thread:** GPUI. Owns `AppState`. Never blocks on IO or decode.
- **Render thread:** owns the wgpu device and queue. Single consumer of a
  `watch`-style channel carrying the latest `(image_id, Params, split)`.
- **Tokio runtime (2 to 4 workers):** import scanning, hashing, thumbnail
  generation, sync uploads, gallery encoding.
- **Decode pool (rayon):** demosaic and resize, CPU bound.

All results reach the UI via `cx.spawn` / `cx.background_spawn` and a
`cx.notify()` on the owning view.

## Risks and fallbacks

| Risk | Fallback |
|---|---|
| GPUI lacks letter-spacing | Manual per-glyph layout for the four tracked styles, or accept untracked text. |
| Readback loop can't hold 60fps | Render preview at 1024 long edge during drag, full preview on release. |
| rawler camera coverage gaps | Swap `laika-raw` internals to LibRaw. The crate boundary keeps the rest untouched. |
| Highlights/shadows look wrong | Acceptable for the prototype. Note in the UI that the pipeline is provisional. |
| GPUI API churn between versions | Pin the version for the whole prototype. Upgrade once at the end. |
| Wrangler not installed | Detect on startup, disable Build & deploy with a hint, Build only still works. |
| Tokio and GPUI executor interop friction | Keep tokio behind a small `Jobs` facade that returns `oneshot` receivers polled from GPUI's executor. |

## Testing

- Unit: `Params` default diffing, temperature/tint to multiplier mapping, XMP
  roundtrip, slug derivation, catalog queries for each filter combination.
- Golden image: decode a fixture, render with fixed params, compare against a
  stored PNG with a tolerance. Catches pipeline regressions.
- Integration: import the fixture folder into a temp catalog, assert counts,
  build a gallery, assert the file tree.
- Manual: a checklist per screen against the design board at 1440 x 900.

## Sequencing

Phase 0 gates everything. Phases 1 and 2 give a usable browser. Phase 3 is the
longest and highest risk, so start the decode work in `laika-raw` during Phase 2
in parallel. Phase 4 can run alongside Phase 3 since it touches only
`laika-core` and the Library rail. Phase 5 depends on the export render from
Phase 3.

Rough total: 10 to 11 weeks for one developer, less with the parallel tracks.
