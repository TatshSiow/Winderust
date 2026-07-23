use std::collections::BTreeSet;

use windows_sys::Win32::System::Threading::GetCurrentProcessId;

use crate::{
    action_log::{ActionLog, ActionLogFeature, ActionLogResult},
    config::{ByRunningAppRule, ByRunningAppSettings},
    foreground::{
        contains_process_name, list_processes, process_session_id, same_process_name, ProcessInfo,
        EXTENDED_BUILT_IN_PROCESS_EXCLUSIONS,
    },
};

const BUILT_IN_EXCLUSIONS: &[&str] = EXTENDED_BUILT_IN_PROCESS_EXCLUSIONS;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ByRunningAppSnapshot {
    pub enabled: bool,
    pub active: bool,
    pub scanned_processes: usize,
    pub matched_processes: usize,
    pub active_rule: Option<String>,
    pub active_process: Option<String>,
    pub target_guid: Option<String>,
    pub previous_guid: Option<String>,
    pub message: String,
    pub last_error: Option<String>,
}

#[derive(Default)]
pub struct ByRunningAppManager {
    active: Option<ActiveByRunningApp>,
}

#[derive(Debug, Clone)]
struct ActiveByRunningApp {
    rule_name: String,
    process_id: u32,
    process_name: String,
    target_guid: String,
}

#[derive(Debug, Clone)]
struct ByRunningAppMatch {
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
        action_log: &mut ActionLog,
    ) -> ByRunningAppSnapshot {
        if !automation_enabled {
            self.release(action_log, "automation disabled");
            return ByRunningAppSnapshot {
                enabled: false,
                message: "Automation disabled.".to_owned(),
                ..Default::default()
            };
        }

        if !settings.enabled {
            self.release(action_log, "By Running App disabled");
            return ByRunningAppSnapshot {
                enabled: false,
                message: "By Running App disabled.".to_owned(),
                ..Default::default()
            };
        }

        // SAFETY: GetCurrentProcessId takes no arguments and has no caller requirements.
        let current_process_id = unsafe { GetCurrentProcessId() };
        let Some(current_session_id) = process_session_id(current_process_id) else {
            self.release(action_log, "current Windows session is unknown");
            return ByRunningAppSnapshot {
                enabled: true,
                message: "Paused: current Windows session is unknown.".to_owned(),
                ..Default::default()
            };
        };

        let processes = match list_processes() {
            Ok(processes) => processes,
            Err(err) => {
                self.release(action_log, "process list unavailable");
                return ByRunningAppSnapshot {
                    enabled: true,
                    message: err,
                    ..Default::default()
                };
            }
        };

        let scanned_processes = processes.len();
        let eligible_processes = processes
            .into_iter()
            .filter(|process| {
                process.id != 0
                    && process.id != current_process_id
                    && !is_builtin_excluded(&process.name)
                    && process_session_id(process.id) == Some(current_session_id)
            })
            .collect::<Vec<_>>();
        let matched_processes = matching_process_names(settings, &eligible_processes).len();
        let matched = matching_rule_process(settings, &eligible_processes);

        let Some(matched) = matched else {
            self.release(action_log, "no By Running App process is running");
            return ByRunningAppSnapshot {
                enabled: true,
                scanned_processes,
                matched_processes,
                message: "By Running App waiting for a matching process.".to_owned(),
                ..Default::default()
            };
        };

        if self.active_matches(&matched) {
            return self.snapshot(true, scanned_processes, matched_processes, None);
        }

        action_log.record(
            ActionLogFeature::ByRunningApp,
            Some(matched.process_id),
            matched.process_name.clone(),
            ActionLogResult::Applied,
            format!(
                "Rule '{}' requested performance plan {}.",
                matched.rule_name, matched.target_guid
            ),
        );
        self.active = Some(ActiveByRunningApp {
            rule_name: matched.rule_name,
            process_id: matched.process_id,
            process_name: matched.process_name,
            target_guid: matched.target_guid,
        });
        self.snapshot(true, scanned_processes, matched_processes, None)
    }

    pub fn is_active(&self) -> bool {
        self.active.is_some()
    }

    pub fn active_process_ids(&self) -> BTreeSet<u32> {
        self.active
            .as_ref()
            .map(|active| BTreeSet::from([active.process_id]))
            .unwrap_or_default()
    }

    pub fn active_decision(&self) -> Option<(String, String, String)> {
        self.active.as_ref().map(|active| {
            (
                active.rule_name.clone(),
                active.process_name.clone(),
                active.target_guid.clone(),
            )
        })
    }

    fn active_matches(&self, matched: &ByRunningAppMatch) -> bool {
        self.active.as_ref().is_some_and(|active| {
            active.process_id == matched.process_id
                && same_process_name(&active.process_name, &matched.process_name)
                && active
                    .target_guid
                    .eq_ignore_ascii_case(&matched.target_guid)
        })
    }

    fn release(&mut self, action_log: &mut ActionLog, reason: &str) {
        let Some(active) = self.active.take() else {
            return;
        };

        action_log.record(
            ActionLogFeature::ByRunningApp,
            Some(active.process_id),
            active.process_name,
            ActionLogResult::Restored,
            format!("{reason}; released By Running App decision."),
        );
    }

    fn snapshot(
        &self,
        enabled: bool,
        scanned_processes: usize,
        matched_processes: usize,
        last_error: Option<String>,
    ) -> ByRunningAppSnapshot {
        let Some(active) = self.active.as_ref() else {
            return ByRunningAppSnapshot {
                enabled,
                scanned_processes,
                matched_processes,
                message: if enabled {
                    "By Running App waiting for a matching process.".to_owned()
                } else {
                    "By Running App disabled.".to_owned()
                },
                last_error,
                ..Default::default()
            };
        };

        ByRunningAppSnapshot {
            enabled,
            active: true,
            scanned_processes,
            matched_processes,
            active_rule: Some(active.rule_name.clone()),
            active_process: Some(active.process_name.clone()),
            target_guid: Some(active.target_guid.clone()),
            previous_guid: None,
            message: "By Running App active.".to_owned(),
            last_error,
        }
    }
}

impl Drop for ByRunningAppManager {
    fn drop(&mut self) {
        let mut action_log = ActionLog::new(1);
        self.release(&mut action_log, "By Running App manager dropped");
    }
}

impl Default for ByRunningAppSnapshot {
    fn default() -> Self {
        Self {
            enabled: false,
            active: false,
            scanned_processes: 0,
            matched_processes: 0,
            active_rule: None,
            active_process: None,
            target_guid: None,
            previous_guid: None,
            message: "By Running App disabled.".to_owned(),
            last_error: None,
        }
    }
}

pub fn is_builtin_excluded(process_name: &str) -> bool {
    contains_process_name(BUILT_IN_EXCLUSIONS, process_name)
}

fn matching_rule_process(
    settings: &ByRunningAppSettings,
    processes: &[ProcessInfo],
) -> Option<ByRunningAppMatch> {
    for rule in &settings.rules {
        if !rule.enabled || rule.process_name.trim().is_empty() {
            continue;
        }
        let Some(target_guid) = rule.power_plan_guid.clone() else {
            continue;
        };
        let Some(process) = processes
            .iter()
            .find(|process| same_process_name(&process.name, &rule.process_name))
        else {
            continue;
        };

        return Some(ByRunningAppMatch {
            rule_name: performance_rule_name(rule),
            process_id: process.id,
            process_name: process.name.clone(),
            target_guid,
        });
    }

    None
}

fn matching_process_names(
    settings: &ByRunningAppSettings,
    processes: &[ProcessInfo],
) -> BTreeSet<String> {
    processes
        .iter()
        .filter(|process| {
            settings.rules.iter().any(|rule| {
                rule.enabled
                    && !rule.process_name.trim().is_empty()
                    && same_process_name(&process.name, &rule.process_name)
            })
        })
        .map(|process| process.name.clone())
        .collect()
}

fn performance_rule_name(rule: &ByRunningAppRule) -> String {
    let name = rule.name.trim();
    if name.is_empty() {
        rule.process_name.trim().to_owned()
    } else {
        name.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching_rule_uses_selected_plan() {
        let settings = ByRunningAppSettings {
            enabled: true,
            rules: vec![ByRunningAppRule {
                enabled: true,
                name: "Game".to_owned(),
                process_name: "Game.EXE".to_owned(),
                power_plan_guid: Some("custom-guid".to_owned()),
            }],
        };
        let processes = vec![ProcessInfo {
            id: 42,
            parent_id: None,
            name: "game.exe".to_owned(),
        }];

        let matched = matching_rule_process(&settings, &processes).unwrap();

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
                    process_name: "game.exe".to_owned(),
                    power_plan_guid: Some("disabled-guid".to_owned()),
                },
                ByRunningAppRule {
                    enabled: true,
                    name: "Missing plan".to_owned(),
                    process_name: "game.exe".to_owned(),
                    power_plan_guid: None,
                },
            ],
        };
        let processes = vec![ProcessInfo {
            id: 42,
            parent_id: None,
            name: "game.exe".to_owned(),
        }];

        assert!(matching_rule_process(&settings, &processes).is_none());
    }

    #[test]
    fn matching_rule_continues_past_unmatched_rules() {
        let settings = ByRunningAppSettings {
            enabled: true,
            rules: vec![
                ByRunningAppRule {
                    enabled: true,
                    name: "Missing process".to_owned(),
                    process_name: "missing.exe".to_owned(),
                    power_plan_guid: Some("missing-guid".to_owned()),
                },
                ByRunningAppRule {
                    enabled: true,
                    name: "Game".to_owned(),
                    process_name: "game.exe".to_owned(),
                    power_plan_guid: Some("game-guid".to_owned()),
                },
            ],
        };
        let processes = vec![ProcessInfo {
            id: 42,
            parent_id: None,
            name: "game.exe".to_owned(),
        }];

        let matched = matching_rule_process(&settings, &processes).unwrap();

        assert_eq!(matched.rule_name, "Game");
        assert_eq!(matched.target_guid, "game-guid");
    }

    #[test]
    fn builtin_exclusions_cover_sensitive_windows_processes() {
        assert!(is_builtin_excluded("csrss.exe"));
        assert!(is_builtin_excluded("winlogon.exe"));
        assert!(!is_builtin_excluded("game.exe"));
    }
}
