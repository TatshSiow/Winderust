use crate::ui::app::*;

pub(in crate::ui::app) fn adaptive_engine_enabled(settings: &Settings) -> bool {
    settings.adaptive_engine.enabled
}

pub(in crate::ui::app) fn apply_adaptive_engine(settings: &mut Settings, enabled: bool) {
    settings.adaptive_engine.enabled = enabled;
}

pub(in crate::ui::app) fn built_in_adaptive_engine_preset_label(
    preset: BuiltInAdaptiveEnginePreset,
) -> String {
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

fn built_in_adaptive_engine_processor_policy(
    preset: BuiltInAdaptiveEnginePreset,
) -> ProcessorPowerValues {
    match preset {
        BuiltInAdaptiveEnginePreset::PowerSave => {
            ProcessorPowerValues::new_with_boost_mode(0, 5, 45, 0, ProcessorBoostMode::Disabled)
        }
        BuiltInAdaptiveEnginePreset::Balanced => ProcessorPowerValues::new_with_boost_mode(
            25,
            5,
            95,
            60,
            ProcessorBoostMode::EfficientEnabled,
        ),
        BuiltInAdaptiveEnginePreset::Performance => ProcessorPowerValues::new_with_boost_mode(
            100,
            25,
            100,
            85,
            ProcessorBoostMode::EfficientAggressive,
        ),
        BuiltInAdaptiveEnginePreset::Speed => ProcessorPowerValues::new_with_boost_mode(
            100,
            25,
            100,
            100,
            ProcessorBoostMode::Aggressive,
        ),
    }
}

pub(in crate::ui::app) fn apply_built_in_adaptive_engine_preset(
    settings: &mut Settings,
    preset: BuiltInAdaptiveEnginePreset,
) {
    settings.adaptive_engine.processor_power_policy_enabled = true;
    settings.adaptive_engine.base_processor_policy =
        built_in_adaptive_engine_processor_policy(preset);
    settings.adaptive_engine.background_pressure_profile =
        AdaptivePowerBoostValues::BACKGROUND_PRESSURE;
    settings.adaptive_engine.focus_and_launch_profile = AdaptivePowerBoostValues::FOCUS_AND_LAUNCH;
    apply_cpu_scheduler_preset(&mut settings.cpu_scheduler, preset);
}

pub(in crate::ui::app) fn matches_built_in_adaptive_engine_preset(
    settings: &Settings,
    preset: BuiltInAdaptiveEnginePreset,
) -> bool {
    settings
        .adaptive_engine
        .background_pressure_profile
        .normalized()
        == AdaptivePowerBoostValues::BACKGROUND_PRESSURE
        && settings
            .adaptive_engine
            .focus_and_launch_profile
            .normalized()
            == AdaptivePowerBoostValues::FOCUS_AND_LAUNCH
        && settings.adaptive_engine.processor_power_policy_enabled
        && settings.adaptive_engine.base_processor_policy.normalized()
            == built_in_adaptive_engine_processor_policy(preset)
        && cpu_scheduler_matches_preset(&settings.cpu_scheduler, preset)
}

pub(in crate::ui::app) fn capture_adaptive_engine_preset(
    settings: &Settings,
    name: String,
) -> AdaptiveEnginePreset {
    AdaptiveEnginePreset {
        name,
        processor_power_policy_enabled: settings.adaptive_engine.processor_power_policy_enabled,
        base_processor_policy: settings.adaptive_engine.base_processor_policy.normalized(),
        background_pressure_profile: settings
            .adaptive_engine
            .background_pressure_profile
            .normalized(),
        focus_and_launch_profile: settings
            .adaptive_engine
            .focus_and_launch_profile
            .normalized(),
        cpu_scheduler: comparable_cpu_scheduler_tuning(&settings.cpu_scheduler),
    }
}

pub(in crate::ui::app) fn built_in_adaptive_engine_preset(
    settings: &Settings,
    preset: BuiltInAdaptiveEnginePreset,
) -> AdaptiveEnginePreset {
    let mut settings = settings.clone();
    apply_built_in_adaptive_engine_preset(&mut settings, preset);
    capture_adaptive_engine_preset(&settings, built_in_adaptive_engine_preset_label(preset))
}

pub(in crate::ui::app) fn apply_adaptive_engine_preset(
    settings: &mut Settings,
    preset: &AdaptiveEnginePreset,
) {
    settings.adaptive_engine.processor_power_policy_enabled = preset.processor_power_policy_enabled;
    settings.adaptive_engine.base_processor_policy = preset.base_processor_policy.normalized();
    settings.adaptive_engine.background_pressure_profile =
        preset.background_pressure_profile.normalized();
    settings.adaptive_engine.focus_and_launch_profile =
        preset.focus_and_launch_profile.normalized();

    let cpu_pressure_restraint_enabled = settings.cpu_scheduler.cpu_pressure_restraint_enabled;
    let exclusions = std::mem::take(&mut settings.cpu_scheduler.custom_rules);
    let io_exclusions = std::mem::take(&mut settings.cpu_scheduler.io_priority.exclusions);
    let thread_exclusions = std::mem::take(&mut settings.cpu_scheduler.thread_priority.exclusions);
    let dynamic_boost_exclusions =
        std::mem::take(&mut settings.cpu_scheduler.dynamic_priority_boost.exclusions);
    let gpu_exclusions = std::mem::take(&mut settings.cpu_scheduler.gpu_priority.exclusions);
    settings.cpu_scheduler = preset.cpu_scheduler.clone();
    settings.cpu_scheduler.cpu_pressure_restraint_enabled = cpu_pressure_restraint_enabled;
    settings.cpu_scheduler.custom_rules = exclusions;
    settings.cpu_scheduler.io_priority.exclusions = io_exclusions;
    settings.cpu_scheduler.thread_priority.exclusions = thread_exclusions;
    settings.cpu_scheduler.dynamic_priority_boost.exclusions = dynamic_boost_exclusions;
    settings.cpu_scheduler.gpu_priority.exclusions = gpu_exclusions;
}

pub(in crate::ui::app) fn adaptive_engine_matches_preset(
    settings: &Settings,
    preset: &AdaptiveEnginePreset,
) -> bool {
    settings.adaptive_engine.processor_power_policy_enabled == preset.processor_power_policy_enabled
        && settings.adaptive_engine.base_processor_policy.normalized()
            == preset.base_processor_policy.normalized()
        && settings
            .adaptive_engine
            .background_pressure_profile
            .normalized()
            == preset.background_pressure_profile.normalized()
        && settings
            .adaptive_engine
            .focus_and_launch_profile
            .normalized()
            == preset.focus_and_launch_profile.normalized()
        && comparable_cpu_scheduler_tuning(&settings.cpu_scheduler)
            == comparable_cpu_scheduler_tuning(&preset.cpu_scheduler)
}

fn comparable_cpu_scheduler_tuning(settings: &CpuSchedulerSettings) -> CpuSchedulerSettings {
    let mut settings = settings.clone();
    settings.cpu_pressure_restraint_enabled = false;
    settings.custom_rules.clear();
    settings.io_priority.exclusions.clear();
    settings.thread_priority.exclusions.clear();
    settings.dynamic_priority_boost.exclusions.clear();
    settings.gpu_priority.exclusions.clear();
    settings
}

pub(in crate::ui::app) fn process_memory_priority_setting_label(
    priority: ProcessMemoryPrioritySetting,
) -> String {
    match priority {
        ProcessMemoryPrioritySetting::Default => t!("memory_priority.priority_default").to_string(),
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

pub(in crate::ui::app) fn background_processor_selection_label(
    selection: BackgroundProcessorSelection,
) -> String {
    match selection {
        BackgroundProcessorSelection::LeastUsed => {
            t!("cpu_scheduler.processor_selection_least_used_all").to_string()
        }
        BackgroundProcessorSelection::LeastUsedPerformanceCores => {
            t!("cpu_scheduler.processor_selection_least_used_p_cores").to_string()
        }
        BackgroundProcessorSelection::LeastUsedEfficiencyCores => {
            t!("cpu_scheduler.processor_selection_least_used_e_cores").to_string()
        }
        BackgroundProcessorSelection::PerformanceCores => t!("cpu_allocation.p_cores").to_string(),
        BackgroundProcessorSelection::EfficiencyCores => t!("cpu_allocation.e_cores").to_string(),
        BackgroundProcessorSelection::AllCoresNoSmt => {
            t!("cpu_allocation.all_cores_no_smt").to_string()
        }
        BackgroundProcessorSelection::PerformanceCoresNoSmt => {
            t!("cpu_allocation.p_cores_no_smt").to_string()
        }
        BackgroundProcessorSelection::EfficiencyCoresNoSmt => {
            t!("cpu_allocation.e_cores_no_smt").to_string()
        }
        BackgroundProcessorSelection::Custom => t!("cpu_allocation.custom").to_string(),
    }
}

pub(in crate::ui::app) fn apply_cpu_scheduler_preset(
    settings: &mut CpuSchedulerSettings,
    preset: BuiltInAdaptiveEnginePreset,
) {
    let values = cpu_scheduler_preset_values(preset);
    settings.process_priority_enabled = values.process_priority_enabled;
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
    settings.focus_process_memory_priority = values.focus_process_memory_priority;
    settings.visible_window_memory_priority = values.visible_window_memory_priority;
    settings.background_memory_priority = values.background_memory_priority;
    settings.limit_background_processors_enabled = values.limit_background_processors_enabled;
    settings.background_processor_selection = values.background_processor_selection;
    settings.processor_limit_percent = values.processor_limit_percent;
    settings.foreground_or_system_cpu_threshold_percent =
        values.foreground_or_system_cpu_threshold_percent;
    settings.background_app_cpu_threshold_percent = values.background_app_cpu_threshold_percent;
    settings.cpu_recovery_threshold_percent = values.cpu_recovery_threshold_percent;
    settings.reaction_time_ms = values.reaction_time_ms;
    settings.cpu_restraint_time_seconds = values.cpu_restraint_time_seconds;
    settings.cpu_recovery_time_seconds = values.cpu_recovery_time_seconds;
    settings.maximum_restrained_apps = values.maximum_restrained_apps;
}

pub(in crate::ui::app) fn cpu_scheduler_matches_preset(
    settings: &CpuSchedulerSettings,
    preset: BuiltInAdaptiveEnginePreset,
) -> bool {
    let values = cpu_scheduler_preset_values(preset);
    let mut io_priority = settings.io_priority.clone();
    io_priority.foreground_detection_enabled = true;
    io_priority.preserve_foreground_priority = true;
    io_priority.preserve_background_priority = true;
    let mut thread_priority = settings.thread_priority.clone();
    thread_priority.foreground_detection_enabled = true;
    thread_priority.preserve_foreground_priority = true;
    thread_priority.preserve_background_priority = true;
    let mut gpu_priority = settings.gpu_priority.clone();
    gpu_priority.foreground_detection_enabled = true;
    gpu_priority.preserve_foreground_priority = true;
    gpu_priority.preserve_background_priority = true;
    settings.process_priority_enabled == values.process_priority_enabled
        && settings.background_efficiency_enabled == values.background_efficiency_enabled
        && settings.focus_process_background_efficiency_override_enabled
        && settings.visible_window_background_efficiency_override_enabled
        && !settings.focus_process_background_efficiency_mode
        && !settings.visible_window_background_efficiency_mode
        && settings.background_efficiency_mode == values.background_efficiency_mode
        && settings.focus_process_priority == values.focus_process_priority
        && settings.background_priority == values.background_priority
        && settings.visible_window_priority == values.visible_window_priority
        && io_priority == io_priority_preset_values(values)
        && thread_priority == thread_priority_preset_values(preset)
        && settings.dynamic_priority_boost == dynamic_priority_boost_preset_values(preset)
        && gpu_priority == gpu_priority_preset_values(preset)
        && settings.memory_priority_enabled == values.memory_priority_enabled
        && settings.focus_process_memory_priority == values.focus_process_memory_priority
        && settings.visible_window_memory_priority == values.visible_window_memory_priority
        && settings.background_memory_priority == values.background_memory_priority
        && settings.limit_background_processors_enabled
            == values.limit_background_processors_enabled
        && settings.background_processor_selection == values.background_processor_selection
        && settings.processor_limit_percent == values.processor_limit_percent
        && settings.foreground_or_system_cpu_threshold_percent
            == values.foreground_or_system_cpu_threshold_percent
        && settings.background_app_cpu_threshold_percent
            == values.background_app_cpu_threshold_percent
        && settings.cpu_recovery_threshold_percent == values.cpu_recovery_threshold_percent
        && settings.reaction_time_ms == values.reaction_time_ms
        && settings.cpu_restraint_time_seconds == values.cpu_restraint_time_seconds
        && settings.cpu_recovery_time_seconds == values.cpu_recovery_time_seconds
        && settings.maximum_restrained_apps == values.maximum_restrained_apps
}

#[derive(Clone, Copy)]
pub(in crate::ui::app) struct CpuSchedulerPresetValues {
    pub(in crate::ui::app) process_priority_enabled: bool,
    pub(in crate::ui::app) background_efficiency_enabled: bool,
    pub(in crate::ui::app) background_efficiency_mode: bool,
    pub(in crate::ui::app) focus_process_priority: ProcessPrioritySetting,
    pub(in crate::ui::app) background_priority: ProcessPrioritySetting,
    pub(in crate::ui::app) visible_window_priority: ProcessPrioritySetting,
    pub(in crate::ui::app) focus_process_io_priority: ProcessIoPrioritySetting,
    pub(in crate::ui::app) visible_window_io_priority: ProcessIoPrioritySetting,
    pub(in crate::ui::app) io_priority_enabled: bool,
    pub(in crate::ui::app) background_io_priority: ProcessIoPriority,
    pub(in crate::ui::app) memory_priority_enabled: bool,
    pub(in crate::ui::app) focus_process_memory_priority: ProcessMemoryPrioritySetting,
    pub(in crate::ui::app) visible_window_memory_priority: ProcessMemoryPrioritySetting,
    pub(in crate::ui::app) background_memory_priority: ProcessMemoryPrioritySetting,
    pub(in crate::ui::app) limit_background_processors_enabled: bool,
    pub(in crate::ui::app) background_processor_selection: BackgroundProcessorSelection,
    pub(in crate::ui::app) processor_limit_percent: u8,
    pub(in crate::ui::app) foreground_or_system_cpu_threshold_percent: u8,
    pub(in crate::ui::app) background_app_cpu_threshold_percent: u8,
    pub(in crate::ui::app) cpu_recovery_threshold_percent: u8,
    pub(in crate::ui::app) reaction_time_ms: u64,
    pub(in crate::ui::app) cpu_restraint_time_seconds: u64,
    pub(in crate::ui::app) cpu_recovery_time_seconds: u64,
    pub(in crate::ui::app) maximum_restrained_apps: u8,
}

pub(in crate::ui::app) fn cpu_scheduler_preset_values(
    preset: BuiltInAdaptiveEnginePreset,
) -> CpuSchedulerPresetValues {
    match preset {
        BuiltInAdaptiveEnginePreset::PowerSave | BuiltInAdaptiveEnginePreset::Balanced => {
            CpuSchedulerPresetValues {
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
                background_processor_selection: BackgroundProcessorSelection::LeastUsed,
                processor_limit_percent: 60,
                foreground_or_system_cpu_threshold_percent: 75,
                background_app_cpu_threshold_percent: 10,
                cpu_recovery_threshold_percent: 5,
                reaction_time_ms: 1_500,
                cpu_restraint_time_seconds: 2,
                cpu_recovery_time_seconds: 4,
                maximum_restrained_apps: 4,
            }
        }
        BuiltInAdaptiveEnginePreset::Performance => CpuSchedulerPresetValues {
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
            background_processor_selection: BackgroundProcessorSelection::LeastUsed,
            processor_limit_percent: 16,
            foreground_or_system_cpu_threshold_percent: 60,
            background_app_cpu_threshold_percent: 8,
            cpu_recovery_threshold_percent: 4,
            reaction_time_ms: 750,
            cpu_restraint_time_seconds: 3,
            cpu_recovery_time_seconds: 5,
            maximum_restrained_apps: 8,
        },
        BuiltInAdaptiveEnginePreset::Speed => CpuSchedulerPresetValues {
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
            background_processor_selection: BackgroundProcessorSelection::LeastUsed,
            processor_limit_percent: 10,
            foreground_or_system_cpu_threshold_percent: 35,
            background_app_cpu_threshold_percent: 4,
            cpu_recovery_threshold_percent: 2,
            reaction_time_ms: 500,
            cpu_restraint_time_seconds: 5,
            cpu_recovery_time_seconds: 8,
            maximum_restrained_apps: 12,
        },
    }
}

pub(in crate::ui::app) fn io_priority_preset_values(
    values: CpuSchedulerPresetValues,
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

pub(in crate::ui::app) fn thread_priority_preset_values(
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

pub(in crate::ui::app) fn dynamic_priority_boost_preset_values(
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

pub(in crate::ui::app) fn gpu_priority_preset_values(
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
