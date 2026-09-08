#![windows_subsystem = "windows"]
#![warn(clippy::undocumented_unsafe_blocks)]
#![warn(unsafe_op_in_unsafe_fn)]

#[cfg(not(windows))]
compile_error!("Winderust is a Windows-only application.");

mod action_log;
mod activity;
mod application;
mod backend;
mod bottleneck_classifier;
mod config;
mod control;
mod cpu;
mod features;
mod foreground;
mod platform;
mod power;
mod rules;
mod runtime;
mod ui;

use application::SettingsEditor;
use backend::{
    audio_activity, automation, crash_recovery, dashboard_metrics, file_dialog, power_source,
    privilege, process_icon, self_power, tray, update_checker, win_registry, win_util,
    windows_events,
};
use features::{
    advanced_controls::{app_suspension, timer_resolution},
    cpu_control::{cpu_allocation, cpu_limiter},
    priority_control::{
        dynamic_priority_boost, gpu_priority, io_priority, memory_priority, process_priority,
        thread_priority,
    },
    winderust_features::{background_efficiency, cpu_scheduler, memory_trim},
};
use ui::app;

rust_i18n::i18n!("locales", fallback = "en");

fn main() {
    #[cfg(feature = "render-smoke")]
    if std::env::args().any(|argument| argument == "--render-smoke") {
        ui::app::smoke::render_all_pages();
        return;
    }
    if crash_recovery::run_watchdog_if_requested() {
        return;
    }

    let wait_for_previous_instance = privilege::elevated_relaunch_requested();
    let Some(_single_instance_guard) = SingleInstanceGuard::acquire(wait_for_previous_instance)
    else {
        if !wait_for_previous_instance {
            SingleInstanceRestoreEvent::signal_existing();
        }
        return;
    };
    if !privilege::is_running_as_admin() {
        privilege::relaunch_as_admin();
        return;
    }
    let restore_event = SingleInstanceRestoreEvent::create();

    let (mut settings, settings_load_error) = match SettingsEditor::load() {
        Ok((settings, outcome)) => (
            settings,
            outcome
                .startup_registration_error()
                .map(|error| format!("Startup registration reconciliation failed: {error}")),
        ),
        Err(error) => (
            SettingsEditor::with_settings(config::Settings::default()),
            Some(error.to_string()),
        ),
    };
    let mut recovery_client = crash_recovery::RecoveryClient::start();
    let adaptive_plan_recovery_error = power::restore_stale_adaptive_plans()
        .err()
        .map(|error| format!("Adaptive power plan recovery failed: {error}"));
    let settings_load_error = settings_load_error.or(adaptive_plan_recovery_error);
    let runtime_settings = settings.runtime_settings_snapshot();
    let runtime_handle = automation::RuntimeHandle::start(&runtime_settings);

    if let Err(error) = app::run(settings, settings_load_error, runtime_handle, restore_event) {
        eprintln!("{error}");
    }

    if let Err(error) = recovery_client.finish() {
        eprintln!("{error}");
    }
}

struct SingleInstanceRestoreEvent {
    handle: win_util::WinHandle,
}

// SAFETY: Win32 event handles may be waited on and closed from a different thread.
unsafe impl Send for SingleInstanceRestoreEvent {}

impl SingleInstanceRestoreEvent {
    fn create() -> Option<Self> {
        Self::create_named(&single_instance_object_name("RestoreWindow"))
    }

    fn create_named(name: &str) -> Option<Self> {
        use windows_sys::Win32::System::Threading::CreateEventW;

        let name = win_util::wide_null(name);
        // SAFETY: name is terminated UTF-16 and the returned auto-reset event handle is owned by
        // this process.
        let handle = unsafe { CreateEventW(std::ptr::null(), 0, 0, name.as_ptr()) };
        (!handle.is_null()).then(|| Self {
            handle: win_util::WinHandle::new(handle),
        })
    }

    fn signal_existing() {
        Self::signal_named(&single_instance_object_name("RestoreWindow"));
    }

    fn signal_named(name: &str) -> bool {
        use windows_sys::Win32::System::Threading::{OpenEventW, SetEvent, EVENT_MODIFY_STATE};

        let name = win_util::wide_null(name);
        // SAFETY: name is terminated UTF-16 and the requested access is limited to signaling.
        let handle = unsafe { OpenEventW(EVENT_MODIFY_STATE, 0, name.as_ptr()) };
        if handle.is_null() {
            return false;
        }
        let handle = win_util::WinHandle::new(handle);
        // SAFETY: handle owns a live event opened with EVENT_MODIFY_STATE access.
        unsafe { SetEvent(handle.raw()) != 0 }
    }

    fn wait(&self, wait_milliseconds: u32) -> bool {
        use windows_sys::Win32::{
            Foundation::WAIT_OBJECT_0, System::Threading::WaitForSingleObject,
        };

        // SAFETY: handle owns a live event and the wait does not retain pointers.
        unsafe { WaitForSingleObject(self.handle.raw(), wait_milliseconds) == WAIT_OBJECT_0 }
    }

    fn listen(self, hwnd: windows_sys::Win32::Foundation::HWND) {
        let hwnd = hwnd as usize;
        if let Err(error) = std::thread::Builder::new()
            .name("single-instance-restore".to_owned())
            .spawn(move || {
                while self.wait(windows_sys::Win32::System::Threading::INFINITE) {
                    tray::show_window(hwnd as windows_sys::Win32::Foundation::HWND);
                }
            })
        {
            eprintln!("Failed to start the single-instance restore listener: {error}");
        }
    }
}

struct SingleInstanceGuard {
    handle: win_util::WinHandle,
}

impl SingleInstanceGuard {
    fn acquire(wait_for_previous_instance: bool) -> Option<Self> {
        let wait_milliseconds = if wait_for_previous_instance {
            windows_sys::Win32::System::Threading::INFINITE
        } else {
            0
        };
        Self::acquire_named(
            &single_instance_object_name("SingleInstance"),
            wait_milliseconds,
        )
    }

    fn acquire_named(name: &str, wait_milliseconds: u32) -> Option<Self> {
        use windows_sys::Win32::{
            Foundation::{WAIT_ABANDONED, WAIT_OBJECT_0},
            System::Threading::{CreateMutexW, WaitForSingleObject},
        };

        let name = name
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect::<Vec<_>>();

        // SAFETY: name is terminated UTF-16 and the returned handle is owned by this process.
        let handle = unsafe { CreateMutexW(std::ptr::null(), 0, name.as_ptr()) };
        if handle.is_null() {
            return None;
        }

        let handle = win_util::WinHandle::new(handle);
        // SAFETY: handle owns a live mutex and the wait does not retain pointers. Only the
        // explicitly elevated replacement waits for the previous instance to finish shutdown.
        let wait_status = unsafe { WaitForSingleObject(handle.raw(), wait_milliseconds) };
        matches!(wait_status, WAIT_OBJECT_0 | WAIT_ABANDONED).then_some(Self { handle })
    }
}

impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        use windows_sys::Win32::System::Threading::ReleaseMutex;

        // SAFETY: acquire only constructs the guard after this thread owns the mutex.
        unsafe {
            ReleaseMutex(self.handle.raw());
        }
    }
}

fn single_instance_object_name(kind: &str) -> String {
    use std::os::windows::ffi::OsStrExt;

    // Scope the mutex to this executable path so separate portable copies can run independently.
    let digest = std::env::current_exe()
        .ok()
        .map(|path| path.canonicalize().unwrap_or(path))
        .map(|path| fnv1a64(path.as_os_str().encode_wide()))
        .unwrap_or(0x5f3f_2a4e_13a5_59f0);

    format!("Local\\Winderust.{kind}.{digest:016x}")
}

fn fnv1a64(input: impl IntoIterator<Item = u16>) -> u64 {
    let mut hash: u64 = 0xcbf29ce484222325;
    for unit in input {
        for byte in unit.to_le_bytes() {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x00000100000001b3);
        }
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn executable_path_hash_preserves_non_unicode_units() {
        assert_ne!(fnv1a64([0xD800]), fnv1a64([0xFFFD]));
    }

    #[test]
    fn elevated_relaunch_waits_for_the_previous_instance_mutex() {
        use std::{sync::mpsc, thread};

        let name = format!(
            "Local\\Winderust.SingleInstance.Test.{}",
            std::process::id()
        );
        let (owned_tx, owned_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        let owner_name = name.clone();
        let owner = thread::spawn(move || {
            let guard = SingleInstanceGuard::acquire_named(&owner_name, 0)
                .expect("test owner acquires mutex");
            owned_tx.send(()).expect("report mutex ownership");
            release_rx.recv().expect("wait for handoff");
            drop(guard);
        });

        owned_rx.recv().expect("wait for mutex ownership");
        assert!(SingleInstanceGuard::acquire_named(&name, 0).is_none());
        let (waiting_tx, waiting_rx) = mpsc::sync_channel(1);
        let waiter = thread::spawn(move || {
            waiting_tx.send(()).expect("report handoff wait");
            SingleInstanceGuard::acquire_named(&name, 1_000).is_some()
        });
        waiting_rx.recv().expect("wait for elevated handoff");
        release_tx.send(()).expect("release previous instance");
        assert!(waiter.join().expect("handoff waiter exits cleanly"));
        owner.join().expect("test owner exits cleanly");
    }

    #[test]
    fn duplicate_launch_signal_wakes_restore_event() {
        let name = format!("Local\\Winderust.RestoreWindow.Test.{}", std::process::id());
        let event = SingleInstanceRestoreEvent::create_named(&name).expect("create restore event");

        assert!(SingleInstanceRestoreEvent::signal_named(&name));
        assert!(event.wait(0));
        assert!(!event.wait(0));
    }

    #[test]
    fn application_lifecycle_orders_settings_recovery_runtime_and_helper_finish() {
        let source = include_str!("main.rs");
        let main_body = source
            .split_once("fn main()")
            .expect("main function")
            .1
            .split_once("struct SingleInstanceGuard")
            .expect("main function end")
            .0;
        let helper_mode = main_body
            .find("run_watchdog_if_requested")
            .expect("helper mode");
        let elevated_relaunch = main_body
            .find("elevated_relaunch_requested")
            .expect("elevated relaunch handoff");
        let single_instance = main_body
            .find("SingleInstanceGuard::acquire")
            .expect("single-instance guard");
        let admin_check = main_body
            .find("is_running_as_admin")
            .expect("administrator check");
        let admin_relaunch = main_body
            .find("relaunch_as_admin")
            .expect("administrator relaunch");
        let settings = main_body
            .find("SettingsEditor::load")
            .expect("settings load");
        let recovery = main_body
            .find("RecoveryClient::start")
            .expect("RecoveryClient startup");
        let stale_plan_recovery = main_body
            .find("restore_stale_adaptive_plans")
            .expect("stale adaptive-plan recovery");
        let application = main_body.find("app::run(settings").expect("Iced startup");
        let runtime = main_body
            .find("RuntimeHandle::start")
            .expect("runtime startup");
        let finish = main_body
            .find("recovery_client.finish")
            .expect("RecoveryClient finish");

        assert!(helper_mode < elevated_relaunch);
        assert!(elevated_relaunch < single_instance);
        assert!(single_instance < admin_check);
        assert!(admin_check < admin_relaunch);
        assert!(admin_relaunch < settings);
        assert!(settings < recovery);
        assert!(recovery < stale_plan_recovery);
        assert!(stale_plan_recovery < runtime);
        assert!(runtime < application);
        assert!(application < finish);
    }
}
