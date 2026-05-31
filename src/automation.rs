use std::{
    sync::{Arc, Condvar, Mutex},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use crate::{
    activity::{input_hook, IdleDetector, InputHookEvents},
    config::Settings,
    cpu::{CpuUsageMonitor, CpuUsageSnapshot},
    foreground::ForegroundDetector,
    power::PowerPlanManager,
    rules::{DecisionEngine, DecisionInput, DecisionOutcome},
    scheduler::{CpuUsageScheduler, Scheduler},
    tray,
};

const ACTIVE_PLAN_REFRESH_INTERVAL: Duration = Duration::from_secs(10);
const CPU_USAGE_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const SWITCH_RETRY_INTERVAL: Duration = Duration::from_secs(15);
const HIDDEN_POLL_INTERVAL: Duration = Duration::from_millis(250);

pub struct BackgroundAutomation {
    shared: Arc<SharedAutomationState>,
    thread: Option<JoinHandle<()>>,
}

struct SharedAutomationState {
    state: Mutex<AutomationWorkerState>,
    changed: Condvar,
}

struct AutomationWorkerState {
    settings: Settings,
    stop_requested: bool,
}

impl BackgroundAutomation {
    pub fn start(settings: &Settings) -> Self {
        let shared = Arc::new(SharedAutomationState {
            state: Mutex::new(AutomationWorkerState {
                settings: settings.clone(),
                stop_requested: false,
            }),
            changed: Condvar::new(),
        });
        let thread_shared = Arc::clone(&shared);
        let thread = thread::spawn(move || run_background_automation(thread_shared));

        Self {
            shared,
            thread: Some(thread),
        }
    }

    pub fn update_settings(&self, settings: &Settings) {
        if let Ok(mut state) = self.shared.state.lock() {
            state.settings = settings.clone();
            self.shared.changed.notify_one();
        }
    }
}

impl Drop for BackgroundAutomation {
    fn drop(&mut self) {
        if let Ok(mut state) = self.shared.state.lock() {
            state.stop_requested = true;
            self.shared.changed.notify_one();
        }

        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn run_background_automation(shared: Arc<SharedAutomationState>) {
    let mut runner = HiddenAutomationRunner::default();
    let mut next_check = Instant::now();

    loop {
        let settings = match settings_snapshot(&shared) {
            Some(settings) => settings,
            None => break,
        };

        let wait_for = if tray::is_hidden_to_tray() {
            let input_events = input_hook::take_pending_events();
            let should_check_now =
                Instant::now() >= next_check || input_hook_should_check(&settings, input_events);

            if should_check_now {
                runner.run_check(&settings);
                next_check = Instant::now()
                    + Duration::from_millis(settings.general.check_interval_ms.max(250));
            }

            next_check
                .saturating_duration_since(Instant::now())
                .min(HIDDEN_POLL_INTERVAL)
        } else {
            next_check = Instant::now();
            HIDDEN_POLL_INTERVAL
        };

        if wait_or_stop(&shared, wait_for) {
            break;
        }
    }
}

fn settings_snapshot(shared: &SharedAutomationState) -> Option<Settings> {
    shared
        .state
        .lock()
        .ok()
        .and_then(|state| (!state.stop_requested).then(|| state.settings.clone()))
}

fn wait_or_stop(shared: &SharedAutomationState, wait_for: Duration) -> bool {
    let Ok(state) = shared.state.lock() else {
        return true;
    };
    if state.stop_requested {
        return true;
    }

    match shared.changed.wait_timeout(state, wait_for) {
        Ok((state, _)) => state.stop_requested,
        Err(_) => true,
    }
}

fn input_hook_should_check(settings: &Settings, events: InputHookEvents) -> bool {
    settings.general.enabled
        && settings.activity_mode.enabled
        && ((events.keyboard && settings.activity_mode.input_detection.keyboard)
            || (events.mouse && settings.activity_mode.input_detection.mouse))
}

#[derive(Default)]
struct HiddenAutomationRunner {
    current_guid: Option<String>,
    next_active_plan_refresh: Option<Instant>,
    last_switch_attempt: Option<(String, Instant)>,
    power: PowerPlanManager,
    cpu_usage: CpuUsageSnapshot,
    next_cpu_usage_refresh: Option<Instant>,
    cpu_monitor: CpuUsageMonitor,
    idle_detector: IdleDetector,
    foreground_detector: ForegroundDetector,
    scheduler: Scheduler,
    cpu_usage_scheduler: CpuUsageScheduler,
    decision_engine: DecisionEngine,
}

impl HiddenAutomationRunner {
    fn run_check(&mut self, settings: &Settings) {
        let should_refresh_active_plan = self
            .next_active_plan_refresh
            .map_or(true, |refresh_at| Instant::now() >= refresh_at);
        if should_refresh_active_plan {
            self.refresh_active_plan();
        }

        let activity = self.idle_detector.snapshot(Duration::from_secs(
            settings.activity_mode.idle_timeout_seconds,
        ));
        if self
            .next_cpu_usage_refresh
            .map_or(true, |refresh_at| Instant::now() >= refresh_at)
        {
            self.cpu_usage = self.cpu_monitor.sample();
            self.next_cpu_usage_refresh = Some(Instant::now() + CPU_USAGE_REFRESH_INTERVAL);
        }
        let foreground_app = self.foreground_detector.process_name();
        let schedule = self.scheduler.current_decision(&settings.schedule_mode);
        let cpu_usage_decision = self
            .cpu_usage_scheduler
            .current_decision(&settings.cpu_usage_mode, self.cpu_usage.percent);
        let decision = self.decision_engine.decide(
            settings,
            DecisionInput {
                activity_state: activity.state,
                foreground_app,
                schedule,
                cpu_usage: cpu_usage_decision,
            },
        );

        self.apply_decision(&decision);
    }

    fn refresh_active_plan(&mut self) {
        self.next_active_plan_refresh = Some(Instant::now() + ACTIVE_PLAN_REFRESH_INTERVAL);

        if let Ok(Some(active)) = self.power.active_plan() {
            self.current_guid = Some(active.guid);
        }
    }

    fn apply_decision(&mut self, decision: &DecisionOutcome) {
        let Some(target_guid) = decision.target_guid.as_deref() else {
            return;
        };

        let already_active = self
            .current_guid
            .as_deref()
            .is_some_and(|guid| guid.eq_ignore_ascii_case(target_guid));
        if already_active {
            return;
        }

        if let Some((last_guid, attempted_at)) = &self.last_switch_attempt {
            if last_guid.eq_ignore_ascii_case(target_guid)
                && attempted_at.elapsed() < SWITCH_RETRY_INTERVAL
            {
                return;
            }
        }

        self.last_switch_attempt = Some((target_guid.to_owned(), Instant::now()));

        if self.power.set_active(target_guid).is_ok() {
            self.current_guid = Some(target_guid.to_owned());
        }
    }
}
