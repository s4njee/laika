//! V31: the Preferences window — General, File Handling, Interface,
//! External Editing, Performance, Appearance — and Reset to Defaults.
//! App-wide values live in `library.json` (app support directory); rows
//! that belong to the open catalog say so.

use laika_core::catalog::StartupMode;
use laika_core::prefs::{AppPrefs, EditorFormat, FilmstripSize, GpuPreference, SidecarPolicy};

use super::*;

const TABS: [&str; 6] = [
    "General",
    "File Handling",
    "Interface",
    "External Editing",
    "Performance",
    "Appearance",
];

fn row(label: &'static str) -> Div {
    div().flex().items_center().gap(px(8.)).child(
        div()
            .w(px(150.))
            .flex_none()
            .text_size(sp(12.))
            .text_color(rgb(TEXT_DIM))
            .child(label),
    )
}

fn heading(label: &'static str) -> Div {
    div()
        .pt(px(4.))
        .text_size(sp(11.))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(rgb(TEXT_MUTED))
        .child(label)
}

fn note(text: impl Into<SharedString>) -> Div {
    div()
        .text_size(sp(11.5))
        .text_color(rgb(TEXT_DIM))
        .child(text.into())
}

fn value(text: impl Into<SharedString>) -> Div {
    div()
        .text_size(sp(12.))
        .text_color(rgb(TEXT_SECONDARY))
        .child(text.into())
}

impl Laika {
    /// Save app preferences and apply the runtime ones immediately.
    pub(crate) fn set_prefs(&mut self, f: impl FnOnce(&mut AppPrefs), cx: &mut Context<Self>) {
        let mut prefs = self.library.prefs.clone();
        f(&mut prefs);
        let base = self.app_base.clone();
        if let Err(e) = self.library.set_prefs(prefs, &base) {
            self.status_note = format!("preferences not saved: {e}");
        }
        self.apply_runtime_prefs();
        cx.notify();
    }

    pub(crate) fn apply_runtime_prefs(&mut self) {
        let p = &self.library.prefs;
        theme::layout::set_filmstrip_scale(p.filmstrip.scale());
        theme::set_text_scale(p.text_scale_percent);
        self.left_rail_width = p.left_rail_width as f32;
        self.right_rail_width = p.right_rail_width as f32;
        IMPORT_WORKERS_PREF.store(
            p.import_workers as usize,
            std::sync::atomic::Ordering::Relaxed,
        );
    }

    fn chip_choice(
        &self,
        id: (&'static str, usize),
        label: impl Into<SharedString>,
        on: bool,
        f: impl Fn(&mut Laika, &mut Context<Laika>) + 'static,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let label: SharedString = label.into();
        div()
            .id(id)
            .role(Role::Button)
            .aria_label(label.clone())
            .aria_toggled(if on { Toggled::True } else { Toggled::False })
            .on_click(cx.listener(move |this, _, _, cx| f(this, cx)))
            .child(chip::filter_chip(&label, on))
    }

    fn stepper(
        &self,
        id: &'static str,
        shown: String,
        f: impl Fn(&mut Laika, i32, &mut Context<Laika>) + 'static + Clone,
        cx: &mut Context<Self>,
    ) -> Div {
        let down = f.clone();
        div()
            .flex()
            .items_center()
            .gap(px(6.))
            .child(self.chip_choice((id, 0), "−", false, move |this, cx| down(this, -1, cx), cx))
            .child(value(shown).min_w(px(90.)))
            .child(self.chip_choice((id, 1), "+", false, move |this, cx| f(this, 1, cx), cx))
    }

    pub(crate) fn prefs_tabs(&self, cx: &mut Context<Self>) -> Div {
        let mut tabs = div()
            .flex()
            .flex_wrap()
            .gap(px(4.))
            .pb(px(4.))
            .border_b_1()
            .border_color(hairline());
        for (i, name) in TABS.iter().enumerate() {
            let on = self.prefs_tab as usize == i;
            tabs = tabs.child(
                div()
                    .id(("prefs-tab", i))
                    .role(Role::Tab)
                    .aria_label(*name)
                    .aria_selected(on)
                    .px(px(10.))
                    .py(px(5.))
                    .rounded(px(4.))
                    .text_size(sp(12.))
                    .text_color(rgb(if on { TEXT_PRIMARY } else { TEXT_DIM }))
                    .when(on, |d| d.bg(rgb(bg_segment_active())))
                    .hover(|s| s.bg(rgb(bg_row_hover())))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.prefs_tab = i as u8;
                        cx.notify();
                    }))
                    .child(*name),
            );
        }
        tabs
    }

    pub(crate) fn prefs_general(&self, cx: &mut Context<Self>) -> Div {
        let startup = self.library.startup;
        let fixed = self.library.fixed.clone();
        let mode = |this: &mut Laika, m: StartupMode, cx: &mut Context<Laika>| {
            let base = this.app_base.clone();
            let mut fixed = this.library.fixed.clone();
            if m == StartupMode::Fixed && fixed.is_empty() {
                // Fixed with nothing chosen falls back to the current file.
                fixed = this
                    .library
                    .ordered()
                    .into_iter()
                    .next()
                    .unwrap_or_default();
            }
            this.library.set_startup(m, &fixed, &base);
            cx.notify();
        };
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(heading("Launch"))
            .child(
                row("Catalog at launch")
                    .child(self.chip_choice(
                        ("pref-startup", 0),
                        "Last opened",
                        startup == StartupMode::Last,
                        move |this, cx| mode(this, StartupMode::Last, cx),
                        cx,
                    ))
                    .child(self.chip_choice(
                        ("pref-startup", 1),
                        "Ask",
                        startup == StartupMode::Ask,
                        move |this, cx| mode(this, StartupMode::Ask, cx),
                        cx,
                    ))
                    .child(self.chip_choice(
                        ("pref-startup", 2),
                        "Always this one",
                        startup == StartupMode::Fixed,
                        move |this, cx| mode(this, StartupMode::Fixed, cx),
                        cx,
                    )),
            )
            .when(startup == StartupMode::Fixed, |d| {
                d.child(
                    row("Fixed catalog")
                        .child(value(if fixed.is_empty() { "—".to_string() } else { fixed }).truncate().min_w_0())
                        .child(self.chip_choice(
                            ("pref-fixed", 0),
                            "Choose…",
                            false,
                            |this, cx| this.open_fixed_picker(cx),
                            cx,
                        )),
                )
            })
            .child(heading("Language"))
            .child(
                row("Language")
                    .child(self.chip_choice(("pref-lang", 0), "English", true, |_, _| {}, cx)),
            )
            .child(note(
                "English is the only language in this build; other languages will appear here when translated.",
            ))
            .child(heading("Diagnostics"))
            .child(
                div()
                    .id("pref-crash")
                    .on_click(cx.listener(|this, _, _, cx| {
                        let on = !this.library.prefs.crash_reports;
                        this.set_crash_reports(on, cx);
                    }))
                    .child(toggle::toggle(
                        self.library.prefs.crash_reports,
                        "Save a crash report if Laika quits unexpectedly",
                    )),
            )
            .child(note(
                "Logs and crash reports stay on this Mac and never contain passwords or keys. Nothing is sent anywhere.",
            ))
            .child(
                row("Log")
                    .child(self.chip_choice(("pref-log", 0), "Show Log", false, |this, cx| this.show_log(cx), cx))
                    .child(self.chip_choice(("pref-log", 1), "About Laika…", false, |this, cx| this.open_about(cx), cx)),
            )
            .child(heading("Updates"))
            .child(row("Version").child(value(format!("Laika {}", env!("CARGO_PKG_VERSION")))))
            .child(note(
                "This build has no update service to check. Rebuilding from source is how you update for now.",
            ))
    }

    pub(crate) fn prefs_file_handling(&self, cx: &mut Context<Self>) -> Div {
        let p = self.library.prefs.clone();
        let (cap, ttl) = self.cache_policy();
        let catalog_grouping = self
            .catalog
            .as_ref()
            .map(|c| c.get_import_default("pair_grouped"))
            .unwrap_or_default();
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(heading("Preview cache"))
            .child(
                row("Location")
                    .child(value(self.cache_dir.to_string_lossy().to_string()).truncate().min_w_0())
                    .child(self.chip_choice(("pref-cache", 0), "Move…", false, |this, cx| {
                        this.open_folder_picker(PickerTarget::CacheDir, "Preview cache folder", cx);
                    }, cx)),
            )
            .child(row("Size cap").child(self.stepper(
                "pref-cache-cap",
                format!("{:.2} GB", cap as f32 / 1024.),
                |this, dir, cx| {
                    this.bump_cache_policy(if dir < 0 { "cache-cap-down" } else { "cache-cap-up" }, cx)
                },
                cx,
            )))
            .child(row("Keep 1:1 previews").child(self.stepper(
                "pref-cache-ttl",
                if ttl == 0 {
                    "until over cap".to_string()
                } else {
                    format!("{ttl} days")
                },
                |this, dir, cx| {
                    this.bump_cache_policy(if dir < 0 { "cache-ttl-down" } else { "cache-ttl-up" }, cx)
                },
                cx,
            )))
            .child(heading("Import defaults"))
            .child(
                row("Files")
                    .child(self.chip_choice(("pref-import-copy", 0), "Add in place", !p.import_copy, |this, cx| {
                        this.set_prefs(|p| p.import_copy = false, cx)
                    }, cx))
                    .child(self.chip_choice(("pref-import-copy", 1), "Copy to a destination", p.import_copy, |this, cx| {
                        this.set_prefs(|p| p.import_copy = true, cx)
                    }, cx)),
            )
            .child(
                row("Options")
                    .child(self.chip_choice(("pref-import-opt", 0), "Skip duplicates", p.import_skip_duplicates, |this, cx| {
                        this.set_prefs(|p| p.import_skip_duplicates = !p.import_skip_duplicates, cx)
                    }, cx))
                    .child(self.chip_choice(("pref-import-opt", 1), "New photos only", p.import_new_only, |this, cx| {
                        this.set_prefs(|p| p.import_new_only = !p.import_new_only, cx)
                    }, cx))
                    .child(self.chip_choice(("pref-import-opt", 2), "Eject card after", p.import_eject, |this, cx| {
                        this.set_prefs(|p| p.import_eject = !p.import_eject, cx)
                    }, cx)),
            )
            .child(note("The import dialog starts from these; you can still change them per import."))
            .child(heading("RAW + JPEG"))
            .child(
                row("New catalogs")
                    .child(self.chip_choice(("pref-pairs", 0), "Group pairs into one cell", p.group_pairs_default, |this, cx| {
                        this.set_prefs(|p| p.group_pairs_default = true, cx)
                    }, cx))
                    .child(self.chip_choice(("pref-pairs", 1), "Show both files", !p.group_pairs_default, |this, cx| {
                        this.set_prefs(|p| p.group_pairs_default = false, cx)
                    }, cx)),
            )
            .child(note(if catalog_grouping.is_empty() {
                "This catalog follows this default.".to_string()
            } else {
                format!(
                    "This catalog has its own choice ({}) — change it with Group RAW+JPEG in the left rail.",
                    if catalog_grouping == "0" { "both files" } else { "grouped" }
                )
            }))
            .child(heading("Sidecars"))
            .child(
                row("XMP sidecars")
                    .child(self.chip_choice(("pref-sidecar", 0), "Write next to originals", p.sidecars == SidecarPolicy::Always, |this, cx| {
                        this.set_prefs(|p| p.sidecars = SidecarPolicy::Always, cx)
                    }, cx))
                    .child(self.chip_choice(("pref-sidecar", 1), "Catalog only", p.sidecars == SidecarPolicy::Never, |this, cx| {
                        this.set_prefs(|p| p.sidecars = SidecarPolicy::Never, cx)
                    }, cx)),
            )
            .child(note(
                "Catalog only never writes .xmp files, but edits other apps make to existing sidecars are still read. Photos inside an Apple Photos library never get sidecars.",
            ))
            .child(self.coexist_prefs(cx))
    }

    pub(crate) fn prefs_interface(&self, cx: &mut Context<Self>) -> Div {
        let p = self.library.prefs.clone();
        let badges = laika_core::state::BadgeSet::parse(&p.default_badges);
        let badge_names: [(&'static str, bool); 9] = [
            ("flag", badges.flag),
            ("rating", badges.rating),
            ("label", badges.label),
            ("crop", badges.crop),
            ("edit", badges.edit),
            ("keywords", badges.keywords),
            ("pair", badges.pair),
            ("video", badges.video),
            ("sync", badges.sync),
        ];
        let mut badge_row = row("Badges").flex_wrap();
        for (i, (name, on)) in badge_names.into_iter().enumerate() {
            badge_row = badge_row.child(self.chip_choice(
                ("pref-badge", i),
                name,
                on,
                move |this, cx| {
                    this.set_prefs(
                        |p| {
                            let mut set = laika_core::state::BadgeSet::parse(&p.default_badges);
                            let slot = match name {
                                "flag" => &mut set.flag,
                                "rating" => &mut set.rating,
                                "label" => &mut set.label,
                                "crop" => &mut set.crop,
                                "edit" => &mut set.edit,
                                "keywords" => &mut set.keywords,
                                "pair" => &mut set.pair,
                                "video" => &mut set.video,
                                _ => &mut set.sync,
                            };
                            *slot = !*slot;
                            p.default_badges = if set == laika_core::state::BadgeSet::all_on() {
                                String::new()
                            } else if set.serialize().is_empty() {
                                "none".to_string()
                            } else {
                                set.serialize()
                            };
                        },
                        cx,
                    )
                },
                cx,
            ));
        }
        let mut film = row("Filmstrip size");
        for (i, size) in [
            FilmstripSize::Small,
            FilmstripSize::Medium,
            FilmstripSize::Large,
        ]
        .into_iter()
        .enumerate()
        {
            film = film.child(self.chip_choice(
                ("pref-film", i),
                size.label(),
                p.filmstrip == size,
                move |this, cx| this.set_prefs(|p| p.filmstrip = size, cx),
                cx,
            ));
        }
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(heading("Keyboard"))
            .child(
                row("Shortcuts")
                    .child(self.chip_choice(("pref-keys", 0), "Laika", !p.lightroom_keys, |this, cx| {
                        this.set_lightroom_keys(false, cx)
                    }, cx))
                    .child(self.chip_choice(("pref-keys", 1), "Lightroom Classic", p.lightroom_keys, |this, cx| {
                        this.set_lightroom_keys(true, cx)
                    }, cx)),
            )
            .child(note(
                "Lightroom Classic: W white balance, R crop, ⌥⌘1–3 modules, ⇧⌘C/V copy and paste settings, ⌘R show in Finder; Q, K and M explain what Laika doesn't have yet. ⇧⌘P finds any command by its Laika or Lightroom name.",
            ))
            .child(heading("Thumbnail defaults"))
            .child(note(
                "Used by catalogs that haven't chosen their own; the left rail's Display section sets the open catalog.",
            ))
            .child(row("Columns").child(self.stepper(
                "pref-columns",
                p.default_columns.to_string(),
                |this, dir, cx| {
                    this.set_prefs(
                        |p| p.default_columns = (p.default_columns as i32 + dir).clamp(3, 20) as u8,
                        cx,
                    )
                },
                cx,
            )))
            .child(
                row("Cell style")
                    .child(self.chip_choice(("pref-style", 0), "Expanded", p.default_cell_style != "compact", |this, cx| {
                        this.set_prefs(|p| p.default_cell_style = "expanded".to_string(), cx)
                    }, cx))
                    .child(self.chip_choice(("pref-style", 1), "Compact", p.default_cell_style == "compact", |this, cx| {
                        this.set_prefs(|p| p.default_cell_style = "compact".to_string(), cx)
                    }, cx)),
            )
            .child(
                row("Overlay")
                    .child(self.chip_choice(("pref-overlay", 0), "None", p.default_overlay == "none", |this, cx| {
                        this.set_prefs(|p| p.default_overlay = "none".to_string(), cx)
                    }, cx))
                    .child(self.chip_choice(("pref-overlay", 1), "Capture time", p.default_overlay == "file", |this, cx| {
                        this.set_prefs(|p| p.default_overlay = "file".to_string(), cx)
                    }, cx))
                    .child(self.chip_choice(("pref-overlay", 2), "Camera + exposure", p.default_overlay == "exif", |this, cx| {
                        this.set_prefs(|p| p.default_overlay = "exif".to_string(), cx)
                    }, cx)),
            )
            .child(badge_row)
            .child(heading("Wall and filmstrip"))
            .child(
                row("Workspace panels")
                    .child(self.chip_choice(
                        ("pref-panel", 0),
                        "Left",
                        p.left_rail_visible,
                        |this, cx| this.set_prefs(|p| p.left_rail_visible = !p.left_rail_visible, cx),
                        cx,
                    ))
                    .child(self.chip_choice(
                        ("pref-panel", 1),
                        "Right",
                        p.right_rail_visible,
                        |this, cx| this.set_prefs(|p| p.right_rail_visible = !p.right_rail_visible, cx),
                        cx,
                    ))
                    .child(self.chip_choice(
                        ("pref-panel", 2),
                        "Filmstrip",
                        p.filmstrip_visible,
                        |this, cx| this.set_prefs(|p| p.filmstrip_visible = !p.filmstrip_visible, cx),
                        cx,
                    )),
            )
            .child(
                row("Left panel width").child(self.stepper(
                    "pref-left-width",
                    format!("{} px", p.left_rail_width),
                    |this, dir, cx| {
                        this.set_prefs(
                            |p| {
                                p.left_rail_width =
                                    (p.left_rail_width as i32 + dir * 10).clamp(168, 420) as u16
                            },
                            cx,
                        )
                    },
                    cx,
                )),
            )
            .child(
                row("Right panel width").child(self.stepper(
                    "pref-right-width",
                    format!("{} px", p.right_rail_width),
                    |this, dir, cx| {
                        this.set_prefs(
                            |p| {
                                p.right_rail_width =
                                    (p.right_rail_width as i32 + dir * 10).clamp(220, 460) as u16
                            },
                            cx,
                        )
                    },
                    cx,
                )),
            )
            .child(
                row("Text size").child(self.stepper(
                    "pref-text-scale",
                    format!("{}%", p.text_scale_percent),
                    |this, dir, cx| {
                        this.set_prefs(
                            |p| {
                                p.text_scale_percent =
                                    (p.text_scale_percent as i32 + dir * 5).clamp(85, 150) as u8
                            },
                            cx,
                        )
                    },
                    cx,
                )),
            )
            .child(note(
                "Drag either panel divider to resize. These controls provide the same changes without dragging; panel visibility, widths, filmstrip, and text size persist across restarts.",
            ))
            .child(
                row("Entering the wall")
                    .child(self.chip_choice(("pref-wall", 0), "Also hide panels", p.wall_hides_chrome, |this, cx| {
                        this.set_prefs(|p| p.wall_hides_chrome = !p.wall_hides_chrome, cx)
                    }, cx)),
            )
            .child(film)
    }

    pub(crate) fn prefs_external(&self, cx: &mut Context<Self>) -> Div {
        let p = self.library.prefs.clone();
        let app = if p.editor_app.is_empty() {
            "none chosen".to_string()
        } else {
            p.editor_app.clone()
        };
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(
                row("Application")
                    .child(value(app).truncate().min_w_0())
                    .child(self.chip_choice(("pref-editor", 0), "Choose…", false, |this, cx| this.choose_editor_app(cx), cx))
                    .when(!p.editor_app.is_empty(), |d| {
                        d.child(self.chip_choice(("pref-editor", 1), "Clear", false, |this, cx| {
                            this.set_prefs(|p| p.editor_app.clear(), cx)
                        }, cx))
                    }),
            )
            .child(
                row("File format")
                    .child(self.chip_choice(("pref-editor-format", 0), "TIFF", p.editor_format == EditorFormat::Tiff, |this, cx| {
                        this.set_prefs(|p| p.editor_format = EditorFormat::Tiff, cx)
                    }, cx))
                    .child(self.chip_choice(("pref-editor-format", 1), "JPEG", p.editor_format == EditorFormat::Jpeg, |this, cx| {
                        this.set_prefs(|p| p.editor_format = EditorFormat::Jpeg, cx)
                    }, cx)),
            )
            .child(row("Color space").child(value("sRGB")))
            .child(row("Bit depth").child(value("8 bits per channel")))
            .child(note(
                "The renderer writes 8-bit sRGB today; wider color and 16-bit output will be offered here once export supports them.",
            ))
            .child(self.backup_field(
                text_input::FieldId::EditorNaming,
                "File name",
                &p.editor_naming,
                "{original}-edit",
                cx,
            ))
            .child(note(
                "Edit in External Editor (⌘E, File menu) renders the selection with its develop settings next to each original under this name, then opens the files in the application. Tokens: {original} {date} {seq}.",
            ))
    }

    pub(crate) fn choose_editor_app(&mut self, cx: &mut Context<Self>) {
        let rx = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: true,
            multiple: false,
            prompt: Some("Use as external editor".into()),
        });
        cx.spawn(async move |entity, cx| {
            let path = rx
                .await
                .ok()
                .and_then(|r| r.ok())
                .flatten()
                .and_then(|p| p.into_iter().next());
            if let Some(path) = path {
                entity
                    .update(cx, |this, cx| {
                        let s = path.to_string_lossy().to_string();
                        this.set_prefs(|p| p.editor_app = s, cx);
                    })
                    .ok();
            }
        })
        .detach();
    }

    pub(crate) fn prefs_performance(&self, cx: &mut Context<Self>) -> Div {
        let p = self.library.prefs.clone();
        let cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);
        let env_workers = std::env::var("LAIKA_IMPORT_WORKERS").is_ok();
        div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .child(heading("Graphics"))
            .child(
                row("GPU")
                    .child(self.chip_choice(("pref-gpu", 0), "High performance", p.gpu == GpuPreference::HighPerformance, |this, cx| {
                        this.set_prefs(|p| p.gpu = GpuPreference::HighPerformance, cx)
                    }, cx))
                    .child(self.chip_choice(("pref-gpu", 1), "Low power", p.gpu == GpuPreference::LowPower, |this, cx| {
                        this.set_prefs(|p| p.gpu = GpuPreference::LowPower, cx)
                    }, cx)),
            )
            .child(row("In use").child(value(if self.gpu_adapter.is_empty() {
                "renderer not started yet".to_string()
            } else {
                self.gpu_adapter.clone()
            })))
            .child(note(
                "Takes effect the next time the renderer starts (restart Laika). Machines with a single GPU use it either way.",
            ))
            .child(heading("Threads"))
            .child(row("Import workers").child(self.stepper(
                "pref-workers",
                if p.import_workers == 0 {
                    format!("Auto ({})", p.import_worker_count(cores))
                } else {
                    p.import_workers.to_string()
                },
                |this, dir, cx| {
                    this.set_prefs(
                        |p| p.import_workers = (p.import_workers as i32 + dir).clamp(0, 32) as u8,
                        cx,
                    )
                },
                cx,
            )))
            .when(env_workers, |d| {
                d.child(note("LAIKA_IMPORT_WORKERS is set in the environment and overrides this."))
            })
            .child(row("Thumbnails at once").child(self.stepper(
                "pref-thumbs",
                p.thumb_concurrency.to_string(),
                |this, dir, cx| {
                    this.set_prefs(
                        |p| p.thumb_concurrency = (p.thumb_concurrency as i32 + dir).clamp(1, 32) as u8,
                        cx,
                    )
                },
                cx,
            )))
            .child(note(format!(
                "Import workers decode and build previews in parallel ({cores} cores here). Thumbnails at once is how many grid previews load together."
            )))
    }

    pub(crate) fn prefs_footer(&self, cx: &mut Context<Self>) -> Div {
        div()
            .flex()
            .items_center()
            .justify_between()
            .pt(px(6.))
            .border_t_1()
            .border_color(hairline())
            .child(note("Saved in Laika's app support folder, not in the catalog."))
            .child(
                div()
                    .id("prefs-reset")
                    .px(px(10.))
                    .py(px(5.))
                    .rounded(px(4.))
                    .border_1()
                    .border_color(border_control())
                    .text_size(sp(11.5))
                    .text_color(rgb(TEXT_SECONDARY))
                    .hover(|s| s.bg(rgb(bg_row_hover())))
                    .on_hover(self.tip(
                        "Preferences, appearance, and cache limits return to defaults (catalogs and recent files stay)",
                    ))
                    .on_click(cx.listener(|this, _, _, cx| {
                        let base = this.app_base.clone();
                        match this.library.reset_prefs(&base) {
                            Ok(()) => {
                                this.status_note = "preferences reset to defaults".to_string();
                            }
                            Err(e) => this.status_note = format!("reset not saved: {e}"),
                        }
                        theme::set_appearance(theme::Appearance::Grey);
                        theme::set_accent(theme::ACCENT_PRESETS[0].1);
                        this.apply_runtime_prefs();
                        cx.refresh_windows();
                        cx.notify();
                    }))
                    .child("Reset to Defaults"),
            )
    }
}
