//! V32: About window, diagnostics text, Show Log, crash-report opt-in,
//! and the first-run welcome screen.

use std::path::PathBuf;

use super::*;

#[derive(Default)]
pub(crate) struct DiagUi {
    pub about_open: bool,
    /// First-run welcome screen showing.
    pub welcome_open: bool,
    /// `sw_vers` result, read once when About opens.
    pub os_version: String,
    pub note: String,
}

/// Where sample RAWs ship: inside the app bundle, else the workspace
/// fixtures (development builds).
pub(crate) fn bundled_samples() -> Option<PathBuf> {
    let has_raw = |d: &PathBuf| {
        std::fs::read_dir(d)
            .map(|r| r.flatten().any(|e| laika_raw::is_raw(&e.path())))
            .unwrap_or(false)
    };
    let exe = std::env::current_exe().ok()?;
    let bundled = exe.parent()?.parent()?.join("Resources").join("samples");
    if has_raw(&bundled) {
        return Some(bundled);
    }
    let dev = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/raw"));
    has_raw(&dev).then_some(dev)
}

fn build_line() -> String {
    format!(
        "Laika {} (build {}, {}, {})",
        env!("CARGO_PKG_VERSION"),
        env!("LAIKA_GIT_REV"),
        env!("LAIKA_BUILD_DATE"),
        env!("LAIKA_PROFILE"),
    )
}

fn human_bytes(b: u64) -> String {
    if b >= 1 << 30 {
        format!("{:.1} GB", b as f64 / (1u64 << 30) as f64)
    } else if b >= 1 << 20 {
        format!("{:.1} MB", b as f64 / (1u64 << 20) as f64)
    } else {
        format!("{} KB", (b / 1024).max(1))
    }
}

impl Laika {
    /// Plain-text diagnostics (About → Copy, crash reports). Never holds
    /// credentials: paths, versions, counts, and the GPU name only.
    pub(crate) fn diagnostics_text(&self) -> String {
        let mut lines = vec![build_line()];
        let os = if self.diag.os_version.is_empty() {
            format!("{} {}", std::env::consts::OS, std::env::consts::ARCH)
        } else {
            format!(
                "macOS {} ({})",
                self.diag.os_version,
                std::env::consts::ARCH
            )
        };
        lines.push(format!("OS: {os}"));
        lines.push(format!(
            "GPU: {}",
            if self.gpu_adapter.is_empty() {
                "unavailable".to_string()
            } else {
                self.gpu_adapter.clone()
            }
        ));
        match self.catalog.as_ref() {
            Some(cat) => {
                let db = cat.db_path();
                // SQLite WAL mode keeps recent writes beside the main file.
                let size: u64 = ["", "-wal", "-shm"]
                    .iter()
                    .filter_map(|ext| {
                        let mut p = db.as_os_str().to_owned();
                        p.push(ext);
                        std::fs::metadata(PathBuf::from(p)).ok()
                    })
                    .map(|m| m.len())
                    .sum();
                lines.push(format!(
                    "Catalog: {} ({}, {} photos)",
                    db.display(),
                    human_bytes(size),
                    self.photos.len()
                ));
                lines.push(format!("Catalog root: {}", cat.root_path()));
            }
            None => lines.push("Catalog: none open".to_string()),
        }
        lines.push(format!("Cache: {}", self.cache_dir.display()));
        lines.push(format!(
            "Components: rawler {} · wgpu {} · gpui {}",
            env!("LAIKA_RAWLER_VERSION"),
            env!("LAIKA_WGPU_VERSION"),
            env!("LAIKA_GPUI_VERSION")
        ));
        lines.push(format!(
            "Log: {}",
            laika_core::logging::log_path()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "not enabled".to_string())
        ));
        lines.push(format!(
            "Crash reports: {}",
            if self.library.prefs.crash_reports {
                "on"
            } else {
                "off"
            }
        ));
        lines.join("\n")
    }

    /// Keep the crash-report context current (cheap; called on open/switch).
    pub(crate) fn refresh_crash_context(&self) {
        laika_core::logging::set_crash_context(&self.diagnostics_text());
    }

    pub(crate) fn open_about(&mut self, cx: &mut Context<Self>) {
        self.close_modals(cx);
        if self.diag.os_version.is_empty() {
            self.diag.os_version = std::process::Command::new("sw_vers")
                .arg("-productVersion")
                .output()
                .ok()
                .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
                .unwrap_or_default();
        }
        // The GPU line names the adapter; starting the renderer is quick.
        self.ensure_dev(cx);
        self.diag.note.clear();
        self.diag.about_open = true;
        self.refresh_crash_context();
        cx.notify();
    }

    /// Reveal the live log in Finder.
    pub(crate) fn show_log(&mut self, cx: &mut Context<Self>) {
        match laika_core::logging::log_path() {
            Some(p) => {
                // The file exists once the first line is written at launch.
                if let Err(e) = laika_core::import::reveal_in_manager(&p) {
                    self.status_note = format!("couldn't show the log: {e}");
                } else {
                    self.status_note = format!("log: {}", p.display());
                }
            }
            None => self.status_note = "logging is off for this launch (LAIKA_LOG=0)".to_string(),
        }
        self.diag.note = self.status_note.clone();
        cx.notify();
    }

    pub(crate) fn set_crash_reports(&mut self, on: bool, cx: &mut Context<Self>) {
        laika_core::logging::set_crash_reports(on);
        self.set_prefs(move |p| p.crash_reports = on, cx);
        self.refresh_crash_context();
    }

    pub(crate) fn about_modal(&self, cx: &mut Context<Self>) -> Div {
        let rows: Vec<(String, String)> = self
            .diagnostics_text()
            .lines()
            .skip(1)
            .filter_map(|l| {
                l.split_once(": ")
                    .map(|(k, v)| (k.to_string(), v.to_string()))
            })
            .collect();
        let crash = self.library.prefs.crash_reports;
        let reports = laika_core::logging::crash_reports();
        let button = |id: &'static str, label: &str| {
            div()
                .id(id)
                .px(px(10.))
                .py(px(6.))
                .rounded(px(4.))
                .border_1()
                .border_color(border_control())
                .text_size(px(11.))
                .text_color(rgb(TEXT_SECONDARY))
                .hover(|s| s.bg(rgb(bg_row_hover())))
                .child(label.to_string())
        };
        let content = div()
            .p(px(24.))
            .flex()
            .flex_col()
            .gap(px(14.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(div().size(px(12.)).rounded_full().bg(rgb(accent_line())))
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_size(px(20.))
                            .text_color(rgb(TEXT_PRIMARY))
                            .child("LAIKA"),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .id("about-close")
                            .text_size(px(15.))
                            .text_color(rgb(TEXT_DIM))
                            .hover(|d| d.text_color(rgb(TEXT_PRIMARY)))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.diag.about_open = false;
                                cx.notify();
                            }))
                            .child("✕"),
                    ),
            )
            .child(
                div()
                    .text_size(px(12.5))
                    .text_color(rgb(TEXT_SECONDARY))
                    .child(build_line()),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(5.))
                    .p(px(12.))
                    .rounded(px(5.))
                    .bg(rgb(bg_well()))
                    .children(rows.into_iter().map(|(k, v)| {
                        div()
                            .flex()
                            .gap(px(10.))
                            .child(
                                div()
                                    .w(px(96.))
                                    .flex_none()
                                    .text_size(px(11.))
                                    .text_color(rgb(TEXT_DIM))
                                    .child(k),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .text_size(px(11.))
                                    .text_color(rgb(TEXT_SECONDARY))
                                    .child(v),
                            )
                    })),
            )
            .child(
                div()
                    .id("about-crash")
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .on_click(cx.listener(move |this, _, _, cx| this.set_crash_reports(!crash, cx)))
                    .child(toggle::toggle(crash, "Save a crash report if Laika quits unexpectedly"))
                    .child(
                        div()
                            .pl(px(35.))
                            .text_size(px(10.5))
                            .text_color(rgb(TEXT_DIM))
                            .child("Reports stay on this Mac beside the log. Nothing is sent anywhere; attach one to an issue if you choose."),
                    ),
            )
            .when(!self.diag.note.is_empty(), |d| {
                d.child(
                    div()
                        .text_size(px(10.5))
                        .text_color(rgb(TEXT_TERTIARY))
                        .child(self.diag.note.clone()),
                )
            })
            .child(
                div()
                    .flex()
                    .gap(px(8.))
                    .child(button("about-copy", "Copy Diagnostics").on_click(cx.listener(|this, _, _, cx| {
                        cx.write_to_clipboard(ClipboardItem::new_string(this.diagnostics_text()));
                        this.diag.note = "diagnostics copied — paste them into your issue".to_string();
                        cx.notify();
                    })))
                    .child(button("about-log", "Show Log").on_click(cx.listener(|this, _, _, cx| this.show_log(cx))))
                    .when(!reports.is_empty(), |d| {
                        let newest = reports[0].clone();
                        d.child(
                            button("about-crashes", &format!("Show Crash Reports ({})", reports.len())).on_click(
                                cx.listener(move |this, _, _, cx| {
                                    if let Err(e) = laika_core::import::reveal_in_manager(&newest) {
                                        this.diag.note = e;
                                    }
                                    cx.notify();
                                }),
                            ),
                        )
                    })
                    .child(div().flex_1())
                    .child(
                        div()
                            .id("about-done")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.diag.about_open = false;
                                cx.notify();
                            }))
                            .child(button::outline("Close")),
                    ),
            );
        modal::modal_shell_w(content, 600.)
    }

    // ---- first run ---------------------------------------------------------------

    /// Show the welcome screen on first launch with an empty catalog.
    /// Existing libraries (photos already present) are marked welcomed.
    pub(crate) fn check_first_run(&mut self) {
        if self.library.welcomed {
            return;
        }
        let has_photos = self.catalog.as_ref().is_some_and(|c| c.photo_count() > 0);
        if has_photos {
            self.finish_welcome();
        } else {
            self.diag.welcome_open = true;
        }
    }

    pub(crate) fn finish_welcome(&mut self) {
        self.diag.welcome_open = false;
        let base = self.app_base.clone();
        if let Err(e) = self.library.set_welcomed(&base) {
            eprintln!("[laika] welcome state not saved: {e}");
        }
    }

    /// Copy the bundled sample RAWs next to the catalog and import them.
    pub(crate) fn import_samples(&mut self, cx: &mut Context<Self>) {
        let Some(src) = bundled_samples() else {
            self.diag.note = "sample photos aren't included in this build".to_string();
            cx.notify();
            return;
        };
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        let dest = PathBuf::from(home).join("Pictures").join("Laika Samples");
        if let Err(e) = std::fs::create_dir_all(&dest) {
            self.diag.note = format!("couldn't create {}: {e}", dest.display());
            cx.notify();
            return;
        }
        let mut copied = 0;
        for entry in std::fs::read_dir(&src).into_iter().flatten().flatten() {
            let path = entry.path();
            if !laika_raw::is_raw(&path) {
                continue;
            }
            let Some(name) = path.file_name() else {
                continue;
            };
            let target = dest.join(name);
            if target.exists() || std::fs::copy(&path, &target).is_ok() {
                copied += 1;
            }
        }
        eprintln!("[laika] sample photos: {copied} in {}", dest.display());
        if copied == 0 {
            self.diag.note = "couldn't copy the sample photos".to_string();
            cx.notify();
            return;
        }
        self.finish_welcome();
        self.begin_import(dest, cx);
        cx.notify();
    }

    pub(crate) fn welcome_screen(&self, cx: &mut Context<Self>) -> Div {
        let catalog = self
            .catalog
            .as_ref()
            .map(|c| c.db_path().display().to_string())
            .unwrap_or_else(|| "no catalog is open".to_string());
        let samples = bundled_samples().is_some();
        let point = |title: &str, body: &str| {
            div()
                .flex()
                .flex_col()
                .gap(px(3.))
                .child(
                    div()
                        .text_size(px(12.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(rgb(TEXT_PRIMARY))
                        .child(title.to_string()),
                )
                .child(
                    div()
                        .text_size(px(11.5))
                        .line_height(relative(1.5))
                        .text_color(rgb(TEXT_TERTIARY))
                        .child(body.to_string()),
                )
        };
        let action = |id: &'static str, title: &str, body: &str, primary: bool| {
            div()
                .id(id)
                .flex_1()
                .flex()
                .flex_col()
                .gap(px(4.))
                .p(px(14.))
                .rounded(px(6.))
                .border_1()
                .when(primary, |d| d.border_color(rgb(accent_line())))
                .when(!primary, |d| d.border_color(border_control()))
                .hover(|d| d.bg(rgb(bg_row_hover())))
                .child(
                    div()
                        .text_size(px(12.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(rgb(if primary { accent_line() } else { TEXT_PRIMARY }))
                        .child(title.to_string()),
                )
                .child(
                    div()
                        .text_size(px(11.))
                        .text_color(rgb(TEXT_DIM))
                        .child(body.to_string()),
                )
        };
        let content = div()
            .p(px(30.))
            .flex()
            .flex_col()
            .gap(px(18.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .child(
                        div()
                            .text_size(px(24.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(rgb(TEXT_PRIMARY))
                            .child("Welcome to Laika"),
                    )
                    .child(
                        div()
                            .text_size(px(12.5))
                            .text_color(rgb(TEXT_SECONDARY))
                            .child("A photo catalog and RAW developer that keeps everything on your Mac."),
                    ),
            )
            .child(point(
                "Your originals stay yours",
                "Laika reads your files where they are, or copies them from a card into a folder you choose. It never changes an original: edits are instructions stored in the catalog and in .xmp sidecars beside each photo.",
            ))
            .child(point(
                "One catalog file",
                "Ratings, edits, collections, and galleries live in a single catalog you can move or back up. Backup and Apple Photos sync are optional — set them up later.",
            ))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .p(px(10.))
                    .rounded(px(5.))
                    .bg(rgb(bg_well()))
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(div().text_size(px(10.)).text_color(rgb(TEXT_DIM)).child("CATALOG"))
                            .child(div().text_size(px(11.)).text_color(rgb(TEXT_SECONDARY)).child(catalog)),
                    )
                    .child(
                        div()
                            .id("welcome-new-catalog")
                            .px(px(10.))
                            .py(px(6.))
                            .rounded(px(4.))
                            .border_1()
                            .border_color(border_control())
                            .text_size(px(11.))
                            .text_color(rgb(TEXT_SECONDARY))
                            .hover(|s| s.bg(rgb(bg_row_hover())))
                            .on_click(cx.listener(|this, _, _, cx| {
                                // V07's new-catalog flow: pick a folder, name it.
                                this.diag.welcome_open = false;
                                this.manage_open = true;
                                this.open_folder_picker(PickerTarget::CatalogDir, "New catalog folder", cx);
                                cx.notify();
                            }))
                            .child("Create Catalog Elsewhere…"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .gap(px(10.))
                    .child(
                        action(
                            "welcome-samples",
                            "Try with sample photos",
                            if samples {
                                "Copies three RAW files to Pictures/Laika Samples and imports them"
                            } else {
                                "Not included in this build"
                            },
                            samples,
                        )
                        .when(!samples, |d| d.opacity(0.5))
                        .on_click(cx.listener(|this, _, _, cx| this.import_samples(cx))),
                    )
                    .child(
                        action(
                            "welcome-import",
                            "Import your photos",
                            "From a memory card or a folder",
                            !samples,
                        )
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.finish_welcome();
                            this.open_import_dialog(cx);
                            cx.notify();
                        })),
                    ),
            )
            .when(!self.diag.note.is_empty(), |d| {
                d.child(div().text_size(px(11.)).text_color(rgb(WARNING)).child(self.diag.note.clone()))
            })
            .child(
                div()
                    .flex()
                    .justify_end()
                    .child(
                        div()
                            .id("welcome-skip")
                            .text_size(px(11.5))
                            .text_color(rgb(TEXT_DIM))
                            .hover(|d| d.text_color(rgb(TEXT_PRIMARY)))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.finish_welcome();
                                cx.notify();
                            }))
                            .child("Skip for now"),
                    ),
            );
        modal::modal_shell_w(content, 640.)
    }
}
