use crate::ui::app::*;

type ProcessCandidateRefresh = (Vec<ProcessCandidate>, HashMap<PathBuf, Option<Arc<Image>>>);
type RunningProcessRefresh = (
    Vec<ProcessInfo>,
    Vec<ProcessCandidate>,
    HashMap<PathBuf, Option<Arc<Image>>>,
    BTreeMap<u32, ProcessResourceSample>,
);

pub(in crate::ui::app) fn process_load_state_message(state: &ProcessLoadState) -> Option<String> {
    match state {
        ProcessLoadState::Loading => Some(t!("common.loading_running_apps").to_string()),
        ProcessLoadState::Failed(error) => {
            Some(t!("common.running_apps_load_failed", error = error).to_string())
        }
        ProcessLoadState::Paused => Some(t!("common.process_population_paused").to_string()),
        ProcessLoadState::Loaded => None,
    }
}

fn process_candidates_with_icons(
    processes: Vec<ProcessCandidateInfo>,
    mut icon_cache: HashMap<PathBuf, Option<Arc<Image>>>,
) -> (Vec<ProcessCandidate>, HashMap<PathBuf, Option<Arc<Image>>>) {
    let candidates = processes
        .into_iter()
        .map(|process| {
            let ProcessCandidateInfo { name, image_path } = process;
            let icon = if let Some(icon) = icon_cache.get(&image_path) {
                icon.clone()
            } else {
                let icon = load_process_icon(&image_path);
                icon_cache.insert(image_path.clone(), icon.clone());
                icon
            };
            ProcessCandidate {
                name,
                image_path,
                icon,
            }
        })
        .collect();
    (candidates, icon_cache)
}

impl WinderustApp {
    pub(in crate::ui::app) fn retain_current_process_icons<T>(
        cache: &mut HashMap<PathBuf, T>,
        candidates: &[ProcessCandidate],
    ) {
        if cache.is_empty() {
            return;
        }

        let current_paths = candidates
            .iter()
            .map(|candidate| candidate.image_path.as_path())
            .collect::<HashSet<_>>();
        cache.retain(|path, _| current_paths.contains(path.as_path()));
    }

    pub(in crate::ui::app) fn refresh_process_candidates(
        &mut self,
        report_status: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.settings.advanced.pause_process_population {
            let changed = self.process_candidate_load_state != ProcessLoadState::Paused;
            self.process_candidate_load_state = ProcessLoadState::Paused;
            return changed;
        }
        if self.process_refresh_in_progress {
            return false;
        }

        self.process_refresh_in_progress = true;
        self.next_process_refresh = Instant::now() + PROCESS_REFRESH_INTERVAL;
        let show_loading = self.process_candidates.is_empty() || report_status;
        if show_loading {
            self.process_candidate_load_state = ProcessLoadState::Loading;
        }
        let icon_cache = self.process_icon_cache.clone();
        let scan = cx.background_executor().spawn(async move {
            list_process_candidates()
                .map(|processes| process_candidates_with_icons(processes, icon_cache))
        });
        cx.spawn(async move |this, cx| {
            let result = scan.await;
            let _ = this.update(cx, |app, cx| {
                app.finish_process_candidate_refresh(result, report_status);
                cx.notify();
            });
        })
        .detach();
        true
    }

    fn finish_process_candidate_refresh(
        &mut self,
        result: Result<ProcessCandidateRefresh, String>,
        report_status: bool,
    ) {
        self.process_refresh_in_progress = false;
        if self.settings.advanced.pause_process_population {
            self.process_candidate_load_state = ProcessLoadState::Paused;
            return;
        }
        match result {
            Ok((processes, icon_cache)) => {
                self.process_candidates = processes;
                self.process_icon_cache = icon_cache;
                self.process_candidate_load_state = ProcessLoadState::Loaded;
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
                    self.status_message = message;
                }
            }
            Err(err) => {
                let load_state = ProcessLoadState::Failed(err.clone());
                self.status_message = err;
                self.process_candidate_load_state = load_state;
            }
        }
    }

    pub(in crate::ui::app) fn refresh_running_processes(
        &mut self,
        report_status: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.settings.advanced.pause_process_population {
            let changed = self.running_process_load_state != ProcessLoadState::Paused;
            self.running_process_load_state = ProcessLoadState::Paused;
            return changed;
        }
        if self.process_refresh_in_progress {
            return false;
        }

        self.process_refresh_in_progress = true;
        self.next_process_refresh = Instant::now() + PROCESS_LIST_REFRESH_INTERVAL;
        let show_loading = self.running_processes.is_empty() || report_status;
        if show_loading {
            self.running_process_load_state = ProcessLoadState::Loading;
        }
        let icon_cache = self.process_icon_cache.clone();
        let scan = cx.background_executor().spawn(async move {
            let processes = list_processes_with_paths()?;
            let candidate_info = process_candidates_from_processes(&processes);
            let (candidates, icon_cache) =
                process_candidates_with_icons(candidate_info, icon_cache);
            let resource_samples = sample_process_resources(&processes);
            Ok((processes, candidates, icon_cache, resource_samples))
        });
        cx.spawn(async move |this, cx| {
            let result = scan.await;
            let _ = this.update(cx, |app, cx| {
                app.finish_running_process_refresh(result, report_status);
                cx.notify();
            });
        })
        .detach();
        true
    }

    fn finish_running_process_refresh(
        &mut self,
        result: Result<RunningProcessRefresh, String>,
        report_status: bool,
    ) {
        self.process_refresh_in_progress = false;
        if self.settings.advanced.pause_process_population {
            self.running_process_load_state = ProcessLoadState::Paused;
            return;
        }
        match result {
            Ok((mut processes, candidates, icon_cache, resource_samples)) => {
                self.process_efficiency_mode_overrides.retain(
                    |process_id, (creation_time, _, _)| {
                        resource_samples
                            .get(process_id)
                            .is_some_and(|sample| sample.creation_time == *creation_time)
                    },
                );
                self.process_resource_usage = resource_samples
                    .iter()
                    .map(|(process_id, current)| {
                        let cpu_percent = self
                            .process_resource_samples
                            .get(process_id)
                            .filter(|previous| previous.creation_time == current.creation_time)
                            .and_then(|previous| {
                                process_cpu_usage_percent(previous.cpu, current.cpu)
                            });
                        (
                            *process_id,
                            ProcessResourceUsage {
                                cpu_percent,
                                working_set_bytes: current.working_set_bytes,
                                efficiency_mode: current.efficiency_mode,
                            },
                        )
                    })
                    .collect();
                self.process_resource_samples = resource_samples;
                processes.sort_by(|left, right| {
                    left.name
                        .cmp(&right.name)
                        .then_with(|| left.id.cmp(&right.id))
                });
                self.running_processes = processes;
                self.running_process_load_state = ProcessLoadState::Loaded;
                self.process_candidates = candidates;
                self.process_candidate_load_state = ProcessLoadState::Loaded;
                self.process_icon_cache = icon_cache;
                Self::retain_current_process_icons(
                    &mut self.process_icon_cache,
                    &self.process_candidates,
                );
                let expanded_group_count = self.expanded_process_list_groups.len();
                if expanded_group_count != 0 {
                    let active_group_keys = self
                        .running_processes
                        .iter()
                        .filter_map(|process| {
                            process
                                .image_path
                                .as_deref()
                                .map(process_list_executable_path_group_key)
                        })
                        .collect::<HashSet<_>>();
                    self.expanded_process_list_groups
                        .retain(|key| active_group_keys.contains(key));
                }
                if report_status {
                    let message = t!(
                        "status.loaded_running_processes",
                        count = self.running_processes.len()
                    )
                    .to_string();
                    self.status_message = message;
                }
            }
            Err(err) => {
                let load_state = ProcessLoadState::Failed(err.clone());
                self.status_message = err;
                self.running_process_load_state = load_state.clone();
                self.process_candidate_load_state = load_state;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_loading_states_have_user_feedback() {
        assert!(process_load_state_message(&ProcessLoadState::Loading).is_some());
        assert!(
            process_load_state_message(&ProcessLoadState::Failed("error".to_owned())).is_some()
        );
        assert!(process_load_state_message(&ProcessLoadState::Paused).is_some());
        assert!(process_load_state_message(&ProcessLoadState::Loaded).is_none());
    }
}
