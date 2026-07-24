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
}
