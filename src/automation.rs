use std::{
    sync::{Arc, Condvar, Mutex},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use crate::{
    activity::{input_hook, IdleDetector, InputHookEvents},
    affinity::{CpuAffinityManager, CpuAffinitySnapshot},
    config::Settings,
    cpu::{CpuUsageMonitor, CpuUsageSnapshot},
    ecoqos::{EcoQosManager, EcoQosSnapshot},
    foreground::ForegroundDetector,
    power::PowerPlanManager,
    power_source,
    rules::{DecisionEngine, DecisionInput, DecisionOutcome},
    scheduler::{CpuUsageScheduler, Scheduler},
    suspension::{AppSuspensionManager, AppSuspensionSnapshot},
    tray,
};

const ACTIVE_PLAN_REFRESH_INTERVAL: Duration = Duration::from_secs(10);
const CPU_USAGE_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const ECO_QOS_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const APP_SUSPENSION_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const CPU_AFFINITY_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const VISIBLE_AUTOMATION_REFRESH_INTERVAL: Duration = Duration::from_secs(3);
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
    eco_qos_status: EcoQosSnapshot,
    app_suspension_status: AppSuspensionSnapshot,
    cpu_affinity_status: CpuAffinitySnapshot,
    app_suspension_freeze_requests: Vec<String>,
    stop_requested: bool,
}

impl BackgroundAutomation {
    pub fn start(settings: &Settings) -> Self {
        let shared = Arc::new(SharedAutomationState {
            state: Mutex::new(AutomationWorkerState {
                settings: settings.clone(),
                eco_qos_status: EcoQosSnapshot::default(),
                app_suspension_status: AppSuspensionSnapshot::default(),
                cpu_affinity_status: CpuAffinitySnapshot::default(),
                app_suspension_freeze_requests: Vec::new(),
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
            if state.settings == *settings {
                return;
            }
            state.settings = settings.clone();
            self.shared.changed.notify_one();
        }
    }

    pub fn eco_qos_status(&self) -> EcoQosSnapshot {
        self.shared
            .state
            .lock()
            .map(|state| state.eco_qos_status.clone())
            .unwrap_or_default()
    }

    pub fn app_suspension_status(&self) -> AppSuspensionSnapshot {
        self.shared
            .state
            .lock()
            .map(|state| state.app_suspension_status.clone())
            .unwrap_or_default()
    }

    pub fn cpu_affinity_status(&self) -> CpuAffinitySnapshot {
        self.shared
            .state
            .lock()
            .map(|state| state.cpu_affinity_status.clone())
            .unwrap_or_default()
    }

    pub fn request_app_suspension_freeze(&self, process_name: &str) {
        let process_name = process_name.trim().to_ascii_lowercase();
        if process_name.is_empty() {
            return;
        }

        if let Ok(mut state) = self.shared.state.lock() {
            state.app_suspension_freeze_requests.push(process_name);
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
    let mut next_eco_qos_refresh = Instant::now();
    let mut next_app_suspension_refresh = Instant::now();
    let mut next_cpu_affinity_refresh = Instant::now();

    loop {
        let snapshot = match automation_snapshot(&shared) {
            Some(snapshot) => snapshot,
            None => break,
        };
        let settings = snapshot.settings;
        let app_suspension_freeze_requests = snapshot.app_suspension_freeze_requests;
        let hidden_to_tray = tray::is_hidden_to_tray();
        let eco_qos_refresh_interval =
            automation_refresh_interval(hidden_to_tray, ECO_QOS_REFRESH_INTERVAL);
        let app_suspension_refresh_interval =
            automation_refresh_interval(hidden_to_tray, APP_SUSPENSION_REFRESH_INTERVAL);
        let cpu_affinity_refresh_interval =
            automation_refresh_interval(hidden_to_tray, CPU_AFFINITY_REFRESH_INTERVAL);

        if !app_suspension_freeze_requests.is_empty() {
            next_app_suspension_refresh = Instant::now();
        }

        if Instant::now() >= next_eco_qos_refresh {
            let eco_qos_status = runner.run_eco_qos_update(&settings);
            update_eco_qos_status(&shared, eco_qos_status);
            next_eco_qos_refresh = Instant::now() + eco_qos_refresh_interval;
        }
        if Instant::now() >= next_app_suspension_refresh {
            let app_suspension_status =
                runner.run_app_suspension_update(&settings, &app_suspension_freeze_requests);
            update_app_suspension_status(&shared, app_suspension_status);
            next_app_suspension_refresh = Instant::now() + app_suspension_refresh_interval;
        }
        if Instant::now() >= next_cpu_affinity_refresh {
            let cpu_affinity_status = runner.run_cpu_affinity_update(&settings);
            update_cpu_affinity_status(&shared, cpu_affinity_status);
            next_cpu_affinity_refresh = Instant::now() + cpu_affinity_refresh_interval;
        }

        let wait_for = if hidden_to_tray {
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
                .min(next_eco_qos_refresh.saturating_duration_since(Instant::now()))
                .min(next_app_suspension_refresh.saturating_duration_since(Instant::now()))
                .min(next_cpu_affinity_refresh.saturating_duration_since(Instant::now()))
        } else {
            next_check = Instant::now();
            next_eco_qos_refresh
                .saturating_duration_since(Instant::now())
                .min(eco_qos_refresh_interval)
                .min(next_app_suspension_refresh.saturating_duration_since(Instant::now()))
                .min(app_suspension_refresh_interval)
                .min(next_cpu_affinity_refresh.saturating_duration_since(Instant::now()))
                .min(cpu_affinity_refresh_interval)
        };

        match wait_for_wake(&shared, wait_for) {
            WorkerWake::Stop => break,
            WorkerWake::SettingsChanged => {
                next_check = Instant::now();
                next_eco_qos_refresh = Instant::now();
                next_app_suspension_refresh = Instant::now();
                next_cpu_affinity_refresh = Instant::now();
            }
            WorkerWake::Timeout => {}
        }
    }
}

struct AutomationSnapshot {
    settings: Settings,
    app_suspension_freeze_requests: Vec<String>,
}

fn automation_snapshot(shared: &SharedAutomationState) -> Option<AutomationSnapshot> {
    shared.state.lock().ok().and_then(|mut state| {
        (!state.stop_requested).then(|| AutomationSnapshot {
            settings: state.settings.clone(),
            app_suspension_freeze_requests: std::mem::take(
                &mut state.app_suspension_freeze_requests,
            ),
        })
    })
}

fn update_eco_qos_status(shared: &SharedAutomationState, status: EcoQosSnapshot) {
    if let Ok(mut state) = shared.state.lock() {
        state.eco_qos_status = status;
    }
}

fn update_app_suspension_status(shared: &SharedAutomationState, status: AppSuspensionSnapshot) {
    if let Ok(mut state) = shared.state.lock() {
        state.app_suspension_status = status;
    }
}

fn update_cpu_affinity_status(shared: &SharedAutomationState, status: CpuAffinitySnapshot) {
    if let Ok(mut state) = shared.state.lock() {
        state.cpu_affinity_status = status;
    }
}

fn automation_refresh_interval(hidden_to_tray: bool, hidden_interval: Duration) -> Duration {
    if hidden_to_tray {
        hidden_interval
    } else {
        VISIBLE_AUTOMATION_REFRESH_INTERVAL
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkerWake {
    Stop,
    SettingsChanged,
    Timeout,
}

fn wait_for_wake(shared: &SharedAutomationState, wait_for: Duration) -> WorkerWake {
    let Ok(state) = shared.state.lock() else {
        return WorkerWake::Stop;
    };
    if state.stop_requested {
        return WorkerWake::Stop;
    }

    match shared.changed.wait_timeout(state, wait_for) {
        Ok((state, _)) if state.stop_requested => WorkerWake::Stop,
        Ok((_state, timeout)) if timeout.timed_out() => WorkerWake::Timeout,
        Ok((_state, _)) => WorkerWake::SettingsChanged,
        Err(_) => WorkerWake::Stop,
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
    eco_qos_manager: EcoQosManager,
    app_suspension_manager: AppSuspensionManager,
    cpu_affinity_manager: CpuAffinityManager,
}

impl HiddenAutomationRunner {
    fn run_eco_qos_update(&mut self, settings: &Settings) -> EcoQosSnapshot {
        let foreground_process_id = self.foreground_detector.process_id();
        self.eco_qos_manager.update(
            &settings.eco_qos,
            settings.general.enabled,
            foreground_process_id,
        )
    }

    fn run_app_suspension_update(
        &mut self,
        settings: &Settings,
        manual_freeze_processes: &[String],
    ) -> AppSuspensionSnapshot {
        let foreground_process_id = self.foreground_detector.process_id();
        self.app_suspension_manager.update(
            &settings.app_suspension,
            settings.general.enabled,
            foreground_process_id,
            manual_freeze_processes,
        )
    }

    fn run_cpu_affinity_update(&mut self, settings: &Settings) -> CpuAffinitySnapshot {
        let foreground_process_id = self.foreground_detector.process_id();
        self.cpu_affinity_manager.update(
            &settings.cpu_affinity,
            settings.general.enabled,
            foreground_process_id,
        )
    }

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
                plugged_in: power_source::is_plugged_in(),
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
