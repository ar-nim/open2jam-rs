//! Menu GUI for open2jam-rs — eframe (egui + winit + wgpu).
//!
//! Run with: `cargo run -p open2jam-rs --bin open2jam-rs-menu`
//! or: `cargo run --bin open2jam-rs-menu`
//!
//! **Fonts**: Inter (Latin) and Noto Sans SC (CJK) are automatically
//! downloaded at build time by `build.rs` if not already present in
//! `crates/game/assets/`. No manual setup required.

use anyhow::Result;

use open2jam_rs::menu::menu_app::MenuApp;

fn main() -> Result<()> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();
    log::info!("open2jam-rs-menu starting");

    let app = MenuApp::new()?;
    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("open2jam-rs — Music Select")
            .with_inner_size([928.0, 730.0]),
        ..Default::default()
    };
    eframe::run_native(
        "open2jam-rs — Music Select",
        native_options,
        Box::new(
            |cc| -> std::result::Result<
                Box<dyn eframe::App>,
                Box<dyn std::error::Error + Send + Sync>,
            > {
                configure_fonts(&cc.egui_ctx);
                Ok(Box::new(app))
            },
        ),
    )
    .map_err(|e| anyhow::anyhow!("eframe error: {:?}", e))?;
    Ok(())
}

/// Try loading a font file from the `assets/` directory.
/// Checks multiple locations to support both `cargo run` and deployed binaries.
fn load_bundled_font(filename: &str) -> Option<Vec<u8>> {
    use std::path::PathBuf;

    let candidates = [
        // Relative to crate root (for `cargo run`)
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join(filename),
        // Relative to current working directory
        PathBuf::from("assets").join(filename),
        // Relative to executable directory
        std::env::current_exe()
            .ok()?
            .parent()?
            .join("assets")
            .join(filename),
    ];

    for path in &candidates {
        if path.exists() {
            if let Ok(data) = std::fs::read(path) {
                log::info!("Loaded bundled font: {}", path.display());
                return Some(data);
            }
        }
    }
    None
}

/// Configure egui fonts with bundled Inter (Latin) and Noto Sans SC (CJK).
fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    if let Some(data) = load_bundled_font("Inter-Regular.ttf") {
        fonts.font_data.insert(
            "inter".to_string(),
            std::sync::Arc::new(egui::FontData::from_owned(data)),
        );
        let family = fonts
            .families
            .get_mut(&egui::FontFamily::Proportional)
            .unwrap();
        family.insert(0, "inter".to_string());
        log::info!("Inter font loaded as primary Latin font");
    } else {
        log::warn!(
            "Inter font not found — using egui default for Latin text. \
             The font will be automatically downloaded on next build."
        );
    }

    if let Some(data) = load_bundled_font("NotoSansSC-Regular.ttf") {
        fonts.font_data.insert(
            "noto-sans-sc".to_string(),
            std::sync::Arc::new(egui::FontData::from_owned(data)),
        );
        let family = fonts
            .families
            .get_mut(&egui::FontFamily::Proportional)
            .unwrap();
        family.push("noto-sans-sc".to_string());

        let mono_family = fonts
            .families
            .get_mut(&egui::FontFamily::Monospace)
            .unwrap();
        mono_family.push("noto-sans-sc".to_string());

        log::info!("Noto Sans SC loaded as CJK fallback");
    } else {
        log::warn!(
            "Noto Sans SC not found — CJK characters may not render correctly. \
             The font will be automatically downloaded on next build."
        );
    }

    ctx.set_fonts(fonts);
}
