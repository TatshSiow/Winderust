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
    #[serde(default)]
    pub cpu_affinity: CpuAffinitySettings,
    #[serde(default)]
    pub cpu_limiter: CpuLimiterSettings,
    #[serde(default)]
    pub performance_mode: PerformanceModeSettings,
    #[serde(default)]
    pub watchdog: WatchdogSettings,
    #[serde(default)]
    pub foreground_responsiveness: ForegroundResponsivenessSettings,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneralSettings {
    pub enabled: bool,
    pub startup_with_windows: bool,
    #[serde(default)]
    pub start_minimized: bool,
    #[serde(default)]
    pub hide_to_tray: bool,
    #[serde(default)]
    pub theme_mode: AppThemeMode,
    #[serde(default)]
    pub accent: AccentSettings,
    #[serde(default)]
    pub language: AppLanguage,
    #[serde(default)]
    pub pause_power_plan_switching_while_plugged_in: bool,
    pub check_interval_ms: u64,
    pub manual_override: ManualOverride,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppThemeMode {
    #[default]
    System,
    Light,
    Dark,
}

impl AppThemeMode {
    pub const ALL: [Self; 3] = [Self::System, Self::Light, Self::Dark];
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccentSettings {
    #[serde(default)]
    pub source: AccentColorSource,
    #[serde(default = "default_custom_accent_color")]
    pub custom_color: u32,
    #[serde(default)]
    pub custom_colors: Vec<u32>,
}

impl Default for AccentSettings {
    fn default() -> Self {
        Self {
            source: AccentColorSource::Windows,
            custom_color: default_custom_accent_color(),
            custom_colors: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccentColorSource {
    #[default]
    Windows,
    Custom,
}

impl AccentColorSource {
    pub const ALL: [Self; 2] = [Self::Windows, Self::Custom];
}

fn default_custom_accent_color() -> u32 {
    0x4cc2ff
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AppLanguage {
    #[default]
    English,
    ZhTw,
}

impl AppLanguage {
    pub const ALL: [Self; 2] = [Self::English, Self::ZhTw];

    pub const fn locale(self) -> &'static str {
        match self {
            Self::English => "en",
            Self::ZhTw => "zh-TW",
        }
    }

    pub const fn native_label(self) -> &'static str {
        match self {
            Self::English => "English",
            Self::ZhTw => "繁體中文（台灣）",
        }
    }
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
    #[serde(default = "default_rule_enabled")]
    pub enabled: bool,
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
    #[serde(default, skip_serializing_if = "PowerPlanSettings::is_empty")]
    pub power_plans: PowerPlanSettings,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduleRule {
    #[serde(default = "default_rule_enabled")]
    pub enabled: bool,
    pub name: String,
    pub days: Vec<WeekdaySetting>,
    pub start_time: String,
    pub end_time: String,
    #[serde(default)]
    pub power_plan_guid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub power_save_guid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub performance_guid: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CpuUsageModeSettings {
    pub enabled: bool,
    pub rules: Vec<CpuUsageRule>,
    #[serde(default, skip_serializing_if = "PowerPlanSettings::is_empty")]
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
    #[serde(default)]
    pub prefer_efficiency_cores: bool,
    #[serde(default)]
    pub limit_cpu_sets_on_non_hybrid: bool,
    #[serde(default, alias = "excluded_processes")]
    pub efficiency_whitelist: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppSuspensionSettings {
    pub enabled: bool,
    pub background_delay_seconds: u64,
    #[serde(default)]
    pub temporary_thaw_enabled: bool,
    #[serde(default = "default_temporary_thaw_interval_seconds")]
    pub temporary_thaw_interval_seconds: u64,
    #[serde(default = "default_temporary_thaw_duration_seconds")]
    pub temporary_thaw_duration_seconds: u64,
    #[serde(default)]
    pub network_wake_enabled: bool,
    #[serde(default = "default_network_wake_duration_seconds")]
    pub network_wake_duration_seconds: u64,
    #[serde(default)]
    pub audio_wake_enabled: bool,
    #[serde(default = "default_audio_wake_duration_seconds")]
    pub audio_wake_duration_seconds: u64,
    #[serde(
        default,
        alias = "suspend_whitelist",
        deserialize_with = "deserialize_app_suspension_rules"
    )]
    pub suspendable_apps: Vec<AppSuspensionRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CpuAffinitySettings {
    pub enabled: bool,
    #[serde(
        default = "default_exclude_foreground_app",
        alias = "ignore_foreground_app"
    )]
    pub exclude_foreground_app: bool,
    #[serde(default)]
    pub rules: Vec<CpuAffinityRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CpuAffinityRule {
    #[serde(default = "default_rule_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub mode: CpuAffinityMode,
    pub process_name: String,
    pub core_mask: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CpuLimiterSettings {
    pub enabled: bool,
    #[serde(
        default = "default_exclude_foreground_app",
        alias = "ignore_foreground_app"
    )]
    pub exclude_foreground_app: bool,
    #[serde(default)]
    pub rules: Vec<CpuLimiterRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CpuLimiterRule {
    #[serde(default = "default_rule_enabled")]
    pub enabled: bool,
    pub process_name: String,
    #[serde(default = "default_cpu_limiter_threshold_percent")]
    pub threshold_percent: u8,
    #[serde(default = "default_cpu_limiter_sustain_seconds")]
    pub sustain_seconds: u64,
    #[serde(default = "default_cpu_limiter_cooldown_seconds")]
    pub cooldown_seconds: u64,
    #[serde(default = "default_cpu_limiter_max_logical_processors")]
    pub max_logical_processors: u8,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PerformanceModeSettings {
    pub enabled: bool,
    #[serde(default)]
    pub rules: Vec<PerformanceModeRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PerformanceModeRule {
    #[serde(default = "default_rule_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub name: String,
    pub process_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub power_plan_guid: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatchdogSettings {
    pub enabled: bool,
    #[serde(default)]
    pub rules: Vec<WatchdogRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WatchdogRule {
    #[serde(default = "default_rule_enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub name: String,
    pub process_name: String,
    #[serde(default)]
    pub action: WatchdogAction,
    #[serde(default)]
    pub launch_path: String,
    #[serde(default)]
    pub launch_args: Vec<String>,
    #[serde(default = "default_watchdog_restart_delay_seconds")]
    pub restart_delay_seconds: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WatchdogAction {
    #[default]
    TerminateOnLaunch,
    RestartIfExited,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForegroundResponsivenessSettings {
    pub enabled: bool,
    #[serde(default = "default_lower_background_apps")]
    pub lower_background_apps: bool,
    #[serde(default)]
    pub auto_balance_enabled: bool,
    #[serde(default = "default_auto_balance_threshold_percent")]
    pub auto_balance_threshold_percent: u8,
    #[serde(default = "default_auto_balance_sustain_seconds")]
    pub auto_balance_sustain_seconds: u64,
    #[serde(default = "default_auto_balance_cooldown_seconds")]
    pub auto_balance_cooldown_seconds: u64,
    #[serde(default)]
    pub boost_foreground_app: bool,
    #[serde(default)]
    pub foreground_boost: ForegroundBoostPriority,
    #[serde(default = "default_foreground_stability_delay_ms")]
    pub foreground_stability_delay_ms: u64,
    #[serde(default)]
    pub rules: Vec<PriorityRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PriorityRule {
    #[serde(default = "default_rule_enabled")]
    pub enabled: bool,
    pub process_name: String,
    pub priority: ProcessPriority,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessPriority {
    Normal,
    BelowNormal,
    #[default]
    Idle,
}

impl ProcessPriority {
    pub const ALL: [Self; 3] = [Self::Normal, Self::BelowNormal, Self::Idle];
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ForegroundBoostPriority {
    Normal,
    #[default]
    AboveNormal,
}

impl ForegroundBoostPriority {
    pub const ALL: [Self; 2] = [Self::Normal, Self::AboveNormal];
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CpuAffinityMode {
    #[default]
    Hard,
    Soft,
    EfficiencyOff,
}

impl CpuAffinityMode {
    pub const ALL: [Self; 3] = [Self::Hard, Self::Soft, Self::EfficiencyOff];
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppSuspensionRule {
    pub process_name: String,
    #[serde(default = "default_rule_network_wake_enabled")]
    pub network_wake_enabled: bool,
    #[serde(default = "default_rule_audio_wake_enabled")]
    pub audio_wake_enabled: bool,
    #[serde(default = "default_rule_network_download_threshold_bytes")]
    pub network_download_threshold_bytes: u64,
    #[serde(default)]
    pub network_download_threshold_unit: NetworkThresholdUnit,
    #[serde(default)]
    pub network_upload_threshold_bytes: u64,
    #[serde(default)]
    pub network_upload_threshold_unit: NetworkThresholdUnit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkThresholdUnit {
    Bytes,
    Kilobytes,
    Megabytes,
    Gigabytes,
    Bits,
    Kilobits,
    Megabits,
    Gigabits,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CpuUsageRule {
    #[serde(default = "default_rule_enabled")]
    pub enabled: bool,
    pub name: String,
    pub comparison: CpuUsageComparison,
    pub threshold_percent: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upper_threshold_percent: Option<u8>,
    pub duration_seconds: u64,
    #[serde(default)]
    pub power_plan_guid: Option<String>,
    #[serde(default)]
    pub else_enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub else_power_plan_guid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<CpuUsageTarget>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CpuUsageComparison {
    AtOrAbove,
    AtOrBelow,
    Between,
    Else,
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
                start_minimized: false,
                hide_to_tray: false,
                theme_mode: AppThemeMode::System,
                accent: AccentSettings::default(),
                language: AppLanguage::English,
                pause_power_plan_switching_while_plugged_in: false,
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
                    enabled: true,
                    name: "Night Idle Plan".to_owned(),
                    days: WeekdaySetting::all().to_vec(),
                    start_time: "22:00".to_owned(),
                    end_time: "08:00".to_owned(),
                    power_plan_guid: None,
                    power_save_guid: None,
                    performance_guid: None,
                }],
            },
            cpu_usage_mode: CpuUsageModeSettings::default(),
            eco_qos: EcoQosSettings::default(),
            app_suspension: AppSuspensionSettings::default(),
            cpu_affinity: CpuAffinitySettings::default(),
            cpu_limiter: CpuLimiterSettings::default(),
            performance_mode: PerformanceModeSettings::default(),
            watchdog: WatchdogSettings::default(),
            foreground_responsiveness: ForegroundResponsivenessSettings::default(),
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
                    enabled: true,
                    name: "Low CPU Idle".to_owned(),
                    comparison: CpuUsageComparison::AtOrBelow,
                    threshold_percent: 15,
                    upper_threshold_percent: None,
                    duration_seconds: 60,
                    power_plan_guid: None,
                    else_enabled: false,
                    else_power_plan_guid: None,
                    target: None,
                },
                CpuUsageRule {
                    enabled: true,
                    name: "High CPU Active".to_owned(),
                    comparison: CpuUsageComparison::AtOrAbove,
                    threshold_percent: 50,
                    upper_threshold_percent: None,
                    duration_seconds: 10,
                    power_plan_guid: None,
                    else_enabled: false,
                    else_power_plan_guid: None,
                    target: None,
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
            prefer_efficiency_cores: false,
            limit_cpu_sets_on_non_hybrid: false,
            efficiency_whitelist: Vec::new(),
        }
    }
}

const fn default_exclude_foreground_app() -> bool {
    true
}

const fn default_rule_enabled() -> bool {
    true
}

const fn default_lower_background_apps() -> bool {
    true
}

const fn default_auto_balance_threshold_percent() -> u8 {
    25
}

const fn default_auto_balance_sustain_seconds() -> u64 {
    2
}

const fn default_auto_balance_cooldown_seconds() -> u64 {
    10
}

const fn default_foreground_stability_delay_ms() -> u64 {
    750
}

const fn default_cpu_limiter_threshold_percent() -> u8 {
    75
}

const fn default_cpu_limiter_sustain_seconds() -> u64 {
    5
}

const fn default_cpu_limiter_cooldown_seconds() -> u64 {
    10
}

const fn default_cpu_limiter_max_logical_processors() -> u8 {
    1
}

const fn default_watchdog_restart_delay_seconds() -> u64 {
    5
}

const fn default_temporary_thaw_interval_seconds() -> u64 {
    900
}

const fn default_temporary_thaw_duration_seconds() -> u64 {
    20
}

const fn default_network_wake_duration_seconds() -> u64 {
    30
}

const fn default_audio_wake_duration_seconds() -> u64 {
    10
}

const fn default_rule_network_wake_enabled() -> bool {
    true
}

const fn default_rule_audio_wake_enabled() -> bool {
    true
}

const fn default_rule_network_download_threshold_bytes() -> u64 {
    1
}

impl Default for NetworkThresholdUnit {
    fn default() -> Self {
        Self::Bytes
    }
}

impl NetworkThresholdUnit {
    pub const ALL: [Self; 8] = [
        Self::Bytes,
        Self::Kilobytes,
        Self::Megabytes,
        Self::Gigabytes,
        Self::Bits,
        Self::Kilobits,
        Self::Megabits,
        Self::Gigabits,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Bytes => "B",
            Self::Kilobytes => "KB",
            Self::Megabytes => "MB",
            Self::Gigabytes => "GB",
            Self::Bits => "b",
            Self::Kilobits => "kb",
            Self::Megabits => "mb",
            Self::Gigabits => "gb",
        }
    }

    pub fn threshold_value_from_bytes(self, bytes: u64) -> f64 {
        let bytes = bytes as f64;
        match self {
            Self::Bytes => bytes,
            Self::Kilobytes => bytes / 1_000.0,
            Self::Megabytes => bytes / 1_000_000.0,
            Self::Gigabytes => bytes / 1_000_000_000.0,
            Self::Bits => bytes * 8.0,
            Self::Kilobits => bytes * 8.0 / 1_000.0,
            Self::Megabits => bytes * 8.0 / 1_000_000.0,
            Self::Gigabits => bytes * 8.0 / 1_000_000_000.0,
        }
    }

    pub fn threshold_bytes_from_value(self, value: f64) -> u64 {
        if !value.is_finite() || value <= 0.0 {
            return 0;
        }

        let bytes = match self {
            Self::Bytes => value,
            Self::Kilobytes => value * 1_000.0,
            Self::Megabytes => value * 1_000_000.0,
            Self::Gigabytes => value * 1_000_000_000.0,
            Self::Bits => value / 8.0,
            Self::Kilobits => value * 1_000.0 / 8.0,
            Self::Megabits => value * 1_000_000.0 / 8.0,
            Self::Gigabits => value * 1_000_000_000.0 / 8.0,
        };

        if bytes >= u64::MAX as f64 {
            u64::MAX
        } else {
            bytes.ceil() as u64
        }
    }
}

impl Default for AppSuspensionSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            background_delay_seconds: 300,
            temporary_thaw_enabled: false,
            temporary_thaw_interval_seconds: default_temporary_thaw_interval_seconds(),
            temporary_thaw_duration_seconds: default_temporary_thaw_duration_seconds(),
            network_wake_enabled: false,
            network_wake_duration_seconds: default_network_wake_duration_seconds(),
            audio_wake_enabled: false,
            audio_wake_duration_seconds: default_audio_wake_duration_seconds(),
            suspendable_apps: Vec::new(),
        }
    }
}

impl Default for CpuAffinitySettings {
    fn default() -> Self {
        Self {
            enabled: false,
            exclude_foreground_app: default_exclude_foreground_app(),
            rules: Vec::new(),
        }
    }
}

impl Default for CpuLimiterSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            exclude_foreground_app: default_exclude_foreground_app(),
            rules: Vec::new(),
        }
    }
}

impl Default for PerformanceModeSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            rules: Vec::new(),
        }
    }
}

impl Default for WatchdogSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            rules: Vec::new(),
        }
    }
}

impl Default for ForegroundResponsivenessSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            lower_background_apps: default_lower_background_apps(),
            auto_balance_enabled: false,
            auto_balance_threshold_percent: default_auto_balance_threshold_percent(),
            auto_balance_sustain_seconds: default_auto_balance_sustain_seconds(),
            auto_balance_cooldown_seconds: default_auto_balance_cooldown_seconds(),
            boost_foreground_app: false,
            foreground_boost: ForegroundBoostPriority::AboveNormal,
            foreground_stability_delay_ms: default_foreground_stability_delay_ms(),
            rules: Vec::new(),
        }
    }
}

impl AppSuspensionSettings {
    pub fn contains_suspendable_app(&self, process_name: &str) -> bool {
        self.suspendable_apps.iter().any(|rule| {
            rule.process_name
                .trim()
                .eq_ignore_ascii_case(process_name.trim())
        })
    }

    pub fn network_wake_enabled_for(&self, process_name: &str) -> bool {
        self.network_wake_enabled
            && self.suspendable_apps.iter().any(|rule| {
                rule.network_wake_enabled
                    && rule
                        .process_name
                        .trim()
                        .eq_ignore_ascii_case(process_name.trim())
            })
    }

    pub fn audio_wake_enabled_for(&self, process_name: &str) -> bool {
        self.audio_wake_enabled
            && self.suspendable_apps.iter().any(|rule| {
                rule.audio_wake_enabled
                    && rule
                        .process_name
                        .trim()
                        .eq_ignore_ascii_case(process_name.trim())
            })
    }

    pub fn network_wake_thresholds_for(&self, process_name: &str) -> Option<(u64, u64)> {
        self.network_wake_enabled.then_some(())?;
        self.suspendable_apps.iter().find_map(|rule| {
            (rule.network_wake_enabled
                && rule
                    .process_name
                    .trim()
                    .eq_ignore_ascii_case(process_name.trim()))
            .then_some((
                rule.network_download_threshold_bytes,
                rule.network_upload_threshold_bytes,
            ))
        })
    }
}

impl CpuAffinitySettings {
    pub fn contains_rule_for(&self, process_name: &str) -> bool {
        self.rules.iter().any(|rule| {
            rule.process_name
                .trim()
                .eq_ignore_ascii_case(process_name.trim())
        })
    }
}

impl ForegroundResponsivenessSettings {
    pub fn contains_rule_for(&self, process_name: &str) -> bool {
        self.rules.iter().any(|rule| {
            rule.process_name
                .trim()
                .eq_ignore_ascii_case(process_name.trim())
        })
    }
}

#[derive(Deserialize)]
#[serde(untagged)]
enum AppSuspensionRuleInput {
    ProcessName(String),
    Rule(AppSuspensionRule),
}

fn deserialize_app_suspension_rules<'de, D>(
    deserializer: D,
) -> Result<Vec<AppSuspensionRule>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let rules = Vec::<AppSuspensionRuleInput>::deserialize(deserializer)?;
    Ok(rules
        .into_iter()
        .filter_map(|rule| match rule {
            AppSuspensionRuleInput::ProcessName(process_name) => {
                let process_name = process_name.trim().to_ascii_lowercase();
                let download_threshold = default_rule_network_download_threshold_bytes();
                (!process_name.is_empty()).then_some(AppSuspensionRule {
                    process_name,
                    network_wake_enabled: true,
                    audio_wake_enabled: true,
                    network_download_threshold_bytes: download_threshold,
                    network_download_threshold_unit: NetworkThresholdUnit::Bytes,
                    network_upload_threshold_bytes: 0,
                    network_upload_threshold_unit: NetworkThresholdUnit::Bytes,
                })
            }
            AppSuspensionRuleInput::Rule(mut rule) => {
                rule.process_name = rule.process_name.trim().to_ascii_lowercase();
                (!rule.process_name.is_empty()).then_some(rule)
            }
        })
        .collect())
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
        self.cpu_usage_mode
            .power_plans
            .fill_missing_from(&self.power_plans);

        self.migrate_legacy_schedule_rules();
        self.migrate_legacy_cpu_usage_rules();
        self.migrate_legacy_foreground_rules();
    }

    fn migrate_legacy_schedule_rules(&mut self) {
        let schedule_power_save_guid = self.schedule_mode.power_plans.power_save_guid.clone();
        let fallback_power_save_guid = self.power_plans.power_save_guid.clone();

        for rule in &mut self.schedule_mode.rules {
            if rule.power_plan_guid.is_none() {
                rule.power_plan_guid = rule
                    .power_save_guid
                    .clone()
                    .or_else(|| schedule_power_save_guid.clone())
                    .or_else(|| fallback_power_save_guid.clone());
            }

            rule.power_save_guid = None;
            rule.performance_guid = None;
        }

        self.schedule_mode.power_plans = PowerPlanSettings::default();
    }

    fn migrate_legacy_cpu_usage_rules(&mut self) {
        let idle_guid = self.cpu_usage_mode.power_plans.power_save_guid.clone();
        let active_guid = self.cpu_usage_mode.power_plans.performance_guid.clone();

        for rule in &mut self.cpu_usage_mode.rules {
            if rule.power_plan_guid.is_none() {
                rule.power_plan_guid = match rule.target {
                    Some(CpuUsageTarget::Idle) => idle_guid.clone(),
                    Some(CpuUsageTarget::Active) => active_guid.clone(),
                    None => None,
                };
            }

            if rule.is_else() {
                rule.else_enabled = true;
                if rule.else_power_plan_guid.is_none() {
                    rule.else_power_plan_guid = rule.power_plan_guid.clone();
                }
                rule.power_plan_guid = None;
                rule.comparison = CpuUsageComparison::AtOrBelow;
            }

            rule.target = None;
        }

        self.cpu_usage_mode.power_plans = PowerPlanSettings::default();
    }

    fn migrate_legacy_foreground_rules(&mut self) {
        if !self.foreground_rules.rules.is_empty() {
            return;
        }

        for process in &self.foreground_rules.whitelist {
            self.foreground_rules.rules.push(ForegroundRule {
                enabled: true,
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
                enabled: true,
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
    pub fn is_empty(&self) -> bool {
        self.power_save_guid.is_none() && self.performance_guid.is_none()
    }

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
            CpuUsageComparison::Between => {
                let upper = f32::from(self.upper_threshold_percent.unwrap_or(100).min(100));
                let lower = threshold.min(upper);
                let upper = threshold.max(upper);
                cpu_usage_percent >= lower && cpu_usage_percent <= upper
            }
            CpuUsageComparison::Else => false,
        }
    }

    pub const fn is_else(&self) -> bool {
        matches!(self.comparison, CpuUsageComparison::Else)
    }
}

impl CpuUsageComparison {
    pub const fn label(self) -> &'static str {
        match self {
            Self::AtOrAbove => ">= greater than or equal to",
            Self::AtOrBelow => "<= less than or equal to",
            Self::Between => "between",
            Self::Else => "else",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_threshold_units_convert_to_canonical_bytes() {
        assert_eq!(
            NetworkThresholdUnit::Bytes.threshold_bytes_from_value(42.0),
            42
        );
        assert_eq!(
            NetworkThresholdUnit::Kilobytes.threshold_bytes_from_value(1.5),
            1_500
        );
        assert_eq!(
            NetworkThresholdUnit::Megabytes.threshold_bytes_from_value(1.25),
            1_250_000
        );
        assert_eq!(
            NetworkThresholdUnit::Bits.threshold_bytes_from_value(9.0),
            2
        );
        assert_eq!(
            NetworkThresholdUnit::Kilobits.threshold_bytes_from_value(1.0),
            125
        );
        assert_eq!(
            NetworkThresholdUnit::Megabits.threshold_value_from_bytes(125_000),
            1.0
        );
    }
}
