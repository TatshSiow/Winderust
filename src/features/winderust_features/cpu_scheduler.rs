use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use windows_sys::Win32::System::Threading::GetCurrentProcessId;

use crate::{
    action_log::{ActionLog, ActionLogFeature, ActionLogResult},
    config::{
        BackgroundProcessorSelection, CpuAllocationMethod, CpuSchedulerSettings,
        ProcessPrioritySetting,
    },
    control::{
        cpu_allocation::CpuAllocationCoordinator,
        memory_priority::MemoryPriorityController,
        priority_efficiency::{
            PowerThrottlingClaim, PriorityClassClaim, PriorityClassPreservation,
            PriorityClassValue, PriorityEfficiencyController, PriorityEfficiencyReleaseSummary,
            ProcessPropertyApplyOutcome,
        },
        process::{ControlOwner, ProcessControlError, ProcessControlTarget, ProcessTargetKey},
    },
    cpu::{
        process_cpu_demand_percent, process_cpu_usage_percent, PerProcessorUsageMonitor,
        ProcessCpuSample,
    },
    cpu_allocation::{
        self, CpuAllocationManager, CpuAllocationMode, CpuAllocationTarget, LogicalProcessorInfo,
        LogicalProcessorKind,
    },
    foreground::{
        contains_process_name, process_count_label, process_executable_path, process_failure_key,
        process_session_id, same_executable_path, same_process_name, unique_app_names, ProcessInfo,
        ProtectedProcesses, EXTENDED_BUILT_IN_PROCESS_EXCLUSIONS,
    },
    memory_priority::{MemoryPriorityManager, MemoryPriorityTarget},
    rules::{execution_failure_suppression_threshold, ExecutionFailureTracker},
    runtime::observations::CycleObservations,
};

mod policy;
mod process_control;

pub use policy::is_builtin_excluded;

use policy::*;
use process_control::*;

const BUILT_IN_EXCLUSIONS: &[&str] = EXTENDED_BUILT_IN_PROCESS_EXCLUSIONS;
const CPU_SCHEDULER_RECOVERY_BAND_PERCENT: u8 = 5;
const CPU_SCHEDULER_CORE_REBALANCE_INTERVAL_SECS: u64 = 3;
const CPU_SCHEDULER_CORE_REBALANCE_IMPROVEMENT_PERCENT: f32 = 15.0;
const CPU_SCHEDULER_SELECTION_STICKINESS_TENTHS: u32 = 50;
const BACKGROUND_APPLY_SUMMARY_LOG_INTERVAL: Duration = Duration::from_secs(30);
const FOCUS_AND_LAUNCH_PROFILE_WINDOW: Duration = Duration::from_secs(8);
const FOCUS_PROCESS_PRIORITY_STABILITY_DELAY_MS: u64 = 750;
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CpuSchedulerSnapshot {
    pub enabled: bool,
    pub scanned_processes: usize,
    pub adjusted_processes: usize,
    pub focus_and_launch_profile_active: bool,
    pub cpu_pressure_restraint_active: bool,
    pub foreground_cpu_usage_tenths: Option<u16>,
    pub skipped_processes: usize,
    pub failed_processes: usize,
    pub adjusted_apps: Vec<String>,
    pub auto_excluded_processes: Vec<String>,
    pub message: String,
    pub last_error: Option<String>,
}

pub struct CpuSchedulerManager {
    focus_process_candidate: Option<FocusProcessCandidate>,
    foreground_cpu_sample: Option<(BTreeSet<u32>, ProcessCpuSample)>,
    tracked_processes: BTreeMap<u32, CpuSchedulerProcess>,
    background_pressure_active: bool,
    cpu_allocation: CpuAllocationManager,
    background_memory_priority: MemoryPriorityManager,
    cpu_allocation_selection: Option<CpuAllocationSelection>,
    last_background_apply_summary_logged_at: Option<Instant>,
    per_processor_usage: PerProcessorUsageMonitor,
    failure_suppression: ExecutionFailureTracker,
    unavailable_power_targets: BTreeSet<ProcessTargetKey>,
}

impl Default for CpuSchedulerManager {
    fn default() -> Self {
        Self {
            focus_process_candidate: None,
            foreground_cpu_sample: None,
            tracked_processes: BTreeMap::new(),
            background_pressure_active: false,
            cpu_allocation: CpuAllocationManager::with_action_log_feature(
                ActionLogFeature::CpuScheduler,
            ),
            background_memory_priority: MemoryPriorityManager::default(),
            cpu_allocation_selection: None,
            last_background_apply_summary_logged_at: None,
            per_processor_usage: PerProcessorUsageMonitor::default(),
            failure_suppression: ExecutionFailureTracker::default(),
            unavailable_power_targets: BTreeSet::new(),
        }
    }
}

struct FocusProcessCandidate {
    process_id: u32,
    process_name: String,
    executable_path: String,
    creation_time: u64,
    first_seen: Instant,
}

#[derive(Default)]
struct FocusProcessPriorityGroupResult {
    skipped: usize,
    failures: PriorityFailures,
    auto_excluded_processes: Vec<String>,
}

#[derive(Clone)]
struct CpuSchedulerProcess {
    process_name: String,
    executable_path: String,
    creation_time: u64,
    previous_cpu_time: Option<ProcessCpuSample>,
    last_usage_tenths: Option<u16>,
    high_since: Option<Instant>,
    below_since: Option<Instant>,
    active_since: Option<Instant>,
    decision: Option<CpuSchedulerDecision>,
    active: bool,
    selected: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CpuSchedulerDecision {
    LowerPriority,
    LimitProcessors,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CpuSchedulerTier {
    VisibleWindow,
    Background,
}

#[derive(Clone)]
struct CpuSchedulerCandidate {
    process_id: u32,
    process_name: String,
    decision: CpuSchedulerDecision,
    tier: CpuSchedulerTier,
    score: u32,
}

#[derive(Clone, Copy)]
struct CpuAllocationSelection {
    mask: u64,
    kind: Option<LogicalProcessorKind>,
    selected_at: Instant,
}

struct PressureTargetPolicy {
    priority: Option<PriorityClassValue>,
    preservation: PriorityClassPreservation,
    apply_background_efficiency: bool,
}

struct PriorityTarget {
    process_name: String,
    executable_path: String,
    creation_time: u64,
    priority: Option<PriorityClassValue>,
    preservation: PriorityClassPreservation,
    apply_background_efficiency: bool,
}

pub struct CpuSchedulerUpdate<'a> {
    pub settings: &'a CpuSchedulerSettings,
    pub automation_enabled: bool,
    pub allow_cross_session_process_control: bool,
    pub foreground_process_id: Option<u32>,
    pub total_cpu_usage_percent: Option<f32>,
    pub background_efficiency_managed: bool,
    pub excluded_process_ids: &'a BTreeSet<u32>,
    pub explicit_cpu_allocation_paths: &'a [String],
    pub observations: &'a mut CycleObservations,
}

struct FocusProcessPriorityGroup<'a> {
    foreground_id: u32,
    foreground_process_name: Option<&'a str>,
    targets: &'a [(u32, String, String, u64)],
    stability_delay_ms: u64,
    priority: PriorityClassValue,
    preservation: PriorityClassPreservation,
}

impl CpuSchedulerManager {
    pub fn update(
        &mut self,
        input: CpuSchedulerUpdate<'_>,
        cpu_allocation_coordinator: &mut CpuAllocationCoordinator,
        priority_efficiency_controller: &mut PriorityEfficiencyController,
        memory_priority_controller: &mut MemoryPriorityController,
        action_log: &mut ActionLog,
    ) -> CpuSchedulerSnapshot {
        let CpuSchedulerUpdate {
            settings,
            automation_enabled,
            allow_cross_session_process_control,
            foreground_process_id,
            total_cpu_usage_percent,
            background_efficiency_managed,
            excluded_process_ids,
            explicit_cpu_allocation_paths,
            observations,
        } = input;

        if !automation_enabled {
            let failed = self.clear_all_with_memory_priority(
                cpu_allocation_coordinator,
                priority_efficiency_controller,
                memory_priority_controller,
                action_log,
                "automation disabled",
            );
            self.failure_suppression.clear();
            return CpuSchedulerSnapshot {
                enabled: false,
                failed_processes: failed.count,
                message: "Automation disabled.".to_owned(),
                last_error: failed.last_error,
                ..Default::default()
            };
        }

        if !settings.cpu_pressure_restraint_enabled && !settings.limit_background_processors_enabled
        {
            let failed = self.clear_all_with_memory_priority(
                cpu_allocation_coordinator,
                priority_efficiency_controller,
                memory_priority_controller,
                action_log,
                "CPU Scheduler disabled",
            );
            self.failure_suppression.clear();
            return CpuSchedulerSnapshot {
                enabled: false,
                failed_processes: failed.count,
                message: "CPU Scheduler disabled.".to_owned(),
                last_error: failed.last_error,
                ..Default::default()
            };
        }

        // SAFETY: GetCurrentProcessId takes no arguments and has no caller requirements.
        let current_process_id = unsafe { GetCurrentProcessId() };
        let Some(current_session_id) = process_session_id(current_process_id) else {
            let failed = self.clear_all_with_memory_priority(
                cpu_allocation_coordinator,
                priority_efficiency_controller,
                memory_priority_controller,
                action_log,
                "current Windows session is unknown",
            );
            return CpuSchedulerSnapshot {
                enabled: true,
                failed_processes: failed.count,
                message: "Paused: current Windows session is unknown.".to_owned(),
                last_error: failed.last_error,
                ..Default::default()
            };
        };

        let exact_paths_required = settings
            .custom_rules
            .iter()
            .any(|rule| rule.enabled && Path::new(rule.executable_path.trim()).is_absolute());
        let processes = match if exact_paths_required {
            observations.processes_with_paths()
        } else {
            observations.processes()
        } {
            Ok(processes) => processes,
            Err(err) => {
                let failed = self.clear_all_with_memory_priority(
                    cpu_allocation_coordinator,
                    priority_efficiency_controller,
                    memory_priority_controller,
                    action_log,
                    "process list unavailable",
                );
                return CpuSchedulerSnapshot {
                    enabled: true,
                    failed_processes: failed.count,
                    message: err,
                    last_error: failed.last_error,
                    ..Default::default()
                };
            }
        };

        let scanned_processes = processes.len();
        let Ok(visible_window_process_ids) = observations.visible_window_process_ids() else {
            let failed = self.clear_all_with_memory_priority(
                cpu_allocation_coordinator,
                priority_efficiency_controller,
                memory_priority_controller,
                action_log,
                "visible windows are unavailable",
            );
            return CpuSchedulerSnapshot {
                enabled: true,
                failed_processes: failed.count,
                message: "Paused: visible windows are unavailable.".to_owned(),
                last_error: failed.last_error,
                ..Default::default()
            };
        };
        let visible_processes = ProtectedProcesses::capture(
            processes.as_ref(),
            false,
            None,
            visible_window_process_ids,
        );
        let processes_by_id = processes
            .iter()
            .map(|process| (process.id, process))
            .collect::<BTreeMap<_, _>>();
        let mut executable_paths = processes
            .iter()
            .filter_map(|process| {
                process
                    .image_path
                    .as_ref()
                    .map(|path| (process.id, path.to_string_lossy().into_owned()))
            })
            .collect::<BTreeMap<_, _>>();
        let foreground_process_name = foreground_process_id
            .and_then(|id| processes_by_id.get(&id).map(|process| process.name.clone()));
        let foreground_process_excluded = foreground_process_id
            .and_then(|id| processes_by_id.get(&id))
            .and_then(|process| cached_executable_path(process, &mut executable_paths))
            .is_some_and(|path| settings.custom_rule_enabled_for(&path));
        let foreground_process_group_ids =
            foreground_process_group_ids(processes.as_ref(), foreground_process_id);
        let visible_window_process_group_ids = processes
            .iter()
            .filter(|process| !foreground_process_group_ids.contains(&process.id))
            .filter_map(|process| {
                process_executable_path(process).and_then(|path| {
                    visible_processes
                        .contains(process.id, &path)
                        .then_some(process.id)
                })
            })
            .collect::<BTreeSet<_>>();
        let foreground_cpu_usage_percent =
            self.update_foreground_cpu_usage(&foreground_process_group_ids);
        let foreground_cpu_usage_tenths = foreground_cpu_usage_percent.map(percent_tenths);

        let mut failures = PriorityFailures::default();
        let mut restrainable_processes = BTreeMap::new();
        for process in processes.iter() {
            if process.is_critical != Some(false)
                || !process.can_set_information
                || should_skip_process(
                    process.id,
                    &process.name,
                    current_process_id,
                    foreground_process_id,
                    &foreground_process_group_ids,
                    excluded_process_ids,
                )
            {
                continue;
            }

            if !allow_cross_session_process_control
                && process_session_id(process.id) != Some(current_session_id)
            {
                continue;
            }

            let tier = if visible_window_process_group_ids.contains(&process.id) {
                CpuSchedulerTier::VisibleWindow
            } else {
                CpuSchedulerTier::Background
            };
            restrainable_processes.insert(process.id, (process.name.clone(), tier));
        }

        let mut target_processes = BTreeMap::new();
        let focus_and_launch_profile_target = foreground_process_id
            .zip(foreground_process_name.as_deref())
            .is_some_and(|(process_id, process_name)| {
                !excluded_process_ids.contains(&process_id)
                    && !foreground_process_excluded
                    && focus_process_priority_eligible(
                        process_id,
                        process_name,
                        current_process_id,
                        current_session_id,
                    )
                    && focus_and_launch_profile_eligible(process_id)
            });
        let focus_and_launch_profile_active = settings.process_priority_enabled
            && focus_and_launch_profile_enabled(settings, focus_and_launch_profile_target);
        let background_pressure_triggered = self.update_background_pressure(
            settings,
            foreground_cpu_usage_percent,
            total_cpu_usage_percent,
        );
        let background_pressure_applies =
            background_pressure_triggered && !focus_and_launch_profile_active;
        let cpu_pressure_restraint_applies =
            settings.cpu_pressure_restraint_enabled && background_pressure_applies;
        let cpu_allocation_applies =
            settings.limit_background_processors_enabled && background_pressure_applies;
        let dynamic_resource_zones_apply =
            settings.dynamic_resource_zones_enabled && cpu_allocation_applies;
        let mut auto_excluded_processes = BTreeSet::new();

        let mut cpu_allocation_targets = Vec::new();
        let mut cpu_scheduler_memory_targets = Vec::new();
        if settings.cpu_pressure_restraint_enabled && settings.memory_priority_enabled {
            for process in processes
                .iter()
                .filter(|process| process.is_critical == Some(false) && process.can_set_information)
                .filter(|process| foreground_process_group_ids.contains(&process.id))
                .filter(|process| !excluded_process_ids.contains(&process.id))
                .filter(|process| {
                    !settings.custom_rules.iter().any(|rule| rule.enabled)
                        || crate::foreground::process_executable_path(process).is_some_and(|path| {
                            !settings.custom_rule_enabled_for(path.to_string_lossy().as_ref())
                        })
                })
                .filter(|process| {
                    focus_process_priority_eligible(
                        process.id,
                        &process.name,
                        current_process_id,
                        current_session_id,
                    )
                })
            {
                let (value, foreground, visible_window) = memory_priority_policy(
                    settings,
                    true,
                    visible_window_process_group_ids.contains(&process.id),
                );
                let Some(priority) = value.priority() else {
                    continue;
                };
                let Some(executable_path) = cached_executable_path(process, &mut executable_paths)
                else {
                    continue;
                };
                let Some(creation_time) = process.creation_time else {
                    continue;
                };
                cpu_scheduler_memory_targets.push(MemoryPriorityTarget {
                    process_id: process.id,
                    process_name: process.name.clone(),
                    executable_path,
                    creation_time,
                    priority,
                    foreground,
                    visible_window,
                    preserve_foreground_priority: settings.memory_priority_preserve_foreground,
                    preserve_visible_window_priority: settings
                        .memory_priority_preserve_visible_window,
                    preserve_background_priority: settings.memory_priority_preserve_background,
                });
            }
        }
        if background_pressure_applies {
            if cpu_pressure_restraint_applies {
                for (process_id, (_, tier)) in &restrainable_processes {
                    let Some(process) = processes_by_id.get(process_id) else {
                        continue;
                    };
                    let Some(executable_path) =
                        cached_executable_path(process, &mut executable_paths)
                    else {
                        continue;
                    };
                    let Some(creation_time) = process.creation_time else {
                        continue;
                    };
                    if settings.custom_rule_enabled_for(&executable_path) {
                        continue;
                    }
                    let Some(policy) = cpu_pressure_restraint_target(
                        settings,
                        *tier,
                        background_efficiency_managed,
                    ) else {
                        continue;
                    };
                    target_processes.insert(
                        *process_id,
                        PriorityTarget {
                            process_name: process.name.clone(),
                            executable_path,
                            creation_time,
                            priority: policy.priority,
                            preservation: policy.preservation,
                            apply_background_efficiency: policy.apply_background_efficiency,
                        },
                    );
                }
            }

            let now = Instant::now();
            let allocation_percent = if dynamic_resource_zones_apply {
                dynamic_background_zone_percent(settings.processor_limit_percent)
            } else {
                settings.processor_limit_percent
            };
            let cpu_allocation_mask = cpu_allocation_applies
                .then(|| self.cpu_allocation_mask(settings, allocation_percent, now))
                .flatten();
            if dynamic_resource_zones_apply {
                let processors = cpu_allocation::logical_processors();
                let all_mask = cpu_allocation::logical_processor_mask(&processors);
                if let Some((foreground_mask, _background_mask)) =
                    cpu_allocation_mask.and_then(|background_mask| {
                        dynamic_resource_zone_masks(all_mask, background_mask)
                    })
                {
                    for process_id in &foreground_process_group_ids {
                        let Some(process) = processes_by_id.get(process_id) else {
                            continue;
                        };
                        if process.is_critical != Some(false)
                            || !process.can_set_information
                            || excluded_process_ids.contains(process_id)
                            || !focus_process_priority_eligible(
                                *process_id,
                                &process.name,
                                current_process_id,
                                current_session_id,
                            )
                        {
                            continue;
                        }
                        let Some(executable_path) =
                            cached_executable_path(process, &mut executable_paths)
                        else {
                            continue;
                        };
                        if settings.custom_rule_enabled_for(&executable_path)
                            || cpu_allocation::contains_process(
                                explicit_cpu_allocation_paths,
                                &executable_path,
                            )
                        {
                            continue;
                        }
                        let Some(creation_time) = process.creation_time else {
                            continue;
                        };
                        cpu_allocation_targets.push(CpuAllocationTarget {
                            process_id: *process_id,
                            process_name: process.name.clone(),
                            executable_path,
                            mode: CpuAllocationMode::SoftCpuSets,
                            core_mask: foreground_mask,
                            creation_time,
                        });
                    }
                }
            }
            let current_ids = restrainable_processes
                .keys()
                .copied()
                .collect::<BTreeSet<_>>();
            self.tracked_processes
                .retain(|process_id, _| current_ids.contains(process_id));

            let mut cpu_scheduler_candidates = Vec::new();
            for (process_id, (process_name, tier)) in &restrainable_processes {
                if *tier == CpuSchedulerTier::VisibleWindow
                    && !settings.cpu_pressure_restraint_enabled
                {
                    continue;
                }
                let Some(process) = processes_by_id.get(process_id) else {
                    continue;
                };
                let Some(executable_path) = cached_executable_path(process, &mut executable_paths)
                else {
                    continue;
                };
                if settings.custom_rule_enabled_for(&executable_path) {
                    continue;
                }

                if let Some(candidate) = self.update_cpu_scheduler_process(
                    *process_id,
                    process_name,
                    &executable_path,
                    settings,
                    *tier,
                    now,
                ) {
                    cpu_scheduler_candidates.push(candidate);
                }
            }

            let selected_candidates = select_cpu_scheduler_candidates(
                cpu_scheduler_candidates,
                settings.maximum_restrained_apps,
            );
            let selected_ids = selected_candidates
                .iter()
                .map(|candidate| candidate.process_id)
                .collect::<BTreeSet<_>>();
            for (process_id, process) in &mut self.tracked_processes {
                if process.active && !selected_ids.contains(process_id) {
                    process.selected = false;
                    process.decision = None;
                }
            }

            for candidate in selected_candidates {
                let Some(process) = processes_by_id.get(&candidate.process_id) else {
                    continue;
                };
                let Some(executable_path) = cached_executable_path(process, &mut executable_paths)
                else {
                    continue;
                };
                let Some(creation_time) = process.creation_time else {
                    continue;
                };
                if let Some(process) = self.tracked_processes.get_mut(&candidate.process_id) {
                    process.selected = true;
                    process.decision = Some(candidate.decision);
                }
                if settings.cpu_pressure_restraint_enabled && settings.memory_priority_enabled {
                    let (value, foreground, visible_window) = memory_priority_policy(
                        settings,
                        false,
                        candidate.tier == CpuSchedulerTier::VisibleWindow,
                    );
                    let priority = value.priority();
                    if let Some(priority) = priority {
                        cpu_scheduler_memory_targets.push(MemoryPriorityTarget {
                            process_id: candidate.process_id,
                            process_name: candidate.process_name.clone(),
                            executable_path: executable_path.clone(),
                            creation_time,
                            priority,
                            foreground,
                            visible_window,
                            preserve_foreground_priority: settings
                                .memory_priority_preserve_foreground,
                            preserve_visible_window_priority: settings
                                .memory_priority_preserve_visible_window,
                            preserve_background_priority: settings
                                .memory_priority_preserve_background,
                        });
                    }
                }
                if candidate.decision == CpuSchedulerDecision::LimitProcessors
                    && candidate.tier == CpuSchedulerTier::Background
                    && !cpu_allocation::contains_process(
                        explicit_cpu_allocation_paths,
                        &executable_path,
                    )
                {
                    if let (Some(core_mask), Some(creation_time)) = (
                        cpu_allocation_mask,
                        self.tracked_processes
                            .get(&candidate.process_id)
                            .map(|process| process.creation_time),
                    ) {
                        cpu_allocation_targets.push(CpuAllocationTarget {
                            process_id: candidate.process_id,
                            process_name: candidate.process_name.clone(),
                            executable_path,
                            mode: if dynamic_resource_zones_apply {
                                CpuAllocationMode::SoftCpuSets
                            } else {
                                cpu_allocation_method(settings)
                            },
                            core_mask,
                            creation_time,
                        });
                    }
                }
            }
        } else {
            self.tracked_processes.clear();
            self.cpu_allocation_selection = None;
        }

        let cpu_allocation_snapshot = if cpu_allocation_applies {
            self.cpu_allocation.update_discovered_targets(
                cpu_allocation_coordinator,
                ControlOwner::AdaptiveEngine,
                cpu_allocation_targets,
                scanned_processes,
                "CPU Scheduler active.",
                allow_cross_session_process_control,
                action_log,
            )
        } else {
            self.cpu_allocation.update_discovered_targets(
                cpu_allocation_coordinator,
                ControlOwner::AdaptiveEngine,
                Vec::new(),
                scanned_processes,
                "CPU Scheduler idle.",
                allow_cross_session_process_control,
                action_log,
            )
        };
        auto_excluded_processes.extend(
            cpu_allocation_snapshot
                .auto_excluded_processes
                .iter()
                .cloned(),
        );
        failures.count += cpu_allocation_snapshot.failed_processes;
        if failures.last_error.is_none() {
            failures.last_error = cpu_allocation_snapshot.last_error.clone();
        }
        let cpu_scheduler_memory_snapshot = self.background_memory_priority.update(
            memory_priority_controller,
            ControlOwner::AdaptiveEngine,
            if cpu_pressure_restraint_applies && settings.memory_priority_enabled {
                cpu_scheduler_memory_targets
            } else {
                Vec::new()
            },
            automation_enabled,
            allow_cross_session_process_control,
            ActionLogFeature::CpuScheduler,
            action_log,
        );
        auto_excluded_processes.extend(
            cpu_scheduler_memory_snapshot
                .auto_excluded_processes
                .iter()
                .cloned(),
        );
        failures.count += cpu_scheduler_memory_snapshot.failed_processes;
        if failures.last_error.is_none() {
            failures.last_error = cpu_scheduler_memory_snapshot.last_error.clone();
        }

        let active_priority_targets = target_processes
            .iter()
            .filter(|(_, target)| target.priority.is_some())
            .map(|(process_id, target)| cpu_scheduler_priority_target_key(*process_id, target))
            .collect::<BTreeSet<_>>();
        let active_power_targets = target_processes
            .iter()
            .filter(|(_, target)| target.apply_background_efficiency)
            .map(|(process_id, target)| cpu_scheduler_priority_target_key(*process_id, target))
            .collect::<BTreeSet<_>>();
        self.unavailable_power_targets
            .retain(|target| active_power_targets.contains(target));
        let mut active_target_names = target_processes
            .values()
            .map(|target| process_failure_key(&target.executable_path))
            .collect::<BTreeSet<_>>();
        if let Some(process) = foreground_process_id.and_then(|id| processes_by_id.get(&id)) {
            if let Some(path) = cached_executable_path(process, &mut executable_paths) {
                active_target_names.insert(process_failure_key(&path));
            }
        }
        self.failure_suppression.retain_keys(&active_target_names);
        self.merge_controller_release(
            priority_efficiency_controller.release_priority_policy_except(
                ControlOwner::AdaptiveEngine,
                &active_priority_targets,
            ),
            action_log,
            "process no longer needs CPU Scheduler restraint",
            "process priority",
            &mut failures,
        );
        self.merge_controller_release(
            priority_efficiency_controller
                .release_power_policy_except(ControlOwner::AdaptiveEngine, &active_power_targets),
            action_log,
            "process no longer needs CPU Scheduler restraint",
            "Background Efficiency",
            &mut failures,
        );
        let mut skipped_processes = 0;
        skipped_processes += cpu_scheduler_memory_snapshot.skipped_processes;
        let mut summarized_background_applies = 0;

        for (process_id, target) in target_processes {
            let failure_process_name = target.process_name.clone();
            if self.is_executable_path_suppressed(
                process_id,
                &failure_process_name,
                &target.executable_path,
                action_log,
                &mut auto_excluded_processes,
            ) {
                skipped_processes += 1;
                continue;
            }
            let control_target = ProcessControlTarget::automatic(
                process_id,
                target.process_name.clone(),
                PathBuf::from(&target.executable_path),
                target.creation_time,
            );
            let mut changed = false;
            let mut skipped = false;
            let mut hard_failure = false;
            let target_key = control_target.key();

            if target.apply_background_efficiency {
                if self.unavailable_power_targets.contains(&target_key) {
                    skipped = true;
                } else {
                    match priority_efficiency_controller.apply_power_claim(
                        PowerThrottlingClaim {
                            target: control_target.clone(),
                            owner: ControlOwner::AdaptiveEngine,
                            ignore_timer_resolution: false,
                        },
                        allow_cross_session_process_control,
                    ) {
                        Ok(ProcessPropertyApplyOutcome::Applied) => {
                            self.unavailable_power_targets.remove(&target_key);
                            changed = true;
                        }
                        Ok(
                            ProcessPropertyApplyOutcome::Unchanged
                            | ProcessPropertyApplyOutcome::Preserved
                            | ProcessPropertyApplyOutcome::Shadowed,
                        ) => {
                            self.unavailable_power_targets.remove(&target_key);
                        }
                        Err(ProcessControlError::Unavailable(error)) => {
                            skipped = true;
                            self.unavailable_power_targets.insert(target_key.clone());
                            action_log.record(
                            ActionLogFeature::CpuScheduler,
                            Some(process_id),
                            target.process_name.clone(),
                            ActionLogResult::Skipped,
                            format!(
                                "Skipped Background Efficiency because its original state is unavailable: {error}"
                            ),
                        );
                        }
                        Err(ProcessControlError::ProcessExited) => skipped = true,
                        Err(ProcessControlError::AccessDenied(error)) => {
                            skipped = true;
                            self.failure_suppression
                                .suppress_process_failure(&target.executable_path);
                            action_log.record(
                                ActionLogFeature::CpuScheduler,
                                Some(process_id),
                                target.process_name.clone(),
                                ActionLogResult::Skipped,
                                error,
                            );
                        }
                        Err(error) => {
                            hard_failure = true;
                            self.record_process_failure(&target.executable_path);
                            failures.record_control_error(
                                "Apply Background Efficiency",
                                process_id,
                                &failure_process_name,
                                error,
                                action_log,
                            );
                        }
                    }
                }
            }

            if let Some(priority) = target.priority {
                match priority_efficiency_controller.apply_priority_claim(
                    PriorityClassClaim {
                        target: control_target,
                        owner: ControlOwner::AdaptiveEngine,
                        priority,
                        preservation: target.preservation,
                    },
                    allow_cross_session_process_control,
                ) {
                    Ok(ProcessPropertyApplyOutcome::Applied) => {
                        changed = true;
                    }
                    Ok(ProcessPropertyApplyOutcome::Preserved) => skipped = true,
                    Ok(
                        ProcessPropertyApplyOutcome::Unchanged
                        | ProcessPropertyApplyOutcome::Shadowed,
                    ) => {}
                    Err(ProcessControlError::ProcessExited) => skipped = true,
                    Err(ProcessControlError::AccessDenied(error)) => {
                        skipped = true;
                        self.failure_suppression
                            .suppress_process_failure(&target.executable_path);
                        action_log.record(
                            ActionLogFeature::CpuScheduler,
                            Some(process_id),
                            target.process_name.clone(),
                            ActionLogResult::Skipped,
                            error,
                        );
                    }
                    Err(error) => {
                        hard_failure = true;
                        self.record_process_failure(&target.executable_path);
                        failures.record_control_error(
                            "Apply priority",
                            process_id,
                            &failure_process_name,
                            error,
                            action_log,
                        );
                    }
                }
            }

            if !hard_failure {
                self.clear_process_failure(&target.executable_path);
            }
            if skipped {
                skipped_processes += 1;
            }
            if changed {
                summarized_background_applies += 1;
            }
        }
        let now = Instant::now();
        if summarized_background_applies > 0
            && background_apply_summary_log_due(self.last_background_apply_summary_logged_at, now)
        {
            self.last_background_apply_summary_logged_at = Some(now);
            action_log.record(
                ActionLogFeature::CpuScheduler,
                None,
                "CPU Scheduler",
                ActionLogResult::Applied,
                background_apply_summary_message(summarized_background_applies),
            );
        }

        if let Some(foreground_id) = foreground_process_id {
            let (configured_priority, preservation) = process_priority_policy(
                settings,
                true,
                visible_window_process_group_ids.contains(&foreground_id),
            );
            let foreground_priority = if settings.process_priority_enabled
                && (cpu_pressure_restraint_applies || focus_and_launch_profile_active)
            {
                if focus_and_launch_profile_active
                    && settings.process_priority_foreground_detection_enabled
                {
                    Some(PriorityClassValue::AboveNormal)
                } else {
                    configured_priority
                }
            } else {
                None
            };
            if let Some(priority) =
                foreground_priority.filter(|_| !excluded_process_ids.contains(&foreground_id))
            {
                let boost_targets = processes
                    .iter()
                    .filter(|process| {
                        process.is_critical == Some(false) && process.can_set_information
                    })
                    .filter(|process| foreground_process_group_ids.contains(&process.id))
                    .filter(|process| !excluded_process_ids.contains(&process.id))
                    .filter(|process| {
                        focus_process_priority_eligible(
                            process.id,
                            &process.name,
                            current_process_id,
                            current_session_id,
                        )
                    })
                    .filter_map(|process| {
                        cached_executable_path(process, &mut executable_paths).and_then(|path| {
                            if settings.custom_rule_enabled_for(&path) {
                                return None;
                            }
                            process.creation_time.map(|creation_time| {
                                (process.id, process.name.clone(), path, creation_time)
                            })
                        })
                    })
                    .collect::<Vec<_>>();
                let result = self.apply_focus_process_priority_group(
                    FocusProcessPriorityGroup {
                        foreground_id,
                        foreground_process_name: foreground_process_name.as_deref(),
                        targets: &boost_targets,
                        stability_delay_ms: if focus_and_launch_profile_active {
                            0
                        } else {
                            FOCUS_PROCESS_PRIORITY_STABILITY_DELAY_MS
                        },
                        priority,
                        preservation,
                    },
                    priority_efficiency_controller,
                    allow_cross_session_process_control,
                    action_log,
                );
                skipped_processes += result.skipped;
                auto_excluded_processes.extend(result.auto_excluded_processes);
                failures.merge(result.failures);
            } else if let Some(error) = self.clear_focus_process_priority(
                priority_efficiency_controller,
                true,
                action_log,
                "focus process priority disabled or blocked",
            ) {
                failures.merge(error);
            }
        } else if let Some(error) = self.clear_focus_process_priority(
            priority_efficiency_controller,
            true,
            action_log,
            "foreground app is unknown",
        ) {
            failures.merge(error);
        }

        CpuSchedulerSnapshot {
            enabled: true,
            scanned_processes,
            adjusted_processes: priority_efficiency_controller
                .policy_managed_process_count(ControlOwner::AdaptiveEngine)
                .max(cpu_allocation_snapshot.adjusted_processes),
            focus_and_launch_profile_active,
            cpu_pressure_restraint_active: cpu_pressure_restraint_applies,
            foreground_cpu_usage_tenths,
            skipped_processes,
            failed_processes: failures.count,
            adjusted_apps: unique_app_names(
                priority_efficiency_controller
                    .policy_managed_process_names(ControlOwner::AdaptiveEngine)
                    .iter()
                    .map(String::as_str),
            ),
            auto_excluded_processes: auto_excluded_processes.into_iter().collect(),
            message: "CPU Scheduler active.".to_owned(),
            last_error: failures.last_error,
        }
    }

    fn clear_all_with_memory_priority(
        &mut self,
        cpu_allocation_coordinator: &mut CpuAllocationCoordinator,
        priority_efficiency_controller: &mut PriorityEfficiencyController,
        memory_priority_controller: &mut MemoryPriorityController,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> PriorityFailures {
        let mut failures = self.clear_all(
            cpu_allocation_coordinator,
            priority_efficiency_controller,
            action_log,
            reason,
        );
        let memory_snapshot = self.background_memory_priority.update(
            memory_priority_controller,
            ControlOwner::AdaptiveEngine,
            Vec::new(),
            true,
            true,
            ActionLogFeature::CpuScheduler,
            action_log,
        );
        failures.count += memory_snapshot.failed_processes;
        if failures.last_error.is_none() {
            failures.last_error = memory_snapshot.last_error;
        }
        failures
    }

    fn clear_all(
        &mut self,
        cpu_allocation_coordinator: &mut CpuAllocationCoordinator,
        priority_efficiency_controller: &mut PriorityEfficiencyController,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> PriorityFailures {
        let mut failures = self
            .clear_focus_process_priority(priority_efficiency_controller, true, action_log, reason)
            .unwrap_or_default();
        self.merge_controller_release(
            priority_efficiency_controller
                .release_all_priority_policy(ControlOwner::AdaptiveEngine),
            action_log,
            reason,
            "process priority",
            &mut failures,
        );
        self.merge_controller_release(
            priority_efficiency_controller.release_all_power_policy(ControlOwner::AdaptiveEngine),
            action_log,
            reason,
            "Background Efficiency",
            &mut failures,
        );
        self.focus_process_candidate = None;
        self.foreground_cpu_sample = None;
        self.tracked_processes.clear();
        self.background_pressure_active = false;
        self.last_background_apply_summary_logged_at = None;
        self.unavailable_power_targets.clear();
        let cpu_allocation_snapshot = self.cpu_allocation.update_discovered_targets(
            cpu_allocation_coordinator,
            ControlOwner::AdaptiveEngine,
            Vec::new(),
            0,
            "CPU Scheduler disabled.",
            true,
            action_log,
        );
        failures.count += cpu_allocation_snapshot.failed_processes;
        if failures.last_error.is_none() {
            failures.last_error = cpu_allocation_snapshot.last_error;
        }
        failures
    }

    fn clear_focus_process_priority(
        &mut self,
        priority_efficiency_controller: &mut PriorityEfficiencyController,
        reset_candidate: bool,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> Option<PriorityFailures> {
        if reset_candidate {
            self.focus_process_candidate = None;
        }
        let summary = priority_efficiency_controller
            .release_all_priority_policy(ControlOwner::CpuSchedulerFocusPriority);
        if summary.restored_processes == 0 && summary.failures.is_empty() {
            return None;
        }
        let mut failures = PriorityFailures::default();
        self.merge_controller_release(
            summary,
            action_log,
            reason,
            "focus process priority",
            &mut failures,
        );
        Some(failures)
    }

    fn merge_controller_release(
        &mut self,
        summary: PriorityEfficiencyReleaseSummary,
        action_log: &mut ActionLog,
        reason: &str,
        property: &str,
        failures: &mut PriorityFailures,
    ) {
        if summary.restored_processes > 0 {
            action_log.record(
                ActionLogFeature::CpuScheduler,
                None,
                "CPU Scheduler",
                ActionLogResult::Restored,
                format!(
                    "Restored {property} for {}: {reason}.",
                    process_count_label(summary.restored_processes)
                ),
            );
        }
        for failure in summary.failures {
            self.record_process_failure(&failure.executable_path);
            failures.record_control_error(
                &format!("Restore {}", failure.property),
                failure.process_id,
                &failure.process_name,
                failure.error,
                action_log,
            );
        }
    }

    fn is_executable_path_suppressed(
        &mut self,
        process_id: u32,
        process_name: &str,
        executable_path: &str,
        action_log: &mut ActionLog,
        auto_excluded_processes: &mut BTreeSet<String>,
    ) -> bool {
        let suppression = self
            .failure_suppression
            .process_suppression(executable_path);
        if !suppression.suppressed {
            return false;
        }

        if suppression.newly_suppressed {
            auto_excluded_processes.insert(executable_path.to_owned());
            action_log.record(
                ActionLogFeature::CpuScheduler,
                Some(process_id),
                process_name.trim().to_owned(),
                ActionLogResult::Skipped,
                format!(
                    "Stopped retrying CPU Scheduler after {} failed attempts.",
                    execution_failure_suppression_threshold(),
                ),
            );
        }

        true
    }

    #[cfg(test)]
    fn is_process_suppressed(
        &mut self,
        process_id: u32,
        process_name: &str,
        action_log: &mut ActionLog,
        auto_excluded_processes: &mut BTreeSet<String>,
    ) -> bool {
        self.is_executable_path_suppressed(
            process_id,
            process_name,
            process_name,
            action_log,
            auto_excluded_processes,
        )
    }

    fn record_process_failure(&mut self, process_name: &str) {
        self.failure_suppression
            .record_process_failure(process_name);
    }

    fn clear_process_failure(&mut self, process_name: &str) {
        self.failure_suppression.clear_process_failure(process_name);
    }

    fn update_background_pressure(
        &mut self,
        settings: &CpuSchedulerSettings,
        foreground_cpu_usage_percent: Option<f32>,
        total_cpu_usage_percent: Option<f32>,
    ) -> bool {
        if cpu_pressure_restraint_should_run(
            settings,
            foreground_cpu_usage_percent,
            total_cpu_usage_percent,
        ) {
            self.background_pressure_active = true;
            return true;
        }

        if self.background_pressure_active
            && foreground_cpu_usage_percent.is_none()
            && total_cpu_usage_percent.is_none()
        {
            return true;
        }

        if self.background_pressure_active
            && cpu_pressure_above_recovery_threshold(
                settings,
                foreground_cpu_usage_percent,
                total_cpu_usage_percent,
            )
        {
            return true;
        }

        self.background_pressure_active = false;
        false
    }

    fn foreground_boost_stable(
        &mut self,
        process_id: u32,
        process_name: &str,
        executable_path: &str,
        creation_time: u64,
        stability_delay_ms: u64,
    ) -> bool {
        let now = Instant::now();
        if focus_and_launch_profile_eligible(process_id) {
            self.focus_process_candidate = Some(FocusProcessCandidate {
                process_id,
                process_name: process_name.to_owned(),
                executable_path: executable_path.to_owned(),
                creation_time,
                first_seen: now,
            });
            return true;
        }

        match &mut self.focus_process_candidate {
            Some(candidate)
                if candidate.process_id == process_id
                    && same_process_name(&candidate.process_name, process_name)
                    && candidate.creation_time == creation_time
                    && same_executable_path(
                        Path::new(&candidate.executable_path),
                        Path::new(executable_path),
                    ) =>
            {
                now.duration_since(candidate.first_seen).as_millis()
                    >= u128::from(stability_delay_ms)
            }
            _ => {
                self.focus_process_candidate = Some(FocusProcessCandidate {
                    process_id,
                    process_name: process_name.to_owned(),
                    executable_path: executable_path.to_owned(),
                    creation_time,
                    first_seen: now,
                });
                false
            }
        }
    }

    fn apply_focus_process_priority_group(
        &mut self,
        group: FocusProcessPriorityGroup<'_>,
        priority_efficiency_controller: &mut PriorityEfficiencyController,
        allow_cross_session_process_control: bool,
        action_log: &mut ActionLog,
    ) -> FocusProcessPriorityGroupResult {
        let FocusProcessPriorityGroup {
            foreground_id,
            foreground_process_name,
            targets,
            stability_delay_ms,
            priority,
            preservation,
        } = group;
        let mut result = FocusProcessPriorityGroupResult::default();
        let foreground_name = foreground_process_name.unwrap_or("").trim();
        if foreground_name.is_empty() || targets.is_empty() {
            if let Some(error) = self.clear_focus_process_priority(
                priority_efficiency_controller,
                true,
                action_log,
                "foreground process is not eligible",
            ) {
                result.failures.merge(error);
            }
            return result;
        }

        let Some((foreground_path, foreground_creation_time)) =
            targets.iter().find_map(|(id, _, path, creation_time)| {
                (*id == foreground_id).then_some((path.as_str(), *creation_time))
            })
        else {
            if let Some(error) = self.clear_focus_process_priority(
                priority_efficiency_controller,
                true,
                action_log,
                "foreground process identity unavailable",
            ) {
                result.failures.merge(error);
            }
            return result;
        };
        if !self.foreground_boost_stable(
            foreground_id,
            foreground_name,
            foreground_path,
            foreground_creation_time,
            stability_delay_ms,
        ) {
            if let Some(error) = self.clear_focus_process_priority(
                priority_efficiency_controller,
                false,
                action_log,
                "foreground app changed before stability delay",
            ) {
                result.failures.merge(error);
            }
            return result;
        }

        let active_targets = targets
            .iter()
            .map(
                |(process_id, process_name, executable_path, creation_time)| {
                    ProcessControlTarget::automatic(
                        *process_id,
                        process_name.clone(),
                        PathBuf::from(executable_path),
                        *creation_time,
                    )
                    .key()
                },
            )
            .collect::<BTreeSet<_>>();
        self.merge_controller_release(
            priority_efficiency_controller.release_priority_policy_except(
                ControlOwner::CpuSchedulerFocusPriority,
                &active_targets,
            ),
            action_log,
            "foreground focus changed",
            "focus process priority",
            &mut result.failures,
        );

        let mut auto_excluded_processes = BTreeSet::new();
        for (process_id, process_name, executable_path, creation_time) in targets {
            if self.is_executable_path_suppressed(
                *process_id,
                process_name,
                executable_path,
                action_log,
                &mut auto_excluded_processes,
            ) {
                result.skipped += 1;
                continue;
            }
            match priority_efficiency_controller.apply_priority_claim(
                PriorityClassClaim {
                    target: ProcessControlTarget::automatic(
                        *process_id,
                        process_name.clone(),
                        PathBuf::from(executable_path),
                        *creation_time,
                    ),
                    owner: ControlOwner::CpuSchedulerFocusPriority,
                    priority,
                    preservation,
                },
                allow_cross_session_process_control,
            ) {
                Ok(ProcessPropertyApplyOutcome::Applied) => {
                    self.clear_process_failure(executable_path);
                    action_log.record(
                        ActionLogFeature::CpuScheduler,
                        Some(*process_id),
                        process_name.clone(),
                        ActionLogResult::Applied,
                        format!("Set focus process priority to {}.", priority.label()),
                    );
                }
                Ok(ProcessPropertyApplyOutcome::Preserved) => {
                    self.clear_process_failure(executable_path);
                    result.skipped += 1;
                }
                Ok(
                    ProcessPropertyApplyOutcome::Unchanged | ProcessPropertyApplyOutcome::Shadowed,
                ) => self.clear_process_failure(executable_path),
                Err(ProcessControlError::ProcessExited) => result.skipped += 1,
                Err(ProcessControlError::AccessDenied(error)) => {
                    result.skipped += 1;
                    self.failure_suppression
                        .suppress_process_failure(executable_path);
                    action_log.record(
                        ActionLogFeature::CpuScheduler,
                        Some(*process_id),
                        process_name.clone(),
                        ActionLogResult::Skipped,
                        error,
                    );
                }
                Err(error) => {
                    self.record_process_failure(executable_path);
                    result.failures.record_control_error(
                        "Set Focus Process Priority",
                        *process_id,
                        process_name,
                        error,
                        action_log,
                    );
                }
            }
        }

        result.auto_excluded_processes = auto_excluded_processes.into_iter().collect();
        result
    }

    fn update_cpu_scheduler_process(
        &mut self,
        process_id: u32,
        process_name: &str,
        executable_path: &str,
        settings: &CpuSchedulerSettings,
        tier: CpuSchedulerTier,
        now: Instant,
    ) -> Option<CpuSchedulerCandidate> {
        let threshold = f32::from(settings.background_app_cpu_threshold_percent.min(100));
        let recovery_threshold = f32::from(
            settings
                .cpu_recovery_threshold_percent
                .min(settings.background_app_cpu_threshold_percent)
                .min(100),
        );
        let cpu_restraint_time = Duration::from_secs(settings.cpu_restraint_time_seconds);
        let cpu_recovery_time = Duration::from_secs(settings.cpu_recovery_time_seconds);
        let (current, creation_time) =
            process_cpu_sample_with_identity(process_id, executable_path)?;
        if self
            .tracked_processes
            .get(&process_id)
            .is_some_and(|process| {
                process.creation_time != creation_time
                    || !same_executable_path(
                        Path::new(&process.executable_path),
                        Path::new(executable_path),
                    )
            })
        {
            self.tracked_processes.remove(&process_id);
        }
        let state =
            self.tracked_processes
                .entry(process_id)
                .or_insert_with(|| CpuSchedulerProcess {
                    process_name: process_name.to_owned(),
                    executable_path: executable_path.to_owned(),
                    creation_time,
                    previous_cpu_time: None,
                    last_usage_tenths: None,
                    high_since: None,
                    below_since: None,
                    active_since: None,
                    decision: None,
                    active: false,
                    selected: false,
                });
        state.process_name = process_name.to_owned();
        state.executable_path = executable_path.to_owned();
        state.creation_time = creation_time;

        let usage = state
            .previous_cpu_time
            .and_then(|previous| process_cpu_demand_percent(previous, current));
        state.previous_cpu_time = Some(current);

        let usage = usage?;
        state.last_usage_tenths = Some(percent_tenths(usage));
        if usage >= threshold {
            state.below_since = None;
            state.high_since.get_or_insert(now);
            if !state.active {
                state.active = true;
                state.active_since = Some(now);
            }
            let decision = cpu_scheduler_process_decision(settings, tier);
            state.decision = Some(decision);
            return Some(cpu_scheduler_candidate(process_id, state, decision, tier));
        }

        if !state.active {
            state.high_since = None;
            state.below_since = None;
            return None;
        }
        if state.active && !state.selected {
            state.active = false;
            state.high_since = None;
            state.below_since = None;
            state.active_since = None;
            state.decision = None;
            return None;
        }

        let active_since = state.active_since.unwrap_or(now);
        if usage > recovery_threshold || now.duration_since(active_since) < cpu_restraint_time {
            state.below_since = None;
            let decision = cpu_scheduler_process_decision(settings, tier);
            state.decision = Some(decision);
            return Some(cpu_scheduler_candidate(process_id, state, decision, tier));
        }

        let below_since = *state.below_since.get_or_insert(now);
        if now.duration_since(below_since) < cpu_recovery_time {
            state.decision = Some(CpuSchedulerDecision::LowerPriority);
            return Some(cpu_scheduler_candidate(
                process_id,
                state,
                CpuSchedulerDecision::LowerPriority,
                tier,
            ));
        }
        state.active = false;
        state.selected = false;
        state.high_since = None;
        state.below_since = None;
        state.active_since = None;
        state.decision = None;
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

    fn cpu_allocation_mask(
        &mut self,
        settings: &CpuSchedulerSettings,
        percent: u8,
        now: Instant,
    ) -> Option<u64> {
        match settings.background_processor_selection {
            BackgroundProcessorSelection::LeastUsed => {
                return self.load_aware_cpu_allocation_mask(percent, None, now);
            }
            BackgroundProcessorSelection::LeastUsedPerformanceCores => {
                return self.load_aware_cpu_allocation_mask(
                    percent,
                    Some(LogicalProcessorKind::Performance),
                    now,
                );
            }
            BackgroundProcessorSelection::LeastUsedEfficiencyCores => {
                return self.load_aware_cpu_allocation_mask(
                    percent,
                    Some(LogicalProcessorKind::Efficiency),
                    now,
                );
            }
            _ => {}
        }

        let processors = cpu_allocation::logical_processors();
        self.cpu_allocation_selection = None;
        selected_background_processor_mask(
            &processors,
            settings.background_processor_selection,
            &settings.specific_processors,
        )
    }

    fn load_aware_cpu_allocation_mask(
        &mut self,
        percent: u8,
        kind: Option<LogicalProcessorKind>,
        now: Instant,
    ) -> Option<u64> {
        let processors = cpu_allocation::logical_processors();
        let usages = self.per_processor_usage.sample()?;
        let next_mask = load_aware_limited_core_mask(&processors, &usages, percent, kind)?;
        if next_mask == cpu_allocation::logical_processor_mask(&processors) {
            self.cpu_allocation_selection = None;
            return None;
        }

        let mask = if let Some(previous) = self.cpu_allocation_selection {
            let previous_count = previous.mask.count_ones();
            let next_count = next_mask.count_ones();
            let elapsed = now.duration_since(previous.selected_at);
            let previous_load = average_masked_core_load(previous.mask, &usages);
            let next_load = average_masked_core_load(next_mask, &usages);
            if previous.kind == kind
                && previous_count == next_count
                && elapsed < Duration::from_secs(CPU_SCHEDULER_CORE_REBALANCE_INTERVAL_SECS)
                && previous_load
                    .zip(next_load)
                    .is_none_or(|(previous_load, next_load)| {
                        previous_load - next_load < CPU_SCHEDULER_CORE_REBALANCE_IMPROVEMENT_PERCENT
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
            .cpu_allocation_selection
            .is_none_or(|selection| selection.mask != mask || selection.kind != kind)
        {
            self.cpu_allocation_selection = Some(CpuAllocationSelection {
                mask,
                kind,
                selected_at: now,
            });
        }
        Some(mask)
    }
}

fn cached_executable_path(
    process: &ProcessInfo,
    executable_paths: &mut BTreeMap<u32, String>,
) -> Option<String> {
    if let Some(path) = executable_paths.get(&process.id) {
        return Some(path.clone());
    }

    let path = process_executable_path(process)?
        .to_string_lossy()
        .into_owned();
    executable_paths.insert(process.id, path.clone());
    Some(path)
}

fn cpu_scheduler_priority_value(priority: ProcessPrioritySetting) -> Option<PriorityClassValue> {
    PriorityClassValue::from_setting(priority)
}

fn cpu_scheduler_priority_target_key(process_id: u32, target: &PriorityTarget) -> ProcessTargetKey {
    ProcessControlTarget::automatic(
        process_id,
        target.process_name.clone(),
        PathBuf::from(&target.executable_path),
        target.creation_time,
    )
    .key()
}

impl Default for CpuSchedulerSnapshot {
    fn default() -> Self {
        Self {
            enabled: false,
            scanned_processes: 0,
            adjusted_processes: 0,
            focus_and_launch_profile_active: false,
            cpu_pressure_restraint_active: false,
            foreground_cpu_usage_tenths: None,
            skipped_processes: 0,
            failed_processes: 0,
            adjusted_apps: Vec::new(),
            auto_excluded_processes: Vec::new(),
            message: "CPU Scheduler disabled.".to_owned(),
            last_error: None,
        }
    }
}

#[cfg(test)]
mod tests;
