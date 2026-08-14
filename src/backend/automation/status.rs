use super::*;

pub(super) struct AutomationSnapshot {
    pub(super) settings: Arc<Settings>,
    pub(super) change_generation: u64,
    pub(super) process_control_commands: VecDeque<ProcessControlCommand>,
    pub(super) action_log_clear_requested: bool,
    pub(super) wake_events: AutomationWakeEvents,
    pub(super) windows_event_watcher_active: bool,
}

pub(super) fn automation_snapshot(shared: &SharedAutomationState) -> Option<AutomationSnapshot> {
    let mut state = lock_unpoisoned(&shared.state);
    (!state.stop_requested).then(|| AutomationSnapshot {
        settings: state.settings.clone(),
        change_generation: state.change_generation,
        process_control_commands: std::mem::take(&mut state.process_control_commands),
        action_log_clear_requested: std::mem::take(&mut state.action_log_clear_requested),
        wake_events: std::mem::take(&mut state.pending_events),
        windows_event_watcher_active: state.windows_event_watcher_active,
    })
}

pub(super) fn set_windows_event_watcher_active(shared: &SharedAutomationState, active: bool) {
    let mut state = lock_unpoisoned(&shared.state);
    if state.windows_event_watcher_active == active {
        return;
    }

    state.windows_event_watcher_active = active;
    state.change_generation = state.change_generation.wrapping_add(1);
    shared.changed.notify_one();
}

pub(super) fn notify_windows_event(shared: &SharedAutomationState, event: WindowsAutomationEvent) {
    let mut state = lock_unpoisoned(&shared.state);
    if state.stop_requested || !windows_event_wake_required(&state.settings, event) {
        return;
    }

    if event == WindowsAutomationEvent::AppearanceChanged {
        state.status.appearance_change_generation =
            state.status.appearance_change_generation.wrapping_add(1);
        bump_status_generation(shared, &mut state);
    }
    #[cfg(feature = "architecture-diagnostics")]
    crate::architecture_diagnostics::record_windows_event(event);
    state.pending_events.insert_windows_event(event);
    state.change_generation = state.change_generation.wrapping_add(1);
    shared.changed.notify_one();
}

pub(super) fn notify_input_event(shared: &SharedAutomationState, events: InputHookEvents) {
    let mut state = lock_unpoisoned(&shared.state);
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
    #[cfg(feature = "architecture-diagnostics")]
    crate::architecture_diagnostics::record_input_notification();
    state.change_generation = state.change_generation.wrapping_add(1);
    shared.changed.notify_one();
}

pub(super) fn update_background_efficiency_status(
    shared: &SharedAutomationState,
    status: BackgroundEfficiencySnapshot,
) {
    update_feature_status(
        shared,
        status,
        |feature_status| &feature_status.background_efficiency,
        |feature_status| &mut feature_status.background_efficiency,
    );
}

pub(super) fn update_app_suspension_status(
    shared: &SharedAutomationState,
    status: AppSuspensionSnapshot,
) {
    update_status_with_auto_exclusions(
        shared,
        status,
        |status| &status.auto_excluded_processes,
        |pending, path_list| append_unique_executable_paths(path_list, &mut pending.app_suspension),
        |feature_status| &feature_status.app_suspension,
        |feature_status| &mut feature_status.app_suspension,
    );
}

pub(super) fn update_cpu_sets_soft_status(
    shared: &SharedAutomationState,
    status: CpuAllocationSnapshot,
) {
    update_status_with_auto_exclusions(
        shared,
        status,
        |status| &status.auto_excluded_processes,
        |pending, path_list| append_unique_executable_paths(path_list, &mut pending.cpu_sets_soft),
        |feature_status| &feature_status.cpu_sets_soft,
        |feature_status| &mut feature_status.cpu_sets_soft,
    );
}

pub(super) fn update_processor_affinity_hard_status(
    shared: &SharedAutomationState,
    status: CpuAllocationSnapshot,
) {
    update_status_with_auto_exclusions(
        shared,
        status,
        |status| &status.auto_excluded_processes,
        |pending, path_list| {
            append_unique_executable_paths(path_list, &mut pending.processor_affinity_hard)
        },
        |feature_status| &feature_status.processor_affinity_hard,
        |feature_status| &mut feature_status.processor_affinity_hard,
    );
}

fn append_unique_executable_paths(executable_paths: &[String], target: &mut Vec<String>) -> bool {
    let old_len = target.len();
    for executable_path in executable_paths {
        let executable_path = executable_path_key(Path::new(executable_path));
        if Path::new(&executable_path).is_absolute()
            && !target.iter().any(|existing| {
                same_executable_path(Path::new(existing), Path::new(&executable_path))
            })
        {
            target.push(executable_path);
        }
    }
    target.len() != old_len
}

pub(super) fn merge_auto_exclusion_patch(
    target: &mut AutoExclusionPatch,
    incoming: AutoExclusionPatch,
) -> bool {
    let mut changed = false;
    changed |= append_unique_executable_paths(&incoming.app_suspension, &mut target.app_suspension);
    changed |= append_unique_executable_paths(&incoming.cpu_sets_soft, &mut target.cpu_sets_soft);
    changed |= append_unique_executable_paths(
        &incoming.processor_affinity_hard,
        &mut target.processor_affinity_hard,
    );
    changed |= append_unique_executable_paths(&incoming.core_limiter, &mut target.core_limiter);
    changed |=
        append_unique_executable_paths(&incoming.workload_engine, &mut target.workload_engine);
    changed |= append_unique_executable_paths(&incoming.io_priority, &mut target.io_priority);
    changed |=
        append_unique_executable_paths(&incoming.process_priority, &mut target.process_priority);
    changed |=
        append_unique_executable_paths(&incoming.thread_priority, &mut target.thread_priority);
    changed |= append_unique_executable_paths(
        &incoming.dynamic_priority_boost,
        &mut target.dynamic_priority_boost,
    );
    changed |= append_unique_executable_paths(&incoming.gpu_priority, &mut target.gpu_priority);
    changed |=
        append_unique_executable_paths(&incoming.memory_priority, &mut target.memory_priority);
    changed |= append_unique_executable_paths(&incoming.memory_trim, &mut target.memory_trim);
    changed
}

pub(super) fn update_core_limiter_status(
    shared: &SharedAutomationState,
    status: CoreLimiterSnapshot,
) {
    update_status_with_auto_exclusions(
        shared,
        status,
        |status| &status.auto_excluded_processes,
        |pending, path_list| append_unique_executable_paths(path_list, &mut pending.core_limiter),
        |feature_status| &feature_status.core_limiter,
        |feature_status| &mut feature_status.core_limiter,
    );
}

pub(super) fn update_by_running_app_status(
    shared: &SharedAutomationState,
    status: ByRunningAppSnapshot,
) {
    update_feature_status(
        shared,
        status,
        |feature_status| &feature_status.by_running_app,
        |feature_status| &mut feature_status.by_running_app,
    );
}

pub(super) fn update_workload_engine_status(
    shared: &SharedAutomationState,
    status: WorkloadEngineSnapshot,
) {
    update_status_with_auto_exclusions(
        shared,
        status,
        |status| &status.auto_excluded_processes,
        |pending, path_list| {
            append_unique_executable_paths(path_list, &mut pending.workload_engine)
        },
        |feature_status| &feature_status.workload_engine,
        |feature_status| &mut feature_status.workload_engine,
    );
}

pub(super) fn update_io_priority_status(
    shared: &SharedAutomationState,
    status: IoPrioritySnapshot,
) {
    update_status_with_auto_exclusions(
        shared,
        status,
        |status| &status.auto_excluded_processes,
        |pending, path_list| append_unique_executable_paths(path_list, &mut pending.io_priority),
        |feature_status| &feature_status.io_priority,
        |feature_status| &mut feature_status.io_priority,
    );
}

pub(super) fn update_process_priority_status(
    shared: &SharedAutomationState,
    status: ProcessPrioritySnapshot,
) {
    update_status_with_auto_exclusions(
        shared,
        status,
        |status| &status.auto_excluded_processes,
        |pending, path_list| {
            append_unique_executable_paths(path_list, &mut pending.process_priority)
        },
        |feature_status| &feature_status.process_priority,
        |feature_status| &mut feature_status.process_priority,
    );
}

pub(super) fn update_thread_priority_status(
    shared: &SharedAutomationState,
    status: ThreadPrioritySnapshot,
) {
    update_status_with_auto_exclusions(
        shared,
        status,
        |status| &status.auto_excluded_processes,
        |pending, path_list| {
            append_unique_executable_paths(path_list, &mut pending.thread_priority)
        },
        |feature_status| &feature_status.thread_priority,
        |feature_status| &mut feature_status.thread_priority,
    );
}

pub(super) fn update_dynamic_priority_boost_status(
    shared: &SharedAutomationState,
    status: DynamicPriorityBoostSnapshot,
) {
    update_status_with_auto_exclusions(
        shared,
        status,
        |status| &status.auto_excluded_processes,
        |pending, path_list| {
            append_unique_executable_paths(path_list, &mut pending.dynamic_priority_boost)
        },
        |feature_status| &feature_status.dynamic_priority_boost,
        |feature_status| &mut feature_status.dynamic_priority_boost,
    );
}

pub(super) fn update_gpu_priority_status(
    shared: &SharedAutomationState,
    status: GpuPrioritySnapshot,
) {
    update_status_with_auto_exclusions(
        shared,
        status,
        |status| &status.auto_excluded_processes,
        |pending, path_list| append_unique_executable_paths(path_list, &mut pending.gpu_priority),
        |feature_status| &feature_status.gpu_priority,
        |feature_status| &mut feature_status.gpu_priority,
    );
}

pub(super) fn update_memory_priority_status(
    shared: &SharedAutomationState,
    status: MemoryPrioritySnapshot,
) {
    update_status_with_auto_exclusions(
        shared,
        status,
        |status| &status.auto_excluded_processes,
        |pending, path_list| {
            append_unique_executable_paths(path_list, &mut pending.memory_priority)
        },
        |feature_status| &feature_status.memory_priority,
        |feature_status| &mut feature_status.memory_priority,
    );
}

pub(super) fn update_memory_trim_status(
    shared: &SharedAutomationState,
    status: MemoryTrimSnapshot,
) {
    update_status_with_auto_exclusions(
        shared,
        status,
        |status| &status.auto_excluded_processes,
        |pending, path_list| append_unique_executable_paths(path_list, &mut pending.memory_trim),
        |feature_status| &feature_status.memory_trim,
        |feature_status| &mut feature_status.memory_trim,
    );
}

pub(super) fn update_timer_resolution_status(
    shared: &SharedAutomationState,
    status: TimerResolutionSnapshot,
) {
    update_feature_status(
        shared,
        status,
        |feature_status| &feature_status.timer_resolution,
        |feature_status| &mut feature_status.timer_resolution,
    );
}

pub(super) fn update_worker_error(shared: &SharedAutomationState, error: Option<String>) {
    let mut state = lock_unpoisoned(&shared.state);
    if state.status.worker_error != error {
        state.status.worker_error = error;
        bump_status_generation(shared, &mut state);
    }
}

pub(super) fn update_power_plan_status(shared: &SharedAutomationState, status: PowerPlanStatus) {
    let mut state = lock_unpoisoned(&shared.state);
    if state.status.power_plan_status.as_ref() == &status {
        return;
    }
    state.status.power_plan_status = Arc::new(status);
    bump_status_generation(shared, &mut state);
}

pub(super) fn update_feature_status<T: PartialEq>(
    shared: &SharedAutomationState,
    status: T,
    current: impl for<'a> FnOnce(&'a RuntimeFeatureStatus) -> &'a T,
    field: impl for<'a> FnOnce(&'a mut RuntimeFeatureStatus) -> &'a mut T,
) {
    let mut state = lock_unpoisoned(&shared.state);
    if current(state.status.feature_status.as_ref()) == &status {
        return;
    }
    *field(Arc::make_mut(&mut state.status.feature_status)) = status;
    bump_status_generation(shared, &mut state);
}

pub(super) fn update_status_with_auto_exclusions<T: PartialEq>(
    shared: &SharedAutomationState,
    status: T,
    auto_excluded_processes: impl for<'a> FnOnce(&'a T) -> &'a [String],
    pending_update: impl for<'a> FnOnce(&'a mut AutoExclusionPatch, &[String]) -> bool,
    current_status: impl for<'a> FnOnce(&'a RuntimeFeatureStatus) -> &'a T,
    status_field: impl for<'a> FnOnce(&'a mut RuntimeFeatureStatus) -> &'a mut T,
) {
    let mut state = lock_unpoisoned(&shared.state);
    if pending_update(
        &mut state.pending_auto_exclusions,
        auto_excluded_processes(&status),
    ) {
        let persisted_revision = state.persisted_revision;
        let base_revision = *state
            .pending_auto_exclusions_revision
            .get_or_insert(persisted_revision);
        state.pending_auto_exclusions.base_revision = base_revision;
        shared
            .pending_auto_exclusions_generation
            .fetch_add(1, Ordering::Release);
    }

    if current_status(state.status.feature_status.as_ref()) == &status {
        return;
    }
    *status_field(Arc::make_mut(&mut state.status.feature_status)) = status;
    bump_status_generation(shared, &mut state);
}

pub(super) fn update_action_log(
    shared: &SharedAutomationState,
    entries: Vec<ActionLogEntry>,
    summaries: ActionLogSummaries,
) {
    let mut state = lock_unpoisoned(&shared.state);
    let entries = Arc::new(entries);
    let summaries = Arc::new(summaries);
    if state.status.action_log_entries == entries && state.status.action_log_summaries == summaries
    {
        return;
    }
    state.status.action_log_entries = entries;
    state.status.action_log_summaries = summaries;
    bump_status_generation(shared, &mut state);
}

pub(super) fn bump_status_generation(
    shared: &SharedAutomationState,
    state: &mut AutomationWorkerState,
) {
    state.status.generation = state.status.generation.wrapping_add(1);
    shared
        .status_generation
        .store(state.status.generation, Ordering::Release);
}
