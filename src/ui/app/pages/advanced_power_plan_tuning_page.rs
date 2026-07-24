use crate::ui::app::*;

impl WinderustApp {
    pub(in crate::ui::app) fn render_advanced_power_plan_tuning_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.page_shell(Page::AdvancedPowerPlanTuning, cx)
            .child(self.render_processor_power_card(window, cx))
            .into_any_element()
    }

    pub(in crate::ui::app) fn render_processor_power_card(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.sync_processor_power_slider_states(window, cx);
        let has_target_plan = self.processor_power_target_plan().is_some();
        let target_plan_notice = self.processor_power_target_plan_notice();
        let processor_power_presets = [
            ProcessorPowerPreset::Performance,
            ProcessorPowerPreset::Balanced,
            ProcessorPowerPreset::Saver,
        ];
        let selected_preset = processor_power_presets
            .iter()
            .copied()
            .find(|preset| self.processor_power_matches_preset(*preset));
        let preset_dropdown = self.render_dropdown_select(
            "processor-power-preset",
            selected_preset
                .map(processor_power_preset_label)
                .unwrap_or_else(|| "Custom".to_owned()),
            true,
            DropdownSelectWidth::Standard,
            processor_power_presets.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for preset in processor_power_presets {
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!("processor-power-preset-option-{preset:?}")),
                            processor_power_preset_label(preset),
                            selected_preset == Some(preset),
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            app.fill_processor_power_preset(preset);
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                options
            },
        );

        v_flex()
            .w_full()
            .min_w(px(0.0))
            .gap_2()
            .child(self.render_processor_power_plan_picker(window, cx))
            .child(text_muted(self.effective_power_mode_status()))
            .when_some(target_plan_notice, |card, (notice, warning)| {
                if warning {
                    card.child(text_warning(notice))
                } else {
                    card.child(text_muted(notice))
                }
            })
            .child(feature_toggle_switch(
                "processor-power-link-ac-dc",
                t!("processor_power.link_ac_dc").to_string(),
                self.processor_power_link_ac_dc,
                cx.listener(|app, checked: &bool, _, cx| {
                    app.processor_power_link_ac_dc = *checked;
                    if *checked {
                        let values = app.processor_power_values();
                        app.set_processor_power_values(ProcessorPowerAcDcValues::same(values.ac));
                        app.processor_power_dirty = true;
                    }
                    cx.notify();
                }),
            ))
            .child(setting_action_card(
                "processor-power-presets-card",
                t!("processor_power.presets").to_string(),
                preset_dropdown,
            ))
            .child(
                v_flex()
                    .w_full()
                    .gap_2()
                    .child(
                        v_flex()
                            .w_full()
                            .min_w(px(0.0))
                            .gap_1()
                            .child(processor_power_column_header(
                                t!("processor_power.ac_values").to_string(),
                            ))
                            .child(processor_power_slider(
                                "processor-power-ac-core-parking-min",
                                &t!("processor_power.core_parking_min"),
                                self.render_numeric_value(
                                    NumericField::ProcessorAcCoreParkingMin,
                                    format!("{}%", self.processor_power_ac_core_parking_min),
                                    self.processor_power_ac_core_parking_min.to_string(),
                                    cx,
                                ),
                                &self.inputs.processor_power_ac_core_parking_min,
                                window,
                                cx,
                                cx.listener(|app, change: &StepChange<u64>, _, cx| {
                                    let value = apply_u64_step(
                                        app.processor_power_ac_core_parking_min,
                                        change,
                                        0,
                                        100,
                                    );
                                    app.set_processor_power_slider_value(
                                        ProcessorPowerSlider::AcCoreParkingMin,
                                        value,
                                    );
                                    cx.notify();
                                }),
                            ))
                            .child(processor_power_slider(
                                "processor-power-ac-performance-min",
                                &t!("processor_power.processor_min"),
                                self.render_numeric_value(
                                    NumericField::ProcessorAcPerformanceMin,
                                    format!("{}%", self.processor_power_ac_performance_min),
                                    self.processor_power_ac_performance_min.to_string(),
                                    cx,
                                ),
                                &self.inputs.processor_power_ac_performance_min,
                                window,
                                cx,
                                cx.listener(|app, change: &StepChange<u64>, _, cx| {
                                    let value = apply_u64_step(
                                        app.processor_power_ac_performance_min,
                                        change,
                                        0,
                                        100,
                                    );
                                    app.set_processor_power_slider_value(
                                        ProcessorPowerSlider::AcPerformanceMin,
                                        value,
                                    );
                                    cx.notify();
                                }),
                            ))
                            .child(processor_power_slider(
                                "processor-power-ac-performance-max",
                                &t!("processor_power.processor_max"),
                                self.render_numeric_value(
                                    NumericField::ProcessorAcPerformanceMax,
                                    format!("{}%", self.processor_power_ac_performance_max),
                                    self.processor_power_ac_performance_max.to_string(),
                                    cx,
                                ),
                                &self.inputs.processor_power_ac_performance_max,
                                window,
                                cx,
                                cx.listener(|app, change: &StepChange<u64>, _, cx| {
                                    let value = apply_u64_step(
                                        app.processor_power_ac_performance_max,
                                        change,
                                        0,
                                        100,
                                    );
                                    app.set_processor_power_slider_value(
                                        ProcessorPowerSlider::AcPerformanceMax,
                                        value,
                                    );
                                    cx.notify();
                                }),
                            ))
                            .child(processor_power_slider(
                                "processor-power-ac-boost-policy",
                                &t!("processor_power.boost_policy"),
                                self.render_numeric_value(
                                    NumericField::ProcessorAcBoostPolicy,
                                    format!("{}%", self.processor_power_ac_boost_policy),
                                    self.processor_power_ac_boost_policy.to_string(),
                                    cx,
                                ),
                                &self.inputs.processor_power_ac_boost_policy,
                                window,
                                cx,
                                cx.listener(|app, change: &StepChange<u64>, _, cx| {
                                    let value = apply_u64_step(
                                        app.processor_power_ac_boost_policy,
                                        change,
                                        0,
                                        100,
                                    );
                                    app.set_processor_power_slider_value(
                                        ProcessorPowerSlider::AcBoostPolicy,
                                        value,
                                    );
                                    cx.notify();
                                }),
                            ))
                            .child(processor_power_setting_row(
                                "processor-power-ac-boost-mode",
                                t!("processor_power.boost_mode").to_string(),
                                self.render_processor_boost_mode_picker(
                                    ProcessorPowerSource::Ac,
                                    window,
                                    cx,
                                ),
                            )),
                    )
                    .child(
                        v_flex()
                            .w_full()
                            .min_w(px(0.0))
                            .gap_1()
                            .child(processor_power_column_header(
                                t!("processor_power.dc_values").to_string(),
                            ))
                            .child(processor_power_slider(
                                "processor-power-dc-core-parking-min",
                                &t!("processor_power.core_parking_min"),
                                self.render_numeric_value(
                                    NumericField::ProcessorDcCoreParkingMin,
                                    format!("{}%", self.processor_power_dc_core_parking_min),
                                    self.processor_power_dc_core_parking_min.to_string(),
                                    cx,
                                ),
                                &self.inputs.processor_power_dc_core_parking_min,
                                window,
                                cx,
                                cx.listener(|app, change: &StepChange<u64>, _, cx| {
                                    let value = apply_u64_step(
                                        app.processor_power_dc_core_parking_min,
                                        change,
                                        0,
                                        100,
                                    );
                                    app.set_processor_power_slider_value(
                                        ProcessorPowerSlider::DcCoreParkingMin,
                                        value,
                                    );
                                    cx.notify();
                                }),
                            ))
                            .child(processor_power_slider(
                                "processor-power-dc-performance-min",
                                &t!("processor_power.processor_min"),
                                self.render_numeric_value(
                                    NumericField::ProcessorDcPerformanceMin,
                                    format!("{}%", self.processor_power_dc_performance_min),
                                    self.processor_power_dc_performance_min.to_string(),
                                    cx,
                                ),
                                &self.inputs.processor_power_dc_performance_min,
                                window,
                                cx,
                                cx.listener(|app, change: &StepChange<u64>, _, cx| {
                                    let value = apply_u64_step(
                                        app.processor_power_dc_performance_min,
                                        change,
                                        0,
                                        100,
                                    );
                                    app.set_processor_power_slider_value(
                                        ProcessorPowerSlider::DcPerformanceMin,
                                        value,
                                    );
                                    cx.notify();
                                }),
                            ))
                            .child(processor_power_slider(
                                "processor-power-dc-performance-max",
                                &t!("processor_power.processor_max"),
                                self.render_numeric_value(
                                    NumericField::ProcessorDcPerformanceMax,
                                    format!("{}%", self.processor_power_dc_performance_max),
                                    self.processor_power_dc_performance_max.to_string(),
                                    cx,
                                ),
                                &self.inputs.processor_power_dc_performance_max,
                                window,
                                cx,
                                cx.listener(|app, change: &StepChange<u64>, _, cx| {
                                    let value = apply_u64_step(
                                        app.processor_power_dc_performance_max,
                                        change,
                                        0,
                                        100,
                                    );
                                    app.set_processor_power_slider_value(
                                        ProcessorPowerSlider::DcPerformanceMax,
                                        value,
                                    );
                                    cx.notify();
                                }),
                            ))
                            .child(processor_power_slider(
                                "processor-power-dc-boost-policy",
                                &t!("processor_power.boost_policy"),
                                self.render_numeric_value(
                                    NumericField::ProcessorDcBoostPolicy,
                                    format!("{}%", self.processor_power_dc_boost_policy),
                                    self.processor_power_dc_boost_policy.to_string(),
                                    cx,
                                ),
                                &self.inputs.processor_power_dc_boost_policy,
                                window,
                                cx,
                                cx.listener(|app, change: &StepChange<u64>, _, cx| {
                                    let value = apply_u64_step(
                                        app.processor_power_dc_boost_policy,
                                        change,
                                        0,
                                        100,
                                    );
                                    app.set_processor_power_slider_value(
                                        ProcessorPowerSlider::DcBoostPolicy,
                                        value,
                                    );
                                    cx.notify();
                                }),
                            ))
                            .child(processor_power_setting_row(
                                "processor-power-dc-boost-mode",
                                t!("processor_power.boost_mode").to_string(),
                                self.render_processor_boost_mode_picker(
                                    ProcessorPowerSource::Dc,
                                    window,
                                    cx,
                                ),
                            )),
                    ),
            )
            .child(
                h_flex()
                    .gap_2()
                    .justify_end()
                    .child(
                        control_button(Button::new("processor-power-refresh-values"))
                            .label(t!("processor_power.refresh_values").to_string())
                            .disabled(!has_target_plan)
                            .on_click(cx.listener(|app, _, _, cx| {
                                app.refresh_processor_power_values();
                                cx.notify();
                            })),
                    )
                    .child(
                        primary_control_button(Button::new("processor-power-apply-custom"), cx)
                            .label(t!("processor_power.apply_custom").to_string())
                            .disabled(!has_target_plan)
                            .on_click(cx.listener(|app, _, _, cx| {
                                app.apply_processor_power_custom();
                                cx.notify();
                            })),
                    ),
            )
            .into_any_element()
    }

    pub(in crate::ui::app) fn effective_power_mode_status(&self) -> String {
        t!(
            "processor_power.effective_power_mode",
            mode = effective_power_mode_label(self.effective_power_mode)
        )
        .to_string()
    }

    pub(in crate::ui::app) fn processor_power_target_plan_notice(&self) -> Option<(String, bool)> {
        let target_plan = self.processor_power_target_plan()?;
        if !target_plan.active {
            let active_plan = self
                .current_plan
                .as_ref()
                .map(|plan| plan.name.clone())
                .unwrap_or_else(|| t!("processor_power.no_active_plan").to_string());
            return Some((
                t!("processor_power.target_plan_inactive", plan = active_plan).to_string(),
                false,
            ));
        }

        if self.processor_power_target_plan_personality != Some(PowerPlanPersonality::Balanced)
            || matches!(
                self.effective_power_mode,
                EffectivePowerMode::Unknown | EffectivePowerMode::Balanced
            )
        {
            return None;
        }

        Some((t!("processor_power.overlay_warning").to_string(), true))
    }

    pub(in crate::ui::app) fn render_processor_boost_mode_picker(
        &self,
        source: ProcessorPowerSource,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let picker_id = processor_boost_mode_picker_id(source);
        let is_open = self.active_power_plan_picker.as_deref() == Some(picker_id);
        let placement = self.dropdown_placement(
            picker_id,
            dropdown_list_height(ProcessorBoostMode::ALL.len()),
            window,
        );
        let selected = match source {
            ProcessorPowerSource::Ac => self.processor_power_ac_boost_mode,
            ProcessorPowerSource::Dc => self.processor_power_dc_boost_mode,
        };
        let mut options = dropdown_surface(cx, placement.max_height);
        for boost_mode in ProcessorBoostMode::ALL {
            options = options.child(
                dropdown_option_row(
                    SharedString::from(format!(
                        "processor-boost-mode-{source:?}-option-{boost_mode:?}"
                    )),
                    processor_boost_mode_label(boost_mode),
                    selected == boost_mode,
                    cx,
                )
                .on_click(cx.listener(move |app, _: &gpui::ClickEvent, _, cx| {
                    app.set_processor_power_boost_mode(source, boost_mode);
                    app.active_power_plan_picker = None;
                    cx.notify();
                })),
            );
        }

        let phase = dropdown_popup_phase(picker_id, is_open, cx);
        dropdown_select_container(DropdownSelectWidth::Wide)
            .child(
                dropdown_select_control(
                    SharedString::from(format!("{picker_id}-control")),
                    processor_boost_mode_label(selected),
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

    pub(in crate::ui::app) fn render_processor_power_plan_picker(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let id = "processor-power-target-plan";
        let is_open = self.active_power_plan_picker.as_deref() == Some(id);
        let option_count = self.plans.len().max(1);
        let placement = self.dropdown_placement(id, dropdown_list_height(option_count), window);
        let selected_guid = self
            .processor_power_target_plan_guid
            .as_deref()
            .or_else(|| self.current_plan.as_ref().map(|plan| plan.guid.as_str()));
        let selected_text = selected_guid
            .and_then(|guid| {
                self.plans
                    .iter()
                    .find(|plan| plan.guid.eq_ignore_ascii_case(guid))
            })
            .map(PowerPlan::display_name)
            .unwrap_or_else(|| t!("processor_power.no_active_plan").to_string());

        let mut options = dropdown_surface(cx, placement.max_height);

        if self.plans.is_empty() {
            options = options.child(dropdown_empty_row(
                t!("common.no_power_plans_loaded").to_string(),
                cx,
            ));
        } else {
            for plan in &self.plans {
                let selected =
                    selected_guid.is_some_and(|selected| selected.eq_ignore_ascii_case(&plan.guid));
                options = options.child(power_plan_option_row(
                    format!("{id}-{}", plan.guid),
                    plan.display_name(),
                    selected,
                    Some(plan.guid.clone()),
                    PowerPlanField::ProcessorPowerTarget,
                    cx,
                ));
            }
        }

        let phase = dropdown_popup_phase(id, is_open, cx);
        let target_plan_select = dropdown_select_container(DropdownSelectWidth::Wide)
            .child(
                dropdown_select_control(
                    "processor-power-target-plan-control",
                    selected_text,
                    true,
                    is_open,
                    phase,
                    cx,
                )
                .on_click(cx.listener(|app, _: &gpui::ClickEvent, _, cx| {
                    app.refresh_power_plans();
                    app.active_power_plan_picker = (app.active_power_plan_picker.as_deref()
                        != Some("processor-power-target-plan"))
                    .then_some("processor-power-target-plan".to_owned());
                    cx.notify();
                })),
            )
            .child(dropdown_anchor_sensor(
                id,
                Rc::clone(&self.dropdown_anchor_bounds),
            ))
            .child(dropdown_popup_or_empty(
                SharedString::from(id),
                phase,
                placement,
                options,
                cx,
            ));

        let picker = v_flex().w_full().min_w(px(0.0)).relative().child(
            h_flex()
                .id("processor-power-target-plan-card")
                .h(px(CARD_ROW_HEIGHT))
                .w_full()
                .items_center()
                .justify_between()
                .gap_2()
                .py_3()
                .px_4()
                .relative()
                .overflow_hidden()
                .rounded(px(BRAND_RADIUS_CONTROL))
                .bg(rgb(settings_card_color()))
                .text_color(rgb(primary_text_color()))
                .text_size(px(TEXT_BODY_SIZE))
                .line_height(px(TEXT_BODY_LINE_HEIGHT))
                .child(
                    h_flex()
                        .flex_1()
                        .min_w(px(0.0))
                        .items_center()
                        .gap_1()
                        .child(
                            div()
                                .min_w(px(0.0))
                                .truncate()
                                .child(t!("processor_power.target_plan").to_string()),
                        )
                        .child(title_info_button(
                            "processor-power-target-plan-info",
                            t!("processor_power.help").to_string(),
                        )),
                )
                .child(target_plan_select),
        );

        picker
    }
}
