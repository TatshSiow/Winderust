use crate::ui::app::*;

impl WinderustApp {
    pub(in crate::ui::app) fn render_winderust_behaviour_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.page_shell(Page::WinderustBehaviour, cx)
            .child(checkbox(
                "general-enabled",
                t!("settings.master_switch").to_string(),
                self.settings.general.enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings.general.enabled = *checked;
                    cx.notify();
                }),
            ))
            .child(checkbox(
                "startup-windows",
                t!("settings.startup_windows").to_string(),
                self.settings.general.startup_with_windows,
                cx.listener(|app, checked, _, cx| {
                    app.settings.general.startup_with_windows = *checked;
                    cx.notify();
                }),
            ))
            .child(checkbox(
                "start-minimized",
                t!("settings.start_minimized").to_string(),
                self.settings.general.start_minimized,
                cx.listener(|app, checked, _, cx| {
                    app.settings.general.start_minimized = *checked;
                    cx.notify();
                }),
            ))
            .child(checkbox(
                "hide-to-tray",
                t!("settings.hide_to_tray").to_string(),
                self.settings.general.hide_to_tray,
                cx.listener(|app, checked, _, cx| {
                    app.settings.general.hide_to_tray = *checked;
                    cx.notify();
                }),
            ))
            .child(section_title_text(t!("settings.advanced").to_string()))
            .child(self.render_failure_suppression_threshold_setting(cx))
            .child(self.render_action_log_mode_selector(window, cx))
            .child(section_title_text(
                t!("settings.settings_files").to_string(),
            ))
            .child(
                h_flex()
                    .gap_2()
                    .flex_wrap()
                    .child(
                        Button::new("export-settings")
                            .small()
                            .label(t!("settings.export_settings").to_string())
                            .on_click(cx.listener(|app, _, _, cx| {
                                app.export_settings_toml();
                                cx.notify();
                            })),
                    )
                    .child(
                        Button::new("import-settings")
                            .small()
                            .label(t!("settings.import_settings").to_string())
                            .on_click(cx.listener(|app, _, window, cx| {
                                app.import_settings_toml(window, cx);
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }
}
