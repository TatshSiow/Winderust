use super::*;

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
fn watchdog_validation_rejects_invalid_memory_thresholds_during_decode() {
    let mut settings = Settings::default();
    let mut rule = crate::config::WatchdogRule::new(r"C:\Apps\test.exe".into());
    rule.memory.enabled = true;
    rule.memory.critical.threshold = 50;
    settings.watchdog.rules.push(rule);
    let raw = toml::to_string(&settings).unwrap();
    assert!(parse_toml_settings(Path::new("test.toml"), &raw).is_err());
    assert!(settings.validate_watchdog().is_err());
    settings.watchdog.rules[0].memory.critical.threshold = 90;
    settings.battery_profile_mut();
    settings.sync_shared_settings_to_battery();
    let raw = toml::to_string(&settings).unwrap();
    assert_eq!(
        parse_toml_settings(Path::new("test.toml"), &raw).unwrap(),
        settings
    );
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
    AdaptiveEngineSettings, AdvancedPowerPlanTuningPreset, AdvancedSettings, AppLanguage,
    AppSuspensionRule, AppSuspensionSettings, AppThemeMode, BackgroundEfficiencyAggressiveness,
    BackgroundEfficiencyRule, BackgroundEfficiencySettings, BackgroundProcessorSelection,
    ByActivitySettings, ByCpuLoadRule, ByCpuLoadSettings, ByFocusRule, ByFocusSettings,
    ByRunningAppRule, ByRunningAppSettings, ByTimeRule, ByTimeSettings, CpuAllocationMethod,
    CpuAllocationPreset, CpuAllocationRule, CpuAllocationSettings, CpuLimiterRule,
    CpuLimiterSettings, CpuUsageComparison, DynamicPriorityBoostSettings, GeneralSettings,
    GpuPrioritySettings, InputDetectionSettings, IoPrioritySettings, MemoryPrioritySettings,
    MemoryTrimSettings, NetworkThresholdUnit, PowerPlanSettings,
    ProcessDynamicPriorityBoostSetting, ProcessExclusionRule, ProcessGpuPrioritySetting,
    ProcessIoPrioritySetting, ProcessMemoryPrioritySetting, ProcessPrioritySetting,
    ProcessPrioritySettings, ProcessRuleMode, ProcessThreadPrioritySetting, ThreadPrioritySettings,
    TimerResolutionRule, TimerResolutionSettings, WeekdaySetting,
};

fn set_cpu_allocation_masks(settings: &mut Settings, mask: u64) {
    let rule = CpuAllocationRule {
        enabled: true,
        executable_path: r"C:\Apps\test.exe".into(),
        focus_core_mask: mask,
        visible_window_core_mask: mask,
        background_core_mask: mask,
    };
    settings.cpu_sets_soft.rules = vec![rule.clone()];
    settings.processor_affinity_hard.rules = vec![rule];
    settings.cpu_allocation_presets = vec![CpuAllocationPreset {
        name: "Selected processors".into(),
        core_mask: mask,
    }];
}

#[test]
fn full_width_cpu_allocation_masks_survive_save_export_load_and_import() {
    let path = std::env::temp_dir().join(format!(
        "winderust-cpu-allocation-masks-save-{}.toml",
        std::process::id()
    ));
    let export_path = path.with_extension("export.toml");
    let masks = [0, 1 << 63, u64::MAX];
    for (index, root_mask) in masks.into_iter().enumerate() {
        let battery_mask = masks[(index + 1) % masks.len()];
        let mut settings = Settings::default();
        set_cpu_allocation_masks(&mut settings, root_mask);
        set_cpu_allocation_masks(settings.battery_profile_mut(), battery_mask);

        // This is the same atomic serialization path used by Save, with an isolated path.
        write_toml_settings(&path, &settings).unwrap();
        let raw = fs::read_to_string(&path).unwrap();
        let value: toml::Value = toml::from_str(&raw).unwrap();
        for (profile, mask) in [(&value, root_mask), (&value["on_battery"], battery_mask)] {
            let encoded = format!("0x{mask:016x}");
            for section in ["cpu_sets_soft", "processor_affinity_hard"] {
                for field in [
                    "focus_core_mask",
                    "visible_window_core_mask",
                    "background_core_mask",
                ] {
                    assert_eq!(
                        profile[section]["rules"][0][field].as_str(),
                        Some(encoded.as_str())
                    );
                }
            }
            assert_eq!(
                profile["cpu_allocation_presets"][0]["core_mask"].as_str(),
                Some(encoded.as_str())
            );
        }
        assert_eq!(load_from_path(&path).unwrap(), settings);
        export_toml_to(&export_path, &settings).unwrap();
        assert_eq!(fs::read_to_string(&export_path).unwrap(), raw);
        assert_eq!(import_toml_from(&export_path).unwrap(), settings);
    }
    fs::remove_file(path).unwrap();
    fs::remove_file(export_path).unwrap();
}

#[test]
fn malformed_cpu_allocation_masks_fail_load_and_import_without_rewriting() {
    let mut settings = Settings::default();
    set_cpu_allocation_masks(&mut settings, u64::MAX);
    settings.battery_profile_mut();
    let path = std::env::temp_dir().join(format!(
        "winderust-cpu-allocation-masks-invalid-{}.toml",
        std::process::id()
    ));
    for battery in [false, true] {
        for (section, field) in [
            ("cpu_sets_soft", "focus_core_mask"),
            ("cpu_sets_soft", "visible_window_core_mask"),
            ("cpu_sets_soft", "background_core_mask"),
            ("processor_affinity_hard", "focus_core_mask"),
            ("processor_affinity_hard", "visible_window_core_mask"),
            ("processor_affinity_hard", "background_core_mask"),
            ("cpu_allocation_presets", "core_mask"),
        ] {
            for invalid in [
                toml::Value::Integer(0),
                toml::Value::String("0x800000000000000".into()),
                toml::Value::String("0x800000000000000g".into()),
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
                rules[0][field] = invalid;
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
    settings.adaptive_engine_process.specific_processors = vec![0];
    settings
        .adaptive_engine_process
        .dynamic_resource_zone_settings
        .foreground_share_percent = 80;
    settings
        .adaptive_engine_process
        .dynamic_resource_zone_settings
        .specific_processors = vec![1];
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
                assert_eq!(parsed.adaptive_engine_process.specific_processors, vec![0]);
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
    for (share, indices, enabled) in [
        (0, vec![], false),
        (100, vec![], false),
        (75, vec![64], false),
        (75, vec![], true),
    ] {
        let mut settings = Settings::default();
        let process = &mut settings.adaptive_engine_process;
        process.dynamic_resource_zones_enabled = enabled;
        process
            .dynamic_resource_zone_settings
            .foreground_share_percent = share;
        process
            .dynamic_resource_zone_settings
            .background_processor_selection = BackgroundProcessorSelection::Custom;
        process.dynamic_resource_zone_settings.specific_processors = indices;
        assert!(settings.validate_dynamic_resource_zones().is_err());
        assert!(toml::from_str::<Settings>(&toml::to_string(&settings).unwrap()).is_err());
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
fn background_efficiency_rule_without_tiers_remains_an_exclusion() {
    let rule: BackgroundEfficiencyRule = toml::from_str(
        r#"
            executable_path = "C:\\Apps\\legacy.exe"
        "#,
    )
    .expect("existing Background Efficiency rule should parse");

    assert!(rule.enabled);
    assert_eq!(rule.focus_efficiency_mode, ProcessRuleMode::Disabled);
    assert_eq!(
        rule.visible_window_efficiency_mode,
        ProcessRuleMode::Disabled
    );
    assert_eq!(rule.background_efficiency_mode, ProcessRuleMode::Disabled);
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
    };

    let serialized = toml::to_string_pretty(&rule).expect("CPU Limiter rule should serialize");
    let parsed: CpuLimiterRule =
        toml::from_str(&serialized).expect("CPU Limiter rule should parse");

    assert_eq!(parsed, rule);
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
            search_hotkeys: crate::config::SearchHotkeys::default(),
            check_interval_ms: 2_500,
        },
        advanced: AdvancedSettings {
            action_log_mode: ActionLogMode::Error,
            execution_failure_suppression_threshold: 5,
            expose_all_priority_values: true,
            show_advanced_controls: true,
        },
        adaptive_engine: AdaptiveEngineSettings {
            enabled: true,
            processor_power_policy_enabled: true,
            base_processor_policy: crate::power::plan::ProcessorPowerValues::new_with_boost_mode(
                0,
                5,
                45,
                0,
                crate::power::plan::ProcessorBoostMode::Disabled,
            ),
            background_pressure_profile:
                crate::power::AdaptivePowerBoostValues::BACKGROUND_PRESSURE,
            focus_and_launch_profile: crate::power::AdaptivePowerBoostValues::FOCUS_AND_LAUNCH,
        },
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
        by_activity: ByActivitySettings {
            enabled: true,
            idle_timeout_seconds: 12,
            switch_to_performance_on_resume: true,
            resume_action: crate::config::PowerPlanReturnAction::RestorePreviousPlan,
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
            restore_previous_plan: true,
            rules: vec![
                ByFocusRule {
                    enabled: true,
                    name: "Game plan".to_owned(),
                    executable_path: "game.exe".to_owned(),
                    power_plan_guid: Some("gaming-guid".to_owned()),
                },
                ByFocusRule {
                    enabled: false,
                    name: "Backup plan".to_owned(),
                    executable_path: "backup\\tool.exe".to_owned(),
                    power_plan_guid: Some("backup-guid".to_owned()),
                },
            ],
        },
        by_time: ByTimeSettings {
            enabled: true,
            restore_previous_plan: true,
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
                else_action: crate::config::PowerPlanReturnAction::RestorePreviousPlan,
                else_power_plan_guid: Some("normal-cpu-guid".to_owned()),
            }],
        },
        background_efficiency: BackgroundEfficiencySettings {
            enabled: true,
            foreground_detection_enabled: false,
            visible_window_detection_enabled: true,
            foreground_efficiency_mode: false,
            visible_window_efficiency_mode: false,
            background_efficiency_mode: true,
            aggressiveness: BackgroundEfficiencyAggressiveness::Safe,
            custom_rules: vec![
                BackgroundEfficiencyRule {
                    enabled: true,
                    executable_path: "mouse.exe".to_owned(),
                    focus_efficiency_mode: ProcessRuleMode::Disabled,
                    visible_window_efficiency_mode: ProcessRuleMode::Disabled,
                    background_efficiency_mode: ProcessRuleMode::Disabled,
                },
                BackgroundEfficiencyRule {
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
                    focus_core_mask: 0b1111,
                    visible_window_core_mask: 0b0111,
                    background_core_mask: 0b0011,
                },
                CpuAllocationRule {
                    enabled: false,
                    executable_path: "indexer.exe".to_owned(),
                    focus_core_mask: 0b1111,
                    visible_window_core_mask: 0b1100,
                    background_core_mask: 0b1000,
                },
            ],
        },
        processor_affinity_hard: CpuAllocationSettings {
            enabled: true,
            rules: vec![CpuAllocationRule {
                enabled: true,
                executable_path: "game.exe".to_owned(),
                focus_core_mask: 0b1111,
                visible_window_core_mask: 0b0111,
                background_core_mask: 0b0101,
            }],
        },
        cpu_allocation_presets: vec![CpuAllocationPreset {
            name: "Performance cores".to_owned(),
            core_mask: 0b0101,
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
            }],
        },
        by_running_app: ByRunningAppSettings {
            enabled: true,
            restore_previous_plan: true,
            rules: vec![ByRunningAppRule {
                enabled: true,
                name: "Game performance".to_owned(),
                executable_path: "game.exe".to_owned(),
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
            cpu_allocation_method: CpuAllocationMethod::CpuSetsSoft,
            background_processor_selection: BackgroundProcessorSelection::LeastUsed,
            processor_limit_percent: 50,
            specific_processors: vec![0, 2],
            foreground_or_system_cpu_threshold_percent: 70,
            background_app_cpu_threshold_percent: 25,
            cpu_recovery_threshold_percent: 5,
            reaction_time_ms: 750,
            cpu_restraint_time_seconds: 4,
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
        watchdog: crate::config::WatchdogSettings::default(),
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

    for mode in AppThemeMode::ALL {
        settings.general.theme_mode = mode;
        let raw = toml::to_string_pretty(&settings).expect("settings should serialize");
        let parsed: Settings = toml::from_str(&raw).expect("TOML should parse");

        assert_eq!(parsed, settings);
    }
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
