use chrono::{NaiveTime, Weekday};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    pub general: GeneralSettings,
    pub power_plans: PowerPlanSettings,
    pub activity_mode: ActivityModeSettings,
    pub foreground_rules: ForegroundRules,
    pub schedule_mode: ScheduleModeSettings,
    #[serde(default)]
    pub cpu_usage_mode: CpuUsageModeSettings,
    #[serde(default)]
    pub eco_qos: EcoQosSettings,
    #[serde(default)]
    pub app_suspension: AppSuspensionSettings,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneralSettings {
    pub enabled: bool,
    pub startup_with_windows: bool,
    #[serde(default)]
    pub hide_to_tray: bool,
    pub check_interval_ms: u64,
    pub manual_override: ManualOverride,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ManualOverride {
    None,
    UntilEpochSeconds(i64),
    UntilRestart,
    Indefinite,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PowerPlanSettings {
    pub power_save_guid: Option<String>,
    pub performance_guid: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActivityModeSettings {
    pub enabled: bool,
    pub idle_timeout_seconds: u64,
    pub switch_to_performance_on_resume: bool,
    #[serde(default)]
    pub input_detection: InputDetectionSettings,
    #[serde(default)]
    pub power_plans: PowerPlanSettings,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputDetectionSettings {
    pub keyboard: bool,
    pub mouse: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForegroundRules {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub rules: Vec<ForegroundRule>,
    #[serde(default)]
    pub whitelist: Vec<String>,
    #[serde(default)]
    pub force_power_save: Vec<String>,
    #[serde(default)]
    pub power_plans: PowerPlanSettings,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForegroundRule {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub process_name: String,
    #[serde(default)]
    pub power_plan_guid: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleModeSettings {
    pub enabled: bool,
    pub rules: Vec<ScheduleRule>,
    #[serde(default)]
    pub power_plans: PowerPlanSettings,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleRule {
    pub name: String,
    pub days: Vec<WeekdaySetting>,
    pub start_time: String,
    pub end_time: String,
    pub power_save_guid: Option<String>,
    pub performance_guid: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CpuUsageModeSettings {
    pub enabled: bool,
    pub rules: Vec<CpuUsageRule>,
    #[serde(default)]
    pub power_plans: PowerPlanSettings,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EcoQosSettings {
    pub enabled: bool,
    #[serde(
        default = "default_exclude_foreground_app",
        alias = "ignore_foreground_app"
    )]
    pub exclude_foreground_app: bool,
    #[serde(default, alias = "excluded_processes")]
    pub efficiency_whitelist: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppSuspensionSettings {
    pub enabled: bool,
    pub background_delay_seconds: u64,
    #[serde(default, alias = "suspend_whitelist")]
    pub suspendable_apps: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CpuUsageRule {
    pub name: String,
    pub comparison: CpuUsageComparison,
    pub threshold_percent: u8,
    pub duration_seconds: u64,
    pub target: CpuUsageTarget,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CpuUsageComparison {
    AtOrAbove,
    AtOrBelow,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CpuUsageTarget {
    Active,
    Idle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WeekdaySetting {
    Mon,
    Tue,
    Wed,
    Thu,
    Fri,
    Sat,
    Sun,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            general: GeneralSettings {
                enabled: true,
                startup_with_windows: false,
                hide_to_tray: false,
                check_interval_ms: 1000,
                manual_override: ManualOverride::None,
            },
            power_plans: PowerPlanSettings::default(),
            activity_mode: ActivityModeSettings {
                enabled: true,
                idle_timeout_seconds: 300,
                switch_to_performance_on_resume: true,
                input_detection: InputDetectionSettings::default(),
                power_plans: PowerPlanSettings::default(),
            },
            foreground_rules: ForegroundRules::default(),
            schedule_mode: ScheduleModeSettings {
                enabled: false,
                power_plans: PowerPlanSettings::default(),
                rules: vec![ScheduleRule {
                    name: "Night Idle Plan".to_owned(),
                    days: WeekdaySetting::all().to_vec(),
                    start_time: "22:00".to_owned(),
                    end_time: "08:00".to_owned(),
                    power_save_guid: None,
                    performance_guid: None,
                }],
            },
            cpu_usage_mode: CpuUsageModeSettings::default(),
            eco_qos: EcoQosSettings::default(),
            app_suspension: AppSuspensionSettings::default(),
        }
    }
}

impl Default for InputDetectionSettings {
    fn default() -> Self {
        Self {
            keyboard: true,
            mouse: true,
        }
    }
}

impl Default for ForegroundRules {
    fn default() -> Self {
        Self {
            enabled: true,
            rules: Vec::new(),
            whitelist: Vec::new(),
            force_power_save: Vec::new(),
            power_plans: PowerPlanSettings::default(),
        }
    }
}

impl Default for CpuUsageModeSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            power_plans: PowerPlanSettings::default(),
            rules: vec![
                CpuUsageRule {
                    name: "Low CPU Idle".to_owned(),
                    comparison: CpuUsageComparison::AtOrBelow,
                    threshold_percent: 15,
                    duration_seconds: 60,
                    target: CpuUsageTarget::Idle,
                },
                CpuUsageRule {
                    name: "High CPU Active".to_owned(),
                    comparison: CpuUsageComparison::AtOrAbove,
                    threshold_percent: 50,
                    duration_seconds: 10,
                    target: CpuUsageTarget::Active,
                },
            ],
        }
    }
}

impl Default for EcoQosSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            exclude_foreground_app: default_exclude_foreground_app(),
            efficiency_whitelist: Vec::new(),
        }
    }
}

const fn default_exclude_foreground_app() -> bool {
    true
}

impl Default for AppSuspensionSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            background_delay_seconds: 300,
            suspendable_apps: Vec::new(),
        }
    }
}

impl InputDetectionSettings {
    pub const fn any_enabled(&self) -> bool {
        self.keyboard || self.mouse
    }

    pub fn ensure_any_enabled(&mut self) {
        if !self.any_enabled() {
            self.keyboard = true;
        }
    }
}

impl Settings {
    pub fn fill_missing_power_plan_mappings(&mut self) {
        self.activity_mode
            .power_plans
            .fill_missing_from(&self.power_plans);
        self.foreground_rules
            .power_plans
            .fill_missing_from(&self.power_plans);
        self.schedule_mode
            .power_plans
            .fill_missing_from(&self.power_plans);
        self.cpu_usage_mode
            .power_plans
            .fill_missing_from(&self.power_plans);

        if let Some(rule) = self.schedule_mode.rules.first() {
            let fallback = PowerPlanSettings {
                power_save_guid: rule.power_save_guid.clone(),
                performance_guid: rule.performance_guid.clone(),
            };
            self.schedule_mode.power_plans.fill_missing_from(&fallback);
        }

        self.migrate_legacy_foreground_rules();
    }

    fn migrate_legacy_foreground_rules(&mut self) {
        if !self.foreground_rules.rules.is_empty() {
            return;
        }

        for process in &self.foreground_rules.whitelist {
            self.foreground_rules.rules.push(ForegroundRule {
                name: process.clone(),
                process_name: process.clone(),
                power_plan_guid: self
                    .foreground_rules
                    .power_plans
                    .performance_guid
                    .clone()
                    .or_else(|| self.power_plans.performance_guid.clone()),
            });
        }

        for process in &self.foreground_rules.force_power_save {
            self.foreground_rules.rules.push(ForegroundRule {
                name: process.clone(),
                process_name: process.clone(),
                power_plan_guid: self
                    .foreground_rules
                    .power_plans
                    .power_save_guid
                    .clone()
                    .or_else(|| self.power_plans.power_save_guid.clone()),
            });
        }
    }
}

impl PowerPlanSettings {
    pub fn fill_missing_from(&mut self, fallback: &Self) {
        if self.power_save_guid.is_none() {
            self.power_save_guid = fallback.power_save_guid.clone();
        }
        if self.performance_guid.is_none() {
            self.performance_guid = fallback.performance_guid.clone();
        }
    }
}

impl ManualOverride {
    pub fn is_active(&self, now_epoch_seconds: i64) -> bool {
        match self {
            ManualOverride::None => false,
            ManualOverride::UntilEpochSeconds(until) => now_epoch_seconds < *until,
            ManualOverride::UntilRestart | ManualOverride::Indefinite => true,
        }
    }
}

impl WeekdaySetting {
    pub const fn all() -> [Self; 7] {
        [
            Self::Mon,
            Self::Tue,
            Self::Wed,
            Self::Thu,
            Self::Fri,
            Self::Sat,
            Self::Sun,
        ]
    }

    pub const fn short_label(self) -> &'static str {
        match self {
            Self::Mon => "Mon",
            Self::Tue => "Tue",
            Self::Wed => "Wed",
            Self::Thu => "Thu",
            Self::Fri => "Fri",
            Self::Sat => "Sat",
            Self::Sun => "Sun",
        }
    }

    pub const fn from_chrono(day: Weekday) -> Self {
        match day {
            Weekday::Mon => Self::Mon,
            Weekday::Tue => Self::Tue,
            Weekday::Wed => Self::Wed,
            Weekday::Thu => Self::Thu,
            Weekday::Fri => Self::Fri,
            Weekday::Sat => Self::Sat,
            Weekday::Sun => Self::Sun,
        }
    }
}

impl ScheduleRule {
    pub fn parsed_times(&self) -> Option<(NaiveTime, NaiveTime)> {
        let start = NaiveTime::parse_from_str(&self.start_time, "%H:%M").ok()?;
        let end = NaiveTime::parse_from_str(&self.end_time, "%H:%M").ok()?;
        Some((start, end))
    }
}

impl CpuUsageRule {
    pub fn matches_usage(&self, cpu_usage_percent: f32) -> bool {
        let threshold = f32::from(self.threshold_percent.min(100));
        match self.comparison {
            CpuUsageComparison::AtOrAbove => cpu_usage_percent >= threshold,
            CpuUsageComparison::AtOrBelow => cpu_usage_percent <= threshold,
        }
    }
}

impl CpuUsageComparison {
    pub const fn label(self) -> &'static str {
        match self {
            Self::AtOrAbove => "At or above",
            Self::AtOrBelow => "At or below",
        }
    }
}

impl CpuUsageTarget {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Active => "Active plan",
            Self::Idle => "Idle plan",
        }
    }
}
