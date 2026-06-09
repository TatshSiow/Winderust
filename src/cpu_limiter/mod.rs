use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};

use windows_sys::Win32::{
    Foundation::{
        CloseHandle, GetLastError, ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER, FILETIME, HANDLE,
    },
    System::{
        RemoteDesktop::ProcessIdToSessionId,
        Threading::{
            GetCurrentProcessId, GetProcessAffinityMask, GetProcessTimes, OpenProcess,
            SetProcessAffinityMask, PROCESS_QUERY_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION,
            PROCESS_SET_INFORMATION,
        },
    },
};

use crate::{
    action_log::{ActionLog, ActionLogAction, ActionLogFeature, ActionLogResult},
    config::{CpuLimiterRule, CpuLimiterSettings},
    foreground::list_processes,
    rules::{
        Action, ActionExecution, ActionExecutor, AppMatcher, AppResourceActionBackend,
        ExecutionFailureState, DEFAULT_EXECUTION_FAILURE_SUPPRESSION_THRESHOLD,
    },
};

const BUILT_IN_EXCLUSIONS: &[&str] = &[
    "audiodg.exe",
    "conhost.exe",
    "csrss.exe",
    "ctfmon.exe",
    "dwm.exe",
    "explorer.exe",
    "fontdrvhost.exe",
    "lsaiso.exe",
    "lsass.exe",
    "registry",
    "searchapp.exe",
    "searchhost.exe",
    "securityhealthservice.exe",
    "securityhealthsystray.exe",
    "services.exe",
    "shellexperiencehost.exe",
    "sihost.exe",
    "smss.exe",
    "startmenuexperiencehost.exe",
    "system",
    "systemsettings.exe",
    "taskmgr.exe",
    "textinputhost.exe",
    "wininit.exe",
    "winlogon.exe",
    "wudfhost.exe",
];
const FAILURE_SUPPRESSION_THRESHOLD: u8 = DEFAULT_EXECUTION_FAILURE_SUPPRESSION_THRESHOLD;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CpuLimiterSnapshot {
    pub enabled: bool,
    pub scanned_processes: usize,
    pub limited_processes: usize,
    pub tracked_processes: usize,
    pub skipped_processes: usize,
    pub failed_processes: usize,
    pub limited_apps: Vec<String>,
    pub message: String,
    pub last_error: Option<String>,
}

#[derive(Default)]
pub struct CpuLimiterManager {
    tracked: BTreeMap<u32, TrackedProcess>,
    limited: BTreeMap<u32, LimitedProcess>,
    failure_suppression: BTreeMap<String, CpuLimiterFailureSuppression>,
}

#[derive(Clone)]
struct TrackedProcess {
    process_name: String,
    previous_cpu_time: Option<ProcessCpuSample>,
    high_since: Option<Instant>,
    below_since: Option<Instant>,
}

#[derive(Clone)]
struct LimitedProcess {
    process_name: String,
    previous_affinity: usize,
    applied_affinity: usize,
}

type CpuLimiterFailureSuppression = ExecutionFailureState;

#[derive(Clone, Copy)]
struct ProcessCpuSample {
    cpu_time_100ns: u64,
    sampled_at: Instant,
}

impl CpuLimiterManager {
    pub fn update(
        &mut self,
        settings: &CpuLimiterSettings,
        automation_enabled: bool,
        foreground_process_id: Option<u32>,
        core_steering_process_ids: &BTreeSet<u32>,
        action_log: &mut ActionLog,
    ) -> CpuLimiterSnapshot {
        if !automation_enabled {
            let failed = self.clear_all(action_log, "automation disabled");
            self.failure_suppression.clear();
            return CpuLimiterSnapshot {
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
            return CpuLimiterSnapshot {
                enabled: false,
                failed_processes: failed.count,
                message: "Core Limiter disabled.".to_owned(),
                last_error: failed.last_error,
                ..Default::default()
            };
        }

        if settings.exclude_foreground_app && foreground_process_id.is_none() {
            let failed = self.clear_all(action_log, "foreground app is unknown");
            return CpuLimiterSnapshot {
                enabled: true,
                failed_processes: failed.count,
                message: "Paused: foreground app is unknown.".to_owned(),
                last_error: failed.last_error,
                ..Default::default()
            };
        }

        let current_process_id = unsafe { GetCurrentProcessId() };
        let Some(current_session_id) = process_session_id(current_process_id) else {
            let failed = self.clear_all(action_log, "current Windows session is unknown");
            return CpuLimiterSnapshot {
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
                return CpuLimiterSnapshot {
                    enabled: true,
                    failed_processes: failed.count,
                    message: err,
                    last_error: failed.last_error,
                    ..Default::default()
                };
            }
        };

        let scanned_processes = processes.len();
        let current_process_names = processes
            .iter()
            .map(|process| (process.id, process.name.clone()))
            .collect::<BTreeMap<_, _>>();
        let foreground_process_name = if settings.exclude_foreground_app {
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
                || is_builtin_excluded(&process.name)
                || should_ignore_foreground_process(
                    settings,
                    process.id,
                    &process.name,
                    foreground_process_id,
                    foreground_process_name.as_deref(),
                )
                || process_session_id(process.id) != Some(current_session_id)
            {
                continue;
            }

            if core_steering_process_ids.contains(&process.id) {
                if self.limited.contains_key(&process.id) {
                    action_log.record(
                        ActionLogFeature::CpuLimiter,
                        Some(process.id),
                        process.name.clone(),
                        ActionLogAction::Skip,
                        ActionLogResult::Skipped,
                        "Skipped because Core Steering is already managing this process.",
                    );
                }
                continue;
            }

            if let Some(rule) = matching_rule(settings, &process.name) {
                target_processes.insert(process.id, (process.name, rule.clone()));
            }
        }

        let target_ids = target_processes.keys().copied().collect::<BTreeSet<_>>();
        let active_target_names = target_processes
            .values()
            .map(|(name, _rule)| process_failure_key(name))
            .collect::<BTreeSet<_>>();
        self.failure_suppression
            .retain(|name, _| active_target_names.contains(name));
        let mut failures = self.release_non_targets(
            &target_ids,
            &current_process_names,
            action_log,
            "process no longer matches a Core Limiter rule",
        );
        self.tracked
            .retain(|process_id, _| target_ids.contains(process_id));

        let mut skipped_processes = 0;
        let now = Instant::now();
        for (process_id, (process_name, rule)) in target_processes {
            let failure_process_name = process_name.clone();
            if self.is_process_suppressed(process_id, &failure_process_name, action_log) {
                skipped_processes += 1;
                continue;
            }

            match self.update_process(process_id, process_name, &rule, now, action_log) {
                Ok(()) => {
                    self.clear_process_failure(&failure_process_name);
                }
                Err(err @ (CpuLimiterError::AccessDenied | CpuLimiterError::ProcessExited)) => {
                    skipped_processes += 1;
                    if !matches!(err, CpuLimiterError::ProcessExited) {
                        self.record_process_failure(&failure_process_name);
                    }
                    action_log.record(
                        ActionLogFeature::CpuLimiter,
                        Some(process_id),
                        failure_process_name,
                        ActionLogAction::Skip,
                        ActionLogResult::Skipped,
                        "Skipped because the process could not be opened.",
                    );
                }
                Err(CpuLimiterError::Failed(err)) => {
                    self.record_process_failure(&failure_process_name);
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

        CpuLimiterSnapshot {
            enabled: true,
            scanned_processes,
            limited_processes: self.limited.len(),
            tracked_processes: self.tracked.len(),
            skipped_processes,
            failed_processes: failures.count,
            limited_apps: unique_app_names(
                self.limited
                    .values()
                    .map(|process| process.process_name.as_str()),
            ),
            message: "Core Limiter active.".to_owned(),
            last_error: failures.last_error,
        }
    }

    fn update_process(
        &mut self,
        process_id: u32,
        process_name: String,
        rule: &CpuLimiterRule,
        now: Instant,
        action_log: &mut ActionLog,
    ) -> Result<(), CpuLimiterError> {
        let current = process_cpu_sample(process_id)?;
        let state = self
            .tracked
            .entry(process_id)
            .or_insert_with(|| TrackedProcess {
                process_name: process_name.clone(),
                previous_cpu_time: None,
                high_since: None,
                below_since: None,
            });
        state.process_name = process_name.clone();

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
                self.apply_limit(
                    process_id,
                    process_name,
                    rule.max_logical_processors,
                    action_log,
                )?;
            }
            return Ok(());
        }

        state.high_since = None;
        if self.limited.contains_key(&process_id) {
            let below_since = *state.below_since.get_or_insert(now);
            if now.duration_since(below_since) >= Duration::from_secs(rule.cooldown_seconds) {
                self.release_processes(&[process_id], None, action_log, "CPU usage cooled down")
                    .into_result()?;
                self.tracked.remove(&process_id);
            }
        }

        Ok(())
    }

    fn apply_limit(
        &mut self,
        process_id: u32,
        process_name: String,
        max_logical_processors: u8,
        action_log: &mut ActionLog,
    ) -> Result<(), CpuLimiterError> {
        let action = Action::SetAppCpuLimit {
            app: AppMatcher::ProcessName(process_name.clone()),
            logical_processor_percent: max_logical_processors,
        };
        let mut backend = CpuLimiterActionBackend {
            process_id,
            process_name,
            limited: &mut self.limited,
            action_log,
            last_error: None,
        };

        match ActionExecutor.apply_app_resource_action(&action, &mut backend) {
            ActionExecution::Applied | ActionExecution::AlreadyApplied => Ok(()),
            ActionExecution::Failed(err) => {
                Err(backend.last_error.unwrap_or(CpuLimiterError::Failed(err)))
            }
            ActionExecution::Unsupported => Err(CpuLimiterError::Failed(
                "Core Limiter action was not supported by the generic executor.".to_owned(),
            )),
        }
    }

    fn release_non_targets(
        &mut self,
        target_ids: &BTreeSet<u32>,
        current_process_names: &BTreeMap<u32, String>,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> CpuLimiterFailures {
        let process_ids = self
            .limited
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

    fn clear_all(&mut self, action_log: &mut ActionLog, reason: &str) -> CpuLimiterFailures {
        self.tracked.clear();
        let process_ids = self.limited.keys().copied().collect::<Vec<_>>();
        self.release_processes(&process_ids, None, action_log, reason)
    }

    fn release_processes(
        &mut self,
        process_ids: &[u32],
        current_process_names: Option<&BTreeMap<u32, String>>,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> CpuLimiterFailures {
        let mut failures = CpuLimiterFailures::default();
        for process_id in process_ids {
            if let Some(process) = self.limited.remove(process_id) {
                let process_name = process.process_name.clone();
                let still_same_process = current_process_names.map_or(true, |names| {
                    names
                        .get(process_id)
                        .is_some_and(|name| name.eq_ignore_ascii_case(&process.process_name))
                });
                if still_same_process {
                    if let Err(err) = restore_affinity(*process_id, process) {
                        if !matches!(err, CpuLimiterError::ProcessExited) {
                            failures.record_error(
                                "Restore",
                                *process_id,
                                &process_name,
                                err,
                                action_log,
                            );
                        }
                    } else {
                        action_log.record(
                            ActionLogFeature::CpuLimiter,
                            Some(*process_id),
                            process_name,
                            ActionLogAction::Restore,
                            ActionLogResult::Restored,
                            reason.to_owned(),
                        );
                    }
                }
            }
        }
        failures
    }

    fn is_process_suppressed(
        &mut self,
        process_id: u32,
        process_name: &str,
        action_log: &mut ActionLog,
    ) -> bool {
        let Some(suppression) = self
            .failure_suppression
            .get_mut(&process_failure_key(process_name))
        else {
            return false;
        };
        if !suppression.is_suppressed_at(FAILURE_SUPPRESSION_THRESHOLD) {
            return false;
        }

        if suppression.mark_suppression_logged() {
            action_log.record(
                ActionLogFeature::CpuLimiter,
                Some(process_id),
                process_name.to_owned(),
                ActionLogAction::Skip,
                ActionLogResult::Skipped,
                format!(
                    "Stopped retrying Core Limiter after {FAILURE_SUPPRESSION_THRESHOLD} failed attempts."
                ),
            );
        }

        true
    }

    fn record_process_failure(&mut self, process_name: &str) {
        let suppression = self
            .failure_suppression
            .entry(process_failure_key(process_name))
            .or_default();
        suppression.record_failure();
    }

    fn clear_process_failure(&mut self, process_name: &str) {
        self.failure_suppression
            .remove(&process_failure_key(process_name));
    }
}

impl Drop for CpuLimiterManager {
    fn drop(&mut self) {
        let mut action_log = ActionLog::new(1);
        self.clear_all(&mut action_log, "Core Limiter manager dropped");
    }
}

impl Default for CpuLimiterSnapshot {
    fn default() -> Self {
        Self {
            enabled: false,
            scanned_processes: 0,
            limited_processes: 0,
            tracked_processes: 0,
            skipped_processes: 0,
            failed_processes: 0,
            limited_apps: Vec::new(),
            message: "Core Limiter disabled.".to_owned(),
            last_error: None,
        }
    }
}

pub fn is_builtin_excluded(process_name: &str) -> bool {
    let process_name = process_name.trim();
    BUILT_IN_EXCLUSIONS
        .iter()
        .any(|excluded| excluded.eq_ignore_ascii_case(process_name))
}

fn matching_rule<'a>(
    settings: &'a CpuLimiterSettings,
    process_name: &str,
) -> Option<&'a CpuLimiterRule> {
    settings.rules.iter().find(|rule| {
        rule.enabled
            && !rule.process_name.trim().is_empty()
            && rule
                .process_name
                .trim()
                .eq_ignore_ascii_case(process_name.trim())
    })
}

fn should_ignore_foreground_process(
    settings: &CpuLimiterSettings,
    process_id: u32,
    process_name: &str,
    foreground_process_id: Option<u32>,
    foreground_process_name: Option<&str>,
) -> bool {
    settings.exclude_foreground_app
        && (foreground_process_id.is_some_and(|id| id == process_id)
            || foreground_process_name
                .is_some_and(|name| name.eq_ignore_ascii_case(process_name.trim())))
}

fn process_failure_key(process_name: &str) -> String {
    process_name.trim().to_ascii_lowercase()
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

struct CpuLimiterActionBackend<'a, 'log> {
    process_id: u32,
    process_name: String,
    limited: &'a mut BTreeMap<u32, LimitedProcess>,
    action_log: &'log mut ActionLog,
    last_error: Option<CpuLimiterError>,
}

impl AppResourceActionBackend for CpuLimiterActionBackend<'_, '_> {
    fn set_app_efficiency_mode(&mut self, _app: &AppMatcher, _enabled: bool) -> Result<(), String> {
        Err("Core Limiter backend only supports CPU limit actions.".to_owned())
    }

    fn set_app_affinity(
        &mut self,
        _app: &AppMatcher,
        _affinity: &crate::rules::AffinityPolicy,
    ) -> Result<(), String> {
        Err("Core Limiter backend only supports CPU limit actions.".to_owned())
    }

    fn set_app_cpu_limit(
        &mut self,
        _app: &AppMatcher,
        logical_processor_percent: u8,
    ) -> Result<(), String> {
        match apply_cpu_limit_to_process(
            self.process_id,
            self.process_name.clone(),
            logical_processor_percent,
            self.limited,
            self.action_log,
        ) {
            Ok(()) => Ok(()),
            Err(error) => {
                let message = cpu_limiter_error_message(&error);
                self.last_error = Some(error);
                Err(message)
            }
        }
    }

    fn suspend_app(&mut self, _app: &AppMatcher) -> Result<(), String> {
        Err("Core Limiter backend only supports CPU limit actions.".to_owned())
    }

    fn resume_app(&mut self, _app: &AppMatcher) -> Result<(), String> {
        Err("Core Limiter backend only supports CPU limit actions.".to_owned())
    }

    fn configure_background_efficiency_policy(
        &mut self,
        _exclusions: &[AppMatcher],
        _prefer_efficiency_cores: bool,
        _logical_processor_percent: Option<u8>,
    ) -> Result<(), String> {
        Err("Core Limiter backend only supports CPU limit actions.".to_owned())
    }
}

fn apply_cpu_limit_to_process(
    process_id: u32,
    process_name: String,
    max_logical_processors: u8,
    limited: &mut BTreeMap<u32, LimitedProcess>,
    action_log: &mut ActionLog,
) -> Result<(), CpuLimiterError> {
    let process = ProcessHandle::open(process_id)?;
    let (current_affinity, system_affinity) = process.affinity_mask()?;
    let Some(target_affinity) =
        limited_affinity_mask(current_affinity, system_affinity, max_logical_processors)
    else {
        return Ok(());
    };

    if limited.get(&process_id).is_some_and(|limited| {
        limited.process_name.eq_ignore_ascii_case(&process_name)
            && limited.applied_affinity == target_affinity
            && current_affinity == target_affinity
    }) {
        return Ok(());
    }

    let previous_affinity = limited
        .get(&process_id)
        .filter(|limited| limited.process_name.eq_ignore_ascii_case(&process_name))
        .map(|limited| limited.previous_affinity)
        .unwrap_or(current_affinity);

    if current_affinity != target_affinity {
        process.set_affinity_mask(target_affinity)?;
        action_log.record(
            ActionLogFeature::CpuLimiter,
            Some(process_id),
            process_name.clone(),
            ActionLogAction::Apply,
            ActionLogResult::Applied,
            format!("Constrained affinity from {previous_affinity:#x} to {target_affinity:#x}."),
        );
    }

    limited.insert(
        process_id,
        LimitedProcess {
            process_name,
            previous_affinity,
            applied_affinity: target_affinity,
        },
    );
    Ok(())
}

fn restore_affinity(process_id: u32, process_state: LimitedProcess) -> Result<(), CpuLimiterError> {
    let process = ProcessHandle::open(process_id)?;
    process.set_affinity_mask(process_state.previous_affinity)
}

fn process_session_id(process_id: u32) -> Option<u32> {
    let mut session_id = 0;
    let ok = unsafe { ProcessIdToSessionId(process_id, &mut session_id) };
    (ok != 0).then_some(session_id)
}

fn process_cpu_sample(process_id: u32) -> Result<ProcessCpuSample, CpuLimiterError> {
    let process = ProcessHandle::open_query(process_id)?;
    process.cpu_sample()
}

fn process_cpu_usage_percent(previous: ProcessCpuSample, current: ProcessCpuSample) -> Option<f32> {
    let elapsed = current.sampled_at.duration_since(previous.sampled_at);
    let elapsed_100ns = elapsed.as_nanos() / 100;
    if elapsed_100ns == 0 {
        return None;
    }

    let cpu_delta = current
        .cpu_time_100ns
        .saturating_sub(previous.cpu_time_100ns) as f64;
    let processor_count = std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(1)
        .max(1) as f64;
    Some(((cpu_delta / (elapsed_100ns as f64 * processor_count)) * 100.0).clamp(0.0, 100.0) as f32)
}

enum CpuLimiterError {
    AccessDenied,
    ProcessExited,
    Failed(String),
}

#[derive(Default)]
struct CpuLimiterFailures {
    count: usize,
    last_error: Option<String>,
}

impl CpuLimiterFailures {
    fn record_error(
        &mut self,
        action: &str,
        process_id: u32,
        process_name: &str,
        error: CpuLimiterError,
        action_log: &mut ActionLog,
    ) {
        let message = match error {
            CpuLimiterError::AccessDenied => "Access denied.".to_owned(),
            CpuLimiterError::ProcessExited => "Process exited.".to_owned(),
            CpuLimiterError::Failed(message) => message,
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
            ActionLogFeature::CpuLimiter,
            Some(process_id),
            process_name.to_owned(),
            ActionLogAction::Fail,
            ActionLogResult::Failed,
            message,
        );
    }

    fn into_result(self) -> Result<(), CpuLimiterError> {
        match self.last_error {
            Some(error) => Err(CpuLimiterError::Failed(error)),
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

fn cpu_limiter_error_message(error: &CpuLimiterError) -> String {
    match error {
        CpuLimiterError::AccessDenied => "Access denied.".to_owned(),
        CpuLimiterError::ProcessExited => "Process exited.".to_owned(),
        CpuLimiterError::Failed(message) => message.clone(),
    }
}

fn unique_app_names<'a>(names: impl Iterator<Item = &'a str>) -> Vec<String> {
    names
        .map(|name| name.trim().to_ascii_lowercase())
        .filter(|name| !name.is_empty())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

struct ProcessHandle(HANDLE);

impl ProcessHandle {
    fn open(process_id: u32) -> Result<Self, CpuLimiterError> {
        let access_masks = [
            PROCESS_QUERY_INFORMATION | PROCESS_SET_INFORMATION,
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_INFORMATION,
        ];

        let mut last_open_error = 0;
        for access in access_masks {
            let handle = unsafe { OpenProcess(access, 0, process_id) };
            if !handle.is_null() {
                return Ok(Self(handle));
            }
            last_open_error = last_error();
        }

        Err(open_process_error(process_id, last_open_error))
    }

    fn open_query(process_id: u32) -> Result<Self, CpuLimiterError> {
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
        if !handle.is_null() {
            Ok(Self(handle))
        } else {
            Err(open_process_error(process_id, last_error()))
        }
    }

    fn affinity_mask(&self) -> Result<(usize, usize), CpuLimiterError> {
        let mut process_affinity = 0;
        let mut system_affinity = 0;
        let ok =
            unsafe { GetProcessAffinityMask(self.0, &mut process_affinity, &mut system_affinity) };
        if ok == 0 {
            Err(CpuLimiterError::Failed(format!(
                "GetProcessAffinityMask failed with error {}.",
                last_error()
            )))
        } else {
            Ok((process_affinity, system_affinity))
        }
    }

    fn set_affinity_mask(&self, affinity_mask: usize) -> Result<(), CpuLimiterError> {
        let ok = unsafe { SetProcessAffinityMask(self.0, affinity_mask) };
        if ok == 0 {
            Err(CpuLimiterError::Failed(format!(
                "SetProcessAffinityMask failed with error {}.",
                last_error()
            )))
        } else {
            Ok(())
        }
    }

    fn cpu_sample(&self) -> Result<ProcessCpuSample, CpuLimiterError> {
        let mut creation = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        let ok =
            unsafe { GetProcessTimes(self.0, &mut creation, &mut exit, &mut kernel, &mut user) };
        if ok == 0 {
            Err(CpuLimiterError::Failed(format!(
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

impl Drop for ProcessHandle {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

fn open_process_error(process_id: u32, error: u32) -> CpuLimiterError {
    match error {
        ERROR_ACCESS_DENIED => CpuLimiterError::AccessDenied,
        ERROR_INVALID_PARAMETER => CpuLimiterError::ProcessExited,
        _ => CpuLimiterError::Failed(format!(
            "OpenProcess({process_id}) failed with error {error}."
        )),
    }
}

fn last_error() -> u32 {
    unsafe { GetLastError() }
}

fn filetime_to_u64(value: FILETIME) -> u64 {
    (u64::from(value.dwHighDateTime) << 32) | u64::from(value.dwLowDateTime)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matching_rule_is_case_insensitive() {
        let settings = CpuLimiterSettings {
            enabled: true,
            exclude_foreground_app: true,
            rules: vec![CpuLimiterRule {
                enabled: true,
                process_name: " Worker.EXE ".to_owned(),
                threshold_percent: 75,
                sustain_seconds: 5,
                cooldown_seconds: 10,
                max_logical_processors: 1,
            }],
        };

        assert!(matching_rule(&settings, "worker.exe").is_some());
        assert!(matching_rule(&settings, "other.exe").is_none());
    }

    #[test]
    fn builtin_exclusions_cover_sensitive_windows_processes() {
        assert!(is_builtin_excluded("csrss.exe"));
        assert!(is_builtin_excluded("winlogon.exe"));
        assert!(!is_builtin_excluded("worker.exe"));
    }

    #[test]
    fn foreground_skip_matches_pid_or_name() {
        let settings = CpuLimiterSettings {
            enabled: true,
            exclude_foreground_app: true,
            rules: Vec::new(),
        };

        assert!(should_ignore_foreground_process(
            &settings,
            42,
            "helper.exe",
            Some(42),
            Some("app.exe"),
        ));
        assert!(should_ignore_foreground_process(
            &settings,
            99,
            "APP.EXE",
            Some(42),
            Some("app.exe"),
        ));
        assert!(!should_ignore_foreground_process(
            &settings,
            99,
            "other.exe",
            Some(42),
            Some("app.exe"),
        ));
    }

    #[test]
    fn repeated_failures_suppress_future_core_limiter_attempts_once() {
        let mut manager = CpuLimiterManager::default();
        let mut log = ActionLog::new(8);

        manager.record_process_failure("APP.exe");
        manager.record_process_failure("app.exe");
        assert!(!manager.is_process_suppressed(42, "app.exe", &mut log));
        assert!(log.entries().is_empty());

        manager.record_process_failure("app.exe");
        assert!(manager.is_process_suppressed(42, "app.exe", &mut log));
        assert!(manager.is_process_suppressed(43, "APP.exe", &mut log));

        let entries = log.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].process_name, "app.exe");
        assert_eq!(entries[0].action, ActionLogAction::Skip);
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
    fn limiter_backend_rejects_non_cpu_limit_resource_actions() {
        let mut limited = BTreeMap::new();
        let mut log = ActionLog::new(8);
        let mut backend = CpuLimiterActionBackend {
            process_id: 42,
            process_name: "worker.exe".to_owned(),
            limited: &mut limited,
            action_log: &mut log,
            last_error: None,
        };

        assert_eq!(
            ActionExecutor.apply_app_resource_action(
                &Action::SuspendApp {
                    app: AppMatcher::ProcessName("worker.exe".to_owned()),
                },
                &mut backend,
            ),
            ActionExecution::Failed(
                "Core Limiter backend only supports CPU limit actions.".to_owned()
            )
        );
        assert!(limited.is_empty());
        assert!(log.entries().is_empty());
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
        let mut manager = CpuLimiterManager::default();
        manager.limited.insert(
            0,
            LimitedProcess {
                process_name: "exited.exe".to_owned(),
                previous_affinity: 0b1111,
                applied_affinity: 0b0001,
            },
        );
        let mut log = ActionLog::new(8);

        let failures = manager.release_processes(&[0], Some(&BTreeMap::new()), &mut log, "test");

        assert_eq!(failures.count, 0);
        assert!(log.entries().is_empty());
        assert!(manager.limited.is_empty());
    }
}
