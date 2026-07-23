use super::*;

pub(super) struct AutomationSnapshot {
    pub(super) settings: Arc<Settings>,
    pub(super) change_generation: u64,
    pub(super) app_suspension_freeze_requests: Vec<String>,
    pub(super) memory_trim_now_requested: bool,
    pub(super) action_log_clear_requested: bool,
    pub(super) wake_events: AutomationWakeEvents,
    pub(super) windows_event_watcher_active: bool,
}

pub(super) fn automation_snapshot(shared: &SharedAutomationState) -> Option<AutomationSnapshot> {
    shared.state.lock().ok().and_then(|mut state| {
        (!state.stop_requested).then(|| AutomationSnapshot {
            settings: state.settings.clone(),
            change_generation: state.change_generation,
            app_suspension_freeze_requests: std::mem::take(
                &mut state.app_suspension_freeze_requests,
            ),
            memory_trim_now_requested: std::mem::take(&mut state.memory_trim_now_requested),
            action_log_clear_requested: std::mem::take(&mut state.action_log_clear_requested),
            wake_events: std::mem::take(&mut state.pending_events),
            windows_event_watcher_active: state.windows_event_watcher_active,
        })
    })
}

pub(super) fn set_windows_event_watcher_active(shared: &SharedAutomationState, active: bool) {
    if let Ok(mut state) = shared.state.lock() {
        if state.windows_event_watcher_active == active {
            return;
        }

        state.windows_event_watcher_active = active;
        state.change_generation = state.change_generation.wrapping_add(1);
        shared.changed.notify_one();
    }
}

pub(super) fn notify_windows_event(shared: &SharedAutomationState, event: WindowsAutomationEvent) {
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

pub(super) fn notify_input_event(shared: &SharedAutomationState, events: InputHookEvents) {
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

pub(super) fn update_background_efficiency_status(
    shared: &SharedAutomationState,
    status: BackgroundEfficiencySnapshot,
) {
    update_status_with_auto_exclusions(
        shared,
        status.clone(),
        &status.auto_excluded_processes,
        |pending| &mut pending.background_efficiency,
        |state| &mut state.background_efficiency_status,
    );
}

pub(super) fn update_app_suspension_status(
    shared: &SharedAutomationState,
    status: AppSuspensionSnapshot,
) {
    update_status_with_auto_exclusions(
        shared,
        status.clone(),
        &status.auto_excluded_processes,
        |pending| &mut pending.app_suspension,
        |state| &mut state.app_suspension_status,
    );
}

pub(super) fn update_core_steering_status(
    shared: &SharedAutomationState,
    status: CoreSteeringSnapshot,
) {
    update_status_with_auto_exclusions(
        shared,
        status.clone(),
        &status.auto_excluded_processes,
        |pending| &mut pending.core_steering,
        |state| &mut state.core_steering_status,
    );
}

pub(super) fn update_background_cpu_restriction_status(
    shared: &SharedAutomationState,
    status: CoreSteeringSnapshot,
) {
    update_status_with_auto_exclusions(
        shared,
        status.clone(),
        &status.auto_excluded_processes,
        |pending| &mut pending.background_cpu_restriction,
        |state| &mut state.background_cpu_restriction_status,
    );
}

pub(super) fn append_unique_process_names(target: &mut Vec<String>, names: &[String]) -> bool {
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

pub(super) fn update_core_limiter_status(
    shared: &SharedAutomationState,
    status: CoreLimiterSnapshot,
) {
    update_status_with_auto_exclusions(
        shared,
        status.clone(),
        &status.auto_excluded_processes,
        |pending| &mut pending.core_limiter,
        |state| &mut state.core_limiter_status,
    );
}

pub(super) fn update_by_running_app_status(
    shared: &SharedAutomationState,
    status: ByRunningAppSnapshot,
) {
    update_status(shared, status, |state| &mut state.by_running_app_status);
}

pub(super) fn update_workload_engine_status(
    shared: &SharedAutomationState,
    status: WorkloadEngineSnapshot,
) {
    update_status_with_auto_exclusions(
        shared,
        status.clone(),
        &status.auto_excluded_processes,
        |pending| &mut pending.workload_engine,
        |state| &mut state.workload_engine_status,
    );
}

pub(super) fn update_io_priority_status(
    shared: &SharedAutomationState,
    status: IoPrioritySnapshot,
) {
    update_status_with_auto_exclusions(
        shared,
        status.clone(),
        &status.auto_excluded_processes,
        |pending| &mut pending.io_priority,
        |state| &mut state.io_priority_status,
    );
}

pub(super) fn update_process_priority_status(
    shared: &SharedAutomationState,
    status: ProcessPrioritySnapshot,
) {
    update_status_with_auto_exclusions(
        shared,
        status.clone(),
        &status.auto_excluded_processes,
        |pending| &mut pending.process_priority,
        |state| &mut state.process_priority_status,
    );
}

pub(super) fn update_thread_priority_status(
    shared: &SharedAutomationState,
    status: ThreadPrioritySnapshot,
) {
    update_status_with_auto_exclusions(
        shared,
        status.clone(),
        &status.auto_excluded_processes,
        |pending| &mut pending.thread_priority,
        |state| &mut state.thread_priority_status,
    );
}

pub(super) fn update_dynamic_priority_boost_status(
    shared: &SharedAutomationState,
    status: DynamicPriorityBoostSnapshot,
) {
    update_status_with_auto_exclusions(
        shared,
        status.clone(),
        &status.auto_excluded_processes,
        |pending| &mut pending.dynamic_priority_boost,
        |state| &mut state.dynamic_priority_boost_status,
    );
}

pub(super) fn update_gpu_priority_status(
    shared: &SharedAutomationState,
    status: GpuPrioritySnapshot,
) {
    update_status_with_auto_exclusions(
        shared,
        status.clone(),
        &status.auto_excluded_processes,
        |pending| &mut pending.gpu_priority,
        |state| &mut state.gpu_priority_status,
    );
}

pub(super) fn update_memory_priority_status(
    shared: &SharedAutomationState,
    status: MemoryPrioritySnapshot,
) {
    update_status_with_auto_exclusions(
        shared,
        status.clone(),
        &status.auto_excluded_processes,
        |pending| &mut pending.memory_priority,
        |state| &mut state.memory_priority_status,
    );
}

pub(super) fn update_memory_trim_status(
    shared: &SharedAutomationState,
    status: MemoryTrimSnapshot,
) {
    update_status_with_auto_exclusions(
        shared,
        status.clone(),
        &status.auto_excluded_processes,
        |pending| &mut pending.memory_trim,
        |state| &mut state.memory_trim_status,
    );
}

pub(super) fn update_timer_resolution_status(
    shared: &SharedAutomationState,
    status: TimerResolutionSnapshot,
) {
    update_status(shared, status, |state| &mut state.timer_resolution_status);
}

pub(super) fn update_status<T: PartialEq>(
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

pub(super) fn update_status_with_auto_exclusions<T: PartialEq>(
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

pub(super) fn update_action_log_entries(
    shared: &SharedAutomationState,
    entries: Vec<ActionLogEntry>,
) {
    if let Ok(mut state) = shared.state.lock() {
        let entries = Arc::new(entries);
        if state.action_log_entries != entries {
            state.action_log_entries = entries;
            bump_status_generation(shared, &mut state);
        }
    }
}

pub(super) fn set_status<T: PartialEq>(current: &mut T, next: T) -> bool {
    if *current != next {
        *current = next;
        true
    } else {
        false
    }
}

pub(super) fn bump_status_generation(
    shared: &SharedAutomationState,
    state: &mut AutomationWorkerState,
) {
    state.status_generation = state.status_generation.wrapping_add(1);
    shared
        .status_generation
        .store(state.status_generation, Ordering::Release);
}
