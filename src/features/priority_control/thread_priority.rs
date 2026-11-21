use std::collections::{BTreeMap, BTreeSet};

use windows_sys::Win32::System::Threading::GetCurrentProcessId;

use crate::{
    action_log::{ActionLog, ActionLogFeature, ActionLogResult},
    config::{ProcessThreadPrioritySetting, ThreadPrioritySettings},
    control::{
        process::{ControlOwner, ProcessControlError, ProcessControlTarget, ProcessTargetKey},
        thread_priority::{
            thread_priority_is_actionable, ThreadInventory, ThreadPriorityApplyOutcome,
            ThreadPriorityClaim, ThreadPriorityController, ThreadPriorityPreservation,
            ThreadPriorityReleaseSummary,
        },
    },
    foreground::{
        process_executable_path, process_failure_key, process_session_id, same_process_name,
        ProtectedProcesses, CORE_BUILT_IN_PROCESS_EXCLUSIONS,
    },
    rules::{execution_failure_suppression_threshold, ExecutionFailureTracker},
    runtime::observations::CycleObservations,
};

use super::PriorityProcessTier;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThreadPrioritySnapshot {
    pub enabled: bool,
    pub scanned_processes: usize,
    pub adjusted_processes: usize,
    pub skipped_processes: usize,
    pub failed_processes: usize,
    pub adjusted_apps: Vec<String>,
    pub auto_excluded_processes: Vec<String>,
    pub(crate) observation_paused: bool,
    pub last_error: Option<String>,
}

#[derive(Default)]
pub struct ThreadPriorityManager {
    failure_suppression: ExecutionFailureTracker,
}

impl ThreadPriorityManager {
    #[expect(
        clippy::too_many_arguments,
        reason = "policy reconciliation needs settings, observations, controller, and reporting"
    )]
    pub fn update(
        &mut self,
        controller: &mut ThreadPriorityController,
        owner: ControlOwner,
        settings: &ThreadPrioritySettings,
        automation_enabled: bool,
        allow_cross_session_process_control: bool,
        foreground_process_id: Option<u32>,
        observations: &mut CycleObservations,
        action_log: &mut ActionLog,
    ) -> ThreadPrioritySnapshot {
        if !automation_enabled {
            let failures = record_release(
                controller.release_all_policy(),
                action_log,
                "automation disabled",
            );
            self.failure_suppression.clear();
            return inactive_snapshot(failures);
        }
        if !settings.enabled {
            let failures = record_release(
                controller.release_all_policy(),
                action_log,
                "Thread Priority disabled",
            );
            self.failure_suppression.clear();
            return inactive_snapshot(failures);
        }

        let focus_unknown =
            settings.foreground_detection_enabled && foreground_process_id.is_none();
        if focus_unknown && settings.foreground_priority != settings.background_priority {
            return pause_for_unknown_focus(controller, action_log);
        }

        // SAFETY: GetCurrentProcessId takes no arguments and has no caller requirements.
        let current_process_id = unsafe { GetCurrentProcessId() };
        let current_session_id = if allow_cross_session_process_control {
            None
        } else {
            let Some(session_id) = process_session_id(current_process_id) else {
                return paused_snapshot(
                    controller,
                    action_log,
                    "current Windows session is unknown",
                    None,
                );
            };
            Some(session_id)
        };

        let processes = match observations.processes_with_paths() {
            Ok(processes) => processes,
            Err(error) => {
                return paused_snapshot(
                    controller,
                    action_log,
                    "process list unavailable",
                    Some(error),
                );
            }
        };
        let scanned_processes = processes.len();
        let visible_processes = if settings.visible_window_detection_enabled {
            let Ok(process_ids) = observations.visible_window_process_ids() else {
                return paused_snapshot(
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
        let adaptive_workload = (owner == ControlOwner::AdaptiveEngine)
            .then(|| observations.adaptive_workload(processes.as_ref()));
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

        let mut targets = BTreeMap::<ProcessTargetKey, AutomaticTarget>::new();
        for process in processes.iter() {
            if process.id == 0
                || process.id == current_process_id
                || process.is_critical != Some(false)
                || current_session_id.is_some_and(|session| process.session_id != Some(session))
                || is_builtin_excluded(&process.name)
            {
                continue;
            }
            let (Some(executable_path), Some(creation_time)) =
                (process.image_path.as_deref(), process.creation_time)
            else {
                continue;
            };
            let Some(path) = executable_path.to_str() else {
                continue;
            };
            let foreground = settings.foreground_detection_enabled
                && super::foreground_for_owner(
                    owner,
                    process,
                    executable_path,
                    foreground_process_id,
                    foreground_executable_path.as_deref(),
                    adaptive_workload.as_deref(),
                );
            let visible_window = !foreground
                && settings.visible_window_detection_enabled
                && visible_processes.contains(process.id, executable_path);
            let tier = PriorityProcessTier::from_flags(foreground, visible_window);
            let policy = priority_policy(settings, path, tier);
            if focus_unknown
                && policy != priority_policy(settings, path, PriorityProcessTier::Foreground)
            {
                return pause_for_unknown_focus(controller, action_log);
            }
            let Some((priority, preservation)) = policy else {
                continue;
            };
            let target = ProcessControlTarget::automatic(
                process.id,
                process.name.clone(),
                executable_path.to_path_buf(),
                creation_time,
            );
            targets.insert(
                target.key(),
                AutomaticTarget {
                    claim: ThreadPriorityClaim {
                        target,
                        owner,
                        priority,
                        preservation,
                    },
                    process_name: process.name.clone(),
                    executable_path: path.to_owned(),
                },
            );
        }

        let active_targets = targets.keys().cloned().collect::<BTreeSet<_>>();
        let active_target_names = targets
            .values()
            .map(|target| process_failure_key(&target.executable_path))
            .collect::<BTreeSet<_>>();
        self.failure_suppression.retain_keys(&active_target_names);
        let mut failures = record_release(
            controller.release_policy_except(&active_targets),
            action_log,
            "process is excluded or no longer matches Thread Priority defaults",
        );
        let mut skipped_processes = 0;
        let mut applied_threads = 0;
        let mut auto_excluded_processes = BTreeSet::new();

        let mut inventory = ThreadInventory::default();
        for target in targets.into_values() {
            let process_id = target.claim.target.id;
            if self.is_process_suppressed(
                process_id,
                &target.process_name,
                &target.executable_path,
                action_log,
                &mut auto_excluded_processes,
            ) {
                skipped_processes += 1;
                continue;
            }
            match controller.apply_policy_claim(
                &mut inventory,
                target.claim,
                allow_cross_session_process_control,
            ) {
                Ok(outcome) => {
                    applied_threads += outcome.applied_threads;
                    skipped_processes += usize::from(process_was_only_preserved(outcome));
                    self.failure_suppression
                        .clear_process_failure(&target.executable_path);
                }
                Err(error) if inventory.failed() => {
                    // A shared observation failure is not a failure of any application.
                    failures.count += 1;
                    failures.last_error.get_or_insert_with(|| error.to_string());
                    break;
                }
                Err(ProcessControlError::ProcessExited) => skipped_processes += 1,
                Err(ProcessControlError::AccessDenied(message)) => {
                    skipped_processes += 1;
                    self.failure_suppression
                        .suppress_process_failure(&target.executable_path);
                    action_log.record(
                        ActionLogFeature::ThreadPriority,
                        Some(process_id),
                        target.process_name,
                        ActionLogResult::Skipped,
                        message,
                    );
                }
                Err(error) => {
                    self.failure_suppression
                        .record_process_failure(&target.executable_path);
                    failures.record(
                        "Apply",
                        process_id,
                        &target.process_name,
                        error.to_string(),
                        action_log,
                    );
                }
            }
        }
        if applied_threads > 0 {
            action_log.record(
                ActionLogFeature::ThreadPriority,
                None,
                "",
                ActionLogResult::Applied,
                format!("Priority updated for {applied_threads} thread(s)."),
            );
        }

        ThreadPrioritySnapshot {
            enabled: true,
            scanned_processes,
            adjusted_processes: controller.policy_managed_process_count(),
            skipped_processes,
            failed_processes: failures.count,
            adjusted_apps: controller.policy_managed_process_names(),
            auto_excluded_processes: auto_excluded_processes.into_iter().collect(),
            observation_paused: inventory.failed(),
            last_error: failures.last_error,
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
                ActionLogFeature::ThreadPriority,
                Some(process_id),
                process_name.to_owned(),
                ActionLogResult::Skipped,
                format!(
                    "Stopped retrying Thread Priority after {} failed attempts.",
                    execution_failure_suppression_threshold(),
                ),
            );
        }
        true
    }
}

struct AutomaticTarget {
    claim: ThreadPriorityClaim,
    process_name: String,
    executable_path: String,
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
        message: String,
        action_log: &mut ActionLog,
    ) {
        if self.last_error.is_none() {
            self.last_error = Some(format!("{action} {process_name} ({id}): {message}"));
        }
        self.count += 1;
        action_log.record(
            ActionLogFeature::ThreadPriority,
            Some(id),
            process_name.to_owned(),
            ActionLogResult::Failed,
            message,
        );
    }
}

fn inactive_snapshot(failures: ThreadPriorityFailures) -> ThreadPrioritySnapshot {
    ThreadPrioritySnapshot {
        failed_processes: failures.count,
        last_error: failures.last_error,
        ..Default::default()
    }
}

fn pause_for_unknown_focus(
    controller: &mut ThreadPriorityController,
    action_log: &mut ActionLog,
) -> ThreadPrioritySnapshot {
    paused_snapshot(controller, action_log, "foreground app is unknown", None)
}

fn paused_snapshot(
    controller: &mut ThreadPriorityController,
    action_log: &mut ActionLog,
    reason: &str,
    observation_error: Option<String>,
) -> ThreadPrioritySnapshot {
    let failures = record_release(controller.release_all_policy(), action_log, reason);
    ThreadPrioritySnapshot {
        enabled: true,
        observation_paused: true,
        failed_processes: failures.count,
        last_error: failures.last_error.or(observation_error),
        ..Default::default()
    }
}

fn record_release(
    summary: ThreadPriorityReleaseSummary,
    action_log: &mut ActionLog,
    reason: &str,
) -> ThreadPriorityFailures {
    let mut failures = ThreadPriorityFailures::default();
    for failure in summary.failures {
        failures.record(
            "Restore",
            failure.process_id,
            &failure.process_name,
            format!("Thread {}: {}", failure.thread_id, failure.error),
            action_log,
        );
    }
    if summary.restored_threads > 0 {
        action_log.record(
            ActionLogFeature::ThreadPriority,
            None,
            "",
            ActionLogResult::Restored,
            format!(
                "Restored thread priority for {} thread(s): {reason}.",
                summary.restored_threads
            ),
        );
    }
    failures
}

fn priority_policy(
    settings: &ThreadPrioritySettings,
    executable_path: &str,
    tier: PriorityProcessTier,
) -> Option<(ProcessThreadPrioritySetting, ThreadPriorityPreservation)> {
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
    thread_priority_is_actionable(priority)
        .then(|| (priority, preservation_for_tier(settings, tier)))
}

fn preservation_for_tier(
    settings: &ThreadPrioritySettings,
    tier: PriorityProcessTier,
) -> ThreadPriorityPreservation {
    match tier {
        PriorityProcessTier::Foreground if settings.preserve_foreground_priority => {
            ThreadPriorityPreservation::PreserveHigher
        }
        PriorityProcessTier::VisibleWindow if settings.preserve_visible_window_priority => {
            ThreadPriorityPreservation::PreserveHigher
        }
        PriorityProcessTier::Background if settings.preserve_background_priority => {
            ThreadPriorityPreservation::PreserveLower
        }
        _ => ThreadPriorityPreservation::Exact,
    }
}

fn process_was_only_preserved(outcome: ThreadPriorityApplyOutcome) -> bool {
    outcome.applied_threads == 0 && outcome.unchanged_threads == 0 && outcome.preserved_threads > 0
}

pub fn is_builtin_excluded(process_name: &str) -> bool {
    CORE_BUILT_IN_PROCESS_EXCLUSIONS
        .iter()
        .any(|excluded| same_process_name(excluded, process_name))
}

#[cfg(test)]
mod tests {
    use crate::foreground::is_foreground_process;
    use std::path::Path;

    use super::*;

    #[test]
    fn foreground_matching_uses_pid_or_same_executable_path() {
        let foreground = Path::new(r"C:\Apps\Foreground\app.exe");

        assert!(is_foreground_process(
            7,
            Path::new(r"D:\Other\app.exe"),
            Some(7),
            Some(foreground),
        ));
        assert!(is_foreground_process(
            8,
            foreground,
            Some(7),
            Some(foreground),
        ));
        assert!(!is_foreground_process(
            8,
            Path::new(r"D:\Other\app.exe"),
            Some(7),
            Some(foreground),
        ));
    }

    #[test]
    fn repeated_failures_emit_one_thread_priority_auto_exclusion_and_success_resets_it() {
        let mut manager = ThreadPriorityManager::default();
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
        assert_eq!(log.entries()[0].feature, ActionLogFeature::ThreadPriority);

        manager.failure_suppression.clear_process_failure(path);
        auto_excluded.clear();
        assert!(!manager.is_process_suppressed(42, "app.exe", path, &mut log, &mut auto_excluded,));
    }

    #[test]
    fn preservation_direction_matches_the_visible_tier_contract() {
        let settings = ThreadPrioritySettings {
            preserve_foreground_priority: true,
            preserve_visible_window_priority: true,
            preserve_background_priority: true,
            ..Default::default()
        };
        assert_eq!(
            preservation_for_tier(&settings, PriorityProcessTier::Foreground),
            ThreadPriorityPreservation::PreserveHigher
        );
        assert_eq!(
            preservation_for_tier(&settings, PriorityProcessTier::VisibleWindow),
            ThreadPriorityPreservation::PreserveHigher
        );
        assert_eq!(
            preservation_for_tier(&settings, PriorityProcessTier::Background),
            ThreadPriorityPreservation::PreserveLower
        );
    }
}
