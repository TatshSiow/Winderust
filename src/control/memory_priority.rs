use std::collections::{BTreeMap, BTreeSet};

use crate::{
    backend::crash_recovery::{
        forget_memory_priority_change, record_process_change, ProcessValue, RecoveryIntent,
    },
    config::ProcessMemoryPriority,
    foreground::ProcessActionTarget,
    platform::windows::{memory_priority as windows_memory_priority, ProcessOperationError},
    win_util::WinHandle,
};

use super::process::{
    open_process_for_set_information, ControlOwner, ProcessControlError, ProcessControlTarget,
    ProcessIdentity, ProcessTargetKey,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MemoryPriorityPreservation {
    Exact,
    PreserveHigher,
    PreserveLower,
}

#[derive(Debug, Clone)]
pub(crate) struct MemoryPriorityClaim {
    pub(crate) target: ProcessControlTarget,
    pub(crate) owner: ControlOwner,
    pub(crate) priority: ProcessMemoryPriority,
    pub(crate) preservation: MemoryPriorityPreservation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MemoryPriorityApplyOutcome {
    Applied,
    Unchanged,
    Preserved,
    Shadowed,
}

#[derive(Debug)]
pub(crate) struct MemoryPriorityReleaseFailure {
    pub(crate) process_id: u32,
    pub(crate) process_name: String,
    pub(crate) executable_path: String,
    pub(crate) error: ProcessControlError,
}

#[derive(Debug, Default)]
pub(crate) struct MemoryPriorityReleaseSummary {
    pub(crate) restored_processes: usize,
    pub(crate) failures: Vec<MemoryPriorityReleaseFailure>,
}

struct ManagedMemoryPriority {
    baseline: u32,
    expected: u32,
    owner: ControlOwner,
    apply_sequence: u64,
}

pub(crate) trait MemoryPriorityRecoveryIntent {
    fn commit(self) -> Result<(), String>;
}

impl MemoryPriorityRecoveryIntent for RecoveryIntent {
    fn commit(self) -> Result<(), String> {
        RecoveryIntent::commit(self)
    }
}

pub(crate) trait MemoryPriorityPlatform {
    type Process;
    type RecoveryIntent: MemoryPriorityRecoveryIntent;

    fn open(
        &mut self,
        target: &ProcessControlTarget,
        allow_cross_session_process_control: bool,
    ) -> Result<(ProcessIdentity, Self::Process), ProcessControlError>;
    fn query(&mut self, process: &Self::Process) -> Result<u32, ProcessControlError>;
    fn begin_change(
        &mut self,
        process: &Self::Process,
        original: u32,
        expected: u32,
    ) -> Result<Self::RecoveryIntent, ProcessControlError>;
    fn apply(&mut self, process: &Self::Process, priority: u32) -> Result<(), ProcessControlError>;
    fn relinquish(&mut self, identity: &ProcessIdentity) -> Result<(), ProcessControlError>;
}

#[derive(Default)]
pub(crate) struct WindowsMemoryPriorityPlatform;

impl MemoryPriorityPlatform for WindowsMemoryPriorityPlatform {
    type Process = WinHandle;
    type RecoveryIntent = RecoveryIntent;

    fn open(
        &mut self,
        target: &ProcessControlTarget,
        allow_cross_session_process_control: bool,
    ) -> Result<(ProcessIdentity, Self::Process), ProcessControlError> {
        open_process_for_set_information(target, allow_cross_session_process_control)
    }

    fn query(&mut self, process: &Self::Process) -> Result<u32, ProcessControlError> {
        windows_memory_priority::query(process).map_err(process_error)
    }

    fn begin_change(
        &mut self,
        process: &Self::Process,
        original: u32,
        expected: u32,
    ) -> Result<Self::RecoveryIntent, ProcessControlError> {
        record_process_change(
            process.raw(),
            ProcessValue::MemoryPriority(original),
            ProcessValue::MemoryPriority(expected),
        )
        .map_err(ProcessControlError::Failed)
    }

    fn apply(&mut self, process: &Self::Process, priority: u32) -> Result<(), ProcessControlError> {
        windows_memory_priority::set(process, priority).map_err(process_error)
    }

    fn relinquish(&mut self, identity: &ProcessIdentity) -> Result<(), ProcessControlError> {
        forget_memory_priority_change(identity.id, identity.creation_time)
            .map_err(ProcessControlError::Failed)
    }
}

pub(crate) struct MemoryPriorityController<
    P: MemoryPriorityPlatform = WindowsMemoryPriorityPlatform,
> {
    platform: P,
    static_claims: BTreeMap<ProcessTargetKey, MemoryPriorityClaim>,
    adaptive_claims: BTreeMap<ProcessTargetKey, MemoryPriorityClaim>,
    managed: BTreeMap<ProcessIdentity, ManagedMemoryPriority>,
    next_apply_sequence: u64,
}

impl Default for MemoryPriorityController {
    fn default() -> Self {
        Self::with_platform(WindowsMemoryPriorityPlatform)
    }
}

impl<P: MemoryPriorityPlatform> MemoryPriorityController<P> {
    fn with_platform(platform: P) -> Self {
        Self {
            platform,
            static_claims: BTreeMap::new(),
            adaptive_claims: BTreeMap::new(),
            managed: BTreeMap::new(),
            next_apply_sequence: 1,
        }
    }

    pub(crate) fn apply_policy_claim(
        &mut self,
        claim: MemoryPriorityClaim,
        allow_cross_session_process_control: bool,
    ) -> Result<MemoryPriorityApplyOutcome, ProcessControlError> {
        let owner = claim.owner;
        let key = claim.target.key();
        let previous = self.policy_claims_mut(owner)?.insert(key.clone(), claim);
        let effective = self.effective_claim(&key).cloned().ok_or_else(|| {
            ProcessControlError::Failed("Memory Priority claim arbitration failed.".to_owned())
        })?;
        if effective.owner != owner {
            return Ok(MemoryPriorityApplyOutcome::Shadowed);
        }
        let result = self.apply_effective_claim(effective, allow_cross_session_process_control);
        if result.is_err() {
            let claims = self.policy_claims_mut(owner)?;
            if let Some(previous) = previous {
                claims.insert(key, previous);
            } else {
                claims.remove(&key);
            }
        }
        result
    }

    pub(crate) fn apply_process_list_action(
        &mut self,
        target: &ProcessActionTarget,
        priority: ProcessMemoryPriority,
        allow_cross_session_process_control: bool,
    ) -> Result<MemoryPriorityApplyOutcome, ProcessControlError> {
        self.apply_effective_claim(
            MemoryPriorityClaim {
                target: ProcessControlTarget::from_action_target(target),
                owner: ControlOwner::ProcessList,
                priority,
                preservation: MemoryPriorityPreservation::Exact,
            },
            allow_cross_session_process_control,
        )
    }

    fn apply_effective_claim(
        &mut self,
        claim: MemoryPriorityClaim,
        allow_cross_session_process_control: bool,
    ) -> Result<MemoryPriorityApplyOutcome, ProcessControlError> {
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

        let desired = memory_priority_raw(claim.priority);
        let baseline = managed.as_ref().map_or(current, |managed| managed.baseline);
        if priority_is_preserved(claim.preservation, baseline, desired) {
            if let Some(managed) = managed {
                self.managed.insert(identity.clone(), managed);
                self.release_identity(&identity)?;
                self.managed.remove(&identity);
            }
            return Ok(MemoryPriorityApplyOutcome::Preserved);
        }

        if current == desired {
            if let Some(mut managed) = managed {
                if claim.owner.is_automatic() || managed.owner == ControlOwner::ProcessList {
                    managed.owner = claim.owner;
                }
                managed.expected = current;
                self.managed.insert(identity, managed);
            }
            return Ok(MemoryPriorityApplyOutcome::Unchanged);
        }

        let sequence = self.next_apply_sequence;
        self.next_apply_sequence = self.next_apply_sequence.wrapping_add(1).max(1);
        match apply_transition(
            &mut self.platform,
            &process,
            current,
            desired,
            CommitFailureBehavior::Compensate,
        ) {
            Ok(()) => {
                self.managed.insert(
                    identity,
                    ManagedMemoryPriority {
                        baseline,
                        expected: desired,
                        owner: claim.owner,
                        apply_sequence: sequence,
                    },
                );
                Ok(MemoryPriorityApplyOutcome::Applied)
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
                        ManagedMemoryPriority {
                            baseline,
                            expected: desired,
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
        owner: ControlOwner,
        active_targets: &BTreeSet<ProcessTargetKey>,
    ) -> MemoryPriorityReleaseSummary {
        let stale_keys = match self.policy_claims(owner) {
            Ok(claims) => claims
                .keys()
                .filter(|key| !active_targets.contains(*key))
                .cloned()
                .collect::<BTreeSet<_>>(),
            Err(error) => {
                return MemoryPriorityReleaseSummary {
                    failures: vec![MemoryPriorityReleaseFailure {
                        process_id: 0,
                        process_name: "Memory Priority".to_owned(),
                        executable_path: String::new(),
                        error,
                    }],
                    ..Default::default()
                };
            }
        };
        if let Ok(claims) = self.policy_claims_mut(owner) {
            claims.retain(|key, _| !stale_keys.contains(key));
        }

        let mut reconcile_keys = stale_keys;
        reconcile_keys.extend(self.managed.iter().filter_map(|(identity, managed)| {
            let effective_owner = self
                .effective_claim(&identity.key())
                .map(|claim| claim.owner);
            (managed.owner == owner && effective_owner != Some(owner)).then(|| identity.key())
        }));

        let mut summary = MemoryPriorityReleaseSummary::default();
        for key in reconcile_keys {
            self.reconcile_policy_key(owner, &key, &mut summary);
        }
        summary
    }

    pub(crate) fn release_all_policy(
        &mut self,
        owner: ControlOwner,
    ) -> MemoryPriorityReleaseSummary {
        self.release_policy_except(owner, &BTreeSet::new())
    }

    fn reconcile_policy_key(
        &mut self,
        releasing_owner: ControlOwner,
        key: &ProcessTargetKey,
        summary: &mut MemoryPriorityReleaseSummary,
    ) {
        let managed_identity = self
            .managed
            .keys()
            .find(|identity| identity.key() == *key)
            .cloned();
        let was_managed_by_releasing_owner = managed_identity.as_ref().is_some_and(|identity| {
            self.managed
                .get(identity)
                .is_some_and(|managed| managed.owner == releasing_owner)
        });

        if let Some(claim) = self.effective_claim(key).cloned() {
            match self.apply_effective_claim(claim.clone(), true) {
                Ok(_) => {
                    summary.restored_processes += usize::from(was_managed_by_releasing_owner);
                }
                Err(ProcessControlError::ProcessExited) => {
                    self.drop_exited_identity(managed_identity.as_ref(), summary);
                }
                Err(error) => summary
                    .failures
                    .push(release_failure_for_claim(&claim, error)),
            }
            return;
        }

        let Some(identity) = managed_identity else {
            return;
        };
        let Some(managed) = self.managed.get(&identity) else {
            return;
        };
        if !managed.owner.is_automatic() {
            return;
        }
        match self.release_identity(&identity) {
            Ok(restored) => {
                summary.restored_processes += usize::from(restored);
                self.managed.remove(&identity);
            }
            Err(ProcessControlError::ProcessExited) => {
                self.drop_exited_identity(Some(&identity), summary);
            }
            Err(error) => summary
                .failures
                .push(release_failure_for_identity(&identity, error)),
        }
    }

    fn drop_exited_identity(
        &mut self,
        identity: Option<&ProcessIdentity>,
        summary: &mut MemoryPriorityReleaseSummary,
    ) {
        let Some(identity) = identity else {
            return;
        };
        match self.platform.relinquish(identity) {
            Ok(()) => {
                self.managed.remove(identity);
            }
            Err(error) => summary
                .failures
                .push(release_failure_for_identity(identity, error)),
        }
    }

    pub(crate) fn shutdown(&mut self) -> Result<(), String> {
        self.static_claims.clear();
        self.adaptive_claims.clear();
        let mut identities = self
            .managed
            .iter()
            .map(|(identity, managed)| (managed.apply_sequence, identity.clone()))
            .collect::<Vec<_>>();
        identities.sort_by_key(|identity| std::cmp::Reverse(identity.0));
        let mut summary = MemoryPriorityReleaseSummary::default();
        for (_, identity) in identities {
            match self.release_identity(&identity) {
                Ok(restored) => {
                    summary.restored_processes += usize::from(restored);
                    self.managed.remove(&identity);
                }
                Err(ProcessControlError::ProcessExited) => {
                    self.drop_exited_identity(Some(&identity), &mut summary);
                }
                Err(error) => summary
                    .failures
                    .push(release_failure_for_identity(&identity, error)),
            }
        }
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

    pub(crate) fn policy_managed_process_names(&self, owner: ControlOwner) -> Vec<String> {
        self.managed
            .iter()
            .filter(|(_, managed)| managed.owner == owner)
            .map(|(identity, _)| identity.name.clone())
            .collect()
    }

    pub(crate) fn policy_managed_process_count(&self, owner: ControlOwner) -> usize {
        self.managed
            .values()
            .filter(|managed| managed.owner == owner)
            .count()
    }

    pub(crate) fn has_managed_state(&self) -> bool {
        !self.managed.is_empty()
    }

    fn policy_claims(
        &self,
        owner: ControlOwner,
    ) -> Result<&BTreeMap<ProcessTargetKey, MemoryPriorityClaim>, ProcessControlError> {
        match owner {
            ControlOwner::MemoryPriority => Ok(&self.static_claims),
            ControlOwner::AdaptiveEngine => Ok(&self.adaptive_claims),
            _ => Err(ProcessControlError::Failed(
                "This owner cannot control Memory Priority policy.".to_owned(),
            )),
        }
    }

    fn policy_claims_mut(
        &mut self,
        owner: ControlOwner,
    ) -> Result<&mut BTreeMap<ProcessTargetKey, MemoryPriorityClaim>, ProcessControlError> {
        match owner {
            ControlOwner::MemoryPriority => Ok(&mut self.static_claims),
            ControlOwner::AdaptiveEngine => Ok(&mut self.adaptive_claims),
            _ => Err(ProcessControlError::Failed(
                "This owner cannot control Memory Priority policy.".to_owned(),
            )),
        }
    }

    fn effective_claim(&self, key: &ProcessTargetKey) -> Option<&MemoryPriorityClaim> {
        self.static_claims
            .get(key)
            .or_else(|| self.adaptive_claims.get(key))
    }
}

impl<P: MemoryPriorityPlatform> Drop for MemoryPriorityController<P> {
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

fn apply_transition<P: MemoryPriorityPlatform>(
    platform: &mut P,
    process: &P::Process,
    original: u32,
    expected: u32,
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
                format!("Memory Priority verification returned {actual}, expected {expected}."),
            ));
        }
        Err(error) => {
            return Err(compensate_with_intent(
                platform,
                process,
                original,
                intent,
                format!("Memory Priority verification failed: {error}"),
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

fn compensate_with_intent<P: MemoryPriorityPlatform>(
    platform: &mut P,
    process: &P::Process,
    original: u32,
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

fn compensate_without_intent<P: MemoryPriorityPlatform>(
    platform: &mut P,
    process: &P::Process,
    original: u32,
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

fn restore_and_verify<P: MemoryPriorityPlatform>(
    platform: &mut P,
    process: &P::Process,
    original: u32,
) -> Result<(), ProcessControlError> {
    platform.apply(process, original)?;
    let actual = platform.query(process)?;
    if actual == original {
        Ok(())
    } else {
        Err(ProcessControlError::Failed(format!(
            "Compensation returned {actual}, expected {original}."
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
        message.push_str(&format!(
            " Recovery journal commit failed: {recovery_error}."
        ));
    }
    message
}

fn release_failure_for_claim(
    claim: &MemoryPriorityClaim,
    error: ProcessControlError,
) -> MemoryPriorityReleaseFailure {
    MemoryPriorityReleaseFailure {
        process_id: claim.target.id,
        process_name: claim.target.name.clone(),
        executable_path: claim.target.executable_path.to_string_lossy().into_owned(),
        error,
    }
}

fn release_failure_for_identity(
    identity: &ProcessIdentity,
    error: ProcessControlError,
) -> MemoryPriorityReleaseFailure {
    MemoryPriorityReleaseFailure {
        process_id: identity.id,
        process_name: identity.name.clone(),
        executable_path: identity.executable_path.to_string_lossy().into_owned(),
        error,
    }
}

fn process_error(error: ProcessOperationError) -> ProcessControlError {
    match error {
        ProcessOperationError::AccessDenied { operation } => ProcessControlError::AccessDenied(
            format!("Windows denied Memory Priority access during {operation}."),
        ),
        ProcessOperationError::ProcessExited => ProcessControlError::ProcessExited,
        ProcessOperationError::Failed { operation, code } => {
            ProcessControlError::Failed(format!("{operation} failed with Windows error {code}."))
        }
    }
}

fn priority_is_preserved(
    preservation: MemoryPriorityPreservation,
    baseline: u32,
    desired: u32,
) -> bool {
    match preservation {
        MemoryPriorityPreservation::Exact => false,
        MemoryPriorityPreservation::PreserveHigher => baseline >= desired,
        MemoryPriorityPreservation::PreserveLower => baseline <= desired,
    }
}

pub(crate) const fn memory_priority_raw(priority: ProcessMemoryPriority) -> u32 {
    match priority {
        ProcessMemoryPriority::VeryLow => windows_memory_priority::VERY_LOW,
        ProcessMemoryPriority::Low => windows_memory_priority::LOW,
        ProcessMemoryPriority::Medium => windows_memory_priority::MEDIUM,
        ProcessMemoryPriority::BelowNormal => windows_memory_priority::BELOW_NORMAL,
        ProcessMemoryPriority::Normal => windows_memory_priority::NORMAL,
    }
}

pub(crate) const fn memory_priority_from_raw(raw: u32) -> Option<ProcessMemoryPriority> {
    match raw {
        windows_memory_priority::VERY_LOW => Some(ProcessMemoryPriority::VeryLow),
        windows_memory_priority::LOW => Some(ProcessMemoryPriority::Low),
        windows_memory_priority::MEDIUM => Some(ProcessMemoryPriority::Medium),
        windows_memory_priority::BELOW_NORMAL => Some(ProcessMemoryPriority::BelowNormal),
        windows_memory_priority::NORMAL => Some(ProcessMemoryPriority::Normal),
        _ => None,
    }
}

pub(crate) fn current_process_memory_priority(
    target: &ProcessActionTarget,
    allow_cross_session_process_control: bool,
) -> Result<ProcessMemoryPriority, ProcessControlError> {
    let target = ProcessControlTarget::from_action_target(target);
    let mut platform = WindowsMemoryPriorityPlatform;
    let (_, process) = platform.open(&target, allow_cross_session_process_control)?;
    let raw = platform.query(&process)?;
    memory_priority_from_raw(raw).ok_or_else(|| {
        ProcessControlError::Unavailable(format!(
            "Windows reported unsupported Memory Priority value {raw}."
        ))
    })
}

#[cfg(test)]
mod tests {
    use std::{
        path::PathBuf,
        process::{Child, Command},
        time::{Duration, Instant},
    };

    use super::*;

    #[derive(Clone)]
    struct FakeProcess {
        identity: ProcessIdentity,
        value: u32,
    }

    #[derive(Default)]
    struct FakePlatform {
        processes: BTreeMap<u32, FakeProcess>,
        applied: Vec<(u32, u32)>,
        relinquished: Vec<(u32, u64)>,
        fail_next_begin: bool,
        fail_next_apply: bool,
        fail_next_commit: bool,
        fail_relinquish: bool,
    }

    struct FakeIntent {
        fail: bool,
    }

    impl MemoryPriorityRecoveryIntent for FakeIntent {
        fn commit(self) -> Result<(), String> {
            if self.fail {
                Err("injected commit failure".to_owned())
            } else {
                Ok(())
            }
        }
    }

    impl MemoryPriorityPlatform for FakePlatform {
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

        fn query(&mut self, process: &Self::Process) -> Result<u32, ProcessControlError> {
            self.processes
                .get(process)
                .map(|process| process.value)
                .ok_or(ProcessControlError::ProcessExited)
        }

        fn begin_change(
            &mut self,
            _process: &Self::Process,
            _original: u32,
            _expected: u32,
        ) -> Result<Self::RecoveryIntent, ProcessControlError> {
            if std::mem::take(&mut self.fail_next_begin) {
                return Err(ProcessControlError::Failed(
                    "injected begin failure".to_owned(),
                ));
            }
            Ok(FakeIntent {
                fail: std::mem::take(&mut self.fail_next_commit),
            })
        }

        fn apply(
            &mut self,
            process: &Self::Process,
            priority: u32,
        ) -> Result<(), ProcessControlError> {
            if std::mem::take(&mut self.fail_next_apply) {
                return Err(ProcessControlError::Failed(
                    "injected apply failure".to_owned(),
                ));
            }
            let state = self
                .processes
                .get_mut(process)
                .ok_or(ProcessControlError::ProcessExited)?;
            state.value = priority;
            self.applied.push((*process, priority));
            Ok(())
        }

        fn relinquish(&mut self, identity: &ProcessIdentity) -> Result<(), ProcessControlError> {
            if self.fail_relinquish {
                return Err(ProcessControlError::Failed(
                    "injected relinquish failure".to_owned(),
                ));
            }
            self.relinquished
                .push((identity.id, identity.creation_time));
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

    fn platform_with(id: u32, creation_time: u64, value: u32) -> FakePlatform {
        let mut platform = FakePlatform::default();
        platform.processes.insert(
            id,
            FakeProcess {
                identity: identity(id, creation_time),
                value,
            },
        );
        platform
    }

    fn claim(
        id: u32,
        creation_time: u64,
        owner: ControlOwner,
        priority: ProcessMemoryPriority,
    ) -> MemoryPriorityClaim {
        MemoryPriorityClaim {
            target: target(id, creation_time),
            owner,
            priority,
            preservation: MemoryPriorityPreservation::Exact,
        }
    }

    #[test]
    fn known_raw_memory_priorities_round_trip_and_unknown_values_do_not_collapse() {
        for priority in ProcessMemoryPriority::ALL {
            assert_eq!(
                memory_priority_from_raw(memory_priority_raw(priority)),
                Some(priority)
            );
        }
        assert_eq!(memory_priority_from_raw(99), None);
    }

    #[test]
    fn static_policy_overrides_adaptive_then_reveals_it_without_baseline_bounce() {
        let mut controller = MemoryPriorityController::with_platform(platform_with(7, 1, 5));
        controller
            .apply_policy_claim(
                claim(
                    7,
                    1,
                    ControlOwner::AdaptiveEngine,
                    ProcessMemoryPriority::Low,
                ),
                true,
            )
            .unwrap();
        controller
            .apply_policy_claim(
                claim(
                    7,
                    1,
                    ControlOwner::MemoryPriority,
                    ProcessMemoryPriority::Medium,
                ),
                true,
            )
            .unwrap();

        let summary = controller.release_all_policy(ControlOwner::MemoryPriority);
        assert!(summary.failures.is_empty());
        assert_eq!(controller.platform.processes[&7].value, 2);
        assert_eq!(controller.platform.applied, vec![(7, 2), (7, 3), (7, 2)]);

        controller.release_all_policy(ControlOwner::AdaptiveEngine);
        assert_eq!(controller.platform.processes[&7].value, 5);
    }

    #[test]
    fn non_overlapping_static_and_adaptive_claims_remain_independently_effective() {
        let mut platform = platform_with(7, 1, 5);
        platform.processes.insert(
            8,
            FakeProcess {
                identity: identity(8, 1),
                value: 5,
            },
        );
        let mut controller = MemoryPriorityController::with_platform(platform);
        controller
            .apply_policy_claim(
                claim(
                    7,
                    1,
                    ControlOwner::AdaptiveEngine,
                    ProcessMemoryPriority::Low,
                ),
                true,
            )
            .unwrap();
        controller
            .apply_policy_claim(
                claim(
                    8,
                    1,
                    ControlOwner::MemoryPriority,
                    ProcessMemoryPriority::Medium,
                ),
                true,
            )
            .unwrap();

        controller.release_all_policy(ControlOwner::MemoryPriority);
        assert_eq!(controller.platform.processes[&7].value, 2);
        assert_eq!(controller.platform.processes[&8].value, 5);
        assert_eq!(
            controller.policy_managed_process_count(ControlOwner::AdaptiveEngine),
            1
        );
    }

    #[test]
    fn lower_precedence_claim_is_stored_without_touching_static_state() {
        let mut controller = MemoryPriorityController::with_platform(platform_with(7, 1, 5));
        controller
            .apply_policy_claim(
                claim(
                    7,
                    1,
                    ControlOwner::MemoryPriority,
                    ProcessMemoryPriority::Medium,
                ),
                true,
            )
            .unwrap();
        let outcome = controller
            .apply_policy_claim(
                claim(
                    7,
                    1,
                    ControlOwner::AdaptiveEngine,
                    ProcessMemoryPriority::VeryLow,
                ),
                true,
            )
            .unwrap();

        assert_eq!(outcome, MemoryPriorityApplyOutcome::Shadowed);
        assert_eq!(controller.platform.processes[&7].value, 3);
        assert_eq!(controller.platform.applied, vec![(7, 3)]);

        let summary = controller.release_all_policy(ControlOwner::AdaptiveEngine);
        assert_eq!(summary.restored_processes, 0);
        assert_eq!(controller.platform.processes[&7].value, 3);
        assert_eq!(controller.platform.applied, vec![(7, 3)]);
    }

    #[test]
    fn failed_owner_replacement_is_retried_after_the_removed_claim_is_gone() {
        let mut controller = MemoryPriorityController::with_platform(platform_with(7, 1, 5));
        controller
            .apply_policy_claim(
                claim(
                    7,
                    1,
                    ControlOwner::AdaptiveEngine,
                    ProcessMemoryPriority::Low,
                ),
                true,
            )
            .unwrap();
        controller
            .apply_policy_claim(
                claim(
                    7,
                    1,
                    ControlOwner::MemoryPriority,
                    ProcessMemoryPriority::Medium,
                ),
                true,
            )
            .unwrap();
        controller.platform.fail_next_apply = true;

        let first = controller.release_all_policy(ControlOwner::MemoryPriority);
        assert_eq!(first.failures.len(), 1);
        assert_eq!(controller.platform.processes[&7].value, 3);

        let second = controller.release_all_policy(ControlOwner::MemoryPriority);
        assert!(second.failures.is_empty());
        assert_eq!(controller.platform.processes[&7].value, 2);
    }

    #[test]
    fn process_list_action_is_immediate_then_automatic_policy_supersedes_it() {
        let mut controller = MemoryPriorityController::with_platform(platform_with(7, 1, 5));
        let automatic = claim(
            7,
            1,
            ControlOwner::AdaptiveEngine,
            ProcessMemoryPriority::Low,
        );
        controller
            .apply_policy_claim(automatic.clone(), true)
            .unwrap();
        let action_target = ProcessActionTarget {
            id: 7,
            name: "app7.exe".to_owned(),
            executable_path: PathBuf::from(r"C:\Apps\app7.exe"),
            creation_time: 1,
            session_id: Some(1),
            is_service_account: Some(false),
        };
        controller
            .apply_process_list_action(&action_target, ProcessMemoryPriority::Medium, true)
            .unwrap();
        assert_eq!(controller.platform.processes[&7].value, 3);

        controller.apply_policy_claim(automatic, true).unwrap();
        assert_eq!(controller.platform.processes[&7].value, 2);
        controller.shutdown().unwrap();
        assert_eq!(controller.platform.processes[&7].value, 5);
    }

    #[test]
    fn preservation_uses_the_original_baseline() {
        let mut controller = MemoryPriorityController::with_platform(platform_with(7, 1, 5));
        let mut lower = claim(
            7,
            1,
            ControlOwner::AdaptiveEngine,
            ProcessMemoryPriority::Low,
        );
        lower.preservation = MemoryPriorityPreservation::PreserveHigher;
        assert_eq!(
            controller.apply_policy_claim(lower, true).unwrap(),
            MemoryPriorityApplyOutcome::Preserved
        );
        assert!(!controller.has_managed_state());
        assert!(controller.platform.applied.is_empty());
    }

    #[test]
    fn external_state_break_is_relinquished_without_overwrite_on_release() {
        let mut controller = MemoryPriorityController::with_platform(platform_with(7, 1, 5));
        controller
            .apply_policy_claim(
                claim(
                    7,
                    1,
                    ControlOwner::AdaptiveEngine,
                    ProcessMemoryPriority::Low,
                ),
                true,
            )
            .unwrap();
        controller.platform.processes.get_mut(&7).unwrap().value = 4;
        controller.release_all_policy(ControlOwner::AdaptiveEngine);

        assert_eq!(controller.platform.processes[&7].value, 4);
        assert_eq!(controller.platform.applied, vec![(7, 2)]);
        assert_eq!(controller.platform.relinquished, vec![(7, 1)]);
        assert!(!controller.has_managed_state());
    }

    #[test]
    fn pid_reuse_relinquishes_the_old_identity_before_applying_the_new_claim() {
        let mut controller = MemoryPriorityController::with_platform(platform_with(7, 1, 5));
        controller
            .apply_policy_claim(
                claim(
                    7,
                    1,
                    ControlOwner::AdaptiveEngine,
                    ProcessMemoryPriority::Low,
                ),
                true,
            )
            .unwrap();
        controller.platform.processes.insert(
            7,
            FakeProcess {
                identity: identity(7, 2),
                value: 5,
            },
        );
        controller
            .apply_policy_claim(
                claim(
                    7,
                    2,
                    ControlOwner::MemoryPriority,
                    ProcessMemoryPriority::Medium,
                ),
                true,
            )
            .unwrap();

        assert_eq!(controller.platform.relinquished, vec![(7, 1)]);
        assert_eq!(controller.platform.processes[&7].value, 3);
    }

    #[test]
    fn begin_failure_leaves_the_process_and_controller_unmanaged() {
        let mut platform = platform_with(7, 1, 5);
        platform.fail_next_begin = true;
        let mut controller = MemoryPriorityController::with_platform(platform);
        assert!(controller
            .apply_policy_claim(
                claim(
                    7,
                    1,
                    ControlOwner::MemoryPriority,
                    ProcessMemoryPriority::Low,
                ),
                true,
            )
            .is_err());
        assert_eq!(controller.platform.processes[&7].value, 5);
        assert!(!controller.has_managed_state());
    }

    #[test]
    fn apply_failure_compensates_to_the_original_value() {
        let mut platform = platform_with(7, 1, 5);
        platform.fail_next_apply = true;
        let mut controller = MemoryPriorityController::with_platform(platform);
        assert!(controller
            .apply_policy_claim(
                claim(
                    7,
                    1,
                    ControlOwner::MemoryPriority,
                    ProcessMemoryPriority::Low,
                ),
                true,
            )
            .is_err());
        assert_eq!(controller.platform.processes[&7].value, 5);
        assert!(!controller.has_managed_state());
    }

    #[test]
    fn release_commit_failure_keeps_the_verified_baseline_and_relinquishes() {
        let mut controller = MemoryPriorityController::with_platform(platform_with(7, 1, 5));
        controller
            .apply_policy_claim(
                claim(
                    7,
                    1,
                    ControlOwner::MemoryPriority,
                    ProcessMemoryPriority::Low,
                ),
                true,
            )
            .unwrap();
        controller.platform.fail_next_commit = true;
        let summary = controller.release_all_policy(ControlOwner::MemoryPriority);

        assert!(summary.failures.is_empty());
        assert_eq!(controller.platform.processes[&7].value, 5);
        assert_eq!(controller.platform.relinquished, vec![(7, 1)]);
        assert!(!controller.has_managed_state());
    }

    #[test]
    fn shutdown_restores_in_reverse_successful_application_order() {
        let mut platform = platform_with(7, 1, 5);
        platform.processes.insert(
            8,
            FakeProcess {
                identity: identity(8, 1),
                value: 5,
            },
        );
        let mut controller = MemoryPriorityController::with_platform(platform);
        for id in [7, 8] {
            controller
                .apply_policy_claim(
                    claim(
                        id,
                        1,
                        ControlOwner::MemoryPriority,
                        ProcessMemoryPriority::Low,
                    ),
                    true,
                )
                .unwrap();
        }
        controller.shutdown().unwrap();

        assert_eq!(
            controller.platform.applied,
            vec![(7, 2), (8, 2), (8, 5), (7, 5)]
        );
    }

    #[test]
    #[ignore = "modifies and restores a disposable Windows process; run in explicit integration QA"]
    fn memory_priority_live_apply_and_clean_release() -> Result<(), String> {
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
        let baseline =
            current_process_memory_priority(&target, true).map_err(|error| error.to_string())?;
        let expected = if baseline == ProcessMemoryPriority::VeryLow {
            ProcessMemoryPriority::Low
        } else {
            ProcessMemoryPriority::VeryLow
        };
        let mut controller = MemoryPriorityController::default();

        controller
            .apply_process_list_action(&target, expected, true)
            .map_err(|error| error.to_string())?;
        assert_eq!(
            current_process_memory_priority(&target, true).map_err(|error| error.to_string())?,
            expected
        );
        controller.shutdown()?;
        assert_eq!(
            current_process_memory_priority(&target, true).map_err(|error| error.to_string())?,
            baseline
        );
        Ok(())
    }
}
