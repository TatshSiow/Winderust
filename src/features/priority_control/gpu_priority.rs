use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use windows_sys::Win32::System::Threading::GetCurrentProcessId;

use crate::{
    action_log::{ActionLog, ActionLogFeature, ActionLogResult},
    config::{GpuPrioritySettings, ProcessGpuPriority},
    control::{
        gpu_priority::{
            GpuPriorityApplyOutcome, GpuPriorityClaim, GpuPriorityController,
            GpuPriorityPreservation, GpuPriorityReleaseSummary,
        },
        process::{ControlOwner, ProcessControlError, ProcessControlTarget, ProcessTargetKey},
    },
    foreground::{
        contains_process_name, process_count_label, process_executable_path, process_failure_key,
        process_session_id, ProtectedProcesses, CORE_BUILT_IN_PROCESS_EXCLUSIONS,
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
    pub denied_processes: usize,
    pub failed_processes: usize,
    pub auto_excluded_processes: Vec<String>,
    pub(crate) observation_paused: bool,
    pub last_error: Option<String>,
}

#[derive(Default)]
pub struct GpuPriorityManager {
    failure_suppression: ExecutionFailureTracker,
    non_release_failures: GpuPriorityFailures,
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
        self.non_release_failures = GpuPriorityFailures::default();
        if !automation_enabled || !settings.enabled {
            let reason = if automation_enabled {
                "GPU priority defaults disabled"
            } else {
                "automation disabled"
            };
            let failures = self.clear_all(controller, action_log, reason);
            self.failure_suppression.clear();
            return GpuPrioritySnapshot {
                failed_processes: failures.count,
                last_error: failures.last_error,
                ..Default::default()
            };
        }

        let focus_unknown =
            settings.foreground_detection_enabled && foreground_process_id.is_none();
        if focus_unknown && settings.foreground_priority != settings.background_priority {
            return self.paused_snapshot(controller, action_log, "foreground app is unknown", None);
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

        let processes = match observations.processes_with_paths() {
            Ok(processes) => processes,
            Err(err) => {
                return self.paused_snapshot(
                    controller,
                    action_log,
                    "process list unavailable",
                    Some(err),
                );
            }
        };

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

        let scanned_processes = processes.len();
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
            let foreground = settings.foreground_detection_enabled
                && super::foreground_for_owner(
                    owner,
                    process,
                    path,
                    foreground_process_id,
                    foreground_executable_path.as_deref(),
                    adaptive_workload.as_deref(),
                );
            let visible_window = !foreground
                && settings.visible_window_detection_enabled
                && visible_processes.contains(process.id, path);
            let tier = PriorityProcessTier::from_flags(foreground, visible_window);
            let policy = priority_policy(settings, &executable_path, tier);
            if focus_unknown
                && policy
                    != priority_policy(settings, &executable_path, PriorityProcessTier::Foreground)
            {
                return self.paused_snapshot(
                    controller,
                    action_log,
                    "foreground app is unknown",
                    None,
                );
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
        self.pending_context
            .retain(|path| active_target_names.contains(path));
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
                GpuPriorityClaim {
                    target: ProcessControlTarget::automatic(
                        process_id,
                        process_name,
                        PathBuf::from(executable_path.as_str()),
                        creation_time,
                    ),
                    owner,
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
            "process is excluded or no longer matches GPU priority defaults",
        );
        let mut denied_processes = 0;
        let mut applied_log_count = 0;
        let mut pending_context_log_count = 0;
        let mut access_denied_log_count = 0;
        for (claim, executable_path) in claims {
            let process_id = claim.target.id;
            let process_name = claim.target.name.clone();
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
                    if self.record_process_pending_context(&executable_path) {
                        pending_context_log_count += 1;
                    }
                }
                Err(error) => {
                    self.clear_process_pending_context(&executable_path);
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
        self.record_pending_log_summaries(
            applied_log_count,
            pending_context_log_count,
            access_denied_log_count,
            Instant::now(),
            action_log,
        );

        let failures = self.with_non_release_failures(release_failures);
        GpuPrioritySnapshot {
            enabled: true,
            scanned_processes,
            adjusted_processes: controller.policy_managed_process_count(),
            skipped_processes,
            denied_processes,
            failed_processes: failures.count,
            auto_excluded_processes: auto_excluded_processes.into_iter().collect(),
            observation_paused: false,
            last_error: failures.last_error,
        }
    }

    fn paused_snapshot(
        &mut self,
        controller: &mut GpuPriorityController,
        action_log: &mut ActionLog,
        reason: &str,
        observation_error: Option<String>,
    ) -> GpuPrioritySnapshot {
        let failures = self.clear_all(controller, action_log, reason);
        self.non_release_failures.last_error = observation_error;
        let failures = self.with_non_release_failures(failures);
        GpuPrioritySnapshot {
            enabled: true,
            observation_paused: true,
            failed_processes: failures.count,
            last_error: failures.last_error,
            ..Default::default()
        }
    }

    pub(crate) fn report_release_retry(
        &mut self,
        controller: &GpuPriorityController,
        summary: GpuPriorityReleaseSummary,
        mut status: GpuPrioritySnapshot,
        action_log: &mut ActionLog,
    ) -> GpuPrioritySnapshot {
        let failures =
            self.record_release_summary(summary, action_log, "restoration retry completed", false);
        let failures = self.with_non_release_failures(failures);
        status.auto_excluded_processes.clear();
        status.failed_processes = failures.count;
        status.last_error = failures.last_error;
        if status.enabled && !status.observation_paused {
            status.adjusted_processes = controller.policy_managed_process_count();
        }
        status
    }

    fn with_non_release_failures(&self, release: GpuPriorityFailures) -> GpuPriorityFailures {
        GpuPriorityFailures {
            count: release.count + self.non_release_failures.count,
            last_error: release
                .last_error
                .or_else(|| self.non_release_failures.last_error.clone()),
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
        self.record_release_summary(summary, action_log, reason, true)
    }

    fn clear_all(
        &mut self,
        controller: &mut GpuPriorityController,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> GpuPriorityFailures {
        let summary = controller.release_all_policy();
        let failures = self.record_release_summary(summary, action_log, reason, true);
        self.pending_context.clear();
        self.reset_log_summaries();
        failures
    }

    fn record_release_summary(
        &mut self,
        summary: GpuPriorityReleaseSummary,
        action_log: &mut ActionLog,
        reason: &str,
        report_failures: bool,
    ) -> GpuPriorityFailures {
        let mut failures = GpuPriorityFailures::default();
        for failure in summary.failures {
            if failure.error == ProcessControlError::ProcessExited {
                continue;
            }
            if let (true, Some(executable_path)) =
                (report_failures, failure.executable_path.to_str())
            {
                self.failure_suppression
                    .record_process_failure(executable_path);
            }
            let skip_reason = match &failure.error {
                ProcessControlError::AccessDenied(_) => Some("Windows denied access"),
                ProcessControlError::Unavailable(_) => {
                    Some("GPU scheduling priority is unavailable")
                }
                _ => None,
            };
            if let Some(skip_reason) = skip_reason {
                if report_failures {
                    action_log.record(
                        ActionLogFeature::GpuPriority,
                        Some(failure.process_id),
                        failure.process_name,
                        ActionLogResult::Skipped,
                        format!("Skipped restoring previous GPU priority because {skip_reason}: {reason}."),
                    );
                }
            } else {
                failures.record(
                    "Restore",
                    failure.process_id,
                    &failure.process_name,
                    failure.error,
                    report_failures.then_some(&mut *action_log),
                );
            }
        }
        if summary.restored_processes > 0 {
            action_log.record(
                ActionLogFeature::GpuPriority,
                None,
                "",
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
                "",
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
                "",
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
    format!("GPU priority updated for {}.", process_count_label(count))
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

fn priority_policy(
    settings: &GpuPrioritySettings,
    path: &str,
    tier: PriorityProcessTier,
) -> Option<(ProcessGpuPriority, GpuPriorityPreservation)> {
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
        action_log: Option<&mut ActionLog>,
    ) {
        let message = error.to_string();
        if self.last_error.is_none() {
            self.last_error = Some(format!("{action} {process_name} ({process_id}): {message}"));
        }
        self.count += 1;
        if let Some(action_log) = action_log {
            action_log.record(
                ActionLogFeature::GpuPriority,
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
