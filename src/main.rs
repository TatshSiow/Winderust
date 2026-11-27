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
mod cpu_topology;
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
    privilege, process_icon, self_power, tray, update_checker, win_util, windows_events,
};
use features::{
    advanced_controls::timer_resolution,
    cpu_control::{adaptive_engine_process, cpu_allocation, cpu_limiter},
    priority_control::{
        dynamic_priority_boost, efficiency_mode, gpu_priority, io_priority, memory_priority,
        process_priority, thread_priority,
    },
    watchdog::{app_suspension, memory_trim, sleep_prevention},
};
use ui::app;

rust_i18n::i18n!("locales", fallback = "en");

fn main() {
    if backend::interrupt_timing::run_worker_if_requested() {
        return;
    }
    #[cfg(feature = "recovery-smoke")]
    if crash_recovery::smoke::run_if_requested() {
        return;
    }
    #[cfg(feature = "render-smoke")]
    if std::env::args().any(|argument| argument == "--window-lifecycle-smoke") {
        ui::app::window_lifecycle_smoke::run_check();
        return;
    }
    #[cfg(feature = "render-smoke")]
    if std::env::args().any(|argument| argument == "--render-smoke") {
        ui::app::smoke::render_all_pages();
        return;
    }
    if crash_recovery::run_watchdog_if_requested() {
        return;
    }
    #[cfg(feature = "runtime-benchmark")]
    if ui::runtime_benchmark::run_if_requested() {
        return;
    }

    let wait_for_previous_instance = privilege::elevated_relaunch_requested();
    let wait_ms = if wait_for_previous_instance {
        windows_sys::Win32::System::Threading::INFINITE
    } else {
        0
    };
    let Ok(Some(_single_instance_guard)) = acquire_single_instance(wait_ms) else {
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
    backend::diagnostics::initialize("winderust-diagnostics");
    let previous_recovery = match crash_recovery::acquire_watchdog_handoff() {
        Ok(guard) => guard,
        Err(error) => {
            let locale = SettingsEditor::saved_language().locale();
            let message = if error.kind() == std::io::ErrorKind::TimedOut {
                rust_i18n::t!("status.previous_recovery_running", locale = locale)
            } else {
                rust_i18n::t!(
                    "status.previous_recovery_wait_failed",
                    locale = locale,
                    error = error
                )
            };
            backend::diagnostics::error(&message);
            backend::diagnostics::finish();
            let _ = rfd::MessageDialog::new()
                .set_title("Winderust")
                .set_description(message)
                .set_level(rfd::MessageLevel::Warning)
                .show();
            return;
        }
    };
    if let Err(error) = backend::interrupt_timing::recover_at_startup() {
        backend::diagnostics::error(&format!("Interrupt Timing startup cleanup failed: {error}"));
    }

    let (mut settings, outcome) = SettingsEditor::load();
    let settings_warning = settings.load_error().map(str::to_owned).or_else(|| {
        outcome
            .startup_registration_error()
            .map(|error| format!("Startup registration reconciliation failed: {error}"))
    });
    let adaptive_plan_recovery_error = power::restore_stale_adaptive_plans()
        .err()
        .map(|error| format!("Adaptive power plan recovery failed: {error}"));
    drop(previous_recovery);
    let recovery_client = std::sync::Arc::new(std::sync::Mutex::new(
        crash_recovery::RecoveryClient::start(),
    ));
    let mut startup_warnings = Vec::new();
    for warning in [settings_warning, adaptive_plan_recovery_error]
        .into_iter()
        .flatten()
    {
        backend::diagnostics::error(&warning);
        startup_warnings.push(warning);
    }
    startup_warnings.extend(crash_recovery::startup_error());
    let runtime_settings = settings.runtime_settings_snapshot();
    let runtime_handle = automation::RuntimeHandle::start(&runtime_settings);

    if let Err(error) = app::run(
        settings,
        startup_warnings,
        runtime_handle,
        restore_event,
        recovery_client.clone(),
    ) {
        backend::diagnostics::error(&error.to_string());
        eprintln!("{error}");
    }

    if let Err(error) = recovery_client
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .finish()
    {
        backend::diagnostics::error(&error.to_string());
        eprintln!("{error}");
    }
    backend::diagnostics::event("Application exit completed.");
    backend::diagnostics::finish();
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

    fn listen(self) {
        if let Err(error) = std::thread::Builder::new()
            .name("single-instance-restore".to_owned())
            .spawn(move || {
                while self.wait(windows_sys::Win32::System::Threading::INFINITE) {
                    tray::request_restore();
                }
            })
        {
            eprintln!("Failed to start the single-instance restore listener: {error}");
        }
    }
}

fn single_instance_object_name(kind: &str) -> String {
    // All builds and portable copies share one automation instance per Windows session.
    format!("Local\\Winderust.{kind}")
}

fn acquire_single_instance(wait_ms: u32) -> std::io::Result<Option<win_util::NamedMutexGuard>> {
    win_util::NamedMutexGuard::acquire(&single_instance_object_name("SingleInstance"), wait_ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translations_support_language_switching_and_interpolation() {
        const CHILD: &str = "WINDERUST_TEST_LOCALE_CHILD";
        if std::env::var_os(CHILD).is_none() {
            let output = std::process::Command::new(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "tests::translations_support_language_switching_and_interpolation",
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            return;
        }
        for (locale, dashboard, unavailable) in [
            ("en", "Dashboard", "Unavailable"),
            ("zh-TW", "\u{5100}\u{8868}\u{677f}", "無法使用"),
            ("en", "Dashboard", "Unavailable"),
        ] {
            rust_i18n::set_locale(locale);
            assert_eq!(rust_i18n::t!("nav.dashboard"), dashboard);
            assert_eq!(rust_i18n::t!("common.unavailable"), unavailable);
        }
        assert_eq!(rust_i18n::t!("common.rule", number = 7), "Rule 7");
        assert_eq!(
            rust_i18n::t!("nav.dashboard", locale = "unknown-locale"),
            "Dashboard"
        );
    }

    #[test]
    fn locales_have_matching_keys_placeholders_and_complete_labels() {
        use std::collections::{BTreeMap, BTreeSet};

        fn placeholders(value: &str) -> BTreeSet<&str> {
            value
                .split("%{")
                .skip(1)
                .map(|part| {
                    let (name, _) = part.split_once('}').expect("Unclosed locale placeholder");
                    assert!(!name.is_empty(), "Empty locale placeholder");
                    name
                })
                .collect()
        }

        let backend = _rust_i18n_backend();
        let english = backend
            .messages_for_locale("en")
            .expect("English locale must exist")
            .into_iter()
            .collect::<BTreeMap<_, _>>();
        let chinese = backend
            .messages_for_locale("zh-TW")
            .expect("Traditional Chinese locale must exist")
            .into_iter()
            .collect::<BTreeMap<_, _>>();
        assert!(!english.is_empty(), "Locale catalog must not be empty");
        assert_eq!(
            english.keys().collect::<Vec<_>>(),
            chinese.keys().collect::<Vec<_>>(),
            "Locales must contain the same keys without relying on fallback"
        );

        for (locale, messages) in [("en", &english), ("zh-TW", &chinese)] {
            for (key, value) in messages {
                assert!(
                    !value.trim().is_empty()
                        && !value.chars().all(|c| c == '?')
                        && !value.contains("??")
                        && !value.contains('\u{fffd}'),
                    "{locale}:{key} contains an empty or corrupted label: {value}"
                );
                assert_eq!(
                    placeholders(value),
                    placeholders(&english[key]),
                    "{locale}:{key} must preserve interpolation parameters"
                );
            }
        }
    }

    #[test]
    fn instance_names_are_session_scoped_and_independent_of_executable_path() {
        assert_eq!(
            single_instance_object_name("SingleInstance"),
            r"Local\Winderust.SingleInstance"
        );
        assert_eq!(
            single_instance_object_name("RestoreWindow"),
            r"Local\Winderust.RestoreWindow"
        );
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
            let guard = win_util::NamedMutexGuard::acquire(&owner_name, 0)
                .expect("open mutex")
                .expect("test owner acquires mutex");
            owned_tx.send(()).expect("report mutex ownership");
            release_rx.recv().expect("wait for handoff");
            drop(guard);
        });

        owned_rx.recv().expect("wait for mutex ownership");
        assert!(win_util::NamedMutexGuard::acquire(&name, 0)
            .expect("wait for occupied mutex")
            .is_none());
        let (waiting_tx, waiting_rx) = mpsc::sync_channel(1);
        let waiter = thread::spawn(move || {
            waiting_tx.send(()).expect("report handoff wait");
            win_util::NamedMutexGuard::acquire(&name, 1_000)
                .expect("wait for previous owner")
                .is_some()
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
            .split_once("struct SingleInstanceRestoreEvent")
            .expect("main function end")
            .0;
        let helper_mode = main_body
            .find("run_watchdog_if_requested")
            .expect("helper mode");
        let elevated_relaunch = main_body
            .find("elevated_relaunch_requested")
            .expect("elevated relaunch handoff");
        let single_instance = main_body
            .find("acquire_single_instance")
            .expect("single-instance guard");
        let admin_check = main_body
            .find("is_running_as_admin")
            .expect("administrator check");
        let admin_relaunch = main_body
            .find("relaunch_as_admin")
            .expect("administrator relaunch");
        let restore_event = main_body
            .find("SingleInstanceRestoreEvent::create")
            .expect("duplicate launch restore event");
        let handoff = main_body
            .find("acquire_watchdog_handoff")
            .expect("previous recovery handoff");
        let release_handoff = main_body
            .find("drop(previous_recovery)")
            .expect("release previous recovery handoff");
        let settings = main_body
            .find("SettingsEditor::load")
            .expect("settings load");
        let recovery = main_body
            .find("RecoveryClient::start")
            .expect("RecoveryClient startup");
        let stale_plan_recovery = main_body
            .find("restore_stale_adaptive_plans")
            .expect("stale adaptive-plan recovery");
        let application = main_body.find("app::run(").expect("Iced startup");
        let runtime = main_body
            .find("RuntimeHandle::start")
            .expect("runtime startup");
        let finish = main_body.find(".finish()").expect("RecoveryClient finish");

        assert!(helper_mode < elevated_relaunch);
        assert!(elevated_relaunch < single_instance);
        assert!(single_instance < admin_check);
        assert!(admin_check < admin_relaunch);
        assert!(admin_relaunch < restore_event);
        assert!(restore_event < handoff);
        assert!(handoff < settings);
        assert!(settings < stale_plan_recovery);
        assert!(stale_plan_recovery < release_handoff);
        assert!(release_handoff < recovery);
        assert!(recovery < runtime);
        assert!(runtime < application);
        assert!(application < finish);

        let ui_source = include_str!("ui/app.rs");
        let shutdown = ui_source
            .split_once("fn shutdown(&mut self)")
            .expect("UI shutdown")
            .1
            .split_once("fn finish_exit")
            .expect("UI exit completion")
            .0;
        let exiting = shutdown.find("self.exiting = true").expect("exiting modal");
        let restoration = shutdown
            .find("runtime.shutdown()")
            .expect("managed restoration");
        let helper = shutdown
            .find(".finish()")
            .expect("recovery helper completion");
        let completion = shutdown
            .find(".map(Message::ShutdownFinished)")
            .expect("UI completion");
        assert!(exiting < restoration);
        assert!(restoration < helper);
        assert!(helper < completion);
    }
}
