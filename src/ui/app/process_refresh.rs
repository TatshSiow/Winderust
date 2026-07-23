use crate::ui::app::*;

impl WinderustApp {
    pub(in crate::ui::app) fn process_candidates_from_info(
        &mut self,
        processes: Vec<ProcessCandidateInfo>,
    ) -> Vec<ProcessCandidate> {
        processes
            .into_iter()
            .map(|process| {
                let icon = process
                    .image_path
                    .as_deref()
                    .and_then(|path| self.cached_process_icon(path));
                ProcessCandidate {
                    name: process.name,
                    image_path: process.image_path,
                    icon,
                }
            })
            .collect()
    }

    pub(in crate::ui::app) fn cached_process_icon(&mut self, path: &Path) -> Option<Arc<Image>> {
        if !self.process_icon_cache.contains_key(path) {
            let icon = load_process_icon(path);
            self.process_icon_cache.insert(path.to_path_buf(), icon);
        }

        self.process_icon_cache.get(path).and_then(Clone::clone)
    }

    pub(in crate::ui::app) fn retain_current_process_icons(
        cache: &mut HashMap<PathBuf, Option<Arc<Image>>>,
        candidates: &[ProcessCandidate],
    ) {
        if cache.is_empty() {
            return;
        }

        let current_paths = candidates
            .iter()
            .filter_map(|candidate| candidate.image_path.as_deref())
            .collect::<HashSet<_>>();
        let old_len = cache.len();
        cache.retain(|path, _| current_paths.contains(path.as_path()));
        if cache.len() != old_len {
            cache.shrink_to_fit();
        }
    }

    pub(in crate::ui::app) fn refresh_process_candidates(&mut self, report_status: bool) -> bool {
        self.next_process_refresh = Instant::now() + PROCESS_REFRESH_INTERVAL;
        match list_process_candidates() {
            Ok(processes) => {
                let processes = self.process_candidates_from_info(processes);
                let changed = self.process_candidates != processes;
                self.process_candidates = processes;
                Self::retain_current_process_icons(
                    &mut self.process_icon_cache,
                    &self.process_candidates,
                );
                if report_status {
                    let message = t!(
                        "status.loaded_running_apps",
                        count = self.process_candidates.len()
                    )
                    .to_string();
                    let status_changed = self.status_message != message;
                    self.status_message = message;
                    changed || status_changed
                } else {
                    changed
                }
            }
            Err(err) => {
                let changed = self.status_message != err;
                self.status_message = err;
                changed
            }
        }
    }

    pub(in crate::ui::app) fn refresh_running_processes(&mut self, report_status: bool) -> bool {
        self.next_process_refresh = Instant::now() + PROCESS_REFRESH_INTERVAL;
        match list_processes() {
            Ok(mut processes) => {
                processes.sort_by(|left, right| {
                    left.name
                        .cmp(&right.name)
                        .then_with(|| left.id.cmp(&right.id))
                });
                let changed = self.running_processes != processes;
                self.running_processes = processes;
                let expanded_group_count = self.expanded_process_list_groups.len();
                if expanded_group_count != 0 {
                    let active_group_keys = self
                        .running_processes
                        .iter()
                        .map(|process| process_list_group_key(&process.name))
                        .collect::<HashSet<_>>();
                    self.expanded_process_list_groups
                        .retain(|key| active_group_keys.contains(key));
                }
                let groups_changed =
                    self.expanded_process_list_groups.len() != expanded_group_count;
                if report_status {
                    let message = t!(
                        "status.loaded_running_processes",
                        count = self.running_processes.len()
                    )
                    .to_string();
                    let status_changed = self.status_message != message;
                    self.status_message = message;
                    changed || groups_changed || status_changed
                } else {
                    changed || groups_changed
                }
            }
            Err(err) => {
                let changed = self.status_message != err;
                self.status_message = err;
                changed
            }
        }
    }
}
