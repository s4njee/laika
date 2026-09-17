//! XMP sidecars (Camera Raw Settings namespace) so Lightroom and darktable
//! can read the values. Written on every committed edit, read on import and
//! catalog open — sidecar wins over the DB.

use laika_raw::decode::LinearImage;

pub const PARAM_KEYS: [(&str, usize); 65] = [
    ("Temperature", 0),
    ("Tint", 1),
    ("Exposure2012", 2),
    ("Contrast2012", 3),
    ("Highlights2012", 4),
    ("Shadows2012", 5),
    ("Whites2012", 6),
    ("Blacks2012", 7),
    ("Texture", 8),
    ("Clarity2012", 9),
    ("Vibrance", 10),
    ("Saturation", 11),
    // U18: Color Mix hue/saturation/luminance (Adobe scalar semantics).
    ("HueAdjustmentRed", 16),
    ("HueAdjustmentOrange", 17),
    ("HueAdjustmentYellow", 18),
    ("HueAdjustmentGreen", 19),
    ("HueAdjustmentAqua", 20),
    ("HueAdjustmentBlue", 21),
    ("HueAdjustmentPurple", 22),
    ("HueAdjustmentMagenta", 23),
    ("SaturationAdjustmentRed", 24),
    ("SaturationAdjustmentOrange", 25),
    ("SaturationAdjustmentYellow", 26),
    ("SaturationAdjustmentGreen", 27),
    ("SaturationAdjustmentAqua", 28),
    ("SaturationAdjustmentBlue", 29),
    ("SaturationAdjustmentPurple", 30),
    ("SaturationAdjustmentMagenta", 31),
    ("LuminanceAdjustmentRed", 32),
    ("LuminanceAdjustmentOrange", 33),
    ("LuminanceAdjustmentYellow", 34),
    ("LuminanceAdjustmentGreen", 35),
    ("LuminanceAdjustmentAqua", 36),
    ("LuminanceAdjustmentBlue", 37),
    ("LuminanceAdjustmentPurple", 38),
    ("LuminanceAdjustmentMagenta", 39),
    // U18: Detail + manual optics (Adobe scalar semantics).
    ("Sharpness", 40),
    ("SharpenRadius", 41),
    ("LuminanceSmoothing", 42),
    ("ColorNoiseReduction", 43),
    ("LensManualDistortionAmount", 44),
    // Effects (Adobe scalar names).
    ("Dehaze", 46),
    ("PostCropVignetteAmount", 47),
    ("GrainAmount", 48),
    // Color Grading (Lightroom keys; shadows/highlights keep the legacy
    // SplitToning names Adobe still writes).
    ("SplitToningShadowHue", 49),
    ("SplitToningShadowSaturation", 50),
    ("ColorGradeShadowLum", 51),
    ("ColorGradeMidtoneHue", 52),
    ("ColorGradeMidtoneSat", 53),
    ("ColorGradeMidtoneLum", 54),
    ("SplitToningHighlightHue", 55),
    ("SplitToningHighlightSaturation", 56),
    ("ColorGradeHighlightLum", 57),
    ("ColorGradeGlobalHue", 58),
    ("ColorGradeGlobalSat", 59),
    ("ColorGradeGlobalLum", 60),
    ("ColorGradeBlending", 61),
    ("SplitToningBalance", 62),
    // V22 Transform sliders (Adobe names; Laika's projection model — the
    // values round-trip, rendering equivalence is not claimed).
    ("PerspectiveVertical", 63),
    ("PerspectiveHorizontal", 64),
    ("PerspectiveRotate", 65),
    ("PerspectiveAspect", 66),
    ("PerspectiveScale", 67),
    ("PerspectiveX", 68),
    ("PerspectiveY", 69),
    // Curve points (12..15) travel as ToneCurvePV2012 below; CA (45) as
    // laika:ChromaticAberration (no Adobe scalar equivalent).
];

/// Format one param the way Camera Raw writes it (ints except Exposure and
/// Texture, which keep two decimals; Temperature is Kelvin).
pub fn format_param(i: usize, v: f32) -> String {
    // Dispatches on the param definition (identical output for 0..11).
    match crate::edit::PARAMS[i].fmt {
        crate::edit::Fmt::Kelvin => format!("{}", v.round() as i32),
        // Signed scales carry an explicit sign like Camera Raw; 0-based
        // amounts (saturation, grain, sharpening) are written unsigned.
        crate::edit::Fmt::Int if crate::edit::PARAMS[i].min >= 0. => {
            format!("{}", v.round() as i32)
        }
        crate::edit::Fmt::Int => format!("{:+}", v.round() as i32),
        crate::edit::Fmt::Ev => format!("{:+.2}", (v * 100.).round() / 100.),
        crate::edit::Fmt::Unit => format!("{:.3}", v.clamp(0., 1.)),
        crate::edit::Fmt::Float => format!("{:.1}", v),
        crate::edit::Fmt::Hue => format!("{}", v.round() as i32),
    }
}

fn parse_param(i: usize, s: &str) -> Option<f32> {
    let s = s.replace('\u{2212}', "-");
    match crate::edit::PARAMS[i].fmt {
        crate::edit::Fmt::Kelvin => s.parse::<i32>().ok().map(|v| v as f32),
        // "NaN"/"inf" parse as f32 but would poison every pixel downstream.
        _ => s.parse::<f32>().ok().filter(|v| v.is_finite()),
    }
}

/// U18: our four curve points as a ToneCurvePV2012 list (Adobe-readable).
pub fn format_tone_curve(params: &[f32; crate::edit::PARAM_COUNT]) -> String {
    let y = |i: usize| (params[i].clamp(0., 1.) * 255.).round() as i32;
    format!(
        "0, 0, 51, {}, 102, {}, 153, {}, 204, {}, 255, 255",
        y(12),
        y(13),
        y(14),
        y(15)
    )
}

/// U18: parse our 6-point shape back (x = 0,51,102,153,204,255);
/// foreign curves are ignored, never misread.
pub fn parse_tone_curve(s: &str) -> Option<[f32; 4]> {
    let nums: Vec<i32> = s
        .split(',')
        .map(|t| t.trim().parse().unwrap_or(-1))
        .collect();
    if nums.len() != 12 {
        return None;
    }
    let xs = [0, 51, 102, 153, 204, 255];
    for (i, x) in xs.iter().enumerate() {
        if nums[2 * i] != *x {
            return None;
        }
    }
    let mut out = [0f32; 4];
    for k in 0..4 {
        let y = nums[2 * k + 3];
        if !(0..=255).contains(&y) {
            return None;
        }
        out[k] = y as f32 / 255.;
    }
    Some(out)
}

pub struct Sidecar {
    pub params: [f32; crate::edit::PARAM_COUNT],
    pub rating: Option<u8>,
    pub history: Vec<String>,
    pub preset: Option<String>,
    /// V03: authorship + keywords. V12 writes standard structures
    /// (Alt/Bag/Seq) that Lightroom and darktable read; the reader
    /// still accepts our legacy flat attributes on old sidecars.
    pub title: String,
    pub caption: String,
    pub headline: String,
    pub creator: String,
    pub copyright: String,
    pub rights_usage: String,
    pub contact: String,
    pub location: String,
    pub keywords: Vec<String>,
    /// U08: geometry — `Some` iff the file carries any crop attribute.
    /// Absent attributes mean "leave catalog geometry alone" (a foreign
    /// rewrite without crop keys must not wipe a Laika crop).
    pub geom: Option<crate::edit::CropGeom>,
    /// V13: `xmp:Label` text when present (a name, mapped by the catalog).
    pub label: Option<String>,
    /// U18: whether the file carried any tone key (a rating-only foreign
    /// rewrite must not reset catalog tone to defaults).
    pub has_tone: bool,
}

impl Default for Sidecar {
    fn default() -> Self {
        Self {
            params: crate::edit::defaults(),
            rating: None,
            history: Vec::new(),
            preset: None,
            title: String::new(),
            caption: String::new(),
            headline: String::new(),
            creator: String::new(),
            copyright: String::new(),
            rights_usage: String::new(),
            contact: String::new(),
            location: String::new(),
            keywords: Vec::new(),
            geom: None,
            label: None,
            has_tone: false,
        }
    }
}

/// U08 crop attribute keys. `crs:` names match Adobe's spelling; the
/// semantics are Laika's (rect in unrotated source space, clockwise
/// positive angle — see `edit::crop_sample`), documented, not claimed
/// as rendering equivalence (same stance as U20).
pub const CROP_ATTRS: [&str; 7] = [
    "CropLeft",
    "CropTop",
    "CropRight",
    "CropBottom",
    "CropAngle",
    "laika:FlipH",
    "laika:FlipV",
];

pub fn sidecar_path(photo_path: &str) -> String {
    format!("{photo_path}.xmp")
}

/// mtime of the sidecar next to `photo_path`, whole seconds. `None` when the
/// sidecar (or its metadata) is missing.
pub fn sidecar_mtime(photo_path: &str) -> Option<i64> {
    std::fs::metadata(sidecar_path(photo_path))
        .ok()?
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs() as i64)
}

/// Well-known prefixes, bound on rewrite when a carried foreign attribute
/// uses one and the source file declared it elsewhere (or older Laika
/// rewrites dropped the declaration).
const KNOWN_NS: [(&str, &str); 12] = [
    ("crs", "http://ns.adobe.com/camera-raw-settings/1.0/"),
    ("xmp", "http://ns.adobe.com/xap/1.0/"),
    ("laika", "http://laika.studio/ns/1.0/"),
    ("tiff", "http://ns.adobe.com/tiff/1.0/"),
    ("dc", "http://purl.org/dc/elements/1.1/"),
    ("xmpRights", "http://ns.adobe.com/xap/1.0/rights/"),
    ("lr", "http://ns.adobe.com/lightroom/1.0/"),
    ("photoshop", "http://ns.adobe.com/photoshop/1.0/"),
    (
        "Iptc4xmpCore",
        "http://iptc.org/std/Iptc4xmpCore/1.0/xmlns/",
    ),
    ("xmpMM", "http://ns.adobe.com/xap/1.0/mm/"),
    ("exif", "http://ns.adobe.com/exif/1.0/"),
    ("aux", "http://ns.adobe.com/exif/1.0/aux/"),
];

/// Attribute keys owned by Laika. Anything else on `rdf:Description`
/// (e.g. Lightroom's `crs:ProcessVersion`) is foreign metadata we must
/// carry across rewrites untouched (U02).
fn is_known_attr(key: &str) -> bool {
    if matches!(
        key,
        "xmp:Rating"
            | "laika:History"
            | "laika:Preset"
            | "laika:Version"
            | "laika:Contact"
            | "dc:title"
            | "dc:description"
            | "dc:creator"
            | "dc:rights"
            | "dc:subject"
            | "lr:hierarchicalSubject"
            | "xmpRights:Marked"
            | "xmpRights:UsageTerms"
            | "photoshop:Headline"
            | "Iptc4xmpCore:Location"
            | "tiff:Orientation"
            | "laika:Rotation"
            | "laika:FlipH"
            | "laika:FlipV"
            | "laika:ChromaticAberration"
            | "laika:UprightMode"
            | "laika:UprightAuto"
            | "laika:UprightGuides"
            | "laika:ConstrainCrop"
    ) {
        return true;
    }
    if let Some(stripped) = key.strip_prefix("crs:") {
        if stripped == "ToneCurvePV2012" {
            return true;
        }
        if matches!(
            stripped,
            "CropLeft" | "CropTop" | "CropRight" | "CropBottom" | "CropAngle"
        ) {
            return true;
        }
    }
    if let Some(stripped) = key.strip_prefix("crs:") {
        return PARAM_KEYS.iter().any(|(k, _)| *k == stripped);
    }
    false
}

/// Authorship block written alongside develop params (V03).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Authorship {
    pub title: String,
    pub caption: String,
    pub headline: String,
    pub creator: String,
    pub copyright: String,
    pub rights_usage: String,
    pub contact: String,
    pub location: String,
    /// Full canonical paths, export-included only (leaves feed
    /// dc:subject, paths feed lr:hierarchicalSubject).
    pub keywords: Vec<String>,
    /// V13: color label name for `xmp:Label` (empty = no label).
    pub label: String,
    /// V13: this catalog's label names. A carried foreign `xmp:Label`
    /// survives only when it is none of them (e.g. Lightroom's "Select"),
    /// so clearing a Laika label never resurrects the old text.
    pub label_names: Vec<String>,
}

impl Authorship {
    pub fn is_empty(&self) -> bool {
        self.title.is_empty()
            && self.caption.is_empty()
            && self.headline.is_empty()
            && self.creator.is_empty()
            && self.copyright.is_empty()
            && self.rights_usage.is_empty()
            && self.contact.is_empty()
            && self.location.is_empty()
            && self.keywords.is_empty()
    }
}

/// V12: descriptive metadata as standard XMP child elements — the
/// form external readers (Lightroom, darktable, exiftool) parse.
/// `x-default` Alt for title/description/rights/usage, Seq for
/// creator, Bag for subjects (leaves) and hierarchical paths.
struct ChildBlock {
    title: String,
    caption: String,
    headline: String,
    creator: String,
    copyright: String,
    rights_usage: String,
    contact: String,
    location: String,
    keywords: Vec<String>,
}

/// V12: minimal text escaping for element content (quick-xml writes
/// `BytesText` raw — pre-escape before handing it over).
fn escape_text(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// V12: resolve one entity reference (the `GeneralRef` payload between
/// `&` and `;`). Unknown references survive literally — never drop data.
fn resolve_ref(raw: &[u8]) -> String {
    match raw {
        b"lt" => "<".to_string(),
        b"gt" => ">".to_string(),
        b"amp" => "&".to_string(),
        b"quot" => "\"".to_string(),
        b"apos" => "'".to_string(),
        _ if raw.first() == Some(&b'#') => {
            let num = &raw[1..];
            let code = if num
                .first()
                .map(|c| c.eq_ignore_ascii_case(&b'x'))
                .unwrap_or(false)
            {
                u32::from_str_radix(std::str::from_utf8(&num[1..]).unwrap_or(""), 16).ok()
            } else {
                std::str::from_utf8(num).unwrap_or("").parse::<u32>().ok()
            };
            code.and_then(char::from_u32)
                .map(|c| c.to_string())
                .unwrap_or_else(|| format!("&#{};", String::from_utf8_lossy(num)))
        }
        _ => format!("&{};", String::from_utf8_lossy(raw)),
    }
}

impl ChildBlock {
    fn is_empty(&self) -> bool {
        self.title.is_empty()
            && self.caption.is_empty()
            && self.headline.is_empty()
            && self.creator.is_empty()
            && self.copyright.is_empty()
            && self.rights_usage.is_empty()
            && self.contact.is_empty()
            && self.location.is_empty()
            && self.keywords.is_empty()
    }

    fn leaf_paths(&self) -> Vec<String> {
        self.keywords
            .iter()
            .map(|p| match p.rsplit(" > ").next() {
                Some(leaf) => leaf.to_string(),
                None => p.clone(),
            })
            .filter(|l| !l.is_empty())
            .collect()
    }

    fn write(&self, w: &mut quick_xml::writer::Writer<Vec<u8>>) -> Result<(), String> {
        use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
        fn el(
            w: &mut quick_xml::writer::Writer<Vec<u8>>,
            tag: &str,
            text: &str,
        ) -> Result<(), String> {
            w.write_event(Event::Start(BytesStart::new(tag)))
                .map_err(|e| e.to_string())?;
            w.write_event(Event::Text(BytesText::from_escaped(escape_text(text))))
                .map_err(|e| e.to_string())?;
            w.write_event(Event::End(BytesEnd::new(tag)))
                .map_err(|e| e.to_string())
        }
        fn alt(
            w: &mut quick_xml::writer::Writer<Vec<u8>>,
            tag: &str,
            text: &str,
        ) -> Result<(), String> {
            w.write_event(Event::Start(BytesStart::new(tag)))
                .map_err(|e| e.to_string())?;
            w.write_event(Event::Start(BytesStart::new("rdf:Alt")))
                .map_err(|e| e.to_string())?;
            let mut li = BytesStart::new("rdf:li");
            li.push_attribute(("xml:lang", "x-default"));
            w.write_event(Event::Start(li)).map_err(|e| e.to_string())?;
            w.write_event(Event::Text(BytesText::from_escaped(escape_text(text))))
                .map_err(|e| e.to_string())?;
            w.write_event(Event::End(BytesEnd::new("rdf:li")))
                .map_err(|e| e.to_string())?;
            w.write_event(Event::End(BytesEnd::new("rdf:Alt")))
                .map_err(|e| e.to_string())?;
            w.write_event(Event::End(BytesEnd::new(tag)))
                .map_err(|e| e.to_string())
        }
        fn seq_bag(
            w: &mut quick_xml::writer::Writer<Vec<u8>>,
            tag: &str,
            kind: &str,
            items: &[String],
        ) -> Result<(), String> {
            w.write_event(Event::Start(BytesStart::new(tag)))
                .map_err(|e| e.to_string())?;
            w.write_event(Event::Start(BytesStart::new(kind)))
                .map_err(|e| e.to_string())?;
            for item in items {
                el(w, "rdf:li", item)?;
            }
            w.write_event(Event::End(BytesEnd::new(kind)))
                .map_err(|e| e.to_string())?;
            w.write_event(Event::End(BytesEnd::new(tag)))
                .map_err(|e| e.to_string())
        }
        if !self.title.is_empty() {
            alt(w, "dc:title", &self.title)?;
        }
        if !self.caption.is_empty() {
            alt(w, "dc:description", &self.caption)?;
        }
        if !self.creator.is_empty() {
            seq_bag(w, "dc:creator", "rdf:Seq", &[self.creator.clone()])?;
        }
        if !self.copyright.is_empty() {
            alt(w, "dc:rights", &self.copyright)?;
        }
        let leaves = self.leaf_paths();
        if !leaves.is_empty() {
            seq_bag(w, "dc:subject", "rdf:Bag", &leaves)?;
        }
        if !self.headline.is_empty() {
            el(w, "photoshop:Headline", &self.headline)?;
        }
        if !self.location.is_empty() {
            el(w, "Iptc4xmpCore:Location", &self.location)?;
        }
        if !self.copyright.is_empty() || !self.rights_usage.is_empty() {
            el(w, "xmpRights:Marked", "True")?;
        }
        if !self.rights_usage.is_empty() {
            alt(w, "xmpRights:UsageTerms", &self.rights_usage)?;
        }
        if !self.contact.is_empty() {
            el(w, "laika:Contact", &self.contact)?;
        }
        if !self.keywords.is_empty() {
            seq_bag(w, "lr:hierarchicalSubject", "rdf:Bag", &self.keywords)?;
        }
        Ok(())
    }
}

pub fn write(
    photo_path: &str,
    params: &[f32; crate::edit::PARAM_COUNT],
    rating: u8,
    history: &[String],
    preset: Option<&str>,
    authorship: &Authorship,
    geom: &crate::edit::CropGeom,
) -> Result<(), String> {
    // Nothing is ever written inside a Photos library; the catalog holds
    // these photos' settings.
    if crate::apple_photos::in_library(photo_path) {
        return Ok(());
    }
    // Read-modify-write: foreign attributes survive our rewrites.
    let unknown = read_unknown(photo_path);
    let bytes = render(params, rating, history, preset, authorship, geom, &unknown)?;
    write_atomic(&sidecar_path(photo_path), &bytes)
}

/// U10: render a sidecar document to bytes (same content `write` stores;
/// exports reuse it with their own destination + foreign carry-over).
#[allow(clippy::too_many_arguments)]
pub fn render(
    params: &[f32; crate::edit::PARAM_COUNT],
    rating: u8,
    history: &[String],
    preset: Option<&str>,
    authorship: &Authorship,
    geom: &crate::edit::CropGeom,
    unknown: &[(String, String)],
) -> Result<Vec<u8>, String> {
    use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, Event};
    use quick_xml::writer::Writer;
    let mut w = Writer::new_with_indent(Vec::new(), b' ', 1);
    w.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))
        .map_err(|e| e.to_string())?;
    let mut root = BytesStart::new("x:xmpmeta");
    root.push_attribute(("xmlns:x", "adobe:ns:meta/"));
    root.push_attribute(("x:xmptk", "Laika"));
    w.write_event(Event::Start(root))
        .map_err(|e| e.to_string())?;
    let mut rdf = BytesStart::new("rdf:RDF");
    rdf.push_attribute(("xmlns:rdf", "http://www.w3.org/1999/02/22-rdf-syntax-ns#"));
    w.write_event(Event::Start(rdf))
        .map_err(|e| e.to_string())?;
    let mut desc = BytesStart::new("rdf:Description");
    desc.push_attribute(("xmlns:crs", "http://ns.adobe.com/camera-raw-settings/1.0/"));
    desc.push_attribute(("xmlns:xmp", "http://ns.adobe.com/xap/1.0/"));
    desc.push_attribute(("xmlns:laika", "http://laika.studio/ns/1.0/"));
    if !authorship.creator.is_empty()
        || !authorship.copyright.is_empty()
        || !authorship.keywords.is_empty()
        || !authorship.title.is_empty()
        || !authorship.caption.is_empty()
    {
        desc.push_attribute(("xmlns:dc", "http://purl.org/dc/elements/1.1/"));
    }
    if !authorship.rights_usage.is_empty() || !authorship.copyright.is_empty() {
        desc.push_attribute(("xmlns:xmpRights", "http://ns.adobe.com/xap/1.0/rights/"));
    }
    if !authorship.keywords.is_empty() {
        desc.push_attribute(("xmlns:lr", "http://ns.adobe.com/lightroom/1.0/"));
    }
    if !authorship.headline.is_empty() {
        desc.push_attribute(("xmlns:photoshop", "http://ns.adobe.com/photoshop/1.0/"));
    }
    if !authorship.location.is_empty() {
        desc.push_attribute((
            "xmlns:Iptc4xmpCore",
            "http://iptc.org/std/Iptc4xmpCore/1.0/xmlns/",
        ));
    }
    desc.push_attribute(("xmp:Rating", rating.to_string().as_str()));
    // V13: Lightroom stores the label's name.
    if !authorship.label.trim().is_empty() {
        desc.push_attribute(("xmp:Label", authorship.label.trim()));
    }
    // V15: orientation always explicit — tiff:Orientation for foreign
    // readers, laika:Rotation as the lossless source of truth.
    desc.push_attribute(("xmlns:tiff", "http://ns.adobe.com/tiff/1.0/"));
    desc.push_attribute(("tiff:Orientation", geom.orientation().to_string().as_str()));
    desc.push_attribute(("laika:Rotation", geom.rotation.to_string().as_str()));
    for (key, i) in PARAM_KEYS {
        desc.push_attribute((
            format!("crs:{key}").as_str(),
            format_param(i, params[i]).as_str(),
        ));
    }
    // U18: point curve as a standard list; CA as a Laika scalar.
    desc.push_attribute(("crs:ToneCurvePV2012", format_tone_curve(params).as_str()));
    if params[45] != 0. {
        desc.push_attribute((
            "laika:ChromaticAberration",
            format_param(45, params[45]).as_str(),
        ));
    }
    // U08: geometry always explicit, so an external reset to full-frame
    // round-trips instead of reading as "absent".
    let r = geom.rect;
    for (key, v) in [
        ("crs:CropLeft", r[0]),
        ("crs:CropTop", r[1]),
        ("crs:CropRight", r[0] + r[2]),
        ("crs:CropBottom", r[1] + r[3]),
    ] {
        desc.push_attribute((key, format!("{v:.6}").as_str()));
    }
    desc.push_attribute(("crs:CropAngle", format!("{:.2}", geom.angle).as_str()));
    if geom.flip_h {
        desc.push_attribute(("laika:FlipH", "True"));
    }
    if geom.flip_v {
        desc.push_attribute(("laika:FlipV", "True"));
    }
    // V22: Upright's mode, solved correction, and guides (Laika-only;
    // the manual sliders travel as crs:Perspective*).
    let up = &geom.upright;
    if up.mode != 0 || up.auto != [0.; 3] {
        desc.push_attribute(("laika:UprightMode", up.mode().label()));
        desc.push_attribute((
            "laika:UprightAuto",
            format!("{:.3},{:.3},{:.3}", up.auto[0], up.auto[1], up.auto[2]).as_str(),
        ));
    }
    if up.n_guides > 0 {
        desc.push_attribute(("laika:UprightGuides", up.guides_text().as_str()));
    }
    if !up.constrain {
        desc.push_attribute(("laika:ConstrainCrop", "False"));
    }
    if !history.is_empty() {
        desc.push_attribute(("laika:History", history.join(" | ").as_str()));
    }
    if let Some(p) = preset {
        desc.push_attribute(("laika:Preset", p));
    }
    // Marker so readers know the matrix chain already applied.
    desc.push_attribute(("laika:Version", "1"));
    // V12: descriptive block as standard structures (Alt/Bag/Seq),
    // the form Lightroom and darktable read. Legacy flat attributes
    // are never written anymore; the reader still accepts them.
    let html = ChildBlock {
        title: authorship.title.clone(),
        caption: authorship.caption.clone(),
        headline: authorship.headline.clone(),
        creator: authorship.creator.clone(),
        copyright: authorship.copyright.clone(),
        rights_usage: authorship.rights_usage.clone(),
        contact: authorship.contact.clone(),
        location: authorship.location.clone(),
        keywords: authorship.keywords.clone(),
    };
    for (k, v) in unknown {
        // A duplicate attribute (e.g. a carried `xmlns:dc` we already
        // declared) makes the whole document malformed.
        if matches!(desc.try_get_attribute(k.as_str()), Ok(Some(_))) {
            continue;
        }
        // V13: a label text Laika owns is rewritten from the catalog, never
        // carried (clearing a label must stick).
        if k == "xmp:Label" {
            let t = v.trim();
            let ours = authorship
                .label_names
                .iter()
                .map(String::as_str)
                .chain(crate::labels::DEFAULT_NAMES)
                .any(|n| n.eq_ignore_ascii_case(t));
            if ours {
                continue;
            }
        }
        desc.push_attribute((k.as_str(), v.as_str()));
    }
    // Foreign attributes in our conditional namespaces (e.g. Lightroom's
    // `photoshop:DateCreated` without a headline) need their prefix bound,
    // or namespace-aware readers reject the sidecar.
    for (k, _) in unknown {
        let Some((prefix, _)) = k.split_once(':') else {
            continue;
        };
        let Some((_, uri)) = KNOWN_NS.iter().find(|(p, _)| *p == prefix) else {
            continue;
        };
        let decl = format!("xmlns:{prefix}");
        if matches!(desc.try_get_attribute(decl.as_str()), Ok(None)) {
            desc.push_attribute((decl.as_str(), *uri));
        }
    }
    if html.is_empty() {
        w.write_event(Event::Empty(desc))
            .map_err(|e| e.to_string())?;
    } else {
        w.write_event(Event::Start(desc))
            .map_err(|e| e.to_string())?;
        html.write(&mut w)?;
        w.write_event(Event::End(BytesEnd::new("rdf:Description")))
            .map_err(|e| e.to_string())?;
    }
    w.write_event(Event::End(BytesEnd::new("rdf:RDF")))
        .map_err(|e| e.to_string())?;
    w.write_event(Event::End(BytesEnd::new("x:xmpmeta")))
        .map_err(|e| e.to_string())?;
    Ok(w.into_inner())
}

/// U10: atomic file write (tmp + fsync + rename) to any destination.
/// Crash-safe like the catalog sidecar path.
pub fn write_atomic(dest: &str, bytes: &[u8]) -> Result<(), String> {
    // Unique per call: two threads writing the same destination must not
    // share (and truncate or rename away) one temp file.
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = format!("{dest}.tmp-{}-{seq}", std::process::id());
    let written = std::fs::write(&tmp, bytes)
        .map_err(|e| format!("write {}: {e}", tmp))
        .and_then(|_| {
            std::fs::File::open(&tmp)
                .and_then(|f| f.sync_all())
                .map_err(|e| format!("sync {}: {e}", tmp))
        });
    if let Err(e) = written {
        std::fs::remove_file(&tmp).ok();
        return Err(e);
    }
    std::fs::rename(&tmp, dest).map_err(|e| {
        std::fs::remove_file(&tmp).ok();
        format!("replace {dest}: {e}")
    })?;
    Ok(())
}

/// U10: copyright-only sidecar (creator + rights, no tone or history).
/// Exports with the Copyright policy write exactly this.
/// V12: minimal delivery sidecar in the same standard form as the
/// full render (Seq creator, Alt rights) so external readers parse it.
/// V12: standalone XMP packet for JPEG embedding (APP1 payload).
/// Standard structures — the form Lightroom and exiftool read.
pub fn descriptive_packet(auth: &Authorship) -> Vec<u8> {
    use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, Event};
    use quick_xml::writer::Writer;
    let mut w = Writer::new_with_indent(Vec::new(), b' ', 1);
    w.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))
        .map_err(|e| e.to_string())
        .unwrap_or(());
    let mut root = BytesStart::new("x:xmpmeta");
    root.push_attribute(("xmlns:x", "adobe:ns:meta/"));
    root.push_attribute(("x:xmptk", "Laika"));
    w.write_event(Event::Start(root))
        .map_err(|e| e.to_string())
        .unwrap_or(());
    let mut rdf = BytesStart::new("rdf:RDF");
    rdf.push_attribute(("xmlns:rdf", "http://www.w3.org/1999/02/22-rdf-syntax-ns#"));
    w.write_event(Event::Start(rdf))
        .map_err(|e| e.to_string())
        .unwrap_or(());
    let mut desc = BytesStart::new("rdf:Description");
    desc.push_attribute(("rdf:about", ""));
    // Every `dc:` element ChildBlock may write: rights and subject too.
    if !auth.title.is_empty()
        || !auth.caption.is_empty()
        || !auth.creator.is_empty()
        || !auth.copyright.is_empty()
        || !auth.keywords.is_empty()
    {
        desc.push_attribute(("xmlns:dc", "http://purl.org/dc/elements/1.1/"));
    }
    if !auth.copyright.is_empty() || !auth.rights_usage.is_empty() {
        desc.push_attribute(("xmlns:xmpRights", "http://ns.adobe.com/xap/1.0/rights/"));
    }
    if !auth.headline.is_empty() {
        desc.push_attribute(("xmlns:photoshop", "http://ns.adobe.com/photoshop/1.0/"));
    }
    if !auth.location.is_empty() {
        desc.push_attribute((
            "xmlns:Iptc4xmpCore",
            "http://iptc.org/std/Iptc4xmpCore/1.0/xmlns/",
        ));
    }
    if !auth.keywords.is_empty() {
        desc.push_attribute(("xmlns:lr", "http://ns.adobe.com/lightroom/1.0/"));
    }
    w.write_event(Event::Start(desc))
        .map_err(|e| e.to_string())
        .unwrap_or(());
    ChildBlock {
        title: auth.title.clone(),
        caption: auth.caption.clone(),
        headline: auth.headline.clone(),
        creator: auth.creator.clone(),
        copyright: auth.copyright.clone(),
        rights_usage: auth.rights_usage.clone(),
        contact: String::new(),
        location: auth.location.clone(),
        keywords: auth.keywords.clone(),
    }
    .write(&mut w)
    .unwrap_or(());
    w.write_event(Event::End(BytesEnd::new("rdf:Description")))
        .map_err(|e| e.to_string())
        .unwrap_or(());
    w.write_event(Event::End(BytesEnd::new("rdf:RDF")))
        .map_err(|e| e.to_string())
        .unwrap_or(());
    w.write_event(Event::End(BytesEnd::new("x:xmpmeta")))
        .map_err(|e| e.to_string())
        .unwrap_or(());
    w.into_inner()
}

pub fn render_minimal(creator: &str, copyright: &str) -> Vec<u8> {
    use quick_xml::events::{BytesDecl, BytesEnd, BytesStart, Event};
    use quick_xml::writer::Writer;
    let mut w = Writer::new_with_indent(Vec::new(), b' ', 1);
    w.write_event(Event::Decl(BytesDecl::new("1.0", Some("UTF-8"), None)))
        .map_err(|e| e.to_string())
        .unwrap_or(());
    let mut root = BytesStart::new("x:xmpmeta");
    root.push_attribute(("xmlns:x", "adobe:ns:meta/"));
    root.push_attribute(("x:xmptk", "Laika"));
    w.write_event(Event::Start(root))
        .map_err(|e| e.to_string())
        .unwrap_or(());
    let mut rdf = BytesStart::new("rdf:RDF");
    rdf.push_attribute(("xmlns:rdf", "http://www.w3.org/1999/02/22-rdf-syntax-ns#"));
    w.write_event(Event::Start(rdf))
        .map_err(|e| e.to_string())
        .unwrap_or(());
    let mut desc = BytesStart::new("rdf:Description");
    desc.push_attribute(("xmlns:dc", "http://purl.org/dc/elements/1.1/"));
    desc.push_attribute(("xmlns:xmpRights", "http://ns.adobe.com/xap/1.0/rights/"));
    w.write_event(Event::Start(desc))
        .map_err(|e| e.to_string())
        .unwrap_or(());
    ChildBlock {
        title: String::new(),
        caption: String::new(),
        headline: String::new(),
        creator: creator.to_string(),
        copyright: copyright.to_string(),
        rights_usage: String::new(),
        contact: String::new(),
        location: String::new(),
        keywords: Vec::new(),
    }
    .write(&mut w)
    .unwrap_or(());
    w.write_event(Event::End(BytesEnd::new("rdf:Description")))
        .map_err(|e| e.to_string())
        .unwrap_or(());
    w.write_event(Event::End(BytesEnd::new("rdf:RDF")))
        .map_err(|e| e.to_string())
        .unwrap_or(());
    w.write_event(Event::End(BytesEnd::new("x:xmpmeta")))
        .map_err(|e| e.to_string())
        .unwrap_or(());
    w.into_inner()
}

/// Foreign `rdf:Description` attributes in the existing sidecar, if any.
/// Export reuses this so delivery sidecars keep foreign metadata too.
pub fn read_unknown(photo_path: &str) -> Vec<(String, String)> {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;
    let Ok(bytes) = std::fs::read(sidecar_path(photo_path)) else {
        return Vec::new();
    };
    let mut r = Reader::from_reader(&bytes[..]);
    let mut out: Vec<(String, String)> = Vec::new();
    // Namespace declarations seen on the description or its ancestors,
    // re-emitted for the prefixes carried attributes actually use.
    let mut decls: Vec<(String, String)> = Vec::new();
    // Nested descriptions (struct values such as `crs:Look`) belong to
    // their parent property, not to the top-level attribute set.
    let mut depth = 0usize;
    loop {
        let (e, is_start) = match r.read_event() {
            Ok(Event::Start(e)) => (e, true),
            Ok(Event::Empty(e)) => (e, false),
            Ok(Event::End(e)) => {
                if e.name().as_ref() == b"rdf:Description" {
                    depth = depth.saturating_sub(1);
                }
                continue;
            }
            Ok(Event::Eof) | Err(_) => break,
            _ => continue,
        };
        let is_desc = e.name().as_ref() == b"rdf:Description";
        let top = is_desc && depth == 0;
        if is_desc && is_start {
            depth += 1;
        }
        if depth > usize::from(top && is_start) {
            continue;
        }
        for attr in e.attributes().flatten() {
            let key = String::from_utf8_lossy(attr.key.as_ref()).into_owned();
            let val = attr.unescape_value().unwrap_or_default().into_owned();
            if let Some(prefix) = key.strip_prefix("xmlns:") {
                if !matches!(prefix, "x" | "rdf" | "xml") {
                    decls.retain(|(k, _)| *k != key);
                    decls.push((key, val));
                }
                continue;
            }
            if !top || key.starts_with("xmlns") || is_known_attr(&key) {
                continue;
            }
            if !out.iter().any(|(k, _)| *k == key) {
                out.push((key, val));
            }
        }
    }
    let used: Vec<String> = out
        .iter()
        .filter_map(|(k, _)| k.split_once(':').map(|(p, _)| format!("xmlns:{p}")))
        .collect();
    for (k, v) in decls {
        if used.contains(&k) && !out.iter().any(|(ok, _)| *ok == k) {
            out.push((k, v));
        }
    }
    out
}

pub fn read(photo_path: &str) -> Option<Sidecar> {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;
    let bytes = std::fs::read(sidecar_path(photo_path)).ok()?;
    let mut r = Reader::from_reader(&bytes[..]);
    // No trim_text: fragments around entity references must keep their
    // spaces — the End arm trims the assembled buffer instead.
    let mut params = crate::edit::defaults();
    let mut rating = None;
    let mut label: Option<String> = None;
    let mut history = Vec::new();
    let mut preset = None;
    let mut title = String::new();
    let mut caption = String::new();
    let mut headline = String::new();
    let mut creator = String::new();
    let mut copyright = String::new();
    let mut rights_usage = String::new();
    let mut contact = String::new();
    let mut location = String::new();
    let mut keywords: Vec<String> = Vec::new();
    let mut lr_keywords: Vec<String> = Vec::new();
    // V12: element stack for standard structures (Alt/Bag/Seq children).
    let mut stack: Vec<String> = Vec::new();
    let mut in_desc = false;
    let mut desc_depth = 0usize;
    // V12: quick-xml splits entity references into GeneralRef events, so
    // text accumulates here and flushes at the closing tag.
    let mut frag = String::new();
    // U08: crop edges/angle/flips; presence of any one carries geometry.
    let mut crop_left: Option<f32> = None;
    let mut crop_top: Option<f32> = None;
    let mut crop_right: Option<f32> = None;
    let mut crop_bottom: Option<f32> = None;
    let mut crop_angle: Option<f32> = None;
    let mut flip_h = false;
    let mut flip_v = false;
    // V15: explicit rotation + foreign orientation fallback.
    let mut rotation: u8 = 0;
    let mut orientation: Option<u8> = None;
    let mut crs_geom_seen = false;
    // V22: Upright state (Laika keys).
    let mut upright = crate::upright::Upright::default();
    let mut upright_seen = false;
    let mut has_tone = false;
    let mut curve: Option<[f32; 4]> = None;
    let mut found = false;
    // V12: standard child elements fill first; legacy flat attributes
    // only fill what the standard form left empty (old sidecars).
    let mut take = |slot: &mut String, v: String| {
        if slot.is_empty() {
            *slot = v;
        }
    };
    // Match an element stack suffix, e.g. Description/dc:subject/Bag/li.
    fn at(stack: &[String], path: &[&str]) -> bool {
        stack.len() >= path.len()
            && stack[stack.len() - path.len()..]
                .iter()
                .zip(path)
                .all(|(a, b)| a == b)
    }
    // One attribute parser shared by Start and Empty descriptions —
    // childless sidecars arrive as Empty, never Start. A macro (not a
    // closure) so no borrow outlives its arm.
    macro_rules! on_attr {
        ($key:expr, $val:expr) => {{
            let key: &str = $key;
            let val: String = $val;
            if let Some(stripped) = key.strip_prefix("crs:") {
                if let Some((_, i)) = PARAM_KEYS.iter().find(|(k, _)| *k == stripped) {
                    if let Some(v) = parse_param(*i, &val) {
                        params[*i] = v;
                        found = true;
                        has_tone = true;
                    }
                } else if stripped == "ToneCurvePV2012" {
                    // U18: only our own 6-point shape adopts.
                    curve = parse_tone_curve(&val);
                // U08: geometry keys live in the same namespace.
                } else if stripped == "CropLeft" {
                    crop_left = val.parse::<f32>().ok();
                    crs_geom_seen = true;
                } else if stripped == "CropTop" {
                    crop_top = val.parse::<f32>().ok();
                    crs_geom_seen = true;
                } else if stripped == "CropRight" {
                    crop_right = val.parse::<f32>().ok();
                    crs_geom_seen = true;
                } else if stripped == "CropBottom" {
                    crop_bottom = val.parse::<f32>().ok();
                    crs_geom_seen = true;
                } else if stripped == "CropAngle" {
                    crop_angle = val.parse::<f32>().ok();
                    crs_geom_seen = true;
                }
            } else if key == "xmp:Rating" {
                rating = val.parse::<u8>().ok();
                found = true;
            } else if key == "xmp:Label" {
                label = Some(val.clone());
                found = true;
            } else if key == "laika:History" {
                history = val.split(" | ").map(|s| s.to_string()).collect();
            } else if key == "laika:Preset" {
                preset = Some(val);
            } else if key == "dc:creator" {
                take(&mut creator, val);
                found = true;
            } else if key == "dc:rights" {
                take(&mut copyright, val);
                found = true;
            } else if key == "xmpRights:UsageTerms" {
                take(&mut rights_usage, val);
                found = true;
            } else if key == "laika:Contact" {
                take(&mut contact, val);
                found = true;
            } else if key == "dc:subject" {
                if keywords.is_empty() {
                    keywords = val
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                }
                found = true;
            } else if key == "lr:hierarchicalSubject" {
                if lr_keywords.is_empty() {
                    lr_keywords = val
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                }
            } else if key == "laika:FlipH" {
                flip_h = val.eq_ignore_ascii_case("true");
                crs_geom_seen = true;
            } else if key == "laika:FlipV" {
                flip_v = val.eq_ignore_ascii_case("true");
                crs_geom_seen = true;
            } else if key == "laika:UprightMode" {
                upright.mode = crate::upright::UprightMode::parse(&val).code();
                upright_seen = true;
            } else if key == "laika:UprightAuto" {
                let nums: Vec<f32> = val
                    .split(',')
                    .filter_map(|t| t.trim().parse::<f32>().ok())
                    .filter(|v| v.is_finite())
                    .collect();
                if nums.len() == 3 {
                    upright.auto = [
                        nums[0].clamp(-200., 200.),
                        nums[1].clamp(-200., 200.),
                        nums[2].clamp(-45., 45.),
                    ];
                }
                upright_seen = true;
            } else if key == "laika:UprightGuides" {
                upright.set_guides_text(&val);
                upright_seen = true;
            } else if key == "laika:ConstrainCrop" {
                upright.constrain = !val.eq_ignore_ascii_case("false");
                upright_seen = true;
            } else if key == "laika:Rotation" {
                // Our own lossless rotation (explicit keys win below).
                rotation = val.parse::<u8>().map(|r| r % 4).unwrap_or(0);
                crs_geom_seen = true;
            } else if key == "tiff:Orientation" {
                // Foreign fallback only — explicit geometry wins so our
                // own files (which carry both) never double-rotate.
                orientation = val.parse::<u8>().ok();
            } else if key == "laika:ChromaticAberration" {
                // U18: Laika-only scalar (no Adobe equivalent).
                if let Some(v) = val.parse::<f32>().ok().filter(|v| v.is_finite()) {
                    params[45] = v.clamp(-100., 100.);
                    has_tone = true;
                }
            }
        }};
    }
    loop {
        match r.read_event() {
            Ok(Event::Start(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                // Only a top-level description carries our attributes; a
                // nested one is a struct value (e.g. Lightroom's crs:Look).
                let top = name == "rdf:Description" && !in_desc;
                if name == "rdf:Description" {
                    in_desc = true;
                    desc_depth += 1;
                }
                if in_desc {
                    stack.push(name);
                    frag.clear();
                }
                if !top {
                    continue;
                }
                for attr in e.attributes().flatten() {
                    let key = String::from_utf8_lossy(&attr.key.as_ref()).into_owned();
                    let val = attr.unescape_value().unwrap_or_default().into_owned();
                    on_attr!(&key, val);
                }
            }
            // V12: childless descriptions arrive as Empty, never Start —
            // they carry the same attributes (params, crop, legacy flat).
            Ok(Event::Empty(e)) => {
                if e.name().as_ref() != b"rdf:Description" || in_desc {
                    continue;
                }
                for attr in e.attributes().flatten() {
                    let key = String::from_utf8_lossy(&attr.key.as_ref()).into_owned();
                    let val = attr.unescape_value().unwrap_or_default().into_owned();
                    on_attr!(&key, val);
                }
            }
            Ok(Event::End(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                if name == "rdf:Description" && desc_depth <= 1 {
                    // Closing a nested description must not end the outer
                    // one (its later children would be silently dropped).
                    in_desc = false;
                    desc_depth = 0;
                    stack.clear();
                    frag.clear();
                } else if in_desc {
                    // V12: flush the accumulated text against the closing
                    // element (stack still includes it for matching).
                    let text = std::mem::take(&mut frag);
                    if !text.trim().is_empty() {
                        let text = text.trim().to_string();
                        const D: &str = "rdf:Description";
                        if at(&stack, &[D, "dc:title", "rdf:Alt", "rdf:li"]) {
                            take(&mut title, text);
                            found = true;
                        } else if at(&stack, &[D, "dc:description", "rdf:Alt", "rdf:li"]) {
                            take(&mut caption, text);
                            found = true;
                        } else if at(&stack, &[D, "dc:creator", "rdf:Seq", "rdf:li"]) {
                            take(&mut creator, text);
                            found = true;
                        } else if at(&stack, &[D, "dc:rights", "rdf:Alt", "rdf:li"]) {
                            take(&mut copyright, text);
                            found = true;
                        } else if at(&stack, &[D, "dc:subject", "rdf:Bag", "rdf:li"]) {
                            if !keywords.contains(&text) {
                                keywords.push(text);
                            }
                            found = true;
                        } else if at(&stack, &[D, "lr:hierarchicalSubject", "rdf:Bag", "rdf:li"]) {
                            if !lr_keywords.contains(&text) {
                                lr_keywords.push(text);
                            }
                            found = true;
                        } else if at(&stack, &[D, "photoshop:Headline"]) {
                            take(&mut headline, text);
                            found = true;
                        } else if at(&stack, &[D, "Iptc4xmpCore:Location"]) {
                            take(&mut location, text);
                            found = true;
                        } else if at(&stack, &[D, "xmpRights:UsageTerms", "rdf:Alt", "rdf:li"])
                            || at(&stack, &[D, "xmpRights:UsageTerms"])
                        {
                            take(&mut rights_usage, text);
                            found = true;
                        } else if at(&stack, &[D, "laika:Contact"]) {
                            take(&mut contact, text);
                            found = true;
                        } else if at(&stack, &[D, "xmpRights:Marked"]) {
                            found = true;
                        }
                    }
                    if name == "rdf:Description" {
                        desc_depth -= 1;
                    }
                    stack.pop();
                    frag.clear();
                }
            }
            // V12: text fragments accumulate (entity references arrive as
            // separate GeneralRef events); the End arm flushes them.
            Ok(Event::Text(e)) => {
                if in_desc {
                    frag.push_str(&String::from_utf8_lossy(&e));
                }
            }
            Ok(Event::CData(e)) => {
                if in_desc {
                    frag.push_str(&String::from_utf8_lossy(&e));
                }
            }
            Ok(Event::GeneralRef(e)) => {
                if in_desc {
                    frag.push_str(&resolve_ref(&e));
                }
            }
            Ok(Event::Eof) => break,
            Err(_) => break,
            _ => {}
        }
    }
    // V12: full paths win over flat leaves whenever the hierarchical
    // bag is present (it is strictly more informative — leaves derive
    // from paths, never the reverse).
    if !lr_keywords.is_empty() {
        keywords = lr_keywords;
    }
    // U08: any crop attribute carries geometry; an uncropped Laika file
    // writes explicit full-frame edges, so Some(full-frame) is normal.
    // V15: foreign Orientation derives rotation/flips only when no
    // explicit geometry is present (never double-applied).
    let (rotation, flip_h, flip_v) = if crs_geom_seen {
        (rotation, flip_h, flip_v)
    } else if let Some(o) = orientation {
        let (r, fh, fv) = crate::edit::CropGeom::from_orientation(o);
        (r, fh, fv)
    } else {
        (rotation, flip_h, flip_v)
    };
    let geom = match (crop_left, crop_top, crop_right, crop_bottom, crop_angle) {
        (Some(l), Some(t), Some(r), Some(b), _) => Some(
            crate::edit::CropGeom {
                rect: [l, t, (r - l).max(0.02), (b - t).max(0.02)],
                angle: crop_angle.unwrap_or(0.).clamp(-45., 45.),
                flip_h,
                flip_v,
                rotation,
                upright,
            }
            .sanitized(),
        ),
        _ if crop_angle.is_some() || flip_h || flip_v || rotation != 0 || upright_seen => {
            Some(crate::edit::CropGeom {
                angle: crop_angle.unwrap_or(0.).clamp(-45., 45.),
                flip_h,
                flip_v,
                rotation,
                upright,
                ..Default::default()
            })
        }
        _ => None,
    };
    if geom.is_some() {
        found = true;
    }
    if let Some(c) = curve {
        params[12] = c[0];
        params[13] = c[1];
        params[14] = c[2];
        params[15] = c[3];
        has_tone = true;
    }
    found.then_some(Sidecar {
        params,
        rating,
        history,
        preset,
        title,
        caption,
        headline,
        creator,
        copyright,
        rights_usage,
        contact,
        location,
        keywords,
        geom,
        label,
        has_tone,
    })
}

/// Reference white-balance multipliers, mirroring the WGSL stage 1 mapping
/// (`develop.wgsl`). Tested here; the shader is verified visually.
pub fn wb_multipliers(temperature: f32, tint: f32, as_shot: [f32; 3]) -> [f32; 3] {
    let kt = (temperature - 5480.) / 5480.;
    [
        as_shot[0] * (1. + 0.8 * kt),
        as_shot[1] * (1. - tint / 300.),
        as_shot[2] * (1. - 0.8 * kt),
    ]
}

#[allow(dead_code)]
fn camera_of(_img: &LinearImage) -> &str {
    &_img.camera
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        let dir = std::env::temp_dir().join(format!("laika-xmp-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let photo = dir.join("a.dng").to_string_lossy().to_string();
        let mut params = crate::edit::defaults();
        params[0] = 6500.;
        params[1] = -12.;
        params[2] = 0.35;
        params[4] = -40.;
        params[10] = 18.;
        write(
            &photo,
            &params,
            4,
            &["Exposure +0.35".into(), "Preset Portra warm".into()],
            Some("Portra warm"),
            &Authorship::default(),
            &crate::edit::CropGeom {
                rect: [0.1, 0.2, 0.5, 0.4],
                angle: -3.5,
                flip_h: true,
                flip_v: false,
                rotation: 0,
                upright: Default::default(),
            },
        )
        .unwrap();
        // U08: geometry round-trips through the sidecar.
        let back = read(&photo).expect("sidecar reads");
        let g = back.geom.expect("crop attrs carry geometry");
        assert!((g.rect[0] - 0.1).abs() < 1e-5);
        assert!((g.rect[1] - 0.2).abs() < 1e-5);
        assert!((g.rect[2] - 0.5).abs() < 1e-5);
        assert!((g.rect[3] - 0.4).abs() < 1e-5);
        assert!((g.angle + 3.5).abs() < 1e-5);
        assert!(g.flip_h && !g.flip_v);
        let back = read(&photo).expect("sidecar reads");
        assert_eq!(back.params, params);
        assert_eq!(back.rating, Some(4));
        assert_eq!(back.history.len(), 2);
        assert_eq!(back.preset.as_deref(), Some("Portra warm"));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn missing_sidecar_is_none() {
        assert!(read("/nonexistent/photo.nef").is_none());
    }

    #[test]
    fn foreign_metadata_survives_rewrite() {
        // U02: Lightroom-style attributes we don't own must persist.
        let dir = std::env::temp_dir().join(format!("laika-xmp-f{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let photo = dir.join("a.dng").to_string_lossy().to_string();
        let params = crate::edit::defaults();
        write(
            &photo,
            &params,
            0,
            &[],
            None,
            &Authorship::default(),
            &Default::default(),
        )
        .unwrap();
        // Simulate an external editor adding foreign attributes.
        let raw = std::fs::read_to_string(sidecar_path(&photo)).unwrap();
        let raw = raw.replace(
            "laika:Version=\"1\"",
            "laika:Version=\"1\" crs:ProcessVersion=\"15.4\" xmp:Label=\"Select\"",
        );
        std::fs::write(sidecar_path(&photo), raw).unwrap();
        // Our rewrite must carry the foreign attributes forward.
        let mut warm = params;
        warm[0] = 7000.;
        write(
            &photo,
            &warm,
            5,
            &[],
            None,
            &Authorship::default(),
            &Default::default(),
        )
        .unwrap();
        let back = std::fs::read_to_string(sidecar_path(&photo)).unwrap();
        assert!(back.contains("crs:ProcessVersion=\"15.4\""), "{back}");
        assert!(back.contains("xmp:Label=\"Select\""), "{back}");
        assert_eq!(read(&photo).unwrap().rating, Some(5));
        // V13: our own label replaces it; clearing ours leaves no label
        // (a recognized name is never carried back in).
        let red = Authorship {
            label: "Red".into(),
            ..Authorship::default()
        };
        write(&photo, &warm, 5, &[], None, &red, &Default::default()).unwrap();
        let back = std::fs::read_to_string(sidecar_path(&photo)).unwrap();
        assert_eq!(back.matches("xmp:Label").count(), 1, "{back}");
        assert_eq!(read(&photo).unwrap().label.as_deref(), Some("Red"));
        write(
            &photo,
            &warm,
            5,
            &[],
            None,
            &Authorship::default(),
            &Default::default(),
        )
        .unwrap();
        assert_eq!(read(&photo).unwrap().label, None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn nested_descriptions_and_foreign_namespaces_survive() {
        let dir = std::env::temp_dir().join(format!("laika-xmp-ns{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let photo = dir.join("a.nef").to_string_lossy().to_string();
        std::fs::write(
            sidecar_path(&photo),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:aux="http://ns.adobe.com/exif/1.0/aux/">
<rdf:Description rdf:about="" xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/" xmlns:photoshop="http://ns.adobe.com/photoshop/1.0/" xmlns:dc="http://purl.org/dc/elements/1.1/" crs:Exposure2012="+0.50" crs:ProcessVersion="15.4" photoshop:DateCreated="2024-01-01" aux:Lens="50mm">
<crs:Look><rdf:Description crs:Name="Adobe Color" crs:Exposure2012="+3.00"><crs:Parameters><rdf:Description crs:Version="1"/></crs:Parameters></rdf:Description></crs:Look>
<dc:title><rdf:Alt><rdf:li xml:lang="x-default">After look</rdf:li></rdf:Alt></dc:title>
</rdf:Description>
</rdf:RDF>
</x:xmpmeta>"#,
        )
        .unwrap();
        let back = read(&photo).expect("reads");
        assert_eq!(back.params[2], 0.5, "nested struct must not override");
        assert_eq!(back.title, "After look", "children after a struct survive");
        let unknown = read_unknown(&photo);
        assert!(
            !unknown
                .iter()
                .any(|(k, _)| k == "crs:Name" || k == "crs:Version")
        );
        write(
            &photo,
            &back.params,
            0,
            &[],
            None,
            &Authorship::default(),
            &Default::default(),
        )
        .unwrap();
        let raw = std::fs::read_to_string(sidecar_path(&photo)).unwrap();
        for decl in ["xmlns:photoshop=", "xmlns:aux=", "xmlns:crs="] {
            assert_eq!(raw.matches(decl).count(), 1, "{decl} in {raw}");
        }
        assert!(
            raw.contains("photoshop:DateCreated=\"2024-01-01\""),
            "{raw}"
        );
        assert!(raw.contains("aux:Lens=\"50mm\""), "{raw}");
        assert!(!raw.contains("crs:Name"), "{raw}");
        // A packet with rights only still binds the dc prefix.
        let packet = String::from_utf8(descriptive_packet(&Authorship {
            copyright: "© Ada".into(),
            ..Default::default()
        }))
        .unwrap();
        assert!(
            packet.contains("dc:rights") && packet.contains("xmlns:dc="),
            "{packet}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn write_is_atomic_and_leaves_no_tmp() {
        let dir = std::env::temp_dir().join(format!("laika-xmp-a{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let photo = dir.join("a.dng").to_string_lossy().to_string();
        write(
            &photo,
            &crate::edit::defaults(),
            3,
            &[],
            None,
            &Authorship::default(),
            &Default::default(),
        )
        .unwrap();
        let dest = sidecar_path(&photo);
        assert!(std::fs::metadata(&dest).is_ok());
        assert!(read(&photo).unwrap().rating == Some(3));
        let leftovers: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".tmp-"))
            .collect();
        assert!(leftovers.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn write_failure_is_actionable() {
        // Missing parent dir: rename target can't be created.
        let err = write(
            "/nonexistent-dir-xyz/photo.nef",
            &crate::edit::defaults(),
            0,
            &[],
            None,
            &Authorship::default(),
            &Default::default(),
        )
        .expect_err("must fail");
        assert!(err.contains("photo.nef.xmp"), "{err}");
    }

    #[test]
    fn authorship_roundtrips_with_params() {
        // V03: creator/copyright/keywords survive a rewrite next to edits.
        let dir = std::env::temp_dir().join(format!("laika-xmp-m{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let photo = dir.join("a.dng").to_string_lossy().to_string();
        let auth = Authorship {
            creator: "Ada Lovelace".into(),
            copyright: "© 2026 Ada".into(),
            rights_usage: "Editorial only".into(),
            contact: "ada@example.com".into(),
            keywords: vec!["ceremony".into(), "Lisbon".into()],
            ..Default::default()
        };
        write(
            &photo,
            &crate::edit::defaults(),
            0,
            &[],
            None,
            &auth,
            &Default::default(),
        )
        .unwrap();
        let raw = std::fs::read_to_string(sidecar_path(&photo)).unwrap();
        assert!(raw.contains("dc:creator"), "{raw}");
        assert!(raw.contains("dc:rights"), "{raw}");
        assert!(raw.contains("xmpRights:UsageTerms"), "{raw}");
        let back = read(&photo).expect("sidecar reads");
        assert_eq!(back.creator, "Ada Lovelace");
        assert_eq!(back.copyright, "© 2026 Ada");
        assert_eq!(back.rights_usage, "Editorial only");
        assert_eq!(back.contact, "ada@example.com");
        assert_eq!(back.keywords, vec!["ceremony", "Lisbon"]);
        // Rewrite with empty authorship keeps params but drops the block.
        write(
            &photo,
            &crate::edit::defaults(),
            0,
            &[],
            None,
            &Authorship::default(),
            &Default::default(),
        )
        .unwrap();
        let back = read(&photo).expect("sidecar reads");
        assert!(back.creator.is_empty() && back.keywords.is_empty());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn u18_new_stages_round_trip() {
        // HSL/detail/optics scalars + curve list + CA survive a rewrite.
        let dir = std::env::temp_dir().join(format!("laika-xmp-u18{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let photo = dir.join("a.dng").to_string_lossy().to_string();
        let mut params = crate::edit::defaults();
        params[13] = 0.75;
        params[16] = -20.;
        params[32] = 15.;
        params[40] = 60.;
        params[41] = 1.5;
        params[44] = -30.;
        params[45] = 25.;
        params[46] = 35.;
        params[47] = -40.;
        params[48] = 20.;
        params[49] = 190.;
        params[50] = 40.;
        params[57] = -15.;
        params[61] = 70.;
        params[62] = -25.;
        write(
            &photo,
            &params,
            0,
            &[],
            None,
            &Authorship::default(),
            &Default::default(),
        )
        .unwrap();
        let raw = std::fs::read_to_string(sidecar_path(&photo)).unwrap();
        assert!(raw.contains("ToneCurvePV2012"), "{raw}");
        assert!(raw.contains("HueAdjustmentRed"), "{raw}");
        assert!(raw.contains("Sharpness"), "{raw}");
        assert!(raw.contains("LensManualDistortionAmount"), "{raw}");
        assert!(raw.contains("laika:ChromaticAberration"), "{raw}");
        let back = read(&photo).expect("sidecar reads");
        assert!(back.has_tone);
        assert!((back.params[13] - 0.75).abs() < 0.01, "{}", back.params[13]);
        assert_eq!(back.params[16], -20.);
        assert_eq!(back.params[32], 15.);
        assert_eq!(back.params[40], 60.);
        assert_eq!(back.params[41], 1.5);
        assert_eq!(back.params[44], -30.);
        assert_eq!(back.params[45], 25.);
        // Effects travel as Adobe's own keys.
        assert!(
            raw.contains("crs:Dehaze") && raw.contains("crs:PostCropVignetteAmount"),
            "{raw}"
        );
        assert!(raw.contains("crs:GrainAmount"), "{raw}");
        assert_eq!(back.params[46], 35.);
        assert_eq!(back.params[47], -40.);
        assert_eq!(back.params[48], 20.);
        // Color grading uses Lightroom's keys (hue written unsigned).
        assert!(raw.contains(r#"crs:SplitToningShadowHue="190""#), "{raw}");
        assert!(
            raw.contains("crs:ColorGradeBlending") && raw.contains("crs:SplitToningBalance"),
            "{raw}"
        );
        assert_eq!(back.params[49], 190.);
        assert_eq!(back.params[50], 40.);
        assert_eq!(back.params[57], -15.);
        assert_eq!(back.params[61], 70.);
        assert_eq!(back.params[62], -25.);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn u18_foreign_curve_ignored_toneless_keeps_quiet() {
        // A foreign 3-point curve never parses as ours; a rating-only
        // file carries no tone (catalog tone survives adoption).
        assert!(parse_tone_curve("0, 0, 128, 200, 255, 255").is_none());
        assert!(parse_tone_curve("0, 0, 51, 300, 102, 0, 153, 0, 204, 0, 255, 255").is_none());
        let ours = format_tone_curve(&crate::edit::defaults());
        let back = parse_tone_curve(&ours).expect("our shape reads");
        assert!((back[0] - 0.2).abs() < 0.01);
        let dir = std::env::temp_dir().join(format!("laika-xmp-u18b{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let photo = dir.join("b.dng").to_string_lossy().to_string();
        std::fs::write(
            sidecar_path(&photo),
            r#"<?xml version="1.0" encoding="UTF-8"?><x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:xmp="http://ns.adobe.com/xap/1.0/" xmp:Rating="5"/></rdf:RDF></x:xmpmeta>"#,
        )
        .unwrap();
        let back = read(&photo).expect("rating-only reads");
        assert!(!back.has_tone);
        assert_eq!(back.rating, Some(5));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn u10_minimal_sidecar_carries_rights_only() {
        let bytes = render_minimal("Ada", "© 2026 Ada");
        let raw = String::from_utf8(bytes).unwrap();
        assert!(raw.contains("dc:creator"), "{raw}");
        assert!(raw.contains("© 2026 Ada"), "{raw}");
        assert!(!raw.contains("crs:Exposure"), "{raw}");
        // Empty authorship renders a valid (empty) document.
        let empty = String::from_utf8(render_minimal("", "")).unwrap();
        assert!(empty.contains("rdf:Description"), "{empty}");
    }

    #[test]
    fn v12_descriptive_block_round_trips_standard_structures() {
        // V12 gate: title/caption/headline/location + hierarchical
        // keywords write as standard Alt/Bag/Seq and read back.
        let dir = std::env::temp_dir().join(format!("laika-xmp-v12-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let photo = dir.join("a.dng").to_string_lossy().to_string();
        let auth = Authorship {
            title: "Harbor <dawn> & gulls".into(),
            caption: "First light over the \"harbor\"".into(),
            headline: "Harbor dawn".into(),
            creator: "Ada Lovelace".into(),
            copyright: "© 2026 Ada".into(),
            rights_usage: "Editorial only".into(),
            contact: "ada@example.com".into(),
            location: "Lisbon".into(),
            keywords: vec!["Places > Portugal > Lisbon".into(), "Boats".into()],
            label: "Green".into(),
            label_names: Vec::new(),
        };
        write(
            &photo,
            &crate::edit::defaults(),
            0,
            &[],
            None,
            &auth,
            &Default::default(),
        )
        .unwrap();
        let raw = std::fs::read_to_string(sidecar_path(&photo)).unwrap();
        // Standard structures, not flat attributes.
        assert!(raw.contains("<dc:title>"), "{raw}");
        assert!(raw.contains("<rdf:Bag>"), "{raw}");
        assert!(raw.contains("<rdf:Seq>"), "{raw}");
        assert!(raw.contains("photoshop:Headline"), "{raw}");
        assert!(raw.contains("Iptc4xmpCore:Location"), "{raw}");
        assert!(!raw.contains("dc:subject=\""), "{raw}");
        // Escaping holds through the round-trip.
        let back = read(&photo).expect("sidecar reads");
        assert_eq!(back.title, "Harbor <dawn> & gulls");
        assert_eq!(back.caption, "First light over the \"harbor\"");
        assert_eq!(back.headline, "Harbor dawn");
        assert_eq!(back.creator, "Ada Lovelace");
        assert_eq!(back.copyright, "© 2026 Ada");
        assert_eq!(back.rights_usage, "Editorial only");
        assert_eq!(back.contact, "ada@example.com");
        assert_eq!(back.location, "Lisbon");
        assert_eq!(back.keywords, vec!["Places > Portugal > Lisbon", "Boats"]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn v12_legacy_flat_sidecars_still_parse() {
        // Pre-V12 sidecars (flat attributes) upgrade on next write but
        // read correctly until then.
        let dir = std::env::temp_dir().join(format!("laika-xmp-v12f-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let photo = dir.join("a.dng").to_string_lossy().to_string();
        std::fs::write(
            sidecar_path(&photo),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="Laika">
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
<rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/" crs:Exposure2012="+0.35" dc:creator="Ada" dc:rights="© Ada" dc:subject="a, b" lr:hierarchicalSubject="Places > Lisbon" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:lr="http://ns.adobe.com/lightroom/1.0/" xmp:Rating="3"/>
</rdf:RDF>
</x:xmpmeta>"#,
        )
        .unwrap();
        let back = read(&photo).expect("legacy reads");
        assert_eq!(back.creator, "Ada");
        assert_eq!(back.copyright, "© Ada");
        assert_eq!(back.rating, Some(3));
        // Full paths win over flat leaves, so hierarchy survives.
        assert_eq!(back.keywords, vec!["Places > Lisbon"]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn v12_lightroom_style_packet_parses() {
        // Hand-written packet in the shape Lightroom writes: bags and
        // Alt lists our reader must accept from foreign files.
        let dir = std::env::temp_dir().join(format!("laika-xmp-v12lr-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let photo = dir.join("a.dng").to_string_lossy().to_string();
        std::fs::write(
            sidecar_path(&photo),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
<rdf:Description rdf:about="" xmlns:dc="http://purl.org/dc/elements/1.1/" xmlns:photoshop="http://ns.adobe.com/photoshop/1.0/" xmlns:xmpRights="http://ns.adobe.com/xap/1.0/rights/" xmlns:lr="http://ns.adobe.com/lightroom/1.0/">
<dc:description><rdf:Alt><rdf:li xml:lang="x-default">LR caption</rdf:li></rdf:Alt></dc:description>
<dc:subject><rdf:Bag><rdf:li>Owl</rdf:li><rdf:li>Night</rdf:li></rdf:Bag></dc:subject>
<photoshop:Headline>LR headline</photoshop:Headline>
<xmpRights:UsageTerms><rdf:Alt><rdf:li xml:lang="x-default">All rights</rdf:li></rdf:Alt></xmpRights:UsageTerms>
<lr:hierarchicalSubject><rdf:Bag><rdf:li>Birds &gt; Owl</rdf:li></rdf:Bag></lr:hierarchicalSubject>
</rdf:Description>
</rdf:RDF>
</x:xmpmeta>"#,
        )
        .unwrap();
        let back = read(&photo).expect("LR packet reads");
        assert_eq!(back.caption, "LR caption");
        assert_eq!(back.headline, "LR headline");
        assert_eq!(back.rights_usage, "All rights");
        assert_eq!(back.keywords, vec!["Birds > Owl"]);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn v22_perspective_round_trips_with_adobe_slider_names() {
        let dir = std::env::temp_dir().join(format!("laika-xmp-v22-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let photo = dir.join("tower.nef").to_string_lossy().to_string();
        let mut params = crate::edit::defaults();
        params[63] = -42.;
        params[64] = 7.;
        params[65] = -1.5;
        params[66] = 12.;
        params[67] = 95.;
        params[68] = -3.;
        params[69] = 4.;
        let mut geom = crate::edit::CropGeom {
            rect: [0.1, 0.05, 0.8, 0.9],
            ..Default::default()
        };
        geom.upright.mode = crate::upright::UprightMode::Guided.code();
        geom.upright.auto = [-18.25, 3.5, 0.75];
        geom.upright
            .set_guides_text("0.1,0.1,0.12,0.9;0.8,0.1,0.78,0.9");
        geom.upright.constrain = false;
        write(&photo, &params, 0, &[], None, &Authorship::default(), &geom).unwrap();
        let raw = std::fs::read_to_string(sidecar_path(&photo)).unwrap();
        for key in [
            "crs:PerspectiveVertical=\"-42\"",
            "crs:PerspectiveHorizontal=\"+7\"",
            "crs:PerspectiveRotate=\"-1.5\"",
            "crs:PerspectiveScale=\"95\"",
            "laika:UprightMode=\"Guided\"",
            "laika:ConstrainCrop=\"False\"",
        ] {
            assert!(raw.contains(key), "{key} missing in {raw}");
        }
        let back = read(&photo).expect("sidecar reads");
        assert_eq!(&back.params[63..70], &params[63..70]);
        let g = back.geom.expect("geometry");
        assert_eq!(g.upright.mode(), crate::upright::UprightMode::Guided);
        assert_eq!(g.upright.auto, [-18.25, 3.5, 0.75]);
        assert_eq!(g.upright.n_guides, 2);
        assert!(!g.upright.constrain);
        // A rewrite never duplicates the perspective keys.
        write(&photo, &params, 0, &[], None, &Authorship::default(), &g).unwrap();
        let raw = std::fs::read_to_string(sidecar_path(&photo)).unwrap();
        assert_eq!(raw.matches("crs:PerspectiveVertical").count(), 1, "{raw}");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn v15_rotation_round_trips_and_foreign_orientation_derives() {
        // V15 gate: rotation + flips survive the sidecar with both
        // orientation keys; foreign Orientation-only files derive
        // geometry; explicit keys never double-apply.
        let dir = std::env::temp_dir().join(format!("laika-xmp-v15-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let photo = dir.join("a.dng").to_string_lossy().to_string();
        let geom = crate::edit::CropGeom {
            rect: [0., 0., 1., 0.5],
            angle: 0.,
            flip_h: false,
            flip_v: false,
            rotation: 1,
            upright: Default::default(),
        };
        write(
            &photo,
            &crate::edit::defaults(),
            0,
            &[],
            None,
            &Authorship::default(),
            &geom,
        )
        .unwrap();
        let raw = std::fs::read_to_string(sidecar_path(&photo)).unwrap();
        assert!(raw.contains("tiff:Orientation=\"6\""), "{raw}");
        assert!(raw.contains("laika:Rotation=\"1\""), "{raw}");
        let back = read(&photo).expect("sidecar reads");
        let g = back.geom.expect("rotation carries geometry");
        assert_eq!(g.rotation, 1);
        assert_eq!(g.rect, [0., 0., 1., 0.5]);
        // Foreign file: Orientation only, no crop keys.
        let fphoto = dir.join("f.dng").to_string_lossy().to_string();
        std::fs::write(
            sidecar_path(&fphoto),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
<rdf:Description xmlns:tiff="http://ns.adobe.com/tiff/1.0/" tiff:Orientation="8"/>
</rdf:RDF>
</x:xmpmeta>"#,
        )
        .unwrap();
        let back = read(&fphoto).expect("foreign orientation reads");
        let g = back.geom.expect("orientation derives geometry");
        assert_eq!((g.rotation, g.flip_h, g.flip_v), (3, false, false));
        // Explicit keys win over a conflicting Orientation (no double).
        let dphoto = dir.join("d.dng").to_string_lossy().to_string();
        std::fs::write(
            sidecar_path(&dphoto),
            r#"<?xml version="1.0" encoding="UTF-8"?>
<x:xmpmeta xmlns:x="adobe:ns:meta/">
<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
<rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/" xmlns:tiff="http://ns.adobe.com/tiff/1.0/" crs:CropLeft="0.000000" crs:CropTop="0.000000" crs:CropRight="1.000000" crs:CropBottom="1.000000" crs:CropAngle="0.00" tiff:Orientation="6" laika:Rotation="0"/>
</rdf:RDF>
</x:xmpmeta>"#,
        )
        .unwrap();
        let back = read(&dphoto).expect("explicit wins");
        assert_eq!(back.geom.expect("geom").rotation, 0);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn wb_mapping_direction() {
        let shot = [2.0, 1.0, 1.5];
        let base = wb_multipliers(5480., 6., shot);
        let warm = wb_multipliers(7000., 6., shot);
        assert!(
            warm[0] > base[0] && warm[2] < base[2],
            "warmer pushes R up, B down"
        );
        let cool = wb_multipliers(3200., 6., shot);
        assert!(cool[0] < base[0] && cool[2] > base[2]);
        let magenta = wb_multipliers(5480., 40., shot);
        assert!(magenta[1] < base[1], "positive tint cuts green");
    }
}
