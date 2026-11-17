use std::{collections::BTreeSet, sync::Arc};

use crate::foreground::{
    active_window::{process_from_id, ForegroundProcess},
    foreground_process_id, list_processes,
    process_list::enrich_process_paths,
    top_level_window_process_ids, visible_window_process_ids, ProcessInfo,
};

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
    processes: fn() -> Result<Vec<ProcessInfo>, String>,
    enrich_process_paths: fn(&mut [ProcessInfo]),
    foreground_process_id: fn() -> Option<u32>,
    process_from_id: fn(u32) -> Option<ForegroundProcess>,
    visible_window_process_ids: fn() -> Option<BTreeSet<u32>>,
    top_level_window_process_ids: fn() -> BTreeSet<u32>,
    cpu_topology: fn() -> Result<crate::cpu_topology::CpuTopology, String>,
}

impl Default for ObservationSources {
    fn default() -> Self {
        Self {
            processes: list_processes,
            enrich_process_paths,
            foreground_process_id,
            process_from_id,
            visible_window_process_ids,
            top_level_window_process_ids,
            cpu_topology: crate::features::cpu_control::cpu_allocation::processor_topology,
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
    process_paths_enriched: bool,
    adaptive_workload: Option<Arc<std::collections::BTreeMap<u32, u64>>>,
    foreground_process_id: Observation<Option<u32>>,
    foreground_process: Observation<Option<ForegroundProcess>>,
    visible_window_process_ids: Observation<Arc<BTreeSet<u32>>>,
    top_level_window_process_ids: Observation<Arc<BTreeSet<u32>>>,
    cpu_topology: Observation<Arc<crate::cpu_topology::CpuTopology>>,
}

impl Default for CycleObservations {
    fn default() -> Self {
        Self::with_sources(ObservationSources::default())
    }
}

impl CycleObservations {
    fn with_sources(sources: ObservationSources) -> Self {
        Self {
            sources,
            processes: Observation::NotRequested,
            process_paths_enriched: false,
            adaptive_workload: None,
            foreground_process_id: Observation::NotRequested,
            foreground_process: Observation::NotRequested,
            visible_window_process_ids: Observation::NotRequested,
            top_level_window_process_ids: Observation::NotRequested,
            cpu_topology: Observation::NotRequested,
        }
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

    pub(crate) fn adaptive_workload(
        &mut self,
        processes: &[ProcessInfo],
    ) -> Arc<std::collections::BTreeMap<u32, u64>> {
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

    pub(crate) fn processes(&mut self) -> Result<Arc<[ProcessInfo]>, String> {
        if matches!(self.processes, Observation::NotRequested) {
            self.processes = match (self.sources.processes)() {
                Ok(processes) => Observation::Available(Arc::from(processes)),
                Err(error) => Observation::Unavailable(error),
            };
        }

        match &self.processes {
            Observation::Available(processes) => Ok(Arc::clone(processes)),
            Observation::Unavailable(error) => Err(error.clone()),
            Observation::NotRequested => unreachable!("process observation was just requested"),
        }
    }

    pub(crate) fn processes_with_paths(&mut self) -> Result<Arc<[ProcessInfo]>, String> {
        if matches!(self.processes, Observation::NotRequested) {
            drop(self.processes()?);
        }
        if !self.process_paths_enriched {
            let Observation::Available(processes) = &mut self.processes else {
                return match &self.processes {
                    Observation::Unavailable(error) => Err(error.clone()),
                    Observation::NotRequested | Observation::Available(_) => {
                        unreachable!("process observation was already initialized")
                    }
                };
            };
            let generations = processes
                .iter()
                .map(|p| p.creation_time)
                .collect::<Vec<_>>();
            let records = Arc::make_mut(processes);
            (self.sources.enrich_process_paths)(records);
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
            self.process_paths_enriched = true;
        }
        match &self.processes {
            Observation::Available(processes) => Ok(Arc::clone(processes)),
            Observation::Unavailable(error) => Err(error.clone()),
            Observation::NotRequested => unreachable!("process observation was initialized"),
        }
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
        features::advanced_controls::app_suspension::AppSuspensionManager,
    };

    static TEST_LOCK: Mutex<()> = Mutex::new(());
    static PROCESS_CALLS: AtomicUsize = AtomicUsize::new(0);
    static PROCESS_PATH_CALLS: AtomicUsize = AtomicUsize::new(0);
    static FOREGROUND_ID_CALLS: AtomicUsize = AtomicUsize::new(0);
    static FOREGROUND_PROCESS_CALLS: AtomicUsize = AtomicUsize::new(0);
    static VISIBLE_WINDOW_CALLS: AtomicUsize = AtomicUsize::new(0);
    static TOP_LEVEL_WINDOW_CALLS: AtomicUsize = AtomicUsize::new(0);
    static TOPOLOGY_CALLS: AtomicUsize = AtomicUsize::new(0);

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
            records[1].creation_time = Some(3);
            records[2].creation_time = Some(4);
        }
        let mut observations = CycleObservations::with_sources(ObservationSources {
            processes: initial,
            enrich_process_paths: enrich,
            foreground_process_id: foreground_id,
            process_from_id: foreground_process,
            visible_window_process_ids: visible_windows,
            top_level_window_process_ids: top_level_windows,
            cpu_topology: topology,
        });
        let before = observations.processes().unwrap();
        let workload = observations.adaptive_workload(&before);
        assert_eq!(
            *workload,
            std::collections::BTreeMap::from([(42, 1), (102, 2)])
        );
        let after = observations.processes_with_paths().unwrap();
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
            crate::control::process::ControlOwner::AdaptiveEngine,
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
        let group = std::collections::BTreeMap::from([(42, 100), (101, 110)]);
        for (index, adaptive, standalone) in [(0, true, true), (2, true, false), (4, false, true)] {
            let process = &entries[index];
            for (owner, expected) in [
                (
                    crate::control::process::ControlOwner::AdaptiveEngine,
                    adaptive,
                ),
                (
                    crate::control::process::ControlOwner::ThreadPriority,
                    standalone,
                ),
            ] {
                assert_eq!(
                    crate::features::priority_control::foreground_for_owner(
                        owner,
                        process,
                        process.image_path.as_deref().unwrap(),
                        Some(42),
                        entries[0].image_path.as_deref(),
                        Some(&group)
                    ),
                    expected
                );
            }
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
            processes,
            enrich_process_paths: enrich_paths,
            foreground_process_id: foreground_id,
            process_from_id: foreground_process,
            visible_window_process_ids: visible_windows,
            top_level_window_process_ids: top_level_windows,
            cpu_topology: topology,
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

        assert_eq!(observations.foreground_process_id(), Some(42));
        assert_eq!(observations.foreground_process_id(), Some(42));
        assert_eq!(
            observations.foreground_process().map(|process| process.id),
            Some(42)
        );
        assert_eq!(
            observations.foreground_process().map(|process| process.id),
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
            Ok(vec![ProcessInfo {
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
            }])
        }

        let path = r"C:\Apps\test.exe";
        let mut observations = CycleObservations::with_sources(ObservationSources {
            processes: raw_processes,
            enrich_process_paths: enrich_paths,
            foreground_process_id: || Some(7),
            process_from_id: foreground_process,
            visible_window_process_ids: visible_windows,
            top_level_window_process_ids: top_level_windows,
            cpu_topology: topology,
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
            processes: unavailable_processes,
            enrich_process_paths: enrich_paths,
            foreground_process_id: foreground_id,
            process_from_id: foreground_process,
            visible_window_process_ids: unavailable_visible_windows,
            top_level_window_process_ids: top_level_windows,
            cpu_topology: topology,
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
