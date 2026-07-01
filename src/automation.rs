use std::{
    collections::BTreeSet,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Condvar, Mutex,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use crate::{
    action_log::{ActionLog, ActionLogEntry},
    activity::{
        input_hook, input_tracker, merge_activity_snapshot, ControllerActivityDetector,
        IdleDetector, InputHookEvents, CONTROLLER_ACTIVITY_POLL_INTERVAL,
    },
    affinity::{CpuAffinityManager, CpuAffinitySnapshot},
    background_cpu::BackgroundCpuRestrictionManager,
    config::{
        AccentColorSource, AnimationMode, AppThemeMode, ForegroundRule, PowerPlanSettings,
        ProcessIoPriority, Settings,
    },
    cpu::{CpuUsageMonitor, CpuUsageSnapshot},
    cpu_limiter::{CpuLimiterManager, CpuLimiterSnapshot},
    cpu_priority::{CpuPriorityManager, CpuPrioritySnapshot},
    ecoqos::{EcoQosManager, EcoQosSnapshot},
    foreground::{
        list_processes, process_name_key, top_level_window_process_ids, ForegroundDetector,
    },
    gpu_priority::{GpuPriorityManager, GpuPrioritySnapshot},
    io_priority::{IoPriorityManager, IoPrioritySnapshot},
    memory_priority::{MemoryPriorityManager, MemoryPrioritySnapshot},
    performance_mode::{PerformanceModeManager, PerformanceModeSnapshot},
    power::PowerPlanManager,
    power_source,
    priority_boost::{PriorityBoostManager, PriorityBoostSnapshot},
    responsiveness::{
        ForegroundResponsivenessManager, ForegroundResponsivenessSnapshot,
        ForegroundResponsivenessUpdate,
    },
    rules::{
        set_execution_failure_suppression_threshold, DecisionEngine, DecisionInput,
        ExecutionFailureTracker, PerformanceModeDecision,
    },
    scheduler::{CpuUsageScheduler, Scheduler},
    smart_trim::{SmartTrimManager, SmartTrimSnapshot},
    suspension::{AppSuspensionManager, AppSuspensionSnapshot},
    thread_priority::{ThreadPriorityManager, ThreadPrioritySnapshot},
    timer_resolution::{TimerResolutionManager, TimerResolutionSnapshot},
    tray,
    windows_events::{WindowsAutomationEvent, WindowsEventWatcher},
};

const ACTIVE_PLAN_REFRESH_INTERVAL: Duration = Duration::from_secs(10);
const CPU_USAGE_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const ECO_QOS_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const APP_SUSPENSION_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const APP_SUSPENSION_FOREGROUND_RELEASE_INTERVAL: Duration = Duration::from_millis(500);
const APP_SUSPENSION_SHELL_USER_INTENT_INTERVAL: Duration = Duration::from_millis(750);
const CPU_AFFINITY_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const BACKGROUND_CPU_RESTRICTION_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const CPU_LIMITER_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const PERFORMANCE_MODE_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const FOREGROUND_RESPONSIVENESS_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const FOREGROUND_RESPONSIVENESS_FAST_REFRESH_INTERVAL: Duration = Duration::from_millis(250);
const FOREGROUND_RESPONSIVENESS_FAST_REFRESH_WINDOW: Duration = Duration::from_secs(8);
const CPU_PRIORITY_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const THREAD_PRIORITY_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const PRIORITY_BOOST_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const IO_PRIORITY_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const GPU_PRIORITY_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const MEMORY_PRIORITY_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const TIMER_RESOLUTION_REFRESH_INTERVAL: Duration = Duration::from_secs(3);
const PROCESS_APPEARANCE_SCAN_INTERVAL: Duration = Duration::from_secs(1);
const HIDDEN_AUTOMATION_REFRESH_INTERVAL: Duration = Duration::from_secs(10);
const VISIBLE_AUTOMATION_REFRESH_INTERVAL: Duration = Duration::from_secs(3);
const SWITCH_RETRY_INTERVAL: Duration = Duration::from_secs(15);

pub struct BackgroundAutomation {
    shared: Arc<SharedAutomationState>,
    thread: Mutex<Option<JoinHandle<()>>>,
    event_watcher: Mutex<Option<WindowsEventWatcher>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AutomationStatusSnapshot {
    pub generation: u64,
    pub eco_qos: EcoQosSnapshot,
    pub app_suspension: AppSuspensionSnapshot,
    pub cpu_affinity: CpuAffinitySnapshot,
    pub background_cpu_restriction: CpuAffinitySnapshot,
    pub cpu_limiter: CpuLimiterSnapshot,
    pub performance_mode: PerformanceModeSnapshot,
    pub foreground_responsiveness: ForegroundResponsivenessSnapshot,
    pub cpu_priority: CpuPrioritySnapshot,
    pub thread_priority: ThreadPrioritySnapshot,
    pub priority_boost: PriorityBoostSnapshot,
    pub io_priority: IoPrioritySnapshot,
    pub gpu_priority: GpuPrioritySnapshot,
    pub memory_priority: MemoryPrioritySnapshot,
    pub smart_trim: SmartTrimSnapshot,
    pub timer_resolution: TimerResolutionSnapshot,
    pub action_log_entries: Arc<Vec<ActionLogEntry>>,
    pub appearance_change_generation: u64,
}

struct SharedAutomationState {
    state: Mutex<AutomationWorkerState>,
    changed: Condvar,
    status_generation: AtomicU64,
    pending_auto_exclusions_generation: AtomicU64,
}

struct AutomationWorkerState {
    settings: Arc<Settings>,
    change_generation: u64,
    status_generation: u64,
    eco_qos_status: EcoQosSnapshot,
    app_suspension_status: AppSuspensionSnapshot,
    cpu_affinity_status: CpuAffinitySnapshot,
    background_cpu_restriction_status: CpuAffinitySnapshot,
    cpu_limiter_status: CpuLimiterSnapshot,
    performance_mode_status: PerformanceModeSnapshot,
    foreground_responsiveness_status: ForegroundResponsivenessSnapshot,
    cpu_priority_status: CpuPrioritySnapshot,
    thread_priority_status: ThreadPrioritySnapshot,
    priority_boost_status: PriorityBoostSnapshot,
    io_priority_status: IoPrioritySnapshot,
    gpu_priority_status: GpuPrioritySnapshot,
    memory_priority_status: MemoryPrioritySnapshot,
    smart_trim_status: SmartTrimSnapshot,
    timer_resolution_status: TimerResolutionSnapshot,
    action_log_entries: Arc<Vec<ActionLogEntry>>,
    appearance_change_generation: u64,
    pending_auto_exclusions: PendingAutoExclusions,
    app_suspension_freeze_requests: Vec<String>,
    smart_trim_now_requested: bool,
    action_log_clear_requested: bool,
    pending_events: AutomationWakeEvents,
    windows_event_watcher_active: bool,
    stop_requested: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PendingAutoExclusions {
    pub eco_qos: Vec<String>,
    pub app_suspension: Vec<String>,
    pub cpu_affinity: Vec<String>,
    pub background_cpu_restriction: Vec<String>,
    pub cpu_limiter: Vec<String>,
    pub foreground_responsiveness: Vec<String>,
    pub io_priority: Vec<String>,
    pub cpu_priority: Vec<String>,
    pub thread_priority: Vec<String>,
    pub priority_boost: Vec<String>,
    pub gpu_priority: Vec<String>,
    pub memory_priority: Vec<String>,
    pub smart_trim: Vec<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct AutomationWakeEvents {
    settings_changed: bool,
    foreground_changed: bool,
    window_created: bool,
    power_changed: bool,
    session_changed: bool,
    appearance_changed: bool,
    input_activity: bool,
    app_switch: bool,
    app_switch_mouse_click: bool,
}

impl AutomationWakeEvents {
    fn insert_windows_event(&mut self, event: WindowsAutomationEvent) {
        match event {
            WindowsAutomationEvent::ForegroundChanged => self.foreground_changed = true,
            WindowsAutomationEvent::WindowCreated => self.window_created = true,
            WindowsAutomationEvent::PowerChanged => self.power_changed = true,
            WindowsAutomationEvent::SessionChanged => self.session_changed = true,
            WindowsAutomationEvent::AppearanceChanged => self.appearance_changed = true,
        }
    }
}

impl BackgroundAutomation {
    pub fn start(settings: &Settings) -> Self {
        let shared = Arc::new(SharedAutomationState {
            state: Mutex::new(AutomationWorkerState {
                settings: Arc::new(settings.clone()),
                change_generation: 0,
                status_generation: 1,
                eco_qos_status: EcoQosSnapshot::default(),
                app_suspension_status: AppSuspensionSnapshot::default(),
                cpu_affinity_status: CpuAffinitySnapshot::default(),
                background_cpu_restriction_status: CpuAffinitySnapshot::default(),
                cpu_limiter_status: CpuLimiterSnapshot::default(),
                performance_mode_status: PerformanceModeSnapshot::default(),
                foreground_responsiveness_status: ForegroundResponsivenessSnapshot::default(),
                cpu_priority_status: CpuPrioritySnapshot::default(),
                thread_priority_status: ThreadPrioritySnapshot::default(),
                priority_boost_status: PriorityBoostSnapshot::default(),
                io_priority_status: IoPrioritySnapshot::default(),
                gpu_priority_status: GpuPrioritySnapshot::default(),
                memory_priority_status: MemoryPrioritySnapshot::default(),
                smart_trim_status: SmartTrimSnapshot::default(),
                timer_resolution_status: TimerResolutionSnapshot::default(),
                action_log_entries: Arc::new(Vec::new()),
                appearance_change_generation: 0,
                pending_auto_exclusions: PendingAutoExclusions::default(),
                app_suspension_freeze_requests: Vec::new(),
                smart_trim_now_requested: false,
                action_log_clear_requested: false,
                pending_events: AutomationWakeEvents::default(),
                windows_event_watcher_active: false,
                stop_requested: false,
            }),
            changed: Condvar::new(),
            status_generation: AtomicU64::new(1),
            pending_auto_exclusions_generation: AtomicU64::new(0),
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
            if state.settings.as_ref() == settings {
                return;
            }
            state.settings = Arc::new(settings.clone());
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

    pub fn status_snapshot_since(
        &self,
        observed_generation: u64,
    ) -> Option<AutomationStatusSnapshot> {
        if self.shared.status_generation.load(Ordering::Acquire) == observed_generation {
            return None;
        }

        self.shared.state.lock().ok().and_then(|state| {
            (state.status_generation != observed_generation).then(|| AutomationStatusSnapshot {
                generation: state.status_generation,
                eco_qos: state.eco_qos_status.clone(),
                app_suspension: state.app_suspension_status.clone(),
                cpu_affinity: state.cpu_affinity_status.clone(),
                background_cpu_restriction: state.background_cpu_restriction_status.clone(),
                cpu_limiter: state.cpu_limiter_status.clone(),
                performance_mode: state.performance_mode_status.clone(),
                foreground_responsiveness: state.foreground_responsiveness_status.clone(),
                cpu_priority: state.cpu_priority_status.clone(),
                thread_priority: state.thread_priority_status.clone(),
                priority_boost: state.priority_boost_status.clone(),
                io_priority: state.io_priority_status.clone(),
                gpu_priority: state.gpu_priority_status.clone(),
                memory_priority: state.memory_priority_status.clone(),
                smart_trim: state.smart_trim_status.clone(),
                timer_resolution: state.timer_resolution_status.clone(),
                action_log_entries: state.action_log_entries.clone(),
                appearance_change_generation: state.appearance_change_generation,
            })
        })
    }

    pub fn clear_action_log(&self) {
        if let Ok(mut state) = self.shared.state.lock() {
            state.action_log_entries = Arc::new(Vec::new());
            state.action_log_clear_requested = true;
            state.change_generation = state.change_generation.wrapping_add(1);
            self.shared.changed.notify_one();
        }
    }

    pub fn take_pending_auto_exclusions_since(
        &self,
        observed_generation: &mut u64,
    ) -> Option<PendingAutoExclusions> {
        if self
            .shared
            .pending_auto_exclusions_generation
            .load(Ordering::Acquire)
            == *observed_generation
        {
            return None;
        }

        self.shared.state.lock().ok().and_then(|mut state| {
            let generation = self
                .shared
                .pending_auto_exclusions_generation
                .load(Ordering::Acquire);
            if generation == *observed_generation {
                return None;
            }

            *observed_generation = generation;
            Some(std::mem::take(&mut state.pending_auto_exclusions))
        })
    }

    pub fn request_app_suspension_freeze(&self, process_name: &str) {
        let process_name = process_name_key(process_name);
        if process_name.is_empty() {
            return;
        }

        let mut settings_to_sync = None;
        if let Ok(mut state) = self.shared.state.lock() {
            state.app_suspension_freeze_requests.push(process_name);
            state.pending_events.settings_changed = true;
            state.change_generation = state.change_generation.wrapping_add(1);
            settings_to_sync = Some(Arc::clone(&state.settings));
            self.shared.changed.notify_one();
        }

        if let Some(settings) = settings_to_sync {
            self.sync_worker(settings.as_ref());
        }
    }

    pub fn request_smart_trim_now(&self) {
        let mut settings_to_sync = None;
        if let Ok(mut state) = self.shared.state.lock() {
            state.smart_trim_now_requested = true;
            state.change_generation = state.change_generation.wrapping_add(1);
            settings_to_sync = Some(Arc::clone(&state.settings));
            self.shared.changed.notify_one();
        }

        if let Some(settings) = settings_to_sync {
            self.sync_worker(settings.as_ref());
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
    let mut next_background_cpu_restriction_refresh = Instant::now();
    let mut next_cpu_limiter_refresh = Instant::now();
    let mut next_performance_mode_refresh = Instant::now();
    let mut next_foreground_responsiveness_refresh = Instant::now();
    let mut next_cpu_priority_refresh = Instant::now();
    let mut next_thread_priority_refresh = Instant::now();
    let mut next_priority_boost_refresh = Instant::now();
    let mut next_io_priority_refresh = Instant::now();
    let mut next_gpu_priority_refresh = Instant::now();
    let mut next_memory_priority_refresh = Instant::now();
    let mut next_smart_trim_refresh = Instant::now();
    let mut next_timer_resolution_refresh = Instant::now();
    let mut next_process_appearance_scan = Instant::now();
    let mut next_controller_activity_poll = Instant::now();
    let mut foreground_responsiveness_fast_until: Option<Instant> = None;

    while let Some(snapshot) = automation_snapshot(&shared) {
        let settings = snapshot.settings;
        let change_generation = snapshot.change_generation;
        let app_suspension_freeze_requests = snapshot.app_suspension_freeze_requests;
        let smart_trim_now_requested = snapshot.smart_trim_now_requested;
        if snapshot.action_log_clear_requested {
            runner.action_log.clear();
            runner.publish_action_log_if_changed(&shared);
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
        let background_cpu_restriction_refresh_interval = automation_refresh_interval(
            hidden_to_tray,
            BACKGROUND_CPU_RESTRICTION_REFRESH_INTERVAL,
        );
        let cpu_limiter_refresh_interval =
            automation_refresh_interval(hidden_to_tray, CPU_LIMITER_REFRESH_INTERVAL);
        let performance_mode_refresh_interval =
            automation_refresh_interval(hidden_to_tray, PERFORMANCE_MODE_REFRESH_INTERVAL);
        let mut foreground_responsiveness_refresh_interval =
            automation_refresh_interval(hidden_to_tray, FOREGROUND_RESPONSIVENESS_REFRESH_INTERVAL);
        let cpu_priority_refresh_interval =
            automation_refresh_interval(hidden_to_tray, CPU_PRIORITY_REFRESH_INTERVAL);
        let thread_priority_refresh_interval =
            automation_refresh_interval(hidden_to_tray, THREAD_PRIORITY_REFRESH_INTERVAL);
        let priority_boost_refresh_interval =
            automation_refresh_interval(hidden_to_tray, PRIORITY_BOOST_REFRESH_INTERVAL);
        let io_priority_refresh_interval =
            automation_refresh_interval(hidden_to_tray, IO_PRIORITY_REFRESH_INTERVAL);
        let gpu_priority_refresh_interval =
            automation_refresh_interval(hidden_to_tray, GPU_PRIORITY_REFRESH_INTERVAL);
        let memory_priority_refresh_interval =
            automation_refresh_interval(hidden_to_tray, MEMORY_PRIORITY_REFRESH_INTERVAL);
        let smart_trim_refresh_interval =
            automation_refresh_interval(hidden_to_tray, smart_trim_refresh_interval(&settings));
        let timer_resolution_refresh_interval =
            automation_refresh_interval(hidden_to_tray, TIMER_RESOLUTION_REFRESH_INTERVAL);
        let event_now = Instant::now();
        let settings_changed = wake_events.settings_changed || runner.note_settings(&settings);
        if settings_changed {
            next_check = event_now;
            next_eco_qos_refresh = event_now;
            next_app_suspension_refresh = event_now;
            next_app_suspension_foreground_release = event_now;
            next_cpu_affinity_refresh = event_now;
            next_background_cpu_restriction_refresh = event_now;
            next_cpu_limiter_refresh = event_now;
            next_performance_mode_refresh = event_now;
            next_foreground_responsiveness_refresh = event_now;
            next_cpu_priority_refresh = event_now;
            next_thread_priority_refresh = event_now;
            next_priority_boost_refresh = event_now;
            next_io_priority_refresh = event_now;
            next_gpu_priority_refresh = event_now;
            next_memory_priority_refresh = event_now;
            next_smart_trim_refresh = event_now;
            next_timer_resolution_refresh = event_now;
            next_process_appearance_scan = event_now;
            next_controller_activity_poll = event_now;
            foreground_responsiveness_fast_until = None;
        }
        if wake_events.foreground_changed || wake_events.session_changed {
            next_check = event_now;
            next_eco_qos_refresh = event_now;
            next_cpu_affinity_refresh = event_now;
            next_background_cpu_restriction_refresh = event_now;
            next_cpu_limiter_refresh = event_now;
            next_foreground_responsiveness_refresh = event_now;
            next_cpu_priority_refresh = event_now;
            next_thread_priority_refresh = event_now;
            next_priority_boost_refresh = event_now;
            next_io_priority_refresh = event_now;
            next_gpu_priority_refresh = event_now;
            next_memory_priority_refresh = event_now;
            next_smart_trim_refresh = event_now;
            next_timer_resolution_refresh = event_now;
            next_app_suspension_foreground_release = event_now;
            foreground_responsiveness_fast_until =
                foreground_responsiveness_fast_refresh_deadline(&settings, event_now);
        }
        if wake_events.window_created || wake_events.session_changed {
            next_process_appearance_scan = event_now;
            next_app_suspension_refresh = event_now;
            foreground_responsiveness_fast_until =
                foreground_responsiveness_fast_refresh_deadline(&settings, event_now);
        }
        if wake_events.power_changed || wake_events.session_changed {
            next_check = event_now;
            runner.refresh_active_plan();
        }
        if wake_events.input_activity {
            next_check = event_now;
        }
        let controller_poll_required =
            hidden_to_tray && controller_activity_poll_required(&settings);
        if controller_poll_required && event_now >= next_controller_activity_poll {
            if runner.poll_controller_activity(event_now) {
                next_check = event_now;
            }
            next_controller_activity_poll = event_now + CONTROLLER_ACTIVITY_POLL_INTERVAL;
        } else if !controller_poll_required {
            runner.clear_controller_activity();
            next_controller_activity_poll = event_now;
        }
        if wake_events.app_switch || wake_events.app_switch_mouse_click {
            next_app_suspension_foreground_release = event_now;
            next_timer_resolution_refresh = event_now;
            if runner.app_suspension_manager.has_suspended_processes() {
                let app_suspension_status = if wake_events.app_switch {
                    runner.run_app_suspension_app_switch_release()
                } else {
                    runner.run_app_suspension_shell_click_release()
                };
                if let Some(app_suspension_status) = app_suspension_status {
                    update_app_suspension_status(&shared, app_suspension_status);
                    runner.publish_action_log_if_changed(&shared);
                }
            }
        }
        let now = Instant::now();
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
        let background_cpu_restriction_refresh_required = settings_changed
            || feature_refresh_required(&settings, settings.background_cpu_restriction.enabled);
        let cpu_limiter_refresh_required =
            settings_changed || feature_refresh_required(&settings, settings.cpu_limiter.enabled);
        let performance_mode_refresh_required = settings_changed
            || feature_refresh_required(&settings, settings.performance_mode.enabled);
        let foreground_responsiveness_refresh_required = settings_changed
            || feature_refresh_required(&settings, settings.foreground_responsiveness.enabled);
        let cpu_priority_refresh_required =
            settings_changed || feature_refresh_required(&settings, settings.cpu_priority.enabled);
        let thread_priority_refresh_required = settings_changed
            || feature_refresh_required(&settings, settings.thread_priority.enabled);
        let priority_boost_refresh_required = settings_changed
            || feature_refresh_required(&settings, settings.priority_boost.enabled);
        let io_priority_refresh_required = settings_changed
            || feature_refresh_required(&settings, io_priority_required(&settings));
        let gpu_priority_refresh_required =
            settings_changed || feature_refresh_required(&settings, settings.gpu_priority.enabled);
        let memory_priority_refresh_required = settings_changed
            || feature_refresh_required(&settings, memory_priority_required(&settings));
        let smart_trim_refresh_required = settings_changed
            || smart_trim_now_requested
            || feature_refresh_required(&settings, settings.smart_trim.enabled);
        let timer_resolution_refresh_required = settings_changed
            || feature_refresh_required(&settings, settings.timer_resolution.enabled);
        if !app_suspension_freeze_requests.is_empty() {
            next_app_suspension_refresh = now;
        }
        if smart_trim_now_requested {
            next_smart_trim_refresh = now;
        }

        if foreground_responsiveness_fast_refresh_active(
            &settings,
            foreground_responsiveness_fast_until,
            now,
        ) {
            foreground_responsiveness_refresh_interval =
                FOREGROUND_RESPONSIVENESS_FAST_REFRESH_INTERVAL;
        }

        if scan_process_appearance && now >= next_process_appearance_scan {
            if runner.detect_process_appearance() {
                next_eco_qos_refresh = now;
                next_cpu_affinity_refresh = now;
                next_background_cpu_restriction_refresh = now;
                next_cpu_limiter_refresh = now;
                next_performance_mode_refresh = now;
                next_foreground_responsiveness_refresh = now;
                next_cpu_priority_refresh = now;
                next_thread_priority_refresh = now;
                next_priority_boost_refresh = now;
                next_io_priority_refresh = now;
                next_gpu_priority_refresh = now;
                next_memory_priority_refresh = now;
                next_smart_trim_refresh = now;
                foreground_responsiveness_fast_until =
                    foreground_responsiveness_fast_refresh_deadline(&settings, now);
            }
            next_process_appearance_scan = now + PROCESS_APPEARANCE_SCAN_INTERVAL;
        } else if !scan_process_appearance {
            runner.known_process_ids.clear();
            next_process_appearance_scan = now + PROCESS_APPEARANCE_SCAN_INTERVAL;
        }

        if runner.app_suspension_manager.has_suspended_processes()
            && now >= next_app_suspension_foreground_release
        {
            if let Some(app_suspension_status) = runner.run_app_suspension_foreground_release() {
                update_app_suspension_status(&shared, app_suspension_status);
                runner.publish_action_log_if_changed(&shared);
            }
            next_app_suspension_foreground_release =
                now + APP_SUSPENSION_FOREGROUND_RELEASE_INTERVAL;
        }

        if eco_qos_refresh_required && now >= next_eco_qos_refresh {
            let eco_qos_status = runner.run_eco_qos_update(&settings);
            update_eco_qos_status(&shared, eco_qos_status);
            runner.publish_action_log_if_changed(&shared);
            next_eco_qos_refresh = now + eco_qos_refresh_interval;
        }
        if foreground_responsiveness_refresh_required
            && now >= next_foreground_responsiveness_refresh
        {
            let foreground_responsiveness_status =
                runner.run_foreground_responsiveness_update(&settings);
            if foreground_responsiveness_status
                .foreground_boosted_process
                .is_some()
                || foreground_responsiveness_status.auto_balanced_processes > 0
            {
                foreground_responsiveness_fast_until =
                    foreground_responsiveness_fast_refresh_deadline(&settings, now);
            }
            update_foreground_responsiveness_status(&shared, foreground_responsiveness_status);
            runner.publish_action_log_if_changed(&shared);
            next_foreground_responsiveness_refresh =
                now + foreground_responsiveness_refresh_interval;
        }
        if io_priority_refresh_required && now >= next_io_priority_refresh {
            let io_priority_status = runner.run_io_priority_update(&settings);
            update_io_priority_status(&shared, io_priority_status);
            runner.publish_action_log_if_changed(&shared);
            next_io_priority_refresh = now + io_priority_refresh_interval;
        }
        if cpu_priority_refresh_required && now >= next_cpu_priority_refresh {
            let cpu_priority_status = runner.run_cpu_priority_update(&settings);
            update_cpu_priority_status(&shared, cpu_priority_status);
            runner.publish_action_log_if_changed(&shared);
            next_cpu_priority_refresh = now + cpu_priority_refresh_interval;
        }
        if thread_priority_refresh_required && now >= next_thread_priority_refresh {
            let thread_priority_status = runner.run_thread_priority_update(&settings);
            update_thread_priority_status(&shared, thread_priority_status);
            runner.publish_action_log_if_changed(&shared);
            next_thread_priority_refresh = now + thread_priority_refresh_interval;
        }
        if priority_boost_refresh_required && now >= next_priority_boost_refresh {
            let priority_boost_status = runner.run_priority_boost_update(&settings);
            update_priority_boost_status(&shared, priority_boost_status);
            runner.publish_action_log_if_changed(&shared);
            next_priority_boost_refresh = now + priority_boost_refresh_interval;
        }
        if gpu_priority_refresh_required && now >= next_gpu_priority_refresh {
            let gpu_priority_status = runner.run_gpu_priority_update(&settings);
            update_gpu_priority_status(&shared, gpu_priority_status);
            runner.publish_action_log_if_changed(&shared);
            next_gpu_priority_refresh = now + gpu_priority_refresh_interval;
        }
        if memory_priority_refresh_required && now >= next_memory_priority_refresh {
            let memory_priority_status = runner.run_memory_priority_update(&settings);
            update_memory_priority_status(&shared, memory_priority_status);
            runner.publish_action_log_if_changed(&shared);
            next_memory_priority_refresh = now + memory_priority_refresh_interval;
        }
        if app_suspension_refresh_required && now >= next_app_suspension_refresh {
            let app_suspension_status =
                runner.run_app_suspension_update(&settings, &app_suspension_freeze_requests);
            update_app_suspension_status(&shared, app_suspension_status);
            runner.publish_action_log_if_changed(&shared);
            next_app_suspension_refresh = now + app_suspension_refresh_interval;
            if runner.app_suspension_manager.has_suspended_processes() {
                next_app_suspension_foreground_release = now;
            }
        }
        if cpu_affinity_refresh_required && now >= next_cpu_affinity_refresh {
            let cpu_affinity_status = runner.run_cpu_affinity_update(&settings);
            update_cpu_affinity_status(&shared, cpu_affinity_status);
            runner.publish_action_log_if_changed(&shared);
            next_cpu_affinity_refresh = now + cpu_affinity_refresh_interval;
        }
        if background_cpu_restriction_refresh_required
            && now >= next_background_cpu_restriction_refresh
        {
            let status = runner.run_background_cpu_restriction_update(&settings);
            update_background_cpu_restriction_status(&shared, status);
            runner.publish_action_log_if_changed(&shared);
            next_background_cpu_restriction_refresh =
                now + background_cpu_restriction_refresh_interval;
        }
        if cpu_limiter_refresh_required && now >= next_cpu_limiter_refresh {
            let cpu_limiter_status = runner.run_cpu_limiter_update(&settings);
            update_cpu_limiter_status(&shared, cpu_limiter_status);
            runner.publish_action_log_if_changed(&shared);
            next_cpu_limiter_refresh = now + cpu_limiter_refresh_interval;
        }
        if performance_mode_refresh_required && now >= next_performance_mode_refresh {
            let performance_mode_status = runner.run_performance_mode_update(&settings);
            update_performance_mode_status(&shared, performance_mode_status);
            runner.publish_action_log_if_changed(&shared);
            next_performance_mode_refresh = now + performance_mode_refresh_interval;
        }
        if smart_trim_refresh_required && now >= next_smart_trim_refresh {
            let smart_trim_status = if smart_trim_now_requested {
                runner.run_smart_trim_now(&settings)
            } else {
                runner.run_smart_trim_update(&settings)
            };
            update_smart_trim_status(&shared, smart_trim_status);
            runner.publish_action_log_if_changed(&shared);
            next_smart_trim_refresh = now + smart_trim_refresh_interval;
        }
        if timer_resolution_refresh_required && now >= next_timer_resolution_refresh {
            let timer_resolution_status = runner.run_timer_resolution_update(&settings);
            update_timer_resolution_status(&shared, timer_resolution_status);
            runner.publish_action_log_if_changed(&shared);
            next_timer_resolution_refresh = now + timer_resolution_refresh_interval;
        }

        let wait_now = Instant::now();
        let mut wait_for = if hidden_to_tray {
            if power_plan_checks_required {
                let input_events = input_hook::take_pending_events();
                if input_hook_should_check(&settings, input_events) {
                    next_check = wait_now;
                }

                if wait_now >= next_check && !runner.performance_mode_manager.is_active() {
                    runner.run_check(&settings);
                }

                if let Some(delay) =
                    hidden_power_plan_check_delay(&settings, windows_event_watcher_active)
                {
                    next_check = wait_now + delay;
                    Some(next_check.saturating_duration_since(wait_now))
                } else {
                    next_check = wait_now;
                    None
                }
            } else {
                next_check = wait_now;
                None
            }
        } else {
            next_check = wait_now;
            None
        };

        if eco_qos_refresh_required {
            wait_for = Some(min_worker_wait(
                wait_for,
                next_eco_qos_refresh
                    .saturating_duration_since(wait_now)
                    .min(eco_qos_refresh_interval),
            ));
        }
        if app_suspension_refresh_required {
            wait_for = Some(min_worker_wait(
                wait_for,
                next_app_suspension_refresh
                    .saturating_duration_since(wait_now)
                    .min(app_suspension_refresh_interval),
            ));
        }
        if cpu_affinity_refresh_required {
            wait_for = Some(min_worker_wait(
                wait_for,
                next_cpu_affinity_refresh
                    .saturating_duration_since(wait_now)
                    .min(cpu_affinity_refresh_interval),
            ));
        }
        if background_cpu_restriction_refresh_required {
            wait_for = Some(min_worker_wait(
                wait_for,
                next_background_cpu_restriction_refresh
                    .saturating_duration_since(wait_now)
                    .min(background_cpu_restriction_refresh_interval),
            ));
        }
        if cpu_limiter_refresh_required {
            wait_for = Some(min_worker_wait(
                wait_for,
                next_cpu_limiter_refresh
                    .saturating_duration_since(wait_now)
                    .min(cpu_limiter_refresh_interval),
            ));
        }
        if performance_mode_refresh_required {
            wait_for = Some(min_worker_wait(
                wait_for,
                next_performance_mode_refresh
                    .saturating_duration_since(wait_now)
                    .min(performance_mode_refresh_interval),
            ));
        }
        if foreground_responsiveness_refresh_required {
            wait_for = Some(min_worker_wait(
                wait_for,
                next_foreground_responsiveness_refresh
                    .saturating_duration_since(wait_now)
                    .min(foreground_responsiveness_refresh_interval),
            ));
        }
        if io_priority_refresh_required {
            wait_for = Some(min_worker_wait(
                wait_for,
                next_io_priority_refresh
                    .saturating_duration_since(wait_now)
                    .min(io_priority_refresh_interval),
            ));
        }
        if gpu_priority_refresh_required {
            wait_for = Some(min_worker_wait(
                wait_for,
                next_gpu_priority_refresh
                    .saturating_duration_since(wait_now)
                    .min(gpu_priority_refresh_interval),
            ));
        }
        if memory_priority_refresh_required {
            wait_for = Some(min_worker_wait(
                wait_for,
                next_memory_priority_refresh
                    .saturating_duration_since(wait_now)
                    .min(memory_priority_refresh_interval),
            ));
        }
        if smart_trim_refresh_required {
            wait_for = Some(min_worker_wait(
                wait_for,
                next_smart_trim_refresh
                    .saturating_duration_since(wait_now)
                    .min(smart_trim_refresh_interval),
            ));
        }
        if timer_resolution_refresh_required {
            wait_for = Some(min_worker_wait(
                wait_for,
                next_timer_resolution_refresh
                    .saturating_duration_since(wait_now)
                    .min(timer_resolution_refresh_interval),
            ));
        }
        if scan_process_appearance {
            wait_for = Some(min_worker_wait(
                wait_for,
                next_process_appearance_scan
                    .saturating_duration_since(wait_now)
                    .min(PROCESS_APPEARANCE_SCAN_INTERVAL),
            ));
        }
        if controller_poll_required {
            wait_for = Some(min_worker_wait(
                wait_for,
                next_controller_activity_poll
                    .saturating_duration_since(wait_now)
                    .min(CONTROLLER_ACTIVITY_POLL_INTERVAL),
            ));
        }
        if runner.app_suspension_manager.has_suspended_processes() {
            wait_for = Some(min_worker_wait(
                wait_for,
                next_app_suspension_foreground_release
                    .saturating_duration_since(wait_now)
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
    settings: Arc<Settings>,
    change_generation: u64,
    app_suspension_freeze_requests: Vec<String>,
    smart_trim_now_requested: bool,
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
            smart_trim_now_requested: std::mem::take(&mut state.smart_trim_now_requested),
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

        if event == WindowsAutomationEvent::AppearanceChanged {
            state.appearance_change_generation = state.appearance_change_generation.wrapping_add(1);
            bump_status_generation(shared, &mut state);
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

        if input_hook_should_check_activity(&state.settings, events) {
            state.pending_events.input_activity = true;
        }
        if input_hook_should_check_app_switch(&state.settings, events) {
            state.pending_events.app_switch = true;
        }
        if input_hook_should_check_app_switch_mouse_click(&state.settings, events) {
            state.pending_events.app_switch_mouse_click = true;
        }
        state.change_generation = state.change_generation.wrapping_add(1);
        shared.changed.notify_one();
    }
}

fn update_eco_qos_status(shared: &SharedAutomationState, status: EcoQosSnapshot) {
    update_status_with_auto_exclusions(
        shared,
        status.clone(),
        &status.auto_excluded_processes,
        |pending| &mut pending.eco_qos,
        |state| &mut state.eco_qos_status,
    );
}

fn update_app_suspension_status(shared: &SharedAutomationState, status: AppSuspensionSnapshot) {
    update_status_with_auto_exclusions(
        shared,
        status.clone(),
        &status.auto_excluded_processes,
        |pending| &mut pending.app_suspension,
        |state| &mut state.app_suspension_status,
    );
}

fn update_cpu_affinity_status(shared: &SharedAutomationState, status: CpuAffinitySnapshot) {
    update_status_with_auto_exclusions(
        shared,
        status.clone(),
        &status.auto_excluded_processes,
        |pending| &mut pending.cpu_affinity,
        |state| &mut state.cpu_affinity_status,
    );
}

fn update_background_cpu_restriction_status(
    shared: &SharedAutomationState,
    status: CpuAffinitySnapshot,
) {
    update_status_with_auto_exclusions(
        shared,
        status.clone(),
        &status.auto_excluded_processes,
        |pending| &mut pending.background_cpu_restriction,
        |state| &mut state.background_cpu_restriction_status,
    );
}

fn append_unique_process_names(target: &mut Vec<String>, names: &[String]) -> bool {
    let old_len = target.len();
    for name in names {
        let name = process_name_key(name);
        if !name.is_empty()
            && !target
                .iter()
                .any(|existing| existing.eq_ignore_ascii_case(&name))
        {
            target.push(name);
        }
    }
    target.len() != old_len
}

fn update_cpu_limiter_status(shared: &SharedAutomationState, status: CpuLimiterSnapshot) {
    update_status_with_auto_exclusions(
        shared,
        status.clone(),
        &status.auto_excluded_processes,
        |pending| &mut pending.cpu_limiter,
        |state| &mut state.cpu_limiter_status,
    );
}

fn update_performance_mode_status(shared: &SharedAutomationState, status: PerformanceModeSnapshot) {
    update_status(shared, status, |state| &mut state.performance_mode_status);
}

fn update_foreground_responsiveness_status(
    shared: &SharedAutomationState,
    status: ForegroundResponsivenessSnapshot,
) {
    update_status_with_auto_exclusions(
        shared,
        status.clone(),
        &status.auto_excluded_processes,
        |pending| &mut pending.foreground_responsiveness,
        |state| &mut state.foreground_responsiveness_status,
    );
}

fn update_io_priority_status(shared: &SharedAutomationState, status: IoPrioritySnapshot) {
    update_status_with_auto_exclusions(
        shared,
        status.clone(),
        &status.auto_excluded_processes,
        |pending| &mut pending.io_priority,
        |state| &mut state.io_priority_status,
    );
}

fn update_cpu_priority_status(shared: &SharedAutomationState, status: CpuPrioritySnapshot) {
    update_status_with_auto_exclusions(
        shared,
        status.clone(),
        &status.auto_excluded_processes,
        |pending| &mut pending.cpu_priority,
        |state| &mut state.cpu_priority_status,
    );
}

fn update_thread_priority_status(shared: &SharedAutomationState, status: ThreadPrioritySnapshot) {
    update_status_with_auto_exclusions(
        shared,
        status.clone(),
        &status.auto_excluded_processes,
        |pending| &mut pending.thread_priority,
        |state| &mut state.thread_priority_status,
    );
}

fn update_priority_boost_status(shared: &SharedAutomationState, status: PriorityBoostSnapshot) {
    update_status_with_auto_exclusions(
        shared,
        status.clone(),
        &status.auto_excluded_processes,
        |pending| &mut pending.priority_boost,
        |state| &mut state.priority_boost_status,
    );
}

fn update_gpu_priority_status(shared: &SharedAutomationState, status: GpuPrioritySnapshot) {
    update_status_with_auto_exclusions(
        shared,
        status.clone(),
        &status.auto_excluded_processes,
        |pending| &mut pending.gpu_priority,
        |state| &mut state.gpu_priority_status,
    );
}

fn update_memory_priority_status(shared: &SharedAutomationState, status: MemoryPrioritySnapshot) {
    update_status_with_auto_exclusions(
        shared,
        status.clone(),
        &status.auto_excluded_processes,
        |pending| &mut pending.memory_priority,
        |state| &mut state.memory_priority_status,
    );
}

fn update_smart_trim_status(shared: &SharedAutomationState, status: SmartTrimSnapshot) {
    update_status_with_auto_exclusions(
        shared,
        status.clone(),
        &status.auto_excluded_processes,
        |pending| &mut pending.smart_trim,
        |state| &mut state.smart_trim_status,
    );
}

fn update_timer_resolution_status(shared: &SharedAutomationState, status: TimerResolutionSnapshot) {
    update_status(shared, status, |state| &mut state.timer_resolution_status);
}

fn update_status<T: PartialEq>(
    shared: &SharedAutomationState,
    status: T,
    field: impl for<'a> FnOnce(&'a mut AutomationWorkerState) -> &'a mut T,
) {
    if let Ok(mut state) = shared.state.lock() {
        if set_status(field(&mut state), status) {
            bump_status_generation(shared, &mut state);
        }
    }
}

fn update_status_with_auto_exclusions<T: PartialEq>(
    shared: &SharedAutomationState,
    status: T,
    auto_excluded_processes: &[String],
    pending_field: impl for<'a> FnOnce(&'a mut PendingAutoExclusions) -> &'a mut Vec<String>,
    status_field: impl for<'a> FnOnce(&'a mut AutomationWorkerState) -> &'a mut T,
) {
    if let Ok(mut state) = shared.state.lock() {
        if append_unique_process_names(
            pending_field(&mut state.pending_auto_exclusions),
            auto_excluded_processes,
        ) {
            shared
                .pending_auto_exclusions_generation
                .fetch_add(1, Ordering::Release);
        }
        if set_status(status_field(&mut state), status) {
            bump_status_generation(shared, &mut state);
        }
    }
}

fn update_action_log_entries(shared: &SharedAutomationState, entries: Vec<ActionLogEntry>) {
    if let Ok(mut state) = shared.state.lock() {
        let entries = Arc::new(entries);
        if state.action_log_entries != entries {
            state.action_log_entries = entries;
            bump_status_generation(shared, &mut state);
        }
    }
}

fn set_status<T: PartialEq>(current: &mut T, next: T) -> bool {
    if *current != next {
        *current = next;
        true
    } else {
        false
    }
}

fn bump_status_generation(shared: &SharedAutomationState, state: &mut AutomationWorkerState) {
    state.status_generation = state.status_generation.wrapping_add(1);
    shared
        .status_generation
        .store(state.status_generation, Ordering::Release);
}

fn automation_refresh_interval(hidden_to_tray: bool, hidden_interval: Duration) -> Duration {
    if hidden_to_tray {
        hidden_interval.max(HIDDEN_AUTOMATION_REFRESH_INTERVAL)
    } else {
        VISIBLE_AUTOMATION_REFRESH_INTERVAL
    }
}

fn smart_trim_refresh_interval(settings: &Settings) -> Duration {
    Duration::from_secs(
        settings
            .smart_trim
            .check_interval_minutes
            .max(1)
            .saturating_mul(60),
    )
}

fn foreground_responsiveness_fast_refresh_deadline(
    settings: &Settings,
    now: Instant,
) -> Option<Instant> {
    feature_refresh_required(settings, settings.foreground_responsiveness.enabled)
        .then_some(now + FOREGROUND_RESPONSIVENESS_FAST_REFRESH_WINDOW)
}

fn foreground_responsiveness_fast_refresh_active(
    settings: &Settings,
    fast_until: Option<Instant>,
    now: Instant,
) -> bool {
    feature_refresh_required(settings, settings.foreground_responsiveness.enabled)
        && fast_until.is_some_and(|until| now < until)
}

fn feature_refresh_required(settings: &Settings, feature_enabled: bool) -> bool {
    settings.general.enabled && feature_enabled
}

fn io_priority_required(settings: &Settings) -> bool {
    settings.io_priority.enabled
        || (settings.foreground_responsiveness.enabled
            && (settings
                .foreground_responsiveness
                .lower_background_io_priority_enabled
                || settings.foreground_responsiveness.auto_balance_enabled))
}

fn memory_priority_required(settings: &Settings) -> bool {
    settings.memory_priority.enabled
}

fn timer_resolution_required(settings: &Settings) -> bool {
    settings.timer_resolution.enabled
}

fn effective_io_priority_settings(
    settings: &Settings,
    launch_boost_active: bool,
) -> crate::config::IoPrioritySettings {
    let mut io_priority = settings.io_priority.clone();
    if launch_boost_active {
        io_priority.enabled = true;
        io_priority.foreground_detection_enabled = true;
        io_priority.foreground_priority = ProcessIoPriority::Normal.into();
        io_priority.background_priority = ProcessIoPriority::VeryLow.into();
    } else if settings.foreground_responsiveness.enabled
        && settings
            .foreground_responsiveness
            .lower_background_io_priority_enabled
    {
        io_priority.enabled = true;
        io_priority.foreground_detection_enabled = true;
        io_priority.foreground_priority = ProcessIoPriority::Normal.into();
        io_priority.background_priority = settings
            .foreground_responsiveness
            .lower_background_io_priority
            .into();
    }
    io_priority
}

fn effective_memory_priority_settings(
    settings: &Settings,
) -> crate::config::MemoryPrioritySettings {
    settings.memory_priority.clone()
}

fn process_appearance_scan_required(settings: &Settings) -> bool {
    settings.general.enabled
        && (settings.eco_qos.enabled
            || settings.cpu_affinity.enabled
            || settings.background_cpu_restriction.enabled
            || settings.cpu_limiter.enabled
            || settings.performance_mode.enabled
            || settings.foreground_responsiveness.enabled
            || settings.cpu_priority.enabled
            || settings.thread_priority.enabled
            || settings.priority_boost.enabled
            || io_priority_required(settings)
            || settings.gpu_priority.enabled
            || memory_priority_required(settings)
            || settings.smart_trim.enabled)
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
            || settings.background_cpu_restriction.enabled
            || settings.cpu_limiter.enabled
            || settings.performance_mode.enabled
            || settings.foreground_responsiveness.enabled
            || settings.cpu_priority.enabled
            || settings.thread_priority.enabled
            || settings.priority_boost.enabled
            || io_priority_required(settings)
            || settings.gpu_priority.enabled
            || memory_priority_required(settings)
            || settings.smart_trim.enabled
            || timer_resolution_required(settings))
}

fn windows_event_watcher_required(settings: &Settings) -> bool {
    appearance_events_required(settings)
        || (settings.general.enabled
            && (power_plan_checks_required(settings)
                || settings.app_suspension.enabled
                || process_appearance_scan_required(settings)))
}

fn windows_event_wake_required(settings: &Settings, event: WindowsAutomationEvent) -> bool {
    if event == WindowsAutomationEvent::AppearanceChanged {
        return appearance_events_required(settings);
    }

    if settings.general.enabled {
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
            WindowsAutomationEvent::AppearanceChanged => false,
        }
    } else {
        false
    }
}

fn appearance_events_required(settings: &Settings) -> bool {
    settings.general.theme_mode == AppThemeMode::System
        || settings.general.accent.source == AccentColorSource::Windows
        || settings.general.animation_mode == AnimationMode::System
}

fn activity_power_plan_required(settings: &Settings) -> bool {
    settings.activity_mode.enabled
        && (has_idle_plan(&settings.activity_mode.power_plans, settings)
            || (settings.activity_mode.switch_to_performance_on_resume
                && settings.activity_mode.input_detection.any_enabled()
                && has_active_plan(&settings.activity_mode.power_plans, settings)))
}

fn controller_activity_poll_required(settings: &Settings) -> bool {
    settings.general.enabled
        && settings.activity_mode.enabled
        && settings.activity_mode.input_detection.controller
        && (has_idle_plan(&settings.activity_mode.power_plans, settings)
            || has_active_plan(&settings.activity_mode.power_plans, settings))
}

fn foreground_rules_required(settings: &Settings) -> bool {
    settings.foreground_rules.enabled
        && (settings
            .foreground_rules
            .rules
            .iter()
            .any(|rule| rule.enabled && rule.power_plan_guid.is_some()))
}

fn foreground_lookup_required(settings: &Settings) -> bool {
    settings.foreground_rules.enabled && !settings.foreground_rules.rules.is_empty()
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
    input_hook_should_check_activity(settings, events)
        || input_hook_should_check_app_switch(settings, events)
        || input_hook_should_check_app_switch_mouse_click(settings, events)
}

fn input_hook_should_check_activity(settings: &Settings, events: InputHookEvents) -> bool {
    settings.general.enabled
        && settings.activity_mode.enabled
        && ((events.keyboard && settings.activity_mode.input_detection.keyboard)
            || (events.mouse && settings.activity_mode.input_detection.mouse))
}

fn input_hook_should_check_app_switch(settings: &Settings, events: InputHookEvents) -> bool {
    settings.general.enabled && settings.app_suspension.enabled && events.app_switch
}

fn input_hook_should_check_app_switch_mouse_click(
    settings: &Settings,
    events: InputHookEvents,
) -> bool {
    settings.general.enabled && settings.app_suspension.enabled && events.mouse_click
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
    switch_failure_suppression: ExecutionFailureTracker,
    power: PowerPlanManager,
    cpu_usage: CpuUsageSnapshot,
    next_cpu_usage_refresh: Option<Instant>,
    cpu_monitor: CpuUsageMonitor,
    idle_detector: IdleDetector,
    controller_activity_detector: ControllerActivityDetector,
    foreground_detector: ForegroundDetector,
    scheduler: Scheduler,
    cpu_usage_scheduler: CpuUsageScheduler,
    eco_qos_manager: EcoQosManager,
    app_suspension_manager: AppSuspensionManager,
    last_app_suspension_shell_user_intent: Option<Instant>,
    cpu_affinity_manager: CpuAffinityManager,
    background_cpu_restriction_manager: BackgroundCpuRestrictionManager,
    cpu_limiter_manager: CpuLimiterManager,
    performance_mode_manager: PerformanceModeManager,
    action_log: ActionLog,
    foreground_responsiveness_manager: ForegroundResponsivenessManager,
    launch_boost_active: bool,
    cpu_priority_manager: CpuPriorityManager,
    thread_priority_manager: ThreadPriorityManager,
    priority_boost_manager: PriorityBoostManager,
    io_priority_manager: IoPriorityManager,
    gpu_priority_manager: GpuPriorityManager,
    memory_priority_manager: MemoryPriorityManager,
    smart_trim_manager: SmartTrimManager,
    timer_resolution_manager: TimerResolutionManager,
    known_process_ids: BTreeSet<u32>,
    published_action_log_sequence: Option<u64>,
}

impl HiddenAutomationRunner {
    fn note_settings(&mut self, settings: &Settings) -> bool {
        self.action_log.set_mode(settings.advanced.action_log_mode);
        set_execution_failure_suppression_threshold(
            settings.advanced.execution_failure_suppression_threshold(),
        );

        let changed = self.last_settings.as_ref() != Some(settings);
        if changed {
            self.last_settings = Some(settings.clone());
            self.switch_failure_suppression.clear();
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

    fn poll_controller_activity(&mut self, now: Instant) -> bool {
        self.controller_activity_detector.poll(now)
    }

    fn clear_controller_activity(&mut self) {
        self.controller_activity_detector.clear();
    }

    fn publish_action_log_if_changed(&mut self, shared: &SharedAutomationState) {
        let latest_sequence = self.action_log.latest_sequence();
        if self.published_action_log_sequence == latest_sequence {
            return;
        }

        update_action_log_entries(shared, self.action_log.entries());
        self.published_action_log_sequence = latest_sequence;
    }

    fn activity_snapshot(
        &self,
        settings: &Settings,
        now: Instant,
    ) -> crate::activity::ActivitySnapshot {
        let idle_timeout = Duration::from_secs(settings.activity_mode.idle_timeout_seconds);
        let snapshot = self.idle_detector.snapshot(idle_timeout);
        let controller_idle_for = settings
            .activity_mode
            .input_detection
            .controller
            .then(|| self.controller_activity_detector.idle_for(now))
            .flatten();

        merge_activity_snapshot(snapshot, controller_idle_for, idle_timeout)
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

    fn run_app_suspension_app_switch_release(&mut self) -> Option<AppSuspensionSnapshot> {
        self.app_suspension_manager
            .release_window_owner_processes_for_user_intent(
                &top_level_window_process_ids(),
                &mut self.action_log,
            )
    }

    fn run_app_suspension_shell_click_release(&mut self) -> Option<AppSuspensionSnapshot> {
        if !self.foreground_detector.cursor_is_shell_window() {
            return None;
        }

        self.run_app_suspension_app_switch_release()
    }

    fn app_suspension_shell_user_intent_due(&self, now: Instant) -> bool {
        self.last_app_suspension_shell_user_intent
            .is_none_or(|last| {
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

    fn run_background_cpu_restriction_update(
        &mut self,
        settings: &Settings,
    ) -> CpuAffinitySnapshot {
        self.background_cpu_restriction_manager.update(
            &settings.background_cpu_restriction,
            settings.general.enabled,
            self.foreground_detector.process_id(),
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
        self.performance_mode_manager.update(
            &settings.performance_mode,
            &settings.power_plans,
            settings.general.enabled,
            &mut self.action_log,
        )
    }

    fn run_foreground_responsiveness_update(
        &mut self,
        settings: &Settings,
    ) -> ForegroundResponsivenessSnapshot {
        self.refresh_cpu_usage();
        let foreground_process_id = self.foreground_detector.process_id();
        let mut excluded_process_ids = self.eco_qos_manager.throttled_process_ids();
        excluded_process_ids.extend(self.performance_mode_manager.active_process_ids());
        let snapshot = self.foreground_responsiveness_manager.update(
            ForegroundResponsivenessUpdate {
                settings: &settings.foreground_responsiveness,
                automation_enabled: settings.general.enabled,
                foreground_process_id,
                total_cpu_usage_percent: self.cpu_usage.percent,
                background_efficiency_managed: settings.eco_qos.enabled,
                eco_qos_process_ids: &excluded_process_ids,
            },
            &mut self.action_log,
        );
        self.launch_boost_active = snapshot.launch_boost_active;
        snapshot
    }

    fn run_io_priority_update(&mut self, settings: &Settings) -> IoPrioritySnapshot {
        let io_priority_settings =
            effective_io_priority_settings(settings, self.launch_boost_active);
        self.io_priority_manager.update(
            &io_priority_settings,
            settings.general.enabled,
            self.foreground_detector.process_id(),
            &mut self.action_log,
        )
    }

    fn run_cpu_priority_update(&mut self, settings: &Settings) -> CpuPrioritySnapshot {
        self.cpu_priority_manager.update(
            &settings.cpu_priority,
            settings.general.enabled,
            self.foreground_detector.process_id(),
            &mut self.action_log,
        )
    }

    fn run_thread_priority_update(&mut self, settings: &Settings) -> ThreadPrioritySnapshot {
        self.thread_priority_manager.update(
            &settings.thread_priority,
            settings.general.enabled,
            self.foreground_detector.process_id(),
            &mut self.action_log,
        )
    }

    fn run_priority_boost_update(&mut self, settings: &Settings) -> PriorityBoostSnapshot {
        self.priority_boost_manager.update(
            &settings.priority_boost,
            settings.general.enabled,
            self.foreground_detector.process_id(),
            &mut self.action_log,
        )
    }

    fn run_gpu_priority_update(&mut self, settings: &Settings) -> GpuPrioritySnapshot {
        self.gpu_priority_manager.update(
            &settings.gpu_priority,
            settings.general.enabled,
            self.foreground_detector.process_id(),
            &mut self.action_log,
        )
    }

    fn run_memory_priority_update(&mut self, settings: &Settings) -> MemoryPrioritySnapshot {
        let memory_priority_settings = effective_memory_priority_settings(settings);
        self.memory_priority_manager.update_rules(
            &memory_priority_settings,
            settings.general.enabled,
            self.foreground_detector.process_id(),
            &mut self.action_log,
        )
    }

    fn run_smart_trim_update(&mut self, settings: &Settings) -> SmartTrimSnapshot {
        self.smart_trim_manager.update(
            &settings.smart_trim,
            settings.general.enabled,
            self.foreground_detector.process_id(),
            self.performance_mode_manager.is_active(),
            &mut self.action_log,
        )
    }

    fn run_smart_trim_now(&mut self, settings: &Settings) -> SmartTrimSnapshot {
        self.smart_trim_manager.trim_now(
            &settings.smart_trim,
            settings.general.enabled,
            self.foreground_detector.process_id(),
            self.performance_mode_manager.is_active(),
            &mut self.action_log,
        )
    }

    fn run_timer_resolution_update(&mut self, settings: &Settings) -> TimerResolutionSnapshot {
        let foreground_process_name = self.foreground_detector.process_name();
        self.timer_resolution_manager.update(
            &settings.timer_resolution,
            settings.general.enabled,
            foreground_process_name.as_deref(),
            &mut self.action_log,
        )
    }

    fn run_check(&mut self, settings: &Settings) {
        let should_refresh_active_plan = self
            .next_active_plan_refresh
            .is_none_or(|refresh_at| Instant::now() >= refresh_at);
        if should_refresh_active_plan {
            self.refresh_active_plan();
        }

        let activity = self.activity_snapshot(settings, Instant::now());
        self.refresh_cpu_usage();
        let foreground_app = foreground_lookup_required(settings)
            .then(|| self.foreground_detector.process_name())
            .flatten();
        let schedule = self.scheduler.current_decision(&settings.schedule_mode);
        let cpu_usage_decision = self
            .cpu_usage_scheduler
            .current_decision(&settings.cpu_usage_mode, self.cpu_usage.percent);
        let decision_input = DecisionInput {
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
        };
        let decision = DecisionEngine.decide(settings, decision_input);
        self.apply_power_plan_guid(decision.target_guid.as_deref());
    }

    fn refresh_active_plan(&mut self) {
        self.next_active_plan_refresh = Some(Instant::now() + ACTIVE_PLAN_REFRESH_INTERVAL);

        if let Ok(Some(active)) = self.power.active_plan() {
            self.current_guid = Some(active.guid);
        }
    }

    fn refresh_cpu_usage(&mut self) {
        if self
            .next_cpu_usage_refresh
            .is_none_or(|refresh_at| Instant::now() >= refresh_at)
        {
            self.cpu_usage = self.cpu_monitor.sample_usage();
            self.next_cpu_usage_refresh = Some(Instant::now() + CPU_USAGE_REFRESH_INTERVAL);
        }
    }

    fn apply_power_plan_guid(&mut self, plan_guid: Option<&str>) {
        let Some(plan_guid) = plan_guid else {
            return;
        };

        let already_active = self
            .current_guid
            .as_deref()
            .is_some_and(|guid| guid.eq_ignore_ascii_case(plan_guid));
        if already_active {
            self.clear_switch_failure(plan_guid);
            return;
        }

        if self.is_switch_suppressed(plan_guid) {
            return;
        }

        if let Some((last_guid, attempted_at)) = &self.last_switch_attempt {
            if last_guid.eq_ignore_ascii_case(plan_guid)
                && attempted_at.elapsed() < SWITCH_RETRY_INTERVAL
            {
                return;
            }
        }

        self.last_switch_attempt = Some((plan_guid.to_owned(), Instant::now()));

        match self.power.set_active(plan_guid) {
            Ok(()) => {
                self.current_guid = Some(plan_guid.to_owned());
                self.clear_switch_failure(plan_guid);
            }
            Err(_) => self.record_switch_failure(plan_guid),
        }
    }

    fn is_switch_suppressed(&self, target_guid: &str) -> bool {
        self.switch_failure_suppression
            .is_key_suppressed(&switch_failure_key(target_guid))
    }

    fn record_switch_failure(&mut self, target_guid: &str) {
        self.switch_failure_suppression
            .record_key_failure(&switch_failure_key(target_guid));
    }

    fn clear_switch_failure(&mut self, target_guid: &str) {
        self.switch_failure_suppression
            .clear_key_failure(&switch_failure_key(target_guid));
    }
}

fn switch_failure_key(target_guid: &str) -> String {
    target_guid.trim().to_ascii_lowercase()
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
    fn repeated_power_plan_switch_failures_suppress_future_attempts() {
        let mut runner = HiddenAutomationRunner::default();

        runner.record_switch_failure("PLAN-GUID");
        runner.record_switch_failure("plan-guid");
        assert!(!runner.is_switch_suppressed("plan-guid"));

        runner.record_switch_failure("plan-guid");
        assert!(runner.is_switch_suppressed("plan-guid"));

        runner.clear_switch_failure("PLAN-GUID");
        assert!(!runner.is_switch_suppressed("plan-guid"));
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
    fn foreground_lookup_runs_only_for_configured_foreground_rules() {
        let mut settings = Settings::default();

        assert!(!foreground_lookup_required(&settings));

        settings.foreground_rules.enabled = true;
        assert!(!foreground_lookup_required(&settings));

        settings.foreground_rules.rules.push(ForegroundRule {
            enabled: true,
            name: "editor.exe".to_owned(),
            process_name: "editor.exe".to_owned(),
            power_plan_guid: Some("active-guid".to_owned()),
        });
        assert!(foreground_lookup_required(&settings));
    }

    #[test]
    fn automation_worker_sleeps_when_no_automation_work_exists() {
        let settings = Settings::default();

        assert!(!automation_worker_required(&settings));
    }

    #[test]
    fn status_snapshot_since_skips_unchanged_status() {
        let automation = BackgroundAutomation::start(&Settings::default());
        let snapshot = automation
            .status_snapshot_since(0)
            .expect("initial status snapshot should be visible");

        assert!(automation
            .status_snapshot_since(snapshot.generation)
            .is_none());
    }

    #[test]
    fn pending_auto_exclusions_are_taken_only_after_generation_change() {
        let automation = BackgroundAutomation::start(&Settings::default());
        let mut generation = 0;

        assert!(automation
            .take_pending_auto_exclusions_since(&mut generation)
            .is_none());

        update_eco_qos_status(
            &automation.shared,
            EcoQosSnapshot {
                auto_excluded_processes: vec!["Editor.exe".to_owned()],
                ..EcoQosSnapshot::default()
            },
        );

        let pending = automation
            .take_pending_auto_exclusions_since(&mut generation)
            .expect("new pending exclusions should be visible");
        assert_eq!(pending.eco_qos, vec!["editor.exe"]);
        assert!(pending.cpu_affinity.is_empty());
        assert!(pending.background_cpu_restriction.is_empty());
        update_cpu_affinity_status(
            &automation.shared,
            CpuAffinitySnapshot {
                auto_excluded_processes: vec!["Game.exe".to_owned()],
                ..CpuAffinitySnapshot::default()
            },
        );

        let pending = automation
            .take_pending_auto_exclusions_since(&mut generation)
            .expect("new pending affinity exclusions should be visible");
        assert_eq!(pending.cpu_affinity, vec!["game.exe"]);
        assert!(automation
            .take_pending_auto_exclusions_since(&mut generation)
            .is_none());
    }

    #[test]
    fn automation_worker_runs_for_enabled_process_feature() {
        let mut settings = Settings::default();
        settings.eco_qos.enabled = true;

        assert!(automation_worker_required(&settings));
    }

    #[test]
    fn automation_worker_runs_for_enabled_smart_trim() {
        let mut settings = Settings::default();
        settings.smart_trim.enabled = true;

        assert!(automation_worker_required(&settings));
    }

    #[test]
    fn foreground_responsiveness_fast_refresh_requires_enabled_feature() {
        let now = Instant::now();
        let mut settings = Settings::default();

        assert!(foreground_responsiveness_fast_refresh_deadline(&settings, now).is_none());
        assert!(!foreground_responsiveness_fast_refresh_active(
            &settings,
            Some(now + FOREGROUND_RESPONSIVENESS_FAST_REFRESH_WINDOW),
            now,
        ));

        settings.general.enabled = true;
        settings.foreground_responsiveness.enabled = true;
        let deadline = foreground_responsiveness_fast_refresh_deadline(&settings, now)
            .expect("foreground responsiveness should enable fast refresh");
        assert_eq!(
            deadline.duration_since(now),
            FOREGROUND_RESPONSIVENESS_FAST_REFRESH_WINDOW
        );
        assert!(foreground_responsiveness_fast_refresh_active(
            &settings,
            Some(deadline),
            now,
        ));
        assert!(!foreground_responsiveness_fast_refresh_active(
            &settings,
            Some(deadline),
            deadline,
        ));
    }

    #[test]
    fn launch_boost_forces_background_io_assist() {
        let settings = Settings::default();
        let io_priority = effective_io_priority_settings(&settings, true);

        assert!(io_priority.enabled);
        assert!(io_priority.foreground_detection_enabled);
        assert_eq!(
            io_priority.foreground_priority.priority(),
            Some(ProcessIoPriority::Normal)
        );
        assert_eq!(
            io_priority.background_priority.priority(),
            Some(ProcessIoPriority::VeryLow)
        );
    }

    #[test]
    fn auto_balance_makes_launch_boost_io_refresh_available() {
        let mut settings = Settings::default();
        settings.foreground_responsiveness.enabled = true;
        settings.foreground_responsiveness.auto_balance_enabled = true;
        settings.foreground_responsiveness.boost_foreground_app = false;

        assert!(io_priority_required(&settings));
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
    fn system_appearance_uses_windows_events_without_power_automation() {
        let mut settings = Settings::default();
        settings.general.enabled = false;
        settings.general.accent.source = AccentColorSource::Windows;

        assert!(windows_event_watcher_required(&settings));
        assert!(windows_event_wake_required(
            &settings,
            WindowsAutomationEvent::AppearanceChanged
        ));
        assert!(!windows_event_wake_required(
            &settings,
            WindowsAutomationEvent::PowerChanged
        ));
    }

    #[test]
    fn event_driven_power_checks_drop_idle_polling_for_foreground_only_rules() {
        let mut settings = Settings::default();
        settings.activity_mode.enabled = false;
        settings.foreground_rules.enabled = true;
        settings.foreground_rules.rules.push(ForegroundRule {
            enabled: true,
            name: "chat.exe".to_owned(),
            process_name: "chat.exe".to_owned(),
            power_plan_guid: Some("active-guid".to_owned()),
        });

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

    #[test]
    fn decision_engine_returns_default_active_power_plan() {
        let mut settings = Settings::default();
        settings.activity_mode.enabled = false;
        settings.power_plans.performance_guid = Some("target-guid".to_owned());
        let input = DecisionInput {
            activity_state: crate::activity::ActivityState::Active,
            foreground_app: None,
            plugged_in: None,
            performance_mode: None,
            schedule: None,
            cpu_usage: None,
        };

        assert_eq!(
            DecisionEngine
                .decide(&settings, input)
                .target_guid
                .as_deref(),
            Some("target-guid")
        );
    }
}
