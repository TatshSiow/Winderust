use super::*;

pub(super) fn wait_for_wake(
    shared: &SharedAutomationState,
    wait_for: Option<Duration>,
    observed_generation: u64,
) -> bool {
    let Ok(state) = shared.state.lock() else {
        return true;
    };
    if state.stop_requested {
        return true;
    }
    if state.change_generation != observed_generation {
        return false;
    }

    if let Some(wait_for) = wait_for {
        shared
            .changed
            .wait_timeout(state, wait_for)
            .map_or(true, |(state, _)| state.stop_requested)
    } else {
        shared
            .changed
            .wait(state)
            .map_or(true, |state| state.stop_requested)
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
