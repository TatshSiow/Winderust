use chrono::{NaiveTime, Weekday};
use serde::{Deserialize, Serialize};

use crate::foreground::same_process_name;
use crate::power::plan::{ProcessorBoostMode, ProcessorPowerValues};
use crate::rules::{
    normalize_execution_failure_suppression_threshold,
    DEFAULT_EXECUTION_FAILURE_SUPPRESSION_THRESHOLD,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    pub general: GeneralSettings,
    #[serde(default)]
    pub advanced: AdvancedSettings,
    #[serde(default)]
    pub adaptive_engine: AdaptiveEngineSettings,
    pub by_activity: ByActivitySettings,
    pub by_foreground: ByForegroundSettings,
    pub by_time: ByTimeSettings,
    #[serde(default)]
    pub by_cpu_load: ByCpuLoadSettings,
    #[serde(default)]
    pub background_efficiency: BackgroundEfficiencySettings,
    #[serde(default)]
    pub app_suspension: AppSuspensionSettings,
    #[serde(default)]
    pub core_steering: CoreSteeringSettings,
    #[serde(default)]
    pub background_cpu_restriction: BackgroundCpuRestrictionSettings,
    #[serde(default)]
    pub core_limiter: CoreLimiterSettings,
    #[serde(default)]
    pub by_running_app: ByRunningAppSettings,
    #[serde(default)]
    pub workload_engine: WorkloadEngineSettings,
    #[serde(default)]
    pub process_priority: ProcessPrioritySettings,
    #[serde(default)]
    pub thread_priority: ThreadPrioritySettings,
    #[serde(default)]
    pub dynamic_priority_boost: DynamicPriorityBoostSettings,
    #[serde(default)]
    pub io_priority: IoPrioritySettings,
    #[serde(default)]
    pub gpu_priority: GpuPrioritySettings,
    #[serde(default)]
    pub memory_priority: MemoryPrioritySettings,
    #[serde(default)]
    pub memory_trim: MemoryTrimSettings,
    #[serde(default)]
    pub timer_resolution: TimerResolutionSettings,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdvancedSettings {
    #[serde(default)]
    pub action_log_mode: ActionLogMode,
    #[serde(default = "default_execution_failure_suppression_threshold")]
    pub execution_failure_suppression_threshold: u8,
    #[serde(default)]
    pub expose_all_priority_values: bool,
    #[serde(default)]
    pub show_advanced_controls: bool,
}

impl AdvancedSettings {
    pub fn execution_failure_suppression_threshold(&self) -> u8 {
        normalize_execution_failure_suppression_threshold(
            self.execution_failure_suppression_threshold,
        )
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionLogMode {
    Off,
    Error,
    Warning,
    #[default]
    Full,
}

impl ActionLogMode {
    pub const ALL: [Self; 4] = [Self::Full, Self::Warning, Self::Error, Self::Off];
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GeneralSettings {
    pub enabled: bool,
    pub startup_with_windows: bool,
    #[serde(default)]
    pub start_minimized: bool,
    #[serde(default)]
    pub hide_to_tray: bool,
    #[serde(default = "default_true")]
    pub check_for_updates: bool,
    #[serde(default)]
    pub update_channel: UpdateChannel,
    #[serde(default)]
    pub theme_mode: AppThemeMode,
    #[serde(default)]
    pub accent: AccentSettings,
    #[serde(default)]
    pub language: AppLanguage,
    #[serde(default)]
    pub animation_mode: AnimationMode,
    #[serde(default)]
    pub pause_power_plan_switching_while_plugged_in: bool,
    pub check_interval_ms: u64,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpdateChannel {
    Stable,
    #[default]
    PreRelease,
}

impl UpdateChannel {
    pub const ALL: [Self; 2] = [Self::Stable, Self::PreRelease];
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnimationMode {
    #[default]
    System,
    On,
    Off,
}

impl AnimationMode {
    pub const ALL: [Self; 3] = [Self::System, Self::On, Self::Off];
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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PowerPlanSettings {
    pub power_save_guid: Option<String>,
    pub performance_guid: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ByActivitySettings {
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
    #[serde(default = "default_true")]
    pub keyboard: bool,
    #[serde(default = "default_true")]
    pub mouse: bool,
    #[serde(default = "default_true")]
    pub controller: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ByForegroundSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub rules: Vec<ByForegroundRule>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ByForegroundRule {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub process_name: String,
    #[serde(default)]
    pub power_plan_guid: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ByTimeSettings {
    pub enabled: bool,
    pub rules: Vec<ByTimeRule>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ByTimeRule {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub name: String,
    pub days: Vec<WeekdaySetting>,
    pub start_time: String,
    pub end_time: String,
    #[serde(default)]
    pub power_plan_guid: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ByCpuLoadSettings {
    pub enabled: bool,
    pub rules: Vec<ByCpuLoadRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundEfficiencySettings {
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub exclude_foreground_app: bool,
    #[serde(default)]
    pub aggressiveness: BackgroundEfficiencyAggressiveness,
    #[serde(default)]
    pub custom_rules: Vec<BackgroundEfficiencyRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdaptiveEngineSettings {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub processor_policy_enabled: bool,
    #[serde(default = "default_adaptive_engine_processor_policy_values")]
    pub processor_policy_values: ProcessorPowerValues,
}

impl Default for AdaptiveEngineSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            processor_policy_enabled: true,
            processor_policy_values: default_adaptive_engine_processor_policy_values(),
        }
    }
}

fn default_adaptive_engine_processor_policy_values() -> ProcessorPowerValues {
    ProcessorPowerValues::new_with_boost_mode(0, 5, 45, 0, ProcessorBoostMode::Disabled)
        .normalized()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundEfficiencyRule {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub process_name: String,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BackgroundEfficiencyAggressiveness {
    #[default]
    Safe,
    Balanced,
    Aggressive,
}

impl BackgroundEfficiencyAggressiveness {
    pub const ALL: [Self; 3] = [Self::Safe, Self::Balanced, Self::Aggressive];
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CpuRestrictionMode {
    #[default]
    SoftCpuSets,
    HardAffinity,
}

impl CpuRestrictionMode {
    pub const ALL: [Self; 2] = [Self::SoftCpuSets, Self::HardAffinity];
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CpuRestrictionStrategy {
    Off,
    #[default]
    Auto,
    PreferEfficiencyCores,
    LimitLogicalCpus,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CpuRestrictionControlStyle {
    #[default]
    Percentage,
    CoreToggle,
}

impl CpuRestrictionControlStyle {
    pub const ALL: [Self; 2] = [Self::Percentage, Self::CoreToggle];
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
    #[serde(default)]
    pub suspendable_apps: Vec<AppSuspensionRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoreSteeringSettings {
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub exclude_foreground_app: bool,
    #[serde(default)]
    pub rules: Vec<CoreSteeringRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BackgroundCpuRestrictionSettings {
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub exclude_foreground_app: bool,
    #[serde(default)]
    pub mode: CpuRestrictionMode,
    #[serde(default)]
    pub strategy: CpuRestrictionStrategy,
    #[serde(default)]
    pub control_style: CpuRestrictionControlStyle,
    #[serde(default = "default_cpu_restriction_percent")]
    pub percent: u8,
    #[serde(default = "default_cpu_restriction_max_logical_processors")]
    pub max_logical_processors: u8,
    #[serde(default)]
    pub core_mask: u64,
    #[serde(default)]
    pub exclusions: Vec<ProcessExclusionRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessExclusionRule {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub process_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process_foreground_priority: Option<ProcessPrioritySetting>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process_background_priority: Option<ProcessPrioritySetting>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_foreground_priority: Option<ProcessThreadPrioritySetting>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_background_priority: Option<ProcessThreadPrioritySetting>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dynamic_priority_boost_foreground: Option<ProcessDynamicPriorityBoostSetting>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dynamic_priority_boost_background: Option<ProcessDynamicPriorityBoostSetting>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub io_foreground_priority: Option<ProcessIoPrioritySetting>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub io_background_priority: Option<ProcessIoPrioritySetting>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_foreground_priority: Option<ProcessGpuPrioritySetting>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_background_priority: Option<ProcessGpuPrioritySetting>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_foreground_priority: Option<ProcessMemoryPrioritySetting>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_background_priority: Option<ProcessMemoryPrioritySetting>,
}

impl Default for ProcessExclusionRule {
    fn default() -> Self {
        Self {
            enabled: true,
            process_name: String::new(),
            process_foreground_priority: None,
            process_background_priority: None,
            thread_foreground_priority: None,
            thread_background_priority: None,
            dynamic_priority_boost_foreground: None,
            dynamic_priority_boost_background: None,
            io_foreground_priority: None,
            io_background_priority: None,
            gpu_foreground_priority: None,
            gpu_background_priority: None,
            memory_foreground_priority: None,
            memory_background_priority: None,
        }
    }
}

impl ProcessExclusionRule {
    pub fn process_priority_override(&self, foreground: bool) -> ProcessPrioritySetting {
        if foreground {
            self.process_foreground_priority.unwrap_or_default()
        } else {
            self.process_background_priority.unwrap_or_default()
        }
    }

    pub fn set_process_priority_override(
        &mut self,
        foreground: bool,
        priority: ProcessPrioritySetting,
    ) {
        set_optional_default(
            if foreground {
                &mut self.process_foreground_priority
            } else {
                &mut self.process_background_priority
            },
            priority,
        );
    }

    pub fn thread_priority_override(&self, foreground: bool) -> ProcessThreadPrioritySetting {
        if foreground {
            self.thread_foreground_priority.unwrap_or_default()
        } else {
            self.thread_background_priority.unwrap_or_default()
        }
    }

    pub fn set_thread_priority_override(
        &mut self,
        foreground: bool,
        priority: ProcessThreadPrioritySetting,
    ) {
        set_optional_default(
            if foreground {
                &mut self.thread_foreground_priority
            } else {
                &mut self.thread_background_priority
            },
            priority,
        );
    }

    pub fn dynamic_priority_boost_override(
        &self,
        foreground: bool,
    ) -> ProcessDynamicPriorityBoostSetting {
        if foreground {
            self.dynamic_priority_boost_foreground.unwrap_or_default()
        } else {
            self.dynamic_priority_boost_background.unwrap_or_default()
        }
    }

    pub fn set_dynamic_priority_boost_override(
        &mut self,
        foreground: bool,
        boost: ProcessDynamicPriorityBoostSetting,
    ) {
        set_optional_default(
            if foreground {
                &mut self.dynamic_priority_boost_foreground
            } else {
                &mut self.dynamic_priority_boost_background
            },
            boost,
        );
    }

    pub fn io_priority_override(&self, foreground: bool) -> ProcessIoPrioritySetting {
        if foreground {
            self.io_foreground_priority.unwrap_or_default()
        } else {
            self.io_background_priority.unwrap_or_default()
        }
    }

    pub fn set_io_priority_override(
        &mut self,
        foreground: bool,
        priority: ProcessIoPrioritySetting,
    ) {
        set_optional_default(
            if foreground {
                &mut self.io_foreground_priority
            } else {
                &mut self.io_background_priority
            },
            priority,
        );
    }

    pub fn gpu_priority_override(&self, foreground: bool) -> ProcessGpuPrioritySetting {
        if foreground {
            self.gpu_foreground_priority.unwrap_or_default()
        } else {
            self.gpu_background_priority.unwrap_or_default()
        }
    }

    pub fn set_gpu_priority_override(
        &mut self,
        foreground: bool,
        priority: ProcessGpuPrioritySetting,
    ) {
        set_optional_default(
            if foreground {
                &mut self.gpu_foreground_priority
            } else {
                &mut self.gpu_background_priority
            },
            priority,
        );
    }

    pub fn memory_priority_override(&self, foreground: bool) -> ProcessMemoryPrioritySetting {
        if foreground {
            self.memory_foreground_priority.unwrap_or_default()
        } else {
            self.memory_background_priority.unwrap_or_default()
        }
    }

    pub fn set_memory_priority_override(
        &mut self,
        foreground: bool,
        priority: ProcessMemoryPrioritySetting,
    ) {
        set_optional_default(
            if foreground {
                &mut self.memory_foreground_priority
            } else {
                &mut self.memory_background_priority
            },
            priority,
        );
    }
}

fn set_optional_default<T>(target: &mut Option<T>, value: T)
where
    T: Default + PartialEq,
{
    *target = (value != T::default()).then_some(value);
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoreSteeringRule {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub mode: CoreSteeringMode,
    pub process_name: String,
    pub core_mask: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoreLimiterSettings {
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub exclude_foreground_app: bool,
    #[serde(default)]
    pub rules: Vec<CoreLimiterRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoreLimiterRule {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub process_name: String,
    #[serde(default = "default_core_limiter_threshold_percent")]
    pub threshold_percent: u8,
    #[serde(default = "default_core_limiter_sustain_seconds")]
    pub sustain_seconds: u64,
    #[serde(default = "default_core_limiter_cooldown_seconds")]
    pub cooldown_seconds: u64,
    #[serde(default = "default_core_limiter_max_logical_processors")]
    pub max_logical_processors: u8,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ByRunningAppSettings {
    pub enabled: bool,
    #[serde(default)]
    pub rules: Vec<ByRunningAppRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ByRunningAppRule {
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub name: String,
    pub process_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub power_plan_guid: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkloadEngineSettings {
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub lower_background_apps: bool,
    #[serde(default = "default_true")]
    pub workload_engine_background_efficiency_enabled: bool,
    #[serde(default = "default_workload_engine_background_priority")]
    pub workload_engine_background_priority: ProcessPriority,
    #[serde(default)]
    pub lower_background_io_priority_enabled: bool,
    #[serde(default)]
    pub lower_background_io_priority: ProcessIoPriority,
    #[serde(default = "default_workload_engine_io_priority_settings")]
    pub workload_engine_io_priority: IoPrioritySettings,
    #[serde(default = "default_workload_engine_thread_priority_settings")]
    pub workload_engine_thread_priority: ThreadPrioritySettings,
    #[serde(default = "default_workload_engine_dynamic_priority_boost_settings")]
    pub workload_engine_dynamic_priority_boost: DynamicPriorityBoostSettings,
    #[serde(default = "default_workload_engine_gpu_priority_settings")]
    pub workload_engine_gpu_priority: GpuPrioritySettings,
    #[serde(default)]
    pub workload_engine_memory_priority_enabled: bool,
    #[serde(default = "default_workload_engine_foreground_memory_priority")]
    pub workload_engine_foreground_memory_priority: ProcessMemoryPrioritySetting,
    #[serde(default)]
    pub workload_engine_memory_priority: ProcessMemoryPriority,
    #[serde(default = "default_true")]
    pub lower_background_auto_cpu_percent: bool,
    #[serde(default)]
    pub workload_engine_enabled: bool,
    #[serde(default)]
    pub workload_engine_advanced_settings_enabled: bool,
    #[serde(default)]
    pub workload_engine_affinity_escalation_enabled: bool,
    #[serde(default)]
    pub workload_engine_affinity_mode: CpuRestrictionMode,
    #[serde(default = "default_workload_engine_cpu_percent")]
    pub workload_engine_cpu_percent: u8,
    #[serde(default = "default_cpu_restriction_max_logical_processors")]
    pub workload_engine_max_logical_processors: u8,
    #[serde(default = "default_workload_engine_total_threshold_percent")]
    pub workload_engine_total_threshold_percent: u8,
    #[serde(default = "default_workload_engine_threshold_percent")]
    pub workload_engine_threshold_percent: u8,
    #[serde(default = "default_workload_engine_restore_threshold_percent")]
    pub workload_engine_restore_threshold_percent: u8,
    #[serde(default = "default_workload_engine_sustain_seconds")]
    pub workload_engine_sustain_seconds: u64,
    #[serde(default = "default_workload_engine_minimum_restraint_seconds")]
    pub workload_engine_minimum_restraint_seconds: u64,
    #[serde(default = "default_workload_engine_cooldown_seconds")]
    pub workload_engine_cooldown_seconds: u64,
    #[serde(default = "default_workload_engine_max_targeted_processes")]
    pub workload_engine_max_targeted_processes: u8,
    #[serde(default)]
    pub workload_engine_exclusions: Vec<ProcessExclusionRule>,
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
pub struct IoPrioritySettings {
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub foreground_detection_enabled: bool,
    #[serde(default = "default_io_priority_foreground")]
    pub foreground_priority: ProcessIoPrioritySetting,
    #[serde(default = "default_io_priority_background")]
    pub background_priority: ProcessIoPrioritySetting,
    #[serde(default = "default_true")]
    pub preserve_foreground_priority: bool,
    #[serde(default = "default_true")]
    pub preserve_background_priority: bool,
    #[serde(default)]
    pub exclusions: Vec<ProcessExclusionRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessPrioritySettings {
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub foreground_detection_enabled: bool,
    #[serde(default = "default_process_priority_foreground")]
    pub foreground_priority: ProcessPrioritySetting,
    #[serde(default = "default_process_priority_background")]
    pub background_priority: ProcessPrioritySetting,
    #[serde(default = "default_true")]
    pub preserve_foreground_priority: bool,
    #[serde(default = "default_true")]
    pub preserve_background_priority: bool,
    #[serde(default)]
    pub exclusions: Vec<ProcessExclusionRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadPrioritySettings {
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub foreground_detection_enabled: bool,
    #[serde(default = "default_thread_priority_foreground")]
    pub foreground_priority: ProcessThreadPrioritySetting,
    #[serde(default = "default_thread_priority_background")]
    pub background_priority: ProcessThreadPrioritySetting,
    #[serde(default = "default_true")]
    pub preserve_foreground_priority: bool,
    #[serde(default = "default_true")]
    pub preserve_background_priority: bool,
    #[serde(default)]
    pub exclusions: Vec<ProcessExclusionRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DynamicPriorityBoostSettings {
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub foreground_detection_enabled: bool,
    #[serde(default = "default_dynamic_priority_boost_foreground")]
    pub foreground_boost: ProcessDynamicPriorityBoostSetting,
    #[serde(default = "default_dynamic_priority_boost_background")]
    pub background_boost: ProcessDynamicPriorityBoostSetting,
    #[serde(default)]
    pub exclusions: Vec<ProcessExclusionRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GpuPrioritySettings {
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub foreground_detection_enabled: bool,
    #[serde(default = "default_gpu_priority_foreground")]
    pub foreground_priority: ProcessGpuPrioritySetting,
    #[serde(default = "default_gpu_priority_background")]
    pub background_priority: ProcessGpuPrioritySetting,
    #[serde(default = "default_true")]
    pub preserve_foreground_priority: bool,
    #[serde(default = "default_true")]
    pub preserve_background_priority: bool,
    #[serde(default)]
    pub exclusions: Vec<ProcessExclusionRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryPrioritySettings {
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub foreground_detection_enabled: bool,
    #[serde(default = "default_memory_priority_foreground")]
    pub foreground_priority: ProcessMemoryPrioritySetting,
    #[serde(default = "default_memory_priority_background")]
    pub background_priority: ProcessMemoryPrioritySetting,
    #[serde(default = "default_true")]
    pub preserve_foreground_priority: bool,
    #[serde(default = "default_true")]
    pub preserve_background_priority: bool,
    #[serde(default)]
    pub exclusions: Vec<ProcessExclusionRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimerResolutionSettings {
    pub enabled: bool,
    #[serde(default = "default_timer_resolution_100ns")]
    pub desired_100ns: u32,
    #[serde(default)]
    pub rules: Vec<TimerResolutionRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryTrimSettings {
    pub enabled: bool,
    #[serde(default = "default_memory_trim_check_interval_minutes")]
    pub check_interval_minutes: u64,
    #[serde(default = "default_true")]
    pub exclude_foreground_app: bool,
    #[serde(default = "default_true")]
    pub trim_working_sets: bool,
    #[serde(default = "default_memory_trim_system_memory_load_threshold_percent")]
    pub system_memory_load_threshold_percent: u8,
    #[serde(default = "default_memory_trim_process_working_set_threshold_mb")]
    pub process_working_set_threshold_mb: u64,
    #[serde(default = "default_memory_trim_process_cpu_idle_threshold_percent")]
    pub process_cpu_idle_threshold_percent: u8,
    #[serde(default = "default_memory_trim_process_idle_seconds")]
    pub process_idle_seconds: u64,
    #[serde(default = "default_memory_trim_cooldown_seconds")]
    pub trim_cooldown_seconds: u64,
    #[serde(default)]
    pub purge_standby_list: bool,
    #[serde(default)]
    pub purge_system_file_cache: bool,
    #[serde(default = "default_true")]
    pub purge_only_in_performance_mode: bool,
    #[serde(default = "default_memory_trim_purge_free_ram_threshold_mb")]
    pub purge_free_ram_threshold_mb: u64,
    #[serde(default)]
    pub exclusions: Vec<ProcessExclusionRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimerResolutionRule {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub process_name: String,
    #[serde(default = "default_timer_resolution_100ns")]
    pub desired_100ns: u32,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessIoPriority {
    Critical,
    High,
    Normal,
    Low,
    #[default]
    VeryLow,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessIoPrioritySetting {
    #[default]
    Default,
    Auto,
    Critical,
    High,
    Normal,
    Low,
    VeryLow,
}

impl ProcessIoPrioritySetting {
    pub const ALL: [Self; 4] = [Self::Default, Self::VeryLow, Self::Low, Self::Normal];
    pub const CUSTOM_RULE_ALL: [Self; 5] = [
        Self::Default,
        Self::Auto,
        Self::VeryLow,
        Self::Low,
        Self::Normal,
    ];
    pub const ADVANCED_ALL: [Self; 6] = [
        Self::Default,
        Self::VeryLow,
        Self::Low,
        Self::Normal,
        Self::High,
        Self::Critical,
    ];
    pub const CUSTOM_RULE_ADVANCED_ALL: [Self; 7] = [
        Self::Default,
        Self::Auto,
        Self::VeryLow,
        Self::Low,
        Self::Normal,
        Self::High,
        Self::Critical,
    ];

    pub const fn priority(self) -> Option<ProcessIoPriority> {
        match self {
            Self::Default | Self::Auto => None,
            Self::Critical => Some(ProcessIoPriority::Critical),
            Self::High => Some(ProcessIoPriority::High),
            Self::Normal => Some(ProcessIoPriority::Normal),
            Self::Low => Some(ProcessIoPriority::Low),
            Self::VeryLow => Some(ProcessIoPriority::VeryLow),
        }
    }

    pub const fn safe_when_advanced_disabled(self) -> Self {
        match self {
            Self::Critical | Self::High => Self::Normal,
            _ => self,
        }
    }
}

impl From<ProcessIoPriority> for ProcessIoPrioritySetting {
    fn from(priority: ProcessIoPriority) -> Self {
        match priority {
            ProcessIoPriority::Critical => Self::Critical,
            ProcessIoPriority::High => Self::High,
            ProcessIoPriority::Normal => Self::Normal,
            ProcessIoPriority::Low => Self::Low,
            ProcessIoPriority::VeryLow => Self::VeryLow,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessGpuPriority {
    Realtime,
    High,
    AboveNormal,
    Normal,
    #[default]
    BelowNormal,
    Idle,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessGpuPrioritySetting {
    #[default]
    Default,
    Auto,
    Realtime,
    High,
    AboveNormal,
    Normal,
    BelowNormal,
    Idle,
}

impl ProcessGpuPrioritySetting {
    pub const ALL: [Self; 5] = [
        Self::Default,
        Self::Idle,
        Self::BelowNormal,
        Self::Normal,
        Self::AboveNormal,
    ];
    pub const CUSTOM_RULE_ALL: [Self; 6] = [
        Self::Default,
        Self::Auto,
        Self::Idle,
        Self::BelowNormal,
        Self::Normal,
        Self::AboveNormal,
    ];
    pub const ADVANCED_ALL: [Self; 7] = [
        Self::Default,
        Self::Idle,
        Self::BelowNormal,
        Self::Normal,
        Self::AboveNormal,
        Self::High,
        Self::Realtime,
    ];
    pub const CUSTOM_RULE_ADVANCED_ALL: [Self; 8] = [
        Self::Default,
        Self::Auto,
        Self::Idle,
        Self::BelowNormal,
        Self::Normal,
        Self::AboveNormal,
        Self::High,
        Self::Realtime,
    ];

    pub const fn priority(self) -> Option<ProcessGpuPriority> {
        match self {
            Self::Default | Self::Auto => None,
            Self::Realtime => Some(ProcessGpuPriority::Realtime),
            Self::High => Some(ProcessGpuPriority::High),
            Self::AboveNormal => Some(ProcessGpuPriority::AboveNormal),
            Self::Normal => Some(ProcessGpuPriority::Normal),
            Self::BelowNormal => Some(ProcessGpuPriority::BelowNormal),
            Self::Idle => Some(ProcessGpuPriority::Idle),
        }
    }

    pub const fn safe_when_advanced_disabled(self) -> Self {
        match self {
            Self::Realtime | Self::High => Self::AboveNormal,
            _ => self,
        }
    }
}

impl From<ProcessGpuPriority> for ProcessGpuPrioritySetting {
    fn from(priority: ProcessGpuPriority) -> Self {
        match priority {
            ProcessGpuPriority::Realtime => Self::Realtime,
            ProcessGpuPriority::High => Self::High,
            ProcessGpuPriority::AboveNormal => Self::AboveNormal,
            ProcessGpuPriority::Normal => Self::Normal,
            ProcessGpuPriority::BelowNormal => Self::BelowNormal,
            ProcessGpuPriority::Idle => Self::Idle,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessMemoryPriority {
    VeryLow,
    #[default]
    Low,
    Medium,
    BelowNormal,
    Normal,
}

impl ProcessMemoryPriority {
    pub const ALL: [Self; 5] = [
        Self::VeryLow,
        Self::Low,
        Self::Medium,
        Self::BelowNormal,
        Self::Normal,
    ];
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessMemoryPrioritySetting {
    #[default]
    Default,
    Auto,
    VeryLow,
    Low,
    Medium,
    BelowNormal,
    Normal,
}

impl ProcessMemoryPrioritySetting {
    pub const ALL: [Self; 6] = [
        Self::Default,
        Self::VeryLow,
        Self::Low,
        Self::Medium,
        Self::BelowNormal,
        Self::Normal,
    ];
    pub const CUSTOM_RULE_ALL: [Self; 7] = [
        Self::Default,
        Self::Auto,
        Self::VeryLow,
        Self::Low,
        Self::Medium,
        Self::BelowNormal,
        Self::Normal,
    ];

    pub const fn priority(self) -> Option<ProcessMemoryPriority> {
        match self {
            Self::Default | Self::Auto => None,
            Self::VeryLow => Some(ProcessMemoryPriority::VeryLow),
            Self::Low => Some(ProcessMemoryPriority::Low),
            Self::Medium => Some(ProcessMemoryPriority::Medium),
            Self::BelowNormal => Some(ProcessMemoryPriority::BelowNormal),
            Self::Normal => Some(ProcessMemoryPriority::Normal),
        }
    }
}

impl From<ProcessMemoryPriority> for ProcessMemoryPrioritySetting {
    fn from(priority: ProcessMemoryPriority) -> Self {
        match priority {
            ProcessMemoryPriority::VeryLow => Self::VeryLow,
            ProcessMemoryPriority::Low => Self::Low,
            ProcessMemoryPriority::Medium => Self::Medium,
            ProcessMemoryPriority::BelowNormal => Self::BelowNormal,
            ProcessMemoryPriority::Normal => Self::Normal,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessPrioritySetting {
    #[default]
    Default,
    Auto,
    Realtime,
    High,
    AboveNormal,
    Normal,
    BelowNormal,
    Idle,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessThreadPrioritySetting {
    #[default]
    Default,
    Auto,
    TimeCritical,
    Highest,
    AboveNormal,
    Normal,
    BelowNormal,
    Lowest,
    Idle,
}

impl ProcessThreadPrioritySetting {
    pub const ALL: [Self; 7] = [
        Self::Default,
        Self::Idle,
        Self::Lowest,
        Self::BelowNormal,
        Self::Normal,
        Self::AboveNormal,
        Self::Highest,
    ];
    pub const CUSTOM_RULE_ALL: [Self; 8] = [
        Self::Default,
        Self::Auto,
        Self::Idle,
        Self::Lowest,
        Self::BelowNormal,
        Self::Normal,
        Self::AboveNormal,
        Self::Highest,
    ];
    pub const ADVANCED_ALL: [Self; 8] = [
        Self::Default,
        Self::Idle,
        Self::Lowest,
        Self::BelowNormal,
        Self::Normal,
        Self::AboveNormal,
        Self::Highest,
        Self::TimeCritical,
    ];
    pub const CUSTOM_RULE_ADVANCED_ALL: [Self; 9] = [
        Self::Default,
        Self::Auto,
        Self::Idle,
        Self::Lowest,
        Self::BelowNormal,
        Self::Normal,
        Self::AboveNormal,
        Self::Highest,
        Self::TimeCritical,
    ];

    pub const fn safe_when_advanced_disabled(self) -> Self {
        match self {
            Self::TimeCritical => Self::Highest,
            _ => self,
        }
    }
}

impl ProcessPrioritySetting {
    pub const ALL: [Self; 6] = [
        Self::Default,
        Self::Idle,
        Self::BelowNormal,
        Self::Normal,
        Self::AboveNormal,
        Self::High,
    ];
    pub const CUSTOM_RULE_ALL: [Self; 7] = [
        Self::Default,
        Self::Auto,
        Self::Idle,
        Self::BelowNormal,
        Self::Normal,
        Self::AboveNormal,
        Self::High,
    ];

    pub const ADVANCED_ALL: [Self; 7] = [
        Self::Default,
        Self::Idle,
        Self::BelowNormal,
        Self::Normal,
        Self::AboveNormal,
        Self::High,
        Self::Realtime,
    ];
    pub const CUSTOM_RULE_ADVANCED_ALL: [Self; 8] = [
        Self::Default,
        Self::Auto,
        Self::Idle,
        Self::BelowNormal,
        Self::Normal,
        Self::AboveNormal,
        Self::High,
        Self::Realtime,
    ];

    pub const fn safe_when_advanced_disabled(self) -> Self {
        match self {
            Self::Realtime => Self::High,
            _ => self,
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessDynamicPriorityBoostSetting {
    #[default]
    Default,
    Auto,
    Enabled,
    Disabled,
}

impl ProcessDynamicPriorityBoostSetting {
    pub const ALL: [Self; 3] = [Self::Default, Self::Enabled, Self::Disabled];
    pub const CUSTOM_RULE_ALL: [Self; 4] =
        [Self::Default, Self::Auto, Self::Enabled, Self::Disabled];

    pub const fn disabled_flag(self) -> Option<bool> {
        match self {
            Self::Default | Self::Auto => None,
            Self::Enabled => Some(false),
            Self::Disabled => Some(true),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PriorityRule {
    #[serde(default = "default_true")]
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ForegroundBoostPriority {
    #[default]
    Auto,
    Normal,
    AboveNormal,
}

impl ForegroundBoostPriority {
    pub const ALL: [Self; 3] = [Self::Auto, Self::Normal, Self::AboveNormal];
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CoreSteeringMode {
    #[default]
    Hard,
    Soft,
    EfficiencyOff,
}

impl CoreSteeringMode {
    pub const ALL: [Self; 3] = [Self::Hard, Self::Soft, Self::EfficiencyOff];
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppSuspensionRule {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub process_name: String,
    #[serde(default = "default_true")]
    pub network_wake_enabled: bool,
    #[serde(default = "default_true")]
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkThresholdUnit {
    #[default]
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
pub struct ByCpuLoadRule {
    #[serde(default = "default_true")]
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
                check_for_updates: true,
                update_channel: UpdateChannel::PreRelease,
                theme_mode: AppThemeMode::System,
                accent: AccentSettings::default(),
                language: AppLanguage::English,
                animation_mode: AnimationMode::System,
                pause_power_plan_switching_while_plugged_in: false,
                check_interval_ms: 1000,
            },
            advanced: AdvancedSettings::default(),
            adaptive_engine: AdaptiveEngineSettings::default(),
            by_activity: ByActivitySettings {
                enabled: true,
                idle_timeout_seconds: 300,
                switch_to_performance_on_resume: true,
                input_detection: InputDetectionSettings::default(),
                power_plans: PowerPlanSettings::default(),
            },
            by_foreground: ByForegroundSettings::default(),
            by_time: ByTimeSettings {
                enabled: false,
                rules: vec![ByTimeRule {
                    enabled: true,
                    name: "Night Idle Plan".to_owned(),
                    days: WeekdaySetting::all().to_vec(),
                    start_time: "22:00".to_owned(),
                    end_time: "08:00".to_owned(),
                    power_plan_guid: None,
                }],
            },
            by_cpu_load: ByCpuLoadSettings::default(),
            background_efficiency: BackgroundEfficiencySettings::default(),
            app_suspension: AppSuspensionSettings::default(),
            core_steering: CoreSteeringSettings::default(),
            background_cpu_restriction: BackgroundCpuRestrictionSettings::default(),
            core_limiter: CoreLimiterSettings::default(),
            by_running_app: ByRunningAppSettings::default(),
            workload_engine: WorkloadEngineSettings::default(),
            process_priority: ProcessPrioritySettings::default(),
            thread_priority: ThreadPrioritySettings::default(),
            dynamic_priority_boost: DynamicPriorityBoostSettings::default(),
            io_priority: IoPrioritySettings::default(),
            gpu_priority: GpuPrioritySettings::default(),
            memory_priority: MemoryPrioritySettings::default(),
            memory_trim: MemoryTrimSettings::default(),
            timer_resolution: TimerResolutionSettings::default(),
        }
    }
}

impl Default for AdvancedSettings {
    fn default() -> Self {
        Self {
            action_log_mode: ActionLogMode::Full,
            execution_failure_suppression_threshold:
                default_execution_failure_suppression_threshold(),
            expose_all_priority_values: false,
            show_advanced_controls: false,
        }
    }
}

fn default_execution_failure_suppression_threshold() -> u8 {
    DEFAULT_EXECUTION_FAILURE_SUPPRESSION_THRESHOLD
}

impl Default for InputDetectionSettings {
    fn default() -> Self {
        Self {
            keyboard: true,
            mouse: true,
            controller: true,
        }
    }
}

const fn default_true() -> bool {
    true
}

impl Default for ByForegroundSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            rules: Vec::new(),
        }
    }
}

impl Default for ByCpuLoadSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            rules: vec![
                ByCpuLoadRule {
                    enabled: true,
                    name: "Low CPU Idle".to_owned(),
                    comparison: CpuUsageComparison::AtOrBelow,
                    threshold_percent: 15,
                    upper_threshold_percent: None,
                    duration_seconds: 60,
                    power_plan_guid: None,
                    else_enabled: false,
                    else_power_plan_guid: None,
                },
                ByCpuLoadRule {
                    enabled: true,
                    name: "High CPU Active".to_owned(),
                    comparison: CpuUsageComparison::AtOrAbove,
                    threshold_percent: 50,
                    upper_threshold_percent: None,
                    duration_seconds: 10,
                    power_plan_guid: None,
                    else_enabled: false,
                    else_power_plan_guid: None,
                },
            ],
        }
    }
}

impl Default for BackgroundEfficiencySettings {
    fn default() -> Self {
        Self {
            enabled: false,
            exclude_foreground_app: default_true(),
            aggressiveness: BackgroundEfficiencyAggressiveness::Safe,
            custom_rules: Vec::new(),
        }
    }
}

const fn default_io_priority_foreground() -> ProcessIoPrioritySetting {
    ProcessIoPrioritySetting::Normal
}

const fn default_io_priority_background() -> ProcessIoPrioritySetting {
    ProcessIoPrioritySetting::VeryLow
}

const fn default_process_priority_foreground() -> ProcessPrioritySetting {
    ProcessPrioritySetting::Default
}

const fn default_process_priority_background() -> ProcessPrioritySetting {
    ProcessPrioritySetting::BelowNormal
}

const fn default_thread_priority_foreground() -> ProcessThreadPrioritySetting {
    ProcessThreadPrioritySetting::Default
}

const fn default_thread_priority_background() -> ProcessThreadPrioritySetting {
    ProcessThreadPrioritySetting::BelowNormal
}

const fn default_dynamic_priority_boost_foreground() -> ProcessDynamicPriorityBoostSetting {
    ProcessDynamicPriorityBoostSetting::Default
}

const fn default_dynamic_priority_boost_background() -> ProcessDynamicPriorityBoostSetting {
    ProcessDynamicPriorityBoostSetting::Disabled
}

const fn default_gpu_priority_foreground() -> ProcessGpuPrioritySetting {
    ProcessGpuPrioritySetting::AboveNormal
}

const fn default_gpu_priority_background() -> ProcessGpuPrioritySetting {
    ProcessGpuPrioritySetting::BelowNormal
}

const fn default_memory_priority_foreground() -> ProcessMemoryPrioritySetting {
    ProcessMemoryPrioritySetting::Default
}

const fn default_memory_priority_background() -> ProcessMemoryPrioritySetting {
    ProcessMemoryPrioritySetting::Low
}

const fn default_workload_engine_threshold_percent() -> u8 {
    30
}

const fn default_workload_engine_restore_threshold_percent() -> u8 {
    10
}

const fn default_workload_engine_total_threshold_percent() -> u8 {
    75
}

const fn default_workload_engine_cpu_percent() -> u8 {
    75
}

const fn default_workload_engine_sustain_seconds() -> u64 {
    3
}

const fn default_workload_engine_minimum_restraint_seconds() -> u64 {
    3
}

const fn default_workload_engine_cooldown_seconds() -> u64 {
    6
}

const fn default_workload_engine_max_targeted_processes() -> u8 {
    6
}

const fn default_workload_engine_background_priority() -> ProcessPriority {
    ProcessPriority::BelowNormal
}

const fn default_workload_engine_foreground_memory_priority() -> ProcessMemoryPrioritySetting {
    ProcessMemoryPrioritySetting::Default
}

fn default_workload_engine_io_priority_settings() -> IoPrioritySettings {
    IoPrioritySettings {
        enabled: false,
        foreground_detection_enabled: true,
        foreground_priority: ProcessIoPrioritySetting::Normal,
        background_priority: ProcessIoPrioritySetting::VeryLow,
        preserve_foreground_priority: true,
        preserve_background_priority: true,
        exclusions: Vec::new(),
    }
}

fn default_workload_engine_thread_priority_settings() -> ThreadPrioritySettings {
    ThreadPrioritySettings {
        enabled: true,
        foreground_detection_enabled: true,
        foreground_priority: ProcessThreadPrioritySetting::Default,
        background_priority: ProcessThreadPrioritySetting::BelowNormal,
        preserve_foreground_priority: true,
        preserve_background_priority: true,
        exclusions: Vec::new(),
    }
}

fn default_workload_engine_dynamic_priority_boost_settings() -> DynamicPriorityBoostSettings {
    DynamicPriorityBoostSettings {
        enabled: true,
        foreground_detection_enabled: true,
        foreground_boost: ProcessDynamicPriorityBoostSetting::Enabled,
        background_boost: ProcessDynamicPriorityBoostSetting::Disabled,
        exclusions: Vec::new(),
    }
}

fn default_workload_engine_gpu_priority_settings() -> GpuPrioritySettings {
    GpuPrioritySettings {
        enabled: true,
        foreground_detection_enabled: true,
        foreground_priority: ProcessGpuPrioritySetting::Default,
        background_priority: ProcessGpuPrioritySetting::BelowNormal,
        preserve_foreground_priority: true,
        preserve_background_priority: true,
        exclusions: Vec::new(),
    }
}

const fn default_foreground_stability_delay_ms() -> u64 {
    750
}

const fn default_core_limiter_threshold_percent() -> u8 {
    75
}

const fn default_core_limiter_sustain_seconds() -> u64 {
    5
}

const fn default_core_limiter_cooldown_seconds() -> u64 {
    10
}

const fn default_core_limiter_max_logical_processors() -> u8 {
    1
}

const fn default_memory_trim_system_memory_load_threshold_percent() -> u8 {
    65
}

const fn default_memory_trim_process_working_set_threshold_mb() -> u64 {
    196
}

const fn default_memory_trim_process_cpu_idle_threshold_percent() -> u8 {
    1
}

const fn default_memory_trim_process_idle_seconds() -> u64 {
    300
}

const fn default_memory_trim_cooldown_seconds() -> u64 {
    900
}

const fn default_memory_trim_check_interval_minutes() -> u64 {
    15
}

const fn default_memory_trim_purge_free_ram_threshold_mb() -> u64 {
    1024
}

const fn default_timer_resolution_100ns() -> u32 {
    10_000
}

const fn default_cpu_restriction_percent() -> u8 {
    50
}

const fn default_cpu_restriction_max_logical_processors() -> u8 {
    0
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

const fn default_rule_network_download_threshold_bytes() -> u64 {
    1
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

impl Default for CoreSteeringSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            exclude_foreground_app: default_true(),
            rules: Vec::new(),
        }
    }
}

impl Default for BackgroundCpuRestrictionSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            exclude_foreground_app: default_true(),
            mode: CpuRestrictionMode::HardAffinity,
            strategy: CpuRestrictionStrategy::Auto,
            control_style: CpuRestrictionControlStyle::Percentage,
            percent: default_cpu_restriction_percent(),
            max_logical_processors: default_cpu_restriction_max_logical_processors(),
            core_mask: 0,
            exclusions: Vec::new(),
        }
    }
}

impl Default for CoreLimiterSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            exclude_foreground_app: default_true(),
            rules: Vec::new(),
        }
    }
}

impl Default for WorkloadEngineSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            lower_background_apps: default_true(),
            workload_engine_background_efficiency_enabled: default_true(),
            workload_engine_background_priority: default_workload_engine_background_priority(),
            lower_background_io_priority_enabled: false,
            lower_background_io_priority: ProcessIoPriority::VeryLow,
            workload_engine_io_priority: default_workload_engine_io_priority_settings(),
            workload_engine_thread_priority: default_workload_engine_thread_priority_settings(),
            workload_engine_dynamic_priority_boost:
                default_workload_engine_dynamic_priority_boost_settings(),
            workload_engine_gpu_priority: default_workload_engine_gpu_priority_settings(),
            workload_engine_memory_priority_enabled: false,
            workload_engine_foreground_memory_priority:
                default_workload_engine_foreground_memory_priority(),
            workload_engine_memory_priority: ProcessMemoryPriority::Low,
            lower_background_auto_cpu_percent: default_true(),
            workload_engine_enabled: false,
            workload_engine_advanced_settings_enabled: false,
            workload_engine_affinity_escalation_enabled: false,
            workload_engine_affinity_mode: CpuRestrictionMode::SoftCpuSets,
            workload_engine_cpu_percent: default_workload_engine_cpu_percent(),
            workload_engine_max_logical_processors: default_cpu_restriction_max_logical_processors(
            ),
            workload_engine_total_threshold_percent:
                default_workload_engine_total_threshold_percent(),
            workload_engine_threshold_percent: default_workload_engine_threshold_percent(),
            workload_engine_restore_threshold_percent:
                default_workload_engine_restore_threshold_percent(),
            workload_engine_sustain_seconds: default_workload_engine_sustain_seconds(),
            workload_engine_minimum_restraint_seconds:
                default_workload_engine_minimum_restraint_seconds(),
            workload_engine_cooldown_seconds: default_workload_engine_cooldown_seconds(),
            workload_engine_max_targeted_processes: default_workload_engine_max_targeted_processes(
            ),
            workload_engine_exclusions: Vec::new(),
            boost_foreground_app: true,
            foreground_boost: ForegroundBoostPriority::Auto,
            foreground_stability_delay_ms: default_foreground_stability_delay_ms(),
            rules: Vec::new(),
        }
    }
}

impl Default for IoPrioritySettings {
    fn default() -> Self {
        Self {
            enabled: false,
            foreground_detection_enabled: default_true(),
            foreground_priority: default_io_priority_foreground(),
            background_priority: default_io_priority_background(),
            preserve_foreground_priority: true,
            preserve_background_priority: true,
            exclusions: Vec::new(),
        }
    }
}

impl Default for ProcessPrioritySettings {
    fn default() -> Self {
        Self {
            enabled: false,
            foreground_detection_enabled: default_true(),
            foreground_priority: default_process_priority_foreground(),
            background_priority: default_process_priority_background(),
            preserve_foreground_priority: true,
            preserve_background_priority: true,
            exclusions: Vec::new(),
        }
    }
}

impl Default for ThreadPrioritySettings {
    fn default() -> Self {
        Self {
            enabled: false,
            foreground_detection_enabled: default_true(),
            foreground_priority: default_thread_priority_foreground(),
            background_priority: default_thread_priority_background(),
            preserve_foreground_priority: true,
            preserve_background_priority: true,
            exclusions: Vec::new(),
        }
    }
}

impl Default for DynamicPriorityBoostSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            foreground_detection_enabled: default_true(),
            foreground_boost: default_dynamic_priority_boost_foreground(),
            background_boost: default_dynamic_priority_boost_background(),
            exclusions: Vec::new(),
        }
    }
}

impl Default for GpuPrioritySettings {
    fn default() -> Self {
        Self {
            enabled: false,
            foreground_detection_enabled: default_true(),
            foreground_priority: default_gpu_priority_foreground(),
            background_priority: default_gpu_priority_background(),
            preserve_foreground_priority: true,
            preserve_background_priority: true,
            exclusions: Vec::new(),
        }
    }
}

impl Default for MemoryPrioritySettings {
    fn default() -> Self {
        Self {
            enabled: false,
            foreground_detection_enabled: default_true(),
            foreground_priority: default_memory_priority_foreground(),
            background_priority: default_memory_priority_background(),
            preserve_foreground_priority: true,
            preserve_background_priority: true,
            exclusions: Vec::new(),
        }
    }
}

impl Default for TimerResolutionSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            desired_100ns: default_timer_resolution_100ns(),
            rules: Vec::new(),
        }
    }
}

impl Default for MemoryTrimSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            check_interval_minutes: default_memory_trim_check_interval_minutes(),
            exclude_foreground_app: default_true(),
            trim_working_sets: default_true(),
            system_memory_load_threshold_percent:
                default_memory_trim_system_memory_load_threshold_percent(),
            process_working_set_threshold_mb: default_memory_trim_process_working_set_threshold_mb(
            ),
            process_cpu_idle_threshold_percent:
                default_memory_trim_process_cpu_idle_threshold_percent(),
            process_idle_seconds: default_memory_trim_process_idle_seconds(),
            trim_cooldown_seconds: default_memory_trim_cooldown_seconds(),
            purge_standby_list: false,
            purge_system_file_cache: false,
            purge_only_in_performance_mode: default_true(),
            purge_free_ram_threshold_mb: default_memory_trim_purge_free_ram_threshold_mb(),
            exclusions: Vec::new(),
        }
    }
}

impl IoPrioritySettings {
    pub fn contains_exclusion(&self, process_name: &str) -> bool {
        self.exclusions
            .iter()
            .any(|rule| same_process_name(&rule.process_name, process_name))
    }

    pub fn exclusion_enabled_for(&self, process_name: &str) -> bool {
        self.exclusions.iter().any(|rule| {
            process_exclusion_rule_matches(rule, process_name)
                && rule.io_foreground_priority.unwrap_or_default()
                    == ProcessIoPrioritySetting::Default
                && rule.io_background_priority.unwrap_or_default()
                    == ProcessIoPrioritySetting::Default
        })
    }

    pub fn override_for(
        &self,
        process_name: &str,
        foreground: bool,
    ) -> Option<Option<ProcessIoPrioritySetting>> {
        process_custom_rule_override(
            &self.exclusions,
            process_name,
            foreground,
            |rule, foreground| rule.io_priority_override(foreground),
        )
    }
}

impl ProcessPrioritySettings {
    pub fn contains_exclusion(&self, process_name: &str) -> bool {
        self.exclusions
            .iter()
            .any(|rule| same_process_name(&rule.process_name, process_name))
    }

    pub fn override_for(
        &self,
        process_name: &str,
        foreground: bool,
    ) -> Option<Option<ProcessPrioritySetting>> {
        process_custom_rule_override(
            &self.exclusions,
            process_name,
            foreground,
            |rule, foreground| rule.process_priority_override(foreground),
        )
    }
}

impl ThreadPrioritySettings {
    pub fn contains_exclusion(&self, process_name: &str) -> bool {
        self.exclusions
            .iter()
            .any(|rule| same_process_name(&rule.process_name, process_name))
    }

    pub fn override_for(
        &self,
        process_name: &str,
        foreground: bool,
    ) -> Option<Option<ProcessThreadPrioritySetting>> {
        process_custom_rule_override(
            &self.exclusions,
            process_name,
            foreground,
            |rule, foreground| rule.thread_priority_override(foreground),
        )
    }
}

impl DynamicPriorityBoostSettings {
    pub fn contains_exclusion(&self, process_name: &str) -> bool {
        self.exclusions
            .iter()
            .any(|rule| same_process_name(&rule.process_name, process_name))
    }

    pub fn override_for(
        &self,
        process_name: &str,
        foreground: bool,
    ) -> Option<Option<ProcessDynamicPriorityBoostSetting>> {
        process_custom_rule_override(
            &self.exclusions,
            process_name,
            foreground,
            |rule, foreground| rule.dynamic_priority_boost_override(foreground),
        )
    }
}

impl GpuPrioritySettings {
    pub fn contains_exclusion(&self, process_name: &str) -> bool {
        self.exclusions
            .iter()
            .any(|rule| same_process_name(&rule.process_name, process_name))
    }

    pub fn exclusion_enabled_for(&self, process_name: &str) -> bool {
        self.exclusions.iter().any(|rule| {
            process_exclusion_rule_matches(rule, process_name)
                && rule.gpu_foreground_priority.unwrap_or_default()
                    == ProcessGpuPrioritySetting::Default
                && rule.gpu_background_priority.unwrap_or_default()
                    == ProcessGpuPrioritySetting::Default
        })
    }

    pub fn override_for(
        &self,
        process_name: &str,
        foreground: bool,
    ) -> Option<Option<ProcessGpuPrioritySetting>> {
        process_custom_rule_override(
            &self.exclusions,
            process_name,
            foreground,
            |rule, foreground| rule.gpu_priority_override(foreground),
        )
    }
}

impl MemoryPrioritySettings {
    pub fn contains_exclusion(&self, process_name: &str) -> bool {
        self.exclusions
            .iter()
            .any(|rule| same_process_name(&rule.process_name, process_name))
    }

    pub fn exclusion_enabled_for(&self, process_name: &str) -> bool {
        self.exclusions.iter().any(|rule| {
            process_exclusion_rule_matches(rule, process_name)
                && rule.memory_foreground_priority.unwrap_or_default()
                    == ProcessMemoryPrioritySetting::Default
                && rule.memory_background_priority.unwrap_or_default()
                    == ProcessMemoryPrioritySetting::Default
        })
    }

    pub fn override_for(
        &self,
        process_name: &str,
        foreground: bool,
    ) -> Option<Option<ProcessMemoryPrioritySetting>> {
        process_custom_rule_override(
            &self.exclusions,
            process_name,
            foreground,
            |rule, foreground| rule.memory_priority_override(foreground),
        )
    }
}

fn process_exclusion_rule_matches(rule: &ProcessExclusionRule, process_name: &str) -> bool {
    rule.enabled && process_name_matches_pattern(&rule.process_name, process_name)
}

fn process_custom_rule_override<T>(
    rules: &[ProcessExclusionRule],
    process_name: &str,
    foreground: bool,
    value: impl Fn(&ProcessExclusionRule, bool) -> T,
) -> Option<Option<T>>
where
    T: Copy + Default + PartialEq,
{
    rules
        .iter()
        .find(|rule| process_exclusion_rule_matches(rule, process_name))
        .map(|rule| {
            let value = value(rule, foreground);
            (value != T::default()).then_some(value)
        })
}

impl TimerResolutionSettings {
    pub fn desired_resolution_for_foreground(&self, process_name: &str) -> Option<(String, u32)> {
        self.rules
            .iter()
            .find(|rule| {
                rule.enabled
                    && !rule.process_name.trim().is_empty()
                    && process_name_matches_pattern(&rule.process_name, process_name)
            })
            .map(|rule| (rule.process_name.clone(), rule.desired_100ns))
    }

    pub fn contains_rule_for(&self, process_name: &str) -> bool {
        self.rules
            .iter()
            .any(|rule| same_process_name(&rule.process_name, process_name))
    }
}

impl MemoryTrimSettings {
    pub fn exclusion_enabled_for(&self, process_name: &str) -> bool {
        self.exclusions.iter().any(|rule| {
            rule.enabled && process_name_matches_pattern(&rule.process_name, process_name)
        })
    }
}

impl AppSuspensionSettings {
    pub fn contains_suspendable_app(&self, process_name: &str) -> bool {
        self.suspendable_apps
            .iter()
            .any(|rule| same_process_name(&rule.process_name, process_name))
    }

    pub fn suspendable_app_enabled_for(&self, process_name: &str) -> bool {
        self.suspendable_apps
            .iter()
            .any(|rule| rule.enabled && same_process_name(&rule.process_name, process_name))
    }

    pub fn network_wake_enabled_for(&self, process_name: &str) -> bool {
        self.network_wake_enabled
            && self.suspendable_apps.iter().any(|rule| {
                rule.enabled
                    && rule.network_wake_enabled
                    && same_process_name(&rule.process_name, process_name)
            })
    }

    pub fn audio_wake_enabled_for(&self, process_name: &str) -> bool {
        self.audio_wake_enabled
            && self.suspendable_apps.iter().any(|rule| {
                rule.enabled
                    && rule.audio_wake_enabled
                    && same_process_name(&rule.process_name, process_name)
            })
    }

    pub fn network_wake_thresholds_for(&self, process_name: &str) -> Option<(u64, u64)> {
        self.network_wake_enabled.then_some(())?;
        self.suspendable_apps.iter().find_map(|rule| {
            (rule.enabled
                && rule.network_wake_enabled
                && same_process_name(&rule.process_name, process_name))
            .then_some((
                rule.network_download_threshold_bytes,
                rule.network_upload_threshold_bytes,
            ))
        })
    }
}

impl BackgroundEfficiencySettings {
    pub fn contains_custom_rule(&self, process_name: &str) -> bool {
        self.custom_rules
            .iter()
            .any(|rule| same_process_name(&rule.process_name, process_name))
    }

    pub fn custom_rule_enabled_for(&self, process_name: &str) -> bool {
        self.custom_rules
            .iter()
            .any(|rule| rule.enabled && same_process_name(&rule.process_name, process_name))
    }
}

impl BackgroundCpuRestrictionSettings {
    pub fn contains_exclusion(&self, process_name: &str) -> bool {
        self.exclusions
            .iter()
            .any(|rule| same_process_name(&rule.process_name, process_name))
    }

    pub fn exclusion_enabled_for(&self, process_name: &str) -> bool {
        self.exclusions
            .iter()
            .any(|rule| rule.enabled && same_process_name(&rule.process_name, process_name))
    }
}

impl CoreSteeringSettings {
    pub fn contains_rule_for(&self, process_name: &str) -> bool {
        self.rules
            .iter()
            .any(|rule| same_process_name(&rule.process_name, process_name))
    }
}

impl WorkloadEngineSettings {
    pub fn contains_rule_for(&self, process_name: &str) -> bool {
        self.rules
            .iter()
            .any(|rule| same_process_name(&rule.process_name, process_name))
    }

    pub fn contains_exclusion(&self, process_name: &str) -> bool {
        self.workload_engine_exclusions
            .iter()
            .any(|rule| same_process_name(&rule.process_name, process_name))
    }

    pub fn workload_engine_exclusion_enabled_for(&self, process_name: &str) -> bool {
        self.workload_engine_exclusions.iter().any(|rule| {
            rule.enabled && process_name_matches_pattern(&rule.process_name, process_name)
        })
    }
}

fn process_name_matches_pattern(pattern: &str, process_name: &str) -> bool {
    wildcard_match(
        &pattern.trim().to_ascii_lowercase(),
        &process_name.trim().to_ascii_lowercase(),
    )
}

fn wildcard_match(pattern: &str, value: &str) -> bool {
    let pattern = pattern.as_bytes();
    let value = value.as_bytes();
    let mut pattern_index = 0;
    let mut value_index = 0;
    let mut star_index = None;
    let mut star_value_index = 0;

    while value_index < value.len() {
        if pattern_index < pattern.len()
            && (pattern[pattern_index] == b'?' || pattern[pattern_index] == value[value_index])
        {
            pattern_index += 1;
            value_index += 1;
        } else if pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
            star_index = Some(pattern_index);
            pattern_index += 1;
            star_value_index = value_index;
        } else if let Some(star) = star_index {
            pattern_index = star + 1;
            star_value_index += 1;
            value_index = star_value_index;
        } else {
            return false;
        }
    }

    while pattern_index < pattern.len() && pattern[pattern_index] == b'*' {
        pattern_index += 1;
    }

    pattern_index == pattern.len()
}

impl InputDetectionSettings {
    pub const fn any_enabled(&self) -> bool {
        self.keyboard || self.mouse || self.controller
    }

    pub const fn keyboard_or_mouse_enabled(&self) -> bool {
        self.keyboard || self.mouse
    }

    pub fn ensure_any_enabled(&mut self) {
        if !self.any_enabled() {
            self.keyboard = true;
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

impl ByTimeRule {
    pub fn parsed_times(&self) -> Option<(NaiveTime, NaiveTime)> {
        let start = NaiveTime::parse_from_str(&self.start_time, "%H:%M").ok()?;
        let end = NaiveTime::parse_from_str(&self.end_time, "%H:%M").ok()?;
        Some((start, end))
    }
}

impl ByCpuLoadRule {
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

    #[test]
    fn workload_engine_exclusions_support_wildcards() {
        let settings = WorkloadEngineSettings {
            workload_engine_exclusions: vec![
                ProcessExclusionRule {
                    enabled: true,
                    process_name: "game*.exe".to_owned(),
                    ..Default::default()
                },
                ProcessExclusionRule {
                    enabled: true,
                    process_name: "worker?.exe".to_owned(),
                    ..Default::default()
                },
                ProcessExclusionRule {
                    enabled: false,
                    process_name: "disabled.exe".to_owned(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        assert!(settings.workload_engine_exclusion_enabled_for("GameClient.exe"));
        assert!(settings.workload_engine_exclusion_enabled_for("worker1.exe"));
        assert!(!settings.workload_engine_exclusion_enabled_for("worker12.exe"));
        assert!(!settings.workload_engine_exclusion_enabled_for("disabled.exe"));
    }

    #[test]
    fn realtime_process_priority_downgrades_when_advanced_is_hidden() {
        assert_eq!(
            ProcessPrioritySetting::Realtime.safe_when_advanced_disabled(),
            ProcessPrioritySetting::High
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
    fn timer_resolution_rules_match_foreground_process_names() {
        let settings = TimerResolutionSettings {
            enabled: true,
            desired_100ns: 10_000,
            rules: vec![
                TimerResolutionRule {
                    enabled: true,
                    process_name: "game*.exe".to_owned(),
                    desired_100ns: 20_000,
                },
                TimerResolutionRule {
                    enabled: false,
                    process_name: "disabled.exe".to_owned(),
                    desired_100ns: 10_000,
                },
            ],
        };

        assert_eq!(
            settings.desired_resolution_for_foreground("GameClient.exe"),
            Some(("game*.exe".to_owned(), 20_000))
        );
        assert_eq!(
            settings.desired_resolution_for_foreground("disabled.exe"),
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
                process_name: "chat.exe".to_owned(),
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
}
