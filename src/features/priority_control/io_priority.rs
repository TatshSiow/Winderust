use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
};

use windows_sys::Win32::System::Threading::GetCurrentProcessId;

use crate::{
    action_log::{ActionLog, ActionLogFeature, ActionLogResult},
    config::{IoPrioritySettings, ProcessIoPriority},
    control::{
        io_priority::{
            IoPriorityApplyOutcome, IoPriorityClaim, IoPriorityController, IoPriorityPreservation,
            IoPriorityReleaseSummary,
        },
        process::{ControlOwner, ProcessControlError, ProcessControlTarget, ProcessTargetKey},
    },
    foreground::{
        contains_process_name, is_foreground_process, process_count_label, process_executable_path,
        process_failure_key, process_list::catalog_process_executable_path, process_session_id,
        ProtectedProcesses, CORE_BUILT_IN_PROCESS_EXCLUSIONS,
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
    pub auto_excluded_processes: Vec<String>,
    pub observation_paused: bool,
    pub(crate) waiting_for_power_source: bool,
    pub last_error: Option<String>,
}

#[derive(Default)]
pub struct IoPriorityManager {
    failure_suppression: ExecutionFailureTracker,
    non_release_failures: IoPriorityFailures,
}

impl IoPriorityManager {
    #[expect(
        clippy::too_many_arguments,
        reason = "the pass-local controller and observations are clearer here than an argument bundle"
    )]
    pub fn update(
        &mut self,
        controller: &mut IoPriorityController,
        settings: &IoPrioritySettings,
        automation_enabled: bool,
        allow_cross_session_process_control: bool,
        foreground_process_id: Option<u32>,
        observations: &mut CycleObservations,
        action_log: &mut ActionLog,
    ) -> IoPrioritySnapshot {
        self.non_release_failures = IoPriorityFailures::default();
        if !automation_enabled {
            let failures = self.clear_all(controller, action_log, "automation disabled");
            self.failure_suppression.clear();
            return IoPrioritySnapshot {
                enabled: false,
                failed_processes: failures.count,
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
                last_error: failures.last_error,
                ..Default::default()
            };
        }

        let requires_focus = settings.foreground_detection_enabled
            && (settings.child_protection.focused_app_children
                || settings.foreground_priority != settings.background_priority);
        if requires_focus && foreground_process_id.is_none() {
            return self.paused_snapshot(controller, action_log, "foreground app is unknown");
        }

        // SAFETY: GetCurrentProcessId takes no arguments and has no caller requirements.
        let current_process_id = unsafe { GetCurrentProcessId() };
        let Some(current_session_id) = process_session_id(current_process_id) else {
            return self.paused_snapshot(
                controller,
                action_log,
                "current Windows session is unknown",
            );
        };

        let processes = match observations.processes_with_paths() {
            Ok(processes) => processes,
            Err(err) => {
                self.non_release_failures.last_error = Some(err);
                return self.paused_snapshot(controller, action_log, "process list unavailable");
            }
        };

        let visible_processes = if settings.visible_window_detection_enabled {
            let Ok(process_ids) = observations.visible_window_process_ids() else {
                return self.paused_snapshot(
                    controller,
                    action_log,
                    "visible windows are unavailable",
                );
            };
            ProtectedProcesses::capture(processes.as_ref(), false, None, process_ids)
        } else {
            ProtectedProcesses::default()
        };

        let foreground_executable_path = foreground_process_id
            .filter(|_| settings.foreground_detection_enabled)
            .and_then(|id| catalog_process_executable_path(&processes, id));
        let foreground_process_id =
            foreground_process_id.filter(|_| foreground_executable_path.is_some());
        let focus_unknown =
            settings.foreground_detection_enabled && foreground_process_id.is_none();
        if focus_unknown && requires_focus {
            return self.paused_snapshot(controller, action_log, "foreground app is unknown");
        }
        let scanned_processes = processes.len();

        let child_protection = observations.priority_child_protection(
            settings.child_protection,
            &processes,
            foreground_process_id,
            &visible_processes,
        );
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

            let Some(executable_path) = process_executable_path(process)
                .and_then(|path| path.into_os_string().into_string().ok())
            else {
                continue;
            };
            let Some(creation_time) = process.creation_time else {
                continue;
            };
            let path = Path::new(&executable_path);
            let foreground = is_foreground_process(
                process.id,
                path,
                foreground_process_id,
                foreground_executable_path.as_deref(),
            );
            let visible_window = !foreground
                && settings.visible_window_detection_enabled
                && visible_processes.contains(process.id, path);
            let tier = PriorityProcessTier::from_flags(foreground, visible_window);
            if tier == PriorityProcessTier::Background && child_protection.contains(process.id) {
                continue;
            }
            let policy = priority_policy(settings, &executable_path, tier);
            if focus_unknown
                && policy
                    != priority_policy(settings, &executable_path, PriorityProcessTier::Foreground)
            {
                return self.paused_snapshot(controller, action_log, "foreground app is unknown");
            }
            if let Some((priority, preservation)) = policy {
                target_processes.insert(
                    process.id,
                    (
                        process.name.clone(),
                        executable_path,
                        priority,
                        preservation,
                        creation_time,
                    ),
                );
            }
        }

        let active_target_names = target_processes
            .values()
            .map(|(_, path, _, _, _)| process_failure_key(path))
            .collect::<BTreeSet<_>>();
        self.failure_suppression.retain_keys(&active_target_names);
        let active_targets = target_processes
            .iter()
            .map(|(process_id, (_, executable_path, _, _, creation_time))| {
                ProcessTargetKey::new(*process_id, *creation_time, Path::new(executable_path))
            })
            .collect::<BTreeSet<ProcessTargetKey>>();

        let mut skipped_processes = 0;
        let mut auto_excluded_processes = BTreeSet::new();
        let mut claims = Vec::new();
        for (process_id, (process_name, executable_path, priority, preservation, creation_time)) in
            target_processes
        {
            if self.is_process_suppressed(
                process_id,
                &process_name,
                &executable_path,
                action_log,
                &mut auto_excluded_processes,
            ) {
                skipped_processes += 1;
                continue;
            }
            claims.push((
                IoPriorityClaim {
                    target: ProcessControlTarget::automatic(
                        process_id,
                        process_name,
                        PathBuf::from(executable_path.as_str()),
                        creation_time,
                    ),
                    owner: ControlOwner::IoPriority,
                    priority,
                    preservation,
                },
                executable_path,
            ));
        }

        let release_failures = self.release_non_targets(
            controller,
            &active_targets,
            action_log,
            "process is excluded or no longer matches I/O priority defaults",
        );
        let mut applied_processes = 0;
        for (claim, executable_path) in claims {
            let process_id = claim.target.id;
            let process_name = claim.target.name.clone();
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
                Ok(IoPriorityApplyOutcome::Preserved | IoPriorityApplyOutcome::Shadowed) => {
                    skipped_processes += 1;
                    self.failure_suppression
                        .clear_process_failure(&executable_path);
                }
                Err(ProcessControlError::ProcessExited) => skipped_processes += 1,
                Err(ProcessControlError::AccessDenied(message)) => {
                    skipped_processes += 1;
                    if self
                        .failure_suppression
                        .suppress_process_failure(&executable_path)
                    {
                        action_log.record(
                            ActionLogFeature::IoPriority,
                            Some(process_id),
                            process_name,
                            ActionLogResult::Skipped,
                            message,
                        );
                    }
                }
                Err(error) => {
                    self.failure_suppression
                        .record_process_failure(&executable_path);
                    self.non_release_failures.record(
                        "Apply",
                        process_id,
                        &process_name,
                        error,
                        Some(action_log),
                    );
                }
            }
        }
        if applied_processes > 0 {
            action_log.record(
                ActionLogFeature::IoPriority,
                None,
                "",
                ActionLogResult::Applied,
                format!(
                    "I/O priority updated for {}.",
                    process_count_label(applied_processes)
                ),
            );
        }

        IoPrioritySnapshot {
            enabled: true,
            scanned_processes,
            adjusted_processes: controller.policy_managed_process_count(ControlOwner::IoPriority),
            skipped_processes,
            failed_processes: release_failures.count + self.non_release_failures.count,
            auto_excluded_processes: auto_excluded_processes.into_iter().collect(),
            observation_paused: false,
            waiting_for_power_source: false,
            last_error: release_failures
                .last_error
                .or_else(|| self.non_release_failures.last_error.clone()),
        }
    }

    pub(crate) fn pause_for_power_source(
        &mut self,
        controller: &mut IoPriorityController,
        action_log: &mut ActionLog,
    ) -> IoPrioritySnapshot {
        self.non_release_failures = IoPriorityFailures::default();
        IoPrioritySnapshot {
            waiting_for_power_source: true,
            ..self.paused_snapshot(controller, action_log, "power source is unknown")
        }
    }

    fn paused_snapshot(
        &mut self,
        controller: &mut IoPriorityController,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> IoPrioritySnapshot {
        let failures = self.clear_all(controller, action_log, reason);
        IoPrioritySnapshot {
            enabled: true,
            observation_paused: true,
            failed_processes: failures.count,
            last_error: failures
                .last_error
                .or_else(|| self.non_release_failures.last_error.clone()),
            ..Default::default()
        }
    }

    fn release_non_targets(
        &mut self,
        controller: &mut IoPriorityController,
        active_targets: &BTreeSet<ProcessTargetKey>,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> IoPriorityFailures {
        let summary = controller.release_policy_except(ControlOwner::IoPriority, active_targets);
        self.record_policy_release(summary, action_log, reason)
    }

    fn clear_all(
        &mut self,
        controller: &mut IoPriorityController,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> IoPriorityFailures {
        let summary = controller.release_all_policy(ControlOwner::IoPriority);
        self.record_policy_release(summary, action_log, reason)
    }

    pub(crate) fn report_release_retry(
        &self,
        controller: &IoPriorityController,
        summary: IoPriorityReleaseSummary,
        mut status: IoPrioritySnapshot,
        action_log: &mut ActionLog,
    ) -> IoPrioritySnapshot {
        let failures =
            Self::record_release_summary(summary, action_log, "restoration retry completed", false);
        status.auto_excluded_processes.clear();
        status.failed_processes = self.non_release_failures.count + failures.count;
        status.last_error = failures
            .last_error
            .or_else(|| self.non_release_failures.last_error.clone());
        if status.enabled {
            status.adjusted_processes =
                controller.policy_managed_process_count(ControlOwner::IoPriority);
        }
        status
    }

    fn record_policy_release(
        &mut self,
        summary: IoPriorityReleaseSummary,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> IoPriorityFailures {
        for failure in &summary.failures {
            if let Some(executable_path) = failure.executable_path.to_str() {
                self.failure_suppression
                    .record_process_failure(executable_path);
            }
        }
        Self::record_release_summary(summary, action_log, reason, true)
    }

    fn record_release_summary(
        summary: IoPriorityReleaseSummary,
        action_log: &mut ActionLog,
        reason: &str,
        log_failures: bool,
    ) -> IoPriorityFailures {
        let mut failures = IoPriorityFailures::default();
        for failure in summary.failures {
            failures.record(
                "Restore",
                failure.process_id,
                &failure.process_name,
                failure.error,
                log_failures.then_some(&mut *action_log),
            );
        }
        if summary.restored_processes > 0 {
            action_log.record(
                ActionLogFeature::IoPriority,
                None,
                "",
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

fn priority_policy(
    settings: &IoPrioritySettings,
    path: &str,
    tier: PriorityProcessTier,
) -> Option<(ProcessIoPriority, IoPriorityPreservation)> {
    let priority = settings
        .override_for(
            path,
            tier == PriorityProcessTier::Foreground,
            tier == PriorityProcessTier::VisibleWindow,
        )
        .unwrap_or_else(|| {
            Some(tier.select(
                settings.foreground_priority,
                settings.visible_window_priority,
                settings.background_priority,
            ))
        })?
        .priority()?;
    Some((priority, tier_preservation(settings, tier)))
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
        action_log: Option<&mut ActionLog>,
    ) {
        let message = error.to_string();
        if self.last_error.is_none() {
            self.last_error = Some(format!("{action} {process_name} ({process_id}): {message}"));
        }
        self.count += 1;
        if let Some(action_log) = action_log {
            action_log.record(
                ActionLogFeature::IoPriority,
                Some(process_id),
                process_name.to_owned(),
                ActionLogResult::Failed,
                message,
            );
        }
    }
}

pub fn is_builtin_excluded(process_name: &str) -> bool {
    contains_process_name(CORE_BUILT_IN_PROCESS_EXCLUSIONS, process_name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::ProcessIoPrioritySetting, foreground::ProcessInfo};

    #[test]
    fn unverified_foreground_pauses_when_policy_depends_on_focus() {
        let process_id = std::process::id();
        let mut observations = CycleObservations::with_processes(vec![ProcessInfo {
            id: process_id,
            creation_time: Some(0),
            parent_id: None,
            session_id: None,
            user_name: None,
            is_service_account: None,
            is_critical: None,
            can_set_information: false,
            name: "test process".to_owned(),
            image_path: None,
        }]);
        let mut manager = IoPriorityManager::default();
        let mut controller = IoPriorityController::default();
        let mut action_log = ActionLog::default();
        let mut settings = IoPrioritySettings {
            enabled: true,
            foreground_priority: ProcessIoPrioritySetting::Low,
            background_priority: ProcessIoPrioritySetting::Default,
            ..Default::default()
        };
        let mut update = |settings: &IoPrioritySettings| {
            manager.update(
                &mut controller,
                settings,
                true,
                true,
                Some(process_id),
                &mut observations,
                &mut action_log,
            )
        };

        assert!(update(&settings).observation_paused);
        settings.foreground_detection_enabled = false;
        assert!(!update(&settings).observation_paused);

        settings.foreground_detection_enabled = true;
        settings.foreground_priority = ProcessIoPrioritySetting::Default;
        settings.child_protection.focused_app_children = false;
        let uniform = update(&settings);
        assert!(!uniform.observation_paused);
        assert_eq!(uniform.scanned_processes, 1);
    }

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
