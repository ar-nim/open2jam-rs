//! Display configuration panel — resolution, fullscreen, vsync, FPS limiter.

use egui::Slider;
use open2jam_rs_core::game_options::GameOptions;
use open2jam_rs_core::FpsLimiter;

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

/// Common refresh rates (in Hz).
const COMMON_REFRESH_RATES: &[u32] = &[60, 75, 120, 144, 165, 240];

pub fn ui_display_config(
    ui: &mut egui::Ui,
    opts: &mut GameOptions,
    native_res: Option<(u32, u32)>,
) {
    ui.label(egui::RichText::new("Display Configuration").strong());

    // ── Resolution ──
    ui.label(egui::RichText::new("Resolution").strong().size(13.0));

    // Show native resolution prominently
    if let Some((w, h)) = native_res {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Native:").weak());
            ui.label(format!("{}×{}", w, h));
        });
        ui.add_space(4.0);
    }

    // Resolution selector: common presets filtered by native height
    let available_resolutions: Vec<(u32, u32)> = {
        let max_h = native_res.map(|(_, nh)| nh).unwrap_or(2160);
        let max_w = native_res.map(|(nw, _)| nw).unwrap_or(3840);
        PRESET_RESOLUTIONS
            .iter()
            .filter(|(w, h)| *w <= max_w && *h <= max_h)
            .copied()
            .collect()
    };

    // Current resolution picker
    let current_text = format!("{}×{}", opts.display_width, opts.display_height);
    egui::ComboBox::from_id_salt("resolution_select")
        .selected_text(&current_text)
        .show_ui(ui, |ui| {
            for (w, h) in &available_resolutions {
                let text = format!("{}×{}", w, h);
                if ui
                    .selectable_value(
                        &mut (opts.display_width, opts.display_height),
                        (*w, *h),
                        text,
                    )
                    .clicked()
                {
                    // Selection updated
                }
            }
        });

    // Manual override with drag values (collapsed by default)
    ui.collapsing("Custom resolution", |ui| {
        ui.horizontal(|ui| {
            ui.label("Width:");
            ui.add(egui::DragValue::new(&mut opts.display_width).range(640..=3840));
            ui.label("Height:");
            ui.add(egui::DragValue::new(&mut opts.display_height).range(480..=2160));
        });
    });

    ui.separator();

    // ── Refresh Rate ──
    ui.label(egui::RichText::new("Refresh Rate").strong().size(13.0));
    ui.horizontal(|ui| {
        for &rate in COMMON_REFRESH_RATES {
            let text = format!("{} Hz", rate);
            let is_selected = opts.display_refresh_rate == rate;
            if ui.selectable_label(is_selected, text).clicked() {
                opts.display_refresh_rate = rate;
            }
        }
    });

    ui.separator();

    // ── Display Mode ──
    ui.checkbox(&mut opts.menu_fullscreen, "Menu Fullscreen");
    ui.checkbox(&mut opts.display_fullscreen, "Game Fullscreen");

    ui.separator();

    // ── Sync & Limiting ──
    ui.checkbox(&mut opts.display_vsync, "Use VSync");

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

    // ── Latency ──
    ui.horizontal(|ui| {
        ui.label("Display Lag (ms):");
        ui.add(Slider::new(&mut opts.display_lag, 0.0..=100.0).text(""));
        ui.label("Audio Latency (ms):");
        ui.add(Slider::new(&mut opts.audio_latency, 0.0..=500.0).text(""));
    });
}
