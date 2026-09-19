//! Small OS integration layer shared by the app and core.
//!
//! Keeping environment lookup and "open with the default application" here
//! prevents Windows support from becoming a scattering of `HOME`, `open`, and
//! `/Volumes` assumptions.

use std::path::{Path, PathBuf};

pub fn home_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .or_else(|| {
                let drive = std::env::var_os("HOMEDRIVE")?;
                let path = std::env::var_os("HOMEPATH")?;
                Some(PathBuf::from(drive).join(path))
            })
            .unwrap_or_else(|| PathBuf::from("."))
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
    }
}

pub fn pictures_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::ffi::OsStringExt;
        use windows_sys::Win32::System::Com::CoTaskMemFree;
        use windows_sys::Win32::UI::Shell::{FOLDERID_Pictures, SHGetKnownFolderPath};

        let mut raw = std::ptr::null_mut();
        // SAFETY: SHGetKnownFolderPath allocates a NUL-terminated UTF-16
        // string for the caller; it is copied before CoTaskMemFree.
        let hr =
            unsafe { SHGetKnownFolderPath(&FOLDERID_Pictures, 0, std::ptr::null_mut(), &mut raw) };
        if hr >= 0 && !raw.is_null() {
            let mut len = 0usize;
            // SAFETY: a successful call returned a NUL-terminated string.
            unsafe {
                while *raw.add(len) != 0 {
                    len += 1;
                }
                let path = PathBuf::from(std::ffi::OsString::from_wide(
                    std::slice::from_raw_parts(raw, len),
                ));
                CoTaskMemFree(raw.cast());
                return path;
            }
        }
    }
    home_dir().join("Pictures")
}

/// Per-user durable application data (catalog, preferences, logs).
pub fn data_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("LOCALAPPDATA")
            .or_else(|| std::env::var_os("APPDATA"))
            .map(PathBuf::from)
            .unwrap_or_else(|| home_dir().join("AppData").join("Local"))
            .join("Laika")
    }
    #[cfg(target_os = "macos")]
    {
        home_dir().join("Library/Application Support/Laika")
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        // Preserve Laika's historic Linux catalog location.
        std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home_dir().join(".cache"))
            .join("laika")
    }
}

pub fn cache_dir() -> PathBuf {
    #[cfg(target_os = "windows")]
    {
        std::env::var_os("LOCALAPPDATA")
            .map(PathBuf::from)
            .unwrap_or_else(|| home_dir().join("AppData").join("Local"))
            .join("Laika")
            .join("cache")
    }
    #[cfg(target_os = "macos")]
    {
        data_dir().join("cache")
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        data_dir().join("cache")
    }
}

/// Resource directory beside the packaged executable/app bundle.
pub fn resource_dir() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    #[cfg(target_os = "macos")]
    return Some(exe.parent()?.parent()?.join("Resources"));
    #[cfg(not(target_os = "macos"))]
    return Some(exe.parent()?.join("Resources"));
}

fn status(result: std::io::Result<std::process::ExitStatus>, action: &str) -> Result<(), String> {
    result.map_err(|e| format!("{action}: {e}")).and_then(|s| {
        s.success()
            .then_some(())
            .ok_or_else(|| format!("{action} failed"))
    })
}

#[cfg(target_os = "windows")]
fn shell_open(target: &std::ffi::OsStr) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::ShellExecuteW;
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let verb: Vec<u16> = "open\0".encode_utf16().collect();
    let mut target: Vec<u16> = target.encode_wide().collect();
    target.push(0);
    // SAFETY: all strings are NUL-terminated UTF-16 and ShellExecuteW does
    // not retain the pointers after returning.
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            target.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        )
    };
    ((result as isize) > 32).then_some(()).ok_or_else(|| {
        format!(
            "Windows could not open the target (ShellExecute code {})",
            result as isize
        )
    })
}

/// Open a file or directory with the OS default handler.
pub fn open_path(path: &Path) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    return status(
        std::process::Command::new("open").arg(path).status(),
        "open",
    );
    #[cfg(target_os = "linux")]
    return status(
        std::process::Command::new("xdg-open").arg(path).status(),
        "open (is xdg-open installed?)",
    );
    #[cfg(target_os = "windows")]
    return shell_open(path.as_os_str());
    #[allow(unreachable_code)]
    Err("open is not supported on this platform".to_string())
}

/// Open a URL with the OS default browser.
pub fn open_url(url: &str) -> Result<(), String> {
    #[cfg(target_os = "windows")]
    return shell_open(std::ffi::OsStr::new(url));
    #[cfg(not(target_os = "windows"))]
    return open_path(Path::new(url));
    #[allow(unreachable_code)]
    Err("browser launch is not supported on this platform".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_directories_are_absolute_or_useful_fallbacks() {
        assert!(!home_dir().as_os_str().is_empty());
        assert!(
            data_dir().ends_with(if cfg!(target_os = "windows") {
                "Laika"
            } else {
                "laika"
            }) || cfg!(target_os = "macos")
        );
        assert!(cache_dir().ends_with("cache") || cache_dir().ends_with("laika"));
    }
}
