//! S03: sidecars shared with Lightroom (and Camera Raw / Bridge).
//!
//! - **Naming:** Adobe names a raw file's sidecar `IMG_0001.xmp`; Laika's own
//!   convention is `IMG_0001.NEF.xmp`. When Adobe's file exists (or the
//!   catalog works beside Lightroom) both apps use that one file.
//! - **Merge writes:** Laika rewrites only the fields whose value it changed.
//!   Everything else — Adobe-only settings, nested structures (curves,
//!   profiles, masks, edit history), other apps' namespaces — is carried
//!   through byte-for-byte.
//! - **Last writer:** who wrote a sidecar last (Laika, a named Adobe app,
//!   or another tool), from `laika:Modified`, `xmp:MetadataDate`, and the
//!   `xmpMM:History` trail.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::edit::{CropGeom, PARAM_COUNT};
use crate::xmp::{Authorship, PARAM_KEYS, Sidecar};

static ADOBE_NAMING: AtomicBool = AtomicBool::new(false);

/// Name new raw-file sidecars the Adobe way (`IMG_0001.xmp`). Set per
/// catalog when it works beside Lightroom.
pub fn set_adobe_naming(on: bool) {
    ADOBE_NAMING.store(on, Ordering::Relaxed);
}

pub fn adobe_naming() -> bool {
    ADOBE_NAMING.load(Ordering::Relaxed)
}

/// Adobe's sidecar name for a raw file: the extension replaced by `.xmp`.
pub fn adobe_path(photo_path: &str) -> Option<String> {
    let p = Path::new(photo_path);
    if !laika_raw::is_raw(p) {
        return None;
    }
    Some(p.with_extension("xmp").to_string_lossy().into_owned())
}

/// The sidecar a photo uses: Adobe's file for a raw when it exists or the
/// catalog shares files with Lightroom; otherwise Laika's `<file>.xmp`.
/// JPEG/TIFF/HEIC keep `<file>.xmp` (Adobe embeds their XMP in the file,
/// which Laika never modifies).
pub fn path_for(photo_path: &str) -> String {
    if let Some(adobe) = adobe_path(photo_path) {
        if adobe_naming() || Path::new(&adobe).exists() {
            return adobe;
        }
    }
    format!("{photo_path}.xmp")
}

// ---- document items ----------------------------------------------------------------

/// One top-level property of the sidecar's description, with its source
/// bytes (attribute value as written, or the whole element).
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Item {
    Attr { key: String, raw: String },
    Elem { key: String, raw: String },
}

impl Item {
    fn key(&self) -> &str {
        match self {
            Item::Attr { key, .. } | Item::Elem { key, .. } => key,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Doc {
    /// Namespace declarations (prefix → raw URI) from every level.
    pub decls: Vec<(String, String)>,
    pub items: Vec<Item>,
    /// `x:xmptk` of the writer's toolkit.
    pub toolkit: String,
}

/// Split a sidecar into its top-level properties, keeping source bytes.
/// All top-level `rdf:Description`s are merged (equivalent RDF).
pub(crate) fn parse_doc(bytes: &[u8]) -> Option<Doc> {
    use quick_xml::events::Event;
    use quick_xml::reader::Reader;
    let mut r = Reader::from_reader(bytes);
    let mut doc = Doc::default();
    let mut stack: Vec<String> = Vec::new();
    // (key, start offset, depth at start) of the element being captured.
    let mut capture: Option<(String, usize, usize)> = None;
    let mut saw_rdf = false;
    loop {
        let before = r.buffer_position() as usize;
        let ev = match r.read_event() {
            Ok(ev) => ev,
            Err(_) => return None,
        };
        match ev {
            Event::Start(ref e) | Event::Empty(ref e) => {
                let empty = matches!(ev, Event::Empty(_));
                let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                let parent = stack.last().map(String::as_str).unwrap_or("");
                if name == "rdf:RDF" {
                    saw_rdf = true;
                }
                let top_desc = name == "rdf:Description" && parent == "rdf:RDF";
                if capture.is_none() {
                    for a in e.attributes().with_checks(false).flatten() {
                        let key = String::from_utf8_lossy(a.key.as_ref()).into_owned();
                        let raw = String::from_utf8_lossy(&a.value).into_owned();
                        if let Some(prefix) = key.strip_prefix("xmlns:") {
                            if !doc.decls.iter().any(|(p, _)| p == prefix) {
                                doc.decls.push((prefix.to_string(), raw));
                            }
                        } else if key == "x:xmptk" {
                            doc.toolkit = raw;
                        } else if top_desc && key != "rdf:about" {
                            doc.items.push(Item::Attr { key, raw });
                        }
                    }
                }
                let in_top_desc = parent == "rdf:Description"
                    && stack.len() >= 2
                    && stack[stack.len() - 2] == "rdf:RDF";
                if capture.is_none() && in_top_desc {
                    if empty {
                        let after = r.buffer_position() as usize;
                        doc.items.push(Item::Elem {
                            key: name.clone(),
                            raw: String::from_utf8_lossy(&bytes[before..after]).into_owned(),
                        });
                    } else {
                        capture = Some((name.clone(), before, stack.len()));
                    }
                }
                if !empty {
                    stack.push(name);
                }
            }
            Event::End(_) => {
                stack.pop();
                if let Some((key, start, depth)) = &capture {
                    if stack.len() == *depth {
                        let after = r.buffer_position() as usize;
                        doc.items.push(Item::Elem {
                            key: key.clone(),
                            raw: String::from_utf8_lossy(&bytes[*start..after]).into_owned(),
                        });
                        capture = None;
                    }
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    saw_rdf.then_some(doc)
}

fn serialize(doc: &Doc) -> Vec<u8> {
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str("<x:xmpmeta xmlns:x=\"adobe:ns:meta/\" x:xmptk=\"Laika\">\n");
    out.push_str(" <rdf:RDF xmlns:rdf=\"http://www.w3.org/1999/02/22-rdf-syntax-ns#\">\n");
    out.push_str("  <rdf:Description rdf:about=\"\"");
    for (p, uri) in &doc.decls {
        if p == "x" || p == "rdf" {
            continue;
        }
        out.push_str(&format!("\n    xmlns:{p}=\"{uri}\""));
    }
    for item in &doc.items {
        if let Item::Attr { key, raw } = item {
            // Values from single-quoted sources may hold a bare `"`.
            let raw = raw.replace('"', "&quot;");
            out.push_str(&format!("\n   {key}=\"{raw}\""));
        }
    }
    let elems: Vec<&str> = doc
        .items
        .iter()
        .filter_map(|i| match i {
            Item::Elem { raw, .. } => Some(raw.as_str()),
            _ => None,
        })
        .collect();
    if elems.is_empty() {
        out.push_str("/>\n");
    } else {
        out.push_str(">\n");
        for raw in elems {
            out.push_str("   ");
            out.push_str(raw.trim());
            out.push('\n');
        }
        out.push_str("  </rdf:Description>\n");
    }
    out.push_str(" </rdf:RDF>\n</x:xmpmeta>\n");
    out.into_bytes()
}

// ---- field groups ------------------------------------------------------------------

/// Which Laika field a sidecar property belongs to. `None` = foreign
/// (always carried).
fn key_group(key: &str) -> Option<String> {
    let g = match key {
        "xmp:Rating" => "rating",
        "xmp:Label" => "label",
        "crs:ToneCurvePV2012" => "curve",
        "laika:ChromaticAberration" => "p45",
        "crs:CropLeft" | "crs:CropTop" | "crs:CropRight" | "crs:CropBottom" | "crs:CropAngle" => {
            "crop"
        }
        "laika:FlipH" | "laika:FlipV" => "flip",
        "laika:UprightMode"
        | "laika:UprightAuto"
        | "laika:UprightGuides"
        | "laika:ConstrainCrop" => "upright",
        "tiff:Orientation" | "laika:Rotation" => "orient",
        "dc:title" => "title",
        "dc:description" => "caption",
        "dc:creator" => "creator",
        "dc:rights" | "xmpRights:Marked" | "xmpRights:UsageTerms" => "rights",
        "photoshop:Headline" => "headline",
        "Iptc4xmpCore:Location" => "location",
        "laika:Contact" => "contact",
        "dc:subject" | "lr:hierarchicalSubject" => "keywords",
        "exif:GPSLatitude" | "exif:GPSLongitude" => "gps",
        "laika:History" | "laika:Preset" | "laika:Version" | "laika:Modified" => "laika",
        _ => {
            if let Some(k) = key.strip_prefix("crs:") {
                if let Some((_, i)) = PARAM_KEYS.iter().find(|(pk, _)| *pk == k) {
                    return Some(format!("p{i}"));
                }
            }
            return None;
        }
    };
    Some(g.to_string())
}

const DEVELOP_GROUPS: [&str; 5] = ["curve", "crop", "flip", "upright", "orient"];

fn is_develop_group(g: &str) -> bool {
    DEVELOP_GROUPS.contains(&g) || (g.starts_with('p') && g[1..].parse::<usize>().is_ok())
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() <= 1e-3 * (1. + a.abs().max(b.abs()))
}

/// Groups whose value Laika is about to change relative to the file.
fn changed_groups(
    prev: &Sidecar,
    params: &[f32; PARAM_COUNT],
    rating: u8,
    auth: &Authorship,
    geom: &CropGeom,
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    let mut add = |g: &str| out.push(g.to_string());
    if prev.rating.unwrap_or(0) != rating {
        add("rating");
    }
    // A label text Laika doesn't know (Lightroom's "Select") survives
    // while Laika has none; one of Laika's names is Laika's to clear.
    let prev_label = prev.label.as_deref().unwrap_or("").trim();
    let ours = auth
        .label_names
        .iter()
        .map(String::as_str)
        .chain(crate::labels::DEFAULT_NAMES)
        .any(|n| n.eq_ignore_ascii_case(prev_label));
    if prev_label != auth.label.trim() && (!auth.label.trim().is_empty() || ours) {
        add("label");
    }
    for (_, i) in PARAM_KEYS {
        if !close(prev.params[i], params[i]) {
            out.push(format!("p{i}"));
        }
    }
    if (12..16).any(|i| !close(prev.params[i], params[i])) {
        out.push("curve".to_string());
    }
    if !close(prev.params[45], params[45]) {
        out.push("p45".to_string());
    }
    let pg = prev.geom.unwrap_or_default();
    if pg
        .rect
        .iter()
        .zip(geom.rect.iter())
        .any(|(a, b)| !close(*a, *b))
        || !close(pg.angle, geom.angle)
    {
        out.push("crop".to_string());
    }
    if (pg.flip_h, pg.flip_v) != (geom.flip_h, geom.flip_v) {
        out.push("flip".to_string());
    }
    if pg.upright != geom.upright {
        out.push("upright".to_string());
    }
    if pg.rotation != geom.rotation {
        out.push("orient".to_string());
    }
    let text = [
        ("title", &prev.title, &auth.title),
        ("caption", &prev.caption, &auth.caption),
        ("creator", &prev.creator, &auth.creator),
        ("headline", &prev.headline, &auth.headline),
        ("location", &prev.location, &auth.location),
        ("contact", &prev.contact, &auth.contact),
    ];
    for (g, a, b) in text {
        if a.trim() != b.trim() {
            out.push(g.to_string());
        }
    }
    if prev.copyright.trim() != auth.copyright.trim()
        || prev.rights_usage.trim() != auth.rights_usage.trim()
    {
        out.push("rights".to_string());
    }
    let norm = |v: &[String]| {
        let mut v: Vec<String> = v
            .iter()
            .map(|k| crate::catalog::Catalog::canon_keyword_path(k))
            .collect();
        v.sort();
        v.dedup();
        v
    };
    if norm(&prev.keywords) != norm(&auth.keywords) {
        out.push("keywords".to_string());
    }
    if crate::geo::parse_gps(&prev.gps) != crate::geo::parse_gps(&auth.gps) {
        out.push("gps".to_string());
    }
    out
}

/// What a merged write may change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WriteScope {
    /// Develop settings, crop and orientation. Off when Lightroom leads:
    /// the file's develop fields are then carried untouched.
    pub develop: bool,
}

impl Default for WriteScope {
    fn default() -> Self {
        Self { develop: true }
    }
}

/// Merge Laika's values into the existing sidecar bytes (or start a new
/// one). Only changed fields are rewritten; everything else is carried
/// byte-for-byte.
#[allow(clippy::too_many_arguments)]
pub fn merge(
    existing: Option<&[u8]>,
    params: &[f32; PARAM_COUNT],
    rating: u8,
    history: &[String],
    preset: Option<&str>,
    auth: &Authorship,
    geom: &CropGeom,
    scope: WriteScope,
    now: &str,
) -> Result<Vec<u8>, String> {
    let fresh_bytes = crate::xmp::render(params, rating, history, preset, auth, geom, &[])?;
    let mut fresh = parse_doc(&fresh_bytes).ok_or("render produced no document")?;
    fresh.items.push(Item::Attr {
        key: "laika:Modified".to_string(),
        raw: now.to_string(),
    });
    let old = existing.and_then(|b| crate::xmp::parse(b).zip(parse_doc(b)));
    let (prev, old_doc) = match old {
        Some((p, d)) => (Some(p), d),
        None => (None, Doc::default()),
    };
    let changed: Vec<String> = match &prev {
        Some(p) => changed_groups(p, params, rating, auth, geom),
        // A new file: everything Laika has.
        None => fresh
            .items
            .iter()
            .filter_map(|i| key_group(i.key()))
            .collect(),
    };
    let writes = |g: &str| -> bool {
        if g == "laika" {
            return true;
        }
        if !scope.develop && is_develop_group(g) {
            return false;
        }
        changed.iter().any(|c| c == g)
    };
    let mut out = Doc {
        decls: old_doc.decls.clone(),
        items: Vec::new(),
        toolkit: String::new(),
    };
    for (p, uri) in &fresh.decls {
        if !out.decls.iter().any(|(q, _)| q == p) {
            out.decls.push((p.clone(), uri.clone()));
        }
    }
    let mut placed: Vec<String> = Vec::new();
    for item in &old_doc.items {
        let key = item.key();
        match key_group(key) {
            Some(g) if writes(&g) => {
                // Replace in place (same property from Laika), or drop.
                if let Some(f) = fresh.items.iter().find(|f| f.key() == key) {
                    if !placed.iter().any(|p| p == key) {
                        out.items.push(f.clone());
                        placed.push(key.to_string());
                    }
                }
            }
            _ => {
                if key == "xmp:MetadataDate" && changed.iter().any(|c| writes(c) && c != "laika") {
                    continue;
                }
                out.items.push(item.clone());
            }
        }
    }
    for f in &fresh.items {
        let key = f.key();
        if placed.iter().any(|p| p == key) {
            continue;
        }
        let Some(g) = key_group(key) else { continue };
        if writes(&g) && !out.items.iter().any(|i| i.key() == key) {
            out.items.push(f.clone());
        }
    }
    // Tell Adobe apps the metadata changed.
    if changed.iter().any(|c| writes(c) && c != "laika") {
        out.items.push(Item::Attr {
            key: "xmp:MetadataDate".to_string(),
            raw: now.to_string(),
        });
    }
    Ok(serialize(&out))
}

/// ISO-8601 "now" in UTC, as XMP dates are written.
pub fn now_iso() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    iso_from_epoch(secs)
}

pub fn iso_from_epoch(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let (y, m, d) = civil(days);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Seconds since the epoch from an XMP date ("2025-10-16T09:06:11.60",
/// "…Z", "…+02:00"); times without a zone count as UTC.
pub fn parse_iso(s: &str) -> Option<i64> {
    let s = s.trim();
    let y: i64 = s.get(0..4)?.parse().ok()?;
    let mo: i64 = s.get(5..7)?.parse().ok()?;
    let d: i64 = s.get(8..10)?.parse().ok()?;
    let (mut h, mut mi, mut se) = (0i64, 0i64, 0i64);
    let mut rest = "";
    if let Some(t) = s.get(11..) {
        h = t.get(0..2)?.parse().ok()?;
        mi = t.get(3..5)?.parse().ok()?;
        rest = t.get(5..).unwrap_or("");
        if let Some(r) = rest.strip_prefix(':') {
            se = r.get(0..2)?.parse().ok()?;
            rest = r.get(2..).unwrap_or("");
        }
        // Fractions.
        if let Some(r) = rest.strip_prefix('.') {
            rest = r.trim_start_matches(|c: char| c.is_ascii_digit());
        }
    }
    let offset = match rest.chars().next() {
        Some(sign @ ('+' | '-')) => {
            let oh: i64 = rest.get(1..3)?.parse().ok()?;
            let om: i64 = rest.get(4..6).and_then(|m| m.parse().ok()).unwrap_or(0);
            let o = oh * 3600 + om * 60;
            if sign == '+' { o } else { -o }
        }
        _ => 0,
    };
    let yy = if mo <= 2 { y - 1 } else { y };
    let era = yy.div_euclid(400);
    let yoe = yy - era * 400;
    let mp = (mo + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    Some(days * 86_400 + h * 3600 + mi * 60 + se - offset)
}

// ---- last writer ------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LastWriter {
    /// "Laika", "Lightroom Classic 13.0", "Camera Raw 16.0", "another app".
    pub app: String,
    pub is_laika: bool,
    /// Seconds since the epoch, when known.
    pub when: Option<i64>,
}

/// "Adobe Photoshop Lightroom Classic 13.0 (Macintosh)" → "Lightroom Classic 13.0".
fn short_agent(agent: &str) -> String {
    let a = agent.trim();
    let a = a.split(" (").next().unwrap_or(a);
    let a = a
        .strip_prefix("Adobe Photoshop ")
        .or_else(|| a.strip_prefix("Adobe "))
        .unwrap_or(a);
    a.to_string()
}

/// Values of `stEvt:<field>` in an xmpMM:History element (attribute or
/// element form), in order.
fn history_values(raw: &str, field: &str) -> Vec<String> {
    let mut out = Vec::new();
    let attr = format!("stEvt:{field}=\"");
    let open = format!("<stEvt:{field}>");
    let mut i = 0;
    while i < raw.len() {
        let a = raw[i..].find(&attr).map(|p| (p + i, attr.len(), '"'));
        let e = raw[i..].find(&open).map(|p| (p + i, open.len(), '<'));
        let next = match (a, e) {
            (Some(x), Some(y)) => Some(if x.0 < y.0 { x } else { y }),
            (x, y) => x.or(y),
        };
        let Some((pos, len, end)) = next else { break };
        let start = pos + len;
        let stop = raw[start..]
            .find(end)
            .map(|p| p + start)
            .unwrap_or(raw.len());
        out.push(raw[start..stop].to_string());
        i = stop;
    }
    out
}

/// Who wrote a sidecar last.
pub fn last_writer(bytes: &[u8]) -> Option<LastWriter> {
    let doc = parse_doc(bytes)?;
    let attr = |k: &str| {
        doc.items.iter().find_map(|i| match i {
            Item::Attr { key, raw } if key == k => Some(raw.clone()),
            _ => None,
        })
    };
    let laika = attr("laika:Modified").and_then(|t| parse_iso(&t));
    let laika_file = laika.is_some() || attr("laika:Version").is_some() || doc.toolkit == "Laika";
    let history = doc.items.iter().find_map(|i| match i {
        Item::Elem { key, raw } if key == "xmpMM:History" => Some(raw.clone()),
        _ => None,
    });
    let (agent, hist_when) = history
        .as_deref()
        .map(|h| {
            (
                history_values(h, "softwareAgent").pop(),
                history_values(h, "when").pop().and_then(|w| parse_iso(&w)),
            )
        })
        .unwrap_or((None, None));
    let meta = attr("xmp:MetadataDate").and_then(|t| parse_iso(&t));
    let adobe_when = match (meta, hist_when) {
        (Some(a), Some(b)) => Some(a.max(b)),
        (a, b) => a.or(b),
    };
    if laika_file
        && (adobe_when.is_none() || laika.is_some_and(|l| l + 2 >= adobe_when.unwrap_or(0)))
    {
        return Some(LastWriter {
            app: "Laika".to_string(),
            is_laika: true,
            when: laika,
        });
    }
    let app = match agent {
        Some(a) if !a.trim().is_empty() => short_agent(&a),
        _ if doc.toolkit.starts_with("Adobe XMP") || attr("crs:Version").is_some() => {
            "an Adobe app".to_string()
        }
        _ if laika_file => "Laika".to_string(),
        _ => "another app".to_string(),
    };
    Some(LastWriter {
        is_laika: app == "Laika",
        app,
        when: adobe_when,
    })
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// A sidecar shaped like Lightroom Classic's, with things Laika can't
    /// represent (RGB curves, a profile look, masks, history, a foreign
    /// namespace).
    pub(crate) const ADOBE: &str = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/" x:xmptk="Adobe XMP Core 7.0-c000 1.000000, 0000/00/00-00:00:00        ">
 <rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#">
  <rdf:Description rdf:about=""
    xmlns:xmp="http://ns.adobe.com/xap/1.0/"
    xmlns:xmpMM="http://ns.adobe.com/xap/1.0/mm/"
    xmlns:stEvt="http://ns.adobe.com/xap/1.0/sType/ResourceEvent#"
    xmlns:dc="http://purl.org/dc/elements/1.1/"
    xmlns:lr="http://ns.adobe.com/lightroom/1.0/"
    xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
    xmlns:other="http://example.com/other/1.0/"
   xmp:Rating="3"
   xmp:MetadataDate="2026-09-10T12:00:00+02:00"
   xmpMM:DocumentID="xmp.did:ABC"
   crs:Version="17.1"
   crs:ProcessVersion="15.4"
   crs:Exposure2012="+0.50"
   crs:Contrast2012="+10"
   crs:LensProfileEnable="1"
   other:Tag="keep &amp; me">
   <dc:subject>
    <rdf:Bag>
     <rdf:li>Lisbon</rdf:li>
    </rdf:Bag>
   </dc:subject>
   <crs:ToneCurvePV2012Red>
    <rdf:Seq>
     <rdf:li>0, 0</rdf:li>
     <rdf:li>128, 150</rdf:li>
     <rdf:li>255, 255</rdf:li>
    </rdf:Seq>
   </crs:ToneCurvePV2012Red>
   <crs:Look>
    <rdf:Description crs:Name="Adobe Vivid" crs:Amount="1"/>
   </crs:Look>
   <crs:MaskGroupBasedCorrections><rdf:Seq><rdf:li>mask data</rdf:li></rdf:Seq></crs:MaskGroupBasedCorrections>
   <xmpMM:History>
    <rdf:Seq>
     <rdf:li stEvt:action="saved" stEvt:when="2026-09-10T12:00:00+02:00" stEvt:softwareAgent="Adobe Photoshop Lightroom Classic 13.0 (Macintosh)" stEvt:changed="/metadata"/>
    </rdf:Seq>
   </xmpMM:History>
  </rdf:Description>
 </rdf:RDF>
</x:xmpmeta>
"#;

    fn elem<'a>(doc: &'a Doc, key: &str) -> Option<&'a str> {
        doc.items.iter().find_map(|i| match i {
            Item::Elem { key: k, raw } if k == key => Some(raw.as_str()),
            _ => None,
        })
    }

    fn attr<'a>(doc: &'a Doc, key: &str) -> Option<&'a str> {
        doc.items.iter().find_map(|i| match i {
            Item::Attr { key: k, raw } if k == key => Some(raw.as_str()),
            _ => None,
        })
    }

    fn values_from(bytes: &[u8]) -> (Sidecar, Authorship) {
        let s = crate::xmp::parse(bytes).unwrap();
        let auth = Authorship {
            title: s.title.clone(),
            caption: s.caption.clone(),
            headline: s.headline.clone(),
            creator: s.creator.clone(),
            copyright: s.copyright.clone(),
            rights_usage: s.rights_usage.clone(),
            contact: s.contact.clone(),
            location: s.location.clone(),
            keywords: s.keywords.clone(),
            label: s.label.clone().unwrap_or_default(),
            gps: s.gps.clone(),
            ..Default::default()
        };
        (s, auth)
    }

    #[test]
    fn unchanged_write_keeps_every_foreign_byte() {
        let (s, auth) = values_from(ADOBE.as_bytes());
        let geom = s.geom.unwrap_or_default();
        let out = merge(
            Some(ADOBE.as_bytes()),
            &s.params,
            s.rating.unwrap_or(0),
            &[],
            None,
            &auth,
            &geom,
            WriteScope::default(),
            "2026-09-18T10:00:00Z",
        )
        .unwrap();
        let before = parse_doc(ADOBE.as_bytes()).unwrap();
        let after = parse_doc(&out).unwrap();
        for item in &before.items {
            assert!(after.items.contains(item), "lost {item:?}");
        }
        // Nothing changed → Adobe's MetadataDate stays; Laika marks itself.
        assert_eq!(
            attr(&after, "xmp:MetadataDate"),
            Some("2026-09-10T12:00:00+02:00")
        );
        assert_eq!(attr(&after, "laika:Modified"), Some("2026-09-18T10:00:00Z"));
        assert!(crate::xmp::parse(&out).is_some(), "still valid XMP");
    }

    #[test]
    fn changes_touch_only_their_own_properties() {
        let (s, mut auth) = values_from(ADOBE.as_bytes());
        let mut params = s.params;
        let exp = PARAM_KEYS
            .iter()
            .find(|(k, _)| *k == "Exposure2012")
            .unwrap()
            .1;
        params[exp] = 1.25;
        auth.keywords = vec!["Lisbon".into(), "Tram".into()];
        let out = merge(
            Some(ADOBE.as_bytes()),
            &params,
            5,
            &[],
            None,
            &auth,
            &s.geom.unwrap_or_default(),
            WriteScope::default(),
            "2026-09-18T10:00:00Z",
        )
        .unwrap();
        let after = parse_doc(&out).unwrap();
        let before = parse_doc(ADOBE.as_bytes()).unwrap();
        assert_eq!(attr(&after, "crs:Exposure2012"), Some("+1.25"));
        assert_eq!(
            attr(&after, "crs:Contrast2012"),
            Some("+10"),
            "untouched value keeps its text"
        );
        assert_eq!(attr(&after, "xmp:Rating"), Some("5"));
        assert_eq!(attr(&after, "other:Tag"), Some("keep &amp; me"));
        for k in [
            "crs:ToneCurvePV2012Red",
            "crs:Look",
            "crs:MaskGroupBasedCorrections",
            "xmpMM:History",
        ] {
            assert_eq!(elem(&after, k), elem(&before, k), "{k} byte-for-byte");
        }
        assert_eq!(
            attr(&after, "xmp:MetadataDate"),
            Some("2026-09-18T10:00:00Z")
        );
        let back = crate::xmp::parse(&out).unwrap();
        assert!(back.keywords.iter().any(|k| k == "Tram"));
        // Laika's own point curve wasn't changed, so none is written.
        assert!(attr(&after, "crs:ToneCurvePV2012").is_none());
        assert_eq!(last_writer(&out).unwrap().app, "Laika");
    }

    #[test]
    fn lightroom_leads_never_writes_develop_fields() {
        let (s, auth) = values_from(ADOBE.as_bytes());
        let mut params = s.params;
        params[2] = -2.0;
        params[13] = 0.5; // curve
        let mut geom = s.geom.unwrap_or_default();
        geom.rect = [0.1, 0.1, 0.8, 0.8];
        geom.rotation = 1;
        let out = merge(
            Some(ADOBE.as_bytes()),
            &params,
            4,
            &[],
            None,
            &auth,
            &geom,
            WriteScope { develop: false },
            "2026-09-18T10:00:00Z",
        )
        .unwrap();
        let after = parse_doc(&out).unwrap();
        let crs_before: Vec<&Item> = parse_doc(ADOBE.as_bytes())
            .unwrap()
            .items
            .clone()
            .leak()
            .iter()
            .filter(|i| i.key().starts_with("crs:"))
            .collect();
        let crs_after: Vec<&Item> = after
            .items
            .iter()
            .filter(|i| i.key().starts_with("crs:"))
            .collect();
        assert_eq!(crs_before, crs_after, "develop fields untouched");
        for k in ["tiff:Orientation", "laika:Rotation", "laika:FlipH"] {
            assert!(attr(&after, k).is_none(), "{k} not written");
        }
        assert_eq!(
            attr(&after, "xmp:Rating"),
            Some("4"),
            "metadata still flows"
        );
    }

    #[test]
    fn last_writer_reads_history_and_laika_marks() {
        let w = last_writer(ADOBE.as_bytes()).unwrap();
        assert_eq!(w.app, "Lightroom Classic 13.0");
        assert!(!w.is_laika);
        assert_eq!(w.when, parse_iso("2026-09-10T10:00:00Z"));
        // Laika wrote after Lightroom.
        let (s, auth) = values_from(ADOBE.as_bytes());
        let laika = merge(
            Some(ADOBE.as_bytes()),
            &s.params,
            1,
            &[],
            None,
            &auth,
            &s.geom.unwrap_or_default(),
            WriteScope::default(),
            "2026-09-18T10:00:00Z",
        )
        .unwrap();
        assert!(last_writer(&laika).unwrap().is_laika);
        // Lightroom writes again later (keeps laika:Modified, adds history).
        let again = String::from_utf8(laika).unwrap().replace(
            "</rdf:Seq>\n   </xmpMM:History>",
            "<rdf:li stEvt:action=\"saved\" stEvt:when=\"2026-09-19T08:00:00Z\" stEvt:softwareAgent=\"Adobe Photoshop Lightroom Classic 13.1 (Macintosh)\"/></rdf:Seq>\n   </xmpMM:History>",
        );
        assert_eq!(
            last_writer(again.as_bytes()).unwrap().app,
            "Lightroom Classic 13.1"
        );
    }

    #[test]
    fn adobe_sidecar_names_for_raw_files() {
        let dir = std::env::temp_dir().join(format!("laika-sidecar-name-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let raw = dir.join("IMG_0001.NEF").to_string_lossy().into_owned();
        let jpg = dir.join("IMG_0002.JPG").to_string_lossy().into_owned();
        assert_eq!(path_for(&raw), format!("{raw}.xmp"));
        std::fs::write(dir.join("IMG_0001.xmp"), ADOBE).unwrap();
        assert_eq!(path_for(&raw), dir.join("IMG_0001.xmp").to_string_lossy());
        assert_eq!(
            path_for(&jpg),
            format!("{jpg}.xmp"),
            "JPEGs keep Laika's name"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn iso_dates_round_trip() {
        let t = parse_iso("2026-09-18T10:00:00Z").unwrap();
        assert_eq!(iso_from_epoch(t), "2026-09-18T10:00:00Z");
        assert_eq!(parse_iso("2026-09-18T12:00:00+02:00"), Some(t));
        assert_eq!(parse_iso("2026-09-18T10:00:00.60"), Some(t));
        assert_eq!(parse_iso("2026-09-18T10:00"), Some(t));
    }
}
