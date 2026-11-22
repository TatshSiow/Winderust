use std::path::{Path, PathBuf};

use crate::{
    config::WatchdogSelector,
    features::winderust_features::watchdog::observed_path_matches_name,
    foreground::{same_executable_path, same_process_name, ProcessInfo},
    runtime::observations::CycleObservations,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MatchKind {
    Matched,
    SameName,
    Unavailable(UnavailableReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnavailableReason {
    PathUnavailable,
    NameUnverified,
    StartTimeUnavailable,
    CriticalityUnknown,
    CriticalProcess,
    WinderustProcess,
}

impl UnavailableReason {
    pub(super) fn label_key(self) -> &'static str {
        match self {
            Self::PathUnavailable => "watchdog.preview_path_unavailable",
            Self::NameUnverified => "watchdog.preview_name_unverified",
            Self::StartTimeUnavailable => "watchdog.preview_start_time_unavailable",
            Self::CriticalityUnknown => "watchdog.preview_criticality_unknown",
            Self::CriticalProcess => "watchdog.preview_critical_process",
            Self::WinderustProcess => "watchdog.preview_self_skipped",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct MatchRow {
    pub(crate) pid: u32,
    pub(crate) path: Option<PathBuf>,
    pub(crate) kind: MatchKind,
}

pub(super) fn load(selector: &WatchdogSelector) -> Result<Vec<MatchRow>, String> {
    let processes = CycleObservations::default().processes_with_paths()?;
    Ok(classify(selector, &processes, std::process::id()))
}

fn classify(
    selector: &WatchdogSelector,
    processes: &[ProcessInfo],
    current_pid: u32,
) -> Vec<MatchRow> {
    use UnavailableReason::*;

    let (name, expected_path) = match selector {
        WatchdogSelector::ExactPath { executable_path } => {
            let path = Path::new(executable_path.trim());
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                return Vec::new();
            };
            (name, Some(path))
        }
        WatchdogSelector::ExactName { executable_name } => (executable_name.as_str(), None),
    };

    let mut rows = Vec::new();
    for process in processes {
        if !same_process_name(&process.name, name) {
            continue;
        }
        let kind = match process.image_path.as_deref() {
            Some(actual_path)
                if expected_path.is_some_and(|path| !same_executable_path(actual_path, path)) =>
            {
                MatchKind::SameName
            }
            None => MatchKind::Unavailable(PathUnavailable),
            Some(_) if expected_path.is_none() && !observed_path_matches_name(process, name) => {
                MatchKind::Unavailable(NameUnverified)
            }
            Some(_) if process.id == current_pid => MatchKind::Unavailable(WinderustProcess),
            Some(_) if process.is_critical == Some(true) => MatchKind::Unavailable(CriticalProcess),
            Some(_) if process.creation_time.is_none() => {
                MatchKind::Unavailable(StartTimeUnavailable)
            }
            Some(_) if process.is_critical.is_none() => MatchKind::Unavailable(CriticalityUnknown),
            Some(_) => MatchKind::Matched,
        };
        rows.push(MatchRow {
            pid: process.id,
            path: process.image_path.clone(),
            kind,
        });
    }
    rows.sort_unstable_by_key(|row| row.pid);
    rows
}

#[cfg(test)]
mod tests {
    use super::{UnavailableReason::*, *};

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
    fn preview_lists_each_pid_and_separates_other_paths_and_unavailable_instances() {
        let path = r"C:\Apps\worker.exe";
        let other_path = r"C:\Other\worker.exe";
        let mut processes = [
            process(6, "worker.exe", Some(path)),
            process(2, "WORKER.EXE", Some("C:/Apps/worker.exe")),
            process(3, "worker.exe", Some(other_path)),
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

        let selector = WatchdogSelector::ExactPath {
            executable_path: path.into(),
        };
        let rows = classify(&selector, &processes, 7);
        assert_eq!(
            rows.iter()
                .map(|row| (row.pid, row.kind))
                .collect::<Vec<_>>(),
            [
                (1, MatchKind::Matched),
                (2, MatchKind::Matched),
                (3, MatchKind::SameName),
                (4, MatchKind::Unavailable(PathUnavailable)),
                (5, MatchKind::Unavailable(StartTimeUnavailable)),
                (6, MatchKind::Unavailable(CriticalProcess)),
                (7, MatchKind::Unavailable(WinderustProcess)),
                (9, MatchKind::Unavailable(CriticalityUnknown)),
            ]
        );
        assert_eq!(rows[2].path.as_deref(), Some(Path::new(other_path)));
        assert_eq!(rows[3].path, None);
        assert_eq!(rows[4].path.as_deref(), Some(Path::new(path)));
    }

    #[test]
    fn name_preview_lists_verified_paths_and_marks_unverified_instances_unavailable() {
        let path = r"C:\Apps\worker.exe";
        let other_path = r"D:\Tools\WORKER.EXE";
        let processes = [
            process(1, "worker.exe", Some(path)),
            process(2, "WORKER.EXE", Some(other_path)),
            process(3, "worker.exe", None),
            process(4, "worker.exe", Some(r"C:\Apps\different.exe")),
            process(5, "worker.exe", Some("worker.exe")),
            process(6, "different.exe", Some(path)),
        ];
        let selector = WatchdogSelector::ExactName {
            executable_name: "worker.exe".into(),
        };

        let rows = classify(&selector, &processes, 0);
        assert_eq!(
            rows.iter()
                .map(|row| (row.pid, row.kind))
                .collect::<Vec<_>>(),
            [
                (1, MatchKind::Matched),
                (2, MatchKind::Matched),
                (3, MatchKind::Unavailable(PathUnavailable)),
                (4, MatchKind::Unavailable(NameUnverified)),
                (5, MatchKind::Unavailable(NameUnverified)),
            ]
        );
        assert_eq!(rows[0].path.as_deref(), Some(Path::new(path)));
        assert_eq!(rows[1].path.as_deref(), Some(Path::new(other_path)));
    }
}
