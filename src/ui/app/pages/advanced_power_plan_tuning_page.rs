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

    pub(in crate::ui::app) fn render_advanced_power_plan_tuning_side_panel(
        &self,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let header = h_flex()
            .min_h(px(48.0))
            .px_3()
            .child(section_title_text(
                t!("processor_power.presets").to_string(),
            ))
            .into_any_element();
        let row_hover = cx.theme().secondary_hover;
        let mut presets = v_flex()
            .flex_1()
            .min_h(px(0.0))
            .w_full()
            .min_w(px(0.0))
            .overflow_y_scrollbar()
            .gap_4()
            .p_3();
        let mut built_in_presets = v_flex().w_full().gap_1().child(
            text_muted(t!("processor_power.built_in_presets").to_string())
                .px_1()
                .pb_1(),
        );
        for preset in processor_power_builtin_presets() {
            built_in_presets = built_in_presets.child(
                h_flex()
                    .id(SharedString::from(format!(
                        "advanced-power-plan-tuning-built-in-preset-{preset:?}"
                    )))
                    .w_full()
                    .min_w(px(0.0))
                    .h(px(40.0))
                    .gap_2()
                    .px_2()
                    .rounded(px(BRAND_RADIUS_CONTROL))
                    .text_size(px(TEXT_CONTROL_SIZE))
                    .line_height(px(TEXT_CONTROL_LINE_HEIGHT))
                    .hover(move |style| style.bg(row_hover))
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .truncate()
                            .child(processor_power_preset_label(preset)),
                    )
                    .child(
                        control_button(
                            Button::new(SharedString::from(format!(
                                "view-advanced-power-plan-tuning-built-in-preset-{preset:?}"
                            )))
                            .ghost(),
                        )
                        .with_size(px(28.0))
                        .icon(Icon::new(NavIcon::Info).with_size(px(12.0)))
                        .tooltip(t!("processor_power.view_preset").to_string())
                        .on_click(cx.listener(
                            move |app, _, window, cx| {
                                app.open_advanced_power_plan_tuning_preset_editor(
                                    AdvancedPowerPlanTuningPresetEditorTarget::BuiltIn(preset),
                                    window,
                                    cx,
                                );
                            },
                        )),
                    ),
            );
        }
        presets = presets.child(built_in_presets);

        let mut custom_presets = v_flex().w_full().gap_1().child(
            text_muted(t!("processor_power.custom_presets").to_string())
                .px_1()
                .pb_1(),
        );
        for (index, preset) in self
            .settings
            .advanced_power_plan_tuning_presets
            .iter()
            .enumerate()
        {
            let removal_target = ListItemRemovalTarget::new(
                ListItemRemovalKind::AdvancedPowerPlanTuningPreset,
                index,
            );
            let row = h_flex()
                .id(SharedString::from(format!(
                    "advanced-power-plan-tuning-custom-preset-row-{index}"
                )))
                .w_full()
                .min_w(px(0.0))
                .h(px(40.0))
                .gap_2()
                .px_2()
                .rounded(px(BRAND_RADIUS_CONTROL))
                .text_size(px(TEXT_CONTROL_SIZE))
                .line_height(px(TEXT_CONTROL_LINE_HEIGHT))
                .hover(move |style| style.bg(row_hover))
                .child(
                    div()
                        .flex_1()
                        .min_w(px(0.0))
                        .truncate()
                        .child(advanced_power_plan_tuning_preset_label(&preset.name)),
                )
                .child(
                    h_flex()
                        .flex_shrink_0()
                        .gap_1()
                        .child(
                            control_button(
                                Button::new(SharedString::from(format!(
                                    "edit-advanced-power-plan-tuning-preset-{index}"
                                )))
                                .ghost(),
                            )
                            .with_size(px(28.0))
                            .icon(Icon::new(NavIcon::SquarePen).with_size(px(12.0)))
                            .tooltip(t!("common.edit").to_string())
                            .on_click(cx.listener(
                                move |app, _, window, cx| {
                                    app.open_advanced_power_plan_tuning_preset_editor(
                                        AdvancedPowerPlanTuningPresetEditorTarget::Custom(Some(
                                            index,
                                        )),
                                        window,
                                        cx,
                                    );
                                },
                            )),
                        )
                        .child(
                            remove_control_button(Button::new(SharedString::from(format!(
                                "remove-advanced-power-plan-tuning-preset-{index}"
                            ))))
                            .with_size(px(28.0))
                            .icon(Icon::new(NavIcon::Trash2).with_size(px(12.0)))
                            .on_click(cx.listener(
                                move |app, _, _, cx| {
                                    app.request_list_item_removal(removal_target, cx);
                                },
                            )),
                        ),
                )
                .into_any_element();
            custom_presets = custom_presets.child(self.animated_list_item(
                removal_target,
                SharedString::from(format!("advanced-power-plan-tuning-preset-{index}")),
                row,
            ));
        }
        if self.settings.advanced_power_plan_tuning_presets.is_empty() {
            custom_presets = custom_presets.child(
                text_muted(t!("processor_power.no_custom_presets").to_string())
                    .px_1()
                    .py_2(),
            );
        }
        presets = presets.child(custom_presets);

        let body = v_flex()
            .flex_1()
            .min_h(px(0.0))
            .overflow_hidden()
            .child(presets)
            .child(
                div()
                    .w_full()
                    .p_3()
                    .border_t_1()
                    .border_color(rgb(border_color()))
                    .child(
                        primary_control_button(
                            Button::new("add-advanced-power-plan-tuning-preset"),
                            cx,
                        )
                        .w_full()
                        .icon(Icon::new(NavIcon::Plus).with_size(px(14.0)))
                        .label(t!("processor_power.add_preset").to_string())
                        .on_click(cx.listener(|app, _, window, cx| {
                            app.open_advanced_power_plan_tuning_preset_editor(
                                AdvancedPowerPlanTuningPresetEditorTarget::Custom(None),
                                window,
                                cx,
                            );
                        })),
                    ),
            )
            .into_any_element();

        page_side_panel(header, body)
    }

    fn open_advanced_power_plan_tuning_preset_editor(
        &mut self,
        target: AdvancedPowerPlanTuningPresetEditorTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let (name, values) = match target {
            AdvancedPowerPlanTuningPresetEditorTarget::BuiltIn(preset) => {
                (None, ProcessorPowerValues::for_preset(preset))
            }
            AdvancedPowerPlanTuningPresetEditorTarget::Custom(Some(index)) => {
                let Some(preset) = self.settings.advanced_power_plan_tuning_presets.get(index)
                else {
                    return;
                };
                (
                    Some(preset.name.clone()),
                    advanced_power_plan_tuning_preset_values(preset),
                )
            }
            AdvancedPowerPlanTuningPresetEditorTarget::Custom(None) => {
                (Some(String::new()), self.processor_power_values().ac)
            }
        };

        let sliders = [
            make_processor_power_slider(cx, u64::from(values.core_parking_min)),
            make_processor_power_slider(cx, u64::from(values.performance_min)),
            make_processor_power_slider(cx, u64::from(values.performance_max)),
            make_processor_power_slider(cx, u64::from(values.boost_policy)),
        ];
        let slider_subscriptions = ADVANCED_POWER_PLAN_TUNING_PRESET_FIELDS
            .into_iter()
            .zip(sliders.iter())
            .map(|(field, slider)| {
                cx.subscribe_in(slider, window, move |app, _, event: &SliderEvent, _, cx| {
                    let SliderEvent::Change(value) = event;
                    app.set_advanced_power_plan_tuning_preset_field_value(
                        field,
                        value.end().round() as u64,
                    );
                    cx.notify();
                })
            })
            .collect();
        self.advanced_power_plan_tuning_preset_editor = Some(AdvancedPowerPlanTuningPresetEditor {
            target,
            values,
            sliders,
            _slider_subscriptions: slider_subscriptions,
        });
        self.active_power_plan_picker = None;
        if let Some(name) = name {
            clear_input_to(
                &self.inputs.advanced_power_plan_tuning_preset_name,
                &name,
                window,
                cx,
            );
            self.inputs
                .advanced_power_plan_tuning_preset_name
                .read(cx)
                .focus_handle(cx)
                .focus(window);
        }
        cx.notify();
    }

    pub(in crate::ui::app) fn close_advanced_power_plan_tuning_preset_editor(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        self.advanced_power_plan_tuning_preset_editor = None;
        self.editing_numeric = None;
        self.active_power_plan_picker = None;
        cx.notify();
    }

    fn save_advanced_power_plan_tuning_preset(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = self.advanced_power_plan_tuning_preset_editor.as_ref() else {
            return;
        };
        let AdvancedPowerPlanTuningPresetEditorTarget::Custom(index) = editor.target else {
            return;
        };
        let name = self
            .inputs
            .advanced_power_plan_tuning_preset_name
            .read(cx)
            .value()
            .to_string();
        if upsert_advanced_power_plan_tuning_preset(
            &mut self.settings.advanced_power_plan_tuning_presets,
            index,
            &name,
            editor.values,
        ) {
            self.close_advanced_power_plan_tuning_preset_editor(cx);
        }
    }

    fn step_advanced_power_plan_tuning_preset_value(
        &mut self,
        field: AdaptiveEngineProcessorPowerPolicyField,
        change: &StepChange<u64>,
    ) {
        let Some(editor) = self.advanced_power_plan_tuning_preset_editor.as_mut() else {
            return;
        };
        let current = advanced_power_plan_tuning_preset_field_value(editor.values, field);
        let value = apply_u64_step(current, change, 0, 100);
        self.set_advanced_power_plan_tuning_preset_field_value(field, value);
    }

    pub(in crate::ui::app) fn set_advanced_power_plan_tuning_preset_field_value(
        &mut self,
        field: AdaptiveEngineProcessorPowerPolicyField,
        value: u64,
    ) {
        let Some(editor) = self.advanced_power_plan_tuning_preset_editor.as_mut() else {
            return;
        };
        set_advanced_power_plan_tuning_preset_field_value(
            &mut editor.values,
            field,
            value.min(100) as u32,
        );
        editor.values = editor.values.normalized();
    }

    fn set_advanced_power_plan_tuning_preset_boost_mode(&mut self, boost_mode: ProcessorBoostMode) {
        let Some(editor) = self.advanced_power_plan_tuning_preset_editor.as_mut() else {
            return;
        };
        editor.values.boost_mode = boost_mode;
    }

    fn render_advanced_power_plan_tuning_preset_values(
        &self,
        editable: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let editor = self
            .advanced_power_plan_tuning_preset_editor
            .as_ref()
            .expect("processor tuning preset values require an editor");
        let values = editor.values;
        for (field, slider) in ADVANCED_POWER_PLAN_TUNING_PRESET_FIELDS
            .into_iter()
            .zip(editor.sliders.iter())
        {
            let value = advanced_power_plan_tuning_preset_field_value(values, field) as f32;
            slider.update(cx, |state, cx| {
                if (state.value().end() - value).abs() > f32::EPSILON {
                    state.set_value(value, window, cx);
                }
            });
        }
        let rows = [
            (
                AdaptiveEngineProcessorPowerPolicyField::CoreParkingMin,
                t!("processor_power.core_parking_min").to_string(),
                values.core_parking_min,
                &editor.sliders[0],
            ),
            (
                AdaptiveEngineProcessorPowerPolicyField::PerformanceMin,
                t!("processor_power.processor_min").to_string(),
                values.performance_min,
                &editor.sliders[1],
            ),
            (
                AdaptiveEngineProcessorPowerPolicyField::PerformanceMax,
                t!("processor_power.processor_max").to_string(),
                values.performance_max,
                &editor.sliders[2],
            ),
            (
                AdaptiveEngineProcessorPowerPolicyField::BoostPolicy,
                t!("processor_power.boost_policy").to_string(),
                values.boost_policy,
                &editor.sliders[3],
            ),
        ];
        let mut panel = branded_panel();
        for (index, (field, label, value, slider)) in rows.into_iter().enumerate() {
            let id = SharedString::from(format!("advanced-power-plan-tuning-preset-{field:?}"));
            panel = panel.child(if editable {
                processor_power_group_slider(
                    id,
                    &label,
                    self.render_numeric_value(
                        NumericField::AdvancedPowerPlanTuningPreset(field),
                        format!("{value}%"),
                        value.to_string(),
                        cx,
                    ),
                    slider,
                    window,
                    cx,
                    cx.listener(move |app, change: &StepChange<u64>, _, cx| {
                        app.step_advanced_power_plan_tuning_preset_value(field, change);
                        cx.notify();
                    }),
                )
            } else {
                win32_priority_registry_value_row(id, label, None, format!("{value}%"), index != 0)
            });
        }
        panel = panel.child(if editable {
            setting_group_action_row(
                "advanced-power-plan-tuning-preset-boost-mode",
                t!("processor_power.boost_mode").to_string(),
                self.render_advanced_power_plan_tuning_preset_boost_mode_picker(window, cx),
                true,
            )
            .into_any_element()
        } else {
            win32_priority_registry_value_row(
                "advanced-power-plan-tuning-preset-boost-mode",
                t!("processor_power.boost_mode").to_string(),
                None,
                processor_boost_mode_label(values.boost_mode),
                true,
            )
        });

        panel.into_any_element()
    }

    fn render_advanced_power_plan_tuning_preset_boost_mode_picker(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let picker_id = "advanced-power-plan-tuning-preset-boost-mode-picker";
        let is_open = self.active_power_plan_picker.as_deref() == Some(picker_id);
        let placement = self.dropdown_placement(
            picker_id,
            dropdown_list_height(ProcessorBoostMode::ALL.len()),
            window,
        );
        let editor = self
            .advanced_power_plan_tuning_preset_editor
            .as_ref()
            .expect("processor tuning preset boost picker requires an editor");
        let selected = editor.values.boost_mode;
        let mut options = dropdown_surface(cx, placement.max_height);
        for boost_mode in ProcessorBoostMode::ALL {
            options = options.child(
                dropdown_option_row(
                    SharedString::from(format!("{picker_id}-option-{boost_mode:?}")),
                    processor_boost_mode_label(boost_mode),
                    selected == boost_mode,
                    cx,
                )
                .on_click(cx.listener(move |app, _, _, cx| {
                    app.set_advanced_power_plan_tuning_preset_boost_mode(boost_mode);
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
                .on_click(cx.listener(move |app, _, _, cx| {
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
                picker_id.into(),
                phase,
                placement,
                options,
                cx,
            ))
            .into_any_element()
    }

    pub(in crate::ui::app) fn render_advanced_power_plan_tuning_preset_modal(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let editor = self
            .advanced_power_plan_tuning_preset_editor
            .as_ref()
            .expect("processor tuning preset modal requires an editor");
        let input = &self.inputs.advanced_power_plan_tuning_preset_name;
        let name = input.read(cx).value().to_string();
        let custom_index = match editor.target {
            AdvancedPowerPlanTuningPresetEditorTarget::BuiltIn(_) => None,
            AdvancedPowerPlanTuningPresetEditorTarget::Custom(index) => index,
        };
        let edits_custom = matches!(
            editor.target,
            AdvancedPowerPlanTuningPresetEditorTarget::Custom(_)
        );
        let duplicate_name = edits_custom
            && advanced_power_plan_tuning_preset_name_exists(
                &self.settings.advanced_power_plan_tuning_presets,
                custom_index,
                &name,
            );
        let can_save = !name.trim().is_empty() && !duplicate_name;
        let name_focused = input.read(cx).focus_handle(cx).is_focused(window);
        let title = match editor.target {
            AdvancedPowerPlanTuningPresetEditorTarget::BuiltIn(preset) => format!(
                "{}: {}",
                t!("processor_power.view_preset"),
                processor_power_preset_label(preset)
            ),
            AdvancedPowerPlanTuningPresetEditorTarget::Custom(Some(_)) => {
                t!("processor_power.edit_preset").to_string()
            }
            AdvancedPowerPlanTuningPresetEditorTarget::Custom(None) => {
                t!("processor_power.add_preset").to_string()
            }
        };
        let name_help = if duplicate_name {
            text_warning(t!("processor_power.duplicate_preset_name").to_string())
        } else {
            text_muted(t!("processor_power.preset_name_help").to_string())
        };
        let mut content = v_flex().w_full().min_w(px(0.0)).gap_4().p_4();
        if edits_custom {
            content = content.child(
                branded_panel()
                    .child(setting_group_stacked_action_row(
                        "advanced-power-plan-tuning-preset-name-row",
                        t!("processor_power.preset_name").to_string(),
                        app_input(input, name_focused, cx).into_any_element(),
                        false,
                    ))
                    .child(name_help.px_4().pb_3()),
            );
        }
        content = content.child(self.render_advanced_power_plan_tuning_preset_values(
            edits_custom,
            window,
            cx,
        ));

        let mut footer = h_flex()
            .w_full()
            .flex_shrink_0()
            .items_center()
            .justify_end()
            .gap_2()
            .p_4()
            .border_t_1()
            .border_color(rgb(border_color()));
        if edits_custom {
            footer = footer
                .child(
                    control_button(Button::new("cancel-advanced-power-plan-tuning-preset"))
                        .label(t!("common.cancel").to_string())
                        .on_click(cx.listener(|app, _, _, cx| {
                            app.close_advanced_power_plan_tuning_preset_editor(cx);
                        })),
                )
                .child(
                    primary_control_button(
                        Button::new("save-advanced-power-plan-tuning-preset"),
                        cx,
                    )
                    .label(t!("common.save").to_string())
                    .disabled(!can_save)
                    .on_click(cx.listener(|app, _, _, cx| {
                        app.save_advanced_power_plan_tuning_preset(cx);
                    })),
                );
        } else {
            footer = footer.child(
                primary_control_button(
                    Button::new("close-advanced-power-plan-tuning-preset-view"),
                    cx,
                )
                .label(t!("common.done").to_string())
                .on_click(cx.listener(|app, _, _, cx| {
                    app.close_advanced_power_plan_tuning_preset_editor(cx);
                })),
            );
        }

        let modal = v_flex()
            .w_full()
            .max_w(px(760.0))
            .h_full()
            .max_h(px(620.0))
            .overflow_hidden()
            .rounded(px(BRAND_RADIUS_OVERLAY))
            .border_1()
            .border_color(rgb(border_color()))
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
            .child(
                h_flex()
                    .w_full()
                    .flex_shrink_0()
                    .items_center()
                    .justify_between()
                    .gap_3()
                    .p_4()
                    .border_b_1()
                    .border_color(rgb(border_color()))
                    .child(section_title_text(title))
                    .child(
                        control_button(Button::new("close-advanced-power-plan-tuning-preset"))
                            .with_size(px(32.0))
                            .icon(Icon::new(NavIcon::X).with_size(px(14.0)))
                            .on_click(cx.listener(|app, _, _, cx| {
                                app.close_advanced_power_plan_tuning_preset_editor(cx);
                            })),
                    ),
            )
            .child(
                div()
                    .id("advanced-power-plan-tuning-preset-scroll")
                    .flex_1()
                    .min_h(px(0.0))
                    .overflow_y_scrollbar()
                    .child(content),
            )
            .child(footer);
        let modal = with_optional_motion(
            modal,
            "advanced-power-plan-tuning-preset-modal-open",
            MotionSpeed::Standard,
            |modal| modal,
            |modal, delta| {
                modal
                    .relative()
                    .top(px(10.0 * (1.0 - delta)))
                    .opacity(0.18 + 0.82 * delta)
            },
        );
        let backdrop = h_flex()
            .absolute()
            .inset_0()
            .size_full()
            .items_center()
            .justify_center()
            .p_4()
            .bg(rgba(0x0000008c))
            .occlude()
            .on_any_mouse_down(cx.listener(|app, _, _, cx| {
                app.close_advanced_power_plan_tuning_preset_editor(cx);
            }))
            .child(modal);

        with_optional_motion(
            backdrop,
            "advanced-power-plan-tuning-preset-backdrop-open",
            MotionSpeed::Fast,
            |backdrop| backdrop,
            |backdrop, delta| backdrop.opacity(delta),
        )
    }

    pub(in crate::ui::app) fn render_processor_power_card(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.sync_processor_power_slider_states(window, cx);
        let has_target_plan = self.processor_power_target_plan().is_some();
        let target_plan_notice = self.processor_power_target_plan_notice();

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
            .child(self.render_processor_power_source_group(ProcessorPowerSource::Ac, window, cx))
            .child(self.render_processor_power_source_group(
                ProcessorPowerSource::Battery,
                window,
                cx,
            ))
            .child(
                h_flex().justify_end().child(
                    control_button(Button::new("processor-power-refresh-values"))
                        .label(t!("processor_power.refresh_values").to_string())
                        .disabled(!has_target_plan)
                        .on_click(cx.listener(|app, _, _, cx| {
                            app.refresh_processor_power_values();
                            cx.notify();
                        })),
                ),
            )
            .into_any_element()
    }

    fn render_processor_power_source_group(
        &self,
        source: ProcessorPowerSource,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (target, title, boost_mode_id, rows) = match source {
            ProcessorPowerSource::Ac => (
                SettingGroupTarget::ProcessorPowerAc,
                t!("processor_power.ac_preset").to_string(),
                "processor-power-ac-boost-mode",
                [
                    (
                        "processor-power-ac-core-parking-min",
                        t!("processor_power.core_parking_min").to_string(),
                        NumericField::ProcessorAcCoreParkingMin,
                        ProcessorPowerSlider::AcCoreParkingMin,
                        self.processor_power_ac_core_parking_min,
                    ),
                    (
                        "processor-power-ac-performance-min",
                        t!("processor_power.processor_min").to_string(),
                        NumericField::ProcessorAcPerformanceMin,
                        ProcessorPowerSlider::AcPerformanceMin,
                        self.processor_power_ac_performance_min,
                    ),
                    (
                        "processor-power-ac-performance-max",
                        t!("processor_power.processor_max").to_string(),
                        NumericField::ProcessorAcPerformanceMax,
                        ProcessorPowerSlider::AcPerformanceMax,
                        self.processor_power_ac_performance_max,
                    ),
                    (
                        "processor-power-ac-boost-policy",
                        t!("processor_power.boost_policy").to_string(),
                        NumericField::ProcessorAcBoostPolicy,
                        ProcessorPowerSlider::AcBoostPolicy,
                        self.processor_power_ac_boost_policy,
                    ),
                ],
            ),
            ProcessorPowerSource::Battery => (
                SettingGroupTarget::ProcessorPowerBattery,
                t!("processor_power.battery_preset").to_string(),
                "processor-power-battery-boost-mode",
                [
                    (
                        "processor-power-battery-core-parking-min",
                        t!("processor_power.core_parking_min").to_string(),
                        NumericField::ProcessorDcCoreParkingMin,
                        ProcessorPowerSlider::BatteryCoreParkingMin,
                        self.processor_power_battery_core_parking_min,
                    ),
                    (
                        "processor-power-battery-performance-min",
                        t!("processor_power.processor_min").to_string(),
                        NumericField::ProcessorDcPerformanceMin,
                        ProcessorPowerSlider::BatteryPerformanceMin,
                        self.processor_power_battery_performance_min,
                    ),
                    (
                        "processor-power-battery-performance-max",
                        t!("processor_power.processor_max").to_string(),
                        NumericField::ProcessorDcPerformanceMax,
                        ProcessorPowerSlider::BatteryPerformanceMax,
                        self.processor_power_battery_performance_max,
                    ),
                    (
                        "processor-power-battery-boost-policy",
                        t!("processor_power.boost_policy").to_string(),
                        NumericField::ProcessorDcBoostPolicy,
                        ProcessorPowerSlider::BatteryBoostPolicy,
                        self.processor_power_battery_boost_policy,
                    ),
                ],
            ),
        };
        let mut controls = Vec::with_capacity(rows.len() + 1);
        for (id, label, numeric_field, slider, value) in rows {
            controls.push(processor_power_group_slider(
                id,
                &label,
                self.render_numeric_value(
                    numeric_field,
                    format!("{value}%"),
                    value.to_string(),
                    cx,
                ),
                &processor_power_slider_input(&self.inputs, slider),
                window,
                cx,
                cx.listener(move |app, change: &StepChange<u64>, _, cx| {
                    app.set_processor_power_slider_value(
                        slider,
                        apply_u64_step(value, change, 0, 100),
                    );
                    cx.notify();
                }),
            ));
        }
        controls.push(
            setting_group_action_row(
                boost_mode_id,
                t!("processor_power.boost_mode").to_string(),
                self.render_processor_boost_mode_picker(source, window, cx),
                false,
            )
            .into_any_element(),
        );

        setting_group(
            target,
            title,
            self.render_processor_power_preset_picker(source, window, cx),
            self.is_setting_group_collapsed(target),
            controls,
            window,
            cx,
        )
        .into_any_element()
    }

    fn render_processor_power_preset_picker(
        &self,
        source: ProcessorPowerSource,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let current = match source {
            ProcessorPowerSource::Ac => self.processor_power_values().ac,
            ProcessorPowerSource::Battery => self.processor_power_values().battery,
        };
        let mut presets = processor_power_builtin_presets()
            .into_iter()
            .map(|preset| {
                (
                    processor_power_preset_label(preset),
                    ProcessorPowerValues::for_preset(preset),
                )
            })
            .collect::<Vec<_>>();
        let custom_start = presets.len();
        presets.extend(
            self.settings
                .advanced_power_plan_tuning_presets
                .iter()
                .map(|preset| {
                    (
                        advanced_power_plan_tuning_preset_label(&preset.name),
                        preset.values.normalized(),
                    )
                }),
        );
        let selected_index = (custom_start..presets.len())
            .find(|index| presets[*index].1 == current)
            .or_else(|| (0..custom_start).find(|index| presets[*index].1 == current));
        let selected = selected_index
            .and_then(|index| presets.get(index))
            .map(|(label, _)| label.clone())
            .unwrap_or_else(|| t!("common.custom").to_string());
        let picker_id = format!("processor-power-{source:?}-preset-picker");

        self.render_dropdown_select(
            picker_id.clone(),
            selected,
            true,
            DropdownSelectWidth::Wide,
            presets.len(),
            window,
            cx,
            move |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for (index, (label, values)) in presets.into_iter().enumerate() {
                    let status_label = label.clone();
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!("{picker_id}-option-{index}")),
                            label,
                            selected_index == Some(index),
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            app.load_processor_power_source_preset(
                                source,
                                status_label.clone(),
                                values,
                            );
                            app.active_power_plan_picker = None;
                            cx.stop_propagation();
                            cx.notify();
                        })),
                    );
                }
                options
            },
        )
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
            ProcessorPowerSource::Battery => self.processor_power_battery_boost_mode,
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

const fn processor_power_builtin_presets() -> [ProcessorPowerPreset; 3] {
    [
        ProcessorPowerPreset::Performance,
        ProcessorPowerPreset::Balanced,
        ProcessorPowerPreset::Saver,
    ]
}

fn advanced_power_plan_tuning_preset_values(
    preset: &AdvancedPowerPlanTuningPreset,
) -> ProcessorPowerValues {
    preset.values.normalized()
}

fn advanced_power_plan_tuning_preset_label(name: &str) -> String {
    let name = name.trim();
    if name.is_empty() {
        t!("processor_power.unnamed_preset").to_string()
    } else {
        name.to_owned()
    }
}

const ADVANCED_POWER_PLAN_TUNING_PRESET_FIELDS: [AdaptiveEngineProcessorPowerPolicyField; 4] = [
    AdaptiveEngineProcessorPowerPolicyField::CoreParkingMin,
    AdaptiveEngineProcessorPowerPolicyField::PerformanceMin,
    AdaptiveEngineProcessorPowerPolicyField::PerformanceMax,
    AdaptiveEngineProcessorPowerPolicyField::BoostPolicy,
];

fn advanced_power_plan_tuning_preset_field_value(
    values: ProcessorPowerValues,
    field: AdaptiveEngineProcessorPowerPolicyField,
) -> u64 {
    u64::from(match field {
        AdaptiveEngineProcessorPowerPolicyField::CoreParkingMin => values.core_parking_min,
        AdaptiveEngineProcessorPowerPolicyField::PerformanceMin => values.performance_min,
        AdaptiveEngineProcessorPowerPolicyField::PerformanceMax => values.performance_max,
        AdaptiveEngineProcessorPowerPolicyField::BoostPolicy => values.boost_policy,
    })
}

fn set_advanced_power_plan_tuning_preset_field_value(
    values: &mut ProcessorPowerValues,
    field: AdaptiveEngineProcessorPowerPolicyField,
    value: u32,
) {
    match field {
        AdaptiveEngineProcessorPowerPolicyField::CoreParkingMin => values.core_parking_min = value,
        AdaptiveEngineProcessorPowerPolicyField::PerformanceMin => values.performance_min = value,
        AdaptiveEngineProcessorPowerPolicyField::PerformanceMax => values.performance_max = value,
        AdaptiveEngineProcessorPowerPolicyField::BoostPolicy => values.boost_policy = value,
    }
}

fn advanced_power_plan_tuning_preset_name_exists(
    presets: &[AdvancedPowerPlanTuningPreset],
    editing_index: Option<usize>,
    name: &str,
) -> bool {
    let name = name.trim();
    !name.is_empty()
        && presets.iter().enumerate().any(|(index, preset)| {
            Some(index) != editing_index && preset.name.trim().eq_ignore_ascii_case(name)
        })
}

fn upsert_advanced_power_plan_tuning_preset(
    presets: &mut Vec<AdvancedPowerPlanTuningPreset>,
    editing_index: Option<usize>,
    name: &str,
    values: ProcessorPowerValues,
) -> bool {
    let name = name.trim();
    if name.is_empty()
        || advanced_power_plan_tuning_preset_name_exists(presets, editing_index, name)
    {
        return false;
    }

    let values = values.normalized();
    let preset = AdvancedPowerPlanTuningPreset {
        name: name.to_owned(),
        values,
    };
    match editing_index {
        Some(index) => {
            let Some(existing) = presets.get_mut(index) else {
                return false;
            };
            *existing = preset;
        }
        None => presets.push(preset),
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_processor_power_presets_store_one_reusable_value_set() {
        let values = ProcessorPowerValues::for_preset(ProcessorPowerPreset::Performance);
        let mut presets = Vec::new();

        assert!(upsert_advanced_power_plan_tuning_preset(
            &mut presets,
            None,
            "  Performance custom  ",
            values,
        ));
        assert_eq!(presets[0].name, "Performance custom");
        assert_eq!(presets[0].values, values);
        assert!(!upsert_advanced_power_plan_tuning_preset(
            &mut presets,
            None,
            "performance CUSTOM",
            values,
        ));
    }
}
