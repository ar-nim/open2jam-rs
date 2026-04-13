//! Build script for the menu crate.
//!
//! Automatically downloads fonts at build time if they aren't already present
//! in the assets directory. This ensures the menu app has proper typography
//! with full CJK character support on first build with no manual setup.
//!
//! Fonts downloaded:
//! - **Inter** (Latin/Western text) — from GitHub releases
//! - **Noto Sans SC** (CJK fallback) — from GitHub releases

use std::path::PathBuf;

/// Inter v4.1 release download URL (contains InterVariable.ttf).
const INTER_URL: &str =
    "https://github.com/rsms/inter/releases/download/v4.1/Inter-4.1.zip";

/// Noto Sans SC release download URL.
const NOTO_SC_URL: &str =
    "https://github.com/notofonts/noto-cjk/releases/download/Sans2.004/08_NotoSansCJKsc.zip";

fn main() {
    let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    let assets_dir = manifest_dir.join("assets");

    // Download Inter if missing
    if !assets_dir.join("Inter-Regular.ttf").exists() {
        if let Err(e) = download_inter(&assets_dir) {
            println!("cargo:warning=Failed to download Inter font: {}", e);
        }
    }

    // Download Noto Sans SC if missing
    if !assets_dir.join("NotoSansSC-Regular.ttf").exists() {
        if let Err(e) = download_noto_sans_sc(&assets_dir) {
            println!("cargo:warning=Failed to download Noto Sans SC font: {}", e);
        }
    }
}

fn download_inter(assets_dir: &PathBuf) -> Result<(), Box<dyn std::error::Error>> {
    let tmp_dir = download_zip("inter", INTER_URL)?;
    // Find InterVariable.ttf anywhere in the extracted tree
    if let Some(font_path) = find_file(&tmp_dir, "InterVariable.ttf") {
        std::fs::create_dir_all(assets_dir).ok();
        let target = assets_dir.join("Inter-Regular.ttf");
        std::fs::copy(&font_path, &target)
            .map_err(|e| format!("Failed to copy font to {:?}: {}", target, e))?;
        println!("cargo:warning=Downloaded Inter font");
    } else {
        return Err("Could not find InterVariable.ttf in archive".into());
    }
    let _ = std::fs::remove_dir_all(&tmp_dir);
    Ok(())
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
        let target = assets_dir.join("NotoSansSC-Regular.ttf");
        std::fs::copy(&font_path, &target)
            .map_err(|e| format!("Failed to copy font to {:?}: {}", target, e))?;
        println!("cargo:warning=Downloaded Noto Sans SC font");
    } else {
        return Err("Could not find NotoSansCJKsc font in archive".into());
    }
    let _ = std::fs::remove_dir_all(&tmp_dir);
    Ok(())
}

fn download_zip(name: &str, url: &str) -> Result<PathBuf, Box<dyn std::error::Error>> {
    use std::process::Command;

    let tmp_dir = std::env::temp_dir().join(format!("open2jam-rs-font-{}-{}", name, std::process::id()));
    std::fs::create_dir_all(&tmp_dir).map_err(|e| format!("Failed to create temp dir {:?}: {}", tmp_dir, e))?;
    let zip_path = tmp_dir.join("font.zip");

    let _ = std::fs::remove_file(&zip_path);

    println!("cargo:warning=Downloading {}...", url);

    let status = Command::new("curl")
        .args(["-sL", "--connect-timeout", "30", "-o", zip_path.to_str().unwrap(), url])
        .status()
        .map_err(|e| format!("Failed to run curl: {}", e))?;
    if !status.success() {
        return Err(format!("curl download failed (exit: {:?})", status.code()).into());
    }

    let status = Command::new("unzip")
        .args(["-q", "-o", zip_path.to_str().unwrap(), "-d", tmp_dir.to_str().unwrap()])
        .status()
        .map_err(|e| format!("Failed to run unzip: {}", e))?;
    if !status.success() {
        return Err("unzip failed".into());
    }

    Ok(tmp_dir)
}

fn find_file(root: &PathBuf, filename: &str) -> Option<PathBuf> {
    for entry in walk_dir(root) {
        if entry.file_name().map(|n| n == filename).unwrap_or(false) {
            return Some(entry);
        }
    }
    None
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
