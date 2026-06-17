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
mod dashboard_metrics;
mod ecoqos;
mod foreground;
mod io_priority;
mod performance_mode;
mod power;
mod power_source;
mod privilege;
mod process_icon;
mod responsiveness;
mod rules;
mod scheduler;
mod self_power;
mod smart_trim;
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
            Foundation::{CloseHandle, WAIT_TIMEOUT},
            System::Threading::{CreateMutexW, WaitForSingleObject},
        };

        let name = single_instance_mutex_name()
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();

        unsafe {
            let handle = CreateMutexW(std::ptr::null(), 1, name.as_ptr());
            if handle.is_null() {
                return None;
            }

            let wait_status = WaitForSingleObject(handle, 0);
            if wait_status == WAIT_TIMEOUT || wait_status == 0xFFFFFFFF {
                CloseHandle(handle);
                return None;
            }

            Some(Self(handle))
        }
    }
}

fn single_instance_mutex_name() -> String {
    let digest = std::env::current_exe()
        .ok()
        .and_then(|path| path.canonicalize().ok())
        .and_then(|path| path.to_str().map(str::to_owned))
        .map(fnv1a64)
        .unwrap_or(0x5f3f_2a4e_13a5_59f0);

    format!("Local\\PowerLeaf.SingleInstance.{digest:016x}")
}

fn fnv1a64(input: impl AsRef<str>) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in input.as_ref().as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x00000100000001b3);
    }
    hash
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
