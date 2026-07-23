use super::*;

pub(super) fn adaptive_power_plan_required(settings: &Settings) -> bool {
    settings.adaptive_engine.enabled && settings.adaptive_engine.processor_policy_enabled
}

pub(super) fn static_processor_power_values(settings: &Settings) -> Option<ProcessorPowerValues> {
    let values = settings
        .adaptive_engine
        .processor_policy_values
        .normalized();
    let default_saver_values = ProcessorPowerValues::new_with_boost_mode(
        0,
        5,
        45,
        0,
        crate::power::ProcessorBoostMode::Disabled,
    );

    (settings.general.enabled
        && !settings.adaptive_engine.enabled
        && settings.adaptive_engine.processor_policy_enabled
        && !settings.background_efficiency.enabled
        && settings.workload_engine.enabled
        && settings.workload_engine.workload_engine_enabled
        && values != default_saver_values)
        .then_some(values)
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
    pub(super) fn update_peak(peak: &mut Option<f32>, usage: f32) {
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

pub(super) struct AppliedStaticProcessorPolicy {
    plan_guid: String,
    restore_values: ProcessorPowerAcDcValues,
    applied_values: ProcessorPowerValues,
}

#[derive(Default)]
pub(super) struct HiddenAutomationRunner {
    last_settings: Option<Settings>,
    current_guid: Option<String>,
    next_active_plan_refresh: Option<Instant>,
    last_switch_attempt: Option<(String, Instant)>,
    switch_failure_suppression: ExecutionFailureTracker,
    power: PowerPlanManager,
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
    static_processor_policy: Option<AppliedStaticProcessorPolicy>,
    idle_detector: IdleDetector,
    controller_activity_detector: ControllerActivityDetector,
    foreground_detector: ForegroundDetector,
    by_time_scheduler: ByTimeScheduler,
    by_cpu_load_scheduler: ByCpuLoadScheduler,
    background_efficiency_manager: BackgroundEfficiencyManager,
    pub(super) app_suspension_manager: AppSuspensionManager,
    last_app_suspension_shell_user_intent: Option<Instant>,
    core_steering_manager: CoreSteeringManager,
    background_cpu_restriction_manager: BackgroundCpuRestrictionManager,
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
        let foreground_process_id = self.foreground_detector.process_id();
        self.background_efficiency_manager.update(
            &settings.background_efficiency,
            settings.general.enabled,
            foreground_process_id,
            !settings.process_priority.enabled,
            &mut self.action_log,
        )
    }

    pub(super) fn run_app_suspension_update(
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

    pub(super) fn run_app_suspension_foreground_release(
        &mut self,
    ) -> Option<AppSuspensionSnapshot> {
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
        if !self.foreground_detector.cursor_is_shell_window() {
            return None;
        }

        self.run_app_suspension_app_switch_release()
    }

    pub(super) fn app_suspension_shell_user_intent_due(&self, now: Instant) -> bool {
        self.last_app_suspension_shell_user_intent
            .is_none_or(|last| {
                now.duration_since(last) >= APP_SUSPENSION_SHELL_USER_INTENT_INTERVAL
            })
    }

    pub(super) fn run_core_steering_update(&mut self, settings: &Settings) -> CoreSteeringSnapshot {
        let foreground_process_id = self.foreground_detector.process_id();
        self.core_steering_manager.update(
            &settings.core_steering,
            settings.general.enabled,
            foreground_process_id,
            &mut self.action_log,
        )
    }

    pub(super) fn run_background_cpu_restriction_update(
        &mut self,
        settings: &Settings,
    ) -> CoreSteeringSnapshot {
        self.background_cpu_restriction_manager.update(
            &settings.background_cpu_restriction,
            settings.general.enabled,
            self.foreground_detector.process_id(),
            &mut self.action_log,
        )
    }

    pub(super) fn run_core_limiter_update(&mut self, settings: &Settings) -> CoreLimiterSnapshot {
        let foreground_process_id = self.foreground_detector.process_id();
        let core_steering_process_ids = self.core_steering_manager.adjusted_process_ids();
        self.core_limiter_manager.update(
            &settings.core_limiter,
            settings.general.enabled,
            foreground_process_id,
            &core_steering_process_ids,
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
        let foreground_process_id = self.foreground_detector.process_id();
        let mut excluded_process_ids = self.background_efficiency_manager.throttled_process_ids();
        excluded_process_ids.extend(self.by_running_app_manager.active_process_ids());
        let mut snapshot = self.workload_engine_manager.update(
            WorkloadEngineUpdate {
                settings: &settings.workload_engine,
                automation_enabled: settings.general.enabled,
                foreground_process_id,
                total_cpu_usage_percent: self.cpu_usage.percent,
                background_efficiency_managed: settings.background_efficiency.enabled,
                background_efficiency_process_ids: &excluded_process_ids,
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
            self.restore_static_processor_policy()?;
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
            self.restore_adaptive_power_plan()?;
            self.sync_static_processor_policy(settings)
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
            self.adaptive_processor_topology = core_steering::logical_processors();
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
            let original_guid = self
                .power
                .active_plan()?
                .ok_or_else(|| "Windows has no active power plan.".to_owned())?
                .guid;
            let plan_guid = self.power.create_adaptive_plan(&original_guid)?;
            if let Err(error) = self
                .power
                .apply_processor_power_values(
                    &plan_guid,
                    desired_profile.calibrated_power_values(baseline, has_efficiency_cores),
                )
                .and_then(|()| self.power.set_active(&plan_guid))
            {
                let _ = self.power.delete_plan(&plan_guid);
                return Err(error);
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
            self.power.set_active(&plan.plan_guid)?;
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
            self.power.apply_processor_power_values(
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
        if let Err(error) = self.power.set_active(&plan.original_guid) {
            self.adaptive_power_plan = Some(plan);
            return Err(error);
        }

        self.current_guid = Some(plan.original_guid);
        self.power.delete_plan(&plan.plan_guid)
    }

    pub(super) fn sync_static_processor_policy(
        &mut self,
        settings: &Settings,
    ) -> Result<(), String> {
        let desired_values = static_processor_power_values(settings);
        if self
            .static_processor_policy
            .as_ref()
            .is_some_and(|policy| Some(policy.applied_values) == desired_values)
        {
            return Ok(());
        }

        self.restore_static_processor_policy()?;
        let Some(values) = desired_values else {
            return Ok(());
        };
        let plan_guid = self
            .power
            .active_plan()?
            .ok_or_else(|| "Windows has no active power plan.".to_owned())?
            .guid;
        let restore_values = self.power.read_processor_power_values(&plan_guid)?;
        self.power
            .apply_processor_power_values(&plan_guid, ProcessorPowerAcDcValues::same(values))?;
        self.static_processor_policy = Some(AppliedStaticProcessorPolicy {
            plan_guid,
            restore_values,
            applied_values: values,
        });
        Ok(())
    }

    pub(super) fn restore_static_processor_policy(&mut self) -> Result<(), String> {
        let Some(policy) = self.static_processor_policy.take() else {
            return Ok(());
        };
        if let Err(error) = self
            .power
            .apply_processor_power_values(&policy.plan_guid, policy.restore_values)
        {
            self.static_processor_policy = Some(policy);
            return Err(error);
        }
        Ok(())
    }

    pub(super) fn run_io_priority_update(&mut self, settings: &Settings) -> IoPrioritySnapshot {
        let io_priority_settings = effective_io_priority_settings(
            settings,
            self.launch_boost_active,
            self.workload_engine_active,
        );
        self.io_priority_manager.update(
            &io_priority_settings,
            settings.general.enabled,
            self.foreground_detector.process_id(),
            &mut self.action_log,
        )
    }

    pub(super) fn run_process_priority_update(
        &mut self,
        settings: &Settings,
    ) -> ProcessPrioritySnapshot {
        let excluded_process_ids = self.workload_engine_manager.managed_process_ids();
        self.process_priority_manager.update(
            &settings.process_priority,
            settings.general.enabled,
            self.foreground_detector.process_id(),
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
            self.foreground_detector.process_id(),
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
            self.foreground_detector.process_id(),
            &mut self.action_log,
        )
    }

    pub(super) fn run_gpu_priority_update(&mut self, settings: &Settings) -> GpuPrioritySnapshot {
        let gpu_priority_settings =
            effective_gpu_priority_settings(settings, self.workload_engine_active);
        self.gpu_priority_manager.update(
            &gpu_priority_settings,
            settings.general.enabled,
            self.foreground_detector.process_id(),
            &mut self.action_log,
        )
    }

    pub(super) fn run_memory_priority_update(
        &mut self,
        settings: &Settings,
    ) -> MemoryPrioritySnapshot {
        let memory_priority_settings = effective_memory_priority_settings(settings);
        self.memory_priority_manager.update_rules(
            &memory_priority_settings,
            settings.general.enabled,
            self.foreground_detector.process_id(),
            &mut self.action_log,
        )
    }

    pub(super) fn run_memory_trim_update(&mut self, settings: &Settings) -> MemoryTrimSnapshot {
        self.memory_trim_manager.update(
            &settings.memory_trim,
            settings.general.enabled,
            self.foreground_detector.process_id(),
            self.by_running_app_manager.is_active(),
            &mut self.action_log,
        )
    }

    pub(super) fn run_memory_trim_now(&mut self, settings: &Settings) -> MemoryTrimSnapshot {
        self.memory_trim_manager.trim_now(
            &settings.memory_trim,
            settings.general.enabled,
            self.foreground_detector.process_id(),
            self.by_running_app_manager.is_active(),
            &mut self.action_log,
        )
    }

    pub(super) fn run_timer_resolution_update(
        &mut self,
        settings: &Settings,
    ) -> TimerResolutionSnapshot {
        let foreground_process_name = self.foreground_detector.process_name();
        self.timer_resolution_manager.update(
            &settings.timer_resolution,
            settings.general.enabled,
            foreground_process_name.as_deref(),
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
        let foreground_process_name = foreground_lookup_required(settings)
            .then(|| self.foreground_detector.process_name())
            .flatten();
        let by_time_decision = self.by_time_scheduler.current_decision(&settings.by_time);
        let by_cpu_load_decision = self
            .by_cpu_load_scheduler
            .current_decision(&settings.by_cpu_load, self.cpu_usage.percent);
        let decision_input = DecisionInput {
            activity_state: activity.state,
            foreground_process_name,
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

        if let Ok(Some(active)) = self.power.active_plan() {
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

        match self.power.set_active(plan_guid) {
            Ok(()) => {
                self.current_guid = Some(plan_guid.to_owned());
                self.clear_switch_failure(plan_guid);
            }
            Err(_) => self.record_switch_failure(plan_guid),
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

impl Drop for HiddenAutomationRunner {
    fn drop(&mut self) {
        let _ = self.restore_adaptive_power_plan();
        let _ = self.restore_static_processor_policy();
    }
}

pub(super) fn switch_failure_key(target_guid: &str) -> String {
    target_guid.trim().to_ascii_lowercase()
}
