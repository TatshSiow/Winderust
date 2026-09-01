use std::collections::{BTreeMap, BTreeSet};

use crate::{
    backend::crash_recovery::{forget_thread_suspension, record_thread_suspension, RecoveryIntent},
    control::{
        cpu_limiter::{CpuLimiterTarget, CpuLimiterTargetError},
        process::{open_process_for_thread_control, ProcessIdentity},
    },
    platform::windows::thread_suspension::{
        self, CapturedThread, ThreadHandle, ThreadSuspensionError,
    },
    win_util::WinHandle,
};

pub(super) trait ThreadFallbackRecoveryIntent {
    fn commit(self) -> Result<(), CpuLimiterTargetError>;
}

pub(super) trait ThreadFallbackOperations {
    type ProcessHandle;
    type ThreadHandle;
    type RecoveryIntent: ThreadFallbackRecoveryIntent;

    fn open_process(
        &mut self,
        target: &CpuLimiterTarget,
    ) -> Result<(ProcessIdentity, Self::ProcessHandle), CpuLimiterTargetError>;
    fn capture_threads(
        &mut self,
        process: &Self::ProcessHandle,
    ) -> Result<Vec<CapturedThread>, CpuLimiterTargetError>;
    fn open_thread(
        &mut self,
        process: &ProcessIdentity,
        captured: CapturedThread,
    ) -> Result<Self::ThreadHandle, CpuLimiterTargetError>;
    fn is_thread_active(
        &mut self,
        thread: &Self::ThreadHandle,
    ) -> Result<bool, CpuLimiterTargetError>;
    fn begin_suspension(
        &mut self,
        process: &Self::ProcessHandle,
        thread: &Self::ThreadHandle,
        captured: CapturedThread,
    ) -> Result<Self::RecoveryIntent, CpuLimiterTargetError>;
    fn suspend_once(&mut self, thread: &Self::ThreadHandle) -> Result<u32, CpuLimiterTargetError>;
    fn resume_once(&mut self, thread: &Self::ThreadHandle) -> Result<u32, CpuLimiterTargetError>;
    fn forget(
        &mut self,
        process: &ProcessIdentity,
        captured: CapturedThread,
    ) -> Result<(), CpuLimiterTargetError>;
}

pub(super) struct ThreadFallbackTarget<
    O: ThreadFallbackOperations = WindowsThreadFallbackOperations,
> {
    process: ProcessIdentity,
    process_handle: O::ProcessHandle,
    threads: BTreeMap<(u32, u64), ManagedThread<O>>,
    frozen: bool,
    operations: O,
}

// SAFETY: the production target owns only process/thread kernel handles, which remain valid when
// moved between threads. WorkerState's mutex serializes every operation on those handles.
unsafe impl Send for ThreadFallbackTarget<WindowsThreadFallbackOperations> {}

struct ManagedThread<O: ThreadFallbackOperations> {
    captured: CapturedThread,
    handle: O::ThreadHandle,
    recovery_armed: bool,
    owned_increment: bool,
}

impl ThreadFallbackTarget<WindowsThreadFallbackOperations> {
    pub(super) fn prepare(target: CpuLimiterTarget) -> Result<Self, CpuLimiterTargetError> {
        Self::prepare_with(target, WindowsThreadFallbackOperations)
    }
}

impl<O: ThreadFallbackOperations> ThreadFallbackTarget<O> {
    fn prepare_with(
        target: CpuLimiterTarget,
        mut operations: O,
    ) -> Result<Self, CpuLimiterTargetError> {
        let (process, process_handle) = operations.open_process(&target)?;
        if process.key() != target.suspension_target.process.key() {
            return Err(crate::control::process::ProcessControlError::ProcessExited.into());
        }
        let captured = operations.capture_threads(&process_handle)?;
        let mut threads = BTreeMap::new();
        for captured in captured {
            let handle = operations.open_thread(&process, captured)?;
            threads.insert(
                (captured.id, captured.creation_time),
                ManagedThread {
                    captured,
                    handle,
                    recovery_armed: false,
                    owned_increment: false,
                },
            );
        }
        Ok(Self {
            process,
            process_handle,
            threads,
            frozen: false,
            operations,
        })
    }

    pub(super) fn freeze_known_threads(&mut self) -> Result<(), CpuLimiterTargetError> {
        let keys = self.threads.keys().copied().collect::<Vec<_>>();
        self.freeze_threads(&keys)
    }

    pub(super) fn adopt_new_threads(
        &mut self,
        inventory: &BTreeSet<u32>,
    ) -> Result<(), CpuLimiterTargetError> {
        let result = self.adopt_new_threads_inner(inventory);
        if result.is_err() {
            self.frozen = false;
        }
        result
    }

    fn adopt_new_threads_inner(
        &mut self,
        inventory: &BTreeSet<u32>,
    ) -> Result<(), CpuLimiterTargetError> {
        self.remove_inactive_threads()?;
        let unknown = inventory
            .iter()
            .copied()
            .filter(|id| {
                !self
                    .threads
                    .values()
                    .any(|thread| thread.captured.id == *id)
            })
            .collect::<BTreeSet<_>>();
        if unknown.is_empty() {
            self.frozen = inventory.iter().all(|id| {
                self.threads
                    .values()
                    .any(|thread| thread.captured.id == *id && thread.owned_increment)
            });
            return Ok(());
        }

        let captured = self.operations.capture_threads(&self.process_handle)?;
        let captured = captured
            .into_iter()
            .filter(|thread| unknown.contains(&thread.id))
            .map(|thread| (thread.id, thread))
            .collect::<BTreeMap<_, _>>();
        let mut opened = Vec::with_capacity(unknown.len());
        for id in unknown {
            let captured = captured
                .get(&id)
                .copied()
                .ok_or(ThreadSuspensionError::ThreadExited { thread_id: id })?;
            let handle = self.operations.open_thread(&self.process, captured)?;
            opened.push((captured, handle));
        }

        let mut keys = Vec::with_capacity(opened.len());
        for (captured, handle) in opened {
            let key = (captured.id, captured.creation_time);
            keys.push(key);
            self.threads.insert(
                key,
                ManagedThread {
                    captured,
                    handle,
                    recovery_armed: false,
                    owned_increment: false,
                },
            );
        }
        self.freeze_threads(&keys)?;
        self.frozen = inventory.iter().all(|id| {
            self.threads
                .values()
                .any(|thread| thread.captured.id == *id && thread.owned_increment)
        });
        Ok(())
    }

    pub(super) fn thaw(&mut self) -> Result<(), CpuLimiterTargetError> {
        let keys = self.threads.keys().copied().collect::<Vec<_>>();
        let mut failure = None;
        for key in keys {
            let result = {
                let Some(thread) = self.threads.get_mut(&key) else {
                    return Err(missing_thread_state(key));
                };
                if thread.owned_increment {
                    resume_owned_thread(&mut self.operations, &self.process, thread, false)
                } else {
                    Ok(())
                }
            };
            if failure.is_none() {
                failure = result.err();
            }
        }
        self.frozen = self.threads.values().any(|thread| thread.owned_increment);
        match failure {
            Some(error) => Err(error),
            None => Ok(()),
        }
    }

    pub(super) fn release(&mut self) -> Result<bool, CpuLimiterTargetError> {
        let mut failure = self.thaw().err();
        let keys = self.threads.keys().copied().collect::<Vec<_>>();
        for key in keys {
            let Some(thread) = self.threads.get_mut(&key) else {
                continue;
            };
            if thread.owned_increment || !thread.recovery_armed {
                continue;
            }
            if let Err(error) = forget_thread(&mut self.operations, &self.process, thread) {
                failure = Some(combine_failure(failure, error));
            }
        }
        self.threads
            .retain(|_, thread| thread.owned_increment || thread.recovery_armed);
        match failure {
            Some(error) => Err(error),
            None => Ok(self.threads.is_empty()),
        }
    }

    fn remove_inactive_threads(&mut self) -> Result<(), CpuLimiterTargetError> {
        let keys = self.threads.keys().copied().collect::<Vec<_>>();
        for key in keys {
            let active = {
                let Some(thread) = self.threads.get(&key) else {
                    return Err(missing_thread_state(key));
                };
                self.operations.is_thread_active(&thread.handle)?
            };
            if active {
                continue;
            }
            let Some(thread) = self.threads.get_mut(&key) else {
                return Err(missing_thread_state(key));
            };
            thread.owned_increment = false;
            forget_thread(&mut self.operations, &self.process, thread)?;
            self.threads.remove(&key);
        }
        Ok(())
    }

    fn freeze_threads(&mut self, keys: &[(u32, u64)]) -> Result<(), CpuLimiterTargetError> {
        let mut changed = Vec::new();
        for key in keys {
            let Some(thread) = self.threads.get(key) else {
                return Err(missing_thread_state(*key));
            };
            if thread.owned_increment {
                continue;
            }
            let result = {
                let Some(thread) = self.threads.get_mut(key) else {
                    return Err(missing_thread_state(*key));
                };
                suspend_thread(
                    &mut self.operations,
                    &self.process_handle,
                    &self.process,
                    thread,
                )
            };
            if let Err(cause) = result {
                let rollback = self.rollback_threads(&changed);
                self.frozen = false;
                return Err(match rollback {
                    Some(rollback) => CpuLimiterTargetError::Rollback {
                        cause: Box::new(cause),
                        rollback: Box::new(rollback),
                    },
                    None => cause,
                });
            }
            changed.push(*key);
        }
        self.frozen = self.threads.values().all(|thread| thread.owned_increment);
        Ok(())
    }

    fn rollback_threads(&mut self, keys: &[(u32, u64)]) -> Option<CpuLimiterTargetError> {
        let mut failure = None;
        for key in keys.iter().rev() {
            let result = {
                let Some(thread) = self.threads.get_mut(key) else {
                    if failure.is_none() {
                        failure = Some(missing_thread_state(*key));
                    }
                    continue;
                };
                resume_owned_thread(&mut self.operations, &self.process, thread, true)
            };
            if failure.is_none() {
                failure = result.err();
            }
        }
        failure
    }
}

fn suspend_thread<O: ThreadFallbackOperations>(
    operations: &mut O,
    process_handle: &O::ProcessHandle,
    process: &ProcessIdentity,
    thread: &mut ManagedThread<O>,
) -> Result<(), CpuLimiterTargetError> {
    let intent = if thread.recovery_armed {
        None
    } else {
        Some(operations.begin_suspension(process_handle, &thread.handle, thread.captured)?)
    };
    let prior_count = operations.suspend_once(&thread.handle)?;
    thread.owned_increment = true;
    if let Some(intent) = intent {
        if let Err(cause) = intent.commit() {
            return match resume_owned_thread(operations, process, thread, true) {
                Ok(()) => Err(cause),
                Err(rollback) => Err(CpuLimiterTargetError::Rollback {
                    cause: Box::new(cause),
                    rollback: Box::new(rollback),
                }),
            };
        }
        thread.recovery_armed = true;
    }

    let expected = u32::from(thread.captured.suspend_count);
    if prior_count == expected {
        return Ok(());
    }
    let cause = suspend_count_conflict(thread.captured.id, expected, prior_count);
    match compensate_conflicting_suspend(operations, process, thread) {
        Ok(()) => Err(cause),
        Err(rollback) => Err(CpuLimiterTargetError::Rollback {
            cause: Box::new(cause),
            rollback: Box::new(rollback),
        }),
    }
}

fn compensate_conflicting_suspend<O: ThreadFallbackOperations>(
    operations: &mut O,
    process: &ProcessIdentity,
    thread: &mut ManagedThread<O>,
) -> Result<(), CpuLimiterTargetError> {
    match operations.resume_once(&thread.handle) {
        Ok(_) | Err(CpuLimiterTargetError::Thread(ThreadSuspensionError::ThreadExited { .. })) => {
            thread.owned_increment = false;
            forget_thread(operations, process, thread)
        }
        Err(error) => Err(error),
    }
}

fn resume_owned_thread<O: ThreadFallbackOperations>(
    operations: &mut O,
    process: &ProcessIdentity,
    thread: &mut ManagedThread<O>,
    forget_recovery: bool,
) -> Result<(), CpuLimiterTargetError> {
    let prior_count = match operations.resume_once(&thread.handle) {
        Ok(prior_count) => prior_count,
        Err(CpuLimiterTargetError::Thread(ThreadSuspensionError::ThreadExited { .. })) => {
            thread.owned_increment = false;
            return forget_thread(operations, process, thread);
        }
        Err(error) => return Err(error),
    };
    thread.owned_increment = false;
    let expected = u32::from(thread.captured.suspend_count) + 1;
    let conflict = (prior_count != expected)
        .then(|| suspend_count_conflict(thread.captured.id, expected, prior_count));
    let forget_result = if forget_recovery || conflict.is_some() {
        forget_thread(operations, process, thread)
    } else {
        Ok(())
    };
    match (conflict, forget_result) {
        (Some(cause), Err(rollback)) => Err(CpuLimiterTargetError::Rollback {
            cause: Box::new(cause),
            rollback: Box::new(rollback),
        }),
        (Some(error), Ok(())) | (None, Err(error)) => Err(error),
        (None, Ok(())) => Ok(()),
    }
}

fn combine_failure(
    prior: Option<CpuLimiterTargetError>,
    error: CpuLimiterTargetError,
) -> CpuLimiterTargetError {
    match prior {
        Some(cause) => CpuLimiterTargetError::Rollback {
            cause: Box::new(cause),
            rollback: Box::new(error),
        },
        None => error,
    }
}

fn forget_thread<O: ThreadFallbackOperations>(
    operations: &mut O,
    process: &ProcessIdentity,
    thread: &mut ManagedThread<O>,
) -> Result<(), CpuLimiterTargetError> {
    if thread.recovery_armed {
        operations.forget(process, thread.captured)?;
        thread.recovery_armed = false;
    }
    Ok(())
}

fn suspend_count_conflict(thread_id: u32, expected: u32, actual: u32) -> CpuLimiterTargetError {
    ThreadSuspensionError::SuspendCountConflict {
        thread_id,
        expected,
        actual,
    }
    .into()
}

fn missing_thread_state(key: (u32, u64)) -> CpuLimiterTargetError {
    CpuLimiterTargetError::Recovery(format!(
        "CPU Limiter thread {}:{} disappeared from managed state.",
        key.0, key.1
    ))
}

pub(super) struct WindowsThreadFallbackOperations;

impl ThreadFallbackRecoveryIntent for RecoveryIntent {
    fn commit(self) -> Result<(), CpuLimiterTargetError> {
        RecoveryIntent::commit(self).map_err(CpuLimiterTargetError::Recovery)
    }
}

impl ThreadFallbackOperations for WindowsThreadFallbackOperations {
    type ProcessHandle = WinHandle;
    type ThreadHandle = ThreadHandle;
    type RecoveryIntent = RecoveryIntent;

    fn open_process(
        &mut self,
        target: &CpuLimiterTarget,
    ) -> Result<(ProcessIdentity, Self::ProcessHandle), CpuLimiterTargetError> {
        open_process_for_thread_control(
            &target.suspension_target.process,
            target.allow_cross_session_process_control,
        )
        .map_err(Into::into)
    }

    fn capture_threads(
        &mut self,
        process: &Self::ProcessHandle,
    ) -> Result<Vec<CapturedThread>, CpuLimiterTargetError> {
        thread_suspension::capture_threads(process.raw()).map_err(Into::into)
    }

    fn open_thread(
        &mut self,
        process: &ProcessIdentity,
        captured: CapturedThread,
    ) -> Result<Self::ThreadHandle, CpuLimiterTargetError> {
        thread_suspension::open_exact_thread(process.id, captured).map_err(Into::into)
    }

    fn is_thread_active(
        &mut self,
        thread: &Self::ThreadHandle,
    ) -> Result<bool, CpuLimiterTargetError> {
        thread_suspension::is_active(thread).map_err(Into::into)
    }

    fn begin_suspension(
        &mut self,
        process: &Self::ProcessHandle,
        thread: &Self::ThreadHandle,
        captured: CapturedThread,
    ) -> Result<Self::RecoveryIntent, CpuLimiterTargetError> {
        record_thread_suspension(process.raw(), thread.raw(), captured.suspend_count)
            .map_err(CpuLimiterTargetError::Recovery)
    }

    fn suspend_once(&mut self, thread: &Self::ThreadHandle) -> Result<u32, CpuLimiterTargetError> {
        thread_suspension::suspend_once(thread).map_err(Into::into)
    }

    fn resume_once(&mut self, thread: &Self::ThreadHandle) -> Result<u32, CpuLimiterTargetError> {
        thread_suspension::resume_once(thread).map_err(Into::into)
    }

    fn forget(
        &mut self,
        process: &ProcessIdentity,
        captured: CapturedThread,
    ) -> Result<(), CpuLimiterTargetError> {
        forget_thread_suspension(
            process.id,
            process.creation_time,
            captured.id,
            captured.creation_time,
        )
        .map_err(CpuLimiterTargetError::Recovery)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        cell::RefCell,
        collections::{BTreeMap, BTreeSet, VecDeque},
        path::PathBuf,
        rc::Rc,
    };

    use crate::control::suspension::SuspensionTarget;

    use super::*;

    #[derive(Debug, Clone, PartialEq, Eq)]
    enum Event {
        OpenProcess,
        Snapshot,
        OpenThread(u32),
        IsActive(u32),
        Begin(u32),
        Suspend(u32),
        Commit(u32),
        Resume(u32),
        Forget(u32),
    }

    struct FakeState {
        events: Vec<Event>,
        process_identity: ProcessIdentity,
        captures: VecDeque<Result<Vec<CapturedThread>, CpuLimiterTargetError>>,
        open_errors: BTreeMap<u32, CpuLimiterTargetError>,
        active_results: BTreeMap<u32, VecDeque<Result<bool, CpuLimiterTargetError>>>,
        baselines: BTreeMap<u32, u16>,
        suspend_results: BTreeMap<u32, VecDeque<Result<u32, CpuLimiterTargetError>>>,
        resume_results: BTreeMap<u32, VecDeque<Result<u32, CpuLimiterTargetError>>>,
        recovery: BTreeSet<(u32, u64)>,
    }

    #[derive(Clone)]
    struct FakeOperations(Rc<RefCell<FakeState>>);

    struct FakeIntent {
        state: Rc<RefCell<FakeState>>,
        key: (u32, u64),
    }

    impl ThreadFallbackRecoveryIntent for FakeIntent {
        fn commit(self) -> Result<(), CpuLimiterTargetError> {
            let mut state = self.state.borrow_mut();
            state.events.push(Event::Commit(self.key.0));
            state.recovery.insert(self.key);
            Ok(())
        }
    }

    impl ThreadFallbackOperations for FakeOperations {
        type ProcessHandle = u32;
        type ThreadHandle = u32;
        type RecoveryIntent = FakeIntent;

        fn open_process(
            &mut self,
            _target: &CpuLimiterTarget,
        ) -> Result<(ProcessIdentity, Self::ProcessHandle), CpuLimiterTargetError> {
            let mut state = self.0.borrow_mut();
            state.events.push(Event::OpenProcess);
            Ok((state.process_identity.clone(), 1))
        }

        fn capture_threads(
            &mut self,
            _process: &Self::ProcessHandle,
        ) -> Result<Vec<CapturedThread>, CpuLimiterTargetError> {
            let mut state = self.0.borrow_mut();
            state.events.push(Event::Snapshot);
            state.captures.pop_front().unwrap_or_else(|| Ok(Vec::new()))
        }

        fn open_thread(
            &mut self,
            _process: &ProcessIdentity,
            captured: CapturedThread,
        ) -> Result<Self::ThreadHandle, CpuLimiterTargetError> {
            let mut state = self.0.borrow_mut();
            state.events.push(Event::OpenThread(captured.id));
            if let Some(error) = state.open_errors.remove(&captured.id) {
                return Err(error);
            }
            state.baselines.insert(captured.id, captured.suspend_count);
            Ok(captured.id)
        }

        fn is_thread_active(
            &mut self,
            thread: &Self::ThreadHandle,
        ) -> Result<bool, CpuLimiterTargetError> {
            let mut state = self.0.borrow_mut();
            state.events.push(Event::IsActive(*thread));
            state
                .active_results
                .get_mut(thread)
                .and_then(VecDeque::pop_front)
                .unwrap_or(Ok(true))
        }

        fn begin_suspension(
            &mut self,
            _process: &Self::ProcessHandle,
            thread: &Self::ThreadHandle,
            captured: CapturedThread,
        ) -> Result<Self::RecoveryIntent, CpuLimiterTargetError> {
            self.0.borrow_mut().events.push(Event::Begin(*thread));
            Ok(FakeIntent {
                state: Rc::clone(&self.0),
                key: (captured.id, captured.creation_time),
            })
        }

        fn suspend_once(
            &mut self,
            thread: &Self::ThreadHandle,
        ) -> Result<u32, CpuLimiterTargetError> {
            let mut state = self.0.borrow_mut();
            state.events.push(Event::Suspend(*thread));
            let baseline = u32::from(state.baselines[thread]);
            state
                .suspend_results
                .get_mut(thread)
                .and_then(VecDeque::pop_front)
                .unwrap_or(Ok(baseline))
        }

        fn resume_once(
            &mut self,
            thread: &Self::ThreadHandle,
        ) -> Result<u32, CpuLimiterTargetError> {
            let mut state = self.0.borrow_mut();
            state.events.push(Event::Resume(*thread));
            let expected = u32::from(state.baselines[thread]) + 1;
            state
                .resume_results
                .get_mut(thread)
                .and_then(VecDeque::pop_front)
                .unwrap_or(Ok(expected))
        }

        fn forget(
            &mut self,
            _process: &ProcessIdentity,
            captured: CapturedThread,
        ) -> Result<(), CpuLimiterTargetError> {
            let mut state = self.0.borrow_mut();
            state.events.push(Event::Forget(captured.id));
            state
                .recovery
                .remove(&(captured.id, captured.creation_time));
            Ok(())
        }
    }

    fn target() -> CpuLimiterTarget {
        CpuLimiterTarget {
            suspension_target: SuspensionTarget::automatic(
                42,
                "app.exe".to_owned(),
                PathBuf::from(r"C:\Apps\app.exe"),
                7,
                Some(false),
            ),
            allowed_cpu_time_percent: 50,
            allow_cross_session_process_control: true,
            ancestor_process_ids: Vec::new(),
        }
    }

    fn thread(id: u32, creation_time: u64, suspend_count: u16) -> CapturedThread {
        CapturedThread {
            id,
            creation_time,
            suspend_count,
        }
    }

    fn harness(
        captures: impl IntoIterator<Item = Vec<CapturedThread>>,
    ) -> (FakeOperations, Rc<RefCell<FakeState>>) {
        let state = Rc::new(RefCell::new(FakeState {
            events: Vec::new(),
            process_identity: ProcessIdentity::new(
                42,
                "app.exe".to_owned(),
                PathBuf::from(r"C:\Apps\app.exe"),
                7,
                Some(1),
            ),
            captures: captures.into_iter().map(Ok).collect(),
            open_errors: BTreeMap::new(),
            active_results: BTreeMap::new(),
            baselines: BTreeMap::new(),
            suspend_results: BTreeMap::new(),
            resume_results: BTreeMap::new(),
            recovery: BTreeSet::new(),
        }));
        (FakeOperations(Rc::clone(&state)), state)
    }

    fn prepare(
        operations: FakeOperations,
    ) -> Result<ThreadFallbackTarget<FakeOperations>, CpuLimiterTargetError> {
        ThreadFallbackTarget::prepare_with(target(), operations)
    }

    #[test]
    fn first_freeze_prepares_before_arming_and_suspends_each_thread_once() {
        let (operations, state) = harness([vec![thread(1, 11, 3), thread(2, 22, 0)]]);
        let mut managed = prepare(operations).unwrap();

        managed.freeze_known_threads().unwrap();

        assert!(managed.frozen);
        assert!(managed
            .threads
            .values()
            .all(|thread| thread.owned_increment));
        assert_eq!(state.borrow().recovery, BTreeSet::from([(1, 11), (2, 22)]));
        assert_eq!(
            state.borrow().events,
            [
                Event::OpenProcess,
                Event::Snapshot,
                Event::OpenThread(1),
                Event::OpenThread(2),
                Event::Begin(1),
                Event::Suspend(1),
                Event::Commit(1),
                Event::Begin(2),
                Event::Suspend(2),
                Event::Commit(2),
            ]
        );
    }

    #[test]
    fn awake_cycles_reuse_one_recovery_record_until_release() {
        let (operations, state) = harness([vec![thread(1, 11, 4)]]);
        let mut managed = prepare(operations).unwrap();
        managed.freeze_known_threads().unwrap();
        managed.thaw().unwrap();
        managed.freeze_known_threads().unwrap();
        managed.thaw().unwrap();

        assert!(!managed.frozen);
        assert!(!managed.threads[&(1, 11)].owned_increment);
        assert!(managed.threads[&(1, 11)].recovery_armed);
        assert_eq!(state.borrow().recovery, BTreeSet::from([(1, 11)]));
        assert_eq!(
            state
                .borrow()
                .events
                .iter()
                .filter(|event| matches!(event, Event::Suspend(1)))
                .count(),
            2
        );
        assert_eq!(
            state
                .borrow()
                .events
                .iter()
                .filter(|event| matches!(event, Event::Resume(1)))
                .count(),
            2
        );
        assert_eq!(
            state
                .borrow()
                .events
                .iter()
                .filter(|event| matches!(event, Event::Begin(1) | Event::Commit(1)))
                .count(),
            2
        );
        assert!(!state.borrow().events.contains(&Event::Forget(1)));

        assert!(managed.release().unwrap());
        assert!(state.borrow().recovery.is_empty());
        assert_eq!(state.borrow().events.last(), Some(&Event::Forget(1)));
    }

    #[test]
    fn partial_freeze_failure_compensates_prior_increments() {
        let (operations, state) = harness([vec![thread(1, 11, 0), thread(2, 22, 0)]]);
        state
            .borrow_mut()
            .suspend_results
            .insert(2, VecDeque::from([Err(failed_thread("SuspendThread", 2))]));
        let mut managed = prepare(operations).unwrap();

        assert!(managed.freeze_known_threads().is_err());

        assert!(!managed.frozen);
        assert!(managed
            .threads
            .values()
            .all(|thread| !thread.owned_increment));
        assert!(state.borrow().recovery.is_empty());
        assert!(state.borrow().events.contains(&Event::Resume(1)));
    }

    #[test]
    fn failed_thaw_retains_owned_increment_and_recovery_for_retry() {
        let (operations, state) = harness([vec![thread(1, 11, 0)]]);
        state.borrow_mut().resume_results.insert(
            1,
            VecDeque::from([Err(failed_thread("ResumeThread", 1)), Ok(1)]),
        );
        let mut managed = prepare(operations).unwrap();
        managed.freeze_known_threads().unwrap();

        assert!(managed.thaw().is_err());
        assert!(managed.threads[&(1, 11)].owned_increment);
        assert!(managed.threads[&(1, 11)].recovery_armed);
        assert_eq!(state.borrow().recovery, BTreeSet::from([(1, 11)]));

        managed.thaw().unwrap();
        assert!(!managed.threads[&(1, 11)].owned_increment);
        assert_eq!(state.borrow().recovery, BTreeSet::from([(1, 11)]));
    }

    #[test]
    fn successful_thaw_count_conflict_forgets_recovery_after_one_resume() {
        let (operations, state) = harness([vec![thread(1, 11, 4)]]);
        state
            .borrow_mut()
            .resume_results
            .insert(1, VecDeque::from([Ok(6)]));
        let mut managed = prepare(operations).unwrap();
        managed.freeze_known_threads().unwrap();

        assert!(matches!(
            managed.thaw(),
            Err(CpuLimiterTargetError::Thread(
                ThreadSuspensionError::SuspendCountConflict {
                    thread_id: 1,
                    expected: 5,
                    actual: 6,
                }
            ))
        ));
        assert!(!managed.threads[&(1, 11)].owned_increment);
        assert!(!managed.threads[&(1, 11)].recovery_armed);
        assert!(state.borrow().recovery.is_empty());
        assert_eq!(
            state
                .borrow()
                .events
                .iter()
                .filter(|event| matches!(event, Event::Resume(1)))
                .count(),
            1
        );
        assert_eq!(state.borrow().events.last(), Some(&Event::Forget(1)));
    }

    #[test]
    fn suspend_count_conflict_compensates_at_most_once_and_fails() {
        let (operations, state) = harness([vec![thread(1, 11, 0)]]);
        state
            .borrow_mut()
            .suspend_results
            .insert(1, VecDeque::from([Ok(2)]));
        state
            .borrow_mut()
            .resume_results
            .insert(1, VecDeque::from([Ok(3)]));
        let mut managed = prepare(operations).unwrap();

        assert!(matches!(
            managed.freeze_known_threads(),
            Err(CpuLimiterTargetError::Thread(
                ThreadSuspensionError::SuspendCountConflict { .. }
            ))
        ));
        assert!(!managed.threads[&(1, 11)].owned_increment);
        assert!(state.borrow().recovery.is_empty());
        assert_eq!(
            state
                .borrow()
                .events
                .iter()
                .filter(|event| matches!(event, Event::Resume(1)))
                .count(),
            1
        );
    }

    #[test]
    fn new_inventory_threads_are_snapshotted_opened_armed_and_frozen() {
        let (operations, state) = harness([
            vec![thread(1, 11, 0)],
            vec![thread(1, 11, 1), thread(2, 22, 0)],
        ]);
        let mut managed = prepare(operations).unwrap();
        managed.freeze_known_threads().unwrap();
        let event_count = state.borrow().events.len();

        managed.adopt_new_threads(&BTreeSet::from([1, 2])).unwrap();

        assert!(managed.frozen);
        assert!(managed.threads[&(2, 22)].owned_increment);
        assert_eq!(
            &state.borrow().events[event_count..],
            &[
                Event::IsActive(1),
                Event::Snapshot,
                Event::OpenThread(2),
                Event::Begin(2),
                Event::Suspend(2),
                Event::Commit(2),
            ]
        );
    }

    #[test]
    fn reused_numeric_thread_id_replaces_the_inactive_generation() {
        let (operations, state) = harness([vec![thread(1, 11, 0)], vec![thread(1, 22, 0)]]);
        let mut managed = prepare(operations).unwrap();
        managed.freeze_known_threads().unwrap();
        managed.thaw().unwrap();
        state
            .borrow_mut()
            .active_results
            .insert(1, VecDeque::from([Ok(false)]));
        let event_count = state.borrow().events.len();

        managed.adopt_new_threads(&BTreeSet::from([1])).unwrap();

        assert!(managed.frozen);
        assert!(!managed.threads.contains_key(&(1, 11)));
        assert!(managed.threads[&(1, 22)].owned_increment);
        assert_eq!(state.borrow().recovery, BTreeSet::from([(1, 22)]));
        assert_eq!(
            &state.borrow().events[event_count..],
            &[
                Event::IsActive(1),
                Event::Forget(1),
                Event::Snapshot,
                Event::OpenThread(1),
                Event::Begin(1),
                Event::Suspend(1),
                Event::Commit(1),
            ]
        );
    }

    #[test]
    fn inactive_thread_without_replacement_is_removed_without_a_snapshot() {
        let (operations, state) = harness([vec![thread(1, 11, 0)]]);
        let mut managed = prepare(operations).unwrap();
        managed.freeze_known_threads().unwrap();
        managed.thaw().unwrap();
        state
            .borrow_mut()
            .active_results
            .insert(1, VecDeque::from([Ok(false)]));
        let event_count = state.borrow().events.len();

        managed.adopt_new_threads(&BTreeSet::new()).unwrap();

        assert!(managed.threads.is_empty());
        assert!(state.borrow().recovery.is_empty());
        assert_eq!(
            &state.borrow().events[event_count..],
            &[Event::IsActive(1), Event::Forget(1)]
        );
    }

    #[test]
    fn reused_process_or_thread_identity_is_rejected() {
        let (process_operations, process_state) = harness([Vec::new()]);
        process_state.borrow_mut().process_identity.creation_time = 8;
        assert!(matches!(
            prepare(process_operations),
            Err(CpuLimiterTargetError::Process(
                crate::control::process::ProcessControlError::ProcessExited
            ))
        ));

        let (thread_operations, thread_state) = harness([vec![thread(1, 11, 0)]]);
        thread_state.borrow_mut().open_errors.insert(
            1,
            ThreadSuspensionError::IdentityChanged { thread_id: 1 }.into(),
        );
        assert!(matches!(
            prepare(thread_operations),
            Err(CpuLimiterTargetError::Thread(
                ThreadSuspensionError::IdentityChanged { thread_id: 1 }
            ))
        ));
    }

    #[test]
    fn rejected_new_thread_identity_marks_the_inventory_not_fully_frozen() {
        let (operations, state) = harness([
            vec![thread(1, 11, 0)],
            vec![thread(1, 11, 1), thread(2, 22, 0)],
        ]);
        let mut managed = prepare(operations).unwrap();
        managed.freeze_known_threads().unwrap();
        state.borrow_mut().open_errors.insert(
            2,
            ThreadSuspensionError::IdentityChanged { thread_id: 2 }.into(),
        );

        assert!(matches!(
            managed.adopt_new_threads(&BTreeSet::from([1, 2])),
            Err(CpuLimiterTargetError::Thread(
                ThreadSuspensionError::IdentityChanged { thread_id: 2 }
            ))
        ));
        assert!(!managed.frozen);
        assert!(managed.threads[&(1, 11)].owned_increment);
        assert!(!managed
            .threads
            .values()
            .any(|thread| thread.captured.id == 2));
    }

    #[test]
    fn release_forgets_only_after_successful_owned_thaw() {
        let (operations, state) = harness([vec![thread(1, 11, 0)]]);
        let mut managed = prepare(operations).unwrap();
        managed.freeze_known_threads().unwrap();
        let event_count = state.borrow().events.len();

        assert!(managed.release().unwrap());

        assert_eq!(
            &state.borrow().events[event_count..],
            &[Event::Resume(1), Event::Forget(1)]
        );
        assert!(managed.threads.is_empty());
    }

    #[test]
    fn repeated_release_is_idempotent() {
        let (operations, state) = harness([vec![thread(1, 11, 0)]]);
        let mut managed = prepare(operations).unwrap();
        managed.freeze_known_threads().unwrap();
        assert!(managed.release().unwrap());
        let event_count = state.borrow().events.len();

        assert!(managed.release().unwrap());

        assert_eq!(state.borrow().events.len(), event_count);
    }

    fn failed_thread(operation: &'static str, thread_id: u32) -> CpuLimiterTargetError {
        ThreadSuspensionError::Failed {
            operation,
            thread_id: Some(thread_id),
            code: 5,
        }
        .into()
    }
}
