use super::*;

#[test]
fn process_resource_columns_format_usage() {
    let mut summary = default_process_policy_summary();
    summary.cpu_percent = Some(12.34);
    summary.memory_bytes = Some(512 * 1024 * 1024);

    assert_eq!(
        process_list_column_value(&summary, ProcessListColumn::CpuUsage),
        "12.3%"
    );
    assert_eq!(
        process_list_column_value(&summary, ProcessListColumn::MemoryUsage),
        "512.0 MB"
    );
}

#[test]
fn limited_access_process_uses_dash_for_unavailable_metrics() {
    let mut summary = default_process_policy_summary();
    summary.status = t!("process_list.status_limited_access").to_string();

    assert_eq!(
        process_list_column_value(&summary, ProcessListColumn::CpuUsage),
        "\u{2014}"
    );
    assert_eq!(
        process_list_column_value(&summary, ProcessListColumn::MemoryUsage),
        "\u{2014}"
    );
}

#[test]
fn process_list_stretches_name_column_to_push_metrics_right() {
    let mut layout = ProcessListColumnLayout {
        name_width: 200.0,
        column_widths: PROCESS_LIST_OVERVIEW_COLUMNS
            .iter()
            .map(|column| (*column, 100.0))
            .collect(),
    };

    let memory_width = layout.column_width(ProcessListColumn::MemoryUsage);
    stretch_process_list_layout(&mut layout, px(900.0));

    assert_eq!(process_list_table_width(&layout), px(900.0));
    assert!(layout.name_width > 200.0);
    assert_eq!(
        layout.column_width(ProcessListColumn::MemoryUsage),
        memory_width
    );
}

#[test]
fn process_resource_columns_sort_busiest_first() {
    let sort = ProcessListSort::default()
        .toggled_for(ProcessListSortColumn::Column(ProcessListColumn::CpuUsage));

    assert_eq!(sort.direction, ProcessListSortDirection::Descending);
}

#[test]
fn process_status_reports_suspension_and_efficiency_mode() {
    let snapshot = AppSuspensionSnapshot {
        suspended_apps: vec![r"C:\Apps\editor.exe".to_owned()],
        suspended_process_ids: vec![42],
        ..Default::default()
    };

    assert_eq!(
        process_list_status_label(&snapshot, Some(42), r"c:\apps\EDITOR.exe", true),
        t!("process_list.status_suspended").to_string()
    );
    assert_eq!(
        process_list_status_label(&snapshot, Some(7), r"C:\Apps\active.exe", true),
        t!("process_list.status_efficiency_mode").to_string()
    );
    assert_eq!(
        process_list_status_label(&snapshot, Some(7), r"C:\Apps\active.exe", false),
        t!("process_list.status_active").to_string()
    );
}

#[test]
fn process_list_column_layout_fits_headers_and_values() {
    let settings = Settings::default();
    let processes = vec![
        ProcessInfo {
            id: 1234,
            parent_id: None,
            name: "editor.exe".to_owned(),
            image_path: Some(PathBuf::from("editor.exe".to_owned())),
        },
        ProcessInfo {
            id: 12345,
            parent_id: None,
            name: "worker.exe".to_owned(),
            image_path: Some(PathBuf::from("worker.exe".to_owned())),
        },
    ];
    let groups = process_list_groups(&processes);
    let summaries = groups
        .iter()
        .map(|_| default_process_policy_summary())
        .collect::<Vec<_>>();

    let layout = process_list_column_layout(&settings, &groups, &summaries);

    assert!(layout.column_width(ProcessListColumn::Pid) < PROCESS_LIST_PID_MAX_WIDTH);
    assert!(layout.column_width(ProcessListColumn::Status) >= 120.0);
    assert!(layout.column_width(ProcessListColumn::CpuUsage) >= 88.0);
    assert!(layout.column_width(ProcessListColumn::MemoryUsage) >= 112.0);
}

#[test]
fn process_icon_cache_drops_stale_paths() {
    let kept_path = PathBuf::from("C:\\Apps\\kept.exe");
    let stale_path = PathBuf::from("C:\\Apps\\stale.exe");
    let mut cache = HashMap::from([(kept_path.clone(), ()), (stale_path.clone(), ())]);
    let candidates = vec![ProcessCandidate {
        name: "kept.exe".to_owned(),
        image_path: kept_path.clone(),
        icon: None,
    }];

    WinderustApp::retain_current_process_icons(&mut cache, &candidates);

    assert!(cache.contains_key(&kept_path));
    assert!(!cache.contains_key(&stale_path));
}

#[test]
fn process_list_icon_lookup_handles_mixed_case_windows_path() {
    let executable_path = PathBuf::from(r"C:\Apps\MixedCase\Editor.EXE");
    let processes = vec![ProcessInfo {
        id: 1,
        parent_id: None,
        name: "Editor.EXE".to_owned(),
        image_path: Some(executable_path.clone()),
    }];
    let groups = process_list_groups(&processes);
    assert_eq!(
        groups[0].executable_path,
        executable_path.to_string_lossy().as_ref()
    );
    assert_ne!(
        process_list_executable_path_group_key(&executable_path),
        process_list_executable_path_group_key(Path::new(r"c:\apps\mixedcase\editor.exe"))
    );
    let rows = groups
        .into_iter()
        .map(|group| (group, default_process_policy_summary()))
        .collect::<Vec<_>>();
    let icon = Arc::new(Image::empty());
    let candidates = vec![ProcessCandidate {
        name: "Editor.EXE".to_owned(),
        image_path: executable_path,
        icon: Some(Arc::clone(&icon)),
    }];
    let icons_by_path = process_list_icons_by_path(&candidates);

    let rendered = process_list_rendered_rows(&rows, &icons_by_path, |_| true);

    let Some(ProcessListRenderedRow::Entry {
        icon: Some(rendered_icon),
        ..
    }) = rendered.first()
    else {
        panic!("expected a process row with an icon");
    };
    assert!(Arc::ptr_eq(rendered_icon, &icon));
}

#[test]
fn process_list_sort_orders_groups_by_name_direction() {
    let processes = vec![
        ProcessInfo {
            id: 1,
            parent_id: None,
            name: "editor.exe".to_owned(),
            image_path: Some(PathBuf::from("editor.exe".to_owned())),
        },
        ProcessInfo {
            id: 2,
            parent_id: None,
            name: "worker.exe".to_owned(),
            image_path: Some(PathBuf::from("worker.exe".to_owned())),
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
fn process_list_keeps_same_named_executables_in_separate_groups() {
    let processes = vec![
        ProcessInfo {
            id: 1,
            parent_id: None,
            name: "game.exe".to_owned(),
            image_path: Some(PathBuf::from(r"C:\Games\game.exe")),
        },
        ProcessInfo {
            id: 2,
            parent_id: None,
            name: "game.exe".to_owned(),
            image_path: Some(PathBuf::from(r"C:\Other\game.exe")),
        },
    ];

    let groups = process_list_groups(&processes);

    assert_eq!(groups.len(), 2);
    assert_ne!(groups[0].executable_path, groups[1].executable_path);
    assert_ne!(groups[0].display_name, groups[1].display_name);
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
            image_path: Some(PathBuf::from("editor.exe".to_owned())),
        },
        ProcessInfo {
            id: 10,
            parent_id: None,
            name: "worker.exe".to_owned(),
            image_path: Some(PathBuf::from("worker.exe".to_owned())),
        },
        ProcessInfo {
            id: 20,
            parent_id: None,
            name: "editor.exe".to_owned(),
            image_path: Some(PathBuf::from("editor.exe".to_owned())),
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
            image_path: Some(PathBuf::from("editor.exe".to_owned())),
        },
        ProcessInfo {
            id: 2,
            parent_id: None,
            name: "worker.exe".to_owned(),
            image_path: Some(PathBuf::from("worker.exe".to_owned())),
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
fn process_policy_summary_carries_typed_active_state() {
    let mut settings = Settings::default();
    let path = r"C:\Apps\editor.exe";

    let summary = process_policy_summary(&settings, &[], path);
    assert!(summary.value_is_active(ProcessListColumn::AdaptiveEngine));
    assert!(summary.value_is_active(ProcessListColumn::BackgroundEfficiency));
    assert!(!summary.value_is_active(ProcessListColumn::ProcessPriority));

    settings
        .background_efficiency
        .custom_rules
        .push(new_background_efficiency_rule(path));
    set_process_priority_rule(
        &mut settings.process_priority,
        path,
        ProcessPrioritySetting::Idle,
    );

    let summary = process_policy_summary(&settings, &[], path);
    assert!(!summary.value_is_active(ProcessListColumn::BackgroundEfficiency));
    assert!(summary.value_is_active(ProcessListColumn::ProcessPriority));
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
    assert!(process_list_policy_cell_editable(
        true,
        ProcessListColumn::ProcessPriority
    ));
    assert!(!process_list_policy_cell_editable(
        false,
        ProcessListColumn::ProcessPriority
    ));
    assert!(!process_list_policy_cell_editable(
        true,
        ProcessListColumn::Status
    ));
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
fn process_policy_summary_ignores_disabled_priority_rules() {
    let mut settings = Settings::default();
    let path = r"C:\Apps\editor.exe";
    let mut rule = new_process_exclusion_rule(path);
    rule.enabled = false;
    rule.set_process_priority_override(true, ProcessPrioritySetting::Idle);
    rule.set_thread_priority_override(true, ProcessThreadPrioritySetting::Lowest);
    rule.set_dynamic_priority_boost_override(true, ProcessDynamicPriorityBoostSetting::Disabled);
    rule.set_io_priority_override(true, ProcessIoPrioritySetting::Low);
    rule.set_gpu_priority_override(true, ProcessGpuPrioritySetting::BelowNormal);
    rule.set_memory_priority_override(true, ProcessMemoryPrioritySetting::Low);

    settings.process_priority.exclusions.push(rule.clone());
    settings.thread_priority.exclusions.push(rule.clone());
    settings
        .dynamic_priority_boost
        .exclusions
        .push(rule.clone());
    settings.io_priority.exclusions.push(rule.clone());
    settings.gpu_priority.exclusions.push(rule.clone());
    settings.memory_priority.exclusions.push(rule);

    let summary = process_policy_summary(&settings, &[], path);

    for column in [
        ProcessListColumn::ProcessPriority,
        ProcessListColumn::ThreadPriority,
        ProcessListColumn::DynamicPriorityBoost,
        ProcessListColumn::IoPriority,
        ProcessListColumn::GpuPriority,
        ProcessListColumn::MemoryPriority,
    ] {
        assert!(!summary.uses_custom_rule(column));
        assert!(!summary.value_is_active(column));
    }
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
        executable_path: "editor.exe".to_owned(),
        power_plan_guid: Some("balanced-guid".to_owned()),
    });
    settings.by_running_app.enabled = true;
    settings.by_running_app.rules.push(ByRunningAppRule {
        enabled: true,
        name: "Editor".to_owned(),
        executable_path: "editor.exe".to_owned(),
        power_plan_guid: Some("performance-guid".to_owned()),
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
}

#[test]
fn process_policy_summary_reports_include_exclude_columns() {
    let mut settings = Settings::default();
    settings
        .background_efficiency
        .custom_rules
        .push(new_background_efficiency_rule("editor.exe"));

    let summary = process_policy_summary(&settings, &[], "editor.exe");

    assert_eq!(summary.background_efficiency, process_list_exclude_label());
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
