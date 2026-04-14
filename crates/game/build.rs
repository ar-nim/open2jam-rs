//! Build script for the game crate.
//!
//! Automatically downloads Noto Sans CJK SC at build time if not already
//! present in `crates/game/assets/`. This font covers both Latin and
//! CJK characters, providing consistent typography everywhere.

use std::path::PathBuf;

/// Noto Sans SC release download URL.
const NOTO_SC_URL: &str =
    "https://github.com/notofonts/noto-cjk/releases/download/Sans2.004/08_NotoSansCJKsc.zip";

fn main() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let assets_dir = manifest_dir.join("assets");

    // Download Noto Sans SC if missing
    if !assets_dir.join("NotoSansCJKsc-Regular.otf").exists() {
        if let Err(e) = download_noto_sans_sc(&assets_dir) {
            println!("cargo:warning=Failed to download Noto Sans CJK SC font: {}", e);
        }
    }
}

fn download_noto_sans_sc(assets_dir: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let tmp_dir = download_zip("noto-sc", NOTO_SC_URL)?;
    // Find NotoSansCJKsc*.otf (not Subset or VF variants)
    let font_path = find_file_matching(&tmp_dir, |name| {
        name.starts_with("NotoSansCJKsc")
            && name.ends_with(".otf")
            && !name.contains("Subset")
            && !name.contains("VF")
    });
    if let Some(font_path) = font_path {
        std::fs::create_dir_all(assets_dir).ok();
        let target = assets_dir.join("NotoSansCJKsc-Regular.otf");
        std::fs::copy(&font_path, &target)
            .map_err(|e| format!("Failed to copy font to {:?}: {}", target, e))?;
        println!("cargo:warning=Downloaded Noto Sans CJK SC font");
    } else {
        return Err("Could not find NotoSansCJKsc font in archive".into());
    }
    let _ = std::fs::remove_dir_all(&tmp_dir);
    Ok(())
}

fn download_zip(name: &str, url: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    use std::process::Command;

    let tmp_dir =
        std::env::temp_dir().join(format!("open2jam-rs-font-{}-{}", name, std::process::id()));
    std::fs::create_dir_all(&tmp_dir)
        .map_err(|e| format!("Failed to create temp dir {:?}: {}", tmp_dir, e))?;
    let zip_path = tmp_dir.join("font.zip");

    let _ = std::fs::remove_file(&zip_path);

    println!("cargo:warning=Downloading {}...", url);

    let status = Command::new("curl")
        .args([
            "-sL",
            "--connect-timeout",
            "30",
            "-o",
            zip_path.to_str().unwrap(),
            url,
        ])
        .status()
        .map_err(|e| format!("Failed to run curl: {}", e))?;
    if !status.success() {
        return Err(format!("curl download failed (exit: {:?})", status.code()).into());
    }

    let status = Command::new("unzip")
        .args([
            "-q",
            "-o",
            zip_path.to_str().unwrap(),
            "-d",
            tmp_dir.to_str().unwrap(),
        ])
        .status()
        .map_err(|e| format!("Failed to run unzip: {}", e))?;
    if !status.success() {
        return Err("unzip failed".into());
    }

    Ok(tmp_dir)
}

fn find_file_matching<P: Fn(&str) -> bool>(root: &PathBuf, predicate: P) -> Option<PathBuf> {
    for entry in walk_dir(root) {
        let name = entry.file_name()?.to_string_lossy();
        if predicate(&name) {
            return Some(entry);
        }
    }
    None
}

fn walk_dir(root: &PathBuf) -> Vec<PathBuf> {
    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                files.push(path);
            } else if path.is_dir() {
                files.extend(walk_dir(&path));
            }
        }
    }
    files
}
