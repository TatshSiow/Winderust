use super::*;
use crate::platform::windows::priority_efficiency::{
    POWER_IGNORE_TIMER_RESOLUTION, PRIORITY_NORMAL,
};
use std::sync::mpsc::{channel, RecvTimeoutError};

#[derive(Clone)]
struct SharedPlatform {
    platform: Arc<Mutex<FakePlatform>>,
    block_query: Arc<Mutex<Option<Receiver<()>>>>,
}

impl SharedPlatform {
    fn new() -> Self {
        Self {
            platform: Arc::new(Mutex::new(FakePlatform::new())),
            block_query: Arc::new(Mutex::new(None)),
        }
    }
}

impl SelfPowerPlatform for SharedPlatform {
    fn query(&mut self) -> Result<SelfPowerState, String> {
        let gate = lock_unpoisoned(&self.block_query).take();
        if let Some(gate) = gate {
            gate.recv().unwrap();
        }
        lock_unpoisoned(&self.platform).query()
    }

    fn set_power_throttling(&mut self, state: PowerThrottlingState) -> Result<(), String> {
        lock_unpoisoned(&self.platform).set_power_throttling(state)
    }

    fn set_priority_class(&mut self, priority_class: u32) -> Result<(), String> {
        lock_unpoisoned(&self.platform).set_priority_class(priority_class)
    }
}

fn retry_watcher(
    platform: SharedPlatform,
    retry_interval: Duration,
    shutdown_timeout: Duration,
) -> (
    SelfPowerRetryWatcher<SharedPlatform>,
    Receiver<Result<(), String>>,
) {
    let (reported, reports) = channel();
    let mut controller = SelfPowerController::with_platform(platform);
    controller.retry_interval = retry_interval;
    let watcher = SelfPowerRetryWatcher::with_controller(
        controller,
        Arc::new(move |result| {
            let _ = reported.send(result);
        }),
        shutdown_timeout,
    )
    .unwrap();
    (watcher, reports)
}

#[test]
fn visible_restore_retries_without_visibility_settings_or_automation_work() {
    let platform = SharedPlatform::new();
    let baseline = {
        let mut state = lock_unpoisoned(&platform.platform);
        state.snapshot.power_throttling.control_mask |= POWER_IGNORE_TIMER_RESOLUTION;
        state.snapshot.power_throttling.state_mask |= POWER_IGNORE_TIMER_RESOLUTION;
        state.snapshot
    };
    let (mut watcher, reports) = retry_watcher(
        platform.clone(),
        Duration::from_millis(30),
        Duration::from_secs(1),
    );
    let request = watcher.request();
    request.set_hidden_mode(true);
    assert!(reports
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .is_ok());
    lock_unpoisoned(&platform.platform).priority_error = true;

    request.set_hidden_mode(false);
    assert!(reports
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .is_err());
    assert_eq!(
        lock_unpoisoned(&platform.platform).snapshot.priority_class,
        PRIORITY_IDLE
    );

    assert!(reports
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .is_ok());
    assert!(same_snapshot(
        lock_unpoisoned(&platform.platform).snapshot,
        baseline
    ));
    let query_count = lock_unpoisoned(&platform.platform).query_count;
    assert_eq!(
        reports.recv_timeout(Duration::from_millis(90)),
        Err(RecvTimeoutError::Timeout)
    );
    assert_eq!(lock_unpoisoned(&platform.platform).query_count, query_count);
    watcher.shutdown().unwrap();
}

#[test]
fn repeated_restore_failures_wait_for_retry_cadence_and_preserve_baseline() {
    let platform = SharedPlatform::new();
    let baseline = lock_unpoisoned(&platform.platform).snapshot;
    let (mut watcher, reports) = retry_watcher(
        platform.clone(),
        Duration::from_millis(100),
        Duration::from_secs(1),
    );
    let request = watcher.request();
    request.set_hidden_mode(true);
    assert!(reports
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .is_ok());
    lock_unpoisoned(&platform.platform).query_error = true;
    request.set_hidden_mode(false);
    assert!(reports
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .is_err());

    for _ in 0..2 {
        let query_count = lock_unpoisoned(&platform.platform).query_count;
        assert_eq!(
            reports.recv_timeout(Duration::from_millis(30)),
            Err(RecvTimeoutError::Timeout)
        );
        assert_eq!(lock_unpoisoned(&platform.platform).query_count, query_count);
        assert!(reports
            .recv_timeout(Duration::from_secs(1))
            .unwrap()
            .is_err());
    }
    lock_unpoisoned(&platform.platform).query_error = false;
    assert!(reports
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .is_ok());
    assert!(same_snapshot(
        lock_unpoisoned(&platform.platform).snapshot,
        baseline
    ));
    watcher.shutdown().unwrap();
}

#[test]
fn retry_shutdown_restores_once_and_rejects_late_visibility_requests() {
    let platform = SharedPlatform::new();
    let baseline = lock_unpoisoned(&platform.platform).snapshot;
    let (mut watcher, _) = retry_watcher(
        platform.clone(),
        Duration::from_secs(5),
        Duration::from_secs(1),
    );
    let request = watcher.request();
    request.set_hidden_mode(true);
    watcher.shutdown().unwrap();
    watcher.shutdown().unwrap();
    let writes = lock_unpoisoned(&platform.platform).priority_writes;
    request.set_hidden_mode(true);
    assert!(same_snapshot(
        lock_unpoisoned(&platform.platform).snapshot,
        baseline
    ));
    assert_eq!(lock_unpoisoned(&platform.platform).priority_writes, writes);
    assert!(watcher.thread.is_none());
}

#[test]
fn visibility_request_waiting_for_controller_cannot_mutate_after_stop() {
    let platform = SharedPlatform::new();
    let baseline = lock_unpoisoned(&platform.platform).snapshot;
    let (mut watcher, _) = retry_watcher(
        platform.clone(),
        Duration::from_secs(5),
        Duration::from_secs(1),
    );
    let controller = lock_unpoisoned(&watcher.request.shared.controller);
    let request = watcher.request();
    let (started, waiting) = channel();
    let visibility = thread::spawn(move || {
        started.send(()).unwrap();
        request.set_hidden_mode(true);
    });
    waiting.recv_timeout(Duration::from_secs(1)).unwrap();
    thread::sleep(Duration::from_millis(10));
    lock_unpoisoned(&watcher.request.shared.wake).stopped = true;
    drop(controller);

    visibility.join().unwrap();
    watcher.shutdown().unwrap();
    let observed = lock_unpoisoned(&platform.platform);
    assert_eq!(observed.snapshot, baseline);
    assert_eq!(observed.power_writes, 0);
    assert_eq!(observed.priority_writes, 0);
}

#[test]
fn retry_shutdown_reports_restore_failure_and_keeps_original_baseline() {
    let platform = SharedPlatform::new();
    let baseline = lock_unpoisoned(&platform.platform).snapshot;
    let (mut watcher, _) = retry_watcher(
        platform.clone(),
        Duration::from_secs(5),
        Duration::from_secs(1),
    );
    watcher.request().set_hidden_mode(true);
    lock_unpoisoned(&platform.platform).priority_error = true;
    assert!(watcher
        .shutdown()
        .unwrap_err()
        .contains("injected priority failure"));
    let controller = lock_unpoisoned(&watcher.request.shared.controller);
    assert_eq!(controller.state.unwrap().baseline, baseline);
}

#[test]
fn retry_shutdown_does_not_wait_forever_for_an_inflight_platform_call() {
    let platform = SharedPlatform::new();
    let baseline = lock_unpoisoned(&platform.platform).snapshot;
    let (mut watcher, reports) = retry_watcher(
        platform.clone(),
        Duration::from_millis(20),
        Duration::from_millis(40),
    );
    let request = watcher.request();
    request.set_hidden_mode(true);
    assert!(reports
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .is_ok());
    lock_unpoisoned(&platform.platform).query_error = true;
    request.set_hidden_mode(false);
    assert!(reports
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .is_err());
    let (release, blocked) = channel();
    *lock_unpoisoned(&platform.block_query) = Some(blocked);
    let deadline = Instant::now() + Duration::from_secs(1);
    while lock_unpoisoned(&platform.block_query).is_some() {
        assert!(
            Instant::now() < deadline,
            "retry should enter the injected query"
        );
        thread::sleep(Duration::from_millis(1));
    }

    let started = Instant::now();
    assert!(watcher.shutdown().unwrap_err().contains("timed out"));
    assert!(started.elapsed() < Duration::from_millis(250));
    request.set_hidden_mode(true);
    lock_unpoisoned(&platform.platform).query_error = false;
    release.send(()).unwrap();
    assert!(reports
        .recv_timeout(Duration::from_secs(1))
        .unwrap()
        .is_ok());
    assert!(same_snapshot(
        lock_unpoisoned(&platform.platform).snapshot,
        baseline
    ));
}

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
fn visible_window_leaves_process_power_state_untouched() {
    let mut controller = SelfPowerController::with_platform(FakePlatform::new());
    let baseline = controller.platform.snapshot;
    controller.set_hidden_mode(false).unwrap();
    assert!(same_snapshot(controller.platform.snapshot, baseline));
    assert_eq!(controller.platform.power_writes, 0);
    assert_eq!(controller.platform.priority_writes, 0);
    assert!(controller.state.is_none());
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
fn showing_window_restores_priority_and_throttling_without_changing_timer_resolution() {
    let mut controller = SelfPowerController::with_platform(FakePlatform::new());
    let baseline = controller.platform.snapshot;

    controller.set_hidden_mode(true).unwrap();
    assert_eq!(controller.platform.snapshot.priority_class, PRIORITY_IDLE);
    assert_ne!(
        controller.platform.snapshot.power_throttling.state_mask & POWER_EXECUTION_SPEED,
        0
    );

    assert_eq!(
        controller.platform.snapshot.power_throttling.state_mask & POWER_IGNORE_TIMER_RESOLUTION,
        0
    );
    controller.set_hidden_mode(false).unwrap();
    assert_eq!(
        controller.platform.snapshot.priority_class,
        baseline.priority_class
    );

    assert!(same_snapshot(controller.platform.snapshot, baseline));
    assert!(controller.state.is_none());
}

#[test]
fn hidden_startup_applies_one_verified_transition() {
    let mut controller = SelfPowerController::with_platform(FakePlatform::new());

    controller.set_hidden_mode(true).unwrap();

    assert_eq!(controller.platform.power_writes, 1);
    assert_eq!(controller.platform.priority_writes, 1);
    assert_eq!(controller.platform.snapshot.priority_class, PRIORITY_IDLE);
    assert_eq!(
        controller.platform.snapshot.power_throttling.state_mask & POWER_IGNORE_TIMER_RESOLUTION,
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

    assert!(controller.set_hidden_mode(true).is_err());

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
