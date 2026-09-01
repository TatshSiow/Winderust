use crate::ui::app::*;

impl WinderustApp {
    pub(in crate::ui::app) fn render_gpu_priority_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let input_value = self.process_picker_path(
            SuggestionTarget::GpuPriority,
            &self.inputs.gpu_priority_process,
            cx,
        );
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
                    priority_level_label(
                        PriorityDefaultTarget::Background,
                        t!("nav.gpu_priority").to_string(),
                    ),
                    self.render_gpu_priority_default_selector(
                        PriorityDefaultTarget::Background,
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
        let body = feature_body()
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
                        priority_level_label(
                            PriorityDefaultTarget::Foreground,
                            t!("nav.gpu_priority").to_string(),
                        ),
                        self.render_gpu_priority_default_selector(
                            PriorityDefaultTarget::Foreground,
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
            .child(setting_group_with_help(
                SettingGroupTarget::GpuPriorityVisibleWindowDetection,
                (
                    t!("common.visible_window_detection").to_string(),
                    t!("common.visible_window_detection_help").to_string(),
                ),
                setting_group_switch_action(
                    "gpu-priority-visible-window-detection-toggle",
                    self.settings.gpu_priority.visible_window_detection_enabled,
                    cx.listener(|app, checked, _, cx| {
                        app.settings.gpu_priority.visible_window_detection_enabled = *checked;
                        cx.notify();
                    }),
                ),
                self.is_setting_group_collapsed(
                    SettingGroupTarget::GpuPriorityVisibleWindowDetection,
                ),
                vec![
                    setting_group_action_row(
                        "gpu-priority-visible-window-default-row",
                        priority_level_label(
                            PriorityDefaultTarget::VisibleWindow,
                            t!("nav.gpu_priority").to_string(),
                        ),
                        self.render_gpu_priority_default_selector(
                            PriorityDefaultTarget::VisibleWindow,
                            self.settings.gpu_priority.visible_window_priority,
                            self.settings.gpu_priority.visible_window_detection_enabled,
                            window,
                            cx,
                        ),
                        false,
                    )
                    .into_any_element(),
                    setting_group_action_row(
                        "gpu-priority-preserve-visible-window-row",
                        t!("common.preserve_visible_window_priority").to_string(),
                        setting_group_switch_action(
                            "gpu-priority-preserve-visible-window-toggle",
                            self.settings.gpu_priority.preserve_visible_window_priority,
                            cx.listener(|app, checked, _, cx| {
                                app.settings.gpu_priority.preserve_visible_window_priority =
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
                                let process = app.process_picker_path(
                                    SuggestionTarget::GpuPriority,
                                    &app.inputs.gpu_priority_process,
                                    cx,
                                );
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

        page_body_shell()
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

    pub(in crate::ui::app) fn render_gpu_priority_default_selector(
        &self,
        target: PriorityDefaultTarget,
        selected_priority: ProcessGpuPrioritySetting,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = match target {
            PriorityDefaultTarget::Background => "gpu-priority-background-default",
            PriorityDefaultTarget::VisibleWindow => "gpu-priority-visible-window-default",
            PriorityDefaultTarget::Foreground => "gpu-priority-foreground-default",
        };
        let priorities: &[ProcessGpuPrioritySetting] =
            if self.settings.advanced.expose_all_priority_values {
                &ProcessGpuPrioritySetting::ADVANCED_ALL
            } else {
                &ProcessGpuPrioritySetting::ALL
            };
        self.render_priority_default_dropdown(
            id,
            target,
            selected_priority,
            enabled,
            priorities,
            process_gpu_priority_setting_label,
            |app, target, priority| match target {
                PriorityDefaultTarget::Background => {
                    app.settings.gpu_priority.background_priority = priority;
                }
                PriorityDefaultTarget::VisibleWindow => {
                    app.settings.gpu_priority.visible_window_priority = priority;
                }
                PriorityDefaultTarget::Foreground => {
                    app.settings.gpu_priority.foreground_priority = priority;
                }
            },
            window,
            cx,
        )
    }
}
