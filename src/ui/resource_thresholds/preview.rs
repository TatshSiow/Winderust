use std::{collections::HashMap, path::PathBuf, sync::Arc};

use crate::{
    config::ResourceThresholdsSelector,
    features::watchdog::resource_thresholds::{matching_path, selector_basename_matches},
    foreground::ProcessInfo,
    runtime::observations::CycleObservations,
};
use iced::widget::image;

#[derive(Debug, Clone)]
pub(crate) struct MatchRow {
    pub(super) pid: u32,
    pub(super) name: String,
    pub(super) path: PathBuf,
    pub(super) icon: Option<Arc<image::Handle>>,
}

pub(super) fn load(selector: &ResourceThresholdsSelector) -> Result<Vec<MatchRow>, String> {
    let mut observations = CycleObservations::default();
    let processes = observations.processes_for_path_matching(|process| {
        selector_basename_matches(selector, &process.name)
    })?;
    let mut matches = observed_matches(selector, &processes, std::process::id());
    let mut icons = HashMap::new();
    for entry in &mut matches {
        entry.icon = icons
            .entry(entry.path.clone())
            .or_insert_with(|| crate::process_icon::load_process_icon(&entry.path))
            .clone();
    }
    Ok(matches)
}

fn observed_matches(
    selector: &ResourceThresholdsSelector,
    processes: &[ProcessInfo],
    current_pid: u32,
) -> Vec<MatchRow> {
    let mut matches = processes
        .iter()
        .filter_map(|process| {
            if process.id == current_pid
                || process.creation_time.is_none()
                || process.is_critical != Some(false)
            {
                return None;
            }
            let path = process.image_path.as_ref()?;
            matching_path(selector, process)?;
            Some(MatchRow {
                pid: process.id,
                name: process.name.clone(),
                path: path.clone(),
                icon: None,
            })
        })
        .collect::<Vec<_>>();
    matches.sort_unstable_by_key(|entry| entry.pid);
    matches
}

#[cfg(test)]
mod tests {
    use super::*;

    fn process(id: u32, name: &str, path: Option<&str>) -> ProcessInfo {
        ProcessInfo {
            id,
            creation_time: Some(u64::from(id)),
            parent_id: None,
            session_id: None,
            user_name: None,
            is_service_account: None,
            is_critical: Some(false),
            can_set_information: false,
            name: name.into(),
            image_path: path.map(PathBuf::from),
        }
    }

    #[test]
    fn path_preview_only_includes_verified_instances_of_the_selected_executable() {
        let path = r"C:\Apps\worker.exe";
        let mut processes = [
            process(6, "worker.exe", Some(path)),
            process(2, "WORKER.EXE", Some("C:/Apps/worker.exe")),
            process(3, "worker.exe", Some(r"C:\Other\worker.exe")),
            process(4, "worker.exe", None),
            process(5, "worker.exe", Some(path)),
            process(1, "worker.exe", Some(path)),
            process(7, "worker.exe", Some(path)),
            process(8, "different.exe", Some(r"C:\Apps\different.exe")),
            process(9, "worker.exe", Some(path)),
        ];
        processes[0].is_critical = Some(true);
        processes[4].creation_time = None;
        processes[8].is_critical = None;

        let selector = ResourceThresholdsSelector::ExactPath {
            executable_path: path.into(),
        };
        let matches = observed_matches(&selector, &processes, 7);
        assert_eq!(
            matches
                .iter()
                .map(|entry| (entry.pid, entry.name.as_str(), entry.path.as_path()))
                .collect::<Vec<_>>(),
            [
                (1, "worker.exe", std::path::Path::new(path)),
                (2, "WORKER.EXE", std::path::Path::new("C:/Apps/worker.exe")),
            ]
        );
    }

    #[test]
    fn name_preview_only_includes_instances_with_a_matching_verified_absolute_path() {
        let path = r"C:\Apps\worker.exe";
        let other_path = r"D:\Tools\WORKER.EXE";
        let processes = [
            process(1, "worker.exe", Some(path)),
            process(2, "WORKER.EXE", Some(other_path)),
            process(3, "worker.exe", None),
            process(4, "worker.exe", Some(r"C:\Apps\different.exe")),
            process(5, "worker.exe", Some("worker.exe")),
            process(6, "different.exe", Some(path)),
            process(7, "worker.exe", Some("C:/Apps/worker.exe")),
        ];
        let selector = ResourceThresholdsSelector::ExactName {
            executable_name: "worker.exe".into(),
        };

        let matches = observed_matches(&selector, &processes, 0);
        assert_eq!(
            matches.iter().map(|entry| entry.pid).collect::<Vec<_>>(),
            [1, 2, 7]
        );
    }
}
