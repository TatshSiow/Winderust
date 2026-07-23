use crate::ui::app::*;

impl WinderustApp {
    pub(in crate::ui::app) fn render_by_running_app_page(
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

    pub(in crate::ui::app) fn render_by_running_app_rules(
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
