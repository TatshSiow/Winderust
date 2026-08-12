use crate::ui::app::*;

impl WinderustApp {
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

    pub(in crate::ui::app) fn export_settings_toml(&mut self) {
        match choose_settings_file(self.hwnd, FileDialogMode::Save) {
            Some(path) => match self.settings.export_toml_to(&path) {
                Ok(()) => {
                    self.status_message =
                        t!("status.exported_settings", path = path.display()).to_string();
                }
                Err(err) => self.status_message = err.to_string(),
            },
            None => {
                self.status_message = t!("status.export_canceled").to_string();
            }
        }
    }

    pub(in crate::ui::app) fn export_action_log_csv(&mut self) {
        if self.action_log_entries.is_empty() {
            self.status_message = t!("status.action_log_export_empty").to_string();
            return;
        }

        match choose_action_log_export_file(self.hwnd) {
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
        match choose_settings_file(self.hwnd, FileDialogMode::Open) {
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
                    self.rebuild_inputs(window, cx);
                    self.sync_runtime_settings();
                }
                Err(err) => self.status_message = err.to_string(),
            },
            None => {
                self.status_message = t!("status.import_canceled").to_string();
            }
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

    pub(in crate::ui::app) fn cancel_settings_changes(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let had_unsaved_changes = self.settings.has_unsaved_changes();
        self.settings.cancel();
        apply_language(self.settings.general.language);
        apply_appearance_settings(&self.settings.general, window, cx);
        self.status_message = t!("status.unsaved_canceled").to_string();
        self.editing_rule_title = None;
        self.expanded_rule_cards.clear();
        self.rebuild_inputs(window, cx);
        self.sync_runtime_settings();
        if had_unsaved_changes {
            self.start_unsaved_popup_vanish();
        }
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

        let started = self.unsaved_popup_vanish_started?;
        let duration = Duration::from_secs_f64(UNSAVED_POPUP_VANISH_SECONDS);
        let elapsed = started.elapsed();
        if elapsed >= duration {
            self.unsaved_popup_was_visible = false;
            self.unsaved_popup_vanish_started = None;
            None
        } else {
            window.request_animation_frame();
            Some(expandable_motion_ease(
                (elapsed.as_secs_f32() / duration.as_secs_f32().max(f32::EPSILON)).clamp(0.0, 1.0),
                false,
            ))
        }
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
