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
    // Git is already required for revision metadata and works identically on
    // Windows; unlike the Unix `date` utility it is present in Git for Windows.
    let date = Command::new("git")
        .args(["show", "-s", "--format=%cs", "HEAD"])
        .current_dir(&root)
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

    println!("cargo:rerun-if-changed=../../assets/icon/Laika.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let icon = root.join("assets/icon/Laika.ico");
        let out = Path::new(&std::env::var_os("OUT_DIR").expect("OUT_DIR")).join("laika.rc");
        let icon_path = icon.to_string_lossy().replace('\\', "/");
        std::fs::write(&out, format!("1 ICON \"{icon_path}\"\n"))
            .expect("write Windows resource file");
        embed_resource::compile(&out, embed_resource::NONE)
            .manifest_required()
            .expect("compile Laika Windows icon");
    }
}
