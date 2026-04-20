pub fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    let inter_data = include_bytes!("../../../assets/fonts/Inter-Regular.ttf");
    let noto_data = include_bytes!("../../../assets/fonts/NotoSansCJKsc-Regular.otf");

    fonts.font_data.insert(
        "inter".to_string(),
        std::sync::Arc::new(egui::FontData::from_owned(inter_data.to_vec())),
    );
    fonts
        .families
        .get_mut(&egui::FontFamily::Proportional)
        .unwrap()
        .insert(0, "inter".to_string());
    fonts
        .families
        .get_mut(&egui::FontFamily::Monospace)
        .unwrap()
        .insert(0, "inter".to_string());
    log::info!("Bundled Inter font loaded");

    fonts.font_data.insert(
        "noto-sans-cjk".to_string(),
        std::sync::Arc::new(egui::FontData::from_owned(noto_data.to_vec())),
    );
    fonts
        .families
        .get_mut(&egui::FontFamily::Proportional)
        .unwrap()
        .push("noto-sans-cjk".to_string());
    fonts
        .families
        .get_mut(&egui::FontFamily::Monospace)
        .unwrap()
        .push("noto-sans-cjk".to_string());
    log::info!("Bundled Noto Sans CJK font loaded");

    ctx.set_fonts(fonts);
}
