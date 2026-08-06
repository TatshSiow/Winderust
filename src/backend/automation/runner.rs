use super::*;
use crate::action_log::ActionLogFeature;

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

pub(super) struct ActiveAdaptivePowerPlan {
    original_guid: String,
    plan_guid: String,
    profile: AdaptivePowerProfile,
    baseline: ProcessorPowerValues,
    has_efficiency_cores: bool,
    lower_demand_since: Option<Instant>,
}

#[derive(Default)]
pub(super) struct HiddenAutomationRunner {
    last_settings: Option<Settings>,
    current_guid: Option<String>,
    original_power_plan_guid: Option<String>,
    next_active_plan_refresh: Option<Instant>,
    last_switch_attempt: Option<(String, Instant)>,
    switch_failure_suppression: ExecutionFailureTracker,
    cpu_usage: CpuUsageSnapshot,
    next_cpu_usage_refresh: Option<Instant>,
    cpu_monitor: CpuUsageMonitor,
    per_processor_cpu_monitor: PerProcessorUsageMonitor,
    io_monitor: IoUsageMonitor,
    adaptive_processor_topology: Vec<LogicalProcessorInfo>,
    adaptive_io_usage: IoUsageSnapshot,
    next_adaptive_io_refresh: Option<Instant>,
    adaptive_power_plan: Option<ActiveAdaptivePowerPlan>,
    adaptive_foreground_process_id: Option<u32>,
    idle_detector: IdleDetector,
    controller_activity_detector: ControllerActivityDetector,
    by_cpu_load_scheduler: ByCpuLoadScheduler,
    background_efficiency_manager: BackgroundEfficiencyManager,
    pub(super) app_suspension_manager: AppSuspensionManager,
    last_app_suspension_shell_user_intent: Option<Instant>,
    cpu_sets_soft_manager: CpuAllocationManager,
    processor_affinity_hard_manager: CpuAllocationManager,
    core_limiter_manager: CoreLimiterManager,
    pub(super) by_running_app_manager: ByRunningAppManager,
    pub(super) action_log: ActionLog,
    workload_engine_manager: WorkloadEngineManager,
    launch_boost_active: bool,
    workload_engine_active: bool,
    process_priority_manager: ProcessPriorityManager,
    thread_priority_manager: ThreadPriorityManager,
    dynamic_priority_boost_manager: DynamicPriorityBoostManager,
    io_priority_manager: IoPriorityManager,
    gpu_priority_manager: GpuPriorityManager,
    memory_priority_manager: MemoryPriorityManager,
    memory_trim_manager: MemoryTrimManager,
    timer_resolution_manager: TimerResolutionManager,
    pub(super) known_process_ids: BTreeSet<u32>,
    published_action_log_sequence: Option<u64>,
}

impl HiddenAutomationRunner {
    pub(super) fn shutdown(&mut self) {
        let mut settings = self.last_settings.clone().unwrap_or_default();
        settings.general.enabled = false;

        // Restore in the reverse order used by the automation loop. Several features can touch
        // the same process state, so relying on field drop order can restore an intermediate
        // Winderust-managed value instead of the value that preceded Winderust.
        self.run_timer_resolution_update(&settings);
        self.run_by_running_app_update(&settings);
        self.run_core_limiter_update(&settings);
        self.run_processor_affinity_hard_update(&settings);
        self.run_cpu_sets_soft_update(&settings);
        self.run_app_suspension_update(&settings, &[], &[]);
        self.run_memory_priority_update(&settings);
        self.run_gpu_priority_update(&settings);
        self.run_dynamic_priority_boost_update(&settings);
        self.run_thread_priority_update(&settings);
        self.run_process_priority_update(&settings);
        self.run_io_priority_update(&settings);
        self.run_workload_engine_update(&settings);
        self.run_background_efficiency_update(&settings);
        let _ = self.restore_adaptive_power_plan();
        self.restore_original_power_plan();
    }

    pub(super) fn note_settings(&mut self, settings: &Settings) -> bool {
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

    pub(super) fn detect_process_appearance(&mut self) -> bool {
        let Ok(processes) = list_processes() else {
            return false;
        };
        let current_ids = processes
            .into_iter()
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
        let latest_sequence = self.action_log.latest_sequence();
        if self.published_action_log_sequence == latest_sequence {
            return;
        }

        update_action_log_entries(shared, self.action_log.entries());
        self.published_action_log_sequence = latest_sequence;
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
    ) -> BackgroundEfficiencySnapshot {
        let foreground_process_id = foreground_process_id();
        let background_efficiency = settings.background_efficiency.clone();
        self.background_efficiency_manager.update(
            &background_efficiency,
            settings.general.enabled,
            settings.general.allow_cross_session_process_control,
            foreground_process_id,
            true,
            &mut self.action_log,
        )
    }

    pub(super) fn run_app_suspension_update(
        &mut self,
        settings: &Settings,
        manual_freeze_processes: &[String],
        process_requests: &[(ProcessActionTarget, bool)],
    ) -> AppSuspensionSnapshot {
        for (target, suspend) in process_requests {
            self.app_suspension_manager.apply_manual_process_action(
                target,
                *suspend,
                settings.general.allow_cross_session_process_control,
                &mut self.action_log,
            );
        }
        let foreground_process_id = foreground_process_id();
        self.app_suspension_manager.update(
            &settings.app_suspension,
            settings.general.enabled,
            settings.general.allow_cross_session_process_control,
            foreground_process_id,
            manual_freeze_processes,
            &mut self.action_log,
        )
    }

    pub(super) fn run_app_suspension_foreground_release(
        &mut self,
    ) -> Option<AppSuspensionSnapshot> {
        let now = Instant::now();
        if shell_window_mouse_pressed() && self.app_suspension_shell_user_intent_due(now) {
            self.last_app_suspension_shell_user_intent = Some(now);
            if let Some(status) = self
                .app_suspension_manager
                .release_all_suspended_processes_for_user_intent(&mut self.action_log)
            {
                return Some(status);
            }
        }

        let foreground_process_id = foreground_process_id();
        let foreground_process = foreground_process();
        if let Some(status) = foreground_process_id.and_then(|process_id| {
            self.app_suspension_manager.release_interactive_process(
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
    ) -> Option<AppSuspensionSnapshot> {
        self.app_suspension_manager
            .release_window_owner_processes_for_user_intent(
                &top_level_window_process_ids(),
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
            .release_all_suspended_processes_for_user_intent(&mut self.action_log)
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
    ) -> CpuAllocationSnapshot {
        let foreground_process_id = foreground_process_id();
        self.cpu_sets_soft_manager.update(
            &settings.cpu_sets_soft,
            (
                cpu_allocation::CpuAllocationMode::SoftCpuSets,
                ActionLogFeature::CpuSetsSoft,
            ),
            settings.general.enabled,
            settings.general.allow_cross_session_process_control,
            foreground_process_id,
            &mut self.action_log,
        )
    }

    pub(super) fn run_processor_affinity_hard_update(
        &mut self,
        settings: &Settings,
    ) -> CpuAllocationSnapshot {
        let processor_affinity_hard = processor_affinity_hard_settings(settings);
        self.processor_affinity_hard_manager.update(
            &processor_affinity_hard,
            (
                cpu_allocation::CpuAllocationMode::HardAffinity,
                ActionLogFeature::ProcessorAffinityHard,
            ),
            settings.general.enabled,
            settings.general.allow_cross_session_process_control,
            foreground_process_id(),
            &mut self.action_log,
        )
    }

    pub(super) fn run_core_limiter_update(&mut self, settings: &Settings) -> CoreLimiterSnapshot {
        let foreground_process_id = foreground_process_id();
        let mut allocated_process_ids = self.cpu_sets_soft_manager.adjusted_process_ids();
        allocated_process_ids.extend(self.processor_affinity_hard_manager.adjusted_process_ids());
        self.core_limiter_manager.update(
            &settings.core_limiter,
            settings.general.enabled,
            settings.general.allow_cross_session_process_control,
            foreground_process_id,
            &allocated_process_ids,
            &mut self.action_log,
        )
    }

    pub(super) fn run_by_running_app_update(
        &mut self,
        settings: &Settings,
    ) -> ByRunningAppSnapshot {
        self.by_running_app_manager.update(
            &settings.by_running_app,
            settings.general.enabled,
            &mut self.action_log,
        )
    }

    pub(super) fn run_workload_engine_update(
        &mut self,
        settings: &Settings,
    ) -> WorkloadEngineSnapshot {
        self.refresh_cpu_usage();
        let foreground_process_id = foreground_process_id();
        let mut workload_settings = settings.workload_engine.clone();
        workload_settings.enabled &= settings.adaptive_engine.enabled;
        let mut excluded_process_ids = self.background_efficiency_manager.throttled_process_ids();
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
                    .background_efficiency
                    .protect_foreground_app,
                protect_visible_window_apps_from_efficiency: settings
                    .background_efficiency
                    .protect_visible_window_apps,
                foreground_process_id,
                total_cpu_usage_percent: self.cpu_usage.percent,
                background_efficiency_managed: settings.background_efficiency.enabled,
                excluded_process_ids: &excluded_process_ids,
                explicit_cpu_allocation_paths: &explicit_cpu_allocation_paths,
            },
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
            self.restore_adaptive_power_plan()
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

        if self.adaptive_power_plan.is_none() {
            let original_guid = active_plan()?.guid;
            let plan_guid = create_adaptive_plan(&original_guid)?;
            if let Err(error) = apply_processor_power_values(
                &plan_guid,
                desired_profile.calibrated_power_values(baseline, has_efficiency_cores),
            )
            .and_then(|()| set_active_with_recovery(&plan_guid))
            {
                return Err(adaptive_plan_setup_error(error, delete_plan(&plan_guid)));
            }
            self.current_guid = Some(plan_guid.clone());
            self.adaptive_power_plan = Some(ActiveAdaptivePowerPlan {
                original_guid,
                plan_guid,
                profile: desired_profile,
                baseline,
                has_efficiency_cores,
                lower_demand_since: None,
            });
        }

        let should_refresh_active_plan = self
            .next_active_plan_refresh
            .is_none_or(|refresh_at| now >= refresh_at);
        if should_refresh_active_plan {
            self.refresh_active_plan();
        }
        let plan = self
            .adaptive_power_plan
            .as_mut()
            .ok_or_else(|| "Adaptive power plan was not initialized.".to_owned())?;
        if self
            .current_guid
            .as_deref()
            .is_none_or(|guid| !guid.eq_ignore_ascii_case(&plan.plan_guid))
        {
            set_active_with_recovery(&plan.plan_guid)?;
            self.current_guid = Some(plan.plan_guid.clone());
        }

        let lower_demand_elapsed = if desired_profile < plan.profile {
            now.duration_since(*plan.lower_demand_since.get_or_insert(now))
        } else {
            plan.lower_demand_since = None;
            Duration::ZERO
        };
        let next_profile =
            adaptive_power_profile_transition(plan.profile, desired_profile, lower_demand_elapsed);
        if next_profile != plan.profile || baseline != plan.baseline {
            apply_processor_power_values(
                &plan.plan_guid,
                next_profile.calibrated_power_values(baseline, plan.has_efficiency_cores),
            )?;
            plan.profile = next_profile;
            plan.baseline = baseline;
            plan.lower_demand_since = None;
        }

        snapshot.adaptive_power_profile = Some(plan.profile.label().to_owned());
        Ok(())
    }

    pub(super) fn restore_adaptive_power_plan(&mut self) -> Result<(), String> {
        let Some(plan) = self.adaptive_power_plan.take() else {
            return Ok(());
        };
        if let Err(error) = set_active_with_recovery(&plan.original_guid) {
            self.adaptive_power_plan = Some(plan);
            return Err(error);
        }

        self.current_guid = Some(plan.original_guid.clone());
        if let Err(error) = delete_plan(&plan.plan_guid) {
            self.adaptive_power_plan = Some(plan);
            return Err(error);
        }
        Ok(())
    }

    pub(super) fn run_io_priority_update(&mut self, settings: &Settings) -> IoPrioritySnapshot {
        let io_priority_settings =
            effective_io_priority_settings(settings, self.workload_engine_active);
        self.io_priority_manager.update(
            &io_priority_settings,
            settings.general.enabled,
            settings.general.allow_cross_session_process_control,
            foreground_process_id(),
            &mut self.action_log,
        )
    }

    pub(super) fn run_process_priority_update(
        &mut self,
        settings: &Settings,
    ) -> ProcessPrioritySnapshot {
        let mut excluded_process_ids = self.workload_engine_manager.managed_process_ids();
        excluded_process_ids.extend(self.background_efficiency_manager.throttled_process_ids());
        self.process_priority_manager.update(
            &settings.process_priority,
            settings.general.enabled,
            settings.general.allow_cross_session_process_control,
            foreground_process_id(),
            &excluded_process_ids,
            &mut self.action_log,
        )
    }

    pub(super) fn run_thread_priority_update(
        &mut self,
        settings: &Settings,
    ) -> ThreadPrioritySnapshot {
        let thread_priority_settings =
            effective_thread_priority_settings(settings, self.workload_engine_active);
        self.thread_priority_manager.update(
            &thread_priority_settings,
            settings.general.enabled,
            settings.general.allow_cross_session_process_control,
            foreground_process_id(),
            &mut self.action_log,
        )
    }

    pub(super) fn run_dynamic_priority_boost_update(
        &mut self,
        settings: &Settings,
    ) -> DynamicPriorityBoostSnapshot {
        let dynamic_priority_boost_settings =
            effective_dynamic_priority_boost_settings(settings, self.workload_engine_active);
        self.dynamic_priority_boost_manager.update(
            &dynamic_priority_boost_settings,
            settings.general.enabled,
            settings.general.allow_cross_session_process_control,
            foreground_process_id(),
            &mut self.action_log,
        )
    }

    pub(super) fn run_gpu_priority_update(&mut self, settings: &Settings) -> GpuPrioritySnapshot {
        let gpu_priority_settings =
            effective_gpu_priority_settings(settings, self.workload_engine_active);
        self.gpu_priority_manager.update(
            &gpu_priority_settings,
            settings.general.enabled,
            settings.general.allow_cross_session_process_control,
            foreground_process_id(),
            &mut self.action_log,
        )
    }

    pub(super) fn run_memory_priority_update(
        &mut self,
        settings: &Settings,
    ) -> MemoryPrioritySnapshot {
        self.memory_priority_manager.update_rules(
            &settings.memory_priority,
            settings.general.enabled,
            settings.general.allow_cross_session_process_control,
            foreground_process_id(),
            &mut self.action_log,
        )
    }

    pub(super) fn run_memory_trim_update(&mut self, settings: &Settings) -> MemoryTrimSnapshot {
        self.memory_trim_manager.update(
            &settings.memory_trim,
            settings.general.enabled,
            settings.general.allow_cross_session_process_control,
            foreground_process_id(),
            &mut self.action_log,
        )
    }

    pub(super) fn run_memory_trim_now(&mut self, settings: &Settings) -> MemoryTrimSnapshot {
        self.memory_trim_manager.trim_now(
            &settings.memory_trim,
            settings.general.enabled,
            settings.general.allow_cross_session_process_control,
            foreground_process_id(),
            &mut self.action_log,
        )
    }

    pub(super) fn run_timer_resolution_update(
        &mut self,
        settings: &Settings,
    ) -> TimerResolutionSnapshot {
        let foreground_executable_path = timer_resolution_required(settings)
            .then(foreground_process)
            .flatten()
            .filter(|process| process_is_critical(process.id) == Some(false))
            .map(|process| process.executable_path.to_string_lossy().into_owned());
        self.timer_resolution_manager.update(
            &settings.timer_resolution,
            settings.general.enabled,
            foreground_executable_path.as_deref(),
            &mut self.action_log,
        )
    }

    pub(super) fn run_check(&mut self, settings: &Settings) {
        if self.adaptive_power_plan.is_some() {
            return;
        }

        let should_refresh_active_plan = self
            .next_active_plan_refresh
            .is_none_or(|refresh_at| Instant::now() >= refresh_at);
        if should_refresh_active_plan {
            self.refresh_active_plan();
        }

        let activity = self.activity_snapshot(settings, Instant::now());
        self.refresh_cpu_usage();
        let foreground_executable_path = foreground_lookup_required(settings)
            .then(foreground_process)
            .flatten()
            .filter(|process| process_is_critical(process.id) == Some(false))
            .map(|process| process.executable_path.to_string_lossy().into_owned());
        let by_time_decision = current_by_time_decision(&settings.by_time);
        let by_cpu_load_decision = self
            .by_cpu_load_scheduler
            .current_decision(&settings.by_cpu_load, self.cpu_usage.percent);
        let decision_input = DecisionInput {
            activity_state: activity.state,
            foreground_executable_path,
            plugged_in: power_source::is_plugged_in(),
            by_running_app: self.by_running_app_manager.active_decision().map(
                |(rule_name, process_name, power_plan_guid)| ByRunningAppDecision {
                    rule_name,
                    process_name,
                    power_plan_guid,
                },
            ),
            by_time: by_time_decision,
            by_cpu_load: by_cpu_load_decision,
        };
        let decision = decide(settings, decision_input);
        self.apply_power_plan_guid(decision.power_plan_guid.as_deref());
    }

    pub(super) fn refresh_active_plan(&mut self) {
        self.next_active_plan_refresh = Some(Instant::now() + ACTIVE_PLAN_REFRESH_INTERVAL);

        if let Ok(active) = active_plan() {
            self.current_guid = Some(active.guid);
        }
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

    pub(super) fn apply_power_plan_guid(&mut self, plan_guid: Option<&str>) {
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
        let previous_guid = self
            .current_guid
            .clone()
            .or_else(|| active_plan().ok().map(|plan| plan.guid));

        match set_active_with_recovery(plan_guid) {
            Ok(()) => {
                if self.original_power_plan_guid.is_none() {
                    self.original_power_plan_guid = previous_guid;
                }
                self.current_guid = Some(plan_guid.to_owned());
                self.clear_switch_failure(plan_guid);
            }
            Err(_) => self.record_switch_failure(plan_guid),
        }
    }

    fn restore_original_power_plan(&mut self) {
        let Some(plan_guid) = self.original_power_plan_guid.take() else {
            return;
        };
        if set_active_with_recovery(&plan_guid).is_ok() {
            self.current_guid = Some(plan_guid);
        } else {
            self.original_power_plan_guid = Some(plan_guid);
        }
    }

    pub(super) fn is_switch_suppressed(&self, target_guid: &str) -> bool {
        self.switch_failure_suppression
            .is_key_suppressed(&switch_failure_key(target_guid))
    }

    pub(super) fn record_switch_failure(&mut self, target_guid: &str) {
        self.switch_failure_suppression
            .record_key_failure(&switch_failure_key(target_guid));
    }

    pub(super) fn clear_switch_failure(&mut self, target_guid: &str) {
        self.switch_failure_suppression
            .clear_key_failure(&switch_failure_key(target_guid));
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
                && rule.core_mask != 0
                && Path::new(rule.executable_path.trim()).is_absolute()
        })
        .map(|rule| rule.executable_path.clone())
        .collect()
}

pub(super) fn adaptive_plan_setup_error(
    operation_error: String,
    cleanup: Result<(), String>,
) -> String {
    match cleanup {
        Ok(()) => operation_error,
        Err(cleanup_error) => {
            format!("{operation_error} Adaptive plan cleanup also failed: {cleanup_error}")
        }
    }
}

impl Drop for HiddenAutomationRunner {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn set_active_with_recovery(plan_guid: &str) -> Result<(), String> {
    let current_guid = active_plan()?.guid;
    let recovery = crate::crash_recovery::record_power_plan_change(&current_guid, plan_guid)?;
    set_active(plan_guid)?;
    recovery.commit()
}

pub(super) fn switch_failure_key(target_guid: &str) -> String {
    target_guid.trim().to_ascii_lowercase()
}
