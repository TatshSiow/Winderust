use std::{
    collections::{BTreeSet, VecDeque},
    fmt,
    path::Path,
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc::{sync_channel, Receiver, SyncSender},
        Arc, Condvar, Mutex, MutexGuard,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use crate::{
    action_log::{ActionLog, ActionLogEntry, ActionLogSummaries},
    activity::{
        input_tracker, merge_activity_snapshot, ControllerActivityDetector, IdleDetector,
        InputHook, InputHookConfig, InputHookEvents, CONTROLLER_ACTIVITY_POLL_INTERVAL,
    },
    app_suspension::{AppSuspensionManager, AppSuspensionSnapshot},
    application::settings::{AutoExclusionPatch, RuntimeSettingsSnapshot, SettingsRevision},
    background_efficiency::{BackgroundEfficiencyManager, BackgroundEfficiencySnapshot},
    bottleneck_classifier::{BottleneckClassifier, BottleneckSnapshot},
    config::{
        AccentColorSource, AnimationMode, AppThemeMode, CpuAllocationSettings, PowerPlanSettings,
        ProcessGpuPriority, ProcessIoPriority, ProcessMemoryPriority, ProcessPrioritySetting,
        ProcessThreadPrioritySetting, Settings, CHECK_INTERVAL_MAX_MS, CHECK_INTERVAL_MIN_MS,
    },
    control::{
        cpu_allocation::CpuAllocationCoordinator,
        dynamic_priority_boost::{DynamicPriorityBoostController, DynamicPriorityBoostState},
        gpu_priority::GpuPriorityController,
        io_priority::IoPriorityController,
        memory_priority::MemoryPriorityController,
        memory_trim::MemoryTrimController,
        power_plan::{AdaptivePowerPlanRequest, PowerPlanController, PowerPlanStatus},
        priority_efficiency::PriorityEfficiencyController,
        process::ControlOwner,
        process_termination::{ProcessTerminationBatchResult, ProcessTerminationController},
        suspension::SuspensionController,
        thread_priority::ThreadPriorityController,
        timer_resolution::TimerResolutionController,
    },
    core_limiter::{CoreLimiterManager, CoreLimiterSnapshot},
    cpu::{CpuUsageMonitor, CpuUsageSnapshot, PerProcessorUsageMonitor},
    cpu_allocation::{
        self, record_cpu_allocation_reconciliation, CpuAllocationManager, CpuAllocationSnapshot,
        LogicalProcessorInfo, LogicalProcessorKind,
    },
    cpu_scheduler::{CpuSchedulerManager, CpuSchedulerSnapshot, CpuSchedulerUpdate},
    dashboard_metrics::{IoUsageMonitor, IoUsageSnapshot},
    dynamic_priority_boost::{DynamicPriorityBoostManager, DynamicPriorityBoostSnapshot},
    features::power_plan_control::by_running_app::{ByRunningAppManager, ByRunningAppSnapshot},
    features::power_plan_control::{
        current_by_time_decision, next_by_time_change_delay, ByCpuLoadScheduler,
    },
    foreground::{
        cursor_is_shell_window, cursor_process, cursor_process_id, executable_path_key,
        process_is_critical, same_executable_path, shell_window_mouse_pressed, ProcessActionTarget,
        ProcessActionTargetError,
    },
    gpu_priority::{GpuPriorityManager, GpuPrioritySnapshot},
    io_priority::{IoPriorityManager, IoPrioritySnapshot},
    memory_priority::{MemoryPriorityManager, MemoryPrioritySnapshot},
    memory_trim::{MemoryTrimManager, MemoryTrimSnapshot},
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
    self_power::SelfPowerController,
    thread_priority::{ThreadPriorityManager, ThreadPrioritySnapshot},
    timer_resolution::{TimerResolutionManager, TimerResolutionSnapshot},
    tray::{self, TrayVisibilityWatcher},
    windows_events::{WindowsAutomationEvent, WindowsEventWatcher},
};

mod requirements;
mod runner;
mod status;
mod wake;

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
const MEMORY_TRIM_REFRESH_INTERVAL: Duration = Duration::from_secs(15 * 60);
const TIMER_RESOLUTION_REFRESH_INTERVAL: Duration = Duration::from_secs(3);
const PROCESS_APPEARANCE_SCAN_INTERVAL: Duration = Duration::from_secs(1);
const HIDDEN_AUTOMATION_REFRESH_INTERVAL: Duration = Duration::from_secs(10);
const ADAPTIVE_ENGINE_AUTOMATION_REFRESH_INTERVAL: Duration = Duration::from_secs(60);
const VISIBLE_AUTOMATION_REFRESH_INTERVAL: Duration = Duration::from_secs(3);
const SCHEDULE_RULE_MAX_SLEEP: Duration = Duration::from_secs(60 * 60);
const AUTO_EXCLUSION_PATCH_RETRY_INTERVAL: Duration = Duration::from_secs(5);
const PROCESS_CONTROL_COMMAND_QUEUE_CAPACITY: usize = 32;

pub struct RuntimeHandle {
    shared: Arc<SharedAutomationState>,
    lifecycle: Mutex<()>,
    thread: Mutex<Option<JoinHandle<Result<(), String>>>>,
    event_watcher: Mutex<Option<WindowsEventWatcher>>,
    input_hook: Mutex<Option<InputHook>>,
    self_power: Arc<Mutex<SelfPowerController>>,
    tray_visibility_watcher: Mutex<Option<TrayVisibilityWatcher>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RuntimeCommandError {
    RuntimeStopped,
    WorkerExited,
    QueueFull,
    InvalidRequest(String),
    CommandFailed(String),
}

impl fmt::Display for RuntimeCommandError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RuntimeStopped => formatter.write_str("The automation runtime has stopped."),
            Self::WorkerExited => {
                formatter.write_str("The automation worker exited before processing the request.")
            }
            Self::QueueFull => formatter.write_str(
                "Too many process control requests are already waiting. Try again shortly.",
            ),
            Self::InvalidRequest(message) | Self::CommandFailed(message) => {
                formatter.write_str(message)
            }
        }
    }
}

impl std::error::Error for RuntimeCommandError {}

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
        let target_count = self.results.len();
        let failures = self
            .results
            .into_iter()
            .filter_map(Result::err)
            .collect::<Vec<_>>();
        if failures.is_empty() && target_count > 0 {
            Ok(())
        } else {
            Err(format!(
                "{} of {target_count} process actions failed: {}",
                failures.len(),
                failures
                    .first()
                    .cloned()
                    .unwrap_or_else(|| "No process targets were available.".to_owned())
            ))
        }
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
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuntimeFeatureStatus {
    pub bottleneck_classifier: BottleneckSnapshot,
    pub background_efficiency: BackgroundEfficiencySnapshot,
    pub app_suspension: AppSuspensionSnapshot,
    pub cpu_sets_soft: CpuAllocationSnapshot,
    pub processor_affinity_hard: CpuAllocationSnapshot,
    pub core_limiter: CoreLimiterSnapshot,
    pub by_running_app: ByRunningAppSnapshot,
    pub cpu_scheduler: CpuSchedulerSnapshot,
    pub process_priority: ProcessPrioritySnapshot,
    pub thread_priority: ThreadPrioritySnapshot,
    pub dynamic_priority_boost: DynamicPriorityBoostSnapshot,
    pub io_priority: IoPrioritySnapshot,
    pub gpu_priority: GpuPrioritySnapshot,
    pub memory_priority: MemoryPrioritySnapshot,
    pub memory_trim: MemoryTrimSnapshot,
    pub timer_resolution: TimerResolutionSnapshot,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuntimeStatusSnapshot {
    pub generation: u64,
    pub worker_error: Option<String>,
    pub feature_status: Arc<RuntimeFeatureStatus>,
    pub(crate) power_plan_status: Arc<PowerPlanStatus>,
    pub action_log_entries: Arc<Vec<ActionLogEntry>>,
    pub action_log_summaries: Arc<ActionLogSummaries>,
    pub appearance_change_generation: u64,
}

struct SharedAutomationState {
    state: Mutex<AutomationWorkerState>,
    changed: Condvar,
    status_generation: AtomicU64,
    pending_auto_exclusions_generation: AtomicU64,
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
    action_log_clear_requested: bool,
    pending_events: AutomationWakeEvents,
    windows_event_watcher_active: bool,
    worker_accepting_work: bool,
    stop_requested: bool,
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
                action_log_clear_requested: false,
                pending_events: AutomationWakeEvents::default(),
                windows_event_watcher_active: false,
                worker_accepting_work: false,
                stop_requested: false,
            }),
            changed: Condvar::new(),
            status_generation: AtomicU64::new(1),
            pending_auto_exclusions_generation: AtomicU64::new(0),
        });
        let self_power = Arc::new(Mutex::new(SelfPowerController::default()));
        let self_power_error = {
            let mut controller = lock_unpoisoned(&self_power);
            controller
                .set_requests(
                    tray::is_hidden_to_tray(),
                    settings.value.adaptive_engine.enabled,
                )
                .err()
        };
        let visibility_self_power = Arc::clone(&self_power);
        let visibility_shared = Arc::clone(&shared);
        let tray_visibility_watcher = TrayVisibilityWatcher::start(Arc::new(move |hidden| {
            if let Err(error) = lock_unpoisoned(&visibility_self_power).set_hidden_mode(hidden) {
                update_worker_error(&visibility_shared, Some(error));
            }
        }));
        let automation = Self {
            shared,
            lifecycle: Mutex::new(()),
            thread: Mutex::new(None),
            event_watcher: Mutex::new(None),
            input_hook: Mutex::new(None),
            self_power,
            tray_visibility_watcher: Mutex::new(Some(tray_visibility_watcher)),
        };
        if let Some(error) = self_power_error {
            update_worker_error(&automation.shared, Some(error));
        }
        automation.sync_worker(settings, false);
        automation.sync_windows_event_watcher(settings);
        automation.sync_input_hook(settings);
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
                state.settings = Arc::clone(&settings.value);
                state.runtime_revision = settings.runtime_revision;
                state.pending_events.settings_changed = true;
                state.change_generation = state.change_generation.wrapping_add(1);
                self.shared.changed.notify_one();
                true
            }
        };

        if settings_changed {
            self.sync_worker(settings, false);
            self.sync_windows_event_watcher(settings);
            self.sync_input_hook(settings);
        }
        self.sync_self_power(settings);
    }

    pub fn shutdown(&self) -> Result<(), String> {
        let _lifecycle = lock_unpoisoned(&self.lifecycle);
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
        if let Err(error) = lock_unpoisoned(&self.self_power).shutdown() {
            errors.push(format!("Winderust self-power restoration failed: {error}"));
        }
        if errors.is_empty() {
            Ok(())
        } else {
            let error = errors.join(" ");
            update_worker_error(&self.shared, Some(error.clone()));
            Err(error)
        }
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
        snapshot.worker_error = state.status.worker_error.take();
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
        state.action_log_clear_requested = true;
        state.change_generation = state.change_generation.wrapping_add(1);
        self.shared.changed.notify_one();
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
        let executable_path = executable_path_key(Path::new(executable_path));
        if !Path::new(&executable_path).is_absolute() {
            return Err(RuntimeCommandError::InvalidRequest(
                "App Suspension requires an absolute executable path.".to_owned(),
            ));
        }
        let (result, receiver) = sync_channel(1);
        self.enqueue_process_control_command(ProcessControlCommand::AppSuspensionPathAction {
            executable_path,
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

    pub(crate) fn request_memory_trim_now(
        &self,
    ) -> Result<MemoryTrimActionReceiver, RuntimeCommandError> {
        let (result, receiver) = sync_channel(1);
        self.enqueue_process_control_command(ProcessControlCommand::MemoryTrim { result })?;
        Ok(receiver)
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
                run_background_automation(thread_shared)
            }));
        }
    }

    fn sync_windows_event_watcher(&self, settings: &RuntimeSettingsSnapshot) {
        let _lifecycle = lock_unpoisoned(&self.lifecycle);
        if lock_unpoisoned(&self.shared.state).stop_requested {
            return;
        }
        let mut watcher = lock_unpoisoned(&self.event_watcher);

        if windows_event_watcher_required(&settings.value) {
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

    fn sync_input_hook(&self, settings: &RuntimeSettingsSnapshot) {
        let _lifecycle = lock_unpoisoned(&self.lifecycle);
        if lock_unpoisoned(&self.shared.state).stop_requested {
            return;
        }
        let mut input_hook = lock_unpoisoned(&self.input_hook);
        if !input_hook_required(&settings.value) {
            input_hook.take();
            return;
        }

        let config = input_hook_config(&settings.value);
        if input_hook
            .as_ref()
            .is_some_and(|input_hook| input_hook.config() == config)
        {
            return;
        }
        input_hook.take();
        let shared = Arc::clone(&self.shared);
        match InputHook::install(
            config,
            Arc::new(move |events| notify_input_event(&shared, events)),
        ) {
            Ok(installed) => *input_hook = Some(installed),
            Err(error) => update_worker_error(&self.shared, Some(error)),
        }
    }

    fn sync_self_power(&self, settings: &RuntimeSettingsSnapshot) {
        if lock_unpoisoned(&self.shared.state).stop_requested {
            return;
        }
        if let Err(error) = lock_unpoisoned(&self.self_power).set_adaptive_engine(
            settings.value.adaptive_engine.enabled
                || settings
                    .value
                    .on_battery
                    .as_deref()
                    .is_some_and(|settings| settings.adaptive_engine.enabled),
        ) {
            update_worker_error(&self.shared, Some(error));
        }
    }
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
        if exited_while_accepting_work {
            for command in state.process_control_commands.drain(..) {
                command.reject(RuntimeCommandError::WorkerExited);
            }
        }
        self.shared.changed.notify_all();
    }
}

fn run_background_automation(shared: Arc<SharedAutomationState>) -> Result<(), String> {
    let _exit_guard = AutomationWorkerExitGuard { shared: &shared };
    let mut runner = RuntimeCore::default();
    let mut scheduler = RefreshScheduler::new(Instant::now());
    let mut cpu_allocation_reconciliation_retry_interval =
        CPU_ALLOCATION_RECONCILIATION_RETRY_INITIAL;

    while let Some(snapshot) = automation_snapshot(&shared) {
        let configured_settings = snapshot.settings;
        let settings = active_power_source_settings(
            configured_settings.as_ref(),
            power_source::is_plugged_in(),
        );
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
        if snapshot.action_log_clear_requested {
            runner.action_log.clear();
        }
        let wake_events = snapshot.wake_events;
        let windows_event_watcher_active = snapshot.windows_event_watcher_active;
        let mut observations = CycleObservations::default();
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
        let cpu_scheduler_refresh_interval = cpu_scheduler_refresh_interval(settings);
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
        let settings_changed = wake_events.settings_changed || runner.note_settings(settings);
        if settings_changed {
            scheduler.invalidate(SchedulerEvent::SettingsChanged, event_now);
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
        if wake_events.session_changed {
            scheduler.invalidate(SchedulerEvent::SessionChanged, event_now);
        }
        if wake_events.power_changed || wake_events.session_changed {
            if let Err(error) = runner.refresh_active_plan() {
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
            if runner
                .app_suspension_manager
                .has_suspended_processes(&runner.app_suspension_controller)
            {
                let app_suspension_status = if wake_events.app_switch {
                    runner.run_app_suspension_app_switch_release(&mut observations)
                } else {
                    runner.run_app_suspension_shell_click_release()
                };
                if let Some(app_suspension_status) = app_suspension_status {
                    update_app_suspension_status(&shared, app_suspension_status);
                }
            }
        }
        let now = Instant::now();
        let power_plan_checks_required = power_plan_checks_required(settings);
        let scan_process_appearance = process_appearance_scan_required(settings);
        let background_efficiency_refresh_required = settings_changed
            || feature_refresh_required(settings, settings.background_efficiency.enabled);
        let app_suspension_refresh_required = settings_changed
            || feature_refresh_required(settings, app_suspension_required(settings))
            || app_suspension_command_requested
            || runner
                .app_suspension_manager
                .has_suspended_processes(&runner.app_suspension_controller);
        let cpu_sets_soft_refresh_required = settings_changed
            || feature_refresh_required(settings, cpu_sets_soft_required(settings));
        let processor_affinity_hard_refresh_required = settings_changed
            || feature_refresh_required(settings, processor_affinity_hard_required(settings));
        let core_limiter_refresh_required =
            settings_changed || feature_refresh_required(settings, core_limiter_required(settings));
        let by_running_app_refresh_required = settings_changed
            || feature_refresh_required(settings, by_running_app_required(settings));
        let cpu_scheduler_refresh_required = settings_changed
            || feature_refresh_required(settings, cpu_scheduler_required(settings));
        let bottleneck_classifier_refresh_required = settings_changed
            || feature_refresh_required(settings, bottleneck_classifier_required(settings));
        let adaptive_power_plan_refresh_required = settings_changed
            || feature_refresh_required(settings, adaptive_power_plan_required(settings))
            || runner.adaptive_power_plan_active();
        let process_priority_refresh_required = settings_changed
            || feature_refresh_required(settings, settings.process_priority.enabled);
        let thread_priority_refresh_required = settings_changed
            || feature_refresh_required(settings, thread_priority_required(settings));
        let dynamic_priority_boost_refresh_required = settings_changed
            || feature_refresh_required(settings, dynamic_priority_boost_required(settings));
        let io_priority_refresh_required =
            settings_changed || feature_refresh_required(settings, io_priority_required(settings));
        let gpu_priority_refresh_required =
            settings_changed || feature_refresh_required(settings, gpu_priority_required(settings));
        let memory_priority_refresh_required = settings_changed
            || feature_refresh_required(settings, settings.memory_priority.enabled);
        let memory_trim_refresh_required = settings_changed
            || memory_trim_command_requested
            || feature_refresh_required(settings, settings.memory_trim.enabled);
        let timer_resolution_refresh_required = settings_changed
            || feature_refresh_required(settings, timer_resolution_required(settings));
        if app_suspension_command_requested {
            scheduler.invalidate(SchedulerEvent::AppSuspensionRequested, now);
        }
        if memory_trim_command_requested {
            scheduler.invalidate(SchedulerEvent::MemoryTrimRequested, now);
        }

        if scan_process_appearance && scheduler.is_due(RefreshDomain::ProcessAppearance, now) {
            if runner.detect_process_appearance(&mut observations) {
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

        if runner
            .app_suspension_manager
            .has_suspended_processes(&runner.app_suspension_controller)
            && scheduler.is_due(RefreshDomain::AppSuspensionForegroundRelease, now)
        {
            if let Some(app_suspension_status) =
                runner.run_app_suspension_foreground_release(&mut observations)
            {
                update_app_suspension_status(&shared, app_suspension_status);
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
            let background_efficiency_status =
                runner.run_background_efficiency_update(settings, &mut observations);
            update_background_efficiency_status(&shared, background_efficiency_status);
            scheduler.schedule_after(
                RefreshDomain::BackgroundEfficiency,
                now,
                background_efficiency_refresh_interval,
            );
        }
        if cpu_scheduler_refresh_required && scheduler.is_due(RefreshDomain::CpuScheduler, now) {
            let cpu_scheduler_status = runner.run_cpu_scheduler_update(settings, &mut observations);
            update_cpu_scheduler_status(&shared, cpu_scheduler_status);
            scheduler.schedule_after(
                RefreshDomain::CpuScheduler,
                now,
                cpu_scheduler_refresh_interval,
            );
        }
        if bottleneck_classifier_refresh_required
            && scheduler.is_due(RefreshDomain::BottleneckClassifier, now)
        {
            let status = runner.run_bottleneck_classifier_update(settings);
            update_bottleneck_classifier_status(&shared, status);
            scheduler.schedule_after(
                RefreshDomain::BottleneckClassifier,
                now,
                BOTTLENECK_CLASSIFIER_REFRESH_INTERVAL,
            );
        }
        if adaptive_power_plan_refresh_required
            && scheduler.is_due(RefreshDomain::AdaptivePowerPlan, now)
        {
            if let Err(error) = runner.run_adaptive_power_plan_update(settings, &mut observations) {
                update_worker_error(&shared, Some(error));
            }
            scheduler.schedule_after(
                RefreshDomain::AdaptivePowerPlan,
                now,
                ADAPTIVE_POWER_PLAN_REFRESH_INTERVAL,
            );
        }
        if io_priority_refresh_required && scheduler.is_due(RefreshDomain::IoPriority, now) {
            let io_priority_status = runner.run_io_priority_update(settings, &mut observations);
            update_io_priority_status(&shared, io_priority_status);
            scheduler.schedule_after(RefreshDomain::IoPriority, now, io_priority_refresh_interval);
        }
        if process_priority_refresh_required
            && scheduler.is_due(RefreshDomain::ProcessPriority, now)
        {
            let process_priority_status =
                runner.run_process_priority_update(settings, &mut observations);
            update_process_priority_status(&shared, process_priority_status);
            scheduler.schedule_after(
                RefreshDomain::ProcessPriority,
                now,
                process_priority_refresh_interval,
            );
        }
        if thread_priority_refresh_required && scheduler.is_due(RefreshDomain::ThreadPriority, now)
        {
            let thread_priority_status =
                runner.run_thread_priority_update(settings, &mut observations);
            update_thread_priority_status(&shared, thread_priority_status);
            scheduler.schedule_after(
                RefreshDomain::ThreadPriority,
                now,
                thread_priority_refresh_interval,
            );
        }
        if dynamic_priority_boost_refresh_required
            && scheduler.is_due(RefreshDomain::DynamicPriorityBoost, now)
        {
            let dynamic_priority_boost_status =
                runner.run_dynamic_priority_boost_update(settings, &mut observations);
            update_dynamic_priority_boost_status(&shared, dynamic_priority_boost_status);
            scheduler.schedule_after(
                RefreshDomain::DynamicPriorityBoost,
                now,
                dynamic_priority_boost_refresh_interval,
            );
        }
        if gpu_priority_refresh_required && scheduler.is_due(RefreshDomain::GpuPriority, now) {
            let gpu_priority_status = runner.run_gpu_priority_update(settings, &mut observations);
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
                process_control_commands,
                &mut observations,
            )
        };
        let manual_memory_trim_processed = command_statuses.memory_trim.is_some();
        if let Some(status) = command_statuses.memory_trim {
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
            let app_suspension_status =
                runner.run_app_suspension_update(settings, &[], &mut observations);
            update_app_suspension_status(&shared, app_suspension_status);
            scheduler.schedule_after(
                RefreshDomain::AppSuspension,
                now,
                app_suspension_refresh_interval,
            );
            if runner
                .app_suspension_manager
                .has_suspended_processes(&runner.app_suspension_controller)
            {
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
        if core_limiter_refresh_required && scheduler.is_due(RefreshDomain::CoreLimiter, now) {
            let core_limiter_status = runner.run_core_limiter_update(settings, &mut observations);
            update_core_limiter_status(&shared, core_limiter_status);
            scheduler.schedule_after(
                RefreshDomain::CoreLimiter,
                now,
                core_limiter_refresh_interval,
            );
        }
        let immediate_cpu_allocation_reconciliation =
            runner.cpu_allocation_immediate_reconciliation_pending();
        let cpu_allocation_release_retry_due = cpu_allocation_release_retry_pending_at_pass_start
            && scheduler.is_due(RefreshDomain::CpuAllocationReconciliation, now);
        if immediate_cpu_allocation_reconciliation || cpu_allocation_release_retry_due {
            runner.run_cpu_allocation_reconciliation(settings, cpu_allocation_release_retry_due);
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
            let by_running_app_status =
                runner.run_by_running_app_update(settings, &mut observations);
            update_by_running_app_status(&shared, by_running_app_status);
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
            let memory_trim_status = runner.run_memory_trim_update(settings, &mut observations);
            update_memory_trim_status(&shared, memory_trim_status);
            scheduler.schedule_after(RefreshDomain::MemoryTrim, now, memory_trim_refresh_interval);
        }
        if timer_resolution_refresh_required
            && scheduler.is_due(RefreshDomain::TimerResolution, now)
        {
            let timer_resolution_status =
                runner.run_timer_resolution_update(settings, &mut observations);
            update_timer_resolution_status(&shared, timer_resolution_status);
            scheduler.schedule_after(
                RefreshDomain::TimerResolution,
                now,
                timer_resolution_refresh_interval,
            );
        }

        let wait_now = Instant::now();
        let mut wait_for = if power_plan_checks_required {
            if scheduler.is_due(RefreshDomain::PowerPlanCheck, wait_now) {
                if let Err(error) = runner.run_check(settings, &mut observations) {
                    update_worker_error(&shared, Some(error));
                }
            }

            if let Some(delay) = power_plan_check_delay(settings, windows_event_watcher_active) {
                scheduler.schedule_after(RefreshDomain::PowerPlanCheck, wait_now, delay);
                Some(delay)
            } else {
                scheduler.schedule_now(RefreshDomain::PowerPlanCheck, wait_now);
                None
            }
        } else {
            scheduler.schedule_now(RefreshDomain::PowerPlanCheck, wait_now);
            None
        };
        runner.publish_action_log_if_changed(&shared);
        update_power_plan_status(&shared, runner.power_plan_status());

        wait_for = scheduler.minimum_wait(
            wait_for,
            wait_now,
            [
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
                    core_limiter_refresh_required,
                    RefreshDomain::CoreLimiter,
                    core_limiter_refresh_interval,
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
                    cpu_scheduler_refresh_required,
                    RefreshDomain::CpuScheduler,
                    cpu_scheduler_refresh_interval,
                ),
                (
                    bottleneck_classifier_refresh_required,
                    RefreshDomain::BottleneckClassifier,
                    BOTTLENECK_CLASSIFIER_REFRESH_INTERVAL,
                ),
                (
                    adaptive_power_plan_refresh_required,
                    RefreshDomain::AdaptivePowerPlan,
                    ADAPTIVE_POWER_PLAN_REFRESH_INTERVAL,
                ),
                (
                    process_priority_refresh_required,
                    RefreshDomain::ProcessPriority,
                    process_priority_refresh_interval,
                ),
                (
                    thread_priority_refresh_required,
                    RefreshDomain::ThreadPriority,
                    thread_priority_refresh_interval,
                ),
                (
                    dynamic_priority_boost_refresh_required,
                    RefreshDomain::DynamicPriorityBoost,
                    dynamic_priority_boost_refresh_interval,
                ),
                (
                    io_priority_refresh_required,
                    RefreshDomain::IoPriority,
                    io_priority_refresh_interval,
                ),
                (
                    gpu_priority_refresh_required,
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
                    runner
                        .app_suspension_manager
                        .has_suspended_processes(&runner.app_suspension_controller),
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
            automation_worker_required(settings),
            runner.has_managed_process_control_state(),
        ) {
            if !worker_exit_is_still_idle(&shared, change_generation) {
                continue;
            }
            let shutdown_result = runner.shutdown();
            if commit_worker_exit(&shared, change_generation) {
                return shutdown_result;
            }
            if let Err(error) = shutdown_result {
                update_worker_error(
                    &shared,
                    Some(format!(
                        "Background automation worker handoff cleanup failed: {error}"
                    )),
                );
            }
            runner = RuntimeCore::default();
            scheduler = RefreshScheduler::new(Instant::now());
            cpu_allocation_reconciliation_retry_interval =
                CPU_ALLOCATION_RECONCILIATION_RETRY_INITIAL;
            continue;
        }

        if wait_for_wake(&shared, wait_for, change_generation) {
            break;
        }
    }

    runner.shutdown()
}

fn active_power_source_settings(settings: &Settings, plugged_in: Option<bool>) -> &Settings {
    if plugged_in == Some(false) {
        settings.battery_profile()
    } else {
        settings
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

fn min_worker_wait(current: Option<Duration>, candidate: Duration) -> Duration {
    current.map_or(candidate, |current| current.min(candidate))
}

#[cfg(test)]
mod tests;
