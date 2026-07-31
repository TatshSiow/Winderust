use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    time::{Duration, Instant},
};

use windows_sys::Win32::{
    Foundation::{ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER, FILETIME},
    System::Threading::{
        GetCurrentProcessId, GetProcessAffinityMask, GetProcessTimes, OpenProcess,
        SetProcessAffinityMask, PROCESS_QUERY_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION,
        PROCESS_SET_INFORMATION,
    },
};

use crate::{
    action_log::{ActionLog, ActionLogFeature, ActionLogResult},
    config::{CoreLimiterRule, CoreLimiterSettings},
    cpu::{process_cpu_usage_percent, ProcessCpuSample},
    foreground::{
        contains_process_name, list_processes, process_executable_path, process_failure_key,
        process_handle_matches_executable_path, process_session_id, same_executable_path,
        same_process_name, should_ignore_foreground_process, EXTENDED_BUILT_IN_PROCESS_EXCLUSIONS,
    },
    rules::{execution_failure_suppression_threshold, ExecutionFailureTracker},
    win_util::{filetime_to_u64, last_error, WinHandle},
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
    process_name: String,
    executable_path: String,
    creation_time: u64,
    previous_affinity: usize,
    applied_affinity: usize,
}

impl CoreLimiterManager {
    pub fn update(
        &mut self,
        settings: &CoreLimiterSettings,
        automation_enabled: bool,
        allow_cross_session_process_control: bool,
        foreground_process_id: Option<u32>,
        core_steering_process_ids: &BTreeSet<u32>,
        action_log: &mut ActionLog,
    ) -> CoreLimiterSnapshot {
        if !automation_enabled {
            let failed = self.clear_all(action_log, "automation disabled");
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
            let failed = self.clear_all(action_log, "Core Limiter disabled");
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
            let failed = self.clear_all(action_log, "no Core Limiter rules configured");
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

        if settings.exclude_foreground_app && foreground_process_id.is_none() {
            let failed = self.clear_all(action_log, "foreground app is unknown");
            return CoreLimiterSnapshot {
                enabled: true,
                failed_processes: failed.count,
                message: "Paused: foreground app is unknown.".to_owned(),
                last_error: failed.last_error,
                ..Default::default()
            };
        }

        // SAFETY: GetCurrentProcessId takes no arguments and has no caller requirements.
        let current_process_id = unsafe { GetCurrentProcessId() };
        let Some(current_session_id) = process_session_id(current_process_id) else {
            let failed = self.clear_all(action_log, "current Windows session is unknown");
            return CoreLimiterSnapshot {
                enabled: true,
                failed_processes: failed.count,
                message: "Paused: current Windows session is unknown.".to_owned(),
                last_error: failed.last_error,
                ..Default::default()
            };
        };

        let processes = match list_processes() {
            Ok(processes) => processes,
            Err(err) => {
                let failed = self.clear_all(action_log, "process list unavailable");
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
        let foreground_executable_path = if settings.exclude_foreground_app {
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
        for process in processes {
            if process.id == 0
                || process.is_critical != Some(false)
                || process.id == current_process_id
                || is_builtin_excluded(&process.name)
                || !enabled_process_names.contains(&process.name.to_ascii_lowercase())
                || (!allow_cross_session_process_control
                    && process_session_id(process.id) != Some(current_session_id))
            {
                continue;
            }

            let Some(executable_path) = process_executable_path(&process) else {
                continue;
            };
            if should_ignore_foreground_process(
                settings.exclude_foreground_app,
                process.id,
                &executable_path,
                foreground_process_id,
                foreground_executable_path.as_deref(),
            ) {
                continue;
            }

            if core_steering_process_ids.contains(&process.id) {
                if self.limited.contains_key(&process.id) {
                    action_log.record(
                        ActionLogFeature::CoreLimiter,
                        Some(process.id),
                        process.name.clone(),
                        ActionLogResult::Skipped,
                        "Skipped because Core Steering is already managing this process.",
                    );
                }
                continue;
            }

            if let Some(rule) = matching_rule(settings, &executable_path) {
                target_processes.insert(
                    process.id,
                    (
                        process.name,
                        executable_path.to_string_lossy().into_owned(),
                        rule.clone(),
                    ),
                );
            }
        }

        let target_ids = target_processes.keys().copied().collect::<BTreeSet<_>>();
        let active_target_names = target_processes
            .values()
            .map(|(_name, path, _rule)| process_failure_key(path))
            .collect::<BTreeSet<_>>();
        self.failure_suppression.retain_keys(&active_target_names);
        let mut failures = self.release_non_targets(
            &target_ids,
            action_log,
            "process no longer matches a Core Limiter rule",
        );
        self.tracked
            .retain(|process_id, _| target_ids.contains(process_id));

        let mut skipped_processes = 0;
        let mut auto_excluded_processes = BTreeSet::new();
        let now = Instant::now();
        for (process_id, (process_name, executable_path, rule)) in target_processes {
            let failure_process_name = process_name.clone();
            let failure_executable_path = executable_path.clone();
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
                process_id,
                process_name,
                executable_path,
                &rule,
                now,
                action_log,
            ) {
                Ok(()) => {
                    self.clear_process_failure(&failure_executable_path);
                }
                Err(CoreLimiterError::ProcessExited) => {
                    skipped_processes += 1;
                    self.tracked.remove(&process_id);
                    self.limited.remove(&process_id);
                }
                Err(CoreLimiterError::AccessDenied) => {
                    skipped_processes += 1;
                    self.failure_suppression
                        .suppress_process_failure(&failure_executable_path);
                    action_log.record(
                        ActionLogFeature::CoreLimiter,
                        Some(process_id),
                        failure_process_name,
                        ActionLogResult::Skipped,
                        "Skipped because the process could not be opened.",
                    );
                }
                Err(CoreLimiterError::Failed(err)) => {
                    self.record_process_failure(&failure_executable_path);
                    failures.record_message(
                        "Limit",
                        process_id,
                        &failure_process_name,
                        err,
                        action_log,
                    );
                }
            }
        }

        CoreLimiterSnapshot {
            enabled: true,
            scanned_processes,
            limited_processes: self.limited.len(),
            tracked_processes: self.tracked.len(),
            skipped_processes,
            failed_processes: failures.count,
            limited_apps: self
                .limited
                .values()
                .map(|process| process.executable_path.clone())
                .collect::<BTreeSet<_>>()
                .into_iter()
                .collect(),
            auto_excluded_processes: auto_excluded_processes.into_iter().collect(),
            message: "Core Limiter active.".to_owned(),
            last_error: failures.last_error,
        }
    }

    fn update_process(
        &mut self,
        process_id: u32,
        process_name: String,
        executable_path: String,
        rule: &CoreLimiterRule,
        now: Instant,
        action_log: &mut ActionLog,
    ) -> Result<(), CoreLimiterError> {
        let (current, creation_time) = process_cpu_sample(process_id, &executable_path)?;
        let tracked_identity_changed = self.tracked.get(&process_id).is_some_and(|process| {
            process.creation_time != creation_time
                || !same_executable_path(
                    Path::new(&process.executable_path),
                    Path::new(&executable_path),
                )
        });
        let limited_identity_changed = self.limited.get(&process_id).is_some_and(|process| {
            process.creation_time != creation_time
                || !same_executable_path(
                    Path::new(&process.executable_path),
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
            return Ok(());
        };

        let threshold = f32::from(rule.threshold_percent.min(100));
        if usage >= threshold {
            state.below_since = None;
            let high_since = *state.high_since.get_or_insert(now);
            if self.limited.contains_key(&process_id)
                || now.duration_since(high_since) >= Duration::from_secs(rule.sustain_seconds)
            {
                apply_cpu_limit_to_process(
                    process_id,
                    process_name,
                    executable_path,
                    creation_time,
                    rule.max_logical_processors,
                    &mut self.limited,
                    action_log,
                )?;
            }
            return Ok(());
        }

        state.high_since = None;
        if self.limited.contains_key(&process_id) {
            let below_since = *state.below_since.get_or_insert(now);
            if now.duration_since(below_since) >= Duration::from_secs(rule.cooldown_seconds) {
                self.release_processes(&[process_id], action_log, "CPU usage cooled down")
                    .into_result()?;
                self.tracked.remove(&process_id);
            }
        }

        Ok(())
    }

    fn release_non_targets(
        &mut self,
        target_ids: &BTreeSet<u32>,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> CoreLimiterFailures {
        let process_ids = self
            .limited
            .keys()
            .copied()
            .filter(|process_id| !target_ids.contains(process_id))
            .collect::<Vec<_>>();

        self.release_processes(&process_ids, action_log, reason)
    }

    fn clear_all(&mut self, action_log: &mut ActionLog, reason: &str) -> CoreLimiterFailures {
        self.tracked.clear();
        let process_ids = self.limited.keys().copied().collect::<Vec<_>>();
        self.release_processes(&process_ids, action_log, reason)
    }

    fn release_processes(
        &mut self,
        process_ids: &[u32],
        action_log: &mut ActionLog,
        reason: &str,
    ) -> CoreLimiterFailures {
        let mut failures = CoreLimiterFailures::default();
        for process_id in process_ids {
            if let Some(process) = self.limited.get(process_id).cloned() {
                let process_name = process.process_name.clone();
                if let Err(err) = restore_affinity(*process_id, &process) {
                    if matches!(err, CoreLimiterError::ProcessExited) {
                        self.limited.remove(process_id);
                    } else {
                        failures.record_error(
                            "Restore",
                            *process_id,
                            &process_name,
                            err,
                            action_log,
                        );
                    }
                } else {
                    self.limited.remove(process_id);
                    action_log.record(
                        ActionLogFeature::CoreLimiter,
                        Some(*process_id),
                        process_name,
                        ActionLogResult::Restored,
                        reason.to_owned(),
                    );
                }
            }
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

impl Drop for CoreLimiterManager {
    fn drop(&mut self) {
        let mut action_log = ActionLog::new(1);
        self.clear_all(&mut action_log, "Core Limiter manager dropped");
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

fn limited_affinity_mask(
    current_affinity: usize,
    system_affinity: usize,
    max_logical_processors: u8,
) -> Option<usize> {
    let max_processors = usize::from(max_logical_processors.max(1));
    let available = if current_affinity != 0 {
        current_affinity
    } else {
        system_affinity
    };
    let mut target = 0_usize;
    let mut selected = 0;

    for bit in 0..usize::BITS as usize {
        let processor = 1_usize << bit;
        if (available & processor) != 0 {
            target |= processor;
            selected += 1;
            if selected >= max_processors {
                break;
            }
        }
    }

    (target != 0 && target != current_affinity).then_some(target)
}

fn apply_cpu_limit_to_process(
    process_id: u32,
    process_name: String,
    executable_path: String,
    expected_creation_time: u64,
    max_logical_processors: u8,
    limited: &mut BTreeMap<u32, LimitedProcess>,
    action_log: &mut ActionLog,
) -> Result<(), CoreLimiterError> {
    let process = ProcessHandle::open(process_id)?;
    if !process_handle_matches_executable_path(&process.0, Path::new(&executable_path)) {
        return Err(CoreLimiterError::ProcessExited);
    }
    let creation_time = process
        .0
        .process_creation_time()
        .ok_or(CoreLimiterError::ProcessExited)?;
    if creation_time != expected_creation_time {
        return Err(CoreLimiterError::ProcessExited);
    }
    let (current_affinity, system_affinity) = process.affinity_mask()?;
    let existing = limited
        .get(&process_id)
        .filter(|limited| limited.creation_time == creation_time)
        .filter(|limited| same_process_name(&limited.process_name, &process_name))
        .cloned();
    let original_affinity = existing
        .as_ref()
        .map_or(current_affinity, |limited| limited.previous_affinity);

    let Some(target_affinity) =
        limited_affinity_mask(original_affinity, system_affinity, max_logical_processors)
    else {
        if let Some(existing) = existing {
            if current_affinity != existing.previous_affinity {
                process.set_affinity_mask(existing.previous_affinity)?;
                action_log.record(
                    ActionLogFeature::CoreLimiter,
                    Some(process_id),
                    process_name,
                    ActionLogResult::Restored,
                    "Rule no longer limits this process.",
                );
            }
            limited.remove(&process_id);
        }
        return Ok(());
    };

    if existing.as_ref().is_some_and(|limited| {
        limited.applied_affinity == target_affinity && current_affinity == target_affinity
    }) {
        return Ok(());
    }

    if current_affinity != target_affinity {
        process.set_affinity_mask(target_affinity)?;
        action_log.record(
            ActionLogFeature::CoreLimiter,
            Some(process_id),
            process_name.clone(),
            ActionLogResult::Applied,
            format!("Constrained affinity from {original_affinity:#x} to {target_affinity:#x}."),
        );
    }

    limited.insert(
        process_id,
        LimitedProcess {
            process_name,
            executable_path,
            creation_time,
            previous_affinity: original_affinity,
            applied_affinity: target_affinity,
        },
    );
    Ok(())
}
fn restore_affinity(
    process_id: u32,
    process_state: &LimitedProcess,
) -> Result<(), CoreLimiterError> {
    let process = ProcessHandle::open(process_id)?;
    if process.0.process_creation_time() != Some(process_state.creation_time)
        || !process_handle_matches_executable_path(
            &process.0,
            Path::new(&process_state.executable_path),
        )
    {
        return Err(CoreLimiterError::ProcessExited);
    }
    process.set_affinity_mask(process_state.previous_affinity)
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

#[derive(Default)]
struct CoreLimiterFailures {
    count: usize,
    last_error: Option<String>,
}

impl CoreLimiterFailures {
    fn record_error(
        &mut self,
        action: &str,
        process_id: u32,
        process_name: &str,
        error: CoreLimiterError,
        action_log: &mut ActionLog,
    ) {
        let message = match error {
            CoreLimiterError::AccessDenied => "Access denied.".to_owned(),
            CoreLimiterError::ProcessExited => return,
            CoreLimiterError::Failed(message) => message,
        };
        self.record_message(action, process_id, process_name, message, action_log);
    }

    fn record_message(
        &mut self,
        action: &str,
        process_id: u32,
        process_name: &str,
        message: String,
        action_log: &mut ActionLog,
    ) {
        self.count += 1;
        if self.last_error.is_none() {
            self.last_error = Some(process_failure_message(
                action,
                process_id,
                process_name,
                &message,
            ));
        }
        action_log.record(
            ActionLogFeature::CoreLimiter,
            Some(process_id),
            process_name.to_owned(),
            ActionLogResult::Failed,
            message,
        );
    }

    fn into_result(self) -> Result<(), CoreLimiterError> {
        match self.last_error {
            Some(error) => Err(CoreLimiterError::Failed(error)),
            None => Ok(()),
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
    fn open(process_id: u32) -> Result<Self, CoreLimiterError> {
        let access_masks = [
            PROCESS_QUERY_INFORMATION | PROCESS_SET_INFORMATION,
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_INFORMATION,
        ];

        let mut last_open_error = 0;
        for access in access_masks {
            // SAFETY: process_id came from the current process snapshot, access is one of the two
            // documented masks above, and no inherited handle is requested.
            let handle = unsafe { OpenProcess(access, 0, process_id) };
            if !handle.is_null() {
                return Ok(Self(WinHandle::new(handle)));
            }
            last_open_error = last_error();
        }

        Err(open_process_error(process_id, last_open_error))
    }

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

    fn affinity_mask(&self) -> Result<(usize, usize), CoreLimiterError> {
        let mut process_affinity = 0;
        let mut system_affinity = 0;
        // SAFETY: self owns a live process handle and both affinity outputs are writable.
        let ok = unsafe {
            GetProcessAffinityMask(self.0.raw(), &mut process_affinity, &mut system_affinity)
        };
        if ok == 0 {
            Err(CoreLimiterError::Failed(format!(
                "GetProcessAffinityMask failed with error {}.",
                last_error()
            )))
        } else {
            Ok((process_affinity, system_affinity))
        }
    }

    fn set_affinity_mask(&self, affinity_mask: usize) -> Result<(), CoreLimiterError> {
        // SAFETY: self owns a live process handle and affinity_mask was normalized against the
        // system mask read from this process.
        let ok = unsafe { SetProcessAffinityMask(self.0.raw(), affinity_mask) };
        if ok == 0 {
            Err(CoreLimiterError::Failed(format!(
                "SetProcessAffinityMask failed with error {}.",
                last_error()
            )))
        } else {
            Ok(())
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
            exclude_foreground_app: true,
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
    fn foreground_skip_matches_pid_or_executable_path() {
        let settings = CoreLimiterSettings {
            enabled: true,
            exclude_foreground_app: true,
            rules: Vec::new(),
        };

        assert!(should_ignore_foreground_process(
            settings.exclude_foreground_app,
            42,
            Path::new(r"C:\Apps\helper.exe"),
            Some(42),
            Some(Path::new(r"C:\Apps\app.exe")),
        ));
        assert!(should_ignore_foreground_process(
            settings.exclude_foreground_app,
            99,
            Path::new(r"c:/apps/APP.exe"),
            Some(42),
            Some(Path::new(r"C:\Apps\app.exe")),
        ));
        assert!(!should_ignore_foreground_process(
            settings.exclude_foreground_app,
            99,
            Path::new(r"C:\Other\app.exe"),
            Some(42),
            Some(Path::new(r"C:\Apps\app.exe")),
        ));
    }

    #[test]
    fn affinity_apply_rejects_a_different_process_instance() {
        // SAFETY: GetCurrentProcessId takes no arguments and has no caller requirements.
        let process_id = unsafe { GetCurrentProcessId() };
        let executable_path = std::env::current_exe()
            .expect("the test process executable path should be available")
            .to_string_lossy()
            .into_owned();
        let process_name = Path::new(&executable_path)
            .file_name()
            .expect("the test executable should have a file name")
            .to_string_lossy()
            .into_owned();
        let mut limited = BTreeMap::new();
        let mut log = ActionLog::new(4);

        let result = apply_cpu_limit_to_process(
            process_id,
            process_name,
            executable_path,
            u64::MAX,
            1,
            &mut limited,
            &mut log,
        );

        assert!(matches!(result, Err(CoreLimiterError::ProcessExited)));
        assert!(limited.is_empty());
        assert!(log.entries().is_empty());
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
    fn limited_affinity_selects_lowest_available_processors() {
        assert_eq!(limited_affinity_mask(0b1111, 0b1111, 2), Some(0b0011));
        assert_eq!(limited_affinity_mask(0b1010, 0b1111, 1), Some(0b0010));
        assert_eq!(limited_affinity_mask(0b0011, 0b1111, 2), None);
        assert_eq!(limited_affinity_mask(0b1111, 0b1111, 0), Some(0b0001));
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

    #[test]
    fn release_processes_skips_restore_when_process_identity_is_unknown() {
        let mut manager = CoreLimiterManager::default();
        manager.limited.insert(
            0,
            LimitedProcess {
                process_name: "exited.exe".to_owned(),
                executable_path: r"C:\Apps\exited.exe".to_owned(),
                creation_time: 0,
                previous_affinity: 0b1111,
                applied_affinity: 0b0001,
            },
        );
        let mut log = ActionLog::new(8);

        let failures = manager.release_processes(&[0], &mut log, "test");

        assert_eq!(failures.count, 0);
        assert!(log.entries().is_empty());
        assert!(manager.limited.is_empty());
    }
}
