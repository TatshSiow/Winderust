use std::collections::{BTreeMap, BTreeSet};

pub(crate) use crate::platform::windows::priority_efficiency::PowerThrottlingState;

use crate::{
    backend::crash_recovery::{
        forget_power_throttling_change, forget_priority_class_change, record_process_change,
        ProcessValue, RecoveryIntent,
    },
    config::ProcessPrioritySetting,
    foreground::ProcessActionTarget,
    platform::windows::priority_efficiency::{
        self as windows_priority_efficiency, PowerThrottlingError,
    },
    win_util::WinHandle,
};

use super::process::{
    open_process_for_set_information, ControlOwner, ProcessControlError, ProcessControlTarget,
    ProcessIdentity, ProcessTargetKey,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PriorityClassValue {
    Idle,
    BelowNormal,
    Normal,
    AboveNormal,
    High,
    Realtime,
}

impl PriorityClassValue {
    pub(crate) const fn raw(self) -> u32 {
        match self {
            Self::Idle => windows_priority_efficiency::PRIORITY_IDLE,
            Self::BelowNormal => windows_priority_efficiency::PRIORITY_BELOW_NORMAL,
            Self::Normal => windows_priority_efficiency::PRIORITY_NORMAL,
            Self::AboveNormal => windows_priority_efficiency::PRIORITY_ABOVE_NORMAL,
            Self::High => windows_priority_efficiency::PRIORITY_HIGH,
            Self::Realtime => windows_priority_efficiency::PRIORITY_REALTIME,
        }
    }

    pub(crate) const fn from_raw(raw: u32) -> Option<Self> {
        match raw {
            windows_priority_efficiency::PRIORITY_IDLE => Some(Self::Idle),
            windows_priority_efficiency::PRIORITY_BELOW_NORMAL => Some(Self::BelowNormal),
            windows_priority_efficiency::PRIORITY_NORMAL => Some(Self::Normal),
            windows_priority_efficiency::PRIORITY_ABOVE_NORMAL => Some(Self::AboveNormal),
            windows_priority_efficiency::PRIORITY_HIGH => Some(Self::High),
            windows_priority_efficiency::PRIORITY_REALTIME => Some(Self::Realtime),
            _ => None,
        }
    }

    pub(crate) const fn from_setting(setting: ProcessPrioritySetting) -> Option<Self> {
        match setting {
            ProcessPrioritySetting::Idle => Some(Self::Idle),
            ProcessPrioritySetting::BelowNormal => Some(Self::BelowNormal),
            ProcessPrioritySetting::Normal => Some(Self::Normal),
            ProcessPrioritySetting::AboveNormal => Some(Self::AboveNormal),
            ProcessPrioritySetting::High => Some(Self::High),
            ProcessPrioritySetting::Realtime => Some(Self::Realtime),
            ProcessPrioritySetting::Default => None,
        }
    }

    pub(crate) const fn setting(self) -> ProcessPrioritySetting {
        match self {
            Self::Idle => ProcessPrioritySetting::Idle,
            Self::BelowNormal => ProcessPrioritySetting::BelowNormal,
            Self::Normal => ProcessPrioritySetting::Normal,
            Self::AboveNormal => ProcessPrioritySetting::AboveNormal,
            Self::High => ProcessPrioritySetting::High,
            Self::Realtime => ProcessPrioritySetting::Realtime,
        }
    }

    pub(crate) const fn label(self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::BelowNormal => "Below Normal",
            Self::Normal => "Normal",
            Self::AboveNormal => "Above Normal",
            Self::High => "High",
            Self::Realtime => "Realtime",
        }
    }

    const fn rank(self) -> i32 {
        match self {
            Self::Idle => 0,
            Self::BelowNormal => 1,
            Self::Normal => 2,
            Self::AboveNormal => 3,
            Self::High => 4,
            Self::Realtime => 5,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PriorityClassPreservation {
    Exact,
    PreserveHigher,
    PreserveLower,
    PreserveHighOrRealtime,
    PreserveLowerOrHighOrRealtime,
    PreserveHigherOrHighOrRealtime,
}

#[derive(Debug, Clone)]
pub(crate) struct PriorityClassClaim {
    pub(crate) target: ProcessControlTarget,
    pub(crate) owner: ControlOwner,
    pub(crate) priority: PriorityClassValue,
    pub(crate) preservation: PriorityClassPreservation,
}

#[derive(Debug, Clone)]
pub(crate) struct PowerThrottlingClaim {
    pub(crate) target: ProcessControlTarget,
    pub(crate) owner: ControlOwner,
    pub(crate) ignore_timer_resolution: bool,
}

#[derive(Debug, Clone)]
pub(crate) struct EfficiencyModeClaim {
    pub(crate) target: ProcessControlTarget,
    pub(crate) owner: ControlOwner,
    pub(crate) ignore_timer_resolution: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ProcessPropertyApplyOutcome {
    Applied,
    Unchanged,
    Preserved,
    Shadowed,
}

#[derive(Debug)]
pub(crate) struct PriorityEfficiencyReleaseFailure {
    pub(crate) process_id: u32,
    pub(crate) process_name: String,
    pub(crate) executable_path: String,
    pub(crate) property: &'static str,
    pub(crate) error: ProcessControlError,
}

#[derive(Debug, Default)]
pub(crate) struct PriorityEfficiencyReleaseSummary {
    pub(crate) restored_processes: usize,
    pub(crate) failures: Vec<PriorityEfficiencyReleaseFailure>,
}

impl PowerThrottlingState {
    pub(crate) fn execution_speed_enabled(self) -> bool {
        self.control_mask & windows_priority_efficiency::POWER_EXECUTION_SPEED != 0
            && self.state_mask & windows_priority_efficiency::POWER_EXECUTION_SPEED != 0
    }

    pub(crate) fn ignore_timer_resolution_enabled(self) -> bool {
        self.control_mask & windows_priority_efficiency::POWER_IGNORE_TIMER_RESOLUTION != 0
            && self.state_mask & windows_priority_efficiency::POWER_IGNORE_TIMER_RESOLUTION != 0
    }

    fn with_efficiency_enabled(self, ignore_timer_resolution: bool) -> Self {
        let previous_ignore = self.ignore_timer_resolution_enabled();
        let mut state = self;
        state.version = windows_priority_efficiency::POWER_CURRENT_VERSION;
        state.control_mask |= windows_priority_efficiency::POWER_EXECUTION_SPEED
            | windows_priority_efficiency::POWER_IGNORE_TIMER_RESOLUTION;
        state.state_mask |= windows_priority_efficiency::POWER_EXECUTION_SPEED;
        if ignore_timer_resolution || previous_ignore {
            state.state_mask |= windows_priority_efficiency::POWER_IGNORE_TIMER_RESOLUTION;
        } else {
            state.state_mask &= !windows_priority_efficiency::POWER_IGNORE_TIMER_RESOLUTION;
        }
        state
    }

    fn with_process_list_efficiency_enabled(self) -> Self {
        let mut state = self;
        state.version = windows_priority_efficiency::POWER_CURRENT_VERSION;
        state.control_mask |= windows_priority_efficiency::POWER_EXECUTION_SPEED;
        state.state_mask |= windows_priority_efficiency::POWER_EXECUTION_SPEED;
        state
    }

    fn with_process_list_efficiency_disabled(self) -> Self {
        let mut state = self;
        state.version = windows_priority_efficiency::POWER_CURRENT_VERSION;
        state.control_mask |= windows_priority_efficiency::POWER_EXECUTION_SPEED;
        state.state_mask &= !windows_priority_efficiency::POWER_EXECUTION_SPEED;
        state
    }
}

#[derive(Clone)]
struct ManagedValue<T> {
    baseline: T,
    expected: T,
    owner: ControlOwner,
    apply_sequence: u64,
}

pub(crate) trait PriorityEfficiencyRecoveryIntent {
    fn commit(self) -> Result<(), String>;
}

impl PriorityEfficiencyRecoveryIntent for RecoveryIntent {
    fn commit(self) -> Result<(), String> {
        RecoveryIntent::commit(self)
    }
}

pub(crate) trait PriorityEfficiencyPlatform {
    type Process;
    type RecoveryIntent: PriorityEfficiencyRecoveryIntent;

    fn open(
        &mut self,
        target: &ProcessControlTarget,
        allow_cross_session_process_control: bool,
    ) -> Result<(ProcessIdentity, Self::Process), ProcessControlError>;
    fn query_priority(&mut self, process: &Self::Process) -> Result<u32, ProcessControlError>;
    fn query_power(
        &mut self,
        process: &Self::Process,
    ) -> Result<PowerThrottlingState, ProcessControlError>;
    fn begin_priority_change(
        &mut self,
        process: &Self::Process,
        original: u32,
        expected: u32,
    ) -> Result<Self::RecoveryIntent, ProcessControlError>;
    fn begin_power_change(
        &mut self,
        process: &Self::Process,
        original: PowerThrottlingState,
        expected: PowerThrottlingState,
    ) -> Result<Self::RecoveryIntent, ProcessControlError>;
    fn apply_priority(
        &mut self,
        process: &Self::Process,
        priority: u32,
    ) -> Result<(), ProcessControlError>;
    fn apply_power(
        &mut self,
        process: &Self::Process,
        state: PowerThrottlingState,
    ) -> Result<(), ProcessControlError>;
    fn relinquish_priority(
        &mut self,
        identity: &ProcessIdentity,
    ) -> Result<(), ProcessControlError>;
    fn relinquish_power(&mut self, identity: &ProcessIdentity) -> Result<(), ProcessControlError>;
}

#[derive(Default)]
pub(crate) struct WindowsPriorityEfficiencyPlatform;

impl PriorityEfficiencyPlatform for WindowsPriorityEfficiencyPlatform {
    type Process = WinHandle;
    type RecoveryIntent = RecoveryIntent;

    fn open(
        &mut self,
        target: &ProcessControlTarget,
        allow_cross_session_process_control: bool,
    ) -> Result<(ProcessIdentity, Self::Process), ProcessControlError> {
        open_process_for_set_information(target, allow_cross_session_process_control)
    }

    fn query_priority(&mut self, process: &Self::Process) -> Result<u32, ProcessControlError> {
        windows_priority_efficiency::query_priority(process).map_err(ProcessControlError::from)
    }

    fn query_power(
        &mut self,
        process: &Self::Process,
    ) -> Result<PowerThrottlingState, ProcessControlError> {
        windows_priority_efficiency::query_power(process).map_err(power_throttling_error)
    }

    fn begin_priority_change(
        &mut self,
        process: &Self::Process,
        original: u32,
        expected: u32,
    ) -> Result<Self::RecoveryIntent, ProcessControlError> {
        record_process_change(
            process.raw(),
            ProcessValue::PriorityClass(original),
            ProcessValue::PriorityClass(expected),
        )
        .map_err(ProcessControlError::Failed)
    }

    fn begin_power_change(
        &mut self,
        process: &Self::Process,
        original: PowerThrottlingState,
        expected: PowerThrottlingState,
    ) -> Result<Self::RecoveryIntent, ProcessControlError> {
        record_process_change(
            process.raw(),
            ProcessValue::PowerThrottling {
                version: original.version,
                control_mask: original.control_mask,
                state_mask: original.state_mask,
            },
            ProcessValue::PowerThrottling {
                version: expected.version,
                control_mask: expected.control_mask,
                state_mask: expected.state_mask,
            },
        )
        .map_err(ProcessControlError::Failed)
    }

    fn apply_priority(
        &mut self,
        process: &Self::Process,
        priority: u32,
    ) -> Result<(), ProcessControlError> {
        windows_priority_efficiency::set_priority(process, priority)
            .map_err(ProcessControlError::from)
    }

    fn apply_power(
        &mut self,
        process: &Self::Process,
        state: PowerThrottlingState,
    ) -> Result<(), ProcessControlError> {
        windows_priority_efficiency::set_power(process, state).map_err(power_throttling_error)
    }

    fn relinquish_priority(
        &mut self,
        identity: &ProcessIdentity,
    ) -> Result<(), ProcessControlError> {
        forget_priority_class_change(identity.id, identity.creation_time)
            .map_err(ProcessControlError::Failed)
    }

    fn relinquish_power(&mut self, identity: &ProcessIdentity) -> Result<(), ProcessControlError> {
        forget_power_throttling_change(identity.id, identity.creation_time)
            .map_err(ProcessControlError::Failed)
    }
}

pub(crate) struct PriorityEfficiencyController<
    P: PriorityEfficiencyPlatform = WindowsPriorityEfficiencyPlatform,
> {
    platform: P,
    priority_claims: BTreeMap<ControlOwner, BTreeMap<ProcessTargetKey, PriorityClassClaim>>,
    power_claims: BTreeMap<ControlOwner, BTreeMap<ProcessTargetKey, PowerThrottlingClaim>>,
    managed_priorities: BTreeMap<ProcessIdentity, ManagedValue<u32>>,
    managed_power: BTreeMap<ProcessIdentity, ManagedValue<PowerThrottlingState>>,
    next_apply_sequence: u64,
}

impl Default for PriorityEfficiencyController {
    fn default() -> Self {
        Self::with_platform(WindowsPriorityEfficiencyPlatform)
    }
}

impl<P: PriorityEfficiencyPlatform> PriorityEfficiencyController<P> {
    fn with_platform(platform: P) -> Self {
        Self {
            platform,
            priority_claims: BTreeMap::new(),
            power_claims: BTreeMap::new(),
            managed_priorities: BTreeMap::new(),
            managed_power: BTreeMap::new(),
            next_apply_sequence: 1,
        }
    }

    fn next_sequence(&mut self) -> u64 {
        let sequence = self.next_apply_sequence;
        self.next_apply_sequence = self.next_apply_sequence.wrapping_add(1).max(1);
        sequence
    }

    pub(crate) fn apply_priority_claim(
        &mut self,
        claim: PriorityClassClaim,
        allow_cross_session_process_control: bool,
    ) -> Result<ProcessPropertyApplyOutcome, ProcessControlError> {
        validate_priority_owner(claim.owner)?;
        let owner = claim.owner;
        let key = claim.target.key();
        let previous = self
            .priority_claims
            .entry(owner)
            .or_default()
            .insert(key.clone(), claim);
        let Some(effective) = self.effective_priority_claim(&key).cloned() else {
            return Err(ProcessControlError::Failed(
                "Process Priority claim arbitration failed.".to_owned(),
            ));
        };
        if effective.owner != owner {
            return Ok(ProcessPropertyApplyOutcome::Shadowed);
        }
        let result = self.apply_effective_priority(effective, allow_cross_session_process_control);
        if result.is_err() {
            restore_claim(&mut self.priority_claims, owner, key, previous);
        }
        result
    }

    pub(crate) fn apply_power_claim(
        &mut self,
        claim: PowerThrottlingClaim,
        allow_cross_session_process_control: bool,
    ) -> Result<ProcessPropertyApplyOutcome, ProcessControlError> {
        validate_power_owner(claim.owner)?;
        let owner = claim.owner;
        let key = claim.target.key();
        let previous = self
            .power_claims
            .entry(owner)
            .or_default()
            .insert(key.clone(), claim);
        let Some(effective) = self.effective_power_claim(&key).cloned() else {
            return Err(ProcessControlError::Failed(
                "process power-throttling claim arbitration failed.".to_owned(),
            ));
        };
        if effective.owner != owner {
            return Ok(ProcessPropertyApplyOutcome::Shadowed);
        }
        let result = self.apply_effective_power(effective, allow_cross_session_process_control);
        if result.is_err() {
            restore_claim(&mut self.power_claims, owner, key, previous);
        }
        result
    }

    pub(crate) fn apply_efficiency_claim(
        &mut self,
        claim: EfficiencyModeClaim,
        allow_cross_session_process_control: bool,
    ) -> Result<ProcessPropertyApplyOutcome, ProcessControlError> {
        if claim.owner != ControlOwner::BackgroundEfficiency {
            return Err(ProcessControlError::Failed(
                "Only Background Efficiency can submit an automatic Efficiency Mode claim."
                    .to_owned(),
            ));
        }
        let key = claim.target.key();
        let priority_claim = PriorityClassClaim {
            target: claim.target.clone(),
            owner: claim.owner,
            priority: PriorityClassValue::Idle,
            preservation: PriorityClassPreservation::Exact,
        };
        let power_claim = PowerThrottlingClaim {
            target: claim.target,
            owner: claim.owner,
            ignore_timer_resolution: claim.ignore_timer_resolution,
        };
        let previous_priority = self
            .priority_claims
            .entry(claim.owner)
            .or_default()
            .insert(key.clone(), priority_claim.clone());
        let previous_power = self
            .power_claims
            .entry(claim.owner)
            .or_default()
            .insert(key.clone(), power_claim.clone());
        let is_effective = self
            .effective_priority_claim(&key)
            .is_some_and(|effective| effective.owner == claim.owner)
            && self
                .effective_power_claim(&key)
                .is_some_and(|effective| effective.owner == claim.owner);
        if !is_effective {
            return Ok(ProcessPropertyApplyOutcome::Shadowed);
        }
        let result = self.apply_compound(
            priority_claim,
            power_claim,
            allow_cross_session_process_control,
        );
        if result.is_err() {
            restore_claim(
                &mut self.priority_claims,
                claim.owner,
                key.clone(),
                previous_priority,
            );
            restore_claim(&mut self.power_claims, claim.owner, key, previous_power);
        }
        result
    }

    pub(crate) fn apply_process_list_priority(
        &mut self,
        target: &ProcessActionTarget,
        priority: ProcessPrioritySetting,
        allow_cross_session_process_control: bool,
    ) -> Result<ProcessPropertyApplyOutcome, ProcessControlError> {
        let priority = PriorityClassValue::from_setting(priority).filter(|priority| {
            matches!(
                priority,
                PriorityClassValue::Idle
                    | PriorityClassValue::BelowNormal
                    | PriorityClassValue::Normal
                    | PriorityClassValue::AboveNormal
            )
        });
        let priority = priority.ok_or_else(|| {
            ProcessControlError::Unavailable(
                "This priority is not available as a Process List action.".to_owned(),
            )
        })?;
        self.apply_effective_priority(
            PriorityClassClaim {
                target: ProcessControlTarget::from_action_target(target),
                owner: ControlOwner::ProcessList,
                priority,
                preservation: PriorityClassPreservation::Exact,
            },
            allow_cross_session_process_control,
        )
    }

    pub(crate) fn apply_process_list_efficiency_mode(
        &mut self,
        target: &ProcessActionTarget,
        enabled: bool,
        allow_cross_session_process_control: bool,
    ) -> Result<ProcessPropertyApplyOutcome, ProcessControlError> {
        let control_target = ProcessControlTarget::from_action_target(target);
        let (identity, process) = self
            .platform
            .open(&control_target, allow_cross_session_process_control)?;
        self.relinquish_stale_priority_identities(&identity)?;
        self.relinquish_stale_power_identities(&identity)?;
        let current_priority = self.platform.query_priority(&process)?;
        if current_priority == windows_priority_efficiency::PRIORITY_REALTIME {
            return Err(ProcessControlError::AccessDenied(
                "Realtime processes are not changed by Process List actions.".to_owned(),
            ));
        }
        let current_power = self.platform.query_power(&process)?;
        self.rebase_broken_priority(&identity, current_priority)?;
        self.rebase_broken_power(&identity, current_power)?;

        let priority_baseline = self
            .managed_priorities
            .get(&identity)
            .map_or(current_priority, |managed| managed.baseline);
        let power_baseline = self
            .managed_power
            .get(&identity)
            .map_or(current_power, |managed| managed.baseline);
        let desired_priority = if enabled {
            windows_priority_efficiency::PRIORITY_IDLE
        } else if self
            .managed_priorities
            .get(&identity)
            .is_some_and(|managed| managed.owner == ControlOwner::ProcessList)
        {
            priority_baseline
        } else {
            windows_priority_efficiency::PRIORITY_NORMAL
        };
        let desired_power = if enabled {
            power_baseline.with_process_list_efficiency_enabled()
        } else if self
            .managed_power
            .get(&identity)
            .is_some_and(|managed| managed.owner == ControlOwner::ProcessList)
        {
            power_baseline
        } else {
            current_power.with_process_list_efficiency_disabled()
        };

        self.apply_compound_values(
            identity,
            process,
            current_priority,
            current_power,
            desired_priority,
            desired_power,
            ControlOwner::ProcessList,
        )
    }

    fn apply_compound(
        &mut self,
        priority_claim: PriorityClassClaim,
        power_claim: PowerThrottlingClaim,
        allow_cross_session_process_control: bool,
    ) -> Result<ProcessPropertyApplyOutcome, ProcessControlError> {
        let (identity, process) = self
            .platform
            .open(&priority_claim.target, allow_cross_session_process_control)?;
        self.relinquish_stale_priority_identities(&identity)?;
        self.relinquish_stale_power_identities(&identity)?;
        let current_priority = self.platform.query_priority(&process)?;
        let current_power = self.platform.query_power(&process)?;
        self.rebase_broken_priority(&identity, current_priority)?;
        self.rebase_broken_power(&identity, current_power)?;
        let power_baseline = self
            .managed_power
            .get(&identity)
            .map_or(current_power, |managed| managed.baseline);
        let desired_priority = priority_claim.priority.raw();
        let desired_power =
            power_baseline.with_efficiency_enabled(power_claim.ignore_timer_resolution);
        self.apply_compound_values(
            identity,
            process,
            current_priority,
            current_power,
            desired_priority,
            desired_power,
            priority_claim.owner,
        )
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "the compound transition keeps both observed and desired property values explicit"
    )]
    fn apply_compound_values(
        &mut self,
        identity: ProcessIdentity,
        process: P::Process,
        current_priority: u32,
        current_power: PowerThrottlingState,
        desired_priority: u32,
        desired_power: PowerThrottlingState,
        owner: ControlOwner,
    ) -> Result<ProcessPropertyApplyOutcome, ProcessControlError> {
        let previous_priority_managed = self.managed_priorities.get(&identity).cloned();
        let previous_power_managed = self.managed_power.get(&identity).cloned();
        let priority_baseline = previous_priority_managed
            .as_ref()
            .map_or(current_priority, |managed| managed.baseline);
        let power_baseline = previous_power_managed
            .as_ref()
            .map_or(current_power, |managed| managed.baseline);

        let power_changed = current_power != desired_power;
        if power_changed {
            let sequence = self.next_sequence();
            match apply_power_transition(
                &mut self.platform,
                &process,
                current_power,
                desired_power,
                CommitFailureBehavior::Compensate,
            ) {
                Ok(()) => {
                    self.managed_power.insert(
                        identity.clone(),
                        ManagedValue {
                            baseline: power_baseline,
                            expected: desired_power,
                            owner,
                            apply_sequence: sequence,
                        },
                    );
                }
                Err(failure) => {
                    self.finish_power_failure(
                        identity,
                        power_baseline,
                        desired_power,
                        owner,
                        sequence,
                        previous_power_managed,
                        failure,
                    )?;
                    unreachable!("finish_power_failure always returns an error")
                }
            }
        } else if let Some(mut managed) = previous_power_managed.clone() {
            managed.owner = owner;
            managed.expected = desired_power;
            self.managed_power.insert(identity.clone(), managed);
        }

        let priority_changed = current_priority != desired_priority;
        let priority_result = if priority_changed {
            let sequence = self.next_sequence();
            match apply_priority_transition(
                &mut self.platform,
                &process,
                current_priority,
                desired_priority,
                CommitFailureBehavior::Compensate,
            ) {
                Ok(()) => {
                    self.managed_priorities.insert(
                        identity.clone(),
                        ManagedValue {
                            baseline: priority_baseline,
                            expected: desired_priority,
                            owner,
                            apply_sequence: sequence,
                        },
                    );
                    Ok(())
                }
                Err(failure) => self.finish_priority_failure(
                    identity.clone(),
                    priority_baseline,
                    desired_priority,
                    owner,
                    sequence,
                    previous_priority_managed.clone(),
                    failure,
                ),
            }
        } else {
            if let Some(mut managed) = previous_priority_managed.clone() {
                managed.owner = owner;
                managed.expected = desired_priority;
                self.managed_priorities.insert(identity.clone(), managed);
            }
            Ok(())
        };

        if let Err(priority_error) = priority_result {
            if power_changed {
                let rollback = apply_power_transition(
                    &mut self.platform,
                    &process,
                    desired_power,
                    current_power,
                    CommitFailureBehavior::Compensate,
                );
                match rollback {
                    Ok(()) => restore_managed_snapshot(
                        &mut self.managed_power,
                        identity,
                        previous_power_managed,
                    ),
                    Err(failure) => {
                        return Err(ProcessControlError::Failed(format!(
                            "{priority_error} Efficiency Mode rollback also failed: {}",
                            failure.message
                        )));
                    }
                }
            } else {
                restore_managed_snapshot(&mut self.managed_power, identity, previous_power_managed);
            }
            return Err(priority_error);
        }

        Ok(if power_changed || priority_changed {
            ProcessPropertyApplyOutcome::Applied
        } else {
            ProcessPropertyApplyOutcome::Unchanged
        })
    }

    fn apply_effective_priority(
        &mut self,
        claim: PriorityClassClaim,
        allow_cross_session_process_control: bool,
    ) -> Result<ProcessPropertyApplyOutcome, ProcessControlError> {
        let (identity, process) = self
            .platform
            .open(&claim.target, allow_cross_session_process_control)?;
        self.relinquish_stale_priority_identities(&identity)?;
        let current = self.platform.query_priority(&process)?;
        self.rebase_broken_priority(&identity, current)?;
        let managed = self.managed_priorities.remove(&identity);
        let baseline = managed.as_ref().map_or(current, |managed| managed.baseline);
        let desired = claim.priority.raw();
        if priority_is_preserved(claim.preservation, baseline, desired) {
            if let Some(managed) = managed {
                self.managed_priorities.insert(identity.clone(), managed);
                self.release_priority_identity(&identity)?;
                self.managed_priorities.remove(&identity);
            }
            return Ok(ProcessPropertyApplyOutcome::Preserved);
        }
        if current == desired {
            if let Some(mut managed) = managed {
                if claim.owner.is_automatic() || managed.owner == ControlOwner::ProcessList {
                    managed.owner = claim.owner;
                }
                managed.expected = current;
                self.managed_priorities.insert(identity, managed);
            }
            return Ok(ProcessPropertyApplyOutcome::Unchanged);
        }

        let sequence = self.next_sequence();
        match apply_priority_transition(
            &mut self.platform,
            &process,
            current,
            desired,
            CommitFailureBehavior::Compensate,
        ) {
            Ok(()) => {
                self.managed_priorities.insert(
                    identity,
                    ManagedValue {
                        baseline,
                        expected: desired,
                        owner: claim.owner,
                        apply_sequence: sequence,
                    },
                );
                Ok(ProcessPropertyApplyOutcome::Applied)
            }
            Err(failure) => self
                .finish_priority_failure(
                    identity,
                    baseline,
                    desired,
                    claim.owner,
                    sequence,
                    managed,
                    failure,
                )
                .map(|_| ProcessPropertyApplyOutcome::Applied),
        }
    }

    fn apply_effective_power(
        &mut self,
        claim: PowerThrottlingClaim,
        allow_cross_session_process_control: bool,
    ) -> Result<ProcessPropertyApplyOutcome, ProcessControlError> {
        let (identity, process) = self
            .platform
            .open(&claim.target, allow_cross_session_process_control)?;
        self.relinquish_stale_power_identities(&identity)?;
        let current = self.platform.query_power(&process)?;
        self.rebase_broken_power(&identity, current)?;
        let managed = self.managed_power.remove(&identity);
        let baseline = managed.as_ref().map_or(current, |managed| managed.baseline);
        let desired = baseline.with_efficiency_enabled(claim.ignore_timer_resolution);
        if current == desired {
            if let Some(mut managed) = managed {
                if claim.owner.is_automatic() || managed.owner == ControlOwner::ProcessList {
                    managed.owner = claim.owner;
                }
                managed.expected = current;
                self.managed_power.insert(identity, managed);
            }
            return Ok(ProcessPropertyApplyOutcome::Unchanged);
        }

        let sequence = self.next_sequence();
        match apply_power_transition(
            &mut self.platform,
            &process,
            current,
            desired,
            CommitFailureBehavior::Compensate,
        ) {
            Ok(()) => {
                self.managed_power.insert(
                    identity,
                    ManagedValue {
                        baseline,
                        expected: desired,
                        owner: claim.owner,
                        apply_sequence: sequence,
                    },
                );
                Ok(ProcessPropertyApplyOutcome::Applied)
            }
            Err(failure) => self
                .finish_power_failure(
                    identity,
                    baseline,
                    desired,
                    claim.owner,
                    sequence,
                    managed,
                    failure,
                )
                .map(|_| ProcessPropertyApplyOutcome::Applied),
        }
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "failure handling must retain the exact baseline, expected value, owner, and prior ledger entry"
    )]
    fn finish_priority_failure(
        &mut self,
        identity: ProcessIdentity,
        baseline: u32,
        desired: u32,
        owner: ControlOwner,
        sequence: u64,
        previous: Option<ManagedValue<u32>>,
        mut failure: TransitionFailure,
    ) -> Result<(), ProcessControlError> {
        if failure.relinquish_recovery {
            if let Err(error) = self.platform.relinquish_priority(&identity) {
                failure
                    .message
                    .push_str(&format!(" Recovery journal relinquish failed: {error}."));
                failure.uncertain = true;
            }
        }
        if failure.uncertain {
            self.managed_priorities.insert(
                identity,
                ManagedValue {
                    baseline,
                    expected: desired,
                    owner,
                    apply_sequence: sequence,
                },
            );
        } else if let Some(previous) = previous {
            self.managed_priorities.insert(identity, previous);
        }
        Err(ProcessControlError::Failed(failure.message))
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "failure handling must retain the exact baseline, expected value, owner, and prior ledger entry"
    )]
    fn finish_power_failure(
        &mut self,
        identity: ProcessIdentity,
        baseline: PowerThrottlingState,
        desired: PowerThrottlingState,
        owner: ControlOwner,
        sequence: u64,
        previous: Option<ManagedValue<PowerThrottlingState>>,
        mut failure: TransitionFailure,
    ) -> Result<(), ProcessControlError> {
        if failure.relinquish_recovery {
            if let Err(error) = self.platform.relinquish_power(&identity) {
                failure
                    .message
                    .push_str(&format!(" Recovery journal relinquish failed: {error}."));
                failure.uncertain = true;
            }
        }
        if failure.uncertain {
            self.managed_power.insert(
                identity,
                ManagedValue {
                    baseline,
                    expected: desired,
                    owner,
                    apply_sequence: sequence,
                },
            );
        } else if let Some(previous) = previous {
            self.managed_power.insert(identity, previous);
        }
        Err(ProcessControlError::Failed(failure.message))
    }

    fn rebase_broken_priority(
        &mut self,
        identity: &ProcessIdentity,
        current: u32,
    ) -> Result<(), ProcessControlError> {
        if self
            .managed_priorities
            .get(identity)
            .is_some_and(|managed| managed.expected != current)
        {
            self.platform.relinquish_priority(identity)?;
            self.managed_priorities.remove(identity);
        }
        Ok(())
    }

    fn rebase_broken_power(
        &mut self,
        identity: &ProcessIdentity,
        current: PowerThrottlingState,
    ) -> Result<(), ProcessControlError> {
        if self
            .managed_power
            .get(identity)
            .is_some_and(|managed| managed.expected != current)
        {
            self.platform.relinquish_power(identity)?;
            self.managed_power.remove(identity);
        }
        Ok(())
    }

    fn relinquish_stale_priority_identities(
        &mut self,
        identity: &ProcessIdentity,
    ) -> Result<(), ProcessControlError> {
        let stale = self
            .managed_priorities
            .keys()
            .filter(|candidate| candidate.id == identity.id && **candidate != *identity)
            .cloned()
            .collect::<Vec<_>>();
        for stale_identity in stale {
            self.platform.relinquish_priority(&stale_identity)?;
            self.managed_priorities.remove(&stale_identity);
        }
        Ok(())
    }

    fn relinquish_stale_power_identities(
        &mut self,
        identity: &ProcessIdentity,
    ) -> Result<(), ProcessControlError> {
        let stale = self
            .managed_power
            .keys()
            .filter(|candidate| candidate.id == identity.id && **candidate != *identity)
            .cloned()
            .collect::<Vec<_>>();
        for stale_identity in stale {
            self.platform.relinquish_power(&stale_identity)?;
            self.managed_power.remove(&stale_identity);
        }
        Ok(())
    }

    pub(crate) fn release_priority_policy_except(
        &mut self,
        owner: ControlOwner,
        active_targets: &BTreeSet<ProcessTargetKey>,
    ) -> PriorityEfficiencyReleaseSummary {
        let stale_keys = stale_claim_keys(&self.priority_claims, owner, active_targets);
        remove_claim_keys(&mut self.priority_claims, owner, &stale_keys);
        let mut summary = PriorityEfficiencyReleaseSummary::default();
        for key in stale_keys {
            self.reconcile_priority_key(owner, &key, &mut summary);
        }
        summary
    }

    pub(crate) fn release_power_policy_except(
        &mut self,
        owner: ControlOwner,
        active_targets: &BTreeSet<ProcessTargetKey>,
    ) -> PriorityEfficiencyReleaseSummary {
        let stale_keys = stale_claim_keys(&self.power_claims, owner, active_targets);
        remove_claim_keys(&mut self.power_claims, owner, &stale_keys);
        let mut summary = PriorityEfficiencyReleaseSummary::default();
        for key in stale_keys {
            self.reconcile_power_key(owner, &key, &mut summary);
        }
        summary
    }

    pub(crate) fn release_efficiency_policy_except(
        &mut self,
        owner: ControlOwner,
        active_targets: &BTreeSet<ProcessTargetKey>,
    ) -> PriorityEfficiencyReleaseSummary {
        let mut stale_keys = stale_claim_keys(&self.priority_claims, owner, active_targets);
        stale_keys.extend(stale_claim_keys(&self.power_claims, owner, active_targets));
        remove_claim_keys(&mut self.priority_claims, owner, &stale_keys);
        remove_claim_keys(&mut self.power_claims, owner, &stale_keys);
        let mut summary = PriorityEfficiencyReleaseSummary::default();
        for key in stale_keys {
            let before = summary.restored_processes;
            self.reconcile_priority_key(owner, &key, &mut summary);
            let priority_restored = summary.restored_processes > before;
            self.reconcile_power_key(owner, &key, &mut summary);
            if priority_restored && summary.restored_processes > before + 1 {
                summary.restored_processes -= 1;
            }
        }
        summary
    }

    pub(crate) fn release_all_priority_policy(
        &mut self,
        owner: ControlOwner,
    ) -> PriorityEfficiencyReleaseSummary {
        self.release_priority_policy_except(owner, &BTreeSet::new())
    }

    pub(crate) fn release_all_power_policy(
        &mut self,
        owner: ControlOwner,
    ) -> PriorityEfficiencyReleaseSummary {
        self.release_power_policy_except(owner, &BTreeSet::new())
    }

    pub(crate) fn release_all_efficiency_policy(
        &mut self,
        owner: ControlOwner,
    ) -> PriorityEfficiencyReleaseSummary {
        self.release_efficiency_policy_except(owner, &BTreeSet::new())
    }

    fn reconcile_priority_key(
        &mut self,
        releasing_owner: ControlOwner,
        key: &ProcessTargetKey,
        summary: &mut PriorityEfficiencyReleaseSummary,
    ) {
        let identity = identity_for_key(&self.managed_priorities, key);
        let was_owned = identity.as_ref().is_some_and(|identity| {
            self.managed_priorities
                .get(identity)
                .is_some_and(|managed| managed.owner == releasing_owner)
        });
        if let Some(claim) = self.effective_priority_claim(key).cloned() {
            match self.apply_effective_priority(claim.clone(), true) {
                Ok(_) => summary.restored_processes += usize::from(was_owned),
                Err(ProcessControlError::ProcessExited) => {
                    self.drop_exited_priority(identity.as_ref(), summary)
                }
                Err(error) => summary.failures.push(release_failure_for_target(
                    &claim.target,
                    "Process Priority",
                    error,
                )),
            }
            return;
        }
        let Some(identity) = identity else {
            return;
        };
        if self
            .managed_priorities
            .get(&identity)
            .is_some_and(|managed| !managed.owner.is_automatic())
        {
            return;
        }
        match self.release_priority_identity(&identity) {
            Ok(restored) => {
                summary.restored_processes += usize::from(restored);
                self.managed_priorities.remove(&identity);
            }
            Err(ProcessControlError::ProcessExited) => {
                self.drop_exited_priority(Some(&identity), summary)
            }
            Err(error) => summary.failures.push(release_failure_for_identity(
                &identity,
                "Process Priority",
                error,
            )),
        }
    }

    fn reconcile_power_key(
        &mut self,
        releasing_owner: ControlOwner,
        key: &ProcessTargetKey,
        summary: &mut PriorityEfficiencyReleaseSummary,
    ) {
        let identity = identity_for_key(&self.managed_power, key);
        let was_owned = identity.as_ref().is_some_and(|identity| {
            self.managed_power
                .get(identity)
                .is_some_and(|managed| managed.owner == releasing_owner)
        });
        if let Some(claim) = self.effective_power_claim(key).cloned() {
            match self.apply_effective_power(claim.clone(), true) {
                Ok(_) => summary.restored_processes += usize::from(was_owned),
                Err(ProcessControlError::ProcessExited) => {
                    self.drop_exited_power(identity.as_ref(), summary)
                }
                Err(error) => summary.failures.push(release_failure_for_target(
                    &claim.target,
                    "Power Throttling",
                    error,
                )),
            }
            return;
        }
        let Some(identity) = identity else {
            return;
        };
        if self
            .managed_power
            .get(&identity)
            .is_some_and(|managed| !managed.owner.is_automatic())
        {
            return;
        }
        match self.release_power_identity(&identity) {
            Ok(restored) => {
                summary.restored_processes += usize::from(restored);
                self.managed_power.remove(&identity);
            }
            Err(ProcessControlError::ProcessExited) => {
                self.drop_exited_power(Some(&identity), summary)
            }
            Err(error) => summary.failures.push(release_failure_for_identity(
                &identity,
                "Power Throttling",
                error,
            )),
        }
    }

    fn release_priority_identity(
        &mut self,
        identity: &ProcessIdentity,
    ) -> Result<bool, ProcessControlError> {
        let Some(managed) = self.managed_priorities.get(identity).cloned() else {
            return Ok(false);
        };
        let (_, process) = self.platform.open(&identity.target(), true)?;
        let current = self.platform.query_priority(&process)?;
        if current != managed.expected || current == managed.baseline {
            self.platform.relinquish_priority(identity)?;
            return Ok(false);
        }
        match apply_priority_transition(
            &mut self.platform,
            &process,
            current,
            managed.baseline,
            CommitFailureBehavior::KeepExpected,
        ) {
            Ok(()) => Ok(true),
            Err(failure) if failure.expected_preserved => {
                match self.platform.relinquish_priority(identity) {
                    Ok(()) => Ok(true),
                    Err(error) => {
                        if let Some(managed) = self.managed_priorities.get_mut(identity) {
                            managed.expected = managed.baseline;
                        }
                        Err(ProcessControlError::Failed(format!(
                            "{} Recovery journal relinquish failed: {error}.",
                            failure.message
                        )))
                    }
                }
            }
            Err(failure) => Err(ProcessControlError::Failed(failure.message)),
        }
    }

    fn release_power_identity(
        &mut self,
        identity: &ProcessIdentity,
    ) -> Result<bool, ProcessControlError> {
        let Some(managed) = self.managed_power.get(identity).cloned() else {
            return Ok(false);
        };
        let (_, process) = self.platform.open(&identity.target(), true)?;
        let current = self.platform.query_power(&process)?;
        if current != managed.expected || current == managed.baseline {
            self.platform.relinquish_power(identity)?;
            return Ok(false);
        }
        match apply_power_transition(
            &mut self.platform,
            &process,
            current,
            managed.baseline,
            CommitFailureBehavior::KeepExpected,
        ) {
            Ok(()) => Ok(true),
            Err(failure) if failure.expected_preserved => {
                match self.platform.relinquish_power(identity) {
                    Ok(()) => Ok(true),
                    Err(error) => {
                        if let Some(managed) = self.managed_power.get_mut(identity) {
                            managed.expected = managed.baseline;
                        }
                        Err(ProcessControlError::Failed(format!(
                            "{} Recovery journal relinquish failed: {error}.",
                            failure.message
                        )))
                    }
                }
            }
            Err(failure) => Err(ProcessControlError::Failed(failure.message)),
        }
    }

    fn drop_exited_priority(
        &mut self,
        identity: Option<&ProcessIdentity>,
        summary: &mut PriorityEfficiencyReleaseSummary,
    ) {
        let Some(identity) = identity else {
            return;
        };
        match self.platform.relinquish_priority(identity) {
            Ok(()) => {
                self.managed_priorities.remove(identity);
            }
            Err(error) => summary.failures.push(release_failure_for_identity(
                identity,
                "Process Priority",
                error,
            )),
        }
    }

    fn drop_exited_power(
        &mut self,
        identity: Option<&ProcessIdentity>,
        summary: &mut PriorityEfficiencyReleaseSummary,
    ) {
        let Some(identity) = identity else {
            return;
        };
        match self.platform.relinquish_power(identity) {
            Ok(()) => {
                self.managed_power.remove(identity);
            }
            Err(error) => summary.failures.push(release_failure_for_identity(
                identity,
                "Power Throttling",
                error,
            )),
        }
    }

    pub(crate) fn policy_target_process_ids(&self, owners: &[ControlOwner]) -> BTreeSet<u32> {
        owners
            .iter()
            .flat_map(|owner| {
                self.priority_claims
                    .get(owner)
                    .into_iter()
                    .flat_map(BTreeMap::values)
                    .map(|claim| claim.target.id)
                    .chain(
                        self.power_claims
                            .get(owner)
                            .into_iter()
                            .flat_map(BTreeMap::values)
                            .map(|claim| claim.target.id),
                    )
            })
            .collect()
    }

    pub(crate) fn policy_managed_process_names(&self, owner: ControlOwner) -> Vec<String> {
        self.managed_priorities
            .iter()
            .filter(|(_, managed)| managed.owner == owner)
            .map(|(identity, _)| identity.name.clone())
            .chain(
                self.managed_power
                    .iter()
                    .filter(|(_, managed)| managed.owner == owner)
                    .map(|(identity, _)| identity.name.clone()),
            )
            .collect()
    }

    pub(crate) fn policy_managed_process_count(&self, owner: ControlOwner) -> usize {
        self.managed_priorities
            .iter()
            .filter(|(_, managed)| managed.owner == owner)
            .map(|(identity, _)| identity.key())
            .chain(
                self.managed_power
                    .iter()
                    .filter(|(_, managed)| managed.owner == owner)
                    .map(|(identity, _)| identity.key()),
            )
            .collect::<BTreeSet<_>>()
            .len()
    }

    pub(crate) fn policy_ignore_timer_resolution_count(&self, owner: ControlOwner) -> usize {
        self.power_claims
            .get(&owner)
            .into_iter()
            .flat_map(BTreeMap::values)
            .filter(|claim| claim.ignore_timer_resolution)
            .count()
    }

    pub(crate) fn has_managed_state(&self) -> bool {
        !self.managed_priorities.is_empty() || !self.managed_power.is_empty()
    }

    pub(crate) fn shutdown(&mut self) -> Result<(), String> {
        self.priority_claims.clear();
        self.power_claims.clear();
        let mut releases = self
            .managed_priorities
            .iter()
            .map(|(identity, managed)| {
                (
                    managed.apply_sequence,
                    ManagedProperty::Priority,
                    identity.clone(),
                )
            })
            .chain(self.managed_power.iter().map(|(identity, managed)| {
                (
                    managed.apply_sequence,
                    ManagedProperty::Power,
                    identity.clone(),
                )
            }))
            .collect::<Vec<_>>();
        releases.sort_by_key(|release| std::cmp::Reverse(release.0));
        let mut failures = Vec::new();
        for (_, property, identity) in releases {
            let result = match property {
                ManagedProperty::Priority => self.release_priority_identity(&identity),
                ManagedProperty::Power => self.release_power_identity(&identity),
            };
            match result {
                Ok(_) => match property {
                    ManagedProperty::Priority => {
                        self.managed_priorities.remove(&identity);
                    }
                    ManagedProperty::Power => {
                        self.managed_power.remove(&identity);
                    }
                },
                Err(ProcessControlError::ProcessExited) => {
                    let relinquish = match property {
                        ManagedProperty::Priority => self.platform.relinquish_priority(&identity),
                        ManagedProperty::Power => self.platform.relinquish_power(&identity),
                    };
                    if let Err(error) = relinquish {
                        failures.push(format!("{} ({}): {error}", identity.name, identity.id));
                    }
                    self.managed_priorities.remove(&identity);
                    self.managed_power.remove(&identity);
                }
                Err(error) => {
                    failures.push(format!("{} ({}): {error}", identity.name, identity.id))
                }
            }
        }
        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures.join("; "))
        }
    }

    fn effective_priority_claim(&self, key: &ProcessTargetKey) -> Option<&PriorityClassClaim> {
        priority_owner_precedence()
            .iter()
            .find_map(|owner| self.priority_claims.get(owner)?.get(key))
    }

    fn effective_power_claim(&self, key: &ProcessTargetKey) -> Option<&PowerThrottlingClaim> {
        power_owner_precedence()
            .iter()
            .find_map(|owner| self.power_claims.get(owner)?.get(key))
    }
}

impl<P: PriorityEfficiencyPlatform> Drop for PriorityEfficiencyController<P> {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

#[derive(Clone, Copy)]
enum ManagedProperty {
    Priority,
    Power,
}

struct TransitionFailure {
    message: String,
    uncertain: bool,
    relinquish_recovery: bool,
    expected_preserved: bool,
}

#[derive(Clone, Copy)]
enum CommitFailureBehavior {
    Compensate,
    KeepExpected,
}

fn apply_priority_transition<P: PriorityEfficiencyPlatform>(
    platform: &mut P,
    process: &P::Process,
    original: u32,
    expected: u32,
    commit_failure_behavior: CommitFailureBehavior,
) -> Result<(), TransitionFailure> {
    let intent = platform
        .begin_priority_change(process, original, expected)
        .map_err(transition_begin_failure)?;
    if let Err(error) = platform.apply_priority(process, expected) {
        return Err(compensate_priority_with_intent(
            platform,
            process,
            original,
            intent,
            error.to_string(),
        ));
    }
    match platform.query_priority(process) {
        Ok(actual) if actual == expected => {}
        Ok(actual) => {
            return Err(compensate_priority_with_intent(
                platform,
                process,
                original,
                intent,
                format!("Process Priority verification returned {actual}, expected {expected}."),
            ));
        }
        Err(error) => {
            return Err(compensate_priority_with_intent(
                platform,
                process,
                original,
                intent,
                format!("Process Priority verification failed: {error}"),
            ));
        }
    }
    finish_transition_commit(intent, commit_failure_behavior, || {
        restore_and_verify_priority(platform, process, original)
    })
}

fn apply_power_transition<P: PriorityEfficiencyPlatform>(
    platform: &mut P,
    process: &P::Process,
    original: PowerThrottlingState,
    expected: PowerThrottlingState,
    commit_failure_behavior: CommitFailureBehavior,
) -> Result<(), TransitionFailure> {
    let intent = platform
        .begin_power_change(process, original, expected)
        .map_err(transition_begin_failure)?;
    if let Err(error) = platform.apply_power(process, expected) {
        return Err(compensate_power_with_intent(
            platform,
            process,
            original,
            intent,
            error.to_string(),
        ));
    }
    match platform.query_power(process) {
        Ok(actual) if actual == expected => {}
        Ok(actual) => {
            return Err(compensate_power_with_intent(
                platform,
                process,
                original,
                intent,
                format!(
                    "Power Throttling verification returned {actual:?}, expected {expected:?}."
                ),
            ));
        }
        Err(error) => {
            return Err(compensate_power_with_intent(
                platform,
                process,
                original,
                intent,
                format!("Power Throttling verification failed: {error}"),
            ));
        }
    }
    finish_transition_commit(intent, commit_failure_behavior, || {
        restore_and_verify_power(platform, process, original)
    })
}

fn finish_transition_commit<I: PriorityEfficiencyRecoveryIntent>(
    intent: I,
    behavior: CommitFailureBehavior,
    compensate: impl FnOnce() -> Result<(), ProcessControlError>,
) -> Result<(), TransitionFailure> {
    if let Err(error) = intent.commit() {
        let message = format!("Crash recovery commit failed: {error}");
        return match behavior {
            CommitFailureBehavior::Compensate => match compensate() {
                Ok(()) => Err(TransitionFailure {
                    message,
                    uncertain: false,
                    relinquish_recovery: true,
                    expected_preserved: false,
                }),
                Err(compensation_error) => Err(TransitionFailure {
                    message: transition_failure_message(
                        message,
                        compensation_error.to_string(),
                        None,
                    ),
                    uncertain: true,
                    relinquish_recovery: false,
                    expected_preserved: false,
                }),
            },
            CommitFailureBehavior::KeepExpected => Err(TransitionFailure {
                message,
                uncertain: false,
                relinquish_recovery: true,
                expected_preserved: true,
            }),
        };
    }
    Ok(())
}

fn compensate_priority_with_intent<P: PriorityEfficiencyPlatform>(
    platform: &mut P,
    process: &P::Process,
    original: u32,
    intent: P::RecoveryIntent,
    primary_error: String,
) -> TransitionFailure {
    match restore_and_verify_priority(platform, process, original) {
        Ok(()) => TransitionFailure {
            message: primary_error,
            uncertain: false,
            relinquish_recovery: false,
            expected_preserved: false,
        },
        Err(compensation_error) => {
            let recovery_error = intent.commit().err();
            TransitionFailure {
                message: transition_failure_message(
                    primary_error,
                    compensation_error.to_string(),
                    recovery_error,
                ),
                uncertain: true,
                relinquish_recovery: false,
                expected_preserved: false,
            }
        }
    }
}

fn compensate_power_with_intent<P: PriorityEfficiencyPlatform>(
    platform: &mut P,
    process: &P::Process,
    original: PowerThrottlingState,
    intent: P::RecoveryIntent,
    primary_error: String,
) -> TransitionFailure {
    match restore_and_verify_power(platform, process, original) {
        Ok(()) => TransitionFailure {
            message: primary_error,
            uncertain: false,
            relinquish_recovery: false,
            expected_preserved: false,
        },
        Err(compensation_error) => {
            let recovery_error = intent.commit().err();
            TransitionFailure {
                message: transition_failure_message(
                    primary_error,
                    compensation_error.to_string(),
                    recovery_error,
                ),
                uncertain: true,
                relinquish_recovery: false,
                expected_preserved: false,
            }
        }
    }
}

fn restore_and_verify_priority<P: PriorityEfficiencyPlatform>(
    platform: &mut P,
    process: &P::Process,
    original: u32,
) -> Result<(), ProcessControlError> {
    platform.apply_priority(process, original)?;
    let actual = platform.query_priority(process)?;
    if actual == original {
        Ok(())
    } else {
        Err(ProcessControlError::Failed(format!(
            "Process Priority compensation returned {actual}, expected {original}."
        )))
    }
}

fn restore_and_verify_power<P: PriorityEfficiencyPlatform>(
    platform: &mut P,
    process: &P::Process,
    original: PowerThrottlingState,
) -> Result<(), ProcessControlError> {
    platform.apply_power(process, original)?;
    let actual = platform.query_power(process)?;
    if actual == original {
        Ok(())
    } else {
        Err(ProcessControlError::Failed(format!(
            "Power Throttling compensation returned {actual:?}, expected {original:?}."
        )))
    }
}

fn transition_begin_failure(error: ProcessControlError) -> TransitionFailure {
    TransitionFailure {
        message: error.to_string(),
        uncertain: false,
        relinquish_recovery: false,
        expected_preserved: false,
    }
}

fn transition_failure_message(
    primary_error: String,
    compensation_error: String,
    recovery_error: Option<String>,
) -> String {
    let mut message = format!("{primary_error} Compensation failed: {compensation_error}.");
    if let Some(recovery_error) = recovery_error {
        message.push_str(&format!(
            " Crash recovery commit also failed: {recovery_error}."
        ));
    }
    message
}

fn priority_is_preserved(
    preservation: PriorityClassPreservation,
    baseline: u32,
    desired: u32,
) -> bool {
    let baseline = PriorityClassValue::from_raw(baseline);
    let desired = PriorityClassValue::from_raw(desired);
    match preservation {
        PriorityClassPreservation::Exact => false,
        PriorityClassPreservation::PreserveHigher => baseline
            .zip(desired)
            .is_some_and(|(baseline, desired)| baseline.rank() >= desired.rank()),
        PriorityClassPreservation::PreserveLower => baseline
            .zip(desired)
            .is_some_and(|(baseline, desired)| baseline.rank() <= desired.rank()),
        PriorityClassPreservation::PreserveHigherOrHighOrRealtime => {
            baseline.is_some_and(|baseline| {
                matches!(
                    baseline,
                    PriorityClassValue::High | PriorityClassValue::Realtime
                ) || desired.is_some_and(|desired| baseline.rank() >= desired.rank())
            })
        }
        PriorityClassPreservation::PreserveLowerOrHighOrRealtime => {
            baseline.is_some_and(|baseline| {
                matches!(
                    baseline,
                    PriorityClassValue::High | PriorityClassValue::Realtime
                ) || desired.is_some_and(|desired| baseline.rank() <= desired.rank())
            })
        }
        PriorityClassPreservation::PreserveHighOrRealtime => baseline.is_some_and(|baseline| {
            matches!(
                baseline,
                PriorityClassValue::High | PriorityClassValue::Realtime
            )
        }),
    }
}

fn priority_owner_precedence() -> &'static [ControlOwner] {
    &[
        ControlOwner::BackgroundEfficiency,
        ControlOwner::CpuSchedulerFocusPriority,
        ControlOwner::AdaptiveEngine,
        ControlOwner::ProcessPriority,
    ]
}

fn power_owner_precedence() -> &'static [ControlOwner] {
    &[
        ControlOwner::BackgroundEfficiency,
        ControlOwner::AdaptiveEngine,
    ]
}

fn validate_priority_owner(owner: ControlOwner) -> Result<(), ProcessControlError> {
    if priority_owner_precedence().contains(&owner) {
        Ok(())
    } else {
        Err(ProcessControlError::Failed(
            "This owner cannot submit automatic Process Priority claims.".to_owned(),
        ))
    }
}

fn validate_power_owner(owner: ControlOwner) -> Result<(), ProcessControlError> {
    if power_owner_precedence().contains(&owner) {
        Ok(())
    } else {
        Err(ProcessControlError::Failed(
            "This owner cannot submit automatic Power Throttling claims.".to_owned(),
        ))
    }
}

fn restore_claim<C>(
    claims: &mut BTreeMap<ControlOwner, BTreeMap<ProcessTargetKey, C>>,
    owner: ControlOwner,
    key: ProcessTargetKey,
    previous: Option<C>,
) {
    let owner_claims = claims.entry(owner).or_default();
    if let Some(previous) = previous {
        owner_claims.insert(key, previous);
    } else {
        owner_claims.remove(&key);
    }
    if owner_claims.is_empty() {
        claims.remove(&owner);
    }
}

fn stale_claim_keys<C>(
    claims: &BTreeMap<ControlOwner, BTreeMap<ProcessTargetKey, C>>,
    owner: ControlOwner,
    active_targets: &BTreeSet<ProcessTargetKey>,
) -> BTreeSet<ProcessTargetKey> {
    claims
        .get(&owner)
        .into_iter()
        .flat_map(BTreeMap::keys)
        .filter(|key| !active_targets.contains(*key))
        .cloned()
        .collect()
}

fn remove_claim_keys<C>(
    claims: &mut BTreeMap<ControlOwner, BTreeMap<ProcessTargetKey, C>>,
    owner: ControlOwner,
    keys: &BTreeSet<ProcessTargetKey>,
) {
    if let Some(owner_claims) = claims.get_mut(&owner) {
        owner_claims.retain(|key, _| !keys.contains(key));
        if owner_claims.is_empty() {
            claims.remove(&owner);
        }
    }
}

fn identity_for_key<T>(
    managed: &BTreeMap<ProcessIdentity, ManagedValue<T>>,
    key: &ProcessTargetKey,
) -> Option<ProcessIdentity> {
    managed
        .keys()
        .find(|identity| identity.key() == *key)
        .cloned()
}

fn restore_managed_snapshot<T>(
    managed: &mut BTreeMap<ProcessIdentity, ManagedValue<T>>,
    identity: ProcessIdentity,
    previous: Option<ManagedValue<T>>,
) {
    if let Some(previous) = previous {
        managed.insert(identity, previous);
    } else {
        managed.remove(&identity);
    }
}

fn release_failure_for_target(
    target: &ProcessControlTarget,
    property: &'static str,
    error: ProcessControlError,
) -> PriorityEfficiencyReleaseFailure {
    PriorityEfficiencyReleaseFailure {
        process_id: target.id,
        process_name: target.name.clone(),
        executable_path: target.executable_path.to_string_lossy().into_owned(),
        property,
        error,
    }
}

fn release_failure_for_identity(
    identity: &ProcessIdentity,
    property: &'static str,
    error: ProcessControlError,
) -> PriorityEfficiencyReleaseFailure {
    PriorityEfficiencyReleaseFailure {
        process_id: identity.id,
        process_name: identity.name.clone(),
        executable_path: identity.executable_path.to_string_lossy().into_owned(),
        property,
        error,
    }
}

fn power_throttling_error(error: PowerThrottlingError) -> ProcessControlError {
    match error {
        PowerThrottlingError::AccessDenied => {
            ProcessControlError::AccessDenied("Access denied.".to_owned())
        }
        PowerThrottlingError::Unavailable => ProcessControlError::Unavailable(
            "Windows process power throttling is unavailable for this process.".to_owned(),
        ),
        PowerThrottlingError::Failed { operation, code } => {
            ProcessControlError::Failed(format!("{operation} failed with error {code}."))
        }
    }
}

pub(crate) fn current_process_priority(
    target: &ProcessActionTarget,
    allow_cross_session_process_control: bool,
) -> Result<ProcessPrioritySetting, ProcessControlError> {
    let mut platform = WindowsPriorityEfficiencyPlatform;
    let (_, process) = platform.open(
        &ProcessControlTarget::from_action_target(target),
        allow_cross_session_process_control,
    )?;
    let raw = platform.query_priority(&process)?;
    PriorityClassValue::from_raw(raw)
        .map(PriorityClassValue::setting)
        .ok_or_else(|| {
            ProcessControlError::Unavailable(format!(
                "Windows returned unsupported Process Priority value {raw}."
            ))
        })
}

pub(crate) fn current_efficiency_mode(
    target: &ProcessActionTarget,
    allow_cross_session_process_control: bool,
) -> Result<bool, ProcessControlError> {
    let mut platform = WindowsPriorityEfficiencyPlatform;
    let (_, process) = platform.open(
        &ProcessControlTarget::from_action_target(target),
        allow_cross_session_process_control,
    )?;
    let priority = platform.query_priority(&process)?;
    let power = platform.query_power(&process)?;
    Ok(power.execution_speed_enabled() && priority == windows_priority_efficiency::PRIORITY_IDLE)
}

#[cfg(test)]
mod tests {
    use std::{
        path::PathBuf,
        process::{Child, Command},
        time::{Duration, Instant},
    };

    use super::*;
    use crate::platform::windows::priority_efficiency::{
        POWER_CURRENT_VERSION as PROCESS_POWER_THROTTLING_CURRENT_VERSION,
        POWER_EXECUTION_SPEED as PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
        POWER_IGNORE_TIMER_RESOLUTION as PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
        PRIORITY_ABOVE_NORMAL as ABOVE_NORMAL_PRIORITY_CLASS,
        PRIORITY_BELOW_NORMAL as BELOW_NORMAL_PRIORITY_CLASS, PRIORITY_HIGH as HIGH_PRIORITY_CLASS,
        PRIORITY_IDLE as IDLE_PRIORITY_CLASS, PRIORITY_NORMAL as NORMAL_PRIORITY_CLASS,
    };

    #[derive(Clone)]
    struct FakeProcess {
        identity: ProcessIdentity,
        priority: u32,
        power: PowerThrottlingState,
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum FakeEvent {
        Priority(u32, u32),
        Power(u32, PowerThrottlingState),
        RelinquishPriority(u32, u64),
        RelinquishPower(u32, u64),
    }

    #[derive(Default)]
    struct FakePlatform {
        processes: BTreeMap<u32, FakeProcess>,
        events: Vec<FakeEvent>,
        fail_next_priority_begin: bool,
        fail_next_power_begin: bool,
        fail_next_priority_apply: bool,
        fail_next_power_apply: bool,
        fail_next_priority_commit: bool,
        fail_next_power_commit: bool,
        fail_priority_relinquish: bool,
        fail_power_relinquish: bool,
        power_unavailable: bool,
    }

    struct FakeIntent {
        fail: bool,
    }

    impl PriorityEfficiencyRecoveryIntent for FakeIntent {
        fn commit(self) -> Result<(), String> {
            if self.fail {
                Err("injected commit failure".to_owned())
            } else {
                Ok(())
            }
        }
    }

    impl PriorityEfficiencyPlatform for FakePlatform {
        type Process = u32;
        type RecoveryIntent = FakeIntent;

        fn open(
            &mut self,
            target: &ProcessControlTarget,
            _allow_cross_session_process_control: bool,
        ) -> Result<(ProcessIdentity, Self::Process), ProcessControlError> {
            let process = self
                .processes
                .get(&target.id)
                .ok_or(ProcessControlError::ProcessExited)?;
            if process.identity.key() != target.key() {
                return Err(ProcessControlError::ProcessExited);
            }
            Ok((process.identity.clone(), target.id))
        }

        fn query_priority(&mut self, process: &Self::Process) -> Result<u32, ProcessControlError> {
            self.processes
                .get(process)
                .map(|process| process.priority)
                .ok_or(ProcessControlError::ProcessExited)
        }

        fn query_power(
            &mut self,
            process: &Self::Process,
        ) -> Result<PowerThrottlingState, ProcessControlError> {
            if self.power_unavailable {
                return Err(ProcessControlError::Unavailable(
                    "injected unavailable power state".to_owned(),
                ));
            }
            self.processes
                .get(process)
                .map(|process| process.power)
                .ok_or(ProcessControlError::ProcessExited)
        }

        fn begin_priority_change(
            &mut self,
            _process: &Self::Process,
            _original: u32,
            _expected: u32,
        ) -> Result<Self::RecoveryIntent, ProcessControlError> {
            if std::mem::take(&mut self.fail_next_priority_begin) {
                return Err(ProcessControlError::Failed(
                    "injected priority begin failure".to_owned(),
                ));
            }
            Ok(FakeIntent {
                fail: std::mem::take(&mut self.fail_next_priority_commit),
            })
        }

        fn begin_power_change(
            &mut self,
            _process: &Self::Process,
            _original: PowerThrottlingState,
            _expected: PowerThrottlingState,
        ) -> Result<Self::RecoveryIntent, ProcessControlError> {
            if std::mem::take(&mut self.fail_next_power_begin) {
                return Err(ProcessControlError::Failed(
                    "injected power begin failure".to_owned(),
                ));
            }
            Ok(FakeIntent {
                fail: std::mem::take(&mut self.fail_next_power_commit),
            })
        }

        fn apply_priority(
            &mut self,
            process: &Self::Process,
            priority: u32,
        ) -> Result<(), ProcessControlError> {
            if std::mem::take(&mut self.fail_next_priority_apply) {
                return Err(ProcessControlError::Failed(
                    "injected priority apply failure".to_owned(),
                ));
            }
            let state = self
                .processes
                .get_mut(process)
                .ok_or(ProcessControlError::ProcessExited)?;
            state.priority = priority;
            self.events.push(FakeEvent::Priority(*process, priority));
            Ok(())
        }

        fn apply_power(
            &mut self,
            process: &Self::Process,
            power: PowerThrottlingState,
        ) -> Result<(), ProcessControlError> {
            if std::mem::take(&mut self.fail_next_power_apply) {
                return Err(ProcessControlError::Failed(
                    "injected power apply failure".to_owned(),
                ));
            }
            let state = self
                .processes
                .get_mut(process)
                .ok_or(ProcessControlError::ProcessExited)?;
            state.power = power;
            self.events.push(FakeEvent::Power(*process, power));
            Ok(())
        }

        fn relinquish_priority(
            &mut self,
            identity: &ProcessIdentity,
        ) -> Result<(), ProcessControlError> {
            if self.fail_priority_relinquish {
                return Err(ProcessControlError::Failed(
                    "injected priority relinquish failure".to_owned(),
                ));
            }
            self.events.push(FakeEvent::RelinquishPriority(
                identity.id,
                identity.creation_time,
            ));
            Ok(())
        }

        fn relinquish_power(
            &mut self,
            identity: &ProcessIdentity,
        ) -> Result<(), ProcessControlError> {
            if self.fail_power_relinquish {
                return Err(ProcessControlError::Failed(
                    "injected power relinquish failure".to_owned(),
                ));
            }
            self.events.push(FakeEvent::RelinquishPower(
                identity.id,
                identity.creation_time,
            ));
            Ok(())
        }
    }

    fn identity(id: u32, creation_time: u64) -> ProcessIdentity {
        ProcessIdentity::new(
            id,
            format!("app{id}.exe"),
            PathBuf::from(format!(r"C:\Apps\app{id}.exe")),
            creation_time,
            Some(1),
        )
    }

    fn target(id: u32, creation_time: u64) -> ProcessControlTarget {
        identity(id, creation_time).target()
    }

    fn action_target(id: u32, creation_time: u64) -> ProcessActionTarget {
        let identity = identity(id, creation_time);
        ProcessActionTarget {
            id,
            name: identity.name,
            executable_path: identity.executable_path,
            creation_time,
            session_id: Some(1),
            is_service_account: Some(false),
        }
    }

    fn baseline_power() -> PowerThrottlingState {
        PowerThrottlingState {
            version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
            control_mask: PROCESS_POWER_THROTTLING_EXECUTION_SPEED,
            state_mask: 0,
        }
    }

    fn platform_with(id: u32, creation_time: u64, priority: u32) -> FakePlatform {
        let mut platform = FakePlatform::default();
        platform.processes.insert(
            id,
            FakeProcess {
                identity: identity(id, creation_time),
                priority,
                power: baseline_power(),
            },
        );
        platform
    }

    fn priority_claim(
        id: u32,
        creation_time: u64,
        owner: ControlOwner,
        priority: PriorityClassValue,
    ) -> PriorityClassClaim {
        PriorityClassClaim {
            target: target(id, creation_time),
            owner,
            priority,
            preservation: PriorityClassPreservation::Exact,
        }
    }

    fn power_claim(
        id: u32,
        creation_time: u64,
        owner: ControlOwner,
        ignore_timer_resolution: bool,
    ) -> PowerThrottlingClaim {
        PowerThrottlingClaim {
            target: target(id, creation_time),
            owner,
            ignore_timer_resolution,
        }
    }

    fn efficiency_claim(id: u32, creation_time: u64) -> EfficiencyModeClaim {
        EfficiencyModeClaim {
            target: target(id, creation_time),
            owner: ControlOwner::BackgroundEfficiency,
            ignore_timer_resolution: false,
        }
    }

    #[test]
    fn adaptive_priority_temporarily_overrides_static_without_losing_the_baseline() {
        let mut controller =
            PriorityEfficiencyController::with_platform(platform_with(7, 1, NORMAL_PRIORITY_CLASS));
        controller
            .apply_priority_claim(
                priority_claim(
                    7,
                    1,
                    ControlOwner::ProcessPriority,
                    PriorityClassValue::BelowNormal,
                ),
                true,
            )
            .unwrap();
        controller
            .apply_priority_claim(
                priority_claim(7, 1, ControlOwner::AdaptiveEngine, PriorityClassValue::Idle),
                true,
            )
            .unwrap();

        controller.release_all_priority_policy(ControlOwner::AdaptiveEngine);
        assert_eq!(
            controller.platform.processes[&7].priority,
            BELOW_NORMAL_PRIORITY_CLASS
        );
        controller.release_all_priority_policy(ControlOwner::ProcessPriority);
        assert_eq!(
            controller.platform.processes[&7].priority,
            NORMAL_PRIORITY_CLASS
        );
    }

    #[test]
    fn background_efficiency_overrides_then_reveals_adaptive_claims() {
        let mut controller =
            PriorityEfficiencyController::with_platform(platform_with(7, 1, NORMAL_PRIORITY_CLASS));
        controller
            .apply_priority_claim(
                priority_claim(
                    7,
                    1,
                    ControlOwner::AdaptiveEngine,
                    PriorityClassValue::BelowNormal,
                ),
                true,
            )
            .unwrap();
        controller
            .apply_power_claim(power_claim(7, 1, ControlOwner::AdaptiveEngine, true), true)
            .unwrap();
        controller
            .apply_efficiency_claim(efficiency_claim(7, 1), true)
            .unwrap();

        assert_eq!(
            controller.platform.processes[&7].priority,
            IDLE_PRIORITY_CLASS
        );
        assert!(controller.platform.processes[&7]
            .power
            .execution_speed_enabled());
        let summary = controller.release_all_efficiency_policy(ControlOwner::BackgroundEfficiency);
        assert!(summary.failures.is_empty());
        assert_eq!(
            controller.platform.processes[&7].priority,
            BELOW_NORMAL_PRIORITY_CLASS
        );
        assert!(controller.platform.processes[&7]
            .power
            .ignore_timer_resolution_enabled());

        controller.release_all_priority_policy(ControlOwner::AdaptiveEngine);
        controller.release_all_power_policy(ControlOwner::AdaptiveEngine);
        assert_eq!(
            controller.platform.processes[&7].priority,
            NORMAL_PRIORITY_CLASS
        );
        assert_eq!(controller.platform.processes[&7].power, baseline_power());
    }

    #[test]
    fn foreground_boost_outranks_adaptive_but_not_background_efficiency() {
        let mut controller =
            PriorityEfficiencyController::with_platform(platform_with(7, 1, NORMAL_PRIORITY_CLASS));
        controller
            .apply_priority_claim(
                priority_claim(7, 1, ControlOwner::AdaptiveEngine, PriorityClassValue::Idle),
                true,
            )
            .unwrap();
        controller
            .apply_priority_claim(
                priority_claim(
                    7,
                    1,
                    ControlOwner::CpuSchedulerFocusPriority,
                    PriorityClassValue::AboveNormal,
                ),
                true,
            )
            .unwrap();
        assert_eq!(
            controller.platform.processes[&7].priority,
            ABOVE_NORMAL_PRIORITY_CLASS
        );

        controller
            .apply_efficiency_claim(efficiency_claim(7, 1), true)
            .unwrap();
        assert_eq!(
            controller.platform.processes[&7].priority,
            IDLE_PRIORITY_CLASS
        );
        controller.release_all_efficiency_policy(ControlOwner::BackgroundEfficiency);
        assert_eq!(
            controller.platform.processes[&7].priority,
            ABOVE_NORMAL_PRIORITY_CLASS
        );
        controller.release_all_priority_policy(ControlOwner::CpuSchedulerFocusPriority);
        assert_eq!(
            controller.platform.processes[&7].priority,
            IDLE_PRIORITY_CLASS
        );
    }

    #[test]
    fn process_list_action_is_immediate_then_automatic_policy_supersedes_it() {
        let mut controller =
            PriorityEfficiencyController::with_platform(platform_with(7, 1, NORMAL_PRIORITY_CLASS));
        let automatic =
            priority_claim(7, 1, ControlOwner::AdaptiveEngine, PriorityClassValue::Idle);
        controller
            .apply_priority_claim(automatic.clone(), true)
            .unwrap();
        controller
            .apply_process_list_priority(
                &action_target(7, 1),
                ProcessPrioritySetting::AboveNormal,
                true,
            )
            .unwrap();
        assert_eq!(
            controller.platform.processes[&7].priority,
            ABOVE_NORMAL_PRIORITY_CLASS
        );

        controller.apply_priority_claim(automatic, true).unwrap();
        assert_eq!(
            controller.platform.processes[&7].priority,
            IDLE_PRIORITY_CLASS
        );
        controller.shutdown().unwrap();
        assert_eq!(
            controller.platform.processes[&7].priority,
            NORMAL_PRIORITY_CLASS
        );
    }

    #[test]
    fn unavailable_power_state_does_not_block_an_independent_priority_claim() {
        let mut platform = platform_with(7, 1, NORMAL_PRIORITY_CLASS);
        platform.power_unavailable = true;
        let mut controller = PriorityEfficiencyController::with_platform(platform);

        assert!(matches!(
            controller
                .apply_power_claim(power_claim(7, 1, ControlOwner::AdaptiveEngine, false), true,),
            Err(ProcessControlError::Unavailable(_))
        ));
        assert_eq!(
            controller
                .apply_priority_claim(
                    priority_claim(
                        7,
                        1,
                        ControlOwner::AdaptiveEngine,
                        PriorityClassValue::BelowNormal,
                    ),
                    true,
                )
                .unwrap(),
            ProcessPropertyApplyOutcome::Applied
        );
        assert_eq!(
            controller.platform.processes[&7].priority,
            BELOW_NORMAL_PRIORITY_CLASS
        );
    }

    #[test]
    fn compound_priority_failure_rolls_power_back_and_leaves_no_owner() {
        let mut platform = platform_with(7, 1, NORMAL_PRIORITY_CLASS);
        platform.fail_next_priority_apply = true;
        let mut controller = PriorityEfficiencyController::with_platform(platform);

        assert!(controller
            .apply_efficiency_claim(efficiency_claim(7, 1), true)
            .is_err());
        assert_eq!(
            controller.platform.processes[&7].priority,
            NORMAL_PRIORITY_CLASS
        );
        assert_eq!(controller.platform.processes[&7].power, baseline_power());
        assert!(!controller.has_managed_state());
        assert!(controller
            .priority_claims
            .get(&ControlOwner::BackgroundEfficiency)
            .is_none_or(BTreeMap::is_empty));
        assert!(controller
            .power_claims
            .get(&ControlOwner::BackgroundEfficiency)
            .is_none_or(BTreeMap::is_empty));
    }

    #[test]
    fn compound_commit_failure_compensates_both_properties_and_relinquishes() {
        let mut platform = platform_with(7, 1, NORMAL_PRIORITY_CLASS);
        platform.fail_next_priority_commit = true;
        let mut controller = PriorityEfficiencyController::with_platform(platform);

        assert!(controller
            .apply_efficiency_claim(efficiency_claim(7, 1), true)
            .is_err());
        assert_eq!(
            controller.platform.processes[&7].priority,
            NORMAL_PRIORITY_CLASS
        );
        assert_eq!(controller.platform.processes[&7].power, baseline_power());
        assert!(controller
            .platform
            .events
            .contains(&FakeEvent::RelinquishPriority(7, 1)));
        assert!(!controller.has_managed_state());
    }

    #[test]
    fn external_break_is_relinquished_without_overwriting_either_property() {
        let mut controller =
            PriorityEfficiencyController::with_platform(platform_with(7, 1, NORMAL_PRIORITY_CLASS));
        controller
            .apply_efficiency_claim(efficiency_claim(7, 1), true)
            .unwrap();
        let external_power = PowerThrottlingState {
            version: PROCESS_POWER_THROTTLING_CURRENT_VERSION,
            control_mask: PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
            state_mask: PROCESS_POWER_THROTTLING_IGNORE_TIMER_RESOLUTION,
        };
        let process = controller.platform.processes.get_mut(&7).unwrap();
        process.priority = ABOVE_NORMAL_PRIORITY_CLASS;
        process.power = external_power;

        controller.release_all_efficiency_policy(ControlOwner::BackgroundEfficiency);
        assert_eq!(
            controller.platform.processes[&7].priority,
            ABOVE_NORMAL_PRIORITY_CLASS
        );
        assert_eq!(controller.platform.processes[&7].power, external_power);
        assert!(controller
            .platform
            .events
            .contains(&FakeEvent::RelinquishPriority(7, 1)));
        assert!(controller
            .platform
            .events
            .contains(&FakeEvent::RelinquishPower(7, 1)));
        assert!(!controller.has_managed_state());
    }

    #[test]
    fn pid_reuse_relinquishes_the_old_identity_before_mutating_the_new_process() {
        let mut controller =
            PriorityEfficiencyController::with_platform(platform_with(7, 1, NORMAL_PRIORITY_CLASS));
        controller
            .apply_efficiency_claim(efficiency_claim(7, 1), true)
            .unwrap();
        controller.platform.processes.insert(
            7,
            FakeProcess {
                identity: identity(7, 2),
                priority: NORMAL_PRIORITY_CLASS,
                power: baseline_power(),
            },
        );

        controller
            .apply_efficiency_claim(efficiency_claim(7, 2), true)
            .unwrap();
        assert!(controller
            .platform
            .events
            .contains(&FakeEvent::RelinquishPriority(7, 1)));
        assert!(controller
            .platform
            .events
            .contains(&FakeEvent::RelinquishPower(7, 1)));
        assert_eq!(
            controller.platform.processes[&7].priority,
            IDLE_PRIORITY_CLASS
        );
    }

    #[test]
    fn session_metadata_drift_does_not_create_a_second_process_identity() {
        let mut controller =
            PriorityEfficiencyController::with_platform(platform_with(7, 1, NORMAL_PRIORITY_CLASS));
        controller
            .apply_priority_claim(
                priority_claim(
                    7,
                    1,
                    ControlOwner::ProcessPriority,
                    PriorityClassValue::BelowNormal,
                ),
                true,
            )
            .unwrap();
        controller
            .platform
            .processes
            .get_mut(&7)
            .unwrap()
            .identity
            .session_id = None;
        controller
            .apply_priority_claim(
                priority_claim(
                    7,
                    1,
                    ControlOwner::ProcessPriority,
                    PriorityClassValue::BelowNormal,
                ),
                true,
            )
            .unwrap();

        assert_eq!(controller.managed_priorities.len(), 1);
        assert!(!controller
            .platform
            .events
            .contains(&FakeEvent::RelinquishPriority(7, 1)));
    }

    #[test]
    fn high_or_realtime_preservation_uses_the_original_baseline() {
        let mut controller =
            PriorityEfficiencyController::with_platform(platform_with(7, 1, HIGH_PRIORITY_CLASS));
        let mut claim =
            priority_claim(7, 1, ControlOwner::AdaptiveEngine, PriorityClassValue::Idle);
        claim.preservation = PriorityClassPreservation::PreserveHighOrRealtime;

        assert_eq!(
            controller.apply_priority_claim(claim, true).unwrap(),
            ProcessPropertyApplyOutcome::Preserved
        );
        assert!(!controller.has_managed_state());
        assert!(controller.platform.events.is_empty());
    }

    #[test]
    fn release_commit_failure_keeps_the_verified_baseline_and_relinquishes() {
        let mut controller =
            PriorityEfficiencyController::with_platform(platform_with(7, 1, NORMAL_PRIORITY_CLASS));
        controller
            .apply_priority_claim(
                priority_claim(
                    7,
                    1,
                    ControlOwner::ProcessPriority,
                    PriorityClassValue::BelowNormal,
                ),
                true,
            )
            .unwrap();
        controller.platform.fail_next_priority_commit = true;

        let summary = controller.release_all_priority_policy(ControlOwner::ProcessPriority);
        assert!(summary.failures.is_empty());
        assert_eq!(
            controller.platform.processes[&7].priority,
            NORMAL_PRIORITY_CLASS
        );
        assert!(controller
            .platform
            .events
            .contains(&FakeEvent::RelinquishPriority(7, 1)));
        assert!(!controller.has_managed_state());
    }

    #[test]
    fn shutdown_restores_priority_and_power_in_reverse_application_order() {
        let mut platform = platform_with(7, 1, NORMAL_PRIORITY_CLASS);
        platform.processes.insert(
            8,
            FakeProcess {
                identity: identity(8, 1),
                priority: NORMAL_PRIORITY_CLASS,
                power: baseline_power(),
            },
        );
        let mut controller = PriorityEfficiencyController::with_platform(platform);
        controller
            .apply_efficiency_claim(efficiency_claim(7, 1), true)
            .unwrap();
        controller
            .apply_priority_claim(
                priority_claim(
                    8,
                    1,
                    ControlOwner::ProcessPriority,
                    PriorityClassValue::BelowNormal,
                ),
                true,
            )
            .unwrap();
        controller.shutdown().unwrap();

        let tail = &controller.platform.events[controller.platform.events.len() - 3..];
        assert_eq!(
            tail,
            &[
                FakeEvent::Priority(8, NORMAL_PRIORITY_CLASS),
                FakeEvent::Priority(7, NORMAL_PRIORITY_CLASS),
                FakeEvent::Power(7, baseline_power()),
            ]
        );
    }

    #[test]
    #[ignore = "modifies and restores a disposable Windows process; run in explicit integration QA"]
    fn priority_efficiency_live_apply_and_clean_release() -> Result<(), String> {
        struct DisposableProcess(Child);

        impl Drop for DisposableProcess {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }

        let mut executable_path = std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .ok_or_else(|| "SystemRoot is unavailable.".to_owned())?;
        executable_path.push(r"System32\PING.EXE");
        let child = Command::new(&executable_path)
            .args(["-t", "127.0.0.1"])
            .spawn()
            .map_err(|error| format!("Could not start disposable process: {error}"))?;
        let child = DisposableProcess(child);
        let deadline = Instant::now() + Duration::from_secs(2);
        let target = loop {
            match crate::foreground::capture_process_action_target(
                child.0.id(),
                &executable_path,
                true,
            ) {
                Ok(target) => break target,
                Err(_) if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(25));
                }
                Err(error) => return Err(error.to_string()),
            }
        };
        let baseline_priority =
            current_process_priority(&target, true).map_err(|error| error.to_string())?;
        let baseline_efficiency =
            current_efficiency_mode(&target, true).map_err(|error| error.to_string())?;
        let expected_efficiency = !baseline_efficiency;
        let mut controller = PriorityEfficiencyController::default();

        controller
            .apply_process_list_efficiency_mode(&target, expected_efficiency, true)
            .map_err(|error| error.to_string())?;
        assert_eq!(
            current_efficiency_mode(&target, true).map_err(|error| error.to_string())?,
            expected_efficiency
        );
        controller.shutdown()?;
        assert_eq!(
            current_process_priority(&target, true).map_err(|error| error.to_string())?,
            baseline_priority
        );
        assert_eq!(
            current_efficiency_mode(&target, true).map_err(|error| error.to_string())?,
            baseline_efficiency
        );
        Ok(())
    }
}

#[cfg(test)]
mod adaptive_preservation_tests {
    use super::*;

    #[test]
    fn adaptive_preservation_keeps_direction_and_high_priority_protection() {
        for policy in [
            PriorityClassPreservation::PreserveHighOrRealtime,
            PriorityClassPreservation::PreserveHigherOrHighOrRealtime,
            PriorityClassPreservation::PreserveLowerOrHighOrRealtime,
        ] {
            for baseline in [PriorityClassValue::High, PriorityClassValue::Realtime] {
                assert!(priority_is_preserved(
                    policy,
                    baseline.raw(),
                    PriorityClassValue::Realtime.raw()
                ));
            }
        }
        for (policy, kept, changed) in [
            (
                PriorityClassPreservation::PreserveHigherOrHighOrRealtime,
                PriorityClassValue::AboveNormal,
                PriorityClassValue::BelowNormal,
            ),
            (
                PriorityClassPreservation::PreserveLowerOrHighOrRealtime,
                PriorityClassValue::BelowNormal,
                PriorityClassValue::AboveNormal,
            ),
        ] {
            assert!(priority_is_preserved(
                policy,
                kept.raw(),
                PriorityClassValue::Normal.raw()
            ));
            assert!(priority_is_preserved(
                policy,
                PriorityClassValue::Normal.raw(),
                PriorityClassValue::Normal.raw()
            ));
            assert!(!priority_is_preserved(
                policy,
                changed.raw(),
                PriorityClassValue::Normal.raw()
            ));
        }
    }
}
