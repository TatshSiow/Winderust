use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, Instant},
};

use crate::{
    backend::crash_recovery::{
        forget_gpu_priority_change, record_process_change, ProcessValue, RecoveryIntent,
    },
    config::ProcessGpuPriority,
    foreground::ProcessActionTarget,
    platform::windows::gpu_priority::{self as windows_gpu_priority, GpuPriorityError},
    win_util::WinHandle,
};

use super::process::{
    open_process_for_set_information, transition_failure_error, ControlOwner, ProcessControlError,
    ProcessControlTarget, ProcessIdentity, ProcessTargetKey,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GpuPriorityPreservation {
    Exact,
    PreserveHigher,
    PreserveLower,
}

#[derive(Debug, Clone)]
pub(crate) struct GpuPriorityClaim {
    pub(crate) target: ProcessControlTarget,
    pub(crate) owner: ControlOwner,
    pub(crate) priority: ProcessGpuPriority,
    pub(crate) preservation: GpuPriorityPreservation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum GpuPriorityApplyOutcome {
    Applied,
    Unchanged,
    Preserved,
}

#[derive(Debug)]
pub(crate) struct GpuPriorityReleaseFailure {
    pub(crate) process_id: u32,
    pub(crate) process_name: String,
    pub(crate) executable_path: String,
    pub(crate) error: ProcessControlError,
}

#[derive(Debug, Default)]
pub(crate) struct GpuPriorityReleaseSummary {
    pub(crate) restored_processes: usize,
    pub(crate) failures: Vec<GpuPriorityReleaseFailure>,
}

struct ManagedGpuPriority {
    baseline: u32,
    expected: u32,
    owner: ControlOwner,
    apply_sequence: u64,
}

pub(crate) trait GpuPriorityRecoveryIntent {
    fn commit(self) -> Result<(), String>;
    fn retain_pending(self) -> Result<(), String>;
}

impl GpuPriorityRecoveryIntent for RecoveryIntent {
    fn commit(self) -> Result<(), String> {
        RecoveryIntent::commit(self)
    }

    fn retain_pending(self) -> Result<(), String> {
        RecoveryIntent::retain_pending(self)
    }
}

pub(crate) trait GpuPriorityPlatform {
    type Process;
    type RecoveryIntent: GpuPriorityRecoveryIntent;

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
pub(crate) struct WindowsGpuPriorityPlatform;

impl GpuPriorityPlatform for WindowsGpuPriorityPlatform {
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
        windows_gpu_priority::query(process).map_err(gpu_error)
    }

    fn begin_change(
        &mut self,
        process: &Self::Process,
        original: u32,
        expected: u32,
    ) -> Result<Self::RecoveryIntent, ProcessControlError> {
        record_process_change(
            process.raw(),
            ProcessValue::GpuPriority(original),
            ProcessValue::GpuPriority(expected),
        )
        .map_err(ProcessControlError::Failed)
    }

    fn apply(&mut self, process: &Self::Process, priority: u32) -> Result<(), ProcessControlError> {
        windows_gpu_priority::set(process, priority).map_err(gpu_error)
    }

    fn relinquish(&mut self, identity: &ProcessIdentity) -> Result<(), ProcessControlError> {
        forget_gpu_priority_change(identity.id, identity.creation_time)
            .map_err(ProcessControlError::Failed)
    }
}

pub(crate) struct GpuPriorityController<P: GpuPriorityPlatform = WindowsGpuPriorityPlatform> {
    platform: P,
    managed: BTreeMap<ProcessIdentity, ManagedGpuPriority>,
    next_apply_sequence: u64,
    pending_releases: BTreeSet<ProcessIdentity>,
    next_release_retry: Option<Instant>,
}

impl Default for GpuPriorityController {
    fn default() -> Self {
        Self::with_platform(WindowsGpuPriorityPlatform)
    }
}

impl<P: GpuPriorityPlatform> GpuPriorityController<P> {
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
        claim: GpuPriorityClaim,
        allow_cross_session_process_control: bool,
    ) -> Result<GpuPriorityApplyOutcome, ProcessControlError> {
        if !matches!(
            claim.owner,
            ControlOwner::GpuPriority | ControlOwner::AdaptiveEngine
        ) {
            return Err(ProcessControlError::Failed(
                "This owner cannot control GPU Priority policy.".to_owned(),
            ));
        }
        self.apply_claim(claim, allow_cross_session_process_control)
    }

    pub(crate) fn apply_process_list_action(
        &mut self,
        target: &ProcessActionTarget,
        priority: ProcessGpuPriority,
        allow_cross_session_process_control: bool,
    ) -> Result<GpuPriorityApplyOutcome, ProcessControlError> {
        self.apply_claim(
            GpuPriorityClaim {
                target: ProcessControlTarget::from_action_target(target),
                owner: ControlOwner::ProcessList,
                priority,
                preservation: GpuPriorityPreservation::Exact,
            },
            allow_cross_session_process_control,
        )
    }

    fn apply_claim(
        &mut self,
        claim: GpuPriorityClaim,
        allow_cross_session_process_control: bool,
    ) -> Result<GpuPriorityApplyOutcome, ProcessControlError> {
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

        let desired = gpu_priority_raw(claim.priority);
        let baseline = managed.as_ref().map_or(current, |managed| managed.baseline);
        if priority_is_preserved(claim.preservation, baseline, desired) {
            if let Some(managed) = managed {
                self.managed.insert(identity.clone(), managed);
                self.release_identity(&identity)?;
                self.managed.remove(&identity);
                self.pending_releases.remove(&identity);
            }
            return Ok(GpuPriorityApplyOutcome::Preserved);
        }

        if current == desired {
            if let Some(mut managed) = managed {
                if claim.owner.is_automatic() || managed.owner == ControlOwner::ProcessList {
                    managed.owner = claim.owner;
                    self.pending_releases.remove(&identity);
                }
                managed.expected = current;
                self.managed.insert(identity, managed);
            }
            return Ok(GpuPriorityApplyOutcome::Unchanged);
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
                self.pending_releases.remove(&identity);
                self.managed.insert(
                    identity,
                    ManagedGpuPriority {
                        baseline,
                        expected: desired,
                        owner: claim.owner,
                        apply_sequence: sequence,
                    },
                );
                Ok(GpuPriorityApplyOutcome::Applied)
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
                        ManagedGpuPriority {
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
        active_targets: &BTreeSet<ProcessTargetKey>,
    ) -> GpuPriorityReleaseSummary {
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

    pub(crate) fn retry_pending_releases(&mut self, now: Instant) -> GpuPriorityReleaseSummary {
        if self.release_retry_delay(now) != Some(Duration::ZERO) {
            return GpuPriorityReleaseSummary::default();
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

    pub(crate) fn release_all_policy(&mut self) -> GpuPriorityReleaseSummary {
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
    ) -> GpuPriorityReleaseSummary {
        identities.sort_by_key(|identity| std::cmp::Reverse(identity.0));
        let mut summary = GpuPriorityReleaseSummary::default();
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
                Err(error) => summary.failures.push(GpuPriorityReleaseFailure {
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

impl<P: GpuPriorityPlatform> Drop for GpuPriorityController<P> {
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

fn apply_transition<P: GpuPriorityPlatform>(
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
                    "GPU Priority verification returned {actual}, expected {expected}."
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

fn compensate_with_intent<P: GpuPriorityPlatform>(
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

fn restore_and_verify<P: GpuPriorityPlatform>(
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
    preservation: GpuPriorityPreservation,
    baseline: u32,
    desired: u32,
) -> bool {
    match preservation {
        GpuPriorityPreservation::Exact => false,
        GpuPriorityPreservation::PreserveHigher => baseline >= desired,
        GpuPriorityPreservation::PreserveLower => baseline <= desired,
    }
}

pub(crate) const fn gpu_priority_raw(priority: ProcessGpuPriority) -> u32 {
    match priority {
        ProcessGpuPriority::Idle => 0,
        ProcessGpuPriority::BelowNormal => 1,
        ProcessGpuPriority::Normal => 2,
        ProcessGpuPriority::AboveNormal => 3,
        ProcessGpuPriority::High => 4,
        ProcessGpuPriority::Realtime => 5,
    }
}

fn gpu_priority_from_raw(priority: u32) -> Option<ProcessGpuPriority> {
    match priority {
        0 => Some(ProcessGpuPriority::Idle),
        1 => Some(ProcessGpuPriority::BelowNormal),
        2 => Some(ProcessGpuPriority::Normal),
        3 => Some(ProcessGpuPriority::AboveNormal),
        4 => Some(ProcessGpuPriority::High),
        5 => Some(ProcessGpuPriority::Realtime),
        _ => None,
    }
}

pub(crate) fn current_process_gpu_priority(
    target: &ProcessActionTarget,
    allow_cross_session_process_control: bool,
) -> Result<ProcessGpuPriority, String> {
    let mut platform = WindowsGpuPriorityPlatform;
    let (_, process) = platform
        .open(
            &ProcessControlTarget::from_action_target(target),
            allow_cross_session_process_control,
        )
        .map_err(|error| error.to_string())?;
    let raw = platform
        .query(&process)
        .map_err(|error| error.to_string())?;
    gpu_priority_from_raw(raw)
        .ok_or_else(|| format!("Windows returned unsupported GPU priority value {raw}."))
}

fn gpu_error(error: GpuPriorityError) -> ProcessControlError {
    match error {
        GpuPriorityError::ProcessExited => ProcessControlError::ProcessExited,
        GpuPriorityError::ContextUnavailable => ProcessControlError::Unavailable(
            "GPU scheduling priority is not available yet for this process.".to_owned(),
        ),
        GpuPriorityError::InvalidReturnedPriority(priority) => ProcessControlError::Failed(
            format!("Windows returned invalid GPU priority {priority}."),
        ),
        GpuPriorityError::PriorityOutOfRange(priority) => {
            ProcessControlError::Failed(format!("GPU priority {priority} is out of range."))
        }
        GpuPriorityError::NtStatus(status) => {
            ProcessControlError::Failed(format!("NTSTATUS 0x{status:08X}."))
        }
    }
}

#[cfg(test)]
mod tests;
