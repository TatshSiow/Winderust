use std::collections::{BTreeMap, BTreeSet};

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
    open_process_for_set_information, ControlOwner, ProcessControlError, ProcessControlTarget,
    ProcessIdentity, ProcessTargetKey,
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
}

impl GpuPriorityRecoveryIntent for RecoveryIntent {
    fn commit(self) -> Result<(), String> {
        RecoveryIntent::commit(self)
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

        let desired = gpu_priority_raw(claim.priority);
        let baseline = managed.as_ref().map_or(current, |managed| managed.baseline);
        if priority_is_preserved(claim.preservation, baseline, desired) {
            if let Some(managed) = managed {
                self.managed.insert(identity.clone(), managed);
                self.release_identity(&identity)?;
                self.managed.remove(&identity);
            }
            return Ok(GpuPriorityApplyOutcome::Preserved);
        }

        if current == desired {
            if let Some(mut managed) = managed {
                if claim.owner.is_automatic() || managed.owner == ControlOwner::ProcessList {
                    managed.owner = claim.owner;
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
            CommitFailureBehavior::Compensate,
        ) {
            Ok(()) => {
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
                        failure
                            .message
                            .push_str(&format!(" Recovery journal relinquish failed: {error}."));
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
                Err(ProcessControlError::Failed(failure.message))
            }
        }
    }

    pub(crate) fn release_policy_except(
        &mut self,
        active_targets: &BTreeSet<ProcessTargetKey>,
    ) -> GpuPriorityReleaseSummary {
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
        identities.sort_by(|left, right| right.0.cmp(&left.0));
        let mut summary = GpuPriorityReleaseSummary::default();
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
                        Err(error) => summary.failures.push(GpuPriorityReleaseFailure {
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

impl<P: GpuPriorityPlatform> Drop for GpuPriorityController<P> {
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
                format!("GPU Priority verification returned {actual}, expected {expected}."),
            ));
        }
        Err(error) => {
            return Err(compensate_with_intent(
                platform,
                process,
                original,
                intent,
                format!("GPU Priority verification failed: {error}"),
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

fn compensate_with_intent<P: GpuPriorityPlatform>(
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

fn compensate_without_intent<P: GpuPriorityPlatform>(
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
mod tests {
    use std::{
        collections::VecDeque,
        path::PathBuf,
        sync::{Arc, Mutex},
    };

    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum FakeFailure {
        Open,
        Unavailable,
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

    impl GpuPriorityRecoveryIntent for FakeRecoveryIntent {
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

    impl GpuPriorityPlatform for FakePlatform {
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
            let follows_apply = self
                .events
                .lock()
                .unwrap()
                .last()
                .is_some_and(|event| event.starts_with("apply:"));
            self.events.lock().unwrap().push("query".to_owned());
            if self.take_failure(FakeFailure::Unavailable) {
                return Err(ProcessControlError::Unavailable(
                    "GPU context unavailable".to_owned(),
                ));
            }
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
        priority: ProcessGpuPriority,
        preservation: GpuPriorityPreservation,
    ) -> GpuPriorityClaim {
        GpuPriorityClaim {
            target: target(42, 7),
            owner,
            priority,
            preservation,
        }
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
    fn raw_values_match_windows_priority_hint_order_and_unknown_values_fail_closed() {
        assert_eq!(gpu_priority_raw(ProcessGpuPriority::Idle), 0);
        assert_eq!(gpu_priority_raw(ProcessGpuPriority::BelowNormal), 1);
        assert_eq!(gpu_priority_raw(ProcessGpuPriority::Normal), 2);
        assert_eq!(gpu_priority_raw(ProcessGpuPriority::AboveNormal), 3);
        assert_eq!(gpu_priority_raw(ProcessGpuPriority::High), 4);
        assert_eq!(gpu_priority_raw(ProcessGpuPriority::Realtime), 5);
        assert_eq!(gpu_priority_from_raw(0), Some(ProcessGpuPriority::Idle));
        assert_eq!(
            gpu_priority_from_raw(1),
            Some(ProcessGpuPriority::BelowNormal)
        );
        assert_eq!(gpu_priority_from_raw(2), Some(ProcessGpuPriority::Normal));
        assert_eq!(
            gpu_priority_from_raw(3),
            Some(ProcessGpuPriority::AboveNormal)
        );
        assert_eq!(gpu_priority_from_raw(4), Some(ProcessGpuPriority::High));
        assert_eq!(gpu_priority_from_raw(5), Some(ProcessGpuPriority::Realtime));
        assert_eq!(gpu_priority_from_raw(6), None);
    }

    #[test]
    fn gpu_status_classifies_exit_context_unavailable_and_other_failures() {
        assert_eq!(
            gpu_error(GpuPriorityError::ProcessExited),
            ProcessControlError::ProcessExited
        );
        assert!(matches!(
            gpu_error(GpuPriorityError::ContextUnavailable),
            ProcessControlError::Unavailable(_)
        ));
        assert!(matches!(
            gpu_error(GpuPriorityError::NtStatus(0xC000_0001)),
            ProcessControlError::Failed(message) if message.contains("0xC0000001")
        ));
    }

    #[test]
    fn unmanaged_matching_state_is_not_adopted() {
        let platform = FakePlatform::new(2);
        let mut controller = GpuPriorityController::with_platform(platform);

        assert_eq!(
            controller
                .apply_policy_claim(
                    claim(
                        ControlOwner::GpuPriority,
                        ProcessGpuPriority::Normal,
                        GpuPriorityPreservation::Exact,
                    ),
                    true,
                )
                .unwrap(),
            GpuPriorityApplyOutcome::Unchanged
        );
        assert!(!controller.has_managed_state());
    }

    #[test]
    fn unavailable_gpu_context_is_typed_and_does_not_create_managed_state() {
        let mut platform = FakePlatform::new(2);
        platform.fail_next(FakeFailure::Unavailable);
        let mut controller = GpuPriorityController::with_platform(platform);

        assert!(matches!(
            controller.apply_policy_claim(
                claim(
                    ControlOwner::GpuPriority,
                    ProcessGpuPriority::BelowNormal,
                    GpuPriorityPreservation::Exact,
                ),
                true,
            ),
            Err(ProcessControlError::Unavailable(message))
                if message == "GPU context unavailable"
        ));
        assert!(!controller.has_managed_state());
    }

    #[test]
    fn process_list_action_is_superseded_without_losing_the_first_baseline() {
        let platform = FakePlatform::new(2);
        let mut controller = GpuPriorityController::with_platform(platform);
        controller
            .apply_process_list_action(&action_target(), ProcessGpuPriority::BelowNormal, true)
            .unwrap();
        controller
            .apply_policy_claim(
                claim(
                    ControlOwner::GpuPriority,
                    ProcessGpuPriority::Idle,
                    GpuPriorityPreservation::Exact,
                ),
                true,
            )
            .unwrap();

        let managed = controller.managed.values().next().unwrap();
        assert_eq!(managed.baseline, 2);
        assert_eq!(managed.expected, 0);
        assert_eq!(managed.owner, ControlOwner::GpuPriority);
    }

    #[test]
    fn static_to_adaptive_replacement_keeps_the_first_baseline() {
        let platform = FakePlatform::new(2);
        let mut controller = GpuPriorityController::with_platform(platform);
        controller
            .apply_policy_claim(
                claim(
                    ControlOwner::GpuPriority,
                    ProcessGpuPriority::BelowNormal,
                    GpuPriorityPreservation::Exact,
                ),
                true,
            )
            .unwrap();
        controller
            .apply_policy_claim(
                claim(
                    ControlOwner::AdaptiveEngine,
                    ProcessGpuPriority::Idle,
                    GpuPriorityPreservation::Exact,
                ),
                true,
            )
            .unwrap();

        let managed = controller.managed.values().next().unwrap();
        assert_eq!(managed.baseline, 2);
        assert_eq!(managed.owner, ControlOwner::AdaptiveEngine);
    }

    #[test]
    fn identity_metadata_drift_preserves_the_existing_baseline() {
        let platform = FakePlatform::new(2);
        let mut controller = GpuPriorityController::with_platform(platform);
        let claim = claim(
            ControlOwner::GpuPriority,
            ProcessGpuPriority::BelowNormal,
            GpuPriorityPreservation::Exact,
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
        let platform = FakePlatform::new(9);
        let mut controller = GpuPriorityController::with_platform(platform);
        controller
            .apply_policy_claim(
                claim(
                    ControlOwner::GpuPriority,
                    ProcessGpuPriority::BelowNormal,
                    GpuPriorityPreservation::Exact,
                ),
                true,
            )
            .unwrap();

        controller.shutdown().unwrap();
        assert_eq!(controller.platform.processes[&42].priority, 9);
    }

    #[test]
    fn preservation_uses_the_original_baseline_and_releases_managed_state() {
        let platform = FakePlatform::new(2);
        let mut controller = GpuPriorityController::with_platform(platform);
        controller
            .apply_policy_claim(
                claim(
                    ControlOwner::GpuPriority,
                    ProcessGpuPriority::BelowNormal,
                    GpuPriorityPreservation::Exact,
                ),
                true,
            )
            .unwrap();
        assert_eq!(
            controller
                .apply_policy_claim(
                    claim(
                        ControlOwner::AdaptiveEngine,
                        ProcessGpuPriority::BelowNormal,
                        GpuPriorityPreservation::PreserveHigher,
                    ),
                    true,
                )
                .unwrap(),
            GpuPriorityApplyOutcome::Preserved
        );
        assert!(!controller.has_managed_state());
        assert_eq!(controller.platform.processes[&42].priority, 2);

        controller.platform.processes.get_mut(&42).unwrap().priority = 0;
        assert_eq!(
            controller
                .apply_policy_claim(
                    claim(
                        ControlOwner::GpuPriority,
                        ProcessGpuPriority::BelowNormal,
                        GpuPriorityPreservation::PreserveLower,
                    ),
                    true,
                )
                .unwrap(),
            GpuPriorityApplyOutcome::Preserved
        );
        assert!(!controller.has_managed_state());
    }

    #[test]
    fn external_break_rebases_before_policy_is_reasserted() {
        let platform = FakePlatform::new(2);
        let mut controller = GpuPriorityController::with_platform(platform);
        let claim = claim(
            ControlOwner::GpuPriority,
            ProcessGpuPriority::Idle,
            GpuPriorityPreservation::Exact,
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
        let mut controller = GpuPriorityController::with_platform(platform);
        let claim = claim(
            ControlOwner::GpuPriority,
            ProcessGpuPriority::BelowNormal,
            GpuPriorityPreservation::Exact,
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
        let mut controller = GpuPriorityController::with_platform(platform);
        controller
            .apply_policy_claim(
                claim(
                    ControlOwner::GpuPriority,
                    ProcessGpuPriority::BelowNormal,
                    GpuPriorityPreservation::Exact,
                ),
                true,
            )
            .unwrap();
        controller.platform.processes.get_mut(&42).unwrap().priority = 3;

        let summary = controller.release_all_policy();
        assert!(summary.failures.is_empty());
        assert_eq!(summary.restored_processes, 0);
        assert_eq!(controller.platform.processes[&42].priority, 3);
    }

    #[test]
    fn process_exit_and_pid_reuse_relinquish_the_old_identity() {
        let platform = FakePlatform::new(2);
        let mut controller = GpuPriorityController::with_platform(platform);
        controller
            .apply_policy_claim(
                claim(
                    ControlOwner::GpuPriority,
                    ProcessGpuPriority::BelowNormal,
                    GpuPriorityPreservation::Exact,
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
                GpuPriorityClaim {
                    target: target(42, 8),
                    owner: ControlOwner::GpuPriority,
                    priority: ProcessGpuPriority::Idle,
                    preservation: GpuPriorityPreservation::Exact,
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
    fn begin_apply_verification_and_commit_failures_compensate() {
        for failure in [
            FakeFailure::Begin,
            FakeFailure::Apply,
            FakeFailure::Verify,
            FakeFailure::Commit,
        ] {
            let mut platform = FakePlatform::new(2);
            platform.fail_next(failure);
            let mut controller = GpuPriorityController::with_platform(platform);
            assert!(controller
                .apply_policy_claim(
                    claim(
                        ControlOwner::GpuPriority,
                        ProcessGpuPriority::BelowNormal,
                        GpuPriorityPreservation::Exact,
                    ),
                    true,
                )
                .is_err());
            assert_eq!(controller.platform.processes[&42].priority, 2);
            assert!(!controller.has_managed_state());
        }
    }

    #[test]
    fn failed_compensation_keeps_uncertain_state_for_recovery() {
        let mut platform = FakePlatform::new(2);
        platform.fail_next(FakeFailure::Apply);
        platform.fail_next(FakeFailure::Apply);
        let mut controller = GpuPriorityController::with_platform(platform);

        assert!(controller
            .apply_policy_claim(
                claim(
                    ControlOwner::GpuPriority,
                    ProcessGpuPriority::BelowNormal,
                    GpuPriorityPreservation::Exact,
                ),
                true,
            )
            .is_err());
        assert!(controller.has_managed_state());
    }

    #[test]
    fn release_commit_failure_keeps_baseline_and_relinquishes_recovery() {
        let platform = FakePlatform::new(2);
        let mut controller = GpuPriorityController::with_platform(platform);
        controller
            .apply_policy_claim(
                claim(
                    ControlOwner::GpuPriority,
                    ProcessGpuPriority::BelowNormal,
                    GpuPriorityPreservation::Exact,
                ),
                true,
            )
            .unwrap();
        controller.platform.fail_next(FakeFailure::Commit);

        let summary = controller.release_all_policy();
        assert!(summary.failures.is_empty());
        assert_eq!(summary.restored_processes, 1);
        assert_eq!(controller.platform.processes[&42].priority, 2);
        assert!(!controller.has_managed_state());
    }

    #[test]
    fn failed_release_relinquish_retries_without_reapplying() {
        let platform = FakePlatform::new(2);
        let mut controller = GpuPriorityController::with_platform(platform);
        controller
            .apply_policy_claim(
                claim(
                    ControlOwner::GpuPriority,
                    ProcessGpuPriority::BelowNormal,
                    GpuPriorityPreservation::Exact,
                ),
                true,
            )
            .unwrap();
        controller.platform.fail_next(FakeFailure::Commit);
        controller.platform.fail_next(FakeFailure::Relinquish);
        let first = controller.release_all_policy();
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

        let second = controller.release_all_policy();
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
        let mut controller = GpuPriorityController::with_platform(platform);
        for (id, creation_time) in [(42, 7), (43, 8)] {
            controller
                .apply_policy_claim(
                    GpuPriorityClaim {
                        target: target(id, creation_time),
                        owner: ControlOwner::GpuPriority,
                        priority: ProcessGpuPriority::BelowNormal,
                        preservation: GpuPriorityPreservation::Exact,
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
    #[ignore = "modifies and restores the explicit WINDERUST_GPU_TEST_PID target; run in integration QA"]
    fn gpu_priority_live_apply_and_clean_release() -> Result<(), String> {
        let process_id = std::env::var("WINDERUST_GPU_TEST_PID")
            .map_err(|_| {
                "Set WINDERUST_GPU_TEST_PID to a disposable GPU-using process.".to_owned()
            })?
            .parse::<u32>()
            .map_err(|error| format!("WINDERUST_GPU_TEST_PID is invalid: {error}"))?;
        let executable_path = std::env::var_os("WINDERUST_GPU_TEST_PATH")
            .map(PathBuf::from)
            .ok_or_else(|| {
                "Set WINDERUST_GPU_TEST_PATH to that process's absolute executable path.".to_owned()
            })?;
        let target =
            crate::foreground::capture_process_action_target(process_id, &executable_path, true)
                .map_err(|error| error.to_string())?;
        let baseline = current_process_gpu_priority(&target, true)?;
        let expected = if baseline == ProcessGpuPriority::Idle {
            ProcessGpuPriority::BelowNormal
        } else {
            ProcessGpuPriority::Idle
        };
        let mut controller = GpuPriorityController::default();

        controller
            .apply_process_list_action(&target, expected, true)
            .map_err(|error| error.to_string())?;
        assert_eq!(current_process_gpu_priority(&target, true)?, expected);
        controller.shutdown()?;
        assert_eq!(current_process_gpu_priority(&target, true)?, baseline);
        Ok(())
    }
}
