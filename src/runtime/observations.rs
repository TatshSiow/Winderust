use std::{
    borrow::Cow,
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::Arc,
    time::Instant,
};

use crate::{
    cpu::{process_cpu_usage_percent, ProcessCpuSample},
    foreground::{
        active_window::{process_from_id, ForegroundProcess},
        foreground_process_id,
        process_list::{
            enrich_process_paths, list_process_ancestry, list_process_entries,
            observe_process_metadata, process_cpu_times,
        },
        top_level_window_process_ids, visible_window_process_ids, ProcessInfo,
    },
};

pub(crate) type AdaptiveWorkload = Arc<BTreeMap<u32, u64>>;

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct ForegroundCpuHistory(Option<(AdaptiveWorkload, ProcessCpuSample)>);

impl ForegroundCpuHistory {
    pub(crate) fn update(
        &mut self,
        workload: AdaptiveWorkload,
        sample: Option<ProcessCpuSample>,
    ) -> Option<f32> {
        let previous = std::mem::replace(&mut self.0, sample.map(|sample| (workload, sample)));
        let (previous_workload, previous) = previous?;
        let (workload, current) = self.0.as_ref()?;
        (&previous_workload == workload)
            .then(|| process_cpu_usage_percent(previous, *current))
            .flatten()
    }
}

fn sample_workload(
    workload: &BTreeMap<u32, u64>,
    mut times: impl FnMut(u32) -> Option<(u64, u64)>,
) -> Option<ProcessCpuSample> {
    if workload.is_empty() {
        return None;
    }
    let sampled_at = Instant::now();
    let mut cpu_time_100ns = 0u64;
    for (&id, &expected_creation) in workload {
        let (creation, ticks) = times(id)?;
        if creation != expected_creation {
            return None;
        }
        cpu_time_100ns = cpu_time_100ns.checked_add(ticks)?;
    }
    Some(ProcessCpuSample {
        cpu_time_100ns,
        sampled_at,
    })
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ObservationAvailability {
    NotRequested,
    Available,
    Unavailable,
}

#[derive(Debug)]
enum Observation<T> {
    NotRequested,
    Available(T),
    Unavailable(String),
}

#[cfg(test)]
impl<T> Observation<T> {
    fn availability(&self) -> ObservationAvailability {
        match self {
            Self::NotRequested => ObservationAvailability::NotRequested,
            Self::Available(_) => ObservationAvailability::Available,
            Self::Unavailable(_) => ObservationAvailability::Unavailable,
        }
    }
}

#[derive(Clone, Copy)]
struct ObservationSources {
    process_entries: fn() -> Result<Vec<ProcessInfo>, String>,
    process_metadata: fn(&mut ProcessInfo),
    enrich_process_paths: fn(&mut [ProcessInfo]),
    foreground_process_id: fn() -> Option<u32>,
    process_from_id: fn(u32) -> Option<ForegroundProcess>,
    visible_window_process_ids: fn() -> Option<BTreeSet<u32>>,
    top_level_window_process_ids: fn() -> BTreeSet<u32>,
    cpu_topology: fn() -> Result<crate::cpu_topology::CpuTopology, String>,
    service_process_ids: fn() -> Result<BTreeSet<u32>, String>,
}

impl Default for ObservationSources {
    fn default() -> Self {
        Self {
            process_entries: list_process_entries,
            process_metadata: |process| drop(observe_process_metadata(process)),
            enrich_process_paths,
            foreground_process_id,
            process_from_id,
            visible_window_process_ids,
            top_level_window_process_ids,
            cpu_topology: crate::features::cpu_control::cpu_allocation::processor_topology,
            service_process_ids:
                crate::platform::windows::service_hosts::active_service_process_ids,
        }
    }
}

/// Read-side Windows observations shared only within one runtime reconciliation pass.
///
/// Cached observations select policy targets but never authorize a mutation. Feature managers
/// still reopen and revalidate each process immediately before applying a Windows change.
pub(crate) struct CycleObservations {
    sources: ObservationSources,
    processes: Observation<Arc<[ProcessInfo]>>,
    // Remember the first metadata attempt per PID until the full catalog is filled.
    partial_process_metadata: Option<BTreeSet<u32>>,
    process_paths_enriched: bool,
    foreground_ancestry: Observation<Arc<[ProcessInfo]>>,
    adaptive_workload: Option<AdaptiveWorkload>,
    adaptive_cpu_sample: Option<Option<ProcessCpuSample>>,
    foreground_process_id: Observation<Option<u32>>,
    foreground_process: Observation<Option<ForegroundProcess>>,
    visible_window_process_ids: Observation<Arc<BTreeSet<u32>>>,
    top_level_window_process_ids: Observation<Arc<BTreeSet<u32>>>,
    cpu_topology: Observation<Arc<crate::cpu_topology::CpuTopology>>,
}

fn enrich_observed_paths(records: &mut [ProcessInfo], enrich: fn(&mut [ProcessInfo])) {
    let generations: Vec<_> = records
        .iter()
        .map(|process| process.creation_time)
        .collect();
    enrich(records);
    for (process, captured) in records.iter_mut().zip(generations) {
        if process.creation_time != captured {
            // Enrichment cannot promote a new identity into an already-observed cycle.
            // Defer this target until a fresh process observation establishes its generation.
            process.creation_time = captured;
            process.image_path = None;
            process.user_name = None;
            process.is_service_account = None;
            process.is_critical = None;
            process.can_set_information = false;
        }
    }
}

impl Default for CycleObservations {
    fn default() -> Self {
        Self::with_sources(ObservationSources::default())
    }
}

impl CycleObservations {
    pub(crate) fn service_process_ids(&self) -> Result<BTreeSet<u32>, String> {
        (self.sources.service_process_ids)()
    }

    fn with_sources(sources: ObservationSources) -> Self {
        Self {
            sources,
            processes: Observation::NotRequested,
            partial_process_metadata: Some(BTreeSet::new()),
            process_paths_enriched: false,
            foreground_ancestry: Observation::NotRequested,
            adaptive_workload: None,
            adaptive_cpu_sample: None,
            foreground_process_id: Observation::NotRequested,
            foreground_process: Observation::NotRequested,
            visible_window_process_ids: Observation::NotRequested,
            top_level_window_process_ids: Observation::NotRequested,
            cpu_topology: Observation::NotRequested,
        }
    }

    #[cfg(test)]
    pub(crate) fn with_processes(processes: Vec<ProcessInfo>) -> Self {
        Self {
            processes: Observation::Available(Arc::from(processes)),
            partial_process_metadata: None,
            ..Self::default()
        }
    }

    #[cfg(test)]
    pub(crate) fn with_foreground_source(mut self, source: fn() -> Option<u32>) -> Self {
        self.sources.foreground_process_id = source;
        self
    }

    #[cfg(test)]
    pub(crate) fn with_adaptive_cpu_sample(
        processes: Vec<ProcessInfo>,
        foreground_id: Option<u32>,
        sample: Option<ProcessCpuSample>,
    ) -> Self {
        Self {
            foreground_process_id: Observation::Available(foreground_id),
            adaptive_cpu_sample: Some(sample),
            ..Self::with_processes(processes)
        }
    }

    #[cfg(test)]
    pub(crate) fn with_visible_windows(mut self, process_ids: BTreeSet<u32>) -> Self {
        self.visible_window_process_ids = Observation::Available(Arc::new(process_ids));
        self
    }

    #[cfg(test)]
    pub(crate) fn with_visible_window_source(
        mut self,
        source: fn() -> Option<BTreeSet<u32>>,
    ) -> Self {
        self.sources.visible_window_process_ids = source;
        self
    }

    pub(crate) fn cpu_topology(&mut self) -> Result<Arc<crate::cpu_topology::CpuTopology>, String> {
        if matches!(self.cpu_topology, Observation::NotRequested) {
            self.cpu_topology = match (self.sources.cpu_topology)() {
                Ok(topology) => Observation::Available(Arc::new(topology)),
                Err(error) => Observation::Unavailable(error),
            };
        }
        match &self.cpu_topology {
            Observation::Available(topology) => Ok(Arc::clone(topology)),
            Observation::Unavailable(error) => Err(error.clone()),
            Observation::NotRequested => unreachable!("CPU topology observation was initialized"),
        }
    }

    pub(crate) fn adaptive_workload(&mut self, processes: &[ProcessInfo]) -> AdaptiveWorkload {
        let root = self.foreground_process_id();
        Arc::clone(self.adaptive_workload.get_or_insert_with(|| {
            let ids = foreground_process_group_ids(processes, root);
            Arc::new(
                processes
                    .iter()
                    .filter(|p| ids.contains(&p.id))
                    .filter_map(|p| p.creation_time.map(|creation| (p.id, creation)))
                    .collect(),
            )
        }))
    }

    pub(crate) fn adaptive_cpu_sample(
        &mut self,
        workload: &BTreeMap<u32, u64>,
    ) -> Option<ProcessCpuSample> {
        *self
            .adaptive_cpu_sample
            .get_or_insert_with(|| sample_workload(workload, process_cpu_times))
    }

    pub(crate) fn foreground_process_ancestry(
        &mut self,
        foreground_id: u32,
    ) -> Result<Arc<[ProcessInfo]>, String> {
        if !matches!(self.processes, Observation::NotRequested) {
            return self.processes();
        }
        if matches!(self.foreground_ancestry, Observation::NotRequested) {
            self.foreground_ancestry = match list_process_ancestry(foreground_id) {
                Ok(processes) => Observation::Available(Arc::from(processes)),
                Err(error) => Observation::Unavailable(error),
            };
        }
        match &self.foreground_ancestry {
            Observation::Available(processes) => Ok(Arc::clone(processes)),
            Observation::Unavailable(error) => Err(error.clone()),
            Observation::NotRequested => unreachable!("foreground ancestry was just observed"),
        }
    }

    fn ensure_process_entries(&mut self) -> Result<(), String> {
        if matches!(self.processes, Observation::NotRequested) {
            self.processes = match (self.sources.process_entries)() {
                Ok(processes) => Observation::Available(Arc::from(processes)),
                Err(error) => Observation::Unavailable(error),
            };
        }

        match &self.processes {
            Observation::Available(_) => Ok(()),
            Observation::Unavailable(error) => Err(error.clone()),
            Observation::NotRequested => unreachable!("process observation was just requested"),
        }
    }

    pub(crate) fn processes(&mut self) -> Result<Arc<[ProcessInfo]>, String> {
        self.ensure_process_entries()?;
        let Observation::Available(processes) = &mut self.processes else {
            unreachable!("process observation just succeeded")
        };
        if let Some(observed) = self.partial_process_metadata.take() {
            for process in Arc::make_mut(processes) {
                if !observed.contains(&process.id) {
                    (self.sources.process_metadata)(process);
                }
            }
        }
        Ok(Arc::clone(processes))
    }

    pub(crate) fn processes_with_paths(&mut self) -> Result<Arc<[ProcessInfo]>, String> {
        drop(self.processes()?);
        let Observation::Available(processes) = &mut self.processes else {
            unreachable!("process observation just succeeded")
        };
        if !self.process_paths_enriched {
            enrich_observed_paths(Arc::make_mut(processes), self.sources.enrich_process_paths);
            self.process_paths_enriched = true;
        }
        Ok(Arc::clone(processes))
    }

    /// Selects candidates using snapshot names and PIDs before reading metadata and paths.
    pub(crate) fn processes_for_path_matching(
        &mut self,
        mut is_candidate: impl FnMut(&ProcessInfo) -> bool,
    ) -> Result<Cow<'_, [ProcessInfo]>, String> {
        self.ensure_process_entries()?;
        let Observation::Available(processes) = &mut self.processes else {
            unreachable!("process observation just succeeded")
        };
        if self.process_paths_enriched {
            return Ok(Cow::Borrowed(processes));
        }

        let mut candidates: Vec<_> = if let Some(observed) = &mut self.partial_process_metadata {
            let mut candidates = Vec::new();
            for process in Arc::make_mut(processes) {
                if is_candidate(process) {
                    if observed.insert(process.id) {
                        (self.sources.process_metadata)(process);
                    }
                    candidates.push(process.clone());
                }
            }
            candidates
        } else {
            processes
                .iter()
                .filter(|process| is_candidate(process))
                .cloned()
                .collect()
        };
        enrich_observed_paths(&mut candidates, self.sources.enrich_process_paths);
        Ok(Cow::Owned(candidates))
    }

    pub(crate) fn foreground_process_id(&mut self) -> Option<u32> {
        if matches!(self.foreground_process_id, Observation::NotRequested) {
            self.foreground_process_id =
                Observation::Available((self.sources.foreground_process_id)());
        }

        match self.foreground_process_id {
            Observation::Available(process_id) => process_id,
            Observation::Unavailable(_) | Observation::NotRequested => {
                unreachable!("foreground process ID collection cannot report an error")
            }
        }
    }

    pub(crate) fn foreground_process(&mut self) -> Option<ForegroundProcess> {
        if matches!(self.foreground_process, Observation::NotRequested) {
            let process = self
                .foreground_process_id()
                .and_then(self.sources.process_from_id);
            self.foreground_process = Observation::Available(process);
        }

        match &self.foreground_process {
            Observation::Available(process) => process.clone(),
            Observation::Unavailable(_) | Observation::NotRequested => {
                unreachable!("foreground process collection cannot report an error")
            }
        }
    }

    pub(crate) fn noncritical_foreground_process(&mut self) -> Option<ForegroundProcess> {
        self.foreground_process()
            .filter(|process| process.is_critical == Some(false))
    }

    pub(crate) fn cached_foreground_executable_path(&self) -> Option<&Path> {
        match &self.foreground_process {
            Observation::Available(Some(process)) if process.executable_path.is_absolute() => {
                Some(&process.executable_path)
            }
            _ => None,
        }
    }

    pub(crate) fn visible_window_process_ids(&mut self) -> Result<Arc<BTreeSet<u32>>, String> {
        if matches!(self.visible_window_process_ids, Observation::NotRequested) {
            self.visible_window_process_ids = match (self.sources.visible_window_process_ids)() {
                Some(process_ids) => Observation::Available(Arc::new(process_ids)),
                None => Observation::Unavailable("Visible windows are unavailable.".to_owned()),
            };
        }

        match &self.visible_window_process_ids {
            Observation::Available(process_ids) => Ok(Arc::clone(process_ids)),
            Observation::Unavailable(error) => Err(error.clone()),
            Observation::NotRequested => {
                unreachable!("visible-window observation was just requested")
            }
        }
    }

    pub(crate) fn top_level_window_process_ids(&mut self) -> Arc<BTreeSet<u32>> {
        if matches!(self.top_level_window_process_ids, Observation::NotRequested) {
            self.top_level_window_process_ids =
                Observation::Available(Arc::new((self.sources.top_level_window_process_ids)()));
        }

        match &self.top_level_window_process_ids {
            Observation::Available(process_ids) => Arc::clone(process_ids),
            Observation::Unavailable(_) | Observation::NotRequested => {
                unreachable!("top-level window collection cannot report an error")
            }
        }
    }

    #[cfg(test)]
    fn availability(&self) -> ObservationAvailabilitySnapshot {
        ObservationAvailabilitySnapshot {
            processes: self.processes.availability(),
            foreground_process_id: self.foreground_process_id.availability(),
            foreground_process: self.foreground_process.availability(),
            visible_window_process_ids: self.visible_window_process_ids.availability(),
            top_level_window_process_ids: self.top_level_window_process_ids.availability(),
        }
    }
}

#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ObservationAvailabilitySnapshot {
    processes: ObservationAvailability,
    foreground_process_id: ObservationAvailability,
    foreground_process: ObservationAvailability,
    visible_window_process_ids: ObservationAvailability,
    top_level_window_process_ids: ObservationAvailability,
}

/// Unknown or stale ancestry does not establish membership in the current workload.
pub(crate) fn foreground_process_group_ids(
    processes: &[ProcessInfo],
    root: Option<u32>,
) -> BTreeSet<u32> {
    let Some(root) = root else {
        return BTreeSet::new();
    };
    let Some(root_created) = processes
        .iter()
        .rev()
        .find(|process| process.id == root)
        .and_then(|process| process.creation_time)
    else {
        return BTreeSet::new();
    };
    if !processes.iter().any(|child| {
        child.parent_id == Some(root)
            && child.id != root
            && child
                .creation_time
                .is_some_and(|created| created >= root_created)
    }) {
        return BTreeSet::from([root]);
    }
    let by_id = processes
        .iter()
        .map(|process| (process.id, process))
        .collect::<std::collections::BTreeMap<_, _>>();
    let valid_generation = |child: &ProcessInfo, parent_created| {
        child
            .creation_time
            .zip(parent_created)
            .is_some_and(|(child, parent)| child >= parent)
    };
    let mut group = BTreeSet::from([root]);
    let mut pending = vec![root];
    // Scanning small families costs less than building the complete ancestry index.
    for _ in 0..16 {
        let Some(parent) = pending.pop() else {
            return group;
        };
        let parent_created = by_id[&parent].creation_time;
        for child in processes {
            if child.parent_id == Some(parent)
                && valid_generation(child, parent_created)
                && group.insert(child.id)
            {
                pending.push(child.id);
            }
        }
    }
    if pending.is_empty() {
        return group;
    }
    let mut children = processes
        .iter()
        .filter_map(|process| process.parent_id.map(|parent| (parent, process)))
        .collect::<Vec<_>>();
    children.sort_by_key(|(parent, _)| *parent);
    while let Some(parent) = pending.pop() {
        let parent_created = by_id[&parent].creation_time;
        let first = children.partition_point(|(parent_id, _)| *parent_id < parent);
        for (_, child) in children[first..]
            .iter()
            .take_while(|(parent_id, _)| *parent_id == parent)
        {
            if valid_generation(child, parent_created) && group.insert(child.id) {
                pending.push(child.id);
            }
        }
    }
    group
}

#[cfg(test)]
mod tests {
    use std::{
        path::PathBuf,
        sync::{
            atomic::{AtomicUsize, Ordering},
            Mutex,
        },
    };

    use super::*;

    use crate::{
        action_log::ActionLog,
        config::{AppSuspensionRule, AppSuspensionSettings},
        control::suspension::SuspensionController,
        features::watchdog::app_suspension::AppSuspensionManager,
    };

    static TEST_LOCK: Mutex<()> = Mutex::new(());
    static PROCESS_CALLS: AtomicUsize = AtomicUsize::new(0);
    static PROCESS_PATH_CALLS: AtomicUsize = AtomicUsize::new(0);
    static FOREGROUND_ID_CALLS: AtomicUsize = AtomicUsize::new(0);
    static FOREGROUND_PROCESS_CALLS: AtomicUsize = AtomicUsize::new(0);
    static VISIBLE_WINDOW_CALLS: AtomicUsize = AtomicUsize::new(0);
    static TOP_LEVEL_WINDOW_CALLS: AtomicUsize = AtomicUsize::new(0);
    static TOPOLOGY_CALLS: AtomicUsize = AtomicUsize::new(0);

    #[test]
    fn group_samples_require_every_exact_member() {
        let group = BTreeMap::from([(1, 10), (2, 20)]);
        assert!(sample_workload(&group, |id| (id == 1).then_some((10, 100))).is_none());
        assert!(sample_workload(&group, |id| Some((u64::from(id) * 10 + 1, 100))).is_none());
        assert_eq!(
            sample_workload(&group, |id| Some((u64::from(id) * 10, u64::from(id) * 100)))
                .unwrap()
                .cpu_time_100ns,
            300
        );
    }

    #[test]
    fn power_only_reads_exact_native_cpu_counters_without_the_control_catalogue() {
        let mut observations = CycleObservations::with_sources(ObservationSources {
            foreground_process_id: || Some(std::process::id()),
            ..ObservationSources::default()
        });
        let pid = std::process::id();
        let processes = observations.foreground_process_ancestry(pid).unwrap();
        let root = processes.iter().find(|process| process.id == pid).unwrap();
        assert!(root.creation_time.is_some());
        assert!(!root.can_set_information);
        assert!(root.is_critical.is_none());
        let workload = observations.adaptive_workload(&processes);
        assert_eq!(workload.get(&pid).copied(), root.creation_time);
        let sample = observations.adaptive_cpu_sample(&workload).unwrap();
        assert_eq!(observations.adaptive_cpu_sample(&workload), Some(sample));
        assert_eq!(
            observations.availability().processes,
            ObservationAvailability::NotRequested
        );
    }

    #[test]
    fn adaptive_engine_requests_the_process_list_only_after_foreground_pressure() {
        use crate::{
            config::AdaptiveEngineProcessSettings,
            features::cpu_control::adaptive_engine_process::{
                AdaptiveEngineProcessManager, AdaptiveEngineProcessUpdate,
            },
        };
        use std::time::Duration;

        let _guard = TEST_LOCK.lock().unwrap();
        let foreground = Arc::from(processes().unwrap());
        let settings = AdaptiveEngineProcessSettings {
            cpu_pressure_restraint_enabled: true,
            foreground_or_system_cpu_threshold_percent: 50,
            ..Default::default()
        };
        let mut manager = AdaptiveEngineProcessManager::default();
        let mut allocation = Default::default();
        let mut priority = Default::default();
        let mut log = ActionLog::default();
        let start = Instant::now();
        for (seconds, ticks, pressured) in [(0, 0, false), (1, 0, false), (2, u64::MAX, true)] {
            let mut observations = CycleObservations {
                foreground_ancestry: Observation::Available(Arc::clone(&foreground)),
                foreground_process_id: Observation::Available(Some(42)),
                adaptive_cpu_sample: Some(Some(ProcessCpuSample {
                    cpu_time_100ns: ticks,
                    sampled_at: start + Duration::from_secs(seconds),
                })),
                sources: ObservationSources {
                    process_entries: || Err("background list requested".into()),
                    ..ObservationSources::default()
                },
                ..CycleObservations::default()
            };
            let status = manager.update(
                AdaptiveEngineProcessUpdate {
                    settings: &settings,
                    automation_enabled: true,
                    allow_cross_session_process_control: true,
                    foreground_process_id: Some(42),
                    total_cpu_usage_percent: Some(0.0),
                    user_idle_for: None,
                    excluded_process_id: None,
                    explicit_cpu_allocation_paths: &[],
                    observations: &mut observations,
                },
                &mut allocation,
                &mut priority,
                &mut log,
            );
            assert!(status.enabled);
            assert_eq!(status.observation_paused, pressured);
            if pressured {
                assert_eq!(
                    status.last_error.as_deref(),
                    Some("background list requested")
                );
            } else {
                assert_eq!(status.scanned_processes, 0);
                assert_eq!(
                    status.foreground_cpu_usage_tenths,
                    (seconds > 0).then_some(0)
                );
                assert_eq!(
                    observations.availability().processes,
                    ObservationAvailability::NotRequested
                );
            }
        }
    }

    #[test]
    fn adaptive_engine_skips_foreground_reads_only_during_known_quiet_system_pressure() {
        use crate::{
            config::{
                AdaptiveEngineProcessSettings,
                AdaptivePressureSource::{Either, System},
            },
            features::cpu_control::adaptive_engine_process::{
                AdaptiveEngineProcessManager, AdaptiveEngineProcessUpdate,
            },
        };

        let mut manager = AdaptiveEngineProcessManager::default();
        let mut allocation = Default::default();
        let mut priority = Default::default();
        let mut log = ActionLog::default();
        let ancestry_error = "foreground ancestry requested";
        let catalog_error = "process list requested";
        for (source, system_cpu, zones_enabled, expected_error) in [
            (System, Some(74.0), false, None),
            (System, Some(75.0), false, Some(catalog_error)),
            (System, None, false, Some(ancestry_error)),
            (Either, Some(74.0), false, Some(ancestry_error)),
            (System, Some(74.0), true, Some(ancestry_error)),
        ] {
            let settings = AdaptiveEngineProcessSettings {
                limit_background_processors_enabled: true,
                dynamic_resource_zones_enabled: zones_enabled,
                pressure_source: source,
                foreground_or_system_cpu_threshold_percent: 75,
                ..Default::default()
            };
            let mut observations = CycleObservations {
                foreground_ancestry: Observation::Unavailable(ancestry_error.into()),
                sources: ObservationSources {
                    process_entries: || Err("process list requested".into()),
                    ..ObservationSources::default()
                },
                ..CycleObservations::default()
            };
            let status = manager.update(
                AdaptiveEngineProcessUpdate {
                    settings: &settings,
                    automation_enabled: true,
                    allow_cross_session_process_control: true,
                    foreground_process_id: Some(42),
                    total_cpu_usage_percent: system_cpu,
                    user_idle_for: None,
                    excluded_process_id: None,
                    explicit_cpu_allocation_paths: &[],
                    observations: &mut observations,
                },
                &mut allocation,
                &mut priority,
                &mut log,
            );
            assert!(status.enabled);
            assert_eq!(
                status.last_error.as_deref(),
                expected_error,
                "{source:?} with load {system_cpu:?} and zones {zones_enabled}"
            );
            assert_eq!(status.observation_paused, expected_error.is_some());
            if expected_error.is_none() {
                assert_eq!(status.scanned_processes, 0);
                assert!(observations.adaptive_workload.is_none());
                assert!(observations.adaptive_cpu_sample.is_none());
                assert_eq!(
                    observations.availability().processes,
                    ObservationAvailability::NotRequested
                );
            }
        }
    }

    #[test]
    fn adaptive_engine_reuses_the_catalogue_only_for_known_system_pressure() {
        use crate::{
            config::{AdaptiveEngineProcessSettings, AdaptivePressureSource},
            features::cpu_control::adaptive_engine_process::{
                AdaptiveEngineProcessManager, AdaptiveEngineProcessUpdate,
            },
        };

        #[derive(PartialEq, Eq)]
        enum ExpectedPass {
            Quiet,
            SystemPressure,
            ChangedAncestry,
        }
        use ExpectedPass::{ChangedAncestry, Quiet, SystemPressure};

        fn catalogue_with_child() -> Result<Vec<ProcessInfo>, String> {
            let mut rows = processes()?;
            rows[0].can_set_information = false;
            rows.push(ProcessInfo {
                id: 101,
                parent_id: Some(42),
                creation_time: Some(2),
                ..rows[0].clone()
            });
            Ok(rows)
        }

        let _guard = TEST_LOCK.lock().unwrap();
        let foreground = Arc::from(processes().unwrap());
        let mut manager = AdaptiveEngineProcessManager::default();
        let mut allocation = Default::default();
        let mut priority = Default::default();
        let mut log = ActionLog::default();
        for (case, threshold, system_cpu, expected) in [
            ("fresh recovery band", 75, Some(72.0), Quiet),
            ("entry", 75, Some(80.0), SystemPressure),
            ("held recovery band", 75, Some(72.0), SystemPressure),
            ("below recovery", 75, Some(69.0), Quiet),
            ("re-entry", 75, Some(80.0), SystemPressure),
            ("threshold changed", 76, Some(73.0), Quiet),
            ("entry with new threshold", 76, Some(80.0), SystemPressure),
            ("both signals unknown", 76, None, ChangedAncestry),
        ] {
            let settings = AdaptiveEngineProcessSettings {
                dynamic_resource_zones_enabled: true,
                pressure_source: AdaptivePressureSource::Either,
                foreground_or_system_cpu_threshold_percent: threshold,
                ..Default::default()
            };
            let mut observations = CycleObservations {
                foreground_ancestry: if expected == SystemPressure {
                    Observation::Unavailable("unexpected standalone ancestry read".into())
                } else {
                    Observation::Available(Arc::clone(&foreground))
                },
                foreground_process_id: Observation::Available(Some(42)),
                adaptive_cpu_sample: Some(None),
                sources: ObservationSources {
                    process_entries: catalogue_with_child,
                    process_metadata: |_| {},
                    visible_window_process_ids: || Some(BTreeSet::new()),
                    ..ObservationSources::default()
                },
                ..CycleObservations::default()
            };
            let status = manager.update(
                AdaptiveEngineProcessUpdate {
                    settings: &settings,
                    automation_enabled: true,
                    allow_cross_session_process_control: true,
                    foreground_process_id: Some(42),
                    total_cpu_usage_percent: system_cpu,
                    user_idle_for: None,
                    excluded_process_id: Some(42),
                    explicit_cpu_allocation_paths: &[],
                    observations: &mut observations,
                },
                &mut allocation,
                &mut priority,
                &mut log,
            );
            assert_eq!(
                status.observation_paused,
                expected == ChangedAncestry,
                "{case}"
            );
            assert_eq!(
                status.scanned_processes,
                if expected == SystemPressure { 2 } else { 0 },
                "{case}"
            );
            assert_eq!(
                observations.availability().processes,
                if expected == Quiet {
                    ObservationAvailability::NotRequested
                } else {
                    ObservationAvailability::Available
                },
                "{case}"
            );
        }
    }

    #[test]
    fn adaptive_engine_only_enriches_catalogue_for_an_enabled_path_exclusion() {
        use crate::{
            config::{
                AdaptiveEngineExclusionRule, AdaptiveEngineProcessSettings, AdaptivePressureSource,
            },
            features::cpu_control::adaptive_engine_process::{
                AdaptiveEngineProcessManager, AdaptiveEngineProcessUpdate,
            },
        };

        let name = AdaptiveEngineExclusionRule::exact_name("test.exe");
        let pattern = toml::from_str("executable_name_pattern = 'test*.exe'").unwrap();
        let path = AdaptiveEngineExclusionRule::exact_path(r"C:\Apps\test.exe");
        let disabled_path = AdaptiveEngineExclusionRule {
            enabled: false,
            ..path.clone()
        };
        let _guard = TEST_LOCK.lock().unwrap();
        for (custom_rules, expected_enrichment) in [
            (vec![name.clone(), disabled_path], false),
            (vec![pattern], false),
            (vec![name, path], true),
        ] {
            let settings = AdaptiveEngineProcessSettings {
                limit_background_processors_enabled: true,
                pressure_source: AdaptivePressureSource::System,
                custom_rules,
                ..Default::default()
            };
            let mut observations =
                CycleObservations::with_adaptive_cpu_sample(processes().unwrap(), Some(42), None);
            observations.sources.visible_window_process_ids = || Some(BTreeSet::new());
            observations.sources.cpu_topology = || Ok(crate::cpu_topology::test_topology());
            observations.sources.enrich_process_paths = |_| {};
            let status = AdaptiveEngineProcessManager::default().update(
                AdaptiveEngineProcessUpdate {
                    settings: &settings,
                    automation_enabled: true,
                    allow_cross_session_process_control: true,
                    foreground_process_id: Some(42),
                    total_cpu_usage_percent: Some(90.0),
                    user_idle_for: None,
                    excluded_process_id: None,
                    explicit_cpu_allocation_paths: &[],
                    observations: &mut observations,
                },
                &mut Default::default(),
                &mut Default::default(),
                &mut ActionLog::default(),
            );
            assert!(!status.observation_paused);
            assert_eq!(status.scanned_processes, 1);
            assert_eq!(observations.process_paths_enriched, expected_enrichment);
        }
    }

    #[test]
    fn adaptive_service_protection_reads_only_during_pressure_and_pauses_on_failure() {
        use crate::{
            config::{AdaptiveEngineProcessSettings, AdaptivePressureSource},
            features::cpu_control::adaptive_engine_process::{
                AdaptiveEngineProcessManager, AdaptiveEngineProcessUpdate,
            },
        };

        let _guard = TEST_LOCK.lock().unwrap();
        for (exclude, system_cpu, expected_pause) in [
            (true, 20.0, false),
            (false, 90.0, false),
            (true, 90.0, true),
        ] {
            let settings = AdaptiveEngineProcessSettings {
                limit_background_processors_enabled: true,
                pressure_source: AdaptivePressureSource::System,
                exclude_service_hosts: exclude,
                ..Default::default()
            };
            let mut observations =
                CycleObservations::with_adaptive_cpu_sample(processes().unwrap(), Some(42), None);
            observations.sources.visible_window_process_ids = || Some(BTreeSet::new());
            observations.sources.cpu_topology = || Ok(crate::cpu_topology::test_topology());
            observations.sources.service_process_ids = if expected_pause {
                || Err("Services database unavailable in test".into())
            } else {
                || panic!("service-host guard must not query the database for this pass")
            };
            let status = AdaptiveEngineProcessManager::default().update(
                AdaptiveEngineProcessUpdate {
                    settings: &settings,
                    automation_enabled: true,
                    allow_cross_session_process_control: true,
                    foreground_process_id: Some(42),
                    total_cpu_usage_percent: Some(system_cpu),
                    user_idle_for: None,
                    excluded_process_id: None,
                    explicit_cpu_allocation_paths: &[],
                    observations: &mut observations,
                },
                &mut Default::default(),
                &mut Default::default(),
                &mut ActionLog::default(),
            );
            assert_eq!(status.observation_paused, expected_pause);
            if expected_pause {
                assert_eq!(
                    status.last_error.as_deref(),
                    Some("Services database unavailable in test")
                );
            }
        }
    }

    #[test]
    fn idle_restraint_still_scans_for_independently_enabled_placement() {
        use crate::{
            config::AdaptiveEngineProcessSettings,
            features::cpu_control::adaptive_engine_process::{
                AdaptiveEngineProcessManager, AdaptiveEngineProcessUpdate, RestraintIdlePause,
            },
        };
        use std::time::Duration;

        for placement_enabled in [false, true] {
            let settings = AdaptiveEngineProcessSettings {
                cpu_pressure_restraint_enabled: true,
                pause_restraint_when_idle: true,
                limit_background_processors_enabled: placement_enabled,
                ..Default::default()
            };
            let mut observations = CycleObservations {
                sources: ObservationSources {
                    process_entries: || Err("background list requested".into()),
                    ..ObservationSources::default()
                },
                ..CycleObservations::default()
            };
            let snapshot = AdaptiveEngineProcessManager::default().update(
                AdaptiveEngineProcessUpdate {
                    settings: &settings,
                    automation_enabled: true,
                    allow_cross_session_process_control: true,
                    foreground_process_id: Some(42),
                    total_cpu_usage_percent: Some(100.0),
                    user_idle_for: Some(Duration::from_secs(
                        settings.restraint_idle_timeout_seconds,
                    )),
                    excluded_process_id: None,
                    explicit_cpu_allocation_paths: &[],
                    observations: &mut observations,
                },
                &mut Default::default(),
                &mut Default::default(),
                &mut ActionLog::default(),
            );
            assert!(snapshot.enabled);
            assert!(!snapshot.cpu_pressure_restraint_active);
            assert_eq!(
                snapshot.restraint_idle_pause,
                Some(RestraintIdlePause::Idle)
            );
            if placement_enabled {
                assert_eq!(
                    snapshot.last_error.as_deref(),
                    Some("background list requested")
                );
            } else {
                assert_eq!(
                    observations.availability().processes,
                    ObservationAvailability::NotRequested
                );
            }
        }
    }

    fn topology() -> Result<crate::cpu_topology::CpuTopology, String> {
        use crate::cpu_topology::{CpuTopology, LogicalProcessor, ProcessorGroup, ProcessorId};
        TOPOLOGY_CALLS.fetch_add(1, Ordering::Relaxed);
        let id = ProcessorId {
            group: 0,
            number: 0,
        };
        CpuTopology::new(
            vec![LogicalProcessor {
                id,
                core: id,
                efficiency_class: 0,
            }],
            vec![ProcessorGroup {
                number: 0,
                active_mask: 1,
            }],
            Vec::new(),
            Vec::new(),
        )
        .map_err(|error| error.to_string())
    }

    #[test]
    fn topology_success_and_failure_are_shared_only_within_a_cycle() {
        let _guard = TEST_LOCK.lock().unwrap();
        TOPOLOGY_CALLS.store(0, Ordering::Relaxed);
        let mut observations = CycleObservations::with_sources(ObservationSources {
            cpu_topology: topology,
            ..ObservationSources::default()
        });
        let first = observations.cpu_topology().unwrap();
        assert!(Arc::ptr_eq(&first, &observations.cpu_topology().unwrap()));
        assert_eq!(TOPOLOGY_CALLS.load(Ordering::Relaxed), 1);

        fn unavailable() -> Result<crate::cpu_topology::CpuTopology, String> {
            TOPOLOGY_CALLS.fetch_add(1, Ordering::Relaxed);
            Err("topology unavailable".to_owned())
        }
        let mut failed = CycleObservations::with_sources(ObservationSources {
            cpu_topology: unavailable,
            ..ObservationSources::default()
        });
        assert_eq!(failed.cpu_topology().unwrap_err(), "topology unavailable");
        assert_eq!(failed.cpu_topology().unwrap_err(), "topology unavailable");
        assert_eq!(TOPOLOGY_CALLS.load(Ordering::Relaxed), 2);
        let mut next = CycleObservations::with_sources(ObservationSources {
            cpu_topology: topology,
            ..ObservationSources::default()
        });
        assert!(next.cpu_topology().is_ok());
        assert_eq!(TOPOLOGY_CALLS.load(Ordering::Relaxed), 3);
    }

    fn processes() -> Result<Vec<ProcessInfo>, String> {
        PROCESS_CALLS.fetch_add(1, Ordering::Relaxed);
        Ok(vec![ProcessInfo {
            id: 42,
            creation_time: Some(1),
            parent_id: None,
            session_id: Some(1),
            user_name: None,
            is_service_account: Some(false),
            is_critical: Some(false),
            can_set_information: true,
            name: "test.exe".to_owned(),
            image_path: Some(PathBuf::from(r"C:\Apps\test.exe")),
        }])
    }

    #[test]
    fn sparse_paths_defer_unrelated_metadata_and_reuse_first_attempts() {
        let _guard = TEST_LOCK.lock().unwrap();
        static METADATA_CALLS: AtomicUsize = AtomicUsize::new(0);

        PROCESS_CALLS.store(0, Ordering::Relaxed);
        METADATA_CALLS.store(0, Ordering::Relaxed);
        let mut observations = CycleObservations::with_sources(ObservationSources {
            process_entries: || {
                let row = processes()?.remove(0);
                Ok((1..=3)
                    .map(|id| ProcessInfo {
                        id,
                        creation_time: None,
                        image_path: None,
                        ..row.clone()
                    })
                    .collect())
            },
            process_metadata: |process| {
                METADATA_CALLS.fetch_add(1, Ordering::Relaxed);
                process.creation_time = (process.id != 2).then_some(u64::from(process.id));
            },
            enrich_process_paths: |processes| {
                for process in processes {
                    process.creation_time = Some(u64::from(process.id));
                    process.image_path = Some(PathBuf::from(r"C:\Apps\test.exe"));
                }
            },
            ..ObservationSources::default()
        });

        for _ in 0..2 {
            let selected = observations
                .processes_for_path_matching(|process| process.id <= 2)
                .unwrap();
            assert_eq!(selected.len(), 2);
            assert_eq!(selected[0].creation_time, Some(1));
            assert!(selected[0].image_path.is_some());
            assert!(selected[1].creation_time.is_none());
            assert!(selected[1].image_path.is_none());
            assert_eq!(METADATA_CALLS.load(Ordering::Relaxed), 2);
        }

        let full = observations.foreground_process_ancestry(1).unwrap();
        assert_eq!(full[2].creation_time, Some(3));
        assert!(full[1].creation_time.is_none());
        assert!(Arc::ptr_eq(&full, &observations.processes().unwrap()));
        assert_eq!(METADATA_CALLS.load(Ordering::Relaxed), 3);
        assert_eq!(PROCESS_CALLS.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn enrichment_defers_new_generations_without_changing_cached_workload() {
        let _guard = TEST_LOCK.lock().unwrap();
        fn initial() -> Result<Vec<ProcessInfo>, String> {
            let root = processes()?.remove(0);
            Ok(vec![
                root.clone(),
                ProcessInfo {
                    id: 101,
                    parent_id: Some(42),
                    creation_time: None,
                    ..root.clone()
                },
                ProcessInfo {
                    id: 102,
                    parent_id: Some(42),
                    creation_time: Some(2),
                    ..root
                },
            ])
        }
        fn enrich(records: &mut [ProcessInfo]) {
            enrich_paths(records);
            for record in records {
                record.creation_time = match record.id {
                    101 => Some(3),
                    102 => Some(4),
                    _ => record.creation_time,
                };
            }
        }
        let mut observations = CycleObservations::with_sources(ObservationSources {
            process_entries: initial,
            process_metadata: |_| {},
            enrich_process_paths: enrich,
            foreground_process_id: foreground_id,
            process_from_id: foreground_process,
            visible_window_process_ids: visible_windows,
            top_level_window_process_ids: top_level_windows,
            cpu_topology: topology,
            ..ObservationSources::default()
        });
        let before = observations.processes().unwrap();
        let workload = observations.adaptive_workload(&before);
        assert_eq!(
            *workload,
            std::collections::BTreeMap::from([(42, 1), (102, 2)])
        );
        let selected = observations
            .processes_for_path_matching(|process| process.id == 102)
            .unwrap();
        assert_eq!((selected.len(), selected[0].id), (1, 102));
        assert_eq!(selected[0].creation_time, Some(2));
        assert!(selected[0].image_path.is_none());
        let after = observations.processes_with_paths().unwrap();
        let reused = observations
            .processes_for_path_matching(|_| panic!("full enrichment already available"))
            .unwrap();
        assert!(std::ptr::eq(reused.as_ref(), after.as_ref()));
        assert_eq!(after[1].creation_time, None);
        assert_eq!(after[2].creation_time, Some(2));
        assert_eq!(observations.adaptive_workload(&after), workload);
        assert!(before[2].image_path.is_some());
        for record in &after[1..] {
            assert!(record.image_path.is_none());
            assert_eq!(record.is_critical, None);
            assert!(!record.can_set_information);
        }
        // The actual Thread Priority consumer must skip deferred targets before opening them.
        let mut manager =
            crate::features::priority_control::thread_priority::ThreadPriorityManager::default();
        let result = manager.update(
            &mut crate::control::thread_priority::ThreadPriorityController::default(),
            &crate::config::ThreadPrioritySettings {
                enabled: true,
                foreground_detection_enabled: true,
                foreground_priority: crate::config::ProcessThreadPrioritySetting::Default,
                background_priority: crate::config::ProcessThreadPrioritySetting::Idle,
                ..Default::default()
            },
            true,
            true,
            Some(42),
            &mut observations,
            &mut ActionLog::default(),
        );
        assert_eq!(result.failed_processes, 0);
        assert_eq!(result.adjusted_processes, 0);
    }

    #[test]
    fn adaptive_workload_rejects_reused_ancestry_and_preserves_standalone_matching() {
        let _guard = TEST_LOCK.lock().unwrap();
        let root = processes().unwrap().remove(0);
        let make = |id, parent, creation, name: &str| ProcessInfo {
            id,
            parent_id: parent,
            creation_time: creation,
            name: name.into(),
            image_path: Some(PathBuf::from(format!(r"C:\Apps\{name}"))),
            ..root.clone()
        };
        let entries = vec![
            make(42, None, Some(100), "root.exe"),
            make(99, Some(42), Some(20), "stale.exe"),
            make(101, Some(42), Some(110), "child.exe"),
            make(102, Some(101), None, "unknown.exe"),
            make(103, None, Some(120), "root.exe"),
        ];
        assert_eq!(
            foreground_process_group_ids(&entries, Some(42)),
            BTreeSet::from([42, 101])
        );
        assert!(foreground_process_group_ids(&entries, Some(999)).is_empty());
        for (index, matches) in [(0, true), (2, false), (4, true)] {
            let process = &entries[index];
            assert_eq!(
                crate::foreground::is_foreground_process(
                    process.id,
                    process.image_path.as_deref().unwrap(),
                    Some(42),
                    entries[0].image_path.as_deref(),
                ),
                matches,
            );
        }
    }

    #[test]
    fn foreground_group_index_preserves_scan_results_across_forests_and_cycles() {
        fn reference(processes: &[ProcessInfo], root: Option<u32>) -> BTreeSet<u32> {
            let by_id = processes
                .iter()
                .map(|process| (process.id, process))
                .collect::<std::collections::BTreeMap<_, _>>();
            let Some(root) = root.filter(|id| {
                by_id
                    .get(id)
                    .is_some_and(|process| process.creation_time.is_some())
            }) else {
                return BTreeSet::new();
            };
            let mut group = BTreeSet::from([root]);
            let mut pending = vec![root];
            while let Some(parent) = pending.pop() {
                for child in processes {
                    if child.parent_id == Some(parent)
                        && child
                            .creation_time
                            .zip(by_id[&parent].creation_time)
                            .is_some_and(|(child, parent)| child >= parent)
                        && group.insert(child.id)
                    {
                        pending.push(child.id);
                    }
                }
            }
            group
        }

        let make = |id, parent_id, creation_time| ProcessInfo {
            id,
            parent_id,
            creation_time,
            session_id: Some(1),
            user_name: None,
            is_service_account: Some(false),
            is_critical: Some(false),
            can_set_information: false,
            name: format!("{id}.exe"),
            image_path: None,
        };
        let deep = (1..=512)
            .map(|id| make(id, (id > 1).then_some(id - 1), Some(u64::from(id))))
            .collect::<Vec<_>>();
        let leaf = (1..=512)
            .map(|id| make(id, None, Some(u64::from(id))))
            .collect::<Vec<_>>();
        let mut wide = (1..=512)
            .map(|id| make(id, (id > 1).then_some(1), Some(u64::from(id))))
            .collect::<Vec<_>>();
        wide.extend((513..=1024).map(|id| make(id, None, Some(u64::from(id)))));
        let mut unknown_root = deep.clone();
        unknown_root[0].creation_time = None;
        let mut interrupted = deep.clone();
        interrupted[100].creation_time = None;
        interrupted[200].creation_time = Some(1);
        let cyclic = vec![
            make(1, Some(3), Some(10)),
            make(2, Some(1), Some(10)),
            make(3, Some(2), Some(10)),
            make(4, Some(5), Some(10)),
            make(5, Some(4), Some(10)),
            make(6, Some(6), Some(10)),
            make(7, Some(999), Some(10)),
        ];
        let duplicate_ids = vec![
            make(1, None, Some(10)),
            make(2, Some(1), Some(11)),
            make(3, Some(2), Some(12)),
            make(1, None, Some(100)),
        ];
        let mut forests = vec![
            Vec::new(),
            leaf,
            deep,
            wide,
            unknown_root,
            interrupted,
            cyclic,
            duplicate_ids,
        ];
        for family_size in [15, 16, 17] {
            let mut entries = (1..=family_size)
                .map(|id| make(id, (id > 1).then_some(id - 1), Some(u64::from(id))))
                .collect::<Vec<_>>();
            entries.extend((513..=1024).map(|id| make(id, None, Some(u64::from(id)))));
            forests.push(entries);
        }
        for mut entries in forests {
            for _ in 0..2 {
                for root in [None, Some(1), Some(2), Some(6), Some(9999)] {
                    assert_eq!(
                        foreground_process_group_ids(&entries, root),
                        reference(&entries, root),
                        "root {root:?} in {} process records",
                        entries.len()
                    );
                }
                entries.reverse();
            }
        }
    }

    fn foreground_id() -> Option<u32> {
        FOREGROUND_ID_CALLS.fetch_add(1, Ordering::Relaxed);
        Some(42)
    }

    fn enrich_paths(processes: &mut [ProcessInfo]) {
        PROCESS_PATH_CALLS.fetch_add(1, Ordering::Relaxed);
        for process in processes {
            process.image_path = Some(PathBuf::from(r"C:\Apps\test.exe"));
            process.is_service_account = Some(false);
            process.is_critical = Some(false);
        }
    }

    fn foreground_process(process_id: u32) -> Option<ForegroundProcess> {
        FOREGROUND_PROCESS_CALLS.fetch_add(1, Ordering::Relaxed);
        Some(ForegroundProcess {
            id: process_id,
            name: "test.exe".to_owned(),
            executable_path: PathBuf::from(r"C:\Apps\test.exe"),
            is_critical: Some(false),
        })
    }

    fn visible_windows() -> Option<BTreeSet<u32>> {
        VISIBLE_WINDOW_CALLS.fetch_add(1, Ordering::Relaxed);
        Some(BTreeSet::from([42]))
    }

    fn top_level_windows() -> BTreeSet<u32> {
        TOP_LEVEL_WINDOW_CALLS.fetch_add(1, Ordering::Relaxed);
        BTreeSet::from([42])
    }

    #[test]
    fn policy_foreground_requires_confirmed_noncritical_state() {
        let mut observations = CycleObservations::default();
        for (is_critical, accepted) in [(Some(false), true), (Some(true), false), (None, false)] {
            observations.foreground_process = Observation::Available(Some(ForegroundProcess {
                id: 42,
                name: "test.exe".to_owned(),
                executable_path: PathBuf::from(r"C:\Apps\test.exe"),
                is_critical,
            }));
            assert_eq!(
                observations.noncritical_foreground_process().is_some(),
                accepted
            );
        }
        observations.foreground_process = Observation::Available(None);
        assert!(observations.noncritical_foreground_process().is_none());
    }

    #[test]
    fn requested_domains_are_collected_once_and_a_new_cycle_starts_dormant() {
        let _guard = TEST_LOCK.lock().unwrap();
        for counter in [
            &PROCESS_CALLS,
            &PROCESS_PATH_CALLS,
            &FOREGROUND_ID_CALLS,
            &FOREGROUND_PROCESS_CALLS,
            &VISIBLE_WINDOW_CALLS,
            &TOP_LEVEL_WINDOW_CALLS,
        ] {
            counter.store(0, Ordering::Relaxed);
        }

        let mut observations = CycleObservations::with_sources(ObservationSources {
            process_entries: processes,
            process_metadata: |_| {},
            enrich_process_paths: enrich_paths,
            foreground_process_id: foreground_id,
            process_from_id: foreground_process,
            visible_window_process_ids: visible_windows,
            top_level_window_process_ids: top_level_windows,
            cpu_topology: topology,
            ..ObservationSources::default()
        });
        assert_eq!(
            observations.availability(),
            ObservationAvailabilitySnapshot {
                processes: ObservationAvailability::NotRequested,
                foreground_process_id: ObservationAvailability::NotRequested,
                foreground_process: ObservationAvailability::NotRequested,
                visible_window_process_ids: ObservationAvailability::NotRequested,
                top_level_window_process_ids: ObservationAvailability::NotRequested,
            }
        );

        assert_eq!(observations.cached_foreground_executable_path(), None);
        assert_eq!(observations.foreground_process_id(), Some(42));
        assert_eq!(observations.foreground_process_id(), Some(42));
        assert_eq!(observations.cached_foreground_executable_path(), None);
        assert_eq!(
            observations.foreground_process().map(|process| process.id),
            Some(42)
        );
        assert_eq!(
            observations.cached_foreground_executable_path(),
            Some(Path::new(r"C:\Apps\test.exe"))
        );
        assert_eq!(
            observations
                .noncritical_foreground_process()
                .map(|process| process.id),
            Some(42)
        );
        assert_eq!(observations.processes().unwrap().len(), 1);
        assert_eq!(observations.processes().unwrap().len(), 1);
        assert!(observations.processes_with_paths().unwrap()[0]
            .image_path
            .is_some());
        assert!(observations.processes_with_paths().unwrap()[0]
            .image_path
            .is_some());
        assert!(observations
            .visible_window_process_ids()
            .unwrap()
            .contains(&42));
        assert!(observations.top_level_window_process_ids().contains(&42));
        assert!(observations.top_level_window_process_ids().contains(&42));
        assert!(observations
            .visible_window_process_ids()
            .unwrap()
            .contains(&42));

        assert_eq!(PROCESS_CALLS.load(Ordering::Relaxed), 1);
        assert_eq!(PROCESS_PATH_CALLS.load(Ordering::Relaxed), 1);
        assert_eq!(FOREGROUND_ID_CALLS.load(Ordering::Relaxed), 1);
        assert_eq!(FOREGROUND_PROCESS_CALLS.load(Ordering::Relaxed), 1);
        assert_eq!(VISIBLE_WINDOW_CALLS.load(Ordering::Relaxed), 1);
        assert_eq!(TOP_LEVEL_WINDOW_CALLS.load(Ordering::Relaxed), 1);
        assert_eq!(
            observations.availability(),
            ObservationAvailabilitySnapshot {
                processes: ObservationAvailability::Available,
                foreground_process_id: ObservationAvailability::Available,
                foreground_process: ObservationAvailability::Available,
                visible_window_process_ids: ObservationAvailability::Available,
                top_level_window_process_ids: ObservationAvailability::Available,
            }
        );
    }

    fn unavailable_processes() -> Result<Vec<ProcessInfo>, String> {
        PROCESS_CALLS.fetch_add(1, Ordering::Relaxed);
        Err("process snapshot failed".to_owned())
    }

    fn unavailable_visible_windows() -> Option<BTreeSet<u32>> {
        VISIBLE_WINDOW_CALLS.fetch_add(1, Ordering::Relaxed);
        None
    }

    #[test]
    fn app_suspension_reports_a_running_app_after_process_enrichment() {
        let _guard = TEST_LOCK.lock().unwrap();

        fn raw_processes() -> Result<Vec<ProcessInfo>, String> {
            let background = ProcessInfo {
                id: 42,
                creation_time: Some(1),
                parent_id: None,
                session_id: Some(1),
                user_name: None,
                is_service_account: None,
                is_critical: Some(false),
                can_set_information: true,
                name: "test.exe".to_owned(),
                image_path: None,
            };
            let foreground = ProcessInfo {
                id: 7,
                name: "foreground.exe".to_owned(),
                ..background.clone()
            };
            Ok(vec![background, foreground])
        }

        let path = r"C:\Apps\test.exe";
        let mut observations = CycleObservations::with_sources(ObservationSources {
            process_entries: raw_processes,
            process_metadata: |_| {},
            enrich_process_paths: |processes| {
                enrich_paths(processes);
                processes[1].image_path = Some(PathBuf::from(r"C:\Apps\foreground.exe"));
            },
            foreground_process_id: || Some(7),
            process_from_id: foreground_process,
            visible_window_process_ids: visible_windows,
            top_level_window_process_ids: top_level_windows,
            cpu_topology: topology,
            ..ObservationSources::default()
        });
        let settings = AppSuspensionSettings {
            enabled: true,
            background_delay_seconds: 60,
            suspendable_apps: vec![AppSuspensionRule {
                enabled: true,
                executable_path: path.to_owned(),
                network_wake_enabled: false,
                audio_wake_enabled: false,
                network_download_threshold_bytes: 0,
                network_download_threshold_unit: Default::default(),
                network_upload_threshold_bytes: 0,
                network_upload_threshold_unit: Default::default(),
            }],
            ..Default::default()
        };
        let mut manager = AppSuspensionManager::default();
        let mut controller = SuspensionController::default();
        let mut action_log = ActionLog::new(8);

        let snapshot = manager.update(
            &mut controller,
            &settings,
            true,
            true,
            Some(7),
            &[],
            &mut observations,
            &mut action_log,
        );

        assert_eq!(snapshot.running_apps, vec![path.to_owned()]);
    }

    #[test]
    #[ignore = "profiles live Windows observations; run alone with --nocapture"]
    fn profile_live_observation_costs() {
        use std::time::{Duration, Instant};
        let mut classifier = crate::bottleneck_classifier::BottleneckClassifier::default();
        let mut cpu = crate::cpu::CpuUsageMonitor::default();
        for sample in 0..12 {
            let mut observations = CycleObservations::default();
            let start = Instant::now();
            let processes = observations.processes().expect("process observation");
            let process_time = start.elapsed();
            let start = Instant::now();
            let visible = observations
                .visible_window_process_ids()
                .expect("visible windows");
            let _ = observations.adaptive_workload(&processes);
            let window_time = start.elapsed();
            let protected =
                crate::foreground::ProtectedProcesses::capture(&processes, false, None, visible);
            let mut groups = Vec::new();
            for prefilter in [false, true] {
                let start = Instant::now();
                let group: BTreeSet<_> = processes
                    .iter()
                    .filter(|process| !prefilter || protected.may_contain(process))
                    .filter(|process| {
                        crate::foreground::process_executable_path(process)
                            .is_some_and(|path| protected.contains(process.id, &path))
                    })
                    .map(|process| process.id)
                    .collect();
                eprintln!(
                    "sample={sample} prefilter={prefilter} grouping_us={}",
                    start.elapsed().as_micros()
                );
                groups.push(group);
            }
            assert_eq!(
                groups[0], groups[1],
                "visible process membership changed during the probe"
            );
            let start = Instant::now();
            let total_cpu = cpu.sample_usage();
            let cpu_time = start.elapsed();
            let start = Instant::now();
            let status = classifier.sample(total_cpu.percent);
            assert!(status.last_error.is_none(), "{:?}", status.last_error);
            eprintln!(
                "sample={sample} processes={:?} windows={:?} cpu={:?} bottleneck={:?}",
                process_time,
                window_time,
                cpu_time,
                start.elapsed()
            );
            std::thread::sleep(Duration::from_secs(1));
        }
    }

    #[test]
    fn unavailable_domains_are_cached_without_retrying_inside_the_pass() {
        let _guard = TEST_LOCK.lock().unwrap();
        PROCESS_CALLS.store(0, Ordering::Relaxed);
        VISIBLE_WINDOW_CALLS.store(0, Ordering::Relaxed);
        let mut observations = CycleObservations::with_sources(ObservationSources {
            process_entries: unavailable_processes,
            process_metadata: |_| {},
            enrich_process_paths: enrich_paths,
            foreground_process_id: foreground_id,
            process_from_id: foreground_process,
            visible_window_process_ids: unavailable_visible_windows,
            top_level_window_process_ids: top_level_windows,
            cpu_topology: topology,
            ..ObservationSources::default()
        });

        assert_eq!(
            observations.processes().unwrap_err(),
            "process snapshot failed"
        );
        assert_eq!(
            observations.processes().unwrap_err(),
            "process snapshot failed"
        );
        assert!(observations.visible_window_process_ids().is_err());
        assert!(observations.visible_window_process_ids().is_err());
        assert_eq!(PROCESS_CALLS.load(Ordering::Relaxed), 1);
        assert_eq!(VISIBLE_WINDOW_CALLS.load(Ordering::Relaxed), 1);
        assert_eq!(
            observations.availability().processes,
            ObservationAvailability::Unavailable
        );
        assert_eq!(
            observations.availability().visible_window_process_ids,
            ObservationAvailability::Unavailable
        );
    }
}
