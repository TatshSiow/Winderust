use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};

use windows_sys::{
    Wdk::Graphics::Direct3D::{
        D3DKMTGetProcessSchedulingPriorityClass, D3DKMTSetProcessSchedulingPriorityClass,
        D3DKMT_SCHEDULINGPRIORITYCLASS,
    },
    Win32::{
        Foundation::{ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER},
        System::Threading::{
            GetCurrentProcessId, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
            PROCESS_SET_INFORMATION,
        },
    },
};

use crate::win_util::{last_error, WinHandle};

use crate::{
    action_log::{ActionLog, ActionLogAction, ActionLogFeature, ActionLogResult},
    config::{GpuPrioritySettings, ProcessGpuPriority, ProcessGpuPrioritySetting},
    foreground::{
        contains_process_name, is_foreground_process, is_process_exited_message, list_processes,
        process_count_label, process_failure_key, process_names_by_id, process_session_id,
        same_process_name, unique_app_names, CORE_BUILT_IN_PROCESS_EXCLUSIONS,
    },
    rules::ExecutionFailureTracker,
};

const STATUS_PROCESS_IS_TERMINATING: u32 = 0xC000010A;
const STATUS_INVALID_PARAMETER: u32 = 0xC000000D;
const GPU_PRIORITY_SUMMARY_LOG_INTERVAL: Duration = Duration::from_secs(30);

const BUILT_IN_EXCLUSIONS: &[&str] = CORE_BUILT_IN_PROCESS_EXCLUSIONS;

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
    adjusted: BTreeMap<u32, AdjustedProcess>,
    failure_suppression: ExecutionFailureTracker,
    pending_context: BTreeSet<String>,
    pending_apply_log_count: usize,
    pending_context_log_count: usize,
    pending_access_denied_log_count: usize,
    last_apply_summary_logged_at: Option<Instant>,
    last_skip_summary_logged_at: Option<Instant>,
}

#[derive(Clone)]
struct AdjustedProcess {
    process_name: String,
    previous_priority_raw: u32,
    applied_priority: ProcessGpuPriority,
}

#[derive(Debug)]
enum GpuPriorityError {
    AccessDenied,
    ProcessExited,
    GpuContextUnavailable,
    Failed(String),
}

impl GpuPriorityManager {
    pub fn update(
        &mut self,
        settings: &GpuPrioritySettings,
        automation_enabled: bool,
        foreground_process_id: Option<u32>,
        action_log: &mut ActionLog,
    ) -> GpuPrioritySnapshot {
        if !automation_enabled {
            let failures = self.clear_all(action_log, "automation disabled");
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
            let failures = self.clear_all(action_log, "GPU priority defaults disabled");
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
            let failures = self.clear_all(action_log, "foreground app is unknown");
            return GpuPrioritySnapshot {
                enabled: true,
                failed_processes: failures.count,
                message: "Paused: foreground app is unknown.".to_owned(),
                last_error: failures.last_error,
                ..Default::default()
            };
        }

        let current_process_id = unsafe { GetCurrentProcessId() };
        let Some(current_session_id) = process_session_id(current_process_id) else {
            let failures = self.clear_all(action_log, "current Windows session is unknown");
            return GpuPrioritySnapshot {
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
                return GpuPrioritySnapshot {
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
                Some(Some(ProcessGpuPrioritySetting::Auto)) if foreground => {
                    settings.foreground_priority
                }
                Some(Some(ProcessGpuPrioritySetting::Auto)) => settings.background_priority,
                Some(Some(priority)) => priority,
                Some(None) => continue,
                None if foreground => settings.foreground_priority,
                None => settings.background_priority,
            };
            if let Some(priority) = priority.priority() {
                target_processes.insert(process.id, (process.name, priority, foreground));
            }
        }

        let target_ids = target_processes.keys().copied().collect::<BTreeSet<_>>();
        let active_target_names = target_processes
            .values()
            .map(|(name, _priority, _foreground)| process_failure_key(name))
            .collect::<BTreeSet<_>>();
        self.failure_suppression.retain_keys(&active_target_names);
        self.pending_context
            .retain(|process_name| active_target_names.contains(process_name));

        let mut failures = self.release_non_targets(
            &target_ids,
            &current_process_names,
            action_log,
            "process is excluded or no longer matches GPU priority defaults",
        );
        let mut skipped_processes = 0;
        let mut pending_processes = 0;
        let mut denied_processes = 0;
        let mut suppressed_processes = 0;
        let mut applied_log_count = 0;
        let mut pending_context_log_count = 0;
        let mut access_denied_log_count = 0;
        let mut auto_excluded_processes = BTreeSet::new();

        for (process_id, (process_name, priority, foreground)) in target_processes {
            if self.is_process_suppressed(&process_name, &mut auto_excluded_processes) {
                skipped_processes += 1;
                suppressed_processes += 1;
                continue;
            }

            match self.apply_process(
                process_id,
                process_name.clone(),
                priority,
                foreground,
                settings.preserve_foreground_priority,
                settings.preserve_background_priority,
            ) {
                Ok(ApplyOutcome::Applied { loggable }) => {
                    if loggable {
                        applied_log_count += 1;
                    }
                    self.clear_process_failure(&process_name);
                    self.clear_process_pending_context(&process_name);
                }
                Ok(ApplyOutcome::AlreadyApplied) => {
                    self.clear_process_failure(&process_name);
                    self.clear_process_pending_context(&process_name);
                }
                Ok(ApplyOutcome::Preserved) => {
                    skipped_processes += 1;
                    self.clear_process_failure(&process_name);
                    self.clear_process_pending_context(&process_name);
                }
                Err(GpuPriorityError::ProcessExited) => {
                    skipped_processes += 1;
                }
                Err(GpuPriorityError::AccessDenied) => {
                    skipped_processes += 1;
                    denied_processes += 1;
                    self.clear_process_pending_context(&process_name);
                    if self.record_process_failure(&process_name) {
                        access_denied_log_count += 1;
                    }
                }
                Err(GpuPriorityError::GpuContextUnavailable) => {
                    skipped_processes += 1;
                    pending_processes += 1;
                    if self.record_process_pending_context(&process_name) {
                        pending_context_log_count += 1;
                    }
                }
                Err(err) => {
                    self.clear_process_pending_context(&process_name);
                    self.record_process_failure(&process_name);
                    failures.record("Apply", process_id, &process_name, err, action_log);
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

        GpuPrioritySnapshot {
            enabled: true,
            scanned_processes,
            adjusted_processes: self.adjusted.len(),
            skipped_processes,
            pending_processes,
            denied_processes,
            suppressed_processes,
            failed_processes: failures.count,
            adjusted_apps: unique_app_names(
                self.adjusted
                    .values()
                    .map(|process| process.process_name.as_str()),
            ),
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

    fn apply_process(
        &mut self,
        process_id: u32,
        process_name: String,
        priority: ProcessGpuPriority,
        foreground: bool,
        preserve_foreground: bool,
        preserve_background: bool,
    ) -> Result<ApplyOutcome, GpuPriorityError> {
        let process = ProcessHandle::open(process_id)?;
        let reusable_existing = self
            .adjusted
            .get(&process_id)
            .filter(|adjusted| same_process_name(&adjusted.process_name, &process_name));
        let current_priority_raw = process.gpu_priority_raw()?;
        let desired_priority_raw = gpu_priority_raw(priority);
        let baseline_priority_raw = reusable_existing
            .map(|adjusted| adjusted.previous_priority_raw)
            .unwrap_or(current_priority_raw);
        if should_preserve_priority(
            foreground,
            preserve_foreground,
            preserve_background,
            baseline_priority_raw,
            desired_priority_raw,
        ) {
            if let Some(adjusted) = self.adjusted.remove(&process_id) {
                process.set_gpu_priority_raw(adjusted.previous_priority_raw)?;
            }
            return Ok(ApplyOutcome::Preserved);
        }

        let changed = current_priority_raw != desired_priority_raw;
        let loggable = reusable_existing
            .map(|adjusted| adjusted.applied_priority != priority)
            .unwrap_or(true);

        if reusable_existing.is_some_and(|adjusted| {
            adjusted.applied_priority == priority && current_priority_raw == desired_priority_raw
        }) {
            return Ok(ApplyOutcome::AlreadyApplied);
        }

        if changed {
            process.set_gpu_priority_raw(desired_priority_raw)?;
            let refreshed_priority_raw = process.gpu_priority_raw()?;
            if refreshed_priority_raw != desired_priority_raw {
                return Err(GpuPriorityError::Failed(format!(
                    "GPU priority remained {} after requesting {}.",
                    gpu_priority_raw_label(refreshed_priority_raw),
                    gpu_priority_label(priority)
                )));
            }
        } else if reusable_existing.is_none() {
            return Ok(ApplyOutcome::AlreadyApplied);
        }

        self.adjusted.insert(
            process_id,
            AdjustedProcess {
                process_name,
                previous_priority_raw: baseline_priority_raw,
                applied_priority: priority,
            },
        );
        Ok(ApplyOutcome::Applied {
            loggable: loggable && changed,
        })
    }

    fn release_non_targets(
        &mut self,
        target_ids: &BTreeSet<u32>,
        current_process_names: &BTreeMap<u32, String>,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> GpuPriorityFailures {
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

    fn clear_all(&mut self, action_log: &mut ActionLog, reason: &str) -> GpuPriorityFailures {
        let process_ids = self.adjusted.keys().copied().collect::<Vec<_>>();
        let failures = self.release_processes(&process_ids, None, action_log, reason);
        self.pending_context.clear();
        self.reset_log_summaries();
        failures
    }

    fn release_processes(
        &mut self,
        process_ids: &[u32],
        current_process_names: Option<&BTreeMap<u32, String>>,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> GpuPriorityFailures {
        let mut failures = GpuPriorityFailures::default();
        for process_id in process_ids {
            let Some(process_state) = self.adjusted.remove(process_id) else {
                continue;
            };
            let log_name = current_process_names
                .and_then(|names| names.get(process_id))
                .cloned()
                .unwrap_or_else(|| process_state.process_name.clone());
            match restore_process(*process_id, process_state) {
                Ok(()) => {
                    self.clear_process_failure(&log_name);
                    action_log.record(
                        ActionLogFeature::GpuPriority,
                        Some(*process_id),
                        log_name,
                        ActionLogAction::Restore,
                        ActionLogResult::Restored,
                        format!("Restored previous GPU priority: {reason}."),
                    );
                }
                Err(GpuPriorityError::ProcessExited) => {}
                Err(GpuPriorityError::AccessDenied) => {
                    self.record_process_failure(&log_name);
                    action_log.record(
                        ActionLogFeature::GpuPriority,
                        Some(*process_id),
                        log_name,
                        ActionLogAction::Skip,
                        ActionLogResult::Skipped,
                        format!(
                            "Skipped restoring previous GPU priority because Windows denied access: {reason}."
                        ),
                    );
                }
                Err(GpuPriorityError::GpuContextUnavailable) => {
                    self.record_process_failure(&log_name);
                    action_log.record(
                        ActionLogFeature::GpuPriority,
                        Some(*process_id),
                        log_name,
                        ActionLogAction::Skip,
                        ActionLogResult::Skipped,
                        format!(
                            "Skipped restoring previous GPU priority because GPU scheduling priority is unavailable: {reason}."
                        ),
                    );
                }
                Err(err) => {
                    self.record_process_failure(&log_name);
                    failures.record("Restore", *process_id, &log_name, err, action_log);
                }
            }
        }
        failures
    }

    fn is_process_suppressed(
        &mut self,
        process_name: &str,
        auto_excluded_processes: &mut BTreeSet<String>,
    ) -> bool {
        let suppression = self.failure_suppression.process_suppression(process_name);
        if suppression.newly_suppressed {
            auto_excluded_processes.insert(process_failure_key(process_name));
        }
        suppression.suppressed
    }

    fn record_process_failure(&mut self, process_name: &str) -> bool {
        self.failure_suppression
            .record_process_failure(process_name)
    }

    fn clear_process_failure(&mut self, process_name: &str) {
        self.failure_suppression.clear_process_failure(process_name);
    }

    fn record_process_pending_context(&mut self, process_name: &str) -> bool {
        self.pending_context
            .insert(process_failure_key(process_name))
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
                ActionLogAction::Apply,
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
                ActionLogAction::Skip,
                ActionLogResult::Skipped,
                gpu_priority_skip_summary_message(pending_context_count, access_denied_count),
            );
        }
    }

    fn clear_process_pending_context(&mut self, process_name: &str) {
        self.pending_context
            .remove(&process_failure_key(process_name));
    }

    fn reset_log_summaries(&mut self) {
        self.pending_apply_log_count = 0;
        self.pending_context_log_count = 0;
        self.pending_access_denied_log_count = 0;
        self.last_apply_summary_logged_at = None;
        self.last_skip_summary_logged_at = None;
    }
}

enum ApplyOutcome {
    Applied { loggable: bool },
    AlreadyApplied,
    Preserved,
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
        error: GpuPriorityError,
        action_log: &mut ActionLog,
    ) {
        let message = gpu_priority_error_message(error);
        if is_process_exited_message(&message) {
            return;
        }
        if self.last_error.is_none() {
            self.last_error = Some(format!("{action} {process_name} ({process_id}): {message}"));
        }
        self.count += 1;
        action_log.record(
            ActionLogFeature::GpuPriority,
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
    fn open(process_id: u32) -> Result<Self, GpuPriorityError> {
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

    fn gpu_priority_raw(&self) -> Result<u32, GpuPriorityError> {
        let mut priority = 0;
        let status = unsafe {
            D3DKMTGetProcessSchedulingPriorityClass(self.0.raw(), &mut priority as *mut _)
        };
        ntstatus_result(status).and_then(|()| {
            u32::try_from(priority).map_err(|_| {
                GpuPriorityError::Failed(format!("Unexpected GPU priority {priority}."))
            })
        })
    }

    fn set_gpu_priority_raw(&self, priority: u32) -> Result<(), GpuPriorityError> {
        let priority = D3DKMT_SCHEDULINGPRIORITYCLASS::try_from(priority)
            .map_err(|_| GpuPriorityError::Failed(format!("Invalid GPU priority {priority}.")))?;
        let status = unsafe { D3DKMTSetProcessSchedulingPriorityClass(self.0.raw(), priority) };
        ntstatus_result(status)
    }
}

fn restore_process(
    process_id: u32,
    process_state: AdjustedProcess,
) -> Result<(), GpuPriorityError> {
    let process = ProcessHandle::open(process_id)?;
    process.set_gpu_priority_raw(process_state.previous_priority_raw)?;
    let refreshed_priority_raw = process.gpu_priority_raw()?;
    if refreshed_priority_raw == process_state.previous_priority_raw {
        Ok(())
    } else {
        Err(GpuPriorityError::Failed(format!(
            "GPU priority remained {} after restoring {}.",
            gpu_priority_raw_label(refreshed_priority_raw),
            gpu_priority_raw_label(process_state.previous_priority_raw)
        )))
    }
}

fn ntstatus_result(status: i32) -> Result<(), GpuPriorityError> {
    if status >= 0 {
        Ok(())
    } else {
        match status as u32 {
            STATUS_PROCESS_IS_TERMINATING => Err(GpuPriorityError::ProcessExited),
            STATUS_INVALID_PARAMETER => Err(GpuPriorityError::GpuContextUnavailable),
            status => Err(GpuPriorityError::Failed(format!(
                "NTSTATUS 0x{status:08X}."
            ))),
        }
    }
}

fn open_process_error(process_id: u32, error: u32) -> GpuPriorityError {
    match error {
        ERROR_ACCESS_DENIED => GpuPriorityError::AccessDenied,
        ERROR_INVALID_PARAMETER => GpuPriorityError::ProcessExited,
        _ => GpuPriorityError::Failed(format!(
            "OpenProcess({process_id}) failed with error {error}."
        )),
    }
}

fn gpu_priority_raw(priority: ProcessGpuPriority) -> u32 {
    match priority {
        ProcessGpuPriority::Realtime => 5,
        ProcessGpuPriority::High => 4,
        ProcessGpuPriority::AboveNormal => 3,
        ProcessGpuPriority::Normal => 2,
        ProcessGpuPriority::Idle => 0,
        ProcessGpuPriority::BelowNormal => 1,
    }
}

fn should_preserve_priority(
    foreground: bool,
    preserve_foreground: bool,
    preserve_background: bool,
    current_rank: u32,
    desired_rank: u32,
) -> bool {
    if foreground {
        preserve_foreground && current_rank >= desired_rank
    } else {
        preserve_background && current_rank <= desired_rank
    }
}

pub fn gpu_priority_label(priority: ProcessGpuPriority) -> &'static str {
    match priority {
        ProcessGpuPriority::Realtime => "Realtime",
        ProcessGpuPriority::High => "High",
        ProcessGpuPriority::AboveNormal => "Above Normal",
        ProcessGpuPriority::Normal => "Normal",
        ProcessGpuPriority::BelowNormal => "Below Normal",
        ProcessGpuPriority::Idle => "Idle",
    }
}

fn gpu_priority_raw_label(priority: u32) -> String {
    match priority {
        0 => "Idle".to_owned(),
        1 => "Below Normal".to_owned(),
        2 => "Normal".to_owned(),
        3 => "Above Normal".to_owned(),
        4 => "High".to_owned(),
        5 => "Realtime".to_owned(),
        other => format!("Unknown ({other})"),
    }
}

fn gpu_priority_error_message(error: GpuPriorityError) -> String {
    match error {
        GpuPriorityError::AccessDenied => "Access denied.".to_owned(),
        GpuPriorityError::ProcessExited => "Process exited.".to_owned(),
        GpuPriorityError::GpuContextUnavailable => {
            "GPU scheduling priority is not available yet for this process.".to_owned()
        }
        GpuPriorityError::Failed(message) => message,
    }
}

fn gpu_priority_apply_summary_message(count: usize) -> String {
    if count == 1 {
        "Applied GPU priority to 1 process.".to_owned()
    } else {
        format!("Applied GPU priority to {count} processes.")
    }
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

pub fn is_builtin_excluded(process_name: &str) -> bool {
    contains_process_name(BUILT_IN_EXCLUSIONS, process_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gpu_priority_raw_values_match_wddm_order() {
        assert_eq!(gpu_priority_raw(ProcessGpuPriority::Idle), 0);
        assert_eq!(gpu_priority_raw(ProcessGpuPriority::BelowNormal), 1);
        assert_eq!(gpu_priority_raw(ProcessGpuPriority::Normal), 2);
        assert_eq!(gpu_priority_raw(ProcessGpuPriority::AboveNormal), 3);
        assert_eq!(gpu_priority_raw(ProcessGpuPriority::High), 4);
        assert_eq!(gpu_priority_raw(ProcessGpuPriority::Realtime), 5);
    }

    #[test]
    fn d3dkmt_invalid_parameter_waits_for_gpu_context_retry() {
        assert!(matches!(
            ntstatus_result(STATUS_INVALID_PARAMETER as i32),
            Err(GpuPriorityError::GpuContextUnavailable)
        ));
    }

    #[test]
    fn pending_gpu_context_does_not_suppress_future_retries() {
        let mut manager = GpuPriorityManager::default();

        assert!(manager.record_process_pending_context("game.exe"));
        assert!(!manager.record_process_pending_context("GAME.exe"));

        assert!(!manager.is_process_suppressed("game.exe", &mut BTreeSet::new()));
    }

    #[test]
    fn repeated_process_failures_suppress_gpu_priority_retries() {
        let mut manager = GpuPriorityManager::default();

        assert!(manager.record_process_failure("APP.exe"));
        assert!(!manager.record_process_failure("app.exe"));
        assert!(!manager.is_process_suppressed("app.exe", &mut BTreeSet::new()));

        assert!(!manager.record_process_failure("app.exe"));
        assert!(manager.is_process_suppressed("app.exe", &mut BTreeSet::new()));
        assert!(manager.is_process_suppressed("APP.exe", &mut BTreeSet::new()));
    }

    #[test]
    fn gpu_priority_summary_messages_use_process_counts() {
        assert_eq!(
            gpu_priority_apply_summary_message(1),
            "Applied GPU priority to 1 process."
        );
        assert_eq!(
            gpu_priority_apply_summary_message(3),
            "Applied GPU priority to 3 processes."
        );
        assert_eq!(
            gpu_priority_skip_summary_message(2, 0),
            "Skipped GPU priority for 2 processes: waiting for GPU scheduling context."
        );
        assert_eq!(
            gpu_priority_skip_summary_message(0, 1),
            "Skipped GPU priority for 1 process: Windows denied access."
        );
        assert_eq!(
            gpu_priority_skip_summary_message(2, 1),
            "Skipped GPU priority for 3 processes: 2 processes waiting for GPU scheduling context, 1 process denied access."
        );
    }

    #[test]
    fn gpu_priority_summary_log_is_rate_limited() {
        let now = Instant::now();

        assert!(gpu_priority_summary_log_due(None, now));
        assert!(!gpu_priority_summary_log_due(Some(now), now));
        assert!(!gpu_priority_summary_log_due(
            Some(now),
            now + GPU_PRIORITY_SUMMARY_LOG_INTERVAL - Duration::from_millis(1)
        ));
        assert!(gpu_priority_summary_log_due(
            Some(now),
            now + GPU_PRIORITY_SUMMARY_LOG_INTERVAL
        ));
    }
}
