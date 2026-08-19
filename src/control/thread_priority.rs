use std::collections::{BTreeMap, BTreeSet};

use crate::{
    backend::crash_recovery::{
        forget_thread_priority_change, record_thread_priority_change, RecoveryIntent,
    },
    config::ProcessThreadPrioritySetting,
    foreground::ProcessActionTarget,
    platform::windows::thread_priority::{self as windows_thread_priority, ThreadPriorityError},
    win_util::WinHandle,
};

use super::process::{
    open_process_for_thread_control, ControlOwner, ProcessControlError, ProcessControlTarget,
    ProcessIdentity, ProcessTargetKey,
};

#[cfg(test)]
use windows_thread_priority::{
    PRIORITY_ABOVE_NORMAL as THREAD_PRIORITY_ABOVE_NORMAL,
    PRIORITY_BELOW_NORMAL as THREAD_PRIORITY_BELOW_NORMAL,
    PRIORITY_HIGHEST as THREAD_PRIORITY_HIGHEST, PRIORITY_LOWEST as THREAD_PRIORITY_LOWEST,
    PRIORITY_NORMAL as THREAD_PRIORITY_NORMAL,
    PRIORITY_TIME_CRITICAL as THREAD_PRIORITY_TIME_CRITICAL,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ThreadPriorityPreservation {
    Exact,
    PreserveHigher,
    PreserveLower,
}

#[derive(Debug, Clone)]
pub(crate) struct ThreadPriorityClaim {
    pub(crate) target: ProcessControlTarget,
    pub(crate) owner: ControlOwner,
    pub(crate) priority: ProcessThreadPrioritySetting,
    pub(crate) preservation: ThreadPriorityPreservation,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct ThreadPriorityApplyOutcome {
    pub(crate) applied_threads: usize,
    pub(crate) unchanged_threads: usize,
    pub(crate) preserved_threads: usize,
}

#[derive(Debug)]
pub(crate) struct ThreadPriorityReleaseFailure {
    pub(crate) process_id: u32,
    pub(crate) process_name: String,
    pub(crate) thread_id: u32,
    pub(crate) error: ProcessControlError,
}

#[derive(Debug, Default)]
pub(crate) struct ThreadPriorityReleaseSummary {
    pub(crate) restored_threads: usize,
    pub(crate) failures: Vec<ThreadPriorityReleaseFailure>,
}

#[derive(Debug, Clone)]
pub(crate) struct ThreadIdentity {
    pub(crate) process: ProcessIdentity,
    pub(crate) id: u32,
    pub(crate) creation_time: u64,
}

impl PartialEq for ThreadIdentity {
    fn eq(&self, other: &Self) -> bool {
        self.process == other.process
            && self.id == other.id
            && self.creation_time == other.creation_time
    }
}

impl Eq for ThreadIdentity {}

impl PartialOrd for ThreadIdentity {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ThreadIdentity {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        (&self.process, self.id, self.creation_time).cmp(&(
            &other.process,
            other.id,
            other.creation_time,
        ))
    }
}

struct ManagedThreadPriority {
    baseline: i32,
    expected: i32,
    owner: ControlOwner,
    apply_sequence: u64,
}

pub(crate) trait ThreadPriorityRecoveryIntent {
    fn commit(self) -> Result<(), String>;
}

impl ThreadPriorityRecoveryIntent for RecoveryIntent {
    fn commit(self) -> Result<(), String> {
        RecoveryIntent::commit(self)
    }
}

pub(crate) trait ThreadPriorityPlatform {
    type Process;
    type Thread;
    type RecoveryIntent: ThreadPriorityRecoveryIntent;

    fn open_process(
        &mut self,
        target: &ProcessControlTarget,
        allow_cross_session_process_control: bool,
    ) -> Result<(ProcessIdentity, Self::Process), ProcessControlError>;
    fn thread_ids(
        &mut self,
        process: &Self::Process,
        identity: &ProcessIdentity,
    ) -> Result<Vec<u32>, ProcessControlError>;
    fn open_thread(
        &mut self,
        process: &Self::Process,
        identity: &ProcessIdentity,
        thread_id: u32,
    ) -> Result<(ThreadIdentity, Self::Thread), ProcessControlError>;
    fn query(&mut self, thread: &Self::Thread) -> Result<i32, ProcessControlError>;
    fn begin_change(
        &mut self,
        process: &Self::Process,
        thread: &Self::Thread,
        original: i32,
        expected: i32,
    ) -> Result<Self::RecoveryIntent, ProcessControlError>;
    fn apply(&mut self, thread: &Self::Thread, priority: i32) -> Result<(), ProcessControlError>;
    fn relinquish(&mut self, identity: &ThreadIdentity) -> Result<(), ProcessControlError>;
}

#[derive(Default)]
pub(crate) struct WindowsThreadPriorityPlatform;

pub(crate) struct WindowsThread {
    identity: ThreadIdentity,
    handle: windows_thread_priority::ThreadHandle,
}

impl WindowsThread {
    fn validate(&self) -> Result<(), ProcessControlError> {
        let owner_process_id = windows_thread_priority::owner_process_id(&self.handle)
            .map_err(map_thread_priority_error)?;
        if owner_process_id != self.identity.process.id {
            return Err(ProcessControlError::ProcessExited);
        }
        let creation_time = windows_thread_priority::creation_time(&self.handle)
            .map_err(map_thread_priority_error)?;
        if creation_time != self.identity.creation_time {
            return Err(ProcessControlError::ProcessExited);
        }
        Ok(())
    }
}

impl ThreadPriorityPlatform for WindowsThreadPriorityPlatform {
    type Process = WinHandle;
    type Thread = WindowsThread;
    type RecoveryIntent = RecoveryIntent;

    fn open_process(
        &mut self,
        target: &ProcessControlTarget,
        allow_cross_session_process_control: bool,
    ) -> Result<(ProcessIdentity, Self::Process), ProcessControlError> {
        open_process_for_thread_control(target, allow_cross_session_process_control)
    }

    fn thread_ids(
        &mut self,
        _: &Self::Process,
        identity: &ProcessIdentity,
    ) -> Result<Vec<u32>, ProcessControlError> {
        windows_thread_priority::thread_ids(identity.id).map_err(map_thread_priority_error)
    }

    fn open_thread(
        &mut self,
        _: &Self::Process,
        identity: &ProcessIdentity,
        thread_id: u32,
    ) -> Result<(ThreadIdentity, Self::Thread), ProcessControlError> {
        let handle =
            windows_thread_priority::open_thread(thread_id).map_err(map_thread_priority_error)?;
        let owner_process_id = windows_thread_priority::owner_process_id(&handle)
            .map_err(map_thread_priority_error)?;
        if owner_process_id != identity.id {
            return Err(ProcessControlError::ProcessExited);
        }
        let thread_identity = ThreadIdentity {
            process: identity.clone(),
            id: thread_id,
            creation_time: windows_thread_priority::creation_time(&handle)
                .map_err(map_thread_priority_error)?,
        };
        Ok((
            thread_identity.clone(),
            WindowsThread {
                identity: thread_identity,
                handle,
            },
        ))
    }

    fn query(&mut self, thread: &Self::Thread) -> Result<i32, ProcessControlError> {
        thread.validate()?;
        windows_thread_priority::query_priority(&thread.handle).map_err(map_thread_priority_error)
    }

    fn begin_change(
        &mut self,
        process: &Self::Process,
        thread: &Self::Thread,
        original: i32,
        expected: i32,
    ) -> Result<Self::RecoveryIntent, ProcessControlError> {
        thread.validate()?;
        record_thread_priority_change(process.raw(), thread.handle.raw(), original, expected)
            .map_err(ProcessControlError::Failed)
    }

    fn apply(&mut self, thread: &Self::Thread, priority: i32) -> Result<(), ProcessControlError> {
        thread.validate()?;
        windows_thread_priority::set_priority(&thread.handle, priority)
            .map_err(map_thread_priority_error)
    }

    fn relinquish(&mut self, identity: &ThreadIdentity) -> Result<(), ProcessControlError> {
        forget_thread_priority_change(
            identity.process.id,
            identity.process.creation_time,
            identity.id,
            identity.creation_time,
        )
        .map_err(ProcessControlError::Failed)
    }
}

pub(crate) struct ThreadPriorityController<
    P: ThreadPriorityPlatform = WindowsThreadPriorityPlatform,
> {
    platform: P,
    managed: BTreeMap<ThreadIdentity, ManagedThreadPriority>,
    next_apply_sequence: u64,
}

impl Default for ThreadPriorityController {
    fn default() -> Self {
        Self::with_platform(WindowsThreadPriorityPlatform)
    }
}

impl<P: ThreadPriorityPlatform> ThreadPriorityController<P> {
    fn with_platform(platform: P) -> Self {
        Self {
            platform,
            managed: BTreeMap::new(),
            next_apply_sequence: 1,
        }
    }

    pub(crate) fn apply_policy_claim(
        &mut self,
        claim: ThreadPriorityClaim,
        allow_cross_session_process_control: bool,
    ) -> Result<ThreadPriorityApplyOutcome, ProcessControlError> {
        if !matches!(
            claim.owner,
            ControlOwner::ThreadPriority | ControlOwner::AdaptiveEngine
        ) {
            return Err(ProcessControlError::Failed(
                "This owner cannot control Thread Priority policy.".to_owned(),
            ));
        }
        self.apply_process_claim(claim, allow_cross_session_process_control)
    }

    pub(crate) fn apply_process_list_action(
        &mut self,
        target: &ProcessActionTarget,
        priority: ProcessThreadPrioritySetting,
        allow_cross_session_process_control: bool,
    ) -> Result<ThreadPriorityApplyOutcome, ProcessControlError> {
        self.apply_process_claim(
            ThreadPriorityClaim {
                target: ProcessControlTarget::from_action_target(target),
                owner: ControlOwner::ProcessList,
                priority,
                preservation: ThreadPriorityPreservation::Exact,
            },
            allow_cross_session_process_control,
        )
    }

    fn apply_process_claim(
        &mut self,
        claim: ThreadPriorityClaim,
        allow_cross_session_process_control: bool,
    ) -> Result<ThreadPriorityApplyOutcome, ProcessControlError> {
        let desired = thread_priority_value(claim.priority).ok_or_else(|| {
            ProcessControlError::Failed(
                "This thread priority is not available as a process action.".to_owned(),
            )
        })?;
        let (process_identity, process) = match self
            .platform
            .open_process(&claim.target, allow_cross_session_process_control)
        {
            Ok(opened) => opened,
            Err(ProcessControlError::ProcessExited) => {
                self.relinquish_target(&claim.target.key())?;
                return Err(ProcessControlError::ProcessExited);
            }
            Err(error) => return Err(error),
        };
        self.relinquish_reused_process_identities(&process_identity)?;
        let thread_ids = self.platform.thread_ids(&process, &process_identity)?;
        if thread_ids.is_empty() {
            return Err(ProcessControlError::ProcessExited);
        }

        let seen_thread_ids = thread_ids.iter().copied().collect::<BTreeSet<_>>();
        let mut outcome = ThreadPriorityApplyOutcome::default();
        let mut first_error = None;
        for thread_id in thread_ids {
            let result = self
                .platform
                .open_thread(&process, &process_identity, thread_id)
                .and_then(|(identity, thread)| {
                    self.relinquish_reused_thread_identity(&identity)?;
                    self.apply_thread_claim(
                        &process,
                        &thread,
                        identity,
                        claim.owner,
                        desired,
                        claim.preservation,
                    )
                });
            match result {
                Ok(ThreadApply::Applied) => outcome.applied_threads += 1,
                Ok(ThreadApply::Unchanged) => outcome.unchanged_threads += 1,
                Ok(ThreadApply::Preserved) => outcome.preserved_threads += 1,
                Err(ProcessControlError::ProcessExited) => {}
                Err(error) => {
                    first_error.get_or_insert(error);
                }
            }
        }
        if let Err(error) = self.relinquish_missing_threads(&process_identity, &seen_thread_ids) {
            first_error.get_or_insert(error);
        }
        if let Some(error) = first_error {
            Err(error)
        } else if outcome.applied_threads + outcome.unchanged_threads + outcome.preserved_threads
            == 0
        {
            Err(ProcessControlError::ProcessExited)
        } else {
            Ok(outcome)
        }
    }

    fn apply_thread_claim(
        &mut self,
        process: &P::Process,
        thread: &P::Thread,
        identity: ThreadIdentity,
        owner: ControlOwner,
        desired: i32,
        preservation: ThreadPriorityPreservation,
    ) -> Result<ThreadApply, ProcessControlError> {
        let mut managed = self.managed.remove(&identity);
        let current = match self.platform.query(thread) {
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

        let baseline = managed.as_ref().map_or(current, |managed| managed.baseline);
        if priority_is_preserved(preservation, baseline, desired) {
            if let Some(managed) = managed {
                self.managed.insert(identity.clone(), managed);
                self.release_identity(&identity)?;
                self.managed.remove(&identity);
            }
            return Ok(ThreadApply::Preserved);
        }
        if current == desired {
            if let Some(mut managed) = managed {
                if owner.is_automatic() || managed.owner == ControlOwner::ProcessList {
                    managed.owner = owner;
                }
                managed.expected = current;
                self.managed.insert(identity, managed);
            }
            return Ok(ThreadApply::Unchanged);
        }

        let sequence = self.next_apply_sequence;
        self.next_apply_sequence = self.next_apply_sequence.wrapping_add(1).max(1);
        match apply_transition(
            &mut self.platform,
            process,
            thread,
            current,
            desired,
            CommitFailureBehavior::Compensate,
        ) {
            Ok(()) => {
                self.managed.insert(
                    identity,
                    ManagedThreadPriority {
                        baseline,
                        expected: desired,
                        owner,
                        apply_sequence: sequence,
                    },
                );
                Ok(ThreadApply::Applied)
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
                        ManagedThreadPriority {
                            baseline,
                            expected: desired,
                            owner,
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

    fn relinquish_reused_process_identities(
        &mut self,
        identity: &ProcessIdentity,
    ) -> Result<(), ProcessControlError> {
        let stale = self
            .managed
            .keys()
            .filter(|thread| thread.process.id == identity.id && thread.process != *identity)
            .cloned()
            .collect::<Vec<_>>();
        self.relinquish_identities(stale)
    }

    fn relinquish_target(&mut self, target: &ProcessTargetKey) -> Result<(), ProcessControlError> {
        let stale = self
            .managed
            .keys()
            .filter(|thread| thread.process.key() == *target)
            .cloned()
            .collect::<Vec<_>>();
        self.relinquish_identities(stale)
    }

    fn relinquish_reused_thread_identity(
        &mut self,
        identity: &ThreadIdentity,
    ) -> Result<(), ProcessControlError> {
        let stale = self
            .managed
            .keys()
            .filter(|thread| {
                thread.process == identity.process
                    && thread.id == identity.id
                    && thread.creation_time != identity.creation_time
            })
            .cloned()
            .collect::<Vec<_>>();
        self.relinquish_identities(stale)
    }

    fn relinquish_missing_threads(
        &mut self,
        process: &ProcessIdentity,
        seen_thread_ids: &BTreeSet<u32>,
    ) -> Result<(), ProcessControlError> {
        let stale = self
            .managed
            .keys()
            .filter(|thread| thread.process == *process && !seen_thread_ids.contains(&thread.id))
            .cloned()
            .collect::<Vec<_>>();
        self.relinquish_identities(stale)
    }

    fn relinquish_identities(
        &mut self,
        identities: Vec<ThreadIdentity>,
    ) -> Result<(), ProcessControlError> {
        for identity in identities {
            self.platform.relinquish(&identity)?;
            self.managed.remove(&identity);
        }
        Ok(())
    }

    pub(crate) fn release_policy_except(
        &mut self,
        active_targets: &BTreeSet<ProcessTargetKey>,
    ) -> ThreadPriorityReleaseSummary {
        let identities = self
            .managed
            .iter()
            .filter(|(identity, managed)| {
                managed.owner.is_automatic() && !active_targets.contains(&identity.process.key())
            })
            .map(|(identity, managed)| (managed.apply_sequence, identity.clone()))
            .collect::<Vec<_>>();
        self.release_identities(identities)
    }

    pub(crate) fn release_all_policy(&mut self) -> ThreadPriorityReleaseSummary {
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
                        "{} (process {}, thread {}): {}",
                        failure.process_name, failure.process_id, failure.thread_id, failure.error
                    )
                })
                .collect::<Vec<_>>()
                .join("; "))
        }
    }

    fn release_identities(
        &mut self,
        mut identities: Vec<(u64, ThreadIdentity)>,
    ) -> ThreadPriorityReleaseSummary {
        identities.sort_by_key(|identity| std::cmp::Reverse(identity.0));
        let mut summary = ThreadPriorityReleaseSummary::default();
        for (_, identity) in identities {
            match self.release_identity(&identity) {
                Ok(restored) => {
                    summary.restored_threads += usize::from(restored);
                    self.managed.remove(&identity);
                }
                Err(ProcessControlError::ProcessExited) => {
                    match self.platform.relinquish(&identity) {
                        Ok(()) => {
                            self.managed.remove(&identity);
                        }
                        Err(error) => summary.failures.push(release_failure(&identity, error)),
                    }
                }
                Err(error) => summary.failures.push(release_failure(&identity, error)),
            }
        }
        summary
    }

    fn release_identity(&mut self, identity: &ThreadIdentity) -> Result<bool, ProcessControlError> {
        let Some(managed) = self.managed.get(identity) else {
            return Ok(false);
        };
        let target = identity.process.target();
        let (process_identity, process) = self.platform.open_process(&target, true)?;
        if process_identity != identity.process {
            return Err(ProcessControlError::ProcessExited);
        }
        let (current_identity, thread) =
            self.platform
                .open_thread(&process, &process_identity, identity.id)?;
        if current_identity != *identity {
            return Err(ProcessControlError::ProcessExited);
        }
        let current = self.platform.query(&thread)?;
        if current != managed.expected || current == managed.baseline {
            self.platform.relinquish(identity)?;
            return Ok(false);
        }
        let baseline = managed.baseline;
        match apply_transition(
            &mut self.platform,
            &process,
            &thread,
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
            .map(|(identity, _)| identity.process.name.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect()
    }

    pub(crate) fn policy_managed_process_count(&self) -> usize {
        self.managed
            .iter()
            .filter(|(_, managed)| managed.owner.is_automatic())
            .map(|(identity, _)| identity.process.key())
            .collect::<BTreeSet<_>>()
            .len()
    }

    pub(crate) fn policy_managed_thread_count(&self) -> usize {
        self.managed
            .values()
            .filter(|managed| managed.owner.is_automatic())
            .count()
    }

    pub(crate) fn has_managed_state(&self) -> bool {
        !self.managed.is_empty()
    }
}

impl<P: ThreadPriorityPlatform> Drop for ThreadPriorityController<P> {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

#[derive(Clone, Copy)]
enum ThreadApply {
    Applied,
    Unchanged,
    Preserved,
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

fn apply_transition<P: ThreadPriorityPlatform>(
    platform: &mut P,
    process: &P::Process,
    thread: &P::Thread,
    original: i32,
    expected: i32,
    commit_failure_behavior: CommitFailureBehavior,
) -> Result<(), TransitionFailure> {
    let intent = platform
        .begin_change(process, thread, original, expected)
        .map_err(|error| TransitionFailure {
            message: error.to_string(),
            uncertain: false,
            relinquish_recovery: false,
            expected_preserved: false,
        })?;
    if let Err(error) = platform.apply(thread, expected) {
        return Err(compensate_with_intent(
            platform,
            thread,
            original,
            intent,
            error.to_string(),
        ));
    }
    match platform.query(thread) {
        Ok(actual) if actual == expected => {}
        Ok(actual) => {
            return Err(compensate_with_intent(
                platform,
                thread,
                original,
                intent,
                format!(
                    "Thread Priority verification returned {}, expected {}.",
                    thread_priority_label(actual),
                    thread_priority_label(expected)
                ),
            ));
        }
        Err(error) => {
            return Err(compensate_with_intent(
                platform,
                thread,
                original,
                intent,
                format!("Thread Priority verification failed: {error}"),
            ));
        }
    }
    if let Err(error) = intent.commit() {
        let message = format!("Crash recovery commit failed: {error}");
        return match commit_failure_behavior {
            CommitFailureBehavior::Compensate => Err(compensate_without_intent(
                platform, thread, original, message,
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

fn compensate_with_intent<P: ThreadPriorityPlatform>(
    platform: &mut P,
    thread: &P::Thread,
    original: i32,
    intent: P::RecoveryIntent,
    primary_error: String,
) -> TransitionFailure {
    match restore_and_verify(platform, thread, original) {
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

fn compensate_without_intent<P: ThreadPriorityPlatform>(
    platform: &mut P,
    thread: &P::Thread,
    original: i32,
    primary_error: String,
) -> TransitionFailure {
    match restore_and_verify(platform, thread, original) {
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

fn restore_and_verify<P: ThreadPriorityPlatform>(
    platform: &mut P,
    thread: &P::Thread,
    priority: i32,
) -> Result<(), ProcessControlError> {
    platform.apply(thread, priority)?;
    let actual = platform.query(thread)?;
    if actual == priority {
        Ok(())
    } else {
        Err(ProcessControlError::Failed(format!(
            "Thread Priority compensation returned {}, expected {}.",
            thread_priority_label(actual),
            thread_priority_label(priority)
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
        message.push_str(&format!(" Crash recovery commit failed: {recovery_error}."));
    }
    message
}

fn release_failure(
    identity: &ThreadIdentity,
    error: ProcessControlError,
) -> ThreadPriorityReleaseFailure {
    ThreadPriorityReleaseFailure {
        process_id: identity.process.id,
        process_name: identity.process.name.clone(),
        thread_id: identity.id,
        error,
    }
}

fn priority_is_preserved(
    preservation: ThreadPriorityPreservation,
    baseline: i32,
    desired: i32,
) -> bool {
    match preservation {
        ThreadPriorityPreservation::Exact => false,
        ThreadPriorityPreservation::PreserveHigher => baseline >= desired,
        ThreadPriorityPreservation::PreserveLower => baseline <= desired,
    }
}

pub(crate) fn current_process_thread_priority(
    target: &ProcessActionTarget,
    allow_cross_session_process_control: bool,
) -> Result<Option<ProcessThreadPrioritySetting>, String> {
    let mut platform = WindowsThreadPriorityPlatform;
    let target = ProcessControlTarget::from_action_target(target);
    let (identity, process) = platform
        .open_process(&target, allow_cross_session_process_control)
        .map_err(|error| error.to_string())?;
    let thread_ids = platform
        .thread_ids(&process, &identity)
        .map_err(|error| error.to_string())?;
    if thread_ids.is_empty() {
        return Err(ProcessControlError::ProcessExited.to_string());
    }
    let mut current = None;
    for thread_id in thread_ids {
        let (_, thread) = platform
            .open_thread(&process, &identity, thread_id)
            .map_err(|error| error.to_string())?;
        let priority = platform.query(&thread).map_err(|error| error.to_string())?;
        if current.is_some_and(|existing| existing != priority) {
            return Ok(None);
        }
        current = Some(priority);
    }
    Ok(current.map(thread_priority_setting_from_value))
}

pub(crate) fn thread_priority_is_actionable(priority: ProcessThreadPrioritySetting) -> bool {
    thread_priority_value(priority).is_some()
}

fn thread_priority_value(priority: ProcessThreadPrioritySetting) -> Option<i32> {
    match priority {
        ProcessThreadPrioritySetting::Default => None,
        ProcessThreadPrioritySetting::TimeCritical => {
            Some(windows_thread_priority::PRIORITY_TIME_CRITICAL)
        }
        ProcessThreadPrioritySetting::Highest => Some(windows_thread_priority::PRIORITY_HIGHEST),
        ProcessThreadPrioritySetting::AboveNormal => {
            Some(windows_thread_priority::PRIORITY_ABOVE_NORMAL)
        }
        ProcessThreadPrioritySetting::Normal => Some(windows_thread_priority::PRIORITY_NORMAL),
        ProcessThreadPrioritySetting::BelowNormal => {
            Some(windows_thread_priority::PRIORITY_BELOW_NORMAL)
        }
        ProcessThreadPrioritySetting::Lowest => Some(windows_thread_priority::PRIORITY_LOWEST),
        ProcessThreadPrioritySetting::Idle => Some(windows_thread_priority::PRIORITY_IDLE),
    }
}

fn thread_priority_setting_from_value(priority: i32) -> ProcessThreadPrioritySetting {
    match priority {
        windows_thread_priority::PRIORITY_TIME_CRITICAL => {
            ProcessThreadPrioritySetting::TimeCritical
        }
        windows_thread_priority::PRIORITY_HIGHEST => ProcessThreadPrioritySetting::Highest,
        windows_thread_priority::PRIORITY_ABOVE_NORMAL => ProcessThreadPrioritySetting::AboveNormal,
        windows_thread_priority::PRIORITY_BELOW_NORMAL => ProcessThreadPrioritySetting::BelowNormal,
        windows_thread_priority::PRIORITY_LOWEST => ProcessThreadPrioritySetting::Lowest,
        windows_thread_priority::PRIORITY_IDLE => ProcessThreadPrioritySetting::Idle,
        _ => ProcessThreadPrioritySetting::Normal,
    }
}

fn thread_priority_label(priority: i32) -> &'static str {
    match priority {
        windows_thread_priority::PRIORITY_TIME_CRITICAL => "Time Critical",
        windows_thread_priority::PRIORITY_HIGHEST => "Highest",
        windows_thread_priority::PRIORITY_ABOVE_NORMAL => "Above Normal",
        windows_thread_priority::PRIORITY_NORMAL => "Normal",
        windows_thread_priority::PRIORITY_BELOW_NORMAL => "Below Normal",
        windows_thread_priority::PRIORITY_LOWEST => "Lowest",
        windows_thread_priority::PRIORITY_IDLE => "Idle",
        _ => "Unknown",
    }
}

fn map_thread_priority_error(error: ThreadPriorityError) -> ProcessControlError {
    match error {
        ThreadPriorityError::AccessDenied => {
            ProcessControlError::AccessDenied("Access denied.".to_owned())
        }
        ThreadPriorityError::ThreadExited => ProcessControlError::ProcessExited,
        ThreadPriorityError::Failed {
            operation,
            thread_id: Some(thread_id),
            code,
        } => ProcessControlError::Failed(format!(
            "{operation}({thread_id}) failed with error {code}."
        )),
        ThreadPriorityError::Failed {
            operation,
            thread_id: None,
            code,
        } => ProcessControlError::Failed(format!("{operation} failed with error {code}.")),
    }
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

    #[derive(Clone)]
    struct FakeThread {
        creation_time: u64,
        priority: i32,
    }

    struct FakeProcess {
        identity: ProcessIdentity,
        threads: BTreeMap<u32, FakeThread>,
    }

    #[derive(Clone, Copy, PartialEq, Eq)]
    enum FakeFailure {
        OpenExited,
        Begin,
        Apply,
        Verify,
        Commit,
        Relinquish,
    }

    struct FakeIntent {
        fail: bool,
        events: Arc<Mutex<Vec<String>>>,
    }

    impl ThreadPriorityRecoveryIntent for FakeIntent {
        fn commit(self) -> Result<(), String> {
            self.events.lock().unwrap().push("commit".to_owned());
            if self.fail {
                Err("commit failed".to_owned())
            } else {
                Ok(())
            }
        }
    }

    struct FakePlatform {
        process: FakeProcess,
        failures: VecDeque<FakeFailure>,
        events: Arc<Mutex<Vec<String>>>,
    }

    impl FakePlatform {
        fn new(priorities: &[i32]) -> Self {
            let threads = priorities
                .iter()
                .enumerate()
                .map(|(index, priority)| {
                    (
                        100 + index as u32,
                        FakeThread {
                            creation_time: 1000 + index as u64,
                            priority: *priority,
                        },
                    )
                })
                .collect();
            Self {
                process: FakeProcess {
                    identity: process_identity(7),
                    threads,
                },
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

    impl ThreadPriorityPlatform for FakePlatform {
        type Process = u32;
        type Thread = u32;
        type RecoveryIntent = FakeIntent;

        fn open_process(
            &mut self,
            target: &ProcessControlTarget,
            _: bool,
        ) -> Result<(ProcessIdentity, Self::Process), ProcessControlError> {
            if target.id != self.process.identity.id
                || target.creation_time != self.process.identity.creation_time
            {
                return Err(ProcessControlError::ProcessExited);
            }
            Ok((self.process.identity.clone(), target.id))
        }

        fn thread_ids(
            &mut self,
            _: &Self::Process,
            _: &ProcessIdentity,
        ) -> Result<Vec<u32>, ProcessControlError> {
            Ok(self.process.threads.keys().copied().collect())
        }

        fn open_thread(
            &mut self,
            _: &Self::Process,
            identity: &ProcessIdentity,
            thread_id: u32,
        ) -> Result<(ThreadIdentity, Self::Thread), ProcessControlError> {
            if self.take_failure(FakeFailure::OpenExited) {
                return Err(ProcessControlError::ProcessExited);
            }
            let thread = self
                .process
                .threads
                .get(&thread_id)
                .ok_or(ProcessControlError::ProcessExited)?;
            Ok((
                ThreadIdentity {
                    process: identity.clone(),
                    id: thread_id,
                    creation_time: thread.creation_time,
                },
                thread_id,
            ))
        }

        fn query(&mut self, thread: &Self::Thread) -> Result<i32, ProcessControlError> {
            let priority = self
                .process
                .threads
                .get(thread)
                .map(|thread| thread.priority)
                .ok_or(ProcessControlError::ProcessExited)?;
            if priority != THREAD_PRIORITY_NORMAL && self.take_failure(FakeFailure::Verify) {
                Err(ProcessControlError::Failed("query failed".to_owned()))
            } else {
                Ok(priority)
            }
        }

        fn begin_change(
            &mut self,
            _: &Self::Process,
            _: &Self::Thread,
            _: i32,
            _: i32,
        ) -> Result<Self::RecoveryIntent, ProcessControlError> {
            self.events.lock().unwrap().push("begin".to_owned());
            if self.take_failure(FakeFailure::Begin) {
                return Err(ProcessControlError::Failed("begin failed".to_owned()));
            }
            let fail = self.take_failure(FakeFailure::Commit);
            Ok(FakeIntent {
                fail,
                events: Arc::clone(&self.events),
            })
        }

        fn apply(
            &mut self,
            thread: &Self::Thread,
            priority: i32,
        ) -> Result<(), ProcessControlError> {
            self.events
                .lock()
                .unwrap()
                .push(format!("apply:{thread}:{priority}"));
            if self.take_failure(FakeFailure::Apply) {
                return Err(ProcessControlError::Failed("apply failed".to_owned()));
            }
            self.process
                .threads
                .get_mut(thread)
                .ok_or(ProcessControlError::ProcessExited)?
                .priority = priority;
            Ok(())
        }

        fn relinquish(&mut self, identity: &ThreadIdentity) -> Result<(), ProcessControlError> {
            self.events.lock().unwrap().push(format!(
                "relinquish:{}:{}:{}:{}",
                identity.process.id,
                identity.process.creation_time,
                identity.id,
                identity.creation_time
            ));
            if self.take_failure(FakeFailure::Relinquish) {
                Err(ProcessControlError::Failed("relinquish failed".to_owned()))
            } else {
                Ok(())
            }
        }
    }

    fn process_identity(creation_time: u64) -> ProcessIdentity {
        ProcessIdentity::new(
            42,
            "app.exe".to_owned(),
            PathBuf::from(r"C:\Apps\app.exe"),
            creation_time,
            Some(1),
        )
    }

    fn target(creation_time: u64) -> ProcessControlTarget {
        ProcessControlTarget::automatic(
            42,
            "app.exe".to_owned(),
            PathBuf::from(r"C:\Apps\app.exe"),
            creation_time,
        )
    }

    fn claim(
        owner: ControlOwner,
        priority: ProcessThreadPrioritySetting,
        preservation: ThreadPriorityPreservation,
    ) -> ThreadPriorityClaim {
        ThreadPriorityClaim {
            target: target(7),
            owner,
            priority,
            preservation,
        }
    }

    fn action_target() -> ProcessActionTarget {
        ProcessActionTarget {
            id: 42,
            name: "app.exe".to_owned(),
            executable_path: PathBuf::from(r"C:\Apps\app.exe"),
            creation_time: 7,
            session_id: Some(1),
            is_service_account: Some(false),
        }
    }

    #[test]
    fn priority_mapping_uses_thread_offsets() {
        assert_eq!(
            thread_priority_value(ProcessThreadPrioritySetting::TimeCritical),
            Some(THREAD_PRIORITY_TIME_CRITICAL)
        );
        assert_eq!(
            thread_priority_value(ProcessThreadPrioritySetting::Highest),
            Some(THREAD_PRIORITY_HIGHEST)
        );
        assert_eq!(
            thread_priority_value(ProcessThreadPrioritySetting::Default),
            None
        );
    }

    #[test]
    fn unmanaged_matching_threads_are_not_adopted() {
        let platform = FakePlatform::new(&[THREAD_PRIORITY_NORMAL, THREAD_PRIORITY_NORMAL]);
        let mut controller = ThreadPriorityController::with_platform(platform);

        let result = controller
            .apply_policy_claim(
                claim(
                    ControlOwner::ThreadPriority,
                    ProcessThreadPrioritySetting::Normal,
                    ThreadPriorityPreservation::Exact,
                ),
                true,
            )
            .unwrap();

        assert_eq!(result.unchanged_threads, 2);
        assert!(!controller.has_managed_state());
    }

    #[test]
    fn mixed_process_list_baselines_are_captured_and_restored_individually() {
        let platform = FakePlatform::new(&[THREAD_PRIORITY_NORMAL, THREAD_PRIORITY_LOWEST]);
        let mut controller = ThreadPriorityController::with_platform(platform);

        let result = controller
            .apply_process_list_action(
                &action_target(),
                ProcessThreadPrioritySetting::AboveNormal,
                true,
            )
            .unwrap();
        assert_eq!(result.applied_threads, 2);
        controller.shutdown().unwrap();

        assert_eq!(
            controller.platform.process.threads[&100].priority,
            THREAD_PRIORITY_NORMAL
        );
        assert_eq!(
            controller.platform.process.threads[&101].priority,
            THREAD_PRIORITY_LOWEST
        );
    }

    #[test]
    fn process_list_action_is_superseded_by_automatic_policy() {
        let platform = FakePlatform::new(&[THREAD_PRIORITY_NORMAL]);
        let mut controller = ThreadPriorityController::with_platform(platform);
        controller
            .apply_process_list_action(
                &action_target(),
                ProcessThreadPrioritySetting::AboveNormal,
                true,
            )
            .unwrap();
        controller
            .apply_policy_claim(
                claim(
                    ControlOwner::ThreadPriority,
                    ProcessThreadPrioritySetting::BelowNormal,
                    ThreadPriorityPreservation::Exact,
                ),
                true,
            )
            .unwrap();

        let managed = controller.managed.values().next().unwrap();
        assert_eq!(managed.owner, ControlOwner::ThreadPriority);
        assert_eq!(managed.baseline, THREAD_PRIORITY_NORMAL);
        assert_eq!(managed.expected, THREAD_PRIORITY_BELOW_NORMAL);
    }

    #[test]
    fn static_to_adaptive_replacement_keeps_the_first_baseline() {
        let platform = FakePlatform::new(&[THREAD_PRIORITY_NORMAL]);
        let mut controller = ThreadPriorityController::with_platform(platform);
        controller
            .apply_policy_claim(
                claim(
                    ControlOwner::ThreadPriority,
                    ProcessThreadPrioritySetting::BelowNormal,
                    ThreadPriorityPreservation::Exact,
                ),
                true,
            )
            .unwrap();
        controller
            .apply_policy_claim(
                claim(
                    ControlOwner::AdaptiveEngine,
                    ProcessThreadPrioritySetting::AboveNormal,
                    ThreadPriorityPreservation::Exact,
                ),
                true,
            )
            .unwrap();

        let managed = controller.managed.values().next().unwrap();
        assert_eq!(managed.owner, ControlOwner::AdaptiveEngine);
        assert_eq!(managed.baseline, THREAD_PRIORITY_NORMAL);
    }

    #[test]
    fn preservation_uses_the_original_baseline_and_releases_managed_state() {
        let platform = FakePlatform::new(&[THREAD_PRIORITY_NORMAL]);
        let mut controller = ThreadPriorityController::with_platform(platform);
        controller
            .apply_policy_claim(
                claim(
                    ControlOwner::ThreadPriority,
                    ProcessThreadPrioritySetting::BelowNormal,
                    ThreadPriorityPreservation::Exact,
                ),
                true,
            )
            .unwrap();
        let result = controller
            .apply_policy_claim(
                claim(
                    ControlOwner::AdaptiveEngine,
                    ProcessThreadPrioritySetting::BelowNormal,
                    ThreadPriorityPreservation::PreserveHigher,
                ),
                true,
            )
            .unwrap();

        assert_eq!(result.preserved_threads, 1);
        assert!(!controller.has_managed_state());
        assert_eq!(
            controller.platform.process.threads[&100].priority,
            THREAD_PRIORITY_NORMAL
        );
    }

    #[test]
    fn new_threads_are_discovered_and_exited_threads_are_relinquished() {
        let platform = FakePlatform::new(&[THREAD_PRIORITY_NORMAL]);
        let mut controller = ThreadPriorityController::with_platform(platform);
        let policy = claim(
            ControlOwner::ThreadPriority,
            ProcessThreadPrioritySetting::BelowNormal,
            ThreadPriorityPreservation::Exact,
        );
        controller.apply_policy_claim(policy.clone(), true).unwrap();
        controller.platform.process.threads.insert(
            101,
            FakeThread {
                creation_time: 1001,
                priority: THREAD_PRIORITY_NORMAL,
            },
        );
        controller.apply_policy_claim(policy.clone(), true).unwrap();
        assert_eq!(controller.managed.len(), 2);
        controller.platform.process.threads.remove(&100);
        controller.apply_policy_claim(policy, true).unwrap();

        assert_eq!(controller.managed.len(), 1);
        assert!(controller
            .platform
            .events
            .lock()
            .unwrap()
            .iter()
            .any(|event| event == "relinquish:42:7:100:1000"));
    }

    #[test]
    fn a_thread_that_exits_during_enumeration_does_not_block_its_siblings() {
        let mut platform = FakePlatform::new(&[THREAD_PRIORITY_NORMAL, THREAD_PRIORITY_NORMAL]);
        platform.fail_next(FakeFailure::OpenExited);
        let mut controller = ThreadPriorityController::with_platform(platform);

        let outcome = controller
            .apply_policy_claim(
                claim(
                    ControlOwner::ThreadPriority,
                    ProcessThreadPrioritySetting::BelowNormal,
                    ThreadPriorityPreservation::Exact,
                ),
                true,
            )
            .unwrap();

        assert_eq!(outcome.applied_threads, 1);
        assert_eq!(controller.managed.len(), 1);
    }

    #[test]
    fn thread_id_reuse_relinquishes_the_old_exact_identity() {
        let platform = FakePlatform::new(&[THREAD_PRIORITY_NORMAL]);
        let mut controller = ThreadPriorityController::with_platform(platform);
        let policy = claim(
            ControlOwner::ThreadPriority,
            ProcessThreadPrioritySetting::BelowNormal,
            ThreadPriorityPreservation::Exact,
        );
        controller.apply_policy_claim(policy.clone(), true).unwrap();
        let thread = controller.platform.process.threads.get_mut(&100).unwrap();
        thread.creation_time = 2000;
        thread.priority = THREAD_PRIORITY_NORMAL;
        controller.apply_policy_claim(policy, true).unwrap();

        assert_eq!(controller.managed.len(), 1);
        assert_eq!(
            controller.managed.keys().next().unwrap().creation_time,
            2000
        );
    }

    #[test]
    fn process_id_reuse_relinquishes_every_old_thread_identity() {
        let platform = FakePlatform::new(&[THREAD_PRIORITY_NORMAL]);
        let mut controller = ThreadPriorityController::with_platform(platform);
        controller
            .apply_policy_claim(
                claim(
                    ControlOwner::ThreadPriority,
                    ProcessThreadPrioritySetting::BelowNormal,
                    ThreadPriorityPreservation::Exact,
                ),
                true,
            )
            .unwrap();
        controller.platform.process.identity = process_identity(8);
        let thread = controller.platform.process.threads.get_mut(&100).unwrap();
        thread.creation_time = 2000;
        thread.priority = THREAD_PRIORITY_NORMAL;

        controller
            .apply_policy_claim(
                ThreadPriorityClaim {
                    target: target(8),
                    owner: ControlOwner::ThreadPriority,
                    priority: ProcessThreadPrioritySetting::BelowNormal,
                    preservation: ThreadPriorityPreservation::Exact,
                },
                true,
            )
            .unwrap();

        assert_eq!(controller.managed.len(), 1);
        assert_eq!(
            controller
                .managed
                .keys()
                .next()
                .unwrap()
                .process
                .creation_time,
            8
        );
        assert!(controller
            .platform
            .events
            .lock()
            .unwrap()
            .iter()
            .any(|event| event == "relinquish:42:7:100:1000"));
    }

    #[test]
    fn stale_process_observation_relinquishes_the_exited_exact_instance() {
        let platform = FakePlatform::new(&[THREAD_PRIORITY_NORMAL]);
        let mut controller = ThreadPriorityController::with_platform(platform);
        let policy = claim(
            ControlOwner::ThreadPriority,
            ProcessThreadPrioritySetting::BelowNormal,
            ThreadPriorityPreservation::Exact,
        );
        controller.apply_policy_claim(policy.clone(), true).unwrap();
        controller.platform.process.identity = process_identity(8);

        assert_eq!(
            controller.apply_policy_claim(policy, true),
            Err(ProcessControlError::ProcessExited)
        );
        assert!(!controller.has_managed_state());
        assert!(controller
            .platform
            .events
            .lock()
            .unwrap()
            .iter()
            .any(|event| event == "relinquish:42:7:100:1000"));
    }

    #[test]
    fn external_break_relinquishes_and_rebases_before_reassertion() {
        let platform = FakePlatform::new(&[THREAD_PRIORITY_NORMAL]);
        let mut controller = ThreadPriorityController::with_platform(platform);
        let policy = claim(
            ControlOwner::ThreadPriority,
            ProcessThreadPrioritySetting::BelowNormal,
            ThreadPriorityPreservation::Exact,
        );
        controller.apply_policy_claim(policy.clone(), true).unwrap();
        controller
            .platform
            .process
            .threads
            .get_mut(&100)
            .unwrap()
            .priority = THREAD_PRIORITY_ABOVE_NORMAL;
        controller.apply_policy_claim(policy, true).unwrap();

        assert_eq!(
            controller.managed.values().next().unwrap().baseline,
            THREAD_PRIORITY_ABOVE_NORMAL
        );
    }

    #[test]
    fn failed_relinquish_keeps_the_existing_chain() {
        let platform = FakePlatform::new(&[THREAD_PRIORITY_NORMAL]);
        let mut controller = ThreadPriorityController::with_platform(platform);
        let policy = claim(
            ControlOwner::ThreadPriority,
            ProcessThreadPrioritySetting::BelowNormal,
            ThreadPriorityPreservation::Exact,
        );
        controller.apply_policy_claim(policy.clone(), true).unwrap();
        controller
            .platform
            .process
            .threads
            .get_mut(&100)
            .unwrap()
            .priority = THREAD_PRIORITY_ABOVE_NORMAL;
        controller.platform.fail_next(FakeFailure::Relinquish);

        assert!(controller.apply_policy_claim(policy, true).is_err());
        assert!(controller.has_managed_state());
    }

    #[test]
    fn external_break_is_not_overwritten_during_release() {
        let platform = FakePlatform::new(&[THREAD_PRIORITY_NORMAL]);
        let mut controller = ThreadPriorityController::with_platform(platform);
        controller
            .apply_policy_claim(
                claim(
                    ControlOwner::ThreadPriority,
                    ProcessThreadPrioritySetting::BelowNormal,
                    ThreadPriorityPreservation::Exact,
                ),
                true,
            )
            .unwrap();
        controller
            .platform
            .process
            .threads
            .get_mut(&100)
            .unwrap()
            .priority = THREAD_PRIORITY_ABOVE_NORMAL;

        let summary = controller.release_all_policy();

        assert!(summary.failures.is_empty());
        assert!(!controller.has_managed_state());
        assert_eq!(
            controller.platform.process.threads[&100].priority,
            THREAD_PRIORITY_ABOVE_NORMAL
        );
    }

    #[test]
    fn begin_and_commit_failures_do_not_leave_untracked_mutations() {
        for failure in [FakeFailure::Begin, FakeFailure::Commit] {
            let mut platform = FakePlatform::new(&[THREAD_PRIORITY_NORMAL]);
            platform.fail_next(failure);
            let mut controller = ThreadPriorityController::with_platform(platform);
            assert!(controller
                .apply_policy_claim(
                    claim(
                        ControlOwner::ThreadPriority,
                        ProcessThreadPrioritySetting::BelowNormal,
                        ThreadPriorityPreservation::Exact,
                    ),
                    true,
                )
                .is_err());
            assert_eq!(
                controller.platform.process.threads[&100].priority,
                THREAD_PRIORITY_NORMAL
            );
            assert!(!controller.has_managed_state());
        }
    }

    #[test]
    fn apply_failure_compensates_and_cancels_the_intent() {
        let mut platform = FakePlatform::new(&[THREAD_PRIORITY_NORMAL]);
        platform.fail_next(FakeFailure::Apply);
        let mut controller = ThreadPriorityController::with_platform(platform);

        assert!(controller
            .apply_policy_claim(
                claim(
                    ControlOwner::ThreadPriority,
                    ProcessThreadPrioritySetting::BelowNormal,
                    ThreadPriorityPreservation::Exact,
                ),
                true,
            )
            .is_err());
        assert_eq!(
            controller.platform.process.threads[&100].priority,
            THREAD_PRIORITY_NORMAL
        );
        assert!(!controller.has_managed_state());
    }

    #[test]
    fn failed_compensation_keeps_uncertain_state_for_later_release() {
        let mut platform = FakePlatform::new(&[THREAD_PRIORITY_NORMAL]);
        platform.fail_next(FakeFailure::Apply);
        platform.fail_next(FakeFailure::Apply);
        let mut controller = ThreadPriorityController::with_platform(platform);

        assert!(controller
            .apply_policy_claim(
                claim(
                    ControlOwner::ThreadPriority,
                    ProcessThreadPrioritySetting::BelowNormal,
                    ThreadPriorityPreservation::Exact,
                ),
                true,
            )
            .is_err());
        assert!(controller.has_managed_state());
        assert!(controller.release_all_policy().failures.is_empty());
        assert!(!controller.has_managed_state());
    }

    #[test]
    fn verification_failure_compensates() {
        let mut platform = FakePlatform::new(&[THREAD_PRIORITY_NORMAL]);
        platform.fail_next(FakeFailure::Verify);
        let mut controller = ThreadPriorityController::with_platform(platform);
        assert!(controller
            .apply_policy_claim(
                claim(
                    ControlOwner::ThreadPriority,
                    ProcessThreadPrioritySetting::BelowNormal,
                    ThreadPriorityPreservation::Exact,
                ),
                true,
            )
            .is_err());
        assert_eq!(
            controller.platform.process.threads[&100].priority,
            THREAD_PRIORITY_NORMAL
        );
    }

    #[test]
    fn release_commit_failure_keeps_baseline_and_relinquishes_recovery() {
        let platform = FakePlatform::new(&[THREAD_PRIORITY_NORMAL]);
        let mut controller = ThreadPriorityController::with_platform(platform);
        controller
            .apply_policy_claim(
                claim(
                    ControlOwner::ThreadPriority,
                    ProcessThreadPrioritySetting::BelowNormal,
                    ThreadPriorityPreservation::Exact,
                ),
                true,
            )
            .unwrap();
        controller.platform.fail_next(FakeFailure::Commit);

        let summary = controller.release_all_policy();

        assert!(summary.failures.is_empty());
        assert_eq!(summary.restored_threads, 1);
        assert!(!controller.has_managed_state());
        assert_eq!(
            controller.platform.process.threads[&100].priority,
            THREAD_PRIORITY_NORMAL
        );
    }

    #[test]
    fn failed_release_relinquish_retries_without_reapplying() {
        let platform = FakePlatform::new(&[THREAD_PRIORITY_NORMAL]);
        let mut controller = ThreadPriorityController::with_platform(platform);
        controller
            .apply_policy_claim(
                claim(
                    ControlOwner::ThreadPriority,
                    ProcessThreadPrioritySetting::BelowNormal,
                    ThreadPriorityPreservation::Exact,
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
            controller.platform.process.threads[&100].priority,
            THREAD_PRIORITY_NORMAL
        );
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
        assert!(!controller.has_managed_state());
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
        let platform = FakePlatform::new(&[THREAD_PRIORITY_NORMAL, THREAD_PRIORITY_NORMAL]);
        let events = Arc::clone(&platform.events);
        let mut controller = ThreadPriorityController::with_platform(platform);
        controller
            .apply_policy_claim(
                claim(
                    ControlOwner::ThreadPriority,
                    ProcessThreadPrioritySetting::BelowNormal,
                    ThreadPriorityPreservation::Exact,
                ),
                true,
            )
            .unwrap();
        events.lock().unwrap().clear();
        controller.shutdown().unwrap();

        let applied = events
            .lock()
            .unwrap()
            .iter()
            .filter(|event| event.starts_with("apply:"))
            .cloned()
            .collect::<Vec<_>>();
        assert_eq!(
            applied,
            vec!["apply:101:0".to_owned(), "apply:100:0".to_owned()]
        );
    }

    #[test]
    #[ignore = "modifies and restores threads in a disposable Windows process; run in explicit integration QA"]
    fn thread_priority_live_apply_and_clean_release() -> Result<(), String> {
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
        let baseline = current_process_thread_priority(&target, true)?
            .ok_or_else(|| "Disposable process started with mixed thread priorities.".to_owned())?;
        let expected = if baseline == ProcessThreadPrioritySetting::BelowNormal {
            ProcessThreadPrioritySetting::AboveNormal
        } else {
            ProcessThreadPrioritySetting::BelowNormal
        };
        let mut controller = ThreadPriorityController::default();

        controller
            .apply_process_list_action(&target, expected, true)
            .map_err(|error| error.to_string())?;
        assert_eq!(
            current_process_thread_priority(&target, true)?,
            Some(expected)
        );
        controller.shutdown()?;
        assert_eq!(
            current_process_thread_priority(&target, true)?,
            Some(baseline)
        );
        Ok(())
    }
}
