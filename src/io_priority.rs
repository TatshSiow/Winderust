use std::collections::{BTreeMap, BTreeSet};

use windows_sys::Win32::{
    Foundation::{ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER, HANDLE},
    System::Threading::{
        GetCurrentProcessId, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
        PROCESS_SET_INFORMATION,
    },
};

use crate::win_util::{last_error, WinHandle};

use crate::{
    action_log::{ActionLog, ActionLogAction, ActionLogFeature, ActionLogResult},
    config::{IoPrioritySettings, ProcessIoPriority, ProcessIoPrioritySetting},
    foreground::{
        contains_process_name, is_foreground_process, is_process_exited_message, list_processes,
        process_count_label, process_failure_key, process_names_by_id, process_session_id,
        same_process_name, unique_app_names, CORE_BUILT_IN_PROCESS_EXCLUSIONS,
    },
    rules::{execution_failure_suppression_threshold, ExecutionFailureTracker},
};

const PROCESS_IO_PRIORITY: u32 = 33;
const STATUS_PROCESS_IS_TERMINATING: u32 = 0xC000010A;

const BUILT_IN_EXCLUSIONS: &[&str] = CORE_BUILT_IN_PROCESS_EXCLUSIONS;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IoPrioritySnapshot {
    pub enabled: bool,
    pub scanned_processes: usize,
    pub adjusted_processes: usize,
    pub skipped_processes: usize,
    pub failed_processes: usize,
    pub adjusted_apps: Vec<String>,
    pub auto_excluded_processes: Vec<String>,
    pub message: String,
    pub last_error: Option<String>,
}

#[derive(Default)]
pub struct IoPriorityManager {
    adjusted: BTreeMap<u32, AdjustedProcess>,
    failure_suppression: ExecutionFailureTracker,
}

#[derive(Clone)]
struct AdjustedProcess {
    process_name: String,
    creation_time: u64,
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
            self.failure_suppression.clear();
            return IoPrioritySnapshot {
                enabled: false,
                failed_processes: failures.count,
                message: "Automation disabled.".to_owned(),
                last_error: failures.last_error,
                ..Default::default()
            };
        }

        if !settings.enabled {
            let failures = self.clear_all(action_log, "I/O priority defaults disabled");
            self.failure_suppression.clear();
            return IoPrioritySnapshot {
                enabled: false,
                failed_processes: failures.count,
                message: "I/O priority defaults disabled.".to_owned(),
                last_error: failures.last_error,
                ..Default::default()
            };
        }

        let foreground_sensitive = settings.foreground_detection_enabled
            && settings.foreground_priority != settings.background_priority;
        if foreground_sensitive && foreground_process_id.is_none() {
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
        let current_process_names = process_names_by_id(&processes);
        let foreground_process_name = if foreground_sensitive {
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
                || is_builtin_excluded(&process.name)
            {
                continue;
            }

            let foreground = settings.foreground_detection_enabled
                && is_foreground_process(
                    process.id,
                    &process.name,
                    foreground_process_id,
                    foreground_process_name.as_deref(),
                );
            let priority = match settings.override_for(&process.name, foreground) {
                Some(Some(ProcessIoPrioritySetting::Auto)) if foreground => {
                    settings.foreground_priority
                }
                Some(Some(ProcessIoPrioritySetting::Auto)) => settings.background_priority,
                Some(Some(priority)) => priority,
                Some(None) => continue,
                None if foreground => settings.foreground_priority,
                None => settings.background_priority,
            };
            if let Some(priority) = priority.priority() {
                target_processes.insert(process.id, (process.name, priority, foreground));
            }
        }

        let target_ids = target_processes.keys().copied().collect::<BTreeSet<_>>();
        let active_target_names = target_processes
            .values()
            .map(|(name, _priority, _foreground)| process_failure_key(name))
            .collect::<BTreeSet<_>>();
        self.failure_suppression.retain_keys(&active_target_names);

        let mut failures = self.release_non_targets(
            &target_ids,
            &current_process_names,
            action_log,
            "process is excluded or no longer matches I/O priority defaults",
        );
        let mut skipped_processes = 0;
        let mut applied_processes = 0;
        let mut auto_excluded_processes = BTreeSet::new();

        for (process_id, (process_name, priority, foreground)) in target_processes {
            if self.is_process_suppressed(
                process_id,
                &process_name,
                action_log,
                &mut auto_excluded_processes,
            ) {
                skipped_processes += 1;
                continue;
            }

            match self.apply_process(
                process_id,
                process_name.clone(),
                priority,
                foreground,
                settings.preserve_foreground_priority,
                settings.preserve_background_priority,
            ) {
                Ok(ApplyOutcome::Applied { loggable }) => {
                    if loggable {
                        applied_processes += 1;
                    }
                    self.clear_process_failure(&process_name);
                }
                Ok(ApplyOutcome::AlreadyApplied) => {
                    self.clear_process_failure(&process_name);
                }
                Ok(ApplyOutcome::Preserved) => {
                    skipped_processes += 1;
                    self.clear_process_failure(&process_name);
                }
                Err(IoPriorityError::ProcessExited) => {
                    skipped_processes += 1;
                }
                Err(IoPriorityError::AccessDenied) => {
                    skipped_processes += 1;
                    self.record_process_failure(&process_name);
                    action_log.record(
                        ActionLogFeature::IoPriority,
                        Some(process_id),
                        process_name,
                        ActionLogAction::Skip,
                        ActionLogResult::Skipped,
                        "Skipped because the process could not be opened.",
                    );
                }
                Err(err) => {
                    self.record_process_failure(&process_name);
                    failures.record("Apply", process_id, &process_name, err, action_log);
                }
            }
        }
        if applied_processes > 0 {
            action_log.record(
                ActionLogFeature::IoPriority,
                None,
                "I/O Priority",
                ActionLogAction::Apply,
                ActionLogResult::Applied,
                io_priority_apply_summary_message(applied_processes),
            );
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
            auto_excluded_processes: auto_excluded_processes.into_iter().collect(),
            message: "I/O priority defaults active.".to_owned(),
            last_error: failures.last_error,
        }
    }

    fn apply_process(
        &mut self,
        process_id: u32,
        process_name: String,
        priority: ProcessIoPriority,
        foreground: bool,
        preserve_foreground: bool,
        preserve_background: bool,
    ) -> Result<ApplyOutcome, IoPriorityError> {
        let process = ProcessHandle::open(process_id)?;
        let creation_time = process
            .0
            .process_creation_time()
            .ok_or(IoPriorityError::ProcessExited)?;
        let reusable_existing = self.adjusted.get(&process_id).filter(|adjusted| {
            adjusted.creation_time == creation_time
                && same_process_name(&adjusted.process_name, &process_name)
        });
        let current_priority = process.io_priority()?;

        let baseline_priority = reusable_existing
            .map(|adjusted| adjusted.previous_priority)
            .unwrap_or(current_priority);
        if should_preserve_priority(
            foreground,
            preserve_foreground,
            preserve_background,
            io_priority_raw(baseline_priority),
            io_priority_raw(priority),
        ) {
            if let Some(adjusted) = reusable_existing.cloned() {
                process.set_io_priority(adjusted.previous_priority)?;
                self.adjusted.remove(&process_id);
            }
            return Ok(ApplyOutcome::Preserved);
        }

        if reusable_existing.is_some_and(|adjusted| {
            adjusted.applied_priority == priority && current_priority == priority
        }) {
            return Ok(ApplyOutcome::AlreadyApplied);
        }

        if current_priority != priority {
            process.set_io_priority(priority)?;
            let refreshed_priority = process.io_priority()?;
            if refreshed_priority != priority {
                return Err(IoPriorityError::Failed(format!(
                    "I/O priority remained {} after requesting {}.",
                    io_priority_label(refreshed_priority),
                    io_priority_label(priority)
                )));
            }
        }

        self.adjusted.insert(
            process_id,
            AdjustedProcess {
                process_name,
                creation_time,
                previous_priority: baseline_priority,
                applied_priority: priority,
            },
        );
        Ok(ApplyOutcome::Applied {
            loggable: current_priority != priority,
        })
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
        let mut restored_processes = 0;
        for process_id in process_ids {
            let Some(process_state) = self.adjusted.get(process_id).cloned() else {
                continue;
            };
            let log_name = current_process_names
                .and_then(|names| names.get(process_id))
                .cloned()
                .unwrap_or_else(|| process_state.process_name.clone());
            match restore_process(*process_id, &process_state) {
                Ok(()) => {
                    self.adjusted.remove(process_id);
                    self.clear_process_failure(&log_name);
                    restored_processes += 1;
                }
                Err(IoPriorityError::ProcessExited) => {
                    self.adjusted.remove(process_id);
                }
                Err(err) => {
                    self.record_process_failure(&log_name);
                    failures.record("Restore", *process_id, &log_name, err, action_log);
                }
            }
        }
        if restored_processes > 0 {
            action_log.record(
                ActionLogFeature::IoPriority,
                None,
                "I/O Priority",
                ActionLogAction::Restore,
                ActionLogResult::Restored,
                io_priority_restore_summary_message(restored_processes, reason),
            );
        }
        failures
    }

    fn is_process_suppressed(
        &mut self,
        process_id: u32,
        process_name: &str,
        action_log: &mut ActionLog,
        auto_excluded_processes: &mut BTreeSet<String>,
    ) -> bool {
        let suppression = self.failure_suppression.process_suppression(process_name);
        if !suppression.suppressed {
            return false;
        }

        if suppression.newly_suppressed {
            auto_excluded_processes.insert(process_failure_key(process_name));
            action_log.record(
                ActionLogFeature::IoPriority,
                Some(process_id),
                process_name.to_owned(),
                ActionLogAction::Skip,
                ActionLogResult::Skipped,
                format!(
                    "Stopped retrying I/O Priority after {} failed attempts.",
                    execution_failure_suppression_threshold(),
                ),
            );
        }

        true
    }

    fn record_process_failure(&mut self, process_name: &str) {
        self.failure_suppression
            .record_process_failure(process_name);
    }

    fn clear_process_failure(&mut self, process_name: &str) {
        self.failure_suppression.clear_process_failure(process_name);
    }
}

impl Drop for IoPriorityManager {
    fn drop(&mut self) {
        let mut action_log = ActionLog::new(1);
        self.clear_all(&mut action_log, stringify!(IoPriorityManager));
    }
}

enum ApplyOutcome {
    Applied { loggable: bool },
    AlreadyApplied,
    Preserved,
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

struct ProcessHandle(WinHandle);

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
            Ok(Self(WinHandle::new(handle)))
        } else {
            Err(open_process_error(process_id, last_error()))
        }
    }

    fn io_priority(&self) -> Result<ProcessIoPriority, IoPriorityError> {
        let mut priority = 0_u32;
        let status = unsafe {
            NtQueryInformationProcess(
                self.0.raw(),
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
                self.0.raw(),
                PROCESS_IO_PRIORITY,
                &mut raw as *mut _ as *mut _,
                std::mem::size_of::<u32>() as u32,
            )
        };
        ntstatus_result(status)
    }
}

fn restore_process(
    process_id: u32,
    process_state: &AdjustedProcess,
) -> Result<(), IoPriorityError> {
    let process = ProcessHandle::open(process_id)?;
    if process.0.process_creation_time() != Some(process_state.creation_time) {
        return Err(IoPriorityError::ProcessExited);
    }
    process.set_io_priority(process_state.previous_priority)?;
    let refreshed_priority = process.io_priority()?;
    if refreshed_priority == process_state.previous_priority {
        Ok(())
    } else {
        Err(IoPriorityError::Failed(format!(
            "I/O priority remained {} after restoring {}.",
            io_priority_label(refreshed_priority),
            io_priority_label(process_state.previous_priority)
        )))
    }
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
        ProcessIoPriority::Critical => 4,
        ProcessIoPriority::High => 3,
        ProcessIoPriority::Normal => 2,
        ProcessIoPriority::VeryLow => 0,
        ProcessIoPriority::Low => 1,
    }
}

fn io_priority_from_raw(priority: u32) -> ProcessIoPriority {
    match priority {
        0 => ProcessIoPriority::VeryLow,
        1 => ProcessIoPriority::Low,
        3 => ProcessIoPriority::High,
        4 => ProcessIoPriority::Critical,
        _ => ProcessIoPriority::Normal,
    }
}

fn should_preserve_priority(
    foreground: bool,
    preserve_foreground: bool,
    preserve_background: bool,
    current_rank: u32,
    desired_rank: u32,
) -> bool {
    if foreground {
        preserve_foreground && current_rank >= desired_rank
    } else {
        preserve_background && current_rank <= desired_rank
    }
}

pub fn io_priority_label(priority: ProcessIoPriority) -> &'static str {
    match priority {
        ProcessIoPriority::Critical => "Critical",
        ProcessIoPriority::High => "High",
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

fn io_priority_restore_summary_message(count: usize, reason: &str) -> String {
    format!(
        "Restored previous I/O priority for {}: {reason}.",
        process_count_label(count)
    )
}

fn io_priority_apply_summary_message(count: usize) -> String {
    format!("Applied I/O priority to {}.", process_count_label(count))
}

pub fn is_builtin_excluded(process_name: &str) -> bool {
    contains_process_name(BUILT_IN_EXCLUSIONS, process_name)
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
    fn repeated_process_failures_suppress_io_priority_retries() {
        let mut manager = IoPriorityManager::default();
        let mut log = ActionLog::new(8);

        manager.record_process_failure("APP.exe");
        manager.record_process_failure("app.exe");
        assert!(!manager.is_process_suppressed(42, "app.exe", &mut log, &mut BTreeSet::new()));

        manager.record_process_failure("app.exe");
        assert!(manager.is_process_suppressed(42, "app.exe", &mut log, &mut BTreeSet::new()));
        assert!(manager.is_process_suppressed(43, "APP.exe", &mut log, &mut BTreeSet::new()));

        let entries = log.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].feature, ActionLogFeature::IoPriority);
        assert_eq!(entries[0].action, ActionLogAction::Skip);
        assert_eq!(entries[0].result, ActionLogResult::Skipped);
        assert!(entries[0].reason.contains("Stopped retrying I/O Priority"));
    }

    #[test]
    fn successful_process_clears_io_priority_failure_suppression() {
        let mut manager = IoPriorityManager::default();
        let mut log = ActionLog::new(8);

        manager.record_process_failure("app.exe");
        manager.record_process_failure("app.exe");
        manager.record_process_failure("app.exe");
        assert!(manager.is_process_suppressed(42, "app.exe", &mut log, &mut BTreeSet::new()));

        manager.clear_process_failure("APP.exe");
        assert!(!manager.is_process_suppressed(42, "app.exe", &mut log, &mut BTreeSet::new()));
    }

    #[test]
    fn io_priority_restore_summary_message_uses_process_count() {
        assert_eq!(
            io_priority_restore_summary_message(1, "foreground app is unknown"),
            "Restored previous I/O priority for 1 process: foreground app is unknown."
        );
        assert_eq!(
            io_priority_restore_summary_message(68, "foreground app is unknown"),
            "Restored previous I/O priority for 68 processes: foreground app is unknown."
        );
    }

    #[test]
    fn io_priority_apply_summary_message_uses_process_count() {
        assert_eq!(
            io_priority_apply_summary_message(1),
            "Applied I/O priority to 1 process."
        );
        assert_eq!(
            io_priority_apply_summary_message(68),
            "Applied I/O priority to 68 processes."
        );
    }

    #[test]
    fn io_priority_raw_values_match_priority_hint_order() {
        assert_eq!(io_priority_raw(ProcessIoPriority::VeryLow), 0);
        assert_eq!(io_priority_raw(ProcessIoPriority::Low), 1);
        assert_eq!(io_priority_raw(ProcessIoPriority::Normal), 2);
        assert_eq!(io_priority_raw(ProcessIoPriority::High), 3);
        assert_eq!(io_priority_raw(ProcessIoPriority::Critical), 4);
    }

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
