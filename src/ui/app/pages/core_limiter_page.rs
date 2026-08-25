use crate::config::ProcessRuleMode;
use crate::ui::app::*;

impl ProcessRuleTier {
    const fn core_limiter_mode(self, rule: &CoreLimiterRule) -> ProcessRuleMode {
        match self {
            Self::Focus => rule.focus_mode,
            Self::VisibleWindow => rule.visible_window_mode,
            Self::Background => rule.background_mode,
        }
    }

    fn set_core_limiter_mode(self, rule: &mut CoreLimiterRule, mode: ProcessRuleMode) {
        match self {
            Self::Focus => rule.focus_mode = mode,
            Self::VisibleWindow => rule.visible_window_mode = mode,
            Self::Background => rule.background_mode = mode,
        }
    }
}

impl WinderustApp {
    pub(in crate::ui::app) fn render_core_limiter_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let input_value = self.process_picker_path(
            SuggestionTarget::CoreLimiter,
            &self.inputs.core_limiter_process,
            cx,
        );
        let enabled = self.settings.core_limiter.enabled;
        let body = feature_body()
            .child(feature_toggle_switch_with_help(
                "core-limiter-foreground",
                t!("common.protect_foreground_app").to_string(),
                t!("common.protect_foreground_app_help").to_string(),
                self.settings.core_limiter.protect_foreground_app,
                cx.listener(|app, checked, _, cx| {
                    app.settings.core_limiter.protect_foreground_app = *checked;
                    cx.notify();
                }),
            ))
            .child(feature_toggle_switch_with_help(
                "core-limiter-visible-windows",
                t!("common.protect_visible_window_apps").to_string(),
                t!("common.protect_visible_window_apps_help").to_string(),
                self.settings.core_limiter.protect_visible_window_apps,
                cx.listener(|app, checked, _, cx| {
                    app.settings.core_limiter.protect_visible_window_apps = *checked;
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
                                let process = app.process_picker_path(
                                    SuggestionTarget::CoreLimiter,
                                    &app.inputs.core_limiter_process,
                                    cx,
                                );
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

        page_body_shell()
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
        let mut list = rule_list(vec![
            rule_table_active_header(),
            rule_table_title_header(t!("process_list.app_name").to_string()),
            rule_table_title_header(t!("process_list.executable_path").to_string()),
            rule_table_centered_header(
                t!("cpu_allocation.focus").to_string(),
                DROPDOWN_SELECT_COMPACT_WIDTH,
            ),
            rule_table_centered_header(
                t!("common.visible_window").to_string(),
                DROPDOWN_SELECT_COMPACT_WIDTH,
            ),
            rule_table_centered_header(
                t!("common.background_process").to_string(),
                DROPDOWN_SELECT_COMPACT_WIDTH,
            ),
            rule_table_action_header(),
        ]);
        for (index, rule) in self.settings.core_limiter.rules.iter().enumerate() {
            let process = rule.executable_path.clone();
            let indicator = core_limiter_indicator(&self.feature_status.core_limiter, &process);
            let card_target = RuleCardTarget::CoreLimiter(process.clone());
            let collapsed = self.is_rule_card_collapsed(&card_target);
            let mut title = h_flex()
                .flex_1()
                .min_w(px(0.0))
                .child(self.process_rule_title(&process, cx));
            for tier in ProcessRuleTier::ALL {
                title = title.child(self.render_core_limiter_rule_selector(
                    index,
                    tier,
                    tier.core_limiter_mode(rule),
                    window,
                    cx,
                ));
            }
            let mut card = rule_card(
                title.into_any_element(),
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

    fn render_core_limiter_rule_selector(
        &self,
        index: usize,
        tier: ProcessRuleTier,
        selected: ProcessRuleMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tier_key = tier.key();
        self.render_dropdown_select(
            format!("core-limiter-{tier_key}-mode-{index}"),
            core_limiter_rule_mode_label(selected),
            true,
            DropdownSelectWidth::Compact,
            ProcessRuleMode::ALL.len(),
            window,
            cx,
            move |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for mode in ProcessRuleMode::ALL {
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!(
                                "core-limiter-{tier_key}-mode-{index}-{mode:?}"
                            )),
                            core_limiter_rule_mode_label(mode),
                            selected == mode,
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            if let Some(rule) = app.settings.core_limiter.rules.get_mut(index) {
                                tier.set_core_limiter_mode(rule, mode);
                            }
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                options
            },
        )
    }
}

fn core_limiter_rule_mode_label(mode: ProcessRuleMode) -> String {
    match mode {
        ProcessRuleMode::Default => t!("common.default").to_string(),
        ProcessRuleMode::Enabled => t!("common.enabled").to_string(),
        ProcessRuleMode::Disabled => t!("common.disabled").to_string(),
    }
}
