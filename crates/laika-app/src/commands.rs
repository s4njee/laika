//! V31: every command in one table — the native macOS menu bar, the
//! in-window menu bar (Linux, or `LAIKA_INWINDOW_MENU=1`), and the keyboard
//! all reach the same `run_command`. Menu titles carry their shortcut so no
//! command exists only as a key.

use super::*;

/// One menu action: which command to run (a single action type keeps
/// every menu item pointing at `run_command`).
#[derive(Clone, PartialEq, Debug, gpui_kit::Action)]
#[action(namespace = laika, no_json)]
pub(crate) struct RunCommand {
    pub id: usize,
}

gpui_kit::actions!(laika, [Quit, OpenPreferences, HideApp, HideOthers]);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Command {
    About,
    Preferences,
    Quit,
    // File
    ImportPhotos,
    ImportLightroom,
    ImportPresets,
    SidecarConflicts,
    Export,
    ExportPrevious,
    ExportEverything,
    EditExternal,
    Publish,
    Rename,
    MoveToFolder,
    RevealInFinder,
    RelinkMissing,
    ManageCatalog,
    NewCatalog,
    OpenCatalog,
    BackUpCatalog,
    BackupSettings,
    SyncNow,
    RetrySync,
    ApplePhotos,
    ApplePhotosSync,
    BuildSmartPreviews,
    BuildOneToOnePreviews,
    // Edit
    Undo,
    Redo,
    SelectAll,
    Deselect,
    CopySettings,
    PasteSettings,
    Find,
    KeywordManager,
    // Photo
    Rating(u8),
    Pick,
    Reject,
    Unflag,
    Label(u8),
    AddToTarget,
    NewCollection,
    NewSmartCollection,
    CreateStack,
    ToggleStack,
    Unstack,
    StackPairs,
    RotateLeft,
    RotateRight,
    FlipHorizontal,
    FlipVertical,
    DeleteRejected,
    AutoAdvance,
    // Develop
    Library,
    Develop,
    WhiteBalancePicker,
    CropTool,
    ApplyCrop,
    CancelCrop,
    ResetCrop,
    ResetGeometry,
    Upright(u8),
    AutoWhiteBalance,
    BeforeAfter,
    // View
    ViewGrid,
    ViewLoupe,
    ViewCompare,
    ViewSurvey,
    ViewTimeline,
    Map,
    ViewWall,
    ZoomCycle,
    ZoomFit,
    Zoom100,
    Zoom200,
    CellOverlay,
    LoupeInfo,
    CropOverlay,
    Slideshow,
    Lights,
    ToggleLeftRail,
    ToggleRightRail,
    ToggleFilmstrip,
    TextLarger,
    TextSmaller,
    TextDefault,
    HideChrome,
    FullScreen,
    // Help
    CommandPalette,
    LightroomGuide,
    Shortcuts,
    ShowLog,
    Welcome,
}

/// A menu entry: a command, a separator, or a submenu.
pub(crate) enum Entry {
    Cmd(Command),
    Sep,
    Sub(&'static str, Vec<Entry>),
}

use Command as C;
use Entry::{Cmd, Sep, Sub};

/// S04: Lightroom Classic keyboard map (app-wide preference).
static LIGHTROOM_KEYS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

pub(crate) fn set_lightroom_keys(on: bool) {
    LIGHTROOM_KEYS.store(on, std::sync::atomic::Ordering::Relaxed);
}

pub(crate) fn lightroom_keys() -> bool {
    LIGHTROOM_KEYS.load(std::sync::atomic::Ordering::Relaxed)
}

impl Command {
    /// Title and shortcut (empty = none).
    pub fn title(self) -> (String, &'static str) {
        let (title, key) = self.base_title();
        if !lightroom_keys() {
            return (title, key);
        }
        // S04: shortcuts that differ under the Lightroom keyboard map.
        let lr = match self {
            C::Library => "⌥⌘1",
            C::Develop => "D",
            C::Map => "⌥⌘3",
            C::WhiteBalancePicker => "W",
            C::CropTool => "R",
            C::ViewWall => "",
            C::RetrySync => "",
            C::CopySettings => "⇧⌘C",
            C::PasteSettings => "⇧⌘V",
            C::Export => "⇧⌘E",
            C::ImportPhotos => "⇧⌘I",
            C::RevealInFinder => "⌘R",
            C::KeywordManager => "⌘K",
            _ => key,
        };
        (title, lr)
    }

    fn base_title(self) -> (String, &'static str) {
        let s = |t: &str| t.to_string();
        match self {
            C::Library => (s("Library"), ""),
            C::WhiteBalancePicker => (s("White Balance Picker"), ""),
            C::CommandPalette => (s("Find a Command…"), "⇧⌘P"),
            C::LightroomGuide => (s("Laika for Lightroom Users"), ""),
            C::About => (s("About Laika"), ""),
            C::Preferences => (s("Preferences…"), "⌘,"),
            C::Quit => (s("Quit Laika"), "⌘Q"),
            C::ImportPhotos => (s("Import Photos…"), ""),
            C::ImportLightroom => (s("Import from Lightroom…"), ""),
            C::ImportPresets => (s("Import Presets…"), ""),
            C::SidecarConflicts => (s("Review Sidecar Conflicts…"), ""),
            C::Export => (s("Export…"), ""),
            C::ExportPrevious => (s("Export with Previous"), "⇧E"),
            C::ExportEverything => (s("Export Everything…"), ""),
            C::EditExternal => (s("Edit in External Editor"), "⌘E"),
            C::Publish => (s("Publish Gallery…"), ""),
            C::Rename => (s("Rename Photos…"), "F2"),
            C::MoveToFolder => (s("Move to Folder…"), ""),
            C::RevealInFinder => (
                s(if cfg!(target_os = "windows") {
                    "Show in File Explorer"
                } else {
                    "Reveal in Finder"
                }),
                "",
            ),
            C::RelinkMissing => (s("Relink Missing Folder…"), ""),
            C::ManageCatalog => (s("Manage Catalog…"), ""),
            C::NewCatalog => (s("New Catalog…"), ""),
            C::OpenCatalog => (s("Open Catalog…"), ""),
            C::BackUpCatalog => (s("Back Up Catalog"), ""),
            C::BackupSettings => (s("Backup Settings…"), "S"),
            C::SyncNow => (s("Back Up Now"), ""),
            C::RetrySync => (s("Retry Failed Uploads"), "R"),
            C::ApplePhotos => (s("Apple Photos…"), ""),
            C::ApplePhotosSync => (s("Sync with Apple Photos"), ""),
            C::BuildSmartPreviews => (s("Build Smart Previews"), ""),
            C::BuildOneToOnePreviews => (s("Build 1:1 Previews"), ""),
            C::Undo => (s("Undo"), "⌘Z"),
            C::Redo => (s("Redo"), "⇧⌘Z"),
            C::SelectAll => (s("Select All"), "⌘A"),
            C::Deselect => (s("Deselect"), "⌘D"),
            C::CopySettings => (s("Copy Develop Settings"), ""),
            C::PasteSettings => (s("Paste Develop Settings"), ""),
            C::Find => (s("Find"), "/"),
            C::KeywordManager => (s("Keyword Manager…"), "K"),
            C::Rating(0) => (s("No Rating"), "0"),
            C::Rating(n) => (
                format!("{} Star{}", n, if n == 1 { "" } else { "s" }),
                ["", "1", "2", "3", "4", "5"][n.min(5) as usize],
            ),
            C::Pick => (s("Pick"), "P"),
            C::Reject => (s("Reject"), "X"),
            C::Unflag => (s("Unflag"), "U"),
            C::Label(0) => (s("No Label"), ""),
            C::Label(n) => (
                ["", "Red", "Yellow", "Green", "Blue", "Purple"][n.min(5) as usize].to_string(),
                ["", "6", "7", "8", "9", ""][n.min(5) as usize],
            ),
            C::AddToTarget => (s("Add to Target Collection"), "B"),
            C::NewCollection => (s("New Collection…"), ""),
            C::NewSmartCollection => (s("New Smart Collection from Filters…"), ""),
            C::CreateStack => (s("Stack Selected Photos"), ""),
            C::ToggleStack => (s("Expand / Collapse Stack"), ""),
            C::Unstack => (s("Unstack Photos"), ""),
            C::StackPairs => (s("Stack RAW/JPEG Pairs"), ""),
            C::RotateLeft => (s("Rotate Left"), "⌘["),
            C::RotateRight => (s("Rotate Right"), "⌘]"),
            C::FlipHorizontal => (s("Flip Horizontal"), "["),
            C::FlipVertical => (s("Flip Vertical"), "]"),
            C::DeleteRejected => (s("Delete Rejected Photos…"), "⌘⌫"),
            C::AutoAdvance => (s("Auto-Advance"), "A"),
            C::Develop => (s("Develop"), "D"),
            C::CropTool => (s("Crop Tool"), ""),
            C::ApplyCrop => (s("Apply Crop"), "↩"),
            C::CancelCrop => (s("Cancel Crop"), "Esc"),
            C::ResetCrop => (s("Reset Crop"), ""),
            C::ResetGeometry => (s("Reset Geometry"), ""),
            C::Upright(m) => (
                laika_core::upright::UprightMode::from_code(m)
                    .label()
                    .to_string(),
                "",
            ),
            C::AutoWhiteBalance => (s("Auto White Balance"), ""),
            C::BeforeAfter => (s("Before / After Split"), "Y"),
            C::ViewGrid => (s("Grid"), "G"),
            C::ViewLoupe => (s("Loupe"), "E"),
            C::ViewCompare => (s("Compare"), "C"),
            C::ViewSurvey => (s("Survey"), "N"),
            C::ViewTimeline => (s("Timeline"), "T"),
            C::Map => (s("Map"), "M"),
            C::ViewWall => (s("Wall"), "W"),
            C::ZoomCycle => (s("Cycle Zoom"), "Z"),
            C::ZoomFit => (s("Zoom to Fit"), ""),
            C::Zoom100 => (s("Zoom 100%"), ""),
            C::Zoom200 => (s("Zoom 200%"), ""),
            C::CellOverlay => (s("Cycle Cell Overlay"), "J"),
            C::LoupeInfo => (s("Cycle Loupe Info"), "I"),
            C::CropOverlay => (s("Cycle Crop Overlay"), "O"),
            C::Slideshow => (s("Slideshow"), "⌘↩"),
            C::Lights => (s("Cycle Lights"), "L"),
            C::ToggleLeftRail => (s("Show / Hide Left Panel"), ""),
            C::ToggleRightRail => (s("Show / Hide Right Panel"), ""),
            C::ToggleFilmstrip => (s("Show / Hide Filmstrip"), ""),
            C::TextLarger => (s("Make Text Larger"), ""),
            C::TextSmaller => (s("Make Text Smaller"), ""),
            C::TextDefault => (s("Reset Text Size"), ""),
            C::HideChrome => (s("Hide Panels"), "⇧⇥"),
            C::FullScreen => (s("Full Screen"), "F"),
            C::Shortcuts => (s("Keyboard Shortcuts"), "?"),
            C::ShowLog => (
                s(if cfg!(target_os = "windows") {
                    "Show Log in File Explorer"
                } else {
                    "Show Log in Finder"
                }),
                "",
            ),
            C::Welcome => (s("Welcome Screen"), ""),
        }
    }

    /// Menu label with the shortcut shown after the title.
    pub fn label(self) -> String {
        let (title, key) = self.title();
        if key.is_empty() {
            title
        } else {
            let key = if cfg!(target_os = "windows") {
                key.replace('⇧', "Shift+")
                    .replace('⌥', "Alt+")
                    .replace('⌘', "Ctrl+")
            } else {
                key.to_string()
            };
            format!("{title}    {key}")
        }
    }
}

/// The whole menu bar (App menu first; macOS names it after the app).
pub(crate) fn menu_tree() -> Vec<(&'static str, Vec<Entry>)> {
    let mut file_menu = vec![
        Cmd(C::ImportPhotos),
        Cmd(C::ImportLightroom),
        Cmd(C::ImportPresets),
        Cmd(C::SidecarConflicts),
        Cmd(C::Export),
        Cmd(C::ExportPrevious),
        Cmd(C::ExportEverything),
        Cmd(C::EditExternal),
        Cmd(C::Publish),
        Sep,
        Cmd(C::Rename),
        Cmd(C::MoveToFolder),
        Cmd(C::RevealInFinder),
        Cmd(C::RelinkMissing),
        Sep,
        Sub(
            "Catalog",
            vec![
                Cmd(C::ManageCatalog),
                Cmd(C::NewCatalog),
                Cmd(C::OpenCatalog),
                Cmd(C::BackUpCatalog),
                Sep,
                Cmd(C::BuildSmartPreviews),
                Cmd(C::BuildOneToOnePreviews),
            ],
        ),
        Sub(
            "Backup",
            vec![Cmd(C::BackupSettings), Cmd(C::SyncNow), Cmd(C::RetrySync)],
        ),
    ];
    if cfg!(target_os = "macos") {
        file_menu.push(Sub(
            "Apple Photos",
            vec![Cmd(C::ApplePhotos), Cmd(C::ApplePhotosSync)],
        ));
    }
    vec![
        (
            "Laika",
            vec![Cmd(C::About), Sep, Cmd(C::Preferences), Sep, Cmd(C::Quit)],
        ),
        ("File", file_menu),
        (
            "Edit",
            vec![
                Cmd(C::Undo),
                Cmd(C::Redo),
                Sep,
                Cmd(C::SelectAll),
                Cmd(C::Deselect),
                Sep,
                Cmd(C::CopySettings),
                Cmd(C::PasteSettings),
                Sep,
                Cmd(C::Find),
                Cmd(C::KeywordManager),
            ],
        ),
        (
            "Photo",
            vec![
                Sub("Rating", (0..=5).map(|n| Cmd(C::Rating(n))).collect()),
                Sub("Flag", vec![Cmd(C::Pick), Cmd(C::Reject), Cmd(C::Unflag)]),
                Sub(
                    "Color Label",
                    (1..=5).chain([0]).map(|n| Cmd(C::Label(n))).collect(),
                ),
                Cmd(C::AddToTarget),
                Cmd(C::NewCollection),
                Cmd(C::NewSmartCollection),
                Sub(
                    "Stacking",
                    vec![
                        Cmd(C::CreateStack),
                        Cmd(C::ToggleStack),
                        Cmd(C::Unstack),
                        Cmd(C::StackPairs),
                    ],
                ),
                Cmd(C::AutoAdvance),
                Sep,
                Cmd(C::RotateLeft),
                Cmd(C::RotateRight),
                Cmd(C::FlipHorizontal),
                Cmd(C::FlipVertical),
                Sep,
                Cmd(C::DeleteRejected),
            ],
        ),
        (
            "Develop",
            vec![
                Cmd(C::Library),
                Cmd(C::Develop),
                Sep,
                Cmd(C::WhiteBalancePicker),
                Cmd(C::CropTool),
                Cmd(C::ApplyCrop),
                Cmd(C::CancelCrop),
                Cmd(C::ResetCrop),
                Cmd(C::CropOverlay),
                Sep,
                Sub("Upright", (0..=5).map(|m| Cmd(C::Upright(m))).collect()),
                Cmd(C::ResetGeometry),
                Sep,
                Cmd(C::AutoWhiteBalance),
                Cmd(C::BeforeAfter),
            ],
        ),
        (
            "View",
            vec![
                Cmd(C::ViewGrid),
                Cmd(C::ViewLoupe),
                Cmd(C::ViewCompare),
                Cmd(C::ViewSurvey),
                Cmd(C::ViewTimeline),
                Cmd(C::Map),
                Cmd(C::ViewWall),
                Sep,
                Cmd(C::ZoomCycle),
                Cmd(C::ZoomFit),
                Cmd(C::Zoom100),
                Cmd(C::Zoom200),
                Sep,
                Cmd(C::CellOverlay),
                Cmd(C::LoupeInfo),
                Sep,
                Cmd(C::Slideshow),
                Cmd(C::Lights),
                Sep,
                Cmd(C::ToggleLeftRail),
                Cmd(C::ToggleRightRail),
                Cmd(C::ToggleFilmstrip),
                Sub(
                    "Text Size",
                    vec![Cmd(C::TextLarger), Cmd(C::TextSmaller), Cmd(C::TextDefault)],
                ),
                Cmd(C::HideChrome),
                Cmd(C::FullScreen),
            ],
        ),
        (
            "Help",
            vec![
                Cmd(C::CommandPalette),
                Cmd(C::Shortcuts),
                Cmd(C::LightroomGuide),
                Sep,
                Cmd(C::Welcome),
                Cmd(C::ShowLog),
                Cmd(C::About),
            ],
        ),
    ]
}

/// Stable ids for actions (index into the flattened command list).
pub(crate) fn all_commands() -> Vec<Command> {
    fn walk(entries: &[Entry], out: &mut Vec<Command>) {
        for e in entries {
            match e {
                Cmd(c) => {
                    if !out.contains(c) {
                        out.push(*c)
                    }
                }
                Sub(_, sub) => walk(sub, out),
                Sep => {}
            }
        }
    }
    let mut out = Vec::new();
    for (_, entries) in menu_tree() {
        walk(&entries, &mut out);
    }
    out
}

fn command_id(c: Command) -> usize {
    all_commands().iter().position(|x| *x == c).unwrap_or(0)
}

/// Native menus. Only commands that never conflict with typing get real
/// key equivalents (⌘Q, ⌘,, ⌘H); the rest show their key in the title and
/// stay on the window's own key handler (so fields and dialogs keep them).
pub(crate) fn install_native_menus(cx: &mut App, handle: WindowHandle<Laika>) {
    if cfg!(target_os = "macos") {
        cx.bind_keys([
            KeyBinding::new("cmd-q", Quit, None),
            KeyBinding::new("cmd-,", OpenPreferences, None),
            KeyBinding::new("cmd-h", HideApp, None),
            KeyBinding::new("alt-cmd-h", HideOthers, None),
        ]);
    } else {
        cx.bind_keys([
            KeyBinding::new("ctrl-q", Quit, None),
            KeyBinding::new("ctrl-,", OpenPreferences, None),
        ]);
    }
    // Menu actions arrive while the window is mid-dispatch (it can't be
    // updated re-entrantly), so the work runs right after.
    cx.on_action(move |a: &RunCommand, cx| {
        let Some(cmd) = all_commands().get(a.id).copied() else {
            return;
        };
        cx.defer(move |cx| {
            handle
                .update(cx, |laika, window, cx| laika.run_command(cmd, window, cx))
                .ok();
        });
    });
    cx.on_action(move |_: &Quit, cx| {
        cx.defer(move |cx| {
            handle.update(cx, |laika, _, _| laika.flush_saves()).ok();
            cx.quit();
        });
    });
    cx.on_action(move |_: &OpenPreferences, cx| {
        cx.defer(move |cx| {
            handle
                .update(cx, |laika, _, cx| laika.open_settings(cx))
                .ok();
        });
    });
    cx.on_action(|_: &HideApp, cx| cx.hide());
    cx.on_action(|_: &HideOthers, cx| cx.hide_other_apps());

    refresh_native_menus(cx);
}

/// Rebuild the menu bar (titles carry shortcuts, which follow the keyboard
/// map).
pub(crate) fn refresh_native_menus(cx: &mut App) {
    fn items(entries: &[Entry]) -> Vec<MenuItem> {
        entries
            .iter()
            .map(|e| match e {
                Sep => MenuItem::separator(),
                Sub(name, sub) => MenuItem::submenu(Menu::new(*name).items(items(sub))),
                Cmd(C::Quit) => MenuItem::action("Quit Laika", Quit),
                Cmd(C::Preferences) => MenuItem::action("Preferences…", OpenPreferences),
                Cmd(c) => MenuItem::action(c.label(), RunCommand { id: command_id(*c) }),
            })
            .collect()
    }
    let mut menus: Vec<Menu> = Vec::new();
    for (name, entries) in menu_tree() {
        let mut list = items(&entries);
        if name == "Laika" && cfg!(target_os = "macos") {
            // macOS app-menu conventions.
            list.insert(list.len() - 1, MenuItem::action("Hide Laika", HideApp));
            list.insert(list.len() - 1, MenuItem::action("Hide Others", HideOthers));
            list.insert(list.len() - 1, MenuItem::separator());
        }
        menus.push(Menu::new(name).items(list));
    }
    cx.set_menus(menus);
}

/// Show the in-window bar where there is no native menu bar.
pub(crate) fn in_window_menu() -> bool {
    !cfg!(target_os = "macos") || std::env::var("LAIKA_INWINDOW_MENU").is_ok_and(|v| v == "1")
}

impl Laika {
    /// The single entry point every menu, button-less command, and the
    /// keyboard's view/mode keys use.
    pub(crate) fn run_command(
        &mut self,
        cmd: Command,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.menu_open = None;
        match cmd {
            C::About => self.open_about(cx),
            C::ShowLog => self.show_log(cx),
            C::Welcome => {
                self.close_modals(cx);
                self.diag.note.clear();
                self.diag.welcome_open = true;
            }
            C::Preferences => self.open_settings(cx),
            C::Quit => {
                self.flush_saves();
                cx.quit();
            }
            C::ImportPhotos => self.open_import_dialog(cx),
            C::ImportLightroom => self.open_lightroom_import(cx),
            C::ImportPresets => self.open_preset_import(cx),
            C::SidecarConflicts => self.open_conflicts(cx),
            C::Export => self.open_export_dialog(cx),
            C::ExportPrevious => self.export_with_previous(cx),
            C::ExportEverything => self.open_exit_bundle(cx),
            C::EditExternal => self.edit_in_external_editor(cx),
            C::Publish => self.open_publish(cx),
            C::Rename => self.open_rename(cx),
            C::MoveToFolder => self.open_move_picker(cx),
            C::RevealInFinder => self.reveal_primary(cx),
            C::RelinkMissing => self.open_relink_folder(cx),
            C::ManageCatalog => {
                self.close_modals(cx);
                self.manage_open = true;
                self.manage_note.clear();
                self.kick_cache_audit(cx);
            }
            C::NewCatalog => self.open_new_catalog_picker(cx),
            C::OpenCatalog => self.open_existing_picker(cx),
            C::BackUpCatalog => self.backup_catalog(cx),
            C::BackupSettings => self.open_sync(cx),
            C::SyncNow => self.start_sync(cx),
            C::RetrySync => self.retry_failed_sync(cx),
            C::ApplePhotos => self.open_apple(cx),
            C::ApplePhotosSync => self.start_apple_sync(true, cx),
            C::BuildSmartPreviews => self.start_preview_build(PreviewKind::Smart, cx),
            C::BuildOneToOnePreviews => self.start_preview_build(PreviewKind::OneToOne, cx),
            C::Undo => self.undo_action(cx),
            C::Redo => self.redo_action(cx),
            C::SelectAll => self.select_all(cx),
            C::Deselect => self.deselect(cx),
            C::CopySettings => {
                if let Some(pid) = self.state.primary {
                    self.copy_settings_from(pid, cx);
                }
            }
            C::PasteSettings => self.paste_settings(cx),
            C::Find => self.focus_field(text_input::FieldId::SearchQuery, cx),
            C::KeywordManager => {
                self.close_modals(cx);
                self.kw_open = true;
            }
            C::Rating(n) => self.apply_rating(n.min(5), cx),
            C::Pick => self.apply_flag(true, cx),
            C::Reject => self.apply_flag(false, cx),
            C::Unflag => self.clear_flags(cx),
            C::Label(0) => {
                let current = self
                    .state
                    .primary
                    .and_then(|id| self.find(id))
                    .map(|p| p.label);
                if let Some(l) = current.filter(|l| *l > 0) {
                    self.apply_label(l, cx);
                }
            }
            C::Label(n) => self.apply_label(n, cx),
            C::AddToTarget => self.toggle_in_target(cx),
            C::NewCollection => self.open_name_field(collections::NameMode::New, cx),
            C::NewSmartCollection => self.open_name_field(collections::NameMode::NewSmart, cx),
            C::CreateStack => self.create_stack_from_selection(cx),
            C::ToggleStack => self.toggle_primary_stack(cx),
            C::Unstack => self.unstack_primary(cx),
            C::StackPairs => self.stack_raw_jpeg_pairs(cx),
            C::RotateLeft => self.rotate_targets(false, cx),
            C::RotateRight => self.rotate_targets(true, cx),
            C::FlipHorizontal => self.flip_targets(true, cx),
            C::FlipVertical => self.flip_targets(false, cx),
            C::DeleteRejected => self.delete_rejected(cx),
            C::AutoAdvance => {
                self.auto_advance = !self.auto_advance;
                self.status_note = if self.auto_advance {
                    "auto-advance on — ratings step to the next photo".to_string()
                } else {
                    "auto-advance off".to_string()
                };
            }
            C::Library => {
                self.flush_saves();
                if self.crop_open {
                    self.cancel_crop(cx);
                }
                self.state.active_module = Module::Library;
                self.publish_open = false;
            }
            C::WhiteBalancePicker => {
                if self.state.active_module != Module::Develop {
                    self.run_command(C::Develop, window, cx);
                }
                self.wb_pick = !self.wb_pick;
                self.status_note = if self.wb_pick {
                    "click neutral gray to set white balance (Esc exits)".to_string()
                } else {
                    String::new()
                };
            }
            C::CommandPalette => self.open_palette(cx),
            C::LightroomGuide => self.open_lightroom_guide(cx),
            C::Develop => {
                self.flush_saves();
                self.state.active_module = Module::Develop;
                if let Some(pid) = self.state.primary {
                    self.open_develop_for(pid, cx);
                }
            }
            C::CropTool => {
                if self.state.active_module != Module::Develop {
                    self.run_command(C::Develop, window, cx);
                }
                if self.crop_open {
                    self.cancel_crop(cx);
                } else {
                    if self.zoom != zoom::ZoomLevel::Fit {
                        self.set_zoom(zoom::ZoomLevel::Fit, Some((0.5, 0.5)), cx);
                    }
                    self.open_crop_tool(cx);
                }
            }
            C::ApplyCrop => {
                if self.crop_open {
                    self.apply_crop(cx);
                }
            }
            C::CancelCrop => {
                if self.crop_open {
                    self.cancel_crop(cx);
                }
            }
            C::ResetCrop => {
                if self.crop_open {
                    self.reset_crop(cx);
                } else {
                    self.status_note = "open the crop tool first".to_string();
                }
            }
            C::ResetGeometry => self.reset_geometry(cx),
            C::Upright(m) => {
                self.set_upright_mode(laika_core::upright::UprightMode::from_code(m), cx)
            }
            C::AutoWhiteBalance => self.auto_wb(cx),
            C::BeforeAfter => {
                let pid = self.state.primary;
                let geom = pid.map(|id| self.render_geom(id)).unwrap_or_default();
                let values = pid
                    .map(|id| self.effective_values(id, self.values))
                    .unwrap_or_else(edit::defaults);
                if let Some(dev) = self.dev.as_mut() {
                    dev.split = if dev.split > 0. { 0. } else { 0.38 };
                    dev.submit_current(&values, geom, pid);
                }
            }
            C::ViewGrid | C::ViewLoupe => {
                // U02: never switch modules dirty.
                self.flush_saves();
                self.state.active_module = Module::Library;
                let v = if cmd == C::ViewGrid {
                    ViewMode::Grid
                } else {
                    ViewMode::Loupe
                };
                self.view = v;
                self.prev_view = v;
                self.publish_open = false;
                // U09: the eyedropper is a Develop-mode tool.
                self.wb_pick = false;
            }
            C::ViewCompare => self.open_compare(cx),
            C::ViewSurvey => self.open_survey(cx),
            C::Map => self.open_map(cx),
            C::ViewTimeline => {
                self.state.active_module = Module::Library;
                self.prev_view = ViewMode::Timeline;
                self.view = ViewMode::Timeline;
            }
            C::ViewWall => {
                self.state.active_module = Module::Library;
                if self.view == ViewMode::Wall {
                    self.view = self.prev_view;
                    // V31: undo the chrome the wall hid on entry.
                    if self.library.prefs.wall_hides_chrome {
                        self.hide_chrome = false;
                    }
                } else {
                    self.prev_view = self.view;
                    self.view = ViewMode::Wall;
                    if self.library.prefs.wall_hides_chrome {
                        self.hide_chrome = true;
                    }
                }
            }
            C::ZoomCycle => self.cycle_zoom(cx),
            C::ZoomFit => self.set_zoom(zoom::ZoomLevel::Fit, Some((0.5, 0.5)), cx),
            C::Zoom100 => self.set_zoom(zoom::ZoomLevel::Full, None, cx),
            C::Zoom200 => self.set_zoom(zoom::ZoomLevel::Double, None, cx),
            C::CellOverlay => {
                self.overlay = self.overlay.cycle();
                self.save_cell_prefs();
            }
            C::LoupeInfo => self.cycle_loupe_info(cx),
            C::CropOverlay => {
                if self.crop_open {
                    self.cycle_overlay(false, cx);
                } else {
                    self.status_note = "crop overlays show while cropping".to_string();
                }
            }
            C::Slideshow => self.start_slideshow(window, cx),
            C::Lights => self.lights = (self.lights + 1) % 3,
            C::ToggleLeftRail => self.set_prefs(|p| p.left_rail_visible = !p.left_rail_visible, cx),
            C::ToggleRightRail => {
                self.set_prefs(|p| p.right_rail_visible = !p.right_rail_visible, cx)
            }
            C::ToggleFilmstrip => {
                self.set_prefs(|p| p.filmstrip_visible = !p.filmstrip_visible, cx)
            }
            C::TextLarger => self.set_prefs(
                |p| p.text_scale_percent = p.text_scale_percent.saturating_add(5).min(150),
                cx,
            ),
            C::TextSmaller => self.set_prefs(
                |p| p.text_scale_percent = p.text_scale_percent.saturating_sub(5).max(85),
                cx,
            ),
            C::TextDefault => self.set_prefs(|p| p.text_scale_percent = 100, cx),
            C::HideChrome => self.hide_chrome = !self.hide_chrome,
            C::FullScreen => window.toggle_fullscreen(),
            C::Shortcuts => {
                let open = !self.help_open;
                self.close_modals(cx);
                self.help_open = open;
            }
        }
        cx.notify();
    }

    /// In-window menu bar (Linux; macOS with `LAIKA_INWINDOW_MENU=1`).
    pub(crate) fn menu_bar(&self, cx: &mut Context<Self>) -> Option<Div> {
        if !in_window_menu() {
            return None;
        }
        let mut bar = div()
            .flex_none()
            .h(px(26.))
            .flex()
            .items_center()
            .gap(px(2.))
            .px(px(8.))
            .bg(rgb(bg_chrome()))
            .border_b_1()
            .border_color(hairline())
            .font_family(SANS)
            .text_size(sp(12.));
        for (i, (name, _)) in menu_tree().into_iter().enumerate() {
            let open = self.menu_open == Some(i);
            bar = bar.child(
                div()
                    .id(("menu-top", i))
                    .px(px(8.))
                    .py(px(3.))
                    .rounded(px(3.))
                    .text_color(rgb(if open { TEXT_PRIMARY } else { TEXT_SECONDARY }))
                    .when(open, |d| d.bg(rgb(bg_segment_active())))
                    .hover(|s| s.bg(rgb(bg_row_hover())))
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.menu_open = if this.menu_open == Some(i) {
                            None
                        } else {
                            Some(i)
                        };
                        cx.notify();
                    }))
                    .child(name),
            );
        }
        Some(bar)
    }

    /// The open in-window dropdown (submenus render inline, indented).
    pub(crate) fn menu_dropdown(&self, cx: &mut Context<Self>) -> Option<Div> {
        let i = self.menu_open?;
        if !in_window_menu() {
            return None;
        }
        let (_, entries) = menu_tree().into_iter().nth(i)?;
        let x = 8.
            + menu_tree()
                .iter()
                .take(i)
                .map(|(n, _)| n.len() as f32 * 7.5 + 18.)
                .sum::<f32>();
        let mut list = div().flex().flex_col().py(px(4.));
        fn add(
            this: &Laika,
            list: Div,
            entries: &[Entry],
            depth: usize,
            key: &mut usize,
            cx: &mut Context<Laika>,
        ) -> Div {
            let mut list = list;
            for e in entries {
                *key += 1;
                match e {
                    Sep => list = list.child(div().my(px(3.)).h(px(1.)).bg(hairline())),
                    Sub(name, sub) => {
                        list = list.child(
                            div()
                                .px(px(12. + depth as f32 * 12.))
                                .py(px(3.))
                                .text_size(sp(10.5))
                                .text_color(rgb(TEXT_DIM))
                                .child(name.to_string()),
                        );
                        list = add(this, list, sub, depth + 1, key, cx);
                    }
                    Cmd(c) => {
                        let c = *c;
                        let (title, shortcut) = c.title();
                        list = list.child(
                            div()
                                .id(("menu-item", *key))
                                .flex()
                                .justify_between()
                                .gap(px(24.))
                                .pl(px(12. + depth as f32 * 12.))
                                .pr(px(12.))
                                .py(px(4.))
                                .text_color(rgb(TEXT_PRIMARY))
                                .hover(|s| s.bg(rgb(bg_row_hover())))
                                .on_click(cx.listener(move |this, _, window, cx| {
                                    this.run_command(c, window, cx);
                                }))
                                .child(title)
                                .child(div().text_color(rgb(TEXT_DIM)).child(shortcut)),
                        );
                    }
                }
            }
            list
        }
        let mut key = 0;
        list = add(self, list, &entries, 0, &mut key, cx);
        Some(
            div()
                .absolute()
                .size_full()
                .child(
                    div()
                        .id("menu-catcher")
                        .absolute()
                        .size_full()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _, _, cx| {
                                this.menu_open = None;
                                cx.notify();
                            }),
                        ),
                )
                .child(
                    div()
                        .id("menu-dropdown")
                        .occlude()
                        .absolute()
                        .left(px(x))
                        .top(px(26.))
                        .min_w(px(240.))
                        .max_h(px(620.))
                        .overflow_y_scroll()
                        .rounded(px(5.))
                        .bg(rgb(bg_chrome()))
                        .border_1()
                        .border_color(border_control())
                        .shadow_lg()
                        .font_family(SANS)
                        .text_size(sp(12.))
                        .child(list),
                ),
        )
    }

    /// V31: render the targets with the External Editing preferences next
    /// to their originals and open them in the chosen application.
    pub(crate) fn edit_in_external_editor(&mut self, cx: &mut Context<Self>) {
        let prefs = self.library.prefs.clone();
        if prefs.editor_app.trim().is_empty() {
            self.status_note =
                "choose an external editor in Preferences → External Editing".to_string();
            self.open_settings(cx);
            self.prefs_tab = 3;
            cx.notify();
            return;
        }
        let ids = self.in_visible_order(self.targets());
        let Some(first) = ids.first().and_then(|id| self.find(*id)).cloned() else {
            self.status_note = "select photos first — nothing to edit".to_string();
            cx.notify();
            return;
        };
        let dest = std::path::Path::new(&first.path)
            .parent()
            .map(|p| p.to_path_buf());
        self.flush_saves();
        self.export_dialog = Some(ExportDialog {
            ids,
            dest,
            naming: prefs.editor_naming.clone(),
            quality: 95,
            long_edge: None,
            meta: ExportMeta::All,
            collision: ExportCollision::Suffix,
            format: match prefs.editor_format {
                laika_core::prefs::EditorFormat::Tiff => ExportFormat::Tiff,
                laika_core::prefs::EditorFormat::Jpeg => ExportFormat::Jpeg,
            },
            format_opts: ExportFormatOpts {
                quality: 95,
                ..ExportFormatOpts::default()
            },
            watermark: WatermarkSpec::default(),
            post_action: ExportPostAction::Open,
            script: String::new(),
            presets: Vec::new(),
            run_set: Vec::new(),
            preset_name: String::new(),
            preset_folder: String::new(),
            run_actions: Vec::new(),
            exported: std::collections::HashMap::new(),
            run: None,
            report: Vec::new(),
            retry_ids: Vec::new(),
            done_count: 0,
            skipped_count: 0,
            open_with: Some(prefs.editor_app.clone()),
        });
        self.status_note = format!(
            "rendering for {}…",
            std::path::Path::new(&prefs.editor_app)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| prefs.editor_app.clone())
        );
        self.start_export(false, cx);
    }
}

#[cfg(test)]
mod tests {
    use super::{Command, all_commands, command_id};

    #[test]
    fn every_command_is_in_a_menu_once_with_a_stable_id() {
        let all = all_commands();
        assert!(all.len() > 70, "{}", all.len());
        for (i, c) in all.iter().enumerate() {
            assert_eq!(command_id(*c), i);
            assert!(!c.title().0.is_empty());
        }
        // Keyboard shortcuts documented in the menus.
        for (c, key) in [
            (Command::ViewGrid, "G"),
            (Command::Label(1), "6"),
            (Command::AddToTarget, "B"),
            (Command::Slideshow, "⌘↩"),
            (Command::Rating(3), "3"),
        ] {
            assert!(all.contains(&c));
            assert_eq!(c.title().1, key);
        }
    }
}
