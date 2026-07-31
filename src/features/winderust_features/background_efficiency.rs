use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::c_void,
    path::Path,
};

use windows_sys::Win32::{
    Foundation::{ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER, ERROR_NOT_SUPPORTED},
    System::Threading::{
        GetCurrentProcessId, GetPriorityClass, GetProcessInformation, OpenProcess,
        ProcessPowerThrottling, SetPriorityClass, SetProcessInformation, IDLE_PRIORITY_CLASS,
        NORMAL_PRIORITY_CLASS, PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        PROCESS_POWER_THROTTLING_EXECUTION_SPEED, PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
        PROCESS_POWER_THROTTLING_STATE, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_INFORMATION,
    },
};

use crate::win_util::{last_error, WinHandle};

use crate::{
    action_log::{ActionLog, ActionLogFeature, ActionLogResult},
    audio_activity::active_audio_process_ids,
    config::{BackgroundEfficiencyAggressiveness, BackgroundEfficiencySettings},
    foreground::{
        contains_process_name, ensure_process_action_target_access, list_processes,
        process_executable_path, process_failure_key, process_handle_matches_executable_path,
        process_session_id, should_ignore_foreground_process, ProcessActionAccess,
        ProcessActionTarget,
    },
    rules::{
        execution_failure_suppression_threshold, ExecutionFailureTracker, ExecutionSuppression,
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
    "searchhost.exe",
    "securityhealthservice.exe",
    "securityhealthsystray.exe",
    "services.exe",
    "shellexperiencehost.exe",
    "sihost.exe",
    "smss.exe",
    "startmenuexperiencehost.exe",
    "system",
    "taskmgr.exe",
    "textinputhost.exe",
    "wininit.exe",
    "winlogon.exe",
    "wudfhost.exe",
];

const BALANCED_BUILT_IN_EXCLUSIONS: &[&str] = &[
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
    "securityhealthservice.exe",
    "securityhealthsystray.exe",
    "services.exe",
    "sihost.exe",
    "smss.exe",
    "startmenuexperiencehost.exe",
    "system",
    "taskmgr.exe",
    "textinputhost.exe",
    "wininit.exe",
    "winlogon.exe",
];

const AGGRESSIVE_BUILT_IN_EXCLUSIONS: &[&str] = &[
    "csrss.exe",
    "lsaiso.exe",
    "lsass.exe",
    "registry",
    "services.exe",
    "smss.exe",
    "system",
    "wininit.exe",
    "winlogon.exe",
];
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackgroundEfficiencySnapshot {
    pub enabled: bool,
    pub unsupported: bool,
    pub scanned_processes: usize,
    pub throttled_processes: usize,
    pub timer_resolution_ignored_processes: usize,
    pub skipped_processes: usize,
    pub access_denied_processes: usize,
    pub failed_processes: usize,
    pub message: String,
    pub last_error: Option<String>,
}

#[derive(Default)]
pub struct BackgroundEfficiencyManager {
    throttled: BTreeMap<u32, ThrottledProcess>,
    failure_suppression: ExecutionFailureTracker,
}

#[derive(Clone)]
struct ThrottledProcess {
    process_name: String,
    executable_path: String,
    creation_time: u64,
    previous_state: Option<PROCESS_POWER_THROTTLING_STATE>,
    previous_priority: Option<u32>,
    applied_ignore_timer_resolution: bool,
}

impl BackgroundEfficiencyManager {
    pub fn throttled_process_ids(&self) -> BTreeSet<u32> {
        self.throttled.keys().copied().collect()
    }

    pub fn update(
        &mut self,
        settings: &BackgroundEfficiencySettings,
        automation_enabled: bool,
        allow_cross_session_process_control: bool,
        foreground_process_id: Option<u32>,
        manage_process_priority: bool,
        action_log: &mut ActionLog,
    ) -> BackgroundEfficiencySnapshot {
        if !automation_enabled {
            let failed = self.clear_all(action_log, "automation disabled");
            self.failure_suppression.clear();
            return BackgroundEfficiencySnapshot {
                enabled: false,
                failed_processes: failed.count,
                message: "Automation disabled.".to_owned(),
                last_error: failed.last_error,
                ..Default::default()
            };
        }

        if !settings.enabled {
            let failed = self.clear_all(action_log, "Background Efficiency disabled");
            self.failure_suppression.clear();
            return BackgroundEfficiencySnapshot {
                enabled: false,
                failed_processes: failed.count,
                message: "Background Efficiency disabled.".to_owned(),
                last_error: failed.last_error,
                ..Default::default()
            };
        }

        if settings.exclude_foreground_app && foreground_process_id.is_none() {
            let failed = self.clear_all(action_log, "foreground app is unknown");
            return BackgroundEfficiencySnapshot {
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
            return BackgroundEfficiencySnapshot {
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
                return BackgroundEfficiencySnapshot {
                    enabled: true,
                    failed_processes: failed.count,
                    message: err,
                    last_error: failed.last_error,
                    ..Default::default()
                };
            }
        };

        let scanned_processes = processes.len();
        let mut skipped_processes = 0;
        let mut access_denied_processes = 0;
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
                || !process.can_set_information
                || process.id == current_process_id
                || is_builtin_excluded_for(&process.name, settings.aggressiveness)
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
            ) || settings.custom_rule_enabled_for(executable_path.to_string_lossy().as_ref())
            {
                continue;
            }
            target_processes.insert(
                process.id,
                (process.name, executable_path.to_string_lossy().into_owned()),
            );
        }

        let active_target_names = target_processes
            .values()
            .map(|(_name, path)| process_failure_key(path))
            .collect::<BTreeSet<_>>();
        self.failure_suppression.retain_keys(&active_target_names);

        let target_ids = target_processes.keys().copied().collect::<BTreeSet<_>>();
        let mut failures =
            self.release_non_targets(&target_ids, action_log, "process no longer matches EcoQoS");
        let mut unsupported = false;
        let active_audio_process_ids = active_audio_process_ids().ok();

        for (process_id, (name, executable_path)) in target_processes {
            let suppression =
                self.check_process_suppression(process_id, &name, &executable_path, action_log);
            if suppression.suppressed {
                skipped_processes += 1;
                continue;
            }

            match apply_background_efficiency_to_process(
                process_id,
                name.clone(),
                executable_path.clone(),
                ignore_timer_resolution_allowed(process_id, active_audio_process_ids.as_ref()),
                manage_process_priority,
                &mut self.throttled,
                action_log,
            ) {
                Ok(()) => self.clear_process_failure(&executable_path),
                Err(BackgroundEfficiencyError::ProcessExited) => {
                    skipped_processes += 1;
                    self.throttled.remove(&process_id);
                }
                Err(BackgroundEfficiencyError::AccessDenied) => {
                    skipped_processes += 1;
                    access_denied_processes += 1;
                    self.failure_suppression
                        .suppress_process_failure(&executable_path);
                    action_log.record(
                        ActionLogFeature::BackgroundEfficiency,
                        Some(process_id),
                        name,
                        ActionLogResult::Skipped,
                        "Skipped because the process could not be opened.",
                    );
                }
                Err(BackgroundEfficiencyError::Unsupported) => {
                    skipped_processes += 1;
                    unsupported = true;
                    self.failure_suppression
                        .suppress_process_failure(&executable_path);
                    action_log.record(
                        ActionLogFeature::BackgroundEfficiency,
                        Some(process_id),
                        name,
                        ActionLogResult::Skipped,
                        "Skipped because Windows process power throttling is unsupported.",
                    );
                }
                Err(error) => {
                    failures.record_error("Apply", process_id, &name, error, action_log);
                    self.record_process_failure(&executable_path);
                }
            }
        }

        BackgroundEfficiencySnapshot {
            enabled: true,
            unsupported,
            scanned_processes,
            throttled_processes: self.throttled.len(),
            timer_resolution_ignored_processes: self
                .throttled
                .values()
                .filter(|process| process.applied_ignore_timer_resolution)
                .count(),
            skipped_processes,
            access_denied_processes,
            failed_processes: failures.count,
            message: "Background Efficiency active.".to_owned(),
            last_error: failures.last_error,
        }
    }

    fn release_non_targets(
        &mut self,
        target_ids: &BTreeSet<u32>,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> BackgroundEfficiencyFailures {
        let process_ids = self
            .throttled
            .keys()
            .copied()
            .filter(|process_id| !target_ids.contains(process_id))
            .collect::<Vec<_>>();

        self.release_processes(&process_ids, action_log, reason)
    }

    fn clear_all(
        &mut self,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> BackgroundEfficiencyFailures {
        let process_ids = self.throttled.keys().copied().collect::<Vec<_>>();
        self.release_processes(&process_ids, action_log, reason)
    }

    fn release_processes(
        &mut self,
        process_ids: &[u32],
        action_log: &mut ActionLog,
        reason: &str,
    ) -> BackgroundEfficiencyFailures {
        let mut failures = BackgroundEfficiencyFailures::default();
        for process_id in process_ids {
            let Some(process) = self.throttled.get(process_id).cloned() else {
                continue;
            };
            let process_name = process.process_name.clone();
            match restore_background_efficiency(*process_id, &process) {
                Ok(()) => {
                    self.throttled.remove(process_id);
                    action_log.record(
                        ActionLogFeature::BackgroundEfficiency,
                        Some(*process_id),
                        process_name,
                        ActionLogResult::Restored,
                        reason.to_owned(),
                    );
                }
                Err(BackgroundEfficiencyError::ProcessExited) => {
                    self.throttled.remove(process_id);
                }
                Err(error) => {
                    failures.record_error("Restore", *process_id, &process_name, error, action_log);
                }
            }
        }
        failures
    }

    fn check_process_suppression(
        &mut self,
        process_id: u32,
        process_name: &str,
        executable_path: &str,
        action_log: &mut ActionLog,
    ) -> ExecutionSuppression {
        let suppression = self
            .failure_suppression
            .process_suppression(executable_path);
        if suppression.newly_suppressed {
            action_log.record(
                ActionLogFeature::BackgroundEfficiency,
                Some(process_id),
                process_name.to_owned(),
                ActionLogResult::Skipped,
                format!(
                    "Stopped retrying Background Efficiency after {} failed attempts.",
                    execution_failure_suppression_threshold(),
                ),
            );
        }

        suppression
    }

    #[cfg(test)]
    fn is_process_suppressed(
        &mut self,
        process_id: u32,
        process_name: &str,
        action_log: &mut ActionLog,
    ) -> bool {
        self.check_process_suppression(process_id, process_name, process_name, action_log)
            .suppressed
    }

    fn record_process_failure(&mut self, process_name: &str) {
        self.failure_suppression
            .record_process_failure(process_name);
    }

    fn clear_process_failure(&mut self, process_name: &str) {
        self.failure_suppression.clear_process_failure(process_name);
    }
}

impl Drop for BackgroundEfficiencyManager {
    fn drop(&mut self) {
        let mut action_log = ActionLog::new(1);
        self.clear_all(&mut action_log, "Background Efficiency manager dropped");
    }
}

impl Default for BackgroundEfficiencySnapshot {
    fn default() -> Self {
        Self {
            enabled: false,
            unsupported: false,
            scanned_processes: 0,
            throttled_processes: 0,
            timer_resolution_ignored_processes: 0,
            skipped_processes: 0,
            access_denied_processes: 0,
            failed_processes: 0,
            message: "Background Efficiency disabled.".to_owned(),
            last_error: None,
        }
    }
}

pub fn is_builtin_excluded(process_name: &str) -> bool {
    is_builtin_excluded_for(process_name, BackgroundEfficiencyAggressiveness::Safe)
}

#[cfg(test)]
fn is_process_excluded(process: &str, settings: &BackgroundEfficiencySettings) -> bool {
    let process_name = std::path::Path::new(process)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(process);
    is_builtin_excluded_for(process_name, settings.aggressiveness)
        || settings.custom_rule_enabled_for(process)
}

fn is_builtin_excluded_for(
    process_name: &str,
    aggressiveness: BackgroundEfficiencyAggressiveness,
) -> bool {
    contains_process_name(built_in_exclusions_for(aggressiveness), process_name)
}

fn built_in_exclusions_for(
    aggressiveness: BackgroundEfficiencyAggressiveness,
) -> &'static [&'static str] {
    match aggressiveness {
        BackgroundEfficiencyAggressiveness::Safe => BUILT_IN_EXCLUSIONS,
        BackgroundEfficiencyAggressiveness::Balanced => BALANCED_BUILT_IN_EXCLUSIONS,
        BackgroundEfficiencyAggressiveness::Aggressive => AGGRESSIVE_BUILT_IN_EXCLUSIONS,
    }
}

enum BackgroundEfficiencyError {
    AccessDenied,
    ProcessExited,
    Unsupported,
    Failed(String),
}

#[derive(Default)]
struct BackgroundEfficiencyFailures {
    count: usize,
    last_error: Option<String>,
}

impl BackgroundEfficiencyFailures {
    fn record_error(
        &mut self,
        action: &str,
        process_id: u32,
        process_name: &str,
        error: BackgroundEfficiencyError,
        action_log: &mut ActionLog,
    ) {
        let message = match error {
            BackgroundEfficiencyError::AccessDenied => "Access denied.".to_owned(),
            BackgroundEfficiencyError::ProcessExited => return,
            BackgroundEfficiencyError::Unsupported => "Operation unsupported.".to_owned(),
            BackgroundEfficiencyError::Failed(message) => message,
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
            ActionLogFeature::BackgroundEfficiency,
            Some(process_id),
            process_name.to_owned(),
            ActionLogResult::Failed,
            message,
        );
    }
}

fn apply_background_efficiency_to_process(
    process_id: u32,
    process_name: String,
    executable_path: String,
    ignore_timer_resolution: bool,
    manage_process_priority: bool,
    throttled: &mut BTreeMap<u32, ThrottledProcess>,
    action_log: &mut ActionLog,
) -> Result<(), BackgroundEfficiencyError> {
    if let Some(process) = throttled.get_mut(&process_id) {
        let update = update_background_efficiency(
            process_id,
            process,
            ignore_timer_resolution,
            manage_process_priority,
        );
        match update {
            Ok(()) => return Ok(()),
            Err(BackgroundEfficiencyError::ProcessExited) => {
                throttled.remove(&process_id);
            }
            Err(err) => return Err(err),
        }
    }

    let process = enable_background_efficiency(
        process_id,
        process_name.clone(),
        executable_path,
        ignore_timer_resolution,
        manage_process_priority,
    )?;
    throttled.insert(process_id, process);
    action_log.record(
        ActionLogFeature::BackgroundEfficiency,
        Some(process_id),
        process_name,
        ActionLogResult::Applied,
        "Applied Background Efficiency: enabled EcoQoS and lowered priority.".to_owned(),
    );
    Ok(())
}

fn update_background_efficiency(
    process_id: u32,
    process_state: &mut ThrottledProcess,
    ignore_timer_resolution: bool,
    manage_process_priority: bool,
) -> Result<(), BackgroundEfficiencyError> {
    let process = ProcessHandle::open(process_id)?;
    if process.0.process_creation_time() != Some(process_state.creation_time)
        || !process_handle_matches_executable_path(
            &process.0,
            Path::new(&process_state.executable_path),
        )
    {
        return Err(BackgroundEfficiencyError::ProcessExited);
    }
    if process_state.applied_ignore_timer_resolution == ignore_timer_resolution
        && process_state.previous_priority.is_some() == manage_process_priority
    {
        return Ok(());
    }
    process.set_power_throttling_state(power_throttling_enabled_state(
        process_state
            .previous_state
            .unwrap_or_else(system_managed_power_throttling_state),
        ignore_timer_resolution,
    ))?;
    if manage_process_priority && process_state.previous_priority.is_none() {
        let previous_priority = process.priority_class()?;
        process.set_priority_class(IDLE_PRIORITY_CLASS)?;
        process_state.previous_priority = Some(previous_priority);
    } else if !manage_process_priority {
        if let Some(previous_priority) = process_state.previous_priority {
            process.set_priority_class(previous_priority)?;
            process_state.previous_priority = None;
        }
    }
    process_state.applied_ignore_timer_resolution = ignore_timer_resolution;
    Ok(())
}

fn process_failure_message(
    action: &str,
    process_id: u32,
    process_name: &str,
    message: &str,
) -> String {
    format!("{action} {process_name} ({process_id}): {message}")
}

fn enable_background_efficiency(
    process_id: u32,
    process_name: String,
    executable_path: String,
    ignore_timer_resolution: bool,
    manage_process_priority: bool,
) -> Result<ThrottledProcess, BackgroundEfficiencyError> {
    let process = ProcessHandle::open(process_id)?;
    if !process_handle_matches_executable_path(&process.0, Path::new(&executable_path)) {
        return Err(BackgroundEfficiencyError::ProcessExited);
    }
    let creation_time = process
        .0
        .process_creation_time()
        .ok_or(BackgroundEfficiencyError::ProcessExited)?;
    let previous_state = match process.power_throttling_state() {
        Ok(state) => Some(state),
        Err(BackgroundEfficiencyError::Unsupported) => None,
        Err(error) => return Err(error),
    };
    let previous_priority = manage_process_priority
        .then(|| process.priority_class())
        .transpose()?;

    let restore_state = previous_state.unwrap_or_else(system_managed_power_throttling_state);
    let next_state = power_throttling_enabled_state(restore_state, ignore_timer_resolution);
    process.set_power_throttling_state(next_state)?;
    if manage_process_priority {
        if let Err(err) = process.set_priority_class(IDLE_PRIORITY_CLASS) {
            return Err(background_efficiency_rollback_error(
                err,
                process.set_power_throttling_state(restore_state),
            ));
        }
    }

    Ok(ThrottledProcess {
        process_name,
        executable_path,
        creation_time,
        previous_state,
        previous_priority,
        applied_ignore_timer_resolution: ignore_timer_resolution,
    })
}

fn restore_background_efficiency(
    process_id: u32,
    process_state: &ThrottledProcess,
) -> Result<(), BackgroundEfficiencyError> {
    let process = ProcessHandle::open(process_id)?;
    if process.0.process_creation_time() != Some(process_state.creation_time)
        || !process_handle_matches_executable_path(
            &process.0,
            Path::new(&process_state.executable_path),
        )
    {
        return Err(BackgroundEfficiencyError::ProcessExited);
    }
    let mut last_error = None;

    if let Err(err) = process.set_power_throttling_state(
        process_state
            .previous_state
            .unwrap_or_else(system_managed_power_throttling_state),
    ) {
        last_error = Some(err);
    }

    if let Some(previous_priority) = process_state.previous_priority {
        if let Err(err) = process.set_priority_class(previous_priority) {
            last_error = Some(err);
        }
    }

    match last_error {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

#[cfg(test)]
fn power_throttling_disabled_state() -> PROCESS_POWER_THROTTLING_STATE {
    PROCESS_POWER_THROTTLING_STATE {
        Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
        StateMask: 0,
    }
}

fn system_managed_power_throttling_state() -> PROCESS_POWER_THROTTLING_STATE {
    PROCESS_POWER_THROTTLING_STATE {
        Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        ControlMask: 0,
        StateMask: 0,
    }
}

fn power_throttling_enabled_state(
    mut state: PROCESS_POWER_THROTTLING_STATE,
    ignore_timer_resolution: bool,
) -> PROCESS_POWER_THROTTLING_STATE {
    let previous_ignore_timer_resolution =
        (state.StateMask & PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION) != 0;
    state.Version = PROCESS_POWER_THROTTLING_CURRENT_VERSION;
    state.ControlMask |=
        PROCESS_POWER_THROTTLING_EXECUTION_SPEED | PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION;
    state.StateMask |= PROCESS_POWER_THROTTLING_EXECUTION_SPEED;
    if ignore_timer_resolution || previous_ignore_timer_resolution {
        state.StateMask |= PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION;
    } else {
        state.StateMask &= !PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION;
    }
    state
}

fn ignore_timer_resolution_allowed(
    process_id: u32,
    active_audio_process_ids: Option<&BTreeSet<u32>>,
) -> bool {
    active_audio_process_ids.is_some_and(|ids| !ids.contains(&process_id))
}

struct ProcessHandle(WinHandle);

impl ProcessHandle {
    fn open_query(process_id: u32) -> Result<Self, BackgroundEfficiencyError> {
        // SAFETY: process_id came from the current process snapshot, only query access is
        // requested, and no inherited handle is requested.
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
        if handle.is_null() {
            Err(open_process_error(process_id, last_error()))
        } else {
            Ok(Self(WinHandle::new(handle)))
        }
    }

    fn open(process_id: u32) -> Result<Self, BackgroundEfficiencyError> {
        let access = PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_INFORMATION;
        // SAFETY: process_id came from the current process snapshot, the documented query and
        // mutation rights are requested, and no inherited handle is requested.
        let handle = unsafe { OpenProcess(access, 0, process_id) };
        if handle.is_null() {
            Err(open_process_error(process_id, last_error()))
        } else {
            Ok(Self(WinHandle::new(handle)))
        }
    }

    fn power_throttling_state(
        &self,
    ) -> Result<PROCESS_POWER_THROTTLING_STATE, BackgroundEfficiencyError> {
        let mut state = PROCESS_POWER_THROTTLING_STATE {
            Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
            ..Default::default()
        };
        // SAFETY: self owns a live process handle and state is writable for exactly the supplied
        // structure size.
        let ok = unsafe {
            GetProcessInformation(
                self.0.raw(),
                ProcessPowerThrottling,
                &mut state as *mut _ as *mut c_void,
                std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
            )
        };
        if ok == 0 {
            Err(process_power_throttling_error(
                "GetProcessInformation",
                last_error(),
            ))
        } else {
            Ok(state)
        }
    }

    fn priority_class(&self) -> Result<u32, BackgroundEfficiencyError> {
        // SAFETY: self owns a live process handle.
        let priority = unsafe { GetPriorityClass(self.0.raw()) };
        if priority == 0 {
            Err(BackgroundEfficiencyError::Failed(format!(
                "GetPriorityClass failed with error {}.",
                last_error()
            )))
        } else {
            Ok(priority)
        }
    }

    fn set_power_throttling_state(
        &self,
        state: PROCESS_POWER_THROTTLING_STATE,
    ) -> Result<(), BackgroundEfficiencyError> {
        // SAFETY: self owns a live process handle and state is fully initialized for exactly the
        // supplied structure size.
        let ok = unsafe {
            SetProcessInformation(
                self.0.raw(),
                ProcessPowerThrottling,
                &state as *const _ as *const c_void,
                std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
            )
        };
        if ok == 0 {
            Err(process_power_throttling_error(
                "SetProcessInformation",
                last_error(),
            ))
        } else {
            Ok(())
        }
    }

    fn set_priority_class(&self, priority_class: u32) -> Result<(), BackgroundEfficiencyError> {
        // SAFETY: self owns a live process handle and priority_class is a documented class or a
        // previously read value.
        let ok = unsafe { SetPriorityClass(self.0.raw(), priority_class) };
        if ok == 0 {
            Err(BackgroundEfficiencyError::Failed(format!(
                "SetPriorityClass failed with error {}.",
                last_error()
            )))
        } else {
            Ok(())
        }
    }
}

fn process_power_throttling_error(operation: &str, error: u32) -> BackgroundEfficiencyError {
    match error {
        ERROR_INVALID_PARAMETER | ERROR_NOT_SUPPORTED => BackgroundEfficiencyError::Unsupported,
        _ => BackgroundEfficiencyError::Failed(format!("{operation} failed with error {error}.")),
    }
}

fn open_process_error(process_id: u32, error: u32) -> BackgroundEfficiencyError {
    match error {
        ERROR_ACCESS_DENIED => BackgroundEfficiencyError::AccessDenied,
        ERROR_INVALID_PARAMETER => BackgroundEfficiencyError::ProcessExited,
        _ => BackgroundEfficiencyError::Failed(format!(
            "OpenProcess({process_id}) failed with error {error}."
        )),
    }
}

pub(crate) fn current_efficiency_mode(target: &ProcessActionTarget) -> Result<bool, String> {
    let process =
        ProcessHandle::open_query(target.id).map_err(background_efficiency_error_message)?;
    if process.0.process_creation_time() != Some(target.creation_time)
        || !process_handle_matches_executable_path(&process.0, &target.executable_path)
    {
        return Err("The selected process instance has changed.".to_owned());
    }
    let eco_qos = process
        .power_throttling_state()
        .map(|state| {
            state.ControlMask & PROCESS_POWER_THROTTLING_EXECUTION_SPEED != 0
                && state.StateMask & PROCESS_POWER_THROTTLING_EXECUTION_SPEED != 0
        })
        .map_err(background_efficiency_error_message)?;
    process
        .priority_class()
        .map(|priority| eco_qos && priority == IDLE_PRIORITY_CLASS)
        .map_err(background_efficiency_error_message)
}

pub(crate) fn apply_efficiency_mode_once(
    target: &ProcessActionTarget,
    enabled: bool,
    previous_priority: Option<u32>,
) -> Result<Option<u32>, String> {
    ensure_process_action_target_access(target, ProcessActionAccess::SetInformation)?;
    if is_builtin_excluded(&target.name) {
        return Err("Built-in Windows processes cannot be modified.".to_owned());
    }
    let process = ProcessHandle::open(target.id).map_err(background_efficiency_error_message)?;
    if process.0.process_creation_time() != Some(target.creation_time)
        || !process_handle_matches_executable_path(&process.0, &target.executable_path)
    {
        return Err("The selected process instance has changed.".to_owned());
    }
    let previous_state = process
        .power_throttling_state()
        .map_err(background_efficiency_error_message)?;
    let current_priority = process
        .priority_class()
        .map_err(background_efficiency_error_message)?;
    let state = PROCESS_POWER_THROTTLING_STATE {
        Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
        StateMask: if enabled {
            PROCESS_POWER_THROTTLING_EXECUTION_SPEED
        } else {
            0
        },
    };
    process
        .set_power_throttling_state(state)
        .map_err(background_efficiency_error_message)?;
    let target_priority = if enabled {
        IDLE_PRIORITY_CLASS
    } else {
        previous_priority.unwrap_or(NORMAL_PRIORITY_CLASS)
    };
    if let Err(error) = process.set_priority_class(target_priority) {
        return Err(background_efficiency_error_message(
            background_efficiency_rollback_error(
                error,
                process.set_power_throttling_state(previous_state),
            ),
        ));
    }
    let changed = process.power_throttling_state().and_then(|state| {
        process.priority_class().map(|priority| {
            let eco_qos = state.ControlMask & PROCESS_POWER_THROTTLING_EXECUTION_SPEED != 0
                && state.StateMask & PROCESS_POWER_THROTTLING_EXECUTION_SPEED != 0;
            eco_qos == enabled && priority == target_priority
        })
    });
    match changed {
        Ok(true) => Ok(enabled.then_some(current_priority)),
        Ok(false) => {
            let error = BackgroundEfficiencyError::Failed(
                "Efficiency mode did not change after request.".to_owned(),
            );
            Err(background_efficiency_error_message(
                background_efficiency_rollback_error(
                    error,
                    (|| {
                        process.set_power_throttling_state(previous_state)?;
                        process.set_priority_class(current_priority)
                    })(),
                ),
            ))
        }
        Err(error) => Err(background_efficiency_error_message(
            background_efficiency_rollback_error(
                error,
                (|| {
                    process.set_power_throttling_state(previous_state)?;
                    process.set_priority_class(current_priority)
                })(),
            ),
        )),
    }
}

fn background_efficiency_error_message(error: BackgroundEfficiencyError) -> String {
    match error {
        BackgroundEfficiencyError::AccessDenied => "Access denied.".to_owned(),
        BackgroundEfficiencyError::ProcessExited => "Process exited.".to_owned(),
        BackgroundEfficiencyError::Unsupported => "Operation unsupported.".to_owned(),
        BackgroundEfficiencyError::Failed(message) => message,
    }
}

fn background_efficiency_rollback_error(
    operation_error: BackgroundEfficiencyError,
    rollback: Result<(), BackgroundEfficiencyError>,
) -> BackgroundEfficiencyError {
    match rollback {
        Ok(()) => operation_error,
        Err(rollback_error) => BackgroundEfficiencyError::Failed(format!(
            "{} Rollback also failed: {}",
            background_efficiency_error_message(operation_error),
            background_efficiency_error_message(rollback_error)
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rollback_failure_preserves_both_background_efficiency_errors() {
        let error = background_efficiency_rollback_error(
            BackgroundEfficiencyError::AccessDenied,
            Err(BackgroundEfficiencyError::Failed(
                "SetProcessInformation failed.".to_owned(),
            )),
        );

        assert_eq!(
            background_efficiency_error_message(error),
            "Access denied. Rollback also failed: SetProcessInformation failed."
        );
    }

    #[test]
    fn successful_rollback_preserves_the_original_background_efficiency_error() {
        let error =
            background_efficiency_rollback_error(BackgroundEfficiencyError::AccessDenied, Ok(()));

        assert_eq!(background_efficiency_error_message(error), "Access denied.");
    }

    #[test]
    fn efficiency_mode_round_trips_on_live_process() {
        let command = std::env::var_os("ComSpec").expect("ComSpec is defined on Windows");
        let mut child = std::process::Command::new(&command)
            .args(["/d", "/c", "ping -n 30 127.0.0.1 > nul"])
            .spawn()
            .expect("test process starts");
        let result: Result<(), String> = (|| {
            let target = crate::foreground::capture_process_action_target(
                child.id(),
                Path::new(&command),
                false,
            )
            .map_err(|error| error.to_string())?;
            let previous_priority = apply_efficiency_mode_once(&target, true, None)
                .map_err(|error| format!("enable: {error}"))?;
            assert!(current_efficiency_mode(&target)?);
            apply_efficiency_mode_once(&target, false, previous_priority)
                .map_err(|error| format!("disable: {error}"))?;
            assert!(!current_efficiency_mode(&target)?);
            let managed = enable_background_efficiency(
                target.id,
                target.name.clone(),
                target.executable_path.to_string_lossy().into_owned(),
                false,
                true,
            )
            .map_err(background_efficiency_error_message)?;
            assert!(current_efficiency_mode(&target)?);
            restore_background_efficiency(target.id, &managed)
                .map_err(background_efficiency_error_message)?;
            assert!(!current_efficiency_mode(&target)?);
            Ok(())
        })();
        let _ = child.kill();
        let _ = child.wait();
        assert_eq!(result, Ok(()));
    }

    #[test]
    fn exclusions_include_builtin_and_user_entries() {
        let settings = BackgroundEfficiencySettings {
            enabled: true,
            exclude_foreground_app: true,
            aggressiveness: BackgroundEfficiencyAggressiveness::Safe,
            custom_rules: vec![crate::config::BackgroundEfficiencyRule {
                enabled: true,
                executable_path: "mouse.exe".to_owned(),
            }],
        };

        assert!(is_process_excluded("EXPLORER.EXE", &settings));
        assert!(is_process_excluded("csrss.exe", &settings));
        assert!(is_process_excluded("winlogon.exe", &settings));
        assert!(is_process_excluded("Mouse.exe", &settings));
        assert!(!is_process_excluded("browser.exe", &settings));
    }

    #[test]
    fn aggressiveness_profiles_control_builtin_exclusions() {
        let mut settings = BackgroundEfficiencySettings {
            aggressiveness: BackgroundEfficiencyAggressiveness::Safe,
            ..Default::default()
        };

        assert!(is_process_excluded("SearchHost.exe", &settings));
        assert!(is_process_excluded("dwm.exe", &settings));
        assert!(is_process_excluded("winlogon.exe", &settings));

        settings.aggressiveness = BackgroundEfficiencyAggressiveness::Balanced;
        assert!(!is_process_excluded("SearchHost.exe", &settings));
        assert!(is_process_excluded("dwm.exe", &settings));
        assert!(is_process_excluded("winlogon.exe", &settings));

        settings.aggressiveness = BackgroundEfficiencyAggressiveness::Aggressive;
        assert!(!is_process_excluded("SearchHost.exe", &settings));
        assert!(!is_process_excluded("dwm.exe", &settings));
        assert!(is_process_excluded("winlogon.exe", &settings));
    }

    #[test]
    fn disabled_user_exclusions_do_not_exclude_processes() {
        let settings = BackgroundEfficiencySettings {
            enabled: true,
            exclude_foreground_app: true,
            aggressiveness: BackgroundEfficiencyAggressiveness::Safe,
            custom_rules: vec![crate::config::BackgroundEfficiencyRule {
                enabled: false,
                executable_path: "mouse.exe".to_owned(),
            }],
        };

        assert!(settings.contains_custom_rule("MOUSE.EXE"));
        assert!(!is_process_excluded("mouse.exe", &settings));
    }

    #[test]
    fn power_throttling_unsupported_codes_mark_feature_unsupported() {
        assert!(matches!(
            process_power_throttling_error("SetProcessInformation", ERROR_NOT_SUPPORTED),
            BackgroundEfficiencyError::Unsupported
        ));
        assert!(matches!(
            process_power_throttling_error("SetProcessInformation", ERROR_INVALID_PARAMETER),
            BackgroundEfficiencyError::Unsupported
        ));
    }

    #[test]
    fn process_failure_message_includes_action_name_pid_and_error() {
        assert_eq!(
            process_failure_message("Restore", 42, "browser.exe", "OpenProcess failed."),
            "Restore browser.exe (42): OpenProcess failed."
        );
    }

    #[test]
    fn open_process_invalid_parameter_means_process_exited() {
        assert!(matches!(
            open_process_error(42, ERROR_INVALID_PARAMETER),
            BackgroundEfficiencyError::ProcessExited
        ));
    }

    #[test]
    fn repeated_failures_suppress_future_efficiency_attempts_once() {
        let mut manager = BackgroundEfficiencyManager::default();
        let mut log = ActionLog::new(8);
        let executable_path = r"C:\Apps\app.exe";

        manager.record_process_failure(executable_path);
        manager.record_process_failure(executable_path);
        assert!(
            !manager
                .check_process_suppression(42, "app.exe", executable_path, &mut log)
                .suppressed
        );
        assert!(log.entries().is_empty());

        manager.record_process_failure(executable_path);
        assert!(
            manager
                .check_process_suppression(42, "app.exe", executable_path, &mut log)
                .suppressed
        );
        assert!(manager.is_process_suppressed(43, r"C:/Apps/app.exe", &mut log));

        let entries = log.entries();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].process_name, "app.exe");
        assert_eq!(entries[0].result, ActionLogResult::Skipped);
    }

    #[test]
    fn first_suppression_reports_auto_exclusion_once() {
        let mut manager = BackgroundEfficiencyManager::default();
        let mut log = ActionLog::new(8);

        manager.record_process_failure("app.exe");
        manager.record_process_failure("app.exe");
        manager.record_process_failure("app.exe");

        let first = manager.check_process_suppression(42, "app.exe", "app.exe", &mut log);
        let second = manager.check_process_suppression(42, "app.exe", "app.exe", &mut log);

        assert!(first.suppressed);
        assert!(first.newly_suppressed);
        assert!(second.suppressed);
        assert!(!second.newly_suppressed);
    }

    #[test]
    fn disabled_state_clears_execution_speed_control() {
        let state = power_throttling_disabled_state();

        assert_eq!(state.Version, PROCESS_POWER_THROTTLING_CURRENT_VERSION);
        assert_eq!(state.ControlMask, PROCESS_POWER_THROTTLING_EXECUTION_SPEED);
        assert_eq!(state.StateMask, 0);
    }

    #[test]
    fn enabled_state_sets_timer_ignore_only_when_allowed() {
        let allowed = power_throttling_enabled_state(power_throttling_disabled_state(), true);
        assert_ne!(
            allowed.StateMask & PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
            0
        );
        assert_ne!(
            allowed.StateMask & PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
            0
        );

        let blocked = power_throttling_enabled_state(power_throttling_disabled_state(), false);
        assert_ne!(
            blocked.StateMask & PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
            0
        );
        assert_eq!(
            blocked.StateMask & PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
            0
        );
    }

    #[test]
    fn timer_ignore_guard_fails_closed_for_audio_detection() {
        let mut audio_processes = BTreeSet::new();
        audio_processes.insert(42);

        assert!(!ignore_timer_resolution_allowed(42, Some(&audio_processes)));
        assert!(ignore_timer_resolution_allowed(7, Some(&audio_processes)));
        assert!(!ignore_timer_resolution_allowed(7, None));
    }

    #[test]
    fn foreground_ignore_matches_pid_or_exact_path() {
        let mut settings = BackgroundEfficiencySettings {
            exclude_foreground_app: true,
            ..Default::default()
        };
        let foreground = Path::new(r"C:\Apps\Foreground\app.exe");

        assert!(should_ignore_foreground_process(
            settings.exclude_foreground_app,
            42,
            Path::new(r"C:\Apps\helper.exe"),
            Some(42),
            Some(foreground),
        ));
        assert!(should_ignore_foreground_process(
            settings.exclude_foreground_app,
            99,
            Path::new(r"c:\apps\foreground\APP.EXE"),
            Some(42),
            Some(foreground),
        ));
        assert!(!should_ignore_foreground_process(
            settings.exclude_foreground_app,
            99,
            Path::new(r"D:\Other\app.exe"),
            Some(42),
            Some(foreground),
        ));

        settings.exclude_foreground_app = false;
        assert!(!should_ignore_foreground_process(
            settings.exclude_foreground_app,
            42,
            foreground,
            Some(42),
            Some(foreground),
        ));
    }

    #[test]
    fn release_processes_drops_exited_process_without_log_entry() {
        let mut manager = BackgroundEfficiencyManager::default();
        manager.throttled.insert(
            0,
            ThrottledProcess {
                process_name: "exited.exe".to_owned(),
                executable_path: r"C:\Apps\exited.exe".to_owned(),
                creation_time: 0,
                previous_state: Some(power_throttling_disabled_state()),
                previous_priority: None,
                applied_ignore_timer_resolution: false,
            },
        );
        let mut log = ActionLog::new(8);

        let failures = manager.release_processes(&[0], &mut log, "test");

        assert_eq!(failures.count, 0);
        assert!(log.entries().is_empty());
        assert!(manager.throttled.is_empty());
    }
}
