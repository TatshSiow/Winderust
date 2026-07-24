use crate::ui::app::*;

impl WinderustApp {
    pub(in crate::ui::app) fn render_dynamic_priority_boost_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let input_value = self
            .inputs
            .dynamic_priority_boost_process
            .read(cx)
            .value()
            .to_string();
        let enabled = self.settings.dynamic_priority_boost.enabled;
        let help = tooltip_lines(vec![
            t!("dynamic_priority_boost.intro_1").to_string(),
            t!("dynamic_priority_boost.intro_2").to_string(),
        ]);
        let master_card = setting_group_with_help(
            SettingGroupTarget::DynamicPriorityBoostMaster,
            (t!("dynamic_priority_boost.enable").to_string(), help),
            setting_group_switch_action(
                "dynamic-priority-boost-enabled-toggle",
                enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings.dynamic_priority_boost.enabled = *checked;
                    cx.notify();
                }),
            ),
            self.is_setting_group_collapsed(SettingGroupTarget::DynamicPriorityBoostMaster),
            vec![setting_group_action_row(
                "dynamic-priority-boost-background-default-row",
                t!("dynamic_priority_boost.background_default").to_string(),
                self.render_dynamic_priority_boost_default_selector(
                    DynamicPriorityBoostDefaultTarget::Background,
                    self.settings.dynamic_priority_boost.background_boost,
                    enabled,
                    window,
                    cx,
                ),
                false,
            )
            .into_any_element()],
            window,
            cx,
        );
        let body = feature_body(enabled)
            .child(setting_group_with_help(
                SettingGroupTarget::DynamicPriorityBoostForegroundDetection,
                (
                    t!("dynamic_priority_boost.foreground_detection").to_string(),
                    t!("dynamic_priority_boost.foreground_detection_help").to_string(),
                ),
                setting_group_switch_action(
                    "dynamic-priority-boost-foreground-detection-toggle",
                    self.settings
                        .dynamic_priority_boost
                        .foreground_detection_enabled,
                    cx.listener(|app, checked, _, cx| {
                        app.settings
                            .dynamic_priority_boost
                            .foreground_detection_enabled = *checked;
                        cx.notify();
                    }),
                ),
                self.is_setting_group_collapsed(
                    SettingGroupTarget::DynamicPriorityBoostForegroundDetection,
                ),
                vec![setting_group_action_row(
                    "dynamic-priority-boost-foreground-default-row",
                    t!("dynamic_priority_boost.foreground_default").to_string(),
                    self.render_dynamic_priority_boost_default_selector(
                        DynamicPriorityBoostDefaultTarget::Foreground,
                        self.settings.dynamic_priority_boost.foreground_boost,
                        self.settings
                            .dynamic_priority_boost
                            .foreground_detection_enabled,
                        window,
                        cx,
                    ),
                    false,
                )
                .into_any_element()],
                window,
                cx,
            ))
            .child(self.render_dynamic_priority_boost_status_card())
            .child(section_header(
                &t!("dynamic_priority_boost.exclusions"),
                t!("dynamic_priority_boost.exclusions_help").to_string(),
            ))
            .child(
                h_flex()
                    .gap_2()
                    .items_start()
                    .flex_wrap()
                    .child(self.render_process_picker(
                        "dynamic-priority-boost-process-suggestion",
                        &self.inputs.dynamic_priority_boost_process,
                        SuggestionTarget::DynamicPriorityBoost,
                        window,
                        cx,
                    ))
                    .child(
                        primary_control_button(
                            Button::new("add-dynamic-priority-boost-exclusion"),
                            cx,
                        )
                        .label(t!("common.add").to_string())
                        .disabled(
                            !enabled
                                || !can_add_dynamic_priority_boost_exclusion(
                                    &self.settings.dynamic_priority_boost,
                                    &input_value,
                                ),
                        )
                        .on_click(cx.listener(|app, _, window, cx| {
                            let process = app
                                .inputs
                                .dynamic_priority_boost_process
                                .read(cx)
                                .value()
                                .to_string();
                            if can_add_dynamic_priority_boost_exclusion(
                                &app.settings.dynamic_priority_boost,
                                &process,
                            ) {
                                app.settings
                                    .dynamic_priority_boost
                                    .exclusions
                                    .push(new_process_exclusion_rule(&process));
                                clear_input(&app.inputs.dynamic_priority_boost_process, window, cx);
                            }
                            cx.notify();
                        })),
                    ),
            )
            .child(self.render_dynamic_priority_boost_exclusions(window, cx));

        self.page_shell(Page::DynamicPriorityBoost, cx)
            .child(master_card)
            .child(disabled_feature_body(
                "dynamic-priority-boost-body",
                body,
                enabled,
                cx,
            ))
            .into_any_element()
    }

    pub(in crate::ui::app) fn render_dynamic_priority_boost_exclusions(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.render_priority_exclusion_table(
            "dynamic-priority-boost-exclusion",
            &self.settings.dynamic_priority_boost.exclusions,
            ListItemRemovalKind::DynamicPriorityBoostExclusion,
            t!("dynamic_priority_boost.no_exclusions").to_string(),
            window,
            cx,
        )
    }

    pub(in crate::ui::app) fn render_dynamic_priority_boost_status_card(&self) -> gpui::Div {
        let status = &self.dynamic_priority_boost_status;
        let message = if status.message.is_empty() {
            t!("dynamic_priority_boost.not_checked").to_string()
        } else {
            localized_runtime_status(&status.message)
        };
        let mut rows = vec![
            (t!("common.status").to_string(), message),
            (
                t!("dynamic_priority_boost.adjusted_processes").to_string(),
                status.adjusted_processes.to_string(),
            ),
            (
                t!("dynamic_priority_boost.scanned_processes").to_string(),
                status.scanned_processes.to_string(),
            ),
            (
                t!("dynamic_priority_boost.skipped_processes").to_string(),
                status.skipped_processes.to_string(),
            ),
            (
                t!("dynamic_priority_boost.failed_actions").to_string(),
                status.failed_processes.to_string(),
            ),
        ];
        if let Some(error) = &status.last_error {
            rows.push((t!("common.last_failure").to_string(), error.clone()));
        }
        stat_grid(rows)
    }

    pub(in crate::ui::app) fn render_dynamic_priority_boost_default_selector(
        &self,
        target: DynamicPriorityBoostDefaultTarget,
        selected_boost: ProcessDynamicPriorityBoostSetting,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = match target {
            DynamicPriorityBoostDefaultTarget::Background => {
                "dynamic-priority-boost-background-default"
            }
            DynamicPriorityBoostDefaultTarget::Foreground => {
                "dynamic-priority-boost-foreground-default"
            }
        };
        self.render_dropdown_select(
            id,
            process_dynamic_priority_boost_setting_label(selected_boost),
            enabled,
            DropdownSelectWidth::Standard,
            ProcessDynamicPriorityBoostSetting::ALL.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for boost in ProcessDynamicPriorityBoostSetting::ALL {
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!("{id}-option-{boost:?}")),
                            process_dynamic_priority_boost_setting_label(boost),
                            selected_boost == boost,
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            match target {
                                DynamicPriorityBoostDefaultTarget::Background => {
                                    app.settings.dynamic_priority_boost.background_boost = boost;
                                }
                                DynamicPriorityBoostDefaultTarget::Foreground => {
                                    app.settings.dynamic_priority_boost.foreground_boost = boost;
                                }
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
