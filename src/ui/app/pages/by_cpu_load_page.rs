use crate::ui::app::*;

impl WinderustApp {
    pub(in crate::ui::app) fn render_by_cpu_load_page(
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

    pub(in crate::ui::app) fn render_by_cpu_load_rule(
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
}
