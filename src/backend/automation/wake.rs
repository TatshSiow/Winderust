use super::*;

pub(super) fn wait_for_wake(
    shared: &SharedAutomationState,
    wait_for: Option<Duration>,
    observed_generation: u64,
) -> bool {
    let state = lock_unpoisoned(&shared.state);
    if state.stop_requested {
        return true;
    }
    if state.change_generation != observed_generation {
        return false;
    }

    if let Some(wait_for) = wait_for {
        let (state, _) = shared
            .changed
            .wait_timeout(state, wait_for)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.stop_requested
    } else {
        let state = shared
            .changed
            .wait(state)
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.stop_requested
    }
}
pub(super) fn input_hook_should_check_activity(
    settings: &Settings,
    events: InputHookEvents,
) -> bool {
    std::iter::once(settings)
        .chain(settings.on_battery.as_deref())
        .any(|settings| {
            settings.general.enabled
                && activity_power_plan_required(settings)
                && ((events.keyboard && settings.by_activity.input_detection.keyboard)
                    || (events.mouse && settings.by_activity.input_detection.mouse))
        })
}

pub(super) fn input_hook_should_check_app_switch(
    settings: &Settings,
    events: InputHookEvents,
) -> bool {
    std::iter::once(settings)
        .chain(settings.on_battery.as_deref())
        .any(|settings| {
            settings.general.enabled && settings.app_suspension.enabled && events.app_switch
        })
}

pub(super) fn input_hook_should_check_app_switch_mouse_click(
    settings: &Settings,
    events: InputHookEvents,
) -> bool {
    std::iter::once(settings)
        .chain(settings.on_battery.as_deref())
        .any(|settings| {
            settings.general.enabled && settings.app_suspension.enabled && events.mouse_click
        })
}

pub(super) fn process_ids_have_new_entries(
    known_process_ids: &mut Vec<u32>,
    mut current_ids: Vec<u32>,
) -> bool {
    current_ids.retain(|&id| id != 0);
    current_ids.sort_unstable();
    current_ids.dedup();
    if current_ids == *known_process_ids {
        return false;
    }
    let has_new_entries = !known_process_ids.is_empty()
        && current_ids
            .iter()
            .any(|id| known_process_ids.binary_search(id).is_err());
    *known_process_ids = current_ids;
    has_new_entries
}
