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
        page_header_with_help(
            page,
            self.page_header_help(page),
            self.active_breadcrumb_transition(page),
            cx,
        )
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
