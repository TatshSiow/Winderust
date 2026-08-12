use crate::ui::app::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::ui::app) enum UpdateModalDismissal {
    NoChange,
    Hidden,
    Closing,
}

pub(in crate::ui::app) struct UpdateModel {
    pub(in crate::ui::app) available: Option<AvailableUpdate>,
    pub(in crate::ui::app) latest_version: Option<String>,
    pub(in crate::ui::app) check_in_progress: bool,
    pub(in crate::ui::app) check_message: Option<String>,
    pub(in crate::ui::app) startup_modal_visible: bool,
    pub(in crate::ui::app) startup_modal_closing: bool,
}

impl UpdateModel {
    pub(in crate::ui::app) fn new() -> Self {
        Self {
            available: None,
            latest_version: None,
            check_in_progress: false,
            check_message: None,
            startup_modal_visible: false,
            startup_modal_closing: false,
        }
    }

    pub(in crate::ui::app) fn begin_check(&mut self) -> bool {
        if self.check_in_progress {
            return false;
        }
        self.check_in_progress = true;
        self.check_message = None;
        true
    }

    pub(in crate::ui::app) fn finish_check(&mut self) {
        self.check_in_progress = false;
    }

    pub(in crate::ui::app) fn record_success(
        &mut self,
        latest_version: String,
        available: Option<AvailableUpdate>,
        show_startup_modal: bool,
    ) {
        if show_startup_modal && available.is_some() {
            self.startup_modal_visible = true;
            self.startup_modal_closing = false;
        }
        self.latest_version = Some(latest_version);
        self.available = available;
    }

    pub(in crate::ui::app) fn record_failure(&mut self, message: String) {
        self.check_message = Some(message);
    }

    pub(in crate::ui::app) fn clear_results(&mut self) {
        self.latest_version = None;
        self.available = None;
        self.check_message = None;
    }

    pub(in crate::ui::app) fn begin_modal_dismissal(
        &mut self,
        animations_enabled: bool,
    ) -> UpdateModalDismissal {
        if !self.startup_modal_visible || self.startup_modal_closing {
            return UpdateModalDismissal::NoChange;
        }
        if !animations_enabled {
            self.finish_modal_dismissal();
            return UpdateModalDismissal::Hidden;
        }

        self.startup_modal_closing = true;
        UpdateModalDismissal::Closing
    }

    pub(in crate::ui::app) fn finish_modal_dismissal(&mut self) {
        self.startup_modal_visible = false;
        self.startup_modal_closing = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn available_update() -> AvailableUpdate {
        AvailableUpdate {
            url: "https://example.invalid/release".to_owned(),
        }
    }

    #[test]
    fn only_automatic_available_updates_show_the_startup_modal() {
        let mut model = UpdateModel::new();
        model.record_success("0.6.0-alpha".to_owned(), Some(available_update()), false);
        assert!(!model.startup_modal_visible);

        model.record_success("0.6.0-alpha".to_owned(), Some(available_update()), true);
        assert!(model.startup_modal_visible);

        model.finish_modal_dismissal();
        model.record_success("0.6.0-alpha".to_owned(), None, true);
        assert!(!model.startup_modal_visible);
    }

    #[test]
    fn animated_modal_dismissal_has_an_explicit_closing_state() {
        let mut model = UpdateModel::new();
        model.startup_modal_visible = true;

        assert_eq!(
            model.begin_modal_dismissal(true),
            UpdateModalDismissal::Closing
        );
        assert!(model.startup_modal_visible);
        assert!(model.startup_modal_closing);
        assert_eq!(
            model.begin_modal_dismissal(true),
            UpdateModalDismissal::NoChange
        );

        model.finish_modal_dismissal();
        assert!(!model.startup_modal_visible);
        assert!(!model.startup_modal_closing);
    }

    #[test]
    fn a_check_cannot_start_twice() {
        let mut model = UpdateModel::new();
        assert!(model.begin_check());
        assert!(!model.begin_check());
        model.finish_check();
        assert!(model.begin_check());
    }
}
