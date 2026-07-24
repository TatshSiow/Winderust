use super::*;

#[test]
fn process_list_column_layout_fits_headers_and_values() {
    let settings = Settings::default();
    let processes = vec![
        ProcessInfo {
            id: 1234,
            parent_id: None,
            name: "editor.exe".to_owned(),
        },
        ProcessInfo {
            id: 12345,
            parent_id: None,
            name: "worker.exe".to_owned(),
        },
    ];
    let groups = process_list_groups(&processes);
    let summaries = groups
        .iter()
        .map(|_| default_process_policy_summary())
        .collect::<Vec<_>>();

    let layout = process_list_column_layout(&settings, &groups, &summaries);

    assert!(layout.column_width(ProcessListColumn::Pid) < PROCESS_LIST_PID_MAX_WIDTH);
    assert!(layout.column_width(ProcessListColumn::MemoryTrim) < 140.0);
    assert!(
        layout.column_width(ProcessListColumn::PowerPlanForeground)
            >= process_list_estimated_cell_width(
                &process_list_column_label(ProcessListColumn::PowerPlanForeground, &settings),
                PROCESS_LIST_TEXT_CELL_HORIZONTAL_PADDING,
            )
    );
}

#[test]
fn process_icon_cache_drops_stale_paths() {
    let kept_path = PathBuf::from("C:\\Apps\\kept.exe");
    let stale_path = PathBuf::from("C:\\Apps\\stale.exe");
    let mut cache = HashMap::from([(kept_path.clone(), None), (stale_path.clone(), None)]);
    let candidates = vec![ProcessCandidate {
        name: "kept.exe".to_owned(),
        image_path: Some(kept_path.clone()),
        icon: None,
    }];

    WinderustApp::retain_current_process_icons(&mut cache, &candidates);

    assert!(cache.contains_key(&kept_path));
    assert!(!cache.contains_key(&stale_path));
}

#[test]
fn process_list_sort_orders_groups_by_name_direction() {
    let processes = vec![
        ProcessInfo {
            id: 1,
            parent_id: None,
            name: "editor.exe".to_owned(),
        },
        ProcessInfo {
            id: 2,
            parent_id: None,
            name: "worker.exe".to_owned(),
        },
    ];
    let groups = process_list_groups(&processes);
    let summaries = groups
        .iter()
        .map(|_| default_process_policy_summary())
        .collect::<Vec<_>>();
    let rows = process_list_sorted_rows(
        groups,
        summaries,
        ProcessListSort {
            column: ProcessListSortColumn::ProcessName,
            direction: ProcessListSortDirection::Descending,
        },
    );

    assert_eq!(rows[0].0.display_name, "worker.exe");
    assert_eq!(rows[1].0.display_name, "editor.exe");
}

#[test]
fn process_list_text_sort_cmp_matches_ascii_lowercase_sorting() {
    for (left, right) in [
        ("Alpha.exe", "alpha.exe"),
        ("worker.exe", "Editor.exe"),
        ("z.exe", "é.exe"),
    ] {
        let expected = left
            .to_ascii_lowercase()
            .cmp(&right.to_ascii_lowercase())
            .then_with(|| left.cmp(right));
        assert_eq!(process_list_text_sort_cmp(left, right), expected);
    }
}

#[test]
fn process_list_sort_orders_groups_and_children_by_pid() {
    let processes = vec![
        ProcessInfo {
            id: 30,
            parent_id: None,
            name: "editor.exe".to_owned(),
        },
        ProcessInfo {
            id: 10,
            parent_id: None,
            name: "worker.exe".to_owned(),
        },
        ProcessInfo {
            id: 20,
            parent_id: None,
            name: "editor.exe".to_owned(),
        },
    ];
    let sort = ProcessListSort {
        column: ProcessListSortColumn::Column(ProcessListColumn::Pid),
        direction: ProcessListSortDirection::Ascending,
    };
    let mut groups = process_list_groups(&processes);
    for group in &mut groups {
        process_list_sort_group_processes(group, sort);
    }
    let summaries = groups
        .iter()
        .map(|_| default_process_policy_summary())
        .collect::<Vec<_>>();
    let rows = process_list_sorted_rows(groups, summaries, sort);

    assert_eq!(rows[0].0.display_name, "worker.exe");
    assert_eq!(rows[1].0.display_name, "editor.exe");
    assert_eq!(rows[1].0.processes[0].id, 20);
    assert_eq!(rows[1].0.processes[1].id, 30);

    let sort = ProcessListSort {
        column: ProcessListSortColumn::Column(ProcessListColumn::Pid),
        direction: ProcessListSortDirection::Descending,
    };
    let mut groups = process_list_groups(&processes);
    for group in &mut groups {
        process_list_sort_group_processes(group, sort);
    }
    let summaries = groups
        .iter()
        .map(|_| default_process_policy_summary())
        .collect::<Vec<_>>();
    let rows = process_list_sorted_rows(groups, summaries, sort);

    assert_eq!(rows[0].0.display_name, "editor.exe");
    assert_eq!(rows[0].0.processes[0].id, 30);
    assert_eq!(rows[0].0.processes[1].id, 20);
    assert_eq!(rows[1].0.display_name, "worker.exe");
}

#[test]
fn process_list_sort_orders_groups_by_policy_column_value() {
    let processes = vec![
        ProcessInfo {
            id: 1,
            parent_id: None,
            name: "editor.exe".to_owned(),
        },
        ProcessInfo {
            id: 2,
            parent_id: None,
            name: "worker.exe".to_owned(),
        },
    ];
    let groups = process_list_groups(&processes);
    let mut low = default_process_policy_summary();
    low.process_priority = "Idle".to_owned();
    let mut high = default_process_policy_summary();
    high.process_priority = "Normal".to_owned();
    let rows = process_list_sorted_rows(
        groups,
        vec![high, low],
        ProcessListSort {
            column: ProcessListSortColumn::Column(ProcessListColumn::ProcessPriority),
            direction: ProcessListSortDirection::Ascending,
        },
    );

    assert_eq!(rows[0].0.display_name, "worker.exe");
    assert_eq!(rows[1].0.display_name, "editor.exe");
}

#[test]
fn process_list_policy_value_active_tracks_state_and_custom_values() {
    assert!(process_list_policy_value_active("Include", false));
    assert!(process_list_policy_value_active("Include (50%)", false));
    assert!(!process_list_policy_value_active("Exclude", true));
    assert!(!process_list_policy_value_active(
        process_list_default_label().as_str(),
        true
    ));
    assert!(!process_list_policy_value_active("Balanced", false));
    assert!(process_list_policy_value_active("Balanced", true));
}

#[test]
fn process_list_split_policy_value_parses_foreground_background_pairs() {
    assert_eq!(
        process_list_split_policy_value("Normal / Very low"),
        Some(("Normal", "Very low"))
    );
    assert_eq!(
        process_list_split_policy_value("  Above normal / Idle  "),
        Some(("Above normal", "Idle"))
    );
    assert_eq!(process_list_split_policy_value("Default"), None);
}

#[test]
fn process_list_policy_cell_editing_respects_row_editability() {
    assert!(!process_list_policy_cell_editable(
        true,
        ProcessListColumn::ProcessPriority
    ));
    assert!(!process_list_policy_cell_editable(
        false,
        ProcessListColumn::ProcessPriority
    ));
    assert!(!process_list_policy_cell_editable(
        true,
        ProcessListColumn::CoreSteering
    ));
}

#[test]
fn process_policy_summary_matches_exact_process_rule() {
    let mut settings = Settings::default();
    settings.core_steering.enabled = true;
    settings.core_steering.rules.push(CoreSteeringRule {
        enabled: true,
        mode: CoreSteeringMode::Soft,
        process_name: "Editor.EXE".to_owned(),
        core_mask: 0b1011,
    });

    let matching = process_policy_summary(&settings, &[], "editor.exe");
    assert_eq!(matching.core_steering, "0-1, 3");
    assert!(matching.uses_custom_rule(ProcessListColumn::CoreSteering));

    let non_matching = process_policy_summary(&settings, &[], "browser.exe");
    assert_eq!(
        non_matching.power_plan_foreground,
        process_list_default_label()
    );
    assert_eq!(
        non_matching.power_plan_running,
        process_list_default_label()
    );
    assert_eq!(non_matching.core_steering, default_core_steering_label());
    assert_eq!(
        non_matching.process_priority,
        process_priority_setting_label(ProcessPrioritySetting::Default)
    );
    assert!(!non_matching.uses_custom_rule(ProcessListColumn::CoreSteering));
}

#[test]
fn process_policy_summary_reports_priority_policy_values() {
    let mut settings = Settings::default();
    settings.io_priority.enabled = true;
    settings.gpu_priority.enabled = true;
    settings.memory_priority.enabled = true;

    let summary = process_policy_summary(&settings, &[], "editor.exe");

    assert_eq!(
        summary.io_priority,
        io_priority_policy_label(&settings.io_priority)
    );
    assert_eq!(
        summary.gpu_priority,
        gpu_priority_policy_label(&settings.gpu_priority)
    );
    assert_eq!(
        summary.memory_priority,
        memory_priority_policy_label(&settings.memory_priority)
    );
}

#[test]
fn process_policy_summary_reports_process_rule_columns() {
    let mut settings = Settings::default();
    settings.by_foreground.enabled = true;
    settings.by_foreground.rules.push(ByForegroundRule {
        enabled: true,
        name: "Editor".to_owned(),
        process_name: "editor.exe".to_owned(),
        power_plan_guid: Some("balanced-guid".to_owned()),
    });
    settings.by_running_app.enabled = true;
    settings.by_running_app.rules.push(ByRunningAppRule {
        enabled: true,
        name: "Editor".to_owned(),
        process_name: "editor.exe".to_owned(),
        power_plan_guid: Some("performance-guid".to_owned()),
    });
    settings.core_limiter.enabled = true;
    settings.core_limiter.rules.push(CoreLimiterRule {
        enabled: true,
        process_name: "editor.exe".to_owned(),
        threshold_percent: 80,
        sustain_seconds: 5,
        cooldown_seconds: 30,
        max_logical_processors: 50,
    });
    settings.app_suspension.enabled = true;
    settings
        .app_suspension
        .suspendable_apps
        .push(AppSuspensionRule {
            enabled: true,
            process_name: "editor.exe".to_owned(),
            network_wake_enabled: true,
            audio_wake_enabled: true,
            network_download_threshold_bytes: 1,
            network_download_threshold_unit: NetworkThresholdUnit::Bytes,
            network_upload_threshold_bytes: 0,
            network_upload_threshold_unit: NetworkThresholdUnit::Bytes,
        });
    settings.timer_resolution.enabled = true;
    settings.timer_resolution.rules.push(TimerResolutionRule {
        enabled: true,
        process_name: "editor.exe".to_owned(),
        desired_100ns: 10_000,
    });
    let plans = vec![
        PowerPlan {
            guid: "balanced-guid".to_owned(),
            name: "Balanced".to_owned(),
            active: false,
        },
        PowerPlan {
            guid: "performance-guid".to_owned(),
            name: "Performance".to_owned(),
            active: false,
        },
    ];

    let summary = process_policy_summary(&settings, &plans, "editor.exe");

    assert_eq!(summary.power_plan_foreground, "Balanced");
    assert_eq!(summary.power_plan_running, "Performance");
    assert_eq!(
        summary.core_limiter,
        process_list_include_value_label("50%")
    );
    assert_eq!(summary.app_suspension, process_list_include_label());
    assert_eq!(summary.timer_resolution, "1.00 ms");
    assert!(summary.uses_custom_rule(ProcessListColumn::PowerPlanForeground));
    assert!(summary.uses_custom_rule(ProcessListColumn::PowerPlanRunning));
    assert!(summary.uses_custom_rule(ProcessListColumn::CoreLimiter));
    assert!(summary.uses_custom_rule(ProcessListColumn::AppSuspension));
    assert!(summary.uses_custom_rule(ProcessListColumn::TimerResolution));
}

#[test]
fn process_policy_summary_reports_include_exclude_columns() {
    let mut settings = Settings::default();
    settings
        .background_efficiency
        .custom_rules
        .push(new_background_efficiency_rule("editor.exe"));
    settings
        .memory_trim
        .exclusions
        .push(new_process_exclusion_rule("editor.exe"));
    settings
        .app_suspension
        .suspendable_apps
        .push(AppSuspensionRule {
            enabled: true,
            process_name: "editor.exe".to_owned(),
            network_wake_enabled: true,
            audio_wake_enabled: true,
            network_download_threshold_bytes: 1,
            network_download_threshold_unit: NetworkThresholdUnit::Bytes,
            network_upload_threshold_bytes: 0,
            network_upload_threshold_unit: NetworkThresholdUnit::Bytes,
        });

    let summary = process_policy_summary(&settings, &[], "editor.exe");

    assert_eq!(summary.background_efficiency, process_list_exclude_label());
    assert_eq!(summary.core_limiter, process_list_exclude_label());
    assert_eq!(summary.memory_trim, process_list_exclude_label());
    assert_eq!(summary.app_suspension, process_list_include_label());
    assert_eq!(summary.timer_resolution, process_list_default_label());
    assert!(summary.uses_custom_rule(ProcessListColumn::BackgroundEfficiency));
    assert!(!summary.uses_custom_rule(ProcessListColumn::CoreLimiter));
    assert!(summary.uses_custom_rule(ProcessListColumn::MemoryTrim));
    assert!(summary.uses_custom_rule(ProcessListColumn::AppSuspension));
    assert!(!summary.uses_custom_rule(ProcessListColumn::TimerResolution));
}

#[test]
fn process_policy_summary_reports_priority_exclusions_as_exclude() {
    let mut settings = Settings::default();
    settings
        .io_priority
        .exclusions
        .push(new_process_exclusion_rule("editor.exe"));
    settings
        .gpu_priority
        .exclusions
        .push(new_process_exclusion_rule("editor.exe"));
    settings
        .memory_priority
        .exclusions
        .push(new_process_exclusion_rule("editor.exe"));

    let summary = process_policy_summary(&settings, &[], "editor.exe");

    assert_eq!(summary.io_priority, process_list_exclude_label());
    assert_eq!(summary.gpu_priority, process_list_exclude_label());
    assert_eq!(summary.memory_priority, process_list_exclude_label());
    assert!(summary.uses_custom_rule(ProcessListColumn::IoPriority));
    assert!(summary.uses_custom_rule(ProcessListColumn::GpuPriority));
    assert!(summary.uses_custom_rule(ProcessListColumn::MemoryPriority));
}

#[test]
fn process_list_rule_edit_helpers_update_process_overrides() {
    let mut settings = Settings::default();

    set_foreground_power_plan_override(
        &mut settings.by_foreground,
        "Editor.EXE",
        Some("balanced-guid".to_owned()),
    );
    let summary = process_policy_summary(&settings, &[], "editor.exe");
    assert_eq!(summary.power_plan_foreground, "balanced-guid");
    assert!(summary.uses_custom_rule(ProcessListColumn::PowerPlanForeground));

    set_foreground_power_plan_override(&mut settings.by_foreground, "editor.exe", None);
    let summary = process_policy_summary(&settings, &[], "editor.exe");
    assert_eq!(summary.power_plan_foreground, process_list_default_label());
    assert!(!summary.uses_custom_rule(ProcessListColumn::PowerPlanForeground));

    set_core_limiter_override(&mut settings.core_limiter, "editor.exe", Some(50));
    let summary = process_policy_summary(&settings, &[], "editor.exe");
    assert_eq!(
        summary.core_limiter,
        process_list_include_value_label("50%")
    );
    assert!(summary.uses_custom_rule(ProcessListColumn::CoreLimiter));

    set_core_limiter_override(&mut settings.core_limiter, "editor.exe", None);
    let summary = process_policy_summary(&settings, &[], "editor.exe");
    assert_eq!(summary.core_limiter, process_list_exclude_label());
    assert!(!summary.uses_custom_rule(ProcessListColumn::CoreLimiter));
}

#[test]
fn process_list_rule_edit_helpers_update_timer_overrides() {
    let mut settings = Settings::default();

    set_timer_resolution_override(&mut settings.timer_resolution, "editor.exe", Some(20_000));
    let summary = process_policy_summary(&settings, &[], "editor.exe");
    assert_eq!(
        summary.timer_resolution,
        timer_resolution::format_resolution_ms(20_000)
    );
    assert!(summary.uses_custom_rule(ProcessListColumn::TimerResolution));

    set_timer_resolution_override(&mut settings.timer_resolution, "editor.exe", None);
    let summary = process_policy_summary(&settings, &[], "editor.exe");
    assert_eq!(summary.timer_resolution, process_list_default_label());
    assert!(!summary.uses_custom_rule(ProcessListColumn::TimerResolution));
}

#[test]
fn process_policy_summary_reports_default_power_plan_when_unset() {
    let mut settings = Settings::default();
    settings.by_foreground.enabled = true;
    settings.by_foreground.rules.push(ByForegroundRule {
        enabled: true,
        name: "Editor".to_owned(),
        process_name: "editor.exe".to_owned(),
        power_plan_guid: None,
    });
    settings.by_running_app.enabled = true;
    settings.by_running_app.rules.push(ByRunningAppRule {
        enabled: true,
        name: "Editor".to_owned(),
        process_name: "editor.exe".to_owned(),
        power_plan_guid: None,
    });

    let summary = process_policy_summary(&settings, &[], "editor.exe");

    assert_eq!(summary.power_plan_foreground, process_list_default_label());
    assert_eq!(summary.power_plan_running, process_list_default_label());
    assert_eq!(summary.timer_resolution, process_list_default_label());
    assert!(summary.uses_custom_rule(ProcessListColumn::PowerPlanForeground));
    assert!(summary.uses_custom_rule(ProcessListColumn::PowerPlanRunning));
    assert!(!summary.uses_custom_rule(ProcessListColumn::TimerResolution));
}

#[test]
fn process_policy_summary_reports_configured_rules_when_feature_disabled() {
    let mut settings = Settings::default();
    settings.by_foreground.enabled = false;
    settings.by_foreground.rules.push(ByForegroundRule {
        enabled: true,
        name: "Editor".to_owned(),
        process_name: "editor.exe".to_owned(),
        power_plan_guid: Some("balanced-guid".to_owned()),
    });
    settings.by_running_app.enabled = false;
    settings.by_running_app.rules.push(ByRunningAppRule {
        enabled: true,
        name: "Editor".to_owned(),
        process_name: "editor.exe".to_owned(),
        power_plan_guid: Some("performance-guid".to_owned()),
    });
    settings.core_limiter.enabled = false;
    settings.core_limiter.rules.push(CoreLimiterRule {
        enabled: true,
        process_name: "editor.exe".to_owned(),
        threshold_percent: 80,
        sustain_seconds: 5,
        cooldown_seconds: 30,
        max_logical_processors: 25,
    });
    settings.timer_resolution.enabled = false;
    settings.timer_resolution.rules.push(TimerResolutionRule {
        enabled: true,
        process_name: "editor.exe".to_owned(),
        desired_100ns: 10_000,
    });
    let plans = vec![
        PowerPlan {
            guid: "balanced-guid".to_owned(),
            name: "Balanced".to_owned(),
            active: false,
        },
        PowerPlan {
            guid: "performance-guid".to_owned(),
            name: "Performance".to_owned(),
            active: false,
        },
    ];

    let summary = process_policy_summary(&settings, &plans, "editor.exe");

    assert_eq!(summary.power_plan_foreground, "Balanced");
    assert_eq!(summary.power_plan_running, "Performance");
    assert_eq!(
        summary.core_limiter,
        process_list_include_value_label("25%")
    );
    assert_eq!(summary.timer_resolution, "1.00 ms");
    assert!(summary.uses_custom_rule(ProcessListColumn::PowerPlanForeground));
    assert!(summary.uses_custom_rule(ProcessListColumn::PowerPlanRunning));
    assert!(summary.uses_custom_rule(ProcessListColumn::CoreLimiter));
    assert!(summary.uses_custom_rule(ProcessListColumn::TimerResolution));
}

#[test]
fn cpu_mask_formatter_uses_ranges() {
    assert_eq!(format_cpu_mask(0), t!("common.none").to_string());
    assert_eq!(format_cpu_mask(0b1111), "0-3");
    assert_eq!(format_cpu_mask(0b101101), "0, 2-3, 5");
}

#[test]
fn no_smt_mask_selects_one_logical_cpu_per_physical_core() {
    let processors = vec![
        LogicalProcessorInfo {
            index: 0,
            core_index: 0,
            kind: LogicalProcessorKind::Standard,
            efficiency_class: 0,
        },
        LogicalProcessorInfo {
            index: 1,
            core_index: 0,
            kind: LogicalProcessorKind::Standard,
            efficiency_class: 0,
        },
        LogicalProcessorInfo {
            index: 2,
            core_index: 1,
            kind: LogicalProcessorKind::Standard,
            efficiency_class: 0,
        },
        LogicalProcessorInfo {
            index: 3,
            core_index: 1,
            kind: LogicalProcessorKind::Standard,
            efficiency_class: 0,
        },
    ];

    assert_eq!(core_steering_processors_no_smt_mask(&processors), 0b0101);
}

#[test]
fn topology_aware_core_toggle_keeps_one_available_cpu_selected() {
    let mut mask = (1_u64 << 63) | 0b0001;
    toggle_affinity_core_with_available_mask(&mut mask, 0, 0b0011);

    assert_eq!(mask, 0b0001);

    toggle_affinity_core_with_available_mask(&mut mask, 1, 0b0011);
    assert_eq!(mask, 0b0011);

    toggle_affinity_core_with_available_mask(&mut mask, 0, 0b0011);
    assert_eq!(mask, 0b0010);
}

#[test]
fn new_core_steering_rules_default_to_soft_cpu_sets() {
    let rule = new_core_steering_rule("game.exe");

    assert_eq!(rule.mode, CoreSteeringMode::Soft);
}
