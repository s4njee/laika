# `laika-render`

`laika-render` is the reference, non-GUI renderer shipped in Laika's release
bundle. It links the same RAW decoder, WGSL pipeline, edit compatibility layer,
and output encoders as the desktop app. It never migrates or writes a catalog.
In a macOS installation the executable is at
`/Applications/Laika.app/Contents/MacOS/laika-render`; source builds place it
at `target/release/laika-render`.

```text
laika-render --catalog catalog.db --photo-id 42 --output frame.tif
laika-render --catalog catalog.db --photo /Volumes/Photos/a.nef --output frame.jpg
laika-render --sidecar /Volumes/Photos/a.xmp --photo /Volumes/Photos/a.nef --output frame.tif
laika-render --photo /Volumes/Photos/a.nef --output frame.jpg
```

The final form uses Laika's normal sidecar lookup. `--quality 1..100` controls
JPEG quality and defaults to 90. Output format is inferred strictly from
`.jpg`, `.jpeg`, `.tif`, or `.tiff`.

Catalog lookup accepts an exact stored path or numeric photo ID. With no edits
row it renders defaults. It accepts schema 0 through the current schema and
legacy parameter widths 12, 46, 49, and 63. Newer schemas and unknown widths
are refused with a diagnostic. The original must be locally available.

The output contract is opaque 8-bit sRGB. TIFF is uncompressed; JPEG is lossy.
The process exits 0 after an atomic output write, 1 for a read/decode/render/
encode failure, and 2 for command-line usage errors. Progress and the selected
GPU adapter go to stderr; the completed output path goes to stdout.

## Golden-image policy

The renderer is pinned by the `laika-develop` RAW guard and raster identity
goldens. They cover fixed scene-linear pixels, multiple parameter sets, the
camera-to-display/base-look path, geometry CPU/GPU agreement, and the real
raster bridge. RGB channels tolerate at most one code value in the RAW guard;
the raster identity test allows two maximum and less than 0.5 mean error to
accommodate GPU rounding. A semantic pipeline change requires an intentional
golden update and review.
