use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};

use super::{eligible_process, has_affinity_rules, matching_rule, CpuLimiterSnapshot};
use crate::{
    action_log::{ActionLog, ActionLogFeature, ActionLogResult},
    config::{CpuLimiterAffinityLimit, CpuLimiterSettings},
    control::{
        cpu_allocation::{
            CpuAllocationApplyOutcome, CpuAllocationClaim, CpuAllocationCoordinator,
            CpuAllocationRequest, CpuSetInventory,
        },
        cpu_limiter::CpuLimiterController,
        process::{ControlOwner, ProcessControlTarget, ProcessTargetKey},
        suspension::SuspensionTarget,
    },
    cpu::{process_cpu_time_percent, ProcessCpuSample},
    foreground::{
        process_list::verified_process_executable_path, process_session_id,
        sample_process_resources, ProcessResourceSampling,
    },
    platform::windows::process::current_process_id,
    runtime::observations::CycleObservations,
};

const MAX_SAMPLE_GAP: Duration = Duration::from_secs(3);
const MIN_HOLD: Duration = Duration::from_secs(10);
const RELEASE_SUSTAIN: Duration = Duration::from_secs(3);
const COOLDOWN: Duration = Duration::from_secs(20);
const RELEASE_THRESHOLD_FRACTION: f32 = 0.7;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum CpuLimiterAffinityStatus {
    Qualifying,
    CoolingDown,
    Shadowed,
    Unavailable,
    Active(usize),
    RestorationPending,
}

pub(crate) struct AffinityLimiterCycle<'a> {
    pub(crate) settings: &'a CpuLimiterSettings,
    pub(crate) enabled: bool,
    pub(crate) allow_cross_session_process_control: bool,
    pub(crate) duty_targets: Result<&'a BTreeSet<ProcessTargetKey>, &'a str>,
    pub(crate) duty_controller: Option<&'a CpuLimiterController>,
    pub(crate) observations: &'a mut CycleObservations,
}

#[derive(Default)]
pub(crate) struct AffinityLimiter {
    processes: BTreeMap<ProcessTargetKey, ObservedProcess>,
    release_pending: bool,
}

struct ObservedProcess {
    rule: CpuLimiterAffinityLimit,
    previous_sample: Option<ProcessCpuSample>,
    phase: Phase,
}

#[derive(Default)]
enum Phase {
    #[default]
    Qualifying,
    Sustaining(Instant),
    Holding {
        since: Instant,
        low_since: Option<Instant>,
        processors: usize,
    },
    CoolingDown(Instant, CpuLimiterAffinityStatus),
}

enum Decision {
    Wait(CpuLimiterAffinityStatus),
    Claim(Instant),
}

impl ObservedProcess {
    fn new(rule: &CpuLimiterAffinityLimit) -> Self {
        Self {
            rule: rule.clone(),
            previous_sample: None,
            phase: Phase::Qualifying,
        }
    }

    fn observe(&mut self, current: ProcessCpuSample, logical_processors: usize) -> Decision {
        use CpuLimiterAffinityStatus::{CoolingDown, Qualifying};

        let now = current.sampled_at;
        let previous = self.previous_sample.replace(current);
        if let Phase::CoolingDown(until, status) = self.phase {
            if now < until {
                return Decision::Wait(status);
            }
            self.phase = Phase::Qualifying;
        }
        let usage = previous
            .filter(|previous| {
                now > previous.sampled_at
                    && now.duration_since(previous.sampled_at) <= MAX_SAMPLE_GAP
            })
            .and_then(|previous| process_cpu_time_percent(previous, current, logical_processors));
        let Some(usage) = usage else {
            return if matches!(self.phase, Phase::Holding { .. }) {
                self.cool_down(now, CoolingDown)
            } else {
                self.phase = Phase::Qualifying;
                Decision::Wait(Qualifying)
            };
        };
        let trigger = f32::from(self.rule.trigger_percent);
        if let Phase::Holding {
            since,
            ref mut low_since,
            processors,
        } = self.phase
        {
            let max_restricted_usage = processors as f32 * 100.0 / logical_processors as f32;
            let release_threshold = trigger.min(max_restricted_usage) * RELEASE_THRESHOLD_FRACTION;
            if usage < release_threshold {
                let low_since = *low_since.get_or_insert(now);
                if now.duration_since(since) >= MIN_HOLD
                    && now.duration_since(low_since) >= RELEASE_SUSTAIN
                {
                    return self.cool_down(now, CoolingDown);
                }
            } else {
                *low_since = None;
            }
            return Decision::Claim(since);
        }
        if usage < trigger {
            self.phase = Phase::Qualifying;
            return Decision::Wait(Qualifying);
        }
        if let Phase::Sustaining(since) = self.phase {
            if now.duration_since(since) >= Duration::from_secs(self.rule.sustain_seconds) {
                return Decision::Claim(now);
            }
        } else {
            self.phase = Phase::Sustaining(now);
        }
        Decision::Wait(Qualifying)
    }

    fn cool_down(&mut self, now: Instant, status: CpuLimiterAffinityStatus) -> Decision {
        self.phase = Phase::CoolingDown(now + COOLDOWN, status);
        Decision::Wait(status)
    }

    fn confirm(&mut self, since: Instant, processors: usize) {
        if let Phase::Holding {
            processors: previous,
            ..
        } = &mut self.phase
        {
            *previous = processors;
        } else {
            self.phase = Phase::Holding {
                since,
                low_since: None,
                processors,
            };
        }
    }
}

impl AffinityLimiter {
    pub(crate) fn update(
        &mut self,
        cycle: AffinityLimiterCycle<'_>,
        coordinator: &mut CpuAllocationCoordinator,
        log: &mut ActionLog,
        snapshot: &mut CpuLimiterSnapshot,
    ) {
        let mut active = BTreeSet::new();
        let mut statuses = Vec::new();
        let was_observing = !self.processes.is_empty();
        if cycle.enabled && has_affinity_rules(cycle.settings) {
            if let Err(error) = self.evaluate(
                cycle,
                coordinator,
                log,
                snapshot,
                &mut active,
                &mut statuses,
            ) {
                self.processes.clear();
                snapshot.last_error.get_or_insert(error);
            }
        } else {
            self.processes.clear();
        }
        if was_observing
            || !self.processes.is_empty()
            || self.release_pending
            || coordinator
                .policy_managed_processes(ControlOwner::CpuLimiter)
                .next()
                .is_some()
        {
            let release = coordinator.release_policy_except(ControlOwner::CpuLimiter, &active);
            for failure in release.failures {
                let error = failure.error.to_string();
                log.record(
                    ActionLogFeature::CpuLimiter,
                    Some(failure.process_id),
                    failure.process_name,
                    ActionLogResult::Failed,
                    format!("Affinity restoration pending: {error}"),
                );
                snapshot.last_error.get_or_insert(error);
            }
            if !release.restored_owners.is_empty() {
                log.record(
                    ActionLogFeature::CpuLimiter,
                    None,
                    "CPU Limiter",
                    ActionLogResult::Restored,
                    "Released the temporary processor affinity limit.",
                );
            }
        }
        self.release_pending = false;
        for identity in coordinator.policy_managed_processes(ControlOwner::CpuLimiter) {
            if !active.contains(&identity.key()) {
                self.release_pending = true;
                let path = identity.executable_path.to_string_lossy().into_owned();
                statuses.push((path.clone(), CpuLimiterAffinityStatus::RestorationPending));
                snapshot.restore_pending_apps.push(path);
            }
        }
        snapshot.release_pending |= self.release_pending;
        snapshot.limited_processes += active.len();
        statuses.sort_unstable();
        snapshot.active_affinity = statuses;
        snapshot.restore_pending_apps.sort_unstable();
        snapshot.restore_pending_apps.dedup();
        snapshot.failed_apps.sort_unstable();
        snapshot.failed_apps.dedup();
    }

    pub(crate) fn release_pending(&self) -> bool {
        self.release_pending
    }

    fn evaluate(
        &mut self,
        cycle: AffinityLimiterCycle<'_>,
        coordinator: &mut CpuAllocationCoordinator,
        log: &mut ActionLog,
        snapshot: &mut CpuLimiterSnapshot,
        active: &mut BTreeSet<ProcessTargetKey>,
        statuses: &mut Vec<(String, CpuLimiterAffinityStatus)>,
    ) -> Result<(), String> {
        use CpuLimiterAffinityStatus::{Active, Qualifying, Shadowed, Unavailable};

        let duty_targets = cycle.duty_targets.map_err(str::to_owned)?;
        let self_id = current_process_id();
        let session = if cycle.allow_cross_session_process_control {
            None
        } else {
            Some(
                process_session_id(self_id)
                    .ok_or("CPU Limiter could not verify the current session.")?,
            )
        };
        let processes = cycle.observations.processes_with_paths()?;
        snapshot.scanned_processes = snapshot.scanned_processes.max(processes.len());
        let mut candidates = Vec::new();
        let mut sampled_processes = Vec::new();
        for process in processes.iter() {
            let (Some(created), Some(path)) = (
                process.creation_time,
                verified_process_executable_path(process),
            ) else {
                continue;
            };
            let Some(rule) = matching_rule(cycle.settings, &path) else {
                continue;
            };
            let Some(limit) = rule.affinity_limit.as_ref() else {
                continue;
            };
            let target = ProcessControlTarget::automatic(
                process.id,
                process.name.clone(),
                path.into_owned(),
                created,
            );
            if duty_targets.contains(&target.key()) {
                statuses.push((rule.executable_path.clone(), Shadowed));
                snapshot.skipped_processes += 1;
            } else if !limit.is_valid() || !eligible_process(process, self_id, session) {
                statuses.push((rule.executable_path.clone(), Unavailable));
                snapshot.skipped_processes += 1;
            } else {
                sampled_processes.push(process.clone());
                candidates.push((process, target, rule, limit));
            }
        }
        if candidates.is_empty() {
            self.processes.clear();
            return Ok(());
        }
        let topology = cycle.observations.cpu_topology();
        let Ok(topology) = topology else {
            for (_, _, rule, _) in candidates {
                statuses.push((rule.executable_path.clone(), Unavailable));
            }
            return topology.map(|_| ());
        };
        let logical_processors = topology.processors().len();
        let samples =
            sample_process_resources(&sampled_processes, ProcessResourceSampling::CpuOnly);
        let mut previous = std::mem::take(&mut self.processes);
        let mut inventory = CpuSetInventory::default();
        for (process, target, rule, limit) in candidates {
            let key = target.key();
            let mut observed = previous
                .remove(&key)
                .filter(|old| old.rule == *limit)
                .unwrap_or_else(|| ObservedProcess::new(limit));
            let status = match samples
                .get(&process.id)
                .filter(|sample| Some(sample.creation_time) == process.creation_time)
            {
                Some(sample) => match observed.observe(sample.cpu, logical_processors) {
                    Decision::Wait(status) => status,
                    Decision::Claim(since) => {
                        let coverage = cycle
                            .duty_controller
                            .filter(|_| !duty_targets.is_empty())
                            .map(|controller| {
                                let suspension = SuspensionTarget {
                                    process: target.clone(),
                                    is_service_account: process.is_service_account,
                                };
                                controller.covers_target(
                                    &suspension,
                                    cycle.allow_cross_session_process_control,
                                )
                            })
                            .transpose();
                        let result = match coverage {
                            Ok(Some(true)) => Ok(CpuAllocationApplyOutcome::Shadowed),
                            Ok(_) => coordinator
                                .apply_policy_claim(
                                    &mut inventory,
                                    Ok(&topology),
                                    CpuAllocationClaim {
                                        target,
                                        owner: ControlOwner::CpuLimiter,
                                        request: CpuAllocationRequest::AffinityLimit {
                                            selection: limit.selection.clone(),
                                        },
                                        balancing_generation: None,
                                    },
                                    cycle.allow_cross_session_process_control,
                                )
                                .map_err(|error| error.to_string()),
                            Err(error) => Err(error),
                        };
                        let status = match result {
                            Ok(
                                CpuAllocationApplyOutcome::Applied
                                | CpuAllocationApplyOutcome::Unchanged,
                            ) => match coordinator.effective_affinity_limit_count(&key) {
                                Some(count) => {
                                    if matches!(result, Ok(CpuAllocationApplyOutcome::Applied)) {
                                        log.record(
                                                    ActionLogFeature::CpuLimiter,
                                                    Some(process.id),
                                                    &process.name,
                                                    ActionLogResult::Applied,
                                                    format!("Temporarily limited processor affinity to {count} logical processors."),
                                                );
                                    }
                                    observed.confirm(since, count);
                                    active.insert(key.clone());
                                    Active(count)
                                }
                                None => Unavailable,
                            },
                            Ok(CpuAllocationApplyOutcome::Shadowed) => Shadowed,
                            Ok(CpuAllocationApplyOutcome::NoUsableTarget) => Unavailable,
                            Err(error) => {
                                log.record(
                                    ActionLogFeature::CpuLimiter,
                                    Some(process.id),
                                    &process.name,
                                    ActionLogResult::Failed,
                                    &error,
                                );
                                snapshot.last_error.get_or_insert(error);
                                snapshot.failed_processes += 1;
                                snapshot.failed_apps.push(rule.executable_path.clone());
                                Unavailable
                            }
                        };
                        if !matches!(status, Active(_)) {
                            observed.cool_down(sample.cpu.sampled_at, status);
                        }
                        status
                    }
                },
                None => {
                    observed.previous_sample = None;
                    observed.phase = Phase::Qualifying;
                    if coordinator.effective_affinity_limit_count(&key).is_some() {
                        observed.cool_down(Instant::now(), CpuLimiterAffinityStatus::CoolingDown);
                        CpuLimiterAffinityStatus::CoolingDown
                    } else {
                        Qualifying
                    }
                }
            };
            snapshot.tracked_processes += 1;
            self.processes.insert(key, observed);
            statuses.push((rule.executable_path.clone(), status));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample(start: Instant, seconds: u64, cpu_100ns: u64) -> ProcessCpuSample {
        ProcessCpuSample {
            sampled_at: start + Duration::from_secs(seconds),
            cpu_time_100ns: cpu_100ns,
        }
    }

    #[test]
    fn qualifies_from_total_cpu_usage_and_holds_while_restricted_cores_are_busy() {
        let start = Instant::now();
        let mut process = ObservedProcess::new(&CpuLimiterAffinityLimit {
            trigger_percent: 25,
            sustain_seconds: 2,
            ..Default::default()
        });
        for second in 0..3 {
            assert!(matches!(
                process.observe(sample(start, second, second * 30_000_000), 8),
                Decision::Wait(CpuLimiterAffinityStatus::Qualifying)
            ));
        }
        let Decision::Claim(since) = process.observe(sample(start, 3, 90_000_000), 8) else {
            panic!("three busy logical processors exceed 25% of an eight-processor host");
        };
        process.confirm(since, 1);
        for second in 4..=25 {
            assert!(matches!(
                process.observe(
                    sample(start, second, 90_000_000 + (second - 3) * 10_000_000),
                    8
                ),
                Decision::Claim(_)
            ));
        }
        for second in 26..29 {
            assert!(matches!(
                process.observe(sample(start, second, 310_000_000), 8),
                Decision::Claim(_)
            ));
        }
        assert!(matches!(
            process.observe(sample(start, 29, 310_000_000), 8),
            Decision::Wait(CpuLimiterAffinityStatus::CoolingDown)
        ));
        for second in 30..49 {
            assert!(matches!(
                process.observe(
                    sample(start, second, 310_000_000 + (second - 29) * 30_000_000),
                    8
                ),
                Decision::Wait(CpuLimiterAffinityStatus::CoolingDown)
            ));
        }
        assert!(matches!(
            process.observe(sample(start, 49, 910_000_000), 8),
            Decision::Wait(CpuLimiterAffinityStatus::Qualifying)
        ));
    }

    #[test]
    fn a_gap_resets_qualification_and_releases_a_held_limit() {
        let start = Instant::now();
        let mut process = ObservedProcess::new(&CpuLimiterAffinityLimit {
            trigger_percent: 10,
            sustain_seconds: 1,
            ..Default::default()
        });
        for second in [0, 1, 5, 6] {
            assert!(matches!(
                process.observe(sample(start, second, second * 10_000_000), 8),
                Decision::Wait(CpuLimiterAffinityStatus::Qualifying)
            ));
        }
        let Decision::Claim(since) = process.observe(sample(start, 7, 70_000_000), 8) else {
            panic!("fresh samples sustained the configured threshold");
        };
        process.confirm(since, 1);
        assert!(matches!(
            process.observe(sample(start, 11, 110_000_000), 8),
            Decision::Wait(CpuLimiterAffinityStatus::CoolingDown)
        ));
    }
}
