use crate::ui::app::*;

impl WinderustApp {
    pub(in crate::ui::app) fn schedule_tick(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self._tick_task = cx.spawn_in(window, async move |this, cx| {
            Timer::after(APP_TICK_INTERVAL).await;
            let _ = cx.update(move |window, app_cx| {
                if let Some(this) = this.upgrade() {
                    this.update(app_cx, |app, cx| match app.tick(window, cx) {
                        TickOutcome::Continue { changed } => {
                            app.schedule_tick(window, cx);
                            if changed {
                                cx.notify();
                            }
                        }
                        TickOutcome::Stop => {}
                    });
                }
            });
        });
    }

    pub(in crate::ui::app) fn refresh_power_plans(&mut self) {
        match list_plans() {
            Ok(plans) => {
                self.plans = plans;
                self.current_plan = self.plans.iter().find(|plan| plan.active).cloned();
                self.next_active_plan_refresh = Instant::now() + ACTIVE_PLAN_REFRESH_INTERVAL;
                self.status_message =
                    t!("status.loaded_power_plans", count = self.plans.len()).to_string();
                self.ensure_processor_power_target_plan();
                self.sync_processor_power_values_from_target_plan(false);
            }
            Err(err) => self.status_message = err,
        }
    }

    pub(in crate::ui::app) fn refresh_active_plan(&mut self) {
        self.next_active_plan_refresh = Instant::now() + ACTIVE_PLAN_REFRESH_INTERVAL;

        match active_plan() {
            Ok(active) => {
                let active_guid = active.guid.clone();
                for plan in &mut self.plans {
                    plan.active = plan.guid.eq_ignore_ascii_case(&active_guid);
                }
                self.current_plan = self
                    .plans
                    .iter()
                    .find(|plan| plan.guid.eq_ignore_ascii_case(&active_guid))
                    .cloned()
                    .or(Some(active));
                self.ensure_processor_power_target_plan();
                self.sync_processor_power_values_from_target_plan(false);
            }
            Err(err) => self.status_message = err,
        }
    }

    pub(in crate::ui::app) fn refresh_effective_power_mode(&mut self) -> bool {
        let Some(monitor) = &self.effective_power_mode_monitor else {
            return false;
        };
        let mode = monitor.snapshot();
        if self.effective_power_mode == mode {
            return false;
        }

        self.effective_power_mode = mode;
        true
    }

    pub(in crate::ui::app) fn sync_adaptive_engine(&mut self, settings: &Settings) {
        let result = if settings.adaptive_engine.enabled {
            self_power::enable_adaptive_engine()
        } else {
            self_power::disable_adaptive_engine()
        };
        if let Err(err) = result {
            self.status_message = err;
        }
    }

    pub(in crate::ui::app) fn run_check(&mut self, sample_dashboard: bool, now: Instant) {
        if now >= self.next_active_plan_refresh {
            self.refresh_active_plan();
        }

        let decision_settings = self.cached_runtime_settings();
        let decision_settings = decision_settings.as_ref();
        self.activity = self.activity_snapshot(decision_settings, now);
        if sample_dashboard && self.page == Page::Home {
            self.refresh_dashboard_resource_samples();
        } else if decision_settings.by_cpu_load.enabled {
            self.refresh_cpu_usage_sample(now);
        }
        let foreground_process = foreground_lookup_required(decision_settings)
            .then(foreground_process)
            .flatten();
        self.foreground_app = foreground_process
            .as_ref()
            .map(|process| process.name.clone());
        let by_time = current_by_time_decision(&decision_settings.by_time);
        let by_cpu_load = self
            .by_cpu_load_scheduler
            .current_decision(&decision_settings.by_cpu_load, self.cpu_usage.percent);
        self.next_schedule = next_by_time_switch_label(&decision_settings.by_time);

        self.decision = decide(
            decision_settings,
            DecisionInput {
                activity_state: self.activity.state,
                foreground_executable_path: foreground_process
                    .map(|process| process.executable_path.to_string_lossy().into_owned()),
                plugged_in: power_source::is_plugged_in(),
                by_running_app: by_running_app_decision(&self.by_running_app_status),
                by_time,
                by_cpu_load,
            },
        );

        if !(decision_settings.general.enabled
            && decision_settings.adaptive_engine.enabled
            && decision_settings.adaptive_engine.processor_policy_enabled)
        {
            self.apply_decision();
        }
    }

    pub(in crate::ui::app) fn run_check_changed(&mut self, now: Instant) -> bool {
        let activity_state = self.activity.state;
        let activity_idle_for = self.activity.idle_for;
        let cpu_usage = self.cpu_usage;
        let memory_usage = self.memory_usage;
        let io_usage = self.io_usage;
        let network_usage = self.network_usage;
        let decision_power_plan_guid = self.decision.power_plan_guid.take();
        let decision_state = self.decision.state;
        let decision_reason = std::mem::take(&mut self.decision.reason);
        let next_schedule = std::mem::take(&mut self.next_schedule);
        let plan_count = self.plans.len();
        let previous_active_plan_guid = active_plan_guid(&self.plans).map(str::to_owned);
        let current_plan_guid = self.current_plan.as_ref().map(|plan| plan.guid.clone());
        let processor_power_target_plan_personality = self.processor_power_target_plan_personality;
        let status_message = self.status_message.clone();

        self.run_check(false, now);

        let resource_samples_changed = self.cpu_usage != cpu_usage
            || self.memory_usage != memory_usage
            || self.io_usage != io_usage
            || self.network_usage != network_usage;
        let resource_samples_visible = self.page == Page::Home;

        self.activity.state != activity_state
            || self.activity.idle_for != activity_idle_for
            || (resource_samples_visible && resource_samples_changed)
            || self.decision.power_plan_guid != decision_power_plan_guid
            || self.decision.state != decision_state
            || self.decision.reason != decision_reason
            || self.next_schedule != next_schedule
            || self.plans.len() != plan_count
            || active_plan_guid(&self.plans) != previous_active_plan_guid.as_deref()
            || self.current_plan.as_ref().map(|plan| plan.guid.as_str())
                != current_plan_guid.as_deref()
            || self.processor_power_target_plan_personality
                != processor_power_target_plan_personality
            || self.status_message != status_message
    }

    pub(in crate::ui::app) fn activity_snapshot(
        &mut self,
        settings: &Settings,
        now: Instant,
    ) -> ActivitySnapshot {
        let idle_timeout = Duration::from_secs(settings.by_activity.idle_timeout_seconds);
        let snapshot = self.idle_detector.snapshot(idle_timeout);
        let controller_idle_for = if settings.by_activity.input_detection.controller {
            self.controller_activity_detector.poll(now);
            self.controller_activity_detector.idle_for(now)
        } else {
            self.controller_activity_detector.clear();
            None
        };

        merge_activity_snapshot(snapshot, controller_idle_for, idle_timeout)
    }

    pub(in crate::ui::app) fn refresh_cpu_usage_sample(&mut self, now: Instant) -> bool {
        if !refresh_due(
            now,
            &mut self.next_cpu_usage_refresh,
            CPU_USAGE_REFRESH_INTERVAL,
        ) {
            return false;
        }

        let previous_cpu_usage = self.cpu_usage;
        self.cpu_usage = self.cpu_monitor.sample_usage();
        self.cpu_usage != previous_cpu_usage
    }

    pub(in crate::ui::app) fn refresh_dashboard_resource_samples(&mut self) -> bool {
        if self.settings.advanced.pause_dashboard_metrics {
            return false;
        }

        let now = Instant::now();
        if !refresh_due(
            now,
            &mut self.next_cpu_usage_refresh,
            CPU_USAGE_REFRESH_INTERVAL,
        ) {
            return false;
        }

        let previous_cpu_usage = self.cpu_usage;
        let previous_memory_usage = self.memory_usage;
        let sample_io = refresh_due(
            now,
            &mut self.next_dashboard_io_refresh,
            DASHBOARD_IO_REFRESH_INTERVAL,
        );

        self.cpu_usage = self.cpu_monitor.sample();
        self.memory_usage = sample_memory_usage();

        let mut changed =
            self.cpu_usage != previous_cpu_usage || self.memory_usage != previous_memory_usage;

        if let Some(percent) = self.cpu_usage.percent {
            if self.cpu_usage_history.len() == DASHBOARD_HISTORY_LEN {
                self.cpu_usage_history.pop_front();
            }
            self.cpu_usage_history.push_back(CpuUsageHistorySample {
                percent: percent.clamp(0.0, 100.0),
                frequency_mhz: self.cpu_usage.frequency_mhz,
            });
            changed = true;
        }
        if let Some(percent) = self.memory_usage.percent {
            if self.memory_usage_history.len() == DASHBOARD_HISTORY_LEN {
                self.memory_usage_history.pop_front();
            }
            self.memory_usage_history
                .push_back(MemoryUsageHistorySample {
                    usage_percent: percent.clamp(0.0, 100.0),
                    cache_percent: memory_cache_percent(self.memory_usage).unwrap_or(0.0),
                });
            changed = true;
        }
        if sample_io {
            let previous_io_usage = self.io_usage;
            let previous_network_usage = self.network_usage;
            self.io_usage = self.io_monitor.sample();
            self.network_usage = self.network_monitor.sample();
            changed |=
                self.io_usage != previous_io_usage || self.network_usage != previous_network_usage;

            if self.io_usage.bytes_per_second.is_some() {
                if self.io_usage_history.len() == DASHBOARD_HISTORY_LEN {
                    self.io_usage_history.pop_front();
                }
                self.io_usage_history.push_back(IoUsageHistorySample {
                    read_bytes_per_second: self
                        .io_usage
                        .read_bytes_per_second
                        .unwrap_or(0.0)
                        .clamp(0.0, f32::MAX as f64)
                        as f32,
                    write_bytes_per_second: self
                        .io_usage
                        .write_bytes_per_second
                        .unwrap_or(0.0)
                        .clamp(0.0, f32::MAX as f64)
                        as f32,
                });
                changed = true;
            }
            if self.network_usage.bytes_per_second.is_some() {
                if self.network_usage_history.len() == DASHBOARD_HISTORY_LEN {
                    self.network_usage_history.pop_front();
                }
                self.network_usage_history
                    .push_back(NetworkUsageHistorySample {
                        download_bytes_per_second: self
                            .network_usage
                            .download_bytes_per_second
                            .unwrap_or(0.0)
                            .clamp(0.0, f32::MAX as f64)
                            as f32,
                        upload_bytes_per_second: self
                            .network_usage
                            .upload_bytes_per_second
                            .unwrap_or(0.0)
                            .clamp(0.0, f32::MAX as f64)
                            as f32,
                    });
                changed = true;
            }
        }

        changed
    }

    pub(in crate::ui::app) fn install_input_hook(&mut self, config: InputHookConfig) {
        match InputHook::install(config, self.background_automation.input_event_callback()) {
            Ok(input_hook) => {
                self.input_hook = Some(input_hook);
            }
            Err(err) => {
                self.status_message = err;
            }
        }
    }

    pub(in crate::ui::app) fn sync_input_hook(&mut self) {
        if input_hook_required(&self.saved_settings) {
            let config = input_hook_config(&self.saved_settings);
            if self
                .input_hook
                .as_ref()
                .is_none_or(|input_hook| input_hook.config() != config)
            {
                self.input_hook = None;
                self.install_input_hook(config);
            }
        } else {
            self.input_hook = None;
        }
    }

    pub(in crate::ui::app) fn apply_decision(&mut self) {
        let Some(target_guid) = self.decision.power_plan_guid.as_deref() else {
            return;
        };

        let already_active = self
            .current_plan
            .as_ref()
            .is_some_and(|plan| plan.guid.eq_ignore_ascii_case(target_guid));
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

        let switch_result = active_plan().and_then(|current| {
            let recovery =
                crate::crash_recovery::record_power_plan_change(&current.guid, target_guid)?;
            set_active(target_guid)?;
            recovery.commit()
        });
        match switch_result {
            Ok(()) => {
                self.status_message =
                    t!("status.switched_power_plan", reason = self.decision.reason).to_string();
                self.refresh_power_plans();
            }
            Err(err) => self.status_message = err,
        }
    }

    pub(in crate::ui::app) fn tick(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> TickOutcome {
        if tray::take_quit_requested() {
            self.set_tray_hide_on_close(false);
            self.tray_icon = None;
            window.remove_window();
            return TickOutcome::Stop;
        }

        let mut changed = self.apply_start_minimized(window);
        changed |= self.apply_pending_auto_exclusions();
        if tray::is_hidden_to_tray() {
            self.sync_input_hook();
            self.sync_background_settings();
            return TickOutcome::Stop;
        }

        if let Some(background_status) = self
            .background_automation
            .status_snapshot_since(self.last_background_status_generation)
        {
            self.last_background_status_generation = background_status.generation;

            if let Some(worker_error) = background_status.worker_error.as_ref() {
                if self.status_message != *worker_error {
                    self.status_message = worker_error.clone();
                    changed = true;
                }
            }

            if self.background_efficiency_status != background_status.background_efficiency {
                self.background_efficiency_status = background_status.background_efficiency;
                changed = true;
            }

            if self.app_suspension_status != background_status.app_suspension {
                self.app_suspension_status = background_status.app_suspension;
                changed = true;
            }

            if self.core_limiter_status != background_status.core_limiter {
                self.core_limiter_status = background_status.core_limiter;
                changed = true;
            }

            if self.cpu_sets_soft_status != background_status.cpu_sets_soft {
                self.cpu_sets_soft_status = background_status.cpu_sets_soft;
                changed = true;
            }

            if self.processor_affinity_hard_status != background_status.processor_affinity_hard {
                self.processor_affinity_hard_status = background_status.processor_affinity_hard;
                changed = true;
            }

            if self.by_running_app_status != background_status.by_running_app {
                self.by_running_app_status = background_status.by_running_app;
                changed = true;
            }

            if self.workload_engine_status != background_status.workload_engine {
                self.workload_engine_status = background_status.workload_engine;
                changed = true;
            }

            if self.process_priority_status != background_status.process_priority {
                self.process_priority_status = background_status.process_priority;
                changed = true;
            }

            if self.thread_priority_status != background_status.thread_priority {
                self.thread_priority_status = background_status.thread_priority;
                changed = true;
            }

            if self.dynamic_priority_boost_status != background_status.dynamic_priority_boost {
                self.dynamic_priority_boost_status = background_status.dynamic_priority_boost;
                changed = true;
            }

            if self.io_priority_status != background_status.io_priority {
                self.io_priority_status = background_status.io_priority;
                changed = true;
            }

            if self.gpu_priority_status != background_status.gpu_priority {
                self.gpu_priority_status = background_status.gpu_priority;
                changed = true;
            }

            if self.memory_priority_status != background_status.memory_priority {
                self.memory_priority_status = background_status.memory_priority;
                changed = true;
            }

            if self.memory_trim_status != background_status.memory_trim {
                self.memory_trim_status = background_status.memory_trim;
                changed = true;
            }

            if self.timer_resolution_status != background_status.timer_resolution {
                self.timer_resolution_status = background_status.timer_resolution;
                changed = true;
            }

            if !Arc::ptr_eq(
                &self.action_log_entries,
                &background_status.action_log_entries,
            ) {
                self.action_log_entries = background_status.action_log_entries;
                changed = true;
            }

            if self.last_appearance_change_generation
                != background_status.appearance_change_generation
            {
                self.last_appearance_change_generation =
                    background_status.appearance_change_generation;
                apply_appearance_settings(&self.settings.general, window, cx);
                changed = true;
            }
        }

        changed |= self.refresh_effective_power_mode();

        let now = Instant::now();

        if self.page == Page::TimerResolution
            && !self.settings.timer_resolution.enabled
            && refresh_due(
                now,
                &mut self.next_timer_resolution_status_refresh,
                TIMER_RESOLUTION_STATUS_REFRESH_INTERVAL,
            )
        {
            let timer_resolution_status =
                timer_resolution::query_snapshot(self.settings.timer_resolution.enabled);
            if self.timer_resolution_status != timer_resolution_status {
                self.timer_resolution_status = timer_resolution_status;
                changed = true;
            }
        }

        if now >= self.next_process_refresh {
            if self.page == Page::ProcessList {
                changed |= self.refresh_running_processes(false, cx);
            } else if self.page_uses_process_candidates() {
                changed |= self.refresh_process_candidates(false, cx);
            }
        }

        if self.page == Page::Home {
            changed |= self.refresh_dashboard_resource_samples();
        }

        let should_check_now = now >= self.next_check;

        if should_check_now {
            changed |= self.run_check_changed(now);
            self.next_check = now
                + Duration::from_millis(
                    self.settings
                        .general
                        .check_interval_ms
                        .clamp(CHECK_INTERVAL_MIN_MS, CHECK_INTERVAL_MAX_MS),
                );
        }

        changed |= self.sync_tray_icon();

        if !should_check_now {
            self.sync_background_settings();
        }
        TickOutcome::Continue { changed }
    }

    pub(in crate::ui::app) fn apply_pending_auto_exclusions(&mut self) -> bool {
        let Some(pending) = self
            .background_automation
            .take_pending_auto_exclusions_since(&mut self.last_pending_auto_exclusions_generation)
        else {
            return false;
        };
        let mut changed = false;

        for process in pending.app_suspension {
            changed |= upsert_rule_enabled(
                &mut self.settings.app_suspension.suspendable_apps,
                &process,
                false,
                |rule| &rule.executable_path,
                |rule, enabled| set_enabled_value(&mut rule.enabled, enabled),
                || {
                    let mut rule = new_app_suspension_rule(&process);
                    rule.enabled = false;
                    rule
                },
            );
        }

        for process in pending.cpu_sets_soft {
            changed |= upsert_rule_enabled(
                &mut self.settings.cpu_sets_soft.rules,
                &process,
                false,
                |rule| &rule.executable_path,
                |rule, enabled| set_enabled_value(&mut rule.enabled, enabled),
                || {
                    let mut rule = new_cpu_allocation_rule(&process);
                    rule.enabled = false;
                    rule
                },
            );
        }

        for process in pending.processor_affinity_hard {
            changed |= upsert_rule_enabled(
                &mut self.settings.processor_affinity_hard.rules,
                &process,
                false,
                |rule| &rule.executable_path,
                |rule, enabled| set_enabled_value(&mut rule.enabled, enabled),
                || {
                    let mut rule = new_cpu_allocation_rule(&process);
                    rule.enabled = false;
                    rule
                },
            );
        }

        for process in pending.core_limiter {
            changed |= upsert_rule_enabled(
                &mut self.settings.core_limiter.rules,
                &process,
                false,
                |rule| &rule.executable_path,
                |rule, enabled| set_enabled_value(&mut rule.enabled, enabled),
                || {
                    let mut rule = new_core_limiter_rule(&process);
                    rule.enabled = false;
                    rule
                },
            );
        }

        for process in pending.workload_engine {
            changed |= enable_process_exclusion(
                &mut self.settings.workload_engine.workload_engine_exclusions,
                &process,
            );
        }

        for process in pending.io_priority {
            changed |=
                enable_process_exclusion(&mut self.settings.io_priority.exclusions, &process);
        }

        for process in pending.process_priority {
            changed |=
                enable_process_exclusion(&mut self.settings.process_priority.exclusions, &process);
        }

        for process in pending.thread_priority {
            changed |=
                enable_process_exclusion(&mut self.settings.thread_priority.exclusions, &process);
        }

        for process in pending.dynamic_priority_boost {
            changed |= enable_process_exclusion(
                &mut self.settings.dynamic_priority_boost.exclusions,
                &process,
            );
        }

        for process in pending.gpu_priority {
            changed |=
                enable_process_exclusion(&mut self.settings.gpu_priority.exclusions, &process);
        }

        for process in pending.memory_priority {
            changed |=
                enable_process_exclusion(&mut self.settings.memory_priority.exclusions, &process);
        }

        for process in pending.memory_trim {
            changed |=
                enable_process_exclusion(&mut self.settings.memory_trim.exclusions, &process);
        }

        if changed {
            self.save_settings();
        }

        changed
    }
}

fn upsert_rule_enabled<T>(
    rules: &mut Vec<T>,
    executable_path: &str,
    enabled: bool,
    rule_path: impl Fn(&T) -> &str,
    set_enabled: impl Fn(&mut T, bool) -> bool,
    new_rule: impl FnOnce() -> T,
) -> bool {
    if let Some(rule) = rules
        .iter_mut()
        .find(|rule| same_executable_path(Path::new(rule_path(rule)), Path::new(executable_path)))
    {
        return set_enabled(rule, enabled);
    }

    rules.push(new_rule());
    true
}

fn set_enabled_value(current: &mut bool, enabled: bool) -> bool {
    if *current == enabled {
        return false;
    }
    *current = enabled;
    true
}

fn enable_process_exclusion(rules: &mut Vec<ProcessExclusionRule>, executable_path: &str) -> bool {
    upsert_rule_enabled(
        rules,
        executable_path,
        true,
        |rule| &rule.executable_path,
        |rule, enabled| set_enabled_value(&mut rule.enabled, enabled),
        || {
            let mut rule = new_process_exclusion_rule(executable_path);
            rule.enabled = true;
            rule
        },
    )
}

#[cfg(test)]
mod auto_exclusion_tests {
    use super::*;

    #[test]
    fn broad_auto_exclusion_reenables_matching_rule() {
        let mut rule = new_process_exclusion_rule(r"C:\Apps\worker.exe");
        rule.enabled = false;
        let mut rules = vec![rule];

        assert!(enable_process_exclusion(&mut rules, r"C:\Apps\worker.exe"));
        assert!(rules[0].enabled);
        assert_eq!(rules.len(), 1);
    }

    #[test]
    fn target_auto_exclusion_disables_matching_rule() {
        let mut rules = vec![new_cpu_allocation_rule(r"C:\Apps\worker.exe")];

        assert!(upsert_rule_enabled(
            &mut rules,
            r"C:\Apps\worker.exe",
            false,
            |rule| &rule.executable_path,
            |rule, enabled| set_enabled_value(&mut rule.enabled, enabled),
            || unreachable!("matching rule should be updated"),
        ));
        assert!(!rules[0].enabled);
        assert_eq!(rules.len(), 1);
    }
}
