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
    config::{CpuPrioritySettings, ProcessCpuPrioritySetting},
    foreground::{
        is_foreground_process, is_process_exited_message, list_processes, process_failure_key,
        process_names_by_id, process_session_id, same_process_name, unique_app_names,
        CORE_BUILT_IN_PROCESS_EXCLUSIONS,
    },
    rules::{execution_failure_suppression_threshold, ExecutionFailureTracker},
};

const BUILT_IN_EXCLUSIONS: &[&str] = CORE_BUILT_IN_PROCESS_EXCLUSIONS;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CpuPrioritySnapshot {
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
pub struct CpuPriorityManager {
    adjusted: BTreeMap<u32, AdjustedProcess>,
    failure_suppression: ExecutionFailureTracker,
}

#[derive(Clone)]
struct AdjustedProcess {
    process_name: String,
    previous_priority_class: u32,
    applied_priority_class: u32,
}

#[derive(Debug)]
enum CpuPriorityError {
    AccessDenied,
    ProcessExited,
    Failed(String),
}

impl CpuPriorityManager {
    pub fn update(
        &mut self,
        settings: &CpuPrioritySettings,
        automation_enabled: bool,
        foreground_process_id: Option<u32>,
        action_log: &mut ActionLog,
    ) -> CpuPrioritySnapshot {
        if !automation_enabled {
            let failures = self.clear_all(action_log, "automation disabled");
            self.failure_suppression.clear();
            return CpuPrioritySnapshot {
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
            return CpuPrioritySnapshot {
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
            return CpuPrioritySnapshot {
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
            return CpuPrioritySnapshot {
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
                return CpuPrioritySnapshot {
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
                Some(Some(ProcessCpuPrioritySetting::Auto)) if foreground => {
                    settings.foreground_priority
                }
                Some(Some(ProcessCpuPrioritySetting::Auto)) => settings.background_priority,
                Some(Some(priority)) => priority,
                Some(None) => continue,
                None if foreground => settings.foreground_priority,
                None => settings.background_priority,
            };
            if let Some(priority_class) = priority_class(priority) {
                target_processes.insert(process.id, (process.name, priority_class, foreground));
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
            "process is excluded or no longer matches Process Priority defaults",
        );
        let mut skipped_processes = 0;
        let mut applied_processes = 0;

        for (process_id, (process_name, priority_class, foreground)) in target_processes {
            if self.is_process_suppressed(process_id, &process_name, action_log) {
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
                Err(CpuPriorityError::ProcessExited) => skipped_processes += 1,
                Err(CpuPriorityError::AccessDenied) => {
                    skipped_processes += 1;
                    self.record_process_failure(&process_name);
                    action_log.record(
                        ActionLogFeature::CpuPriority,
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
                ActionLogFeature::CpuPriority,
                None,
                "Process Priority",
                ActionLogAction::Apply,
                ActionLogResult::Applied,
                format!("Applied process priority defaults to {applied_processes} process(es)."),
            );
        }

        CpuPrioritySnapshot {
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
    ) -> Result<ApplyOutcome, CpuPriorityError> {
        let process = ProcessHandle::open(process_id)?;
        let reusable_existing = self
            .adjusted
            .get(&process_id)
            .filter(|adjusted| same_process_name(&adjusted.process_name, &process_name));
        let current_priority_class = process.priority_class()?;

        if current_priority_class == REALTIME_PRIORITY_CLASS {
            return Err(CpuPriorityError::AccessDenied);
        }

        let baseline_priority_class = reusable_existing
            .map(|adjusted| adjusted.previous_priority_class)
            .unwrap_or(current_priority_class);
        if should_preserve_priority(
            foreground,
            preserve_foreground,
            preserve_background,
            cpu_priority_rank(baseline_priority_class),
            cpu_priority_rank(priority_class),
        ) {
            if let Some(adjusted) = self.adjusted.remove(&process_id) {
                process.set_priority_class(adjusted.previous_priority_class)?;
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
                return Err(CpuPriorityError::Failed(format!(
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
    ) -> CpuPriorityFailures {
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

    fn clear_all(&mut self, action_log: &mut ActionLog, reason: &str) -> CpuPriorityFailures {
        let process_ids = self.adjusted.keys().copied().collect::<Vec<_>>();
        self.release_processes(&process_ids, None, action_log, reason)
    }

    fn release_processes(
        &mut self,
        process_ids: &[u32],
        current_process_names: Option<&BTreeMap<u32, String>>,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> CpuPriorityFailures {
        let mut failures = CpuPriorityFailures::default();
        let mut restored_processes = 0;
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
                    restored_processes += 1;
                }
                Err(CpuPriorityError::ProcessExited) => {}
                Err(err) => {
                    self.record_process_failure(&log_name);
                    failures.record("Restore", *process_id, &log_name, err, action_log);
                }
            }
        }
        if restored_processes > 0 {
            action_log.record(
                ActionLogFeature::CpuPriority,
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
    ) -> bool {
        let suppression = self.failure_suppression.process_suppression(process_name);
        if !suppression.suppressed {
            return false;
        }

        if suppression.newly_suppressed {
            action_log.record(
                ActionLogFeature::CpuPriority,
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

enum ApplyOutcome {
    Applied { loggable: bool },
    AlreadyApplied,
    Preserved,
}

#[derive(Default)]
struct CpuPriorityFailures {
    count: usize,
    last_error: Option<String>,
}

impl CpuPriorityFailures {
    fn record(
        &mut self,
        action: &str,
        process_id: u32,
        process_name: &str,
        error: CpuPriorityError,
        action_log: &mut ActionLog,
    ) {
        let message = cpu_priority_error_message(error);
        if is_process_exited_message(&message) {
            return;
        }
        if self.last_error.is_none() {
            self.last_error = Some(format!("{action} {process_name} ({process_id}): {message}"));
        }
        self.count += 1;
        action_log.record(
            ActionLogFeature::CpuPriority,
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
    fn open(process_id: u32) -> Result<Self, CpuPriorityError> {
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

    fn priority_class(&self) -> Result<u32, CpuPriorityError> {
        let priority_class = unsafe { GetPriorityClass(self.0.raw()) };
        if priority_class != 0 {
            Ok(priority_class)
        } else {
            Err(CpuPriorityError::Failed(format!(
                "GetPriorityClass failed with error {}.",
                last_error()
            )))
        }
    }

    fn set_priority_class(&self, priority_class: u32) -> Result<(), CpuPriorityError> {
        if unsafe { SetPriorityClass(self.0.raw(), priority_class) } != 0 {
            Ok(())
        } else {
            Err(CpuPriorityError::Failed(format!(
                "SetPriorityClass failed with error {}.",
                last_error()
            )))
        }
    }
}

fn restore_process(
    process_id: u32,
    process_state: AdjustedProcess,
) -> Result<(), CpuPriorityError> {
    let process = ProcessHandle::open(process_id)?;
    if process.priority_class()? == REALTIME_PRIORITY_CLASS {
        return Ok(());
    }
    process.set_priority_class(process_state.previous_priority_class)?;
    let refreshed_priority_class = process.priority_class()?;
    if refreshed_priority_class == process_state.previous_priority_class {
        Ok(())
    } else {
        Err(CpuPriorityError::Failed(format!(
            "Process priority remained {} after restoring {}.",
            priority_class_label(refreshed_priority_class),
            priority_class_label(process_state.previous_priority_class)
        )))
    }
}

fn open_process_error(process_id: u32, error: u32) -> CpuPriorityError {
    match error {
        ERROR_ACCESS_DENIED => CpuPriorityError::AccessDenied,
        ERROR_INVALID_PARAMETER => CpuPriorityError::ProcessExited,
        _ => CpuPriorityError::Failed(format!(
            "OpenProcess({process_id}) failed with error {error}."
        )),
    }
}

fn priority_class(priority: ProcessCpuPrioritySetting) -> Option<u32> {
    match priority {
        ProcessCpuPrioritySetting::Default | ProcessCpuPrioritySetting::Auto => None,
        ProcessCpuPrioritySetting::Realtime => Some(REALTIME_PRIORITY_CLASS),
        ProcessCpuPrioritySetting::High => Some(HIGH_PRIORITY_CLASS),
        ProcessCpuPrioritySetting::AboveNormal => Some(ABOVE_NORMAL_PRIORITY_CLASS),
        ProcessCpuPrioritySetting::Normal => Some(NORMAL_PRIORITY_CLASS),
        ProcessCpuPrioritySetting::BelowNormal => Some(BELOW_NORMAL_PRIORITY_CLASS),
        ProcessCpuPrioritySetting::Idle => Some(IDLE_PRIORITY_CLASS),
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

fn cpu_priority_rank(priority_class: u32) -> i32 {
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

fn cpu_priority_error_message(error: CpuPriorityError) -> String {
    match error {
        CpuPriorityError::AccessDenied => "Access denied.".to_owned(),
        CpuPriorityError::ProcessExited => "Process exited.".to_owned(),
        CpuPriorityError::Failed(message) => message,
    }
}

pub fn is_builtin_excluded(process_name: &str) -> bool {
    BUILT_IN_EXCLUSIONS
        .iter()
        .any(|excluded| same_process_name(excluded, process_name))
}
