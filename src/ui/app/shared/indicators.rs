use crate::ui::app::*;

pub(in crate::ui::app) struct SuspensionIndicator {
    pub(in crate::ui::app) label: String,
    pub(in crate::ui::app) bg: u32,
    pub(in crate::ui::app) fg: u32,
    pub(in crate::ui::app) hover: String,
}

pub(in crate::ui::app) fn app_suspension_indicator(
    status: &AppSuspensionSnapshot,
    process: &str,
    unavailable: bool,
) -> SuspensionIndicator {
    let accent = accent_color();
    let accent_bg = settings_card_hover_color();
    if app_suspension::is_builtin_excluded(process) {
        SuspensionIndicator {
            label: t!("app_suspension.indicator.protected").to_string(),
            bg: accent_bg,
            fg: accent,
            hover: t!("app_suspension.indicator.protected_help").to_string(),
        }
    } else if unavailable {
        SuspensionIndicator {
            label: t!("app_suspension.indicator.unavailable").to_string(),
            bg: panel_active_color(),
            fg: muted_text_color(),
            hover: t!("app_suspension.indicator.unavailable_help").to_string(),
        }
    } else if app_suspension::contains_process(&status.network_wake_apps, process) {
        SuspensionIndicator {
            label: t!("app_suspension.indicator.network").to_string(),
            bg: accent_bg,
            fg: accent,
            hover: t!("app_suspension.indicator.network_help").to_string(),
        }
    } else if app_suspension::contains_process(&status.audio_wake_apps, process) {
        SuspensionIndicator {
            label: t!("app_suspension.indicator.audio").to_string(),
            bg: accent_bg,
            fg: accent,
            hover: t!("app_suspension.indicator.audio_help").to_string(),
        }
    } else if app_suspension::contains_process(&status.suspended_apps, process) {
        SuspensionIndicator {
            label: t!("app_suspension.indicator.frozen").to_string(),
            bg: success_bg_color(),
            fg: success_text_color(),
            hover: t!("app_suspension.indicator.frozen_help").to_string(),
        }
    } else if app_suspension::contains_process(&status.temporary_thawed_apps, process) {
        SuspensionIndicator {
            label: t!("app_suspension.indicator.thawed").to_string(),
            bg: accent_bg,
            fg: accent,
            hover: t!("app_suspension.indicator.thawed_help").to_string(),
        }
    } else if app_suspension::contains_process(&status.background_grace_apps, process) {
        SuspensionIndicator {
            label: t!("app_suspension.indicator.waiting").to_string(),
            bg: warning_bg_color(),
            fg: warning_text_color(),
            hover: t!("app_suspension.indicator.waiting_help").to_string(),
        }
    } else if status.status_unknown {
        SuspensionIndicator {
            label: t!("app_suspension.indicator.unknown").to_string(),
            bg: panel_active_color(),
            fg: muted_text_color(),
            hover: t!("app_suspension.indicator.unknown_help").to_string(),
        }
    } else if app_suspension::contains_process(&status.running_apps, process) {
        SuspensionIndicator {
            label: t!("app_suspension.indicator.running").to_string(),
            bg: panel_active_color(),
            fg: muted_text_color(),
            hover: t!("app_suspension.indicator.running_help").to_string(),
        }
    } else if status.enabled {
        SuspensionIndicator {
            label: t!("app_suspension.indicator.not_running").to_string(),
            bg: panel_active_color(),
            fg: muted_text_color(),
            hover: t!("app_suspension.indicator.not_running_help").to_string(),
        }
    } else {
        SuspensionIndicator {
            label: t!("app_suspension.indicator.off").to_string(),
            bg: panel_active_color(),
            fg: dim_text_color(),
            hover: t!("app_suspension.indicator.off_help").to_string(),
        }
    }
}

pub(in crate::ui::app) fn action_log_mode_label(mode: ActionLogMode) -> String {
    match mode {
        ActionLogMode::Full => t!("settings.action_log_mode_full").to_string(),
        ActionLogMode::Warning => t!("settings.action_log_mode_warning").to_string(),
        ActionLogMode::Error => t!("settings.action_log_mode_error").to_string(),
        ActionLogMode::Off => t!("settings.action_log_mode_off").to_string(),
    }
}

pub(in crate::ui::app) fn action_log_mode_help(mode: ActionLogMode) -> String {
    match mode {
        ActionLogMode::Full => t!("settings.action_log_mode_full_help").to_string(),
        ActionLogMode::Warning => t!("settings.action_log_mode_warning_help").to_string(),
        ActionLogMode::Error => t!("settings.action_log_mode_error_help").to_string(),
        ActionLogMode::Off => t!("settings.action_log_mode_off_help").to_string(),
    }
}

pub(in crate::ui::app) fn cpu_allocation_method_label(mode: CpuAllocationMethod) -> String {
    match mode {
        CpuAllocationMethod::CpuSetsSoft => {
            t!("background_efficiency.cpu_restriction_soft").to_string()
        }
        CpuAllocationMethod::ProcessorAffinityHard => {
            t!("background_efficiency.cpu_restriction_hard").to_string()
        }
    }
}

pub(in crate::ui::app) fn default_affinity_mask() -> u64 {
    cpu_allocation::default_cpu_mask()
}

pub(in crate::ui::app) fn affinity_mask_contains(mask: u64, core: usize) -> bool {
    core < 64 && (mask & (1_u64 << core)) != 0
}

pub(in crate::ui::app) fn toggle_affinity_core(mask: &mut u64, core: usize) {
    if core >= 64 {
        return;
    }

    let bit = 1_u64 << core;
    if (*mask & bit) == 0 {
        *mask |= bit;
    } else if mask.count_ones() > 1 {
        *mask &= !bit;
    }
}

pub(in crate::ui::app) fn toggle_specific_processor(processors: &mut Vec<u8>, index: usize) {
    let Ok(index) = u8::try_from(index) else {
        return;
    };
    if let Some(position) = processors.iter().position(|processor| *processor == index) {
        processors.remove(position);
    } else {
        processors.push(index);
        processors.sort_unstable();
    }
}

pub(in crate::ui::app) fn cpu_allocation_processors_mask(
    processors: &[LogicalProcessorInfo],
) -> u64 {
    cpu_allocation::logical_processor_mask(processors)
}

pub(in crate::ui::app) fn cpu_allocation_processors_kind_mask(
    processors: &[LogicalProcessorInfo],
    kind: LogicalProcessorKind,
) -> u64 {
    cpu_allocation::logical_processor_kind_mask(processors, kind)
}

pub(in crate::ui::app) fn cpu_allocation_processors_no_smt_mask(
    processors: &[LogicalProcessorInfo],
) -> u64 {
    cpu_allocation::logical_processor_no_smt_mask(processors)
}

pub(in crate::ui::app) fn core_tile_kind_label(processor: &LogicalProcessorInfo) -> String {
    match processor.kind {
        LogicalProcessorKind::Performance => t!("cpu_allocation.p_core").to_string(),
        LogicalProcessorKind::Efficiency => t!("cpu_allocation.e_core").to_string(),
        LogicalProcessorKind::Standard => t!("cpu_allocation.core").to_string(),
    }
}

pub(in crate::ui::app) fn processor_power_preset_label(preset: ProcessorPowerPreset) -> String {
    match preset {
        ProcessorPowerPreset::Performance => t!("processor_power.performance").to_string(),
        ProcessorPowerPreset::Balanced => t!("processor_power.balanced").to_string(),
        ProcessorPowerPreset::Saver => t!("processor_power.saver").to_string(),
    }
}

pub(in crate::ui::app) fn effective_power_mode_label(mode: EffectivePowerMode) -> String {
    match mode {
        EffectivePowerMode::Unknown => t!("processor_power.mode_unknown").to_string(),
        EffectivePowerMode::BatterySaver => t!("processor_power.mode_battery_saver").to_string(),
        EffectivePowerMode::BetterBattery => t!("processor_power.mode_better_battery").to_string(),
        EffectivePowerMode::Balanced => t!("processor_power.mode_balanced").to_string(),
        EffectivePowerMode::HighPerformance => {
            t!("processor_power.mode_high_performance").to_string()
        }
        EffectivePowerMode::MaxPerformance => {
            t!("processor_power.mode_max_performance").to_string()
        }
        EffectivePowerMode::GameMode => t!("processor_power.mode_game_mode").to_string(),
        EffectivePowerMode::MixedReality => t!("processor_power.mode_mixed_reality").to_string(),
    }
}

pub(in crate::ui::app) fn processor_boost_mode_label(boost_mode: ProcessorBoostMode) -> String {
    match boost_mode {
        ProcessorBoostMode::Disabled => t!("processor_power.boost_disabled").to_string(),
        ProcessorBoostMode::Enabled => t!("processor_power.boost_enabled").to_string(),
        ProcessorBoostMode::Aggressive => t!("processor_power.boost_aggressive").to_string(),
        ProcessorBoostMode::EfficientEnabled => {
            t!("processor_power.boost_efficient_enabled").to_string()
        }
        ProcessorBoostMode::EfficientAggressive => {
            t!("processor_power.boost_efficient_aggressive").to_string()
        }
        ProcessorBoostMode::AggressiveAtGuaranteed => {
            t!("processor_power.boost_aggressive_at_guaranteed").to_string()
        }
        ProcessorBoostMode::EfficientAggressiveAtGuaranteed => {
            t!("processor_power.boost_efficient_aggressive_at_guaranteed").to_string()
        }
    }
}

pub(in crate::ui::app) const fn processor_boost_mode_picker_id(
    source: ProcessorPowerSource,
) -> &'static str {
    match source {
        ProcessorPowerSource::Ac => "processor-power-ac-boost-mode-picker",
        ProcessorPowerSource::Battery => "processor-power-battery-boost-mode-picker",
    }
}

pub(in crate::ui::app) fn network_threshold_edit_value(
    threshold_bytes: u64,
    unit: NetworkThresholdUnit,
) -> String {
    let value = unit.threshold_value_from_bytes(threshold_bytes);
    network_threshold_value_label(value)
}

pub(in crate::ui::app) fn network_threshold_value_label(value: f64) -> String {
    format!("{value:.3}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned()
}
