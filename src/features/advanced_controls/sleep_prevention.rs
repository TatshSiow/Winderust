use std::{collections::BTreeMap, path::Path, sync::Arc};

use crate::{
    action_log::{ActionLog, ActionLogFeature, ActionLogResult},
    config::{PowerSourceProfile, Settings, SleepPreventionSettings},
    control::sleep_prevention::{
        SleepPreventionController, SleepPreventionMode, SleepPreventionRequests,
    },
    foreground::{
        contains_process_name, process_session_id, same_executable_path, same_process_name,
        ProcessInfo, EXTENDED_BUILT_IN_PROCESS_EXCLUSIONS,
    },
    runtime::observations::CycleObservations,
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SleepPreventionSnapshot {
    pub(crate) source: Option<PowerSourceProfile>,
    pub enabled: bool,
    pub matched_rules: usize,
    pub matched_processes: usize,
    pub running_apps: Vec<String>,
    pub suppressed_apps: Vec<String>,
    pub(crate) requesting_apps: Vec<RequestingApp>,
    pub(crate) observed_rule_paths: Option<Arc<[String]>>,
    pub system_requested: bool,
    pub display_requested: bool,
    pub observation_known: bool,
    pub failed_actions: usize,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RequestingApp {
    pub(crate) path: Box<str>,
    pub(crate) keep_display_on: bool,
}

type AppIdentity = (u32, u64);

#[derive(Default)]
pub struct SleepPreventionManager {
    matches: Matches,
    suppressed: BTreeMap<AppIdentity, String>,
    suppress_next_observation: bool,
    rearmed_paths: Vec<String>,
    power_source: Option<bool>,
    configuration: Option<[(bool, SleepPreventionSettings); 2]>,
    snapshot: SleepPreventionSnapshot,
}

#[derive(Default)]
struct Matches {
    apps: BTreeMap<AppIdentity, MatchedApp>,
    rules: usize,
    known: bool,
}

struct MatchedApp {
    path: String,
    display: bool,
    in_current_session: bool,
}

impl SleepPreventionManager {
    pub(crate) fn update(
        &mut self,
        controller: &mut SleepPreventionController,
        configured_settings: &Settings,
        plugged_in: Option<bool>,
        windows_event_watcher_active: bool,
        observations: &mut CycleObservations,
        action_log: &mut ActionLog,
    ) -> SleepPreventionSnapshot {
        // Preserve the accepted cohort before a profile handoff closes its request.
        if !windows_event_watcher_active && controller.requests().system {
            self.suppress_current();
        }
        self.note_configuration(configured_settings, plugged_in);
        if self.change_power_source(plugged_in) {
            self.reset(controller, action_log);
        }
        let Some(plugged_in) = self.power_source else {
            let profiles = [configured_settings, configured_settings.battery_profile()];
            let enabled = profiles
                .iter()
                .any(|profile| profile.general.enabled && profile.sleep_prevention.enabled);
            if !profiles.iter().any(|profile| {
                profile.general.enabled
                    && profile.sleep_prevention.enabled
                    && profile.sleep_prevention.rules.iter().any(|rule| {
                        rule.enabled && Path::new(rule.executable_path.trim()).is_absolute()
                    })
            }) {
                self.matches = Matches::default();
                return self.apply(
                    controller,
                    SleepPreventionMode::Off,
                    SleepPreventionSnapshot {
                        enabled,
                        observation_known: true,
                        ..Default::default()
                    },
                    action_log,
                );
            }
            return self.unavailable(
                controller,
                action_log,
                &rust_i18n::t!("sleep_prevention.power_source_unavailable"),
            );
        };
        let profile = if plugged_in {
            configured_settings
        } else {
            configured_settings.battery_profile()
        };
        if profile.general.enabled
            && profile.sleep_prevention.enabled
            && !windows_event_watcher_active
            && profile
                .sleep_prevention
                .rules
                .iter()
                .any(|rule| rule.enabled)
        {
            return self.unavailable(
                controller,
                action_log,
                &rust_i18n::t!("sleep_prevention.watcher_unavailable"),
            );
        }
        self.update_profile(
            controller,
            &profile.sleep_prevention,
            profile.general.enabled,
            profile.general.allow_cross_session_process_control,
            observations,
            action_log,
        )
    }

    pub(crate) fn last_known_power_source(&self) -> Option<bool> {
        self.power_source
    }

    fn change_power_source(&mut self, plugged_in: Option<bool>) -> bool {
        let Some(plugged_in) = plugged_in else {
            return false;
        };
        let changed = self
            .power_source
            .is_some_and(|previous| previous != plugged_in);
        self.power_source = Some(plugged_in);
        changed
    }

    fn note_configuration(&mut self, settings: &Settings, plugged_in: Option<bool>) {
        let profiles = [settings, settings.battery_profile()];
        if self.configuration.as_ref().is_some_and(|previous| {
            previous.iter().zip(&profiles).all(|(previous, profile)| {
                previous.0 == profile.general.enabled && previous.1 == profile.sleep_prevention
            })
        }) {
            return;
        }
        let current =
            profiles.map(|profile| (profile.general.enabled, profile.sleep_prevention.clone()));
        if let (Some(previous), Some(active_source)) =
            (self.configuration.take(), plugged_in.or(self.power_source))
        {
            for (index, (before, after)) in previous.iter().zip(&current).enumerate() {
                let source = index == 0;
                let protected = if source == active_source {
                    None
                } else if Some(source) == self.power_source {
                    // An edit to the outgoing profile must preserve the incoming profile's claims.
                    Some(&current[usize::from(!active_source)])
                } else {
                    continue;
                };
                self.rearm_withdrawn(before, after, protected);
                // Fresh opt-ins must not inherit a pending capture from an earlier sleep.
                self.rearm_withdrawn(after, before, protected);
            }
        }
        self.configuration = Some(current);
    }

    fn rearm_withdrawn(
        &mut self,
        previous: &(bool, SleepPreventionSettings),
        current: &(bool, SleepPreventionSettings),
        protected: Option<&(bool, SleepPreventionSettings)>,
    ) {
        if !previous.0 || !previous.1.enabled {
            return;
        }
        for rule in previous.1.rules.iter().filter(|rule| rule.enabled) {
            let path = Path::new(rule.executable_path.trim());
            if std::iter::once(current)
                .chain(protected)
                .any(|(enabled, settings)| {
                    *enabled
                        && settings.enabled
                        && settings.rules.iter().any(|rule| {
                            rule.enabled
                                && same_executable_path(
                                    Path::new(rule.executable_path.trim()),
                                    path,
                                )
                        })
                })
            {
                continue;
            }
            self.suppressed
                .retain(|_, suppressed| !same_executable_path(Path::new(suppressed), path));
            if self.suppress_next_observation {
                self.rearmed_paths.push(rule.executable_path.clone());
            }
        }
    }

    fn update_profile(
        &mut self,
        controller: &mut SleepPreventionController,
        settings: &SleepPreventionSettings,
        automation_enabled: bool,
        allow_cross_session_process_control: bool,
        observations: &mut CycleObservations,
        action_log: &mut ActionLog,
    ) -> SleepPreventionSnapshot {
        if !automation_enabled || !settings.enabled {
            self.matches = Matches::default();
            return self.reset(controller, action_log);
        }
        if !settings
            .rules
            .iter()
            .any(|rule| rule.enabled && Path::new(rule.executable_path.trim()).is_absolute())
        {
            self.matches = Matches {
                known: true,
                ..Default::default()
            };
            return self.apply(
                controller,
                SleepPreventionMode::Off,
                self.matching_snapshot(),
                action_log,
            );
        }
        let error = match observations.processes_with_paths() {
            Ok(processes) => {
                self.observe(
                    settings,
                    &processes,
                    std::process::id(),
                    process_session_id(std::process::id()),
                    allow_cross_session_process_control,
                );
                None
            }
            Err(error) => {
                self.retain_enabled_claims(settings, allow_cross_session_process_control);
                self.matches.known = false;
                Some(error)
            }
        };
        let mut snapshot = self.matching_snapshot();
        if snapshot.observation_known {
            let paths = settings
                .rules
                .iter()
                .filter(|rule| rule.enabled)
                .filter_map(|rule| matchable_target(&rule.executable_path).map(|(path, _)| path));
            snapshot.observed_rule_paths = Some(
                self.snapshot
                    .observed_rule_paths
                    .as_ref()
                    .filter(|previous| previous.iter().map(String::as_str).eq(paths.clone()))
                    .cloned()
                    .unwrap_or_else(|| paths.map(str::to_owned).collect()),
            );
        }
        snapshot.last_error = error;
        let mode = self.permitted_mode(controller.requests());
        self.apply(controller, mode, snapshot, action_log)
    }

    fn matching_snapshot(&self) -> SleepPreventionSnapshot {
        let mut snapshot = SleepPreventionSnapshot {
            enabled: true,
            matched_rules: self.matches.rules,
            matched_processes: self.matches.apps.len(),
            observation_known: self.matches.known,
            ..Default::default()
        };
        // Insert distinct paths directly into the sorted status lists.
        for (identity, app) in &self.matches.apps {
            let path = &app.path;
            if let Err(index) = snapshot.running_apps.binary_search(path) {
                snapshot.running_apps.insert(index, path.clone());
            }
            if self.suppressed.contains_key(identity) {
                if let Err(index) = snapshot.suppressed_apps.binary_search(path) {
                    snapshot.suppressed_apps.insert(index, path.clone());
                }
            } else {
                let requests = &mut snapshot.requesting_apps;
                match requests.binary_search_by(|entry| entry.path.as_ref().cmp(path.as_str())) {
                    Ok(index) => requests[index].keep_display_on |= app.display,
                    Err(index) => requests.insert(
                        index,
                        RequestingApp {
                            path: path.as_str().into(),
                            keep_display_on: app.display,
                        },
                    ),
                }
            }
        }
        // A fresh instance can request sleep prevention while an older instance stays suppressed.
        snapshot.suppressed_apps.retain(|path| {
            snapshot
                .requesting_apps
                .binary_search_by(|request| request.path.as_ref().cmp(path.as_str()))
                .is_err()
        });
        snapshot
    }

    pub(crate) fn suspend(
        &mut self,
        controller: &mut SleepPreventionController,
        action_log: &mut ActionLog,
    ) -> SleepPreventionSnapshot {
        self.suppress_current();
        let mut snapshot = self.snapshot.clone();
        snapshot.suppressed_apps.clone_from(&snapshot.running_apps);
        snapshot.requesting_apps.clear();
        snapshot.failed_actions = 0;
        snapshot.last_error = None;
        self.apply(controller, SleepPreventionMode::Off, snapshot, action_log)
    }

    pub(crate) fn reset(
        &mut self,
        controller: &mut SleepPreventionController,
        action_log: &mut ActionLog,
    ) -> SleepPreventionSnapshot {
        // Profile changes release requests without rearming apps suppressed by sleep.
        self.apply(
            controller,
            SleepPreventionMode::Off,
            SleepPreventionSnapshot {
                observation_known: true,
                ..Default::default()
            },
            action_log,
        )
    }

    pub(crate) fn unavailable(
        &mut self,
        controller: &mut SleepPreventionController,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> SleepPreventionSnapshot {
        let mut snapshot = self.matching_snapshot();
        snapshot.observation_known = false;
        snapshot.requesting_apps.clear();
        snapshot.last_error = Some(reason.to_owned());
        self.apply(controller, SleepPreventionMode::Off, snapshot, action_log)
    }

    fn observe(
        &mut self,
        settings: &SleepPreventionSettings,
        processes: &[ProcessInfo],
        own_pid: u32,
        current_session: Option<u32>,
        allow_cross_session: bool,
    ) {
        let previous = std::mem::take(&mut self.matches.apps);
        self.matches = match_rules(
            settings,
            processes,
            own_pid,
            current_session,
            allow_cross_session,
        );
        if !self.matches.known {
            for (identity, app) in previous {
                if processes.iter().any(|process| {
                    process.id == identity.0
                        && process
                            .creation_time
                            .is_none_or(|creation| creation == identity.1)
                        && process
                            .image_path
                            .as_ref()
                            .is_none_or(|path| same_executable_path(path, Path::new(&app.path)))
                        && process.is_critical != Some(true)
                        && process.is_service_account != Some(true)
                        && process.session_id != Some(0)
                        && (allow_cross_session
                            || process
                                .session_id
                                .zip(current_session)
                                .is_none_or(|(session, current)| session == current))
                }) {
                    self.matches.apps.entry(identity).or_insert(app);
                }
            }
            self.retain_enabled_claims(settings, allow_cross_session);
        }
        self.suppressed.retain(|(pid, creation), _| {
            processes.iter().any(|process| {
                process.id == *pid
                    && process
                        .creation_time
                        .is_none_or(|observed| observed == *creation)
            })
        });
        if self.suppress_next_observation {
            self.suppressed.extend(
                self.matches
                    .apps
                    .iter()
                    .filter(|(_, app)| {
                        !self.rearmed_paths.iter().any(|path| {
                            same_executable_path(Path::new(path.trim()), Path::new(&app.path))
                        })
                    })
                    .map(|(identity, app)| (*identity, app.path.clone())),
            );
            if self.matches.known {
                self.suppress_next_observation = false;
                self.rearmed_paths.clear();
            }
        }
    }

    fn retain_enabled_claims(
        &mut self,
        settings: &SleepPreventionSettings,
        allow_cross_session: bool,
    ) {
        self.matches.apps.retain(|_, app| {
            if !allow_cross_session && !app.in_current_session {
                return false;
            }
            let mut rules = settings.rules.iter().filter(|rule| {
                rule.enabled
                    && same_executable_path(
                        Path::new(rule.executable_path.trim()),
                        Path::new(&app.path),
                    )
            });
            let Some(first) = rules.next() else {
                return false;
            };
            app.display = first.keep_display_on || rules.any(|rule| rule.keep_display_on);
            true
        });
        self.matches.rules = settings
            .rules
            .iter()
            .filter(|rule| {
                rule.enabled
                    && self.matches.apps.values().any(|app| {
                        same_executable_path(
                            Path::new(rule.executable_path.trim()),
                            Path::new(&app.path),
                        )
                    })
            })
            .count();
    }

    fn suppress_current(&mut self) {
        self.rearmed_paths.clear();
        self.suppressed.extend(
            self.matches
                .apps
                .iter()
                .map(|(identity, app)| (*identity, app.path.clone())),
        );
        // Capture once after resume too: an event may precede the first app observation.
        self.suppress_next_observation = true;
    }

    fn desired_mode(&self) -> SleepPreventionMode {
        let mut mode = SleepPreventionMode::Off;
        for (identity, app) in &self.matches.apps {
            if !self.suppressed.contains_key(identity) {
                if app.display {
                    return SleepPreventionMode::SystemAndDisplay;
                }
                mode = SleepPreventionMode::System;
            }
        }
        mode
    }

    fn permitted_mode(&self, accepted: SleepPreventionRequests) -> SleepPreventionMode {
        let desired = self.desired_mode();
        if self.matches.known {
            return desired;
        }
        // Unknown observations can retain or withdraw accepted counts, never renew or add them.
        match desired {
            SleepPreventionMode::Off => SleepPreventionMode::Off,
            _ if !accepted.system => SleepPreventionMode::Off,
            SleepPreventionMode::SystemAndDisplay if accepted.display => desired,
            _ => SleepPreventionMode::System,
        }
    }

    fn apply(
        &mut self,
        controller: &mut SleepPreventionController,
        mode: SleepPreventionMode,
        mut snapshot: SleepPreventionSnapshot,
        action_log: &mut ActionLog,
    ) -> SleepPreventionSnapshot {
        let before = controller.requests();
        let reason = rust_i18n::t!("sleep_prevention.request_reason");
        if let Err(error) = controller.reconcile(mode, &reason) {
            snapshot.last_error = Some(error.to_string());
        }
        let after = controller.requests();
        snapshot.source = self.power_source.map(|plugged_in| {
            if plugged_in {
                PowerSourceProfile::PluggedIn
            } else {
                PowerSourceProfile::OnBattery
            }
        });
        snapshot.system_requested = after.system;
        snapshot.display_requested = after.display;
        if before != after {
            let (result, key) = if !after.system {
                (ActionLogResult::Restored, "sleep_prevention.log_released")
            } else if after.display {
                (
                    ActionLogResult::Applied,
                    "sleep_prevention.log_display_requested",
                )
            } else {
                (
                    if before.display {
                        ActionLogResult::Restored
                    } else {
                        ActionLogResult::Applied
                    },
                    "sleep_prevention.log_system_requested",
                )
            };
            action_log.record(
                ActionLogFeature::SleepPrevention,
                None,
                "System",
                result,
                rust_i18n::t!(key),
            );
        }
        if let Some(error) = &snapshot.last_error {
            snapshot.failed_actions = 1;
            if self.snapshot.last_error.as_ref() != Some(error) {
                action_log.record(
                    ActionLogFeature::SleepPrevention,
                    None,
                    "System",
                    ActionLogResult::Failed,
                    rust_i18n::t!("sleep_prevention.log_failed", error = error),
                );
            }
        }
        self.snapshot = snapshot.clone();
        snapshot
    }
}

fn matchable_target(executable: &str) -> Option<(&str, &str)> {
    let executable = executable.trim();
    let path = Path::new(executable);
    if !path.is_absolute() {
        return None;
    }
    Some((executable, path.file_name()?.to_str()?))
}

fn match_rules(
    settings: &SleepPreventionSettings,
    processes: &[ProcessInfo],
    own_pid: u32,
    current_session: Option<u32>,
    allow_cross_session: bool,
) -> Matches {
    let mut matches = Matches {
        known: allow_cross_session || current_session.is_some(),
        ..Default::default()
    };
    for rule in settings.rules.iter().filter(|rule| rule.enabled) {
        let Some((executable, name)) = matchable_target(&rule.executable_path) else {
            continue;
        };
        let path = Path::new(executable);
        let mut rule_matched = false;
        for process in processes {
            if process.id == 0
                || process.id == own_pid
                || crate::backend::is_helper_process(process.id)
                || contains_process_name(EXTENDED_BUILT_IN_PROCESS_EXCLUSIONS, &process.name)
                || !same_process_name(&process.name, name)
                || process.is_critical == Some(true)
                || process.is_service_account == Some(true)
                || process.session_id == Some(0)
                || (!allow_cross_session
                    && process
                        .session_id
                        .zip(current_session)
                        .is_some_and(|(session, current)| session != current))
            {
                continue;
            }
            let Some(actual_path) = &process.image_path else {
                matches.known = false;
                continue;
            };
            if !same_executable_path(actual_path, path) {
                continue;
            }
            let Some(creation) = process.creation_time.filter(|_| {
                process.is_critical == Some(false)
                    && process.is_service_account == Some(false)
                    && process.session_id.is_some()
            }) else {
                matches.known = false;
                continue;
            };
            if !process.can_set_information {
                continue;
            }
            matches
                .apps
                .entry((process.id, creation))
                .and_modify(|app| app.display |= rule.keep_display_on)
                .or_insert_with(|| MatchedApp {
                    path: executable.to_owned(),
                    display: rule.keep_display_on,
                    in_current_session: current_session.is_some()
                        && process.session_id == current_session,
                });
            rule_matched = true;
        }
        matches.rules += usize::from(rule_matched);
    }
    matches
}

#[cfg(test)]
mod tests;
