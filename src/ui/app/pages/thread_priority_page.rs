use crate::ui::app::*;

impl WinderustApp {
    pub(in crate::ui::app) fn render_thread_priority_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let input_value = self
            .inputs
            .thread_priority_process
            .read(cx)
            .value()
            .to_string();
        let enabled = self.settings.thread_priority.enabled;
        let help = tooltip_lines(vec![
            t!("thread_priority.intro_1").to_string(),
            t!("thread_priority.intro_2").to_string(),
        ]);
        let master_card = setting_group_with_help(
            SettingGroupTarget::ThreadPriorityMaster,
            (t!("thread_priority.enable").to_string(), help),
            setting_group_switch_action(
                "thread-priority-enabled-toggle",
                enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings.thread_priority.enabled = *checked;
                    cx.notify();
                }),
            ),
            self.is_setting_group_collapsed(SettingGroupTarget::ThreadPriorityMaster),
            vec![
                setting_group_action_row(
                    "thread-priority-background-default-row",
                    t!("thread_priority.background_default").to_string(),
                    self.render_thread_priority_default_selector(
                        ThreadPriorityDefaultTarget::Background,
                        self.settings.thread_priority.background_priority,
                        enabled,
                        window,
                        cx,
                    ),
                    false,
                )
                .into_any_element(),
                setting_group_action_row(
                    "thread-priority-preserve-background-row",
                    t!("common.preserve_background_priority").to_string(),
                    setting_group_switch_action(
                        "thread-priority-preserve-background-toggle",
                        self.settings.thread_priority.preserve_background_priority,
                        cx.listener(|app, checked, _, cx| {
                            app.settings.thread_priority.preserve_background_priority = *checked;
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
                SettingGroupTarget::ThreadPriorityForegroundDetection,
                (
                    t!("thread_priority.foreground_detection").to_string(),
                    t!("thread_priority.foreground_detection_help").to_string(),
                ),
                setting_group_switch_action(
                    "thread-priority-foreground-detection-toggle",
                    self.settings.thread_priority.foreground_detection_enabled,
                    cx.listener(|app, checked, _, cx| {
                        app.settings.thread_priority.foreground_detection_enabled = *checked;
                        cx.notify();
                    }),
                ),
                self.is_setting_group_collapsed(
                    SettingGroupTarget::ThreadPriorityForegroundDetection,
                ),
                vec![
                    setting_group_action_row(
                        "thread-priority-foreground-default-row",
                        t!("thread_priority.foreground_default").to_string(),
                        self.render_thread_priority_default_selector(
                            ThreadPriorityDefaultTarget::Foreground,
                            self.settings.thread_priority.foreground_priority,
                            self.settings.thread_priority.foreground_detection_enabled,
                            window,
                            cx,
                        ),
                        false,
                    )
                    .into_any_element(),
                    setting_group_action_row(
                        "thread-priority-preserve-foreground-row",
                        t!("common.preserve_foreground_priority").to_string(),
                        setting_group_switch_action(
                            "thread-priority-preserve-foreground-toggle",
                            self.settings.thread_priority.preserve_foreground_priority,
                            cx.listener(|app, checked, _, cx| {
                                app.settings.thread_priority.preserve_foreground_priority =
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
            .child(self.render_thread_priority_status_card())
            .child(section_header(
                &t!("thread_priority.exclusions"),
                t!("thread_priority.exclusions_help").to_string(),
            ))
            .child(
                h_flex()
                    .gap_2()
                    .items_start()
                    .flex_wrap()
                    .child(self.render_process_picker(
                        "thread-priority-process-suggestion",
                        &self.inputs.thread_priority_process,
                        SuggestionTarget::ThreadPriority,
                        window,
                        cx,
                    ))
                    .child(
                        primary_control_button(Button::new("add-thread-priority-exclusion"), cx)
                            .label(t!("common.add").to_string())
                            .disabled(
                                !enabled
                                    || !can_add_thread_priority_exclusion(
                                        &self.settings.thread_priority,
                                        &input_value,
                                    ),
                            )
                            .on_click(cx.listener(|app, _, window, cx| {
                                let process = app
                                    .inputs
                                    .thread_priority_process
                                    .read(cx)
                                    .value()
                                    .to_string();
                                if can_add_thread_priority_exclusion(
                                    &app.settings.thread_priority,
                                    &process,
                                ) {
                                    app.settings
                                        .thread_priority
                                        .exclusions
                                        .push(new_process_exclusion_rule(&process));
                                    clear_input(&app.inputs.thread_priority_process, window, cx);
                                }
                                cx.notify();
                            })),
                    ),
            )
            .child(self.render_thread_priority_exclusions(window, cx));

        self.page_shell(Page::ThreadPriority, cx)
            .child(master_card)
            .child(disabled_feature_body(
                "thread-priority-body",
                body,
                enabled,
                cx,
            ))
            .into_any_element()
    }

    pub(in crate::ui::app) fn render_thread_priority_exclusions(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.render_priority_exclusion_table(
            "thread-priority-exclusion",
            &self.settings.thread_priority.exclusions,
            ListItemRemovalKind::ThreadPriorityExclusion,
            t!("thread_priority.no_exclusions").to_string(),
            window,
            cx,
        )
    }

    pub(in crate::ui::app) fn render_thread_priority_status_card(&self) -> gpui::Div {
        let status = &self.thread_priority_status;
        let message = if status.message.is_empty() {
            t!("thread_priority.not_checked").to_string()
        } else {
            localized_runtime_status(&status.message)
        };
        let mut rows = vec![
            (t!("common.status").to_string(), message),
            (
                t!("thread_priority.adjusted_processes").to_string(),
                status.adjusted_processes.to_string(),
            ),
            (
                t!("thread_priority.adjusted_threads").to_string(),
                status.adjusted_threads.to_string(),
            ),
            (
                t!("thread_priority.scanned_processes").to_string(),
                status.scanned_processes.to_string(),
            ),
            (
                t!("thread_priority.skipped_processes").to_string(),
                status.skipped_processes.to_string(),
            ),
            (
                t!("thread_priority.failed_actions").to_string(),
                status.failed_processes.to_string(),
            ),
        ];
        if let Some(error) = &status.last_error {
            rows.push((t!("common.last_failure").to_string(), error.clone()));
        }
        stat_grid(rows)
    }

    pub(in crate::ui::app) fn render_thread_priority_default_selector(
        &self,
        target: ThreadPriorityDefaultTarget,
        selected_priority: ProcessThreadPrioritySetting,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = match target {
            ThreadPriorityDefaultTarget::Background => "thread-priority-background-default",
            ThreadPriorityDefaultTarget::Foreground => "thread-priority-foreground-default",
        };
        let priorities: &[ProcessThreadPrioritySetting] =
            if self.settings.advanced.expose_all_priority_values {
                &ProcessThreadPrioritySetting::ADVANCED_ALL
            } else {
                &ProcessThreadPrioritySetting::ALL
            };
        self.render_dropdown_select(
            id,
            process_thread_priority_setting_label(selected_priority),
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
                            process_thread_priority_setting_label(priority),
                            selected_priority == priority,
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            match target {
                                ThreadPriorityDefaultTarget::Background => {
                                    app.settings.thread_priority.background_priority = priority;
                                }
                                ThreadPriorityDefaultTarget::Foreground => {
                                    app.settings.thread_priority.foreground_priority = priority;
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
