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
            process_paths_enriched: false,
            foreground_process_id: Observation::NotRequested,
            foreground_process: Observation::NotRequested,
            visible_window_process_ids: Observation::NotRequested,
            top_level_window_process_ids: Observation::NotRequested,
        }
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
            (self.sources.enrich_process_paths)(Arc::make_mut(processes));
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
