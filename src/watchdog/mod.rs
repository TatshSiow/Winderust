use std::{
    collections::{BTreeMap, BTreeSet},
    process::Command,
    time::{Duration, Instant},
};

use windows_sys::Win32::{
    Foundation::{CloseHandle, GetLastError, ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER, HANDLE},
    System::{
        RemoteDesktop::ProcessIdToSessionId,
        Threading::{
            GetCurrentProcessId, OpenProcess, TerminateProcess, PROCESS_QUERY_LIMITED_INFORMATION,
            PROCESS_TERMINATE,
        },
    },
};

use crate::{
    action_log::{ActionLog, ActionLogAction, ActionLogFeature, ActionLogResult},
    config::{WatchdogAction, WatchdogRule, WatchdogSettings},
    foreground::{list_processes, ProcessInfo},
};

const BUILT_IN_EXCLUSIONS: &[&str] = &[
    "audiodg.exe",
    "conhost.exe",
    "csrss.exe",
    "ctfmon.exe",
    "dwm.exe",
    "explorer.exe",
    "fontdrvhost.exe",
    "lsaiso.exe",
    "lsass.exe",
    "registry",
    "searchapp.exe",
    "searchhost.exe",
    "securityhealthservice.exe",
    "securityhealthsystray.exe",
    "services.exe",
    "shellexperiencehost.exe",
    "sihost.exe",
    "smss.exe",
    "startmenuexperiencehost.exe",
    "system",
    "systemsettings.exe",
    "taskmgr.exe",
    "textinputhost.exe",
    "wininit.exe",
    "winlogon.exe",
    "wudfhost.exe",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchdogSnapshot {
    pub enabled: bool,
    pub scanned_processes: usize,
    pub matched_processes: usize,
    pub terminated_processes: usize,
    pub restarted_processes: usize,
    pub skipped_processes: usize,
    pub failed_actions: usize,
    pub message: String,
    pub last_error: Option<String>,
}

#[derive(Default)]
pub struct WatchdogManager {
    restart_state: BTreeMap<String, RestartRuleState>,
    terminated_process_ids: BTreeSet<u32>,
}

#[derive(Default)]
struct RestartRuleState {
    seen_running: bool,
    missing_since: Option<Instant>,
}

#[derive(Default)]
struct WatchdogFailures {
    count: usize,
    last_error: Option<String>,
}

impl WatchdogManager {
    pub fn update(
        &mut self,
        settings: &WatchdogSettings,
        automation_enabled: bool,
        action_log: &mut ActionLog,
    ) -> WatchdogSnapshot {
        if !automation_enabled {
            self.restart_state.clear();
            self.terminated_process_ids.clear();
            return WatchdogSnapshot {
                enabled: false,
                message: "Automation disabled.".to_owned(),
                ..Default::default()
            };
        }

        if !settings.enabled {
            self.restart_state.clear();
            self.terminated_process_ids.clear();
            return WatchdogSnapshot {
                enabled: false,
                message: "Watchdog Rules disabled.".to_owned(),
                ..Default::default()
            };
        }

        let current_process_id = unsafe { GetCurrentProcessId() };
        let Some(current_session_id) = process_session_id(current_process_id) else {
            return WatchdogSnapshot {
                enabled: true,
                message: "Paused: current Windows session is unknown.".to_owned(),
                ..Default::default()
            };
        };

        let processes = match list_processes() {
            Ok(processes) => processes,
            Err(err) => {
                return WatchdogSnapshot {
                    enabled: true,
                    message: err,
                    ..Default::default()
                };
            }
        };

        let scanned_processes = processes.len();
        let current_process_ids = processes
            .iter()
            .map(|process| process.id)
            .collect::<BTreeSet<_>>();
        self.terminated_process_ids
            .retain(|process_id| current_process_ids.contains(process_id));

        let eligible_processes = processes
            .into_iter()
            .filter(|process| {
                process.id != 0
                    && process.id != current_process_id
                    && process_session_id(process.id) == Some(current_session_id)
                    && !is_builtin_excluded(&process.name)
            })
            .collect::<Vec<_>>();

        let active_rule_keys = settings
            .rules
            .iter()
            .filter(|rule| rule.enabled && rule.action == WatchdogAction::RestartIfExited)
            .map(watchdog_rule_key)
            .collect::<BTreeSet<_>>();
        self.restart_state
            .retain(|key, _| active_rule_keys.contains(key));

        let now = Instant::now();
        let mut matched_processes = 0;
        let mut terminated_processes = 0;
        let mut restarted_processes = 0;
        let mut skipped_processes = 0;
        let mut failures = WatchdogFailures::default();

        for rule in settings.rules.iter().filter(|rule| rule.enabled) {
            if rule.process_name.trim().is_empty() {
                continue;
            }

            let matches = matching_processes(rule, &eligible_processes);
            matched_processes += matches.len();

            match rule.action {
                WatchdogAction::TerminateOnLaunch => {
                    for process in matches {
                        if self.terminated_process_ids.contains(&process.id) {
                            continue;
                        }
                        match terminate_process(process.id) {
                            Ok(()) => {
                                self.terminated_process_ids.insert(process.id);
                                terminated_processes += 1;
                                action_log.record(
                                    ActionLogFeature::Watchdog,
                                    Some(process.id),
                                    process.name.clone(),
                                    ActionLogAction::Apply,
                                    ActionLogResult::Applied,
                                    format!(
                                        "Rule '{}' terminated matching process.",
                                        rule_label(rule)
                                    ),
                                );
                            }
                            Err(WatchdogError::AccessDenied | WatchdogError::ProcessExited) => {
                                skipped_processes += 1;
                                action_log.record(
                                    ActionLogFeature::Watchdog,
                                    Some(process.id),
                                    process.name.clone(),
                                    ActionLogAction::Skip,
                                    ActionLogResult::Skipped,
                                    "Skipped because the process could not be terminated.",
                                );
                            }
                            Err(WatchdogError::Failed(err)) => {
                                failures.record(
                                    "Terminate",
                                    Some(process.id),
                                    &process.name,
                                    err,
                                    action_log,
                                );
                            }
                        }
                    }
                }
                WatchdogAction::RestartIfExited => {
                    let key = watchdog_rule_key(rule);
                    let state = self.restart_state.entry(key).or_default();
                    if !matches.is_empty() {
                        state.seen_running = true;
                        state.missing_since = None;
                        continue;
                    }

                    if !state.seen_running {
                        continue;
                    }

                    let missing_since = *state.missing_since.get_or_insert(now);
                    if now.duration_since(missing_since)
                        < Duration::from_secs(rule.restart_delay_seconds)
                    {
                        continue;
                    }

                    match restart_process(rule) {
                        Ok(()) => {
                            restarted_processes += 1;
                            state.seen_running = false;
                            state.missing_since = None;
                            action_log.record(
                                ActionLogFeature::Watchdog,
                                None,
                                rule.process_name.clone(),
                                ActionLogAction::Apply,
                                ActionLogResult::Applied,
                                format!("Rule '{}' restarted missing process.", rule_label(rule)),
                            );
                        }
                        Err(err) => {
                            state.missing_since = Some(now);
                            failures.record("Restart", None, &rule.process_name, err, action_log);
                        }
                    }
                }
            }
        }

        WatchdogSnapshot {
            enabled: true,
            scanned_processes,
            matched_processes,
            terminated_processes,
            restarted_processes,
            skipped_processes,
            failed_actions: failures.count,
            message: "Watchdog Rules active.".to_owned(),
            last_error: failures.last_error,
        }
    }
}

impl Default for WatchdogSnapshot {
    fn default() -> Self {
        Self {
            enabled: false,
            scanned_processes: 0,
            matched_processes: 0,
            terminated_processes: 0,
            restarted_processes: 0,
            skipped_processes: 0,
            failed_actions: 0,
            message: "Watchdog Rules disabled.".to_owned(),
            last_error: None,
        }
    }
}

impl WatchdogFailures {
    fn record(
        &mut self,
        action: &str,
        process_id: Option<u32>,
        process_name: &str,
        message: String,
        action_log: &mut ActionLog,
    ) {
        self.count += 1;
        if self.last_error.is_none() {
            self.last_error = Some(match process_id {
                Some(process_id) => format!("{action} {process_name} ({process_id}): {message}"),
                None => format!("{action} {process_name}: {message}"),
            });
        }
        action_log.record(
            ActionLogFeature::Watchdog,
            process_id,
            process_name.to_owned(),
            ActionLogAction::Fail,
            ActionLogResult::Failed,
            message,
        );
    }
}

pub fn is_builtin_excluded(process_name: &str) -> bool {
    let process_name = process_name.trim();
    BUILT_IN_EXCLUSIONS
        .iter()
        .any(|excluded| excluded.eq_ignore_ascii_case(process_name))
}

fn matching_processes<'a>(
    rule: &WatchdogRule,
    processes: &'a [ProcessInfo],
) -> Vec<&'a ProcessInfo> {
    processes
        .iter()
        .filter(|process| {
            process
                .name
                .trim()
                .eq_ignore_ascii_case(rule.process_name.trim())
        })
        .collect()
}

fn restart_process(rule: &WatchdogRule) -> Result<(), String> {
    let launch_path = rule.launch_path.trim();
    if launch_path.is_empty() {
        return Err("Restart rule has no executable path.".to_owned());
    }

    Command::new(launch_path)
        .args(rule.launch_args.iter().map(String::as_str))
        .spawn()
        .map(|_| ())
        .map_err(|err| format!("Failed to start {launch_path}: {err}"))
}

fn terminate_process(process_id: u32) -> Result<(), WatchdogError> {
    let process = ProcessHandle::open(process_id)?;
    let ok = unsafe { TerminateProcess(process.0, 1) };
    if ok == 0 {
        Err(WatchdogError::Failed(format!(
            "TerminateProcess failed with error {}.",
            last_error()
        )))
    } else {
        Ok(())
    }
}

fn process_session_id(process_id: u32) -> Option<u32> {
    let mut session_id = 0;
    let ok = unsafe { ProcessIdToSessionId(process_id, &mut session_id) };
    (ok != 0).then_some(session_id)
}

fn watchdog_rule_key(rule: &WatchdogRule) -> String {
    format!(
        "{}\0{}\0{}",
        rule.process_name.trim().to_ascii_lowercase(),
        rule.launch_path.trim().to_ascii_lowercase(),
        rule.launch_args.join("\0")
    )
}

fn rule_label(rule: &WatchdogRule) -> String {
    let name = rule.name.trim();
    if name.is_empty() {
        rule.process_name.trim().to_owned()
    } else {
        name.to_owned()
    }
}

enum WatchdogError {
    AccessDenied,
    ProcessExited,
    Failed(String),
}

struct ProcessHandle(HANDLE);

impl ProcessHandle {
    fn open(process_id: u32) -> Result<Self, WatchdogError> {
        let handle = unsafe {
            OpenProcess(
                PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION,
                0,
                process_id,
            )
        };
        if !handle.is_null() {
            Ok(Self(handle))
        } else {
            Err(open_process_error(process_id, last_error()))
        }
    }
}

impl Drop for ProcessHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

fn open_process_error(process_id: u32, error: u32) -> WatchdogError {
    match error {
        ERROR_ACCESS_DENIED => WatchdogError::AccessDenied,
        ERROR_INVALID_PARAMETER => WatchdogError::ProcessExited,
        _ => WatchdogError::Failed(format!(
            "OpenProcess({process_id}) failed with error {error}."
        )),
    }
}

fn last_error() -> u32 {
    unsafe { GetLastError() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching_processes_are_case_insensitive() {
        let rule = WatchdogRule {
            enabled: true,
            name: "Block".to_owned(),
            process_name: "Tool.EXE".to_owned(),
            action: WatchdogAction::TerminateOnLaunch,
            launch_path: String::new(),
            launch_args: Vec::new(),
            restart_delay_seconds: 5,
        };
        let processes = vec![ProcessInfo {
            id: 42,
            name: "tool.exe".to_owned(),
        }];

        assert_eq!(matching_processes(&rule, &processes).len(), 1);
    }

    #[test]
    fn builtin_exclusions_cover_sensitive_windows_processes() {
        assert!(is_builtin_excluded("csrss.exe"));
        assert!(is_builtin_excluded("winlogon.exe"));
        assert!(!is_builtin_excluded("tool.exe"));
    }

    #[test]
    fn rule_key_includes_restart_target() {
        let first = WatchdogRule {
            enabled: true,
            name: String::new(),
            process_name: "tool.exe".to_owned(),
            action: WatchdogAction::RestartIfExited,
            launch_path: "C:\\Tools\\tool.exe".to_owned(),
            launch_args: vec!["--one".to_owned()],
            restart_delay_seconds: 5,
        };
        let mut second = first.clone();
        second.launch_args = vec!["--two".to_owned()];

        assert_ne!(watchdog_rule_key(&first), watchdog_rule_key(&second));
    }
}
