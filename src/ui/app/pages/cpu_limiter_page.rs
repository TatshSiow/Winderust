use crate::config::ProcessRuleMode;
use crate::ui::app::*;

impl ProcessRuleTier {
    const fn cpu_limiter_mode(self, rule: &CpuLimiterRule) -> ProcessRuleMode {
        match self {
            Self::Focus => rule.focus_mode,
            Self::VisibleWindow => rule.visible_window_mode,
            Self::Background => rule.background_mode,
        }
    }

    fn set_cpu_limiter_mode(self, rule: &mut CpuLimiterRule, mode: ProcessRuleMode) {
        match self {
            Self::Focus => rule.focus_mode = mode,
            Self::VisibleWindow => rule.visible_window_mode = mode,
            Self::Background => rule.background_mode = mode,
        }
    }

    fn cpu_limiter_allowed_time_label(self) -> String {
        match self {
            Self::Focus => t!("cpu_limiter.focus_allowed_cpu_time").to_string(),
            Self::VisibleWindow => t!("cpu_limiter.visible_window_allowed_cpu_time").to_string(),
            Self::Background => t!("cpu_limiter.background_allowed_cpu_time").to_string(),
        }
    }
}

impl WinderustApp {
    pub(in crate::ui::app) fn render_cpu_limiter_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.sync_cpu_limiter_slider_states(window, cx);

        let input_value = self.process_picker_path(
            SuggestionTarget::CpuLimiter,
            &self.inputs.cpu_limiter_process,
            cx,
        );
        let enabled = self.settings.cpu_limiter.enabled;
        let body = feature_body()
            .child(feature_toggle_switch_with_help(
                "cpu-limiter-foreground",
                t!("common.protect_foreground_app").to_string(),
                t!("common.protect_foreground_app_help").to_string(),
                self.settings.cpu_limiter.protect_foreground_app,
                cx.listener(|app, checked, _, cx| {
                    app.settings.cpu_limiter.protect_foreground_app = *checked;
                    cx.notify();
                }),
            ))
            .child(feature_toggle_switch_with_help(
                "cpu-limiter-visible-windows",
                t!("common.protect_visible_window_apps").to_string(),
                t!("common.protect_visible_window_apps_help").to_string(),
                self.settings.cpu_limiter.protect_visible_window_apps,
                cx.listener(|app, checked, _, cx| {
                    app.settings.cpu_limiter.protect_visible_window_apps = *checked;
                    cx.notify();
                }),
            ))
            .child(section_header(
                &t!("cpu_limiter.rules"),
                t!("cpu_limiter.rules_help").to_string(),
            ))
            .child(
                h_flex()
                    .gap_2()
                    .items_start()
                    .flex_wrap()
                    .child(self.render_process_picker(
                        "cpu-limiter-suggestion",
                        &self.inputs.cpu_limiter_process,
                        SuggestionTarget::CpuLimiter,
                        window,
                        cx,
                    ))
                    .child(
                        primary_control_button(Button::new("add-cpu-limiter-process"), cx)
                            .label(t!("common.add").to_string())
                            .disabled(
                                !enabled
                                    || !can_add_cpu_limiter_process(
                                        &self.settings.cpu_limiter,
                                        &input_value,
                                    ),
                            )
                            .on_click(cx.listener(|app, _, window, cx| {
                                let process = app.process_picker_path(
                                    SuggestionTarget::CpuLimiter,
                                    &app.inputs.cpu_limiter_process,
                                    cx,
                                );
                                if can_add_cpu_limiter_process(&app.settings.cpu_limiter, &process)
                                {
                                    app.settings
                                        .cpu_limiter
                                        .rules
                                        .push(new_cpu_limiter_rule(&process));
                                    clear_input(&app.inputs.cpu_limiter_process, window, cx);
                                }
                                cx.notify();
                            })),
                    ),
            )
            .child(self.render_cpu_limiter_rules(window, cx));

        let help = tooltip_lines(vec![
            t!("cpu_limiter.intro_1").to_string(),
            t!("cpu_limiter.intro_2").to_string(),
            t!("cpu_limiter.intro_3").to_string(),
            t!("cpu_limiter.intro_4").to_string(),
            t!("cpu_limiter.intro_5").to_string(),
        ]);

        page_body_shell()
            .child(feature_toggle_switch_with_help(
                "cpu-limiter-enabled",
                t!("cpu_limiter.enable").to_string(),
                help,
                enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings.cpu_limiter.enabled = *checked;
                    cx.notify();
                }),
            ))
            .child(disabled_feature_body("cpu-limiter-body", body, enabled, cx))
            .into_any_element()
    }

    pub(in crate::ui::app) fn render_cpu_limiter_rules(
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
        for (index, rule) in self.settings.cpu_limiter.rules.iter().enumerate() {
            let process = rule.executable_path.clone();
            let indicator = cpu_limiter_indicator(&self.feature_status.cpu_limiter, &process);
            let card_target = RuleCardTarget::CpuLimiter(process.clone());
            let collapsed = self.is_rule_card_collapsed(&card_target);
            let mut title = h_flex()
                .flex_1()
                .min_w(px(0.0))
                .child(self.process_rule_title(&process, cx));
            for tier in ProcessRuleTier::ALL {
                title = title.child(self.render_cpu_limiter_rule_selector(
                    index,
                    tier,
                    tier.cpu_limiter_mode(rule),
                    window,
                    cx,
                ));
            }
            let mut card = rule_card(
                title.into_any_element(),
                rule_active_cell(
                    format!("cpu-limiter-rule-enabled-{index}"),
                    rule.enabled,
                    cx.listener(move |app, checked, _, cx| {
                        if let Some(rule) = app.settings.cpu_limiter.rules.get_mut(index) {
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
                let mut limiter_rows = Vec::new();
                for tier in ProcessRuleTier::ALL {
                    let Some(state) = cpu_limiter_slider_input(&self.inputs, index, tier) else {
                        continue;
                    };
                    let value = tier.cpu_limiter_allowed_time(rule);
                    let (focus, visible_window) = tier.flags();
                    let tier_enabled = rule.enabled
                        && self
                            .settings
                            .cpu_limiter
                            .rule_enabled(rule, focus, visible_window);
                    let value_element = if tier_enabled {
                        self.render_numeric_value(
                            NumericField::CpuLimiterAllowedTime(index, tier),
                            format!("{value}%"),
                            value.to_string(),
                            cx,
                        )
                    } else {
                        value_pill(format!("{value}%")).into_any_element()
                    };
                    limiter_rows.push(rule_percent_slider_row(
                        SliderRowSpec {
                            id: SharedString::from(format!(
                                "cpu-limiter-{}-allowed-time-{index}",
                                tier.key()
                            )),
                            label: SharedString::from(tier.cpu_limiter_allowed_time_label()),
                            value_element,
                            state: &state,
                            enabled: tier_enabled,
                            delta: 1_u8,
                            range: SliderRange {
                                min: 1,
                                max: 99,
                                step: 1,
                            },
                        },
                        window,
                        cx,
                        cx.listener(move |app, change: &StepChange<u8>, _, cx| {
                            if let Some(value) =
                                app.settings.cpu_limiter.rules.get(index).map(|rule| {
                                    apply_u8_step(
                                        tier.cpu_limiter_allowed_time(rule),
                                        change,
                                        1,
                                        99,
                                    )
                                })
                            {
                                app.set_cpu_limiter_slider_value(index, tier, value);
                            }
                            cx.notify();
                        }),
                    ));
                }

                card = card
                    .child(animated_rule_card_body_child(
                        &card_target,
                        0,
                        1,
                        rule_card_body_row(vec![rule_action_row(
                            format!("cpu-limiter-rule-status-{index}"),
                            t!("common.status").to_string(),
                            status_pill(indicator.0, indicator.1, indicator.2).into_any_element(),
                        )
                        .into_any_element()]),
                    ))
                    .child(animated_rule_card_body_child(
                        &card_target,
                        1,
                        limiter_rows.len(),
                        rule_card_body_row(limiter_rows),
                    ))
                    .child(animated_rule_card_body_child(
                        &card_target,
                        2,
                        1,
                        rule_card_body_action(
                            remove_control_button(Button::new(SharedString::from(format!(
                                "remove-cpu-limiter-{index}"
                            ))))
                            .on_click(cx.listener({
                                move |app, _, _, cx| {
                                    app.request_list_item_removal(
                                        ListItemRemovalTarget::new(
                                            ListItemRemovalKind::CpuLimiterRule,
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
                ListItemRemovalTarget::new(ListItemRemovalKind::CpuLimiterRule, index),
                SharedString::from(format!("cpu-limiter-rule-{index}")),
                card.into_any_element(),
            ));
        }
        if self.settings.cpu_limiter.rules.is_empty() {
            list = list.child(text_muted(t!("cpu_limiter.no_rules").to_string()).p_4());
        }
        list.into_any_element()
    }

    fn render_cpu_limiter_rule_selector(
        &self,
        index: usize,
        tier: ProcessRuleTier,
        selected: ProcessRuleMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tier_key = tier.key();
        self.render_dropdown_select(
            format!("cpu-limiter-{tier_key}-mode-{index}"),
            cpu_limiter_rule_mode_label(selected),
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
                                "cpu-limiter-{tier_key}-mode-{index}-{mode:?}"
                            )),
                            cpu_limiter_rule_mode_label(mode),
                            selected == mode,
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            if let Some(rule) = app.settings.cpu_limiter.rules.get_mut(index) {
                                tier.set_cpu_limiter_mode(rule, mode);
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

fn cpu_limiter_rule_mode_label(mode: ProcessRuleMode) -> String {
    match mode {
        ProcessRuleMode::Default => t!("common.default").to_string(),
        ProcessRuleMode::Enabled => t!("common.enabled").to_string(),
        ProcessRuleMode::Disabled => t!("common.disabled").to_string(),
    }
}
