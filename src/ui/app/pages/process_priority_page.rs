use crate::ui::app::*;

impl WinderustApp {
    pub(in crate::ui::app) fn render_process_priority_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let input_value = self
            .inputs
            .process_priority_process
            .read(cx)
            .value()
            .to_string();
        let enabled = self.settings.process_priority.enabled;
        let help = tooltip_lines(vec![
            t!("process_priority.intro_1").to_string(),
            t!("process_priority.intro_2").to_string(),
        ]);
        let master_card = setting_group_with_help(
            SettingGroupTarget::ProcessPriorityMaster,
            (t!("process_priority.enable").to_string(), help),
            setting_group_switch_action(
                "process-priority-enabled-toggle",
                enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings.process_priority.enabled = *checked;
                    cx.notify();
                }),
            ),
            self.is_setting_group_collapsed(SettingGroupTarget::ProcessPriorityMaster),
            vec![
                setting_group_action_row(
                    "process-priority-background-default-row",
                    t!("process_priority.background_default").to_string(),
                    self.render_process_priority_default_selector(
                        ProcessPriorityDefaultTarget::Background,
                        self.settings.process_priority.background_priority,
                        enabled,
                        window,
                        cx,
                    ),
                    false,
                )
                .into_any_element(),
                setting_group_action_row(
                    "process-priority-preserve-background-row",
                    t!("common.preserve_background_priority").to_string(),
                    setting_group_switch_action(
                        "process-priority-preserve-background-toggle",
                        self.settings.process_priority.preserve_background_priority,
                        cx.listener(|app, checked, _, cx| {
                            app.settings.process_priority.preserve_background_priority = *checked;
                            cx.notify();
                        }),
                    ),
                    false,
                )
                .into_any_element(),
            ],
            window,
            cx,
        );
        let body = feature_body(enabled)
            .child(setting_group_with_help(
                SettingGroupTarget::ProcessPriorityForegroundDetection,
                (
                    t!("process_priority.foreground_detection").to_string(),
                    t!("process_priority.foreground_detection_help").to_string(),
                ),
                setting_group_switch_action(
                    "process-priority-foreground-detection-toggle",
                    self.settings.process_priority.foreground_detection_enabled,
                    cx.listener(|app, checked, _, cx| {
                        app.settings.process_priority.foreground_detection_enabled = *checked;
                        cx.notify();
                    }),
                ),
                self.is_setting_group_collapsed(
                    SettingGroupTarget::ProcessPriorityForegroundDetection,
                ),
                vec![
                    setting_group_action_row(
                        "process-priority-foreground-default-row",
                        t!("process_priority.foreground_default").to_string(),
                        self.render_process_priority_default_selector(
                            ProcessPriorityDefaultTarget::Foreground,
                            self.settings.process_priority.foreground_priority,
                            self.settings.process_priority.foreground_detection_enabled,
                            window,
                            cx,
                        ),
                        false,
                    )
                    .into_any_element(),
                    setting_group_action_row(
                        "process-priority-preserve-foreground-row",
                        t!("common.preserve_foreground_priority").to_string(),
                        setting_group_switch_action(
                            "process-priority-preserve-foreground-toggle",
                            self.settings.process_priority.preserve_foreground_priority,
                            cx.listener(|app, checked, _, cx| {
                                app.settings.process_priority.preserve_foreground_priority =
                                    *checked;
                                cx.notify();
                            }),
                        ),
                        false,
                    )
                    .into_any_element(),
                ],
                window,
                cx,
            ))
            .child(self.render_process_priority_status_card())
            .child(section_header(
                &t!("process_priority.exclusions"),
                t!("process_priority.exclusions_help").to_string(),
            ))
            .child(
                h_flex()
                    .gap_2()
                    .items_start()
                    .flex_wrap()
                    .child(self.render_process_picker(
                        "process-priority-process-suggestion",
                        &self.inputs.process_priority_process,
                        SuggestionTarget::ProcessPriority,
                        window,
                        cx,
                    ))
                    .child(
                        primary_control_button(Button::new("add-process-priority-exclusion"), cx)
                            .label(t!("common.add").to_string())
                            .disabled(
                                !enabled
                                    || !can_add_process_priority_exclusion(
                                        &self.settings.process_priority,
                                        &input_value,
                                    ),
                            )
                            .on_click(cx.listener(|app, _, window, cx| {
                                let process = app
                                    .inputs
                                    .process_priority_process
                                    .read(cx)
                                    .value()
                                    .to_string();
                                if can_add_process_priority_exclusion(
                                    &app.settings.process_priority,
                                    &process,
                                ) {
                                    app.settings
                                        .process_priority
                                        .exclusions
                                        .push(new_process_exclusion_rule(&process));
                                    clear_input(&app.inputs.process_priority_process, window, cx);
                                }
                                cx.notify();
                            })),
                    ),
            )
            .child(self.render_process_priority_exclusions(window, cx));

        self.page_shell(Page::ProcessPriority, cx)
            .child(master_card)
            .child(disabled_feature_body(
                "process-priority-body",
                body,
                enabled,
                cx,
            ))
            .into_any_element()
    }

    pub(in crate::ui::app) fn render_process_priority_exclusions(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.render_priority_exclusion_table(
            "process-priority-exclusion",
            &self.settings.process_priority.exclusions,
            ListItemRemovalKind::ProcessPriorityExclusion,
            t!("process_priority.no_exclusions").to_string(),
            window,
            cx,
        )
    }

    pub(in crate::ui::app) fn render_process_priority_status_card(&self) -> gpui::Div {
        let status = &self.process_priority_status;
        let message = if status.message.is_empty() {
            t!("process_priority.not_checked").to_string()
        } else {
            localized_runtime_status(&status.message)
        };
        let mut rows = vec![
            (t!("common.status").to_string(), message),
            (
                t!("process_priority.adjusted_processes").to_string(),
                status.adjusted_processes.to_string(),
            ),
            (
                t!("process_priority.scanned_processes").to_string(),
                status.scanned_processes.to_string(),
            ),
            (
                t!("process_priority.skipped_processes").to_string(),
                status.skipped_processes.to_string(),
            ),
            (
                t!("process_priority.failed_actions").to_string(),
                status.failed_processes.to_string(),
            ),
        ];
        if let Some(error) = &status.last_error {
            rows.push((t!("common.last_failure").to_string(), error.clone()));
        }
        stat_grid(rows)
    }

    pub(in crate::ui::app) fn render_process_priority_default_selector(
        &self,
        target: ProcessPriorityDefaultTarget,
        selected_priority: ProcessPrioritySetting,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = match target {
            ProcessPriorityDefaultTarget::Background => "process-priority-background-default",
            ProcessPriorityDefaultTarget::Foreground => "process-priority-foreground-default",
        };
        let priorities: &[ProcessPrioritySetting] =
            if self.settings.advanced.expose_all_priority_values {
                &ProcessPrioritySetting::ADVANCED_ALL
            } else {
                &ProcessPrioritySetting::ALL
            };
        self.render_dropdown_select(
            id,
            process_priority_setting_label(selected_priority),
            enabled,
            DropdownSelectWidth::Standard,
            priorities.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for priority in priorities.iter().copied() {
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!("{id}-option-{priority:?}")),
                            process_priority_setting_label(priority),
                            selected_priority == priority,
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            match target {
                                ProcessPriorityDefaultTarget::Background => {
                                    app.settings.process_priority.background_priority = priority;
                                }
                                ProcessPriorityDefaultTarget::Foreground => {
                                    app.settings.process_priority.foreground_priority = priority;
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
