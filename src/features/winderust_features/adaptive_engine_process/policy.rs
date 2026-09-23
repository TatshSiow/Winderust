use super::*;

pub fn is_builtin_excluded(process_name: &str) -> bool {
    contains_process_name(BUILT_IN_EXCLUSIONS, process_name)
}

pub(super) fn percent_tenths(usage: f32) -> u16 {
    (usage.clamp(0.0, 100.0) * 10.0).round() as u16
}

pub(super) fn should_skip_foreground_process(
    process_id: u32,
    foreground_process_id: Option<u32>,
    foreground_process_group_ids: &BTreeSet<u32>,
) -> bool {
    foreground_process_id.is_some_and(|id| id == process_id)
        || foreground_process_group_ids.contains(&process_id)
}

pub(super) fn focus_process_priority_eligible(
    process_id: u32,
    process_name: &str,
    current_process_id: u32,
    current_session_id: u32,
) -> bool {
    let process_name = process_name.trim();
    !process_name.is_empty()
        && process_id != 0
        && process_id != current_process_id
        && !is_builtin_excluded(process_name)
        && process_session_id(process_id) == Some(current_session_id)
}

pub(super) fn should_skip_process(
    process_id: u32,
    process_name: &str,
    current_process_id: u32,
    foreground_process_id: Option<u32>,
    foreground_process_group_ids: &BTreeSet<u32>,
    background_efficiency_process_ids: &BTreeSet<u32>,
) -> bool {
    process_id == 0
        || process_id == current_process_id
        || background_efficiency_process_ids.contains(&process_id)
        || is_builtin_excluded(process_name)
        || should_skip_foreground_process(
            process_id,
            foreground_process_id,
            foreground_process_group_ids,
        )
}

#[cfg(test)]
pub(super) use crate::runtime::observations::foreground_process_group_ids;

pub(super) fn cpu_pressure_restraint_should_run(
    settings: &AdaptiveEngineProcessSettings,
    foreground_cpu_usage_percent: Option<f32>,
    total_cpu_usage_percent: Option<f32>,
) -> bool {
    if !settings.cpu_pressure_restraint_enabled
        && !settings.limit_background_processors_enabled
        && !settings.dynamic_resource_zones_enabled
    {
        return false;
    }
    let threshold = f32::from(settings.foreground_or_system_cpu_threshold_percent.min(100));
    foreground_cpu_usage_percent.is_some_and(|usage| usage >= threshold)
        || total_cpu_usage_percent.is_some_and(|usage| usage >= threshold)
}

pub(super) fn cpu_pressure_above_recovery_threshold(
    settings: &AdaptiveEngineProcessSettings,
    foreground_cpu_usage_percent: Option<f32>,
    total_cpu_usage_percent: Option<f32>,
) -> bool {
    let trigger = settings.foreground_or_system_cpu_threshold_percent.min(100);
    let recovery_threshold =
        f32::from(trigger.saturating_sub(ADAPTIVE_ENGINE_PROCESS_RECOVERY_BAND_PERCENT));
    foreground_cpu_usage_percent.is_some_and(|usage| usage >= recovery_threshold)
        || total_cpu_usage_percent.is_some_and(|usage| usage >= recovery_threshold)
}

pub(super) fn cpu_allocation_method(settings: &AdaptiveEngineProcessSettings) -> CpuAllocationMode {
    match settings.cpu_allocation_method {
        CpuAllocationMethod::CpuSetsSoft => CpuAllocationMode::SoftCpuSets,
        CpuAllocationMethod::ProcessorAffinityHard => CpuAllocationMode::HardAffinity,
    }
}

pub(super) fn adaptive_engine_process_process_decision(
    settings: &AdaptiveEngineProcessSettings,
    tier: AdaptiveEngineProcessTier,
) -> AdaptiveEngineProcessDecision {
    if settings.limit_background_processors_enabled && tier == AdaptiveEngineProcessTier::Background
    {
        AdaptiveEngineProcessDecision::LimitProcessors
    } else {
        AdaptiveEngineProcessDecision::LowerPriority
    }
}

pub(super) fn cpu_pressure_restraint_target(
    settings: &AdaptiveEngineProcessSettings,
    tier: AdaptiveEngineProcessTier,
    background_efficiency_managed: bool,
) -> Option<PressureTargetPolicy> {
    let (priority, preservation) = process_priority_policy(
        settings,
        false,
        tier == AdaptiveEngineProcessTier::VisibleWindow,
    );
    let apply_background_efficiency = !background_efficiency_managed
        && settings.background_efficiency_enabled
        && settings.background_efficiency_mode_for(
            false,
            tier == AdaptiveEngineProcessTier::VisibleWindow,
        );

    (priority.is_some() || apply_background_efficiency).then_some(PressureTargetPolicy {
        priority,
        preservation,
        apply_background_efficiency,
    })
}

pub(super) fn adaptive_engine_process_candidate(
    process_id: u32,
    process: &AdaptiveEngineProcessProcess,
    decision: AdaptiveEngineProcessDecision,
    tier: AdaptiveEngineProcessTier,
) -> AdaptiveEngineProcessCandidate {
    AdaptiveEngineProcessCandidate {
        process_id,
        process_name: process.process_name.clone(),
        decision,
        tier,
        score: u32::from(process.last_usage_tenths.unwrap_or_default())
            + if process.selected {
                ADAPTIVE_ENGINE_PROCESS_SELECTION_STICKINESS_TENTHS
            } else {
                0
            },
    }
}

pub(super) fn select_adaptive_engine_process_candidates(
    mut candidates: Vec<AdaptiveEngineProcessCandidate>,
    maximum_restrained_apps: u8,
) -> Vec<AdaptiveEngineProcessCandidate> {
    candidates.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.process_id.cmp(&right.process_id))
    });
    candidates.truncate(usize::from(maximum_restrained_apps.max(1)));
    candidates
}

pub(super) fn selected_background_processor_mask(
    processors: &[LogicalProcessorInfo],
    selection: BackgroundProcessorSelection,
    specific_processors: &[u8],
) -> Option<u64> {
    let all_mask = cpu_allocation::logical_processor_mask(processors);
    let no_smt_mask = cpu_allocation::logical_processor_no_smt_mask(processors);
    let kind_mask = |kind| cpu_allocation::logical_processor_kind_mask(processors, kind);
    let mask = match selection {
        BackgroundProcessorSelection::LeastUsed
        | BackgroundProcessorSelection::LeastUsedPerformanceCores
        | BackgroundProcessorSelection::LeastUsedEfficiencyCores => return None,
        BackgroundProcessorSelection::PerformanceCores => {
            kind_mask(LogicalProcessorKind::Performance)
        }
        BackgroundProcessorSelection::EfficiencyCores => {
            kind_mask(LogicalProcessorKind::Efficiency)
        }
        BackgroundProcessorSelection::AllCoresNoSmt => no_smt_mask,
        BackgroundProcessorSelection::PerformanceCoresNoSmt => {
            kind_mask(LogicalProcessorKind::Performance) & no_smt_mask
        }
        BackgroundProcessorSelection::Custom => {
            cpu_allocation::logical_processor_indices_mask(specific_processors) & all_mask
        }
    };
    (mask != 0 && mask != all_mask).then_some(mask)
}

pub(super) fn load_aware_limited_core_mask(
    processors: &[LogicalProcessorInfo],
    usages: &[f32],
    percent: u8,
    kind: Option<LogicalProcessorKind>,
) -> Option<u64> {
    let mut candidates = processors
        .iter()
        .filter(|processor| kind.is_none_or(|kind| processor.kind == kind))
        .filter(|processor| processor.index < usages.len() && processor.index < u64::BITS as usize)
        .map(|processor| (processor.index, usages[processor.index]))
        .collect::<Vec<_>>();
    if candidates.is_empty() {
        return None;
    }

    candidates.sort_by(|(left_index, left_usage), (right_index, right_usage)| {
        left_usage
            .total_cmp(right_usage)
            .then_with(|| left_index.cmp(right_index))
    });
    let limit = (candidates.len() * usize::from(percent.clamp(1, 100)))
        .div_ceil(100)
        .clamp(1, candidates.len());
    let mask = candidates
        .into_iter()
        .take(limit)
        .fold(0_u64, |mask, (index, _usage)| mask | (1_u64 << index));
    (mask != 0).then_some(mask)
}

pub(super) fn average_masked_core_load(mask: u64, usages: &[f32]) -> Option<f32> {
    let mut total = 0.0;
    let mut count = 0usize;
    for (index, usage) in usages.iter().enumerate() {
        if index < u64::BITS as usize && (mask & (1_u64 << index)) != 0 {
            total += *usage;
            count += 1;
        }
    }
    (count > 0).then_some(total / count as f32)
}

pub(super) fn dynamic_background_zone_percent(foreground_percent: u8) -> u8 {
    100_u8.saturating_sub(foreground_percent.min(99)).max(1)
}

pub(super) fn dynamic_resource_zone_masks(
    all_mask: u64,
    background_mask: u64,
) -> Option<(u64, u64)> {
    let background_mask = background_mask & all_mask;
    let foreground_mask = all_mask & !background_mask;
    (foreground_mask != 0 && background_mask != 0).then_some((foreground_mask, background_mask))
}

pub(super) fn focus_and_launch_profile_eligible(process_id: u32) -> bool {
    process_age(process_id).is_some_and(|age| {
        process_age_in_focus_and_launch_window(age, FOCUS_AND_LAUNCH_PROFILE_WINDOW)
    })
}

pub(super) fn focus_and_launch_profile_enabled(
    settings: &AdaptiveEngineProcessSettings,
    focus_and_launch_profile_target: bool,
) -> bool {
    settings.cpu_pressure_restraint_enabled && focus_and_launch_profile_target
}

pub(super) fn process_age_in_focus_and_launch_window(
    age: Duration,
    profile_window: Duration,
) -> bool {
    age <= profile_window
}

pub(super) fn process_priority_policy(
    settings: &AdaptiveEngineProcessSettings,
    foreground: bool,
    visible_window: bool,
) -> (Option<PriorityClassValue>, PriorityClassPreservation) {
    let foreground = foreground && settings.process_priority_foreground_detection_enabled;
    let visible_window =
        visible_window && settings.process_priority_visible_window_detection_enabled;
    let (value, preserve) = if foreground {
        (
            settings.focus_process_priority,
            settings.process_priority_preserve_foreground,
        )
    } else if visible_window {
        (
            settings.visible_window_priority,
            settings.process_priority_preserve_visible_window,
        )
    } else {
        (
            settings.background_priority,
            settings.process_priority_preserve_background,
        )
    };
    let preservation = if !preserve {
        PriorityClassPreservation::PreserveHighOrRealtime
    } else if foreground || visible_window {
        PriorityClassPreservation::PreserveHigherOrHighOrRealtime
    } else {
        PriorityClassPreservation::PreserveLowerOrHighOrRealtime
    };
    (
        settings
            .process_priority_enabled
            .then(|| adaptive_engine_process_priority_value(value))
            .flatten(),
        preservation,
    )
}

pub(super) fn memory_priority_policy(
    settings: &AdaptiveEngineProcessSettings,
    foreground: bool,
    visible_window: bool,
) -> (crate::config::ProcessMemoryPrioritySetting, bool, bool) {
    let foreground = foreground && settings.memory_priority_foreground_detection_enabled;
    let visible_window =
        !foreground && visible_window && settings.memory_priority_visible_window_detection_enabled;
    let priority = if foreground {
        settings.focus_process_memory_priority
    } else if visible_window {
        settings.visible_window_memory_priority
    } else {
        settings.background_memory_priority
    };
    (priority, foreground, visible_window)
}

// Hold/recovery eligibility alone is not evidence of current competing demand.
pub(super) fn fresh_background_competition(usage_tenths: Option<u16>, threshold: u8) -> bool {
    usage_tenths.is_some_and(|usage| usage > 0 && usage >= u16::from(threshold) * 10)
}
