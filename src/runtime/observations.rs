use std::{collections::BTreeSet, sync::Arc};

use crate::foreground::{
    active_window::{process_from_id, ForegroundProcess},
    foreground_process_id, list_processes,
    process_list::{
        enrich_process_control, enrich_process_identities, enrich_process_paths,
        list_process_metadata, list_process_observations,
    },
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
    process_inventory: fn() -> Result<Vec<ProcessInfo>, String>,
    process_observations: fn() -> Result<Vec<ProcessInfo>, String>,
    enrich_process_identities: fn(&mut [ProcessInfo]),
    enrich_process_control: fn(&mut [ProcessInfo]),
    enrich_process_paths: fn(&mut [ProcessInfo]),
    foreground_process_id: fn() -> Option<u32>,
    process_from_id: fn(u32) -> Option<ForegroundProcess>,
    visible_window_process_ids: fn() -> Option<BTreeSet<u32>>,
    top_level_window_process_ids: fn() -> BTreeSet<u32>,
}

impl Default for ObservationSources {
    fn default() -> Self {
        Self {
            processes: list_processes,
            process_inventory: list_process_metadata,
            process_observations: list_process_observations,
            enrich_process_identities,
            enrich_process_control,
            enrich_process_paths,
            foreground_process_id,
            process_from_id,
            visible_window_process_ids,
            top_level_window_process_ids,
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
    // An attempted identity read, including failure, must never be repeated in this cycle.
    process_identity_observed: bool,
    process_control_enriched: bool,
    process_paths_enriched: bool,
    adaptive_workload: Option<Arc<std::collections::BTreeMap<u32, u64>>>,
    foreground_process_id: Observation<Option<u32>>,
    foreground_process: Observation<Option<ForegroundProcess>>,
    visible_window_process_ids: Observation<Arc<BTreeSet<u32>>>,
    top_level_window_process_ids: Observation<Arc<BTreeSet<u32>>>,
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
            process_identity_observed: false,
            process_control_enriched: false,
            process_paths_enriched: false,
            adaptive_workload: None,
            foreground_process_id: Observation::NotRequested,
            foreground_process: Observation::NotRequested,
            visible_window_process_ids: Observation::NotRequested,
            top_level_window_process_ids: Observation::NotRequested,
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

    /// PID/name metadata for conservative routing only; generations may not be observed yet.
    pub(crate) fn process_inventory(&mut self) -> Result<Arc<[ProcessInfo]>, String> {
        if matches!(self.processes, Observation::NotRequested) {
            self.processes = match (self.sources.process_inventory)() {
                Ok(processes) => Observation::Available(Arc::from(processes)),
                Err(error) => Observation::Unavailable(error),
            };
        }
        self.cached_processes()
    }

    pub(crate) fn processes(&mut self) -> Result<Arc<[ProcessInfo]>, String> {
        if matches!(self.processes, Observation::NotRequested) {
            self.processes = match (self.sources.processes)() {
                Ok(processes) => Observation::Available(Arc::from(processes)),
                Err(error) => Observation::Unavailable(error),
            };
            self.process_identity_observed = true;
            self.process_control_enriched = true;
        } else if !self.process_control_enriched {
            drop(self.process_observations()?);
            if let Observation::Available(processes) = &mut self.processes {
                let generations = processes
                    .iter()
                    .map(|process| process.creation_time)
                    .collect::<Vec<_>>();
                let records = Arc::make_mut(processes);
                (self.sources.enrich_process_control)(records);
                preserve_observed_generations(records, &generations);
            }
            self.process_control_enriched = true;
        }

        self.cached_processes()
    }

    pub(crate) fn process_observations(&mut self) -> Result<Arc<[ProcessInfo]>, String> {
        if matches!(self.processes, Observation::NotRequested) {
            self.processes = match (self.sources.process_observations)() {
                Ok(processes) => Observation::Available(Arc::from(processes)),
                Err(error) => Observation::Unavailable(error),
            };
            self.process_identity_observed = true;
        } else if !self.process_identity_observed {
            if let Observation::Available(processes) = &mut self.processes {
                (self.sources.enrich_process_identities)(Arc::make_mut(processes));
            }
            self.process_identity_observed = true;
        }
        self.cached_processes()
    }

    pub(crate) fn workload_for_observation(
        &mut self,
    ) -> Result<Arc<std::collections::BTreeMap<u32, u64>>, String> {
        if let Some(workload) = &self.adaptive_workload {
            return Ok(Arc::clone(workload));
        }
        let processes = self.process_observations()?;
        Ok(self.adaptive_workload(&processes))
    }

    fn cached_processes(&self) -> Result<Arc<[ProcessInfo]>, String> {
        match &self.processes {
            Observation::Available(processes) => Ok(Arc::clone(processes)),
            Observation::Unavailable(error) => Err(error.clone()),
            Observation::NotRequested => unreachable!("process observation was just requested"),
        }
    }

    pub(crate) fn processes_with_paths(&mut self) -> Result<Arc<[ProcessInfo]>, String> {
        if !self.process_control_enriched {
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
            preserve_observed_generations(records, &generations);
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

fn preserve_observed_generations(processes: &mut [ProcessInfo], generations: &[Option<u64>]) {
    for (process, &captured) in processes.iter_mut().zip(generations) {
        if captured.is_none() || process.creation_time != captured {
            // Enrichment cannot promote a new identity into an already-observed cycle.
            // Defer this target until a fresh process observation establishes its generation.
            process.creation_time = captured;
            process.session_id = None;
            process.image_path = None;
            process.user_name = None;
            process.is_service_account = None;
            process.is_critical = None;
            process.can_set_information = false;
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
    let by_id = processes
        .iter()
        .map(|p| (p.id, p))
        .collect::<std::collections::BTreeMap<_, _>>();
    let Some(root) = root.filter(|id| by_id.get(id).is_some_and(|p| p.creation_time.is_some()))
    else {
        return BTreeSet::new();
    };
    let mut group = BTreeSet::from([root]);
    let mut pending = vec![root];
    while let Some(parent) = pending.pop() {
        let parent_created = by_id[&parent].creation_time;
        for child in processes {
            if child.parent_id == Some(parent)
                && child
                    .creation_time
                    .zip(parent_created)
                    .is_some_and(|(child, parent)| child >= parent)
                && group.insert(child.id)
            {
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
    static PROCESS_INVENTORY_CALLS: AtomicUsize = AtomicUsize::new(0);
    static PROCESS_OBSERVATION_CALLS: AtomicUsize = AtomicUsize::new(0);
    static PROCESS_IDENTITY_CALLS: AtomicUsize = AtomicUsize::new(0);
    static PROCESS_CONTROL_CALLS: AtomicUsize = AtomicUsize::new(0);
    static PROCESS_PATH_CALLS: AtomicUsize = AtomicUsize::new(0);
    static FOREGROUND_ID_CALLS: AtomicUsize = AtomicUsize::new(0);
    static FOREGROUND_PROCESS_CALLS: AtomicUsize = AtomicUsize::new(0);
    static VISIBLE_WINDOW_CALLS: AtomicUsize = AtomicUsize::new(0);
    static TOP_LEVEL_WINDOW_CALLS: AtomicUsize = AtomicUsize::new(0);

    fn processes() -> Result<Vec<ProcessInfo>, String> {
        PROCESS_CALLS.fetch_add(1, Ordering::Relaxed);
        Ok(vec![process_record()])
    }

    fn process_record() -> ProcessInfo {
        ProcessInfo {
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
        }
    }

    fn process_observations() -> Result<Vec<ProcessInfo>, String> {
        PROCESS_OBSERVATION_CALLS.fetch_add(1, Ordering::Relaxed);
        Ok(vec![ProcessInfo {
            session_id: None,
            is_service_account: None,
            is_critical: None,
            can_set_information: false,
            image_path: None,
            ..process_record()
        }])
    }

    fn process_inventory() -> Result<Vec<ProcessInfo>, String> {
        PROCESS_INVENTORY_CALLS.fetch_add(1, Ordering::Relaxed);
        Ok(vec![ProcessInfo {
            creation_time: None,
            session_id: None,
            is_service_account: None,
            is_critical: None,
            can_set_information: false,
            image_path: None,
            ..process_record()
        }])
    }

    fn enrich_identities(processes: &mut [ProcessInfo]) {
        PROCESS_IDENTITY_CALLS.fetch_add(1, Ordering::Relaxed);
        for process in processes {
            process.creation_time = Some(1);
        }
    }

    fn enrich_control(processes: &mut [ProcessInfo]) {
        PROCESS_CONTROL_CALLS.fetch_add(1, Ordering::Relaxed);
        for process in processes {
            process.session_id = Some(1);
            process.is_critical = Some(false);
            process.can_set_information = true;
        }
    }

    fn sources() -> ObservationSources {
        ObservationSources {
            processes,
            process_inventory,
            process_observations,
            enrich_process_identities: enrich_identities,
            enrich_process_control: enrich_control,
            enrich_process_paths: enrich_paths,
            foreground_process_id: foreground_id,
            process_from_id: foreground_process,
            visible_window_process_ids: visible_windows,
            top_level_window_process_ids: top_level_windows,
        }
    }

    #[test]
    fn metadata_inventory_upgrades_once_without_recollecting_the_table() {
        let _guard = TEST_LOCK.lock().unwrap();
        for (control, paths) in [(false, false), (true, false), (true, true)] {
            for counter in [
                &PROCESS_CALLS,
                &PROCESS_INVENTORY_CALLS,
                &PROCESS_OBSERVATION_CALLS,
                &PROCESS_IDENTITY_CALLS,
                &PROCESS_CONTROL_CALLS,
                &PROCESS_PATH_CALLS,
            ] {
                counter.store(0, Ordering::Relaxed);
            }
            let mut observations = CycleObservations::with_sources(sources());
            let inventory = observations.process_inventory().unwrap();
            assert!(Arc::ptr_eq(
                &inventory,
                &observations.process_inventory().unwrap()
            ));
            assert_eq!(inventory[0].creation_time, None);
            assert!(!observations.process_identity_observed);
            let inventory_ptr = Arc::as_ptr(&inventory);
            drop(inventory);

            let upgraded = if paths {
                observations.processes_with_paths().unwrap()
            } else if control {
                observations.processes().unwrap()
            } else {
                observations.process_observations().unwrap()
            };
            assert!(std::ptr::eq(inventory_ptr, Arc::as_ptr(&upgraded)));
            assert_eq!(upgraded[0].creation_time, Some(1));
            assert_eq!(upgraded[0].can_set_information, control);
            assert_eq!(upgraded[0].image_path.is_some(), paths);
            assert!(observations.process_identity_observed);
            assert!(Arc::ptr_eq(
                &upgraded,
                &observations.process_observations().unwrap()
            ));
            assert!(Arc::ptr_eq(
                &upgraded,
                &observations.process_inventory().unwrap()
            ));
            assert_eq!(
                *observations.workload_for_observation().unwrap(),
                std::collections::BTreeMap::from([(42, 1)])
            );
            assert_eq!(PROCESS_INVENTORY_CALLS.load(Ordering::Relaxed), 1);
            assert_eq!(PROCESS_IDENTITY_CALLS.load(Ordering::Relaxed), 1);
            assert_eq!(
                PROCESS_CONTROL_CALLS.load(Ordering::Relaxed),
                usize::from(control)
            );
            assert_eq!(
                PROCESS_PATH_CALLS.load(Ordering::Relaxed),
                usize::from(paths)
            );
            assert_eq!(PROCESS_CALLS.load(Ordering::Relaxed), 0);
            assert_eq!(PROCESS_OBSERVATION_CALLS.load(Ordering::Relaxed), 0);
        }
    }

    #[test]
    fn metadata_upgrade_quarantines_failed_and_replaced_identities_for_the_cycle() {
        let _guard = TEST_LOCK.lock().unwrap();
        fn metadata() -> Result<Vec<ProcessInfo>, String> {
            let root = process_inventory()?.remove(0);
            Ok(vec![
                root.clone(),
                ProcessInfo {
                    id: 101,
                    parent_id: Some(42),
                    ..root.clone()
                },
                ProcessInfo {
                    id: 102,
                    parent_id: Some(42),
                    ..root
                },
            ])
        }
        fn identities(records: &mut [ProcessInfo]) {
            PROCESS_IDENTITY_CALLS.fetch_add(1, Ordering::Relaxed);
            records[0].creation_time = Some(1);
            records[1].creation_time = None;
            records[2].creation_time = Some(2);
        }
        fn control(records: &mut [ProcessInfo]) {
            enrich_control(records);
            records[1].creation_time = Some(3);
            records[2].creation_time = Some(4);
        }
        fn paths(records: &mut [ProcessInfo]) {
            enrich_paths(records);
            records[1].creation_time = Some(5);
            records[2].creation_time = Some(6);
        }
        for counter in [
            &PROCESS_INVENTORY_CALLS,
            &PROCESS_IDENTITY_CALLS,
            &PROCESS_CONTROL_CALLS,
            &PROCESS_PATH_CALLS,
        ] {
            counter.store(0, Ordering::Relaxed);
        }
        let mut observations = CycleObservations::with_sources(ObservationSources {
            process_inventory: metadata,
            enrich_process_identities: identities,
            enrich_process_control: control,
            enrich_process_paths: paths,
            ..sources()
        });
        let raw = observations.process_inventory().unwrap();
        let workload = observations.workload_for_observation().unwrap();
        assert!(raw.iter().all(|process| process.creation_time.is_none()));
        assert_eq!(
            *workload,
            std::collections::BTreeMap::from([(42, 1), (102, 2)])
        );
        for upgraded in [
            observations.processes().unwrap(),
            observations.processes_with_paths().unwrap(),
            observations.process_observations().unwrap(),
        ] {
            assert_eq!(upgraded[1].creation_time, None);
            assert_eq!(upgraded[2].creation_time, Some(2));
            for process in &upgraded[1..] {
                assert_eq!(process.session_id, None);
                assert!(process.image_path.is_none());
                assert_eq!(process.is_critical, None);
                assert!(!process.can_set_information);
            }
        }
        assert!(Arc::ptr_eq(
            &workload,
            &observations.workload_for_observation().unwrap()
        ));
        assert_eq!(PROCESS_INVENTORY_CALLS.load(Ordering::Relaxed), 1);
        assert_eq!(PROCESS_IDENTITY_CALLS.load(Ordering::Relaxed), 1);
        assert_eq!(PROCESS_CONTROL_CALLS.load(Ordering::Relaxed), 1);
        assert_eq!(PROCESS_PATH_CALLS.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn metadata_inventory_failure_is_shared_without_identity_or_control_queries() {
        let _guard = TEST_LOCK.lock().unwrap();
        fn unavailable() -> Result<Vec<ProcessInfo>, String> {
            PROCESS_INVENTORY_CALLS.fetch_add(1, Ordering::Relaxed);
            Err("metadata process snapshot failed".to_owned())
        }
        for counter in [
            &PROCESS_CALLS,
            &PROCESS_INVENTORY_CALLS,
            &PROCESS_OBSERVATION_CALLS,
            &PROCESS_IDENTITY_CALLS,
            &PROCESS_CONTROL_CALLS,
            &PROCESS_PATH_CALLS,
        ] {
            counter.store(0, Ordering::Relaxed);
        }
        let mut observations = CycleObservations::with_sources(ObservationSources {
            process_inventory: unavailable,
            ..sources()
        });
        for error in [
            observations.process_inventory().unwrap_err(),
            observations.process_inventory().unwrap_err(),
            observations.process_observations().unwrap_err(),
            observations.processes().unwrap_err(),
            observations.processes_with_paths().unwrap_err(),
        ] {
            assert_eq!(error, "metadata process snapshot failed");
        }
        assert_eq!(PROCESS_INVENTORY_CALLS.load(Ordering::Relaxed), 1);
        for counter in [
            &PROCESS_CALLS,
            &PROCESS_OBSERVATION_CALLS,
            &PROCESS_IDENTITY_CALLS,
            &PROCESS_CONTROL_CALLS,
            &PROCESS_PATH_CALLS,
        ] {
            assert_eq!(counter.load(Ordering::Relaxed), 0);
        }
    }

    #[test]
    fn read_only_workload_observation_skips_control_and_path_collection() {
        let _guard = TEST_LOCK.lock().unwrap();
        for counter in [
            &PROCESS_CALLS,
            &PROCESS_INVENTORY_CALLS,
            &PROCESS_OBSERVATION_CALLS,
            &PROCESS_IDENTITY_CALLS,
            &PROCESS_CONTROL_CALLS,
            &PROCESS_PATH_CALLS,
            &FOREGROUND_ID_CALLS,
            &FOREGROUND_PROCESS_CALLS,
            &VISIBLE_WINDOW_CALLS,
            &TOP_LEVEL_WINDOW_CALLS,
        ] {
            counter.store(0, Ordering::Relaxed);
        }
        let mut observations = CycleObservations::with_sources(sources());
        let first = observations.process_observations().unwrap();
        let second = observations.process_observations().unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        assert!(Arc::ptr_eq(
            &first,
            &observations.process_inventory().unwrap()
        ));
        assert!(observations.process_identity_observed);
        assert_eq!(first[0].creation_time, Some(1));
        assert_eq!(first[0].session_id, None);
        assert_eq!(first[0].is_critical, None);
        assert!(!first[0].can_set_information);
        assert!(first[0].image_path.is_none());
        let workload = observations.workload_for_observation().unwrap();
        assert_eq!(*workload, std::collections::BTreeMap::from([(42, 1)]));
        assert!(Arc::ptr_eq(
            &workload,
            &observations.workload_for_observation().unwrap()
        ));
        assert_eq!(PROCESS_OBSERVATION_CALLS.load(Ordering::Relaxed), 1);
        assert_eq!(FOREGROUND_ID_CALLS.load(Ordering::Relaxed), 1);
        for counter in [
            &PROCESS_CALLS,
            &PROCESS_INVENTORY_CALLS,
            &PROCESS_IDENTITY_CALLS,
            &PROCESS_CONTROL_CALLS,
            &PROCESS_PATH_CALLS,
            &FOREGROUND_PROCESS_CALLS,
            &VISIBLE_WINDOW_CALLS,
            &TOP_LEVEL_WINDOW_CALLS,
        ] {
            assert_eq!(counter.load(Ordering::Relaxed), 0);
        }
    }

    #[test]
    fn process_priority_no_match_uses_metadata_without_identity_or_control_enrichment() {
        use crate::{
            config::{ProcessExclusionRule, ProcessPrioritySetting, ProcessPrioritySettings},
            control::priority_efficiency::PriorityEfficiencyController,
            features::priority_control::process_priority::ProcessPriorityManager,
        };

        fn metadata() -> Result<Vec<ProcessInfo>, String> {
            Ok(vec![ProcessInfo {
                id: 42,
                creation_time: None,
                parent_id: None,
                session_id: None,
                user_name: None,
                is_service_account: None,
                is_critical: None,
                can_set_information: false,
                name: "unrelated.exe".to_owned(),
                image_path: None,
            }])
        }
        fn unexpected_full_inventory() -> Result<Vec<ProcessInfo>, String> {
            panic!("a no-match priority scan must not request process identities or eligibility")
        }
        fn unexpected_enrichment(_: &mut [ProcessInfo]) {
            panic!("a no-match priority scan must not enrich identities, eligibility or paths")
        }

        let mut observations = CycleObservations::with_sources(ObservationSources {
            processes: unexpected_full_inventory,
            process_inventory: metadata,
            process_observations: unexpected_full_inventory,
            enrich_process_identities: unexpected_enrichment,
            enrich_process_control: unexpected_enrichment,
            enrich_process_paths: unexpected_enrichment,
            ..sources()
        });
        let settings = ProcessPrioritySettings {
            enabled: true,
            foreground_detection_enabled: false,
            foreground_priority: ProcessPrioritySetting::Default,
            visible_window_detection_enabled: false,
            visible_window_priority: ProcessPrioritySetting::Default,
            background_priority: ProcessPrioritySetting::Default,
            exclusions: vec![ProcessExclusionRule {
                enabled: true,
                executable_path: r"C:\WinderustTest\missing-rule-only.exe".to_owned(),
                process_background_priority: Some(ProcessPrioritySetting::BelowNormal),
                ..Default::default()
            }],
            ..Default::default()
        };
        let mut action_log = ActionLog::default();
        let snapshot = ProcessPriorityManager::default().update(
            &mut PriorityEfficiencyController::default(),
            &settings,
            true,
            true,
            None,
            &BTreeSet::new(),
            &mut observations,
            &mut action_log,
        );

        assert!(snapshot.enabled);
        assert_eq!(snapshot.scanned_processes, 1);
        assert_eq!(snapshot.adjusted_processes, 0);
        assert_eq!(snapshot.skipped_processes, 0);
        assert_eq!(snapshot.failed_processes, 0);
        assert!(snapshot.adjusted_apps.is_empty());
        assert!(snapshot.auto_excluded_processes.is_empty());
        assert!(snapshot.last_error.is_none());
        assert_eq!(snapshot.message, "Process priority defaults active.");
        assert!(action_log.entries().is_empty());
        assert!(!observations.process_identity_observed);
        assert!(!observations.process_control_enriched);
        assert!(!observations.process_paths_enriched);
    }

    #[test]
    fn control_upgrade_reuses_inventory_and_captured_helper_workload() {
        let _guard = TEST_LOCK.lock().unwrap();
        fn initial() -> Result<Vec<ProcessInfo>, String> {
            let root = process_observations()?.remove(0);
            Ok(vec![
                root.clone(),
                ProcessInfo {
                    id: 101,
                    parent_id: Some(42),
                    creation_time: Some(2),
                    ..root.clone()
                },
                ProcessInfo {
                    id: 102,
                    parent_id: Some(42),
                    creation_time: None,
                    ..root.clone()
                },
                ProcessInfo {
                    id: 103,
                    parent_id: Some(42),
                    creation_time: Some(3),
                    ..root
                },
            ])
        }
        fn enrich(records: &mut [ProcessInfo]) {
            enrich_control(records);
            records[2].creation_time = Some(4);
            records[3].creation_time = Some(5);
        }
        fn paths(records: &mut [ProcessInfo]) {
            // Paths cannot run on the cheap table before control enrichment.
            assert!(records[0].can_set_information);
            assert!(records[1].can_set_information);
            assert!(!records[2].can_set_information);
            assert!(!records[3].can_set_information);
            enrich_paths(records);
            records[2].creation_time = Some(4);
            records[3].creation_time = Some(5);
        }
        for counter in [
            &PROCESS_CALLS,
            &PROCESS_OBSERVATION_CALLS,
            &PROCESS_CONTROL_CALLS,
            &PROCESS_PATH_CALLS,
        ] {
            counter.store(0, Ordering::Relaxed);
        }
        let mut observations = CycleObservations::with_sources(ObservationSources {
            process_observations: initial,
            enrich_process_control: enrich,
            enrich_process_paths: paths,
            ..sources()
        });
        let workload = observations.workload_for_observation().unwrap();
        assert_eq!(
            *workload,
            std::collections::BTreeMap::from([(42, 1), (101, 2), (103, 3)])
        );
        let cheap = observations.process_observations().unwrap();
        let controlled = observations.processes().unwrap();
        assert!(!cheap[0].can_set_information);
        assert!(controlled[0].can_set_information);
        assert!(controlled[1].can_set_information);
        assert_eq!(controlled[2].creation_time, None);
        assert_eq!(controlled[3].creation_time, Some(3));
        for process in &controlled[2..] {
            assert_eq!(process.session_id, None);
            assert_eq!(process.is_critical, None);
            assert!(!process.can_set_information);
        }
        let paths = observations.processes_with_paths().unwrap();
        assert!(paths[0].image_path.is_some());
        assert!(paths[2..]
            .iter()
            .all(|process| process.image_path.is_none() && !process.can_set_information));
        assert!(Arc::ptr_eq(
            &workload,
            &observations.adaptive_workload(&paths)
        ));
        assert!(Arc::ptr_eq(
            &workload,
            &observations.workload_for_observation().unwrap()
        ));
        assert_eq!(PROCESS_OBSERVATION_CALLS.load(Ordering::Relaxed), 1);
        assert_eq!(PROCESS_CONTROL_CALLS.load(Ordering::Relaxed), 1);
        assert_eq!(PROCESS_PATH_CALLS.load(Ordering::Relaxed), 1);
        assert_eq!(PROCESS_CALLS.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn unavailable_read_only_inventory_is_shared_by_later_control_requests() {
        let _guard = TEST_LOCK.lock().unwrap();
        fn unavailable() -> Result<Vec<ProcessInfo>, String> {
            PROCESS_OBSERVATION_CALLS.fetch_add(1, Ordering::Relaxed);
            Err("read-only process snapshot failed".to_owned())
        }
        for counter in [
            &PROCESS_CALLS,
            &PROCESS_OBSERVATION_CALLS,
            &PROCESS_CONTROL_CALLS,
            &PROCESS_PATH_CALLS,
        ] {
            counter.store(0, Ordering::Relaxed);
        }
        let mut observations = CycleObservations::with_sources(ObservationSources {
            process_observations: unavailable,
            ..sources()
        });
        assert!(observations.workload_for_observation().is_err());
        assert_eq!(
            observations.processes().unwrap_err(),
            "read-only process snapshot failed"
        );
        assert!(observations.processes_with_paths().is_err());
        assert!(observations.process_observations().is_err());
        assert_eq!(PROCESS_OBSERVATION_CALLS.load(Ordering::Relaxed), 1);
        assert_eq!(PROCESS_CALLS.load(Ordering::Relaxed), 0);
        assert_eq!(PROCESS_CONTROL_CALLS.load(Ordering::Relaxed), 0);
        assert_eq!(PROCESS_PATH_CALLS.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn pressure_monitor_uses_read_only_cycles_after_initial_control_cleanup() {
        use crate::{
            config::AdaptiveEngineProcessSettings,
            control::{
                cpu_allocation::CpuAllocationCoordinator,
                memory_priority::MemoryPriorityController,
                priority_efficiency::PriorityEfficiencyController,
            },
            features::winderust_features::adaptive_engine_process::{
                AdaptiveEngineProcessManager, AdaptiveEngineProcessUpdate,
            },
        };

        let _guard = TEST_LOCK.lock().unwrap();
        for counter in [
            &PROCESS_CALLS,
            &PROCESS_OBSERVATION_CALLS,
            &PROCESS_CONTROL_CALLS,
            &PROCESS_PATH_CALLS,
            &FOREGROUND_PROCESS_CALLS,
            &VISIBLE_WINDOW_CALLS,
            &TOP_LEVEL_WINDOW_CALLS,
        ] {
            counter.store(0, Ordering::Relaxed);
        }
        let settings = AdaptiveEngineProcessSettings {
            cpu_pressure_restraint_enabled: true,
            process_priority_enabled: false,
            background_efficiency_enabled: false,
            focus_process_background_efficiency_override_enabled: false,
            visible_window_background_efficiency_override_enabled: false,
            memory_priority_enabled: false,
            limit_background_processors_enabled: false,
            dynamic_resource_zones_enabled: false,
            io_priority: Default::default(),
            thread_priority: Default::default(),
            dynamic_priority_boost: Default::default(),
            gpu_priority: Default::default(),
            ..Default::default()
        };
        let mut manager = AdaptiveEngineProcessManager::default();
        let mut allocation = CpuAllocationCoordinator::default();
        let mut priority = PriorityEfficiencyController::default();
        let mut memory = MemoryPriorityController::default();
        let mut action_log = ActionLog::default();
        let excluded = BTreeSet::new();
        for (cycle, cpu, pressure) in [
            (0, Some(90.0), true),
            (1, None, true),
            (2, Some(72.0), true),
            (3, Some(10.0), false),
        ] {
            let mut observations = CycleObservations::with_sources(sources());
            let snapshot = manager.update(
                AdaptiveEngineProcessUpdate {
                    settings: &settings,
                    automation_enabled: true,
                    allow_cross_session_process_control: true,
                    foreground_process_id: Some(42),
                    total_cpu_usage_percent: cpu,
                    background_efficiency_managed: false,
                    excluded_process_ids: &excluded,
                    explicit_cpu_allocation_paths: &[],
                    observations: &mut observations,
                },
                &mut allocation,
                &mut priority,
                &mut memory,
                &mut action_log,
            );
            assert_eq!(snapshot.cpu_pressure_restraint_active, pressure);
            // The fake generation cannot resolve to a live CPU sample, including on cheap cycles.
            assert_eq!(snapshot.foreground_cpu_usage_tenths, None);
            assert_eq!(snapshot.adjusted_processes, 0);
            assert_eq!(snapshot.failed_processes, 0);
            let workload = observations.workload_for_observation().unwrap();
            assert_eq!(*workload, std::collections::BTreeMap::from([(42, 1)]));
            assert!(Arc::ptr_eq(
                &workload,
                &observations.workload_for_observation().unwrap()
            ));
            assert_eq!(PROCESS_CALLS.load(Ordering::Relaxed), 1);
            assert_eq!(PROCESS_OBSERVATION_CALLS.load(Ordering::Relaxed), cycle);
            assert_eq!(PROCESS_CONTROL_CALLS.load(Ordering::Relaxed), 0);
            assert_eq!(PROCESS_PATH_CALLS.load(Ordering::Relaxed), 0);
            assert_eq!(VISIBLE_WINDOW_CALLS.load(Ordering::Relaxed), 1);
            assert_eq!(TOP_LEVEL_WINDOW_CALLS.load(Ordering::Relaxed), 0);
            assert_eq!(FOREGROUND_PROCESS_CALLS.load(Ordering::Relaxed), 0);
        }
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
            ..sources()
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
        assert_eq!(result.adjusted_threads, 0);
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
            &PROCESS_INVENTORY_CALLS,
            &PROCESS_OBSERVATION_CALLS,
            &PROCESS_IDENTITY_CALLS,
            &PROCESS_CONTROL_CALLS,
            &PROCESS_PATH_CALLS,
            &FOREGROUND_ID_CALLS,
            &FOREGROUND_PROCESS_CALLS,
            &VISIBLE_WINDOW_CALLS,
            &TOP_LEVEL_WINDOW_CALLS,
        ] {
            counter.store(0, Ordering::Relaxed);
        }

        let mut observations = CycleObservations::with_sources(sources());
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
        assert!(observations.process_identity_observed);
        assert!(observations.process_inventory().unwrap()[0].can_set_information);
        assert!(observations.processes_with_paths().unwrap()[0]
            .image_path
            .is_some());
        assert!(observations.process_observations().unwrap()[0].can_set_information);
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
        assert_eq!(PROCESS_INVENTORY_CALLS.load(Ordering::Relaxed), 0);
        assert_eq!(PROCESS_OBSERVATION_CALLS.load(Ordering::Relaxed), 0);
        assert_eq!(PROCESS_IDENTITY_CALLS.load(Ordering::Relaxed), 0);
        assert_eq!(PROCESS_CONTROL_CALLS.load(Ordering::Relaxed), 0);
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
            foreground_process_id: || Some(7),
            ..sources()
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
            visible_window_process_ids: unavailable_visible_windows,
            ..sources()
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
