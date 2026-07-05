use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::c_void,
};

use windows_sys::Win32::{
    Foundation::{ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER, ERROR_NOT_SUPPORTED},
    System::Threading::{
        GetCurrentProcessId, GetPriorityClass, GetProcessInformation, OpenProcess,
        ProcessPowerThrottling, SetPriorityClass, SetProcessInformation, IDLE_PRIORITY_CLASS,
        NORMAL_PRIORITY_CLASS, PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        PROCESS_POWER_THROTTLING_EXECUTION_SPEED, PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
        PROCESS_POWER_THROTTLING_STATE, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_INFORMATION,
        PROCESS_SET_LIMITED_INFORMATION,
    },
};

use crate::win_util::{last_error, WinHandle};

use crate::{
    action_log::{ActionLog, ActionLogAction, ActionLogFeature, ActionLogResult},
    audio_activity::active_audio_process_ids,
    config::{EcoQosAggressiveness, EcoQosSettings},
    foreground::{
        contains_process_name, is_process_exited_message, list_processes, process_failure_key,
        process_session_id, should_ignore_foreground_process,
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
pub struct EcoQosSnapshot {
    pub enabled: bool,
    pub unsupported: bool,
    pub scanned_processes: usize,
    pub throttled_processes: usize,
    pub timer_resolution_ignored_processes: usize,
    pub skipped_processes: usize,
    pub failed_processes: usize,
    pub auto_excluded_processes: Vec<String>,
    pub message: String,
    pub last_error: Option<String>,
}

#[derive(Default)]
pub struct EcoQosManager {
    throttled: BTreeMap<u32, ThrottledProcess>,
    failure_suppression: ExecutionFailureTracker,
}

struct ThrottledProcess {
    process_name: String,
    previous_state: Option<PROCESS_POWER_THROTTLING_STATE>,
    previous_priority: Option<u32>,
    applied_ignore_timer_resolution: bool,
}

impl EcoQosManager {
    pub fn throttled_process_ids(&self) -> BTreeSet<u32> {
        self.throttled.keys().copied().collect()
    }

    pub fn update(
        &mut self,
        settings: &EcoQosSettings,
        automation_enabled: bool,
        foreground_process_id: Option<u32>,
        action_log: &mut ActionLog,
    ) -> EcoQosSnapshot {
        if !automation_enabled {
            let failed = self.clear_all(action_log, "automation disabled");
            self.failure_suppression.clear();
            return EcoQosSnapshot {
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
            return EcoQosSnapshot {
                enabled: false,
                failed_processes: failed.count,
                message: "Background Efficiency disabled.".to_owned(),
                last_error: failed.last_error,
                ..Default::default()
            };
        }

        if settings.exclude_foreground_app && foreground_process_id.is_none() {
            let failed = self.clear_all(action_log, "foreground app is unknown");
            return EcoQosSnapshot {
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
            return EcoQosSnapshot {
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
                return EcoQosSnapshot {
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
                || should_ignore_foreground_process(
                    settings.exclude_foreground_app,
                    process.id,
                    &process.name,
                    foreground_process_id,
                    foreground_process_name.as_deref(),
                )
                || is_process_excluded(&process.name, settings)
            {
                continue;
            }

            if process_session_id(process.id) != Some(current_session_id) {
                continue;
            }

            target_processes.insert(process.id, process.name);
        }

        let active_target_names = target_processes
            .values()
            .map(|name| process_failure_key(name))
            .collect::<BTreeSet<_>>();
        self.failure_suppression.retain_keys(&active_target_names);

        let target_ids = target_processes.keys().copied().collect::<BTreeSet<_>>();
        let mut failures =
            self.release_non_targets(&target_ids, action_log, "process no longer matches EcoQoS");
        let mut unsupported = false;
        let mut auto_excluded_processes = BTreeSet::new();
        let active_audio_process_ids = active_audio_process_ids().ok();

        for (process_id, name) in target_processes {
            let suppression = self.check_process_suppression(process_id, &name, action_log);
            if suppression.suppressed {
                skipped_processes += 1;
                if suppression.newly_suppressed {
                    auto_excluded_processes.insert(process_failure_key(&name));
                }
                continue;
            }

            match apply_efficiency_mode_to_process(
                process_id,
                name.clone(),
                ignore_timer_resolution_allowed(process_id, active_audio_process_ids.as_ref()),
                &mut self.throttled,
                action_log,
            ) {
                Ok(enabled_new_process) => {
                    if enabled_new_process {
                        self.clear_process_failure(&name);
                    }
                }
                Err(EcoQosError::ProcessExited) => skipped_processes += 1,
                Err(EcoQosError::AccessDenied) => {
                    skipped_processes += 1;
                    self.record_process_failure(&name);
                    action_log.record(
                        ActionLogFeature::EcoQos,
                        Some(process_id),
                        name,
                        ActionLogAction::Skip,
                        ActionLogResult::Skipped,
                        "Skipped because the process could not be opened.",
                    );
                }
                Err(EcoQosError::Unsupported) => {
                    skipped_processes += 1;
                    unsupported = true;
                    self.record_process_failure(&name);
                    action_log.record(
                        ActionLogFeature::EcoQos,
                        Some(process_id),
                        name,
                        ActionLogAction::Skip,
                        ActionLogResult::Skipped,
                        "Skipped because Windows process power throttling is unsupported.",
                    );
                }
                Err(error) => {
                    let err = eco_qos_error_message(&error);
                    if is_process_exited_message(&err) {
                        skipped_processes += 1;
                        continue;
                    }
                    failures.record_message("Apply", process_id, &name, err, action_log);
                    self.record_process_failure(&name);
                }
            }
        }

        EcoQosSnapshot {
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
            failed_processes: failures.count,
            auto_excluded_processes: auto_excluded_processes.into_iter().collect(),
            message: "Background Efficiency active.".to_owned(),
            last_error: failures.last_error,
        }
    }

    fn release_non_targets(
        &mut self,
        target_ids: &BTreeSet<u32>,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> EcoQosFailures {
        let process_ids = self
            .throttled
            .keys()
            .copied()
            .filter(|process_id| !target_ids.contains(process_id))
            .collect::<Vec<_>>();

        self.release_processes(&process_ids, action_log, reason)
    }

    fn clear_all(&mut self, action_log: &mut ActionLog, reason: &str) -> EcoQosFailures {
        let process_ids = self.throttled.keys().copied().collect::<Vec<_>>();
        self.release_processes(&process_ids, action_log, reason)
    }

    fn release_processes(
        &mut self,
        process_ids: &[u32],
        action_log: &mut ActionLog,
        reason: &str,
    ) -> EcoQosFailures {
        let mut failures = EcoQosFailures::default();
        for process_id in process_ids {
            if let Some(process) = self.throttled.remove(process_id) {
                let process_name = process.process_name.clone();
                if let Err(err) = restore_efficiency_mode(*process_id, process) {
                    if !matches!(err, EcoQosError::ProcessExited) {
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
                        ActionLogFeature::EcoQos,
                        Some(*process_id),
                        process_name,
                        ActionLogAction::Restore,
                        ActionLogResult::Restored,
                        reason.to_owned(),
                    );
                }
            }
        }
        failures
    }

    fn check_process_suppression(
        &mut self,
        process_id: u32,
        process_name: &str,
        action_log: &mut ActionLog,
    ) -> ProcessSuppression {
        let suppression = self.failure_suppression.process_suppression(process_name);
        if suppression.newly_suppressed {
            action_log.record(
                ActionLogFeature::EcoQos,
                Some(process_id),
                process_name.to_owned(),
                ActionLogAction::Skip,
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
        self.check_process_suppression(process_id, process_name, action_log)
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

type ProcessSuppression = ExecutionSuppression;

impl Drop for EcoQosManager {
    fn drop(&mut self) {
        let mut action_log = ActionLog::new(1);
        self.clear_all(&mut action_log, "EcoQoS manager dropped");
    }
}

impl Default for EcoQosSnapshot {
    fn default() -> Self {
        Self {
            enabled: false,
            unsupported: false,
            scanned_processes: 0,
            throttled_processes: 0,
            timer_resolution_ignored_processes: 0,
            skipped_processes: 0,
            failed_processes: 0,
            auto_excluded_processes: Vec::new(),
            message: "Background Efficiency disabled.".to_owned(),
            last_error: None,
        }
    }
}

pub fn is_process_excluded(process_name: &str, settings: &EcoQosSettings) -> bool {
    is_builtin_excluded_for(process_name, settings.aggressiveness)
        || settings.efficiency_exclusion_enabled_for(process_name)
}

pub fn is_builtin_excluded(process_name: &str) -> bool {
    is_builtin_excluded_for(process_name, EcoQosAggressiveness::Safe)
}

fn is_builtin_excluded_for(process_name: &str, aggressiveness: EcoQosAggressiveness) -> bool {
    contains_process_name(built_in_exclusions_for(aggressiveness), process_name)
}

fn built_in_exclusions_for(aggressiveness: EcoQosAggressiveness) -> &'static [&'static str] {
    match aggressiveness {
        EcoQosAggressiveness::Safe => BUILT_IN_EXCLUSIONS,
        EcoQosAggressiveness::Balanced => BALANCED_BUILT_IN_EXCLUSIONS,
        EcoQosAggressiveness::Aggressive => AGGRESSIVE_BUILT_IN_EXCLUSIONS,
    }
}

enum EcoQosError {
    AccessDenied,
    ProcessExited,
    Unsupported,
    Failed(String),
}

#[derive(Default)]
struct EcoQosFailures {
    count: usize,
    last_error: Option<String>,
}

impl EcoQosFailures {
    fn record_error(
        &mut self,
        action: &str,
        process_id: u32,
        process_name: &str,
        error: EcoQosError,
        action_log: &mut ActionLog,
    ) {
        let message = match error {
            EcoQosError::AccessDenied => "Access denied.".to_owned(),
            EcoQosError::ProcessExited => return,
            EcoQosError::Unsupported => "Operation unsupported.".to_owned(),
            EcoQosError::Failed(message) => message,
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
        if is_process_exited_message(&message) {
            return;
        }
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
            ActionLogFeature::EcoQos,
            Some(process_id),
            process_name.to_owned(),
            ActionLogAction::Fail,
            ActionLogResult::Failed,
            message,
        );
    }
}

fn apply_efficiency_mode_to_process(
    process_id: u32,
    process_name: String,
    ignore_timer_resolution: bool,
    throttled: &mut BTreeMap<u32, ThrottledProcess>,
    action_log: &mut ActionLog,
) -> Result<bool, EcoQosError> {
    if let Some(process) = throttled.get_mut(&process_id) {
        update_efficiency_mode(process_id, process, ignore_timer_resolution)?;
        return Ok(false);
    }

    let process =
        enable_efficiency_mode(process_id, process_name.clone(), ignore_timer_resolution)?;
    throttled.insert(process_id, process);
    action_log.record(
        ActionLogFeature::EcoQos,
        Some(process_id),
        process_name,
        ActionLogAction::Apply,
        ActionLogResult::Applied,
        "Applied Background Efficiency: enabled EcoQoS and lowered priority.".to_owned(),
    );
    Ok(true)
}

fn update_efficiency_mode(
    process_id: u32,
    process_state: &mut ThrottledProcess,
    ignore_timer_resolution: bool,
) -> Result<(), EcoQosError> {
    if process_state.applied_ignore_timer_resolution == ignore_timer_resolution {
        return Ok(());
    }

    let process = ProcessHandle::open(process_id)?;
    process.set_power_throttling_state(power_throttling_enabled_state(
        process_state.previous_state,
        ignore_timer_resolution,
    ))?;
    process_state.applied_ignore_timer_resolution = ignore_timer_resolution;
    Ok(())
}

fn eco_qos_error_message(error: &EcoQosError) -> String {
    match error {
        EcoQosError::AccessDenied => "Access denied.".to_owned(),
        EcoQosError::ProcessExited => "Process exited.".to_owned(),
        EcoQosError::Unsupported => "Operation unsupported.".to_owned(),
        EcoQosError::Failed(message) => message.clone(),
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

fn enable_efficiency_mode(
    process_id: u32,
    process_name: String,
    ignore_timer_resolution: bool,
) -> Result<ThrottledProcess, EcoQosError> {
    let process = ProcessHandle::open(process_id)?;
    let previous_state = process.power_throttling_state().ok();
    let previous_priority = process.priority_class().ok();

    let next_state = power_throttling_enabled_state(previous_state, ignore_timer_resolution);
    process.set_power_throttling_state(next_state)?;
    if let Err(err) = process.set_priority_class(IDLE_PRIORITY_CLASS) {
        let _ = process.set_power_throttling_state(
            previous_state.unwrap_or_else(power_throttling_disabled_state),
        );
        return Err(err);
    }

    Ok(ThrottledProcess {
        process_name,
        previous_state,
        previous_priority,
        applied_ignore_timer_resolution: ignore_timer_resolution,
    })
}

fn restore_efficiency_mode(
    process_id: u32,
    process_state: ThrottledProcess,
) -> Result<(), EcoQosError> {
    let process = ProcessHandle::open(process_id)?;
    let mut last_error = None;

    if let Err(err) = process.set_power_throttling_state(
        process_state
            .previous_state
            .unwrap_or_else(power_throttling_disabled_state),
    ) {
        last_error = Some(err);
    }

    if let Err(err) = process.set_priority_class(
        process_state
            .previous_priority
            .unwrap_or(NORMAL_PRIORITY_CLASS),
    ) {
        last_error = Some(err);
    }

    match last_error {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

fn power_throttling_disabled_state() -> PROCESS_POWER_THROTTLING_STATE {
    PROCESS_POWER_THROTTLING_STATE {
        Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
        StateMask: 0,
    }
}

fn power_throttling_enabled_state(
    previous: Option<PROCESS_POWER_THROTTLING_STATE>,
    ignore_timer_resolution: bool,
) -> PROCESS_POWER_THROTTLING_STATE {
    let previous_ignore_timer_resolution = previous.is_some_and(|state| {
        (state.StateMask & PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION) != 0
    });
    let mut state = previous.unwrap_or_else(power_throttling_disabled_state);
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
    fn open(process_id: u32) -> Result<Self, EcoQosError> {
        let access_masks = [
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_INFORMATION,
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_LIMITED_INFORMATION,
        ];

        let mut last_open_error = 0;
        for access in access_masks {
            let handle = unsafe { OpenProcess(access, 0, process_id) };
            if !handle.is_null() {
                return Ok(Self(WinHandle::new(handle)));
            }
            last_open_error = last_error();
        }

        Err(open_process_error(process_id, last_open_error))
    }

    fn power_throttling_state(&self) -> Result<PROCESS_POWER_THROTTLING_STATE, EcoQosError> {
        let mut state = PROCESS_POWER_THROTTLING_STATE::default();
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

    fn priority_class(&self) -> Result<u32, EcoQosError> {
        let priority = unsafe { GetPriorityClass(self.0.raw()) };
        if priority == 0 {
            Err(EcoQosError::Failed(format!(
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
    ) -> Result<(), EcoQosError> {
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

    fn set_priority_class(&self, priority_class: u32) -> Result<(), EcoQosError> {
        let ok = unsafe { SetPriorityClass(self.0.raw(), priority_class) };
        if ok == 0 {
            Err(EcoQosError::Failed(format!(
                "SetPriorityClass failed with error {}.",
                last_error()
            )))
        } else {
            Ok(())
        }
    }
}

fn process_power_throttling_error(operation: &str, error: u32) -> EcoQosError {
    match error {
        ERROR_INVALID_PARAMETER | ERROR_NOT_SUPPORTED => EcoQosError::Unsupported,
        _ => EcoQosError::Failed(format!("{operation} failed with error {error}.")),
    }
}

fn open_process_error(process_id: u32, error: u32) -> EcoQosError {
    match error {
        ERROR_ACCESS_DENIED => EcoQosError::AccessDenied,
        ERROR_INVALID_PARAMETER => EcoQosError::ProcessExited,
        _ => EcoQosError::Failed(format!(
            "OpenProcess({process_id}) failed with error {error}."
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exclusions_include_builtin_and_user_entries() {
        let settings = EcoQosSettings {
            enabled: true,
            exclude_foreground_app: true,
            aggressiveness: EcoQosAggressiveness::Safe,
            efficiency_whitelist: vec![crate::config::EcoQosExclusionRule {
                enabled: true,
                process_name: "mouse.exe".to_owned(),
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
        let mut settings = EcoQosSettings {
            aggressiveness: EcoQosAggressiveness::Safe,
            ..Default::default()
        };

        assert!(is_process_excluded("SearchHost.exe", &settings));
        assert!(is_process_excluded("dwm.exe", &settings));
        assert!(is_process_excluded("winlogon.exe", &settings));

        settings.aggressiveness = EcoQosAggressiveness::Balanced;
        assert!(!is_process_excluded("SearchHost.exe", &settings));
        assert!(is_process_excluded("dwm.exe", &settings));
        assert!(is_process_excluded("winlogon.exe", &settings));

        settings.aggressiveness = EcoQosAggressiveness::Aggressive;
        assert!(!is_process_excluded("SearchHost.exe", &settings));
        assert!(!is_process_excluded("dwm.exe", &settings));
        assert!(is_process_excluded("winlogon.exe", &settings));
    }

    #[test]
    fn disabled_user_exclusions_do_not_exclude_processes() {
        let settings = EcoQosSettings {
            enabled: true,
            exclude_foreground_app: true,
            aggressiveness: EcoQosAggressiveness::Safe,
            efficiency_whitelist: vec![crate::config::EcoQosExclusionRule {
                enabled: false,
                process_name: "mouse.exe".to_owned(),
            }],
        };

        assert!(settings.contains_efficiency_exclusion("MOUSE.EXE"));
        assert!(!is_process_excluded("mouse.exe", &settings));
    }

    #[test]
    fn power_throttling_unsupported_codes_mark_feature_unsupported() {
        assert!(matches!(
            process_power_throttling_error("SetProcessInformation", ERROR_NOT_SUPPORTED),
            EcoQosError::Unsupported
        ));
        assert!(matches!(
            process_power_throttling_error("SetProcessInformation", ERROR_INVALID_PARAMETER),
            EcoQosError::Unsupported
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
            EcoQosError::ProcessExited
        ));
    }

    #[test]
    fn repeated_failures_suppress_future_efficiency_attempts_once() {
        let mut manager = EcoQosManager::default();
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
    fn first_suppression_reports_auto_exclusion_once() {
        let mut manager = EcoQosManager::default();
        let mut log = ActionLog::new(8);

        manager.record_process_failure("app.exe");
        manager.record_process_failure("app.exe");
        manager.record_process_failure("app.exe");

        let first = manager.check_process_suppression(42, "app.exe", &mut log);
        let second = manager.check_process_suppression(42, "app.exe", &mut log);

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
        let allowed = power_throttling_enabled_state(None, true);
        assert_ne!(
            allowed.StateMask & PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
            0
        );
        assert_ne!(
            allowed.StateMask & PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
            0
        );

        let blocked = power_throttling_enabled_state(None, false);
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
    fn foreground_ignore_matches_pid_or_process_name() {
        let mut settings = EcoQosSettings {
            exclude_foreground_app: true,
            ..Default::default()
        };

        assert!(should_ignore_foreground_process(
            settings.exclude_foreground_app,
            42,
            "helper.exe",
            Some(42),
            Some("app.exe"),
        ));
        assert!(should_ignore_foreground_process(
            settings.exclude_foreground_app,
            99,
            "APP.EXE",
            Some(42),
            Some("app.exe"),
        ));
        assert!(!should_ignore_foreground_process(
            settings.exclude_foreground_app,
            99,
            "other.exe",
            Some(42),
            Some("app.exe"),
        ));

        settings.exclude_foreground_app = false;
        assert!(!should_ignore_foreground_process(
            settings.exclude_foreground_app,
            42,
            "app.exe",
            Some(42),
            Some("app.exe"),
        ));
    }

    #[test]
    fn release_processes_drops_exited_process_without_log_entry() {
        let mut manager = EcoQosManager::default();
        manager.throttled.insert(
            0,
            ThrottledProcess {
                process_name: "exited.exe".to_owned(),
                previous_state: None,
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
