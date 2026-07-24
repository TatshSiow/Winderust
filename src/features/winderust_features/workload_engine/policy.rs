use super::*;
use rust_i18n::t;

pub fn is_builtin_excluded(process_name: &str) -> bool {
    contains_process_name(BUILT_IN_EXCLUSIONS, process_name)
}

pub(super) fn percent_tenths(usage: f32) -> u16 {
    (usage.clamp(0.0, 100.0) * 10.0).round() as u16
}

pub(super) fn duration_millis_u64(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

pub(super) fn workload_engine_status_message(
    settings: &WorkloadEngineSettings,
    foreground_cpu_usage_percent: Option<f32>,
    total_cpu_usage_percent: Option<f32>,
    launch_boost_running: bool,
    running: bool,
    restrained_count: usize,
) -> String {
    if !settings.workload_engine_enabled {
        return "Workload Engine disabled.".to_owned();
    }

    if launch_boost_running {
        return t!("runtime_status.workload_engine_launch_boost").to_string();
    }

    if !running {
        let threshold = settings.workload_engine_total_threshold_percent.min(100);
        return match (foreground_cpu_usage_percent, total_cpu_usage_percent) {
            (Some(usage), _) if foreground_cpu_saturates_workload(usage) => t!(
                "runtime_status.workload_engine_foreground_saturated",
                usage = format!("{:.1}", usage.clamp(0.0, 100.0))
            ),
            (Some(foreground), Some(total)) => t!(
                "runtime_status.workload_engine_waiting_for_cpu",
                foreground = format!("{:.1}", foreground.clamp(0.0, 100.0)),
                total = format!("{:.1}", total.clamp(0.0, 100.0)),
                threshold = threshold
            ),
            (Some(foreground), None) => t!(
                "runtime_status.workload_engine_waiting_for_foreground_cpu",
                foreground = format!("{:.1}", foreground.clamp(0.0, 100.0)),
                threshold = threshold
            ),
            (None, Some(total)) => t!(
                "runtime_status.workload_engine_waiting_for_system_cpu",
                total = format!("{:.1}", total.clamp(0.0, 100.0)),
                threshold = threshold
            ),
            (None, None) => t!("runtime_status.workload_engine_waiting_for_sample"),
        }
        .to_string();
    }

    if restrained_count == 0 {
        return t!("runtime_status.workload_engine_watching").to_string();
    }

    let key = if restrained_count == 1 {
        "runtime_status.workload_engine_balancing_one"
    } else {
        "runtime_status.workload_engine_balancing_many"
    };
    t!(key, count = restrained_count).to_string()
}

pub(super) fn matching_rule<'a>(
    settings: &'a WorkloadEngineSettings,
    process_name: &str,
) -> Option<&'a PriorityRule> {
    settings
        .rules
        .iter()
        .find(|rule| rule.enabled && same_process_name(&rule.process_name, process_name))
}

pub(super) fn should_skip_foreground_process(
    process_id: u32,
    process_name: &str,
    foreground_process_id: Option<u32>,
    foreground_process_group_ids: &BTreeSet<u32>,
    foreground_process_name: Option<&str>,
) -> bool {
    foreground_process_id.is_some_and(|id| id == process_id)
        || foreground_process_group_ids.contains(&process_id)
        || foreground_process_name.is_some_and(|name| same_process_name(name, process_name))
}

pub(super) fn foreground_boost_eligible(
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
    foreground_process_name: Option<&str>,
    background_efficiency_process_ids: &BTreeSet<u32>,
) -> bool {
    process_id == 0
        || process_id == current_process_id
        || background_efficiency_process_ids.contains(&process_id)
        || is_builtin_excluded(process_name)
        || should_skip_foreground_process(
            process_id,
            process_name,
            foreground_process_id,
            foreground_process_group_ids,
            foreground_process_name,
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

pub(super) fn workload_engine_should_run(
    settings: &WorkloadEngineSettings,
    foreground_cpu_usage_percent: Option<f32>,
    total_cpu_usage_percent: Option<f32>,
) -> bool {
    settings.workload_engine_enabled
        && cpu_pressure_should_run(
            settings,
            foreground_cpu_usage_percent,
            total_cpu_usage_percent,
        )
}

pub(super) fn smart_efficiency_should_run(
    settings: &WorkloadEngineSettings,
    foreground_cpu_usage_percent: Option<f32>,
    total_cpu_usage_percent: Option<f32>,
) -> bool {
    if !settings.lower_background_auto_cpu_percent {
        return true;
    }

    cpu_pressure_should_run(
        settings,
        foreground_cpu_usage_percent,
        total_cpu_usage_percent,
    )
}

pub(super) fn cpu_pressure_should_run(
    settings: &WorkloadEngineSettings,
    foreground_cpu_usage_percent: Option<f32>,
    total_cpu_usage_percent: Option<f32>,
) -> bool {
    let foreground_saturated =
        foreground_cpu_usage_percent.is_some_and(foreground_cpu_saturates_workload);
    let foreground_pressure = foreground_cpu_usage_percent.is_some_and(|usage| {
        usage >= f32::from(settings.workload_engine_total_threshold_percent.min(100))
            && !foreground_cpu_saturates_workload(usage)
    });
    let system_pressure = !foreground_saturated
        && total_cpu_usage_percent.is_some_and(|usage| {
            usage >= f32::from(settings.workload_engine_total_threshold_percent.min(100))
        });

    foreground_pressure || system_pressure
}

pub(super) fn cpu_pressure_above_restore_threshold(
    settings: &WorkloadEngineSettings,
    foreground_cpu_usage_percent: Option<f32>,
    total_cpu_usage_percent: Option<f32>,
) -> bool {
    let foreground_saturated =
        foreground_cpu_usage_percent.is_some_and(foreground_cpu_saturates_workload);
    if foreground_saturated {
        return false;
    }

    let restore_threshold = workload_engine_pressure_restore_threshold(settings);
    foreground_cpu_usage_percent.is_some_and(|usage| usage >= restore_threshold)
        || total_cpu_usage_percent.is_some_and(|usage| usage >= restore_threshold)
}

pub(super) fn workload_engine_pressure_restore_threshold(settings: &WorkloadEngineSettings) -> f32 {
    let trigger = settings.workload_engine_total_threshold_percent.min(100);
    f32::from(trigger.saturating_sub(WORKLOAD_ENGINE_PRESSURE_RESTORE_BAND_PERCENT))
}

pub(super) fn foreground_cpu_saturates_workload(usage: f32) -> bool {
    usage >= WORKLOAD_ENGINE_FOREGROUND_SATURATION_PERCENT
}

pub(super) fn workload_engine_effective_restriction_mode(
    settings: &WorkloadEngineSettings,
) -> CpuRestrictionMode {
    if settings.lower_background_auto_cpu_percent {
        CpuRestrictionMode::SoftCpuSets
    } else {
        settings.workload_engine_affinity_mode
    }
}

pub(super) fn workload_engine_affinity_mode(settings: &WorkloadEngineSettings) -> CoreSteeringMode {
    match workload_engine_effective_restriction_mode(settings) {
        CpuRestrictionMode::SoftCpuSets => CoreSteeringMode::Soft,
        CpuRestrictionMode::HardAffinity => CoreSteeringMode::Hard,
    }
}

pub(super) fn workload_engine_process_decision(
    settings: &WorkloadEngineSettings,
    active_since: Option<Instant>,
    now: Instant,
) -> WorkloadEngineDecision {
    if !settings.lower_background_auto_cpu_percent {
        return WorkloadEngineDecision::RestrictAffinity;
    }

    if !settings.workload_engine_affinity_escalation_enabled {
        return WorkloadEngineDecision::LowerPriority;
    }

    let Some(active_since) = active_since else {
        return WorkloadEngineDecision::LowerPriority;
    };
    let escalation_delay = Duration::from_secs(settings.workload_engine_sustain_seconds.max(1));
    if now.duration_since(active_since) >= escalation_delay {
        WorkloadEngineDecision::RestrictAffinity
    } else {
        WorkloadEngineDecision::LowerPriority
    }
}

pub(super) fn workload_engine_priority_sustain(
    settings: &WorkloadEngineSettings,
    restraint_count: u32,
) -> Duration {
    if settings.lower_background_auto_cpu_percent && settings.workload_engine_sustain_seconds <= 1 {
        return Duration::ZERO;
    }

    let sustain = Duration::from_secs(settings.workload_engine_sustain_seconds);
    if settings.lower_background_auto_cpu_percent && restraint_count > 0 {
        sustain / WORKLOAD_ENGINE_REPEAT_OFFENDER_SUSTAIN_DIVISOR
    } else {
        sustain
    }
}

pub(super) fn workload_engine_candidate(
    process_id: u32,
    process: &WorkloadEngineProcess,
    decision: WorkloadEngineDecision,
    now: Instant,
) -> WorkloadEngineCandidate {
    let selected_bonus = if process.selected { 500 } else { 0 };
    let decision_bonus = if decision == WorkloadEngineDecision::RestrictAffinity {
        1_000
    } else {
        0
    };
    let active_seconds = process
        .active_since
        .map(|started| now.duration_since(started).as_secs().min(60) as u32)
        .unwrap_or_default();
    WorkloadEngineCandidate {
        process_id,
        process_name: process.process_name.clone(),
        decision,
        score: selected_bonus
            + decision_bonus
            + u32::from(process.last_usage_tenths.unwrap_or_default()).saturating_mul(4)
            + process.restraint_count.saturating_mul(100)
            + active_seconds.saturating_mul(10),
    }
}

pub(super) fn select_workload_engine_candidates(
    mut candidates: Vec<WorkloadEngineCandidate>,
    max_targeted_processes: u8,
) -> Vec<WorkloadEngineCandidate> {
    candidates.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| left.process_id.cmp(&right.process_id))
    });
    candidates.truncate(usize::from(max_targeted_processes.max(1)));
    candidates
}

pub(super) fn workload_engine_effective_cpu_percent(
    settings: &WorkloadEngineSettings,
    foreground_cpu_usage_percent: Option<f32>,
) -> u8 {
    workload_engine_effective_cpu_percent_for_topology(
        settings,
        foreground_cpu_usage_percent,
        workload_engine_has_efficiency_cores(),
    )
}

pub(super) fn workload_engine_effective_cpu_percent_for_topology(
    settings: &WorkloadEngineSettings,
    foreground_cpu_usage_percent: Option<f32>,
    has_efficiency_cores: bool,
) -> u8 {
    let configured =
        workload_engine_minimum_cpu_percent_for_topology(settings, has_efficiency_cores);
    let Some(usage) = foreground_cpu_usage_percent else {
        return configured;
    };
    let threshold = f32::from(settings.workload_engine_total_threshold_percent.min(100));
    let saturation = WORKLOAD_ENGINE_FOREGROUND_SATURATION_PERCENT;
    if usage >= saturation || threshold >= saturation {
        return if settings.lower_background_auto_cpu_percent {
            100
        } else {
            configured
        };
    }

    let relaxed = if settings.lower_background_auto_cpu_percent {
        100.0
    } else {
        ((u16::from(configured) + 100) / 2) as f32
    };
    let pressure = ((usage - threshold) / (saturation - threshold)).clamp(0.0, 1.0);
    (relaxed - ((relaxed - f32::from(configured)) * pressure))
        .round()
        .clamp(f32::from(configured), 100.0) as u8
}

pub(super) fn workload_engine_minimum_cpu_percent_for_topology(
    settings: &WorkloadEngineSettings,
    has_efficiency_cores: bool,
) -> u8 {
    if !settings.lower_background_auto_cpu_percent {
        return settings.workload_engine_cpu_percent.clamp(1, 100);
    }

    let trigger = settings.workload_engine_total_threshold_percent.min(100);
    if has_efficiency_cores {
        if trigger >= 80 {
            80
        } else if trigger >= 70 {
            70
        } else {
            50
        }
    } else if trigger >= 80 {
        80
    } else if trigger >= 70 {
        65
    } else {
        50
    }
}

pub(super) fn workload_engine_has_efficiency_cores() -> bool {
    core_steering::logical_processors()
        .iter()
        .any(|processor| processor.kind == LogicalProcessorKind::Efficiency)
}

pub(super) fn limited_efficiency_preferred_core_mask(
    percent: u8,
    max_logical_processors: u8,
) -> Option<u64> {
    let processors = core_steering::logical_processors();
    if processors.is_empty() {
        return None;
    }

    let e_core_indices = processors
        .iter()
        .filter(|processor| processor.kind == LogicalProcessorKind::Efficiency)
        .map(|processor| processor.index)
        .collect::<Vec<_>>();
    let mut selected = if e_core_indices.is_empty() {
        processors
            .iter()
            .map(|processor| processor.index)
            .collect::<Vec<_>>()
    } else {
        e_core_indices
    };
    selected.sort_unstable();
    selected.dedup();

    logical_indices_to_limited_mask(&selected, percent, max_logical_processors)
}

pub(super) fn load_aware_limited_core_mask(
    processors: &[LogicalProcessorInfo],
    usages: &[f32],
    percent: u8,
    max_logical_processors: u8,
) -> Option<u64> {
    let e_core_exists = processors
        .iter()
        .any(|processor| processor.kind == LogicalProcessorKind::Efficiency);
    let mut candidates = processors
        .iter()
        .filter(|processor| !e_core_exists || processor.kind == LogicalProcessorKind::Efficiency)
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
    let selected = candidates
        .into_iter()
        .map(|(index, _usage)| index)
        .collect::<Vec<_>>();
    logical_indices_to_limited_mask(&selected, percent, max_logical_processors)
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

pub(super) fn logical_indices_to_limited_mask(
    indices: &[usize],
    percent: u8,
    max_logical_processors: u8,
) -> Option<u64> {
    if indices.is_empty() {
        return None;
    }
    let percent_count = (indices.len() * usize::from(percent.clamp(1, 100))).div_ceil(100);
    let max_count = usize::from(max_logical_processors);
    let limit = if max_count == 0 {
        percent_count
    } else {
        percent_count.min(max_count)
    }
    .clamp(1, indices.len());

    let mut mask = 0_u64;
    for index in indices.iter().take(limit) {
        if *index < u64::BITS as usize {
            mask |= 1_u64 << index;
        }
    }
    (mask != 0).then_some(mask)
}

pub const fn process_priority_class(priority: ProcessPriority) -> u32 {
    match priority {
        ProcessPriority::Normal => NORMAL_PRIORITY_CLASS,
        ProcessPriority::BelowNormal => BELOW_NORMAL_PRIORITY_CLASS,
        ProcessPriority::Idle => IDLE_PRIORITY_CLASS,
    }
}

pub fn foreground_boost_priority_class(
    priority: ForegroundBoostPriority,
    foreground_cpu_usage_percent: Option<f32>,
) -> u32 {
    match priority {
        ForegroundBoostPriority::Auto => {
            if foreground_cpu_usage_percent.is_some_and(foreground_cpu_saturates_workload) {
                NORMAL_PRIORITY_CLASS
            } else {
                ABOVE_NORMAL_PRIORITY_CLASS
            }
        }
        ForegroundBoostPriority::Normal => NORMAL_PRIORITY_CLASS,
        ForegroundBoostPriority::AboveNormal => ABOVE_NORMAL_PRIORITY_CLASS,
    }
}

pub(super) fn foreground_launch_boost_eligible(process_id: u32) -> bool {
    process_age(process_id)
        .is_some_and(|age| process_age_in_launch_boost_window(age, FOREGROUND_LAUNCH_BOOST_WINDOW))
}

pub(super) fn workload_engine_launch_boost_enabled(
    settings: &WorkloadEngineSettings,
    foreground_is_launching: bool,
) -> bool {
    settings.workload_engine_enabled && foreground_is_launching
}

pub(super) fn process_age_in_launch_boost_window(age: Duration, launch_window: Duration) -> bool {
    age <= launch_window
}
