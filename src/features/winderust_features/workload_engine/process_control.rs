use super::*;

pub(super) struct ApplyPriorityOutcome {
    pub(super) adjusted: Option<AdjustedProcess>,
    pub(super) skipped: bool,
    pub(super) changed: bool,
}

pub(super) fn apply_priority(
    request: ApplyPriorityRequest<'_>,
    action_log: &mut ActionLog,
) -> Result<ApplyPriorityOutcome, PriorityError> {
    let ApplyPriorityRequest {
        process_id,
        process_name,
        priority_class,
        existing,
        source,
        apply_priority_class,
        apply_background_efficiency,
        ignore_timer_resolution,
        disable_dynamic_priority_boost,
        log_success,
    } = request;
    let mut changed = false;
    let process = ProcessHandle::open(process_id)?;
    let creation_time = process.creation_time_100ns()?;
    let reusable_existing = existing
        .filter(|adjusted| adjusted.creation_time == creation_time)
        .filter(|adjusted| same_process_name(&adjusted.process_name, &process_name));

    if let Some(adjusted) = existing {
        if adjusted.creation_time == creation_time
            && !same_process_name(&adjusted.process_name, &process_name)
        {
            restore_adjusted_process(&process, adjusted)?;
            action_log.record(
                ActionLogFeature::WorkloadEngine,
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
        return Ok(ApplyPriorityOutcome {
            adjusted: None,
            skipped: true,
            changed,
        });
    }
    let previous_dynamic_priority_boost_disabled = if disable_dynamic_priority_boost {
        let current_disabled = process.dynamic_priority_boost_disabled().ok();
        if current_disabled == Some(false) {
            process.set_dynamic_priority_boost_disabled(true)?;
            changed = true;
            if log_success {
                action_log.record(
                    ActionLogFeature::WorkloadEngine,
                    Some(process_id),
                    process_name.clone(),
                    ActionLogAction::Apply,
                    ActionLogResult::Applied,
                    "Disabled Windows dynamic priority boost for Workload Engine.",
                );
            }
        }
        reusable_existing
            .and_then(|adjusted| adjusted.previous_dynamic_priority_boost_disabled)
            .or(current_disabled)
    } else {
        if let Some(adjusted) =
            reusable_existing.filter(|adjusted| adjusted.applied_dynamic_priority_boost_disabled)
        {
            if let Some(previous_disabled) = adjusted.previous_dynamic_priority_boost_disabled {
                process.set_dynamic_priority_boost_disabled(previous_disabled)?;
                changed = true;
            }
        }
        reusable_existing.and_then(|adjusted| adjusted.previous_dynamic_priority_boost_disabled)
    };
    let previous_efficiency_state = if apply_background_efficiency {
        let current_state = process.power_throttling_state().ok();
        let previous_state = reusable_existing
            .and_then(|adjusted| adjusted.previous_efficiency_state)
            .or(current_state);
        let ignore_timer_resolution_changed = reusable_existing.is_none_or(|adjusted| {
            adjusted.applied_ignore_timer_resolution != ignore_timer_resolution
        });
        let ignore_timer_resolution_missing = ignore_timer_resolution
            && !current_state.is_some_and(power_throttling_ignore_timer_resolution_enabled);
        if !current_state.is_some_and(power_throttling_execution_enabled)
            || ignore_timer_resolution_changed
            || ignore_timer_resolution_missing
        {
            process.set_power_throttling_state(power_throttling_enabled_state(
                previous_state,
                ignore_timer_resolution,
            ))?;
            changed = true;
            if log_success {
                action_log.record(
                    ActionLogFeature::WorkloadEngine,
                    Some(process_id),
                    process_name.clone(),
                    ActionLogAction::Apply,
                    ActionLogResult::Applied,
                    "Applied Background Efficiency: enabled EcoQoS.",
                );
            }
        }
        previous_state
    } else {
        if let Some(adjusted) =
            reusable_existing.filter(|adjusted| adjusted.applied_background_efficiency)
        {
            let state = adjusted
                .previous_efficiency_state
                .unwrap_or_else(power_throttling_disabled_state);
            process.set_power_throttling_state(state)?;
            changed = true;
        }
        reusable_existing.and_then(|adjusted| adjusted.previous_efficiency_state)
    };
    let mut applied_priority = current_priority;
    let mut priority_already_applied = true;
    if apply_priority_class {
        applied_priority = priority_class;
        priority_already_applied = current_priority == priority_class;
    } else if let Some(adjusted) = reusable_existing {
        if current_priority == adjusted.applied_priority
            && current_priority != adjusted.previous_priority
        {
            process.set_priority_class(adjusted.previous_priority)?;
            changed = true;
        }
        applied_priority = adjusted.previous_priority;
    }
    if reusable_existing.is_some_and(|adjusted| {
        adjusted.applied_priority == applied_priority
            && priority_already_applied
            && adjusted.applied_background_efficiency == apply_background_efficiency
            && adjusted.applied_ignore_timer_resolution == ignore_timer_resolution
            && adjusted.applied_dynamic_priority_boost_disabled == disable_dynamic_priority_boost
    }) {
        return Ok(ApplyPriorityOutcome {
            adjusted: existing.cloned(),
            skipped: false,
            changed,
        });
    }

    if apply_priority_class && current_priority != priority_class {
        process.set_priority_class(priority_class)?;
        changed = true;
        if log_success {
            action_log.record(
                ActionLogFeature::WorkloadEngine,
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
    }

    let previous_priority = reusable_existing
        .map(|adjusted| adjusted.previous_priority)
        .unwrap_or(current_priority);

    Ok(ApplyPriorityOutcome {
        adjusted: Some(AdjustedProcess {
            process_name,
            creation_time,
            previous_priority,
            applied_priority,
            previous_dynamic_priority_boost_disabled,
            applied_dynamic_priority_boost_disabled: disable_dynamic_priority_boost,
            previous_efficiency_state,
            applied_background_efficiency: apply_background_efficiency,
            applied_ignore_timer_resolution: apply_background_efficiency && ignore_timer_resolution,
        }),
        skipped: false,
        changed,
    })
}

pub(super) fn restore_adjusted_priority(
    process_id: u32,
    process_state: &AdjustedProcess,
) -> Result<(), PriorityError> {
    let process = ProcessHandle::open(process_id)?;
    if process.creation_time_100ns()? != process_state.creation_time {
        return Err(PriorityError::ProcessExited);
    }
    restore_adjusted_process(&process, process_state)
}

pub(super) fn restore_adjusted_process(
    process: &ProcessHandle,
    process_state: &AdjustedProcess,
) -> Result<(), PriorityError> {
    let mut last_error = None;
    if process_state.applied_background_efficiency {
        let state = process_state
            .previous_efficiency_state
            .unwrap_or_else(power_throttling_disabled_state);
        if let Err(err) = process.set_power_throttling_state(state) {
            last_error = Some(err);
        }
    }
    if process_state.applied_dynamic_priority_boost_disabled {
        if let Err(err) = process.set_dynamic_priority_boost_disabled(
            process_state
                .previous_dynamic_priority_boost_disabled
                .unwrap_or(false),
        ) {
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

pub(super) fn restore_boosted_priority(
    process_state: &BoostedProcess,
) -> Result<(), PriorityError> {
    let process = ProcessHandle::open(process_state.process_id)?;
    if process.creation_time_100ns()? != process_state.creation_time {
        return Err(PriorityError::ProcessExited);
    }
    process.set_priority_class(process_state.previous_priority)
}

pub(super) fn process_cpu_sample(process_id: u32) -> Result<ProcessCpuSample, PriorityError> {
    let process = ProcessHandle::open_query(process_id)?;
    process.cpu_sample()
}

pub(super) fn process_age(process_id: u32) -> Option<Duration> {
    let process = ProcessHandle::open_query(process_id).ok()?;
    let creation_time_100ns = process.creation_time_100ns().ok()?;
    let mut now = FILETIME::default();
    // SAFETY: now is writable FILETIME storage for the duration of the call.
    unsafe {
        GetSystemTimeAsFileTime(&mut now);
    }
    let age_100ns = filetime_to_u64(now).saturating_sub(creation_time_100ns);
    Some(Duration::from_nanos(age_100ns.saturating_mul(100)))
}

pub(super) fn process_group_cpu_sample(process_ids: &BTreeSet<u32>) -> Option<ProcessCpuSample> {
    let sampled_at = Instant::now();
    let mut cpu_time_100ns = 0u64;
    let mut sampled_any = false;
    for process_id in process_ids {
        let sample = match process_cpu_sample(*process_id) {
            Ok(sample) => sample,
            Err(PriorityError::ProcessExited) => continue,
            Err(PriorityError::AccessDenied | PriorityError::Failed(_)) => continue,
        };
        cpu_time_100ns = cpu_time_100ns.saturating_add(sample.cpu_time_100ns);
        sampled_any = true;
    }

    sampled_any.then_some(ProcessCpuSample {
        cpu_time_100ns,
        sampled_at,
    })
}

pub(super) fn power_throttling_disabled_state() -> PROCESS_POWER_THROTTLING_STATE {
    PROCESS_POWER_THROTTLING_STATE {
        Version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        ControlMask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
        StateMask: 0,
    }
}

pub(super) fn power_throttling_enabled_state(
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

pub(super) fn power_throttling_execution_enabled(state: PROCESS_POWER_THROTTLING_STATE) -> bool {
    (state.StateMask & PROCESS_POWER_THROTTLING_EXECUTION_SPEED) != 0
}

pub(super) fn power_throttling_ignore_timer_resolution_enabled(
    state: PROCESS_POWER_THROTTLING_STATE,
) -> bool {
    (state.StateMask & PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION) != 0
}

pub(super) fn ignore_timer_resolution_allowed(
    process_id: u32,
    active_audio_process_ids: Option<&BTreeSet<u32>>,
) -> bool {
    active_audio_process_ids.is_some_and(|ids| !ids.contains(&process_id))
}

pub(super) enum PriorityError {
    AccessDenied,
    ProcessExited,
    Failed(String),
}

pub(super) fn priority_error_message(error: &PriorityError) -> String {
    match error {
        PriorityError::AccessDenied => "Access denied.".to_owned(),
        PriorityError::ProcessExited => "Process exited.".to_owned(),
        PriorityError::Failed(message) => message.clone(),
    }
}

#[derive(Default)]
pub(super) struct PriorityFailures {
    pub(super) count: usize,
    pub(super) last_error: Option<String>,
}

impl PriorityFailures {
    pub(super) fn merge(&mut self, other: Self) {
        self.count += other.count;
        if self.last_error.is_none() {
            self.last_error = other.last_error;
        }
    }

    pub(super) fn record_error(
        &mut self,
        action: &str,
        process_id: u32,
        process_name: &str,
        error: PriorityError,
        action_log: &mut ActionLog,
    ) {
        let message = match error {
            PriorityError::AccessDenied => "Access denied.".to_owned(),
            PriorityError::ProcessExited => return,
            PriorityError::Failed(message) => message,
        };
        self.record_message(action, process_id, process_name, message, action_log);
    }

    pub(super) fn record_message(
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
            ActionLogFeature::WorkloadEngine,
            Some(process_id),
            process_name.to_owned(),
            ActionLogAction::Fail,
            ActionLogResult::Failed,
            message,
        );
    }
}

pub(super) fn process_failure_message(
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

pub(super) fn priority_source_label(source: PriorityTargetSource) -> &'static str {
    match source {
        PriorityTargetSource::WorkloadEngine => "Workload Engine",
        PriorityTargetSource::BackgroundPolicy => "Background policy",
        PriorityTargetSource::Rule => "Rule",
    }
}

pub(super) fn background_apply_summary_message(count: usize) -> String {
    if count == 1 {
        "Applied Workload Engine background restraint to 1 process.".to_owned()
    } else {
        format!("Applied Workload Engine background restraint to {count} processes.")
    }
}

pub(super) fn background_priority_restore_summary_message(count: usize, reason: &str) -> String {
    format!(
        "Restored background priority for {}: {reason}.",
        process_count_label(count)
    )
}

pub(super) fn foreground_boost_restore_summary_message(count: usize, reason: &str) -> String {
    format!(
        "Restored foreground boost for {}: {reason}.",
        process_count_label(count)
    )
}

pub(super) fn background_apply_summary_log_due(
    last_logged_at: Option<Instant>,
    now: Instant,
) -> bool {
    last_logged_at
        .is_none_or(|last| now.duration_since(last) >= BACKGROUND_APPLY_SUMMARY_LOG_INTERVAL)
}

pub(super) fn priority_class_label(priority_class: u32) -> &'static str {
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

pub(super) struct ProcessHandle(WinHandle);

impl ProcessHandle {
    pub(super) fn open(process_id: u32) -> Result<Self, PriorityError> {
        // SAFETY: process_id came from the current process snapshot and no inherited handle is
        // requested.
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

    pub(super) fn open_query(process_id: u32) -> Result<Self, PriorityError> {
        // SAFETY: process_id came from the current process snapshot and no inherited handle is
        // requested.
        let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id) };
        if !handle.is_null() {
            Ok(Self(WinHandle::new(handle)))
        } else {
            Err(open_process_error(process_id, last_error()))
        }
    }

    pub(super) fn priority_class(&self) -> Result<u32, PriorityError> {
        // SAFETY: self owns a live process handle.
        let priority = unsafe { GetPriorityClass(self.0.raw()) };
        if priority == 0 {
            Err(PriorityError::Failed(format!(
                "GetPriorityClass failed with error {}.",
                last_error()
            )))
        } else {
            Ok(priority)
        }
    }

    pub(super) fn set_priority_class(&self, priority_class: u32) -> Result<(), PriorityError> {
        // SAFETY: self owns a live process handle and priority_class is a documented class or a
        // previously read value.
        let ok = unsafe { SetPriorityClass(self.0.raw(), priority_class) };
        if ok == 0 {
            Err(PriorityError::Failed(format!(
                "SetPriorityClass failed with error {}.",
                last_error()
            )))
        } else {
            Ok(())
        }
    }

    pub(super) fn dynamic_priority_boost_disabled(&self) -> Result<bool, PriorityError> {
        let mut disabled = 0;
        // SAFETY: self owns a live process handle and disabled is writable for the call.
        let ok = unsafe { GetProcessPriorityBoost(self.0.raw(), &mut disabled) };
        if ok == 0 {
            Err(PriorityError::Failed(format!(
                "GetProcessPriorityBoost failed with error {}.",
                last_error()
            )))
        } else {
            Ok(disabled != 0)
        }
    }

    pub(super) fn set_dynamic_priority_boost_disabled(
        &self,
        disabled: bool,
    ) -> Result<(), PriorityError> {
        // SAFETY: self owns a live process handle and disabled is converted to the documented BOOL
        // representation.
        let ok = unsafe { SetProcessPriorityBoost(self.0.raw(), i32::from(disabled)) };
        if ok == 0 {
            Err(PriorityError::Failed(format!(
                "SetProcessPriorityBoost failed with error {}.",
                last_error()
            )))
        } else {
            Ok(())
        }
    }

    pub(super) fn power_throttling_state(
        &self,
    ) -> Result<PROCESS_POWER_THROTTLING_STATE, PriorityError> {
        let mut state = PROCESS_POWER_THROTTLING_STATE::default();
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
            Err(PriorityError::Failed(format!(
                "GetProcessInformation ProcessPowerThrottling failed with error {}.",
                last_error()
            )))
        } else {
            Ok(state)
        }
    }

    pub(super) fn set_power_throttling_state(
        &self,
        state: PROCESS_POWER_THROTTLING_STATE,
    ) -> Result<(), PriorityError> {
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
            Err(PriorityError::Failed(format!(
                "SetProcessInformation ProcessPowerThrottling failed with error {}.",
                last_error()
            )))
        } else {
            Ok(())
        }
    }

    pub(super) fn cpu_sample(&self) -> Result<ProcessCpuSample, PriorityError> {
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

    pub(super) fn creation_time_100ns(&self) -> Result<u64, PriorityError> {
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
            Err(PriorityError::Failed(format!(
                "GetProcessTimes failed with error {}.",
                last_error()
            )))
        } else {
            Ok(filetime_to_u64(creation))
        }
    }
}

pub(super) fn open_process_error(process_id: u32, error: u32) -> PriorityError {
    match error {
        ERROR_ACCESS_DENIED => PriorityError::AccessDenied,
        ERROR_INVALID_PARAMETER => PriorityError::ProcessExited,
        _ => PriorityError::Failed(format!(
            "OpenProcess({process_id}) failed with error {error}."
        )),
    }
}
