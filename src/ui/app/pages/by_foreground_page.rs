use crate::ui::app::*;

impl WinderustApp {
    pub(in crate::ui::app) fn render_by_foreground_page(
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

    pub(in crate::ui::app) fn render_foreground_rule(
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

    pub(in crate::ui::app) fn new_foreground_rule(&self, process: &str) -> ByForegroundRule {
        new_foreground_rule(
            process,
            self.current_plan.as_ref().map(|plan| plan.guid.clone()),
        )
    }

    pub(in crate::ui::app) fn new_by_running_app_rule(&self, process: &str) -> ByRunningAppRule {
        new_by_running_app_rule(
            process,
            self.current_plan.as_ref().map(|plan| plan.guid.clone()),
        )
    }

    pub(in crate::ui::app) fn render_rule_title(
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
}
