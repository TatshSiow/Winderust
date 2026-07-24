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
        AccentSettings, ActionLogMode, AdaptiveEngineSettings, AdvancedSettings, AnimationMode,
        AppLanguage, AppSuspensionRule, AppSuspensionSettings, AppThemeMode,
        BackgroundCpuRestrictionSettings, BackgroundEfficiencyAggressiveness,
        BackgroundEfficiencyRule, BackgroundEfficiencySettings, ByActivitySettings, ByCpuLoadRule,
        ByCpuLoadSettings, ByForegroundRule, ByForegroundSettings, ByRunningAppRule,
        ByRunningAppSettings, ByTimeRule, ByTimeSettings, CoreLimiterRule, CoreLimiterSettings,
        CoreSteeringMode, CoreSteeringRule, CoreSteeringSettings, CpuRestrictionMode,
        CpuUsageComparison, DynamicPriorityBoostSettings, ForegroundBoostPriority, GeneralSettings,
        GpuPrioritySettings, InputDetectionSettings, IoPrioritySettings, MemoryPrioritySettings,
        MemoryTrimSettings, NetworkThresholdUnit, PowerPlanSettings, PriorityRule,
        ProcessDynamicPriorityBoostSetting, ProcessExclusionRule, ProcessGpuPrioritySetting,
        ProcessIoPriority, ProcessIoPrioritySetting, ProcessMemoryPriority,
        ProcessMemoryPrioritySetting, ProcessPriority, ProcessPrioritySetting,
        ProcessPrioritySettings, TimerResolutionRule, TimerResolutionSettings, WeekdaySetting,
        WorkloadEngineSettings,
    };

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
        let settings = Settings {
            general: GeneralSettings {
                enabled: false,
                startup_with_windows: true,
                start_minimized: true,
                hide_to_tray: true,
                check_for_updates: true,
                update_channel: crate::config::UpdateChannel::PreRelease,
                theme_mode: AppThemeMode::Dark,
                accent: AccentSettings::default(),
                language: AppLanguage::ZhTw,
                animation_mode: AnimationMode::Off,
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
                processor_policy_enabled: true,
                processor_policy_values:
                    crate::power::plan::ProcessorPowerValues::new_with_boost_mode(
                        0,
                        5,
                        45,
                        0,
                        crate::power::plan::ProcessorBoostMode::Disabled,
                    ),
            },
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
                        process_name: "game.exe".to_owned(),
                        power_plan_guid: Some("gaming-guid".to_owned()),
                    },
                    ByForegroundRule {
                        enabled: false,
                        name: "Backup plan".to_owned(),
                        process_name: "backup\\tool.exe".to_owned(),
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
                exclude_foreground_app: false,
                aggressiveness: BackgroundEfficiencyAggressiveness::Safe,
                custom_rules: vec![
                    BackgroundEfficiencyRule {
                        enabled: true,
                        process_name: "mouse.exe".to_owned(),
                    },
                    BackgroundEfficiencyRule {
                        enabled: false,
                        process_name: "comma,app.exe".to_owned(),
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
                        process_name: "chat.exe".to_owned(),
                        network_wake_enabled: true,
                        audio_wake_enabled: true,
                        network_download_threshold_bytes: 1,
                        network_download_threshold_unit: NetworkThresholdUnit::Bytes,
                        network_upload_threshold_bytes: 0,
                        network_upload_threshold_unit: NetworkThresholdUnit::Bytes,
                    },
                    AppSuspensionRule {
                        enabled: false,
                        process_name: "comma,app.exe".to_owned(),
                        network_wake_enabled: false,
                        audio_wake_enabled: false,
                        network_download_threshold_bytes: 1,
                        network_download_threshold_unit: NetworkThresholdUnit::Bytes,
                        network_upload_threshold_bytes: 0,
                        network_upload_threshold_unit: NetworkThresholdUnit::Bytes,
                    },
                ],
            },
            core_steering: CoreSteeringSettings {
                enabled: true,
                exclude_foreground_app: true,
                rules: vec![
                    CoreSteeringRule {
                        enabled: true,
                        mode: CoreSteeringMode::Hard,
                        process_name: "backup.exe".to_owned(),
                        core_mask: 0b0011,
                    },
                    CoreSteeringRule {
                        enabled: false,
                        mode: CoreSteeringMode::Soft,
                        process_name: "indexer.exe".to_owned(),
                        core_mask: 0b1100,
                    },
                    CoreSteeringRule {
                        enabled: true,
                        mode: CoreSteeringMode::EfficiencyOff,
                        process_name: "game.exe".to_owned(),
                        core_mask: 0,
                    },
                ],
            },
            background_cpu_restriction: BackgroundCpuRestrictionSettings::default(),
            core_limiter: CoreLimiterSettings {
                enabled: true,
                exclude_foreground_app: true,
                rules: vec![CoreLimiterRule {
                    enabled: true,
                    process_name: "encoder.exe".to_owned(),
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
                    process_name: "game.exe".to_owned(),
                    power_plan_guid: Some("gaming-guid".to_owned()),
                }],
            },
            workload_engine: WorkloadEngineSettings {
                enabled: true,
                lower_background_apps: true,
                workload_engine_background_efficiency_enabled: true,
                workload_engine_background_priority: ProcessPriority::BelowNormal,
                lower_background_io_priority_enabled: true,
                lower_background_io_priority: ProcessIoPriority::VeryLow,
                workload_engine_io_priority: IoPrioritySettings::default(),
                workload_engine_thread_priority: crate::config::ThreadPrioritySettings::default(),
                workload_engine_dynamic_priority_boost: DynamicPriorityBoostSettings::default(),
                workload_engine_gpu_priority: GpuPrioritySettings::default(),
                workload_engine_memory_priority_enabled: true,
                workload_engine_foreground_memory_priority: ProcessMemoryPrioritySetting::Normal,
                workload_engine_memory_priority: ProcessMemoryPriority::Low,
                lower_background_auto_cpu_percent: true,
                workload_engine_enabled: true,
                workload_engine_advanced_settings_enabled: true,
                workload_engine_affinity_escalation_enabled: true,
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
                workload_engine_exclusions: vec![ProcessExclusionRule {
                    enabled: true,
                    process_name: "game*.exe".to_owned(),
                    ..Default::default()
                }],
                boost_foreground_app: true,
                foreground_boost: ForegroundBoostPriority::AboveNormal,
                foreground_stability_delay_ms: 750,
                rules: vec![PriorityRule {
                    enabled: true,
                    process_name: "backup.exe".to_owned(),
                    priority: ProcessPriority::BelowNormal,
                }],
            },
            process_priority: ProcessPrioritySettings {
                enabled: true,
                foreground_detection_enabled: true,
                foreground_priority: ProcessPrioritySetting::Default,
                background_priority: ProcessPrioritySetting::BelowNormal,
                preserve_foreground_priority: true,
                preserve_background_priority: true,
                exclusions: vec![ProcessExclusionRule {
                    enabled: true,
                    process_name: "backup.exe".to_owned(),
                    ..Default::default()
                }],
            },
            thread_priority: crate::config::ThreadPrioritySettings::default(),
            dynamic_priority_boost: DynamicPriorityBoostSettings {
                enabled: true,
                foreground_detection_enabled: true,
                foreground_boost: ProcessDynamicPriorityBoostSetting::Default,
                background_boost: ProcessDynamicPriorityBoostSetting::Disabled,
                exclusions: vec![ProcessExclusionRule {
                    enabled: true,
                    process_name: "backup.exe".to_owned(),
                    ..Default::default()
                }],
            },
            io_priority: IoPrioritySettings {
                enabled: true,
                foreground_detection_enabled: true,
                foreground_priority: ProcessIoPrioritySetting::Normal,
                background_priority: ProcessIoPrioritySetting::VeryLow,
                preserve_foreground_priority: true,
                preserve_background_priority: true,
                exclusions: vec![ProcessExclusionRule {
                    enabled: true,
                    process_name: "backup.exe".to_owned(),
                    ..Default::default()
                }],
            },
            gpu_priority: GpuPrioritySettings {
                enabled: true,
                foreground_detection_enabled: true,
                foreground_priority: ProcessGpuPrioritySetting::AboveNormal,
                background_priority: ProcessGpuPrioritySetting::BelowNormal,
                preserve_foreground_priority: true,
                preserve_background_priority: true,
                exclusions: vec![ProcessExclusionRule {
                    enabled: true,
                    process_name: "render.exe".to_owned(),
                    ..Default::default()
                }],
            },
            memory_priority: MemoryPrioritySettings {
                enabled: true,
                foreground_detection_enabled: true,
                foreground_priority: ProcessMemoryPrioritySetting::Default,
                background_priority: ProcessMemoryPrioritySetting::Low,
                preserve_foreground_priority: true,
                preserve_background_priority: true,
                exclusions: vec![ProcessExclusionRule {
                    enabled: true,
                    process_name: "backup.exe".to_owned(),
                    ..Default::default()
                }],
            },
            timer_resolution: TimerResolutionSettings {
                enabled: true,
                desired_100ns: 10_000,
                rules: vec![TimerResolutionRule {
                    enabled: true,
                    process_name: "game.exe".to_owned(),
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
                    process_name: "keep.exe".to_owned(),
                    ..Default::default()
                }],
            },
        };

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
    fn toml_export_uses_toml_extension() {
        let filename = default_export_toml_filename();

        assert!(filename.starts_with(&format!("winderust_{}_", env!("CARGO_PKG_VERSION"))));
        assert!(filename.ends_with(".toml"));
    }
}
