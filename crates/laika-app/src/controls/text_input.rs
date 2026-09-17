//! Immediate-mode single-line text input (U04).
//!
//! The owning view keeps one active [`FieldEdit`], routes keystrokes to it,
//! and renders it with [`render_editor`]. Buffer mechanics are pure string
//! operations over char-boundary byte indices, unit-tested below without a
//! window. Platform IME composition is not exposed by the UI stack, so CJK
//! entry arrives via clipboard paste; every other deliverable (caret,
//! selection, clipboard, undo, validation, Enter/Esc) is implemented.

use crate::theme::*;
use gpui_kit::prelude::*;
use gpui_kit::*;

/// Editable field identities.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FieldId {
    PublishTitle,
    PublishSlug,
    S3Endpoint,
    S3Bucket,
    S3Region,
    S3Key,
    S3Secret,
    /// Backup to a Linux server over SFTP.
    SftpHost,
    SftpPort,
    SftpUser,
    SftpPath,
    SftpIdentity,
    /// Backup to a mounted SMB/NFS share.
    ShareServer,
    ShareName,
    SharePath,
    /// Settings: custom accent color (`#RRGGBB`).
    AccentHex,
    /// Apple Photos: album (or folder) name.
    PhotosAlbum,
    /// V20: Loupe info line templates.
    InfoFileLine,
    InfoExposureLine,
    /// V22: numeric crop box (display pixels or percent).
    CropX,
    CropY,
    CropW,
    CropH,
    SliderValue(usize),
    /// V02: shoot text shared by the import/rename name forms.
    NameShoot,
    /// V02: custom folder / rename template text.
    NameFolderCustom,
    NameRenameCustom,
    /// V02: sequence start number.
    NameSeqStart,
    /// V03: metadata preset name for save-as.
    MdPresetName,
    /// V03: inline authorship fields.
    MdCreator,
    MdCopyright,
    MdRights,
    MdContact,
    MdKeywords,
    /// V03: capture offset in decimal hours → whole minutes.
    MdOffsetHours,
    /// U12: library search query.
    SearchQuery,
    /// U12: filter preset name for save-as.
    FilterPresetName,
    /// U12: capture-date window edges (`YYYY-MM-DD`, empty clears).
    DateFrom,
    DateTo,
    /// U14: snapshot name for save-as.
    SnapshotName,
    /// V04: folder create/rename name entry.
    FolderName,
    /// V07: new catalog name entry.
    CatalogName,
    /// U08: custom crop ratio (`W:H` or decimal).
    CropCustom,
    /// U08: straighten angle in degrees (-45..45).
    CropAngle,
    /// U10: export filename template.
    ExportNaming,
    /// U10: JPEG quality 1..100.
    ExportQuality,
    /// U10: long-edge pixels (empty = full resolution).
    ExportLongEdge,
    /// V27: JPEG size cap in MB (empty = off).
    ExportMaxMb,
    /// V28: watermark copy (one line).
    ExportWmText,
    /// V28: watermark font family.
    ExportWmFont,
    /// V28: watermark graphic file path.
    ExportWmGraphic,
    /// V28: watermark edge margin in px (empty resets to 24).
    ExportWmMargin,
    /// V28: export preset name (save box).
    ExportPresetName,
    /// V28: export preset folder (save box).
    ExportPresetFolder,
    /// V28: post-export script path.
    ExportPostScript,
    /// V12: right-rail descriptive fields (single + batch).
    MetaTitle,
    MetaCaption,
    MetaHeadline,
    MetaCreator,
    MetaCopyright,
    MetaRights,
    MetaContact,
    MetaLocation,
    /// V12: rail keyword add box (comma/semicolon list, union).
    MetaKeywords,
    /// V12: rail metadata preset name (save box).
    MetaPresetName,
    /// V12: keyword manager: rename target for the selected node.
    KwRename,
    /// V12: keyword manager: merge-selected-into target path.
    KwMergeInto,
    /// V12: keyword manager: synonym to add.
    KwSynonym,
    /// V12: keyword manager: set name (save box).
    KwSetName,
    /// V12: keyword manager: keyword import/export file paths.
    KwImportPath,
    KwExportPath,
    /// V18: timeline Jump to Date (YYYY-MM-DD or YYYY-MM).
    TimelineJump,
}

/// One active editor: buffer plus caret/selection/undo state.
/// `caret` and `anchor` are always char boundaries into `buffer`.
#[derive(Clone, Debug)]
pub struct FieldEdit {
    pub id: FieldId,
    pub buffer: String,
    pub caret: usize,
    pub anchor: Option<usize>,
    pub undo: Vec<String>,
    pub error: Option<String>,
}

impl FieldEdit {
    pub fn new(id: FieldId, text: &str) -> Self {
        let caret = text.len();
        Self {
            id,
            buffer: text.to_string(),
            caret,
            anchor: None,
            undo: Vec::new(),
            error: None,
        }
    }

    fn push_undo(&mut self) {
        if self.undo.last().is_none_or(|top| *top != self.buffer) {
            self.undo.push(self.buffer.clone());
            if self.undo.len() > 50 {
                self.undo.remove(0);
            }
        }
    }

    pub fn undo(&mut self) {
        if let Some(prev) = self.undo.pop() {
            self.buffer = prev;
            self.caret = self.buffer.len();
            self.anchor = None;
        }
    }

    fn boundary_at(s: &str, mut idx: usize, dir: i32) -> usize {
        idx = idx.clamp(0, s.len());
        loop {
            if dir < 0 {
                if idx == 0 {
                    return 0;
                }
                idx -= 1;
            } else {
                if idx >= s.len() {
                    return s.len();
                }
                idx += 1;
            }
            if s.is_char_boundary(idx) {
                return idx;
            }
        }
    }

    pub fn move_left(&mut self, extend: bool) {
        let next = Self::boundary_at(&self.buffer, self.caret, -1);
        self.set_caret(next, extend);
    }

    pub fn move_right(&mut self, extend: bool) {
        let next = Self::boundary_at(&self.buffer, self.caret, 1);
        self.set_caret(next, extend);
    }

    pub fn home(&mut self, extend: bool) {
        self.set_caret(0, extend);
    }

    pub fn end(&mut self, extend: bool) {
        let len = self.buffer.len();
        self.set_caret(len, extend);
    }

    fn set_caret(&mut self, idx: usize, extend: bool) {
        if extend {
            if self.anchor.is_none() {
                self.anchor = Some(self.caret);
            }
        } else {
            self.anchor = None;
        }
        self.caret = idx.clamp(0, self.buffer.len());
        if self.anchor == Some(self.caret) {
            self.anchor = None;
        }
    }

    pub fn select_all(&mut self) {
        if self.buffer.is_empty() {
            return;
        }
        self.anchor = Some(0);
        self.caret = self.buffer.len();
    }

    /// Sorted (start, end) byte range of the selection, if any.
    pub fn selection(&self) -> Option<(usize, usize)> {
        let a = self.anchor?;
        if a == self.caret {
            return None;
        }
        Some((a.min(self.caret), a.max(self.caret)))
    }

    pub fn selected_text(&self) -> Option<&str> {
        let (a, b) = self.selection()?;
        Some(&self.buffer[a..b])
    }

    /// Insert text, replacing the selection. Newlines are stripped
    /// (single-line field) and pasted CJK survives intact.
    pub fn insert(&mut self, text: &str) {
        let clean: String = text.chars().filter(|c| *c != '\n' && *c != '\r').collect();
        if clean.is_empty() && self.selection().is_none() {
            return;
        }
        self.push_undo();
        if let Some((a, b)) = self.selection() {
            self.buffer.replace_range(a..b, &clean);
            self.caret = a + clean.len();
        } else {
            self.buffer.insert_str(self.caret, &clean);
            self.caret += clean.len();
        }
        self.anchor = None;
    }

    pub fn backspace(&mut self) {
        if let Some((a, b)) = self.selection() {
            self.push_undo();
            self.buffer.replace_range(a..b, "");
            self.caret = a;
            self.anchor = None;
            return;
        }
        if self.caret == 0 {
            return;
        }
        self.push_undo();
        let prev = Self::boundary_at(&self.buffer, self.caret, -1);
        self.buffer.replace_range(prev..self.caret, "");
        self.caret = prev;
    }

    pub fn delete_forward(&mut self) {
        if let Some((a, b)) = self.selection() {
            self.push_undo();
            self.buffer.replace_range(a..b, "");
            self.caret = a;
            self.anchor = None;
            return;
        }
        if self.caret >= self.buffer.len() {
            return;
        }
        self.push_undo();
        let next = Self::boundary_at(&self.buffer, self.caret, 1);
        self.buffer.replace_range(self.caret..next, "");
    }
}

// ---- validation (pure, tested) ----------------------------------------------

/// Gallery title: free text, bounded length.
pub fn validate_title(s: &str) -> Result<(), String> {
    if s.chars().count() > 120 {
        return Err("keep the title under 120 characters".to_string());
    }
    Ok(())
}

/// Gallery slug: empty (auto) or lowercase segments joined by dashes.
pub fn validate_slug(s: &str) -> Result<(), String> {
    if s.is_empty() {
        return Ok(());
    }
    let ok = !s.starts_with('-')
        && !s.ends_with('-')
        && !s.is_empty()
        && s.chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if ok && !s.contains("--") {
        Ok(())
    } else {
        Err("use lowercase letters, numbers, and single dashes (e.g. spring-shoot)".to_string())
    }
}

/// S3 endpoint: needs a scheme so the object store can dial it.
pub fn validate_endpoint(s: &str) -> Result<(), String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("enter an endpoint like http://localhost:9000".to_string());
    }
    if s.chars().any(|c| c.is_whitespace()) {
        return Err("endpoints can't contain spaces".to_string());
    }
    if !s.contains("://") {
        return Err("include the scheme, e.g. http://host:9000".to_string());
    }
    if !(s.starts_with("http://") || s.starts_with("https://")) {
        return Err("scheme must be http:// or https://".to_string());
    }
    Ok(())
}

/// S3 bucket: DNS-ish, lowercase.
pub fn validate_bucket(s: &str) -> Result<(), String> {
    let s = s.trim();
    if !(3..=63).contains(&s.len()) {
        return Err("use 3–63 lowercase letters, numbers, dots, or dashes".to_string());
    }
    let ok = s
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.');
    if !ok {
        return Err("use 3–63 lowercase letters, numbers, dots, or dashes".to_string());
    }
    Ok(())
}

/// Region: empty means the us-east-1 default.
pub fn validate_region(s: &str) -> Result<(), String> {
    if s.chars().any(|c| c.is_whitespace()) {
        return Err("regions can't contain spaces".to_string());
    }
    Ok(())
}

/// Access key: must be present.
pub fn validate_key(s: &str) -> Result<(), String> {
    if s.trim().is_empty() {
        return Err("enter the access key id".to_string());
    }
    if s.chars().any(|c| c.is_whitespace()) {
        return Err("keys can't contain spaces".to_string());
    }
    Ok(())
}

/// Secret: anything goes; empty keeps the stored one.
pub fn validate_host(s: &str) -> Result<(), String> {
    let s = s.trim();
    if s.is_empty() {
        return Err("enter the server's hostname or IP".to_string());
    }
    if s.chars().any(char::is_whitespace) || s.contains("://") {
        return Err("just the host, like nas.local or 192.168.1.20".to_string());
    }
    Ok(())
}

pub fn validate_port(s: &str) -> Result<(), String> {
    let s = s.trim();
    if s.is_empty() || s.parse::<u16>().is_ok_and(|p| p > 0) {
        Ok(())
    } else {
        Err("use a port from 1 to 65535 (empty = 22)".to_string())
    }
}

pub fn validate_no_spaces(s: &str, what: &str) -> Result<(), String> {
    if s.trim().chars().any(char::is_whitespace) {
        Err(format!("{what} can't contain spaces"))
    } else {
        Ok(())
    }
}

pub fn validate_required(s: &str, what: &str) -> Result<(), String> {
    if s.trim().is_empty() {
        Err(format!("enter {what}"))
    } else {
        Ok(())
    }
}

pub fn validate_secret(_s: &str) -> Result<(), String> {
    Ok(())
}

/// V02: sequence start number for `{seq}`.
pub fn validate_seq_start(s: &str) -> Result<u64, String> {
    let v: u64 = s
        .trim()
        .parse()
        .map_err(|_| "enter a start number like 1".to_string())?;
    if v > 999_999 {
        return Err("keep the start number under 1000000".to_string());
    }
    Ok(v)
}

/// V03: capture offset in decimal hours → whole minutes.
pub fn validate_offset_hours(s: &str) -> Result<i32, String> {
    let t = s.trim();
    if t.is_empty() {
        return Ok(0);
    }
    let h: f32 = t
        .replace('\u{2212}', "-")
        .parse()
        .map_err(|_| "enter hours like -5 or 5.5".to_string())?;
    if !h.is_finite() || h < -24. || h > 24. {
        return Err("keep the offset between -24 and +24 hours".to_string());
    }
    Ok((h * 60.).round() as i32)
}

/// U12: `YYYY-MM-DD` or empty (clears the edge).
pub fn validate_day(s: &str, what: &str) -> Result<(), String> {
    let t = s.trim();
    if t.is_empty() {
        return Ok(());
    }
    let parts: Vec<&str> = t.split('-').collect();
    let ok = parts.len() == 3
        && parts[0].len() == 4
        && parts[1].len() == 2
        && parts[2].len() == 2
        && parts.iter().all(|p| p.bytes().all(|b| b.is_ascii_digit()));
    if ok {
        Ok(())
    } else {
        Err(format!("{what} needs YYYY-MM-DD or empty"))
    }
}

/// V03: short text fields (creator/copyright/contact/preset names).
pub fn validate_short(s: &str, what: &str) -> Result<(), String> {
    validate_text(s, what, 200)
}

/// V12: longer single-line fields (captions run past 200 characters).
pub fn validate_text(s: &str, what: &str, max: usize) -> Result<(), String> {
    if s.chars().count() > max {
        return Err(format!("keep {what} under {max} characters"));
    }
    if s.chars().any(|c| c == '\n' || c == '\r') {
        return Err(format!("{what} must be one line"));
    }
    Ok(())
}

/// Parse a typed slider value: plain number, snapped to the def's range.
/// Unicode minus (from our own formatted readouts) is accepted.
pub fn parse_param_number(i: usize, s: &str) -> Result<f32, String> {
    let d = laika_core::edit::PARAMS[i];
    let norm = s.trim().replace('\u{2212}', "-");
    let v: f32 = norm.parse().map_err(|_| "enter a number".to_string())?;
    if !v.is_finite() {
        return Err("enter a number".to_string());
    }
    if v < d.min || v > d.max {
        return Err(format!(
            "enter a number from {} to {}",
            plain_param(i, d.min),
            plain_param(i, d.max)
        ));
    }
    Ok(v)
}

/// Plain editable rendering of a param value (no units or display signs).
pub fn plain_param(i: usize, v: f32) -> String {
    use laika_core::edit::Fmt;
    match laika_core::edit::PARAMS[i].fmt {
        Fmt::Kelvin => format!("{:.0}", v.round()),
        _ => {
            let r = (v * 100.).round() / 100.;
            let mut s = format!("{r:.2}");
            while s.contains('.') && (s.ends_with('0') || s.ends_with('.')) {
                s.pop();
            }
            if s == "-0" {
                s = "0".to_string();
            }
            s
        }
    }
}

// ---- rendering ----------------------------------------------------------------

/// Render an active editor: text with selection highlight, caret bar, focus
/// ring, and an optional validation error line. `mask` renders bullets for
/// secrets without ever echoing the value.
pub fn render_editor(edit: &FieldEdit, mask: bool) -> Div {
    let shown = if mask {
        "•".repeat(edit.buffer.chars().count())
    } else {
        edit.buffer.clone()
    };
    // Masking changes display width only; caret math stays on the buffer.
    let caret = if mask {
        shown.len()
    } else {
        edit.caret.min(shown.len())
    };
    let sel = edit.selection().and_then(|(a, b)| {
        if mask {
            None
        } else {
            Some((a.min(shown.len()), b.min(shown.len())))
        }
    });
    let (pre, mid, post) = match sel {
        Some((a, b)) => (&shown[..a], &shown[a..b], &shown[b..]),
        None => (
            &shown[..caret.min(shown.len())],
            "",
            &shown[caret.min(shown.len())..],
        ),
    };
    // When a selection exists the caret bar renders at its end (standard
    // behavior); the split above already placed `mid` before the bar.
    div()
        .flex()
        .flex_col()
        .gap(px(4.))
        .child(
            div()
                .flex()
                .items_center()
                .px(px(10.))
                .py(px(8.))
                .bg(rgb(bg_well()))
                .border_1()
                .border_color(rgb(accent_line()))
                .rounded(px(3.))
                .font_family(SANS)
                .text_size(px(11.5))
                .text_color(rgb(TEXT_PRIMARY))
                .child(div().child(pre.to_string()))
                .when(!mid.is_empty(), |d| {
                    d.child(
                        div()
                            .bg(rgb(accent_fill()))
                            .text_color(rgb(accent_on_fill()))
                            .child(mid.to_string()),
                    )
                })
                .child(
                    // Caret bar; sits at the selection end when selected.
                    div().w(px(2.)).h(px(14.)).bg(rgb(TEXT_PRIMARY)),
                )
                .child(div().child(post.to_string())),
        )
        .when(edit.error.is_some(), |d| {
            d.child(
                div()
                    .font_family(SANS)
                    .text_size(px(10.))
                    .text_color(rgb(0xE56060))
                    .child(edit.error.clone().unwrap_or_default()),
            )
        })
}

#[cfg(test)]
mod tests {
    // Explicit imports: the module glob pulls a `test` macro from the UI
    // stack that would shadow the builtin `#[test]` attribute.
    use super::{
        FieldEdit, FieldId, parse_param_number, plain_param, validate_bucket, validate_day,
        validate_endpoint, validate_key, validate_offset_hours, validate_region, validate_secret,
        validate_short, validate_slug, validate_title,
    };

    #[test]
    fn caret_moves_by_char_not_byte() {
        let mut e = FieldEdit::new(FieldId::PublishTitle, "aé☃b");
        e.end(false);
        assert_eq!(e.caret, "aé☃b".len());
        e.move_left(false);
        assert_eq!(e.caret, "aé☃".len());
        e.move_left(false);
        assert_eq!(&e.buffer[..e.caret], "aé");
        e.move_right(false);
        assert_eq!(&e.buffer[..e.caret], "aé☃");
        e.home(false);
        assert_eq!(e.caret, 0);
        // No-op at the edges.
        e.move_left(false);
        assert_eq!(e.caret, 0);
        e.end(false);
        e.move_right(false);
        assert_eq!(e.caret, e.buffer.len());
    }

    #[test]
    fn shift_selection_and_replace() {
        let mut e = FieldEdit::new(FieldId::PublishTitle, "hello");
        e.home(false);
        e.move_right(true);
        e.move_right(true);
        assert_eq!(e.selected_text(), Some("he"));
        e.insert("HE");
        assert_eq!(e.buffer, "HEllo");
        assert_eq!(e.selection(), None);
        e.select_all();
        assert_eq!(e.selected_text(), Some("HEllo"));
        e.backspace();
        assert_eq!(e.buffer, "");
    }

    #[test]
    fn backspace_delete_and_undo() {
        let mut e = FieldEdit::new(FieldId::PublishTitle, "abc");
        e.home(false);
        e.move_right(false);
        e.backspace();
        assert_eq!(e.buffer, "bc");
        e.undo();
        assert_eq!(e.buffer, "abc");
        e.end(false);
        e.move_left(false);
        e.delete_forward();
        assert_eq!(e.buffer, "ab");
        e.undo();
        assert_eq!(e.buffer, "abc");
        // Newlines never enter the buffer (paste of multiline text).
        e.end(false);
        e.insert("x\ny\n");
        assert_eq!(e.buffer, "abcxy");
    }

    #[test]
    fn validators_accept_and_reject() {
        assert!(validate_title(&"a".repeat(120)).is_ok());
        assert!(validate_title(&"a".repeat(121)).is_err());
        assert!(validate_slug("").is_ok());
        assert!(validate_slug("spring-shoot-2").is_ok());
        assert!(validate_slug("Spring").is_err());
        assert!(validate_slug("-ab").is_err());
        assert!(validate_slug("a--b").is_err());
        assert!(validate_endpoint("http://localhost:9000").is_ok());
        assert!(validate_endpoint("localhost:9000").is_err());
        assert!(validate_endpoint("").is_err());
        assert!(validate_bucket("laika-test").is_ok());
        assert!(validate_bucket("AB").is_err());
        assert!(validate_bucket("a/b").is_err());
        assert!(validate_key("laika").is_ok());
        assert!(validate_key("  ").is_err());
        assert!(validate_region("").is_ok());
        assert!(validate_region("eu-west-1").is_ok());
        assert!(validate_region("eu west").is_err());
        assert!(validate_secret("").is_ok());
        assert_eq!(validate_offset_hours("").unwrap(), 0);
        assert_eq!(validate_offset_hours("5.5").unwrap(), 330);
        assert_eq!(validate_offset_hours("-2").unwrap(), -120);
        assert!(validate_offset_hours("abc").is_err());
        assert!(validate_offset_hours("30").is_err());
        assert!(validate_short("Ada", "creator").is_ok());
        assert!(validate_short(&"a".repeat(201), "creator").is_err());
        assert!(validate_day("", "from").is_ok());
        assert!(validate_day("2026-06-14", "from").is_ok());
        assert!(validate_day("2026-6-14", "from").is_err());
        assert!(validate_day("tomorrow", "from").is_err());
    }

    #[test]
    fn param_numbers_parse_and_range() {
        // Exposure is index 2, range -5..5.
        assert_eq!(parse_param_number(2, "0.35").unwrap(), 0.35);
        assert_eq!(parse_param_number(2, "+0.35").unwrap(), 0.35);
        assert_eq!(parse_param_number(2, "\u{2212}1.5").unwrap(), -1.5);
        assert!(parse_param_number(2, "abc").is_err());
        let err = parse_param_number(2, "9").unwrap_err();
        assert!(err.contains("-5") && err.contains('5'), "{err}");
        // Temperature is index 0.
        assert_eq!(parse_param_number(0, "6500").unwrap(), 6500.);
        assert!(parse_param_number(0, "1000").is_err());
        assert_eq!(plain_param(0, 6500.4), "6500");
        assert_eq!(plain_param(2, 0.35), "0.35");
        assert_eq!(plain_param(2, 1.0), "1");
    }
}
