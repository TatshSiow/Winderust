use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    time::{Duration, Instant},
};

use crate::{
    backend::crash_recovery::{
        forget_io_priority_change, record_process_change, ProcessValue, RecoveryIntent,
    },
    config::ProcessIoPriority,
    foreground::ProcessActionTarget,
    platform::windows::io_priority::{self as windows_io_priority, IoPriorityError},
    win_util::WinHandle,
};

use super::process::{
    open_process_for_set_information, transition_failure_error, ControlOwner, ProcessControlError,
    ProcessControlTarget, ProcessIdentity, ProcessTargetKey,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IoPriorityPreservation {
    Exact,
    PreserveHigher,
    PreserveLower,
}

#[derive(Debug, Clone)]
pub(crate) struct IoPriorityClaim {
    pub(crate) target: ProcessControlTarget,
    pub(crate) owner: ControlOwner,
    pub(crate) priority: ProcessIoPriority,
    pub(crate) preservation: IoPriorityPreservation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum IoPriorityApplyOutcome {
    Applied,
    Unchanged,
    Preserved,
    Shadowed,
}

#[derive(Debug)]
pub(crate) struct IoPriorityReleaseFailure {
    pub(crate) process_id: u32,
    pub(crate) process_name: String,
    pub(crate) executable_path: PathBuf,
    pub(crate) error: ProcessControlError,
}

#[derive(Debug, Default)]
pub(crate) struct IoPriorityReleaseSummary {
    pub(crate) restored_processes: usize,
    pub(crate) failures: Vec<IoPriorityReleaseFailure>,
}

#[derive(Debug, Default)]
pub(crate) struct IoPriorityReleaseRetries {
    pub(crate) static_policy: IoPriorityReleaseSummary,
    pub(crate) adaptive_engine: IoPriorityReleaseSummary,
}

struct PendingIoPriorityRelease {
    identity: ProcessIdentity,
    owner: ControlOwner,
    error: ProcessControlError,
}

#[derive(Default)]
struct AdaptiveIoEpisode {
    eligible: bool,
    manual_yield: bool,
}

struct ManagedIoPriority {
    baseline: u32,
    expected: u32,
    owner: ControlOwner,
    apply_sequence: u64,
}

pub(crate) trait IoPriorityRecoveryIntent {
    fn commit(self) -> Result<(), String>;
    fn retain_pending(self) -> Result<(), String>;
}

impl IoPriorityRecoveryIntent for RecoveryIntent {
    fn commit(self) -> Result<(), String> {
        RecoveryIntent::commit(self)
    }

    fn retain_pending(self) -> Result<(), String> {
        RecoveryIntent::retain_pending(self)
    }
}

pub(crate) trait IoPriorityPlatform {
    type Process;
    type RecoveryIntent: IoPriorityRecoveryIntent;

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
pub(crate) struct WindowsIoPriorityPlatform;

impl IoPriorityPlatform for WindowsIoPriorityPlatform {
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
        windows_io_priority::query(process).map_err(io_priority_error)
    }

    fn begin_change(
        &mut self,
        process: &Self::Process,
        original: u32,
        expected: u32,
    ) -> Result<Self::RecoveryIntent, ProcessControlError> {
        record_process_change(
            process.raw(),
            ProcessValue::IoPriority(original),
            ProcessValue::IoPriority(expected),
        )
        .map_err(ProcessControlError::Failed)
    }

    fn apply(&mut self, process: &Self::Process, priority: u32) -> Result<(), ProcessControlError> {
        windows_io_priority::set(process, priority).map_err(io_priority_error)
    }

    fn relinquish(&mut self, identity: &ProcessIdentity) -> Result<(), ProcessControlError> {
        forget_io_priority_change(identity.id, identity.creation_time)
            .map_err(ProcessControlError::Failed)
    }
}

pub(crate) struct IoPriorityController<P: IoPriorityPlatform = WindowsIoPriorityPlatform> {
    platform: P,
    static_targets: BTreeSet<ProcessTargetKey>,
    adaptive_episodes: BTreeMap<ProcessTargetKey, AdaptiveIoEpisode>,
    managed: BTreeMap<ProcessIdentity, ManagedIoPriority>,
    next_apply_sequence: u64,
    pending_releases: BTreeMap<ProcessTargetKey, PendingIoPriorityRelease>,
    next_release_retry: Option<Instant>,
}

impl Default for IoPriorityController {
    fn default() -> Self {
        Self::with_platform(WindowsIoPriorityPlatform)
    }
}

impl<P: IoPriorityPlatform> IoPriorityController<P> {
    fn with_platform(platform: P) -> Self {
        Self {
            platform,
            static_targets: BTreeSet::new(),
            adaptive_episodes: BTreeMap::new(),
            managed: BTreeMap::new(),
            next_apply_sequence: 1,
            pending_releases: BTreeMap::new(),
            next_release_retry: None,
        }
    }

    pub(crate) fn apply_policy_claim(
        &mut self,
        claim: IoPriorityClaim,
        allow_cross_session_process_control: bool,
    ) -> Result<IoPriorityApplyOutcome, ProcessControlError> {
        let key = claim.target.key();
        match claim.owner {
            ControlOwner::IoPriority => {
                self.static_targets.insert(key);
            }
            ControlOwner::AdaptiveEngine => {
                if !matches!(
                    claim.priority,
                    ProcessIoPriority::VeryLow | ProcessIoPriority::Low
                ) || claim.preservation != IoPriorityPreservation::PreserveLower
                {
                    return Err(ProcessControlError::Failed(
                        "Adaptive Engine can only lower I/O priority to Low or Very Low."
                            .to_owned(),
                    ));
                }
                if !self
                    .adaptive_episodes
                    .get(&key)
                    .is_some_and(|episode| episode.eligible && !episode.manual_yield)
                    || self.static_targets.contains(&key)
                    || self.pending_releases.contains_key(&key)
                {
                    return Ok(IoPriorityApplyOutcome::Shadowed);
                }
            }
            _ => {
                return Err(ProcessControlError::Failed(
                    "This owner cannot control I/O Priority policy.".to_owned(),
                ));
            }
        }
        self.apply_claim(claim, allow_cross_session_process_control)
    }

    pub(crate) fn apply_process_list_action(
        &mut self,
        target: &ProcessActionTarget,
        priority: ProcessIoPriority,
        allow_cross_session_process_control: bool,
    ) -> Result<IoPriorityApplyOutcome, ProcessControlError> {
        let target = ProcessControlTarget::from_action_target(target);
        let key = target.key();
        let outcome = self.apply_claim(
            IoPriorityClaim {
                target,
                owner: ControlOwner::ProcessList,
                priority,
                preservation: IoPriorityPreservation::Exact,
            },
            allow_cross_session_process_control,
        )?;
        if let Some(episode) = self.adaptive_episodes.get_mut(&key) {
            episode.manual_yield = true;
        }
        Ok(outcome)
    }

    fn apply_claim(
        &mut self,
        claim: IoPriorityClaim,
        allow_cross_session_process_control: bool,
    ) -> Result<IoPriorityApplyOutcome, ProcessControlError> {
        let (identity, process) = self
            .platform
            .open(&claim.target, allow_cross_session_process_control)?;
        let key = identity.key();
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
            self.pending_releases.remove(&stale_identity.key());
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
            self.pending_releases.remove(&key);
            managed = None;
        }

        managed = match managed {
            Some(managed)
                if claim.owner == ControlOwner::AdaptiveEngine
                    && managed.owner == ControlOwner::ProcessList =>
            {
                self.managed.insert(identity, managed);
                return Ok(IoPriorityApplyOutcome::Shadowed);
            }
            managed => managed,
        };
        if claim.owner == ControlOwner::AdaptiveEngine && io_priority_from_raw(current).is_none() {
            return Ok(IoPriorityApplyOutcome::Preserved);
        }

        let desired = io_priority_raw(claim.priority);
        let baseline = managed.as_ref().map_or(current, |managed| managed.baseline);
        if priority_is_preserved(claim.preservation, baseline, desired) {
            if let Some(managed) = managed {
                self.managed.insert(identity.clone(), managed);
                self.release_identity(&identity)?;
                self.managed.remove(&identity);
                self.pending_releases.remove(&key);
            }
            return Ok(IoPriorityApplyOutcome::Preserved);
        }

        if current == desired {
            if let Some(mut managed) = managed {
                if claim.owner.is_automatic() || managed.owner != ControlOwner::IoPriority {
                    managed.owner = claim.owner;
                    self.pending_releases.remove(&key);
                }
                managed.expected = current;
                self.managed.insert(identity, managed);
            }
            return Ok(IoPriorityApplyOutcome::Unchanged);
        }

        let sequence = self.next_apply_sequence;
        self.next_apply_sequence = self.next_apply_sequence.wrapping_add(1).max(1);
        match apply_transition(
            &mut self.platform,
            &process,
            current,
            desired,
            CommitFailureBehavior::RetainOwnership,
        ) {
            Ok(()) => {
                self.pending_releases.remove(&key);
                self.managed.insert(
                    identity,
                    ManagedIoPriority {
                        baseline,
                        expected: desired,
                        owner: claim.owner,
                        apply_sequence: sequence,
                    },
                );
                Ok(IoPriorityApplyOutcome::Applied)
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
                    if claim.owner == ControlOwner::AdaptiveEngine {
                        self.defer_release(
                            key,
                            identity.clone(),
                            claim.owner,
                            failure.error.clone(),
                        );
                    }
                    self.managed.insert(
                        identity,
                        ManagedIoPriority {
                            baseline,
                            expected: desired,
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
        owner: ControlOwner,
        active_targets: &BTreeSet<ProcessTargetKey>,
    ) -> IoPriorityReleaseSummary {
        match owner {
            ControlOwner::IoPriority => {
                if &self.static_targets != active_targets {
                    self.static_targets.clone_from(active_targets);
                }
            }
            ControlOwner::AdaptiveEngine => {}
            _ => {
                return IoPriorityReleaseSummary {
                    failures: vec![IoPriorityReleaseFailure {
                        process_id: 0,
                        process_name: "I/O Priority".to_owned(),
                        executable_path: PathBuf::new(),
                        error: ProcessControlError::Failed(
                            "This owner cannot release I/O Priority policy.".to_owned(),
                        ),
                    }],
                    ..Default::default()
                };
            }
        }
        let identities = self
            .managed
            .iter()
            .filter_map(|(identity, managed)| {
                if managed.owner != owner {
                    return None;
                }
                let key = identity.key();
                if self.pending_releases.get(&key).is_some_and(|pending| {
                    owner == ControlOwner::AdaptiveEngine || pending.owner != owner
                }) {
                    return None;
                }
                let shadowed = owner == ControlOwner::AdaptiveEngine
                    && (self.static_targets.contains(&key)
                        || !self.adaptive_episodes.contains_key(&key));
                (shadowed || !active_targets.contains(&key))
                    .then(|| (managed.apply_sequence, identity.clone()))
            })
            .collect::<Vec<_>>();
        let summary = self.release_identities(identities, Some(owner));
        if self.pending_releases.is_empty() {
            self.next_release_retry = None;
        }
        summary
    }

    pub(crate) fn reconcile_adaptive_restraint_targets(
        &mut self,
        eligible_targets: &BTreeSet<ProcessTargetKey>,
        verified_episodes: &BTreeSet<ProcessTargetKey>,
    ) -> IoPriorityReleaseSummary {
        self.adaptive_episodes
            .retain(|key, _| verified_episodes.contains(key));
        for key in verified_episodes {
            self.adaptive_episodes
                .entry(key.clone())
                .or_default()
                .eligible = eligible_targets.contains(key);
        }
        self.release_policy_except(ControlOwner::AdaptiveEngine, eligible_targets)
    }

    pub(crate) fn has_adaptive_restraint_state(&self) -> bool {
        !self.adaptive_episodes.is_empty()
            || self
                .pending_releases
                .values()
                .any(|entry| entry.owner == ControlOwner::AdaptiveEngine)
    }

    pub(crate) fn release_retry_delay(&self, now: Instant) -> Option<Duration> {
        (!self.pending_releases.is_empty()).then(|| {
            self.next_release_retry
                .map_or(Duration::ZERO, |due| due.saturating_duration_since(now))
        })
    }

    pub(crate) fn retry_pending_releases(&mut self, now: Instant) -> IoPriorityReleaseRetries {
        if self.release_retry_delay(now) != Some(Duration::ZERO) {
            return IoPriorityReleaseRetries::default();
        }
        let mut static_identities = Vec::new();
        let mut adaptive_identities = Vec::new();
        for pending in self.pending_releases.values() {
            if let Some(managed) = self.managed.get(&pending.identity) {
                let identities = match pending.owner {
                    ControlOwner::IoPriority => &mut static_identities,
                    ControlOwner::AdaptiveEngine => &mut adaptive_identities,
                    // Only policy withdrawal can queue an automatic release.
                    _ => continue,
                };
                identities.push((managed.apply_sequence, pending.identity.clone()));
            }
        }
        let static_policy =
            self.release_identities(static_identities, Some(ControlOwner::IoPriority));
        let adaptive_engine =
            self.release_identities(adaptive_identities, Some(ControlOwner::AdaptiveEngine));
        self.pending_releases
            .retain(|_, pending| self.managed.contains_key(&pending.identity));
        self.next_release_retry =
            (!self.pending_releases.is_empty()).then(|| now + Duration::from_secs(1));
        IoPriorityReleaseRetries {
            static_policy,
            adaptive_engine,
        }
    }

    pub(crate) fn release_all_policy(&mut self, owner: ControlOwner) -> IoPriorityReleaseSummary {
        let active_targets = BTreeSet::new();
        if owner == ControlOwner::AdaptiveEngine {
            self.reconcile_adaptive_restraint_targets(&active_targets, &active_targets)
        } else {
            self.release_policy_except(owner, &active_targets)
        }
    }

    pub(crate) fn shutdown(&mut self) -> Result<(), String> {
        self.static_targets.clear();
        self.adaptive_episodes.clear();
        let identities = self
            .managed
            .iter()
            .map(|(identity, managed)| (managed.apply_sequence, identity.clone()))
            .collect::<Vec<_>>();
        let summary = self.release_identities(identities, None);
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
        release_owner: Option<ControlOwner>,
    ) -> IoPriorityReleaseSummary {
        identities.sort_by_key(|identity| std::cmp::Reverse(identity.0));
        let mut summary = IoPriorityReleaseSummary::default();
        for (_, identity) in identities {
            let released = match self.release_identity(&identity) {
                Err(ProcessControlError::ProcessExited) => {
                    self.platform.relinquish(&identity).map(|()| false)
                }
                released => released,
            };
            let key = identity.key();
            match released {
                Ok(restored) => {
                    summary.restored_processes += usize::from(restored);
                    self.managed.remove(&identity);
                    self.pending_releases.remove(&key);
                }
                Err(error) => {
                    if let Some(owner) = release_owner {
                        self.defer_release(key, identity.clone(), owner, error.clone());
                    }
                    summary.failures.push(IoPriorityReleaseFailure {
                        process_id: identity.id,
                        process_name: identity.name,
                        executable_path: identity.executable_path,
                        error,
                    });
                }
            }
        }
        summary
    }

    fn defer_release(
        &mut self,
        key: ProcessTargetKey,
        identity: ProcessIdentity,
        owner: ControlOwner,
        error: ProcessControlError,
    ) {
        if self.pending_releases.is_empty() {
            self.next_release_retry = Some(Instant::now() + Duration::from_secs(1));
        }
        match self.pending_releases.entry(key) {
            std::collections::btree_map::Entry::Occupied(mut existing) => {
                existing.get_mut().error = error;
            }
            std::collections::btree_map::Entry::Vacant(entry) => {
                entry.insert(PendingIoPriorityRelease {
                    identity,
                    owner,
                    error,
                });
            }
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

    pub(crate) fn policy_managed_process_count(&self, owner: ControlOwner) -> usize {
        self.managed
            .iter()
            .filter(|(identity, managed)| {
                managed.owner == owner
                    && (owner != ControlOwner::AdaptiveEngine
                        || !self.pending_releases.contains_key(&identity.key()))
            })
            .count()
    }

    pub(crate) fn policy_release_failures(
        &self,
        owner: ControlOwner,
    ) -> impl Iterator<Item = (&ProcessIdentity, &ProcessControlError)> {
        self.pending_releases
            .values()
            .filter(move |pending| pending.owner == owner)
            .map(|pending| (&pending.identity, &pending.error))
    }

    pub(crate) fn has_managed_state(&self) -> bool {
        !self.managed.is_empty()
    }
}

impl<P: IoPriorityPlatform> Drop for IoPriorityController<P> {
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

fn apply_transition<P: IoPriorityPlatform>(
    platform: &mut P,
    process: &P::Process,
    original: u32,
    expected: u32,
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
                    "I/O Priority verification returned {actual}, expected {expected}."
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

fn compensate_with_intent<P: IoPriorityPlatform>(
    platform: &mut P,
    process: &P::Process,
    original: u32,
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

fn restore_and_verify<P: IoPriorityPlatform>(
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

fn priority_is_preserved(
    preservation: IoPriorityPreservation,
    baseline: u32,
    desired: u32,
) -> bool {
    match preservation {
        IoPriorityPreservation::Exact => false,
        IoPriorityPreservation::PreserveHigher => baseline >= desired,
        IoPriorityPreservation::PreserveLower => baseline <= desired,
    }
}

pub(crate) const fn io_priority_raw(priority: ProcessIoPriority) -> u32 {
    match priority {
        ProcessIoPriority::VeryLow => 0,
        ProcessIoPriority::Low => 1,
        ProcessIoPriority::Normal => 2,
        ProcessIoPriority::High => 3,
        ProcessIoPriority::Critical => 4,
    }
}

fn io_priority_from_raw(priority: u32) -> Option<ProcessIoPriority> {
    match priority {
        0 => Some(ProcessIoPriority::VeryLow),
        1 => Some(ProcessIoPriority::Low),
        2 => Some(ProcessIoPriority::Normal),
        3 => Some(ProcessIoPriority::High),
        4 => Some(ProcessIoPriority::Critical),
        _ => None,
    }
}

pub(crate) fn current_process_io_priority(
    target: &ProcessActionTarget,
    allow_cross_session_process_control: bool,
) -> Result<ProcessIoPriority, String> {
    let mut platform = WindowsIoPriorityPlatform;
    let (_, process) = platform
        .open(
            &ProcessControlTarget::from_action_target(target),
            allow_cross_session_process_control,
        )
        .map_err(|error| error.to_string())?;
    let raw = platform
        .query(&process)
        .map_err(|error| error.to_string())?;
    io_priority_from_raw(raw)
        .ok_or_else(|| format!("Windows returned unsupported I/O priority value {raw}."))
}

fn io_priority_error(error: IoPriorityError) -> ProcessControlError {
    match error {
        IoPriorityError::ProcessExited => ProcessControlError::ProcessExited,
        IoPriorityError::PrivilegeSetup(5 | 1300)
        | IoPriorityError::NtStatus(0xC0000022 | 0xC0000061) => {
            ProcessControlError::AccessDenied(error.to_string())
        }
        _ => ProcessControlError::Failed(error.to_string()),
    }
}

#[cfg(test)]
mod tests {
    mod adaptive;

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
        Privilege,
        BeginExited,
        QueryDenied,
        ApplyExited,
        Open,
        Verify,
        Begin,
        Apply,
        Commit,
        Relinquish,
    }

    struct FakeRecoveryIntent {
        fail: bool,
        events: Arc<Mutex<Vec<String>>>,
    }

    impl IoPriorityRecoveryIntent for FakeRecoveryIntent {
        fn commit(self) -> Result<(), String> {
            self.events.lock().unwrap().push("commit".to_owned());
            if self.fail {
                Err("commit failed".to_owned())
            } else {
                Ok(())
            }
        }

        fn retain_pending(self) -> Result<(), String> {
            self.events
                .lock()
                .unwrap()
                .push("retain_pending".to_owned());
            Ok(())
        }
    }

    #[derive(Clone)]
    struct FakeProcess {
        identity: ProcessIdentity,
        priority: u32,
    }

    struct FakePlatform {
        processes: BTreeMap<u32, FakeProcess>,
        failures: VecDeque<FakeFailure>,
        events: Arc<Mutex<Vec<String>>>,
    }

    impl FakePlatform {
        fn new(priority: u32) -> Self {
            Self::with_processes(&[(42, 7, priority)])
        }

        fn with_processes(processes: &[(u32, u64, u32)]) -> Self {
            Self {
                processes: processes
                    .iter()
                    .map(|(id, creation_time, priority)| {
                        (
                            *id,
                            FakeProcess {
                                identity: identity(*id, *creation_time),
                                priority: *priority,
                            },
                        )
                    })
                    .collect(),
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

    impl IoPriorityPlatform for FakePlatform {
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

        fn query(&mut self, process: &Self::Process) -> Result<u32, ProcessControlError> {
            if self.take_failure(FakeFailure::QueryDenied) {
                return Err(ProcessControlError::AccessDenied("query denied".into()));
            }
            let follows_apply = self
                .events
                .lock()
                .unwrap()
                .last()
                .is_some_and(|event| event.starts_with("apply:"));
            self.events.lock().unwrap().push("query".to_owned());
            if follows_apply && self.take_failure(FakeFailure::Verify) {
                return Err(ProcessControlError::Failed("query failed".to_owned()));
            }
            self.processes
                .get(process)
                .map(|process| process.priority)
                .ok_or(ProcessControlError::ProcessExited)
        }

        fn begin_change(
            &mut self,
            _: &Self::Process,
            _: u32,
            _: u32,
        ) -> Result<Self::RecoveryIntent, ProcessControlError> {
            self.events.lock().unwrap().push("begin".to_owned());
            if self.take_failure(FakeFailure::BeginExited) {
                return Err(ProcessControlError::ProcessExited);
            }
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
            priority: u32,
        ) -> Result<(), ProcessControlError> {
            self.events
                .lock()
                .unwrap()
                .push(format!("apply:{process}:{priority}"));
            if self.take_failure(FakeFailure::Privilege) {
                return Err(io_priority_error(IoPriorityError::PrivilegeSetup(1300)));
            }
            if self.take_failure(FakeFailure::ApplyExited) {
                self.processes.remove(process);
                return Err(ProcessControlError::ProcessExited);
            }
            if self.take_failure(FakeFailure::Apply) {
                return Err(ProcessControlError::Failed("apply failed".to_owned()));
            }
            self.processes
                .get_mut(process)
                .ok_or(ProcessControlError::ProcessExited)?
                .priority = priority;
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
            format!("app-{id}.exe"),
            PathBuf::from(format!(r"C:\Apps\app-{id}.exe")),
            creation_time,
            Some(1),
        )
    }

    fn target(id: u32, creation_time: u64) -> ProcessControlTarget {
        identity(id, creation_time).target()
    }

    fn claim(
        owner: ControlOwner,
        priority: ProcessIoPriority,
        preservation: IoPriorityPreservation,
    ) -> IoPriorityClaim {
        IoPriorityClaim {
            target: target(42, 7),
            owner,
            priority,
            preservation,
        }
    }

    #[test]
    fn missing_privilege_blocks_mutation_but_retains_pending_restoration() {
        let mut controller = IoPriorityController::with_platform(FakePlatform::new(2));
        let request = claim(
            ControlOwner::IoPriority,
            ProcessIoPriority::Low,
            IoPriorityPreservation::Exact,
        );
        controller.platform.fail_next(FakeFailure::Privilege);
        assert!(matches!(
            controller.apply_policy_claim(request.clone(), true),
            Err(ProcessControlError::AccessDenied(_))
        ));
        assert!(!controller.has_managed_state());
        assert_eq!(controller.platform.processes[&42].priority, 2);

        controller.apply_policy_claim(request, true).unwrap();
        controller.platform.fail_next(FakeFailure::Privilege);
        let summary = controller.release_all_policy(ControlOwner::IoPriority);
        assert_eq!(summary.failures.len(), 1);
        assert!(controller.has_managed_state());
        assert_eq!(controller.pending_releases.len(), 1);
        let summary = controller
            .retry_pending_releases(Instant::now() + Duration::from_secs(2))
            .static_policy;
        assert_eq!(summary.restored_processes, 1);
        assert!(!controller.has_managed_state());
        assert_eq!(controller.platform.processes[&42].priority, 2);
    }

    #[test]
    fn native_permission_errors_use_the_suppression_path() {
        for error in [
            IoPriorityError::PrivilegeSetup(1300),
            IoPriorityError::PrivilegeSetup(5),
            IoPriorityError::NtStatus(0xC0000061),
            IoPriorityError::NtStatus(0xC0000022),
        ] {
            assert!(matches!(
                io_priority_error(error),
                ProcessControlError::AccessDenied(_)
            ));
        }
        assert!(io_priority_error(IoPriorityError::NtStatus(0xC0000061))
            .to_string()
            .contains("SeIncreaseBasePriorityPrivilege"));
        assert!(matches!(
            io_priority_error(IoPriorityError::NtStatus(0xC0000001)),
            ProcessControlError::Failed(_)
        ));
    }

    fn action_target() -> ProcessActionTarget {
        let target = target(42, 7);
        ProcessActionTarget {
            id: target.id,
            name: target.name,
            executable_path: target.executable_path,
            creation_time: target.creation_time,
            session_id: Some(1),
            is_service_account: Some(false),
        }
    }

    #[test]
    fn disabled_cleanup_survives_failed_replacement_and_respects_accepted_owner() {
        for replace in [false, true] {
            let mut controller = IoPriorityController::with_platform(FakePlatform::new(2));
            controller
                .apply_policy_claim(
                    claim(
                        ControlOwner::IoPriority,
                        ProcessIoPriority::VeryLow,
                        IoPriorityPreservation::Exact,
                    ),
                    true,
                )
                .unwrap();
            controller.platform.fail_next(FakeFailure::Open);
            assert_eq!(
                controller
                    .release_all_policy(ControlOwner::IoPriority)
                    .failures
                    .len(),
                1
            );
            for failure in [
                FakeFailure::Open,
                FakeFailure::QueryDenied,
                FakeFailure::Apply,
            ] {
                controller.platform.fail_next(failure);
                assert!(controller
                    .apply_process_list_action(&action_target(), ProcessIoPriority::Low, true)
                    .is_err());
                assert_eq!(controller.pending_releases.len(), 1);
            }
            let due = controller.next_release_retry.unwrap();
            for _ in 0..8 {
                assert!(controller
                    .release_policy_except(
                        ControlOwner::IoPriority,
                        &BTreeSet::from([target(42, 7).key()])
                    )
                    .failures
                    .is_empty());
                assert_eq!(controller.next_release_retry, Some(due));
            }
            let now = due - Duration::from_millis(1);
            assert_eq!(
                controller
                    .retry_pending_releases(now)
                    .static_policy
                    .restored_processes,
                0
            );
            controller.platform.fail_next(FakeFailure::Open);
            assert_eq!(
                controller
                    .retry_pending_releases(due)
                    .static_policy
                    .failures
                    .len(),
                1
            );
            assert_eq!(
                controller.release_retry_delay(due),
                Some(Duration::from_secs(1))
            );
            if replace {
                controller
                    .apply_process_list_action(&action_target(), ProcessIoPriority::Low, true)
                    .unwrap();
            }
            assert_eq!(
                controller
                    .retry_pending_releases(due + Duration::from_secs(1))
                    .static_policy
                    .restored_processes,
                usize::from(!replace)
            );
            assert_eq!(
                controller.platform.processes[&42].priority,
                if replace { 1 } else { 2 }
            );
            assert!(controller.release_retry_delay(now).is_none());
            controller.release_all_policy(ControlOwner::IoPriority);
            assert_eq!(
                controller.platform.processes[&42].priority,
                if replace { 1 } else { 2 }
            );
            controller.shutdown().unwrap();
        }
    }

    #[test]
    fn raw_values_match_windows_priority_hint_order_and_unknown_values_fail_closed() {
        assert_eq!(io_priority_raw(ProcessIoPriority::VeryLow), 0);
        assert_eq!(io_priority_raw(ProcessIoPriority::Low), 1);
        assert_eq!(io_priority_raw(ProcessIoPriority::Normal), 2);
        assert_eq!(io_priority_raw(ProcessIoPriority::High), 3);
        assert_eq!(io_priority_raw(ProcessIoPriority::Critical), 4);
        assert_eq!(io_priority_from_raw(5), None);
    }

    #[test]
    fn unmanaged_matching_state_is_not_adopted() {
        let platform = FakePlatform::new(2);
        let mut controller = IoPriorityController::with_platform(platform);

        assert_eq!(
            controller
                .apply_policy_claim(
                    claim(
                        ControlOwner::IoPriority,
                        ProcessIoPriority::Normal,
                        IoPriorityPreservation::Exact,
                    ),
                    true,
                )
                .unwrap(),
            IoPriorityApplyOutcome::Unchanged
        );
        assert!(!controller.has_managed_state());
    }

    #[test]
    fn process_list_action_is_superseded_without_losing_the_first_baseline() {
        let platform = FakePlatform::new(2);
        let mut controller = IoPriorityController::with_platform(platform);
        controller
            .apply_process_list_action(&action_target(), ProcessIoPriority::Low, true)
            .unwrap();
        controller
            .apply_policy_claim(
                claim(
                    ControlOwner::IoPriority,
                    ProcessIoPriority::VeryLow,
                    IoPriorityPreservation::Exact,
                ),
                true,
            )
            .unwrap();

        let managed = controller.managed.values().next().unwrap();
        assert_eq!(managed.baseline, 2);
        assert_eq!(managed.expected, 0);
        assert_eq!(managed.owner, ControlOwner::IoPriority);
    }

    #[test]
    fn identity_metadata_drift_preserves_the_existing_baseline() {
        let platform = FakePlatform::new(2);
        let mut controller = IoPriorityController::with_platform(platform);
        let claim = claim(
            ControlOwner::IoPriority,
            ProcessIoPriority::Low,
            IoPriorityPreservation::Exact,
        );
        controller.apply_policy_claim(claim.clone(), true).unwrap();
        let process = controller.platform.processes.get_mut(&42).unwrap();
        process.identity = ProcessIdentity::new(
            42,
            "APP-42.EXE".to_owned(),
            PathBuf::from(r"c:/apps/APP-42.exe"),
            7,
            None,
        );
        controller.apply_policy_claim(claim, true).unwrap();

        assert_eq!(controller.managed.len(), 1);
        assert_eq!(controller.managed.values().next().unwrap().baseline, 2);
    }

    #[test]
    fn unknown_windows_baseline_is_restored_exactly() {
        let platform = FakePlatform::new(5);
        let mut controller = IoPriorityController::with_platform(platform);
        controller
            .apply_policy_claim(
                claim(
                    ControlOwner::IoPriority,
                    ProcessIoPriority::Low,
                    IoPriorityPreservation::Exact,
                ),
                true,
            )
            .unwrap();

        controller.shutdown().unwrap();
        assert_eq!(controller.platform.processes[&42].priority, 5);
    }

    #[test]
    fn preservation_uses_the_original_baseline_and_releases_managed_state() {
        let platform = FakePlatform::new(2);
        let mut controller = IoPriorityController::with_platform(platform);
        controller
            .apply_policy_claim(
                claim(
                    ControlOwner::IoPriority,
                    ProcessIoPriority::Low,
                    IoPriorityPreservation::Exact,
                ),
                true,
            )
            .unwrap();
        assert_eq!(
            controller
                .apply_policy_claim(
                    claim(
                        ControlOwner::IoPriority,
                        ProcessIoPriority::Low,
                        IoPriorityPreservation::PreserveHigher,
                    ),
                    true,
                )
                .unwrap(),
            IoPriorityApplyOutcome::Preserved
        );
        assert!(!controller.has_managed_state());
        assert_eq!(controller.platform.processes[&42].priority, 2);

        controller.platform.processes.get_mut(&42).unwrap().priority = 0;
        assert_eq!(
            controller
                .apply_policy_claim(
                    claim(
                        ControlOwner::IoPriority,
                        ProcessIoPriority::Low,
                        IoPriorityPreservation::PreserveLower,
                    ),
                    true,
                )
                .unwrap(),
            IoPriorityApplyOutcome::Preserved
        );
        assert!(!controller.has_managed_state());
    }

    #[test]
    fn external_break_rebases_before_policy_is_reasserted() {
        let platform = FakePlatform::new(2);
        let mut controller = IoPriorityController::with_platform(platform);
        let claim = claim(
            ControlOwner::IoPriority,
            ProcessIoPriority::VeryLow,
            IoPriorityPreservation::Exact,
        );
        controller.apply_policy_claim(claim.clone(), true).unwrap();
        controller.platform.processes.get_mut(&42).unwrap().priority = 3;
        controller.apply_policy_claim(claim, true).unwrap();

        assert_eq!(controller.managed.values().next().unwrap().baseline, 3);
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
        let platform = FakePlatform::new(2);
        let mut controller = IoPriorityController::with_platform(platform);
        let claim = claim(
            ControlOwner::IoPriority,
            ProcessIoPriority::Low,
            IoPriorityPreservation::Exact,
        );
        controller.apply_policy_claim(claim.clone(), true).unwrap();
        controller.platform.processes.get_mut(&42).unwrap().priority = 3;
        controller.platform.fail_next(FakeFailure::Relinquish);

        assert!(controller.apply_policy_claim(claim, true).is_err());
        assert!(controller.has_managed_state());
    }

    #[test]
    fn external_break_is_not_overwritten_during_release() {
        let platform = FakePlatform::new(2);
        let mut controller = IoPriorityController::with_platform(platform);
        controller
            .apply_policy_claim(
                claim(
                    ControlOwner::IoPriority,
                    ProcessIoPriority::Low,
                    IoPriorityPreservation::Exact,
                ),
                true,
            )
            .unwrap();
        controller.platform.processes.get_mut(&42).unwrap().priority = 3;

        let summary = controller.release_all_policy(ControlOwner::IoPriority);
        assert!(summary.failures.is_empty());
        assert_eq!(summary.restored_processes, 0);
        assert_eq!(controller.platform.processes[&42].priority, 3);
    }

    #[test]
    fn process_exit_and_pid_reuse_relinquish_the_old_identity() {
        let platform = FakePlatform::new(2);
        let mut controller = IoPriorityController::with_platform(platform);
        controller
            .apply_policy_claim(
                claim(
                    ControlOwner::IoPriority,
                    ProcessIoPriority::Low,
                    IoPriorityPreservation::Exact,
                ),
                true,
            )
            .unwrap();
        controller.platform.processes.insert(
            42,
            FakeProcess {
                identity: identity(42, 8),
                priority: 2,
            },
        );
        controller
            .apply_policy_claim(
                IoPriorityClaim {
                    target: target(42, 8),
                    owner: ControlOwner::IoPriority,
                    priority: ProcessIoPriority::VeryLow,
                    preservation: IoPriorityPreservation::Exact,
                },
                true,
            )
            .unwrap();

        assert_eq!(controller.managed.len(), 1);
        assert_eq!(controller.managed.keys().next().unwrap().creation_time, 8);
        assert!(controller
            .platform
            .events
            .lock()
            .unwrap()
            .iter()
            .any(|event| event == "relinquish:42:7"));
    }

    #[test]
    fn begin_apply_and_verification_failures_compensate() {
        for failure in [FakeFailure::Begin, FakeFailure::Apply, FakeFailure::Verify] {
            let mut platform = FakePlatform::new(2);
            platform.fail_next(failure);
            let mut controller = IoPriorityController::with_platform(platform);
            assert!(controller
                .apply_policy_claim(
                    claim(
                        ControlOwner::IoPriority,
                        ProcessIoPriority::Low,
                        IoPriorityPreservation::Exact,
                    ),
                    true,
                )
                .is_err());
            assert_eq!(controller.platform.processes[&42].priority, 2);
            assert!(!controller.has_managed_state());
        }
    }

    #[test]
    fn commit_failure_retains_verified_target_and_the_first_baseline() {
        for replacing in [false, true] {
            let mut controller = IoPriorityController::with_platform(FakePlatform::new(2));
            if replacing {
                controller
                    .apply_policy_claim(
                        claim(
                            ControlOwner::IoPriority,
                            ProcessIoPriority::Low,
                            IoPriorityPreservation::Exact,
                        ),
                        true,
                    )
                    .unwrap();
            }
            controller.platform.fail_next(FakeFailure::Commit);
            let error = controller
                .apply_policy_claim(
                    claim(
                        ControlOwner::IoPriority,
                        ProcessIoPriority::High,
                        IoPriorityPreservation::Exact,
                    ),
                    true,
                )
                .unwrap_err();
            assert!(error.to_string().contains("commit failed"));
            assert_eq!(controller.platform.processes[&42].priority, 3);
            let managed = controller.managed.values().next().unwrap();
            assert_eq!((managed.baseline, managed.expected), (2, 3));
            let events = controller.platform.events.lock().unwrap();
            assert_eq!(
                events
                    .iter()
                    .filter(|event| event.starts_with("apply:"))
                    .count(),
                if replacing { 2 } else { 1 }
            );
            assert!(!events.iter().any(|event| event.starts_with("relinquish:")));
            drop(events);
            controller.shutdown().unwrap();
            assert_eq!(controller.platform.processes[&42].priority, 2);
            assert!(!controller.has_managed_state());
        }
    }

    #[test]
    fn unverified_compensation_retains_a_pending_edge_to_the_first_baseline() {
        let mut controller = IoPriorityController::with_platform(FakePlatform::new(2));
        controller
            .apply_policy_claim(
                claim(
                    ControlOwner::IoPriority,
                    ProcessIoPriority::Low,
                    IoPriorityPreservation::Exact,
                ),
                true,
            )
            .unwrap();
        controller.platform.events.lock().unwrap().clear();
        controller.platform.fail_next(FakeFailure::Verify);
        controller.platform.fail_next(FakeFailure::Verify);
        assert!(controller
            .apply_policy_claim(
                claim(
                    ControlOwner::IoPriority,
                    ProcessIoPriority::High,
                    IoPriorityPreservation::Exact
                ),
                true,
            )
            .is_err());
        assert_eq!(controller.platform.processes[&42].priority, 1);
        let managed = controller.managed.values().next().unwrap();
        assert_eq!((managed.baseline, managed.expected), (2, 3));
        let events = controller.platform.events.lock().unwrap();
        assert!(events.iter().any(|event| event == "retain_pending"));
        assert!(!events
            .iter()
            .any(|event| event == "commit" || event.starts_with("relinquish:")));
    }

    #[test]
    fn failed_compensation_keeps_uncertain_state_for_recovery() {
        let mut platform = FakePlatform::new(2);
        platform.fail_next(FakeFailure::Apply);
        platform.fail_next(FakeFailure::Apply);
        let mut controller = IoPriorityController::with_platform(platform);

        assert!(controller
            .apply_policy_claim(
                claim(
                    ControlOwner::IoPriority,
                    ProcessIoPriority::Low,
                    IoPriorityPreservation::Exact,
                ),
                true,
            )
            .is_err());
        assert!(controller.has_managed_state());
    }

    #[test]
    fn shutdown_keeps_confirmed_exit_when_followup_query_is_denied() {
        let platform = FakePlatform::new(2);
        let mut controller = IoPriorityController::with_platform(platform);
        controller
            .apply_policy_claim(
                claim(
                    ControlOwner::IoPriority,
                    ProcessIoPriority::Low,
                    IoPriorityPreservation::Exact,
                ),
                true,
            )
            .unwrap();
        controller.platform.fail_next(FakeFailure::BeginExited);
        controller.platform.fail_next(FakeFailure::QueryDenied);
        assert!(controller.shutdown().is_ok());
        assert!(!controller.has_managed_state());
    }

    #[test]
    fn shutdown_cleans_up_exit_during_restoration() {
        let platform = FakePlatform::new(2);
        let mut controller = IoPriorityController::with_platform(platform);
        controller
            .apply_policy_claim(
                claim(
                    ControlOwner::IoPriority,
                    ProcessIoPriority::Low,
                    IoPriorityPreservation::Exact,
                ),
                true,
            )
            .unwrap();
        controller.platform.fail_next(FakeFailure::ApplyExited);
        assert!(controller.shutdown().is_ok());
        assert!(!controller.has_managed_state());
    }

    #[test]
    fn release_commit_failure_keeps_baseline_and_relinquishes_recovery() {
        let platform = FakePlatform::new(2);
        let mut controller = IoPriorityController::with_platform(platform);
        controller
            .apply_policy_claim(
                claim(
                    ControlOwner::IoPriority,
                    ProcessIoPriority::Low,
                    IoPriorityPreservation::Exact,
                ),
                true,
            )
            .unwrap();
        controller.platform.fail_next(FakeFailure::Commit);

        let summary = controller.release_all_policy(ControlOwner::IoPriority);
        assert!(summary.failures.is_empty());
        assert_eq!(summary.restored_processes, 1);
        assert_eq!(controller.platform.processes[&42].priority, 2);
        assert!(!controller.has_managed_state());
    }

    #[test]
    fn failed_release_relinquish_retries_without_reapplying() {
        let platform = FakePlatform::new(2);
        let mut controller = IoPriorityController::with_platform(platform);
        controller
            .apply_policy_claim(
                claim(
                    ControlOwner::IoPriority,
                    ProcessIoPriority::Low,
                    IoPriorityPreservation::Exact,
                ),
                true,
            )
            .unwrap();
        controller.platform.fail_next(FakeFailure::Commit);
        controller.platform.fail_next(FakeFailure::Relinquish);
        let first = controller.release_all_policy(ControlOwner::IoPriority);
        assert_eq!(first.failures.len(), 1);
        assert_eq!(controller.platform.processes[&42].priority, 2);
        let apply_count = controller
            .platform
            .events
            .lock()
            .unwrap()
            .iter()
            .filter(|event| event.starts_with("apply:"))
            .count();

        let second = controller.release_all_policy(ControlOwner::IoPriority);
        assert!(second.failures.is_empty());
        assert_eq!(
            controller
                .platform
                .events
                .lock()
                .unwrap()
                .iter()
                .filter(|event| event.starts_with("apply:"))
                .count(),
            apply_count
        );
    }

    #[test]
    fn shutdown_restores_in_reverse_application_order() {
        let platform = FakePlatform::with_processes(&[(42, 7, 2), (43, 8, 2)]);
        let events = Arc::clone(&platform.events);
        let mut controller = IoPriorityController::with_platform(platform);
        for (id, creation_time) in [(42, 7), (43, 8)] {
            controller
                .apply_policy_claim(
                    IoPriorityClaim {
                        target: target(id, creation_time),
                        owner: ControlOwner::IoPriority,
                        priority: ProcessIoPriority::Low,
                        preservation: IoPriorityPreservation::Exact,
                    },
                    true,
                )
                .unwrap();
        }
        events.lock().unwrap().clear();
        controller.shutdown().unwrap();

        let restores = events
            .lock()
            .unwrap()
            .iter()
            .filter(|event| event.starts_with("apply:"))
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(restores, ["apply:43:2", "apply:42:2"]);
    }

    #[test]
    #[ignore = "modifies and restores a disposable Windows process; run in explicit integration QA"]
    fn io_priority_live_apply_and_clean_release() -> Result<(), String> {
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
        let baseline = current_process_io_priority(&target, true)?;
        let expected = if baseline == ProcessIoPriority::VeryLow {
            ProcessIoPriority::Low
        } else {
            ProcessIoPriority::VeryLow
        };
        let mut controller = IoPriorityController::default();

        if baseline == ProcessIoPriority::VeryLow {
            controller
                .apply_process_list_action(&target, expected, true)
                .map_err(|error| error.to_string())?;
        } else {
            let control_target = ProcessControlTarget::from_action_target(&target);
            let active = BTreeSet::from([control_target.key()]);
            controller.reconcile_adaptive_restraint_targets(&active, &active);
            let outcome = controller
                .apply_policy_claim(
                    IoPriorityClaim {
                        target: control_target,
                        owner: ControlOwner::AdaptiveEngine,
                        priority: expected,
                        preservation: IoPriorityPreservation::PreserveLower,
                    },
                    true,
                )
                .map_err(|error| error.to_string())?;
            assert_eq!(outcome, IoPriorityApplyOutcome::Applied);
            assert_eq!(current_process_io_priority(&target, true)?, expected);
            let outcome = controller
                .apply_process_list_action(&target, expected, true)
                .map_err(|error| error.to_string())?;
            assert_eq!(outcome, IoPriorityApplyOutcome::Unchanged);
            let release = controller.release_all_policy(ControlOwner::AdaptiveEngine);
            assert!(release.failures.is_empty());
            assert_eq!(release.restored_processes, 0);
        }
        assert_eq!(current_process_io_priority(&target, true)?, expected);
        controller.shutdown()?;
        assert_eq!(current_process_io_priority(&target, true)?, baseline);
        Ok(())
    }
}
