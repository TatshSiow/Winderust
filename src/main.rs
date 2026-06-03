#![windows_subsystem = "windows"]

#[cfg(not(windows))]
compile_error!("PowerLeaf is a Windows-only application.");

mod activity;
mod affinity;
mod app;
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

fn main() {
    use gpui::{px, rgb, size, App, AppContext, Application, Bounds, WindowBounds, WindowOptions};

    Application::new().run(|cx: &mut App| {
        gpui_component::init(cx);
        gpui_component::Theme::change(gpui_component::ThemeMode::Dark, None, cx);

        let color = |value| -> gpui::Hsla { rgb(value).into() };
        let theme = gpui_component::Theme::global_mut(cx);
        theme.colors.background = color(0x282c33);
        theme.colors.foreground = color(0xdce0e5);
        theme.colors.muted = color(0x363c46);
        theme.colors.muted_foreground = color(0xa9afbc);
        theme.colors.border = color(0x464b57);
        theme.colors.input = color(0x464b57);
        theme.colors.primary = color(0x293b5b);
        theme.colors.primary_hover = color(0x363c46);
        theme.colors.primary_active = color(0x454a56);
        theme.colors.primary_foreground = color(0xdce0e5);
        theme.colors.secondary = color(0x363c46);
        theme.colors.secondary_hover = color(0x454a56);
        theme.colors.secondary_active = color(0x2f343e);
        theme.colors.secondary_foreground = color(0xdce0e5);
        theme.colors.accent = color(0x293b5b);
        theme.colors.accent_foreground = color(0x74ade8);
        theme.colors.ring = color(0x74ade8);
        theme.colors.link = color(0x74ade8);
        theme.colors.success = color(0x38482f);
        theme.colors.success_foreground = color(0xa1c181);
        theme.colors.warning = color(0x5d4c2f);
        theme.colors.warning_foreground = color(0xdec184);
        theme.colors.danger = color(0x4c2b2c);
        theme.colors.danger_foreground = color(0xd07277);

        let bounds = Bounds::centered(None, size(px(1120.0), px(760.0)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                window_min_size: Some(size(px(900.0), px(620.0))),
                app_id: Some("PowerLeaf".to_owned()),
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
