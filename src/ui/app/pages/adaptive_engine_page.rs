use crate::ui::app::*;

struct AdaptiveEngineTuningMut<'a> {
    processor_power_policy_enabled: &'a mut bool,
    base_processor_policy: &'a mut ProcessorPowerValues,
    background_pressure_profile: &'a mut AdaptivePowerBoostValues,
    focus_and_launch_profile: &'a mut AdaptivePowerBoostValues,
    cpu_scheduler: &'a mut CpuSchedulerSettings,
}

impl<'a> AdaptiveEngineTuningMut<'a> {
    fn live(settings: &'a mut Settings) -> Self {
        Self {
            processor_power_policy_enabled: &mut settings
                .adaptive_engine
                .processor_power_policy_enabled,
            base_processor_policy: &mut settings.adaptive_engine.base_processor_policy,
            background_pressure_profile: &mut settings.adaptive_engine.background_pressure_profile,
            focus_and_launch_profile: &mut settings.adaptive_engine.focus_and_launch_profile,
            cpu_scheduler: &mut settings.cpu_scheduler,
        }
    }

    fn preset(preset: &'a mut AdaptiveEnginePreset) -> Self {
        Self {
            processor_power_policy_enabled: &mut preset.processor_power_policy_enabled,
            base_processor_policy: &mut preset.base_processor_policy,
            background_pressure_profile: &mut preset.background_pressure_profile,
            focus_and_launch_profile: &mut preset.focus_and_launch_profile,
            cpu_scheduler: &mut preset.cpu_scheduler,
        }
    }
}

impl WinderustApp {
    pub(in crate::ui::app) fn render_adaptive_engine_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let enabled = adaptive_engine_enabled(&self.settings);
        let body =
            self.render_adaptive_engine_tuning(AdaptiveEngineTuningTarget::Live, true, window, cx);

        let help = tooltip_lines(vec![
            t!("adaptive_engine.intro_1").to_string(),
            t!("adaptive_engine.intro_2").to_string(),
            t!("adaptive_engine.intro_3").to_string(),
        ]);

        self.page_shell(Page::AdaptiveEngine, cx)
            .child(feature_toggle_switch_with_help(
                "adaptive-engine-enabled",
                t!("adaptive_engine.enable").to_string(),
                help,
                enabled,
                cx.listener(|app, checked, _, cx| {
                    apply_adaptive_engine(&mut app.settings, *checked);
                    cx.notify();
                }),
            ))
            .child(self.render_adaptive_engine_preset_selector(window, cx))
            .child(body)
            .into_any_element()
    }

    pub(in crate::ui::app) fn render_adaptive_engine_preset_selector(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected_builtin = BuiltInAdaptiveEnginePreset::ALL
            .iter()
            .copied()
            .find(|preset| matches_built_in_adaptive_engine_preset(&self.settings, *preset));
        let selected_custom = selected_builtin
            .is_none()
            .then(|| {
                self.settings
                    .adaptive_engine_presets
                    .iter()
                    .position(|preset| adaptive_engine_matches_preset(&self.settings, preset))
            })
            .flatten();
        let preset_count =
            BuiltInAdaptiveEnginePreset::ALL.len() + self.settings.adaptive_engine_presets.len();
        let dropdown = self.render_dropdown_select(
            "adaptive-engine-preset",
            selected_builtin
                .map(built_in_adaptive_engine_preset_label)
                .or_else(|| {
                    selected_custom.and_then(|index| {
                        self.settings
                            .adaptive_engine_presets
                            .get(index)
                            .map(|preset| adaptive_engine_preset_label(&preset.name))
                    })
                })
                .unwrap_or_else(|| t!("common.custom").to_string()),
            true,
            DropdownSelectWidth::Wide,
            preset_count,
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for preset in BuiltInAdaptiveEnginePreset::ALL {
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!(
                                "adaptive-engine-power-mode-option-{preset:?}"
                            )),
                            built_in_adaptive_engine_preset_label(preset),
                            selected_builtin == Some(preset),
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            apply_built_in_adaptive_engine_preset(&mut app.settings, preset);
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                for (index, preset) in self.settings.adaptive_engine_presets.iter().enumerate() {
                    let preset = preset.clone();
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!(
                                "adaptive-engine-custom-preset-option-{index}"
                            )),
                            adaptive_engine_preset_label(&preset.name),
                            selected_custom == Some(index),
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            apply_adaptive_engine_preset(&mut app.settings, &preset);
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                options
            },
        );

        setting_action_card_with_help(
            "adaptive-engine-preset",
            t!("adaptive_engine.preset").to_string(),
            t!("adaptive_engine.preset_help").to_string(),
            dropdown,
        )
        .into_any_element()
    }

    pub(in crate::ui::app) fn render_adaptive_engine_side_panel(
        &self,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let status_selected = self.adaptive_engine_side_panel_tab == PresetSidePanelTab::Status;
        let selected_background = cx.theme().secondary_active;
        let hover_background = cx.theme().secondary_hover;
        let header = h_flex()
            .min_h(px(48.0))
            .gap_1()
            .px_3()
            .child(
                div()
                    .id("adaptive-engine-status-tab")
                    .flex_1()
                    .h(px(32.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(BRAND_RADIUS_CONTROL))
                    .text_size(px(TEXT_CONTROL_SIZE))
                    .cursor_pointer()
                    .when(status_selected, |tab| tab.bg(selected_background))
                    .hover(move |style| style.bg(hover_background))
                    .on_click(cx.listener(|app, _, _, cx| {
                        app.adaptive_engine_side_panel_tab = PresetSidePanelTab::Status;
                        cx.notify();
                    }))
                    .child(t!("common.status").to_string()),
            )
            .child(
                div()
                    .id("adaptive-engine-presets-tab")
                    .flex_1()
                    .h(px(32.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(BRAND_RADIUS_CONTROL))
                    .text_size(px(TEXT_CONTROL_SIZE))
                    .cursor_pointer()
                    .when(!status_selected, |tab| tab.bg(selected_background))
                    .hover(move |style| style.bg(hover_background))
                    .on_click(cx.listener(|app, _, _, cx| {
                        app.adaptive_engine_side_panel_tab = PresetSidePanelTab::Presets;
                        cx.notify();
                    }))
                    .child(t!("adaptive_engine.presets").to_string()),
            )
            .into_any_element();
        let body = if status_selected {
            v_flex()
                .flex_1()
                .min_h(px(0.0))
                .overflow_y_scrollbar()
                .p_3()
                .child(
                    self.render_normalized_feature_status(Page::AdaptiveEngine)
                        .expect("Adaptive Engine always has normalized runtime status"),
                )
                .child(self.render_bottleneck_classifier_status())
                .into_any_element()
        } else {
            self.render_adaptive_engine_presets_content(cx)
        };
        let body = animated_tab_content(
            body,
            if status_selected {
                "adaptive-engine-status-content"
            } else {
                "adaptive-engine-presets-content"
            },
            if status_selected { -12.0 } else { 12.0 },
        );
        page_side_panel(header, body)
    }

    fn render_adaptive_engine_presets_content(&self, cx: &mut Context<Self>) -> AnyElement {
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
            text_muted(t!("adaptive_engine.built_in_presets").to_string())
                .px_1()
                .pb_1(),
        );
        for preset in BuiltInAdaptiveEnginePreset::ALL {
            built_in_presets = built_in_presets.child(
                h_flex()
                    .id(SharedString::from(format!(
                        "adaptive-engine-built-in-preset-{preset:?}"
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
                            .child(built_in_adaptive_engine_preset_label(preset)),
                    )
                    .child(
                        control_button(
                            Button::new(SharedString::from(format!(
                                "view-adaptive-engine-built-in-preset-{preset:?}"
                            )))
                            .ghost(),
                        )
                        .with_size(px(28.0))
                        .icon(Icon::new(NavIcon::Info).with_size(px(12.0)))
                        .tooltip(t!("adaptive_engine.view_preset").to_string())
                        .on_click(cx.listener(
                            move |app, _, window, cx| {
                                app.open_adaptive_engine_preset_editor(
                                    AdaptiveEnginePresetEditorTarget::BuiltIn(preset),
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
            text_muted(t!("adaptive_engine.custom_presets").to_string())
                .px_1()
                .pb_1(),
        );
        for (index, preset) in self.settings.adaptive_engine_presets.iter().enumerate() {
            let removal_target =
                ListItemRemovalTarget::new(ListItemRemovalKind::AdaptiveEnginePreset, index);
            let row = h_flex()
                .id(SharedString::from(format!(
                    "adaptive-engine-custom-preset-row-{index}"
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
                        .child(adaptive_engine_preset_label(&preset.name)),
                )
                .child(
                    h_flex()
                        .flex_shrink_0()
                        .gap_1()
                        .child(
                            control_button(
                                Button::new(SharedString::from(format!(
                                    "edit-adaptive-engine-preset-{index}"
                                )))
                                .ghost(),
                            )
                            .with_size(px(28.0))
                            .icon(Icon::new(NavIcon::SquarePen).with_size(px(12.0)))
                            .tooltip(t!("common.edit").to_string())
                            .on_click(cx.listener(
                                move |app, _, window, cx| {
                                    app.open_adaptive_engine_preset_editor(
                                        AdaptiveEnginePresetEditorTarget::Custom(Some(index)),
                                        window,
                                        cx,
                                    );
                                },
                            )),
                        )
                        .child(
                            remove_control_button(Button::new(SharedString::from(format!(
                                "remove-adaptive-engine-preset-{index}"
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
                SharedString::from(format!("adaptive-engine-preset-{index}")),
                row,
            ));
        }
        if self.settings.adaptive_engine_presets.is_empty() {
            custom_presets = custom_presets.child(
                text_muted(t!("adaptive_engine.no_custom_presets").to_string())
                    .px_1()
                    .py_2(),
            );
        }
        presets = presets.child(custom_presets);

        v_flex()
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
                        primary_control_button(Button::new("add-adaptive-engine-preset"), cx)
                            .w_full()
                            .icon(Icon::new(NavIcon::Plus).with_size(px(14.0)))
                            .label(t!("adaptive_engine.add_preset").to_string())
                            .on_click(cx.listener(|app, _, window, cx| {
                                app.open_adaptive_engine_preset_editor(
                                    AdaptiveEnginePresetEditorTarget::Custom(None),
                                    window,
                                    cx,
                                );
                            })),
                    ),
            )
            .into_any_element()
    }

    fn open_adaptive_engine_preset_editor(
        &mut self,
        target: AdaptiveEnginePresetEditorTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let preset = match target {
            AdaptiveEnginePresetEditorTarget::BuiltIn(preset) => {
                built_in_adaptive_engine_preset(&self.settings, preset)
            }
            AdaptiveEnginePresetEditorTarget::Custom(Some(index)) => {
                let Some(preset) = self.settings.adaptive_engine_presets.get(index) else {
                    return;
                };
                preset.clone()
            }
            AdaptiveEnginePresetEditorTarget::Custom(None) => {
                capture_adaptive_engine_preset(&self.settings, String::new())
            }
        };
        let name = preset.name.clone();
        self.adaptive_engine_preset_editor = Some(AdaptiveEnginePresetEditor {
            target,
            preset,
            tuning_tab: AdaptiveEngineTuningTab::default(),
        });
        self.active_power_plan_picker = None;
        if matches!(target, AdaptiveEnginePresetEditorTarget::Custom(_)) {
            clear_input_to(&self.inputs.adaptive_engine_preset_name, &name, window, cx);
            self.inputs
                .adaptive_engine_preset_name
                .read(cx)
                .focus_handle(cx)
                .focus(window);
        }
        cx.notify();
    }

    pub(in crate::ui::app) fn close_adaptive_engine_preset_editor(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        self.adaptive_engine_preset_editor = None;
        self.editing_numeric = None;
        self.active_power_plan_picker = None;
        cx.notify();
    }

    fn update_adaptive_engine_preset_from_current(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = self.adaptive_engine_preset_editor.as_ref() else {
            return;
        };
        if !matches!(editor.target, AdaptiveEnginePresetEditorTarget::Custom(_)) {
            return;
        }
        let name = editor.preset.name.clone();
        let preset = capture_adaptive_engine_preset(&self.settings, name);
        if let Some(editor) = self.adaptive_engine_preset_editor.as_mut() {
            editor.preset = preset;
        }
        cx.notify();
    }

    fn adaptive_engine_processor_tuning(
        &self,
        target: AdaptiveEngineTuningTarget,
    ) -> (bool, ProcessorPowerValues) {
        match target {
            AdaptiveEngineTuningTarget::Live => (
                self.settings.adaptive_engine.processor_power_policy_enabled,
                self.settings
                    .adaptive_engine
                    .base_processor_policy
                    .normalized(),
            ),
            AdaptiveEngineTuningTarget::Preset => {
                let preset = &self
                    .adaptive_engine_preset_editor
                    .as_ref()
                    .expect("preset tuning requires an Adaptive Engine preset editor")
                    .preset;
                (
                    preset.processor_power_policy_enabled,
                    preset.base_processor_policy.normalized(),
                )
            }
        }
    }

    fn adaptive_engine_boost_tuning(
        &self,
        target: AdaptiveEngineTuningTarget,
        profile: AdaptiveEngineProfile,
    ) -> AdaptivePowerBoostValues {
        let (background, focus) = match target {
            AdaptiveEngineTuningTarget::Live => (
                self.settings.adaptive_engine.background_pressure_profile,
                self.settings.adaptive_engine.focus_and_launch_profile,
            ),
            AdaptiveEngineTuningTarget::Preset => {
                let preset = &self
                    .adaptive_engine_preset_editor
                    .as_ref()
                    .expect("preset tuning requires an Adaptive Engine preset editor")
                    .preset;
                (
                    preset.background_pressure_profile,
                    preset.focus_and_launch_profile,
                )
            }
        };
        match profile {
            AdaptiveEngineProfile::BackgroundPressure => background,
            AdaptiveEngineProfile::FocusAndLaunch => focus,
        }
        .normalized()
    }

    fn adaptive_engine_cpu_scheduler_tuning(
        &self,
        target: AdaptiveEngineTuningTarget,
    ) -> &CpuSchedulerSettings {
        match target {
            AdaptiveEngineTuningTarget::Live => &self.settings.cpu_scheduler,
            AdaptiveEngineTuningTarget::Preset => {
                &self
                    .adaptive_engine_preset_editor
                    .as_ref()
                    .expect("preset tuning requires an Adaptive Engine preset editor")
                    .preset
                    .cpu_scheduler
            }
        }
    }

    fn update_adaptive_engine_tuning(
        &mut self,
        target: AdaptiveEngineTuningTarget,
        update: impl FnOnce(AdaptiveEngineTuningMut<'_>),
    ) {
        match target {
            AdaptiveEngineTuningTarget::Live => {
                update(AdaptiveEngineTuningMut::live(&mut self.settings));
            }
            AdaptiveEngineTuningTarget::Preset => {
                if let Some(editor) = self.adaptive_engine_preset_editor.as_mut() {
                    update(AdaptiveEngineTuningMut::preset(&mut editor.preset));
                }
            }
        }
    }

    fn adaptive_engine_tuning_tab(
        &self,
        target: AdaptiveEngineTuningTarget,
    ) -> AdaptiveEngineTuningTab {
        match target {
            AdaptiveEngineTuningTarget::Live => self.adaptive_engine_tuning_tab,
            AdaptiveEngineTuningTarget::Preset => self
                .adaptive_engine_preset_editor
                .as_ref()
                .map_or_else(AdaptiveEngineTuningTab::default, |editor| editor.tuning_tab),
        }
    }

    fn set_adaptive_engine_tuning_tab(
        &mut self,
        target: AdaptiveEngineTuningTarget,
        tab: AdaptiveEngineTuningTab,
    ) {
        match target {
            AdaptiveEngineTuningTarget::Live => self.adaptive_engine_tuning_tab = tab,
            AdaptiveEngineTuningTarget::Preset => {
                if let Some(editor) = self.adaptive_engine_preset_editor.as_mut() {
                    editor.tuning_tab = tab;
                }
            }
        }
    }

    pub(in crate::ui::app) fn adaptive_engine_tuning_numeric_value(
        &self,
        target: AdaptiveEngineTuningTarget,
        field: AdaptiveEngineTuningNumericField,
    ) -> u64 {
        match field {
            AdaptiveEngineTuningNumericField::ProcessorPowerPolicy(field) => {
                let (_, values) = self.adaptive_engine_processor_tuning(target);
                u64::from(match field {
                    AdaptiveEngineProcessorPowerPolicyField::CoreParkingMin => {
                        values.core_parking_min
                    }
                    AdaptiveEngineProcessorPowerPolicyField::PerformanceMin => {
                        values.performance_min
                    }
                    AdaptiveEngineProcessorPowerPolicyField::PerformanceMax => {
                        values.performance_max
                    }
                    AdaptiveEngineProcessorPowerPolicyField::BoostPolicy => values.boost_policy,
                })
            }
            AdaptiveEngineTuningNumericField::ProfileBoostPolicy(profile, source) => {
                let values = self.adaptive_engine_boost_tuning(target, profile);
                u64::from(match source {
                    ProcessorPowerSource::Ac => values.ac_policy,
                    ProcessorPowerSource::Battery => values.battery_policy,
                })
            }
            AdaptiveEngineTuningNumericField::ProcessorLimit => u64::from(
                self.adaptive_engine_cpu_scheduler_tuning(target)
                    .processor_limit_percent,
            ),
            AdaptiveEngineTuningNumericField::ForegroundOrSystemCpuThreshold => u64::from(
                self.adaptive_engine_cpu_scheduler_tuning(target)
                    .foreground_or_system_cpu_threshold_percent,
            ),
            AdaptiveEngineTuningNumericField::BackgroundAppCpuThreshold => u64::from(
                self.adaptive_engine_cpu_scheduler_tuning(target)
                    .background_app_cpu_threshold_percent,
            ),
            AdaptiveEngineTuningNumericField::CpuRecoveryThreshold => u64::from(
                self.adaptive_engine_cpu_scheduler_tuning(target)
                    .cpu_recovery_threshold_percent,
            ),
            AdaptiveEngineTuningNumericField::MaximumRestrainedApps => u64::from(
                self.adaptive_engine_cpu_scheduler_tuning(target)
                    .maximum_restrained_apps,
            ),
            AdaptiveEngineTuningNumericField::ReactionTime => {
                self.adaptive_engine_cpu_scheduler_tuning(target)
                    .reaction_time_ms
            }
            AdaptiveEngineTuningNumericField::CpuRestraintTime => {
                self.adaptive_engine_cpu_scheduler_tuning(target)
                    .cpu_restraint_time_seconds
            }
            AdaptiveEngineTuningNumericField::CpuRecoveryTime => {
                self.adaptive_engine_cpu_scheduler_tuning(target)
                    .cpu_recovery_time_seconds
            }
        }
    }

    pub(in crate::ui::app) fn set_adaptive_engine_tuning_numeric_value(
        &mut self,
        target: AdaptiveEngineTuningTarget,
        field: AdaptiveEngineTuningNumericField,
        value: u64,
    ) {
        self.update_adaptive_engine_tuning(target, |tuning| match field {
            AdaptiveEngineTuningNumericField::ProcessorPowerPolicy(field) => {
                let mut values = tuning.base_processor_policy.normalized();
                let value = value.min(100) as u32;
                match field {
                    AdaptiveEngineProcessorPowerPolicyField::CoreParkingMin => {
                        values.core_parking_min = value;
                    }
                    AdaptiveEngineProcessorPowerPolicyField::PerformanceMin => {
                        values.performance_min = value;
                    }
                    AdaptiveEngineProcessorPowerPolicyField::PerformanceMax => {
                        values.performance_max = value;
                    }
                    AdaptiveEngineProcessorPowerPolicyField::BoostPolicy => {
                        values.boost_policy = value;
                    }
                }
                *tuning.base_processor_policy = values.normalized();
            }
            AdaptiveEngineTuningNumericField::ProfileBoostPolicy(profile, source) => {
                let values = match profile {
                    AdaptiveEngineProfile::BackgroundPressure => tuning.background_pressure_profile,
                    AdaptiveEngineProfile::FocusAndLaunch => tuning.focus_and_launch_profile,
                };
                match source {
                    ProcessorPowerSource::Ac => values.ac_policy = value.min(100) as u32,
                    ProcessorPowerSource::Battery => values.battery_policy = value.min(100) as u32,
                }
                *values = values.normalized();
            }
            AdaptiveEngineTuningNumericField::ProcessorLimit => {
                tuning.cpu_scheduler.processor_limit_percent = value.clamp(
                    CPU_SCHEDULER_THRESHOLD_MIN_PERCENT,
                    CPU_SCHEDULER_THRESHOLD_MAX_PERCENT,
                ) as u8;
            }
            AdaptiveEngineTuningNumericField::ForegroundOrSystemCpuThreshold => {
                tuning
                    .cpu_scheduler
                    .foreground_or_system_cpu_threshold_percent = value.clamp(
                    CPU_SCHEDULER_THRESHOLD_MIN_PERCENT,
                    CPU_SCHEDULER_THRESHOLD_MAX_PERCENT,
                ) as u8;
            }
            AdaptiveEngineTuningNumericField::BackgroundAppCpuThreshold => {
                tuning.cpu_scheduler.background_app_cpu_threshold_percent = value.clamp(
                    CPU_SCHEDULER_THRESHOLD_MIN_PERCENT,
                    CPU_SCHEDULER_THRESHOLD_MAX_PERCENT,
                ) as u8;
            }
            AdaptiveEngineTuningNumericField::CpuRecoveryThreshold => {
                tuning.cpu_scheduler.cpu_recovery_threshold_percent = value.clamp(
                    CPU_SCHEDULER_THRESHOLD_MIN_PERCENT,
                    CPU_SCHEDULER_THRESHOLD_MAX_PERCENT,
                ) as u8;
            }
            AdaptiveEngineTuningNumericField::MaximumRestrainedApps => {
                tuning.cpu_scheduler.maximum_restrained_apps = value.clamp(
                    CPU_SCHEDULER_TARGET_LIMIT_MIN,
                    CPU_SCHEDULER_TARGET_LIMIT_MAX,
                ) as u8;
            }
            AdaptiveEngineTuningNumericField::ReactionTime => {
                tuning.cpu_scheduler.reaction_time_ms = value.clamp(
                    CPU_SCHEDULER_REACTION_INTERVAL_MIN_MS,
                    CPU_SCHEDULER_REACTION_INTERVAL_MAX_MS,
                );
            }
            AdaptiveEngineTuningNumericField::CpuRestraintTime => {
                tuning.cpu_scheduler.cpu_restraint_time_seconds =
                    value.clamp(CPU_SCHEDULER_SECONDS_MIN, CPU_SCHEDULER_SECONDS_MAX);
            }
            AdaptiveEngineTuningNumericField::CpuRecoveryTime => {
                tuning.cpu_scheduler.cpu_recovery_time_seconds =
                    value.clamp(CPU_SCHEDULER_SECONDS_MIN, CPU_SCHEDULER_SECONDS_MAX);
            }
        });
    }

    pub(in crate::ui::app) fn apply_adaptive_engine_tuning_numeric_input(
        &mut self,
        target: AdaptiveEngineTuningTarget,
        field: AdaptiveEngineTuningNumericField,
        value: &str,
    ) {
        let (minimum, maximum) = match field {
            AdaptiveEngineTuningNumericField::ProcessorPowerPolicy(_)
            | AdaptiveEngineTuningNumericField::ProfileBoostPolicy(_, _) => (0, 100),
            AdaptiveEngineTuningNumericField::ProcessorLimit
            | AdaptiveEngineTuningNumericField::ForegroundOrSystemCpuThreshold
            | AdaptiveEngineTuningNumericField::BackgroundAppCpuThreshold
            | AdaptiveEngineTuningNumericField::CpuRecoveryThreshold => (
                CPU_SCHEDULER_THRESHOLD_MIN_PERCENT,
                CPU_SCHEDULER_THRESHOLD_MAX_PERCENT,
            ),
            AdaptiveEngineTuningNumericField::MaximumRestrainedApps => (
                CPU_SCHEDULER_TARGET_LIMIT_MIN,
                CPU_SCHEDULER_TARGET_LIMIT_MAX,
            ),
            AdaptiveEngineTuningNumericField::ReactionTime => (
                CPU_SCHEDULER_REACTION_INTERVAL_MIN_MS,
                CPU_SCHEDULER_REACTION_INTERVAL_MAX_MS,
            ),
            AdaptiveEngineTuningNumericField::CpuRestraintTime
            | AdaptiveEngineTuningNumericField::CpuRecoveryTime => {
                (CPU_SCHEDULER_SECONDS_MIN, CPU_SCHEDULER_SECONDS_MAX)
            }
        };
        if let Some(value) = parse_u64_input(value, minimum, maximum) {
            self.set_adaptive_engine_tuning_numeric_value(target, field, value);
        }
    }

    fn save_adaptive_engine_preset(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = self.adaptive_engine_preset_editor.as_ref() else {
            return;
        };
        let AdaptiveEnginePresetEditorTarget::Custom(index) = editor.target else {
            return;
        };
        let name = self
            .inputs
            .adaptive_engine_preset_name
            .read(cx)
            .value()
            .to_string();
        let saved = upsert_adaptive_engine_preset(
            &mut self.settings.adaptive_engine_presets,
            index,
            &name,
            editor.preset.clone(),
        );
        if saved {
            self.close_adaptive_engine_preset_editor(cx);
        }
    }

    fn render_adaptive_engine_tuning(
        &self,
        target: AdaptiveEngineTuningTarget,
        editable: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected_tab = self.adaptive_engine_tuning_tab(target);
        let target_key = adaptive_engine_tuning_target_key(target);
        let selected_background = cx.theme().secondary_active;
        let hover_background = cx.theme().secondary_hover;
        let mut tabs = h_flex()
            .w_full()
            .min_w(px(0.0))
            .gap_1()
            .p_1()
            .rounded(px(BRAND_RADIUS_SURFACE))
            .bg(rgb(settings_card_color()));
        let tuning_tabs: &[AdaptiveEngineTuningTab] = match target {
            AdaptiveEngineTuningTarget::Live => &AdaptiveEngineTuningTab::LIVE,
            AdaptiveEngineTuningTarget::Preset => &AdaptiveEngineTuningTab::PRESET,
        };
        for tab in tuning_tabs.iter().copied() {
            let selected = selected_tab == tab;
            tabs = tabs.child(
                div()
                    .id(SharedString::from(format!(
                        "adaptive-engine-{target_key}-{tab:?}-tab"
                    )))
                    .flex_1()
                    .min_w(px(0.0))
                    .h(px(32.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(BRAND_RADIUS_CONTROL))
                    .text_size(px(TEXT_CONTROL_SIZE))
                    .truncate()
                    .cursor_pointer()
                    .when(selected, |item| item.bg(selected_background))
                    .hover(move |style| style.bg(hover_background))
                    .on_click(cx.listener(move |app, _, _, cx| {
                        app.set_adaptive_engine_tuning_tab(target, tab);
                        app.active_power_plan_picker = None;
                        cx.notify();
                    }))
                    .child(adaptive_engine_tuning_tab_label(tab)),
            );
        }

        let content = match selected_tab {
            AdaptiveEngineTuningTab::CpuBehaviour => feature_body(true)
                .child(self.render_cpu_scheduler_cpu_behaviour_groups(target, window, cx))
                .into_any_element(),
            AdaptiveEngineTuningTab::ProcessorPower => {
                self.render_adaptive_engine_processor_power_policy_cards(target, window, cx)
            }
            AdaptiveEngineTuningTab::PriorityControl => feature_body(true)
                .child(self.render_cpu_scheduler_priority_table(target, window, cx))
                .into_any_element(),
            AdaptiveEngineTuningTab::CustomRules => {
                assert_eq!(
                    target,
                    AdaptiveEngineTuningTarget::Live,
                    "Adaptive Engine Custom Rules are live-only"
                );
                feature_body(true)
                    .child(self.render_custom_rules_section(window, cx))
                    .into_any_element()
            }
        };
        let controls_enabled = editable;
        let body = feature_body(controls_enabled).child(content);

        v_flex()
            .id(SharedString::from(format!(
                "adaptive-engine-{target_key}-tuning"
            )))
            .w_full()
            .min_w(px(0.0))
            .gap_2()
            .child(tabs)
            .child(disabled_feature_body(
                SharedString::from(format!("adaptive-engine-{target_key}-tuning-content")),
                body,
                controls_enabled,
                cx,
            ))
            .into_any_element()
    }

    pub(in crate::ui::app) fn render_adaptive_engine_preset_modal(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let editor = self
            .adaptive_engine_preset_editor
            .as_ref()
            .expect("Adaptive Engine preset modal requires an editor");
        let input = &self.inputs.adaptive_engine_preset_name;
        let name = input.read(cx).value().to_string();
        let custom_index = match editor.target {
            AdaptiveEnginePresetEditorTarget::BuiltIn(_) => None,
            AdaptiveEnginePresetEditorTarget::Custom(index) => index,
        };
        let edits_custom = matches!(editor.target, AdaptiveEnginePresetEditorTarget::Custom(_));
        let duplicate_name = edits_custom
            && adaptive_engine_preset_name_exists(
                &self.settings.adaptive_engine_presets,
                custom_index,
                &name,
            );
        let can_save = !name.trim().is_empty() && !duplicate_name;
        let name_focused = input.read(cx).focus_handle(cx).is_focused(window);
        let title = match editor.target {
            AdaptiveEnginePresetEditorTarget::BuiltIn(preset) => format!(
                "{}: {}",
                t!("adaptive_engine.view_preset"),
                built_in_adaptive_engine_preset_label(preset)
            ),
            AdaptiveEnginePresetEditorTarget::Custom(Some(_)) => {
                t!("adaptive_engine.edit_preset").to_string()
            }
            AdaptiveEnginePresetEditorTarget::Custom(None) => {
                t!("adaptive_engine.add_preset").to_string()
            }
        };
        let mut content = v_flex().w_full().min_w(px(0.0)).gap_3().p_4();
        if edits_custom {
            let name_help = if duplicate_name {
                text_warning(t!("adaptive_engine.duplicate_preset_name").to_string())
            } else {
                text_muted(t!("adaptive_engine.preset_name_help").to_string())
            };
            content = content.child(
                branded_panel()
                    .child(setting_group_stacked_action_row(
                        "adaptive-engine-preset-name-row",
                        t!("adaptive_engine.preset_name").to_string(),
                        app_input(input, name_focused, cx).into_any_element(),
                        false,
                    ))
                    .child(name_help.px_4().pb_3()),
            );
        }
        content = content.child(self.render_adaptive_engine_tuning(
            AdaptiveEngineTuningTarget::Preset,
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
                    control_button(Button::new("update-adaptive-engine-preset-values"))
                        .label(t!("adaptive_engine.use_current_settings").to_string())
                        .on_click(cx.listener(|app, _, _, cx| {
                            app.update_adaptive_engine_preset_from_current(cx);
                        })),
                )
                .child(
                    control_button(Button::new("cancel-adaptive-engine-preset"))
                        .label(t!("common.cancel").to_string())
                        .on_click(cx.listener(|app, _, _, cx| {
                            app.close_adaptive_engine_preset_editor(cx);
                        })),
                )
                .child(
                    primary_control_button(Button::new("save-adaptive-engine-preset"), cx)
                        .label(t!("common.save").to_string())
                        .disabled(!can_save)
                        .on_click(cx.listener(|app, _, _, cx| {
                            app.save_adaptive_engine_preset(cx);
                        })),
                );
        } else {
            footer = footer.child(
                primary_control_button(Button::new("close-adaptive-engine-preset"), cx)
                    .label(t!("common.done").to_string())
                    .on_click(cx.listener(|app, _, _, cx| {
                        app.close_adaptive_engine_preset_editor(cx);
                    })),
            );
        }

        let modal = v_flex()
            .w_full()
            .max_w(px(960.0))
            .h_full()
            .max_h(px(680.0))
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
                        control_button(Button::new("close-adaptive-engine-preset-modal"))
                            .with_size(px(32.0))
                            .icon(Icon::new(NavIcon::X).with_size(px(14.0)))
                            .on_click(cx.listener(|app, _, _, cx| {
                                app.close_adaptive_engine_preset_editor(cx);
                            })),
                    ),
            )
            .child(
                div()
                    .id("adaptive-engine-preset-scroll")
                    .flex_1()
                    .min_h(px(0.0))
                    .overflow_y_scrollbar()
                    .child(content),
            )
            .child(footer);
        let modal = with_optional_motion(
            modal,
            "adaptive-engine-preset-modal-open",
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
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|app, _, _, cx| {
                    app.close_adaptive_engine_preset_editor(cx);
                }),
            )
            .child(modal);
        with_optional_motion(
            backdrop,
            "adaptive-engine-preset-backdrop-open",
            MotionSpeed::Standard,
            |backdrop| backdrop,
            |backdrop, delta| backdrop.opacity(delta),
        )
        .into_any_element()
    }

    pub(in crate::ui::app) fn render_adaptive_engine_processor_power_policy_cards(
        &self,
        target: AdaptiveEngineTuningTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let (processor_power_policy_enabled, _) = self.adaptive_engine_processor_tuning(target);
        let target_key = adaptive_engine_tuning_target_key(target);
        let controls = feature_body(true)
            .child(section_header(
                t!("adaptive_engine.base_processor_policy").as_ref(),
                t!("adaptive_engine.base_processor_policy_help").to_string(),
            ))
            .child(self.render_adaptive_engine_processor_power_policy_card(
                target,
                AdaptiveEngineProcessorPowerPolicyField::CoreParkingMin,
                format!("adaptive-engine-{target_key}-processor-policy-core-parking-min"),
                t!("processor_power.core_parking_min").to_string(),
                t!("adaptive_engine.core_parking_min_help").to_string(),
                cx,
            ))
            .child(self.render_adaptive_engine_processor_power_policy_card(
                target,
                AdaptiveEngineProcessorPowerPolicyField::PerformanceMin,
                format!("adaptive-engine-{target_key}-processor-policy-performance-min"),
                t!("processor_power.processor_min").to_string(),
                t!("adaptive_engine.processor_min_help").to_string(),
                cx,
            ))
            .child(self.render_adaptive_engine_processor_power_policy_card(
                target,
                AdaptiveEngineProcessorPowerPolicyField::PerformanceMax,
                format!("adaptive-engine-{target_key}-processor-policy-performance-max"),
                t!("processor_power.processor_max").to_string(),
                t!("adaptive_engine.processor_max_help").to_string(),
                cx,
            ))
            .child(self.render_adaptive_engine_processor_power_policy_card(
                target,
                AdaptiveEngineProcessorPowerPolicyField::BoostPolicy,
                format!("adaptive-engine-{target_key}-processor-policy-boost-policy"),
                t!("processor_power.boost_policy").to_string(),
                t!("adaptive_engine.base_boost_policy_help").to_string(),
                cx,
            ))
            .child(setting_action_card_with_help(
                format!("adaptive-engine-{target_key}-processor-policy-boost-mode"),
                t!("processor_power.boost_mode").to_string(),
                t!("adaptive_engine.base_boost_mode_help").to_string(),
                self.render_adaptive_engine_processor_boost_mode_picker(
                    target,
                    AdaptiveEngineBoostModeField::Base,
                    window,
                    cx,
                ),
            ))
            .children(self.render_adaptive_engine_boost_profile_cards(
                target,
                AdaptiveEngineProfile::BackgroundPressure,
                window,
                cx,
            ))
            .children(self.render_adaptive_engine_boost_profile_cards(
                target,
                AdaptiveEngineProfile::FocusAndLaunch,
                window,
                cx,
            ));

        feature_body(true)
            .child(setting_action_card_with_help(
                format!("adaptive-engine-{target_key}-processor-policy"),
                t!("adaptive_engine.processor_power_policy").to_string(),
                t!("adaptive_engine.processor_power_policy_help").to_string(),
                switch_toggle_action(
                    format!("adaptive-engine-{target_key}-processor-policy-toggle"),
                    processor_power_policy_enabled,
                    cx.listener(move |app, checked, _, cx| {
                        app.update_adaptive_engine_tuning(target, |tuning| {
                            *tuning.processor_power_policy_enabled = *checked;
                        });
                        cx.notify();
                    }),
                ),
            ))
            .child(disabled_feature_body(
                format!("adaptive-engine-{target_key}-processor-policy-controls"),
                controls,
                processor_power_policy_enabled,
                cx,
            ))
            .into_any_element()
    }

    pub(in crate::ui::app) fn render_adaptive_engine_processor_power_policy_card(
        &self,
        target: AdaptiveEngineTuningTarget,
        field: AdaptiveEngineProcessorPowerPolicyField,
        id: String,
        title: String,
        help: String,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let numeric_field = AdaptiveEngineTuningNumericField::ProcessorPowerPolicy(field);
        let value = self.adaptive_engine_tuning_numeric_value(target, numeric_field);
        setting_stepper_card_u64_with_help(
            id,
            title,
            help,
            value,
            self.render_numeric_value(
                NumericField::AdaptiveEngineTuning(target, numeric_field),
                format!("{value}%"),
                value.to_string(),
                cx,
            ),
            cx.listener(move |app, change: &StepChange<u64>, _, cx| {
                let current = app.adaptive_engine_tuning_numeric_value(target, numeric_field);
                let value = apply_u64_step(current, change, 0, 100);
                app.set_adaptive_engine_tuning_numeric_value(target, numeric_field, value);
                cx.notify();
            }),
        )
        .into_any_element()
    }

    fn render_adaptive_engine_boost_profile_cards(
        &self,
        target: AdaptiveEngineTuningTarget,
        profile: AdaptiveEngineProfile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let (title, help) = match profile {
            AdaptiveEngineProfile::BackgroundPressure => (
                t!("adaptive_engine.background_pressure_profile").to_string(),
                t!("adaptive_engine.background_pressure_profile_help").to_string(),
            ),
            AdaptiveEngineProfile::FocusAndLaunch => (
                t!("adaptive_engine.focus_and_launch_profile").to_string(),
                t!("adaptive_engine.focus_and_launch_profile_help").to_string(),
            ),
        };

        let mut cards = vec![section_header(&title, help).into_any_element()];
        for source in [ProcessorPowerSource::Ac, ProcessorPowerSource::Battery] {
            let source_key = match source {
                ProcessorPowerSource::Ac => "ac",
                ProcessorPowerSource::Battery => "battery",
            };
            let policy_title = match source {
                ProcessorPowerSource::Ac => t!("adaptive_engine.ac_boost_policy").to_string(),
                ProcessorPowerSource::Battery => {
                    t!("adaptive_engine.battery_boost_policy").to_string()
                }
            };
            let mode_title = match source {
                ProcessorPowerSource::Ac => t!("adaptive_engine.ac_boost_mode").to_string(),
                ProcessorPowerSource::Battery => {
                    t!("adaptive_engine.battery_boost_mode").to_string()
                }
            };
            let numeric_field =
                AdaptiveEngineTuningNumericField::ProfileBoostPolicy(profile, source);
            let value = self.adaptive_engine_tuning_numeric_value(target, numeric_field);
            let id_prefix = format!(
                "adaptive-engine-{}-{}-{source_key}",
                adaptive_engine_tuning_target_key(target),
                profile.key(),
            );
            cards.push(
                setting_stepper_card_u64_with_help(
                    format!("{id_prefix}-boost-policy"),
                    policy_title,
                    t!("adaptive_engine.profile_boost_policy_help").to_string(),
                    value,
                    self.render_numeric_value(
                        NumericField::AdaptiveEngineTuning(target, numeric_field),
                        format!("{value}%"),
                        value.to_string(),
                        cx,
                    ),
                    cx.listener(move |app, change: &StepChange<u64>, _, cx| {
                        let current =
                            app.adaptive_engine_tuning_numeric_value(target, numeric_field);
                        app.set_adaptive_engine_tuning_numeric_value(
                            target,
                            numeric_field,
                            apply_u64_step(current, change, 0, 100),
                        );
                        cx.notify();
                    }),
                )
                .into_any_element(),
            );
            cards.push(
                setting_action_card_with_help(
                    format!("{id_prefix}-boost-mode"),
                    mode_title,
                    t!("adaptive_engine.profile_boost_mode_help").to_string(),
                    self.render_adaptive_engine_processor_boost_mode_picker(
                        target,
                        AdaptiveEngineBoostModeField::Profile(profile, source),
                        window,
                        cx,
                    ),
                )
                .into_any_element(),
            );
        }
        cards
    }

    pub(in crate::ui::app) fn render_adaptive_engine_processor_boost_mode_picker(
        &self,
        target: AdaptiveEngineTuningTarget,
        field: AdaptiveEngineBoostModeField,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let picker_id = format!(
            "adaptive-engine-{}-{}-boost-mode-picker",
            adaptive_engine_tuning_target_key(target),
            field.key(),
        );
        let is_open = self.active_power_plan_picker.as_deref() == Some(picker_id.as_str());
        let placement = self.dropdown_placement(
            picker_id.as_str(),
            dropdown_list_height(ProcessorBoostMode::ALL.len()),
            window,
        );
        let selected = match field {
            AdaptiveEngineBoostModeField::Base => {
                self.adaptive_engine_processor_tuning(target).1.boost_mode
            }
            AdaptiveEngineBoostModeField::Profile(profile, source) => {
                let values = self.adaptive_engine_boost_tuning(target, profile);
                match source {
                    ProcessorPowerSource::Ac => values.ac_mode,
                    ProcessorPowerSource::Battery => values.battery_mode,
                }
            }
        };
        let mut options = dropdown_surface(cx, placement.max_height);
        for boost_mode in ProcessorBoostMode::ALL {
            options = options.child(
                dropdown_option_row(
                    SharedString::from(format!("{picker_id}-option-{boost_mode:?}")),
                    processor_boost_mode_label(boost_mode),
                    selected == boost_mode,
                    cx,
                )
                .on_click(cx.listener(move |app, _: &gpui::ClickEvent, _, cx| {
                    app.update_adaptive_engine_tuning(target, |tuning| match field {
                        AdaptiveEngineBoostModeField::Base => {
                            tuning.base_processor_policy.boost_mode = boost_mode;
                        }
                        AdaptiveEngineBoostModeField::Profile(profile, source) => {
                            let values = match profile {
                                AdaptiveEngineProfile::BackgroundPressure => {
                                    tuning.background_pressure_profile
                                }
                                AdaptiveEngineProfile::FocusAndLaunch => {
                                    tuning.focus_and_launch_profile
                                }
                            };
                            match source {
                                ProcessorPowerSource::Ac => values.ac_mode = boost_mode,
                                ProcessorPowerSource::Battery => values.battery_mode = boost_mode,
                            }
                        }
                    });
                    app.active_power_plan_picker = None;
                    cx.notify();
                })),
            );
        }

        let phase = dropdown_popup_phase(&picker_id, is_open, cx);
        let click_picker_id = picker_id.clone();
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
                        != Some(click_picker_id.as_str()))
                    .then_some(click_picker_id.clone());
                    cx.notify();
                })),
            )
            .child(dropdown_anchor_sensor(
                picker_id.as_str(),
                Rc::clone(&self.dropdown_anchor_bounds),
            ))
            .child(dropdown_popup_or_empty(
                SharedString::from(picker_id.clone()),
                phase,
                placement,
                options,
                cx,
            ))
            .into_any_element()
    }

    fn render_cpu_scheduler_efficiency_mode_picker(
        &self,
        tuning_target: AdaptiveEngineTuningTarget,
        target: PriorityDefaultTarget,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let settings = self.adaptive_engine_cpu_scheduler_tuning(tuning_target);
        let tier = match target {
            PriorityDefaultTarget::Foreground => "foreground",
            PriorityDefaultTarget::VisibleWindow => "visible-window",
            PriorityDefaultTarget::Background => "background",
        };
        let selected = match target {
            PriorityDefaultTarget::Foreground => settings
                .focus_process_background_efficiency_override_enabled
                .then_some(settings.focus_process_background_efficiency_mode),
            PriorityDefaultTarget::VisibleWindow => settings
                .visible_window_background_efficiency_override_enabled
                .then_some(settings.visible_window_background_efficiency_mode),
            PriorityDefaultTarget::Background => Some(settings.background_efficiency_mode),
        };
        let supports_default = !matches!(target, PriorityDefaultTarget::Background);
        self.render_dropdown_select(
            format!(
                "cpu-scheduler-{}-{tier}-efficiency-mode",
                adaptive_engine_tuning_target_key(tuning_target)
            ),
            adaptive_efficiency_tier_label(selected),
            enabled,
            DropdownSelectWidth::Table,
            if supports_default { 3 } else { 2 },
            window,
            cx,
            move |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for mode in [None, Some(true), Some(false)] {
                    if !supports_default && mode.is_none() {
                        continue;
                    }
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!(
                                "cpu-scheduler-{tier}-efficiency-mode-{mode:?}"
                            )),
                            adaptive_efficiency_tier_label(mode),
                            selected == mode,
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            app.update_adaptive_engine_tuning(tuning_target, |tuning| {
                                set_adaptive_efficiency_tier(tuning.cpu_scheduler, target, mode);
                            });
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                options
            },
        )
        .into_any_element()
    }

    pub(in crate::ui::app) fn render_cpu_scheduler_cpu_behaviour_groups(
        &self,
        target: AdaptiveEngineTuningTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let settings = self.adaptive_engine_cpu_scheduler_tuning(target);
        let target_key = adaptive_engine_tuning_target_key(target);
        let (cpu_pressure_group, cpu_allocation_group) =
            adaptive_engine_cpu_behaviour_group_targets(target);
        let pressure_controls_enabled = adaptive_engine_cpu_pressure_controls_enabled(
            target,
            settings.cpu_pressure_restraint_enabled,
        );
        let processors = cpu_allocation::logical_processors();
        let available_processor_mask = cpu_allocation_processors_mask(&processors);
        let mut cpu_allocation_rows = vec![
            setting_group_stepper_row_u64_with_help(
                format!("adaptive-engine-{target_key}-background-app-cpu-threshold"),
                t!("cpu_scheduler.background_app_cpu_threshold").to_string(),
                t!("cpu_scheduler.background_app_cpu_threshold_help").to_string(),
                u64::from(settings.background_app_cpu_threshold_percent),
                self.render_numeric_value(
                    NumericField::AdaptiveEngineTuning(
                        target,
                        AdaptiveEngineTuningNumericField::BackgroundAppCpuThreshold,
                    ),
                    format!("{}%", settings.background_app_cpu_threshold_percent),
                    settings.background_app_cpu_threshold_percent.to_string(),
                    cx,
                ),
                true,
                cx.listener(move |app, change: &StepChange<u64>, _, cx| {
                    let field = AdaptiveEngineTuningNumericField::BackgroundAppCpuThreshold;
                    let current = app.adaptive_engine_tuning_numeric_value(target, field);
                    let value = apply_u64_step(
                        current,
                        change,
                        CPU_SCHEDULER_THRESHOLD_MIN_PERCENT,
                        CPU_SCHEDULER_THRESHOLD_MAX_PERCENT,
                    );
                    app.set_adaptive_engine_tuning_numeric_value(target, field, value);
                    cx.notify();
                }),
            )
            .into_any_element(),
            setting_group_action_row(
                format!("adaptive-engine-{target_key}-background-processor-selection"),
                t!("cpu_scheduler.processor_selection").to_string(),
                self.render_background_processor_selection_selector(target, window, cx),
                true,
            )
            .into_any_element(),
            setting_group_action_row_with_help(
                format!("adaptive-engine-{target_key}-dynamic-resource-zones"),
                t!("cpu_scheduler.dynamic_resource_zones").to_string(),
                t!("cpu_scheduler.dynamic_resource_zones_help").to_string(),
                setting_group_switch_action(
                    format!("adaptive-engine-{target_key}-dynamic-resource-zones-switch"),
                    settings.dynamic_resource_zones_enabled,
                    cx.listener(move |app, checked, _, cx| {
                        app.update_adaptive_engine_tuning(target, |tuning| {
                            tuning.cpu_scheduler.dynamic_resource_zones_enabled = *checked;
                        });
                        cx.notify();
                    }),
                ),
                true,
            )
            .into_any_element(),
        ];
        if !settings.dynamic_resource_zones_enabled {
            cpu_allocation_rows.push(
                setting_group_action_row(
                    format!("adaptive-engine-{target_key}-cpu-allocation-method"),
                    t!("cpu_scheduler.cpu_allocation_method").to_string(),
                    self.render_cpu_allocation_method_selector(target, window, cx),
                    true,
                )
                .into_any_element(),
            );
        }
        if settings.background_processor_selection.is_least_used() {
            let (processor_limit_title, processor_limit_help) =
                if settings.dynamic_resource_zones_enabled {
                    (
                        t!("cpu_scheduler.foreground_zone_share").to_string(),
                        t!("cpu_scheduler.foreground_zone_share_help").to_string(),
                    )
                } else {
                    (
                        t!("cpu_scheduler.processor_limit").to_string(),
                        t!("cpu_scheduler.processor_limit_help").to_string(),
                    )
                };
            cpu_allocation_rows.push(
                setting_group_stepper_row_u64_with_help(
                    format!("adaptive-engine-{target_key}-processor-limit"),
                    processor_limit_title,
                    processor_limit_help,
                    u64::from(settings.processor_limit_percent),
                    self.render_numeric_value(
                        NumericField::AdaptiveEngineTuning(
                            target,
                            AdaptiveEngineTuningNumericField::ProcessorLimit,
                        ),
                        format!("{}%", settings.processor_limit_percent),
                        settings.processor_limit_percent.to_string(),
                        cx,
                    ),
                    true,
                    cx.listener(move |app, change: &StepChange<u64>, _, cx| {
                        let field = AdaptiveEngineTuningNumericField::ProcessorLimit;
                        let current = app.adaptive_engine_tuning_numeric_value(target, field);
                        let value = apply_u64_step(
                            current,
                            change,
                            CPU_SCHEDULER_THRESHOLD_MIN_PERCENT,
                            CPU_SCHEDULER_THRESHOLD_MAX_PERCENT,
                        );
                        app.set_adaptive_engine_tuning_numeric_value(target, field, value);
                        cx.notify();
                    }),
                )
                .into_any_element(),
            );
        }
        if settings.background_processor_selection == BackgroundProcessorSelection::Custom {
            cpu_allocation_rows.push(
                setting_group_stacked_action_row(
                    format!("adaptive-engine-{target_key}-custom-processors"),
                    t!("cpu_scheduler.specific_processors").to_string(),
                    self.render_core_tile_grid(
                        &processors,
                        cpu_allocation::logical_processor_indices_mask(
                            &settings.specific_processors,
                        ) & available_processor_mask,
                        format!("adaptive-engine-{target_key}-custom-processor"),
                        true,
                        move |app, core| {
                            app.update_adaptive_engine_tuning(target, |tuning| {
                                tuning
                                    .cpu_scheduler
                                    .specific_processors
                                    .retain(|processor| {
                                        let index = usize::from(*processor);
                                        index < u64::BITS as usize
                                            && (available_processor_mask & (1_u64 << index)) != 0
                                    });
                                tuning.cpu_scheduler.specific_processors.sort_unstable();
                                tuning.cpu_scheduler.specific_processors.dedup();
                                toggle_specific_processor(
                                    &mut tuning.cpu_scheduler.specific_processors,
                                    core,
                                );
                            });
                        },
                        cx,
                    ),
                    true,
                )
                .into_any_element(),
            );
        }
        let pressure_action = if target == AdaptiveEngineTuningTarget::Live {
            setting_group_switch_action(
                "adaptive-engine-live-cpu-pressure-toggle",
                settings.cpu_pressure_restraint_enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings.cpu_scheduler.cpu_pressure_restraint_enabled = *checked;
                    cx.notify();
                }),
            )
        } else {
            div().into_any_element()
        };
        let cpu_allocation_group = setting_group_with_help_enabled(
            cpu_allocation_group,
            (
                t!("cpu_scheduler.limit_background_processors").to_string(),
                t!("cpu_scheduler.limit_background_processors_help").to_string(),
            ),
            setting_group_switch_action(
                format!("adaptive-engine-{target_key}-limit-background-processors-switch"),
                settings.limit_background_processors_enabled,
                cx.listener(move |app, checked, _, cx| {
                    app.update_adaptive_engine_tuning(target, |tuning| {
                        tuning.cpu_scheduler.limit_background_processors_enabled = *checked;
                    });
                    cx.notify();
                }),
            ),
            settings.limit_background_processors_enabled,
            self.is_setting_group_collapsed(cpu_allocation_group),
            cpu_allocation_rows,
            window,
            cx,
        )
        .into_any_element();
        let pressure_group = setting_group_with_help_enabled(
            cpu_pressure_group,
            (
                t!("adaptive_engine.cpu_pressure").to_string(),
                t!("adaptive_engine.cpu_pressure_help").to_string(),
            ),
            pressure_action,
            pressure_controls_enabled,
            self.is_setting_group_collapsed(cpu_pressure_group),
            vec![
                setting_group_stepper_row_u64_with_help(
                    format!("adaptive-engine-{target_key}-maximum-restrained-apps"),
                    t!("cpu_scheduler.maximum_restrained_apps").to_string(),
                    t!("cpu_scheduler.maximum_restrained_apps_help").to_string(),
                    u64::from(settings.maximum_restrained_apps),
                    self.render_numeric_value(
                        NumericField::AdaptiveEngineTuning(
                            target,
                            AdaptiveEngineTuningNumericField::MaximumRestrainedApps,
                        ),
                        settings.maximum_restrained_apps.to_string(),
                        settings.maximum_restrained_apps.to_string(),
                        cx,
                    ),
                    true,
                    cx.listener(move |app, change: &StepChange<u64>, _, cx| {
                        let field = AdaptiveEngineTuningNumericField::MaximumRestrainedApps;
                        let current = app.adaptive_engine_tuning_numeric_value(target, field);
                        let value = apply_u64_step(
                            current,
                            change,
                            CPU_SCHEDULER_TARGET_LIMIT_MIN,
                            CPU_SCHEDULER_TARGET_LIMIT_MAX,
                        );
                        app.set_adaptive_engine_tuning_numeric_value(target, field, value);
                        cx.notify();
                    }),
                ),
                setting_group_stepper_row_u64_with_help(
                    format!("adaptive-engine-{target_key}-reaction-time"),
                    t!("cpu_scheduler.reaction_time").to_string(),
                    t!("cpu_scheduler.reaction_time_help").to_string(),
                    settings.reaction_time_ms,
                    self.render_numeric_value(
                        NumericField::AdaptiveEngineTuning(
                            target,
                            AdaptiveEngineTuningNumericField::ReactionTime,
                        ),
                        duration_label_ms(settings.reaction_time_ms),
                        settings.reaction_time_ms.to_string(),
                        cx,
                    ),
                    true,
                    cx.listener(move |app, change: &StepChange<u64>, _, cx| {
                        let field = AdaptiveEngineTuningNumericField::ReactionTime;
                        let current = app.adaptive_engine_tuning_numeric_value(target, field);
                        let value = apply_u64_step(
                            current,
                            change,
                            CPU_SCHEDULER_REACTION_INTERVAL_MIN_MS,
                            CPU_SCHEDULER_REACTION_INTERVAL_MAX_MS,
                        );
                        app.set_adaptive_engine_tuning_numeric_value(target, field, value);
                        cx.notify();
                    }),
                ),
                setting_group_stepper_row_u64_with_help(
                    format!("adaptive-engine-{target_key}-foreground-or-system-cpu-threshold"),
                    t!("cpu_scheduler.foreground_or_system_cpu_threshold").to_string(),
                    t!("cpu_scheduler.foreground_or_system_cpu_threshold_help").to_string(),
                    u64::from(settings.foreground_or_system_cpu_threshold_percent),
                    self.render_numeric_value(
                        NumericField::AdaptiveEngineTuning(
                            target,
                            AdaptiveEngineTuningNumericField::ForegroundOrSystemCpuThreshold,
                        ),
                        format!("{}%", settings.foreground_or_system_cpu_threshold_percent),
                        settings
                            .foreground_or_system_cpu_threshold_percent
                            .to_string(),
                        cx,
                    ),
                    true,
                    cx.listener(move |app, change: &StepChange<u64>, _, cx| {
                        let field =
                            AdaptiveEngineTuningNumericField::ForegroundOrSystemCpuThreshold;
                        let current = app.adaptive_engine_tuning_numeric_value(target, field);
                        let value = apply_u64_step(
                            current,
                            change,
                            CPU_SCHEDULER_THRESHOLD_MIN_PERCENT,
                            CPU_SCHEDULER_THRESHOLD_MAX_PERCENT,
                        );
                        app.set_adaptive_engine_tuning_numeric_value(target, field, value);
                        cx.notify();
                    }),
                ),
                setting_group_stepper_row_u64_with_help(
                    format!("adaptive-engine-{target_key}-cpu-restraint-time"),
                    t!("cpu_scheduler.cpu_restraint_time").to_string(),
                    t!("cpu_scheduler.cpu_restraint_time_help").to_string(),
                    settings.cpu_restraint_time_seconds,
                    self.render_numeric_value(
                        NumericField::AdaptiveEngineTuning(
                            target,
                            AdaptiveEngineTuningNumericField::CpuRestraintTime,
                        ),
                        ui::duration_label(settings.cpu_restraint_time_seconds),
                        settings.cpu_restraint_time_seconds.to_string(),
                        cx,
                    ),
                    true,
                    cx.listener(move |app, change: &StepChange<u64>, _, cx| {
                        let field = AdaptiveEngineTuningNumericField::CpuRestraintTime;
                        let current = app.adaptive_engine_tuning_numeric_value(target, field);
                        let value = apply_u64_step(
                            current,
                            change,
                            CPU_SCHEDULER_SECONDS_MIN,
                            CPU_SCHEDULER_SECONDS_MAX,
                        );
                        app.set_adaptive_engine_tuning_numeric_value(target, field, value);
                        cx.notify();
                    }),
                ),
                setting_group_stepper_row_u64_with_help(
                    format!("adaptive-engine-{target_key}-cpu-recovery-threshold"),
                    t!("cpu_scheduler.cpu_recovery_threshold").to_string(),
                    t!("cpu_scheduler.cpu_recovery_threshold_help").to_string(),
                    u64::from(settings.cpu_recovery_threshold_percent),
                    self.render_numeric_value(
                        NumericField::AdaptiveEngineTuning(
                            target,
                            AdaptiveEngineTuningNumericField::CpuRecoveryThreshold,
                        ),
                        format!("{}%", settings.cpu_recovery_threshold_percent),
                        settings.cpu_recovery_threshold_percent.to_string(),
                        cx,
                    ),
                    true,
                    cx.listener(move |app, change: &StepChange<u64>, _, cx| {
                        let field = AdaptiveEngineTuningNumericField::CpuRecoveryThreshold;
                        let current = app.adaptive_engine_tuning_numeric_value(target, field);
                        let value = apply_u64_step(
                            current,
                            change,
                            CPU_SCHEDULER_THRESHOLD_MIN_PERCENT,
                            CPU_SCHEDULER_THRESHOLD_MAX_PERCENT,
                        );
                        app.set_adaptive_engine_tuning_numeric_value(target, field, value);
                        cx.notify();
                    }),
                ),
                setting_group_stepper_row_u64_with_help(
                    format!("adaptive-engine-{target_key}-cpu-recovery-time"),
                    t!("cpu_scheduler.cpu_recovery_time").to_string(),
                    t!("cpu_scheduler.cpu_recovery_time_help").to_string(),
                    settings.cpu_recovery_time_seconds,
                    self.render_numeric_value(
                        NumericField::AdaptiveEngineTuning(
                            target,
                            AdaptiveEngineTuningNumericField::CpuRecoveryTime,
                        ),
                        ui::duration_label(settings.cpu_recovery_time_seconds),
                        settings.cpu_recovery_time_seconds.to_string(),
                        cx,
                    ),
                    true,
                    cx.listener(move |app, change: &StepChange<u64>, _, cx| {
                        let field = AdaptiveEngineTuningNumericField::CpuRecoveryTime;
                        let current = app.adaptive_engine_tuning_numeric_value(target, field);
                        let value = apply_u64_step(
                            current,
                            change,
                            CPU_SCHEDULER_SECONDS_MIN,
                            CPU_SCHEDULER_SECONDS_MAX,
                        );
                        app.set_adaptive_engine_tuning_numeric_value(target, field, value);
                        cx.notify();
                    }),
                ),
            ],
            window,
            cx,
        )
        .into_any_element();
        v_flex()
            .gap_2()
            .children([pressure_group, cpu_allocation_group])
            .into_any_element()
    }

    pub(in crate::ui::app) fn render_cpu_scheduler_priority_table(
        &self,
        target: AdaptiveEngineTuningTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let settings = self.adaptive_engine_cpu_scheduler_tuning(target);
        let target_key = adaptive_engine_tuning_target_key(target);
        let mut table = rule_list(vec![
            rule_table_active_header(),
            rule_table_title_header(t!("common.control").to_string()),
            rule_table_centered_header(
                t!("common.focus_process").to_string(),
                DROPDOWN_SELECT_TABLE_WIDTH,
            ),
            rule_table_centered_header(
                t!("common.visible_window").to_string(),
                DROPDOWN_SELECT_TABLE_WIDTH,
            ),
            rule_table_centered_header(
                t!("common.background_process").to_string(),
                DROPDOWN_SELECT_TABLE_WIDTH,
            ),
        ]);

        table = table.child(cpu_scheduler_priority_table_row(
            format!("adaptive-engine-{target_key}-process-priority-row"),
            t!("nav.process_priority").to_string(),
            t!("cpu_scheduler.process_priority_help").to_string(),
            switch_toggle_action(
                format!("adaptive-engine-{target_key}-process-priority-toggle"),
                settings.process_priority_enabled,
                cx.listener(move |app, checked, _, cx| {
                    app.update_adaptive_engine_tuning(target, |tuning| {
                        tuning.cpu_scheduler.process_priority_enabled = *checked;
                    });
                    cx.notify();
                }),
            ),
            self.render_cpu_scheduler_process_priority_selector(
                target,
                PriorityDefaultTarget::Foreground,
                settings.process_priority_enabled,
                window,
                cx,
            ),
            self.render_cpu_scheduler_process_priority_selector(
                target,
                PriorityDefaultTarget::VisibleWindow,
                settings.process_priority_enabled,
                window,
                cx,
            ),
            self.render_cpu_scheduler_process_priority_selector(
                target,
                PriorityDefaultTarget::Background,
                settings.process_priority_enabled,
                window,
                cx,
            ),
        ));

        table = table.child(cpu_scheduler_priority_table_row(
            format!("adaptive-engine-{target_key}-background-efficiency-row"),
            t!("cpu_scheduler.background_efficiency").to_string(),
            t!("cpu_scheduler.background_efficiency_help").to_string(),
            switch_toggle_action(
                format!("adaptive-engine-{target_key}-background-efficiency-toggle"),
                settings.background_efficiency_enabled,
                cx.listener(move |app, checked, _, cx| {
                    app.update_adaptive_engine_tuning(target, |tuning| {
                        tuning.cpu_scheduler.background_efficiency_enabled = *checked;
                    });
                    cx.notify();
                }),
            ),
            self.render_cpu_scheduler_efficiency_mode_picker(
                target,
                PriorityDefaultTarget::Foreground,
                settings.background_efficiency_enabled,
                window,
                cx,
            ),
            self.render_cpu_scheduler_efficiency_mode_picker(
                target,
                PriorityDefaultTarget::VisibleWindow,
                settings.background_efficiency_enabled,
                window,
                cx,
            ),
            self.render_cpu_scheduler_efficiency_mode_picker(
                target,
                PriorityDefaultTarget::Background,
                settings.background_efficiency_enabled,
                window,
                cx,
            ),
        ));

        table = table.child(cpu_scheduler_priority_table_row(
            format!("adaptive-engine-{target_key}-thread-priority-row"),
            t!("nav.thread_priority").to_string(),
            t!("thread_priority.intro_1").to_string(),
            switch_toggle_action(
                format!("adaptive-engine-{target_key}-thread-priority-toggle"),
                settings.thread_priority.enabled,
                cx.listener(move |app, checked, _, cx| {
                    app.update_adaptive_engine_tuning(target, |tuning| {
                        tuning.cpu_scheduler.thread_priority.enabled = *checked;
                    });
                    cx.notify();
                }),
            ),
            self.render_thread_priority_selector(
                target,
                PriorityDefaultTarget::Foreground,
                settings.thread_priority.foreground_priority,
                settings.thread_priority.enabled,
                window,
                cx,
            ),
            self.render_thread_priority_selector(
                target,
                PriorityDefaultTarget::VisibleWindow,
                settings.thread_priority.visible_window_priority,
                settings.thread_priority.enabled,
                window,
                cx,
            ),
            self.render_thread_priority_selector(
                target,
                PriorityDefaultTarget::Background,
                settings.thread_priority.background_priority,
                settings.thread_priority.enabled,
                window,
                cx,
            ),
        ));

        table = table.child(cpu_scheduler_priority_table_row(
            format!("adaptive-engine-{target_key}-dynamic-priority-boost-row"),
            t!("nav.dynamic_priority_boost").to_string(),
            t!("dynamic_priority_boost.intro_1").to_string(),
            switch_toggle_action(
                format!("adaptive-engine-{target_key}-dynamic-priority-boost-toggle"),
                settings.dynamic_priority_boost.enabled,
                cx.listener(move |app, checked, _, cx| {
                    app.update_adaptive_engine_tuning(target, |tuning| {
                        tuning.cpu_scheduler.dynamic_priority_boost.enabled = *checked;
                    });
                    cx.notify();
                }),
            ),
            self.render_dynamic_priority_boost_selector(
                target,
                PriorityDefaultTarget::Foreground,
                settings.dynamic_priority_boost.foreground_boost,
                settings.dynamic_priority_boost.enabled,
                window,
                cx,
            ),
            self.render_dynamic_priority_boost_selector(
                target,
                PriorityDefaultTarget::VisibleWindow,
                settings.dynamic_priority_boost.visible_window_boost,
                settings.dynamic_priority_boost.enabled,
                window,
                cx,
            ),
            self.render_dynamic_priority_boost_selector(
                target,
                PriorityDefaultTarget::Background,
                settings.dynamic_priority_boost.background_boost,
                settings.dynamic_priority_boost.enabled,
                window,
                cx,
            ),
        ));

        table = table.child(cpu_scheduler_priority_table_row(
            format!("adaptive-engine-{target_key}-io-priority-row"),
            t!("nav.io_priority").to_string(),
            t!("cpu_scheduler.io_priority_help").to_string(),
            switch_toggle_action(
                format!("adaptive-engine-{target_key}-io-priority-toggle"),
                settings.io_priority.enabled,
                cx.listener(move |app, checked, _, cx| {
                    app.update_adaptive_engine_tuning(target, |tuning| {
                        tuning.cpu_scheduler.io_priority.enabled = *checked;
                    });
                    cx.notify();
                }),
            ),
            self.render_io_priority_selector(
                target,
                PriorityDefaultTarget::Foreground,
                settings.io_priority.foreground_priority,
                settings.io_priority.enabled,
                window,
                cx,
            ),
            self.render_io_priority_selector(
                target,
                PriorityDefaultTarget::VisibleWindow,
                settings.io_priority.visible_window_priority,
                settings.io_priority.enabled,
                window,
                cx,
            ),
            self.render_io_priority_selector(
                target,
                PriorityDefaultTarget::Background,
                settings.io_priority.background_priority,
                settings.io_priority.enabled,
                window,
                cx,
            ),
        ));

        table = table.child(cpu_scheduler_priority_table_row(
            format!("adaptive-engine-{target_key}-gpu-priority-row"),
            t!("nav.gpu_priority").to_string(),
            t!("gpu_priority.intro_1").to_string(),
            switch_toggle_action(
                format!("adaptive-engine-{target_key}-gpu-priority-toggle"),
                settings.gpu_priority.enabled,
                cx.listener(move |app, checked, _, cx| {
                    app.update_adaptive_engine_tuning(target, |tuning| {
                        tuning.cpu_scheduler.gpu_priority.enabled = *checked;
                    });
                    cx.notify();
                }),
            ),
            self.render_gpu_priority_selector(
                target,
                PriorityDefaultTarget::Foreground,
                settings.gpu_priority.foreground_priority,
                settings.gpu_priority.enabled,
                window,
                cx,
            ),
            self.render_gpu_priority_selector(
                target,
                PriorityDefaultTarget::VisibleWindow,
                settings.gpu_priority.visible_window_priority,
                settings.gpu_priority.enabled,
                window,
                cx,
            ),
            self.render_gpu_priority_selector(
                target,
                PriorityDefaultTarget::Background,
                settings.gpu_priority.background_priority,
                settings.gpu_priority.enabled,
                window,
                cx,
            ),
        ));

        table = table.child(cpu_scheduler_priority_table_row(
            format!("adaptive-engine-{target_key}-memory-priority-row"),
            t!("nav.memory_priority").to_string(),
            t!("cpu_scheduler.memory_priority_help").to_string(),
            switch_toggle_action(
                format!("adaptive-engine-{target_key}-memory-priority-toggle"),
                settings.memory_priority_enabled,
                cx.listener(move |app, checked, _, cx| {
                    app.update_adaptive_engine_tuning(target, |tuning| {
                        tuning.cpu_scheduler.memory_priority_enabled = *checked;
                    });
                    cx.notify();
                }),
            ),
            self.render_cpu_scheduler_memory_priority_selector(
                target,
                PriorityDefaultTarget::Foreground,
                settings.memory_priority_enabled,
                window,
                cx,
            ),
            self.render_cpu_scheduler_memory_priority_selector(
                target,
                PriorityDefaultTarget::VisibleWindow,
                settings.memory_priority_enabled,
                window,
                cx,
            ),
            self.render_cpu_scheduler_memory_priority_selector(
                target,
                PriorityDefaultTarget::Background,
                settings.memory_priority_enabled,
                window,
                cx,
            ),
        ));

        table.into_any_element()
    }

    pub(in crate::ui::app) fn render_io_priority_selector(
        &self,
        tuning_target: AdaptiveEngineTuningTarget,
        target: PriorityDefaultTarget,
        selected_priority: ProcessIoPrioritySetting,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = format!(
            "{}-{}",
            adaptive_engine_tuning_target_key(tuning_target),
            match target {
                PriorityDefaultTarget::Background => "cpu-scheduler-io-background-priority",
                PriorityDefaultTarget::VisibleWindow => "cpu-scheduler-io-visible-window-priority",
                PriorityDefaultTarget::Foreground => "cpu-scheduler-io-foreground-priority",
            }
        );
        let priorities: &[ProcessIoPrioritySetting] =
            if self.settings.advanced.expose_all_priority_values {
                &ProcessIoPrioritySetting::ADVANCED_ALL
            } else {
                &ProcessIoPrioritySetting::ALL
            };
        self.render_dropdown_select(
            &id,
            process_io_priority_setting_label(selected_priority),
            enabled,
            DropdownSelectWidth::Table,
            priorities.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for priority in priorities.iter().copied() {
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!("{id}-option-{priority:?}")),
                            process_io_priority_setting_label(priority),
                            selected_priority == priority,
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            app.update_adaptive_engine_tuning(
                                tuning_target,
                                |tuning| match target {
                                    PriorityDefaultTarget::Background => {
                                        tuning.cpu_scheduler.io_priority.background_priority =
                                            priority
                                    }
                                    PriorityDefaultTarget::VisibleWindow => {
                                        tuning.cpu_scheduler.io_priority.visible_window_priority =
                                            priority
                                    }
                                    PriorityDefaultTarget::Foreground => {
                                        tuning.cpu_scheduler.io_priority.foreground_priority =
                                            priority
                                    }
                                },
                            );
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                options
            },
        )
    }

    pub(in crate::ui::app) fn render_thread_priority_selector(
        &self,
        tuning_target: AdaptiveEngineTuningTarget,
        target: PriorityDefaultTarget,
        selected_priority: ProcessThreadPrioritySetting,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = format!(
            "{}-{}",
            adaptive_engine_tuning_target_key(tuning_target),
            match target {
                PriorityDefaultTarget::Background => "cpu-scheduler-thread-background-priority",
                PriorityDefaultTarget::VisibleWindow => {
                    "cpu-scheduler-thread-visible-window-priority"
                }
                PriorityDefaultTarget::Foreground => "cpu-scheduler-thread-foreground-priority",
            }
        );
        let priorities: &[ProcessThreadPrioritySetting] =
            if self.settings.advanced.expose_all_priority_values {
                &ProcessThreadPrioritySetting::ADVANCED_ALL
            } else {
                &ProcessThreadPrioritySetting::ALL
            };
        self.render_dropdown_select(
            &id,
            process_thread_priority_setting_label(selected_priority),
            enabled,
            DropdownSelectWidth::Table,
            priorities.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for priority in priorities.iter().copied() {
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!("{id}-option-{priority:?}")),
                            process_thread_priority_setting_label(priority),
                            selected_priority == priority,
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            app.update_adaptive_engine_tuning(
                                tuning_target,
                                |tuning| match target {
                                    PriorityDefaultTarget::Background => {
                                        tuning.cpu_scheduler.thread_priority.background_priority =
                                            priority
                                    }
                                    PriorityDefaultTarget::VisibleWindow => {
                                        tuning
                                            .cpu_scheduler
                                            .thread_priority
                                            .visible_window_priority = priority
                                    }
                                    PriorityDefaultTarget::Foreground => {
                                        tuning.cpu_scheduler.thread_priority.foreground_priority =
                                            priority
                                    }
                                },
                            );
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                options
            },
        )
    }

    pub(in crate::ui::app) fn render_dynamic_priority_boost_selector(
        &self,
        tuning_target: AdaptiveEngineTuningTarget,
        target: PriorityDefaultTarget,
        selected_boost: ProcessDynamicPriorityBoostSetting,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = format!(
            "{}-{}",
            adaptive_engine_tuning_target_key(tuning_target),
            match target {
                PriorityDefaultTarget::Background => "cpu-scheduler-boost-background",
                PriorityDefaultTarget::VisibleWindow => "cpu-scheduler-boost-visible-window",
                PriorityDefaultTarget::Foreground => "cpu-scheduler-boost-foreground",
            }
        );
        self.render_dropdown_select(
            &id,
            process_dynamic_priority_boost_setting_label(selected_boost),
            enabled,
            DropdownSelectWidth::Table,
            ProcessDynamicPriorityBoostSetting::ALL.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for boost in ProcessDynamicPriorityBoostSetting::ALL {
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!("{id}-option-{boost:?}")),
                            process_dynamic_priority_boost_setting_label(boost),
                            selected_boost == boost,
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            app.update_adaptive_engine_tuning(
                                tuning_target,
                                |tuning| match target {
                                    PriorityDefaultTarget::Background => {
                                        tuning
                                            .cpu_scheduler
                                            .dynamic_priority_boost
                                            .background_boost = boost
                                    }
                                    PriorityDefaultTarget::VisibleWindow => {
                                        tuning
                                            .cpu_scheduler
                                            .dynamic_priority_boost
                                            .visible_window_boost = boost
                                    }
                                    PriorityDefaultTarget::Foreground => {
                                        tuning
                                            .cpu_scheduler
                                            .dynamic_priority_boost
                                            .foreground_boost = boost
                                    }
                                },
                            );
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                options
            },
        )
    }

    pub(in crate::ui::app) fn render_gpu_priority_selector(
        &self,
        tuning_target: AdaptiveEngineTuningTarget,
        target: PriorityDefaultTarget,
        selected_priority: ProcessGpuPrioritySetting,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = format!(
            "{}-{}",
            adaptive_engine_tuning_target_key(tuning_target),
            match target {
                PriorityDefaultTarget::Background => "cpu-scheduler-gpu-background-priority",
                PriorityDefaultTarget::VisibleWindow => "cpu-scheduler-gpu-visible-window-priority",
                PriorityDefaultTarget::Foreground => "cpu-scheduler-gpu-foreground-priority",
            }
        );
        let priorities: &[ProcessGpuPrioritySetting] =
            if self.settings.advanced.expose_all_priority_values {
                &ProcessGpuPrioritySetting::ADVANCED_ALL
            } else {
                &ProcessGpuPrioritySetting::ALL
            };
        self.render_dropdown_select(
            &id,
            process_gpu_priority_setting_label(selected_priority),
            enabled,
            DropdownSelectWidth::Table,
            priorities.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for priority in priorities.iter().copied() {
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!("{id}-option-{priority:?}")),
                            process_gpu_priority_setting_label(priority),
                            selected_priority == priority,
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            app.update_adaptive_engine_tuning(
                                tuning_target,
                                |tuning| match target {
                                    PriorityDefaultTarget::Background => {
                                        tuning.cpu_scheduler.gpu_priority.background_priority =
                                            priority
                                    }
                                    PriorityDefaultTarget::VisibleWindow => {
                                        tuning.cpu_scheduler.gpu_priority.visible_window_priority =
                                            priority
                                    }
                                    PriorityDefaultTarget::Foreground => {
                                        tuning.cpu_scheduler.gpu_priority.foreground_priority =
                                            priority
                                    }
                                },
                            );
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                options
            },
        )
    }

    fn render_custom_rules_section(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let input_value = self.process_picker_path(
            SuggestionTarget::CpuScheduler,
            &self.inputs.cpu_scheduler_process,
            cx,
        );
        v_flex()
            .w_full()
            .min_w(px(0.0))
            .gap_2()
            .child(
                section_header(
                    &t!("cpu_scheduler.custom_rules"),
                    t!("cpu_scheduler.custom_rules_help").to_string(),
                )
                .into_any_element(),
            )
            .child(
                h_flex()
                    .gap_2()
                    .items_start()
                    .flex_wrap()
                    .child(self.render_process_picker(
                        "cpu-scheduler-exclusion-suggestion",
                        &self.inputs.cpu_scheduler_process,
                        SuggestionTarget::CpuScheduler,
                        window,
                        cx,
                    ))
                    .child(
                        primary_control_button(Button::new("add-cpu-scheduler-exclusion"), cx)
                            .label(t!("common.add").to_string())
                            .disabled(!can_add_cpu_scheduler_custom_rule(
                                &self.settings.cpu_scheduler,
                                &input_value,
                            ))
                            .on_click(cx.listener(|app, _, window, cx| {
                                let process = app.process_picker_path(
                                    SuggestionTarget::CpuScheduler,
                                    &app.inputs.cpu_scheduler_process,
                                    cx,
                                );
                                if can_add_cpu_scheduler_custom_rule(
                                    &app.settings.cpu_scheduler,
                                    &process,
                                ) {
                                    app.settings
                                        .cpu_scheduler
                                        .custom_rules
                                        .push(new_process_exclusion_rule(&process));
                                    clear_input(&app.inputs.cpu_scheduler_process, window, cx);
                                }
                                cx.notify();
                            })),
                    ),
            )
            .child(self.render_process_exclusion_list(
                &self.settings.cpu_scheduler.custom_rules,
                ListItemRemovalKind::CpuSchedulerCustomRule,
                "cpu-scheduler-exclusion",
                text_muted(t!("cpu_scheduler.no_custom_rules").to_string()).into_any_element(),
                cx,
            ))
            .into_any_element()
    }

    pub(in crate::ui::app) fn render_cpu_scheduler_process_priority_selector(
        &self,
        tuning_target: AdaptiveEngineTuningTarget,
        target: PriorityDefaultTarget,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let cpu_scheduler = self.adaptive_engine_cpu_scheduler_tuning(tuning_target);
        let selected = match target {
            PriorityDefaultTarget::Foreground => cpu_scheduler.focus_process_priority,
            PriorityDefaultTarget::VisibleWindow => cpu_scheduler.visible_window_priority,
            PriorityDefaultTarget::Background => cpu_scheduler.background_priority,
        }
        .safe_for_automatic_control();
        let id = format!(
            "{}-cpu-scheduler-{}-process-priority",
            adaptive_engine_tuning_target_key(tuning_target),
            match target {
                PriorityDefaultTarget::Foreground => "foreground",
                PriorityDefaultTarget::VisibleWindow => "visible-window",
                PriorityDefaultTarget::Background => "background",
            }
        );
        let priorities = &ProcessPrioritySetting::AUTOMATIC_ALL;
        self.render_dropdown_select(
            &id,
            process_priority_setting_label(selected),
            enabled,
            DropdownSelectWidth::Table,
            priorities.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for priority in priorities.iter().copied() {
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!("{id}-option-{priority:?}")),
                            process_priority_setting_label(priority),
                            selected == priority,
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            app.update_adaptive_engine_tuning(
                                tuning_target,
                                |tuning| match target {
                                    PriorityDefaultTarget::Foreground => {
                                        tuning.cpu_scheduler.focus_process_priority = priority
                                    }
                                    PriorityDefaultTarget::VisibleWindow => {
                                        tuning.cpu_scheduler.visible_window_priority = priority
                                    }
                                    PriorityDefaultTarget::Background => {
                                        tuning.cpu_scheduler.background_priority = priority
                                    }
                                },
                            );
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                options
            },
        )
    }
    pub(in crate::ui::app) fn render_background_processor_selection_selector(
        &self,
        target: AdaptiveEngineTuningTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = self
            .adaptive_engine_cpu_scheduler_tuning(target)
            .background_processor_selection;
        let processors = cpu_allocation::logical_processors();
        let target_key = adaptive_engine_tuning_target_key(target);
        self.render_dropdown_select(
            format!("{target_key}-cpu-scheduler-background-processor-selection"),
            background_processor_selection_label(selected),
            true,
            DropdownSelectWidth::Wide,
            BackgroundProcessorSelection::ALL.len(),
            window,
            cx,
            move |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for selection in BackgroundProcessorSelection::ALL {
                    let available = background_processor_selection_available(selection, &processors);
                    let row = dropdown_option_row(
                        SharedString::from(format!(
                            "{target_key}-cpu-scheduler-background-processor-selection-{selection:?}"
                        )),
                        background_processor_selection_label(selection),
                        selected == selection,
                        cx,
                    )
                    .when(!available, |row| row.opacity(0.48).cursor_default());
                    options = options.child(if available {
                        row.on_click(cx.listener(move |app, _, _, cx| {
                            app.update_adaptive_engine_tuning(target, |tuning| {
                                tuning.cpu_scheduler.background_processor_selection = selection;
                            });
                            app.active_power_plan_picker = None;
                            cx.notify();
                        }))
                    } else {
                        row
                    });
                }
                options
            },
        )
    }

    pub(in crate::ui::app) fn render_cpu_allocation_method_selector(
        &self,
        target: AdaptiveEngineTuningTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = self
            .adaptive_engine_cpu_scheduler_tuning(target)
            .cpu_allocation_method;
        self.render_dropdown_select(
            format!(
                "{}-cpu-scheduler-cpu-allocation-method",
                adaptive_engine_tuning_target_key(target)
            ),
            cpu_allocation_method_label(selected),
            true,
            DropdownSelectWidth::Standard,
            CpuAllocationMethod::ALL.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for mode in CpuAllocationMethod::ALL {
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!(
                                "cpu-scheduler-cpu-allocation-method-option-{mode:?}"
                            )),
                            cpu_allocation_method_label(mode),
                            selected == mode,
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            app.update_adaptive_engine_tuning(target, |tuning| {
                                tuning.cpu_scheduler.cpu_allocation_method = mode;
                            });
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                options
            },
        )
    }

    pub(in crate::ui::app) fn render_cpu_scheduler_memory_priority_selector(
        &self,
        target: AdaptiveEngineTuningTarget,
        tier: PriorityDefaultTarget,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let cpu_scheduler = self.adaptive_engine_cpu_scheduler_tuning(target);
        let selected = match tier {
            PriorityDefaultTarget::Foreground => cpu_scheduler.focus_process_memory_priority,
            PriorityDefaultTarget::VisibleWindow => cpu_scheduler.visible_window_memory_priority,
            PriorityDefaultTarget::Background => cpu_scheduler.background_memory_priority,
        };
        let id = match tier {
            PriorityDefaultTarget::Foreground => "cpu-scheduler-focus-process-memory-priority",
            PriorityDefaultTarget::VisibleWindow => "cpu-scheduler-visible-window-memory-priority",
            PriorityDefaultTarget::Background => "cpu-scheduler-background-memory-priority",
        };
        self.render_dropdown_select(
            format!("{}-{id}-level", adaptive_engine_tuning_target_key(target)),
            process_memory_priority_setting_label(selected),
            enabled,
            DropdownSelectWidth::Table,
            ProcessMemoryPrioritySetting::ALL.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for priority in ProcessMemoryPrioritySetting::ALL {
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!("{id}-option-{priority:?}")),
                            process_memory_priority_setting_label(priority),
                            selected == priority,
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            app.update_adaptive_engine_tuning(target, |tuning| match tier {
                                PriorityDefaultTarget::Foreground => {
                                    tuning.cpu_scheduler.focus_process_memory_priority = priority;
                                }
                                PriorityDefaultTarget::VisibleWindow => {
                                    tuning.cpu_scheduler.visible_window_memory_priority = priority;
                                }
                                PriorityDefaultTarget::Background => {
                                    tuning.cpu_scheduler.background_memory_priority = priority;
                                }
                            });
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                options
            },
        )
    }
}

fn cpu_scheduler_priority_table_row(
    id: impl Into<SharedString>,
    title: impl Into<SharedString>,
    help: impl Into<SharedString>,
    active: AnyElement,
    focus: AnyElement,
    visible_window: AnyElement,
    background: AnyElement,
) -> AnyElement {
    let id = id.into();
    compact_rule_row(id.clone())
        .child(
            h_flex()
                .w(px(SUSPENSION_ACTIVE_COLUMN_WIDTH))
                .flex_shrink_0()
                .justify_center()
                .child(active),
        )
        .child(
            h_flex()
                .flex_1()
                .min_w(px(0.0))
                .gap_1()
                .child(div().truncate().child(title.into()))
                .child(title_info_button(format!("{id}-help"), help)),
        )
        .child(focus)
        .child(visible_window)
        .child(background)
        .into_any_element()
}

fn adaptive_engine_preset_label(name: &str) -> String {
    let name = name.trim();
    if name.is_empty() {
        t!("adaptive_engine.unnamed_preset").to_string()
    } else {
        name.to_owned()
    }
}

fn adaptive_engine_tuning_target_key(target: AdaptiveEngineTuningTarget) -> &'static str {
    match target {
        AdaptiveEngineTuningTarget::Live => "live",
        AdaptiveEngineTuningTarget::Preset => "preset",
    }
}

fn adaptive_engine_cpu_behaviour_group_targets(
    target: AdaptiveEngineTuningTarget,
) -> (SettingGroupTarget, SettingGroupTarget) {
    match target {
        AdaptiveEngineTuningTarget::Live => (
            SettingGroupTarget::CpuPressureRestraint,
            SettingGroupTarget::LimitBackgroundProcessors,
        ),
        AdaptiveEngineTuningTarget::Preset => (
            SettingGroupTarget::AdaptiveEnginePresetCpuPressureRestraint,
            SettingGroupTarget::AdaptiveEnginePresetLimitBackgroundProcessors,
        ),
    }
}

fn adaptive_engine_cpu_pressure_controls_enabled(
    target: AdaptiveEngineTuningTarget,
    restraint_enabled: bool,
) -> bool {
    target == AdaptiveEngineTuningTarget::Preset || restraint_enabled
}

fn adaptive_engine_tuning_tab_label(tab: AdaptiveEngineTuningTab) -> String {
    match tab {
        AdaptiveEngineTuningTab::CpuBehaviour => t!("adaptive_engine.cpu_behaviour").to_string(),
        AdaptiveEngineTuningTab::ProcessorPower => {
            t!("adaptive_engine.processor_power").to_string()
        }
        AdaptiveEngineTuningTab::PriorityControl => {
            t!("adaptive_engine.priority_control").to_string()
        }
        AdaptiveEngineTuningTab::CustomRules => t!("cpu_scheduler.custom_rules").to_string(),
    }
}

fn adaptive_engine_preset_name_exists(
    presets: &[AdaptiveEnginePreset],
    editing_index: Option<usize>,
    name: &str,
) -> bool {
    let name = name.trim();
    !name.is_empty()
        && presets.iter().enumerate().any(|(index, preset)| {
            Some(index) != editing_index && preset.name.trim().eq_ignore_ascii_case(name)
        })
}

fn upsert_adaptive_engine_preset(
    presets: &mut Vec<AdaptiveEnginePreset>,
    editing_index: Option<usize>,
    name: &str,
    mut preset: AdaptiveEnginePreset,
) -> bool {
    let name = name.trim();
    if name.is_empty() || adaptive_engine_preset_name_exists(presets, editing_index, name) {
        return false;
    }
    preset.name = name.to_owned();
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

fn adaptive_efficiency_tier_label(mode: Option<bool>) -> String {
    match mode {
        None => t!("common.default").to_string(),
        Some(true) => t!("common.enabled").to_string(),
        Some(false) => t!("common.disabled").to_string(),
    }
}

fn background_processor_selection_available(
    selection: BackgroundProcessorSelection,
    processors: &[LogicalProcessorInfo],
) -> bool {
    let all_mask = cpu_allocation_processors_mask(processors);
    if selection == BackgroundProcessorSelection::Custom {
        return all_mask.count_ones() > 1;
    }

    let no_smt_mask = cpu_allocation_processors_no_smt_mask(processors);
    let kind_mask = |kind| cpu_allocation_processors_kind_mask(processors, kind);
    let mask = match selection {
        BackgroundProcessorSelection::LeastUsed => all_mask,
        BackgroundProcessorSelection::PerformanceCores
        | BackgroundProcessorSelection::LeastUsedPerformanceCores => {
            kind_mask(LogicalProcessorKind::Performance)
        }
        BackgroundProcessorSelection::EfficiencyCores
        | BackgroundProcessorSelection::LeastUsedEfficiencyCores => {
            kind_mask(LogicalProcessorKind::Efficiency)
        }
        BackgroundProcessorSelection::AllCoresNoSmt => no_smt_mask,
        BackgroundProcessorSelection::PerformanceCoresNoSmt => {
            kind_mask(LogicalProcessorKind::Performance) & no_smt_mask
        }
        BackgroundProcessorSelection::Custom => unreachable!("custom selection returns above"),
    };
    if selection.is_least_used() {
        all_mask.count_ones() > 1 && mask != 0
    } else {
        mask != 0 && mask != all_mask
    }
}

fn set_adaptive_efficiency_tier(
    settings: &mut CpuSchedulerSettings,
    target: PriorityDefaultTarget,
    mode: Option<bool>,
) {
    match target {
        PriorityDefaultTarget::Foreground => {
            settings.focus_process_background_efficiency_override_enabled = mode.is_some();
            if let Some(mode) = mode {
                settings.focus_process_background_efficiency_mode = mode;
            }
        }
        PriorityDefaultTarget::VisibleWindow => {
            settings.visible_window_background_efficiency_override_enabled = mode.is_some();
            if let Some(mode) = mode {
                settings.visible_window_background_efficiency_mode = mode;
            }
        }
        PriorityDefaultTarget::Background => {
            if let Some(mode) = mode {
                settings.background_efficiency_mode = mode;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adaptive_engine_live_tuning_edits_backing_settings_in_place() {
        let mut settings = Settings::default();
        settings.cpu_scheduler.custom_rules =
            vec![new_process_exclusion_rule("C:\\Apps\\keep.exe")];

        let tuning = AdaptiveEngineTuningMut::live(&mut settings);
        *tuning.processor_power_policy_enabled = false;
        tuning.base_processor_policy.performance_min = 17;
        tuning.background_pressure_profile.ac_policy = 73;
        tuning.focus_and_launch_profile.battery_policy = 61;
        tuning.cpu_scheduler.processor_limit_percent = 23;

        assert!(!settings.adaptive_engine.processor_power_policy_enabled);
        assert_eq!(
            settings
                .adaptive_engine
                .base_processor_policy
                .performance_min,
            17
        );
        assert_eq!(
            settings
                .adaptive_engine
                .background_pressure_profile
                .ac_policy,
            73
        );
        assert_eq!(
            settings
                .adaptive_engine
                .focus_and_launch_profile
                .battery_policy,
            61
        );
        assert_eq!(settings.cpu_scheduler.processor_limit_percent, 23);
        assert_eq!(settings.cpu_scheduler.custom_rules.len(), 1);
    }

    #[test]
    fn adaptive_engine_preset_editor_has_distinct_cpu_behaviour_ui_state() {
        assert_ne!(
            adaptive_engine_cpu_behaviour_group_targets(AdaptiveEngineTuningTarget::Live),
            adaptive_engine_cpu_behaviour_group_targets(AdaptiveEngineTuningTarget::Preset),
        );
        assert_ne!(
            adaptive_engine_tuning_target_key(AdaptiveEngineTuningTarget::Live),
            adaptive_engine_tuning_target_key(AdaptiveEngineTuningTarget::Preset),
        );
        assert!(!adaptive_engine_cpu_pressure_controls_enabled(
            AdaptiveEngineTuningTarget::Live,
            false,
        ));
        assert!(adaptive_engine_cpu_pressure_controls_enabled(
            AdaptiveEngineTuningTarget::Live,
            true,
        ));
        assert!(adaptive_engine_cpu_pressure_controls_enabled(
            AdaptiveEngineTuningTarget::Preset,
            false,
        ));
    }

    #[test]
    fn adaptive_engine_tuning_tabs_keep_custom_rules_live_only() {
        assert_eq!(
            AdaptiveEngineTuningTab::LIVE,
            [
                AdaptiveEngineTuningTab::CpuBehaviour,
                AdaptiveEngineTuningTab::ProcessorPower,
                AdaptiveEngineTuningTab::PriorityControl,
                AdaptiveEngineTuningTab::CustomRules,
            ]
        );
        assert_eq!(
            AdaptiveEngineTuningTab::PRESET,
            [
                AdaptiveEngineTuningTab::CpuBehaviour,
                AdaptiveEngineTuningTab::ProcessorPower,
                AdaptiveEngineTuningTab::PriorityControl,
            ]
        );
    }

    #[test]
    fn adaptive_efficiency_table_preserves_inheritance_and_template_matching() {
        let mut settings = CpuSchedulerSettings::default();
        apply_cpu_scheduler_preset(&mut settings, BuiltInAdaptiveEnginePreset::Performance);
        assert!(cpu_scheduler_matches_preset(
            &settings,
            BuiltInAdaptiveEnginePreset::Performance
        ));

        set_adaptive_efficiency_tier(&mut settings, PriorityDefaultTarget::Foreground, Some(true));
        assert!(!cpu_scheduler_matches_preset(
            &settings,
            BuiltInAdaptiveEnginePreset::Performance
        ));

        set_adaptive_efficiency_tier(&mut settings, PriorityDefaultTarget::Foreground, None);

        assert!(!settings.focus_process_background_efficiency_override_enabled);
        assert!(settings.focus_process_background_efficiency_mode);
    }

    #[test]
    fn adaptive_engine_custom_presets_preserve_non_preset_state() {
        let mut source = Settings::default();
        source.adaptive_engine.base_processor_policy =
            ProcessorPowerValues::for_preset(ProcessorPowerPreset::Performance);
        source.adaptive_engine.background_pressure_profile.ac_policy = 71;
        source.cpu_scheduler.processor_limit_percent = 23;
        let preset = capture_adaptive_engine_preset(&source, "Custom".to_owned());

        let mut target = Settings::default();
        target.adaptive_engine.enabled = false;
        target.cpu_scheduler.cpu_pressure_restraint_enabled = true;
        target.cpu_scheduler.custom_rules = vec![new_process_exclusion_rule("C:\\Apps\\keep.exe")];
        target.cpu_scheduler.io_priority.exclusions =
            vec![new_process_exclusion_rule("C:\\Apps\\keep-io.exe")];
        apply_adaptive_engine_preset(&mut target, &preset);

        assert!(!target.adaptive_engine.enabled);
        assert!(target.cpu_scheduler.cpu_pressure_restraint_enabled);
        assert_eq!(target.cpu_scheduler.processor_limit_percent, 23);
        assert_eq!(target.cpu_scheduler.custom_rules.len(), 1);
        assert_eq!(
            target.adaptive_engine.background_pressure_profile.ac_policy,
            71
        );
        assert_eq!(target.cpu_scheduler.io_priority.exclusions.len(), 1);
        assert!(adaptive_engine_matches_preset(&target, &preset));
    }

    #[test]
    fn adaptive_engine_custom_presets_cover_every_tuning_tab() {
        let mut source = Settings::default();
        source.adaptive_engine.processor_power_policy_enabled =
            !source.adaptive_engine.processor_power_policy_enabled;
        source.cpu_scheduler.background_processor_selection =
            BackgroundProcessorSelection::AllCoresNoSmt;
        source.cpu_scheduler.dynamic_priority_boost.enabled =
            !source.cpu_scheduler.dynamic_priority_boost.enabled;
        source
            .cpu_scheduler
            .focus_process_background_efficiency_mode = !source
            .cpu_scheduler
            .focus_process_background_efficiency_mode;
        source.adaptive_engine.focus_and_launch_profile.battery_mode = ProcessorBoostMode::Disabled;
        let preset = capture_adaptive_engine_preset(&source, "All tabs".to_owned());

        let mut target = Settings::default();
        apply_adaptive_engine_preset(&mut target, &preset);

        assert_eq!(
            target.adaptive_engine.processor_power_policy_enabled,
            source.adaptive_engine.processor_power_policy_enabled
        );
        assert_eq!(
            target.cpu_scheduler.background_processor_selection,
            source.cpu_scheduler.background_processor_selection
        );
        assert_eq!(
            target.cpu_scheduler.dynamic_priority_boost.enabled,
            source.cpu_scheduler.dynamic_priority_boost.enabled
        );
        assert_eq!(
            target
                .cpu_scheduler
                .focus_process_background_efficiency_mode,
            source
                .cpu_scheduler
                .focus_process_background_efficiency_mode
        );
        assert_eq!(
            target.adaptive_engine.focus_and_launch_profile.battery_mode,
            ProcessorBoostMode::Disabled
        );
    }

    #[test]
    fn adaptive_engine_custom_presets_require_unique_names() {
        let settings = Settings::default();
        let preset = capture_adaptive_engine_preset(&settings, String::new());
        let mut presets = Vec::new();

        assert!(upsert_adaptive_engine_preset(
            &mut presets,
            None,
            "Quiet",
            preset.clone(),
        ));
        assert!(!upsert_adaptive_engine_preset(
            &mut presets,
            None,
            "quiet",
            preset.clone(),
        ));
        assert!(upsert_adaptive_engine_preset(
            &mut presets,
            Some(0),
            "Responsive",
            preset,
        ));
        assert_eq!(presets[0].name, "Responsive");
    }
}
