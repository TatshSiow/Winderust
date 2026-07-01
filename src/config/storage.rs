use std::{
    ffi::OsString,
    fs, io,
    os::windows::ffi::OsStrExt,
    path::{Path, PathBuf},
};

use chrono::Local;
use windows_sys::Win32::Storage::FileSystem::{
    MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
};

use super::Settings;

const CONFIG_FILE: &str = "settings.toml";
const CONFIG_DIR: &str = "Winderust";

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
        export_date()
    )
}

fn export_date() -> String {
    Local::now().format("%Y-%m-%d").to_string()
}

fn config_dir() -> PathBuf {
    base_config_dir().join(CONFIG_DIR)
}

fn base_config_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn load() -> Result<Settings, String> {
    let path = load_config_path();
    if !path.exists() {
        return Ok(Settings::default());
    }

    read_toml_settings(&path)
}

fn load_config_path() -> PathBuf {
    config_path()
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
    let mut settings = toml_to_settings(&raw)
        .map_err(|err| format!("Failed to parse {}: {err}", path.display()))?;
    settings.fill_missing_power_plan_mappings();
    Ok(settings)
}

fn write_toml_settings(path: &Path, settings: &Settings) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let raw = settings_to_toml(settings)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    let temp_path = temp_settings_path(path);
    fs::write(&temp_path, raw)?;
    replace_file(&temp_path, path).inspect_err(|_| {
        let _ = fs::remove_file(&temp_path);
    })
}

fn settings_to_toml(settings: &Settings) -> Result<String, toml::ser::Error> {
    toml::to_string_pretty(settings)
}

fn toml_to_settings(raw: &str) -> Result<Settings, toml::de::Error> {
    toml::from_str(raw)
}

fn temp_settings_path(path: &Path) -> PathBuf {
    let mut value: OsString = path.as_os_str().to_os_string();
    value.push(".tmp");
    PathBuf::from(value)
}

fn replace_file(from: &Path, to: &Path) -> io::Result<()> {
    let from = wide_path(from);
    let to = wide_path(to);
    let ok = unsafe {
        MoveFileExW(
            from.as_ptr(),
            to.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

fn wide_path(path: &Path) -> Vec<u16> {
    path.as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        AccentSettings, ActionLogMode, ActivityModeSettings, AdvancedSettings, AnimationMode,
        AppLanguage, AppSuspensionRule, AppSuspensionSettings, AppThemeMode,
        BackgroundCpuRestrictionSettings, CpuAffinityMode, CpuAffinityRule, CpuAffinitySettings,
        CpuLimiterRule, CpuLimiterSettings, CpuPrioritySettings, CpuUsageComparison,
        CpuUsageModeSettings, CpuUsageRule, EcoQosAggressiveness, EcoQosCpuRestrictionControlStyle,
        EcoQosCpuRestrictionMode, EcoQosCpuRestrictionStrategy, EcoQosExclusionRule,
        EcoQosSettings, ForegroundBoostPriority, ForegroundResponsivenessSettings, ForegroundRule,
        ForegroundRules, GeneralSettings, GpuPrioritySettings, InputDetectionSettings,
        IoPrioritySettings, MemoryPrioritySettings, NetworkThresholdUnit, PerformanceModeRule,
        PerformanceModeSettings, PowerPlanSettings, PriorityBoostSettings, PriorityRule,
        ProcessCpuPrioritySetting, ProcessExclusionRule, ProcessGpuPrioritySetting,
        ProcessIoPriority, ProcessIoPrioritySetting, ProcessMemoryPriority,
        ProcessMemoryPrioritySetting, ProcessPriority, ProcessPriorityBoostSetting,
        ScheduleModeSettings, ScheduleRule, SmartTrimSettings, TimerResolutionRule,
        TimerResolutionSettings, WeekdaySetting,
    };

    #[test]
    fn toml_round_trip_preserves_settings() {
        let settings = Settings {
            general: GeneralSettings {
                enabled: false,
                startup_with_windows: true,
                start_minimized: true,
                hide_to_tray: true,
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
            },
            power_plans: PowerPlanSettings {
                power_save_guid: Some("idle-guid".to_owned()),
                performance_guid: Some("active-guid".to_owned()),
            },
            activity_mode: ActivityModeSettings {
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
            foreground_rules: ForegroundRules {
                enabled: true,
                rules: vec![
                    ForegroundRule {
                        enabled: true,
                        name: "Game plan".to_owned(),
                        process_name: "game.exe".to_owned(),
                        power_plan_guid: Some("gaming-guid".to_owned()),
                    },
                    ForegroundRule {
                        enabled: false,
                        name: "Backup plan".to_owned(),
                        process_name: "backup\\tool.exe".to_owned(),
                        power_plan_guid: Some("backup-guid".to_owned()),
                    },
                ],
                power_plans: PowerPlanSettings {
                    power_save_guid: Some("foreground-idle-guid".to_owned()),
                    performance_guid: Some("foreground-active-guid".to_owned()),
                },
            },
            schedule_mode: ScheduleModeSettings {
                enabled: true,
                power_plans: PowerPlanSettings::default(),
                rules: vec![ScheduleRule {
                    enabled: true,
                    name: "Work hours".to_owned(),
                    days: vec![WeekdaySetting::Mon, WeekdaySetting::Fri],
                    start_time: "09:00".to_owned(),
                    end_time: "17:30".to_owned(),
                    power_plan_guid: Some("work-hours-guid".to_owned()),
                }],
            },
            cpu_usage_mode: CpuUsageModeSettings {
                enabled: true,
                power_plans: PowerPlanSettings::default(),
                rules: vec![CpuUsageRule {
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
            eco_qos: EcoQosSettings {
                enabled: true,
                exclude_foreground_app: false,
                cpu_restriction_mode: EcoQosCpuRestrictionMode::SoftCpuSets,
                cpu_restriction_strategy: EcoQosCpuRestrictionStrategy::Auto,
                cpu_restriction_control_style: EcoQosCpuRestrictionControlStyle::Percentage,
                cpu_restriction_percent: 50,
                cpu_restriction_max_logical_processors: 0,
                cpu_restriction_core_mask: 0,
                aggressiveness: EcoQosAggressiveness::Safe,
                efficiency_whitelist: vec![
                    EcoQosExclusionRule {
                        enabled: true,
                        process_name: "mouse.exe".to_owned(),
                    },
                    EcoQosExclusionRule {
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
            cpu_affinity: CpuAffinitySettings {
                enabled: true,
                exclude_foreground_app: true,
                rules: vec![
                    CpuAffinityRule {
                        enabled: true,
                        mode: CpuAffinityMode::Hard,
                        process_name: "backup.exe".to_owned(),
                        core_mask: 0b0011,
                    },
                    CpuAffinityRule {
                        enabled: false,
                        mode: CpuAffinityMode::Soft,
                        process_name: "indexer.exe".to_owned(),
                        core_mask: 0b1100,
                    },
                    CpuAffinityRule {
                        enabled: true,
                        mode: CpuAffinityMode::EfficiencyOff,
                        process_name: "game.exe".to_owned(),
                        core_mask: 0,
                    },
                ],
            },
            background_cpu_restriction: BackgroundCpuRestrictionSettings::default(),
            cpu_limiter: CpuLimiterSettings {
                enabled: true,
                exclude_foreground_app: true,
                rules: vec![CpuLimiterRule {
                    enabled: true,
                    process_name: "encoder.exe".to_owned(),
                    threshold_percent: 80,
                    sustain_seconds: 5,
                    cooldown_seconds: 15,
                    max_logical_processors: 2,
                }],
            },
            performance_mode: PerformanceModeSettings {
                enabled: true,
                rules: vec![PerformanceModeRule {
                    enabled: true,
                    name: "Game performance".to_owned(),
                    process_name: "game.exe".to_owned(),
                    power_plan_guid: Some("gaming-guid".to_owned()),
                }],
            },
            foreground_responsiveness: ForegroundResponsivenessSettings {
                enabled: true,
                lower_background_apps: true,
                lower_background_affinity_enabled: true,
                lower_background_io_priority_enabled: true,
                lower_background_io_priority: ProcessIoPriority::VeryLow,
                auto_balance_memory_priority_enabled: true,
                auto_balance_memory_priority: ProcessMemoryPriority::Low,
                lower_background_affinity_mode: EcoQosCpuRestrictionMode::SoftCpuSets,
                lower_background_cpu_percent: 50,
                lower_background_max_logical_processors: 0,
                lower_background_auto_cpu_percent: true,
                auto_balance_enabled: true,
                auto_balance_advanced_settings_enabled: true,
                auto_balance_affinity_escalation_enabled: true,
                auto_balance_affinity_mode: EcoQosCpuRestrictionMode::SoftCpuSets,
                auto_balance_cpu_percent: 50,
                auto_balance_max_logical_processors: 0,
                auto_balance_total_threshold_percent: 70,
                auto_balance_threshold_percent: 25,
                auto_balance_restore_threshold_percent: 5,
                auto_balance_sustain_seconds: 2,
                auto_balance_minimum_restraint_seconds: 4,
                auto_balance_cooldown_seconds: 10,
                auto_balance_exclusions: vec![ProcessExclusionRule {
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
            cpu_priority: CpuPrioritySettings {
                enabled: true,
                foreground_detection_enabled: true,
                foreground_priority: ProcessCpuPrioritySetting::Default,
                background_priority: ProcessCpuPrioritySetting::BelowNormal,
                preserve_foreground_priority: true,
                preserve_background_priority: true,
                exclusions: vec![ProcessExclusionRule {
                    enabled: true,
                    process_name: "backup.exe".to_owned(),
                    ..Default::default()
                }],
            },
            thread_priority: crate::config::ThreadPrioritySettings::default(),
            priority_boost: PriorityBoostSettings {
                enabled: true,
                foreground_detection_enabled: true,
                foreground_boost: ProcessPriorityBoostSetting::Default,
                background_boost: ProcessPriorityBoostSetting::Disabled,
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
                    desired_100ns: 5_000,
                }],
            },
            smart_trim: SmartTrimSettings {
                enabled: true,
                check_interval_minutes: 15,
                exclude_foreground_app: true,
                trim_working_sets: true,
                system_memory_load_threshold_percent: 85,
                process_working_set_threshold_mb: 768,
                process_cpu_idle_threshold_percent: 2,
                process_idle_seconds: 120,
                trim_cooldown_seconds: 600,
                purge_standby_list: true,
                purge_system_file_cache: true,
                purge_only_in_performance_mode: true,
                purge_free_ram_threshold_mb: 1024,
                exclusions: vec![ProcessExclusionRule {
                    enabled: true,
                    process_name: "game*.exe".to_owned(),
                    ..Default::default()
                }],
            },
        };

        let raw = settings_to_toml(&settings).expect("settings should serialize");
        let parsed = toml_to_settings(&raw).expect("TOML should parse");

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

        let raw = settings_to_toml(&settings).expect("settings should serialize");
        assert!(raw.contains("background_priority = \"default\""));
        assert!(raw.contains("foreground_priority = \"default\""));

        let parsed = toml_to_settings(&raw).expect("TOML should parse");

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

    #[test]
    fn temp_settings_path_keeps_original_path_intact() {
        assert_eq!(
            temp_settings_path(Path::new(r"C:\Users\me\settings.toml")),
            PathBuf::from(r"C:\Users\me\settings.toml.tmp")
        );
    }
}
