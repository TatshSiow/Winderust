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

    pub(in crate::ui::app) fn run_check(&mut self, now: Instant) {
        if now >= self.next_active_plan_refresh {
            self.refresh_active_plan();
        }

        let runtime_settings = self.current_runtime_settings();
        self.activity = self.activity_snapshot(runtime_settings.as_ref(), now);
        self.next_schedule = next_by_time_switch_label(&runtime_settings.by_time);
    }

    pub(in crate::ui::app) fn run_check_changed(&mut self, now: Instant) -> bool {
        let activity_state = self.activity.state;
        let activity_idle_for = self.activity.idle_for;
        let next_schedule = std::mem::take(&mut self.next_schedule);
        let plan_count = self.plans.len();
        let previous_active_plan_guid = active_plan_guid(&self.plans).map(str::to_owned);
        let current_plan_guid = self.current_plan.as_ref().map(|plan| plan.guid.clone());
        let processor_power_target_plan_personality = self.processor_power_target_plan_personality;
        let status_message = self.status_message.clone();

        self.run_check(now);

        self.activity.state != activity_state
            || self.activity.idle_for != activity_idle_for
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
        let snapshot = activity_snapshot(idle_timeout);
        let controller_idle_for = if settings.by_activity.input_detection.controller {
            self.controller_activity_detector.poll(now);
            self.controller_activity_detector.idle_for(now)
        } else {
            self.controller_activity_detector.clear();
            None
        };

        merge_activity_snapshot(snapshot, controller_idle_for, idle_timeout)
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

        let sample_io = refresh_due(
            now,
            &mut self.next_dashboard_io_refresh,
            DASHBOARD_IO_REFRESH_INTERVAL,
        );

        let mut changed = self
            .dashboard
            .record_cpu_memory(self.cpu_monitor.sample(), sample_memory_usage());
        if sample_io {
            changed |= self
                .dashboard
                .record_io_network(self.io_monitor.sample(), self.network_monitor.sample());
        }

        changed
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
            self.sync_runtime_settings();
            return TickOutcome::Stop;
        }

        if let Some(runtime_status) = self
            .runtime_handle
            .status_snapshot_since(self.last_runtime_status_generation)
        {
            self.last_runtime_status_generation = runtime_status.generation;
            if let Some(worker_error) = runtime_status.worker_error.as_ref() {
                if self.status_message != *worker_error {
                    self.status_message = worker_error.clone();
                    changed = true;
                }
            }

            if let Some(runtime_guid) = runtime_status.power_plan_status.current_guid.as_deref() {
                let displayed_guid = self.current_plan.as_ref().map(|plan| plan.guid.as_str());
                if displayed_guid.is_none_or(|guid| !guid.eq_ignore_ascii_case(runtime_guid)) {
                    self.refresh_active_plan();
                    changed = true;
                }
            }

            if !Arc::ptr_eq(&self.power_plan_status, &runtime_status.power_plan_status) {
                self.power_plan_status = runtime_status.power_plan_status;
                changed = true;
            }

            if !Arc::ptr_eq(&self.feature_status, &runtime_status.feature_status) {
                self.feature_status = runtime_status.feature_status;
                changed = true;
            }

            if !Arc::ptr_eq(&self.action_log_entries, &runtime_status.action_log_entries) {
                self.action_log_entries = runtime_status.action_log_entries;
                changed = true;
            }

            if !Arc::ptr_eq(
                &self.action_log_summaries,
                &runtime_status.action_log_summaries,
            ) {
                self.action_log_summaries = runtime_status.action_log_summaries;
                changed = true;
            }

            if self.last_appearance_change_generation != runtime_status.appearance_change_generation
            {
                self.last_appearance_change_generation =
                    runtime_status.appearance_change_generation;
                apply_appearance_settings(&self.settings.general, window, cx);
                changed = true;
            }
        }

        changed |= self.refresh_effective_power_mode();

        let now = Instant::now();

        if self.shell.page == Page::TimerResolution
            && !self.settings.timer_resolution.enabled
            && refresh_due(
                now,
                &mut self.next_timer_resolution_status_refresh,
                TIMER_RESOLUTION_STATUS_REFRESH_INTERVAL,
            )
        {
            let timer_resolution_status =
                timer_resolution::query_snapshot(self.settings.timer_resolution.enabled);
            if self.feature_status.timer_resolution != timer_resolution_status {
                Arc::make_mut(&mut self.feature_status).timer_resolution = timer_resolution_status;
                changed = true;
            }
        }

        if now >= self.next_process_refresh {
            if self.shell.page == Page::ProcessList {
                changed |= self.refresh_running_processes(false, cx);
            } else if self.page_uses_process_candidates() {
                changed |= self.refresh_process_candidates(false, cx);
            }
        }

        if self.shell.page == Page::Home {
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
            self.sync_runtime_settings();
        }
        TickOutcome::Continue { changed }
    }

    pub(in crate::ui::app) fn apply_pending_auto_exclusions(&mut self) -> bool {
        let Some(patch) = self
            .runtime_handle
            .take_auto_exclusion_patch_since(&mut self.last_auto_exclusion_patch_generation)
        else {
            return false;
        };
        match self.settings.apply_auto_exclusion_patch(&patch) {
            Ok(changed) => {
                if changed {
                    self.sync_runtime_settings();
                }
                changed
            }
            Err(error) => {
                self.runtime_handle.requeue_auto_exclusion_patch(patch);
                let message = error.to_string();
                let changed = self.status_message != message;
                self.status_message = message;
                changed
            }
        }
    }
}
