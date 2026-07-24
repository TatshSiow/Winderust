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
}
