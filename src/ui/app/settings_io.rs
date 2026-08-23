use crate::ui::app::*;

impl WinderustApp {
    pub(in crate::ui::app) fn has_pending_changes(&self) -> bool {
        self.settings.has_unsaved_changes() || self.processor_power_dirty
    }

    pub(in crate::ui::app) fn save_settings(&mut self) -> bool {
        match self.settings.save() {
            Ok(outcome) => {
                self.sync_runtime_settings();
                self.status_message = match outcome.startup_registration_error() {
                    None => t!(
                        "status.saved_settings",
                        path = config::storage::config_path().display()
                    )
                    .to_string(),
                    Some(error) => {
                        t!("status.saved_settings_with_error", error = error).to_string()
                    }
                };
                true
            }
            Err(err) => {
                self.status_message = err.to_string();
                false
            }
        }
    }

    pub(in crate::ui::app) fn save_pending_changes(&mut self) -> bool {
        if self.settings.has_unsaved_changes() && !self.save_settings() {
            return false;
        }

        !self.processor_power_dirty || self.save_processor_power_tuning()
    }

    pub(in crate::ui::app) fn discard_pending_changes(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let had_unsaved_changes = self.has_pending_changes();
        let had_processor_power_changes = self.processor_power_dirty;
        self.settings.cancel();
        apply_language(self.settings.general.language);
        apply_appearance_settings(&self.settings.general, window, cx);
        self.editing_rule_title = None;
        self.expanded_rule_cards.clear();
        self.rebuild_inputs(window, cx);
        self.sync_runtime_settings();
        let tuning_discarded =
            !had_processor_power_changes || self.sync_processor_power_values_from_target_plan(true);
        if tuning_discarded {
            self.status_message = t!("status.unsaved_canceled").to_string();
        }
        if had_unsaved_changes && !self.has_pending_changes() {
            self.start_unsaved_popup_vanish();
        }
    }

    pub(in crate::ui::app) fn export_settings_toml(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let hwnd = self.hwnd;
        cx.spawn_in(window, async move |this, cx| {
            let path = choose_settings_file(hwnd, FileDialogMode::Save).await;
            let _ = cx.update(move |_window, app_cx| {
                let Some(this) = this.upgrade() else {
                    return;
                };
                this.update(app_cx, |app, cx| app.finish_export_settings(path, cx));
            });
        })
        .detach();
    }

    fn finish_export_settings(&mut self, path: Option<PathBuf>, cx: &mut Context<Self>) {
        match path {
            Some(path) => match self.settings.export_toml_to(&path) {
                Ok(()) => {
                    self.status_message =
                        t!("status.exported_settings", path = path.display()).to_string();
                    self.show_settings_io_toast(
                        t!("settings_io_toast.exported").to_string(),
                        true,
                        cx,
                    );
                }
                Err(err) => {
                    self.status_message = err.to_string();
                    self.show_settings_io_toast(
                        t!("settings_io_toast.export_failed").to_string(),
                        false,
                        cx,
                    );
                }
            },
            None => {
                self.status_message = t!("status.export_canceled").to_string();
            }
        }
    }

    pub(in crate::ui::app) fn export_action_log_csv(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.action_log_entries.is_empty() {
            self.status_message = t!("status.action_log_export_empty").to_string();
            return;
        }

        let hwnd = self.hwnd;
        cx.spawn_in(window, async move |this, cx| {
            let path = choose_action_log_export_file(hwnd).await;
            let _ = cx.update(move |_window, app_cx| {
                let Some(this) = this.upgrade() else {
                    return;
                };
                this.update(app_cx, |app, _cx| app.finish_export_action_log(path));
            });
        })
        .detach();
    }

    fn finish_export_action_log(&mut self, path: Option<PathBuf>) {
        match path {
            Some(path) => {
                let csv = action_log_entries_to_csv(self.action_log_entries.as_slice());
                match config::storage::write_bytes_atomically(&path, csv.as_bytes()) {
                    Ok(()) => {
                        self.status_message =
                            t!("status.exported_action_log", path = path.display()).to_string();
                    }
                    Err(err) => {
                        self.status_message = t!(
                            "status.action_log_export_failed",
                            path = path.display(),
                            error = err
                        )
                        .to_string();
                    }
                }
            }
            None => {
                self.status_message = t!("status.action_log_export_canceled").to_string();
            }
        }
    }

    pub(in crate::ui::app) fn import_settings_toml(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let hwnd = self.hwnd;
        cx.spawn_in(window, async move |this, cx| {
            let path = choose_settings_file(hwnd, FileDialogMode::Open).await;
            let _ = cx.update(move |window, app_cx| {
                let Some(this) = this.upgrade() else {
                    return;
                };
                this.update(app_cx, |app, cx| {
                    app.finish_import_settings(path, window, cx)
                });
            });
        })
        .detach();
    }

    fn finish_import_settings(
        &mut self,
        path: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match path {
            Some(path) => match self.settings.import_toml_from(&path) {
                Ok(outcome) => {
                    apply_language(self.settings.general.language);
                    apply_appearance_settings(&self.settings.general, window, cx);
                    self.status_message = match outcome.startup_registration_error() {
                        None => t!("status.imported_settings", path = path.display()).to_string(),
                        Some(error) => {
                            t!("status.imported_settings_with_error", error = error).to_string()
                        }
                    };
                    let (title, success) = if outcome.startup_registration_error().is_none() {
                        (t!("settings_io_toast.imported").to_string(), true)
                    } else {
                        (t!("settings_io_toast.import_failed").to_string(), false)
                    };
                    self.show_settings_io_toast(title, success, cx);
                    self.rebuild_inputs(window, cx);
                    self.sync_runtime_settings();
                }
                Err(err) => {
                    self.status_message = err.to_string();
                    self.show_settings_io_toast(
                        t!("settings_io_toast.import_failed").to_string(),
                        false,
                        cx,
                    );
                }
            },
            None => {
                self.status_message = t!("status.import_canceled").to_string();
            }
        }
    }

    fn show_settings_io_toast(&mut self, title: String, success: bool, cx: &mut Context<Self>) {
        let shown_at = Instant::now();
        self.settings_io_toast = Some(SettingsIoToast {
            title,
            message: self.status_message.clone(),
            success,
            shown_at,
            closing: false,
        });
        cx.spawn(async move |this, cx| {
            Timer::after(Duration::from_secs(3)).await;
            let _ = this.update(cx, |app, cx| {
                if let Some(toast) = app
                    .settings_io_toast
                    .as_mut()
                    .filter(|toast| toast.shown_at == shown_at)
                {
                    toast.closing = true;
                    cx.notify();
                }
            });
            Timer::after(Duration::from_secs_f64(MOTION_STANDARD_SECONDS)).await;
            let _ = this.update(cx, |app, cx| {
                if app.settings_io_toast.as_ref().map(|toast| toast.shown_at) == Some(shown_at) {
                    app.settings_io_toast = None;
                    cx.notify();
                }
            });
        })
        .detach();
    }

    pub(in crate::ui::app) fn render_settings_io_toast(
        &self,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(toast) = self.settings_io_toast.as_ref() else {
            return div().into_any_element();
        };
        let color = if toast.success {
            rgb(success_text_color()).into()
        } else {
            cx.theme().danger_foreground
        };
        let card = v_flex()
            .absolute()
            .right(px(24.0))
            .top(px(64.0))
            .w(px(372.0))
            .occlude()
            .gap_2()
            .p_3()
            .rounded(px(BRAND_RADIUS_OVERLAY))
            .border_1()
            .border_color(color)
            .bg(cx.theme().popover)
            .child(
                div()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(color)
                    .child(toast.title.clone()),
            )
            .child(text_muted(toast.message.clone()));

        if toast.closing {
            with_optional_motion(
                card,
                "settings-io-toast-exit",
                MotionSpeed::Standard,
                |card| card.opacity(0.0),
                |card, delta| card.opacity(1.0 - delta),
            )
        } else {
            with_optional_motion(
                card,
                "settings-io-toast-enter",
                MotionSpeed::Standard,
                |card| card,
                |card, delta| card.opacity(0.18 + 0.82 * delta),
            )
        }
    }

    pub(in crate::ui::app) fn page_uses_process_candidates(&self) -> bool {
        matches!(
            self.shell.page,
            Page::ByForeground
                | Page::BackgroundEfficiency
                | Page::AppSuspension
                | Page::ProcessPriority
                | Page::DynamicPriorityBoost
                | Page::CoreLimiter
                | Page::CpuSetsSoft
                | Page::IoPriority
                | Page::GpuPriority
                | Page::MemoryPriority
                | Page::TimerResolution
                | Page::ByRunningApp
                | Page::ProcessorAffinityHard
        )
    }

    pub(in crate::ui::app) fn start_unsaved_popup_vanish(&mut self) {
        self.unsaved_popup_was_visible = false;
        self.unsaved_popup_vanish_started = ui_animations_enabled().then_some(Instant::now());
    }

    pub(in crate::ui::app) fn unsaved_popup_vanish_progress(
        &mut self,
        unsaved: bool,
        window: &mut Window,
    ) -> Option<f32> {
        if unsaved {
            self.unsaved_popup_was_visible = true;
            self.unsaved_popup_vanish_started = None;
            return None;
        }

        if !ui_animations_enabled() {
            self.unsaved_popup_was_visible = false;
            self.unsaved_popup_vanish_started = None;
            return None;
        }

        if self.unsaved_popup_vanish_started.is_none() {
            if self.unsaved_popup_was_visible {
                self.start_unsaved_popup_vanish();
            } else {
                return None;
            }
        } else {
            self.unsaved_popup_was_visible = false;
        }

        let progress = popup_vanish_progress(&mut self.unsaved_popup_vanish_started, window);
        if progress.is_none() {
            self.unsaved_popup_was_visible = false;
        }
        progress
    }

    pub(in crate::ui::app) fn current_runtime_settings(&mut self) -> Arc<Settings> {
        let snapshot = self.settings.runtime_settings_snapshot();
        self.runtime_handle.replace_settings(&snapshot);
        snapshot.value
    }

    pub(in crate::ui::app) fn sync_runtime_settings(&mut self) {
        let _ = self.current_runtime_settings();
    }
}
