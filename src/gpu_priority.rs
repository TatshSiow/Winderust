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
    config::{GpuPrioritySettings, ProcessGpuPriority},
    foreground::list_processes,
    rules::{execution_failure_suppression_threshold, ExecutionFailureState},
};

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
pub struct GpuPrioritySnapshot {
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
pub struct GpuPriorityManager {
    adjusted: BTreeMap<u32, AdjustedProcess>,
    failure_suppression: BTreeMap<String, GpuPriorityFailureSuppression>,
}

type GpuPriorityFailureSuppression = ExecutionFailureState;

#[derive(Clone)]
struct AdjustedProcess {
    process_name: String,
    previous_priority_raw: u32,
    applied_priority: ProcessGpuPriority,
}

#[derive(Debug)]
enum GpuPriorityError {
    AccessDenied,
    ProcessExited,
    Failed(String),
}

impl GpuPriorityManager {
    pub fn update(
        &mut self,
        settings: &GpuPrioritySettings,
        automation_enabled: bool,
        foreground_process_id: Option<u32>,
        action_log: &mut ActionLog,
    ) -> GpuPrioritySnapshot {
        if !automation_enabled {
            let failures = self.clear_all(action_log, "automation disabled");
            self.failure_suppression.clear();
            return GpuPrioritySnapshot {
                enabled: false,
                failed_processes: failures.count,
                message: "Automation disabled.".to_owned(),
                last_error: failures.last_error,
                ..Default::default()
            };
        }

        if !settings.enabled {
            let failures = self.clear_all(action_log, "GPU priority defaults disabled");
            self.failure_suppression.clear();
            return GpuPrioritySnapshot {
                enabled: false,
                failed_processes: failures.count,
                message: "GPU priority defaults disabled.".to_owned(),
                last_error: failures.last_error,
                ..Default::default()
            };
        }

        let foreground_sensitive = settings.foreground_detection_enabled
            && settings.foreground_priority != settings.background_priority;
        if foreground_sensitive && foreground_process_id.is_none() {
            let failures = self.clear_all(action_log, "foreground app is unknown");
            return GpuPrioritySnapshot {
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
            return GpuPrioritySnapshot {
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
                return GpuPrioritySnapshot {
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
                || settings.exclusion_enabled_for(&process.name)
                || is_builtin_excluded(&process.name)
            {
                continue;
            }

            let priority = if settings.foreground_detection_enabled
                && is_foreground_process(
                    process.id,
                    &process.name,
                    foreground_process_id,
                    foreground_process_name.as_deref(),
                ) {
                settings.foreground_priority
            } else {
                settings.background_priority
            };
            if let Some(priority) = priority.priority() {
                target_processes.insert(process.id, (process.name, priority));
            }
        }

        let target_ids = target_processes.keys().copied().collect::<BTreeSet<_>>();
        let active_target_names = target_processes
            .values()
            .map(|(name, _priority)| process_failure_key(name))
            .collect::<BTreeSet<_>>();
        self.failure_suppression
            .retain(|process_name, _| active_target_names.contains(process_name));

        let mut failures = self.release_non_targets(
            &target_ids,
            &current_process_names,
            action_log,
            "process is excluded or no longer matches GPU priority defaults",
        );
        let mut skipped_processes = 0;

        for (process_id, (process_name, priority)) in target_processes {
            if self.is_process_suppressed(process_id, &process_name, action_log) {
                skipped_processes += 1;
                continue;
            }

            match self.apply_process(process_id, process_name.clone(), priority, action_log) {
                Ok(ApplyOutcome::Applied) | Ok(ApplyOutcome::AlreadyApplied) => {
                    self.clear_process_failure(&process_name);
                }
                Err(GpuPriorityError::ProcessExited) => {
                    skipped_processes += 1;
                }
                Err(GpuPriorityError::AccessDenied) => {
                    skipped_processes += 1;
                    self.record_process_failure(&process_name);
                    action_log.record(
                        ActionLogFeature::GpuPriority,
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

        GpuPrioritySnapshot {
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
            message: "GPU priority defaults active.".to_owned(),
            last_error: failures.last_error,
        }
    }

    fn apply_process(
        &mut self,
        process_id: u32,
        process_name: String,
        priority: ProcessGpuPriority,
        action_log: &mut ActionLog,
    ) -> Result<ApplyOutcome, GpuPriorityError> {
        let process = ProcessHandle::open(process_id)?;
        let reusable_existing = self
            .adjusted
            .get(&process_id)
            .filter(|adjusted| adjusted.process_name.eq_ignore_ascii_case(&process_name));
        let current_priority_raw = process.gpu_priority_raw()?;
        let desired_priority_raw = gpu_priority_raw(priority);

        if reusable_existing.is_some_and(|adjusted| {
            adjusted.applied_priority == priority && current_priority_raw == desired_priority_raw
        }) {
            return Ok(ApplyOutcome::AlreadyApplied);
        }

        if current_priority_raw != desired_priority_raw {
            process.set_gpu_priority_raw(desired_priority_raw)?;
            action_log.record(
                ActionLogFeature::GpuPriority,
                Some(process_id),
                process_name.clone(),
                ActionLogAction::Apply,
                ActionLogResult::Applied,
                format!("Set GPU priority to {}.", gpu_priority_label(priority)),
            );
        }

        let previous_priority_raw = reusable_existing
            .map(|adjusted| adjusted.previous_priority_raw)
            .unwrap_or(current_priority_raw);
        self.adjusted.insert(
            process_id,
            AdjustedProcess {
                process_name,
                previous_priority_raw,
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
    ) -> GpuPriorityFailures {
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

    fn clear_all(&mut self, action_log: &mut ActionLog, reason: &str) -> GpuPriorityFailures {
        let process_ids = self.adjusted.keys().copied().collect::<Vec<_>>();
        self.release_processes(&process_ids, None, action_log, reason)
    }

    fn release_processes(
        &mut self,
        process_ids: &[u32],
        current_process_names: Option<&BTreeMap<u32, String>>,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> GpuPriorityFailures {
        let mut failures = GpuPriorityFailures::default();
        for process_id in process_ids {
            let Some(process_state) = self.adjusted.remove(process_id) else {
                continue;
            };
            let log_name = current_process_names
                .and_then(|names| names.get(process_id))
                .cloned()
                .unwrap_or_else(|| process_state.process_name.clone());
            match restore_process(*process_id, process_state) {
                Ok(()) => {
                    self.clear_process_failure(&log_name);
                    action_log.record(
                        ActionLogFeature::GpuPriority,
                        Some(*process_id),
                        log_name,
                        ActionLogAction::Restore,
                        ActionLogResult::Restored,
                        format!("Restored previous GPU priority: {reason}."),
                    );
                }
                Err(GpuPriorityError::ProcessExited) => {}
                Err(err) => {
                    self.record_process_failure(&log_name);
                    failures.record("Restore", *process_id, &log_name, err, action_log);
                }
            }
        }
        failures
    }

    fn is_process_suppressed(
        &mut self,
        process_id: u32,
        process_name: &str,
        action_log: &mut ActionLog,
    ) -> bool {
        let Some(suppression) = self
            .failure_suppression
            .get_mut(&process_failure_key(process_name))
        else {
            return false;
        };
        if !suppression.is_suppressed() {
            return false;
        }

        if suppression.mark_suppression_logged() {
            action_log.record(
                ActionLogFeature::GpuPriority,
                Some(process_id),
                process_name.to_owned(),
                ActionLogAction::Skip,
                ActionLogResult::Skipped,
                format!(
                    "Stopped retrying GPU Priority after {} failed attempts.",
                    execution_failure_suppression_threshold(),
                ),
            );
        }

        true
    }

    fn record_process_failure(&mut self, process_name: &str) {
        let suppression = self
            .failure_suppression
            .entry(process_failure_key(process_name))
            .or_default();
        suppression.record_failure();
    }

    fn clear_process_failure(&mut self, process_name: &str) {
        self.failure_suppression
            .remove(&process_failure_key(process_name));
    }
}

enum ApplyOutcome {
    Applied,
    AlreadyApplied,
}

#[derive(Default)]
struct GpuPriorityFailures {
    count: usize,
    last_error: Option<String>,
}

impl GpuPriorityFailures {
    fn record(
        &mut self,
        action: &str,
        process_id: u32,
        process_name: &str,
        error: GpuPriorityError,
        action_log: &mut ActionLog,
    ) {
        let message = gpu_priority_error_message(error);
        if is_process_exited_message(&message) {
            return;
        }
        if self.last_error.is_none() {
            self.last_error = Some(format!("{action} {process_name} ({process_id}): {message}"));
        }
        self.count += 1;
        action_log.record(
            ActionLogFeature::GpuPriority,
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
    fn open(process_id: u32) -> Result<Self, GpuPriorityError> {
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

    fn gpu_priority_raw(&self) -> Result<u32, GpuPriorityError> {
        let mut priority = 0_u32;
        let status =
            unsafe { D3DKMTGetProcessSchedulingPriorityClass(self.0, &mut priority as *mut _) };
        ntstatus_result(status).map(|()| priority)
    }

    fn set_gpu_priority_raw(&self, priority: u32) -> Result<(), GpuPriorityError> {
        let status = unsafe { D3DKMTSetProcessSchedulingPriorityClass(self.0, priority) };
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

fn restore_process(
    process_id: u32,
    process_state: AdjustedProcess,
) -> Result<(), GpuPriorityError> {
    let process = ProcessHandle::open(process_id)?;
    process.set_gpu_priority_raw(process_state.previous_priority_raw)
}

fn ntstatus_result(status: i32) -> Result<(), GpuPriorityError> {
    if status >= 0 {
        Ok(())
    } else if status as u32 == STATUS_PROCESS_IS_TERMINATING {
        Err(GpuPriorityError::ProcessExited)
    } else {
        Err(GpuPriorityError::Failed(format!(
            "NTSTATUS 0x{:08X}.",
            status as u32
        )))
    }
}

fn open_process_error(process_id: u32, error: u32) -> GpuPriorityError {
    match error {
        ERROR_ACCESS_DENIED => GpuPriorityError::AccessDenied,
        ERROR_INVALID_PARAMETER => GpuPriorityError::ProcessExited,
        _ => GpuPriorityError::Failed(format!(
            "OpenProcess({process_id}) failed with error {error}."
        )),
    }
}

fn gpu_priority_raw(priority: ProcessGpuPriority) -> u32 {
    match priority {
        ProcessGpuPriority::Idle => 0,
        ProcessGpuPriority::BelowNormal => 1,
        ProcessGpuPriority::Normal => 2,
        ProcessGpuPriority::AboveNormal => 3,
    }
}

pub fn gpu_priority_label(priority: ProcessGpuPriority) -> &'static str {
    match priority {
        ProcessGpuPriority::AboveNormal => "Above Normal",
        ProcessGpuPriority::Normal => "Normal",
        ProcessGpuPriority::BelowNormal => "Below Normal",
        ProcessGpuPriority::Idle => "Idle",
    }
}

fn gpu_priority_error_message(error: GpuPriorityError) -> String {
    match error {
        GpuPriorityError::AccessDenied => "Access denied.".to_owned(),
        GpuPriorityError::ProcessExited => "Process exited.".to_owned(),
        GpuPriorityError::Failed(message) => message,
    }
}

fn is_process_exited_message(message: &str) -> bool {
    message
        .trim()
        .trim_end_matches('.')
        .eq_ignore_ascii_case("Process exited")
}

fn process_failure_key(process_name: &str) -> String {
    process_name.trim().to_ascii_lowercase()
}

fn process_session_id(process_id: u32) -> Option<u32> {
    let mut session_id = 0;
    let ok = unsafe { ProcessIdToSessionId(process_id, &mut session_id) };
    (ok != 0).then_some(session_id)
}

fn is_foreground_process(
    process_id: u32,
    process_name: &str,
    foreground_process_id: Option<u32>,
    foreground_process_name: Option<&str>,
) -> bool {
    Some(process_id) == foreground_process_id
        || foreground_process_name
            .is_some_and(|foreground| foreground.trim().eq_ignore_ascii_case(process_name.trim()))
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

#[link(name = "gdi32")]
unsafe extern "system" {
    fn D3DKMTGetProcessSchedulingPriorityClass(
        Process: HANDLE,
        SchedulingPriorityClass: *mut u32,
    ) -> i32;

    fn D3DKMTSetProcessSchedulingPriorityClass(
        Process: HANDLE,
        SchedulingPriorityClass: u32,
    ) -> i32;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_priority_raw_values_match_wddm_order() {
        assert_eq!(gpu_priority_raw(ProcessGpuPriority::Idle), 0);
        assert_eq!(gpu_priority_raw(ProcessGpuPriority::BelowNormal), 1);
        assert_eq!(gpu_priority_raw(ProcessGpuPriority::Normal), 2);
        assert_eq!(gpu_priority_raw(ProcessGpuPriority::AboveNormal), 3);
    }

    #[test]
    fn repeated_process_failures_suppress_gpu_priority_retries() {
        let mut manager = GpuPriorityManager::default();
        let mut log = ActionLog::new(8);

        manager.record_process_failure("APP.exe");
        manager.record_process_failure("app.exe");
        assert!(!manager.is_process_suppressed(42, "app.exe", &mut log));

        manager.record_process_failure("app.exe");
        assert!(manager.is_process_suppressed(42, "app.exe", &mut log));
        assert!(manager.is_process_suppressed(43, "APP.exe", &mut log));

        let entries = log.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].feature, ActionLogFeature::GpuPriority);
        assert_eq!(entries[0].action, ActionLogAction::Skip);
        assert_eq!(entries[0].result, ActionLogResult::Skipped);
        assert!(entries[0].reason.contains("Stopped retrying GPU Priority"));
    }
}
