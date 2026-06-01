use std::{
    fs, io,
    path::{Path, PathBuf},
};

use chrono::Local;

use super::Settings;

const CONFIG_FILE: &str = "settings.toml";
const CONFIG_DIR: &str = "PowerLeaf";
const LEGACY_CONFIG_DIR: &str = "PowerSwitcher";

pub fn config_path() -> PathBuf {
    config_dir().join(CONFIG_FILE)
}

pub fn default_export_toml_path() -> PathBuf {
    config_dir().join(default_export_toml_filename())
}

fn default_export_toml_filename() -> String {
    format!(
        "powerleaf_{}_{}.toml",
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

fn legacy_config_dir() -> PathBuf {
    base_config_dir().join(LEGACY_CONFIG_DIR)
}

fn base_config_dir() -> PathBuf {
    dirs::config_dir()
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
    let path = config_path();
    if path.exists() {
        return path;
    }

    let legacy_path = legacy_config_dir().join(CONFIG_FILE);
    if legacy_path.exists() {
        legacy_path
    } else {
        path
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
    fs::write(path, raw)
}

fn settings_to_toml(settings: &Settings) -> Result<String, toml::ser::Error> {
    toml::to_string_pretty(settings)
}

fn toml_to_settings(raw: &str) -> Result<Settings, toml::de::Error> {
    toml::from_str(raw)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        ActivityModeSettings, AppSuspensionSettings, CpuUsageComparison, CpuUsageModeSettings,
        CpuUsageRule, CpuUsageTarget, EcoQosSettings, ForegroundRule, ForegroundRules,
        GeneralSettings, InputDetectionSettings, ManualOverride, PowerPlanSettings,
        ScheduleModeSettings, ScheduleRule, WeekdaySetting,
    };

    #[test]
    fn toml_round_trip_preserves_settings() {
        let settings = Settings {
            general: GeneralSettings {
                enabled: false,
                startup_with_windows: true,
                start_minimized: true,
                hide_to_tray: true,
                pause_power_plan_switching_while_plugged_in: true,
                check_interval_ms: 2_500,
                manual_override: ManualOverride::UntilEpochSeconds(42),
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
                        name: "Game plan".to_owned(),
                        process_name: "game.exe".to_owned(),
                        power_plan_guid: Some("gaming-guid".to_owned()),
                    },
                    ForegroundRule {
                        name: "Backup plan".to_owned(),
                        process_name: "backup\\tool.exe".to_owned(),
                        power_plan_guid: Some("backup-guid".to_owned()),
                    },
                ],
                whitelist: vec!["game.exe".to_owned(), "comma,app.exe".to_owned()],
                force_power_save: vec!["backup\\tool.exe".to_owned()],
                power_plans: PowerPlanSettings {
                    power_save_guid: Some("foreground-idle-guid".to_owned()),
                    performance_guid: Some("foreground-active-guid".to_owned()),
                },
            },
            schedule_mode: ScheduleModeSettings {
                enabled: true,
                power_plans: PowerPlanSettings {
                    power_save_guid: Some("schedule-idle-guid".to_owned()),
                    performance_guid: Some("schedule-active-guid".to_owned()),
                },
                rules: vec![ScheduleRule {
                    name: "Work hours".to_owned(),
                    days: vec![WeekdaySetting::Mon, WeekdaySetting::Fri],
                    start_time: "09:00".to_owned(),
                    end_time: "17:30".to_owned(),
                    power_save_guid: Some("idle-guid".to_owned()),
                    performance_guid: Some("active-guid".to_owned()),
                }],
            },
            cpu_usage_mode: CpuUsageModeSettings {
                enabled: true,
                power_plans: PowerPlanSettings {
                    power_save_guid: Some("cpu-idle-guid".to_owned()),
                    performance_guid: Some("cpu-active-guid".to_owned()),
                },
                rules: vec![CpuUsageRule {
                    name: "Low CPU".to_owned(),
                    comparison: CpuUsageComparison::AtOrBelow,
                    threshold_percent: 18,
                    duration_seconds: 45,
                    target: CpuUsageTarget::Idle,
                }],
            },
            eco_qos: EcoQosSettings {
                enabled: true,
                exclude_foreground_app: false,
                exclude_suspended_processes: true,
                efficiency_whitelist: vec!["mouse.exe".to_owned(), "comma,app.exe".to_owned()],
            },
            app_suspension: AppSuspensionSettings {
                enabled: true,
                background_delay_seconds: 120,
                suspendable_apps: vec!["chat.exe".to_owned(), "comma,app.exe".to_owned()],
            },
        };

        let raw = settings_to_toml(&settings).expect("settings should serialize");
        let parsed = toml_to_settings(&raw).expect("TOML should parse");

        assert_eq!(parsed, settings);
    }

    #[test]
    fn toml_export_uses_toml_extension() {
        let filename = default_export_toml_filename();

        assert!(filename.starts_with(&format!("powerleaf_{}_", env!("CARGO_PKG_VERSION"))));
        assert!(filename.ends_with(".toml"));
    }
}
