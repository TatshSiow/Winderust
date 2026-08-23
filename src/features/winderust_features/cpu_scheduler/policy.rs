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

pub(super) fn foreground_process_group_ids(
    processes: &[ProcessInfo],
    foreground_process_id: Option<u32>,
) -> BTreeSet<u32> {
    let Some(foreground_process_id) = foreground_process_id else {
        return BTreeSet::new();
    };

    let mut group = BTreeSet::from([foreground_process_id]);
    let mut changed = true;
    while changed {
        changed = false;
        for process in processes {
            if process
                .parent_id
                .is_some_and(|parent_id| group.contains(&parent_id))
                && group.insert(process.id)
            {
                changed = true;
            }
        }
    }
    group
}

pub(super) fn cpu_pressure_restraint_should_run(
    settings: &CpuSchedulerSettings,
    foreground_cpu_usage_percent: Option<f32>,
    total_cpu_usage_percent: Option<f32>,
) -> bool {
    if !settings.cpu_pressure_restraint_enabled && !settings.limit_background_processors_enabled {
        return false;
    }
    let threshold = f32::from(settings.foreground_or_system_cpu_threshold_percent.min(100));
    foreground_cpu_usage_percent.is_some_and(|usage| usage >= threshold)
        || total_cpu_usage_percent.is_some_and(|usage| usage >= threshold)
}

pub(super) fn cpu_pressure_above_recovery_threshold(
    settings: &CpuSchedulerSettings,
    foreground_cpu_usage_percent: Option<f32>,
    total_cpu_usage_percent: Option<f32>,
) -> bool {
    let trigger = settings.foreground_or_system_cpu_threshold_percent.min(100);
    let recovery_threshold = f32::from(trigger.saturating_sub(CPU_SCHEDULER_RECOVERY_BAND_PERCENT));
    foreground_cpu_usage_percent.is_some_and(|usage| usage >= recovery_threshold)
        || total_cpu_usage_percent.is_some_and(|usage| usage >= recovery_threshold)
}

pub(super) fn cpu_allocation_method(settings: &CpuSchedulerSettings) -> CpuAllocationMode {
    match settings.cpu_allocation_method {
        CpuAllocationMethod::CpuSetsSoft => CpuAllocationMode::SoftCpuSets,
        CpuAllocationMethod::ProcessorAffinityHard => CpuAllocationMode::HardAffinity,
    }
}

pub(super) fn cpu_scheduler_process_decision(
    settings: &CpuSchedulerSettings,
    tier: CpuSchedulerTier,
) -> CpuSchedulerDecision {
    if settings.limit_background_processors_enabled && tier == CpuSchedulerTier::Background {
        CpuSchedulerDecision::LimitProcessors
    } else {
        CpuSchedulerDecision::LowerPriority
    }
}

pub(super) fn cpu_pressure_restraint_target(
    settings: &CpuSchedulerSettings,
    tier: CpuSchedulerTier,
    background_efficiency_managed: bool,
) -> Option<PressureTargetPolicy> {
    let priority = if settings.process_priority_enabled {
        let priority = match tier {
            CpuSchedulerTier::VisibleWindow => settings.visible_window_priority,
            CpuSchedulerTier::Background => settings.background_priority,
        };
        cpu_scheduler_priority_value(priority)
    } else {
        None
    };
    let apply_background_efficiency = !background_efficiency_managed
        && settings.background_efficiency_enabled
        && settings.background_efficiency_mode_for(false, tier == CpuSchedulerTier::VisibleWindow);

    (priority.is_some() || apply_background_efficiency).then_some(PressureTargetPolicy {
        priority,
        apply_background_efficiency,
    })
}

pub(super) fn cpu_scheduler_candidate(
    process_id: u32,
    process: &CpuSchedulerProcess,
    decision: CpuSchedulerDecision,
    tier: CpuSchedulerTier,
) -> CpuSchedulerCandidate {
    CpuSchedulerCandidate {
        process_id,
        process_name: process.process_name.clone(),
        decision,
        tier,
        score: u32::from(process.last_usage_tenths.unwrap_or_default())
            + if process.selected {
                CPU_SCHEDULER_SELECTION_STICKINESS_TENTHS
            } else {
                0
            },
    }
}

pub(super) fn select_cpu_scheduler_candidates(
    mut candidates: Vec<CpuSchedulerCandidate>,
    maximum_restrained_apps: u8,
) -> Vec<CpuSchedulerCandidate> {
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
    settings: &CpuSchedulerSettings,
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
