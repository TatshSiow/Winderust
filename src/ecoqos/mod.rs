use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::c_void,
};

use windows_sys::Win32::{
    Foundation::{
        CloseHandle, GetLastError, ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER,
        ERROR_NOT_SUPPORTED, HANDLE,
    },
    System::{
        RemoteDesktop::ProcessIdToSessionId,
        Threading::{
            GetCurrentProcessId, GetPriorityClass, GetProcessInformation, OpenProcess,
            ProcessPowerThrottling, SetPriorityClass, SetProcessInformation, IDLE_PRIORITY_CLASS,
            NORMAL_PRIORITY_CLASS, PROCESS_POWER_THROTTLING_CURRENT_VERSION,
            PROCESS_POWER_THROTTLING_EXECUTION_SPEED, PROCESS_POWER_THROTTLING_STATE,
            PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_INFORMATION,
            PROCESS_SET_LIMITED_INFORMATION,
        },
    },
};

use crate::{
    action_log::{ActionLog, ActionLogAction, ActionLogFeature, ActionLogResult},
    config::EcoQosSettings,
    foreground::list_processes,
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
    "searchhost.exe",
    "securityhealthservice.exe",
    "securityhealthsystray.exe",
    "services.exe",
    "shellexperiencehost.exe",
    "sihost.exe",
    "smss.exe",
    "startmenuexperiencehost.exe",
    "system",
    "taskmgr.exe",
    "textinputhost.exe",
    "wininit.exe",
    "winlogon.exe",
    "wudfhost.exe",
];
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EcoQosSnapshot {
    pub enabled: bool,
    pub unsupported: bool,
    pub scanned_processes: usize,
    pub throttled_processes: usize,
    pub skipped_processes: usize,
    pub failed_processes: usize,
    pub message: String,
    pub last_error: Option<String>,
}

#[derive(Default)]
pub struct EcoQosManager {
    throttled: BTreeMap<u32, ThrottledProcess>,
}

struct ThrottledProcess {
    process_name: String,
    previous_state: Option<PROCESS_POWER_THROTTLING_STATE>,
    previous_priority: Option<u32>,
}

impl EcoQosManager {
    pub fn throttled_process_ids(&self) -> BTreeSet<u32> {
        self.throttled.keys().copied().collect()
    }

    pub fn update(
        &mut self,
        settings: &EcoQosSettings,
        automation_enabled: bool,
        foreground_process_id: Option<u32>,
        action_log: &mut ActionLog,
    ) -> EcoQosSnapshot {
        if !automation_enabled {
            let failed = self.clear_all(action_log, "automation disabled");
            return EcoQosSnapshot {
                enabled: false,
                failed_processes: failed.count,
                message: "Automation disabled.".to_owned(),
                last_error: failed.last_error,
                ..Default::default()
            };
        }

        if !settings.enabled {
            let failed = self.clear_all(action_log, "Efficiency Mode disabled");
            return EcoQosSnapshot {
                enabled: false,
                failed_processes: failed.count,
                message: "Efficiency Mode disabled.".to_owned(),
                last_error: failed.last_error,
                ..Default::default()
            };
        }

        if settings.exclude_foreground_app && foreground_process_id.is_none() {
            let failed = self.clear_all(action_log, "foreground app is unknown");
            return EcoQosSnapshot {
                enabled: true,
                failed_processes: failed.count,
                message: "Paused: foreground app is unknown.".to_owned(),
                last_error: failed.last_error,
                ..Default::default()
            };
        }

        let current_process_id = unsafe { GetCurrentProcessId() };
        let Some(current_session_id) = process_session_id(current_process_id) else {
            let failed = self.clear_all(action_log, "current Windows session is unknown");
            return EcoQosSnapshot {
                enabled: true,
                failed_processes: failed.count,
                message: "Paused: current Windows session is unknown.".to_owned(),
                last_error: failed.last_error,
                ..Default::default()
            };
        };

        let processes = match list_processes() {
            Ok(processes) => processes,
            Err(err) => {
                let failed = self.clear_all(action_log, "process list unavailable");
                return EcoQosSnapshot {
                    enabled: true,
                    failed_processes: failed.count,
                    message: err,
                    last_error: failed.last_error,
                    ..Default::default()
                };
            }
        };

        let scanned_processes = processes.len();
        let mut skipped_processes = 0;
        let foreground_process_name = if settings.exclude_foreground_app {
            foreground_process_id.and_then(|id| {
                processes
                    .iter()
                    .find(|process| process.id == id)
                    .map(|process| process.name.clone())
            })
        } else {
            None
        };
        let mut target_processes = BTreeMap::new();
        for process in processes {
            if process.id == 0
                || process.id == current_process_id
                || should_ignore_foreground_process(
                    settings,
                    process.id,
                    &process.name,
                    foreground_process_id,
                    foreground_process_name.as_deref(),
                )
                || is_process_excluded(&process.name, settings)
            {
                continue;
            }

            if process_session_id(process.id) != Some(current_session_id) {
                continue;
            }

            target_processes.insert(process.id, process.name);
        }

        let target_ids = target_processes.keys().copied().collect::<BTreeSet<_>>();
        let mut failures =
            self.release_non_targets(&target_ids, action_log, "process no longer matches EcoQoS");
        let mut unsupported = false;

        for (process_id, name) in target_processes {
            if self.throttled.contains_key(&process_id) {
                continue;
            }

            match enable_efficiency_mode(process_id, name.clone()) {
                Ok(process) => {
                    self.throttled.insert(process_id, process);
                    action_log.record(
                        ActionLogFeature::EcoQos,
                        Some(process_id),
                        name,
                        ActionLogAction::Apply,
                        ActionLogResult::Applied,
                        "Enabled Windows Efficiency Mode and lowered priority.",
                    );
                }
                Err(EcoQosError::AccessDenied | EcoQosError::ProcessExited) => {
                    skipped_processes += 1;
                    action_log.record(
                        ActionLogFeature::EcoQos,
                        Some(process_id),
                        name,
                        ActionLogAction::Skip,
                        ActionLogResult::Skipped,
                        "Skipped because the process could not be opened.",
                    );
                }
                Err(EcoQosError::Unsupported) => {
                    skipped_processes += 1;
                    unsupported = true;
                    action_log.record(
                        ActionLogFeature::EcoQos,
                        Some(process_id),
                        name,
                        ActionLogAction::Skip,
                        ActionLogResult::Skipped,
                        "Skipped because Windows process power throttling is unsupported.",
                    );
                }
                Err(EcoQosError::Failed(err)) => {
                    failures.record_message("Apply", process_id, &name, err, action_log);
                }
            }
        }

        EcoQosSnapshot {
            enabled: true,
            unsupported,
            scanned_processes,
            throttled_processes: self.throttled.len(),
            skipped_processes,
            failed_processes: failures.count,
            message: "Efficiency Mode active.".to_owned(),
            last_error: failures.last_error,
        }
    }

    fn release_non_targets(
        &mut self,
        target_ids: &BTreeSet<u32>,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> EcoQosFailures {
        let process_ids = self
            .throttled
            .keys()
            .copied()
            .filter(|process_id| !target_ids.contains(process_id))
            .collect::<Vec<_>>();

        self.release_processes(&process_ids, action_log, reason)
    }

    fn clear_all(&mut self, action_log: &mut ActionLog, reason: &str) -> EcoQosFailures {
        let process_ids = self.throttled.keys().copied().collect::<Vec<_>>();
        self.release_processes(&process_ids, action_log, reason)
    }

    fn release_processes(
        &mut self,
        process_ids: &[u32],
        action_log: &mut ActionLog,
        reason: &str,
    ) -> EcoQosFailures {
        let mut failures = EcoQosFailures::default();
        for process_id in process_ids {
            if let Some(process) = self.throttled.remove(process_id) {
                let process_name = process.process_name.clone();
                if let Err(err) = restore_efficiency_mode(*process_id, process) {
                    if !matches!(err, EcoQosError::ProcessExited) {
                        failures.record_error(
                            "Restore",
                            *process_id,
                            &process_name,
                            err,
                            action_log,
                        );
                    }
                } else {
                    action_log.record(
                        ActionLogFeature::EcoQos,
                        Some(*process_id),
                        process_name,
                        ActionLogAction::Restore,
                        ActionLogResult::Restored,
                        reason.to_owned(),
                    );
                }
            }
        }
        failures
    }
}

fn should_ignore_foreground_process(
    settings: &EcoQosSettings,
    process_id: u32,
    process_name: &str,
    foreground_process_id: Option<u32>,
    foreground_process_name: Option<&str>,
) -> bool {
    settings.exclude_foreground_app
        && (foreground_process_id.is_some_and(|id| id == process_id)
            || foreground_process_name
                .is_some_and(|name| name.eq_ignore_ascii_case(process_name.trim())))
}

impl Drop for EcoQosManager {
    fn drop(&mut self) {
        let mut action_log = ActionLog::new(1);
        self.clear_all(&mut action_log, "EcoQoS manager dropped");
    }
}

impl Default for EcoQosSnapshot {
    fn default() -> Self {
        Self {
            enabled: false,
            unsupported: false,
            scanned_processes: 0,
            throttled_processes: 0,
            skipped_processes: 0,
            failed_processes: 0,
            message: "Efficiency Mode disabled.".to_owned(),
            last_error: None,
        }
    }
}

pub fn is_process_excluded(process_name: &str, settings: &EcoQosSettings) -> bool {
    let process_name = process_name.trim();
    BUILT_IN_EXCLUSIONS
        .iter()
        .any(|excluded| excluded.eq_ignore_ascii_case(process_name))
        || settings
            .efficiency_whitelist
            .iter()
            .any(|excluded| excluded.trim().eq_ignore_ascii_case(process_name))
}

fn process_session_id(process_id: u32) -> Option<u32> {
    let mut session_id = 0;
    let ok = unsafe { ProcessIdToSessionId(process_id, &mut session_id) };
    (ok != 0).then_some(session_id)
}

enum EcoQosError {
    AccessDenied,
    ProcessExited,
    Unsupported,
    Failed(String),
}

#[derive(Default)]
struct EcoQosFailures {
    count: usize,
    last_error: Option<String>,
}

impl EcoQosFailures {
    fn record_error(
        &mut self,
        action: &str,
        process_id: u32,
        process_name: &str,
        error: EcoQosError,
        action_log: &mut ActionLog,
    ) {
        let message = match error {
            EcoQosError::AccessDenied => "Access denied.".to_owned(),
            EcoQosError::ProcessExited => "Process exited.".to_owned(),
            EcoQosError::Unsupported => "Operation unsupported.".to_owned(),
            EcoQosError::Failed(message) => message,
        };
        self.record_message(action, process_id, process_name, message, action_log);
    }

    fn record_message(
        &mut self,
        action: &str,
        process_id: u32,
        process_name: &str,
        message: String,
        action_log: &mut ActionLog,
    ) {
        self.count += 1;
        if self.last_error.is_none() {
            self.last_error = Some(process_failure_message(
                action,
                process_id,
                process_name,
                &message,
            ));
        }
        action_log.record(
            ActionLogFeature::EcoQos,
            Some(process_id),
            process_name.to_owned(),
            ActionLogAction::Fail,
            ActionLogResult::Failed,
            message,
        );
    }
}

fn process_failure_message(
    action: &str,
    process_id: u32,
    process_name: &str,
    message: &str,
) -> String {
    format!("{action} {process_name} ({process_id}): {message}")
}

fn enable_efficiency_mode(
    process_id: u32,
    process_name: String,
) -> Result<ThrottledProcess, EcoQosError> {
    let process = ProcessHandle::open(process_id)?;
    let previous_state = process.power_throttling_state().ok();
    let previous_priority = process.priority_class().ok();

    let mut next_state = previous_state.unwrap_or_default();
    next_state.Version = PROCESS_POWER_THROTTLING_CURRENT_VERSION;
    next_state.ControlMask |= PROCESS_POWER_THROTTLING_EXECUTION_SPEED;
    next_state.StateMask |= PROCESS_POWER_THROTTLING_EXECUTION_SPEED;
    process.set_power_throttling_state(next_state)?;
    if let Err(err) = process.set_priority_class(IDLE_PRIORITY_CLASS) {
        let _ = process.set_power_throttling_state(
            previous_state.unwrap_or_else(power_throttling_disabled_state),
        );
        return Err(err);
    }

    Ok(ThrottledProcess {
        process_name,
        previous_state,
        previous_priority,
    })
}

fn restore_efficiency_mode(
    process_id: u32,
    process_state: ThrottledProcess,
) -> Result<(), EcoQosError> {
    let process = ProcessHandle::open(process_id)?;
    let mut last_error = None;

    if let Err(err) = process.set_power_throttling_state(
        process_state
            .previous_state
            .unwrap_or_else(power_throttling_disabled_state),
    ) {
        last_error = Some(err);
    }

    if let Err(err) = process.set_priority_class(
        process_state
            .previous_priority
            .unwrap_or(NORMAL_PRIORITY_CLASS),
    ) {
        last_error = Some(err);
    }

    match last_error {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

fn power_throttling_disabled_state() -> PROCESS_POWER_THROTTLING_STATE {
    PROCESS_POWER_THROTTLING_STATE {
        Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
        StateMask: 0,
    }
}

struct ProcessHandle(HANDLE);

impl ProcessHandle {
    fn open(process_id: u32) -> Result<Self, EcoQosError> {
        let access_masks = [
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_INFORMATION,
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_LIMITED_INFORMATION,
        ];

        let mut last_open_error = 0;
        for access in access_masks {
            let handle = unsafe { OpenProcess(access, 0, process_id) };
            if !handle.is_null() {
                return Ok(Self(handle));
            }
            last_open_error = last_error();
        }

        Err(open_process_error(process_id, last_open_error))
    }

    fn power_throttling_state(&self) -> Result<PROCESS_POWER_THROTTLING_STATE, EcoQosError> {
        let mut state = PROCESS_POWER_THROTTLING_STATE::default();
        let ok = unsafe {
            GetProcessInformation(
                self.0,
                ProcessPowerThrottling,
                &mut state as *mut _ as *mut c_void,
                std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
            )
        };
        if ok == 0 {
            Err(process_power_throttling_error(
                "GetProcessInformation",
                last_error(),
            ))
        } else {
            Ok(state)
        }
    }

    fn priority_class(&self) -> Result<u32, EcoQosError> {
        let priority = unsafe { GetPriorityClass(self.0) };
        if priority == 0 {
            Err(EcoQosError::Failed(format!(
                "GetPriorityClass failed with error {}.",
                last_error()
            )))
        } else {
            Ok(priority)
        }
    }

    fn set_power_throttling_state(
        &self,
        state: PROCESS_POWER_THROTTLING_STATE,
    ) -> Result<(), EcoQosError> {
        let ok = unsafe {
            SetProcessInformation(
                self.0,
                ProcessPowerThrottling,
                &state as *const _ as *const c_void,
                std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
            )
        };
        if ok == 0 {
            Err(process_power_throttling_error(
                "SetProcessInformation",
                last_error(),
            ))
        } else {
            Ok(())
        }
    }

    fn set_priority_class(&self, priority_class: u32) -> Result<(), EcoQosError> {
        let ok = unsafe { SetPriorityClass(self.0, priority_class) };
        if ok == 0 {
            Err(EcoQosError::Failed(format!(
                "SetPriorityClass failed with error {}.",
                last_error()
            )))
        } else {
            Ok(())
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

fn process_power_throttling_error(operation: &str, error: u32) -> EcoQosError {
    match error {
        ERROR_INVALID_PARAMETER | ERROR_NOT_SUPPORTED => EcoQosError::Unsupported,
        _ => EcoQosError::Failed(format!("{operation} failed with error {error}.")),
    }
}

fn open_process_error(process_id: u32, error: u32) -> EcoQosError {
    match error {
        ERROR_ACCESS_DENIED => EcoQosError::AccessDenied,
        ERROR_INVALID_PARAMETER => EcoQosError::ProcessExited,
        _ => EcoQosError::Failed(format!(
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
    fn exclusions_include_builtin_and_user_entries() {
        let settings = EcoQosSettings {
            enabled: true,
            exclude_foreground_app: true,
            efficiency_whitelist: vec!["mouse.exe".to_owned()],
        };

        assert!(is_process_excluded("EXPLORER.EXE", &settings));
        assert!(is_process_excluded("csrss.exe", &settings));
        assert!(is_process_excluded("winlogon.exe", &settings));
        assert!(is_process_excluded("Mouse.exe", &settings));
        assert!(!is_process_excluded("browser.exe", &settings));
    }

    #[test]
    fn power_throttling_unsupported_codes_mark_feature_unsupported() {
        assert!(matches!(
            process_power_throttling_error("SetProcessInformation", ERROR_NOT_SUPPORTED),
            EcoQosError::Unsupported
        ));
        assert!(matches!(
            process_power_throttling_error("SetProcessInformation", ERROR_INVALID_PARAMETER),
            EcoQosError::Unsupported
        ));
    }

    #[test]
    fn process_failure_message_includes_action_name_pid_and_error() {
        assert_eq!(
            process_failure_message("Restore", 42, "browser.exe", "OpenProcess failed."),
            "Restore browser.exe (42): OpenProcess failed."
        );
    }

    #[test]
    fn open_process_invalid_parameter_means_process_exited() {
        assert!(matches!(
            open_process_error(42, ERROR_INVALID_PARAMETER),
            EcoQosError::ProcessExited
        ));
    }

    #[test]
    fn disabled_state_clears_execution_speed_control() {
        let state = power_throttling_disabled_state();

        assert_eq!(state.Version, PROCESS_POWER_THROTTLING_CURRENT_VERSION);
        assert_eq!(state.ControlMask, PROCESS_POWER_THROTTLING_EXECUTION_SPEED);
        assert_eq!(state.StateMask, 0);
    }

    #[test]
    fn foreground_ignore_matches_pid_or_process_name() {
        let mut settings = EcoQosSettings::default();
        settings.exclude_foreground_app = true;

        assert!(should_ignore_foreground_process(
            &settings,
            42,
            "helper.exe",
            Some(42),
            Some("app.exe"),
        ));
        assert!(should_ignore_foreground_process(
            &settings,
            99,
            "APP.EXE",
            Some(42),
            Some("app.exe"),
        ));
        assert!(!should_ignore_foreground_process(
            &settings,
            99,
            "other.exe",
            Some(42),
            Some("app.exe"),
        ));

        settings.exclude_foreground_app = false;
        assert!(!should_ignore_foreground_process(
            &settings,
            42,
            "app.exe",
            Some(42),
            Some("app.exe"),
        ));
    }

    #[test]
    fn release_processes_drops_exited_process_without_log_entry() {
        let mut manager = EcoQosManager::default();
        manager.throttled.insert(
            0,
            ThrottledProcess {
                process_name: "exited.exe".to_owned(),
                previous_state: None,
                previous_priority: None,
            },
        );
        let mut log = ActionLog::new(8);

        let failures = manager.release_processes(&[0], &mut log, "test");

        assert_eq!(failures.count, 0);
        assert!(log.entries().is_empty());
        assert!(manager.throttled.is_empty());
    }
}
