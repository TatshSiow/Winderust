use super::*;
use crate::config::{ProcessMemoryPrioritySetting, WorkloadEngineSettings};

#[test]
fn repeated_failures_suppress_future_workload_engine_attempts_once() {
    let mut manager = WorkloadEngineManager::default();
    let mut log = ActionLog::new(8);

    manager.record_process_failure("APP.exe");
    manager.record_process_failure("app.exe");
    assert!(!manager.is_process_suppressed(42, "app.exe", &mut log, &mut BTreeSet::new()));
    assert!(log.entries().is_empty());

    manager.record_process_failure("app.exe");
    assert!(manager.is_process_suppressed(42, "app.exe", &mut log, &mut BTreeSet::new()));
    assert!(manager.is_process_suppressed(43, "APP.exe", &mut log, &mut BTreeSet::new()));

    let entries = log.entries();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].process_name, "app.exe");
    assert_eq!(entries[0].action, ActionLogAction::Skip);
    assert_eq!(entries[0].result, ActionLogResult::Skipped);
}

#[test]
fn priority_mapping_uses_safe_classes() {
    assert_eq!(
        process_priority_class(ProcessPriority::Normal),
        NORMAL_PRIORITY_CLASS
    );
    assert_eq!(
        process_priority_class(ProcessPriority::BelowNormal),
        BELOW_NORMAL_PRIORITY_CLASS
    );
    assert_eq!(
        process_priority_class(ProcessPriority::Idle),
        IDLE_PRIORITY_CLASS
    );
    assert_eq!(
        foreground_boost_priority_class(ForegroundBoostPriority::Auto, None),
        ABOVE_NORMAL_PRIORITY_CLASS
    );
    assert_eq!(
        foreground_boost_priority_class(ForegroundBoostPriority::Auto, Some(85.0)),
        NORMAL_PRIORITY_CLASS
    );
    assert_eq!(
        foreground_boost_priority_class(ForegroundBoostPriority::AboveNormal, Some(100.0)),
        ABOVE_NORMAL_PRIORITY_CLASS
    );
}

#[test]
fn foreground_launch_boost_window_includes_new_processes_only() {
    assert!(process_age_in_launch_boost_window(
        Duration::from_secs(2),
        FOREGROUND_LAUNCH_BOOST_WINDOW,
    ));
    assert!(process_age_in_launch_boost_window(
        FOREGROUND_LAUNCH_BOOST_WINDOW,
        FOREGROUND_LAUNCH_BOOST_WINDOW,
    ));
    assert!(!process_age_in_launch_boost_window(
        FOREGROUND_LAUNCH_BOOST_WINDOW + Duration::from_millis(1),
        FOREGROUND_LAUNCH_BOOST_WINDOW,
    ));
}

#[test]
fn launch_boost_runs_for_any_workload_engine_preset_while_app_is_launching() {
    let settings = WorkloadEngineSettings {
        workload_engine_enabled: true,
        boost_foreground_app: false,
        ..Default::default()
    };

    assert!(workload_engine_launch_boost_enabled(&settings, true));
    assert!(!workload_engine_launch_boost_enabled(&settings, false));
    assert!(!workload_engine_launch_boost_enabled(
        &WorkloadEngineSettings {
            workload_engine_enabled: false,
            ..settings.clone()
        },
        true
    ));
}

#[test]
fn workload_engine_status_reports_launch_boost_before_cpu_waiting() {
    let settings = WorkloadEngineSettings {
        workload_engine_enabled: true,
        ..Default::default()
    };

    assert_eq!(
        workload_engine_status_message(&settings, None, None, true, false, 0),
        "Launch boost active: boosting the foreground app while background restraints wait."
    );
}

#[test]
fn background_apply_summary_message_uses_process_count() {
    assert_eq!(
        background_apply_summary_message(1),
        "Applied Workload Engine background restraint to 1 process."
    );
    assert_eq!(
        background_apply_summary_message(3),
        "Applied Workload Engine background restraint to 3 processes."
    );
}

#[test]
fn workload_engine_restore_summary_messages_use_process_count() {
    assert_eq!(
            background_priority_restore_summary_message(1, "process no longer matches a Workload Engine rule"),
            "Restored background priority for 1 process: process no longer matches a Workload Engine rule."
        );
    assert_eq!(
            background_priority_restore_summary_message(20, "process no longer matches a Workload Engine rule"),
            "Restored background priority for 20 processes: process no longer matches a Workload Engine rule."
        );
    assert_eq!(
            foreground_boost_restore_summary_message(17, "foreground app changed before stability delay"),
            "Restored foreground boost for 17 processes: foreground app changed before stability delay."
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
fn matching_rule_is_case_insensitive() {
    let settings = WorkloadEngineSettings {
        enabled: true,
        lower_background_apps: true,
        workload_engine_background_efficiency_enabled: true,
        workload_engine_background_priority: ProcessPriority::BelowNormal,
        lower_background_io_priority_enabled: false,
        lower_background_io_priority: crate::config::ProcessIoPriority::VeryLow,
        workload_engine_io_priority: crate::config::IoPrioritySettings::default(),
        workload_engine_thread_priority: crate::config::ThreadPrioritySettings::default(),
        workload_engine_dynamic_priority_boost:
            crate::config::DynamicPriorityBoostSettings::default(),
        workload_engine_gpu_priority: crate::config::GpuPrioritySettings::default(),
        workload_engine_memory_priority_enabled: false,
        workload_engine_foreground_memory_priority: ProcessMemoryPrioritySetting::Default,
        workload_engine_memory_priority: crate::config::ProcessMemoryPriority::Low,
        lower_background_auto_cpu_percent: false,
        workload_engine_enabled: false,
        workload_engine_advanced_settings_enabled: false,
        workload_engine_affinity_escalation_enabled: false,
        workload_engine_affinity_mode: CpuRestrictionMode::SoftCpuSets,
        workload_engine_cpu_percent: 50,
        workload_engine_max_logical_processors: 0,
        workload_engine_total_threshold_percent: 70,
        workload_engine_threshold_percent: 25,
        workload_engine_restore_threshold_percent: 5,
        workload_engine_sustain_seconds: 2,
        workload_engine_minimum_restraint_seconds: 4,
        workload_engine_cooldown_seconds: 10,
        workload_engine_max_targeted_processes: 6,
        workload_engine_exclusions: Vec::new(),
        boost_foreground_app: false,
        foreground_boost: ForegroundBoostPriority::AboveNormal,
        foreground_stability_delay_ms: 750,
        rules: vec![PriorityRule {
            enabled: true,
            process_name: " Worker.EXE ".to_owned(),
            priority: ProcessPriority::BelowNormal,
        }],
    };

    assert!(matching_rule(&settings, "worker.exe").is_some());
    assert!(matching_rule(&settings, "other.exe").is_none());
}

#[test]
fn builtin_exclusions_cover_system_shell_processes() {
    assert!(is_builtin_excluded("explorer.exe"));
    assert!(is_builtin_excluded("winlogon.exe"));
    assert!(!is_builtin_excluded("browser.exe"));
}

#[test]
fn foreground_skip_matches_pid_or_process_name() {
    let foreground_group = BTreeSet::from([42]);
    assert!(should_skip_foreground_process(
        42,
        "helper.exe",
        Some(42),
        &foreground_group,
        Some("app.exe"),
    ));
    assert!(should_skip_foreground_process(
        99,
        "APP.EXE",
        Some(42),
        &foreground_group,
        Some("app.exe"),
    ));
    assert!(!should_skip_foreground_process(
        99,
        "other.exe",
        Some(42),
        &foreground_group,
        Some("app.exe"),
    ));
}

#[test]
fn foreground_group_includes_child_processes() {
    let processes = vec![
        ProcessInfo {
            id: 42,
            parent_id: None,
            name: "foreground.exe".to_owned(),
        },
        ProcessInfo {
            id: 99,
            parent_id: Some(42),
            name: "worker.exe".to_owned(),
        },
        ProcessInfo {
            id: 100,
            parent_id: Some(99),
            name: "helper.exe".to_owned(),
        },
        ProcessInfo {
            id: 101,
            parent_id: None,
            name: "background.exe".to_owned(),
        },
    ];

    let group = foreground_process_group_ids(&processes, Some(42));

    assert!(group.contains(&42));
    assert!(group.contains(&99));
    assert!(group.contains(&100));
    assert!(!group.contains(&101));
}

#[test]
fn workload_engine_pauses_when_foreground_saturates_cpu() {
    let settings = WorkloadEngineSettings {
        workload_engine_enabled: true,
        workload_engine_total_threshold_percent: 70,
        ..Default::default()
    };

    assert!(!workload_engine_should_run(&settings, Some(69.0), None));
    assert!(workload_engine_should_run(&settings, Some(70.0), None));
    assert!(!workload_engine_should_run(&settings, Some(85.0), None));
    assert!(!workload_engine_should_run(&settings, Some(100.0), None));
    assert!(!workload_engine_should_run(
        &settings,
        Some(85.0),
        Some(100.0)
    ));
}

#[test]
fn workload_engine_runs_under_system_cpu_pressure() {
    let settings = WorkloadEngineSettings {
        workload_engine_enabled: true,
        workload_engine_total_threshold_percent: 70,
        ..Default::default()
    };

    assert!(!workload_engine_should_run(
        &settings,
        Some(10.0),
        Some(69.0)
    ));
    assert!(workload_engine_should_run(
        &settings,
        Some(10.0),
        Some(70.0)
    ));
    assert!(workload_engine_should_run(&settings, None, Some(70.0)));
}

#[test]
fn workload_engine_pressure_uses_restore_band_before_stopping() {
    let settings = WorkloadEngineSettings {
        workload_engine_enabled: true,
        workload_engine_total_threshold_percent: 70,
        workload_engine_restore_threshold_percent: 20,
        ..Default::default()
    };
    let mut manager = WorkloadEngineManager::default();

    assert!(!manager.update_workload_engine_pressure(&settings, Some(10.0), Some(69.0)));
    assert!(manager.update_workload_engine_pressure(&settings, Some(10.0), Some(70.0)));
    assert!(manager.update_workload_engine_pressure(&settings, Some(10.0), Some(66.0)));
    assert!(!manager.update_workload_engine_pressure(&settings, Some(10.0), Some(64.0)));
}

#[test]
fn workload_engine_selects_highest_scored_candidates() {
    let max_targeted = 6;
    let candidates = (0..=u32::from(max_targeted))
        .map(|process_id| WorkloadEngineCandidate {
            process_id,
            process_name: format!("app{process_id}.exe"),
            decision: WorkloadEngineDecision::LowerPriority,
            score: process_id,
        })
        .collect::<Vec<_>>();

    let selected = select_workload_engine_candidates(candidates, max_targeted);

    assert_eq!(selected.len(), usize::from(max_targeted));
    assert!(!selected.iter().any(|candidate| candidate.process_id == 0));
}

#[test]
fn workload_engine_selection_can_replace_cooler_selected_process() {
    let now = Instant::now();
    let selected = WorkloadEngineProcess {
        process_name: "selected.exe".to_owned(),
        previous_cpu_time: None,
        last_usage_tenths: Some(100),
        high_since: Some(now - Duration::from_secs(60)),
        below_since: None,
        active_since: Some(now - Duration::from_secs(60)),
        last_reaction_millis: Some(100),
        restraint_count: 3,
        decision: Some(WorkloadEngineDecision::LowerPriority),
        active: true,
        selected: true,
    };
    let hotter = WorkloadEngineProcess {
        process_name: "hotter.exe".to_owned(),
        previous_cpu_time: None,
        last_usage_tenths: Some(900),
        high_since: Some(now),
        below_since: None,
        active_since: Some(now),
        last_reaction_millis: Some(100),
        restraint_count: 0,
        decision: Some(WorkloadEngineDecision::LowerPriority),
        active: true,
        selected: false,
    };

    let selected = select_workload_engine_candidates(
        vec![
            workload_engine_candidate(1, &selected, WorkloadEngineDecision::LowerPriority, now),
            workload_engine_candidate(2, &hotter, WorkloadEngineDecision::LowerPriority, now),
        ],
        1,
    );

    assert_eq!(selected[0].process_id, 2);
}

#[test]
fn workload_engine_status_treats_unselected_hot_process_as_watching() {
    let now = Instant::now();
    let mut manager = WorkloadEngineManager::default();
    manager.workload_engine.insert(
        42,
        WorkloadEngineProcess {
            process_name: "worker.exe".to_owned(),
            previous_cpu_time: None,
            last_usage_tenths: Some(900),
            high_since: Some(now - Duration::from_secs(2)),
            below_since: None,
            active_since: Some(now - Duration::from_secs(1)),
            last_reaction_millis: Some(100),
            restraint_count: 1,
            decision: Some(WorkloadEngineDecision::RestrictAffinity),
            active: true,
            selected: false,
        },
    );

    let statuses = manager.workload_engine_statuses(now);

    assert_eq!(statuses.len(), 1);
    assert_eq!(statuses[0].state, WorkloadEngineProcessState::Watching);
    assert_eq!(statuses[0].elapsed_seconds, Some(2));
}

#[test]
fn workload_engine_cpu_percent_relaxes_under_moderate_pressure() {
    let settings = WorkloadEngineSettings {
        lower_background_auto_cpu_percent: false,
        workload_engine_cpu_percent: 50,
        workload_engine_total_threshold_percent: 70,
        ..Default::default()
    };

    assert_eq!(workload_engine_effective_cpu_percent(&settings, None), 50);
    assert_eq!(
        workload_engine_effective_cpu_percent(&settings, Some(70.0)),
        75
    );
    assert_eq!(
        workload_engine_effective_cpu_percent(&settings, Some(77.5)),
        63
    );
    assert_eq!(
        workload_engine_effective_cpu_percent(&settings, Some(85.0)),
        50
    );
}

#[test]
fn workload_engine_auto_cpu_percent_uses_topology_behavior_floor() {
    let settings = WorkloadEngineSettings {
        lower_background_auto_cpu_percent: true,
        workload_engine_cpu_percent: 25,
        workload_engine_total_threshold_percent: 75,
        ..Default::default()
    };

    assert_eq!(
        workload_engine_minimum_cpu_percent_for_topology(&settings, true),
        70
    );
    let foreground_first = WorkloadEngineSettings {
        workload_engine_total_threshold_percent: 45,
        ..settings.clone()
    };
    assert_eq!(
        workload_engine_minimum_cpu_percent_for_topology(&foreground_first, true),
        50
    );
    assert_eq!(
        workload_engine_effective_cpu_percent_for_topology(&settings, Some(75.0), true),
        100
    );
    assert_eq!(
        workload_engine_effective_cpu_percent_for_topology(&settings, Some(80.0), true),
        85
    );

    assert_eq!(
        workload_engine_minimum_cpu_percent_for_topology(&settings, false),
        65
    );
    assert_eq!(
        workload_engine_effective_cpu_percent_for_topology(&settings, Some(75.0), false),
        100
    );
    assert_eq!(
        workload_engine_effective_cpu_percent_for_topology(&settings, Some(80.0), false),
        83
    );
}

#[test]
fn workload_engine_auto_mode_escalates_from_priority_to_affinity() {
    let settings = WorkloadEngineSettings {
        lower_background_auto_cpu_percent: true,
        workload_engine_affinity_escalation_enabled: false,
        workload_engine_sustain_seconds: 3,
        ..Default::default()
    };
    let now = Instant::now();

    assert_eq!(
        workload_engine_process_decision(&settings, Some(now), now + Duration::from_secs(2)),
        WorkloadEngineDecision::LowerPriority
    );
    assert_eq!(
        workload_engine_process_decision(&settings, Some(now), now + Duration::from_secs(3)),
        WorkloadEngineDecision::LowerPriority
    );

    let escalating_settings = WorkloadEngineSettings {
        workload_engine_affinity_escalation_enabled: true,
        ..settings.clone()
    };
    assert_eq!(
        workload_engine_process_decision(
            &escalating_settings,
            Some(now),
            now + Duration::from_secs(3)
        ),
        WorkloadEngineDecision::RestrictAffinity
    );

    let manual_settings = WorkloadEngineSettings {
        lower_background_auto_cpu_percent: false,
        ..escalating_settings
    };
    assert_eq!(
        workload_engine_process_decision(&manual_settings, Some(now), now),
        WorkloadEngineDecision::RestrictAffinity
    );
}

#[test]
fn workload_engine_fast_priority_sustain_is_immediate() {
    let fast_settings = WorkloadEngineSettings {
        lower_background_auto_cpu_percent: true,
        workload_engine_sustain_seconds: 1,
        ..Default::default()
    };
    assert_eq!(
        workload_engine_priority_sustain(&fast_settings, 0),
        Duration::ZERO
    );

    let balanced_settings = WorkloadEngineSettings {
        lower_background_auto_cpu_percent: true,
        workload_engine_sustain_seconds: 2,
        ..Default::default()
    };
    assert_eq!(
        workload_engine_priority_sustain(&balanced_settings, 0),
        Duration::from_secs(2)
    );
    assert_eq!(
        workload_engine_priority_sustain(&balanced_settings, 1),
        Duration::from_secs(1)
    );

    let manual_settings = WorkloadEngineSettings {
        lower_background_auto_cpu_percent: false,
        workload_engine_sustain_seconds: 1,
        ..Default::default()
    };
    assert_eq!(
        workload_engine_priority_sustain(&manual_settings, 1),
        Duration::from_secs(1)
    );
}

#[test]
fn smart_efficiency_auto_mode_runs_under_cpu_pressure() {
    let settings = WorkloadEngineSettings {
        lower_background_auto_cpu_percent: true,
        workload_engine_total_threshold_percent: 75,
        ..Default::default()
    };

    assert!(!smart_efficiency_should_run(&settings, None, None));
    assert!(!smart_efficiency_should_run(&settings, Some(74.0), None));
    assert!(smart_efficiency_should_run(&settings, Some(75.0), None));
    assert!(smart_efficiency_should_run(
        &settings,
        Some(10.0),
        Some(75.0)
    ));
    assert!(!smart_efficiency_should_run(
        &settings,
        Some(85.0),
        Some(100.0)
    ));

    let manual_settings = WorkloadEngineSettings {
        lower_background_auto_cpu_percent: false,
        ..settings
    };
    assert!(smart_efficiency_should_run(&manual_settings, None, None));
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

    let mask = load_aware_limited_core_mask(&processors, &usages, 50, 0).unwrap();

    assert_eq!(mask, 0b1010);
}

#[test]
fn load_aware_core_mask_prefers_efficiency_processors() {
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

    let mask = load_aware_limited_core_mask(&processors, &usages, 50, 0).unwrap();

    assert_eq!(mask, 0b0100);
}

#[test]
fn release_processes_skips_restore_when_process_identity_is_unknown() {
    let mut manager = WorkloadEngineManager::default();
    manager.adjusted.insert(
        0,
        AdjustedProcess {
            process_name: "exited.exe".to_owned(),
            creation_time: 0,
            previous_priority: NORMAL_PRIORITY_CLASS,
            applied_priority: BELOW_NORMAL_PRIORITY_CLASS,
            previous_dynamic_priority_boost_disabled: None,
            applied_dynamic_priority_boost_disabled: false,
            previous_efficiency_state: None,
            applied_background_efficiency: false,
            applied_ignore_timer_resolution: false,
        },
    );
    let mut log = ActionLog::new(8);

    let failures = manager.release_processes(&[0], Some(&BTreeMap::new()), &mut log, "test");

    assert_eq!(failures.count, 0);
    assert!(log.entries().is_empty());
    assert!(manager.adjusted.is_empty());
}

#[test]
fn process_cpu_usage_percent_scales_by_processor_count() {
    let now = Instant::now();
    let previous = ProcessCpuSample {
        cpu_time_100ns: 0,
        sampled_at: now,
    };
    let current = ProcessCpuSample {
        cpu_time_100ns: 10_000_000,
        sampled_at: now + Duration::from_secs(1),
    };

    let usage = process_cpu_usage_percent(previous, current).unwrap();

    assert!(usage > 0.0);
    assert!(usage <= 100.0);
}

#[test]
fn power_throttling_state_sets_timer_ignore_only_when_allowed() {
    let allowed = power_throttling_enabled_state(None, true);
    assert_ne!(
        allowed.StateMask & PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
        0
    );
    assert_ne!(
        allowed.StateMask & PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
        0
    );

    let blocked = power_throttling_enabled_state(None, false);
    assert_ne!(
        blocked.StateMask & PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
        0
    );
    assert_eq!(
        blocked.StateMask & PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
        0
    );
}
