use crate::ui::app::*;

impl WinderustApp {
    pub(in crate::ui::app) fn render_by_activity_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.sync_activity_slider_states(window, cx);
        let enabled = self.settings.by_activity.enabled;
        let body = feature_body(enabled)
            .child(setting_action_card(
                "activity-idle-plan-card",
                t!("by_activity.idle_plan").to_string(),
                self.render_inline_power_plan_picker(
                    "activity-idle-plan",
                    self.settings
                        .by_activity
                        .power_plans
                        .power_save_guid
                        .clone(),
                    PowerPlanField::ActivityKind(PowerPlanKind::Idle),
                    window,
                    cx,
                ),
            ))
            .child(setting_action_card(
                "activity-active-plan-card",
                t!("by_activity.active_plan").to_string(),
                self.render_inline_power_plan_picker(
                    "activity-active-plan",
                    self.settings
                        .by_activity
                        .power_plans
                        .performance_guid
                        .clone(),
                    PowerPlanField::ActivityKind(PowerPlanKind::Active),
                    window,
                    cx,
                ),
            ))
            .child(feature_toggle_switch(
                "keyboard-input",
                t!("by_activity.keyboard_input").to_string(),
                self.settings.by_activity.input_detection.keyboard,
                cx.listener(|app, checked: &bool, _, cx| {
                    if !*checked
                        && !app.settings.by_activity.input_detection.mouse
                        && !app.settings.by_activity.input_detection.controller
                    {
                        return;
                    }
                    app.settings.by_activity.input_detection.keyboard = *checked;
                    app.settings
                        .by_activity
                        .input_detection
                        .ensure_any_enabled();
                    app.settings.by_activity.switch_to_performance_on_resume =
                        app.settings.by_activity.input_detection.any_enabled();
                    cx.notify();
                }),
            ))
            .child(feature_toggle_switch(
                "mouse-input",
                t!("by_activity.mouse_input").to_string(),
                self.settings.by_activity.input_detection.mouse,
                cx.listener(|app, checked: &bool, _, cx| {
                    if !*checked
                        && !app.settings.by_activity.input_detection.keyboard
                        && !app.settings.by_activity.input_detection.controller
                    {
                        return;
                    }
                    app.settings.by_activity.input_detection.mouse = *checked;
                    app.settings
                        .by_activity
                        .input_detection
                        .ensure_any_enabled();
                    app.settings.by_activity.switch_to_performance_on_resume =
                        app.settings.by_activity.input_detection.any_enabled();
                    cx.notify();
                }),
            ))
            .child(feature_toggle_switch(
                "controller-input",
                t!("by_activity.controller_input").to_string(),
                self.settings.by_activity.input_detection.controller,
                cx.listener(|app, checked: &bool, _, cx| {
                    if !*checked
                        && !app.settings.by_activity.input_detection.keyboard
                        && !app.settings.by_activity.input_detection.mouse
                    {
                        return;
                    }
                    app.settings.by_activity.input_detection.controller = *checked;
                    app.settings
                        .by_activity
                        .input_detection
                        .ensure_any_enabled();
                    app.settings.by_activity.switch_to_performance_on_resume =
                        app.settings.by_activity.input_detection.any_enabled();
                    cx.notify();
                }),
            ))
            .child(activity_slider_card(
                ActivitySliderCardSpec {
                    id: SharedString::from("activity-idle-timeout"),
                    label: SharedString::from(t!("by_activity.idle_timeout").to_string()),
                    value_element: self.render_numeric_value(
                        NumericField::ActivityIdleTimeout,
                        seconds_label(self.settings.by_activity.idle_timeout_seconds),
                        self.settings.by_activity.idle_timeout_seconds.to_string(),
                        cx,
                    ),
                    state: &self.inputs.activity_idle_timeout,
                    enabled,
                    range: SliderRange {
                        min: ACTIVITY_IDLE_TIMEOUT_MIN_SECONDS,
                        max: ACTIVITY_IDLE_TIMEOUT_MAX_SECONDS,
                        step: 1,
                    },
                },
                window,
                cx,
                cx.listener(|app, change: &StepChange<u64>, _, cx| {
                    let value = apply_u64_step(
                        app.settings.by_activity.idle_timeout_seconds,
                        change,
                        ACTIVITY_IDLE_TIMEOUT_MIN_SECONDS,
                        ACTIVITY_IDLE_TIMEOUT_MAX_SECONDS,
                    );
                    app.set_activity_slider_value(ActivitySlider::IdleTimeout, value);
                    cx.notify();
                }),
            ))
            .child(activity_slider_card(
                ActivitySliderCardSpec {
                    id: SharedString::from("general-check-interval"),
                    label: SharedString::from(t!("by_activity.check_interval").to_string()),
                    value_element: self.render_numeric_value(
                        NumericField::GeneralCheckInterval,
                        milliseconds_label(self.settings.general.check_interval_ms),
                        self.settings.general.check_interval_ms.to_string(),
                        cx,
                    ),
                    state: &self.inputs.activity_check_interval,
                    enabled,
                    range: SliderRange {
                        min: ACTIVITY_CHECK_INTERVAL_MIN_MS,
                        max: ACTIVITY_CHECK_INTERVAL_MAX_MS,
                        step: ACTIVITY_CHECK_INTERVAL_STEP_MS,
                    },
                },
                window,
                cx,
                cx.listener(|app, change: &StepChange<u64>, _, cx| {
                    let value = apply_u64_step(
                        app.settings.general.check_interval_ms,
                        change,
                        ACTIVITY_CHECK_INTERVAL_MIN_MS,
                        ACTIVITY_CHECK_INTERVAL_MAX_MS,
                    );
                    app.set_activity_slider_value(ActivitySlider::CheckInterval, value);
                    cx.notify();
                }),
            ));

        let help = tooltip_lines(vec![
            t!("by_activity.intro_1").to_string(),
            t!("by_activity.intro_2").to_string(),
            t!("common.power_plan_priority").to_string(),
            t!("common.power_plan_pause_priority").to_string(),
        ]);

        self.page_shell(Page::ByActivity, cx)
            .child(feature_toggle_switch_with_help(
                "activity-enabled",
                t!("by_activity.enable").to_string(),
                help,
                enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings.by_activity.enabled = *checked;
                    cx.notify();
                }),
            ))
            .child(disabled_feature_body("activity-body", body, enabled, cx))
            .into_any_element()
    }
}
