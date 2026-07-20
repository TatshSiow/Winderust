use std::collections::{BTreeMap, BTreeSet};

use windows_sys::Win32::{
    Foundation::{ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER},
    System::Threading::{
        GetCurrentProcessId, GetPriorityClass, OpenProcess, SetPriorityClass,
        ABOVE_NORMAL_PRIORITY_CLASS, BELOW_NORMAL_PRIORITY_CLASS, HIGH_PRIORITY_CLASS,
        IDLE_PRIORITY_CLASS, NORMAL_PRIORITY_CLASS, PROCESS_QUERY_LIMITED_INFORMATION,
        PROCESS_SET_INFORMATION, REALTIME_PRIORITY_CLASS,
    },
};

use crate::win_util::{last_error, WinHandle};

use crate::{
    action_log::{ActionLog, ActionLogAction, ActionLogFeature, ActionLogResult},
    config::{ProcessPrioritySetting, ProcessPrioritySettings},
    foreground::{
        is_foreground_process, is_process_exited_message, list_processes, process_failure_key,
        process_names_by_id, process_session_id, same_process_name, unique_app_names,
        ProcessActionTarget, CORE_BUILT_IN_PROCESS_EXCLUSIONS,
    },
    rules::{execution_failure_suppression_threshold, ExecutionFailureTracker},
};

const BUILT_IN_EXCLUSIONS: &[&str] = CORE_BUILT_IN_PROCESS_EXCLUSIONS;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProcessPrioritySnapshot {
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
pub struct ProcessPriorityManager {
    adjusted: BTreeMap<u32, AdjustedProcess>,
    failure_suppression: ExecutionFailureTracker,
}

#[derive(Clone)]
struct AdjustedProcess {
    process_name: String,
    creation_time: u64,
    previous_priority_class: u32,
    applied_priority_class: u32,
}

#[derive(Debug)]
enum ProcessPriorityError {
    AccessDenied,
    ProcessExited,
    Failed(String),
}

impl ProcessPriorityManager {
    pub(crate) fn update(
        &mut self,
        settings: &ProcessPrioritySettings,
        automation_enabled: bool,
        foreground_process_id: Option<u32>,
        excluded_process_ids: &BTreeSet<u32>,
        action_log: &mut ActionLog,
    ) -> ProcessPrioritySnapshot {
        if !automation_enabled {
            let failures = self.clear_all(action_log, "automation disabled");
            self.failure_suppression.clear();
            return ProcessPrioritySnapshot {
                enabled: false,
                failed_processes: failures.count,
                message: "Automation disabled.".to_owned(),
                last_error: failures.last_error,
                ..Default::default()
            };
        }

        if !settings.enabled {
            let failures = self.clear_all(action_log, "Process priority defaults disabled");
            self.failure_suppression.clear();
            return ProcessPrioritySnapshot {
                enabled: false,
                failed_processes: failures.count,
                message: "Process priority defaults disabled.".to_owned(),
                last_error: failures.last_error,
                ..Default::default()
            };
        }

        let foreground_sensitive = settings.foreground_detection_enabled
            && settings.foreground_priority != settings.background_priority;
        if foreground_sensitive && foreground_process_id.is_none() {
            let failures = self.clear_all(action_log, "foreground app is unknown");
            return ProcessPrioritySnapshot {
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
            return ProcessPrioritySnapshot {
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
                return ProcessPrioritySnapshot {
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
                || excluded_process_ids.contains(&process.id)
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
                Some(Some(ProcessPrioritySetting::Auto)) if foreground => {
                    settings.foreground_priority
                }
                Some(Some(ProcessPrioritySetting::Auto)) => settings.background_priority,
                Some(Some(priority)) => priority,
                Some(None) => continue,
                None if foreground => settings.foreground_priority,
                None => settings.background_priority,
            };
            if let Some(priority_class) = priority_class(priority) {
                target_processes.insert(process.id, (process.name, priority_class, foreground));
            }
        }

        let mut target_ids = target_processes.keys().copied().collect::<BTreeSet<_>>();
        target_ids.extend(
            excluded_process_ids
                .iter()
                .filter(|process_id| self.adjusted.contains_key(process_id))
                .copied(),
        );
        let active_target_names = target_processes
            .values()
            .map(|(name, _priority, _foreground)| process_failure_key(name))
            .collect::<BTreeSet<_>>();
        self.failure_suppression.retain_keys(&active_target_names);

        let mut failures = self.release_non_targets(
            &target_ids,
            &current_process_names,
            action_log,
            "process is excluded or no longer matches Process Priority defaults",
        );
        let mut skipped_processes = 0;
        let mut applied_processes = 0;
        let mut auto_excluded_processes = BTreeSet::new();

        for (process_id, (process_name, priority_class, foreground)) in target_processes {
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
                priority_class,
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
                Err(ProcessPriorityError::ProcessExited) => skipped_processes += 1,
                Err(ProcessPriorityError::AccessDenied) => {
                    skipped_processes += 1;
                    self.record_process_failure(&process_name);
                    action_log.record(
                        ActionLogFeature::ProcessPriority,
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
                ActionLogFeature::ProcessPriority,
                None,
                "Process Priority",
                ActionLogAction::Apply,
                ActionLogResult::Applied,
                format!("Applied process priority defaults to {applied_processes} process(es)."),
            );
        }

        ProcessPrioritySnapshot {
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
            message: "Process priority defaults active.".to_owned(),
            last_error: failures.last_error,
        }
    }

    fn apply_process(
        &mut self,
        process_id: u32,
        process_name: String,
        priority_class: u32,
        foreground: bool,
        preserve_foreground: bool,
        preserve_background: bool,
    ) -> Result<ApplyOutcome, ProcessPriorityError> {
        let process = ProcessHandle::open(process_id)?;
        let creation_time = process
            .0
            .process_creation_time()
            .ok_or(ProcessPriorityError::ProcessExited)?;
        let reusable_existing = self.adjusted.get(&process_id).filter(|adjusted| {
            adjusted.creation_time == creation_time
                && same_process_name(&adjusted.process_name, &process_name)
        });
        let current_priority_class = process.priority_class()?;

        if current_priority_class == REALTIME_PRIORITY_CLASS {
            return Err(ProcessPriorityError::AccessDenied);
        }

        let baseline_priority_class = reusable_existing
            .map(|adjusted| adjusted.previous_priority_class)
            .unwrap_or(current_priority_class);
        if should_preserve_priority(
            foreground,
            preserve_foreground,
            preserve_background,
            process_priority_rank(baseline_priority_class),
            process_priority_rank(priority_class),
        ) {
            if let Some(adjusted) = reusable_existing.cloned() {
                process.set_priority_class(adjusted.previous_priority_class)?;
                self.adjusted.remove(&process_id);
            }
            return Ok(ApplyOutcome::Preserved);
        }

        if reusable_existing.is_some_and(|adjusted| {
            adjusted.applied_priority_class == priority_class
                && current_priority_class == priority_class
        }) {
            return Ok(ApplyOutcome::AlreadyApplied);
        }

        if current_priority_class != priority_class {
            process.set_priority_class(priority_class)?;
            let refreshed_priority_class = process.priority_class()?;
            if refreshed_priority_class != priority_class {
                return Err(ProcessPriorityError::Failed(format!(
                    "Process priority remained {} after requesting {}.",
                    priority_class_label(refreshed_priority_class),
                    priority_class_label(priority_class)
                )));
            }
        }

        self.adjusted.insert(
            process_id,
            AdjustedProcess {
                process_name,
                creation_time,
                previous_priority_class: baseline_priority_class,
                applied_priority_class: priority_class,
            },
        );
        Ok(ApplyOutcome::Applied {
            loggable: current_priority_class != priority_class,
        })
    }

    fn release_non_targets(
        &mut self,
        target_ids: &BTreeSet<u32>,
        current_process_names: &BTreeMap<u32, String>,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> ProcessPriorityFailures {
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

    fn clear_all(&mut self, action_log: &mut ActionLog, reason: &str) -> ProcessPriorityFailures {
        let process_ids = self.adjusted.keys().copied().collect::<Vec<_>>();
        self.release_processes(&process_ids, None, action_log, reason)
    }

    fn release_processes(
        &mut self,
        process_ids: &[u32],
        current_process_names: Option<&BTreeMap<u32, String>>,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> ProcessPriorityFailures {
        let mut failures = ProcessPriorityFailures::default();
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
                Err(ProcessPriorityError::ProcessExited) => {
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
                ActionLogFeature::ProcessPriority,
                None,
                "Process Priority",
                ActionLogAction::Restore,
                ActionLogResult::Restored,
                format!(
                    "Restored process priority for {restored_processes} process(es): {reason}."
                ),
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
                ActionLogFeature::ProcessPriority,
                Some(process_id),
                process_name.to_owned(),
                ActionLogAction::Skip,
                ActionLogResult::Skipped,
                format!(
                    "Stopped retrying Process Priority after {} failed attempts.",
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

impl Drop for ProcessPriorityManager {
    fn drop(&mut self) {
        let mut action_log = ActionLog::new(1);
        self.clear_all(&mut action_log, stringify!(ProcessPriorityManager));
    }
}

enum ApplyOutcome {
    Applied { loggable: bool },
    AlreadyApplied,
    Preserved,
}

#[derive(Default)]
struct ProcessPriorityFailures {
    count: usize,
    last_error: Option<String>,
}

impl ProcessPriorityFailures {
    fn record(
        &mut self,
        action: &str,
        process_id: u32,
        process_name: &str,
        error: ProcessPriorityError,
        action_log: &mut ActionLog,
    ) {
        let message = process_priority_error_message(error);
        if is_process_exited_message(&message) {
            return;
        }
        if self.last_error.is_none() {
            self.last_error = Some(format!("{action} {process_name} ({process_id}): {message}"));
        }
        self.count += 1;
        action_log.record(
            ActionLogFeature::ProcessPriority,
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
    fn open(process_id: u32) -> Result<Self, ProcessPriorityError> {
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

    fn priority_class(&self) -> Result<u32, ProcessPriorityError> {
        let priority_class = unsafe { GetPriorityClass(self.0.raw()) };
        if priority_class != 0 {
            Ok(priority_class)
        } else {
            Err(ProcessPriorityError::Failed(format!(
                "GetPriorityClass failed with error {}.",
                last_error()
            )))
        }
    }

    fn set_priority_class(&self, priority_class: u32) -> Result<(), ProcessPriorityError> {
        if unsafe { SetPriorityClass(self.0.raw(), priority_class) } != 0 {
            Ok(())
        } else {
            Err(ProcessPriorityError::Failed(format!(
                "SetPriorityClass failed with error {}.",
                last_error()
            )))
        }
    }
}

fn restore_process(
    process_id: u32,
    process_state: &AdjustedProcess,
) -> Result<(), ProcessPriorityError> {
    let process = ProcessHandle::open(process_id)?;
    if process.0.process_creation_time() != Some(process_state.creation_time) {
        return Err(ProcessPriorityError::ProcessExited);
    }
    if process.priority_class()? == REALTIME_PRIORITY_CLASS {
        return Ok(());
    }
    process.set_priority_class(process_state.previous_priority_class)?;
    let refreshed_priority_class = process.priority_class()?;
    if refreshed_priority_class == process_state.previous_priority_class {
        Ok(())
    } else {
        Err(ProcessPriorityError::Failed(format!(
            "Process priority remained {} after restoring {}.",
            priority_class_label(refreshed_priority_class),
            priority_class_label(process_state.previous_priority_class)
        )))
    }
}

fn open_process_error(process_id: u32, error: u32) -> ProcessPriorityError {
    match error {
        ERROR_ACCESS_DENIED => ProcessPriorityError::AccessDenied,
        ERROR_INVALID_PARAMETER => ProcessPriorityError::ProcessExited,
        _ => ProcessPriorityError::Failed(format!(
            "OpenProcess({process_id}) failed with error {error}."
        )),
    }
}

fn priority_class(priority: ProcessPrioritySetting) -> Option<u32> {
    match priority {
        ProcessPrioritySetting::Default | ProcessPrioritySetting::Auto => None,
        ProcessPrioritySetting::Realtime => Some(REALTIME_PRIORITY_CLASS),
        ProcessPrioritySetting::High => Some(HIGH_PRIORITY_CLASS),
        ProcessPrioritySetting::AboveNormal => Some(ABOVE_NORMAL_PRIORITY_CLASS),
        ProcessPrioritySetting::Normal => Some(NORMAL_PRIORITY_CLASS),
        ProcessPrioritySetting::BelowNormal => Some(BELOW_NORMAL_PRIORITY_CLASS),
        ProcessPrioritySetting::Idle => Some(IDLE_PRIORITY_CLASS),
    }
}

pub(crate) fn apply_once(
    target: &ProcessActionTarget,
    priority: ProcessPrioritySetting,
) -> Result<&'static str, String> {
    let priority_class = quick_apply_priority_class(priority)
        .ok_or_else(|| "This priority is not available as a quick action.".to_owned())?;
    if is_builtin_excluded(&target.name) {
        return Err("Built-in Windows processes cannot be modified.".to_owned());
    }
    let process = ProcessHandle::open(target.id).map_err(process_priority_error_message)?;
    if process.0.process_creation_time() != Some(target.creation_time) {
        return Err("The selected process instance has changed.".to_owned());
    }
    if process
        .priority_class()
        .map_err(process_priority_error_message)?
        == REALTIME_PRIORITY_CLASS
    {
        return Err("Realtime processes are not changed by quick actions.".to_owned());
    }
    process
        .set_priority_class(priority_class)
        .map_err(process_priority_error_message)?;
    let applied = process
        .priority_class()
        .map_err(process_priority_error_message)?;
    if applied != priority_class {
        return Err(format!(
            "Process priority remained {}.",
            priority_class_label(applied)
        ));
    }
    Ok(priority_class_label(applied))
}

fn quick_apply_priority_class(priority: ProcessPrioritySetting) -> Option<u32> {
    match priority {
        ProcessPrioritySetting::Idle => Some(IDLE_PRIORITY_CLASS),
        ProcessPrioritySetting::BelowNormal => Some(BELOW_NORMAL_PRIORITY_CLASS),
        ProcessPrioritySetting::Normal => Some(NORMAL_PRIORITY_CLASS),
        ProcessPrioritySetting::AboveNormal => Some(ABOVE_NORMAL_PRIORITY_CLASS),
        ProcessPrioritySetting::Default
        | ProcessPrioritySetting::Auto
        | ProcessPrioritySetting::High
        | ProcessPrioritySetting::Realtime => None,
    }
}

fn priority_class_label(priority_class: u32) -> &'static str {
    match priority_class {
        HIGH_PRIORITY_CLASS => "High",
        REALTIME_PRIORITY_CLASS => "Realtime",
        ABOVE_NORMAL_PRIORITY_CLASS => "Above Normal",
        NORMAL_PRIORITY_CLASS => "Normal",
        BELOW_NORMAL_PRIORITY_CLASS => "Below Normal",
        IDLE_PRIORITY_CLASS => "Idle",
        _ => "Unknown",
    }
}

fn process_priority_rank(priority_class: u32) -> i32 {
    match priority_class {
        IDLE_PRIORITY_CLASS => 0,
        BELOW_NORMAL_PRIORITY_CLASS => 1,
        NORMAL_PRIORITY_CLASS => 2,
        ABOVE_NORMAL_PRIORITY_CLASS => 3,
        HIGH_PRIORITY_CLASS => 4,
        REALTIME_PRIORITY_CLASS => 5,
        _ => 2,
    }
}

fn should_preserve_priority(
    foreground: bool,
    preserve_foreground: bool,
    preserve_background: bool,
    current_rank: i32,
    desired_rank: i32,
) -> bool {
    if foreground {
        preserve_foreground && current_rank >= desired_rank
    } else {
        preserve_background && current_rank <= desired_rank
    }
}

fn process_priority_error_message(error: ProcessPriorityError) -> String {
    match error {
        ProcessPriorityError::AccessDenied => "Access denied.".to_owned(),
        ProcessPriorityError::ProcessExited => "Process exited.".to_owned(),
        ProcessPriorityError::Failed(message) => message,
    }
}

pub fn is_builtin_excluded(process_name: &str) -> bool {
    BUILT_IN_EXCLUSIONS
        .iter()
        .any(|excluded| same_process_name(excluded, process_name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quick_apply_only_accepts_safe_concrete_priorities() {
        assert_eq!(
            quick_apply_priority_class(ProcessPrioritySetting::BelowNormal),
            Some(BELOW_NORMAL_PRIORITY_CLASS)
        );
        assert_eq!(
            quick_apply_priority_class(ProcessPrioritySetting::Normal),
            Some(NORMAL_PRIORITY_CLASS)
        );
        assert_eq!(
            quick_apply_priority_class(ProcessPrioritySetting::AboveNormal),
            Some(ABOVE_NORMAL_PRIORITY_CLASS)
        );
        assert_eq!(
            quick_apply_priority_class(ProcessPrioritySetting::High),
            None
        );
        assert_eq!(
            quick_apply_priority_class(ProcessPrioritySetting::Realtime),
            None
        );
    }
}
