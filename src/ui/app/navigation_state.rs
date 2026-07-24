use crate::ui::app::*;

impl WinderustApp {
    pub(in crate::ui::app) fn navigate_to(&mut self, page: Page, cx: &mut Context<Self>) {
        if self.page == page {
            return;
        }

        clear_page_hovered();
        Self::push_navigation_page(&mut self.back_stack, self.page);
        self.begin_breadcrumb_transition(self.page, page);
        self.page = page;
        self.forward_stack.clear();
        cx.notify();
    }

    pub(in crate::ui::app) fn navigate_back(&mut self, cx: &mut Context<Self>) {
        let Some(page) = self.back_stack.pop() else {
            return;
        };

        clear_page_hovered();
        Self::push_navigation_page(&mut self.forward_stack, self.page);
        self.begin_breadcrumb_transition(self.page, page);
        self.page = page;
        cx.notify();
    }

    pub(in crate::ui::app) fn navigate_forward(&mut self, cx: &mut Context<Self>) {
        let Some(page) = self.forward_stack.pop() else {
            return;
        };

        clear_page_hovered();
        Self::push_navigation_page(&mut self.back_stack, self.page);
        self.begin_breadcrumb_transition(self.page, page);
        self.page = page;
        cx.notify();
    }

    pub(in crate::ui::app) fn begin_breadcrumb_transition(
        &mut self,
        previous: Page,
        current: Page,
    ) {
        if previous == current || !ui_animations_enabled() {
            self.breadcrumb_transition = None;
            return;
        }

        let previous = breadcrumb_trail(previous);
        let current = breadcrumb_trail(current);
        if previous == current {
            self.breadcrumb_transition = None;
            return;
        }

        self.page_transition_generation = self.page_transition_generation.wrapping_add(1);
        self.breadcrumb_transition = Some(BreadcrumbTransition {
            previous,
            current,
            started: Instant::now(),
            generation: self.page_transition_generation,
        });
    }

    pub(in crate::ui::app) fn clear_finished_breadcrumb_transition(&mut self) {
        if !ui_animations_enabled()
            || self
                .breadcrumb_transition
                .as_ref()
                .is_some_and(|transition| {
                    transition.started.elapsed() >= Duration::from_secs_f64(MOTION_FAST_SECONDS)
                })
        {
            self.breadcrumb_transition = None;
        }
    }

    pub(in crate::ui::app) fn active_breadcrumb_transition(
        &self,
        page: Page,
    ) -> Option<&BreadcrumbTransition> {
        self.breadcrumb_transition
            .as_ref()
            .filter(|transition| transition.current == breadcrumb_trail(page))
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

    pub(in crate::ui::app) fn push_navigation_page(stack: &mut Vec<Page>, page: Page) {
        if stack.last().copied() == Some(page) {
            return;
        }

        stack.push(page);
        if stack.len() > NAV_HISTORY_LIMIT {
            stack.remove(0);
        }
    }
}
