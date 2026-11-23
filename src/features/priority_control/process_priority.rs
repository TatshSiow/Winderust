use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

use windows_sys::Win32::System::Threading::GetCurrentProcessId;

use crate::{
    action_log::{ActionLog, ActionLogFeature, ActionLogResult},
    config::ProcessPrioritySettings,
    control::{
        priority_efficiency::{
            PriorityClassClaim, PriorityClassPreservation, PriorityClassValue,
            PriorityEfficiencyController, PriorityEfficiencyReleaseSummary,
            ProcessPropertyApplyOutcome,
        },
        process::{ControlOwner, ProcessControlError, ProcessControlTarget, ProcessTargetKey},
    },
    foreground::{
        is_foreground_process, process_executable_path, process_failure_key,
        process_list::catalog_process_executable_path, process_session_id, ProtectedProcesses,
        CORE_BUILT_IN_PROCESS_EXCLUSIONS,
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
    pub auto_excluded_processes: Vec<String>,
    pub(crate) observation_paused: bool,
    pub last_error: Option<String>,
}

#[derive(Default)]
pub struct ProcessPriorityManager {
    failure_suppression: ExecutionFailureTracker,
    non_release_failures: ProcessPriorityFailures,
    release_error_reported: bool,
}

struct ProcessPriorityTarget {
    process_id: u32,
    process_name: String,
    executable_path: String,
    creation_time: u64,
    priority: PriorityClassValue,
    preservation: PriorityClassPreservation,
}

impl ProcessPriorityManager {
    #[expect(
        clippy::too_many_arguments,
        reason = "the policy boundary receives shared observations and its controller"
    )]
    pub(crate) fn update(
        &mut self,
        controller: &mut PriorityEfficiencyController,
        settings: &ProcessPrioritySettings,
        automation_enabled: bool,
        allow_cross_session_process_control: bool,
        foreground_process_id: Option<u32>,
        observations: &mut CycleObservations,
        action_log: &mut ActionLog,
    ) -> ProcessPrioritySnapshot {
        self.non_release_failures = ProcessPriorityFailures::default();
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

        let focus_changes_default = settings.foreground_detection_enabled
            && settings.foreground_priority != settings.background_priority;
        if focus_changes_default && foreground_process_id.is_none() {
            return self.pause_for_unknown_focus(controller, action_log);
        }

        // SAFETY: GetCurrentProcessId takes no arguments and has no caller requirements.
        let current_process_id = unsafe { GetCurrentProcessId() };
        let Some(current_session_id) = process_session_id(current_process_id) else {
            return self.paused_snapshot(
                controller,
                action_log,
                "current Windows session is unknown",
                None,
            );
        };

        let processes = match observations.processes() {
            Ok(processes) => processes,
            Err(error) => {
                return self.paused_snapshot(
                    controller,
                    action_log,
                    "process list unavailable",
                    Some(error),
                );
            }
        };
        let foreground_executable_path = foreground_process_id
            .filter(|_| settings.foreground_detection_enabled)
            .and_then(|id| catalog_process_executable_path(&processes, id));
        let foreground_process_id =
            foreground_process_id.filter(|_| foreground_executable_path.is_some());
        let focus_unknown =
            settings.foreground_detection_enabled && foreground_process_id.is_none();
        if focus_unknown && focus_changes_default {
            return self.pause_for_unknown_focus(controller, action_log);
        }

        let scanned_processes = processes.len();
        let visible_processes = if settings.visible_window_detection_enabled {
            let Ok(process_ids) = observations.visible_window_process_ids() else {
                return self.paused_snapshot(
                    controller,
                    action_log,
                    "visible windows are unavailable",
                    None,
                );
            };
            ProtectedProcesses::capture(processes.as_ref(), false, None, process_ids)
        } else {
            ProtectedProcesses::default()
        };
        let mut targets = Vec::new();
        for process in processes.iter() {
            if process.id == 0
                || process.id == current_process_id
                || process.is_critical != Some(false)
                || !process.can_set_information
                || (!allow_cross_session_process_control
                    && process_session_id(process.id) != Some(current_session_id))
                || is_builtin_excluded(&process.name)
            {
                continue;
            }
            let (Some(executable_path), Some(creation_time)) =
                (process_executable_path(process), process.creation_time)
            else {
                continue;
            };
            let foreground = is_foreground_process(
                process.id,
                &executable_path,
                foreground_process_id,
                foreground_executable_path.as_deref(),
            );
            let visible_window = !foreground
                && settings.visible_window_detection_enabled
                && visible_processes.contains(process.id, &executable_path);
            let tier = PriorityProcessTier::from_flags(foreground, visible_window);
            let Some(executable_path) = executable_path.to_str() else {
                continue;
            };
            let policy = priority_policy(settings, executable_path, tier);
            if focus_unknown
                && policy
                    != priority_policy(settings, executable_path, PriorityProcessTier::Foreground)
            {
                return self.pause_for_unknown_focus(controller, action_log);
            }
            let Some((priority, preservation)) = policy else {
                continue;
            };
            targets.push(ProcessPriorityTarget {
                process_id: process.id,
                process_name: process.name.clone(),
                executable_path: executable_path.to_owned(),
                creation_time,
                priority,
                preservation,
            });
        }

        let active_targets = targets
            .iter()
            .map(process_priority_target_key)
            .collect::<BTreeSet<_>>();
        let active_target_names = targets
            .iter()
            .map(|target| process_failure_key(&target.executable_path))
            .collect::<BTreeSet<_>>();
        self.failure_suppression.retain_keys(&active_target_names);

        self.record_policy_release(
            controller
                .release_priority_policy_except(ControlOwner::ProcessPriority, &active_targets),
            action_log,
            "process is excluded or no longer matches Process Priority defaults",
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
                preservation: target.preservation,
            };
            match controller.apply_priority_claim(claim, allow_cross_session_process_control) {
                Ok(ProcessPropertyApplyOutcome::Applied) => {
                    applied_processes += 1;
                    self.failure_suppression
                        .clear_process_failure(&target.executable_path);
                }
                Ok(
                    ProcessPropertyApplyOutcome::Unchanged | ProcessPropertyApplyOutcome::Shadowed,
                ) => {
                    self.failure_suppression
                        .clear_process_failure(&target.executable_path);
                }
                Ok(ProcessPropertyApplyOutcome::Preserved) => {
                    skipped_processes += 1;
                    self.failure_suppression
                        .clear_process_failure(&target.executable_path);
                }
                Err(ProcessControlError::ProcessExited) => {
                    skipped_processes += 1;
                }
                Err(ProcessControlError::AccessDenied(message)) => {
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
                    self.failure_suppression
                        .record_process_failure(&target.executable_path);
                    self.non_release_failures.record(
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
                "",
                ActionLogResult::Applied,
                format!("Priority updated for {applied_processes} process(es)."),
            );
        }

        self.with_release_status(
            controller,
            ProcessPrioritySnapshot {
                enabled: true,
                scanned_processes,
                adjusted_processes: controller
                    .policy_managed_process_count(ControlOwner::ProcessPriority),
                skipped_processes,
                auto_excluded_processes: auto_excluded_processes.into_iter().collect(),
                ..Default::default()
            },
        )
    }

    pub(crate) fn reported_release_is_resolved(
        &self,
        controller: &PriorityEfficiencyController,
    ) -> bool {
        self.release_error_reported
            && controller
                .process_priority_release_failures()
                .next()
                .is_none()
    }

    pub(crate) fn report_release_retry(
        &mut self,
        controller: &PriorityEfficiencyController,
        restored: usize,
        mut status: ProcessPrioritySnapshot,
        action_log: &mut ActionLog,
    ) -> ProcessPrioritySnapshot {
        Self::record_restoration(action_log, restored, "restoration retry completed");
        status.auto_excluded_processes.clear();
        if status.enabled && !status.observation_paused {
            status.adjusted_processes =
                controller.policy_managed_process_count(ControlOwner::ProcessPriority);
        }
        self.with_release_status(controller, status)
    }

    fn with_release_status(
        &mut self,
        controller: &PriorityEfficiencyController,
        mut status: ProcessPrioritySnapshot,
    ) -> ProcessPrioritySnapshot {
        let mut failures = controller.process_priority_release_failures();
        let first = failures.next();
        self.release_error_reported = first.is_some();
        status.failed_processes = self.non_release_failures.count
            + usize::from(self.release_error_reported)
            + failures.count();
        status.last_error = first
            .map(|failure| {
                format!(
                    "Restore {} {} ({}): {}",
                    failure.property, failure.process_name, failure.process_id, failure.error,
                )
            })
            .or_else(|| self.non_release_failures.last_error.clone());
        status
    }

    fn disabled_snapshot(
        &mut self,
        controller: &mut PriorityEfficiencyController,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> ProcessPrioritySnapshot {
        self.record_policy_release(
            controller.release_all_priority_policy(ControlOwner::ProcessPriority),
            action_log,
            reason,
        );
        self.failure_suppression.clear();
        self.with_release_status(controller, ProcessPrioritySnapshot::default())
    }

    fn pause_for_unknown_focus(
        &mut self,
        controller: &mut PriorityEfficiencyController,
        action_log: &mut ActionLog,
    ) -> ProcessPrioritySnapshot {
        self.paused_snapshot(controller, action_log, "foreground app is unknown", None)
    }

    fn paused_snapshot(
        &mut self,
        controller: &mut PriorityEfficiencyController,
        action_log: &mut ActionLog,
        reason: &str,
        observation_error: Option<String>,
    ) -> ProcessPrioritySnapshot {
        self.record_policy_release(
            controller.release_all_priority_policy(ControlOwner::ProcessPriority),
            action_log,
            reason,
        );
        self.non_release_failures.last_error = observation_error;
        self.with_release_status(
            controller,
            ProcessPrioritySnapshot {
                enabled: true,
                observation_paused: true,
                ..Default::default()
            },
        )
    }

    fn record_policy_release(
        &mut self,
        summary: PriorityEfficiencyReleaseSummary,
        action_log: &mut ActionLog,
        reason: &str,
    ) {
        Self::record_restoration(action_log, summary.restored_processes, reason);
        for failure in summary.failures {
            self.failure_suppression
                .record_process_failure(&failure.executable_path);
            action_log.record(
                ActionLogFeature::ProcessPriority,
                Some(failure.process_id),
                failure.process_name,
                ActionLogResult::Failed,
                failure.error.to_string(),
            );
        }
    }

    fn record_restoration(action_log: &mut ActionLog, restored: usize, reason: &str) {
        if restored > 0 {
            action_log.record(
                ActionLogFeature::ProcessPriority,
                None,
                "",
                ActionLogResult::Restored,
                format!("Restored process priority for {restored} process(es): {reason}."),
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
        process_id: u32,
        process_name: &str,
        error: ProcessControlError,
        action_log: &mut ActionLog,
    ) {
        let message = error.to_string();
        if self.last_error.is_none() {
            self.last_error = Some(format!("Apply {process_name} ({process_id}): {message}"));
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

fn process_priority_target_key(target: &ProcessPriorityTarget) -> ProcessTargetKey {
    ProcessTargetKey::new(
        target.process_id,
        target.creation_time,
        Path::new(&target.executable_path),
    )
}

fn priority_policy(
    settings: &ProcessPrioritySettings,
    executable_path: &str,
    tier: PriorityProcessTier,
) -> Option<(PriorityClassValue, PriorityClassPreservation)> {
    let priority = settings
        .override_for(
            executable_path,
            tier == PriorityProcessTier::Foreground,
            tier == PriorityProcessTier::VisibleWindow,
        )
        .unwrap_or_else(|| {
            Some(tier.select(
                settings.foreground_priority,
                settings.visible_window_priority,
                settings.background_priority,
            ))
        })?;
    let priority = PriorityClassValue::from_setting(priority)?;
    let preservation = priority_preservation(
        tier,
        settings.preserve_foreground_priority,
        settings.preserve_visible_window_priority,
        settings.preserve_background_priority,
    );
    Some((priority, preservation))
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

pub fn is_builtin_excluded(process_name: &str) -> bool {
    crate::foreground::contains_process_name(CORE_BUILT_IN_PROCESS_EXCLUSIONS, process_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::{ProcessExclusionRule, ProcessPrioritySetting},
        foreground::ProcessInfo,
    };

    #[test]
    fn unverified_focus_pauses_only_focus_dependent_priority_policy() {
        let mut manager = ProcessPriorityManager::default();
        let mut controller = PriorityEfficiencyController::default();
        let mut log = ActionLog::new(8);
        let mut update = |settings: &ProcessPrioritySettings, processes: Vec<ProcessInfo>| {
            manager.update(
                &mut controller,
                settings,
                true,
                true,
                Some(std::process::id()),
                &mut CycleObservations::with_processes(processes),
                &mut log,
            )
        };
        let mut settings = ProcessPrioritySettings {
            enabled: true,
            ..Default::default()
        };
        assert!(update(&settings, vec![]).observation_paused);

        settings.foreground_detection_enabled = false;
        assert!(!update(&settings, vec![]).observation_paused);

        settings.foreground_detection_enabled = true;
        settings.background_priority = settings.foreground_priority;
        let path = r"C:\Apps\helper.exe";
        let helper = ProcessInfo {
            id: u32::MAX,
            creation_time: Some(1),
            parent_id: None,
            session_id: None,
            user_name: None,
            is_service_account: None,
            is_critical: Some(false),
            can_set_information: true,
            name: "helper.exe".to_owned(),
            image_path: Some(path.into()),
        };
        assert!(!update(&settings, vec![helper.clone()]).observation_paused);

        settings.exclusions.push(ProcessExclusionRule {
            executable_path: path.to_owned(),
            process_foreground_priority: Some(ProcessPrioritySetting::AboveNormal),
            ..Default::default()
        });
        assert!(update(&settings, vec![helper]).observation_paused);
        assert!(log.entries().is_empty());
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
