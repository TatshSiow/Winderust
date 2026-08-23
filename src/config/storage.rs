use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use atomic_write_file::AtomicWriteFile;
use chrono::Local;

use super::Settings;

const CONFIG_FILE: &str = "settings.toml";

pub fn config_path() -> PathBuf {
    config_dir().join(CONFIG_FILE)
}

pub fn default_export_toml_path() -> PathBuf {
    config_dir().join(default_export_toml_filename())
}

fn default_export_toml_filename() -> String {
    format!(
        "winderust_{}_{}.toml",
        env!("CARGO_PKG_VERSION"),
        Local::now().format("%Y-%m-%d")
    )
}

fn config_dir() -> PathBuf {
    std::env::current_exe()
        .ok()
        .and_then(|path| path.parent().map(Path::to_path_buf))
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn load() -> Result<Settings, String> {
    load_from_path(&config_path())
}

fn load_from_path(path: &Path) -> Result<Settings, String> {
    match fs::read_to_string(path) {
        Ok(raw) => parse_toml_settings(path, &raw),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Settings::default()),
        Err(error) => Err(format!("Failed to read {}: {error}", path.display())),
    }
}

pub fn save(settings: &Settings) -> Result<(), String> {
    let path = config_path();
    write_toml_settings(&path, settings)
        .map_err(|err| format!("Failed to save {}: {err}", path.display()))
}

pub fn export_toml_to(path: &Path, settings: &Settings) -> Result<(), String> {
    write_toml_settings(path, settings)
        .map_err(|err| format!("Failed to export {}: {err}", path.display()))
}

pub fn import_toml_from(path: &Path) -> Result<Settings, String> {
    read_toml_settings(path)
}

fn read_toml_settings(path: &Path) -> Result<Settings, String> {
    let raw = fs::read_to_string(path)
        .map_err(|err| format!("Failed to read {}: {err}", path.display()))?;
    parse_toml_settings(path, &raw)
}

fn parse_toml_settings(path: &Path, raw: &str) -> Result<Settings, String> {
    toml::from_str(raw).map_err(|err| format!("Failed to parse {}: {err}", path.display()))
}

fn write_toml_settings(path: &Path, settings: &Settings) -> io::Result<()> {
    let raw = toml::to_string_pretty(settings)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    write_bytes_atomically(path, raw.as_bytes())
}

pub fn write_bytes_atomically(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut file = AtomicWriteFile::open(path)?;
    file.write_all(bytes)?;
    file.commit()
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::config::{
        AccentSettings, ActionLogMode, AdaptiveEnginePreset, AdaptiveEngineSettings,
        AdvancedPowerPlanTuningPreset, AdvancedSettings, AnimationMode, AppLanguage,
        AppSuspensionRule, AppSuspensionSettings, AppThemeMode, BackgroundEfficiencyAggressiveness,
        BackgroundEfficiencyRule, BackgroundEfficiencySettings, BackgroundProcessorSelection,
        ByActivitySettings, ByCpuLoadRule, ByCpuLoadSettings, ByForegroundRule,
        ByForegroundSettings, ByRunningAppRule, ByRunningAppSettings, ByTimeRule, ByTimeSettings,
        CoreLimiterRule, CoreLimiterSettings, CpuAllocationMethod, CpuAllocationPreset,
        CpuAllocationRule, CpuAllocationSettings, CpuSchedulerSettings, CpuUsageComparison,
        DynamicPriorityBoostSettings, GeneralSettings, GpuPrioritySettings, InputDetectionSettings,
        IoPrioritySettings, MemoryPrioritySettings, MemoryTrimSettings, NetworkThresholdUnit,
        PowerPlanSettings, ProcessDynamicPriorityBoostSetting, ProcessExclusionRule,
        ProcessGpuPrioritySetting, ProcessIoPrioritySetting, ProcessMemoryPrioritySetting,
        ProcessPrioritySetting, ProcessPrioritySettings, ProcessRuleMode,
        ProcessThreadPrioritySetting, ThreadPrioritySettings, TimerResolutionRule,
        TimerResolutionSettings, WeekdaySetting,
    };

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
                animation_mode: AnimationMode::Off,
                navigation_collapsed: true,
                show_enabled_feature_counts_in_sidebar: false,
                show_feature_status_on_cards: false,
                pause_power_plan_switching_while_plugged_in: true,
                check_interval_ms: 2_500,
            },
            advanced: AdvancedSettings {
                action_log_mode: ActionLogMode::Error,
                execution_failure_suppression_threshold: 5,
                expose_all_priority_values: true,
                show_advanced_controls: true,
                pause_dashboard_metrics: true,
                pause_process_population: true,
            },
            adaptive_engine: AdaptiveEngineSettings {
                enabled: true,
                processor_power_policy_enabled: true,
                base_processor_policy:
                    crate::power::plan::ProcessorPowerValues::new_with_boost_mode(
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
                base_processor_policy:
                    crate::power::plan::ProcessorPowerValues::new_with_boost_mode(
                        25,
                        5,
                        80,
                        40,
                        crate::power::plan::ProcessorBoostMode::EfficientEnabled,
                    ),
                background_pressure_profile:
                    crate::power::AdaptivePowerBoostValues::BACKGROUND_PRESSURE,
                focus_and_launch_profile: crate::power::AdaptivePowerBoostValues::FOCUS_AND_LAUNCH,
                cpu_scheduler: CpuSchedulerSettings::default(),
            }],
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
            by_foreground: ByForegroundSettings {
                enabled: true,
                rules: vec![
                    ByForegroundRule {
                        enabled: true,
                        name: "Game plan".to_owned(),
                        executable_path: "game.exe".to_owned(),
                        power_plan_guid: Some("gaming-guid".to_owned()),
                    },
                    ByForegroundRule {
                        enabled: false,
                        name: "Backup plan".to_owned(),
                        executable_path: "backup\\tool.exe".to_owned(),
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
                    start_time: "09:00".to_owned(),
                    end_time: "17:30".to_owned(),
                    power_plan_guid: Some("work-hours-guid".to_owned()),
                }],
            },
            by_cpu_load: ByCpuLoadSettings {
                enabled: true,
                rules: vec![ByCpuLoadRule {
                    enabled: true,
                    name: "Low CPU".to_owned(),
                    comparison: CpuUsageComparison::AtOrBelow,
                    threshold_percent: 18,
                    upper_threshold_percent: None,
                    duration_seconds: 45,
                    power_plan_guid: Some("low-cpu-guid".to_owned()),
                    else_enabled: true,
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
                ),
            }],
            core_limiter: CoreLimiterSettings {
                enabled: true,
                protect_foreground_app: true,
                protect_visible_window_apps: false,
                rules: vec![CoreLimiterRule {
                    enabled: true,
                    executable_path: "encoder.exe".to_owned(),
                    focus_mode: ProcessRuleMode::Default,
                    visible_window_mode: ProcessRuleMode::Default,
                    background_mode: ProcessRuleMode::Default,
                    threshold_percent: 80,
                    sustain_seconds: 5,
                    cooldown_seconds: 15,
                    max_logical_processors: 2,
                }],
            },
            by_running_app: ByRunningAppSettings {
                enabled: true,
                rules: vec![ByRunningAppRule {
                    enabled: true,
                    name: "Game performance".to_owned(),
                    executable_path: "game.exe".to_owned(),
                    power_plan_guid: Some("gaming-guid".to_owned()),
                }],
            },
            cpu_scheduler: CpuSchedulerSettings {
                process_priority_enabled: true,
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
                focus_process_memory_priority: ProcessMemoryPrioritySetting::Normal,
                visible_window_memory_priority: ProcessMemoryPrioritySetting::Medium,
                background_memory_priority: ProcessMemoryPrioritySetting::Low,
                cpu_pressure_restraint_enabled: true,
                limit_background_processors_enabled: true,
                dynamic_resource_zones_enabled: true,
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

        let raw = toml::to_string_pretty(&settings).expect("settings should serialize");
        let parsed: Settings = toml::from_str(&raw).expect("TOML should parse");

        assert_eq!(parsed, settings);
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
    fn adaptive_base_processor_policy_serializes() {
        let raw = toml::to_string_pretty(&Settings::default())
            .expect("default settings should serialize");

        assert!(raw.contains("[adaptive_engine.base_processor_policy]"));
        assert!(!raw.contains("processor_policy_values"));
    }

    #[test]
    fn retired_adaptive_engine_schema_is_rejected() {
        let raw = toml::to_string_pretty(&Settings::default())
            .expect("default settings should serialize");

        let old_processor_policy = raw.replace(
            "adaptive_engine.base_processor_policy",
            "adaptive_engine.processor_power_policy_values",
        );
        assert!(toml::from_str::<Settings>(&old_processor_policy).is_err());

        let removed_cpu_scheduler_schema = raw.replacen(
            "[cpu_scheduler]",
            "[cpu_scheduler]\nlower_background_io_priority_enabled = true\nlower_background_io_priority = \"very_low\"",
            1,
        );
        assert!(toml::from_str::<Settings>(&removed_cpu_scheduler_schema).is_err());

        let removed_cpu_scheduler_gate =
            raw.replacen("[cpu_scheduler]", "[cpu_scheduler]\nenabled = true", 1);
        assert!(toml::from_str::<Settings>(&removed_cpu_scheduler_gate).is_err());

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
            "cpu_scheduler_enabled = false",
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
            dynamic_priority_boost_visible_window: Some(
                ProcessDynamicPriorityBoostSetting::Default,
            ),
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
}
