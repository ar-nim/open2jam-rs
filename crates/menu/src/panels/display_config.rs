//! Display configuration panel — resolution, fullscreen, vsync, FPS limiter.

use egui::Slider;
use open2jam_rs_core::{FpsLimiter};
use open2jam_rs_core::game_options::GameOptions;

pub fn ui_display_config(ui: &mut egui::Ui, opts: &mut GameOptions) {
    ui.label(egui::RichText::new("Display Configuration").strong());

    // Resolution
    ui.horizontal(|ui| {
        ui.label("Width:");
        ui.add(egui::DragValue::new(&mut opts.display_width).clamp_range(640..=3840));
        ui.label("Height:");
        ui.add(egui::DragValue::new(&mut opts.display_height).clamp_range(480..=2160));
    });

    // Common presets
    ui.horizontal(|ui| {
        if ui.button("1280×720").clicked() { opts.display_width = 1280; opts.display_height = 720; }
        if ui.button("1920×1080").clicked() { opts.display_width = 1920; opts.display_height = 1080; }
        if ui.button("2560×1440").clicked() { opts.display_width = 2560; opts.display_height = 1440; }
    });

    ui.separator();

    // Fullscreen
    ui.checkbox(&mut opts.menu_fullscreen, "Menu Fullscreen");
    ui.checkbox(&mut opts.display_fullscreen, "Game Fullscreen");

    ui.separator();

    // VSync
    ui.checkbox(&mut opts.display_vsync, "Use VSync");

    // FPS limiter
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

    ui.separator();

    // Display Lag
    ui.horizontal(|ui| {
        ui.label("Display Lag (ms):");
        ui.add(Slider::new(&mut opts.display_lag, 0.0..=100.0).text(""));
        ui.label("Audio Latency (ms):");
        ui.add(Slider::new(&mut opts.audio_latency, 0.0..=500.0).text(""));
    });
}
