use std::{collections::BTreeSet, path::PathBuf};

use windows_sys::Win32::System::Threading::GetCurrentProcessId;

use crate::{
    action_log::{ActionLog, ActionLogFeature, ActionLogResult},
    config::{ProcessPrioritySetting, ProcessPrioritySettings},
    control::{
        priority_efficiency::{
            PriorityClassClaim, PriorityClassPreservation, PriorityClassValue,
            PriorityEfficiencyController, PriorityEfficiencyReleaseSummary,
            ProcessPropertyApplyOutcome,
        },
        process::{ControlOwner, ProcessControlError, ProcessControlTarget},
    },
    foreground::{
        is_foreground_process, process_executable_path, process_failure_key, process_session_id,
        unique_app_names, ProtectedProcesses, CORE_BUILT_IN_PROCESS_EXCLUSIONS,
    },
    rules::{execution_failure_suppression_threshold, ExecutionFailureTracker},
    runtime::observations::CycleObservations,
};

use super::PriorityProcessTier;

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
    failure_suppression: ExecutionFailureTracker,
}

struct ProcessPriorityTarget {
    process_id: u32,
    process_name: String,
    executable_path: String,
    creation_time: u64,
    priority: PriorityClassValue,
    tier: PriorityProcessTier,
}

impl ProcessPriorityManager {
    #[expect(
        clippy::too_many_arguments,
        reason = "the policy boundary receives shared observations, exclusions, and its controller"
    )]
    pub(crate) fn update(
        &mut self,
        controller: &mut PriorityEfficiencyController,
        settings: &ProcessPrioritySettings,
        automation_enabled: bool,
        allow_cross_session_process_control: bool,
        foreground_process_id: Option<u32>,
        excluded_process_ids: &BTreeSet<u32>,
        observations: &mut CycleObservations,
        action_log: &mut ActionLog,
    ) -> ProcessPrioritySnapshot {
        if !automation_enabled {
            return self.disabled_snapshot(controller, action_log, "automation disabled");
        }
        if !settings.enabled {
            return self.disabled_snapshot(
                controller,
                action_log,
                "Process priority defaults disabled",
            );
        }

        let foreground_sensitive = settings.foreground_detection_enabled
            && settings.foreground_priority != settings.background_priority;
        if foreground_sensitive && foreground_process_id.is_none() {
            return self.paused_snapshot(
                controller,
                action_log,
                "foreground app is unknown",
                "Paused: foreground app is unknown.",
            );
        }

        // SAFETY: GetCurrentProcessId takes no arguments and has no caller requirements.
        let current_process_id = unsafe { GetCurrentProcessId() };
        let Some(current_session_id) = process_session_id(current_process_id) else {
            return self.paused_snapshot(
                controller,
                action_log,
                "current Windows session is unknown",
                "Paused: current Windows session is unknown.",
            );
        };

        let processes = match observations.processes() {
            Ok(processes) => processes,
            Err(error) => {
                return self.paused_snapshot(
                    controller,
                    action_log,
                    "process list unavailable",
                    &error,
                );
            }
        };
        let scanned_processes = processes.len();
        let visible_processes = if settings.visible_window_detection_enabled {
            let Ok(process_ids) = observations.visible_window_process_ids() else {
                return self.paused_snapshot(
                    controller,
                    action_log,
                    "visible windows are unavailable",
                    "Paused: visible windows are unavailable.",
                );
            };
            ProtectedProcesses::capture(processes.as_ref(), false, None, process_ids)
        } else {
            ProtectedProcesses::default()
        };
        let foreground_executable_path = settings.foreground_detection_enabled.then(|| {
            foreground_process_id.and_then(|id| {
                processes
                    .iter()
                    .find(|process| process.id == id)
                    .and_then(process_executable_path)
            })
        });
        let foreground_executable_path = foreground_executable_path.flatten();

        let targets = processes
            .iter()
            .filter_map(|process| {
                if process.id == 0
                    || process.id == current_process_id
                    || process.is_critical != Some(false)
                    || !process.can_set_information
                    || excluded_process_ids.contains(&process.id)
                    || (!allow_cross_session_process_control
                        && process_session_id(process.id) != Some(current_session_id))
                    || is_builtin_excluded(&process.name)
                {
                    return None;
                }
                let executable_path = process_executable_path(process)?;
                let creation_time = process.creation_time?;
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
                let default_priority = tier.select(
                    settings.foreground_priority,
                    settings.visible_window_priority,
                    settings.background_priority,
                );
                let configured_override = settings.override_for(
                    executable_path.to_string_lossy().as_ref(),
                    foreground,
                    visible_window,
                );
                let priority = match configured_override {
                    Some(Some(priority)) => priority,
                    Some(None) => return None,
                    None => default_priority,
                };
                PriorityClassValue::from_setting(priority).map(|priority| ProcessPriorityTarget {
                    process_id: process.id,
                    process_name: process.name.clone(),
                    executable_path: executable_path.to_string_lossy().into_owned(),
                    creation_time,
                    priority,
                    tier,
                })
            })
            .collect::<Vec<_>>();

        #[cfg(feature = "architecture-diagnostics")]
        crate::architecture_diagnostics::record_process_priority_cycle(
            scanned_processes,
            targets.len(),
        );
        let active_targets = targets
            .iter()
            .map(process_priority_target_key)
            .collect::<BTreeSet<_>>();
        let active_target_names = targets
            .iter()
            .map(|target| process_failure_key(&target.executable_path))
            .collect::<BTreeSet<_>>();
        self.failure_suppression.retain_keys(&active_target_names);

        let mut failures = ProcessPriorityFailures::default();
        self.merge_release_summary(
            controller
                .release_priority_policy_except(ControlOwner::ProcessPriority, &active_targets),
            action_log,
            "process is excluded or no longer matches Process Priority defaults",
            &mut failures,
        );
        let mut skipped_processes = 0;
        let mut applied_processes = 0;
        let mut auto_excluded_processes = BTreeSet::new();

        for target in targets {
            if self.is_process_suppressed(
                target.process_id,
                &target.process_name,
                &target.executable_path,
                action_log,
                &mut auto_excluded_processes,
            ) {
                skipped_processes += 1;
                continue;
            }
            let claim = PriorityClassClaim {
                target: ProcessControlTarget::automatic(
                    target.process_id,
                    target.process_name.clone(),
                    PathBuf::from(&target.executable_path),
                    target.creation_time,
                ),
                owner: ControlOwner::ProcessPriority,
                priority: target.priority,
                preservation: priority_preservation(
                    target.tier,
                    settings.preserve_foreground_priority,
                    settings.preserve_visible_window_priority,
                    settings.preserve_background_priority,
                ),
            };
            match controller.apply_priority_claim(claim, allow_cross_session_process_control) {
                Ok(ProcessPropertyApplyOutcome::Applied) => {
                    #[cfg(feature = "architecture-diagnostics")]
                    crate::architecture_diagnostics::record_process_priority_applied();
                    applied_processes += 1;
                    self.failure_suppression
                        .clear_process_failure(&target.executable_path);
                }
                Ok(
                    ProcessPropertyApplyOutcome::Unchanged | ProcessPropertyApplyOutcome::Shadowed,
                ) => {
                    #[cfg(feature = "architecture-diagnostics")]
                    crate::architecture_diagnostics::record_process_priority_already_applied();
                    self.failure_suppression
                        .clear_process_failure(&target.executable_path);
                }
                Ok(ProcessPropertyApplyOutcome::Preserved) => {
                    #[cfg(feature = "architecture-diagnostics")]
                    crate::architecture_diagnostics::record_process_priority_preserved();
                    skipped_processes += 1;
                    self.failure_suppression
                        .clear_process_failure(&target.executable_path);
                }
                Err(ProcessControlError::ProcessExited) => {
                    #[cfg(feature = "architecture-diagnostics")]
                    crate::architecture_diagnostics::record_process_priority_exit_failure();
                    skipped_processes += 1;
                }
                Err(ProcessControlError::AccessDenied(message)) => {
                    #[cfg(feature = "architecture-diagnostics")]
                    crate::architecture_diagnostics::record_process_priority_access_failure();
                    skipped_processes += 1;
                    self.failure_suppression
                        .suppress_process_failure(&target.executable_path);
                    action_log.record(
                        ActionLogFeature::ProcessPriority,
                        Some(target.process_id),
                        target.process_name,
                        ActionLogResult::Skipped,
                        message,
                    );
                }
                Err(error) => {
                    #[cfg(feature = "architecture-diagnostics")]
                    crate::architecture_diagnostics::record_process_priority_other_failure();
                    self.failure_suppression
                        .record_process_failure(&target.executable_path);
                    failures.record(
                        "Apply",
                        target.process_id,
                        &target.process_name,
                        error,
                        action_log,
                    );
                }
            }
        }
        if applied_processes > 0 {
            action_log.record(
                ActionLogFeature::ProcessPriority,
                None,
                "Process Priority",
                ActionLogResult::Applied,
                format!("Applied process priority defaults to {applied_processes} process(es)."),
            );
        }

        ProcessPrioritySnapshot {
            enabled: true,
            scanned_processes,
            adjusted_processes: controller
                .policy_managed_process_count(ControlOwner::ProcessPriority),
            skipped_processes,
            failed_processes: failures.count,
            adjusted_apps: unique_app_names(
                controller
                    .policy_managed_process_names(ControlOwner::ProcessPriority)
                    .iter()
                    .map(String::as_str),
            ),
            auto_excluded_processes: auto_excluded_processes.into_iter().collect(),
            message: "Process priority defaults active.".to_owned(),
            last_error: failures.last_error,
        }
    }

    fn disabled_snapshot(
        &mut self,
        controller: &mut PriorityEfficiencyController,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> ProcessPrioritySnapshot {
        let mut failures = ProcessPriorityFailures::default();
        self.merge_release_summary(
            controller.release_all_priority_policy(ControlOwner::ProcessPriority),
            action_log,
            reason,
            &mut failures,
        );
        self.failure_suppression.clear();
        ProcessPrioritySnapshot {
            enabled: false,
            failed_processes: failures.count,
            message: if reason == "automation disabled" {
                "Automation disabled.".to_owned()
            } else {
                "Process priority defaults disabled.".to_owned()
            },
            last_error: failures.last_error,
            ..Default::default()
        }
    }

    fn paused_snapshot(
        &mut self,
        controller: &mut PriorityEfficiencyController,
        action_log: &mut ActionLog,
        reason: &str,
        message: &str,
    ) -> ProcessPrioritySnapshot {
        let mut failures = ProcessPriorityFailures::default();
        self.merge_release_summary(
            controller.release_all_priority_policy(ControlOwner::ProcessPriority),
            action_log,
            reason,
            &mut failures,
        );
        ProcessPrioritySnapshot {
            enabled: true,
            failed_processes: failures.count,
            message: message.to_owned(),
            last_error: failures.last_error,
            ..Default::default()
        }
    }

    fn merge_release_summary(
        &mut self,
        summary: PriorityEfficiencyReleaseSummary,
        action_log: &mut ActionLog,
        reason: &str,
        failures: &mut ProcessPriorityFailures,
    ) {
        if summary.restored_processes > 0 {
            action_log.record(
                ActionLogFeature::ProcessPriority,
                None,
                "Process Priority",
                ActionLogResult::Restored,
                format!(
                    "Restored process priority for {} process(es): {reason}.",
                    summary.restored_processes
                ),
            );
        }
        for failure in summary.failures {
            self.failure_suppression
                .record_process_failure(&failure.executable_path);
            failures.record(
                &format!("Restore {}", failure.property),
                failure.process_id,
                &failure.process_name,
                failure.error,
                action_log,
            );
        }
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
                ActionLogFeature::ProcessPriority,
                Some(process_id),
                process_name.to_owned(),
                ActionLogResult::Skipped,
                format!(
                    "Stopped retrying Process Priority after {} failed attempts.",
                    execution_failure_suppression_threshold(),
                ),
            );
        }
        true
    }
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
        error: ProcessControlError,
        action_log: &mut ActionLog,
    ) {
        let message = error.to_string();
        if self.last_error.is_none() {
            self.last_error = Some(format!("{action} {process_name} ({process_id}): {message}"));
        }
        self.count += 1;
        action_log.record(
            ActionLogFeature::ProcessPriority,
            Some(process_id),
            process_name.to_owned(),
            ActionLogResult::Failed,
            message,
        );
    }
}

fn process_priority_target_key(
    target: &ProcessPriorityTarget,
) -> crate::control::process::ProcessTargetKey {
    ProcessControlTarget::automatic(
        target.process_id,
        target.process_name.clone(),
        PathBuf::from(&target.executable_path),
        target.creation_time,
    )
    .key()
}

fn priority_preservation(
    tier: PriorityProcessTier,
    preserve_foreground: bool,
    preserve_visible_window: bool,
    preserve_background: bool,
) -> PriorityClassPreservation {
    match tier {
        PriorityProcessTier::Foreground if preserve_foreground => {
            PriorityClassPreservation::PreserveHigher
        }
        PriorityProcessTier::VisibleWindow if preserve_visible_window => {
            PriorityClassPreservation::PreserveHigher
        }
        PriorityProcessTier::Background if preserve_background => {
            PriorityClassPreservation::PreserveLower
        }
        _ => PriorityClassPreservation::Exact,
    }
}

pub(crate) fn can_apply_once(priority: ProcessPrioritySetting) -> bool {
    matches!(
        priority,
        ProcessPrioritySetting::Idle
            | ProcessPrioritySetting::BelowNormal
            | ProcessPrioritySetting::Normal
            | ProcessPrioritySetting::AboveNormal
    )
}

pub fn is_builtin_excluded(process_name: &str) -> bool {
    crate::foreground::contains_process_name(CORE_BUILT_IN_PROCESS_EXCLUSIONS, process_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quick_apply_only_accepts_safe_concrete_priorities() {
        assert!(can_apply_once(ProcessPrioritySetting::BelowNormal));
        assert!(can_apply_once(ProcessPrioritySetting::Normal));
        assert!(can_apply_once(ProcessPrioritySetting::AboveNormal));
        assert!(!can_apply_once(ProcessPrioritySetting::High));
        assert!(!can_apply_once(ProcessPrioritySetting::Realtime));
    }

    #[test]
    fn repeated_failures_emit_one_process_priority_auto_exclusion_and_success_resets_it() {
        let mut manager = ProcessPriorityManager::default();
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
        assert_eq!(log.entries()[0].feature, ActionLogFeature::ProcessPriority);

        manager.failure_suppression.clear_process_failure(path);
        auto_excluded.clear();
        assert!(!manager.is_process_suppressed(42, "app.exe", path, &mut log, &mut auto_excluded,));
    }
}
