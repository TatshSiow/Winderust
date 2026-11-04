use std::{
    sync::{
        mpsc::{self, Receiver, RecvTimeoutError},
        Arc, Condvar, Mutex, MutexGuard,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use crate::platform::windows::self_power::{
    self as windows_self_power, PowerThrottlingState, SelfPowerState, POWER_CURRENT_VERSION,
    POWER_EXECUTION_SPEED, PRIORITY_IDLE,
};

const SELF_POWER_RETRY_INTERVAL: Duration = Duration::from_secs(5);
const SELF_POWER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);

type SelfPowerReporter = Arc<dyn Fn(Result<(), String>) + Send + Sync>;

#[derive(Default)]
struct RetryWakeState {
    generation: u64,
    stopped: bool,
}

struct SelfPowerRetryShared<P: SelfPowerPlatform> {
    controller: Mutex<SelfPowerController<P>>,
    wake: Mutex<RetryWakeState>,
    changed: Condvar,
    report: SelfPowerReporter,
}

pub(crate) struct SelfPowerRequest<P: SelfPowerPlatform = WindowsSelfPowerPlatform> {
    shared: Arc<SelfPowerRetryShared<P>>,
}

impl<P: SelfPowerPlatform> SelfPowerRequest<P> {
    pub(crate) fn set_hidden_mode(&self, hidden: bool) {
        if lock_unpoisoned(&self.shared.wake).stopped {
            return;
        }
        {
            let mut controller = lock_unpoisoned(&self.shared.controller);
            if lock_unpoisoned(&self.shared.wake).stopped {
                return;
            }
            let result = controller.set_hidden_mode(hidden);
            (self.shared.report)(result);
        }
        let mut wake = lock_unpoisoned(&self.shared.wake);
        wake.generation = wake.generation.wrapping_add(1);
        self.shared.changed.notify_one();
    }
}

pub(crate) struct SelfPowerRetryWatcher<
    P: SelfPowerPlatform + Send + 'static = WindowsSelfPowerPlatform,
> {
    request: SelfPowerRequest<P>,
    thread: Option<JoinHandle<()>>,
    completion: Receiver<Result<(), String>>,
    shutdown_timeout: Duration,
    shutdown_result: Option<Result<(), String>>,
}

impl SelfPowerRetryWatcher {
    pub(crate) fn start(report: SelfPowerReporter) -> Result<Self, String> {
        Self::with_controller(
            SelfPowerController::default(),
            report,
            SELF_POWER_SHUTDOWN_TIMEOUT,
        )
    }
}

impl<P: SelfPowerPlatform + Send + 'static> SelfPowerRetryWatcher<P> {
    fn with_controller(
        controller: SelfPowerController<P>,
        report: SelfPowerReporter,
        shutdown_timeout: Duration,
    ) -> Result<Self, String> {
        let shared = Arc::new(SelfPowerRetryShared {
            controller: Mutex::new(controller),
            wake: Mutex::new(RetryWakeState::default()),
            changed: Condvar::new(),
            report,
        });
        let worker_shared = Arc::clone(&shared);
        let (completed, completion) = mpsc::channel();
        let thread = thread::Builder::new()
            .name("winderust-self-power".to_owned())
            .spawn(move || {
                let result = run_self_power_retries(&worker_shared);
                let _ = completed.send(result);
            })
            .map_err(|error| {
                format!("Winderust self-power retry worker failed to start: {error}")
            })?;
        Ok(Self {
            request: SelfPowerRequest { shared },
            thread: Some(thread),
            completion,
            shutdown_timeout,
            shutdown_result: None,
        })
    }

    pub(crate) fn request(&self) -> SelfPowerRequest<P> {
        SelfPowerRequest {
            shared: Arc::clone(&self.request.shared),
        }
    }

    pub(crate) fn shutdown(&mut self) -> Result<(), String> {
        if let Some(result) = &self.shutdown_result {
            return result.clone();
        }
        {
            let mut wake = lock_unpoisoned(&self.request.shared.wake);
            wake.stopped = true;
            self.request.shared.changed.notify_one();
        }
        let deadline = Instant::now() + self.shutdown_timeout;
        let mut result = match self.completion.recv_timeout(self.shutdown_timeout) {
            Ok(result) => result,
            Err(RecvTimeoutError::Timeout) => Err(
                "Winderust self-power restoration timed out; completion is unconfirmed.".to_owned(),
            ),
            Err(RecvTimeoutError::Disconnected) => Err(
                "Winderust self-power retry worker stopped without confirming restoration."
                    .to_owned(),
            ),
        };
        if let Some(thread) = self.thread.take() {
            // Completion is sent immediately before return; join only after the thread has exited.
            while !thread.is_finished() && Instant::now() < deadline {
                thread::sleep(Duration::from_millis(1));
            }
            if thread.is_finished() && thread.join().is_err() {
                result = Err(
                    "Winderust self-power retry worker panicked; restoration is unconfirmed."
                        .to_owned(),
                );
            }
            // A stalled native call keeps the controller/baseline alive on its detached thread.
        }
        self.shutdown_result = Some(result.clone());
        result
    }
}

impl<P: SelfPowerPlatform + Send + 'static> Drop for SelfPowerRetryWatcher<P> {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

fn run_self_power_retries<P: SelfPowerPlatform>(
    shared: &SelfPowerRetryShared<P>,
) -> Result<(), String> {
    loop {
        let generation = {
            let wake = lock_unpoisoned(&shared.wake);
            if wake.stopped {
                break;
            }
            wake.generation
        };
        let retry_delay = {
            let mut controller = lock_unpoisoned(&shared.controller);
            let now = Instant::now();
            if controller.retry_at.is_some_and(|deadline| now >= deadline)
                && !lock_unpoisoned(&shared.wake).stopped
            {
                let result = controller.reconcile();
                (shared.report)(result);
            }
            controller
                .retry_at
                .map(|deadline| deadline.saturating_duration_since(Instant::now()))
        };
        let wake = lock_unpoisoned(&shared.wake);
        if wake.stopped {
            break;
        }
        if wake.generation != generation {
            continue;
        }
        if let Some(delay) = retry_delay {
            drop(
                shared
                    .changed
                    .wait_timeout(wake, delay)
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
            );
        } else {
            drop(
                shared
                    .changed
                    .wait(wake)
                    .unwrap_or_else(std::sync::PoisonError::into_inner),
            );
        }
    }
    let mut controller = lock_unpoisoned(&shared.controller);
    let result = controller.shutdown();
    (shared.report)(result.clone());
    result
}

fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[derive(Clone, Copy)]
struct ManagedSelfPowerState {
    baseline: SelfPowerState,
    expected: SelfPowerState,
    preserve_baseline_after_failure: bool,
}

pub(crate) trait SelfPowerPlatform {
    fn query(&mut self) -> Result<SelfPowerState, String>;
    fn set_power_throttling(&mut self, state: PowerThrottlingState) -> Result<(), String>;
    fn set_priority_class(&mut self, priority_class: u32) -> Result<(), String>;
}

#[derive(Default)]
pub(crate) struct WindowsSelfPowerPlatform;

impl SelfPowerPlatform for WindowsSelfPowerPlatform {
    fn query(&mut self) -> Result<SelfPowerState, String> {
        windows_self_power::query()
    }

    fn set_power_throttling(&mut self, state: PowerThrottlingState) -> Result<(), String> {
        windows_self_power::set_power_throttling(state)
    }

    fn set_priority_class(&mut self, priority_class: u32) -> Result<(), String> {
        windows_self_power::set_priority_class(priority_class)
    }
}

pub(crate) struct SelfPowerController<P: SelfPowerPlatform = WindowsSelfPowerPlatform> {
    platform: P,
    state: Option<ManagedSelfPowerState>,
    hidden_mode: bool,
    retry_at: Option<Instant>,
    retry_interval: Duration,
    shutdown: bool,
}

impl Default for SelfPowerController {
    fn default() -> Self {
        Self::with_platform(WindowsSelfPowerPlatform)
    }
}

impl<P: SelfPowerPlatform> SelfPowerController<P> {
    fn with_platform(platform: P) -> Self {
        Self {
            platform,
            state: None,
            hidden_mode: false,
            retry_at: None,
            retry_interval: SELF_POWER_RETRY_INTERVAL,
            shutdown: false,
        }
    }

    pub(crate) fn set_hidden_mode(&mut self, enabled: bool) -> Result<(), String> {
        if self.shutdown {
            return Ok(());
        }
        self.hidden_mode = enabled;
        self.reconcile()
    }

    pub(crate) fn shutdown(&mut self) -> Result<(), String> {
        if self.shutdown {
            return Ok(());
        }
        self.hidden_mode = false;
        let result = self.reconcile();
        if result.is_ok() {
            self.shutdown = true;
        }
        result
    }

    fn reconcile(&mut self) -> Result<(), String> {
        let result = self.reconcile_now();
        if result.is_ok() {
            self.retry_at = None;
        } else {
            self.retry_at = Some(Instant::now() + self.retry_interval);
        }
        result
    }

    fn reconcile_now(&mut self) -> Result<(), String> {
        // Foreground software rendering must not inherit an EcoQoS request.
        let enabled = self.hidden_mode;
        let current = if self.state.is_none() {
            if !enabled {
                return Ok(());
            }
            let baseline = self.platform.query()?;
            self.state = Some(ManagedSelfPowerState {
                baseline,
                expected: baseline,
                preserve_baseline_after_failure: false,
            });
            baseline
        } else {
            self.platform.query()?
        };
        let state = self
            .state
            .as_mut()
            .ok_or_else(|| "Winderust self-power state disappeared.".to_owned())?;
        if !same_snapshot(current, state.expected) {
            if !state.preserve_baseline_after_failure {
                state.baseline = current;
            }
            state.expected = current;
        }
        state.preserve_baseline_after_failure = false;

        let desired = SelfPowerState {
            power_throttling: if enabled {
                power_throttling_enabled_state(state.baseline.power_throttling)
            } else {
                state.baseline.power_throttling
            },
            priority_class: if self.hidden_mode {
                PRIORITY_IDLE
            } else {
                state.baseline.priority_class
            },
        };

        if let Err(error) = apply_snapshot(&mut self.platform, current, desired) {
            if let Ok(observed) = self.platform.query() {
                state.expected = observed;
            }
            state.preserve_baseline_after_failure = true;
            return Err(error);
        }
        state.expected = desired;
        if !enabled {
            self.state = None;
        }
        Ok(())
    }
}

fn apply_snapshot<P: SelfPowerPlatform>(
    platform: &mut P,
    current: SelfPowerState,
    desired: SelfPowerState,
) -> Result<(), String> {
    let power_changed = !same_power_state(current.power_throttling, desired.power_throttling);
    let priority_changed = current.priority_class != desired.priority_class;
    if power_changed {
        platform.set_power_throttling(desired.power_throttling)?;
    }
    if priority_changed {
        if let Err(error) = platform.set_priority_class(desired.priority_class) {
            let compensation =
                power_changed.then(|| platform.set_power_throttling(current.power_throttling));
            return Err(compensated_error(error, compensation));
        }
    }

    let observed = match platform.query() {
        Ok(observed) => observed,
        Err(error) => {
            return Err(compensate_snapshot(
                platform,
                current,
                priority_changed,
                power_changed,
                format!("Winderust self-power verification failed: {error}"),
            ));
        }
    };
    if same_snapshot(observed, desired) {
        return Ok(());
    }

    Err(compensate_snapshot(
        platform,
        current,
        observed.priority_class != current.priority_class,
        !same_power_state(observed.power_throttling, current.power_throttling),
        "Winderust self-power verification returned an unexpected state.".to_owned(),
    ))
}

fn compensate_snapshot<P: SelfPowerPlatform>(
    platform: &mut P,
    original: SelfPowerState,
    restore_priority: bool,
    restore_power: bool,
    operation_error: String,
) -> String {
    let priority_compensation =
        restore_priority.then(|| platform.set_priority_class(original.priority_class));
    let power_compensation =
        restore_power.then(|| platform.set_power_throttling(original.power_throttling));
    let compensation_error = [priority_compensation, power_compensation]
        .into_iter()
        .flatten()
        .filter_map(Result::err)
        .collect::<Vec<_>>()
        .join(" ");
    let mut error = operation_error;
    if !compensation_error.is_empty() {
        error.push_str(&format!(" Compensation also failed: {compensation_error}"));
    }
    error
}

fn compensated_error(operation_error: String, compensation: Option<Result<(), String>>) -> String {
    match compensation.and_then(Result::err) {
        Some(compensation_error) => {
            format!(
                "{operation_error} Restoring self-power state also failed: {compensation_error}"
            )
        }
        None => operation_error,
    }
}

fn same_snapshot(left: SelfPowerState, right: SelfPowerState) -> bool {
    left.priority_class == right.priority_class
        && same_power_state(left.power_throttling, right.power_throttling)
}

fn same_power_state(left: PowerThrottlingState, right: PowerThrottlingState) -> bool {
    left == right
}

#[cfg(test)]
fn power_throttling_disabled_state() -> PowerThrottlingState {
    PowerThrottlingState {
        version: POWER_CURRENT_VERSION,
        control_mask: POWER_EXECUTION_SPEED,
        state_mask: 0,
    }
}

fn power_throttling_enabled_state(previous: PowerThrottlingState) -> PowerThrottlingState {
    let mut state = previous;
    state.version = POWER_CURRENT_VERSION;
    state.control_mask |= POWER_EXECUTION_SPEED;
    state.state_mask |= POWER_EXECUTION_SPEED;
    state
}

#[cfg(test)]
mod tests;
