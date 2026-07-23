use crate::ui::app::*;

impl WinderustApp {
    pub(in crate::ui::app) fn render_timer_resolution_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let input_value = self
            .inputs
            .timer_resolution_process
            .read(cx)
            .value()
            .to_string();
        let enabled = self.settings.timer_resolution.enabled;
        let help = tooltip_lines(vec![
            t!("timer_resolution.intro_1").to_string(),
            t!("timer_resolution.intro_2").to_string(),
            t!("timer_resolution.intro_3").to_string(),
        ]);
        let body = feature_body(enabled)
            .child(section_title_text(t!("common.rules").to_string()))
            .child(
                h_flex()
                    .gap_2()
                    .items_start()
                    .flex_wrap()
                    .child(self.render_process_picker(
                        "timer-resolution-process-suggestion",
                        &self.inputs.timer_resolution_process,
                        SuggestionTarget::TimerResolution,
                        window,
                        cx,
                    ))
                    .child(
                        primary_control_button(Button::new("add-timer-resolution-rule"), cx)
                            .label(t!("common.add").to_string())
                            .disabled(
                                !enabled
                                    || !can_add_timer_resolution_process(
                                        &self.settings.timer_resolution,
                                        &input_value,
                                    ),
                            )
                            .on_click(cx.listener(|app, _, window, cx| {
                                let process = app
                                    .inputs
                                    .timer_resolution_process
                                    .read(cx)
                                    .value()
                                    .to_string();
                                if can_add_timer_resolution_process(
                                    &app.settings.timer_resolution,
                                    &process,
                                ) {
                                    let desired_100ns = app.settings.timer_resolution.desired_100ns;
                                    app.settings
                                        .timer_resolution
                                        .rules
                                        .push(new_timer_resolution_rule(&process, desired_100ns));
                                    clear_input(&app.inputs.timer_resolution_process, window, cx);
                                }
                                cx.notify();
                            })),
                    ),
            )
            .child(self.render_timer_resolution_rules(cx))
            .child(self.render_timer_resolution_status_card());

        self.page_shell(Page::TimerResolution, cx)
            .child(feature_toggle_switch_with_help(
                "timer-resolution-enabled",
                t!("timer_resolution.enable").to_string(),
                help,
                enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings.timer_resolution.enabled = *checked;
                    cx.notify();
                }),
            ))
            .child(text_muted(t!("timer_resolution.warning").to_string()))
            .child(disabled_feature_body(
                "timer-resolution-body",
                body,
                enabled,
                cx,
            ))
            .into_any_element()
    }

    pub(in crate::ui::app) fn render_timer_resolution_rules(
        &self,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut list = rule_list(vec![
            rule_table_active_header(),
            rule_table_title_header(t!("process_list.process_name").to_string()),
            rule_table_centered_header(t!("timer_resolution.requested").to_string(), 104.0),
            rule_table_action_header(),
        ]);
        for (index, rule) in self.settings.timer_resolution.rules.iter().enumerate() {
            let row = compact_rule_row(format!("timer-resolution-rule-row-{index}"))
                .child(rule_active_cell(
                    format!("timer-resolution-rule-enabled-{index}"),
                    rule.enabled,
                    cx.listener(move |app, checked, _, cx| {
                        if let Some(rule) = app.settings.timer_resolution.rules.get_mut(index) {
                            rule.enabled = *checked;
                        }
                        cx.notify();
                    }),
                ))
                .child(self.process_rule_title(&rule.process_name, cx))
                .child(self.render_numeric_value(
                    NumericField::TimerResolutionRule(index),
                    timer_resolution::format_resolution_ms(rule.desired_100ns),
                    timer_resolution_edit_value(rule.desired_100ns),
                    cx,
                ))
                .child(rule_table_action_cell(
                    remove_control_button(Button::new(SharedString::from(format!(
                        "remove-timer-resolution-rule-{index}"
                    ))))
                    .on_click(cx.listener(move |app, _, _, cx| {
                        app.request_list_item_removal(
                            ListItemRemovalTarget::new(
                                ListItemRemovalKind::TimerResolutionRule,
                                index,
                            ),
                            cx,
                        );
                    }))
                    .into_any_element(),
                ));
            list = list.child(self.animated_list_item(
                ListItemRemovalTarget::new(ListItemRemovalKind::TimerResolutionRule, index),
                SharedString::from(format!("timer-resolution-rule-{index}")),
                row.into_any_element(),
            ));
        }
        if self.settings.timer_resolution.rules.is_empty() {
            list = list.child(text_muted(t!("timer_resolution.no_rules").to_string()).p_4());
        }
        list.into_any_element()
    }

    pub(in crate::ui::app) fn render_timer_resolution_status_card(&self) -> gpui::Div {
        let status = &self.timer_resolution_status;
        let requested = status
            .requested_100ns
            .map(timer_resolution::format_resolution_ms)
            .unwrap_or_else(|| {
                if self.settings.timer_resolution.enabled {
                    t!("timer_resolution.no_active_request").to_string()
                } else {
                    t!("common.disabled").to_string()
                }
            });
        let active_rule = status.active_rule_process.clone().unwrap_or_else(|| {
            if self.settings.timer_resolution.enabled {
                t!("timer_resolution.no_matching_rule").to_string()
            } else {
                t!("common.disabled").to_string()
            }
        });

        let mut rows = vec![
            (
                t!("timer_resolution.foreground_rule").to_string(),
                active_rule,
            ),
            (t!("timer_resolution.requested").to_string(), requested),
            (
                t!("timer_resolution.minimum").to_string(),
                format_optional_timer_resolution(status.minimum_100ns),
            ),
            (
                t!("timer_resolution.maximum").to_string(),
                format_optional_timer_resolution(status.maximum_100ns),
            ),
            (
                t!("common.status").to_string(),
                localized_runtime_status(&status.message),
            ),
        ];
        if let Some(error) = &status.last_error {
            rows.push((t!("common.last_failure").to_string(), error.clone()));
        }
        stat_grid(rows)
    }
}
