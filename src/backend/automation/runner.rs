use super::*;
use crate::features::winderust_features::memory_trim::MemoryTrimStatus;
use crate::runtime::observations::{CycleObservations, ForegroundCpuHistory};
use crate::{
    action_log::{ActionLogFeature, ActionLogResult},
    adaptive_engine_process::{focus_and_launch_profile_eligible, focus_process_priority_eligible},
    control::{memory_priority::MemoryPriorityReleaseOwner, power_plan::PowerPlanOwner},
    foreground::{for_each_process_id, process_session_id, ProcessInfo},
    platform::windows::process::current_process_id,
    rules::decision_engine::{matching_by_focus_rules, ordinary_power_plan_stop},
};
use std::{
    collections::{btree_map::Entry, BTreeSet},
    path::PathBuf,
};

fn app_suspension_lock_failure(error: String) -> AppSuspensionSnapshot {
    AppSuspensionSnapshot {
        status_unknown: true,
        status: AppSuspensionStatus::Error(error.clone()),
        last_error: Some(error),
        ..Default::default()
    }
}

pub(super) fn app_suspension_release_status(
    mut status: AppSuspensionSnapshot,
    settings: &Settings,
    waits_for_source: bool,
) -> AppSuspensionSnapshot {
    if waits_for_source {
        if !matches!(status.status, AppSuspensionStatus::Error(_)) {
            status.status = if status.unsupported {
                AppSuspensionStatus::Unsupported
            } else {
                AppSuspensionStatus::PowerSourceUnknown
            };
        }
        status.status_unknown = true;
        return status;
    }
    status.status = if !settings.general.enabled {
        AppSuspensionStatus::AutomationDisabled
    } else if !settings.app_suspension.enabled {
        AppSuspensionStatus::Disabled
    } else {
        return status;
    };
    status.enabled = false;
    status
}

fn app_suspension_status_with_retry(
    mut status: AppSuspensionSnapshot,
    controller: &SuspensionController,
) -> AppSuspensionSnapshot {
    if status.last_error.is_some() {
        return status;
    }
    if let Some(error) = controller.app_suspension_retry_error() {
        status.failed_actions = status.failed_actions.max(1);
        status.last_error = Some(error.to_owned());
    }
    status
}

pub(super) fn adaptive_power_plan_required(settings: &Settings) -> bool {
    settings.adaptive_engine.adaptive_processor_power_enabled
}

#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub(super) struct AdaptiveProcessorDemand {
    pub(super) peak_cpu_percent: Option<f32>,
    pub(super) performance_peak_cpu_percent: Option<f32>,
    pub(super) efficiency_peak_cpu_percent: Option<f32>,
}

pub(super) fn adaptive_processor_demand(
    usage: &[f32],
    processors: &[LogicalProcessorInfo],
) -> AdaptiveProcessorDemand {
    fn update_peak(peak: &mut Option<f32>, usage: f32) {
        *peak = Some(peak.map_or(usage, |current| current.max(usage)));
    }

    let mut demand = AdaptiveProcessorDemand::default();
    let hybrid = processors
        .iter()
        .any(|processor| processor.kind == LogicalProcessorKind::Performance)
        && processors
            .iter()
            .any(|processor| processor.kind == LogicalProcessorKind::Efficiency);
    if usage.len() != processors.len() {
        demand.peak_cpu_percent = usage.iter().copied().reduce(f32::max);
        return demand;
    }

    for (usage, processor) in usage.iter().copied().zip(processors) {
        match (hybrid, processor.kind) {
            (true, LogicalProcessorKind::Performance) => {
                update_peak(&mut demand.performance_peak_cpu_percent, usage);
            }
            (true, LogicalProcessorKind::Efficiency) => {
                update_peak(&mut demand.efficiency_peak_cpu_percent, usage);
            }
            _ => update_peak(&mut demand.peak_cpu_percent, usage),
        }
    }
    demand
}

struct ObservedSettings {
    configured: Arc<Settings>,
    plugged_in: Option<bool>,
}

impl ObservedSettings {
    fn effective(&self) -> &Settings {
        active_power_source_settings(&self.configured, self.plugged_in)
    }
}

#[derive(Default)]
pub(super) struct RuntimeCore {
    adaptive_processor_power_error: Option<String>,
    shutdown_started: bool,
    last_settings: Option<ObservedSettings>,
    power_plan_controller: PowerPlanController,
    power_plan_rule_context: Option<PowerPlanRuleContext>,
    cpu_usage: CpuUsageSnapshot,
    cpu_usage_sampled_at: Option<Instant>,
    cpu_monitor: CpuUsageMonitor,
    bottleneck_classifier: BottleneckClassifier,
    per_processor_cpu_monitor: PerProcessorUsageMonitor,
    io_monitor: IoUsageMonitor,
    adaptive_processor_topology: Vec<LogicalProcessorInfo>,
    adaptive_io_usage: IoUsageSnapshot,
    next_adaptive_io_refresh: Option<Instant>,
    adaptive_foreground_identity: Option<(u32, u64)>,
    adaptive_foreground_cpu_history: ForegroundCpuHistory,
    controller_activity_detector: ControllerActivityDetector,
    controller_observed_since: Option<Instant>,
    pub(super) input_activity: InputActivityTracker,
    by_cpu_load_scheduler: ByCpuLoadScheduler,
    background_efficiency_manager: BackgroundEfficiencyManager,
    pub(super) app_suspension_manager: AppSuspensionManager,
    pub(super) app_suspension_controller: Arc<Mutex<SuspensionController>>,
    last_app_suspension_shell_user_intent: Option<Instant>,
    cpu_sets_soft_manager: CpuAllocationManager,
    processor_affinity_hard_manager: CpuAllocationManager,
    pub(super) cpu_allocation_coordinator: CpuAllocationCoordinator,
    pub(super) instance_balancing: instance_balancing::Runtime,
    cpu_limiter_controller: Option<CpuLimiterController>,
    cpu_limiter_manager: CpuLimiterManager,
    pub(super) by_running_app_manager: ByRunningAppManager,
    pub(super) action_log: ActionLog,
    action_log_session: Option<Arc<SharedAutomationState>>,
    adaptive_engine_process_manager: AdaptiveEngineProcessManager,
    process_priority_manager: ProcessPriorityManager,
    priority_efficiency_controller: PriorityEfficiencyController,
    thread_priority_manager: ThreadPriorityManager,
    thread_priority_controller: ThreadPriorityController,
    dynamic_priority_boost_manager: DynamicPriorityBoostManager,
    dynamic_priority_boost_controller: DynamicPriorityBoostController,
    io_priority_manager: IoPriorityManager,
    io_priority_controller: IoPriorityController,
    gpu_priority_manager: GpuPriorityManager,
    gpu_priority_controller: GpuPriorityController,
    memory_priority_manager: MemoryPriorityManager,
    memory_priority_controller: MemoryPriorityController,
    memory_trim_manager: MemoryTrimManager,
    memory_trim_controller: MemoryTrimController,
    watchdog_manager: WatchdogManager,
    watchdog_restart_cooldowns: Vec<(PathBuf, Instant)>,
    process_termination_controller: ProcessTerminationController,
    timer_resolution_manager: TimerResolutionManager,
    timer_resolution_controller: TimerResolutionController,
    sleep_prevention_manager: SleepPreventionManager,
    sleep_prevention_controller: SleepPreventionController,
    pub(super) keep_running: keep_running::KeepRunningCoordinator,
    pub(super) instance_limits: InstanceLimitsManager,
    pub(super) known_process_ids: BTreeSet<u32>,
    published_action_log_revision: u64,
}

#[derive(Default)]
pub(super) struct ProcessControlCommandStatuses {
    pub(super) memory_trim: Option<MemoryTrimSnapshot>,
    pub(super) app_suspension: Option<AppSuspensionSnapshot>,
}

const WATCHDOG_RESTART_COOLDOWN: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WatchdogRestartOutcome {
    Applied,
    AlreadyRestarted,
    CoolingDown,
}

#[derive(Default)]
struct WatchdogActionCycle {
    termination_handled: BTreeSet<(u32, u64)>,
    restarts: std::collections::BTreeMap<Vec<(u32, u64)>, Result<WatchdogRestartOutcome, String>>,
}

impl WatchdogActionCycle {
    fn skipped_termination_reason(
        &self,
        targets: &[crate::foreground::ProcessActionTarget],
    ) -> Option<&'static str> {
        if targets.is_empty()
            || targets.iter().any(|target| {
                !self
                    .termination_handled
                    .contains(&(target.id, target.creation_time))
            })
        {
            return None;
        }
        let mut key: Vec<_> = targets
            .iter()
            .map(|target| (target.id, target.creation_time))
            .collect();
        key.sort_unstable();
        Some(match self.restarts.get(&key) {
            Some(Ok(WatchdogRestartOutcome::Applied)) => "watchdog.already_restarted",
            _ => "watchdog.termination_already_handled",
        })
    }

    fn restart_once(
        &mut self,
        targets: &[crate::foreground::ProcessActionTarget],
        cooldowns: &mut Vec<(PathBuf, Instant)>,
        now: Instant,
        restart: impl FnOnce() -> crate::control::process_restart::RestartResult,
    ) -> Result<WatchdogRestartOutcome, String> {
        let mut key: Vec<_> = targets
            .iter()
            .map(|target| (target.id, target.creation_time))
            .collect();
        key.sort_unstable();
        let entry = match self.restarts.entry(key) {
            Entry::Occupied(entry) => {
                return match entry.get() {
                    Ok(WatchdogRestartOutcome::Applied) => {
                        Ok(WatchdogRestartOutcome::AlreadyRestarted)
                    }
                    outcome => outcome.clone(),
                };
            }
            Entry::Vacant(entry) => entry,
        };
        let outcome = (|| {
            cooldowns.retain(|(_, deadline)| now < *deadline);
            if let Some(path) = targets.first().map(|target| &target.executable_path) {
                if cooldowns
                    .iter()
                    .any(|(active_path, _)| same_executable_path(active_path, path))
                {
                    return Ok(WatchdogRestartOutcome::CoolingDown);
                }
                cooldowns.push((path.clone(), now + WATCHDOG_RESTART_COOLDOWN));
            }
            let outcome = restart();
            self.termination_handled.extend(outcome.termination_handled);
            outcome.result.map(|()| WatchdogRestartOutcome::Applied)
        })();
        entry.insert(outcome).clone()
    }
}

impl RuntimeCore {
    pub(super) fn for_worker(shared: &Arc<SharedAutomationState>) -> Result<Self, String> {
        let action_log = lock_unpoisoned(&shared.state)
            .idle_action_log
            .take()
            .ok_or_else(|| "Action Log is already owned by an automation worker.".to_owned())?;
        let mut runner = Self::default();
        runner.published_action_log_revision = action_log.revision();
        runner.action_log = action_log;
        runner.action_log_session = Some(Arc::clone(shared));
        Ok(runner)
    }

    pub(super) fn run_watchdog_update(
        &mut self,
        shared: &SharedAutomationState,
        settings: &Settings,
        observations: &mut CycleObservations,
    ) -> WatchdogSnapshot {
        use crate::config::{WatchdogAction, WatchdogScope};
        use rust_i18n::t;
        let (mut snapshot, mut events) = self.watchdog_manager.update(
            &settings.watchdog,
            settings.general.enabled,
            observations,
        );
        // Prepare replacements before a simultaneous termination can stop the same app.
        events.sort_by_key(|event| event.response.action != WatchdogAction::Restart);
        let mut actions = WatchdogActionCycle::default();
        let mut notification_error = None;
        for event in events {
            let level = t!(if event.critical {
                "watchdog.critical"
            } else {
                "watchdog.warning"
            });
            let metric = t!(event.metric.label_key());
            let process_id = event.process_id;
            let value = format!("{:.1}", event.value);
            let comparison = event.comparison.symbol();
            let detail = match (event.scope, process_id) {
                (WatchdogScope::PerInstance, Some(pid)) => t!(
                    "watchdog.instance_trigger_detail",
                    level = level,
                    app = &event.name,
                    pid = pid,
                    metric = metric,
                    value = value,
                    comparison = comparison,
                    threshold = event.response.threshold
                ),
                (WatchdogScope::PerInstance, None) => {
                    snapshot.last_error =
                        Some("Watchdog per-instance event has no process ID.".into());
                    continue;
                }
                (WatchdogScope::Combined, _) => t!(
                    "watchdog.trigger_detail",
                    level = level,
                    app = &event.name,
                    count = event.process_count,
                    metric = metric,
                    value = value,
                    comparison = comparison,
                    threshold = event.response.threshold
                ),
            }
            .into_owned();
            let mut action_detail = t!("watchdog.no_action").to_string();
            if event.response.action != WatchdogAction::None {
                if !watchdog_action_is_current(shared, settings) {
                    continue;
                }
                let restarting = event.response.action == WatchdogAction::Restart;
                let result = if restarting {
                    actions
                        .restart_once(
                            &event.targets,
                            &mut self.watchdog_restart_cooldowns,
                            Instant::now(),
                            || {
                                self.keep_running
                                    .pause_targets(&event.targets, PauseReason::Watchdog);
                                crate::control::process_restart::restart_app(
                                    event.targets.clone(),
                                    settings.general.allow_cross_session_process_control,
                                )
                            },
                        )
                        .map(|outcome| match outcome {
                            WatchdogRestartOutcome::Applied => (
                                ActionLogResult::Applied,
                                t!("watchdog.restarted", count = event.targets.len()),
                            ),
                            WatchdogRestartOutcome::AlreadyRestarted => (
                                ActionLogResult::Skipped,
                                t!("watchdog.restart_already_restarted"),
                            ),
                            WatchdogRestartOutcome::CoolingDown => (
                                ActionLogResult::Skipped,
                                t!(
                                    "watchdog.restart_cooldown",
                                    seconds = WATCHDOG_RESTART_COOLDOWN.as_secs()
                                ),
                            ),
                        })
                } else if let Some(reason) = actions.skipped_termination_reason(&event.targets) {
                    Ok((ActionLogResult::Skipped, t!(reason)))
                } else {
                    self.keep_running
                        .pause_targets(&event.targets, PauseReason::Watchdog);
                    let targets = event
                        .targets
                        .iter()
                        .filter(|target| {
                            !actions
                                .termination_handled
                                .contains(&(target.id, target.creation_time))
                        })
                        .cloned()
                        .collect();
                    let batch = self.process_termination_controller.terminate_batch(
                        targets,
                        settings.general.allow_cross_session_process_control,
                    );
                    for result in &batch.results {
                        if result.outcome == crate::control::process_termination::ProcessTerminationOutcome::Requested {
                            actions.termination_handled.insert((result.target.id, result.target.creation_time));
                        }
                    }
                    batch.into_termination_result().map(|count| {
                        (
                            ActionLogResult::Applied,
                            t!("watchdog.termination_requested", count = count),
                        )
                    })
                };
                let outcome = match result {
                    Ok((outcome, message)) => {
                        action_detail = message.into_owned();
                        outcome
                    }
                    Err(error) => {
                        snapshot.last_error = Some(error.clone());
                        action_detail = t!(
                            if restarting {
                                "watchdog.restart_failed"
                            } else {
                                "watchdog.termination_failed"
                            },
                            error = error
                        )
                        .to_string();
                        ActionLogResult::Failed
                    }
                };
                self.action_log.record(
                    ActionLogFeature::Watchdog,
                    process_id,
                    &event.name,
                    outcome,
                    format!("{detail}. {action_detail}"),
                );
            }
            if event.notify {
                let result = tray::show_notification(
                    &format!("Winderust Watchdog · {level}"),
                    &t!(
                        "watchdog.notification_body",
                        action = action_detail,
                        detail = detail
                    ),
                    event.critical,
                );
                let (outcome, reason) = match result {
                    Ok(()) => (
                        ActionLogResult::Applied,
                        format!("{detail}. {}", t!("watchdog.notification_sent")),
                    ),
                    Err(error) => {
                        let error = error.localized();
                        notification_error = Some(error.clone());
                        (ActionLogResult::Failed, format!("{detail}. {error}"))
                    }
                };
                self.action_log.record(
                    ActionLogFeature::Watchdog,
                    process_id,
                    &event.name,
                    outcome,
                    reason,
                );
            }
        }
        snapshot.last_error = snapshot.last_error.or(notification_error);
        snapshot
    }

    fn with_app_suspension<R>(
        &mut self,
        apply: impl FnOnce(&mut AppSuspensionManager, &mut SuspensionController, &mut ActionLog) -> R,
    ) -> Result<R, String> {
        let suspension = Arc::clone(&self.app_suspension_controller);
        let mut controller = suspension
            .lock()
            .map_err(|_| "Shared suspension state is unavailable.".to_owned())?;
        Ok(apply(
            &mut self.app_suspension_manager,
            &mut controller,
            &mut self.action_log,
        ))
    }

    pub(super) fn shutdown(&mut self) -> Result<(), String> {
        if self.shutdown_started {
            return Ok(());
        }
        self.shutdown_started = true;
        self.keep_running.shutdown();

        let mut errors = Vec::<String>::new();
        let mut settings = self
            .last_settings
            .as_ref()
            .map(ObservedSettings::effective)
            .cloned()
            .unwrap_or_default();
        settings.general.enabled = false;
        let mut observations = CycleObservations::default();

        // Restore in the reverse order used by the automation loop. Several features can touch
        // the same process state, so relying on field drop order can restore an intermediate
        // Winderust-managed value instead of the value that preceded Winderust.
        // Priority, allocation, timer, and limiter controllers retry state left by the disabled
        // feature update. Use the final shutdown result so a transient error does not report
        // failure after that retry restored everything.
        collect_restore_error(
            &mut errors,
            "Sleep Prevention",
            self.sleep_prevention_manager
                .reset(&mut self.sleep_prevention_controller, &mut self.action_log)
                .last_error,
        );
        self.run_timer_resolution_update(&settings, &mut observations);
        if let Err(error) = self.timer_resolution_controller.shutdown() {
            errors.push(format!("Timer Resolution restoration failed: {error}"));
        }
        let _ = self.run_by_running_app_update(&settings, &mut observations);
        self.run_cpu_limiter_update(&settings, false, &mut observations);
        if let Some(mut controller) = self.cpu_limiter_controller.take() {
            if let Err(error) = controller.shutdown() {
                errors.push(format!("CPU Limiter restoration failed: {error}"));
            }
        }
        self.run_processor_affinity_hard_update(&settings, &mut observations);
        self.run_cpu_sets_soft_update(&settings, &mut observations);
        collect_restore_error(
            &mut errors,
            "App Suspension",
            self.run_app_suspension_update(&settings, false, &[], &mut observations)
                .last_error,
        );
        let suspension = Arc::clone(&self.app_suspension_controller);
        match suspension.lock() {
            Ok(mut controller) => {
                if let Err(error) = self
                    .app_suspension_manager
                    .shutdown(&mut controller, &mut self.action_log)
                {
                    errors.push(format!("App Suspension restoration failed: {error}"));
                }
                if let Err(error) = controller.shutdown() {
                    errors.push(format!("Suspension restoration failed: {error}"));
                }
            }
            Err(_) => errors.push("Suspension restoration state is unavailable.".to_owned()),
        }
        self.run_memory_priority_update(&settings, &mut observations);
        if let Err(error) = self.memory_priority_controller.shutdown() {
            errors.push(format!("Memory Priority restoration failed: {error}"));
        }
        self.run_gpu_priority_update(&settings, &mut observations);
        if let Err(error) = self.gpu_priority_controller.shutdown() {
            errors.push(format!("GPU Priority restoration failed: {error}"));
        }
        self.run_dynamic_priority_boost_update(&settings, &mut observations);
        if let Err(error) = self.dynamic_priority_boost_controller.shutdown() {
            errors.push(format!(
                "Dynamic Priority Boost restoration failed: {error}"
            ));
        }
        self.run_thread_priority_update(&settings, &mut observations);
        if let Err(error) = self.thread_priority_controller.shutdown() {
            errors.push(format!("Thread Priority restoration failed: {error}"));
        }
        self.run_process_priority_update(&settings, &mut observations);
        self.run_io_priority_update(&settings, &mut observations);
        if let Err(error) = self.io_priority_controller.shutdown() {
            errors.push(format!("I/O Priority restoration failed: {error}"));
        }
        self.run_adaptive_engine_process_update(&settings, &mut observations);
        if let Err(error) = self.cpu_allocation_coordinator.shutdown() {
            errors.push(format!("CPU allocation restoration failed: {error}"));
        }
        self.run_background_efficiency_update(&settings, &mut observations);
        if let Err(error) = self.priority_efficiency_controller.shutdown() {
            errors.push(format!(
                "Process Priority and Efficiency restoration failed: {error}"
            ));
        }
        if let Err(error) = self.power_plan_controller.shutdown(Instant::now()) {
            errors.push(error);
        }

        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }

    pub(super) fn replay_sleep_prevention_settings(
        &mut self,
        snapshot: &AutomationSnapshot,
        plugged_in: Option<bool>,
    ) {
        let events = snapshot.wake_events;
        let Some(previous) = &self.last_settings else {
            return;
        };
        if snapshot.sleep_prevention_settings.is_empty()
            || !snapshot.windows_event_watcher_active
            || events.windows_event_watcher_changed
            || events.power_changed
            || events.suspend_resume
            || events.session_changed
            || plugged_in.is_none()
            || previous.plugged_in != plugged_in
            || self.sleep_prevention_power_source() != plugged_in
        {
            return;
        }

        // A saved configuration can filter these callbacks before the watcher is synced.
        let covered = std::iter::once(&previous.configured)
            .chain(&snapshot.sleep_prevention_settings)
            .chain(std::iter::once(&snapshot.settings))
            .all(|settings| {
                windows_event_watcher_required(settings)
                    && [
                        WindowsAutomationEvent::PowerChanged,
                        WindowsAutomationEvent::SuspendResume,
                        WindowsAutomationEvent::SessionChanged,
                    ]
                    .into_iter()
                    .all(|event| windows_event_wake_required(settings, event))
            });
        if covered {
            for settings in &snapshot.sleep_prevention_settings {
                self.sleep_prevention_manager
                    .note_configuration(settings, plugged_in);
            }
        }
    }

    pub(super) fn note_settings(
        &mut self,
        configured: &Arc<Settings>,
        events: AutomationWakeEvents,
        plugged_in: Option<bool>,
    ) -> bool {
        let settings = active_power_source_settings(configured, plugged_in);
        if events.restore_previous_plan_disabled || !settings.general.restore_previous_power_plan {
            self.power_plan_controller.cancel_temporary_restoration();
        }
        self.action_log.set_mode(settings.advanced.action_log_mode);
        set_execution_failure_suppression_threshold(
            settings.advanced.execution_failure_suppression_threshold(),
        );

        if self
            .power_plan_rule_context
            .as_ref()
            .is_some_and(|context| context.source != active_power_source(plugged_in))
        {
            self.power_plan_rule_context = None;
        }
        let source_changed = self
            .last_settings
            .as_ref()
            .is_some_and(|previous| previous.plugged_in != plugged_in);
        if events.suspend_resume {
            self.adaptive_engine_process_manager
                .reset_pressure_observations();
        }
        if source_changed
            || events.suspend_resume
            || events.session_changed
            || events.windows_event_watcher_changed
        {
            self.reset_adaptive_power_demand();
        }
        if events.suspend_resume || events.session_changed {
            self.adaptive_foreground_identity = None;
        }
        // A watcher transition invalidates the coverage behind accepted requests.
        if events.suspend_resume
            || (events.windows_event_watcher_changed
                && self.sleep_prevention_controller.requests().system)
        {
            self.sleep_prevention_manager.suppress_and_release(
                &mut self.sleep_prevention_controller,
                configured,
                &mut self.action_log,
            );
        }
        let previous_settings = self.last_settings.as_ref().map(ObservedSettings::effective);
        let changed = previous_settings
            .is_none_or(|previous| !std::ptr::eq(previous, settings) && previous != settings);
        let cpu_usage_resumed = changed
            && self.last_settings.as_ref().is_some_and(|previous| {
                !cpu_usage_required(&previous.configured, previous.plugged_in)
                    && cpu_usage_required(configured, plugged_in)
            });
        if source_changed || events.suspend_resume || cpu_usage_resumed {
            self.cpu_usage = CpuUsageSnapshot::default();
            self.cpu_usage_sampled_at = None;
            self.cpu_monitor = CpuUsageMonitor::default();
        }
        let explicitly_changed = events.settings_changed || events.suspend_resume || source_changed;
        if source_changed
            || self
                .last_settings
                .as_ref()
                .is_none_or(|previous| !Arc::ptr_eq(&previous.configured, configured))
        {
            self.last_settings = Some(ObservedSettings {
                configured: Arc::clone(configured),
                plugged_in,
            });
        }
        if changed {
            self.power_plan_controller.clear_failures();
        }
        if changed || explicitly_changed {
            self.by_cpu_load_scheduler.reset();
            self.adaptive_engine_process_manager
                .invalidate_allocation_selection();
            if adaptive_power_plan_waits_for_source(configured, plugged_in) {
                self.power_plan_controller.reset_adaptive_demand();
            }
        }
        changed || explicitly_changed
    }

    pub(super) fn detect_process_appearance(&mut self) -> bool {
        let mut current_ids = BTreeSet::new();
        if for_each_process_id(|id| {
            if id != 0 {
                current_ids.insert(id);
            }
        })
        .is_err()
        {
            return false;
        }

        process_ids_have_new_entries(&mut self.known_process_ids, current_ids)
    }

    pub(super) fn poll_controller_activity(&mut self, now: Instant) -> bool {
        self.controller_observed_since.get_or_insert(now);
        self.controller_activity_detector.poll(now)
    }

    pub(super) fn clear_controller_activity(&mut self) {
        self.controller_activity_detector.clear();
        self.controller_observed_since = None;
    }

    pub(super) fn publish_action_log_if_changed(&mut self, shared: &SharedAutomationState) {
        let revision = self.action_log.revision();
        if self.published_action_log_revision == revision {
            return;
        }

        let entries = self.action_log.entries_snapshot();
        let summaries = self.action_log.summaries();
        update_action_log(
            shared,
            &mut lock_unpoisoned(&shared.state),
            entries,
            summaries,
        );
        self.published_action_log_revision = revision;
    }

    pub(super) fn activity_snapshot(
        &self,
        settings: &Settings,
        now: Instant,
    ) -> crate::activity::ActivitySnapshot {
        let idle_timeout = Duration::from_secs(settings.by_activity.idle_timeout_seconds);
        let controller_idle_for = self.controller_activity_detector.idle_for(now).or_else(|| {
            self.controller_observed_since
                .map(|start| now.saturating_duration_since(start))
        });
        let idle_for = self.input_activity.idle_for(
            &settings.by_activity.input_detection,
            controller_idle_for,
            now,
        );
        activity_snapshot(idle_for, idle_timeout)
    }

    pub(super) fn by_activity_observation(
        &self,
        settings: &Settings,
        source: PowerSourceProfile,
        now: Instant,
    ) -> Option<ByActivityObservation> {
        if !settings.general.enabled || !activity_power_plan_required(settings) {
            return None;
        }
        let idle_for = self.activity_snapshot(settings, now).idle_for?;
        Some(ByActivityObservation {
            source,
            input_detection: settings.by_activity.input_detection.clone(),
            idle_timeout_seconds: settings.by_activity.idle_timeout_seconds,
            last_activity: now.checked_sub(idle_for)?,
        })
    }

    pub(super) fn run_background_efficiency_update(
        &mut self,
        settings: &Settings,
        observations: &mut CycleObservations,
    ) -> BackgroundEfficiencySnapshot {
        let foreground_process_id = observations.foreground_process_id();
        self.background_efficiency_manager.update(
            &mut self.priority_efficiency_controller,
            &settings.background_efficiency,
            settings.general.enabled,
            settings.general.allow_cross_session_process_control,
            foreground_process_id,
            observations,
            &mut self.action_log,
        )
    }

    pub(super) fn run_app_suspension_update(
        &mut self,
        settings: &Settings,
        waits_for_source: bool,
        manual_freeze_processes: &[String],
        observations: &mut CycleObservations,
    ) -> AppSuspensionSnapshot {
        let foreground_process_id = if waits_for_source {
            None
        } else {
            observations.foreground_process_id()
        };
        self.with_app_suspension(|manager, controller, action_log| {
            let status = manager.update(
                controller,
                &settings.app_suspension,
                settings.general.enabled,
                waits_for_source,
                settings.general.allow_cross_session_process_control,
                foreground_process_id,
                manual_freeze_processes,
                observations,
                action_log,
            );
            app_suspension_status_with_retry(status, controller)
        })
        .unwrap_or_else(app_suspension_lock_failure)
    }

    fn with_app_suspension_release(
        &mut self,
        release: impl FnOnce(
            &mut AppSuspensionManager,
            &mut SuspensionController,
            &mut ActionLog,
        ) -> Option<AppSuspensionSnapshot>,
    ) -> Option<AppSuspensionSnapshot> {
        self.with_app_suspension(|manager, controller, action_log| {
            release(manager, controller, action_log)
                .map(|status| app_suspension_status_with_retry(status, controller))
        })
        .unwrap_or_else(|error| Some(app_suspension_lock_failure(error)))
    }

    pub(super) fn run_app_suspension_foreground_release(
        &mut self,
        observations: &mut CycleObservations,
    ) -> Option<AppSuspensionSnapshot> {
        let now = Instant::now();
        if shell_window_mouse_pressed() && self.app_suspension_shell_user_intent_due(now) {
            self.last_app_suspension_shell_user_intent = Some(now);
            let status = self.with_app_suspension_release(|manager, controller, action_log| {
                manager.release_all_suspended_processes_for_user_intent(controller, action_log)
            });
            if let Some(status) = status {
                return Some(status);
            }
        }

        let foreground_process_id = observations.foreground_process_id();
        let foreground_process = observations.foreground_process();
        let status = foreground_process_id.and_then(|process_id| {
            self.with_app_suspension_release(|manager, controller, action_log| {
                manager.release_interactive_process(
                    controller,
                    process_id,
                    foreground_process
                        .as_ref()
                        .filter(|process| process.id == process_id)
                        .map(|process| process.executable_path.as_path()),
                    action_log,
                )
            })
        });
        if let Some(status) = status {
            return Some(status);
        }

        let cursor_process_id = cursor_process_id()?;
        if foreground_process_id == Some(cursor_process_id) {
            return None;
        }
        let cursor_process = cursor_process();
        self.with_app_suspension_release(|manager, controller, action_log| {
            manager.release_interactive_process(
                controller,
                cursor_process_id,
                cursor_process
                    .as_ref()
                    .filter(|process| process.id == cursor_process_id)
                    .map(|process| process.executable_path.as_path()),
                action_log,
            )
        })
    }

    pub(super) fn run_app_suspension_app_switch_release(
        &mut self,
        observations: &mut CycleObservations,
    ) -> Option<AppSuspensionSnapshot> {
        self.with_app_suspension_release(|manager, controller, action_log| {
            manager.release_window_owner_processes_for_user_intent(
                controller,
                observations.top_level_window_process_ids().as_ref(),
                action_log,
            )
        })
    }

    pub(super) fn run_app_suspension_shell_click_release(
        &mut self,
    ) -> Option<AppSuspensionSnapshot> {
        if !cursor_is_shell_window() {
            return None;
        }

        self.with_app_suspension_release(|manager, controller, action_log| {
            manager.release_all_suspended_processes_for_user_intent(controller, action_log)
        })
    }

    pub(super) fn app_suspension_shell_user_intent_due(&self, now: Instant) -> bool {
        self.last_app_suspension_shell_user_intent
            .is_none_or(|last| {
                now.duration_since(last) >= APP_SUSPENSION_SHELL_USER_INTENT_INTERVAL
            })
    }

    pub(super) fn run_cpu_sets_soft_update(
        &mut self,
        settings: &Settings,
        observations: &mut CycleObservations,
    ) -> CpuAllocationSnapshot {
        let foreground_process_id = observations.foreground_process_id();
        self.cpu_sets_soft_manager.update(
            &mut self.cpu_allocation_coordinator,
            ControlOwner::CpuSetsSoft,
            &settings.cpu_sets_soft,
            (
                cpu_allocation::CpuAllocationMode::SoftCpuSets,
                ActionLogFeature::CpuSetsSoft,
            ),
            settings.general.enabled,
            settings.general.allow_cross_session_process_control,
            foreground_process_id,
            observations,
            &mut self.action_log,
        )
    }

    pub(super) fn run_processor_affinity_hard_update(
        &mut self,
        settings: &Settings,
        observations: &mut CycleObservations,
    ) -> CpuAllocationSnapshot {
        let processor_affinity_hard = processor_affinity_hard_settings(settings);
        self.processor_affinity_hard_manager.update(
            &mut self.cpu_allocation_coordinator,
            ControlOwner::ProcessorAffinityHard,
            &processor_affinity_hard,
            (
                cpu_allocation::CpuAllocationMode::HardAffinity,
                ActionLogFeature::ProcessorAffinityHard,
            ),
            settings.general.enabled,
            settings.general.allow_cross_session_process_control,
            observations.foreground_process_id(),
            observations,
            &mut self.action_log,
        )
    }

    pub(super) fn run_cpu_limiter_update(
        &mut self,
        settings: &Settings,
        waiting_for_power_source: bool,
        observations: &mut CycleObservations,
    ) -> CpuLimiterSnapshot {
        if waiting_for_power_source {
            return match self.cpu_limiter_controller.as_mut() {
                Some(controller) => self
                    .cpu_limiter_manager
                    .paused(controller, &mut self.action_log),
                None => CpuLimiterSnapshot {
                    enabled: true,
                    observation_paused: true,
                    ..Default::default()
                },
            };
        }
        if self.cpu_limiter_controller.is_none()
            && !feature_refresh_required(settings, cpu_limiter_required(settings))
        {
            return CpuLimiterSnapshot {
                enabled: settings.general.enabled && settings.cpu_limiter.enabled,
                ..Default::default()
            };
        }
        let controller = match self.cpu_limiter_controller.as_mut() {
            Some(controller) => controller,
            None => match CpuLimiterController::new(Arc::clone(&self.app_suspension_controller)) {
                Ok(controller) => self.cpu_limiter_controller.insert(controller),
                Err(error) => {
                    return CpuLimiterSnapshot {
                        enabled: settings.general.enabled && settings.cpu_limiter.enabled,
                        last_error: Some(error),
                        ..Default::default()
                    };
                }
            },
        };
        self.cpu_limiter_manager.update(
            controller,
            &settings.cpu_limiter,
            settings.general.enabled,
            settings.general.allow_cross_session_process_control,
            observations,
            &mut self.action_log,
        )
    }

    pub(super) fn run_cpu_allocation_reconciliation(
        &mut self,
        shared: &SharedAutomationState,
        settings: &Settings,
        observations: &mut CycleObservations,
        include_release_retries: bool,
    ) {
        let topology = observations.cpu_topology();
        let summary = self.cpu_allocation_coordinator.reconcile_pending(
            topology.as_deref().map_err(String::as_str),
            settings.general.allow_cross_session_process_control,
            include_release_retries,
        );
        record_cpu_allocation_reconciliation(summary, &mut self.action_log);
        if let Some(status) = self.cpu_sets_soft_manager.take_settled_release_status(
            &self.cpu_allocation_coordinator,
            ControlOwner::CpuSetsSoft,
        ) {
            update_cpu_sets_soft_status(shared, status);
        }
        if let Some(status) = self
            .processor_affinity_hard_manager
            .take_settled_release_status(
                &self.cpu_allocation_coordinator,
                ControlOwner::ProcessorAffinityHard,
            )
        {
            update_processor_affinity_hard_status(shared, status);
        }
    }

    pub(super) fn cpu_allocation_immediate_reconciliation_pending(&self) -> bool {
        self.cpu_allocation_coordinator
            .has_pending_immediate_reconciliation()
    }

    pub(super) fn cpu_allocation_release_retry_pending(&self) -> bool {
        self.cpu_allocation_coordinator.has_pending_release_retry()
    }

    pub(super) fn run_by_running_app_update(
        &mut self,
        settings: &Settings,
        observations: &mut CycleObservations,
    ) -> Option<ByRunningAppSnapshot> {
        self.by_running_app_manager.update(
            &settings.by_running_app,
            settings.general.enabled,
            observations,
        )
    }

    pub(super) fn run_adaptive_engine_process_update(
        &mut self,
        settings: &Settings,
        observations: &mut CycleObservations,
    ) -> AdaptiveEngineProcessSnapshot {
        if bottleneck_classifier_required(settings) {
            self.refresh_cpu_usage();
        }
        let foreground_process_id = observations.foreground_process_id();
        let excluded_process_ids = self.by_running_app_manager.active_process_ids();
        let explicit_cpu_allocation_paths = explicit_cpu_allocation_paths(settings);
        self.adaptive_engine_process_manager.update(
            AdaptiveEngineProcessUpdate {
                settings: &settings.adaptive_engine_process,
                automation_enabled: settings.general.enabled && settings.adaptive_engine.enabled,
                allow_cross_session_process_control: settings
                    .general
                    .allow_cross_session_process_control,
                foreground_process_id,
                total_cpu_usage_percent: self.cpu_usage.percent,
                excluded_process_ids: &excluded_process_ids,
                explicit_cpu_allocation_paths: &explicit_cpu_allocation_paths,
                observations,
            },
            &mut self.cpu_allocation_coordinator,
            &mut self.priority_efficiency_controller,
            &mut self.action_log,
        )
    }

    pub(super) fn publish_adaptive_engine_process_status(
        &self,
        shared: &SharedAutomationState,
        source: PowerSourceProfile,
        mut status: AdaptiveEngineProcessSnapshot,
    ) {
        status.source = Some(source);
        if status.enabled && !status.observation_paused {
            let owner = ControlOwner::AdaptiveEngine;
            status.adjusted_processes = self
                .priority_efficiency_controller
                .policy_managed_processes(owner)
                .chain(
                    self.cpu_allocation_coordinator
                        .policy_managed_processes(owner),
                )
                .collect::<BTreeSet<_>>()
                .len();
        }
        update_adaptive_engine_process_status(shared, status);
    }

    pub(super) fn reset_bottleneck_classifier(&mut self) -> BottleneckSnapshot {
        self.bottleneck_classifier.reset()
    }

    pub(super) fn run_bottleneck_classifier_update(&mut self) -> BottleneckSnapshot {
        self.refresh_cpu_usage();
        self.bottleneck_classifier.sample(self.cpu_usage.percent)
    }

    pub(super) fn run_adaptive_power_plan_update(
        &mut self,
        settings: &Settings,
        observations: &mut CycleObservations,
    ) {
        let result = if adaptive_power_plan_required(settings) && settings.general.enabled {
            self.refresh_cpu_usage();
            let (focus_or_launch, foreground_cpu_percent) =
                self.sample_adaptive_power_foreground(observations);
            self.update_adaptive_power_plan(
                settings.adaptive_engine.base_processor_policy.normalized(),
                settings.adaptive_engine.background_pressure_profile,
                settings.adaptive_engine.focus_and_launch_profile,
                focus_or_launch,
                foreground_cpu_percent,
            )
        } else {
            self.reset_adaptive_power_demand();
            self.adaptive_foreground_identity = None;
            if self.power_plan_controller.has_pending_cleanup() {
                self.power_plan_controller.release_adaptive(Instant::now())
            } else {
                Ok(())
            }
        };
        self.adaptive_processor_power_error = result.err();
    }

    fn reset_adaptive_power_demand(&mut self) {
        self.adaptive_foreground_cpu_history = ForegroundCpuHistory::default();
        self.per_processor_cpu_monitor = PerProcessorUsageMonitor::default();
        self.io_monitor = IoUsageMonitor::default();
        self.adaptive_io_usage = IoUsageSnapshot::default();
        self.next_adaptive_io_refresh = None;
    }

    fn sample_adaptive_power_foreground(
        &mut self,
        observations: &mut CycleObservations,
    ) -> (bool, Option<f32>) {
        let Some(id) = observations.foreground_process_id() else {
            self.adaptive_foreground_identity = None;
            self.adaptive_foreground_cpu_history = ForegroundCpuHistory::default();
            return (false, None);
        };
        let Ok(processes) = observations.foreground_process_ancestry(id) else {
            self.adaptive_foreground_cpu_history = ForegroundCpuHistory::default();
            return (false, None);
        };
        let workload = observations.adaptive_workload(&processes);
        let cpu_sample = observations.adaptive_cpu_sample(&workload);
        let identity = workload.get(&id).map(|&creation| (id, creation));
        let foreground_cpu_percent = self
            .adaptive_foreground_cpu_history
            .update(workload, cpu_sample);
        let changed = identity.is_some() && identity != self.adaptive_foreground_identity;
        if identity.is_some() {
            self.adaptive_foreground_identity = identity;
        }
        let launch = identity.is_some_and(|(_, creation)| {
            processes
                .iter()
                .find(|process| process.id == id)
                .is_some_and(|process| adaptive_power_launch_eligible(process, creation))
        });
        (changed || launch, foreground_cpu_percent)
    }

    fn update_adaptive_power_plan(
        &mut self,
        baseline: ProcessorPowerValues,
        background_pressure_profile: AdaptivePowerBoostValues,
        focus_and_launch_profile: AdaptivePowerBoostValues,
        focus_or_launch: bool,
        foreground_cpu_percent: Option<f32>,
    ) -> Result<(), String> {
        let now = Instant::now();
        if self.adaptive_processor_topology.is_empty() {
            self.adaptive_processor_topology = cpu_allocation::logical_processors();
        }
        let processor_demand = self
            .per_processor_cpu_monitor
            .sample()
            .map(|usage| adaptive_processor_demand(&usage, &self.adaptive_processor_topology))
            .unwrap_or_default();
        let demand = AdaptivePowerDemand {
            focus_and_launch_profile_active: focus_or_launch,
            total_cpu_percent: self.cpu_usage.percent,
            peak_cpu_percent: processor_demand.peak_cpu_percent,
            performance_peak_cpu_percent: processor_demand.performance_peak_cpu_percent,
            efficiency_peak_cpu_percent: processor_demand.efficiency_peak_cpu_percent,
            foreground_cpu_percent,
            io_bytes_per_second: None,
        };
        let mut desired_profile = AdaptivePowerProfile::for_demand(demand);
        if desired_profile == AdaptivePowerProfile::Idle {
            if self
                .next_adaptive_io_refresh
                .is_none_or(|refresh_at| now >= refresh_at)
            {
                self.adaptive_io_usage = self.io_monitor.sample();
                self.next_adaptive_io_refresh = Some(now + ADAPTIVE_IO_REFRESH_INTERVAL);
            }
            desired_profile = AdaptivePowerProfile::for_demand(AdaptivePowerDemand {
                io_bytes_per_second: self.adaptive_io_usage.bytes_per_second,
                ..demand
            });
        } else {
            // I/O can only raise Idle to Responsive. Restart its baseline when needed;
            // the power controller's deescalation delay exceeds the sampling interval.
            self.io_monitor = IoUsageMonitor::default();
            self.adaptive_io_usage = IoUsageSnapshot::default();
            self.next_adaptive_io_refresh = None;
        }
        let has_efficiency_cores = self
            .adaptive_processor_topology
            .iter()
            .any(|processor| processor.kind == LogicalProcessorKind::Efficiency);

        self.power_plan_controller
            .reconcile_adaptive(
                AdaptivePowerPlanRequest {
                    profile: desired_profile,
                    baseline,
                    has_efficiency_cores,
                    background_pressure_profile,
                    focus_and_launch_profile,
                },
                now,
            )
            .map(|_| ())
    }

    pub(super) fn run_io_priority_update(
        &mut self,
        settings: &Settings,
        observations: &mut CycleObservations,
    ) -> IoPrioritySnapshot {
        self.io_priority_manager.update(
            &mut self.io_priority_controller,
            &settings.io_priority,
            settings.general.enabled,
            settings.general.allow_cross_session_process_control,
            observations.foreground_process_id(),
            observations,
            &mut self.action_log,
        )
    }

    pub(super) fn run_process_priority_update(
        &mut self,
        settings: &Settings,
        observations: &mut CycleObservations,
    ) -> ProcessPrioritySnapshot {
        self.process_priority_manager.update(
            &mut self.priority_efficiency_controller,
            &settings.process_priority,
            settings.general.enabled,
            settings.general.allow_cross_session_process_control,
            observations.foreground_process_id(),
            observations,
            &mut self.action_log,
        )
    }

    pub(super) fn run_thread_priority_update(
        &mut self,
        settings: &Settings,
        observations: &mut CycleObservations,
    ) -> ThreadPrioritySnapshot {
        self.thread_priority_manager.update(
            &mut self.thread_priority_controller,
            &settings.thread_priority,
            settings.general.enabled,
            settings.general.allow_cross_session_process_control,
            observations.foreground_process_id(),
            observations,
            &mut self.action_log,
        )
    }

    pub(super) fn run_dynamic_priority_boost_update(
        &mut self,
        settings: &Settings,
        observations: &mut CycleObservations,
    ) -> DynamicPriorityBoostSnapshot {
        self.dynamic_priority_boost_manager.update(
            &mut self.dynamic_priority_boost_controller,
            &settings.dynamic_priority_boost,
            settings.general.enabled,
            settings.general.allow_cross_session_process_control,
            observations.foreground_process_id(),
            observations,
            &mut self.action_log,
        )
    }

    pub(super) fn run_process_control_commands(
        &mut self,
        settings: &Settings,
        app_suspension_waits_for_source: bool,
        commands: VecDeque<ProcessControlCommand>,
        observations: &mut CycleObservations,
    ) -> ProcessControlCommandStatuses {
        let mut statuses = ProcessControlCommandStatuses::default();
        for command in commands {
            match command {
                ProcessControlCommand::ProcessPriority {
                    targets,
                    priority,
                    result,
                } => {
                    let results = targets
                        .into_iter()
                        .map(|target| {
                            let target = target.map_err(|error| error.to_string())?;
                            self.priority_efficiency_controller
                                .apply_process_list_priority(
                                    &target,
                                    priority,
                                    settings.general.allow_cross_session_process_control,
                                )
                                .map(|_| ())
                                .map_err(|error| error.to_string())
                        })
                        .collect();
                    let _ = result.try_send(Ok(ProcessControlBatchResult { results }));
                }
                ProcessControlCommand::EfficiencyMode {
                    targets,
                    enabled,
                    result,
                } => {
                    let results = targets
                        .into_iter()
                        .map(|target| {
                            let target = target.map_err(|error| error.to_string())?;
                            self.priority_efficiency_controller
                                .apply_process_list_efficiency_mode(
                                    &target,
                                    enabled,
                                    settings.general.allow_cross_session_process_control,
                                )
                                .map(|_| ())
                                .map_err(|error| error.to_string())
                        })
                        .collect();
                    let _ = result.try_send(Ok(ProcessControlBatchResult { results }));
                }
                ProcessControlCommand::DynamicPriorityBoost {
                    targets,
                    state,
                    result,
                } => {
                    let results = targets
                        .into_iter()
                        .map(|target| {
                            let target = target.map_err(|error| error.to_string())?;
                            self.dynamic_priority_boost_controller
                                .apply_process_list_action(
                                    &target,
                                    state,
                                    settings.general.allow_cross_session_process_control,
                                )
                                .map(|_| ())
                                .map_err(|error| error.to_string())
                        })
                        .collect();
                    let _ = result.try_send(Ok(ProcessControlBatchResult { results }));
                }
                ProcessControlCommand::ThreadPriority {
                    targets,
                    priority,
                    result,
                } => {
                    let results = targets
                        .into_iter()
                        .map(|target| {
                            let target = target.map_err(|error| error.to_string())?;
                            self.thread_priority_controller
                                .apply_process_list_action(
                                    &target,
                                    priority,
                                    settings.general.allow_cross_session_process_control,
                                )
                                .map(|_| ())
                                .map_err(|error| error.to_string())
                        })
                        .collect();
                    let _ = result.try_send(Ok(ProcessControlBatchResult { results }));
                }
                ProcessControlCommand::IoPriority {
                    targets,
                    priority,
                    result,
                } => {
                    let results = targets
                        .into_iter()
                        .map(|target| {
                            let target = target.map_err(|error| error.to_string())?;
                            self.io_priority_controller
                                .apply_process_list_action(
                                    &target,
                                    priority,
                                    settings.general.allow_cross_session_process_control,
                                )
                                .map(|_| ())
                                .map_err(|error| error.to_string())
                        })
                        .collect();
                    let _ = result.try_send(Ok(ProcessControlBatchResult { results }));
                }
                ProcessControlCommand::GpuPriority {
                    targets,
                    priority,
                    result,
                } => {
                    let results = targets
                        .into_iter()
                        .map(|target| {
                            let target = target.map_err(|error| error.to_string())?;
                            self.gpu_priority_controller
                                .apply_process_list_action(
                                    &target,
                                    priority,
                                    settings.general.allow_cross_session_process_control,
                                )
                                .map(|_| ())
                                .map_err(|error| error.to_string())
                        })
                        .collect();
                    let _ = result.try_send(Ok(ProcessControlBatchResult { results }));
                }
                ProcessControlCommand::MemoryPriority {
                    targets,
                    priority,
                    result,
                } => {
                    let results = targets
                        .into_iter()
                        .map(|target| {
                            let target = target.map_err(|error| error.to_string())?;
                            self.memory_priority_controller
                                .apply_process_list_action(
                                    &target,
                                    priority,
                                    settings.general.allow_cross_session_process_control,
                                )
                                .map(|_| ())
                                .map_err(|error| error.to_string())
                        })
                        .collect();
                    let _ = result.try_send(Ok(ProcessControlBatchResult { results }));
                }
                ProcessControlCommand::AppSuspension {
                    targets,
                    suspend,
                    result,
                } => {
                    match self.with_app_suspension(|manager, controller, action_log| {
                        targets
                            .into_iter()
                            .map(|target| {
                                let target = target.map_err(|error| error.to_string())?;
                                manager.apply_manual_process_action(
                                    controller,
                                    &target,
                                    suspend,
                                    settings.general.allow_cross_session_process_control,
                                    action_log,
                                )
                            })
                            .collect()
                    }) {
                        Ok(results) => {
                            let _ = result.try_send(Ok(ProcessControlBatchResult { results }));
                        }
                        Err(error) => {
                            let _ = result.try_send(Err(RuntimeCommandError::CommandFailed(error)));
                        }
                    }
                }
                ProcessControlCommand::AppSuspensionPathAction {
                    executable_path,
                    freeze,
                    result,
                } => {
                    let (status, reply) = if freeze {
                        let status = self.run_app_suspension_update(
                            settings,
                            app_suspension_waits_for_source,
                            std::slice::from_ref(&executable_path),
                            observations,
                        );
                        let reply = self
                            .app_suspension_manager
                            .manual_freeze_result(&executable_path)
                            .map(|()| status.clone())
                            .map_err(|error| {
                                RuntimeCommandError::CommandFailed(
                                    if !status.enabled
                                        || status.unsupported
                                        || status.status_unknown
                                    {
                                        status.status.to_string()
                                    } else {
                                        error
                                    },
                                )
                            });
                        (status, reply)
                    } else {
                        self.with_app_suspension(|manager, controller, action_log| {
                            let status = manager.release_suspended_path_for_user_intent(
                                controller,
                                Path::new(&executable_path),
                                action_log,
                            );
                            let status = app_suspension_release_status(
                                status,
                                settings,
                                app_suspension_waits_for_source,
                            );
                            let reply = match &status.last_error {
                                Some(error) => {
                                    Err(RuntimeCommandError::CommandFailed(error.clone()))
                                }
                                None => Ok(status.clone()),
                            };
                            (app_suspension_status_with_retry(status, controller), reply)
                        })
                        .unwrap_or_else(|error| {
                            let status = app_suspension_release_status(
                                app_suspension_lock_failure(error.clone()),
                                settings,
                                app_suspension_waits_for_source,
                            );
                            (status, Err(RuntimeCommandError::CommandFailed(error)))
                        })
                    };
                    let _ = result.try_send(reply);
                    statuses.app_suspension = Some(status);
                }
                ProcessControlCommand::MemoryTrim { result } => {
                    let status = self.run_memory_trim_now(settings, observations);
                    let reply = match &status.status {
                        MemoryTrimStatus::Error(error) => {
                            Err(RuntimeCommandError::CommandFailed(error.clone()))
                        }
                        _ => Ok(status.clone()),
                    };
                    let _ = result.try_send(reply);
                    statuses.memory_trim = Some(status);
                }
                ProcessControlCommand::StopProcesses { targets, result } => {
                    self.keep_running
                        .pause_targets(&targets, PauseReason::IntentionalStop);
                    let outcome = self.process_termination_controller.terminate_batch(
                        targets,
                        settings.general.allow_cross_session_process_control,
                    );
                    let _ = result.try_send(Ok(outcome));
                }
                ProcessControlCommand::StopTreeIntent { roots } => {
                    self.keep_running
                        .pause_targets(&roots, PauseReason::IntentionalStop);
                }
                ProcessControlCommand::KeepRunningResume { executable_path } => {
                    self.keep_running.resume(&executable_path);
                }
                ProcessControlCommand::SwitchPowerPlan { guid, result } => {
                    let _ = result.try_send(self.switch_power_plan(settings, &guid));
                }
            }
        }
        statuses
    }

    pub(super) fn retry_control_releases(
        &mut self,
        shared: &SharedAutomationState,
        now: Instant,
    ) -> Option<String> {
        fn current_feature_status<T: Clone>(
            shared: &SharedAutomationState,
            select: impl FnOnce(&RuntimeFeatureStatus) -> &T,
        ) -> T {
            select(&lock_unpoisoned(&shared.state).status.feature_status).clone()
        }

        let process_failure = |property, process_name, process_id, error| {
            format!("{property} restoration failed for {process_name} ({process_id}): {error}")
        };
        let retries = self
            .priority_efficiency_controller
            .retry_pending_releases(now);
        if retries.process_priority_restored.is_some()
            || self
                .process_priority_manager
                .reported_release_is_resolved(&self.priority_efficiency_controller)
        {
            let status = current_feature_status(shared, |features| &features.process_priority);
            update_process_priority_status(
                shared,
                self.process_priority_manager.report_release_retry(
                    &self.priority_efficiency_controller,
                    retries.process_priority_restored.unwrap_or_default(),
                    status,
                    &mut self.action_log,
                ),
            );
        }
        let refresh_background_efficiency = retries.background_efficiency.is_some()
            || self
                .background_efficiency_manager
                .reported_release_is_resolved(&self.priority_efficiency_controller);
        if let Some(summary) = retries.background_efficiency {
            BackgroundEfficiencyManager::record_release_summary(
                summary,
                &mut self.action_log,
                "background_efficiency.release_reason_retry_completed",
            );
        }
        if refresh_background_efficiency {
            let status = current_feature_status(shared, |features| &features.background_efficiency);
            update_background_efficiency_status(
                shared,
                self.background_efficiency_manager
                    .with_release_status(&self.priority_efficiency_controller, status),
            );
        }
        let mut errors = retries
            .other
            .failures
            .into_iter()
            .map(|failure| {
                process_failure(
                    failure.property,
                    failure.process_name,
                    failure.process_id,
                    failure.error,
                )
            })
            .collect::<Vec<_>>();
        if self.thread_priority_controller.release_retry_delay(now) == Some(Duration::ZERO) {
            let summary = self.thread_priority_controller.retry_pending_releases(now);
            let status = current_feature_status(shared, |features| &features.thread_priority);
            update_thread_priority_status(
                shared,
                self.thread_priority_manager.report_release_retry(
                    &self.thread_priority_controller,
                    summary,
                    status,
                    &mut self.action_log,
                ),
            );
        }
        if self.io_priority_controller.release_retry_delay(now) == Some(Duration::ZERO) {
            let summary = self.io_priority_controller.retry_pending_releases(now);
            let status = current_feature_status(shared, |features| &features.io_priority);
            update_io_priority_status(
                shared,
                self.io_priority_manager.report_release_retry(
                    &self.io_priority_controller,
                    summary,
                    status,
                    &mut self.action_log,
                ),
            );
        }
        if self.gpu_priority_controller.release_retry_delay(now) == Some(Duration::ZERO) {
            let summary = self.gpu_priority_controller.retry_pending_releases(now);
            let status = current_feature_status(shared, |features| &features.gpu_priority);
            update_gpu_priority_status(
                shared,
                self.gpu_priority_manager.report_release_retry(
                    &self.gpu_priority_controller,
                    summary,
                    status,
                    &mut self.action_log,
                ),
            );
        }
        let memory_retry_due =
            self.memory_priority_controller.release_retry_delay(now) == Some(Duration::ZERO);
        let memory_retries = self.memory_priority_controller.retry_pending_releases(now);
        for (owner, count) in memory_retries.restored_by_owner {
            let feature = match owner {
                MemoryPriorityReleaseOwner::MemoryPriority => ActionLogFeature::MemoryPriority,
                MemoryPriorityReleaseOwner::AdaptiveEngine => ActionLogFeature::AdaptiveEngine,
            };
            MemoryPriorityManager::record_restoration(
                &mut self.action_log,
                feature,
                count,
                "restoration retry completed",
            );
        }
        errors.extend(
            memory_retries
                .failures
                .into_iter()
                .filter(|failure| {
                    failure.pending_owner != Some(MemoryPriorityReleaseOwner::MemoryPriority)
                })
                .map(|failure| {
                    process_failure(
                        "Memory Priority",
                        failure.process_name,
                        failure.process_id,
                        failure.error,
                    )
                }),
        );
        if memory_retry_due {
            let status = current_feature_status(shared, |features| &features.memory_priority);
            update_feature_status(
                shared,
                self.memory_priority_manager
                    .with_static_release_status(&self.memory_priority_controller, status),
                |features| &features.memory_priority,
                |features| &mut features.memory_priority,
            );
        }
        if self
            .dynamic_priority_boost_controller
            .release_retry_delay(now)
            == Some(Duration::ZERO)
        {
            let summary = self
                .dynamic_priority_boost_controller
                .retry_pending_releases(now);
            let status =
                current_feature_status(shared, |features| &features.dynamic_priority_boost);
            update_dynamic_priority_boost_status(
                shared,
                self.dynamic_priority_boost_manager.report_release_retry(
                    &self.dynamic_priority_boost_controller,
                    summary,
                    status,
                    &mut self.action_log,
                ),
            );
        }
        if let Err(error) = self.power_plan_controller.retry_pending_cleanup(now) {
            self.adaptive_processor_power_error =
                Some(format!("Adaptive plan cleanup failed: {error}"));
        }
        (!errors.is_empty()).then(|| errors.join("; "))
    }

    pub(super) fn control_release_retry_delay(&self, now: Instant) -> Option<Duration> {
        [
            self.priority_efficiency_controller.release_retry_delay(now),
            self.thread_priority_controller.release_retry_delay(now),
            self.io_priority_controller.release_retry_delay(now),
            self.gpu_priority_controller.release_retry_delay(now),
            self.memory_priority_controller.release_retry_delay(now),
            self.dynamic_priority_boost_controller
                .release_retry_delay(now),
            self.power_plan_controller.cleanup_retry_delay(now),
        ]
        .into_iter()
        .flatten()
        .min()
    }

    pub(super) fn has_managed_process_control_state(&self) -> bool {
        let app_suspension_active = self.app_suspension_active();
        self.power_plan_controller.has_pending_cleanup()
            || self.power_plan_controller.has_temporary_plan()
            || self.cpu_allocation_coordinator.has_managed_state()
            || self.cpu_allocation_coordinator.has_pending_reconciliation()
            || self.dynamic_priority_boost_controller.has_managed_state()
            || self.thread_priority_controller.has_managed_state()
            || self.io_priority_controller.has_managed_state()
            || self.gpu_priority_controller.has_managed_state()
            || self.memory_priority_controller.has_managed_state()
            || self.priority_efficiency_controller.has_managed_state()
            || self
                .cpu_limiter_controller
                .as_ref()
                .is_some_and(CpuLimiterController::has_managed_state)
            || app_suspension_active
            || self.keep_running.has_pending()
    }

    pub(super) fn cpu_limiter_release_pending(&self) -> bool {
        self.cpu_limiter_manager.release_pending()
    }

    pub(super) fn cpu_limiter_release_in_progress(&self) -> bool {
        self.cpu_limiter_controller
            .as_ref()
            .is_some_and(CpuLimiterController::release_in_progress)
    }

    pub(super) fn app_suspension_active(&self) -> bool {
        self.app_suspension_controller
            .lock()
            .map_or(true, |controller| {
                self.app_suspension_manager
                    .has_suspended_processes(&controller)
            })
    }

    pub(super) fn adaptive_power_plan_managed(&self) -> bool {
        self.power_plan_controller.has_pending_cleanup()
    }

    pub(super) fn adaptive_power_plan_waiting_for_source(&self) -> bool {
        self.last_settings.as_ref().is_some_and(|observed| {
            adaptive_power_plan_waits_for_source(&observed.configured, observed.plugged_in)
        })
    }

    pub(super) fn has_temporary_power_plan(&self) -> bool {
        self.power_plan_controller.has_temporary_plan()
    }

    pub(super) fn temporary_power_plan_retry_delay(&self) -> Option<Duration> {
        self.power_plan_controller.temporary_release_retry_delay()
    }

    pub(super) fn run_gpu_priority_update(
        &mut self,
        settings: &Settings,
        observations: &mut CycleObservations,
    ) -> GpuPrioritySnapshot {
        self.gpu_priority_manager.update(
            &mut self.gpu_priority_controller,
            &settings.gpu_priority,
            settings.general.enabled,
            settings.general.allow_cross_session_process_control,
            observations.foreground_process_id(),
            observations,
            &mut self.action_log,
        )
    }

    pub(super) fn run_memory_priority_update(
        &mut self,
        settings: &Settings,
        observations: &mut CycleObservations,
    ) -> MemoryPrioritySnapshot {
        self.memory_priority_manager.update_rules(
            &mut self.memory_priority_controller,
            &settings.memory_priority,
            settings.general.enabled,
            settings.general.allow_cross_session_process_control,
            observations.foreground_process_id(),
            observations,
            &mut self.action_log,
        )
    }

    pub(super) fn pause_memory_trim_for_power_source(
        &mut self,
        manual_trim_blocked: bool,
    ) -> MemoryTrimSnapshot {
        self.memory_trim_manager.break_idle_intervals();
        MemoryTrimSnapshot::waiting_for_power_source(manual_trim_blocked)
    }

    pub(super) fn run_memory_trim_update(
        &mut self,
        settings: &Settings,
        observations: &mut CycleObservations,
    ) -> MemoryTrimSnapshot {
        self.memory_trim_manager.update(
            &mut self.memory_trim_controller,
            &settings.memory_trim,
            settings.general.enabled,
            settings.general.allow_cross_session_process_control,
            observations.foreground_process_id(),
            observations,
            &mut self.action_log,
        )
    }

    pub(super) fn run_memory_trim_now(
        &mut self,
        settings: &Settings,
        observations: &mut CycleObservations,
    ) -> MemoryTrimSnapshot {
        if self.last_settings.as_ref().is_some_and(|observed| {
            observed.plugged_in.is_none()
                && memory_trim_policy_difference(&observed.configured)
                    == MemoryTrimPolicyDifference::ManualAndAutomatic
        }) {
            return MemoryTrimSnapshot::waiting_for_power_source(true);
        }
        self.memory_trim_manager.trim_now(
            &mut self.memory_trim_controller,
            &settings.memory_trim,
            settings.general.enabled,
            settings.general.allow_cross_session_process_control,
            observations.foreground_process_id(),
            observations,
            &mut self.action_log,
        )
    }

    pub(super) fn run_sleep_prevention_update(
        &mut self,
        settings: &Settings,
        observations: &mut CycleObservations,
        windows_event_watcher_active: bool,
        plugged_in: Option<bool>,
    ) -> SleepPreventionSnapshot {
        self.sleep_prevention_manager.update(
            &mut self.sleep_prevention_controller,
            settings,
            plugged_in,
            windows_event_watcher_active,
            observations,
            &mut self.action_log,
        )
    }

    pub(super) fn sleep_prevention_power_source(&self) -> Option<bool> {
        self.sleep_prevention_manager.last_known_power_source()
    }

    pub(super) fn timer_resolution_request_active(&self) -> bool {
        self.timer_resolution_controller
            .active_request_100ns()
            .is_some()
    }

    pub(super) fn run_timer_resolution_update(
        &mut self,
        settings: &Settings,
        observations: &mut CycleObservations,
    ) -> TimerResolutionSnapshot {
        let foreground_process =
            feature_refresh_required(settings, timer_resolution_required(settings))
                .then(|| observations.noncritical_foreground_process())
                .flatten();
        self.timer_resolution_manager.update(
            &mut self.timer_resolution_controller,
            &settings.timer_resolution,
            settings.general.enabled,
            foreground_process
                .as_ref()
                .and_then(|process| process.executable_path.to_str()),
            &mut self.action_log,
        )
    }

    pub(super) fn run_check(
        &mut self,
        configured_settings: &Arc<Settings>,
        plugged_in: Option<bool>,
        wall_time: DateTime<Local>,
        activity_state: ActivityState,
        observations: &mut CycleObservations,
    ) -> Result<(), ActivePlanCheckError> {
        if self.power_plan_controller.adaptive_active() {
            self.power_plan_rule_context = None;
            self.by_cpu_load_scheduler.reset();
            return Ok(());
        }

        let active_plan = self
            .power_plan_controller
            .refresh_active_plan_for_status(Instant::now(), false);
        if ordinary_power_plan_waits_for_source(configured_settings, plugged_in) {
            return active_plan;
        }

        let settings = active_power_source_settings(configured_settings, plugged_in);
        if let Err(error) = active_plan {
            self.by_cpu_load_scheduler.reset();
            let status = self.power_plan_controller.status();
            let activity_conflicts_with_choice = matches!(
                (status.decision_state, activity_state),
                (Some(DecisionState::ByActivityIdle), ActivityState::Active)
                    | (Some(DecisionState::ByActivityActive), ActivityState::Idle)
            );
            let schedule_conflicts_with_choice = status.decision_state
                == Some(DecisionState::ByTime)
                && !current_by_time_decision(&settings.by_time, wall_time).is_some_and(
                    |decision| {
                        status.rule_index == Some(decision.rule_index)
                            && status.target_guid.as_deref().is_some_and(|target| {
                                target.eq_ignore_ascii_case(&decision.power_plan_guid)
                            })
                    },
                );
            if self
                .power_plan_rule_context
                .as_ref()
                .is_some_and(|context| {
                    let focus_conflicts_with_choice = status.decision_state
                        == Some(DecisionState::ByFocus)
                        && observations
                            .cached_foreground_executable_path()
                            .is_some_and(|path| {
                                !settings.by_focus.enabled
                                    || !matching_by_focus_rules(&settings.by_focus, path)
                                        .next()
                                        .is_some_and(|(index, _, plan)| {
                                            status.rule_index == Some(index)
                                                && status.target_guid.as_deref().is_some_and(
                                                    |target| target.eq_ignore_ascii_case(plan),
                                                )
                                        })
                            });
                    activity_conflicts_with_choice
                        || focus_conflicts_with_choice
                        || schedule_conflicts_with_choice
                        || (status.decision_state == Some(DecisionState::ByRunningApp)
                            && status.rule_index.is_some_and(|index| {
                                self.by_running_app_manager.rules_out_selection(index)
                            }))
                        || (!Arc::ptr_eq(&context.settings, configured_settings)
                            && !ordinary_power_plan_choice_matches(
                                &status,
                                active_power_source_settings(&context.settings, plugged_in),
                                settings,
                            ))
                })
            {
                self.power_plan_rule_context = None;
            }
            if let Some(feature) = status
                .decision_state
                .and_then(power_plan_action_log_feature)
            {
                self.action_log.record(
                    feature,
                    None,
                    String::new(),
                    ActionLogResult::Failed,
                    format!("Refreshing the active power plan failed: {error}"),
                );
            }
            return Err(error);
        }
        self.by_cpu_load_scheduler
            .observe_active_plan(self.power_plan_controller.current_guid());

        let cpu_sample = if by_cpu_load_sample_required(settings, plugged_in) {
            self.refresh_cpu_usage();
            self.cpu_usage
                .percent
                .zip(self.cpu_usage_sampled_at)
                .filter(|(usage, _)| usage.is_finite() && (0.0..=100.0).contains(usage))
        } else {
            None
        };
        let observe_foreground =
            foreground_lookup_required(settings) || self.power_plan_controller.has_focus_plan();
        let foreground_process_id = observe_foreground
            .then(|| observations.foreground_process_id())
            .flatten();
        let (foreground_executable_path, focus_process_name) = observe_foreground
            .then(|| observations.noncritical_foreground_process())
            .flatten()
            .map(|process| (process.executable_path, process.name))
            .unzip();
        let by_time_decision = current_by_time_decision(&settings.by_time, wall_time);
        let by_cpu_load_decision = self.by_cpu_load_scheduler.current_decision_for_sample_at(
            &settings.by_cpu_load,
            cpu_sample,
            Instant::now(),
        );
        let primary_cpu_load_rule_index = by_cpu_load_decision
            .as_ref()
            .filter(|decision| !decision.is_else())
            .map(|decision| decision.rule_index);
        let by_running_app = self.by_running_app_manager.active_decision().map(
            |(rule_index, rule_name, process_name, power_plan_guid)| ByRunningAppDecision {
                rule_index,
                rule_name,
                process_name,
                power_plan_guid,
            },
        );
        let running_process_name = by_running_app
            .as_ref()
            .map(|decision| decision.process_name.clone());
        let decision_input = DecisionInput {
            activity_state,
            foreground_executable_path,
            foreground_process_id,
            by_running_app_known: self.by_running_app_manager.observation_known(),
            cpu_load_known: cpu_sample.is_some(),
            plugged_in,
            by_running_app,
            by_time: by_time_decision,
            by_cpu_load: by_cpu_load_decision,
        };
        let decision = decide(settings, decision_input);
        self.power_plan_rule_context =
            decision
                .power_plan_guid
                .as_ref()
                .map(|_| PowerPlanRuleContext {
                    settings: Arc::clone(configured_settings),
                    source: active_power_source(plugged_in),
                });
        let result = self
            .power_plan_controller
            .reconcile_ordinary(decision, Instant::now());
        let status = self.power_plan_controller.status();
        let action_process_name = match (status.decision_state, status.rule_index) {
            (Some(DecisionState::ByFocus), Some(_)) => focus_process_name,
            (Some(DecisionState::ByRunningApp), Some(_)) => running_process_name,
            _ => None,
        }
        .unwrap_or_default();
        let feature = status
            .decision_state
            .and_then(power_plan_action_log_feature);
        let target_guid = status.target_guid.unwrap_or_default();
        let reason = status.reason.unwrap_or_default();
        match result {
            Ok(applied) => {
                let selected_cpu_load_rule_index = primary_cpu_load_rule_index.filter(|index| {
                    status.decision_state == Some(DecisionState::ByCpuLoad)
                        && status.rule_index == Some(*index)
                });
                self.by_cpu_load_scheduler.confirm_applied_rule(
                    selected_cpu_load_rule_index,
                    status.current_guid.as_deref(),
                    Instant::now(),
                );
                if let (true, Some(feature)) = (applied, feature) {
                    self.action_log.record(
                        feature,
                        None,
                        action_process_name,
                        ActionLogResult::Applied,
                        format!("{reason} Applied power plan {target_guid}."),
                    );
                }
                Ok(())
            }
            Err(error) => {
                self.by_cpu_load_scheduler
                    .confirm_applied_rule(None, None, Instant::now());
                if let Some(feature) = feature {
                    self.action_log.record(
                        feature,
                        None,
                        action_process_name,
                        ActionLogResult::Failed,
                        format!("{reason} Power plan switch failed: {error}"),
                    );
                }
                Err(ActivePlanCheckError::Operation(error))
            }
        }
    }

    fn switch_power_plan(
        &mut self,
        settings: &Settings,
        guid: &str,
    ) -> Result<(), RuntimeCommandError> {
        let plans = crate::power::list_plans().map_err(RuntimeCommandError::CommandFailed)?;
        let Some(plan) = plans
            .into_iter()
            .find(|plan| plan.guid.eq_ignore_ascii_case(guid))
        else {
            return Err(RuntimeCommandError::PowerPlanUnavailable);
        };
        if plan.managed_adaptive {
            return Err(ManualPowerPlanError::ManagedTarget.into());
        }
        let now = Instant::now();
        // Save can disable Adaptive Processor Power and submit a staged selection together.
        if !(settings.general.enabled && adaptive_power_plan_required(settings))
            && !self.adaptive_power_plan_waiting_for_source()
            && self.power_plan_controller.has_pending_cleanup()
        {
            self.power_plan_controller
                .release_adaptive(now)
                .map_err(ManualPowerPlanError::SwitchFailed)?;
        }
        let result = self.power_plan_controller.switch_manual(&plan.guid, now);
        if matches!(
            result,
            Ok(true) | Err(ManualPowerPlanError::ActiveRecoveryPending(_))
        ) {
            self.by_cpu_load_scheduler
                .observe_active_plan(Some(&plan.guid));
        }
        result.map(|_| ()).map_err(Into::into)
    }

    pub(super) fn refresh_active_plan(&mut self, force: bool) -> Result<(), ActivePlanCheckError> {
        self.power_plan_controller
            .refresh_active_plan_for_status(Instant::now(), force)
    }

    pub(super) fn refresh_cpu_usage(&mut self) {
        if self.cpu_usage_sampled_at.is_none_or(|sampled_at| {
            Instant::now().saturating_duration_since(sampled_at) >= CPU_USAGE_REFRESH_INTERVAL
        }) {
            let sample = self.cpu_monitor.sample_usage();
            self.record_cpu_usage(sample, Instant::now());
        }
    }

    fn record_cpu_usage(&mut self, sample: CpuUsageSnapshot, sampled_at: Instant) {
        if !sample
            .percent
            .is_some_and(|usage| usage.is_finite() && (0.0..=100.0).contains(&usage))
        {
            self.by_cpu_load_scheduler.reset();
        }
        self.cpu_usage = sample;
        self.cpu_usage_sampled_at = Some(sampled_at);
    }

    pub(super) fn power_plan_status(&mut self) -> (PowerPlanStatus, Option<&PowerPlanRuleContext>) {
        let adaptive_waits_for_source = self.adaptive_power_plan_waiting_for_source();
        // Cleanup can complete between Adaptive Processor Power updates.
        if self.adaptive_processor_power_error.is_some()
            && !self.power_plan_controller.has_pending_cleanup()
            && (adaptive_waits_for_source
                || self.last_settings.as_ref().is_some_and(|observed| {
                    let settings = observed.effective();
                    !feature_refresh_required(settings, adaptive_power_plan_required(settings))
                }))
        {
            self.adaptive_processor_power_error = None;
        }
        let mut status = self.power_plan_controller.status();
        status.adaptive_processor_power_error = self.adaptive_processor_power_error.clone();
        status.adaptive_processor_power_waiting_for_source = adaptive_waits_for_source;
        let stop_state = self.last_settings.as_ref().and_then(|settings| {
            ordinary_power_plan_stop(settings.effective(), settings.plugged_in)
                .map(|(state, _)| state)
        });
        let waits_for_source = self.last_settings.as_ref().is_some_and(|settings| {
            ordinary_power_plan_waits_for_source(&settings.configured, settings.plugged_in)
        });
        if status.owner == Some(PowerPlanOwner::AdaptiveProcessorPower)
            || (!waits_for_source
                && (status.owner != Some(PowerPlanOwner::OrdinaryAutomation)
                    || (status.restoring_previous
                        && (stop_state.is_none() || self.has_temporary_power_plan()))))
        {
            self.power_plan_rule_context = None;
            return (status, None);
        }
        if waits_for_source {
            self.power_plan_rule_context = None;
        }
        let context = self.power_plan_rule_context.as_ref();
        if let Some(decision_state) = waits_for_source
            .then_some(DecisionState::WaitingForObservation)
            .or(stop_state)
            .or_else(|| {
                context
                    .is_none()
                    .then_some(DecisionState::WaitingForObservation)
            })
        {
            // This is a display projection. The controller still owns any pending restoration.
            return (
                PowerPlanStatus {
                    active_plan_read_error: status.active_plan_read_error,
                    adaptive_processor_power_error: status.adaptive_processor_power_error,
                    adaptive_processor_power_waiting_for_source: adaptive_waits_for_source,
                    current_guid: status.current_guid,
                    decision_state: Some(decision_state),
                    ..Default::default()
                },
                None,
            );
        }
        let rule_context = context.filter(|_| status.rule_index.is_some());
        (status, rule_context)
    }

    pub(super) fn clear_ordinary_power_plan_decision(&mut self) {
        self.power_plan_controller.clear_ordinary_decision();
    }
}

fn adaptive_power_launch_eligible(process: &ProcessInfo, creation: u64) -> bool {
    let self_id = current_process_id();
    let Some(session) = process_session_id(self_id) else {
        return false;
    };
    focus_process_priority_eligible(process.id, &process.name, self_id, session)
        && focus_and_launch_profile_eligible(process.id, creation)
}

fn ordinary_power_plan_choice_matches(
    status: &PowerPlanStatus,
    previous: &Settings,
    current: &Settings,
) -> bool {
    let previous_activity = &previous.by_activity;
    let activity = &current.by_activity;
    let same_target = match status.decision_state {
        Some(DecisionState::ByActivityIdle) => {
            activity.power_plans.power_save_guid == previous_activity.power_plans.power_save_guid
        }
        Some(DecisionState::ByActivityActive) => {
            activity.switch_to_performance_on_resume
                && activity.power_plans.performance_guid
                    == previous_activity.power_plans.performance_guid
        }
        _ => return indexed_power_plan_rule_matches(status, previous, current),
    };
    same_target
        && activity.enabled
        && activity.idle_timeout_seconds == previous_activity.idle_timeout_seconds
        && activity.input_detection == previous_activity.input_detection
}

fn indexed_power_plan_rule_matches(
    status: &PowerPlanStatus,
    previous: &Settings,
    current: &Settings,
) -> bool {
    let Some(index) = status.rule_index else {
        return false;
    };
    macro_rules! same_rule {
        ($feature:ident) => {
            current.$feature.enabled
                && current
                    .$feature
                    .rules
                    .get(index)
                    .is_some_and(|rule| previous.$feature.rules.get(index) == Some(rule))
        };
    }
    match status.decision_state {
        Some(DecisionState::ByFocus) => same_rule!(by_focus),
        Some(DecisionState::ByRunningApp) => same_rule!(by_running_app),
        Some(DecisionState::ByCpuLoad) => same_rule!(by_cpu_load),
        Some(DecisionState::ByTime) => same_rule!(by_time),
        _ => false,
    }
}

pub(super) fn power_plan_action_log_feature(state: DecisionState) -> Option<ActionLogFeature> {
    match state {
        DecisionState::ByFocus => Some(ActionLogFeature::ByFocus),
        DecisionState::ByRunningApp => Some(ActionLogFeature::ByRunningApp),
        DecisionState::ByTime => Some(ActionLogFeature::ByTime),
        DecisionState::ByCpuLoad => Some(ActionLogFeature::ByCpuLoad),
        DecisionState::ByActivityIdle | DecisionState::ByActivityActive => {
            Some(ActionLogFeature::ByActivity)
        }
        DecisionState::Disabled
        | DecisionState::PausedWhilePluggedIn
        | DecisionState::WaitingForObservation
        | DecisionState::NoPowerPlanSelected => None,
    }
}

fn collect_restore_error(errors: &mut Vec<String>, feature: &str, error: Option<String>) {
    if let Some(error) = error {
        errors.push(format!("restore {feature}: {error}"));
    }
}

pub(super) fn processor_affinity_hard_settings(settings: &Settings) -> CpuAllocationSettings {
    let mut processor_affinity_hard = settings.processor_affinity_hard.clone();
    processor_affinity_hard.rules.retain(|rule| {
        !settings
            .cpu_sets_soft
            .contains_rule_for(rule.executable_path.trim_start())
    });
    processor_affinity_hard
}

pub(super) fn explicit_cpu_allocation_paths(settings: &Settings) -> Vec<String> {
    [&settings.cpu_sets_soft, &settings.processor_affinity_hard]
        .into_iter()
        .filter(|feature| feature.enabled)
        .flat_map(|feature| &feature.rules)
        .filter(|rule| {
            rule.enabled
                && rule.has_cpu_selection()
                && Path::new(rule.executable_path.trim()).is_absolute()
        })
        .map(|rule| rule.executable_path.trim_start().to_owned())
        .collect()
}

impl Drop for RuntimeCore {
    fn drop(&mut self) {
        let _ = self.shutdown();
        if let Some(shared) = self.action_log_session.take() {
            let mut state = lock_unpoisoned(&shared.state);
            let mut log = std::mem::take(&mut self.action_log);
            if std::mem::take(&mut state.action_log_clear_requested) {
                log.clear();
            } else if log.revision() != self.published_action_log_revision {
                update_action_log(&shared, &mut state, log.entries_snapshot(), log.summaries());
            }
            state.idle_action_log = Some(log);
        }
    }
}

#[cfg(test)]
mod settings_tests {
    use super::*;

    #[test]
    #[allow(clippy::field_reassign_with_default)] // Keep this status-only fixture free of native shutdown.
    fn inactive_adaptive_power_clears_errors_after_the_managed_plan_is_released() {
        const CLEANUP_ERROR: &str = "Adaptive plan cleanup failed";
        const APPLY_ERROR: &str = "Adaptive plan apply failed";
        let profiles = |master, ac, battery| {
            let mut settings = Settings::default();
            settings.general.enabled = master;
            settings.adaptive_engine.adaptive_processor_power_enabled = ac;
            settings
                .battery_profile_mut()
                .adaptive_engine
                .adaptive_processor_power_enabled = battery;
            Arc::new(settings)
        };
        for (plugged_in, master, ac, battery) in [
            (Some(true), true, false, true),
            (Some(false), true, true, false),
            (Some(false), false, true, true),
            (None, true, false, true),
        ] {
            let mut runner = RuntimeCore::default();
            runner.shutdown_started = true;
            runner.last_settings = Some(ObservedSettings {
                configured: profiles(master, ac, battery),
                plugged_in,
            });
            runner.power_plan_controller = PowerPlanController::temporary_adaptive_fixture();
            runner.adaptive_processor_power_error = Some(CLEANUP_ERROR.into());
            assert_eq!(
                runner
                    .power_plan_status()
                    .0
                    .adaptive_processor_power_error
                    .as_deref(),
                Some(CLEANUP_ERROR)
            );

            runner.power_plan_controller = PowerPlanController::default();
            assert!(runner
                .power_plan_status()
                .0
                .adaptive_processor_power_error
                .is_none());
            runner.last_settings = Some(ObservedSettings {
                configured: profiles(true, true, true),
                plugged_in,
            });
            assert!(runner
                .power_plan_status()
                .0
                .adaptive_processor_power_error
                .is_none());

            runner.adaptive_processor_power_error = Some(APPLY_ERROR.into());
            assert_eq!(
                runner
                    .power_plan_status()
                    .0
                    .adaptive_processor_power_error
                    .as_deref(),
                Some(APPLY_ERROR)
            );
        }
    }

    #[test]
    fn power_only_samples_foreground_demand_and_reprimes_on_power_source_change() {
        let mut runner = RuntimeCore::default();
        runner.shutdown_started = true;
        let mut settings = Settings::default();
        settings.adaptive_engine.adaptive_processor_power_enabled = true;
        let _ = settings.battery_profile_mut();
        let settings = Arc::new(settings);
        assert!(!adaptive_engine_process_required(&settings));
        assert!(adaptive_power_plan_required(&settings));
        assert!(runner.note_settings(&settings, AutomationWakeEvents::default(), Some(true)));

        let id = u32::MAX;
        let start = Instant::now();
        let sample = |second, generation, cpu_time_100ns: Option<u64>| {
            CycleObservations::with_adaptive_cpu_sample(
                vec![ProcessInfo {
                    id,
                    name: "csrss.exe".into(),
                    creation_time: Some(generation),
                    parent_id: None,
                    session_id: None,
                    user_name: None,
                    is_service_account: None,
                    is_critical: None,
                    can_set_information: false,
                    image_path: None,
                }],
                Some(id),
                cpu_time_100ns.map(|cpu_time_100ns| crate::cpu::ProcessCpuSample {
                    cpu_time_100ns,
                    sampled_at: start + Duration::from_secs(second),
                }),
            )
        };
        let work = 1_000_000_000_000;
        assert_eq!(
            runner.sample_adaptive_power_foreground(&mut sample(0, 1, Some(0))),
            (true, None)
        );
        assert_eq!(
            runner.sample_adaptive_power_foreground(&mut sample(1, 1, Some(work))),
            (false, Some(100.0))
        );
        assert!(runner.note_settings(&settings, AutomationWakeEvents::default(), Some(false)));
        assert_eq!(
            runner.sample_adaptive_power_foreground(&mut sample(2, 1, Some(work * 2))),
            (false, None)
        );
        assert_eq!(
            runner.sample_adaptive_power_foreground(&mut sample(3, 1, Some(work * 3))),
            (false, Some(100.0))
        );
        for (second, generation, counter, expected) in [
            (4, 1, None, (false, None)),
            (5, 1, Some(work * 5), (false, None)),
            (6, 2, Some(0), (true, None)),
        ] {
            assert_eq!(
                runner.sample_adaptive_power_foreground(&mut sample(second, generation, counter)),
                expected
            );
        }
        let mut missing_root = CycleObservations::with_adaptive_cpu_sample(vec![], Some(id), None);
        assert_eq!(
            runner.sample_adaptive_power_foreground(&mut missing_root),
            (false, None)
        );
        assert_eq!(
            runner.sample_adaptive_power_foreground(&mut sample(7, 2, Some(work))),
            (false, None)
        );
    }

    #[test]
    #[allow(clippy::field_reassign_with_default)] // RuntimeCore implements Drop.
    fn equal_settings_replacement_updates_identity_without_invalidation() {
        let mut runner = RuntimeCore::default();
        runner.shutdown_started = true;
        let settings = Arc::new(Settings::default());
        let replacement = Arc::new(settings.as_ref().clone());
        let events = AutomationWakeEvents::default();
        assert!(runner.note_settings(&settings, events, Some(true)));
        assert!(!runner.note_settings(&replacement, events, Some(true)));
        assert!(runner
            .last_settings
            .as_ref()
            .is_some_and(|observed| Arc::ptr_eq(&observed.configured, &replacement)));
    }

    #[test]
    #[allow(clippy::field_reassign_with_default)] // RuntimeCore implements Drop, preventing struct update.
    fn coalesced_settings_cancel_restoration_even_while_adaptive_checks_are_paused() {
        let mut runner = RuntimeCore::default();
        // The fixture contains synthetic plan IDs and must never run native shutdown.
        runner.shutdown_started = true;
        runner.power_plan_controller = PowerPlanController::temporary_adaptive_fixture();
        let mut settings = Arc::new(Settings::default());
        Arc::make_mut(&mut settings)
            .general
            .restore_previous_power_plan = true;
        assert!(!automation_worker_required(&settings));
        let mut revision = SettingsRevision::initial();
        let runtime = RuntimeHandle::start(&RuntimeSettingsSnapshot {
            value: Arc::clone(&settings),
            runtime_revision: revision,
            persisted_revision: revision,
        });
        let mut save = |value| {
            revision = revision.next();
            runtime.replace_settings(&RuntimeSettingsSnapshot {
                value,
                runtime_revision: revision,
                persisted_revision: revision,
            });
        };
        let mut edited = Arc::clone(&settings);
        Arc::make_mut(&mut edited).general.check_interval_ms += 100;
        save(edited);
        save(Arc::clone(&settings));
        let snapshot = automation_snapshot(&runtime.shared).unwrap();
        runner.note_settings(&snapshot.settings, snapshot.wake_events, Some(true));
        assert!(runner.has_temporary_power_plan());

        let mut disabled = Arc::clone(&settings);
        Arc::make_mut(&mut disabled)
            .general
            .restore_previous_power_plan = false;
        save(disabled);
        save(Arc::clone(&settings));
        let snapshot = automation_snapshot(&runtime.shared).unwrap();
        assert!(snapshot.settings.general.restore_previous_power_plan);
        runner.note_settings(&snapshot.settings, snapshot.wake_events, Some(true));
        assert!(!runner.has_temporary_power_plan());
        assert!(
            !automation_snapshot(&runtime.shared)
                .unwrap()
                .wake_events
                .restore_previous_plan_disabled
        );

        runner.power_plan_rule_context = Some(PowerPlanRuleContext {
            settings: Arc::clone(&settings),
            source: PowerSourceProfile::PluggedIn,
        });
        assert!(runner
            .run_check(
                &settings,
                Some(true),
                Local::now(),
                ActivityState::Unknown,
                &mut CycleObservations::default(),
            )
            .is_ok());
        assert!(runner.power_plan_rule_context.is_none());
        assert!(!runner.has_temporary_power_plan());
        assert!(runner.power_plan_controller.adaptive_active());
        assert!(runner.cpu_usage_sampled_at.is_none());
        runner.power_plan_controller = PowerPlanController::default();
    }

    #[test]
    #[allow(clippy::field_reassign_with_default)] // Keep this context-only fixture free of native shutdown.
    fn power_plan_rule_context_tracks_logical_source_and_decision_release() {
        let mut runner = RuntimeCore::default();
        runner.shutdown_started = true;
        let settings = Arc::new(Settings::default());
        for plugged_in in [None, Some(false)] {
            runner.power_plan_rule_context = Some(PowerPlanRuleContext {
                settings: Arc::clone(&settings),
                source: PowerSourceProfile::PluggedIn,
            });
            runner.note_settings(&settings, AutomationWakeEvents::default(), plugged_in);
            assert_eq!(
                runner.power_plan_rule_context.is_some(),
                plugged_in.is_none()
            );
        }
        runner.power_plan_rule_context = Some(PowerPlanRuleContext {
            settings,
            source: PowerSourceProfile::OnBattery,
        });
        let (_, context) = runner.power_plan_status();
        assert!(context.is_none());
        assert!(runner.power_plan_rule_context.is_none());
    }

    #[test]
    #[allow(clippy::field_reassign_with_default)] // Keep this context-only fixture free of native shutdown.
    fn battery_only_power_plan_rules_wait_for_a_verified_source() {
        let mut settings = Settings::default();
        let battery = settings.battery_profile_mut();
        battery.by_time.enabled = true;
        battery.by_time.rules.push(crate::config::ByTimeRule {
            enabled: true,
            days: vec![crate::config::WeekdaySetting::Mon],
            power_plan_guid: Some("battery-plan".into()),
            ..Default::default()
        });
        let settings = Arc::new(settings);
        let mut runner = RuntimeCore::default();
        runner.shutdown_started = true;
        runner.note_settings(&settings, AutomationWakeEvents::default(), None);
        runner.power_plan_rule_context = Some(PowerPlanRuleContext {
            settings: Arc::clone(&settings),
            source: PowerSourceProfile::PluggedIn,
        });

        assert!(!power_plan_checks_required(&settings));
        assert!(ordinary_power_plan_waits_for_source(&settings, None));
        let (status, context) = runner.power_plan_status();
        assert_eq!(
            status.decision_state,
            Some(DecisionState::WaitingForObservation)
        );
        assert!(context.is_none());
        assert!(runner.power_plan_rule_context.is_none());
        for plugged_in in [Some(false), Some(true)] {
            assert!(!ordinary_power_plan_waits_for_source(&settings, plugged_in));
        }
    }

    #[test]
    fn saved_power_plan_rules_preserve_only_the_matching_index() {
        let mut previous = Settings::default();
        previous.by_time.enabled = true;
        previous.by_time.rules.push(crate::config::ByTimeRule {
            power_plan_guid: Some("original".to_owned()),
            ..Default::default()
        });
        let status = PowerPlanStatus {
            decision_state: Some(DecisionState::ByTime),
            rule_index: Some(1),
            ..Default::default()
        };
        let mut current = previous.clone();
        current.by_time.rules[0].enabled = false;
        assert!(indexed_power_plan_rule_matches(
            &status, &previous, &current
        ));
        current.by_time.rules[1].power_plan_guid = Some("replacement".to_owned());
        assert!(!indexed_power_plan_rule_matches(
            &status, &previous, &current
        ));
        current.by_time.rules = previous.by_time.rules.clone();
        current.by_time.rules.swap(0, 1);
        assert!(!indexed_power_plan_rule_matches(
            &status, &previous, &current
        ));
    }

    #[test]
    fn saved_activity_plan_depends_on_the_selected_slot_and_input_policy() {
        let mut previous = Settings::default();
        previous.by_activity.enabled = true;
        previous.by_activity.power_plans.performance_guid = Some("active".to_owned());
        let status = PowerPlanStatus {
            decision_state: Some(DecisionState::ByActivityActive),
            ..Default::default()
        };
        let mut current = previous.clone();
        current.by_activity.power_plans.power_save_guid = Some("idle".to_owned());
        assert!(ordinary_power_plan_choice_matches(
            &status, &previous, &current
        ));
        current.by_activity.idle_timeout_seconds += 1;
        assert!(!ordinary_power_plan_choice_matches(
            &status, &previous, &current
        ));
    }

    #[test]
    fn cpu_load_source_boundaries_discard_cached_usage_before_zero_duration_rules() {
        use crate::config::{ByCpuLoadRule, CpuLoadPercent, CpuUsageComparison};

        let mut settings = Settings::default();
        settings.by_cpu_load.enabled = true;
        settings.by_cpu_load.rules = vec![ByCpuLoadRule {
            comparison: CpuUsageComparison::AtOrAbove,
            threshold_percent: CpuLoadPercent::new(75).unwrap(),
            duration_seconds: 0,
            power_plan_guid: Some("target".into()),
            ..Default::default()
        }];
        let settings = Arc::new(settings);
        let mut power_changed = AutomationWakeEvents::default();
        power_changed.insert_windows_event(WindowsAutomationEvent::PowerChanged);
        let mut suspend_resume = AutomationWakeEvents::default();
        suspend_resume.insert_windows_event(WindowsAutomationEvent::SuspendResume);
        let settings_changed = AutomationWakeEvents {
            settings_changed: true,
            ..Default::default()
        };
        for (previous_source, next_source, events, discard) in [
            (Some(true), Some(false), power_changed, true),
            (Some(false), Some(true), power_changed, true),
            (Some(true), None, power_changed, true),
            (None, Some(true), power_changed, true),
            (Some(true), Some(true), suspend_resume, true),
            (Some(true), Some(true), power_changed, false),
            (Some(true), Some(true), settings_changed, false),
        ] {
            let mut runner = RuntimeCore::default();
            runner.note_settings(&settings, AutomationWakeEvents::default(), previous_source);
            let start = Instant::now();
            runner.cpu_usage.percent = Some(80.0);
            runner.cpu_usage_sampled_at = Some(start);

            runner.note_settings(&settings, events, next_source);
            let decision = runner.by_cpu_load_scheduler.current_decision_at(
                &settings.by_cpu_load,
                runner.cpu_usage.percent,
                start,
            );
            assert_eq!(decision.is_none(), discard);
            assert_eq!(runner.cpu_usage_sampled_at, (!discard).then_some(start));
        }
    }

    #[test]
    fn power_plan_notifications_preserve_cpu_load_qualification_and_holds() {
        use crate::config::{
            ByCpuLoadRule, ByCpuLoadStability, CpuLoadPercent, CpuUsageComparison,
        };

        for stability in [
            None,
            Some(ByCpuLoadStability {
                exit_threshold_percent: CpuLoadPercent::new(65).unwrap(),
                exit_upper_threshold_percent: None,
                minimum_hold_seconds: 30,
            }),
        ] {
            let mut settings = Settings::default();
            settings.by_cpu_load.enabled = true;
            settings.by_cpu_load.rules = vec![ByCpuLoadRule {
                comparison: CpuUsageComparison::AtOrAbove,
                threshold_percent: CpuLoadPercent::new(75).unwrap(),
                duration_seconds: 2,
                stability: stability.clone(),
                power_plan_guid: Some("target".into()),
                else_enabled: true,
                else_power_plan_guid: Some("return".into()),
                ..Default::default()
            }];
            let settings = Arc::new(settings);
            let mut runner = RuntimeCore::default();
            runner.note_settings(&settings, AutomationWakeEvents::default(), Some(true));
            let start = Instant::now();
            assert!(runner
                .by_cpu_load_scheduler
                .current_decision_at(&settings.by_cpu_load, Some(80.0), start)
                .unwrap()
                .is_else());
            let decision = runner
                .by_cpu_load_scheduler
                .current_decision_at(
                    &settings.by_cpu_load,
                    Some(80.0),
                    start + Duration::from_secs(2),
                )
                .unwrap();
            assert!(!decision.is_else());
            runner.by_cpu_load_scheduler.confirm_applied_rule(
                Some(decision.rule_index),
                Some("target"),
                start + Duration::from_secs(2),
            );

            let mut events = AutomationWakeEvents::default();
            events.insert_windows_event(WindowsAutomationEvent::PowerChanged);
            assert!(!runner.note_settings(&settings, events, Some(true)));
            let usage = if stability.is_some() { 70.0 } else { 80.0 };
            assert!(!runner
                .by_cpu_load_scheduler
                .current_decision_at(
                    &settings.by_cpu_load,
                    Some(usage),
                    start + Duration::from_secs(3),
                )
                .unwrap()
                .is_else());
            if stability.is_some() {
                assert!(!runner
                    .by_cpu_load_scheduler
                    .current_decision_at(
                        &settings.by_cpu_load,
                        Some(20.0),
                        start + Duration::from_secs(4),
                    )
                    .unwrap()
                    .is_else());
                assert!(runner
                    .by_cpu_load_scheduler
                    .current_decision_at(
                        &settings.by_cpu_load,
                        Some(20.0),
                        start + Duration::from_secs(32),
                    )
                    .unwrap()
                    .is_else());
            }
        }
    }

    #[test]
    fn source_transitions_and_suspend_resume_restart_cpu_load_qualification_and_holds() {
        use crate::config::{
            ByCpuLoadRule, ByCpuLoadStability, CpuLoadPercent, CpuUsageComparison,
        };

        for (previous_source, next_source, suspend_resume) in [
            (Some(true), Some(false), false),
            (Some(false), Some(true), false),
            (Some(true), None, false),
            (None, Some(true), false),
            (Some(true), Some(true), true),
        ] {
            for activated in [false, true] {
                let mut settings = Settings::default();
                settings.by_cpu_load.enabled = true;
                settings.by_cpu_load.rules = vec![ByCpuLoadRule {
                    comparison: CpuUsageComparison::AtOrAbove,
                    threshold_percent: CpuLoadPercent::new(75).unwrap(),
                    duration_seconds: 2,
                    stability: Some(ByCpuLoadStability {
                        exit_threshold_percent: CpuLoadPercent::new(65).unwrap(),
                        exit_upper_threshold_percent: None,
                        minimum_hold_seconds: 30,
                    }),
                    power_plan_guid: Some("target".into()),
                    ..Default::default()
                }];
                let settings = Arc::new(settings);
                let mut runner = RuntimeCore::default();
                runner.note_settings(&settings, AutomationWakeEvents::default(), previous_source);
                let start = Instant::now();
                assert!(runner
                    .by_cpu_load_scheduler
                    .current_decision_at(&settings.by_cpu_load, Some(80.0), start)
                    .is_none());
                if activated {
                    let decision = runner
                        .by_cpu_load_scheduler
                        .current_decision_at(
                            &settings.by_cpu_load,
                            Some(80.0),
                            start + Duration::from_secs(2),
                        )
                        .unwrap();
                    runner.by_cpu_load_scheduler.confirm_applied_rule(
                        Some(decision.rule_index),
                        Some("target"),
                        start + Duration::from_secs(2),
                    );
                }

                let mut events = AutomationWakeEvents::default();
                events.insert_windows_event(if suspend_resume {
                    WindowsAutomationEvent::SuspendResume
                } else {
                    WindowsAutomationEvent::PowerChanged
                });
                assert!(events.power_changed);
                assert_eq!(events.suspend_resume, suspend_resume);
                assert!(runner.note_settings(&settings, events, next_source));
                assert!(!runner.note_settings(
                    &settings,
                    AutomationWakeEvents::default(),
                    next_source,
                ));
                assert!(runner
                    .by_cpu_load_scheduler
                    .current_decision_at(
                        &settings.by_cpu_load,
                        Some(if activated { 70.0 } else { 80.0 }),
                        start + Duration::from_secs(3),
                    )
                    .is_none());
            }
        }
    }

    #[test]
    fn cpu_load_qualification_and_hold_restart_after_an_inactive_power_profile() {
        use crate::config::{
            ByCpuLoadRule, ByCpuLoadStability, CpuLoadPercent, CpuUsageComparison,
        };

        let mut settings = Settings::default();
        settings.by_activity.enabled = false;
        settings.by_time.enabled = false;
        settings.by_focus.enabled = false;
        settings.by_running_app.enabled = false;
        settings.by_cpu_load.enabled = true;
        settings.by_cpu_load.rules = vec![ByCpuLoadRule {
            enabled: true,
            name: "Sustained load".into(),
            comparison: CpuUsageComparison::AtOrAbove,
            threshold_percent: CpuLoadPercent::new(75).unwrap(),
            upper_threshold_percent: None,
            duration_seconds: 30,
            stability: Some(ByCpuLoadStability {
                exit_threshold_percent: CpuLoadPercent::new(65).unwrap(),
                exit_upper_threshold_percent: None,
                minimum_hold_seconds: 30,
            }),
            power_plan_guid: Some("high-load-plan".into()),
            else_enabled: false,
            else_power_plan_guid: None,
        }];
        settings.battery_profile_mut().by_cpu_load.enabled = false;
        assert!(automation_worker_required(&settings));
        assert!(!power_plan_checks_required(settings.battery_profile()));

        let settings = Arc::new(settings);
        let mut runner = RuntimeCore::default();
        let start = Instant::now();
        runner.note_settings(&settings, AutomationWakeEvents::default(), Some(true));
        assert!(runner
            .by_cpu_load_scheduler
            .current_decision_at(&settings.by_cpu_load, Some(80.0), start)
            .is_none());
        runner.note_settings(&settings, AutomationWakeEvents::default(), Some(false));
        // The idle Battery profile keeps the worker alive without calling the scheduler.
        runner.note_settings(&settings, AutomationWakeEvents::default(), Some(true));
        assert!(runner
            .by_cpu_load_scheduler
            .current_decision_at(
                &settings.by_cpu_load,
                Some(80.0),
                start + Duration::from_secs(60),
            )
            .is_none());
        assert!(!runner.note_settings(&settings, AutomationWakeEvents::default(), Some(true)));
        let decision = runner
            .by_cpu_load_scheduler
            .current_decision_at(
                &settings.by_cpu_load,
                Some(80.0),
                start + Duration::from_secs(90),
            )
            .unwrap();
        assert_eq!(decision.power_plan_guid, "high-load-plan");
        runner.by_cpu_load_scheduler.confirm_applied_rule(
            Some(decision.rule_index),
            Some("high-load-plan"),
            start + Duration::from_secs(90),
        );
        assert!(runner
            .by_cpu_load_scheduler
            .current_decision_at(
                &settings.by_cpu_load,
                Some(20.0),
                start + Duration::from_secs(91),
            )
            .is_some());
        runner.note_settings(&settings, AutomationWakeEvents::default(), Some(false));
        runner.note_settings(&settings, AutomationWakeEvents::default(), Some(true));
        assert!(runner
            .by_cpu_load_scheduler
            .current_decision_at(
                &settings.by_cpu_load,
                Some(20.0),
                start + Duration::from_secs(92),
            )
            .is_none());
    }

    #[test]
    fn cpu_load_resumes_with_a_fresh_sample_only_after_all_consumers_yield() {
        for (plugged_in, paused, adaptive_enabled, processor_power_enabled) in [
            (Some(true), false, false, false),
            (Some(true), false, true, false),
            (Some(true), true, false, false),
            (Some(true), true, true, false),
            (Some(true), true, false, true),
            (None, true, false, false),
        ] {
            let mut runner = RuntimeCore::default();
            let mut settings = Settings::default();
            settings.general.enabled = true;
            settings.general.pause_power_plan_switching_while_plugged_in = paused;
            settings.adaptive_engine.enabled = adaptive_enabled;
            settings.adaptive_engine.adaptive_processor_power_enabled = processor_power_enabled;
            settings.by_cpu_load.rules = vec![crate::config::ByCpuLoadRule {
                power_plan_guid: Some("target".into()),
                ..Default::default()
            }];
            settings.by_cpu_load.enabled = paused;
            let mut settings = Arc::new(settings);
            runner.note_settings(&settings, AutomationWakeEvents::default(), plugged_in);

            let sampled_at = Instant::now();
            runner.cpu_usage.percent = Some(90.0);
            runner.cpu_usage_sampled_at = Some(sampled_at);

            let edited = Arc::make_mut(&mut settings);
            edited.by_cpu_load.enabled = true;
            edited.general.pause_power_plan_switching_while_plugged_in = false;
            runner.note_settings(&settings, AutomationWakeEvents::default(), plugged_in);
            let shared = adaptive_enabled || processor_power_enabled;
            assert_eq!(runner.cpu_usage.percent, shared.then_some(90.0));
            assert_eq!(runner.cpu_usage_sampled_at, shared.then_some(sampled_at));
        }
    }

    #[test]
    fn incidental_cpu_usage_gaps_reset_by_cpu_load_qualification() {
        use crate::config::{ByCpuLoadRule, CpuLoadPercent, CpuUsageComparison};

        let mut settings = Settings::default().by_cpu_load;
        settings.enabled = true;
        settings.rules = vec![ByCpuLoadRule {
            comparison: CpuUsageComparison::AtOrAbove,
            threshold_percent: CpuLoadPercent::new(75).unwrap(),
            duration_seconds: 8,
            power_plan_guid: Some("target".into()),
            ..Default::default()
        }];
        let mut runner = RuntimeCore::default();
        let start = Instant::now();
        let hot = CpuUsageSnapshot {
            percent: Some(85.0),
            ..Default::default()
        };
        let check = |runner: &mut RuntimeCore, now| {
            runner.by_cpu_load_scheduler.current_decision_for_sample_at(
                &settings,
                runner.cpu_usage.percent.zip(runner.cpu_usage_sampled_at),
                now,
            )
        };
        runner.record_cpu_usage(hot, start);
        assert!(check(&mut runner, start).is_none());
        runner.record_cpu_usage(CpuUsageSnapshot::default(), start + Duration::from_secs(5));
        let next_check = start + Duration::from_secs(10);
        runner.record_cpu_usage(hot, next_check);
        assert!(check(&mut runner, next_check).is_none());
    }

    #[test]
    fn explicit_settings_events_do_not_skip_bookkeeping() {
        for explicit in [false, true] {
            for changed in [false, true] {
                let mut runner = RuntimeCore::default();
                let mut settings = Settings::default();
                settings.advanced.action_log_mode = crate::config::ActionLogMode::Off;
                let mut settings = Arc::new(settings);
                runner.note_settings(&settings, AutomationWakeEvents::default(), Some(true));
                if changed {
                    let settings = Arc::make_mut(&mut settings);
                    settings.advanced.action_log_mode = crate::config::ActionLogMode::Full;
                    settings.general.enabled = !settings.general.enabled;
                }
                assert_eq!(
                    runner.note_settings(
                        &settings,
                        AutomationWakeEvents {
                            settings_changed: explicit,
                            ..Default::default()
                        },
                        Some(true),
                    ),
                    changed || explicit
                );
                assert_eq!(
                    runner
                        .last_settings
                        .as_ref()
                        .map(ObservedSettings::effective),
                    Some(settings.as_ref())
                );
                runner.action_log.record(
                    crate::action_log::ActionLogFeature::ThreadPriority,
                    None,
                    "",
                    crate::action_log::ActionLogResult::Applied,
                    "test",
                );
                assert_eq!(runner.action_log.entries().len(), usize::from(changed));
                assert!(!runner.note_settings(
                    &settings,
                    AutomationWakeEvents::default(),
                    Some(true),
                ));
            }
        }
    }

    #[test]
    fn activity_classification_and_deadline_use_only_selected_sources() {
        let start = Instant::now();
        let now = start + Duration::from_secs(60);
        let mut settings = Settings::default();
        settings.by_activity.enabled = true;
        settings.by_activity.idle_timeout_seconds = 30;
        settings.by_activity.power_plans.power_save_guid = Some("idle".into());
        settings.by_activity.switch_to_performance_on_resume = false;
        assert_ne!(input_hook_config(&settings), InputHookConfig::default());

        for selected in 1..8 {
            settings.by_activity.input_detection.keyboard = selected & 1 != 0;
            settings.by_activity.input_detection.mouse = selected & 2 != 0;
            settings.by_activity.input_detection.controller = selected & 4 != 0;
            let mut runner = RuntimeCore::default();
            runner.input_activity.configure(
                InputHookConfig {
                    keyboard: true,
                    mouse: true,
                },
                start,
            );
            runner.controller_observed_since = Some(start);
            // Recent events from disabled sources must not postpone idle.
            runner.input_activity.record(
                InputHookEvents {
                    keyboard: selected & 1 == 0,
                    mouse: selected & 2 == 0,
                    ..Default::default()
                },
                now,
            );
            let snapshot = runner.activity_snapshot(&settings, now);
            assert_eq!(snapshot.state, crate::activity::ActivityState::Idle);
            assert_eq!(snapshot.idle_for, Some(Duration::from_secs(60)));
            assert_eq!(
                activity_idle_check_delay(&settings, snapshot.idle_for),
                None
            );

            if selected & 3 != 0 {
                runner.input_activity.record(
                    InputHookEvents {
                        keyboard: selected & 1 != 0,
                        mouse: selected & 2 != 0,
                        ..Default::default()
                    },
                    now - Duration::from_secs(5),
                );
                let snapshot = runner.activity_snapshot(&settings, now);
                assert_eq!(snapshot.state, crate::activity::ActivityState::Active);
                assert_eq!(snapshot.idle_for, Some(Duration::from_secs(5)));
                assert_eq!(
                    activity_idle_check_delay(&settings, snapshot.idle_for),
                    Some(Duration::from_secs(25))
                );
            }

            runner.input_activity.configure(
                InputHookConfig {
                    keyboard: true,
                    mouse: true,
                },
                now,
            );
            runner.controller_observed_since = Some(now);
            let snapshot = runner.activity_snapshot(&settings, now);
            assert_eq!(snapshot.state, crate::activity::ActivityState::Active);
            assert_eq!(
                activity_idle_check_delay(&settings, snapshot.idle_for),
                Some(Duration::from_secs(30))
            );
        }

        let runner = RuntimeCore::default();
        let snapshot = runner.activity_snapshot(&settings, now);
        assert_eq!(snapshot.state, crate::activity::ActivityState::Unknown);
        assert_eq!(
            activity_idle_check_delay(&settings, snapshot.idle_for),
            Some(configured_check_interval(&settings))
        );
    }
}

#[cfg(test)]
mod watchdog_action_tests {
    use super::*;
    use crate::{control::process_restart::RestartResult, foreground::ProcessActionTarget};

    #[test]
    fn skipped_termination_reports_only_a_successful_restart_of_the_same_instances() {
        let target = |id| ProcessActionTarget {
            id,
            name: "app.exe".into(),
            executable_path: r"C:\Apps\app.exe".into(),
            creation_time: u64::from(id),
            session_id: Some(1),
            is_service_account: Some(false),
        };
        let targets = [target(1), target(2)];
        let now = Instant::now();
        for result in [Ok(()), Err("Replacement could not start.".to_owned())] {
            let mut cycle = WatchdogActionCycle::default();
            assert_eq!(cycle.skipped_termination_reason(&targets), None);
            let expected = if result.is_ok() {
                "watchdog.already_restarted"
            } else {
                "watchdog.termination_already_handled"
            };
            let _ = cycle.restart_once(&targets, &mut Vec::new(), now, || RestartResult {
                termination_handled: vec![(1, 1), (2, 2)],
                result,
            });
            assert_eq!(cycle.skipped_termination_reason(&targets), Some(expected));
            assert_eq!(
                cycle.skipped_termination_reason(&[target(2), target(1)]),
                Some(expected)
            );
            let mut replacement = target(1);
            replacement.creation_time += 1;
            assert_eq!(cycle.skipped_termination_reason(&[replacement]), None);
        }
        let mut cycle = WatchdogActionCycle::default();
        let _ = cycle.restart_once(&[target(3)], &mut Vec::new(), now, || RestartResult {
            termination_handled: vec![(3, 3)],
            result: Ok(()),
        });
        cycle.termination_handled.insert((1, 1));
        assert_eq!(cycle.skipped_termination_reason(&targets), None);
        cycle.termination_handled.insert((2, 2));
        assert_eq!(
            cycle.skipped_termination_reason(&targets),
            Some("watchdog.termination_already_handled")
        );
        assert_eq!(cycle.skipped_termination_reason(&[]), None);
    }

    #[test]
    fn restarts_share_an_attempt_across_monitors_and_cool_down_across_replacements() {
        for result in [
            Ok(()),
            Err("Stopped one process; another was denied.".to_owned()),
        ] {
            let mut cycle = WatchdogActionCycle::default();
            let target = |id| ProcessActionTarget {
                id,
                name: "app.exe".into(),
                executable_path: r"C:\Apps\app.exe".into(),
                creation_time: u64::from(id),
                session_id: Some(1),
                is_service_account: Some(false),
            };
            let targets = [target(1), target(2)];
            let start = Instant::now();
            let mut cooldowns = Vec::new();
            let mut launches = 0;
            for (batch, success) in [
                (targets.to_vec(), WatchdogRestartOutcome::Applied),
                (
                    targets.into_iter().rev().collect(),
                    WatchdogRestartOutcome::AlreadyRestarted,
                ),
            ] {
                assert_eq!(
                    cycle.restart_once(&batch, &mut cooldowns, start, || {
                        launches += 1;
                        RestartResult {
                            termination_handled: vec![(1, 1)],
                            result: result.clone(),
                        }
                    }),
                    result.clone().map(|()| success)
                );
            }
            assert_eq!(launches, 1);
            assert_eq!(cycle.termination_handled, BTreeSet::from([(1, 1)]));
            let mut replacement = target(3);
            replacement.executable_path = "c:/apps/APP.exe".into();
            let mut next_cycle = WatchdogActionCycle::default();
            let just_before_expiry = start + WATCHDOG_RESTART_COOLDOWN - Duration::from_millis(1);
            assert_eq!(
                next_cycle.restart_once(
                    std::slice::from_ref(&replacement),
                    &mut cooldowns,
                    just_before_expiry,
                    || panic!("Cooldown must not attempt a restart"),
                ),
                Ok(WatchdogRestartOutcome::CoolingDown)
            );
            assert_eq!(
                next_cycle.skipped_termination_reason(std::slice::from_ref(&replacement)),
                None
            );

            let mut independent = target(4);
            independent.executable_path = r"C:\Other\app.exe".into();
            assert_eq!(
                next_cycle.restart_once(&[independent], &mut cooldowns, just_before_expiry, || {
                    RestartResult {
                        termination_handled: Vec::new(),
                        result: Ok(()),
                    }
                },),
                Ok(WatchdogRestartOutcome::Applied)
            );
            assert_eq!(
                WatchdogActionCycle::default().restart_once(
                    &[replacement],
                    &mut cooldowns,
                    start + WATCHDOG_RESTART_COOLDOWN,
                    || RestartResult {
                        termination_handled: vec![(3, 3)],
                        result: Ok(())
                    },
                ),
                Ok(WatchdogRestartOutcome::Applied)
            );
        }
    }
}
