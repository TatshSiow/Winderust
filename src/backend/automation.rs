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
    app_suspension::{AppSuspensionManager, AppSuspensionSnapshot},
    background_cpu::BackgroundCpuRestrictionManager,
    background_efficiency::{BackgroundEfficiencyManager, BackgroundEfficiencySnapshot},
    config::{
        AccentColorSource, AnimationMode, AppThemeMode, PowerPlanSettings, ProcessIoPriority,
        Settings,
    },
    core_limiter::{CoreLimiterManager, CoreLimiterSnapshot},
    core_steering::{
        self, CoreSteeringManager, CoreSteeringSnapshot, LogicalProcessorInfo, LogicalProcessorKind,
    },
    cpu::{CpuUsageMonitor, CpuUsageSnapshot, PerProcessorUsageMonitor},
    dashboard_metrics::{IoUsageMonitor, IoUsageSnapshot},
    dynamic_priority_boost::{DynamicPriorityBoostManager, DynamicPriorityBoostSnapshot},
    features::power_plan_control::by_running_app::{ByRunningAppManager, ByRunningAppSnapshot},
    features::power_plan_control::{
        current_by_time_decision, next_by_time_change_delay, ByCpuLoadScheduler,
    },
    foreground::{
        cursor_is_shell_window, cursor_process, cursor_process_id, foreground_process,
        foreground_process_id, foreground_process_name, list_processes, process_name_key,
        shell_window_mouse_pressed, top_level_window_process_ids,
    },
    gpu_priority::{GpuPriorityManager, GpuPrioritySnapshot},
    io_priority::{IoPriorityManager, IoPrioritySnapshot},
    memory_priority::{MemoryPriorityManager, MemoryPrioritySnapshot},
    memory_trim::{MemoryTrimManager, MemoryTrimSnapshot},
    power::{
        active_plan, adaptive_power_profile_transition, apply_processor_power_values,
        create_adaptive_plan, delete_plan, read_processor_power_values, set_active,
        AdaptivePowerDemand, AdaptivePowerProfile, ProcessorPowerAcDcValues, ProcessorPowerValues,
    },
    power_source,
    process_priority::{ProcessPriorityManager, ProcessPrioritySnapshot},
    rules::{
        decide, set_execution_failure_suppression_threshold, ByRunningAppDecision, DecisionInput,
        ExecutionFailureTracker,
    },
    thread_priority::{ThreadPriorityManager, ThreadPrioritySnapshot},
    timer_resolution::{TimerResolutionManager, TimerResolutionSnapshot},
    tray,
    windows_events::{WindowsAutomationEvent, WindowsEventWatcher},
    workload_engine::{WorkloadEngineManager, WorkloadEngineSnapshot, WorkloadEngineUpdate},
};

mod requirements;
mod runner;
mod status;
mod wake;

pub(crate) use requirements::foreground_lookup_required;
use requirements::*;
use runner::*;
use status::*;
use wake::*;

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
const WORKLOAD_ENGINE_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const WORKLOAD_ENGINE_FAST_REFRESH_INTERVAL: Duration = Duration::from_millis(250);
const WORKLOAD_ENGINE_FAST_REFRESH_WINDOW: Duration = Duration::from_secs(8);
const ADAPTIVE_IO_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const PROCESS_PRIORITY_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const THREAD_PRIORITY_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const PRIORITY_BOOST_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const IO_PRIORITY_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const GPU_PRIORITY_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const MEMORY_PRIORITY_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const MEMORY_TRIM_REFRESH_INTERVAL: Duration = Duration::from_secs(15 * 60);
const TIMER_RESOLUTION_REFRESH_INTERVAL: Duration = Duration::from_secs(3);
const PROCESS_APPEARANCE_SCAN_INTERVAL: Duration = Duration::from_secs(1);
const HIDDEN_AUTOMATION_REFRESH_INTERVAL: Duration = Duration::from_secs(10);
const ADAPTIVE_ENGINE_AUTOMATION_REFRESH_INTERVAL: Duration = Duration::from_secs(60);
const VISIBLE_AUTOMATION_REFRESH_INTERVAL: Duration = Duration::from_secs(3);
const SCHEDULE_RULE_MAX_SLEEP: Duration = Duration::from_secs(60 * 60);
const SWITCH_RETRY_INTERVAL: Duration = Duration::from_secs(15);

pub struct BackgroundAutomation {
    shared: Arc<SharedAutomationState>,
    thread: Mutex<Option<JoinHandle<()>>>,
    event_watcher: Mutex<Option<WindowsEventWatcher>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AutomationStatusSnapshot {
    pub generation: u64,
    pub background_efficiency: BackgroundEfficiencySnapshot,
    pub app_suspension: AppSuspensionSnapshot,
    pub core_steering: CoreSteeringSnapshot,
    pub background_cpu_restriction: CoreSteeringSnapshot,
    pub core_limiter: CoreLimiterSnapshot,
    pub by_running_app: ByRunningAppSnapshot,
    pub workload_engine: WorkloadEngineSnapshot,
    pub process_priority: ProcessPrioritySnapshot,
    pub thread_priority: ThreadPrioritySnapshot,
    pub dynamic_priority_boost: DynamicPriorityBoostSnapshot,
    pub io_priority: IoPrioritySnapshot,
    pub gpu_priority: GpuPrioritySnapshot,
    pub memory_priority: MemoryPrioritySnapshot,
    pub memory_trim: MemoryTrimSnapshot,
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
    status: AutomationStatusSnapshot,

    pending_auto_exclusions: PendingAutoExclusions,
    app_suspension_freeze_requests: Vec<String>,
    memory_trim_now_requested: bool,
    action_log_clear_requested: bool,
    pending_events: AutomationWakeEvents,
    windows_event_watcher_active: bool,
    stop_requested: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PendingAutoExclusions {
    pub background_efficiency: Vec<String>,
    pub app_suspension: Vec<String>,
    pub core_steering: Vec<String>,
    pub background_cpu_restriction: Vec<String>,
    pub core_limiter: Vec<String>,
    pub workload_engine: Vec<String>,
    pub io_priority: Vec<String>,
    pub process_priority: Vec<String>,
    pub thread_priority: Vec<String>,
    pub dynamic_priority_boost: Vec<String>,
    pub gpu_priority: Vec<String>,
    pub memory_priority: Vec<String>,
    pub memory_trim: Vec<String>,
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
                status: AutomationStatusSnapshot {
                    generation: 1,
                    ..Default::default()
                },

                pending_auto_exclusions: PendingAutoExclusions::default(),
                app_suspension_freeze_requests: Vec::new(),
                memory_trim_now_requested: false,
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
            (state.status.generation != observed_generation).then(|| state.status.clone())
        })
    }

    pub fn clear_action_log(&self) {
        if let Ok(mut state) = self.shared.state.lock() {
            state.status.action_log_entries = Arc::new(Vec::new());
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
            state.change_generation = state.change_generation.wrapping_add(1);
            settings_to_sync = Some(Arc::clone(&state.settings));
            self.shared.changed.notify_one();
        }

        if let Some(settings) = settings_to_sync {
            self.sync_worker(settings.as_ref());
        }
    }

    pub fn request_memory_trim_now(&self) {
        let mut settings_to_sync = None;
        if let Ok(mut state) = self.shared.state.lock() {
            state.memory_trim_now_requested = true;
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
    let mut next_background_efficiency_refresh = Instant::now();
    let mut next_app_suspension_refresh = Instant::now();
    let mut next_app_suspension_foreground_release = Instant::now();
    let mut next_core_steering_refresh = Instant::now();
    let mut next_background_cpu_restriction_refresh = Instant::now();
    let mut next_core_limiter_refresh = Instant::now();
    let mut next_by_running_app_refresh = Instant::now();
    let mut next_workload_engine_refresh = Instant::now();
    let mut next_process_priority_refresh = Instant::now();
    let mut next_thread_priority_refresh = Instant::now();
    let mut next_dynamic_priority_boost_refresh = Instant::now();
    let mut next_io_priority_refresh = Instant::now();
    let mut next_gpu_priority_refresh = Instant::now();
    let mut next_memory_priority_refresh = Instant::now();
    let mut next_memory_trim_refresh = Instant::now();
    let mut next_timer_resolution_refresh = Instant::now();
    let mut next_process_appearance_scan = Instant::now();
    let mut next_controller_activity_poll = Instant::now();
    let mut workload_engine_fast_until: Option<Instant> = None;

    while let Some(snapshot) = automation_snapshot(&shared) {
        let settings = snapshot.settings;
        let change_generation = snapshot.change_generation;
        let app_suspension_freeze_requests = snapshot.app_suspension_freeze_requests;
        let memory_trim_now_requested = snapshot.memory_trim_now_requested;
        if snapshot.action_log_clear_requested {
            runner.action_log.clear();
        }
        let wake_events = snapshot.wake_events;
        let windows_event_watcher_active = snapshot.windows_event_watcher_active;
        let hidden_to_tray = tray::is_hidden_to_tray();
        let adaptive_engine_enabled = settings.adaptive_engine.enabled;
        let background_efficiency_refresh_interval = automation_refresh_interval(
            hidden_to_tray,
            adaptive_engine_enabled,
            ECO_QOS_REFRESH_INTERVAL,
        );
        let app_suspension_refresh_interval = automation_refresh_interval(
            hidden_to_tray,
            adaptive_engine_enabled,
            APP_SUSPENSION_REFRESH_INTERVAL,
        );
        let core_steering_refresh_interval = automation_refresh_interval(
            hidden_to_tray,
            adaptive_engine_enabled,
            CPU_AFFINITY_REFRESH_INTERVAL,
        );
        let background_cpu_restriction_refresh_interval = automation_refresh_interval(
            hidden_to_tray,
            adaptive_engine_enabled,
            BACKGROUND_CPU_RESTRICTION_REFRESH_INTERVAL,
        );
        let core_limiter_refresh_interval = automation_refresh_interval(
            hidden_to_tray,
            adaptive_engine_enabled,
            CPU_LIMITER_REFRESH_INTERVAL,
        );
        let by_running_app_refresh_interval = automation_refresh_interval(
            hidden_to_tray,
            adaptive_engine_enabled,
            PERFORMANCE_MODE_REFRESH_INTERVAL,
        );
        let mut workload_engine_refresh_interval =
            workload_refresh_interval(&settings, hidden_to_tray, adaptive_engine_enabled);
        let process_priority_refresh_interval = automation_refresh_interval(
            hidden_to_tray,
            adaptive_engine_enabled,
            PROCESS_PRIORITY_REFRESH_INTERVAL,
        );
        let thread_priority_refresh_interval = automation_refresh_interval(
            hidden_to_tray,
            adaptive_engine_enabled,
            THREAD_PRIORITY_REFRESH_INTERVAL,
        );
        let dynamic_priority_boost_refresh_interval = automation_refresh_interval(
            hidden_to_tray,
            adaptive_engine_enabled,
            PRIORITY_BOOST_REFRESH_INTERVAL,
        );
        let io_priority_refresh_interval = automation_refresh_interval(
            hidden_to_tray,
            adaptive_engine_enabled,
            IO_PRIORITY_REFRESH_INTERVAL,
        );
        let gpu_priority_refresh_interval = automation_refresh_interval(
            hidden_to_tray,
            adaptive_engine_enabled,
            GPU_PRIORITY_REFRESH_INTERVAL,
        );
        let memory_priority_refresh_interval = automation_refresh_interval(
            hidden_to_tray,
            adaptive_engine_enabled,
            MEMORY_PRIORITY_REFRESH_INTERVAL,
        );
        let memory_trim_refresh_interval = automation_refresh_interval(
            hidden_to_tray,
            adaptive_engine_enabled,
            MEMORY_TRIM_REFRESH_INTERVAL,
        );
        let timer_resolution_refresh_interval = automation_refresh_interval(
            hidden_to_tray,
            adaptive_engine_enabled,
            TIMER_RESOLUTION_REFRESH_INTERVAL,
        );
        let process_appearance_scan_interval = automation_refresh_interval(
            hidden_to_tray,
            adaptive_engine_enabled,
            PROCESS_APPEARANCE_SCAN_INTERVAL,
        );
        let app_suspension_foreground_release_interval = automation_refresh_interval(
            hidden_to_tray,
            adaptive_engine_enabled,
            APP_SUSPENSION_FOREGROUND_RELEASE_INTERVAL,
        );
        let event_now = Instant::now();
        let settings_changed = wake_events.settings_changed || runner.note_settings(&settings);
        if settings_changed {
            for refresh_at in [
                &mut next_check,
                &mut next_background_efficiency_refresh,
                &mut next_app_suspension_refresh,
                &mut next_app_suspension_foreground_release,
                &mut next_core_steering_refresh,
                &mut next_background_cpu_restriction_refresh,
                &mut next_core_limiter_refresh,
                &mut next_by_running_app_refresh,
                &mut next_workload_engine_refresh,
                &mut next_process_priority_refresh,
                &mut next_thread_priority_refresh,
                &mut next_dynamic_priority_boost_refresh,
                &mut next_io_priority_refresh,
                &mut next_gpu_priority_refresh,
                &mut next_memory_priority_refresh,
                &mut next_memory_trim_refresh,
                &mut next_timer_resolution_refresh,
                &mut next_process_appearance_scan,
                &mut next_controller_activity_poll,
            ] {
                *refresh_at = event_now;
            }
            workload_engine_fast_until = None;
        }
        if wake_events.foreground_changed || wake_events.session_changed {
            for refresh_at in [
                &mut next_check,
                &mut next_background_efficiency_refresh,
                &mut next_core_steering_refresh,
                &mut next_background_cpu_restriction_refresh,
                &mut next_core_limiter_refresh,
                &mut next_workload_engine_refresh,
                &mut next_process_priority_refresh,
                &mut next_thread_priority_refresh,
                &mut next_dynamic_priority_boost_refresh,
                &mut next_io_priority_refresh,
                &mut next_gpu_priority_refresh,
                &mut next_memory_priority_refresh,
                &mut next_memory_trim_refresh,
                &mut next_timer_resolution_refresh,
                &mut next_app_suspension_foreground_release,
            ] {
                *refresh_at = event_now;
            }
            workload_engine_fast_until =
                workload_engine_fast_refresh_deadline(&settings, event_now);
        }
        if wake_events.window_created || wake_events.session_changed {
            next_process_appearance_scan = event_now;
            next_app_suspension_refresh = event_now;
            workload_engine_fast_until =
                workload_engine_fast_refresh_deadline(&settings, event_now);
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
                }
            }
        }
        let now = Instant::now();
        let power_plan_checks_required = power_plan_checks_required(&settings);
        let scan_process_appearance = process_appearance_scan_required(&settings);
        let background_efficiency_refresh_required = settings_changed
            || feature_refresh_required(&settings, settings.background_efficiency.enabled);
        let app_suspension_refresh_required = settings_changed
            || feature_refresh_required(&settings, settings.app_suspension.enabled)
            || !app_suspension_freeze_requests.is_empty()
            || runner.app_suspension_manager.has_suspended_processes();
        let core_steering_refresh_required =
            settings_changed || feature_refresh_required(&settings, settings.core_steering.enabled);
        let background_cpu_restriction_refresh_required = settings_changed
            || feature_refresh_required(&settings, settings.background_cpu_restriction.enabled);
        let core_limiter_refresh_required =
            settings_changed || feature_refresh_required(&settings, settings.core_limiter.enabled);
        let by_running_app_refresh_required = settings_changed
            || feature_refresh_required(&settings, settings.by_running_app.enabled);
        let workload_engine_refresh_required = settings_changed
            || feature_refresh_required(
                &settings,
                workload_engine_required(&settings) || adaptive_power_plan_required(&settings),
            );
        let process_priority_refresh_required = settings_changed
            || feature_refresh_required(&settings, settings.process_priority.enabled);
        let thread_priority_refresh_required = settings_changed
            || feature_refresh_required(&settings, thread_priority_required(&settings));
        let dynamic_priority_boost_refresh_required = settings_changed
            || feature_refresh_required(&settings, dynamic_priority_boost_required(&settings));
        let io_priority_refresh_required = settings_changed
            || feature_refresh_required(&settings, io_priority_required(&settings));
        let gpu_priority_refresh_required = settings_changed
            || feature_refresh_required(&settings, gpu_priority_required(&settings));
        let memory_priority_refresh_required = settings_changed
            || feature_refresh_required(&settings, settings.memory_priority.enabled);
        let memory_trim_refresh_required = settings_changed
            || memory_trim_now_requested
            || feature_refresh_required(&settings, settings.memory_trim.enabled);
        let timer_resolution_refresh_required = settings_changed
            || feature_refresh_required(&settings, settings.timer_resolution.enabled);
        if !app_suspension_freeze_requests.is_empty() {
            next_app_suspension_refresh = now;
        }
        if memory_trim_now_requested {
            next_memory_trim_refresh = now;
        }

        if workload_engine_fast_refresh_active(&settings, workload_engine_fast_until, now) {
            workload_engine_refresh_interval = WORKLOAD_ENGINE_FAST_REFRESH_INTERVAL;
        }

        if scan_process_appearance && now >= next_process_appearance_scan {
            if runner.detect_process_appearance() {
                for refresh_at in [
                    &mut next_background_efficiency_refresh,
                    &mut next_core_steering_refresh,
                    &mut next_background_cpu_restriction_refresh,
                    &mut next_core_limiter_refresh,
                    &mut next_by_running_app_refresh,
                    &mut next_workload_engine_refresh,
                    &mut next_process_priority_refresh,
                    &mut next_thread_priority_refresh,
                    &mut next_dynamic_priority_boost_refresh,
                    &mut next_io_priority_refresh,
                    &mut next_gpu_priority_refresh,
                    &mut next_memory_priority_refresh,
                    &mut next_memory_trim_refresh,
                ] {
                    *refresh_at = now;
                }
                workload_engine_fast_until = workload_engine_fast_refresh_deadline(&settings, now);
            }
            next_process_appearance_scan = now + process_appearance_scan_interval;
        } else if !scan_process_appearance {
            runner.known_process_ids.clear();
            next_process_appearance_scan = now + process_appearance_scan_interval;
        }

        if runner.app_suspension_manager.has_suspended_processes()
            && now >= next_app_suspension_foreground_release
        {
            if let Some(app_suspension_status) = runner.run_app_suspension_foreground_release() {
                update_app_suspension_status(&shared, app_suspension_status);
            }
            next_app_suspension_foreground_release =
                now + app_suspension_foreground_release_interval;
        }

        if background_efficiency_refresh_required && now >= next_background_efficiency_refresh {
            let background_efficiency_status = runner.run_background_efficiency_update(&settings);
            update_background_efficiency_status(&shared, background_efficiency_status);
            next_background_efficiency_refresh = now + background_efficiency_refresh_interval;
        }
        if workload_engine_refresh_required && now >= next_workload_engine_refresh {
            let workload_engine_status = runner.run_workload_engine_update(&settings);
            if workload_engine_status.foreground_boosted_process.is_some()
                || workload_engine_status.workload_managed_processes > 0
            {
                workload_engine_fast_until = workload_engine_fast_refresh_deadline(&settings, now);
            }
            update_workload_engine_status(&shared, workload_engine_status);
            next_workload_engine_refresh = now + workload_engine_refresh_interval;
        }
        if io_priority_refresh_required && now >= next_io_priority_refresh {
            let io_priority_status = runner.run_io_priority_update(&settings);
            update_io_priority_status(&shared, io_priority_status);
            next_io_priority_refresh = now + io_priority_refresh_interval;
        }
        if process_priority_refresh_required && now >= next_process_priority_refresh {
            let process_priority_status = runner.run_process_priority_update(&settings);
            update_process_priority_status(&shared, process_priority_status);
            next_process_priority_refresh = now + process_priority_refresh_interval;
        }
        if thread_priority_refresh_required && now >= next_thread_priority_refresh {
            let thread_priority_status = runner.run_thread_priority_update(&settings);
            update_thread_priority_status(&shared, thread_priority_status);
            next_thread_priority_refresh = now + thread_priority_refresh_interval;
        }
        if dynamic_priority_boost_refresh_required && now >= next_dynamic_priority_boost_refresh {
            let dynamic_priority_boost_status = runner.run_dynamic_priority_boost_update(&settings);
            update_dynamic_priority_boost_status(&shared, dynamic_priority_boost_status);
            next_dynamic_priority_boost_refresh = now + dynamic_priority_boost_refresh_interval;
        }
        if gpu_priority_refresh_required && now >= next_gpu_priority_refresh {
            let gpu_priority_status = runner.run_gpu_priority_update(&settings);
            update_gpu_priority_status(&shared, gpu_priority_status);
            next_gpu_priority_refresh = now + gpu_priority_refresh_interval;
        }
        if memory_priority_refresh_required && now >= next_memory_priority_refresh {
            let memory_priority_status = runner.run_memory_priority_update(&settings);
            update_memory_priority_status(&shared, memory_priority_status);
            next_memory_priority_refresh = now + memory_priority_refresh_interval;
        }
        if app_suspension_refresh_required && now >= next_app_suspension_refresh {
            let app_suspension_status =
                runner.run_app_suspension_update(&settings, &app_suspension_freeze_requests);
            update_app_suspension_status(&shared, app_suspension_status);
            next_app_suspension_refresh = now + app_suspension_refresh_interval;
            if runner.app_suspension_manager.has_suspended_processes() {
                next_app_suspension_foreground_release = now;
            }
        }
        if core_steering_refresh_required && now >= next_core_steering_refresh {
            let core_steering_status = runner.run_core_steering_update(&settings);
            update_core_steering_status(&shared, core_steering_status);
            next_core_steering_refresh = now + core_steering_refresh_interval;
        }
        if background_cpu_restriction_refresh_required
            && now >= next_background_cpu_restriction_refresh
        {
            let status = runner.run_background_cpu_restriction_update(&settings);
            update_background_cpu_restriction_status(&shared, status);
            next_background_cpu_restriction_refresh =
                now + background_cpu_restriction_refresh_interval;
        }
        if core_limiter_refresh_required && now >= next_core_limiter_refresh {
            let core_limiter_status = runner.run_core_limiter_update(&settings);
            update_core_limiter_status(&shared, core_limiter_status);
            next_core_limiter_refresh = now + core_limiter_refresh_interval;
        }
        if by_running_app_refresh_required && now >= next_by_running_app_refresh {
            let by_running_app_status = runner.run_by_running_app_update(&settings);
            update_by_running_app_status(&shared, by_running_app_status);
            next_by_running_app_refresh = now + by_running_app_refresh_interval;
        }
        if memory_trim_refresh_required && now >= next_memory_trim_refresh {
            let memory_trim_status = if memory_trim_now_requested {
                runner.run_memory_trim_now(&settings)
            } else {
                runner.run_memory_trim_update(&settings)
            };
            update_memory_trim_status(&shared, memory_trim_status);
            next_memory_trim_refresh = now + memory_trim_refresh_interval;
        }
        if timer_resolution_refresh_required && now >= next_timer_resolution_refresh {
            let timer_resolution_status = runner.run_timer_resolution_update(&settings);
            update_timer_resolution_status(&shared, timer_resolution_status);
            next_timer_resolution_refresh = now + timer_resolution_refresh_interval;
        }

        runner.publish_action_log_if_changed(&shared);

        let wait_now = Instant::now();
        let mut wait_for = if hidden_to_tray {
            if power_plan_checks_required {
                let input_events = input_hook::take_pending_events();
                if input_hook_should_check(&settings, input_events) {
                    next_check = wait_now;
                }

                if wait_now >= next_check && !runner.by_running_app_manager.is_active() {
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

        for (required, refresh_at, interval) in [
            (
                background_efficiency_refresh_required,
                next_background_efficiency_refresh,
                background_efficiency_refresh_interval,
            ),
            (
                app_suspension_refresh_required,
                next_app_suspension_refresh,
                app_suspension_refresh_interval,
            ),
            (
                core_steering_refresh_required,
                next_core_steering_refresh,
                core_steering_refresh_interval,
            ),
            (
                background_cpu_restriction_refresh_required,
                next_background_cpu_restriction_refresh,
                background_cpu_restriction_refresh_interval,
            ),
            (
                core_limiter_refresh_required,
                next_core_limiter_refresh,
                core_limiter_refresh_interval,
            ),
            (
                by_running_app_refresh_required,
                next_by_running_app_refresh,
                by_running_app_refresh_interval,
            ),
            (
                workload_engine_refresh_required,
                next_workload_engine_refresh,
                workload_engine_refresh_interval,
            ),
            (
                process_priority_refresh_required,
                next_process_priority_refresh,
                process_priority_refresh_interval,
            ),
            (
                thread_priority_refresh_required,
                next_thread_priority_refresh,
                thread_priority_refresh_interval,
            ),
            (
                dynamic_priority_boost_refresh_required,
                next_dynamic_priority_boost_refresh,
                dynamic_priority_boost_refresh_interval,
            ),
            (
                io_priority_refresh_required,
                next_io_priority_refresh,
                io_priority_refresh_interval,
            ),
            (
                gpu_priority_refresh_required,
                next_gpu_priority_refresh,
                gpu_priority_refresh_interval,
            ),
            (
                memory_priority_refresh_required,
                next_memory_priority_refresh,
                memory_priority_refresh_interval,
            ),
            (
                memory_trim_refresh_required,
                next_memory_trim_refresh,
                memory_trim_refresh_interval,
            ),
            (
                timer_resolution_refresh_required,
                next_timer_resolution_refresh,
                timer_resolution_refresh_interval,
            ),
            (
                scan_process_appearance,
                next_process_appearance_scan,
                process_appearance_scan_interval,
            ),
            (
                controller_poll_required,
                next_controller_activity_poll,
                CONTROLLER_ACTIVITY_POLL_INTERVAL,
            ),
            (
                runner.app_suspension_manager.has_suspended_processes(),
                next_app_suspension_foreground_release,
                app_suspension_foreground_release_interval,
            ),
        ] {
            if required {
                wait_for = Some(min_worker_wait(
                    wait_for,
                    refresh_at.saturating_duration_since(wait_now).min(interval),
                ));
            }
        }
        if wait_for.is_none() && !automation_worker_required(&settings) {
            break;
        }

        if wait_for_wake(&shared, wait_for, change_generation) {
            break;
        }
    }
}

fn min_worker_wait(current: Option<Duration>, candidate: Duration) -> Duration {
    current.map_or(candidate, |current| current.min(candidate))
}

#[cfg(test)]
mod tests;
