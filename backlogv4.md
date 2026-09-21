# Laika backlog v4 — polish to ship

Updated: 2026-09-19. All items below are open.

## Scope of this document

`backlog.md` (U01–U27) made Laika trustworthy for one shoot. `backlogv2.md`
(V01–V32) added the everyday Lightroom Classic features. `backlogv3.md`
(S01–S30) made the case for switching. `photogallery.md` (G01–G28) covers the
gallery builder.

This backlog adds **no features**. It answers one question: **what stands
between the current `exp` branch and a build a stranger can download, install,
trust with their photos, and get help with?** Every item is one of:

1. **A legal or distribution blocker** — we may not, or cannot, ship without it.
2. **A way the app can crash, lose work, or produce wrong output** on inputs
   real users have.
3. **A broken promise in the UI** — a control that does nothing, a shortcut
   that isn't bound, an error nobody sees.
4. **A missing platform convention** — what a Mac user expects from any app
   before they judge this one.
5. **A gap in docs, support, or engineering hygiene** that makes the first
   public release unsupportable.

Anything that would be a new capability belongs in the earlier backlogs. Where
an earlier item already covers a polish concern (S23 notarization, S24 license,
S26 telemetry, U20 color management, U23/G28 validation), this document
narrows it to the ship-blocking slice and references the original instead of
restating it.

Numbering is Q01–Q47 (Q for quality; P would collide with the P0–P2
priorities). Format, priorities, and gates follow `backlog.md`. Findings come
from a source audit on 2026-09-19; the headline claims were re-checked against
the code, but this is not a claim of live-app verification. `main.rs` line
numbers are as of that audit and will drift.

## What the audit found, in one table

| Area | Already solid — do not re-backlog | Gap this backlog addresses |
| --- | --- | --- |
| Legal | Zero git deps, lockfile committed, gpui pinned | No LICENSE; IBM Plex OFL text not shipped in the app or in published galleries; `rawler` is LGPL-2.1 statically linked; no third-party notices |
| Distribution | Bundle script, `.icns`/`.ico` complete, nightly + tagged releases, version + git rev in About | Ad-hoc signed (Gatekeeper rejects the DMG), no hardened runtime, arm64 only, no `[profile.release]`, no updater, 76 MB of sample NEFs in the bundle, bare DMG |
| Crash safety | `exif.rs` isolates rawler panics; panic hook writes crash reports; zero `TODO`/`todo!()` in the tree | `decode.rs` calls the same rawler entry point unguarded; wgpu errors panic the render thread; `expect` in render paths; ⌘Q kills a running import |
| Data safety | `write_atomic` sidecars, blake3-verified copies, WAL + busy timeout + FK, backup-before-migrate, deletes go to Trash, resumable sync queue, keyring secrets, scrubbed rotating logs | Preview cache and `library.json` written non-atomically; `read_version` swallows errors; failed catalog removes are ignored; no free-space preflight |
| Output | Correct camera→sRGB math, orientation baked, GPS cannot reach exports, collision policy defaults to Suffix | No ICC profile in any export; 8192 px texture limit breaks 60 MP+ exports; XMP on JPEG only; two publish toggles wired to nothing |
| Mac feel | Command registry (70+ commands, one dispatcher), native + in-window menu bars, destructive-action confirm modal, polished import HUD, rails/text-scale persisted | Only 4 real key bindings — the rest are strings painted on menus; no Window menu, ⌘W, or text Cut/Copy/Paste; window frame not persisted; no file drop; one context menu |
| Feedback | Human, actionable error copy; good empty states in grid/map/gallery/compare | 203 `status_note` one-liners with no history; 72 `eprintln!` failures no windowed user sees; HUD shows two job types; several long jobs can't be cancelled |
| Accessibility | Text scale 85–150 %, contrast pass (U22), min window 1280×800 | ARIA labels in 7 of ~25 UI files; no tab order; no reduced motion |
| Docs & support | `docs/format/*` spec, first-run welcome with samples, Reveal Logs + Copy Diagnostics, catalog-problem screens | No user manual, FAQ, privacy statement, CHANGELOG, SECURITY.md; Help menu links nowhere; README contradicts shipped features; About has no credits |
| Hygiene | ~330 tests in 53 modules, GPU pixel tests, golden gallery snapshots, macOS + Linux + Windows CI, one justified crate-level `allow` | Clippy never runs; no cargo-deny/audit; no fuzzing; `main.rs` is 30k lines with 12 tests; backlog statuses contradict the code |

Source anchors: `scripts/bundle-macos.sh`, `.github/workflows/{ci,release}.yml`,
root `Cargo.toml`, `crates/laika-raw/src/{decode,exif,video}.rs`,
`crates/laika-develop/src/lib.rs`, `crates/laika-export/src/{formats.rs,site/}`,
`crates/laika-core/src/{catalog,sync,import,logging,prefs}.rs`, and
`crates/laika-app/src/{main,commands,diagnostics,prefs_ui,progress_hud}.rs`.

## Priorities and delivery order

- **P0 — Cannot ship without it:** legal exposure, Gatekeeper rejection,
  crashes and data loss on ordinary inputs, wrong output, controls that lie.
- **P1 — Ship is embarrassing without it:** platform conventions, error
  visibility, docs, update path, hygiene gates that keep P0 fixed.
- **P2 — First point release:** accessibility depth, consistency passes,
  Windows parity, refactors that make the next year cheaper.

| Milestone | Exit condition | Items |
| --- | --- | --- |
| A. Allowed to ship | A lawyer-readable LICENSE and notices file ship in every artifact Laika produces; the LGPL question has a written answer | Q01–Q04 |
| B. Installs like a Mac app | A downloaded DMG opens with no Gatekeeper prompt on a clean Apple-silicon and Intel Mac, and can update itself | Q05–Q11 |
| C. Survives real input | The fault-injection suite (corrupt RAW, 100 MP file, GPU error, disk full, ⌘Q mid-import) ends with a message, never a crash or a lie | Q12–Q21 |
| D. Output is right | An export opened in Preview, Photoshop, and a browser looks the same; every publish toggle does what it says | Q22–Q25 |
| E. Feels native | Every menu item shows and honours its key equivalent; window, drop, and Window-menu conventions pass a checklist | Q26–Q32 |
| F. Tells you what happened | No user-visible failure goes only to stderr; every long job is visible and cancellable | Q33–Q36 |
| G. Supportable | A new user can find the manual, the privacy statement, the changelog, and where to report a bug from inside the app | Q37–Q41 |
| H. Stays fixed | CI enforces clippy, licenses, advisories, and fuzz smoke; backlog statuses match the code | Q42–Q47 |

Milestones A–D are P0. E–G are P1. H is P1 for the CI gates and P2 for the
rest.

## Section A — Allowed to ship (P0)

### [ ] Q01 — Choose and apply a license

- **Problem:** No `LICENSE` at the repo root and no `license` field in any of
  the six crate manifests, yet binaries are already published on GitHub
  Releases. Nobody has been granted the right to use them.
- **Deliver:** Decide the license (this is S24's decision — make it now, the
  pricing and comparison page can wait). Add `LICENSE`, set `license` /
  `license-file` in every `Cargo.toml`, add `NSHumanReadableCopyright` to the
  Info.plist and a copyright line to About and the README.
- **Done when:** `cargo metadata` reports a license for every workspace member;
  the `.app`, the Windows zip, and the README all name the same license; the
  choice is compatible with the answer to Q02.
- **Depends on:** Q02 (the choices constrain each other). **Relation:** narrows
  S24. **Touchpoints:** repo root, `crates/*/Cargo.toml`,
  `scripts/bundle-macos.sh`, `diagnostics.rs` About.

### [ ] Q02 — Resolve the `rawler` LGPL-2.1 obligation

- **Problem:** `rawler` 0.8 (`crates/laika-raw/Cargo.toml:7`) is LGPL-2.1-only
  and is statically linked as the RAW decode core. Static linking triggers
  LGPL §6: recipients must be able to relink against a modified library. A
  closed, statically linked binary does not meet that today.
- **Deliver:** A written decision, then its implementation. The realistic
  options: (a) license Laika under an LGPL-compatible open-source license so
  §6 is satisfied by the source; (b) isolate `rawler` behind a dynamic library
  or helper process (`laika-render` shows the shape) and ship relink
  instructions; (c) publish relinkable object files with each release;
  (d) replace the decoder. Get the decision reviewed by someone qualified —
  this document is not legal advice.
- **Done when:** `docs/licensing.md` records the decision and how each release
  artifact satisfies it, and Q43's license gate encodes it so a future
  dependency bump can't silently reopen the question.
- **Depends on:** none. **Touchpoints:** `laika-raw`, release workflow, Q01.

### [ ] Q03 — Ship font licenses in the app and in every published gallery

- **Problem:** `assets/fonts/` holds five IBM Plex TTFs and no license; the OFL
  text lives only in the dead `spikes/p0/assets/fonts/`.
  `scripts/bundle-macos.sh:25` and `bundle-windows.ps1:33` copy the fonts
  blind, and `site/build.rs:449-455` copies them into every gallery a user
  publishes — so users redistribute OFL fonts without the required notice.
- **Deliver:** Put `OFL.txt` beside the fonts in `assets/fonts/`; copy it in
  both bundle scripts; emit it next to the fonts in the gallery build; add a
  golden-test assertion that a built site contains it.
- **Done when:** The DMG, the Windows zip, and a freshly built gallery each
  contain the OFL text next to the fonts, and the site test fails if it's
  removed.
- **Depends on:** none. **Touchpoints:** `assets/fonts/`, bundle scripts,
  `laika-export/src/site/{build.rs,tests.rs}`.

### [ ] Q04 — Third-party notices, generated not hand-written

- **Deliver:** Generate a third-party license file from the lockfile
  (`cargo-about` or equivalent) at bundle time, ship it in
  `Contents/Resources/`, and open it from an "Acknowledgements" button in
  About and a Help menu item. Include the fonts (Q03), the map tile
  attribution, and anything vendored.
- **Done when:** The file regenerates in CI from `Cargo.lock`, covers all ~900
  packages, and the release job fails if generation fails or an unknown
  license appears.
- **Depends on:** Q01, Q43. **Touchpoints:** release workflow, bundle scripts,
  `diagnostics.rs`, `commands.rs` Help menu.

## Section B — Installs like a Mac app (P0)

### [ ] Q05 — Developer ID signing, hardened runtime, notarization

- **Problem:** `bundle-macos.sh:53` signs ad-hoc. `spctl -a` rejects the
  published DMG. There is no entitlements file, so notarization is currently
  impossible, not just skipped.
- **Deliver:** Sign with a Developer ID certificate and `--options runtime`;
  add a minimal entitlements file (justify each entitlement in a comment — wgpu
  and the Photos/keychain bridges are the likely ones); notarize and staple the
  app and the DMG in `release.yml` using repository secrets; keep the ad-hoc
  path for local builds.
- **Done when:** On a Mac that has never seen Laika, the downloaded DMG opens
  and the app launches with no Gatekeeper dialog beyond the standard
  first-open confirmation; `spctl -a -vv` reports "Notarized Developer ID";
  import, export, keychain, Apple Photos, and map tiles still work under the
  hardened runtime.
- **Depends on:** Q01. **Relation:** the signing half of S23.
  **Touchpoints:** `scripts/bundle-macos.sh`, `.github/workflows/release.yml`,
  new `scripts/entitlements.plist`.

### [ ] Q06 — A real release profile and symbolicated crash reports

- **Problem:** Root `Cargo.toml` has no `[profile.release]`. The shipped binary
  is 50 MB, unstripped, without LTO, and crash reports from
  `logging.rs` are raw addresses nobody can symbolicate.
- **Deliver:** Add a release profile (`lto = "thin"`, `codegen-units`,
  `strip`, `split-debuginfo = "packed"`; keep `panic = "unwind"` — Q12 and
  `exif.rs` depend on `catch_unwind`). Archive the dSYM / PDB as a release
  asset keyed by git rev. Add a `scripts/symbolicate` helper that takes a crash
  report and a rev.
- **Done when:** Binary size and cold-start time are recorded before and after;
  a deliberately triggered panic in a release build produces a report that the
  helper turns into file:line frames; release gates from `backlog.md` still
  pass.
- **Depends on:** none. **Touchpoints:** root `Cargo.toml`, `release.yml`,
  `laika-core/src/logging.rs`.

### [ ] Q07 — Universal binary and an honest minimum macOS

- **Problem:** `bundle-macos.sh:11` builds for `uname -m` only, so releases are
  arm64-only and the README never says Intel is unsupported. The Info.plist
  claims `LSMinimumSystemVersion 13.0` but the binary's `minos` is 11.0 and
  `MACOSX_DEPLOYMENT_TARGET` is never set.
- **Deliver:** Build both targets and `lipo` them (or state Apple-silicon-only
  in the README, the release notes, and an `LSArchitecturePriority` /
  launch-time message — pick one). Set `MACOSX_DEPLOYMENT_TARGET` to the
  version actually tested and make the plist match.
- **Done when:** The release asset runs on one Intel and one Apple-silicon Mac
  at the stated minimum OS, or the unsupported case fails with a clear message
  instead of a Finder error.
- **Depends on:** Q05. **Touchpoints:** `bundle-macos.sh`, `release.yml`,
  README system requirements.

### [ ] Q08 — Info.plist and version hygiene

- **Deliver:** A monotonically increasing integer `CFBundleVersion` separate
  from `CFBundleShortVersionString` (nightly currently injects
  `0.1.1-nightly.20260919.abc1234`, which is invalid there). Add
  `NSHumanReadableCopyright`, `LSApplicationCategoryType`, usage-description
  strings for every protected resource the app touches (Photos library,
  removable volumes, network volumes), and `CFBundleDocumentTypes` for Laika
  catalogs and the supported RAW/image types, with an open-document handler so
  Open With and drop-on-Dock work (see Q30).
- **Done when:** `plutil -lint` passes; every TCC prompt shows Laika-written
  copy; double-clicking a catalog opens it; "Open With → Laika" on a RAW file
  starts an import of that file.
- **Depends on:** Q05. **Touchpoints:** `bundle-macos.sh:29-49`, `main.rs`
  app-launch path.

### [ ] Q09 — A DMG people know how to use, without 76 MB of samples

- **Problem:** The DMG root contains only `Laika.app` — no `/Applications`
  symlink, background, or layout — and the bundle embeds three sample NEFs
  (76 MB of a 98 MB download).
- **Deliver:** DMG with an Applications alias, background, and fixed window
  layout. Replace the bundled NEFs with small samples (or download-on-demand
  from the welcome screen with a size shown), keeping the first-run "try it
  with samples" path working offline if samples stay bundled.
- **Done when:** DMG is under 40 MB; drag-to-Applications is obvious without
  instructions; first-run samples still work.
- **Depends on:** Q46 for fixture provenance. **Touchpoints:**
  `bundle-macos.sh:20-27,58`, `diagnostics.rs` welcome.

### [ ] Q10 — Update path

- **Problem:** `prefs_ui.rs:236-240` says "rebuilding from source is how you
  update". A DMG user has no way to learn a fix exists, including a security
  fix.
- **Deliver:** The smallest honest version first: "Check for Updates…" in the
  app menu and Preferences that fetches a signed release manifest, compares
  versions, shows the release notes, and opens the download. Then the full
  S23 updater (Sparkle or equivalent, signature-verified, rollback-safe). The
  check is opt-in on first run and listed in the privacy statement (Q39).
- **Done when:** A build one version behind reports the newer version and its
  notes; with checks off, Q39's egress test sees zero requests.
- **Depends on:** Q05, Q40. **Relation:** the updater half of S23.
  **Touchpoints:** `prefs_ui.rs`, `commands.rs`, release workflow.

### [ ] Q11 — Quit, close, and second-instance behaviour

- **Problem:** ⌘Q calls `cx.quit()` unconditionally (`commands.rs:530-535`,
  `:602-605`); `flush_saves()` does not drain jobs. Quitting during a
  copy-mode import off a card can leave a half-copied folder that disagrees
  with the catalog. There is no single-instance guard: a second launch opens a
  permanently catalog-less window.
- **Deliver:** On quit with running import/export/backup/publish jobs, show a
  sheet naming them: Cancel, Quit After Jobs Finish, Quit Now (jobs cancelled
  cleanly via their existing cancel tokens). Second launch activates the
  existing instance and forwards any file arguments to it.
- **Done when:** ⌘Q mid-import never leaves a `.part-` file or a catalog row
  without its file; launching twice yields one window.
- **Depends on:** none. **Touchpoints:** `commands.rs`, `main.rs` job state,
  `activity.rs`.

## Section C — Survives real input (P0)

### [ ] Q12 — Isolate every rawler call, not just the EXIF one

- **Problem:** `laika-raw/src/exif.rs:154,175` wraps `rawler::decode_file` in
  `catch_unwind` with a comment that rawler panics on malformed input.
  `decode.rs:55` calls the same function bare. A corrupt RAW survives import
  and then kills the app in Develop or export. `decode_error_message` already
  branches on `"panic"`, which is unreachable today.
- **Deliver:** One `guarded_decode` helper used by both files; map a caught
  panic to the existing human message; add the corrupt fixtures to the decode
  tests.
- **Done when:** Truncated, zero-byte, and bit-flipped RAW fixtures each
  produce the "damaged RAW" explanation in Loupe, Develop, and export, and the
  app keeps running.
- **Depends on:** none. **Touchpoints:** `laika-raw/src/{decode,exif}.rs`,
  fixtures.

### [ ] Q13 — Exports larger than 8192 px

- **Problem:** `laika-develop/src/lib.rs:652` calls
  `request_device(&Default::default())`, which accepts wgpu's default
  `max_texture_dimension_2d` of 8192, and `adapter.limits()` is never read.
  Full-resolution export uploads with no clamp or tiling, so 61 MP (9504 px),
  100 MP (11648 px), and any panorama fail validation. Previews are safe
  (clamped).
- **Deliver:** Request the adapter's real limits; tile the develop pass when
  the image still exceeds them (overlap sized for the widest spatial kernel —
  clarity, sharpening, masks, and upright all need care at seams); cap and
  explain when even tiling cannot fit in memory (see Q15).
- **Done when:** A 100 MP fixture and a 20000 px panorama export correctly;
  a tiled export is pixel-identical (within the existing CPU-mirror tolerance)
  to an untiled one at a size both can do.
- **Depends on:** none. **Touchpoints:** `laika-develop/src/lib.rs`,
  `develop.wgsl`, GPU tests.

### [ ] Q14 — GPU errors and device loss don't end the session

- **Problem:** No `on_uncaptured_error` and no device-lost handler. wgpu's
  default handler panics; the single render thread dies while `self.dev` stays
  `Some`, and every later render fails with an opaque channel-disconnect
  string. `main.rs` also `.expect("open window")`s on GPU failure at launch.
- **Deliver:** Install error and device-lost callbacks; on loss, tear down and
  recreate the device once, re-queue the in-flight job, and tell the user if
  recovery fails. Replace the launch `expect` with a native alert explaining
  that no compatible GPU was found and where the log is.
- **Done when:** A forced validation error and a simulated device loss each
  end in a recovered render or a readable message; sleep/wake and external-GPU
  unplug during export don't require a relaunch.
- **Depends on:** none. **Touchpoints:** `laika-develop/src/lib.rs:443-1065`,
  `main.rs` window open.

### [ ] Q15 — Memory ceiling for huge files and stray helpers

- **Deliver:** Full-res decode holds roughly three copies (f32 → f16 → RGBA +
  readback), over 2 GB at 100 MP. Drop intermediate buffers as soon as they're
  consumed, set `image::Limits` on untrusted raster decode paths
  (`preview.rs:42,61`), and refuse with a clear message above a stated
  megapixel ceiling rather than being killed by the OS. Make
  `run_helper_timeout` (`video.rs:351-362`) kill and reap the child it
  abandons, so a stalled `qlmanage`/`ffmpeg` doesn't leak per timeout.
- **Done when:** Peak RSS for a 100 MP export is measured and documented in
  system requirements; a decompression-bomb PNG is rejected; no orphan helper
  processes remain after 50 forced timeouts.
- **Depends on:** Q13. **Touchpoints:** `laika-develop`, `laika-raw`.

### [ ] Q16 — Remove the panics reachable from a click

- **Problem:** Production `unwrap`/`expect` counts are low, but several sit on
  user paths: `main.rs` ~6580 `RgbaImage::from_raw(..).expect("render dims")`;
  ~22178/22186 `self.state.primary.unwrap()` on tone-curve double-click with no
  selection; ~16215 eyedropper `shown.clone().unwrap()`; ~10477
  `inserted.unwrap()` on import; `import_dialog…expect("dialog")` in render;
  `expect("open gallery")` in `gallery_inspector.rs` (5 sites) and
  `gallery_canvas.rs` (4 sites) while `gallery_inspector.rs:1171` handles the
  same case safely; `lock().unwrap()` poison cascades in
  `gallery_publish.rs:648-676`.
- **Deliver:** Convert each to an early return, an empty state, or a reported
  error. Wrap the top-level job runners in `catch_unwind` so a panicking
  background job reports failure in the HUD instead of silently vanishing. Add
  `clippy::unwrap_used`/`expect_used` as warnings in the app crate so new ones
  are visible in review (Q42).
- **Done when:** Each listed site has a test or a manual repro that no longer
  panics; a panic injected into an export job surfaces as a failed job with
  "details in the log".
- **Depends on:** none. **Touchpoints:** `main.rs`, `gallery_*.rs`,
  `slideshow.rs`, `controls/text_input.rs`.

### [ ] Q17 — Failed catalog writes must not look like success

- **Problem:** `main.rs` ~5085 and ~14110 call `cat.remove_photo(id).ok()` —
  the photo disappears from the UI while possibly remaining in the catalog,
  and reappears on next launch. Similar silent drops: custom folder/rename
  templates not saved (~9373, ~9381), an unwritable cache directory accepted in
  Preferences (~8438), backup settings (~5429), `gallery_ui.rs:420`,
  `albums.rs:319`, the Lightroom import report (`lightroom_ui.rs:512`).
- **Deliver:** Audit every `.ok()` / `let _ =` on a catalog or filesystem write
  in `laika-app`. Each becomes: update the UI only on success, and report the
  failure through Q33. Validate a chosen cache directory by writing a probe
  file before accepting it.
- **Done when:** With the catalog made read-only mid-session, every one of
  these actions reports a failure and the UI still matches the database after
  relaunch.
- **Depends on:** Q33. **Touchpoints:** `main.rs`, `albums.rs`,
  `gallery_ui.rs`, `lightroom_ui.rs`, `prefs_ui.rs`.

### [ ] Q18 — Atomic writes for the last two non-atomic files

- **Problem:** Preview cache files are written with bare `fs::write`
  (`catalog.rs:1662,2365,6236`) and readers only check `.exists()`; a crash or
  ENOSPC leaves a 0-byte preview that `already_imported` trusts forever, with
  no repair path. `library.json` (prefs, recents, launch catalog) is written in
  place (`catalog.rs:586-589`) and the reader silently falls back to `Default`,
  so preferences vanish without a word — while `xmp::write_atomic` sits next
  door.
- **Deliver:** Route both through `write_atomic`. Treat zero-length or
  undecodable cached previews as missing and rebuild them. On an unreadable
  `library.json`, keep the bad file as `library.json.corrupt-<date>`, tell the
  user once, and continue with defaults. Add a parent-directory fsync to
  `write_atomic` and the three other temp+rename paths (`import.rs:437`,
  `exit_bundle.rs:608`, `sync.rs:703`).
- **Done when:** Killing the process during a preview build or a prefs save
  never leaves a file the next launch trusts; a planted 0-byte preview is
  rebuilt on view.
- **Depends on:** none. **Touchpoints:** `laika-core/src/{catalog,xmp}.rs`.

### [ ] Q19 — Migration version read and stamp are crash-safe

- **Deliver:** `read_version` (`catalog.rs:1191-1194`) uses `.or(Ok(0))`, so
  any rusqlite error — including SQLITE_BUSY — reads as "pre-versioning" and
  replays all 14 migrations. Distinguish "no version row" from "error" and
  propagate the error. Move `stamp_version` (`:1506-1507`) inside the final
  migration transaction so a crash can't land between migrate and stamp.
- **Done when:** A test holding a write lock during open gets a "catalog is
  busy" error, not a migration run; a crash injected after the last migration
  step leaves a catalog that opens without replaying.
- **Depends on:** none. **Touchpoints:** `catalog.rs` `migrations`.

### [ ] Q20 — Disk-full, read-only, and offline volumes are named, not guessed

- **Deliver:** A free-space preflight before import copy, export, exit bundle,
  gallery build, and catalog backup, using the existing `capacity()` probe
  (`import.rs:46-60`), with the shortfall stated in the dialog before the job
  starts. Classify `ENOSPC`/`EACCES`/`EROFS`/`ENOENT`-on-volume from
  `raw_os_error()` in one helper and replace the two hard-coded guesses
  (`main.rs` ~8011, `site/build.rs:301`). Export gains the resume-friendly
  behaviour import already gets from content-hash dedupe: re-running an
  interrupted export with "Skip" finishes the remainder without re-rendering.
- **Done when:** Import to a nearly full disk is refused up front with the
  numbers; export to a read-only volume says "read-only", not "failed"; an
  export interrupted at 60 % completes the remaining 40 % on retry.
- **Depends on:** Q33. **Touchpoints:** `import.rs`, `main.rs` export,
  `laika-export`, `exit_bundle.rs`.

### [ ] Q21 — Backup endpoints: no silent cleartext, no predictable `/tmp` socket

- **Deliver:** `sync.rs:327` sets `with_allow_http(true)` and the endpoint
  validator (`controls/text_input.rs:389`) accepts `http://` for any host.
  Allow plain HTTP only for loopback and private-range hosts, or behind an
  explicit "I understand this is unencrypted" confirmation that the backup
  panel keeps showing. Move the SSH `ControlPath` (`sync.rs:544`,
  `/tmp/laika-ssh-%C`) into a 0700 per-user directory. Give the S3 client an
  explicit `RetryConfig` with backoff, and make request timeouts explicit.
  Land the in-progress SFTP pull-restore (`sync.rs`, uncommitted on `exp`)
  with its tests, and confirm the dropped trailing `-o` in `ssh_args` was the
  intended fix.
- **Done when:** A remote `http://` endpoint cannot be saved without the
  warning; the control socket path is not world-predictable; a flaky-network
  test shows retries with backoff, then a clear failure.
- **Depends on:** none. **Relation:** U24 stays the owner of restore drills.
  **Touchpoints:** `laika-core/src/sync.rs`, backup UI.

## Section D — Output is right (P0)

### [ ] Q22 — Embed an ICC profile in every export

- **Problem:** No format writes a profile — no JPEG APP2, PNG `iCCP`, TIFF tag
  34675, or AVIF/WebP colour box (`formats.rs:194-302`). The pixel math is
  correct sRGB, but untagged files render differently across viewers and wide
  gamut displays, and print labs reject them.
- **Deliver:** Embed a compact sRGB profile in all five formats (the `image`
  encoders already accept one for TIFF; add the container writers for the
  rest). This is the shippable slice of U20 — wider output spaces, per-display
  transforms, and soft proofing stay there.
- **Done when:** `exiftool -icc_profile:all` reports sRGB on an export in every
  format; the same export looks identical in Preview, Photoshop, Safari, and
  Chrome on a P3 display; gallery derivatives are tagged too.
- **Depends on:** none. **Relation:** narrows U20. **Touchpoints:**
  `laika-export/src/formats.rs`, `site/build.rs`.

### [ ] Q23 — Metadata on export is a choice, in every format

- **Deliver:** XMP is embedded on JPEG only (`main.rs` ~7981-8003); copyright
  and creator vanish silently on TIFF/PNG/WebP/AVIF. Embed in every format
  that has a container for it, and replace the implicit behaviour with one
  export option: All / Copyright & contact only / None. The export summary
  states what was written.
- **Done when:** Copyright survives a round trip through each format; "None"
  produces a file with no XMP/EXIF beyond what the decoder needs; the dialog
  never offers a choice the format can't honour.
- **Depends on:** none. **Touchpoints:** `main.rs` export, `formats.rs`,
  `xmp.rs`.

### [ ] Q24 — Remove or wire the two publish toggles that do nothing

- **Problem:** "Password protect" and "Strip GPS from EXIF" in the publish form
  (`main.rs` ~23043-23058; `state.rs:579-580`) flip a boolean nothing reads.
  One promises access control that does not exist. `photogallery.md`'s own
  open decisions say password protection stays disabled.
- **Deliver:** Remove "Password protect" until a real implementation exists.
  Replace "Strip GPS" with a static line stating the truth ("Published images
  never contain location data") since no EXIF is written. Then run a U01-style
  truthfulness sweep over everything added since U01 closed — Map, Lightroom
  mode, Apple Photos sync, gallery builder (G25), Preferences — for any other
  control without a consumer.
- **Done when:** A script (or review checklist) maps every toggle in `state.rs`
  to at least one read site; none are orphaned.
- **Depends on:** none. **Relation:** extends U01; absorbs G25.
  **Touchpoints:** `main.rs`, `state.rs`, `gallery_inspector.rs`.

### [ ] Q25 — Published gallery finish

- **Deliver:** Open Graph / Twitter card tags and a favicon (`html.rs:116-144`);
  `lang` from a gallery setting instead of hard-coded `en`; stop publishing
  internal catalog IDs in the web-root `manifest.json` (`build.rs:427`); honour
  "captions off" inside the lightbox (`gallery.js:23`); keep prev/next
  reachable under 640 px (`gallery.css:64`); optional 4:4:4 chroma for
  high-quality JPEG (`formats.rs:125`) if the encoder allows.
- **Done when:** A shared gallery link unfurls with title and image in
  Messages and Slack; the golden tests cover each fix; the existing
  no-external-URL test still passes.
- **Depends on:** Q03. **Touchpoints:** `laika-export/src/site/`.

## Section E — Feels native (P1)

### [ ] Q26 — One source of truth for shortcuts, actually bound

- **Problem:** `commands.rs` holds a good registry (70+ commands) but only four
  real bindings exist (⌘Q, ⌘,, ⌘H, ⌥⌘H at `commands.rs:506-517`). Every other
  shortcut is a string painted into the menu title (`:296-310`) and
  re-implemented in a 360-line inline `match` (`main.rs` ~25445-25805). Native
  menu items show no key equivalent, shortcuts die when focus is in a field,
  and the `?` cheat sheet (`main.rs` ~29200) is a third hand-maintained list.
- **Deliver:** Generate gpui `KeyBinding`s, native menu key equivalents, and
  the cheat sheet from the registry. Delete the inline match. Add a test that
  fails on duplicate bindings within a context. Resolve the known conflicts:
  `/` vs `?`; bare `R` (Retry Uploads) and `S` (Backup) colliding with
  Lightroom muscle memory and with `lightroom_mode.rs:1024`; give Copy/Paste
  Settings, the purple label, and Compare/Survey default bindings.
- **Done when:** Every menu item shows its shortcut natively; the cheat sheet
  has no literal key strings; the duplicate-binding test passes; the README
  shortcut table is generated from the same source (Q37).
- **Depends on:** none. **Relation:** finishes what U03/V31 started.
  **Touchpoints:** `commands.rs`, `main.rs` key handler, cheat sheet.

### [ ] Q27 — The standard macOS menus

- **Deliver:** A Window menu (Minimize ⌘M, Zoom, Bring All to Front, window
  list); ⌘W; Services and Show All in the app menu; Edit-menu Cut/Copy/Paste/
  Select All that route to the focused text field (today clipboard handling is
  buried in `field_key`, `main.rs` ~4565-4580) and to Copy/Paste Settings
  otherwise; Enter Full Screen on ⌃⌘F (and Globe-F) in addition to `F`; About
  listed once, not twice (`commands.rs:357,469`); a Help menu with search
  and real links (Q38).
- **Done when:** The HIG menu checklist passes; ⌘C in a rename field copies
  text and ⌘C on a photo copies settings; full-screen state is a real macOS
  space.
- **Depends on:** Q26. **Touchpoints:** `commands.rs`.

### [ ] Q28 — Remember the window

- **Deliver:** The window opens centred at 1440×900 on every launch (`main.rs`
  ~29616). Persist frame, display, full-screen state, and last module and
  selection alongside the rail widths that already persist (`prefs.rs:78-84`).
  Clamp a restored frame onto a display that still exists.
- **Done when:** Quit on an external display in full screen, relaunch: same
  place. Unplug the display, relaunch: window is fully on the built-in screen.
- **Depends on:** none. **Touchpoints:** `main.rs` window open, `prefs.rs`.

### [ ] Q29 — One undo stack that says what it will undo

- **Problem:** Three disjoint stacks arbitrated by mode flags (`main.rs`
  ~5116-5215): develop edits, metadata batches, remove-from-catalog. ⌘Z after
  mixed actions is unpredictable, and the Edit item never names its target.
  Collection create/delete/rename, add/remove from collection, album reorder,
  and preset delete aren't undoable at all.
- **Deliver:** A single ordered history of named actions feeding "Undo
  <Action>" / "Redo <Action>" in the Edit menu. Bring collection and album
  membership, reorder, and preset delete into it. Anything that stays
  non-undoable gets a confirmation instead (Q31).
- **Done when:** Rate, edit, add to collection, remove from catalog, then four
  ⌘Z presses reverse them in order with the right menu titles each time.
- **Depends on:** none. **Touchpoints:** `main.rs`, `collections.rs`,
  `albums.rs`, `presets_ui.rs`, `commands.rs`.

### [ ] Q30 — Drop files on it

- **Deliver:** There is no `on_drop`/`ExternalPaths` handler anywhere in the
  app; import is only reachable through `prompt_for_paths`. Accept files and
  folders dropped on the window and the Dock icon (and "Open With", via Q08),
  opening the import dialog pre-filled. Dropping onto a collection row adds
  after import. Show a drop highlight; reject unsupported types with a reason.
- **Done when:** Dragging a card folder from Finder onto the grid, the Dock
  icon, or a collection each does the obvious thing.
- **Depends on:** Q08. **Touchpoints:** `main.rs` root view, import dialog.

### [ ] Q31 — Confirm what can't be undone

- **Deliver:** Reuse the existing confirm modal (`main.rs` ~994-1000) for:
  delete collection (`collections.rs:997,1032`), Clear Quick Collection
  (`:950`), delete gallery, delete preset (`presets_ui.rs:623`), Reset All
  edits on a multi-selection, and Preferences "Reset to Defaults"
  (`prefs_ui.rs:707-718`, currently global and instant). Skip the confirmation
  wherever Q29 makes the action undoable.
- **Done when:** No single click destroys user-authored data without either an
  undo or a confirmation naming what will be lost.
- **Depends on:** Q29. **Touchpoints:** as listed.

### [ ] Q32 — Context menus, double-click, tooltips, and empty states everywhere

- **Deliver:** Context menus exist only on photo cells (`main.rs`
  ~25888-26212). Add them for folders, collections, keywords, presets,
  galleries, and the filmstrip, built from the command registry. Double-click
  to rename on collection, preset, and keyword rows. Tooltips on every
  icon-only control in `lightroom_ui.rs`, `batch_edit.rs`, `zoom.rs`, and
  `diagnostics.rs` (currently none). Empty states for the collections sidebar,
  albums, filmstrip, text-search results, and presets list, following the
  filter-miss pattern with its Reset action (`main.rs` ~12900-12938).
- **Done when:** Right-click does something sensible on every list row; no
  icon-only button lacks a tooltip; no list renders as blank space.
- **Depends on:** Q26. **Touchpoints:** sidebar modules, `presets_ui.rs`,
  `gallery_ui.rs`.

## Section F — Tells you what happened (P1)

### [ ] Q33 — One notification surface, with history

- **Problem:** Failures reach the user through `status_note` — 203 assignments
  in `main.rs` to a single transient status-bar line with no severity and no
  way to re-read it. A real toast component exists but is wired to two call
  sites (`lightroom_mode.rs:580-610`, slideshow). 72 `eprintln!` calls send
  failures to a stderr no windowed user sees — decode (~6492), export
  (~7746-7856), rename (~9882), import (~10569).
- **Deliver:** `notify(severity, message, detail, action)` that shows a toast,
  records the event in a Notifications/Activity panel the user can reopen, and
  writes the log line. Migrate every `status_note` failure and every
  user-relevant `eprintln!`. Errors persist until dismissed; info fades.
- **Done when:** `grep eprintln!` in `laika-app` returns only startup/CLI
  paths; a failed export is still readable five minutes later; each error
  offers Reveal in Log.
- **Depends on:** none. **Touchpoints:** `main.rs`, toast component, new
  activity panel.

### [ ] Q34 — Errors speak human everywhere

- **Deliver:** Extend the `failure_reason()` translation layer (`main.rs`
  ~6504) to the sites that still pass `e.to_string()` straight through:
  `main.rs` ~5411, ~10593, ~19165, ~19196; `presets_ui.rs:252,1065`;
  `gallery_publish.rs:1042`. Every message states what failed, to which file
  or destination, why in plain words, and what to try. Don't assume the user
  knows tool names (`gallery_publish.rs:649` says "wrangler" with no
  explanation) or environment variables (`LAIKA_S3_SECRET` appears in user
  strings, `main.rs` ~3599, ~5340).
- **Done when:** A table-driven test maps the common `io::ErrorKind`, SQLite,
  object-store, and ssh failures to their messages; no user-visible string
  contains a Rust type name or `os error N` without a sentence around it.
- **Depends on:** Q20, Q33. **Touchpoints:** as listed.

### [ ] Q35 — Every long job is visible and cancellable

- **Deliver:** `progress_hud.rs` is good but shows two job types
  (`progress_hud.rs:43,91`). Bring in: backup and SFTP restore (`main.rs`
  ~5701, ~5799 — pause only, not in the HUD), Apple Photos sync (hard-coded
  `cancellable=false`, `progress_hud.rs:100`), Cloudflare deploy
  (`gallery_publish.rs:294`), thumbnail rescan (~10824 — no progress at all),
  preset batch apply (`presets_ui.rs:1065`), and catalog migration (~8733 —
  progress only; not cancellable by design, and it says so). Several
  concurrent jobs stack in the HUD.
- **Done when:** Nothing that can exceed two seconds runs without progress;
  every job except migration cancels within `backlog.md`'s 250 ms gate and
  leaves no partial files.
- **Depends on:** Q33. **Touchpoints:** `progress_hud.rs`, job runners.

### [ ] Q36 — No file I/O on the UI thread

- **Deliver:** Move the remaining synchronous I/O out of view and event paths:
  per-photo `fs::metadata` at `main.rs` ~7066 and ~24912; `read_dir` at
  ~23245, ~27835, ~28729; `read_to_string` at ~19178. Cache the results and
  refresh in the background like the 37 existing spawns do. Gate the
  `LAIKA_PROFILE` `[prof]` output (~24551) behind the diagnostics preference.
- **Done when:** With the catalog's originals on a sleeping network volume,
  scrolling the grid and opening the export dialog never stall beyond
  `backlog.md`'s 100 ms gate.
- **Depends on:** none. **Relation:** the fix-now slice of S11.
  **Touchpoints:** `main.rs`.

## Section G — Supportable (P1)

### [ ] Q37 — User documentation

- **Problem:** No user manual, FAQ, or troubleshooting page exists.
  `docs/*.md` are internal notes — `backup-recovery.md` cites ticket "U24",
  and the README sends users to `windows-port.md`, a developer handoff. The
  README's limitations list is wrong: it says masking/healing and
  Compare/Survey are missing; both ship. Map and Lightroom-migration mode are
  undocumented. The 62-row shortcut table exists only in the binary.
- **Deliver:** `docs/manual/` with: install and system requirements, first
  import, culling, Develop, export, galleries, backup and restore, Map, coming
  from Lightroom, shortcuts (generated, Q26), troubleshooting (Gatekeeper,
  permissions, unsupported RAW, no GPU, locked catalog, where logs live), and
  known limitations. Fix the README; move developer notes to `docs/dev/`.
  Refresh the screenshots.
- **Done when:** Someone who has never seen Laika completes import → cull →
  edit → export using only the manual; every README claim is checked against
  the build being released.
- **Depends on:** Q26. **Relation:** S29 owns the camera matrix the manual
  links to. **Touchpoints:** `docs/`, `README.md`.

### [ ] Q38 — Help, About, and "report a problem" lead somewhere

- **Deliver:** Help menu items for the manual, shortcuts, release notes,
  acknowledgements (Q04), privacy (Q39), and Report a Problem — which opens
  the issue tracker with version, OS, and GPU pre-filled and Copy Diagnostics
  one click away (`platform::open_url` already exists). About gains copyright,
  license, credits, and website alongside the diagnostics it has today. After
  a crash, the next launch says so once and offers to reveal the report —
  today `crash_reports()` is read only when About renders, and capture is off
  by default (`prefs.rs:130`) with no prompt to turn it on.
- **Done when:** From inside the app a user reaches the manual, the changelog,
  and a pre-filled bug report in two clicks each; a forced crash is
  acknowledged on relaunch.
- **Depends on:** Q04, Q37, Q40. **Touchpoints:** `commands.rs:460-471`,
  `diagnostics.rs`.

### [ ] Q39 — Privacy statement that matches the binary

- **Deliver:** A short `PRIVACY.md`, linked from Help and the welcome screen,
  listing every network request Laika can make — opt-in map tiles
  (`map_view.rs:147-185`), backup endpoints, gallery publish, update check
  (Q10) — and stating there is no telemetry and crash reports never leave the
  machine. Reword README:9 "Nothing leaves your computer" to be exactly true.
  Replace the home directory with `~` in diagnostics bundles and crash reports
  (`diagnostics.rs:94-116`, `logging.rs:383`), which today embed the username
  beneath a line promising nothing sensitive. Add the CI egress test from S26
  in its minimal form: default settings, zero outbound connections.
- **Done when:** The statement and the egress test agree; a diagnostics bundle
  contains no username.
- **Depends on:** Q10. **Relation:** the statement half of S26; the Network
  Activity panel stays there. **Touchpoints:** docs, `diagnostics.rs`,
  `logging.rs`, CI.

### [ ] Q40 — Changelog, release notes, and a version policy

- **Deliver:** `CHANGELOG.md` (Keep a Changelog); `release.yml` takes the
  release body from it instead of three hard-coded lines (`:59-63`) that
  currently overclaim — v0.1.1 has macOS assets only while the README badge
  says macOS | Windows. State the versioning policy, including what a catalog
  schema bump means for downgrades. Add `SECURITY.md` with a contact.
  Add `rust-version` and a `rust-toolchain.toml` so builds are reproducible.
- **Done when:** Tagging a release without a changelog entry fails CI; the
  release page, README badges, and actual assets agree.
- **Depends on:** none. **Touchpoints:** repo root, `release.yml`.

### [ ] Q41 — Validate with photographers before calling it 1.0

- **Deliver:** Run the U23 task script — never run to date — with five
  photographers on the notarized build, adding install, first-run, update, and
  "something went wrong" tasks from this backlog. Fold in G28's gallery
  script. Triage findings into this file as Q-items, not new features.
- **Done when:** 4 of 5 complete every task unaided; every blocker found is
  fixed or listed in known limitations (Q37).
- **Depends on:** Milestones A–F. **Relation:** executes U23 and G28.
  **Touchpoints:** `docs/validation/`.

## Section H — Stays fixed (P1 gates, P2 rest)

### [ ] Q42 — Clippy and fmt gate CI

- **Deliver:** Clippy runs nowhere today. Add `cargo clippy --workspace
  --all-targets -- -D warnings` to `ci.yml` with a `[workspace.lints]` table;
  fix or justify what it finds. Add build caching and a release-mode
  `cargo check` so profile-only breakage is caught. Fix the current
  `cargo fmt --check` failure in the untracked `cull_ui.rs` before it lands.
- **Done when:** CI is green with clippy enforced on macOS, Linux, and
  Windows; median CI time doesn't regress thanks to caching.
- **Depends on:** none. **Touchpoints:** `.github/workflows/ci.yml`, root
  `Cargo.toml`.

### [ ] Q43 — License and advisory gate

- **Deliver:** `deny.toml` with an explicit license allow-list that encodes the
  Q02 decision, a ban on git and wildcard dependencies (none today — keep it
  so), and RustSec advisories checked in CI and nightly.
- **Done when:** Adding a GPL dependency or one with an open advisory fails CI
  with a readable reason.
- **Depends on:** Q02. **Touchpoints:** CI, new `deny.toml`.

### [ ] Q44 — Fuzz the parsers that read strangers' files

- **Deliver:** `cargo-fuzz` targets for RAW decode, EXIF, XMP parse, sidecar
  read, `.laikapreset` and Lightroom preset import, and the msgpack reader,
  seeded from `fixtures/`. A short smoke run per PR; a longer run nightly.
  Crashes become regression fixtures.
- **Done when:** Each target survives a one-hour run; nightly fuzzing reports
  to the same place as CI failures.
- **Depends on:** Q12. **Touchpoints:** new `fuzz/`, `laika-raw`,
  `laika-core/src/{xmp,sidecar,presets,msgpack,lightroom}.rs`.

### [ ] Q45 — Accessibility, second pass

- **Problem:** U22 is marked done, but `aria_label` and `role()` appear ~30
  times each across ~46k lines, in 7 files. `lightroom_ui.rs`,
  `collections.rs`, `albums.rs`, `presets_ui.rs`, `map_view.rs`,
  `slideshow.rs`, and `batch_edit.rs` have none. `tab_index` is never used, so
  there is no tab order. Nothing honours Reduce Motion.
- **Deliver:** Labels and roles on every interactive element in the listed
  files; a defined tab order for dialogs, Preferences, and the import and
  export forms; focus rings; Reduce Motion respected by toasts, slideshow, and
  panel transitions. Migrate stray literal colours into `theme.rs` tokens
  (49 in `main.rs`, 15 in `slideshow.rs`, 10 in `map_view.rs`; the danger red
  is duplicated in `gallery_canvas.rs:364` and `progress_hud.rs:115`) so the
  contrast guarantees cover them.
- **Done when:** Import → rate → export completes with VoiceOver and keyboard
  only; a lint or test fails on a clickable element without a label.
- **Depends on:** Q26. **Relation:** reopens part of U22. **Touchpoints:** as
  listed, `theme.rs`.

### [ ] Q46 — Terminology, strings, fixtures, and the 30k-line file

- **Deliver:** (1) A one-page UI style guide that settles Remove vs Delete,
  Catalog vs Library, Photo vs Image, and capitalisation (Title Case menus,
  sentence-case messages), then a pass applying it. (2) Pull user-facing
  strings behind one lookup so localisation is a later option rather than a
  rewrite — English only ships. (3) Document or make discoverable the env
  overrides that change behaviour in release builds
  (`LAIKA_INWINDOW_MENU`, `LAIKA_IMPORT_WORKERS` silently overriding
  Preferences). (4) Record provenance and redistribution rights for
  `fixtures/raw/*.NEF` and move them to LFS or a download step (76 MB in git).
  (5) Keep splitting `main.rs` (30k lines, 12 tests) along the seams other
  modules already follow — export, import dialog, backup panel, key handling
  — and add tests for the logic-heavy modules that have none
  (`photos_sync`, `photos_ingest`, `albums`, `batch_edit`, `site/build.rs`).
- **Done when:** The style guide exists and a grep for the banned synonyms is
  clean; `main.rs` is under 15k lines; the listed modules each have tests.
- **Depends on:** none. **Touchpoints:** `laika-app`, `fixtures/`, `docs/dev/`.

### [ ] Q47 — Backlogs tell the truth, and Windows gets a verdict

- **Deliver:** All four backlog headers say "All items below are open" while
  `backlog.md` has 20 checked and `backlogv2.md` 21. `photogallery.md` shows
  G01–G28 unchecked with no progress notes, although `gallery/`,
  `catalog_gallery.rs`, four `gallery_*.rs` UI modules, and
  `laika-export/src/site/` exist; G20 still specifies `askama` templates the
  2026-09-17 review replaced with plain Rust. `backlogv3.md`'s baseline says
  schema v10 and macOS-only; the code is at v14 (`catalog.rs:1183`),
  `docs/format/` documents 0–13, and a Windows port has landed. `plan.md`
  phases 5–6 and its out-of-scope line are stale; U25 is superseded by
  G19–G24. Reconcile every status under v3's honesty gate. For Windows: run
  the nine-step native smoke test in `docs/windows-port.md:39-56` (never run),
  add `#![windows_subsystem = "windows"]` and VERSIONINFO, replace macOS glyphs
  in shared strings, and decide — signed installer in 1.0, or labelled
  "preview" everywhere including the README badge.
- **Done when:** Every checkbox in every backlog matches a Done-when verified
  on a real build; `docs/format/` covers schema v14; the README's platform
  claims match the release assets.
- **Depends on:** none. **Touchpoints:** `backlog*.md`, `photogallery.md`,
  `plan.md`, `docs/format/`, `docs/windows-port.md`, `crates/laika-app/build.rs`.

## Release gates and validation

These extend the gates in `backlog.md`, `backlogv2.md`, and `backlogv3.md`;
they do not replace them. v3's honesty gate applies: nothing here is checked
off without its Done-when verified on a release build.

| Scenario | Acceptance target |
| --- | --- |
| Fresh-Mac install | Notarized DMG opens and launches on clean Apple-silicon and Intel Macs at the stated minimum OS with no Gatekeeper override |
| Licence audit | LICENSE, OFL, and generated third-party notices present in the `.app`, the Windows zip, and a built gallery; `cargo deny check` green |
| Corrupt input | Truncated, zero-byte, and bit-flipped RAW/JPEG/XMP/preset fixtures: zero crashes in import, Loupe, Develop, export |
| Large input | 100 MP RAW and 20000 px panorama export correctly; peak memory recorded |
| GPU fault | Forced validation error and device loss recover or explain; no relaunch needed |
| Disk fault | Disk-full, read-only, and unplugged-volume runs of import, export, backup, gallery build: refused up front or failed with the cause named; no partial files; catalog matches disk |
| Kill test | `kill -9` during import, preview build, prefs save, migration: next launch is clean, nothing trusted that wasn't fully written |
| Quit test | ⌘Q during import/export/backup prompts; all three choices behave as labelled |
| Colour | sRGB-tagged output in all five formats; visually identical in Preview, Photoshop, Safari, Chrome on a P3 display |
| Menu & shortcut audit | Every registry command has a native key equivalent or an explicit "none"; zero duplicate bindings per context; HIG menu checklist passes |
| Error visibility | No user-relevant `eprintln!` in `laika-app`; every injected failure appears in the notification history |
| Accessibility | Import → rate → export with VoiceOver + keyboard only |
| Privacy | Default-settings egress test: zero connections; diagnostics bundle contains no username |
| Crash loop | Forced panic in a release build → report written → acknowledged on relaunch → symbolicated to file:line |
| Photographer script | 4 of 5 complete install-to-export unaided on the release candidate (Q41) |

Reference hardware and catalog sizes are those in `backlog.md`. Fault
injection fixtures live in `fixtures/` with provenance recorded (Q46).

## Explicitly not in this backlog

New adjustments, views, or import sources (V06–V29 open items); `.lrcat`
import and the rest of the switching case (S01–S22, S27–S30); the Linux build
(S25); pricing and the comparison page (the remainder of S24); wide-gamut
output, per-display colour management, and soft proofing (U20); on-canvas
brush painting (U19); a light theme; localisation beyond making it possible
(Q46). They matter. They are not what stops 1.0.
