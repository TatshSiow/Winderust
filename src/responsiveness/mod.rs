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
            GetCurrentProcessId, GetPriorityClass, GetProcessTimes, OpenProcess, SetPriorityClass,
            ABOVE_NORMAL_PRIORITY_CLASS, BELOW_NORMAL_PRIORITY_CLASS, HIGH_PRIORITY_CLASS,
            IDLE_PRIORITY_CLASS, NORMAL_PRIORITY_CLASS, PROCESS_QUERY_LIMITED_INFORMATION,
            PROCESS_SET_INFORMATION, REALTIME_PRIORITY_CLASS,
        },
    },
};

use crate::{
    action_log::{ActionLog, ActionLogAction, ActionLogFeature, ActionLogResult},
    config::{
        ForegroundBoostPriority, ForegroundResponsivenessSettings, PriorityRule, ProcessPriority,
    },
    foreground::list_processes,
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
    pub skipped_processes: usize,
    pub failed_processes: usize,
    pub adjusted_apps: Vec<String>,
    pub message: String,
    pub last_error: Option<String>,
}

#[derive(Default)]
pub struct ForegroundResponsivenessManager {
    adjusted: BTreeMap<u32, AdjustedProcess>,
    boosted: Option<BoostedProcess>,
    foreground_candidate: Option<ForegroundCandidate>,
    auto_balance: BTreeMap<u32, AutoBalanceProcess>,
}

#[derive(Clone)]
struct AdjustedProcess {
    process_name: String,
    previous_priority: u32,
    applied_priority: u32,
}

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
    high_since: Option<Instant>,
    below_since: Option<Instant>,
    active: bool,
}

#[derive(Clone, Copy)]
struct ProcessCpuSample {
    cpu_time_100ns: u64,
    sampled_at: Instant,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum PriorityTargetSource {
    Rule,
    AutoBalance,
}

impl ForegroundResponsivenessManager {
    pub fn update(
        &mut self,
        settings: &ForegroundResponsivenessSettings,
        automation_enabled: bool,
        foreground_process_id: Option<u32>,
        eco_qos_process_ids: &BTreeSet<u32>,
        action_log: &mut ActionLog,
    ) -> ForegroundResponsivenessSnapshot {
        if !automation_enabled {
            let failed = self.clear_all(action_log, "automation disabled");
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
                && settings.foreground_boost != ForegroundBoostPriority::Normal
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

        let mut target_processes = BTreeMap::new();
        if settings.lower_background_apps {
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

                if let Some(rule) = matching_rule(settings, &process.name) {
                    target_processes.insert(
                        process.id,
                        (
                            process.name.clone(),
                            rule.priority,
                            PriorityTargetSource::Rule,
                        ),
                    );
                }
            }
        }

        if settings.auto_balance_enabled {
            let now = Instant::now();
            let current_ids = processes
                .iter()
                .map(|process| process.id)
                .collect::<BTreeSet<_>>();
            self.auto_balance
                .retain(|process_id, _| current_ids.contains(process_id));

            for process in &processes {
                if target_processes.contains_key(&process.id)
                    || should_skip_process(
                        process.id,
                        &process.name,
                        current_process_id,
                        foreground_process_id,
                        foreground_process_name.as_deref(),
                        eco_qos_process_ids,
                    )
                    || process_session_id(process.id) != Some(current_session_id)
                {
                    continue;
                }

                let target =
                    self.update_auto_balance_process(process.id, &process.name, settings, now);
                if let Some(priority) = target {
                    target_processes.insert(
                        process.id,
                        (
                            process.name.clone(),
                            priority,
                            PriorityTargetSource::AutoBalance,
                        ),
                    );
                }
            }
        } else {
            self.auto_balance.clear();
        }

        let target_ids = target_processes.keys().copied().collect::<BTreeSet<_>>();
        failures.merge(self.release_non_targets(
            &target_ids,
            &current_process_names,
            action_log,
            "process no longer matches a responsiveness rule",
        ));
        let mut skipped_processes = 0;

        let mut auto_balanced_processes = 0;
        for (process_id, (process_name, priority, source)) in target_processes {
            let failure_process_name = process_name.clone();
            if source == PriorityTargetSource::AutoBalance {
                auto_balanced_processes += 1;
            }
            match apply_priority(
                process_id,
                process_name,
                priority_class(priority),
                self.adjusted.get(&process_id),
                action_log,
                source,
            ) {
                Ok(Some(adjusted)) => {
                    self.adjusted.insert(process_id, adjusted);
                }
                Ok(None) => {
                    skipped_processes += 1;
                }
                Err(PriorityError::AccessDenied | PriorityError::ProcessExited) => {
                    skipped_processes += 1;
                    action_log.record(
                        ActionLogFeature::ForegroundResponsiveness,
                        Some(process_id),
                        failure_process_name,
                        ActionLogAction::Skip,
                        ActionLogResult::Skipped,
                        "Skipped because the process could not be opened.",
                    );
                }
                Err(PriorityError::Failed(err)) => {
                    failures.record_message(
                        "Apply",
                        process_id,
                        &failure_process_name,
                        err,
                        action_log,
                    );
                }
            }
        }

        if let Some(foreground_id) = foreground_process_id {
            if settings.boost_foreground_app
                && !eco_qos_process_ids.contains(&foreground_id)
                && !settings
                    .foreground_boost
                    .eq(&ForegroundBoostPriority::Normal)
            {
                match self.update_foreground_boost(
                    foreground_id,
                    foreground_process_name.as_deref(),
                    current_process_id,
                    current_session_id,
                    settings.foreground_stability_delay_ms,
                    foreground_boost_priority_class(settings.foreground_boost),
                    action_log,
                ) {
                    Ok(()) => {}
                    Err(PriorityError::AccessDenied | PriorityError::ProcessExited) => {
                        skipped_processes += 1;
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
                        failures.record_message(
                            "Boost",
                            foreground_id,
                            foreground_process_name.as_deref().unwrap_or(""),
                            err,
                            action_log,
                        );
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

    fn clear_all(&mut self, action_log: &mut ActionLog, reason: &str) -> PriorityFailures {
        let mut failures = self
            .clear_boosted(true, action_log, reason)
            .unwrap_or_default();
        let process_ids = self.adjusted.keys().copied().collect::<Vec<_>>();
        failures.merge(self.release_processes(&process_ids, None, action_log, reason));
        self.foreground_candidate = None;
        self.auto_balance.clear();
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
    ) -> Option<ProcessPriority> {
        let threshold = f32::from(settings.auto_balance_threshold_percent.min(100));
        let sustain = Duration::from_secs(settings.auto_balance_sustain_seconds);
        let cooldown = Duration::from_secs(settings.auto_balance_cooldown_seconds);
        let state = self
            .auto_balance
            .entry(process_id)
            .or_insert_with(|| AutoBalanceProcess {
                process_name: process_name.to_owned(),
                previous_cpu_time: None,
                high_since: None,
                below_since: None,
                active: false,
            });
        state.process_name = process_name.to_owned();

        let current = process_cpu_sample(process_id).ok()?;
        let usage = state
            .previous_cpu_time
            .and_then(|previous| process_cpu_usage_percent(previous, current));
        state.previous_cpu_time = Some(current);

        let usage = usage?;
        if usage >= threshold {
            state.below_since = None;
            let high_since = *state.high_since.get_or_insert(now);
            if state.active || now.duration_since(high_since) >= sustain {
                state.active = true;
                return Some(ProcessPriority::Idle);
            }
            return None;
        }

        state.high_since = None;
        if state.active {
            let below_since = *state.below_since.get_or_insert(now);
            if now.duration_since(below_since) < cooldown {
                return Some(ProcessPriority::BelowNormal);
            }
            state.active = false;
            state.below_since = None;
        }

        None
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

pub fn contains_process(list: &[String], process_name: &str) -> bool {
    list.iter()
        .any(|name| name.trim().eq_ignore_ascii_case(process_name.trim()))
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

pub const fn priority_class(priority: ProcessPriority) -> u32 {
    match priority {
        ProcessPriority::Normal => NORMAL_PRIORITY_CLASS,
        ProcessPriority::BelowNormal => BELOW_NORMAL_PRIORITY_CLASS,
        ProcessPriority::Idle => IDLE_PRIORITY_CLASS,
    }
}

pub const fn foreground_boost_priority_class(priority: ForegroundBoostPriority) -> u32 {
    match priority {
        ForegroundBoostPriority::Normal => NORMAL_PRIORITY_CLASS,
        ForegroundBoostPriority::AboveNormal => ABOVE_NORMAL_PRIORITY_CLASS,
    }
}

fn apply_priority(
    process_id: u32,
    process_name: String,
    priority_class: u32,
    existing: Option<&AdjustedProcess>,
    action_log: &mut ActionLog,
    source: PriorityTargetSource,
) -> Result<Option<AdjustedProcess>, PriorityError> {
    let process = ProcessHandle::open(process_id)?;
    let reusable_existing =
        existing.filter(|adjusted| adjusted.process_name.eq_ignore_ascii_case(&process_name));

    if let Some(adjusted) = existing {
        if !adjusted.process_name.eq_ignore_ascii_case(&process_name) {
            process.set_priority_class(adjusted.previous_priority)?;
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
    if reusable_existing.is_some_and(|adjusted| {
        adjusted.applied_priority == priority_class && current_priority == priority_class
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
    }))
}

fn restore_adjusted_priority(
    process_id: u32,
    process_state: AdjustedProcess,
) -> Result<(), PriorityError> {
    let process = ProcessHandle::open(process_id)?;
    process.set_priority_class(process_state.previous_priority)
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

enum PriorityError {
    AccessDenied,
    ProcessExited,
    Failed(String),
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

fn priority_source_label(source: PriorityTargetSource) -> &'static str {
    match source {
        PriorityTargetSource::Rule => "Rule",
        PriorityTargetSource::AutoBalance => "Auto-balance",
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
    fn matching_rule_is_case_insensitive() {
        let settings = ForegroundResponsivenessSettings {
            enabled: true,
            lower_background_apps: true,
            auto_balance_enabled: false,
            auto_balance_threshold_percent: 25,
            auto_balance_sustain_seconds: 2,
            auto_balance_cooldown_seconds: 10,
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
