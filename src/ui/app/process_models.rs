use crate::ui::app::*;

pub(in crate::ui::app) struct ProcessCatalogModel {
    pub(in crate::ui::app) candidates: Vec<ProcessCandidate>,
    pub(in crate::ui::app) load_state: ProcessLoadState,
    pub(in crate::ui::app) selected_paths: HashMap<SuggestionTarget, String>,
    pub(in crate::ui::app) refresh_in_progress: bool,
    pub(in crate::ui::app) icon_cache: HashMap<PathBuf, Option<Arc<Image>>>,
}

impl ProcessCatalogModel {
    pub(in crate::ui::app) fn new(load_state: ProcessLoadState) -> Self {
        Self {
            candidates: Vec::new(),
            load_state,
            selected_paths: HashMap::new(),
            refresh_in_progress: false,
            icon_cache: HashMap::new(),
        }
    }
}

pub(in crate::ui::app) struct ProcessListModel {
    pub(in crate::ui::app) processes: Vec<ProcessInfo>,
    pub(in crate::ui::app) resource_samples: BTreeMap<u32, ProcessResourceSample>,
    pub(in crate::ui::app) resource_usage: HashMap<u32, ProcessResourceUsage>,
    pub(in crate::ui::app) hide_inaccessible: bool,
    pub(in crate::ui::app) load_state: ProcessLoadState,
    pub(in crate::ui::app) refresh_in_progress: bool,
    pub(in crate::ui::app) expanded_groups: HashSet<String>,
    pub(in crate::ui::app) sort: ProcessListSort,
    pub(in crate::ui::app) selected_process_id: Option<u32>,
    pub(in crate::ui::app) details: Option<ProcessDetailsDraft>,
}

impl ProcessListModel {
    pub(in crate::ui::app) fn new(load_state: ProcessLoadState) -> Self {
        Self {
            processes: Vec::new(),
            resource_samples: BTreeMap::new(),
            resource_usage: HashMap::new(),
            hide_inaccessible: true,
            load_state,
            refresh_in_progress: false,
            expanded_groups: HashSet::new(),
            sort: ProcessListSort::default(),
            selected_process_id: None,
            details: None,
        }
    }

    pub(in crate::ui::app) fn clear_population(&mut self, load_state: ProcessLoadState) {
        self.processes.clear();
        self.resource_samples.clear();
        self.resource_usage.clear();
        self.expanded_groups.clear();
        self.selected_process_id = None;
        self.details = None;
        self.load_state = load_state;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_and_process_list_refreshes_are_independent() {
        let mut catalog = ProcessCatalogModel::new(ProcessLoadState::Loading);
        let mut process_list = ProcessListModel::new(ProcessLoadState::Loading);

        catalog.refresh_in_progress = true;
        assert!(!process_list.refresh_in_progress);

        process_list.refresh_in_progress = true;
        assert!(catalog.refresh_in_progress);
    }

    #[test]
    fn clearing_population_preserves_an_in_flight_refresh() {
        let mut process_list = ProcessListModel::new(ProcessLoadState::Loaded);
        process_list.refresh_in_progress = true;
        process_list.expanded_groups.insert("c:/app.exe".to_owned());
        process_list.selected_process_id = Some(42);
        process_list.details = Some(ProcessDetailsDraft {
            display_name: "app.exe".to_owned(),
            executable_path: "C:/app.exe".to_owned(),
        });

        process_list.clear_population(ProcessLoadState::Paused);

        assert!(process_list.refresh_in_progress);
        assert!(process_list.expanded_groups.is_empty());
        assert_eq!(process_list.selected_process_id, None);
        assert!(process_list.details.is_none());
        assert_eq!(process_list.load_state, ProcessLoadState::Paused);
    }
}
