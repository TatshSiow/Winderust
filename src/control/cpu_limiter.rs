use std::{
    collections::{BTreeMap, BTreeSet},
    panic::{catch_unwind, AssertUnwindSafe},
    sync::{Arc, Mutex},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use crate::{
    control::{
        process::{ProcessControlError, ProcessTargetKey},
        suspension::{SuspensionController, SuspensionError, SuspensionTarget},
    },
    platform::windows::{
        cpu_limiter::{CommandEvent, HighResolutionTimer},
        thread_suspension::{self, ThreadSuspensionError},
    },
};

mod thread_fallback;

const CYCLE: Duration = Duration::from_millis(100);

fn phase_lengths(allowed_cpu_time_percent: u8) -> Option<(Duration, Duration)> {
    (1..=99).contains(&allowed_cpu_time_percent).then(|| {
        let awake = Duration::from_millis(u64::from(allowed_cpu_time_percent));
        (awake, CYCLE - awake)
    })
}

struct DutySchedule {
    cycle_started: Instant,
    awake: Duration,
    frozen: bool,
    next_deadline: Instant,
}

#[derive(Clone)]
pub(crate) struct CpuLimiterTarget {
    pub(crate) suspension_target: SuspensionTarget,
    pub(crate) allowed_cpu_time_percent: u8,
    pub(crate) allow_cross_session_process_control: bool,
    pub(crate) ancestor_process_ids: Vec<u32>,
}

#[derive(Debug)]
pub(crate) struct CpuLimiterFailure {
    pub(crate) suspension_target: SuspensionTarget,
    pub(crate) error: CpuLimiterTargetError,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum CpuLimiterTargetError {
    Job(SuspensionError),
    Process(ProcessControlError),
    Thread(ThreadSuspensionError),
    Recovery(String),
    Rollback {
        cause: Box<CpuLimiterTargetError>,
        rollback: Box<CpuLimiterTargetError>,
    },
}

impl CpuLimiterTargetError {
    pub(crate) fn is_access_denied(&self) -> bool {
        match self {
            Self::Job(SuspensionError::AccessDenied)
            | Self::Process(ProcessControlError::AccessDenied(_))
            | Self::Thread(ThreadSuspensionError::AccessDenied { .. }) => true,
            Self::Rollback { cause, rollback } => {
                cause.is_access_denied() || rollback.is_access_denied()
            }
            _ => false,
        }
    }

    pub(crate) fn should_report(&self) -> bool {
        match self {
            Self::Job(error) => error.should_report(),
            Self::Process(ProcessControlError::ProcessExited)
            | Self::Thread(ThreadSuspensionError::ProcessExited)
            | Self::Thread(ThreadSuspensionError::ThreadExited { .. }) => false,
            Self::Rollback { cause, rollback } => cause.should_report() || rollback.should_report(),
            _ => true,
        }
    }
}

impl std::fmt::Display for CpuLimiterTargetError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Job(error) => error.fmt(formatter),
            Self::Process(error) => error.fmt(formatter),
            Self::Thread(error) => write!(formatter, "{error:?}"),
            Self::Recovery(error) => formatter.write_str(error),
            Self::Rollback { cause, rollback } => {
                write!(formatter, "{cause} Rollback failed: {rollback}")
            }
        }
    }
}

impl std::error::Error for CpuLimiterTargetError {}

impl From<SuspensionError> for CpuLimiterTargetError {
    fn from(error: SuspensionError) -> Self {
        Self::Job(error)
    }
}

impl From<ProcessControlError> for CpuLimiterTargetError {
    fn from(error: ProcessControlError) -> Self {
        Self::Process(error)
    }
}

impl From<ThreadSuspensionError> for CpuLimiterTargetError {
    fn from(error: ThreadSuspensionError) -> Self {
        Self::Thread(error)
    }
}

enum CpuLimiterBackend {
    Job,
    Thread(thread_fallback::ThreadFallbackTarget),
}

impl CpuLimiterBackend {
    fn is_job(&self) -> bool {
        matches!(self, Self::Job)
    }
}

struct WorkerTarget {
    target: CpuLimiterTarget,
    backend: CpuLimiterBackend,
    schedule: DutySchedule,
    applied_frozen: bool,
    retiring: bool,
}

#[derive(Default)]
struct WorkerState {
    targets: BTreeMap<ProcessTargetKey, WorkerTarget>,
    covered_targets: BTreeMap<ProcessTargetKey, ProcessTargetKey>,
    failures: Vec<CpuLimiterFailure>,
    fatal_error: Option<String>,
    shutdown: bool,
}

fn should_use_thread_fallback(error: &SuspensionError) -> bool {
    matches!(error, SuspensionError::NotSupported)
}

fn release_worker_target<P>(
    target: &mut WorkerTarget,
    suspension: &mut SuspensionController<P>,
    force: bool,
) -> Result<bool, CpuLimiterTargetError>
where
    P: crate::control::suspension::SuspensionPlatform,
{
    match &mut target.backend {
        CpuLimiterBackend::Job => suspension
            .release_cpu_limiter_target(&target.target.suspension_target, force)
            .map(|_| true)
            .map_err(Into::into),
        CpuLimiterBackend::Thread(target) => target.release(),
    }
}

fn can_retain_target(current: &WorkerTarget, requested: &CpuLimiterTarget) -> bool {
    !current.retiring
        && current.target.allowed_cpu_time_percent == requested.allowed_cpu_time_percent
        && current.target.allow_cross_session_process_control
            == requested.allow_cross_session_process_control
}

pub(crate) struct CpuLimiterController {
    suspension: Arc<Mutex<SuspensionController>>,
    state: Arc<Mutex<WorkerState>>,
    command_event: CommandEvent,
    worker: Option<JoinHandle<()>>,
}

impl CpuLimiterController {
    pub(crate) fn new(suspension: Arc<Mutex<SuspensionController>>) -> Result<Self, String> {
        let state = Arc::new(Mutex::new(WorkerState::default()));
        let command_event = CommandEvent::new()?;
        let worker_state = Arc::clone(&state);
        let worker_suspension = Arc::clone(&suspension);
        let worker_event = command_event.clone();
        let run_state = Arc::clone(&worker_state);
        let run_suspension = Arc::clone(&worker_suspension);
        let worker = thread::Builder::new()
            .name("winderust-cpu-limiter".to_owned())
            .spawn(move || {
                run_worker_with_cleanup(worker_state, worker_suspension, move || {
                    run_worker(run_state, run_suspension, worker_event);
                });
            })
            .map_err(|error| format!("Failed to start CPU Limiter worker: {error}"))?;
        Ok(Self {
            suspension,
            state,
            command_event,
            worker: Some(worker),
        })
    }

    pub(crate) fn replace_targets(&mut self, targets: Vec<CpuLimiterTarget>) -> Result<(), String> {
        let now = Instant::now();
        let requested_keys = targets
            .iter()
            .map(|target| target.suspension_target.key())
            .collect::<BTreeSet<_>>();
        let mut state = self
            .state
            .lock()
            .map_err(|_| "CPU Limiter worker state is unavailable.".to_owned())?;
        let fatal_error = state.fatal_error.clone();
        let removed_keys = state
            .targets
            .keys()
            .filter(|key| !requested_keys.contains(*key))
            .cloned()
            .collect::<Vec<_>>();
        let mut suspension = self
            .suspension
            .lock()
            .map_err(|_| "CPU Limiter suspension state is unavailable.".to_owned())?;
        if let Some(error) = fatal_error {
            let keys = state.targets.keys().cloned().collect::<Vec<_>>();
            for key in keys {
                let released = state.targets.get_mut(&key).is_some_and(|target| {
                    target.retiring = true;
                    release_worker_target(target, &mut suspension, true) == Ok(true)
                });
                if released {
                    state.targets.remove(&key);
                }
            }
            return Err(error);
        }
        for key in removed_keys {
            let Some((suspension_target, result)) = state.targets.get_mut(&key).map(|target| {
                target.retiring = true;
                let suspension_target = target.target.suspension_target.clone();
                let result = release_worker_target(target, &mut suspension, true);
                (suspension_target, result)
            }) else {
                continue;
            };
            match result {
                Ok(true) => {
                    state.targets.remove(&key);
                }
                Ok(false) => {}
                Err(error) => {
                    state.failures.push(CpuLimiterFailure {
                        suspension_target,
                        error,
                    });
                }
            }
        }
        let managed_job_keys = state
            .targets
            .iter()
            .filter(|(_key, target)| target.backend.is_job())
            .map(|(key, _target)| key.clone())
            .collect::<BTreeSet<_>>();
        state
            .covered_targets
            .retain(|_target, owner| managed_job_keys.contains(owner));
        for target in targets {
            let key = target.suspension_target.key();
            if retained_covered_owner(&state, &key).is_some() {
                continue;
            }
            match target_is_covered_by_ancestor(&state, &mut suspension, &target) {
                Ok(Some(owner)) => {
                    state.covered_targets.insert(key, owner);
                    continue;
                }
                Ok(None) => {
                    state.covered_targets.remove(&key);
                }
                Err(error) => {
                    state.failures.push(CpuLimiterFailure {
                        suspension_target: target.suspension_target,
                        error: error.into(),
                    });
                    continue;
                }
            }
            if let Some(current) = state.targets.get_mut(&key) {
                if can_retain_target(current, &target) {
                    continue;
                }
            }
            if state.targets.contains_key(&key) {
                let Some((suspension_target, result)) =
                    state.targets.get_mut(&key).map(|current| {
                        current.retiring = true;
                        let suspension_target = current.target.suspension_target.clone();
                        let result = release_worker_target(current, &mut suspension, true);
                        (suspension_target, result)
                    })
                else {
                    continue;
                };
                match result {
                    Ok(true) => {
                        state.targets.remove(&key);
                    }
                    Ok(false) => continue,
                    Err(error) => {
                        state.failures.push(CpuLimiterFailure {
                            suspension_target,
                            error,
                        });
                        continue;
                    }
                }
            }
            let Some(schedule) = DutySchedule::new(now, target.allowed_cpu_time_percent) else {
                state.failures.push(CpuLimiterFailure {
                    suspension_target: target.suspension_target,
                    error: SuspensionError::Failed(
                        "Allowed CPU Time must be from 1% to 99%.".to_owned(),
                    )
                    .into(),
                });
                continue;
            };
            let backend = match suspension.set_cpu_limiter_phase(
                &target.suspension_target,
                false,
                target.allow_cross_session_process_control,
            ) {
                Ok(()) => CpuLimiterBackend::Job,
                Err(error) if should_use_thread_fallback(&error) => {
                    match thread_fallback::ThreadFallbackTarget::prepare(target.clone()) {
                        Ok(target) => CpuLimiterBackend::Thread(target),
                        Err(error) => {
                            state.failures.push(CpuLimiterFailure {
                                suspension_target: target.suspension_target,
                                error,
                            });
                            continue;
                        }
                    }
                }
                Err(error) => {
                    state.failures.push(CpuLimiterFailure {
                        suspension_target: target.suspension_target,
                        error: error.into(),
                    });
                    continue;
                }
            };
            state.targets.insert(
                key,
                WorkerTarget {
                    target,
                    backend,
                    schedule,
                    applied_frozen: false,
                    retiring: false,
                },
            );
        }
        drop(suspension);
        drop(state);
        self.command_event.signal()
    }

    pub(crate) fn drain_failures(&mut self) -> Result<Vec<CpuLimiterFailure>, String> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| "CPU Limiter worker state is unavailable.".to_owned())?;
        if let Some(error) = &state.fatal_error {
            return Err(error.clone());
        }
        Ok(std::mem::take(&mut state.failures))
    }

    pub(crate) fn managed_target_keys(&self) -> Result<BTreeSet<ProcessTargetKey>, String> {
        self.state
            .lock()
            .map(|state| effective_managed_target_keys(&state))
            .map_err(|_| "CPU Limiter worker state is unavailable.".to_owned())
    }

    pub(crate) fn fallback_target_keys(&self) -> Result<BTreeSet<ProcessTargetKey>, String> {
        self.state
            .lock()
            .map(|state| {
                state
                    .targets
                    .iter()
                    .filter(|(_key, target)| {
                        !target.retiring && matches!(target.backend, CpuLimiterBackend::Thread(_))
                    })
                    .map(|(key, _target)| key.clone())
                    .collect()
            })
            .map_err(|_| "CPU Limiter worker state is unavailable.".to_owned())
    }

    pub(crate) fn has_managed_state(&self) -> bool {
        self.state
            .lock()
            .map_or(true, |state| !state.targets.is_empty())
    }

    pub(crate) fn shutdown(&mut self) -> Result<(), String> {
        let mut errors = Vec::new();
        if let Ok(mut state) = self.state.lock() {
            if !state.shutdown {
                if let Ok(mut suspension) = self.suspension.lock() {
                    let keys = state.targets.keys().cloned().collect::<Vec<_>>();
                    for key in keys {
                        let Some(result) = state.targets.get_mut(&key).map(|target| {
                            target.retiring = true;
                            release_worker_target(target, &mut suspension, true)
                        }) else {
                            continue;
                        };
                        match result {
                            Ok(true) => {
                                state.targets.remove(&key);
                            }
                            Ok(false) => {}
                            Err(error) => errors.push(error.to_string()),
                        }
                    }
                } else {
                    errors.push("CPU Limiter suspension state is unavailable.".to_owned());
                }
                state.shutdown = true;
            }
        } else {
            errors.push("CPU Limiter worker state is unavailable.".to_owned());
        }
        if let Err(error) = self.command_event.signal() {
            errors.push(error);
        }
        if let Some(worker) = self.worker.take() {
            if worker.join().is_err() {
                errors.push("CPU Limiter worker panicked.".to_owned());
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join(" "))
        }
    }
}

fn target_is_covered_by_ancestor<P>(
    state: &WorkerState,
    suspension: &mut SuspensionController<P>,
    target: &CpuLimiterTarget,
) -> Result<Option<ProcessTargetKey>, SuspensionError>
where
    P: crate::control::suspension::SuspensionPlatform,
{
    for ancestor in state.targets.values().filter(|ancestor| {
        !ancestor.retiring
            && ancestor.backend.is_job()
            && target
                .ancestor_process_ids
                .contains(&ancestor.target.suspension_target.process.id)
    }) {
        if suspension.cpu_limiter_job_contains(
            &ancestor.target.suspension_target,
            &target.suspension_target,
            target.allow_cross_session_process_control,
        )? {
            return Ok(Some(ancestor.target.suspension_target.key()));
        }
    }
    Ok(None)
}

fn effective_managed_target_keys(state: &WorkerState) -> BTreeSet<ProcessTargetKey> {
    let mut keys = state.targets.keys().cloned().collect::<BTreeSet<_>>();
    keys.extend(
        state
            .covered_targets
            .iter()
            .filter(|(_target, owner)| {
                state
                    .targets
                    .get(*owner)
                    .is_some_and(|target| target.backend.is_job())
            })
            .map(|(target, _owner)| target.clone()),
    );
    keys
}

fn retained_covered_owner(
    state: &WorkerState,
    target: &ProcessTargetKey,
) -> Option<ProcessTargetKey> {
    state
        .covered_targets
        .get(target)
        .filter(|owner| {
            state
                .targets
                .get(*owner)
                .is_some_and(|target| target.backend.is_job())
        })
        .cloned()
}

impl Drop for CpuLimiterController {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

fn run_worker(
    state: Arc<Mutex<WorkerState>>,
    suspension: Arc<Mutex<SuspensionController>>,
    command_event: CommandEvent,
) {
    let timer = match HighResolutionTimer::new() {
        Ok(timer) => timer,
        Err(error) => {
            fail_worker(&state, &suspension, error);
            return;
        }
    };
    let mut timer_armed = false;
    loop {
        let next_deadline = {
            let mut worker_state = state
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if worker_state.shutdown {
                return;
            }
            let now = Instant::now();
            let mut failed = Vec::new();
            let mut released = Vec::new();
            let mut fallback_freezes = Vec::new();
            let mut suspension = suspension
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            for (key, target) in &mut worker_state.targets {
                if now < target.schedule.next_deadline {
                    continue;
                }
                if target.retiring {
                    match release_worker_target(target, &mut suspension, false) {
                        Ok(true) => released.push(key.clone()),
                        Ok(false) | Err(_) => target.schedule.defer(now),
                    }
                    continue;
                }
                let frozen = target.schedule.advance(now);
                if frozen == target.applied_frozen {
                    continue;
                }
                let process_id = target.target.suspension_target.process.id;
                match (&mut target.backend, frozen) {
                    (CpuLimiterBackend::Job, _) => match suspension.set_cpu_limiter_phase(
                        &target.target.suspension_target,
                        frozen,
                        target.target.allow_cross_session_process_control,
                    ) {
                        Ok(()) => target.applied_frozen = frozen,
                        Err(error) => failed.push((key.clone(), error.into())),
                    },
                    (CpuLimiterBackend::Thread(target), true) => {
                        match target.freeze_known_threads() {
                            Ok(()) => fallback_freezes.push((key.clone(), process_id)),
                            Err(error) => failed.push((key.clone(), error)),
                        }
                    }
                    (CpuLimiterBackend::Thread(fallback), false) => match fallback.thaw() {
                        Ok(()) => target.applied_frozen = false,
                        Err(error) => failed.push((key.clone(), error)),
                    },
                }
            }
            if !fallback_freezes.is_empty() {
                let process_ids = fallback_freezes
                    .iter()
                    .map(|(_key, process_id)| *process_id)
                    .collect::<BTreeSet<_>>();
                match thread_suspension::thread_ids_by_process(&process_ids) {
                    Ok(inventory) => {
                        for (key, process_id) in fallback_freezes {
                            let Some(target) = worker_state.targets.get_mut(&key) else {
                                continue;
                            };
                            let CpuLimiterBackend::Thread(fallback) = &mut target.backend else {
                                continue;
                            };
                            let thread_ids =
                                inventory.get(&process_id).cloned().unwrap_or_default();
                            match fallback.adopt_new_threads(&thread_ids) {
                                Ok(()) => target.applied_frozen = true,
                                Err(error) => failed.push((key, error)),
                            }
                        }
                    }
                    Err(error) => failed.extend(
                        fallback_freezes
                            .into_iter()
                            .map(|(key, _process_id)| (key, error.clone().into())),
                    ),
                }
            }
            for key in released {
                worker_state.targets.remove(&key);
            }
            for (key, cause) in failed {
                let Some((suspension_target, release_result)) =
                    worker_state.targets.get_mut(&key).map(|target| {
                        target.retiring = true;
                        let suspension_target = target.target.suspension_target.clone();
                        let result = release_worker_target(target, &mut suspension, true);
                        (suspension_target, result)
                    })
                else {
                    continue;
                };
                let error = match release_result {
                    Ok(true) => {
                        worker_state.targets.remove(&key);
                        cause
                    }
                    Ok(false) => {
                        if let Some(target) = worker_state.targets.get_mut(&key) {
                            target.schedule.defer(now);
                        }
                        cause
                    }
                    Err(rollback) => {
                        if let Some(target) = worker_state.targets.get_mut(&key) {
                            target.schedule.defer(now);
                        }
                        CpuLimiterTargetError::Rollback {
                            cause: Box::new(cause),
                            rollback: Box::new(rollback),
                        }
                    }
                };
                worker_state.failures.push(CpuLimiterFailure {
                    suspension_target,
                    error,
                });
            }
            worker_state
                .targets
                .values()
                .map(|target| target.schedule.next_deadline)
                .min()
        };

        if let Some(deadline) = next_deadline {
            if let Err(error) = timer.arm(deadline.saturating_duration_since(Instant::now())) {
                fail_worker(&state, &suspension, error);
                return;
            }
            timer_armed = true;
        } else if timer_armed {
            if let Err(error) = timer.cancel() {
                fail_worker(&state, &suspension, error);
                return;
            }
            timer_armed = false;
        }

        if let Err(error) = timer.wait(&command_event) {
            fail_worker(&state, &suspension, error);
            return;
        }
    }
}

fn run_worker_with_cleanup<F>(
    state: Arc<Mutex<WorkerState>>,
    suspension: Arc<Mutex<SuspensionController>>,
    worker: F,
) where
    F: FnOnce(),
{
    if catch_unwind(AssertUnwindSafe(worker)).is_err() {
        fail_worker(
            &state,
            &suspension,
            "CPU Limiter worker panicked.".to_owned(),
        );
    }
}

fn fail_worker(
    state: &Arc<Mutex<WorkerState>>,
    suspension: &Arc<Mutex<SuspensionController>>,
    message: String,
) {
    {
        let mut worker_state = state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let mut suspension_state = suspension
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let keys = worker_state.targets.keys().cloned().collect::<Vec<_>>();
        for key in keys {
            let released = worker_state.targets.get_mut(&key).is_some_and(|target| {
                target.retiring = true;
                release_worker_target(target, &mut suspension_state, true) == Ok(true)
            });
            if released {
                worker_state.targets.remove(&key);
            }
        }
        worker_state.fatal_error = Some(message);
    }
    state.clear_poison();
    suspension.clear_poison();
}

impl DutySchedule {
    fn new(now: Instant, allowed_cpu_time_percent: u8) -> Option<Self> {
        let (awake, _frozen) = phase_lengths(allowed_cpu_time_percent)?;
        Some(Self {
            cycle_started: now,
            awake,
            frozen: false,
            next_deadline: now + awake,
        })
    }

    fn advance(&mut self, now: Instant) -> bool {
        while now >= self.next_deadline {
            if self.frozen {
                self.frozen = false;
                self.cycle_started += CYCLE;
                self.next_deadline = self.cycle_started + self.awake;
            } else {
                self.frozen = true;
                self.next_deadline = self.cycle_started + CYCLE;
            }
        }
        self.frozen
    }

    fn defer(&mut self, now: Instant) {
        self.next_deadline = now + CYCLE;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::{process::ProcessIdentity, suspension::SuspensionPlatform};

    #[derive(Clone, Copy)]
    struct CoveragePlatform;

    impl SuspensionPlatform for CoveragePlatform {
        type Handle = u32;
        type Intent = ();

        fn assign(
            &mut self,
            target: &SuspensionTarget,
            _allow_cross_session_process_control: bool,
        ) -> Result<(ProcessIdentity, Self::Handle), SuspensionError> {
            Ok((
                ProcessIdentity::new(
                    target.process.id,
                    target.process.name.clone(),
                    target.process.executable_path.clone(),
                    target.process.creation_time,
                    Some(1),
                ),
                target.process.id,
            ))
        }

        fn contains_process(
            &mut self,
            _handle: &Self::Handle,
            target: &SuspensionTarget,
            _allow_cross_session_process_control: bool,
        ) -> Result<bool, SuspensionError> {
            Ok(target.process.id == 2)
        }

        fn begin_freeze(
            &mut self,
            _identity: &ProcessIdentity,
            _handle: &Self::Handle,
        ) -> Result<Self::Intent, SuspensionError> {
            Ok(())
        }

        fn set_frozen(
            &mut self,
            _handle: &Self::Handle,
            _frozen: bool,
        ) -> Result<(), SuspensionError> {
            Ok(())
        }

        fn commit_freeze(&mut self, _intent: Self::Intent) -> Result<(), SuspensionError> {
            Ok(())
        }

        fn forget(&mut self, _handle: &Self::Handle) -> Result<(), SuspensionError> {
            Ok(())
        }
    }

    fn target(process_id: u32, ancestor_process_ids: Vec<u32>) -> CpuLimiterTarget {
        CpuLimiterTarget {
            suspension_target: SuspensionTarget::automatic(
                process_id,
                format!("process-{process_id}.exe"),
                format!(r"C:\Apps\process-{process_id}.exe").into(),
                u64::from(process_id),
                Some(false),
            ),
            allowed_cpu_time_percent: 50,
            allow_cross_session_process_control: true,
            ancestor_process_ids,
        }
    }

    #[test]
    fn allowed_cpu_time_maps_to_a_fixed_hundred_millisecond_cycle() {
        assert_eq!(
            phase_lengths(1),
            Some((Duration::from_millis(1), Duration::from_millis(99)))
        );
        assert_eq!(
            phase_lengths(50),
            Some((Duration::from_millis(50), Duration::from_millis(50)))
        );
        assert_eq!(
            phase_lengths(99),
            Some((Duration::from_millis(99), Duration::from_millis(1)))
        );
        assert_eq!(phase_lengths(0), None);
        assert_eq!(phase_lengths(100), None);
    }

    #[test]
    fn late_wakes_advance_from_the_original_cycle_boundary() {
        let start = Instant::now();
        let mut schedule = DutySchedule::new(start, 25).unwrap();

        assert_eq!(schedule.next_deadline, start + Duration::from_millis(25));
        assert!(schedule.advance(start + Duration::from_millis(26)));
        assert_eq!(schedule.next_deadline, start + Duration::from_millis(100));

        assert!(schedule.advance(start + Duration::from_millis(225)));
        assert_eq!(schedule.next_deadline, start + Duration::from_millis(300));
    }

    #[test]
    fn inherited_child_does_not_get_a_second_limiter_job() {
        let owner = target(1, Vec::new());
        let owner_key = owner.suspension_target.key();
        let child = target(2, vec![1]);
        let mut suspension = SuspensionController::new(CoveragePlatform);
        suspension
            .set_cpu_limiter_phase(&owner.suspension_target, false, true)
            .unwrap();
        let mut state = WorkerState::default();
        state.targets.insert(
            owner_key.clone(),
            WorkerTarget {
                target: owner,
                backend: CpuLimiterBackend::Job,
                schedule: DutySchedule::new(Instant::now(), 50).unwrap(),
                applied_frozen: false,
                retiring: false,
            },
        );

        assert_eq!(
            target_is_covered_by_ancestor(&state, &mut suspension, &child).unwrap(),
            Some(owner_key)
        );
    }

    #[test]
    fn worker_panic_releases_targets_and_marks_the_worker_failed() {
        let target = target(42, Vec::new());
        let key = target.suspension_target.key();
        let state = Arc::new(Mutex::new(WorkerState {
            targets: BTreeMap::from([(
                key,
                WorkerTarget {
                    target,
                    backend: CpuLimiterBackend::Job,
                    schedule: DutySchedule::new(Instant::now(), 50).unwrap(),
                    applied_frozen: false,
                    retiring: false,
                },
            )]),
            ..Default::default()
        }));
        let suspension = Arc::new(Mutex::new(SuspensionController::default()));
        let panic_state = Arc::clone(&state);
        let panic_suspension = Arc::clone(&suspension);

        run_worker_with_cleanup(Arc::clone(&state), Arc::clone(&suspension), move || {
            let _state = panic_state.lock().unwrap();
            let _suspension = panic_suspension.lock().unwrap();
            panic!("boom");
        });

        assert!(!state.is_poisoned());
        assert!(!suspension.is_poisoned());
        let state = state.lock().unwrap();
        assert!(state.targets.is_empty());
        assert_eq!(
            state.fatal_error.as_deref(),
            Some("CPU Limiter worker panicked.")
        );
    }

    #[test]
    fn covered_child_remains_managed_while_ancestor_release_is_pending() {
        let owner = target(1, Vec::new());
        let owner_key = owner.suspension_target.key();
        let child_key = target(2, vec![1]).suspension_target.key();
        let state = WorkerState {
            targets: BTreeMap::from([(
                owner_key.clone(),
                WorkerTarget {
                    target: owner,
                    backend: CpuLimiterBackend::Job,
                    schedule: DutySchedule::new(Instant::now(), 50).unwrap(),
                    applied_frozen: true,
                    retiring: true,
                },
            )]),
            covered_targets: BTreeMap::from([(child_key.clone(), owner_key.clone())]),
            ..Default::default()
        };

        assert_eq!(
            effective_managed_target_keys(&state),
            BTreeSet::from([owner_key.clone(), child_key.clone()])
        );
        assert_eq!(retained_covered_owner(&state, &child_key), Some(owner_key));
    }

    #[test]
    fn only_incompatible_job_assignment_selects_thread_fallback() {
        assert!(should_use_thread_fallback(&SuspensionError::NotSupported));
        assert!(!should_use_thread_fallback(&SuspensionError::AccessDenied));
        assert!(!should_use_thread_fallback(&SuspensionError::ProcessExited));
        assert!(!should_use_thread_fallback(&SuspensionError::Unsupported));
        assert!(!should_use_thread_fallback(&SuspensionError::Failed(
            "job failure".to_owned()
        )));
    }

    #[test]
    fn retiring_target_is_released_instead_of_reactivated() {
        let requested = target(42, Vec::new());
        let mut current = WorkerTarget {
            target: requested.clone(),
            backend: CpuLimiterBackend::Job,
            schedule: DutySchedule::new(Instant::now(), 50).unwrap(),
            applied_frozen: false,
            retiring: false,
        };

        assert!(can_retain_target(&current, &requested));
        current.retiring = true;
        assert!(!can_retain_target(&current, &requested));
    }

    #[test]
    #[ignore = "creates and restores a disposable process in an incompatible foreign Job Object"]
    fn foreign_job_uses_thread_fallback_and_restores() -> Result<(), String> {
        use std::{
            os::windows::process::CommandExt,
            path::PathBuf,
            process::{Child, Command, Stdio},
        };

        use crate::{
            foreground::capture_process_action_target,
            platform::windows::{job, thread_suspension},
        };

        const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;

        struct DisposableProcess(Child);

        impl Drop for DisposableProcess {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
            }
        }

        let executable_path = std::env::var_os("SystemRoot")
            .map(PathBuf::from)
            .ok_or_else(|| "SystemRoot is unavailable.".to_owned())?
            .join(r"System32\PING.EXE");
        let spawn = |creation_flags| {
            Command::new(&executable_path)
                .args(["-t", "127.0.0.1"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .creation_flags(creation_flags)
                .spawn()
        };
        let child = spawn(CREATE_BREAKAWAY_FROM_JOB | CREATE_NO_WINDOW)
            .or_else(|_| spawn(CREATE_NO_WINDOW))
            .map_err(|error| format!("Could not start the disposable process: {error}"))?;
        let child = DisposableProcess(child);
        let deadline = Instant::now() + Duration::from_secs(2);
        let action_target = loop {
            match capture_process_action_target(child.0.id(), &executable_path, true) {
                Ok(target) => break target,
                Err(_) if Instant::now() < deadline => {
                    thread::sleep(Duration::from_millis(25));
                }
                Err(error) => return Err(error.to_string()),
            }
        };
        let suspension_target = SuspensionTarget::automatic(
            action_target.id,
            action_target.name,
            action_target.executable_path,
            action_target.creation_time,
            action_target.is_service_account,
        );
        let limiter_target = CpuLimiterTarget {
            suspension_target: suspension_target.clone(),
            allowed_cpu_time_percent: 1,
            allow_cross_session_process_control: true,
            ancestor_process_ids: Vec::new(),
        };
        let (_identity, process) = crate::control::process::open_process_for_thread_control(
            &suspension_target.process,
            true,
        )
        .map_err(|error| error.to_string())?;
        if job::process_is_in_job(&process, None) != Some(false) {
            return Err(
                "The test host prevents a disposable child from breaking away; run this ignored test outside that Job Object."
                    .to_owned(),
            );
        }
        let foreign_job = job::create_job(&format!(
            "Local\\Winderust.CpuLimiter.ForeignTest.{}.{}",
            std::process::id(),
            child.0.id()
        ))
        .map_err(|error| format!("{error:?}"))?;
        job::set_ui_restriction_for_test(&foreign_job.handle)
            .map_err(|error| format!("{error:?}"))?;
        job::assign_process(&foreign_job.handle, &process, child.0.id())
            .map_err(|error| format!("Could not assign the foreign test job: {error:?}"))?;

        let baseline = thread_suspension::capture_threads(process.raw())
            .map_err(|error| format!("{error:?}"))?
            .into_iter()
            .map(|thread| ((thread.id, thread.creation_time), thread.suspend_count))
            .collect::<BTreeMap<_, _>>();
        if baseline.is_empty() {
            return Err("The disposable process exposed no threads.".to_owned());
        }

        let suspension = Arc::new(Mutex::new(SuspensionController::default()));
        assert_eq!(
            suspension
                .lock()
                .map_err(|_| "Suspension state is unavailable.".to_owned())?
                .set_cpu_limiter_phase(&suspension_target, false, true),
            Err(SuspensionError::NotSupported)
        );

        let mut controller = CpuLimiterController::new(Arc::clone(&suspension))?;
        controller.replace_targets(vec![limiter_target])?;
        assert!(controller
            .fallback_target_keys()?
            .contains(&suspension_target.key()));

        let deadline = Instant::now() + Duration::from_secs(2);
        let mut observed_frozen = false;
        while Instant::now() < deadline {
            let current = thread_suspension::capture_threads(process.raw())
                .map_err(|error| format!("{error:?}"))?
                .into_iter()
                .map(|thread| ((thread.id, thread.creation_time), thread.suspend_count))
                .collect::<BTreeMap<_, _>>();
            observed_frozen = baseline.iter().any(|(key, baseline_count)| {
                baseline_count
                    .checked_add(1)
                    .is_some_and(|expected| current.get(key) == Some(&expected))
            });
            if observed_frozen {
                break;
            }
            thread::sleep(Duration::from_millis(2));
        }
        if !observed_frozen {
            return Err("The 1% thread fallback never entered its frozen phase.".to_owned());
        }

        controller.replace_targets(Vec::new())?;
        assert!(controller.managed_target_keys()?.is_empty());
        let restored = thread_suspension::capture_threads(process.raw())
            .map_err(|error| format!("{error:?}"))?
            .into_iter()
            .map(|thread| ((thread.id, thread.creation_time), thread.suspend_count))
            .collect::<BTreeMap<_, _>>();
        for (key, baseline_count) in baseline {
            assert_eq!(restored.get(&key), Some(&baseline_count));
        }
        controller.shutdown()
    }
}
