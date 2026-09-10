use super::*;
use crate::config::{CpuSchedulerSettings, ProcessPrioritySetting};
use windows_sys::Win32::System::Threading::{
    ABOVE_NORMAL_PRIORITY_CLASS, BELOW_NORMAL_PRIORITY_CLASS, HIGH_PRIORITY_CLASS,
    IDLE_PRIORITY_CLASS, NORMAL_PRIORITY_CLASS, REALTIME_PRIORITY_CLASS,
};

#[test]
fn adaptive_efficiency_uses_the_configured_process_tier() {
    let mut settings = CpuSchedulerSettings {
        background_efficiency_mode: false,
        focus_process_background_efficiency_mode: true,
        visible_window_background_efficiency_mode: true,
        ..Default::default()
    };
    assert!(settings.background_efficiency_mode_for(true, false));
    assert!(settings.background_efficiency_mode_for(false, true));
    assert!(!settings.background_efficiency_mode_for(false, false));
    settings.focus_process_background_efficiency_override_enabled = false;
    assert!(!settings.background_efficiency_mode_for(true, false));
}

#[test]
fn repeated_failures_suppress_future_cpu_scheduler_attempts_once() {
    let mut manager = CpuSchedulerManager::default();
    let mut log = ActionLog::new(8);
    let executable_path = r"C:\Apps\app.exe";

    manager.record_process_failure(executable_path);
    manager.record_process_failure(r"C:/Apps/app.exe");
    assert!(!manager.is_process_suppressed(42, executable_path, &mut log, &mut BTreeSet::new()));
    assert!(log.entries().is_empty());

    manager.record_process_failure(executable_path);
    assert!(manager.is_process_suppressed(42, executable_path, &mut log, &mut BTreeSet::new()));
    assert!(manager.is_process_suppressed(43, r"C:/Apps/app.exe", &mut log, &mut BTreeSet::new()));

    let entries = log.entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].process_name, executable_path);
    assert_eq!(entries[0].result, ActionLogResult::Skipped);
}

#[test]
fn priority_mapping_matches_configured_classes() {
    assert_eq!(
        cpu_scheduler_priority_value(ProcessPrioritySetting::Normal)
            .unwrap()
            .raw(),
        NORMAL_PRIORITY_CLASS
    );
    assert_eq!(
        cpu_scheduler_priority_value(ProcessPrioritySetting::BelowNormal)
            .unwrap()
            .raw(),
        BELOW_NORMAL_PRIORITY_CLASS
    );
    assert_eq!(
        cpu_scheduler_priority_value(ProcessPrioritySetting::Idle)
            .unwrap()
            .raw(),
        IDLE_PRIORITY_CLASS
    );
    assert_eq!(
        cpu_scheduler_priority_value(ProcessPrioritySetting::AboveNormal)
            .unwrap()
            .raw(),
        ABOVE_NORMAL_PRIORITY_CLASS
    );
    assert_eq!(
        cpu_scheduler_priority_value(ProcessPrioritySetting::Default),
        None
    );
    assert_eq!(
        cpu_scheduler_priority_value(ProcessPrioritySetting::High)
            .unwrap()
            .raw(),
        HIGH_PRIORITY_CLASS
    );
    assert_eq!(
        cpu_scheduler_priority_value(ProcessPrioritySetting::Realtime)
            .unwrap()
            .raw(),
        REALTIME_PRIORITY_CLASS
    );
}

#[test]
fn cpu_pressure_restraint_respects_tiers_and_existing_ownership() {
    let settings = CpuSchedulerSettings {
        process_priority_enabled: true,
        background_priority: ProcessPrioritySetting::BelowNormal,
        visible_window_priority: ProcessPrioritySetting::Normal,
        background_efficiency_enabled: true,
        visible_window_background_efficiency_mode: true,
        background_efficiency_mode: true,
        ..Default::default()
    };

    let visible =
        cpu_pressure_restraint_target(&settings, CpuSchedulerTier::VisibleWindow, false).unwrap();
    assert_eq!(visible.priority.unwrap().raw(), NORMAL_PRIORITY_CLASS);
    assert!(visible.apply_background_efficiency);

    let background =
        cpu_pressure_restraint_target(&settings, CpuSchedulerTier::Background, true).unwrap();
    assert_eq!(
        background.priority.unwrap().raw(),
        BELOW_NORMAL_PRIORITY_CLASS
    );
    assert!(!background.apply_background_efficiency);
}

#[test]
fn focus_and_launch_profile_window_includes_new_processes_only() {
    assert!(process_age_in_focus_and_launch_window(
        Duration::from_secs(2),
        FOCUS_AND_LAUNCH_PROFILE_WINDOW,
    ));
    assert!(process_age_in_focus_and_launch_window(
        FOCUS_AND_LAUNCH_PROFILE_WINDOW,
        FOCUS_AND_LAUNCH_PROFILE_WINDOW,
    ));
    assert!(!process_age_in_focus_and_launch_window(
        FOCUS_AND_LAUNCH_PROFILE_WINDOW + Duration::from_millis(1),
        FOCUS_AND_LAUNCH_PROFILE_WINDOW,
    ));
}

#[test]
fn focus_and_launch_profile_active_runs_for_any_cpu_scheduler_preset_while_app_is_launching() {
    let settings = CpuSchedulerSettings {
        cpu_pressure_restraint_enabled: true,
        ..Default::default()
    };

    assert!(focus_and_launch_profile_enabled(&settings, true));
    assert!(!focus_and_launch_profile_enabled(&settings, false));
    assert!(!focus_and_launch_profile_enabled(
        &CpuSchedulerSettings {
            cpu_pressure_restraint_enabled: false,
            ..settings.clone()
        },
        true
    ));
}

#[test]
fn background_apply_summary_message_uses_process_count() {
    assert_eq!(
        background_apply_summary_message(1),
        "Applied CPU Scheduler restraint to 1 process."
    );
    assert_eq!(
        background_apply_summary_message(3),
        "Applied CPU Scheduler restraint to 3 processes."
    );
}

#[test]
fn background_apply_summary_log_is_rate_limited() {
    let now = Instant::now();

    assert!(background_apply_summary_log_due(None, now));
    assert!(!background_apply_summary_log_due(Some(now), now));
    assert!(!background_apply_summary_log_due(
        Some(now),
        now + BACKGROUND_APPLY_SUMMARY_LOG_INTERVAL - Duration::from_millis(1)
    ));
    assert!(background_apply_summary_log_due(
        Some(now),
        now + BACKGROUND_APPLY_SUMMARY_LOG_INTERVAL
    ));
}

#[test]
fn builtin_exclusions_cover_system_shell_processes() {
    assert!(is_builtin_excluded("explorer.exe"));
    assert!(is_builtin_excluded("winlogon.exe"));
    assert!(!is_builtin_excluded("browser.exe"));
}

#[test]
fn foreground_skip_matches_pid_or_process_group() {
    let foreground_group = BTreeSet::from([42, 43]);
    assert!(should_skip_foreground_process(
        42,
        Some(42),
        &foreground_group
    ));
    assert!(should_skip_foreground_process(
        43,
        Some(42),
        &foreground_group
    ));
    assert!(!should_skip_foreground_process(
        99,
        Some(42),
        &foreground_group
    ));
}

#[test]
fn foreground_group_includes_child_processes() {
    let processes = vec![
        ProcessInfo {
            id: 42,
            creation_time: Some(1),
            parent_id: None,
            session_id: None,
            user_name: None,
            is_service_account: None,
            is_critical: Some(false),
            can_set_information: true,
            name: "foreground.exe".to_owned(),
            image_path: Some(PathBuf::from("foreground.exe".to_owned())),
        },
        ProcessInfo {
            id: 99,
            creation_time: Some(1),
            parent_id: Some(42),
            session_id: None,
            user_name: None,
            is_service_account: None,
            is_critical: Some(false),
            can_set_information: true,
            name: "worker.exe".to_owned(),
            image_path: Some(PathBuf::from("worker.exe".to_owned())),
        },
        ProcessInfo {
            id: 100,
            creation_time: Some(1),
            parent_id: Some(99),
            session_id: None,
            user_name: None,
            is_service_account: None,
            is_critical: Some(false),
            can_set_information: true,
            name: "helper.exe".to_owned(),
            image_path: Some(PathBuf::from("helper.exe".to_owned())),
        },
        ProcessInfo {
            id: 101,
            creation_time: Some(1),
            parent_id: None,
            session_id: None,
            user_name: None,
            is_service_account: None,
            is_critical: Some(false),
            can_set_information: true,
            name: "background.exe".to_owned(),
            image_path: Some(PathBuf::from("background.exe".to_owned())),
        },
    ];

    let group = foreground_process_group_ids(&processes, Some(42));

    assert!(group.contains(&42));
    assert!(group.contains(&99));
    assert!(group.contains(&100));
    assert!(!group.contains(&101));
}

#[test]
fn cpu_scheduler_keeps_relative_restraints_when_foreground_saturates_cpu() {
    let settings = CpuSchedulerSettings {
        cpu_pressure_restraint_enabled: true,
        foreground_or_system_cpu_threshold_percent: 70,
        ..Default::default()
    };

    assert!(!cpu_pressure_restraint_should_run(
        &settings,
        Some(69.0),
        None
    ));
    assert!(cpu_pressure_restraint_should_run(
        &settings,
        Some(70.0),
        None
    ));
    assert!(cpu_pressure_restraint_should_run(
        &settings,
        Some(85.0),
        None
    ));
    assert!(cpu_pressure_restraint_should_run(
        &settings,
        Some(100.0),
        None
    ));
    assert!(cpu_pressure_restraint_should_run(
        &settings,
        Some(85.0),
        Some(100.0)
    ));
}

#[test]
fn cpu_scheduler_runs_under_system_cpu_pressure() {
    let settings = CpuSchedulerSettings {
        cpu_pressure_restraint_enabled: true,
        foreground_or_system_cpu_threshold_percent: 70,
        ..Default::default()
    };

    assert!(!cpu_pressure_restraint_should_run(
        &settings,
        Some(10.0),
        Some(69.0)
    ));
    assert!(cpu_pressure_restraint_should_run(
        &settings,
        Some(10.0),
        Some(70.0)
    ));
    assert!(cpu_pressure_restraint_should_run(
        &settings,
        None,
        Some(70.0)
    ));
}

#[test]
fn cpu_pressure_restraint_uses_recovery_band_before_stopping() {
    let settings = CpuSchedulerSettings {
        cpu_pressure_restraint_enabled: true,
        foreground_or_system_cpu_threshold_percent: 70,
        cpu_recovery_threshold_percent: 20,
        ..Default::default()
    };
    let mut manager = CpuSchedulerManager::default();

    assert!(!manager.update_background_pressure(&settings, Some(10.0), Some(69.0)));
    assert!(manager.update_background_pressure(&settings, Some(10.0), Some(70.0)));
    assert!(manager.update_background_pressure(&settings, None, None));
    assert!(manager.update_background_pressure(&settings, Some(10.0), Some(66.0)));
    assert!(manager.update_background_pressure(&settings, Some(10.0), Some(65.0)));
    assert!(manager.update_background_pressure(&settings, Some(85.0), Some(65.0)));
    assert!(!manager.update_background_pressure(&settings, Some(10.0), Some(64.9)));
}

#[test]
fn cpu_scheduler_selects_highest_scored_candidates() {
    let max_targeted = 6;
    let candidates = (0..=u32::from(max_targeted))
        .map(|process_id| CpuSchedulerCandidate {
            process_id,
            process_name: format!("app{process_id}.exe"),
            decision: CpuSchedulerDecision::LowerPriority,
            tier: CpuSchedulerTier::Background,
            score: process_id,
        })
        .collect::<Vec<_>>();

    let selected = select_cpu_scheduler_candidates(candidates, max_targeted);

    assert_eq!(selected.len(), usize::from(max_targeted));
    assert!(!selected.iter().any(|candidate| candidate.process_id == 0));
}

#[test]
fn cpu_scheduler_selection_can_replace_cooler_selected_process() {
    let now = Instant::now();
    let selected = CpuSchedulerProcess {
        process_name: "selected.exe".to_owned(),
        executable_path: r"C:\Apps\selected.exe".to_owned(),
        creation_time: 1,
        previous_cpu_time: None,
        last_usage_tenths: Some(100),
        high_since: Some(now - Duration::from_secs(60)),
        below_since: None,
        active_since: Some(now - Duration::from_secs(60)),
        decision: Some(CpuSchedulerDecision::LowerPriority),
        active: true,
        selected: true,
    };
    let hotter = CpuSchedulerProcess {
        process_name: "hotter.exe".to_owned(),
        executable_path: r"C:\Apps\hotter.exe".to_owned(),
        creation_time: 2,
        previous_cpu_time: None,
        last_usage_tenths: Some(900),
        high_since: Some(now),
        below_since: None,
        active_since: Some(now),
        decision: Some(CpuSchedulerDecision::LowerPriority),
        active: true,
        selected: false,
    };

    let selected = select_cpu_scheduler_candidates(
        vec![
            cpu_scheduler_candidate(
                1,
                &selected,
                CpuSchedulerDecision::LowerPriority,
                CpuSchedulerTier::Background,
            ),
            cpu_scheduler_candidate(
                2,
                &hotter,
                CpuSchedulerDecision::LowerPriority,
                CpuSchedulerTier::Background,
            ),
        ],
        1,
    );

    assert_eq!(selected[0].process_id, 2);
}

#[test]
fn cpu_scheduler_candidate_score_uses_only_cpu_and_selection_stickiness() {
    let now = Instant::now();
    let process = |selected| CpuSchedulerProcess {
        process_name: "worker.exe".to_owned(),
        executable_path: r"C:\Apps\worker.exe".to_owned(),
        creation_time: 1,
        previous_cpu_time: None,
        last_usage_tenths: Some(100),
        high_since: Some(now),
        below_since: None,
        active_since: Some(now),
        decision: Some(CpuSchedulerDecision::LowerPriority),
        active: true,
        selected,
    };

    let first = cpu_scheduler_candidate(
        1,
        &process(false),
        CpuSchedulerDecision::LowerPriority,
        CpuSchedulerTier::Background,
    );
    let repeated = cpu_scheduler_candidate(
        1,
        &process(false),
        CpuSchedulerDecision::LowerPriority,
        CpuSchedulerTier::Background,
    );

    assert_eq!(first.score, repeated.score);
    let selected = cpu_scheduler_candidate(
        1,
        &process(true),
        CpuSchedulerDecision::LowerPriority,
        CpuSchedulerTier::Background,
    );
    assert_eq!(
        selected.score,
        first.score + CPU_SCHEDULER_SELECTION_STICKINESS_TENTHS
    );
}

#[test]
fn limit_background_processors_is_immediate_for_background_only() {
    let settings = CpuSchedulerSettings {
        cpu_pressure_restraint_enabled: true,
        limit_background_processors_enabled: true,
        ..Default::default()
    };

    assert_eq!(
        cpu_scheduler_process_decision(&settings, CpuSchedulerTier::VisibleWindow),
        CpuSchedulerDecision::LowerPriority
    );
    assert_eq!(
        cpu_scheduler_process_decision(&settings, CpuSchedulerTier::Background),
        CpuSchedulerDecision::LimitProcessors
    );

    let priority_only = CpuSchedulerSettings {
        limit_background_processors_enabled: false,
        ..settings
    };
    assert_eq!(
        cpu_scheduler_process_decision(&priority_only, CpuSchedulerTier::Background),
        CpuSchedulerDecision::LowerPriority
    );
}

#[test]
fn load_aware_core_mask_picks_low_load_standard_processors() {
    let processors = vec![
        LogicalProcessorInfo {
            index: 0,
            core_index: 0,
            kind: LogicalProcessorKind::Standard,
            efficiency_class: 0,
        },
        LogicalProcessorInfo {
            index: 1,
            core_index: 1,
            kind: LogicalProcessorKind::Standard,
            efficiency_class: 0,
        },
        LogicalProcessorInfo {
            index: 2,
            core_index: 2,
            kind: LogicalProcessorKind::Standard,
            efficiency_class: 0,
        },
        LogicalProcessorInfo {
            index: 3,
            core_index: 3,
            kind: LogicalProcessorKind::Standard,
            efficiency_class: 0,
        },
    ];
    let usages = vec![80.0, 10.0, 70.0, 20.0];

    let mask = load_aware_limited_core_mask(&processors, &usages, 50, None).unwrap();

    assert_eq!(mask, 0b1010);
}

#[test]
fn processor_limiting_detects_pressure_without_priority_restraint() {
    let settings = CpuSchedulerSettings {
        cpu_pressure_restraint_enabled: false,
        limit_background_processors_enabled: true,
        foreground_or_system_cpu_threshold_percent: 70,
        ..Default::default()
    };

    assert!(cpu_pressure_restraint_should_run(
        &settings,
        None,
        Some(70.0)
    ));
}

#[test]
fn dynamic_resource_zones_keep_foreground_and_background_disjoint() {
    assert_eq!(dynamic_background_zone_percent(75), 25);
    assert_eq!(dynamic_background_zone_percent(100), 1);
    assert_eq!(
        dynamic_resource_zone_masks(0xFFFF, 0xF000),
        Some((0x0FFF, 0xF000))
    );
    assert_eq!(dynamic_resource_zone_masks(0xFFFF, 0xFFFF), None);
    assert_eq!(dynamic_resource_zone_masks(0xFFFF, 0), None);
}

#[test]
fn load_aware_core_mask_respects_all_performance_and_efficiency_pools() {
    let processors = vec![
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
        LogicalProcessorInfo {
            index: 2,
            core_index: 2,
            kind: LogicalProcessorKind::Efficiency,
            efficiency_class: 0,
        },
        LogicalProcessorInfo {
            index: 3,
            core_index: 3,
            kind: LogicalProcessorKind::Performance,
            efficiency_class: 1,
        },
    ];
    let usages = vec![1.0, 90.0, 10.0, 2.0];

    assert_eq!(
        load_aware_limited_core_mask(&processors, &usages, 50, None),
        Some(0b1001)
    );
    assert_eq!(
        load_aware_limited_core_mask(
            &processors,
            &usages,
            50,
            Some(LogicalProcessorKind::Performance),
        ),
        Some(0b0001)
    );
    assert_eq!(
        load_aware_limited_core_mask(
            &processors,
            &usages,
            50,
            Some(LogicalProcessorKind::Efficiency),
        ),
        Some(0b0100)
    );
    assert_eq!(
        load_aware_limited_core_mask(
            &processors,
            &usages,
            100,
            Some(LogicalProcessorKind::Performance),
        ),
        Some(0b1001)
    );
    assert_eq!(
        load_aware_limited_core_mask(
            &processors,
            &usages,
            100,
            Some(LogicalProcessorKind::Efficiency),
        ),
        Some(0b0110)
    );
}

#[test]
fn processor_selection_resolves_topology_and_custom_masks() {
    let processors = vec![
        LogicalProcessorInfo {
            index: 0,
            core_index: 0,
            kind: LogicalProcessorKind::Performance,
            efficiency_class: 1,
        },
        LogicalProcessorInfo {
            index: 1,
            core_index: 0,
            kind: LogicalProcessorKind::Performance,
            efficiency_class: 1,
        },
        LogicalProcessorInfo {
            index: 2,
            core_index: 1,
            kind: LogicalProcessorKind::Efficiency,
            efficiency_class: 0,
        },
        LogicalProcessorInfo {
            index: 3,
            core_index: 2,
            kind: LogicalProcessorKind::Efficiency,
            efficiency_class: 0,
        },
    ];

    assert_eq!(
        selected_background_processor_mask(
            &processors,
            BackgroundProcessorSelection::EfficiencyCores,
            &[],
        ),
        Some(0b1100)
    );
    assert_eq!(
        selected_background_processor_mask(
            &processors,
            BackgroundProcessorSelection::AllCoresNoSmt,
            &[],
        ),
        Some(0b1101)
    );
    assert_eq!(
        selected_background_processor_mask(
            &processors,
            BackgroundProcessorSelection::Custom,
            &[1, 4],
        ),
        Some(0b0010)
    );
}

#[test]
fn process_cpu_demand_percent_uses_one_logical_processor_capacity() {
    let now = Instant::now();
    let previous = ProcessCpuSample {
        cpu_time_100ns: 0,
        sampled_at: now,
    };
    let current = ProcessCpuSample {
        cpu_time_100ns: 10_000_000,
        sampled_at: now + Duration::from_secs(1),
    };

    let usage = process_cpu_demand_percent(previous, current).unwrap();

    assert_eq!(usage, 100.0);
}

#[test]
fn process_detection_falls_through_and_preservation_follows_selected_tier() {
    let mut settings = CpuSchedulerSettings {
        process_priority_preserve_foreground: true,
        process_priority_preserve_visible_window: true,
        process_priority_preserve_background: true,
        ..Default::default()
    };
    assert_eq!(
        process_priority_policy(&settings, true, true),
        (
            Some(PriorityClassValue::AboveNormal),
            PriorityClassPreservation::PreserveHigherOrHighOrRealtime
        )
    );
    settings.process_priority_foreground_detection_enabled = false;
    assert_eq!(
        process_priority_policy(&settings, true, true),
        (
            Some(PriorityClassValue::Normal),
            PriorityClassPreservation::PreserveHigherOrHighOrRealtime
        )
    );
    settings.process_priority_visible_window_detection_enabled = false;
    assert_eq!(
        process_priority_policy(&settings, true, true),
        (
            Some(PriorityClassValue::BelowNormal),
            PriorityClassPreservation::PreserveLowerOrHighOrRealtime
        )
    );
    let pressure =
        cpu_pressure_restraint_target(&settings, CpuSchedulerTier::VisibleWindow, true).unwrap();
    assert_eq!(pressure.priority, Some(PriorityClassValue::BelowNormal));
    assert_eq!(
        pressure.preservation,
        PriorityClassPreservation::PreserveLowerOrHighOrRealtime
    );
    settings.process_priority_preserve_background = false;
    assert_eq!(
        process_priority_policy(&settings, true, true).1,
        PriorityClassPreservation::PreserveHighOrRealtime
    );
    settings.process_priority_enabled = false;
    assert_eq!(process_priority_policy(&settings, true, true).0, None);
}

#[test]
fn memory_detection_falls_through_without_changing_default_semantics() {
    use crate::config::ProcessMemoryPrioritySetting as Memory;
    let mut settings = CpuSchedulerSettings {
        focus_process_memory_priority: Memory::Normal,
        visible_window_memory_priority: Memory::Medium,
        background_memory_priority: Memory::Low,
        ..Default::default()
    };
    assert_eq!(
        memory_priority_policy(&settings, true, true),
        (Memory::Normal, true, false)
    );
    settings.memory_priority_foreground_detection_enabled = false;
    assert_eq!(
        memory_priority_policy(&settings, true, true),
        (Memory::Medium, false, true)
    );
    settings.memory_priority_visible_window_detection_enabled = false;
    assert_eq!(
        memory_priority_policy(&settings, true, true),
        (Memory::Low, false, false)
    );
    settings.background_memory_priority = Memory::Default;
    assert!(memory_priority_policy(&settings, false, true)
        .0
        .priority()
        .is_none());
}
