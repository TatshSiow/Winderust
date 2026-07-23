use crate::ui::app::*;

pub(in crate::ui::app) fn input_hook_required(settings: &Settings) -> bool {
    settings.general.enabled
        && (activity_input_hook_required(settings) || app_suspension_input_hook_required(settings))
}

pub(in crate::ui::app) fn input_hook_config(settings: &Settings) -> InputHookConfig {
    let app_suspension = app_suspension_input_hook_required(settings);
    InputHookConfig {
        keyboard: settings.by_activity.input_detection.keyboard || app_suspension,
        mouse: settings.by_activity.input_detection.mouse || app_suspension,
    }
}

pub(in crate::ui::app) fn app_suspension_input_hook_required(settings: &Settings) -> bool {
    settings.app_suspension.enabled && !settings.adaptive_engine.enabled
}

pub(in crate::ui::app) fn activity_input_hook_required(settings: &Settings) -> bool {
    settings.by_activity.enabled
        && settings.by_activity.switch_to_performance_on_resume
        && settings
            .by_activity
            .input_detection
            .keyboard_or_mouse_enabled()
        && settings.by_activity.power_plans.performance_guid.is_some()
}
