//! G22–G24: build a gallery (preview in the browser, or publish to a
//! folder), deploy with wrangler, and show what will change.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use laika_core::gallery::Status;
use laika_export::site::{self, BuildDiff, BuildOpts, BuildReport, Manifest, SitePhoto};

use super::gallery_ui::*;
use super::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BuildKind {
    Preview,
    Publish { deploy: bool },
}

pub(crate) struct BuildRun {
    pub kind: BuildKind,
    pub gallery_id: i64,
    pub done: usize,
    pub total: usize,
    pub current: String,
    pub cancel: Arc<AtomicBool>,
}

impl BuildRun {
    pub fn label(&self) -> String {
        let what = match self.kind {
            BuildKind::Preview => "Building preview",
            BuildKind::Publish { .. } => "Building",
        };
        if self.total == 0 {
            format!("{what}…")
        } else {
            format!("{what}… {}/{}", self.done.min(self.total), self.total)
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Destination {
    Folder,
    Cloudflare,
}

pub(crate) struct PublishSheet {
    pub dest: Destination,
    pub diff: Option<BuildDiff>,
    pub estimate: Option<u64>,
    pub wrangler: Option<PathBuf>,
    pub log: Arc<Mutex<Vec<String>>>,
    pub deploying: bool,
    /// Last outcome: Ok(message) or Err(reason).
    pub result: Option<Result<String, String>>,
    pub url: Option<String>,
    pub failures: Vec<(String, String)>,
}

/// One photo's render inputs, captured on the main thread.
#[derive(Clone)]
struct RenderJob {
    photo_id: i64,
    src_path: String,
    blake3: String,
    smart: bool,
    params: [f32; edit::PARAM_COUNT],
    geom: laika_develop::CropRender,
}

fn default_gallery_root() -> PathBuf {
    laika_core::platform::pictures_dir().join("Laika Galleries")
}

fn fonts_dir() -> Option<PathBuf> {
    // Bundled app: Contents/Resources/fonts; dev: the workspace assets.
    let bundled = laika_core::platform::resource_dir()?.join("fonts");
    if bundled.join(site::build::FONT_FILES[0]).is_file() {
        return Some(bundled);
    }
    let dev = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/fonts"));
    dev.join(site::build::FONT_FILES[0])
        .is_file()
        .then_some(dev)
}

/// GUI apps get a minimal PATH: look where npm and Homebrew install.
pub(crate) fn find_wrangler() -> Option<PathBuf> {
    let home = laika_core::platform::home_dir();
    #[cfg(target_os = "windows")]
    let mut candidates = {
        let mut paths = vec![
            home.join("AppData/Roaming/npm/wrangler.cmd"),
            home.join(".bun/bin/wrangler.exe"),
            home.join(".volta/bin/wrangler.cmd"),
        ];
        if let Some(appdata) = std::env::var_os("APPDATA") {
            paths.push(PathBuf::from(appdata).join("npm/wrangler.cmd"));
        }
        paths
    };
    #[cfg(not(target_os = "windows"))]
    let mut candidates = vec![
        PathBuf::from("/opt/homebrew/bin/wrangler"),
        PathBuf::from("/usr/local/bin/wrangler"),
        home.join(".npm-global/bin/wrangler"),
        home.join(".bun/bin/wrangler"),
        home.join(".volta/bin/wrangler"),
    ];
    if let Some(path) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&path) {
            #[cfg(target_os = "windows")]
            candidates.extend([dir.join("wrangler.cmd"), dir.join("wrangler.exe")]);
            #[cfg(not(target_os = "windows"))]
            candidates.push(dir.join("wrangler"));
        }
    }
    candidates.into_iter().find(|p| p.is_file())
}

/// A `*.pages.dev` (or any https) URL from wrangler's output.
pub(crate) fn deploy_url(lines: &[String]) -> Option<String> {
    let urls: Vec<String> = lines
        .iter()
        .flat_map(|l| l.split_whitespace())
        .filter(|w| w.starts_with("https://"))
        .map(|w| w.trim_end_matches(['.', ',', ')']).to_string())
        .collect();
    urls.iter()
        .find(|u| u.contains(".pages.dev"))
        .or(urls.last())
        .cloned()
}

/// Parallel photos per build: half the cores, 2–4 (each holds a
/// full-resolution decode, ~300 MB at 24 MP).
fn build_workers() -> usize {
    let cores = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4);
    (cores / 2).clamp(2, 4)
}

fn fmt_bytes(b: u64) -> String {
    if b >= 1 << 30 {
        format!("{:.1} GB", b as f64 / (1u64 << 30) as f64)
    } else if b >= 1 << 20 {
        format!("{:.0} MB", b as f64 / (1u64 << 20) as f64)
    } else {
        format!("{:.0} KB", (b as f64 / 1024.).max(1.))
    }
}

/// Decode + develop one photo at full resolution (the export path).
fn render_photo(
    renderer: &laika_develop::Renderer,
    cache_dir: &Path,
    job: &RenderJob,
) -> Result<image::RgbaImage, String> {
    let src = PathBuf::from(&job.src_path);
    let linear = if job.smart {
        laika_raw::decode::decode_editing(&src, &job.blake3, cache_dir)
            .map_err(|_| "original offline and no smart preview".to_string())?
    } else {
        if !src.exists() {
            return Err("original is offline — reconnect its drive".to_string());
        }
        let is_raw = laika_raw::is_raw(&src);
        match laika_raw::on_big_stack(move || {
            if is_raw {
                laika_raw::decode::decode(&src)
            } else {
                laika_raw::decode::linear_from_raster(&src, None)
            }
        }) {
            Ok(Ok(img)) => img,
            Ok(Err(e)) | Err(e) => return Err(laika_raw::decode::failure_reason(&e)),
        }
    };
    let frame = renderer
        .render_export(linear, job.params, 0., job.geom)
        .map_err(|e| format!("render failed: {e}"))?;
    image::RgbaImage::from_raw(frame.width, frame.height, frame.rgba)
        .ok_or_else(|| "bad pixels".to_string())
}

impl Laika {
    /// Placed photos' site inputs and render jobs (edit fingerprint =
    /// acknowledged params + geometry).
    fn gallery_jobs(&self) -> (Vec<SitePhoto>, Vec<RenderJob>) {
        let Some(g) = self.gal.current.as_ref() else {
            return (Vec::new(), Vec::new());
        };
        let mut photos = Vec::new();
        let mut jobs = Vec::new();
        for p in g.photos.iter().filter(|p| p.cell.is_some()) {
            let Some(row) = self.find(p.photo_id) else {
                continue;
            };
            let base = self
                .state
                .edits
                .get(&p.photo_id)
                .map(|e| e.params)
                .unwrap_or_else(edit::defaults);
            let params = self.effective_values(p.photo_id, base);
            let geom = self.acknowledged_export_geom(p.photo_id);
            let smart = self.offline.contains(&p.photo_id)
                && self
                    .cache_dir
                    .join(&row.blake3)
                    .join("linear-2048.f16")
                    .exists();
            let edit_key = format!("{params:?}|{geom:?}|{smart}");
            photos.push(SitePhoto {
                photo_id: p.photo_id,
                source_hash: row.blake3.clone(),
                edit_key,
                name: row.filename.clone(),
            });
            jobs.push(RenderJob {
                photo_id: p.photo_id,
                src_path: row.path.clone(),
                blake3: row.blake3.clone(),
                smart,
                params,
                geom,
            });
        }
        (photos, jobs)
    }

    fn publish_dir(&self) -> Option<PathBuf> {
        let g = self.gal.current.as_ref()?;
        if !g.output_dir.trim().is_empty() {
            return Some(PathBuf::from(g.output_dir.trim()));
        }
        (!g.slug.is_empty()).then(|| default_gallery_root().join(&g.slug))
    }

    fn preview_dir(&self, id: i64) -> PathBuf {
        std::env::temp_dir()
            .join("laika-gallery-preview")
            .join(id.to_string())
    }

    /// G22: build at 1280 only into a temp folder and open the browser.
    pub(crate) fn preview_gallery(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.gal.current.as_ref().map(|g| g.id) else {
            return;
        };
        let out = self.preview_dir(id);
        self.run_gallery_build(BuildKind::Preview, out, Some(vec![1280]), cx);
    }

    pub(crate) fn open_publish_sheet(&mut self, cx: &mut Context<Self>) {
        if self.gal.current.is_none() {
            return;
        }
        let deploy_project = self
            .gal
            .current
            .as_ref()
            .map(|g| g.deploy_project.clone())
            .unwrap_or_default();
        self.gal.publish = Some(PublishSheet {
            dest: if deploy_project.is_empty() {
                Destination::Folder
            } else {
                Destination::Cloudflare
            },
            diff: None,
            estimate: None,
            wrangler: find_wrangler(),
            log: Arc::new(Mutex::new(Vec::new())),
            deploying: false,
            result: None,
            url: self
                .gal
                .current
                .as_ref()
                .map(|g| g.last_deploy_url.clone())
                .filter(|u| !u.is_empty()),
            failures: Vec::new(),
        });
        self.refresh_publish_diff();
        cx.notify();
    }

    pub(crate) fn close_publish_sheet(&mut self, cx: &mut Context<Self>) {
        if self.gal.publish.as_ref().is_some_and(|p| p.deploying) {
            self.status_note = "deploy still running — it continues in the background".to_string();
        }
        if self.field.is_some() {
            self.commit_field(cx);
        }
        self.gal.publish = None;
        cx.notify();
    }

    pub(crate) fn refresh_publish_diff_pub(&mut self) {
        if self.gal.publish.is_some() {
            self.refresh_publish_diff();
        }
    }

    fn refresh_publish_diff(&mut self) {
        let dir = self.publish_dir();
        let (photos, _) = self.gallery_jobs();
        let Some(g) = self.gal.current.as_ref() else {
            return;
        };
        let (diff, estimate) = match dir {
            Some(d) => {
                let prev = Manifest::read(&d);
                (
                    Some(site::preview_diff(g, &photos, &d)),
                    site::manifest::estimate_bytes(prev.as_ref(), photos.len()),
                )
            }
            None => (None, None),
        };
        if let Some(sheet) = self.gal.publish.as_mut() {
            sheet.diff = diff;
            sheet.estimate = estimate;
        }
    }

    pub(crate) fn publish_build(&mut self, deploy: bool, cx: &mut Context<Self>) {
        if self.field.is_some() && !self.commit_field(cx) {
            return;
        }
        let Some(g) = self.gal.current.as_ref() else {
            return;
        };
        if g.slug.is_empty() && g.output_dir.trim().is_empty() {
            self.set_publish_result(Err(
                "set a web address (slug) on the Page tab first".to_string()
            ));
            cx.notify();
            return;
        }
        if deploy {
            if self
                .gal
                .publish
                .as_ref()
                .is_some_and(|p| p.wrangler.is_none())
            {
                self.set_publish_result(Err(
                    "wrangler isn't installed — run `npm install -g wrangler` and `wrangler login`"
                        .to_string(),
                ));
                cx.notify();
                return;
            }
            if g.deploy_project.trim().is_empty() {
                self.set_publish_result(Err("name the Cloudflare Pages project first".to_string()));
                cx.notify();
                return;
            }
        }
        let Some(out) = self.publish_dir() else {
            return;
        };
        self.run_gallery_build(BuildKind::Publish { deploy }, out, None, cx);
    }

    fn set_publish_result(&mut self, r: Result<String, String>) {
        match self.gal.publish.as_mut() {
            Some(s) => s.result = Some(r),
            None => {
                self.status_note = match r {
                    Ok(m) | Err(m) => m,
                }
            }
        }
    }

    fn run_gallery_build(
        &mut self,
        kind: BuildKind,
        out: PathBuf,
        sizes: Option<Vec<u32>>,
        cx: &mut Context<Self>,
    ) {
        if self.gal.build.is_some() {
            self.status_note = "a gallery build is already running".to_string();
            cx.notify();
            return;
        }
        let Some(g) = self.gal.current.clone() else {
            return;
        };
        let (photos, jobs) = self.gallery_jobs();
        if photos.is_empty() {
            let msg = "place at least one photo on the page first".to_string();
            if kind == BuildKind::Preview {
                self.status_note = msg;
            } else {
                self.set_publish_result(Err(msg));
            }
            cx.notify();
            return;
        }
        self.ensure_dev(cx);
        let Some(renderer) = self.dev.as_ref().map(|d| d.renderer.clone()) else {
            self.set_publish_result(Err(
                "GPU unavailable — building needs the renderer".to_string()
            ));
            self.status_note = "GPU unavailable — building needs the renderer".to_string();
            cx.notify();
            return;
        };
        let cancel = Arc::new(AtomicBool::new(false));
        self.gal.build = Some(BuildRun {
            kind,
            gallery_id: g.id,
            done: 0,
            total: photos.len(),
            current: String::new(),
            cancel: cancel.clone(),
        });
        if let Some(s) = self.gal.publish.as_mut() {
            s.result = None;
            s.failures.clear();
        }
        let progress = Arc::new(Mutex::new((0usize, photos.len(), String::new())));
        let result: Arc<Mutex<Option<Result<BuildReport, String>>>> = Arc::new(Mutex::new(None));
        let opts = BuildOpts {
            out_dir: out.clone(),
            sizes,
            fonts_dir: fonts_dir(),
            generator: format!("Laika {}", env!("CARGO_PKG_VERSION")),
            laika_version: env!("CARGO_PKG_VERSION").to_string(),
            workers: build_workers(),
        };
        let cache_dir = self.cache_dir.clone();
        {
            let progress = progress.clone();
            let result = result.clone();
            let cancel = cancel.clone();
            cx.background_spawn(async move {
                let r = site::build(
                    &g,
                    &photos,
                    &opts,
                    |sp, _max| {
                        let job = jobs
                            .iter()
                            .find(|j| j.photo_id == sp.photo_id)
                            .ok_or_else(|| "photo left the catalog".to_string())?;
                        render_photo(&renderer, &cache_dir, job)
                    },
                    |p| {
                        if let Ok(mut s) = progress.lock() {
                            *s = (p.done, p.total, p.current);
                        }
                    },
                    &cancel,
                );
                if let Ok(mut slot) = result.lock() {
                    *slot = Some(r);
                }
            })
            .detach();
        }
        cx.spawn(async move |entity, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(150))
                    .await;
                let done = result.lock().ok().and_then(|mut s| s.take());
                let snapshot = progress.lock().ok().map(|s| s.clone());
                let finished = done.is_some();
                let alive = entity
                    .update(cx, |this, cx| {
                        if let (Some(run), Some((d, t, c))) = (this.gal.build.as_mut(), snapshot) {
                            run.done = d;
                            run.total = t;
                            run.current = c;
                        }
                        if let Some(r) = done {
                            this.finish_gallery_build(kind, out.clone(), r, cx);
                        }
                        cx.notify();
                    })
                    .is_ok();
                if finished || !alive {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }

    fn finish_gallery_build(
        &mut self,
        kind: BuildKind,
        out: PathBuf,
        r: Result<BuildReport, String>,
        cx: &mut Context<Self>,
    ) {
        let run = self.gal.build.take();
        let gid = run.map(|r| r.gallery_id);
        let report = match r {
            Ok(rep) => rep,
            Err(e) => {
                eprintln!(
                    "[gallery] build FAILED ({kind:?}) at {}: {e}",
                    out.display()
                );
                if kind == BuildKind::Preview {
                    self.status_note = format!("preview failed — {e}");
                } else {
                    self.set_publish_result(Err(e));
                }
                return;
            }
        };
        let failed = report.failures.len();
        eprintln!(
            "[gallery] built ({kind:?}) {} at {}",
            report.summary(),
            out.display()
        );
        for (name, reason) in &report.failures {
            eprintln!("[gallery] FAILED {name}: {reason}");
        }
        match kind {
            BuildKind::Preview => {
                let index = out.join("index.html");
                if let Err(e) = laika_core::platform::open_path(&index) {
                    self.status_note = format!(
                        "preview built at {} but the browser didn't open ({e})",
                        index.display()
                    );
                } else {
                    self.status_note = if failed == 0 {
                        format!("preview opened · {}", report.summary())
                    } else {
                        format!(
                            "preview opened · {failed} photo{} missing: {}",
                            if failed == 1 { "" } else { "s" },
                            report.failures[0].1
                        )
                    };
                }
            }
            BuildKind::Publish { deploy } => {
                let same = self.gal.current.as_ref().is_some_and(|g| Some(g.id) == gid);
                if same {
                    if let Some(g) = self.gal.current.as_mut() {
                        g.last_build_at = unix_now();
                        g.last_build_dir = out.to_string_lossy().to_string();
                    }
                    self.gal_save();
                }
                let msg = format!(
                    "built {} · {} at {}",
                    report.summary(),
                    fmt_bytes(report.bytes),
                    out.display()
                );
                if let Some(s) = self.gal.publish.as_mut() {
                    s.failures = report.failures.clone();
                }
                self.set_publish_result(Ok(msg));
                self.refresh_publish_diff();
                if deploy && same {
                    self.deploy_gallery(out, cx);
                }
            }
        }
    }

    /// `wrangler pages deploy <dir> --project-name <project>`, streaming
    /// its output into the sheet.
    fn deploy_gallery(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        let Some(sheet) = self.gal.publish.as_mut() else {
            return;
        };
        let Some(wrangler) = sheet.wrangler.clone() else {
            return;
        };
        let Some(g) = self.gal.current.as_ref() else {
            return;
        };
        let project = g.deploy_project.trim().to_string();
        let gid = g.id;
        sheet.deploying = true;
        sheet.url = None;
        let log = sheet.log.clone();
        if let Ok(mut l) = log.lock() {
            l.clear();
            l.push(format!(
                "$ wrangler pages deploy {} --project-name {project}",
                dir.display()
            ));
        }
        let status: Arc<Mutex<Option<Result<(), String>>>> = Arc::new(Mutex::new(None));
        {
            let log = log.clone();
            let status = status.clone();
            std::thread::spawn(move || {
                use std::io::{BufRead, BufReader};
                let path = std::env::var("PATH").unwrap_or_default();
                #[cfg(target_os = "windows")]
                let mut command = {
                    let is_script =
                        wrangler
                            .extension()
                            .and_then(|e| e.to_str())
                            .is_some_and(|e| {
                                e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat")
                            });
                    if is_script {
                        let mut cmd = std::process::Command::new("cmd.exe");
                        cmd.arg("/C").arg(&wrangler);
                        cmd
                    } else {
                        std::process::Command::new(&wrangler)
                    }
                };
                #[cfg(not(target_os = "windows"))]
                let mut command = std::process::Command::new(&wrangler);
                command
                    .arg("pages")
                    .arg("deploy")
                    .arg(&dir)
                    .arg("--project-name")
                    .arg(&project)
                    .arg("--commit-dirty=true")
                    .env("CI", "1")
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::piped());
                #[cfg(target_os = "macos")]
                command.env("PATH", format!("/opt/homebrew/bin:/usr/local/bin:{path}"));
                #[cfg(not(target_os = "macos"))]
                command.env("PATH", path);
                let child = command.spawn();
                let mut child = match child {
                    Ok(c) => c,
                    Err(e) => {
                        *status.lock().unwrap() =
                            Some(Err(format!("couldn't start wrangler: {e}")));
                        return;
                    }
                };
                let err = child.stderr.take().map(|e| {
                    let log = log.clone();
                    std::thread::spawn(move || {
                        for line in BufReader::new(e).lines().map_while(Result::ok) {
                            eprintln!("[deploy] {line}");
                            log.lock().unwrap().push(line);
                        }
                    })
                });
                if let Some(out) = child.stdout.take() {
                    for line in BufReader::new(out).lines().map_while(Result::ok) {
                        eprintln!("[deploy] {line}");
                        log.lock().unwrap().push(line);
                    }
                }
                if let Some(t) = err {
                    t.join().ok();
                }
                let r = match child.wait() {
                    Ok(s) if s.success() => Ok(()),
                    Ok(s) => Err(format!("wrangler exited with {s} — see the log")),
                    Err(e) => Err(e.to_string()),
                };
                *status.lock().unwrap() = Some(r);
            });
        }
        cx.spawn(async move |entity, cx| {
            loop {
                cx.background_executor()
                    .timer(std::time::Duration::from_millis(250))
                    .await;
                let done = status.lock().ok().and_then(|mut s| s.take());
                let finished = done.is_some();
                let alive = entity
                    .update(cx, |this, cx| {
                        if let Some(r) = done {
                            let lines = log.lock().map(|l| l.clone()).unwrap_or_default();
                            let url = deploy_url(&lines);
                            if let Some(s) = this.gal.publish.as_mut() {
                                s.deploying = false;
                            }
                            match r {
                                Ok(()) => {
                                    if let Some(g) =
                                        this.gal.current.as_mut().filter(|g| g.id == gid)
                                    {
                                        g.last_deploy_at = unix_now();
                                        g.last_deploy_url = url.clone().unwrap_or_default();
                                        g.status = Status::Published;
                                    }
                                    this.gal_save();
                                    if let Some(s) = this.gal.publish.as_mut() {
                                        s.url = url.clone();
                                    }
                                    this.set_publish_result(Ok(match url {
                                        Some(u) => format!("deployed — live at {u}"),
                                        None => {
                                            "deployed (no URL in wrangler's output — see the log)"
                                                .to_string()
                                        }
                                    }));
                                }
                                Err(e) => {
                                    eprintln!("[deploy] FAILED: {e}");
                                    this.set_publish_result(Err(format!(
                                        "deploy failed, the build is kept on disk — {e}"
                                    )))
                                }
                            }
                        }
                        cx.notify();
                    })
                    .is_ok();
                if finished || !alive {
                    break;
                }
            }
        })
        .detach();
        cx.notify();
    }

    // ---- the sheet ------------------------------------------------------------------

    pub(crate) fn publish_sheet(&self, cx: &mut Context<Self>) -> Div {
        let g = self.gal.current.as_ref().expect("open gallery");
        let s = self.gal.publish.as_ref().expect("sheet open");
        let building = self
            .gal
            .build
            .as_ref()
            .filter(|b| matches!(b.kind, BuildKind::Publish { .. }));
        let dir = self.publish_dir();
        let busy = building.is_some() || s.deploying;
        let label = |t: &str| {
            div()
                .w(px(110.))
                .flex_none()
                .text_size(sp(11.5))
                .text_color(rgb(TEXT_TERTIARY))
                .child(t.to_string())
        };
        let dest_button = |id: &'static str, text: &str, on: bool| {
            div()
                .id(id)
                .px(px(11.))
                .py(px(5.))
                .rounded(px(4.))
                .text_size(sp(11.5))
                .when(on, |d| {
                    d.bg(rgb(bg_segment_active())).text_color(rgb(TEXT_PRIMARY))
                })
                .when(!on, |d| {
                    d.text_color(rgb(TEXT_MUTED))
                        .hover(|d| d.text_color(rgb(TEXT_PRIMARY)))
                })
                .child(text.to_string())
        };
        let log_lines: Vec<String> = s
            .log
            .lock()
            .map(|l| l.iter().rev().take(14).rev().cloned().collect())
            .unwrap_or_default();
        let summary = match (&s.diff, dir.is_some()) {
            (_, false) => "Set a web address on the Page tab to choose where it builds".to_string(),
            (Some(d), true) => d.summary(),
            (None, true) => String::new(),
        };
        let estimate = match s.estimate {
            Some(b) => format!("≈ {} of images", fmt_bytes(b)),
            None => "no size estimate yet (first build)".to_string(),
        };
        let content = div()
            .p(px(24.))
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(
                div()
                    .flex()
                    .justify_between()
                    .child(
                        div()
                            .text_size(sp(19.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(TEXT_PRIMARY))
                            .child(format!(
                                "Publish “{}”",
                                if g.title.trim().is_empty() { "Untitled gallery" } else { g.title.trim() }
                            )),
                    )
                    .child(
                        div()
                            .id("gal-sheet-close")
                            .text_size(sp(15.))
                            .text_color(rgb(TEXT_DIM))
                            .hover(|d| d.text_color(rgb(TEXT_PRIMARY)))
                            .on_click(cx.listener(|this, _, _, cx| this.close_publish_sheet(cx)))
                            .child("✕"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .child(label("Destination"))
                    .child(
                        div()
                            .flex()
                            .p(px(2.))
                            .rounded(px(5.))
                            .bg(rgb(bg_segment_shell()))
                            .child(dest_button("gal-dest-folder", "Folder only", s.dest == Destination::Folder).on_click(cx.listener(|this, _, _, cx| {
                                if let Some(s) = this.gal.publish.as_mut() {
                                    s.dest = Destination::Folder;
                                }
                                cx.notify();
                            })))
                            .child(dest_button("gal-dest-cf", "Cloudflare Pages", s.dest == Destination::Cloudflare).on_click(cx.listener(|this, _, _, cx| {
                                if let Some(s) = this.gal.publish.as_mut() {
                                    s.dest = Destination::Cloudflare;
                                    s.wrangler = find_wrangler();
                                }
                                cx.notify();
                            })))
                            .child(
                                dest_button("gal-dest-s3", "S3", false)
                                    .opacity(0.45)
                                    .on_hover(self.tip("S3-compatible hosting is planned; use Folder or Cloudflare for now")),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(label("Folder"))
                    .child(
                        div().flex_1().min_w_0().flex().child(self.field_cell(
                            text_input::FieldId::GalleryOutputDir,
                            div()
                                .flex_1()
                                .min_w_0()
                                .px(px(7.))
                                .py(px(5.))
                                .rounded(px(4.))
                                .bg(rgb(bg_well()))
                                .overflow_hidden()
                                .text_size(sp(11.))
                                .text_color(rgb(if g.output_dir.is_empty() { TEXT_DIM } else { TEXT_SECONDARY }))
                                .child(
                                    dir.as_ref()
                                        .map(|d| d.display().to_string())
                                        .unwrap_or_else(|| "~/Pictures/Laika Galleries/<web address>".to_string()),
                                ),
                            false,
                            "Where the site is written (empty uses ~/Pictures/Laika Galleries/<web address>)",
                            cx,
                        )),
                    )
                    .child(
                        div()
                            .id("gal-choose-dir")
                            .px(px(8.))
                            .py(px(5.))
                            .rounded(px(4.))
                            .border_1()
                            .border_color(border_control())
                            .text_size(sp(11.))
                            .text_color(rgb(TEXT_SECONDARY))
                            .hover(|d| d.bg(rgb(bg_row_hover())))
                            .on_click(cx.listener(|this, _, _, cx| this.choose_gallery_folder(cx)))
                            .child("Choose…"),
                    )
                    .when(dir.as_ref().is_some_and(|d| d.exists()), |d| {
                        let target = dir.clone().unwrap_or_default();
                        d.child(
                            div()
                                .id("gal-reveal")
                                .px(px(8.))
                                .py(px(5.))
                                .rounded(px(4.))
                                .border_1()
                                .border_color(border_control())
                                .text_size(sp(11.))
                                .text_color(rgb(TEXT_SECONDARY))
                                .hover(|d| d.bg(rgb(bg_row_hover())))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Err(e) = laika_core::import::reveal_in_manager(&target.join("index.html")) {
                                        this.status_note = e;
                                    }
                                    cx.notify();
                                }))
                                .child("Reveal"),
                        )
                    }),
            )
            .when(s.dest == Destination::Cloudflare, |d| {
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(label("Pages project"))
                        .child(
                            div().flex_1().min_w_0().flex().child(self.field_cell(
                                text_input::FieldId::GalleryProject,
                                div()
                                    .flex_1()
                                    .px(px(7.))
                                    .py(px(5.))
                                    .rounded(px(4.))
                                    .bg(rgb(bg_well()))
                                    .text_size(sp(11.))
                                    .text_color(rgb(if g.deploy_project.is_empty() { TEXT_DIM } else { TEXT_SECONDARY }))
                                    .child(if g.deploy_project.is_empty() {
                                        "my-photos".to_string()
                                    } else {
                                        g.deploy_project.clone()
                                    }),
                                false,
                                "Cloudflare Pages project name (created on first deploy)",
                                cx,
                            )),
                        ),
                )
                .child(
                    div()
                        .pl(px(110.))
                        .text_size(sp(10.5))
                        .text_color(rgb(if s.wrangler.is_some() { TEXT_DIM } else { WARNING }))
                        .child(match &s.wrangler {
                            Some(p) => format!("wrangler found at {}", p.display()),
                            None => "wrangler not found — install with `npm install -g wrangler`, then run `wrangler login`".to_string(),
                        }),
                )
            })
            .child(
                div()
                    .p(px(12.))
                    .rounded(px(6.))
                    .bg(rgb(bg_well()))
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .text_size(sp(11.5))
                    .text_color(rgb(TEXT_SECONDARY))
                    .child(summary)
                    .child(div().text_color(rgb(TEXT_DIM)).child(format!(
                        "{} placed photos · {} · sizes {}",
                        g.placed_count(),
                        estimate,
                        g.sizes.iter().map(|s| s.to_string()).collect::<Vec<_>>().join(" / ")
                    )))
                    .child(div().text_color(rgb(TEXT_DIM)).child("Images are re-rendered with your edits and carry no EXIF or GPS.")),
            )
            .children(building.map(|b| {
                let frac = if b.total == 0 { 0. } else { b.done as f32 / b.total as f32 };
                div()
                    .flex()
                    .flex_col()
                    .gap(px(5.))
                    .child(
                        div()
                            .flex()
                            .justify_between()
                            .text_size(sp(11.))
                            .text_color(rgb(TEXT_SECONDARY))
                            .child(b.label())
                            .child(div().text_color(rgb(TEXT_DIM)).child(b.current.clone())),
                    )
                    .child(
                        div()
                            .h(px(3.))
                            .rounded(px(2.))
                            .bg(rgb(track()))
                            .child(div().h_full().w(relative(frac)).rounded(px(2.)).bg(rgb(progress_fill()))),
                    )
            }))
            .when(s.deploying || !log_lines.is_empty(), |d| {
                d.child(
                    div()
                        .id("gal-deploy-log")
                        .max_h(px(160.))
                        .overflow_y_scroll()
                        .p(px(8.))
                        .rounded(px(4.))
                        .bg(rgb(0x0C0D0E))
                        .font_family(PLEX_MONO)
                        .text_size(sp(10.))
                        .text_color(rgb(TEXT_TERTIARY))
                        .children(log_lines.iter().map(|l| div().child(l.clone()))),
                )
            })
            .when_some(s.result.clone(), |d, r| {
                let (text, color) = match r {
                    Ok(m) => (m, TEXT_SECONDARY),
                    Err(e) => (e, 0xE56060),
                };
                d.child(div().text_size(sp(11.5)).text_color(rgb(color)).child(text))
            })
            .when(!s.failures.is_empty(), |d| {
                d.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .text_size(sp(10.5))
                        .text_color(rgb(WARNING))
                        .children(s.failures.iter().take(6).map(|(n, e)| div().child(format!("{n}: {e}")))),
                )
            })
            .when_some(s.url.clone(), |d, url| {
                let open = url.clone();
                let copy = url.clone();
                d.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            div()
                                .id("gal-url")
                                .text_size(sp(12.))
                                .text_color(rgb(accent_line()))
                                .hover(|d| d.underline())
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if let Err(e) = laika_core::platform::open_url(&open) {
                                        this.status_note = e.to_string();
                                        cx.notify();
                                    }
                                }))
                                .child(url.clone()),
                        )
                        .child(
                            div()
                                .id("gal-url-copy")
                                .text_size(sp(10.5))
                                .text_color(rgb(TEXT_DIM))
                                .hover(|d| d.text_color(rgb(TEXT_PRIMARY)))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(copy.clone()));
                                    this.status_note = "URL copied".to_string();
                                    cx.notify();
                                }))
                                .child("Copy"),
                        ),
                )
            })
            .child(
                div()
                    .pt(px(6.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(div().flex_1())
                    .when(!log_lines.is_empty(), |d| {
                        let text = s.log.lock().map(|l| l.join("\n")).unwrap_or_default();
                        d.child(
                            div()
                                .id("gal-log-copy")
                                .text_size(sp(11.))
                                .text_color(rgb(TEXT_DIM))
                                .hover(|d| d.text_color(rgb(TEXT_PRIMARY)))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(text.clone()));
                                    this.status_note = "deploy log copied".to_string();
                                    cx.notify();
                                }))
                                .child("Copy log"),
                        )
                    })
                    .child(if busy && building.is_some() {
                        div()
                            .id("gal-build-cancel")
                            .on_click(cx.listener(|this, _, _, cx| {
                                if let Some(b) = this.gal.build.as_ref() {
                                    b.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                                }
                                cx.notify();
                            }))
                            .child(button::outline("Cancel build"))
                    } else {
                        div()
                            .id("gal-sheet-done")
                            .on_click(cx.listener(|this, _, _, cx| this.close_publish_sheet(cx)))
                            .child(button::outline("Close"))
                    })
                    .child(
                        div()
                            .id("gal-build")
                            .when(busy, |d| d.opacity(0.5))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                if !busy {
                                    this.publish_build(false, cx);
                                }
                            }))
                            .child(if s.dest == Destination::Folder {
                                button::primary("Build")
                            } else {
                                button::outline("Build only")
                            }),
                    )
                    .when(s.dest == Destination::Cloudflare, |d| {
                        let ready = s.wrangler.is_some() && !g.deploy_project.trim().is_empty();
                        d.child(
                            div()
                                .id("gal-build-deploy")
                                .when(busy || !ready, |d| d.opacity(0.5))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    if !busy {
                                        this.publish_build(true, cx);
                                    }
                                }))
                                .child(button::primary("Build & deploy")),
                        )
                    }),
            );
        modal::modal_shell_w(content, 620.)
    }

    fn choose_gallery_folder(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Build Here".into()),
        });
        cx.spawn(async move |entity, cx| {
            let Ok(Ok(Some(paths))) = rx.await else {
                return;
            };
            let Some(parent) = paths.into_iter().next() else {
                return;
            };
            entity
                .update(cx, |this, cx| {
                    let slug = this
                        .gal
                        .current
                        .as_ref()
                        .map(|g| {
                            if g.slug.is_empty() {
                                "gallery".to_string()
                            } else {
                                g.slug.clone()
                            }
                        })
                        .unwrap_or_default();
                    // A chosen folder that already holds a build is reused;
                    // otherwise the site goes in a subfolder named for it.
                    let dir = if parent.join(site::manifest::MANIFEST_FILE).is_file() {
                        parent
                    } else {
                        parent.join(slug)
                    };
                    let d = dir.to_string_lossy().to_string();
                    this.gal_edit("Output folder", None, |g| g.output_dir = d, cx);
                    this.refresh_publish_diff();
                    cx.notify();
                })
                .ok();
        })
        .detach();
    }
}

fn unix_now() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::deploy_url;

    #[test]
    fn deploy_url_prefers_pages_dev() {
        let lines = vec![
            "Uploading... (12/12)".to_string(),
            "See https://developers.cloudflare.com/pages for help".to_string(),
            "✨ Deployment complete! Take a peek over at https://1a2b3c.my-photos.pages.dev"
                .to_string(),
        ];
        assert_eq!(
            deploy_url(&lines).as_deref(),
            Some("https://1a2b3c.my-photos.pages.dev")
        );
        assert_eq!(deploy_url(&["no url".to_string()]), None);
    }
}
