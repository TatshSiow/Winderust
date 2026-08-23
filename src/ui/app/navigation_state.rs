use crate::ui::app::*;

impl WinderustApp {
    pub(in crate::ui::app) fn navigate_to(&mut self, page: Page, cx: &mut Context<Self>) {
        if !self
            .shell
            .navigate_to(page, ui_animations_enabled(), Instant::now())
        {
            return;
        }

        clear_page_hovered();
        self.sync_power_source_editor();
        self.process_list.details = None;
        self.schedule_process_refresh_for_current_page();
        cx.notify();
    }

    pub(in crate::ui::app) fn navigate_back(&mut self, cx: &mut Context<Self>) {
        if !self
            .shell
            .navigate_back(ui_animations_enabled(), Instant::now())
        {
            return;
        }

        clear_page_hovered();
        self.sync_power_source_editor();
        self.process_list.details = None;
        self.schedule_process_refresh_for_current_page();
        cx.notify();
    }

    pub(in crate::ui::app) fn navigate_forward(&mut self, cx: &mut Context<Self>) {
        if !self
            .shell
            .navigate_forward(ui_animations_enabled(), Instant::now())
        {
            return;
        }

        clear_page_hovered();
        self.sync_power_source_editor();
        self.process_list.details = None;
        self.schedule_process_refresh_for_current_page();
        cx.notify();
    }

    fn schedule_process_refresh_for_current_page(&mut self) {
        if self.shell.page == Page::ProcessList
            || (self.page_uses_process_candidates() && self.process_catalog.candidates.is_empty())
        {
            self.next_process_refresh = Instant::now();
        }
    }

    pub(in crate::ui::app) fn clear_finished_breadcrumb_transition(&mut self) {
        self.shell
            .clear_finished_breadcrumb_transition(ui_animations_enabled(), Instant::now());
    }

    pub(in crate::ui::app) fn active_breadcrumb_transition(
        &self,
        page: Page,
    ) -> Option<&BreadcrumbTransition> {
        self.shell.active_breadcrumb_transition(page)
    }

    pub(in crate::ui::app) fn page_header(&self, page: Page, cx: &mut Context<Self>) -> gpui::Div {
        let header = page_header_with_help(
            page,
            self.page_header_help(page),
            self.active_breadcrumb_transition(page),
            cx,
        );
        if page.supports_power_source_profiles() {
            header.child(self.power_source_profile_selector(cx))
        } else {
            header
        }
    }

    fn sync_power_source_editor(&mut self) {
        let profile = if self.shell.page.supports_power_source_profiles() {
            self.editing_power_source_profile
        } else {
            PowerSourceProfile::PluggedIn
        };
        self.settings.select_power_source(profile);
    }

    fn power_source_profile_selector(&self, cx: &mut Context<Self>) -> gpui::Div {
        let selected_background = cx.theme().secondary_active;
        let hover_background = cx.theme().secondary_hover;
        let active_profile = if crate::backend::power_source::is_plugged_in() == Some(false) {
            PowerSourceProfile::OnBattery
        } else {
            PowerSourceProfile::PluggedIn
        };
        let mut tabs = h_flex()
            .flex_shrink_0()
            .gap_1()
            .p_1()
            .rounded(px(BRAND_RADIUS_SURFACE))
            .bg(rgb(settings_card_color()));

        for (profile, label) in [
            (PowerSourceProfile::PluggedIn, t!("power_source.plugged_in")),
            (PowerSourceProfile::OnBattery, t!("power_source.on_battery")),
        ] {
            let selected = self.editing_power_source_profile == profile;
            let active = active_profile == profile;
            tabs = tabs.child(
                div()
                    .id(SharedString::from(format!("power-source-{profile:?}-tab")))
                    .h(px(30.0))
                    .px_3()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(BRAND_RADIUS_CONTROL))
                    .text_size(px(TEXT_CONTROL_SIZE))
                    .cursor_pointer()
                    .when(selected, |item| item.bg(selected_background))
                    .hover(move |style| style.bg(hover_background))
                    .on_click(cx.listener(move |app, _, window, cx| {
                        app.sync_input_values(cx);
                        app.editing_power_source_profile = profile;
                        app.settings.select_power_source(profile);
                        app.load_power_source_input_values(window, cx);
                        app.active_power_plan_picker = None;
                        cx.notify();
                    }))
                    .child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            .child(label.to_string())
                            .when(active, |item| {
                                item.child(
                                    div()
                                        .size(px(6.0))
                                        .rounded_full()
                                        .bg(rgb(success_text_color())),
                                )
                            }),
                    ),
            );
        }

        tabs
    }

    pub(in crate::ui::app) fn page_header_help(&self, page: Page) -> Option<SharedString> {
        match page {
            Page::ActionLog => Some(action_log_page_help()),
            _ => None,
        }
    }

    pub(in crate::ui::app) fn page_shell(&self, _page: Page, _cx: &mut Context<Self>) -> gpui::Div {
        page_body_shell()
    }
}
