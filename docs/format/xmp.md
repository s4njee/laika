# XMP sidecar mapping

Laika writes an RDF/XMP packet beside the original. RAW files use Adobe's
`basename.xmp` convention when that file already exists or Lightroom sharing
is enabled; other files may use `filename.ext.xmp`. A reader should use the
sidecar path supplied by the catalog/workflow rather than infer solely from
this document.

Absent values mean “unspecified” and must not reset a catalog value. Laika
retains foreign top-level attributes and their namespace declarations when it
rewrites a packet. Standard RDF containers are used for title, caption,
creator, rights, keywords, and hierarchical keywords.

## Develop parameters

| Laika indices | XMP attribute(s) |
|---|---|
| 0–11 | `crs:Temperature`, `Tint`, `Exposure2012`, `Contrast2012`, `Highlights2012`, `Shadows2012`, `Whites2012`, `Blacks2012`, `Texture`, `Clarity2012`, `Vibrance`, `Saturation` |
| 12–15 | Six-point `crs:ToneCurvePV2012`: endpoints plus x=51/102/153/204 |
| 16–23 | `crs:HueAdjustment{Red,Orange,Yellow,Green,Aqua,Blue,Purple,Magenta}` |
| 24–31 | `crs:SaturationAdjustment{…}` in the same order |
| 32–39 | `crs:LuminanceAdjustment{…}` in the same order |
| 40–44 | `crs:Sharpness`, `SharpenRadius`, `LuminanceSmoothing`, `ColorNoiseReduction`, `LensManualDistortionAmount` |
| 45 | `laika:ChromaticAberration` (no Adobe scalar equivalent) |
| 46–48 | `crs:Dehaze`, `PostCropVignetteAmount`, `GrainAmount` |
| 49–51 | `crs:SplitToningShadowHue`, `SplitToningShadowSaturation`, `ColorGradeShadowLum` |
| 52–54 | `crs:ColorGradeMidtoneHue`, `ColorGradeMidtoneSat`, `ColorGradeMidtoneLum` |
| 55–57 | `crs:SplitToningHighlightHue`, `SplitToningHighlightSaturation`, `ColorGradeHighlightLum` |
| 58–60 | `crs:ColorGradeGlobalHue`, `ColorGradeGlobalSat`, `ColorGradeGlobalLum` |
| 61–62 | `crs:ColorGradeBlending`, `SplitToningBalance` |
| 63–69 | `crs:PerspectiveVertical`, `PerspectiveHorizontal`, `PerspectiveRotate`, `PerspectiveAspect`, `PerspectiveScale`, `PerspectiveX`, `PerspectiveY` |

Adobe names indicate interchange syntax, not pixel equivalence. Exposure is
written with two decimal places and an explicit sign; Kelvin and integer
amounts are rounded; radius/rotate use one decimal; curve outputs use three.

## Compatibility boundary

Metadata compatibility means Laika can discover the standard sidecar name, read
or write ratings/descriptive fields/keywords, and preserve unknown XMP properties.
It does **not** imply render equivalence. Laika reports non-neutral Adobe masks,
profiles, RGB curves, lens corrections, legacy-process settings, and other
unsupported adjustments during Lightroom migration. Supported scalar names can
still produce visually different pixels because the raw decoder, camera profile,
tone mapping, masking, and color-management pipeline differ.

## Geometry

| Field | Meaning |
|---|---|
| `crs:CropLeft/Top/Right/Bottom` | normalized display-space edges |
| `crs:CropAngle` | clockwise degrees |
| `tiff:Orientation` | standard derived orientation |
| `laika:Rotation` | exact clockwise quarter turns 0–3 |
| `laika:FlipH`, `laika:FlipV` | `True` when enabled |
| `laika:UprightMode` | Off, Auto, Level, Vertical, Full, Guided |
| `laika:UprightAuto` | comma-separated solved vertical,horizontal,rotate |
| `laika:UprightGuides` | semicolon-separated `u0,v0,u1,v1` lines |
| `laika:ConstrainCrop` | boolean |

Explicit Laika rotation/flip fields win over `tiff:Orientation`; this prevents
double application. A packet with no crop or Laika geometry field carries no
geometry opinion.

## Metadata

| Data | XMP representation |
|---|---|
| Rating | `xmp:Rating`, integer 0–5 |
| Color label | `xmp:Label` using the catalog's label names |
| Title | `dc:title/rdf:Alt/rdf:li` |
| Caption | `dc:description/rdf:Alt/rdf:li` |
| Headline | `photoshop:Headline` |
| Creator | `dc:creator/rdf:Seq/rdf:li` |
| Copyright | `dc:rights/rdf:Alt/rdf:li` |
| Usage terms | `xmpRights:UsageTerms/rdf:Alt/rdf:li` |
| Contact | `laika:Contact` |
| Location | `Iptc4xmpCore:Location` |
| Flat keywords | `dc:subject/rdf:Bag/rdf:li` |
| Hierarchical keywords | `lr:hierarchicalSubject/rdf:Bag/rdf:li`, levels separated by `|` |
| GPS | `exif:GPSLatitude`, `exif:GPSLongitude` decimal coordinates |
| History labels | `laika:History`, ` | ` separated |
| Applied preset | `laika:Preset` |

Panel bypass switches and full undo snapshots are catalog-only in schema 13;
sidecars contain effective scalar values and descriptive history labels.
