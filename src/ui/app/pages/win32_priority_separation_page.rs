use crate::ui::app::*;

impl WinderustApp {
    pub(in crate::ui::app) fn render_win32_priority_separation_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.page_shell(Page::Win32PrioritySeparation, cx)
            .child(self.render_win32_priority_separation_card(window, cx))
            .into_any_element()
    }

    pub(in crate::ui::app) fn render_win32_priority_separation_card(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let edit_value = self.win32_priority_separation_edit_value;
        let has_backup = self.win32_priority_separation_backup.is_some();

        v_flex()
            .w_full()
            .min_w(px(0.0))
            .gap_2()
            .child(text_muted(
                t!("settings.win32_priority_separation_warning").to_string(),
            ))
            .child(self.render_win32_priority_separation_target_card())
            .child(
                v_flex()
                    .w_full()
                    .min_w(px(0.0))
                    .gap_1()
                    .child(processor_power_column_header(
                        t!("settings.win32_priority_separation_tuning").to_string(),
                    ))
                    .child(win32_priority_row(
                        "win32-priority-separation-duration-row",
                        t!("settings.win32_priority_separation_quantum_duration").to_string(),
                        Some(
                            t!("settings.win32_priority_separation_quantum_duration_help")
                                .to_string(),
                        ),
                        self.render_win32_priority_separation_field_picker(
                            Win32PrioritySeparationField::QuantumDuration,
                            window,
                            cx,
                        ),
                    ))
                    .child(win32_priority_row(
                        "win32-priority-separation-behaviour-row",
                        t!("settings.win32_priority_separation_quantum_behaviour").to_string(),
                        Some(
                            t!("settings.win32_priority_separation_quantum_behaviour_help")
                                .to_string(),
                        ),
                        self.render_win32_priority_separation_field_picker(
                            Win32PrioritySeparationField::QuantumBehaviour,
                            window,
                            cx,
                        ),
                    ))
                    .child(win32_priority_row(
                        "win32-priority-separation-boost-row",
                        t!("settings.win32_priority_separation_foreground_boost").to_string(),
                        Some(
                            t!("settings.win32_priority_separation_foreground_boost_help")
                                .to_string(),
                        ),
                        self.render_win32_priority_separation_field_picker(
                            Win32PrioritySeparationField::ForegroundBoost,
                            window,
                            cx,
                        ),
                    ))
                    .child(win32_priority_row(
                        "win32-priority-separation-result-row",
                        t!("settings.win32_priority_separation_resulting_value").to_string(),
                        Some(win32_priority_separation_description(edit_value)),
                        value_pill(format_win32_priority_separation(edit_value)).into_any_element(),
                    )),
            )
            .child(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .flex_wrap()
                    .child(
                        control_button(Button::new("refresh-win32-priority-separation"))
                            .label(t!("settings.refresh").to_string())
                            .on_click(cx.listener(|app, _, _, cx| {
                                app.refresh_win32_priority_separation();
                                cx.notify();
                            })),
                    )
                    .child(
                        control_button(Button::new("save-win32-priority-separation-backup"))
                            .label(t!("settings.save_backup").to_string())
                            .on_click(cx.listener(|app, _, _, cx| {
                                app.save_win32_priority_separation_backup();
                                cx.notify();
                            })),
                    )
                    .child(
                        control_button(Button::new("restore-win32-priority-separation-backup"))
                            .label(t!("settings.restore_backup").to_string())
                            .disabled(!has_backup)
                            .on_click(cx.listener(|app, _, _, cx| {
                                app.restore_win32_priority_separation_backup();
                                cx.notify();
                            })),
                    )
                    .child(
                        primary_control_button(Button::new("apply-win32-priority-separation"), cx)
                            .label(t!("settings.apply").to_string())
                            .on_click(cx.listener(|app, _, _, cx| {
                                app.apply_win32_priority_separation(
                                    app.win32_priority_separation_edit_value,
                                );
                                cx.notify();
                            })),
                    ),
            )
            .child(text_muted(self.win32_priority_separation_status.clone()))
            .into_any_element()
    }

    pub(in crate::ui::app) fn render_win32_priority_separation_target_card(&self) -> AnyElement {
        let current_value = self
            .win32_priority_separation_value
            .map(format_win32_priority_separation_with_description)
            .unwrap_or_else(|| t!("settings.win32_priority_separation_unavailable").to_string());
        let backup_value = self
            .win32_priority_separation_backup
            .map(format_win32_priority_separation_with_description)
            .unwrap_or_else(|| t!("settings.win32_priority_separation_no_backup").to_string());

        v_flex()
            .id("win32-priority-separation-target-card")
            .w_full()
            .min_w(px(0.0))
            .overflow_hidden()
            .rounded(px(BRAND_RADIUS_CONTROL))
            .bg(rgb(settings_card_color()))
            .text_color(rgb(primary_text_color()))
            .text_size(px(TEXT_BODY_SIZE))
            .line_height(px(TEXT_BODY_LINE_HEIGHT))
            .child(win32_priority_registry_value_row(
                "win32-priority-separation-current-row",
                t!("settings.win32_priority_separation_current").to_string(),
                Some(t!("settings.win32_priority_separation_warning").to_string()),
                current_value,
                false,
            ))
            .child(win32_priority_registry_value_row(
                "win32-priority-separation-backup-row",
                t!("settings.win32_priority_separation_backup").to_string(),
                None,
                backup_value,
                true,
            ))
            .into_any_element()
    }

    pub(in crate::ui::app) fn render_win32_priority_separation_field_picker(
        &self,
        field: Win32PrioritySeparationField,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let picker_id = win32_priority_separation_field_picker_id(field);
        let options = win32_priority_separation_field_options(field);
        let is_open = self.active_power_plan_picker.as_deref() == Some(picker_id);
        let placement =
            self.dropdown_placement(picker_id, dropdown_list_height(options.len()), window);
        let mut options = dropdown_surface(cx, placement.max_height);
        for option in win32_priority_separation_field_options(field) {
            let selected = win32_priority_separation_field_bits(
                self.win32_priority_separation_edit_value,
                field,
            ) == option.bits;
            options = options.child(
                dropdown_option_row(
                    SharedString::from(format!("{}-option-{:02x}", picker_id, option.bits)),
                    win32_priority_separation_field_option_label(field, option.bits),
                    selected,
                    cx,
                )
                .on_click(cx.listener(move |app, _: &gpui::ClickEvent, _, cx| {
                    app.set_win32_priority_separation_field(field, option.bits);
                    app.active_power_plan_picker = None;
                    cx.notify();
                })),
            );
        }

        let current_label =
            win32_priority_separation_field_label(field, self.win32_priority_separation_edit_value);
        let phase = dropdown_popup_phase(picker_id, is_open, cx);
        dropdown_select_container(DropdownSelectWidth::Standard)
            .child(
                dropdown_select_control(
                    SharedString::from(format!("{picker_id}-control")),
                    current_label,
                    true,
                    is_open,
                    phase,
                    cx,
                )
                .on_click(cx.listener(move |app, _: &gpui::ClickEvent, _, cx| {
                    app.active_power_plan_picker = (app.active_power_plan_picker.as_deref()
                        != Some(picker_id))
                    .then_some(picker_id.to_owned());
                    cx.notify();
                })),
            )
            .child(dropdown_anchor_sensor(
                picker_id,
                Rc::clone(&self.dropdown_anchor_bounds),
            ))
            .child(dropdown_popup_or_empty(
                SharedString::from(picker_id),
                phase,
                placement,
                options,
                cx,
            ))
            .into_any_element()
    }
    pub(in crate::ui::app) fn refresh_win32_priority_separation(&mut self) {
        let (value, status) = read_win32_priority_separation_with_status();
        self.win32_priority_separation_value = value;
        if let Some(value) = value {
            self.win32_priority_separation_edit_value =
                normalize_win32_priority_separation_value(value);
        }
        self.win32_priority_separation_backup = read_win32_priority_separation_backup();
        self.win32_priority_separation_status = status.clone();
        self.status_message = status;
    }

    pub(in crate::ui::app) fn set_win32_priority_separation_field(
        &mut self,
        field: Win32PrioritySeparationField,
        bits: u32,
    ) {
        let value =
            normalize_win32_priority_separation_value(self.win32_priority_separation_edit_value);
        self.win32_priority_separation_edit_value = match field {
            Win32PrioritySeparationField::QuantumDuration => (value & !0x30) | bits,
            Win32PrioritySeparationField::QuantumBehaviour => (value & !0x0C) | bits,
            Win32PrioritySeparationField::ForegroundBoost => (value & !0x03) | bits,
        };
    }

    pub(in crate::ui::app) fn save_win32_priority_separation_backup(&mut self) {
        let Some(value) = read_win32_priority_separation() else {
            self.win32_priority_separation_status =
                t!("settings.win32_priority_separation_load_failed").to_string();
            self.status_message = self.win32_priority_separation_status.clone();
            return;
        };

        match write_win32_priority_separation_backup(value) {
            Ok(()) => {
                self.win32_priority_separation_backup = Some(value);
                self.win32_priority_separation_status = t!(
                    "settings.win32_priority_separation_backup_saved",
                    value = format_win32_priority_separation_with_description(value)
                )
                .to_string();
            }
            Err(err) => {
                self.win32_priority_separation_status = t!(
                    "settings.win32_priority_separation_backup_failed",
                    error = err
                )
                .to_string();
            }
        }
        self.status_message = self.win32_priority_separation_status.clone();
    }

    pub(in crate::ui::app) fn apply_win32_priority_separation(&mut self, value: u32) {
        let value = value.clamp(
            WIN32_PRIORITY_SEPARATION_MIN as u32,
            WIN32_PRIORITY_SEPARATION_MAX as u32,
        );
        if let Err(err) = self.ensure_win32_priority_separation_backup() {
            self.win32_priority_separation_status = t!(
                "settings.win32_priority_separation_backup_failed",
                error = err
            )
            .to_string();
            self.status_message = self.win32_priority_separation_status.clone();
            return;
        }
        match write_win32_priority_separation(value) {
            Ok(()) => {
                self.win32_priority_separation_value = Some(value);
                self.win32_priority_separation_edit_value = value;
                self.win32_priority_separation_status = t!(
                    "settings.win32_priority_separation_saved",
                    value = format_win32_priority_separation_with_description(value)
                )
                .to_string();
            }
            Err(err) => {
                self.win32_priority_separation_status = t!(
                    "settings.win32_priority_separation_save_failed",
                    error = err
                )
                .to_string();
            }
        }
        self.status_message = self.win32_priority_separation_status.clone();
    }

    pub(in crate::ui::app) fn restore_win32_priority_separation_backup(&mut self) {
        let Some(value) = self.win32_priority_separation_backup else {
            self.win32_priority_separation_status =
                t!("settings.win32_priority_separation_no_backup").to_string();
            self.status_message = self.win32_priority_separation_status.clone();
            return;
        };

        match write_win32_priority_separation(value) {
            Ok(()) => {
                self.win32_priority_separation_value = Some(value);
                self.win32_priority_separation_edit_value = value;
                self.win32_priority_separation_status = t!(
                    "settings.win32_priority_separation_restored",
                    value = format_win32_priority_separation_with_description(value)
                )
                .to_string();
            }
            Err(err) => {
                self.win32_priority_separation_status = t!(
                    "settings.win32_priority_separation_restore_failed",
                    error = err
                )
                .to_string();
            }
        }
        self.status_message = self.win32_priority_separation_status.clone();
    }

    pub(in crate::ui::app) fn ensure_win32_priority_separation_backup(
        &mut self,
    ) -> Result<(), String> {
        if self.win32_priority_separation_backup.is_some() {
            return Ok(());
        }
        let current = read_win32_priority_separation()
            .ok_or_else(|| t!("settings.win32_priority_separation_load_failed").to_string())?;
        write_win32_priority_separation_backup(current)?;
        self.win32_priority_separation_backup = Some(current);
        Ok(())
    }
}
