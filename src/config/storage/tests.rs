use super::*;

#[test]
fn adaptive_processor_power_presets_round_trip_and_reject_invalid_entries() {
    use crate::config::{AdaptiveProcessorPowerPreset, PowerSourceProfile};

    let mut settings = Settings::default();
    let preset = AdaptiveProcessorPowerPreset::capture(
        "Quiet".into(),
        &settings.adaptive_engine,
        PowerSourceProfile::PluggedIn,
    );
    settings
        .adaptive_processor_power_presets
        .push(preset.clone());
    let path = std::env::temp_dir().join(format!(
        "winderust-adaptive-processor-power-presets-{}.toml",
        std::process::id()
    ));
    write_toml_settings(&path, &settings).unwrap();
    assert_eq!(load_from_path(&path).unwrap(), settings);
    fs::remove_file(&path).unwrap();
    let mut invalid = toml::Value::try_from(&settings).unwrap();
    invalid["adaptive_processor_power_presets"][0]["background_pressure_profile"]["policy"] =
        101.into();
    assert!(
        parse_toml_settings(&path, &toml::to_string(&invalid).unwrap())
            .unwrap_err()
            .contains("out-of-range processor settings")
    );

    let mut duplicate = preset;
    duplicate.name = " quiet ".into();
    settings.adaptive_processor_power_presets.push(duplicate);
    assert!(
        parse_toml_settings(&path, &toml::to_string(&settings).unwrap())
            .unwrap_err()
            .contains("is already in use")
    );
}

#[test]
fn export_destination_reserves_active_settings_even_when_missing() {
    let directory =
        std::env::temp_dir().join(format!("winderust-export-path-{}", std::process::id()));
    fs::create_dir(&directory).unwrap();
    let active = directory.join(CONFIG_FILE);
    let alias = directory.canonicalize().unwrap().join("SETTINGS.TOML");
    for path in [&active, &alias] {
        assert_eq!(
            export_destination(path, &active).unwrap_err().kind(),
            io::ErrorKind::InvalidInput
        );
    }
    assert!(!active.exists());
    let separate = directory.join("backup").join(CONFIG_FILE);
    assert_eq!(export_destination(&separate, &active).unwrap(), separate);
    fs::remove_dir(directory).unwrap();
}

#[test]
fn instance_limits_validation_guards_persistence_without_overwriting_files() {
    let path = std::env::temp_dir().join(format!(
        "winderust-instance-limits-{}.toml",
        std::process::id()
    ));
    for battery in [false, true] {
        for invalid in 0..3 {
            let mut settings = Settings::default();
            let profile = if battery {
                settings.battery_profile_mut()
            } else {
                &mut settings
            };
            let mut rule = crate::config::InstanceLimitsRule::new(r"C:\Apps\app.exe".into());
            match invalid {
                0 => rule.max_instances = 0,
                1 => rule.executable_path = "relative.exe".into(),
                _ => profile.instance_limits.rules.push(rule.clone()),
            }
            profile.instance_limits.rules.push(rule);
            let raw = toml::to_string(&settings).unwrap();
            fs::write(&path, &raw).unwrap();
            assert!(load_from_path(&path).is_err());
            assert!(import_toml_from(&path).is_err());
            assert_eq!(fs::read_to_string(&path).unwrap(), raw);
            fs::write(&path, "original settings").unwrap();
            assert!(write_toml_settings(&path, &settings).is_err());
            assert!(export_toml_to(&path, &settings).is_err());
            assert_eq!(fs::read_to_string(&path).unwrap(), "original settings");
        }
    }
    fs::remove_file(path).unwrap();
}

#[test]
fn keep_running_launch_validation_guards_load_import_save_and_export() {
    let path = std::env::temp_dir().join(format!(
        "winderust-keep-running-{}.toml",
        std::process::id()
    ));
    for battery in [false, true] {
        for invalid in 0..4 {
            let mut settings = Settings::default();
            let profile = if battery {
                settings.battery_profile_mut()
            } else {
                &mut settings
            };
            let mut rule = crate::config::KeepRunningRule::new(r"C:\Apps\test.exe".into());
            match invalid {
                0 => rule.executable_path = "relative.exe".into(),
                1 => rule.working_directory.push('\0'),
                2 => rule.arguments = "x".repeat(32_767),
                _ => profile.keep_running.rules.push(rule.clone()),
            }
            profile.keep_running.rules.push(rule);
            let raw = toml::to_string(&settings).unwrap();
            fs::write(&path, &raw).unwrap();
            assert!(load_from_path(&path).is_err());
            assert!(import_toml_from(&path).is_err());
            assert_eq!(fs::read_to_string(&path).unwrap(), raw);
            fs::write(&path, "original settings").unwrap();
            assert!(write_toml_settings(&path, &settings).is_err());
            assert!(export_toml_to(&path, &settings).is_err());
            assert_eq!(fs::read_to_string(&path).unwrap(), "original settings");
        }
    }
    fs::remove_file(path).unwrap();
}

#[test]
fn app_suspension_delay_limits_apply_to_both_profiles_before_io() {
    let path = std::env::temp_dir().join(format!(
        "winderust-suspension-delay-{}.toml",
        std::process::id()
    ));
    for battery in [false, true] {
        for (field, maximum) in [
            ("background_delay_seconds", 86_400),
            ("temporary_thaw_interval_seconds", 86_400),
            ("temporary_thaw_duration_seconds", 3600),
            ("network_wake_duration_seconds", 3600),
            ("audio_wake_duration_seconds", 3600),
        ] {
            for seconds in [0, 1, maximum, maximum + 1] {
                let mut settings = Settings::default();
                settings.battery_profile_mut();
                let mut value = toml::Value::try_from(&settings).unwrap();
                let profile = if battery {
                    &mut value["on_battery"]
                } else {
                    &mut value
                };
                profile["app_suspension"][field] = seconds.into();
                let raw = toml::to_string(&value).unwrap();
                fs::write(&path, &raw).unwrap();
                let valid = (1..=maximum).contains(&seconds);
                assert_eq!(
                    load_from_path(&path).is_ok(),
                    valid,
                    "{battery}/{field}/{seconds}"
                );
                assert_eq!(
                    import_toml_from(&path).is_ok(),
                    valid,
                    "{battery}/{field}/{seconds}"
                );
                assert_eq!(fs::read_to_string(&path).unwrap(), raw);

                let settings: Settings = value.try_into().unwrap();
                fs::write(&path, "existing settings").unwrap();
                let result = export_toml_to(&path, &settings);
                assert_eq!(result.is_ok(), valid, "{battery}/{field}/{seconds}");
                if !valid {
                    assert!(result.unwrap_err().contains(field));
                    assert_eq!(fs::read_to_string(&path).unwrap(), "existing settings");
                    assert!(write_toml_settings(&path, &settings).is_err());
                    assert_eq!(fs::read_to_string(&path).unwrap(), "existing settings");
                }
            }
        }
    }
    fs::remove_file(path).unwrap();
}

#[test]
fn unrepresentable_cpu_load_durations_fail_preflight_without_rewriting() {
    let path = std::env::temp_dir().join(format!(
        "winderust-cpu-duration-{}.toml",
        std::process::id()
    ));
    for battery in [false, true] {
        let mut settings = Settings::default();
        let profile = if battery {
            settings.battery_profile_mut()
        } else {
            &mut settings
        };
        for seconds in [i64::MAX as u64 + 1, u64::MAX] {
            profile.by_cpu_load.rules[0].duration_seconds = seconds;
            assert!(profile
                .validate_by_cpu_load_rules()
                .unwrap_err()
                .contains("duration"));
        }
        fs::write(&path, "existing settings").unwrap();
        assert!(export_toml_to(&path, &settings)
            .unwrap_err()
            .contains("duration"));
        assert_eq!(fs::read_to_string(&path).unwrap(), "existing settings");
    }
    fs::remove_file(path).unwrap();
}

#[test]
fn nested_battery_profiles_fail_load_import_and_export_without_rewriting() {
    let mut settings = Settings::default();
    settings.battery_profile_mut().battery_profile_mut();
    let path = std::env::temp_dir().join(format!(
        "winderust-nested-battery-{}.toml",
        std::process::id()
    ));
    let raw = toml::to_string(&settings).unwrap();
    fs::write(&path, &raw).unwrap();
    for result in [load_from_path(&path), import_toml_from(&path)] {
        assert!(result.unwrap_err().contains("another on_battery profile"));
    }
    assert_eq!(fs::read_to_string(&path).unwrap(), raw);
    let original = "existing export must survive";
    fs::write(&path, original).unwrap();
    assert!(export_toml_to(&path, &settings)
        .unwrap_err()
        .contains("another on_battery profile"));
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    fs::remove_file(path).unwrap();
}

#[test]
fn resource_thresholds_settings_persist_optional_durations_and_reject_invalid_rules() {
    let mut settings = Settings::default();
    let mut rule = crate::config::ResourceThresholdsRule::new(r"C:\Apps\test.exe".into());
    rule.memory.enabled = true;
    rule.memory.critical.threshold = 50;
    settings.resource_thresholds.rules.push(rule);
    let raw = toml::to_string(&settings).unwrap();
    assert!(parse_toml_settings(Path::new("test.toml"), &raw).is_err());
    assert!(settings.validate_resource_thresholds().is_err());
    settings.resource_thresholds.rules[0]
        .memory
        .critical
        .threshold = 90;
    settings
        .resource_thresholds
        .rules
        .push(crate::config::ResourceThresholdsRule::with_selector(
            crate::config::ResourceThresholdsSelector::ExactName {
                executable_name: "test.exe".into(),
            },
        ));
    settings.battery_profile_mut();
    settings.sync_shared_settings_to_battery();
    let raw = toml::to_string(&settings).unwrap();
    assert!(raw.contains("[[watchdog.rules]]"));
    assert!(raw.contains("[[on_battery.watchdog.rules]]"));
    assert_eq!(
        parse_toml_settings(Path::new("test.toml"), &raw).unwrap(),
        settings
    );

    let path = std::env::temp_dir().join(format!(
        "winderust-resource-thresholds-{}.toml",
        std::process::id()
    ));
    settings.resource_thresholds.rules[0].memory_unit =
        crate::config::ResourceThresholdsMemoryUnit::PrivateCommitMb;
    let rule = &mut settings.resource_thresholds.rules[0];
    rule.scope = crate::config::ResourceThresholdsScope::PerInstance;
    rule.memory.comparison = crate::config::ResourceThresholdsComparison::AtOrBelow;
    rule.memory.critical.threshold = 1;
    for duration in [Some(1), Some(3600), None] {
        settings.resource_thresholds.rules[0].sustained_seconds = duration;
        settings.sync_shared_settings_to_battery();
        write_toml_settings(&path, &settings).unwrap();
        assert_eq!(load_from_path(&path).unwrap(), settings);
    }
    for seconds in [0, 3601] {
        settings.resource_thresholds.rules[0].sustained_seconds = Some(seconds);
        let raw = toml::to_string(&settings).unwrap();
        assert!(parse_toml_settings(&path, &raw).is_err());
        assert!(write_toml_settings(&path, &settings).is_err());
    }
    settings.resource_thresholds.rules[0].sustained_seconds = None;
    settings.resource_thresholds.rules[1].cpu.critical.action =
        crate::config::ResourceThresholdsAction::Restart;
    settings.sync_shared_settings_to_battery();
    let raw = toml::to_string(&settings).unwrap();
    assert!(parse_toml_settings(&path, &raw).is_err());
    assert!(write_toml_settings(&path, &settings).is_err());
    settings.resource_thresholds.rules[1].cpu.critical.action =
        crate::config::ResourceThresholdsAction::None;
    let saved = load_from_path(&path).unwrap();
    settings.sync_shared_settings_to_battery();
    assert_eq!(saved, settings);
    settings.resource_thresholds.rules[0].cpu.critical.action =
        crate::config::ResourceThresholdsAction::Terminate;
    settings.sync_shared_settings_to_battery();
    let raw = toml::to_string(&settings).unwrap();
    assert!(parse_toml_settings(&path, &raw).is_err());
    assert!(write_toml_settings(&path, &settings).is_err());
    settings.resource_thresholds.rules[0].cpu.critical.action =
        crate::config::ResourceThresholdsAction::None;
    settings.resource_thresholds.rules[0].scope = crate::config::ResourceThresholdsScope::Combined;
    settings.resource_thresholds.rules[0].memory.critical.action =
        crate::config::ResourceThresholdsAction::Terminate;
    settings.sync_shared_settings_to_battery();
    let raw = toml::to_string(&settings).unwrap();
    assert!(parse_toml_settings(&path, &raw).is_err());
    assert!(write_toml_settings(&path, &settings).is_err());
    assert_eq!(load_from_path(&path).unwrap(), saved);
    fs::remove_file(path).unwrap();
}

#[test]
fn process_power_plan_rules_keep_flat_selectors_on_both_power_profiles() {
    let path = Path::new("power-plan-rules.toml");
    let path_rule = "executable_path = 'C:/Apps/game.exe'\npower_plan_guid = 'gaming-guid'";
    let name_rule = "executable_name = 'Game Tool.exe'\npower_plan_guid = 'gaming-guid'";
    for mode in ["By Focus", "By Running App"] {
        let mut settings = Settings::default();
        if mode == "By Focus" {
            settings.by_focus.rules = vec![toml::from_str(path_rule).unwrap()];
            settings.battery_profile_mut().by_focus.rules =
                vec![toml::from_str(name_rule).unwrap()];
        } else {
            settings.by_running_app.rules = vec![toml::from_str(path_rule).unwrap()];
            settings.battery_profile_mut().by_running_app.rules =
                vec![toml::from_str(name_rule).unwrap()];
        }

        let raw = toml::to_string(&settings).unwrap();
        assert!(raw.contains("executable_path = \"C:/Apps/game.exe\""));
        assert!(raw.contains("executable_name = \"Game Tool.exe\""));
        assert_eq!(parse_toml_settings(path, &raw).unwrap(), settings);

        for (changed, expected_error) in [
            (
                raw.replace(
                    "executable_name = \"Game Tool.exe\"",
                    "executable_name = \"Game Tool.exe\"\nexecutable_path = \"C:/Apps/game.exe\"",
                ),
                "exactly one executable path or executable name".to_owned(),
            ),
            (
                raw.replace("executable_name = \"Game Tool.exe\"\n", ""),
                "exactly one executable path or executable name".to_owned(),
            ),
            (
                raw.replace(
                    "executable_name = \"Game Tool.exe\"",
                    "executable_name = \"*.exe\"",
                ),
                format!("{mode} requires an exact executable filename"),
            ),
        ] {
            let error = parse_toml_settings(path, &changed).unwrap_err();
            assert!(error.contains(&expected_error), "{error}");
        }
    }
}

#[test]
fn invalid_by_time_clock_values_fail_load_and_import_without_rewriting() {
    let mut settings = Settings::default();
    settings.battery_profile_mut();
    let path = std::env::temp_dir().join(format!(
        "winderust-by-time-clock-{}.toml",
        std::process::id()
    ));
    for battery in [false, true] {
        for field in ["start_time", "end_time"] {
            for invalid in ["", "25:00", "12:60", "12:30:00"] {
                let mut value = toml::Value::try_from(&settings).unwrap();
                let profile = if battery {
                    &mut value["on_battery"]
                } else {
                    &mut value
                };
                profile["by_time"]["rules"][0][field] = invalid.into();
                let raw = toml::to_string(&value).unwrap();
                fs::write(&path, &raw).unwrap();
                for result in [load_from_path(&path), import_toml_from(&path)] {
                    assert!(result.unwrap_err().contains(field));
                }
                assert_eq!(fs::read_to_string(&path).unwrap(), raw);
            }
        }
    }
    fs::remove_file(path).unwrap();
}

use crate::config::{
    AccentSettings, ActionLogMode, AdaptiveEnginePreset, AdaptiveEngineProcessSettings,
    AdaptiveEngineSettings, AdaptivePressureSource, AdaptiveRestraintPriority,
    AdvancedPowerPlanTuningPreset, AdvancedSettings, AppLanguage, AppSuspensionRule,
    AppSuspensionSettings, AppThemeMode, BackgroundProcessorSelection, ByActivitySettings,
    ByCpuLoadRule, ByCpuLoadSettings, ByFocusRule, ByFocusSettings, ByRunningAppRule,
    ByRunningAppSettings, ByTimeRule, ByTimeSettings, CpuAllocationMethod, CpuAllocationPreset,
    CpuAllocationRule, CpuAllocationSettings, CpuLimiterRule, CpuLimiterSettings,
    CpuUsageComparison, DynamicPriorityBoostSettings, EfficiencyModeRule, EfficiencyModeSettings,
    GeneralSettings, GpuPrioritySettings, InputDetectionSettings, IoPrioritySettings,
    MemoryPrioritySettings, MemoryTrimSettings, NetworkThresholdUnit, PowerPlanSettings,
    ProcessDynamicPriorityBoostSetting, ProcessExclusionRule, ProcessGpuPrioritySetting,
    ProcessIoPrioritySetting, ProcessMemoryPrioritySetting, ProcessPrioritySetting,
    ProcessPrioritySettings, ProcessRuleMode, ProcessThreadPrioritySetting, ThreadPrioritySettings,
    TimerResolutionRule, TimerResolutionSettings, WeekdaySetting,
};

fn set_cpu_allocation_selections(settings: &mut Settings, mask: u64) {
    let rule = CpuAllocationRule {
        enabled: true,
        executable_path: r"C:\Apps\test.exe".into(),
        focus_selection: crate::cpu_topology::test_selection(mask),
        visible_window_selection: crate::cpu_topology::test_selection(mask),
        background_selection: crate::cpu_topology::test_selection(mask),
    };
    settings.cpu_sets_soft.rules = vec![rule.clone()];
    settings.processor_affinity_hard.rules = vec![rule];
    settings.cpu_allocation_presets = vec![CpuAllocationPreset {
        name: "Selected processors".into(),
        selection: crate::cpu_topology::test_selection(mask),
    }];
}

#[test]
fn bound_cpu_selections_survive_save_export_load_and_import() {
    let path = std::env::temp_dir().join(format!(
        "winderust-cpu-selections-save-{}.toml",
        std::process::id()
    ));
    let export_path = path.with_extension("export.toml");
    let masks = [0, 1 << 63, u64::MAX];
    for (index, root_mask) in masks.into_iter().enumerate() {
        let mut settings = Settings::default();
        set_cpu_allocation_selections(&mut settings, root_mask);
        settings.adaptive_engine_process.specific_processors =
            crate::cpu_topology::test_selection(root_mask);
        settings
            .adaptive_engine_process
            .dynamic_resource_zone_settings
            .specific_processors = crate::cpu_topology::test_selection(root_mask);
        let battery_mask = masks[(index + 1) % masks.len()];
        set_cpu_allocation_selections(settings.battery_profile_mut(), battery_mask);
        settings
            .battery_profile_mut()
            .adaptive_engine_process
            .specific_processors = crate::cpu_topology::test_selection(battery_mask);
        settings
            .battery_profile_mut()
            .adaptive_engine_process
            .dynamic_resource_zone_settings
            .specific_processors = crate::cpu_topology::test_selection(battery_mask);
        write_toml_settings(&path, &settings).unwrap();
        let raw = fs::read_to_string(&path).unwrap();
        assert_eq!(load_from_path(&path).unwrap(), settings);
        export_toml_to(&export_path, &settings).unwrap();
        assert_eq!(fs::read_to_string(&export_path).unwrap(), raw);
        assert_eq!(import_toml_from(&export_path).unwrap(), settings);
    }
    fs::remove_file(path).unwrap();
    fs::remove_file(export_path).unwrap();
}

#[test]
fn malformed_cpu_selections_fail_load_and_import_without_rewriting() {
    let mut settings = Settings::default();
    set_cpu_allocation_selections(&mut settings, u64::MAX);
    settings.battery_profile_mut();
    let path = std::env::temp_dir().join(format!(
        "winderust-cpu-selections-invalid-{}.toml",
        std::process::id()
    ));
    for battery in [false, true] {
        for (section, field) in [
            ("cpu_sets_soft", "focus_selection"),
            ("cpu_sets_soft", "visible_window_selection"),
            ("cpu_sets_soft", "background_selection"),
            ("processor_affinity_hard", "focus_selection"),
            ("processor_affinity_hard", "visible_window_selection"),
            ("processor_affinity_hard", "background_selection"),
            ("cpu_allocation_presets", "selection"),
        ] {
            for invalid in [
                "value = 0",
                "value = '0xffffffffffffffff'",
                "value = { processors = [{ group = 0, number = 64 }] }",
                "value = { processors = [{ group = 0, number = 1 }, { group = 0, number = 1 }] }",
            ] {
                let mut value = toml::Value::try_from(&settings).unwrap();
                let profile = if battery {
                    &mut value["on_battery"]
                } else {
                    &mut value
                };
                let rules = if section == "cpu_allocation_presets" {
                    &mut profile[section]
                } else {
                    &mut profile[section]["rules"]
                };
                rules[0][field] = toml::from_str::<toml::Value>(invalid).unwrap()["value"].clone();
                let raw = toml::to_string(&value).unwrap();
                fs::write(&path, &raw).unwrap();
                assert!(
                    load_from_path(&path).is_err(),
                    "{battery}/{section}/{field}"
                );
                assert!(
                    import_toml_from(&path).is_err(),
                    "{battery}/{section}/{field}"
                );
                assert_eq!(fs::read_to_string(&path).unwrap(), raw);
            }
        }
    }
    fs::remove_file(path).unwrap();
}

#[test]
fn unresolved_cpu_selections_are_preserved_across_profiles_and_presets() {
    use crate::cpu_topology::{CpuSelection, ProcessorId};
    let unresolved = CpuSelection::new(
        vec![ProcessorId {
            group: 1,
            number: 63,
        }],
        None,
    )
    .unwrap();
    let mut settings = Settings::default();
    set_cpu_allocation_selections(&mut settings, 1);
    settings.cpu_sets_soft.rules[0].focus_selection = unresolved.clone();
    settings.cpu_allocation_presets[0].selection = unresolved.clone();
    settings.adaptive_engine_process.specific_processors = unresolved.clone();
    settings
        .adaptive_engine_process
        .dynamic_resource_zone_settings
        .specific_processors = unresolved.clone();
    settings.adaptive_engine_presets.push(AdaptiveEnginePreset {
        name: "Review selection".into(),
        processor_power_policy_enabled: false,
        base_processor_policy: settings.adaptive_engine.base_processor_policy,
        background_pressure_profile: settings.adaptive_engine.background_pressure_profile,
        focus_and_launch_profile: settings.adaptive_engine.focus_and_launch_profile,
        adaptive_engine_process: settings.adaptive_engine_process.clone(),
    });
    settings.battery_profile_mut();
    let path = std::env::temp_dir().join(format!(
        "winderust-unresolved-selections-{}.toml",
        std::process::id()
    ));
    write_toml_settings(&path, &settings).unwrap();
    let raw = fs::read_to_string(&path).unwrap();
    for parsed in [
        load_from_path(&path).unwrap(),
        import_toml_from(&path).unwrap(),
    ] {
        assert_eq!(parsed, settings);
        assert!(parsed
            .adaptive_engine_process
            .specific_processors
            .resolve_group_zero(&crate::cpu_topology::test_topology())
            .is_err());
    }
    assert_eq!(fs::read_to_string(&path).unwrap(), raw);
    fs::remove_file(path).unwrap();
}

#[test]
fn invalid_cpu_load_thresholds_fail_load_and_import_without_rewriting() {
    let mut settings = Settings::default();
    settings.battery_profile_mut();
    let path = std::env::temp_dir().join(format!(
        "winderust-cpu-load-thresholds-{}.toml",
        std::process::id()
    ));
    for battery in [false, true] {
        for (lower, upper) in [(101, 100), (20, 255), (80, 20)] {
            let mut value = toml::Value::try_from(&settings).unwrap();
            let profile = if battery {
                &mut value["on_battery"]
            } else {
                &mut value
            };
            let rule = &mut profile["by_cpu_load"]["rules"][0];
            rule["comparison"] = "between".into();
            rule["threshold_percent"] = lower.into();
            rule.as_table_mut()
                .unwrap()
                .insert("upper_threshold_percent".into(), upper.into());
            let raw = toml::to_string(&value).unwrap();
            fs::write(&path, &raw).unwrap();
            assert!(load_from_path(&path).is_err());
            assert!(import_toml_from(&path).is_err());
            assert_eq!(fs::read_to_string(&path).unwrap(), raw);
        }
    }
    fs::remove_file(path).unwrap();
}

#[test]
fn reversed_cpu_load_ranges_fail_export_without_overwriting() {
    let path = std::env::temp_dir().join(format!(
        "winderust-cpu-load-export-{}.toml",
        std::process::id()
    ));
    fs::write(&path, "original settings").unwrap();
    for battery in [false, true] {
        let mut settings = Settings::default();
        let profile = if battery {
            settings.battery_profile_mut()
        } else {
            &mut settings
        };
        let rule = &mut profile.by_cpu_load.rules[0];
        rule.comparison = CpuUsageComparison::Between;
        rule.threshold_percent = crate::config::CpuLoadPercent::new(80).unwrap();
        rule.upper_threshold_percent = Some(crate::config::CpuLoadPercent::new(20).unwrap());
        assert!(export_toml_to(&path, &settings)
            .unwrap_err()
            .contains("upper threshold"));
        assert_eq!(fs::read_to_string(&path).unwrap(), "original settings");
    }
    fs::remove_file(path).unwrap();
}

#[test]
fn cpu_load_stability_validates_both_profiles_without_overwriting_invalid_settings() {
    use crate::config::{ByCpuLoadStability, CpuLoadPercent};

    let path = std::env::temp_dir().join(format!(
        "winderust-cpu-load-stability-{}.toml",
        std::process::id()
    ));
    for battery in [false, true] {
        let mut settings = Settings::default();
        let profile = if battery {
            settings.battery_profile_mut()
        } else {
            &mut settings
        };
        profile.by_cpu_load.rules[0].stability = Some(ByCpuLoadStability {
            exit_threshold_percent: CpuLoadPercent::new(20).unwrap(),
            exit_upper_threshold_percent: None,
            minimum_hold_seconds: 30,
        });
        export_toml_to(&path, &settings).unwrap();
        assert_eq!(import_toml_from(&path).unwrap(), settings);
        for (field, invalid) in [
            ("exit_threshold_percent", 14),
            ("exit_threshold_percent", 101),
            ("exit_upper_threshold_percent", 80),
            ("minimum_hold_seconds", -1),
        ] {
            let mut value = toml::Value::try_from(&settings).unwrap();
            let profile = if battery {
                &mut value["on_battery"]
            } else {
                &mut value
            };
            profile["by_cpu_load"]["rules"][0]["stability"]
                .as_table_mut()
                .unwrap()
                .insert(field.into(), invalid.into());
            let raw = toml::to_string(&value).unwrap();
            fs::write(&path, &raw).unwrap();
            assert!(load_from_path(&path).is_err(), "{battery}/{field}");
            assert!(import_toml_from(&path).is_err(), "{battery}/{field}");
            assert_eq!(fs::read_to_string(&path).unwrap(), raw);
        }
        let profile = if battery {
            settings.battery_profile_mut()
        } else {
            &mut settings
        };
        profile.by_cpu_load.rules[0]
            .stability
            .as_mut()
            .unwrap()
            .minimum_hold_seconds = u64::MAX;
        fs::write(&path, "existing settings").unwrap();
        assert!(export_toml_to(&path, &settings)
            .unwrap_err()
            .contains("minimum hold"));
        assert!(write_toml_settings(&path, &settings).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), "existing settings");
    }
    fs::remove_file(path).unwrap();
}

#[test]
fn independent_zone_settings_validate_root_battery_and_presets_without_rewriting() {
    let mut settings = Settings::default();
    settings.adaptive_engine_process.processor_limit_percent = 40;
    settings.adaptive_engine_process.specific_processors = crate::cpu_topology::test_selection(1);
    settings
        .adaptive_engine_process
        .dynamic_resource_zone_settings
        .foreground_share_percent = 80;
    settings
        .adaptive_engine_process
        .dynamic_resource_zone_settings
        .specific_processors = crate::cpu_topology::test_selection(2);
    let raw = toml::to_string(&settings).unwrap();
    assert_eq!(toml::from_str::<Settings>(&raw).unwrap(), settings);
    let mut value = toml::Value::try_from(&settings).unwrap();
    value["adaptive_engine_process"]
        .as_table_mut()
        .unwrap()
        .remove("dynamic_resource_zone_settings");
    let path = std::env::temp_dir().join(format!(
        "winderust-zone-cutover-{}.toml",
        std::process::id()
    ));
    for enabled in [false, true] {
        value["adaptive_engine_process"]["dynamic_resource_zones_enabled"] =
            toml::Value::Boolean(enabled);
        let raw = toml::to_string(&value).unwrap();
        fs::write(&path, &raw).unwrap();
        for result in [load_from_path(&path), import_toml_from(&path)] {
            if enabled {
                assert!(result
                    .unwrap_err()
                    .contains("explicit dynamic_resource_zone_settings block"));
            } else {
                let parsed = result.unwrap();
                assert_eq!(parsed.adaptive_engine_process.processor_limit_percent, 40);
                assert_eq!(
                    parsed.adaptive_engine_process.specific_processors,
                    crate::cpu_topology::test_selection(1)
                );
                assert_eq!(
                    parsed
                        .adaptive_engine_process
                        .dynamic_resource_zone_settings,
                    Default::default()
                );
            }
            assert_eq!(fs::read_to_string(&path).unwrap(), raw);
        }
        assert_eq!(toml::from_str::<Settings>(&raw).is_err(), enabled);
    }
    fs::remove_file(path).unwrap();
    let invalid_process = value["adaptive_engine_process"].clone();
    let mut battery = toml::Value::try_from(&settings).unwrap();
    battery["adaptive_engine_process"] = invalid_process.clone();
    value = toml::Value::try_from(&settings).unwrap();
    value
        .as_table_mut()
        .unwrap()
        .insert("on_battery".into(), battery);
    assert!(value.try_into::<Settings>().is_err());
    let preset = AdaptiveEnginePreset {
        name: "invalid zone".into(),
        processor_power_policy_enabled: false,
        base_processor_policy: settings.adaptive_engine.base_processor_policy,
        background_pressure_profile: settings.adaptive_engine.background_pressure_profile,
        focus_and_launch_profile: settings.adaptive_engine.focus_and_launch_profile,
        adaptive_engine_process: settings.adaptive_engine_process.clone(),
    };
    let mut preset = toml::Value::try_from(preset).unwrap();
    preset["adaptive_engine_process"] = invalid_process;
    value = toml::Value::try_from(&settings).unwrap();
    value["adaptive_engine_presets"] = toml::Value::Array(vec![preset]);
    assert!(value.try_into::<Settings>().is_err());
}

#[test]
fn invalid_zone_configuration_is_rejected_before_save_or_deserialization() {
    for (share, enabled) in [(0, false), (100, false), (75, true)] {
        let mut settings = Settings::default();
        let process = &mut settings.adaptive_engine_process;
        process.dynamic_resource_zones_enabled = enabled;
        process
            .dynamic_resource_zone_settings
            .foreground_share_percent = share;
        process
            .dynamic_resource_zone_settings
            .background_processor_selection = BackgroundProcessorSelection::Custom;
        process.dynamic_resource_zone_settings.specific_processors =
            crate::cpu_topology::CpuSelection::default();
        assert!(settings.validate_dynamic_resource_zones().is_err());
        assert!(toml::from_str::<Settings>(&toml::to_string(&settings).unwrap()).is_err());
    }
}

#[test]
fn omitted_cpu_behaviour_settings_use_defaults_independent_of_sampling_interval() {
    let mut settings = toml::Value::try_from(AdaptiveEngineProcessSettings::default()).unwrap();
    let fields = settings.as_table_mut().unwrap();
    fields.insert("reaction_time_ms".into(), toml::Value::Integer(750));
    for key in [
        "pressure_source",
        "exclude_service_hosts",
        "preserve_custom_affinity",
        "processors_to_remove",
        "restraint_trigger_time_ms",
        "pause_restraint_when_idle",
        "restraint_idle_timeout_seconds",
        "restraint_priority",
        "maximum_restraint_time_seconds",
    ] {
        assert!(fields.remove(key).is_some());
    }
    assert_eq!(
        settings
            .try_into::<AdaptiveEngineProcessSettings>()
            .unwrap(),
        AdaptiveEngineProcessSettings {
            reaction_time_ms: 750,
            ..Default::default()
        }
    );
}

#[test]
fn invalid_cpu_behaviour_values_are_rejected_for_live_profiles_and_presets() {
    let maximum_error = rust_i18n::t!("adaptive_engine_process.maximum_restraint_time_invalid");
    let removal_error = rust_i18n::t!("adaptive_engine_process.processors_to_remove_invalid");
    for (trigger, idle, maximum, remove, field) in [
        (249, 300, 0, 1, "time above threshold"),
        (60_001, 300, 0, 1, "time above threshold"),
        (1_000, 0, 0, 1, "idle timeout"),
        (1_000, 3_601, 0, 1, "idle timeout"),
        (1_000, 300, 2, 1, maximum_error.as_ref()),
        (1_000, 300, 3_601, 1, maximum_error.as_ref()),
        (1_000, 300, 0, 0, removal_error.as_ref()),
        (1_000, 300, 0, 64, removal_error.as_ref()),
    ] {
        let process = AdaptiveEngineProcessSettings {
            restraint_trigger_time_ms: trigger,
            restraint_idle_timeout_seconds: idle,
            maximum_restraint_time_seconds: maximum,
            processors_to_remove: remove,
            ..Default::default()
        };
        let root = Settings {
            adaptive_engine_process: process.clone(),
            ..Default::default()
        };
        let mut battery = Settings::default();
        battery.battery_profile_mut().adaptive_engine_process = process.clone();
        let mut preset = Settings::default();
        preset.adaptive_engine_presets.push(AdaptiveEnginePreset {
            name: "Invalid tuning".into(),
            processor_power_policy_enabled: false,
            base_processor_policy: preset.adaptive_engine.base_processor_policy,
            background_pressure_profile: preset.adaptive_engine.background_pressure_profile,
            focus_and_launch_profile: preset.adaptive_engine.focus_and_launch_profile,
            adaptive_engine_process: process,
        });
        for settings in [root, battery, preset] {
            assert!(settings.validate().unwrap_err().contains(field));
            let raw = toml::to_string(&settings).unwrap();
            assert!(toml::from_str::<Settings>(&raw)
                .unwrap_err()
                .to_string()
                .contains(field));
        }
    }
}

#[test]
fn load_and_import_normalize_saved_colors_without_rewriting_the_file() {
    let path = std::env::temp_dir().join(format!("winderust-palette-{}.toml", std::process::id()));
    let mut settings = Settings::default();
    settings.general.accent.source = crate::config::AccentColorSource::Custom;
    settings.general.accent.custom_color = 0xabcdef;
    settings.general.accent.custom_colors = vec![0xabcdef, 0xabcdef, 0x112233, 0xabcdef, 0x112233];
    let raw = toml::to_string_pretty(&settings).unwrap();
    fs::write(&path, &raw).unwrap();
    for parsed in [
        load_from_path(&path).unwrap(),
        import_toml_from(&path).unwrap(),
    ] {
        assert_eq!(
            parsed.general.accent.custom_colors,
            vec![0xabcdef, 0x112233]
        );
        assert_eq!(parsed.general.accent.custom_color, 0xabcdef);
        assert_eq!(
            parsed.general.accent.source,
            crate::config::AccentColorSource::Custom
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), raw);
    }
    fs::remove_file(path).unwrap();
    let empty: AccentSettings = toml::from_str("").unwrap();
    assert!(empty.custom_colors.is_empty());
    assert!(toml::from_str::<AccentSettings>("custom_colors = [1, 'invalid']").is_err());
}

#[test]
fn omitted_adaptive_priority_options_use_behavior_preserving_defaults() {
    let defaults = AdaptiveEngineProcessSettings::default();
    let raw = toml::to_string(&defaults).unwrap();
    let raw = raw
        .lines()
        .filter(|line| {
            ![
                "process_priority_foreground_detection_enabled",
                "process_priority_visible_window_detection_enabled",
                "process_priority_preserve_",
                "memory_priority_foreground_detection_enabled",
                "memory_priority_visible_window_detection_enabled",
                "memory_priority_preserve_",
            ]
            .iter()
            .any(|key| line.starts_with(key))
        })
        .collect::<Vec<_>>()
        .join("\n");
    let parsed: AdaptiveEngineProcessSettings = toml::from_str(&raw).unwrap();
    assert_eq!(parsed, defaults);
}

#[test]
fn efficiency_mode_rule_without_tiers_remains_an_exclusion() {
    let rule: EfficiencyModeRule = toml::from_str(
        r#"
            executable_path = "C:\\Apps\\legacy.exe"
        "#,
    )
    .expect("existing Efficiency Mode rule should parse");

    assert!(rule.enabled);
    assert_eq!(rule.focus_efficiency_mode, ProcessRuleMode::Disabled);
    assert_eq!(
        rule.visible_window_efficiency_mode,
        ProcessRuleMode::Disabled
    );
    assert_eq!(rule.background_efficiency_mode, ProcessRuleMode::Disabled);
}

#[test]
fn efficiency_mode_reads_only_its_current_section_on_both_profiles() {
    let mut settings = Settings::default();
    settings.efficiency_mode.enabled = true;
    settings
        .efficiency_mode
        .custom_rules
        .push(EfficiencyModeRule {
            enabled: true,
            executable_path: r"C:\Apps\worker.exe".into(),
            focus_efficiency_mode: ProcessRuleMode::Disabled,
            visible_window_efficiency_mode: ProcessRuleMode::Disabled,
            background_efficiency_mode: ProcessRuleMode::Enabled,
        });
    settings
        .battery_profile_mut()
        .efficiency_mode
        .background_efficiency_mode = false;
    let raw = toml::to_string_pretty(&settings).unwrap();
    assert!(raw.contains("[efficiency_mode]"));
    assert!(raw.contains("[on_battery.efficiency_mode]"));
    assert_eq!(toml::from_str::<Settings>(&raw).unwrap(), settings);

    let legacy = raw
        .replace("efficiency_mode]", "background_efficiency]")
        .replace(
            "efficiency_mode.custom_rules]]",
            "background_efficiency.custom_rules]]",
        );
    let parsed: Settings = toml::from_str(&legacy).unwrap();
    let mut defaults = settings;
    defaults.efficiency_mode = EfficiencyModeSettings::default();
    defaults.battery_profile_mut().efficiency_mode = EfficiencyModeSettings::default();
    assert_eq!(parsed, defaults);
}

#[test]
fn cpu_limiter_schema_defaults_rules_to_the_page_policy() {
    let defaults =
        toml::to_string_pretty(&Settings::default()).expect("default settings should serialize");
    assert!(defaults.contains("focus_allowed_cpu_time_percent = 100"));
    let raw = defaults.replace(
        "background_allowed_cpu_time_percent = 50\nrules = []",
        "background_allowed_cpu_time_percent = 50",
    );
    let raw = format!("{raw}\n[[cpu_limiter.rules]]\nexecutable_path = \"encoder.exe\"\n");

    let parsed: Settings = toml::from_str(&raw).expect("CPU Limiter settings should parse");
    let round_trip = toml::to_string_pretty(&parsed).expect("settings should serialize");

    assert!(round_trip.contains("[[cpu_limiter.rules]]"));
    assert!(round_trip.contains("focus_allowed_cpu_time_percent = 50"));
    assert!(round_trip.contains("visible_window_allowed_cpu_time_percent = 50"));
    assert!(round_trip.contains("background_allowed_cpu_time_percent = 50"));
    assert!(!round_trip
        .lines()
        .any(|line| line.trim_start().starts_with("allowed_cpu_time_percent =")));
    assert!(round_trip.contains("focus_mode = \"default\""));
    assert!(round_trip.contains("visible_window_mode = \"default\""));
    assert!(round_trip.contains("background_mode = \"default\""));
    assert!(parsed.cpu_limiter.rules[0].affinity_limit.is_none());
}

#[test]
fn cpu_limiter_tier_percentages_round_trip_at_boundaries() {
    let rule = CpuLimiterRule {
        enabled: true,
        executable_path: r"C:\Apps\encoder.exe".to_owned(),
        focus_mode: ProcessRuleMode::Enabled,
        visible_window_mode: ProcessRuleMode::Enabled,
        background_mode: ProcessRuleMode::Enabled,
        focus_allowed_cpu_time_percent: 1,
        visible_window_allowed_cpu_time_percent: 50,
        background_allowed_cpu_time_percent: 100,
        affinity_limit: None,
    };

    let serialized = toml::to_string_pretty(&rule).expect("CPU Limiter rule should serialize");
    let parsed: CpuLimiterRule =
        toml::from_str(&serialized).expect("CPU Limiter rule should parse");

    assert_eq!(parsed, rule);
}

#[test]
fn cpu_limiter_affinity_limits_round_trip_and_validate_in_both_profiles() {
    use crate::config::CpuLimiterAffinityLimit;
    use crate::cpu_topology::test_selection;

    let mut rule: CpuLimiterRule =
        toml::from_str(r"executable_path = 'C:\Apps\encoder.exe'").unwrap();
    rule.affinity_limit = Some(CpuLimiterAffinityLimit {
        trigger_percent: 25,
        sustain_seconds: 5,
        selection: test_selection(1),
    });
    let mut battery = Settings::default();
    battery.cpu_limiter.rules.push(rule);
    let settings = Settings {
        on_battery: Some(Box::new(battery)),
        ..Settings::default()
    };
    assert!(settings.validate().is_ok());

    let raw = toml::to_string_pretty(&settings).unwrap();
    let mut parsed: Settings = toml::from_str(&raw).unwrap();
    assert_eq!(parsed.on_battery, settings.on_battery);
    let rule = &mut parsed.on_battery.as_mut().unwrap().cpu_limiter.rules[0];
    rule.affinity_limit.as_mut().unwrap().sustain_seconds = 601;
    assert!(parsed.validate().is_err());
}

#[test]
fn only_missing_settings_use_defaults() {
    let path = std::env::temp_dir().join(format!(
        "winderust-settings-load-test-{}.toml",
        std::process::id()
    ));
    let _ = fs::remove_file(&path);

    assert_eq!(load_from_path(&path).unwrap(), Settings::default());
    fs::write(&path, "invalid = [").unwrap();
    assert!(load_from_path(&path).is_err());

    fs::remove_file(path).unwrap();
}

#[test]
fn atomic_write_replaces_existing_file() {
    let path = std::env::temp_dir().join(format!(
        "winderust-atomic-write-test-{}.txt",
        std::process::id()
    ));
    fs::write(&path, "old").unwrap();

    write_bytes_atomically(&path, b"new").unwrap();

    assert_eq!(fs::read_to_string(&path).unwrap(), "new");
    fs::remove_file(path).unwrap();
}

#[test]
fn toml_round_trip_preserves_settings() {
    let mut adaptive_engine = AdaptiveEngineSettings::default();
    adaptive_engine.enabled = true;
    adaptive_engine.adaptive_processor_power_enabled = true;
    let processor_power_preset = crate::config::AdaptiveProcessorPowerPreset::capture(
        "Quiet power".into(),
        &adaptive_engine,
        crate::config::PowerSourceProfile::OnBattery,
    );
    let mut settings = Settings {
        general: GeneralSettings {
            enabled: false,
            startup_with_windows: true,
            start_minimized: true,
            hide_to_tray: true,
            allow_cross_session_process_control: false,
            check_for_updates: true,
            update_channel: crate::config::UpdateChannel::PreRelease,
            theme_mode: AppThemeMode::Dark,
            accent: AccentSettings::default(),
            language: AppLanguage::ZhTw,
            show_enabled_feature_counts_in_sidebar: false,
            show_feature_status_on_cards: false,
            animation_mode: crate::config::AnimationMode::Off,
            pause_power_plan_switching_while_plugged_in: true,
            restore_previous_power_plan: true,
            search_hotkeys: crate::config::SearchHotkeys::default(),
            show_winderust_shortcut: None,
            check_interval_ms: 2_500,
        },
        advanced: AdvancedSettings {
            action_log_mode: ActionLogMode::Error,
            execution_failure_suppression_threshold: 5,
            expose_all_priority_values: true,
            show_advanced_controls: true,
        },
        adaptive_engine,
        adaptive_engine_presets: vec![AdaptiveEnginePreset {
            name: "Quiet work".to_owned(),
            processor_power_policy_enabled: true,
            base_processor_policy: crate::power::plan::ProcessorPowerValues::new_with_boost_mode(
                25,
                5,
                80,
                40,
                crate::power::plan::ProcessorBoostMode::EfficientEnabled,
            ),
            background_pressure_profile:
                crate::power::AdaptivePowerBoostValues::BACKGROUND_PRESSURE,
            focus_and_launch_profile: crate::power::AdaptivePowerBoostValues::FOCUS_AND_LAUNCH,
            adaptive_engine_process: AdaptiveEngineProcessSettings::default(),
        }],
        adaptive_processor_power_presets: vec![processor_power_preset],
        by_activity: ByActivitySettings {
            enabled: true,
            idle_timeout_seconds: 12,
            switch_to_performance_on_resume: true,
            input_detection: InputDetectionSettings {
                keyboard: true,
                mouse: false,
                controller: true,
            },
            power_plans: PowerPlanSettings {
                power_save_guid: Some("activity-idle-guid".to_owned()),
                performance_guid: Some("activity-active-guid".to_owned()),
            },
        },
        by_focus: ByFocusSettings {
            enabled: true,
            rules: vec![
                ByFocusRule {
                    enabled: true,
                    name: "Game plan".to_owned(),
                    selector: crate::config::ExecutableSelector::ExactPath {
                        executable_path: "game.exe".to_owned(),
                    },
                    power_plan_guid: Some("gaming-guid".to_owned()),
                },
                ByFocusRule {
                    enabled: false,
                    name: "Backup plan".to_owned(),
                    selector: crate::config::ExecutableSelector::ExactPath {
                        executable_path: "backup\\tool.exe".to_owned(),
                    },
                    power_plan_guid: Some("backup-guid".to_owned()),
                },
            ],
        },
        by_time: ByTimeSettings {
            enabled: true,
            rules: vec![ByTimeRule {
                enabled: true,
                name: "Work hours".to_owned(),
                days: vec![WeekdaySetting::Mon, WeekdaySetting::Fri],
                start_time: "09:00".parse().unwrap(),
                end_time: "17:30".parse().unwrap(),
                power_plan_guid: Some("work-hours-guid".to_owned()),
            }],
        },
        by_cpu_load: ByCpuLoadSettings {
            enabled: true,
            rules: vec![ByCpuLoadRule {
                enabled: true,
                name: "Low CPU".to_owned(),
                comparison: CpuUsageComparison::AtOrBelow,
                threshold_percent: crate::config::CpuLoadPercent::new(18).unwrap(),
                upper_threshold_percent: None,
                duration_seconds: 45,
                stability: None,
                power_plan_guid: Some("low-cpu-guid".to_owned()),
                else_enabled: true,
                else_power_plan_guid: Some("normal-cpu-guid".to_owned()),
            }],
        },
        efficiency_mode: EfficiencyModeSettings {
            enabled: true,
            foreground_detection_enabled: false,
            visible_window_detection_enabled: true,
            foreground_efficiency_mode: false,
            visible_window_efficiency_mode: false,
            background_efficiency_mode: true,
            custom_rules: vec![
                EfficiencyModeRule {
                    enabled: true,
                    executable_path: "mouse.exe".to_owned(),
                    focus_efficiency_mode: ProcessRuleMode::Disabled,
                    visible_window_efficiency_mode: ProcessRuleMode::Disabled,
                    background_efficiency_mode: ProcessRuleMode::Disabled,
                },
                EfficiencyModeRule {
                    enabled: false,
                    executable_path: "comma,app.exe".to_owned(),
                    focus_efficiency_mode: ProcessRuleMode::Disabled,
                    visible_window_efficiency_mode: ProcessRuleMode::Disabled,
                    background_efficiency_mode: ProcessRuleMode::Disabled,
                },
            ],
        },
        app_suspension: AppSuspensionSettings {
            enabled: true,
            background_delay_seconds: 120,
            temporary_thaw_enabled: true,
            temporary_thaw_interval_seconds: 600,
            temporary_thaw_duration_seconds: 15,
            network_wake_enabled: true,
            network_wake_duration_seconds: 20,
            audio_wake_enabled: true,
            audio_wake_duration_seconds: 8,
            suspendable_apps: vec![
                AppSuspensionRule {
                    enabled: true,
                    executable_path: "chat.exe".to_owned(),
                    network_wake_enabled: true,
                    audio_wake_enabled: true,
                    network_download_threshold_bytes: 1,
                    network_download_threshold_unit: NetworkThresholdUnit::Bytes,
                    network_upload_threshold_bytes: 0,
                    network_upload_threshold_unit: NetworkThresholdUnit::Bytes,
                },
                AppSuspensionRule {
                    enabled: false,
                    executable_path: "comma,app.exe".to_owned(),
                    network_wake_enabled: false,
                    audio_wake_enabled: false,
                    network_download_threshold_bytes: 1,
                    network_download_threshold_unit: NetworkThresholdUnit::Bytes,
                    network_upload_threshold_bytes: 0,
                    network_upload_threshold_unit: NetworkThresholdUnit::Bytes,
                },
            ],
        },
        cpu_sets_soft: CpuAllocationSettings {
            enabled: true,
            rules: vec![
                CpuAllocationRule {
                    enabled: true,
                    executable_path: "backup.exe".to_owned(),
                    focus_selection: crate::cpu_topology::test_selection(0b1111),
                    visible_window_selection: crate::cpu_topology::test_selection(0b0111),
                    background_selection: crate::cpu_topology::test_selection(0b0011),
                },
                CpuAllocationRule {
                    enabled: false,
                    executable_path: "indexer.exe".to_owned(),
                    focus_selection: crate::cpu_topology::test_selection(0b1111),
                    visible_window_selection: crate::cpu_topology::test_selection(0b1100),
                    background_selection: crate::cpu_topology::test_selection(0b1000),
                },
            ],
        },
        processor_affinity_hard: CpuAllocationSettings {
            enabled: true,
            rules: vec![CpuAllocationRule {
                enabled: true,
                executable_path: "game.exe".to_owned(),
                focus_selection: crate::cpu_topology::test_selection(0b1111),
                visible_window_selection: crate::cpu_topology::test_selection(0b0111),
                background_selection: crate::cpu_topology::test_selection(0b0101),
            }],
        },
        cpu_allocation_presets: vec![CpuAllocationPreset {
            name: "Performance cores".to_owned(),
            selection: crate::cpu_topology::test_selection(0b0101),
        }],
        advanced_power_plan_tuning_presets: vec![AdvancedPowerPlanTuningPreset {
            name: "Plugged in performance".to_owned(),
            values: crate::power::plan::ProcessorPowerValues::new_with_boost_mode(
                100,
                100,
                100,
                100,
                crate::power::plan::ProcessorBoostMode::Aggressive,
            )
            .into(),
        }],
        cpu_limiter: CpuLimiterSettings {
            enabled: true,
            focus_allowed_cpu_time_percent: 100,
            visible_window_allowed_cpu_time_percent: 50,
            background_allowed_cpu_time_percent: 50,
            rules: vec![CpuLimiterRule {
                enabled: true,
                executable_path: "encoder.exe".to_owned(),
                focus_mode: ProcessRuleMode::Disabled,
                visible_window_mode: ProcessRuleMode::Disabled,
                background_mode: ProcessRuleMode::Enabled,
                focus_allowed_cpu_time_percent: 35,
                visible_window_allowed_cpu_time_percent: 35,
                background_allowed_cpu_time_percent: 35,
                affinity_limit: None,
            }],
        },
        by_running_app: ByRunningAppSettings {
            enabled: true,
            rules: vec![ByRunningAppRule {
                enabled: true,
                name: "Game performance".to_owned(),
                selector: crate::config::ExecutableSelector::ExactPath {
                    executable_path: "game.exe".into(),
                },
                power_plan_guid: Some("gaming-guid".to_owned()),
            }],
        },
        adaptive_engine_process: AdaptiveEngineProcessSettings {
            process_priority_enabled: true,
            process_priority_foreground_detection_enabled: true,
            process_priority_visible_window_detection_enabled: true,
            process_priority_preserve_foreground: false,
            process_priority_preserve_visible_window: false,
            process_priority_preserve_background: false,
            background_efficiency_enabled: true,
            focus_process_background_efficiency_override_enabled: true,
            visible_window_background_efficiency_override_enabled: true,
            focus_process_background_efficiency_mode: false,
            visible_window_background_efficiency_mode: false,
            background_efficiency_mode: true,
            focus_process_priority: ProcessPrioritySetting::AboveNormal,
            background_priority: ProcessPrioritySetting::BelowNormal,
            visible_window_priority: ProcessPrioritySetting::Normal,
            io_priority: IoPrioritySettings::default(),
            thread_priority: crate::config::ThreadPrioritySettings::default(),
            dynamic_priority_boost: DynamicPriorityBoostSettings::default(),
            gpu_priority: GpuPrioritySettings::default(),
            memory_priority_enabled: true,
            memory_priority_foreground_detection_enabled: true,
            memory_priority_visible_window_detection_enabled: true,
            memory_priority_preserve_foreground: true,
            memory_priority_preserve_visible_window: true,
            memory_priority_preserve_background: true,
            focus_process_memory_priority: ProcessMemoryPrioritySetting::Normal,
            visible_window_memory_priority: ProcessMemoryPrioritySetting::Medium,
            background_memory_priority: ProcessMemoryPrioritySetting::Low,
            cpu_pressure_restraint_enabled: true,
            limit_background_processors_enabled: true,
            dynamic_resource_zones_enabled: true,
            dynamic_resource_zone_settings: Default::default(),
            cpu_allocation_method: CpuAllocationMethod::ReduceAffinity,
            preserve_custom_affinity: true,
            processors_to_remove: crate::config::ADAPTIVE_AFFINITY_REMOVAL_MAX,
            background_processor_selection: BackgroundProcessorSelection::Custom,
            processor_limit_percent: 50,
            specific_processors: crate::cpu_topology::test_selection(0b101),
            pressure_source: AdaptivePressureSource::Both,
            exclude_service_hosts: true,
            foreground_or_system_cpu_threshold_percent: 70,
            background_app_cpu_threshold_percent: 25,
            cpu_recovery_threshold_percent: 5,
            reaction_time_ms: 750,
            restraint_trigger_time_ms: 2_700,
            pause_restraint_when_idle: true,
            restraint_idle_timeout_seconds: 120,
            restraint_priority: AdaptiveRestraintPriority::Idle,
            cpu_restraint_time_seconds: 4,
            maximum_restraint_time_seconds: crate::config::ADAPTIVE_RESTRAINT_MAX_TIME_SECONDS,
            cpu_recovery_time_seconds: 10,
            maximum_restrained_apps: 6,
            custom_rules: vec![ProcessExclusionRule {
                enabled: true,
                executable_path: "C:\\Games\\game.exe".to_owned(),
                ..Default::default()
            }],
        },
        process_priority: ProcessPrioritySettings {
            enabled: true,
            foreground_detection_enabled: true,
            foreground_priority: ProcessPrioritySetting::Default,
            visible_window_detection_enabled: true,
            visible_window_priority: ProcessPrioritySetting::Normal,
            background_priority: ProcessPrioritySetting::BelowNormal,
            preserve_foreground_priority: true,
            preserve_visible_window_priority: true,
            preserve_background_priority: true,
            exclusions: vec![ProcessExclusionRule {
                enabled: true,
                executable_path: "backup.exe".to_owned(),
                ..Default::default()
            }],
        },
        thread_priority: crate::config::ThreadPrioritySettings::default(),
        dynamic_priority_boost: DynamicPriorityBoostSettings {
            enabled: true,
            foreground_detection_enabled: true,
            foreground_boost: ProcessDynamicPriorityBoostSetting::Default,
            visible_window_detection_enabled: true,
            visible_window_boost: ProcessDynamicPriorityBoostSetting::Enabled,
            background_boost: ProcessDynamicPriorityBoostSetting::Disabled,
            exclusions: vec![ProcessExclusionRule {
                enabled: true,
                executable_path: "backup.exe".to_owned(),
                ..Default::default()
            }],
        },
        io_priority: IoPrioritySettings {
            enabled: true,
            foreground_detection_enabled: true,
            foreground_priority: ProcessIoPrioritySetting::Normal,
            visible_window_detection_enabled: true,
            visible_window_priority: ProcessIoPrioritySetting::Low,
            background_priority: ProcessIoPrioritySetting::VeryLow,
            preserve_foreground_priority: true,
            preserve_visible_window_priority: true,
            preserve_background_priority: true,
            exclusions: vec![ProcessExclusionRule {
                enabled: true,
                executable_path: "backup.exe".to_owned(),
                ..Default::default()
            }],
        },
        gpu_priority: GpuPrioritySettings {
            enabled: true,
            foreground_detection_enabled: true,
            foreground_priority: ProcessGpuPrioritySetting::AboveNormal,
            visible_window_detection_enabled: true,
            visible_window_priority: ProcessGpuPrioritySetting::Normal,
            background_priority: ProcessGpuPrioritySetting::BelowNormal,
            preserve_foreground_priority: true,
            preserve_visible_window_priority: true,
            preserve_background_priority: true,
            exclusions: vec![ProcessExclusionRule {
                enabled: true,
                executable_path: "render.exe".to_owned(),
                ..Default::default()
            }],
        },
        memory_priority: MemoryPrioritySettings {
            enabled: true,
            foreground_detection_enabled: true,
            foreground_priority: ProcessMemoryPrioritySetting::Default,
            visible_window_detection_enabled: true,
            visible_window_priority: ProcessMemoryPrioritySetting::Medium,
            background_priority: ProcessMemoryPrioritySetting::Low,
            preserve_foreground_priority: true,
            preserve_visible_window_priority: true,
            preserve_background_priority: true,
            exclusions: vec![ProcessExclusionRule {
                enabled: true,
                executable_path: "backup.exe".to_owned(),
                ..Default::default()
            }],
        },
        timer_resolution: TimerResolutionSettings {
            enabled: true,
            desired_100ns: 10_000,
            rules: vec![TimerResolutionRule {
                enabled: true,
                executable_path: "game.exe".to_owned(),
                desired_100ns: 20_000,
            }],
        },
        sleep_prevention: crate::config::SleepPreventionSettings {
            enabled: true,
            rules: vec![crate::config::SleepPreventionRule {
                enabled: true,
                executable_path: r"C:\Apps\encoder.exe".into(),
                keep_display_on: true,
            }],
        },
        resource_thresholds: crate::config::ResourceThresholdsSettings::default(),
        keep_running: crate::config::KeepRunningSettings {
            enabled: true,
            rules: vec![crate::config::KeepRunningRule {
                enabled: true,
                notify: true,
                executable_path: r"C:\Apps\keeper.exe".into(),
                arguments: r#"--document "C:\Documents\notes.txt""#.into(),
                working_directory: r"C:\Documents".into(),
            }],
        },
        instance_limits: crate::config::InstanceLimitsSettings {
            enabled: true,
            notify: false,
            rules: vec![crate::config::InstanceLimitsRule::new(
                r"C:\Apps\app.exe".into(),
            )],
        },
        instance_balancing: crate::config::InstanceBalancingSettings {
            enabled: true,
            rules: vec![crate::config::InstanceBalancingRule {
                enabled: true,
                executable_path: r"C:\Apps\app.exe".into(),
            }],
        },
        memory_trim: MemoryTrimSettings {
            enabled: true,
            system_memory_load_threshold_percent: 80,
            process_working_set_threshold_mb: 512,
            process_idle_seconds: 300,
            exclusions: vec![ProcessExclusionRule {
                enabled: true,
                executable_path: "keep.exe".to_owned(),
                ..Default::default()
            }],
        },
        on_battery: None,
    };
    settings
        .battery_profile_mut()
        .memory_trim
        .system_memory_load_threshold_percent = 72;
    settings.battery_profile_mut().sleep_prevention.enabled = false;
    let battery_restraint = &mut settings.battery_profile_mut().adaptive_engine_process;
    battery_restraint.pressure_source = AdaptivePressureSource::System;
    battery_restraint.exclude_service_hosts = false;
    battery_restraint.restraint_priority = AdaptiveRestraintPriority::BelowNormal;
    battery_restraint.maximum_restraint_time_seconds = battery_restraint.cpu_restraint_time_seconds;
    battery_restraint.cpu_allocation_method = CpuAllocationMethod::ProcessorAffinityHard;
    battery_restraint.preserve_custom_affinity = false;
    battery_restraint.processors_to_remove = 1;

    for mode in AppThemeMode::ALL {
        settings.general.theme_mode = mode;
        let raw = toml::to_string_pretty(&settings).expect("settings should serialize");
        let parsed: Settings = toml::from_str(&raw).expect("TOML should parse");

        assert_eq!(parsed, settings);
    }
}

#[test]
fn saved_adaptive_power_tuning_does_not_enable_the_independent_policy() {
    let mut settings = Settings::default();
    settings.adaptive_engine.enabled = true;
    settings
        .adaptive_engine
        .base_processor_policy
        .performance_max = 83;
    settings
        .adaptive_engine
        .background_pressure_profile
        .ac_policy = 61;
    settings
        .battery_profile_mut()
        .adaptive_engine
        .focus_and_launch_profile
        .battery_policy = 38;
    let mut file = toml::Value::try_from(&settings).unwrap();
    let old_adaptive = |source: &mut toml::Value| {
        let table = source["adaptive_engine"].as_table_mut().unwrap();
        table.remove("adaptive_processor_power_enabled");
        table.insert("processor_power_policy_enabled".into(), true.into());
    };
    old_adaptive(&mut file);
    old_adaptive(&mut file["on_battery"]);

    let loaded: Settings = file.try_into().unwrap();
    assert_eq!(loaded, settings);
    let saved = toml::to_string(&loaded).unwrap();
    assert!(!saved.contains("processor_power_policy_enabled"));
}

#[test]
fn priority_default_selections_round_trip() {
    let mut settings = Settings::default();
    settings.io_priority.enabled = true;
    settings.io_priority.background_priority = ProcessIoPrioritySetting::Default;
    settings.gpu_priority.enabled = true;
    settings.gpu_priority.foreground_priority = ProcessGpuPrioritySetting::Default;
    settings.memory_priority.enabled = true;
    settings.memory_priority.foreground_priority = ProcessMemoryPrioritySetting::Default;

    let raw = toml::to_string_pretty(&settings).expect("settings should serialize");
    assert!(raw.contains("background_priority = \"default\""));
    assert!(raw.contains("foreground_priority = \"default\""));

    let parsed: Settings = toml::from_str(&raw).expect("TOML should parse");

    assert_eq!(
        parsed.io_priority.background_priority,
        ProcessIoPrioritySetting::Default
    );
    assert_eq!(
        parsed.gpu_priority.foreground_priority,
        ProcessGpuPrioritySetting::Default
    );
    assert_eq!(
        parsed.memory_priority.foreground_priority,
        ProcessMemoryPrioritySetting::Default
    );
}

#[test]
fn retired_adaptive_engine_schema_is_rejected() {
    let raw =
        toml::to_string_pretty(&Settings::default()).expect("default settings should serialize");

    assert!(raw.contains("[adaptive_engine.base_processor_policy]"));
    assert!(!raw.contains("processor_policy_values"));

    let old_processor_policy = raw.replace(
        "adaptive_engine.base_processor_policy",
        "adaptive_engine.processor_power_policy_values",
    );
    assert!(toml::from_str::<Settings>(&old_processor_policy).is_err());

    let removed_adaptive_engine_process_schema = raw.replacen(
        "[adaptive_engine_process]",
        "[adaptive_engine_process]\nlower_background_io_priority_enabled = true\nlower_background_io_priority = \"very_low\"",
        1,
    );
    assert!(toml::from_str::<Settings>(&removed_adaptive_engine_process_schema).is_err());

    let removed_adaptive_engine_process_gate = raw.replacen(
        "[adaptive_engine_process]",
        "[adaptive_engine_process]\nenabled = true",
        1,
    );
    assert!(toml::from_str::<Settings>(&removed_adaptive_engine_process_gate).is_err());

    let old_restriction_mode = raw.replace(
        "cpu_allocation_method = \"cpu_sets_soft\"",
        concat!("cpu_allocation_method = \"soft", "_cpu_sets\""),
    );
    assert!(toml::from_str::<Settings>(&old_restriction_mode).is_err());

    let old_auto_priority = raw.replace(
        "focus_process_priority = \"above_normal\"",
        "focus_process_priority = \"auto\"",
    );
    assert!(toml::from_str::<Settings>(&old_auto_priority).is_err());

    let old_process_priority_toggle = raw.replace(
        "process_priority_enabled = true",
        "lower_background_apps = true",
    );
    assert!(toml::from_str::<Settings>(&old_process_priority_toggle).is_err());

    let old_pressure_toggle = raw.replace(
        "cpu_pressure_restraint_enabled = false",
        "adaptive_engine_process_enabled = false",
    );
    assert!(toml::from_str::<Settings>(&old_pressure_toggle).is_err());
}

#[test]
fn process_custom_priority_overrides_round_trip_with_their_semantics() {
    let rule = ProcessExclusionRule {
        enabled: true,
        executable_path: r"C:\Apps\worker.exe".to_owned(),
        process_foreground_priority: Some(ProcessPrioritySetting::AboveNormal),
        process_visible_window_priority: Some(ProcessPrioritySetting::Normal),
        process_background_priority: Some(ProcessPrioritySetting::BelowNormal),
        thread_foreground_priority: Some(ProcessThreadPrioritySetting::Highest),
        thread_visible_window_priority: Some(ProcessThreadPrioritySetting::AboveNormal),
        thread_background_priority: Some(ProcessThreadPrioritySetting::Lowest),
        dynamic_priority_boost_foreground: Some(ProcessDynamicPriorityBoostSetting::Enabled),
        dynamic_priority_boost_visible_window: Some(ProcessDynamicPriorityBoostSetting::Default),
        dynamic_priority_boost_background: Some(ProcessDynamicPriorityBoostSetting::Disabled),
        io_foreground_priority: Some(ProcessIoPrioritySetting::Normal),
        io_visible_window_priority: Some(ProcessIoPrioritySetting::Low),
        io_background_priority: Some(ProcessIoPrioritySetting::VeryLow),
        gpu_foreground_priority: Some(ProcessGpuPrioritySetting::AboveNormal),
        gpu_visible_window_priority: Some(ProcessGpuPrioritySetting::Normal),
        gpu_background_priority: Some(ProcessGpuPrioritySetting::Idle),
        memory_foreground_priority: Some(ProcessMemoryPrioritySetting::Normal),
        memory_visible_window_priority: Some(ProcessMemoryPrioritySetting::Medium),
        memory_background_priority: Some(ProcessMemoryPrioritySetting::VeryLow),
    };
    let settings = Settings {
        process_priority: ProcessPrioritySettings {
            exclusions: vec![rule.clone()],
            ..Default::default()
        },
        thread_priority: ThreadPrioritySettings {
            exclusions: vec![rule.clone()],
            ..Default::default()
        },
        dynamic_priority_boost: DynamicPriorityBoostSettings {
            exclusions: vec![rule.clone()],
            ..Default::default()
        },
        io_priority: IoPrioritySettings {
            exclusions: vec![rule.clone()],
            ..Default::default()
        },
        gpu_priority: GpuPrioritySettings {
            exclusions: vec![rule.clone()],
            ..Default::default()
        },
        memory_priority: MemoryPrioritySettings {
            exclusions: vec![rule],
            ..Default::default()
        },
        ..Settings::default()
    };

    let raw = toml::to_string_pretty(&settings).expect("settings should serialize");
    let parsed: Settings = toml::from_str(&raw).expect("TOML should parse");

    assert_eq!(parsed, settings);
    assert_eq!(
        parsed
            .process_priority
            .override_for(r"c:/apps/WORKER.exe", true, false),
        Some(Some(ProcessPrioritySetting::AboveNormal))
    );
    assert_eq!(
        parsed
            .thread_priority
            .override_for(r"c:/apps/WORKER.exe", false, true),
        Some(Some(ProcessThreadPrioritySetting::AboveNormal))
    );
    assert_eq!(
        parsed
            .thread_priority
            .override_for(r"c:/apps/WORKER.exe", false, false),
        Some(Some(ProcessThreadPrioritySetting::Lowest))
    );
    assert_eq!(
        parsed
            .dynamic_priority_boost
            .override_for(r"c:/apps/WORKER.exe", true, false),
        Some(Some(ProcessDynamicPriorityBoostSetting::Enabled))
    );
    assert_eq!(
        parsed
            .io_priority
            .override_for(r"c:/apps/WORKER.exe", false, false),
        Some(Some(ProcessIoPrioritySetting::VeryLow))
    );
    assert_eq!(
        parsed
            .gpu_priority
            .override_for(r"c:/apps/WORKER.exe", true, false),
        Some(Some(ProcessGpuPrioritySetting::AboveNormal))
    );
    assert_eq!(
        parsed
            .memory_priority
            .override_for(r"c:/apps/WORKER.exe", false, false),
        Some(Some(ProcessMemoryPrioritySetting::VeryLow))
    );
}

#[test]
fn toml_export_uses_toml_extension() {
    let filename = default_export_toml_filename();

    assert!(filename.starts_with(&format!("winderust_{}_", env!("CARGO_PKG_VERSION"))));
    assert!(filename.ends_with(".toml"));
}
