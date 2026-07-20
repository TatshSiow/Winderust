use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::c_void,
    time::{Duration, Instant},
};

use windows_sys::Win32::{
    Foundation::{ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER, FILETIME},
    System::{
        SystemInformation::GetSystemTimeAsFileTime,
        Threading::{
            GetCurrentProcessId, GetPriorityClass, GetProcessInformation, GetProcessPriorityBoost,
            GetProcessTimes, OpenProcess, ProcessPowerThrottling, SetPriorityClass,
            SetProcessInformation, SetProcessPriorityBoost, ABOVE_NORMAL_PRIORITY_CLASS,
            BELOW_NORMAL_PRIORITY_CLASS, HIGH_PRIORITY_CLASS, IDLE_PRIORITY_CLASS,
            NORMAL_PRIORITY_CLASS, PROCESS_POWER_THROTTLING_CURRENT_VERSION,
            PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
            PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION, PROCESS_POWER_THROTTLING_STATE,
            PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_INFORMATION, REALTIME_PRIORITY_CLASS,
        },
    },
};

use crate::{
    action_log::{ActionLog, ActionLogAction, ActionLogFeature, ActionLogResult},
    audio_activity::active_audio_process_ids,
    config::{
        CoreSteeringMode, CoreSteeringRule, CoreSteeringSettings, CpuRestrictionMode,
        ForegroundBoostPriority, PriorityRule, ProcessPriority, WorkloadEngineSettings,
    },
    core_steering::{self, CoreSteeringManager, LogicalProcessorInfo, LogicalProcessorKind},
    cpu::{process_cpu_usage_percent, PerProcessorUsageMonitor, ProcessCpuSample},
    foreground::{
        contains_process_name, is_process_exited_message, list_processes, process_count_label,
        process_failure_key, process_names_by_id, process_session_id, same_process_name,
        unique_app_names, ProcessInfo, EXTENDED_BUILT_IN_PROCESS_EXCLUSIONS,
    },
    memory_priority::{MemoryPriorityManager, MemoryPriorityTarget},
    rules::{execution_failure_suppression_threshold, ExecutionFailureTracker},
    win_util::{filetime_to_u64, last_error, WinHandle},
};

const BUILT_IN_EXCLUSIONS: &[&str] = EXTENDED_BUILT_IN_PROCESS_EXCLUSIONS;
const WORKLOAD_ENGINE_FOREGROUND_SATURATION_PERCENT: f32 = 85.0;
const WORKLOAD_ENGINE_PRESSURE_RESTORE_BAND_PERCENT: u8 = 5;
const WORKLOAD_ENGINE_CORE_REBALANCE_INTERVAL_SECS: u64 = 3;
const WORKLOAD_ENGINE_CORE_REBALANCE_IMPROVEMENT_PERCENT: f32 = 15.0;
const BACKGROUND_APPLY_SUMMARY_LOG_INTERVAL: Duration = Duration::from_secs(30);
const FOREGROUND_LAUNCH_BOOST_WINDOW: Duration = Duration::from_secs(8);
const WORKLOAD_ENGINE_REPEAT_OFFENDER_SUSTAIN_DIVISOR: u32 = 2;
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadEngineSnapshot {
    pub enabled: bool,
    pub scanned_processes: usize,
    pub background_adjusted_processes: usize,
    pub timer_resolution_ignored_processes: usize,
    pub foreground_boosted_process: Option<String>,
    pub launch_boost_active: bool,
    pub workload_engine_active: bool,
    pub workload_managed_processes: usize,
    pub workload_engine_message: String,
    pub workload_engine_total_cpu_usage_tenths: Option<u16>,
    pub adaptive_power_profile: Option<String>,
    pub workload_engine_details: Vec<WorkloadEngineProcessStatus>,
    pub skipped_processes: usize,
    pub failed_processes: usize,
    pub adjusted_apps: Vec<String>,
    pub auto_excluded_processes: Vec<String>,
    pub message: String,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkloadEngineProcessStatus {
    pub process_id: u32,
    pub process_name: String,
    pub state: WorkloadEngineProcessState,
    pub cpu_usage_tenths: Option<u16>,
    pub elapsed_seconds: Option<u64>,
    pub reaction_millis: Option<u64>,
    pub restraint_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkloadEngineProcessState {
    Watching,
    Lowered,
    AffinityRestrained,
    CoolingDown,
}

pub struct WorkloadEngineManager {
    adjusted: BTreeMap<u32, AdjustedProcess>,
    boosted: BTreeMap<u32, BoostedProcess>,
    foreground_candidate: Option<ForegroundCandidate>,
    foreground_cpu_sample: Option<(BTreeSet<u32>, ProcessCpuSample)>,
    lower_background_affinity: CoreSteeringManager,
    workload_engine: BTreeMap<u32, WorkloadEngineProcess>,
    workload_engine_pressure_active: bool,
    workload_engine_affinity: CoreSteeringManager,
    workload_engine_memory_priority: MemoryPriorityManager,
    workload_engine_core_selection: Option<WorkloadEngineCoreSelection>,
    last_background_apply_summary_logged_at: Option<Instant>,
    per_processor_usage: PerProcessorUsageMonitor,
    failure_suppression: ExecutionFailureTracker,
}

impl Default for WorkloadEngineManager {
    fn default() -> Self {
        Self {
            adjusted: BTreeMap::new(),
            boosted: BTreeMap::new(),
            foreground_candidate: None,
            foreground_cpu_sample: None,
            lower_background_affinity: CoreSteeringManager::with_action_log_feature(
                ActionLogFeature::WorkloadEngine,
            ),
            workload_engine: BTreeMap::new(),
            workload_engine_pressure_active: false,
            workload_engine_affinity: CoreSteeringManager::with_action_log_feature(
                ActionLogFeature::WorkloadEngine,
            ),
            workload_engine_memory_priority: MemoryPriorityManager::default(),
            workload_engine_core_selection: None,
            last_background_apply_summary_logged_at: None,
            per_processor_usage: PerProcessorUsageMonitor::default(),
            failure_suppression: ExecutionFailureTracker::default(),
        }
    }
}

#[derive(Clone)]
struct AdjustedProcess {
    process_name: String,
    creation_time: u64,
    previous_priority: u32,
    applied_priority: u32,
    previous_dynamic_priority_boost_disabled: Option<bool>,
    applied_dynamic_priority_boost_disabled: bool,
    previous_efficiency_state: Option<PROCESS_POWER_THROTTLING_STATE>,
    applied_background_efficiency: bool,
    applied_ignore_timer_resolution: bool,
}

#[derive(Clone)]
struct BoostedProcess {
    process_id: u32,
    process_name: String,
    creation_time: u64,
    previous_priority: u32,
    applied_priority: u32,
}

struct ForegroundCandidate {
    process_id: u32,
    process_name: String,
    first_seen: Instant,
}

#[derive(Default)]
struct ForegroundBoostGroupResult {
    skipped: usize,
    failures: PriorityFailures,
    auto_excluded_processes: Vec<String>,
}

#[derive(Clone)]
struct WorkloadEngineProcess {
    process_name: String,
    previous_cpu_time: Option<ProcessCpuSample>,
    last_usage_tenths: Option<u16>,
    high_since: Option<Instant>,
    below_since: Option<Instant>,
    active_since: Option<Instant>,
    last_reaction_millis: Option<u64>,
    restraint_count: u32,
    decision: Option<WorkloadEngineDecision>,
    active: bool,
    selected: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum WorkloadEngineDecision {
    LowerPriority,
    RestrictAffinity,
}

#[derive(Clone)]
struct WorkloadEngineCandidate {
    process_id: u32,
    process_name: String,
    decision: WorkloadEngineDecision,
    score: u32,
}

#[derive(Clone, Copy)]
struct WorkloadEngineCoreSelection {
    mask: u64,
    selected_at: Instant,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PriorityTargetSource {
    WorkloadEngine,
    BackgroundPolicy,
    Rule,
}

pub struct WorkloadEngineUpdate<'a> {
    pub settings: &'a WorkloadEngineSettings,
    pub automation_enabled: bool,
    pub foreground_process_id: Option<u32>,
    pub total_cpu_usage_percent: Option<f32>,
    pub background_efficiency_managed: bool,
    pub background_efficiency_process_ids: &'a BTreeSet<u32>,
}

struct ForegroundBoostGroup<'a> {
    foreground_id: u32,
    foreground_process_name: Option<&'a str>,
    targets: &'a [(u32, String)],
    stability_delay_ms: u64,
    foreground_boost: ForegroundBoostPriority,
    foreground_cpu_usage_percent: Option<f32>,
}

struct ApplyPriorityRequest<'a> {
    process_id: u32,
    process_name: String,
    priority_class: u32,
    existing: Option<&'a AdjustedProcess>,
    source: PriorityTargetSource,
    apply_priority_class: bool,
    apply_background_efficiency: bool,
    ignore_timer_resolution: bool,
    disable_dynamic_priority_boost: bool,
    log_success: bool,
}

impl WorkloadEngineManager {
    pub fn managed_process_ids(&self) -> BTreeSet<u32> {
        self.adjusted
            .keys()
            .chain(self.boosted.keys())
            .copied()
            .collect()
    }

    pub fn update(
        &mut self,
        input: WorkloadEngineUpdate<'_>,
        action_log: &mut ActionLog,
    ) -> WorkloadEngineSnapshot {
        let WorkloadEngineUpdate {
            settings,
            automation_enabled,
            foreground_process_id,
            total_cpu_usage_percent,
            background_efficiency_managed,
            background_efficiency_process_ids,
        } = input;

        if !automation_enabled {
            let failed = self.clear_all(action_log, "automation disabled");
            self.failure_suppression.clear();
            return WorkloadEngineSnapshot {
                enabled: false,
                failed_processes: failed.count,
                message: "Automation disabled.".to_owned(),
                last_error: failed.last_error,
                ..Default::default()
            };
        }

        if !settings.enabled {
            let failed = self.clear_all(action_log, "Workload Engine disabled");
            self.failure_suppression.clear();
            return WorkloadEngineSnapshot {
                enabled: false,
                failed_processes: failed.count,
                message: "Workload Engine disabled.".to_owned(),
                last_error: failed.last_error,
                ..Default::default()
            };
        }

        let current_process_id = unsafe { GetCurrentProcessId() };
        let Some(current_session_id) = process_session_id(current_process_id) else {
            let failed = self.clear_all(action_log, "current Windows session is unknown");
            return WorkloadEngineSnapshot {
                enabled: true,
                failed_processes: failed.count,
                message: "Paused: current Windows session is unknown.".to_owned(),
                last_error: failed.last_error,
                ..Default::default()
            };
        };

        let processes = match list_processes() {
            Ok(processes) => processes,
            Err(err) => {
                let failed = self.clear_all(action_log, "process list unavailable");
                return WorkloadEngineSnapshot {
                    enabled: true,
                    failed_processes: failed.count,
                    message: err,
                    last_error: failed.last_error,
                    ..Default::default()
                };
            }
        };

        let scanned_processes = processes.len();
        let current_process_names = process_names_by_id(&processes);
        let foreground_process_name = foreground_process_id.and_then(|id| {
            processes
                .iter()
                .find(|process| process.id == id)
                .map(|process| process.name.clone())
        });
        let foreground_process_group_ids =
            foreground_process_group_ids(&processes, foreground_process_id);
        let foreground_cpu_usage_percent =
            self.update_foreground_cpu_usage(&foreground_process_group_ids);
        let foreground_cpu_usage_tenths = foreground_cpu_usage_percent.map(percent_tenths);

        let mut failures = PriorityFailures::default();
        let active_audio_process_ids = active_audio_process_ids().ok();

        let mut lowerable_background_processes = BTreeMap::new();
        for process in &processes {
            if should_skip_process(
                process.id,
                &process.name,
                current_process_id,
                foreground_process_id,
                &foreground_process_group_ids,
                foreground_process_name.as_deref(),
                background_efficiency_process_ids,
            ) {
                continue;
            }

            if process_session_id(process.id) != Some(current_session_id) {
                continue;
            }

            lowerable_background_processes.insert(process.id, process.name.clone());
        }

        let mut target_processes = BTreeMap::new();
        let foreground_launch_boost_target = foreground_process_id
            .zip(foreground_process_name.as_deref())
            .is_some_and(|(process_id, process_name)| {
                !background_efficiency_process_ids.contains(&process_id)
                    && foreground_boost_eligible(
                        process_id,
                        process_name,
                        current_process_id,
                        current_session_id,
                    )
                    && foreground_launch_boost_eligible(process_id)
            });
        let launch_boost_running =
            workload_engine_launch_boost_enabled(settings, foreground_launch_boost_target);
        let background_policy_can_run = !background_efficiency_managed
            && !launch_boost_running
            && smart_efficiency_should_run(
                settings,
                foreground_cpu_usage_percent,
                total_cpu_usage_percent,
            );
        let lower_background_policy_enabled =
            settings.lower_background_apps && background_policy_can_run;
        let auto_efficiency_policy_enabled =
            settings.workload_engine_background_efficiency_enabled && background_policy_can_run;
        if (settings.lower_background_apps
            || settings.workload_engine_background_efficiency_enabled)
            && !background_efficiency_managed
        {
            for (process_id, process_name) in &lowerable_background_processes {
                let matched_rule = settings
                    .lower_background_apps
                    .then(|| matching_rule(settings, process_name))
                    .flatten();
                let (priority, source, apply_priority_class) = if let Some(rule) = matched_rule {
                    (rule.priority, PriorityTargetSource::Rule, true)
                } else if lower_background_policy_enabled || auto_efficiency_policy_enabled {
                    (
                        settings.workload_engine_background_priority,
                        PriorityTargetSource::BackgroundPolicy,
                        settings.lower_background_apps,
                    )
                } else {
                    continue;
                };
                target_processes.insert(
                    *process_id,
                    (process_name.clone(), priority, source, apply_priority_class),
                );
            }
        }

        let workload_engine_running = self.update_workload_engine_pressure(
            settings,
            foreground_cpu_usage_percent,
            total_cpu_usage_percent,
        );
        let workload_engine_restraints_running = workload_engine_running && !launch_boost_running;
        let lower_background_affinity_settings = CoreSteeringSettings {
            enabled: false,
            exclude_foreground_app: true,
            rules: Vec::new(),
        };
        let lower_background_affinity_snapshot = self.lower_background_affinity.update(
            &lower_background_affinity_settings,
            automation_enabled,
            foreground_process_id,
            action_log,
        );
        let mut auto_excluded_processes = lower_background_affinity_snapshot
            .auto_excluded_processes
            .iter()
            .map(|name| process_failure_key(name))
            .collect::<BTreeSet<_>>();
        failures.count += lower_background_affinity_snapshot.failed_processes;
        if failures.last_error.is_none() {
            failures.last_error = lower_background_affinity_snapshot.last_error.clone();
        }

        let mut workload_engine_rules = Vec::new();
        let mut workload_engine_memory_targets = Vec::new();
        if settings.workload_engine_memory_priority_enabled {
            if let Some(priority) = settings
                .workload_engine_foreground_memory_priority
                .priority()
            {
                for process in processes
                    .iter()
                    .filter(|process| foreground_process_group_ids.contains(&process.id))
                    .filter(|process| !background_efficiency_process_ids.contains(&process.id))
                    .filter(|process| {
                        !settings.workload_engine_exclusion_enabled_for(&process.name)
                    })
                    .filter(|process| {
                        foreground_boost_eligible(
                            process.id,
                            &process.name,
                            current_process_id,
                            current_session_id,
                        )
                    })
                {
                    workload_engine_memory_targets.push(MemoryPriorityTarget {
                        process_id: process.id,
                        process_name: process.name.clone(),
                        priority,
                        foreground: true,
                        preserve_foreground_priority: true,
                        preserve_background_priority: true,
                    });
                }
            }
        }
        if workload_engine_restraints_running {
            let now = Instant::now();
            let workload_engine_core_mask =
                self.workload_engine_core_mask(settings, foreground_cpu_usage_percent, now);
            let current_ids = processes
                .iter()
                .map(|process| process.id)
                .collect::<BTreeSet<_>>();
            self.workload_engine
                .retain(|process_id, _| current_ids.contains(process_id));

            let mut workload_engine_candidates = Vec::new();
            for (process_id, process_name) in &lowerable_background_processes {
                if settings.workload_engine_exclusion_enabled_for(process_name) {
                    continue;
                }

                if let Some(candidate) =
                    self.update_workload_engine_process(*process_id, process_name, settings, now)
                {
                    workload_engine_candidates.push(candidate);
                }
            }

            let selected_candidates = select_workload_engine_candidates(
                workload_engine_candidates,
                settings.workload_engine_max_targeted_processes,
            );
            let selected_ids = selected_candidates
                .iter()
                .map(|candidate| candidate.process_id)
                .collect::<BTreeSet<_>>();
            for (process_id, process) in &mut self.workload_engine {
                if process.active && !selected_ids.contains(process_id) {
                    process.selected = false;
                    process.decision = None;
                }
            }

            for candidate in selected_candidates {
                if let Some(process) = self.workload_engine.get_mut(&candidate.process_id) {
                    process.selected = true;
                    process.decision = Some(candidate.decision);
                }
                target_processes
                    .entry(candidate.process_id)
                    .or_insert_with(|| {
                        (
                            candidate.process_name.clone(),
                            settings.workload_engine_background_priority,
                            PriorityTargetSource::WorkloadEngine,
                            true,
                        )
                    });
                if settings.workload_engine_memory_priority_enabled {
                    workload_engine_memory_targets.push(MemoryPriorityTarget {
                        process_id: candidate.process_id,
                        process_name: candidate.process_name.clone(),
                        priority: settings.workload_engine_memory_priority,
                        foreground: false,
                        preserve_foreground_priority: true,
                        preserve_background_priority: true,
                    });
                }
                if candidate.decision == WorkloadEngineDecision::RestrictAffinity {
                    if let Some(core_mask) = workload_engine_core_mask {
                        workload_engine_rules.push(CoreSteeringRule {
                            enabled: true,
                            mode: workload_engine_affinity_mode(settings),
                            process_name: candidate.process_name,
                            core_mask,
                        });
                    }
                }
            }
        } else {
            self.workload_engine.clear();
            self.workload_engine_pressure_active = false;
            self.workload_engine_core_selection = None;
        }

        let affinity_settings = CoreSteeringSettings {
            enabled: settings.enabled
                && settings.workload_engine_enabled
                && workload_engine_restraints_running,
            exclude_foreground_app: true,
            rules: workload_engine_rules,
        };
        let workload_engine_affinity_snapshot = self.workload_engine_affinity.update(
            &affinity_settings,
            automation_enabled,
            foreground_process_id,
            action_log,
        );
        auto_excluded_processes.extend(
            workload_engine_affinity_snapshot
                .auto_excluded_processes
                .iter()
                .map(|name| process_failure_key(name)),
        );
        failures.count += workload_engine_affinity_snapshot.failed_processes;
        if failures.last_error.is_none() {
            failures.last_error = workload_engine_affinity_snapshot.last_error.clone();
        }
        let workload_engine_memory_snapshot = self.workload_engine_memory_priority.update(
            if settings.enabled
                && settings.workload_engine_enabled
                && workload_engine_restraints_running
                && settings.workload_engine_memory_priority_enabled
            {
                workload_engine_memory_targets
            } else {
                Vec::new()
            },
            automation_enabled,
            ActionLogFeature::WorkloadEngine,
            action_log,
        );
        auto_excluded_processes.extend(
            workload_engine_memory_snapshot
                .auto_excluded_processes
                .iter()
                .map(|name| process_failure_key(name)),
        );
        failures.count += workload_engine_memory_snapshot.failed_processes;
        if failures.last_error.is_none() {
            failures.last_error = workload_engine_memory_snapshot.last_error.clone();
        }

        let target_ids = target_processes.keys().copied().collect::<BTreeSet<_>>();
        let mut active_target_names = target_processes
            .values()
            .map(|(name, _priority, _source, _apply_priority_class)| process_failure_key(name))
            .collect::<BTreeSet<_>>();
        if let Some(name) = foreground_process_name.as_deref() {
            active_target_names.insert(process_failure_key(name));
        }
        self.failure_suppression.retain_keys(&active_target_names);
        failures.merge(self.release_non_targets(
            &target_ids,
            &current_process_names,
            action_log,
            "process no longer matches a Workload Engine rule",
        ));
        let mut skipped_processes = 0;
        skipped_processes += workload_engine_memory_snapshot.skipped_processes;
        let mut summarized_background_applies = 0;

        for (process_id, (process_name, priority, source, apply_priority_class)) in target_processes
        {
            let failure_process_name = process_name.clone();
            if self.is_process_suppressed(
                process_id,
                &failure_process_name,
                action_log,
                &mut auto_excluded_processes,
            ) {
                skipped_processes += 1;
                continue;
            }
            match apply_priority(
                ApplyPriorityRequest {
                    process_id,
                    process_name,
                    priority_class: process_priority_class(priority),
                    existing: self.adjusted.get(&process_id),
                    source,
                    apply_priority_class,
                    apply_background_efficiency: settings
                        .workload_engine_background_efficiency_enabled
                        && source != PriorityTargetSource::WorkloadEngine,
                    ignore_timer_resolution: ignore_timer_resolution_allowed(
                        process_id,
                        active_audio_process_ids.as_ref(),
                    ),
                    disable_dynamic_priority_boost: false,
                    log_success: source == PriorityTargetSource::Rule,
                },
                action_log,
            ) {
                Ok(outcome) => {
                    if outcome.changed && source != PriorityTargetSource::Rule {
                        summarized_background_applies += 1;
                    }
                    if let Some(adjusted) = outcome.adjusted {
                        self.clear_process_failure(&failure_process_name);
                        self.adjusted.insert(process_id, adjusted);
                    } else if outcome.skipped {
                        self.clear_process_failure(&failure_process_name);
                        skipped_processes += 1;
                    }
                }
                Err(PriorityError::ProcessExited) => {
                    skipped_processes += 1;
                }
                Err(PriorityError::AccessDenied) => {
                    skipped_processes += 1;
                    self.record_process_failure(&failure_process_name);
                    action_log.record(
                        ActionLogFeature::WorkloadEngine,
                        Some(process_id),
                        failure_process_name,
                        ActionLogAction::Skip,
                        ActionLogResult::Skipped,
                        "Skipped because the process could not be opened.",
                    );
                }
                Err(error) => {
                    let err = priority_error_message(&error);
                    if is_process_exited_message(&err) {
                        skipped_processes += 1;
                        continue;
                    }
                    self.record_process_failure(&failure_process_name);
                    failures.record_message(
                        "Apply",
                        process_id,
                        &failure_process_name,
                        err,
                        action_log,
                    );
                }
            }
        }
        let now = Instant::now();
        if summarized_background_applies > 0
            && background_apply_summary_log_due(self.last_background_apply_summary_logged_at, now)
        {
            self.last_background_apply_summary_logged_at = Some(now);
            action_log.record(
                ActionLogFeature::WorkloadEngine,
                None,
                "Workload Engine",
                ActionLogAction::Apply,
                ActionLogResult::Applied,
                background_apply_summary_message(summarized_background_applies),
            );
        }

        let workload_engine_details = self.workload_engine_statuses(now);
        let workload_managed_processes = workload_engine_details
            .iter()
            .filter(|status| {
                matches!(
                    status.state,
                    WorkloadEngineProcessState::Lowered
                        | WorkloadEngineProcessState::AffinityRestrained
                )
            })
            .count();
        let workload_engine_message = workload_engine_status_message(
            settings,
            foreground_cpu_usage_percent,
            total_cpu_usage_percent,
            launch_boost_running,
            workload_engine_running,
            workload_managed_processes,
        );

        if let Some(foreground_id) = foreground_process_id {
            if (settings.boost_foreground_app || launch_boost_running)
                && !background_efficiency_process_ids.contains(&foreground_id)
            {
                let boost_targets = processes
                    .iter()
                    .filter(|process| foreground_process_group_ids.contains(&process.id))
                    .filter(|process| !background_efficiency_process_ids.contains(&process.id))
                    .filter(|process| {
                        foreground_boost_eligible(
                            process.id,
                            &process.name,
                            current_process_id,
                            current_session_id,
                        )
                    })
                    .map(|process| (process.id, process.name.clone()))
                    .collect::<Vec<_>>();
                let result = self.apply_foreground_boost_group(
                    ForegroundBoostGroup {
                        foreground_id,
                        foreground_process_name: foreground_process_name.as_deref(),
                        targets: &boost_targets,
                        stability_delay_ms: if launch_boost_running {
                            0
                        } else {
                            settings.foreground_stability_delay_ms
                        },
                        foreground_boost: if launch_boost_running {
                            ForegroundBoostPriority::AboveNormal
                        } else {
                            settings.foreground_boost
                        },
                        foreground_cpu_usage_percent,
                    },
                    action_log,
                );
                skipped_processes += result.skipped;
                auto_excluded_processes.extend(result.auto_excluded_processes);
                failures.merge(result.failures);
            } else if let Some(error) =
                self.clear_boosted(true, action_log, "foreground boost disabled or blocked")
            {
                failures.merge(error);
            }
        } else if let Some(error) =
            self.clear_boosted(true, action_log, "foreground app is unknown")
        {
            failures.merge(error);
        }

        WorkloadEngineSnapshot {
            enabled: true,
            scanned_processes,
            background_adjusted_processes: self.adjusted.len(),
            timer_resolution_ignored_processes: self
                .adjusted
                .values()
                .filter(|process| process.applied_ignore_timer_resolution)
                .count(),
            foreground_boosted_process: self.boosted.values().next().map(|process| {
                if self.boosted.len() == 1 {
                    format!("{} ({})", process.process_name, process.process_id)
                } else {
                    format!(
                        "{} ({}) +{}",
                        process.process_name,
                        process.process_id,
                        self.boosted.len() - 1
                    )
                }
            }),
            launch_boost_active: launch_boost_running,
            workload_engine_active: workload_engine_restraints_running,
            workload_managed_processes,
            workload_engine_message,
            workload_engine_total_cpu_usage_tenths: foreground_cpu_usage_tenths,
            adaptive_power_profile: None,
            workload_engine_details,
            skipped_processes,
            failed_processes: failures.count,
            adjusted_apps: unique_app_names(
                self.adjusted
                    .values()
                    .map(|process| process.process_name.as_str()),
            ),
            auto_excluded_processes: auto_excluded_processes.into_iter().collect(),
            message: "Workload Engine active.".to_owned(),
            last_error: failures.last_error,
        }
    }

    fn release_non_targets(
        &mut self,
        target_ids: &BTreeSet<u32>,
        current_process_names: &BTreeMap<u32, String>,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> PriorityFailures {
        let process_ids = self
            .adjusted
            .keys()
            .copied()
            .filter(|process_id| !target_ids.contains(process_id))
            .collect::<Vec<_>>();

        self.release_processes(
            &process_ids,
            Some(current_process_names),
            action_log,
            reason,
        )
    }

    fn clear_all(&mut self, action_log: &mut ActionLog, reason: &str) -> PriorityFailures {
        let mut failures = self
            .clear_boosted(true, action_log, reason)
            .unwrap_or_default();
        let process_ids = self.adjusted.keys().copied().collect::<Vec<_>>();
        failures.merge(self.release_processes(&process_ids, None, action_log, reason));
        self.foreground_candidate = None;
        self.foreground_cpu_sample = None;
        self.workload_engine.clear();
        self.workload_engine_pressure_active = false;
        self.last_background_apply_summary_logged_at = None;
        let affinity_settings = CoreSteeringSettings {
            enabled: false,
            exclude_foreground_app: true,
            rules: Vec::new(),
        };
        let lower_affinity_snapshot =
            self.lower_background_affinity
                .update(&affinity_settings, true, None, action_log);
        failures.count += lower_affinity_snapshot.failed_processes;
        if failures.last_error.is_none() {
            failures.last_error = lower_affinity_snapshot.last_error;
        }
        let affinity_snapshot =
            self.workload_engine_affinity
                .update(&affinity_settings, true, None, action_log);
        failures.count += affinity_snapshot.failed_processes;
        if failures.last_error.is_none() {
            failures.last_error = affinity_snapshot.last_error;
        }
        let memory_snapshot = self.workload_engine_memory_priority.update(
            Vec::new(),
            true,
            ActionLogFeature::WorkloadEngine,
            action_log,
        );
        failures.count += memory_snapshot.failed_processes;
        if failures.last_error.is_none() {
            failures.last_error = memory_snapshot.last_error;
        }
        failures
    }

    fn clear_boosted(
        &mut self,
        reset_candidate: bool,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> Option<PriorityFailures> {
        if reset_candidate {
            self.foreground_candidate = None;
        }
        if self.boosted.is_empty() {
            return None;
        }
        let mut failures = PriorityFailures::default();
        let process_ids = self.boosted.keys().copied().collect::<Vec<_>>();
        let mut restored_processes = 0;
        for process_id in process_ids {
            let Some(process) = self.boosted.get(&process_id).cloned() else {
                continue;
            };
            let process_name = process.process_name.clone();
            if let Err(err) = restore_boosted_priority(&process) {
                if matches!(err, PriorityError::ProcessExited) {
                    self.boosted.remove(&process_id);
                } else {
                    failures.record_error("Restore", process_id, &process_name, err, action_log);
                }
            } else {
                self.boosted.remove(&process_id);
                restored_processes += 1;
            }
        }
        if restored_processes > 0 {
            action_log.record(
                ActionLogFeature::WorkloadEngine,
                None,
                "Workload Engine",
                ActionLogAction::Restore,
                ActionLogResult::Restored,
                foreground_boost_restore_summary_message(restored_processes, reason),
            );
        }
        Some(failures)
    }

    fn is_process_suppressed(
        &mut self,
        process_id: u32,
        process_name: &str,
        action_log: &mut ActionLog,
        auto_excluded_processes: &mut BTreeSet<String>,
    ) -> bool {
        let suppression = self.failure_suppression.process_suppression(process_name);
        if !suppression.suppressed {
            return false;
        }

        if suppression.newly_suppressed {
            auto_excluded_processes.insert(process_failure_key(process_name));
            action_log.record(
                ActionLogFeature::WorkloadEngine,
                Some(process_id),
                process_name.trim().to_owned(),
                ActionLogAction::Skip,
                ActionLogResult::Skipped,
                format!(
                    "Stopped retrying Workload Engine after {} failed attempts.",
                    execution_failure_suppression_threshold(),
                ),
            );
        }

        true
    }

    fn record_process_failure(&mut self, process_name: &str) {
        self.failure_suppression
            .record_process_failure(process_name);
    }

    fn clear_process_failure(&mut self, process_name: &str) {
        self.failure_suppression.clear_process_failure(process_name);
    }

    fn update_workload_engine_pressure(
        &mut self,
        settings: &WorkloadEngineSettings,
        foreground_cpu_usage_percent: Option<f32>,
        total_cpu_usage_percent: Option<f32>,
    ) -> bool {
        if !settings.workload_engine_enabled {
            self.workload_engine_pressure_active = false;
            return false;
        }

        if workload_engine_should_run(
            settings,
            foreground_cpu_usage_percent,
            total_cpu_usage_percent,
        ) {
            self.workload_engine_pressure_active = true;
            return true;
        }

        if self.workload_engine_pressure_active
            && cpu_pressure_above_restore_threshold(
                settings,
                foreground_cpu_usage_percent,
                total_cpu_usage_percent,
            )
        {
            return true;
        }

        self.workload_engine_pressure_active = false;
        false
    }

    fn release_processes(
        &mut self,
        process_ids: &[u32],
        current_process_names: Option<&BTreeMap<u32, String>>,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> PriorityFailures {
        let mut failures = PriorityFailures::default();
        let mut restored_processes = 0;
        for process_id in process_ids {
            if let Some(process) = self.adjusted.get(process_id).cloned() {
                let process_name = current_process_names
                    .and_then(|names| names.get(process_id))
                    .cloned()
                    .unwrap_or_else(|| process.process_name.clone());
                if let Err(err) = restore_adjusted_priority(*process_id, &process) {
                    if matches!(err, PriorityError::ProcessExited) {
                        self.adjusted.remove(process_id);
                    } else {
                        failures.record_error(
                            "Restore",
                            *process_id,
                            &process_name,
                            err,
                            action_log,
                        );
                    }
                } else {
                    self.adjusted.remove(process_id);
                    restored_processes += 1;
                }
            }
        }
        if restored_processes > 0 {
            action_log.record(
                ActionLogFeature::WorkloadEngine,
                None,
                "Workload Engine",
                ActionLogAction::Restore,
                ActionLogResult::Restored,
                background_priority_restore_summary_message(restored_processes, reason),
            );
        }
        failures
    }

    fn foreground_boost_stable(
        &mut self,
        process_id: u32,
        process_name: &str,
        stability_delay_ms: u64,
    ) -> bool {
        let now = Instant::now();
        if foreground_launch_boost_eligible(process_id) {
            self.foreground_candidate = Some(ForegroundCandidate {
                process_id,
                process_name: process_name.to_owned(),
                first_seen: now,
            });
            return true;
        }

        match &mut self.foreground_candidate {
            Some(candidate)
                if candidate.process_id == process_id
                    && same_process_name(&candidate.process_name, process_name) =>
            {
                now.duration_since(candidate.first_seen).as_millis()
                    >= u128::from(stability_delay_ms)
            }
            _ => {
                self.foreground_candidate = Some(ForegroundCandidate {
                    process_id,
                    process_name: process_name.to_owned(),
                    first_seen: now,
                });
                false
            }
        }
    }

    fn apply_foreground_boost_group(
        &mut self,
        group: ForegroundBoostGroup<'_>,
        action_log: &mut ActionLog,
    ) -> ForegroundBoostGroupResult {
        let ForegroundBoostGroup {
            foreground_id,
            foreground_process_name,
            targets,
            stability_delay_ms,
            foreground_boost,
            foreground_cpu_usage_percent,
        } = group;
        let mut result = ForegroundBoostGroupResult::default();
        let foreground_name = foreground_process_name.unwrap_or("").trim();
        if foreground_name.is_empty() || targets.is_empty() {
            if let Some(error) =
                self.clear_boosted(true, action_log, "foreground process is not eligible")
            {
                result.failures.merge(error);
            }
            return result;
        }

        if !self.foreground_boost_stable(foreground_id, foreground_name, stability_delay_ms) {
            if let Some(error) = self.clear_boosted(
                false,
                action_log,
                "foreground app changed before stability delay",
            ) {
                result.failures.merge(error);
            }
            return result;
        }

        let target_ids = targets.iter().map(|(id, _)| *id).collect::<BTreeSet<_>>();
        if let Err(error) = self.release_non_boost_targets(&target_ids, action_log) {
            result.failures.record_error(
                "Restore",
                foreground_id,
                foreground_name,
                error,
                action_log,
            );
        }

        let priority_class =
            foreground_boost_priority_class(foreground_boost, foreground_cpu_usage_percent);
        let mut auto_excluded_processes = BTreeSet::new();
        for (process_id, process_name) in targets {
            if self.is_process_suppressed(
                *process_id,
                process_name,
                action_log,
                &mut auto_excluded_processes,
            ) {
                result.skipped += 1;
                continue;
            }
            match self.apply_boost_process(*process_id, process_name, priority_class, action_log) {
                Ok(()) => self.clear_process_failure(process_name),
                Err(PriorityError::ProcessExited) => {
                    result.skipped += 1;
                }
                Err(PriorityError::AccessDenied) => {
                    result.skipped += 1;
                    self.record_process_failure(process_name);
                    action_log.record(
                        ActionLogFeature::WorkloadEngine,
                        Some(*process_id),
                        process_name.clone(),
                        ActionLogAction::Skip,
                        ActionLogResult::Skipped,
                        "Skipped foreground boost because the process could not be opened.",
                    );
                }
                Err(PriorityError::Failed(err)) => {
                    if is_process_exited_message(&err) {
                        result.skipped += 1;
                        continue;
                    }
                    self.record_process_failure(process_name);
                    result.failures.record_message(
                        "Boost",
                        *process_id,
                        process_name,
                        err,
                        action_log,
                    );
                }
            }
        }

        result.auto_excluded_processes = auto_excluded_processes.into_iter().collect();
        result
    }

    fn release_non_boost_targets(
        &mut self,
        target_ids: &BTreeSet<u32>,
        action_log: &mut ActionLog,
    ) -> Result<(), PriorityError> {
        let process_ids = self
            .boosted
            .keys()
            .copied()
            .filter(|process_id| !target_ids.contains(process_id))
            .collect::<Vec<_>>();
        for process_id in process_ids {
            let Some(boosted) = self.boosted.get(&process_id).cloned() else {
                continue;
            };
            let boosted_process_name = boosted.process_name.clone();
            restore_boosted_priority(&boosted)?;
            self.boosted.remove(&process_id);
            action_log.record(
                ActionLogFeature::WorkloadEngine,
                Some(process_id),
                boosted_process_name,
                ActionLogAction::Restore,
                ActionLogResult::Restored,
                "Foreground focus changed: restored previous foreground boost.",
            );
        }
        Ok(())
    }

    fn apply_boost_process(
        &mut self,
        process_id: u32,
        process_name: &str,
        priority_class: u32,
        action_log: &mut ActionLog,
    ) -> Result<(), PriorityError> {
        let process = ProcessHandle::open(process_id)?;
        let creation_time = process.creation_time_100ns()?;
        if self.boosted.get(&process_id).is_some_and(|boosted| {
            boosted.creation_time == creation_time
                && same_process_name(&boosted.process_name, process_name)
                && boosted.applied_priority == priority_class
        }) {
            return Ok(());
        }

        if let Some(boosted) = self.boosted.get(&process_id).cloned() {
            if boosted.creation_time == creation_time {
                restore_boosted_priority(&boosted)?;
            }
            self.boosted.remove(&process_id);
        }
        let current_priority = process.priority_class()?;
        if current_priority == HIGH_PRIORITY_CLASS || current_priority == REALTIME_PRIORITY_CLASS {
            return Ok(());
        }
        if current_priority != priority_class {
            process.set_priority_class(priority_class)?;
            action_log.record(
                ActionLogFeature::WorkloadEngine,
                Some(process_id),
                process_name.to_owned(),
                ActionLogAction::Apply,
                ActionLogResult::Applied,
                format!(
                    "Boosted foreground priority to {}.",
                    priority_class_label(priority_class)
                ),
            );
        }
        self.boosted.insert(
            process_id,
            BoostedProcess {
                process_id,
                process_name: process_name.to_owned(),
                creation_time,
                previous_priority: current_priority,
                applied_priority: priority_class,
            },
        );
        Ok(())
    }

    fn update_workload_engine_process(
        &mut self,
        process_id: u32,
        process_name: &str,
        settings: &WorkloadEngineSettings,
        now: Instant,
    ) -> Option<WorkloadEngineCandidate> {
        let threshold = f32::from(settings.workload_engine_threshold_percent.min(100));
        let restore_threshold = f32::from(
            settings
                .workload_engine_restore_threshold_percent
                .min(settings.workload_engine_threshold_percent)
                .min(100),
        );
        let minimum_restraint =
            Duration::from_secs(settings.workload_engine_minimum_restraint_seconds);
        let cooldown = Duration::from_secs(settings.workload_engine_cooldown_seconds);
        let state =
            self.workload_engine
                .entry(process_id)
                .or_insert_with(|| WorkloadEngineProcess {
                    process_name: process_name.to_owned(),
                    previous_cpu_time: None,
                    last_usage_tenths: None,
                    high_since: None,
                    below_since: None,
                    active_since: None,
                    last_reaction_millis: None,
                    restraint_count: 0,
                    decision: None,
                    active: false,
                    selected: false,
                });
        state.process_name = process_name.to_owned();
        let priority_sustain = workload_engine_priority_sustain(settings, state.restraint_count);

        let current = process_cpu_sample(process_id).ok()?;
        let usage = state
            .previous_cpu_time
            .and_then(|previous| process_cpu_usage_percent(previous, current));
        state.previous_cpu_time = Some(current);

        let usage = usage?;
        state.last_usage_tenths = Some(percent_tenths(usage));
        if usage >= threshold {
            state.below_since = None;
            let high_since = *state.high_since.get_or_insert(now);
            if state.active || now.duration_since(high_since) >= priority_sustain {
                if !state.active {
                    state.active_since = Some(now);
                    state.last_reaction_millis =
                        Some(duration_millis_u64(now.duration_since(high_since)));
                    state.restraint_count = state.restraint_count.saturating_add(1);
                }
                state.active = true;
                let decision = workload_engine_process_decision(settings, state.active_since, now);
                state.decision = Some(decision);
                return Some(workload_engine_candidate(process_id, state, decision, now));
            }
            return None;
        }

        state.high_since = None;
        if state.active && !state.selected {
            state.active = false;
            state.below_since = None;
            state.active_since = None;
            state.decision = None;
        } else if state.active {
            let active_since = state.active_since.unwrap_or(now);
            if usage > restore_threshold || now.duration_since(active_since) < minimum_restraint {
                state.below_since = None;
                let decision = workload_engine_process_decision(settings, state.active_since, now);
                state.decision = Some(decision);
                return Some(workload_engine_candidate(process_id, state, decision, now));
            }

            let below_since = *state.below_since.get_or_insert(now);
            if now.duration_since(below_since) < cooldown {
                state.decision = Some(WorkloadEngineDecision::LowerPriority);
                return Some(workload_engine_candidate(
                    process_id,
                    state,
                    WorkloadEngineDecision::LowerPriority,
                    now,
                ));
            }
            state.active = false;
            state.selected = false;
            state.below_since = None;
            state.active_since = None;
            state.decision = None;
        }

        None
    }

    fn update_foreground_cpu_usage(
        &mut self,
        foreground_process_ids: &BTreeSet<u32>,
    ) -> Option<f32> {
        if foreground_process_ids.is_empty() {
            self.foreground_cpu_sample = None;
            return None;
        }

        let current = process_group_cpu_sample(foreground_process_ids)?;
        let usage = self
            .foreground_cpu_sample
            .as_ref()
            .and_then(|(previous_ids, previous)| {
                (previous_ids == foreground_process_ids)
                    .then_some(*previous)
                    .and_then(|previous| process_cpu_usage_percent(previous, current))
            });
        self.foreground_cpu_sample = Some((foreground_process_ids.clone(), current));
        usage
    }

    fn workload_engine_statuses(&self, now: Instant) -> Vec<WorkloadEngineProcessStatus> {
        self.workload_engine
            .iter()
            .filter_map(|(process_id, process)| {
                let state = if process.active && process.selected {
                    if process.below_since.is_some() {
                        WorkloadEngineProcessState::CoolingDown
                    } else if process.decision == Some(WorkloadEngineDecision::RestrictAffinity) {
                        WorkloadEngineProcessState::AffinityRestrained
                    } else {
                        WorkloadEngineProcessState::Lowered
                    }
                } else if process.high_since.is_some() {
                    WorkloadEngineProcessState::Watching
                } else {
                    return None;
                };

                let elapsed_seconds = match state {
                    WorkloadEngineProcessState::Watching => process.high_since,
                    WorkloadEngineProcessState::Lowered
                    | WorkloadEngineProcessState::AffinityRestrained
                    | WorkloadEngineProcessState::CoolingDown => {
                        process.active_since.or(process.below_since)
                    }
                }
                .map(|started| now.duration_since(started).as_secs());

                Some(WorkloadEngineProcessStatus {
                    process_id: *process_id,
                    process_name: process.process_name.clone(),
                    state,
                    cpu_usage_tenths: process.last_usage_tenths,
                    elapsed_seconds,
                    reaction_millis: process.last_reaction_millis,
                    restraint_count: process.restraint_count,
                })
            })
            .collect()
    }

    fn workload_engine_core_mask(
        &mut self,
        settings: &WorkloadEngineSettings,
        foreground_cpu_usage_percent: Option<f32>,
        now: Instant,
    ) -> Option<u64> {
        let percent = workload_engine_effective_cpu_percent(settings, foreground_cpu_usage_percent);
        if percent >= 100 {
            return None;
        }

        if workload_engine_effective_restriction_mode(settings) == CpuRestrictionMode::SoftCpuSets {
            if let Some(mask) = self.load_aware_workload_engine_core_mask(
                percent,
                settings.workload_engine_max_logical_processors,
                now,
            ) {
                return Some(mask);
            }
        }

        limited_efficiency_preferred_core_mask(
            percent,
            settings.workload_engine_max_logical_processors,
        )
    }

    fn load_aware_workload_engine_core_mask(
        &mut self,
        percent: u8,
        max_logical_processors: u8,
        now: Instant,
    ) -> Option<u64> {
        let processors = core_steering::logical_processors();
        let usages = self.per_processor_usage.sample()?;
        let next_mask =
            load_aware_limited_core_mask(&processors, &usages, percent, max_logical_processors)?;

        let mask = if let Some(previous) = self.workload_engine_core_selection {
            let previous_count = previous.mask.count_ones();
            let next_count = next_mask.count_ones();
            let elapsed = now.duration_since(previous.selected_at);
            let previous_load = average_masked_core_load(previous.mask, &usages);
            let next_load = average_masked_core_load(next_mask, &usages);
            if previous_count == next_count
                && elapsed < Duration::from_secs(WORKLOAD_ENGINE_CORE_REBALANCE_INTERVAL_SECS)
                && previous_load
                    .zip(next_load)
                    .is_none_or(|(previous_load, next_load)| {
                        previous_load - next_load
                            < WORKLOAD_ENGINE_CORE_REBALANCE_IMPROVEMENT_PERCENT
                    })
            {
                previous.mask
            } else {
                next_mask
            }
        } else {
            next_mask
        };

        if self
            .workload_engine_core_selection
            .is_none_or(|selection| selection.mask != mask)
        {
            self.workload_engine_core_selection = Some(WorkloadEngineCoreSelection {
                mask,
                selected_at: now,
            });
        }
        Some(mask)
    }
}

impl Drop for WorkloadEngineManager {
    fn drop(&mut self) {
        let mut action_log = ActionLog::new(1);
        self.clear_all(&mut action_log, "Workload Engine manager dropped");
    }
}

impl Default for WorkloadEngineSnapshot {
    fn default() -> Self {
        Self {
            enabled: false,
            scanned_processes: 0,
            background_adjusted_processes: 0,
            timer_resolution_ignored_processes: 0,
            foreground_boosted_process: None,
            launch_boost_active: false,
            workload_engine_active: false,
            workload_managed_processes: 0,
            workload_engine_message: "Workload Engine disabled.".to_owned(),
            workload_engine_total_cpu_usage_tenths: None,
            adaptive_power_profile: None,
            workload_engine_details: Vec::new(),
            skipped_processes: 0,
            failed_processes: 0,
            adjusted_apps: Vec::new(),
            auto_excluded_processes: Vec::new(),
            message: "Workload Engine disabled.".to_owned(),
            last_error: None,
        }
    }
}

pub fn is_builtin_excluded(process_name: &str) -> bool {
    contains_process_name(BUILT_IN_EXCLUSIONS, process_name)
}

fn percent_tenths(usage: f32) -> u16 {
    (usage.clamp(0.0, 100.0) * 10.0).round() as u16
}

fn duration_millis_u64(duration: Duration) -> u64 {
    duration.as_millis().min(u128::from(u64::MAX)) as u64
}

fn workload_engine_status_message(
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
        return "Launch boost active: boosting the foreground app while background restraints wait."
            .to_owned();
    }

    if !running {
        return match (foreground_cpu_usage_percent, total_cpu_usage_percent) {
            (Some(usage), _) if foreground_cpu_saturates_workload(usage) => format!(
                "Paused: foreground workload is saturating CPU ({:.1}%).",
                usage.clamp(0.0, 100.0)
            ),
            (Some(foreground), Some(total)) => format!(
                "Waiting for CPU pressure: foreground {:.1}%, system {:.1}% of {}%.",
                foreground.clamp(0.0, 100.0),
                total.clamp(0.0, 100.0),
                settings.workload_engine_total_threshold_percent.min(100)
            ),
            (Some(foreground), None) => format!(
                "Waiting for CPU pressure: foreground {:.1}% of {}%.",
                foreground.clamp(0.0, 100.0),
                settings.workload_engine_total_threshold_percent.min(100)
            ),
            (None, Some(total)) => format!(
                "Waiting for CPU pressure: system {:.1}% of {}%.",
                total.clamp(0.0, 100.0),
                settings.workload_engine_total_threshold_percent.min(100)
            ),
            (None, None) => {
                "Waiting for a CPU pressure sample before Workload Engine can act.".to_owned()
            }
        };
    }

    if restrained_count == 0 {
        return "CPU pressure is high; watching background processes for sustained spikes."
            .to_owned();
    }

    format!(
        "Balancing {restrained_count} background process{} to preserve foreground work.",
        if restrained_count == 1 { "" } else { "es" }
    )
}

fn matching_rule<'a>(
    settings: &'a WorkloadEngineSettings,
    process_name: &str,
) -> Option<&'a PriorityRule> {
    settings
        .rules
        .iter()
        .find(|rule| rule.enabled && same_process_name(&rule.process_name, process_name))
}

fn should_skip_foreground_process(
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

fn foreground_boost_eligible(
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

fn should_skip_process(
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

fn foreground_process_group_ids(
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

fn workload_engine_should_run(
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

fn smart_efficiency_should_run(
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

fn cpu_pressure_should_run(
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

fn cpu_pressure_above_restore_threshold(
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

fn workload_engine_pressure_restore_threshold(settings: &WorkloadEngineSettings) -> f32 {
    let trigger = settings.workload_engine_total_threshold_percent.min(100);
    f32::from(trigger.saturating_sub(WORKLOAD_ENGINE_PRESSURE_RESTORE_BAND_PERCENT))
}

fn foreground_cpu_saturates_workload(usage: f32) -> bool {
    usage >= WORKLOAD_ENGINE_FOREGROUND_SATURATION_PERCENT
}

fn workload_engine_effective_restriction_mode(
    settings: &WorkloadEngineSettings,
) -> CpuRestrictionMode {
    if settings.lower_background_auto_cpu_percent {
        CpuRestrictionMode::SoftCpuSets
    } else {
        settings.workload_engine_affinity_mode
    }
}

fn workload_engine_affinity_mode(settings: &WorkloadEngineSettings) -> CoreSteeringMode {
    match workload_engine_effective_restriction_mode(settings) {
        CpuRestrictionMode::SoftCpuSets => CoreSteeringMode::Soft,
        CpuRestrictionMode::HardAffinity => CoreSteeringMode::Hard,
    }
}

fn workload_engine_process_decision(
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

fn workload_engine_priority_sustain(
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

fn workload_engine_candidate(
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

fn select_workload_engine_candidates(
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

fn workload_engine_effective_cpu_percent(
    settings: &WorkloadEngineSettings,
    foreground_cpu_usage_percent: Option<f32>,
) -> u8 {
    workload_engine_effective_cpu_percent_for_topology(
        settings,
        foreground_cpu_usage_percent,
        workload_engine_has_efficiency_cores(),
    )
}

fn workload_engine_effective_cpu_percent_for_topology(
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

fn workload_engine_minimum_cpu_percent_for_topology(
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

fn workload_engine_has_efficiency_cores() -> bool {
    core_steering::logical_processors()
        .iter()
        .any(|processor| processor.kind == LogicalProcessorKind::Efficiency)
}

fn limited_efficiency_preferred_core_mask(percent: u8, max_logical_processors: u8) -> Option<u64> {
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

fn load_aware_limited_core_mask(
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

fn average_masked_core_load(mask: u64, usages: &[f32]) -> Option<f32> {
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

fn logical_indices_to_limited_mask(
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

fn foreground_launch_boost_eligible(process_id: u32) -> bool {
    process_age(process_id)
        .is_some_and(|age| process_age_in_launch_boost_window(age, FOREGROUND_LAUNCH_BOOST_WINDOW))
}

fn workload_engine_launch_boost_enabled(
    settings: &WorkloadEngineSettings,
    foreground_is_launching: bool,
) -> bool {
    settings.workload_engine_enabled && foreground_is_launching
}

fn process_age_in_launch_boost_window(age: Duration, launch_window: Duration) -> bool {
    age <= launch_window
}

struct ApplyPriorityOutcome {
    adjusted: Option<AdjustedProcess>,
    skipped: bool,
    changed: bool,
}

fn apply_priority(
    request: ApplyPriorityRequest<'_>,
    action_log: &mut ActionLog,
) -> Result<ApplyPriorityOutcome, PriorityError> {
    let ApplyPriorityRequest {
        process_id,
        process_name,
        priority_class,
        existing,
        source,
        apply_priority_class,
        apply_background_efficiency,
        ignore_timer_resolution,
        disable_dynamic_priority_boost,
        log_success,
    } = request;
    let mut changed = false;
    let process = ProcessHandle::open(process_id)?;
    let creation_time = process.creation_time_100ns()?;
    let reusable_existing = existing
        .filter(|adjusted| adjusted.creation_time == creation_time)
        .filter(|adjusted| same_process_name(&adjusted.process_name, &process_name));

    if let Some(adjusted) = existing {
        if adjusted.creation_time == creation_time
            && !same_process_name(&adjusted.process_name, &process_name)
        {
            restore_adjusted_process(&process, adjusted)?;
            action_log.record(
                ActionLogFeature::WorkloadEngine,
                Some(process_id),
                adjusted.process_name.clone(),
                ActionLogAction::Restore,
                ActionLogResult::Restored,
                "PID now belongs to a different process: restored previous priority.",
            );
        }
    }

    let current_priority = process.priority_class()?;
    if current_priority == HIGH_PRIORITY_CLASS || current_priority == REALTIME_PRIORITY_CLASS {
        return Ok(ApplyPriorityOutcome {
            adjusted: None,
            skipped: true,
            changed,
        });
    }
    let previous_dynamic_priority_boost_disabled = if disable_dynamic_priority_boost {
        let current_disabled = process.dynamic_priority_boost_disabled().ok();
        if current_disabled == Some(false) {
            process.set_dynamic_priority_boost_disabled(true)?;
            changed = true;
            if log_success {
                action_log.record(
                    ActionLogFeature::WorkloadEngine,
                    Some(process_id),
                    process_name.clone(),
                    ActionLogAction::Apply,
                    ActionLogResult::Applied,
                    "Disabled Windows dynamic priority boost for Workload Engine.",
                );
            }
        }
        reusable_existing
            .and_then(|adjusted| adjusted.previous_dynamic_priority_boost_disabled)
            .or(current_disabled)
    } else {
        if let Some(adjusted) =
            reusable_existing.filter(|adjusted| adjusted.applied_dynamic_priority_boost_disabled)
        {
            if let Some(previous_disabled) = adjusted.previous_dynamic_priority_boost_disabled {
                process.set_dynamic_priority_boost_disabled(previous_disabled)?;
                changed = true;
            }
        }
        reusable_existing.and_then(|adjusted| adjusted.previous_dynamic_priority_boost_disabled)
    };
    let previous_efficiency_state = if apply_background_efficiency {
        let current_state = process.power_throttling_state().ok();
        let previous_state = reusable_existing
            .and_then(|adjusted| adjusted.previous_efficiency_state)
            .or(current_state);
        let ignore_timer_resolution_changed = reusable_existing.is_none_or(|adjusted| {
            adjusted.applied_ignore_timer_resolution != ignore_timer_resolution
        });
        let ignore_timer_resolution_missing = ignore_timer_resolution
            && !current_state.is_some_and(power_throttling_ignore_timer_resolution_enabled);
        if !current_state.is_some_and(power_throttling_execution_enabled)
            || ignore_timer_resolution_changed
            || ignore_timer_resolution_missing
        {
            process.set_power_throttling_state(power_throttling_enabled_state(
                previous_state,
                ignore_timer_resolution,
            ))?;
            changed = true;
            if log_success {
                action_log.record(
                    ActionLogFeature::WorkloadEngine,
                    Some(process_id),
                    process_name.clone(),
                    ActionLogAction::Apply,
                    ActionLogResult::Applied,
                    "Applied Background Efficiency: enabled EcoQoS.",
                );
            }
        }
        previous_state
    } else {
        if let Some(adjusted) =
            reusable_existing.filter(|adjusted| adjusted.applied_background_efficiency)
        {
            let state = adjusted
                .previous_efficiency_state
                .unwrap_or_else(power_throttling_disabled_state);
            process.set_power_throttling_state(state)?;
            changed = true;
        }
        reusable_existing.and_then(|adjusted| adjusted.previous_efficiency_state)
    };
    let mut applied_priority = current_priority;
    let mut priority_already_applied = true;
    if apply_priority_class {
        applied_priority = priority_class;
        priority_already_applied = current_priority == priority_class;
    } else if let Some(adjusted) = reusable_existing {
        if current_priority == adjusted.applied_priority
            && current_priority != adjusted.previous_priority
        {
            process.set_priority_class(adjusted.previous_priority)?;
            changed = true;
        }
        applied_priority = adjusted.previous_priority;
    }
    if reusable_existing.is_some_and(|adjusted| {
        adjusted.applied_priority == applied_priority
            && priority_already_applied
            && adjusted.applied_background_efficiency == apply_background_efficiency
            && adjusted.applied_ignore_timer_resolution == ignore_timer_resolution
            && adjusted.applied_dynamic_priority_boost_disabled == disable_dynamic_priority_boost
    }) {
        return Ok(ApplyPriorityOutcome {
            adjusted: existing.cloned(),
            skipped: false,
            changed,
        });
    }

    if apply_priority_class && current_priority != priority_class {
        process.set_priority_class(priority_class)?;
        changed = true;
        if log_success {
            action_log.record(
                ActionLogFeature::WorkloadEngine,
                Some(process_id),
                process_name.clone(),
                ActionLogAction::Apply,
                ActionLogResult::Applied,
                format!(
                    "{} set background priority to {}.",
                    priority_source_label(source),
                    priority_class_label(priority_class)
                ),
            );
        }
    }

    let previous_priority = reusable_existing
        .map(|adjusted| adjusted.previous_priority)
        .unwrap_or(current_priority);

    Ok(ApplyPriorityOutcome {
        adjusted: Some(AdjustedProcess {
            process_name,
            creation_time,
            previous_priority,
            applied_priority,
            previous_dynamic_priority_boost_disabled,
            applied_dynamic_priority_boost_disabled: disable_dynamic_priority_boost,
            previous_efficiency_state,
            applied_background_efficiency: apply_background_efficiency,
            applied_ignore_timer_resolution: apply_background_efficiency && ignore_timer_resolution,
        }),
        skipped: false,
        changed,
    })
}

fn restore_adjusted_priority(
    process_id: u32,
    process_state: &AdjustedProcess,
) -> Result<(), PriorityError> {
    let process = ProcessHandle::open(process_id)?;
    if process.creation_time_100ns()? != process_state.creation_time {
        return Err(PriorityError::ProcessExited);
    }
    restore_adjusted_process(&process, process_state)
}

fn restore_adjusted_process(
    process: &ProcessHandle,
    process_state: &AdjustedProcess,
) -> Result<(), PriorityError> {
    let mut last_error = None;
    if process_state.applied_background_efficiency {
        let state = process_state
            .previous_efficiency_state
            .unwrap_or_else(power_throttling_disabled_state);
        if let Err(err) = process.set_power_throttling_state(state) {
            last_error = Some(err);
        }
    }
    if process_state.applied_dynamic_priority_boost_disabled {
        if let Err(err) = process.set_dynamic_priority_boost_disabled(
            process_state
                .previous_dynamic_priority_boost_disabled
                .unwrap_or(false),
        ) {
            last_error = Some(err);
        }
    }
    if let Err(err) = process.set_priority_class(process_state.previous_priority) {
        last_error = Some(err);
    }
    match last_error {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

fn restore_boosted_priority(process_state: &BoostedProcess) -> Result<(), PriorityError> {
    let process = ProcessHandle::open(process_state.process_id)?;
    if process.creation_time_100ns()? != process_state.creation_time {
        return Err(PriorityError::ProcessExited);
    }
    process.set_priority_class(process_state.previous_priority)
}

fn process_cpu_sample(process_id: u32) -> Result<ProcessCpuSample, PriorityError> {
    let process = ProcessHandle::open_query(process_id)?;
    process.cpu_sample()
}

fn process_age(process_id: u32) -> Option<Duration> {
    let process = ProcessHandle::open_query(process_id).ok()?;
    let creation_time_100ns = process.creation_time_100ns().ok()?;
    let mut now = FILETIME::default();
    unsafe {
        GetSystemTimeAsFileTime(&mut now);
    }
    let age_100ns = filetime_to_u64(now).saturating_sub(creation_time_100ns);
    Some(Duration::from_nanos(age_100ns.saturating_mul(100)))
}

fn process_group_cpu_sample(process_ids: &BTreeSet<u32>) -> Option<ProcessCpuSample> {
    let sampled_at = Instant::now();
    let mut cpu_time_100ns = 0u64;
    let mut sampled_any = false;
    for process_id in process_ids {
        let sample = match process_cpu_sample(*process_id) {
            Ok(sample) => sample,
            Err(PriorityError::ProcessExited) => continue,
            Err(PriorityError::AccessDenied | PriorityError::Failed(_)) => continue,
        };
        cpu_time_100ns = cpu_time_100ns.saturating_add(sample.cpu_time_100ns);
        sampled_any = true;
    }

    sampled_any.then_some(ProcessCpuSample {
        cpu_time_100ns,
        sampled_at,
    })
}

fn power_throttling_disabled_state() -> PROCESS_POWER_THROTTLING_STATE {
    PROCESS_POWER_THROTTLING_STATE {
        Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
        StateMask: 0,
    }
}

fn power_throttling_enabled_state(
    previous: Option<PROCESS_POWER_THROTTLING_STATE>,
    ignore_timer_resolution: bool,
) -> PROCESS_POWER_THROTTLING_STATE {
    let previous_ignore_timer_resolution = previous.is_some_and(|state| {
        (state.StateMask & PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION) != 0
    });
    let mut state = previous.unwrap_or_else(power_throttling_disabled_state);
    state.Version = PROCESS_POWER_THROTTLING_CURRENT_VERSION;
    state.ControlMask |=
        PROCESS_POWER_THROTTLING_EXECUTION_SPEED | PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION;
    state.StateMask |= PROCESS_POWER_THROTTLING_EXECUTION_SPEED;
    if ignore_timer_resolution || previous_ignore_timer_resolution {
        state.StateMask |= PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION;
    } else {
        state.StateMask &= !PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION;
    }
    state
}

fn power_throttling_execution_enabled(state: PROCESS_POWER_THROTTLING_STATE) -> bool {
    (state.StateMask & PROCESS_POWER_THROTTLING_EXECUTION_SPEED) != 0
}

fn power_throttling_ignore_timer_resolution_enabled(state: PROCESS_POWER_THROTTLING_STATE) -> bool {
    (state.StateMask & PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION) != 0
}

fn ignore_timer_resolution_allowed(
    process_id: u32,
    active_audio_process_ids: Option<&BTreeSet<u32>>,
) -> bool {
    active_audio_process_ids.is_some_and(|ids| !ids.contains(&process_id))
}

enum PriorityError {
    AccessDenied,
    ProcessExited,
    Failed(String),
}

fn priority_error_message(error: &PriorityError) -> String {
    match error {
        PriorityError::AccessDenied => "Access denied.".to_owned(),
        PriorityError::ProcessExited => "Process exited.".to_owned(),
        PriorityError::Failed(message) => message.clone(),
    }
}

#[derive(Default)]
struct PriorityFailures {
    count: usize,
    last_error: Option<String>,
}

impl PriorityFailures {
    fn merge(&mut self, other: Self) {
        self.count += other.count;
        if self.last_error.is_none() {
            self.last_error = other.last_error;
        }
    }

    fn record_error(
        &mut self,
        action: &str,
        process_id: u32,
        process_name: &str,
        error: PriorityError,
        action_log: &mut ActionLog,
    ) {
        let message = match error {
            PriorityError::AccessDenied => "Access denied.".to_owned(),
            PriorityError::ProcessExited => return,
            PriorityError::Failed(message) => message,
        };
        self.record_message(action, process_id, process_name, message, action_log);
    }

    fn record_message(
        &mut self,
        action: &str,
        process_id: u32,
        process_name: &str,
        message: String,
        action_log: &mut ActionLog,
    ) {
        if is_process_exited_message(&message) {
            return;
        }
        self.count += 1;
        if self.last_error.is_none() {
            self.last_error = Some(process_failure_message(
                action,
                process_id,
                process_name,
                &message,
            ));
        }
        action_log.record(
            ActionLogFeature::WorkloadEngine,
            Some(process_id),
            process_name.to_owned(),
            ActionLogAction::Fail,
            ActionLogResult::Failed,
            message,
        );
    }
}

fn process_failure_message(
    action: &str,
    process_id: u32,
    process_name: &str,
    message: &str,
) -> String {
    let name = if process_name.is_empty() {
        "process"
    } else {
        process_name
    };
    format!("{action} {name} ({process_id}): {message}")
}

fn priority_source_label(source: PriorityTargetSource) -> &'static str {
    match source {
        PriorityTargetSource::WorkloadEngine => "Workload Engine",
        PriorityTargetSource::BackgroundPolicy => "Background policy",
        PriorityTargetSource::Rule => "Rule",
    }
}

fn background_apply_summary_message(count: usize) -> String {
    if count == 1 {
        "Applied Workload Engine background restraint to 1 process.".to_owned()
    } else {
        format!("Applied Workload Engine background restraint to {count} processes.")
    }
}

fn background_priority_restore_summary_message(count: usize, reason: &str) -> String {
    format!(
        "Restored background priority for {}: {reason}.",
        process_count_label(count)
    )
}

fn foreground_boost_restore_summary_message(count: usize, reason: &str) -> String {
    format!(
        "Restored foreground boost for {}: {reason}.",
        process_count_label(count)
    )
}

fn background_apply_summary_log_due(last_logged_at: Option<Instant>, now: Instant) -> bool {
    last_logged_at
        .is_none_or(|last| now.duration_since(last) >= BACKGROUND_APPLY_SUMMARY_LOG_INTERVAL)
}

fn priority_class_label(priority_class: u32) -> &'static str {
    match priority_class {
        NORMAL_PRIORITY_CLASS => "Normal",
        BELOW_NORMAL_PRIORITY_CLASS => "Below Normal",
        IDLE_PRIORITY_CLASS => "Idle",
        ABOVE_NORMAL_PRIORITY_CLASS => "Above Normal",
        HIGH_PRIORITY_CLASS => "High",
        REALTIME_PRIORITY_CLASS => "Realtime",
        _ => "Unknown",
    }
}

struct ProcessHandle(WinHandle);

impl ProcessHandle {
    fn open(process_id: u32) -> Result<Self, PriorityError> {
        let handle = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_INFORMATION,
                0,
                process_id,
            )
        };
        if !handle.is_null() {
            Ok(Self(WinHandle::new(handle)))
        } else {
            Err(open_process_error(process_id, last_error()))
        }
    }

    fn open_query(process_id: u32) -> Result<Self, PriorityError> {
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
        if !handle.is_null() {
            Ok(Self(WinHandle::new(handle)))
        } else {
            Err(open_process_error(process_id, last_error()))
        }
    }

    fn priority_class(&self) -> Result<u32, PriorityError> {
        let priority = unsafe { GetPriorityClass(self.0.raw()) };
        if priority == 0 {
            Err(PriorityError::Failed(format!(
                "GetPriorityClass failed with error {}.",
                last_error()
            )))
        } else {
            Ok(priority)
        }
    }

    fn set_priority_class(&self, priority_class: u32) -> Result<(), PriorityError> {
        let ok = unsafe { SetPriorityClass(self.0.raw(), priority_class) };
        if ok == 0 {
            Err(PriorityError::Failed(format!(
                "SetPriorityClass failed with error {}.",
                last_error()
            )))
        } else {
            Ok(())
        }
    }

    fn dynamic_priority_boost_disabled(&self) -> Result<bool, PriorityError> {
        let mut disabled = 0;
        let ok = unsafe { GetProcessPriorityBoost(self.0.raw(), &mut disabled) };
        if ok == 0 {
            Err(PriorityError::Failed(format!(
                "GetProcessPriorityBoost failed with error {}.",
                last_error()
            )))
        } else {
            Ok(disabled != 0)
        }
    }

    fn set_dynamic_priority_boost_disabled(&self, disabled: bool) -> Result<(), PriorityError> {
        let ok = unsafe { SetProcessPriorityBoost(self.0.raw(), i32::from(disabled)) };
        if ok == 0 {
            Err(PriorityError::Failed(format!(
                "SetProcessPriorityBoost failed with error {}.",
                last_error()
            )))
        } else {
            Ok(())
        }
    }

    fn power_throttling_state(&self) -> Result<PROCESS_POWER_THROTTLING_STATE, PriorityError> {
        let mut state = PROCESS_POWER_THROTTLING_STATE::default();
        let ok = unsafe {
            GetProcessInformation(
                self.0.raw(),
                ProcessPowerThrottling,
                &mut state as *mut _ as *mut c_void,
                std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
            )
        };
        if ok == 0 {
            Err(PriorityError::Failed(format!(
                "GetProcessInformation ProcessPowerThrottling failed with error {}.",
                last_error()
            )))
        } else {
            Ok(state)
        }
    }

    fn set_power_throttling_state(
        &self,
        state: PROCESS_POWER_THROTTLING_STATE,
    ) -> Result<(), PriorityError> {
        let ok = unsafe {
            SetProcessInformation(
                self.0.raw(),
                ProcessPowerThrottling,
                &state as *const _ as *const c_void,
                std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
            )
        };
        if ok == 0 {
            Err(PriorityError::Failed(format!(
                "SetProcessInformation ProcessPowerThrottling failed with error {}.",
                last_error()
            )))
        } else {
            Ok(())
        }
    }

    fn cpu_sample(&self) -> Result<ProcessCpuSample, PriorityError> {
        let mut creation = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        let ok = unsafe {
            GetProcessTimes(
                self.0.raw(),
                &mut creation,
                &mut exit,
                &mut kernel,
                &mut user,
            )
        };
        if ok == 0 {
            Err(PriorityError::Failed(format!(
                "GetProcessTimes failed with error {}.",
                last_error()
            )))
        } else {
            Ok(ProcessCpuSample {
                cpu_time_100ns: filetime_to_u64(kernel).saturating_add(filetime_to_u64(user)),
                sampled_at: Instant::now(),
            })
        }
    }

    fn creation_time_100ns(&self) -> Result<u64, PriorityError> {
        let mut creation = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        let ok = unsafe {
            GetProcessTimes(
                self.0.raw(),
                &mut creation,
                &mut exit,
                &mut kernel,
                &mut user,
            )
        };
        if ok == 0 {
            Err(PriorityError::Failed(format!(
                "GetProcessTimes failed with error {}.",
                last_error()
            )))
        } else {
            Ok(filetime_to_u64(creation))
        }
    }
}

fn open_process_error(process_id: u32, error: u32) -> PriorityError {
    match error {
        ERROR_ACCESS_DENIED => PriorityError::AccessDenied,
        ERROR_INVALID_PARAMETER => PriorityError::ProcessExited,
        _ => PriorityError::Failed(format!(
            "OpenProcess({process_id}) failed with error {error}."
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ProcessMemoryPrioritySetting, WorkloadEngineSettings};

    #[test]
    fn repeated_failures_suppress_future_workload_engine_attempts_once() {
        let mut manager = WorkloadEngineManager::default();
        let mut log = ActionLog::new(8);

        manager.record_process_failure("APP.exe");
        manager.record_process_failure("app.exe");
        assert!(!manager.is_process_suppressed(42, "app.exe", &mut log, &mut BTreeSet::new()));
        assert!(log.entries().is_empty());

        manager.record_process_failure("app.exe");
        assert!(manager.is_process_suppressed(42, "app.exe", &mut log, &mut BTreeSet::new()));
        assert!(manager.is_process_suppressed(43, "APP.exe", &mut log, &mut BTreeSet::new()));

        let entries = log.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].process_name, "app.exe");
        assert_eq!(entries[0].action, ActionLogAction::Skip);
        assert_eq!(entries[0].result, ActionLogResult::Skipped);
    }

    #[test]
    fn priority_mapping_uses_safe_classes() {
        assert_eq!(
            process_priority_class(ProcessPriority::Normal),
            NORMAL_PRIORITY_CLASS
        );
        assert_eq!(
            process_priority_class(ProcessPriority::BelowNormal),
            BELOW_NORMAL_PRIORITY_CLASS
        );
        assert_eq!(
            process_priority_class(ProcessPriority::Idle),
            IDLE_PRIORITY_CLASS
        );
        assert_eq!(
            foreground_boost_priority_class(ForegroundBoostPriority::Auto, None),
            ABOVE_NORMAL_PRIORITY_CLASS
        );
        assert_eq!(
            foreground_boost_priority_class(ForegroundBoostPriority::Auto, Some(85.0)),
            NORMAL_PRIORITY_CLASS
        );
        assert_eq!(
            foreground_boost_priority_class(ForegroundBoostPriority::AboveNormal, Some(100.0)),
            ABOVE_NORMAL_PRIORITY_CLASS
        );
    }

    #[test]
    fn foreground_launch_boost_window_includes_new_processes_only() {
        assert!(process_age_in_launch_boost_window(
            Duration::from_secs(2),
            FOREGROUND_LAUNCH_BOOST_WINDOW,
        ));
        assert!(process_age_in_launch_boost_window(
            FOREGROUND_LAUNCH_BOOST_WINDOW,
            FOREGROUND_LAUNCH_BOOST_WINDOW,
        ));
        assert!(!process_age_in_launch_boost_window(
            FOREGROUND_LAUNCH_BOOST_WINDOW + Duration::from_millis(1),
            FOREGROUND_LAUNCH_BOOST_WINDOW,
        ));
    }

    #[test]
    fn launch_boost_runs_for_any_workload_engine_preset_while_app_is_launching() {
        let settings = WorkloadEngineSettings {
            workload_engine_enabled: true,
            boost_foreground_app: false,
            ..Default::default()
        };

        assert!(workload_engine_launch_boost_enabled(&settings, true));
        assert!(!workload_engine_launch_boost_enabled(&settings, false));
        assert!(!workload_engine_launch_boost_enabled(
            &WorkloadEngineSettings {
                workload_engine_enabled: false,
                ..settings.clone()
            },
            true
        ));
    }

    #[test]
    fn workload_engine_status_reports_launch_boost_before_cpu_waiting() {
        let settings = WorkloadEngineSettings {
            workload_engine_enabled: true,
            ..Default::default()
        };

        assert_eq!(
            workload_engine_status_message(&settings, None, None, true, false, 0),
            "Launch boost active: boosting the foreground app while background restraints wait."
        );
    }

    #[test]
    fn background_apply_summary_message_uses_process_count() {
        assert_eq!(
            background_apply_summary_message(1),
            "Applied Workload Engine background restraint to 1 process."
        );
        assert_eq!(
            background_apply_summary_message(3),
            "Applied Workload Engine background restraint to 3 processes."
        );
    }

    #[test]
    fn workload_engine_restore_summary_messages_use_process_count() {
        assert_eq!(
            background_priority_restore_summary_message(1, "process no longer matches a Workload Engine rule"),
            "Restored background priority for 1 process: process no longer matches a Workload Engine rule."
        );
        assert_eq!(
            background_priority_restore_summary_message(20, "process no longer matches a Workload Engine rule"),
            "Restored background priority for 20 processes: process no longer matches a Workload Engine rule."
        );
        assert_eq!(
            foreground_boost_restore_summary_message(17, "foreground app changed before stability delay"),
            "Restored foreground boost for 17 processes: foreground app changed before stability delay."
        );
    }

    #[test]
    fn background_apply_summary_log_is_rate_limited() {
        let now = Instant::now();

        assert!(background_apply_summary_log_due(None, now));
        assert!(!background_apply_summary_log_due(Some(now), now));
        assert!(!background_apply_summary_log_due(
            Some(now),
            now + BACKGROUND_APPLY_SUMMARY_LOG_INTERVAL - Duration::from_millis(1)
        ));
        assert!(background_apply_summary_log_due(
            Some(now),
            now + BACKGROUND_APPLY_SUMMARY_LOG_INTERVAL
        ));
    }

    #[test]
    fn matching_rule_is_case_insensitive() {
        let settings = WorkloadEngineSettings {
            enabled: true,
            lower_background_apps: true,
            workload_engine_background_efficiency_enabled: true,
            workload_engine_background_priority: ProcessPriority::BelowNormal,
            lower_background_io_priority_enabled: false,
            lower_background_io_priority: crate::config::ProcessIoPriority::VeryLow,
            workload_engine_io_priority: crate::config::IoPrioritySettings::default(),
            workload_engine_thread_priority: crate::config::ThreadPrioritySettings::default(),
            workload_engine_dynamic_priority_boost:
                crate::config::DynamicPriorityBoostSettings::default(),
            workload_engine_gpu_priority: crate::config::GpuPrioritySettings::default(),
            workload_engine_memory_priority_enabled: false,
            workload_engine_foreground_memory_priority: ProcessMemoryPrioritySetting::Default,
            workload_engine_memory_priority: crate::config::ProcessMemoryPriority::Low,
            lower_background_auto_cpu_percent: false,
            workload_engine_enabled: false,
            workload_engine_advanced_settings_enabled: false,
            workload_engine_affinity_escalation_enabled: false,
            workload_engine_affinity_mode: CpuRestrictionMode::SoftCpuSets,
            workload_engine_cpu_percent: 50,
            workload_engine_max_logical_processors: 0,
            workload_engine_total_threshold_percent: 70,
            workload_engine_threshold_percent: 25,
            workload_engine_restore_threshold_percent: 5,
            workload_engine_sustain_seconds: 2,
            workload_engine_minimum_restraint_seconds: 4,
            workload_engine_cooldown_seconds: 10,
            workload_engine_max_targeted_processes: 6,
            workload_engine_exclusions: Vec::new(),
            boost_foreground_app: false,
            foreground_boost: ForegroundBoostPriority::AboveNormal,
            foreground_stability_delay_ms: 750,
            rules: vec![PriorityRule {
                enabled: true,
                process_name: " Worker.EXE ".to_owned(),
                priority: ProcessPriority::BelowNormal,
            }],
        };

        assert!(matching_rule(&settings, "worker.exe").is_some());
        assert!(matching_rule(&settings, "other.exe").is_none());
    }

    #[test]
    fn builtin_exclusions_cover_system_shell_processes() {
        assert!(is_builtin_excluded("explorer.exe"));
        assert!(is_builtin_excluded("winlogon.exe"));
        assert!(!is_builtin_excluded("browser.exe"));
    }

    #[test]
    fn foreground_skip_matches_pid_or_process_name() {
        let foreground_group = BTreeSet::from([42]);
        assert!(should_skip_foreground_process(
            42,
            "helper.exe",
            Some(42),
            &foreground_group,
            Some("app.exe"),
        ));
        assert!(should_skip_foreground_process(
            99,
            "APP.EXE",
            Some(42),
            &foreground_group,
            Some("app.exe"),
        ));
        assert!(!should_skip_foreground_process(
            99,
            "other.exe",
            Some(42),
            &foreground_group,
            Some("app.exe"),
        ));
    }

    #[test]
    fn foreground_group_includes_child_processes() {
        let processes = vec![
            ProcessInfo {
                id: 42,
                parent_id: None,
                name: "foreground.exe".to_owned(),
            },
            ProcessInfo {
                id: 99,
                parent_id: Some(42),
                name: "worker.exe".to_owned(),
            },
            ProcessInfo {
                id: 100,
                parent_id: Some(99),
                name: "helper.exe".to_owned(),
            },
            ProcessInfo {
                id: 101,
                parent_id: None,
                name: "background.exe".to_owned(),
            },
        ];

        let group = foreground_process_group_ids(&processes, Some(42));

        assert!(group.contains(&42));
        assert!(group.contains(&99));
        assert!(group.contains(&100));
        assert!(!group.contains(&101));
    }

    #[test]
    fn workload_engine_pauses_when_foreground_saturates_cpu() {
        let settings = WorkloadEngineSettings {
            workload_engine_enabled: true,
            workload_engine_total_threshold_percent: 70,
            ..Default::default()
        };

        assert!(!workload_engine_should_run(&settings, Some(69.0), None));
        assert!(workload_engine_should_run(&settings, Some(70.0), None));
        assert!(!workload_engine_should_run(&settings, Some(85.0), None));
        assert!(!workload_engine_should_run(&settings, Some(100.0), None));
        assert!(!workload_engine_should_run(
            &settings,
            Some(85.0),
            Some(100.0)
        ));
    }

    #[test]
    fn workload_engine_runs_under_system_cpu_pressure() {
        let settings = WorkloadEngineSettings {
            workload_engine_enabled: true,
            workload_engine_total_threshold_percent: 70,
            ..Default::default()
        };

        assert!(!workload_engine_should_run(
            &settings,
            Some(10.0),
            Some(69.0)
        ));
        assert!(workload_engine_should_run(
            &settings,
            Some(10.0),
            Some(70.0)
        ));
        assert!(workload_engine_should_run(&settings, None, Some(70.0)));
    }

    #[test]
    fn workload_engine_pressure_uses_restore_band_before_stopping() {
        let settings = WorkloadEngineSettings {
            workload_engine_enabled: true,
            workload_engine_total_threshold_percent: 70,
            workload_engine_restore_threshold_percent: 20,
            ..Default::default()
        };
        let mut manager = WorkloadEngineManager::default();

        assert!(!manager.update_workload_engine_pressure(&settings, Some(10.0), Some(69.0)));
        assert!(manager.update_workload_engine_pressure(&settings, Some(10.0), Some(70.0)));
        assert!(manager.update_workload_engine_pressure(&settings, Some(10.0), Some(66.0)));
        assert!(!manager.update_workload_engine_pressure(&settings, Some(10.0), Some(64.0)));
    }

    #[test]
    fn workload_engine_selects_highest_scored_candidates() {
        let max_targeted = 6;
        let candidates = (0..=u32::from(max_targeted))
            .map(|process_id| WorkloadEngineCandidate {
                process_id,
                process_name: format!("app{process_id}.exe"),
                decision: WorkloadEngineDecision::LowerPriority,
                score: process_id,
            })
            .collect::<Vec<_>>();

        let selected = select_workload_engine_candidates(candidates, max_targeted);

        assert_eq!(selected.len(), usize::from(max_targeted));
        assert!(!selected.iter().any(|candidate| candidate.process_id == 0));
    }

    #[test]
    fn workload_engine_selection_can_replace_cooler_selected_process() {
        let now = Instant::now();
        let selected = WorkloadEngineProcess {
            process_name: "selected.exe".to_owned(),
            previous_cpu_time: None,
            last_usage_tenths: Some(100),
            high_since: Some(now - Duration::from_secs(60)),
            below_since: None,
            active_since: Some(now - Duration::from_secs(60)),
            last_reaction_millis: Some(100),
            restraint_count: 3,
            decision: Some(WorkloadEngineDecision::LowerPriority),
            active: true,
            selected: true,
        };
        let hotter = WorkloadEngineProcess {
            process_name: "hotter.exe".to_owned(),
            previous_cpu_time: None,
            last_usage_tenths: Some(900),
            high_since: Some(now),
            below_since: None,
            active_since: Some(now),
            last_reaction_millis: Some(100),
            restraint_count: 0,
            decision: Some(WorkloadEngineDecision::LowerPriority),
            active: true,
            selected: false,
        };

        let selected = select_workload_engine_candidates(
            vec![
                workload_engine_candidate(1, &selected, WorkloadEngineDecision::LowerPriority, now),
                workload_engine_candidate(2, &hotter, WorkloadEngineDecision::LowerPriority, now),
            ],
            1,
        );

        assert_eq!(selected[0].process_id, 2);
    }

    #[test]
    fn workload_engine_status_treats_unselected_hot_process_as_watching() {
        let now = Instant::now();
        let mut manager = WorkloadEngineManager::default();
        manager.workload_engine.insert(
            42,
            WorkloadEngineProcess {
                process_name: "worker.exe".to_owned(),
                previous_cpu_time: None,
                last_usage_tenths: Some(900),
                high_since: Some(now - Duration::from_secs(2)),
                below_since: None,
                active_since: Some(now - Duration::from_secs(1)),
                last_reaction_millis: Some(100),
                restraint_count: 1,
                decision: Some(WorkloadEngineDecision::RestrictAffinity),
                active: true,
                selected: false,
            },
        );

        let statuses = manager.workload_engine_statuses(now);

        assert_eq!(statuses.len(), 1);
        assert_eq!(statuses[0].state, WorkloadEngineProcessState::Watching);
        assert_eq!(statuses[0].elapsed_seconds, Some(2));
    }

    #[test]
    fn workload_engine_cpu_percent_relaxes_under_moderate_pressure() {
        let settings = WorkloadEngineSettings {
            lower_background_auto_cpu_percent: false,
            workload_engine_cpu_percent: 50,
            workload_engine_total_threshold_percent: 70,
            ..Default::default()
        };

        assert_eq!(workload_engine_effective_cpu_percent(&settings, None), 50);
        assert_eq!(
            workload_engine_effective_cpu_percent(&settings, Some(70.0)),
            75
        );
        assert_eq!(
            workload_engine_effective_cpu_percent(&settings, Some(77.5)),
            63
        );
        assert_eq!(
            workload_engine_effective_cpu_percent(&settings, Some(85.0)),
            50
        );
    }

    #[test]
    fn workload_engine_auto_cpu_percent_uses_topology_behavior_floor() {
        let settings = WorkloadEngineSettings {
            lower_background_auto_cpu_percent: true,
            workload_engine_cpu_percent: 25,
            workload_engine_total_threshold_percent: 75,
            ..Default::default()
        };

        assert_eq!(
            workload_engine_minimum_cpu_percent_for_topology(&settings, true),
            70
        );
        let foreground_first = WorkloadEngineSettings {
            workload_engine_total_threshold_percent: 45,
            ..settings.clone()
        };
        assert_eq!(
            workload_engine_minimum_cpu_percent_for_topology(&foreground_first, true),
            50
        );
        assert_eq!(
            workload_engine_effective_cpu_percent_for_topology(&settings, Some(75.0), true),
            100
        );
        assert_eq!(
            workload_engine_effective_cpu_percent_for_topology(&settings, Some(80.0), true),
            85
        );

        assert_eq!(
            workload_engine_minimum_cpu_percent_for_topology(&settings, false),
            65
        );
        assert_eq!(
            workload_engine_effective_cpu_percent_for_topology(&settings, Some(75.0), false),
            100
        );
        assert_eq!(
            workload_engine_effective_cpu_percent_for_topology(&settings, Some(80.0), false),
            83
        );
    }

    #[test]
    fn workload_engine_auto_mode_escalates_from_priority_to_affinity() {
        let settings = WorkloadEngineSettings {
            lower_background_auto_cpu_percent: true,
            workload_engine_affinity_escalation_enabled: false,
            workload_engine_sustain_seconds: 3,
            ..Default::default()
        };
        let now = Instant::now();

        assert_eq!(
            workload_engine_process_decision(&settings, Some(now), now + Duration::from_secs(2)),
            WorkloadEngineDecision::LowerPriority
        );
        assert_eq!(
            workload_engine_process_decision(&settings, Some(now), now + Duration::from_secs(3)),
            WorkloadEngineDecision::LowerPriority
        );

        let escalating_settings = WorkloadEngineSettings {
            workload_engine_affinity_escalation_enabled: true,
            ..settings.clone()
        };
        assert_eq!(
            workload_engine_process_decision(
                &escalating_settings,
                Some(now),
                now + Duration::from_secs(3)
            ),
            WorkloadEngineDecision::RestrictAffinity
        );

        let manual_settings = WorkloadEngineSettings {
            lower_background_auto_cpu_percent: false,
            ..escalating_settings
        };
        assert_eq!(
            workload_engine_process_decision(&manual_settings, Some(now), now),
            WorkloadEngineDecision::RestrictAffinity
        );
    }

    #[test]
    fn workload_engine_fast_priority_sustain_is_immediate() {
        let fast_settings = WorkloadEngineSettings {
            lower_background_auto_cpu_percent: true,
            workload_engine_sustain_seconds: 1,
            ..Default::default()
        };
        assert_eq!(
            workload_engine_priority_sustain(&fast_settings, 0),
            Duration::ZERO
        );

        let balanced_settings = WorkloadEngineSettings {
            lower_background_auto_cpu_percent: true,
            workload_engine_sustain_seconds: 2,
            ..Default::default()
        };
        assert_eq!(
            workload_engine_priority_sustain(&balanced_settings, 0),
            Duration::from_secs(2)
        );
        assert_eq!(
            workload_engine_priority_sustain(&balanced_settings, 1),
            Duration::from_secs(1)
        );

        let manual_settings = WorkloadEngineSettings {
            lower_background_auto_cpu_percent: false,
            workload_engine_sustain_seconds: 1,
            ..Default::default()
        };
        assert_eq!(
            workload_engine_priority_sustain(&manual_settings, 1),
            Duration::from_secs(1)
        );
    }

    #[test]
    fn smart_efficiency_auto_mode_runs_under_cpu_pressure() {
        let settings = WorkloadEngineSettings {
            lower_background_auto_cpu_percent: true,
            workload_engine_total_threshold_percent: 75,
            ..Default::default()
        };

        assert!(!smart_efficiency_should_run(&settings, None, None));
        assert!(!smart_efficiency_should_run(&settings, Some(74.0), None));
        assert!(smart_efficiency_should_run(&settings, Some(75.0), None));
        assert!(smart_efficiency_should_run(
            &settings,
            Some(10.0),
            Some(75.0)
        ));
        assert!(!smart_efficiency_should_run(
            &settings,
            Some(85.0),
            Some(100.0)
        ));

        let manual_settings = WorkloadEngineSettings {
            lower_background_auto_cpu_percent: false,
            ..settings
        };
        assert!(smart_efficiency_should_run(&manual_settings, None, None));
    }

    #[test]
    fn load_aware_core_mask_picks_low_load_standard_processors() {
        let processors = vec![
            LogicalProcessorInfo {
                index: 0,
                core_index: 0,
                kind: LogicalProcessorKind::Standard,
                efficiency_class: 0,
            },
            LogicalProcessorInfo {
                index: 1,
                core_index: 1,
                kind: LogicalProcessorKind::Standard,
                efficiency_class: 0,
            },
            LogicalProcessorInfo {
                index: 2,
                core_index: 2,
                kind: LogicalProcessorKind::Standard,
                efficiency_class: 0,
            },
            LogicalProcessorInfo {
                index: 3,
                core_index: 3,
                kind: LogicalProcessorKind::Standard,
                efficiency_class: 0,
            },
        ];
        let usages = vec![80.0, 10.0, 70.0, 20.0];

        let mask = load_aware_limited_core_mask(&processors, &usages, 50, 0).unwrap();

        assert_eq!(mask, 0b1010);
    }

    #[test]
    fn load_aware_core_mask_prefers_efficiency_processors() {
        let processors = vec![
            LogicalProcessorInfo {
                index: 0,
                core_index: 0,
                kind: LogicalProcessorKind::Performance,
                efficiency_class: 1,
            },
            LogicalProcessorInfo {
                index: 1,
                core_index: 1,
                kind: LogicalProcessorKind::Efficiency,
                efficiency_class: 0,
            },
            LogicalProcessorInfo {
                index: 2,
                core_index: 2,
                kind: LogicalProcessorKind::Efficiency,
                efficiency_class: 0,
            },
            LogicalProcessorInfo {
                index: 3,
                core_index: 3,
                kind: LogicalProcessorKind::Performance,
                efficiency_class: 1,
            },
        ];
        let usages = vec![1.0, 90.0, 10.0, 2.0];

        let mask = load_aware_limited_core_mask(&processors, &usages, 50, 0).unwrap();

        assert_eq!(mask, 0b0100);
    }

    #[test]
    fn release_processes_skips_restore_when_process_identity_is_unknown() {
        let mut manager = WorkloadEngineManager::default();
        manager.adjusted.insert(
            0,
            AdjustedProcess {
                process_name: "exited.exe".to_owned(),
                creation_time: 0,
                previous_priority: NORMAL_PRIORITY_CLASS,
                applied_priority: BELOW_NORMAL_PRIORITY_CLASS,
                previous_dynamic_priority_boost_disabled: None,
                applied_dynamic_priority_boost_disabled: false,
                previous_efficiency_state: None,
                applied_background_efficiency: false,
                applied_ignore_timer_resolution: false,
            },
        );
        let mut log = ActionLog::new(8);

        let failures = manager.release_processes(&[0], Some(&BTreeMap::new()), &mut log, "test");

        assert_eq!(failures.count, 0);
        assert!(log.entries().is_empty());
        assert!(manager.adjusted.is_empty());
    }

    #[test]
    fn process_cpu_usage_percent_scales_by_processor_count() {
        let now = Instant::now();
        let previous = ProcessCpuSample {
            cpu_time_100ns: 0,
            sampled_at: now,
        };
        let current = ProcessCpuSample {
            cpu_time_100ns: 10_000_000,
            sampled_at: now + Duration::from_secs(1),
        };

        let usage = process_cpu_usage_percent(previous, current).unwrap();

        assert!(usage > 0.0);
        assert!(usage <= 100.0);
    }

    #[test]
    fn power_throttling_state_sets_timer_ignore_only_when_allowed() {
        let allowed = power_throttling_enabled_state(None, true);
        assert_ne!(
            allowed.StateMask & PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
            0
        );
        assert_ne!(
            allowed.StateMask & PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
            0
        );

        let blocked = power_throttling_enabled_state(None, false);
        assert_ne!(
            blocked.StateMask & PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
            0
        );
        assert_eq!(
            blocked.StateMask & PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
            0
        );
    }
}
