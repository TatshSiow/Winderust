use std::path::{Path, PathBuf};

use crate::{
    foreground::{same_executable_path, same_process_name, ProcessInfo},
    runtime::observations::CycleObservations,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MatchKind {
    ExactPath,
    SameName,
    Unavailable,
}

#[derive(Debug, Clone)]
pub(crate) struct MatchRow {
    pub(crate) pid: u32,
    pub(crate) path: Option<PathBuf>,
    pub(crate) kind: MatchKind,
}

pub(super) fn load(executable_path: &str) -> Result<Vec<MatchRow>, String> {
    let processes = CycleObservations::default().processes_with_paths()?;
    Ok(classify(
        Path::new(executable_path.trim()),
        &processes,
        std::process::id(),
    ))
}

fn classify(path: &Path, processes: &[ProcessInfo], current_pid: u32) -> Vec<MatchRow> {
    let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
        return Vec::new();
    };

    let mut rows = Vec::new();
    for process in processes {
        if !same_process_name(&process.name, name) {
            continue;
        }
        let kind = match process.image_path.as_deref() {
            Some(actual_path) if !same_executable_path(actual_path, path) => MatchKind::SameName,
            Some(_)
                if process.is_critical == Some(false)
                    && process.creation_time.is_some()
                    && process.id != current_pid =>
            {
                MatchKind::ExactPath
            }
            _ => MatchKind::Unavailable,
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
        ];
        processes[0].is_critical = Some(true);
        processes[4].creation_time = None;

        let rows = classify(Path::new(path), &processes, 7);
        assert_eq!(
            rows.iter()
                .map(|row| (row.pid, row.kind))
                .collect::<Vec<_>>(),
            [
                (1, MatchKind::ExactPath),
                (2, MatchKind::ExactPath),
                (3, MatchKind::SameName),
                (4, MatchKind::Unavailable),
                (5, MatchKind::Unavailable),
                (6, MatchKind::Unavailable),
                (7, MatchKind::Unavailable),
            ]
        );
        assert_eq!(rows[2].path.as_deref(), Some(Path::new(other_path)));
        assert_eq!(rows[3].path, None);
        assert_eq!(rows[4].path.as_deref(), Some(Path::new(path)));
    }
}
