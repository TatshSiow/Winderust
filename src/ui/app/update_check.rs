use crate::ui::app::*;

impl WinderustApp {
    pub(in crate::ui::app) fn check_for_updates(&mut self, manual: bool, cx: &mut Context<Self>) {
        if !self.update.begin_check() {
            return;
        }
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
                app.update.finish_check();
                if app.settings.general.update_channel != channel {
                    cx.notify();
                    return;
                }
                match result {
                    Ok(check) => {
                        app.update.record_success(
                            check.latest_version,
                            check.available_update,
                            !manual,
                        );
                    }
                    Err(()) if manual => {
                        app.update
                            .record_failure(t!("about.update_check_failed").to_string());
                    }
                    Err(()) => {}
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub(in crate::ui::app) fn dismiss_startup_update_modal(&mut self, cx: &mut Context<Self>) {
        match self.update.begin_modal_dismissal(ui_animations_enabled()) {
            UpdateModalDismissal::NoChange => return,
            UpdateModalDismissal::Hidden => {
                cx.notify();
                return;
            }
            UpdateModalDismissal::Closing => {}
        }

        cx.notify();
        cx.spawn(async move |this, cx| {
            Timer::after(Duration::from_secs_f64(MOTION_FAST_SECONDS)).await;
            let _ = this.update(cx, |app, cx| {
                app.update.finish_modal_dismissal();
                cx.notify();
            });
        })
        .detach();
    }
}
