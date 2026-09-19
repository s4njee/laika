# Camera and color support

Laika delegates RAW container parsing, mosaics, black/white levels, as-shot
white balance, and camera matrices to the pinned `rawler` decoder. A filename
extension is an import hint, not proof that a particular camera body is
supported: decode success is the authority and unsupported bodies return a
plain diagnostic without stopping the rest of a batch.

| Family / container | Extensions accepted | Rendering status | Fallback |
|---|---|---|---|
| Adobe / Leica DNG | `.dng` | RAW decode when the embedded mosaic and matrix are supported | explicit unsupported-decode message |
| Canon | `.cr2`, `.cr3` | RAW decode through pinned rawler camera data | explicit unsupported-decode message |
| Nikon | `.nef` | RAW decode through pinned rawler camera data | explicit unsupported-decode message |
| Sony | `.arw` | RAW decode through pinned rawler camera data | explicit unsupported-decode message |
| Fujifilm | `.raf` | RAW decode where the sensor layout is supported | explicit unsupported-layout/decode message |
| Olympus / OM System | `.orf` | RAW decode through pinned rawler camera data | explicit unsupported-decode message |
| Panasonic / Leica | `.rw2`, `.rwl` | RAW decode through pinned rawler camera data | explicit unsupported-decode message |
| Pentax / Samsung | `.pef`, `.srw`, `.dng` | RAW decode through pinned rawler camera data | explicit unsupported-decode message |
| Raster bridge | `.jpg`, `.jpeg`, `.png`, `.tif`, `.tiff`, `.heic`, `.heif`, `.hif` | inverse-sRGB into the common linear pipeline | decode error; never guessed as RAW |

## Validation contract

- As-shot white balance uses decoder multipliers. User Temperature is mapped
  in reciprocal-temperature (mired) space and Tint is an independent green
  scale; neutral-patch unit tests pin the mapping and finite/clamped behavior.
- The camera matrix, baseline RAW look, raster identity bridge, crop/Upright
  coordinate mapping, local-mask preview/export parity, and output sharpening
  are regression-tested.
- `Standard`, `Neutral`, `Vivid`, and `Monochrome` are Laika profiles. Missing
  or future profile values fall back to `Standard`; they are not aliases for
  Adobe Camera profiles.
- The working path is linear camera RGB to linear sRGB. Output is opaque 8-bit
  sRGB. When a trustworthy display ICC transform is unavailable, preview
  explicitly uses the compositor's sRGB fallback. Soft proofing remains gated
  on a trustworthy display-profile path; Laika does not simulate it by merely
  reusing XMP names.
