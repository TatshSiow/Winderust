use crate::ui::app::*;

impl WinderustApp {
    pub(in crate::ui::app) fn render_background_efficiency_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let input_value = self
            .inputs
            .background_efficiency_process
            .read(cx)
            .value()
            .to_string();
        let enabled = self.settings.background_efficiency.enabled;
        let body = feature_body(enabled)
            .child(feature_toggle_switch_with_help(
                "background-efficiency-foreground",
                t!("background_efficiency.focus_detection").to_string(),
                t!("background_efficiency.focus_detection_help").to_string(),
                self.settings.background_efficiency.exclude_foreground_app,
                cx.listener(|app, checked, _, cx| {
                    app.settings.background_efficiency.exclude_foreground_app = *checked;
                    cx.notify();
                }),
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
                        primary_control_button(
                            Button::new("add-background-efficiency-exclusion"),
                            cx,
                        )
                        .label(t!("common.add").to_string())
                        .disabled(
                            !enabled
                                || !can_add_background_efficiency_process(
                                    &self.settings.background_efficiency,
                                    &input_value,
                                ),
                        )
                        .on_click(cx.listener(|app, _, window, cx| {
                            let process = app
                                .inputs
                                .background_efficiency_process
                                .read(cx)
                                .value()
                                .to_string();
                            if can_add_background_efficiency_process(
                                &app.settings.background_efficiency,
                                &process,
                            ) {
                                app.settings
                                    .background_efficiency
                                    .custom_rules
                                    .push(new_background_efficiency_rule(&process));
                                clear_input(&app.inputs.background_efficiency_process, window, cx);
                            }
                            cx.notify();
                        })),
                    ),
            )
            .child(self.render_background_custom_rules(cx));

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
            vec![self.render_background_efficiency_aggressiveness_selector(enabled, window, cx)],
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

    pub(in crate::ui::app) fn effective_background_cpu_restriction_strategy(
        &self,
    ) -> CpuRestrictionStrategy {
        self.settings.background_cpu_restriction.strategy
    }

    pub(in crate::ui::app) fn render_background_custom_rules(
        &self,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut list = rule_list(process_rule_table_headers());
        for (index, rule) in self
            .settings
            .background_efficiency
            .custom_rules
            .iter()
            .enumerate()
        {
            let process = rule.process_name.clone();
            let row = compact_rule_row(format!("background-efficiency-exclusion-row-{index}"))
                .child(rule_active_cell(
                    format!("background-efficiency-exclusion-enabled-{index}"),
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
                .child(self.process_rule_title(&process, cx))
                .child(rule_table_action_cell(
                    remove_control_button(Button::new(SharedString::from(format!(
                        "remove-background-efficiency-{index}"
                    ))))
                    .on_click(cx.listener(move |app, _, _, cx| {
                        app.request_list_item_removal(
                            ListItemRemovalTarget::new(
                                ListItemRemovalKind::BackgroundEfficiencyExclusion,
                                index,
                            ),
                            cx,
                        );
                    }))
                    .into_any_element(),
                ));
            list = list.child(self.animated_list_item(
                ListItemRemovalTarget::new(
                    ListItemRemovalKind::BackgroundEfficiencyExclusion,
                    index,
                ),
                SharedString::from(format!("background-efficiency-exclusion-{index}")),
                row.into_any_element(),
            ));
        }
        if self.settings.background_efficiency.custom_rules.is_empty() {
            list = list
                .child(text_muted(t!("background_efficiency.no_custom_rules").to_string()).p_4());
        }
        list.into_any_element()
    }
}
