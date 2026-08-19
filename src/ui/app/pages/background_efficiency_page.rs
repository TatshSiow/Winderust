use crate::config::ProcessRuleMode;
use crate::ui::app::*;

const BACKGROUND_EFFICIENCY_ACTION_COLUMN_WIDTH: f32 = 48.0;

impl ProcessRuleTier {
    const fn background_efficiency_mode(self, rule: &BackgroundEfficiencyRule) -> ProcessRuleMode {
        match self {
            Self::Focus => rule.focus_efficiency_mode,
            Self::VisibleWindow => rule.visible_window_efficiency_mode,
            Self::Background => rule.background_efficiency_mode,
        }
    }

    fn set_background_efficiency_mode(
        self,
        rule: &mut BackgroundEfficiencyRule,
        mode: ProcessRuleMode,
    ) {
        match self {
            Self::Focus => rule.focus_efficiency_mode = mode,
            Self::VisibleWindow => rule.visible_window_efficiency_mode = mode,
            Self::Background => rule.background_efficiency_mode = mode,
        }
    }
}

impl WinderustApp {
    pub(in crate::ui::app) fn render_background_efficiency_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let input_value = self.process_picker_path(
            SuggestionTarget::BackgroundEfficiency,
            &self.inputs.background_efficiency_process,
            cx,
        );
        let enabled = self.settings.background_efficiency.enabled;
        let body = feature_body(enabled)
            .child(setting_group_with_help(
                SettingGroupTarget::BackgroundEfficiencyForegroundDetection,
                (
                    t!("background_efficiency.foreground_detection").to_string(),
                    t!("background_efficiency.foreground_detection_help").to_string(),
                ),
                setting_group_switch_action(
                    "background-efficiency-foreground-detection-toggle",
                    self.settings
                        .background_efficiency
                        .foreground_detection_enabled,
                    cx.listener(|app, checked, _, cx| {
                        app.settings
                            .background_efficiency
                            .foreground_detection_enabled = *checked;
                        cx.notify();
                    }),
                ),
                self.is_setting_group_collapsed(
                    SettingGroupTarget::BackgroundEfficiencyForegroundDetection,
                ),
                vec![self.render_background_efficiency_default_row(
                    ProcessRuleTier::Focus,
                    self.settings
                        .background_efficiency
                        .foreground_efficiency_mode,
                    self.settings
                        .background_efficiency
                        .foreground_detection_enabled,
                    window,
                    cx,
                )],
                window,
                cx,
            ))
            .child(setting_group_with_help(
                SettingGroupTarget::BackgroundEfficiencyVisibleWindowDetection,
                (
                    t!("common.visible_window_detection").to_string(),
                    t!("common.visible_window_detection_help").to_string(),
                ),
                setting_group_switch_action(
                    "background-efficiency-visible-window-detection-toggle",
                    self.settings
                        .background_efficiency
                        .visible_window_detection_enabled,
                    cx.listener(|app, checked, _, cx| {
                        app.settings
                            .background_efficiency
                            .visible_window_detection_enabled = *checked;
                        cx.notify();
                    }),
                ),
                self.is_setting_group_collapsed(
                    SettingGroupTarget::BackgroundEfficiencyVisibleWindowDetection,
                ),
                vec![self.render_background_efficiency_default_row(
                    ProcessRuleTier::VisibleWindow,
                    self.settings
                        .background_efficiency
                        .visible_window_efficiency_mode,
                    self.settings
                        .background_efficiency
                        .visible_window_detection_enabled,
                    window,
                    cx,
                )],
                window,
                cx,
            ))
            .child(section_header(
                &t!("background_efficiency.custom_rules"),
                t!("background_efficiency.custom_rules_help").to_string(),
            ))
            .child(
                h_flex()
                    .gap_2()
                    .items_start()
                    .flex_wrap()
                    .child(self.render_process_picker(
                        "background-efficiency-suggestion",
                        &self.inputs.background_efficiency_process,
                        SuggestionTarget::BackgroundEfficiency,
                        window,
                        cx,
                    ))
                    .child(
                        primary_control_button(Button::new("add-background-efficiency-rule"), cx)
                            .label(t!("common.add").to_string())
                            .disabled(
                                !enabled
                                    || !can_add_background_efficiency_process(
                                        &self.settings.background_efficiency,
                                        &input_value,
                                    ),
                            )
                            .on_click(cx.listener(|app, _, window, cx| {
                                let process = app.process_picker_path(
                                    SuggestionTarget::BackgroundEfficiency,
                                    &app.inputs.background_efficiency_process,
                                    cx,
                                );
                                if can_add_background_efficiency_process(
                                    &app.settings.background_efficiency,
                                    &process,
                                ) {
                                    app.settings
                                        .background_efficiency
                                        .custom_rules
                                        .push(new_background_efficiency_rule(&process));
                                    clear_input(
                                        &app.inputs.background_efficiency_process,
                                        window,
                                        cx,
                                    );
                                }
                                cx.notify();
                            })),
                    ),
            )
            .child(self.render_background_custom_rules(window, cx));

        let help = tooltip_lines(vec![
            t!("background_efficiency.intro_1").to_string(),
            t!("background_efficiency.intro_2").to_string(),
            t!("background_efficiency.intro_3").to_string(),
        ]);

        self.page_shell(Page::BackgroundEfficiency, cx)
            .child(self.render_background_efficiency_enable_card(enabled, help, window, cx))
            .child(disabled_feature_body(
                "efficiency-exclusions-body",
                body,
                enabled,
                cx,
            ))
            .into_any_element()
    }

    pub(in crate::ui::app) fn render_background_efficiency_status_card(
        &self,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let access_denied_processes = self
            .feature_status
            .background_efficiency
            .access_denied_processes;
        v_flex()
            .gap_2()
            .child(
                self.render_normalized_feature_status(Page::BackgroundEfficiency)
                    .expect("Background Efficiency always has normalized runtime status"),
            )
            .when(
                access_denied_processes > 0 && !privilege::is_running_as_admin(),
                |card| {
                    card.child(
                        h_flex().justify_end().child(
                            primary_control_button(
                                Button::new("background-efficiency-relaunch-admin"),
                                cx,
                            )
                            .label(t!("admin_rights.relaunch").to_string())
                            .on_click(cx.listener(|app, _, _, cx| {
                                if privilege::relaunch_as_admin() {
                                    cx.quit();
                                } else {
                                    app.status_message =
                                        t!("status.admin_relaunch_failed").to_string();
                                    cx.notify();
                                }
                            })),
                        ),
                    )
                },
            )
            .into_any_element()
    }
    pub(in crate::ui::app) fn render_background_efficiency_enable_card(
        &self,
        enabled: bool,
        help: impl Into<SharedString>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        setting_group_with_help(
            SettingGroupTarget::EfficiencyEnable,
            (t!("background_efficiency.enable").to_string(), help),
            setting_group_switch_action(
                "background-efficiency-enabled-switch",
                enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings.background_efficiency.enabled = *checked;
                    cx.notify();
                }),
            ),
            self.is_setting_group_collapsed(SettingGroupTarget::EfficiencyEnable),
            vec![
                self.render_background_efficiency_default_row(
                    ProcessRuleTier::Background,
                    self.settings
                        .background_efficiency
                        .background_efficiency_mode,
                    enabled,
                    window,
                    cx,
                ),
                self.render_background_efficiency_aggressiveness_selector(enabled, window, cx),
            ],
            window,
            cx,
        )
        .into_any_element()
    }

    pub(in crate::ui::app) fn render_background_efficiency_aggressiveness_selector(
        &self,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = self.settings.background_efficiency.aggressiveness;
        setting_group_action_row_element(
            "background-efficiency-aggressiveness-row",
            h_flex()
                .flex_1()
                .min_w(px(0.0))
                .items_center()
                .gap_1()
                .child(
                    div()
                        .min_w(px(0.0))
                        .truncate()
                        .child(t!("background_efficiency.aggressiveness").to_string()),
                )
                .child(title_info_button(
                    "background-efficiency-aggressiveness-info",
                    t!("background_efficiency.aggressiveness_help").to_string(),
                ))
                .into_any_element(),
            self.render_background_efficiency_aggressiveness_picker(selected, enabled, window, cx),
            true,
        )
        .when(!enabled, |row| row.opacity(0.42).cursor_default())
        .into_any_element()
    }

    fn render_background_efficiency_default_row(
        &self,
        tier: ProcessRuleTier,
        selected: bool,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tier_key = tier.key();
        setting_group_action_row(
            format!("background-efficiency-{tier_key}-default-row"),
            t!("process_list.efficiency_mode").to_string(),
            self.render_dropdown_select(
                format!("background-efficiency-{tier_key}-default"),
                background_efficiency_mode_label(selected),
                enabled,
                DropdownSelectWidth::Standard,
                2,
                window,
                cx,
                move |max_height, cx| {
                    let mut options = dropdown_surface(cx, max_height);
                    for mode in [true, false] {
                        options = options.child(
                            dropdown_option_row(
                                SharedString::from(format!(
                                    "background-efficiency-{tier_key}-default-{mode}"
                                )),
                                background_efficiency_mode_label(mode),
                                selected == mode,
                                cx,
                            )
                            .on_click(cx.listener(
                                move |app, _, _, cx| {
                                    match tier {
                                        ProcessRuleTier::Focus => {
                                            app.settings
                                                .background_efficiency
                                                .foreground_efficiency_mode = mode;
                                        }
                                        ProcessRuleTier::VisibleWindow => {
                                            app.settings
                                                .background_efficiency
                                                .visible_window_efficiency_mode = mode;
                                        }
                                        ProcessRuleTier::Background => {
                                            app.settings
                                                .background_efficiency
                                                .background_efficiency_mode = mode;
                                        }
                                    }
                                    app.active_power_plan_picker = None;
                                    cx.notify();
                                },
                            )),
                        );
                    }
                    options
                },
            ),
            false,
        )
        .when(!enabled, |row| row.opacity(0.42).cursor_default())
        .into_any_element()
    }

    pub(in crate::ui::app) fn render_background_efficiency_aggressiveness_picker(
        &self,
        selected: BackgroundEfficiencyAggressiveness,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.render_dropdown_select(
            "background-efficiency-aggressiveness",
            background_efficiency_aggressiveness_label(selected),
            enabled,
            DropdownSelectWidth::Standard,
            BackgroundEfficiencyAggressiveness::ALL.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for aggressiveness in BackgroundEfficiencyAggressiveness::ALL {
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!(
                                "background-efficiency-aggressiveness-option-{aggressiveness:?}"
                            )),
                            background_efficiency_aggressiveness_label(aggressiveness),
                            selected == aggressiveness,
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            app.settings.background_efficiency.aggressiveness = aggressiveness;
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                options
            },
        )
    }

    pub(in crate::ui::app) fn render_background_custom_rules(
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
            rule_table_centered_header(
                t!("common.actions").to_string(),
                BACKGROUND_EFFICIENCY_ACTION_COLUMN_WIDTH,
            ),
        ]);
        for (index, rule) in self
            .settings
            .background_efficiency
            .custom_rules
            .iter()
            .enumerate()
        {
            let process = rule.executable_path.clone();
            let mut row = compact_rule_row(format!("background-efficiency-rule-row-{index}"))
                .child(rule_active_cell(
                    format!("background-efficiency-rule-enabled-{index}"),
                    rule.enabled,
                    cx.listener(move |app, checked, _, cx| {
                        if let Some(rule) = app
                            .settings
                            .background_efficiency
                            .custom_rules
                            .get_mut(index)
                        {
                            rule.enabled = *checked;
                        }
                        cx.notify();
                    }),
                ))
                .child(self.process_rule_title(&process, cx));
            for tier in ProcessRuleTier::ALL {
                row = row.child(self.render_background_efficiency_rule_selector(
                    index,
                    tier,
                    tier.background_efficiency_mode(rule),
                    window,
                    cx,
                ));
            }
            let row = row.child(
                h_flex()
                    .w(px(BACKGROUND_EFFICIENCY_ACTION_COLUMN_WIDTH))
                    .min_w(px(0.0))
                    .flex_shrink_0()
                    .justify_center()
                    .child(
                        remove_control_button(Button::new(SharedString::from(format!(
                            "remove-background-efficiency-{index}"
                        ))))
                        .on_click(cx.listener(move |app, _, _, cx| {
                            app.request_list_item_removal(
                                ListItemRemovalTarget::new(
                                    ListItemRemovalKind::BackgroundEfficiencyRule,
                                    index,
                                ),
                                cx,
                            );
                        })),
                    ),
            );
            list = list.child(self.animated_list_item(
                ListItemRemovalTarget::new(ListItemRemovalKind::BackgroundEfficiencyRule, index),
                SharedString::from(format!("background-efficiency-rule-{index}")),
                row.into_any_element(),
            ));
        }
        if self.settings.background_efficiency.custom_rules.is_empty() {
            list = list
                .child(text_muted(t!("background_efficiency.no_custom_rules").to_string()).p_4());
        }
        list.into_any_element()
    }

    fn render_background_efficiency_rule_selector(
        &self,
        index: usize,
        tier: ProcessRuleTier,
        selected: ProcessRuleMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let tier_key = tier.key();
        self.render_dropdown_select(
            format!("background-efficiency-{tier_key}-mode-{index}"),
            process_rule_mode_label(selected),
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
                                "background-efficiency-{tier_key}-mode-{index}-{mode:?}"
                            )),
                            process_rule_mode_label(mode),
                            selected == mode,
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            if let Some(rule) = app
                                .settings
                                .background_efficiency
                                .custom_rules
                                .get_mut(index)
                            {
                                tier.set_background_efficiency_mode(rule, mode);
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

fn process_rule_mode_label(mode: ProcessRuleMode) -> String {
    match mode {
        ProcessRuleMode::Default => t!("common.default").to_string(),
        ProcessRuleMode::Enabled => t!("common.enabled").to_string(),
        ProcessRuleMode::Disabled => t!("common.disabled").to_string(),
    }
}

fn background_efficiency_mode_label(enabled: bool) -> String {
    if enabled {
        t!("common.enabled").to_string()
    } else {
        t!("common.disabled").to_string()
    }
}
