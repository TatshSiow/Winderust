use crate::ui::app::*;

pub(in crate::ui::app) fn adaptive_engine_enabled(settings: &Settings) -> bool {
    settings.adaptive_engine.enabled
        || settings.background_efficiency.enabled
        || settings.workload_engine.enabled
}

pub(in crate::ui::app) fn app_tick_interval(
    settings: &Settings,
    start_minimized_applied: bool,
) -> Duration {
    if start_minimized_applied && settings.adaptive_engine.enabled {
        ADAPTIVE_ENGINE_APP_TICK_INTERVAL
    } else {
        APP_TICK_INTERVAL
    }
}

pub(in crate::ui::app) fn apply_adaptive_engine(settings: &mut Settings, enabled: bool) {
    settings.adaptive_engine.enabled = enabled;
    if !enabled {
        settings.background_efficiency.enabled = false;
        settings.workload_engine.enabled = false;
    }
}

pub(in crate::ui::app) fn power_mode_preset_label(preset: PowerModePreset) -> String {
    match preset {
        PowerModePreset::PowerSave => t!("adaptive_engine.power_mode_powersave").to_string(),
        PowerModePreset::Balanced => t!("adaptive_engine.power_mode_balanced").to_string(),
        PowerModePreset::Performance => t!("adaptive_engine.power_mode_performance").to_string(),
        PowerModePreset::Speed => t!("adaptive_engine.power_mode_speed").to_string(),
    }
}

pub(in crate::ui::app) fn workload_engine_preset_label(preset: WorkloadEnginePreset) -> String {
    match preset {
        WorkloadEnginePreset::LowImpact => t!("workload_engine.preset_low_impact").to_string(),
        WorkloadEnginePreset::ForegroundFirst => {
            t!("workload_engine.preset_foreground_first").to_string()
        }
        WorkloadEnginePreset::MaxForeground => {
            t!("workload_engine.preset_max_foreground").to_string()
        }
    }
}

pub(in crate::ui::app) fn apply_power_mode_preset(
    settings: &mut Settings,
    preset: PowerModePreset,
) {
    match preset {
        PowerModePreset::PowerSave => {
            apply_adaptive_engine(settings, true);
            settings.adaptive_engine.processor_policy_enabled = true;
            settings.adaptive_engine.processor_policy_values =
                power_mode_powersave_processor_values();
            settings.background_efficiency.enabled = true;
            settings.background_efficiency.exclude_foreground_app = true;
            settings.workload_engine.enabled = true;
            settings.workload_engine.workload_engine_enabled = true;
            apply_workload_engine_preset(
                &mut settings.workload_engine,
                WorkloadEnginePreset::LowImpact,
            );
        }
        PowerModePreset::Balanced => {
            apply_adaptive_engine(settings, true);
            settings.adaptive_engine.processor_policy_enabled = true;
            settings.adaptive_engine.processor_policy_values =
                power_mode_balanced_processor_values();
            settings.background_efficiency.enabled = false;
            settings.workload_engine.enabled = true;
            settings.workload_engine.workload_engine_enabled = true;
            apply_workload_engine_preset(
                &mut settings.workload_engine,
                WorkloadEnginePreset::LowImpact,
            );
        }
        PowerModePreset::Performance => {
            apply_adaptive_engine(settings, false);
            settings.adaptive_engine.processor_policy_enabled = true;
            settings.adaptive_engine.processor_policy_values =
                power_mode_performance_processor_values();
            settings.workload_engine.enabled = true;
            settings.workload_engine.workload_engine_enabled = true;
            apply_workload_engine_preset(
                &mut settings.workload_engine,
                WorkloadEnginePreset::ForegroundFirst,
            );
        }
        PowerModePreset::Speed => {
            apply_adaptive_engine(settings, false);
            settings.adaptive_engine.processor_policy_enabled = true;
            settings.adaptive_engine.processor_policy_values = power_mode_speed_processor_values();
            settings.workload_engine.enabled = true;
            settings.workload_engine.workload_engine_enabled = true;
            apply_workload_engine_preset(
                &mut settings.workload_engine,
                WorkloadEnginePreset::MaxForeground,
            );
        }
    }
}

pub(in crate::ui::app) fn power_mode_matches_preset(
    settings: &Settings,
    preset: PowerModePreset,
) -> bool {
    match preset {
        PowerModePreset::PowerSave => {
            settings.adaptive_engine.enabled
                && settings.adaptive_engine.processor_policy_enabled
                && settings
                    .adaptive_engine
                    .processor_policy_values
                    .normalized()
                    == power_mode_powersave_processor_values()
                && settings.background_efficiency.enabled
                && settings.workload_engine.enabled
                && settings.workload_engine.workload_engine_enabled
                && workload_engine_matches_preset(
                    &settings.workload_engine,
                    WorkloadEnginePreset::LowImpact,
                )
        }
        PowerModePreset::Balanced => {
            settings.adaptive_engine.enabled
                && settings.adaptive_engine.processor_policy_enabled
                && settings
                    .adaptive_engine
                    .processor_policy_values
                    .normalized()
                    == power_mode_balanced_processor_values()
                && !settings.background_efficiency.enabled
                && settings.workload_engine.enabled
                && settings.workload_engine.workload_engine_enabled
                && workload_engine_matches_preset(
                    &settings.workload_engine,
                    WorkloadEnginePreset::LowImpact,
                )
        }
        PowerModePreset::Performance => {
            !settings.adaptive_engine.enabled
                && !settings.background_efficiency.enabled
                && settings.adaptive_engine.processor_policy_enabled
                && settings
                    .adaptive_engine
                    .processor_policy_values
                    .normalized()
                    == power_mode_performance_processor_values()
                && settings.workload_engine.enabled
                && settings.workload_engine.workload_engine_enabled
                && workload_engine_matches_preset(
                    &settings.workload_engine,
                    WorkloadEnginePreset::ForegroundFirst,
                )
        }
        PowerModePreset::Speed => {
            !settings.adaptive_engine.enabled
                && !settings.background_efficiency.enabled
                && settings.adaptive_engine.processor_policy_enabled
                && settings
                    .adaptive_engine
                    .processor_policy_values
                    .normalized()
                    == power_mode_speed_processor_values()
                && settings.workload_engine.enabled
                && settings.workload_engine.workload_engine_enabled
                && workload_engine_matches_preset(
                    &settings.workload_engine,
                    WorkloadEnginePreset::MaxForeground,
                )
        }
    }
}

pub(in crate::ui::app) fn power_mode_powersave_processor_values() -> ProcessorPowerValues {
    ProcessorPowerValues::new_with_boost_mode(0, 5, 45, 0, ProcessorBoostMode::Disabled)
}

pub(in crate::ui::app) fn power_mode_balanced_processor_values() -> ProcessorPowerValues {
    ProcessorPowerValues::new_with_boost_mode(25, 5, 95, 60, ProcessorBoostMode::EfficientEnabled)
}

pub(in crate::ui::app) fn power_mode_performance_processor_values() -> ProcessorPowerValues {
    ProcessorPowerValues::new_with_boost_mode(
        100,
        25,
        100,
        85,
        ProcessorBoostMode::EfficientAggressive,
    )
}

pub(in crate::ui::app) fn power_mode_speed_processor_values() -> ProcessorPowerValues {
    ProcessorPowerValues::new_with_boost_mode(100, 25, 100, 100, ProcessorBoostMode::Aggressive)
}

pub(in crate::ui::app) fn process_memory_priority_setting_label(
    priority: ProcessMemoryPrioritySetting,
) -> String {
    match priority {
        ProcessMemoryPrioritySetting::Default => t!("memory_priority.priority_default").to_string(),
        ProcessMemoryPrioritySetting::Auto => t!("workload_engine.priority_auto").to_string(),
        ProcessMemoryPrioritySetting::VeryLow => {
            t!("memory_priority.priority_very_low").to_string()
        }
        ProcessMemoryPrioritySetting::Low => t!("memory_priority.priority_low").to_string(),
        ProcessMemoryPrioritySetting::Medium => t!("memory_priority.priority_medium").to_string(),
        ProcessMemoryPrioritySetting::BelowNormal => {
            t!("memory_priority.priority_below_normal").to_string()
        }
        ProcessMemoryPrioritySetting::Normal => t!("memory_priority.priority_normal").to_string(),
    }
}

pub(in crate::ui::app) fn core_steering_mode_label(mode: CoreSteeringMode) -> String {
    match mode {
        CoreSteeringMode::Hard => t!("core_steering.mode_hard").to_string(),
        CoreSteeringMode::Soft => t!("core_steering.mode_soft").to_string(),
        CoreSteeringMode::EfficiencyOff => t!("core_steering.mode_efficiency_off").to_string(),
    }
}

pub(in crate::ui::app) fn workload_engine_escalation_tuning_label(auto: bool) -> String {
    if auto {
        t!("workload_engine.priority_auto").to_string()
    } else {
        t!("workload_engine.priority_manual").to_string()
    }
}

pub(in crate::ui::app) fn apply_workload_engine_preset(
    settings: &mut WorkloadEngineSettings,
    preset: WorkloadEnginePreset,
) {
    let values = workload_engine_preset_values(preset);
    settings.lower_background_apps = values.lower_background_apps;
    settings.workload_engine_background_efficiency_enabled =
        values.workload_engine_background_efficiency_enabled;
    settings.workload_engine_background_priority = values.background_priority;
    settings.lower_background_io_priority_enabled = values.lower_background_io_priority_enabled;
    settings.lower_background_io_priority = values.lower_background_io_priority;
    settings.workload_engine_io_priority = workload_engine_io_priority_preset_values(values);
    settings.workload_engine_thread_priority =
        workload_engine_thread_priority_preset_values(preset);
    settings.workload_engine_dynamic_priority_boost =
        workload_engine_dynamic_priority_boost_preset_values(preset);
    settings.workload_engine_gpu_priority = workload_engine_gpu_priority_preset_values(preset);
    settings.workload_engine_memory_priority_enabled =
        values.workload_engine_memory_priority_enabled;
    settings.workload_engine_foreground_memory_priority =
        values.workload_engine_foreground_memory_priority;
    settings.workload_engine_memory_priority = values.workload_engine_memory_priority;
    settings.workload_engine_affinity_escalation_enabled =
        values.workload_engine_affinity_escalation_enabled;
    settings.boost_foreground_app = values.boost_foreground_app;
    if values.boost_foreground_app {
        settings.foreground_boost = values.foreground_boost;
    }
    settings.lower_background_auto_cpu_percent = values.lower_background_auto_cpu_percent;
    settings.workload_engine_cpu_percent = values.manual_cpu_percent;
    settings.workload_engine_total_threshold_percent = values.total_threshold;
    settings.workload_engine_threshold_percent = values.process_threshold;
    settings.workload_engine_restore_threshold_percent = values.restore_threshold;
    settings.workload_engine_sustain_seconds = values.sustain_seconds;
    settings.workload_engine_minimum_restraint_seconds = values.minimum_restraint_seconds;
    settings.workload_engine_cooldown_seconds = values.cooldown_seconds;
    settings.workload_engine_max_targeted_processes = values.max_targeted_processes;
}

pub(in crate::ui::app) fn workload_engine_matches_preset(
    settings: &WorkloadEngineSettings,
    preset: WorkloadEnginePreset,
) -> bool {
    let values = workload_engine_preset_values(preset);
    let mut io_priority = settings.workload_engine_io_priority.clone();
    io_priority.foreground_detection_enabled = true;
    io_priority.preserve_foreground_priority = true;
    io_priority.preserve_background_priority = true;
    let mut thread_priority = settings.workload_engine_thread_priority.clone();
    thread_priority.foreground_detection_enabled = true;
    thread_priority.preserve_foreground_priority = true;
    thread_priority.preserve_background_priority = true;
    let mut gpu_priority = settings.workload_engine_gpu_priority.clone();
    gpu_priority.foreground_detection_enabled = true;
    gpu_priority.preserve_foreground_priority = true;
    gpu_priority.preserve_background_priority = true;
    settings.lower_background_apps == values.lower_background_apps
        && settings.workload_engine_background_efficiency_enabled
            == values.workload_engine_background_efficiency_enabled
        && settings.workload_engine_background_priority == values.background_priority
        && settings.lower_background_io_priority_enabled
            == values.lower_background_io_priority_enabled
        && settings.lower_background_io_priority == values.lower_background_io_priority
        && io_priority == workload_engine_io_priority_preset_values(values)
        && thread_priority == workload_engine_thread_priority_preset_values(preset)
        && settings.workload_engine_dynamic_priority_boost
            == workload_engine_dynamic_priority_boost_preset_values(preset)
        && gpu_priority == workload_engine_gpu_priority_preset_values(preset)
        && settings.workload_engine_memory_priority_enabled
            == values.workload_engine_memory_priority_enabled
        && settings.workload_engine_foreground_memory_priority
            == values.workload_engine_foreground_memory_priority
        && settings.workload_engine_memory_priority == values.workload_engine_memory_priority
        && settings.workload_engine_affinity_escalation_enabled
            == values.workload_engine_affinity_escalation_enabled
        && settings.boost_foreground_app == values.boost_foreground_app
        && (!values.boost_foreground_app || settings.foreground_boost == values.foreground_boost)
        && settings.lower_background_auto_cpu_percent == values.lower_background_auto_cpu_percent
        && settings.workload_engine_cpu_percent == values.manual_cpu_percent
        && settings.workload_engine_total_threshold_percent == values.total_threshold
        && settings.workload_engine_threshold_percent == values.process_threshold
        && settings.workload_engine_restore_threshold_percent == values.restore_threshold
        && settings.workload_engine_sustain_seconds == values.sustain_seconds
        && settings.workload_engine_minimum_restraint_seconds == values.minimum_restraint_seconds
        && settings.workload_engine_cooldown_seconds == values.cooldown_seconds
        && settings.workload_engine_max_targeted_processes == values.max_targeted_processes
}

#[derive(Clone, Copy)]
pub(in crate::ui::app) struct WorkloadEnginePresetValues {
    pub(in crate::ui::app) lower_background_apps: bool,
    pub(in crate::ui::app) workload_engine_background_efficiency_enabled: bool,
    pub(in crate::ui::app) background_priority: ProcessPriority,
    pub(in crate::ui::app) foreground_io_priority: ProcessIoPrioritySetting,
    pub(in crate::ui::app) lower_background_io_priority_enabled: bool,
    pub(in crate::ui::app) lower_background_io_priority: ProcessIoPriority,
    pub(in crate::ui::app) workload_engine_memory_priority_enabled: bool,
    pub(in crate::ui::app) workload_engine_foreground_memory_priority: ProcessMemoryPrioritySetting,
    pub(in crate::ui::app) workload_engine_memory_priority: ProcessMemoryPriority,
    pub(in crate::ui::app) workload_engine_affinity_escalation_enabled: bool,
    pub(in crate::ui::app) boost_foreground_app: bool,
    pub(in crate::ui::app) foreground_boost: ForegroundBoostPriority,
    pub(in crate::ui::app) lower_background_auto_cpu_percent: bool,
    pub(in crate::ui::app) manual_cpu_percent: u8,
    pub(in crate::ui::app) total_threshold: u8,
    pub(in crate::ui::app) process_threshold: u8,
    pub(in crate::ui::app) restore_threshold: u8,
    pub(in crate::ui::app) sustain_seconds: u64,
    pub(in crate::ui::app) minimum_restraint_seconds: u64,
    pub(in crate::ui::app) cooldown_seconds: u64,
    pub(in crate::ui::app) max_targeted_processes: u8,
}

pub(in crate::ui::app) fn workload_engine_preset_values(
    preset: WorkloadEnginePreset,
) -> WorkloadEnginePresetValues {
    match preset {
        WorkloadEnginePreset::LowImpact => WorkloadEnginePresetValues {
            lower_background_apps: true,
            workload_engine_background_efficiency_enabled: true,
            background_priority: ProcessPriority::Idle,
            foreground_io_priority: ProcessIoPrioritySetting::Normal,
            lower_background_io_priority_enabled: true,
            lower_background_io_priority: ProcessIoPriority::Low,
            workload_engine_memory_priority_enabled: true,
            workload_engine_foreground_memory_priority: ProcessMemoryPrioritySetting::Default,
            workload_engine_memory_priority: ProcessMemoryPriority::Low,
            workload_engine_affinity_escalation_enabled: true,
            boost_foreground_app: true,
            foreground_boost: ForegroundBoostPriority::Auto,
            lower_background_auto_cpu_percent: true,
            manual_cpu_percent: 60,
            total_threshold: 70,
            process_threshold: 8,
            restore_threshold: 4,
            sustain_seconds: 2,
            minimum_restraint_seconds: 2,
            cooldown_seconds: 5,
            max_targeted_processes: 12,
        },
        WorkloadEnginePreset::ForegroundFirst => WorkloadEnginePresetValues {
            lower_background_apps: true,
            workload_engine_background_efficiency_enabled: true,
            background_priority: ProcessPriority::Idle,
            foreground_io_priority: ProcessIoPrioritySetting::Normal,
            lower_background_io_priority_enabled: true,
            lower_background_io_priority: ProcessIoPriority::VeryLow,
            workload_engine_memory_priority_enabled: true,
            workload_engine_foreground_memory_priority: ProcessMemoryPrioritySetting::Normal,
            workload_engine_memory_priority: ProcessMemoryPriority::Low,
            workload_engine_affinity_escalation_enabled: true,
            boost_foreground_app: true,
            foreground_boost: ForegroundBoostPriority::Auto,
            lower_background_auto_cpu_percent: true,
            manual_cpu_percent: 16,
            total_threshold: 45,
            process_threshold: 6,
            restore_threshold: 3,
            sustain_seconds: 1,
            minimum_restraint_seconds: 4,
            cooldown_seconds: 6,
            max_targeted_processes: 12,
        },
        WorkloadEnginePreset::MaxForeground => WorkloadEnginePresetValues {
            lower_background_apps: true,
            workload_engine_background_efficiency_enabled: true,
            background_priority: ProcessPriority::Idle,
            foreground_io_priority: ProcessIoPrioritySetting::High,
            lower_background_io_priority_enabled: true,
            lower_background_io_priority: ProcessIoPriority::VeryLow,
            workload_engine_memory_priority_enabled: true,
            workload_engine_foreground_memory_priority: ProcessMemoryPrioritySetting::Normal,
            workload_engine_memory_priority: ProcessMemoryPriority::VeryLow,
            workload_engine_affinity_escalation_enabled: true,
            boost_foreground_app: true,
            foreground_boost: ForegroundBoostPriority::AboveNormal,
            lower_background_auto_cpu_percent: false,
            manual_cpu_percent: 6,
            total_threshold: 35,
            process_threshold: 4,
            restore_threshold: 2,
            sustain_seconds: 1,
            minimum_restraint_seconds: 5,
            cooldown_seconds: 8,
            max_targeted_processes: 12,
        },
    }
}

pub(in crate::ui::app) fn workload_engine_io_priority_preset_values(
    values: WorkloadEnginePresetValues,
) -> IoPrioritySettings {
    IoPrioritySettings {
        enabled: values.lower_background_io_priority_enabled,
        foreground_detection_enabled: true,
        foreground_priority: values.foreground_io_priority,
        background_priority: values.lower_background_io_priority.into(),
        preserve_foreground_priority: true,
        preserve_background_priority: true,
        exclusions: Vec::new(),
    }
}

pub(in crate::ui::app) fn workload_engine_thread_priority_preset_values(
    preset: WorkloadEnginePreset,
) -> ThreadPrioritySettings {
    ThreadPrioritySettings {
        enabled: true,
        foreground_detection_enabled: true,
        foreground_priority: if preset == WorkloadEnginePreset::MaxForeground {
            ProcessThreadPrioritySetting::Highest
        } else {
            ProcessThreadPrioritySetting::Default
        },
        background_priority: if preset == WorkloadEnginePreset::MaxForeground {
            ProcessThreadPrioritySetting::Idle
        } else {
            ProcessThreadPrioritySetting::BelowNormal
        },
        preserve_foreground_priority: true,
        preserve_background_priority: true,
        exclusions: Vec::new(),
    }
}

pub(in crate::ui::app) fn workload_engine_dynamic_priority_boost_preset_values(
    _preset: WorkloadEnginePreset,
) -> DynamicPriorityBoostSettings {
    DynamicPriorityBoostSettings {
        enabled: true,
        foreground_detection_enabled: true,
        foreground_boost: ProcessDynamicPriorityBoostSetting::Enabled,
        background_boost: ProcessDynamicPriorityBoostSetting::Disabled,
        exclusions: Vec::new(),
    }
}

pub(in crate::ui::app) fn workload_engine_gpu_priority_preset_values(
    preset: WorkloadEnginePreset,
) -> GpuPrioritySettings {
    GpuPrioritySettings {
        enabled: true,
        foreground_detection_enabled: true,
        foreground_priority: if preset == WorkloadEnginePreset::MaxForeground {
            ProcessGpuPrioritySetting::High
        } else {
            ProcessGpuPrioritySetting::Default
        },
        background_priority: if preset == WorkloadEnginePreset::MaxForeground {
            ProcessGpuPrioritySetting::Idle
        } else {
            ProcessGpuPrioritySetting::BelowNormal
        },
        preserve_foreground_priority: true,
        preserve_background_priority: true,
        exclusions: Vec::new(),
    }
}

pub(in crate::ui::app) fn foreground_boost_priority_label(
    priority: ForegroundBoostPriority,
) -> String {
    match priority {
        ForegroundBoostPriority::Auto => t!("workload_engine.priority_auto").to_string(),
        ForegroundBoostPriority::Normal => format!("8 ({})", t!("workload_engine.priority_normal")),
        ForegroundBoostPriority::AboveNormal => {
            format!("10 ({})", t!("workload_engine.priority_above_normal"))
        }
    }
}

pub(in crate::ui::app) fn background_efficiency_aggressiveness_label(
    aggressiveness: BackgroundEfficiencyAggressiveness,
) -> String {
    match aggressiveness {
        BackgroundEfficiencyAggressiveness::Safe => {
            t!("background_efficiency.aggressiveness_safe").to_string()
        }
        BackgroundEfficiencyAggressiveness::Balanced => {
            t!("background_efficiency.aggressiveness_balanced").to_string()
        }
        BackgroundEfficiencyAggressiveness::Aggressive => {
            t!("background_efficiency.aggressiveness_aggressive").to_string()
        }
    }
}
