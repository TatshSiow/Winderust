use std::{collections::BTreeSet, path::PathBuf};

use windows_sys::Win32::System::Threading::GetCurrentProcessId;

use crate::{
    action_log::{ActionLog, ActionLogFeature, ActionLogResult},
    config::{MemoryPrioritySettings, ProcessMemoryPriority, ProcessMemoryPrioritySetting},
    control::{
        memory_priority::{
            MemoryPriorityApplyOutcome, MemoryPriorityClaim, MemoryPriorityController,
            MemoryPriorityPreservation, MemoryPriorityReleaseSummary,
        },
        process::{ControlOwner, ProcessControlError, ProcessControlTarget},
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
pub struct MemoryPrioritySnapshot {
    pub enabled: bool,
    pub adjusted_processes: usize,
    pub skipped_processes: usize,
    pub failed_processes: usize,
    pub adjusted_apps: Vec<String>,
    pub auto_excluded_processes: Vec<String>,
    pub last_error: Option<String>,
}

#[derive(Default)]
pub struct MemoryPriorityManager {
    failure_suppression: ExecutionFailureTracker,
}

#[derive(Debug, Clone)]
pub struct MemoryPriorityTarget {
    pub process_id: u32,
    pub process_name: String,
    pub executable_path: String,
    pub creation_time: u64,
    pub priority: ProcessMemoryPriority,
    pub foreground: bool,
    pub visible_window: bool,
    pub preserve_foreground_priority: bool,
    pub preserve_visible_window_priority: bool,
    pub preserve_background_priority: bool,
}

impl MemoryPriorityManager {
    #[expect(
        clippy::too_many_arguments,
        reason = "the policy boundary receives the shared runtime observations and controller"
    )]
    pub fn update_rules(
        &mut self,
        controller: &mut MemoryPriorityController,
        settings: &MemoryPrioritySettings,
        automation_enabled: bool,
        allow_cross_session_process_control: bool,
        foreground_process_id: Option<u32>,
        observations: &mut CycleObservations,
        action_log: &mut ActionLog,
    ) -> MemoryPrioritySnapshot {
        if !automation_enabled {
            return self.disabled_snapshot(
                controller,
                ControlOwner::MemoryPriority,
                ActionLogFeature::MemoryPriority,
                action_log,
                "automation disabled",
            );
        }

        if !settings.enabled {
            return self.disabled_snapshot(
                controller,
                ControlOwner::MemoryPriority,
                ActionLogFeature::MemoryPriority,
                action_log,
                "memory priority defaults disabled",
            );
        }

        let foreground_sensitive = settings.foreground_detection_enabled
            && settings.foreground_priority != settings.background_priority;
        if foreground_sensitive && foreground_process_id.is_none() {
            return self.paused_snapshot(
                controller,
                ControlOwner::MemoryPriority,
                ActionLogFeature::MemoryPriority,
                action_log,
                "foreground app is unknown",
                None,
            );
        }

        // SAFETY: GetCurrentProcessId takes no arguments and has no caller requirements.
        let current_process_id = unsafe { GetCurrentProcessId() };
        let Some(current_session_id) = process_session_id(current_process_id) else {
            return self.paused_snapshot(
                controller,
                ControlOwner::MemoryPriority,
                ActionLogFeature::MemoryPriority,
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
                    ControlOwner::MemoryPriority,
                    ActionLogFeature::MemoryPriority,
                    action_log,
                    "process list unavailable",
                    Some(error),
                );
            }
        };

        let visible_processes = if settings.visible_window_detection_enabled {
            let Ok(process_ids) = observations.visible_window_process_ids() else {
                return self.paused_snapshot(
                    controller,
                    ControlOwner::MemoryPriority,
                    ActionLogFeature::MemoryPriority,
                    action_log,
                    "visible windows are unavailable",
                    Some("Paused: visible windows are unavailable.".to_owned()),
                );
            };
            ProtectedProcesses::capture(processes.as_ref(), false, None, process_ids)
        } else {
            ProtectedProcesses::default()
        };

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

        let targets = processes
            .iter()
            .filter_map(|process| {
                if process.is_critical != Some(false)
                    || !process.can_set_information
                    || should_skip_process(
                        process.id,
                        &process.name,
                        current_process_id,
                        current_session_id,
                        allow_cross_session_process_control,
                    )
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
                let configured_override =
                    settings.override_for(executable_path.to_string_lossy().as_ref(), foreground);
                let priority = match configured_override {
                    Some(Some(ProcessMemoryPrioritySetting::Auto)) => default_priority,
                    Some(Some(priority)) => priority,
                    Some(None) => return None,
                    None => default_priority,
                };

                priority.priority().map(|priority| MemoryPriorityTarget {
                    process_id: process.id,
                    process_name: process.name.clone(),
                    executable_path: executable_path.to_string_lossy().into_owned(),
                    creation_time,
                    priority,
                    foreground,
                    visible_window,
                    preserve_foreground_priority: settings.preserve_foreground_priority,
                    preserve_visible_window_priority: settings.preserve_visible_window_priority,
                    preserve_background_priority: settings.preserve_background_priority,
                })
            })
            .collect();

        let mut snapshot = self.update(
            controller,
            ControlOwner::MemoryPriority,
            targets,
            true,
            allow_cross_session_process_control,
            ActionLogFeature::MemoryPriority,
            action_log,
        );
        snapshot.enabled = true;
        snapshot
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "the policy manager keeps owner, access, and reporting context explicit"
    )]
    pub fn update(
        &mut self,
        controller: &mut MemoryPriorityController,
        owner: ControlOwner,
        targets: Vec<MemoryPriorityTarget>,
        automation_enabled: bool,
        allow_cross_session_process_control: bool,
        action_log_feature: ActionLogFeature,
        action_log: &mut ActionLog,
    ) -> MemoryPrioritySnapshot {
        if !automation_enabled {
            return self.disabled_snapshot(
                controller,
                owner,
                action_log_feature,
                action_log,
                "automation disabled",
            );
        }

        let active_targets = targets
            .iter()
            .map(memory_priority_target_key)
            .collect::<BTreeSet<_>>();
        let target_names = targets
            .iter()
            .map(|target| process_failure_key(&target.executable_path))
            .collect::<BTreeSet<_>>();
        self.failure_suppression.retain_keys(&target_names);

        let mut failures = MemoryPriorityFailures::default();
        self.merge_release_summary(
            controller.release_policy_except(owner, &active_targets),
            action_log_feature,
            action_log,
            "process no longer matches a memory priority target",
            &mut failures,
        );
        let mut skipped_processes = 0;
        let mut applied_processes = 0;
        let mut auto_excluded_processes = BTreeSet::new();

        for target in targets {
            if self.is_executable_path_suppressed(
                target.process_id,
                &target.process_name,
                &target.executable_path,
                action_log_feature,
                action_log,
                &mut auto_excluded_processes,
            ) {
                skipped_processes += 1;
                continue;
            }

            let tier = PriorityProcessTier::from_flags(target.foreground, target.visible_window);
            let preservation = memory_priority_preservation(
                tier,
                target.preserve_foreground_priority,
                target.preserve_visible_window_priority,
                target.preserve_background_priority,
            );
            let claim = MemoryPriorityClaim {
                target: ProcessControlTarget::automatic(
                    target.process_id,
                    target.process_name.clone(),
                    PathBuf::from(&target.executable_path),
                    target.creation_time,
                ),
                owner,
                priority: target.priority,
                preservation,
            };
            match controller.apply_policy_claim(claim, allow_cross_session_process_control) {
                Ok(MemoryPriorityApplyOutcome::Applied) => {
                    applied_processes += 1;
                    self.clear_process_failure(&target.executable_path);
                }
                Ok(
                    MemoryPriorityApplyOutcome::Unchanged | MemoryPriorityApplyOutcome::Shadowed,
                ) => {
                    self.clear_process_failure(&target.executable_path);
                }
                Ok(MemoryPriorityApplyOutcome::Preserved) => {
                    skipped_processes += 1;
                    self.clear_process_failure(&target.executable_path);
                }
                Err(ProcessControlError::ProcessExited) => {
                    skipped_processes += 1;
                }
                Err(ProcessControlError::AccessDenied(message)) => {
                    skipped_processes += 1;
                    self.failure_suppression
                        .suppress_process_failure(&target.executable_path);
                    action_log.record(
                        action_log_feature,
                        Some(target.process_id),
                        target.process_name,
                        ActionLogResult::Skipped,
                        message,
                    );
                }
                Err(error) => {
                    self.record_process_failure(&target.executable_path);
                    failures.record(
                        "Apply",
                        target.process_id,
                        &target.process_name,
                        error,
                        action_log_feature,
                        action_log,
                    );
                }
            }
        }
        if applied_processes > 0 {
            action_log.record(
                action_log_feature,
                None,
                memory_priority_summary_process_name(action_log_feature),
                ActionLogResult::Applied,
                memory_priority_apply_summary_message(applied_processes),
            );
        }

        let adjusted_apps = unique_app_names(
            controller
                .policy_managed_process_names(owner)
                .iter()
                .map(String::as_str),
        );
        MemoryPrioritySnapshot {
            enabled: true,
            adjusted_processes: controller.policy_managed_process_count(owner),
            skipped_processes,
            failed_processes: failures.count,
            adjusted_apps,
            auto_excluded_processes: auto_excluded_processes.into_iter().collect(),
            last_error: failures.last_error,
        }
    }

    fn disabled_snapshot(
        &mut self,
        controller: &mut MemoryPriorityController,
        owner: ControlOwner,
        action_log_feature: ActionLogFeature,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> MemoryPrioritySnapshot {
        let mut failures = MemoryPriorityFailures::default();
        self.merge_release_summary(
            controller.release_all_policy(owner),
            action_log_feature,
            action_log,
            reason,
            &mut failures,
        );
        self.failure_suppression.clear();
        MemoryPrioritySnapshot {
            enabled: false,
            failed_processes: failures.count,
            last_error: failures.last_error,
            ..Default::default()
        }
    }

    fn paused_snapshot(
        &mut self,
        controller: &mut MemoryPriorityController,
        owner: ControlOwner,
        action_log_feature: ActionLogFeature,
        action_log: &mut ActionLog,
        reason: &str,
        error: Option<String>,
    ) -> MemoryPrioritySnapshot {
        let mut failures = MemoryPriorityFailures::default();
        self.merge_release_summary(
            controller.release_all_policy(owner),
            action_log_feature,
            action_log,
            reason,
            &mut failures,
        );
        if let Some(error) = error {
            failures.count += 1;
            failures.last_error.get_or_insert(error);
        }
        MemoryPrioritySnapshot {
            enabled: true,
            failed_processes: failures.count,
            last_error: failures.last_error,
            ..Default::default()
        }
    }

    fn merge_release_summary(
        &mut self,
        summary: MemoryPriorityReleaseSummary,
        action_log_feature: ActionLogFeature,
        action_log: &mut ActionLog,
        reason: &str,
        failures: &mut MemoryPriorityFailures,
    ) {
        if summary.restored_processes > 0 {
            action_log.record(
                action_log_feature,
                None,
                memory_priority_summary_process_name(action_log_feature),
                ActionLogResult::Restored,
                memory_priority_restore_summary_message(summary.restored_processes, reason),
            );
        }
        for failure in summary.failures {
            self.record_process_failure(&failure.executable_path);
            match failure.error {
                ProcessControlError::AccessDenied(message) => action_log.record(
                    action_log_feature,
                    Some(failure.process_id),
                    failure.process_name,
                    ActionLogResult::Skipped,
                    format!("{message} Previous Memory Priority could not be restored: {reason}."),
                ),
                error => failures.record(
                    "Restore",
                    failure.process_id,
                    &failure.process_name,
                    error,
                    action_log_feature,
                    action_log,
                ),
            }
        }
    }

    fn is_executable_path_suppressed(
        &mut self,
        process_id: u32,
        process_name: &str,
        executable_path: &str,
        action_log_feature: ActionLogFeature,
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
                action_log_feature,
                Some(process_id),
                process_name.to_owned(),
                ActionLogResult::Skipped,
                format!(
                    "Stopped retrying memory priority after {} failed attempts.",
                    execution_failure_suppression_threshold(),
                ),
            );
        }
        true
    }

    #[cfg(test)]
    fn is_process_suppressed(
        &mut self,
        process_id: u32,
        process_name: &str,
        action_log_feature: ActionLogFeature,
        action_log: &mut ActionLog,
        auto_excluded_processes: &mut BTreeSet<String>,
    ) -> bool {
        self.is_executable_path_suppressed(
            process_id,
            process_name,
            process_name,
            action_log_feature,
            action_log,
            auto_excluded_processes,
        )
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
        error: ProcessControlError,
        action_log_feature: ActionLogFeature,
        action_log: &mut ActionLog,
    ) {
        let message = error.to_string();
        if self.last_error.is_none() {
            self.last_error = Some(format!("{action} {process_name} ({process_id}): {message}"));
        }
        self.count += 1;
        action_log.record(
            action_log_feature,
            Some(process_id),
            process_name.to_owned(),
            ActionLogResult::Failed,
            message,
        );
    }
}

fn memory_priority_target_key(
    target: &MemoryPriorityTarget,
) -> crate::control::process::ProcessTargetKey {
    ProcessControlTarget::automatic(
        target.process_id,
        target.process_name.clone(),
        PathBuf::from(&target.executable_path),
        target.creation_time,
    )
    .key()
}

fn memory_priority_preservation(
    tier: PriorityProcessTier,
    preserve_foreground: bool,
    preserve_visible_window: bool,
    preserve_background: bool,
) -> MemoryPriorityPreservation {
    match tier {
        PriorityProcessTier::Foreground if preserve_foreground => {
            MemoryPriorityPreservation::PreserveHigher
        }
        PriorityProcessTier::VisibleWindow if preserve_visible_window => {
            MemoryPriorityPreservation::PreserveHigher
        }
        PriorityProcessTier::Background if preserve_background => {
            MemoryPriorityPreservation::PreserveLower
        }
        _ => MemoryPriorityPreservation::Exact,
    }
}

pub fn is_builtin_excluded(process_name: &str) -> bool {
    contains_process_name(CORE_BUILT_IN_PROCESS_EXCLUSIONS, process_name)
}

fn should_skip_process(
    process_id: u32,
    process_name: &str,
    current_process_id: u32,
    current_session_id: u32,
    allow_cross_session_process_control: bool,
) -> bool {
    process_id == 0
        || process_id == current_process_id
        || (!allow_cross_session_process_control
            && process_session_id(process_id) != Some(current_session_id))
        || is_builtin_excluded(process_name)
}

fn memory_priority_apply_summary_message(count: usize) -> String {
    format!("Applied memory priority to {}.", process_count_label(count))
}

fn memory_priority_restore_summary_message(count: usize, reason: &str) -> String {
    format!(
        "Restored previous memory priority for {}: {reason}.",
        process_count_label(count)
    )
}

fn memory_priority_summary_process_name(action_log_feature: ActionLogFeature) -> &'static str {
    match action_log_feature {
        ActionLogFeature::WorkloadEngine => "Workload Engine",
        _ => "Memory Priority",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quick_actions_require_a_concrete_memory_priority() {
        assert_eq!(ProcessMemoryPrioritySetting::Default.priority(), None);
        assert_eq!(ProcessMemoryPrioritySetting::Auto.priority(), None);
        assert_eq!(
            ProcessMemoryPrioritySetting::Low.priority(),
            Some(ProcessMemoryPriority::Low)
        );
    }

    #[test]
    fn tier_preservation_matches_focus_visible_and_background_contract() {
        assert_eq!(
            memory_priority_preservation(PriorityProcessTier::Foreground, true, true, true),
            MemoryPriorityPreservation::PreserveHigher
        );
        assert_eq!(
            memory_priority_preservation(PriorityProcessTier::VisibleWindow, true, true, true),
            MemoryPriorityPreservation::PreserveHigher
        );
        assert_eq!(
            memory_priority_preservation(PriorityProcessTier::Background, true, true, true),
            MemoryPriorityPreservation::PreserveLower
        );
    }

    #[test]
    fn repeated_process_failures_suppress_memory_priority_retries() {
        let mut manager = MemoryPriorityManager::default();
        let mut log = ActionLog::new(8);

        manager.record_process_failure(r"C:\Apps\app.exe");
        manager.record_process_failure(r"C:/Apps/app.exe");
        assert!(!manager.is_process_suppressed(
            42,
            r"C:\Apps\app.exe",
            ActionLogFeature::WorkloadEngine,
            &mut log,
            &mut BTreeSet::new()
        ));

        manager.record_process_failure(r"C:\Apps\app.exe");
        assert!(manager.is_process_suppressed(
            42,
            r"C:\Apps\app.exe",
            ActionLogFeature::WorkloadEngine,
            &mut log,
            &mut BTreeSet::new()
        ));
        assert!(manager.is_process_suppressed(
            43,
            r"C:/Apps/app.exe",
            ActionLogFeature::WorkloadEngine,
            &mut log,
            &mut BTreeSet::new()
        ));

        let entries = log.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].feature, ActionLogFeature::WorkloadEngine);
        assert_eq!(entries[0].result, ActionLogResult::Skipped);
        assert!(entries[0]
            .reason
            .contains("Stopped retrying memory priority"));
    }

    #[test]
    fn memory_priority_summary_messages_use_process_count() {
        assert_eq!(
            memory_priority_apply_summary_message(1),
            "Applied memory priority to 1 process."
        );
        assert_eq!(
            memory_priority_apply_summary_message(5),
            "Applied memory priority to 5 processes."
        );
        assert_eq!(
            memory_priority_restore_summary_message(1, "foreground app is unknown"),
            "Restored previous memory priority for 1 process: foreground app is unknown."
        );
    }

    #[test]
    fn memory_priority_summary_process_name_matches_feature_context() {
        assert_eq!(
            memory_priority_summary_process_name(ActionLogFeature::MemoryPriority),
            "Memory Priority"
        );
        assert_eq!(
            memory_priority_summary_process_name(ActionLogFeature::WorkloadEngine),
            "Workload Engine"
        );
    }
}
