use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};

use windows_sys::Win32::System::Threading::GetCurrentProcessId;

use crate::{
    action_log::{ActionLog, ActionLogFeature, ActionLogResult},
    config::{GpuPrioritySettings, ProcessGpuPrioritySetting},
    control::{
        gpu_priority::{
            GpuPriorityApplyOutcome, GpuPriorityClaim, GpuPriorityController,
            GpuPriorityPreservation, GpuPriorityReleaseSummary,
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

const GPU_PRIORITY_SUMMARY_LOG_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GpuPrioritySnapshot {
    pub enabled: bool,
    pub scanned_processes: usize,
    pub adjusted_processes: usize,
    pub skipped_processes: usize,
    pub pending_processes: usize,
    pub denied_processes: usize,
    pub suppressed_processes: usize,
    pub failed_processes: usize,
    pub adjusted_apps: Vec<String>,
    pub auto_excluded_processes: Vec<String>,
    pub message: String,
    pub last_error: Option<String>,
}

#[derive(Default)]
pub struct GpuPriorityManager {
    failure_suppression: ExecutionFailureTracker,
    pending_context: BTreeSet<String>,
    pending_apply_log_count: usize,
    pending_context_log_count: usize,
    pending_access_denied_log_count: usize,
    last_apply_summary_logged_at: Option<Instant>,
    last_skip_summary_logged_at: Option<Instant>,
}

impl GpuPriorityManager {
    #[expect(
        clippy::too_many_arguments,
        reason = "the pass-local controller and observations are clearer here than an argument bundle"
    )]
    pub fn update(
        &mut self,
        controller: &mut GpuPriorityController,
        owner: ControlOwner,
        settings: &GpuPrioritySettings,
        automation_enabled: bool,
        allow_cross_session_process_control: bool,
        foreground_process_id: Option<u32>,
        observations: &mut CycleObservations,
        action_log: &mut ActionLog,
    ) -> GpuPrioritySnapshot {
        if !automation_enabled {
            let failures = self.clear_all(controller, action_log, "automation disabled");
            self.failure_suppression.clear();
            return GpuPrioritySnapshot {
                enabled: false,
                failed_processes: failures.count,
                message: "Automation disabled.".to_owned(),
                last_error: failures.last_error,
                ..Default::default()
            };
        }

        if !settings.enabled {
            let failures = self.clear_all(controller, action_log, "GPU priority defaults disabled");
            self.failure_suppression.clear();
            return GpuPrioritySnapshot {
                enabled: false,
                failed_processes: failures.count,
                message: "GPU priority defaults disabled.".to_owned(),
                last_error: failures.last_error,
                ..Default::default()
            };
        }

        let foreground_sensitive = settings.foreground_detection_enabled
            && settings.foreground_priority != settings.background_priority;
        if foreground_sensitive && foreground_process_id.is_none() {
            let failures = self.clear_all(controller, action_log, "foreground app is unknown");
            return GpuPrioritySnapshot {
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
            return GpuPrioritySnapshot {
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
                return GpuPrioritySnapshot {
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
                return GpuPrioritySnapshot {
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
            let configured_override = settings.override_for(
                executable_path.to_string_lossy().as_ref(),
                foreground,
                visible_window,
            );
            let default_priority = tier.select(
                settings.foreground_priority,
                settings.visible_window_priority,
                settings.background_priority,
            );
            let priority = match configured_override {
                Some(Some(ProcessGpuPrioritySetting::Auto)) => default_priority,
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
        self.pending_context
            .retain(|path| active_target_names.contains(path));
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
        let mut suppressed_processes = 0;
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
                suppressed_processes += 1;
                continue;
            }
            claims.push(GpuPriorityClaim {
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
            "process is excluded or no longer matches GPU priority defaults",
        );
        let mut pending_processes = 0;
        let mut denied_processes = 0;
        let mut applied_log_count = 0;
        let mut pending_context_log_count = 0;
        let mut access_denied_log_count = 0;
        for claim in claims {
            let process_id = claim.target.id;
            let process_name = claim.target.name.clone();
            let executable_path = claim.target.executable_path.to_string_lossy().into_owned();
            match controller.apply_policy_claim(claim, allow_cross_session_process_control) {
                Ok(GpuPriorityApplyOutcome::Applied) => {
                    applied_log_count += 1;
                    self.failure_suppression
                        .clear_process_failure(&executable_path);
                    self.clear_process_pending_context(&executable_path);
                }
                Ok(GpuPriorityApplyOutcome::Unchanged) => {
                    self.failure_suppression
                        .clear_process_failure(&executable_path);
                    self.clear_process_pending_context(&executable_path);
                }
                Ok(GpuPriorityApplyOutcome::Preserved) => {
                    skipped_processes += 1;
                    self.failure_suppression
                        .clear_process_failure(&executable_path);
                    self.clear_process_pending_context(&executable_path);
                }
                Err(ProcessControlError::ProcessExited) => {
                    skipped_processes += 1;
                    self.clear_process_pending_context(&executable_path);
                }
                Err(ProcessControlError::AccessDenied(_)) => {
                    skipped_processes += 1;
                    denied_processes += 1;
                    self.clear_process_pending_context(&executable_path);
                    if self
                        .failure_suppression
                        .suppress_process_failure(&executable_path)
                    {
                        access_denied_log_count += 1;
                    }
                }
                Err(ProcessControlError::Unavailable(_)) => {
                    skipped_processes += 1;
                    pending_processes += 1;
                    if self.record_process_pending_context(&executable_path) {
                        pending_context_log_count += 1;
                    }
                }
                Err(error) => {
                    self.clear_process_pending_context(&executable_path);
                    self.failure_suppression
                        .record_process_failure(&executable_path);
                    failures.record("Apply", process_id, &process_name, error, action_log);
                }
            }
        }
        self.record_pending_log_summaries(
            applied_log_count,
            pending_context_log_count,
            access_denied_log_count,
            Instant::now(),
            action_log,
        );

        let adjusted_apps = controller.policy_managed_process_names();
        GpuPrioritySnapshot {
            enabled: true,
            scanned_processes,
            adjusted_processes: adjusted_apps.len(),
            skipped_processes,
            pending_processes,
            denied_processes,
            suppressed_processes,
            failed_processes: failures.count,
            adjusted_apps: unique_app_names(adjusted_apps.iter().map(String::as_str)),
            auto_excluded_processes: auto_excluded_processes.into_iter().collect(),
            message: gpu_priority_status_message(
                pending_processes,
                denied_processes,
                suppressed_processes,
                failures.count,
            ),
            last_error: failures.last_error,
        }
    }

    fn release_non_targets(
        &mut self,
        controller: &mut GpuPriorityController,
        active_targets: &BTreeSet<ProcessTargetKey>,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> GpuPriorityFailures {
        let summary = controller.release_policy_except(active_targets);
        self.record_release_summary(summary, action_log, reason)
    }

    fn clear_all(
        &mut self,
        controller: &mut GpuPriorityController,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> GpuPriorityFailures {
        let summary = controller.release_all_policy();
        let failures = self.record_release_summary(summary, action_log, reason);
        self.pending_context.clear();
        self.reset_log_summaries();
        failures
    }

    fn record_release_summary(
        &mut self,
        summary: GpuPriorityReleaseSummary,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> GpuPriorityFailures {
        let mut failures = GpuPriorityFailures::default();
        for failure in summary.failures {
            match failure.error {
                ProcessControlError::ProcessExited => {}
                ProcessControlError::AccessDenied(_) => {
                    self.failure_suppression
                        .record_process_failure(&failure.executable_path);
                    action_log.record(
                        ActionLogFeature::GpuPriority,
                        Some(failure.process_id),
                        failure.process_name,
                        ActionLogResult::Skipped,
                        format!(
                            "Skipped restoring previous GPU priority because Windows denied access: {reason}."
                        ),
                    );
                }
                ProcessControlError::Unavailable(_) => {
                    self.failure_suppression
                        .record_process_failure(&failure.executable_path);
                    action_log.record(
                        ActionLogFeature::GpuPriority,
                        Some(failure.process_id),
                        failure.process_name,
                        ActionLogResult::Skipped,
                        format!(
                            "Skipped restoring previous GPU priority because GPU scheduling priority is unavailable: {reason}."
                        ),
                    );
                }
                error => {
                    self.failure_suppression
                        .record_process_failure(&failure.executable_path);
                    failures.record(
                        "Restore",
                        failure.process_id,
                        &failure.process_name,
                        error,
                        action_log,
                    );
                }
            }
        }
        if summary.restored_processes > 0 {
            action_log.record(
                ActionLogFeature::GpuPriority,
                None,
                "GPU Priority",
                ActionLogResult::Restored,
                format!(
                    "Restored previous GPU priority for {}: {reason}.",
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
                ActionLogFeature::GpuPriority,
                Some(process_id),
                process_name.to_owned(),
                ActionLogResult::Skipped,
                format!(
                    "Stopped retrying GPU Priority after {} failed attempts.",
                    execution_failure_suppression_threshold(),
                ),
            );
        }
        true
    }

    fn record_process_pending_context(&mut self, executable_path: &str) -> bool {
        self.pending_context
            .insert(process_failure_key(executable_path))
    }

    fn clear_process_pending_context(&mut self, executable_path: &str) {
        self.pending_context
            .remove(&process_failure_key(executable_path));
    }

    fn record_pending_log_summaries(
        &mut self,
        applied_count: usize,
        pending_context_count: usize,
        access_denied_count: usize,
        now: Instant,
        action_log: &mut ActionLog,
    ) {
        self.pending_apply_log_count += applied_count;
        self.pending_context_log_count += pending_context_count;
        self.pending_access_denied_log_count += access_denied_count;

        if self.pending_apply_log_count > 0
            && gpu_priority_summary_log_due(self.last_apply_summary_logged_at, now)
        {
            let count = std::mem::take(&mut self.pending_apply_log_count);
            self.last_apply_summary_logged_at = Some(now);
            action_log.record(
                ActionLogFeature::GpuPriority,
                None,
                "GPU Priority",
                ActionLogResult::Applied,
                gpu_priority_apply_summary_message(count),
            );
        }

        if (self.pending_context_log_count > 0 || self.pending_access_denied_log_count > 0)
            && gpu_priority_summary_log_due(self.last_skip_summary_logged_at, now)
        {
            let pending_context_count = std::mem::take(&mut self.pending_context_log_count);
            let access_denied_count = std::mem::take(&mut self.pending_access_denied_log_count);
            self.last_skip_summary_logged_at = Some(now);
            action_log.record(
                ActionLogFeature::GpuPriority,
                None,
                "GPU Priority",
                ActionLogResult::Skipped,
                gpu_priority_skip_summary_message(pending_context_count, access_denied_count),
            );
        }
    }

    fn reset_log_summaries(&mut self) {
        self.pending_apply_log_count = 0;
        self.pending_context_log_count = 0;
        self.pending_access_denied_log_count = 0;
        self.last_apply_summary_logged_at = None;
        self.last_skip_summary_logged_at = None;
    }
}

fn gpu_priority_apply_summary_message(count: usize) -> String {
    format!("Applied GPU priority to {}.", process_count_label(count))
}

fn gpu_priority_skip_summary_message(
    pending_context_count: usize,
    access_denied_count: usize,
) -> String {
    let total = pending_context_count + access_denied_count;
    match (pending_context_count, access_denied_count) {
        (pending, 0) => format!(
            "Skipped GPU priority for {}: waiting for GPU scheduling context.",
            process_count_label(pending)
        ),
        (0, denied) => format!(
            "Skipped GPU priority for {}: Windows denied access.",
            process_count_label(denied)
        ),
        (pending, denied) => format!(
            "Skipped GPU priority for {}: {} waiting for GPU scheduling context, {} denied access.",
            process_count_label(total),
            process_count_label(pending),
            process_count_label(denied)
        ),
    }
}

fn gpu_priority_summary_log_due(last_logged_at: Option<Instant>, now: Instant) -> bool {
    last_logged_at.is_none_or(|last| now.duration_since(last) >= GPU_PRIORITY_SUMMARY_LOG_INTERVAL)
}

fn gpu_priority_status_message(
    pending_processes: usize,
    denied_processes: usize,
    suppressed_processes: usize,
    failed_processes: usize,
) -> String {
    if failed_processes > 0 {
        "GPU priority defaults active with failures.".to_owned()
    } else if denied_processes > 0 {
        "GPU priority defaults active; some protected processes were skipped.".to_owned()
    } else if suppressed_processes > 0 {
        "GPU priority defaults active; repeated failures are being suppressed.".to_owned()
    } else if pending_processes > 0 {
        "GPU priority defaults active; waiting for GPU scheduling contexts.".to_owned()
    } else {
        "GPU priority defaults active.".to_owned()
    }
}

fn tier_preservation(
    settings: &GpuPrioritySettings,
    tier: PriorityProcessTier,
) -> GpuPriorityPreservation {
    match tier {
        PriorityProcessTier::Foreground if settings.preserve_foreground_priority => {
            GpuPriorityPreservation::PreserveHigher
        }
        PriorityProcessTier::VisibleWindow if settings.preserve_visible_window_priority => {
            GpuPriorityPreservation::PreserveHigher
        }
        PriorityProcessTier::Background if settings.preserve_background_priority => {
            GpuPriorityPreservation::PreserveLower
        }
        _ => GpuPriorityPreservation::Exact,
    }
}

#[derive(Default)]
struct GpuPriorityFailures {
    count: usize,
    last_error: Option<String>,
}

impl GpuPriorityFailures {
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
            ActionLogFeature::GpuPriority,
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
    fn repeated_failures_emit_one_gpu_auto_exclusion_and_success_resets_it() {
        let mut manager = GpuPriorityManager::default();
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
        assert_eq!(log.entries()[0].feature, ActionLogFeature::GpuPriority);

        manager.failure_suppression.clear_process_failure(path);
        auto_excluded.clear();
        assert!(!manager.is_process_suppressed(42, "app.exe", path, &mut log, &mut auto_excluded,));
    }

    #[test]
    fn missing_gpu_context_is_deduplicated_without_failure_suppression() {
        let mut manager = GpuPriorityManager::default();
        let path = r"C:\Apps\app.exe";

        assert!(manager.record_process_pending_context(path));
        assert!(!manager.record_process_pending_context(r"C:/Apps/app.exe"));
        assert!(
            !manager
                .failure_suppression
                .process_suppression(path)
                .suppressed
        );

        manager.clear_process_pending_context(path);
        assert!(manager.record_process_pending_context(path));
    }

    #[test]
    fn gpu_summary_logs_are_rate_limited_and_accumulate_counts() {
        let mut manager = GpuPriorityManager::default();
        let mut log = ActionLog::new(8);
        let now = Instant::now();

        manager.record_pending_log_summaries(1, 1, 0, now, &mut log);
        assert_eq!(log.entries().len(), 2);
        manager.record_pending_log_summaries(2, 2, 1, now + Duration::from_secs(1), &mut log);
        assert_eq!(log.entries().len(), 2);
        manager.record_pending_log_summaries(
            0,
            0,
            0,
            now + GPU_PRIORITY_SUMMARY_LOG_INTERVAL,
            &mut log,
        );

        assert_eq!(log.entries().len(), 4);
        assert!(log
            .entries()
            .iter()
            .any(|entry| entry.reason.contains("2 processes")));
        assert!(log
            .entries()
            .iter()
            .any(|entry| entry.reason.contains("3 processes")));
    }

    #[test]
    fn focus_visible_and_background_preservation_directions_are_explicit() {
        let mut settings = GpuPrioritySettings::default();
        assert_eq!(
            tier_preservation(&settings, PriorityProcessTier::Foreground),
            GpuPriorityPreservation::PreserveHigher
        );
        assert_eq!(
            tier_preservation(&settings, PriorityProcessTier::VisibleWindow),
            GpuPriorityPreservation::PreserveHigher
        );
        assert_eq!(
            tier_preservation(&settings, PriorityProcessTier::Background),
            GpuPriorityPreservation::PreserveLower
        );
        settings.preserve_visible_window_priority = false;
        assert_eq!(
            tier_preservation(&settings, PriorityProcessTier::VisibleWindow),
            GpuPriorityPreservation::Exact
        );
    }
}
