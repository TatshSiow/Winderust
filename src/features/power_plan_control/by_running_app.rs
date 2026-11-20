use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use crate::{
    config::{ByRunningAppRule, ByRunningAppSettings, ExecutableSelector},
    foreground::{
        contains_process_name, process_executable_path, process_session_id, same_executable_path,
        same_process_name, ProcessInfo, EXTENDED_BUILT_IN_PROCESS_EXCLUSIONS,
    },
    runtime::observations::CycleObservations,
};

const BUILT_IN_EXCLUSIONS: &[&str] = EXTENDED_BUILT_IN_PROCESS_EXCLUSIONS;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ByRunningAppSnapshot {
    pub observation_known: bool,
    pub active_rule: Option<String>,
    pub active_process: Option<String>,
    pub target_guid: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ByRunningAppPreviewMatch {
    Match,
    SameNameDifferentPath,
    Unavailable,
}

#[derive(Debug)]
pub(crate) struct ByRunningAppPreviewRow {
    pub(crate) pid: u32,
    pub(crate) path: Option<PathBuf>,
    pub(crate) kind: ByRunningAppPreviewMatch,
}

#[derive(Debug)]
pub(crate) struct ByRunningAppPreview {
    pub(crate) rules: Vec<Vec<ByRunningAppPreviewRow>>,
    pub(crate) winner: Option<usize>,
}

#[derive(Default)]
pub struct ByRunningAppManager {
    active: Option<ActiveByRunningApp>,
    observation_known: bool,
}

#[derive(Debug, Clone)]
struct ActiveByRunningApp {
    rule_index: usize,
    rule_name: String,
    process_id: u32,
    process_name: String,
    target_guid: String,
}

impl ByRunningAppManager {
    pub fn update(
        &mut self,
        settings: &ByRunningAppSettings,
        automation_enabled: bool,
        observations: &mut CycleObservations,
    ) -> ByRunningAppSnapshot {
        self.observation_known = true;
        if !automation_enabled
            || !settings.enabled
            || !settings
                .rules
                .iter()
                .any(|rule| rule.configured_plan().is_some())
        {
            self.release();
            return self.snapshot();
        }

        self.observation_known = false;
        let current_process_id = std::process::id();
        let Some(current_session_id) = process_session_id(current_process_id) else {
            self.release();
            return self.snapshot();
        };

        let processes = match observations.processes() {
            Ok(processes) => processes,
            Err(_) => {
                self.release();
                return self.snapshot();
            }
        };
        let (matched, known) = matching_rule_process(
            settings,
            &processes,
            current_process_id,
            current_session_id,
            process_session_id,
            process_executable_path,
        );
        self.observation_known = known;

        let Some(matched) = matched else {
            self.release();
            return self.snapshot();
        };

        if self.active_matches(&matched) {
            return self.snapshot();
        }

        self.active = Some(matched);
        self.snapshot()
    }

    pub(crate) fn observation_known(&self) -> bool {
        self.observation_known
    }

    pub fn active_process_ids(&self) -> BTreeSet<u32> {
        self.active
            .as_ref()
            .map(|active| BTreeSet::from([active.process_id]))
            .unwrap_or_default()
    }

    pub fn active_decision(&self) -> Option<(usize, String, String, String)> {
        self.active.as_ref().map(|active| {
            (
                active.rule_index,
                active.rule_name.clone(),
                active.process_name.clone(),
                active.target_guid.clone(),
            )
        })
    }

    fn active_matches(&self, matched: &ActiveByRunningApp) -> bool {
        self.active.as_ref().is_some_and(|active| {
            active.rule_index == matched.rule_index
                && active.rule_name == matched.rule_name
                && active.process_id == matched.process_id
                && same_process_name(&active.process_name, &matched.process_name)
                && active
                    .target_guid
                    .eq_ignore_ascii_case(&matched.target_guid)
        })
    }

    fn release(&mut self) {
        self.active = None;
    }

    fn snapshot(&self) -> ByRunningAppSnapshot {
        self.active.as_ref().map_or_else(
            || ByRunningAppSnapshot {
                observation_known: self.observation_known,
                ..ByRunningAppSnapshot::default()
            },
            |active| ByRunningAppSnapshot {
                observation_known: self.observation_known,
                active_rule: Some(active.rule_name.clone()),
                active_process: Some(active.process_name.clone()),
                target_guid: Some(active.target_guid.clone()),
            },
        )
    }
}

pub fn is_builtin_excluded(process_name: &str) -> bool {
    contains_process_name(BUILT_IN_EXCLUSIONS, process_name)
}

pub(crate) fn preview_matches(
    settings: &ByRunningAppSettings,
) -> Result<ByRunningAppPreview, String> {
    let current_process_id = std::process::id();
    let current_session_id = process_session_id(current_process_id)
        .ok_or_else(|| "Could not determine the current Windows session.".to_owned())?;
    let processes = CycleObservations::default().processes()?;
    Ok(preview_rule_processes(
        settings,
        &processes,
        current_process_id,
        current_session_id,
        process_session_id,
        process_executable_path,
    ))
}

fn preview_rule_processes(
    settings: &ByRunningAppSettings,
    processes: &[ProcessInfo],
    current_process_id: u32,
    current_session_id: u32,
    mut session_id: impl FnMut(u32) -> Option<u32>,
    mut executable_path: impl FnMut(&ProcessInfo) -> Option<PathBuf>,
) -> ByRunningAppPreview {
    let mut winner = None;
    let rules = settings
        .rules
        .iter()
        .enumerate()
        .map(|(rule_index, rule)| {
            let Some((file_name, configured_path)) = rule_executable(rule) else {
                return Vec::new();
            };
            let mut rows: Vec<_> = processes
                .iter()
                .filter_map(|process| {
                    classify_process(
                        process,
                        configured_path,
                        file_name,
                        current_process_id,
                        current_session_id,
                        &mut session_id,
                        &mut executable_path,
                    )
                })
                .collect();
            if winner.is_none()
                && rule.configured_plan().is_some()
                && rows
                    .iter()
                    .any(|row| row.kind == ByRunningAppPreviewMatch::Match)
            {
                winner = Some(rule_index);
            }
            rows.sort_unstable_by_key(|row| row.pid);
            rows
        })
        .collect();
    ByRunningAppPreview { rules, winner }
}

fn matching_rule_process(
    settings: &ByRunningAppSettings,
    processes: &[ProcessInfo],
    current_process_id: u32,
    current_session_id: u32,
    mut session_id: impl FnMut(u32) -> Option<u32>,
    mut executable_path: impl FnMut(&ProcessInfo) -> Option<PathBuf>,
) -> (Option<ActiveByRunningApp>, bool) {
    let mut known = true;
    for (rule_index, rule) in settings.rules.iter().enumerate() {
        let Some(target_guid) = rule.configured_plan() else {
            continue;
        };
        let Some((file_name, configured_path)) = rule_executable(rule) else {
            continue;
        };

        for process in processes {
            let kind = classify_process(
                process,
                configured_path,
                file_name,
                current_process_id,
                current_session_id,
                &mut session_id,
                &mut executable_path,
            )
            .map(|row| row.kind);
            match kind {
                Some(ByRunningAppPreviewMatch::Match) => {
                    return (
                        Some(ActiveByRunningApp {
                            rule_index,
                            rule_name: rule_name(rule).to_owned(),
                            process_id: process.id,
                            process_name: process.name.clone(),
                            target_guid: target_guid.to_owned(),
                        }),
                        true,
                    );
                }
                Some(ByRunningAppPreviewMatch::Unavailable) => known = false,
                Some(ByRunningAppPreviewMatch::SameNameDifferentPath) | None => {}
            }
        }
    }

    (None, known)
}

fn rule_executable(rule: &ByRunningAppRule) -> Option<(&str, Option<&Path>)> {
    let value = rule.selector.value().trim();
    match &rule.selector {
        ExecutableSelector::ExactPath { .. } => {
            let path = Path::new(value);
            Some((path.file_name()?.to_str()?, Some(path)))
        }
        ExecutableSelector::ExactName { .. } => (!value.is_empty()).then_some((value, None)),
    }
}

fn classify_process(
    process: &ProcessInfo,
    configured_path: Option<&Path>,
    file_name: &str,
    current_process_id: u32,
    current_session_id: u32,
    session_id: &mut impl FnMut(u32) -> Option<u32>,
    executable_path: &mut impl FnMut(&ProcessInfo) -> Option<PathBuf>,
) -> Option<ByRunningAppPreviewRow> {
    if process.id == 0
        || process.id == current_process_id
        || process.is_critical == Some(true)
        || !same_process_name(&process.name, file_name)
        || is_builtin_excluded(&process.name)
    {
        return None;
    }
    let session = session_id(process.id);
    if session.is_some_and(|session| session != current_session_id) {
        return None;
    }
    let path = executable_path(process);
    let kind = match (path.as_deref(), configured_path) {
        (Some(path), Some(expected_path)) if !same_executable_path(path, expected_path) => {
            ByRunningAppPreviewMatch::SameNameDifferentPath
        }
        (Some(path), None) if !path_has_executable_name(path, file_name) => {
            ByRunningAppPreviewMatch::Unavailable
        }
        (Some(_), _) if process.is_critical == Some(false) && session.is_some() => {
            ByRunningAppPreviewMatch::Match
        }
        _ => ByRunningAppPreviewMatch::Unavailable,
    };
    Some(ByRunningAppPreviewRow {
        pid: process.id,
        path,
        kind,
    })
}

fn path_has_executable_name(path: &Path, executable_name: &str) -> bool {
    path.is_absolute()
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| same_process_name(name, executable_name))
}

pub(crate) fn rule_name(rule: &ByRunningAppRule) -> &str {
    let name = rule.name.trim();
    if name.is_empty() {
        rule.selector.value().trim()
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn observed_process(
        id: u32,
        name: &str,
        path: Option<&str>,
        is_critical: Option<bool>,
    ) -> ProcessInfo {
        ProcessInfo {
            id,
            creation_time: Some(1),
            parent_id: None,
            session_id: None,
            user_name: None,
            is_service_account: None,
            is_critical,
            can_set_information: true,
            name: name.into(),
            image_path: path.map(PathBuf::from),
        }
    }

    fn match_processes(
        settings: &ByRunningAppSettings,
        processes: &[ProcessInfo],
    ) -> (Option<ActiveByRunningApp>, bool) {
        matching_rule_process(
            settings,
            processes,
            u32::MAX,
            1,
            |_| Some(1),
            |process| process.image_path.clone(),
        )
    }

    #[test]
    fn matching_rule_uses_selected_plan() {
        let settings = ByRunningAppSettings {
            enabled: true,
            rules: vec![ByRunningAppRule {
                enabled: true,
                name: "Game".to_owned(),
                selector: ExecutableSelector::ExactPath {
                    executable_path: "Game.EXE".to_owned(),
                },
                power_plan_guid: Some("custom-guid".to_owned()),
            }],
        };
        let processes = [ProcessInfo {
            id: 42,
            creation_time: Some(1),
            parent_id: None,
            session_id: None,
            user_name: None,
            is_service_account: None,
            is_critical: Some(false),
            can_set_information: true,
            name: "game.exe".to_owned(),
            image_path: Some(PathBuf::from("game.exe".to_owned())),
        }];
        let matched = match_processes(&settings, &processes).0.unwrap();

        assert_eq!(matched.rule_name, "Game");
        assert_eq!(matched.target_guid, "custom-guid");
    }

    #[test]
    fn matching_rule_ignores_disabled_and_missing_plan_rules() {
        let settings = ByRunningAppSettings {
            enabled: true,
            rules: vec![
                ByRunningAppRule {
                    enabled: false,
                    name: "Disabled".to_owned(),
                    selector: ExecutableSelector::ExactPath {
                        executable_path: "game.exe".to_owned(),
                    },
                    power_plan_guid: Some("disabled-guid".to_owned()),
                },
                ByRunningAppRule {
                    enabled: true,
                    name: "Missing plan".to_owned(),
                    selector: ExecutableSelector::ExactPath {
                        executable_path: "game.exe".to_owned(),
                    },
                    power_plan_guid: None,
                },
            ],
        };
        let processes = [ProcessInfo {
            id: 42,
            creation_time: Some(1),
            parent_id: None,
            session_id: None,
            user_name: None,
            is_service_account: None,
            is_critical: Some(false),
            can_set_information: true,
            name: "game.exe".to_owned(),
            image_path: Some(PathBuf::from("game.exe".to_owned())),
        }];
        assert!(match_processes(&settings, &processes).0.is_none());
    }

    #[test]
    fn matching_rule_continues_past_unmatched_rules() {
        let settings = ByRunningAppSettings {
            enabled: true,
            rules: vec![
                ByRunningAppRule {
                    enabled: true,
                    name: "Missing process".to_owned(),
                    selector: ExecutableSelector::ExactPath {
                        executable_path: "missing.exe".to_owned(),
                    },
                    power_plan_guid: Some("missing-guid".to_owned()),
                },
                ByRunningAppRule {
                    enabled: true,
                    name: "Game".to_owned(),
                    selector: ExecutableSelector::ExactPath {
                        executable_path: "game.exe".to_owned(),
                    },
                    power_plan_guid: Some("game-guid".to_owned()),
                },
            ],
        };
        let processes = [ProcessInfo {
            id: 42,
            creation_time: Some(1),
            parent_id: None,
            session_id: None,
            user_name: None,
            is_service_account: None,
            is_critical: Some(false),
            can_set_information: true,
            name: "game.exe".to_owned(),
            image_path: Some(PathBuf::from("game.exe".to_owned())),
        }];
        let matched = match_processes(&settings, &processes).0.unwrap();

        assert_eq!(matched.rule_name, "Game");
        assert_eq!(matched.target_guid, "game-guid");
    }

    #[test]
    fn active_match_includes_rule_name() {
        let active = ActiveByRunningApp {
            rule_index: 0,
            rule_name: "Old name".to_owned(),
            process_id: 42,
            process_name: "game.exe".to_owned(),
            target_guid: "gaming-guid".to_owned(),
        };
        let manager = ByRunningAppManager {
            active: Some(active.clone()),
            observation_known: true,
        };
        let reordered = ActiveByRunningApp {
            rule_index: 1,
            ..active.clone()
        };
        assert!(!manager.active_matches(&reordered));
        assert_eq!(manager.active_decision().unwrap().0, 0);
        let renamed = ActiveByRunningApp {
            rule_name: "New name".to_owned(),
            ..active
        };

        assert!(!manager.active_matches(&renamed));
    }

    #[test]
    fn matching_observation_distinguishes_unknown_candidates_from_confirmed_nonmatches() {
        let settings = ByRunningAppSettings {
            enabled: true,
            rules: vec![ByRunningAppRule {
                enabled: true,
                name: "Game".into(),
                selector: ExecutableSelector::ExactPath {
                    executable_path: r"C:\Games\game.exe".into(),
                },
                power_plan_guid: Some("game-plan".into()),
            }],
        };
        for (name, critical, session, path, known, matched) in [
            (
                "game.exe",
                Some(false),
                Some(1),
                Some(r"C:\Games\game.exe"),
                true,
                true,
            ),
            (
                "game.exe",
                None,
                Some(1),
                Some(r"C:\Games\game.exe"),
                false,
                false,
            ),
            (
                "game.exe",
                Some(false),
                None,
                Some(r"C:\Games\game.exe"),
                false,
                false,
            ),
            ("game.exe", Some(false), Some(1), None, false, false),
            ("game.exe", Some(true), None, None, true, false),
            ("game.exe", Some(false), Some(2), None, true, false),
            ("other.exe", None, None, None, true, false),
            (
                "game.exe",
                None,
                None,
                Some(r"C:\Other\game.exe"),
                true,
                false,
            ),
        ] {
            let process = ProcessInfo {
                id: 42,
                creation_time: Some(1),
                parent_id: None,
                session_id: session,
                user_name: None,
                is_service_account: None,
                is_critical: critical,
                can_set_information: true,
                name: name.into(),
                image_path: path.map(PathBuf::from),
            };
            let (selected, observation_known) = matching_rule_process(
                &settings,
                &[process],
                99,
                1,
                |_| session,
                |process| process.image_path.clone(),
            );
            assert_eq!(
                observation_known, known,
                "{name}, {critical:?}, {session:?}, {path:?}"
            );
            assert_eq!(selected.is_some(), matched);
        }
        let (selected, known) = match_processes(&settings, &[]);
        assert!(selected.is_none());
        assert!(known);
    }

    #[test]
    fn preview_reports_processes_and_selects_the_first_eligible_rule() {
        use ByRunningAppPreviewMatch::{Match, SameNameDifferentPath, Unavailable};

        let game_path = r"C:\Games\game.exe";
        let other_path = r"C:\Other\game.exe";
        let excluded_path = r"C:\Windows\System32\csrss.exe";
        let rule = |enabled, path: &str, plan: Option<&str>| ByRunningAppRule {
            enabled,
            name: String::new(),
            selector: ExecutableSelector::ExactPath {
                executable_path: path.into(),
            },
            power_plan_guid: plan.map(str::to_owned),
        };
        let settings = ByRunningAppSettings {
            enabled: true,
            rules: vec![
                rule(false, game_path, Some("disabled-plan")),
                rule(true, game_path, None),
                rule(true, r"C:\Games\missing.exe", Some("unmatched-plan")),
                rule(true, game_path, Some("game-plan")),
                rule(true, other_path, Some("other-plan")),
                rule(true, excluded_path, Some("excluded-plan")),
            ],
        };
        let processes = [
            observed_process(0, "game.exe", Some(game_path), Some(false)),
            observed_process(1, "game.exe", Some(game_path), Some(false)),
            observed_process(2, "game.exe", Some(game_path), Some(true)),
            observed_process(3, "game.exe", Some(game_path), Some(false)),
            observed_process(4, "GAME.exe", Some(other_path), Some(false)),
            observed_process(5, "game.exe", None, Some(false)),
            observed_process(6, "game.exe", Some(game_path), Some(false)),
            observed_process(8, "game.exe", Some(game_path), Some(false)),
            observed_process(7, "game.exe", Some(game_path), Some(false)),
            observed_process(9, "csrss.exe", Some(excluded_path), Some(false)),
        ];
        let session_id = |pid| match pid {
            3 => Some(2),
            6 => None,
            _ => Some(1),
        };
        let preview = preview_rule_processes(&settings, &processes, 1, 1, session_id, |process| {
            process.image_path.clone()
        });
        assert_eq!(preview.rules.len(), settings.rules.len());
        assert_eq!(preview.winner, Some(3));
        assert!(preview.rules[2].is_empty());
        assert!(preview.rules[5].is_empty());

        for index in [0, 1, 3] {
            let rows: Vec<_> = preview.rules[index]
                .iter()
                .map(|row| (row.pid, row.kind, row.path.as_deref()))
                .collect();
            assert_eq!(
                rows,
                [
                    (4, SameNameDifferentPath, Some(Path::new(other_path))),
                    (5, Unavailable, None),
                    (6, Unavailable, Some(Path::new(game_path))),
                    (7, Match, Some(Path::new(game_path))),
                    (8, Match, Some(Path::new(game_path))),
                ]
            );
        }
        assert_eq!(preview.rules[4][0].kind, Match);

        let mut queried_paths = Vec::new();
        let (selected, observation_known) =
            matching_rule_process(&settings, &processes, 1, 1, session_id, |process| {
                queried_paths.push(process.id);
                process.image_path.clone()
            });
        let selected = selected.unwrap();
        assert_eq!(preview.winner, Some(selected.rule_index));
        assert_eq!(selected.process_id, 8);
        assert!(observation_known);
        assert_eq!(queried_paths, [4, 5, 6, 8]);
    }

    #[test]
    fn name_rule_requires_a_verified_executable_at_any_path() {
        use ByRunningAppPreviewMatch::{Match, Unavailable};

        let first_path = r"C:\Games\game.exe";
        let second_path = r"C:\Other\game.exe";
        let rule = |selector, plan: &str| ByRunningAppRule {
            enabled: true,
            name: String::new(),
            selector,
            power_plan_guid: Some(plan.into()),
        };
        let settings = ByRunningAppSettings {
            enabled: true,
            rules: vec![
                rule(
                    ExecutableSelector::ExactPath {
                        executable_path: r"C:\Absent\game.exe".into(),
                    },
                    "absent-plan",
                ),
                rule(
                    ExecutableSelector::ExactName {
                        executable_name: "GAME.exe".into(),
                    },
                    "name-plan",
                ),
                rule(
                    ExecutableSelector::ExactPath {
                        executable_path: first_path.into(),
                    },
                    "path-plan",
                ),
            ],
        };
        let processes = [
            observed_process(2, "game.exe", None, Some(false)),
            observed_process(3, "game.exe", Some(first_path), Some(false)),
            observed_process(4, "game.exe", Some(first_path), Some(false)),
            observed_process(5, "game.exe", Some(first_path), None),
            observed_process(6, "game.exe", Some("game.exe"), Some(false)),
            observed_process(7, "game.exe", Some(r"C:\Games\other.exe"), Some(false)),
            observed_process(9, "game.exe", Some(second_path), Some(false)),
            observed_process(8, "game.exe", Some(first_path), Some(false)),
        ];
        let session_id = |pid| match pid {
            3 => None,
            4 => Some(2),
            _ => Some(1),
        };
        let preview = preview_rule_processes(&settings, &processes, 1, 1, session_id, |process| {
            process.image_path.clone()
        });
        let name_matches: Vec<_> = preview.rules[1]
            .iter()
            .map(|row| (row.pid, row.kind))
            .collect();
        assert_eq!(
            name_matches,
            [
                (2, Unavailable),
                (3, Unavailable),
                (5, Unavailable),
                (6, Unavailable),
                (7, Unavailable),
                (8, Match),
                (9, Match),
            ]
        );

        let (selected, known) =
            matching_rule_process(&settings, &processes, 1, 1, session_id, |process| {
                process.image_path.clone()
            });
        let selected = selected.unwrap();
        assert_eq!(preview.winner, Some(1));
        assert_eq!(preview.winner, Some(selected.rule_index));
        assert_eq!(selected.target_guid, "name-plan");
        assert_eq!(selected.process_id, 9);
        assert!(known);
    }

    #[test]
    fn disabled_or_unconfigured_matching_is_known_without_windows_observations() {
        let mut manager = ByRunningAppManager::default();
        let mut observations = CycleObservations::default();
        for enabled in [false, true] {
            let settings = ByRunningAppSettings {
                enabled,
                ..ByRunningAppSettings::default()
            };
            let snapshot = manager.update(&settings, true, &mut observations);
            assert!(snapshot.observation_known);
            assert!(manager.observation_known());
            assert!(snapshot.target_guid.is_none());
        }
    }

    #[test]
    fn builtin_exclusions_cover_sensitive_windows_processes() {
        assert!(is_builtin_excluded("csrss.exe"));
        assert!(is_builtin_excluded("winlogon.exe"));
        assert!(!is_builtin_excluded("game.exe"));
    }
}
