use crate::ui::app::*;

impl WinderustApp {
    pub(in crate::ui::app) fn check_for_updates(&mut self, manual: bool, cx: &mut Context<Self>) {
        if self.update_check_in_progress {
            return;
        }
        self.update_check_in_progress = true;
        self.update_check_message = None;
        if manual {
            cx.notify();
        }
        let channel = self.settings.general.update_channel;
        let check = cx
            .background_executor()
            .spawn(async move { update_checker::check(channel) });
        cx.spawn(async move |this, cx| {
            let result = check.await;
            let _ = this.update(cx, |app, cx| {
                app.update_check_in_progress = false;
                if app.settings.general.update_channel != channel {
                    cx.notify();
                    return;
                }
                match result {
                    Ok(check) => {
                        if should_show_startup_update_modal(
                            manual,
                            check.available_update.is_some(),
                        ) {
                            app.startup_update_modal_visible = true;
                            app.startup_update_modal_closing = false;
                        }
                        app.latest_version = Some(check.latest_version);
                        app.available_update = check.available_update;
                    }
                    Err(()) if manual => {
                        app.update_check_message =
                            Some(t!("about.update_check_failed").to_string());
                    }
                    Err(()) => {}
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(in crate::ui::app) fn dismiss_startup_update_modal(&mut self, cx: &mut Context<Self>) {
        if !self.startup_update_modal_visible || self.startup_update_modal_closing {
            return;
        }
        if !ui_animations_enabled() {
            self.startup_update_modal_visible = false;
            cx.notify();
            return;
        }

        self.startup_update_modal_closing = true;
        cx.notify();
        cx.spawn(async move |this, cx| {
            Timer::after(Duration::from_secs_f64(MOTION_FAST_SECONDS)).await;
            let _ = this.update(cx, |app, cx| {
                app.startup_update_modal_visible = false;
                app.startup_update_modal_closing = false;
                cx.notify();
            });
        })
        .detach();
    }
}
fn should_show_startup_update_modal(manual: bool, update_available: bool) -> bool {
    !manual && update_available
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_update_modal_only_follows_automatic_available_updates() {
        assert!(should_show_startup_update_modal(false, true));
        assert!(!should_show_startup_update_modal(true, true));
        assert!(!should_show_startup_update_modal(false, false));
    }
}
