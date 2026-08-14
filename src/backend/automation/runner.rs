use super::*;
use crate::action_log::{ActionLogFeature, ActionLogResult};
use crate::runtime::observations::CycleObservations;

pub(super) fn adaptive_power_plan_required(settings: &Settings) -> bool {
    settings.adaptive_engine.enabled && settings.adaptive_engine.processor_policy_enabled
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

#[derive(Default)]
pub(super) struct RuntimeCore {
    shutdown_started: bool,
    last_settings: Option<Settings>,
    power_plan_controller: PowerPlanController,
    cpu_usage: CpuUsageSnapshot,
    next_cpu_usage_refresh: Option<Instant>,
    cpu_monitor: CpuUsageMonitor,
    per_processor_cpu_monitor: PerProcessorUsageMonitor,
    io_monitor: IoUsageMonitor,
    adaptive_processor_topology: Vec<LogicalProcessorInfo>,
    adaptive_io_usage: IoUsageSnapshot,
    next_adaptive_io_refresh: Option<Instant>,
    adaptive_foreground_process_id: Option<u32>,
    idle_detector: IdleDetector,
    controller_activity_detector: ControllerActivityDetector,
    by_cpu_load_scheduler: ByCpuLoadScheduler,
    background_efficiency_manager: BackgroundEfficiencyManager,
    pub(super) app_suspension_manager: AppSuspensionManager,
    pub(super) app_suspension_controller: SuspensionController,
    last_app_suspension_shell_user_intent: Option<Instant>,
    cpu_sets_soft_manager: CpuAllocationManager,
    processor_affinity_hard_manager: CpuAllocationManager,
    cpu_allocation_coordinator: CpuAllocationCoordinator,
    core_limiter_manager: CoreLimiterManager,
    pub(super) by_running_app_manager: ByRunningAppManager,
    pub(super) action_log: ActionLog,
    workload_engine_manager: WorkloadEngineManager,
    launch_boost_active: bool,
    workload_engine_active: bool,
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
    process_termination_controller: ProcessTerminationController,
    timer_resolution_manager: TimerResolutionManager,
    timer_resolution_controller: TimerResolutionController,
    pub(super) known_process_ids: BTreeSet<u32>,
    published_action_log_revision: u64,
}

#[derive(Default)]
pub(super) struct ProcessControlCommandStatuses {
    pub(super) memory_trim: Option<MemoryTrimSnapshot>,
    pub(super) app_suspension: Option<AppSuspensionSnapshot>,
}

impl RuntimeCore {
    pub(super) fn shutdown(&mut self) -> Result<(), String> {
        if self.shutdown_started {
            return Ok(());
        }
        self.shutdown_started = true;

        let mut errors = Vec::<String>::new();
        let mut settings = self.last_settings.clone().unwrap_or_default();
        settings.general.enabled = false;
        let mut observations = CycleObservations::default();

        // Restore in the reverse order used by the automation loop. Several features can touch
        // the same process state, so relying on field drop order can restore an intermediate
        // Winderust-managed value instead of the value that preceded Winderust.
        collect_restore_error(
            &mut errors,
            "Timer Resolution",
            self.run_timer_resolution_update(&settings, &mut observations)
                .last_error,
        );
        if let Err(error) = self.timer_resolution_controller.shutdown() {
            errors.push(format!("Timer Resolution restoration failed: {error}"));
        }
        self.run_by_running_app_update(&settings, &mut observations);
        collect_restore_error(
            &mut errors,
            "Core Limiter",
            self.run_core_limiter_update(&settings, &mut observations)
                .last_error,
        );
        collect_restore_error(
            &mut errors,
            "Processor Affinity (Hard)",
            self.run_processor_affinity_hard_update(&settings, &mut observations)
                .last_error,
        );
        collect_restore_error(
            &mut errors,
            "CPU Sets (Soft)",
            self.run_cpu_sets_soft_update(&settings, &mut observations)
                .last_error,
        );
        collect_restore_error(
            &mut errors,
            "App Suspension",
            self.run_app_suspension_update(&settings, &[], &mut observations)
                .last_error,
        );
        if let Err(error) = self
            .app_suspension_manager
            .shutdown(&mut self.app_suspension_controller, &mut self.action_log)
        {
            errors.push(format!("App Suspension restoration failed: {error}"));
        }
        collect_restore_error(
            &mut errors,
            "Memory Priority",
            self.run_memory_priority_update(&settings, &mut observations)
                .last_error,
        );
        if let Err(error) = self.memory_priority_controller.shutdown() {
            errors.push(format!("Memory Priority restoration failed: {error}"));
        }
        collect_restore_error(
            &mut errors,
            "GPU Priority",
            self.run_gpu_priority_update(&settings, &mut observations)
                .last_error,
        );
        if let Err(error) = self.gpu_priority_controller.shutdown() {
            errors.push(format!("GPU Priority restoration failed: {error}"));
        }
        collect_restore_error(
            &mut errors,
            "Dynamic Priority Boost",
            self.run_dynamic_priority_boost_update(&settings, &mut observations)
                .last_error,
        );
        if let Err(error) = self.dynamic_priority_boost_controller.shutdown() {
            errors.push(format!(
                "Dynamic Priority Boost restoration failed: {error}"
            ));
        }
        collect_restore_error(
            &mut errors,
            "Thread Priority",
            self.run_thread_priority_update(&settings, &mut observations)
                .last_error,
        );
        if let Err(error) = self.thread_priority_controller.shutdown() {
            errors.push(format!("Thread Priority restoration failed: {error}"));
        }
        collect_restore_error(
            &mut errors,
            "Process Priority",
            self.run_process_priority_update(&settings, &mut observations)
                .last_error,
        );
        collect_restore_error(
            &mut errors,
            "I/O Priority",
            self.run_io_priority_update(&settings, &mut observations)
                .last_error,
        );
        if let Err(error) = self.io_priority_controller.shutdown() {
            errors.push(format!("I/O Priority restoration failed: {error}"));
        }
        collect_restore_error(
            &mut errors,
            "Workload Engine",
            self.run_workload_engine_update(&settings, &mut observations)
                .last_error,
        );
        if let Err(error) = self.cpu_allocation_coordinator.shutdown() {
            errors.push(format!("CPU allocation restoration failed: {error}"));
        }
        collect_restore_error(
            &mut errors,
            "Background Efficiency",
            self.run_background_efficiency_update(&settings, &mut observations)
                .last_error,
        );
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

    pub(super) fn note_settings(&mut self, settings: &Settings) -> bool {
        self.action_log.set_mode(settings.advanced.action_log_mode);
        set_execution_failure_suppression_threshold(
            settings.advanced.execution_failure_suppression_threshold(),
        );

        let changed = self.last_settings.as_ref() != Some(settings);
        if changed {
            self.last_settings = Some(settings.clone());
            self.power_plan_controller.clear_failures();
        }
        changed
    }

    pub(super) fn detect_process_appearance(
        &mut self,
        observations: &mut CycleObservations,
    ) -> bool {
        let Ok(processes) = observations.processes() else {
            return false;
        };
        let current_ids = processes
            .iter()
            .filter_map(|process| (process.id != 0).then_some(process.id))
            .collect::<BTreeSet<_>>();

        process_ids_have_new_entries(&mut self.known_process_ids, current_ids)
    }

    pub(super) fn poll_controller_activity(&mut self, now: Instant) -> bool {
        self.controller_activity_detector.poll(now)
    }

    pub(super) fn clear_controller_activity(&mut self) {
        self.controller_activity_detector.clear();
    }

    pub(super) fn publish_action_log_if_changed(&mut self, shared: &SharedAutomationState) {
        let revision = self.action_log.revision();
        if self.published_action_log_revision == revision {
            return;
        }

        update_action_log(
            shared,
            self.action_log.entries(),
            self.action_log.summaries(),
        );
        self.published_action_log_revision = revision;
    }

    pub(super) fn activity_snapshot(
        &self,
        settings: &Settings,
        now: Instant,
    ) -> crate::activity::ActivitySnapshot {
        let idle_timeout = Duration::from_secs(settings.by_activity.idle_timeout_seconds);
        let snapshot = self.idle_detector.snapshot(idle_timeout);
        let controller_idle_for = settings
            .by_activity
            .input_detection
            .controller
            .then(|| self.controller_activity_detector.idle_for(now))
            .flatten();

        merge_activity_snapshot(snapshot, controller_idle_for, idle_timeout)
    }

    pub(super) fn run_background_efficiency_update(
        &mut self,
        settings: &Settings,
        observations: &mut CycleObservations,
    ) -> BackgroundEfficiencySnapshot {
        let foreground_process_id = observations.foreground_process_id();
        let background_efficiency = settings.background_efficiency.clone();
        self.background_efficiency_manager.update(
            &mut self.priority_efficiency_controller,
            &background_efficiency,
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
        manual_freeze_processes: &[String],
        observations: &mut CycleObservations,
    ) -> AppSuspensionSnapshot {
        let foreground_process_id = observations.foreground_process_id();
        self.app_suspension_manager.update(
            &mut self.app_suspension_controller,
            &settings.app_suspension,
            settings.general.enabled,
            settings.general.allow_cross_session_process_control,
            foreground_process_id,
            manual_freeze_processes,
            observations,
            &mut self.action_log,
        )
    }

    pub(super) fn run_app_suspension_foreground_release(
        &mut self,
        observations: &mut CycleObservations,
    ) -> Option<AppSuspensionSnapshot> {
        let now = Instant::now();
        if shell_window_mouse_pressed() && self.app_suspension_shell_user_intent_due(now) {
            self.last_app_suspension_shell_user_intent = Some(now);
            if let Some(status) = self
                .app_suspension_manager
                .release_all_suspended_processes_for_user_intent(
                    &mut self.app_suspension_controller,
                    &mut self.action_log,
                )
            {
                return Some(status);
            }
        }

        let foreground_process_id = observations.foreground_process_id();
        let foreground_process = observations.foreground_process();
        if let Some(status) = foreground_process_id.and_then(|process_id| {
            self.app_suspension_manager.release_interactive_process(
                &mut self.app_suspension_controller,
                process_id,
                foreground_process
                    .as_ref()
                    .filter(|process| process.id == process_id)
                    .map(|process| process.executable_path.as_path()),
                &mut self.action_log,
            )
        }) {
            return Some(status);
        }

        let cursor_process_id = cursor_process_id()?;
        if foreground_process_id == Some(cursor_process_id) {
            return None;
        }
        let cursor_process = cursor_process();
        self.app_suspension_manager.release_interactive_process(
            &mut self.app_suspension_controller,
            cursor_process_id,
            cursor_process
                .as_ref()
                .filter(|process| process.id == cursor_process_id)
                .map(|process| process.executable_path.as_path()),
            &mut self.action_log,
        )
    }

    pub(super) fn run_app_suspension_app_switch_release(
        &mut self,
        observations: &mut CycleObservations,
    ) -> Option<AppSuspensionSnapshot> {
        self.app_suspension_manager
            .release_window_owner_processes_for_user_intent(
                &mut self.app_suspension_controller,
                observations.top_level_window_process_ids().as_ref(),
                &mut self.action_log,
            )
    }

    pub(super) fn run_app_suspension_shell_click_release(
        &mut self,
    ) -> Option<AppSuspensionSnapshot> {
        if !cursor_is_shell_window() {
            return None;
        }

        self.app_suspension_manager
            .release_all_suspended_processes_for_user_intent(
                &mut self.app_suspension_controller,
                &mut self.action_log,
            )
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

    pub(super) fn run_core_limiter_update(
        &mut self,
        settings: &Settings,
        observations: &mut CycleObservations,
    ) -> CoreLimiterSnapshot {
        let foreground_process_id = observations.foreground_process_id();
        self.core_limiter_manager.update(
            &mut self.cpu_allocation_coordinator,
            &settings.core_limiter,
            settings.general.enabled,
            settings.general.allow_cross_session_process_control,
            foreground_process_id,
            observations,
            &mut self.action_log,
        )
    }

    pub(super) fn run_cpu_allocation_reconciliation(
        &mut self,
        settings: &Settings,
        include_release_retries: bool,
    ) {
        let summary = self.cpu_allocation_coordinator.reconcile_pending(
            settings.general.allow_cross_session_process_control,
            include_release_retries,
        );
        record_cpu_allocation_reconciliation(summary, &mut self.action_log);
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
    ) -> ByRunningAppSnapshot {
        self.by_running_app_manager.update(
            &settings.by_running_app,
            settings.general.enabled,
            observations,
        )
    }

    pub(super) fn run_workload_engine_update(
        &mut self,
        settings: &Settings,
        observations: &mut CycleObservations,
    ) -> WorkloadEngineSnapshot {
        self.refresh_cpu_usage();
        let foreground_process_id = observations.foreground_process_id();
        let mut workload_settings = settings.workload_engine.clone();
        workload_settings.enabled &= settings.adaptive_engine.enabled;
        let mut excluded_process_ids = self
            .priority_efficiency_controller
            .policy_target_process_ids(&[ControlOwner::BackgroundEfficiency]);
        excluded_process_ids.extend(self.by_running_app_manager.active_process_ids());
        let explicit_cpu_allocation_paths = explicit_cpu_allocation_paths(settings);
        let mut snapshot = self.workload_engine_manager.update(
            WorkloadEngineUpdate {
                settings: &workload_settings,
                automation_enabled: settings.general.enabled,
                allow_cross_session_process_control: settings
                    .general
                    .allow_cross_session_process_control,
                protect_foreground_app_from_efficiency: settings
                    .workload_engine
                    .workload_engine_foreground_detection_enabled
                    && !settings
                        .workload_engine
                        .workload_engine_foreground_efficiency_mode,
                protect_visible_window_apps_from_efficiency: settings
                    .workload_engine
                    .workload_engine_visible_window_detection_enabled
                    && !settings
                        .workload_engine
                        .workload_engine_visible_window_efficiency_mode,
                foreground_process_id,
                total_cpu_usage_percent: self.cpu_usage.percent,
                background_efficiency_managed: settings.background_efficiency.enabled,
                excluded_process_ids: &excluded_process_ids,
                explicit_cpu_allocation_paths: &explicit_cpu_allocation_paths,
                observations,
            },
            &mut self.cpu_allocation_coordinator,
            &mut self.priority_efficiency_controller,
            &mut self.memory_priority_controller,
            &mut self.action_log,
        );
        self.launch_boost_active = snapshot.launch_boost_active;
        self.workload_engine_active = snapshot.workload_engine_active;
        if let Err(error) =
            self.sync_processor_power_policy(settings, &mut snapshot, foreground_process_id)
        {
            snapshot.adaptive_power_profile = None;
            if snapshot.last_error.is_none() {
                snapshot.last_error = Some(error);
            }
        }
        snapshot
    }

    pub(super) fn sync_processor_power_policy(
        &mut self,
        settings: &Settings,
        snapshot: &mut WorkloadEngineSnapshot,
        foreground_process_id: Option<u32>,
    ) -> Result<(), String> {
        if adaptive_power_plan_required(settings) && settings.general.enabled {
            let foreground_changed = foreground_process_id.is_some()
                && self.adaptive_foreground_process_id != foreground_process_id;
            self.adaptive_foreground_process_id = foreground_process_id;
            self.update_adaptive_power_plan(
                snapshot,
                settings
                    .adaptive_engine
                    .processor_policy_values
                    .normalized(),
                foreground_changed,
            )
        } else {
            self.adaptive_foreground_process_id = None;
            self.power_plan_controller.release_adaptive(Instant::now())
        }
    }

    pub(super) fn update_adaptive_power_plan(
        &mut self,
        snapshot: &mut WorkloadEngineSnapshot,
        baseline: ProcessorPowerValues,
        foreground_changed: bool,
    ) -> Result<(), String> {
        let now = Instant::now();
        if self
            .next_adaptive_io_refresh
            .is_none_or(|refresh_at| now >= refresh_at)
        {
            self.adaptive_io_usage = self.io_monitor.sample();
            self.next_adaptive_io_refresh = Some(now + ADAPTIVE_IO_REFRESH_INTERVAL);
        }
        let io_usage = self.adaptive_io_usage;
        if self.adaptive_processor_topology.is_empty() {
            self.adaptive_processor_topology = cpu_allocation::logical_processors();
        }
        let processor_demand = self
            .per_processor_cpu_monitor
            .sample()
            .map(|usage| adaptive_processor_demand(&usage, &self.adaptive_processor_topology))
            .unwrap_or_default();
        let desired_profile = AdaptivePowerProfile::for_demand(AdaptivePowerDemand {
            launch_boost: snapshot.launch_boost_active || foreground_changed,
            workload_active: snapshot.workload_engine_active,
            total_cpu_percent: self.cpu_usage.percent,
            peak_cpu_percent: processor_demand.peak_cpu_percent,
            performance_peak_cpu_percent: processor_demand.performance_peak_cpu_percent,
            efficiency_peak_cpu_percent: processor_demand.efficiency_peak_cpu_percent,
            foreground_cpu_percent: snapshot
                .workload_engine_total_cpu_usage_tenths
                .map(|usage| f32::from(usage) / 10.0),
            io_bytes_per_second: io_usage.bytes_per_second,
        });
        let has_efficiency_cores = self
            .adaptive_processor_topology
            .iter()
            .any(|processor| processor.kind == LogicalProcessorKind::Efficiency);

        let profile = self.power_plan_controller.reconcile_adaptive(
            AdaptivePowerPlanRequest {
                profile: desired_profile,
                baseline,
                has_efficiency_cores,
            },
            now,
        )?;
        snapshot.adaptive_power_profile = Some(profile.label().to_owned());
        Ok(())
    }

    pub(super) fn run_io_priority_update(
        &mut self,
        settings: &Settings,
        observations: &mut CycleObservations,
    ) -> IoPrioritySnapshot {
        let io_priority_settings =
            effective_io_priority_settings(settings, self.workload_engine_active);
        let owner = io_priority_control_owner(settings, self.workload_engine_active);
        self.io_priority_manager.update(
            &mut self.io_priority_controller,
            owner,
            &io_priority_settings,
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
        let excluded_process_ids = self
            .priority_efficiency_controller
            .policy_target_process_ids(&[
                ControlOwner::BackgroundEfficiency,
                ControlOwner::AdaptiveEngine,
                ControlOwner::WorkloadForegroundBoost,
            ]);
        self.process_priority_manager.update(
            &mut self.priority_efficiency_controller,
            &settings.process_priority,
            settings.general.enabled,
            settings.general.allow_cross_session_process_control,
            observations.foreground_process_id(),
            &excluded_process_ids,
            observations,
            &mut self.action_log,
        )
    }

    pub(super) fn run_thread_priority_update(
        &mut self,
        settings: &Settings,
        observations: &mut CycleObservations,
    ) -> ThreadPrioritySnapshot {
        let thread_priority_settings =
            effective_thread_priority_settings(settings, self.workload_engine_active);
        let owner = thread_priority_control_owner(settings, self.workload_engine_active);
        self.thread_priority_manager.update(
            &mut self.thread_priority_controller,
            owner,
            &thread_priority_settings,
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
        let dynamic_priority_boost_settings =
            effective_dynamic_priority_boost_settings(settings, self.workload_engine_active);
        let owner = dynamic_priority_boost_control_owner(settings, self.workload_engine_active);
        self.dynamic_priority_boost_manager.update(
            &mut self.dynamic_priority_boost_controller,
            owner,
            &dynamic_priority_boost_settings,
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
                    let results = targets
                        .into_iter()
                        .map(|target| {
                            let target = target.map_err(|error| error.to_string())?;
                            self.app_suspension_manager.apply_manual_process_action(
                                &mut self.app_suspension_controller,
                                &target,
                                suspend,
                                settings.general.allow_cross_session_process_control,
                                &mut self.action_log,
                            )
                        })
                        .collect();
                    let _ = result.try_send(Ok(ProcessControlBatchResult { results }));
                }
                ProcessControlCommand::AppSuspensionFreezePath {
                    executable_path,
                    result,
                } => {
                    let status = self.run_app_suspension_update(
                        settings,
                        std::slice::from_ref(&executable_path),
                        observations,
                    );
                    let reply = self
                        .app_suspension_manager
                        .manual_freeze_result(&executable_path)
                        .map(|()| status.clone())
                        .map_err(|error| {
                            RuntimeCommandError::CommandFailed(
                                if !status.enabled || status.unsupported || status.status_unknown {
                                    status.message.clone()
                                } else {
                                    error
                                },
                            )
                        });
                    let _ = result.try_send(reply);
                    statuses.app_suspension = Some(status);
                }
                ProcessControlCommand::MemoryTrim { result } => {
                    let status = self.run_memory_trim_now(settings, observations);
                    let _ = result.try_send(Ok(status.clone()));
                    statuses.memory_trim = Some(status);
                }
                ProcessControlCommand::StopProcesses { targets, result } => {
                    let outcome = self.process_termination_controller.terminate_batch(
                        targets,
                        settings.general.allow_cross_session_process_control,
                    );
                    let _ = result.try_send(Ok(outcome));
                }
            }
        }
        statuses
    }

    pub(super) fn has_managed_process_control_state(&self) -> bool {
        self.cpu_allocation_coordinator.has_managed_state()
            || self.cpu_allocation_coordinator.has_pending_reconciliation()
            || self.dynamic_priority_boost_controller.has_managed_state()
            || self.thread_priority_controller.has_managed_state()
            || self.io_priority_controller.has_managed_state()
            || self.gpu_priority_controller.has_managed_state()
            || self.memory_priority_controller.has_managed_state()
            || self.priority_efficiency_controller.has_managed_state()
            || self
                .app_suspension_manager
                .has_suspended_processes(&self.app_suspension_controller)
    }

    pub(super) fn run_gpu_priority_update(
        &mut self,
        settings: &Settings,
        observations: &mut CycleObservations,
    ) -> GpuPrioritySnapshot {
        let gpu_priority_settings =
            effective_gpu_priority_settings(settings, self.workload_engine_active);
        let owner = gpu_priority_control_owner(settings, self.workload_engine_active);
        self.gpu_priority_manager.update(
            &mut self.gpu_priority_controller,
            owner,
            &gpu_priority_settings,
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

    pub(super) fn run_timer_resolution_update(
        &mut self,
        settings: &Settings,
        observations: &mut CycleObservations,
    ) -> TimerResolutionSnapshot {
        let foreground_executable_path = timer_resolution_required(settings)
            .then(|| observations.foreground_process())
            .flatten()
            .filter(|process| process_is_critical(process.id) == Some(false))
            .map(|process| process.executable_path.to_string_lossy().into_owned());
        self.timer_resolution_manager.update(
            &mut self.timer_resolution_controller,
            &settings.timer_resolution,
            settings.general.enabled,
            foreground_executable_path.as_deref(),
            &mut self.action_log,
        )
    }

    pub(super) fn run_check(
        &mut self,
        settings: &Settings,
        observations: &mut CycleObservations,
    ) -> Result<(), String> {
        if self.power_plan_controller.adaptive_active() {
            return Ok(());
        }

        let activity = self.activity_snapshot(settings, Instant::now());
        self.refresh_cpu_usage();
        let foreground_executable_path = foreground_lookup_required(settings)
            .then(|| observations.foreground_process())
            .flatten()
            .filter(|process| process_is_critical(process.id) == Some(false))
            .map(|process| process.executable_path.to_string_lossy().into_owned());
        let by_time_decision = current_by_time_decision(&settings.by_time);
        let by_cpu_load_decision = self
            .by_cpu_load_scheduler
            .current_decision(&settings.by_cpu_load, self.cpu_usage.percent);
        let by_running_app = self.by_running_app_manager.active_decision().map(
            |(rule_name, process_name, power_plan_guid)| ByRunningAppDecision {
                rule_name,
                process_name,
                power_plan_guid,
            },
        );
        let action_process_name = by_running_app
            .as_ref()
            .map(|decision| decision.process_name.clone())
            .unwrap_or_default();
        let decision_input = DecisionInput {
            activity_state: activity.state,
            foreground_executable_path,
            plugged_in: power_source::is_plugged_in(),
            by_running_app,
            by_time: by_time_decision,
            by_cpu_load: by_cpu_load_decision,
        };
        let decision = decide(settings, decision_input);
        let feature = power_plan_action_log_feature(decision.state);
        let target_guid = decision.power_plan_guid.clone().unwrap_or_default();
        let reason = decision.reason.clone();
        match self
            .power_plan_controller
            .reconcile_ordinary(decision, Instant::now())
        {
            Ok(applied) => {
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
                if let Some(feature) = feature {
                    self.action_log.record(
                        feature,
                        None,
                        action_process_name,
                        ActionLogResult::Failed,
                        format!("{reason} Power plan switch failed: {error}"),
                    );
                }
                Err(error)
            }
        }
    }

    pub(super) fn refresh_active_plan(&mut self) -> Result<(), String> {
        self.power_plan_controller
            .refresh_active_plan(Instant::now())
    }

    pub(super) fn refresh_cpu_usage(&mut self) {
        if self
            .next_cpu_usage_refresh
            .is_none_or(|refresh_at| Instant::now() >= refresh_at)
        {
            self.cpu_usage = self.cpu_monitor.sample_usage();
            self.next_cpu_usage_refresh = Some(Instant::now() + CPU_USAGE_REFRESH_INTERVAL);
        }
    }

    pub(super) fn power_plan_status(&self) -> PowerPlanStatus {
        self.power_plan_controller.status()
    }
}

pub(super) fn power_plan_action_log_feature(state: DecisionState) -> Option<ActionLogFeature> {
    match state {
        DecisionState::ByForeground => Some(ActionLogFeature::ByForeground),
        DecisionState::ByRunningApp => Some(ActionLogFeature::ByRunningApp),
        DecisionState::ByTime => Some(ActionLogFeature::ByTime),
        DecisionState::ByCpuLoad => Some(ActionLogFeature::ByCpuLoad),
        DecisionState::ByActivityIdle | DecisionState::ByActivityActive => {
            Some(ActionLogFeature::ByActivity)
        }
        DecisionState::Disabled
        | DecisionState::PausedWhilePluggedIn
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
            .contains_rule_for(&rule.executable_path)
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
        .map(|rule| rule.executable_path.clone())
        .collect()
}

impl Drop for RuntimeCore {
    fn drop(&mut self) {
        let _ = self.shutdown();
    }
}
