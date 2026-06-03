#![windows_subsystem = "windows"]

#[cfg(not(windows))]
compile_error!("PowerLeaf is a Windows-only application.");

mod activity;
mod affinity;
mod app;
mod assets;
mod automation;
mod config;
mod cpu;
mod ecoqos;
mod foreground;
mod power;
mod power_source;
mod rules;
mod scheduler;
mod startup;
mod suspension;
mod tray;
mod ui;

rust_i18n::i18n!("locales", fallback = "en");

fn main() {
    use gpui::{
        px, size, App, AppContext, Application, Bounds, WindowBounds, WindowDecorations,
        WindowOptions,
    };

    Application::new()
        .with_assets(assets::Assets)
        .run(|cx: &mut App| {
            gpui_component::init(cx);

            let bounds = Bounds::centered(None, size(px(1120.0), px(760.0)), cx);
            cx.open_window(
                WindowOptions {
                    titlebar: None,
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    window_min_size: Some(size(px(900.0), px(620.0))),
                    app_id: Some("PowerLeaf".to_owned()),
                    window_decorations: Some(WindowDecorations::Client),
                    ..Default::default()
                },
                |window, cx| {
                    window.set_window_title("PowerLeaf");
                    let view = cx.new(|cx| app::PowerLeafApp::new(window, cx));
                    cx.new(|cx| gpui_component::Root::new(view, window, cx))
                },
            )
            .expect("failed to open PowerLeaf window");
        });
}
