use eframe::egui;

pub fn show(ui: &mut egui::Ui) {
    ui.heading("About");
    ui.add_space(12.0);

    ui.strong("PowerLeaf");
    ui.label(env!("CARGO_PKG_DESCRIPTION"));
    ui.add_space(18.0);

    egui::Grid::new("about_grid")
        .num_columns(2)
        .spacing([24.0, 12.0])
        .striped(true)
        .show(ui, |ui| {
            row(ui, "Author", "Tatsh Siow");
            row(ui, "Version", env!("CARGO_PKG_VERSION"));
        });
}

fn row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.strong(label);
    ui.label(value);
    ui.end_row();
}
