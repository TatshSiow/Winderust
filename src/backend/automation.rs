use std::{
    collections::{BTreeSet, VecDeque},
    path::Path,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc::{sync_channel, Receiver, SyncSender},
        Arc, Condvar, Mutex, MutexGuard,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use chrono::{DateTime, Local};
use rust_i18n::t;

use crate::{
    action_log::{ActionLog, ActionLogEntry, ActionLogSummaries},
    activity::{
        activity_snapshot, input_tracker::InputActivityTracker, ActivityState,
        ControllerActivityDetector, InputHook, InputHookConfig, InputHookEvents,
        CONTROLLER_ACTIVITY_POLL_INTERVAL,
    },
    adaptive_engine_process::{
        AdaptiveEngineProcessManager, AdaptiveEngineProcessSnapshot, AdaptiveEngineProcessUpdate,
    },
    app_suspension::{self, AppSuspensionManager, AppSuspensionSnapshot, AppSuspensionStatus},
    application::settings::{AutoExclusionPatch, RuntimeSettingsSnapshot, SettingsRevision},
    background_efficiency::{BackgroundEfficiencyManager, BackgroundEfficiencySnapshot},
    bottleneck_classifier::{BottleneckClassifier, BottleneckSnapshot},
    config::{
        AccentColorSource, AppThemeMode, ByActivitySettings, CpuAllocationSettings,
        InputDetectionSettings, PowerPlanSettings, PowerSourceProfile, ProcessGpuPriority,
        ProcessIoPriority, ProcessMemoryPriority, ProcessPrioritySetting,
        ProcessThreadPrioritySetting, Settings, CHECK_INTERVAL_MAX_MS, CHECK_INTERVAL_MIN_MS,
    },
    control::{
        cpu_allocation::CpuAllocationCoordinator,
        cpu_limiter::CpuLimiterController,
        dynamic_priority_boost::{DynamicPriorityBoostController, DynamicPriorityBoostState},
        gpu_priority::GpuPriorityController,
        io_priority::IoPriorityController,
        memory_priority::MemoryPriorityController,
        memory_trim::MemoryTrimController,
        power_plan::{
            ActivePlanCheckError, AdaptivePowerPlanRequest, ManualPowerPlanError,
            PowerPlanController, PowerPlanStatus,
        },
        priority_efficiency::PriorityEfficiencyController,
        process::ControlOwner,
        process_termination::{ProcessTerminationBatchResult, ProcessTerminationController},
        sleep_prevention::SleepPreventionController,
        suspension::SuspensionController,
        thread_priority::ThreadPriorityController,
        timer_resolution::TimerResolutionController,
    },
    cpu::{CpuUsageMonitor, CpuUsageSnapshot, PerProcessorUsageMonitor},
    cpu_allocation::{
        self, record_cpu_allocation_reconciliation, CpuAllocationManager, CpuAllocationSnapshot,
        LogicalProcessorInfo, LogicalProcessorKind,
    },
    cpu_limiter::{CpuLimiterManager, CpuLimiterSnapshot},
    dashboard_metrics::{IoUsageMonitor, IoUsageSnapshot},
    dynamic_priority_boost::{DynamicPriorityBoostManager, DynamicPriorityBoostSnapshot},
    features::advanced_controls::{
        instance_limits::{InstanceLimitsEvent, InstanceLimitsManager, InstanceLimitsSnapshot},
        keep_running::{
            CommitDisposition, KeepRunningManager, KeepRunningSnapshot, LaunchTicket, Observation,
            PauseReason,
        },
        sleep_prevention::{SleepPreventionManager, SleepPreventionSnapshot},
    },
    features::power_plan_control::by_running_app::{ByRunningAppManager, ByRunningAppSnapshot},
    features::power_plan_control::{
        current_by_time_decision, next_by_time_change_delay, ByCpuLoadScheduler,
    },
    features::winderust_features::watchdog::{WatchdogManager, WatchdogSnapshot},
    foreground::{
        cursor_is_shell_window, cursor_process, cursor_process_id, same_executable_path,
        shell_window_mouse_pressed, ProcessActionTarget, ProcessActionTargetError,
    },
    gpu_priority::{GpuPriorityManager, GpuPrioritySnapshot},
    io_priority::{IoPriorityManager, IoPrioritySnapshot},
    memory_priority::{MemoryPriorityManager, MemoryPrioritySnapshot},
    memory_trim::{
        memory_trim_policy_difference, MemoryTrimManager, MemoryTrimPolicyDifference,
        MemoryTrimSnapshot,
    },
    power::{
        AdaptivePowerBoostValues, AdaptivePowerDemand, AdaptivePowerProfile, ProcessorPowerValues,
    },
    power_source,
    process_priority::{ProcessPriorityManager, ProcessPrioritySnapshot},
    rules::{
        decide, set_execution_failure_suppression_threshold, ByRunningAppDecision, DecisionInput,
        DecisionState,
    },
    runtime::{
        observations::CycleObservations,
        scheduler::{RefreshDomain, RefreshScheduler, SchedulerEvent},
    },
    self_power::SelfPowerRetryWatcher,
    thread_priority::{ThreadPriorityManager, ThreadPrioritySnapshot},
    timer_resolution::{TimerResolutionManager, TimerResolutionSnapshot},
    tray::{self, TrayVisibilityWatcher},
    windows_events::{WindowsAutomationEvent, WindowsEventWatcher, WindowsEventWatcherMode},
};

mod instance_balancing;
mod instance_limits;
mod keep_running;
mod requirements;
mod runner;
mod status;
mod wake;

#[cfg(any(test, feature = "render-smoke"))]
pub(crate) use instance_balancing::InstanceBalancingDetail;
use instance_balancing::InstanceBalancingSnapshot;
use requirements::*;
use runner::*;
use status::*;
use wake::*;

const CPU_USAGE_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const ECO_QOS_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const APP_SUSPENSION_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const APP_SUSPENSION_FOREGROUND_RELEASE_INTERVAL: Duration = Duration::from_millis(500);
const APP_SUSPENSION_SHELL_USER_INTENT_INTERVAL: Duration = Duration::from_millis(750);
const CPU_ALLOCATION_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const INSTANCE_BALANCING_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const CPU_ALLOCATION_RECONCILIATION_RETRY_INITIAL: Duration = Duration::from_secs(1);
const CPU_ALLOCATION_RECONCILIATION_RETRY_MAX: Duration = Duration::from_secs(60);
const CPU_LIMITER_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const PERFORMANCE_MODE_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const ADAPTIVE_POWER_PLAN_REFRESH_INTERVAL: Duration = Duration::from_millis(500);
const BOTTLENECK_CLASSIFIER_REFRESH_INTERVAL: Duration = Duration::from_secs(5);
const ADAPTIVE_IO_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const PROCESS_PRIORITY_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const THREAD_PRIORITY_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const PRIORITY_BOOST_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const IO_PRIORITY_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const GPU_PRIORITY_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const MEMORY_PRIORITY_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const POWER_SOURCE_RECHECK_INTERVAL: Duration = Duration::from_secs(1);
const MEMORY_TRIM_REFRESH_INTERVAL: Duration = Duration::from_secs(15 * 60);
const MEMORY_TRIM_FOLLOW_UP_INTERVAL: Duration = Duration::from_secs(3);
const TIMER_RESOLUTION_REFRESH_INTERVAL: Duration = Duration::from_secs(3);
const SLEEP_PREVENTION_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const KEEP_RUNNING_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const INSTANCE_LIMITS_REFRESH_INTERVAL: Duration = Duration::from_secs(1);
const PROCESS_APPEARANCE_SCAN_INTERVAL: Duration = Duration::from_secs(1);
const HIDDEN_AUTOMATION_REFRESH_INTERVAL: Duration = Duration::from_secs(10);
const ADAPTIVE_ENGINE_AUTOMATION_REFRESH_INTERVAL: Duration = Duration::from_secs(60);
const VISIBLE_AUTOMATION_REFRESH_INTERVAL: Duration = Duration::from_secs(3);
// Local UTC-offset transitions can change By Time applicability without a clock notification.
const SCHEDULE_RULE_MAX_SLEEP: Duration = Duration::from_secs(60);
const AUTO_EXCLUSION_PATCH_RETRY_INTERVAL: Duration = Duration::from_secs(5);
const PROCESS_CONTROL_COMMAND_QUEUE_CAPACITY: usize = 32;

pub struct RuntimeHandle {
    shared: Arc<SharedAutomationState>,
    lifecycle: Mutex<()>,
    shutdown_result: Mutex<Option<Result<(), String>>>,
    thread: Mutex<Option<JoinHandle<Result<(), String>>>>,
    event_watcher: Mutex<Option<WindowsEventWatcher>>,
    input_hook: Mutex<Option<InputHook>>,
    self_power: Mutex<Option<SelfPowerRetryWatcher>>,
    tray_visibility_watcher: Mutex<Option<TrayVisibilityWatcher>>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub(crate) enum RuntimeCommandError {
    #[error("The automation runtime has stopped.")]
    RuntimeStopped,
    #[error("The automation worker exited before processing the request.")]
    WorkerExited,
    #[error("Too many control requests are already waiting. Try again shortly.")]
    QueueFull,
    #[error("The selected power plan is no longer available.")]
    PowerPlanUnavailable,
    #[error(transparent)]
    PowerPlan(#[from] ManualPowerPlanError),
    #[error("{0}")]
    InvalidRequest(String),
    #[error("{0}")]
    CommandFailed(String),
}

#[derive(Debug)]
pub(crate) struct ProcessControlBatchResult {
    pub(crate) results: Vec<Result<(), String>>,
}

pub(crate) type ProcessControlActionReceiver =
    Receiver<Result<ProcessControlBatchResult, RuntimeCommandError>>;
pub(crate) type MemoryTrimActionReceiver =
    Receiver<Result<MemoryTrimSnapshot, RuntimeCommandError>>;
pub(crate) type ProcessTerminationActionReceiver =
    Receiver<Result<ProcessTerminationBatchResult, RuntimeCommandError>>;
pub(crate) type AppSuspensionPathActionReceiver =
    Receiver<Result<AppSuspensionSnapshot, RuntimeCommandError>>;

impl ProcessControlBatchResult {
    pub(crate) fn into_process_list_result(self) -> Result<(), String> {
        let total = self.results.len();
        if total == 0 {
            return Err(t!("process_list.no_action_targets").into_owned());
        }
        let mut failures = self.results.into_iter().filter_map(Result::err);
        let Some(error) = failures.next() else {
            return Ok(());
        };
        Err(t!(
            "process_list.actions_failed",
            failed = 1 + failures.count(),
            total = total,
            error = error
        )
        .into_owned())
    }
}

enum ProcessControlCommand {
    ProcessPriority {
        targets: Vec<Result<ProcessActionTarget, ProcessActionTargetError>>,
        priority: ProcessPrioritySetting,
        result: SyncSender<Result<ProcessControlBatchResult, RuntimeCommandError>>,
    },
    EfficiencyMode {
        targets: Vec<Result<ProcessActionTarget, ProcessActionTargetError>>,
        enabled: bool,
        result: SyncSender<Result<ProcessControlBatchResult, RuntimeCommandError>>,
    },
    DynamicPriorityBoost {
        targets: Vec<Result<ProcessActionTarget, ProcessActionTargetError>>,
        state: DynamicPriorityBoostState,
        result: SyncSender<Result<ProcessControlBatchResult, RuntimeCommandError>>,
    },
    ThreadPriority {
        targets: Vec<Result<ProcessActionTarget, ProcessActionTargetError>>,
        priority: ProcessThreadPrioritySetting,
        result: SyncSender<Result<ProcessControlBatchResult, RuntimeCommandError>>,
    },
    IoPriority {
        targets: Vec<Result<ProcessActionTarget, ProcessActionTargetError>>,
        priority: ProcessIoPriority,
        result: SyncSender<Result<ProcessControlBatchResult, RuntimeCommandError>>,
    },
    GpuPriority {
        targets: Vec<Result<ProcessActionTarget, ProcessActionTargetError>>,
        priority: ProcessGpuPriority,
        result: SyncSender<Result<ProcessControlBatchResult, RuntimeCommandError>>,
    },
    MemoryPriority {
        targets: Vec<Result<ProcessActionTarget, ProcessActionTargetError>>,
        priority: ProcessMemoryPriority,
        result: SyncSender<Result<ProcessControlBatchResult, RuntimeCommandError>>,
    },
    AppSuspension {
        targets: Vec<Result<ProcessActionTarget, ProcessActionTargetError>>,
        suspend: bool,
        result: SyncSender<Result<ProcessControlBatchResult, RuntimeCommandError>>,
    },
    AppSuspensionPathAction {
        executable_path: String,
        freeze: bool,
        result: SyncSender<Result<AppSuspensionSnapshot, RuntimeCommandError>>,
    },
    MemoryTrim {
        result: SyncSender<Result<MemoryTrimSnapshot, RuntimeCommandError>>,
    },
    StopProcesses {
        targets: Vec<ProcessActionTarget>,
        result: SyncSender<Result<ProcessTerminationBatchResult, RuntimeCommandError>>,
    },
    StopTreeIntent {
        roots: Vec<ProcessActionTarget>,
    },
    KeepRunningResume {
        executable_path: String,
    },
    SwitchPowerPlan {
        guid: String,
        result: SyncSender<Result<(), RuntimeCommandError>>,
    },
}

impl ProcessControlCommand {
    fn reject(self, error: RuntimeCommandError) {
        match self {
            Self::ProcessPriority { result, .. }
            | Self::EfficiencyMode { result, .. }
            | Self::DynamicPriorityBoost { result, .. }
            | Self::ThreadPriority { result, .. }
            | Self::IoPriority { result, .. }
            | Self::GpuPriority { result, .. }
            | Self::MemoryPriority { result, .. }
            | Self::AppSuspension { result, .. } => {
                let _ = result.try_send(Err(error));
            }
            Self::MemoryTrim { result } => {
                let _ = result.try_send(Err(error));
            }
            Self::StopProcesses { result, .. } => {
                let _ = result.try_send(Err(error));
            }
            Self::AppSuspensionPathAction { result, .. } => {
                let _ = result.try_send(Err(error));
            }
            Self::StopTreeIntent { .. } | Self::KeepRunningResume { .. } => {}
            Self::SwitchPowerPlan { result, .. } => {
                let _ = result.try_send(Err(error));
            }
        }
    }

    fn is_memory_trim(&self) -> bool {
        matches!(self, Self::MemoryTrim { .. })
    }

    fn is_app_suspension(&self) -> bool {
        matches!(
            self,
            Self::AppSuspension { .. } | Self::AppSuspensionPathAction { .. }
        )
    }

    fn is_process_stop(&self) -> bool {
        matches!(
            self,
            Self::StopProcesses { .. } | Self::StopTreeIntent { .. }
        )
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuntimeFeatureStatus {
    pub bottleneck_classifier: BottleneckSnapshot,
    pub background_efficiency: BackgroundEfficiencySnapshot,
    pub app_suspension: AppSuspensionSnapshot,
    pub cpu_sets_soft: CpuAllocationSnapshot,
    pub processor_affinity_hard: CpuAllocationSnapshot,
    pub(crate) instance_balancing: InstanceBalancingSnapshot,
    pub cpu_limiter: CpuLimiterSnapshot,
    pub by_running_app: ByRunningAppSnapshot,
    pub adaptive_engine_process: AdaptiveEngineProcessSnapshot,
    pub process_priority: ProcessPrioritySnapshot,
    pub thread_priority: ThreadPrioritySnapshot,
    pub dynamic_priority_boost: DynamicPriorityBoostSnapshot,
    pub io_priority: IoPrioritySnapshot,
    pub gpu_priority: GpuPrioritySnapshot,
    pub memory_priority: MemoryPrioritySnapshot,
    pub memory_trim: MemoryTrimSnapshot,
    pub watchdog: WatchdogSnapshot,
    pub timer_resolution: TimerResolutionSnapshot,
    pub sleep_prevention: SleepPreventionSnapshot,
    pub(crate) keep_running: KeepRunningSnapshot,
    pub(crate) instance_limits: InstanceLimitsSnapshot,
}

#[derive(Debug, Clone)]
pub(crate) struct PowerPlanRuleContext {
    pub(crate) settings: Arc<Settings>,
    pub(crate) source: PowerSourceProfile,
}

impl PartialEq for PowerPlanRuleContext {
    fn eq(&self, other: &Self) -> bool {
        self.source == other.source && Arc::ptr_eq(&self.settings, &other.settings)
    }
}

impl Eq for PowerPlanRuleContext {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ByActivityObservation {
    source: PowerSourceProfile,
    input_detection: InputDetectionSettings,
    idle_timeout_seconds: u64,
    last_activity: Instant,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuntimeStatusSnapshot {
    pub generation: u64,
    pub worker_error: Option<String>,
    pub feature_status: Arc<RuntimeFeatureStatus>,
    pub(crate) power_plan_status: Arc<PowerPlanStatus>,
    pub(crate) power_plan_rule_context: Option<PowerPlanRuleContext>,
    pub(crate) by_activity: Option<ByActivityObservation>,
    pub action_log_entries: Arc<Vec<ActionLogEntry>>,
    pub action_log_summaries: Arc<ActionLogSummaries>,
    pub appearance_change_generation: u64,
}

impl RuntimeStatusSnapshot {
    pub(crate) fn by_activity_state(
        &self,
        source: PowerSourceProfile,
        settings: &ByActivitySettings,
        now: Instant,
    ) -> ActivityState {
        let idle_for = self
            .by_activity
            .as_ref()
            .filter(|observation| {
                settings.enabled
                    && observation.source == source
                    && observation.input_detection == settings.input_detection
                    && observation.idle_timeout_seconds == settings.idle_timeout_seconds
            })
            .map(|observation| now.saturating_duration_since(observation.last_activity));
        activity_snapshot(idle_for, Duration::from_secs(settings.idle_timeout_seconds)).state
    }
}

struct SharedAutomationState {
    state: Mutex<AutomationWorkerState>,
    changed: Condvar,
    status_generation: AtomicU64,
    pending_auto_exclusions_generation: AtomicU64,
    auto_exclusion_wake: Mutex<Option<Arc<dyn Fn() + Send + Sync>>>,
}

fn lock_unpoisoned<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

struct AutomationWorkerState {
    settings: Arc<Settings>,
    runtime_revision: SettingsRevision,
    persisted_revision: SettingsRevision,
    change_generation: u64,
    status: RuntimeStatusSnapshot,

    pending_auto_exclusions: AutoExclusionPatch,
    pending_auto_exclusions_revision: Option<SettingsRevision>,
    pending_auto_exclusions_retry_at: Option<Instant>,
    process_control_commands: VecDeque<ProcessControlCommand>,
    idle_action_log: Option<ActionLog>,
    action_log_clear_requested: bool,
    pending_events: AutomationWakeEvents,
    pending_keep_running_resets: keep_running::PendingResets,
    pending_instance_limits_resets: instance_limits::PendingResets,
    pending_instance_balancing_resets: instance_balancing::PendingResets,
    pending_sleep_prevention_settings: PendingSleepPreventionSettings,
    input_activity: InputActivityTracker,
    windows_event_watcher_active: bool,
    worker_accepting_work: bool,
    stop_requested: bool,
}

#[derive(Default)]
struct PendingSleepPreventionSettings {
    intermediate: Vec<Arc<Settings>>,
    overflowed: bool,
}

impl PendingSleepPreventionSettings {
    fn record(&mut self, settings: &Arc<Settings>) {
        // shortcut: Keep four intermediate saves, revisit when larger bursts need replay.
        const MAX_INTERMEDIATE_SETTINGS: usize = 4;
        if self.overflowed {
            return;
        }
        if self.intermediate.len() == MAX_INTERMEDIATE_SETTINGS {
            self.intermediate.clear();
            self.overflowed = true;
        } else {
            self.intermediate.push(Arc::clone(settings));
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WindowsEventWatcherChange {
    Started,
    Lost,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct AutomationWakeEvents {
    settings_changed: bool,
    restore_previous_plan_disabled: bool,
    tray_visibility_changed: bool,
    windows_event_watcher_change: Option<WindowsEventWatcherChange>,
    foreground_changed: bool,
    window_created: bool,
    power_changed: bool,
    suspend_resume: bool,
    session_changed: bool,
    clock_changed: bool,
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
            WindowsAutomationEvent::SuspendResume => {
                self.power_changed = true;
                self.suspend_resume = true;
            }
            WindowsAutomationEvent::SessionChanged => self.session_changed = true,
            WindowsAutomationEvent::ClockChanged => self.clock_changed = true,
            WindowsAutomationEvent::AppearanceChanged => self.appearance_changed = true,
        }
    }
}

impl RuntimeHandle {
    pub fn start(settings: &RuntimeSettingsSnapshot) -> Self {
        let shared = Arc::new(SharedAutomationState {
            state: Mutex::new(AutomationWorkerState {
                settings: Arc::clone(&settings.value),
                runtime_revision: settings.runtime_revision,
                persisted_revision: settings.persisted_revision,
                change_generation: 0,
                status: RuntimeStatusSnapshot {
                    generation: 1,
                    feature_status: Arc::new(RuntimeFeatureStatus::default()),
                    ..Default::default()
                },

                pending_auto_exclusions: AutoExclusionPatch::default(),
                pending_auto_exclusions_revision: None,
                pending_auto_exclusions_retry_at: None,
                process_control_commands: VecDeque::new(),
                idle_action_log: Some(ActionLog::default()),
                action_log_clear_requested: false,
                pending_events: AutomationWakeEvents::default(),
                pending_keep_running_resets: Default::default(),
                pending_instance_limits_resets: instance_limits::PendingResets::default(),
                pending_instance_balancing_resets: instance_balancing::PendingResets::default(),
                pending_sleep_prevention_settings: Default::default(),
                input_activity: InputActivityTracker::default(),
                windows_event_watcher_active: false,
                worker_accepting_work: false,
                stop_requested: false,
            }),
            changed: Condvar::new(),
            status_generation: AtomicU64::new(1),
            pending_auto_exclusions_generation: AtomicU64::new(0),
            auto_exclusion_wake: Mutex::new(None),
        });
        let self_power_shared = Arc::clone(&shared);
        let self_power_error = Mutex::new(None);
        let self_power = match SelfPowerRetryWatcher::start(Arc::new(move |result| {
            report_self_power_result(&self_power_shared, &self_power_error, result)
        })) {
            Ok(watcher) => {
                watcher.request().set_hidden_mode(tray::is_hidden_to_tray());
                Some(watcher)
            }
            Err(error) => {
                update_worker_error(&shared, Some(error));
                None
            }
        };
        let visibility_self_power = self_power.as_ref().map(SelfPowerRetryWatcher::request);
        let visibility_shared = Arc::clone(&shared);
        let tray_visibility_watcher = TrayVisibilityWatcher::start(Arc::new(move |hidden| {
            notify_tray_visibility_changed(&visibility_shared);
            if let Some(self_power) = &visibility_self_power {
                self_power.set_hidden_mode(hidden);
            }
        }));
        let automation = Self {
            shared,
            lifecycle: Mutex::new(()),
            shutdown_result: Mutex::new(None),
            thread: Mutex::new(None),
            event_watcher: Mutex::new(None),
            input_hook: Mutex::new(None),
            self_power: Mutex::new(self_power),
            tray_visibility_watcher: Mutex::new(Some(tray_visibility_watcher)),
        };
        automation.sync_windows_event_watcher(settings);
        automation.sync_input_hook(settings);
        automation.sync_worker(settings, false);
        automation
    }

    pub fn replace_settings(&self, settings: &RuntimeSettingsSnapshot) {
        let settings_changed = {
            let mut state = lock_unpoisoned(&self.shared.state);
            if state.stop_requested {
                return;
            }
            if state.persisted_revision != settings.persisted_revision {
                state.persisted_revision = settings.persisted_revision;
                if state.pending_auto_exclusions_revision.is_some() {
                    state.pending_auto_exclusions_revision = Some(settings.persisted_revision);
                    state.pending_auto_exclusions.base_revision = settings.persisted_revision;
                }
            }
            if state.runtime_revision == settings.runtime_revision {
                false
            } else {
                keep_running::record_settings_changes(&mut state, &settings.value);
                if instance_limits::record_settings_changes(&mut state, &settings.value) {
                    bump_status_generation(&self.shared, &mut state);
                }
                instance_balancing::record_settings_changes(&mut state, &settings.value);
                // A later save can restore the option before the worker sees this cancellation.
                if state.settings.general.restore_previous_power_plan
                    && !settings.value.general.restore_previous_power_plan
                {
                    state.pending_events.restore_previous_plan_disabled = true;
                }
                if state.pending_events.settings_changed {
                    let state = &mut *state;
                    state
                        .pending_sleep_prevention_settings
                        .record(&state.settings);
                }
                state.settings = Arc::clone(&settings.value);
                state.runtime_revision = settings.runtime_revision;
                state.pending_events.settings_changed = true;
                state.change_generation = state.change_generation.wrapping_add(1);
                self.shared.changed.notify_one();
                true
            }
        };

        if settings_changed {
            self.sync_windows_event_watcher(settings);
            self.sync_input_hook(settings);
            self.sync_worker(settings, false);
        }
    }

    pub fn shutdown(&self) -> Result<(), String> {
        let _lifecycle = lock_unpoisoned(&self.lifecycle);
        let mut result = lock_unpoisoned(&self.shutdown_result);
        if let Some(result) = result.as_ref() {
            return result.clone();
        }
        {
            let mut state = lock_unpoisoned(&self.shared.state);
            if !state.stop_requested {
                state.stop_requested = true;
                for command in state.process_control_commands.drain(..) {
                    command.reject(RuntimeCommandError::RuntimeStopped);
                }
                self.shared.changed.notify_one();
            }
        }

        {
            let mut watcher = lock_unpoisoned(&self.event_watcher);
            watcher.take();
        }
        {
            let mut input_hook = lock_unpoisoned(&self.input_hook);
            input_hook.take();
        }
        {
            let mut watcher = lock_unpoisoned(&self.tray_visibility_watcher);
            watcher.take();
        }
        let thread = lock_unpoisoned(&self.thread).take();

        let mut errors = Vec::new();
        if let Some(thread) = thread {
            match thread.join() {
                Ok(Ok(())) => {}
                Ok(Err(error)) => errors.push(format!(
                    "Background automation worker shutdown failed: {error}"
                )),
                Err(panic) => {
                    let reason = panic
                        .downcast::<&str>()
                        .map(|error| error.to_string())
                        .or_else(|panic| panic.downcast::<String>().map(|error| *error))
                        .unwrap_or_else(|_| "unknown panic".to_owned());
                    errors.push(format!(
                        "Background automation worker panicked during shutdown: {reason}"
                    ));
                }
            }
        }
        if let Some(watcher) = lock_unpoisoned(&self.self_power).as_mut() {
            if let Err(error) = watcher.shutdown() {
                errors.push(format!("Winderust self-power restoration failed: {error}"));
            }
        }
        let outcome = if errors.is_empty() {
            Ok(())
        } else {
            let error = errors.join(" ");
            update_worker_error(&self.shared, Some(error.clone()));
            Err(error)
        };
        *result = Some(outcome.clone());
        outcome
    }

    pub fn status_snapshot_since(&self, observed_generation: u64) -> Option<RuntimeStatusSnapshot> {
        if self.shared.status_generation.load(Ordering::Acquire) == observed_generation {
            return None;
        }

        let mut state = lock_unpoisoned(&self.shared.state);
        if state.status.generation == observed_generation {
            return None;
        }
        let mut snapshot = state.status.clone();
        if !state.stop_requested {
            snapshot.worker_error = state.status.worker_error.take();
        }
        Some(snapshot)
    }

    pub fn clear_action_log(&self) {
        let mut state = lock_unpoisoned(&self.shared.state);
        if state.stop_requested {
            return;
        }
        state.status.action_log_entries = Arc::new(Vec::new());
        state.status.action_log_summaries = Arc::new(ActionLogSummaries::new());
        bump_status_generation(&self.shared, &mut state);
        if let Some(log) = &mut state.idle_action_log {
            log.clear();
        } else {
            state.action_log_clear_requested = true;
        }
        state.change_generation = state.change_generation.wrapping_add(1);
        self.shared.changed.notify_one();
    }

    /// The callback must only enqueue a wake; it runs while runtime state is locked.
    pub(crate) fn set_auto_exclusion_wake(&self, wake: Arc<dyn Fn() + Send + Sync>) {
        *lock_unpoisoned(&self.shared.auto_exclusion_wake) = Some(wake);
    }

    pub fn take_auto_exclusion_patch_since(
        &self,
        observed_generation: &mut u64,
    ) -> Option<AutoExclusionPatch> {
        if self
            .shared
            .pending_auto_exclusions_generation
            .load(Ordering::Acquire)
            == *observed_generation
        {
            return None;
        }

        let mut state = lock_unpoisoned(&self.shared.state);
        let generation = self
            .shared
            .pending_auto_exclusions_generation
            .load(Ordering::Acquire);
        if generation == *observed_generation {
            return None;
        }
        if state
            .pending_auto_exclusions_retry_at
            .is_some_and(|retry_at| Instant::now() < retry_at)
        {
            return None;
        }

        *observed_generation = generation;
        state.pending_auto_exclusions_revision = None;
        state.pending_auto_exclusions_retry_at = None;
        Some(std::mem::take(&mut state.pending_auto_exclusions))
    }

    pub fn requeue_auto_exclusion_patch(&self, patch: AutoExclusionPatch) {
        let mut state = lock_unpoisoned(&self.shared.state);
        if state.stop_requested {
            return;
        }

        merge_auto_exclusion_patch(&mut state.pending_auto_exclusions, patch);
        let persisted_revision = state.persisted_revision;
        state.pending_auto_exclusions.base_revision = persisted_revision;
        state.pending_auto_exclusions_revision = Some(persisted_revision);
        state.pending_auto_exclusions_retry_at =
            Some(Instant::now() + AUTO_EXCLUSION_PATCH_RETRY_INTERVAL);
        self.shared
            .pending_auto_exclusions_generation
            .fetch_add(1, Ordering::Release);
    }

    pub fn request_app_suspension_path_action(
        &self,
        executable_path: &str,
        freeze: bool,
    ) -> Result<AppSuspensionPathActionReceiver, RuntimeCommandError> {
        if !Path::new(executable_path).is_absolute() {
            return Err(RuntimeCommandError::InvalidRequest(
                "App Suspension requires an absolute executable path.".to_owned(),
            ));
        }
        let (result, receiver) = sync_channel(1);
        self.enqueue_process_control_command(ProcessControlCommand::AppSuspensionPathAction {
            executable_path: executable_path.to_owned(),
            freeze,
            result,
        })?;
        Ok(receiver)
    }

    pub fn request_app_suspension_process_action(
        &self,
        targets: Vec<Result<ProcessActionTarget, ProcessActionTargetError>>,
        suspend: bool,
    ) -> Result<ProcessControlActionReceiver, RuntimeCommandError> {
        let (result, receiver) = sync_channel(1);
        self.enqueue_process_control_command(ProcessControlCommand::AppSuspension {
            targets,
            suspend,
            result,
        })?;
        Ok(receiver)
    }

    pub(crate) fn request_dynamic_priority_boost_action(
        &self,
        targets: Vec<Result<ProcessActionTarget, ProcessActionTargetError>>,
        state: DynamicPriorityBoostState,
    ) -> Result<ProcessControlActionReceiver, RuntimeCommandError> {
        let (result, receiver) = sync_channel(1);
        self.enqueue_process_control_command(ProcessControlCommand::DynamicPriorityBoost {
            targets,
            state,
            result,
        })?;
        Ok(receiver)
    }

    pub(crate) fn request_process_priority_action(
        &self,
        targets: Vec<Result<ProcessActionTarget, ProcessActionTargetError>>,
        priority: ProcessPrioritySetting,
    ) -> Result<ProcessControlActionReceiver, RuntimeCommandError> {
        let (result, receiver) = sync_channel(1);
        self.enqueue_process_control_command(ProcessControlCommand::ProcessPriority {
            targets,
            priority,
            result,
        })?;
        Ok(receiver)
    }

    pub(crate) fn request_efficiency_mode_action(
        &self,
        targets: Vec<Result<ProcessActionTarget, ProcessActionTargetError>>,
        enabled: bool,
    ) -> Result<ProcessControlActionReceiver, RuntimeCommandError> {
        let (result, receiver) = sync_channel(1);
        self.enqueue_process_control_command(ProcessControlCommand::EfficiencyMode {
            targets,
            enabled,
            result,
        })?;
        Ok(receiver)
    }

    pub(crate) fn request_thread_priority_action(
        &self,
        targets: Vec<Result<ProcessActionTarget, ProcessActionTargetError>>,
        priority: ProcessThreadPrioritySetting,
    ) -> Result<ProcessControlActionReceiver, RuntimeCommandError> {
        let (result, receiver) = sync_channel(1);
        self.enqueue_process_control_command(ProcessControlCommand::ThreadPriority {
            targets,
            priority,
            result,
        })?;
        Ok(receiver)
    }

    pub(crate) fn request_io_priority_action(
        &self,
        targets: Vec<Result<ProcessActionTarget, ProcessActionTargetError>>,
        priority: ProcessIoPriority,
    ) -> Result<ProcessControlActionReceiver, RuntimeCommandError> {
        let (result, receiver) = sync_channel(1);
        self.enqueue_process_control_command(ProcessControlCommand::IoPriority {
            targets,
            priority,
            result,
        })?;
        Ok(receiver)
    }

    pub(crate) fn request_gpu_priority_action(
        &self,
        targets: Vec<Result<ProcessActionTarget, ProcessActionTargetError>>,
        priority: ProcessGpuPriority,
    ) -> Result<ProcessControlActionReceiver, RuntimeCommandError> {
        let (result, receiver) = sync_channel(1);
        self.enqueue_process_control_command(ProcessControlCommand::GpuPriority {
            targets,
            priority,
            result,
        })?;
        Ok(receiver)
    }

    pub(crate) fn request_memory_priority_action(
        &self,
        targets: Vec<Result<ProcessActionTarget, ProcessActionTargetError>>,
        priority: ProcessMemoryPriority,
    ) -> Result<ProcessControlActionReceiver, RuntimeCommandError> {
        let (result, receiver) = sync_channel(1);
        self.enqueue_process_control_command(ProcessControlCommand::MemoryPriority {
            targets,
            priority,
            result,
        })?;
        Ok(receiver)
    }

    fn enqueue_process_control_command(
        &self,
        command: ProcessControlCommand,
    ) -> Result<(), RuntimeCommandError> {
        let settings = {
            let mut worker = lock_unpoisoned(&self.shared.state);
            if worker.stop_requested {
                return Err(RuntimeCommandError::RuntimeStopped);
            }
            if worker.process_control_commands.len() >= PROCESS_CONTROL_COMMAND_QUEUE_CAPACITY {
                return Err(RuntimeCommandError::QueueFull);
            }
            worker.process_control_commands.push_back(command);
            worker.change_generation = worker.change_generation.wrapping_add(1);
            self.shared.changed.notify_one();
            RuntimeSettingsSnapshot {
                runtime_revision: worker.runtime_revision,
                persisted_revision: worker.persisted_revision,
                value: Arc::clone(&worker.settings),
            }
        };
        self.sync_worker(&settings, true);
        Ok(())
    }

    pub(crate) fn request_power_plan_switch(
        &self,
        guid: String,
    ) -> Result<Receiver<Result<(), RuntimeCommandError>>, RuntimeCommandError> {
        let (result, receiver) = sync_channel(1);
        self.enqueue_process_control_command(ProcessControlCommand::SwitchPowerPlan {
            guid,
            result,
        })?;
        Ok(receiver)
    }

    pub(crate) fn request_memory_trim_now(
        &self,
        result: SyncSender<Result<MemoryTrimSnapshot, RuntimeCommandError>>,
    ) -> Result<(), RuntimeCommandError> {
        self.enqueue_process_control_command(ProcessControlCommand::MemoryTrim { result })
    }

    pub(crate) fn request_process_termination(
        &self,
        targets: Vec<ProcessActionTarget>,
    ) -> Result<ProcessTerminationActionReceiver, RuntimeCommandError> {
        let (result, receiver) = sync_channel(1);
        self.enqueue_process_control_command(ProcessControlCommand::StopProcesses {
            targets,
            result,
        })?;
        Ok(receiver)
    }

    pub(crate) fn request_stop_tree_intent(
        &self,
        roots: &[ProcessActionTarget],
    ) -> Result<(), RuntimeCommandError> {
        self.enqueue_process_control_command(ProcessControlCommand::StopTreeIntent {
            roots: roots.to_vec(),
        })
    }

    pub(crate) fn request_keep_running_resume(
        &self,
        executable_path: String,
    ) -> Result<(), RuntimeCommandError> {
        self.enqueue_process_control_command(ProcessControlCommand::KeepRunningResume {
            executable_path,
        })
    }

    fn sync_worker(&self, settings: &RuntimeSettingsSnapshot, start_requested: bool) {
        let _lifecycle = lock_unpoisoned(&self.lifecycle);
        if lock_unpoisoned(&self.shared.state).stop_requested {
            return;
        }
        let mut thread = lock_unpoisoned(&self.thread);
        let worker_accepting_work = lock_unpoisoned(&self.shared.state).worker_accepting_work;

        if thread.as_ref().is_some_and(|thread| thread.is_finished())
            || (thread.is_some() && !worker_accepting_work)
        {
            if let Some(thread) = thread.take() {
                match thread.join() {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => update_worker_error(
                        &self.shared,
                        Some(format!(
                            "Background automation worker exited unexpectedly: {error}"
                        )),
                    ),
                    Err(panic) => {
                        let reason = panic
                            .downcast::<&str>()
                            .map(|error| error.to_string())
                            .or_else(|panic| panic.downcast::<String>().map(|error| *error))
                            .unwrap_or_else(|_| "unknown panic".to_owned());
                        update_worker_error(
                            &self.shared,
                            Some(format!(
                                "Background automation worker panicked unexpectedly: {reason}"
                            )),
                        );
                    }
                }
            }
            lock_unpoisoned(&self.shared.state).worker_accepting_work = false;
        }

        if (start_requested || automation_worker_required(&settings.value)) && thread.is_none() {
            let thread_shared = Arc::clone(&self.shared);
            lock_unpoisoned(&self.shared.state).worker_accepting_work = true;
            *thread = Some(thread::spawn(move || {
                run_background_automation(
                    thread_shared,
                    power_source::is_plugged_in,
                    tray::is_hidden_to_tray,
                )
            }));
        }
    }

    fn sync_windows_event_watcher(&self, settings: &RuntimeSettingsSnapshot) {
        let _lifecycle = lock_unpoisoned(&self.lifecycle);
        if lock_unpoisoned(&self.shared.state).stop_requested {
            return;
        }
        let mode = windows_event_watcher_mode(&settings.value);
        let mut watcher = lock_unpoisoned(&self.event_watcher);
        if watcher
            .as_ref()
            .is_some_and(|watcher| Some(watcher.mode()) != mode || !watcher.is_running())
        {
            *watcher = None;
        }

        if let Some(mode) = mode.filter(|_| watcher.is_none()) {
            let shared = Arc::clone(&self.shared);
            let stopped_shared = Arc::clone(&self.shared);
            *watcher = WindowsEventWatcher::start(
                mode,
                Arc::new(move |event| notify_windows_event(&shared, event)),
                Arc::new(move || set_windows_event_watcher_active(&stopped_shared, false)),
            )
            .ok();
        }

        // Event-driven safety gates and polling fallbacks require full watcher coverage.
        set_windows_event_watcher_active(
            &self.shared,
            watcher.as_ref().is_some_and(|watcher| {
                watcher.mode() == WindowsEventWatcherMode::Full && watcher.is_running()
            }),
        );
        // The watcher may stop between the liveness read and status publication.
        if watcher
            .as_ref()
            .is_some_and(|watcher| !watcher.is_running())
        {
            set_windows_event_watcher_active(&self.shared, false);
        }
    }

    fn sync_input_hook(&self, settings: &RuntimeSettingsSnapshot) {
        let _lifecycle = lock_unpoisoned(&self.lifecycle);
        if lock_unpoisoned(&self.shared.state).stop_requested {
            return;
        }
        let mut input_hook = lock_unpoisoned(&self.input_hook);
        let config = input_hook_config(&settings.value);
        if config == InputHookConfig::default() {
            input_hook.take();
            configure_input_activity(&self.shared, config);
            return;
        }

        if input_hook
            .as_ref()
            .is_some_and(|input_hook| input_hook.config() == config)
        {
            return;
        }
        input_hook.take();
        configure_input_activity(&self.shared, InputHookConfig::default());
        let shared = Arc::clone(&self.shared);
        match InputHook::install(
            config,
            Arc::new(move |events| notify_input_event(&shared, events)),
        ) {
            Ok(installed) => {
                *input_hook = Some(installed);
                configure_input_activity(&self.shared, config);
            }
            Err(error) => update_worker_error(&self.shared, Some(error)),
        }
    }
}

fn report_self_power_result(
    shared: &SharedAutomationState,
    previous_error: &Mutex<Option<String>>,
    result: Result<(), String>,
) {
    let error = result.err();
    let mut previous = lock_unpoisoned(previous_error);
    if error != *previous {
        if let Some(error) = &error {
            crate::backend::diagnostics::error(error);
        }
    }
    let mut state = lock_unpoisoned(&shared.state);
    if (state.status.worker_error.is_none() || state.status.worker_error == *previous)
        && state.status.worker_error != error
    {
        state.status.worker_error = error.clone();
        bump_status_generation(shared, &mut state);
    }
    *previous = error;
}

impl Drop for RuntimeHandle {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}

struct AutomationWorkerExitGuard<'a> {
    shared: &'a SharedAutomationState,
}

impl Drop for AutomationWorkerExitGuard<'_> {
    fn drop(&mut self) {
        let mut state = lock_unpoisoned(&self.shared.state);
        let exited_while_accepting_work = state.worker_accepting_work;
        state.worker_accepting_work = false;
        if state.status.by_activity.take().is_some() {
            bump_status_generation(self.shared, &mut state);
        }
        if exited_while_accepting_work {
            for command in state.process_control_commands.drain(..) {
                command.reject(RuntimeCommandError::WorkerExited);
            }
        }
        self.shared.changed.notify_all();
    }
}

fn run_background_automation(
    shared: Arc<SharedAutomationState>,
    power_source: impl Fn() -> Option<bool>,
    tray_hidden: impl Fn() -> bool,
) -> Result<(), String> {
    let _exit_guard = AutomationWorkerExitGuard { shared: &shared };
    let mut runner = RuntimeCore::for_worker(&shared)?;
    let mut scheduler = RefreshScheduler::new(Instant::now());
    let mut cpu_allocation_reconciliation_retry_interval =
        CPU_ALLOCATION_RECONCILIATION_RETRY_INITIAL;

    while let Some(snapshot) = automation_snapshot(&shared) {
        if snapshot.action_log_clear_requested {
            runner.action_log.clear();
        }
        runner.action_log.begin_batch();
        let plugged_in = power_source();
        runner.replay_sleep_prevention_settings(&snapshot, plugged_in);
        let configured_settings = snapshot.settings;
        let settings = active_power_source_settings(configured_settings.as_ref(), plugged_in);
        let change_generation = snapshot.change_generation;
        let cpu_allocation_release_retry_pending_at_pass_start =
            runner.cpu_allocation_release_retry_pending();
        let process_control_commands = snapshot.process_control_commands;
        let memory_trim_command_requested = process_control_commands
            .iter()
            .any(ProcessControlCommand::is_memory_trim);
        let app_suspension_command_requested = process_control_commands
            .iter()
            .any(ProcessControlCommand::is_app_suspension);
        let keep_running_command_requested = process_control_commands
            .iter()
            .any(|command| matches!(command, ProcessControlCommand::KeepRunningResume { .. }));
        let wake_events = snapshot.wake_events;
        runner
            .keep_running
            .invalidate_settings(&snapshot.keep_running_resets);
        runner.keep_running.sync_settings(
            &configured_settings.keep_running,
            configured_settings.general.enabled,
        );
        if snapshot.instance_limits_resets.all {
            runner
                .instance_limits
                .sync(&configured_settings.instance_limits, false);
        } else {
            runner
                .instance_limits
                .reset_paths(&snapshot.instance_limits_resets.paths);
        }
        if snapshot.instance_limits_resets.notification_opt_in
            && configured_settings.instance_limits.notify
        {
            runner.instance_limits.requalify_notifications();
        }
        runner.instance_limits.sync(
            &configured_settings.instance_limits,
            configured_settings.general.enabled,
        );
        let instance_balancing_reset = snapshot.instance_balancing_resets.all
            || !snapshot.instance_balancing_resets.paths.is_empty();
        runner.instance_balancing.sync(
            &configured_settings.instance_balancing,
            configured_settings.general.enabled,
            snapshot.instance_balancing_resets,
        );
        let instance_observation_context_changed = wake_events.suspend_resume
            || wake_events.session_changed
            || wake_events.windows_event_watcher_change.is_some();
        if instance_observation_context_changed {
            runner.instance_balancing.invalidate();
            runner
                .instance_limits
                .invalidate(&t!("instance_limits.waiting_context"));
        }
        if snapshot.windows_event_watcher_active
            && wake_events.windows_event_watcher_change == Some(WindowsEventWatcherChange::Lost)
        {
            runner
                .keep_running
                .pause_all(PauseReason::UnsupportedContext);
        }
        if wake_events.suspend_resume {
            runner.keep_running.pause_all(PauseReason::Suspend);
        }
        if wake_events.session_changed {
            runner.keep_running.pause_all(PauseReason::SessionChanged);
        }
        runner.input_activity = snapshot.input_activity;
        let windows_event_watcher_active = snapshot.windows_event_watcher_active;
        let cpu_limiter_policy_varies = (plugged_in.is_none() || !windows_event_watcher_active)
            && cpu_limiter_policy_varies_by_source(&configured_settings);
        let cpu_limiter_waits_for_source = plugged_in.is_none() && cpu_limiter_policy_varies;
        let cpu_limiter_active = !cpu_limiter_waits_for_source
            && feature_refresh_required(settings, cpu_limiter_required(settings));
        let app_suspension_policy_varies = (plugged_in.is_none() || !windows_event_watcher_active)
            && app_suspension_policy_varies_by_source(&configured_settings);
        let app_suspension_waits_for_source = plugged_in.is_none() && app_suspension_policy_varies;
        let memory_trim_policy_difference = if plugged_in.is_none() || !windows_event_watcher_active
        {
            memory_trim_policy_difference(&configured_settings)
        } else {
            MemoryTrimPolicyDifference::Common
        };
        let memory_trim_waits_for_source = plugged_in.is_none()
            && memory_trim_policy_difference != MemoryTrimPolicyDifference::Common;
        let manual_trim_waits_for_source = memory_trim_waits_for_source
            && memory_trim_policy_difference == MemoryTrimPolicyDifference::ManualAndAutomatic;
        let memory_trim_active = !memory_trim_waits_for_source
            && feature_refresh_required(settings, settings.memory_trim.enabled);
        let mut observations = CycleObservations::default();
        let hidden_to_tray = tray_hidden();
        let adaptive_engine_enabled = settings.adaptive_engine.enabled;
        let background_efficiency_refresh_interval = automation_refresh_interval(
            hidden_to_tray,
            adaptive_engine_enabled,
            ECO_QOS_REFRESH_INTERVAL,
        );
        let app_suspension_refresh_interval = if app_suspension_waits_for_source {
            APP_SUSPENSION_REFRESH_INTERVAL
        } else {
            automation_refresh_interval(
                hidden_to_tray,
                adaptive_engine_enabled,
                APP_SUSPENSION_REFRESH_INTERVAL,
            )
        };
        let cpu_sets_soft_refresh_interval = automation_refresh_interval(
            hidden_to_tray,
            adaptive_engine_enabled,
            CPU_ALLOCATION_REFRESH_INTERVAL,
        );
        let processor_affinity_hard_refresh_interval = automation_refresh_interval(
            hidden_to_tray,
            adaptive_engine_enabled,
            CPU_ALLOCATION_REFRESH_INTERVAL,
        );
        let cpu_limiter_refresh_interval = CPU_LIMITER_REFRESH_INTERVAL;
        let by_running_app_refresh_interval = automation_refresh_interval(
            hidden_to_tray,
            adaptive_engine_enabled,
            PERFORMANCE_MODE_REFRESH_INTERVAL,
        );
        let adaptive_engine_process_refresh_interval =
            adaptive_engine_process_refresh_interval(settings);
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
        let memory_trim_refresh_interval = if memory_trim_waits_for_source {
            POWER_SOURCE_RECHECK_INTERVAL
        } else {
            automation_refresh_interval(
                hidden_to_tray,
                adaptive_engine_enabled,
                MEMORY_TRIM_REFRESH_INTERVAL,
            )
        };
        let timer_resolution_refresh_interval = automation_refresh_interval(
            hidden_to_tray,
            adaptive_engine_enabled,
            TIMER_RESOLUTION_REFRESH_INTERVAL,
        );
        let process_appearance_scan_interval = process_appearance_refresh_interval(
            hidden_to_tray,
            adaptive_engine_enabled,
            cpu_limiter_active,
        );
        let app_suspension_foreground_release_interval = automation_refresh_interval(
            hidden_to_tray,
            adaptive_engine_enabled,
            APP_SUSPENSION_FOREGROUND_RELEASE_INTERVAL,
        );
        let event_now = Instant::now();
        let settings_changed = runner.note_settings(&configured_settings, wake_events, plugged_in);
        if settings_changed {
            scheduler.invalidate(SchedulerEvent::SettingsChanged, event_now);
        }
        if wake_events.windows_event_watcher_change.is_some() {
            scheduler.schedule_now(RefreshDomain::PowerPlanCheck, event_now);
            scheduler.schedule_now(RefreshDomain::SleepPrevention, event_now);
        }
        if wake_events.foreground_changed {
            scheduler.invalidate(SchedulerEvent::ForegroundChanged, event_now);
        }
        if wake_events.window_created {
            scheduler.invalidate(SchedulerEvent::WindowCreated, event_now);
        }
        if wake_events.power_changed {
            scheduler.invalidate(SchedulerEvent::PowerChanged, event_now);
        }
        if wake_events.clock_changed {
            scheduler.invalidate(SchedulerEvent::ClockChanged, event_now);
        }
        if wake_events.session_changed {
            scheduler.invalidate(SchedulerEvent::SessionChanged, event_now);
        }
        if instance_observation_context_changed {
            scheduler.schedule_now(RefreshDomain::InstanceLimits, event_now);
        }
        if instance_balancing_reset || instance_observation_context_changed {
            scheduler.schedule_now(RefreshDomain::InstanceBalancing, event_now);
        }
        if keep_running_command_requested || instance_observation_context_changed {
            scheduler.schedule_now(RefreshDomain::KeepRunning, event_now);
        }
        if wake_events.power_changed || wake_events.session_changed {
            if let Err(ActivePlanCheckError::Operation(error)) = runner.refresh_active_plan(true) {
                update_worker_error(&shared, Some(error));
            }
        }
        if wake_events.input_activity {
            scheduler.invalidate(SchedulerEvent::InputActivity, event_now);
        }
        let controller_poll_required = controller_activity_poll_required(settings);
        if controller_poll_required
            && scheduler.is_due(RefreshDomain::ControllerActivity, event_now)
        {
            if runner.poll_controller_activity(event_now) {
                scheduler.invalidate(SchedulerEvent::ControllerActivity, event_now);
            }
            scheduler.schedule_after(
                RefreshDomain::ControllerActivity,
                event_now,
                CONTROLLER_ACTIVITY_POLL_INTERVAL,
            );
        } else if !controller_poll_required {
            runner.clear_controller_activity();
            scheduler.schedule_now(RefreshDomain::ControllerActivity, event_now);
        }
        if wake_events.app_switch || wake_events.app_switch_mouse_click {
            scheduler.invalidate(
                if wake_events.app_switch {
                    SchedulerEvent::AppSwitch
                } else {
                    SchedulerEvent::AppSwitchMouseClick
                },
                event_now,
            );
            if runner.app_suspension_active() {
                let app_suspension_status = if wake_events.app_switch {
                    runner.run_app_suspension_app_switch_release(&mut observations)
                } else {
                    runner.run_app_suspension_shell_click_release()
                };
                if let Some(app_suspension_status) = app_suspension_status {
                    update_app_suspension_status(
                        &shared,
                        app_suspension_release_status(
                            app_suspension_status,
                            settings,
                            app_suspension_waits_for_source,
                        ),
                    );
                }
            }
        }
        let now = Instant::now();
        let power_plan_waits_for_source =
            ordinary_power_plan_waits_for_source(&configured_settings, plugged_in);
        let power_plan_checks_required = power_plan_waits_for_source
            || power_plan_checks_required(settings)
            || runner.has_temporary_power_plan();
        let scan_process_appearance = process_appearance_scan_required_for_runtime(
            settings,
            cpu_limiter_active,
            memory_trim_active,
        );
        let background_efficiency_active =
            feature_refresh_required(settings, settings.background_efficiency.enabled);
        let background_efficiency_policy_varies = (plugged_in.is_none()
            || !windows_event_watcher_active)
            && background_efficiency_policy_varies_by_source(&configured_settings);
        let background_efficiency_waits_for_source =
            plugged_in.is_none() && background_efficiency_policy_varies;
        let background_efficiency_refresh_required = settings_changed
            || (background_efficiency_active && !background_efficiency_waits_for_source);
        let app_suspension_refresh_required = settings_changed
            || app_suspension_waits_for_source
            || feature_refresh_required(settings, app_suspension_required(settings))
            || app_suspension_command_requested
            || runner.app_suspension_active();
        let cpu_sets_soft_refresh_required = settings_changed
            || feature_refresh_required(settings, cpu_sets_soft_required(settings));
        let processor_affinity_hard_refresh_required = settings_changed
            || feature_refresh_required(settings, processor_affinity_hard_required(settings));
        let cpu_limiter_refresh_requested = settings_changed || cpu_limiter_active;
        let cpu_limiter_release_pending = runner.cpu_limiter_release_pending();
        let cpu_limiter_refresh_required = cpu_limiter_refresh_requested
            || cpu_limiter_release_pending
            || cpu_limiter_waits_for_source;
        let by_running_app_refresh_required = settings_changed
            || feature_refresh_required(settings, by_running_app_required(settings))
            || runner.has_temporary_power_plan();
        let adaptive_engine_process_refresh_required = settings_changed
            || feature_refresh_required(settings, adaptive_engine_process_required(settings));
        let bottleneck_classifier_refresh_required =
            !hidden_to_tray && bottleneck_classifier_required(settings);
        let adaptive_waits_for_source = runner.adaptive_power_plan_waiting_for_source();
        let adaptive_power_plan_refresh_interval = if adaptive_waits_for_source {
            configured_check_interval(settings)
        } else {
            ADAPTIVE_POWER_PLAN_REFRESH_INTERVAL
        };
        let adaptive_power_plan_refresh_required = adaptive_waits_for_source
            || settings_changed
            || feature_refresh_required(settings, adaptive_power_plan_required(settings))
            || runner.adaptive_power_plan_managed();
        let process_priority_schedule = priority_source_schedule(
            &configured_settings,
            settings,
            plugged_in,
            windows_event_watcher_active,
            settings_changed,
            |profile| (&profile.process_priority, profile.process_priority.enabled),
        );
        let thread_priority_schedule = priority_source_schedule(
            &configured_settings,
            settings,
            plugged_in,
            windows_event_watcher_active,
            settings_changed,
            |profile| (&profile.thread_priority, profile.thread_priority.enabled),
        );
        let dynamic_priority_boost_schedule = priority_source_schedule(
            &configured_settings,
            settings,
            plugged_in,
            windows_event_watcher_active,
            settings_changed,
            |profile| {
                (
                    &profile.dynamic_priority_boost,
                    profile.dynamic_priority_boost.enabled,
                )
            },
        );
        let io_priority_refresh_required =
            settings_changed || feature_refresh_required(settings, settings.io_priority.enabled);
        let gpu_priority_schedule = priority_source_schedule(
            &configured_settings,
            settings,
            plugged_in,
            windows_event_watcher_active,
            settings_changed,
            |profile| (&profile.gpu_priority, profile.gpu_priority.enabled),
        );
        let memory_priority_refresh_required = settings_changed
            || feature_refresh_required(settings, settings.memory_priority.enabled);
        let watchdog_refresh_required = settings_changed
            || feature_refresh_required(
                configured_settings.as_ref(),
                watchdog_required(configured_settings.as_ref()),
            );
        let keep_running_configured = keep_running_required(configured_settings.as_ref());
        let keep_running_sampling_required =
            keep_running_configured && windows_event_watcher_active;
        let keep_running_refresh_required = settings_changed
            || keep_running_command_requested
            || keep_running_sampling_required
            || (keep_running_configured && instance_observation_context_changed)
            || runner.keep_running.has_pending();
        let instance_limits_configured = instance_limits_required(configured_settings.as_ref());
        let instance_limits_sampling_required =
            instance_limits_configured && windows_event_watcher_active;
        let instance_limits_refresh_required = settings_changed
            || instance_limits_sampling_required
            || (instance_limits_configured && instance_observation_context_changed);
        let instance_balancing_sampling_required = windows_event_watcher_active
            && instance_balancing_required(configured_settings.as_ref());
        let instance_balancing_refresh_required = settings_changed
            || instance_balancing_reset
            || instance_observation_context_changed
            || instance_balancing_sampling_required
            || runner.instance_balancing.active();
        let memory_trim_refresh_required = settings_changed
            || memory_trim_command_requested
            || memory_trim_active
            || memory_trim_waits_for_source;
        let timer_resolution_waits_for_source =
            plugged_in.is_none() && timer_resolution_policy_varies_by_source(&configured_settings);
        let timer_resolution_active = (!timer_resolution_waits_for_source
            && feature_refresh_required(settings, timer_resolution_required(settings)))
            || runner.timer_resolution_request_active();
        let timer_resolution_refresh_required = settings_changed || timer_resolution_active;
        let sleep_power_source = plugged_in.or(runner.sleep_prevention_power_source());
        let sleep_settings = active_power_source_settings(&configured_settings, sleep_power_source);
        let sleep_prevention_refresh_required = settings_changed
            || feature_refresh_required(sleep_settings, sleep_prevention_required(sleep_settings))
            || (plugged_in.is_none()
                && std::iter::once(configured_settings.as_ref())
                    .chain(configured_settings.on_battery.as_deref())
                    .any(|settings| {
                        feature_refresh_required(settings, sleep_prevention_required(settings))
                    }));
        if app_suspension_command_requested {
            scheduler.invalidate(SchedulerEvent::AppSuspensionRequested, now);
        }
        if memory_trim_command_requested {
            scheduler.invalidate(SchedulerEvent::MemoryTrimRequested, now);
        }

        if scan_process_appearance && scheduler.is_due(RefreshDomain::ProcessAppearance, now) {
            if runner.detect_process_appearance() {
                scheduler.invalidate(SchedulerEvent::ProcessAppeared, now);
            }
            scheduler.schedule_after(
                RefreshDomain::ProcessAppearance,
                now,
                process_appearance_scan_interval,
            );
        } else if !scan_process_appearance {
            runner.known_process_ids.clear();
            scheduler.schedule_after(
                RefreshDomain::ProcessAppearance,
                now,
                process_appearance_scan_interval,
            );
        }

        if runner.app_suspension_active()
            && scheduler.is_due(RefreshDomain::AppSuspensionForegroundRelease, now)
        {
            if let Some(app_suspension_status) =
                runner.run_app_suspension_foreground_release(&mut observations)
            {
                update_app_suspension_status(
                    &shared,
                    app_suspension_release_status(
                        app_suspension_status,
                        settings,
                        app_suspension_waits_for_source,
                    ),
                );
            }
            scheduler.schedule_after(
                RefreshDomain::AppSuspensionForegroundRelease,
                now,
                app_suspension_foreground_release_interval,
            );
        }

        if background_efficiency_refresh_required
            && scheduler.is_due(RefreshDomain::BackgroundEfficiency, now)
        {
            let background_efficiency_status = runner.run_background_efficiency_update(
                settings,
                background_efficiency_waits_for_source,
                &mut observations,
            );
            update_background_efficiency_status(&shared, background_efficiency_status);
            scheduler.schedule_after(
                RefreshDomain::BackgroundEfficiency,
                now,
                background_efficiency_refresh_interval,
            );
        }
        if process_priority_schedule.refresh_required
            && scheduler.is_due(RefreshDomain::ProcessPriority, now)
        {
            let process_priority_status = runner.run_process_priority_update(
                settings,
                process_priority_schedule.waiting_for_source,
                &mut observations,
            );
            update_process_priority_status(&shared, process_priority_status);
            scheduler.schedule_after(
                RefreshDomain::ProcessPriority,
                now,
                process_priority_refresh_interval,
            );
        }
        let adaptive_engine_process_status = if adaptive_engine_process_refresh_required
            && scheduler.is_due(RefreshDomain::AdaptiveEngineProcess, now)
        {
            let status = runner.run_adaptive_engine_process_update(settings, &mut observations);
            scheduler.schedule_after(
                RefreshDomain::AdaptiveEngineProcess,
                now,
                adaptive_engine_process_refresh_interval,
            );
            Some(status)
        } else {
            None
        };
        if wake_events.tray_visibility_changed
            || (settings_changed && !bottleneck_classifier_refresh_required)
        {
            let status = runner.reset_bottleneck_classifier();
            if bottleneck_classifier_refresh_required {
                scheduler.schedule_now(RefreshDomain::BottleneckClassifier, now);
            } else {
                update_bottleneck_classifier_status(&shared, status);
            }
        }
        if bottleneck_classifier_refresh_required
            && scheduler.is_due(RefreshDomain::BottleneckClassifier, now)
        {
            let status = runner.run_bottleneck_classifier_update();
            update_bottleneck_classifier_status(&shared, status);
            scheduler.schedule_after(
                RefreshDomain::BottleneckClassifier,
                now,
                BOTTLENECK_CLASSIFIER_REFRESH_INTERVAL,
            );
        }
        if io_priority_refresh_required && scheduler.is_due(RefreshDomain::IoPriority, now) {
            let io_priority_status = runner.run_io_priority_update(settings, &mut observations);
            update_io_priority_status(&shared, io_priority_status);
            scheduler.schedule_after(RefreshDomain::IoPriority, now, io_priority_refresh_interval);
        }
        if thread_priority_schedule.refresh_required
            && scheduler.is_due(RefreshDomain::ThreadPriority, now)
        {
            let thread_priority_status = runner.run_thread_priority_update(
                settings,
                thread_priority_schedule.waiting_for_source,
                &mut observations,
            );
            update_thread_priority_status(&shared, thread_priority_status);
            scheduler.schedule_after(
                RefreshDomain::ThreadPriority,
                now,
                thread_priority_refresh_interval,
            );
        }
        if dynamic_priority_boost_schedule.refresh_required
            && scheduler.is_due(RefreshDomain::DynamicPriorityBoost, now)
        {
            let dynamic_priority_boost_status = runner.run_dynamic_priority_boost_update(
                settings,
                dynamic_priority_boost_schedule.waiting_for_source,
                &mut observations,
            );
            update_dynamic_priority_boost_status(&shared, dynamic_priority_boost_status);
            scheduler.schedule_after(
                RefreshDomain::DynamicPriorityBoost,
                now,
                dynamic_priority_boost_refresh_interval,
            );
        }
        if gpu_priority_schedule.refresh_required
            && scheduler.is_due(RefreshDomain::GpuPriority, now)
        {
            let gpu_priority_status = runner.run_gpu_priority_update(
                settings,
                gpu_priority_schedule.waiting_for_source,
                &mut observations,
            );
            update_gpu_priority_status(&shared, gpu_priority_status);
            scheduler.schedule_after(
                RefreshDomain::GpuPriority,
                now,
                gpu_priority_refresh_interval,
            );
        }
        if memory_priority_refresh_required && scheduler.is_due(RefreshDomain::MemoryPriority, now)
        {
            let memory_priority_status =
                runner.run_memory_priority_update(settings, &mut observations);
            update_memory_priority_status(&shared, memory_priority_status);
            scheduler.schedule_after(
                RefreshDomain::MemoryPriority,
                now,
                memory_priority_refresh_interval,
            );
        }
        let command_statuses = if process_control_commands.is_empty() {
            ProcessControlCommandStatuses::default()
        } else {
            runner.run_process_control_commands(
                settings,
                app_suspension_waits_for_source,
                process_control_commands,
                &mut observations,
            )
        };
        let manual_memory_trim_processed = command_statuses.memory_trim.is_some();
        if let Some(status) = command_statuses.memory_trim {
            let status = if memory_trim_waits_for_source {
                MemoryTrimSnapshot {
                    auto_excluded_processes: status.auto_excluded_processes,
                    ..runner.pause_memory_trim_for_power_source(manual_trim_waits_for_source)
                }
            } else {
                status
            };
            update_memory_trim_status(&shared, status);
            scheduler.schedule_after(RefreshDomain::MemoryTrim, now, memory_trim_refresh_interval);
        }
        if let Some(status) = command_statuses.app_suspension {
            update_app_suspension_status(&shared, status);
            scheduler.schedule_after(
                RefreshDomain::AppSuspension,
                now,
                app_suspension_refresh_interval,
            );
        }
        if app_suspension_refresh_required && scheduler.is_due(RefreshDomain::AppSuspension, now) {
            let app_suspension_status = runner.run_app_suspension_update(
                settings,
                app_suspension_waits_for_source,
                &[],
                &mut observations,
            );
            update_app_suspension_status(&shared, app_suspension_status);
            scheduler.schedule_after(
                RefreshDomain::AppSuspension,
                now,
                app_suspension_refresh_interval,
            );
            if runner.app_suspension_active() {
                scheduler.schedule_now(RefreshDomain::AppSuspensionForegroundRelease, now);
            }
        }
        if cpu_sets_soft_refresh_required && scheduler.is_due(RefreshDomain::CpuSetsSoft, now) {
            let status = runner.run_cpu_sets_soft_update(settings, &mut observations);
            update_cpu_sets_soft_status(&shared, status);
            scheduler.schedule_after(
                RefreshDomain::CpuSetsSoft,
                now,
                cpu_sets_soft_refresh_interval,
            );
        }
        if processor_affinity_hard_refresh_required
            && scheduler.is_due(RefreshDomain::ProcessorAffinityHard, now)
        {
            let status = runner.run_processor_affinity_hard_update(settings, &mut observations);
            update_processor_affinity_hard_status(&shared, status);
            scheduler.schedule_after(
                RefreshDomain::ProcessorAffinityHard,
                now,
                processor_affinity_hard_refresh_interval,
            );
        }
        if instance_balancing_refresh_required
            && scheduler.is_due(RefreshDomain::InstanceBalancing, now)
        {
            let status = runner.run_instance_balancing_update(
                &shared,
                &configured_settings,
                settings,
                windows_event_watcher_active,
            );
            update_instance_balancing_status(&shared, status);
            scheduler.schedule_after(
                RefreshDomain::InstanceBalancing,
                now,
                INSTANCE_BALANCING_REFRESH_INTERVAL,
            );
        }
        if cpu_limiter_refresh_required && scheduler.is_due(RefreshDomain::CpuLimiter, now) {
            if cpu_limiter_refresh_requested
                || (cpu_limiter_release_pending && !runner.cpu_limiter_release_in_progress())
            {
                let cpu_limiter_status = CpuLimiterSnapshot {
                    source: plugged_in.map(|plugged_in| active_power_source(Some(plugged_in))),
                    waiting_for_power_source: cpu_limiter_waits_for_source,
                    ..runner.run_cpu_limiter_update(
                        settings,
                        cpu_limiter_waits_for_source,
                        &mut observations,
                    )
                };
                update_cpu_limiter_status(&shared, cpu_limiter_status);
            }
            scheduler.schedule_after(RefreshDomain::CpuLimiter, now, cpu_limiter_refresh_interval);
        }
        let immediate_cpu_allocation_reconciliation =
            runner.cpu_allocation_immediate_reconciliation_pending();
        let cpu_allocation_release_retry_due = cpu_allocation_release_retry_pending_at_pass_start
            && scheduler.is_due(RefreshDomain::CpuAllocationReconciliation, now);
        if immediate_cpu_allocation_reconciliation || cpu_allocation_release_retry_due {
            runner.run_cpu_allocation_reconciliation(
                &shared,
                settings,
                &mut observations,
                cpu_allocation_release_retry_due,
            );
        }
        let cpu_allocation_release_retry_pending = runner.cpu_allocation_release_retry_pending();
        if cpu_allocation_release_retry_due {
            if cpu_allocation_release_retry_pending {
                scheduler.schedule_after(
                    RefreshDomain::CpuAllocationReconciliation,
                    now,
                    cpu_allocation_reconciliation_retry_interval,
                );
                cpu_allocation_reconciliation_retry_interval =
                    next_cpu_allocation_reconciliation_retry_interval(
                        cpu_allocation_reconciliation_retry_interval,
                    );
            } else {
                cpu_allocation_reconciliation_retry_interval =
                    CPU_ALLOCATION_RECONCILIATION_RETRY_INITIAL;
                scheduler.schedule_now(RefreshDomain::CpuAllocationReconciliation, now);
            }
        } else if !cpu_allocation_release_retry_pending_at_pass_start
            && cpu_allocation_release_retry_pending
        {
            cpu_allocation_reconciliation_retry_interval =
                next_cpu_allocation_reconciliation_retry_interval(
                    CPU_ALLOCATION_RECONCILIATION_RETRY_INITIAL,
                );
            scheduler.schedule_after(
                RefreshDomain::CpuAllocationReconciliation,
                now,
                CPU_ALLOCATION_RECONCILIATION_RETRY_INITIAL,
            );
        } else if !cpu_allocation_release_retry_pending {
            cpu_allocation_reconciliation_retry_interval =
                CPU_ALLOCATION_RECONCILIATION_RETRY_INITIAL;
            scheduler.schedule_now(RefreshDomain::CpuAllocationReconciliation, now);
        }
        if by_running_app_refresh_required && scheduler.is_due(RefreshDomain::ByRunningApp, now) {
            if let Some(by_running_app_status) =
                runner.run_by_running_app_update(settings, &mut observations)
            {
                update_by_running_app_status(&shared, by_running_app_status);
            }
            scheduler.schedule_after(
                RefreshDomain::ByRunningApp,
                now,
                by_running_app_refresh_interval,
            );
        }
        if !manual_memory_trim_processed
            && memory_trim_refresh_required
            && scheduler.is_due(RefreshDomain::MemoryTrim, now)
        {
            let status = if !memory_trim_waits_for_source {
                Some(runner.run_memory_trim_update(settings, &mut observations))
            } else if settings_changed {
                Some(runner.pause_memory_trim_for_power_source(manual_trim_waits_for_source))
            } else {
                None
            };
            let (scheduled_at, interval) = if status
                .as_ref()
                .is_some_and(|status| status.has_deferred_candidates)
            {
                (Instant::now(), MEMORY_TRIM_FOLLOW_UP_INTERVAL)
            } else {
                (now, memory_trim_refresh_interval)
            };
            if let Some(status) = status {
                update_memory_trim_status(&shared, status);
            }
            scheduler.schedule_after(RefreshDomain::MemoryTrim, scheduled_at, interval);
        }
        if watchdog_refresh_required && scheduler.is_due(RefreshDomain::Watchdog, now) {
            let status = runner.run_watchdog_update(
                &shared,
                configured_settings.as_ref(),
                &mut observations,
            );
            update_feature_status(
                &shared,
                status,
                |status| &status.watchdog,
                |status| &mut status.watchdog,
            );
            scheduler.schedule_after(RefreshDomain::Watchdog, now, Duration::from_secs(1));
        }
        if keep_running_refresh_required && scheduler.is_due(RefreshDomain::KeepRunning, now) {
            let status = runner.keep_running.update(
                &configured_settings.keep_running,
                configured_settings.general.enabled,
                windows_event_watcher_active,
                |_, manager, ticket, now, observation| {
                    reserve_keep_running_commit(
                        &shared,
                        &configured_settings,
                        windows_event_watcher_active,
                        manager,
                        ticket,
                        now,
                        observation,
                    )
                },
                &mut runner.action_log,
            );
            update_keep_running_status(&shared, status);
            scheduler.schedule_after(
                RefreshDomain::KeepRunning,
                Instant::now(),
                KEEP_RUNNING_REFRESH_INTERVAL,
            );
        }
        if instance_limits_refresh_required && scheduler.is_due(RefreshDomain::InstanceLimits, now)
        {
            let status = runner.run_instance_limits_update(
                &shared,
                &configured_settings,
                windows_event_watcher_active,
            );
            instance_limits::publish_status(
                &shared,
                &configured_settings,
                windows_event_watcher_active,
                status,
            );
            scheduler.schedule_after(
                RefreshDomain::InstanceLimits,
                Instant::now(),
                INSTANCE_LIMITS_REFRESH_INTERVAL,
            );
        }
        if timer_resolution_refresh_required
            && scheduler.is_due(RefreshDomain::TimerResolution, now)
        {
            let timer_resolution_status = runner.run_timer_resolution_update(
                settings,
                timer_resolution_waits_for_source,
                &mut observations,
            );
            update_timer_resolution_status(&shared, timer_resolution_status);
            scheduler.schedule_after(
                RefreshDomain::TimerResolution,
                now,
                timer_resolution_refresh_interval,
            );
        }

        if sleep_prevention_refresh_required
            && scheduler.is_due(RefreshDomain::SleepPrevention, now)
        {
            let status = runner.run_sleep_prevention_update(
                configured_settings.as_ref(),
                &mut observations,
                windows_event_watcher_active,
                plugged_in,
            );
            update_sleep_prevention_status(&shared, status);
            scheduler.schedule_after(
                RefreshDomain::SleepPrevention,
                now,
                SLEEP_PREVENTION_REFRESH_INTERVAL,
            );
        }

        // Reuse the catalogue if a process policy has already requested it this pass.
        if adaptive_power_plan_refresh_required
            && scheduler.is_due(RefreshDomain::AdaptivePowerPlan, now)
        {
            if adaptive_waits_for_source {
                if let Err(ActivePlanCheckError::Operation(error)) =
                    runner.refresh_active_plan(false)
                {
                    update_worker_error(&shared, Some(error));
                }
            } else {
                runner.run_adaptive_power_plan_update(settings, &mut observations);
            }
            scheduler.schedule_after(
                RefreshDomain::AdaptivePowerPlan,
                now,
                adaptive_power_plan_refresh_interval,
            );
        }
        if let Some(error) = runner.retry_control_releases(&shared, Instant::now()) {
            update_worker_error(&shared, Some(error));
        }
        if let Some(status) = adaptive_engine_process_status {
            runner.publish_adaptive_engine_process_status(
                &shared,
                active_power_source(plugged_in),
                status,
            );
        }
        let wait_now = Instant::now();
        let mut wait_for = if power_plan_checks_required {
            run_scheduled_power_plan_check(&mut scheduler, wait_now, || {
                let activity = runner.activity_snapshot(settings, wait_now);
                let wall_time = Local::now();
                if let Err(ActivePlanCheckError::Operation(error)) = runner.run_check(
                    &configured_settings,
                    plugged_in,
                    wall_time,
                    activity.state,
                    &mut observations,
                ) {
                    update_worker_error(&shared, Some(error));
                }
                let delay = if power_plan_waits_for_source {
                    Some(configured_check_interval(settings))
                } else {
                    power_plan_check_delay(
                        settings,
                        windows_event_watcher_active,
                        activity.idle_for,
                        wall_time,
                        runner.paused_cpu_load_readback_delay(settings, plugged_in, wait_now),
                    )
                };
                runner
                    .temporary_power_plan_retry_delay()
                    .map_or(delay, |retry| Some(min_worker_wait(delay, retry)))
            })
        } else {
            runner.clear_ordinary_power_plan_decision();
            scheduler.schedule_now(RefreshDomain::PowerPlanCheck, wait_now);
            None
        };
        if let Some(status) = runner
            .instance_balancing
            .take_settled_release_status(&runner.cpu_allocation_coordinator)
        {
            update_instance_balancing_status(&shared, status);
        }
        runner.publish_action_log_if_changed(&shared);
        let by_activity =
            runner.by_activity_observation(settings, active_power_source(plugged_in), wait_now);
        let (power_plan_status, rule_context) = runner.power_plan_status();
        update_power_plan_status(&shared, power_plan_status, rule_context, by_activity);

        wait_for = scheduler.minimum_wait(
            wait_for,
            wait_now,
            [
                (
                    watchdog_refresh_required,
                    RefreshDomain::Watchdog,
                    Duration::from_secs(1),
                ),
                (
                    keep_running_sampling_required || runner.keep_running.has_pending(),
                    RefreshDomain::KeepRunning,
                    KEEP_RUNNING_REFRESH_INTERVAL,
                ),
                (
                    instance_limits_sampling_required,
                    RefreshDomain::InstanceLimits,
                    INSTANCE_LIMITS_REFRESH_INTERVAL,
                ),
                (
                    instance_balancing_sampling_required || runner.instance_balancing.active(),
                    RefreshDomain::InstanceBalancing,
                    INSTANCE_BALANCING_REFRESH_INTERVAL,
                ),
                (
                    background_efficiency_refresh_required,
                    RefreshDomain::BackgroundEfficiency,
                    background_efficiency_refresh_interval,
                ),
                (
                    app_suspension_refresh_required,
                    RefreshDomain::AppSuspension,
                    app_suspension_refresh_interval,
                ),
                (
                    cpu_sets_soft_refresh_required,
                    RefreshDomain::CpuSetsSoft,
                    cpu_sets_soft_refresh_interval,
                ),
                (
                    processor_affinity_hard_refresh_required,
                    RefreshDomain::ProcessorAffinityHard,
                    processor_affinity_hard_refresh_interval,
                ),
                (
                    cpu_limiter_refresh_required,
                    RefreshDomain::CpuLimiter,
                    cpu_limiter_refresh_interval,
                ),
                (
                    runner.cpu_allocation_release_retry_pending(),
                    RefreshDomain::CpuAllocationReconciliation,
                    cpu_allocation_reconciliation_retry_interval,
                ),
                (
                    by_running_app_refresh_required,
                    RefreshDomain::ByRunningApp,
                    by_running_app_refresh_interval,
                ),
                (
                    adaptive_engine_process_refresh_required,
                    RefreshDomain::AdaptiveEngineProcess,
                    adaptive_engine_process_refresh_interval,
                ),
                (
                    bottleneck_classifier_refresh_required,
                    RefreshDomain::BottleneckClassifier,
                    BOTTLENECK_CLASSIFIER_REFRESH_INTERVAL,
                ),
                (
                    adaptive_power_plan_refresh_required,
                    RefreshDomain::AdaptivePowerPlan,
                    adaptive_power_plan_refresh_interval,
                ),
                (
                    process_priority_schedule.refresh_required,
                    RefreshDomain::ProcessPriority,
                    process_priority_refresh_interval,
                ),
                (
                    thread_priority_schedule.refresh_required,
                    RefreshDomain::ThreadPriority,
                    thread_priority_refresh_interval,
                ),
                (
                    dynamic_priority_boost_schedule.refresh_required,
                    RefreshDomain::DynamicPriorityBoost,
                    dynamic_priority_boost_refresh_interval,
                ),
                (
                    io_priority_refresh_required,
                    RefreshDomain::IoPriority,
                    io_priority_refresh_interval,
                ),
                (
                    gpu_priority_schedule.refresh_required,
                    RefreshDomain::GpuPriority,
                    gpu_priority_refresh_interval,
                ),
                (
                    memory_priority_refresh_required,
                    RefreshDomain::MemoryPriority,
                    memory_priority_refresh_interval,
                ),
                (
                    memory_trim_refresh_required,
                    RefreshDomain::MemoryTrim,
                    memory_trim_refresh_interval,
                ),
                (
                    timer_resolution_refresh_required,
                    RefreshDomain::TimerResolution,
                    timer_resolution_refresh_interval,
                ),
                (
                    sleep_prevention_refresh_required,
                    RefreshDomain::SleepPrevention,
                    SLEEP_PREVENTION_REFRESH_INTERVAL,
                ),
                (
                    scan_process_appearance,
                    RefreshDomain::ProcessAppearance,
                    process_appearance_scan_interval,
                ),
                (
                    controller_poll_required,
                    RefreshDomain::ControllerActivity,
                    CONTROLLER_ACTIVITY_POLL_INTERVAL,
                ),
                (
                    runner.app_suspension_active(),
                    RefreshDomain::AppSuspensionForegroundRelease,
                    app_suspension_foreground_release_interval,
                ),
            ],
        );
        if runner.cpu_allocation_immediate_reconciliation_pending() {
            wait_for = Some(min_worker_wait(wait_for, Duration::ZERO));
        }
        if automation_worker_can_exit(
            wait_for,
            automation_worker_required(configured_settings.as_ref()),
            runner.has_managed_process_control_state(),
        ) {
            if !worker_exit_is_still_idle(&shared, change_generation) {
                continue;
            }
            let shutdown_result = runner.shutdown();
            let (power_plan_status, rule_context) = runner.power_plan_status();
            update_power_plan_status(&shared, power_plan_status, rule_context, None);
            if let Err(error) = &shutdown_result {
                update_worker_error(
                    &shared,
                    Some(format!(
                        "Background automation worker cleanup failed: {error}"
                    )),
                );
            }
            if commit_worker_exit(&shared, change_generation) {
                return shutdown_result;
            }
            drop(runner);
            runner = RuntimeCore::for_worker(&shared)?;
            scheduler = RefreshScheduler::new(Instant::now());
            cpu_allocation_reconciliation_retry_interval =
                CPU_ALLOCATION_RECONCILIATION_RETRY_INITIAL;
            continue;
        }

        let sleep_now = Instant::now();
        wait_for = wait_for.map(|delay| delay.saturating_sub(sleep_now - wait_now));
        if let Some(delay) = runner.control_release_retry_delay(sleep_now) {
            wait_for = Some(min_worker_wait(wait_for, delay));
        }
        if process_priority_schedule.source_recheck
            || thread_priority_schedule.source_recheck
            || dynamic_priority_boost_schedule.source_recheck
            || gpu_priority_schedule.source_recheck
            || background_efficiency_waits_for_source
            || timer_resolution_waits_for_source
            || (!timer_resolution_active
                && !windows_event_watcher_active
                && timer_resolution_enablement_varies_by_source(&configured_settings))
            || (!windows_event_watcher_active
                && ((!background_efficiency_active && background_efficiency_policy_varies)
                    || adaptive_enablement_varies_by_source(&configured_settings)
                    || memory_trim_policy_difference != MemoryTrimPolicyDifference::Common
                    || cpu_limiter_policy_varies
                    || app_suspension_policy_varies
                    || (!power_plan_checks_required
                        && ordinary_power_plan_policy_varies_by_source(&configured_settings))))
        {
            wait_for = Some(min_worker_wait(wait_for, POWER_SOURCE_RECHECK_INTERVAL));
        }

        if wait_for_wake(&shared, wait_for, change_generation) {
            break;
        }
    }

    runner.shutdown()
}

fn watchdog_action_is_current(shared: &SharedAutomationState, expected: &Settings) -> bool {
    let state = lock_unpoisoned(&shared.state);
    !state.stop_requested
        && !state.pending_events.suspend_resume
        && !state.pending_events.session_changed
        && state.settings.general.enabled
        && state.settings.watchdog.enabled
        && state.settings.watchdog == expected.watchdog
        && state.settings.general.allow_cross_session_process_control
            == expected.general.allow_cross_session_process_control
}

fn reserve_keep_running_commit(
    shared: &SharedAutomationState,
    expected_settings: &Settings,
    expected_watcher_active: bool,
    manager: &mut KeepRunningManager,
    ticket: &LaunchTicket,
    now: Instant,
    observation: Observation<'_>,
) -> CommitDisposition {
    let state = lock_unpoisoned(&shared.state);
    let current_intents_clear = expected_watcher_active
        && state.windows_event_watcher_active
        && state.pending_events.windows_event_watcher_change.is_none()
        && !state.stop_requested
        && state.settings.general.enabled
        && state.settings.keep_running.enabled
        && state.settings.general.enabled == expected_settings.general.enabled
        && state.settings.keep_running == expected_settings.keep_running
        && !state.pending_events.settings_changed
        && state.pending_keep_running_resets.is_empty()
        && !state.pending_events.suspend_resume
        && !state.pending_events.session_changed
        && !state
            .process_control_commands
            .iter()
            .any(ProcessControlCommand::is_process_stop);
    // This reservation orders accepted intents; native launch calls run after the guard is released.
    manager.try_reserve_commit(now, ticket, observation, current_intents_clear)
}

fn active_power_source(plugged_in: Option<bool>) -> PowerSourceProfile {
    if plugged_in == Some(false) {
        PowerSourceProfile::OnBattery
    } else {
        PowerSourceProfile::PluggedIn
    }
}

fn active_power_source_settings(settings: &Settings, plugged_in: Option<bool>) -> &Settings {
    match active_power_source(plugged_in) {
        PowerSourceProfile::OnBattery => settings.battery_profile(),
        PowerSourceProfile::PluggedIn => settings,
    }
}

fn next_cpu_allocation_reconciliation_retry_interval(current: Duration) -> Duration {
    current
        .saturating_mul(2)
        .min(CPU_ALLOCATION_RECONCILIATION_RETRY_MAX)
}

fn worker_exit_is_still_idle(shared: &SharedAutomationState, observed_generation: u64) -> bool {
    let state = lock_unpoisoned(&shared.state);
    state.stop_requested || state.change_generation == observed_generation
}

fn commit_worker_exit(shared: &SharedAutomationState, observed_generation: u64) -> bool {
    let mut state = lock_unpoisoned(&shared.state);
    if !state.stop_requested && state.change_generation != observed_generation {
        return false;
    }
    state.worker_accepting_work = false;
    true
}

fn automation_worker_can_exit(
    wait_for: Option<Duration>,
    automation_required: bool,
    has_managed_process_state: bool,
) -> bool {
    wait_for.is_none() && !automation_required && !has_managed_process_state
}

fn run_scheduled_power_plan_check(
    scheduler: &mut RefreshScheduler,
    now: Instant,
    check: impl FnOnce() -> Option<Duration>,
) -> Option<Duration> {
    if scheduler.is_due(RefreshDomain::PowerPlanCheck, now) {
        let delay = check()?;
        scheduler.schedule_after(RefreshDomain::PowerPlanCheck, now, delay);
    }
    scheduler.minimum_wait(
        None,
        now,
        [(true, RefreshDomain::PowerPlanCheck, Duration::MAX)],
    )
}

fn min_worker_wait(current: Option<Duration>, candidate: Duration) -> Duration {
    current.map_or(candidate, |current| current.min(candidate))
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod self_power_tests;

#[cfg(test)]
mod keep_running_intake_tests {
    use super::*;
    use crate::{
        config::KeepRunningRule,
        control::process::ProcessTargetKey,
        features::advanced_controls::keep_running::{
            AcceptanceDisposition, CleanupAction, DesktopBindingKey, PrepareDisposition,
            PreparedDisposition,
        },
    };

    fn settings() -> Settings {
        let mut settings = Settings::default();
        settings.general.enabled = true;
        settings.keep_running.enabled = true;
        settings
            .keep_running
            .rules
            .push(KeepRunningRule::new(r"C:\Apps\worker.exe".into()));
        settings
    }

    fn shared(settings: &Settings) -> SharedAutomationState {
        SharedAutomationState {
            state: Mutex::new(AutomationWorkerState {
                settings: Arc::new(settings.clone()),
                runtime_revision: SettingsRevision::initial(),
                persisted_revision: SettingsRevision::initial(),
                change_generation: 0,
                status: RuntimeStatusSnapshot::default(),
                pending_auto_exclusions: AutoExclusionPatch::default(),
                pending_auto_exclusions_revision: None,
                pending_auto_exclusions_retry_at: None,
                process_control_commands: VecDeque::new(),
                idle_action_log: Some(ActionLog::default()),
                action_log_clear_requested: false,
                pending_events: AutomationWakeEvents::default(),
                pending_keep_running_resets: Default::default(),
                pending_instance_limits_resets: instance_limits::PendingResets::default(),
                pending_instance_balancing_resets: instance_balancing::PendingResets::default(),
                pending_sleep_prevention_settings: Default::default(),
                input_activity: InputActivityTracker::default(),
                windows_event_watcher_active: true,
                worker_accepting_work: true,
                stop_requested: false,
            }),
            changed: Condvar::new(),
            status_generation: AtomicU64::new(0),
            pending_auto_exclusions_generation: AtomicU64::new(0),
            auto_exclusion_wake: Mutex::new(None),
        }
    }

    fn ready(now: Instant, context: &DesktopBindingKey) -> (KeepRunningManager, LaunchTicket) {
        let key = ProcessTargetKey::new(10, 1, Path::new(r"C:\Apps\worker.exe"));
        let mut manager = KeepRunningManager::default();
        manager.enable();
        manager.observe(
            now,
            Observation::Complete {
                observed_at: now,
                context,
                members: std::slice::from_ref(&key),
            },
        );
        manager.exited(now, &key);
        let due = now + KEEP_RUNNING_REFRESH_INTERVAL;
        let PrepareDisposition::Prepare(ticket) = manager.try_prepare(
            due,
            Observation::Complete {
                observed_at: due,
                context,
                members: &[],
            },
        ) else {
            panic!("Expected one preparation ticket");
        };
        assert_eq!(manager.prepared(due, &ticket), PreparedDisposition::Ready);
        (manager, ticket)
    }

    #[test]
    fn pending_intents_and_changed_eligibility_block_the_shared_mutex_reservation() {
        let base = Instant::now();
        let now = base + KEEP_RUNNING_REFRESH_INTERVAL;
        let context = DesktopBindingKey::new("desktop-a".into());
        let settings = settings();
        let blockers: [fn(&mut AutomationWorkerState); 9] = [
            |state| state.stop_requested = true,
            |state| state.windows_event_watcher_active = false,
            |state| {
                state.pending_events.windows_event_watcher_change =
                    Some(WindowsEventWatcherChange::Lost)
            },
            |state| state.pending_events.settings_changed = true,
            |state| {
                let mut settings = (*state.settings).clone();
                settings.keep_running.enabled = false;
                keep_running::record_settings_changes(state, &settings);
            },
            |state| state.pending_events.suspend_resume = true,
            |state| state.pending_events.session_changed = true,
            |state| Arc::make_mut(&mut state.settings).general.enabled = false,
            |state| {
                Arc::make_mut(&mut state.settings)
                    .keep_running
                    .rules
                    .clear()
            },
        ];
        for block in blockers {
            let shared = shared(&settings);
            block(&mut lock_unpoisoned(&shared.state));
            let (mut manager, ticket) = ready(base, &context);
            assert_eq!(
                reserve_keep_running_commit(
                    &shared,
                    &settings,
                    true,
                    &mut manager,
                    &ticket,
                    now,
                    Observation::Complete {
                        observed_at: now,
                        context: &context,
                        members: &[],
                    },
                ),
                CommitDisposition::Deferred
            );
        }
    }

    #[test]
    fn stop_intake_orders_before_reservation_and_the_mutex_is_released_before_launch() {
        let base = Instant::now();
        let now = base + KEEP_RUNNING_REFRESH_INTERVAL;
        let context = DesktopBindingKey::new("desktop-a".into());
        let settings = settings();
        let shared = shared(&settings);
        let (mut manager, ticket) = ready(base, &context);
        let (result, _receiver) = sync_channel(1);
        let stops = [
            ProcessControlCommand::StopProcesses {
                targets: vec![],
                result,
            },
            ProcessControlCommand::StopTreeIntent { roots: vec![] },
        ];
        let observation = Observation::Complete {
            observed_at: now,
            context: &context,
            members: &[],
        };
        for stop in stops {
            lock_unpoisoned(&shared.state)
                .process_control_commands
                .push_back(stop);
            assert_eq!(
                reserve_keep_running_commit(
                    &shared,
                    &settings,
                    true,
                    &mut manager,
                    &ticket,
                    now,
                    observation,
                ),
                CommitDisposition::Deferred
            );
            lock_unpoisoned(&shared.state)
                .process_control_commands
                .clear();
        }
        assert_eq!(
            reserve_keep_running_commit(
                &shared,
                &settings,
                true,
                &mut manager,
                &ticket,
                now,
                observation,
            ),
            CommitDisposition::Reserved
        );
        // Native execution can reacquire intake; the reservation helper returned its guard.
        let mut intake = shared
            .state
            .try_lock()
            .expect("reservation released intake");
        let (result, _receiver) = sync_channel(1);
        intake
            .process_control_commands
            .push_back(ProcessControlCommand::StopProcesses {
                targets: vec![],
                result,
            });
        drop(intake);
        assert_eq!(
            manager.pause(PauseReason::IntentionalStop),
            CleanupAction::None
        );
        assert_eq!(
            manager.accepted(
                &ticket,
                ProcessTargetKey::new(20, 2, Path::new(r"C:\Apps\worker.exe")),
            ),
            AcceptanceDisposition::Accepted
        );
    }

    #[test]
    fn keep_running_owns_global_worker_and_session_events_without_power_rules() {
        let settings = settings();
        assert!(automation_worker_required(&settings));
        assert!(windows_event_watcher_required(&settings));
        for event in [
            WindowsAutomationEvent::SuspendResume,
            WindowsAutomationEvent::SessionChanged,
        ] {
            assert!(windows_event_wake_required(&settings, event));
        }
        assert!(!power_plan_checks_required(&settings));
        assert_eq!(input_hook_config(&settings), InputHookConfig::default());
        let now = Instant::now();
        let mut scheduler = RefreshScheduler::new(now);
        scheduler.schedule_after(
            RefreshDomain::KeepRunning,
            now,
            KEEP_RUNNING_REFRESH_INTERVAL,
        );
        assert_eq!(
            scheduler.minimum_wait(
                None,
                now,
                [(
                    true,
                    RefreshDomain::KeepRunning,
                    KEEP_RUNNING_REFRESH_INTERVAL
                )],
            ),
            Some(Duration::from_secs(1))
        );
    }
}
