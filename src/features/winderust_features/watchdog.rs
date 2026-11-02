use std::{
    collections::BTreeMap,
    time::{Duration, Instant},
};

use crate::{
    config::{WatchdogMetric, WatchdogResource, WatchdogResponse, WatchdogSettings},
    cpu::{process_cpu_usage_percent, ProcessCpuSample},
    foreground::{
        process_matches_executable_path, sample_process_resources, ProcessActionTarget,
        ProcessInfo, ProcessResourceSample,
    },
    runtime::observations::CycleObservations,
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct WatchdogSnapshot {
    pub monitored_processes: usize,
    pub alerting_processes: usize,
    pub skipped_processes: usize,
    pub last_error: Option<String>,
}

pub(crate) struct WatchdogEvent {
    pub name: String,
    pub targets: Vec<ProcessActionTarget>,
    pub critical: bool,
    pub metric: WatchdogMetric,
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
    fn observe(&mut self, value: f64, threshold: u64, delay: Duration, now: Instant) -> bool {
        if value < threshold as f64 {
            self.since = None;
            // A small recovery margin prevents repeated alerts around the threshold.
            if value < threshold as f64 * 0.95 {
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

#[derive(Default)]
struct AppState {
    cpu: BTreeMap<(u32, u64), ProcessCpuSample>,
    last_sample: Option<Instant>,
    resources: [ResourceState; 2],
}

#[derive(Default)]
pub(crate) struct WatchdogManager {
    settings: WatchdogSettings,
    states: BTreeMap<usize, AppState>,
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
                self.states.clear();
                return (
                    WatchdogSnapshot {
                        last_error: Some(error),
                        ..Default::default()
                    },
                    Vec::new(),
                );
            }
        };
        let matched: Vec<_> = processes
            .iter()
            .filter(|process| {
                settings.rules.iter().any(|rule| {
                    rule.enabled && process_matches_executable_path(process, &rule.executable_path)
                })
            })
            .cloned()
            .collect();
        let samples = sample_process_resources(&matched);
        let total_memory = settings
            .rules
            .iter()
            .any(|rule| {
                rule.enabled
                    && rule.memory.enabled
                    && rule.memory_unit.metric() == WatchdogMetric::MemoryPercent
            })
            .then(|| crate::backend::dashboard_metrics::sample_memory_usage().total_physical_bytes)
            .flatten();
        self.evaluate(settings, &matched, &samples, total_memory, Instant::now())
    }

    fn evaluate(
        &mut self,
        settings: &WatchdogSettings,
        processes: &[ProcessInfo],
        samples: &BTreeMap<u32, ProcessResourceSample>,
        total_memory: Option<u64>,
        now: Instant,
    ) -> (WatchdogSnapshot, Vec<WatchdogEvent>) {
        if self.settings != *settings {
            self.states.clear();
            self.settings.clone_from(settings);
        }
        let mut states = BTreeMap::new();
        let mut snapshot = WatchdogSnapshot::default();
        let mut events = Vec::new();
        for (index, rule) in settings
            .rules
            .iter()
            .enumerate()
            .filter(|(_, rule)| rule.enabled)
        {
            let mut matched = Vec::new();
            let mut complete = true;
            for process in processes
                .iter()
                .filter(|process| process_matches_executable_path(process, &rule.executable_path))
            {
                if process.is_critical != Some(false) || process.id == std::process::id() {
                    snapshot.skipped_processes += 1;
                    continue;
                }
                let Some(sample) = samples
                    .get(&process.id)
                    .filter(|sample| Some(sample.creation_time) == process.creation_time)
                else {
                    complete = false;
                    snapshot.skipped_processes += 1;
                    continue;
                };
                matched.push((process, sample));
            }
            if !complete {
                snapshot.skipped_processes += matched.len();
                continue;
            }
            let Some((first, _)) = matched.first() else {
                continue;
            };
            let mut state = self.states.remove(&index).unwrap_or_default();
            if state
                .last_sample
                .is_some_and(|last| now.saturating_duration_since(last) > Duration::from_secs(3))
                || !matched.iter().any(|(process, sample)| {
                    state.cpu.contains_key(&(process.id, sample.creation_time))
                })
            {
                // A restarted app cannot inherit the previous app's sustained interval.
                state = AppState::default();
            }
            snapshot.monitored_processes += matched.len();
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
                // Every member needs a valid sample; never evaluate a partial app total.
                let value = matched.iter().try_fold(0.0, |total, (process, sample)| {
                    metric_value(
                        metric,
                        state.cpu.get(&(process.id, sample.creation_time)).copied(),
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
                    let delay = Duration::from_secs(settings.sustained_seconds);
                    let critical = (settings.notify
                        || monitor.critical.action != crate::config::WatchdogAction::None)
                        && thresholds.critical.observe(
                            value,
                            monitor.critical.threshold,
                            delay,
                            now,
                        );
                    let warning = (settings.notify
                        || monitor.warning.action != crate::config::WatchdogAction::None)
                        && thresholds
                            .warning
                            .observe(value, monitor.warning.threshold, delay, now);
                    if critical || warning {
                        if critical {
                            thresholds.warning.fired = true;
                        }
                        events.push(WatchdogEvent {
                            name: first.name.clone(),
                            targets: matched
                                .iter()
                                .map(|(process, sample)| ProcessActionTarget {
                                    id: process.id,
                                    name: process.name.clone(),
                                    executable_path: std::path::PathBuf::from(
                                        &rule.executable_path,
                                    ),
                                    creation_time: sample.creation_time,
                                    session_id: process.session_id,
                                    is_service_account: process.is_service_account,
                                })
                                .collect(),
                            critical,
                            metric,
                            value,
                            notify: settings.notify,
                            response: if critical {
                                monitor.critical.clone()
                            } else {
                                monitor.warning.clone()
                            },
                        });
                    }
                    alerting |= thresholds.warning.fired || thresholds.critical.fired;
                } else {
                    thresholds.warning.since = None;
                    thresholds.critical.since = None;
                    skipped = true;
                }
            }
            state.cpu = matched
                .iter()
                .map(|(process, sample)| ((process.id, sample.creation_time), sample.cpu))
                .collect();
            state.last_sample = Some(now);
            snapshot.alerting_processes += if alerting { matched.len() } else { 0 };
            snapshot.skipped_processes += if skipped { matched.len() } else { 0 };
            states.insert(index, state);
        }
        self.states = states;
        (snapshot, events)
    }
}

fn metric_value(
    metric: WatchdogMetric,
    previous: Option<ProcessCpuSample>,
    current: &ProcessResourceSample,
    total_memory: Option<u64>,
) -> Option<f64> {
    match metric {
        WatchdogMetric::CpuPercent => previous
            .and_then(|previous| process_cpu_usage_percent(previous, current.cpu))
            .map(f64::from),
        WatchdogMetric::MemoryPercent => current
            .working_set_bytes
            .zip(total_memory.filter(|total| *total > 0))
            .map(|(bytes, total)| bytes as f64 / total as f64 * 100.0),
        WatchdogMetric::MemoryMib => current
            .working_set_bytes
            .map(|bytes| bytes as f64 / 1_048_576.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{WatchdogAction, WatchdogRule};

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
        settings.notify = false;
        assert!(tick(&mut manager, &settings, &process, start, 12, Some(950)).is_empty());
        let mut observations = CycleObservations::default();
        let (status, events) = manager.update(&settings, false, &mut observations);
        assert_eq!(status, WatchdogSnapshot::default());
        assert!(events.is_empty());
        assert!(manager.states.is_empty());
        assert!(tick(&mut manager, &settings, &process, start, 14, Some(950)).is_empty());
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
    fn units_use_real_memory_and_cpu_requires_an_interval() {
        let now = Instant::now();
        let process = process();
        let mut sample = sample(&process, now, Some(512 * 1_048_576))[&process.id];
        assert_eq!(
            metric_value(WatchdogMetric::MemoryMib, None, &sample, None),
            Some(512.0)
        );
        assert_eq!(
            metric_value(
                WatchdogMetric::MemoryPercent,
                None,
                &sample,
                Some(1024 * 1_048_576)
            ),
            Some(50.0)
        );
        assert_eq!(
            metric_value(WatchdogMetric::MemoryPercent, None, &sample, Some(0)),
            None
        );
        assert_eq!(
            metric_value(WatchdogMetric::CpuPercent, None, &sample, None),
            None
        );
        let previous = sample.cpu;
        sample.cpu.sampled_at += Duration::from_secs(1);
        sample.cpu.cpu_time_100ns += 5_000_000;
        let cpus = std::thread::available_parallelism()
            .map(usize::from)
            .unwrap_or(1);
        let actual =
            metric_value(WatchdogMetric::CpuPercent, Some(previous), &sample, None).unwrap();
        assert!((actual - 50.0 / cpus as f64).abs() < 0.001);
        sample.cpu.cpu_time_100ns = 0;
        assert_eq!(
            metric_value(WatchdogMetric::CpuPercent, Some(previous), &sample, None),
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
    fn shared_duration_and_notifications_apply_to_all_rules_and_restart_observation() {
        let mut settings = settings();
        settings.rules[0].memory.critical.action = WatchdogAction::Terminate;
        let mut other = process();
        other.id += 1;
        other.name = "other.exe".into();
        other.image_path = Some(r"C:\Apps\other.exe".into());
        let mut rule = settings.rules[0].clone();
        rule.executable_path = other
            .image_path
            .as_ref()
            .unwrap()
            .to_string_lossy()
            .into_owned();
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
        assert!(observe(&settings, 5).is_empty());
        assert_eq!(observe(&settings, 6).len(), 2);
        settings.notify = false;
        assert!(observe(&settings, 7).is_empty());
        assert!(observe(&settings, 9).is_empty());
        let events = observe(&settings, 10);
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
                let samples = processes
                    .iter()
                    .flat_map(|p| sample(p, now, Some(memory * 1_048_576)))
                    .collect();
                manager.evaluate(&settings, &processes, &samples, Some(100 * 1_048_576), now)
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
        rule.memory_unit = crate::config::WatchdogMemoryUnit::Mib;
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
        let samples = sample_process_resources(&processes);
        let now = Instant::now();
        assert!(manager
            .evaluate(&settings, &processes, &samples, None, now)
            .1
            .is_empty());
        std::thread::sleep(Duration::from_millis(1100));
        let samples = sample_process_resources(&processes);
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
            .into_process_list_result()
            .unwrap();
        for child in &mut children {
            assert!(!child.0.wait().unwrap().success());
        }
    }
}
