use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use crate::{
    config::{
        WatchdogAction, WatchdogComparison, WatchdogMetric, WatchdogResource, WatchdogResponse,
        WatchdogScope, WatchdogSelector, WatchdogSettings,
    },
    cpu::{process_cpu_usage_batch, ProcessCpuSample},
    foreground::{
        process_list::{path_has_executable_name, same_executable_name},
        process_matches_executable_path, same_executable_path, sample_process_resources,
        ProcessActionTarget, ProcessInfo, ProcessResourceSample, ProcessResourceSampling,
    },
    runtime::observations::CycleObservations,
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct WatchdogSnapshot {
    pub observation_known: bool,
    pub monitored_processes: usize,
    pub alerting_processes: usize,
    pub skipped_processes: usize,
    pub last_error: Option<String>,
}

#[derive(Default)]
struct BatchActivity {
    monitored: bool,
    alerting: bool,
    skipped: bool,
}

pub(crate) struct WatchdogEvent {
    pub name: String,
    pub process_count: usize,
    pub process_id: Option<u32>,
    pub scope: WatchdogScope,
    pub targets: Vec<ProcessActionTarget>,
    pub critical: bool,
    pub metric: WatchdogMetric,
    pub comparison: WatchdogComparison,
    pub value: f64,
    pub response: WatchdogResponse,
    pub notify: bool,
}

#[derive(Default)]
struct ThresholdState {
    since: Option<Instant>,
    fired: bool,
}

impl ThresholdState {
    fn observe(
        &mut self,
        value: f64,
        threshold: u64,
        comparison: WatchdogComparison,
        metric: WatchdogMetric,
        delay: Duration,
        now: Instant,
    ) -> bool {
        let threshold = threshold as f64;
        let qualifies = match comparison {
            WatchdogComparison::AtOrAbove => value >= threshold,
            WatchdogComparison::AtOrBelow => value <= threshold,
        };
        if !qualifies {
            self.since = None;
            // A small recovery margin prevents repeated alerts around the threshold.
            let recovered = match comparison {
                WatchdogComparison::AtOrAbove => value < threshold * 0.95,
                WatchdogComparison::AtOrBelow => {
                    value >= (threshold * 1.05).min(metric.maximum() as f64)
                }
            };
            if recovered {
                self.fired = false;
            }
            return false;
        }
        let since = *self.since.get_or_insert(now);
        if !self.fired && now.saturating_duration_since(since) >= delay {
            self.fired = true;
            true
        } else {
            false
        }
    }
}

#[derive(Default)]
struct ResourceState {
    warning: ThresholdState,
    critical: ThresholdState,
}

impl ResourceState {
    fn interrupt(&mut self) {
        self.warning.since = None;
        self.critical.since = None;
    }
}

type ProcessIdentity = (u32, u64);
type GroupKey<'a> = (Option<ProcessIdentity>, &'a Path);
type StateKey = (usize, Option<ProcessIdentity>, PathBuf);

#[derive(Default)]
struct AppState {
    members: BTreeMap<ProcessIdentity, Option<ProcessCpuSample>>,
    last_observed: Option<Instant>,
    resources: [ResourceState; 2],
}

impl AppState {
    fn interrupt_observation(&mut self) {
        self.members.values_mut().for_each(|sample| *sample = None);
        self.resources.iter_mut().for_each(ResourceState::interrupt);
    }
}

#[derive(Default)]
pub(crate) struct WatchdogManager {
    settings: WatchdogSettings,
    states: BTreeMap<StateKey, AppState>,
}

pub(crate) fn observed_path_matches_name(process: &ProcessInfo, executable_name: &str) -> bool {
    same_executable_name(&process.name, executable_name)
        && process
            .image_path
            .as_deref()
            .is_some_and(|path| path_has_executable_name(path, executable_name))
}

pub(crate) fn matching_path<'a>(
    selector: &'a WatchdogSelector,
    process: &'a ProcessInfo,
) -> Option<&'a Path> {
    match selector {
        WatchdogSelector::ExactPath { executable_path }
            if process_matches_executable_path(process, executable_path) =>
        {
            Some(Path::new(executable_path))
        }
        WatchdogSelector::ExactName { executable_name }
            if observed_path_matches_name(process, executable_name) =>
        {
            process.image_path.as_deref()
        }
        _ => None,
    }
}

fn group_matches<'a>(
    selector: &'a WatchdogSelector,
    scope: WatchdogScope,
    processes: &'a [ProcessInfo],
) -> BTreeMap<GroupKey<'a>, Vec<&'a ProcessInfo>> {
    let mut groups = BTreeMap::<_, Vec<_>>::new();
    for process in processes {
        let Some(path) = matching_path(selector, process) else {
            continue;
        };
        let identity = match scope {
            WatchdogScope::Combined => None,
            WatchdogScope::PerInstance => process.creation_time.map(|time| (process.id, time)),
        };
        let key = (identity, path);
        if let Some(group) = groups.get_mut(&key) {
            group.push(process);
        } else if let Some((_, group)) = groups
            .range_mut((identity, Path::new(""))..)
            .take_while(|((other_identity, _), _)| *other_identity == identity)
            .find(|((_, observed_path), _)| same_executable_path(observed_path, path))
        {
            group.push(process);
        } else {
            groups.insert(key, vec![process]);
        }
    }
    groups
}

impl WatchdogManager {
    pub(crate) fn update(
        &mut self,
        settings: &WatchdogSettings,
        enabled: bool,
        observations: &mut CycleObservations,
    ) -> (WatchdogSnapshot, Vec<WatchdogEvent>) {
        if !enabled || !settings.enabled {
            self.states.clear();
            return (WatchdogSnapshot::default(), Vec::new());
        }
        if let Err(error) = settings.validate() {
            self.states.clear();
            return (
                WatchdogSnapshot {
                    last_error: Some(error),
                    ..Default::default()
                },
                Vec::new(),
            );
        }
        let processes = match observations.processes_with_paths() {
            Ok(processes) => processes,
            Err(error) => {
                self.note_settings(settings);
                for state in self.states.values_mut() {
                    state.interrupt_observation();
                }
                return (
                    WatchdogSnapshot {
                        last_error: Some(error),
                        ..Default::default()
                    },
                    Vec::new(),
                );
            }
        };
        let mut path_gaps = BTreeMap::new();
        let matched: Vec<_> = processes
            .iter()
            .filter(|process| {
                let path = settings
                    .rules
                    .iter()
                    .filter(|rule| rule.enabled)
                    .find_map(|rule| matching_path(&rule.selector, process));
                if !self.states.is_empty()
                    && process.image_path.is_none()
                    && process.is_critical != Some(true)
                {
                    if let Some(time) = process.creation_time {
                        path_gaps.insert((process.id, time), path);
                    }
                }
                path.is_some()
            })
            .cloned()
            .collect();
        let samples = sample_process_resources(&matched, ProcessResourceSampling::UsageOnly);
        // The fallback queries by PID. Only treat its path as evidence for a
        // previous member once this pass's resource sample confirms the generation.
        for (&(pid, time), path) in &mut path_gaps {
            if path.is_some()
                && samples
                    .get(&pid)
                    .is_none_or(|sample| sample.creation_time != time)
            {
                *path = None;
            }
        }
        let total_memory_required = !matched.is_empty()
            && settings.rules.iter().any(|rule| {
                rule.enabled
                    && rule.memory.enabled
                    && rule.memory_unit.metric() == WatchdogMetric::MemoryPercent
            });
        let total_memory = total_memory_required
            .then(|| crate::backend::dashboard_metrics::sample_memory_usage().total_physical_bytes)
            .flatten();
        self.evaluate_catalog(
            settings,
            &matched,
            &path_gaps,
            &samples,
            total_memory,
            Instant::now(),
        )
    }

    fn note_settings(&mut self, settings: &WatchdogSettings) {
        if self.settings != *settings {
            self.states.clear();
            self.settings.clone_from(settings);
        }
    }

    fn evaluate_catalog(
        &mut self,
        settings: &WatchdogSettings,
        processes: &[ProcessInfo],
        path_gaps: &BTreeMap<ProcessIdentity, Option<&Path>>,
        samples: &BTreeMap<u32, ProcessResourceSample>,
        total_memory: Option<u64>,
        now: Instant,
    ) -> (WatchdogSnapshot, Vec<WatchdogEvent>) {
        self.note_settings(settings);
        let mut states = BTreeMap::new();
        let mut snapshot = WatchdogSnapshot {
            observation_known: true,
            ..Default::default()
        };
        // Count each process once per metric even if several rules match it.
        let mut activity = BTreeMap::<u32, BatchActivity>::new();
        let mut events = Vec::new();
        let mut cpu_usage = process_cpu_usage_batch();
        let unresolved_on_path = |identity: &ProcessIdentity, path: &Path| {
            path_gaps.get(identity).is_some_and(|resolved| {
                resolved.is_none_or(|resolved| same_executable_path(resolved, path))
            })
        };
        let groups = settings
            .rules
            .iter()
            .enumerate()
            .filter(|(_, rule)| rule.enabled)
            .flat_map(|(index, rule)| {
                group_matches(&rule.selector, rule.scope, processes)
                    .into_iter()
                    .map(move |((identity, path), group)| {
                        ((index, identity, path.to_owned()), rule, group)
                    })
            });
        for (key, rule, group) in groups {
            let mut matched = Vec::new();
            let mut members = BTreeMap::new();
            let mut complete = true;
            for process in group {
                if process.is_critical == Some(true) || process.id == std::process::id() {
                    activity.entry(process.id).or_default().skipped = true;
                    continue;
                }
                let sample = samples.get(&process.id).filter(|sample| {
                    process.is_critical == Some(false)
                        && Some(sample.creation_time) == process.creation_time
                });
                if let Some(creation_time) = process.creation_time {
                    members.insert((process.id, creation_time), sample.map(|sample| sample.cpu));
                }
                let Some(sample) = sample else {
                    complete = false;
                    activity.entry(process.id).or_default().skipped = true;
                    continue;
                };
                matched.push((process, sample));
            }
            let mut state = self
                .states
                .remove(&key)
                .or_else(|| {
                    let previous = self
                        .states
                        .range((key.0, key.1, PathBuf::new())..)
                        .take_while(|((index, identity, _), _)| {
                            *index == key.0 && *identity == key.1
                        })
                        .find(|((_, _, path), _)| same_executable_path(path, &key.2))
                        .map(|(key, _)| key.clone())?;
                    self.states.remove(&previous)
                })
                .unwrap_or_default();
            // An exact prior member whose path could not be read still affects coverage.
            // Keep its identity without reusing an old path or CPU sample as evidence.
            for &identity in state
                .members
                .keys()
                .filter(|identity| unresolved_on_path(identity, &key.2))
            {
                if let std::collections::btree_map::Entry::Vacant(member) = members.entry(identity)
                {
                    member.insert(None);
                    activity.entry(identity.0).or_default().skipped = true;
                    complete = false;
                }
            }
            if members.is_empty() {
                continue;
            }
            if !members
                .keys()
                .any(|identity| state.members.contains_key(identity))
            {
                // A restarted app cannot inherit the previous app's sustained interval.
                state = AppState::default();
            } else if state
                .last_observed
                .is_some_and(|last| now.saturating_duration_since(last) > Duration::from_secs(3))
            {
                state.interrupt_observation();
            }
            // Catalog identity preserves the incident when a member's CPU baseline is unavailable.
            let previous_members = std::mem::replace(&mut state.members, members);
            state.last_observed = Some(now);
            let Some((first, _)) = matched.first().filter(|_| complete) else {
                for thresholds in &mut state.resources {
                    thresholds.interrupt();
                }
                for (process, _) in matched {
                    activity.entry(process.id).or_default().skipped = true;
                }
                states.insert(key, state);
                continue;
            };
            let delay =
                Duration::from_secs(rule.sustained_seconds.unwrap_or(settings.sustained_seconds));
            let mut alerting = false;
            let mut skipped = false;
            for (resource, thresholds) in
                WatchdogResource::ALL.into_iter().zip(&mut state.resources)
            {
                let monitor = rule.monitor(resource);
                if !monitor.enabled {
                    continue;
                }
                let metric = rule.metric(resource);
                let comparison = monitor.comparison;
                let allows_actions = rule.allows_actions(resource);
                let responds = |response: &WatchdogResponse| {
                    settings.notify || (allows_actions && response.action != WatchdogAction::None)
                };
                // Every member needs a valid sample; never evaluate a partial app total.
                let value = matched.iter().try_fold(0.0, |total, (process, sample)| {
                    metric_value(
                        metric,
                        || {
                            let previous = previous_members
                                .get(&(process.id, sample.creation_time))
                                .copied()
                                .flatten()?;
                            cpu_usage(previous, sample.cpu)
                        },
                        sample,
                        total_memory,
                    )
                    .map(|value| total + value)
                });
                if let Some(value) = value.filter(|value| value.is_finite() && *value >= 0.0) {
                    let value = if metric == WatchdogMetric::CpuPercent {
                        value.min(100.0)
                    } else {
                        value
                    };
                    let critical = responds(&monitor.critical)
                        && thresholds.critical.observe(
                            value,
                            monitor.critical.threshold,
                            comparison,
                            metric,
                            delay,
                            now,
                        );
                    let warning = responds(&monitor.warning)
                        && thresholds.warning.observe(
                            value,
                            monitor.warning.threshold,
                            comparison,
                            metric,
                            delay,
                            now,
                        );
                    if critical || warning {
                        if critical {
                            thresholds.warning.fired = true;
                        }
                        let targets = if allows_actions {
                            matched
                                .iter()
                                .map(|(process, sample)| ProcessActionTarget {
                                    id: process.id,
                                    name: process.name.clone(),
                                    executable_path: key.2.clone(),
                                    creation_time: sample.creation_time,
                                    session_id: process.session_id,
                                    is_service_account: process.is_service_account,
                                })
                                .collect()
                        } else {
                            Vec::new()
                        };
                        let response = if critical {
                            &monitor.critical
                        } else {
                            &monitor.warning
                        };
                        events.push(WatchdogEvent {
                            name: if allows_actions {
                                first.name.clone()
                            } else {
                                key.2.display().to_string()
                            },
                            process_count: matched.len(),
                            process_id: (matched.len() == 1).then_some(first.id),
                            scope: rule.scope,
                            targets,
                            critical,
                            metric,
                            comparison,
                            value,
                            notify: settings.notify,
                            response: WatchdogResponse {
                                threshold: response.threshold,
                                action: if allows_actions {
                                    response.action
                                } else {
                                    WatchdogAction::None
                                },
                            },
                        });
                    }
                    alerting |= thresholds.warning.fired || thresholds.critical.fired;
                } else {
                    thresholds.interrupt();
                    skipped = true;
                }
            }
            for (process, _) in matched {
                let row = activity.entry(process.id).or_default();
                row.monitored = true;
                row.alerting |= alerting;
                row.skipped |= skipped;
            }
            states.insert(key, state);
        }
        // A path gap can hide the entire group. Preserve only exact identities still
        // observed with unresolved paths, and stop qualification until coverage returns.
        for (key, mut state) in std::mem::take(&mut self.states) {
            state
                .members
                .retain(|identity, _| unresolved_on_path(identity, &key.2));
            if !state.members.is_empty() {
                state.interrupt_observation();
                for &(pid, _) in state.members.keys() {
                    activity.entry(pid).or_default().skipped = true;
                }
                states.insert(key, state);
            }
        }
        for row in activity.into_values() {
            snapshot.monitored_processes += usize::from(row.monitored);
            snapshot.alerting_processes += usize::from(row.alerting);
            snapshot.skipped_processes += usize::from(row.skipped);
        }
        self.states = states;
        (snapshot, events)
    }
}

fn metric_value(
    metric: WatchdogMetric,
    cpu_usage: impl FnOnce() -> Option<f32>,
    current: &ProcessResourceSample,
    total_memory: Option<u64>,
) -> Option<f64> {
    match metric {
        WatchdogMetric::CpuPercent => cpu_usage().map(f64::from),
        WatchdogMetric::MemoryPercent => current
            .working_set_bytes
            .zip(total_memory.filter(|total| *total > 0))
            .map(|(bytes, total)| bytes as f64 / total as f64 * 100.0),
        WatchdogMetric::MemoryMb => current
            .working_set_bytes
            .map(|bytes| bytes as f64 / 1_000_000.0),
        WatchdogMetric::PrivateCommitMb => current
            .private_commit_bytes
            .map(|bytes| bytes as f64 / 1_000_000.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{WatchdogMemoryUnit, WatchdogRule};

    impl WatchdogManager {
        fn evaluate(
            &mut self,
            settings: &WatchdogSettings,
            processes: &[ProcessInfo],
            samples: &BTreeMap<u32, ProcessResourceSample>,
            total_memory: Option<u64>,
            now: Instant,
        ) -> (WatchdogSnapshot, Vec<WatchdogEvent>) {
            self.evaluate_catalog(
                settings,
                processes,
                &BTreeMap::new(),
                samples,
                total_memory,
                now,
            )
        }
    }

    fn process() -> ProcessInfo {
        ProcessInfo {
            id: 987654,
            creation_time: Some(10),
            parent_id: None,
            session_id: Some(1),
            user_name: None,
            is_service_account: Some(false),
            is_critical: Some(false),
            can_set_information: true,
            name: "test.exe".into(),
            image_path: Some(r"C:\Apps\test.exe".into()),
        }
    }

    fn settings() -> WatchdogSettings {
        let mut rule = WatchdogRule::new(r"C:\Apps\test.exe".into());
        rule.cpu.enabled = false;
        rule.memory.enabled = true;
        WatchdogSettings {
            enabled: true,
            sustained_seconds: 2,
            rules: vec![rule],
            ..Default::default()
        }
    }

    fn sample(
        process: &ProcessInfo,
        now: Instant,
        memory: Option<u64>,
    ) -> BTreeMap<u32, ProcessResourceSample> {
        BTreeMap::from([(
            process.id,
            ProcessResourceSample {
                cpu: ProcessCpuSample {
                    cpu_time_100ns: 1,
                    sampled_at: now,
                },
                creation_time: process.creation_time.unwrap(),
                working_set_bytes: memory,
                private_commit_bytes: None,
                efficiency_mode: None,
            },
        )])
    }

    fn tick(
        manager: &mut WatchdogManager,
        settings: &WatchdogSettings,
        process: &ProcessInfo,
        start: Instant,
        seconds: u64,
        memory: Option<u64>,
    ) -> Vec<WatchdogEvent> {
        let now = start + Duration::from_secs(seconds);
        manager
            .evaluate(
                settings,
                std::slice::from_ref(process),
                &sample(process, now, memory),
                Some(1000),
                now,
            )
            .1
    }

    #[test]
    fn restart_without_notifications_waits_for_sustained_usage_and_a_fresh_app_incident() {
        let mut manager = WatchdogManager::default();
        let mut settings = settings();
        settings.notify = false;
        settings.rules[0].memory.critical.action = WatchdogAction::Restart;
        let mut process = process();
        let start = Instant::now();
        for second in 0..2 {
            assert!(tick(&mut manager, &settings, &process, start, second, Some(950)).is_empty());
        }
        let events = tick(&mut manager, &settings, &process, start, 2, Some(950));
        assert_eq!(events.len(), 1);
        assert!(events[0].critical);
        assert!(!events[0].notify);
        assert_eq!(events[0].response.action, WatchdogAction::Restart);
        assert!(tick(&mut manager, &settings, &process, start, 3, Some(950)).is_empty());
        process.creation_time = Some(20);
        for second in 4..6 {
            assert!(tick(&mut manager, &settings, &process, start, second, Some(950)).is_empty());
        }
        let events = tick(&mut manager, &settings, &process, start, 6, Some(950));
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].targets[0].creation_time, 20);
        assert_eq!(events[0].response.action, WatchdogAction::Restart);
    }

    #[test]
    fn sustained_warning_escalates_once_and_rearms_only_after_recovery() {
        let mut manager = WatchdogManager::default();
        let mut settings = settings();
        settings.rules[0].memory.critical.action = WatchdogAction::Terminate;
        let process = process();
        let start = Instant::now();
        assert!(tick(&mut manager, &settings, &process, start, 0, Some(750)).is_empty());
        assert!(tick(&mut manager, &settings, &process, start, 1, Some(750)).is_empty());
        let warning = tick(&mut manager, &settings, &process, start, 2, Some(750));
        assert_eq!(warning.len(), 1);
        assert!(!warning[0].critical);
        assert_eq!(warning[0].response.action, WatchdogAction::None);
        assert!(warning[0].notify);
        assert!(tick(&mut manager, &settings, &process, start, 3, Some(950)).is_empty());
        assert!(tick(&mut manager, &settings, &process, start, 4, Some(950)).is_empty());
        let critical = tick(&mut manager, &settings, &process, start, 5, Some(950));
        assert_eq!(critical.len(), 1);
        assert!(critical[0].critical);
        assert_eq!(critical[0].targets[0].creation_time, 10);
        assert_eq!(critical[0].response.action, WatchdogAction::Terminate);
        assert!(tick(&mut manager, &settings, &process, start, 6, Some(950)).is_empty());
        assert!(tick(&mut manager, &settings, &process, start, 7, Some(690)).is_empty());
        for second in 8..=10 {
            assert!(tick(&mut manager, &settings, &process, start, second, Some(750)).is_empty());
        }
        assert!(tick(&mut manager, &settings, &process, start, 11, Some(500)).is_empty());
        for second in 12..14 {
            assert!(tick(&mut manager, &settings, &process, start, second, Some(950)).is_empty());
        }
        let events = tick(&mut manager, &settings, &process, start, 14, Some(950));
        assert_eq!(events.len(), 1);
        assert!(events[0].critical);
    }

    #[test]
    fn low_threshold_qualifies_at_equality_and_rearms_at_the_capped_margin() {
        let mut threshold = ThresholdState::default();
        let start = Instant::now();
        let mut observe = |second, value| {
            threshold.observe(
                value,
                99,
                WatchdogComparison::AtOrBelow,
                WatchdogMetric::CpuPercent,
                Duration::from_secs(1),
                start + Duration::from_secs(second),
            )
        };

        assert!(!observe(0, 99.0));
        assert!(observe(1, 99.0));
        assert!(!observe(2, 99.99));
        assert!(!observe(3, 99.0));
        assert!(!observe(4, 99.0));
        assert!(!observe(5, 100.0));
        assert!(!observe(6, 99.0));
        assert!(observe(7, 99.0));
    }

    #[test]
    fn low_usage_requires_measured_zero_and_never_captures_action_targets() {
        let mut manager = WatchdogManager::default();
        let mut settings = settings();
        let monitor = &mut settings.rules[0].memory;
        monitor.comparison = WatchdogComparison::AtOrBelow;
        monitor.warning.threshold = 1;
        monitor.critical.threshold = 0;
        // Guard the runtime even if invalid settings bypass config validation.
        monitor.critical.action = WatchdogAction::Restart;
        let process = process();
        let start = Instant::now();
        let mut observe =
            |second, memory| tick(&mut manager, &settings, &process, start, second, memory);

        for (second, memory) in [Some(0), None, Some(0), Some(0)].into_iter().enumerate() {
            assert!(observe(second as u64, memory).is_empty());
        }
        let events = observe(4, Some(0));
        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert!(event.critical && event.notify);
        assert_eq!(event.comparison, WatchdogComparison::AtOrBelow);
        assert_eq!(event.response.threshold, 0);
        assert_eq!(event.response.action, WatchdogAction::None);
        assert!(event.targets.is_empty());

        // Any positive reading rearms a zero threshold, even while warning qualifies.
        assert!(observe(5, Some(1)).is_empty());
        assert!(observe(6, Some(0)).is_empty());
        assert!(observe(7, Some(0)).is_empty());
        assert_eq!(observe(8, Some(0)).len(), 1);

        settings.notify = false;
        for second in 9..12 {
            assert!(tick(&mut manager, &settings, &process, start, second, Some(0)).is_empty());
        }
    }

    #[test]
    fn missing_samples_restarts_duration_and_replaced_or_protected_processes_cannot_inherit_it() {
        let mut manager = WatchdogManager::default();
        let settings = settings();
        let mut process = process();
        let start = Instant::now();
        assert!(tick(&mut manager, &settings, &process, start, 0, Some(950)).is_empty());
        assert!(tick(&mut manager, &settings, &process, start, 1, None).is_empty());
        assert!(tick(&mut manager, &settings, &process, start, 2, Some(950)).is_empty());
        process.creation_time = Some(20);
        assert!(tick(&mut manager, &settings, &process, start, 3, Some(950)).is_empty());
        assert!(tick(&mut manager, &settings, &process, start, 4, Some(950)).is_empty());
        process.is_critical = None;
        assert!(tick(&mut manager, &settings, &process, start, 5, Some(950)).is_empty());
        process.is_critical = Some(true);
        assert!(tick(&mut manager, &settings, &process, start, 6, Some(950)).is_empty());
        process.is_critical = Some(false);
        assert!(tick(&mut manager, &settings, &process, start, 7, Some(950)).is_empty());
        let now = start + Duration::from_secs(9);
        let mut samples = sample(&process, now, Some(950));
        samples.get_mut(&process.id).unwrap().creation_time = 30;
        assert!(manager
            .evaluate(&settings, &[process], &samples, Some(1000), now)
            .1
            .is_empty());
    }

    #[test]
    fn gaps_settings_changes_and_disabling_reset_observation_without_actions() {
        let mut manager = WatchdogManager::default();
        let mut settings = settings();
        let process = process();
        let start = Instant::now();
        assert!(tick(&mut manager, &settings, &process, start, 0, Some(950)).is_empty());
        assert!(tick(&mut manager, &settings, &process, start, 10, Some(950)).is_empty());
        assert!(tick(&mut manager, &settings, &process, start, 11, Some(950)).is_empty());
        assert_eq!(
            tick(&mut manager, &settings, &process, start, 12, Some(950)).len(),
            1
        );
        for second in 20..=22 {
            assert!(tick(&mut manager, &settings, &process, start, second, Some(950)).is_empty());
        }
        settings.notify = false;
        assert!(tick(&mut manager, &settings, &process, start, 23, Some(950)).is_empty());
        let mut observations = CycleObservations::default();
        let (status, events) = manager.update(&settings, false, &mut observations);
        assert_eq!(status, WatchdogSnapshot::default());
        assert!(events.is_empty());
        assert!(manager.states.is_empty());
        assert!(tick(&mut manager, &settings, &process, start, 25, Some(950)).is_empty());
        settings.enabled = false;
        assert!(manager
            .update(&settings, true, &mut observations)
            .1
            .is_empty());
        assert!(manager.states.is_empty());
    }

    #[test]
    fn cpu_and_memory_fire_together_and_missing_memory_does_not_reset_cpu() {
        let mut settings = settings();
        settings.rules[0].cpu.enabled = true;
        settings.rules[0].memory.critical.action = WatchdogAction::Terminate;
        let mut manager = WatchdogManager::default();
        let process = process();
        let start = Instant::now();
        let capacity = std::thread::available_parallelism()
            .map(usize::from)
            .unwrap_or(1) as u64;
        let mut cpu_time = 1;
        let mut tick_both = |second, memory, cpu_percent| {
            let now = start + Duration::from_secs(second);
            let mut samples = sample(&process, now, memory);
            cpu_time += capacity * cpu_percent * 100_000;
            samples.get_mut(&process.id).unwrap().cpu.cpu_time_100ns = cpu_time;
            manager.evaluate(
                &settings,
                std::slice::from_ref(&process),
                &samples,
                Some(1000),
                now,
            )
        };
        assert!(tick_both(0, Some(0), 0).1.is_empty());
        assert!(tick_both(1, Some(950), 80).1.is_empty());
        assert!(tick_both(2, Some(950), 80).1.is_empty());
        let (status, events) = tick_both(3, Some(950), 80);
        assert_eq!(status.monitored_processes, 1);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].metric, WatchdogMetric::CpuPercent);
        assert!(!events[0].critical);
        assert_eq!(events[0].response.action, WatchdogAction::None);
        assert_eq!(events[1].metric, WatchdogMetric::MemoryPercent);
        assert!(events[1].critical);
        assert_eq!(events[1].response.action, WatchdogAction::Terminate);
        assert!(tick_both(4, None, 95).1.is_empty());
        assert!(tick_both(5, None, 95).1.is_empty());
        let events = tick_both(6, None, 95).1;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].metric, WatchdogMetric::CpuPercent);
        assert!(events[0].critical);
        assert_eq!(events[0].response.action, WatchdogAction::None);
    }

    #[test]
    fn units_use_decimal_memory_and_cpu_requires_an_interval() {
        let now = Instant::now();
        let process = process();
        let mut sample = sample(&process, now, Some(512 * 1_000_000))[&process.id];
        assert_eq!(
            metric_value(WatchdogMetric::MemoryMb, || None, &sample, None),
            Some(512.0)
        );
        assert_eq!(
            metric_value(WatchdogMetric::PrivateCommitMb, || None, &sample, None),
            None
        );
        sample.private_commit_bytes = Some(384_500_000);
        assert_eq!(
            metric_value(WatchdogMetric::PrivateCommitMb, || None, &sample, None),
            Some(384.5)
        );
        assert_eq!(
            metric_value(
                WatchdogMetric::MemoryPercent,
                || None,
                &sample,
                Some(1024 * 1_000_000)
            ),
            Some(50.0)
        );
        assert_eq!(
            metric_value(WatchdogMetric::MemoryPercent, || None, &sample, Some(0)),
            None
        );
        assert_eq!(
            metric_value(WatchdogMetric::CpuPercent, || None, &sample, None),
            None
        );
        let previous = sample.cpu;
        sample.cpu.sampled_at += Duration::from_secs(1);
        sample.cpu.cpu_time_100ns += 5_000_000;
        let cpus = std::thread::available_parallelism()
            .map(usize::from)
            .unwrap_or(1);
        let mut cpu_usage = process_cpu_usage_batch();
        let actual = metric_value(
            WatchdogMetric::CpuPercent,
            || cpu_usage(previous, sample.cpu),
            &sample,
            None,
        )
        .unwrap();
        assert!((actual - 50.0 / cpus as f64).abs() < 0.001);
        sample.cpu.cpu_time_100ns = 0;
        assert_eq!(
            metric_value(
                WatchdogMetric::CpuPercent,
                || cpu_usage(previous, sample.cpu),
                &sample,
                None,
            ),
            None
        );
    }

    #[test]
    fn disabled_critical_response_does_not_mask_an_enabled_warning() {
        let mut manager = WatchdogManager::default();
        let mut settings = settings();
        settings.notify = false;
        settings.rules[0].memory.warning.action = WatchdogAction::Terminate;
        let process = process();
        let start = Instant::now();
        for second in 0..2 {
            assert!(tick(&mut manager, &settings, &process, start, second, Some(950)).is_empty());
        }
        let events = tick(&mut manager, &settings, &process, start, 2, Some(950));
        assert_eq!(events.len(), 1);
        assert!(!events[0].critical);
        assert!(!events[0].notify);
        assert_eq!(events[0].response.action, WatchdogAction::Terminate);
        settings.rules[0].memory.warning.action = WatchdogAction::None;
        for second in 3..7 {
            assert!(tick(&mut manager, &settings, &process, start, second, Some(950)).is_empty());
        }
    }

    #[test]
    fn rule_duration_overrides_the_default_and_settings_changes_restart_observation() {
        let mut settings = settings();
        settings.rules[0].memory.critical.action = WatchdogAction::Terminate;
        let mut other = process();
        other.id += 1;
        other.name = "other.exe".into();
        other.image_path = Some(r"C:\Apps\other.exe".into());
        let mut rule = settings.rules[0].clone();
        rule.selector = WatchdogSelector::ExactPath {
            executable_path: r"C:\Apps\other.exe".into(),
        };
        settings.rules.push(rule);
        let processes = [process(), other];
        let mut manager = WatchdogManager::default();
        let start = Instant::now();
        let mut observe = |settings: &WatchdogSettings, second| {
            let now = start + Duration::from_secs(second);
            let samples = processes
                .iter()
                .flat_map(|p| sample(p, now, Some(950)))
                .collect();
            manager
                .evaluate(settings, &processes, &samples, Some(1000), now)
                .1
        };
        assert!(observe(&settings, 0).is_empty());
        assert!(observe(&settings, 1).is_empty());
        let events = observe(&settings, 2);
        assert_eq!(events.len(), 2);
        assert!(events.iter().all(|event| event.notify && event.critical));
        settings.sustained_seconds = 3;
        assert!(observe(&settings, 3).is_empty());
        assert!(observe(&settings, 4).is_empty());
        settings.rules[0].sustained_seconds = Some(1);
        assert!(observe(&settings, 5).is_empty());
        let events = observe(&settings, 6);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].targets[0].id, processes[0].id);
        assert!(observe(&settings, 7).is_empty());
        let events = observe(&settings, 8);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].targets[0].id, processes[1].id);
        settings.notify = false;
        settings.rules[0].sustained_seconds = None;
        assert!(observe(&settings, 9).is_empty());
        assert!(observe(&settings, 11).is_empty());
        let events = observe(&settings, 12);
        assert_eq!(events.len(), 2);
        assert!(events
            .iter()
            .all(|event| !event.notify && event.response.action == WatchdogAction::Terminate));
    }

    #[test]
    fn memory_thresholds_use_one_combined_app_total_and_capture_only_matching_members() {
        for unit in crate::config::WatchdogMemoryUnit::ALL {
            let mut settings = settings();
            settings.rules[0].memory_unit = unit;
            settings.rules[0].memory.critical.action = WatchdogAction::Terminate;
            let mut second = process();
            second.id += 1;
            second.creation_time = Some(20);
            let mut unrelated = process();
            unrelated.id += 2;
            unrelated.image_path = Some(r"C:\Other\test.exe".into());
            let processes = [process(), second, unrelated];
            let mut manager = WatchdogManager::default();
            let start = Instant::now();
            let mut observe = |second, memory: u64| {
                let now = start + Duration::from_secs(second);
                let mut samples: BTreeMap<_, _> = processes
                    .iter()
                    .flat_map(|p| sample(p, now, Some(memory * 1_000_000)))
                    .collect();
                if unit == crate::config::WatchdogMemoryUnit::PrivateCommitMb {
                    for sample in samples.values_mut() {
                        sample.private_commit_bytes = sample.working_set_bytes.take();
                    }
                }
                manager.evaluate(&settings, &processes, &samples, Some(100 * 1_000_000), now)
            };
            assert!(observe(0, 40).1.is_empty());
            assert!(observe(1, 40).1.is_empty());
            let (status, events) = observe(2, 40);
            assert_eq!(status.monitored_processes, 2);
            assert_eq!(status.alerting_processes, 2);
            assert_eq!(events.len(), 1);
            assert!(!events[0].critical);
            assert_eq!(events[0].value, 80.0);
            assert_eq!(
                events[0]
                    .targets
                    .iter()
                    .map(|t| (t.id, t.creation_time))
                    .collect::<Vec<_>>(),
                vec![(processes[0].id, 10), (processes[1].id, 20)]
            );
            assert!(observe(3, 48).1.is_empty());
            assert!(observe(4, 48).1.is_empty());
            let (_, events) = observe(5, 48);
            assert_eq!(events.len(), 1);
            assert!(events[0].critical);
            assert_eq!(events[0].value, 96.0);
            assert_eq!(events[0].response.action, WatchdogAction::Terminate);
            assert!(observe(6, 48).1.is_empty());
        }
    }

    #[test]
    fn exact_name_alerts_track_separate_paths_and_require_complete_group_samples() {
        let mut settings = settings();
        settings.rules[0].selector = WatchdogSelector::ExactName {
            executable_name: "TEST.EXE".into(),
        };
        settings.rules[0].memory_unit = WatchdogMemoryUnit::Megabytes;
        let mut second = process();
        second.id += 1;
        let mut other = process();
        other.id += 2;
        other.image_path = Some(r"C:\Other\test.exe".into());
        let mut unverified = process();
        unverified.id += 3;
        unverified.image_path = None;
        let mut different_filename = process();
        different_filename.id += 4;
        different_filename.image_path = Some(r"C:\Apps\ test.exe".into());
        let processes = [process(), second, other, unverified, different_filename];
        let mut manager = WatchdogManager::default();
        let start = Instant::now();
        let mut observe = |second, first_mb, other_mb| {
            let now = start + Duration::from_secs(second);
            let mut samples: BTreeMap<_, _> = processes
                .iter()
                .zip([first_mb, first_mb, other_mb, 100, 100])
                .flat_map(|(process, mb)| sample(process, now, Some(mb * 1_000_000)))
                .collect();
            if second == 6 {
                samples.get_mut(&processes[1].id).unwrap().creation_time += 1;
            }
            manager.evaluate(&settings, &processes, &samples, None, now)
        };

        for second in 0..3 {
            let (snapshot, events) = observe(second, 30, 50);
            assert_eq!(snapshot.monitored_processes, 3);
            assert!(events.is_empty());
        }
        assert!(observe(3, 45, 50).1.is_empty());
        assert!(observe(4, 45, 95).1.is_empty());
        let (snapshot, events) = observe(5, 45, 95);
        assert_eq!(snapshot.alerting_processes, 2);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].name, r"C:\Apps\test.exe");
        assert_eq!(events[0].value, 90.0);
        assert_eq!(events[0].process_count, 2);
        assert_eq!(events[0].process_id, None);
        assert!(events[0].targets.is_empty());

        let (snapshot, events) = observe(6, 45, 95);
        assert_eq!(snapshot.monitored_processes, 1);
        assert_eq!(snapshot.alerting_processes, 1);
        assert_eq!(snapshot.skipped_processes, 2);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].name, r"C:\Other\test.exe");
        assert_eq!(events[0].process_count, 1);
        assert_eq!(events[0].process_id, Some(processes[2].id));
        assert!(events[0].targets.is_empty());
        assert!(observe(7, 45, 95).1.is_empty());
    }

    #[test]
    fn per_instance_thresholds_survive_sibling_gaps_but_not_pid_reuse() {
        let mut settings = settings();
        settings.rules[0].scope = WatchdogScope::PerInstance;
        settings.rules[0].selector = WatchdogSelector::ExactName {
            executable_name: "test.exe".into(),
        };
        let mut processes = [process(), process()];
        processes[1].id += 1;
        let start = Instant::now();
        let mut manager = WatchdogManager::default();
        let mut observe = |processes: &[ProcessInfo], second, memory: [Option<u64>; 2]| {
            let now = start + Duration::from_secs(second);
            let mut samples = BTreeMap::new();
            for (process, memory) in processes.iter().zip(memory) {
                if let Some(memory) = memory {
                    samples.extend(sample(process, now, Some(memory)));
                }
            }
            manager.evaluate(&settings, processes, &samples, Some(1_000), now)
        };
        let assert_instance = |events: Vec<WatchdogEvent>, process: &ProcessInfo| {
            assert_eq!(events.len(), 1);
            let event = &events[0];
            assert_eq!(event.scope, WatchdogScope::PerInstance);
            assert_eq!(event.process_id, Some(process.id));
            assert_eq!(event.process_count, 1);
            assert_eq!(
                event.name,
                process.image_path.as_ref().unwrap().display().to_string()
            );
            assert_eq!(event.value, 95.0);
            assert!(event.notify && event.critical);
            assert_eq!(event.response.action, WatchdogAction::None);
            assert!(event.targets.is_empty());
        };

        for second in 0..3 {
            let (snapshot, events) = observe(&processes, second, [Some(400); 2]);
            assert_eq!(snapshot.monitored_processes, 2);
            assert!(events.is_empty());
        }
        assert!(observe(&processes, 3, [Some(950); 2]).1.is_empty());
        processes[1].image_path = Some(r"c:\apps\TEST.EXE".into());
        let (snapshot, events) = observe(&processes, 4, [None, Some(950)]);
        assert_eq!(snapshot.monitored_processes, 1);
        assert_eq!(snapshot.skipped_processes, 1);
        assert!(events.is_empty());
        assert_instance(observe(&processes, 5, [Some(950); 2]).1, &processes[1]);
        assert!(observe(&processes, 6, [Some(950), Some(400)]).1.is_empty());
        assert_instance(observe(&processes, 7, [Some(950); 2]).1, &processes[0]);

        processes[0].creation_time = Some(20);
        assert!(observe(&processes, 8, [Some(950); 2]).1.is_empty());
        assert_instance(observe(&processes, 9, [Some(950); 2]).1, &processes[1]);
        assert_instance(observe(&processes, 10, [Some(950); 2]).1, &processes[0]);

        let (snapshot, events) = observe(&processes[1..], 11, [Some(950), None]);
        assert_eq!(snapshot.monitored_processes, 1);
        assert_eq!(snapshot.alerting_processes, 1);
        assert!(events.is_empty());
        assert!(observe(&processes, 12, [Some(950); 2]).1.is_empty());
        assert!(observe(&processes, 13, [Some(950); 2]).1.is_empty());
        assert_instance(observe(&processes, 14, [Some(950); 2]).1, &processes[0]);
    }

    #[test]
    fn notification_only_rules_cannot_dispatch_process_actions() {
        for (selector, scope) in [
            (
                WatchdogSelector::ExactName {
                    executable_name: "test.exe".into(),
                },
                WatchdogScope::Combined,
            ),
            (
                WatchdogSelector::ExactPath {
                    executable_path: r"C:\Apps\test.exe".into(),
                },
                WatchdogScope::PerInstance,
            ),
        ] {
            let mut settings = settings();
            settings.rules[0].selector = selector;
            settings.rules[0].scope = scope;
            settings.rules[0].memory.critical.action = WatchdogAction::Restart;
            let mut manager = WatchdogManager::default();
            let process = process();
            let start = Instant::now();
            assert!(tick(&mut manager, &settings, &process, start, 0, Some(950)).is_empty());
            assert!(tick(&mut manager, &settings, &process, start, 1, Some(950)).is_empty());
            let events = tick(&mut manager, &settings, &process, start, 2, Some(950));
            assert_eq!(events.len(), 1);
            assert!(events[0].notify && events[0].critical);
            assert_eq!(events[0].response.action, WatchdogAction::None);
            assert!(events[0].targets.is_empty());

            settings.notify = false;
            for second in 3..=5 {
                assert!(
                    tick(&mut manager, &settings, &process, start, second, Some(950)).is_empty()
                );
            }
        }
    }

    #[test]
    fn cpu_totals_require_complete_member_intervals_and_do_not_inherit_restarted_apps() {
        let mut settings = settings();
        settings.rules[0].memory.enabled = false;
        settings.rules[0].cpu.enabled = true;
        let mut second = process();
        second.id += 1;
        let mut processes = vec![process(), second];
        let mut manager = WatchdogManager::default();
        let start = Instant::now();
        let capacity = std::thread::available_parallelism()
            .map(usize::from)
            .unwrap_or(1) as u64;
        let observe = |manager: &mut WatchdogManager, processes: &[ProcessInfo], second| {
            let now = start + Duration::from_secs(second);
            let samples = processes
                .iter()
                .flat_map(|p| {
                    let mut samples = sample(p, now, Some(0));
                    samples.get_mut(&p.id).unwrap().cpu.cpu_time_100ns =
                        1 + second * capacity * 40 * 100_000;
                    samples
                })
                .collect();
            manager
                .evaluate(&settings, processes, &samples, None, now)
                .1
        };
        for second in 0..3 {
            assert!(observe(&mut manager, &processes, second).is_empty());
        }
        let events = observe(&mut manager, &processes, 3);
        assert_eq!(events.len(), 1);
        assert!((events[0].value - 80.0).abs() < 0.001);
        // Joining processes need their own CPU baseline; app alerts remain one per incident.
        let mut joining = process();
        joining.id += 2;
        processes.push(joining);
        assert!(observe(&mut manager, &processes, 4).is_empty());
        assert!(observe(&mut manager, &processes, 5).is_empty());
        assert!(observe(&mut manager, &processes, 6).is_empty());
        let events = observe(&mut manager, &processes, 7);
        assert_eq!(events.len(), 1);
        assert!(events[0].critical);
        assert_eq!(events[0].value, 100.0);
        for process in &mut processes {
            process.creation_time = Some(50);
        }
        for second in 8..11 {
            assert!(observe(&mut manager, &processes, second).is_empty());
        }
        assert_eq!(observe(&mut manager, &processes, 11).len(), 1);
    }

    #[test]
    fn incomplete_member_memory_samples_break_the_combined_sustained_interval() {
        for missing_process_sample in [false, true] {
            let settings = settings();
            let mut second = process();
            second.id += 1;
            let processes = [process(), second];
            let mut manager = WatchdogManager::default();
            let start = Instant::now();
            for second in 0..5 {
                let now = start + Duration::from_secs(second);
                let mut samples: BTreeMap<_, _> = processes
                    .iter()
                    .enumerate()
                    .flat_map(|(i, p)| {
                        sample(
                            p,
                            now,
                            if second == 1 && i == 1 {
                                None
                            } else {
                                Some(400)
                            },
                        )
                    })
                    .collect();
                if second == 1 && missing_process_sample {
                    samples.remove(&processes[1].id);
                }
                let events = manager
                    .evaluate(&settings, &processes, &samples, Some(1000), now)
                    .1;
                assert_eq!(events.len(), usize::from(second == 4));
            }
        }
    }

    #[test]
    fn unknown_criticality_breaks_the_combined_low_usage_interval() {
        let mut settings = settings();
        let monitor = &mut settings.rules[0].memory;
        monitor.comparison = WatchdogComparison::AtOrBelow;
        monitor.warning.threshold = 30;
        monitor.critical.threshold = 20;
        let mut processes = [process(), process()];
        processes[1].id += 1;
        let mut manager = WatchdogManager::default();
        let start = Instant::now();
        for second in 0..5 {
            processes[1].is_critical = (second != 1).then_some(false);
            let now = start + Duration::from_secs(second);
            let samples = processes
                .iter()
                .flat_map(|process| sample(process, now, Some(100)))
                .collect();
            let (snapshot, events) =
                manager.evaluate(&settings, &processes, &samples, Some(1000), now);
            if second == 1 {
                assert_eq!(snapshot.skipped_processes, 2);
            }
            assert_eq!(events.len(), usize::from(second == 4));
        }
    }

    #[test]
    fn missing_samples_preserve_fired_incidents_for_observed_exact_identities() {
        for scope in [WatchdogScope::Combined, WatchdogScope::PerInstance] {
            let mut settings = settings();
            let rule = &mut settings.rules[0];
            rule.scope = scope;
            if scope == WatchdogScope::Combined {
                rule.memory.critical.action = WatchdogAction::Terminate;
            }
            let mut manager = WatchdogManager::default();
            let mut process = process();
            let start = Instant::now();
            for second in 0..10 {
                if second == 7 {
                    process.creation_time = Some(20);
                }
                let now = start + Duration::from_secs(second);
                let samples = if second == 3 {
                    BTreeMap::new()
                } else {
                    sample(&process, now, Some(950))
                };
                let (_, events) = manager.evaluate(
                    &settings,
                    std::slice::from_ref(&process),
                    &samples,
                    Some(1000),
                    now,
                );
                assert_eq!(events.len(), usize::from(matches!(second, 2 | 9)));
            }
        }
    }

    #[test]
    fn unreadable_path_interrupts_an_exact_members_combined_interval() {
        let mut settings = settings();
        let rule = &mut settings.rules[0];
        rule.selector = WatchdogSelector::ExactName {
            executable_name: "test.exe".into(),
        };
        rule.memory.comparison = WatchdogComparison::AtOrBelow;
        rule.memory.warning.threshold = 30;
        rule.memory.critical.threshold = 5;
        let mut processes = [process(), process()];
        processes[1].id += 1;
        let start = Instant::now();
        let mut manager = WatchdogManager::default();

        for second in 0..5 {
            let now = start + Duration::from_secs(second);
            let samples = sample(&processes[0], now, Some(0));
            let (snapshot, events) = if (1..=3).contains(&second) {
                let unresolved_paths = BTreeMap::from([((processes[1].id, 10), None)]);
                manager.evaluate_catalog(
                    &settings,
                    &processes[..1],
                    &unresolved_paths,
                    &samples,
                    Some(1000),
                    now,
                )
            } else {
                let mut samples = samples;
                samples.extend(sample(&processes[1], now, Some(800)));
                manager.evaluate_catalog(
                    &settings,
                    &processes,
                    &BTreeMap::new(),
                    &samples,
                    Some(1000),
                    now,
                )
            };
            if (1..=3).contains(&second) {
                assert_eq!(snapshot.skipped_processes, 2);
            }
            assert!(events.is_empty());
        }
    }

    #[test]
    fn live_memory_rule_terminates_only_its_owned_disposable_children() {
        use std::{
            os::windows::process::CommandExt,
            process::{Child, Command, Stdio},
        };
        struct ChildGuard(Child);
        impl Drop for ChildGuard {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }
        // Retained stdin keeps these private children alive; no other cmd instance is evaluated.
        let mut children: Vec<_> = (0..2)
            .map(|_| {
                ChildGuard(
                    Command::new("cmd.exe")
                        .args(["/d", "/q", "/c", "set /p watchdog_wait="])
                        .creation_flags(windows_sys::Win32::System::Threading::CREATE_NO_WINDOW)
                        .stdin(Stdio::piped())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn()
                        .unwrap(),
                )
            })
            .collect();
        std::thread::sleep(Duration::from_millis(200));
        let processes: Vec<_> = CycleObservations::default()
            .processes_with_paths()
            .unwrap()
            .iter()
            .filter(|process| children.iter().any(|child| process.id == child.0.id()))
            .cloned()
            .collect();
        assert_eq!(processes.len(), 2);
        let process = &processes[0];
        let mut rule = WatchdogRule::new(
            process
                .image_path
                .as_ref()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
        );
        rule.cpu.enabled = false;
        rule.memory.enabled = true;
        rule.memory_unit = crate::config::WatchdogMemoryUnit::Megabytes;
        rule.memory.warning.threshold = 1;
        rule.memory.critical.threshold = 2;
        rule.memory.critical.action = WatchdogAction::Terminate;
        let settings = WatchdogSettings {
            enabled: true,
            sustained_seconds: 1,
            notify: false,
            rules: vec![rule],
        };
        let mut manager = WatchdogManager::default();
        let samples = sample_process_resources(&processes, ProcessResourceSampling::UsageOnly);
        let now = Instant::now();
        assert!(manager
            .evaluate(&settings, &processes, &samples, None, now)
            .1
            .is_empty());
        std::thread::sleep(Duration::from_millis(1100));
        let samples = sample_process_resources(&processes, ProcessResourceSampling::UsageOnly);
        let (_, events) = manager.evaluate(&settings, &processes, &samples, None, Instant::now());
        assert_eq!(events.len(), 1);
        let event = events.into_iter().next().unwrap();
        assert!(event.critical);
        assert!(!event.notify);
        assert_eq!(event.response.action, WatchdogAction::Terminate);
        assert_eq!(event.targets.len(), 2);
        assert!(event
            .targets
            .iter()
            .all(|target| children.iter().any(|child| target.id == child.0.id())));
        crate::control::process_termination::ProcessTerminationController::default()
            .terminate_batch(event.targets, false)
            .into_termination_result()
            .unwrap();
        for child in &mut children {
            assert!(!child.0.wait().unwrap().success());
        }
    }
}
