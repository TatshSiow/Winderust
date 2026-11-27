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

    pub(super) const fn help_key(self) -> &'static str {
        match self {
            Self::PowerSave => "adaptive_engine.preset_power_save_help",
            Self::Balanced => "adaptive_engine.preset_balanced_help",
            Self::Performance => "adaptive_engine.preset_performance_help",
            Self::Speed => "adaptive_engine.preset_speed_help",
        }
    }
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
    settings
        .adaptive_engine_process
        .copy_cpu_behaviour_from(&preset.adaptive_engine_process);
}

pub(super) fn comparable_adaptive_engine_process_tuning(
    settings: &AdaptiveEngineProcessSettings,
) -> AdaptiveEngineProcessSettings {
    let mut tuning = AdaptiveEngineProcessSettings::default();
    tuning.copy_cpu_behaviour_from(settings);
    tuning
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
    use BuiltInAdaptiveEnginePreset::{Balanced, Performance, PowerSave, Speed};

    let tuning = match preset {
        PowerSave | Balanced => {
            let power_save = preset == PowerSave;
            let interval_ms = if power_save { 2_500 } else { 1_500 };
            AdaptiveEngineProcessSettings {
                cpu_pressure_restraint_enabled: true,
                processor_limit_percent: 60,
                pressure_source: AdaptivePressureSource::System,
                foreground_or_system_cpu_threshold_percent: if power_save { 85 } else { 75 },
                background_app_cpu_threshold_percent: 10,
                cpu_recovery_threshold_percent: 5,
                reaction_time_ms: interval_ms,
                restraint_trigger_time_ms: interval_ms,
                restraint_energy_efficient_scheduling: power_save,
                minimum_restraint_time_ms: 2_000,
                cpu_recovery_time_seconds: 3,
                ..Default::default()
            }
        }
        Performance => AdaptiveEngineProcessSettings {
            limit_background_processors_enabled: true,
            dynamic_resource_zones_enabled: true,
            processor_limit_percent: 90,
            foreground_or_system_cpu_threshold_percent: 60,
            background_app_cpu_threshold_percent: 8,
            cpu_recovery_threshold_percent: 4,
            reaction_time_ms: 750,
            restraint_trigger_time_ms: 750,
            cpu_recovery_time_seconds: 3,
            maximum_restrained_apps: 8,
            ..Default::default()
        },
        Speed => AdaptiveEngineProcessSettings {
            cpu_pressure_restraint_enabled: true,
            limit_background_processors_enabled: true,
            limit_background_processors_during_restraint: true,
            processor_limit_percent: 85,
            pressure_source: AdaptivePressureSource::System,
            foreground_or_system_cpu_threshold_percent: 35,
            background_app_cpu_threshold_percent: 8,
            cpu_recovery_threshold_percent: 7,
            reaction_time_ms: 500,
            restraint_trigger_time_ms: 500,
            minimum_restraint_time_ms: 5_000,
            cpu_recovery_time_seconds: 4,
            maximum_restrained_apps: 12,
            ..Default::default()
        },
    };
    settings.copy_cpu_behaviour_from(&tuning);
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
        for (
            preset,
            (
                dwell_ms,
                minimum_ms,
                recovery_secs,
                hot_percent,
                recovery_percent,
                system_threshold,
                processor_limit,
            ),
        ) in BuiltInAdaptiveEnginePreset::ALL.into_iter().zip([
            (2_500, 2_000, 3, 10, 5, Some(85), 60),
            (1_500, 2_000, 3, 10, 5, Some(75), 60),
            (750, 3_000, 3, 8, 4, None, 90),
            (500, 5_000, 4, 8, 7, Some(35), 85),
        ]) {
            let mut settings = AdaptiveEngineProcessSettings {
                cpu_pressure_restraint_enabled: system_threshold.is_none(),
                dynamic_resource_zones_enabled: true,
                pause_restraint_when_idle: true,
                restraint_idle_timeout_seconds: 120,
                pressure_source: AdaptivePressureSource::System,
                foreground_cpu_threshold_percent: Some(35),
                exclude_service_hosts: true,
                restraint_priority: AdaptiveRestraintPriority::ProcessorPlacementOnly,
                restraint_io_priority: AdaptiveRestraintIoPriority::Low,
                restraint_energy_efficient_scheduling: true,
                protect_focused_app: false,
                protect_focused_children: false,
                protect_non_normal_priority: false,
                minimum_restraint_time_ms: 2_100,
                maximum_restraint_time_ms: 10_000,
                cpu_allocation_method: CpuAllocationMethod::ReduceAffinity,
                specific_processors: crate::cpu_topology::CpuSelection::new(
                    vec![crate::cpu_topology::ProcessorId {
                        group: 0,
                        number: 1,
                    }],
                    None,
                )
                .unwrap(),
                limit_background_processors_during_restraint: true,
                preserve_custom_affinity: true,
                restore_child_affinity: true,
                restore_affinity_before_managed_restart: true,
                processors_to_remove: 4,
                ..Default::default()
            };
            apply_adaptive_engine_process_preset(&mut settings, preset);
            assert_eq!(settings.restraint_trigger_time_ms, dwell_ms);
            assert_eq!(settings.processor_limit_percent, processor_limit);
            assert_eq!(settings.background_app_cpu_threshold_percent, hot_percent);
            assert_eq!(settings.cpu_recovery_threshold_percent, recovery_percent);
            assert!(!settings.pause_restraint_when_idle);
            assert_eq!(settings.restraint_idle_timeout_seconds, 300);
            if let Some(threshold) = system_threshold {
                assert!(settings.cpu_pressure_restraint_enabled);
                assert_eq!(settings.pressure_source, AdaptivePressureSource::System);
                assert_eq!(
                    settings.foreground_or_system_cpu_threshold_percent,
                    threshold
                );
            } else {
                assert!(!settings.cpu_pressure_restraint_enabled);
                assert_eq!(settings.pressure_source, AdaptivePressureSource::Either);
            }
            assert_eq!(
                settings.limit_background_processors_enabled,
                matches!(
                    preset,
                    BuiltInAdaptiveEnginePreset::Performance | BuiltInAdaptiveEnginePreset::Speed
                )
            );
            assert_eq!(settings.reaction_time_ms, dwell_ms);
            assert_eq!(
                settings.dynamic_resource_zones_enabled,
                preset == BuiltInAdaptiveEnginePreset::Performance
            );
            assert_eq!(settings.foreground_cpu_threshold_percent, None);
            assert!(!settings.exclude_service_hosts);
            assert_eq!(
                settings.cpu_allocation_method,
                CpuAllocationMethod::CpuSetsSoft
            );
            assert!(settings.specific_processors.is_empty());
            assert!(!settings.preserve_custom_affinity);
            assert!(!settings.restore_child_affinity);
            assert!(!settings.restore_affinity_before_managed_restart);
            assert_eq!(settings.processors_to_remove, 1);
            assert_eq!(
                settings.restraint_priority,
                AdaptiveRestraintPriority::BelowNormal
            );
            assert_eq!(settings.minimum_restraint_time_ms, minimum_ms);
            assert_eq!(settings.cpu_recovery_time_seconds, recovery_secs);
            assert_eq!(settings.maximum_restraint_time_ms, 0);
            assert_eq!(
                settings.background_processor_limit_scoped(),
                preset == BuiltInAdaptiveEnginePreset::Speed
            );
            assert_eq!(
                settings.restraint_io_priority,
                AdaptiveRestraintIoPriority::Unchanged
            );
            assert_eq!(
                settings.restraint_energy_efficient_scheduling,
                preset == BuiltInAdaptiveEnginePreset::PowerSave
            );
            assert!(settings.protect_focused_app);
            assert!(settings.protect_focused_children);
            assert!(settings.protect_non_normal_priority);
        }
    }

    #[test]
    fn custom_presets_round_trip_process_options() {
        let mut settings = Settings::default();
        settings.adaptive_engine_process.restraint_trigger_time_ms = 2_700;
        settings.adaptive_engine_process.pause_restraint_when_idle = true;
        settings.adaptive_engine_process.pressure_source = AdaptivePressureSource::Both;
        settings
            .adaptive_engine_process
            .foreground_cpu_threshold_percent = Some(35);
        settings.adaptive_engine_process.exclude_service_hosts = true;
        settings
            .adaptive_engine_process
            .limit_background_processors_during_restraint = true;
        settings.adaptive_engine_process.cpu_allocation_method =
            CpuAllocationMethod::ReduceAffinity;
        settings.adaptive_engine_process.preserve_custom_affinity = true;
        settings
            .adaptive_engine_process
            .restore_affinity_before_managed_restart = true;
        settings.adaptive_engine_process.processors_to_remove = 4;
        settings.adaptive_engine_process.restraint_priority =
            AdaptiveRestraintPriority::ProcessorPlacementOnly;
        settings.adaptive_engine_process.restraint_io_priority =
            AdaptiveRestraintIoPriority::VeryLow;
        settings
            .adaptive_engine_process
            .restraint_energy_efficient_scheduling = true;
        settings.adaptive_engine_process.protect_focused_app = false;
        settings.adaptive_engine_process.protect_focused_children = false;
        settings.adaptive_engine_process.protect_non_normal_priority = false;
        settings.adaptive_engine_process.minimum_restraint_time_ms = 2_100;
        settings.adaptive_engine_process.maximum_restraint_time_ms = 10_000;
        settings
            .adaptive_engine_process
            .restraint_idle_timeout_seconds = 120;
        let preset = capture_adaptive_engine_preset(&settings, "Custom".into());
        let encoded = toml::to_string(&preset).unwrap();
        let decoded: AdaptiveEnginePreset = toml::from_str(&encoded).unwrap();
        let mut restored = Settings::default();
        apply_adaptive_engine_preset(&mut restored, &decoded);
        assert_eq!(
            restored.adaptive_engine_process,
            settings.adaptive_engine_process
        );
    }

    #[test]
    fn presets_ignore_inactive_nested_priority_controls_and_keep_exclusions() {
        let mut settings = Settings::default();
        apply_built_in_adaptive_engine_preset(&mut settings, BuiltInAdaptiveEnginePreset::Balanced);
        let original = capture_adaptive_engine_preset(&settings, "Custom".into());
        let live = &mut settings.adaptive_engine_process;
        live.process_priority_foreground_detection_enabled = false;
        live.thread_priority.enabled = true;
        live.custom_rules
            .push(AdaptiveEngineExclusionRule::exact_name("worker.exe"));
        let expected = live.clone();

        let captured = capture_adaptive_engine_preset(&settings, original.name.clone());
        assert_eq!(original, captured);

        apply_adaptive_engine_preset(&mut settings, &original);
        apply_built_in_adaptive_engine_preset(&mut settings, BuiltInAdaptiveEnginePreset::Balanced);
        assert_eq!(settings.adaptive_engine_process, expected);
    }
}
