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
        cpu_restraint_time_seconds: 1,
        cpu_recovery_time_seconds: 1,
        ..Default::default()
    };
    let start = Instant::now();
    let at = |millis| start + Duration::from_millis(millis);
    let mut episode = PressureRestraintEpisode::default();

    assert!(!episode.qualifies(&settings, at(0), 600));
    assert!(!episode.qualifies(&settings, at(500), 0));
    assert!(!episode.qualifies(&settings, at(1000), 600));
    assert!(!episode.qualifies(&settings, at(1499), 600));
    assert!(episode.qualifies(&settings, at(1500), 600));
    episode.selected_since = Some(at(1500));

    assert!(episode.qualifies(&settings, at(2000), 0));
    assert!(episode.qualifies(&settings, at(2500), 0));
    assert!(episode.qualifies(&settings, at(3000), 600));
    assert!(episode.qualifies(&settings, at(3500), 0));
    assert!(episode.qualifies(&settings, at(4000), 0));
    assert!(!episode.qualifies(&settings, at(4500), 0));
    assert!(!episode.qualifies(&settings, at(5000), 600));
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
        cpu_restraint_time_seconds: 5,
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
        cpu_restraint_time_seconds: 0,
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
            cpu_restraint_time_seconds: 10,
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
        cpu_restraint_time_seconds: 3,
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
    for (seconds, expected) in [
        (2, Some(AdaptiveEngineProcessDecision::LimitProcessors)),
        (4, Some(AdaptiveEngineProcessDecision::LowerPriority)),
        (5, Some(AdaptiveEngineProcessDecision::LowerPriority)),
        (6, None),
    ] {
        assert_eq!(
            observations
                .sample(seconds, 10_000_000)
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
