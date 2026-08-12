use std::collections::{BTreeMap, BTreeSet};

use windows_sys::Win32::System::Threading::GetCurrentProcessId;

use crate::{
    action_log::{ActionLog, ActionLogFeature, ActionLogResult},
    config::{IoPrioritySettings, ProcessIoPrioritySetting},
    control::{
        io_priority::{
            IoPriorityApplyOutcome, IoPriorityClaim, IoPriorityController, IoPriorityPreservation,
            IoPriorityReleaseSummary,
        },
        process::{ControlOwner, ProcessControlError, ProcessControlTarget, ProcessTargetKey},
    },
    foreground::{
        contains_process_name, is_foreground_process, process_count_label, process_executable_path,
        process_failure_key, process_session_id, unique_app_names, ProtectedProcesses,
        CORE_BUILT_IN_PROCESS_EXCLUSIONS,
    },
    rules::{execution_failure_suppression_threshold, ExecutionFailureTracker},
    runtime::observations::CycleObservations,
};

use super::PriorityProcessTier;

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
    failure_suppression: ExecutionFailureTracker,
}

impl IoPriorityManager {
    #[expect(
        clippy::too_many_arguments,
        reason = "the pass-local controller and observations are clearer here than an argument bundle"
    )]
    pub fn update(
        &mut self,
        controller: &mut IoPriorityController,
        owner: ControlOwner,
        settings: &IoPrioritySettings,
        automation_enabled: bool,
        allow_cross_session_process_control: bool,
        foreground_process_id: Option<u32>,
        observations: &mut CycleObservations,
        action_log: &mut ActionLog,
    ) -> IoPrioritySnapshot {
        if !automation_enabled {
            let failures = self.clear_all(controller, action_log, "automation disabled");
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
            let failures = self.clear_all(controller, action_log, "I/O priority defaults disabled");
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
            let failures = self.clear_all(controller, action_log, "foreground app is unknown");
            return IoPrioritySnapshot {
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
            let failures =
                self.clear_all(controller, action_log, "current Windows session is unknown");
            return IoPrioritySnapshot {
                enabled: true,
                failed_processes: failures.count,
                message: "Paused: current Windows session is unknown.".to_owned(),
                last_error: failures.last_error,
                ..Default::default()
            };
        };

        let processes = match observations.processes_with_paths() {
            Ok(processes) => processes,
            Err(err) => {
                let failures = self.clear_all(controller, action_log, "process list unavailable");
                return IoPrioritySnapshot {
                    enabled: true,
                    failed_processes: failures.count,
                    message: err,
                    last_error: failures.last_error,
                    ..Default::default()
                };
            }
        };

        let visible_processes = if settings.visible_window_detection_enabled {
            let Ok(process_ids) = observations.visible_window_process_ids() else {
                let failures =
                    self.clear_all(controller, action_log, "visible windows are unavailable");
                return IoPrioritySnapshot {
                    enabled: true,
                    failed_processes: failures.count,
                    message: "Paused: visible windows are unavailable.".to_owned(),
                    last_error: failures.last_error,
                    ..Default::default()
                };
            };
            ProtectedProcesses::capture(processes.as_ref(), false, None, process_ids)
        } else {
            ProtectedProcesses::default()
        };

        let scanned_processes = processes.len();
        let foreground_executable_path = if settings.foreground_detection_enabled {
            foreground_process_id.and_then(|id| {
                processes
                    .iter()
                    .find(|process| process.id == id)
                    .and_then(process_executable_path)
            })
        } else {
            None
        };

        let mut target_processes = BTreeMap::new();
        for process in processes.iter() {
            if process.id == 0
                || process.is_critical != Some(false)
                || !process.can_set_information
                || process.id == current_process_id
                || (!allow_cross_session_process_control
                    && process_session_id(process.id) != Some(current_session_id))
                || is_builtin_excluded(&process.name)
            {
                continue;
            }

            let Some(executable_path) = process_executable_path(process) else {
                continue;
            };
            let Some(creation_time) = process.creation_time else {
                continue;
            };
            let foreground = settings.foreground_detection_enabled
                && is_foreground_process(
                    process.id,
                    &executable_path,
                    foreground_process_id,
                    foreground_executable_path.as_deref(),
                );
            let visible_window = !foreground
                && settings.visible_window_detection_enabled
                && visible_processes.contains(process.id, &executable_path);
            let tier = PriorityProcessTier::from_flags(foreground, visible_window);
            let configured_override =
                settings.override_for(executable_path.to_string_lossy().as_ref(), foreground);
            let default_priority = tier.select(
                settings.foreground_priority,
                settings.visible_window_priority,
                settings.background_priority,
            );
            let priority = match configured_override {
                Some(Some(ProcessIoPrioritySetting::Auto)) => default_priority,
                Some(Some(priority)) => priority,
                Some(None) => continue,
                None => default_priority,
            };
            if let Some(priority) = priority.priority() {
                target_processes.insert(
                    process.id,
                    (
                        process.name.clone(),
                        executable_path,
                        priority,
                        tier,
                        creation_time,
                    ),
                );
            }
        }

        let active_target_names = target_processes
            .values()
            .map(|(_name, path, _, _, _)| process_failure_key(&path.to_string_lossy()))
            .collect::<BTreeSet<_>>();
        self.failure_suppression.retain_keys(&active_target_names);
        let active_targets = target_processes
            .iter()
            .map(
                |(process_id, (process_name, executable_path, _, _, creation_time))| {
                    ProcessControlTarget::automatic(
                        *process_id,
                        process_name.clone(),
                        executable_path.clone(),
                        *creation_time,
                    )
                    .key()
                },
            )
            .collect::<BTreeSet<ProcessTargetKey>>();

        let mut skipped_processes = 0;
        let mut auto_excluded_processes = BTreeSet::new();
        let mut claims = Vec::new();
        for (process_id, (process_name, executable_path, priority, tier, creation_time)) in
            target_processes
        {
            let executable_path_text = executable_path.to_string_lossy().into_owned();
            if self.is_process_suppressed(
                process_id,
                &process_name,
                &executable_path_text,
                action_log,
                &mut auto_excluded_processes,
            ) {
                skipped_processes += 1;
                continue;
            }
            claims.push(IoPriorityClaim {
                target: ProcessControlTarget::automatic(
                    process_id,
                    process_name,
                    executable_path,
                    creation_time,
                ),
                owner,
                priority,
                preservation: tier_preservation(settings, tier),
            });
        }

        let mut failures = self.release_non_targets(
            controller,
            &active_targets,
            action_log,
            "process is excluded or no longer matches I/O priority defaults",
        );
        let mut applied_processes = 0;
        for claim in claims {
            let process_id = claim.target.id;
            let process_name = claim.target.name.clone();
            let executable_path = claim.target.executable_path.to_string_lossy().into_owned();
            match controller.apply_policy_claim(claim, allow_cross_session_process_control) {
                Ok(IoPriorityApplyOutcome::Applied) => {
                    applied_processes += 1;
                    self.failure_suppression
                        .clear_process_failure(&executable_path);
                }
                Ok(IoPriorityApplyOutcome::Unchanged) => {
                    self.failure_suppression
                        .clear_process_failure(&executable_path);
                }
                Ok(IoPriorityApplyOutcome::Preserved) => {
                    skipped_processes += 1;
                    self.failure_suppression
                        .clear_process_failure(&executable_path);
                }
                Err(ProcessControlError::ProcessExited) => skipped_processes += 1,
                Err(ProcessControlError::AccessDenied(_)) => {
                    skipped_processes += 1;
                    self.failure_suppression
                        .suppress_process_failure(&executable_path);
                    action_log.record(
                        ActionLogFeature::IoPriority,
                        Some(process_id),
                        process_name,
                        ActionLogResult::Skipped,
                        "Skipped because the process could not be opened.",
                    );
                }
                Err(error) => {
                    self.failure_suppression
                        .record_process_failure(&executable_path);
                    failures.record("Apply", process_id, &process_name, error, action_log);
                }
            }
        }
        if applied_processes > 0 {
            action_log.record(
                ActionLogFeature::IoPriority,
                None,
                "I/O Priority",
                ActionLogResult::Applied,
                format!(
                    "Applied I/O priority to {}.",
                    process_count_label(applied_processes)
                ),
            );
        }

        let adjusted_apps = controller.policy_managed_process_names();
        IoPrioritySnapshot {
            enabled: true,
            scanned_processes,
            adjusted_processes: adjusted_apps.len(),
            skipped_processes,
            failed_processes: failures.count,
            adjusted_apps: unique_app_names(adjusted_apps.iter().map(String::as_str)),
            auto_excluded_processes: auto_excluded_processes.into_iter().collect(),
            message: "I/O priority defaults active.".to_owned(),
            last_error: failures.last_error,
        }
    }

    fn release_non_targets(
        &mut self,
        controller: &mut IoPriorityController,
        active_targets: &BTreeSet<ProcessTargetKey>,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> IoPriorityFailures {
        let summary = controller.release_policy_except(active_targets);
        self.record_release_summary(summary, action_log, reason)
    }

    fn clear_all(
        &mut self,
        controller: &mut IoPriorityController,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> IoPriorityFailures {
        let summary = controller.release_all_policy();
        self.record_release_summary(summary, action_log, reason)
    }

    fn record_release_summary(
        &mut self,
        summary: IoPriorityReleaseSummary,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> IoPriorityFailures {
        let mut failures = IoPriorityFailures::default();
        for failure in summary.failures {
            self.failure_suppression
                .record_process_failure(&failure.executable_path);
            failures.record(
                "Restore",
                failure.process_id,
                &failure.process_name,
                failure.error,
                action_log,
            );
        }
        if summary.restored_processes > 0 {
            action_log.record(
                ActionLogFeature::IoPriority,
                None,
                "I/O Priority",
                ActionLogResult::Restored,
                format!(
                    "Restored previous I/O priority for {}: {reason}.",
                    process_count_label(summary.restored_processes)
                ),
            );
        }
        failures
    }

    fn is_process_suppressed(
        &mut self,
        process_id: u32,
        process_name: &str,
        executable_path: &str,
        action_log: &mut ActionLog,
        auto_excluded_processes: &mut BTreeSet<String>,
    ) -> bool {
        let suppression = self
            .failure_suppression
            .process_suppression(executable_path);
        if !suppression.suppressed {
            return false;
        }
        if suppression.newly_suppressed {
            auto_excluded_processes.insert(executable_path.to_owned());
            action_log.record(
                ActionLogFeature::IoPriority,
                Some(process_id),
                process_name.to_owned(),
                ActionLogResult::Skipped,
                format!(
                    "Stopped retrying I/O Priority after {} failed attempts.",
                    execution_failure_suppression_threshold(),
                ),
            );
        }
        true
    }
}

fn tier_preservation(
    settings: &IoPrioritySettings,
    tier: PriorityProcessTier,
) -> IoPriorityPreservation {
    match tier {
        PriorityProcessTier::Foreground if settings.preserve_foreground_priority => {
            IoPriorityPreservation::PreserveHigher
        }
        PriorityProcessTier::VisibleWindow if settings.preserve_visible_window_priority => {
            IoPriorityPreservation::PreserveHigher
        }
        PriorityProcessTier::Background if settings.preserve_background_priority => {
            IoPriorityPreservation::PreserveLower
        }
        _ => IoPriorityPreservation::Exact,
    }
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
        error: ProcessControlError,
        action_log: &mut ActionLog,
    ) {
        let message = error.to_string();
        if self.last_error.is_none() {
            self.last_error = Some(format!("{action} {process_name} ({process_id}): {message}"));
        }
        self.count += 1;
        action_log.record(
            ActionLogFeature::IoPriority,
            Some(process_id),
            process_name.to_owned(),
            ActionLogResult::Failed,
            message,
        );
    }
}

pub fn is_builtin_excluded(process_name: &str) -> bool {
    contains_process_name(CORE_BUILT_IN_PROCESS_EXCLUSIONS, process_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_failures_emit_one_io_auto_exclusion_and_success_resets_it() {
        let mut manager = IoPriorityManager::default();
        let mut log = ActionLog::new(8);
        let mut auto_excluded = BTreeSet::new();
        let path = r"C:\Apps\app.exe";

        manager.failure_suppression.record_process_failure(path);
        manager
            .failure_suppression
            .record_process_failure(r"C:/Apps/app.exe");
        assert!(!manager.is_process_suppressed(42, "app.exe", path, &mut log, &mut auto_excluded,));
        manager.failure_suppression.record_process_failure(path);
        assert!(manager.is_process_suppressed(42, "app.exe", path, &mut log, &mut auto_excluded,));
        assert!(manager.is_process_suppressed(
            43,
            "app.exe",
            r"C:/Apps/app.exe",
            &mut log,
            &mut auto_excluded,
        ));
        assert_eq!(auto_excluded, BTreeSet::from([path.to_owned()]));
        assert_eq!(log.entries().len(), 1);
        assert_eq!(log.entries()[0].feature, ActionLogFeature::IoPriority);

        manager.failure_suppression.clear_process_failure(path);
        auto_excluded.clear();
        assert!(!manager.is_process_suppressed(42, "app.exe", path, &mut log, &mut auto_excluded,));
    }

    #[test]
    fn focus_visible_and_background_preservation_directions_are_explicit() {
        let mut settings = IoPrioritySettings::default();
        assert_eq!(
            tier_preservation(&settings, PriorityProcessTier::Foreground),
            IoPriorityPreservation::PreserveHigher
        );
        assert_eq!(
            tier_preservation(&settings, PriorityProcessTier::VisibleWindow),
            IoPriorityPreservation::PreserveHigher
        );
        assert_eq!(
            tier_preservation(&settings, PriorityProcessTier::Background),
            IoPriorityPreservation::PreserveLower
        );
        settings.preserve_visible_window_priority = false;
        assert_eq!(
            tier_preservation(&settings, PriorityProcessTier::VisibleWindow),
            IoPriorityPreservation::Exact
        );
    }
}
