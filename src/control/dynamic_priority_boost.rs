use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};

use crate::{
    backend::crash_recovery::{
        forget_dynamic_priority_boost_change, record_process_change, ProcessValue, RecoveryIntent,
    },
    foreground::ProcessActionTarget,
    platform::windows::dynamic_priority_boost as windows_dynamic_priority_boost,
    win_util::WinHandle,
};

use super::process::{
    open_process_for_set_information, transition_failure_error, ControlOwner, ProcessControlError,
    ProcessControlTarget, ProcessIdentity, ProcessTargetKey,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DynamicPriorityBoostState {
    Enabled,
    Disabled,
}

impl DynamicPriorityBoostState {
    pub(crate) fn from_disabled(disabled: bool) -> Self {
        if disabled {
            Self::Disabled
        } else {
            Self::Enabled
        }
    }

    fn disabled(self) -> bool {
        matches!(self, Self::Disabled)
    }
}

#[derive(Debug, Clone)]
pub(crate) struct DynamicPriorityBoostClaim {
    pub(crate) target: ProcessControlTarget,
    pub(crate) owner: ControlOwner,
    pub(crate) state: DynamicPriorityBoostState,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DynamicPriorityBoostApplyOutcome {
    Applied,
    Unchanged,
}

#[derive(Debug)]
pub(crate) struct DynamicPriorityBoostReleaseFailure {
    pub(crate) process_id: u32,
    pub(crate) process_name: String,
    pub(crate) executable_path: String,
    pub(crate) error: ProcessControlError,
}

#[derive(Debug, Default)]
pub(crate) struct DynamicPriorityBoostReleaseSummary {
    pub(crate) restored_processes: usize,
    pub(crate) failures: Vec<DynamicPriorityBoostReleaseFailure>,
}

struct ManagedDynamicPriorityBoost {
    baseline: DynamicPriorityBoostState,
    expected: DynamicPriorityBoostState,
    owner: ControlOwner,
    apply_sequence: u64,
}

pub(crate) trait DynamicPriorityBoostRecoveryIntent {
    fn commit(self) -> Result<(), String>;
    fn retain_pending(self) -> Result<(), String>;
}

impl DynamicPriorityBoostRecoveryIntent for RecoveryIntent {
    fn commit(self) -> Result<(), String> {
        RecoveryIntent::commit(self)
    }

    fn retain_pending(self) -> Result<(), String> {
        RecoveryIntent::retain_pending(self)
    }
}

pub(crate) trait DynamicPriorityBoostPlatform {
    type Process;
    type RecoveryIntent: DynamicPriorityBoostRecoveryIntent;

    fn open(
        &mut self,
        target: &ProcessControlTarget,
        allow_cross_session_process_control: bool,
    ) -> Result<(ProcessIdentity, Self::Process), ProcessControlError>;
    fn query(
        &mut self,
        process: &Self::Process,
    ) -> Result<DynamicPriorityBoostState, ProcessControlError>;
    fn begin_change(
        &mut self,
        process: &Self::Process,
        original: DynamicPriorityBoostState,
        expected: DynamicPriorityBoostState,
    ) -> Result<Self::RecoveryIntent, ProcessControlError>;
    fn apply(
        &mut self,
        process: &Self::Process,
        state: DynamicPriorityBoostState,
    ) -> Result<(), ProcessControlError>;
    fn relinquish(&mut self, identity: &ProcessIdentity) -> Result<(), ProcessControlError>;
}

#[derive(Default)]
pub(crate) struct WindowsDynamicPriorityBoostPlatform;

impl DynamicPriorityBoostPlatform for WindowsDynamicPriorityBoostPlatform {
    type Process = WinHandle;
    type RecoveryIntent = RecoveryIntent;

    fn open(
        &mut self,
        target: &ProcessControlTarget,
        allow_cross_session_process_control: bool,
    ) -> Result<(ProcessIdentity, Self::Process), ProcessControlError> {
        open_process_for_set_information(target, allow_cross_session_process_control)
    }

    fn query(
        &mut self,
        process: &Self::Process,
    ) -> Result<DynamicPriorityBoostState, ProcessControlError> {
        windows_dynamic_priority_boost::query_disabled(process)
            .map(DynamicPriorityBoostState::from_disabled)
            .map_err(ProcessControlError::from)
    }

    fn begin_change(
        &mut self,
        process: &Self::Process,
        original: DynamicPriorityBoostState,
        expected: DynamicPriorityBoostState,
    ) -> Result<Self::RecoveryIntent, ProcessControlError> {
        record_process_change(
            process.raw(),
            ProcessValue::DynamicPriorityBoostDisabled(original.disabled()),
            ProcessValue::DynamicPriorityBoostDisabled(expected.disabled()),
        )
        .map_err(ProcessControlError::Failed)
    }

    fn apply(
        &mut self,
        process: &Self::Process,
        state: DynamicPriorityBoostState,
    ) -> Result<(), ProcessControlError> {
        windows_dynamic_priority_boost::set_disabled(process, state.disabled())
            .map_err(ProcessControlError::from)
    }

    fn relinquish(&mut self, identity: &ProcessIdentity) -> Result<(), ProcessControlError> {
        forget_dynamic_priority_boost_change(identity.id, identity.creation_time)
            .map_err(ProcessControlError::Failed)
    }
}

pub(crate) struct DynamicPriorityBoostController<
    P: DynamicPriorityBoostPlatform = WindowsDynamicPriorityBoostPlatform,
> {
    platform: P,
    managed: BTreeMap<ProcessIdentity, ManagedDynamicPriorityBoost>,
    next_apply_sequence: u64,
    pending_releases: BTreeSet<ProcessIdentity>,
    next_release_retry: Option<Instant>,
}

impl Default for DynamicPriorityBoostController {
    fn default() -> Self {
        Self::with_platform(WindowsDynamicPriorityBoostPlatform)
    }
}

impl<P: DynamicPriorityBoostPlatform> DynamicPriorityBoostController<P> {
    fn with_platform(platform: P) -> Self {
        Self {
            platform,
            managed: BTreeMap::new(),
            next_apply_sequence: 1,
            pending_releases: BTreeSet::new(),
            next_release_retry: None,
        }
    }

    pub(crate) fn apply_policy_claim(
        &mut self,
        claim: DynamicPriorityBoostClaim,
        allow_cross_session_process_control: bool,
    ) -> Result<DynamicPriorityBoostApplyOutcome, ProcessControlError> {
        if !matches!(
            claim.owner,
            ControlOwner::DynamicPriorityBoost | ControlOwner::AdaptiveEngine
        ) {
            return Err(ProcessControlError::Failed(
                "This owner cannot control Dynamic Priority Boost policy.".to_owned(),
            ));
        }
        self.apply_claim(claim, allow_cross_session_process_control)
    }

    pub(crate) fn apply_process_list_action(
        &mut self,
        target: &ProcessActionTarget,
        state: DynamicPriorityBoostState,
        allow_cross_session_process_control: bool,
    ) -> Result<DynamicPriorityBoostApplyOutcome, ProcessControlError> {
        self.apply_claim(
            DynamicPriorityBoostClaim {
                target: ProcessControlTarget::from_action_target(target),
                owner: ControlOwner::ProcessList,
                state,
            },
            allow_cross_session_process_control,
        )
    }

    fn apply_claim(
        &mut self,
        claim: DynamicPriorityBoostClaim,
        allow_cross_session_process_control: bool,
    ) -> Result<DynamicPriorityBoostApplyOutcome, ProcessControlError> {
        let (identity, process) = self
            .platform
            .open(&claim.target, allow_cross_session_process_control)?;
        let stale_identities = self
            .managed
            .keys()
            .filter(|managed_identity| {
                managed_identity.id == identity.id && **managed_identity != identity
            })
            .cloned()
            .collect::<Vec<_>>();
        for stale_identity in stale_identities {
            self.platform.relinquish(&stale_identity)?;
            self.managed.remove(&stale_identity);
            self.pending_releases.remove(&stale_identity);
        }

        let mut managed = self.managed.remove(&identity);
        let current = match self.platform.query(&process) {
            Ok(current) => current,
            Err(error) => {
                if let Some(managed) = managed {
                    self.managed.insert(identity, managed);
                }
                return Err(error);
            }
        };
        if managed
            .as_ref()
            .is_some_and(|managed| managed.expected != current)
        {
            if let Err(error) = self.platform.relinquish(&identity) {
                if let Some(managed) = managed {
                    self.managed.insert(identity, managed);
                }
                return Err(error);
            }
            self.pending_releases.remove(&identity);
            managed = None;
        }

        if current == claim.state {
            if let Some(mut managed) = managed {
                if claim.owner.is_automatic() || managed.owner == ControlOwner::ProcessList {
                    managed.owner = claim.owner;
                    self.pending_releases.remove(&identity);
                }
                managed.expected = current;
                self.managed.insert(identity, managed);
            }
            return Ok(DynamicPriorityBoostApplyOutcome::Unchanged);
        }

        let baseline = managed.as_ref().map_or(current, |managed| managed.baseline);
        let sequence = self.next_apply_sequence;
        self.next_apply_sequence = self.next_apply_sequence.wrapping_add(1).max(1);
        match apply_transition(
            &mut self.platform,
            &process,
            current,
            claim.state,
            CommitFailureBehavior::RetainOwnership,
        ) {
            Ok(()) => {
                self.pending_releases.remove(&identity);
                self.managed.insert(
                    identity,
                    ManagedDynamicPriorityBoost {
                        baseline,
                        expected: claim.state,
                        owner: claim.owner,
                        apply_sequence: sequence,
                    },
                );
                Ok(DynamicPriorityBoostApplyOutcome::Applied)
            }
            Err(mut failure) => {
                if failure.relinquish_recovery {
                    if let Err(error) = self.platform.relinquish(&identity) {
                        failure.error = ProcessControlError::Failed(format!(
                            "{} Recovery journal relinquish failed: {error}.",
                            failure.error
                        ));
                        failure.uncertain = true;
                    }
                }
                if failure.uncertain {
                    self.managed.insert(
                        identity,
                        ManagedDynamicPriorityBoost {
                            baseline,
                            expected: claim.state,
                            owner: claim.owner,
                            apply_sequence: sequence,
                        },
                    );
                } else if let Some(managed) = managed {
                    self.managed.insert(identity, managed);
                }
                Err(failure.error)
            }
        }
    }

    pub(crate) fn release_policy_except(
        &mut self,
        active_targets: &BTreeSet<ProcessTargetKey>,
    ) -> DynamicPriorityBoostReleaseSummary {
        if self.pending_releases.is_empty() {
            self.next_release_retry = None;
        }
        let identities = self
            .managed
            .iter()
            .filter(|(identity, managed)| {
                managed.owner.is_automatic() && !active_targets.contains(&identity.key())
            })
            .map(|(identity, managed)| (managed.apply_sequence, identity.clone()))
            .collect::<Vec<_>>();
        self.pending_releases
            .extend(identities.iter().map(|(_, identity)| identity.clone()));
        let summary = self.release_identities(identities);
        if self.pending_releases.is_empty() {
            self.next_release_retry = None;
        } else {
            self.next_release_retry
                .get_or_insert_with(|| Instant::now() + Duration::from_secs(1));
        }
        summary
    }

    pub(crate) fn release_retry_delay(&self, now: Instant) -> Option<Duration> {
        (!self.pending_releases.is_empty()).then(|| {
            self.next_release_retry
                .map_or(Duration::ZERO, |due| due.saturating_duration_since(now))
        })
    }

    pub(crate) fn retry_pending_releases(
        &mut self,
        now: Instant,
    ) -> DynamicPriorityBoostReleaseSummary {
        if self.release_retry_delay(now) != Some(Duration::ZERO) {
            return DynamicPriorityBoostReleaseSummary::default();
        }
        let identities = self
            .pending_releases
            .iter()
            .filter_map(|identity| {
                self.managed
                    .get(identity)
                    .map(|managed| (managed.apply_sequence, identity.clone()))
            })
            .collect();
        let summary = self.release_identities(identities);
        self.pending_releases
            .retain(|identity| self.managed.contains_key(identity));
        self.next_release_retry =
            (!self.pending_releases.is_empty()).then(|| now + Duration::from_secs(1));
        summary
    }

    pub(crate) fn release_all_policy(&mut self) -> DynamicPriorityBoostReleaseSummary {
        self.release_policy_except(&BTreeSet::new())
    }

    pub(crate) fn shutdown(&mut self) -> Result<(), String> {
        let identities = self
            .managed
            .iter()
            .map(|(identity, managed)| (managed.apply_sequence, identity.clone()))
            .collect::<Vec<_>>();
        let summary = self.release_identities(identities);
        if summary.failures.is_empty() {
            Ok(())
        } else {
            Err(summary
                .failures
                .into_iter()
                .map(|failure| {
                    format!(
                        "{} ({}): {}",
                        failure.process_name, failure.process_id, failure.error
                    )
                })
                .collect::<Vec<_>>()
                .join("; "))
        }
    }

    fn release_identities(
        &mut self,
        mut identities: Vec<(u64, ProcessIdentity)>,
    ) -> DynamicPriorityBoostReleaseSummary {
        identities.sort_by_key(|identity| std::cmp::Reverse(identity.0));
        let mut summary = DynamicPriorityBoostReleaseSummary::default();
        for (_, identity) in identities {
            let result = self.release_identity(&identity).or_else(|error| {
                if error == ProcessControlError::ProcessExited {
                    self.platform.relinquish(&identity).map(|()| false)
                } else {
                    Err(error)
                }
            });
            match result {
                Ok(restored) => {
                    summary.restored_processes += usize::from(restored);
                    self.managed.remove(&identity);
                    self.pending_releases.remove(&identity);
                }
                Err(error) => summary.failures.push(DynamicPriorityBoostReleaseFailure {
                    process_id: identity.id,
                    process_name: identity.name.clone(),
                    executable_path: identity.executable_path.to_string_lossy().into_owned(),
                    error,
                }),
            }
        }
        summary
    }

    fn release_identity(
        &mut self,
        identity: &ProcessIdentity,
    ) -> Result<bool, ProcessControlError> {
        let Some(managed) = self.managed.get(identity) else {
            return Ok(false);
        };
        let target = identity.target();
        let (_, process) = self.platform.open(&target, true)?;
        let current = self.platform.query(&process)?;
        if current != managed.expected || current == managed.baseline {
            self.platform.relinquish(identity)?;
            return Ok(false);
        }
        let baseline = managed.baseline;
        match apply_transition(
            &mut self.platform,
            &process,
            current,
            baseline,
            CommitFailureBehavior::RelinquishRestored,
        ) {
            Ok(()) => Ok(true),
            // Preserve a confirmed exit; otherwise recheck for exit during restoration.
            Err(failure)
                if failure.error == ProcessControlError::ProcessExited
                    || matches!(
                        self.platform.open(&identity.target(), true),
                        Err(ProcessControlError::ProcessExited)
                    ) =>
            {
                Err(ProcessControlError::ProcessExited)
            }
            Err(failure) if failure.expected_preserved => {
                match self.platform.relinquish(identity) {
                    Ok(()) => Ok(true),
                    Err(error) => {
                        if let Some(managed) = self.managed.get_mut(identity) {
                            managed.expected = baseline;
                        }
                        Err(ProcessControlError::Failed(format!(
                            "{} Recovery journal relinquish failed: {error}.",
                            failure.error
                        )))
                    }
                }
            }
            Err(failure) => Err(failure.error),
        }
    }

    pub(crate) fn policy_managed_process_names(&self) -> Vec<String> {
        self.managed
            .iter()
            .filter(|(_, managed)| managed.owner.is_automatic())
            .map(|(identity, _)| identity.name.clone())
            .collect()
    }

    pub(crate) fn has_managed_state(&self) -> bool {
        !self.managed.is_empty()
    }
}

impl<P: DynamicPriorityBoostPlatform> Drop for DynamicPriorityBoostController<P> {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

struct TransitionFailure {
    error: ProcessControlError,
    uncertain: bool,
    relinquish_recovery: bool,
    expected_preserved: bool,
}

#[derive(Clone, Copy)]
enum CommitFailureBehavior {
    RetainOwnership,
    RelinquishRestored,
}

fn apply_transition<P: DynamicPriorityBoostPlatform>(
    platform: &mut P,
    process: &P::Process,
    original: DynamicPriorityBoostState,
    expected: DynamicPriorityBoostState,
    commit_failure_behavior: CommitFailureBehavior,
) -> Result<(), TransitionFailure> {
    let intent = platform
        .begin_change(process, original, expected)
        .map_err(|error| TransitionFailure {
            error,
            uncertain: false,
            relinquish_recovery: false,
            expected_preserved: false,
        })?;

    if let Err(error) = platform.apply(process, expected) {
        return Err(compensate_with_intent(
            platform, process, original, intent, error,
        ));
    }
    match platform.query(process) {
        Ok(actual) if actual == expected => {}
        Ok(actual) => {
            return Err(compensate_with_intent(
                platform,
                process,
                original,
                intent,
                ProcessControlError::Failed(format!(
                    "Dynamic Priority Boost verification returned {actual:?}, expected {expected:?}."
                )),
            ));
        }
        Err(error) => {
            return Err(compensate_with_intent(
                platform, process, original, intent, error,
            ));
        }
    }

    if let Err(error) = intent.commit() {
        let message = format!("Crash recovery commit failed: {error}");
        // The helper may have compacted this transition into an earlier baseline.
        // Keep the verified target so both pending and committed journals can unwind it.
        return match commit_failure_behavior {
            CommitFailureBehavior::RetainOwnership => Err(TransitionFailure {
                error: ProcessControlError::Failed(message),
                uncertain: true,
                relinquish_recovery: false,
                expected_preserved: true,
            }),
            CommitFailureBehavior::RelinquishRestored => Err(TransitionFailure {
                error: ProcessControlError::Failed(message),
                uncertain: false,
                relinquish_recovery: true,
                expected_preserved: true,
            }),
        };
    }
    Ok(())
}

fn compensate_with_intent<P: DynamicPriorityBoostPlatform>(
    platform: &mut P,
    process: &P::Process,
    original: DynamicPriorityBoostState,
    intent: P::RecoveryIntent,
    primary_error: ProcessControlError,
) -> TransitionFailure {
    match restore_and_verify(platform, process, original) {
        Ok(()) => TransitionFailure {
            error: primary_error,
            uncertain: false,
            relinquish_recovery: false,
            expected_preserved: false,
        },
        Err(compensation_error) => {
            let recovery_error = intent.retain_pending().err();
            TransitionFailure {
                error: transition_failure_error(primary_error, compensation_error, recovery_error),
                uncertain: true,
                relinquish_recovery: false,
                expected_preserved: false,
            }
        }
    }
}

fn restore_and_verify<P: DynamicPriorityBoostPlatform>(
    platform: &mut P,
    process: &P::Process,
    original: DynamicPriorityBoostState,
) -> Result<(), ProcessControlError> {
    platform.apply(process, original)?;
    let actual = platform.query(process)?;
    if actual == original {
        Ok(())
    } else {
        Err(ProcessControlError::Failed(format!(
            "Compensation returned {actual:?}, expected {original:?}."
        )))
    }
}

pub(crate) fn current_dynamic_priority_boost_state(
    target: &ProcessActionTarget,
) -> Result<DynamicPriorityBoostState, String> {
    let mut platform = WindowsDynamicPriorityBoostPlatform;
    let (_, process) = platform
        .open(&ProcessControlTarget::from_action_target(target), true)
        .map_err(|error| error.to_string())?;
    platform.query(&process).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests;
