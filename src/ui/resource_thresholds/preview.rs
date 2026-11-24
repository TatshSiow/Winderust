use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use crate::{
    config::{ResourceThresholdsRule, ResourceThresholdsScope, ResourceThresholdsSelector},
    features::watchdog::resource_thresholds::{
        group_matches, matching_path, observed_path_matches_name, selector_basename_matches,
    },
    foreground::{same_executable_path, ProcessInfo},
    runtime::observations::CycleObservations,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum MatchKind {
    Matched(PathBuf),
    SameName,
    Unavailable(UnavailableReason),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UnavailableReason {
    PathUnavailable,
    PathUnverified,
    NameUnverified,
    StartTimeUnavailable,
    CriticalityUnknown,
    CriticalProcess,
    WinderustProcess,
}

impl UnavailableReason {
    pub(super) fn label_key(self) -> &'static str {
        match self {
            Self::PathUnavailable => "resource_thresholds.preview_path_unavailable",
            Self::PathUnverified => "resource_thresholds.preview_path_unverified",
            Self::NameUnverified => "resource_thresholds.preview_name_unverified",
            Self::StartTimeUnavailable => "resource_thresholds.preview_start_time_unavailable",
            Self::CriticalityUnknown => "resource_thresholds.preview_criticality_unknown",
            Self::CriticalProcess => "resource_thresholds.preview_critical_process",
            Self::WinderustProcess => "resource_thresholds.preview_self_skipped",
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct MatchRow {
    pub(crate) pid: u32,
    pub(crate) path: Option<PathBuf>,
    pub(crate) kind: MatchKind,
    pub(super) other_matching_rules: Vec<usize>,
}

pub(super) fn other_enabled_rules(
    rules: &[ResourceThresholdsRule],
    edited: Option<usize>,
) -> Vec<(usize, ResourceThresholdsSelector)> {
    rules
        .iter()
        .enumerate()
        .filter(|(index, rule)| Some(*index) != edited && rule.enabled)
        .map(|(index, rule)| (index, rule.selector.clone()))
        .collect()
}

pub(super) fn load(
    selector: &ResourceThresholdsSelector,
    other_rules: &[(usize, ResourceThresholdsSelector)],
) -> Result<Vec<MatchRow>, String> {
    let mut observations = CycleObservations::default();
    let processes = observations.processes_for_path_matching(|process| {
        selector_basename_matches(selector, &process.name)
    })?;
    Ok(classify(
        selector,
        other_rules,
        &processes,
        std::process::id(),
    ))
}

fn classify(
    selector: &ResourceThresholdsSelector,
    other_rules: &[(usize, ResourceThresholdsSelector)],
    processes: &[ProcessInfo],
    current_pid: u32,
) -> Vec<MatchRow> {
    use UnavailableReason::*;

    let expected_path = selector.exact_path().map(Path::new);
    // The preview only uses paths already observed for this request.
    let group_paths = group_matches(
        selector,
        ResourceThresholdsScope::Combined,
        processes
            .iter()
            .filter(|process| process.image_path.is_some()),
    )
    .into_iter()
    .flat_map(|((_, path), members)| members.into_iter().map(move |member| (member.id, path)))
    .collect::<BTreeMap<_, _>>();
    let mut rows = Vec::new();
    for process in processes {
        if !selector_basename_matches(selector, &process.name) {
            continue;
        }
        let evaluation_path = group_paths.get(&process.id).copied();
        let kind = match process.image_path.as_deref() {
            Some(actual_path)
                if expected_path.is_some_and(|path| !same_executable_path(actual_path, path)) =>
            {
                MatchKind::SameName
            }
            None => MatchKind::Unavailable(PathUnavailable),
            Some(_)
                if expected_path.is_none()
                    && !observed_path_matches_name(process, selector.value()) =>
            {
                MatchKind::Unavailable(NameUnverified)
            }
            Some(_) if process.id == current_pid => MatchKind::Unavailable(WinderustProcess),
            Some(_) if process.is_critical == Some(true) => MatchKind::Unavailable(CriticalProcess),
            Some(_) if process.creation_time.is_none() => {
                MatchKind::Unavailable(StartTimeUnavailable)
            }
            Some(_) if process.is_critical.is_none() => MatchKind::Unavailable(CriticalityUnknown),
            Some(_) => evaluation_path.map_or(MatchKind::Unavailable(PathUnverified), |path| {
                MatchKind::Matched(path.to_path_buf())
            }),
        };
        let other_matching_rules = if matches!(kind, MatchKind::Matched(_)) {
            other_rules
                .iter()
                .filter_map(|(index, selector)| matching_path(selector, process).map(|_| *index))
                .collect()
        } else {
            Vec::new()
        };
        rows.push(MatchRow {
            pid: process.id,
            path: process.image_path.clone(),
            kind,
            other_matching_rules,
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

        let selector = ResourceThresholdsSelector::ExactPath {
            executable_path: path.into(),
        };
        let enabled =
            ResourceThresholdsRule::with_selector(ResourceThresholdsSelector::ExactName {
                executable_name: "worker.exe".into(),
            });
        let mut disabled = ResourceThresholdsRule::with_selector(selector.clone());
        disabled.enabled = false;
        let edited = ResourceThresholdsRule::with_selector(selector.clone());
        let elsewhere = ResourceThresholdsRule::new(other_path.into());
        let overlapping = ResourceThresholdsRule::new(path.into());
        let others = other_enabled_rules(
            &[enabled, disabled, edited, elsewhere, overlapping],
            Some(2),
        );
        let rows = classify(&selector, &others, &processes, 7);
        assert_eq!(
            rows.iter()
                .map(|row| (row.pid, row.kind.clone()))
                .collect::<Vec<_>>(),
            [
                (1, MatchKind::Matched(path.into())),
                (2, MatchKind::Matched(path.into())),
                (3, MatchKind::SameName),
                (4, MatchKind::Unavailable(PathUnavailable)),
                (5, MatchKind::Unavailable(StartTimeUnavailable)),
                (6, MatchKind::Unavailable(CriticalProcess)),
                (7, MatchKind::Unavailable(WinderustProcess)),
                (9, MatchKind::Unavailable(CriticalityUnknown)),
            ]
        );
        assert_eq!(
            rows.iter()
                .map(|row| row.other_matching_rules.as_slice())
                .collect::<Vec<_>>(),
            [&[0, 4][..], &[0, 4], &[], &[], &[], &[], &[], &[]]
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
            process(7, "worker.exe", Some("C:/Apps/worker.exe")),
        ];
        let selector = ResourceThresholdsSelector::ExactName {
            executable_name: "worker.exe".into(),
        };

        let others = other_enabled_rules(&[ResourceThresholdsRule::new(path.into())], None);
        let rows = classify(&selector, &others, &processes, 0);
        assert_eq!(
            rows.iter()
                .map(|row| (row.pid, row.kind.clone()))
                .collect::<Vec<_>>(),
            [
                (1, MatchKind::Matched(path.into())),
                (2, MatchKind::Matched(other_path.into())),
                (3, MatchKind::Unavailable(PathUnavailable)),
                (4, MatchKind::Unavailable(NameUnverified)),
                (5, MatchKind::Unavailable(NameUnverified)),
                (7, MatchKind::Matched(path.into())),
            ]
        );
        assert_eq!(
            rows.iter()
                .map(|row| row.other_matching_rules.as_slice())
                .collect::<Vec<_>>(),
            [&[0][..], &[], &[], &[], &[], &[0]]
        );
        assert_eq!(rows[0].path.as_deref(), Some(Path::new(path)));
        assert_eq!(rows[1].path.as_deref(), Some(Path::new(other_path)));
    }
}
