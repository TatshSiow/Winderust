#![windows_subsystem = "windows"]
#![warn(clippy::undocumented_unsafe_blocks)]
#![warn(unsafe_op_in_unsafe_fn)]

#[cfg(not(windows))]
compile_error!("Winderust is a Windows-only application.");

mod action_log;
mod activity;
mod backend;
mod config;
mod cpu;
mod features;
mod foreground;
mod power;
mod rules;
mod scheduler;
mod ui;

use backend::{
    audio_activity, automation, dashboard_metrics, file_dialog, power_source, privilege,
    process_icon, self_power, startup, tray, update_checker, win_registry, win_util,
    windows_events,
};
use features::{
    advanced_controls::{app_suspension, timer_resolution},
    cpu_control::{background_cpu_restriction as background_cpu, core_limiter, core_steering},
    priority_control::{
        dynamic_priority_boost, gpu_priority, io_priority, memory_priority, process_priority,
        thread_priority,
    },
    winderust_features::{background_efficiency, memory_trim, workload_engine},
};
use ui::{app, assets};

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
                    app_id: Some("Winderust".to_owned()),
                    window_decorations: Some(WindowDecorations::Client),
                    ..Default::default()
                },
                |window, cx| {
                    window.set_window_title("Winderust");
                    let view = cx.new(|cx| app::WinderustApp::new(window, cx));
                    cx.new(|cx| gpui_component::Root::new(view, window, cx))
                },
            )
            .expect("failed to open Winderust window");
        });
}

struct SingleInstanceGuard {
    _handle: win_util::WinHandle,
}

impl SingleInstanceGuard {
    fn acquire() -> Option<Self> {
        use windows_sys::Win32::{
            Foundation::WAIT_TIMEOUT,
            System::Threading::{CreateMutexW, WaitForSingleObject},
        };

        let name = single_instance_mutex_name()
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();

        // SAFETY: name is terminated UTF-16, the mutex requests initial ownership, and the
        // returned non-null handle is immediately placed in the owning WinHandle wrapper.
        unsafe {
            let handle = CreateMutexW(std::ptr::null(), 1, name.as_ptr());
            if handle.is_null() {
                return None;
            }

            let handle = win_util::WinHandle::new(handle);
            let wait_status = WaitForSingleObject(handle.raw(), 0);
            if wait_status == WAIT_TIMEOUT || wait_status == 0xFFFFFFFF {
                return None;
            }

            Some(Self { _handle: handle })
        }
    }
}

fn single_instance_mutex_name() -> String {
    // Scope the mutex to this executable path so separate portable copies can run independently.
    let digest = std::env::current_exe()
        .ok()
        .and_then(|path| path.canonicalize().ok())
        .and_then(|path| path.to_str().map(str::to_owned))
        .map(fnv1a64)
        .unwrap_or(0x5f3f_2a4e_13a5_59f0);

    format!("Local\\Winderust.SingleInstance.{digest:016x}")
}

fn fnv1a64(input: impl AsRef<str>) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for byte in input.as_ref().as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x00000100000001b3);
    }
    hash
}
