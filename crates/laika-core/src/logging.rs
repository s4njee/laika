//! V32: diagnostics logging. Every existing `eprintln!("[area] …")` line is
//! captured by teeing stderr into a timestamped, rotating log file in the
//! app support directory (still echoed to the terminal). Lines are
//! scrubbed of credentials before they reach disk. An opt-in panic hook
//! writes a crash report beside the log.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

/// Rotate when the live log passes this size.
pub const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;
/// Rotated files kept (`laika.1.log` … `laika.N.log`).
pub const KEEP_ROTATED: usize = 4;
pub const LOG_FILE: &str = "laika.log";

static LOG_DIR: OnceLock<PathBuf> = OnceLock::new();
static SECRETS: Mutex<Vec<String>> = Mutex::new(Vec::new());
static CRASH_REPORTS: AtomicBool = AtomicBool::new(false);
static CRASH_CONTEXT: Mutex<String> = Mutex::new(String::new());

/// `<app support>/logs`.
pub fn log_dir_for(app_base: &Path) -> PathBuf {
    app_base.join("logs")
}

/// The live log file, once `init` ran.
pub fn log_path() -> Option<PathBuf> {
    LOG_DIR.get().map(|d| d.join(LOG_FILE))
}

pub fn log_dir() -> Option<PathBuf> {
    LOG_DIR.get().cloned()
}

/// Values that must never appear in a log (e.g. a keychain secret once
/// loaded). Short strings are ignored so common words are never masked.
pub fn register_secret(s: &str) {
    let s = s.trim();
    if s.len() < 6 {
        return;
    }
    if let Ok(mut v) = SECRETS.lock() {
        if !v.iter().any(|x| x == s) {
            v.push(s.to_string());
        }
    }
}

/// Opt-in crash reports (Preferences → General).
pub fn set_crash_reports(on: bool) {
    CRASH_REPORTS.store(on, Ordering::Relaxed);
}

/// Diagnostics text (version, GPU, catalog) included in a crash report.
pub fn set_crash_context(text: &str) {
    if let Ok(mut c) = CRASH_CONTEXT.lock() {
        *c = redact(text);
    }
}

/// Remove credentials from a log line: registered secrets, URL userinfo,
/// `key=value`/`key: value` pairs for secret-like keys, AWS access key ids.
pub fn redact(line: &str) -> String {
    let mut out = line.to_string();
    if let Ok(secrets) = SECRETS.lock() {
        for s in secrets.iter() {
            if out.contains(s.as_str()) {
                out = out.replace(s.as_str(), "[redacted]");
            }
        }
    }
    // scheme://user:pass@host → scheme://[redacted]@host
    let mut search = 0;
    while let Some(i) = out[search..].find("://") {
        let start = search + i + 3;
        let rest = &out[start..];
        let end = rest
            .find(|c: char| c.is_whitespace() || c == '/' || c == '"' || c == '\'')
            .unwrap_or(rest.len());
        if let Some(at) = rest[..end].rfind('@') {
            if rest[..at].contains(':') {
                out.replace_range(start..start + at, "[redacted]");
            }
        }
        search = start.min(out.len());
    }
    // secret-like keys followed by = or :
    const KEYS: [&str; 9] = [
        "secret",
        "password",
        "passwd",
        "token",
        "authorization",
        "api_key",
        "apikey",
        "access_key",
        "credential",
    ];
    let lower = out.to_ascii_lowercase();
    let mut spans: Vec<(usize, usize)> = Vec::new();
    for key in KEYS {
        let mut from = 0;
        while let Some(i) = lower[from..].find(key) {
            let mut j = from + i + key.len();
            // allow the rest of an identifier (secret_key, tokens) and quotes
            while j < lower.len()
                && (lower.as_bytes()[j].is_ascii_alphanumeric() || lower.as_bytes()[j] == b'_')
            {
                j += 1;
            }
            let bytes = lower.as_bytes();
            let mut k = j;
            while k < bytes.len() && (bytes[k] == b' ' || bytes[k] == b'"' || bytes[k] == b'\'') {
                k += 1;
            }
            if k < bytes.len() && (bytes[k] == b'=' || bytes[k] == b':') {
                k += 1;
                while k < bytes.len() && (bytes[k] == b' ' || bytes[k] == b'"' || bytes[k] == b'\'')
                {
                    k += 1;
                }
                // "Bearer xyz" keeps its scheme word masked too
                let v_start = k;
                let mut v_end = k;
                while v_end < bytes.len()
                    && !matches!(
                        bytes[v_end],
                        b' ' | b',' | b';' | b'"' | b'\'' | b'&' | b')' | b'}'
                    )
                {
                    v_end += 1;
                }
                if lower[v_start..v_end].eq_ignore_ascii_case("bearer")
                    || lower[v_start..v_end].eq_ignore_ascii_case("basic")
                {
                    v_end += 1;
                    while v_end < bytes.len()
                        && !matches!(bytes[v_end], b' ' | b',' | b';' | b'"' | b'\'' | b'&')
                    {
                        v_end += 1;
                    }
                }
                if v_end > v_start {
                    spans.push((v_start, v_end));
                }
            }
            from = j;
        }
    }
    // AWS access key ids: AKIA/ASIA + 16 upper-alphanumerics.
    let bytes = out.as_bytes();
    let mut i = 0;
    while i + 20 <= bytes.len() {
        let w = &bytes[i..i + 4];
        if (w == b"AKIA" || w == b"ASIA")
            && bytes[i + 4..i + 20]
                .iter()
                .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
        {
            spans.push((i, i + 20));
            i += 20;
        } else {
            i += 1;
        }
    }
    spans.sort_unstable();
    for (s, e) in spans.into_iter().rev() {
        if e <= out.len() && out.is_char_boundary(s) && out.is_char_boundary(e) {
            out.replace_range(s..e, "[redacted]");
        }
    }
    out
}

/// UTC timestamp like `2026-09-17T06:40:12.345Z` (no chrono dependency).
pub fn timestamp(unix_ms: u128) -> String {
    let secs = (unix_ms / 1000) as i64;
    let ms = (unix_ms % 1000) as u32;
    let days = secs.div_euclid(86_400);
    let tod = secs.rem_euclid(86_400);
    // Civil-from-days (Howard Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{ms:03}Z",
        tod / 3600,
        (tod / 60) % 60,
        tod % 60
    )
}

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

/// Shift `laika.log` → `laika.1.log` → … dropping the oldest.
pub fn rotate(dir: &Path) {
    let name = |n: usize| {
        if n == 0 {
            dir.join(LOG_FILE)
        } else {
            dir.join(format!("laika.{n}.log"))
        }
    };
    std::fs::remove_file(name(KEEP_ROTATED)).ok();
    for n in (0..KEEP_ROTATED).rev() {
        let from = name(n);
        if from.exists() {
            std::fs::rename(&from, name(n + 1)).ok();
        }
    }
}

/// Appends lines to the live log, rotating by size.
struct LogWriter {
    dir: PathBuf,
    file: Option<std::fs::File>,
    size: u64,
}

impl LogWriter {
    fn open(dir: &Path) -> Self {
        let path = dir.join(LOG_FILE);
        let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        let mut w = Self {
            dir: dir.to_path_buf(),
            file: None,
            size,
        };
        if size > MAX_LOG_BYTES {
            rotate(dir);
            w.size = 0;
        }
        w.file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join(LOG_FILE))
            .ok();
        w
    }

    fn line(&mut self, text: &str) {
        let entry = format!("{} {}\n", timestamp(now_ms()), redact(text));
        if self.size + entry.len() as u64 > MAX_LOG_BYTES {
            self.file = None;
            rotate(&self.dir);
            self.size = 0;
            self.file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(self.dir.join(LOG_FILE))
                .ok();
        }
        if let Some(f) = self.file.as_mut() {
            if f.write_all(entry.as_bytes()).is_ok() {
                self.size += entry.len() as u64;
            }
        }
    }
}

/// Start logging into `dir` (created). On Unix stderr is redirected
/// through a pipe: a thread echoes each line to the original stderr and
/// appends it (timestamped, redacted) to the log. `LAIKA_LOG=0` disables.
/// Safe to call once; later calls are no-ops.
pub fn init(dir: &Path, version: &str) -> Result<PathBuf, String> {
    if LOG_DIR.get().is_some() {
        return Ok(dir.join(LOG_FILE));
    }
    std::fs::create_dir_all(dir).map_err(|e| format!("log folder: {e}"))?;
    LOG_DIR.set(dir.to_path_buf()).ok();
    let mut writer = LogWriter::open(dir);
    writer.line(&format!(
        "[laika] ---- start Laika {version} · {} {} · pid {}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        std::process::id()
    ));
    if std::env::var("LAIKA_LOG").is_ok_and(|v| v == "0") {
        return Ok(dir.join(LOG_FILE));
    }
    #[cfg(unix)]
    {
        use std::os::fd::FromRawFd;
        let mut fds = [0i32; 2];
        // SAFETY: plain POSIX fd calls; failures fall back to no capture.
        unsafe {
            if libc::pipe(fds.as_mut_ptr()) != 0 {
                return Ok(dir.join(LOG_FILE));
            }
            let orig = libc::dup(2);
            if orig < 0 || libc::dup2(fds[1], 2) < 0 {
                libc::close(fds[0]);
                libc::close(fds[1]);
                return Ok(dir.join(LOG_FILE));
            }
            libc::close(fds[1]);
            let reader = std::fs::File::from_raw_fd(fds[0]);
            let mut echo = std::fs::File::from_raw_fd(orig);
            std::thread::Builder::new()
                .name("laika-log".into())
                .spawn(move || {
                    let mut lines = std::io::BufReader::new(reader);
                    let mut buf = Vec::new();
                    loop {
                        buf.clear();
                        match lines.read_until(b'\n', &mut buf) {
                            Ok(0) | Err(_) => break,
                            Ok(_) => {
                                echo.write_all(&buf).ok();
                                let text = String::from_utf8_lossy(&buf);
                                writer.line(text.trim_end_matches(['\n', '\r']));
                            }
                        }
                    }
                })
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(dir.join(LOG_FILE))
}

/// The last `n` lines of the live log (redacted already on write).
pub fn tail(n: usize) -> Vec<String> {
    let Some(path) = log_path() else {
        return Vec::new();
    };
    let Ok(file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let lines: Vec<String> = std::io::BufReader::new(file)
        .lines()
        .map_while(Result::ok)
        .collect();
    lines[lines.len().saturating_sub(n)..].to_vec()
}

/// Crash reports written so far, newest first.
pub fn crash_reports() -> Vec<PathBuf> {
    let Some(dir) = log_dir() else {
        return Vec::new();
    };
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|r| {
            r.flatten()
                .map(|e| e.path())
                .filter(|p| {
                    p.file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.starts_with("crash-") && n.ends_with(".txt"))
                })
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v.reverse();
    v
}

/// Build the text of a crash report (pure; tested).
pub fn crash_report_text(
    message: &str,
    location: &str,
    backtrace: &str,
    context: &str,
    recent: &[String],
) -> String {
    let mut s = String::new();
    s.push_str("Laika crash report\n");
    s.push_str("Attach this file to an issue. It contains no photos and no credentials.\n\n");
    s.push_str(&format!("Time: {}\n", timestamp(now_ms())));
    s.push_str(&format!("Panic: {}\n", redact(message)));
    s.push_str(&format!("At: {location}\n\n"));
    if !context.is_empty() {
        s.push_str("Diagnostics:\n");
        s.push_str(context);
        s.push_str("\n\n");
    }
    s.push_str("Backtrace:\n");
    s.push_str(backtrace);
    s.push_str("\n\nRecent log:\n");
    for l in recent {
        s.push_str(l);
        s.push('\n');
    }
    s
}

/// Log every panic; with crash reports on, also write `crash-<time>.txt`.
pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "panic".to_string());
        let location = info
            .location()
            .map(|l| format!("{}:{}", l.file(), l.line()))
            .unwrap_or_default();
        let thread = std::thread::current()
            .name()
            .unwrap_or("unnamed")
            .to_string();
        eprintln!("[panic] thread '{thread}' at {location}: {message}");
        if CRASH_REPORTS.load(Ordering::Relaxed) {
            if let Some(dir) = log_dir() {
                let bt = std::backtrace::Backtrace::force_capture().to_string();
                let context = CRASH_CONTEXT.lock().map(|c| c.clone()).unwrap_or_default();
                let text = crash_report_text(&message, &location, &bt, &context, &tail(200));
                let stamp = timestamp(now_ms()).replace([':', '.'], "-");
                let path = dir.join(format!("crash-{stamp}.txt"));
                if std::fs::write(&path, text).is_ok() {
                    eprintln!("[panic] crash report written to {}", path.display());
                }
            }
        }
        previous(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn redacts_credentials() {
        register_secret("wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY");
        let line = "[sync] put failed secret=wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY key AKIAIOSFODNN7EXAMPLE";
        let r = redact(line);
        assert!(!r.contains("wJalrXUtnFEMI"), "{r}");
        assert!(!r.contains("AKIAIOSFODNN7EXAMPLE"), "{r}");
        assert!(r.starts_with("[sync] put failed"));
        let r = redact("https://alice:hunter22@minio.local:9000/bucket failed");
        assert_eq!(r, "https://[redacted]@minio.local:9000/bucket failed");
        let r = redact(r#"{"password": "pa55word", "user": "bob"} Authorization: Bearer abc.def"#);
        assert!(!r.contains("pa55word") && !r.contains("abc.def"), "{r}");
        assert!(r.contains("bob"));
        // Ordinary lines pass untouched.
        let plain = "[import] FAILED IMG_5442.NEF: decode failed: truncated file";
        assert_eq!(redact(plain), plain);
        assert_eq!(
            redact("[laika] token count 3 · secrets manager opened"),
            "[laika] token count 3 · secrets manager opened"
        );
    }

    #[test]
    fn timestamps_and_rotation() {
        assert_eq!(timestamp(0), "1970-01-01T00:00:00.000Z");
        assert_eq!(timestamp(1_789_630_812_345), "2026-09-17T07:40:12.345Z");
        assert_eq!(timestamp(951_782_400_000), "2000-02-29T00:00:00.000Z");
        let dir = std::env::temp_dir().join(format!("laika-log-{}", std::process::id()));
        std::fs::remove_dir_all(&dir).ok();
        std::fs::create_dir_all(&dir).unwrap();
        for n in 0..(KEEP_ROTATED + 3) {
            std::fs::write(dir.join(LOG_FILE), format!("gen {n}")).unwrap();
            rotate(&dir);
        }
        assert!(!dir.join(LOG_FILE).exists());
        assert_eq!(
            std::fs::read_to_string(dir.join("laika.1.log")).unwrap(),
            format!("gen {}", KEEP_ROTATED + 2)
        );
        assert!(dir.join(format!("laika.{KEEP_ROTATED}.log")).exists());
        assert!(!dir.join(format!("laika.{}.log", KEEP_ROTATED + 1)).exists());
        // The writer rotates when a line would pass the cap.
        std::fs::write(dir.join(LOG_FILE), vec![b'x'; MAX_LOG_BYTES as usize]).unwrap();
        let mut w = LogWriter::open(&dir);
        w.line("[test] after rotation secret=abcdefgh");
        let live = std::fs::read_to_string(dir.join(LOG_FILE)).unwrap();
        assert!(
            live.ends_with("[test] after rotation secret=[redacted]\n"),
            "{live}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn crash_report_has_the_parts() {
        let t = crash_report_text(
            "index out of bounds",
            "src/main.rs:10",
            "0: main",
            "Laika 0.1.0\nGPU: M3",
            &["a".into(), "b".into()],
        );
        for part in [
            "Panic: index out of bounds",
            "At: src/main.rs:10",
            "GPU: M3",
            "Backtrace:\n0: main",
            "Recent log:\na\nb\n",
        ] {
            assert!(t.contains(part), "{part}");
        }
    }
}
