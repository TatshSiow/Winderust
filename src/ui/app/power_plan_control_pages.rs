use super::*;

impl WinderustApp {
    pub(super) fn render_by_activity_page(
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

    pub(super) fn render_by_foreground_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let input_value = self.inputs.foreground_process.read(cx).value().to_string();
        let enabled = self.settings.by_foreground.enabled;
        let help = tooltip_lines(vec![
            t!("by_foreground.intro_1").to_string(),
            t!("by_foreground.intro_2").to_string(),
            t!("common.power_plan_priority").to_string(),
            t!("common.power_plan_pause_priority").to_string(),
        ]);
        let mut content =
            self.page_shell(Page::ByForeground, cx)
                .child(feature_toggle_switch_with_help(
                    "foreground-enabled",
                    t!("by_foreground.enable").to_string(),
                    help,
                    enabled,
                    cx.listener(|app, checked, _, cx| {
                        app.settings.by_foreground.enabled = *checked;
                        cx.notify();
                    }),
                ));

        let mut body =
            feature_body(enabled).child(section_title_text(t!("common.rules").to_string()));
        body = body.child(
            h_flex()
                .gap_2()
                .items_start()
                .flex_wrap()
                .child(self.render_process_picker(
                    "foreground-suggestion",
                    &self.inputs.foreground_process,
                    SuggestionTarget::Foreground,
                    window,
                    cx,
                ))
                .child(
                    primary_control_button(Button::new("add-by-foreground"), cx)
                        .label(t!("common.add").to_string())
                        .disabled(
                            !self.settings.by_foreground.enabled
                                || !can_add_foreground_process(
                                    &self.settings.by_foreground,
                                    &input_value,
                                ),
                        )
                        .on_click(cx.listener(|app, _, window, cx| {
                            let process =
                                app.inputs.foreground_process.read(cx).value().to_string();
                            if can_add_foreground_process(&app.settings.by_foreground, &process) {
                                app.settings
                                    .by_foreground
                                    .rules
                                    .push(app.new_foreground_rule(&process));
                                app.inputs.ensure_for_settings(window, cx, &app.settings);
                                clear_input(&app.inputs.foreground_process, window, cx);
                            }
                            cx.notify();
                        })),
                ),
        );
        let mut rules = rule_list(vec![
            rule_table_active_header(),
            rule_table_title_header(t!("process_list.process_name").to_string()),
            priority_exclusion_table_cell(t!("by_running_app.power_plan").to_string()),
            rule_table_action_header(),
        ]);
        for (index, rule) in self.settings.by_foreground.rules.iter().enumerate() {
            rules = rules.child(self.animated_list_item(
                ListItemRemovalTarget::new(ListItemRemovalKind::ByForegroundRule, index),
                SharedString::from(format!("by-foreground-{index}")),
                self.render_foreground_rule(index, rule, window, cx),
            ));
        }
        if self.settings.by_foreground.rules.is_empty() {
            rules = rules.child(text_muted(t!("common.no_custom_rules").to_string()).p_4());
        }
        body = body.child(rules);
        content = content.child(disabled_feature_body(
            "by-foreground-body",
            body,
            enabled,
            cx,
        ));

        content.into_any_element()
    }

    pub(super) fn render_foreground_rule(
        &self,
        index: usize,
        rule: &ByForegroundRule,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        compact_rule_row(format!("by-foreground-row-{index}"))
            .child(rule_active_cell(
                format!("by-foreground-enabled-{index}"),
                rule.enabled,
                cx.listener(move |app, checked, _, cx| {
                    if let Some(rule) = app.settings.by_foreground.rules.get_mut(index) {
                        rule.enabled = *checked;
                    }
                    cx.notify();
                }),
            ))
            .child(self.process_rule_title(&rule.process_name, cx))
            .child(self.render_inline_power_plan_picker(
                format!("by-foreground-plan-{index}"),
                rule.power_plan_guid.clone(),
                PowerPlanField::ByForegroundRule(index),
                window,
                cx,
            ))
            .child(rule_table_action_cell(
                remove_control_button(Button::new(SharedString::from(format!(
                    "remove-by-foreground-{index}"
                ))))
                .on_click(cx.listener(move |app, _, _, cx| {
                    app.request_list_item_removal(
                        ListItemRemovalTarget::new(ListItemRemovalKind::ByForegroundRule, index),
                        cx,
                    );
                }))
                .into_any_element(),
            ))
            .into_any_element()
    }

    pub(super) fn new_foreground_rule(&self, process: &str) -> ByForegroundRule {
        new_foreground_rule(
            process,
            self.current_plan.as_ref().map(|plan| plan.guid.clone()),
        )
    }

    pub(super) fn new_by_running_app_rule(&self, process: &str) -> ByRunningAppRule {
        new_by_running_app_rule(
            process,
            self.current_plan.as_ref().map(|plan| plan.guid.clone()),
        )
    }

    pub(super) fn render_rule_title(
        &self,
        title: &str,
        input: &Entity<InputState>,
        target: RuleTitleTarget,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if self.editing_rule_title == Some(target) {
            return h_flex()
                .id(SharedString::from(format!("rule-title-editor-{target:?}")))
                .flex_1()
                .min_w(px(180.0))
                .max_w(px(460.0))
                .items_center()
                .gap_2()
                .on_click(|_, _, cx| {
                    cx.stop_propagation();
                })
                .on_action(cx.listener(move |app, _: &InputEscape, _, cx| {
                    app.finish_rule_title_edit(target, cx);
                }))
                .on_mouse_down_out(cx.listener(move |app, _: &gpui::MouseDownEvent, _, cx| {
                    app.finish_rule_title_edit(target, cx);
                }))
                .child(app_input(input, true, cx))
                .child(
                    primary_control_button(
                        Button::new(SharedString::from(format!(
                            "finish-rule-title-edit-{target:?}"
                        ))),
                        cx,
                    )
                    .label(t!("common.done").to_string())
                    .on_click(cx.listener(move |app, _, _, cx| {
                        app.finish_rule_title_edit(target, cx);
                    })),
                )
                .into_any_element();
        }

        h_flex()
            .flex_1()
            .min_w(px(0.0))
            .overflow_hidden()
            .items_center()
            .gap_1()
            .child(
                div()
                    .id(SharedString::from(format!("rule-title-{target:?}")))
                    .flex_1()
                    .min_w(px(0.0))
                    .max_w(px(420.0))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_size(px(RULE_TITLE_TEXT_SIZE))
                    .line_height(px(RULE_TITLE_LINE_HEIGHT))
                    .cursor_pointer()
                    .child(title.to_owned()),
            )
            .into_any_element()
    }

    pub(super) fn render_by_time_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let enabled = self.settings.by_time.enabled;
        let help = tooltip_lines(vec![
            t!("by_time.intro_1").to_string(),
            t!("by_time.intro_2").to_string(),
            t!("common.power_plan_priority").to_string(),
            t!("common.power_plan_pause_priority").to_string(),
        ]);
        let mut content = self
            .page_shell(Page::ByTime, cx)
            .child(feature_toggle_switch_with_help(
                "schedule-enabled",
                t!("by_time.enable").to_string(),
                help,
                enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings.by_time.enabled = *checked;
                    cx.notify();
                }),
            ));

        let mut body =
            feature_body(enabled).child(section_title_text(t!("common.rules").to_string()));
        body = body.child(create_rule_card(
            "create-time-rule-card",
            t!("by_time.rule_title").to_string(),
            primary_control_button(Button::new("add-time-rule"), cx)
                .label(t!("common.create").to_string())
                .disabled(!enabled)
                .on_click(cx.listener(|app, _, window, cx| {
                    app.settings.by_time.rules.push(ByTimeRule {
                        enabled: true,
                        name: t!("by_time.new_rule").to_string(),
                        days: WeekdaySetting::all().to_vec(),
                        start_time: "22:00".to_owned(),
                        end_time: "08:00".to_owned(),
                        power_plan_guid: app.current_plan.as_ref().map(|plan| plan.guid.clone()),
                    });
                    app.inputs.ensure_for_settings(window, cx, &app.settings);
                    cx.notify();
                }))
                .into_any_element(),
        ));
        let mut rules = rule_list(vec![
            rule_table_active_header(),
            rule_table_title_input_header(t!("common.rule_name").to_string()),
            priority_exclusion_table_cell(t!("by_time.days").to_string()),
            rule_table_centered_header(t!("by_time.start").to_string(), 96.0),
            rule_table_centered_header(t!("by_time.end").to_string(), 96.0),
            rule_table_centered_header(
                t!("by_time.target_power_plan").to_string(),
                DROPDOWN_SELECT_STANDARD_WIDTH,
            ),
            rule_table_action_header(),
        ]);
        for (index, rule) in self.settings.by_time.rules.iter().enumerate() {
            rules = rules.child(self.animated_list_item(
                ListItemRemovalTarget::new(ListItemRemovalKind::ByTimeRule, index),
                SharedString::from(format!("schedule-rule-{index}")),
                self.render_by_time_rule(index, rule, window, cx),
            ));
        }
        if self.settings.by_time.rules.is_empty() {
            rules = rules.child(text_muted(t!("common.no_custom_rules").to_string()).p_4());
        }
        body = body.child(rules);
        content = content.child(disabled_feature_body("schedule-body", body, enabled, cx));

        content.into_any_element()
    }

    pub(super) fn render_by_time_rule(
        &self,
        index: usize,
        rule: &ByTimeRule,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(name_input) = self.inputs.by_time_rule_names.get(index).cloned() else {
            return syncing_rule_card(index);
        };
        let name_focused = name_input.read(cx).focus_handle(cx).is_focused(window);
        let start_input = self.inputs.schedule_start_times.get(index).cloned();
        let end_input = self.inputs.schedule_end_times.get(index).cloned();

        compact_rule_row(format!("schedule-rule-row-{index}"))
            .child(rule_active_cell(
                format!("schedule-rule-enabled-{index}"),
                rule.enabled,
                cx.listener(move |app, checked, _, cx| {
                    if let Some(rule) = app.settings.by_time.rules.get_mut(index) {
                        rule.enabled = *checked;
                    }
                    cx.notify();
                }),
            ))
            .child(rule_table_title_input_cell(app_input(
                &name_input,
                name_focused,
                cx,
            )))
            .child(self.render_by_time_days_dropdown(index, &rule.days, window, cx))
            .child(match start_input {
                Some(input) => rule_table_input_cell(input, 96.0, window, cx).into_any_element(),
                None => text_muted(t!("common.unknown").to_string()).into_any_element(),
            })
            .child(match end_input {
                Some(input) => rule_table_input_cell(input, 96.0, window, cx).into_any_element(),
                None => text_muted(t!("common.unknown").to_string()).into_any_element(),
            })
            .child(self.render_inline_power_plan_picker(
                format!("schedule-rule-plan-{index}"),
                rule.power_plan_guid.clone(),
                PowerPlanField::ByTimeRule(index),
                window,
                cx,
            ))
            .child(rule_table_action_cell(
                remove_control_button(Button::new(SharedString::from(format!(
                    "remove-schedule-rule-{index}"
                ))))
                .on_click(cx.listener(move |app, _, _, cx| {
                    app.request_list_item_removal(
                        ListItemRemovalTarget::new(ListItemRemovalKind::ByTimeRule, index),
                        cx,
                    );
                }))
                .into_any_element(),
            ))
            .into_any_element()
    }

    pub(super) fn render_by_time_days_dropdown(
        &self,
        index: usize,
        days: &[WeekdaySetting],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.render_dropdown_select(
            format!("schedule-days-{index}"),
            schedule_days_label(days),
            true,
            DropdownSelectWidth::Table,
            WeekdaySetting::all().len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for day in WeekdaySetting::all() {
                    let selected = days.contains(&day);
                    let option_id = SharedString::from(format!("schedule-days-{index}-{day:?}"));
                    let motion_id = format!("checkbox-{option_id}");
                    let progress = control_motion_progress(&motion_id, selected);
                    options = options.child(
                        h_flex()
                            .id(option_id.clone())
                            .relative()
                            .min_h(px(40.0))
                            .items_center()
                            .gap_2()
                            .pl_3()
                            .pr_3()
                            .rounded(px(BRAND_RADIUS_CONTROL))
                            .text_size(px(TEXT_CONTROL_SIZE))
                            .line_height(px(TEXT_CONTROL_LINE_HEIGHT))
                            .text_color(cx.theme().popover_foreground)
                            .hover(|style| style.bg(rgb(dropdown_option_hover_color())))
                            .cursor_pointer()
                            .child(checkbox_box(
                                SharedString::from(format!("{option_id}-box")),
                                16.0,
                                SharedString::from(format!("{option_id}-mark")),
                                accent_glyph_color(accent_color()),
                                progress,
                            ))
                            .child(weekday_short_label(day))
                            .on_click(cx.listener(move |app, _, _, cx| {
                                if let Some(rule) = app.settings.by_time.rules.get_mut(index) {
                                    let next = !rule.days.contains(&day);
                                    begin_control_motion(motion_id.clone(), next, cx);
                                    if next {
                                        rule.days.push(day);
                                    } else {
                                        rule.days.retain(|existing| *existing != day);
                                    }
                                }
                                cx.notify();
                            })),
                    );
                }
                options
            },
        )
    }

    pub(super) fn render_by_cpu_load_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.sync_cpu_threshold_slider_states(window, cx);

        let enabled = self.settings.by_cpu_load.enabled;
        let help = tooltip_lines(vec![
            t!("by_cpu_load.intro_1").to_string(),
            t!("by_cpu_load.intro_2").to_string(),
            t!("common.power_plan_priority").to_string(),
            t!("common.power_plan_pause_priority").to_string(),
        ]);
        let mut content =
            self.page_shell(Page::ByCpuLoad, cx)
                .child(feature_toggle_switch_with_help(
                    "cpu-usage-enabled",
                    t!("by_cpu_load.enable").to_string(),
                    help,
                    enabled,
                    cx.listener(|app, checked, _, cx| {
                        app.settings.by_cpu_load.enabled = *checked;
                        cx.notify();
                    }),
                ));

        let mut body =
            feature_body(enabled).child(section_title_text(t!("common.rules").to_string()));
        body = body.child(create_rule_card(
            "create-cpu-rule-card",
            t!("by_cpu_load.rule_title").to_string(),
            primary_control_button(Button::new("add-cpu-rule"), cx)
                .label(t!("common.create").to_string())
                .disabled(!enabled)
                .on_click(cx.listener(|app, _, window, cx| {
                    app.settings.by_cpu_load.rules.push(ByCpuLoadRule {
                        enabled: true,
                        name: t!("by_cpu_load.new_rule").to_string(),
                        comparison: CpuUsageComparison::AtOrBelow,
                        threshold_percent: 20,
                        upper_threshold_percent: None,
                        duration_seconds: 30,
                        power_plan_guid: app.current_plan.as_ref().map(|plan| plan.guid.clone()),
                        else_enabled: false,
                        else_power_plan_guid: app
                            .current_plan
                            .as_ref()
                            .map(|plan| plan.guid.clone()),
                    });
                    app.inputs.ensure_for_settings(window, cx, &app.settings);
                    cx.notify();
                }))
                .into_any_element(),
        ));
        let mut rules = rule_list(vec![
            rule_table_active_header(),
            rule_table_title_header(t!("common.rule_name").to_string()),
            rule_table_action_header(),
        ]);
        for (index, rule) in self.settings.by_cpu_load.rules.iter().enumerate() {
            rules = rules.child(self.animated_list_item(
                ListItemRemovalTarget::new(ListItemRemovalKind::ByCpuLoadRule, index),
                SharedString::from(format!("cpu-rule-{index}")),
                self.render_by_cpu_load_rule(index, rule, enabled, window, cx),
            ));
        }
        if self.settings.by_cpu_load.rules.is_empty() {
            rules = rules.child(text_muted(t!("common.no_custom_rules").to_string()).p_4());
        }
        body = body.child(rules);
        content = content.child(disabled_feature_body("cpu-usage-body", body, enabled, cx));

        content.into_any_element()
    }

    pub(super) fn render_by_cpu_load_rule(
        &self,
        index: usize,
        rule: &ByCpuLoadRule,
        feature_enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(name_input) = self.inputs.by_cpu_load_rule_names.get(index).cloned() else {
            return syncing_rule_card(index);
        };
        let Some(threshold_state) = self.inputs.cpu_rule_thresholds.get(index).cloned() else {
            return syncing_rule_card(index);
        };
        let Some(upper_threshold_state) = self.inputs.cpu_rule_upper_thresholds.get(index).cloned()
        else {
            return syncing_rule_card(index);
        };
        let comparison_options = [
            CpuUsageComparison::AtOrBelow,
            CpuUsageComparison::AtOrAbove,
            CpuUsageComparison::Between,
        ];
        let selected_comparison = rule.comparison;
        let comparison_dropdown = self.render_dropdown_select(
            format!("cpu-comparison-{index}"),
            cpu_usage_comparison_label(selected_comparison),
            true,
            DropdownSelectWidth::Wide,
            comparison_options.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for comparison in comparison_options {
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!(
                                "cpu-comparison-{index}-option-{comparison:?}"
                            )),
                            cpu_usage_comparison_label(comparison),
                            selected_comparison == comparison,
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            if let Some(rule) = app.settings.by_cpu_load.rules.get_mut(index) {
                                rule.comparison = comparison;
                                if comparison == CpuUsageComparison::Between {
                                    rule.upper_threshold_percent.get_or_insert(100);
                                }
                            }
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                options
            },
        );

        let upper = rule.upper_threshold_percent.unwrap_or(100);
        let title_target = RuleTitleTarget::ByCpuLoad(index);
        let card_target = RuleCardTarget::ByCpuLoad(index);
        let collapsed = self.is_rule_card_collapsed(&card_target);
        let mut card = rule_card(
            self.render_rule_title(&rule_card_title(&rule.name), &name_input, title_target, cx),
            rule_active_cell(
                format!("cpu-rule-enabled-{index}"),
                rule.enabled,
                cx.listener(move |app, checked, _, cx| {
                    if let Some(rule) = app.settings.by_cpu_load.rules.get_mut(index) {
                        rule.enabled = *checked;
                    }
                    cx.notify();
                }),
            ),
            rule_card_collapse_indicator(card_target.clone(), collapsed),
            card_target.clone(),
            collapsed,
            cx,
        );
        if rule_card_body_visible(&card_target, collapsed, window) {
            let mut condition_fields =
                vec![
                    rule_action_row(
                        format!("cpu-rule-comparison-{index}"),
                        t!("by_cpu_load.when_cpu_load").to_string(),
                        comparison_dropdown,
                    )
                    .into_any_element(),
                    threshold_level_slider(
                        SliderRowSpec {
                            id: SharedString::from(format!("cpu-rule-threshold-{index}")),
                            label: SharedString::from(t!("by_cpu_load.threshold").to_string()),
                            value_element: self.render_numeric_value(
                                NumericField::CpuThreshold(index),
                                format!("{}%", rule.threshold_percent),
                                rule.threshold_percent.to_string(),
                                cx,
                            ),
                            state: &threshold_state,
                            enabled: feature_enabled,
                            delta: 1_u8,
                        },
                        window,
                        cx,
                        cx.listener(move |app, change: &StepChange<u8>, _, cx| {
                            if let Some(value) =
                                app.settings.by_cpu_load.rules.get(index).map(|rule| {
                                    apply_u8_step(rule.threshold_percent, change, 0, 100)
                                })
                            {
                                app.set_cpu_threshold_slider_value(
                                    CpuThresholdSlider::Lower(index),
                                    value,
                                );
                            }
                            cx.notify();
                        }),
                    )
                    .into_any_element(),
                ];
            if rule.comparison == CpuUsageComparison::Between {
                condition_fields.push(
                    threshold_level_slider(
                        SliderRowSpec {
                            id: SharedString::from(format!("cpu-rule-upper-threshold-{index}")),
                            label: SharedString::from(
                                t!("by_cpu_load.upper_threshold").to_string(),
                            ),
                            value_element: self.render_numeric_value(
                                NumericField::CpuUpperThreshold(index),
                                format!("{upper}%"),
                                upper.to_string(),
                                cx,
                            ),
                            state: &upper_threshold_state,
                            enabled: feature_enabled,
                            delta: 1_u8,
                        },
                        window,
                        cx,
                        cx.listener(move |app, change: &StepChange<u8>, _, cx| {
                            if let Some(value) =
                                app.settings.by_cpu_load.rules.get(index).map(|rule| {
                                    apply_u8_step(
                                        rule.upper_threshold_percent.unwrap_or(100),
                                        change,
                                        0,
                                        100,
                                    )
                                })
                            {
                                app.set_cpu_threshold_slider_value(
                                    CpuThresholdSlider::Upper(index),
                                    value,
                                );
                            }
                            cx.notify();
                        }),
                    )
                    .into_any_element(),
                );
            }
            condition_fields.push(
                rule_stepper_row_u64(
                    format!("cpu-rule-duration-{index}"),
                    t!("by_cpu_load.duration").to_string(),
                    rule.duration_seconds,
                    self.render_numeric_value(
                        NumericField::CpuDuration(index),
                        ui::duration_label(rule.duration_seconds),
                        rule.duration_seconds.to_string(),
                        cx,
                    ),
                    cx.listener(move |app, change: &StepChange<u64>, _, cx| {
                        if let Some(rule) = app.settings.by_cpu_load.rules.get_mut(index) {
                            rule.duration_seconds =
                                apply_u64_step(rule.duration_seconds, change, 0, 86_400);
                        }
                        cx.notify();
                    }),
                )
                .into_any_element(),
            );

            let mut plan_fields = vec![
                rule_action_row(
                    format!("cpu-rule-plan-{index}"),
                    t!("by_cpu_load.use").to_string(),
                    self.render_inline_power_plan_picker(
                        format!("cpu-rule-plan-{index}"),
                        rule.power_plan_guid.clone(),
                        PowerPlanField::CpuRule(index),
                        window,
                        cx,
                    ),
                )
                .into_any_element(),
                rule_checkbox_row(
                    format!("cpu-rule-else-{index}"),
                    t!("by_cpu_load.else").to_string(),
                    rule.else_enabled,
                    cx.listener(move |app, checked, _, cx| {
                        let current_plan = app.current_plan.as_ref().map(|plan| plan.guid.clone());
                        if let Some(rule) = app.settings.by_cpu_load.rules.get_mut(index) {
                            rule.else_enabled = *checked;
                            if rule.else_enabled && rule.else_power_plan_guid.is_none() {
                                rule.else_power_plan_guid = current_plan;
                            }
                        }
                        cx.notify();
                    }),
                ),
            ];
            if rule.else_enabled {
                plan_fields.push(
                    rule_action_row(
                        format!("cpu-rule-else-plan-{index}"),
                        t!("by_cpu_load.else_use").to_string(),
                        self.render_inline_power_plan_picker(
                            format!("cpu-rule-else-plan-{index}"),
                            rule.else_power_plan_guid.clone(),
                            PowerPlanField::CpuRuleElse(index),
                            window,
                            cx,
                        ),
                    )
                    .into_any_element(),
                );
            }
            let condition_row_count = condition_fields.len();
            let plan_row_count = plan_fields.len();

            card = card
                .child(animated_rule_card_body_child(
                    &card_target,
                    0,
                    condition_row_count,
                    rule_card_body_row(condition_fields),
                ))
                .child(animated_rule_card_body_child(
                    &card_target,
                    1,
                    plan_row_count,
                    rule_card_body_row(plan_fields),
                ))
                .child(animated_rule_card_body_child(
                    &card_target,
                    2,
                    1,
                    rule_card_body_actions(vec![
                        rename_rule_button(title_target, cx),
                        remove_control_button(Button::new(SharedString::from(format!(
                            "remove-cpu-rule-{index}"
                        ))))
                        .on_click(cx.listener(move |app, _, _, cx| {
                            app.request_list_item_removal(
                                ListItemRemovalTarget::new(
                                    ListItemRemovalKind::ByCpuLoadRule,
                                    index,
                                ),
                                cx,
                            );
                        }))
                        .into_any_element(),
                    ]),
                ));
        }
        card.into_any_element()
    }

    pub(super) fn render_by_running_app_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let input_value = self.inputs.performance_process.read(cx).value().to_string();
        let enabled = self.settings.by_running_app.enabled;
        let body = feature_body(enabled)
            .child(section_title_text(t!("common.rules").to_string()))
            .child(
                h_flex()
                    .gap_2()
                    .items_start()
                    .flex_wrap()
                    .child(self.render_process_picker(
                        "by-running-app-suggestion",
                        &self.inputs.performance_process,
                        SuggestionTarget::ByRunningApp,
                        window,
                        cx,
                    ))
                    .child(
                        primary_control_button(Button::new("add-by-running-app-process"), cx)
                            .label(t!("common.add").to_string())
                            .disabled(
                                !enabled
                                    || !can_add_by_running_app_process(
                                        &self.settings.by_running_app,
                                        &input_value,
                                    ),
                            )
                            .on_click(cx.listener(|app, _, window, cx| {
                                let process =
                                    app.inputs.performance_process.read(cx).value().to_string();
                                if can_add_by_running_app_process(
                                    &app.settings.by_running_app,
                                    &process,
                                ) {
                                    app.settings
                                        .by_running_app
                                        .rules
                                        .push(app.new_by_running_app_rule(&process));
                                    app.inputs.ensure_for_settings(window, cx, &app.settings);
                                    clear_input(&app.inputs.performance_process, window, cx);
                                }
                                cx.notify();
                            })),
                    ),
            )
            .child(self.render_by_running_app_rules(window, cx));

        let help = tooltip_lines(vec![
            t!("by_running_app.intro_1").to_string(),
            t!("by_running_app.intro_2").to_string(),
            t!("by_running_app.intro_3").to_string(),
            t!("common.power_plan_priority").to_string(),
            t!("common.power_plan_pause_priority").to_string(),
        ]);

        self.page_shell(Page::ByRunningApp, cx)
            .child(feature_toggle_switch_with_help(
                "by-running-app-enabled",
                t!("by_running_app.enable").to_string(),
                help,
                enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings.by_running_app.enabled = *checked;
                    cx.notify();
                }),
            ))
            .child(disabled_feature_body(
                "by-running-app-body",
                body,
                enabled,
                cx,
            ))
            .into_any_element()
    }

    pub(super) fn render_by_running_app_rules(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut list = rule_list(vec![
            rule_table_active_header(),
            rule_table_title_header(t!("process_list.process_name").to_string()),
            priority_exclusion_table_cell(t!("by_running_app.power_plan").to_string()),
            rule_table_action_header(),
        ]);
        for (index, rule) in self.settings.by_running_app.rules.iter().enumerate() {
            let process = rule.process_name.clone();
            let row = compact_rule_row(format!("by-running-app-rule-row-{index}"))
                .child(rule_active_cell(
                    format!("by-running-app-rule-enabled-{index}"),
                    rule.enabled,
                    cx.listener(move |app, checked, _, cx| {
                        if let Some(rule) = app.settings.by_running_app.rules.get_mut(index) {
                            rule.enabled = *checked;
                        }
                        cx.notify();
                    }),
                ))
                .child(self.process_rule_title(&process, cx))
                .child(self.render_inline_power_plan_picker(
                    format!("by-running-app-plan-{index}"),
                    rule.power_plan_guid.clone(),
                    PowerPlanField::ByRunningAppRule(index),
                    window,
                    cx,
                ))
                .child(rule_table_action_cell(
                    remove_control_button(Button::new(SharedString::from(format!(
                        "remove-by-running-app-{index}"
                    ))))
                    .on_click(cx.listener(move |app, _, _, cx| {
                        app.request_list_item_removal(
                            ListItemRemovalTarget::new(
                                ListItemRemovalKind::ByRunningAppRule,
                                index,
                            ),
                            cx,
                        );
                    }))
                    .into_any_element(),
                ));
            list = list.child(self.animated_list_item(
                ListItemRemovalTarget::new(ListItemRemovalKind::ByRunningAppRule, index),
                SharedString::from(format!("by-running-app-rule-{index}")),
                row.into_any_element(),
            ));
        }
        if self.settings.by_running_app.rules.is_empty() {
            list = list.child(text_muted(t!("common.no_custom_rules").to_string()).p_4());
        }
        list.into_any_element()
    }
}
