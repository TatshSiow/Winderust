// Pure tuning semantics extracted from ui/app/shared/presets.rs.
use crate::config::*;
use rust_i18n::t;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::ui) enum BuiltInAdaptiveEnginePreset {
    PowerSave,
    Balanced,
    Performance,
    Speed,
}
impl BuiltInAdaptiveEnginePreset {
    pub(super) const ALL: [Self; 4] = [
        Self::PowerSave,
        Self::Balanced,
        Self::Performance,
        Self::Speed,
    ];
}

pub(super) fn built_in_adaptive_engine_preset_label(preset: BuiltInAdaptiveEnginePreset) -> String {
    match preset {
        BuiltInAdaptiveEnginePreset::PowerSave => {
            t!("adaptive_engine.preset_power_save").to_string()
        }
        BuiltInAdaptiveEnginePreset::Balanced => t!("adaptive_engine.preset_balanced").to_string(),
        BuiltInAdaptiveEnginePreset::Performance => {
            t!("adaptive_engine.preset_performance").to_string()
        }
        BuiltInAdaptiveEnginePreset::Speed => t!("adaptive_engine.preset_speed").to_string(),
    }
}

pub(super) fn apply_built_in_adaptive_engine_preset(
    settings: &mut Settings,
    preset: BuiltInAdaptiveEnginePreset,
) {
    apply_adaptive_engine_process_preset(&mut settings.adaptive_engine_process, preset);
}

pub(super) fn capture_adaptive_engine_preset(
    settings: &Settings,
    name: String,
) -> AdaptiveEnginePreset {
    AdaptiveEnginePreset {
        name,
        processor_power_policy_enabled: false,
        base_processor_policy: settings.adaptive_engine.base_processor_policy.normalized(),
        background_pressure_profile: settings
            .adaptive_engine
            .background_pressure_profile
            .normalized(),
        focus_and_launch_profile: settings
            .adaptive_engine
            .focus_and_launch_profile
            .normalized(),
        adaptive_engine_process: comparable_adaptive_engine_process_tuning(
            &settings.adaptive_engine_process,
        ),
    }
}

pub(super) fn apply_adaptive_engine_preset(settings: &mut Settings, preset: &AdaptiveEnginePreset) {
    let exclusions = std::mem::take(&mut settings.adaptive_engine_process.custom_rules);
    let io_exclusions =
        std::mem::take(&mut settings.adaptive_engine_process.io_priority.exclusions);
    let thread_exclusions =
        std::mem::take(&mut settings.adaptive_engine_process.thread_priority.exclusions);
    let dynamic_boost_exclusions = std::mem::take(
        &mut settings
            .adaptive_engine_process
            .dynamic_priority_boost
            .exclusions,
    );
    let gpu_exclusions =
        std::mem::take(&mut settings.adaptive_engine_process.gpu_priority.exclusions);
    settings.adaptive_engine_process = preset.adaptive_engine_process.clone();
    settings.adaptive_engine_process.custom_rules = exclusions;
    settings.adaptive_engine_process.io_priority.exclusions = io_exclusions;
    settings.adaptive_engine_process.thread_priority.exclusions = thread_exclusions;
    settings
        .adaptive_engine_process
        .dynamic_priority_boost
        .exclusions = dynamic_boost_exclusions;
    settings.adaptive_engine_process.gpu_priority.exclusions = gpu_exclusions;
}

pub(super) fn comparable_adaptive_engine_process_tuning(
    settings: &AdaptiveEngineProcessSettings,
) -> AdaptiveEngineProcessSettings {
    let mut settings = settings.clone();
    settings.custom_rules.clear();
    settings.io_priority.exclusions.clear();
    settings.thread_priority.exclusions.clear();
    settings.dynamic_priority_boost.exclusions.clear();
    settings.gpu_priority.exclusions.clear();
    settings
}

pub(super) fn background_processor_selection_label(
    selection: BackgroundProcessorSelection,
) -> String {
    match selection {
        BackgroundProcessorSelection::LeastUsed => {
            t!("adaptive_engine_process.processor_selection_least_used_all").to_string()
        }
        BackgroundProcessorSelection::LeastUsedPerformanceCores => {
            t!("adaptive_engine_process.processor_selection_least_used_p_cores").to_string()
        }
        BackgroundProcessorSelection::LeastUsedEfficiencyCores => {
            t!("adaptive_engine_process.processor_selection_least_used_e_cores").to_string()
        }
        BackgroundProcessorSelection::PerformanceCores => t!("cpu_allocation.p_cores").to_string(),
        BackgroundProcessorSelection::EfficiencyCores => t!("cpu_allocation.e_cores").to_string(),
        BackgroundProcessorSelection::AllCoresNoSmt => {
            t!("cpu_allocation.all_cores_no_smt").to_string()
        }
        BackgroundProcessorSelection::PerformanceCoresNoSmt => {
            t!("cpu_allocation.p_cores_no_smt").to_string()
        }
        BackgroundProcessorSelection::Custom => t!("cpu_allocation.custom").to_string(),
    }
}

pub(super) fn apply_adaptive_engine_process_preset(
    settings: &mut AdaptiveEngineProcessSettings,
    preset: BuiltInAdaptiveEnginePreset,
) {
    let values = adaptive_engine_process_preset_values(preset);
    settings.process_priority_enabled = values.process_priority_enabled;
    settings.process_priority_foreground_detection_enabled = true;
    settings.process_priority_visible_window_detection_enabled = true;
    settings.process_priority_preserve_foreground = false;
    settings.process_priority_preserve_visible_window = false;
    settings.process_priority_preserve_background = false;
    settings.background_efficiency_enabled = values.background_efficiency_enabled;
    settings.focus_process_background_efficiency_override_enabled = true;
    settings.visible_window_background_efficiency_override_enabled = true;
    settings.focus_process_background_efficiency_mode = false;
    settings.visible_window_background_efficiency_mode = false;
    settings.background_efficiency_mode = values.background_efficiency_mode;
    settings.focus_process_priority = values.focus_process_priority;
    settings.background_priority = values.background_priority;
    settings.visible_window_priority = values.visible_window_priority;
    settings.io_priority = io_priority_preset_values(values);
    settings.thread_priority = thread_priority_preset_values(preset);
    settings.dynamic_priority_boost = dynamic_priority_boost_preset_values(preset);
    settings.gpu_priority = gpu_priority_preset_values(preset);
    settings.memory_priority_enabled = values.memory_priority_enabled;
    settings.memory_priority_foreground_detection_enabled = true;
    settings.memory_priority_visible_window_detection_enabled = true;
    settings.memory_priority_preserve_foreground = true;
    settings.memory_priority_preserve_visible_window = true;
    settings.memory_priority_preserve_background = true;
    settings.focus_process_memory_priority = values.focus_process_memory_priority;
    settings.visible_window_memory_priority = values.visible_window_memory_priority;
    settings.background_memory_priority = values.background_memory_priority;
    settings.limit_background_processors_enabled = values.limit_background_processors_enabled;
    settings.dynamic_resource_zones_enabled = values.dynamic_resource_zones_enabled;
    settings.dynamic_resource_zone_settings = DynamicResourceZoneSettings::default();
    settings.preserve_custom_affinity = false;
    settings.processors_to_remove = default_processors_to_remove();
    settings.background_processor_selection = values.background_processor_selection;
    settings.processor_limit_percent = values.processor_limit_percent;
    settings.pressure_source = AdaptivePressureSource::default();
    settings.exclude_service_hosts = false;
    settings.foreground_or_system_cpu_threshold_percent =
        values.foreground_or_system_cpu_threshold_percent;
    settings.background_app_cpu_threshold_percent = values.background_app_cpu_threshold_percent;
    settings.cpu_recovery_threshold_percent = values.cpu_recovery_threshold_percent;
    settings.reaction_time_ms = values.reaction_time_ms;
    settings.restraint_trigger_time_ms = values.restraint_trigger_time_ms;
    settings.pause_restraint_when_idle = false;
    settings.restraint_idle_timeout_seconds = DEFAULT_ADAPTIVE_RESTRAINT_IDLE_TIMEOUT_SECONDS;
    settings.restraint_priority = AdaptiveRestraintPriority::default();
    settings.cpu_restraint_time_seconds = values.cpu_restraint_time_seconds;
    settings.maximum_restraint_time_seconds = 0;
    settings.cpu_recovery_time_seconds = values.cpu_recovery_time_seconds;
    settings.maximum_restrained_apps = values.maximum_restrained_apps;
}

#[derive(Clone, Copy)]
pub(super) struct AdaptiveEngineProcessPresetValues {
    pub(super) process_priority_enabled: bool,
    pub(super) background_efficiency_enabled: bool,
    pub(super) background_efficiency_mode: bool,
    pub(super) focus_process_priority: ProcessPrioritySetting,
    pub(super) background_priority: ProcessPrioritySetting,
    pub(super) visible_window_priority: ProcessPrioritySetting,
    pub(super) focus_process_io_priority: ProcessIoPrioritySetting,
    pub(super) visible_window_io_priority: ProcessIoPrioritySetting,
    pub(super) io_priority_enabled: bool,
    pub(super) background_io_priority: ProcessIoPriority,
    pub(super) memory_priority_enabled: bool,
    pub(super) focus_process_memory_priority: ProcessMemoryPrioritySetting,
    pub(super) visible_window_memory_priority: ProcessMemoryPrioritySetting,
    pub(super) background_memory_priority: ProcessMemoryPrioritySetting,
    pub(super) limit_background_processors_enabled: bool,
    pub(super) dynamic_resource_zones_enabled: bool,
    pub(super) background_processor_selection: BackgroundProcessorSelection,
    pub(super) processor_limit_percent: u8,
    pub(super) foreground_or_system_cpu_threshold_percent: u8,
    pub(super) background_app_cpu_threshold_percent: u8,
    pub(super) cpu_recovery_threshold_percent: u8,
    pub(super) reaction_time_ms: u64,
    pub(super) restraint_trigger_time_ms: u64,
    pub(super) cpu_restraint_time_seconds: u64,
    pub(super) cpu_recovery_time_seconds: u64,
    pub(super) maximum_restrained_apps: u8,
}

pub(super) fn adaptive_engine_process_preset_values(
    preset: BuiltInAdaptiveEnginePreset,
) -> AdaptiveEngineProcessPresetValues {
    match preset {
        BuiltInAdaptiveEnginePreset::PowerSave | BuiltInAdaptiveEnginePreset::Balanced => {
            AdaptiveEngineProcessPresetValues {
                process_priority_enabled: true,
                background_efficiency_enabled: true,
                background_efficiency_mode: true,
                focus_process_priority: ProcessPrioritySetting::AboveNormal,
                background_priority: ProcessPrioritySetting::BelowNormal,
                visible_window_priority: ProcessPrioritySetting::Normal,
                focus_process_io_priority: ProcessIoPrioritySetting::Normal,
                visible_window_io_priority: ProcessIoPrioritySetting::Low,
                io_priority_enabled: false,
                background_io_priority: ProcessIoPriority::Low,
                memory_priority_enabled: false,
                focus_process_memory_priority: ProcessMemoryPrioritySetting::Default,
                visible_window_memory_priority: ProcessMemoryPrioritySetting::Default,
                background_memory_priority: ProcessMemoryPrioritySetting::Low,
                limit_background_processors_enabled: true,
                dynamic_resource_zones_enabled: false,
                background_processor_selection: BackgroundProcessorSelection::LeastUsed,
                processor_limit_percent: 60,
                foreground_or_system_cpu_threshold_percent: 75,
                background_app_cpu_threshold_percent: 10,
                cpu_recovery_threshold_percent: 5,
                reaction_time_ms: 1_500,
                restraint_trigger_time_ms: 1_500,
                cpu_restraint_time_seconds: 2,
                cpu_recovery_time_seconds: 4,
                maximum_restrained_apps: 4,
            }
        }
        BuiltInAdaptiveEnginePreset::Performance => AdaptiveEngineProcessPresetValues {
            process_priority_enabled: true,
            background_efficiency_enabled: true,
            background_efficiency_mode: true,
            focus_process_priority: ProcessPrioritySetting::AboveNormal,
            background_priority: ProcessPrioritySetting::BelowNormal,
            visible_window_priority: ProcessPrioritySetting::Normal,
            focus_process_io_priority: ProcessIoPrioritySetting::Normal,
            visible_window_io_priority: ProcessIoPrioritySetting::Low,
            io_priority_enabled: true,
            background_io_priority: ProcessIoPriority::VeryLow,
            memory_priority_enabled: true,
            focus_process_memory_priority: ProcessMemoryPrioritySetting::Normal,
            visible_window_memory_priority: ProcessMemoryPrioritySetting::BelowNormal,
            background_memory_priority: ProcessMemoryPrioritySetting::VeryLow,
            limit_background_processors_enabled: true,
            dynamic_resource_zones_enabled: true,
            background_processor_selection: BackgroundProcessorSelection::LeastUsed,
            processor_limit_percent: 75,
            foreground_or_system_cpu_threshold_percent: 60,
            background_app_cpu_threshold_percent: 8,
            cpu_recovery_threshold_percent: 4,
            reaction_time_ms: 750,
            restraint_trigger_time_ms: 750,
            cpu_restraint_time_seconds: 3,
            cpu_recovery_time_seconds: 5,
            maximum_restrained_apps: 8,
        },
        BuiltInAdaptiveEnginePreset::Speed => AdaptiveEngineProcessPresetValues {
            process_priority_enabled: true,
            background_efficiency_enabled: true,
            background_efficiency_mode: true,
            focus_process_priority: ProcessPrioritySetting::AboveNormal,
            background_priority: ProcessPrioritySetting::Idle,
            visible_window_priority: ProcessPrioritySetting::BelowNormal,
            focus_process_io_priority: ProcessIoPrioritySetting::High,
            visible_window_io_priority: ProcessIoPrioritySetting::Normal,
            io_priority_enabled: true,
            background_io_priority: ProcessIoPriority::VeryLow,
            memory_priority_enabled: true,
            focus_process_memory_priority: ProcessMemoryPrioritySetting::Normal,
            visible_window_memory_priority: ProcessMemoryPrioritySetting::Medium,
            background_memory_priority: ProcessMemoryPrioritySetting::VeryLow,
            limit_background_processors_enabled: true,
            dynamic_resource_zones_enabled: false,
            background_processor_selection: BackgroundProcessorSelection::LeastUsed,
            processor_limit_percent: 75,
            foreground_or_system_cpu_threshold_percent: 35,
            background_app_cpu_threshold_percent: 4,
            cpu_recovery_threshold_percent: 2,
            reaction_time_ms: 500,
            restraint_trigger_time_ms: 500,
            cpu_restraint_time_seconds: 5,
            cpu_recovery_time_seconds: 8,
            maximum_restrained_apps: 12,
        },
    }
}

pub(super) fn io_priority_preset_values(
    values: AdaptiveEngineProcessPresetValues,
) -> IoPrioritySettings {
    IoPrioritySettings {
        enabled: values.io_priority_enabled,
        foreground_detection_enabled: true,
        foreground_priority: values.focus_process_io_priority,
        visible_window_detection_enabled: true,
        visible_window_priority: values.visible_window_io_priority,
        background_priority: values.background_io_priority.into(),
        preserve_foreground_priority: true,
        preserve_visible_window_priority: true,
        preserve_background_priority: true,
        exclusions: Vec::new(),
    }
}

pub(super) fn thread_priority_preset_values(
    preset: BuiltInAdaptiveEnginePreset,
) -> ThreadPrioritySettings {
    ThreadPrioritySettings {
        enabled: matches!(
            preset,
            BuiltInAdaptiveEnginePreset::Performance | BuiltInAdaptiveEnginePreset::Speed
        ),
        foreground_detection_enabled: true,
        foreground_priority: if preset == BuiltInAdaptiveEnginePreset::Speed {
            ProcessThreadPrioritySetting::Highest
        } else {
            ProcessThreadPrioritySetting::Default
        },
        visible_window_detection_enabled: true,
        visible_window_priority: if preset == BuiltInAdaptiveEnginePreset::Speed {
            ProcessThreadPrioritySetting::Normal
        } else {
            ProcessThreadPrioritySetting::Default
        },
        background_priority: if preset == BuiltInAdaptiveEnginePreset::Speed {
            ProcessThreadPrioritySetting::Idle
        } else {
            ProcessThreadPrioritySetting::BelowNormal
        },
        preserve_foreground_priority: true,
        preserve_visible_window_priority: true,
        preserve_background_priority: true,
        exclusions: Vec::new(),
    }
}

pub(super) fn dynamic_priority_boost_preset_values(
    preset: BuiltInAdaptiveEnginePreset,
) -> DynamicPriorityBoostSettings {
    DynamicPriorityBoostSettings {
        enabled: matches!(
            preset,
            BuiltInAdaptiveEnginePreset::Performance | BuiltInAdaptiveEnginePreset::Speed
        ),
        foreground_detection_enabled: true,
        foreground_boost: ProcessDynamicPriorityBoostSetting::Enabled,
        visible_window_detection_enabled: true,
        visible_window_boost: ProcessDynamicPriorityBoostSetting::Default,
        background_boost: ProcessDynamicPriorityBoostSetting::Disabled,
        exclusions: Vec::new(),
    }
}

pub(super) fn gpu_priority_preset_values(
    preset: BuiltInAdaptiveEnginePreset,
) -> GpuPrioritySettings {
    GpuPrioritySettings {
        enabled: matches!(
            preset,
            BuiltInAdaptiveEnginePreset::Performance | BuiltInAdaptiveEnginePreset::Speed
        ),
        foreground_detection_enabled: true,
        foreground_priority: if preset == BuiltInAdaptiveEnginePreset::Speed {
            ProcessGpuPrioritySetting::High
        } else {
            ProcessGpuPrioritySetting::Default
        },
        visible_window_detection_enabled: true,
        visible_window_priority: if preset == BuiltInAdaptiveEnginePreset::Speed {
            ProcessGpuPrioritySetting::Normal
        } else {
            ProcessGpuPrioritySetting::Default
        },
        background_priority: if preset == BuiltInAdaptiveEnginePreset::Speed {
            ProcessGpuPrioritySetting::Idle
        } else {
            ProcessGpuPrioritySetting::BelowNormal
        },
        preserve_foreground_priority: true,
        preserve_visible_window_priority: true,
        preserve_background_priority: true,
        exclusions: Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_presets_leave_live_processor_power_unchanged() {
        let mut settings = Settings::default();
        settings.adaptive_engine.enabled = true;
        settings.adaptive_engine.adaptive_processor_power_enabled = true;
        settings
            .adaptive_engine
            .base_processor_policy
            .performance_min = 37;
        settings
            .adaptive_engine
            .background_pressure_profile
            .ac_policy = 13;
        settings
            .adaptive_engine
            .focus_and_launch_profile
            .battery_policy = 29;
        let before = settings.adaptive_engine.clone();
        for built_in in BuiltInAdaptiveEnginePreset::ALL {
            apply_built_in_adaptive_engine_preset(&mut settings, built_in);
            assert_eq!(settings.adaptive_engine, before);
        }
        let mut custom = capture_adaptive_engine_preset(&Settings::default(), "Saved".into());
        custom.processor_power_policy_enabled = true;
        custom.base_processor_policy.performance_min = 7;
        custom.adaptive_engine_process.reaction_time_ms = 1700;
        apply_adaptive_engine_preset(&mut settings, &custom);
        assert_eq!(settings.adaptive_engine, before);
        assert_eq!(settings.adaptive_engine_process.reaction_time_ms, 1700);
    }

    #[test]
    fn built_in_presets_set_restraint_timing_and_reset_advanced_controls() {
        for (preset, dwell_ms) in BuiltInAdaptiveEnginePreset::ALL
            .into_iter()
            .zip([1_500, 1_500, 750, 500])
        {
            let mut settings = AdaptiveEngineProcessSettings {
                pause_restraint_when_idle: true,
                restraint_idle_timeout_seconds: 120,
                pressure_source: AdaptivePressureSource::System,
                exclude_service_hosts: true,
                restraint_priority: AdaptiveRestraintPriority::Idle,
                maximum_restraint_time_seconds: 10,
                cpu_allocation_method: CpuAllocationMethod::ReduceAffinity,
                preserve_custom_affinity: true,
                processors_to_remove: 4,
                ..Default::default()
            };
            apply_adaptive_engine_process_preset(&mut settings, preset);
            assert_eq!(settings.restraint_trigger_time_ms, dwell_ms);
            assert!(!settings.pause_restraint_when_idle);
            assert_eq!(settings.restraint_idle_timeout_seconds, 300);
            assert_eq!(settings.pressure_source, AdaptivePressureSource::Either);
            assert!(!settings.exclude_service_hosts);
            assert_eq!(
                settings.cpu_allocation_method,
                CpuAllocationMethod::ReduceAffinity
            );
            assert!(!settings.preserve_custom_affinity);
            assert_eq!(settings.processors_to_remove, 1);
            assert_eq!(
                settings.restraint_priority,
                AdaptiveRestraintPriority::BelowNormal
            );
            assert_eq!(settings.maximum_restraint_time_seconds, 0);
        }
    }

    #[test]
    fn custom_presets_round_trip_process_options() {
        let mut settings = Settings::default();
        settings.adaptive_engine_process.restraint_trigger_time_ms = 2_700;
        settings.adaptive_engine_process.pause_restraint_when_idle = true;
        settings.adaptive_engine_process.pressure_source = AdaptivePressureSource::Both;
        settings.adaptive_engine_process.exclude_service_hosts = true;
        settings.adaptive_engine_process.cpu_allocation_method =
            CpuAllocationMethod::ReduceAffinity;
        settings.adaptive_engine_process.preserve_custom_affinity = true;
        settings.adaptive_engine_process.processors_to_remove = 4;
        settings.adaptive_engine_process.restraint_priority = AdaptiveRestraintPriority::Idle;
        settings
            .adaptive_engine_process
            .maximum_restraint_time_seconds = 10;
        settings
            .adaptive_engine_process
            .restraint_idle_timeout_seconds = 120;
        settings
            .adaptive_engine_process
            .process_priority_foreground_detection_enabled = false;
        settings
            .adaptive_engine_process
            .process_priority_visible_window_detection_enabled = false;
        settings
            .adaptive_engine_process
            .process_priority_preserve_foreground = true;
        settings
            .adaptive_engine_process
            .process_priority_preserve_visible_window = true;
        settings
            .adaptive_engine_process
            .process_priority_preserve_background = true;
        settings
            .adaptive_engine_process
            .memory_priority_foreground_detection_enabled = false;
        settings
            .adaptive_engine_process
            .memory_priority_visible_window_detection_enabled = false;
        settings
            .adaptive_engine_process
            .memory_priority_preserve_foreground = false;
        settings
            .adaptive_engine_process
            .memory_priority_preserve_visible_window = false;
        settings
            .adaptive_engine_process
            .memory_priority_preserve_background = false;
        let preset = capture_adaptive_engine_preset(&settings, "Custom".into());
        let encoded = toml::to_string(&preset).unwrap();
        let decoded: AdaptiveEnginePreset = toml::from_str(&encoded).unwrap();
        let mut restored = Settings::default();
        apply_adaptive_engine_preset(&mut restored, &decoded);
        assert_eq!(
            restored.adaptive_engine_process,
            settings.adaptive_engine_process
        );

        apply_adaptive_engine_process_preset(
            &mut restored.adaptive_engine_process,
            BuiltInAdaptiveEnginePreset::Balanced,
        );
        let defaults = AdaptiveEngineProcessSettings::default();
        assert_eq!(
            restored
                .adaptive_engine_process
                .process_priority_foreground_detection_enabled,
            defaults.process_priority_foreground_detection_enabled
        );
        assert!(
            !restored
                .adaptive_engine_process
                .process_priority_preserve_background
        );
        assert!(
            restored
                .adaptive_engine_process
                .memory_priority_visible_window_detection_enabled
        );
        assert!(
            restored
                .adaptive_engine_process
                .memory_priority_preserve_background
        );
    }
}
