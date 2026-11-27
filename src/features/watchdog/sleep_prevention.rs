use std::{
    collections::BTreeMap,
    path::Path,
    time::{Duration, Instant},
};

use crate::{
    action_log::{ActionLog, ActionLogFeature, ActionLogResult},
    config::{Settings, SleepPreventionSettings},
    control::sleep_prevention::{
        SleepPreventionController, SleepPreventionMode, SleepPreventionRequests,
    },
    foreground::{
        contains_process_name, process_list::executable_basename_may_match, process_session_id,
        same_executable_path, ProcessInfo, EXTENDED_BUILT_IN_PROCESS_EXCLUSIONS,
    },
    runtime::observations::CycleObservations,
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SleepPreventionSnapshot {
    pub enabled: bool,
    pub matched_rules: usize,
    pub matched_processes: usize,
    pub system_requested: bool,
    pub display_requested: bool,
    pub observation_known: bool,
    pub failed_actions: usize,
    pub last_error: Option<String>,
    pub(crate) temporary: TemporarySleepPreventionTimers,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TemporarySleepPrevention {
    KeepAwake,
    KeepDisplayOn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TemporarySleepPreventionDuration(u16);

impl TemporarySleepPreventionDuration {
    pub(crate) const MAX_MINUTES: u16 = 24 * 60;

    pub(crate) fn from_minutes(minutes: u16) -> Option<Self> {
        (1..=Self::MAX_MINUTES)
            .contains(&minutes)
            .then_some(Self(minutes))
    }

    fn duration(self) -> Duration {
        Duration::from_secs(u64::from(self.0) * 60)
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct TemporarySleepPreventionTimers([Option<Instant>; 2]);

impl TemporarySleepPreventionTimers {
    pub(crate) fn until(self, kind: TemporarySleepPrevention) -> Option<Instant> {
        self.0[kind as usize]
    }

    pub(crate) fn any_active(self) -> bool {
        self.0.iter().any(Option::is_some)
    }

    pub(crate) fn set(&mut self, kind: TemporarySleepPrevention, deadline: Option<Instant>) {
        self.0[kind as usize] = deadline;
    }
}

type AppIdentity = (u32, u64);

pub(crate) struct SleepPreventionEnvironment<'a> {
    pub(crate) windows_event_watcher_active: bool,
    pub(crate) suspended_instances: &'a [AppIdentity],
}

#[derive(Default)]
pub struct SleepPreventionManager {
    matches: Matches,
    suppressed: BTreeMap<AppIdentity, String>,
    suppress_next_observation: bool,
    rearmed_paths: Vec<String>,
    configuration: Option<(bool, SleepPreventionSettings)>,
    snapshot: SleepPreventionSnapshot,
    temporary: TemporarySleepPreventionTimers,
    rule_requests: SleepPreventionRequests,
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
    suspended: bool,
}

impl SleepPreventionManager {
    pub(crate) fn set_temporary(
        &mut self,
        kind: TemporarySleepPrevention,
        duration: Option<TemporarySleepPreventionDuration>,
        now: Instant,
    ) {
        self.temporary
            .set(kind, duration.map(|duration| now + duration.duration()));
    }

    pub(crate) fn temporary_wait(&self, now: Instant) -> Option<Duration> {
        self.temporary
            .0
            .into_iter()
            .flatten()
            .min()
            .map(|deadline| deadline.saturating_duration_since(now))
    }

    pub(crate) fn end_temporary_if_due(&mut self, now: Instant, permitted: bool) -> bool {
        let previous = self.temporary;
        for deadline in &mut self.temporary.0 {
            if deadline.is_some_and(|expiry| !permitted || now >= expiry) {
                *deadline = None;
            }
        }
        self.temporary != previous
    }

    pub(crate) fn rule_system_requested(&self) -> bool {
        self.rule_requests.system
    }

    pub(crate) fn update<P: crate::platform::windows::sleep_prevention::SleepPreventionPlatform>(
        &mut self,
        controller: &mut SleepPreventionController<P>,
        configured_settings: &Settings,
        environment: SleepPreventionEnvironment<'_>,
        observations: &mut CycleObservations,
        action_log: &mut ActionLog,
    ) -> SleepPreventionSnapshot {
        let SleepPreventionEnvironment {
            windows_event_watcher_active,
            suspended_instances,
        } = environment;
        let settings = &configured_settings.sleep_prevention;
        let enabled = configured_settings.general.enabled && settings.enabled;
        self.note_configuration(configured_settings);
        if !enabled
            || !settings
                .rules
                .iter()
                .any(|rule| rule.enabled && matchable_target(&rule.executable_path).is_some())
        {
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
        if !windows_event_watcher_active {
            if self.rule_system_requested() {
                self.suppress_current();
            }
            return self.unavailable(
                controller,
                action_log,
                &rust_i18n::t!("sleep_prevention.watcher_unavailable"),
            );
        }
        let allow_cross_session_process_control = configured_settings
            .general
            .allow_cross_session_process_control;
        let error = match observations
            .processes_for_path_matching(|process| self.needs_path_observation(process, settings))
        {
            Ok(processes) => {
                let own_pid = std::process::id();
                let current_session = processes.first().and_then(|_| process_session_id(own_pid));
                self.observe(
                    settings,
                    &processes,
                    own_pid,
                    current_session,
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
        self.update_app_suspension(suspended_instances);
        let snapshot = SleepPreventionSnapshot {
            enabled,
            matched_rules: self.matches.rules,
            matched_processes: self.matches.apps.len(),
            observation_known: self.matches.known,
            last_error: error,
            ..Default::default()
        };
        let mode = self.permitted_mode(self.rule_requests);
        self.apply(controller, mode, snapshot, action_log)
    }

    pub(crate) fn note_configuration(&mut self, settings: &Settings) {
        let enabled = settings.general.enabled;
        let rules = &settings.sleep_prevention;
        if self
            .configuration
            .as_ref()
            .is_some_and(|(old_enabled, old_rules)| *old_enabled == enabled && old_rules == rules)
        {
            return;
        }
        let current = (enabled, rules.clone());
        if let Some(previous) = self.configuration.take() {
            self.rearm_withdrawn(&previous, &current);
            self.rearm_withdrawn(&current, &previous);
        }
        self.configuration = Some(current);
    }

    fn rearm_withdrawn(
        &mut self,
        previous: &(bool, SleepPreventionSettings),
        current: &(bool, SleepPreventionSettings),
    ) {
        if !previous.0 || !previous.1.enabled {
            return;
        }
        for rule in previous.1.rules.iter().filter(|rule| rule.enabled) {
            let path = Path::new(rule.executable_path.trim_start());
            if current.0
                && current.1.enabled
                && current.1.rules.iter().any(|rule| {
                    rule.enabled
                        && same_executable_path(Path::new(rule.executable_path.trim_start()), path)
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

    fn update_app_suspension(&mut self, suspended_instances: &[AppIdentity]) {
        for (identity, app) in &mut self.matches.apps {
            app.suspended = self.matches.known && suspended_instances.contains(identity);
        }
    }

    pub(crate) fn suppress_and_release(
        &mut self,
        controller: &mut SleepPreventionController,
        configured_settings: &Settings,
        action_log: &mut ActionLog,
    ) -> SleepPreventionSnapshot {
        // Settings observed with the interruption may have been saved before it.
        self.configuration = Some((
            configured_settings.general.enabled,
            configured_settings.sleep_prevention.clone(),
        ));
        self.suppress_current();
        let mut snapshot = self.snapshot.clone();
        snapshot.failed_actions = 0;
        snapshot.last_error = None;
        self.apply(controller, SleepPreventionMode::Off, snapshot, action_log)
    }

    pub(crate) fn reset(
        &mut self,
        controller: &mut SleepPreventionController,
        action_log: &mut ActionLog,
    ) -> SleepPreventionSnapshot {
        self.temporary = TemporarySleepPreventionTimers::default();
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

    pub(crate) fn unavailable<
        P: crate::platform::windows::sleep_prevention::SleepPreventionPlatform,
    >(
        &mut self,
        controller: &mut SleepPreventionController<P>,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> SleepPreventionSnapshot {
        let snapshot = SleepPreventionSnapshot {
            enabled: true,
            matched_rules: self.matches.rules,
            matched_processes: self.matches.apps.len(),
            last_error: Some(reason.to_owned()),
            ..Default::default()
        };
        self.apply(controller, SleepPreventionMode::Off, snapshot, action_log)
    }

    fn needs_path_observation(
        &self,
        process: &ProcessInfo,
        settings: &SleepPreventionSettings,
    ) -> bool {
        // Unknown creation time can still belong to an accepted or suppressed app.
        let first = (process.id, process.creation_time.unwrap_or(0));
        let last = (process.id, process.creation_time.unwrap_or(u64::MAX));
        self.matches.apps.range(first..=last).next().is_some()
            || self.suppressed.range(first..=last).next().is_some()
            || settings
                .rules
                .iter()
                .filter(|rule| rule.enabled)
                .filter_map(|rule| matchable_target(&rule.executable_path))
                .any(|(_, name)| executable_basename_may_match(&process.name, name))
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
                            same_executable_path(Path::new(path.trim_start()), Path::new(&app.path))
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
                        Path::new(rule.executable_path.trim_start()),
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
                            Path::new(rule.executable_path.trim_start()),
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
            if !app.suspended && !self.suppressed.contains_key(identity) {
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
        retain_accepted_requests(desired, accepted)
    }

    fn apply<P: crate::platform::windows::sleep_prevention::SleepPreventionPlatform>(
        &mut self,
        controller: &mut SleepPreventionController<P>,
        mode: SleepPreventionMode,
        mut snapshot: SleepPreventionSnapshot,
        action_log: &mut ActionLog,
    ) -> SleepPreventionSnapshot {
        let before = controller.requests();
        let reason = rust_i18n::t!("sleep_prevention.request_reason");
        let requested = if self
            .temporary
            .until(TemporarySleepPrevention::KeepDisplayOn)
            .is_some()
        {
            SleepPreventionMode::SystemAndDisplay
        } else if mode == SleepPreventionMode::Off && self.temporary.any_active() {
            SleepPreventionMode::System
        } else {
            mode
        };
        if let Err(error) = controller.reconcile(requested, &reason) {
            if requested == SleepPreventionMode::System && before.display {
                // A failed clear must not keep a display request alive after its last claim.
                let _ = controller.shutdown();
            }
            snapshot.last_error = Some(error.to_string());
        }
        let after = controller.requests();
        if !after.system {
            self.temporary = TemporarySleepPreventionTimers::default();
        } else if !after.display {
            self.temporary
                .set(TemporarySleepPrevention::KeepDisplayOn, None);
            // A failed display-only start may have established an unclaimed System request.
            if mode == SleepPreventionMode::Off && !self.temporary.any_active() {
                let _ = controller.shutdown();
            }
        }
        let after = controller.requests();
        snapshot.temporary = self.temporary;
        self.rule_requests = SleepPreventionRequests {
            system: mode != SleepPreventionMode::Off && after.system,
            display: mode == SleepPreventionMode::SystemAndDisplay && after.display,
        };
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

// Incomplete observations can retain or withdraw accepted counts.
fn retain_accepted_requests(
    desired: SleepPreventionMode,
    accepted: SleepPreventionRequests,
) -> SleepPreventionMode {
    match desired {
        SleepPreventionMode::Off => SleepPreventionMode::Off,
        _ if !accepted.system => SleepPreventionMode::Off,
        SleepPreventionMode::SystemAndDisplay if accepted.display => desired,
        _ => SleepPreventionMode::System,
    }
}

fn matchable_target(executable: &str) -> Option<(&str, &str)> {
    let executable = executable.trim_start();
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
        known: processes.is_empty() || allow_cross_session || current_session.is_some(),
        ..Default::default()
    };
    if !matches.known {
        return matches;
    }
    for rule in settings.rules.iter().filter(|rule| rule.enabled) {
        let Some((executable, name)) = matchable_target(&rule.executable_path) else {
            continue;
        };
        let path = Path::new(executable);
        let mut rule_matched = false;
        for process in processes {
            if process.id == 0
                || process.id == own_pid
                || !executable_basename_may_match(&process.name, name)
                || crate::backend::is_helper_process(process.id)
                || contains_process_name(EXTENDED_BUILT_IN_PROCESS_EXCLUSIONS, &process.name)
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
                    suspended: false,
                });
            rule_matched = true;
        }
        matches.rules += usize::from(rule_matched);
    }
    matches
}

#[cfg(test)]
mod tests;
