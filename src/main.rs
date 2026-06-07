#![windows_subsystem = "windows"]

#[cfg(not(windows))]
compile_error!("PowerLeaf is a Windows-only application.");

mod action_log;
mod activity;
mod affinity;
mod app;
mod assets;
mod automation;
mod background_cpu;
mod config;
mod cpu;
mod cpu_limiter;
mod ecoqos;
mod foreground;
mod performance_mode;
mod power;
mod power_source;
mod privilege;
mod responsiveness;
mod rules;
mod scheduler;
mod self_power;
mod startup;
mod suspension;
mod tray;
mod ui;
mod watchdog;
mod windows_events;

rust_i18n::i18n!("locales", fallback = "en");

fn main() {
    use gpui::{
        px, size, App, AppContext, Application, Bounds, WindowBounds, WindowDecorations,
        WindowOptions,
    };

    let Some(_single_instance_guard) = SingleInstanceGuard::acquire() else {
        return;
    };

    privilege::enable_debug_privilege();

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

struct SingleInstanceGuard(windows_sys::Win32::Foundation::HANDLE);

impl SingleInstanceGuard {
    fn acquire() -> Option<Self> {
        use windows_sys::Win32::{
            Foundation::{CloseHandle, ERROR_ALREADY_EXISTS},
            System::Threading::CreateMutexW,
        };

        let name = "Local\\PowerLeaf.SingleInstance.Mutex"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();

        unsafe {
            let handle = CreateMutexW(std::ptr::null(), 1, name.as_ptr());
            if handle.is_null() {
                return None;
            }

            if windows_sys::Win32::Foundation::GetLastError() == ERROR_ALREADY_EXISTS {
                CloseHandle(handle);
                return None;
            }

            Some(Self(handle))
        }
    }
}

impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                windows_sys::Win32::Foundation::CloseHandle(self.0);
            }
        }
    }
}
