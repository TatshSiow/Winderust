use crate::ui::app::*;

impl WinderustApp {
    pub(in crate::ui::app) fn render_core_limiter_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let input_value = self
            .inputs
            .core_limiter_process
            .read(cx)
            .value()
            .to_string();
        let enabled = self.settings.core_limiter.enabled;
        let body = feature_body(enabled)
            .child(feature_toggle_switch_with_help(
                "core-limiter-foreground",
                t!("core_limiter.focus_detection").to_string(),
                t!("core_limiter.focus_detection_help").to_string(),
                self.settings.core_limiter.exclude_foreground_app,
                cx.listener(|app, checked, _, cx| {
                    app.settings.core_limiter.exclude_foreground_app = *checked;
                    cx.notify();
                }),
            ))
            .child(section_header(
                &t!("core_limiter.rules"),
                t!("core_limiter.rules_help").to_string(),
            ))
            .child(
                h_flex()
                    .gap_2()
                    .items_start()
                    .flex_wrap()
                    .child(self.render_process_picker(
                        "core-limiter-suggestion",
                        &self.inputs.core_limiter_process,
                        SuggestionTarget::CoreLimiter,
                        window,
                        cx,
                    ))
                    .child(
                        primary_control_button(Button::new("add-core-limiter-process"), cx)
                            .label(t!("common.add").to_string())
                            .disabled(
                                !enabled
                                    || !can_add_core_limiter_process(
                                        &self.settings.core_limiter,
                                        &input_value,
                                    ),
                            )
                            .on_click(cx.listener(|app, _, window, cx| {
                                let process =
                                    app.inputs.core_limiter_process.read(cx).value().to_string();
                                if can_add_core_limiter_process(
                                    &app.settings.core_limiter,
                                    &process,
                                ) {
                                    app.settings
                                        .core_limiter
                                        .rules
                                        .push(new_core_limiter_rule(&process));
                                    clear_input(&app.inputs.core_limiter_process, window, cx);
                                }
                                cx.notify();
                            })),
                    ),
            )
            .child(self.render_core_limiter_rules(window, cx));

        let help = tooltip_lines(vec![
            t!("core_limiter.intro_1").to_string(),
            t!("core_limiter.intro_2").to_string(),
            t!("core_limiter.intro_3").to_string(),
        ]);

        self.page_shell(Page::CoreLimiter, cx)
            .child(feature_toggle_switch_with_help(
                "core-limiter-enabled",
                t!("core_limiter.enable").to_string(),
                help,
                enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings.core_limiter.enabled = *checked;
                    cx.notify();
                }),
            ))
            .child(disabled_feature_body(
                "core-limiter-body",
                body,
                enabled,
                cx,
            ))
            .into_any_element()
    }

    pub(in crate::ui::app) fn render_core_limiter_rules(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut list = rule_list(process_rule_table_headers());
        for (index, rule) in self.settings.core_limiter.rules.iter().enumerate() {
            let process = rule.process_name.clone();
            let indicator = core_limiter_indicator(&self.core_limiter_status, &process);
            let card_target = RuleCardTarget::CoreLimiter(process.clone());
            let collapsed = self.is_rule_card_collapsed(&card_target);
            let mut card = rule_card(
                self.process_rule_title(&process, cx),
                rule_active_cell(
                    format!("core-limiter-rule-enabled-{index}"),
                    rule.enabled,
                    cx.listener(move |app, checked, _, cx| {
                        if let Some(rule) = app.settings.core_limiter.rules.get_mut(index) {
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
                card = card
                    .child(animated_rule_card_body_child(
                        &card_target,
                        0,
                        1,
                        rule_card_body_row(vec![rule_action_row(
                            format!("core-limiter-rule-status-{index}"),
                            t!("common.status").to_string(),
                            status_pill(indicator.0, indicator.1, indicator.2).into_any_element(),
                        )
                        .into_any_element()]),
                    ))
                    .child(animated_rule_card_body_child(
                        &card_target,
                        1,
                        2,
                        rule_card_body_row(vec![
                            self.render_core_limiter_numeric_row(
                                index,
                                NumericField::CoreLimiterThreshold(index),
                                t!("core_limiter.threshold").to_string(),
                                format!("{}%", rule.threshold_percent),
                                rule.threshold_percent.to_string(),
                                cx,
                            ),
                            self.render_core_limiter_numeric_row(
                                index,
                                NumericField::CoreLimiterMaxProcessors(index),
                                t!("core_limiter.max_processors").to_string(),
                                rule.max_logical_processors.to_string(),
                                rule.max_logical_processors.to_string(),
                                cx,
                            ),
                        ]),
                    ))
                    .child(animated_rule_card_body_child(
                        &card_target,
                        2,
                        2,
                        rule_card_body_row(vec![
                            self.render_core_limiter_numeric_row(
                                index,
                                NumericField::CoreLimiterSustain(index),
                                t!("core_limiter.sustain").to_string(),
                                ui::duration_label(rule.sustain_seconds),
                                rule.sustain_seconds.to_string(),
                                cx,
                            ),
                            self.render_core_limiter_numeric_row(
                                index,
                                NumericField::CoreLimiterCooldown(index),
                                t!("core_limiter.cooldown").to_string(),
                                ui::duration_label(rule.cooldown_seconds),
                                rule.cooldown_seconds.to_string(),
                                cx,
                            ),
                        ]),
                    ))
                    .child(animated_rule_card_body_child(
                        &card_target,
                        3,
                        1,
                        rule_card_body_action(
                            remove_control_button(Button::new(SharedString::from(format!(
                                "remove-core-limiter-{index}"
                            ))))
                            .on_click(cx.listener({
                                move |app, _, _, cx| {
                                    app.request_list_item_removal(
                                        ListItemRemovalTarget::new(
                                            ListItemRemovalKind::CoreLimiterRule,
                                            index,
                                        ),
                                        cx,
                                    );
                                }
                            }))
                            .into_any_element(),
                        ),
                    ));
            }
            list = list.child(self.animated_list_item(
                ListItemRemovalTarget::new(ListItemRemovalKind::CoreLimiterRule, index),
                SharedString::from(format!("core-limiter-rule-{index}")),
                card.into_any_element(),
            ));
        }
        if self.settings.core_limiter.rules.is_empty() {
            list = list.child(text_muted(t!("core_limiter.no_rules").to_string()).p_4());
        }
        list.into_any_element()
    }

    pub(in crate::ui::app) fn render_core_limiter_numeric_row(
        &self,
        index: usize,
        field: NumericField,
        label: String,
        display_value: String,
        edit_value: String,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        rule_action_row(
            format!("core-limiter-numeric-{index}-{field:?}"),
            label,
            self.render_numeric_value(field, display_value, edit_value, cx),
        )
        .into_any_element()
    }
}
