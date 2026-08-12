use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    time::{Duration, Instant},
};

use windows_sys::Win32::{
    Foundation::{ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER, FILETIME},
    System::Threading::{
        GetCurrentProcessId, GetProcessTimes, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    },
};

use crate::{
    action_log::{ActionLog, ActionLogFeature, ActionLogResult},
    config::{CoreLimiterRule, CoreLimiterSettings},
    control::{
        cpu_allocation::{
            CpuAllocationApplyOutcome, CpuAllocationClaim, CpuAllocationCoordinator,
            CpuAllocationReleaseSummary, CpuAllocationRequest,
        },
        process::{ControlOwner, ProcessControlError, ProcessControlTarget},
    },
    cpu::{process_cpu_usage_percent, ProcessCpuSample},
    foreground::{
        contains_process_name, process_executable_path, process_failure_key,
        process_handle_matches_executable_path, process_session_id, same_executable_path,
        ProtectedProcesses, EXTENDED_BUILT_IN_PROCESS_EXCLUSIONS,
    },
    rules::{execution_failure_suppression_threshold, ExecutionFailureTracker},
    runtime::observations::CycleObservations,
    win_util::{filetime_to_u64, last_error, WinHandle},
};

use super::cpu_allocation::{
    cpu_allocation_action_log_context, record_cpu_allocation_restorations,
};

const BUILT_IN_EXCLUSIONS: &[&str] = EXTENDED_BUILT_IN_PROCESS_EXCLUSIONS;
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreLimiterSnapshot {
    pub enabled: bool,
    pub scanned_processes: usize,
    pub limited_processes: usize,
    pub tracked_processes: usize,
    pub skipped_processes: usize,
    pub failed_processes: usize,
    pub limited_apps: Vec<String>,
    pub auto_excluded_processes: Vec<String>,
    pub message: String,
    pub last_error: Option<String>,
}

#[derive(Default)]
pub struct CoreLimiterManager {
    tracked: BTreeMap<u32, TrackedProcess>,
    limited: BTreeMap<u32, LimitedProcess>,
    failure_suppression: ExecutionFailureTracker,
}

#[derive(Clone)]
struct TrackedProcess {
    executable_path: String,
    creation_time: u64,
    previous_cpu_time: Option<ProcessCpuSample>,
    high_since: Option<Instant>,
    below_since: Option<Instant>,
}

#[derive(Clone)]
struct LimitedProcess {
    target: ProcessControlTarget,
}

struct CoreLimiterTarget {
    process_name: String,
    executable_path: String,
    rule: CoreLimiterRule,
}

impl CoreLimiterManager {
    #[expect(
        clippy::too_many_arguments,
        reason = "the pass-local observation dependency is clearer here than an unrelated argument bundle"
    )]
    pub fn update(
        &mut self,
        coordinator: &mut CpuAllocationCoordinator,
        settings: &CoreLimiterSettings,
        automation_enabled: bool,
        allow_cross_session_process_control: bool,
        foreground_process_id: Option<u32>,
        observations: &mut CycleObservations,
        action_log: &mut ActionLog,
    ) -> CoreLimiterSnapshot {
        if !automation_enabled {
            let failed = self.clear_all(coordinator, action_log, "automation disabled");
            self.failure_suppression.clear();
            return CoreLimiterSnapshot {
                enabled: false,
                failed_processes: failed.count,
                message: "Automation disabled.".to_owned(),
                last_error: failed.last_error,
                ..Default::default()
            };
        }

        if !settings.enabled {
            let failed = self.clear_all(coordinator, action_log, "Core Limiter disabled");
            self.failure_suppression.clear();
            return CoreLimiterSnapshot {
                enabled: false,
                failed_processes: failed.count,
                message: "Core Limiter disabled.".to_owned(),
                last_error: failed.last_error,
                ..Default::default()
            };
        }

        let enabled_process_names = settings
            .rules
            .iter()
            .filter(|rule| rule.enabled && Path::new(rule.executable_path.trim()).is_absolute())
            .filter_map(|rule| Path::new(rule.executable_path.trim()).file_name())
            .filter_map(|name| name.to_str())
            .map(str::to_ascii_lowercase)
            .collect::<BTreeSet<_>>();
        if enabled_process_names.is_empty() {
            let failed =
                self.clear_all(coordinator, action_log, "no Core Limiter rules configured");
            self.failure_suppression.clear();
            self.tracked.clear();
            return CoreLimiterSnapshot {
                enabled: true,
                failed_processes: failed.count,
                message: "No Core Limiter rules configured.".to_owned(),
                last_error: failed.last_error,
                ..Default::default()
            };
        }

        if settings.protect_foreground_app && foreground_process_id.is_none() {
            let failed = self.clear_all(coordinator, action_log, "foreground app is unknown");
            return CoreLimiterSnapshot {
                enabled: true,
                failed_processes: failed.count,
                message: "Paused: foreground app is unknown.".to_owned(),
                last_error: failed.last_error,
                ..Default::default()
            };
        }

        let visible_window_process_ids = if settings.protect_visible_window_apps {
            let Ok(process_ids) = observations.visible_window_process_ids() else {
                let failed =
                    self.clear_all(coordinator, action_log, "visible windows are unavailable");
                return CoreLimiterSnapshot {
                    enabled: true,
                    failed_processes: failed.count,
                    message: "Paused: visible windows are unavailable.".to_owned(),
                    last_error: failed.last_error,
                    ..Default::default()
                };
            };
            process_ids
        } else {
            Default::default()
        };

        // SAFETY: GetCurrentProcessId takes no arguments and has no caller requirements.
        let current_process_id = unsafe { GetCurrentProcessId() };
        let Some(current_session_id) = process_session_id(current_process_id) else {
            let failed = self.clear_all(
                coordinator,
                action_log,
                "current Windows session is unknown",
            );
            return CoreLimiterSnapshot {
                enabled: true,
                failed_processes: failed.count,
                message: "Paused: current Windows session is unknown.".to_owned(),
                last_error: failed.last_error,
                ..Default::default()
            };
        };

        let processes = match observations.processes() {
            Ok(processes) => processes,
            Err(err) => {
                let failed = self.clear_all(coordinator, action_log, "process list unavailable");
                return CoreLimiterSnapshot {
                    enabled: true,
                    failed_processes: failed.count,
                    message: err,
                    last_error: failed.last_error,
                    ..Default::default()
                };
            }
        };

        let scanned_processes = processes.len();
        let protected_processes = ProtectedProcesses::capture(
            processes.as_ref(),
            settings.protect_foreground_app,
            foreground_process_id,
            visible_window_process_ids,
        );

        let mut target_processes = BTreeMap::new();
        for process in processes.iter() {
            if process.id == 0
                || process.is_critical != Some(false)
                || !process.can_set_information
                || process.id == current_process_id
                || is_builtin_excluded(&process.name)
                || !enabled_process_names.contains(&process.name.to_ascii_lowercase())
                || (!allow_cross_session_process_control
                    && process_session_id(process.id) != Some(current_session_id))
            {
                continue;
            }

            let Some(executable_path) = process_executable_path(process) else {
                continue;
            };
            if protected_processes.contains(process.id, &executable_path) {
                continue;
            }

            if let Some(rule) = matching_rule(settings, &executable_path) {
                target_processes.insert(
                    process.id,
                    CoreLimiterTarget {
                        process_name: process.name.clone(),
                        executable_path: executable_path.to_string_lossy().into_owned(),
                        rule: rule.clone(),
                    },
                );
            }
        }

        let target_ids = target_processes.keys().copied().collect::<BTreeSet<_>>();
        let active_target_names = target_processes
            .values()
            .map(|target| process_failure_key(&target.executable_path))
            .collect::<BTreeSet<_>>();
        self.failure_suppression.retain_keys(&active_target_names);
        self.limited
            .retain(|process_id, _| target_ids.contains(process_id));
        let mut failures = CoreLimiterFailures::default();
        self.tracked
            .retain(|process_id, _| target_ids.contains(process_id));

        let mut skipped_processes = 0;
        let mut auto_excluded_processes = BTreeSet::new();
        let now = Instant::now();
        for (process_id, target) in target_processes {
            let failure_process_name = target.process_name.clone();
            let failure_executable_path = target.executable_path.clone();
            if self.is_process_suppressed(
                process_id,
                &failure_process_name,
                &failure_executable_path,
                action_log,
                &mut auto_excluded_processes,
            ) {
                skipped_processes += 1;
                continue;
            }

            match self.update_process(
                coordinator,
                process_id,
                &target,
                now,
                allow_cross_session_process_control,
            ) {
                Ok(CpuAllocationApplyOutcome::Applied) => {
                    self.clear_process_failure(&failure_executable_path);
                    action_log.record(
                        ActionLogFeature::CoreLimiter,
                        Some(process_id),
                        failure_process_name,
                        ActionLogResult::Applied,
                        format!(
                            "Limited the process to {} logical processors.",
                            target.rule.max_logical_processors.max(1)
                        ),
                    );
                }
                Ok(CpuAllocationApplyOutcome::Unchanged) => {
                    self.clear_process_failure(&failure_executable_path);
                }
                Ok(
                    CpuAllocationApplyOutcome::Shadowed | CpuAllocationApplyOutcome::NoUsableTarget,
                ) => {
                    skipped_processes += 1;
                    self.clear_process_failure(&failure_executable_path);
                }
                Err(ProcessControlError::ProcessExited) => {
                    skipped_processes += 1;
                    self.tracked.remove(&process_id);
                    self.limited.remove(&process_id);
                }
                Err(ProcessControlError::AccessDenied(message)) => {
                    skipped_processes += 1;
                    self.failure_suppression
                        .suppress_process_failure(&failure_executable_path);
                    action_log.record(
                        ActionLogFeature::CoreLimiter,
                        Some(process_id),
                        failure_process_name,
                        ActionLogResult::Skipped,
                        message,
                    );
                }
                Err(error) => {
                    self.record_process_failure(&failure_executable_path);
                    failures.record_control_error(
                        "Limit",
                        process_id,
                        &failure_process_name,
                        error,
                        ActionLogFeature::CoreLimiter,
                        action_log,
                    );
                }
            }
        }

        let active_claims = self
            .limited
            .values()
            .map(|process| process.target.key())
            .collect::<BTreeSet<_>>();
        self.merge_release_summary(
            coordinator.release_policy_except(ControlOwner::CoreLimiter, &active_claims),
            action_log,
            "process no longer matches an active Core Limiter limit",
            &mut failures,
        );

        CoreLimiterSnapshot {
            enabled: true,
            scanned_processes,
            limited_processes: coordinator.policy_managed_process_count(ControlOwner::CoreLimiter),
            tracked_processes: self.tracked.len(),
            skipped_processes,
            failed_processes: failures.count,
            limited_apps: coordinator.policy_managed_process_paths(ControlOwner::CoreLimiter),
            auto_excluded_processes: auto_excluded_processes.into_iter().collect(),
            message: "Core Limiter active.".to_owned(),
            last_error: failures.last_error,
        }
    }

    fn update_process(
        &mut self,
        coordinator: &mut CpuAllocationCoordinator,
        process_id: u32,
        target_process: &CoreLimiterTarget,
        now: Instant,
        allow_cross_session_process_control: bool,
    ) -> Result<CpuAllocationApplyOutcome, ProcessControlError> {
        let CoreLimiterTarget {
            process_name,
            executable_path,
            rule,
        } = target_process;
        let (current, creation_time) =
            process_cpu_sample(process_id, executable_path).map_err(ProcessControlError::from)?;
        let tracked_identity_changed = self.tracked.get(&process_id).is_some_and(|process| {
            process.creation_time != creation_time
                || !same_executable_path(
                    Path::new(&process.executable_path),
                    Path::new(&executable_path),
                )
        });
        let limited_identity_changed = self.limited.get(&process_id).is_some_and(|process| {
            process.target.creation_time != creation_time
                || !same_executable_path(
                    &process.target.executable_path,
                    Path::new(&executable_path),
                )
        });
        if tracked_identity_changed || limited_identity_changed {
            self.tracked.remove(&process_id);
            self.limited.remove(&process_id);
        }
        let state = self
            .tracked
            .entry(process_id)
            .or_insert_with(|| TrackedProcess {
                executable_path: executable_path.clone(),
                creation_time,
                previous_cpu_time: None,
                high_since: None,
                below_since: None,
            });
        state.executable_path = executable_path.clone();
        state.creation_time = creation_time;

        let usage = state
            .previous_cpu_time
            .and_then(|previous| process_cpu_usage_percent(previous, current));
        state.previous_cpu_time = Some(current);
        let Some(usage) = usage else {
            return Ok(CpuAllocationApplyOutcome::Unchanged);
        };

        let threshold = f32::from(rule.threshold_percent.min(100));
        if usage >= threshold {
            state.below_since = None;
            let high_since = *state.high_since.get_or_insert(now);
            if self.limited.contains_key(&process_id)
                || now.duration_since(high_since) >= Duration::from_secs(rule.sustain_seconds)
            {
                let target = ProcessControlTarget::automatic(
                    process_id,
                    process_name.clone(),
                    executable_path.into(),
                    creation_time,
                );
                let result = coordinator.apply_policy_claim(
                    CpuAllocationClaim {
                        target: target.clone(),
                        owner: ControlOwner::CoreLimiter,
                        request: CpuAllocationRequest::LimitLogicalProcessors {
                            maximum: rule.max_logical_processors,
                        },
                    },
                    allow_cross_session_process_control,
                )?;
                match result {
                    CpuAllocationApplyOutcome::Applied
                    | CpuAllocationApplyOutcome::Unchanged
                    | CpuAllocationApplyOutcome::Shadowed => {
                        self.limited.insert(process_id, LimitedProcess { target });
                    }
                    CpuAllocationApplyOutcome::NoUsableTarget => {
                        self.limited.remove(&process_id);
                    }
                }
                return Ok(result);
            }
            return Ok(CpuAllocationApplyOutcome::Unchanged);
        }

        state.high_since = None;
        if self.limited.contains_key(&process_id) {
            let below_since = *state.below_since.get_or_insert(now);
            if now.duration_since(below_since) >= Duration::from_secs(rule.cooldown_seconds) {
                self.limited.remove(&process_id);
                self.tracked.remove(&process_id);
            }
        }

        Ok(CpuAllocationApplyOutcome::Unchanged)
    }

    fn clear_all(
        &mut self,
        coordinator: &mut CpuAllocationCoordinator,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> CoreLimiterFailures {
        self.tracked.clear();
        self.limited.clear();
        let mut failures = CoreLimiterFailures::default();
        self.merge_release_summary(
            coordinator.release_all_policy(ControlOwner::CoreLimiter),
            action_log,
            reason,
            &mut failures,
        );
        failures
    }

    fn merge_release_summary(
        &mut self,
        summary: CpuAllocationReleaseSummary,
        action_log: &mut ActionLog,
        reason: &str,
        failures: &mut CoreLimiterFailures,
    ) {
        record_cpu_allocation_restorations(summary.restored_owners, reason, action_log);
        for failure in summary.failures {
            let (action_log_feature, _) = cpu_allocation_action_log_context(failure.owner);
            let message = failure.error.to_string();
            if failure.owner == ControlOwner::CoreLimiter
                && !matches!(failure.error, ProcessControlError::ProcessExited)
            {
                failures.note_message(
                    "Restore",
                    failure.process_id,
                    &failure.process_name,
                    &message,
                );
            }
            action_log.record(
                action_log_feature,
                Some(failure.process_id),
                failure.process_name,
                ActionLogResult::Failed,
                message,
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
                ActionLogFeature::CoreLimiter,
                Some(process_id),
                process_name.to_owned(),
                ActionLogResult::Skipped,
                format!(
                    "Stopped retrying Core Limiter after {} failed attempts.",
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

impl Default for CoreLimiterSnapshot {
    fn default() -> Self {
        Self {
            enabled: false,
            scanned_processes: 0,
            limited_processes: 0,
            tracked_processes: 0,
            skipped_processes: 0,
            failed_processes: 0,
            limited_apps: Vec::new(),
            auto_excluded_processes: Vec::new(),
            message: "Core Limiter disabled.".to_owned(),
            last_error: None,
        }
    }
}

pub fn is_builtin_excluded(process_name: &str) -> bool {
    let process_name = std::path::Path::new(process_name)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(process_name);
    contains_process_name(BUILT_IN_EXCLUSIONS, process_name)
}

fn matching_rule<'a>(
    settings: &'a CoreLimiterSettings,
    executable_path: &Path,
) -> Option<&'a CoreLimiterRule> {
    settings.rules.iter().find(|rule| {
        rule.enabled
            && !rule.executable_path.trim().is_empty()
            && same_executable_path(Path::new(rule.executable_path.trim()), executable_path)
    })
}

fn process_cpu_sample(
    process_id: u32,
    executable_path: &str,
) -> Result<(ProcessCpuSample, u64), CoreLimiterError> {
    let process = ProcessHandle::open_query(process_id)?;
    if !process_handle_matches_executable_path(&process.0, Path::new(executable_path)) {
        return Err(CoreLimiterError::ProcessExited);
    }
    let creation_time = process
        .0
        .process_creation_time()
        .ok_or(CoreLimiterError::ProcessExited)?;
    Ok((process.cpu_sample()?, creation_time))
}

enum CoreLimiterError {
    AccessDenied,
    ProcessExited,
    Failed(String),
}

impl From<CoreLimiterError> for ProcessControlError {
    fn from(error: CoreLimiterError) -> Self {
        match error {
            CoreLimiterError::AccessDenied => {
                ProcessControlError::AccessDenied("Access denied.".to_owned())
            }
            CoreLimiterError::ProcessExited => ProcessControlError::ProcessExited,
            CoreLimiterError::Failed(message) => ProcessControlError::Failed(message),
        }
    }
}

#[derive(Default)]
struct CoreLimiterFailures {
    count: usize,
    last_error: Option<String>,
}

impl CoreLimiterFailures {
    fn record_control_error(
        &mut self,
        action: &str,
        process_id: u32,
        process_name: &str,
        error: ProcessControlError,
        action_log_feature: ActionLogFeature,
        action_log: &mut ActionLog,
    ) {
        if matches!(error, ProcessControlError::ProcessExited) {
            return;
        }
        self.record_message(
            action,
            process_id,
            process_name,
            error.to_string(),
            action_log_feature,
            action_log,
        );
    }

    fn record_message(
        &mut self,
        action: &str,
        process_id: u32,
        process_name: &str,
        message: String,
        action_log_feature: ActionLogFeature,
        action_log: &mut ActionLog,
    ) {
        self.note_message(action, process_id, process_name, &message);
        action_log.record(
            action_log_feature,
            Some(process_id),
            process_name.to_owned(),
            ActionLogResult::Failed,
            message,
        );
    }

    fn note_message(&mut self, action: &str, process_id: u32, process_name: &str, message: &str) {
        self.count += 1;
        if self.last_error.is_none() {
            self.last_error = Some(process_failure_message(
                action,
                process_id,
                process_name,
                message,
            ));
        }
    }
}

fn process_failure_message(
    action: &str,
    process_id: u32,
    process_name: &str,
    message: &str,
) -> String {
    format!("{action} {process_name} ({process_id}): {message}")
}

struct ProcessHandle(WinHandle);

impl ProcessHandle {
    fn open_query(process_id: u32) -> Result<Self, CoreLimiterError> {
        // SAFETY: process_id came from the current process snapshot and no inherited handle is
        // requested.
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
        if !handle.is_null() {
            Ok(Self(WinHandle::new(handle)))
        } else {
            Err(open_process_error(process_id, last_error()))
        }
    }

    fn cpu_sample(&self) -> Result<ProcessCpuSample, CoreLimiterError> {
        let mut creation = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        // SAFETY: self owns a live process handle and every FILETIME output is writable for the
        // call.
        let ok = unsafe {
            GetProcessTimes(
                self.0.raw(),
                &mut creation,
                &mut exit,
                &mut kernel,
                &mut user,
            )
        };
        if ok == 0 {
            Err(CoreLimiterError::Failed(format!(
                "GetProcessTimes failed with error {}.",
                last_error()
            )))
        } else {
            Ok(ProcessCpuSample {
                cpu_time_100ns: filetime_to_u64(kernel).saturating_add(filetime_to_u64(user)),
                sampled_at: Instant::now(),
            })
        }
    }
}

fn open_process_error(process_id: u32, error: u32) -> CoreLimiterError {
    match error {
        ERROR_ACCESS_DENIED => CoreLimiterError::AccessDenied,
        ERROR_INVALID_PARAMETER => CoreLimiterError::ProcessExited,
        _ => CoreLimiterError::Failed(format!(
            "OpenProcess({process_id}) failed with error {error}."
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching_rule_requires_the_exact_executable_path() {
        let settings = CoreLimiterSettings {
            enabled: true,
            protect_foreground_app: true,
            protect_visible_window_apps: false,
            rules: vec![CoreLimiterRule {
                enabled: true,
                executable_path: r"C:\Apps\Worker.EXE".to_owned(),
                threshold_percent: 75,
                sustain_seconds: 5,
                cooldown_seconds: 10,
                max_logical_processors: 1,
            }],
        };

        assert!(matching_rule(&settings, Path::new(r"C:\Apps\worker.exe")).is_some());
        assert!(matching_rule(&settings, Path::new(r"D:\Tools\worker.exe")).is_none());
    }

    #[test]
    fn builtin_exclusions_cover_sensitive_windows_processes() {
        assert!(is_builtin_excluded("csrss.exe"));
        assert!(is_builtin_excluded("winlogon.exe"));
        assert!(!is_builtin_excluded("worker.exe"));
    }

    #[test]
    fn repeated_failures_suppress_future_core_limiter_attempts_once() {
        let mut manager = CoreLimiterManager::default();
        let mut log = ActionLog::new(8);
        let executable_path = r"C:\Apps\app.exe";

        manager.record_process_failure(executable_path);
        manager.record_process_failure(executable_path);
        assert!(!manager.is_process_suppressed(
            42,
            "app.exe",
            executable_path,
            &mut log,
            &mut BTreeSet::new()
        ));
        assert!(log.entries().is_empty());

        manager.record_process_failure(executable_path);
        assert!(manager.is_process_suppressed(
            42,
            "app.exe",
            executable_path,
            &mut log,
            &mut BTreeSet::new()
        ));
        assert!(manager.is_process_suppressed(
            43,
            "app.exe",
            r"C:/Apps/app.exe",
            &mut log,
            &mut BTreeSet::new()
        ));

        let entries = log.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].process_name, "app.exe");
        assert_eq!(entries[0].result, ActionLogResult::Skipped);
    }

    #[test]
    fn process_cpu_usage_percent_scales_by_processor_count() {
        let now = Instant::now();
        let previous = ProcessCpuSample {
            cpu_time_100ns: 0,
            sampled_at: now,
        };
        let current = ProcessCpuSample {
            cpu_time_100ns: 10_000_000,
            sampled_at: now + Duration::from_secs(1),
        };

        let usage = process_cpu_usage_percent(previous, current).unwrap();

        assert!(usage > 0.0);
        assert!(usage <= 100.0);
    }
}
