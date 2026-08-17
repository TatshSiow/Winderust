use chrono::{NaiveTime, Weekday};
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::foreground::same_executable_path;
use crate::power::plan::{ProcessorBoostMode, ProcessorPowerValues};
use crate::rules::{
    normalize_execution_failure_suppression_threshold,
    DEFAULT_EXECUTION_FAILURE_SUPPRESSION_THRESHOLD,
};

pub const CHECK_INTERVAL_MIN_MS: u64 = 250;
pub const CHECK_INTERVAL_MAX_MS: u64 = 60 * 1000;

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
    pub cpu_sets_soft: CpuAllocationSettings,
    #[serde(default)]
    pub processor_affinity_hard: CpuAllocationSettings,
    #[serde(default)]
    pub cpu_allocation_presets: Vec<CpuAllocationPreset>,
    #[serde(default)]
    pub advanced_power_plan_tuning_presets: Vec<AdvancedPowerPlanTuningPreset>,
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
    #[serde(default)]
    pub pause_dashboard_metrics: bool,
    #[serde(default)]
    pub pause_process_population: bool,
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
    pub allow_cross_session_process_control: bool,
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
    pub navigation_collapsed: bool,
    #[serde(default = "default_true")]
    pub show_enabled_feature_counts_in_sidebar: bool,
    #[serde(default = "default_true")]
    pub show_feature_status_on_cards: bool,
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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
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
    pub executable_path: String,
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
    pub foreground_detection_enabled: bool,
    #[serde(default)]
    pub visible_window_detection_enabled: bool,
    #[serde(default)]
    pub foreground_efficiency_mode: bool,
    #[serde(default)]
    pub visible_window_efficiency_mode: bool,
    #[serde(default = "default_true")]
    pub background_efficiency_mode: bool,
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
    pub executable_path: String,
    #[serde(default = "default_disabled_process_rule_mode")]
    pub focus_efficiency_mode: ProcessRuleMode,
    #[serde(default = "default_disabled_process_rule_mode")]
    pub visible_window_efficiency_mode: ProcessRuleMode,
    #[serde(default = "default_disabled_process_rule_mode")]
    pub background_efficiency_mode: ProcessRuleMode,
}

impl BackgroundEfficiencyRule {
    pub fn efficiency_mode_for(
        &self,
        focus: bool,
        visible_window: bool,
        default_enabled: bool,
    ) -> bool {
        if focus {
            self.focus_efficiency_mode
        } else if visible_window {
            self.visible_window_efficiency_mode
        } else {
            self.background_efficiency_mode
        }
        .resolve(default_enabled)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessRuleMode {
    #[default]
    Default,
    Enabled,
    Disabled,
}

impl ProcessRuleMode {
    pub const ALL: [Self; 3] = [Self::Default, Self::Enabled, Self::Disabled];

    pub const fn resolve(self, default_enabled: bool) -> bool {
        match self {
            Self::Default => default_enabled,
            Self::Enabled => true,
            Self::Disabled => false,
        }
    }
}

fn default_disabled_process_rule_mode() -> ProcessRuleMode {
    ProcessRuleMode::Disabled
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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CpuAllocationSettings {
    pub enabled: bool,
    #[serde(default)]
    pub rules: Vec<CpuAllocationRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CpuAllocationPreset {
    pub name: String,
    pub core_mask: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdvancedPowerPlanTuningPreset {
    pub name: String,
    pub values: ProcessorPowerValues,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProcessExclusionRule {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub executable_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process_foreground_priority: Option<ProcessPrioritySetting>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process_visible_window_priority: Option<ProcessPrioritySetting>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub process_background_priority: Option<ProcessPrioritySetting>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_foreground_priority: Option<ProcessThreadPrioritySetting>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_visible_window_priority: Option<ProcessThreadPrioritySetting>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thread_background_priority: Option<ProcessThreadPrioritySetting>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dynamic_priority_boost_foreground: Option<ProcessDynamicPriorityBoostSetting>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dynamic_priority_boost_visible_window: Option<ProcessDynamicPriorityBoostSetting>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dynamic_priority_boost_background: Option<ProcessDynamicPriorityBoostSetting>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub io_foreground_priority: Option<ProcessIoPrioritySetting>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub io_visible_window_priority: Option<ProcessIoPrioritySetting>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub io_background_priority: Option<ProcessIoPrioritySetting>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_foreground_priority: Option<ProcessGpuPrioritySetting>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_visible_window_priority: Option<ProcessGpuPrioritySetting>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gpu_background_priority: Option<ProcessGpuPrioritySetting>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_foreground_priority: Option<ProcessMemoryPrioritySetting>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_visible_window_priority: Option<ProcessMemoryPrioritySetting>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_background_priority: Option<ProcessMemoryPrioritySetting>,
}

impl Default for ProcessExclusionRule {
    fn default() -> Self {
        Self {
            enabled: true,
            executable_path: String::new(),
            process_foreground_priority: None,
            process_visible_window_priority: None,
            process_background_priority: None,
            thread_foreground_priority: None,
            thread_visible_window_priority: None,
            thread_background_priority: None,
            dynamic_priority_boost_foreground: None,
            dynamic_priority_boost_visible_window: None,
            dynamic_priority_boost_background: None,
            io_foreground_priority: None,
            io_visible_window_priority: None,
            io_background_priority: None,
            gpu_foreground_priority: None,
            gpu_visible_window_priority: None,
            gpu_background_priority: None,
            memory_foreground_priority: None,
            memory_visible_window_priority: None,
            memory_background_priority: None,
        }
    }
}

impl ProcessExclusionRule {
    pub fn process_priority_override(
        &self,
        foreground: bool,
        visible_window: bool,
    ) -> ProcessPrioritySetting {
        tier_override(
            self.process_foreground_priority,
            self.process_visible_window_priority,
            self.process_background_priority,
            foreground,
            visible_window,
        )
    }

    pub fn set_process_priority_override(
        &mut self,
        foreground: bool,
        visible_window: bool,
        priority: ProcessPrioritySetting,
    ) {
        set_tier_override(
            &mut self.process_foreground_priority,
            &mut self.process_visible_window_priority,
            &mut self.process_background_priority,
            foreground,
            visible_window,
            priority,
        );
    }

    pub fn thread_priority_override(
        &self,
        foreground: bool,
        visible_window: bool,
    ) -> ProcessThreadPrioritySetting {
        tier_override(
            self.thread_foreground_priority,
            self.thread_visible_window_priority,
            self.thread_background_priority,
            foreground,
            visible_window,
        )
    }

    pub fn set_thread_priority_override(
        &mut self,
        foreground: bool,
        visible_window: bool,
        priority: ProcessThreadPrioritySetting,
    ) {
        set_tier_override(
            &mut self.thread_foreground_priority,
            &mut self.thread_visible_window_priority,
            &mut self.thread_background_priority,
            foreground,
            visible_window,
            priority,
        );
    }

    pub fn dynamic_priority_boost_override(
        &self,
        foreground: bool,
        visible_window: bool,
    ) -> ProcessDynamicPriorityBoostSetting {
        tier_override(
            self.dynamic_priority_boost_foreground,
            self.dynamic_priority_boost_visible_window,
            self.dynamic_priority_boost_background,
            foreground,
            visible_window,
        )
    }

    pub fn set_dynamic_priority_boost_override(
        &mut self,
        foreground: bool,
        visible_window: bool,
        boost: ProcessDynamicPriorityBoostSetting,
    ) {
        set_tier_override(
            &mut self.dynamic_priority_boost_foreground,
            &mut self.dynamic_priority_boost_visible_window,
            &mut self.dynamic_priority_boost_background,
            foreground,
            visible_window,
            boost,
        );
    }

    pub fn io_priority_override(
        &self,
        foreground: bool,
        visible_window: bool,
    ) -> ProcessIoPrioritySetting {
        tier_override(
            self.io_foreground_priority,
            self.io_visible_window_priority,
            self.io_background_priority,
            foreground,
            visible_window,
        )
    }

    pub fn set_io_priority_override(
        &mut self,
        foreground: bool,
        visible_window: bool,
        priority: ProcessIoPrioritySetting,
    ) {
        set_tier_override(
            &mut self.io_foreground_priority,
            &mut self.io_visible_window_priority,
            &mut self.io_background_priority,
            foreground,
            visible_window,
            priority,
        );
    }

    pub fn gpu_priority_override(
        &self,
        foreground: bool,
        visible_window: bool,
    ) -> ProcessGpuPrioritySetting {
        tier_override(
            self.gpu_foreground_priority,
            self.gpu_visible_window_priority,
            self.gpu_background_priority,
            foreground,
            visible_window,
        )
    }

    pub fn set_gpu_priority_override(
        &mut self,
        foreground: bool,
        visible_window: bool,
        priority: ProcessGpuPrioritySetting,
    ) {
        set_tier_override(
            &mut self.gpu_foreground_priority,
            &mut self.gpu_visible_window_priority,
            &mut self.gpu_background_priority,
            foreground,
            visible_window,
            priority,
        );
    }

    pub fn memory_priority_override(
        &self,
        foreground: bool,
        visible_window: bool,
    ) -> ProcessMemoryPrioritySetting {
        tier_override(
            self.memory_foreground_priority,
            self.memory_visible_window_priority,
            self.memory_background_priority,
            foreground,
            visible_window,
        )
    }

    pub fn set_memory_priority_override(
        &mut self,
        foreground: bool,
        visible_window: bool,
        priority: ProcessMemoryPrioritySetting,
    ) {
        set_tier_override(
            &mut self.memory_foreground_priority,
            &mut self.memory_visible_window_priority,
            &mut self.memory_background_priority,
            foreground,
            visible_window,
            priority,
        );
    }
}

fn tier_override<T: Copy + Default>(
    foreground_value: Option<T>,
    visible_window_value: Option<T>,
    background_value: Option<T>,
    foreground: bool,
    visible_window: bool,
) -> T {
    if foreground {
        foreground_value
    } else if visible_window {
        visible_window_value.or(background_value)
    } else {
        background_value
    }
    .unwrap_or_default()
}

fn set_tier_override<T: Copy + Default + PartialEq>(
    foreground_target: &mut Option<T>,
    visible_window_target: &mut Option<T>,
    background_target: &mut Option<T>,
    foreground: bool,
    visible_window: bool,
    value: T,
) {
    if visible_window && !foreground {
        *visible_window_target = Some(value);
    } else {
        set_optional_default(
            if foreground {
                foreground_target
            } else {
                background_target
            },
            value,
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
pub struct CpuAllocationRule {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub executable_path: String,
    pub focus_core_mask: u64,
    pub visible_window_core_mask: u64,
    pub background_core_mask: u64,
}

impl CpuAllocationRule {
    pub fn has_cpu_selection(&self) -> bool {
        self.focus_core_mask != 0
            || self.visible_window_core_mask != 0
            || self.background_core_mask != 0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoreLimiterSettings {
    pub enabled: bool,
    #[serde(default = "default_true")]
    pub protect_foreground_app: bool,
    #[serde(default)]
    pub protect_visible_window_apps: bool,
    #[serde(default)]
    pub rules: Vec<CoreLimiterRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoreLimiterRule {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub executable_path: String,
    #[serde(default)]
    pub focus_mode: ProcessRuleMode,
    #[serde(default)]
    pub visible_window_mode: ProcessRuleMode,
    #[serde(default)]
    pub background_mode: ProcessRuleMode,
    #[serde(default = "default_core_limiter_threshold_percent")]
    pub threshold_percent: u8,
    #[serde(default = "default_core_limiter_sustain_seconds")]
    pub sustain_seconds: u64,
    #[serde(default = "default_core_limiter_cooldown_seconds")]
    pub cooldown_seconds: u64,
    #[serde(default = "default_core_limiter_max_logical_processors")]
    pub max_logical_processors: u8,
}

impl CoreLimiterRule {
    pub const fn mode_for(&self, focus: bool, visible_window: bool) -> ProcessRuleMode {
        if focus {
            self.focus_mode
        } else if visible_window {
            self.visible_window_mode
        } else {
            self.background_mode
        }
    }
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
    pub executable_path: String,
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
    #[serde(default = "default_true")]
    pub workload_engine_foreground_detection_enabled: bool,
    #[serde(default = "default_true")]
    pub workload_engine_visible_window_detection_enabled: bool,
    #[serde(default)]
    pub workload_engine_foreground_efficiency_mode: bool,
    #[serde(default)]
    pub workload_engine_visible_window_efficiency_mode: bool,
    #[serde(default = "default_workload_engine_background_priority")]
    pub workload_engine_background_priority: ProcessPriority,
    #[serde(default = "default_workload_engine_visible_window_priority")]
    pub workload_engine_visible_window_priority: ProcessPriority,
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
    #[serde(default = "default_workload_engine_foreground_memory_priority")]
    pub workload_engine_visible_window_memory_priority: ProcessMemoryPrioritySetting,
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
    #[serde(default)]
    pub visible_window_detection_enabled: bool,
    #[serde(default = "default_io_priority_background")]
    pub visible_window_priority: ProcessIoPrioritySetting,
    #[serde(default = "default_io_priority_background")]
    pub background_priority: ProcessIoPrioritySetting,
    #[serde(default = "default_true")]
    pub preserve_foreground_priority: bool,
    #[serde(default = "default_true")]
    pub preserve_visible_window_priority: bool,
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
    #[serde(default)]
    pub visible_window_detection_enabled: bool,
    #[serde(default = "default_process_priority_background")]
    pub visible_window_priority: ProcessPrioritySetting,
    #[serde(default = "default_process_priority_background")]
    pub background_priority: ProcessPrioritySetting,
    #[serde(default = "default_true")]
    pub preserve_foreground_priority: bool,
    #[serde(default = "default_true")]
    pub preserve_visible_window_priority: bool,
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
    #[serde(default)]
    pub visible_window_detection_enabled: bool,
    #[serde(default = "default_thread_priority_background")]
    pub visible_window_priority: ProcessThreadPrioritySetting,
    #[serde(default = "default_thread_priority_background")]
    pub background_priority: ProcessThreadPrioritySetting,
    #[serde(default = "default_true")]
    pub preserve_foreground_priority: bool,
    #[serde(default = "default_true")]
    pub preserve_visible_window_priority: bool,
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
    #[serde(default)]
    pub visible_window_detection_enabled: bool,
    #[serde(default = "default_dynamic_priority_boost_background")]
    pub visible_window_boost: ProcessDynamicPriorityBoostSetting,
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
    #[serde(default)]
    pub visible_window_detection_enabled: bool,
    #[serde(default = "default_gpu_priority_background")]
    pub visible_window_priority: ProcessGpuPrioritySetting,
    #[serde(default = "default_gpu_priority_background")]
    pub background_priority: ProcessGpuPrioritySetting,
    #[serde(default = "default_true")]
    pub preserve_foreground_priority: bool,
    #[serde(default = "default_true")]
    pub preserve_visible_window_priority: bool,
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
    #[serde(default)]
    pub visible_window_detection_enabled: bool,
    #[serde(default = "default_memory_priority_background")]
    pub visible_window_priority: ProcessMemoryPrioritySetting,
    #[serde(default = "default_memory_priority_background")]
    pub background_priority: ProcessMemoryPrioritySetting,
    #[serde(default = "default_true")]
    pub preserve_foreground_priority: bool,
    #[serde(default = "default_true")]
    pub preserve_visible_window_priority: bool,
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
    #[serde(default = "default_memory_trim_system_memory_load_threshold_percent")]
    pub system_memory_load_threshold_percent: u8,
    #[serde(default = "default_memory_trim_process_working_set_threshold_mb")]
    pub process_working_set_threshold_mb: u64,
    #[serde(default = "default_memory_trim_process_idle_seconds")]
    pub process_idle_seconds: u64,
    #[serde(default)]
    pub exclusions: Vec<ProcessExclusionRule>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimerResolutionRule {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub executable_path: String,
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
    pub const CUSTOM_RULE_ALL: [Self; 4] = [Self::Default, Self::VeryLow, Self::Low, Self::Normal];
    pub const ADVANCED_ALL: [Self; 6] = [
        Self::Default,
        Self::VeryLow,
        Self::Low,
        Self::Normal,
        Self::High,
        Self::Critical,
    ];
    pub const CUSTOM_RULE_ADVANCED_ALL: [Self; 6] = [
        Self::Default,
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
    pub const CUSTOM_RULE_ALL: [Self; 5] = [
        Self::Default,
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
    pub const CUSTOM_RULE_ADVANCED_ALL: [Self; 7] = [
        Self::Default,
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
    pub const CUSTOM_RULE_ALL: [Self; 6] = [
        Self::Default,
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
    pub const CUSTOM_RULE_ALL: [Self; 7] = [
        Self::Default,
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
    pub const CUSTOM_RULE_ADVANCED_ALL: [Self; 8] = [
        Self::Default,
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
    pub const CUSTOM_RULE_ALL: [Self; 6] = [
        Self::Default,
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
    pub const CUSTOM_RULE_ADVANCED_ALL: [Self; 7] = [
        Self::Default,
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
    pub const CUSTOM_RULE_ALL: [Self; 3] = [Self::Default, Self::Enabled, Self::Disabled];

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
    pub executable_path: String,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppSuspensionRule {
    #[serde(default = "default_true")]
    pub enabled: bool,
    pub executable_path: String,
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
                allow_cross_session_process_control: true,
                check_for_updates: true,
                update_channel: UpdateChannel::PreRelease,
                theme_mode: AppThemeMode::System,
                accent: AccentSettings::default(),
                language: AppLanguage::English,
                animation_mode: AnimationMode::System,
                navigation_collapsed: false,
                show_enabled_feature_counts_in_sidebar: true,
                show_feature_status_on_cards: true,
                pause_power_plan_switching_while_plugged_in: false,
                check_interval_ms: 1000,
            },
            advanced: AdvancedSettings::default(),
            adaptive_engine: AdaptiveEngineSettings::default(),
            by_activity: ByActivitySettings {
                enabled: false,
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
            cpu_sets_soft: CpuAllocationSettings::default(),
            processor_affinity_hard: CpuAllocationSettings::default(),
            cpu_allocation_presets: Vec::new(),
            advanced_power_plan_tuning_presets: Vec::new(),
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
            pause_dashboard_metrics: false,
            pause_process_population: false,
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
            foreground_detection_enabled: default_true(),
            visible_window_detection_enabled: false,
            foreground_efficiency_mode: false,
            visible_window_efficiency_mode: false,
            background_efficiency_mode: true,
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

const fn default_workload_engine_visible_window_priority() -> ProcessPriority {
    ProcessPriority::Normal
}

const fn default_workload_engine_foreground_memory_priority() -> ProcessMemoryPrioritySetting {
    ProcessMemoryPrioritySetting::Default
}

fn default_workload_engine_io_priority_settings() -> IoPrioritySettings {
    IoPrioritySettings {
        enabled: false,
        foreground_detection_enabled: true,
        foreground_priority: ProcessIoPrioritySetting::Normal,
        visible_window_detection_enabled: true,
        visible_window_priority: ProcessIoPrioritySetting::VeryLow,
        background_priority: ProcessIoPrioritySetting::VeryLow,
        preserve_foreground_priority: true,
        preserve_visible_window_priority: true,
        preserve_background_priority: true,
        exclusions: Vec::new(),
    }
}

fn default_workload_engine_thread_priority_settings() -> ThreadPrioritySettings {
    ThreadPrioritySettings {
        enabled: true,
        foreground_detection_enabled: true,
        foreground_priority: ProcessThreadPrioritySetting::Default,
        visible_window_detection_enabled: true,
        visible_window_priority: ProcessThreadPrioritySetting::BelowNormal,
        background_priority: ProcessThreadPrioritySetting::BelowNormal,
        preserve_foreground_priority: true,
        preserve_visible_window_priority: true,
        preserve_background_priority: true,
        exclusions: Vec::new(),
    }
}

fn default_workload_engine_dynamic_priority_boost_settings() -> DynamicPriorityBoostSettings {
    DynamicPriorityBoostSettings {
        enabled: true,
        foreground_detection_enabled: true,
        foreground_boost: ProcessDynamicPriorityBoostSetting::Enabled,
        visible_window_detection_enabled: true,
        visible_window_boost: ProcessDynamicPriorityBoostSetting::Disabled,
        background_boost: ProcessDynamicPriorityBoostSetting::Disabled,
        exclusions: Vec::new(),
    }
}

fn default_workload_engine_gpu_priority_settings() -> GpuPrioritySettings {
    GpuPrioritySettings {
        enabled: true,
        foreground_detection_enabled: true,
        foreground_priority: ProcessGpuPrioritySetting::Default,
        visible_window_detection_enabled: true,
        visible_window_priority: ProcessGpuPrioritySetting::BelowNormal,
        background_priority: ProcessGpuPrioritySetting::BelowNormal,
        preserve_foreground_priority: true,
        preserve_visible_window_priority: true,
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
    80
}

const fn default_memory_trim_process_working_set_threshold_mb() -> u64 {
    256
}

const fn default_memory_trim_process_idle_seconds() -> u64 {
    300
}

const fn default_timer_resolution_100ns() -> u32 {
    10_000
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

impl Default for CoreLimiterSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            protect_foreground_app: default_true(),
            protect_visible_window_apps: false,
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
            workload_engine_foreground_detection_enabled: default_true(),
            workload_engine_visible_window_detection_enabled: default_true(),
            workload_engine_foreground_efficiency_mode: false,
            workload_engine_visible_window_efficiency_mode: false,
            workload_engine_background_priority: default_workload_engine_background_priority(),
            workload_engine_visible_window_priority:
                default_workload_engine_visible_window_priority(),
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
            workload_engine_visible_window_memory_priority:
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
            visible_window_detection_enabled: false,
            visible_window_priority: default_io_priority_background(),
            background_priority: default_io_priority_background(),
            preserve_foreground_priority: true,
            preserve_visible_window_priority: true,
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
            visible_window_detection_enabled: false,
            visible_window_priority: default_process_priority_background(),
            background_priority: default_process_priority_background(),
            preserve_foreground_priority: true,
            preserve_visible_window_priority: true,
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
            visible_window_detection_enabled: false,
            visible_window_priority: default_thread_priority_background(),
            background_priority: default_thread_priority_background(),
            preserve_foreground_priority: true,
            preserve_visible_window_priority: true,
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
            visible_window_detection_enabled: false,
            visible_window_boost: default_dynamic_priority_boost_background(),
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
            visible_window_detection_enabled: false,
            visible_window_priority: default_gpu_priority_background(),
            background_priority: default_gpu_priority_background(),
            preserve_foreground_priority: true,
            preserve_visible_window_priority: true,
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
            visible_window_detection_enabled: false,
            visible_window_priority: default_memory_priority_background(),
            background_priority: default_memory_priority_background(),
            preserve_foreground_priority: true,
            preserve_visible_window_priority: true,
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
            system_memory_load_threshold_percent:
                default_memory_trim_system_memory_load_threshold_percent(),
            process_working_set_threshold_mb: default_memory_trim_process_working_set_threshold_mb(
            ),
            process_idle_seconds: default_memory_trim_process_idle_seconds(),
            exclusions: Vec::new(),
        }
    }
}

impl IoPrioritySettings {
    pub fn contains_exclusion(&self, process_name: &str) -> bool {
        self.exclusions
            .iter()
            .any(|rule| same_rule_executable_path(&rule.executable_path, process_name))
    }

    pub fn exclusion_enabled_for(&self, process_name: &str) -> bool {
        self.exclusions.iter().any(|rule| {
            process_exclusion_rule_matches(rule, process_name)
                && rule.io_foreground_priority.unwrap_or_default()
                    == ProcessIoPrioritySetting::Default
                && rule.io_visible_window_priority.unwrap_or_default()
                    == ProcessIoPrioritySetting::Default
                && rule.io_background_priority.unwrap_or_default()
                    == ProcessIoPrioritySetting::Default
        })
    }

    pub fn override_for(
        &self,
        process_name: &str,
        foreground: bool,
        visible_window: bool,
    ) -> Option<Option<ProcessIoPrioritySetting>> {
        process_custom_rule_override(&self.exclusions, process_name, |rule| {
            rule.io_priority_override(foreground, visible_window)
        })
    }
}

impl ProcessPrioritySettings {
    pub fn contains_exclusion(&self, process_name: &str) -> bool {
        self.exclusions
            .iter()
            .any(|rule| same_rule_executable_path(&rule.executable_path, process_name))
    }

    pub fn override_for(
        &self,
        process_name: &str,
        foreground: bool,
        visible_window: bool,
    ) -> Option<Option<ProcessPrioritySetting>> {
        process_custom_rule_override(&self.exclusions, process_name, |rule| {
            rule.process_priority_override(foreground, visible_window)
        })
    }
}

impl ThreadPrioritySettings {
    pub fn contains_exclusion(&self, process_name: &str) -> bool {
        self.exclusions
            .iter()
            .any(|rule| same_rule_executable_path(&rule.executable_path, process_name))
    }

    pub fn override_for(
        &self,
        process_name: &str,
        foreground: bool,
        visible_window: bool,
    ) -> Option<Option<ProcessThreadPrioritySetting>> {
        process_custom_rule_override(&self.exclusions, process_name, |rule| {
            rule.thread_priority_override(foreground, visible_window)
        })
    }
}

impl DynamicPriorityBoostSettings {
    pub fn contains_exclusion(&self, process_name: &str) -> bool {
        self.exclusions
            .iter()
            .any(|rule| same_rule_executable_path(&rule.executable_path, process_name))
    }

    pub fn override_for(
        &self,
        process_name: &str,
        foreground: bool,
        visible_window: bool,
    ) -> Option<Option<ProcessDynamicPriorityBoostSetting>> {
        process_custom_rule_override(&self.exclusions, process_name, |rule| {
            rule.dynamic_priority_boost_override(foreground, visible_window)
        })
    }
}

impl GpuPrioritySettings {
    pub fn contains_exclusion(&self, process_name: &str) -> bool {
        self.exclusions
            .iter()
            .any(|rule| same_rule_executable_path(&rule.executable_path, process_name))
    }

    pub fn exclusion_enabled_for(&self, process_name: &str) -> bool {
        self.exclusions.iter().any(|rule| {
            process_exclusion_rule_matches(rule, process_name)
                && rule.gpu_foreground_priority.unwrap_or_default()
                    == ProcessGpuPrioritySetting::Default
                && rule.gpu_visible_window_priority.unwrap_or_default()
                    == ProcessGpuPrioritySetting::Default
                && rule.gpu_background_priority.unwrap_or_default()
                    == ProcessGpuPrioritySetting::Default
        })
    }

    pub fn override_for(
        &self,
        process_name: &str,
        foreground: bool,
        visible_window: bool,
    ) -> Option<Option<ProcessGpuPrioritySetting>> {
        process_custom_rule_override(&self.exclusions, process_name, |rule| {
            rule.gpu_priority_override(foreground, visible_window)
        })
    }
}

impl MemoryPrioritySettings {
    pub fn contains_exclusion(&self, process_name: &str) -> bool {
        self.exclusions
            .iter()
            .any(|rule| same_rule_executable_path(&rule.executable_path, process_name))
    }

    pub fn exclusion_enabled_for(&self, process_name: &str) -> bool {
        self.exclusions.iter().any(|rule| {
            process_exclusion_rule_matches(rule, process_name)
                && rule.memory_foreground_priority.unwrap_or_default()
                    == ProcessMemoryPrioritySetting::Default
                && rule.memory_visible_window_priority.unwrap_or_default()
                    == ProcessMemoryPrioritySetting::Default
                && rule.memory_background_priority.unwrap_or_default()
                    == ProcessMemoryPrioritySetting::Default
        })
    }

    pub fn override_for(
        &self,
        process_name: &str,
        foreground: bool,
        visible_window: bool,
    ) -> Option<Option<ProcessMemoryPrioritySetting>> {
        process_custom_rule_override(&self.exclusions, process_name, |rule| {
            rule.memory_priority_override(foreground, visible_window)
        })
    }
}

fn process_exclusion_rule_matches(rule: &ProcessExclusionRule, process_name: &str) -> bool {
    rule.enabled && same_rule_executable_path(&rule.executable_path, process_name)
}

fn process_custom_rule_override<T>(
    rules: &[ProcessExclusionRule],
    process_name: &str,
    value: impl Fn(&ProcessExclusionRule) -> T,
) -> Option<Option<T>>
where
    T: Copy + Default + PartialEq,
{
    rules
        .iter()
        .find(|rule| process_exclusion_rule_matches(rule, process_name))
        .map(|rule| {
            let value = value(rule);
            (value != T::default()).then_some(value)
        })
}

impl TimerResolutionSettings {
    pub fn desired_resolution_for_foreground(&self, process_name: &str) -> Option<(String, u32)> {
        self.rules
            .iter()
            .find(|rule| {
                rule.enabled
                    && !rule.executable_path.trim().is_empty()
                    && same_rule_executable_path(&rule.executable_path, process_name)
            })
            .map(|rule| (rule.executable_path.clone(), rule.desired_100ns))
    }

    pub fn contains_rule_for(&self, process_name: &str) -> bool {
        self.rules
            .iter()
            .any(|rule| same_rule_executable_path(&rule.executable_path, process_name))
    }
}

impl MemoryTrimSettings {
    pub fn exclusion_enabled_for(&self, process_name: &str) -> bool {
        self.exclusions.iter().any(|rule| {
            rule.enabled && same_rule_executable_path(&rule.executable_path, process_name)
        })
    }
}

impl AppSuspensionSettings {
    pub fn contains_suspendable_app(&self, process_name: &str) -> bool {
        self.suspendable_apps
            .iter()
            .any(|rule| same_rule_executable_path(&rule.executable_path, process_name))
    }

    pub fn suspendable_app_enabled_for(&self, process_name: &str) -> bool {
        self.suspendable_apps.iter().any(|rule| {
            rule.enabled && same_rule_executable_path(&rule.executable_path, process_name)
        })
    }

    pub fn network_wake_enabled_for(&self, process_name: &str) -> bool {
        self.network_wake_enabled
            && self.suspendable_apps.iter().any(|rule| {
                rule.enabled
                    && rule.network_wake_enabled
                    && same_rule_executable_path(&rule.executable_path, process_name)
            })
    }

    pub fn audio_wake_enabled_for(&self, process_name: &str) -> bool {
        self.audio_wake_enabled
            && self.suspendable_apps.iter().any(|rule| {
                rule.enabled
                    && rule.audio_wake_enabled
                    && same_rule_executable_path(&rule.executable_path, process_name)
            })
    }

    pub fn network_wake_thresholds_for(&self, process_name: &str) -> Option<(u64, u64)> {
        self.network_wake_enabled.then_some(())?;
        self.suspendable_apps.iter().find_map(|rule| {
            (rule.enabled
                && rule.network_wake_enabled
                && same_rule_executable_path(&rule.executable_path, process_name))
            .then_some((
                rule.network_download_threshold_bytes,
                rule.network_upload_threshold_bytes,
            ))
        })
    }
}

impl BackgroundEfficiencySettings {
    pub fn custom_rule_applies_efficiency_mode(&self, rule: &BackgroundEfficiencyRule) -> bool {
        rule.background_efficiency_mode
            .resolve(self.background_efficiency_mode)
            || self.foreground_detection_enabled
                && rule
                    .focus_efficiency_mode
                    .resolve(self.foreground_efficiency_mode)
            || self.visible_window_detection_enabled
                && rule
                    .visible_window_efficiency_mode
                    .resolve(self.visible_window_efficiency_mode)
    }

    pub fn contains_custom_rule(&self, process_name: &str) -> bool {
        self.custom_rules
            .iter()
            .any(|rule| same_rule_executable_path(&rule.executable_path, process_name))
    }

    pub fn custom_rule_for(&self, process_name: &str) -> Option<&BackgroundEfficiencyRule> {
        self.custom_rules.iter().find(|rule| {
            rule.enabled && same_rule_executable_path(&rule.executable_path, process_name)
        })
    }
}

impl CpuAllocationSettings {
    pub fn contains_rule_for(&self, process_name: &str) -> bool {
        self.rules
            .iter()
            .any(|rule| same_rule_executable_path(&rule.executable_path, process_name))
    }
}

impl WorkloadEngineSettings {
    pub fn contains_rule_for(&self, process_name: &str) -> bool {
        self.rules
            .iter()
            .any(|rule| same_rule_executable_path(&rule.executable_path, process_name))
    }

    pub fn contains_exclusion(&self, process_name: &str) -> bool {
        self.workload_engine_exclusions
            .iter()
            .any(|rule| same_rule_executable_path(&rule.executable_path, process_name))
    }

    pub fn workload_engine_exclusion_enabled_for(&self, process_name: &str) -> bool {
        self.workload_engine_exclusions.iter().any(|rule| {
            rule.enabled && same_rule_executable_path(&rule.executable_path, process_name)
        })
    }
}

fn same_rule_executable_path(left: &str, right: &str) -> bool {
    let left = left.trim();
    let right = right.trim();
    !left.is_empty() && !right.is_empty() && same_executable_path(Path::new(left), Path::new(right))
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
    fn custom_priority_rule_choices_do_not_offer_auto() {
        assert!(!ProcessPrioritySetting::CUSTOM_RULE_ALL.contains(&ProcessPrioritySetting::Auto));
        assert!(!ProcessThreadPrioritySetting::CUSTOM_RULE_ALL
            .contains(&ProcessThreadPrioritySetting::Auto));
        assert!(!ProcessDynamicPriorityBoostSetting::CUSTOM_RULE_ALL
            .contains(&ProcessDynamicPriorityBoostSetting::Auto));
        assert!(
            !ProcessIoPrioritySetting::CUSTOM_RULE_ALL.contains(&ProcessIoPrioritySetting::Auto)
        );
        assert!(
            !ProcessGpuPrioritySetting::CUSTOM_RULE_ALL.contains(&ProcessGpuPrioritySetting::Auto)
        );
        assert!(!ProcessMemoryPrioritySetting::CUSTOM_RULE_ALL
            .contains(&ProcessMemoryPrioritySetting::Auto));
    }

    #[test]
    fn visible_window_default_remains_independent_from_background() {
        let mut rule = ProcessExclusionRule {
            process_background_priority: Some(ProcessPrioritySetting::BelowNormal),
            thread_background_priority: Some(ProcessThreadPrioritySetting::Lowest),
            dynamic_priority_boost_background: Some(ProcessDynamicPriorityBoostSetting::Disabled),
            io_background_priority: Some(ProcessIoPrioritySetting::VeryLow),
            gpu_background_priority: Some(ProcessGpuPrioritySetting::Idle),
            memory_background_priority: Some(ProcessMemoryPrioritySetting::VeryLow),
            ..Default::default()
        };

        rule.set_process_priority_override(false, true, ProcessPrioritySetting::Default);
        rule.set_thread_priority_override(false, true, ProcessThreadPrioritySetting::Default);
        rule.set_dynamic_priority_boost_override(
            false,
            true,
            ProcessDynamicPriorityBoostSetting::Default,
        );
        rule.set_io_priority_override(false, true, ProcessIoPrioritySetting::Default);
        rule.set_gpu_priority_override(false, true, ProcessGpuPrioritySetting::Default);
        rule.set_memory_priority_override(false, true, ProcessMemoryPrioritySetting::Default);

        assert_eq!(
            rule.process_priority_override(false, true),
            ProcessPrioritySetting::Default
        );
        assert_eq!(
            rule.thread_priority_override(false, true),
            ProcessThreadPrioritySetting::Default
        );
        assert_eq!(
            rule.dynamic_priority_boost_override(false, true),
            ProcessDynamicPriorityBoostSetting::Default
        );
        assert_eq!(
            rule.io_priority_override(false, true),
            ProcessIoPrioritySetting::Default
        );
        assert_eq!(
            rule.gpu_priority_override(false, true),
            ProcessGpuPrioritySetting::Default
        );
        assert_eq!(
            rule.memory_priority_override(false, true),
            ProcessMemoryPrioritySetting::Default
        );
    }

    #[test]
    fn visible_window_priority_override_is_not_an_exclusion() {
        let path = r"C:\Apps\editor.exe";
        let rule = ProcessExclusionRule {
            executable_path: path.to_owned(),
            io_visible_window_priority: Some(ProcessIoPrioritySetting::Low),
            gpu_visible_window_priority: Some(ProcessGpuPrioritySetting::BelowNormal),
            memory_visible_window_priority: Some(ProcessMemoryPrioritySetting::Low),
            ..Default::default()
        };
        let mut settings = Settings::default();
        settings.io_priority.exclusions.push(rule.clone());
        settings.gpu_priority.exclusions.push(rule.clone());
        settings.memory_priority.exclusions.push(rule);

        assert!(!settings.io_priority.exclusion_enabled_for(path));
        assert!(!settings.gpu_priority.exclusion_enabled_for(path));
        assert!(!settings.memory_priority.exclusion_enabled_for(path));
    }

    #[test]
    fn first_run_rule_modules_are_disabled() {
        let settings = Settings::default();

        assert!(![
            settings.adaptive_engine.enabled,
            settings.by_activity.enabled,
            settings.by_foreground.enabled,
            settings.by_time.enabled,
            settings.by_cpu_load.enabled,
            settings.background_efficiency.enabled,
            settings.app_suspension.enabled,
            settings.cpu_sets_soft.enabled,
            settings.processor_affinity_hard.enabled,
            settings.core_limiter.enabled,
            settings.by_running_app.enabled,
            settings.workload_engine.enabled,
            settings.process_priority.enabled,
            settings.thread_priority.enabled,
            settings.dynamic_priority_boost.enabled,
            settings.io_priority.enabled,
            settings.gpu_priority.enabled,
            settings.memory_priority.enabled,
            settings.memory_trim.enabled,
            settings.timer_resolution.enabled,
        ]
        .into_iter()
        .any(|enabled| enabled));
    }

    #[test]
    fn cross_session_process_control_is_enabled_by_default() {
        assert!(
            Settings::default()
                .general
                .allow_cross_session_process_control
        );
    }

    #[test]
    fn appearance_indicators_are_enabled_by_default() {
        let general = Settings::default().general;

        assert!(general.show_enabled_feature_counts_in_sidebar);
        assert!(general.show_feature_status_on_cards);
    }

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
    fn workload_engine_exclusions_match_only_the_configured_executable_path() {
        let settings = WorkloadEngineSettings {
            workload_engine_exclusions: vec![
                ProcessExclusionRule {
                    enabled: true,
                    executable_path: "C:\\Games\\game.exe".to_owned(),
                    ..Default::default()
                },
                ProcessExclusionRule {
                    enabled: false,
                    executable_path: "C:\\Apps\\disabled.exe".to_owned(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };

        assert!(settings.workload_engine_exclusion_enabled_for("c:/games/GAME.exe"));
        assert!(!settings.workload_engine_exclusion_enabled_for("C:\\Other\\game.exe"));
        assert!(!settings.workload_engine_exclusion_enabled_for("C:\\Games\\game*.exe"));
        assert!(!settings.workload_engine_exclusion_enabled_for("C:\\Apps\\disabled.exe"));
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
    fn timer_resolution_rules_match_only_the_foreground_executable_path() {
        let settings = TimerResolutionSettings {
            enabled: true,
            desired_100ns: 10_000,
            rules: vec![
                TimerResolutionRule {
                    enabled: true,
                    executable_path: "C:\\Games\\game.exe".to_owned(),
                    desired_100ns: 20_000,
                },
                TimerResolutionRule {
                    enabled: false,
                    executable_path: "C:\\Apps\\disabled.exe".to_owned(),
                    desired_100ns: 10_000,
                },
            ],
        };

        assert_eq!(
            settings.desired_resolution_for_foreground("c:/games/GAME.exe"),
            Some(("C:\\Games\\game.exe".to_owned(), 20_000))
        );
        assert_eq!(
            settings.desired_resolution_for_foreground("C:\\Other\\game.exe"),
            None
        );
        assert_eq!(
            settings.desired_resolution_for_foreground("C:\\Apps\\disabled.exe"),
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
                executable_path: "chat.exe".to_owned(),
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

    #[test]
    fn process_custom_rules_use_first_enabled_exact_path_override_for_every_priority_property() {
        let disabled = ProcessExclusionRule {
            enabled: false,
            executable_path: r"C:\Apps\worker.exe".to_owned(),
            process_foreground_priority: Some(ProcessPrioritySetting::Idle),
            thread_foreground_priority: Some(ProcessThreadPrioritySetting::Idle),
            dynamic_priority_boost_foreground: Some(ProcessDynamicPriorityBoostSetting::Disabled),
            io_foreground_priority: Some(ProcessIoPrioritySetting::VeryLow),
            gpu_foreground_priority: Some(ProcessGpuPrioritySetting::Idle),
            memory_foreground_priority: Some(ProcessMemoryPrioritySetting::VeryLow),
            ..Default::default()
        };
        let expected = ProcessExclusionRule {
            enabled: true,
            executable_path: r"C:\Apps\worker.exe".to_owned(),
            process_foreground_priority: Some(ProcessPrioritySetting::AboveNormal),
            process_visible_window_priority: Some(ProcessPrioritySetting::Normal),
            process_background_priority: Some(ProcessPrioritySetting::BelowNormal),
            thread_foreground_priority: Some(ProcessThreadPrioritySetting::Highest),
            thread_background_priority: Some(ProcessThreadPrioritySetting::Lowest),
            dynamic_priority_boost_foreground: Some(ProcessDynamicPriorityBoostSetting::Enabled),
            dynamic_priority_boost_background: Some(ProcessDynamicPriorityBoostSetting::Disabled),
            io_foreground_priority: Some(ProcessIoPrioritySetting::Normal),
            io_background_priority: Some(ProcessIoPrioritySetting::VeryLow),
            gpu_foreground_priority: Some(ProcessGpuPrioritySetting::AboveNormal),
            gpu_background_priority: Some(ProcessGpuPrioritySetting::Idle),
            memory_foreground_priority: Some(ProcessMemoryPrioritySetting::Normal),
            memory_background_priority: Some(ProcessMemoryPrioritySetting::VeryLow),
            ..Default::default()
        };
        let later_duplicate = ProcessExclusionRule {
            enabled: true,
            executable_path: r"C:\Apps\worker.exe".to_owned(),
            process_foreground_priority: Some(ProcessPrioritySetting::High),
            thread_foreground_priority: Some(ProcessThreadPrioritySetting::TimeCritical),
            dynamic_priority_boost_foreground: Some(ProcessDynamicPriorityBoostSetting::Disabled),
            io_foreground_priority: Some(ProcessIoPrioritySetting::Low),
            gpu_foreground_priority: Some(ProcessGpuPrioritySetting::Normal),
            memory_foreground_priority: Some(ProcessMemoryPrioritySetting::Medium),
            ..Default::default()
        };
        let exclusion = ProcessExclusionRule {
            enabled: true,
            executable_path: r"C:\Apps\excluded.exe".to_owned(),
            ..Default::default()
        };
        let rules = vec![disabled, expected, later_duplicate, exclusion];

        let process = ProcessPrioritySettings {
            exclusions: rules.clone(),
            ..Default::default()
        };
        let thread = ThreadPrioritySettings {
            exclusions: rules.clone(),
            ..Default::default()
        };
        let boost = DynamicPriorityBoostSettings {
            exclusions: rules.clone(),
            ..Default::default()
        };
        let io = IoPrioritySettings {
            exclusions: rules.clone(),
            ..Default::default()
        };
        let gpu = GpuPrioritySettings {
            exclusions: rules.clone(),
            ..Default::default()
        };
        let memory = MemoryPrioritySettings {
            exclusions: rules,
            ..Default::default()
        };

        assert_eq!(
            process.override_for(r"c:/apps/WORKER.exe", true, false),
            Some(Some(ProcessPrioritySetting::AboveNormal))
        );
        assert_eq!(
            process.override_for(r"c:/apps/WORKER.exe", false, true),
            Some(Some(ProcessPrioritySetting::Normal))
        );
        assert_eq!(
            process.override_for(r"c:/apps/WORKER.exe", false, false),
            Some(Some(ProcessPrioritySetting::BelowNormal))
        );
        assert_eq!(
            thread.override_for(r"c:/apps/WORKER.exe", true, false),
            Some(Some(ProcessThreadPrioritySetting::Highest))
        );
        assert_eq!(
            thread.override_for(r"c:/apps/WORKER.exe", false, false),
            Some(Some(ProcessThreadPrioritySetting::Lowest))
        );
        assert_eq!(
            boost.override_for(r"c:/apps/WORKER.exe", true, false),
            Some(Some(ProcessDynamicPriorityBoostSetting::Enabled))
        );
        assert_eq!(
            boost.override_for(r"c:/apps/WORKER.exe", false, false),
            Some(Some(ProcessDynamicPriorityBoostSetting::Disabled))
        );
        assert_eq!(
            io.override_for(r"c:/apps/WORKER.exe", true, false),
            Some(Some(ProcessIoPrioritySetting::Normal))
        );
        assert_eq!(
            io.override_for(r"c:/apps/WORKER.exe", false, false),
            Some(Some(ProcessIoPrioritySetting::VeryLow))
        );
        assert_eq!(
            gpu.override_for(r"c:/apps/WORKER.exe", true, false),
            Some(Some(ProcessGpuPrioritySetting::AboveNormal))
        );
        assert_eq!(
            gpu.override_for(r"c:/apps/WORKER.exe", false, false),
            Some(Some(ProcessGpuPrioritySetting::Idle))
        );
        assert_eq!(
            memory.override_for(r"c:/apps/WORKER.exe", true, false),
            Some(Some(ProcessMemoryPrioritySetting::Normal))
        );
        assert_eq!(
            memory.override_for(r"c:/apps/WORKER.exe", false, false),
            Some(Some(ProcessMemoryPrioritySetting::VeryLow))
        );

        assert_eq!(
            process.override_for(r"C:\Apps\excluded.exe", true, false),
            Some(None)
        );
        assert_eq!(
            thread.override_for(r"C:\Apps\excluded.exe", true, false),
            Some(None)
        );
        assert_eq!(
            boost.override_for(r"C:\Apps\excluded.exe", true, false),
            Some(None)
        );
        assert_eq!(
            io.override_for(r"C:\Apps\excluded.exe", true, false),
            Some(None)
        );
        assert_eq!(
            gpu.override_for(r"C:\Apps\excluded.exe", true, false),
            Some(None)
        );
        assert_eq!(
            memory.override_for(r"C:\Apps\excluded.exe", true, false),
            Some(None)
        );

        assert_eq!(
            process.override_for(r"C:\Other\worker.exe", true, false),
            None
        );
        assert_eq!(
            thread.override_for(r"C:\Other\worker.exe", true, false),
            None
        );
        assert_eq!(
            boost.override_for(r"C:\Other\worker.exe", true, false),
            None
        );
        assert_eq!(io.override_for(r"C:\Other\worker.exe", true, false), None);
        assert_eq!(gpu.override_for(r"C:\Other\worker.exe", true, false), None);
        assert_eq!(
            memory.override_for(r"C:\Other\worker.exe", true, false),
            None
        );
    }
}
