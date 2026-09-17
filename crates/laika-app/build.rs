//! V32: build facts for the About window and crash reports — git
//! revision, build date, and the decoder/GPU crate versions from
//! Cargo.lock.

use std::path::Path;
use std::process::Command;

fn lock_version(lock: &str, name: &str) -> String {
    let needle = format!("name = \"{name}\"\n");
    lock.split("[[package]]")
        .find(|block| block.contains(&needle))
        .and_then(|block| {
            block.lines().find_map(|l| {
                l.strip_prefix("version = \"")
                    .map(|v| v.trim_end_matches('"').to_string())
            })
        })
        .unwrap_or_else(|| "unknown".to_string())
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let lock = std::fs::read_to_string(root.join("Cargo.lock")).unwrap_or_default();
    println!(
        "cargo:rustc-env=LAIKA_RAWLER_VERSION={}",
        lock_version(&lock, "rawler")
    );
    println!(
        "cargo:rustc-env=LAIKA_WGPU_VERSION={}",
        lock_version(&lock, "wgpu")
    );
    println!(
        "cargo:rustc-env=LAIKA_GPUI_VERSION={}",
        lock_version(&lock, "gpui-pre")
    );
    let git = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(&root)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let dirty = Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=no"])
        .current_dir(&root)
        .output()
        .ok()
        .is_some_and(|o| !o.stdout.is_empty());
    println!(
        "cargo:rustc-env=LAIKA_GIT_REV={git}{}",
        if dirty { "-dirty" } else { "" }
    );
    let date = Command::new("date")
        .args(["-u", "+%Y-%m-%d"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    println!("cargo:rustc-env=LAIKA_BUILD_DATE={date}");
    println!(
        "cargo:rustc-env=LAIKA_PROFILE={}",
        std::env::var("PROFILE").unwrap_or_default()
    );
    println!("cargo:rerun-if-changed=../../Cargo.lock");
    println!("cargo:rerun-if-changed=../../.git/HEAD");
    println!("cargo:rerun-if-changed=../../.git/index");
}
