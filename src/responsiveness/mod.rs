use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::c_void,
    time::{Duration, Instant},
};

use windows_sys::Win32::{
    Foundation::{
        CloseHandle, GetLastError, ERROR_ACCESS_DENIED, ERROR_INVALID_PARAMETER, FILETIME, HANDLE,
    },
    System::{
        RemoteDesktop::ProcessIdToSessionId,
        Threading::{
            GetCurrentProcessId, GetPriorityClass, GetProcessInformation, GetProcessTimes,
            OpenProcess, ProcessPowerThrottling, SetPriorityClass, SetProcessInformation,
            ABOVE_NORMAL_PRIORITY_CLASS, BELOW_NORMAL_PRIORITY_CLASS, HIGH_PRIORITY_CLASS,
            IDLE_PRIORITY_CLASS, NORMAL_PRIORITY_CLASS, PROCESS_POWER_THROTTLING_CURRENT_VERSION,
            PROCESS_POWER_THROTTLING_EXECUTION_SPEED, PROCESS_POWER_THROTTLING_STATE,
            PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SET_INFORMATION, REALTIME_PRIORITY_CLASS,
        },
    },
};

use crate::{
    action_log::{ActionLog, ActionLogAction, ActionLogFeature, ActionLogResult},
    affinity::{self, CpuAffinityManager, LogicalProcessorKind},
    config::{
        CpuAffinityMode, CpuAffinityRule, CpuAffinitySettings, EcoQosCpuRestrictionMode,
        ForegroundBoostPriority, ForegroundResponsivenessSettings, PriorityRule, ProcessPriority,
    },
    foreground::list_processes,
    rules::{
        execution_failure_suppression_threshold, Action, ActionExecution, ActionExecutor,
        AppMatcher, AppPriorityActionBackend, ExecutionFailureState, RuleProcessPriority,
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForegroundResponsivenessSnapshot {
    pub enabled: bool,
    pub scanned_processes: usize,
    pub background_adjusted_processes: usize,
    pub foreground_boosted_process: Option<String>,
    pub auto_balanced_processes: usize,
    pub auto_balance_message: String,
    pub auto_balance_total_cpu_usage_tenths: Option<u16>,
    pub auto_balance_details: Vec<AutoBalanceProcessStatus>,
    pub skipped_processes: usize,
    pub failed_processes: usize,
    pub adjusted_apps: Vec<String>,
    pub message: String,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoBalanceProcessStatus {
    pub process_id: u32,
    pub process_name: String,
    pub state: AutoBalanceProcessState,
    pub cpu_usage_tenths: Option<u16>,
    pub elapsed_seconds: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoBalanceProcessState {
    Watching,
    Restrained,
    CoolingDown,
}

pub struct ForegroundResponsivenessManager {
    adjusted: BTreeMap<u32, AdjustedProcess>,
    boosted: Option<BoostedProcess>,
    foreground_candidate: Option<ForegroundCandidate>,
    foreground_cpu_sample: Option<(u32, ProcessCpuSample)>,
    lower_background_affinity: CpuAffinityManager,
    auto_balance: BTreeMap<u32, AutoBalanceProcess>,
    auto_balance_affinity: CpuAffinityManager,
    failure_suppression: BTreeMap<String, PriorityFailureSuppression>,
}

impl Default for ForegroundResponsivenessManager {
    fn default() -> Self {
        Self {
            adjusted: BTreeMap::new(),
            boosted: None,
            foreground_candidate: None,
            foreground_cpu_sample: None,
            lower_background_affinity: CpuAffinityManager::with_action_log_feature(
                ActionLogFeature::ForegroundResponsiveness,
            ),
            auto_balance: BTreeMap::new(),
            auto_balance_affinity: CpuAffinityManager::with_action_log_feature(
                ActionLogFeature::ForegroundResponsiveness,
            ),
            failure_suppression: BTreeMap::new(),
        }
    }
}

#[derive(Clone)]
struct AdjustedProcess {
    process_name: String,
    previous_priority: u32,
    applied_priority: u32,
    previous_efficiency_state: Option<PROCESS_POWER_THROTTLING_STATE>,
    applied_efficiency_mode: bool,
}

type PriorityFailureSuppression = ExecutionFailureState;

#[derive(Clone)]
struct BoostedProcess {
    process_id: u32,
    process_name: String,
    previous_priority: u32,
    applied_priority: u32,
}

struct ForegroundCandidate {
    process_id: u32,
    process_name: String,
    first_seen: Instant,
}

#[derive(Clone)]
struct AutoBalanceProcess {
    process_name: String,
    previous_cpu_time: Option<ProcessCpuSample>,
    last_usage_tenths: Option<u16>,
    high_since: Option<Instant>,
    below_since: Option<Instant>,
    active_since: Option<Instant>,
    active: bool,
}

#[derive(Clone, Copy)]
struct ProcessCpuSample {
    cpu_time_100ns: u64,
    sampled_at: Instant,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PriorityTargetSource {
    BackgroundPolicy,
    Rule,
}

impl ForegroundResponsivenessManager {
    pub fn update(
        &mut self,
        settings: &ForegroundResponsivenessSettings,
        automation_enabled: bool,
        foreground_process_id: Option<u32>,
        _total_cpu_usage_percent: Option<f32>,
        eco_qos_process_ids: &BTreeSet<u32>,
        action_log: &mut ActionLog,
    ) -> ForegroundResponsivenessSnapshot {
        if !automation_enabled {
            let failed = self.clear_all(action_log, "automation disabled");
            self.failure_suppression.clear();
            return ForegroundResponsivenessSnapshot {
                enabled: false,
                failed_processes: failed.count,
                message: "Automation disabled.".to_owned(),
                last_error: failed.last_error,
                ..Default::default()
            };
        }

        if !settings.enabled {
            let failed = self.clear_all(action_log, "Foreground Responsiveness disabled");
            self.failure_suppression.clear();
            return ForegroundResponsivenessSnapshot {
                enabled: false,
                failed_processes: failed.count,
                message: "Foreground Responsiveness disabled.".to_owned(),
                last_error: failed.last_error,
                ..Default::default()
            };
        }

        let current_process_id = unsafe { GetCurrentProcessId() };
        let Some(current_session_id) = process_session_id(current_process_id) else {
            let failed = self.clear_all(action_log, "current Windows session is unknown");
            return ForegroundResponsivenessSnapshot {
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
                return ForegroundResponsivenessSnapshot {
                    enabled: true,
                    failed_processes: failed.count,
                    message: err,
                    last_error: failed.last_error,
                    ..Default::default()
                };
            }
        };

        let scanned_processes = processes.len();
        let foreground_cpu_usage_percent = self.update_foreground_cpu_usage(foreground_process_id);
        let foreground_cpu_usage_tenths = foreground_cpu_usage_percent.map(percent_tenths);
        let current_process_names = processes
            .iter()
            .map(|process| (process.id, process.name.clone()))
            .collect::<BTreeMap<_, _>>();
        let foreground_process_name = foreground_process_id.and_then(|id| {
            processes
                .iter()
                .find(|process| process.id == id)
                .map(|process| process.name.clone())
        });

        let mut failures = PriorityFailures::default();
        let keep_current_boost = self.boosted.as_ref().is_some_and(|boosted| {
            settings.boost_foreground_app
                && foreground_process_id == Some(boosted.process_id)
                && !eco_qos_process_ids.contains(&boosted.process_id)
        });
        if self.boosted.is_some() && !keep_current_boost {
            if let Some(error) =
                self.clear_boosted(true, action_log, "foreground boost no longer applies")
            {
                failures.merge(error);
            }
        }

        let mut lowerable_background_processes = BTreeMap::new();
        for process in &processes {
            if should_skip_process(
                process.id,
                &process.name,
                current_process_id,
                foreground_process_id,
                foreground_process_name.as_deref(),
                eco_qos_process_ids,
            ) {
                continue;
            }

            if process_session_id(process.id) != Some(current_session_id) {
                continue;
            }

            lowerable_background_processes.insert(process.id, process.name.clone());
        }

        let mut target_processes = BTreeMap::new();
        if settings.lower_background_apps {
            for (process_id, process_name) in &lowerable_background_processes {
                let priority = matching_rule(settings, process_name)
                    .map(|rule| rule.priority)
                    .unwrap_or(ProcessPriority::Idle);
                let source = if matching_rule(settings, process_name).is_some() {
                    PriorityTargetSource::Rule
                } else {
                    PriorityTargetSource::BackgroundPolicy
                };
                target_processes.insert(*process_id, (process_name.clone(), priority, source));
            }
        }

        let auto_balance_running = auto_balance_should_run(settings, foreground_cpu_usage_percent);
        let lower_background_affinity_percent =
            smart_background_affinity_percent(settings, foreground_cpu_usage_percent);
        let lower_background_affinity_active =
            settings.auto_balance_enabled && auto_balance_running;
        let lower_background_affinity_rules = if lower_background_affinity_active {
            lower_background_core_mask(settings, lower_background_affinity_percent)
                .map(|core_mask| {
                    lowerable_background_processes
                        .values()
                        .map(|process_name| CpuAffinityRule {
                            enabled: true,
                            mode: auto_balance_affinity_mode(settings.auto_balance_affinity_mode),
                            process_name: process_name.clone(),
                            core_mask,
                        })
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let lower_background_affinity_settings = CpuAffinitySettings {
            enabled: settings.enabled && lower_background_affinity_active,
            exclude_foreground_app: true,
            rules: lower_background_affinity_rules,
        };
        let lower_background_affinity_snapshot = self.lower_background_affinity.update(
            &lower_background_affinity_settings,
            automation_enabled,
            foreground_process_id,
            action_log,
        );
        failures.count += lower_background_affinity_snapshot.failed_processes;
        if failures.last_error.is_none() {
            failures.last_error = lower_background_affinity_snapshot.last_error.clone();
        }

        let mut auto_balance_rules = Vec::new();
        if auto_balance_running {
            let now = Instant::now();
            let current_ids = processes
                .iter()
                .map(|process| process.id)
                .collect::<BTreeSet<_>>();
            self.auto_balance
                .retain(|process_id, _| current_ids.contains(process_id));

            for process in &processes {
                if should_skip_process(
                    process.id,
                    &process.name,
                    current_process_id,
                    foreground_process_id,
                    foreground_process_name.as_deref(),
                    eco_qos_process_ids,
                ) || settings.auto_balance_exclusion_enabled_for(&process.name)
                    || process_session_id(process.id) != Some(current_session_id)
                {
                    continue;
                }

                if self
                    .update_auto_balance_process(process.id, &process.name, settings, now)
                    .is_some()
                {
                    if let Some(core_mask) = auto_balance_core_mask(settings) {
                        auto_balance_rules.push(CpuAffinityRule {
                            enabled: true,
                            mode: auto_balance_affinity_mode(settings.auto_balance_affinity_mode),
                            process_name: process.name.clone(),
                            core_mask,
                        });
                    }
                }
            }
        } else if settings.auto_balance_enabled {
            self.auto_balance.clear();
        } else {
            self.auto_balance.clear();
        }

        let affinity_settings = CpuAffinitySettings {
            enabled: settings.enabled && settings.auto_balance_enabled && auto_balance_running,
            exclude_foreground_app: true,
            rules: auto_balance_rules,
        };
        let auto_balance_affinity_snapshot = self.auto_balance_affinity.update(
            &affinity_settings,
            automation_enabled,
            foreground_process_id,
            action_log,
        );
        failures.count += auto_balance_affinity_snapshot.failed_processes;
        if failures.last_error.is_none() {
            failures.last_error = auto_balance_affinity_snapshot.last_error.clone();
        }

        let target_ids = target_processes.keys().copied().collect::<BTreeSet<_>>();
        let mut active_target_names = target_processes
            .values()
            .map(|(name, _priority, _source)| process_failure_key(name))
            .collect::<BTreeSet<_>>();
        if let Some(name) = foreground_process_name.as_deref() {
            active_target_names.insert(process_failure_key(name));
        }
        self.failure_suppression
            .retain(|name, _| active_target_names.contains(name));
        failures.merge(self.release_non_targets(
            &target_ids,
            &current_process_names,
            action_log,
            "process no longer matches a responsiveness rule",
        ));
        let mut skipped_processes = 0;

        let auto_balanced_processes = auto_balance_affinity_snapshot.adjusted_processes;
        for (process_id, (process_name, priority, source)) in target_processes {
            let failure_process_name = process_name.clone();
            if self.is_process_suppressed(process_id, &failure_process_name, action_log) {
                skipped_processes += 1;
                continue;
            }
            let action = Action::SetAppPriority {
                app: AppMatcher::ProcessName(process_name.clone()),
                priority: rule_process_priority(priority),
            };
            let mut backend = ForegroundResponsivenessPriorityBackend {
                process_id,
                process_name,
                existing: self.adjusted.get(&process_id),
                action_log,
                source,
                apply_efficiency_mode: true,
                adjusted: None,
                skipped: false,
                last_error: None,
            };
            let execution = ActionExecutor.apply_app_priority_action(&action, &mut backend);
            let adjusted = backend.adjusted.take();
            let skipped = backend.skipped;
            let last_error = backend.last_error.take();
            drop(backend);

            match execution {
                ActionExecution::Applied | ActionExecution::AlreadyApplied => {
                    if let Some(adjusted) = adjusted {
                        self.clear_process_failure(&failure_process_name);
                        self.adjusted.insert(process_id, adjusted);
                    } else if skipped {
                        self.clear_process_failure(&failure_process_name);
                        skipped_processes += 1;
                    }
                }
                ActionExecution::Failed(_)
                    if matches!(
                        last_error.as_ref(),
                        Some(PriorityError::AccessDenied | PriorityError::ProcessExited)
                    ) =>
                {
                    skipped_processes += 1;
                    if !matches!(last_error.as_ref(), Some(PriorityError::ProcessExited)) {
                        self.record_process_failure(&failure_process_name);
                    }
                    action_log.record(
                        ActionLogFeature::ForegroundResponsiveness,
                        Some(process_id),
                        failure_process_name,
                        ActionLogAction::Skip,
                        ActionLogResult::Skipped,
                        "Skipped because the process could not be opened.",
                    );
                }
                ActionExecution::Failed(err) => {
                    self.record_process_failure(&failure_process_name);
                    failures.record_message(
                        "Apply",
                        process_id,
                        &failure_process_name,
                        err,
                        action_log,
                    );
                }
                ActionExecution::Unsupported => {
                    self.record_process_failure(&failure_process_name);
                    failures.record_message(
                        "Apply",
                        process_id,
                        &failure_process_name,
                        "Foreground Responsiveness action was not supported by the generic executor."
                            .to_owned(),
                        action_log,
                    );
                }
            }
        }

        let auto_balance_details = self.auto_balance_statuses(Instant::now());
        let auto_balance_message = auto_balance_status_message(
            settings,
            foreground_cpu_usage_percent,
            auto_balance_running,
            auto_balance_details
                .iter()
                .filter(|status| status.state == AutoBalanceProcessState::Restrained)
                .count(),
        );

        if let Some(foreground_id) = foreground_process_id {
            if settings.boost_foreground_app && !eco_qos_process_ids.contains(&foreground_id) {
                let foreground_failure_name = foreground_process_name.clone().unwrap_or_default();
                if self.is_process_suppressed(foreground_id, &foreground_failure_name, action_log) {
                    skipped_processes += 1;
                } else {
                    match self.apply_foreground_boost_action(
                        foreground_id,
                        foreground_process_name.as_deref(),
                        current_process_id,
                        current_session_id,
                        settings.foreground_stability_delay_ms,
                        settings.foreground_boost,
                        action_log,
                    ) {
                        Ok(()) => {
                            self.clear_process_failure(&foreground_failure_name);
                        }
                        Err(err @ (PriorityError::AccessDenied | PriorityError::ProcessExited)) => {
                            skipped_processes += 1;
                            if !matches!(err, PriorityError::ProcessExited) {
                                self.record_process_failure(&foreground_failure_name);
                            }
                            action_log.record(
                                ActionLogFeature::ForegroundResponsiveness,
                                Some(foreground_id),
                                foreground_process_name.clone().unwrap_or_default(),
                                ActionLogAction::Skip,
                                ActionLogResult::Skipped,
                                "Skipped foreground boost because the process could not be opened.",
                            );
                        }
                        Err(PriorityError::Failed(err)) => {
                            self.record_process_failure(&foreground_failure_name);
                            failures.record_message(
                                "Boost",
                                foreground_id,
                                foreground_process_name.as_deref().unwrap_or(""),
                                err,
                                action_log,
                            );
                        }
                    }
                }
            } else if let Some(error) =
                self.clear_boosted(true, action_log, "foreground boost disabled or blocked")
            {
                failures.merge(error);
            }
        } else if let Some(error) =
            self.clear_boosted(true, action_log, "foreground app is unknown")
        {
            failures.merge(error);
        }

        ForegroundResponsivenessSnapshot {
            enabled: true,
            scanned_processes,
            background_adjusted_processes: self.adjusted.len(),
            foreground_boosted_process: self
                .boosted
                .as_ref()
                .map(|process| format!("{} ({})", process.process_name, process.process_id)),
            auto_balanced_processes,
            auto_balance_message,
            auto_balance_total_cpu_usage_tenths: foreground_cpu_usage_tenths,
            auto_balance_details,
            skipped_processes,
            failed_processes: failures.count,
            adjusted_apps: unique_app_names(
                self.adjusted
                    .values()
                    .map(|process| process.process_name.as_str()),
            ),
            message: "Foreground Responsiveness active.".to_owned(),
            last_error: failures.last_error,
        }
    }

    fn release_non_targets(
        &mut self,
        target_ids: &BTreeSet<u32>,
        current_process_names: &BTreeMap<u32, String>,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> PriorityFailures {
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

    fn apply_foreground_boost_action(
        &mut self,
        process_id: u32,
        process_name: Option<&str>,
        current_process_id: u32,
        current_session_id: u32,
        stability_delay_ms: u64,
        foreground_boost: ForegroundBoostPriority,
        action_log: &mut ActionLog,
    ) -> Result<(), PriorityError> {
        let action = Action::BoostForegroundPriority {
            app: AppMatcher::ProcessName(process_name.unwrap_or_default().to_owned()),
            priority: foreground_boost_rule_priority(foreground_boost),
        };
        let mut backend = ForegroundBoostPriorityBackend {
            manager: self,
            process_id,
            process_name,
            current_process_id,
            current_session_id,
            stability_delay_ms,
            priority_class: foreground_boost_priority_class(foreground_boost),
            action_log,
            last_error: None,
        };
        let execution = ActionExecutor.apply_app_priority_action(&action, &mut backend);
        let last_error = backend.last_error.take();
        drop(backend);

        match execution {
            ActionExecution::Applied | ActionExecution::AlreadyApplied => Ok(()),
            ActionExecution::Failed(err) => Err(last_error.unwrap_or(PriorityError::Failed(err))),
            ActionExecution::Unsupported => Err(PriorityError::Failed(
                "Foreground boost action was not supported by the generic executor.".to_owned(),
            )),
        }
    }

    fn clear_all(&mut self, action_log: &mut ActionLog, reason: &str) -> PriorityFailures {
        let mut failures = self
            .clear_boosted(true, action_log, reason)
            .unwrap_or_default();
        let process_ids = self.adjusted.keys().copied().collect::<Vec<_>>();
        failures.merge(self.release_processes(&process_ids, None, action_log, reason));
        self.foreground_candidate = None;
        self.foreground_cpu_sample = None;
        self.auto_balance.clear();
        let affinity_settings = CpuAffinitySettings {
            enabled: false,
            exclude_foreground_app: true,
            rules: Vec::new(),
        };
        let lower_affinity_snapshot =
            self.lower_background_affinity
                .update(&affinity_settings, true, None, action_log);
        failures.count += lower_affinity_snapshot.failed_processes;
        if failures.last_error.is_none() {
            failures.last_error = lower_affinity_snapshot.last_error;
        }
        let affinity_snapshot =
            self.auto_balance_affinity
                .update(&affinity_settings, true, None, action_log);
        failures.count += affinity_snapshot.failed_processes;
        if failures.last_error.is_none() {
            failures.last_error = affinity_snapshot.last_error;
        }
        failures
    }

    fn clear_boosted(
        &mut self,
        reset_candidate: bool,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> Option<PriorityFailures> {
        if reset_candidate {
            self.foreground_candidate = None;
        }
        let boosted = self.boosted.take()?;
        let mut failures = PriorityFailures::default();
        let process_id = boosted.process_id;
        let process_name = boosted.process_name.clone();
        if let Err(err) = restore_boosted_priority(boosted) {
            if !matches!(err, PriorityError::ProcessExited) {
                failures.record_error("Restore", process_id, &process_name, err, action_log);
            }
        } else {
            action_log.record(
                ActionLogFeature::ForegroundResponsiveness,
                Some(process_id),
                process_name,
                ActionLogAction::Restore,
                ActionLogResult::Restored,
                format!("{reason}: restored foreground boost."),
            );
        }
        Some(failures)
    }

    fn is_process_suppressed(
        &mut self,
        process_id: u32,
        process_name: &str,
        action_log: &mut ActionLog,
    ) -> bool {
        let process_name = process_name.trim();
        if process_name.is_empty() {
            return false;
        }
        let Some(suppression) = self
            .failure_suppression
            .get_mut(&process_failure_key(process_name))
        else {
            return false;
        };
        if !suppression.is_suppressed() {
            return false;
        }

        if suppression.mark_suppression_logged() {
            action_log.record(
                ActionLogFeature::ForegroundResponsiveness,
                Some(process_id),
                process_name.to_owned(),
                ActionLogAction::Skip,
                ActionLogResult::Skipped,
                format!(
                    "Stopped retrying Foreground Responsiveness after {} failed attempts.",
                    execution_failure_suppression_threshold(),
                ),
            );
        }

        true
    }

    fn record_process_failure(&mut self, process_name: &str) {
        let process_name = process_name.trim();
        if process_name.is_empty() {
            return;
        }
        let suppression = self
            .failure_suppression
            .entry(process_failure_key(process_name))
            .or_default();
        suppression.record_failure();
    }

    fn clear_process_failure(&mut self, process_name: &str) {
        let process_name = process_name.trim();
        if process_name.is_empty() {
            return;
        }
        self.failure_suppression
            .remove(&process_failure_key(process_name));
    }

    fn release_processes(
        &mut self,
        process_ids: &[u32],
        current_process_names: Option<&BTreeMap<u32, String>>,
        action_log: &mut ActionLog,
        reason: &str,
    ) -> PriorityFailures {
        let mut failures = PriorityFailures::default();
        for process_id in process_ids {
            if let Some(process) = self.adjusted.remove(process_id) {
                let process_name = process.process_name.clone();
                let still_same_process = current_process_names.map_or(true, |names| {
                    names
                        .get(process_id)
                        .is_some_and(|name| name.eq_ignore_ascii_case(&process.process_name))
                });
                if still_same_process {
                    if let Err(err) = restore_adjusted_priority(*process_id, process) {
                        if !matches!(err, PriorityError::ProcessExited) {
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
                            ActionLogFeature::ForegroundResponsiveness,
                            Some(*process_id),
                            process_name,
                            ActionLogAction::Restore,
                            ActionLogResult::Restored,
                            format!("{reason}: restored background priority."),
                        );
                    }
                }
            }
        }
        failures
    }

    fn update_foreground_boost(
        &mut self,
        process_id: u32,
        process_name: Option<&str>,
        current_process_id: u32,
        current_session_id: u32,
        stability_delay_ms: u64,
        priority_class: u32,
        action_log: &mut ActionLog,
    ) -> Result<(), PriorityError> {
        let process_name = process_name.unwrap_or("").trim();
        if process_name.is_empty()
            || process_id == 0
            || process_id == current_process_id
            || is_builtin_excluded(process_name)
            || process_session_id(process_id) != Some(current_session_id)
        {
            if let Some(error) =
                self.clear_boosted(true, action_log, "foreground process is not eligible")
            {
                return error.into_result();
            }
            return Ok(());
        }

        if self.boosted.as_ref().is_some_and(|boosted| {
            boosted.process_id == process_id
                && boosted.process_name.eq_ignore_ascii_case(process_name)
                && boosted.applied_priority == priority_class
        }) {
            return Ok(());
        }

        let now = Instant::now();
        let stable = match &mut self.foreground_candidate {
            Some(candidate)
                if candidate.process_id == process_id
                    && candidate.process_name.eq_ignore_ascii_case(process_name) =>
            {
                now.duration_since(candidate.first_seen).as_millis()
                    >= u128::from(stability_delay_ms)
            }
            _ => {
                self.foreground_candidate = Some(ForegroundCandidate {
                    process_id,
                    process_name: process_name.to_owned(),
                    first_seen: now,
                });
                false
            }
        };

        if !stable {
            if let Some(error) = self.clear_boosted(
                false,
                action_log,
                "foreground app changed before stability delay",
            ) {
                return error.into_result();
            }
            return Ok(());
        }

        if let Some(boosted) = self.boosted.take() {
            let boosted_process_id = boosted.process_id;
            let boosted_process_name = boosted.process_name.clone();
            restore_boosted_priority(boosted)?;
            action_log.record(
                ActionLogFeature::ForegroundResponsiveness,
                Some(boosted_process_id),
                boosted_process_name,
                ActionLogAction::Restore,
                ActionLogResult::Restored,
                "Foreground focus changed: restored previous foreground boost.",
            );
        }

        let process = ProcessHandle::open(process_id)?;
        let current_priority = process.priority_class()?;
        if current_priority == HIGH_PRIORITY_CLASS || current_priority == REALTIME_PRIORITY_CLASS {
            return Ok(());
        }
        if current_priority != priority_class {
            process.set_priority_class(priority_class)?;
            action_log.record(
                ActionLogFeature::ForegroundResponsiveness,
                Some(process_id),
                process_name.to_owned(),
                ActionLogAction::Apply,
                ActionLogResult::Applied,
                format!(
                    "Boosted foreground priority to {}.",
                    priority_class_label(priority_class)
                ),
            );
        }
        self.boosted = Some(BoostedProcess {
            process_id,
            process_name: process_name.to_owned(),
            previous_priority: current_priority,
            applied_priority: priority_class,
        });
        Ok(())
    }

    fn update_auto_balance_process(
        &mut self,
        process_id: u32,
        process_name: &str,
        settings: &ForegroundResponsivenessSettings,
        now: Instant,
    ) -> Option<()> {
        let threshold = f32::from(settings.auto_balance_threshold_percent.min(100));
        let restore_threshold = f32::from(
            settings
                .auto_balance_restore_threshold_percent
                .min(settings.auto_balance_threshold_percent)
                .min(100),
        );
        let sustain = Duration::from_secs(settings.auto_balance_sustain_seconds);
        let minimum_restraint =
            Duration::from_secs(settings.auto_balance_minimum_restraint_seconds);
        let cooldown = Duration::from_secs(settings.auto_balance_cooldown_seconds);
        let state = self
            .auto_balance
            .entry(process_id)
            .or_insert_with(|| AutoBalanceProcess {
                process_name: process_name.to_owned(),
                previous_cpu_time: None,
                last_usage_tenths: None,
                high_since: None,
                below_since: None,
                active_since: None,
                active: false,
            });
        state.process_name = process_name.to_owned();

        let current = process_cpu_sample(process_id).ok()?;
        let usage = state
            .previous_cpu_time
            .and_then(|previous| process_cpu_usage_percent(previous, current));
        state.previous_cpu_time = Some(current);

        let usage = usage?;
        state.last_usage_tenths = Some(percent_tenths(usage));
        if usage >= threshold {
            state.below_since = None;
            let high_since = *state.high_since.get_or_insert(now);
            if state.active || now.duration_since(high_since) >= sustain {
                if !state.active {
                    state.active_since = Some(now);
                }
                state.active = true;
                return Some(());
            }
            return None;
        }

        state.high_since = None;
        if state.active {
            let active_since = state.active_since.unwrap_or(now);
            if usage > restore_threshold || now.duration_since(active_since) < minimum_restraint {
                state.below_since = None;
                return Some(());
            }

            let below_since = *state.below_since.get_or_insert(now);
            if now.duration_since(below_since) < cooldown {
                return Some(());
            }
            state.active = false;
            state.below_since = None;
            state.active_since = None;
        }

        None
    }

    fn update_foreground_cpu_usage(&mut self, foreground_process_id: Option<u32>) -> Option<f32> {
        let process_id = foreground_process_id?;
        let current = process_cpu_sample(process_id).ok()?;
        let usage = self
            .foreground_cpu_sample
            .and_then(|(previous_id, previous)| {
                (previous_id == process_id)
                    .then_some(previous)
                    .and_then(|previous| process_cpu_usage_percent(previous, current))
            });
        self.foreground_cpu_sample = Some((process_id, current));
        usage
    }

    fn auto_balance_statuses(&self, now: Instant) -> Vec<AutoBalanceProcessStatus> {
        self.auto_balance
            .iter()
            .filter_map(|(process_id, process)| {
                let state = if process.active {
                    if process.below_since.is_some() {
                        AutoBalanceProcessState::CoolingDown
                    } else {
                        AutoBalanceProcessState::Restrained
                    }
                } else if process.high_since.is_some() {
                    AutoBalanceProcessState::Watching
                } else {
                    return None;
                };

                let elapsed_seconds = match state {
                    AutoBalanceProcessState::Watching => process.high_since,
                    AutoBalanceProcessState::Restrained | AutoBalanceProcessState::CoolingDown => {
                        process.active_since.or(process.below_since)
                    }
                }
                .map(|started| now.duration_since(started).as_secs());

                Some(AutoBalanceProcessStatus {
                    process_id: *process_id,
                    process_name: process.process_name.clone(),
                    state,
                    cpu_usage_tenths: process.last_usage_tenths,
                    elapsed_seconds,
                })
            })
            .collect()
    }
}

impl Drop for ForegroundResponsivenessManager {
    fn drop(&mut self) {
        let mut action_log = ActionLog::new(1);
        self.clear_all(&mut action_log, "Foreground Responsiveness manager dropped");
    }
}

impl Default for ForegroundResponsivenessSnapshot {
    fn default() -> Self {
        Self {
            enabled: false,
            scanned_processes: 0,
            background_adjusted_processes: 0,
            foreground_boosted_process: None,
            auto_balanced_processes: 0,
            auto_balance_message: "Auto-balance disabled.".to_owned(),
            auto_balance_total_cpu_usage_tenths: None,
            auto_balance_details: Vec::new(),
            skipped_processes: 0,
            failed_processes: 0,
            adjusted_apps: Vec::new(),
            message: "Foreground Responsiveness disabled.".to_owned(),
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

#[allow(dead_code)]
pub fn contains_process(list: &[String], process_name: &str) -> bool {
    list.iter()
        .any(|name| name.trim().eq_ignore_ascii_case(process_name.trim()))
}

fn percent_tenths(usage: f32) -> u16 {
    (usage.clamp(0.0, 100.0) * 10.0).round() as u16
}

fn auto_balance_status_message(
    settings: &ForegroundResponsivenessSettings,
    total_cpu_usage_percent: Option<f32>,
    running: bool,
    restrained_count: usize,
) -> String {
    if !settings.auto_balance_enabled {
        return "Auto-balance disabled.".to_owned();
    }

    if !running {
        return match total_cpu_usage_percent {
            Some(usage) => format!(
                "Waiting for foreground CPU pressure: {:.1}% of {}%.",
                usage.clamp(0.0, 100.0),
                settings.auto_balance_total_threshold_percent.min(100)
            ),
            None => "Waiting for a foreground CPU sample before auto-balance can act.".to_owned(),
        };
    }

    if restrained_count == 0 {
        return "Foreground CPU pressure is high; watching background processes for sustained spikes."
            .to_owned();
    }

    format!(
        "Restraining {restrained_count} background process{} to preserve foreground responsiveness.",
        if restrained_count == 1 { "" } else { "es" }
    )
}

fn matching_rule<'a>(
    settings: &'a ForegroundResponsivenessSettings,
    process_name: &str,
) -> Option<&'a PriorityRule> {
    settings.rules.iter().find(|rule| {
        rule.enabled
            && rule
                .process_name
                .trim()
                .eq_ignore_ascii_case(process_name.trim())
    })
}

fn should_skip_foreground_process(
    process_id: u32,
    process_name: &str,
    foreground_process_id: Option<u32>,
    foreground_process_name: Option<&str>,
) -> bool {
    foreground_process_id.is_some_and(|id| id == process_id)
        || foreground_process_name
            .is_some_and(|name| name.eq_ignore_ascii_case(process_name.trim()))
}

fn should_skip_process(
    process_id: u32,
    process_name: &str,
    current_process_id: u32,
    foreground_process_id: Option<u32>,
    foreground_process_name: Option<&str>,
    eco_qos_process_ids: &BTreeSet<u32>,
) -> bool {
    process_id == 0
        || process_id == current_process_id
        || eco_qos_process_ids.contains(&process_id)
        || is_builtin_excluded(process_name)
        || should_skip_foreground_process(
            process_id,
            process_name,
            foreground_process_id,
            foreground_process_name,
        )
}

fn auto_balance_should_run(
    settings: &ForegroundResponsivenessSettings,
    foreground_cpu_usage_percent: Option<f32>,
) -> bool {
    settings.auto_balance_enabled
        && foreground_cpu_usage_percent.is_some_and(|usage| {
            usage >= f32::from(settings.auto_balance_total_threshold_percent.min(100))
        })
}

fn auto_balance_affinity_mode(mode: EcoQosCpuRestrictionMode) -> CpuAffinityMode {
    match mode {
        EcoQosCpuRestrictionMode::SoftCpuSets => CpuAffinityMode::Soft,
        EcoQosCpuRestrictionMode::HardAffinity => CpuAffinityMode::Hard,
    }
}

fn auto_balance_core_mask(settings: &ForegroundResponsivenessSettings) -> Option<u64> {
    limited_efficiency_preferred_core_mask(
        settings.auto_balance_cpu_percent,
        settings.auto_balance_max_logical_processors,
    )
}

fn lower_background_core_mask(
    settings: &ForegroundResponsivenessSettings,
    percent: u8,
) -> Option<u64> {
    limited_efficiency_preferred_core_mask(
        percent,
        settings.lower_background_max_logical_processors,
    )
}

fn smart_background_affinity_percent(
    settings: &ForegroundResponsivenessSettings,
    foreground_cpu_usage_percent: Option<f32>,
) -> u8 {
    let maximum = settings.auto_balance_cpu_percent.clamp(1, 100);

    let threshold = f32::from(settings.auto_balance_total_threshold_percent.min(100));
    let Some(usage) = foreground_cpu_usage_percent else {
        return maximum;
    };
    if usage < threshold {
        return maximum;
    }

    let pressure = if threshold >= 100.0 {
        1.0
    } else {
        ((usage.clamp(0.0, 100.0) - threshold) / (100.0 - threshold)).clamp(0.0, 1.0)
    };
    let target = if pressure >= 0.66 {
        25
    } else if pressure >= 0.33 {
        38
    } else {
        50
    };
    maximum.min(target).max(1)
}

fn limited_efficiency_preferred_core_mask(percent: u8, max_logical_processors: u8) -> Option<u64> {
    let processors = affinity::logical_processors();
    if processors.is_empty() {
        return None;
    }

    let e_core_indices = processors
        .iter()
        .filter(|processor| processor.kind == LogicalProcessorKind::Efficiency)
        .map(|processor| processor.index)
        .collect::<Vec<_>>();
    let mut selected = if e_core_indices.is_empty() {
        processors
            .iter()
            .map(|processor| processor.index)
            .collect::<Vec<_>>()
    } else {
        e_core_indices
    };
    selected.sort_unstable();
    selected.dedup();

    logical_indices_to_limited_mask(&selected, percent, max_logical_processors)
}

fn logical_indices_to_limited_mask(
    indices: &[usize],
    percent: u8,
    max_logical_processors: u8,
) -> Option<u64> {
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

    let mut mask = 0_u64;
    for index in indices.iter().take(limit) {
        if *index < u64::BITS as usize {
            mask |= 1_u64 << index;
        }
    }
    (mask != 0).then_some(mask)
}

#[cfg(test)]
pub const fn priority_class(priority: ProcessPriority) -> u32 {
    match priority {
        ProcessPriority::Normal => NORMAL_PRIORITY_CLASS,
        ProcessPriority::BelowNormal => BELOW_NORMAL_PRIORITY_CLASS,
        ProcessPriority::Idle => IDLE_PRIORITY_CLASS,
    }
}

fn rule_process_priority(priority: ProcessPriority) -> RuleProcessPriority {
    match priority {
        ProcessPriority::Normal => RuleProcessPriority::Normal,
        ProcessPriority::BelowNormal => RuleProcessPriority::BelowNormal,
        ProcessPriority::Idle => RuleProcessPriority::Idle,
    }
}

fn priority_class_from_rule_priority(priority: RuleProcessPriority) -> Option<u32> {
    match priority {
        RuleProcessPriority::Idle => Some(IDLE_PRIORITY_CLASS),
        RuleProcessPriority::BelowNormal => Some(BELOW_NORMAL_PRIORITY_CLASS),
        RuleProcessPriority::Normal => Some(NORMAL_PRIORITY_CLASS),
        RuleProcessPriority::AboveNormal | RuleProcessPriority::High => None,
    }
}

pub const fn foreground_boost_priority_class(priority: ForegroundBoostPriority) -> u32 {
    match priority {
        ForegroundBoostPriority::Normal => NORMAL_PRIORITY_CLASS,
        ForegroundBoostPriority::AboveNormal => ABOVE_NORMAL_PRIORITY_CLASS,
    }
}

fn foreground_boost_rule_priority(priority: ForegroundBoostPriority) -> RuleProcessPriority {
    match priority {
        ForegroundBoostPriority::Normal => RuleProcessPriority::Normal,
        ForegroundBoostPriority::AboveNormal => RuleProcessPriority::AboveNormal,
    }
}

struct ForegroundBoostPriorityBackend<'a, 'name, 'log> {
    manager: &'a mut ForegroundResponsivenessManager,
    process_id: u32,
    process_name: Option<&'name str>,
    current_process_id: u32,
    current_session_id: u32,
    stability_delay_ms: u64,
    priority_class: u32,
    action_log: &'log mut ActionLog,
    last_error: Option<PriorityError>,
}

impl AppPriorityActionBackend for ForegroundBoostPriorityBackend<'_, '_, '_> {
    fn app_priority(&mut self, _app: &AppMatcher) -> Result<Option<RuleProcessPriority>, String> {
        Ok(None)
    }

    fn set_app_priority(
        &mut self,
        _app: &AppMatcher,
        priority: RuleProcessPriority,
    ) -> Result<(), String> {
        if !matches!(
            priority,
            RuleProcessPriority::Normal | RuleProcessPriority::AboveNormal
        ) {
            return Err(format!(
                "Foreground boost does not support {priority:?} priority."
            ));
        }

        match self.manager.update_foreground_boost(
            self.process_id,
            self.process_name,
            self.current_process_id,
            self.current_session_id,
            self.stability_delay_ms,
            self.priority_class,
            self.action_log,
        ) {
            Ok(()) => Ok(()),
            Err(error) => {
                let message = priority_error_message(&error);
                self.last_error = Some(error);
                Err(message)
            }
        }
    }

    fn lower_background_apps(
        &mut self,
        _priority: RuleProcessPriority,
        _exclusions: &[AppMatcher],
    ) -> Result<(), String> {
        Err("Foreground boost backend expects a foreground boost action.".to_owned())
    }
}

struct ForegroundResponsivenessPriorityBackend<'a, 'log> {
    process_id: u32,
    process_name: String,
    existing: Option<&'a AdjustedProcess>,
    action_log: &'log mut ActionLog,
    source: PriorityTargetSource,
    apply_efficiency_mode: bool,
    adjusted: Option<AdjustedProcess>,
    skipped: bool,
    last_error: Option<PriorityError>,
}

impl AppPriorityActionBackend for ForegroundResponsivenessPriorityBackend<'_, '_> {
    fn app_priority(&mut self, _app: &AppMatcher) -> Result<Option<RuleProcessPriority>, String> {
        Ok(None)
    }

    fn set_app_priority(
        &mut self,
        _app: &AppMatcher,
        priority: RuleProcessPriority,
    ) -> Result<(), String> {
        let Some(priority_class) = priority_class_from_rule_priority(priority) else {
            return Err(format!(
                "Foreground Responsiveness does not support {priority:?} background priority."
            ));
        };

        match apply_priority(
            self.process_id,
            self.process_name.clone(),
            priority_class,
            self.existing,
            self.action_log,
            self.source,
            self.apply_efficiency_mode,
        ) {
            Ok(Some(adjusted)) => {
                self.adjusted = Some(adjusted);
                Ok(())
            }
            Ok(None) => {
                self.skipped = true;
                Ok(())
            }
            Err(error) => {
                let message = priority_error_message(&error);
                self.last_error = Some(error);
                Err(message)
            }
        }
    }

    fn lower_background_apps(
        &mut self,
        _priority: RuleProcessPriority,
        _exclusions: &[AppMatcher],
    ) -> Result<(), String> {
        Err("Foreground Responsiveness backend expects per-process priority actions.".to_owned())
    }
}

fn apply_priority(
    process_id: u32,
    process_name: String,
    priority_class: u32,
    existing: Option<&AdjustedProcess>,
    action_log: &mut ActionLog,
    source: PriorityTargetSource,
    apply_efficiency_mode: bool,
) -> Result<Option<AdjustedProcess>, PriorityError> {
    let process = ProcessHandle::open(process_id)?;
    let reusable_existing =
        existing.filter(|adjusted| adjusted.process_name.eq_ignore_ascii_case(&process_name));

    if let Some(adjusted) = existing {
        if !adjusted.process_name.eq_ignore_ascii_case(&process_name) {
            restore_adjusted_process(&process, adjusted)?;
            action_log.record(
                ActionLogFeature::ForegroundResponsiveness,
                Some(process_id),
                adjusted.process_name.clone(),
                ActionLogAction::Restore,
                ActionLogResult::Restored,
                "PID now belongs to a different process: restored previous priority.",
            );
        }
    }

    let current_priority = process.priority_class()?;
    if current_priority == HIGH_PRIORITY_CLASS || current_priority == REALTIME_PRIORITY_CLASS {
        return Ok(None);
    }
    let previous_efficiency_state = if apply_efficiency_mode {
        let current_state = process.power_throttling_state().ok();
        if !current_state.is_some_and(power_throttling_execution_enabled) {
            process.set_power_throttling_state(power_throttling_enabled_state(current_state))?;
            action_log.record(
                ActionLogFeature::ForegroundResponsiveness,
                Some(process_id),
                process_name.clone(),
                ActionLogAction::Apply,
                ActionLogResult::Applied,
                "Enabled Windows Efficiency Mode for background responsiveness.",
            );
        }
        reusable_existing
            .and_then(|adjusted| adjusted.previous_efficiency_state)
            .or(current_state)
    } else {
        reusable_existing.and_then(|adjusted| adjusted.previous_efficiency_state)
    };
    if reusable_existing.is_some_and(|adjusted| {
        adjusted.applied_priority == priority_class
            && current_priority == priority_class
            && adjusted.applied_efficiency_mode == apply_efficiency_mode
    }) {
        return Ok(existing.cloned());
    }

    if current_priority != priority_class {
        process.set_priority_class(priority_class)?;
        action_log.record(
            ActionLogFeature::ForegroundResponsiveness,
            Some(process_id),
            process_name.clone(),
            ActionLogAction::Apply,
            ActionLogResult::Applied,
            format!(
                "{} set background priority to {}.",
                priority_source_label(source),
                priority_class_label(priority_class)
            ),
        );
    }

    let previous_priority = reusable_existing
        .map(|adjusted| adjusted.previous_priority)
        .unwrap_or(current_priority);

    Ok(Some(AdjustedProcess {
        process_name,
        previous_priority,
        applied_priority: priority_class,
        previous_efficiency_state,
        applied_efficiency_mode: apply_efficiency_mode,
    }))
}

fn restore_adjusted_priority(
    process_id: u32,
    process_state: AdjustedProcess,
) -> Result<(), PriorityError> {
    let process = ProcessHandle::open(process_id)?;
    restore_adjusted_process(&process, &process_state)
}

fn restore_adjusted_process(
    process: &ProcessHandle,
    process_state: &AdjustedProcess,
) -> Result<(), PriorityError> {
    let mut last_error = None;
    if process_state.applied_efficiency_mode {
        let state = process_state
            .previous_efficiency_state
            .unwrap_or_else(power_throttling_disabled_state);
        if let Err(err) = process.set_power_throttling_state(state) {
            last_error = Some(err);
        }
    }
    if let Err(err) = process.set_priority_class(process_state.previous_priority) {
        last_error = Some(err);
    }
    match last_error {
        Some(err) => Err(err),
        None => Ok(()),
    }
}

fn restore_boosted_priority(process_state: BoostedProcess) -> Result<(), PriorityError> {
    let process = ProcessHandle::open(process_state.process_id)?;
    process.set_priority_class(process_state.previous_priority)
}

fn process_session_id(process_id: u32) -> Option<u32> {
    let mut session_id = 0;
    let ok = unsafe { ProcessIdToSessionId(process_id, &mut session_id) };
    (ok != 0).then_some(session_id)
}

fn process_cpu_sample(process_id: u32) -> Result<ProcessCpuSample, PriorityError> {
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

fn power_throttling_disabled_state() -> PROCESS_POWER_THROTTLING_STATE {
    PROCESS_POWER_THROTTLING_STATE {
        Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
        StateMask: 0,
    }
}

fn power_throttling_enabled_state(
    previous: Option<PROCESS_POWER_THROTTLING_STATE>,
) -> PROCESS_POWER_THROTTLING_STATE {
    let mut state = previous.unwrap_or_else(power_throttling_disabled_state);
    state.Version = PROCESS_POWER_THROTTLING_CURRENT_VERSION;
    state.ControlMask |= PROCESS_POWER_THROTTLING_EXECUTION_SPEED;
    state.StateMask |= PROCESS_POWER_THROTTLING_EXECUTION_SPEED;
    state
}

fn power_throttling_execution_enabled(state: PROCESS_POWER_THROTTLING_STATE) -> bool {
    (state.StateMask & PROCESS_POWER_THROTTLING_EXECUTION_SPEED) != 0
}

enum PriorityError {
    AccessDenied,
    ProcessExited,
    Failed(String),
}

fn priority_error_message(error: &PriorityError) -> String {
    match error {
        PriorityError::AccessDenied => "Access denied.".to_owned(),
        PriorityError::ProcessExited => "Process exited.".to_owned(),
        PriorityError::Failed(message) => message.clone(),
    }
}

#[derive(Default)]
struct PriorityFailures {
    count: usize,
    last_error: Option<String>,
}

impl PriorityFailures {
    fn merge(&mut self, other: Self) {
        self.count += other.count;
        if self.last_error.is_none() {
            self.last_error = other.last_error;
        }
    }

    fn into_result(self) -> Result<(), PriorityError> {
        match self.last_error {
            Some(error) => Err(PriorityError::Failed(error)),
            None => Ok(()),
        }
    }

    fn record_error(
        &mut self,
        action: &str,
        process_id: u32,
        process_name: &str,
        error: PriorityError,
        action_log: &mut ActionLog,
    ) {
        let message = match error {
            PriorityError::AccessDenied => "Access denied.".to_owned(),
            PriorityError::ProcessExited => "Process exited.".to_owned(),
            PriorityError::Failed(message) => message,
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
            ActionLogFeature::ForegroundResponsiveness,
            Some(process_id),
            process_name.to_owned(),
            ActionLogAction::Fail,
            ActionLogResult::Failed,
            message,
        );
    }
}

fn process_failure_message(
    action: &str,
    process_id: u32,
    process_name: &str,
    message: &str,
) -> String {
    let name = if process_name.is_empty() {
        "process"
    } else {
        process_name
    };
    format!("{action} {name} ({process_id}): {message}")
}

fn process_failure_key(process_name: &str) -> String {
    process_name.trim().to_ascii_lowercase()
}

fn priority_source_label(source: PriorityTargetSource) -> &'static str {
    match source {
        PriorityTargetSource::BackgroundPolicy => "Background policy",
        PriorityTargetSource::Rule => "Rule",
    }
}

fn priority_class_label(priority_class: u32) -> &'static str {
    match priority_class {
        NORMAL_PRIORITY_CLASS => "Normal",
        BELOW_NORMAL_PRIORITY_CLASS => "Below Normal",
        IDLE_PRIORITY_CLASS => "Idle",
        ABOVE_NORMAL_PRIORITY_CLASS => "Above Normal",
        HIGH_PRIORITY_CLASS => "High",
        REALTIME_PRIORITY_CLASS => "Realtime",
        _ => "Unknown",
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
    fn open(process_id: u32) -> Result<Self, PriorityError> {
        let handle = unsafe {
            OpenProcess(
                PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_SET_INFORMATION,
                0,
                process_id,
            )
        };
        if !handle.is_null() {
            Ok(Self(handle))
        } else {
            Err(open_process_error(process_id, last_error()))
        }
    }

    fn open_query(process_id: u32) -> Result<Self, PriorityError> {
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
        if !handle.is_null() {
            Ok(Self(handle))
        } else {
            Err(open_process_error(process_id, last_error()))
        }
    }

    fn priority_class(&self) -> Result<u32, PriorityError> {
        let priority = unsafe { GetPriorityClass(self.0) };
        if priority == 0 {
            Err(PriorityError::Failed(format!(
                "GetPriorityClass failed with error {}.",
                last_error()
            )))
        } else {
            Ok(priority)
        }
    }

    fn set_priority_class(&self, priority_class: u32) -> Result<(), PriorityError> {
        let ok = unsafe { SetPriorityClass(self.0, priority_class) };
        if ok == 0 {
            Err(PriorityError::Failed(format!(
                "SetPriorityClass failed with error {}.",
                last_error()
            )))
        } else {
            Ok(())
        }
    }

    fn power_throttling_state(&self) -> Result<PROCESS_POWER_THROTTLING_STATE, PriorityError> {
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
            Err(PriorityError::Failed(format!(
                "GetProcessInformation ProcessPowerThrottling failed with error {}.",
                last_error()
            )))
        } else {
            Ok(state)
        }
    }

    fn set_power_throttling_state(
        &self,
        state: PROCESS_POWER_THROTTLING_STATE,
    ) -> Result<(), PriorityError> {
        let ok = unsafe {
            SetProcessInformation(
                self.0,
                ProcessPowerThrottling,
                &state as *const _ as *const c_void,
                std::mem::size_of::<PROCESS_POWER_THROTTLING_STATE>() as u32,
            )
        };
        if ok == 0 {
            Err(PriorityError::Failed(format!(
                "SetProcessInformation ProcessPowerThrottling failed with error {}.",
                last_error()
            )))
        } else {
            Ok(())
        }
    }

    fn cpu_sample(&self) -> Result<ProcessCpuSample, PriorityError> {
        let mut creation = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        let ok =
            unsafe { GetProcessTimes(self.0, &mut creation, &mut exit, &mut kernel, &mut user) };
        if ok == 0 {
            Err(PriorityError::Failed(format!(
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

fn open_process_error(process_id: u32, error: u32) -> PriorityError {
    match error {
        ERROR_ACCESS_DENIED => PriorityError::AccessDenied,
        ERROR_INVALID_PARAMETER => PriorityError::ProcessExited,
        _ => PriorityError::Failed(format!(
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
    use crate::config::ForegroundResponsivenessSettings;

    #[test]
    fn repeated_failures_suppress_future_responsiveness_attempts_once() {
        let mut manager = ForegroundResponsivenessManager::default();
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
    fn priority_mapping_uses_safe_classes() {
        assert_eq!(
            priority_class(ProcessPriority::Normal),
            NORMAL_PRIORITY_CLASS
        );
        assert_eq!(
            priority_class(ProcessPriority::BelowNormal),
            BELOW_NORMAL_PRIORITY_CLASS
        );
        assert_eq!(priority_class(ProcessPriority::Idle), IDLE_PRIORITY_CLASS);
        assert_eq!(
            foreground_boost_priority_class(ForegroundBoostPriority::AboveNormal),
            ABOVE_NORMAL_PRIORITY_CLASS
        );
    }

    #[test]
    fn process_priority_maps_to_generic_rule_priority() {
        assert_eq!(
            rule_process_priority(ProcessPriority::Normal),
            RuleProcessPriority::Normal
        );
        assert_eq!(
            rule_process_priority(ProcessPriority::BelowNormal),
            RuleProcessPriority::BelowNormal
        );
        assert_eq!(
            rule_process_priority(ProcessPriority::Idle),
            RuleProcessPriority::Idle
        );
    }

    #[test]
    fn foreground_boost_maps_to_generic_rule_priority() {
        assert_eq!(
            foreground_boost_rule_priority(ForegroundBoostPriority::Normal),
            RuleProcessPriority::Normal
        );
        assert_eq!(
            foreground_boost_rule_priority(ForegroundBoostPriority::AboveNormal),
            RuleProcessPriority::AboveNormal
        );
    }

    #[test]
    fn responsiveness_priority_backend_rejects_boost_priorities() {
        let mut log = ActionLog::new(8);
        let mut backend = ForegroundResponsivenessPriorityBackend {
            process_id: 42,
            process_name: "worker.exe".to_owned(),
            existing: None,
            action_log: &mut log,
            source: PriorityTargetSource::Rule,
            apply_efficiency_mode: true,
            adjusted: None,
            skipped: false,
            last_error: None,
        };

        assert_eq!(
            ActionExecutor.apply_app_priority_action(
                &Action::SetAppPriority {
                    app: AppMatcher::ProcessName("worker.exe".to_owned()),
                    priority: RuleProcessPriority::AboveNormal,
                },
                &mut backend,
            ),
            ActionExecution::Failed(
                "Foreground Responsiveness does not support AboveNormal background priority."
                    .to_owned()
            )
        );
        assert!(backend.adjusted.is_none());
        assert!(log.entries().is_empty());
    }

    #[test]
    fn foreground_boost_backend_rejects_non_boost_priority() {
        let mut manager = ForegroundResponsivenessManager::default();
        let mut log = ActionLog::new(8);
        let mut backend = ForegroundBoostPriorityBackend {
            manager: &mut manager,
            process_id: 42,
            process_name: Some("game.exe"),
            current_process_id: 1,
            current_session_id: 0,
            stability_delay_ms: 0,
            priority_class: ABOVE_NORMAL_PRIORITY_CLASS,
            action_log: &mut log,
            last_error: None,
        };

        assert_eq!(
            ActionExecutor.apply_app_priority_action(
                &Action::BoostForegroundPriority {
                    app: AppMatcher::ProcessName("game.exe".to_owned()),
                    priority: RuleProcessPriority::BelowNormal,
                },
                &mut backend,
            ),
            ActionExecution::Failed(
                "Foreground boost does not support BelowNormal priority.".to_owned()
            )
        );
        assert!(backend.manager.boosted.is_none());
        assert!(log.entries().is_empty());
    }

    #[test]
    fn matching_rule_is_case_insensitive() {
        let settings = ForegroundResponsivenessSettings {
            enabled: true,
            lower_background_apps: true,
            lower_background_affinity_enabled: false,
            lower_background_affinity_mode: EcoQosCpuRestrictionMode::SoftCpuSets,
            lower_background_cpu_percent: 50,
            lower_background_max_logical_processors: 0,
            lower_background_auto_cpu_percent: false,
            auto_balance_enabled: false,
            auto_balance_affinity_mode: EcoQosCpuRestrictionMode::SoftCpuSets,
            auto_balance_cpu_percent: 50,
            auto_balance_max_logical_processors: 0,
            auto_balance_total_threshold_percent: 70,
            auto_balance_threshold_percent: 25,
            auto_balance_restore_threshold_percent: 5,
            auto_balance_sustain_seconds: 2,
            auto_balance_minimum_restraint_seconds: 4,
            auto_balance_cooldown_seconds: 10,
            auto_balance_exclusions: Vec::new(),
            boost_foreground_app: false,
            foreground_boost: ForegroundBoostPriority::AboveNormal,
            foreground_stability_delay_ms: 750,
            rules: vec![PriorityRule {
                enabled: true,
                process_name: " Worker.EXE ".to_owned(),
                priority: ProcessPriority::BelowNormal,
            }],
        };

        assert!(matching_rule(&settings, "worker.exe").is_some());
        assert!(matching_rule(&settings, "other.exe").is_none());
    }

    #[test]
    fn builtin_exclusions_cover_system_shell_processes() {
        assert!(is_builtin_excluded("explorer.exe"));
        assert!(is_builtin_excluded("winlogon.exe"));
        assert!(!is_builtin_excluded("browser.exe"));
    }

    #[test]
    fn foreground_skip_matches_pid_or_process_name() {
        assert!(should_skip_foreground_process(
            42,
            "helper.exe",
            Some(42),
            Some("app.exe"),
        ));
        assert!(should_skip_foreground_process(
            99,
            "APP.EXE",
            Some(42),
            Some("app.exe"),
        ));
        assert!(!should_skip_foreground_process(
            99,
            "other.exe",
            Some(42),
            Some("app.exe"),
        ));
    }

    #[test]
    fn release_processes_skips_restore_when_process_identity_is_unknown() {
        let mut manager = ForegroundResponsivenessManager::default();
        manager.adjusted.insert(
            0,
            AdjustedProcess {
                process_name: "exited.exe".to_owned(),
                previous_priority: NORMAL_PRIORITY_CLASS,
                applied_priority: BELOW_NORMAL_PRIORITY_CLASS,
                previous_efficiency_state: None,
                applied_efficiency_mode: false,
            },
        );
        let mut log = ActionLog::new(8);

        let failures = manager.release_processes(&[0], Some(&BTreeMap::new()), &mut log, "test");

        assert_eq!(failures.count, 0);
        assert!(log.entries().is_empty());
        assert!(manager.adjusted.is_empty());
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
