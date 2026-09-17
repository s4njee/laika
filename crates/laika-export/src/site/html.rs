//! G20/G21: the published page — one `index.html` (inline CSS), a small
//! viewer script, and the embedded fonts. Plain Rust with one escaping
//! function; every user string goes through [`esc`].

use std::fmt::Write as _;

use laika_core::gallery::layout::{self, Breakpoint};
use laika_core::gallery::{Fit, Gallery, TypePairing};

use super::manifest::Derivative;

/// Bump when the markup changes so the page fingerprint (and the diff)
/// notices a renderer upgrade.
pub const RENDERER_VERSION: u32 = 1;

pub const CSS: &str = include_str!("gallery.css");
pub const JS: &str = include_str!("gallery.js");

/// HTML-escape text and attribute values.
pub fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

fn hex(c: u32) -> String {
    format!("#{:06X}", c & 0xFF_FFFF)
}

/// A placed photo with its written derivatives (ascending width).
pub struct PagePhoto<'a> {
    /// Index into `gallery.photos`.
    pub index: usize,
    pub files: &'a [Derivative],
}

/// "48 photographs · Sapporo, Otaru" (count auto-prefixed).
pub fn meta_text(count: usize, meta_line: &str) -> String {
    let n = if count == 1 {
        "1 photograph".to_string()
    } else {
        format!("{count} photographs")
    };
    let m = meta_line.trim();
    if m.is_empty() { n } else { format!("{n} · {m}") }
}

/// Render `index.html`. `photos` are the placed photos that built
/// successfully; failed ones are simply absent from the page.
pub fn render_index(g: &Gallery, photos: &[PagePhoto], generator: &str) -> String {
    let t = &g.theme;
    let tpl = layout::template(&g.template);
    let built: std::collections::HashMap<usize, &[Derivative]> =
        photos.iter().map(|p| (p.index, p.files)).collect();

    // Placements per breakpoint, over the photos that made it (a failed
    // photo leaves no hole on tablet/phone).
    let mut kept = g.clone();
    for (i, p) in kept.photos.iter_mut().enumerate() {
        if built.get(&i).is_none_or(|f| f.is_empty()) {
            p.cell = None;
        }
    }
    let g = &kept;
    let desktop = g.layout_at(Breakpoint::Desktop);
    let tablet: std::collections::HashMap<usize, _> = g
        .layout_at(Breakpoint::Tablet)
        .into_iter()
        .map(|(i, c, sx, sy)| (i, (c, sx, sy)))
        .collect();
    let phone: std::collections::HashMap<usize, _> = g
        .layout_at(Breakpoint::Phone)
        .into_iter()
        .map(|(i, c, sx, sy)| (i, (c, sx, sy)))
        .collect();

    let title = if g.title.trim().is_empty() {
        "Untitled gallery"
    } else {
        g.title.trim()
    };
    let doc_title = if g.site_name.trim().is_empty() {
        esc(title)
    } else {
        format!("{} — {}", esc(title), esc(g.site_name.trim()))
    };
    let mut body_class = vec![match t.pairing {
        TypePairing::SansDisplay => "pair-sans",
        TypePairing::SansMono => "pair-mono",
        TypePairing::LightDisplay => "pair-light",
    }];
    if t.hover_zoom {
        body_class.push("hz");
    }
    if !t.show_captions {
        body_class.push("nocap");
    }
    if tpl.scroll_rows {
        body_class.push("strip");
    }

    let mut h = String::with_capacity(8192);
    h.push_str("<!doctype html>\n<html lang=\"en\">\n<head>\n<meta charset=\"utf-8\">\n");
    h.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    let _ = writeln!(h, "<title>{doc_title}</title>");
    if !g.subtitle.trim().is_empty() {
        let _ = writeln!(h, "<meta name=\"description\" content=\"{}\">", esc(g.subtitle.trim()));
    }
    let _ = writeln!(h, "<meta name=\"generator\" content=\"{}\">", esc(generator));
    let _ = writeln!(
        h,
        "<style>\n:root{{--page:{};--canvas:{};--ink:{};--accent:{};--accent-text:{};--radius:{}px;--title:{}px}}\n{}{}</style>",
        hex(t.page),
        hex(t.canvas),
        hex(t.ink),
        hex(t.accent),
        hex(t.accent_text()),
        t.corner_radius.min(48),
        t.title_size.clamp(20, 120),
        CSS,
        pairing_css(t.pairing),
    );
    h.push_str("</head>\n");
    let _ = writeln!(h, "<body class=\"{}\">", body_class.join(" "));
    h.push_str("<a class=\"skip\" href=\"#photos\">Skip to photos</a>\n");
    if !g.site_name.trim().is_empty() {
        let _ = writeln!(
            h,
            "<header class=\"site\"><span class=\"site-name\">{}</span></header>",
            esc(g.site_name.trim())
        );
    }
    h.push_str("<section class=\"intro\">\n<div>\n");
    if !g.eyebrow.trim().is_empty() {
        let _ = writeln!(h, "<p class=\"eyebrow\">{}</p>", esc(g.eyebrow.trim()));
    }
    let _ = writeln!(h, "<h1>{}</h1>\n</div>\n<div>", esc(title));
    if !g.subtitle.trim().is_empty() {
        let _ = writeln!(h, "<p class=\"lede\">{}</p>", esc(g.subtitle.trim()));
    }
    let _ = writeln!(
        h,
        "<p class=\"meta\">{}</p>\n</div>\n</section>",
        esc(&meta_text(photos.len(), &g.meta_line))
    );
    let _ = writeln!(
        h,
        "<main class=\"wrap\" id=\"photos\">\n<div class=\"grid\" style=\"--dcols:{};--tcols:{};--ratio:{};--g:{}\">",
        g.columns.max(1),
        Breakpoint::Tablet.columns(g.columns),
        trim_float(g.ratio.max(0.1)),
        g.gutter,
    );

    let tiles: Vec<_> = desktop
        .into_iter()
        .filter(|(i, ..)| built.get(i).is_some_and(|f| !f.is_empty()))
        .collect();
    let mut current_row: Option<u16> = None;
    for (n, (i, cell, sx, sy)) in tiles.iter().enumerate() {
        let (i, sx, sy) = (*i, *sx, *sy);
        if tpl.scroll_rows && current_row != Some(cell.row) {
            if current_row.is_some() {
                h.push_str("</div>\n");
            }
            h.push_str("<div class=\"row\">\n");
            current_row = Some(cell.row);
        }
        let p = &g.photos[i];
        let files = built[&i];
        let (tc, tsx, tsy) = tablet.get(&i).copied().unwrap_or((*cell, sx.min(2), sy));
        let (pc, ..) = phone.get(&i).copied().unwrap_or((*cell, 1, 1));
        let _ = write!(
            h,
            "<figure class=\"tile\" style=\"--dc:{};--dr:{};--dsx:{};--dsy:{};--tc:{};--tr:{};--tsx:{};--tsy:{};--pr:{}\">",
            cell.col + 1,
            cell.row + 1,
            sx,
            sy,
            tc.col + 1,
            tc.row + 1,
            tsx,
            tsy,
            pc.row + 1,
        );
        let large = files.last().expect("filtered to non-empty");
        // Default src: the smallest derivative at least ~1280 wide.
        let src = files
            .iter()
            .find(|f| f.width >= 1280)
            .unwrap_or(large);
        let srcset: Vec<String> = files
            .iter()
            .map(|f| format!("{} {}w", esc(&f.file), f.width))
            .collect();
        let cols = g.columns.max(1) as u32;
        let sizes = format!(
            "(max-width: 640px) 100vw, (max-width: 1024px) {}vw, {}vw",
            (100 * tsx as u32 / Breakpoint::Tablet.columns(g.columns) as u32).max(1),
            (100 * sx as u32 / cols).max(1),
        );
        let alt = if !p.alt_text.trim().is_empty() {
            p.alt_text.trim().to_string()
        } else if !p.caption.trim().is_empty() {
            p.caption.trim().to_string()
        } else {
            format!("Photograph {}", n + 1)
        };
        let fit = if p.fit == Fit::Fit { " fit" } else { "" };
        let pos = format!(
            "object-position:{}% {}%",
            trim_float((p.focal.0.clamp(0., 1.) * 100.).round()),
            trim_float((p.focal.1.clamp(0., 1.) * 100.).round())
        );
        let img = format!(
            "<img src=\"{}\" srcset=\"{}\" sizes=\"{}\" width=\"{}\" height=\"{}\" alt=\"{}\" loading=\"{}\" decoding=\"async\" style=\"{}\">",
            esc(&src.file),
            srcset.join(", "),
            sizes,
            src.width,
            src.height,
            esc(&alt),
            if n < 3 { "eager" } else { "lazy" },
            pos,
        );
        if p.open_full_size {
            let _ = write!(
                h,
                "<a class=\"ph{fit}\" href=\"{}\" data-full=\"{}\" data-caption=\"{}\">{img}</a>",
                esc(&large.file),
                esc(&large.file),
                esc(p.caption.trim()),
            );
        } else {
            let _ = write!(h, "<div class=\"ph{fit}\">{img}</div>");
        }
        if !p.caption.trim().is_empty() {
            let _ = write!(h, "<figcaption>{}</figcaption>", esc(p.caption.trim()));
        }
        if g.allow_downloads {
            let _ = write!(
                h,
                "<a class=\"dl\" href=\"{}\" download>Download</a>",
                esc(&large.file)
            );
        }
        h.push_str("</figure>\n");
    }
    if current_row.is_some() {
        h.push_str("</div>\n");
    }
    h.push_str("</div>\n</main>\n");
    let _ = writeln!(h, "<footer class=\"foot\">{}</footer>", esc(generator));
    h.push_str(
        "<div class=\"lb\" id=\"lb\" role=\"dialog\" aria-modal=\"true\" aria-label=\"Photo viewer\" hidden>\n\
<div class=\"lb-bar\"><span class=\"lb-count\" aria-live=\"polite\"></span><button type=\"button\" class=\"lb-close\">Close</button></div>\n\
<div class=\"lb-stage\"><button type=\"button\" class=\"lb-prev\" aria-label=\"Previous photo\">←</button><img alt=\"\"><button type=\"button\" class=\"lb-next\" aria-label=\"Next photo\">→</button></div>\n\
<p class=\"lb-cap\" aria-live=\"polite\"></p>\n</div>\n",
    );
    h.push_str("<script src=\"assets/gallery.js\" defer></script>\n</body>\n</html>\n");
    h
}

fn pairing_css(p: TypePairing) -> &'static str {
    match p {
        TypePairing::SansDisplay => "",
        TypePairing::SansMono => {
            ".pair-mono figcaption,.pair-mono .meta{font-family:var(--mono);letter-spacing:.02em}\n"
        }
        // Light display: 300 falls back to the embedded 400 until a Light
        // face ships (see photogallery.md G18).
        TypePairing::LightDisplay => {
            ".pair-light h1{font-weight:300;letter-spacing:-.01em}.pair-light .lede{opacity:.8}\n"
        }
    }
}

fn trim_float(v: f32) -> String {
    let s = format!("{v:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}
