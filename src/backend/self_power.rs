use std::time::{Duration, Instant};

use crate::platform::windows::self_power::{
    self as windows_self_power, PowerThrottlingState, SelfPowerState, POWER_CURRENT_VERSION,
    POWER_EXECUTION_SPEED, PRIORITY_IDLE,
};

const SELF_POWER_RETRY_INTERVAL: Duration = Duration::from_secs(5);

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
    adaptive_engine: bool,
    failed_request: Option<(bool, bool)>,
    retry_at: Option<Instant>,
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
            adaptive_engine: false,
            failed_request: None,
            retry_at: None,
            shutdown: false,
        }
    }

    pub(crate) fn set_requests(
        &mut self,
        hidden_mode: bool,
        adaptive_engine: bool,
    ) -> Result<(), String> {
        if self.shutdown {
            return Ok(());
        }
        self.hidden_mode = hidden_mode;
        self.adaptive_engine = adaptive_engine;
        self.reconcile(false)
    }

    pub(crate) fn set_hidden_mode(&mut self, enabled: bool) -> Result<(), String> {
        if self.shutdown {
            return Ok(());
        }
        self.hidden_mode = enabled;
        self.reconcile(false)
    }

    pub(crate) fn set_adaptive_engine(&mut self, enabled: bool) -> Result<(), String> {
        if self.shutdown {
            return Ok(());
        }
        self.adaptive_engine = enabled;
        self.reconcile(false)
    }

    pub(crate) fn shutdown(&mut self) -> Result<(), String> {
        if self.shutdown {
            return Ok(());
        }
        self.hidden_mode = false;
        self.adaptive_engine = false;
        let result = self.reconcile(true);
        if result.is_ok() {
            self.shutdown = true;
        }
        result
    }

    fn reconcile(&mut self, force: bool) -> Result<(), String> {
        let request = (self.hidden_mode, self.adaptive_engine);
        let now = Instant::now();
        if !force
            && self.failed_request == Some(request)
            && self.retry_at.is_some_and(|retry_at| now < retry_at)
        {
            return Ok(());
        }

        let result = self.reconcile_now();
        if result.is_ok() {
            self.failed_request = None;
            self.retry_at = None;
        } else {
            self.failed_request = Some(request);
            self.retry_at = Some(now + SELF_POWER_RETRY_INTERVAL);
        }
        result
    }

    fn reconcile_now(&mut self) -> Result<(), String> {
        let enabled = self.hidden_mode || self.adaptive_engine;
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
mod tests {
    use super::*;
    use crate::platform::windows::priority_efficiency::{
        POWER_IGNORE_TIMER_RESOLUTION, PRIORITY_NORMAL,
    };

    struct FakePlatform {
        snapshot: SelfPowerState,
        query_error: bool,
        query_error_at: Option<usize>,
        query_count: usize,
        priority_error: bool,
        power_writes: usize,
        priority_writes: usize,
    }

    impl FakePlatform {
        fn new() -> Self {
            Self {
                snapshot: SelfPowerState {
                    power_throttling: power_throttling_disabled_state(),
                    priority_class: PRIORITY_NORMAL,
                },
                query_error: false,
                query_error_at: None,
                query_count: 0,
                priority_error: false,
                power_writes: 0,
                priority_writes: 0,
            }
        }
    }

    impl SelfPowerPlatform for FakePlatform {
        fn query(&mut self) -> Result<SelfPowerState, String> {
            self.query_count += 1;
            if self.query_error || self.query_error_at == Some(self.query_count) {
                Err("injected baseline read failure".to_owned())
            } else {
                Ok(self.snapshot)
            }
        }

        fn set_power_throttling(&mut self, state: PowerThrottlingState) -> Result<(), String> {
            self.power_writes += 1;
            self.snapshot.power_throttling = state;
            Ok(())
        }

        fn set_priority_class(&mut self, priority_class: u32) -> Result<(), String> {
            self.priority_writes += 1;
            if self.priority_error {
                self.priority_error = false;
                Err("injected priority failure".to_owned())
            } else {
                self.snapshot.priority_class = priority_class;
                Ok(())
            }
        }
    }

    #[test]
    fn baseline_read_failure_blocks_every_mutation() {
        let mut platform = FakePlatform::new();
        platform.query_error = true;
        let mut controller = SelfPowerController::with_platform(platform);

        assert!(controller.set_hidden_mode(true).is_err());
        assert_eq!(controller.platform.power_writes, 0);
        assert_eq!(controller.platform.priority_writes, 0);
    }

    #[test]
    fn hidden_and_adaptive_requests_compose_without_changing_timer_resolution() {
        let mut controller = SelfPowerController::with_platform(FakePlatform::new());
        let baseline = controller.platform.snapshot;

        controller.set_hidden_mode(true).unwrap();
        assert_eq!(controller.platform.snapshot.priority_class, PRIORITY_IDLE);
        assert_ne!(
            controller.platform.snapshot.power_throttling.state_mask & POWER_EXECUTION_SPEED,
            0
        );

        controller.set_adaptive_engine(true).unwrap();
        assert_eq!(
            controller.platform.snapshot.power_throttling.state_mask
                & POWER_IGNORE_TIMER_RESOLUTION,
            0
        );
        controller.set_hidden_mode(false).unwrap();
        assert_eq!(
            controller.platform.snapshot.priority_class,
            baseline.priority_class
        );

        controller.set_adaptive_engine(false).unwrap();
        assert!(same_snapshot(controller.platform.snapshot, baseline));
        assert!(controller.state.is_none());
    }

    #[test]
    fn combined_startup_request_applies_one_composed_transition() {
        let mut controller = SelfPowerController::with_platform(FakePlatform::new());

        controller.set_requests(true, true).unwrap();

        assert_eq!(controller.platform.power_writes, 1);
        assert_eq!(controller.platform.priority_writes, 1);
        assert_eq!(controller.platform.snapshot.priority_class, PRIORITY_IDLE);
        assert_eq!(
            controller.platform.snapshot.power_throttling.state_mask
                & POWER_IGNORE_TIMER_RESOLUTION,
            0
        );
    }

    #[test]
    fn failed_second_step_compensates_power_throttling() {
        let mut platform = FakePlatform::new();
        let baseline = platform.snapshot;
        platform.priority_error = true;
        let mut controller = SelfPowerController::with_platform(platform);

        assert!(controller.set_hidden_mode(true).is_err());
        assert!(same_snapshot(controller.platform.snapshot, baseline));
    }

    #[test]
    fn verification_query_failure_compensates_every_completed_write() {
        let mut platform = FakePlatform::new();
        let baseline = platform.snapshot;
        platform.query_error_at = Some(2);
        let mut controller = SelfPowerController::with_platform(platform);

        assert!(controller.set_requests(true, true).is_err());

        assert!(same_snapshot(controller.platform.snapshot, baseline));
    }

    #[test]
    fn shutdown_releases_process_lifetime_state_once() {
        let mut controller = SelfPowerController::with_platform(FakePlatform::new());
        let baseline = controller.platform.snapshot;
        controller.set_hidden_mode(true).unwrap();

        controller.shutdown().unwrap();
        controller.shutdown().unwrap();

        assert!(same_snapshot(controller.platform.snapshot, baseline));
        assert!(controller.shutdown);
    }
}
