use super::*;

pub(super) struct SuspensionIndicator {
    pub(super) label: String,
    pub(super) bg: u32,
    pub(super) fg: u32,
    pub(super) hover: String,
}

pub(super) struct AffinityIndicator {
    pub(super) label: String,
    pub(super) bg: u32,
    pub(super) fg: u32,
    pub(super) hover: String,
}

#[derive(Clone, Copy)]
pub(super) enum CoreTileGridAction {
    BackgroundCpuRestriction { available_mask: u64 },
    CoreSteeringRule { index: usize },
}

pub(super) fn app_suspension_indicator(
    status: &AppSuspensionSnapshot,
    process: &str,
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

pub(super) fn core_steering_indicator(
    status: &CoreSteeringSnapshot,
    process: &str,
) -> AffinityIndicator {
    let accent = accent_color();
    let accent_bg = settings_card_hover_color();
    if core_steering::is_builtin_excluded(process) {
        AffinityIndicator {
            label: t!("core_steering.indicator.protected").to_string(),
            bg: accent_bg,
            fg: accent,
            hover: t!("core_steering.indicator.protected_help").to_string(),
        }
    } else if core_steering::contains_process(&status.adjusted_apps, process) {
        AffinityIndicator {
            label: t!("core_steering.indicator.pinned").to_string(),
            bg: success_bg_color(),
            fg: success_text_color(),
            hover: t!("core_steering.indicator.pinned_help").to_string(),
        }
    } else if status.enabled {
        AffinityIndicator {
            label: t!("core_steering.indicator.ready").to_string(),
            bg: panel_active_color(),
            fg: muted_text_color(),
            hover: t!("core_steering.indicator.ready_help").to_string(),
        }
    } else {
        AffinityIndicator {
            label: t!("core_steering.indicator.off").to_string(),
            bg: panel_active_color(),
            fg: dim_text_color(),
            hover: t!("core_steering.indicator.off_help").to_string(),
        }
    }
}

pub(super) fn can_manual_freeze(status: &AppSuspensionSnapshot, process: &str) -> bool {
    status.enabled && !app_suspension::contains_process(&status.suspended_apps, process)
}

pub(super) fn logical_core_count() -> usize {
    core_steering::logical_processors().len().clamp(1, 64)
}

pub(super) fn action_log_mode_label(mode: ActionLogMode) -> String {
    match mode {
        ActionLogMode::Full => t!("settings.action_log_mode_full").to_string(),
        ActionLogMode::Warning => t!("settings.action_log_mode_warning").to_string(),
        ActionLogMode::Error => t!("settings.action_log_mode_error").to_string(),
        ActionLogMode::Off => t!("settings.action_log_mode_off").to_string(),
    }
}

pub(super) fn action_log_mode_help(mode: ActionLogMode) -> String {
    match mode {
        ActionLogMode::Full => t!("settings.action_log_mode_full_help").to_string(),
        ActionLogMode::Warning => t!("settings.action_log_mode_warning_help").to_string(),
        ActionLogMode::Error => t!("settings.action_log_mode_error_help").to_string(),
        ActionLogMode::Off => t!("settings.action_log_mode_off_help").to_string(),
    }
}

pub(super) fn cpu_restriction_mode_label(mode: CpuRestrictionMode) -> String {
    match mode {
        CpuRestrictionMode::SoftCpuSets => {
            t!("background_efficiency.cpu_restriction_soft").to_string()
        }
        CpuRestrictionMode::HardAffinity => {
            t!("background_efficiency.cpu_restriction_hard").to_string()
        }
    }
}

pub(super) fn cpu_restriction_strategy_label(strategy: CpuRestrictionStrategy) -> String {
    match strategy {
        CpuRestrictionStrategy::Off => t!("background_efficiency.cpu_set_off").to_string(),
        CpuRestrictionStrategy::Auto => t!("background_efficiency.cpu_set_auto").to_string(),
        CpuRestrictionStrategy::PreferEfficiencyCores => {
            t!("background_efficiency.cpu_set_prefer_e_cores").to_string()
        }
        CpuRestrictionStrategy::LimitLogicalCpus => {
            t!("background_efficiency.cpu_set_limit_logical").to_string()
        }
    }
}

pub(super) fn cpu_restriction_control_style_label(style: CpuRestrictionControlStyle) -> String {
    match style {
        CpuRestrictionControlStyle::Percentage => {
            t!("background_efficiency.control_style_percentage").to_string()
        }
        CpuRestrictionControlStyle::CoreToggle => {
            t!("background_efficiency.control_style_core_toggle").to_string()
        }
    }
}

pub(super) fn default_affinity_mask() -> u64 {
    let processors = core_steering::logical_processors();
    let mask = core_steering_processors_mask(&processors);
    if mask == 0 {
        let core_count = logical_core_count();
        if core_count >= 64 {
            u64::MAX
        } else {
            (1_u64 << core_count) - 1
        }
    } else {
        mask
    }
}

pub(super) fn affinity_mask_contains(mask: u64, core: usize) -> bool {
    core < 64 && (mask & (1_u64 << core)) != 0
}

pub(super) fn toggle_affinity_core(mask: &mut u64, core: usize) {
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

pub(super) fn toggle_affinity_core_with_available_mask(
    mask: &mut u64,
    core: usize,
    available_mask: u64,
) {
    *mask &= available_mask;
    let Some(bit) = core_steering_processor_bit(core) else {
        return;
    };
    if (available_mask & bit) == 0 {
        return;
    }

    if (*mask & bit) == 0 {
        *mask |= bit;
    } else if mask.count_ones() > 1 {
        *mask &= !bit;
    }
}

pub(super) fn core_steering_processors_mask(processors: &[LogicalProcessorInfo]) -> u64 {
    processors
        .iter()
        .filter_map(|processor| core_steering_processor_bit(processor.index))
        .fold(0, |mask, bit| mask | bit)
}

pub(super) fn core_steering_processors_kind_mask(
    processors: &[LogicalProcessorInfo],
    kind: LogicalProcessorKind,
) -> u64 {
    processors
        .iter()
        .filter(|processor| processor.kind == kind)
        .filter_map(|processor| core_steering_processor_bit(processor.index))
        .fold(0, |mask, bit| mask | bit)
}

pub(super) fn core_steering_processors_no_smt_mask(processors: &[LogicalProcessorInfo]) -> u64 {
    let mut seen_cores = Vec::new();
    let mut mask = 0;

    for processor in processors {
        if seen_cores.contains(&processor.core_index) {
            continue;
        }
        seen_cores.push(processor.core_index);
        if let Some(bit) = core_steering_processor_bit(processor.index) {
            mask |= bit;
        }
    }

    mask
}

pub(super) fn background_efficiency_strategy_core_mask(
    processors: &[LogicalProcessorInfo],
    strategy: CpuRestrictionStrategy,
) -> u64 {
    match strategy {
        CpuRestrictionStrategy::Off => 0,
        CpuRestrictionStrategy::Auto => {
            let efficiency_mask =
                core_steering_processors_kind_mask(processors, LogicalProcessorKind::Efficiency);
            if efficiency_mask != 0 {
                efficiency_mask
            } else {
                core_steering_processors_mask(processors)
            }
        }
        CpuRestrictionStrategy::PreferEfficiencyCores => {
            core_steering_processors_kind_mask(processors, LogicalProcessorKind::Efficiency)
        }
        CpuRestrictionStrategy::LimitLogicalCpus => core_steering_processors_mask(processors),
    }
}

pub(super) fn core_steering_processor_bit(index: usize) -> Option<u64> {
    (index < 64).then_some(1_u64 << index)
}

pub(super) fn core_tile_kind_label(processor: &LogicalProcessorInfo) -> String {
    match processor.kind {
        LogicalProcessorKind::Performance => "P-Core".to_owned(),
        LogicalProcessorKind::Efficiency => "E-Core".to_owned(),
        LogicalProcessorKind::Standard => "Core".to_owned(),
    }
}

pub(super) fn processor_power_preset_label(preset: ProcessorPowerPreset) -> String {
    match preset {
        ProcessorPowerPreset::Performance => t!("processor_power.performance").to_string(),
        ProcessorPowerPreset::Balanced => t!("processor_power.balanced").to_string(),
        ProcessorPowerPreset::Saver => t!("processor_power.saver").to_string(),
    }
}

pub(super) fn effective_power_mode_label(mode: EffectivePowerMode) -> String {
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

pub(super) fn processor_boost_mode_label(boost_mode: ProcessorBoostMode) -> String {
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

pub(super) const fn processor_boost_mode_picker_id(source: ProcessorPowerSource) -> &'static str {
    match source {
        ProcessorPowerSource::Ac => "processor-power-ac-boost-mode-picker",
        ProcessorPowerSource::Dc => "processor-power-dc-boost-mode-picker",
    }
}

pub(super) fn network_threshold_edit_value(
    threshold_bytes: u64,
    unit: NetworkThresholdUnit,
) -> String {
    let value = unit.threshold_value_from_bytes(threshold_bytes);
    network_threshold_value_label(value)
}

pub(super) fn network_threshold_value_label(value: f64) -> String {
    format!("{value:.3}")
        .trim_end_matches('0')
        .trim_end_matches('.')
        .to_owned()
}
