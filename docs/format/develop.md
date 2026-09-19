# Develop edit format

`params` is an indexed array. Indices never change meaning. Values outside the
documented range are clamped by Laika's editor; readers should clamp untrusted
input and replace non-finite values with the default.

## Parameter indices

| Index | Name | Unit/range | Default |
|---:|---|---|---:|
| 0 | Temperature | kelvin, 2000…8960 | 5480 |
| 1 | Tint | unitless, −150…150 | 6 |
| 2 | Exposure | EV, −5…5 | 0 |
| 3 | Contrast | unitless, −100…100 | 0 |
| 4 | Highlights | unitless, −100…100 | 0 |
| 5 | Shadows | unitless, −100…100 | 0 |
| 6 | Whites | unitless, −100…100 | 0 |
| 7 | Blacks | unitless, −100…100 | 0 |
| 8 | Texture | unitless, −100…100 | 0 |
| 9 | Clarity | unitless, −100…100 | 0 |
| 10 | Vibrance | unitless, −100…100 | 0 |
| 11 | Saturation | unitless, −100…100 | 0 |
| 12–15 | Curve output at 20/40/60/80% input | normalized 0…1 | .2/.4/.6/.8 |
| 16–23 | Hue Red, Orange, Yellow, Green, Aqua, Blue, Purple, Magenta | unitless, −100…100; ±100 maps to ±30° | 0 |
| 24–31 | Saturation for the same eight sectors | unitless, −100…100 | 0 |
| 32–39 | Luminance for the same eight sectors | unitless, −100…100 | 0 |
| 40 | Sharpen | amount, 0…150 | 0 |
| 41 | Sharpen radius | pixels, 0…3 | 1 |
| 42 | Luminance noise reduction | amount, 0…100 | 0 |
| 43 | Color noise reduction | amount, 0…100 | 0 |
| 44 | Manual distortion | unitless, −100…100 | 0 |
| 45 | Chromatic aberration/defringe | unitless, −100…100 | 0 |
| 46 | Dehaze | unitless, −100…100 | 0 |
| 47 | Post-crop vignette | unitless, −100…100 | 0 |
| 48 | Grain | amount, 0…100 | 0 |
| 49–51 | Shadow grading hue/saturation/luminance | degrees 0…359; 0…100; −100…100 | 0/0/0 |
| 52–54 | Midtone grading hue/saturation/luminance | same | 0/0/0 |
| 55–57 | Highlight grading hue/saturation/luminance | same | 0/0/0 |
| 58–60 | Global grading hue/saturation/luminance | same | 0/0/0 |
| 61 | Grading blending | amount, 0…100 | 50 |
| 62 | Grading balance | unitless, −100…100 | 0 |
| 63 | Transform vertical | unitless, −100…100 | 0 |
| 64 | Transform horizontal | unitless, −100…100 | 0 |
| 65 | Transform rotate | clockwise degrees, −10…10 | 0 |
| 66 | Transform aspect | unitless, −100…100 | 0 |
| 67 | Transform scale | percent, 50…150 | 100 |
| 68 | Transform X offset | percent of half display width, −100…100 | 0 |
| 69 | Transform Y offset | percent of half display height, −100…100 | 0 |

Panel switches replace these slots with defaults at render time: `curve_on`
controls 12–15; `hsl_on` 16–39; `detail_on` 40–43; `optics_on` 44–45;
`effects_on` controls 8, 9, and 46–48; `grading_on` controls 49–62. Transform
63–69 has no panel-bypass field.

## Geometry and sampling order

The output size is the displayed source size multiplied by crop width/height,
rounded to the nearest pixel, with a minimum of one. Odd quarter-turns swap
source width and height. For each output coordinate Laika applies:

1. manual radial distortion around output center;
2. horizontal/vertical output flips;
3. normalized crop rectangle;
4. straighten rotation around display center;
5. the inverse Upright/Transform homography;
6. inverse quarter-turn into source coordinates;
7. chromatic-aberration channel offsets while sampling.

The transform homography uses centered display coordinates
`x=(u-.5)*aspect, y=v-.5`. Vertical/horizontal are plane tilts of 0.3° per
unit. Aspect scales axes by `2^(value/200)` and its reciprocal. Scale is
`value/100`. Solved Upright vertical/horizontal/rotate values add to manual
63/64/65. When `constrain` is true, the crop shrinks until all four output
corners sample the source; when false, uncovered transformed pixels are white.

## Color pipeline order

The source is linear RGB: camera RGB for RAW, or inverse-companded sRGB for a
raster. The renderer performs one GPU pass in this order:

1. luminance and color noise reduction in linear source space;
2. clarity (12-pixel cross blur) and texture (2-pixel cross blur);
3. as-shot white balance, mired temperature shift, and green tint scale;
4. exposure multiplication by `2^EV`;
5. dehaze;
6. whites/blacks endpoint remap;
7. highlight/shadow luminance-masked adjustment;
8. contrast power curve about middle gray;
9. vibrance and saturation about luminance;
10. unsharp-mask sharpening;
11. camera-to-linear-sRGB matrix;
12. RAW base look: +1.25 EV gain and the ACES fitted curve; raster sources use identity;
13. fixed-point tone curve and exact sRGB transfer function;
14. eight-sector HSL mix in display-referred RGB;
15. shadow/midtone/highlight/global color grading;
16. local mask adjustments in normalized original-image coordinates;
17. post-crop vignette and deterministic frame-relative grain;
18. quantization to opaque 8-bit sRGB RGBA.

The working path is explicitly scene-linear camera RGB through the decoded
camera matrix into linear sRGB. The output space is explicitly 8-bit sRGB with
the exact sRGB transfer function. The current GPUI surface does not expose a
reliable per-window ICC transform, so preview uses the OS compositor's sRGB
surface fallback; Laika does not claim a wide-gamut display preview. JPEG and
TIFF are encoded as sRGB. This explicit fallback is not a promise of
equivalence with Adobe Camera Raw.

Camera profiles are Laika rendering intents: `Standard` preserves the baseline
look, `Neutral` lowers contrast, `Vivid` raises contrast/chroma, and
`Monochrome` uses the pipeline luminance coefficients. They are not Adobe/DCP
profile names. Unknown stored values fall back to `Standard`.

## Local edits

`locals.masks` stores Brush, Linear, and Radial shapes plus feather, invert,
and adjustment values. Brush samples store flow and erase state. `locals.heals`
stores editable target/donor pairs, radius, and feather. All positions are
normalized against the uncropped, unrotated original. The shader first maps an
output pixel through crop/rotation/Upright into source UV, then evaluates masks
and heals there. Preview and full export therefore share one coordinate and
render path. The live mask outline is a preview-only display aid and is never
packed into an export render.

Output sharpening (`Off`, `Low`, `Standard`, `High`) is a final-size,
luminance-preserving unsharp operation performed after resize and before the
format encoder. It is separate from Develop's source-resolution Detail panel.
