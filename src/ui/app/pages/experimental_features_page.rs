use crate::ui::app::*;

impl WinderustApp {
    pub(in crate::ui::app) fn render_experimental_features_page(
        &self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.page_shell(Page::ExperimentalFeatures, cx)
            .child(setting_action_card_with_help(
                "experimental-priority-values",
                t!("settings.expose_all_priority_values").to_string(),
                t!("settings.expose_all_priority_values_help").to_string(),
                setting_group_switch_action(
                    "experimental-priority-values-toggle",
                    self.settings.advanced.expose_all_priority_values,
                    cx.listener(|app, checked, _, cx| {
                        app.settings.advanced.expose_all_priority_values = *checked;
                        if !*checked {
                            app.settings.process_priority.background_priority = app
                                .settings
                                .process_priority
                                .background_priority
                                .safe_when_advanced_disabled();
                            app.settings.process_priority.foreground_priority = app
                                .settings
                                .process_priority
                                .foreground_priority
                                .safe_when_advanced_disabled();
                            app.settings.thread_priority.background_priority = app
                                .settings
                                .thread_priority
                                .background_priority
                                .safe_when_advanced_disabled();
                            app.settings.thread_priority.foreground_priority = app
                                .settings
                                .thread_priority
                                .foreground_priority
                                .safe_when_advanced_disabled();
                            app.settings.io_priority.background_priority = app
                                .settings
                                .io_priority
                                .background_priority
                                .safe_when_advanced_disabled();
                            app.settings.io_priority.foreground_priority = app
                                .settings
                                .io_priority
                                .foreground_priority
                                .safe_when_advanced_disabled();
                            app.settings.gpu_priority.background_priority = app
                                .settings
                                .gpu_priority
                                .background_priority
                                .safe_when_advanced_disabled();
                            app.settings.gpu_priority.foreground_priority = app
                                .settings
                                .gpu_priority
                                .foreground_priority
                                .safe_when_advanced_disabled();
                            app.settings
                                .workload_engine
                                .workload_engine_io_priority
                                .background_priority = app
                                .settings
                                .workload_engine
                                .workload_engine_io_priority
                                .background_priority
                                .safe_when_advanced_disabled();
                            app.settings
                                .workload_engine
                                .workload_engine_io_priority
                                .foreground_priority = app
                                .settings
                                .workload_engine
                                .workload_engine_io_priority
                                .foreground_priority
                                .safe_when_advanced_disabled();
                            app.settings
                                .workload_engine
                                .workload_engine_thread_priority
                                .background_priority = app
                                .settings
                                .workload_engine
                                .workload_engine_thread_priority
                                .background_priority
                                .safe_when_advanced_disabled();
                            app.settings
                                .workload_engine
                                .workload_engine_thread_priority
                                .foreground_priority = app
                                .settings
                                .workload_engine
                                .workload_engine_thread_priority
                                .foreground_priority
                                .safe_when_advanced_disabled();
                            app.settings
                                .workload_engine
                                .workload_engine_gpu_priority
                                .background_priority = app
                                .settings
                                .workload_engine
                                .workload_engine_gpu_priority
                                .background_priority
                                .safe_when_advanced_disabled();
                            app.settings
                                .workload_engine
                                .workload_engine_gpu_priority
                                .foreground_priority = app
                                .settings
                                .workload_engine
                                .workload_engine_gpu_priority
                                .foreground_priority
                                .safe_when_advanced_disabled();
                            for rule in &mut app.settings.process_priority.exclusions {
                                let foreground = rule
                                    .process_priority_override(true)
                                    .safe_when_advanced_disabled();
                                let background = rule
                                    .process_priority_override(false)
                                    .safe_when_advanced_disabled();
                                rule.set_process_priority_override(true, foreground);
                                rule.set_process_priority_override(false, background);
                            }
                            for rule in &mut app.settings.thread_priority.exclusions {
                                let foreground = rule
                                    .thread_priority_override(true)
                                    .safe_when_advanced_disabled();
                                let background = rule
                                    .thread_priority_override(false)
                                    .safe_when_advanced_disabled();
                                rule.set_thread_priority_override(true, foreground);
                                rule.set_thread_priority_override(false, background);
                            }
                            for rule in &mut app.settings.io_priority.exclusions {
                                let foreground = rule
                                    .io_priority_override(true)
                                    .safe_when_advanced_disabled();
                                let background = rule
                                    .io_priority_override(false)
                                    .safe_when_advanced_disabled();
                                rule.set_io_priority_override(true, foreground);
                                rule.set_io_priority_override(false, background);
                            }
                            for rule in &mut app.settings.gpu_priority.exclusions {
                                let foreground = rule
                                    .gpu_priority_override(true)
                                    .safe_when_advanced_disabled();
                                let background = rule
                                    .gpu_priority_override(false)
                                    .safe_when_advanced_disabled();
                                rule.set_gpu_priority_override(true, foreground);
                                rule.set_gpu_priority_override(false, background);
                            }
                        }
                        cx.notify();
                    }),
                ),
            ))
            .child(setting_action_card_with_help(
                "experimental-advanced-controls",
                t!("settings.show_advanced_controls").to_string(),
                t!("settings.show_advanced_controls_help").to_string(),
                setting_group_switch_action(
                    "experimental-advanced-controls-toggle",
                    self.settings.advanced.show_advanced_controls,
                    cx.listener(|app, checked, _, cx| {
                        app.settings.advanced.show_advanced_controls = *checked;
                        if !*checked && app.page.section_landing_page() == Page::AdvancedControls {
                            app.page = Page::ExperimentalFeatures;
                        }
                        cx.notify();
                    }),
                ),
            ))
            .into_any_element()
    }
}
