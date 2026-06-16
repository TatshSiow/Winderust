use std::collections::{BTreeMap, BTreeSet};

use windows_sys::Win32::{
    Foundation::{CloseHandle, GetLastError, ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER, HANDLE},
    System::{
        RemoteDesktop::ProcessIdToSessionId,
        Threading::{
            GetCurrentProcessId, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
            PROCESS_SET_INFORMATION,
        },
    },
};

use crate::{
    action_log::{ActionLog, ActionLogAction, ActionLogFeature, ActionLogResult},
    config::{IoPrioritySettings, ProcessIoPriority},
    foreground::list_processes,
};

const PROCESS_IO_PRIORITY: u32 = 33;
const STATUS_PROCESS_IS_TERMINATING: u32 = 0xC000010A;

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
    "services.exe",
    "sihost.exe",
    "smss.exe",
    "system",
    "taskmgr.exe",
    "wininit.exe",
    "winlogon.exe",
];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IoPrioritySnapshot {
    pub enabled: bool,
    pub scanned_processes: usize,
    pub adjusted_processes: usize,
    pub skipped_processes: usize,
    pub failed_processes: usize,
    pub adjusted_apps: Vec<String>,
    pub message: String,
    pub last_error: Option<String>,
}

#[derive(Default)]
pub struct IoPriorityManager {
    adjusted: BTreeMap<u32, AdjustedProcess>,
}

#[derive(Clone)]
struct AdjustedProcess {
    process_name: String,
    previous_priority: ProcessIoPriority,
    applied_priority: ProcessIoPriority,
}

#[derive(Debug)]
enum IoPriorityError {
    AccessDenied,
    ProcessExited,
    Failed(String),
}

impl IoPriorityManager {
    pub fn update(
        &mut self,
        settings: &IoPrioritySettings,
        automation_enabled: bool,
        foreground_process_id: Option<u32>,
        action_log: &mut ActionLog,
    ) -> IoPrioritySnapshot {
        if !automation_enabled {
            let failures = self.clear_all(action_log, "automation disabled");
            return IoPrioritySnapshot {
                enabled: false,
                failed_processes: failures.count,
                message: "Automation disabled.".to_owned(),
                last_error: failures.last_error,
                ..Default::default()
            };
        }

        if !settings.enabled {
            let failures = self.clear_all(action_log, "I/O priority rules disabled");
            return IoPrioritySnapshot {
                enabled: false,
                failed_processes: failures.count,
                message: "I/O priority rules disabled.".to_owned(),
                last_error: failures.last_error,
                ..Default::default()
            };
        }

        if settings.exclude_foreground_app && foreground_process_id.is_none() {
            let failures = self.clear_all(action_log, "foreground app is unknown");
            return IoPrioritySnapshot {
                enabled: true,
                failed_processes: failures.count,
                message: "Paused: foreground app is unknown.".to_owned(),
                last_error: failures.last_error,
                ..Default::default()
            };
        }

        let current_process_id = unsafe { GetCurrentProcessId() };
        let Some(current_session_id) = process_session_id(current_process_id) else {
            let failures = self.clear_all(action_log, "current Windows session is unknown");
            return IoPrioritySnapshot {
                enabled: true,
                failed_processes: failures.count,
                message: "Paused: current Windows session is unknown.".to_owned(),
                last_error: failures.last_error,
                ..Default::default()
            };
        };

        let processes = match list_processes() {
            Ok(processes) => processes,
            Err(err) => {
                let failures = self.clear_all(action_log, "process list unavailable");
                return IoPrioritySnapshot {
                    enabled: true,
                    failed_processes: failures.count,
                    message: err,
                    last_error: failures.last_error,
                    ..Default::default()
                };
            }
        };

        let scanned_processes = processes.len();
        let current_process_names = processes
            .iter()
            .map(|process| (process.id, process.name.clone()))
            .collect::<BTreeMap<_, _>>();
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
                || process_session_id(process.id) != Some(current_session_id)
                || should_ignore_foreground_process(
                    settings,
                    process.id,
                    &process.name,
                    foreground_process_id,
                    foreground_process_name.as_deref(),
                )
                || is_builtin_excluded(&process.name)
            {
                continue;
            }

            if let Some(priority) = settings.priority_enabled_for(&process.name) {
                target_processes.insert(process.id, (process.name, priority));
            }
        }

        let target_ids = target_processes.keys().copied().collect::<BTreeSet<_>>();
        let mut failures = self.release_non_targets(
            &target_ids,
            &current_process_names,
            action_log,
            "process no longer matches an I/O priority rule",
        );
        let mut skipped_processes = 0;

        for (process_id, (process_name, priority)) in target_processes {
            match self.apply_process(process_id, process_name.clone(), priority, action_log) {
                Ok(ApplyOutcome::Applied) | Ok(ApplyOutcome::AlreadyApplied) => {}
                Err(IoPriorityError::ProcessExited) => {
                    skipped_processes += 1;
                }
                Err(IoPriorityError::AccessDenied) => {
                    skipped_processes += 1;
                    action_log.record(
                        ActionLogFeature::IoPriority,
                        Some(process_id),
                        process_name,
                        ActionLogAction::Skip,
                        ActionLogResult::Skipped,
                        "Skipped because the process could not be opened.",
                    );
                }
                Err(err) => failures.record("Apply", process_id, &process_name, err, action_log),
            }
        }

        IoPrioritySnapshot {
            enabled: true,
            scanned_processes,
            adjusted_processes: self.adjusted.len(),
            skipped_processes,
            failed_processes: failures.count,
            adjusted_apps: unique_app_names(
                self.adjusted
                    .values()
                    .map(|process| process.process_name.as_str()),
            ),
            message: "I/O priority rules active.".to_owned(),
            last_error: failures.last_error,
        }
    }

    fn apply_process(
        &mut self,
        process_id: u32,
        process_name: String,
        priority: ProcessIoPriority,
        action_log: &mut ActionLog,
    ) -> Result<ApplyOutcome, IoPriorityError> {
        let process = ProcessHandle::open(process_id)?;
        let reusable_existing = self
            .adjusted
            .get(&process_id)
            .filter(|adjusted| adjusted.process_name.eq_ignore_ascii_case(&process_name));
        let current_priority = process.io_priority()?;

        if reusable_existing.is_some_and(|adjusted| {
            adjusted.applied_priority == priority && current_priority == priority
        }) {
            return Ok(ApplyOutcome::AlreadyApplied);
        }

        if current_priority != priority {
            process.set_io_priority(priority)?;
            action_log.record(
                ActionLogFeature::IoPriority,
                Some(process_id),
                process_name.clone(),
                ActionLogAction::Apply,
                ActionLogResult::Applied,
                format!("Set I/O priority to {}.", io_priority_label(priority)),
            );
        }

        let previous_priority = reusable_existing
            .map(|adjusted| adjusted.previous_priority)
            .unwrap_or(current_priority);
        self.adjusted.insert(
            process_id,
            AdjustedProcess {
                process_name,
                previous_priority,
                applied_priority: priority,
            },
        );
        Ok(ApplyOutcome::Applied)
    }

    fn release_non_targets(
        &mut self,
        target_ids: &BTreeSet<u32>,
        current_process_names: &BTreeMap<u32, String>,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> IoPriorityFailures {
        let process_ids = self
            .adjusted
            .keys()
            .copied()
            .filter(|process_id| !target_ids.contains(process_id))
            .collect::<Vec<_>>();
        self.release_processes(
            &process_ids,
            Some(current_process_names),
            action_log,
            reason,
        )
    }

    fn clear_all(&mut self, action_log: &mut ActionLog, reason: &str) -> IoPriorityFailures {
        let process_ids = self.adjusted.keys().copied().collect::<Vec<_>>();
        self.release_processes(&process_ids, None, action_log, reason)
    }

    fn release_processes(
        &mut self,
        process_ids: &[u32],
        current_process_names: Option<&BTreeMap<u32, String>>,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> IoPriorityFailures {
        let mut failures = IoPriorityFailures::default();
        for process_id in process_ids {
            let Some(process_state) = self.adjusted.remove(process_id) else {
                continue;
            };
            let log_name = current_process_names
                .and_then(|names| names.get(process_id))
                .cloned()
                .unwrap_or_else(|| process_state.process_name.clone());
            match restore_process(*process_id, process_state) {
                Ok(()) => action_log.record(
                    ActionLogFeature::IoPriority,
                    Some(*process_id),
                    log_name,
                    ActionLogAction::Restore,
                    ActionLogResult::Restored,
                    format!("Restored previous I/O priority: {reason}."),
                ),
                Err(IoPriorityError::ProcessExited) => {}
                Err(err) => failures.record("Restore", *process_id, &log_name, err, action_log),
            }
        }
        failures
    }
}

enum ApplyOutcome {
    Applied,
    AlreadyApplied,
}

#[derive(Default)]
struct IoPriorityFailures {
    count: usize,
    last_error: Option<String>,
}

impl IoPriorityFailures {
    fn record(
        &mut self,
        action: &str,
        process_id: u32,
        process_name: &str,
        error: IoPriorityError,
        action_log: &mut ActionLog,
    ) {
        let message = io_priority_error_message(error);
        if is_process_exited_message(&message) {
            return;
        }
        if self.last_error.is_none() {
            self.last_error = Some(format!("{action} {process_name} ({process_id}): {message}"));
        }
        self.count += 1;
        action_log.record(
            ActionLogFeature::IoPriority,
            Some(process_id),
            process_name.to_owned(),
            ActionLogAction::Fail,
            ActionLogResult::Failed,
            message,
        );
    }
}

struct ProcessHandle(HANDLE);

impl ProcessHandle {
    fn open(process_id: u32) -> Result<Self, IoPriorityError> {
        let handle = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_INFORMATION,
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

    fn io_priority(&self) -> Result<ProcessIoPriority, IoPriorityError> {
        let mut priority = 0_u32;
        let status = unsafe {
            NtQueryInformationProcess(
                self.0,
                PROCESS_IO_PRIORITY,
                &mut priority as *mut _ as *mut _,
                std::mem::size_of::<u32>() as u32,
                std::ptr::null_mut(),
            )
        };
        ntstatus_result(status).map(|()| io_priority_from_raw(priority))
    }

    fn set_io_priority(&self, priority: ProcessIoPriority) -> Result<(), IoPriorityError> {
        let mut raw = io_priority_raw(priority);
        let status = unsafe {
            NtSetInformationProcess(
                self.0,
                PROCESS_IO_PRIORITY,
                &mut raw as *mut _ as *mut _,
                std::mem::size_of::<u32>() as u32,
            )
        };
        ntstatus_result(status)
    }
}

impl Drop for ProcessHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

fn restore_process(process_id: u32, process_state: AdjustedProcess) -> Result<(), IoPriorityError> {
    let process = ProcessHandle::open(process_id)?;
    process.set_io_priority(process_state.previous_priority)
}

fn ntstatus_result(status: i32) -> Result<(), IoPriorityError> {
    if status >= 0 {
        Ok(())
    } else if status as u32 == STATUS_PROCESS_IS_TERMINATING {
        Err(IoPriorityError::ProcessExited)
    } else {
        Err(IoPriorityError::Failed(format!(
            "NTSTATUS 0x{:08X}.",
            status as u32
        )))
    }
}

fn open_process_error(process_id: u32, error: u32) -> IoPriorityError {
    match error {
        ERROR_ACCESS_DENIED => IoPriorityError::AccessDenied,
        ERROR_INVALID_PARAMETER => IoPriorityError::ProcessExited,
        _ => IoPriorityError::Failed(format!(
            "OpenProcess({process_id}) failed with error {error}."
        )),
    }
}

fn io_priority_raw(priority: ProcessIoPriority) -> u32 {
    match priority {
        ProcessIoPriority::VeryLow => 0,
        ProcessIoPriority::Low => 1,
        ProcessIoPriority::Normal => 2,
    }
}

fn io_priority_from_raw(priority: u32) -> ProcessIoPriority {
    match priority {
        0 => ProcessIoPriority::VeryLow,
        1 => ProcessIoPriority::Low,
        _ => ProcessIoPriority::Normal,
    }
}

pub fn io_priority_label(priority: ProcessIoPriority) -> &'static str {
    match priority {
        ProcessIoPriority::Normal => "Normal",
        ProcessIoPriority::Low => "Low",
        ProcessIoPriority::VeryLow => "Very Low",
    }
}

fn io_priority_error_message(error: IoPriorityError) -> String {
    match error {
        IoPriorityError::AccessDenied => "Access denied.".to_owned(),
        IoPriorityError::ProcessExited => "Process exited.".to_owned(),
        IoPriorityError::Failed(message) => message,
    }
}

fn is_process_exited_message(message: &str) -> bool {
    message
        .trim()
        .trim_end_matches('.')
        .eq_ignore_ascii_case("Process exited")
}

fn process_session_id(process_id: u32) -> Option<u32> {
    let mut session_id = 0;
    let ok = unsafe { ProcessIdToSessionId(process_id, &mut session_id) };
    (ok != 0).then_some(session_id)
}

fn should_ignore_foreground_process(
    settings: &IoPrioritySettings,
    process_id: u32,
    process_name: &str,
    foreground_process_id: Option<u32>,
    foreground_process_name: Option<&str>,
) -> bool {
    settings.exclude_foreground_app
        && (Some(process_id) == foreground_process_id
            || foreground_process_name.is_some_and(|foreground| {
                foreground.trim().eq_ignore_ascii_case(process_name.trim())
            }))
}

pub fn is_builtin_excluded(process_name: &str) -> bool {
    BUILT_IN_EXCLUSIONS
        .iter()
        .any(|excluded| excluded.eq_ignore_ascii_case(process_name.trim()))
}

fn unique_app_names<'a>(names: impl Iterator<Item = &'a str>) -> Vec<String> {
    names
        .map(|name| name.trim().to_ascii_lowercase())
        .filter(|name| !name.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn last_error() -> u32 {
    unsafe { GetLastError() }
}

unsafe extern "system" {
    fn NtQueryInformationProcess(
        ProcessHandle: HANDLE,
        ProcessInformationClass: u32,
        ProcessInformation: *mut std::ffi::c_void,
        ProcessInformationLength: u32,
        ReturnLength: *mut u32,
    ) -> i32;

    fn NtSetInformationProcess(
        ProcessHandle: HANDLE,
        ProcessInformationClass: u32,
        ProcessInformation: *mut std::ffi::c_void,
        ProcessInformationLength: u32,
    ) -> i32;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminating_process_ntstatus_is_treated_as_process_exited() {
        assert!(matches!(
            ntstatus_result(STATUS_PROCESS_IS_TERMINATING as i32),
            Err(IoPriorityError::ProcessExited)
        ));
    }

    #[test]
    fn unrelated_ntstatus_remains_failure() {
        assert!(matches!(
            ntstatus_result(0xC0000001_u32 as i32),
            Err(IoPriorityError::Failed(message)) if message == "NTSTATUS 0xC0000001."
        ));
    }
}
