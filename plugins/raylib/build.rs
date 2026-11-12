//! 链接 raylib.
//!
//! 优先读取 `RAYLIB_DIR`, 否则在 `vendor/` 下按平台选择 raylib 目录.
//! 目录命名沿用 raylib 发布包, 如 `raylib-6.2_macos`.

use std::env;
use std::path::{Path, PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-env-changed=RAYLIB_DIR");
    let root = find_raylib_root();
    let lib = root.join("lib");
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    match target_os.as_str() {
        "macos" => link_macos(&lib),
        "linux" => link_unix(&lib, "$ORIGIN"),
        "windows" => link_windows(&lib),
        other => panic!("暂不支持的目标系统: {other}"),
    }
}

/// 按 `RAYLIB_DIR` 或 `vendor/` 下的平台目录定位 raylib.
fn find_raylib_root() -> PathBuf {
    if let Some(root) = env::var_os("RAYLIB_DIR") {
        let root = PathBuf::from(root);
        assert!(
            root.join("include/raylib.h").exists(),
            "RAYLIB_DIR 缺少 include/raylib.h: {}",
            root.display()
        );
        return root;
    }
    let vendor = Path::new(env!("CARGO_MANIFEST_DIR")).join("vendor");
    let os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let mut candidates = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&vendor) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() && path.join("include/raylib.h").exists() {
                candidates.push(path);
            }
        }
    }
    candidates.sort();
    assert!(
        !candidates.is_empty(),
        "未找到 raylib: 设置 RAYLIB_DIR, 或把 raylib 发布包放到 {}",
        vendor.display()
    );
    let matching: Vec<&PathBuf> = candidates
        .iter()
        .filter(|path| platform_name_matches(path, &os))
        .collect();
    let chosen = matching.last().copied().unwrap_or(&candidates[0]);
    if candidates.len() > 1 {
        println!(
            "cargo:warning=vendor/ 下存在多个 raylib 目录, 使用 {}",
            chosen.display()
        );
    }
    chosen.clone()
}

/// 目录名是否匹配目标平台.
fn platform_name_matches(path: &Path, os: &str) -> bool {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    let name = name.to_ascii_lowercase();
    match os {
        "macos" => name.contains("macos") || name.contains("osx") || name.contains("darwin"),
        "linux" => name.contains("linux"),
        "windows" => {
            name.contains("windows")
                || name.contains("win64")
                || name.contains("win32")
                || name.contains("mingw")
                || name.contains("msvc")
        },
        _ => true,
    }
}

fn link_macos(lib: &Path) {
    let archive = lib.join("libraylib.a");
    if archive.exists() {
        println!("cargo:rustc-link-arg={}", archive.display());
        for framework in [
            "OpenGL",
            "Cocoa",
            "IOKit",
            "CoreAudio",
            "CoreVideo",
            "QuartzCore",
        ] {
            println!("cargo:rustc-link-lib=framework={framework}");
        }
    } else {
        link_unix(lib, "@loader_path");
    }
}

fn link_unix(lib: &Path, origin: &str) {
    println!("cargo:rustc-link-search=native={}", lib.display());
    println!("cargo:rustc-link-lib=dylib=raylib");
    println!("cargo:rustc-link-arg=-Wl,-rpath,{origin}");
}

fn link_windows(lib: &Path) {
    println!("cargo:rustc-link-search=native={}", lib.display());
    println!("cargo:rustc-link-lib=dylib=raylib");
    for system in ["winmm", "gdi32", "opengl32", "shell32"] {
        println!("cargo:rustc-link-lib={system}");
    }
}
