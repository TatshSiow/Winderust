use super::*;
use chrono::{Datelike, Duration as ChronoDuration, Local};

use crate::config::{
    AppSuspensionRule, ByForegroundRule, ByRunningAppRule, ByTimeRule, CoreLimiterRule,
    CoreSteeringRule, ProcessDynamicPriorityBoostSetting, ProcessExclusionRule,
    ProcessGpuPrioritySetting, ProcessThreadPrioritySetting, TimerResolutionRule, WeekdaySetting,
};

fn app_suspension_rule(executable_path: &str) -> AppSuspensionRule {
    AppSuspensionRule {
        enabled: true,
        executable_path: executable_path.to_owned(),
        network_wake_enabled: false,
        audio_wake_enabled: false,
        network_download_threshold_bytes: 0,
        network_download_threshold_unit: Default::default(),
        network_upload_threshold_bytes: 0,
        network_upload_threshold_unit: Default::default(),
    }
}

#[test]
fn process_appearance_detector_ignores_initial_snapshot() {
    let mut known = BTreeSet::new();

    assert!(!process_ids_have_new_entries(
        &mut known,
        BTreeSet::from([1, 2])
    ));
    assert_eq!(known, BTreeSet::from([1, 2]));
}

#[test]
fn process_appearance_detector_reports_new_process_ids() {
    let mut known = BTreeSet::from([1, 2]);

    assert!(process_ids_have_new_entries(
        &mut known,
        BTreeSet::from([1, 2, 3])
    ));
    assert_eq!(known, BTreeSet::from([1, 2, 3]));
}

#[test]
fn repeated_power_plan_switch_failures_suppress_future_attempts() {
    let mut runner = HiddenAutomationRunner::default();

    runner.record_switch_failure("PLAN-GUID");
    runner.record_switch_failure("plan-guid");
    assert!(!runner.is_switch_suppressed("plan-guid"));

    runner.record_switch_failure("plan-guid");
    assert!(runner.is_switch_suppressed("plan-guid"));

    runner.clear_switch_failure("PLAN-GUID");
    assert!(!runner.is_switch_suppressed("plan-guid"));
}

#[test]
fn adaptive_plan_setup_error_preserves_cleanup_failure() {
    assert_eq!(
        adaptive_plan_setup_error(
            "Applying the adaptive plan failed.".to_owned(),
            Err("Deleting the adaptive plan failed.".to_owned()),
        ),
        "Applying the adaptive plan failed. Adaptive plan cleanup also failed: Deleting the adaptive plan failed."
    );
    assert_eq!(
        adaptive_plan_setup_error("Applying the adaptive plan failed.".to_owned(), Ok(())),
        "Applying the adaptive plan failed."
    );
}

#[test]
fn poisoned_automation_mutex_is_recovered() {
    let mutex = Mutex::new(42);
    let _ = std::panic::catch_unwind(|| {
        let _guard = mutex.lock().expect("test mutex starts healthy");
        panic!("poison test mutex");
    });

    assert_eq!(*lock_unpoisoned(&mutex), 42);
}

#[test]
fn automation_worker_error_is_delivered_once() {
    let automation = BackgroundAutomation::start(&Settings::default());
    update_worker_error(
        &automation.shared,
        Some("Background automation worker stopped unexpectedly.".to_owned()),
    );

    let snapshot = automation
        .status_snapshot_since(1)
        .expect("worker failure advances status generation");
    assert_eq!(
        snapshot.worker_error.as_deref(),
        Some("Background automation worker stopped unexpectedly.")
    );
    assert!(lock_unpoisoned(&automation.shared.state)
        .status
        .worker_error
        .is_none());
    assert!(automation
        .status_snapshot_since(snapshot.generation)
        .is_none());
}

#[test]
fn process_appearance_detector_does_not_report_only_exits() {
    let mut known = BTreeSet::from([1, 2, 3]);

    assert!(!process_ids_have_new_entries(
        &mut known,
        BTreeSet::from([1, 2])
    ));
    assert_eq!(known, BTreeSet::from([1, 2]));
}

#[test]
fn process_appearance_scan_sleeps_when_process_features_are_off() {
    let settings = Settings::default();

    assert!(!process_appearance_scan_required(&settings));
}

#[test]
fn foreground_lookup_runs_only_for_configured_by_foreground() {
    let mut settings = Settings::default();

    assert!(!foreground_lookup_required(&settings));

    settings.by_foreground.enabled = true;
    assert!(!foreground_lookup_required(&settings));

    settings.by_foreground.rules.push(ByForegroundRule {
        enabled: true,
        name: "editor.exe".to_owned(),
        executable_path: String::new(),
        power_plan_guid: None,
    });
    assert!(!foreground_lookup_required(&settings));

    settings.by_foreground.rules[0].power_plan_guid = Some("active-guid".to_owned());
    assert!(!foreground_lookup_required(&settings));

    settings.by_foreground.rules[0].executable_path = r"C:\Apps\editor.exe".to_owned();
    assert!(foreground_lookup_required(&settings));

    settings.by_foreground.rules[0].enabled = false;
    assert!(!foreground_lookup_required(&settings));
}

#[test]
fn automation_worker_sleeps_when_no_automation_work_exists() {
    let settings = Settings::default();

    assert!(!automation_worker_required(&settings));
}

#[test]
fn enabled_empty_rule_features_do_not_poll() {
    let mut settings = Settings::default();
    settings.app_suspension.enabled = true;
    settings.core_steering.enabled = true;
    settings.core_limiter.enabled = true;
    settings.by_running_app.enabled = true;
    settings.timer_resolution.enabled = true;
    settings.by_foreground.enabled = true;

    assert!(!automation_worker_required(&settings));

    settings
        .app_suspension
        .suspendable_apps
        .push(app_suspension_rule(" "));
    settings.core_steering.rules.push(CoreSteeringRule {
        enabled: true,
        mode: Default::default(),
        executable_path: " ".to_owned(),
        core_mask: 1,
    });
    settings.core_limiter.rules.push(CoreLimiterRule {
        enabled: true,
        executable_path: " ".to_owned(),
        threshold_percent: 80,
        sustain_seconds: 1,
        cooldown_seconds: 1,
        max_logical_processors: 1,
    });
    settings.by_running_app.rules.push(ByRunningAppRule {
        enabled: true,
        name: "Empty".to_owned(),
        executable_path: " ".to_owned(),
        power_plan_guid: Some("active-guid".to_owned()),
    });
    settings.timer_resolution.rules.push(TimerResolutionRule {
        enabled: true,
        executable_path: " ".to_owned(),
        desired_100ns: 5_000,
    });
    settings.by_foreground.rules.push(ByForegroundRule {
        enabled: true,
        name: "Empty".to_owned(),
        executable_path: " ".to_owned(),
        power_plan_guid: Some("active-guid".to_owned()),
    });

    assert!(!app_suspension_required(&settings));
    assert!(!core_steering_required(&settings));
    assert!(!core_limiter_required(&settings));
    assert!(!by_running_app_required(&settings));
    assert!(!timer_resolution_required(&settings));
    assert!(!foreground_lookup_required(&settings));
    assert!(!process_appearance_scan_required(&settings));
    assert!(!event_driven_process_work_required(&settings));
    assert!(!automation_worker_required(&settings));
}

#[test]
fn enabled_nonempty_rule_features_require_runtime_work() {
    let mut settings = Settings::default();
    settings.app_suspension.enabled = true;
    settings
        .app_suspension
        .suspendable_apps
        .push(app_suspension_rule(r"C:\Apps\chat.exe"));
    settings.core_steering.enabled = true;
    settings.core_steering.rules.push(CoreSteeringRule {
        enabled: true,
        mode: Default::default(),
        executable_path: r"C:\Apps\chat.exe".to_owned(),
        core_mask: 1,
    });
    settings.core_limiter.enabled = true;
    settings.core_limiter.rules.push(CoreLimiterRule {
        enabled: true,
        executable_path: r"C:\Apps\chat.exe".to_owned(),
        threshold_percent: 80,
        sustain_seconds: 1,
        cooldown_seconds: 1,
        max_logical_processors: 1,
    });
    settings.by_running_app.enabled = true;
    settings.by_running_app.rules.push(ByRunningAppRule {
        enabled: true,
        name: "Chat".to_owned(),
        executable_path: r"C:\Apps\chat.exe".to_owned(),
        power_plan_guid: Some("active-guid".to_owned()),
    });
    settings.timer_resolution.enabled = true;
    settings.timer_resolution.rules.push(TimerResolutionRule {
        enabled: true,
        executable_path: r"C:\Apps\chat.exe".to_owned(),
        desired_100ns: 5_000,
    });
    settings.by_foreground.enabled = true;
    settings.by_foreground.rules.push(ByForegroundRule {
        enabled: true,
        name: "Chat".to_owned(),
        executable_path: r"C:\Apps\chat.exe".to_owned(),
        power_plan_guid: Some("active-guid".to_owned()),
    });

    assert!(app_suspension_required(&settings));
    assert!(core_steering_required(&settings));
    assert!(core_limiter_required(&settings));
    assert!(by_running_app_required(&settings));
    assert!(timer_resolution_required(&settings));
    assert!(foreground_lookup_required(&settings));
    assert!(process_appearance_scan_required(&settings));
    assert!(event_driven_process_work_required(&settings));
    assert!(automation_worker_required(&settings));
}

#[test]
fn automation_worker_runs_for_adaptive_power_plan_alone() {
    let mut settings = Settings::default();
    settings.by_activity.enabled = false;
    settings.by_foreground.enabled = false;
    settings.adaptive_engine.enabled = true;
    settings.adaptive_engine.processor_policy_enabled = true;

    assert!(automation_worker_required(&settings));
}

#[test]
fn adaptive_engine_uses_low_power_refresh_cadence() {
    assert_eq!(
        automation_refresh_interval(false, true, Duration::from_secs(1)),
        ADAPTIVE_ENGINE_AUTOMATION_REFRESH_INTERVAL
    );
    assert_eq!(
        automation_refresh_interval(false, true, PROCESS_APPEARANCE_SCAN_INTERVAL),
        ADAPTIVE_ENGINE_AUTOMATION_REFRESH_INTERVAL
    );
    assert_eq!(
        automation_refresh_interval(false, true, APP_SUSPENSION_FOREGROUND_RELEASE_INTERVAL),
        ADAPTIVE_ENGINE_AUTOMATION_REFRESH_INTERVAL
    );
    assert_eq!(
        automation_refresh_interval(true, false, Duration::from_secs(1)),
        HIDDEN_AUTOMATION_REFRESH_INTERVAL
    );
}

#[test]
fn status_snapshot_since_skips_unchanged_status() {
    let automation = BackgroundAutomation::start(&Settings::default());
    let snapshot = automation
        .status_snapshot_since(0)
        .expect("initial status snapshot should be visible");

    assert!(automation
        .status_snapshot_since(snapshot.generation)
        .is_none());
}

#[test]
fn pending_auto_exclusions_are_taken_only_after_generation_change() {
    let automation = BackgroundAutomation::start(&Settings::default());
    let mut generation = 0;

    assert!(automation
        .take_pending_auto_exclusions_since(&mut generation)
        .is_none());

    update_core_steering_status(
        &automation.shared,
        CoreSteeringSnapshot {
            auto_excluded_processes: vec![r"D:\Games\Game.exe".to_owned()],
            ..CoreSteeringSnapshot::default()
        },
    );

    let pending = automation
        .take_pending_auto_exclusions_since(&mut generation)
        .expect("new pending affinity exclusions should be visible");
    assert_eq!(pending.core_steering, vec![r"D:\Games\Game.exe"]);
    assert!(automation
        .take_pending_auto_exclusions_since(&mut generation)
        .is_none());
}

#[test]
fn pending_auto_exclusions_keep_same_named_executable_paths_distinct() {
    let automation = BackgroundAutomation::start(&Settings::default());
    let mut generation = 0;

    update_process_priority_status(
        &automation.shared,
        ProcessPrioritySnapshot {
            auto_excluded_processes: vec![
                r"C:\Apps\Editor.exe".to_owned(),
                r"D:\Tools\Editor.exe".to_owned(),
            ],
            ..ProcessPrioritySnapshot::default()
        },
    );

    let pending = automation
        .take_pending_auto_exclusions_since(&mut generation)
        .expect("absolute executable paths should reach the pending queue");
    assert_eq!(
        pending.process_priority,
        vec![r"C:\Apps\Editor.exe", r"D:\Tools\Editor.exe"]
    );
}

#[test]
fn app_suspension_freeze_queue_preserves_and_deduplicates_executable_paths() {
    let automation = BackgroundAutomation::start(&Settings::default());

    automation.request_app_suspension_freeze("Editor.exe");
    automation.request_app_suspension_freeze(r"C:/Apps/Editor.exe");
    automation.request_app_suspension_freeze(r"C:\Apps\Editor.exe");

    let state = automation
        .shared
        .state
        .lock()
        .expect("automation state should remain available");
    assert_eq!(
        state.app_suspension_freeze_requests,
        vec![r"C:\Apps\Editor.exe"]
    );
}

#[test]
fn manual_app_suspension_request_starts_worker_without_automatic_rules() {
    let mut settings = Settings::default();
    settings.app_suspension.enabled = true;
    let automation = BackgroundAutomation::start(&settings);

    assert!(automation
        .thread
        .lock()
        .expect("automation thread state should remain available")
        .is_none());

    automation.request_app_suspension_freeze(r"C:\Apps\Editor.exe");

    assert!(automation
        .thread
        .lock()
        .expect("automation thread state should remain available")
        .is_some());
}

#[test]
fn automation_worker_runs_for_enabled_process_feature() {
    let mut settings = Settings::default();
    settings.background_efficiency.enabled = true;

    assert!(automation_worker_required(&settings));
}

#[test]
fn automation_worker_runs_for_enabled_memory_trim() {
    let mut settings = Settings::default();
    settings.memory_trim.enabled = true;

    assert!(automation_worker_required(&settings));
}

#[test]
fn workload_engine_fast_refresh_requires_enabled_feature() {
    let now = Instant::now();
    let mut settings = Settings::default();

    assert!(workload_engine_fast_refresh_deadline(&settings, now).is_none());
    assert!(!workload_engine_fast_refresh_active(
        &settings,
        Some(now + WORKLOAD_ENGINE_FAST_REFRESH_WINDOW),
        now,
    ));

    settings.general.enabled = true;
    settings.adaptive_engine.enabled = true;
    settings.workload_engine.enabled = true;
    let deadline = workload_engine_fast_refresh_deadline(&settings, now)
        .expect("Workload Engine should enable fast refresh");
    assert_eq!(
        deadline.duration_since(now),
        WORKLOAD_ENGINE_FAST_REFRESH_WINDOW
    );
    assert!(workload_engine_fast_refresh_active(
        &settings,
        Some(deadline),
        now,
    ));
    assert!(!workload_engine_fast_refresh_active(
        &settings,
        Some(deadline),
        deadline,
    ));
}

#[test]
fn workload_engine_io_assist_waits_for_pressure() {
    let mut settings = Settings::default();
    settings.adaptive_engine.enabled = true;
    settings.workload_engine.enabled = true;
    settings
        .workload_engine
        .lower_background_io_priority_enabled = true;
    settings.workload_engine.lower_background_io_priority = ProcessIoPriority::Low;

    assert!(!effective_io_priority_settings(&settings, false).enabled);

    let io_priority = effective_io_priority_settings(&settings, true);

    assert!(io_priority.enabled);
    assert!(io_priority.foreground_detection_enabled);
    assert_eq!(
        io_priority.foreground_priority.priority(),
        Some(ProcessIoPriority::Normal)
    );
    assert_eq!(
        io_priority.background_priority.priority(),
        Some(ProcessIoPriority::Low)
    );
}

#[test]
fn workload_engine_pressure_feeds_priority_defaults() {
    let mut settings = Settings::default();
    settings.adaptive_engine.enabled = true;
    settings.workload_engine.enabled = true;
    settings.workload_engine.workload_engine_enabled = true;
    settings
        .workload_engine
        .lower_background_io_priority_enabled = true;
    settings.workload_engine.lower_background_io_priority = ProcessIoPriority::Low;
    settings.workload_engine.workload_engine_io_priority.enabled = true;
    settings
        .workload_engine
        .workload_engine_io_priority
        .foreground_detection_enabled = false;
    settings
        .workload_engine
        .workload_engine_io_priority
        .preserve_foreground_priority = false;
    settings
        .workload_engine
        .workload_engine_io_priority
        .preserve_background_priority = false;
    settings
        .workload_engine
        .workload_engine_io_priority
        .background_priority = ProcessIoPriority::Low.into();
    settings
        .workload_engine
        .workload_engine_thread_priority
        .foreground_detection_enabled = false;
    settings
        .workload_engine
        .workload_engine_thread_priority
        .preserve_foreground_priority = false;
    settings
        .workload_engine
        .workload_engine_thread_priority
        .preserve_background_priority = false;
    settings
        .workload_engine
        .workload_engine_dynamic_priority_boost
        .foreground_detection_enabled = false;
    settings
        .workload_engine
        .workload_engine_gpu_priority
        .foreground_detection_enabled = false;
    settings
        .workload_engine
        .workload_engine_gpu_priority
        .preserve_foreground_priority = false;
    settings
        .workload_engine
        .workload_engine_gpu_priority
        .preserve_background_priority = false;
    settings.workload_engine.workload_engine_exclusions = vec![ProcessExclusionRule {
        executable_path: "game.exe".to_owned(),
        ..Default::default()
    }];

    assert!(thread_priority_required(&settings));
    assert!(dynamic_priority_boost_required(&settings));
    assert!(gpu_priority_required(&settings));

    let thread_priority = effective_thread_priority_settings(&settings, true);
    assert!(thread_priority.enabled);
    assert!(thread_priority.foreground_detection_enabled);
    assert!(thread_priority.preserve_foreground_priority);
    assert!(thread_priority.preserve_background_priority);
    assert_eq!(
        thread_priority.background_priority,
        ProcessThreadPrioritySetting::BelowNormal
    );
    assert!(thread_priority.contains_exclusion("game.exe"));

    let dynamic_priority_boost = effective_dynamic_priority_boost_settings(&settings, true);
    assert!(dynamic_priority_boost.enabled);
    assert!(dynamic_priority_boost.foreground_detection_enabled);
    assert_eq!(
        dynamic_priority_boost.foreground_boost,
        ProcessDynamicPriorityBoostSetting::Enabled
    );
    assert_eq!(
        dynamic_priority_boost.background_boost,
        ProcessDynamicPriorityBoostSetting::Disabled
    );
    assert!(dynamic_priority_boost.contains_exclusion("game.exe"));

    let io_priority = effective_io_priority_settings(&settings, true);
    assert_eq!(
        io_priority.background_priority.priority(),
        Some(ProcessIoPriority::Low)
    );
    assert!(io_priority.foreground_detection_enabled);
    assert!(io_priority.preserve_foreground_priority);
    assert!(io_priority.preserve_background_priority);
    assert!(io_priority.contains_exclusion("game.exe"));

    let gpu_priority = effective_gpu_priority_settings(&settings, true);
    assert!(gpu_priority.enabled);
    assert!(gpu_priority.foreground_detection_enabled);
    assert!(gpu_priority.preserve_foreground_priority);
    assert!(gpu_priority.preserve_background_priority);
    assert_eq!(
        gpu_priority.background_priority,
        ProcessGpuPrioritySetting::BelowNormal
    );
    assert!(gpu_priority.contains_exclusion("game.exe"));
}

#[test]
fn workload_engine_page_enabled_without_runtime_work_does_not_poll() {
    let mut settings = Settings::default();
    settings.adaptive_engine.enabled = true;
    settings.workload_engine.enabled = true;
    settings.workload_engine.lower_background_apps = false;
    settings
        .workload_engine
        .workload_engine_background_efficiency_enabled = false;
    settings.workload_engine.workload_engine_enabled = false;
    settings.workload_engine.boost_foreground_app = false;

    assert!(!workload_engine_required(&settings));

    settings.workload_engine.workload_engine_enabled = true;

    assert!(workload_engine_required(&settings));
}

#[test]
fn workload_engine_priority_assist_temporarily_overrides_global_priority_defaults() {
    let mut settings = Settings::default();
    settings.adaptive_engine.enabled = true;
    settings.workload_engine.enabled = true;
    settings.workload_engine.workload_engine_enabled = true;
    settings.thread_priority.enabled = true;
    settings.thread_priority.background_priority = ProcessThreadPrioritySetting::Idle;
    settings.dynamic_priority_boost.enabled = true;
    settings.dynamic_priority_boost.background_boost = ProcessDynamicPriorityBoostSetting::Enabled;
    settings.gpu_priority.enabled = true;
    settings.gpu_priority.background_priority = ProcessGpuPrioritySetting::Idle;
    settings
        .workload_engine
        .workload_engine_thread_priority
        .background_priority = ProcessThreadPrioritySetting::BelowNormal;
    settings
        .workload_engine
        .workload_engine_dynamic_priority_boost
        .background_boost = ProcessDynamicPriorityBoostSetting::Disabled;
    settings
        .workload_engine
        .workload_engine_gpu_priority
        .background_priority = ProcessGpuPrioritySetting::BelowNormal;

    assert_eq!(
        effective_thread_priority_settings(&settings, true).background_priority,
        ProcessThreadPrioritySetting::BelowNormal
    );
    assert_eq!(
        effective_dynamic_priority_boost_settings(&settings, true).background_boost,
        ProcessDynamicPriorityBoostSetting::Disabled
    );
    assert_eq!(
        effective_gpu_priority_settings(&settings, true).background_priority,
        ProcessGpuPrioritySetting::BelowNormal
    );
    assert_eq!(
        effective_thread_priority_settings(&settings, false).background_priority,
        ProcessThreadPrioritySetting::Idle
    );
    assert_eq!(
        effective_dynamic_priority_boost_settings(&settings, false).background_boost,
        ProcessDynamicPriorityBoostSetting::Enabled
    );
    assert_eq!(
        effective_gpu_priority_settings(&settings, false).background_priority,
        ProcessGpuPrioritySetting::Idle
    );
}

#[test]
fn workload_engine_without_io_assist_does_not_require_io_refresh() {
    let mut settings = Settings::default();
    settings.adaptive_engine.enabled = true;
    settings.workload_engine.enabled = true;
    settings.workload_engine.workload_engine_enabled = true;
    settings.workload_engine.boost_foreground_app = false;

    assert!(!io_priority_required(&settings));
}

#[test]
fn default_settings_do_not_poll_power_plans_without_plan_targets() {
    let settings = Settings::default();

    assert!(!power_plan_checks_required(&settings));
}

#[test]
fn app_suspension_uses_own_refresh_without_process_appearance_scan() {
    let mut settings = Settings::default();
    settings.app_suspension.enabled = true;
    settings
        .app_suspension
        .suspendable_apps
        .push(app_suspension_rule(r"C:\Apps\chat.exe"));

    assert!(feature_refresh_required(
        &settings,
        app_suspension_required(&settings)
    ));
    assert!(!process_appearance_scan_required(&settings));
}

#[test]
fn app_suspension_uses_windows_events_without_enabling_process_scan() {
    let mut settings = Settings::default();
    settings.app_suspension.enabled = true;
    settings
        .app_suspension
        .suspendable_apps
        .push(app_suspension_rule(r"C:\Apps\chat.exe"));

    assert!(windows_event_watcher_required(&settings));
    assert!(windows_event_wake_required(
        &settings,
        WindowsAutomationEvent::WindowCreated
    ));
    assert!(!process_appearance_scan_required(&settings));
}

#[test]
fn system_appearance_uses_windows_events_without_power_automation() {
    let mut settings = Settings::default();
    settings.general.enabled = false;
    settings.general.accent.source = AccentColorSource::Windows;

    assert!(windows_event_watcher_required(&settings));
    assert!(windows_event_wake_required(
        &settings,
        WindowsAutomationEvent::AppearanceChanged
    ));
    assert!(!windows_event_wake_required(
        &settings,
        WindowsAutomationEvent::PowerChanged
    ));
}

#[test]
fn adaptive_engine_skips_appearance_only_windows_events() {
    let mut settings = Settings::default();
    settings.adaptive_engine.enabled = true;
    settings.general.accent.source = AccentColorSource::Windows;

    assert!(!windows_event_watcher_required(&settings));
    assert!(!windows_event_wake_required(
        &settings,
        WindowsAutomationEvent::AppearanceChanged
    ));

    settings.app_suspension.enabled = true;
    settings
        .app_suspension
        .suspendable_apps
        .push(app_suspension_rule(r"C:\Apps\chat.exe"));

    assert!(automation_worker_required(&settings));
    assert!(windows_event_watcher_required(&settings));
    assert!(windows_event_wake_required(
        &settings,
        WindowsAutomationEvent::WindowCreated
    ));
    assert!(!windows_event_wake_required(
        &settings,
        WindowsAutomationEvent::AppearanceChanged
    ));

    let input_events = InputHookEvents {
        app_switch: true,
        mouse_click: true,
        ..InputHookEvents::default()
    };
    assert!(input_hook_should_check_app_switch(&settings, input_events));
    assert!(input_hook_should_check_app_switch_mouse_click(
        &settings,
        input_events
    ));
}

#[test]
fn event_driven_power_checks_drop_idle_polling_for_foreground_only_rules() {
    let mut settings = Settings::default();
    settings.by_activity.enabled = false;
    settings.by_foreground.enabled = true;
    settings.by_foreground.rules.push(ByForegroundRule {
        enabled: true,
        name: "chat.exe".to_owned(),
        executable_path: r"C:\Apps\chat.exe".to_owned(),
        power_plan_guid: Some("active-guid".to_owned()),
    });

    assert!(power_plan_checks_required(&settings));
    assert!(windows_event_watcher_required(&settings));
    assert!(hidden_power_plan_check_delay(&settings, true).is_none());
    assert!(hidden_power_plan_check_delay(&settings, false).is_some());
}

#[test]
fn hidden_activity_input_resume_waits_for_hook_event() {
    let mut settings = Settings::default();
    settings.by_activity.enabled = true;
    settings.by_activity.power_plans.performance_guid = Some("active-guid".to_owned());

    assert!(power_plan_checks_required(&settings));
    assert!(windows_event_watcher_required(&settings));
    assert!(hidden_power_plan_check_delay(&settings, true).is_none());
    assert!(hidden_power_plan_check_delay(&settings, false).is_some());
}

#[test]
fn configured_check_interval_clamps_imported_values() {
    let mut settings = Settings::default();
    settings.general.check_interval_ms = 0;
    assert_eq!(
        configured_check_interval(&settings),
        Duration::from_millis(CHECK_INTERVAL_MIN_MS)
    );

    settings.general.check_interval_ms = u64::MAX;
    assert_eq!(
        configured_check_interval(&settings),
        Duration::from_millis(CHECK_INTERVAL_MAX_MS)
    );
}

#[test]
fn hidden_schedule_checks_sleep_until_next_time_boundary() {
    let mut settings = Settings::default();
    settings.by_activity.enabled = false;
    settings.by_time.enabled = true;
    let starts_at = Local::now() + ChronoDuration::minutes(3);
    let ends_at = starts_at + ChronoDuration::minutes(1);
    settings.by_time.rules = vec![ByTimeRule {
        enabled: true,
        name: "Soon".to_owned(),
        days: vec![WeekdaySetting::from_chrono(starts_at.weekday())],
        start_time: starts_at.format("%H:%M").to_string(),
        end_time: ends_at.format("%H:%M").to_string(),
        power_plan_guid: Some("scheduled-guid".to_owned()),
    }];

    let delay = hidden_power_plan_check_delay(&settings, true).unwrap();

    assert!(delay > configured_check_interval(&settings));
    assert!(delay <= Duration::from_secs(180));
}

#[test]
fn hidden_schedule_checks_cap_long_sleeps() {
    let mut settings = Settings::default();
    settings.by_activity.enabled = false;
    settings.by_time.enabled = true;
    let starts_at = Local::now() + ChronoDuration::days(1);
    let ends_at = starts_at + ChronoDuration::minutes(1);
    settings.by_time.rules = vec![ByTimeRule {
        enabled: true,
        name: "Tomorrow".to_owned(),
        days: vec![WeekdaySetting::from_chrono(starts_at.weekday())],
        start_time: starts_at.format("%H:%M").to_string(),
        end_time: ends_at.format("%H:%M").to_string(),
        power_plan_guid: Some("scheduled-guid".to_owned()),
    }];

    assert_eq!(
        hidden_power_plan_check_delay(&settings, true),
        Some(SCHEDULE_RULE_MAX_SLEEP)
    );
}

#[test]
fn by_activity_polls_when_it_can_target_a_power_plan() {
    let mut settings = Settings::default();
    settings.by_activity.enabled = true;
    settings.by_activity.power_plans.power_save_guid = Some("idle-guid".to_owned());

    assert!(power_plan_checks_required(&settings));
}

#[test]
fn controller_activity_poll_requires_a_usable_plan() {
    let mut settings = Settings::default();
    settings.by_activity.enabled = true;
    settings.by_activity.power_plans.power_save_guid = None;
    settings.by_activity.power_plans.performance_guid = Some("active-guid".to_owned());
    settings.by_activity.switch_to_performance_on_resume = false;

    assert!(!controller_activity_poll_required(&settings));

    settings.by_activity.switch_to_performance_on_resume = true;

    assert!(controller_activity_poll_required(&settings));
}

#[test]
fn process_appearance_scan_runs_for_enabled_process_features() {
    let mut settings = Settings::default();
    settings.background_efficiency.enabled = true;

    assert!(process_appearance_scan_required(&settings));
    assert!(!power_plan_checks_required(&settings));
}

#[test]
fn disabled_automation_suppresses_worker_refreshes() {
    let mut settings = Settings::default();
    settings.general.enabled = false;
    settings.background_efficiency.enabled = true;

    assert!(!feature_refresh_required(
        &settings,
        settings.background_efficiency.enabled
    ));
    assert!(!process_appearance_scan_required(&settings));
    assert!(!power_plan_checks_required(&settings));
}

#[test]
fn adaptive_plan_follows_adaptive_engine_processor_policy() {
    let mut settings = Settings::default();
    settings.adaptive_engine.enabled = true;
    settings.adaptive_engine.processor_policy_enabled = true;

    assert!(adaptive_power_plan_required(&settings));

    settings.adaptive_engine.processor_policy_enabled = false;
    assert!(!adaptive_power_plan_required(&settings));
}

#[test]
fn adaptive_processor_demand_separates_hybrid_core_classes() {
    let processors = [
        LogicalProcessorInfo {
            index: 0,
            core_index: 0,
            kind: LogicalProcessorKind::Performance,
            efficiency_class: 1,
        },
        LogicalProcessorInfo {
            index: 1,
            core_index: 1,
            kind: LogicalProcessorKind::Efficiency,
            efficiency_class: 0,
        },
    ];

    let demand = adaptive_processor_demand(&[72.0, 91.0], &processors);

    assert_eq!(demand.peak_cpu_percent, None);
    assert_eq!(demand.performance_peak_cpu_percent, Some(72.0));
    assert_eq!(demand.efficiency_peak_cpu_percent, Some(91.0));
}

#[test]
fn adaptive_plan_uses_fast_cpu_and_slow_aggregate_telemetry() {
    let mut settings = Settings::default();
    settings.adaptive_engine.enabled = true;
    settings.adaptive_engine.processor_policy_enabled = true;

    assert_eq!(
        workload_refresh_interval(&settings, true, true),
        WORKLOAD_ENGINE_FAST_REFRESH_INTERVAL
    );
    assert!(ADAPTIVE_IO_REFRESH_INTERVAL > WORKLOAD_ENGINE_FAST_REFRESH_INTERVAL);
    assert!(
        workload_refresh_interval(&Settings::default(), true, true)
            >= ADAPTIVE_ENGINE_AUTOMATION_REFRESH_INTERVAL
    );
}

#[test]
fn workload_engine_requires_adaptive_engine() {
    let mut settings = Settings::default();
    settings.general.enabled = true;
    settings.workload_engine.enabled = true;
    settings.workload_engine.workload_engine_enabled = true;

    assert!(!workload_engine_required(&settings));
    assert!(!workload_engine_priority_assist_required(&settings));

    settings.adaptive_engine.enabled = true;
    assert!(workload_engine_required(&settings));
    assert!(workload_engine_priority_assist_required(&settings));
}
#[test]
fn power_plan_checks_sleep_when_decision_features_are_off() {
    let mut settings = Settings::default();
    settings.by_activity.enabled = false;
    settings.by_foreground.enabled = false;
    settings.by_time.enabled = false;
    settings.by_cpu_load.enabled = false;
    settings.by_running_app.enabled = false;

    assert!(!power_plan_checks_required(&settings));
}
