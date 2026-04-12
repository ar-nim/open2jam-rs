//! Modifiers panel — volume, speed, channel/visibility mods, difficulty selection.

use egui::{Slider, ComboBox, DragValue};
use open2jam_rs_core::{SpeedType, ChannelMod, VisibilityMod, Difficulty};
use open2jam_rs_core::game_options::GameOptions;

pub fn ui_modifiers(ui: &mut egui::Ui, opts: &mut GameOptions) {
    ui.separator();
    ui.label(egui::RichText::new("Modifiers").strong());

    // ── Volume Sliders ──
    ui.horizontal(|ui| {
        ui.label("Main Vol:");
        ui.add(Slider::new(&mut opts.master_volume, 0.0..=1.0).text(""));
    });
    ui.horizontal(|ui| {
        ui.label("Key Vol:");
        ui.add(Slider::new(&mut opts.key_volume, 0.0..=1.0).text(""));
    });
    ui.horizontal(|ui| {
        ui.label("BGM Vol:");
        ui.add(Slider::new(&mut opts.bgm_volume, 0.0..=1.0).text(""));
    });

    ui.separator();

    // ── Speed Controls ──
    ui.horizontal(|ui| {
        ui.label("Speed:");
        ComboBox::from_id_salt("speed_type")
            .selected_text(opts.speed_type.to_string())
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut opts.speed_type, SpeedType::HiSpeed, "Hi-Speed");
                ui.selectable_value(&mut opts.speed_type, SpeedType::XSpeed, "xR-Speed");
                ui.selectable_value(&mut opts.speed_type, SpeedType::WSpeed, "W-Speed");
                ui.selectable_value(&mut opts.speed_type, SpeedType::RegulSpeed, "Regul-Speed");
            });
        if opts.speed_type == SpeedType::HiSpeed {
            ui.add(DragValue::new(&mut opts.speed_multiplier).speed(0.1).clamp_range(0.5..=10.0));
        }
    });

    ui.separator();

    // ── Difficulty Radio Buttons ──
    ui.horizontal(|ui| {
        ui.label("Difficulty:");
        ui.radio_value(&mut opts.difficulty, Difficulty::Easy, "Easy");
        ui.radio_value(&mut opts.difficulty, Difficulty::Normal, "Normal");
        ui.radio_value(&mut opts.difficulty, Difficulty::Hard, "Hard");
    });

    ui.separator();

    // ── Channel Modifier ──
    ui.horizontal(|ui| {
        ui.label("Channel:");
        ComboBox::from_id_salt("channel_mod")
            .selected_text(opts.channel_modifier.to_string())
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut opts.channel_modifier, ChannelMod::None, "None");
                ui.selectable_value(&mut opts.channel_modifier, ChannelMod::Random, "Random");
                ui.selectable_value(&mut opts.channel_modifier, ChannelMod::Panic, "Panic");
                ui.selectable_value(&mut opts.channel_modifier, ChannelMod::Mirror, "Mirror");
            });
    });

    // ── Visibility Modifier ──
    ui.horizontal(|ui| {
        ui.label("Visibility:");
        ComboBox::from_id_salt("visibility_mod")
            .selected_text(opts.visibility_modifier.to_string())
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut opts.visibility_modifier, VisibilityMod::None, "None");
                ui.selectable_value(&mut opts.visibility_modifier, VisibilityMod::Hidden, "Hidden");
                ui.selectable_value(&mut opts.visibility_modifier, VisibilityMod::Sudden, "Sudden");
                ui.selectable_value(&mut opts.visibility_modifier, VisibilityMod::Dark, "Dark");
            });
    });

    ui.separator();

    // ── Checkboxes ──
    ui.checkbox(&mut opts.timed_judgment, "Use timed judgment");
    ui.checkbox(&mut opts.autosound, "AutoSound");
    ui.checkbox(&mut opts.autoplay, "Autoplay");
}
