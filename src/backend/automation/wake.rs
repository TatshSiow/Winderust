use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WorkerWake {
    Stop,
    Changed,
    Timeout,
}

pub(super) fn wait_for_wake(
    shared: &SharedAutomationState,
    wait_for: Option<Duration>,
    observed_generation: u64,
) -> WorkerWake {
    let Ok(state) = shared.state.lock() else {
        return WorkerWake::Stop;
    };
    if state.stop_requested {
        return WorkerWake::Stop;
    }
    if state.change_generation != observed_generation {
        return WorkerWake::Changed;
    }

    if let Some(wait_for) = wait_for {
        match shared.changed.wait_timeout(state, wait_for) {
            Ok((state, _)) if state.stop_requested => WorkerWake::Stop,
            Ok((state, _)) if state.change_generation != observed_generation => WorkerWake::Changed,
            Ok((_state, timeout)) if timeout.timed_out() => WorkerWake::Timeout,
            Ok((_state, _)) => WorkerWake::Changed,
            Err(_) => WorkerWake::Stop,
        }
    } else {
        match shared.changed.wait(state) {
            Ok(state) if state.stop_requested => WorkerWake::Stop,
            Ok(state) if state.change_generation != observed_generation => WorkerWake::Changed,
            Ok(_) => WorkerWake::Changed,
            Err(_) => WorkerWake::Stop,
        }
    }
}

pub(super) fn input_hook_should_check(settings: &Settings, events: InputHookEvents) -> bool {
    input_hook_should_check_activity(settings, events)
        || input_hook_should_check_app_switch(settings, events)
        || input_hook_should_check_app_switch_mouse_click(settings, events)
}

pub(super) fn input_hook_should_check_activity(
    settings: &Settings,
    events: InputHookEvents,
) -> bool {
    settings.general.enabled
        && settings.by_activity.enabled
        && ((events.keyboard && settings.by_activity.input_detection.keyboard)
            || (events.mouse && settings.by_activity.input_detection.mouse))
}

pub(super) fn input_hook_should_check_app_switch(
    settings: &Settings,
    events: InputHookEvents,
) -> bool {
    settings.general.enabled
        && settings.app_suspension.enabled
        && !settings.adaptive_engine.enabled
        && events.app_switch
}

pub(super) fn input_hook_should_check_app_switch_mouse_click(
    settings: &Settings,
    events: InputHookEvents,
) -> bool {
    settings.general.enabled
        && settings.app_suspension.enabled
        && !settings.adaptive_engine.enabled
        && events.mouse_click
}

pub(super) fn process_ids_have_new_entries(
    known_process_ids: &mut BTreeSet<u32>,
    current_ids: BTreeSet<u32>,
) -> bool {
    let initialized = !known_process_ids.is_empty();
    let has_new_entries = initialized
        && current_ids
            .iter()
            .any(|process_id| !known_process_ids.contains(process_id));
    *known_process_ids = current_ids;
    has_new_entries
}
