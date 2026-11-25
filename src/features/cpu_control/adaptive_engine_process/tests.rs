use super::*;
use crate::config::AdaptiveEngineProcessSettings;

#[test]
fn repeated_failures_suppress_future_adaptive_engine_process_attempts_once() {
    let mut manager = AdaptiveEngineProcessManager::default();
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
fn observed_window_owner_is_protected_without_an_executable_image() {
    let process = |id, image_path: Option<&str>| ProcessInfo {
        id,
        creation_time: image_path.map(|_| u64::from(id)),
        parent_id: None,
        session_id: Some(1),
        user_name: None,
        is_service_account: Some(false),
        is_critical: Some(false),
        can_set_information: true,
        name: "app.exe".to_owned(),
        image_path: image_path.map(PathBuf::from),
    };
    let pathless_id = u32::MAX;
    let processes = [
        process(42, Some(r"C:\Apps\Visible\app.exe")),
        process(43, Some(r"c:\apps\visible\APP.EXE")),
        process(44, Some(r"C:\Apps\Other\app.exe")),
        process(45, Some(r"C:\Apps\Foreground\app.exe")),
        process(pathless_id, None),
    ];
    let windows = Arc::new(BTreeSet::from([42, 45, pathless_id]));
    let mut executable_paths = BTreeMap::new();
    assert_eq!(
        visible_window_group_ids(
            &processes,
            &BTreeSet::from([45]),
            &windows,
            &mut executable_paths,
        ),
        BTreeSet::from([42, 43, pathless_id])
    );
    assert!(!executable_paths.contains_key(&42));
}

#[test]
fn adaptive_engine_process_keeps_relative_restraints_when_foreground_saturates_cpu() {
    let settings = AdaptiveEngineProcessSettings {
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
fn adaptive_engine_process_runs_under_system_cpu_pressure() {
    let settings = AdaptiveEngineProcessSettings {
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
    let settings = AdaptiveEngineProcessSettings {
        cpu_pressure_restraint_enabled: true,
        foreground_or_system_cpu_threshold_percent: 70,
        cpu_recovery_threshold_percent: 20,
        ..Default::default()
    };
    let mut manager = AdaptiveEngineProcessManager::default();

    assert!(!manager.update_background_pressure(&settings, Some(10.0), Some(69.0)));
    assert!(manager.update_background_pressure(&settings, Some(10.0), Some(70.0)));
    assert!(manager.update_background_pressure(&settings, None, None));
    assert!(manager.update_background_pressure(&settings, Some(10.0), Some(66.0)));
    assert!(manager.update_background_pressure(&settings, Some(10.0), Some(65.0)));
    assert!(manager.update_background_pressure(&settings, Some(85.0), Some(65.0)));
    assert!(!manager.update_background_pressure(&settings, Some(10.0), Some(64.9)));
}

#[test]
fn selected_pressure_source_controls_trigger_and_recovery() {
    use AdaptivePressureSource::{Both, Either, Foreground, System};

    for (source, trigger_inputs, recovery_inputs, rejected_inputs) in [
        (
            Either,
            [Some(70.0), None],
            [None, Some(65.0)],
            [None, Some(64.0)],
        ),
        (
            System,
            [None, Some(70.0)],
            [None, Some(65.0)],
            [Some(75.0), None],
        ),
        (
            Foreground,
            [Some(70.0), None],
            [Some(65.0), None],
            [None, Some(75.0)],
        ),
        (
            Both,
            [Some(70.0), Some(70.0)],
            [Some(65.0), Some(65.0)],
            [Some(75.0), None],
        ),
    ] {
        let settings = AdaptiveEngineProcessSettings {
            cpu_pressure_restraint_enabled: true,
            pressure_source: source,
            foreground_or_system_cpu_threshold_percent: 70,
            ..Default::default()
        };
        let mut manager = AdaptiveEngineProcessManager::default();
        assert!(!manager.update_background_pressure(
            &settings,
            recovery_inputs[0],
            recovery_inputs[1]
        ));
        assert!(manager.update_background_pressure(
            &settings,
            trigger_inputs[0],
            trigger_inputs[1]
        ));
        assert!(manager.update_background_pressure(
            &settings,
            recovery_inputs[0],
            recovery_inputs[1]
        ));
        assert!(!manager.update_background_pressure(
            &settings,
            rejected_inputs[0],
            rejected_inputs[1]
        ));
    }
}

#[test]
fn new_pressure_source_requires_fresh_trigger_instead_of_old_recovery_band() {
    let settings = AdaptiveEngineProcessSettings {
        cpu_pressure_restraint_enabled: true,
        foreground_or_system_cpu_threshold_percent: 70,
        ..Default::default()
    };
    let mut manager = AdaptiveEngineProcessManager::default();
    assert!(manager.update_background_pressure(&settings, Some(70.0), Some(65.0)));
    manager.allocation_settings = Some(settings.clone());
    assert!(!manager.update_background_pressure(
        &AdaptiveEngineProcessSettings {
            pressure_source: AdaptivePressureSource::System,
            ..settings
        },
        Some(70.0),
        Some(65.0),
    ));
}

#[test]
fn both_pressure_signals_use_their_own_trigger_and_recovery_boundaries() {
    let settings = AdaptiveEngineProcessSettings {
        cpu_pressure_restraint_enabled: true,
        pressure_source: AdaptivePressureSource::Both,
        foreground_or_system_cpu_threshold_percent: 80,
        foreground_cpu_threshold_percent: Some(30),
        ..Default::default()
    };
    let mut manager = AdaptiveEngineProcessManager::default();
    assert!(!manager.update_background_pressure(&settings, None, Some(80.0)));
    assert!(!manager.update_background_pressure(&settings, Some(30.0), None));
    assert!(!manager.update_background_pressure(&settings, Some(29.9), Some(80.0)));
    assert!(!manager.update_background_pressure(&settings, Some(30.0), Some(79.9)));
    assert!(manager.update_background_pressure(&settings, Some(30.0), Some(80.0)));
    assert!(manager.update_background_pressure(&settings, Some(25.0), Some(75.0)));
    assert!(!manager.update_background_pressure(&settings, Some(30.0), Some(74.9)));
    assert!(manager.update_background_pressure(&settings, Some(30.0), Some(80.0)));
    assert!(!manager.update_background_pressure(&settings, Some(24.9), Some(80.0)));
}

#[test]
fn either_pressure_signal_can_maintain_an_existing_qualified_gate() {
    let settings = AdaptiveEngineProcessSettings {
        cpu_pressure_restraint_enabled: true,
        foreground_or_system_cpu_threshold_percent: 80,
        foreground_cpu_threshold_percent: Some(30),
        ..Default::default()
    };
    let mut manager = AdaptiveEngineProcessManager::default();
    assert!(!manager.update_background_pressure(&settings, None, Some(75.0)));
    assert!(manager.update_background_pressure(&settings, Some(30.0), None));
    assert!(manager.update_background_pressure(&settings, None, Some(75.0)));
    assert!(!manager.update_background_pressure(&settings, None, Some(74.9)));
    assert!(manager.update_background_pressure(&settings, None, Some(80.0)));
    assert!(manager.update_background_pressure(&settings, Some(25.0), None));
    assert!(!manager.update_background_pressure(&settings, Some(24.9), None));
}

#[test]
fn zero_foreground_usage_releases_pressure_with_a_one_percent_override() {
    let settings = AdaptiveEngineProcessSettings {
        cpu_pressure_restraint_enabled: true,
        foreground_or_system_cpu_threshold_percent: 80,
        foreground_cpu_threshold_percent: Some(1),
        ..Default::default()
    };
    let mut manager = AdaptiveEngineProcessManager::default();
    assert!(manager.update_background_pressure(&settings, Some(1.0), Some(30.0)));
    assert!(manager.update_background_pressure(&settings, Some(0.25), Some(30.0)));
    assert!(!manager.update_background_pressure(&settings, Some(0.0), Some(30.0)));
}

#[test]
fn effective_threshold_changes_require_fresh_pressure() {
    let mut settings = AdaptiveEngineProcessSettings {
        cpu_pressure_restraint_enabled: true,
        foreground_or_system_cpu_threshold_percent: 80,
        foreground_cpu_threshold_percent: Some(30),
        ..Default::default()
    };
    let mut manager = AdaptiveEngineProcessManager::default();
    assert!(manager.update_background_pressure(&settings, Some(30.0), None));
    manager.allocation_settings = Some(settings.clone());
    settings.foreground_cpu_threshold_percent = Some(35);
    assert!(!manager.update_background_pressure(&settings, Some(33.0), None));
    assert!(manager.update_background_pressure(&settings, Some(35.0), None));

    manager.allocation_settings = Some(settings.clone());
    settings.foreground_or_system_cpu_threshold_percent = 85;
    assert!(!manager.update_background_pressure(&settings, None, Some(83.0)));
    assert!(manager.update_background_pressure(&settings, None, Some(85.0)));
}

#[test]
fn unused_foreground_override_does_not_reset_system_pressure() {
    let mut settings = AdaptiveEngineProcessSettings {
        cpu_pressure_restraint_enabled: true,
        pressure_source: AdaptivePressureSource::System,
        foreground_or_system_cpu_threshold_percent: 80,
        foreground_cpu_threshold_percent: Some(30),
        ..Default::default()
    };
    let mut manager = AdaptiveEngineProcessManager::default();
    assert!(manager.update_background_pressure(&settings, None, Some(80.0)));
    manager.allocation_settings = Some(settings.clone());
    settings.foreground_cpu_threshold_percent = Some(95);
    assert!(manager.update_background_pressure(&settings, None, Some(75.0)));
}

#[test]
fn adaptive_engine_process_selects_highest_scored_candidates() {
    let max_targeted = 6;
    let candidates = (0..=u32::from(max_targeted))
        .map(|process_id| AdaptiveEngineProcessCandidate {
            process_id,
            process_name: format!("app{process_id}.exe"),
            decision: AdaptiveEngineProcessDecision::LowerPriority,
            tier: AdaptiveEngineProcessTier::Background,
            score: process_id,
        })
        .collect::<Vec<_>>();

    let selected = select_adaptive_engine_process_candidates(candidates, max_targeted);

    assert_eq!(selected.len(), usize::from(max_targeted));
    assert!(!selected.iter().any(|candidate| candidate.process_id == 0));
}

#[test]
fn adaptive_engine_process_selection_can_replace_cooler_selected_process() {
    let now = Instant::now();
    let selected = AdaptiveEngineProcessProcess {
        process_name: "selected.exe".to_owned(),
        executable_path: r"C:\Apps\selected.exe".to_owned(),
        creation_time: 1,
        tier: AdaptiveEngineProcessTier::Background,
        previous_cpu_time: None,
        last_usage_tenths: Some(100),
        pressure_restraint: PressureRestraintEpisode::default(),
        below_since: None,
        active_since: Some(now - Duration::from_secs(60)),
        selected: true,
    };
    let hotter = AdaptiveEngineProcessProcess {
        process_name: "hotter.exe".to_owned(),
        executable_path: r"C:\Apps\hotter.exe".to_owned(),
        creation_time: 2,
        tier: AdaptiveEngineProcessTier::Background,
        previous_cpu_time: None,
        last_usage_tenths: Some(900),
        pressure_restraint: PressureRestraintEpisode::default(),
        below_since: None,
        active_since: Some(now),
        selected: false,
    };

    let selected = select_adaptive_engine_process_candidates(
        vec![
            adaptive_engine_process_candidate(
                1,
                &selected,
                AdaptiveEngineProcessDecision::LowerPriority,
                AdaptiveEngineProcessTier::Background,
            ),
            adaptive_engine_process_candidate(
                2,
                &hotter,
                AdaptiveEngineProcessDecision::LowerPriority,
                AdaptiveEngineProcessTier::Background,
            ),
        ],
        1,
    );

    assert_eq!(selected[0].process_id, 2);
}

#[test]
fn adaptive_engine_process_candidate_score_uses_only_cpu_and_selection_stickiness() {
    let now = Instant::now();
    let process = |selected| AdaptiveEngineProcessProcess {
        process_name: "worker.exe".to_owned(),
        executable_path: r"C:\Apps\worker.exe".to_owned(),
        creation_time: 1,
        tier: AdaptiveEngineProcessTier::Background,
        previous_cpu_time: None,
        last_usage_tenths: Some(100),
        pressure_restraint: PressureRestraintEpisode::default(),
        below_since: None,
        active_since: Some(now),
        selected,
    };

    let first = adaptive_engine_process_candidate(
        1,
        &process(false),
        AdaptiveEngineProcessDecision::LowerPriority,
        AdaptiveEngineProcessTier::Background,
    );
    let repeated = adaptive_engine_process_candidate(
        1,
        &process(false),
        AdaptiveEngineProcessDecision::LowerPriority,
        AdaptiveEngineProcessTier::Background,
    );

    assert_eq!(first.score, repeated.score);
    let selected = adaptive_engine_process_candidate(
        1,
        &process(true),
        AdaptiveEngineProcessDecision::LowerPriority,
        AdaptiveEngineProcessTier::Background,
    );
    assert_eq!(
        selected.score,
        first.score + ADAPTIVE_ENGINE_PROCESS_SELECTION_STICKINESS_TENTHS
    );
}

#[test]
fn limit_background_processors_is_immediate_for_background_only() {
    let settings = AdaptiveEngineProcessSettings {
        cpu_pressure_restraint_enabled: true,
        limit_background_processors_enabled: true,
        ..Default::default()
    };

    assert_eq!(
        adaptive_engine_process_process_decision(
            &settings,
            AdaptiveEngineProcessTier::VisibleWindow
        ),
        AdaptiveEngineProcessDecision::LowerPriority
    );
    assert_eq!(
        adaptive_engine_process_process_decision(&settings, AdaptiveEngineProcessTier::Background),
        AdaptiveEngineProcessDecision::LimitProcessors
    );

    let priority_only = AdaptiveEngineProcessSettings {
        limit_background_processors_enabled: false,
        ..settings
    };
    assert_eq!(
        adaptive_engine_process_process_decision(
            &priority_only,
            AdaptiveEngineProcessTier::Background
        ),
        AdaptiveEngineProcessDecision::LowerPriority
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
    let settings = AdaptiveEngineProcessSettings {
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
            &CpuSelection::default(),
            &crate::cpu_topology::test_topology(),
        ),
        Some(0b1100)
    );
    assert_eq!(
        selected_background_processor_mask(
            &processors,
            BackgroundProcessorSelection::AllCoresNoSmt,
            &CpuSelection::default(),
            &crate::cpu_topology::test_topology(),
        ),
        Some(0b1101)
    );
    assert_eq!(
        selected_background_processor_mask(
            &processors,
            BackgroundProcessorSelection::Custom,
            &crate::cpu_topology::test_selection(0b10010),
            &crate::cpu_topology::test_topology(),
        ),
        None
    );
}

#[test]
fn saved_custom_processors_require_their_topology_while_current_topology_presets_remain_available()
{
    let original = crate::cpu_topology::test_topology();
    let mut members = original.processors().to_vec();
    members[0].efficiency_class = 1;
    let changed =
        CpuTopology::new(members, original.groups().to_vec(), Vec::new(), Vec::new()).unwrap();
    let processors = cpu_allocation::logical_processors_for_group_zero(&changed);
    let saved = CpuSelection::from_group_zero_mask(&original, 3).unwrap();
    assert!(saved_processor_selection_error(
        BackgroundProcessorSelection::Custom,
        &saved,
        Ok(&changed)
    )
    .is_some());
    assert_eq!(
        selected_background_processor_mask(
            &processors,
            BackgroundProcessorSelection::Custom,
            &saved,
            &changed
        ),
        None
    );
    assert_eq!(
        selected_background_processor_mask(
            &processors,
            BackgroundProcessorSelection::PerformanceCores,
            &saved,
            &changed
        ),
        Some(1)
    );
    assert!(saved_processor_selection_error(
        BackgroundProcessorSelection::PerformanceCores,
        &saved,
        Err("topology unavailable")
    )
    .is_some());
    let reviewed = saved.rebind(&changed).unwrap();
    assert_eq!(
        selected_background_processor_mask(
            &processors,
            BackgroundProcessorSelection::Custom,
            &reviewed,
            &changed
        ),
        Some(3)
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

struct AdaptiveCpuObservations {
    manager: AdaptiveEngineProcessManager,
    settings: AdaptiveEngineProcessSettings,
    started_at: Instant,
    path: &'static str,
    creation_time: u64,
    tier: AdaptiveEngineProcessTier,
}

#[test]
fn pressure_restraint_requires_sustained_hot_samples_and_clears_after_cooling() {
    let settings = AdaptiveEngineProcessSettings {
        background_app_cpu_threshold_percent: 30,
        cpu_recovery_threshold_percent: 10,
        reaction_time_ms: 500,
        restraint_trigger_time_ms: 1_500,
        minimum_restraint_time_ms: 2_100,
        cpu_recovery_time_seconds: 1,
        ..Default::default()
    };
    let start = Instant::now();
    let at = |millis| start + Duration::from_millis(millis);
    let mut episode = PressureRestraintEpisode::default();

    assert!(!episode.qualifies(&settings, at(0), 600));
    assert!(!episode.qualifies(&settings, at(500), 0));
    assert!(!episode.qualifies(&settings, at(1000), 600));
    assert!(!episode.qualifies(&settings, at(1500), 600));
    assert!(!episode.qualifies(&settings, at(2000), 600));
    assert!(!episode.qualifies(&settings, at(2499), 600));
    assert!(episode.qualifies(&settings, at(2500), 600));
    episode.selected_since = Some(at(2500));

    assert!(episode.qualifies(&settings, at(3000), 0));
    assert!(episode.qualifies(&settings, at(3500), 0));
    assert!(episode.qualifies(&settings, at(4000), 600));
    assert!(episode.qualifies(&settings, at(4500), 0));
    assert!(episode.qualifies(&settings, at(4599), 0));
    assert!(episode.below_since.is_none());
    assert!(episode.qualifies(&settings, at(4600), 0));
    assert_eq!(episode.below_since, Some(at(4600)));
    assert!(episode.qualifies(&settings, at(5599), 0));
    assert!(!episode.qualifies(&settings, at(5600), 0));
    assert!(!episode.qualifies(&settings, at(6000), 600));
}

#[test]
fn elapsed_restraint_cap_releases_hot_process_and_requires_new_hot_interval() {
    let mut settings = AdaptiveEngineProcessSettings {
        minimum_restraint_time_ms: 1_000,
        restraint_trigger_time_ms: 500,
        ..Default::default()
    };
    let start = Instant::now();
    let at = |millis| start + Duration::from_millis(millis);
    let mut episode = PressureRestraintEpisode::default();

    assert!(!episode.qualifies(&settings, at(0), 1000));
    assert!(episode.qualifies(&settings, at(500), 1000));
    episode.selected_since = Some(at(500));
    assert!(episode.qualifies(&settings, at(2000), 1000));
    // A live cap uses the elapsed time of the current claim.
    settings.maximum_restraint_time_ms = 2_100;
    assert!(episode.qualifies(&settings, at(2599), 1000));
    assert!(!episode.qualifies(&settings, at(2600), 1000));
    assert!(episode.selected_since.is_none());
    assert!(!episode.qualifies(&settings, at(2700), 1000));
    assert!(!episode.qualifies(&settings, at(3199), 1000));
    assert!(episode.qualifies(&settings, at(3200), 1000));
}

#[test]
fn scoped_placement_requires_current_verified_priority_on_the_exact_process() {
    let process = |id, creation_time, path| {
        ProcessIdentity::new(
            id,
            "worker.exe".into(),
            PathBuf::from(path),
            creation_time,
            None,
        )
    };
    let current = process(42, 2, r"C:\Apps\worker.exe");
    let old_process = process(42, 1, r"C:\Apps\worker.exe");
    let stale_path = process(42, 2, r"C:\Old\worker.exe");
    let pending_restore = process(43, 2, r"C:\Apps\worker.exe");
    let unconfirmed = process(44, 2, r"C:\Apps\worker.exe");
    let previously_verified = process(45, 2, r"C:\Apps\worker.exe");
    let explicit = process(46, 2, r"C:\Pinned\worker.exe");
    let successful = BTreeSet::from([
        current.key(),
        pending_restore.key(),
        unconfirmed.key(),
        explicit.key(),
    ]);
    let audit = [
        (&current, AdaptiveRestraintState::Verified),
        (&old_process, AdaptiveRestraintState::Verified),
        (&stale_path, AdaptiveRestraintState::Verified),
        (&pending_restore, AdaptiveRestraintState::RestorationPending),
        (&unconfirmed, AdaptiveRestraintState::ConfirmationPending),
        (&previously_verified, AdaptiveRestraintState::Verified),
        (&explicit, AdaptiveRestraintState::Verified),
    ];
    let targets = verified_restraint_allocation_targets(
        audit.into_iter(),
        &successful,
        &[r"C:\Pinned\worker.exe".into()],
        CpuAllocationMode::HardAffinity,
        &crate::cpu_topology::test_selection(1),
    );

    let [target] = targets.as_slice() else {
        panic!("expected one verified placement, got {}", targets.len());
    };
    assert_eq!(target.process_id, current.id);
    assert_eq!(target.creation_time, current.creation_time);
    assert_eq!(Path::new(&target.executable_path), current.executable_path);
}

#[test]
fn restraint_idle_pause_distinguishes_inactivity_from_unavailable_input() {
    let mut settings = AdaptiveEngineProcessSettings {
        cpu_pressure_restraint_enabled: true,
        pause_restraint_when_idle: true,
        restraint_idle_timeout_seconds: 30,
        ..Default::default()
    };
    assert_eq!(
        RestraintIdlePause::detect(&settings, Some(Duration::from_millis(29_999))),
        None
    );
    assert_eq!(
        RestraintIdlePause::detect(&settings, Some(Duration::from_secs(30))),
        Some(RestraintIdlePause::Idle)
    );
    assert_eq!(
        RestraintIdlePause::detect(&settings, None),
        Some(RestraintIdlePause::Unavailable)
    );
    settings.pause_restraint_when_idle = false;
    assert_eq!(RestraintIdlePause::detect(&settings, None), None);
    settings.pause_restraint_when_idle = true;
    settings.cpu_pressure_restraint_enabled = false;
    assert_eq!(RestraintIdlePause::detect(&settings, None), None);
}

impl AdaptiveCpuObservations {
    fn new(settings: AdaptiveEngineProcessSettings) -> Self {
        Self {
            manager: AdaptiveEngineProcessManager::default(),
            settings,
            started_at: Instant::now(),
            path: r"C:\Apps\worker.exe",
            creation_time: 1,
            tier: AdaptiveEngineProcessTier::Background,
        }
    }

    fn sample(
        &mut self,
        seconds: u64,
        cpu_time_100ns: u64,
    ) -> Option<AdaptiveEngineProcessCandidate> {
        self.observe(Some((
            ProcessCpuSample {
                cpu_time_100ns,
                sampled_at: self.started_at + Duration::from_secs(seconds),
            },
            self.creation_time,
        )))
    }

    fn observe(
        &mut self,
        sample: Option<(ProcessCpuSample, u64)>,
    ) -> Option<AdaptiveEngineProcessCandidate> {
        self.manager.update_adaptive_engine_process_sample(
            42,
            "worker.exe",
            self.path,
            &self.settings,
            self.tier,
            sample,
        )
    }
}

#[test]
fn reset_pressure_observations_requires_fresh_cpu_intervals() {
    let mut observations = AdaptiveCpuObservations::new(AdaptiveEngineProcessSettings {
        limit_background_processors_enabled: true,
        foreground_or_system_cpu_threshold_percent: 70,
        background_app_cpu_threshold_percent: 70,
        minimum_restraint_time_ms: 5_000,
        ..Default::default()
    });
    let started_at = observations.started_at;
    observations.manager.foreground_cpu_history.update(
        [(21, 1)].into_iter().collect::<BTreeMap<_, _>>().into(),
        Some(ProcessCpuSample {
            cpu_time_100ns: 10_000_000,
            sampled_at: started_at,
        }),
    );
    assert!(observations.manager.update_background_pressure(
        &observations.settings,
        Some(80.0),
        None
    ));
    assert!(observations.sample(0, 0).is_none());
    assert!(observations.sample(1, 10_000_000).is_some());
    observations
        .manager
        .tracked_processes
        .get_mut(&42)
        .unwrap()
        .selected = true;

    let monitor = &mut observations.manager.per_processor_usage;
    assert!(monitor.sample_busy_processor_for_test(0).is_none());
    assert!(monitor.sample_busy_processor_for_test(100).is_some());

    observations.manager.reset_pressure_observations();

    let monitor = &mut observations.manager.per_processor_usage;
    assert!(monitor.sample_busy_processor_for_test(200).is_none());
    assert!(monitor.sample_busy_processor_for_test(300).is_some());

    assert_eq!(
        observations.manager.foreground_cpu_history,
        ForegroundCpuHistory::default()
    );
    assert!(!observations
        .manager
        .update_background_pressure(&observations.settings, None, None));
    assert!(observations.manager.tracked_processes.is_empty());
    assert!(observations.sample(2, 10_000_000).is_none());
    assert!(observations.sample(3, 20_000_000).is_some());
}

#[test]
fn adaptive_cpu_sample_gap_discards_only_the_targets_baseline_and_recovery() {
    let mut observations = AdaptiveCpuObservations::new(AdaptiveEngineProcessSettings {
        limit_background_processors_enabled: true,
        background_app_cpu_threshold_percent: 70,
        cpu_recovery_threshold_percent: 20,
        minimum_restraint_time_ms: 0,
        cpu_recovery_time_seconds: 10,
        ..Default::default()
    });
    assert!(observations.sample(0, 0).is_none());
    assert!(observations.sample(1, 10_000_000).is_some());
    observations
        .manager
        .tracked_processes
        .get_mut(&42)
        .unwrap()
        .selected = true;
    assert_eq!(
        observations.sample(2, 10_000_000).unwrap().decision,
        AdaptiveEngineProcessDecision::LowerPriority
    );
    let unrelated = observations.manager.tracked_processes[&42].clone();
    assert!(unrelated.below_since.is_some());
    observations
        .manager
        .tracked_processes
        .insert(99, unrelated.clone());

    assert!(observations.observe(None).is_none());
    assert!(!observations.manager.tracked_processes.contains_key(&42));
    assert_eq!(
        observations.manager.tracked_processes[&99].below_since,
        unrelated.below_since
    );
    assert!(observations.manager.tracked_processes[&99]
        .active_since
        .is_some());
    assert!(observations.sample(10, 90_000_000).is_none());
    assert!(observations.manager.tracked_processes[&42]
        .active_since
        .is_none());
    assert!(observations.manager.tracked_processes[&42]
        .below_since
        .is_none());
    assert!(observations.sample(11, 100_000_000).is_some());
    assert_eq!(
        observations.manager.tracked_processes[&42].active_since,
        Some(observations.started_at + Duration::from_secs(11))
    );
}

#[test]
fn adaptive_invalid_cpu_delta_restarts_the_incident_from_a_fresh_interval() {
    for (invalid_cpu_time, invalid_seconds) in [(0, 2), (20_000_000, 1)] {
        let mut observations = AdaptiveCpuObservations::new(AdaptiveEngineProcessSettings {
            limit_background_processors_enabled: true,
            background_app_cpu_threshold_percent: 70,
            ..Default::default()
        });
        assert!(observations.sample(0, 0).is_none());
        assert!(observations.sample(1, 10_000_000).is_some());
        observations
            .manager
            .tracked_processes
            .get_mut(&42)
            .unwrap()
            .selected = true;
        assert!(observations
            .sample(invalid_seconds, invalid_cpu_time)
            .is_none());
        let state = &observations.manager.tracked_processes[&42];
        assert!(!state.selected);
        assert!(state.below_since.is_none());
        assert!(state.active_since.is_none());
        assert!(state.last_usage_tenths.is_none());

        assert!(observations
            .sample(invalid_seconds + 1, invalid_cpu_time + 10_000_000)
            .is_some());
        assert_eq!(
            observations.manager.tracked_processes[&42].active_since,
            Some(observations.started_at + Duration::from_secs(invalid_seconds + 1))
        );
    }
}

#[test]
fn adaptive_replacement_identity_does_not_reuse_the_previous_cpu_incident() {
    for (creation, path) in [(2, r"C:\Apps\worker.exe"), (1, r"C:\Other\worker.exe")] {
        let mut observations = AdaptiveCpuObservations::new(AdaptiveEngineProcessSettings {
            background_app_cpu_threshold_percent: 70,
            ..Default::default()
        });
        assert!(observations.sample(0, 0).is_none());
        assert!(observations.sample(1, 10_000_000).is_some());
        observations
            .manager
            .tracked_processes
            .get_mut(&42)
            .unwrap()
            .selected = true;
        observations.creation_time = creation;
        observations.path = path;
        assert!(observations.sample(2, 20_000_000).is_none());
        assert!(observations.manager.tracked_processes[&42]
            .active_since
            .is_none());
        assert!(observations.sample(3, 30_000_000).is_some());
        assert_eq!(
            observations.manager.tracked_processes[&42].active_since,
            Some(observations.started_at + Duration::from_secs(3))
        );
    }
}

#[test]
fn adaptive_role_changes_require_a_fresh_hot_interval() {
    use AdaptiveEngineProcessDecision::{LimitProcessors, LowerPriority};
    use AdaptiveEngineProcessTier::{Background, VisibleWindow};

    for (from, to, active_decision, next_decision) in [
        (VisibleWindow, Background, LowerPriority, LimitProcessors),
        (Background, VisibleWindow, LimitProcessors, LowerPriority),
    ] {
        let mut observations = AdaptiveCpuObservations::new(AdaptiveEngineProcessSettings {
            cpu_pressure_restraint_enabled: true,
            limit_background_processors_enabled: true,
            background_app_cpu_threshold_percent: 70,
            cpu_recovery_threshold_percent: 20,
            minimum_restraint_time_ms: 10_000,
            ..Default::default()
        });
        observations.tier = from;
        assert!(observations.sample(0, 0).is_none());
        assert_eq!(
            observations.sample(1, 10_000_000).unwrap().decision,
            active_decision
        );
        let tracked = &mut observations.manager.tracked_processes;
        let active = tracked.get_mut(&42).unwrap();
        active.selected = true;
        let unrelated = active.clone();
        let unrelated_active_since = unrelated.active_since;
        tracked.insert(99, unrelated);

        observations.tier = to;
        assert!(observations.sample(2, 11_000_000).is_none());
        assert!(observations.sample(3, 12_000_000).is_none());
        let tracked = &observations.manager.tracked_processes;
        assert!(!tracked[&42].selected);
        assert!(tracked[&42].active_since.is_none());
        assert_eq!(tracked[&99].active_since, unrelated_active_since);
        assert!(tracked[&99].selected);

        let renewed = observations.sample(4, 22_000_000).unwrap();
        assert_eq!(renewed.tier, to);
        assert_eq!(renewed.decision, next_decision);
        assert_eq!(
            observations.manager.tracked_processes[&42].active_since,
            Some(observations.started_at + Duration::from_secs(4))
        );
    }
}

#[test]
fn adaptive_complete_cpu_intervals_preserve_hold_then_priority_only_recovery() {
    let mut observations = AdaptiveCpuObservations::new(AdaptiveEngineProcessSettings {
        limit_background_processors_enabled: true,
        background_app_cpu_threshold_percent: 70,
        cpu_recovery_threshold_percent: 20,
        minimum_restraint_time_ms: 2_100,
        cpu_recovery_time_seconds: 2,
        ..Default::default()
    });
    assert!(observations.sample(0, 0).is_none());
    assert!(observations.sample(1, 10_000_000).is_some());
    observations
        .manager
        .tracked_processes
        .get_mut(&42)
        .unwrap()
        .selected = true;
    let started_at = observations.started_at;
    let creation_time = observations.creation_time;
    for (milliseconds, expected) in [
        (3_099, Some(AdaptiveEngineProcessDecision::LimitProcessors)),
        (3_100, Some(AdaptiveEngineProcessDecision::LowerPriority)),
        (5_099, Some(AdaptiveEngineProcessDecision::LowerPriority)),
        (5_100, None),
    ] {
        assert_eq!(
            observations
                .observe(Some((
                    ProcessCpuSample {
                        cpu_time_100ns: 10_000_000,
                        sampled_at: started_at + Duration::from_millis(milliseconds),
                    },
                    creation_time,
                )))
                .map(|candidate| candidate.decision),
            expected
        );
    }
    assert!(observations.manager.tracked_processes[&42]
        .active_since
        .is_none());
}

#[test]
fn zoning_alone_reads_pressure_and_masks_never_create_empty_partitions() {
    let settings = AdaptiveEngineProcessSettings {
        dynamic_resource_zones_enabled: true,
        foreground_or_system_cpu_threshold_percent: 50,
        ..Default::default()
    };
    assert!(cpu_pressure_restraint_should_run(
        &settings,
        Some(60.0),
        None
    ));
    assert!(!cpu_pressure_restraint_should_run(&settings, None, None));
    for count in [1, 2, 3, 8, 12, 16, 24] {
        let processors = (0..count)
            .map(|index| LogicalProcessorInfo {
                index,
                core_index: index,
                kind: LogicalProcessorKind::Standard,
                efficiency_class: 0,
            })
            .collect::<Vec<_>>();
        let all = cpu_allocation::logical_processor_mask(&processors);
        for share in [1, 25, 50, 75, 99] {
            let background_count = (count * usize::from(100 - share)).div_ceil(100);
            let mask =
                load_aware_limited_core_mask(&processors, &vec![0.0; count], 100 - share, None)
                    .unwrap();
            let partition = dynamic_resource_zone_masks(all, mask);
            if background_count == count {
                assert!(partition.is_none());
            } else {
                let (foreground, background) = partition.unwrap();
                assert_eq!(background.count_ones() as usize, background_count);
                assert_eq!(foreground | background, all);
                assert_eq!(foreground & background, 0);
            }
        }
        assert!(load_aware_limited_core_mask(
            &processors,
            &vec![0.0; count],
            25,
            Some(LogicalProcessorKind::Efficiency)
        )
        .is_none());
    }
}

#[test]
fn foreground_capacity_is_not_held_by_stale_or_idle_background_demand() {
    assert!(!fresh_background_competition(None, 4));
    assert!(!fresh_background_competition(Some(0), 0));
    assert!(!fresh_background_competition(Some(39), 4));
    assert!(fresh_background_competition(Some(40), 4));
}

#[test]
fn adaptive_engine_source_wait_clears_qualification_and_rearms_after_update() {
    let mut manager = AdaptiveEngineProcessManager::default();
    let mut allocation = CpuAllocationCoordinator::default();
    let mut priority = PriorityEfficiencyController::default();
    let mut log = ActionLog::new(8);
    let settings = AdaptiveEngineProcessSettings {
        reaction_time_ms: 250,
        restraint_trigger_time_ms: 250,
        ..AdaptiveEngineProcessSettings::default()
    };
    let now = Instant::now();
    let mut episode = PressureRestraintEpisode::default();
    assert!(!episode.qualifies(&settings, now, 950));
    assert!(episode.qualifies(&settings, now + Duration::from_millis(250), 950));
    manager.tracked_processes.insert(
        42,
        AdaptiveEngineProcessProcess {
            process_name: String::new(),
            executable_path: String::new(),
            creation_time: 1,
            tier: AdaptiveEngineProcessTier::Background,
            previous_cpu_time: None,
            last_usage_tenths: None,
            pressure_restraint: episode,
            below_since: None,
            active_since: None,
            selected: false,
        },
    );
    manager.background_pressure_active = true;
    let waiting = manager.pause_for_unknown_power_source(&mut allocation, &mut priority, &mut log);
    assert_eq!(waiting.source, None);
    assert!(waiting.waiting_for_power_source && waiting.observation_paused);
    assert!(manager.tracked_processes.is_empty());
    assert!(!manager.background_pressure_active);
    assert_eq!(
        manager.pause_for_unknown_power_source(&mut allocation, &mut priority, &mut log),
        waiting
    );
    let confirmed = manager.update(
        AdaptiveEngineProcessUpdate {
            settings: &settings,
            automation_enabled: false,
            allow_cross_session_process_control: false,
            foreground_process_id: None,
            total_cpu_usage_percent: None,
            user_idle_for: None,
            excluded_process_id: None,
            explicit_cpu_allocation_paths: &[],
            observations: &mut CycleObservations::default(),
        },
        &mut allocation,
        &mut priority,
        &mut log,
    );
    assert!(!confirmed.waiting_for_power_source);
    assert!(manager.source_release.is_none());
    assert!(!PressureRestraintEpisode::default().qualifies(
        &settings,
        now + Duration::from_secs(1),
        950,
    ));
}

struct DisposableProcess(std::process::Child);

impl Drop for DisposableProcess {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn disposable_restraint_target(
) -> Result<(DisposableProcess, crate::foreground::ProcessActionTarget), String> {
    use std::{os::windows::process::CommandExt, process::Command};
    use windows_sys::Win32::System::Threading::{CREATE_NO_WINDOW, NORMAL_PRIORITY_CLASS};

    let executable_path = std::env::var_os("SystemRoot")
        .map(|path| PathBuf::from(path).join(r"System32\PING.EXE"))
        .ok_or_else(|| "SystemRoot is unavailable.".to_owned())?;
    let child = DisposableProcess(
        Command::new(&executable_path)
            .args(["-t", "127.0.0.1"])
            .creation_flags(CREATE_NO_WINDOW | NORMAL_PRIORITY_CLASS)
            .spawn()
            .map_err(|error| format!("Could not start disposable process: {error}"))?,
    );
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match crate::foreground::capture_process_action_target(
            child.0.id(),
            &executable_path,
            false,
        ) {
            Ok(target) => return Ok((child, target)),
            Err(_) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(25));
            }
            Err(error) => return Err(error.to_string()),
        }
    }
}

fn restraint_target_process_info(target: &crate::foreground::ProcessActionTarget) -> ProcessInfo {
    ProcessInfo {
        id: target.id,
        name: target.name.clone(),
        creation_time: Some(target.creation_time),
        parent_id: None,
        session_id: target.session_id,
        user_name: None,
        is_service_account: target.is_service_account,
        is_critical: Some(false),
        can_set_information: true,
        image_path: Some(target.executable_path.clone()),
    }
}

fn restraint_target_observations(
    target: &crate::foreground::ProcessActionTarget,
) -> CycleObservations {
    let foreground_id = std::process::id();
    let worker = restraint_target_process_info(target);
    let foreground = ProcessInfo {
        id: foreground_id,
        name: "foreground.exe".into(),
        creation_time: Some(1),
        image_path: None,
        ..worker.clone()
    };
    CycleObservations::with_adaptive_cpu_sample(vec![foreground, worker], Some(foreground_id), None)
        .with_visible_windows(BTreeSet::new())
}

#[test]
#[ignore = "modifies and restores a disposable Windows process; run in explicit integration QA"]
fn native_pressure_restraint_idle_restores_priority() -> Result<(), String> {
    use crate::config::{AdaptiveEngineExclusionRule, ProcessPrioritySetting};
    use crate::control::priority_efficiency::current_process_priority;

    let (_child, target) = disposable_restraint_target()?;
    let read_priority =
        || current_process_priority(&target, false).map_err(|error| error.to_string());
    assert_eq!(read_priority()?, ProcessPrioritySetting::Normal);

    let mut settings = AdaptiveEngineProcessSettings {
        cpu_pressure_restraint_enabled: true,
        pause_restraint_when_idle: true,
        restraint_idle_timeout_seconds: 30,
        ..Default::default()
    };
    let mut priority = PriorityEfficiencyController::default();
    let control_target = ProcessControlTarget::from_action_target(&target);
    for (priority_class, exclusion) in [
        (PriorityClassValue::BelowNormal, None),
        (PriorityClassValue::Idle, None),
        (
            PriorityClassValue::BelowNormal,
            Some(AdaptiveEngineExclusionRule::exact_name(
                target.name.to_ascii_uppercase(),
            )),
        ),
        (
            PriorityClassValue::BelowNormal,
            Some(
                toml::from_str("executable_name_pattern = 'P?NG*.EXE'")
                    .map_err(|error| format!("Invalid fixture exclusion: {error}"))?,
            ),
        ),
    ] {
        let outcome = priority
            .apply_priority_claim(
                PriorityClassClaim {
                    target: control_target.clone(),
                    owner: ControlOwner::AdaptiveEngine,
                    priority: priority_class,
                    preservation: PriorityClassPreservation::OnlyNormal,
                },
                false,
            )
            .map_err(|error| error.to_string())?;
        assert_eq!(outcome, ProcessPropertyApplyOutcome::Applied);
        assert_eq!(read_priority()?, priority_class.setting());
        assert_eq!(
            priority.can_apply_adaptive_restraint(
                &control_target,
                false,
                Some(priority_class),
                true,
            ),
            Ok(true)
        );

        settings.custom_rules.clear();
        let exclude_by_name = exclusion.is_some();
        let (foreground_id, mut observations) = if let Some(exclusion) = exclusion {
            settings.limit_background_processors_enabled = true;
            settings.custom_rules.push(exclusion);
            (
                Some(std::process::id()),
                restraint_target_observations(&target),
            )
        } else {
            (None, CycleObservations::default())
        };
        let mut manager = AdaptiveEngineProcessManager::default();
        let snapshot = manager.update(
            AdaptiveEngineProcessUpdate {
                settings: &settings,
                automation_enabled: true,
                allow_cross_session_process_control: false,
                foreground_process_id: foreground_id,
                total_cpu_usage_percent: Some(100.0),
                user_idle_for: Some(Duration::from_secs(if exclude_by_name { 0 } else { 30 })),
                excluded_process_id: None,
                explicit_cpu_allocation_paths: &[],
                observations: &mut observations,
            },
            &mut CpuAllocationCoordinator::default(),
            &mut priority,
            &mut ActionLog::default(),
        );
        assert_eq!(
            snapshot.restraint_idle_pause,
            (!exclude_by_name).then_some(RestraintIdlePause::Idle)
        );
        assert_eq!(
            snapshot.cpu_pressure_restraint_active, exclude_by_name,
            "unexpected Adaptive Engine snapshot: {snapshot:?}"
        );
        if exclude_by_name {
            assert_eq!(snapshot.scanned_processes, 2);
            assert!(!manager.tracked_processes.contains_key(&target.id));
        }
        assert_eq!(snapshot.failed_processes, 0);
        assert_eq!(read_priority()?, ProcessPrioritySetting::Normal);
        assert!(priority.adaptive_restraint_processes().next().is_none());
    }
    Ok(())
}

#[test]
#[ignore = "modifies and restores priority on a disposable Windows process"]
fn native_pressure_restraint_focus_protections_are_independent() -> Result<(), String> {
    use crate::{
        config::ProcessPrioritySetting, control::priority_efficiency::current_process_priority,
    };

    let (_child, target) = disposable_restraint_target()?;
    for (is_focused, playing_audio, protect_app, protect_children, should_restrain) in [
        (true, false, true, false, false),
        (true, false, false, true, true),
        (false, false, false, true, false),
        (false, false, true, false, true),
        (true, true, false, true, false),
    ] {
        let mut worker = restraint_target_process_info(&target);
        let (processes, foreground_id) = if is_focused {
            (vec![worker], target.id)
        } else {
            let foreground_id = std::process::id();
            worker.parent_id = Some(foreground_id);
            let foreground = ProcessInfo {
                id: foreground_id,
                name: "foreground.exe".into(),
                creation_time: Some(1),
                image_path: None,
                parent_id: None,
                ..worker.clone()
            };
            (vec![foreground, worker], foreground_id)
        };
        let mut settings = AdaptiveEngineProcessSettings {
            cpu_pressure_restraint_enabled: true,
            protect_focused_app: protect_app,
            protect_focused_children: protect_children,
            background_app_cpu_threshold_percent: 25,
            pressure_source: AdaptivePressureSource::System,
            restraint_trigger_time_ms: 250,
            ..Default::default()
        };
        let mut manager = AdaptiveEngineProcessManager::default();
        let mut allocation = CpuAllocationCoordinator::default();
        let mut priority = PriorityEfficiencyController::default();
        let mut log = ActionLog::default();
        let started_at = Instant::now();
        for elapsed_ms in [0, 250, 500, 750] {
            if elapsed_ms == 750 {
                settings.protect_focused_app = true;
                settings.protect_focused_children = true;
            }
            let mut observations = CycleObservations::with_adaptive_cpu_sample(
                processes.clone(),
                Some(foreground_id),
                None,
            )
            .with_visible_windows(BTreeSet::from([target.id]));
            let status = manager.update_with_activity(
                AdaptiveEngineProcessUpdate {
                    settings: &settings,
                    automation_enabled: true,
                    allow_cross_session_process_control: false,
                    foreground_process_id: Some(foreground_id),
                    total_cpu_usage_percent: Some(100.0),
                    user_idle_for: Some(Duration::ZERO),
                    excluded_process_id: None,
                    explicit_cpu_allocation_paths: &[],
                    observations: &mut observations,
                },
                &mut allocation,
                &mut priority,
                &mut log,
                |id, _| {
                    (id == target.id).then_some((
                        ProcessCpuSample {
                            cpu_time_100ns: elapsed_ms * 10_000,
                            sampled_at: started_at + Duration::from_millis(elapsed_ms),
                        },
                        target.creation_time,
                    ))
                },
                || Ok(playing_audio.then_some(target.id).into_iter().collect()),
            );
            assert!(!status.observation_paused, "{status:?}");
            assert_eq!(status.failed_processes, 0, "{status:?}");
            assert_eq!(
                current_process_priority(&target, false).map_err(|error| error.to_string())?,
                if elapsed_ms == 500 && should_restrain {
                    ProcessPrioritySetting::BelowNormal
                } else {
                    ProcessPrioritySetting::Normal
                },
                "at {elapsed_ms} ms: focused {is_focused}, playing audio {playing_audio}, protect app {protect_app}, protect children {protect_children}"
            );
        }
    }
    Ok(())
}

#[test]
#[ignore = "modifies and restores priority and affinity on a disposable Windows process"]
fn native_scoped_placement_follows_verified_episodes() -> Result<(), String> {
    native_restraint_placement_lifecycle(false)
}

#[test]
#[ignore = "modifies and restores affinity on a disposable Windows process"]
fn native_placement_only_restraint_follows_episode_without_priority_changes() -> Result<(), String>
{
    native_restraint_placement_lifecycle(true)
}

fn native_restraint_placement_lifecycle(placement_only: bool) -> Result<(), String> {
    use crate::{
        config::ProcessPrioritySetting,
        control::{
            priority_efficiency::current_process_priority,
            process::open_process_for_set_information,
        },
        platform::windows::cpu_allocation::query_affinity,
    };

    let (_child, target) = disposable_restraint_target()?;
    let (_, process) =
        open_process_for_set_information(&ProcessControlTarget::from_action_target(&target), false)
            .map_err(|error| error.to_string())?;
    let read_affinity = || {
        query_affinity(&process)
            .map(|(current, _)| current)
            .map_err(|error| format!("Read test process affinity: {error:?}"))
    };
    let baseline = read_affinity()?;
    if baseline.count_ones() < 2 {
        return Err("the affinity test needs at least two available processors".into());
    }
    let expected = 1_usize << baseline.trailing_zeros();
    let topology = cpu_allocation::processor_topology()?;
    let settings = AdaptiveEngineProcessSettings {
        cpu_pressure_restraint_enabled: true,
        restraint_priority: if placement_only {
            AdaptiveRestraintPriority::ProcessorPlacementOnly
        } else {
            AdaptiveRestraintPriority::BelowNormal
        },
        limit_background_processors_enabled: !placement_only,
        limit_background_processors_during_restraint: !placement_only,
        cpu_allocation_method: CpuAllocationMethod::ProcessorAffinityHard,
        background_processor_selection: BackgroundProcessorSelection::Custom,
        specific_processors: CpuSelection::from_group_zero_mask(&topology, expected as u64)
            .map_err(|error| error.to_string())?,
        background_app_cpu_threshold_percent: 25,
        pressure_source: AdaptivePressureSource::System,
        restraint_trigger_time_ms: 250,
        minimum_restraint_time_ms: 250,
        maximum_restraint_time_ms: 250,
        pause_restraint_when_idle: true,
        restraint_idle_timeout_seconds: 30,
        ..Default::default()
    };
    let mut manager = AdaptiveEngineProcessManager::default();
    let mut allocation = CpuAllocationCoordinator::default();
    let mut priority = PriorityEfficiencyController::default();
    let mut log = ActionLog::default();
    let started_at = Instant::now();
    for (elapsed_ms, idle, restrained) in [
        (0, false, false),
        (250, false, false),
        (500, false, true),
        (749, false, true),
        (750, false, false),
        (1_000, false, false),
        (1_250, false, true),
        (1_499, true, false),
    ] {
        let status = manager.update_with_activity(
            AdaptiveEngineProcessUpdate {
                settings: &settings,
                automation_enabled: true,
                allow_cross_session_process_control: false,
                foreground_process_id: Some(std::process::id()),
                total_cpu_usage_percent: Some(100.0),
                user_idle_for: Some(Duration::from_secs(if idle { 30 } else { 0 })),
                excluded_process_id: None,
                explicit_cpu_allocation_paths: &[],
                observations: &mut restraint_target_observations(&target),
            },
            &mut allocation,
            &mut priority,
            &mut log,
            |id, _| {
                (id == target.id).then_some((
                    ProcessCpuSample {
                        cpu_time_100ns: elapsed_ms * 10_000,
                        sampled_at: started_at + Duration::from_millis(elapsed_ms),
                    },
                    target.creation_time,
                ))
            },
            || Ok(BTreeSet::new()),
        );
        assert!(!status.observation_paused, "{status:?}");
        assert_eq!(status.failed_processes, 0, "{status:?}");
        assert_eq!(
            read_affinity()?,
            if restrained { expected } else { baseline },
            "at {elapsed_ms} ms: {status:?}"
        );
        assert_eq!(
            current_process_priority(&target, false).map_err(|error| error.to_string())?,
            if restrained && !placement_only {
                ProcessPrioritySetting::BelowNormal
            } else {
                ProcessPrioritySetting::Normal
            }
        );
        assert_eq!(
            allocation
                .adaptive_episode_placement_processes()
                .map(|process| process.id)
                .collect::<Vec<_>>(),
            if restrained && placement_only {
                vec![target.id]
            } else {
                Vec::new()
            }
        );
    }
    Ok(())
}
