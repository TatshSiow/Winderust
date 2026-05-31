use std::{
    fs, io,
    path::{Path, PathBuf},
};

use std::collections::BTreeMap;

use super::{
    CpuUsageComparison, CpuUsageRule, CpuUsageTarget, ManualOverride, Settings, WeekdaySetting,
};

const CONFIG_FILE: &str = "settings.toml";
const INI_FILE: &str = "settings.ini";
const CONFIG_DIR: &str = "PowerLeaf";
const LEGACY_CONFIG_DIR: &str = "PowerSwitcher";

pub fn config_path() -> PathBuf {
    config_dir().join(CONFIG_FILE)
}

pub fn ini_path() -> PathBuf {
    config_dir().join(INI_FILE)
}

fn config_dir() -> PathBuf {
    base_config_dir().join(CONFIG_DIR)
}

fn legacy_config_dir() -> PathBuf {
    base_config_dir().join(LEGACY_CONFIG_DIR)
}

fn base_config_dir() -> PathBuf {
    let base = dirs::config_dir()
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));

    base
}

pub fn load() -> Result<Settings, String> {
    let path = load_config_path();
    if !path.exists() {
        return Ok(Settings::default());
    }

    let raw = fs::read_to_string(&path)
        .map_err(|err| format!("Failed to read {}: {err}", path.display()))?;
    toml::from_str(&raw).map_err(|err| format!("Failed to parse {}: {err}", path.display()))
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
    write_settings(&path, settings)
        .map_err(|err| format!("Failed to save {}: {err}", path.display()))
}

pub fn export_ini_to(path: &Path, settings: &Settings) -> Result<(), String> {
    write_ini_settings(path, settings)
        .map_err(|err| format!("Failed to export {}: {err}", path.display()))
}

pub fn import_ini_from(path: &Path) -> Result<Settings, String> {
    let raw = fs::read_to_string(&path)
        .map_err(|err| format!("Failed to read {}: {err}", path.display()))?;
    settings_from_ini(&raw).map_err(|err| format!("Failed to parse {}: {err}", path.display()))
}

fn write_settings(path: &Path, settings: &Settings) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let raw = toml::to_string_pretty(settings)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    fs::write(path, raw)
}

fn write_ini_settings(path: &Path, settings: &Settings) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(path, settings_to_ini(settings))
}

fn settings_to_ini(settings: &Settings) -> String {
    let mut raw = String::new();

    raw.push_str("[general]\n");
    raw.push_str(&ini_entry("enabled", settings.general.enabled));
    raw.push_str(&ini_entry(
        "startup_with_windows",
        settings.general.startup_with_windows,
    ));
    raw.push_str(&ini_entry("hide_to_tray", settings.general.hide_to_tray));
    raw.push_str(&ini_entry(
        "check_interval_ms",
        settings.general.check_interval_ms,
    ));
    raw.push_str(&ini_entry(
        "manual_override",
        manual_override_to_ini(&settings.general.manual_override),
    ));

    raw.push_str("\n[power_plans]\n");
    raw.push_str(&ini_entry(
        "power_save_guid",
        settings
            .power_plans
            .power_save_guid
            .as_deref()
            .unwrap_or(""),
    ));
    raw.push_str(&ini_entry(
        "performance_guid",
        settings
            .power_plans
            .performance_guid
            .as_deref()
            .unwrap_or(""),
    ));

    raw.push_str("\n[activity_mode]\n");
    raw.push_str(&ini_entry("enabled", settings.activity_mode.enabled));
    raw.push_str(&ini_entry(
        "idle_timeout_seconds",
        settings.activity_mode.idle_timeout_seconds,
    ));
    raw.push_str(&ini_entry(
        "switch_to_performance_on_resume",
        settings.activity_mode.switch_to_performance_on_resume,
    ));
    raw.push_str(&ini_entry(
        "input_keyboard",
        settings.activity_mode.input_detection.keyboard,
    ));
    raw.push_str(&ini_entry(
        "input_mouse",
        settings.activity_mode.input_detection.mouse,
    ));

    raw.push_str("\n[foreground_rules]\n");
    raw.push_str(&ini_entry("enabled", settings.foreground_rules.enabled));
    raw.push_str(&ini_entry_raw(
        "force_active",
        join_escaped(&settings.foreground_rules.whitelist),
    ));
    raw.push_str(&ini_entry_raw(
        "force_idle",
        join_escaped(&settings.foreground_rules.force_power_save),
    ));

    raw.push_str("\n[schedule_mode]\n");
    raw.push_str(&ini_entry("enabled", settings.schedule_mode.enabled));
    raw.push_str(&ini_entry("rule_count", settings.schedule_mode.rules.len()));

    for (index, rule) in settings.schedule_mode.rules.iter().enumerate() {
        raw.push_str(&format!("\n[schedule_rule.{index}]\n"));
        raw.push_str(&ini_entry("name", &rule.name));
        raw.push_str(&ini_entry_raw(
            "days",
            join_escaped(
                &rule
                    .days
                    .iter()
                    .map(|day| day.short_label().to_ascii_lowercase())
                    .collect::<Vec<_>>(),
            ),
        ));
        raw.push_str(&ini_entry("start_time", &rule.start_time));
        raw.push_str(&ini_entry("end_time", &rule.end_time));
        raw.push_str(&ini_entry(
            "power_save_guid",
            rule.power_save_guid.as_deref().unwrap_or(""),
        ));
        raw.push_str(&ini_entry(
            "performance_guid",
            rule.performance_guid.as_deref().unwrap_or(""),
        ));
    }

    raw.push_str("\n[cpu_usage_mode]\n");
    raw.push_str(&ini_entry("enabled", settings.cpu_usage_mode.enabled));
    raw.push_str(&ini_entry(
        "rule_count",
        settings.cpu_usage_mode.rules.len(),
    ));

    for (index, rule) in settings.cpu_usage_mode.rules.iter().enumerate() {
        raw.push_str(&format!("\n[cpu_usage_rule.{index}]\n"));
        raw.push_str(&ini_entry("name", &rule.name));
        raw.push_str(&ini_entry(
            "comparison",
            cpu_usage_comparison_to_ini(rule.comparison),
        ));
        raw.push_str(&ini_entry(
            "threshold_percent",
            rule.threshold_percent.min(100),
        ));
        raw.push_str(&ini_entry("duration_seconds", rule.duration_seconds));
        raw.push_str(&ini_entry("target", cpu_usage_target_to_ini(rule.target)));
    }

    raw
}

fn settings_from_ini(raw: &str) -> Result<Settings, String> {
    let ini = parse_ini(raw)?;
    let mut settings = Settings::default();

    if let Some(section) = ini.get("general") {
        read_bool(section, "enabled", &mut settings.general.enabled)?;
        read_bool(
            section,
            "startup_with_windows",
            &mut settings.general.startup_with_windows,
        )?;
        read_bool(section, "hide_to_tray", &mut settings.general.hide_to_tray)?;
        read_u64(
            section,
            "check_interval_ms",
            &mut settings.general.check_interval_ms,
        )?;
        if let Some(value) = section.get("manual_override") {
            settings.general.manual_override = manual_override_from_ini(value)?;
        }
    }

    if let Some(section) = ini.get("power_plans") {
        settings.power_plans.power_save_guid = read_optional_string(section, "power_save_guid");
        settings.power_plans.performance_guid = read_optional_string(section, "performance_guid");
    }

    if let Some(section) = ini.get("activity_mode") {
        read_bool(section, "enabled", &mut settings.activity_mode.enabled)?;
        read_u64(
            section,
            "idle_timeout_seconds",
            &mut settings.activity_mode.idle_timeout_seconds,
        )?;
        read_bool(
            section,
            "switch_to_performance_on_resume",
            &mut settings.activity_mode.switch_to_performance_on_resume,
        )?;
        read_bool(
            section,
            "input_keyboard",
            &mut settings.activity_mode.input_detection.keyboard,
        )?;
        read_bool(
            section,
            "input_mouse",
            &mut settings.activity_mode.input_detection.mouse,
        )?;
        settings.activity_mode.input_detection.ensure_any_enabled();
        settings.activity_mode.switch_to_performance_on_resume =
            settings.activity_mode.input_detection.any_enabled();
    }

    if let Some(section) = ini.get("foreground_rules") {
        read_bool(section, "enabled", &mut settings.foreground_rules.enabled)?;
        if let Some(value) = section.get("force_active") {
            settings.foreground_rules.whitelist = split_escaped(value);
        }
        if let Some(value) = section.get("force_idle") {
            settings.foreground_rules.force_power_save = split_escaped(value);
        }
    }

    if let Some(section) = ini.get("schedule_mode") {
        read_bool(section, "enabled", &mut settings.schedule_mode.enabled)?;
        let rule_count = section
            .get("rule_count")
            .map(|value| parse_usize(value, "schedule_mode.rule_count"))
            .transpose()?
            .unwrap_or(settings.schedule_mode.rules.len());

        let mut rules = Vec::with_capacity(rule_count);
        for index in 0..rule_count {
            let section_name = format!("schedule_rule.{index}");
            let Some(rule_section) = ini.get(&section_name) else {
                continue;
            };

            let mut rule = settings
                .schedule_mode
                .rules
                .get(index)
                .cloned()
                .unwrap_or_default();
            if let Some(value) = rule_section.get("name") {
                rule.name = unescape_value(value);
            }
            if let Some(value) = rule_section.get("days") {
                rule.days = split_escaped(value)
                    .into_iter()
                    .map(|day| weekday_from_ini(&day))
                    .collect::<Result<Vec<_>, _>>()?;
            }
            if let Some(value) = rule_section.get("start_time") {
                rule.start_time = unescape_value(value);
            }
            if let Some(value) = rule_section.get("end_time") {
                rule.end_time = unescape_value(value);
            }
            rule.power_save_guid = read_optional_string(rule_section, "power_save_guid");
            rule.performance_guid = read_optional_string(rule_section, "performance_guid");
            rules.push(rule);
        }

        settings.schedule_mode.rules = rules;
    }

    if let Some(section) = ini.get("cpu_usage_mode") {
        read_bool(section, "enabled", &mut settings.cpu_usage_mode.enabled)?;
        let rule_count = section
            .get("rule_count")
            .map(|value| parse_usize(value, "cpu_usage_mode.rule_count"))
            .transpose()?
            .unwrap_or(settings.cpu_usage_mode.rules.len());

        let mut rules = Vec::with_capacity(rule_count);
        for index in 0..rule_count {
            let section_name = format!("cpu_usage_rule.{index}");
            let Some(rule_section) = ini.get(&section_name) else {
                continue;
            };

            let mut rule = settings
                .cpu_usage_mode
                .rules
                .get(index)
                .cloned()
                .unwrap_or_else(|| CpuUsageRule {
                    name: "CPU Rule".to_owned(),
                    comparison: CpuUsageComparison::AtOrBelow,
                    threshold_percent: 20,
                    duration_seconds: 30,
                    target: CpuUsageTarget::Idle,
                });
            if let Some(value) = rule_section.get("name") {
                rule.name = unescape_value(value);
            }
            if let Some(value) = rule_section.get("comparison") {
                rule.comparison = cpu_usage_comparison_from_ini(value)?;
            }
            if let Some(value) = rule_section.get("threshold_percent") {
                rule.threshold_percent =
                    parse_u8(value, "cpu_usage_rule.threshold_percent")?.min(100);
            }
            read_u64(rule_section, "duration_seconds", &mut rule.duration_seconds)?;
            if let Some(value) = rule_section.get("target") {
                rule.target = cpu_usage_target_from_ini(value)?;
            }
            rules.push(rule);
        }

        settings.cpu_usage_mode.rules = rules;
    }

    Ok(settings)
}

fn parse_ini(raw: &str) -> Result<BTreeMap<String, BTreeMap<String, String>>, String> {
    let mut ini: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    let mut section = String::new();

    for (line_number, line) in raw.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }

        if line.starts_with('[') && line.ends_with(']') {
            section = line[1..line.len() - 1].trim().to_ascii_lowercase();
            ini.entry(section.clone()).or_default();
            continue;
        }

        let Some((key, value)) = line.split_once('=') else {
            return Err(format!("Line {} is not a key=value entry", line_number + 1));
        };

        ini.entry(section.clone())
            .or_default()
            .insert(key.trim().to_ascii_lowercase(), value.trim().to_owned());
    }

    Ok(ini)
}

fn ini_entry(key: &str, value: impl ToString) -> String {
    format!("{key}={}\n", escape_value(&value.to_string()))
}

fn ini_entry_raw(key: &str, value: impl ToString) -> String {
    format!("{key}={}\n", value.to_string())
}

fn read_bool(
    section: &BTreeMap<String, String>,
    key: &str,
    target: &mut bool,
) -> Result<(), String> {
    if let Some(value) = section.get(key) {
        *target = match unescape_value(value).to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" | "on" => true,
            "false" | "0" | "no" | "off" => false,
            _ => return Err(format!("{key} must be true or false")),
        };
    }
    Ok(())
}

fn read_u64(section: &BTreeMap<String, String>, key: &str, target: &mut u64) -> Result<(), String> {
    if let Some(value) = section.get(key) {
        *target = unescape_value(value)
            .parse()
            .map_err(|err| format!("{key} must be an unsigned integer: {err}"))?;
    }
    Ok(())
}

fn parse_u8(value: &str, key: &str) -> Result<u8, String> {
    unescape_value(value)
        .parse()
        .map_err(|err| format!("{key} must be an unsigned integer from 0 to 255: {err}"))
}

fn parse_usize(value: &str, key: &str) -> Result<usize, String> {
    unescape_value(value)
        .parse()
        .map_err(|err| format!("{key} must be an unsigned integer: {err}"))
}

fn read_optional_string(section: &BTreeMap<String, String>, key: &str) -> Option<String> {
    section
        .get(key)
        .map(|value| unescape_value(value))
        .filter(|value| !value.trim().is_empty())
}

fn manual_override_to_ini(manual_override: &ManualOverride) -> String {
    match manual_override {
        ManualOverride::None => "none".to_owned(),
        ManualOverride::UntilEpochSeconds(until) => format!("until_epoch_seconds:{until}"),
        ManualOverride::UntilRestart => "until_restart".to_owned(),
        ManualOverride::Indefinite => "indefinite".to_owned(),
    }
}

fn manual_override_from_ini(value: &str) -> Result<ManualOverride, String> {
    let value = unescape_value(value);
    match value.to_ascii_lowercase().as_str() {
        "none" => Ok(ManualOverride::None),
        "until_restart" => Ok(ManualOverride::UntilRestart),
        "indefinite" => Ok(ManualOverride::Indefinite),
        value if value.starts_with("until_epoch_seconds:") => value
            .trim_start_matches("until_epoch_seconds:")
            .parse()
            .map(ManualOverride::UntilEpochSeconds)
            .map_err(|err| format!("manual_override has an invalid epoch value: {err}")),
        _ => Err("manual_override must be none, until_restart, indefinite, or until_epoch_seconds:<epoch>".to_owned()),
    }
}

fn cpu_usage_comparison_to_ini(comparison: CpuUsageComparison) -> &'static str {
    match comparison {
        CpuUsageComparison::AtOrAbove => "at_or_above",
        CpuUsageComparison::AtOrBelow => "at_or_below",
    }
}

fn cpu_usage_comparison_from_ini(value: &str) -> Result<CpuUsageComparison, String> {
    match unescape_value(value).to_ascii_lowercase().as_str() {
        "at_or_above" | "above" | "gte" => Ok(CpuUsageComparison::AtOrAbove),
        "at_or_below" | "below" | "lte" => Ok(CpuUsageComparison::AtOrBelow),
        _ => Err("cpu_usage_rule.comparison must be at_or_above or at_or_below".to_owned()),
    }
}

fn cpu_usage_target_to_ini(target: CpuUsageTarget) -> &'static str {
    match target {
        CpuUsageTarget::Active => "active",
        CpuUsageTarget::Idle => "idle",
    }
}

fn cpu_usage_target_from_ini(value: &str) -> Result<CpuUsageTarget, String> {
    match unescape_value(value).to_ascii_lowercase().as_str() {
        "active" | "performance" => Ok(CpuUsageTarget::Active),
        "idle" | "power_save" => Ok(CpuUsageTarget::Idle),
        _ => Err("cpu_usage_rule.target must be active or idle".to_owned()),
    }
}

fn weekday_from_ini(value: &str) -> Result<WeekdaySetting, String> {
    match value.trim().to_ascii_lowercase().as_str() {
        "mon" => Ok(WeekdaySetting::Mon),
        "tue" => Ok(WeekdaySetting::Tue),
        "wed" => Ok(WeekdaySetting::Wed),
        "thu" => Ok(WeekdaySetting::Thu),
        "fri" => Ok(WeekdaySetting::Fri),
        "sat" => Ok(WeekdaySetting::Sat),
        "sun" => Ok(WeekdaySetting::Sun),
        _ => Err(format!("Invalid weekday: {value}")),
    }
}

fn join_escaped(values: &[String]) -> String {
    values
        .iter()
        .map(|value| escape_value(value))
        .collect::<Vec<_>>()
        .join(",")
}

fn split_escaped(value: &str) -> Vec<String> {
    split_escaped_values(value)
        .into_iter()
        .map(|value| unescape_value(&value))
        .filter(|value| !value.trim().is_empty())
        .collect()
}

fn split_escaped_values(value: &str) -> Vec<String> {
    let mut values = Vec::new();
    let mut current = String::new();
    let mut escaped = false;

    for character in value.chars() {
        if escaped {
            current.push('\\');
            current.push(character);
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else if character == ',' {
            values.push(current);
            current = String::new();
        } else {
            current.push(character);
        }
    }

    if escaped {
        current.push('\\');
    }
    values.push(current);
    values
}

fn escape_value(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace(',', "\\,")
}

fn unescape_value(value: &str) -> String {
    let mut output = String::new();
    let mut escaped = false;

    for character in value.chars() {
        if escaped {
            match character {
                'n' => output.push('\n'),
                'r' => output.push('\r'),
                other => output.push(other),
            }
            escaped = false;
        } else if character == '\\' {
            escaped = true;
        } else {
            output.push(character);
        }
    }

    if escaped {
        output.push('\\');
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        ActivityModeSettings, CpuUsageModeSettings, ForegroundRules, GeneralSettings,
        InputDetectionSettings, PowerPlanSettings, ScheduleModeSettings, ScheduleRule,
    };

    #[test]
    fn ini_round_trip_preserves_settings() {
        let settings = Settings {
            general: GeneralSettings {
                enabled: false,
                startup_with_windows: true,
                hide_to_tray: true,
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
            },
            foreground_rules: ForegroundRules {
                enabled: true,
                whitelist: vec!["game.exe".to_owned(), "comma,app.exe".to_owned()],
                force_power_save: vec!["backup\\tool.exe".to_owned()],
            },
            schedule_mode: ScheduleModeSettings {
                enabled: true,
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
                rules: vec![CpuUsageRule {
                    name: "Low CPU".to_owned(),
                    comparison: CpuUsageComparison::AtOrBelow,
                    threshold_percent: 18,
                    duration_seconds: 45,
                    target: CpuUsageTarget::Idle,
                }],
            },
        };

        let raw = settings_to_ini(&settings);
        let parsed = settings_from_ini(&raw).expect("INI should parse");

        assert_eq!(parsed, settings);
    }
}
