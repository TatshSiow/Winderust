use super::*;

impl WinderustApp {
    pub(super) fn render_app_suspension_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let input_value = self
            .inputs
            .app_suspension_process
            .read(cx)
            .value()
            .to_string();
        let enabled = self.settings.app_suspension.enabled;
        let thaw_group_collapsed =
            self.is_setting_group_collapsed(SettingGroupTarget::SuspensionThaw);
        let audio_group_collapsed =
            self.is_setting_group_collapsed(SettingGroupTarget::SuspensionAudio);
        let network_group_collapsed =
            self.is_setting_group_collapsed(SettingGroupTarget::SuspensionNetwork);
        let body = feature_body(enabled)
            .child(setting_stepper_card_u64(
                "suspension-background-delay",
                t!("app_suspension.background_delay").to_string(),
                self.settings.app_suspension.background_delay_seconds,
                self.render_numeric_value(
                    NumericField::SuspensionBackgroundDelay,
                    format!(
                        "{} sec",
                        self.settings.app_suspension.background_delay_seconds
                    ),
                    self.settings
                        .app_suspension
                        .background_delay_seconds
                        .to_string(),
                    cx,
                ),
                cx.listener(|app, change: &StepChange<u64>, _, cx| {
                    app.settings.app_suspension.background_delay_seconds = apply_u64_step(
                        app.settings.app_suspension.background_delay_seconds,
                        change,
                        1,
                        86_400,
                    );
                    cx.notify();
                }),
            ))
            .child(setting_group(
                SettingGroupTarget::SuspensionThaw,
                t!("app_suspension.temporary_thaw").to_string(),
                setting_group_switch_action(
                    "temporary-thaw",
                    self.settings.app_suspension.temporary_thaw_enabled,
                    cx.listener(|app, checked, _, cx| {
                        app.settings.app_suspension.temporary_thaw_enabled = *checked;
                        cx.notify();
                    }),
                ),
                thaw_group_collapsed,
                vec![
                    setting_group_stepper_row_u64(
                        "suspension-thaw-interval",
                        t!("app_suspension.thaw_every").to_string(),
                        self.settings.app_suspension.temporary_thaw_interval_seconds,
                        self.render_numeric_value(
                            NumericField::SuspensionThawInterval,
                            format!(
                                "{} sec",
                                self.settings.app_suspension.temporary_thaw_interval_seconds
                            ),
                            self.settings
                                .app_suspension
                                .temporary_thaw_interval_seconds
                                .to_string(),
                            cx,
                        ),
                        true,
                        cx.listener(|app, change: &StepChange<u64>, _, cx| {
                            app.settings.app_suspension.temporary_thaw_interval_seconds =
                                apply_u64_step(
                                    app.settings.app_suspension.temporary_thaw_interval_seconds,
                                    change,
                                    1,
                                    86_400,
                                );
                            cx.notify();
                        }),
                    ),
                    setting_group_stepper_row_u64(
                        "suspension-thaw-duration",
                        t!("app_suspension.thaw_duration").to_string(),
                        self.settings.app_suspension.temporary_thaw_duration_seconds,
                        self.render_numeric_value(
                            NumericField::SuspensionThawDuration,
                            format!(
                                "{} sec",
                                self.settings.app_suspension.temporary_thaw_duration_seconds
                            ),
                            self.settings
                                .app_suspension
                                .temporary_thaw_duration_seconds
                                .to_string(),
                            cx,
                        ),
                        true,
                        cx.listener(|app, change: &StepChange<u64>, _, cx| {
                            app.settings.app_suspension.temporary_thaw_duration_seconds =
                                apply_u64_step(
                                    app.settings.app_suspension.temporary_thaw_duration_seconds,
                                    change,
                                    1,
                                    3_600,
                                );
                            cx.notify();
                        }),
                    ),
                ],
                window,
                cx,
            ))
            .child(setting_group(
                SettingGroupTarget::SuspensionAudio,
                t!("app_suspension.audio_detection").to_string(),
                setting_group_switch_action(
                    "audio-wake",
                    self.settings.app_suspension.audio_wake_enabled,
                    cx.listener(|app, checked, _, cx| {
                        app.settings.app_suspension.audio_wake_enabled = *checked;
                        cx.notify();
                    }),
                ),
                audio_group_collapsed,
                vec![setting_group_stepper_row_u64(
                    "suspension-audio-refreeze",
                    t!("app_suspension.audio_refreeze").to_string(),
                    self.settings.app_suspension.audio_wake_duration_seconds,
                    self.render_numeric_value(
                        NumericField::SuspensionAudioRefreeze,
                        format!(
                            "{} sec quiet",
                            self.settings.app_suspension.audio_wake_duration_seconds
                        ),
                        self.settings
                            .app_suspension
                            .audio_wake_duration_seconds
                            .to_string(),
                        cx,
                    ),
                    true,
                    cx.listener(|app, change: &StepChange<u64>, _, cx| {
                        app.settings.app_suspension.audio_wake_duration_seconds = apply_u64_step(
                            app.settings.app_suspension.audio_wake_duration_seconds,
                            change,
                            1,
                            3_600,
                        );
                        cx.notify();
                    }),
                )],
                window,
                cx,
            ))
            .child(setting_group(
                SettingGroupTarget::SuspensionNetwork,
                t!("app_suspension.network_detection").to_string(),
                setting_group_switch_action(
                    "network-wake",
                    self.settings.app_suspension.network_wake_enabled,
                    cx.listener(|app, checked, _, cx| {
                        app.settings.app_suspension.network_wake_enabled = *checked;
                        cx.notify();
                    }),
                ),
                network_group_collapsed,
                vec![setting_group_stepper_row_u64(
                    "suspension-network-refreeze",
                    t!("app_suspension.network_refreeze").to_string(),
                    self.settings.app_suspension.network_wake_duration_seconds,
                    self.render_numeric_value(
                        NumericField::SuspensionNetworkRefreeze,
                        format!(
                            "{} sec quiet",
                            self.settings.app_suspension.network_wake_duration_seconds
                        ),
                        self.settings
                            .app_suspension
                            .network_wake_duration_seconds
                            .to_string(),
                        cx,
                    ),
                    true,
                    cx.listener(|app, change: &StepChange<u64>, _, cx| {
                        app.settings.app_suspension.network_wake_duration_seconds = apply_u64_step(
                            app.settings.app_suspension.network_wake_duration_seconds,
                            change,
                            1,
                            3_600,
                        );
                        cx.notify();
                    }),
                )],
                window,
                cx,
            ))
            .child(section_header(
                &t!("app_suspension.suspendable_apps"),
                t!("app_suspension.suspendable_help").to_string(),
            ))
            .child(
                h_flex()
                    .gap_2()
                    .items_start()
                    .flex_wrap()
                    .child(self.render_process_picker(
                        "suspension-suggestion",
                        &self.inputs.app_suspension_process,
                        SuggestionTarget::AppSuspension,
                        window,
                        cx,
                    ))
                    .child(
                        primary_control_button(Button::new("add-suspension-process"), cx)
                            .label(t!("common.add").to_string())
                            .disabled(
                                !enabled
                                    || !can_add_app_suspension_process(
                                        &self.settings.app_suspension,
                                        &input_value,
                                    ),
                            )
                            .on_click(cx.listener(|app, _, window, cx| {
                                let process = app
                                    .inputs
                                    .app_suspension_process
                                    .read(cx)
                                    .value()
                                    .to_string();
                                if can_add_app_suspension_process(
                                    &app.settings.app_suspension,
                                    &process,
                                ) {
                                    app.settings
                                        .app_suspension
                                        .suspendable_apps
                                        .push(new_app_suspension_rule(&process));
                                    clear_input(&app.inputs.app_suspension_process, window, cx);
                                }
                                cx.notify();
                            })),
                    ),
            )
            .child(self.render_suspendable_apps(window, cx));

        let help = tooltip_lines(vec![
            t!("app_suspension.intro_1").to_string(),
            t!("app_suspension.intro_2").to_string(),
            t!("app_suspension.intro_3").to_string(),
        ]);

        self.page_shell(Page::AppSuspension, cx)
            .child(feature_toggle_switch_with_help(
                "app-suspension-enabled",
                t!("app_suspension.enable").to_string(),
                help,
                enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings.app_suspension.enabled = *checked;
                    cx.notify();
                }),
            ))
            .child(disabled_feature_body(
                "app-suspension-body",
                body,
                enabled,
                cx,
            ))
            .into_any_element()
    }

    pub(super) fn render_suspendable_apps(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut list = rule_list(vec![
            rule_table_active_header(),
            rule_table_left_header(
                t!("common.status").to_string(),
                SUSPENSION_STATUS_COLUMN_WIDTH,
            ),
            rule_table_title_header(t!("process_list.process_name").to_string()),
            rule_table_centered_header(
                t!("app_suspension.audio").to_string(),
                SUSPENSION_DETECT_COLUMN_WIDTH,
            ),
            rule_table_centered_header(
                t!("app_suspension.network").to_string(),
                SUSPENSION_DETECT_COLUMN_WIDTH,
            ),
            rule_table_centered_header("Download".to_string(), 172.0),
            rule_table_centered_header("Upload".to_string(), 172.0),
            rule_table_centered_header(
                t!("action_log.action").to_string(),
                SUSPENSION_ACTION_COLUMN_WIDTH,
            ),
        ]);
        for (index, rule) in self
            .settings
            .app_suspension
            .suspendable_apps
            .iter()
            .enumerate()
        {
            let process = rule.process_name.clone();
            let indicator = app_suspension_indicator(&self.app_suspension_status, &process);
            let rule_enabled = rule.enabled;
            let network_thresholds_enabled = rule_enabled
                && self.settings.app_suspension.network_wake_enabled
                && rule.network_wake_enabled;
            let row = compact_rule_row(format!("suspension-rule-row-{index}"))
                .child(
                    h_flex()
                        .w(px(SUSPENSION_ACTIVE_COLUMN_WIDTH))
                        .min_w(px(0.0))
                        .flex_shrink_0()
                        .justify_center()
                        .items_center()
                        .child(rule_enable_checkbox(
                            format!("suspension-rule-enabled-{index}"),
                            rule.enabled,
                            cx.listener(move |app, checked, _, cx| {
                                if let Some(rule) =
                                    app.settings.app_suspension.suspendable_apps.get_mut(index)
                                {
                                    rule.enabled = *checked;
                                }
                                cx.notify();
                            }),
                        )),
                )
                .child(
                    h_flex()
                        .w(px(SUSPENSION_STATUS_COLUMN_WIDTH))
                        .min_w(px(0.0))
                        .flex_shrink_0()
                        .items_center()
                        .child(status_pill_with_tooltip(
                            format!("suspension-status-pill-{index}"),
                            indicator.label,
                            indicator.bg,
                            indicator.fg,
                            indicator.hover,
                        )),
                )
                .child(self.process_rule_title(&process, cx))
                .child(
                    rule_table_checkbox_cell(
                        "suspension-audio-rule",
                        index,
                        rule.audio_wake_enabled,
                        cx.listener(move |app, checked, _, cx| {
                            if let Some(rule) =
                                app.settings.app_suspension.suspendable_apps.get_mut(index)
                            {
                                rule.audio_wake_enabled = *checked;
                            }
                            cx.notify();
                        }),
                    )
                    .into_any_element(),
                )
                .child(
                    rule_table_checkbox_cell(
                        "suspension-network-rule",
                        index,
                        rule.network_wake_enabled,
                        cx.listener(move |app, checked, _, cx| {
                            if let Some(rule) =
                                app.settings.app_suspension.suspendable_apps.get_mut(index)
                            {
                                rule.network_wake_enabled = *checked;
                            }
                            cx.notify();
                        }),
                    )
                    .into_any_element(),
                )
                .child(self.render_network_threshold_cell(
                    ThresholdField::Download(index),
                    rule.network_download_threshold_bytes,
                    rule.network_download_threshold_unit,
                    network_thresholds_enabled,
                    window,
                    cx,
                ))
                .child(self.render_network_threshold_cell(
                    ThresholdField::Upload(index),
                    rule.network_upload_threshold_bytes,
                    rule.network_upload_threshold_unit,
                    network_thresholds_enabled,
                    window,
                    cx,
                ))
                .child(
                    h_flex()
                        .w(px(SUSPENSION_ACTION_COLUMN_WIDTH))
                        .min_w(px(0.0))
                        .flex_shrink_0()
                        .gap_1()
                        .items_center()
                        .justify_center()
                        .child(
                            control_button(Button::new(SharedString::from(format!(
                                "freeze-suspension-{index}"
                            ))))
                            .with_size(px(32.0))
                            .icon(Icon::new(NavIcon::Snowflake).with_size(px(14.0)))
                            .tooltip(t!("app_suspension.freeze").to_string())
                            .disabled(
                                !rule_enabled
                                    || !can_manual_freeze(&self.app_suspension_status, &process),
                            )
                            .on_click(cx.listener({
                                let process = process.clone();
                                move |app, _, _, cx| {
                                    cx.stop_propagation();
                                    app.background_automation
                                        .request_app_suspension_freeze(&process);
                                    app.status_message = t!(
                                        "app_suspension.manual_freeze_requested",
                                        process = process
                                    )
                                    .to_string();
                                    cx.notify();
                                }
                            })),
                        )
                        .child(
                            remove_control_button(Button::new(SharedString::from(format!(
                                "remove-suspension-{index}"
                            ))))
                            .on_click(cx.listener({
                                move |app, _, _, cx| {
                                    app.request_list_item_removal(
                                        ListItemRemovalTarget::new(
                                            ListItemRemovalKind::AppSuspensionRule,
                                            index,
                                        ),
                                        cx,
                                    );
                                }
                            })),
                        )
                        .into_any_element(),
                );
            list = list.child(self.animated_list_item(
                ListItemRemovalTarget::new(ListItemRemovalKind::AppSuspensionRule, index),
                SharedString::from(format!("suspension-rule-{index}")),
                row.into_any_element(),
            ));
        }
        if self.settings.app_suspension.suspendable_apps.is_empty() {
            list = list.child(text_muted(t!("app_suspension.no_suspendable").to_string()).p_4());
        }
        list.into_any_element()
    }
}
