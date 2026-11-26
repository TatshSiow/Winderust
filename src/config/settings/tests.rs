use super::*;

#[test]
fn sleep_prevention_is_opt_in_and_round_trips_shared_policy() {
    let mut settings = Settings::default();
    assert!(!settings.sleep_prevention.enabled);
    assert!(settings.sleep_prevention.rules.is_empty());
    assert!(!settings.battery_profile().sleep_prevention.enabled);
    settings.battery_profile_mut();
    settings.sleep_prevention.enabled = true;
    settings.sleep_prevention.rules.push(SleepPreventionRule {
        enabled: true,
        executable_path: r"C:\Apps\worker.exe".into(),
        keep_display_on: true,
    });
    settings.sync_shared_settings_to_battery();
    assert_eq!(
        settings.battery_profile().sleep_prevention,
        settings.sleep_prevention
    );
    assert!(settings
        .sleep_prevention
        .contains_rule_for(r"c:/apps/WORKER.exe"));
    assert!(!settings
        .sleep_prevention
        .contains_rule_for(r"C:\Other\worker.exe"));
    let raw = toml::to_string(&settings).unwrap();
    assert_eq!(toml::from_str::<Settings>(&raw).unwrap(), settings);
    let rule: SleepPreventionRule =
        toml::from_str("executable_path = 'C:\\Apps\\worker.exe'").unwrap();
    assert!(rule.enabled);
    assert!(!rule.keep_display_on);
}

#[test]
fn sleep_prevention_validates_paths_in_the_shared_policy() {
    for battery in [false, true] {
        for path in ["", "worker.exe", r"C:\Apps\worker.exe\0"] {
            let path = path.replace("\\0", "\0");
            let mut settings = Settings::default();
            let profile = if battery {
                settings.battery_profile_mut()
            } else {
                &mut settings
            };
            profile.sleep_prevention.rules.push(SleepPreventionRule {
                enabled: false,
                executable_path: path,
                keep_display_on: false,
            });
            let result = settings.validate();
            if battery {
                assert!(result.is_ok());
            } else {
                assert!(result.unwrap_err().contains("Sleep Prevention"));
            }
        }
    }
}

#[test]
fn keep_running_and_resource_thresholds_validate_shared_settings() {
    use crate::config::KeepRunningRule;

    let mut settings = Settings::default();
    let battery = settings.battery_profile_mut();
    battery.resource_thresholds.sustained_seconds = 0;
    battery
        .keep_running
        .rules
        .push(KeepRunningRule::new("relative.exe".into()));

    assert!(settings.validate().is_ok());

    let battery = settings.on_battery.take().unwrap();
    settings.resource_thresholds = battery.resource_thresholds;
    settings.keep_running = battery.keep_running;
    assert!(settings.validate_resource_thresholds().is_err());
    assert!(settings.validate_keep_running().is_err());
}

#[test]
fn cpu_load_percent_preserves_numeric_toml_and_rejects_out_of_range_values() {
    #[derive(Debug, Serialize, Deserialize)]
    struct Threshold {
        percent: CpuLoadPercent,
    }
    for value in [0, 100] {
        let raw = format!("percent = {value}\n");
        let parsed: Threshold = toml::from_str(&raw).unwrap();
        assert_eq!(parsed.percent.get(), value);
        assert_eq!(toml::to_string(&parsed).unwrap(), raw);
    }
    for value in [-1, 101, 255, 256] {
        assert!(toml::from_str::<Threshold>(&format!("percent = {value}")).is_err());
    }
    assert!(CpuLoadPercent::new(101).is_err());
}

#[test]
fn cpu_load_ranges_match_inclusive_bounds_without_reordering() {
    let mut rule = ByCpuLoadRule {
        comparison: CpuUsageComparison::Between,
        threshold_percent: CpuLoadPercent(20),
        upper_threshold_percent: Some(CpuLoadPercent(80)),
        ..ByCpuLoadRule::default()
    };
    assert!(rule.validate().is_ok());
    for usage in [20.0, 50.0, 80.0] {
        assert!(rule.matches_usage(usage));
    }
    for usage in [0.0, 19.9, 80.1, 100.0] {
        assert!(!rule.matches_usage(usage));
    }
    rule.threshold_percent = CpuLoadPercent(80);
    assert!(rule.validate().is_ok());
    assert!(rule.matches_usage(80.0));
    rule.upper_threshold_percent = Some(CpuLoadPercent(20));
    assert!(rule.validate().is_err());
    assert!(!rule.matches_usage(50.0));
}

#[test]
fn cpu_load_stability_is_opt_in_and_release_bounds_enclose_entry_bounds() {
    let mut rule = ByCpuLoadRule::default();
    let raw = toml::to_string(&rule).unwrap();
    assert!(!raw.contains("stability"));
    assert_eq!(toml::from_str::<ByCpuLoadRule>(&raw).unwrap(), rule);
    for (comparison, entry, upper, exit, exit_upper, valid) in [
        (CpuUsageComparison::AtOrAbove, 75, None, 65, None, true),
        (CpuUsageComparison::AtOrAbove, 75, None, 76, None, false),
        (CpuUsageComparison::AtOrBelow, 25, None, 35, None, true),
        (CpuUsageComparison::AtOrBelow, 25, None, 24, None, false),
        (CpuUsageComparison::AtOrBelow, 25, None, 35, Some(80), false),
        (
            CpuUsageComparison::Between,
            40,
            Some(60),
            30,
            Some(70),
            true,
        ),
        (
            CpuUsageComparison::Between,
            40,
            Some(60),
            41,
            Some(70),
            false,
        ),
        (
            CpuUsageComparison::Between,
            40,
            Some(60),
            30,
            Some(59),
            false,
        ),
        (CpuUsageComparison::Between, 40, Some(60), 30, None, false),
        (CpuUsageComparison::Between, 40, None, 30, Some(100), true),
        (CpuUsageComparison::Between, 40, None, 30, Some(99), false),
        (CpuUsageComparison::Else, 40, None, 30, None, false),
    ] {
        rule.comparison = comparison;
        rule.threshold_percent = CpuLoadPercent(entry);
        rule.upper_threshold_percent = upper.map(CpuLoadPercent);
        rule.stability = Some(ByCpuLoadStability {
            exit_threshold_percent: CpuLoadPercent(exit),
            exit_upper_threshold_percent: exit_upper.map(CpuLoadPercent),
            minimum_hold_seconds: 0,
        });
        assert_eq!(rule.validate().is_ok(), valid, "{rule:?}");
        let parsed = toml::from_str::<ByCpuLoadRule>(&toml::to_string(&rule).unwrap());
        if valid {
            assert_eq!(parsed.unwrap(), rule);
        } else {
            assert!(parsed.is_err());
        }
    }
    rule.comparison = CpuUsageComparison::AtOrAbove;
    for seconds in [i64::MAX as u64, i64::MAX as u64 + 1, u64::MAX] {
        rule.stability.as_mut().unwrap().minimum_hold_seconds = seconds;
        assert_eq!(rule.validate().is_ok(), seconds <= i64::MAX as u64);
    }
}

#[test]
fn visible_window_default_remains_independent_from_background() {
    let mut rule = ProcessExclusionRule {
        process_background_priority: Some(ProcessPrioritySetting::BelowNormal),
        thread_background_priority: Some(ProcessThreadPrioritySetting::Lowest),
        dynamic_priority_boost_background: Some(ProcessDynamicPriorityBoostSetting::Disabled),
        io_background_priority: Some(ProcessIoPrioritySetting::VeryLow),
        gpu_background_priority: Some(ProcessGpuPrioritySetting::Idle),
        memory_background_priority: Some(ProcessMemoryPrioritySetting::VeryLow),
        ..Default::default()
    };

    rule.set_process_priority_override(false, true, ProcessPrioritySetting::Default);
    rule.set_thread_priority_override(false, true, ProcessThreadPrioritySetting::Default);
    rule.set_dynamic_priority_boost_override(
        false,
        true,
        ProcessDynamicPriorityBoostSetting::Default,
    );
    rule.set_io_priority_override(false, true, ProcessIoPrioritySetting::Default);
    rule.set_gpu_priority_override(false, true, ProcessGpuPrioritySetting::Default);
    rule.set_memory_priority_override(false, true, ProcessMemoryPrioritySetting::Default);

    assert_eq!(
        rule.process_priority_override(false, true),
        ProcessPrioritySetting::Default
    );
    assert_eq!(
        rule.thread_priority_override(false, true),
        ProcessThreadPrioritySetting::Default
    );
    assert_eq!(
        rule.dynamic_priority_boost_override(false, true),
        ProcessDynamicPriorityBoostSetting::Default
    );
    assert_eq!(
        rule.io_priority_override(false, true),
        ProcessIoPrioritySetting::Default
    );
    assert_eq!(
        rule.gpu_priority_override(false, true),
        ProcessGpuPrioritySetting::Default
    );
    assert_eq!(
        rule.memory_priority_override(false, true),
        ProcessMemoryPrioritySetting::Default
    );
}

#[test]
fn visible_window_priority_override_is_not_an_exclusion() {
    let path = r"C:\Apps\editor.exe";
    let rule = ProcessExclusionRule {
        executable_path: path.to_owned(),
        io_visible_window_priority: Some(ProcessIoPrioritySetting::Low),
        gpu_visible_window_priority: Some(ProcessGpuPrioritySetting::BelowNormal),
        memory_visible_window_priority: Some(ProcessMemoryPrioritySetting::Low),
        ..Default::default()
    };
    let mut settings = Settings::default();
    settings.io_priority.exclusions.push(rule.clone());
    settings.gpu_priority.exclusions.push(rule.clone());
    settings.memory_priority.exclusions.push(rule);

    assert_eq!(
        settings.io_priority.override_for(path, false, true),
        Some(Some(ProcessIoPrioritySetting::Low))
    );
    assert_eq!(
        settings.io_priority.override_for(path, true, false),
        Some(None)
    );
    assert_eq!(
        settings.gpu_priority.override_for(path, false, true),
        Some(Some(ProcessGpuPrioritySetting::BelowNormal))
    );
    assert_eq!(
        settings.gpu_priority.override_for(path, true, false),
        Some(None)
    );
    assert_eq!(
        settings.memory_priority.override_for(path, false, true),
        Some(Some(ProcessMemoryPrioritySetting::Low))
    );
    assert_eq!(
        settings.memory_priority.override_for(path, true, false),
        Some(None)
    );
}

#[test]
fn restore_previous_power_plan_is_shared_without_replacing_local_plans() {
    use crate::application::SettingsEditor;

    for existing_battery in [false, true] {
        let mut initial = Settings::default();
        initial.by_activity.power_plans.performance_guid = Some("ac-active".into());
        if existing_battery {
            initial
                .battery_profile_mut()
                .by_activity
                .power_plans
                .performance_guid = Some("battery-active".into());
        }
        let mut settings = SettingsEditor::with_settings(initial.clone());
        settings.select_power_source(PowerSourceProfile::OnBattery);
        settings.edit_global(|settings| settings.general.restore_previous_power_plan = true);
        assert!(settings.general.restore_previous_power_plan);
        assert!(settings.global().general.restore_previous_power_plan);
        assert_eq!(
            settings.by_activity.power_plans.performance_guid.as_deref(),
            Some(if existing_battery {
                "battery-active"
            } else {
                "ac-active"
            })
        );
        settings.select_power_source(PowerSourceProfile::PluggedIn);
        assert!(settings.general.restore_previous_power_plan);
        assert_eq!(
            settings.by_activity.power_plans.performance_guid.as_deref(),
            Some("ac-active")
        );
        settings.cancel();
        assert_eq!(settings.global(), &initial);
    }
}

#[test]
fn first_run_rule_modules_are_disabled() {
    let settings = Settings::default();
    assert!(!settings.general.restore_previous_power_plan);

    assert!(![
        settings.adaptive_engine.enabled,
        settings.by_activity.enabled,
        settings.by_focus.enabled,
        settings.by_time.enabled,
        settings.by_cpu_load.enabled,
        settings.efficiency_mode.enabled,
        settings.app_suspension.enabled,
        settings.cpu_sets_soft.enabled,
        settings.processor_affinity_hard.enabled,
        settings.cpu_limiter.enabled,
        settings.by_running_app.enabled,
        settings.process_priority.enabled,
        settings.thread_priority.enabled,
        settings.dynamic_priority_boost.enabled,
        settings.io_priority.enabled,
        settings.gpu_priority.enabled,
        settings.memory_priority.enabled,
        settings.memory_trim.enabled,
        settings.resource_thresholds.enabled,
        settings.timer_resolution.enabled,
    ]
    .into_iter()
    .any(|enabled| enabled));
}

#[test]
fn cross_session_process_control_is_enabled_by_default() {
    assert!(
        Settings::default()
            .general
            .allow_cross_session_process_control
    );
}

#[test]
fn appearance_indicators_are_enabled_by_default() {
    let general = Settings::default().general;

    assert!(general.show_enabled_feature_counts_in_sidebar);
    assert!(general.show_feature_status_on_cards);
}

#[test]
fn network_threshold_units_convert_to_canonical_bytes() {
    use NetworkThresholdUnit::{Bytes, Gigabytes, Kilobytes, Megabytes};
    let max = i64::MAX as u64;
    for unit in NetworkThresholdUnit::ALL {
        for bytes in [0, 1, 123, 2007, 1_250_000_000, (1 << 53) + 1, max] {
            assert_eq!(
                unit.parse_threshold(&unit.format_threshold(bytes)),
                Some(bytes)
            );
        }
        for invalid in ["NaN", "inf", "-1", "", ".", "1e-", "1e1000"] {
            assert_eq!(unit.parse_threshold(invalid), None);
        }
    }
    assert_eq!(Bytes.parse_threshold("1.1"), Some(2));
    assert_eq!(Kilobytes.parse_threshold("2.007"), Some(2007));
    assert_eq!(Megabytes.parse_threshold("0.000123"), Some(123));
    assert_eq!(Gigabytes.parse_threshold("1.25e-2"), Some(12_500_000));
    assert_eq!(Gigabytes.parse_threshold("9223372036.854775807"), Some(max));
    assert_eq!(Gigabytes.parse_threshold("9223372036.8547758071"), None);
}

#[test]
fn restraint_scoped_background_processor_limit_requires_restraint() {
    let mut settings = AdaptiveEngineProcessSettings {
        limit_background_processors_enabled: true,
        ..Default::default()
    };
    assert!(settings.background_processor_limit_enabled());
    assert!(settings.has_enabled_policies());

    settings.limit_background_processors_during_restraint = true;
    assert!(!settings.background_processor_limit_enabled());
    assert!(!settings.has_enabled_policies());

    settings.cpu_pressure_restraint_enabled = true;
    assert!(settings.background_processor_limit_enabled());
    assert!(settings.has_enabled_policies());

    settings.limit_background_processors_enabled = false;
    assert!(!settings.background_processor_limit_enabled());
    assert!(settings.has_enabled_policies());
}

#[test]
fn placement_only_restraint_uses_the_limiter_without_changing_its_saved_scope() {
    let mut settings = AdaptiveEngineProcessSettings {
        restraint_priority: AdaptiveRestraintPriority::ProcessorPlacementOnly,
        limit_background_processors_enabled: true,
        ..Default::default()
    };
    assert!(!settings.has_enabled_policies());
    assert!(!settings.background_processor_limit_enabled());

    settings.cpu_pressure_restraint_enabled = true;
    assert!(settings.placement_only_restraint_enabled());
    assert!(settings.background_processor_limit_enabled());
    assert!(settings.background_processor_limit_scoped());

    let file = toml::to_string(&settings).unwrap();
    assert!(file.contains("restraint_priority = \"processor_placement_only\""));
    assert_eq!(
        toml::from_str::<AdaptiveEngineProcessSettings>(&file).unwrap(),
        settings
    );

    settings.restraint_priority = AdaptiveRestraintPriority::BelowNormal;
    assert!(!settings.placement_only_restraint_enabled());
    assert!(settings.background_processor_limit_enabled());
    assert!(!settings.background_processor_limit_scoped());
}

#[test]
fn adaptive_custom_rules_match_the_configured_selector_kind() {
    let settings = AdaptiveEngineProcessSettings {
        custom_rules: vec![
            AdaptiveEngineExclusionRule::exact_path(r"C:\Games\game.exe"),
            AdaptiveEngineExclusionRule {
                enabled: false,
                ..AdaptiveEngineExclusionRule::exact_path(r"C:\Apps\disabled.exe")
            },
            AdaptiveEngineExclusionRule::exact_name("Éditeur.exe"),
            AdaptiveEngineExclusionRule {
                enabled: false,
                ..AdaptiveEngineExclusionRule::exact_name("disabled.exe")
            },
        ],
        ..Default::default()
    };

    assert!(settings.excludes_executable_path("c:/games/GAME.exe"));
    assert!(!settings.excludes_executable_path(r"C:\Other\game.exe"));
    assert!(!settings.excludes_executable_path(r"C:\Games\game*.exe"));
    assert!(!settings.excludes_executable_path(r"C:\Apps\disabled.exe"));
    assert!(!settings.excludes_executable_name("game.exe"));
    assert!(settings.excludes_executable_name("éditeur.EXE"));
    assert!(!settings.excludes_executable_name("my-éditeur.exe"));
    assert!(!settings.excludes_executable_name("disabled.exe"));
    assert!(!settings.excludes_executable_path(r"C:\Apps\éditeur.exe"));
}

#[test]
fn high_process_priorities_downgrade_when_advanced_is_hidden() {
    assert_eq!(
        ProcessPrioritySetting::Realtime.safe_when_advanced_disabled(),
        ProcessPrioritySetting::AboveNormal
    );
    assert_eq!(
        ProcessPrioritySetting::High.safe_when_advanced_disabled(),
        ProcessPrioritySetting::AboveNormal
    );
    assert_eq!(
        ProcessPrioritySetting::BelowNormal.safe_when_advanced_disabled(),
        ProcessPrioritySetting::BelowNormal
    );
}

#[test]
fn time_critical_thread_priority_downgrades_when_advanced_is_hidden() {
    assert_eq!(
        ProcessThreadPrioritySetting::TimeCritical.safe_when_advanced_disabled(),
        ProcessThreadPrioritySetting::Highest
    );
    assert_eq!(
        ProcessThreadPrioritySetting::BelowNormal.safe_when_advanced_disabled(),
        ProcessThreadPrioritySetting::BelowNormal
    );
}

#[test]
fn high_io_priority_downgrades_when_advanced_is_hidden() {
    assert_eq!(
        ProcessIoPrioritySetting::Critical.safe_when_advanced_disabled(),
        ProcessIoPrioritySetting::Normal
    );
    assert_eq!(
        ProcessIoPrioritySetting::High.safe_when_advanced_disabled(),
        ProcessIoPrioritySetting::Normal
    );
    assert_eq!(
        ProcessIoPrioritySetting::Low.safe_when_advanced_disabled(),
        ProcessIoPrioritySetting::Low
    );
}

#[test]
fn high_gpu_priority_downgrades_when_advanced_is_hidden() {
    assert_eq!(
        ProcessGpuPrioritySetting::Realtime.safe_when_advanced_disabled(),
        ProcessGpuPrioritySetting::AboveNormal
    );
    assert_eq!(
        ProcessGpuPrioritySetting::High.safe_when_advanced_disabled(),
        ProcessGpuPrioritySetting::AboveNormal
    );
    assert_eq!(
        ProcessGpuPrioritySetting::BelowNormal.safe_when_advanced_disabled(),
        ProcessGpuPrioritySetting::BelowNormal
    );
}

#[test]
fn timer_resolution_rules_match_only_the_foreground_executable_path() {
    let settings = TimerResolutionSettings {
        enabled: true,
        desired_100ns: 10_000,
        rules: vec![
            TimerResolutionRule {
                enabled: true,
                executable_path: "C:\\Games\\game.exe".to_owned(),
                desired_100ns: 20_000,
            },
            TimerResolutionRule {
                enabled: false,
                executable_path: "C:\\Apps\\disabled.exe".to_owned(),
                desired_100ns: 10_000,
            },
        ],
    };

    assert_eq!(
        settings.desired_resolution_for_foreground("c:/games/GAME.exe"),
        Some(("C:\\Games\\game.exe", 20_000))
    );
    assert_eq!(
        settings.desired_resolution_for_foreground("C:\\Other\\game.exe"),
        None
    );
    assert_eq!(
        settings.desired_resolution_for_foreground("C:\\Apps\\disabled.exe"),
        None
    );
}

#[test]
fn disabled_suspendable_apps_remain_configured_but_do_not_match() {
    let settings = AppSuspensionSettings {
        enabled: true,
        background_delay_seconds: 60,
        temporary_thaw_enabled: false,
        temporary_thaw_interval_seconds: default_temporary_thaw_interval_seconds(),
        temporary_thaw_duration_seconds: default_temporary_thaw_duration_seconds(),
        network_wake_enabled: true,
        network_wake_duration_seconds: default_network_wake_duration_seconds(),
        audio_wake_enabled: true,
        audio_wake_duration_seconds: default_audio_wake_duration_seconds(),
        suspendable_apps: vec![AppSuspensionRule {
            enabled: false,
            executable_path: "chat.exe".to_owned(),
            network_wake_enabled: true,
            audio_wake_enabled: true,
            network_download_threshold_bytes: 1,
            network_download_threshold_unit: NetworkThresholdUnit::Bytes,
            network_upload_threshold_bytes: 0,
            network_upload_threshold_unit: NetworkThresholdUnit::Bytes,
        }],
    };

    assert!(settings.contains_suspendable_app("CHAT.EXE"));
    assert!(!settings.suspendable_app_enabled_for("chat.exe"));
    assert!(!settings.network_wake_enabled_for("chat.exe"));
    assert!(!settings.audio_wake_enabled_for("chat.exe"));
    assert_eq!(settings.network_wake_thresholds_for("chat.exe"), None);
}

#[test]
fn process_custom_rules_use_first_enabled_exact_path_override_for_every_priority_property() {
    let disabled = ProcessExclusionRule {
        enabled: false,
        executable_path: r"C:\Apps\worker.exe".to_owned(),
        process_foreground_priority: Some(ProcessPrioritySetting::Idle),
        thread_foreground_priority: Some(ProcessThreadPrioritySetting::Idle),
        dynamic_priority_boost_foreground: Some(ProcessDynamicPriorityBoostSetting::Disabled),
        io_foreground_priority: Some(ProcessIoPrioritySetting::VeryLow),
        gpu_foreground_priority: Some(ProcessGpuPrioritySetting::Idle),
        memory_foreground_priority: Some(ProcessMemoryPrioritySetting::VeryLow),
        ..Default::default()
    };
    let expected = ProcessExclusionRule {
        enabled: true,
        executable_path: r"C:\Apps\worker.exe".to_owned(),
        process_foreground_priority: Some(ProcessPrioritySetting::AboveNormal),
        process_visible_window_priority: Some(ProcessPrioritySetting::Normal),
        process_background_priority: Some(ProcessPrioritySetting::BelowNormal),
        thread_foreground_priority: Some(ProcessThreadPrioritySetting::Highest),
        thread_background_priority: Some(ProcessThreadPrioritySetting::Lowest),
        dynamic_priority_boost_foreground: Some(ProcessDynamicPriorityBoostSetting::Enabled),
        dynamic_priority_boost_background: Some(ProcessDynamicPriorityBoostSetting::Disabled),
        io_foreground_priority: Some(ProcessIoPrioritySetting::Normal),
        io_background_priority: Some(ProcessIoPrioritySetting::VeryLow),
        gpu_foreground_priority: Some(ProcessGpuPrioritySetting::AboveNormal),
        gpu_background_priority: Some(ProcessGpuPrioritySetting::Idle),
        memory_foreground_priority: Some(ProcessMemoryPrioritySetting::Normal),
        memory_background_priority: Some(ProcessMemoryPrioritySetting::VeryLow),
        ..Default::default()
    };
    let later_duplicate = ProcessExclusionRule {
        enabled: true,
        executable_path: r"C:\Apps\worker.exe".to_owned(),
        process_foreground_priority: Some(ProcessPrioritySetting::High),
        thread_foreground_priority: Some(ProcessThreadPrioritySetting::TimeCritical),
        dynamic_priority_boost_foreground: Some(ProcessDynamicPriorityBoostSetting::Disabled),
        io_foreground_priority: Some(ProcessIoPrioritySetting::Low),
        gpu_foreground_priority: Some(ProcessGpuPrioritySetting::Normal),
        memory_foreground_priority: Some(ProcessMemoryPrioritySetting::Medium),
        ..Default::default()
    };
    let exclusion = ProcessExclusionRule {
        enabled: true,
        executable_path: r"C:\Apps\excluded.exe".to_owned(),
        ..Default::default()
    };
    let rules = vec![disabled, expected, later_duplicate, exclusion];

    let process = ProcessPrioritySettings {
        exclusions: rules.clone(),
        ..Default::default()
    };
    let thread = ThreadPrioritySettings {
        exclusions: rules.clone(),
        ..Default::default()
    };
    let boost = DynamicPriorityBoostSettings {
        exclusions: rules.clone(),
        ..Default::default()
    };
    let io = IoPrioritySettings {
        exclusions: rules.clone(),
        ..Default::default()
    };
    let gpu = GpuPrioritySettings {
        exclusions: rules.clone(),
        ..Default::default()
    };
    let memory = MemoryPrioritySettings {
        exclusions: rules,
        ..Default::default()
    };

    assert_eq!(
        process.override_for(r"c:/apps/WORKER.exe", true, false),
        Some(Some(ProcessPrioritySetting::AboveNormal))
    );
    assert_eq!(
        process.override_for(r"c:/apps/WORKER.exe", false, true),
        Some(Some(ProcessPrioritySetting::Normal))
    );
    assert_eq!(
        process.override_for(r"c:/apps/WORKER.exe", false, false),
        Some(Some(ProcessPrioritySetting::BelowNormal))
    );
    assert_eq!(
        thread.override_for(r"c:/apps/WORKER.exe", true, false),
        Some(Some(ProcessThreadPrioritySetting::Highest))
    );
    assert_eq!(
        thread.override_for(r"c:/apps/WORKER.exe", false, false),
        Some(Some(ProcessThreadPrioritySetting::Lowest))
    );
    assert_eq!(
        boost.override_for(r"c:/apps/WORKER.exe", true, false),
        Some(Some(ProcessDynamicPriorityBoostSetting::Enabled))
    );
    assert_eq!(
        boost.override_for(r"c:/apps/WORKER.exe", false, false),
        Some(Some(ProcessDynamicPriorityBoostSetting::Disabled))
    );
    assert_eq!(
        io.override_for(r"c:/apps/WORKER.exe", true, false),
        Some(Some(ProcessIoPrioritySetting::Normal))
    );
    assert_eq!(
        io.override_for(r"c:/apps/WORKER.exe", false, false),
        Some(Some(ProcessIoPrioritySetting::VeryLow))
    );
    assert_eq!(
        gpu.override_for(r"c:/apps/WORKER.exe", true, false),
        Some(Some(ProcessGpuPrioritySetting::AboveNormal))
    );
    assert_eq!(
        gpu.override_for(r"c:/apps/WORKER.exe", false, false),
        Some(Some(ProcessGpuPrioritySetting::Idle))
    );
    assert_eq!(
        memory.override_for(r"c:/apps/WORKER.exe", true, false),
        Some(Some(ProcessMemoryPrioritySetting::Normal))
    );
    assert_eq!(
        memory.override_for(r"c:/apps/WORKER.exe", false, false),
        Some(Some(ProcessMemoryPrioritySetting::VeryLow))
    );

    assert_eq!(
        process.override_for(r"C:\Apps\excluded.exe", true, false),
        Some(None)
    );
    assert_eq!(
        thread.override_for(r"C:\Apps\excluded.exe", true, false),
        Some(None)
    );
    assert_eq!(
        boost.override_for(r"C:\Apps\excluded.exe", true, false),
        Some(None)
    );
    assert_eq!(
        io.override_for(r"C:\Apps\excluded.exe", true, false),
        Some(None)
    );
    assert_eq!(
        gpu.override_for(r"C:\Apps\excluded.exe", true, false),
        Some(None)
    );
    assert_eq!(
        memory.override_for(r"C:\Apps\excluded.exe", true, false),
        Some(None)
    );

    assert_eq!(
        process.override_for(r"C:\Other\worker.exe", true, false),
        None
    );
    assert_eq!(
        thread.override_for(r"C:\Other\worker.exe", true, false),
        None
    );
    assert_eq!(
        boost.override_for(r"C:\Other\worker.exe", true, false),
        None
    );
    assert_eq!(io.override_for(r"C:\Other\worker.exe", true, false), None);
    assert_eq!(gpu.override_for(r"C:\Other\worker.exe", true, false), None);
    assert_eq!(
        memory.override_for(r"C:\Other\worker.exe", true, false),
        None
    );
}
