//! Key binding editor panel — edit key maps for K4-K8 layouts.

use open2jam_rs_core::key_bindings::{KeyBindings, KeyboardLayout};
use open2jam_rs_core::Config;

const LAYOUTS: &[(&str, fn(&KeyBindings) -> &KeyboardLayout, fn(&mut KeyBindings) -> &mut KeyboardLayout)] = &[
    ("4 Keys", |kb| &kb.k4, |kb| &mut kb.k4),
    ("5 Keys", |kb| &kb.k5, |kb| &mut kb.k5),
    ("6 Keys", |kb| &kb.k6, |kb| &mut kb.k6),
    ("7 Keys", |kb| &kb.k7, |kb| &mut kb.k7),
    ("8 Keys", |kb| &kb.k8, |kb| &mut kb.k8),
];

pub fn ui_key_bind_editor(ui: &mut egui::Ui, config: &mut Config) {
    ui.label(egui::RichText::new("Keyboard Configuration").strong());
    ui.label("Select the keyboard layout you want to edit:");

    let mut selected = 3usize; // Default: 7 Keys
    egui::ComboBox::from_id_salt("layout_select")
        .selected_text(LAYOUTS[selected].0)
        .show_ui(ui, |ui| {
            for (i, (name, _, _)) in LAYOUTS.iter().enumerate() {
                if ui.selectable_value(&mut selected, i, *name).clicked() {
                    // Selection updated
                }
            }
        });

    ui.separator();

    // Show and edit key bindings for selected layout
    let lane_names = ["Lane 1", "Lane 2", "Lane 3", "Lane 4", "Lane 5", "Lane 6", "Lane 7", "Lane 8"];
    let layout = LAYOUTS[selected].1(&config.key_bindings).clone();

    egui::Grid::new("key_grid").striped(true).show(ui, |ui| {
        ui.label(egui::RichText::new("Key").strong());
        ui.label(egui::RichText::new("Lane").strong());
        ui.end_row();

        let layout_mut = LAYOUTS[selected].2(&mut config.key_bindings);
        for (i, _key_map) in layout.lanes.iter().enumerate() {
            if i >= 8 { break; }
            // Editable key field
            let key_ref = &mut layout_mut.lanes[i].key;
            ui.text_edit_singleline(key_ref);
            ui.label(lane_names.get(i).copied().unwrap_or("Unknown"));
            ui.end_row();
        }
    });

    ui.separator();
    ui.label("Tip: Enter the key name as winit identifies it (e.g., KeyA, Space, Semicolon).");
}
