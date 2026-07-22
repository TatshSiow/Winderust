use std::{
    collections::{BTreeMap, BTreeSet},
    mem::size_of,
};

use windows_sys::Win32::{
    Foundation::{
        ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER, ERROR_NO_MORE_FILES, FILETIME,
        INVALID_HANDLE_VALUE,
    },
    System::{
        Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, Thread32First, Thread32Next, TH32CS_SNAPTHREAD, THREADENTRY32,
        },
        Threading::{
            GetCurrentProcessId, GetThreadPriority, GetThreadTimes, OpenThread, SetThreadPriority,
            THREAD_PRIORITY_ABOVE_NORMAL, THREAD_PRIORITY_BELOW_NORMAL, THREAD_PRIORITY_HIGHEST,
            THREAD_PRIORITY_IDLE, THREAD_PRIORITY_LOWEST, THREAD_PRIORITY_NORMAL,
            THREAD_PRIORITY_TIME_CRITICAL, THREAD_QUERY_INFORMATION, THREAD_SET_INFORMATION,
        },
    },
};

use crate::{
    action_log::{ActionLog, ActionLogAction, ActionLogFeature, ActionLogResult},
    config::{ProcessThreadPrioritySetting, ThreadPrioritySettings},
    foreground::{
        is_foreground_process, list_processes, process_failure_key, process_names_by_id,
        process_session_id, same_process_name, unique_app_names, CORE_BUILT_IN_PROCESS_EXCLUSIONS,
    },
    rules::{execution_failure_suppression_threshold, ExecutionFailureTracker},
    win_util::{filetime_to_u64, last_error, WinHandle},
};

const BUILT_IN_EXCLUSIONS: &[&str] = CORE_BUILT_IN_PROCESS_EXCLUSIONS;
const THREAD_PRIORITY_ERROR_RETURN: i32 = i32::MAX;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThreadPrioritySnapshot {
    pub enabled: bool,
    pub scanned_processes: usize,
    pub adjusted_processes: usize,
    pub adjusted_threads: usize,
    pub skipped_processes: usize,
    pub failed_processes: usize,
    pub adjusted_apps: Vec<String>,
    pub auto_excluded_processes: Vec<String>,
    pub message: String,
    pub last_error: Option<String>,
}

#[derive(Default)]
pub struct ThreadPriorityManager {
    adjusted: BTreeMap<u32, AdjustedThread>,
    failure_suppression: ExecutionFailureTracker,
}

struct ThreadApplyOutcome {
    applied_threads: usize,
    preserved_threads: usize,
}

#[derive(Clone)]
struct AdjustedThread {
    process_id: u32,
    process_name: String,
    creation_time: u64,
    previous_priority: i32,
    applied_priority: i32,
}

#[derive(Debug)]
enum ThreadPriorityError {
    AccessDenied,
    ProcessExited,
    Failed(String),
}

impl ThreadPriorityManager {
    pub fn update(
        &mut self,
        settings: &ThreadPrioritySettings,
        automation_enabled: bool,
        foreground_process_id: Option<u32>,
        action_log: &mut ActionLog,
    ) -> ThreadPrioritySnapshot {
        if !automation_enabled {
            let failures = self.clear_all(action_log, "automation disabled");
            self.failure_suppression.clear();
            return ThreadPrioritySnapshot {
                enabled: false,
                failed_processes: failures.count,
                message: "Automation disabled.".to_owned(),
                last_error: failures.last_error,
                ..Default::default()
            };
        }

        if !settings.enabled {
            let failures = self.clear_all(action_log, "Thread Priority disabled");
            self.failure_suppression.clear();
            return ThreadPrioritySnapshot {
                enabled: false,
                failed_processes: failures.count,
                message: "Thread Priority disabled.".to_owned(),
                last_error: failures.last_error,
                ..Default::default()
            };
        }

        let foreground_sensitive = settings.foreground_detection_enabled
            && settings.foreground_priority != settings.background_priority;
        if foreground_sensitive && foreground_process_id.is_none() {
            let failures = self.clear_all(action_log, "foreground app is unknown");
            return ThreadPrioritySnapshot {
                enabled: true,
                failed_processes: failures.count,
                message: "Paused: foreground app is unknown.".to_owned(),
                last_error: failures.last_error,
                ..Default::default()
            };
        }

        // SAFETY: GetCurrentProcessId takes no arguments and has no caller requirements.
        let current_process_id = unsafe { GetCurrentProcessId() };
        let Some(current_session_id) = process_session_id(current_process_id) else {
            let failures = self.clear_all(action_log, "current Windows session is unknown");
            return ThreadPrioritySnapshot {
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
                return ThreadPrioritySnapshot {
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
                Some(Some(ProcessThreadPrioritySetting::Auto)) if foreground => {
                    settings.foreground_priority
                }
                Some(Some(ProcessThreadPrioritySetting::Auto)) => settings.background_priority,
                Some(Some(priority)) => priority,
                Some(None) => continue,
                None if foreground => settings.foreground_priority,
                None => settings.background_priority,
            };
            if let Some(priority) = thread_priority_value(priority) {
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
            "process is excluded or no longer matches Thread Priority defaults",
        );
        let mut skipped_processes = 0;
        let mut applied_threads = 0;
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

            match self.apply_process_threads(
                process_id,
                process_name.clone(),
                priority,
                foreground,
                settings.preserve_foreground_priority,
                settings.preserve_background_priority,
            ) {
                Ok(outcome) => {
                    applied_threads += outcome.applied_threads;
                    skipped_processes +=
                        usize::from(outcome.applied_threads == 0 && outcome.preserved_threads > 0);
                    self.clear_process_failure(&process_name);
                }
                Err(ThreadPriorityError::ProcessExited) => skipped_processes += 1,
                Err(ThreadPriorityError::AccessDenied) => {
                    skipped_processes += 1;
                    self.record_process_failure(&process_name);
                    action_log.record(
                        ActionLogFeature::ThreadPriority,
                        Some(process_id),
                        process_name,
                        ActionLogAction::Skip,
                        ActionLogResult::Skipped,
                        "Skipped because one or more threads could not be opened.",
                    );
                }
                Err(err) => {
                    self.record_process_failure(&process_name);
                    failures.record("Apply", process_id, &process_name, err, action_log);
                }
            }
        }
        if applied_threads > 0 {
            action_log.record(
                ActionLogFeature::ThreadPriority,
                None,
                "Thread Priority",
                ActionLogAction::Apply,
                ActionLogResult::Applied,
                format!("Applied thread priority to {applied_threads} thread(s)."),
            );
        }

        let adjusted_process_ids = self
            .adjusted
            .values()
            .map(|thread| thread.process_id)
            .collect::<BTreeSet<_>>();
        ThreadPrioritySnapshot {
            enabled: true,
            scanned_processes,
            adjusted_processes: adjusted_process_ids.len(),
            adjusted_threads: self.adjusted.len(),
            skipped_processes,
            failed_processes: failures.count,
            adjusted_apps: unique_app_names(
                self.adjusted
                    .values()
                    .map(|thread| thread.process_name.as_str()),
            ),
            auto_excluded_processes: auto_excluded_processes.into_iter().collect(),
            message: "Thread Priority active.".to_owned(),
            last_error: failures.last_error,
        }
    }

    fn apply_process_threads(
        &mut self,
        process_id: u32,
        process_name: String,
        priority: i32,
        foreground: bool,
        preserve_foreground: bool,
        preserve_background: bool,
    ) -> Result<ThreadApplyOutcome, ThreadPriorityError> {
        let mut applied = 0;
        let mut preserved = 0;
        for thread_id in process_thread_ids(process_id)? {
            let thread = ThreadHandle::open(thread_id)?;
            let creation_time = thread.creation_time()?;
            let current_priority = thread.priority()?;
            let reusable_existing = self.adjusted.get(&thread_id).filter(|adjusted| {
                adjusted.process_id == process_id
                    && adjusted.creation_time == creation_time
                    && same_process_name(&adjusted.process_name, &process_name)
            });
            let baseline_priority = reusable_existing
                .map(|adjusted| adjusted.previous_priority)
                .unwrap_or(current_priority);
            if should_preserve_priority(
                foreground,
                preserve_foreground,
                preserve_background,
                baseline_priority,
                priority,
            ) {
                if let Some(adjusted) = reusable_existing.cloned() {
                    thread.set_priority(adjusted.previous_priority)?;
                    self.adjusted.remove(&thread_id);
                }
                preserved += 1;
                continue;
            }
            if reusable_existing.is_some_and(|adjusted| {
                adjusted.applied_priority == priority && current_priority == priority
            }) {
                continue;
            }
            if current_priority != priority {
                thread.set_priority(priority)?;
                if thread.priority()? != priority {
                    return Err(ThreadPriorityError::Failed(format!(
                        "Thread priority remained {} after requesting {}.",
                        thread_priority_label(current_priority),
                        thread_priority_label(priority)
                    )));
                }
                applied += 1;
            }
            self.adjusted.insert(
                thread_id,
                AdjustedThread {
                    process_id,
                    process_name: process_name.clone(),
                    creation_time,
                    previous_priority: baseline_priority,
                    applied_priority: priority,
                },
            );
        }
        Ok(ThreadApplyOutcome {
            applied_threads: applied,
            preserved_threads: preserved,
        })
    }

    fn release_non_targets(
        &mut self,
        target_ids: &BTreeSet<u32>,
        current_process_names: &BTreeMap<u32, String>,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> ThreadPriorityFailures {
        let thread_ids = self
            .adjusted
            .iter()
            .filter_map(|(thread_id, thread)| {
                (!target_ids.contains(&thread.process_id)).then_some(*thread_id)
            })
            .collect::<Vec<_>>();
        self.release_threads(&thread_ids, Some(current_process_names), action_log, reason)
    }

    fn clear_all(&mut self, action_log: &mut ActionLog, reason: &str) -> ThreadPriorityFailures {
        let thread_ids = self.adjusted.keys().copied().collect::<Vec<_>>();
        self.release_threads(&thread_ids, None, action_log, reason)
    }

    fn release_threads(
        &mut self,
        thread_ids: &[u32],
        current_process_names: Option<&BTreeMap<u32, String>>,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> ThreadPriorityFailures {
        let mut failures = ThreadPriorityFailures::default();
        let mut restored_threads = 0;
        for thread_id in thread_ids {
            let Some(thread_state) = self.adjusted.get(thread_id).cloned() else {
                continue;
            };
            let log_name = current_process_names
                .and_then(|names| names.get(&thread_state.process_id))
                .cloned()
                .unwrap_or_else(|| thread_state.process_name.clone());
            match restore_thread(*thread_id, &thread_state) {
                Ok(()) => {
                    self.adjusted.remove(thread_id);
                    restored_threads += 1;
                }
                Err(ThreadPriorityError::ProcessExited) => {
                    self.adjusted.remove(thread_id);
                }
                Err(err) => failures.record("Restore", *thread_id, &log_name, err, action_log),
            }
        }
        if restored_threads > 0 {
            action_log.record(
                ActionLogFeature::ThreadPriority,
                None,
                "Thread Priority",
                ActionLogAction::Restore,
                ActionLogResult::Restored,
                format!("Restored thread priority for {restored_threads} thread(s): {reason}."),
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
                ActionLogFeature::ThreadPriority,
                Some(process_id),
                process_name.to_owned(),
                ActionLogAction::Skip,
                ActionLogResult::Skipped,
                format!(
                    "Stopped retrying Thread Priority after {} failed attempts.",
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

#[derive(Default)]
struct ThreadPriorityFailures {
    count: usize,
    last_error: Option<String>,
}

impl ThreadPriorityFailures {
    fn record(
        &mut self,
        action: &str,
        id: u32,
        process_name: &str,
        error: ThreadPriorityError,
        action_log: &mut ActionLog,
    ) {
        if matches!(&error, ThreadPriorityError::ProcessExited) {
            return;
        }
        let message = thread_priority_error_message(error);
        if self.last_error.is_none() {
            self.last_error = Some(format!("{action} {process_name} ({id}): {message}"));
        }
        self.count += 1;
        action_log.record(
            ActionLogFeature::ThreadPriority,
            Some(id),
            process_name.to_owned(),
            ActionLogAction::Fail,
            ActionLogResult::Failed,
            message,
        );
    }
}

impl Drop for ThreadPriorityManager {
    fn drop(&mut self) {
        let mut action_log = ActionLog::new(1);
        self.clear_all(&mut action_log, stringify!(ThreadPriorityManager));
    }
}

struct ThreadHandle(WinHandle);

impl ThreadHandle {
    fn open(thread_id: u32) -> Result<Self, ThreadPriorityError> {
        // SAFETY: thread_id came from the current thread snapshot and no inherited handle is
        // requested.
        let handle = unsafe {
            OpenThread(
                THREAD_QUERY_INFORMATION | THREAD_SET_INFORMATION,
                0,
                thread_id,
            )
        };
        if !handle.is_null() {
            Ok(Self(WinHandle::new(handle)))
        } else {
            Err(open_thread_error(thread_id, last_error()))
        }
    }

    fn priority(&self) -> Result<i32, ThreadPriorityError> {
        // SAFETY: self owns a live thread handle.
        let priority = unsafe { GetThreadPriority(self.0.raw()) };
        if priority != THREAD_PRIORITY_ERROR_RETURN {
            Ok(priority)
        } else {
            Err(ThreadPriorityError::Failed(format!(
                "GetThreadPriority failed with error {}.",
                last_error()
            )))
        }
    }

    fn creation_time(&self) -> Result<u64, ThreadPriorityError> {
        let mut creation = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        // SAFETY: self owns a live thread handle and every FILETIME output is writable for the
        // duration of the call.
        if unsafe {
            GetThreadTimes(
                self.0.raw(),
                &mut creation,
                &mut exit,
                &mut kernel,
                &mut user,
            )
        } != 0
        {
            Ok(filetime_to_u64(creation))
        } else {
            Err(open_thread_error(0, last_error()))
        }
    }

    fn set_priority(&self, priority: i32) -> Result<(), ThreadPriorityError> {
        // SAFETY: self owns a live thread handle and priority is selected from documented Win32
        // thread-priority constants.
        if unsafe { SetThreadPriority(self.0.raw(), priority) } != 0 {
            Ok(())
        } else {
            Err(ThreadPriorityError::Failed(format!(
                "SetThreadPriority failed with error {}.",
                last_error()
            )))
        }
    }
}

fn process_thread_ids(process_id: u32) -> Result<Vec<u32>, ThreadPriorityError> {
    // SAFETY: TH32CS_SNAPTHREAD ignores the process id argument and returns an owned handle.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPTHREAD, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return Err(ThreadPriorityError::Failed(format!(
            "CreateToolhelp32Snapshot failed with error {}.",
            last_error()
        )));
    }
    let _snapshot = WinHandle::new(snapshot);
    let mut entry = THREADENTRY32 {
        dwSize: size_of::<THREADENTRY32>() as u32,
        ..THREADENTRY32::default()
    };
    let mut ids = Vec::new();
    // SAFETY: snapshot is live and entry declares its size and remains writable.
    let mut ok = unsafe { Thread32First(snapshot, &mut entry) };
    while ok != 0 {
        if entry.th32OwnerProcessID == process_id {
            ids.push(entry.th32ThreadID);
        }
        entry.dwSize = size_of::<THREADENTRY32>() as u32;
        // SAFETY: snapshot remains live and entry remains writable for the next record.
        ok = unsafe { Thread32Next(snapshot, &mut entry) };
    }
    let error = last_error();
    if error != ERROR_NO_MORE_FILES {
        return Err(ThreadPriorityError::Failed(
            std::io::Error::from_raw_os_error(error as i32).to_string(),
        ));
    }
    Ok(ids)
}

fn restore_thread(
    thread_id: u32,
    thread_state: &AdjustedThread,
) -> Result<(), ThreadPriorityError> {
    let thread = ThreadHandle::open(thread_id)?;
    if thread.creation_time()? != thread_state.creation_time {
        return Err(ThreadPriorityError::ProcessExited);
    }
    thread.set_priority(thread_state.previous_priority)?;
    Ok(())
}

fn open_thread_error(thread_id: u32, error: u32) -> ThreadPriorityError {
    match error {
        ERROR_ACCESS_DENIED => ThreadPriorityError::AccessDenied,
        ERROR_INVALID_PARAMETER => ThreadPriorityError::ProcessExited,
        _ => ThreadPriorityError::Failed(format!(
            "OpenThread({thread_id}) failed with error {error}."
        )),
    }
}

fn thread_priority_value(priority: ProcessThreadPrioritySetting) -> Option<i32> {
    match priority {
        ProcessThreadPrioritySetting::Default | ProcessThreadPrioritySetting::Auto => None,
        ProcessThreadPrioritySetting::TimeCritical => Some(THREAD_PRIORITY_TIME_CRITICAL),
        ProcessThreadPrioritySetting::Highest => Some(THREAD_PRIORITY_HIGHEST),
        ProcessThreadPrioritySetting::AboveNormal => Some(THREAD_PRIORITY_ABOVE_NORMAL),
        ProcessThreadPrioritySetting::Normal => Some(THREAD_PRIORITY_NORMAL),
        ProcessThreadPrioritySetting::BelowNormal => Some(THREAD_PRIORITY_BELOW_NORMAL),
        ProcessThreadPrioritySetting::Lowest => Some(THREAD_PRIORITY_LOWEST),
        ProcessThreadPrioritySetting::Idle => Some(THREAD_PRIORITY_IDLE),
    }
}

fn thread_priority_label(priority: i32) -> &'static str {
    match priority {
        THREAD_PRIORITY_TIME_CRITICAL => "Time Critical",
        THREAD_PRIORITY_HIGHEST => "Highest",
        THREAD_PRIORITY_ABOVE_NORMAL => "Above Normal",
        THREAD_PRIORITY_NORMAL => "Normal",
        THREAD_PRIORITY_BELOW_NORMAL => "Below Normal",
        THREAD_PRIORITY_LOWEST => "Lowest",
        THREAD_PRIORITY_IDLE => "Idle",
        _ => "Unknown",
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

fn thread_priority_error_message(error: ThreadPriorityError) -> String {
    match error {
        ThreadPriorityError::AccessDenied => "Access denied.".to_owned(),
        ThreadPriorityError::ProcessExited => "Process exited.".to_owned(),
        ThreadPriorityError::Failed(message) => message,
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
    fn thread_priority_mapping_uses_thread_offsets() {
        assert_eq!(
            thread_priority_value(ProcessThreadPrioritySetting::Default),
            None
        );
        assert_eq!(
            thread_priority_value(ProcessThreadPrioritySetting::BelowNormal),
            Some(THREAD_PRIORITY_BELOW_NORMAL)
        );
        assert_eq!(
            thread_priority_value(ProcessThreadPrioritySetting::TimeCritical),
            Some(THREAD_PRIORITY_TIME_CRITICAL)
        );
    }
}
