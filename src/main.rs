#![windows_subsystem = "windows"]

#[cfg(not(windows))]
compile_error!("PowerLeaf is a Windows-only application.");

mod activity;
mod app;
mod automation;
mod config;
mod cpu;
mod ecoqos;
mod foreground;
mod power;
mod rules;
mod scheduler;
mod tray;
mod ui;

fn main() -> eframe::Result<()> {
    let native_options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1120.0, 760.0])
            .with_min_inner_size([900.0, 620.0])
            .with_title("PowerLeaf"),
        ..Default::default()
    };

    eframe::run_native(
        "PowerLeaf",
        native_options,
        Box::new(|cc| Ok(Box::new(app::PowerLeafApp::new(cc)))),
    )
}
