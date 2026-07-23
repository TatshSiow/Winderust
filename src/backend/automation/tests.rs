use super::*;
use chrono::{Datelike, Duration as ChronoDuration, Local};

use crate::config::{
    ByForegroundRule, ByTimeRule, ProcessDynamicPriorityBoostSetting, ProcessExclusionRule,
    ProcessGpuPrioritySetting, ProcessThreadPrioritySetting, WeekdaySetting,
};

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
        process_name: "editor.exe".to_owned(),
        power_plan_guid: Some("active-guid".to_owned()),
    });
    assert!(foreground_lookup_required(&settings));
}

#[test]
fn automation_worker_sleeps_when_no_automation_work_exists() {
    let settings = Settings::default();

    assert!(!automation_worker_required(&settings));
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

    update_background_efficiency_status(
        &automation.shared,
        BackgroundEfficiencySnapshot {
            auto_excluded_processes: vec!["Editor.exe".to_owned()],
            ..BackgroundEfficiencySnapshot::default()
        },
    );

    let pending = automation
        .take_pending_auto_exclusions_since(&mut generation)
        .expect("new pending exclusions should be visible");
    assert_eq!(pending.background_efficiency, vec!["editor.exe"]);
    assert!(pending.core_steering.is_empty());
    assert!(pending.background_cpu_restriction.is_empty());
    update_core_steering_status(
        &automation.shared,
        CoreSteeringSnapshot {
            auto_excluded_processes: vec!["Game.exe".to_owned()],
            ..CoreSteeringSnapshot::default()
        },
    );

    let pending = automation
        .take_pending_auto_exclusions_since(&mut generation)
        .expect("new pending affinity exclusions should be visible");
    assert_eq!(pending.core_steering, vec!["game.exe"]);
    assert!(automation
        .take_pending_auto_exclusions_since(&mut generation)
        .is_none());
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
        process_name: "game.exe".to_owned(),
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

    assert!(feature_refresh_required(
        &settings,
        settings.app_suspension.enabled
    ));
    assert!(!process_appearance_scan_required(&settings));
}

#[test]
fn app_suspension_uses_windows_events_without_enabling_process_scan() {
    let mut settings = Settings::default();
    settings.app_suspension.enabled = true;

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

    assert!(automation_worker_required(&settings));
    assert!(!windows_event_watcher_required(&settings));
    assert!(!windows_event_wake_required(
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
    assert!(!input_hook_should_check_app_switch(&settings, input_events));
    assert!(!input_hook_should_check_app_switch_mouse_click(
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
        process_name: "chat.exe".to_owned(),
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
    settings.by_activity.power_plans.performance_guid = Some("active-guid".to_owned());

    assert!(power_plan_checks_required(&settings));
    assert!(windows_event_watcher_required(&settings));
    assert!(hidden_power_plan_check_delay(&settings, true).is_none());
    assert!(hidden_power_plan_check_delay(&settings, false).is_some());
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
    settings.by_activity.power_plans.power_save_guid = Some("idle-guid".to_owned());

    assert!(power_plan_checks_required(&settings));
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
    assert_eq!(static_processor_power_values(&settings), None);

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
fn by_running_app_keeps_its_static_processor_target() {
    let mut settings = Settings::default();
    settings.general.enabled = true;
    settings.workload_engine.enabled = true;
    settings.workload_engine.workload_engine_enabled = true;
    settings.adaptive_engine.processor_policy_values = ProcessorPowerValues::new_with_boost_mode(
        100,
        25,
        100,
        85,
        crate::power::ProcessorBoostMode::EfficientAggressive,
    );

    assert_eq!(
        static_processor_power_values(&settings),
        Some(settings.adaptive_engine.processor_policy_values)
    );
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
