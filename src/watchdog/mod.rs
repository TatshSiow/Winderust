use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
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
    rules::{
        Action, ActionExecution, ActionExecutor, AppLifecycleActionBackend, AppMatcher,
        ExecutionFailureState, DEFAULT_EXECUTION_FAILURE_SUPPRESSION_THRESHOLD,
    },
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

const WATCHDOG_ALLOWED_RESTART_EXTENSIONS: &[&str] = &["exe", "com"];
const FAILURE_SUPPRESSION_THRESHOLD: u8 = DEFAULT_EXECUTION_FAILURE_SUPPRESSION_THRESHOLD;

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
    failure_suppression: BTreeMap<String, WatchdogFailureSuppression>,
}

#[derive(Default)]
struct RestartRuleState {
    seen_running: bool,
    missing_since: Option<Instant>,
}

type WatchdogFailureSuppression = ExecutionFailureState;

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
            self.failure_suppression.clear();
            return WatchdogSnapshot {
                enabled: false,
                message: "Automation disabled.".to_owned(),
                ..Default::default()
            };
        }

        if !settings.enabled {
            self.restart_state.clear();
            self.terminated_process_ids.clear();
            self.failure_suppression.clear();
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

        let active_restart_rule_keys = settings
            .rules
            .iter()
            .filter(|rule| rule.enabled && rule.action == WatchdogAction::RestartIfExited)
            .map(watchdog_rule_key)
            .collect::<BTreeSet<_>>();
        self.restart_state
            .retain(|key, _| active_restart_rule_keys.contains(key));
        let active_rule_keys = settings
            .rules
            .iter()
            .filter(|rule| rule.enabled)
            .map(watchdog_rule_key)
            .collect::<BTreeSet<_>>();
        self.failure_suppression
            .retain(|key, _| active_rule_keys.contains(key));

        let now = Instant::now();
        let mut matched_processes = 0;
        let mut terminated_processes = 0;
        let mut restarted_processes = 0;
        let mut skipped_processes = 0;
        let mut failures = WatchdogFailures::default();

        for rule in settings.rules.iter().filter(|rule| rule.enabled) {
            let key = watchdog_rule_key(rule);
            if self.is_rule_suppressed(&key, rule, &mut skipped_processes, action_log) {
                continue;
            }

            let process_name = match watchdog_rule_process_name(rule) {
                Ok(process_name) => process_name,
                Err(err) => {
                    self.record_rule_failure(&key);
                    failures.record("Watchdog rule", None, &rule.process_name, err, action_log);
                    continue;
                }
            };

            if process_name.is_empty() {
                continue;
            }

            let matches = matching_processes(&process_name, &eligible_processes);
            matched_processes += matches.len();

            match rule.action {
                WatchdogAction::TerminateOnLaunch => {
                    for process in matches {
                        if self.terminated_process_ids.contains(&process.id) {
                            continue;
                        }
                        let action = Action::TerminateApp {
                            app: AppMatcher::ProcessName(process.name.clone()),
                        };
                        let mut backend = WatchdogLifecycleBackend::for_process(process.id);
                        let execution =
                            ActionExecutor.apply_app_lifecycle_action(&action, &mut backend);
                        let terminate_error = backend.take_terminate_error();
                        match execution {
                            ActionExecution::Applied => {
                                self.clear_rule_failure(&key);
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
                            ActionExecution::Failed(_err)
                                if matches!(
                                    terminate_error.as_ref(),
                                    Some(
                                        WatchdogError::AccessDenied | WatchdogError::ProcessExited
                                    )
                                ) =>
                            {
                                skipped_processes += 1;
                                if !matches!(
                                    terminate_error.as_ref(),
                                    Some(WatchdogError::ProcessExited)
                                ) {
                                    self.record_rule_failure(&key);
                                }
                                action_log.record(
                                    ActionLogFeature::Watchdog,
                                    Some(process.id),
                                    process.name.clone(),
                                    ActionLogAction::Skip,
                                    ActionLogResult::Skipped,
                                    "Skipped because the process could not be terminated.",
                                );
                            }
                            ActionExecution::Failed(err) => {
                                self.record_rule_failure(&key);
                                failures.record(
                                    "Terminate",
                                    Some(process.id),
                                    &process.name,
                                    err,
                                    action_log,
                                );
                            }
                            ActionExecution::AlreadyApplied | ActionExecution::Unsupported => {}
                        }
                    }
                }
                WatchdogAction::RestartIfExited => {
                    {
                        let state = self.restart_state.entry(key.clone()).or_default();
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
                    }

                    let action = Action::RestartApp {
                        app: AppMatcher::ProcessName(process_name.clone()),
                        launch_path: rule.launch_path.clone(),
                        args: rule.launch_args.clone(),
                    };
                    let mut backend = WatchdogLifecycleBackend::default();
                    match ActionExecutor.apply_app_lifecycle_action(&action, &mut backend) {
                        ActionExecution::Applied => {
                            self.clear_rule_failure(&key);
                            restarted_processes += 1;
                            if let Some(state) = self.restart_state.get_mut(&key) {
                                state.seen_running = false;
                                state.missing_since = None;
                            }
                            action_log.record(
                                ActionLogFeature::Watchdog,
                                None,
                                rule.process_name.clone(),
                                ActionLogAction::Apply,
                                ActionLogResult::Applied,
                                format!("Rule '{}' restarted missing process.", rule_label(rule)),
                            );
                        }
                        ActionExecution::Failed(err) => {
                            self.record_rule_failure(&key);
                            if let Some(state) = self.restart_state.get_mut(&key) {
                                state.missing_since = Some(now);
                            }
                            failures.record("Restart", None, &rule.process_name, err, action_log);
                        }
                        ActionExecution::AlreadyApplied | ActionExecution::Unsupported => {}
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

    fn is_rule_suppressed(
        &mut self,
        key: &str,
        rule: &WatchdogRule,
        skipped_processes: &mut usize,
        action_log: &mut ActionLog,
    ) -> bool {
        let Some(suppression) = self.failure_suppression.get_mut(key) else {
            return false;
        };
        if !suppression.is_suppressed_at(FAILURE_SUPPRESSION_THRESHOLD) {
            return false;
        }

        if suppression.mark_suppression_logged() {
            action_log.record(
                ActionLogFeature::Watchdog,
                None,
                rule.process_name.clone(),
                ActionLogAction::Skip,
                ActionLogResult::Skipped,
                format!(
                    "Stopped retrying Watchdog rule '{}' after {FAILURE_SUPPRESSION_THRESHOLD} failed attempts.",
                    rule_label(rule)
                ),
            );
        }
        *skipped_processes += 1;
        true
    }

    fn record_rule_failure(&mut self, key: &str) {
        let suppression = self.failure_suppression.entry(key.to_owned()).or_default();
        suppression.record_failure();
    }

    fn clear_rule_failure(&mut self, key: &str) {
        self.failure_suppression.remove(key);
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
    process_name: &str,
    processes: &'a [ProcessInfo],
) -> Vec<&'a ProcessInfo> {
    processes
        .iter()
        .filter(|process| process.name.trim().eq_ignore_ascii_case(process_name))
        .collect()
}

#[derive(Default)]
struct WatchdogLifecycleBackend {
    terminate_process_id: Option<u32>,
    terminate_error: Option<WatchdogError>,
}

impl WatchdogLifecycleBackend {
    fn for_process(process_id: u32) -> Self {
        Self {
            terminate_process_id: Some(process_id),
            terminate_error: None,
        }
    }

    fn take_terminate_error(&mut self) -> Option<WatchdogError> {
        self.terminate_error.take()
    }
}

impl AppLifecycleActionBackend for WatchdogLifecycleBackend {
    fn terminate_app(&mut self, _app: &AppMatcher) -> Result<(), String> {
        let process_id = self
            .terminate_process_id
            .ok_or_else(|| "Watchdog terminate requires a selected process ID.".to_owned())?;
        match terminate_process(process_id) {
            Ok(()) => Ok(()),
            Err(error) => {
                let message = watchdog_error_message(&error);
                self.terminate_error = Some(error);
                Err(message)
            }
        }
    }

    fn restart_app(
        &mut self,
        _app: &AppMatcher,
        launch_path: &str,
        args: &[String],
    ) -> Result<(), String> {
        restart_launch_path(launch_path, args)
    }
}

fn restart_launch_path(launch_path: &str, args: &[String]) -> Result<(), String> {
    let launch_path = canonical_watchdog_launch_path(launch_path)?;
    let launch_args = args
        .iter()
        .map(|arg| validate_watchdog_arg(arg))
        .collect::<Result<Vec<_>, _>>()?;

    Command::new(&launch_path)
        .args(launch_args)
        .spawn()
        .map(|_| ())
        .map_err(|err| format!("Failed to start {}: {err}", launch_path.display()))
}

fn canonical_watchdog_launch_path(launch_path: &str) -> Result<PathBuf, String> {
    let launch_path = normalize_watchdog_launch_path(launch_path);
    if launch_path.is_empty() {
        return Err("Restart rule has no executable path.".to_owned());
    }
    if contains_invalid_watchdog_text(&launch_path) {
        return Err("Restart path contains an invalid character.".to_owned());
    }

    let path = Path::new(&launch_path);
    if !path.is_absolute() {
        return Err(format!(
            "Restart path must be an absolute executable path: {launch_path}"
        ));
    }

    let path = path
        .canonicalize()
        .map_err(|err| format!("Failed to resolve executable path '{launch_path}': {err}"))?;
    if let Some(extension) = path.extension().and_then(|extension| extension.to_str()) {
        if !WATCHDOG_ALLOWED_RESTART_EXTENSIONS
            .iter()
            .any(|allowed| extension.eq_ignore_ascii_case(allowed))
        {
            return Err(format!(
                "Restart path must use an executable extension ({:?}): {}",
                WATCHDOG_ALLOWED_RESTART_EXTENSIONS,
                path.display()
            ));
        }
    } else {
        return Err(format!(
            "Restart path must include a file extension: {}",
            path.display()
        ));
    }

    let metadata = path.metadata().map_err(|err| {
        format!(
            "Failed to read executable metadata for '{}': {err}",
            path.display()
        )
    })?;

    if !metadata.is_file() {
        return Err(format!(
            "Restart path is not an executable file: {}",
            path.display()
        ));
    }

    Ok(path)
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
        watchdog_rule_process_name(rule)
            .unwrap_or_else(|_| rule.process_name.trim().to_ascii_lowercase()),
        watchdog_launch_path_key(rule),
        rule.launch_args.join("\0")
    )
}

fn watchdog_rule_process_name(rule: &WatchdogRule) -> Result<String, String> {
    let process_name = rule.process_name.trim();
    if process_name.is_empty() {
        return Err("Watchdog rule has no process name.".to_owned());
    }
    if process_name.contains('\0') {
        return Err("Watchdog process name contains an invalid character.".to_owned());
    }

    let has_invalid_character = process_name.chars().any(|character| {
        character.is_control()
            || matches!(
                character,
                '\\' | '/' | ':' | '*' | '?' | '\"' | '<' | '>' | '|'
            )
    });
    if has_invalid_character {
        return Err(format!(
            "Watchdog process name contains invalid characters: {process_name}"
        ));
    }

    Ok(process_name.to_ascii_lowercase())
}

fn normalize_watchdog_launch_path(value: &str) -> String {
    value.trim().trim_matches('"').to_owned()
}

fn validate_watchdog_arg(value: &str) -> Result<String, String> {
    if contains_invalid_watchdog_text(value) {
        return Err("Restart argument contains an invalid character.".to_owned());
    }

    Ok(value.to_owned())
}

fn contains_invalid_watchdog_text(value: &str) -> bool {
    value
        .chars()
        .any(|character| character.is_control() || character == '\0')
}

fn watchdog_launch_path_key(rule: &WatchdogRule) -> String {
    match canonical_watchdog_launch_path(&rule.launch_path) {
        Ok(path) => path.to_string_lossy().to_ascii_lowercase(),
        Err(_) => rule.launch_path.trim().to_ascii_lowercase(),
    }
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
        let mut handle = unsafe {
            OpenProcess(
                PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION,
                0,
                process_id,
            )
        };
        if handle.is_null() && last_error() == ERROR_ACCESS_DENIED {
            if crate::privilege::enable_debug_privilege() {
                handle = unsafe {
                    OpenProcess(
                        PROCESS_TERMINATE | PROCESS_QUERY_LIMITED_INFORMATION,
                        0,
                        process_id,
                    )
                };
            }
        }
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

fn watchdog_error_message(error: &WatchdogError) -> String {
    match error {
        WatchdogError::AccessDenied => "Access denied.".to_owned(),
        WatchdogError::ProcessExited => "Process exited.".to_owned(),
        WatchdogError::Failed(message) => message.clone(),
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
        let processes = vec![ProcessInfo {
            id: 42,
            name: "tool.exe".to_owned(),
        }];

        assert_eq!(matching_processes("tool.exe", &processes).len(), 1);
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

    #[test]
    fn repeated_failures_suppress_future_watchdog_attempts_once() {
        let mut manager = WatchdogManager::default();
        let mut log = ActionLog::new(8);
        let rule = WatchdogRule {
            enabled: true,
            name: "Restart Tool".to_owned(),
            process_name: "Tool.EXE".to_owned(),
            action: WatchdogAction::RestartIfExited,
            launch_path: "C:\\Tools\\tool.exe".to_owned(),
            launch_args: Vec::new(),
            restart_delay_seconds: 5,
        };
        let key = watchdog_rule_key(&rule);
        let mut skipped = 0;

        manager.record_rule_failure(&key);
        manager.record_rule_failure(&key);
        assert!(!manager.is_rule_suppressed(&key, &rule, &mut skipped, &mut log));
        assert_eq!(skipped, 0);
        assert!(log.entries().is_empty());

        manager.record_rule_failure(&key);
        assert!(manager.is_rule_suppressed(&key, &rule, &mut skipped, &mut log));
        assert!(manager.is_rule_suppressed(&key, &rule, &mut skipped, &mut log));

        let entries = log.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].process_name, "Tool.EXE");
        assert_eq!(entries[0].action, ActionLogAction::Skip);
        assert_eq!(entries[0].result, ActionLogResult::Skipped);
        assert_eq!(skipped, 2);
    }

    #[test]
    fn restart_extensions_exclude_command_scripts() {
        assert!(WATCHDOG_ALLOWED_RESTART_EXTENSIONS.contains(&"exe"));
        assert!(WATCHDOG_ALLOWED_RESTART_EXTENSIONS.contains(&"com"));
        assert!(!WATCHDOG_ALLOWED_RESTART_EXTENSIONS.contains(&"bat"));
        assert!(!WATCHDOG_ALLOWED_RESTART_EXTENSIONS.contains(&"cmd"));
    }

    #[test]
    fn watchdog_args_reject_control_characters() {
        assert_eq!(
            validate_watchdog_arg("--label=hello").unwrap(),
            "--label=hello"
        );
        assert!(validate_watchdog_arg("--label=hello\nworld").is_err());
        assert!(validate_watchdog_arg("bad\0arg").is_err());
    }

    #[test]
    fn lifecycle_backend_routes_restart_through_watchdog_validation() {
        let mut backend = WatchdogLifecycleBackend::default();
        let action = Action::RestartApp {
            app: AppMatcher::ProcessName("tool.exe".to_owned()),
            launch_path: "relative-tool.exe".to_owned(),
            args: Vec::new(),
        };

        assert!(matches!(
            ActionExecutor.apply_app_lifecycle_action(&action, &mut backend),
            ActionExecution::Failed(message)
                if message.contains("Restart path must be an absolute executable path")
        ));
    }

    #[test]
    fn lifecycle_backend_requires_pid_for_terminate() {
        let mut backend = WatchdogLifecycleBackend::default();
        let action = Action::TerminateApp {
            app: AppMatcher::ProcessName("tool.exe".to_owned()),
        };

        assert_eq!(
            ActionExecutor.apply_app_lifecycle_action(&action, &mut backend),
            ActionExecution::Failed(
                "Watchdog terminate requires a selected process ID.".to_owned()
            )
        );
    }
}
