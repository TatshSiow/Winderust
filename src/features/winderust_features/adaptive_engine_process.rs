use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use windows_sys::Win32::System::Threading::GetCurrentProcessId;

use crate::{
    action_log::{ActionLog, ActionLogFeature, ActionLogResult},
    audio_activity::active_audio_process_ids,
    config::{AdaptiveEngineProcessSettings, BackgroundProcessorSelection, CpuAllocationMethod},
    control::{
        cpu_allocation::CpuAllocationCoordinator,
        priority_efficiency::{
            PriorityClassClaim, PriorityClassPreservation, PriorityClassValue,
            PriorityEfficiencyController, PriorityEfficiencyReleaseSummary,
            ProcessPropertyApplyOutcome,
        },
        process::{ControlOwner, ProcessControlError, ProcessControlTarget},
    },
    cpu::{process_cpu_demand_percent, PerProcessorUsageMonitor, ProcessCpuSample},
    cpu_allocation::{
        self, CpuAllocationManager, CpuAllocationMode, CpuAllocationTarget, LogicalProcessorInfo,
        LogicalProcessorKind,
    },
    cpu_topology::{CpuSelection, CpuTopology},
    foreground::{
        contains_process_name, process_count_label, process_executable_path, process_failure_key,
        process_session_id, same_executable_path, ProcessInfo, ProtectedProcesses,
        EXTENDED_BUILT_IN_PROCESS_EXCLUSIONS,
    },
    rules::{execution_failure_suppression_threshold, ExecutionFailureTracker},
    runtime::observations::{
        foreground_process_group_ids, CycleObservations, ForegroundCpuHistory,
    },
};

mod policy;
mod process_control;

pub use policy::is_builtin_excluded;
pub(crate) use policy::{focus_and_launch_profile_eligible, focus_process_priority_eligible};

use policy::*;
use process_control::*;

const BUILT_IN_EXCLUSIONS: &[&str] = EXTENDED_BUILT_IN_PROCESS_EXCLUSIONS;
const ADAPTIVE_ENGINE_PROCESS_RECOVERY_BAND_PERCENT: u8 = 5;
const ADAPTIVE_ENGINE_PROCESS_CORE_REBALANCE_INTERVAL_SECS: u64 = 3;
const ADAPTIVE_ENGINE_PROCESS_CORE_REBALANCE_IMPROVEMENT_PERCENT: f32 = 15.0;
const ADAPTIVE_ENGINE_PROCESS_SELECTION_STICKINESS_TENTHS: u32 = 50;
const FOCUS_AND_LAUNCH_PROFILE_WINDOW: Duration = Duration::from_secs(8);
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AdaptiveEngineProcessSnapshot {
    pub zones: DynamicResourceZoneSnapshot,
    pub enabled: bool,
    pub observation_paused: bool,
    pub scanned_processes: usize,
    pub adjusted_processes: usize,
    pub cpu_pressure_restraint_active: bool,
    pub foreground_cpu_usage_tenths: Option<u16>,
    pub skipped_processes: usize,
    pub failed_processes: usize,
    pub auto_excluded_processes: Vec<String>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum ZoneStatus {
    #[default]
    Disabled,
    Waiting,
    Applying,
    Active,
    Overridden,
    Degraded,
    Unavailable,
}
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DynamicResourceZoneSnapshot {
    pub status: ZoneStatus,
    pub reason: &'static str,
    pub foreground_targets: usize,
    pub background_targets: usize,
    pub foreground_processors: u32,
    pub background_processors: u32,
    pub background_limit_targets: usize,
}

pub struct AdaptiveEngineProcessManager {
    foreground_cpu_history: ForegroundCpuHistory,
    tracked_processes: BTreeMap<u32, AdaptiveEngineProcessProcess>,
    background_pressure_active: bool,
    cpu_allocation: CpuAllocationManager,
    cpu_allocation_selection: [Option<CpuAllocationSelection>; 2],
    allocation_settings: Option<AdaptiveEngineProcessSettings>,
    allocation_foreground: Vec<(u32, u64)>,
    per_processor_usage: PerProcessorUsageMonitor,
    failure_suppression: ExecutionFailureTracker,
}

impl Default for AdaptiveEngineProcessManager {
    fn default() -> Self {
        Self {
            foreground_cpu_history: ForegroundCpuHistory::default(),
            tracked_processes: BTreeMap::new(),
            background_pressure_active: false,
            cpu_allocation: CpuAllocationManager::with_action_log_feature(
                ActionLogFeature::AdaptiveEngine,
            ),
            cpu_allocation_selection: [None; 2],
            allocation_settings: None,
            allocation_foreground: Vec::new(),
            per_processor_usage: PerProcessorUsageMonitor::default(),
            failure_suppression: ExecutionFailureTracker::default(),
        }
    }
}

#[derive(Clone)]
struct AdaptiveEngineProcessProcess {
    process_name: String,
    executable_path: String,
    creation_time: u64,
    tier: AdaptiveEngineProcessTier,
    previous_cpu_time: Option<ProcessCpuSample>,
    last_usage_tenths: Option<u16>,
    pressure_restraint: PressureRestraintEpisode,
    below_since: Option<Instant>,
    active_since: Option<Instant>,
    selected: bool,
}

impl AdaptiveEngineProcessProcess {
    fn clear_restraint(&mut self) {
        self.below_since = None;
        self.active_since = None;
        self.selected = false;
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AdaptiveEngineProcessDecision {
    LowerPriority,
    LimitProcessors,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum AdaptiveEngineProcessTier {
    VisibleWindow,
    Background,
}

#[derive(Clone)]
struct AdaptiveEngineProcessCandidate {
    process_id: u32,
    process_name: String,
    decision: AdaptiveEngineProcessDecision,
    tier: AdaptiveEngineProcessTier,
    score: u32,
}

#[derive(Clone, Copy)]
struct CpuAllocationSelection {
    mask: u64,
    kind: Option<LogicalProcessorKind>,
    percent: u8,
    domain: u64,
    selected_at: Instant,
}

#[derive(Clone, Default)]
struct PressureRestraintEpisode {
    hot_since: Option<Instant>,
    selected_since: Option<Instant>,
    below_since: Option<Instant>,
}

impl PressureRestraintEpisode {
    fn qualifies(
        &mut self,
        settings: &AdaptiveEngineProcessSettings,
        now: Instant,
        usage_tenths: u16,
    ) -> bool {
        let threshold = u16::from(settings.background_app_cpu_threshold_percent.min(100)) * 10;
        if usage_tenths >= threshold {
            self.below_since = None;
            let hot_since = *self.hot_since.get_or_insert(now);
            return self.selected_since.is_some()
                || (now > hot_since
                    && now.duration_since(hot_since)
                        >= Duration::from_millis(settings.reaction_time_ms));
        }

        self.hot_since = None;
        let Some(selected_since) = self.selected_since else {
            return false;
        };
        let recovery_threshold =
            (u16::from(settings.cpu_recovery_threshold_percent) * 10).min(threshold);
        if usage_tenths > recovery_threshold
            || now.duration_since(selected_since)
                < Duration::from_secs(settings.cpu_restraint_time_seconds)
        {
            self.below_since = None;
            return true;
        }

        let below_since = *self.below_since.get_or_insert(now);
        if now.duration_since(below_since) < Duration::from_secs(settings.cpu_recovery_time_seconds)
        {
            return true;
        }
        *self = Self::default();
        false
    }
}

pub struct AdaptiveEngineProcessUpdate<'a> {
    pub settings: &'a AdaptiveEngineProcessSettings,
    pub automation_enabled: bool,
    pub allow_cross_session_process_control: bool,
    pub foreground_process_id: Option<u32>,
    pub total_cpu_usage_percent: Option<f32>,
    pub excluded_process_ids: &'a BTreeSet<u32>,
    pub explicit_cpu_allocation_paths: &'a [String],
    pub observations: &'a mut CycleObservations,
}

impl AdaptiveEngineProcessManager {
    pub(crate) fn invalidate_allocation_selection(&mut self) {
        self.cpu_allocation_selection = [None; 2];
        self.cpu_allocation.invalidate_zone_generation();
    }

    pub(crate) fn reset_pressure_observations(&mut self) {
        self.foreground_cpu_history = ForegroundCpuHistory::default();
        self.tracked_processes.clear();
        self.background_pressure_active = false;
        self.per_processor_usage = PerProcessorUsageMonitor::default();
    }

    pub fn update(
        &mut self,
        input: AdaptiveEngineProcessUpdate<'_>,
        cpu_allocation_coordinator: &mut CpuAllocationCoordinator,
        priority_efficiency_controller: &mut PriorityEfficiencyController,
        action_log: &mut ActionLog,
    ) -> AdaptiveEngineProcessSnapshot {
        let AdaptiveEngineProcessUpdate {
            settings,
            automation_enabled,
            allow_cross_session_process_control,
            foreground_process_id,
            total_cpu_usage_percent,
            excluded_process_ids,
            explicit_cpu_allocation_paths,
            observations,
        } = input;

        if !automation_enabled {
            let failed = self.clear_all(
                cpu_allocation_coordinator,
                priority_efficiency_controller,
                action_log,
                "automation disabled",
            );
            self.failure_suppression.clear();
            return AdaptiveEngineProcessSnapshot {
                zones: DynamicResourceZoneSnapshot {
                    status: if failed.count > 0 {
                        ZoneStatus::Degraded
                    } else {
                        ZoneStatus::Disabled
                    },
                    reason: if failed.count > 0 { "zone_retry" } else { "" },
                    ..Default::default()
                },
                enabled: false,
                failed_processes: failed.count,
                last_error: failed.last_error,
                ..Default::default()
            };
        }

        if !settings.has_enabled_policies() {
            let failed = self.clear_all(
                cpu_allocation_coordinator,
                priority_efficiency_controller,
                action_log,
                "Adaptive Engine disabled",
            );
            self.failure_suppression.clear();
            return AdaptiveEngineProcessSnapshot {
                zones: DynamicResourceZoneSnapshot {
                    status: if failed.count > 0 {
                        ZoneStatus::Degraded
                    } else {
                        ZoneStatus::Disabled
                    },
                    reason: if failed.count > 0 { "zone_retry" } else { "" },
                    ..Default::default()
                },
                enabled: false,
                failed_processes: failed.count,
                last_error: failed.last_error,
                ..Default::default()
            };
        }

        let Some(foreground_id) = foreground_process_id else {
            return self.pause_for_unknown_focus(
                settings.dynamic_resource_zones_enabled,
                cpu_allocation_coordinator,
                priority_efficiency_controller,
                action_log,
            );
        };

        // SAFETY: GetCurrentProcessId takes no arguments and has no caller requirements.
        let current_process_id = unsafe { GetCurrentProcessId() };
        let Some(current_session_id) = process_session_id(current_process_id) else {
            let failed = self.clear_all(
                cpu_allocation_coordinator,
                priority_efficiency_controller,
                action_log,
                "current Windows session is unknown",
            );
            return AdaptiveEngineProcessSnapshot {
                zones: DynamicResourceZoneSnapshot {
                    status: if settings.dynamic_resource_zones_enabled {
                        ZoneStatus::Unavailable
                    } else {
                        ZoneStatus::Disabled
                    },
                    reason: "zone_observations",
                    ..Default::default()
                },
                enabled: true,
                failed_processes: failed.count,
                observation_paused: true,
                last_error: failed.last_error,
                ..Default::default()
            };
        };

        let exact_paths_required = settings
            .custom_rules
            .iter()
            .any(|rule| rule.enabled && Path::new(rule.executable_path.trim()).is_absolute());
        let read_background_processes = |observations: &mut CycleObservations| {
            if exact_paths_required {
                observations.processes_with_paths()
            } else {
                observations.processes()
            }
        };
        let foreground_processes =
            match if cpu_pressure_restraint_should_run(settings, None, total_cpu_usage_percent) {
                read_background_processes(observations)
            } else {
                observations.foreground_process_ancestry(foreground_id)
            } {
                Ok(processes) => processes,
                Err(error) => {
                    let mut status = self.pause_for_unknown_focus(
                        settings.dynamic_resource_zones_enabled,
                        cpu_allocation_coordinator,
                        priority_efficiency_controller,
                        action_log,
                    );
                    status.last_error.get_or_insert(error);
                    return status;
                }
            };
        let foreground_workload = observations.adaptive_workload(&foreground_processes);
        if foreground_workload.is_empty() {
            return self.pause_for_unknown_focus(
                settings.dynamic_resource_zones_enabled,
                cpu_allocation_coordinator,
                priority_efficiency_controller,
                action_log,
            );
        }
        let foreground_identity = foreground_workload
            .iter()
            .map(|(&id, &creation)| (id, creation))
            .collect::<Vec<_>>();
        let foreground_changed = self.allocation_foreground != foreground_identity;
        let cpu_sample = observations.adaptive_cpu_sample(&foreground_workload);
        let foreground_cpu_usage_percent = self
            .foreground_cpu_history
            .update(foreground_workload, cpu_sample);
        let foreground_cpu_usage_tenths = foreground_cpu_usage_percent.map(percent_tenths);
        let background_pressure_triggered = self.update_background_pressure(
            settings,
            foreground_cpu_usage_percent,
            total_cpu_usage_percent,
        );
        let background_observations = if background_pressure_triggered {
            let background_observations = read_background_processes(observations)
                .map_err(|error| ("process list unavailable", Some(error)))
                .and_then(|processes| {
                    observations
                        .visible_window_process_ids()
                        .map(|windows| (processes, windows))
                        .map_err(|_| ("visible windows are unavailable", None))
                });
            match background_observations {
                Ok(observations) => Some(observations),
                Err((reason, error)) => {
                    return self.pause_for_observation_failure(
                        settings.dynamic_resource_zones_enabled,
                        cpu_allocation_coordinator,
                        priority_efficiency_controller,
                        action_log,
                        reason,
                        error,
                    );
                }
            }
        } else {
            None
        };
        let processes = background_observations
            .as_ref()
            .map_or(&[][..], |(processes, _)| processes.as_ref());
        let scanned_processes = processes.len();
        let foreground_group = if background_pressure_triggered
            && !std::ptr::eq(processes, foreground_processes.as_ref())
        {
            let group = foreground_process_group_ids(processes, foreground_process_id);
            // A new child or reused PID in the full scan invalidates the sampled workload.
            if group.len() != foreground_identity.len()
                || foreground_identity.iter().any(|&(id, creation)| {
                    !group.contains(&id)
                        || !processes.iter().any(|process| {
                            process.id == id && process.creation_time == Some(creation)
                        })
                })
            {
                return self.pause_for_unknown_focus(
                    settings.dynamic_resource_zones_enabled,
                    cpu_allocation_coordinator,
                    priority_efficiency_controller,
                    action_log,
                );
            }
            group
        } else {
            foreground_identity.iter().map(|&(id, _)| id).collect()
        };
        let processes_by_id = processes
            .iter()
            .map(|process| (process.id, process))
            .collect::<BTreeMap<_, _>>();
        let mut executable_paths = BTreeMap::new();
        let visible_window_process_group_ids = background_observations
            .as_ref()
            .map(|(_, windows)| {
                let visible = ProtectedProcesses::capture(processes, false, None, windows.clone());
                processes
                    .iter()
                    .filter(|process| !foreground_group.contains(&process.id))
                    .filter(|process| visible.may_contain(process))
                    .filter_map(|process| {
                        let path = cached_executable_path(process, &mut executable_paths)?;
                        visible
                            .contains(process.id, Path::new(&path))
                            .then_some(process.id)
                    })
                    .collect::<BTreeSet<_>>()
            })
            .unwrap_or_default();
        if self.allocation_settings.as_ref().is_some_and(|previous| {
            previous.cpu_pressure_restraint_enabled != settings.cpu_pressure_restraint_enabled
                || previous.background_app_cpu_threshold_percent
                    != settings.background_app_cpu_threshold_percent
                || previous.foreground_or_system_cpu_threshold_percent
                    != settings.foreground_or_system_cpu_threshold_percent
                || previous.reaction_time_ms != settings.reaction_time_ms
                || previous.cpu_restraint_time_seconds != settings.cpu_restraint_time_seconds
                || previous.cpu_recovery_threshold_percent
                    != settings.cpu_recovery_threshold_percent
                || previous.cpu_recovery_time_seconds != settings.cpu_recovery_time_seconds
        }) {
            for process in self.tracked_processes.values_mut() {
                process.pressure_restraint = PressureRestraintEpisode::default();
            }
        }
        if self.allocation_settings.as_ref() != Some(settings) || foreground_changed {
            self.invalidate_allocation_selection();
            self.allocation_settings = Some(settings.clone());
            self.allocation_foreground = foreground_identity;
        }
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
                    &foreground_group,
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
                AdaptiveEngineProcessTier::VisibleWindow
            } else {
                AdaptiveEngineProcessTier::Background
            };
            restrainable_processes.insert(process.id, (process.name.clone(), tier));
        }

        let focus_and_launch_profile_target = foreground_processes
            .iter()
            .find(|process| process.id == foreground_id)
            .is_some_and(|process| {
                !excluded_process_ids.contains(&foreground_id)
                    && focus_process_priority_eligible(
                        foreground_id,
                        &process.name,
                        current_process_id,
                        current_session_id,
                    )
                    && process.creation_time.is_some_and(|created| {
                        focus_and_launch_profile_eligible(foreground_id, created)
                    })
                    && !cached_executable_path(process, &mut executable_paths)
                        .is_some_and(|path| settings.custom_rule_enabled_for(&path))
            });
        let cpu_pressure_restraint_applies = settings.cpu_pressure_restraint_enabled
            && background_pressure_triggered
            && (foreground_cpu_usage_percent.is_some() || total_cpu_usage_percent.is_some());
        let cpu_allocation_applies = settings.limit_background_processors_enabled
            && background_pressure_triggered
            && !focus_and_launch_profile_target;
        let zone_grace = focus_and_launch_profile_target;
        let zone_pressure = cpu_pressure_restraint_should_run(
            settings,
            foreground_cpu_usage_percent,
            total_cpu_usage_percent,
        );
        let dynamic_resource_zones_apply = settings.dynamic_resource_zones_enabled
            && zone_pressure
            && !zone_grace
            && foreground_cpu_usage_percent.is_some()
            && !foreground_changed;
        let placement_applies = cpu_allocation_applies || dynamic_resource_zones_apply;
        let mut zones = DynamicResourceZoneSnapshot {
            status: if settings.dynamic_resource_zones_enabled {
                ZoneStatus::Waiting
            } else {
                ZoneStatus::Disabled
            },
            reason: if zone_grace {
                "zone_grace"
            } else {
                "zone_waiting"
            },
            ..Default::default()
        };
        let mut zone_background_targets = Vec::new();
        let mut zone_foreground_targets = Vec::new();
        let mut auto_excluded_processes = BTreeSet::new();

        let mut cpu_allocation_targets = Vec::new();
        let mut priority_candidates = Vec::new();
        let topology_observation = (placement_applies
            || cpu_allocation_coordinator.needs_topology_observation())
        .then(|| observations.cpu_topology());
        let topology = topology_observation
            .as_ref()
            .map(|value| value.as_deref().map_err(String::as_str));
        let mut allocation_error = None;
        let needs_load = (cpu_allocation_applies
            && settings.background_processor_selection.uses_percentage())
            || (dynamic_resource_zones_apply
                && settings
                    .dynamic_resource_zone_settings
                    .background_processor_selection
                    .uses_percentage());
        if !needs_load {
            self.per_processor_usage = PerProcessorUsageMonitor::default();
        }
        let audio_process_ids = cpu_pressure_restraint_applies
            .then(|| {
                active_audio_process_ids().map_err(|error| {
                    failures.last_error = Some(format!(
                        "CPU Pressure Restraint is waiting for audio activity: {error}"
                    ));
                })
            })
            .and_then(Result::ok);
        if audio_process_ids.is_some() || placement_applies {
            let now = Instant::now();
            let processors = topology
                .and_then(Result::ok)
                .map(cpu_allocation::logical_processors_for_group_zero)
                .unwrap_or_default();
            let zone_domain_available = topology.and_then(Result::ok).is_some_and(|topology| {
                topology.groups().len() == 1
                    && cpu_allocation::zone_domain_supported(1, &processors)
            });
            let limiter_error = topology
                .filter(|_| cpu_allocation_applies)
                .and_then(|topology| {
                    saved_processor_selection_error(
                        settings.background_processor_selection,
                        &settings.specific_processors,
                        topology,
                    )
                });
            let zone_error =
                topology
                    .filter(|_| dynamic_resource_zones_apply)
                    .and_then(|topology| {
                        saved_processor_selection_error(
                            settings
                                .dynamic_resource_zone_settings
                                .background_processor_selection,
                            &settings.dynamic_resource_zone_settings.specific_processors,
                            topology,
                        )
                    });
            allocation_error = limiter_error.clone().or_else(|| zone_error.clone());
            let usages = needs_load
                .then(|| self.per_processor_usage.sample())
                .flatten();
            let cpu_allocation_mask = (cpu_allocation_applies && limiter_error.is_none())
                .then(|| {
                    self.cpu_allocation_mask(
                        settings.background_processor_selection,
                        &settings.specific_processors,
                        topology?.ok()?,
                        settings.processor_limit_percent,
                        false,
                        &processors,
                        usages.as_deref(),
                        now,
                    )
                })
                .flatten();
            let zone_masks = if dynamic_resource_zones_apply {
                if settings
                    .dynamic_resource_zone_settings
                    .validate(true)
                    .is_err()
                {
                    None
                } else if zone_domain_available && zone_error.is_none() {
                    let config = &settings.dynamic_resource_zone_settings;
                    let load_complete = !config.background_processor_selection.uses_percentage()
                        || usages.as_ref().is_some_and(|loads| {
                            processors.iter().all(|processor| {
                                loads
                                    .get(processor.index)
                                    .is_some_and(|load| load.is_finite())
                            })
                        });
                    let background = load_complete
                        .then(|| {
                            self.cpu_allocation_mask(
                                config.background_processor_selection,
                                &config.specific_processors,
                                topology?.ok()?,
                                dynamic_background_zone_percent(config.foreground_share_percent),
                                true,
                                &processors,
                                usages.as_deref(),
                                now,
                            )
                        })
                        .flatten();
                    background.and_then(|mask| {
                        dynamic_resource_zone_masks(
                            cpu_allocation::logical_processor_mask(&processors),
                            mask,
                        )
                    })
                } else {
                    None
                }
            } else {
                None
            };
            let cpu_allocation_selection = cpu_allocation_mask
                .and_then(|mask| CpuSelection::from_group_zero_mask(topology?.ok()?, mask).ok());
            let zone_selections = zone_masks.and_then(|(foreground, background)| {
                let topology = topology?.ok()?;
                Some((
                    CpuSelection::from_group_zero_mask(topology, foreground).ok()?,
                    CpuSelection::from_group_zero_mask(topology, background).ok()?,
                ))
            });
            if dynamic_resource_zones_apply && zone_masks.is_none() {
                zones.status = ZoneStatus::Unavailable;
                zones.reason = "zone_unavailable";
            }
            let mut actionable_process_ids = BTreeSet::new();
            let mut adaptive_engine_process_candidates = Vec::new();
            for (process_id, (process_name, tier)) in &restrainable_processes {
                if *tier != AdaptiveEngineProcessTier::Background {
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
                actionable_process_ids.insert(*process_id);
                let sample = process_cpu_sample_with_identity(*process_id, &executable_path)
                    .filter(|(_, creation_time)| Some(*creation_time) == process.creation_time);
                if let Some(candidate) = self
                    .update_adaptive_engine_process_sample(
                        *process_id,
                        process_name,
                        &executable_path,
                        settings,
                        *tier,
                        sample,
                    )
                    .filter(|_| placement_applies)
                {
                    adaptive_engine_process_candidates.push(candidate);
                }
                let Some(state) = self.tracked_processes.get_mut(process_id) else {
                    continue;
                };
                let pressure_sample = sample.zip(state.last_usage_tenths).filter(|_| {
                    audio_process_ids
                        .as_ref()
                        .is_some_and(|playing| !playing.contains(process_id))
                });
                let Some(((sample, creation_time), usage)) = pressure_sample else {
                    state.pressure_restraint = PressureRestraintEpisode::default();
                    continue;
                };
                if state
                    .pressure_restraint
                    .qualifies(settings, sample.sampled_at, usage)
                {
                    let selected = state.pressure_restraint.selected_since.is_some();
                    let score = u32::from(usage)
                        + u32::from(selected) * ADAPTIVE_ENGINE_PROCESS_SELECTION_STICKINESS_TENTHS;
                    priority_candidates.push((
                        score,
                        sample.sampled_at,
                        ProcessControlTarget::automatic(
                            *process_id,
                            process_name.clone(),
                            PathBuf::from(&executable_path),
                            creation_time,
                        ),
                    ));
                }
            }
            self.tracked_processes
                .retain(|process_id, _| actionable_process_ids.contains(process_id));

            let selected_candidates = select_adaptive_engine_process_candidates(
                adaptive_engine_process_candidates,
                settings.maximum_restrained_apps,
            );
            let selected_ids = selected_candidates
                .iter()
                .map(|candidate| candidate.process_id)
                .collect::<BTreeSet<_>>();
            for (process_id, process) in &mut self.tracked_processes {
                if process.active_since.is_some() && !selected_ids.contains(process_id) {
                    process.selected = false;
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
                }
                if candidate.tier == AdaptiveEngineProcessTier::Background
                    && !cpu_allocation::contains_process(
                        explicit_cpu_allocation_paths,
                        &executable_path,
                    )
                {
                    if cpu_allocation_applies
                        && candidate.decision == AdaptiveEngineProcessDecision::LimitProcessors
                    {
                        if let Some(selection) = &cpu_allocation_selection {
                            cpu_allocation_targets.push(CpuAllocationTarget {
                                process_id: candidate.process_id,
                                process_name: candidate.process_name.clone(),
                                executable_path: executable_path.clone(),
                                mode: cpu_allocation_method(settings),
                                selection: selection.clone(),
                                creation_time,
                            });
                        }
                    }
                    let hot = self
                        .tracked_processes
                        .get(&candidate.process_id)
                        .is_some_and(|state| {
                            state.creation_time == creation_time
                                && state.last_usage_tenths.is_some_and(|usage| {
                                    fresh_background_competition(
                                        Some(usage),
                                        settings.background_app_cpu_threshold_percent,
                                    )
                                })
                        });
                    if hot {
                        if let Some((foreground, background)) = &zone_selections {
                            zones.foreground_processors = foreground.processors().len() as u32;
                            zones.background_processors = background.processors().len() as u32;
                            zone_background_targets.push(CpuAllocationTarget {
                                process_id: candidate.process_id,
                                process_name: candidate.process_name.clone(),
                                executable_path,
                                mode: CpuAllocationMode::SoftCpuSets,
                                selection: background.clone(),
                                creation_time,
                            });
                        }
                    }
                }
            }
            if let Some((foreground_selection, _)) = zone_selections
                .as_ref()
                .filter(|_| !zone_background_targets.is_empty())
            {
                for process_id in &foreground_group {
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
                    zone_foreground_targets.push(CpuAllocationTarget {
                        process_id: *process_id,
                        process_name: process.name.clone(),
                        executable_path,
                        mode: CpuAllocationMode::SoftCpuSets,
                        selection: foreground_selection.clone(),
                        creation_time,
                    });
                }
            }
            if !zone_foreground_targets
                .iter()
                .any(|target| Some(target.process_id) == foreground_process_id)
            {
                zone_foreground_targets.clear();
            }
        } else {
            self.tracked_processes.clear();
            self.cpu_allocation_selection = [None; 2];
        }

        let attempted_zones =
            !zone_background_targets.is_empty() && !zone_foreground_targets.is_empty();
        if dynamic_resource_zones_apply
            && !zone_background_targets.is_empty()
            && zone_foreground_targets.is_empty()
            && zones.status != ZoneStatus::Unavailable
        {
            zones.status = ZoneStatus::Overridden;
            zones.reason = "zone_foreground_unavailable";
        }
        if attempted_zones {
            zones.status = ZoneStatus::Applying;
        }
        let allocation = if let Some(topology) = topology {
            self.cpu_allocation.reconcile_targets(
                cpu_allocation_coordinator,
                topology,
                ControlOwner::AdaptiveEngine,
                cpu_allocation_targets,
                attempted_zones.then_some((zone_background_targets, zone_foreground_targets)),
                scanned_processes,
                allow_cross_session_process_control,
                action_log,
            )
        } else {
            self.cpu_allocation.reconcile_empty_generation(
                cpu_allocation_coordinator,
                ControlOwner::AdaptiveEngine,
                scanned_processes,
                action_log,
            )
        };
        let mut cpu_allocation_snapshot = allocation.snapshot;
        if let Some(error) = allocation_error {
            cpu_allocation_snapshot.last_error.get_or_insert(error);
        }
        if let Some((foreground, background)) = allocation.zone_counts {
            zones.status = ZoneStatus::Active;
            zones.reason = "zone_active";
            zones.foreground_targets = foreground;
            zones.background_targets = background;
        } else {
            zones.foreground_processors = 0;
            zones.background_processors = 0;
            zones.background_limit_targets = allocation.fallback_count;
            if attempted_zones {
                use crate::cpu_allocation::ZoneAllocationFailure;
                (zones.status, zones.reason) = match allocation.zone_failure {
                    ZoneAllocationFailure::Waiting => (ZoneStatus::Waiting, "zone_waiting"),
                    ZoneAllocationFailure::Overridden => {
                        (ZoneStatus::Overridden, "zone_foreground_unavailable")
                    }
                    ZoneAllocationFailure::Unavailable => {
                        (ZoneStatus::Unavailable, "zone_unavailable")
                    }
                    ZoneAllocationFailure::Degraded => (ZoneStatus::Degraded, "zone_retry"),
                };
            }
        }
        if cpu_allocation_snapshot.failed_processes > 0 && settings.dynamic_resource_zones_enabled {
            zones.status = ZoneStatus::Degraded;
            zones.reason = "zone_retry";
        }
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

        priority_candidates.sort_unstable_by_key(|(score, _, target)| (Reverse(*score), target.id));
        let qualified_episodes = priority_candidates
            .iter()
            .map(|(_, _, target)| target.key())
            .collect::<BTreeSet<_>>();
        self.failure_suppression.retain_keys(
            &priority_candidates
                .iter()
                .map(|(_, _, target)| {
                    process_failure_key(&target.executable_path.to_string_lossy())
                })
                .collect(),
        );
        let mut skipped_processes = 0;
        let mut selected_candidates = Vec::new();
        let target_limit = usize::from(settings.maximum_restrained_apps.max(1));
        for (_, sampled_at, target) in priority_candidates {
            if selected_candidates.len() == target_limit {
                break;
            }
            let executable_path = target.executable_path.to_string_lossy();
            if self.is_executable_path_suppressed(
                target.id,
                &target.name,
                &executable_path,
                action_log,
                &mut auto_excluded_processes,
            ) {
                skipped_processes += 1;
                continue;
            }
            match priority_efficiency_controller
                .can_apply_adaptive_restraint(&target, allow_cross_session_process_control)
            {
                Ok(true) => selected_candidates.push((sampled_at, target)),
                Ok(false) => self.clear_process_failure(&executable_path),
                Err(error) => {
                    skipped_processes += usize::from(self.record_restraint_failure(
                        &target,
                        error,
                        action_log,
                        &mut failures,
                    ));
                }
            }
        }
        let active_priority_targets = selected_candidates
            .iter()
            .map(|(_, target)| target.key())
            .collect::<BTreeSet<_>>();
        let selected_ids = active_priority_targets
            .iter()
            .map(|key| key.id)
            .collect::<BTreeSet<_>>();
        for (process_id, process) in &mut self.tracked_processes {
            if !selected_ids.contains(process_id) {
                process.pressure_restraint.selected_since = None;
                process.pressure_restraint.below_since = None;
            }
        }
        let release = priority_efficiency_controller
            .reconcile_adaptive_restraint_targets(&active_priority_targets, &qualified_episodes);
        // A process awaiting restoration can still occupy a restraint slot.
        if !release.failures.is_empty() {
            selected_candidates.clear();
        }
        self.merge_controller_release(
            release,
            action_log,
            "process no longer qualifies for CPU Pressure Restraint",
            "process priority",
            &mut failures,
        );
        for (sampled_at, target) in selected_candidates {
            let process_id = target.id;
            let executable_path = target.executable_path.to_string_lossy();
            let result = priority_efficiency_controller.apply_priority_claim(
                PriorityClassClaim {
                    target: target.clone(),
                    owner: ControlOwner::AdaptiveEngine,
                    priority: PriorityClassValue::BelowNormal,
                    preservation: PriorityClassPreservation::OnlyNormal,
                },
                allow_cross_session_process_control,
            );
            if let Some(process) = self.tracked_processes.get_mut(&process_id).filter(|_| {
                matches!(
                    result,
                    Ok(ProcessPropertyApplyOutcome::Applied
                        | ProcessPropertyApplyOutcome::Unchanged)
                )
            }) {
                process
                    .pressure_restraint
                    .selected_since
                    .get_or_insert(sampled_at);
            }
            match result {
                Ok(ProcessPropertyApplyOutcome::Applied) => {
                    self.clear_process_failure(&executable_path);
                    action_log.record(
                        ActionLogFeature::AdaptiveEngine,
                        Some(process_id),
                        target.name,
                        ActionLogResult::Applied,
                        "Lowered process priority to Below Normal during sustained background CPU pressure.",
                    );
                }
                Ok(ProcessPropertyApplyOutcome::Preserved) => {
                    self.clear_process_failure(&executable_path);
                    skipped_processes += 1;
                }
                Ok(
                    ProcessPropertyApplyOutcome::Unchanged | ProcessPropertyApplyOutcome::Shadowed,
                ) => {
                    self.clear_process_failure(&executable_path);
                }
                Err(error) => {
                    skipped_processes += usize::from(self.record_restraint_failure(
                        &target,
                        error,
                        action_log,
                        &mut failures,
                    ));
                }
            }
        }
        AdaptiveEngineProcessSnapshot {
            zones,
            enabled: true,
            observation_paused: false,
            scanned_processes,
            adjusted_processes: 0,
            cpu_pressure_restraint_active: cpu_pressure_restraint_applies,
            foreground_cpu_usage_tenths,
            skipped_processes,
            failed_processes: failures.count,
            auto_excluded_processes: auto_excluded_processes.into_iter().collect(),
            last_error: failures.last_error,
        }
    }

    fn pause_for_unknown_focus(
        &mut self,
        zones_enabled: bool,
        cpu_allocation_coordinator: &mut CpuAllocationCoordinator,
        priority_efficiency_controller: &mut PriorityEfficiencyController,
        action_log: &mut ActionLog,
    ) -> AdaptiveEngineProcessSnapshot {
        self.pause_for_observation_failure(
            zones_enabled,
            cpu_allocation_coordinator,
            priority_efficiency_controller,
            action_log,
            "foreground app is unknown",
            None,
        )
    }

    fn pause_for_observation_failure(
        &mut self,
        zones_enabled: bool,
        cpu_allocation_coordinator: &mut CpuAllocationCoordinator,
        priority_efficiency_controller: &mut PriorityEfficiencyController,
        action_log: &mut ActionLog,
        reason: &str,
        error: Option<String>,
    ) -> AdaptiveEngineProcessSnapshot {
        let failed = self.clear_all(
            cpu_allocation_coordinator,
            priority_efficiency_controller,
            action_log,
            reason,
        );
        let (status, reason) = if !zones_enabled {
            (ZoneStatus::Disabled, "")
        } else if failed.count > 0 {
            (ZoneStatus::Degraded, "zone_retry")
        } else {
            (ZoneStatus::Unavailable, "zone_observations")
        };
        AdaptiveEngineProcessSnapshot {
            zones: DynamicResourceZoneSnapshot {
                status,
                reason,
                ..Default::default()
            },
            enabled: true,
            failed_processes: failed.count,
            observation_paused: true,
            last_error: failed.last_error.or(error),
            ..Default::default()
        }
    }

    fn clear_all(
        &mut self,
        cpu_allocation_coordinator: &mut CpuAllocationCoordinator,
        priority_efficiency_controller: &mut PriorityEfficiencyController,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> PriorityFailures {
        let mut failures = PriorityFailures::default();
        self.merge_controller_release(
            priority_efficiency_controller
                .release_all_priority_policy(ControlOwner::AdaptiveEngine),
            action_log,
            reason,
            "process priority",
            &mut failures,
        );
        self.reset_pressure_observations();
        let cpu_allocation_snapshot = self.cpu_allocation.clear_discovered_targets(
            cpu_allocation_coordinator,
            ControlOwner::AdaptiveEngine,
            reason,
            action_log,
        );
        failures.count += cpu_allocation_snapshot.failed_processes;
        if failures.last_error.is_none() {
            failures.last_error = cpu_allocation_snapshot.last_error;
        }
        failures
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
                ActionLogFeature::AdaptiveEngine,
                None,
                "",
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
                ActionLogFeature::AdaptiveEngine,
                Some(process_id),
                process_name.trim().to_owned(),
                ActionLogResult::Skipped,
                format!(
                    "Stopped retrying Adaptive Engine after {} failed attempts.",
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

    fn record_restraint_failure(
        &mut self,
        target: &ProcessControlTarget,
        error: ProcessControlError,
        action_log: &mut ActionLog,
        failures: &mut PriorityFailures,
    ) -> bool {
        let executable_path = target.executable_path.to_string_lossy();
        match error {
            ProcessControlError::ProcessExited => true,
            ProcessControlError::AccessDenied(error) => {
                self.failure_suppression
                    .suppress_process_failure(&executable_path);
                action_log.record(
                    ActionLogFeature::AdaptiveEngine,
                    Some(target.id),
                    &target.name,
                    ActionLogResult::Skipped,
                    error,
                );
                true
            }
            error => {
                self.record_process_failure(&executable_path);
                failures.record_control_error(
                    "Apply CPU Pressure Restraint",
                    target.id,
                    &target.name,
                    error,
                    action_log,
                );
                false
            }
        }
    }

    fn clear_process_failure(&mut self, process_name: &str) {
        self.failure_suppression.clear_process_failure(process_name);
    }

    fn update_background_pressure(
        &mut self,
        settings: &AdaptiveEngineProcessSettings,
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

    fn update_adaptive_engine_process_sample(
        &mut self,
        process_id: u32,
        process_name: &str,
        executable_path: &str,
        settings: &AdaptiveEngineProcessSettings,
        tier: AdaptiveEngineProcessTier,
        sample: Option<(ProcessCpuSample, u64)>,
    ) -> Option<AdaptiveEngineProcessCandidate> {
        let Some((current, creation_time)) = sample else {
            self.tracked_processes.remove(&process_id);
            return None;
        };
        let now = current.sampled_at;
        let threshold = f32::from(settings.background_app_cpu_threshold_percent.min(100));
        let recovery_threshold = f32::from(
            settings
                .cpu_recovery_threshold_percent
                .min(settings.background_app_cpu_threshold_percent)
                .min(100),
        );
        let cpu_restraint_time = Duration::from_secs(settings.cpu_restraint_time_seconds);
        let cpu_recovery_time = Duration::from_secs(settings.cpu_recovery_time_seconds);
        if self
            .tracked_processes
            .get(&process_id)
            .is_some_and(|process| {
                process.creation_time != creation_time
                    || process.tier != tier
                    || !same_executable_path(
                        Path::new(&process.executable_path),
                        Path::new(executable_path),
                    )
            })
        {
            self.tracked_processes.remove(&process_id);
        }
        let state = self.tracked_processes.entry(process_id).or_insert_with(|| {
            AdaptiveEngineProcessProcess {
                process_name: process_name.to_owned(),
                executable_path: executable_path.to_owned(),
                creation_time,
                tier,
                previous_cpu_time: None,
                last_usage_tenths: None,
                pressure_restraint: PressureRestraintEpisode::default(),
                below_since: None,
                active_since: None,
                selected: false,
            }
        });
        state.process_name = process_name.to_owned();
        state.executable_path = executable_path.to_owned();
        state.creation_time = creation_time;

        let usage = state
            .previous_cpu_time
            .and_then(|previous| process_cpu_demand_percent(previous, current));
        state.previous_cpu_time = Some(current);

        state.last_usage_tenths = None;
        let Some(usage) = usage else {
            state.clear_restraint();
            state.pressure_restraint = PressureRestraintEpisode::default();
            return None;
        };
        state.last_usage_tenths = Some(percent_tenths(usage));
        if usage >= threshold {
            state.below_since = None;
            state.active_since.get_or_insert(now);
            let decision = adaptive_engine_process_process_decision(settings, tier);
            return Some(adaptive_engine_process_candidate(
                process_id, state, decision, tier,
            ));
        }

        let Some(active_since) = state.active_since.filter(|_| state.selected) else {
            state.clear_restraint();
            return None;
        };

        if usage > recovery_threshold || now.duration_since(active_since) < cpu_restraint_time {
            state.below_since = None;
            let decision = adaptive_engine_process_process_decision(settings, tier);
            return Some(adaptive_engine_process_candidate(
                process_id, state, decision, tier,
            ));
        }

        let below_since = *state.below_since.get_or_insert(now);
        if now.duration_since(below_since) < cpu_recovery_time {
            return Some(adaptive_engine_process_candidate(
                process_id,
                state,
                AdaptiveEngineProcessDecision::LowerPriority,
                tier,
            ));
        }
        state.clear_restraint();
        None
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "both policies share one topology and load observation"
    )]
    fn cpu_allocation_mask(
        &mut self,
        selection: BackgroundProcessorSelection,
        specific_processors: &CpuSelection,
        topology: &CpuTopology,
        percent: u8,
        zones: bool,
        processors: &[LogicalProcessorInfo],
        usages: Option<&[f32]>,
        now: Instant,
    ) -> Option<u64> {
        match selection {
            BackgroundProcessorSelection::LeastUsed => {
                return self.load_aware_cpu_allocation_mask(
                    percent, None, zones, processors, usages?, now,
                );
            }
            BackgroundProcessorSelection::LeastUsedPerformanceCores => {
                return self.load_aware_cpu_allocation_mask(
                    percent,
                    Some(LogicalProcessorKind::Performance),
                    zones,
                    processors,
                    usages?,
                    now,
                );
            }
            BackgroundProcessorSelection::LeastUsedEfficiencyCores => {
                return self.load_aware_cpu_allocation_mask(
                    percent,
                    Some(LogicalProcessorKind::Efficiency),
                    zones,
                    processors,
                    usages?,
                    now,
                );
            }
            _ => {}
        }

        self.cpu_allocation_selection[usize::from(zones)] = None;
        selected_background_processor_mask(processors, selection, specific_processors, topology)
    }

    fn load_aware_cpu_allocation_mask(
        &mut self,
        percent: u8,
        kind: Option<LogicalProcessorKind>,
        zones: bool,
        processors: &[LogicalProcessorInfo],
        usages: &[f32],
        now: Instant,
    ) -> Option<u64> {
        let selection = &mut self.cpu_allocation_selection[usize::from(zones)];
        let domain = kind.map_or_else(
            || cpu_allocation::logical_processor_mask(processors),
            |kind| cpu_allocation::logical_processor_kind_mask(processors, kind),
        );
        let next_mask = load_aware_limited_core_mask(processors, usages, percent, kind)?;
        if next_mask == cpu_allocation::logical_processor_mask(processors) {
            *selection = None;
            return None;
        }

        let mask = if let Some(previous) = *selection {
            let previous_count = previous.mask.count_ones();
            let next_count = next_mask.count_ones();
            let elapsed = now.duration_since(previous.selected_at);
            let previous_load = average_masked_core_load(previous.mask, usages);
            let next_load = average_masked_core_load(next_mask, usages);
            if previous.kind == kind
                && previous.percent == percent
                && previous.domain == domain
                && previous_count == next_count
                && elapsed
                    < Duration::from_secs(ADAPTIVE_ENGINE_PROCESS_CORE_REBALANCE_INTERVAL_SECS)
                && previous_load
                    .zip(next_load)
                    .is_none_or(|(previous_load, next_load)| {
                        previous_load - next_load
                            < ADAPTIVE_ENGINE_PROCESS_CORE_REBALANCE_IMPROVEMENT_PERCENT
                    })
            {
                previous.mask
            } else {
                next_mask
            }
        } else {
            next_mask
        };

        if selection.is_none_or(|selection| {
            selection.mask != mask
                || selection.kind != kind
                || selection.percent != percent
                || selection.domain != domain
        }) {
            *selection = Some(CpuAllocationSelection {
                mask,
                kind,
                percent,
                domain,
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
        .into_os_string()
        .into_string()
        .ok()?;
    executable_paths.insert(process.id, path.clone());
    Some(path)
}

#[cfg(test)]
mod tests;
