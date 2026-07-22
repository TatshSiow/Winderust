use super::*;

impl WinderustApp {
    pub(super) fn render_timer_resolution_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let input_value = self
            .inputs
            .timer_resolution_process
            .read(cx)
            .value()
            .to_string();
        let enabled = self.settings.timer_resolution.enabled;
        let help = tooltip_lines(vec![
            t!("timer_resolution.intro_1").to_string(),
            t!("timer_resolution.intro_2").to_string(),
            t!("timer_resolution.intro_3").to_string(),
        ]);
        let body = feature_body(enabled)
            .child(section_title_text(t!("common.rules").to_string()))
            .child(
                h_flex()
                    .gap_2()
                    .items_start()
                    .flex_wrap()
                    .child(self.render_process_picker(
                        "timer-resolution-process-suggestion",
                        &self.inputs.timer_resolution_process,
                        SuggestionTarget::TimerResolution,
                        window,
                        cx,
                    ))
                    .child(
                        primary_control_button(Button::new("add-timer-resolution-rule"), cx)
                            .label(t!("common.add").to_string())
                            .disabled(
                                !enabled
                                    || !can_add_timer_resolution_process(
                                        &self.settings.timer_resolution,
                                        &input_value,
                                    ),
                            )
                            .on_click(cx.listener(|app, _, window, cx| {
                                let process = app
                                    .inputs
                                    .timer_resolution_process
                                    .read(cx)
                                    .value()
                                    .to_string();
                                if can_add_timer_resolution_process(
                                    &app.settings.timer_resolution,
                                    &process,
                                ) {
                                    let desired_100ns = app.settings.timer_resolution.desired_100ns;
                                    app.settings
                                        .timer_resolution
                                        .rules
                                        .push(new_timer_resolution_rule(&process, desired_100ns));
                                    clear_input(&app.inputs.timer_resolution_process, window, cx);
                                }
                                cx.notify();
                            })),
                    ),
            )
            .child(self.render_timer_resolution_rules(cx))
            .child(self.render_timer_resolution_status_card());

        self.page_shell(Page::TimerResolution, cx)
            .child(feature_toggle_switch_with_help(
                "timer-resolution-enabled",
                t!("timer_resolution.enable").to_string(),
                help,
                enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings.timer_resolution.enabled = *checked;
                    cx.notify();
                }),
            ))
            .child(text_muted(t!("timer_resolution.warning").to_string()))
            .child(disabled_feature_body(
                "timer-resolution-body",
                body,
                enabled,
                cx,
            ))
            .into_any_element()
    }

    pub(super) fn render_timer_resolution_rules(&self, cx: &mut Context<Self>) -> AnyElement {
        let mut list = rule_list(vec![
            rule_table_active_header(),
            rule_table_title_header(t!("process_list.process_name").to_string()),
            rule_table_centered_header(t!("timer_resolution.requested").to_string(), 104.0),
            rule_table_action_header(),
        ]);
        for (index, rule) in self.settings.timer_resolution.rules.iter().enumerate() {
            let row = compact_rule_row(format!("timer-resolution-rule-row-{index}"))
                .child(rule_active_cell(
                    format!("timer-resolution-rule-enabled-{index}"),
                    rule.enabled,
                    cx.listener(move |app, checked, _, cx| {
                        if let Some(rule) = app.settings.timer_resolution.rules.get_mut(index) {
                            rule.enabled = *checked;
                        }
                        cx.notify();
                    }),
                ))
                .child(self.process_rule_title(&rule.process_name, cx))
                .child(self.render_numeric_value(
                    NumericField::TimerResolutionRule(index),
                    timer_resolution::format_resolution_ms(rule.desired_100ns),
                    timer_resolution_edit_value(rule.desired_100ns),
                    cx,
                ))
                .child(rule_table_action_cell(
                    remove_control_button(Button::new(SharedString::from(format!(
                        "remove-timer-resolution-rule-{index}"
                    ))))
                    .on_click(cx.listener(move |app, _, _, cx| {
                        app.request_list_item_removal(
                            ListItemRemovalTarget::new(
                                ListItemRemovalKind::TimerResolutionRule,
                                index,
                            ),
                            cx,
                        );
                    }))
                    .into_any_element(),
                ));
            list = list.child(self.animated_list_item(
                ListItemRemovalTarget::new(ListItemRemovalKind::TimerResolutionRule, index),
                SharedString::from(format!("timer-resolution-rule-{index}")),
                row.into_any_element(),
            ));
        }
        if self.settings.timer_resolution.rules.is_empty() {
            list = list.child(text_muted(t!("timer_resolution.no_rules").to_string()).p_4());
        }
        list.into_any_element()
    }

    pub(super) fn render_timer_resolution_status_card(&self) -> gpui::Div {
        let status = &self.timer_resolution_status;
        let requested = status
            .requested_100ns
            .map(timer_resolution::format_resolution_ms)
            .unwrap_or_else(|| {
                if self.settings.timer_resolution.enabled {
                    t!("timer_resolution.no_active_request").to_string()
                } else {
                    t!("common.disabled").to_string()
                }
            });
        let active_rule = status.active_rule_process.clone().unwrap_or_else(|| {
            if self.settings.timer_resolution.enabled {
                t!("timer_resolution.no_matching_rule").to_string()
            } else {
                t!("common.disabled").to_string()
            }
        });

        let mut rows = vec![
            (
                t!("timer_resolution.current").to_string(),
                format_optional_timer_resolution(status.current_100ns),
            ),
            (
                t!("timer_resolution.foreground_rule").to_string(),
                active_rule,
            ),
            (t!("timer_resolution.requested").to_string(), requested),
            (
                t!("timer_resolution.minimum").to_string(),
                format_optional_timer_resolution(status.minimum_100ns),
            ),
            (
                t!("timer_resolution.maximum").to_string(),
                format_optional_timer_resolution(status.maximum_100ns),
            ),
            (
                t!("common.status").to_string(),
                localized_runtime_status(&status.message),
            ),
        ];
        if let Some(error) = &status.last_error {
            rows.push((t!("common.last_failure").to_string(), error.clone()));
        }
        stat_grid(rows)
    }

    pub(super) fn render_win32_priority_separation_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.page_shell(Page::Win32PrioritySeparation, cx)
            .child(self.render_win32_priority_separation_card(window, cx))
            .into_any_element()
    }

    pub(super) fn render_win32_priority_separation_card(
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

    pub(super) fn render_win32_priority_separation_target_card(&self) -> AnyElement {
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

    pub(super) fn render_win32_priority_separation_field_picker(
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
    pub(super) fn refresh_win32_priority_separation(&mut self) {
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

    pub(super) fn set_win32_priority_separation_field(
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

    pub(super) fn save_win32_priority_separation_backup(&mut self) {
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

    pub(super) fn apply_win32_priority_separation(&mut self, value: u32) {
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

    pub(super) fn restore_win32_priority_separation_backup(&mut self) {
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

    pub(super) fn ensure_win32_priority_separation_backup(&mut self) -> Result<(), String> {
        if self.win32_priority_separation_backup.is_some() {
            return Ok(());
        }
        let current = read_win32_priority_separation()
            .ok_or_else(|| "Current Win32PrioritySeparation value could not be read.".to_owned())?;
        write_win32_priority_separation_backup(current)?;
        self.win32_priority_separation_backup = Some(current);
        Ok(())
    }
}
