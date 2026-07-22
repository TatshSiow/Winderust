use std::collections::{BTreeMap, BTreeSet};

use windows_sys::Win32::{
    Foundation::{ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER},
    System::Threading::{
        GetCurrentProcessId, GetProcessPriorityBoost, OpenProcess, SetProcessPriorityBoost,
        PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_INFORMATION,
    },
};

use crate::win_util::{last_error, WinHandle};

use crate::{
    action_log::{ActionLog, ActionLogAction, ActionLogFeature, ActionLogResult},
    config::{DynamicPriorityBoostSettings, ProcessDynamicPriorityBoostSetting},
    foreground::{
        is_foreground_process, list_processes, process_failure_key, process_names_by_id,
        process_session_id, same_process_name, unique_app_names, CORE_BUILT_IN_PROCESS_EXCLUSIONS,
    },
    rules::{execution_failure_suppression_threshold, ExecutionFailureTracker},
};

const BUILT_IN_EXCLUSIONS: &[&str] = CORE_BUILT_IN_PROCESS_EXCLUSIONS;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DynamicPriorityBoostSnapshot {
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
pub struct DynamicPriorityBoostManager {
    adjusted: BTreeMap<u32, AdjustedProcess>,
    failure_suppression: ExecutionFailureTracker,
}

#[derive(Clone)]
struct AdjustedProcess {
    process_name: String,
    creation_time: u64,
    previous_disabled: bool,
    applied_disabled: bool,
}

#[derive(Debug)]
enum DynamicPriorityBoostError {
    AccessDenied,
    ProcessExited,
    Failed(String),
}

impl DynamicPriorityBoostManager {
    pub fn update(
        &mut self,
        settings: &DynamicPriorityBoostSettings,
        automation_enabled: bool,
        foreground_process_id: Option<u32>,
        action_log: &mut ActionLog,
    ) -> DynamicPriorityBoostSnapshot {
        if !automation_enabled {
            let failures = self.clear_all(action_log, "automation disabled");
            self.failure_suppression.clear();
            return DynamicPriorityBoostSnapshot {
                enabled: false,
                failed_processes: failures.count,
                message: "Automation disabled.".to_owned(),
                last_error: failures.last_error,
                ..Default::default()
            };
        }

        if !settings.enabled {
            let failures = self.clear_all(action_log, "dynamic priority boost defaults disabled");
            self.failure_suppression.clear();
            return DynamicPriorityBoostSnapshot {
                enabled: false,
                failed_processes: failures.count,
                message: "Dynamic priority boost defaults disabled.".to_owned(),
                last_error: failures.last_error,
                ..Default::default()
            };
        }

        let foreground_sensitive = settings.foreground_detection_enabled
            && settings.foreground_boost != settings.background_boost;
        if foreground_sensitive && foreground_process_id.is_none() {
            let failures = self.clear_all(action_log, "foreground app is unknown");
            return DynamicPriorityBoostSnapshot {
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
            return DynamicPriorityBoostSnapshot {
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
                return DynamicPriorityBoostSnapshot {
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
            let boost = match settings.override_for(&process.name, foreground) {
                Some(Some(ProcessDynamicPriorityBoostSetting::Auto)) if foreground => {
                    settings.foreground_boost
                }
                Some(Some(ProcessDynamicPriorityBoostSetting::Auto)) => settings.background_boost,
                Some(Some(boost)) => boost,
                Some(None) => continue,
                None if foreground => settings.foreground_boost,
                None => settings.background_boost,
            };
            if let Some(disabled) = boost.disabled_flag() {
                target_processes.insert(process.id, (process.name, disabled));
            }
        }

        let target_ids = target_processes.keys().copied().collect::<BTreeSet<_>>();
        let active_target_names = target_processes
            .values()
            .map(|(name, _)| process_failure_key(name))
            .collect::<BTreeSet<_>>();
        self.failure_suppression.retain_keys(&active_target_names);

        let mut failures = self.release_non_targets(
            &target_ids,
            &current_process_names,
            action_log,
            "process is excluded or no longer matches dynamic priority boost defaults",
        );
        let mut skipped_processes = 0;
        let mut applied_processes = 0;
        let mut auto_excluded_processes = BTreeSet::new();

        for (process_id, (process_name, disabled)) in target_processes {
            if self.is_process_suppressed(
                process_id,
                &process_name,
                action_log,
                &mut auto_excluded_processes,
            ) {
                skipped_processes += 1;
                continue;
            }

            match self.apply_process(process_id, process_name.clone(), disabled) {
                Ok(ApplyOutcome::Applied { loggable }) => {
                    if loggable {
                        applied_processes += 1;
                    }
                    self.clear_process_failure(&process_name);
                }
                Ok(ApplyOutcome::AlreadyApplied) => {
                    self.clear_process_failure(&process_name);
                }
                Err(DynamicPriorityBoostError::ProcessExited) => skipped_processes += 1,
                Err(DynamicPriorityBoostError::AccessDenied) => {
                    skipped_processes += 1;
                    self.record_process_failure(&process_name);
                    action_log.record(
                        ActionLogFeature::DynamicPriorityBoost,
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
                ActionLogFeature::DynamicPriorityBoost,
                None,
                "Dynamic Priority Boost",
                ActionLogAction::Apply,
                ActionLogResult::Applied,
                format!(
                    "Applied dynamic priority boost defaults to {applied_processes} process(es)."
                ),
            );
        }

        DynamicPriorityBoostSnapshot {
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
            message: "Dynamic priority boost defaults active.".to_owned(),
            last_error: failures.last_error,
        }
    }

    fn apply_process(
        &mut self,
        process_id: u32,
        process_name: String,
        disabled: bool,
    ) -> Result<ApplyOutcome, DynamicPriorityBoostError> {
        let process = ProcessHandle::open(process_id)?;
        let creation_time = process
            .0
            .process_creation_time()
            .ok_or(DynamicPriorityBoostError::ProcessExited)?;
        let reusable_existing = self.adjusted.get(&process_id).filter(|adjusted| {
            adjusted.creation_time == creation_time
                && same_process_name(&adjusted.process_name, &process_name)
        });
        let current_disabled = process.dynamic_priority_boost_disabled()?;

        if reusable_existing.is_some_and(|adjusted| {
            adjusted.applied_disabled == disabled && current_disabled == disabled
        }) {
            return Ok(ApplyOutcome::AlreadyApplied);
        }

        if current_disabled != disabled {
            process.set_dynamic_priority_boost_disabled(disabled)?;
            let refreshed_disabled = process.dynamic_priority_boost_disabled()?;
            if refreshed_disabled != disabled {
                return Err(DynamicPriorityBoostError::Failed(
                    "Dynamic priority boost did not change after request.".to_owned(),
                ));
            }
        }

        let previous_disabled = reusable_existing
            .map(|adjusted| adjusted.previous_disabled)
            .unwrap_or(current_disabled);
        self.adjusted.insert(
            process_id,
            AdjustedProcess {
                process_name,
                creation_time,
                previous_disabled,
                applied_disabled: disabled,
            },
        );
        Ok(ApplyOutcome::Applied {
            loggable: current_disabled != disabled,
        })
    }

    fn release_non_targets(
        &mut self,
        target_ids: &BTreeSet<u32>,
        current_process_names: &BTreeMap<u32, String>,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> DynamicPriorityBoostFailures {
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

    fn clear_all(
        &mut self,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> DynamicPriorityBoostFailures {
        let process_ids = self.adjusted.keys().copied().collect::<Vec<_>>();
        self.release_processes(&process_ids, None, action_log, reason)
    }

    fn release_processes(
        &mut self,
        process_ids: &[u32],
        current_process_names: Option<&BTreeMap<u32, String>>,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> DynamicPriorityBoostFailures {
        let mut failures = DynamicPriorityBoostFailures::default();
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
                Err(DynamicPriorityBoostError::ProcessExited) => {
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
                ActionLogFeature::DynamicPriorityBoost,
                None,
                "Dynamic Priority Boost",
                ActionLogAction::Restore,
                ActionLogResult::Restored,
                format!("Restored dynamic priority boost for {restored_processes} process(es): {reason}."),
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
                ActionLogFeature::DynamicPriorityBoost,
                Some(process_id),
                process_name.to_owned(),
                ActionLogAction::Skip,
                ActionLogResult::Skipped,
                format!(
                    "Stopped retrying Dynamic Priority Boost after {} failed attempts.",
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

impl Drop for DynamicPriorityBoostManager {
    fn drop(&mut self) {
        let mut action_log = ActionLog::new(1);
        self.clear_all(&mut action_log, stringify!(DynamicPriorityBoostManager));
    }
}

enum ApplyOutcome {
    Applied { loggable: bool },
    AlreadyApplied,
}

#[derive(Default)]
struct DynamicPriorityBoostFailures {
    count: usize,
    last_error: Option<String>,
}

impl DynamicPriorityBoostFailures {
    fn record(
        &mut self,
        action: &str,
        process_id: u32,
        process_name: &str,
        error: DynamicPriorityBoostError,
        action_log: &mut ActionLog,
    ) {
        if matches!(&error, DynamicPriorityBoostError::ProcessExited) {
            return;
        }
        let message = dynamic_priority_boost_error_message(error);
        if self.last_error.is_none() {
            self.last_error = Some(format!("{action} {process_name} ({process_id}): {message}"));
        }
        self.count += 1;
        action_log.record(
            ActionLogFeature::DynamicPriorityBoost,
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
    fn open(process_id: u32) -> Result<Self, DynamicPriorityBoostError> {
        // SAFETY: process_id came from the current process snapshot and no inherited handle is
        // requested.
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

    fn dynamic_priority_boost_disabled(&self) -> Result<bool, DynamicPriorityBoostError> {
        let mut disabled = 0_i32;
        // SAFETY: self owns a live process handle and disabled is writable for the call.
        if unsafe { GetProcessPriorityBoost(self.0.raw(), &mut disabled) } != 0 {
            Ok(disabled != 0)
        } else {
            Err(DynamicPriorityBoostError::Failed(format!(
                "GetProcessPriorityBoost failed with error {}.",
                last_error()
            )))
        }
    }

    fn set_dynamic_priority_boost_disabled(
        &self,
        disabled: bool,
    ) -> Result<(), DynamicPriorityBoostError> {
        // SAFETY: self owns a live process handle and disabled is converted to the documented BOOL
        // representation.
        if unsafe { SetProcessPriorityBoost(self.0.raw(), i32::from(disabled)) } != 0 {
            Ok(())
        } else {
            Err(DynamicPriorityBoostError::Failed(format!(
                "SetProcessPriorityBoost failed with error {}.",
                last_error()
            )))
        }
    }
}

fn restore_process(
    process_id: u32,
    process_state: &AdjustedProcess,
) -> Result<(), DynamicPriorityBoostError> {
    let process = ProcessHandle::open(process_id)?;
    if process.0.process_creation_time() != Some(process_state.creation_time) {
        return Err(DynamicPriorityBoostError::ProcessExited);
    }
    process.set_dynamic_priority_boost_disabled(process_state.previous_disabled)?;
    let refreshed_disabled = process.dynamic_priority_boost_disabled()?;
    if refreshed_disabled == process_state.previous_disabled {
        Ok(())
    } else {
        Err(DynamicPriorityBoostError::Failed(
            "Dynamic priority boost did not restore after request.".to_owned(),
        ))
    }
}

fn open_process_error(process_id: u32, error: u32) -> DynamicPriorityBoostError {
    match error {
        ERROR_ACCESS_DENIED => DynamicPriorityBoostError::AccessDenied,
        ERROR_INVALID_PARAMETER => DynamicPriorityBoostError::ProcessExited,
        _ => DynamicPriorityBoostError::Failed(format!(
            "OpenProcess({process_id}) failed with error {error}."
        )),
    }
}

fn dynamic_priority_boost_error_message(error: DynamicPriorityBoostError) -> String {
    match error {
        DynamicPriorityBoostError::AccessDenied => "Access denied.".to_owned(),
        DynamicPriorityBoostError::ProcessExited => "Process exited.".to_owned(),
        DynamicPriorityBoostError::Failed(message) => message,
    }
}

pub fn is_builtin_excluded(process_name: &str) -> bool {
    BUILT_IN_EXCLUSIONS
        .iter()
        .any(|excluded| same_process_name(excluded, process_name))
}
