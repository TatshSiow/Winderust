use crate::ui::app::*;

impl WinderustApp {
    pub(in crate::ui::app) fn sync_tray_icon(&mut self) -> bool {
        let tray_required =
            self.settings.general.hide_to_tray || self.saved_settings.general.start_minimized;
        let tray_present = self.tray_icon.is_some();
        let mut changed = false;

        if tray_required {
            if self.tray_icon.is_none() {
                let Some(hwnd) = self.hwnd else {
                    self.set_tray_hide_on_close(false);
                    let message = t!("status.system_tray_unavailable").to_string();
                    if self.status_message != message {
                        self.status_message = message;
                        changed = true;
                    }
                    return changed;
                };

                match TrayIcon::install(hwnd) {
                    Ok(icon) => {
                        self.tray_icon = Some(icon);
                        changed = true;
                        let message = t!("status.system_tray_enabled").to_string();
                        if self.status_message != message {
                            self.status_message = message;
                            changed = true;
                        }
                    }
                    Err(err) => {
                        if self.status_message != err {
                            self.status_message = err;
                            changed = true;
                        }
                    }
                }
            }
            self.set_tray_hide_on_close(
                self.settings.general.hide_to_tray && self.tray_icon.is_some(),
            );
        } else if self.tray_icon.take().is_some() {
            self.set_tray_hide_on_close(false);
            changed = true;
            let message = t!("status.system_tray_disabled").to_string();
            if self.status_message != message {
                self.status_message = message;
                changed = true;
            }
        } else {
            self.set_tray_hide_on_close(false);
        }

        changed || tray_present != self.tray_icon.is_some()
    }

    pub(in crate::ui::app) fn set_tray_hide_on_close(&mut self, enabled: bool) {
        if self.tray_hide_on_close == enabled {
            return;
        }

        self.tray_hide_on_close = enabled;
        tray::set_hide_on_close(enabled);
    }

    pub(in crate::ui::app) fn apply_start_minimized(&mut self, window: &mut Window) -> bool {
        if self.start_minimized_applied {
            return false;
        }
        self.start_minimized_applied = true;

        if !self.saved_settings.general.start_minimized {
            return false;
        }

        if self.tray_icon.is_some() {
            if let Some(hwnd) = self.hwnd {
                tray::hide_window(hwnd);
                self.status_message = t!("status.started_in_tray").to_string();
                return true;
            }
        }

        window.minimize_window();
        self.status_message = t!("status.started_minimized").to_string();
        true
    }

    pub(in crate::ui::app) fn refresh_after_tray_restore(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.next_check = Instant::now();
        match self.tick(window, cx) {
            TickOutcome::Continue { changed } => {
                self.schedule_tick(window, cx);
                if changed {
                    cx.notify();
                }
            }
            TickOutcome::Stop => {}
        }
    }
}
