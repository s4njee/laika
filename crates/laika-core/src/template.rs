//! Token-based folder and rename templates (V02).
//!
//! Folder templates (`{yyyy}/{mm}/{shoot}`) place copy imports; rename
//! templates (`{date}_{seq:4}_{original}`) name the files. Rendering is pure
//! and total: unknown tokens and empty results are errors, everything else
//! (including Unicode camera names) passes through with only filesystem-
//! invalid characters replaced — and the dialog previews the exact strings
//! before anything is written.

use std::path::{Path, PathBuf};

/// Built-in folder presets: (name, template).
pub const FOLDER_PRESETS: [(&str, &str); 3] = [
    ("Dated", "{yyyy}/{yyyy}-{mm}-{dd}"),
    ("Month + shoot", "{yyyy}/{mm}/{shoot}"),
    ("Camera days", "{camera}/{yyyy}-{mm}-{dd}"),
];

/// Built-in rename presets: (name, template).
pub const RENAME_PRESETS: [(&str, &str); 3] = [
    ("Date + sequence", "{date}_{seq:4}_{original}"),
    ("Shoot + sequence", "{shoot}-{seq:4}"),
    ("Camera + sequence", "{camera}{seq:4}"),
];

/// Per-file inputs for template rendering.
#[derive(Clone, Debug, Default)]
pub struct NameCtx {
    /// EXIF capture time (`YYYY:MM:DD …`) or empty when unknown.
    pub captured_at: String,
    /// File mtime fallback, epoch secs.
    pub mtime_secs: i64,
    /// Original file stem (no extension) and extension (no dot).
    pub original_stem: String,
    pub original_ext: String,
    /// Camera model as reported (may be empty or Unicode).
    pub camera: String,
    /// Free shoot text for this run.
    pub shoot: String,
    /// Catalog display name.
    pub catalog: String,
    /// Import batch stamp (stable before the run for previews).
    pub batch: String,
    /// Sequence number for `{seq}` (already offset by the start number).
    pub seq: u64,
}

impl NameCtx {
    /// (yyyy, mm, dd) from capture time, else from file mtime, else unknowns.
    pub fn date_parts(&self) -> (String, String, String) {
        // `get` (not slicing): a multi-byte char inside the first 10 bytes
        // must fall back, never panic. Digits only — blank EXIF dates
        // (`    :  :  `) would otherwise render as whitespace folders.
        if let Some(head) = self.captured_at.get(..10) {
            let norm = head.replace(':', "-");
            let p: Vec<&str> = norm.split('-').collect();
            if p.len() == 3
                && p[0].len() == 4
                && p[1].len() == 2
                && p[2].len() == 2
                && p.iter().all(|s| s.bytes().all(|b| b.is_ascii_digit()))
            {
                return (p[0].into(), p[1].into(), p[2].into());
            }
        }
        if self.mtime_secs > 0 {
            // Days since epoch → civil date (Howard Hinnant's algorithm).
            let z = (self.mtime_secs / 86400) + 719468;
            let era = if z >= 0 { z } else { z - 146096 } / 146097;
            let doe = z - era * 146097;
            let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
            let y = yoe + era * 400;
            let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
            let mp = (5 * doy + 2) / 153;
            let d = doy - (153 * mp + 2) / 5 + 1;
            let m = if mp < 10 { mp + 3 } else { mp - 9 };
            let y = if m <= 2 { y + 1 } else { y };
            return (format!("{y:04}"), format!("{m:02}"), format!("{d:02}"));
        }
        ("unknown".into(), "unknown".into(), "unknown".into())
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Piece {
    Literal(String),
    Token { key: String, arg: Option<String> },
}

fn parse_template(t: &str) -> Result<Vec<Piece>, String> {
    let mut out = Vec::new();
    let mut lit = String::new();
    let mut chars = t.char_indices().peekable();
    while let Some((_, c)) = chars.next() {
        if c == '{' {
            if matches!(chars.peek(), Some((_, '{'))) {
                chars.next();
                lit.push('{');
                continue;
            }
            let mut inner = String::new();
            let mut closed = false;
            for (_, c2) in chars.by_ref() {
                if c2 == '}' {
                    closed = true;
                    break;
                }
                inner.push(c2);
            }
            if !closed {
                return Err(format!("unclosed '{{' in template {t:?}"));
            }
            if !lit.is_empty() {
                out.push(Piece::Literal(std::mem::take(&mut lit)));
            }
            let (key, arg) = match inner.split_once(':') {
                Some((k, a)) => (k.to_string(), Some(a.to_string())),
                None => (inner, None),
            };
            out.push(Piece::Token { key, arg });
        } else if c == '}' {
            if matches!(chars.peek(), Some((_, '}'))) {
                chars.next();
                lit.push('}');
            } else {
                return Err(format!("unmatched '}}' in template {t:?}"));
            }
        } else {
            lit.push(c);
        }
    }
    if !lit.is_empty() {
        out.push(Piece::Literal(lit));
    }
    Ok(out)
}

/// All renderable token names, for error messages.
pub const TOKEN_HELP: &str =
    "{yyyy} {yy} {mm} {dd} {date} {camera} {shoot} {catalog} {batch} {original} {seq[:width]}";

fn token_value(
    key: &str,
    arg: Option<&str>,
    ctx: &NameCtx,
    date: &(String, String, String),
) -> Result<String, String> {
    let (yyyy, mm, dd) = (&date.0, &date.1, &date.2);
    match key {
        "yyyy" => Ok(yyyy.clone()),
        "yy" => Ok(yyyy
            .chars()
            .rev()
            .take(2)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect()),
        "mm" => Ok(mm.clone()),
        "dd" => Ok(dd.clone()),
        "date" => Ok(format!("{yyyy}-{mm}-{dd}")),
        "camera" => Ok(sanitize_segment(&ctx.camera)),
        "shoot" => Ok(sanitize_segment(&ctx.shoot)),
        "catalog" => Ok(sanitize_segment(&ctx.catalog)),
        "batch" => Ok(sanitize_segment(&ctx.batch)),
        "original" => Ok(sanitize_segment(&ctx.original_stem)),
        "seq" => {
            let width: usize = match arg {
                Some(a) => a
                    .parse()
                    .map_err(|_| format!("bad seq width {{{key}:{a}}} — use e.g. {{seq:4}}"))?,
                None => 4,
            };
            if width > 12 {
                return Err("seq width over 12 makes no sense".to_string());
            }
            Ok(format!("{:0>width$}", ctx.seq, width = width))
        }
        _ => Err(format!("unknown token {{{key}}} — available: {TOKEN_HELP}")),
    }
}

/// Replace filesystem-invalid characters; keep Unicode, spaces, dots.
/// Trims trailing dots/spaces (illegal on Windows endings).
pub fn sanitize_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '/' | '\\' | '<' | '>' | ':' | '"' | '|' | '?' | '*')
            || c == '\0'
            || c.is_control()
        {
            out.push('_');
        } else {
            out.push(c);
        }
    }
    out.trim_end_matches(['.', ' ']).to_string()
}

/// Render a folder template to segments (empty segments dropped so an
/// empty `{shoot}` never creates `//`).
pub fn render_folder(template: &str, ctx: &NameCtx) -> Result<Vec<String>, String> {
    let date = ctx.date_parts();
    let mut rendered = String::new();
    for piece in parse_template(template)? {
        match piece {
            Piece::Literal(s) => rendered.push_str(&s),
            Piece::Token { key, arg } => {
                rendered.push_str(&token_value(&key, arg.as_deref(), ctx, &date)?)
            }
        }
    }
    Ok(rendered
        .split('/')
        .map(sanitize_segment)
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty() && *s != ".")
        .collect())
}

/// Render a rename template to a filename (original extension appended;
/// leading/trailing separators trimmed so empty tokens stay clean).
pub fn render_filename(template: &str, ctx: &NameCtx) -> Result<String, String> {
    let date = ctx.date_parts();
    let mut rendered = String::new();
    for piece in parse_template(template)? {
        match piece {
            Piece::Literal(s) => rendered.push_str(&s),
            Piece::Token { key, arg } => {
                rendered.push_str(&token_value(&key, arg.as_deref(), ctx, &date)?)
            }
        }
    }
    let mut name = sanitize_segment(&rendered);
    while name.starts_with(['-', '_', '.', ' ']) {
        name.remove(0);
    }
    while name.ends_with(['-', '_', ' ', '.']) {
        name.pop();
    }
    if name.is_empty() {
        return Err(
            "rename produced an empty name — fill shoot text or pick another template".to_string(),
        );
    }
    if ctx.original_ext.is_empty() {
        Ok(name)
    } else {
        Ok(format!("{name}.{}", ctx.original_ext))
    }
}

/// Resolve one import file to its placed directory + filename.
pub fn resolve_import_dest(
    root: &Path,
    folder_template: &str,
    rename_template: Option<&str>,
    ctx: &NameCtx,
) -> Result<(PathBuf, String), String> {
    let mut dir = root.to_path_buf();
    for seg in render_folder(folder_template, ctx)? {
        dir.push(seg);
    }
    let filename = match rename_template {
        Some(t) if !t.trim().is_empty() => render_filename(t, ctx)?,
        _ => {
            // Original name, sanitized but otherwise untouched.
            let stem = sanitize_segment(&ctx.original_stem);
            if ctx.original_ext.is_empty() {
                stem
            } else {
                format!("{stem}.{}", ctx.original_ext)
            }
        }
    };
    Ok((dir, filename))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> NameCtx {
        NameCtx {
            captured_at: "2026:06:14 18:42:00".into(),
            mtime_secs: 0,
            original_stem: "DSC_4412".into(),
            original_ext: "NEF".into(),
            camera: "Nikon D600".into(),
            shoot: "Mori Tan".into(),
            catalog: "Laika".into(),
            batch: "1750000000".into(),
            seq: 7,
        }
    }

    #[test]
    fn folder_presets_render() {
        let c = ctx();
        assert_eq!(
            render_folder("{yyyy}/{yyyy}-{mm}-{dd}", &c).unwrap(),
            vec!["2026", "2026-06-14"]
        );
        assert_eq!(
            render_folder("{yyyy}/{mm}/{shoot}", &c).unwrap(),
            vec!["2026", "06", "Mori Tan"]
        );
        assert_eq!(
            render_folder("{camera}/{yyyy}-{mm}-{dd}", &c).unwrap(),
            vec!["Nikon D600", "2026-06-14"]
        );
    }

    #[test]
    fn rename_presets_render() {
        let c = ctx();
        assert_eq!(
            render_filename("{date}_{seq:4}_{original}", &c).unwrap(),
            "2026-06-14_0007_DSC_4412.NEF"
        );
        assert_eq!(
            render_filename("{shoot}-{seq}", &c).unwrap(),
            "Mori Tan-0007.NEF"
        );
        assert_eq!(
            render_filename("{camera}{seq:4}", &c).unwrap(),
            "Nikon D6000007.NEF"
        );
    }

    #[test]
    fn empty_tokens_stay_clean() {
        let mut c = ctx();
        c.shoot = String::new();
        c.camera = String::new();
        assert_eq!(
            render_folder("{yyyy}/{mm}/{shoot}", &c).unwrap(),
            vec!["2026", "06"]
        );
        assert_eq!(render_filename("{shoot}-{seq}", &c).unwrap(), "0007.NEF");
    }

    #[test]
    fn bad_templates_explain() {
        let c = ctx();
        let err = render_filename("{bogus}", &c).unwrap_err();
        assert!(err.contains("{bogus}") && err.contains("{seq"), "{err}");
        assert!(render_filename("{date", &c).is_err());
        assert!(render_filename("}", &c).is_err());
        assert!(render_filename("{seq:99}", &c).is_err());
        assert!(render_filename("{seq:abc}", &c).is_err());
        // A shoot of pure punctuation sanitizes to nothing → empty error.
        let mut c2 = c.clone();
        c2.shoot = "///".into();
        assert!(render_filename("{shoot}", &c2).is_err());
    }

    #[test]
    fn sanitize_keeps_unicode_replaces_invalid() {
        assert_eq!(sanitize_segment("Mori Tan 李雷"), "Mori Tan 李雷");
        assert_eq!(sanitize_segment("a/b\\c:d"), "a_b_c_d");
        assert_eq!(sanitize_segment("trail. "), "trail");
        assert_eq!(sanitize_segment("Canon EOS R5"), "Canon EOS R5");
    }

    #[test]
    fn mtime_fallback_and_escapes() {
        let mut c = ctx();
        c.captured_at.clear();
        c.mtime_secs = 1750468800; // 2025-06-21 01:20 UTC
        assert_eq!(c.date_parts(), ("2025".into(), "06".into(), "21".into()));
        // Blank or non-ASCII EXIF dates fall back to mtime (no panic).
        c.captured_at = "    :  :     :  :  ".into();
        assert_eq!(c.date_parts(), ("2025".into(), "06".into(), "21".into()));
        c.captured_at = "2026:06:1é 18:42:00".into();
        assert_eq!(c.date_parts(), ("2025".into(), "06".into(), "21".into()));
        c.captured_at.clear();
        assert_eq!(
            render_filename("{{literal}}_{original}", &c).unwrap(),
            "{literal}_DSC_4412.NEF"
        );
    }

    #[test]
    fn ten_thousand_paths_stay_unique() {
        // V02 gate, pure render: distinct names stay distinct at scale.
        let mut c = ctx();
        let mut set = std::collections::HashSet::new();
        for n in 0..10_000u64 {
            c.seq = n;
            c.original_stem = format!("DSC_{n:04}");
            let p = render_filename("{date}_{seq:4}_{original}", &c).unwrap();
            assert!(set.insert(p), "collision at {n}");
        }
        assert_eq!(set.len(), 10_000);
    }
}
