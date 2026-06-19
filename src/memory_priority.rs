use std::collections::{BTreeMap, BTreeSet};

use windows_sys::Win32::{
    Foundation::{CloseHandle, GetLastError, ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER, HANDLE},
    System::{
        RemoteDesktop::ProcessIdToSessionId,
        Threading::{
            GetCurrentProcessId, GetProcessInformation, OpenProcess,
            ProcessMemoryPriority as ProcessMemoryPriorityClass, SetProcessInformation,
            MEMORY_PRIORITY_BELOW_NORMAL, MEMORY_PRIORITY_INFORMATION, MEMORY_PRIORITY_LOW,
            MEMORY_PRIORITY_MEDIUM, MEMORY_PRIORITY_NORMAL, MEMORY_PRIORITY_VERY_LOW,
            PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_INFORMATION,
        },
    },
};

use crate::{
    action_log::{ActionLog, ActionLogAction, ActionLogFeature, ActionLogResult},
    config::{MemoryPrioritySettings, ProcessMemoryPriority},
    foreground::list_processes,
    rules::{execution_failure_suppression_threshold, ExecutionFailureState},
};

const BUILT_IN_EXCLUSIONS: &[&str] = &[
    "csrss.exe",
    "lsaiso.exe",
    "lsass.exe",
    "registry",
    "services.exe",
    "smss.exe",
    "system",
    "wininit.exe",
    "winlogon.exe",
];

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemoryPrioritySnapshot {
    pub enabled: bool,
    pub adjusted_processes: usize,
    pub skipped_processes: usize,
    pub failed_processes: usize,
    pub adjusted_apps: Vec<String>,
    pub last_error: Option<String>,
}

#[derive(Default)]
pub struct MemoryPriorityManager {
    adjusted: BTreeMap<u32, AdjustedProcess>,
    failure_suppression: BTreeMap<String, MemoryPriorityFailureSuppression>,
}

#[derive(Debug, Clone)]
pub struct MemoryPriorityTarget {
    pub process_id: u32,
    pub process_name: String,
    pub priority: ProcessMemoryPriority,
}

#[derive(Clone)]
struct AdjustedProcess {
    process_name: String,
    previous_priority: ProcessMemoryPriority,
    applied_priority: ProcessMemoryPriority,
}

type MemoryPriorityFailureSuppression = ExecutionFailureState;

#[derive(Debug)]
enum MemoryPriorityError {
    AccessDenied,
    ProcessExited,
    Failed(String),
}

impl MemoryPriorityManager {
    pub fn update_rules(
        &mut self,
        settings: &MemoryPrioritySettings,
        automation_enabled: bool,
        foreground_process_id: Option<u32>,
        action_log: &mut ActionLog,
    ) -> MemoryPrioritySnapshot {
        if !automation_enabled {
            let failures = self.clear_all(
                ActionLogFeature::MemoryPriority,
                action_log,
                "automation disabled",
            );
            self.failure_suppression.clear();
            return MemoryPrioritySnapshot {
                enabled: false,
                failed_processes: failures.count,
                last_error: failures.last_error,
                ..Default::default()
            };
        }

        if !settings.enabled {
            let failures = self.clear_all(
                ActionLogFeature::MemoryPriority,
                action_log,
                "memory priority defaults disabled",
            );
            self.failure_suppression.clear();
            return MemoryPrioritySnapshot {
                enabled: false,
                failed_processes: failures.count,
                last_error: failures.last_error,
                ..Default::default()
            };
        }

        let foreground_sensitive = settings.foreground_detection_enabled
            && settings.foreground_priority != settings.background_priority;
        if foreground_sensitive && foreground_process_id.is_none() {
            let failures = self.clear_all(
                ActionLogFeature::MemoryPriority,
                action_log,
                "foreground app is unknown",
            );
            return MemoryPrioritySnapshot {
                enabled: true,
                failed_processes: failures.count,
                last_error: failures.last_error,
                ..Default::default()
            };
        }

        let current_process_id = unsafe { GetCurrentProcessId() };
        let Some(current_session_id) = process_session_id(current_process_id) else {
            let failures = self.clear_all(
                ActionLogFeature::MemoryPriority,
                action_log,
                "current Windows session is unknown",
            );
            return MemoryPrioritySnapshot {
                enabled: true,
                failed_processes: failures.count,
                last_error: failures.last_error,
                ..Default::default()
            };
        };

        let processes = match list_processes() {
            Ok(processes) => processes,
            Err(err) => {
                let failures = self.clear_all(
                    ActionLogFeature::MemoryPriority,
                    action_log,
                    "process list unavailable",
                );
                return MemoryPrioritySnapshot {
                    enabled: true,
                    failed_processes: failures.count + 1,
                    last_error: Some(err),
                    ..Default::default()
                };
            }
        };

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

        let targets = processes
            .into_iter()
            .filter_map(|process| {
                if should_skip_process(
                    process.id,
                    &process.name,
                    current_process_id,
                    current_session_id,
                    settings,
                ) {
                    return None;
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

                priority.priority().map(|priority| MemoryPriorityTarget {
                    process_id: process.id,
                    process_name: process.name,
                    priority,
                })
            })
            .collect();

        let mut snapshot = self.update(targets, true, ActionLogFeature::MemoryPriority, action_log);
        snapshot.enabled = true;
        snapshot
    }

    pub fn update(
        &mut self,
        targets: Vec<MemoryPriorityTarget>,
        automation_enabled: bool,
        action_log_feature: ActionLogFeature,
        action_log: &mut ActionLog,
    ) -> MemoryPrioritySnapshot {
        if !automation_enabled {
            let failures = self.clear_all(action_log_feature, action_log, "automation disabled");
            self.failure_suppression.clear();
            return MemoryPrioritySnapshot {
                enabled: false,
                failed_processes: failures.count,
                last_error: failures.last_error,
                ..Default::default()
            };
        }

        let target_ids = targets
            .iter()
            .map(|target| target.process_id)
            .collect::<BTreeSet<_>>();
        let target_names = targets
            .iter()
            .map(|target| process_failure_key(&target.process_name))
            .collect::<BTreeSet<_>>();
        self.failure_suppression
            .retain(|process_name, _| target_names.contains(process_name));

        let current_process_names = targets
            .iter()
            .map(|target| (target.process_id, target.process_name.clone()))
            .collect::<BTreeMap<_, _>>();
        let mut failures = self.release_non_targets(
            &target_ids,
            &current_process_names,
            action_log_feature,
            action_log,
            "process no longer matches a memory priority target",
        );
        let mut skipped_processes = 0;

        for target in targets {
            if self.is_process_suppressed(
                target.process_id,
                &target.process_name,
                action_log_feature,
                action_log,
            ) {
                skipped_processes += 1;
                continue;
            }

            match self.apply_process(
                target.process_id,
                target.process_name.clone(),
                target.priority,
                action_log_feature,
                action_log,
            ) {
                Ok(ApplyOutcome::Applied) | Ok(ApplyOutcome::AlreadyApplied) => {
                    self.clear_process_failure(&target.process_name);
                }
                Err(MemoryPriorityError::ProcessExited) => {
                    skipped_processes += 1;
                }
                Err(MemoryPriorityError::AccessDenied) => {
                    skipped_processes += 1;
                    self.record_process_failure(&target.process_name);
                    action_log.record(
                        action_log_feature,
                        Some(target.process_id),
                        target.process_name,
                        ActionLogAction::Skip,
                        ActionLogResult::Skipped,
                        "Skipped because the process could not be opened.",
                    );
                }
                Err(err) => {
                    self.record_process_failure(&target.process_name);
                    failures.record(
                        "Apply",
                        target.process_id,
                        &target.process_name,
                        err,
                        action_log_feature,
                        action_log,
                    );
                }
            }
        }

        MemoryPrioritySnapshot {
            enabled: true,
            adjusted_processes: self.adjusted.len(),
            skipped_processes,
            failed_processes: failures.count,
            adjusted_apps: unique_app_names(
                self.adjusted
                    .values()
                    .map(|process| process.process_name.as_str()),
            ),
            last_error: failures.last_error,
        }
    }

    fn apply_process(
        &mut self,
        process_id: u32,
        process_name: String,
        priority: ProcessMemoryPriority,
        action_log_feature: ActionLogFeature,
        action_log: &mut ActionLog,
    ) -> Result<ApplyOutcome, MemoryPriorityError> {
        let process = ProcessHandle::open(process_id)?;
        let reusable_existing = self
            .adjusted
            .get(&process_id)
            .filter(|adjusted| adjusted.process_name.eq_ignore_ascii_case(&process_name));
        let current_priority = process.memory_priority()?;

        if reusable_existing.is_some_and(|adjusted| {
            adjusted.applied_priority == priority && current_priority == priority
        }) {
            return Ok(ApplyOutcome::AlreadyApplied);
        }

        if current_priority != priority {
            process.set_memory_priority(priority)?;
            action_log.record(
                action_log_feature,
                Some(process_id),
                process_name.clone(),
                ActionLogAction::Apply,
                ActionLogResult::Applied,
                format!(
                    "Set memory priority to {}.",
                    memory_priority_label(priority)
                ),
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
        action_log_feature: ActionLogFeature,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> MemoryPriorityFailures {
        let process_ids = self
            .adjusted
            .keys()
            .copied()
            .filter(|process_id| !target_ids.contains(process_id))
            .collect::<Vec<_>>();
        self.release_processes(
            &process_ids,
            Some(current_process_names),
            action_log_feature,
            action_log,
            reason,
        )
    }

    fn clear_all(
        &mut self,
        action_log_feature: ActionLogFeature,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> MemoryPriorityFailures {
        let process_ids = self.adjusted.keys().copied().collect::<Vec<_>>();
        self.release_processes(&process_ids, None, action_log_feature, action_log, reason)
    }

    fn release_processes(
        &mut self,
        process_ids: &[u32],
        current_process_names: Option<&BTreeMap<u32, String>>,
        action_log_feature: ActionLogFeature,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> MemoryPriorityFailures {
        let mut failures = MemoryPriorityFailures::default();
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
                        action_log_feature,
                        Some(*process_id),
                        log_name,
                        ActionLogAction::Restore,
                        ActionLogResult::Restored,
                        format!("Restored previous memory priority: {reason}."),
                    );
                }
                Err(MemoryPriorityError::ProcessExited) => {}
                Err(err) => {
                    self.record_process_failure(&log_name);
                    failures.record(
                        "Restore",
                        *process_id,
                        &log_name,
                        err,
                        action_log_feature,
                        action_log,
                    );
                }
            }
        }
        failures
    }

    fn is_process_suppressed(
        &mut self,
        process_id: u32,
        process_name: &str,
        action_log_feature: ActionLogFeature,
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
                action_log_feature,
                Some(process_id),
                process_name.to_owned(),
                ActionLogAction::Skip,
                ActionLogResult::Skipped,
                format!(
                    "Stopped retrying memory priority after {} failed attempts.",
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
struct MemoryPriorityFailures {
    count: usize,
    last_error: Option<String>,
}

impl MemoryPriorityFailures {
    fn record(
        &mut self,
        action: &str,
        process_id: u32,
        process_name: &str,
        error: MemoryPriorityError,
        action_log_feature: ActionLogFeature,
        action_log: &mut ActionLog,
    ) {
        let message = memory_priority_error_message(error);
        if is_process_exited_message(&message) {
            return;
        }
        if self.last_error.is_none() {
            self.last_error = Some(format!("{action} {process_name} ({process_id}): {message}"));
        }
        self.count += 1;
        action_log.record(
            action_log_feature,
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
    fn open(process_id: u32) -> Result<Self, MemoryPriorityError> {
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

    fn memory_priority(&self) -> Result<ProcessMemoryPriority, MemoryPriorityError> {
        let mut priority = MEMORY_PRIORITY_INFORMATION::default();
        let ok = unsafe {
            GetProcessInformation(
                self.0,
                ProcessMemoryPriorityClass,
                (&mut priority as *mut MEMORY_PRIORITY_INFORMATION).cast(),
                std::mem::size_of::<MEMORY_PRIORITY_INFORMATION>() as u32,
            )
        };
        if ok == 0 {
            Err(open_process_error(0, last_error()))
        } else {
            Ok(memory_priority_from_raw(priority.MemoryPriority))
        }
    }

    fn set_memory_priority(
        &self,
        priority: ProcessMemoryPriority,
    ) -> Result<(), MemoryPriorityError> {
        let info = MEMORY_PRIORITY_INFORMATION {
            MemoryPriority: memory_priority_raw(priority),
        };
        let ok = unsafe {
            SetProcessInformation(
                self.0,
                ProcessMemoryPriorityClass,
                (&info as *const MEMORY_PRIORITY_INFORMATION).cast(),
                std::mem::size_of::<MEMORY_PRIORITY_INFORMATION>() as u32,
            )
        };
        if ok == 0 {
            Err(open_process_error(0, last_error()))
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

fn restore_process(
    process_id: u32,
    process_state: AdjustedProcess,
) -> Result<(), MemoryPriorityError> {
    let process = ProcessHandle::open(process_id)?;
    process.set_memory_priority(process_state.previous_priority)
}

fn open_process_error(process_id: u32, error: u32) -> MemoryPriorityError {
    match error {
        ERROR_ACCESS_DENIED => MemoryPriorityError::AccessDenied,
        ERROR_INVALID_PARAMETER => MemoryPriorityError::ProcessExited,
        _ => MemoryPriorityError::Failed(format!(
            "OpenProcess({process_id}) failed with error {error}."
        )),
    }
}

fn memory_priority_raw(priority: ProcessMemoryPriority) -> u32 {
    match priority {
        ProcessMemoryPriority::VeryLow => MEMORY_PRIORITY_VERY_LOW,
        ProcessMemoryPriority::Low => MEMORY_PRIORITY_LOW,
        ProcessMemoryPriority::Medium => MEMORY_PRIORITY_MEDIUM,
        ProcessMemoryPriority::BelowNormal => MEMORY_PRIORITY_BELOW_NORMAL,
        ProcessMemoryPriority::Normal => MEMORY_PRIORITY_NORMAL,
    }
}

fn memory_priority_from_raw(priority: u32) -> ProcessMemoryPriority {
    match priority {
        MEMORY_PRIORITY_VERY_LOW => ProcessMemoryPriority::VeryLow,
        MEMORY_PRIORITY_LOW => ProcessMemoryPriority::Low,
        MEMORY_PRIORITY_MEDIUM => ProcessMemoryPriority::Medium,
        MEMORY_PRIORITY_BELOW_NORMAL => ProcessMemoryPriority::BelowNormal,
        _ => ProcessMemoryPriority::Normal,
    }
}

pub fn memory_priority_label(priority: ProcessMemoryPriority) -> &'static str {
    match priority {
        ProcessMemoryPriority::VeryLow => "Very Low",
        ProcessMemoryPriority::Low => "Low",
        ProcessMemoryPriority::Medium => "Medium",
        ProcessMemoryPriority::BelowNormal => "Below Normal",
        ProcessMemoryPriority::Normal => "Normal",
    }
}

pub fn is_builtin_excluded(process_name: &str) -> bool {
    let process_name = process_name.trim();
    BUILT_IN_EXCLUSIONS
        .iter()
        .any(|excluded| excluded.eq_ignore_ascii_case(process_name))
}

fn should_skip_process(
    process_id: u32,
    process_name: &str,
    current_process_id: u32,
    current_session_id: u32,
    settings: &MemoryPrioritySettings,
) -> bool {
    process_id == 0
        || process_id == current_process_id
        || process_session_id(process_id) != Some(current_session_id)
        || is_builtin_excluded(process_name)
        || settings.exclusion_enabled_for(process_name)
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

fn memory_priority_error_message(error: MemoryPriorityError) -> String {
    match error {
        MemoryPriorityError::AccessDenied => "Access denied.".to_owned(),
        MemoryPriorityError::ProcessExited => "Process exited.".to_owned(),
        MemoryPriorityError::Failed(message) => message,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_memory_priorities_round_trip() {
        for priority in ProcessMemoryPriority::ALL {
            assert_eq!(
                memory_priority_from_raw(memory_priority_raw(priority)),
                priority
            );
        }
    }

    #[test]
    fn repeated_process_failures_suppress_memory_priority_retries() {
        let mut manager = MemoryPriorityManager::default();
        let mut log = ActionLog::new(8);

        manager.record_process_failure("APP.exe");
        manager.record_process_failure("app.exe");
        assert!(!manager.is_process_suppressed(
            42,
            "app.exe",
            ActionLogFeature::ForegroundResponsiveness,
            &mut log
        ));

        manager.record_process_failure("app.exe");
        assert!(manager.is_process_suppressed(
            42,
            "app.exe",
            ActionLogFeature::ForegroundResponsiveness,
            &mut log
        ));
        assert!(manager.is_process_suppressed(
            43,
            "APP.exe",
            ActionLogFeature::ForegroundResponsiveness,
            &mut log
        ));

        let entries = log.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0].feature,
            ActionLogFeature::ForegroundResponsiveness
        );
        assert_eq!(entries[0].action, ActionLogAction::Skip);
        assert_eq!(entries[0].result, ActionLogResult::Skipped);
        assert!(entries[0]
            .reason
            .contains("Stopped retrying memory priority"));
    }
}
