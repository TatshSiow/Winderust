use crate::ui::app::*;

impl WinderustApp {
    pub(in crate::ui::app) fn render_memory_trim_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let input_value = self
            .inputs
            .memory_trim_exclusion
            .read(cx)
            .value()
            .to_string();
        let settings = &self.settings.memory_trim;
        let enabled = settings.enabled;

        let body = feature_body(enabled)
            .child(setting_group_with_help(
                SettingGroupTarget::MemoryTrimThresholds,
                (
                    t!("memory_trim.category_thresholds").to_string(),
                    t!("memory_trim.category_thresholds_help").to_string(),
                ),
                div().into_any_element(),
                self.is_setting_group_collapsed(SettingGroupTarget::MemoryTrimThresholds),
                vec![
                    setting_group_action_row(
                        "memory-trim-memory-threshold",
                        t!("memory_trim.memory_threshold").to_string(),
                        self.render_numeric_value(
                            NumericField::MemoryTrimMemoryLoadThreshold,
                            format!("{}%", settings.system_memory_load_threshold_percent),
                            settings.system_memory_load_threshold_percent.to_string(),
                            cx,
                        ),
                        true,
                    )
                    .into_any_element(),
                    setting_group_action_row(
                        "memory-trim-working-set-threshold",
                        t!("memory_trim.working_set_threshold").to_string(),
                        self.render_numeric_value(
                            NumericField::MemoryTrimWorkingSetThreshold,
                            format!("{} MB", settings.process_working_set_threshold_mb),
                            settings.process_working_set_threshold_mb.to_string(),
                            cx,
                        ),
                        true,
                    )
                    .into_any_element(),
                ],
                window,
                cx,
            ))
            .child(setting_group_with_help(
                SettingGroupTarget::MemoryTrimWhen,
                (
                    t!("memory_trim.category_when_to_trim").to_string(),
                    t!("memory_trim.category_when_to_trim_help").to_string(),
                ),
                div().into_any_element(),
                self.is_setting_group_collapsed(SettingGroupTarget::MemoryTrimWhen),
                vec![setting_group_action_row(
                    "memory-trim-idle-time",
                    t!("memory_trim.idle_time").to_string(),
                    self.render_numeric_value(
                        NumericField::MemoryTrimIdleSeconds,
                        ui::duration_label(settings.process_idle_seconds),
                        settings.process_idle_seconds.to_string(),
                        cx,
                    ),
                    true,
                )
                .into_any_element()],
                window,
                cx,
            ))
            .child(setting_group_with_help(
                SettingGroupTarget::MemoryTrimSafety,
                (
                    t!("memory_trim.category_safety").to_string(),
                    t!("memory_trim.category_safety_help").to_string(),
                ),
                div().into_any_element(),
                self.is_setting_group_collapsed(SettingGroupTarget::MemoryTrimSafety),
                vec![
                    v_flex()
                        .gap_2()
                        .py_3()
                        .px_4()
                        .border_t_1()
                        .border_color(rgb(border_color()))
                        .child(
                            h_flex()
                                .gap_2()
                                .items_start()
                                .flex_wrap()
                                .child(self.render_process_picker(
                                    "memory-trim-exclusion",
                                    &self.inputs.memory_trim_exclusion,
                                    SuggestionTarget::MemoryTrim,
                                    window,
                                    cx,
                                ))
                                .child(
                                    primary_control_button(
                                        Button::new("add-memory-trim-exclusion"),
                                        cx,
                                    )
                                    .label(t!("common.add").to_string())
                                    .disabled(
                                        !enabled
                                            || !can_add_memory_trim_exclusion(
                                                &self.settings.memory_trim,
                                                &input_value,
                                            ),
                                    )
                                    .on_click(cx.listener(
                                        |app, _, window, cx| {
                                            let process = app
                                                .inputs
                                                .memory_trim_exclusion
                                                .read(cx)
                                                .value()
                                                .to_string();
                                            if can_add_memory_trim_exclusion(
                                                &app.settings.memory_trim,
                                                &process,
                                            ) {
                                                app.settings
                                                    .memory_trim
                                                    .exclusions
                                                    .push(new_process_exclusion_rule(&process));
                                                clear_input(
                                                    &app.inputs.memory_trim_exclusion,
                                                    window,
                                                    cx,
                                                );
                                            }
                                            cx.notify();
                                        },
                                    )),
                                ),
                        )
                        .into_any_element(),
                    self.render_memory_trim_exclusions(cx),
                ],
                window,
                cx,
            ))
            .child(setting_group_with_help(
                SettingGroupTarget::MemoryTrimMonitoring,
                (
                    t!("memory_trim.category_monitoring").to_string(),
                    t!("memory_trim.category_monitoring_help").to_string(),
                ),
                primary_control_button(Button::new("memory-trim-now"), cx)
                    .label(t!("memory_trim.trim_now").to_string())
                    .disabled(!enabled)
                    .on_click(cx.listener(|app, _, _, cx| {
                        app.background_automation.request_memory_trim_now();
                        app.status_message = t!("memory_trim.trim_now_requested").to_string();
                        cx.notify();
                    }))
                    .into_any_element(),
                self.is_setting_group_collapsed(SettingGroupTarget::MemoryTrimMonitoring),
                vec![stat_grid(vec![
                    (
                        t!("memory_trim.status").to_string(),
                        localized_runtime_status(&self.memory_trim_status.message),
                    ),
                    (
                        t!("memory_trim.memory_load").to_string(),
                        self.memory_trim_status
                            .memory_load_percent
                            .map(|percent| format!("{percent}%"))
                            .unwrap_or_else(|| t!("common.unknown").to_string()),
                    ),
                    (
                        t!("memory_trim.trimmed_processes").to_string(),
                        self.memory_trim_status.trimmed_processes.to_string(),
                    ),
                    (
                        t!("memory_trim.candidate_processes").to_string(),
                        self.memory_trim_status.candidate_processes.to_string(),
                    ),
                    (
                        t!("memory_trim.scanned_processes").to_string(),
                        self.memory_trim_status.scanned_processes.to_string(),
                    ),
                    (
                        t!("memory_trim.skipped_processes").to_string(),
                        self.memory_trim_status.skipped_processes.to_string(),
                    ),
                    (
                        t!("memory_trim.failed_actions").to_string(),
                        self.memory_trim_status.failed_processes.to_string(),
                    ),
                    (
                        t!("common.last_failure").to_string(),
                        self.memory_trim_status
                            .last_error
                            .clone()
                            .unwrap_or_else(|| t!("common.none").to_string()),
                    ),
                ])
                .into_any_element()],
                window,
                cx,
            ));
        self.page_shell(Page::MemoryTrim, cx)
            .child(feature_toggle_switch_with_help(
                "memory-trim-enabled",
                t!("memory_trim.enable").to_string(),
                tooltip_lines(vec![
                    t!("memory_trim.intro_1").to_string(),
                    t!("memory_trim.intro_2").to_string(),
                    t!("memory_trim.intro_3").to_string(),
                ]),
                enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings.memory_trim.enabled = *checked;
                    cx.notify();
                }),
            ))
            .child(disabled_feature_body("memory-trim-body", body, enabled, cx))
            .into_any_element()
    }

    pub(in crate::ui::app) fn render_memory_trim_exclusions(
        &self,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.render_process_exclusion_list(
            &self.settings.memory_trim.exclusions,
            ListItemRemovalKind::MemoryTrimExclusion,
            "memory-trim-exclusion",
            text_muted(t!("memory_trim.no_exclusions").to_string())
                .p_4()
                .into_any_element(),
            cx,
        )
    }
}
