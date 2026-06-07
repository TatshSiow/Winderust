use std::{
    collections::BTreeSet,
    sync::{Arc, Condvar, Mutex},
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use crate::{
    action_log::{ActionLog, ActionLogEntry},
    activity::{input_hook, input_tracker, IdleDetector, InputHookEvents},
    affinity::{CpuAffinityManager, CpuAffinitySnapshot},
    config::{PowerPlanSettings, Settings},
    cpu::{CpuUsageMonitor, CpuUsageSnapshot},
    cpu_limiter::{CpuLimiterManager, CpuLimiterSnapshot},
    ecoqos::{EcoQosManager, EcoQosSnapshot},
    foreground::{list_processes, top_level_window_process_ids, ForegroundDetector},
    performance_mode::{PerformanceModeManager, PerformanceModeSnapshot},
    power::PowerPlanManager,
    power_source,
    responsiveness::{ForegroundResponsivenessManager, ForegroundResponsivenessSnapshot},
    rules::{DecisionEngine, DecisionInput, DecisionOutcome, PerformanceModeDecision},
    scheduler::{CpuUsageScheduler, Scheduler},
    suspension::{AppSuspensionManager, AppSuspensionSnapshot},
    tray,
    watchdog::{WatchdogManager, WatchdogSnapshot},
    windows_events::{WindowsAutomationEvent, WindowsEventWatcher},
};

const ACTIVE_PLAN_REFRESH_INTERVAL: Duration = Duration::from_secs(10);
const CPU_USAGE_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const ECO_QOS_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const APP_SUSPENSION_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const APP_SUSPENSION_FOREGROUND_RELEASE_INTERVAL: Duration = Duration::from_millis(500);
const APP_SUSPENSION_SHELL_USER_INTENT_INTERVAL: Duration = Duration::from_millis(750);
const CPU_AFFINITY_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const CPU_LIMITER_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const PERFORMANCE_MODE_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const WATCHDOG_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const FOREGROUND_RESPONSIVENESS_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const PROCESS_APPEARANCE_SCAN_INTERVAL: Duration = Duration::from_secs(10);
const HIDDEN_AUTOMATION_REFRESH_INTERVAL: Duration = Duration::from_secs(10);
const VISIBLE_AUTOMATION_REFRESH_INTERVAL: Duration = Duration::from_secs(3);
const SWITCH_RETRY_INTERVAL: Duration = Duration::from_secs(15);

pub struct BackgroundAutomation {
    shared: Arc<SharedAutomationState>,
    thread: Mutex<Option<JoinHandle<()>>>,
    event_watcher: Mutex<Option<WindowsEventWatcher>>,
}

struct SharedAutomationState {
    state: Mutex<AutomationWorkerState>,
    changed: Condvar,
}

struct AutomationWorkerState {
    settings: Settings,
    change_generation: u64,
    eco_qos_status: EcoQosSnapshot,
    app_suspension_status: AppSuspensionSnapshot,
    cpu_affinity_status: CpuAffinitySnapshot,
    cpu_limiter_status: CpuLimiterSnapshot,
    performance_mode_status: PerformanceModeSnapshot,
    watchdog_status: WatchdogSnapshot,
    foreground_responsiveness_status: ForegroundResponsivenessSnapshot,
    action_log_entries: Vec<ActionLogEntry>,
    app_suspension_freeze_requests: Vec<String>,
    action_log_clear_requested: bool,
    pending_events: AutomationWakeEvents,
    windows_event_watcher_active: bool,
    stop_requested: bool,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct AutomationWakeEvents {
    settings_changed: bool,
    foreground_changed: bool,
    window_created: bool,
    power_changed: bool,
    session_changed: bool,
    input_activity: bool,
}

impl AutomationWakeEvents {
    fn insert_windows_event(&mut self, event: WindowsAutomationEvent) {
        match event {
            WindowsAutomationEvent::ForegroundChanged => self.foreground_changed = true,
            WindowsAutomationEvent::WindowCreated => self.window_created = true,
            WindowsAutomationEvent::PowerChanged => self.power_changed = true,
            WindowsAutomationEvent::SessionChanged => self.session_changed = true,
        }
    }
}

impl BackgroundAutomation {
    pub fn start(settings: &Settings) -> Self {
        let shared = Arc::new(SharedAutomationState {
            state: Mutex::new(AutomationWorkerState {
                settings: settings.clone(),
                change_generation: 0,
                eco_qos_status: EcoQosSnapshot::default(),
                app_suspension_status: AppSuspensionSnapshot::default(),
                cpu_affinity_status: CpuAffinitySnapshot::default(),
                cpu_limiter_status: CpuLimiterSnapshot::default(),
                performance_mode_status: PerformanceModeSnapshot::default(),
                watchdog_status: WatchdogSnapshot::default(),
                foreground_responsiveness_status: ForegroundResponsivenessSnapshot::default(),
                action_log_entries: Vec::new(),
                app_suspension_freeze_requests: Vec::new(),
                action_log_clear_requested: false,
                pending_events: AutomationWakeEvents::default(),
                windows_event_watcher_active: false,
                stop_requested: false,
            }),
            changed: Condvar::new(),
        });
        let automation = Self {
            shared,
            thread: Mutex::new(None),
            event_watcher: Mutex::new(None),
        };
        automation.sync_worker(settings);
        automation.sync_windows_event_watcher(settings);
        automation
    }

    pub fn update_settings(&self, settings: &Settings) {
        let mut changed = false;
        if let Ok(mut state) = self.shared.state.lock() {
            if state.settings == *settings {
                return;
            }
            state.settings = settings.clone();
            state.pending_events.settings_changed = true;
            state.change_generation = state.change_generation.wrapping_add(1);
            self.shared.changed.notify_one();
            changed = true;
        }

        if changed {
            self.sync_worker(settings);
            self.sync_windows_event_watcher(settings);
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

    pub fn cpu_limiter_status(&self) -> CpuLimiterSnapshot {
        self.shared
            .state
            .lock()
            .map(|state| state.cpu_limiter_status.clone())
            .unwrap_or_default()
    }

    pub fn cpu_affinity_status(&self) -> CpuAffinitySnapshot {
        self.shared
            .state
            .lock()
            .map(|state| state.cpu_affinity_status.clone())
            .unwrap_or_default()
    }

    pub fn foreground_responsiveness_status(&self) -> ForegroundResponsivenessSnapshot {
        self.shared
            .state
            .lock()
            .map(|state| state.foreground_responsiveness_status.clone())
            .unwrap_or_default()
    }

    pub fn performance_mode_status(&self) -> PerformanceModeSnapshot {
        self.shared
            .state
            .lock()
            .map(|state| state.performance_mode_status.clone())
            .unwrap_or_default()
    }

    pub fn watchdog_status(&self) -> WatchdogSnapshot {
        self.shared
            .state
            .lock()
            .map(|state| state.watchdog_status.clone())
            .unwrap_or_default()
    }

    pub fn action_log_entries(&self) -> Vec<ActionLogEntry> {
        self.shared
            .state
            .lock()
            .map(|state| state.action_log_entries.clone())
            .unwrap_or_default()
    }

    pub fn clear_action_log(&self) {
        if let Ok(mut state) = self.shared.state.lock() {
            state.action_log_entries.clear();
            state.action_log_clear_requested = true;
            state.change_generation = state.change_generation.wrapping_add(1);
            self.shared.changed.notify_one();
        }
    }

    pub fn request_app_suspension_freeze(&self, process_name: &str) {
        let process_name = process_name.trim().to_ascii_lowercase();
        if process_name.is_empty() {
            return;
        }

        let mut settings_to_sync = None;
        if let Ok(mut state) = self.shared.state.lock() {
            state.app_suspension_freeze_requests.push(process_name);
            state.pending_events.settings_changed = true;
            state.change_generation = state.change_generation.wrapping_add(1);
            settings_to_sync = Some(state.settings.clone());
            self.shared.changed.notify_one();
        }

        if let Some(settings) = settings_to_sync {
            self.sync_worker(&settings);
        }
    }

    pub fn input_event_callback(&self) -> Arc<dyn Fn(InputHookEvents) + Send + Sync> {
        let shared = Arc::clone(&self.shared);
        Arc::new(move |events| notify_input_event(&shared, events))
    }

    fn sync_worker(&self, settings: &Settings) {
        let Ok(mut thread) = self.thread.lock() else {
            return;
        };

        if thread.as_ref().is_some_and(|thread| thread.is_finished()) {
            if let Some(thread) = thread.take() {
                let _ = thread.join();
            }
        }

        if automation_worker_required(settings) && thread.is_none() {
            let thread_shared = Arc::clone(&self.shared);
            *thread = Some(thread::spawn(move || {
                run_background_automation(thread_shared)
            }));
        }
    }

    fn sync_windows_event_watcher(&self, settings: &Settings) {
        let Ok(mut watcher) = self.event_watcher.lock() else {
            return;
        };

        if windows_event_watcher_required(settings) {
            if watcher.is_none() {
                let shared = Arc::clone(&self.shared);
                *watcher = WindowsEventWatcher::start(Arc::new(move |event| {
                    notify_windows_event(&shared, event);
                }))
                .ok();
            }
        } else {
            *watcher = None;
        }

        set_windows_event_watcher_active(&self.shared, watcher.is_some());
    }
}

impl Drop for BackgroundAutomation {
    fn drop(&mut self) {
        if let Ok(mut watcher) = self.event_watcher.lock() {
            *watcher = None;
        }

        if let Ok(mut state) = self.shared.state.lock() {
            state.stop_requested = true;
            self.shared.changed.notify_one();
        }

        let thread = self.thread.lock().ok().and_then(|mut thread| thread.take());
        if let Some(thread) = thread {
            let _ = thread.join();
        }
    }
}

fn run_background_automation(shared: Arc<SharedAutomationState>) {
    let mut runner = HiddenAutomationRunner::default();
    let mut next_check = Instant::now();
    let mut next_eco_qos_refresh = Instant::now();
    let mut next_app_suspension_refresh = Instant::now();
    let mut next_app_suspension_foreground_release = Instant::now();
    let mut next_cpu_affinity_refresh = Instant::now();
    let mut next_cpu_limiter_refresh = Instant::now();
    let mut next_performance_mode_refresh = Instant::now();
    let mut next_watchdog_refresh = Instant::now();
    let mut next_foreground_responsiveness_refresh = Instant::now();
    let mut next_process_appearance_scan = Instant::now();

    loop {
        let snapshot = match automation_snapshot(&shared) {
            Some(snapshot) => snapshot,
            None => break,
        };
        let settings = snapshot.settings;
        let change_generation = snapshot.change_generation;
        let app_suspension_freeze_requests = snapshot.app_suspension_freeze_requests;
        if snapshot.action_log_clear_requested {
            runner.action_log.clear();
            update_action_log_entries(&shared, runner.action_log.entries());
        }
        let wake_events = snapshot.wake_events;
        let windows_event_watcher_active = snapshot.windows_event_watcher_active;
        let hidden_to_tray = tray::is_hidden_to_tray();
        let eco_qos_refresh_interval =
            automation_refresh_interval(hidden_to_tray, ECO_QOS_REFRESH_INTERVAL);
        let app_suspension_refresh_interval =
            automation_refresh_interval(hidden_to_tray, APP_SUSPENSION_REFRESH_INTERVAL);
        let cpu_affinity_refresh_interval =
            automation_refresh_interval(hidden_to_tray, CPU_AFFINITY_REFRESH_INTERVAL);
        let cpu_limiter_refresh_interval =
            automation_refresh_interval(hidden_to_tray, CPU_LIMITER_REFRESH_INTERVAL);
        let performance_mode_refresh_interval =
            automation_refresh_interval(hidden_to_tray, PERFORMANCE_MODE_REFRESH_INTERVAL);
        let watchdog_refresh_interval =
            automation_refresh_interval(hidden_to_tray, WATCHDOG_REFRESH_INTERVAL);
        let foreground_responsiveness_refresh_interval =
            automation_refresh_interval(hidden_to_tray, FOREGROUND_RESPONSIVENESS_REFRESH_INTERVAL);
        let settings_changed = wake_events.settings_changed || runner.note_settings(&settings);
        if settings_changed {
            next_check = Instant::now();
            next_eco_qos_refresh = Instant::now();
            next_app_suspension_refresh = Instant::now();
            next_app_suspension_foreground_release = Instant::now();
            next_cpu_affinity_refresh = Instant::now();
            next_cpu_limiter_refresh = Instant::now();
            next_performance_mode_refresh = Instant::now();
            next_watchdog_refresh = Instant::now();
            next_foreground_responsiveness_refresh = Instant::now();
            next_process_appearance_scan = Instant::now();
        }
        if wake_events.foreground_changed || wake_events.session_changed {
            next_check = Instant::now();
            next_eco_qos_refresh = Instant::now();
            next_cpu_affinity_refresh = Instant::now();
            next_cpu_limiter_refresh = Instant::now();
            next_foreground_responsiveness_refresh = Instant::now();
            next_app_suspension_foreground_release = Instant::now();
        }
        if wake_events.window_created || wake_events.session_changed {
            next_process_appearance_scan = Instant::now();
            next_app_suspension_refresh = Instant::now();
        }
        if wake_events.power_changed || wake_events.session_changed {
            next_check = Instant::now();
            runner.refresh_active_plan();
        }
        if wake_events.input_activity {
            next_check = Instant::now();
        }
        let power_plan_checks_required = power_plan_checks_required(&settings);
        let scan_process_appearance = process_appearance_scan_required(&settings);
        let eco_qos_refresh_required =
            settings_changed || feature_refresh_required(&settings, settings.eco_qos.enabled);
        let app_suspension_refresh_required = settings_changed
            || feature_refresh_required(&settings, settings.app_suspension.enabled)
            || !app_suspension_freeze_requests.is_empty()
            || runner.app_suspension_manager.has_suspended_processes();
        let cpu_affinity_refresh_required =
            settings_changed || feature_refresh_required(&settings, settings.cpu_affinity.enabled);
        let cpu_limiter_refresh_required =
            settings_changed || feature_refresh_required(&settings, settings.cpu_limiter.enabled);
        let performance_mode_refresh_required = settings_changed
            || feature_refresh_required(&settings, settings.performance_mode.enabled);
        let watchdog_refresh_required =
            settings_changed || feature_refresh_required(&settings, settings.watchdog.enabled);
        let foreground_responsiveness_refresh_required = settings_changed
            || feature_refresh_required(&settings, settings.foreground_responsiveness.enabled);

        if !app_suspension_freeze_requests.is_empty() {
            next_app_suspension_refresh = Instant::now();
        }

        if scan_process_appearance && Instant::now() >= next_process_appearance_scan {
            if runner.detect_process_appearance() {
                next_eco_qos_refresh = Instant::now();
                next_cpu_affinity_refresh = Instant::now();
                next_cpu_limiter_refresh = Instant::now();
                next_performance_mode_refresh = Instant::now();
                next_watchdog_refresh = Instant::now();
                next_foreground_responsiveness_refresh = Instant::now();
            }
            next_process_appearance_scan = Instant::now() + PROCESS_APPEARANCE_SCAN_INTERVAL;
        } else if !scan_process_appearance {
            runner.known_process_ids.clear();
            next_process_appearance_scan = Instant::now() + PROCESS_APPEARANCE_SCAN_INTERVAL;
        }

        if runner.app_suspension_manager.has_suspended_processes()
            && Instant::now() >= next_app_suspension_foreground_release
        {
            if let Some(app_suspension_status) = runner.run_app_suspension_foreground_release() {
                update_app_suspension_status(&shared, app_suspension_status);
                update_action_log_entries(&shared, runner.action_log.entries());
            }
            next_app_suspension_foreground_release =
                Instant::now() + APP_SUSPENSION_FOREGROUND_RELEASE_INTERVAL;
        }

        if eco_qos_refresh_required && Instant::now() >= next_eco_qos_refresh {
            let eco_qos_status = runner.run_eco_qos_update(&settings);
            update_eco_qos_status(&shared, eco_qos_status);
            update_action_log_entries(&shared, runner.action_log.entries());
            next_eco_qos_refresh = Instant::now() + eco_qos_refresh_interval;
        }
        if foreground_responsiveness_refresh_required
            && Instant::now() >= next_foreground_responsiveness_refresh
        {
            let foreground_responsiveness_status =
                runner.run_foreground_responsiveness_update(&settings);
            update_foreground_responsiveness_status(&shared, foreground_responsiveness_status);
            update_action_log_entries(&shared, runner.action_log.entries());
            next_foreground_responsiveness_refresh =
                Instant::now() + foreground_responsiveness_refresh_interval;
        }
        if app_suspension_refresh_required && Instant::now() >= next_app_suspension_refresh {
            let app_suspension_status =
                runner.run_app_suspension_update(&settings, &app_suspension_freeze_requests);
            update_app_suspension_status(&shared, app_suspension_status);
            update_action_log_entries(&shared, runner.action_log.entries());
            next_app_suspension_refresh = Instant::now() + app_suspension_refresh_interval;
            if runner.app_suspension_manager.has_suspended_processes() {
                next_app_suspension_foreground_release = Instant::now();
            }
        }
        if cpu_affinity_refresh_required && Instant::now() >= next_cpu_affinity_refresh {
            let cpu_affinity_status = runner.run_cpu_affinity_update(&settings);
            update_cpu_affinity_status(&shared, cpu_affinity_status);
            update_action_log_entries(&shared, runner.action_log.entries());
            next_cpu_affinity_refresh = Instant::now() + cpu_affinity_refresh_interval;
        }
        if cpu_limiter_refresh_required && Instant::now() >= next_cpu_limiter_refresh {
            let cpu_limiter_status = runner.run_cpu_limiter_update(&settings);
            update_cpu_limiter_status(&shared, cpu_limiter_status);
            update_action_log_entries(&shared, runner.action_log.entries());
            next_cpu_limiter_refresh = Instant::now() + cpu_limiter_refresh_interval;
        }
        if performance_mode_refresh_required && Instant::now() >= next_performance_mode_refresh {
            let performance_mode_status = runner.run_performance_mode_update(&settings);
            update_performance_mode_status(&shared, performance_mode_status);
            update_action_log_entries(&shared, runner.action_log.entries());
            next_performance_mode_refresh = Instant::now() + performance_mode_refresh_interval;
        }
        if watchdog_refresh_required && Instant::now() >= next_watchdog_refresh {
            let watchdog_status = runner.run_watchdog_update(&settings);
            update_watchdog_status(&shared, watchdog_status);
            update_action_log_entries(&shared, runner.action_log.entries());
            next_watchdog_refresh = Instant::now() + watchdog_refresh_interval;
        }

        let mut wait_for = if hidden_to_tray {
            if power_plan_checks_required {
                let input_events = input_hook::take_pending_events();
                if input_hook_should_check(&settings, input_events) {
                    next_check = Instant::now();
                }

                if Instant::now() >= next_check && !runner.performance_mode_manager.is_active() {
                    runner.run_check(&settings);
                }

                if let Some(delay) =
                    hidden_power_plan_check_delay(&settings, windows_event_watcher_active)
                {
                    next_check = Instant::now() + delay;
                    Some(next_check.saturating_duration_since(Instant::now()))
                } else {
                    next_check = Instant::now();
                    None
                }
            } else {
                next_check = Instant::now();
                None
            }
        } else {
            next_check = Instant::now();
            None
        };

        if eco_qos_refresh_required {
            wait_for = Some(min_worker_wait(
                wait_for,
                next_eco_qos_refresh
                    .saturating_duration_since(Instant::now())
                    .min(eco_qos_refresh_interval),
            ));
        }
        if app_suspension_refresh_required {
            wait_for = Some(min_worker_wait(
                wait_for,
                next_app_suspension_refresh
                    .saturating_duration_since(Instant::now())
                    .min(app_suspension_refresh_interval),
            ));
        }
        if cpu_affinity_refresh_required {
            wait_for = Some(min_worker_wait(
                wait_for,
                next_cpu_affinity_refresh
                    .saturating_duration_since(Instant::now())
                    .min(cpu_affinity_refresh_interval),
            ));
        }
        if cpu_limiter_refresh_required {
            wait_for = Some(min_worker_wait(
                wait_for,
                next_cpu_limiter_refresh
                    .saturating_duration_since(Instant::now())
                    .min(cpu_limiter_refresh_interval),
            ));
        }
        if performance_mode_refresh_required {
            wait_for = Some(min_worker_wait(
                wait_for,
                next_performance_mode_refresh
                    .saturating_duration_since(Instant::now())
                    .min(performance_mode_refresh_interval),
            ));
        }
        if watchdog_refresh_required {
            wait_for = Some(min_worker_wait(
                wait_for,
                next_watchdog_refresh
                    .saturating_duration_since(Instant::now())
                    .min(watchdog_refresh_interval),
            ));
        }
        if foreground_responsiveness_refresh_required {
            wait_for = Some(min_worker_wait(
                wait_for,
                next_foreground_responsiveness_refresh
                    .saturating_duration_since(Instant::now())
                    .min(foreground_responsiveness_refresh_interval),
            ));
        }
        if scan_process_appearance {
            wait_for = Some(min_worker_wait(
                wait_for,
                next_process_appearance_scan
                    .saturating_duration_since(Instant::now())
                    .min(PROCESS_APPEARANCE_SCAN_INTERVAL),
            ));
        }
        if runner.app_suspension_manager.has_suspended_processes() {
            wait_for = Some(min_worker_wait(
                wait_for,
                next_app_suspension_foreground_release
                    .saturating_duration_since(Instant::now())
                    .min(APP_SUSPENSION_FOREGROUND_RELEASE_INTERVAL),
            ));
        }

        if wait_for.is_none() && !automation_worker_required(&settings) {
            break;
        }

        match wait_for_wake(&shared, wait_for, change_generation) {
            WorkerWake::Stop => break,
            WorkerWake::Changed => {}
            WorkerWake::Timeout => {}
        }
    }
}

fn min_worker_wait(current: Option<Duration>, candidate: Duration) -> Duration {
    current.map_or(candidate, |current| current.min(candidate))
}

struct AutomationSnapshot {
    settings: Settings,
    change_generation: u64,
    app_suspension_freeze_requests: Vec<String>,
    action_log_clear_requested: bool,
    wake_events: AutomationWakeEvents,
    windows_event_watcher_active: bool,
}

fn automation_snapshot(shared: &SharedAutomationState) -> Option<AutomationSnapshot> {
    shared.state.lock().ok().and_then(|mut state| {
        (!state.stop_requested).then(|| AutomationSnapshot {
            settings: state.settings.clone(),
            change_generation: state.change_generation,
            app_suspension_freeze_requests: std::mem::take(
                &mut state.app_suspension_freeze_requests,
            ),
            action_log_clear_requested: std::mem::take(&mut state.action_log_clear_requested),
            wake_events: std::mem::take(&mut state.pending_events),
            windows_event_watcher_active: state.windows_event_watcher_active,
        })
    })
}

fn set_windows_event_watcher_active(shared: &SharedAutomationState, active: bool) {
    if let Ok(mut state) = shared.state.lock() {
        if state.windows_event_watcher_active == active {
            return;
        }

        state.windows_event_watcher_active = active;
        state.change_generation = state.change_generation.wrapping_add(1);
        shared.changed.notify_one();
    }
}

fn notify_windows_event(shared: &SharedAutomationState, event: WindowsAutomationEvent) {
    if let Ok(mut state) = shared.state.lock() {
        if state.stop_requested || !windows_event_wake_required(&state.settings, event) {
            return;
        }

        state.pending_events.insert_windows_event(event);
        state.change_generation = state.change_generation.wrapping_add(1);
        shared.changed.notify_one();
    }
}

fn notify_input_event(shared: &SharedAutomationState, events: InputHookEvents) {
    if let Ok(mut state) = shared.state.lock() {
        if state.stop_requested || !input_hook_should_check(&state.settings, events) {
            return;
        }

        state.pending_events.input_activity = true;
        state.change_generation = state.change_generation.wrapping_add(1);
        shared.changed.notify_one();
    }
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

fn update_cpu_limiter_status(shared: &SharedAutomationState, status: CpuLimiterSnapshot) {
    if let Ok(mut state) = shared.state.lock() {
        state.cpu_limiter_status = status;
    }
}

fn update_performance_mode_status(shared: &SharedAutomationState, status: PerformanceModeSnapshot) {
    if let Ok(mut state) = shared.state.lock() {
        state.performance_mode_status = status;
    }
}

fn update_watchdog_status(shared: &SharedAutomationState, status: WatchdogSnapshot) {
    if let Ok(mut state) = shared.state.lock() {
        state.watchdog_status = status;
    }
}

fn update_foreground_responsiveness_status(
    shared: &SharedAutomationState,
    status: ForegroundResponsivenessSnapshot,
) {
    if let Ok(mut state) = shared.state.lock() {
        state.foreground_responsiveness_status = status;
    }
}

fn update_action_log_entries(shared: &SharedAutomationState, entries: Vec<ActionLogEntry>) {
    if let Ok(mut state) = shared.state.lock() {
        state.action_log_entries = entries;
    }
}

fn automation_refresh_interval(hidden_to_tray: bool, hidden_interval: Duration) -> Duration {
    if hidden_to_tray {
        hidden_interval.max(HIDDEN_AUTOMATION_REFRESH_INTERVAL)
    } else {
        VISIBLE_AUTOMATION_REFRESH_INTERVAL
    }
}

fn feature_refresh_required(settings: &Settings, feature_enabled: bool) -> bool {
    settings.general.enabled && feature_enabled
}

fn process_appearance_scan_required(settings: &Settings) -> bool {
    settings.general.enabled
        && (settings.eco_qos.enabled
            || settings.cpu_affinity.enabled
            || settings.cpu_limiter.enabled
            || settings.performance_mode.enabled
            || settings.watchdog.enabled
            || settings.foreground_responsiveness.enabled)
}

fn power_plan_checks_required(settings: &Settings) -> bool {
    settings.general.enabled
        && (activity_power_plan_required(settings)
            || foreground_rules_required(settings)
            || schedule_rules_required(settings)
            || cpu_usage_rules_required(settings)
            || performance_mode_required(settings))
}

fn automation_worker_required(settings: &Settings) -> bool {
    settings.general.enabled
        && (power_plan_checks_required(settings)
            || settings.eco_qos.enabled
            || settings.app_suspension.enabled
            || settings.cpu_affinity.enabled
            || settings.cpu_limiter.enabled
            || settings.performance_mode.enabled
            || settings.watchdog.enabled
            || settings.foreground_responsiveness.enabled)
}

fn windows_event_watcher_required(settings: &Settings) -> bool {
    settings.general.enabled
        && (power_plan_checks_required(settings)
            || settings.app_suspension.enabled
            || process_appearance_scan_required(settings))
}

fn windows_event_wake_required(settings: &Settings, event: WindowsAutomationEvent) -> bool {
    if !settings.general.enabled {
        return false;
    }

    match event {
        WindowsAutomationEvent::ForegroundChanged => {
            power_plan_checks_required(settings)
                || settings.app_suspension.enabled
                || process_appearance_scan_required(settings)
        }
        WindowsAutomationEvent::WindowCreated => {
            settings.app_suspension.enabled || process_appearance_scan_required(settings)
        }
        WindowsAutomationEvent::PowerChanged => power_plan_checks_required(settings),
        WindowsAutomationEvent::SessionChanged => windows_event_watcher_required(settings),
    }
}

fn activity_power_plan_required(settings: &Settings) -> bool {
    settings.activity_mode.enabled
        && (has_idle_plan(&settings.activity_mode.power_plans, settings)
            || (settings.activity_mode.switch_to_performance_on_resume
                && settings.activity_mode.input_detection.any_enabled()
                && has_active_plan(&settings.activity_mode.power_plans, settings)))
}

fn foreground_rules_required(settings: &Settings) -> bool {
    settings.foreground_rules.enabled
        && (settings
            .foreground_rules
            .rules
            .iter()
            .any(|rule| rule.enabled && rule.power_plan_guid.is_some())
            || (!settings.foreground_rules.whitelist.is_empty()
                && has_active_plan(&settings.foreground_rules.power_plans, settings))
            || (!settings.foreground_rules.force_power_save.is_empty()
                && has_idle_plan(&settings.foreground_rules.power_plans, settings)))
}

fn schedule_rules_required(settings: &Settings) -> bool {
    settings.schedule_mode.enabled
        && settings
            .schedule_mode
            .rules
            .iter()
            .any(|rule| rule.enabled && rule.power_plan_guid.is_some())
}

fn cpu_usage_rules_required(settings: &Settings) -> bool {
    settings.cpu_usage_mode.enabled
        && settings.cpu_usage_mode.rules.iter().any(|rule| {
            rule.enabled
                && (rule.power_plan_guid.is_some()
                    || (rule.else_enabled && rule.else_power_plan_guid.is_some()))
        })
}

fn performance_mode_required(settings: &Settings) -> bool {
    settings.performance_mode.enabled
        && settings.performance_mode.rules.iter().any(|rule| {
            rule.enabled
                && (rule.power_plan_guid.is_some()
                    || settings.power_plans.performance_guid.is_some())
        })
}

fn has_idle_plan(power_plans: &PowerPlanSettings, settings: &Settings) -> bool {
    power_plans.power_save_guid.is_some() || settings.power_plans.power_save_guid.is_some()
}

fn has_active_plan(power_plans: &PowerPlanSettings, settings: &Settings) -> bool {
    power_plans.performance_guid.is_some() || settings.power_plans.performance_guid.is_some()
}

fn configured_check_interval(settings: &Settings) -> Duration {
    Duration::from_millis(settings.general.check_interval_ms.max(250))
}

fn hidden_power_plan_check_delay(
    settings: &Settings,
    windows_event_watcher_active: bool,
) -> Option<Duration> {
    if !windows_event_watcher_active {
        return Some(configured_check_interval(settings));
    }

    if cpu_usage_rules_required(settings) {
        return Some(CPU_USAGE_REFRESH_INTERVAL);
    }
    if schedule_rules_required(settings) {
        return Some(configured_check_interval(settings));
    }
    if performance_mode_required(settings) {
        return Some(PERFORMANCE_MODE_REFRESH_INTERVAL);
    }
    activity_idle_check_delay(settings)
}

fn activity_idle_check_delay(settings: &Settings) -> Option<Duration> {
    if !settings.general.enabled
        || !settings.activity_mode.enabled
        || !has_idle_plan(&settings.activity_mode.power_plans, settings)
    {
        return None;
    }

    let timeout = Duration::from_secs(settings.activity_mode.idle_timeout_seconds);
    match input_tracker::last_input_elapsed() {
        Some(idle_for) if idle_for < timeout => Some(timeout - idle_for),
        Some(_) => None,
        None => Some(configured_check_interval(settings)),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkerWake {
    Stop,
    Changed,
    Timeout,
}

fn wait_for_wake(
    shared: &SharedAutomationState,
    wait_for: Option<Duration>,
    observed_generation: u64,
) -> WorkerWake {
    let Ok(state) = shared.state.lock() else {
        return WorkerWake::Stop;
    };
    if state.stop_requested {
        return WorkerWake::Stop;
    }
    if state.change_generation != observed_generation {
        return WorkerWake::Changed;
    }

    if let Some(wait_for) = wait_for {
        match shared.changed.wait_timeout(state, wait_for) {
            Ok((state, _)) if state.stop_requested => WorkerWake::Stop,
            Ok((state, _)) if state.change_generation != observed_generation => WorkerWake::Changed,
            Ok((_state, timeout)) if timeout.timed_out() => WorkerWake::Timeout,
            Ok((_state, _)) => WorkerWake::Changed,
            Err(_) => WorkerWake::Stop,
        }
    } else {
        match shared.changed.wait(state) {
            Ok(state) if state.stop_requested => WorkerWake::Stop,
            Ok(state) if state.change_generation != observed_generation => WorkerWake::Changed,
            Ok(_) => WorkerWake::Changed,
            Err(_) => WorkerWake::Stop,
        }
    }
}

fn input_hook_should_check(settings: &Settings, events: InputHookEvents) -> bool {
    settings.general.enabled
        && settings.activity_mode.enabled
        && ((events.keyboard && settings.activity_mode.input_detection.keyboard)
            || (events.mouse && settings.activity_mode.input_detection.mouse))
}

fn process_ids_have_new_entries(
    known_process_ids: &mut BTreeSet<u32>,
    current_ids: BTreeSet<u32>,
) -> bool {
    let initialized = !known_process_ids.is_empty();
    let has_new_entries = initialized
        && current_ids
            .iter()
            .any(|process_id| !known_process_ids.contains(process_id));
    *known_process_ids = current_ids;
    has_new_entries
}

#[derive(Default)]
struct HiddenAutomationRunner {
    last_settings: Option<Settings>,
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
    last_app_suspension_shell_user_intent: Option<Instant>,
    cpu_affinity_manager: CpuAffinityManager,
    cpu_limiter_manager: CpuLimiterManager,
    performance_mode_manager: PerformanceModeManager,
    watchdog_manager: WatchdogManager,
    action_log: ActionLog,
    foreground_responsiveness_manager: ForegroundResponsivenessManager,
    known_process_ids: BTreeSet<u32>,
}

impl HiddenAutomationRunner {
    fn note_settings(&mut self, settings: &Settings) -> bool {
        self.action_log.set_mode(settings.advanced.action_log_mode);

        let changed = self.last_settings.as_ref() != Some(settings);
        if changed {
            self.last_settings = Some(settings.clone());
        }
        changed
    }

    fn detect_process_appearance(&mut self) -> bool {
        let Ok(processes) = list_processes() else {
            return false;
        };
        let current_ids = processes
            .into_iter()
            .filter_map(|process| (process.id != 0).then_some(process.id))
            .collect::<BTreeSet<_>>();

        process_ids_have_new_entries(&mut self.known_process_ids, current_ids)
    }

    fn run_eco_qos_update(&mut self, settings: &Settings) -> EcoQosSnapshot {
        let foreground_process_id = self.foreground_detector.process_id();
        self.eco_qos_manager.update(
            &settings.eco_qos,
            settings.general.enabled,
            foreground_process_id,
            &mut self.action_log,
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
            &mut self.action_log,
        )
    }

    fn run_app_suspension_foreground_release(&mut self) -> Option<AppSuspensionSnapshot> {
        let now = Instant::now();
        if self.foreground_detector.shell_window_mouse_pressed()
            && self.app_suspension_shell_user_intent_due(now)
        {
            self.last_app_suspension_shell_user_intent = Some(now);
            if let Some(status) = self
                .app_suspension_manager
                .release_window_owner_processes_for_user_intent(
                    &top_level_window_process_ids(),
                    &mut self.action_log,
                )
            {
                return Some(status);
            }
        }

        let foreground_process_id = self.foreground_detector.process_id();
        let foreground_process = self.foreground_detector.process();
        if let Some(status) = foreground_process_id.and_then(|process_id| {
            self.app_suspension_manager.release_interactive_process(
                process_id,
                foreground_process
                    .as_ref()
                    .filter(|process| process.id == process_id)
                    .map(|process| process.name.as_str()),
                &mut self.action_log,
            )
        }) {
            return Some(status);
        }

        let cursor_process_id = self.foreground_detector.cursor_process_id()?;
        if foreground_process_id == Some(cursor_process_id) {
            return None;
        }
        let cursor_process = self.foreground_detector.cursor_process();
        self.app_suspension_manager.release_interactive_process(
            cursor_process_id,
            cursor_process
                .as_ref()
                .filter(|process| process.id == cursor_process_id)
                .map(|process| process.name.as_str()),
            &mut self.action_log,
        )
    }

    fn app_suspension_shell_user_intent_due(&self, now: Instant) -> bool {
        self.last_app_suspension_shell_user_intent
            .map_or(true, |last| {
                now.duration_since(last) >= APP_SUSPENSION_SHELL_USER_INTENT_INTERVAL
            })
    }

    fn run_cpu_affinity_update(&mut self, settings: &Settings) -> CpuAffinitySnapshot {
        let foreground_process_id = self.foreground_detector.process_id();
        self.cpu_affinity_manager.update(
            &settings.cpu_affinity,
            settings.general.enabled,
            foreground_process_id,
            &mut self.action_log,
        )
    }

    fn run_cpu_limiter_update(&mut self, settings: &Settings) -> CpuLimiterSnapshot {
        let foreground_process_id = self.foreground_detector.process_id();
        let core_steering_process_ids = self.cpu_affinity_manager.adjusted_process_ids();
        self.cpu_limiter_manager.update(
            &settings.cpu_limiter,
            settings.general.enabled,
            foreground_process_id,
            &core_steering_process_ids,
            &mut self.action_log,
        )
    }

    fn run_performance_mode_update(&mut self, settings: &Settings) -> PerformanceModeSnapshot {
        let status = self.performance_mode_manager.update(
            &settings.performance_mode,
            &settings.power_plans,
            settings.general.enabled,
            &mut self.action_log,
        );

        status
    }

    fn run_watchdog_update(&mut self, settings: &Settings) -> WatchdogSnapshot {
        self.watchdog_manager.update(
            &settings.watchdog,
            settings.general.enabled,
            &mut self.action_log,
        )
    }

    fn run_foreground_responsiveness_update(
        &mut self,
        settings: &Settings,
    ) -> ForegroundResponsivenessSnapshot {
        let foreground_process_id = self.foreground_detector.process_id();
        let mut excluded_process_ids = self.eco_qos_manager.throttled_process_ids();
        excluded_process_ids.extend(self.performance_mode_manager.active_process_ids());
        self.foreground_responsiveness_manager.update(
            &settings.foreground_responsiveness,
            settings.general.enabled,
            foreground_process_id,
            &excluded_process_ids,
            &mut self.action_log,
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
                performance_mode: self.performance_mode_manager.active_decision().map(
                    |(rule_name, process_name, power_plan_guid)| PerformanceModeDecision {
                        rule_name,
                        process_name,
                        power_plan_guid,
                    },
                ),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_appearance_detector_ignores_initial_snapshot() {
        let mut known = BTreeSet::new();

        assert!(!process_ids_have_new_entries(
            &mut known,
            BTreeSet::from([1, 2])
        ));
        assert_eq!(known, BTreeSet::from([1, 2]));
    }

    #[test]
    fn process_appearance_detector_reports_new_process_ids() {
        let mut known = BTreeSet::from([1, 2]);

        assert!(process_ids_have_new_entries(
            &mut known,
            BTreeSet::from([1, 2, 3])
        ));
        assert_eq!(known, BTreeSet::from([1, 2, 3]));
    }

    #[test]
    fn process_appearance_detector_does_not_report_only_exits() {
        let mut known = BTreeSet::from([1, 2, 3]);

        assert!(!process_ids_have_new_entries(
            &mut known,
            BTreeSet::from([1, 2])
        ));
        assert_eq!(known, BTreeSet::from([1, 2]));
    }

    #[test]
    fn process_appearance_scan_sleeps_when_process_features_are_off() {
        let settings = Settings::default();

        assert!(!process_appearance_scan_required(&settings));
    }

    #[test]
    fn automation_worker_sleeps_when_no_automation_work_exists() {
        let settings = Settings::default();

        assert!(!automation_worker_required(&settings));
    }

    #[test]
    fn automation_worker_runs_for_enabled_process_feature() {
        let mut settings = Settings::default();
        settings.eco_qos.enabled = true;

        assert!(automation_worker_required(&settings));
    }

    #[test]
    fn default_settings_do_not_poll_power_plans_without_plan_targets() {
        let settings = Settings::default();

        assert!(!power_plan_checks_required(&settings));
    }

    #[test]
    fn app_suspension_uses_own_refresh_without_process_appearance_scan() {
        let mut settings = Settings::default();
        settings.app_suspension.enabled = true;

        assert!(feature_refresh_required(
            &settings,
            settings.app_suspension.enabled
        ));
        assert!(!process_appearance_scan_required(&settings));
    }

    #[test]
    fn app_suspension_uses_windows_events_without_enabling_process_scan() {
        let mut settings = Settings::default();
        settings.app_suspension.enabled = true;

        assert!(windows_event_watcher_required(&settings));
        assert!(windows_event_wake_required(
            &settings,
            WindowsAutomationEvent::WindowCreated
        ));
        assert!(!process_appearance_scan_required(&settings));
    }

    #[test]
    fn event_driven_power_checks_drop_idle_polling_for_foreground_only_rules() {
        let mut settings = Settings::default();
        settings.activity_mode.enabled = false;
        settings.foreground_rules.enabled = true;
        settings.foreground_rules.power_plans.performance_guid = Some("active-guid".to_owned());
        settings
            .foreground_rules
            .whitelist
            .push("chat.exe".to_owned());

        assert!(power_plan_checks_required(&settings));
        assert!(windows_event_watcher_required(&settings));
        assert!(hidden_power_plan_check_delay(&settings, true).is_none());
        assert!(hidden_power_plan_check_delay(&settings, false).is_some());
    }

    #[test]
    fn hidden_activity_input_resume_waits_for_hook_event() {
        let mut settings = Settings::default();
        settings.power_plans.performance_guid = Some("active-guid".to_owned());

        assert!(power_plan_checks_required(&settings));
        assert!(windows_event_watcher_required(&settings));
        assert!(hidden_power_plan_check_delay(&settings, true).is_none());
        assert!(hidden_power_plan_check_delay(&settings, false).is_some());
    }

    #[test]
    fn activity_mode_polls_when_it_can_target_a_power_plan() {
        let mut settings = Settings::default();
        settings.power_plans.power_save_guid = Some("idle-guid".to_owned());

        assert!(power_plan_checks_required(&settings));
    }

    #[test]
    fn process_appearance_scan_runs_for_enabled_process_features() {
        let mut settings = Settings::default();
        settings.eco_qos.enabled = true;

        assert!(process_appearance_scan_required(&settings));
        assert!(!power_plan_checks_required(&settings));
    }

    #[test]
    fn disabled_automation_suppresses_worker_refreshes() {
        let mut settings = Settings::default();
        settings.general.enabled = false;
        settings.eco_qos.enabled = true;

        assert!(!feature_refresh_required(
            &settings,
            settings.eco_qos.enabled
        ));
        assert!(!process_appearance_scan_required(&settings));
        assert!(!power_plan_checks_required(&settings));
    }

    #[test]
    fn power_plan_checks_sleep_when_decision_features_are_off() {
        let mut settings = Settings::default();
        settings.activity_mode.enabled = false;
        settings.foreground_rules.enabled = false;
        settings.schedule_mode.enabled = false;
        settings.cpu_usage_mode.enabled = false;
        settings.performance_mode.enabled = false;

        assert!(!power_plan_checks_required(&settings));
    }
}
