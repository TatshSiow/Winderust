use crate::ui::app::*;

impl WinderustApp {
    pub(in crate::ui::app) fn render_io_priority_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let input_value = self.inputs.io_priority_process.read(cx).value().to_string();
        let enabled = self.settings.io_priority.enabled;
        let help = tooltip_lines(vec![
            t!("io_priority.intro_1").to_string(),
            t!("io_priority.intro_2").to_string(),
        ]);
        let master_card = setting_group_with_help(
            SettingGroupTarget::IoPriorityMaster,
            (t!("io_priority.enable").to_string(), help),
            setting_group_switch_action(
                "io-priority-enabled-toggle",
                enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings.io_priority.enabled = *checked;
                    cx.notify();
                }),
            ),
            self.is_setting_group_collapsed(SettingGroupTarget::IoPriorityMaster),
            vec![
                setting_group_action_row(
                    "io-priority-background-default-row",
                    t!("io_priority.background_default").to_string(),
                    self.render_io_priority_default_selector(
                        IoPriorityDefaultTarget::Background,
                        self.settings.io_priority.background_priority,
                        enabled,
                        window,
                        cx,
                    ),
                    false,
                )
                .into_any_element(),
                setting_group_action_row(
                    "io-priority-preserve-background-row",
                    t!("common.preserve_background_priority").to_string(),
                    setting_group_switch_action(
                        "io-priority-preserve-background-toggle",
                        self.settings.io_priority.preserve_background_priority,
                        cx.listener(|app, checked, _, cx| {
                            app.settings.io_priority.preserve_background_priority = *checked;
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
                SettingGroupTarget::IoPriorityForegroundDetection,
                (
                    t!("io_priority.foreground_detection").to_string(),
                    t!("io_priority.foreground_detection_help").to_string(),
                ),
                setting_group_switch_action(
                    "io-priority-foreground-detection-toggle",
                    self.settings.io_priority.foreground_detection_enabled,
                    cx.listener(|app, checked, _, cx| {
                        app.settings.io_priority.foreground_detection_enabled = *checked;
                        cx.notify();
                    }),
                ),
                self.is_setting_group_collapsed(SettingGroupTarget::IoPriorityForegroundDetection),
                vec![
                    setting_group_action_row(
                        "io-priority-foreground-default-row",
                        t!("io_priority.foreground_default").to_string(),
                        self.render_io_priority_default_selector(
                            IoPriorityDefaultTarget::Foreground,
                            self.settings.io_priority.foreground_priority,
                            self.settings.io_priority.foreground_detection_enabled,
                            window,
                            cx,
                        ),
                        false,
                    )
                    .into_any_element(),
                    setting_group_action_row(
                        "io-priority-preserve-foreground-row",
                        t!("common.preserve_foreground_priority").to_string(),
                        setting_group_switch_action(
                            "io-priority-preserve-foreground-toggle",
                            self.settings.io_priority.preserve_foreground_priority,
                            cx.listener(|app, checked, _, cx| {
                                app.settings.io_priority.preserve_foreground_priority = *checked;
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
            .child(section_header(
                &t!("io_priority.exclusions"),
                t!("io_priority.exclusions_help").to_string(),
            ))
            .child(
                h_flex()
                    .gap_2()
                    .items_start()
                    .flex_wrap()
                    .child(self.render_process_picker(
                        "io-priority-process-suggestion",
                        &self.inputs.io_priority_process,
                        SuggestionTarget::IoPriority,
                        window,
                        cx,
                    ))
                    .child(
                        primary_control_button(Button::new("add-io-priority-exclusion"), cx)
                            .label(t!("common.add").to_string())
                            .disabled(
                                !enabled
                                    || !can_add_io_priority_exclusion(
                                        &self.settings.io_priority,
                                        &input_value,
                                    ),
                            )
                            .on_click(cx.listener(|app, _, window, cx| {
                                let process =
                                    app.inputs.io_priority_process.read(cx).value().to_string();
                                if can_add_io_priority_exclusion(
                                    &app.settings.io_priority,
                                    &process,
                                ) {
                                    app.settings
                                        .io_priority
                                        .exclusions
                                        .push(new_process_exclusion_rule(&process));
                                    clear_input(&app.inputs.io_priority_process, window, cx);
                                }
                                cx.notify();
                            })),
                    ),
            )
            .child(self.render_io_priority_exclusions(window, cx));

        self.page_shell(Page::IoPriority, cx)
            .child(master_card)
            .child(disabled_feature_body("io-priority-body", body, enabled, cx))
            .into_any_element()
    }

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

    pub(in crate::ui::app) fn render_io_priority_exclusions(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.render_priority_exclusion_table(
            "io-priority-exclusion",
            &self.settings.io_priority.exclusions,
            ListItemRemovalKind::IoPriorityExclusion,
            t!("io_priority.no_exclusions").to_string(),
            window,
            cx,
        )
    }

    pub(in crate::ui::app) fn render_priority_exclusion_table(
        &self,
        id_prefix: &'static str,
        rules: &[ProcessExclusionRule],
        kind: ListItemRemovalKind,
        empty_message: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut table = v_flex()
            .w_full()
            .min_w(px(0.0))
            .overflow_hidden()
            .rounded(px(BRAND_RADIUS_SURFACE))
            .border_1()
            .border_color(rgb(border_color()))
            .bg(rgb(settings_card_color()))
            .child(
                h_flex()
                    .w_full()
                    .min_w(px(0.0))
                    .h(px(32.0))
                    .items_center()
                    .gap_2()
                    .px_4()
                    .border_b_1()
                    .border_color(rgb(border_color()))
                    .text_size(px(TEXT_LABEL_SIZE))
                    .line_height(px(TEXT_LABEL_LINE_HEIGHT))
                    .text_color(rgb(muted_text_color()))
                    .child(rule_table_active_header())
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .truncate()
                            .child(t!("process_list.process_name").to_string()),
                    )
                    .child(priority_exclusion_table_cell(
                        t!("process_list.foreground").to_string(),
                    ))
                    .child(priority_exclusion_table_cell(
                        t!("process_list.background").to_string(),
                    ))
                    .child(rule_table_action_header()),
            );

        for (index, rule) in rules.iter().enumerate() {
            let process = rule.process_name.clone();
            let target = ListItemRemovalTarget::new(kind, index);
            let row = h_flex()
                .id(SharedString::from(format!("{id_prefix}-row-{index}")))
                .w_full()
                .min_w(px(0.0))
                .h(px(CARD_ROW_HEIGHT))
                .items_center()
                .gap_2()
                .px_4()
                .border_b_1()
                .border_color(rgb(border_color()))
                .text_size(px(TEXT_BODY_SIZE))
                .line_height(px(TEXT_BODY_LINE_HEIGHT))
                .child(rule_active_cell(
                    format!("{id_prefix}-enabled-{index}"),
                    rule.enabled,
                    cx.listener(move |app, checked, _, cx| {
                        app.set_priority_exclusion_enabled(kind, index, *checked);
                        cx.notify();
                    }),
                ))
                .child(self.process_rule_title(&process, cx))
                .child(self.render_priority_exclusion_dropdown(kind, index, true, window, cx))
                .child(self.render_priority_exclusion_dropdown(kind, index, false, window, cx))
                .child(rule_table_action_cell(
                    danger_control_button(Button::new(SharedString::from(format!(
                        "remove-{id_prefix}-{index}"
                    ))))
                    .with_size(px(32.0))
                    .icon(Icon::new(NavIcon::Trash2).with_size(px(14.0)))
                    .tooltip(t!("common.remove").to_string())
                    .on_click(cx.listener(move |app, _, _, cx| {
                        app.request_list_item_removal(target, cx);
                    }))
                    .into_any_element(),
                ));

            table = table.child(self.animated_list_item(
                target,
                SharedString::from(format!("{id_prefix}-{index}")),
                row.into_any_element(),
            ));
        }

        if rules.is_empty() {
            table = table.child(text_muted(empty_message).p_4());
        }

        table.into_any_element()
    }

    pub(in crate::ui::app) fn render_priority_exclusion_dropdown(
        &self,
        kind: ListItemRemovalKind,
        index: usize,
        foreground: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match kind {
            ListItemRemovalKind::ProcessPriorityExclusion => {
                let selected = self.settings.process_priority.exclusions[index]
                    .process_priority_override(foreground);
                let priorities: &[ProcessPrioritySetting] =
                    if self.settings.advanced.expose_all_priority_values {
                        &ProcessPrioritySetting::CUSTOM_RULE_ADVANCED_ALL
                    } else {
                        &ProcessPrioritySetting::CUSTOM_RULE_ALL
                    };
                self.render_priority_rule_dropdown(
                    "process-priority-exclusion",
                    index,
                    foreground,
                    selected,
                    priorities,
                    process_priority_setting_label,
                    |app, index, foreground, priority| {
                        if let Some(rule) = app.settings.process_priority.exclusions.get_mut(index)
                        {
                            rule.set_process_priority_override(foreground, priority);
                        }
                    },
                    window,
                    cx,
                )
            }
            ListItemRemovalKind::ThreadPriorityExclusion => {
                let selected = self.settings.thread_priority.exclusions[index]
                    .thread_priority_override(foreground);
                let priorities: &[ProcessThreadPrioritySetting] =
                    if self.settings.advanced.expose_all_priority_values {
                        &ProcessThreadPrioritySetting::CUSTOM_RULE_ADVANCED_ALL
                    } else {
                        &ProcessThreadPrioritySetting::CUSTOM_RULE_ALL
                    };
                self.render_priority_rule_dropdown(
                    "thread-priority-exclusion",
                    index,
                    foreground,
                    selected,
                    priorities,
                    process_thread_priority_setting_label,
                    |app, index, foreground, priority| {
                        if let Some(rule) = app.settings.thread_priority.exclusions.get_mut(index) {
                            rule.set_thread_priority_override(foreground, priority);
                        }
                    },
                    window,
                    cx,
                )
            }
            ListItemRemovalKind::DynamicPriorityBoostExclusion => {
                let selected = self.settings.dynamic_priority_boost.exclusions[index]
                    .dynamic_priority_boost_override(foreground);
                self.render_priority_rule_dropdown(
                    "dynamic-priority-boost-exclusion",
                    index,
                    foreground,
                    selected,
                    &ProcessDynamicPriorityBoostSetting::CUSTOM_RULE_ALL,
                    process_dynamic_priority_boost_setting_label,
                    |app, index, foreground, boost| {
                        if let Some(rule) = app
                            .settings
                            .dynamic_priority_boost
                            .exclusions
                            .get_mut(index)
                        {
                            rule.set_dynamic_priority_boost_override(foreground, boost);
                        }
                    },
                    window,
                    cx,
                )
            }
            ListItemRemovalKind::IoPriorityExclusion => {
                let selected =
                    self.settings.io_priority.exclusions[index].io_priority_override(foreground);
                let priorities: &[ProcessIoPrioritySetting] =
                    if self.settings.advanced.expose_all_priority_values {
                        &ProcessIoPrioritySetting::CUSTOM_RULE_ADVANCED_ALL
                    } else {
                        &ProcessIoPrioritySetting::CUSTOM_RULE_ALL
                    };
                self.render_priority_rule_dropdown(
                    "io-priority-exclusion",
                    index,
                    foreground,
                    selected,
                    priorities,
                    process_io_priority_setting_label,
                    |app, index, foreground, priority| {
                        if let Some(rule) = app.settings.io_priority.exclusions.get_mut(index) {
                            rule.set_io_priority_override(foreground, priority);
                        }
                    },
                    window,
                    cx,
                )
            }
            ListItemRemovalKind::GpuPriorityExclusion => {
                let selected =
                    self.settings.gpu_priority.exclusions[index].gpu_priority_override(foreground);
                let priorities: &[ProcessGpuPrioritySetting] =
                    if self.settings.advanced.expose_all_priority_values {
                        &ProcessGpuPrioritySetting::CUSTOM_RULE_ADVANCED_ALL
                    } else {
                        &ProcessGpuPrioritySetting::CUSTOM_RULE_ALL
                    };
                self.render_priority_rule_dropdown(
                    "gpu-priority-exclusion",
                    index,
                    foreground,
                    selected,
                    priorities,
                    process_gpu_priority_setting_label,
                    |app, index, foreground, priority| {
                        if let Some(rule) = app.settings.gpu_priority.exclusions.get_mut(index) {
                            rule.set_gpu_priority_override(foreground, priority);
                        }
                    },
                    window,
                    cx,
                )
            }
            ListItemRemovalKind::MemoryPriorityExclusion => {
                let selected = self.settings.memory_priority.exclusions[index]
                    .memory_priority_override(foreground);
                self.render_priority_rule_dropdown(
                    "memory-priority-exclusion",
                    index,
                    foreground,
                    selected,
                    &ProcessMemoryPrioritySetting::CUSTOM_RULE_ALL,
                    process_memory_priority_setting_label,
                    |app, index, foreground, priority| {
                        if let Some(rule) = app.settings.memory_priority.exclusions.get_mut(index) {
                            rule.set_memory_priority_override(foreground, priority);
                        }
                    },
                    window,
                    cx,
                )
            }
            _ => priority_exclusion_table_cell(t!("common.none").to_string()),
        }
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "keeps six custom-rule dropdowns on one rendering path"
    )]
    fn render_priority_rule_dropdown<T>(
        &self,
        id_prefix: &'static str,
        index: usize,
        foreground: bool,
        selected: T,
        values: &[T],
        label: impl Fn(T) -> String + Copy + 'static,
        set: impl Fn(&mut Self, usize, bool, T) + Copy + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement
    where
        T: Copy + PartialEq + std::fmt::Debug + 'static,
    {
        let side = if foreground {
            "foreground"
        } else {
            "background"
        };
        self.render_dropdown_select(
            format!("{id_prefix}-{side}-{index}"),
            label(selected),
            true,
            DropdownSelectWidth::Table,
            values.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for value in values.iter().copied() {
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!("{id_prefix}-{side}-{index}-{value:?}")),
                            label(value),
                            selected == value,
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            set(app, index, foreground, value);
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                options
            },
        )
    }

    pub(in crate::ui::app) fn set_priority_exclusion_enabled(
        &mut self,
        kind: ListItemRemovalKind,
        index: usize,
        enabled: bool,
    ) {
        let rule = match kind {
            ListItemRemovalKind::ProcessPriorityExclusion => {
                self.settings.process_priority.exclusions.get_mut(index)
            }
            ListItemRemovalKind::ThreadPriorityExclusion => {
                self.settings.thread_priority.exclusions.get_mut(index)
            }
            ListItemRemovalKind::DynamicPriorityBoostExclusion => self
                .settings
                .dynamic_priority_boost
                .exclusions
                .get_mut(index),
            ListItemRemovalKind::IoPriorityExclusion => {
                self.settings.io_priority.exclusions.get_mut(index)
            }
            ListItemRemovalKind::GpuPriorityExclusion => {
                self.settings.gpu_priority.exclusions.get_mut(index)
            }
            ListItemRemovalKind::MemoryPriorityExclusion => {
                self.settings.memory_priority.exclusions.get_mut(index)
            }
            _ => None,
        };

        if let Some(rule) = rule {
            rule.enabled = enabled;
        }
    }

    pub(in crate::ui::app) fn render_io_priority_default_selector(
        &self,
        target: IoPriorityDefaultTarget,
        selected_priority: ProcessIoPrioritySetting,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = match target {
            IoPriorityDefaultTarget::Background => "io-priority-background-default",
            IoPriorityDefaultTarget::Foreground => "io-priority-foreground-default",
        };
        let priorities: &[ProcessIoPrioritySetting] =
            if self.settings.advanced.expose_all_priority_values {
                &ProcessIoPrioritySetting::ADVANCED_ALL
            } else {
                &ProcessIoPrioritySetting::ALL
            };
        let dropdown = self.render_dropdown_select(
            id,
            process_io_priority_setting_label(selected_priority),
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
                            process_io_priority_setting_label(priority),
                            selected_priority == priority,
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            match target {
                                IoPriorityDefaultTarget::Background => {
                                    app.settings.io_priority.background_priority = priority;
                                }
                                IoPriorityDefaultTarget::Foreground => {
                                    app.settings.io_priority.foreground_priority = priority;
                                }
                            }
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                options
            },
        );
        dropdown
    }

    pub(in crate::ui::app) fn render_gpu_priority_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let input_value = self
            .inputs
            .gpu_priority_process
            .read(cx)
            .value()
            .to_string();
        let enabled = self.settings.gpu_priority.enabled;
        let help = tooltip_lines(vec![
            t!("gpu_priority.intro_1").to_string(),
            t!("gpu_priority.intro_2").to_string(),
            t!("gpu_priority.intro_3").to_string(),
        ]);
        let master_card = setting_group_with_help(
            SettingGroupTarget::GpuPriorityMaster,
            (t!("gpu_priority.enable").to_string(), help),
            setting_group_switch_action(
                "gpu-priority-enabled-toggle",
                enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings.gpu_priority.enabled = *checked;
                    cx.notify();
                }),
            ),
            self.is_setting_group_collapsed(SettingGroupTarget::GpuPriorityMaster),
            vec![
                setting_group_action_row(
                    "gpu-priority-background-default-row",
                    t!("gpu_priority.background_default").to_string(),
                    self.render_gpu_priority_default_selector(
                        GpuPriorityDefaultTarget::Background,
                        self.settings.gpu_priority.background_priority,
                        enabled,
                        window,
                        cx,
                    ),
                    false,
                )
                .into_any_element(),
                setting_group_action_row(
                    "gpu-priority-preserve-background-row",
                    t!("common.preserve_background_priority").to_string(),
                    setting_group_switch_action(
                        "gpu-priority-preserve-background-toggle",
                        self.settings.gpu_priority.preserve_background_priority,
                        cx.listener(|app, checked, _, cx| {
                            app.settings.gpu_priority.preserve_background_priority = *checked;
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
                SettingGroupTarget::GpuPriorityForegroundDetection,
                (
                    t!("gpu_priority.foreground_detection").to_string(),
                    t!("gpu_priority.foreground_detection_help").to_string(),
                ),
                setting_group_switch_action(
                    "gpu-priority-foreground-detection-toggle",
                    self.settings.gpu_priority.foreground_detection_enabled,
                    cx.listener(|app, checked, _, cx| {
                        app.settings.gpu_priority.foreground_detection_enabled = *checked;
                        cx.notify();
                    }),
                ),
                self.is_setting_group_collapsed(SettingGroupTarget::GpuPriorityForegroundDetection),
                vec![
                    setting_group_action_row(
                        "gpu-priority-foreground-default-row",
                        t!("gpu_priority.foreground_default").to_string(),
                        self.render_gpu_priority_default_selector(
                            GpuPriorityDefaultTarget::Foreground,
                            self.settings.gpu_priority.foreground_priority,
                            self.settings.gpu_priority.foreground_detection_enabled,
                            window,
                            cx,
                        ),
                        false,
                    )
                    .into_any_element(),
                    setting_group_action_row(
                        "gpu-priority-preserve-foreground-row",
                        t!("common.preserve_foreground_priority").to_string(),
                        setting_group_switch_action(
                            "gpu-priority-preserve-foreground-toggle",
                            self.settings.gpu_priority.preserve_foreground_priority,
                            cx.listener(|app, checked, _, cx| {
                                app.settings.gpu_priority.preserve_foreground_priority = *checked;
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
            .child(self.render_gpu_priority_status_card())
            .child(section_header(
                &t!("gpu_priority.exclusions"),
                t!("gpu_priority.exclusions_help").to_string(),
            ))
            .child(
                h_flex()
                    .gap_2()
                    .items_start()
                    .flex_wrap()
                    .child(self.render_process_picker(
                        "gpu-priority-process-suggestion",
                        &self.inputs.gpu_priority_process,
                        SuggestionTarget::GpuPriority,
                        window,
                        cx,
                    ))
                    .child(
                        primary_control_button(Button::new("add-gpu-priority-rule"), cx)
                            .label(t!("common.add").to_string())
                            .disabled(
                                !enabled
                                    || !can_add_gpu_priority_exclusion(
                                        &self.settings.gpu_priority,
                                        &input_value,
                                    ),
                            )
                            .on_click(cx.listener(|app, _, window, cx| {
                                let process =
                                    app.inputs.gpu_priority_process.read(cx).value().to_string();
                                if can_add_gpu_priority_exclusion(
                                    &app.settings.gpu_priority,
                                    &process,
                                ) {
                                    app.settings
                                        .gpu_priority
                                        .exclusions
                                        .push(new_process_exclusion_rule(&process));
                                    clear_input(&app.inputs.gpu_priority_process, window, cx);
                                }
                                cx.notify();
                            })),
                    ),
            )
            .child(self.render_gpu_priority_exclusions(window, cx));

        self.page_shell(Page::GpuPriority, cx)
            .child(master_card)
            .child(disabled_feature_body(
                "gpu-priority-body",
                body,
                enabled,
                cx,
            ))
            .into_any_element()
    }

    pub(in crate::ui::app) fn render_gpu_priority_exclusions(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.render_priority_exclusion_table(
            "gpu-priority-exclusion",
            &self.settings.gpu_priority.exclusions,
            ListItemRemovalKind::GpuPriorityExclusion,
            t!("gpu_priority.no_exclusions").to_string(),
            window,
            cx,
        )
    }

    pub(in crate::ui::app) fn render_gpu_priority_status_card(&self) -> gpui::Div {
        let status = &self.gpu_priority_status;
        let message = if status.message.is_empty() {
            t!("gpu_priority.not_checked").to_string()
        } else {
            localized_runtime_status(&status.message)
        };
        let mut rows = vec![
            (t!("common.status").to_string(), message),
            (
                t!("gpu_priority.adjusted_processes").to_string(),
                status.adjusted_processes.to_string(),
            ),
            (
                t!("gpu_priority.pending_processes").to_string(),
                status.pending_processes.to_string(),
            ),
            (
                t!("gpu_priority.denied_processes").to_string(),
                status.denied_processes.to_string(),
            ),
            (
                t!("gpu_priority.suppressed_processes").to_string(),
                status.suppressed_processes.to_string(),
            ),
            (
                t!("gpu_priority.scanned_processes").to_string(),
                status.scanned_processes.to_string(),
            ),
            (
                t!("gpu_priority.skipped_processes").to_string(),
                status.skipped_processes.to_string(),
            ),
            (
                t!("gpu_priority.failed_actions").to_string(),
                status.failed_processes.to_string(),
            ),
        ];
        if let Some(error) = &status.last_error {
            rows.push((t!("common.last_failure").to_string(), error.clone()));
        }
        stat_grid(rows)
    }

    pub(in crate::ui::app) fn render_gpu_priority_default_selector(
        &self,
        target: GpuPriorityDefaultTarget,
        selected_priority: ProcessGpuPrioritySetting,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = match target {
            GpuPriorityDefaultTarget::Background => "gpu-priority-background-default",
            GpuPriorityDefaultTarget::Foreground => "gpu-priority-foreground-default",
        };
        let priorities: &[ProcessGpuPrioritySetting] =
            if self.settings.advanced.expose_all_priority_values {
                &ProcessGpuPrioritySetting::ADVANCED_ALL
            } else {
                &ProcessGpuPrioritySetting::ALL
            };
        let dropdown = self.render_dropdown_select(
            id,
            process_gpu_priority_setting_label(selected_priority),
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
                            process_gpu_priority_setting_label(priority),
                            selected_priority == priority,
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            match target {
                                GpuPriorityDefaultTarget::Background => {
                                    app.settings.gpu_priority.background_priority = priority;
                                }
                                GpuPriorityDefaultTarget::Foreground => {
                                    app.settings.gpu_priority.foreground_priority = priority;
                                }
                            }
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                options
            },
        );
        dropdown
    }

    pub(in crate::ui::app) fn render_memory_priority_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let input_value = self
            .inputs
            .memory_priority_process
            .read(cx)
            .value()
            .to_string();
        let enabled = self.settings.memory_priority.enabled;
        let help = tooltip_lines(vec![
            t!("memory_priority.intro_1").to_string(),
            t!("memory_priority.intro_2").to_string(),
        ]);
        let master_card = setting_group_with_help(
            SettingGroupTarget::MemoryPriorityMaster,
            (t!("memory_priority.enable").to_string(), help),
            setting_group_switch_action(
                "memory-priority-enabled-toggle",
                enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings.memory_priority.enabled = *checked;
                    cx.notify();
                }),
            ),
            self.is_setting_group_collapsed(SettingGroupTarget::MemoryPriorityMaster),
            vec![
                setting_group_action_row(
                    "memory-priority-background-default-row",
                    t!("memory_priority.background_default").to_string(),
                    self.render_memory_priority_default_selector(
                        MemoryPriorityDefaultTarget::Background,
                        self.settings.memory_priority.background_priority,
                        enabled,
                        window,
                        cx,
                    ),
                    false,
                )
                .into_any_element(),
                setting_group_action_row(
                    "memory-priority-preserve-background-row",
                    t!("common.preserve_background_priority").to_string(),
                    setting_group_switch_action(
                        "memory-priority-preserve-background-toggle",
                        self.settings.memory_priority.preserve_background_priority,
                        cx.listener(|app, checked, _, cx| {
                            app.settings.memory_priority.preserve_background_priority = *checked;
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
                SettingGroupTarget::MemoryPriorityForegroundDetection,
                (
                    t!("memory_priority.foreground_detection").to_string(),
                    t!("memory_priority.foreground_detection_help").to_string(),
                ),
                setting_group_switch_action(
                    "memory-priority-foreground-detection-toggle",
                    self.settings.memory_priority.foreground_detection_enabled,
                    cx.listener(|app, checked, _, cx| {
                        app.settings.memory_priority.foreground_detection_enabled = *checked;
                        cx.notify();
                    }),
                ),
                self.is_setting_group_collapsed(
                    SettingGroupTarget::MemoryPriorityForegroundDetection,
                ),
                vec![
                    setting_group_action_row(
                        "memory-priority-foreground-default-row",
                        t!("memory_priority.foreground_default").to_string(),
                        self.render_memory_priority_default_selector(
                            MemoryPriorityDefaultTarget::Foreground,
                            self.settings.memory_priority.foreground_priority,
                            self.settings.memory_priority.foreground_detection_enabled,
                            window,
                            cx,
                        ),
                        false,
                    )
                    .into_any_element(),
                    setting_group_action_row(
                        "memory-priority-preserve-foreground-row",
                        t!("common.preserve_foreground_priority").to_string(),
                        setting_group_switch_action(
                            "memory-priority-preserve-foreground-toggle",
                            self.settings.memory_priority.preserve_foreground_priority,
                            cx.listener(|app, checked, _, cx| {
                                app.settings.memory_priority.preserve_foreground_priority =
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
            .child(section_header(
                &t!("memory_priority.exclusions"),
                t!("memory_priority.exclusions_help").to_string(),
            ))
            .child(
                h_flex()
                    .gap_2()
                    .items_start()
                    .flex_wrap()
                    .child(self.render_process_picker(
                        "memory-priority-process-suggestion",
                        &self.inputs.memory_priority_process,
                        SuggestionTarget::MemoryPriority,
                        window,
                        cx,
                    ))
                    .child(
                        primary_control_button(Button::new("add-memory-priority-exclusion"), cx)
                            .label(t!("common.add").to_string())
                            .disabled(
                                !enabled
                                    || !can_add_memory_priority_exclusion(
                                        &self.settings.memory_priority,
                                        &input_value,
                                    ),
                            )
                            .on_click(cx.listener(|app, _, window, cx| {
                                let process = app
                                    .inputs
                                    .memory_priority_process
                                    .read(cx)
                                    .value()
                                    .to_string();
                                if can_add_memory_priority_exclusion(
                                    &app.settings.memory_priority,
                                    &process,
                                ) {
                                    app.settings
                                        .memory_priority
                                        .exclusions
                                        .push(new_process_exclusion_rule(&process));
                                    clear_input(&app.inputs.memory_priority_process, window, cx);
                                }
                                cx.notify();
                            })),
                    ),
            )
            .child(self.render_memory_priority_exclusions(window, cx));

        self.page_shell(Page::MemoryPriority, cx)
            .child(master_card)
            .child(disabled_feature_body(
                "memory-priority-body",
                body,
                enabled,
                cx,
            ))
            .into_any_element()
    }

    pub(in crate::ui::app) fn render_memory_priority_exclusions(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.render_priority_exclusion_table(
            "memory-priority-exclusion",
            &self.settings.memory_priority.exclusions,
            ListItemRemovalKind::MemoryPriorityExclusion,
            t!("memory_priority.no_exclusions").to_string(),
            window,
            cx,
        )
    }

    pub(in crate::ui::app) fn render_memory_priority_default_selector(
        &self,
        target: MemoryPriorityDefaultTarget,
        selected_priority: ProcessMemoryPrioritySetting,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = match target {
            MemoryPriorityDefaultTarget::Background => "memory-priority-background-default",
            MemoryPriorityDefaultTarget::Foreground => "memory-priority-foreground-default",
        };
        let dropdown = self.render_dropdown_select(
            id,
            process_memory_priority_setting_label(selected_priority),
            enabled,
            DropdownSelectWidth::Standard,
            ProcessMemoryPrioritySetting::ALL.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for priority in ProcessMemoryPrioritySetting::ALL {
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!("{id}-option-{priority:?}")),
                            process_memory_priority_setting_label(priority),
                            selected_priority == priority,
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            match target {
                                MemoryPriorityDefaultTarget::Background => {
                                    app.settings.memory_priority.background_priority = priority;
                                }
                                MemoryPriorityDefaultTarget::Foreground => {
                                    app.settings.memory_priority.foreground_priority = priority;
                                }
                            }
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                options
            },
        );
        dropdown
    }
}
