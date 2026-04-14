//! Display configuration panel — resolution, fullscreen, vsync, FPS limiter.

use egui::Slider;
use open2jam_rs_core::game_options::GameOptions;
use open2jam_rs_core::game_options::{FpsLimiter, VSyncMode};

/// Common preset resolutions (16:9 and 4:3).
const PRESET_RESOLUTIONS: &[(u32, u32)] = &[
    // 4:3
    (640, 480),
    (800, 600),
    (1024, 768),
    (1280, 960),
    (1400, 1050),
    // 16:9
    (1280, 720),
    (1366, 768),
    (1600, 900),
    (1920, 1080),
    (2560, 1440),
    (3840, 2160),
];

pub fn ui_display_config(
    ui: &mut egui::Ui,
    opts: &mut GameOptions,
    native_res: Option<(u32, u32)>,
) {
    ui.label(egui::RichText::new("Display Configuration").strong());

    // ── Resolution ──
    ui.label(egui::RichText::new("Resolution").strong().size(13.0));

    // Build resolution list: presets filtered by native max, with native marked
    let max_h = native_res.map(|(_, nh)| nh).unwrap_or(2160);
    let max_w = native_res.map(|(nw, _)| nw).unwrap_or(3840);

    let mut presets: Vec<(u32, u32)> = PRESET_RESOLUTIONS
        .iter()
        .filter(|(w, h)| *w <= max_w && *h <= max_h)
        .copied()
        .collect();

    // If native resolution isn't already in the list, insert it at the top
    if let Some((nw, nh)) = native_res {
        if !presets.iter().any(|(w, h)| *w == nw && *h == nh) {
            presets.insert(0, (nw, nh));
        }
    }

    // Sort descending by height so native (highest) appears first
    presets.sort_by(|a, b| b.1.cmp(&a.1).then(b.0.cmp(&a.0)));

    // Custom resolution checkbox
    ui.checkbox(&mut opts.use_custom_resolution, "Use Custom Resolution");

    // Preset dropdown — disabled when custom is active
    let preset_enabled = !opts.use_custom_resolution;
    ui.add_enabled_ui(preset_enabled, |ui| {
        let current_text = format!("{}×{}", opts.display_width, opts.display_height);
        egui::ComboBox::from_id_salt("resolution_select")
            .selected_text(&current_text)
            .show_ui(ui, |ui| {
                for (w, h) in &presets {
                    let is_native = native_res == Some((*w, *h));
                    let label = if is_native {
                        format!("{}×{} (native)", w, h)
                    } else {
                        format!("{}×{}", w, h)
                    };
                    let is_selected = opts.display_width == *w && opts.display_height == *h;
                    if ui.selectable_label(is_selected, label).clicked() {
                        opts.display_width = *w;
                        opts.display_height = *h;
                    }
                }
            });
    });

    // Custom resolution fields — only active when checkbox is checked
    ui.add_enabled_ui(opts.use_custom_resolution, |ui| {
        ui.horizontal(|ui| {
            ui.label("Width:");
            ui.add(egui::DragValue::new(&mut opts.custom_width).range(640..=3840));
            ui.label("Height:");
            ui.add(egui::DragValue::new(&mut opts.custom_height).range(480..=2160));
        });
    });

    ui.separator();

    // ── Display Mode (horizontal with separator) ──
    ui.horizontal(|ui| {
        ui.checkbox(&mut opts.menu_fullscreen, "Menu Fullscreen");
        ui.separator();
        ui.checkbox(&mut opts.display_fullscreen, "Game Fullscreen");
    });

    ui.separator();

    // ── Sync & Limiting ──
    ui.horizontal(|ui| {
        ui.label("VSync:");
        egui::ComboBox::from_id_salt("vsync_mode")
            .selected_text(opts.vsync_mode.to_string())
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut opts.vsync_mode, VSyncMode::On, "On");
                ui.selectable_value(&mut opts.vsync_mode, VSyncMode::Fast, "Fast");
                ui.selectable_value(&mut opts.vsync_mode, VSyncMode::Off, "Off");
            });
    });

    // FPS limiter — greyed out when VSync is On
    let fps_limiter_enabled = opts.vsync_mode != VSyncMode::On;
    ui.add_enabled_ui(fps_limiter_enabled, |ui| {
        ui.horizontal(|ui| {
            ui.label("FPS Limiter:");
            egui::ComboBox::from_id_salt("fps_limiter")
                .selected_text(opts.fps_limiter.to_string())
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut opts.fps_limiter, FpsLimiter::Unlimited, "Unlimited");
                    ui.selectable_value(&mut opts.fps_limiter, FpsLimiter::X1, "1x Refresh Rate");
                    ui.selectable_value(&mut opts.fps_limiter, FpsLimiter::X2, "2x");
                    ui.selectable_value(&mut opts.fps_limiter, FpsLimiter::X4, "4x");
                    ui.selectable_value(&mut opts.fps_limiter, FpsLimiter::X8, "8x");
                });
        });
    });

    ui.separator();

    // ── Latency ──
    ui.horizontal(|ui| {
        ui.label("Display Lag (ms):");
        ui.add(Slider::new(&mut opts.display_lag, 0.0..=100.0).text(""));
        ui.label("Audio Latency (ms):");
        ui.add(Slider::new(&mut opts.audio_latency, 0.0..=500.0).text(""));
    });
}
