# Phase 0 results

**Gate: passed (owner, 2026-09-15).** Laika stays on GPUI.

Spike: `spikes/p0`. Pin: `gpui-kit =0.6.1`, `gpui-pre =0.3.2`, `gpui-pre-platform =0.3.2`, `wgpu =29.0.4`.
This is the pin the foobar2001 G0 spike already proved on macOS.

Machine: Apple M1 Max, macOS, Metal, scale factor 2, 120 Hz display. Release build.

## Letter-spacing

GPUI's `TextStyle` has no tracking field. `src/tracked.rs` shapes the line once and paints each glyph
with `Window::paint_glyph`, shifted by the spacing times the glyphs before it. Kerning survives, and
the width includes the trailing spacing, as CSS does. The owner checked it visually against the
design and approved it. No custom `Element` impl is needed.

Not verified: IBM Plex Mono SemiBold isn't bundled, so the wordmark falls back to the nearest
available weight.

## Canvas loop

A render thread owns the wgpu device and keeps only the newest job. It renders a develop pass from a
half-float source texture into BGRA8, reads it back, and sends it to the UI. The UI wraps the bytes in
a `RenderImage` and drops the previous one from the atlas. The benchmark sweeps Exposure and
Highlights every frame for 10 seconds and discards the first second.

Clean runs, where the frame interval stayed at the display's 8.3 ms:

| Preview size | Preview fps | GPU p50 / p99 | Copy p50 / p99 | Latency p50 / p99 | UI draw p50 / p99 |
|---|---|---|---|---|---|
| 1800 × 1200 | 119.7 | 1.48 / 2.88 ms | 0.54 / 0.70 ms | 8.3 / 9.7 ms | 3.37 / 3.77 ms |
| 1800 × 1200 (repeat) | 119.8 | 1.52 / 2.84 ms | 0.56 / 1.44 ms | 4.5 / 6.2 ms | 3.38 / 3.83 ms |
| 3000 × 2000 | 119.9 | 2.70 / 2.97 ms | 1.40 / 1.74 ms | 8.3 / 8.6 ms | 5.48 / 6.12 ms |

GPU is the uniform write, encode, submit, and wait for the pass and buffer copy. Copy is the CPU row
copy plus the histogram. Latency runs from a slider change to the frame reaching the UI. UI draw
includes the atlas upload of the new image.

Both sizes hold the display rate with a wide margin. The plan's fallback of rendering smaller while
dragging isn't needed on this machine.

## Caveats

- **Measured while the Mac was in use.** Runs where other windows covered the spike were throttled
  by macOS and are excluded. Frame intervals rose to 35 ms in some of those runs, and one run stalled
  until the timeout. The GPU and copy times stayed steady through all of them.
- **Upload cost is only estimated.** No `--skip-upload` run was clean. A partly throttled run drew in
  1.84 ms against 3.4 ms with uploads, which puts the upload near 1.5 ms at 1800 × 1200.
- **1200 × 800 is unexplained.** Its one clean-interval run delivered only 8.4 previews per second.
  It isn't needed for the gate, but look into it if the drag fallback is ever used.
- **The first shader was a stress test, not a result.** It regenerated a procedural scene nine
  times per pixel and managed 45.5 fps with 19.6 ms GPU p50. Moving the scene into a source texture
  fixed it, so the cost was shader work, not readback.
- **Only a high-end Apple GPU was tested.** Linux, Windows and low-end GPUs are untested.
- **Tokio integration is untested.** The spike wakes GPUI from a plain thread through a futures channel.

## Carried into Phase 1

- `laika-develop` keeps the render thread with a latest-job slot and the stale-image drop.
- The tracked-text helper and the slider become shared controls.
- Keep `set_trace_enabled` and the benchmark flag for regression checks.
- `rx.try_next()` in the spike is deprecated. Replace it when the code moves into the workspace.
