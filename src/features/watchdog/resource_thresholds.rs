use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use crate::{
    config::{
        ResourceThresholdsMetric, ResourceThresholdsResource, ResourceThresholdsResponse,
        ResourceThresholdsSelector, ResourceThresholdsSettings,
    },
    cpu::{process_cpu_usage_batch, ProcessCpuSample},
    foreground::{
        process_list::{
            executable_basename_may_match, path_has_executable_name, same_executable_name,
        },
        process_matches_executable_path, same_executable_path, sample_process_resources,
        ProcessActionTarget, ProcessInfo, ProcessResourceSample, ProcessResourceSampling,
    },
    runtime::observations::CycleObservations,
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct ResourceThresholdsSnapshot {
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

pub(crate) struct ResourceThresholdsEvent {
    pub(crate) rule_index: usize,
    pub name: String,
    pub process_count: usize,
    pub process_id: Option<u32>,
    pub targets: Vec<ProcessActionTarget>,
    pub critical: bool,
    pub metric: ResourceThresholdsMetric,
    pub value: f64,
    pub response: ResourceThresholdsResponse,
    pub notify: bool,
}

#[derive(Default)]
struct ThresholdState {
    since: Option<Instant>,
    fired: bool,
}

impl ThresholdState {
    fn observe(&mut self, value: f64, threshold: u64, delay: Duration, now: Instant) -> bool {
        let threshold = threshold as f64;
        if value < threshold {
            self.since = None;
            // A small recovery margin prevents repeated alerts around the threshold.
            if value < threshold * 0.95 {
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
type StateKey = (usize, PathBuf);

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
pub(crate) struct ResourceThresholdsManager {
    settings: ResourceThresholdsSettings,
    states: BTreeMap<StateKey, AppState>,
}

pub(crate) fn selector_basename_matches(
    selector: &ResourceThresholdsSelector,
    process_name: &str,
) -> bool {
    match selector {
        ResourceThresholdsSelector::ExactPath { executable_path } => Path::new(executable_path)
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| executable_basename_may_match(process_name, name)),
        ResourceThresholdsSelector::ExactName { executable_name } => {
            same_executable_name(process_name, executable_name)
        }
        ResourceThresholdsSelector::NamePattern {
            executable_name_pattern,
        } => executable_name_pattern.matches(process_name),
    }
}

fn needs_path_observation(
    process: &ProcessInfo,
    settings: &ResourceThresholdsSettings,
    prior_members: &BTreeSet<ProcessIdentity>,
) -> bool {
    // A snapshot PID can still belong to a prior member before its creation time is read.
    let first = (process.id, process.creation_time.unwrap_or(0));
    let last = (process.id, process.creation_time.unwrap_or(u64::MAX));
    prior_members.range(first..=last).next().is_some()
        || settings
            .rules
            .iter()
            .filter(|rule| rule.enabled)
            .any(|rule| selector_basename_matches(&rule.selector, &process.name))
}

pub(crate) fn observed_path_matches_name(process: &ProcessInfo, executable_name: &str) -> bool {
    same_executable_name(&process.name, executable_name)
        && process
            .image_path
            .as_deref()
            .is_some_and(|path| path_has_executable_name(path, executable_name))
}

pub(crate) fn matching_path<'a>(
    selector: &'a ResourceThresholdsSelector,
    process: &'a ProcessInfo,
) -> Option<&'a Path> {
    match selector {
        ResourceThresholdsSelector::ExactPath { executable_path } => {
            let selected_path = Path::new(executable_path);
            let matches = if process.creation_time.is_some() {
                process_matches_executable_path(process, executable_path)
            } else {
                // The cached image can establish incomplete group coverage, but
                // sampling and action targets still require the process generation.
                let basename = selected_path.file_name()?.to_str()?;
                process.image_path.as_deref().is_some_and(|path| {
                    path.is_absolute()
                        && executable_basename_may_match(&process.name, basename)
                        && same_executable_path(path, selected_path)
                })
            };
            matches.then_some(selected_path)
        }
        ResourceThresholdsSelector::ExactName { executable_name }
            if observed_path_matches_name(process, executable_name) =>
        {
            process.image_path.as_deref()
        }
        ResourceThresholdsSelector::NamePattern {
            executable_name_pattern,
        } => process.image_path.as_deref().filter(|path| {
            executable_name_pattern.matches(&process.name)
                && path_has_executable_name(path, &process.name)
        }),
        _ => None,
    }
}

pub(crate) fn group_matches<'a>(
    selector: &'a ResourceThresholdsSelector,
    processes: impl IntoIterator<Item = &'a ProcessInfo>,
) -> BTreeMap<&'a Path, Vec<&'a ProcessInfo>> {
    let mut groups = BTreeMap::<&Path, Vec<_>>::new();
    for process in processes {
        let Some(path) = matching_path(selector, process) else {
            continue;
        };
        if let Some(group) = groups.get_mut(path) {
            group.push(process);
        } else if let Some((_, group)) = groups
            .iter_mut()
            .find(|(observed_path, _)| same_executable_path(observed_path, path))
        {
            group.push(process);
        } else {
            groups.insert(path, vec![process]);
        }
    }
    groups
}

impl ResourceThresholdsManager {
    pub(crate) fn update(
        &mut self,
        settings: &ResourceThresholdsSettings,
        enabled: bool,
        observations: &mut CycleObservations,
    ) -> (ResourceThresholdsSnapshot, Vec<ResourceThresholdsEvent>) {
        if !enabled || !settings.enabled {
            self.states.clear();
            return (ResourceThresholdsSnapshot::default(), Vec::new());
        }
        if let Err(error) = settings.validate() {
            self.states.clear();
            return (
                ResourceThresholdsSnapshot {
                    last_error: Some(error),
                    ..Default::default()
                },
                Vec::new(),
            );
        }
        let prior_members = self
            .states
            .values()
            .flat_map(|state| state.members.keys().copied())
            .collect();
        let processes = match observations.processes_for_path_matching(|process| {
            needs_path_observation(process, settings, &prior_members)
        }) {
            Ok(processes) => processes,
            Err(error) => {
                self.note_settings(settings);
                for state in self.states.values_mut() {
                    state.interrupt_observation();
                }
                return (
                    ResourceThresholdsSnapshot {
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
        let sampling = sampling_mode(settings);
        let samples = sample_process_resources(&matched, sampling);
        // Treat fallback evidence as current only when this pass also sampled
        // the exact process generation.
        for (&(pid, time), path) in &mut path_gaps {
            if path.is_some()
                && samples
                    .get(&pid)
                    .is_none_or(|sample| sample.creation_time != time)
            {
                *path = None;
            }
        }
        let total_memory_required = !samples.is_empty()
            && settings.rules.iter().any(|rule| {
                rule.enabled
                    && rule.memory.enabled
                    && rule.metric(ResourceThresholdsResource::Memory)
                        == ResourceThresholdsMetric::MemoryPercent
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

    fn note_settings(&mut self, settings: &ResourceThresholdsSettings) {
        if !self.settings.same_monitoring_policy(settings) {
            self.states.clear();
            self.settings.clone_from(settings);
        }
    }

    fn evaluate_catalog(
        &mut self,
        settings: &ResourceThresholdsSettings,
        processes: &[ProcessInfo],
        path_gaps: &BTreeMap<ProcessIdentity, Option<&Path>>,
        samples: &BTreeMap<u32, ProcessResourceSample>,
        total_memory: Option<u64>,
        now: Instant,
    ) -> (ResourceThresholdsSnapshot, Vec<ResourceThresholdsEvent>) {
        self.note_settings(settings);
        let mut states = BTreeMap::new();
        let mut snapshot = ResourceThresholdsSnapshot {
            observation_known: true,
            ..Default::default()
        };
        // Count each process once per metric even if several rules match it.
        let mut activity = BTreeMap::<u32, BatchActivity>::new();
        let mut events = Vec::new();
        let mut cpu_usage = process_cpu_usage_batch();
        let delay = Duration::from_secs(settings.sustained_seconds);
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
                group_matches(&rule.selector, processes)
                    .into_iter()
                    .map(move |(path, group)| ((index, path.to_owned()), rule, group))
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
                        .range((key.0, PathBuf::new())..)
                        .take_while(|((index, _), _)| *index == key.0)
                        .find(|((_, path), _)| same_executable_path(path, &key.1))
                        .map(|(key, _)| key.clone())?;
                    self.states.remove(&previous)
                })
                .unwrap_or_default();
            // An exact prior member whose path could not be read still affects coverage.
            // Keep its identity without reusing an old path or CPU sample as evidence.
            for &identity in state
                .members
                .keys()
                .filter(|identity| unresolved_on_path(identity, &key.1))
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
            let mut alerting = false;
            let mut skipped = false;
            for (resource, thresholds) in ResourceThresholdsResource::ALL
                .into_iter()
                .zip(&mut state.resources)
            {
                let monitor = rule.monitor(resource);
                if !monitor.enabled {
                    continue;
                }
                let metric = rule.metric(resource);
                // Every member needs a valid sample; never evaluate a partial app total.
                let value = combined_metric_value(
                    metric,
                    &matched,
                    |process, sample| {
                        let previous = previous_members
                            .get(&(process.id, sample.creation_time))
                            .copied()
                            .flatten()?;
                        cpu_usage(previous, sample.cpu)
                    },
                    total_memory,
                );
                if let Some(value) = value.filter(|value| value.is_finite() && *value >= 0.0) {
                    let value = if metric == ResourceThresholdsMetric::CpuPercent {
                        value.min(100.0)
                    } else {
                        value
                    };
                    let critical = rule.responds_to(resource, monitor.critical.action)
                        && thresholds.critical.observe(
                            value,
                            monitor.critical.threshold,
                            delay,
                            now,
                        );
                    let warning_enabled = rule.responds_to(resource, monitor.warning.action);
                    let warning = warning_enabled
                        && thresholds
                            .warning
                            .observe(value, monitor.warning.threshold, delay, now);
                    if critical || warning {
                        if critical && warning_enabled {
                            thresholds.warning.fired = true;
                        }
                        let response = if critical {
                            &monitor.critical
                        } else {
                            &monitor.warning
                        };
                        let targets = if response.action.mutates_process() {
                            matched
                                .iter()
                                .map(|(process, sample)| ProcessActionTarget {
                                    id: process.id,
                                    name: process.name.clone(),
                                    executable_path: key.1.clone(),
                                    creation_time: sample.creation_time,
                                    session_id: process.session_id,
                                    is_service_account: process.is_service_account,
                                })
                                .collect()
                        } else {
                            Vec::new()
                        };
                        events.push(ResourceThresholdsEvent {
                            rule_index: key.0,
                            name: if rule.selector.exact_path().is_some() {
                                first.name.clone()
                            } else {
                                key.1.display().to_string()
                            },
                            process_count: matched.len(),
                            process_id: (matched.len() == 1).then_some(first.id),
                            targets,
                            critical,
                            metric,
                            value,
                            notify: settings.notify && rule.notify,
                            response: response.clone(),
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
                .retain(|identity, _| unresolved_on_path(identity, &key.1));
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

fn sampling_mode(settings: &ResourceThresholdsSettings) -> ProcessResourceSampling {
    if settings
        .rules
        .iter()
        .any(|rule| rule.enabled && rule.memory.enabled)
    {
        ProcessResourceSampling::CpuAndMemory
    } else {
        ProcessResourceSampling::CpuOnly
    }
}

fn combined_metric_value(
    metric: ResourceThresholdsMetric,
    members: &[(&ProcessInfo, &ProcessResourceSample)],
    mut cpu_usage: impl FnMut(&ProcessInfo, &ProcessResourceSample) -> Option<f32>,
    total_memory: Option<u64>,
) -> Option<f64> {
    match metric {
        ResourceThresholdsMetric::CpuPercent => {
            members.iter().try_fold(0.0, |total, (process, sample)| {
                Some(total + f64::from(cpu_usage(process, sample)?))
            })
        }
        ResourceThresholdsMetric::MemoryPercent | ResourceThresholdsMetric::MemoryMb => {
            let bytes = members.iter().try_fold(0_u64, |total, (_, sample)| {
                total.checked_add(sample.working_set_bytes?)
            })? as f64;
            if metric == ResourceThresholdsMetric::MemoryMb {
                Some(bytes / 1_000_000.0)
            } else {
                total_memory
                    .filter(|total| *total > 0)
                    .map(|total| bytes / total as f64 * 100.0)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{
        ExecutableNamePattern, ResourceThresholdsAction, ResourceThresholdsMemoryUnit,
        ResourceThresholdsRule,
    };

    impl ResourceThresholdsManager {
        fn evaluate(
            &mut self,
            settings: &ResourceThresholdsSettings,
            processes: &[ProcessInfo],
            samples: &BTreeMap<u32, ProcessResourceSample>,
            total_memory: Option<u64>,
            now: Instant,
        ) -> (ResourceThresholdsSnapshot, Vec<ResourceThresholdsEvent>) {
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

    fn settings() -> ResourceThresholdsSettings {
        let mut rule = ResourceThresholdsRule::new(r"C:\Apps\test.exe".into());
        rule.cpu.enabled = false;
        rule.memory.enabled = true;
        ResourceThresholdsSettings {
            enabled: true,
            sustained_seconds: 2,
            rules: vec![rule],
            ..Default::default()
        }
    }

    #[test]
    fn path_observation_keeps_selector_candidates_and_possible_prior_members() {
        let mut disabled = ResourceThresholdsRule::new(r"C:\Apps\disabled.exe".into());
        disabled.enabled = false;
        let settings = ResourceThresholdsSettings {
            rules: vec![
                ResourceThresholdsRule::new(r"C:\Apps\Éditor.exe".into()),
                ResourceThresholdsRule::with_selector(ResourceThresholdsSelector::ExactName {
                    executable_name: "Player.exe".into(),
                }),
                disabled,
            ],
            ..settings()
        };
        let mut process = process();
        process.image_path = None;
        let prior_members = BTreeSet::from([(process.id, 11)]);

        for (name, expected) in [
            ("  éditor.EXE  ", true),
            ("PLAYER.exe", true),
            (" PLAYER.exe", false),
            ("disabled.exe", false),
            ("unrelated.exe", false),
        ] {
            process.name = name.into();
            assert_eq!(
                needs_path_observation(&process, &settings, &prior_members),
                expected
            );
        }
        process.creation_time = None;
        assert!(needs_path_observation(&process, &settings, &prior_members));
        process.creation_time = Some(11);
        assert!(needs_path_observation(&process, &settings, &prior_members));
    }

    #[test]
    fn sampling_reads_memory_only_when_an_enabled_rule_monitors_it() {
        let mut settings = settings();
        let mut memory_rule = settings.rules.pop().unwrap();
        memory_rule.enabled = false;
        settings
            .rules
            .push(ResourceThresholdsRule::new(r"C:\Apps\cpu.exe".into()));
        assert!(matches!(
            sampling_mode(&settings),
            ProcessResourceSampling::CpuOnly
        ));

        settings.rules.push(memory_rule);
        assert!(matches!(
            sampling_mode(&settings),
            ProcessResourceSampling::CpuOnly
        ));

        settings.rules[1].enabled = true;
        assert!(matches!(
            sampling_mode(&settings),
            ProcessResourceSampling::CpuAndMemory
        ));
        settings.rules[0].enabled = false;
        assert!(matches!(
            sampling_mode(&settings),
            ProcessResourceSampling::CpuAndMemory
        ));
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
        manager: &mut ResourceThresholdsManager,
        settings: &ResourceThresholdsSettings,
        process: &ProcessInfo,
        start: Instant,
        seconds: u64,
        memory: Option<u64>,
    ) -> Vec<ResourceThresholdsEvent> {
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
        let mut manager = ResourceThresholdsManager::default();
        let mut settings = settings();
        settings.notify = false;
        settings.rules[0].memory.critical.action = ResourceThresholdsAction::Restart;
        let mut process = process();
        let start = Instant::now();
        for second in 0..2 {
            assert!(tick(&mut manager, &settings, &process, start, second, Some(950)).is_empty());
        }
        let events = tick(&mut manager, &settings, &process, start, 2, Some(950));
        assert_eq!(events.len(), 1);
        assert!(events[0].critical);
        assert!(!events[0].notify);
        assert_eq!(events[0].response.action, ResourceThresholdsAction::Restart);
        assert!(tick(&mut manager, &settings, &process, start, 3, Some(950)).is_empty());
        process.creation_time = Some(20);
        for second in 4..6 {
            assert!(tick(&mut manager, &settings, &process, start, second, Some(950)).is_empty());
        }
        let events = tick(&mut manager, &settings, &process, start, 6, Some(950));
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].targets[0].creation_time, 20);
        assert_eq!(events[0].response.action, ResourceThresholdsAction::Restart);
    }

    #[test]
    fn sustained_memory_trim_captures_only_instances_of_the_selected_path() {
        let mut manager = ResourceThresholdsManager::default();
        let mut settings = settings();
        settings.rules[0].memory.warning.action = ResourceThresholdsAction::TrimMemory;
        let first = process();
        let second = ProcessInfo {
            id: first.id + 1,
            creation_time: Some(20),
            ..first.clone()
        };
        let other = ProcessInfo {
            id: first.id + 2,
            image_path: Some(r"C:\Other\test.exe".into()),
            ..first.clone()
        };
        let expected_path = first.image_path.clone().unwrap();
        let expected_identities = [(first.id, 10), (second.id, 20)];
        let processes = [first, second, other];
        let start = Instant::now();
        let mut evaluate = |seconds, bytes| {
            let now = start + Duration::from_secs(seconds);
            let samples = processes
                .iter()
                .flat_map(|process| sample(process, now, Some(bytes)))
                .collect();
            manager
                .evaluate(&settings, &processes, &samples, Some(1000), now)
                .1
        };

        assert!(evaluate(0, 350).is_empty());
        assert!(evaluate(1, 350).is_empty());
        let events = evaluate(2, 350);
        assert!(matches!(&events[..], [event] if !event.critical
            && event.response.action == ResourceThresholdsAction::TrimMemory
            && event.value == 70.0));
        let [event] = events.as_slice() else {
            panic!("expected one trim event");
        };
        assert_eq!(
            event
                .targets
                .iter()
                .map(|target| (target.id, target.creation_time))
                .collect::<Vec<_>>(),
            expected_identities,
        );
        assert!(event
            .targets
            .iter()
            .all(|target| target.executable_path == expected_path));
        assert!(evaluate(3, 350).is_empty());
    }

    #[test]
    fn notification_edits_preserve_qualification_actions_and_incidents() {
        let mut manager = ResourceThresholdsManager::default();
        let mut settings = settings();
        settings.rules[0].memory.critical.action = ResourceThresholdsAction::Restart;
        let process = process();
        let start = Instant::now();

        assert!(tick(&mut manager, &settings, &process, start, 0, Some(750)).is_empty());
        settings.rules[0].notify = false;
        let warning = tick(&mut manager, &settings, &process, start, 2, Some(750));
        assert!(matches!(&warning[..], [event] if !event.critical
            && !event.notify
            && event.response.action == ResourceThresholdsAction::Alert
            && event.targets.is_empty()));

        settings.rules[0].notify = true;
        assert!(tick(&mut manager, &settings, &process, start, 3, Some(950)).is_empty());
        settings.notify = false;
        let critical = tick(&mut manager, &settings, &process, start, 5, Some(950));
        assert!(matches!(&critical[..], [event] if event.critical && !event.notify));
        assert_eq!(
            critical[0].response.action,
            ResourceThresholdsAction::Restart
        );

        settings.notify = true;
        assert!(tick(&mut manager, &settings, &process, start, 6, Some(950)).is_empty());
        assert!(tick(&mut manager, &settings, &process, start, 7, Some(500)).is_empty());
        assert!(tick(&mut manager, &settings, &process, start, 8, Some(950)).is_empty());
        let next_incident = tick(&mut manager, &settings, &process, start, 10, Some(950));
        assert!(matches!(&next_incident[..], [event] if event.critical && event.notify));
    }

    #[test]
    fn critical_only_response_clears_alerting_and_rearms_after_recovery() {
        let mut manager = ResourceThresholdsManager::default();
        let mut settings = settings();
        settings.notify = false;
        settings.rules[0].memory.warning.action = ResourceThresholdsAction::None;
        settings.rules[0].memory.critical.action = ResourceThresholdsAction::Terminate;
        let process = process();
        let start = Instant::now();
        let mut evaluate = |seconds, bytes| {
            let now = start + Duration::from_secs(seconds);
            manager.evaluate(
                &settings,
                std::slice::from_ref(&process),
                &sample(&process, now, Some(bytes)),
                Some(1000),
                now,
            )
        };

        assert!(evaluate(0, 950).1.is_empty());
        let (alerting, events) = evaluate(2, 950);
        assert_eq!(alerting.alerting_processes, 1);
        assert!(matches!(&events[..], [event] if event.critical));

        let (recovered, events) = evaluate(3, 800);
        assert_eq!(recovered.alerting_processes, 0);
        assert!(events.is_empty());

        assert!(evaluate(4, 950).1.is_empty());
        let (_, events) = evaluate(6, 950);
        assert!(matches!(&events[..], [event] if event.critical));
    }

    #[test]
    fn sustained_warning_escalates_once_and_rearms_only_after_recovery() {
        let mut manager = ResourceThresholdsManager::default();
        let mut settings = settings();
        settings.rules[0].memory.critical.action = ResourceThresholdsAction::Terminate;
        let process = process();
        let start = Instant::now();
        assert!(tick(&mut manager, &settings, &process, start, 0, Some(750)).is_empty());
        assert!(tick(&mut manager, &settings, &process, start, 1, Some(750)).is_empty());
        let warning = tick(&mut manager, &settings, &process, start, 2, Some(750));
        assert_eq!(warning.len(), 1);
        assert!(!warning[0].critical);
        assert_eq!(warning[0].response.action, ResourceThresholdsAction::Alert);
        assert!(warning[0].notify);
        assert!(tick(&mut manager, &settings, &process, start, 3, Some(950)).is_empty());
        assert!(tick(&mut manager, &settings, &process, start, 4, Some(950)).is_empty());
        let critical = tick(&mut manager, &settings, &process, start, 5, Some(950));
        assert_eq!(critical.len(), 1);
        assert!(critical[0].critical);
        assert_eq!(critical[0].targets[0].creation_time, 10);
        assert_eq!(
            critical[0].response.action,
            ResourceThresholdsAction::Terminate
        );
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
    fn threshold_qualifies_at_equality_and_rearms_only_below_the_recovery_margin() {
        let mut threshold = ThresholdState::default();
        let start = Instant::now();
        let mut observe = |second, value| {
            threshold.observe(
                value,
                100,
                Duration::from_secs(1),
                start + Duration::from_secs(second),
            )
        };

        assert!(!observe(0, 100.0));
        assert!(observe(1, 100.0));
        assert!(!observe(2, 95.0));
        assert!(!observe(3, 100.0));
        assert!(!observe(4, 100.0));
        assert!(!observe(5, 94.9));
        assert!(!observe(6, 100.0));
        assert!(observe(7, 100.0));
    }

    #[test]
    fn missing_samples_restarts_duration_and_replaced_or_protected_processes_cannot_inherit_it() {
        let mut manager = ResourceThresholdsManager::default();
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
        let mut manager = ResourceThresholdsManager::default();
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
        settings.rules[0].memory.warning.action = ResourceThresholdsAction::None;
        assert!(tick(&mut manager, &settings, &process, start, 23, Some(950)).is_empty());
        let mut observations = CycleObservations::default();
        let (status, events) = manager.update(&settings, false, &mut observations);
        assert_eq!(status, ResourceThresholdsSnapshot::default());
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
        settings.rules[0].memory.critical.action = ResourceThresholdsAction::Terminate;
        let mut manager = ResourceThresholdsManager::default();
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
        assert_eq!(events[0].metric, ResourceThresholdsMetric::CpuPercent);
        assert!(!events[0].critical);
        assert_eq!(events[0].response.action, ResourceThresholdsAction::Alert);
        assert_eq!(events[1].metric, ResourceThresholdsMetric::MemoryPercent);
        assert!(events[1].critical);
        assert_eq!(
            events[1].response.action,
            ResourceThresholdsAction::Terminate
        );
        assert!(tick_both(4, None, 95).1.is_empty());
        assert!(tick_both(5, None, 95).1.is_empty());
        let events = tick_both(6, None, 95).1;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].metric, ResourceThresholdsMetric::CpuPercent);
        assert!(events[0].critical);
        assert_eq!(events[0].response.action, ResourceThresholdsAction::Alert);
    }

    #[test]
    fn working_set_uses_physical_memory_and_cpu_requires_an_interval() {
        let now = Instant::now();
        let process = process();
        let mut sample = sample(&process, now, Some(512 * 1_000_000))[&process.id];
        sample.private_commit_bytes = Some(384_500_000);
        assert_eq!(
            combined_metric_value(
                ResourceThresholdsMetric::MemoryPercent,
                &[(&process, &sample)],
                |_, _| None,
                Some(1024 * 1_000_000)
            ),
            Some(50.0)
        );
        assert_eq!(
            combined_metric_value(
                ResourceThresholdsMetric::MemoryPercent,
                &[(&process, &sample)],
                |_, _| None,
                Some(0)
            ),
            None
        );
        assert_eq!(
            combined_metric_value(
                ResourceThresholdsMetric::CpuPercent,
                &[(&process, &sample)],
                |_, _| None,
                None
            ),
            None
        );
        let previous = sample.cpu;
        sample.cpu.sampled_at += Duration::from_secs(1);
        sample.cpu.cpu_time_100ns += 5_000_000;
        let cpus = std::thread::available_parallelism()
            .map(usize::from)
            .unwrap_or(1);
        let mut cpu_usage = process_cpu_usage_batch();
        let actual = combined_metric_value(
            ResourceThresholdsMetric::CpuPercent,
            &[(&process, &sample)],
            |_, sample| cpu_usage(previous, sample.cpu),
            None,
        )
        .unwrap();
        assert!((actual - 50.0 / cpus as f64).abs() < 0.001);
        sample.cpu.cpu_time_100ns = 0;
        assert_eq!(
            combined_metric_value(
                ResourceThresholdsMetric::CpuPercent,
                &[(&process, &sample)],
                |_, sample| cpu_usage(previous, sample.cpu),
                None,
            ),
            None
        );
    }

    #[test]
    fn switching_to_working_set_mb_restarts_the_timer_and_qualifies_at_exact_combined_bytes() {
        let mut settings = settings();
        let mut manager = ResourceThresholdsManager::default();
        let processes: [_; 3] = std::array::from_fn(|offset| {
            let mut member = process();
            member.id += offset as u32;
            member
        });
        let start = Instant::now();
        let mut observe = |settings: &ResourceThresholdsSettings, second, total_memory| {
            let now = start + Duration::from_secs(second);
            // Divide the integer sum to avoid rounding an exact threshold down.
            let samples = processes
                .iter()
                .zip([191_722, 66_664_431, 3_143_847])
                .flat_map(|(process, bytes)| sample(process, now, Some(bytes)))
                .collect();
            manager
                .evaluate(settings, &processes, &samples, total_memory, now)
                .1
        };

        for second in 0..2 {
            assert!(observe(&settings, second, Some(80_000_000)).is_empty());
        }
        settings.rules[0].memory_unit = ResourceThresholdsMemoryUnit::Megabytes;
        assert!(settings.validate().is_ok());
        for second in 2..4 {
            assert!(observe(&settings, second, None).is_empty());
        }
        let events = observe(&settings, 4, None);
        assert_eq!(events.len(), 1);
        let event = &events[0];
        assert_eq!(event.metric, ResourceThresholdsMetric::MemoryMb);
        assert_eq!(event.value, 70.0);
        assert_eq!(event.process_count, 3);
        assert!(!event.critical);
        assert!(observe(&settings, 5, None).is_empty());
    }

    #[test]
    fn disabled_critical_response_does_not_mask_an_enabled_warning() {
        let mut manager = ResourceThresholdsManager::default();
        let mut settings = settings();
        settings.rules[0].memory.warning.action = ResourceThresholdsAction::Terminate;
        settings.rules[0].memory.critical.action = ResourceThresholdsAction::None;
        let process = process();
        let start = Instant::now();
        for second in 0..2 {
            assert!(tick(&mut manager, &settings, &process, start, second, Some(950)).is_empty());
        }
        let events = tick(&mut manager, &settings, &process, start, 2, Some(950));
        assert_eq!(events.len(), 1);
        assert!(!events[0].critical);
        assert!(events[0].notify);
        assert_eq!(
            events[0].response.action,
            ResourceThresholdsAction::Terminate
        );
        settings.rules[0].memory.warning.action = ResourceThresholdsAction::None;
        for second in 3..7 {
            assert!(tick(&mut manager, &settings, &process, start, second, Some(950)).is_empty());
        }
    }

    #[test]
    fn shared_duration_changes_restart_observation_for_every_rule() {
        let mut settings = settings();
        settings.rules[0].memory.critical.action = ResourceThresholdsAction::Terminate;
        let mut other = process();
        other.id += 1;
        other.name = "other.exe".into();
        other.image_path = Some(r"C:\Apps\other.exe".into());
        let mut rule = settings.rules[0].clone();
        rule.selector = ResourceThresholdsSelector::ExactPath {
            executable_path: r"C:\Apps\other.exe".into(),
        };
        settings.rules.push(rule);
        let processes = [process(), other];
        let mut manager = ResourceThresholdsManager::default();
        let start = Instant::now();
        let mut observe = |settings: &ResourceThresholdsSettings, second| {
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
        assert!(observe(&settings, 5).is_empty());
        let events = observe(&settings, 6);
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].targets[0].id, processes[0].id);
        assert_eq!(events[1].targets[0].id, processes[1].id);
    }

    #[test]
    fn memory_thresholds_use_one_combined_app_total_and_capture_only_matching_members() {
        let mut settings = settings();
        settings.rules[0].memory.critical.action = ResourceThresholdsAction::Terminate;
        let mut second = process();
        second.id += 1;
        second.creation_time = Some(20);
        let mut unrelated = process();
        unrelated.id += 2;
        unrelated.image_path = Some(r"C:\Other\test.exe".into());
        let processes = [process(), second, unrelated];
        let mut manager = ResourceThresholdsManager::default();
        let start = Instant::now();
        let mut observe = |second, memory: u64| {
            let now = start + Duration::from_secs(second);
            let samples = processes
                .iter()
                .flat_map(|p| sample(p, now, Some(memory * 1_000_000)))
                .collect();
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
        assert_eq!(events[0].response.action, ResourceThresholdsAction::Alert);
        assert!(events[0].targets.is_empty());
        assert!(observe(3, 48).1.is_empty());
        assert!(observe(4, 48).1.is_empty());
        let (_, events) = observe(5, 48);
        assert_eq!(events.len(), 1);
        assert!(events[0].critical);
        assert_eq!(events[0].value, 96.0);
        assert_eq!(
            events[0]
                .targets
                .iter()
                .map(|t| (t.id, t.creation_time))
                .collect::<Vec<_>>(),
            vec![(processes[0].id, 10), (processes[1].id, 20)]
        );
        assert_eq!(
            events[0].response.action,
            ResourceThresholdsAction::Terminate
        );
        assert!(observe(6, 48).1.is_empty());
    }

    #[test]
    fn exact_name_alerts_track_separate_paths_and_require_complete_group_samples() {
        let mut settings = settings();
        settings.rules[0].selector = ResourceThresholdsSelector::ExactName {
            executable_name: "TEST.EXE".into(),
        };
        let mut second = process();
        second.id += 1;
        second.image_path = Some(r"c:\apps\TEST.EXE".into());
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
        let mut manager = ResourceThresholdsManager::default();
        let start = Instant::now();
        let mut observe = |second, first, other| {
            let now = start + Duration::from_secs(second);
            let mut samples: BTreeMap<_, _> = processes
                .iter()
                .zip([first, first, other, 100, 100])
                .flat_map(|(process, bytes)| sample(process, now, Some(bytes)))
                .collect();
            if second == 6 {
                samples.get_mut(&processes[1].id).unwrap().creation_time += 1;
            }
            manager.evaluate(&settings, &processes, &samples, Some(100), now)
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
    fn name_rules_can_alert_and_cannot_dispatch_process_actions() {
        let mut settings = settings();
        settings.rules[0].selector = ResourceThresholdsSelector::ExactName {
            executable_name: "test.exe".into(),
        };
        settings.rules[0].memory.critical.action = ResourceThresholdsAction::Restart;
        let mut manager = ResourceThresholdsManager::default();
        let process = process();
        let start = Instant::now();
        assert!(tick(&mut manager, &settings, &process, start, 0, Some(950)).is_empty());
        assert!(tick(&mut manager, &settings, &process, start, 1, Some(950)).is_empty());
        let events = tick(&mut manager, &settings, &process, start, 2, Some(950));
        assert_eq!(events.len(), 1);
        assert!(events[0].notify && !events[0].critical);
        assert_eq!(events[0].response.action, ResourceThresholdsAction::Alert);
        assert!(events[0].targets.is_empty());

        settings.notify = false;
        for second in 3..=5 {
            assert!(tick(&mut manager, &settings, &process, start, second, Some(950)).is_empty());
        }
    }

    #[test]
    fn name_pattern_alerts_track_each_verified_path_without_dispatching_actions() {
        let mut settings = settings();
        settings.rules[0].selector = ResourceThresholdsSelector::NamePattern {
            executable_name_pattern: ExecutableNamePattern::try_from("test*.exe".to_owned())
                .unwrap(),
        };
        settings.rules[0].memory.critical.action = ResourceThresholdsAction::Terminate;
        let processes = [
            (0, "TEST.EXE", r"C:\Apps\test.exe"),
            (1, "test-extra.exe", r"C:\Other\test-extra.exe"),
            (2, "test-stale.exe", r"C:\Other\test-extra.exe"),
        ]
        .map(|(offset, name, path)| {
            let mut process = process();
            process.id += offset;
            process.name = name.into();
            process.image_path = Some(path.into());
            process
        });
        let mut manager = ResourceThresholdsManager::default();
        let start = Instant::now();
        let mut observe = |second| {
            let now = start + Duration::from_secs(second);
            let samples = processes
                .iter()
                .flat_map(|process| sample(process, now, Some(95)))
                .collect();
            manager.evaluate(&settings, &processes, &samples, Some(100), now)
        };

        assert!(observe(0).1.is_empty());
        assert!(observe(1).1.is_empty());
        let (snapshot, events) = observe(2);
        assert_eq!(snapshot.monitored_processes, 2);
        assert_eq!(snapshot.alerting_processes, 2);
        assert_eq!(
            events
                .iter()
                .map(|event| event.name.as_str())
                .collect::<Vec<_>>(),
            [r"C:\Apps\test.exe", r"C:\Other\test-extra.exe"]
        );
        assert!(events.iter().all(|event| {
            event.response.action == ResourceThresholdsAction::Alert
                && event.notify
                && event.process_count == 1
                && event.targets.is_empty()
        }));
    }

    #[test]
    fn cpu_totals_require_complete_member_intervals_and_do_not_inherit_restarted_apps() {
        let mut settings = settings();
        settings.rules[0].memory.enabled = false;
        settings.rules[0].cpu.enabled = true;
        let mut second = process();
        second.id += 1;
        let mut processes = vec![process(), second];
        let mut manager = ResourceThresholdsManager::default();
        let start = Instant::now();
        let capacity = std::thread::available_parallelism()
            .map(usize::from)
            .unwrap_or(1) as u64;
        let observe =
            |manager: &mut ResourceThresholdsManager, processes: &[ProcessInfo], second| {
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
        enum Gap {
            Memory,
            Sample,
            CreationTime,
        }

        for gap in [Gap::Memory, Gap::Sample, Gap::CreationTime] {
            let settings = settings();
            let mut second = process();
            second.id += 1;
            let complete = [process(), second];
            let mut missing_creation = complete.clone();
            missing_creation[1].creation_time = None;
            let mut manager = ResourceThresholdsManager::default();
            let start = Instant::now();
            for second in 0..5 {
                let now = start + Duration::from_secs(second);
                let missing_creation_time = second == 1 && matches!(gap, Gap::CreationTime);
                let processes = if missing_creation_time {
                    &missing_creation
                } else {
                    &complete
                };
                let samples = processes
                    .iter()
                    .enumerate()
                    .flat_map(|(i, process)| {
                        if second != 1 || i == 0 {
                            return sample(process, now, Some(400));
                        }
                        match gap {
                            Gap::Memory => sample(process, now, None),
                            Gap::Sample | Gap::CreationTime => BTreeMap::new(),
                        }
                    })
                    .collect();
                let (snapshot, events) =
                    manager.evaluate(&settings, processes, &samples, Some(1000), now);
                if missing_creation_time {
                    assert_eq!(
                        (snapshot.monitored_processes, snapshot.skipped_processes),
                        (0, 2)
                    );
                }
                assert_eq!(events.len(), usize::from(second == 4));
            }
        }
    }

    #[test]
    fn unknown_criticality_breaks_the_combined_sustained_interval() {
        let settings = settings();
        let mut processes = [process(), process()];
        processes[1].id += 1;
        let mut manager = ResourceThresholdsManager::default();
        let start = Instant::now();
        for second in 0..5 {
            processes[1].is_critical = (second != 1).then_some(false);
            let now = start + Duration::from_secs(second);
            let samples = processes
                .iter()
                .flat_map(|process| sample(process, now, Some(400)))
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
        let mut settings = settings();
        settings.rules[0].memory.critical.action = ResourceThresholdsAction::Terminate;
        let mut manager = ResourceThresholdsManager::default();
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

    #[test]
    fn unreadable_path_interrupts_an_exact_members_combined_interval() {
        let mut settings = settings();
        let rule = &mut settings.rules[0];
        rule.selector = ResourceThresholdsSelector::ExactName {
            executable_name: "test.exe".into(),
        };
        let mut processes = [process(), process()];
        processes[1].id += 1;
        let start = Instant::now();
        let mut manager = ResourceThresholdsManager::default();

        for second in 0..7 {
            let now = start + Duration::from_secs(second);
            let samples = sample(&processes[0], now, Some(800));
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
                samples.extend(sample(&processes[1], now, Some(0)));
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
            assert_eq!(events.len(), usize::from(second == 6));
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
                        .args(["/d", "/q", "/c", "set /p resource_thresholds_wait="])
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
        let mut rule = ResourceThresholdsRule::new(
            process
                .image_path
                .as_ref()
                .unwrap()
                .to_string_lossy()
                .into_owned(),
        );
        rule.cpu.enabled = false;
        rule.memory.enabled = true;
        rule.memory.warning.threshold = 1;
        rule.memory.critical.threshold = 2;
        rule.memory.critical.action = ResourceThresholdsAction::Terminate;
        let settings = ResourceThresholdsSettings {
            enabled: true,
            sustained_seconds: 1,
            notify: false,
            rules: vec![rule],
        };
        let mut manager = ResourceThresholdsManager::default();
        let samples = sample_process_resources(&processes, ProcessResourceSampling::CpuAndMemory);
        // Use a fixed test capacity derived from these children to exercise real
        // working-set samples without depending on the host's installed RAM.
        let total_memory = Some(
            samples
                .values()
                .map(|sample| sample.working_set_bytes.unwrap())
                .sum(),
        );
        let now = Instant::now();
        assert!(manager
            .evaluate(&settings, &processes, &samples, total_memory, now)
            .1
            .is_empty());
        std::thread::sleep(Duration::from_millis(1100));
        let samples = sample_process_resources(&processes, ProcessResourceSampling::CpuAndMemory);
        let (_, events) = manager.evaluate(
            &settings,
            &processes,
            &samples,
            total_memory,
            Instant::now(),
        );
        assert_eq!(events.len(), 1);
        let event = events.into_iter().next().unwrap();
        assert!(event.critical);
        assert!(!event.notify);
        assert_eq!(event.response.action, ResourceThresholdsAction::Terminate);
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
