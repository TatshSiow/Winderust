use crate::ui::app::*;

impl WinderustApp {
    pub(in crate::ui::app) fn render_memory_priority_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let input_value = self.process_picker_path(
            SuggestionTarget::MemoryPriority,
            &self.inputs.memory_priority_process,
            cx,
        );
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
                    priority_level_label(
                        PriorityDefaultTarget::Background,
                        t!("nav.memory_priority").to_string(),
                    ),
                    self.render_memory_priority_default_selector(
                        PriorityDefaultTarget::Background,
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
                        priority_level_label(
                            PriorityDefaultTarget::Foreground,
                            t!("nav.memory_priority").to_string(),
                        ),
                        self.render_memory_priority_default_selector(
                            PriorityDefaultTarget::Foreground,
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
            .child(setting_group_with_help(
                SettingGroupTarget::MemoryPriorityVisibleWindowDetection,
                (
                    t!("common.visible_window_detection").to_string(),
                    t!("common.visible_window_detection_help").to_string(),
                ),
                setting_group_switch_action(
                    "memory-priority-visible-window-detection-toggle",
                    self.settings
                        .memory_priority
                        .visible_window_detection_enabled,
                    cx.listener(|app, checked, _, cx| {
                        app.settings
                            .memory_priority
                            .visible_window_detection_enabled = *checked;
                        cx.notify();
                    }),
                ),
                self.is_setting_group_collapsed(
                    SettingGroupTarget::MemoryPriorityVisibleWindowDetection,
                ),
                vec![
                    setting_group_action_row(
                        "memory-priority-visible-window-default-row",
                        priority_level_label(
                            PriorityDefaultTarget::VisibleWindow,
                            t!("nav.memory_priority").to_string(),
                        ),
                        self.render_memory_priority_default_selector(
                            PriorityDefaultTarget::VisibleWindow,
                            self.settings.memory_priority.visible_window_priority,
                            self.settings
                                .memory_priority
                                .visible_window_detection_enabled,
                            window,
                            cx,
                        ),
                        false,
                    )
                    .into_any_element(),
                    setting_group_action_row(
                        "memory-priority-preserve-visible-window-row",
                        t!("common.preserve_visible_window_priority").to_string(),
                        setting_group_switch_action(
                            "memory-priority-preserve-visible-window-toggle",
                            self.settings
                                .memory_priority
                                .preserve_visible_window_priority,
                            cx.listener(|app, checked, _, cx| {
                                app.settings
                                    .memory_priority
                                    .preserve_visible_window_priority = *checked;
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
                                let process = app.process_picker_path(
                                    SuggestionTarget::MemoryPriority,
                                    &app.inputs.memory_priority_process,
                                    cx,
                                );
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
        target: PriorityDefaultTarget,
        selected_priority: ProcessMemoryPrioritySetting,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = match target {
            PriorityDefaultTarget::Background => "memory-priority-background-default",
            PriorityDefaultTarget::VisibleWindow => "memory-priority-visible-window-default",
            PriorityDefaultTarget::Foreground => "memory-priority-foreground-default",
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
                                PriorityDefaultTarget::Background => {
                                    app.settings.memory_priority.background_priority = priority;
                                }
                                PriorityDefaultTarget::VisibleWindow => {
                                    app.settings.memory_priority.visible_window_priority = priority;
                                }
                                PriorityDefaultTarget::Foreground => {
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
