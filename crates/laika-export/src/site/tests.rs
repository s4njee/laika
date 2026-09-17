use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use laika_core::gallery::layout::{Breakpoint, TEMPLATES};
use laika_core::gallery::{Fit, Gallery};

use super::build::{BuildOpts, SitePhoto, build, derivative_edges, preview_diff};
use super::html::{PagePhoto, esc, render_index};
use super::manifest::{Derivative, Manifest};

fn workdir(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("laika-site-{tag}-{}", std::process::id()));
    std::fs::remove_dir_all(&d).ok();
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn fixture_gallery(n: i64) -> (Gallery, Vec<SitePhoto>) {
    let mut g = Gallery {
        id: 7,
        title: "Hokkaidō, February".into(),
        subtitle: "Snow & <silence> — \"quiet\" days 🌨".into(),
        eyebrow: "Travel · 2026".into(),
        slug: "hokkaido-february".into(),
        site_name: "Jun Mirakawa".into(),
        meta_line: "Sapporo, Otaru".into(),
        ..Gallery::default()
    };
    g.add_photos(&(1..=n).collect::<Vec<_>>());
    g.apply_template("mixed");
    g.photos[1].caption = "Harbor <dawn> & 'gulls'".into();
    g.photos[1].alt_text = "Boats at sunrise".into();
    g.photos[2].fit = Fit::Fit;
    g.photos[2].focal = (0.25, 0.8);
    g.photos[3].open_full_size = false;
    let photos = (1..=n)
        .map(|id| SitePhoto {
            photo_id: id,
            source_hash: format!("hash{id}"),
            edit_key: "e0".into(),
            name: format!("DSCF{id:04}.jpg"),
        })
        .collect();
    (g, photos)
}

fn opts(out: &Path) -> BuildOpts {
    BuildOpts {
        out_dir: out.to_path_buf(),
        sizes: None,
        fonts_dir: Some(PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../assets/fonts"
        ))),
        generator: "Laika test".into(),
        laika_version: "0.0.0".into(),
        workers: 4,
    }
}

/// Landscape for odd ids, a small portrait for even ones (smaller than
/// 1280, so it must not be upscaled).
fn synth(p: &SitePhoto, _max: u32) -> Result<image::RgbaImage, String> {
    let (w, h) = if p.photo_id % 2 == 1 {
        (2400, 1600)
    } else {
        (800, 1200)
    };
    Ok(image::RgbaImage::from_pixel(
        w,
        h,
        image::Rgba([(p.photo_id * 40) as u8, 90, 120, 255]),
    ))
}

/// Minimal well-formedness check: non-void tags balance.
fn assert_balanced(html: &str) {
    const VOID: [&str; 6] = ["meta", "img", "br", "link", "input", "!doctype"];
    let mut stack: Vec<String> = Vec::new();
    let mut rest = html;
    // Skip the inline style/script bodies (they contain `<`-free CSS).
    while let Some(start) = rest.find('<') {
        rest = &rest[start + 1..];
        let end = rest.find('>').expect("unterminated tag");
        let tag = &rest[..end];
        rest = &rest[end + 1..];
        let closing = tag.starts_with('/');
        let name: String = tag
            .trim_start_matches('/')
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '!')
            .collect::<String>()
            .to_ascii_lowercase();
        if VOID.contains(&name.as_str()) {
            continue;
        }
        if closing {
            assert_eq!(
                stack.pop().as_deref(),
                Some(name.as_str()),
                "mismatched </{name}>"
            );
        } else {
            stack.push(name);
        }
    }
    assert!(stack.is_empty(), "unclosed: {stack:?}");
}

#[test]
fn escaping() {
    assert_eq!(
        esc("<a href=\"x\">'&'</a> 🌨"),
        "&lt;a href=&quot;x&quot;&gt;&#39;&amp;&#39;&lt;/a&gt; 🌨"
    );
}

#[test]
fn derivative_edges_never_upscale() {
    assert_eq!(
        derivative_edges(3000, 2000, &[640, 1280, 2048]),
        vec![640, 1280, 2048]
    );
    assert_eq!(
        derivative_edges(800, 1200, &[640, 1280, 2048]),
        vec![640, 1200]
    );
    assert_eq!(derivative_edges(300, 200, &[640, 1280]), vec![300]);
}

#[test]
fn build_tree_manifest_and_incremental_rebuilds() {
    let dir = workdir("build");
    let out = dir.join("hokkaido-february");
    let (mut g, photos) = fixture_gallery(6);
    let cancel = AtomicBool::new(false);
    let renders = AtomicUsize::new(0);
    let render = |p: &SitePhoto, m: u32| {
        renders.fetch_add(1, Ordering::Relaxed);
        synth(p, m)
    };

    let r = build(&g, &photos, &opts(&out), &render, |_| {}, &cancel).unwrap();
    assert_eq!((r.photos, r.rendered, r.reused), (6, 6, 0));
    assert!(r.diff.first_build);
    for f in [
        "index.html",
        "manifest.json",
        "assets/gallery.js",
        "assets/fonts/IBMPlexSans-Regular.ttf",
    ] {
        assert!(out.join(f).is_file(), "{f}");
    }
    let m = Manifest::read(&out).unwrap();
    assert_eq!(m.photos.len(), 6);
    assert_eq!(
        m.photos[0]
            .files
            .iter()
            .map(|f| f.width)
            .collect::<Vec<_>>(),
        vec![640, 1280, 2048]
    );
    // Portrait 800×1200: long edges 640 and 1200 → widths 426 and 800.
    let portrait = &m.photos.iter().find(|p| p.photo_id == 2).unwrap().files;
    assert_eq!(
        portrait
            .iter()
            .map(|f| (f.width, f.height))
            .collect::<Vec<_>>(),
        vec![(426, 640), (800, 1200)]
    );
    for p in &m.photos {
        for f in &p.files {
            let meta = std::fs::metadata(out.join(&f.file)).unwrap();
            assert_eq!(meta.len(), f.bytes);
            let img = image::open(out.join(&f.file)).unwrap();
            assert_eq!((img.width(), img.height()), (f.width, f.height));
        }
    }
    let html = std::fs::read_to_string(out.join("index.html")).unwrap();
    assert_balanced(&html);
    assert!(html.contains("Harbor &lt;dawn&gt; &amp; &#39;gulls&#39;"));
    assert!(!html.contains("<dawn>") && !html.contains("<silence>"));
    assert!(html.contains("🌨"));
    assert!(
        !html.contains("http://") && !html.contains("https://"),
        "no external requests"
    );
    assert_eq!(html.matches("<figure").count(), 6);
    assert_eq!(
        html.matches("data-full=").count(),
        5,
        "photo 4 doesn't open full size"
    );
    assert!(html.contains("6 photographs · Sapporo, Otaru"));

    // Caption edit: zero re-encodes, page changes.
    renders.store(0, Ordering::Relaxed);
    g.photos[0].caption = "New caption".into();
    assert_eq!(
        preview_diff(&g, &photos, &out).summary(),
        "6 unchanged, page changed"
    );
    let r = build(&g, &photos, &opts(&out), &render, |_| {}, &cancel).unwrap();
    assert_eq!(
        (r.rendered, r.reused, renders.load(Ordering::Relaxed)),
        (0, 6, 0)
    );
    assert!(
        std::fs::read_to_string(out.join("index.html"))
            .unwrap()
            .contains("New caption")
    );

    // One exposure edit: one re-render, the old derivative is gone.
    let mut edited = photos.clone();
    edited[4].edit_key = "e1".into();
    let d = preview_diff(&g, &edited, &out);
    assert_eq!(
        (d.reencoded.clone(), d.unchanged, d.page_changed),
        (vec![5], 5, false)
    );
    let old_file = m.photos.iter().find(|p| p.photo_id == 5).unwrap().files[0]
        .file
        .clone();
    let r = build(&g, &edited, &opts(&out), &render, |_| {}, &cancel).unwrap();
    assert_eq!((r.rendered, r.reused), (1, 5));
    assert!(!out.join(&old_file).exists(), "stale derivative removed");

    // Unplaced photos drop out of the output.
    g.unplace(6);
    let r = build(&g, &edited, &opts(&out), &render, |_| {}, &cancel).unwrap();
    assert_eq!((r.photos, r.diff.removed.clone()), (5, vec![6]));
    let html = std::fs::read_to_string(out.join("index.html")).unwrap();
    assert_eq!(html.matches("<figure").count(), 5);

    // Cancel leaves the previous build intact and no staging behind.
    let before = std::fs::read_to_string(out.join("index.html")).unwrap();
    let cancelled = AtomicBool::new(true);
    g.title = "Changed".into();
    assert!(
        build(&g, &edited, &opts(&out), &render, |_| {}, &cancelled)
            .unwrap_err()
            .contains("cancelled")
    );
    assert_eq!(
        std::fs::read_to_string(out.join("index.html")).unwrap(),
        before
    );
    let leftovers: Vec<_> = std::fs::read_dir(&dir)
        .unwrap()
        .flatten()
        .map(|e| e.file_name())
        .collect();
    assert_eq!(leftovers.len(), 1, "{leftovers:?}");

    // Per-photo failures: the rest builds and says so.
    let flaky = |p: &SitePhoto, m: u32| {
        if p.photo_id == 3 {
            Err("original is offline".to_string())
        } else {
            synth(p, m)
        }
    };
    let mut changed = edited.clone();
    changed[2].edit_key = "e2".into();
    let r = build(&g, &changed, &opts(&out), flaky, |_| {}, &cancel).unwrap();
    assert_eq!(
        r.failures,
        vec![(
            "DSCF0003.jpg".to_string(),
            "original is offline".to_string()
        )]
    );
    assert_eq!(r.photos, 4);
    assert!(r.summary().contains("1 failed"));
    std::fs::remove_dir_all(&dir).ok();
}

#[test]
fn empty_gallery_refuses_to_build() {
    let dir = workdir("empty");
    let g = Gallery::default();
    let e = build(
        &g,
        &[],
        &opts(&dir.join("x")),
        synth,
        |_| {},
        &AtomicBool::new(false),
    )
    .unwrap_err();
    assert!(e.contains("place at least one photo"));
    std::fs::remove_dir_all(&dir).ok();
}

fn fake_files(n: usize) -> Vec<Vec<Derivative>> {
    (0..n)
        .map(|i| {
            vec![
                Derivative {
                    width: 640,
                    height: 427,
                    file: format!("img/k{i}-640.jpg"),
                    bytes: 1,
                },
                Derivative {
                    width: 2048,
                    height: 1365,
                    file: format!("img/k{i}-2048.jpg"),
                    bytes: 1,
                },
            ]
        })
        .collect()
}

/// Pull `--name:value` out of each figure's style attribute.
fn tile_vars(html: &str, name: &str) -> Vec<u16> {
    html.split("<figure class=\"tile\" style=\"")
        .skip(1)
        .map(|s| {
            let style = &s[..s.find('"').unwrap()];
            style
                .split(';')
                .find_map(|kv| kv.strip_prefix(&format!("--{name}:")))
                .unwrap()
                .parse()
                .unwrap()
        })
        .collect()
}

#[test]
fn page_placements_match_the_engine_for_every_template() {
    let files = fake_files(14);
    for t in TEMPLATES {
        let (mut g, _) = fixture_gallery(14);
        g.apply_template(t.id);
        let page: Vec<PagePhoto> = (0..14)
            .map(|i| PagePhoto {
                index: i,
                files: &files[i],
            })
            .collect();
        let html = render_index(&g, &page, "Laika test");
        assert_balanced(&html);
        let desk = g.layout_at(Breakpoint::Desktop);
        assert_eq!(
            tile_vars(&html, "dc"),
            desk.iter().map(|r| r.1.col + 1).collect::<Vec<_>>(),
            "{}",
            t.id
        );
        assert_eq!(
            tile_vars(&html, "dr"),
            desk.iter().map(|r| r.1.row + 1).collect::<Vec<_>>(),
            "{}",
            t.id
        );
        assert_eq!(
            tile_vars(&html, "dsx"),
            desk.iter().map(|r| r.2 as u16).collect::<Vec<_>>(),
            "{}",
            t.id
        );
        let tab = g.layout_at(Breakpoint::Tablet);
        let tc: Vec<u16> = desk
            .iter()
            .map(|d| tab.iter().find(|x| x.0 == d.0).unwrap().1.col + 1)
            .collect();
        assert_eq!(tile_vars(&html, "tc"), tc, "{}", t.id);
        if t.scroll_rows {
            assert!(html.contains("class=\"row\""));
        }
    }
}

#[test]
fn golden_index_html() {
    let (mut g, _) = fixture_gallery(5);
    g.theme.hover_zoom = true;
    g.allow_downloads = true;
    let files = fake_files(5);
    let page: Vec<PagePhoto> = (0..5)
        .map(|i| PagePhoto {
            index: i,
            files: &files[i],
        })
        .collect();
    let html = render_index(&g, &page, "Laika test");
    let golden = PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/golden/gallery-index.html"
    ));
    if std::env::var_os("LAIKA_UPDATE_GOLDEN").is_some() || !golden.exists() {
        std::fs::create_dir_all(golden.parent().unwrap()).unwrap();
        std::fs::write(&golden, &html).unwrap();
    }
    let want = std::fs::read_to_string(&golden).unwrap();
    assert!(
        html == want,
        "index.html drifted from {} — rerun with LAIKA_UPDATE_GOLDEN=1 if intended",
        golden.display()
    );
}

/// Manual check: builds a demo gallery from JPEGs in `LAIKA_SITE_DEMO_SRC`
/// (cycled to 14 photos) into `LAIKA_SITE_DEMO_OUT`, per template in
/// `LAIKA_SITE_DEMO_TEMPLATE` (default mixed). Open its index.html.
#[test]
#[ignore]
fn demo_site_from_folder() {
    let src = PathBuf::from(std::env::var("LAIKA_SITE_DEMO_SRC").expect("LAIKA_SITE_DEMO_SRC"));
    let out = PathBuf::from(std::env::var("LAIKA_SITE_DEMO_OUT").expect("LAIKA_SITE_DEMO_OUT"));
    let mut files: Vec<PathBuf> = std::fs::read_dir(&src)
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("jpg")))
        .collect();
    files.sort();
    let (mut g, photos) = fixture_gallery(14);
    g.apply_template(&std::env::var("LAIKA_SITE_DEMO_TEMPLATE").unwrap_or_else(|_| "mixed".into()));
    g.theme.show_captions = true;
    g.allow_downloads = std::env::var_os("LAIKA_SITE_DEMO_DL").is_some();
    for (i, p) in g.photos.iter_mut().enumerate() {
        if p.caption.is_empty() && i % 3 == 0 {
            p.caption = format!("Frame {}", i + 1);
        }
    }
    let render = |p: &SitePhoto, _m: u32| {
        let f = &files[(p.photo_id as usize - 1) % files.len()];
        image::open(f)
            .map(|i| i.to_rgba8())
            .map_err(|e| e.to_string())
    };
    let r = build(
        &g,
        &photos,
        &opts(&out),
        &render,
        |_| {},
        &AtomicBool::new(false),
    )
    .unwrap();
    eprintln!("{} → {}", r.summary(), out.display());
}
