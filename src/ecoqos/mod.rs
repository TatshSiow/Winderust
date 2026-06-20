use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::c_void,
    mem::size_of,
    ptr::{null_mut, read_unaligned},
    slice,
};

use windows_sys::Win32::{
    Foundation::{
        CloseHandle, GetLastError, ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER,
        ERROR_NOT_SUPPORTED, HANDLE,
    },
    System::{
        SystemInformation::{GetSystemCpuSetInformation, SYSTEM_CPU_SET_INFORMATION},
        Threading::{
            GetCurrentProcessId, GetPriorityClass, GetProcessAffinityMask,
            GetProcessDefaultCpuSets, GetProcessInformation, OpenProcess, ProcessPowerThrottling,
            SetPriorityClass, SetProcessAffinityMask, SetProcessDefaultCpuSets,
            SetProcessInformation, IDLE_PRIORITY_CLASS, NORMAL_PRIORITY_CLASS,
            PROCESS_POWER_THROTTLING_CURRENT_VERSION, PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
            PROCESS_POWER_THROTTLING_STATE, PROCESS_QUERY_LIMITED_INFORMATION,
            PROCESS_SET_INFORMATION, PROCESS_SET_LIMITED_INFORMATION,
        },
    },
};

use crate::{
    action_log::{ActionLog, ActionLogAction, ActionLogFeature, ActionLogResult},
    affinity::{self, LogicalProcessorKind},
    config::{
        EcoQosAggressiveness, EcoQosCpuRestrictionControlStyle, EcoQosCpuRestrictionMode,
        EcoQosCpuRestrictionStrategy, EcoQosSettings,
    },
    foreground::{
        is_process_exited_message, list_processes, process_failure_key, process_session_id,
        should_ignore_foreground_process,
    },
    rules::{
        execution_failure_suppression_threshold, Action, ActionExecution, ActionExecutor,
        AffinityPolicy, AppMatcher, AppResourceActionBackend, ExecutionFailureTracker,
        ExecutionSuppression,
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
    previous_cpu_set_ids: Option<Vec<u32>>,
    applied_cpu_set_ids: Option<Vec<u32>>,
    previous_affinity: Option<usize>,
    applied_affinity: Option<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EcoQosCpuRestriction {
    mode: EcoQosCpuRestrictionMode,
    strategy: EcoQosCpuRestrictionStrategy,
    control_style: EcoQosCpuRestrictionControlStyle,
    percent: u8,
    max_logical_processors: u8,
    core_mask: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum EcoQosRestrictionTarget {
    None,
    SoftCpuSets(Vec<u32>),
    HardAffinity(usize),
}

#[repr(C)]
#[derive(Clone, Copy)]
struct CpuSetInformationHeader {
    size: u32,
    cpu_set_type: u32,
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
            let failed = self.clear_all(action_log, "Efficiency Mode disabled");
            self.failure_suppression.clear();
            return EcoQosSnapshot {
                enabled: false,
                failed_processes: failed.count,
                message: "Efficiency Mode disabled.".to_owned(),
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

        for (process_id, name) in target_processes {
            let suppression = self.check_process_suppression(process_id, &name, action_log);
            if suppression.suppressed {
                skipped_processes += 1;
                if suppression.newly_suppressed {
                    auto_excluded_processes.insert(process_failure_key(&name));
                }
                continue;
            }

            let action = Action::SetAppEfficiencyMode {
                app: AppMatcher::ProcessName(name.clone()),
                enabled: true,
            };
            let mut backend = EcoQosActionBackend {
                process_id,
                process_name: name.clone(),
                throttled: &mut self.throttled,
                prefer_efficiency_cores: settings.prefer_efficiency_cores,
                limit_cpu_sets_on_non_hybrid: settings.limit_cpu_sets_on_non_hybrid,
                restriction: cpu_restriction(settings),
                action_log,
                last_error: None,
                enabled_new_process: false,
            };
            let execution = ActionExecutor.apply_app_resource_action(&action, &mut backend);
            let last_error = backend.last_error.take();
            let enabled_new_process = backend.enabled_new_process;
            drop(backend);

            match execution {
                ActionExecution::Applied | ActionExecution::AlreadyApplied => {
                    if enabled_new_process {
                        self.clear_process_failure(&name);
                    }
                }
                ActionExecution::Failed(_)
                    if matches!(last_error.as_ref(), Some(EcoQosError::ProcessExited)) =>
                {
                    skipped_processes += 1;
                }
                ActionExecution::Failed(_)
                    if matches!(last_error.as_ref(), Some(EcoQosError::AccessDenied)) =>
                {
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
                ActionExecution::Failed(_)
                    if matches!(last_error.as_ref(), Some(EcoQosError::Unsupported)) =>
                {
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
                ActionExecution::Failed(err) => {
                    if is_process_exited_message(&err) {
                        skipped_processes += 1;
                        continue;
                    }
                    failures.record_message("Apply", process_id, &name, err, action_log);
                    self.record_process_failure(&name);
                }
                ActionExecution::Unsupported => {
                    failures.record_message(
                        "Apply",
                        process_id,
                        &name,
                        "EcoQoS action was not supported by the generic executor.".to_owned(),
                        action_log,
                    );
                    self.record_process_failure(&name);
                }
            }
        }

        EcoQosSnapshot {
            enabled: true,
            unsupported,
            scanned_processes,
            throttled_processes: self.throttled.len(),
            skipped_processes,
            failed_processes: failures.count,
            auto_excluded_processes: auto_excluded_processes.into_iter().collect(),
            message: "Efficiency Mode active.".to_owned(),
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
                    "Stopped retrying Efficiency Mode after {} failed attempts.",
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
            skipped_processes: 0,
            failed_processes: 0,
            auto_excluded_processes: Vec::new(),
            message: "Efficiency Mode disabled.".to_owned(),
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
    let process_name = process_name.trim();
    built_in_exclusions_for(aggressiveness)
        .iter()
        .any(|excluded| excluded.eq_ignore_ascii_case(process_name))
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

struct EcoQosActionBackend<'a> {
    process_id: u32,
    process_name: String,
    throttled: &'a mut BTreeMap<u32, ThrottledProcess>,
    prefer_efficiency_cores: bool,
    limit_cpu_sets_on_non_hybrid: bool,
    restriction: EcoQosCpuRestriction,
    action_log: &'a mut ActionLog,
    last_error: Option<EcoQosError>,
    enabled_new_process: bool,
}

impl EcoQosActionBackend<'_> {
    fn unsupported_action() -> Result<(), String> {
        Err("EcoQoS backend only supports per-process efficiency-mode actions.".to_owned())
    }
}

impl AppResourceActionBackend for EcoQosActionBackend<'_> {
    fn set_app_efficiency_mode(&mut self, app: &AppMatcher, enabled: bool) -> Result<(), String> {
        if !enabled {
            return Err(
                "EcoQoS backend enables efficiency mode; restore is handled by release paths."
                    .to_owned(),
            );
        }

        if !app_matches_process_name(app, &self.process_name) {
            return Err(format!(
                "EcoQoS action target does not match selected process {}.",
                self.process_name
            ));
        }

        if let Some(process_state) = self.throttled.get_mut(&self.process_id) {
            if let Err(error) = sync_efficiency_cpu_sets(
                self.process_id,
                &self.process_name,
                process_state,
                self.prefer_efficiency_cores,
                self.limit_cpu_sets_on_non_hybrid,
                self.restriction,
                self.action_log,
            ) {
                let message = eco_qos_error_message(&error);
                self.last_error = Some(error);
                return Err(message);
            }
            return Ok(());
        }

        match enable_efficiency_mode(
            self.process_id,
            self.process_name.clone(),
            self.prefer_efficiency_cores,
            self.limit_cpu_sets_on_non_hybrid,
            self.restriction,
        ) {
            Ok(process) => {
                self.throttled.insert(self.process_id, process);
                self.enabled_new_process = true;
                let cpu_sets_note = if self.prefer_efficiency_cores {
                    " and preferred efficiency CPU sets"
                } else {
                    ""
                };
                self.action_log.record(
                    ActionLogFeature::EcoQos,
                    Some(self.process_id),
                    self.process_name.clone(),
                    ActionLogAction::Apply,
                    ActionLogResult::Applied,
                    format!("Enabled Windows Efficiency Mode, lowered priority{cpu_sets_note}."),
                );
                Ok(())
            }
            Err(error) => {
                let message = eco_qos_error_message(&error);
                self.last_error = Some(error);
                Err(message)
            }
        }
    }

    fn set_app_affinity(
        &mut self,
        _app: &AppMatcher,
        _affinity: &AffinityPolicy,
    ) -> Result<(), String> {
        Self::unsupported_action()
    }

    fn set_app_cpu_limit(
        &mut self,
        _app: &AppMatcher,
        _logical_processor_percent: u8,
    ) -> Result<(), String> {
        Self::unsupported_action()
    }

    fn suspend_app(&mut self, _app: &AppMatcher) -> Result<(), String> {
        Self::unsupported_action()
    }

    fn resume_app(&mut self, _app: &AppMatcher) -> Result<(), String> {
        Self::unsupported_action()
    }

    fn configure_background_efficiency_policy(
        &mut self,
        _exclusions: &[AppMatcher],
        _prefer_efficiency_cores: bool,
        _logical_processor_percent: Option<u8>,
    ) -> Result<(), String> {
        Self::unsupported_action()
    }
}

fn app_matches_process_name(app: &AppMatcher, process_name: &str) -> bool {
    match app {
        AppMatcher::ProcessName(name) | AppMatcher::Path(name) | AppMatcher::Pattern(name) => {
            name.trim().eq_ignore_ascii_case(process_name.trim())
        }
    }
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
    prefer_efficiency_cores: bool,
    limit_cpu_sets_on_non_hybrid: bool,
    restriction: EcoQosCpuRestriction,
) -> Result<ThrottledProcess, EcoQosError> {
    let process = ProcessHandle::open(process_id)?;
    let previous_state = process.power_throttling_state().ok();
    let previous_priority = process.priority_class().ok();
    let target = efficiency_restriction_target(
        prefer_efficiency_cores,
        limit_cpu_sets_on_non_hybrid,
        restriction,
    )?;
    let previous_cpu_set_ids = match &target {
        EcoQosRestrictionTarget::SoftCpuSets(ids) if !ids.is_empty() => {
            Some(process.default_cpu_set_ids()?)
        }
        _ => None,
    };
    let previous_affinity = match target {
        EcoQosRestrictionTarget::HardAffinity(_) => {
            let (current_affinity, _) = process.affinity_mask()?;
            Some(current_affinity)
        }
        EcoQosRestrictionTarget::None | EcoQosRestrictionTarget::SoftCpuSets(_) => None,
    };

    let mut next_state = previous_state.unwrap_or_default();
    next_state.Version = PROCESS_POWER_THROTTLING_CURRENT_VERSION;
    next_state.ControlMask |= PROCESS_POWER_THROTTLING_EXECUTION_SPEED;
    next_state.StateMask |= PROCESS_POWER_THROTTLING_EXECUTION_SPEED;
    process.set_power_throttling_state(next_state)?;
    if let Err(err) = process.set_priority_class(IDLE_PRIORITY_CLASS) {
        let _ = process.set_power_throttling_state(
            previous_state.unwrap_or_else(power_throttling_disabled_state),
        );
        return Err(err);
    }
    match &target {
        EcoQosRestrictionTarget::SoftCpuSets(target_cpu_set_ids)
            if !target_cpu_set_ids.is_empty() =>
        {
            if let Err(err) = process.set_default_cpu_set_ids(target_cpu_set_ids) {
                let _ =
                    process.set_priority_class(previous_priority.unwrap_or(NORMAL_PRIORITY_CLASS));
                let _ = process.set_power_throttling_state(
                    previous_state.unwrap_or_else(power_throttling_disabled_state),
                );
                return Err(err);
            }
        }
        EcoQosRestrictionTarget::HardAffinity(target_affinity) => {
            if let Err(err) = process.set_affinity_mask(*target_affinity) {
                let _ =
                    process.set_priority_class(previous_priority.unwrap_or(NORMAL_PRIORITY_CLASS));
                let _ = process.set_power_throttling_state(
                    previous_state.unwrap_or_else(power_throttling_disabled_state),
                );
                return Err(err);
            }
        }
        EcoQosRestrictionTarget::None | EcoQosRestrictionTarget::SoftCpuSets(_) => {}
    }

    let (applied_cpu_set_ids, applied_affinity) = match target {
        EcoQosRestrictionTarget::SoftCpuSets(ids) if !ids.is_empty() => (Some(ids), None),
        EcoQosRestrictionTarget::HardAffinity(mask) => (None, Some(mask)),
        EcoQosRestrictionTarget::None | EcoQosRestrictionTarget::SoftCpuSets(_) => (None, None),
    };

    Ok(ThrottledProcess {
        process_name,
        previous_state,
        previous_priority,
        previous_cpu_set_ids,
        applied_cpu_set_ids,
        previous_affinity,
        applied_affinity,
    })
}

fn restore_cpu_restriction(
    process: &ProcessHandle,
    process_state: &mut ThrottledProcess,
) -> Result<(), EcoQosError> {
    let mut last_error = None;
    if let Some(previous_cpu_set_ids) = process_state.previous_cpu_set_ids.take() {
        if let Err(err) = process.set_default_cpu_set_ids(&previous_cpu_set_ids) {
            last_error = Some(err);
        }
        process_state.applied_cpu_set_ids = None;
    }
    if let Some(previous_affinity) = process_state.previous_affinity.take() {
        if let Err(err) = process.set_affinity_mask(previous_affinity) {
            last_error = Some(err);
        }
        process_state.applied_affinity = None;
    }

    match last_error {
        Some(err) => Err(err),
        None => Ok(()),
    }
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

    if let Some(previous_cpu_set_ids) = process_state.previous_cpu_set_ids {
        if let Err(err) = process.set_default_cpu_set_ids(&previous_cpu_set_ids) {
            last_error = Some(err);
        }
    }

    if let Some(previous_affinity) = process_state.previous_affinity {
        if let Err(err) = process.set_affinity_mask(previous_affinity) {
            last_error = Some(err);
        }
    }

    match last_error {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

fn sync_efficiency_cpu_sets(
    process_id: u32,
    process_name: &str,
    process_state: &mut ThrottledProcess,
    prefer_efficiency_cores: bool,
    limit_cpu_sets_on_non_hybrid: bool,
    restriction: EcoQosCpuRestriction,
    action_log: &mut ActionLog,
) -> Result<(), EcoQosError> {
    let target = efficiency_restriction_target(
        prefer_efficiency_cores,
        limit_cpu_sets_on_non_hybrid,
        restriction,
    )?;

    match target {
        EcoQosRestrictionTarget::None => {
            if process_state.previous_cpu_set_ids.is_none()
                && process_state.previous_affinity.is_none()
            {
                return Ok(());
            }
            let process = ProcessHandle::open(process_id)?;
            restore_cpu_restriction(&process, process_state)?;
            action_log.record(
                ActionLogFeature::EcoQos,
                Some(process_id),
                process_name.to_owned(),
                ActionLogAction::Restore,
                ActionLogResult::Applied,
                "Restored previous CPU restriction.",
            );
        }
        EcoQosRestrictionTarget::SoftCpuSets(ref ids) if ids.is_empty() => {
            if process_state.previous_cpu_set_ids.is_none()
                && process_state.previous_affinity.is_none()
            {
                return Ok(());
            }
            let process = ProcessHandle::open(process_id)?;
            restore_cpu_restriction(&process, process_state)?;
            action_log.record(
                ActionLogFeature::EcoQos,
                Some(process_id),
                process_name.to_owned(),
                ActionLogAction::Restore,
                ActionLogResult::Applied,
                "Restored previous CPU restriction.",
            );
        }
        EcoQosRestrictionTarget::SoftCpuSets(target_cpu_set_ids) => {
            if process_state
                .applied_cpu_set_ids
                .as_ref()
                .is_some_and(|applied| *applied == target_cpu_set_ids)
                && process_state.applied_affinity.is_none()
            {
                return Ok(());
            }

            let process = ProcessHandle::open(process_id)?;
            if process_state.previous_affinity.is_some() {
                restore_cpu_restriction(&process, process_state)?;
            }
            if process_state.previous_cpu_set_ids.is_none() {
                process_state.previous_cpu_set_ids = Some(process.default_cpu_set_ids()?);
            }
            process.set_default_cpu_set_ids(&target_cpu_set_ids)?;
            process_state.applied_cpu_set_ids = Some(target_cpu_set_ids.clone());
            action_log.record(
                ActionLogFeature::EcoQos,
                Some(process_id),
                process_name.to_owned(),
                ActionLogAction::Apply,
                ActionLogResult::Applied,
                format!("Applied efficiency CPU Sets: {}.", target_cpu_set_ids.len()),
            );
        }
        EcoQosRestrictionTarget::HardAffinity(target_affinity) => {
            if process_state.applied_affinity == Some(target_affinity)
                && process_state.applied_cpu_set_ids.is_none()
            {
                return Ok(());
            }
            let process = ProcessHandle::open(process_id)?;
            if process_state.previous_cpu_set_ids.is_some() {
                restore_cpu_restriction(&process, process_state)?;
            }
            if process_state.previous_affinity.is_none() {
                let (current_affinity, _) = process.affinity_mask()?;
                process_state.previous_affinity = Some(current_affinity);
            }
            process.set_affinity_mask(target_affinity)?;
            process_state.applied_affinity = Some(target_affinity);
            action_log.record(
                ActionLogFeature::EcoQos,
                Some(process_id),
                process_name.to_owned(),
                ActionLogAction::Apply,
                ActionLogResult::Applied,
                format!("Applied efficiency affinity mask {target_affinity:#x}."),
            );
        }
    }
    Ok(())
}

fn power_throttling_disabled_state() -> PROCESS_POWER_THROTTLING_STATE {
    PROCESS_POWER_THROTTLING_STATE {
        Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
        StateMask: 0,
    }
}

fn cpu_restriction(settings: &EcoQosSettings) -> EcoQosCpuRestriction {
    let legacy_strategy = EcoQosCpuRestrictionStrategy::from_legacy_flags(
        settings.prefer_efficiency_cores,
        settings.limit_cpu_sets_on_non_hybrid,
    );
    let strategy = if settings.cpu_restriction_strategy == EcoQosCpuRestrictionStrategy::Auto
        && legacy_strategy != EcoQosCpuRestrictionStrategy::Auto
    {
        legacy_strategy
    } else {
        settings.cpu_restriction_strategy
    };

    EcoQosCpuRestriction {
        mode: settings.cpu_restriction_mode,
        strategy,
        control_style: settings.cpu_restriction_control_style,
        percent: settings.cpu_restriction_percent.clamp(1, 100),
        max_logical_processors: settings.cpu_restriction_max_logical_processors,
        core_mask: settings.cpu_restriction_core_mask,
    }
}

fn efficiency_restriction_target(
    prefer_efficiency_cores: bool,
    limit_cpu_sets_on_non_hybrid: bool,
    restriction: EcoQosCpuRestriction,
) -> Result<EcoQosRestrictionTarget, EcoQosError> {
    if restriction.strategy == EcoQosCpuRestrictionStrategy::Off {
        return Ok(EcoQosRestrictionTarget::None);
    }

    match restriction.mode {
        EcoQosCpuRestrictionMode::SoftCpuSets => Ok(EcoQosRestrictionTarget::SoftCpuSets(
            efficiency_cpu_set_ids(
                prefer_efficiency_cores,
                limit_cpu_sets_on_non_hybrid,
                restriction,
            )?,
        )),
        EcoQosCpuRestrictionMode::HardAffinity => Ok(efficiency_affinity_mask(restriction)
            .map(EcoQosRestrictionTarget::HardAffinity)
            .unwrap_or(EcoQosRestrictionTarget::None)),
    }
}

fn efficiency_affinity_mask(restriction: EcoQosCpuRestriction) -> Option<usize> {
    let processors = affinity::logical_processors();
    if restriction.control_style == EcoQosCpuRestrictionControlStyle::CoreToggle {
        return logical_indices_to_mask(&selected_logical_indices_from_mask(
            restriction.core_mask,
            &processors,
        ));
    }

    let has_efficiency_cores = processors
        .iter()
        .any(|processor| processor.kind == LogicalProcessorKind::Efficiency);
    let selected = match restriction.strategy {
        EcoQosCpuRestrictionStrategy::Off => Vec::new(),
        EcoQosCpuRestrictionStrategy::Auto if has_efficiency_cores => processors
            .iter()
            .filter(|processor| processor.kind == LogicalProcessorKind::Efficiency)
            .map(|processor| processor.index)
            .collect::<Vec<_>>(),
        EcoQosCpuRestrictionStrategy::PreferEfficiencyCores => processors
            .iter()
            .filter(|processor| processor.kind == LogicalProcessorKind::Efficiency)
            .map(|processor| processor.index)
            .collect::<Vec<_>>(),
        EcoQosCpuRestrictionStrategy::Auto | EcoQosCpuRestrictionStrategy::LimitLogicalCpus => {
            processors
                .iter()
                .map(|processor| processor.index)
                .collect::<Vec<_>>()
        }
    };

    logical_indices_to_limited_mask(
        &selected,
        restriction.percent,
        restriction.max_logical_processors,
    )
}

fn selected_logical_indices_from_mask(
    core_mask: u64,
    processors: &[affinity::LogicalProcessorInfo],
) -> Vec<usize> {
    processors
        .iter()
        .filter_map(|processor| {
            (processor.index < u64::BITS as usize && (core_mask & (1_u64 << processor.index)) != 0)
                .then_some(processor.index)
        })
        .collect()
}

fn logical_indices_to_mask(indices: &[usize]) -> Option<usize> {
    let mut mask = 0_usize;
    for index in indices {
        if *index < usize::BITS as usize {
            mask |= 1_usize << index;
        }
    }
    (mask != 0).then_some(mask)
}

fn logical_indices_to_limited_mask(
    indices: &[usize],
    percent: u8,
    max_logical_processors: u8,
) -> Option<usize> {
    if indices.is_empty() {
        return None;
    }
    let percent_count = (indices.len() * usize::from(percent.clamp(1, 100))).div_ceil(100);
    let max_count = usize::from(max_logical_processors);
    let limit = if max_count == 0 {
        percent_count
    } else {
        percent_count.min(max_count)
    }
    .clamp(1, indices.len());

    let mut mask = 0_usize;
    for index in indices.iter().take(limit) {
        if *index < usize::BITS as usize {
            mask |= 1_usize << index;
        }
    }
    (mask != 0).then_some(mask)
}

fn efficiency_cpu_set_ids(
    prefer_efficiency_cores: bool,
    limit_cpu_sets_on_non_hybrid: bool,
    restriction: EcoQosCpuRestriction,
) -> Result<Vec<u32>, EcoQosError> {
    let mut returned_length = 0;
    unsafe {
        GetSystemCpuSetInformation(null_mut(), 0, &mut returned_length, null_mut(), 0);
    }

    if returned_length == 0 {
        return Ok(Vec::new());
    }

    let word_count = (returned_length as usize).div_ceil(size_of::<usize>());
    let mut buffer = vec![0_usize; word_count];
    let ok = unsafe {
        GetSystemCpuSetInformation(
            buffer.as_mut_ptr() as *mut SYSTEM_CPU_SET_INFORMATION,
            returned_length,
            &mut returned_length,
            null_mut(),
            0,
        )
    };
    if ok == 0 {
        return Err(EcoQosError::Failed(format!(
            "GetSystemCpuSetInformation failed with error {}.",
            last_error()
        )));
    }

    Ok(efficiency_cpu_set_ids_from_bytes(
        unsafe { slice::from_raw_parts(buffer.as_ptr() as *const u8, returned_length as usize) },
        prefer_efficiency_cores,
        limit_cpu_sets_on_non_hybrid,
        restriction,
    ))
}

fn efficiency_cpu_set_ids_from_bytes(
    buffer: &[u8],
    prefer_efficiency_cores: bool,
    limit_cpu_sets_on_non_hybrid: bool,
    restriction: EcoQosCpuRestriction,
) -> Vec<u32> {
    let mut records = Vec::new();
    let mut offset = 0;
    let header_size = size_of::<CpuSetInformationHeader>();

    while offset + header_size <= buffer.len() {
        let header = unsafe {
            read_unaligned(buffer.as_ptr().add(offset) as *const CpuSetInformationHeader)
        };
        let record_size = header.size as usize;
        if record_size < header_size || offset + record_size > buffer.len() {
            break;
        }

        if header.cpu_set_type == 0 && record_size >= size_of::<SYSTEM_CPU_SET_INFORMATION>() {
            let info = unsafe {
                read_unaligned(buffer.as_ptr().add(offset) as *const SYSTEM_CPU_SET_INFORMATION)
            };
            let cpu_set = unsafe { info.Anonymous.CpuSet };
            if cpu_set.Group == 0 {
                records.push((
                    cpu_set.Id,
                    cpu_set.EfficiencyClass,
                    cpu_set.LogicalProcessorIndex,
                ));
            }
        }

        offset += record_size;
    }

    let legacy_strategy = EcoQosCpuRestrictionStrategy::from_legacy_flags(
        prefer_efficiency_cores,
        limit_cpu_sets_on_non_hybrid,
    );
    let restriction = if restriction.strategy == EcoQosCpuRestrictionStrategy::Auto
        && legacy_strategy != EcoQosCpuRestrictionStrategy::Auto
    {
        EcoQosCpuRestriction {
            strategy: legacy_strategy,
            ..restriction
        }
    } else {
        restriction
    };

    cpu_set_target_ids_from_records(&records, restriction)
}

fn cpu_set_target_ids_from_records(
    records: &[(u32, u8, u8)],
    restriction: EcoQosCpuRestriction,
) -> Vec<u32> {
    if restriction.control_style == EcoQosCpuRestrictionControlStyle::CoreToggle {
        let mut ids = records
            .iter()
            .filter_map(|(id, _, logical_index)| {
                let index = usize::from(*logical_index);
                (index < u64::BITS as usize && (restriction.core_mask & (1_u64 << index)) != 0)
                    .then_some(*id)
            })
            .collect::<Vec<_>>();
        ids.sort_unstable();
        ids.dedup();
        return ids;
    }

    let Some(min_efficiency_class) = records.iter().map(|(_, class, _)| *class).min() else {
        return Vec::new();
    };
    let max_efficiency_class = records
        .iter()
        .map(|(_, class, _)| *class)
        .max()
        .unwrap_or(min_efficiency_class);
    let has_hybrid_classes = min_efficiency_class != max_efficiency_class;
    let mut selected = match restriction.strategy {
        EcoQosCpuRestrictionStrategy::Off => Vec::new(),
        EcoQosCpuRestrictionStrategy::Auto if has_hybrid_classes => records
            .iter()
            .filter(|(_, class, _)| *class == min_efficiency_class)
            .copied()
            .collect::<Vec<_>>(),
        EcoQosCpuRestrictionStrategy::PreferEfficiencyCores => {
            if !has_hybrid_classes {
                Vec::new()
            } else {
                records
                    .iter()
                    .filter(|(_, class, _)| *class == min_efficiency_class)
                    .copied()
                    .collect::<Vec<_>>()
            }
        }
        EcoQosCpuRestrictionStrategy::Auto | EcoQosCpuRestrictionStrategy::LimitLogicalCpus => {
            records.to_vec()
        }
    };

    if selected.is_empty() {
        return Vec::new();
    }

    selected.sort_by_key(|(_, _, logical_index)| *logical_index);
    let percent_count =
        (selected.len() * usize::from(restriction.percent.clamp(1, 100))).div_ceil(100);
    let max_count = usize::from(restriction.max_logical_processors);
    let limit = if max_count == 0 {
        percent_count
    } else {
        percent_count.min(max_count)
    }
    .clamp(1, selected.len());

    let mut ids = selected
        .into_iter()
        .take(limit)
        .map(|(id, _, _)| id)
        .collect::<Vec<_>>();
    ids.sort_unstable();
    ids.dedup();
    ids
}

struct ProcessHandle(HANDLE);

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
                return Ok(Self(handle));
            }
            last_open_error = last_error();
        }

        Err(open_process_error(process_id, last_open_error))
    }

    fn power_throttling_state(&self) -> Result<PROCESS_POWER_THROTTLING_STATE, EcoQosError> {
        let mut state = PROCESS_POWER_THROTTLING_STATE::default();
        let ok = unsafe {
            GetProcessInformation(
                self.0,
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
        let priority = unsafe { GetPriorityClass(self.0) };
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
                self.0,
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
        let ok = unsafe { SetPriorityClass(self.0, priority_class) };
        if ok == 0 {
            Err(EcoQosError::Failed(format!(
                "SetPriorityClass failed with error {}.",
                last_error()
            )))
        } else {
            Ok(())
        }
    }

    fn affinity_mask(&self) -> Result<(usize, usize), EcoQosError> {
        let mut process_affinity = 0;
        let mut system_affinity = 0;
        let ok =
            unsafe { GetProcessAffinityMask(self.0, &mut process_affinity, &mut system_affinity) };
        if ok == 0 {
            Err(EcoQosError::Failed(format!(
                "GetProcessAffinityMask failed with error {}.",
                last_error()
            )))
        } else {
            Ok((process_affinity, system_affinity))
        }
    }

    fn set_affinity_mask(&self, affinity_mask: usize) -> Result<(), EcoQosError> {
        let ok = unsafe { SetProcessAffinityMask(self.0, affinity_mask) };
        if ok == 0 {
            Err(EcoQosError::Failed(format!(
                "SetProcessAffinityMask failed with error {}.",
                last_error()
            )))
        } else {
            Ok(())
        }
    }

    fn default_cpu_set_ids(&self) -> Result<Vec<u32>, EcoQosError> {
        let mut required_id_count = 0;
        unsafe {
            GetProcessDefaultCpuSets(self.0, null_mut(), 0, &mut required_id_count);
        }
        if required_id_count == 0 {
            return Ok(Vec::new());
        }

        let mut ids = vec![0_u32; required_id_count as usize];
        let ok = unsafe {
            GetProcessDefaultCpuSets(
                self.0,
                ids.as_mut_ptr(),
                ids.len() as u32,
                &mut required_id_count,
            )
        };
        if ok == 0 {
            Err(EcoQosError::Failed(format!(
                "GetProcessDefaultCpuSets failed with error {}.",
                last_error()
            )))
        } else {
            ids.truncate(required_id_count as usize);
            Ok(ids)
        }
    }

    fn set_default_cpu_set_ids(&self, ids: &[u32]) -> Result<(), EcoQosError> {
        let (ptr, count) = if ids.is_empty() {
            (null_mut(), 0)
        } else {
            (ids.as_ptr() as *mut u32, ids.len() as u32)
        };
        let ok = unsafe { SetProcessDefaultCpuSets(self.0, ptr, count) };
        if ok == 0 {
            Err(EcoQosError::Failed(format!(
                "SetProcessDefaultCpuSets failed with error {}.",
                last_error()
            )))
        } else {
            Ok(())
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

fn last_error() -> u32 {
    unsafe { GetLastError() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exclusions_include_builtin_and_user_entries() {
        let settings = EcoQosSettings {
            enabled: true,
            exclude_foreground_app: true,
            prefer_efficiency_cores: false,
            limit_cpu_sets_on_non_hybrid: false,
            cpu_restriction_mode: EcoQosCpuRestrictionMode::SoftCpuSets,
            cpu_restriction_strategy: EcoQosCpuRestrictionStrategy::Off,
            cpu_restriction_control_style: EcoQosCpuRestrictionControlStyle::Percentage,
            cpu_restriction_percent: 50,
            cpu_restriction_max_logical_processors: 0,
            cpu_restriction_core_mask: 0,
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
            prefer_efficiency_cores: false,
            limit_cpu_sets_on_non_hybrid: false,
            cpu_restriction_mode: EcoQosCpuRestrictionMode::SoftCpuSets,
            cpu_restriction_strategy: EcoQosCpuRestrictionStrategy::Off,
            cpu_restriction_control_style: EcoQosCpuRestrictionControlStyle::Percentage,
            cpu_restriction_percent: 50,
            cpu_restriction_max_logical_processors: 0,
            cpu_restriction_core_mask: 0,
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
    fn non_hybrid_cpu_sets_can_be_limited() {
        let records = [(10, 0), (11, 0), (12, 0), (13, 0)];

        assert_eq!(
            limited_cpu_set_ids_for_test(&records, false, true),
            vec![10, 11]
        );
        assert!(limited_cpu_set_ids_for_test(&records, true, false).is_empty());
    }

    #[test]
    fn hybrid_cpu_sets_prefer_efficiency_class() {
        let records = [(20, 0), (21, 0), (30, 1), (31, 1)];

        assert_eq!(limited_cpu_set_ids_for_test(&records, true, true), vec![20]);
        assert_eq!(
            limited_cpu_set_ids_for_test(&records, false, true),
            vec![20, 21]
        );
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
                previous_cpu_set_ids: None,
                applied_cpu_set_ids: None,
                previous_affinity: None,
                applied_affinity: None,
            },
        );
        let mut log = ActionLog::new(8);

        let failures = manager.release_processes(&[0], &mut log, "test");

        assert_eq!(failures.count, 0);
        assert!(log.entries().is_empty());
        assert!(manager.throttled.is_empty());
    }

    #[test]
    fn eco_qos_backend_rejects_non_efficiency_resource_actions() {
        let mut throttled = BTreeMap::new();
        let mut log = ActionLog::new(8);
        let mut backend = EcoQosActionBackend {
            process_id: 42,
            process_name: "worker.exe".to_owned(),
            throttled: &mut throttled,
            prefer_efficiency_cores: false,
            limit_cpu_sets_on_non_hybrid: false,
            restriction: EcoQosCpuRestriction {
                mode: EcoQosCpuRestrictionMode::SoftCpuSets,
                strategy: EcoQosCpuRestrictionStrategy::Off,
                control_style: EcoQosCpuRestrictionControlStyle::Percentage,
                percent: 50,
                max_logical_processors: 0,
                core_mask: 0,
            },
            action_log: &mut log,
            last_error: None,
            enabled_new_process: false,
        };
        let action = Action::SuspendApp {
            app: AppMatcher::ProcessName("worker.exe".to_owned()),
        };

        assert_eq!(
            ActionExecutor.apply_app_resource_action(&action, &mut backend),
            ActionExecution::Failed(
                "EcoQoS backend only supports per-process efficiency-mode actions.".to_owned()
            )
        );
        assert!(backend.throttled.is_empty());
        assert!(backend.action_log.entries().is_empty());
    }

    fn limited_cpu_set_ids_for_test(
        records: &[(u32, u8)],
        prefer_efficiency_cores: bool,
        limit_cpu_sets_on_non_hybrid: bool,
    ) -> Vec<u32> {
        let records = records
            .iter()
            .enumerate()
            .map(|(index, (id, class))| (*id, *class, index as u8))
            .collect::<Vec<_>>();
        cpu_set_target_ids_from_records(
            &records,
            EcoQosCpuRestriction {
                mode: EcoQosCpuRestrictionMode::SoftCpuSets,
                strategy: EcoQosCpuRestrictionStrategy::from_legacy_flags(
                    prefer_efficiency_cores,
                    limit_cpu_sets_on_non_hybrid,
                ),
                control_style: EcoQosCpuRestrictionControlStyle::Percentage,
                percent: 50,
                max_logical_processors: 0,
                core_mask: 0,
            },
        )
    }

    #[test]
    fn logical_limit_respects_percentage_and_maximum() {
        let records = [(10, 0, 0), (11, 0, 1), (12, 0, 2), (13, 0, 3)];
        assert_eq!(
            cpu_set_target_ids_from_records(
                &records,
                EcoQosCpuRestriction {
                    mode: EcoQosCpuRestrictionMode::SoftCpuSets,
                    strategy: EcoQosCpuRestrictionStrategy::LimitLogicalCpus,
                    control_style: EcoQosCpuRestrictionControlStyle::Percentage,
                    percent: 75,
                    max_logical_processors: 2,
                    core_mask: 0,
                },
            ),
            vec![10, 11]
        );
        assert_eq!(
            logical_indices_to_limited_mask(&[0, 1, 2, 3], 50, 1),
            Some(0b0001)
        );
    }

    #[test]
    fn core_toggle_cpu_sets_use_selected_logical_indices() {
        let records = [(10, 0, 0), (11, 0, 1), (12, 1, 2), (13, 1, 3)];

        assert_eq!(
            cpu_set_target_ids_from_records(
                &records,
                EcoQosCpuRestriction {
                    mode: EcoQosCpuRestrictionMode::SoftCpuSets,
                    strategy: EcoQosCpuRestrictionStrategy::Auto,
                    control_style: EcoQosCpuRestrictionControlStyle::CoreToggle,
                    percent: 50,
                    max_logical_processors: 0,
                    core_mask: 0b1010,
                },
            ),
            vec![11, 13]
        );
    }
}
