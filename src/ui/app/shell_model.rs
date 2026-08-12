use crate::ui::app::*;

pub(in crate::ui::app) struct ShellModel {
    pub(in crate::ui::app) page: Page,
    back_stack: Vec<Page>,
    forward_stack: Vec<Page>,
    breadcrumb_transition: Option<BreadcrumbTransition>,
    transition_generation: u64,
}

impl ShellModel {
    pub(in crate::ui::app) fn new(page: Page) -> Self {
        Self {
            page,
            back_stack: Vec::new(),
            forward_stack: Vec::new(),
            breadcrumb_transition: None,
            transition_generation: 0,
        }
    }

    pub(in crate::ui::app) fn navigate_to(
        &mut self,
        page: Page,
        animations_enabled: bool,
        now: Instant,
    ) -> bool {
        if self.page == page {
            return false;
        }

        let previous = self.page;
        push_navigation_page(&mut self.back_stack, previous);
        self.begin_breadcrumb_transition(previous, page, animations_enabled, now);
        self.page = page;
        self.forward_stack.clear();
        true
    }

    pub(in crate::ui::app) fn navigate_back(
        &mut self,
        animations_enabled: bool,
        now: Instant,
    ) -> bool {
        let Some(page) = self.back_stack.pop() else {
            return false;
        };

        let previous = self.page;
        push_navigation_page(&mut self.forward_stack, previous);
        self.begin_breadcrumb_transition(previous, page, animations_enabled, now);
        self.page = page;
        true
    }

    pub(in crate::ui::app) fn navigate_forward(
        &mut self,
        animations_enabled: bool,
        now: Instant,
    ) -> bool {
        let Some(page) = self.forward_stack.pop() else {
            return false;
        };

        let previous = self.page;
        push_navigation_page(&mut self.back_stack, previous);
        self.begin_breadcrumb_transition(previous, page, animations_enabled, now);
        self.page = page;
        true
    }

    pub(in crate::ui::app) fn replace_page(&mut self, page: Page) {
        self.page = page;
    }

    pub(in crate::ui::app) fn clear_finished_breadcrumb_transition(
        &mut self,
        animations_enabled: bool,
        now: Instant,
    ) {
        if !animations_enabled
            || self
                .breadcrumb_transition
                .as_ref()
                .is_some_and(|transition| {
                    now.saturating_duration_since(transition.started)
                        >= Duration::from_secs_f64(MOTION_FAST_SECONDS)
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

    fn begin_breadcrumb_transition(
        &mut self,
        previous: Page,
        current: Page,
        animations_enabled: bool,
        now: Instant,
    ) {
        if previous == current || !animations_enabled {
            self.breadcrumb_transition = None;
            return;
        }

        let previous = breadcrumb_trail(previous);
        let current = breadcrumb_trail(current);
        if previous == current {
            self.breadcrumb_transition = None;
            return;
        }

        self.transition_generation = self.transition_generation.wrapping_add(1);
        self.breadcrumb_transition = Some(BreadcrumbTransition {
            previous,
            current,
            started: now,
            generation: self.transition_generation,
        });
    }
}

fn push_navigation_page(stack: &mut Vec<Page>, page: Page) {
    if stack.last().copied() == Some(page) {
        return;
    }

    stack.push(page);
    if stack.len() > NAV_HISTORY_LIMIT {
        stack.remove(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn navigation_round_trip_preserves_back_and_forward_history() {
        let now = Instant::now();
        let mut shell = ShellModel::new(Page::Home);

        assert!(shell.navigate_to(Page::About, false, now));
        assert_eq!(shell.page, Page::About);
        assert!(shell.navigate_back(false, now));
        assert_eq!(shell.page, Page::Home);
        assert!(shell.navigate_forward(false, now));
        assert_eq!(shell.page, Page::About);
    }

    #[test]
    fn direct_navigation_clears_forward_history() {
        let now = Instant::now();
        let mut shell = ShellModel::new(Page::Home);
        shell.navigate_to(Page::About, false, now);
        shell.navigate_back(false, now);

        shell.navigate_to(Page::ProcessList, false, now);

        assert!(!shell.navigate_forward(false, now));
        assert_eq!(shell.page, Page::ProcessList);
    }

    #[test]
    fn breadcrumb_transition_expires_at_the_motion_deadline() {
        let now = Instant::now();
        let mut shell = ShellModel::new(Page::Home);
        shell.navigate_to(Page::About, true, now);
        assert!(shell.active_breadcrumb_transition(Page::About).is_some());

        shell.clear_finished_breadcrumb_transition(
            true,
            now + Duration::from_secs_f64(MOTION_FAST_SECONDS),
        );
        assert!(shell.active_breadcrumb_transition(Page::About).is_none());
    }
}
