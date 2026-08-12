use std::collections::{BTreeMap, BTreeSet};

use crate::{
    backend::crash_recovery::{
        forget_dynamic_priority_boost_change, record_process_change, ProcessValue, RecoveryIntent,
    },
    foreground::ProcessActionTarget,
    platform::windows::dynamic_priority_boost as windows_dynamic_priority_boost,
    win_util::WinHandle,
};

use super::process::{
    open_process_for_set_information, ControlOwner, ProcessControlError, ProcessControlTarget,
    ProcessIdentity, ProcessTargetKey,
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
}

impl DynamicPriorityBoostRecoveryIntent for RecoveryIntent {
    fn commit(self) -> Result<(), String> {
        RecoveryIntent::commit(self)
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
            managed = None;
        }

        if current == claim.state {
            if let Some(mut managed) = managed {
                if claim.owner.is_automatic() || managed.owner == ControlOwner::ProcessList {
                    managed.owner = claim.owner;
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
            CommitFailureBehavior::Compensate,
        ) {
            Ok(()) => {
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
                        failure
                            .message
                            .push_str(&format!(" Recovery journal relinquish failed: {error}."));
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
                Err(ProcessControlError::Failed(failure.message))
            }
        }
    }

    pub(crate) fn release_policy_except(
        &mut self,
        active_targets: &BTreeSet<ProcessTargetKey>,
    ) -> DynamicPriorityBoostReleaseSummary {
        let identities = self
            .managed
            .iter()
            .filter(|(identity, managed)| {
                managed.owner.is_automatic() && !active_targets.contains(&identity.key())
            })
            .map(|(identity, managed)| (managed.apply_sequence, identity.clone()))
            .collect::<Vec<_>>();
        self.release_identities(identities)
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
        identities.sort_by(|left, right| right.0.cmp(&left.0));
        let mut summary = DynamicPriorityBoostReleaseSummary::default();
        for (_, identity) in identities {
            match self.release_identity(&identity) {
                Ok(restored) => {
                    summary.restored_processes += usize::from(restored);
                    self.managed.remove(&identity);
                }
                Err(ProcessControlError::ProcessExited) => {
                    match self.platform.relinquish(&identity) {
                        Ok(()) => {
                            self.managed.remove(&identity);
                        }
                        Err(error) => summary.failures.push(DynamicPriorityBoostReleaseFailure {
                            process_id: identity.id,
                            process_name: identity.name.clone(),
                            executable_path: identity
                                .executable_path
                                .to_string_lossy()
                                .into_owned(),
                            error,
                        }),
                    }
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
            CommitFailureBehavior::KeepExpected,
        ) {
            Ok(()) => Ok(true),
            Err(failure) if failure.expected_preserved => {
                match self.platform.relinquish(identity) {
                    Ok(()) => Ok(true),
                    Err(error) => {
                        if let Some(managed) = self.managed.get_mut(identity) {
                            managed.expected = baseline;
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
            message: error.to_string(),
            uncertain: false,
            relinquish_recovery: false,
            expected_preserved: false,
        })?;

    if let Err(error) = platform.apply(process, expected) {
        return Err(compensate_with_intent(
            platform,
            process,
            original,
            intent,
            error.to_string(),
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
                format!(
                    "Dynamic Priority Boost verification returned {actual:?}, expected {expected:?}."
                ),
            ));
        }
        Err(error) => {
            return Err(compensate_with_intent(
                platform,
                process,
                original,
                intent,
                format!("Dynamic Priority Boost verification failed: {error}"),
            ));
        }
    }

    if let Err(error) = intent.commit() {
        let message = format!("Crash recovery commit failed: {error}");
        return match commit_failure_behavior {
            CommitFailureBehavior::Compensate => Err(compensate_without_intent(
                platform, process, original, message,
            )),
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

fn compensate_with_intent<P: DynamicPriorityBoostPlatform>(
    platform: &mut P,
    process: &P::Process,
    original: DynamicPriorityBoostState,
    intent: P::RecoveryIntent,
    primary_error: String,
) -> TransitionFailure {
    match restore_and_verify(platform, process, original) {
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

fn compensate_without_intent<P: DynamicPriorityBoostPlatform>(
    platform: &mut P,
    process: &P::Process,
    original: DynamicPriorityBoostState,
    primary_error: String,
) -> TransitionFailure {
    match restore_and_verify(platform, process, original) {
        Ok(()) => TransitionFailure {
            message: primary_error,
            uncertain: false,
            relinquish_recovery: true,
            expected_preserved: false,
        },
        Err(compensation_error) => TransitionFailure {
            message: transition_failure_message(
                primary_error,
                compensation_error.to_string(),
                None,
            ),
            uncertain: true,
            relinquish_recovery: false,
            expected_preserved: false,
        },
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

fn transition_failure_message(
    primary_error: String,
    compensation_error: String,
    recovery_error: Option<String>,
) -> String {
    let mut message = format!("{primary_error} Compensation failed: {compensation_error}.");
    if let Some(recovery_error) = recovery_error {
        message.push_str(&format!(" Recovery commit also failed: {recovery_error}."));
    }
    message
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
mod tests {
    use std::{
        collections::VecDeque,
        path::PathBuf,
        process::{Child, Command},
        sync::{Arc, Mutex},
        time::{Duration, Instant},
    };

    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum FakeFailure {
        Open,
        VerificationQuery,
        Begin,
        Apply,
        Commit,
        Relinquish,
    }

    struct FakeRecoveryIntent {
        fail: bool,
        events: Arc<Mutex<Vec<String>>>,
    }

    impl DynamicPriorityBoostRecoveryIntent for FakeRecoveryIntent {
        fn commit(self) -> Result<(), String> {
            self.events.lock().unwrap().push("commit".to_owned());
            if self.fail {
                Err("commit failed".to_owned())
            } else {
                Ok(())
            }
        }
    }

    #[derive(Clone)]
    struct FakeProcess {
        identity: ProcessIdentity,
        state: DynamicPriorityBoostState,
    }

    struct FakePlatform {
        processes: BTreeMap<u32, FakeProcess>,
        failures: VecDeque<FakeFailure>,
        events: Arc<Mutex<Vec<String>>>,
    }

    impl FakePlatform {
        fn new(state: DynamicPriorityBoostState) -> Self {
            let identity = identity(42, 7);
            Self {
                processes: BTreeMap::from([(42, FakeProcess { identity, state })]),
                failures: VecDeque::new(),
                events: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn fail_next(&mut self, failure: FakeFailure) {
            self.failures.push_back(failure);
        }

        fn take_failure(&mut self, failure: FakeFailure) -> bool {
            if self.failures.front() == Some(&failure) {
                self.failures.pop_front();
                true
            } else {
                false
            }
        }
    }

    impl DynamicPriorityBoostPlatform for FakePlatform {
        type Process = u32;
        type RecoveryIntent = FakeRecoveryIntent;

        fn open(
            &mut self,
            target: &ProcessControlTarget,
            _: bool,
        ) -> Result<(ProcessIdentity, Self::Process), ProcessControlError> {
            self.events.lock().unwrap().push("open".to_owned());
            if self.take_failure(FakeFailure::Open) {
                return Err(ProcessControlError::AccessDenied("denied".to_owned()));
            }
            let process = self
                .processes
                .get(&target.id)
                .ok_or(ProcessControlError::ProcessExited)?;
            if target.creation_time != process.identity.creation_time {
                return Err(ProcessControlError::ProcessExited);
            }
            Ok((process.identity.clone(), target.id))
        }

        fn query(
            &mut self,
            process: &Self::Process,
        ) -> Result<DynamicPriorityBoostState, ProcessControlError> {
            self.events.lock().unwrap().push("query".to_owned());
            let state = self
                .processes
                .get(process)
                .map(|process| process.state)
                .ok_or(ProcessControlError::ProcessExited)?;
            if state == DynamicPriorityBoostState::Disabled
                && self.take_failure(FakeFailure::VerificationQuery)
            {
                return Err(ProcessControlError::Failed("query failed".to_owned()));
            }
            Ok(state)
        }

        fn begin_change(
            &mut self,
            _: &Self::Process,
            _: DynamicPriorityBoostState,
            _: DynamicPriorityBoostState,
        ) -> Result<Self::RecoveryIntent, ProcessControlError> {
            self.events.lock().unwrap().push("begin".to_owned());
            if self.take_failure(FakeFailure::Begin) {
                return Err(ProcessControlError::Failed("begin failed".to_owned()));
            }
            let fail = self.take_failure(FakeFailure::Commit);
            Ok(FakeRecoveryIntent {
                fail,
                events: Arc::clone(&self.events),
            })
        }

        fn apply(
            &mut self,
            process: &Self::Process,
            state: DynamicPriorityBoostState,
        ) -> Result<(), ProcessControlError> {
            self.events.lock().unwrap().push(format!("apply:{state:?}"));
            if self.take_failure(FakeFailure::Apply) {
                return Err(ProcessControlError::Failed("apply failed".to_owned()));
            }
            self.processes
                .get_mut(process)
                .ok_or(ProcessControlError::ProcessExited)?
                .state = state;
            Ok(())
        }

        fn relinquish(&mut self, identity: &ProcessIdentity) -> Result<(), ProcessControlError> {
            self.events.lock().unwrap().push(format!(
                "relinquish:{}:{}",
                identity.id, identity.creation_time
            ));
            if self.take_failure(FakeFailure::Relinquish) {
                Err(ProcessControlError::Failed("relinquish failed".to_owned()))
            } else {
                Ok(())
            }
        }
    }

    fn identity(id: u32, creation_time: u64) -> ProcessIdentity {
        ProcessIdentity::new(
            id,
            "app.exe".to_owned(),
            PathBuf::from(r"C:\Apps\app.exe"),
            creation_time,
            Some(1),
        )
    }

    fn claim(owner: ControlOwner, state: DynamicPriorityBoostState) -> DynamicPriorityBoostClaim {
        claim_at(owner, state, 7)
    }

    fn claim_at(
        owner: ControlOwner,
        state: DynamicPriorityBoostState,
        creation_time: u64,
    ) -> DynamicPriorityBoostClaim {
        DynamicPriorityBoostClaim {
            target: ProcessControlTarget::automatic(
                42,
                "app.exe".to_owned(),
                PathBuf::from(r"C:\Apps\app.exe"),
                creation_time,
            ),
            owner,
            state,
        }
    }

    #[test]
    fn unmanaged_matching_state_is_not_adopted() {
        let platform = FakePlatform::new(DynamicPriorityBoostState::Enabled);
        let mut controller = DynamicPriorityBoostController::with_platform(platform);

        assert_eq!(
            controller
                .apply_policy_claim(
                    claim(
                        ControlOwner::DynamicPriorityBoost,
                        DynamicPriorityBoostState::Enabled,
                    ),
                    true,
                )
                .unwrap(),
            DynamicPriorityBoostApplyOutcome::Unchanged
        );
        assert!(!controller.has_managed_state());
    }

    #[test]
    fn manual_action_is_superseded_by_the_next_automatic_claim() {
        let platform = FakePlatform::new(DynamicPriorityBoostState::Enabled);
        let mut controller = DynamicPriorityBoostController::with_platform(platform);
        let target = ProcessActionTarget {
            id: 42,
            name: "app.exe".to_owned(),
            executable_path: PathBuf::from(r"C:\Apps\app.exe"),
            creation_time: 7,
            session_id: Some(1),
            is_service_account: Some(false),
        };

        controller
            .apply_process_list_action(&target, DynamicPriorityBoostState::Disabled, true)
            .unwrap();
        controller
            .apply_policy_claim(
                claim(
                    ControlOwner::DynamicPriorityBoost,
                    DynamicPriorityBoostState::Enabled,
                ),
                true,
            )
            .unwrap();

        let managed = controller.managed.values().next().unwrap();
        assert_eq!(managed.owner, ControlOwner::DynamicPriorityBoost);
        assert_eq!(managed.baseline, DynamicPriorityBoostState::Enabled);
        assert_eq!(
            controller.platform.processes[&42].state,
            DynamicPriorityBoostState::Enabled
        );
    }

    #[test]
    fn static_to_adaptive_replacement_preserves_the_first_baseline() {
        let platform = FakePlatform::new(DynamicPriorityBoostState::Enabled);
        let mut controller = DynamicPriorityBoostController::with_platform(platform);
        controller
            .apply_policy_claim(
                claim(
                    ControlOwner::DynamicPriorityBoost,
                    DynamicPriorityBoostState::Disabled,
                ),
                true,
            )
            .unwrap();
        controller
            .apply_policy_claim(
                claim(
                    ControlOwner::AdaptiveEngine,
                    DynamicPriorityBoostState::Enabled,
                ),
                true,
            )
            .unwrap();

        let managed = controller.managed.values().next().unwrap();
        assert_eq!(managed.owner, ControlOwner::AdaptiveEngine);
        assert_eq!(managed.baseline, DynamicPriorityBoostState::Enabled);
        assert_eq!(managed.expected, DynamicPriorityBoostState::Enabled);
    }

    #[test]
    fn external_break_rebases_before_automatic_policy_is_reasserted() {
        let platform = FakePlatform::new(DynamicPriorityBoostState::Enabled);
        let mut controller = DynamicPriorityBoostController::with_platform(platform);
        controller
            .apply_policy_claim(
                claim(
                    ControlOwner::DynamicPriorityBoost,
                    DynamicPriorityBoostState::Disabled,
                ),
                true,
            )
            .unwrap();
        controller.platform.processes.get_mut(&42).unwrap().state =
            DynamicPriorityBoostState::Enabled;

        controller
            .apply_policy_claim(
                claim(
                    ControlOwner::DynamicPriorityBoost,
                    DynamicPriorityBoostState::Disabled,
                ),
                true,
            )
            .unwrap();

        assert_eq!(
            controller.managed.values().next().unwrap().baseline,
            DynamicPriorityBoostState::Enabled
        );
        assert!(controller
            .platform
            .events
            .lock()
            .unwrap()
            .iter()
            .any(|event| event == "relinquish:42:7"));
    }

    #[test]
    fn failed_external_break_relinquish_retains_the_managed_chain() {
        let platform = FakePlatform::new(DynamicPriorityBoostState::Enabled);
        let mut controller = DynamicPriorityBoostController::with_platform(platform);
        controller
            .apply_policy_claim(
                claim(
                    ControlOwner::DynamicPriorityBoost,
                    DynamicPriorityBoostState::Disabled,
                ),
                true,
            )
            .unwrap();
        controller.platform.processes.get_mut(&42).unwrap().state =
            DynamicPriorityBoostState::Enabled;
        controller.platform.fail_next(FakeFailure::Relinquish);

        assert!(controller
            .apply_policy_claim(
                claim(
                    ControlOwner::DynamicPriorityBoost,
                    DynamicPriorityBoostState::Disabled,
                ),
                true,
            )
            .is_err());
        assert!(controller.has_managed_state());
    }

    #[test]
    fn identity_metadata_drift_preserves_the_existing_baseline() {
        let platform = FakePlatform::new(DynamicPriorityBoostState::Enabled);
        let mut controller = DynamicPriorityBoostController::with_platform(platform);
        controller
            .apply_policy_claim(
                claim(
                    ControlOwner::DynamicPriorityBoost,
                    DynamicPriorityBoostState::Disabled,
                ),
                true,
            )
            .unwrap();
        let identity = &mut controller.platform.processes.get_mut(&42).unwrap().identity;
        identity.name = "APP.EXE".to_owned();
        identity.executable_path = PathBuf::from(r"c:/apps/APP.exe");
        identity.session_id = None;

        controller
            .apply_policy_claim(
                claim(
                    ControlOwner::AdaptiveEngine,
                    DynamicPriorityBoostState::Enabled,
                ),
                true,
            )
            .unwrap();

        let managed = controller.managed.values().next().unwrap();
        assert_eq!(managed.baseline, DynamicPriorityBoostState::Enabled);
        assert!(!controller
            .platform
            .events
            .lock()
            .unwrap()
            .iter()
            .any(|event| event.starts_with("relinquish:")));
    }

    #[test]
    fn external_break_is_not_overwritten_during_release() {
        let platform = FakePlatform::new(DynamicPriorityBoostState::Enabled);
        let mut controller = DynamicPriorityBoostController::with_platform(platform);
        controller
            .apply_policy_claim(
                claim(
                    ControlOwner::DynamicPriorityBoost,
                    DynamicPriorityBoostState::Disabled,
                ),
                true,
            )
            .unwrap();
        controller.platform.processes.get_mut(&42).unwrap().state =
            DynamicPriorityBoostState::Enabled;

        assert!(controller.release_all_policy().failures.is_empty());
        assert!(!controller.has_managed_state());
        assert_eq!(
            controller.platform.processes[&42].state,
            DynamicPriorityBoostState::Enabled
        );
    }

    #[test]
    fn process_exit_and_pid_reuse_drop_the_old_identity() {
        let platform = FakePlatform::new(DynamicPriorityBoostState::Enabled);
        let mut controller = DynamicPriorityBoostController::with_platform(platform);
        controller
            .apply_policy_claim(
                claim(
                    ControlOwner::DynamicPriorityBoost,
                    DynamicPriorityBoostState::Disabled,
                ),
                true,
            )
            .unwrap();
        controller.platform.processes.get_mut(&42).unwrap().identity = identity(42, 8);

        controller
            .apply_policy_claim(
                claim_at(
                    ControlOwner::DynamicPriorityBoost,
                    DynamicPriorityBoostState::Enabled,
                    8,
                ),
                true,
            )
            .unwrap();

        assert_eq!(controller.managed.len(), 1);
        assert_eq!(controller.managed.keys().next().unwrap().creation_time, 8);
    }

    #[test]
    fn begin_failure_blocks_the_mutation() {
        let mut platform = FakePlatform::new(DynamicPriorityBoostState::Enabled);
        platform.fail_next(FakeFailure::Begin);
        let mut controller = DynamicPriorityBoostController::with_platform(platform);

        assert!(controller
            .apply_policy_claim(
                claim(
                    ControlOwner::DynamicPriorityBoost,
                    DynamicPriorityBoostState::Disabled,
                ),
                true,
            )
            .is_err());
        assert_eq!(
            controller.platform.processes[&42].state,
            DynamicPriorityBoostState::Enabled
        );
        assert!(!controller.has_managed_state());
    }

    #[test]
    fn apply_failure_compensates_and_cancels_the_intent() {
        let mut platform = FakePlatform::new(DynamicPriorityBoostState::Enabled);
        platform.fail_next(FakeFailure::Apply);
        let mut controller = DynamicPriorityBoostController::with_platform(platform);

        assert!(controller
            .apply_policy_claim(
                claim(
                    ControlOwner::DynamicPriorityBoost,
                    DynamicPriorityBoostState::Disabled,
                ),
                true,
            )
            .is_err());
        assert_eq!(
            controller.platform.processes[&42].state,
            DynamicPriorityBoostState::Enabled
        );
        assert!(!controller.has_managed_state());
    }

    #[test]
    fn verification_failure_compensates() {
        let mut platform = FakePlatform::new(DynamicPriorityBoostState::Enabled);
        platform.fail_next(FakeFailure::VerificationQuery);
        let mut controller = DynamicPriorityBoostController::with_platform(platform);

        assert!(controller
            .apply_policy_claim(
                claim(
                    ControlOwner::DynamicPriorityBoost,
                    DynamicPriorityBoostState::Disabled,
                ),
                true,
            )
            .is_err());
        assert_eq!(
            controller.platform.processes[&42].state,
            DynamicPriorityBoostState::Enabled
        );
        assert!(!controller.has_managed_state());
    }

    #[test]
    fn failed_compensation_keeps_uncertain_state_for_later_recovery() {
        let mut platform = FakePlatform::new(DynamicPriorityBoostState::Enabled);
        platform.fail_next(FakeFailure::Apply);
        platform.fail_next(FakeFailure::Apply);
        let mut controller = DynamicPriorityBoostController::with_platform(platform);

        assert!(controller
            .apply_policy_claim(
                claim(
                    ControlOwner::DynamicPriorityBoost,
                    DynamicPriorityBoostState::Disabled,
                ),
                true,
            )
            .is_err());
        assert!(controller.has_managed_state());
    }

    #[test]
    fn commit_failure_compensates() {
        let mut platform = FakePlatform::new(DynamicPriorityBoostState::Enabled);
        platform.fail_next(FakeFailure::Commit);
        let mut controller = DynamicPriorityBoostController::with_platform(platform);

        assert!(controller
            .apply_policy_claim(
                claim(
                    ControlOwner::DynamicPriorityBoost,
                    DynamicPriorityBoostState::Disabled,
                ),
                true,
            )
            .is_err());
        assert_eq!(
            controller.platform.processes[&42].state,
            DynamicPriorityBoostState::Enabled
        );
        assert!(!controller.has_managed_state());
    }

    #[test]
    fn release_commit_failure_keeps_baseline_and_relinquishes_recovery() {
        let platform = FakePlatform::new(DynamicPriorityBoostState::Enabled);
        let mut controller = DynamicPriorityBoostController::with_platform(platform);
        controller
            .apply_policy_claim(
                claim(
                    ControlOwner::DynamicPriorityBoost,
                    DynamicPriorityBoostState::Disabled,
                ),
                true,
            )
            .unwrap();
        controller.platform.fail_next(FakeFailure::Commit);

        let summary = controller.release_all_policy();

        assert!(summary.failures.is_empty());
        assert_eq!(summary.restored_processes, 1);
        assert!(!controller.has_managed_state());
        assert_eq!(
            controller.platform.processes[&42].state,
            DynamicPriorityBoostState::Enabled
        );
        assert!(controller
            .platform
            .events
            .lock()
            .unwrap()
            .iter()
            .any(|event| event == "relinquish:42:7"));
    }

    #[test]
    fn failed_release_relinquish_is_retried_without_reapplying() {
        let platform = FakePlatform::new(DynamicPriorityBoostState::Enabled);
        let mut controller = DynamicPriorityBoostController::with_platform(platform);
        controller
            .apply_policy_claim(
                claim(
                    ControlOwner::DynamicPriorityBoost,
                    DynamicPriorityBoostState::Disabled,
                ),
                true,
            )
            .unwrap();
        controller.platform.fail_next(FakeFailure::Commit);
        controller.platform.fail_next(FakeFailure::Relinquish);

        let first = controller.release_all_policy();
        assert_eq!(first.failures.len(), 1);
        assert!(controller.has_managed_state());
        assert_eq!(
            controller.platform.processes[&42].state,
            DynamicPriorityBoostState::Enabled
        );

        let second = controller.release_all_policy();
        assert!(second.failures.is_empty());
        assert!(!controller.has_managed_state());
    }

    #[test]
    fn shutdown_restores_all_managed_processes() {
        let platform = FakePlatform::new(DynamicPriorityBoostState::Enabled);
        let mut controller = DynamicPriorityBoostController::with_platform(platform);
        controller
            .apply_policy_claim(
                claim(
                    ControlOwner::DynamicPriorityBoost,
                    DynamicPriorityBoostState::Disabled,
                ),
                true,
            )
            .unwrap();

        controller.shutdown().unwrap();

        assert!(!controller.has_managed_state());
        assert_eq!(
            controller.platform.processes[&42].state,
            DynamicPriorityBoostState::Enabled
        );
    }

    #[test]
    #[ignore = "modifies and restores a disposable Windows process; run in explicit integration QA"]
    fn dynamic_priority_boost_live_apply_and_clean_release() -> Result<(), String> {
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
        let baseline = current_dynamic_priority_boost_state(&target)?;
        let expected = if baseline == DynamicPriorityBoostState::Enabled {
            DynamicPriorityBoostState::Disabled
        } else {
            DynamicPriorityBoostState::Enabled
        };
        let mut controller = DynamicPriorityBoostController::default();

        controller
            .apply_process_list_action(&target, expected, true)
            .map_err(|error| error.to_string())?;
        assert_eq!(current_dynamic_priority_boost_state(&target)?, expected);
        controller.shutdown()?;
        assert_eq!(current_dynamic_priority_boost_state(&target)?, baseline);
        Ok(())
    }
}
