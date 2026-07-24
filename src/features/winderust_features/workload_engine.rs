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
    action_log::{ActionLog, ActionLogFeature, ActionLogResult},
    audio_activity::active_audio_process_ids,
    config::{
        CoreSteeringMode, CoreSteeringRule, CoreSteeringSettings, CpuRestrictionMode,
        ForegroundBoostPriority, PriorityRule, ProcessPriority, WorkloadEngineSettings,
    },
    core_steering::{self, CoreSteeringManager, LogicalProcessorInfo, LogicalProcessorKind},
    cpu::{process_cpu_usage_percent, PerProcessorUsageMonitor, ProcessCpuSample},
    foreground::{
        contains_process_name, list_processes, process_count_label, process_failure_key,
        process_names_by_id, process_session_id, same_process_name, unique_app_names, ProcessInfo,
        EXTENDED_BUILT_IN_PROCESS_EXCLUSIONS,
    },
    memory_priority::{MemoryPriorityManager, MemoryPriorityTarget},
    rules::{execution_failure_suppression_threshold, ExecutionFailureTracker},
    win_util::{filetime_to_u64, last_error, WinHandle},
};

mod policy;
mod process_control;

pub use policy::{foreground_boost_priority_class, is_builtin_excluded};

use policy::*;
use process_control::*;

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

        // SAFETY: GetCurrentProcessId takes no arguments and has no caller requirements.
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
            failures.last_error = lower_background_affinity_snapshot.last_error;
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
                        ActionLogResult::Skipped,
                        "Skipped because the process could not be opened.",
                    );
                }
                Err(error) => {
                    let err = priority_error_message(&error);
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
                        ActionLogResult::Skipped,
                        "Skipped foreground boost because the process could not be opened.",
                    );
                }
                Err(PriorityError::Failed(err)) => {
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

#[cfg(test)]
mod tests;
