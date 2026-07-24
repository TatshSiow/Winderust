use crate::ui::app::*;

impl WinderustApp {
    pub(in crate::ui::app) fn render_adaptive_engine_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let enabled = adaptive_engine_enabled(&self.settings);
        let timer_guard_available = self.settings.background_efficiency.enabled
            || (self.settings.workload_engine.enabled
                && self
                    .settings
                    .workload_engine
                    .workload_engine_background_efficiency_enabled);
        let timer_guard_label = if timer_guard_available {
            t!("adaptive_engine.audio_guarded").to_string()
        } else if self.settings.adaptive_engine.enabled
            && self.settings.adaptive_engine.processor_policy_enabled
        {
            t!("adaptive_engine.cpu_saver_active").to_string()
        } else if self.settings.adaptive_engine.enabled {
            t!("adaptive_engine.winderust_guarded").to_string()
        } else {
            t!("adaptive_engine.timer_guard_unavailable").to_string()
        };
        let advanced_enabled = self
            .settings
            .workload_engine
            .workload_engine_advanced_settings_enabled;
        let advanced_cards_motion_id = "expanded-child-power-mode-advanced-cards";
        let advanced_cards_progress = expandable_motion_progress(advanced_cards_motion_id);
        if advanced_cards_progress.is_some() {
            window.request_animation_frame();
        }
        let workload_engine_input = self
            .inputs
            .workload_engine_process
            .read(cx)
            .value()
            .to_string();
        let workload_engine_tunables =
            self.render_workload_engine_tunables(window, cx, self.settings.workload_engine.enabled);
        let workload_engine_efficiency = feature_body(self.settings.workload_engine.enabled)
            .child(self.render_workload_engine_efficiency_group(window, cx));
        let workload_engine_exclusions = feature_body(self.settings.workload_engine.enabled).child(
            self.render_workload_engine_exclusions_section(
                window,
                cx,
                &workload_engine_input,
                self.settings.workload_engine.enabled,
            ),
        );
        let manual_tuning = feature_body(enabled)
            .child(section_title_text(
                t!("adaptive_engine.cpu_behaviour").to_string(),
            ))
            .child(self.render_adaptive_engine_processor_policy_group(window, cx))
            .child(self.render_adaptive_engine_cpu_scheduling_group(window, cx))
            .child(setting_action_card_with_help(
                "adaptive-engine-cpu-pressure",
                t!("adaptive_engine.cpu_pressure").to_string(),
                t!("adaptive_engine.cpu_pressure_help").to_string(),
                switch_toggle_action(
                    "adaptive-engine-cpu-pressure-toggle",
                    self.settings.workload_engine.workload_engine_enabled,
                    cx.listener(|app, checked, _, cx| {
                        app.settings.workload_engine.workload_engine_enabled = *checked;
                        if *checked {
                            app.settings.workload_engine.enabled = true;
                            app.settings
                                .workload_engine
                                .workload_engine_affinity_escalation_enabled = true;
                            app.settings
                                .workload_engine
                                .lower_background_auto_cpu_percent = true;
                        }
                        cx.notify();
                    }),
                ),
            ))
            .child(disabled_feature_body(
                "adaptive-engine-workload-engine-tunables",
                workload_engine_tunables,
                self.settings.workload_engine.enabled,
                cx,
            ))
            .child(section_title_text(t!("adaptive_engine.misc").to_string()))
            .child(disabled_feature_body(
                "adaptive-engine-workload-engine-efficiency",
                workload_engine_efficiency,
                self.settings.workload_engine.enabled,
                cx,
            ))
            .child(setting_action_card_with_help(
                "adaptive-engine-timer-requests",
                t!("adaptive_engine.timer_requests").to_string(),
                t!("adaptive_engine.timer_requests_help").to_string(),
                value_pill(timer_guard_label).into_any_element(),
            ))
            .child(disabled_feature_body(
                "adaptive-engine-workload-engine-exclusions",
                workload_engine_exclusions,
                self.settings.workload_engine.enabled,
                cx,
            ));
        let mut body =
            feature_body(enabled).child(self.render_power_mode_advanced_settings_toggle(cx));
        if advanced_enabled || advanced_cards_progress.is_some() {
            let cards = manual_tuning.into_any_element();
            body = body.child(if let Some(progress) = advanced_cards_progress {
                expanded_child_at_progress(cards, None, progress)
            } else {
                expanded_child(cards)
            });
        } else {
            remember_expanded_child_hidden("power-mode-advanced-cards");
        }

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
            .child(self.render_power_mode_preset_selector(window, cx))
            .child(disabled_feature_body(
                "adaptive-engine-body",
                body,
                enabled,
                cx,
            ))
            .child(self.render_adaptive_engine_status_card())
            .into_any_element()
    }

    pub(in crate::ui::app) fn render_power_mode_preset_selector(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected_preset = PowerModePreset::ALL
            .iter()
            .copied()
            .find(|preset| power_mode_matches_preset(&self.settings, *preset));
        let dropdown = self.render_dropdown_select(
            "adaptive-engine-power-mode",
            selected_preset
                .map(power_mode_preset_label)
                .unwrap_or_else(|| t!("common.custom").to_string()),
            true,
            DropdownSelectWidth::Wide,
            PowerModePreset::ALL.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for preset in PowerModePreset::ALL {
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!(
                                "adaptive-engine-power-mode-option-{preset:?}"
                            )),
                            power_mode_preset_label(preset),
                            selected_preset == Some(preset),
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            apply_power_mode_preset(&mut app.settings, preset);
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                options
            },
        );

        setting_action_card_with_help(
            "adaptive-engine-power-mode",
            t!("adaptive_engine.power_mode").to_string(),
            t!("adaptive_engine.power_mode_help").to_string(),
            dropdown,
        )
        .into_any_element()
    }

    pub(in crate::ui::app) fn render_adaptive_engine_status_card(&self) -> gpui::Div {
        let selected_preset = PowerModePreset::ALL
            .iter()
            .copied()
            .find(|preset| power_mode_matches_preset(&self.settings, *preset));
        let power_mode = selected_preset
            .map(power_mode_preset_label)
            .unwrap_or_else(|| t!("common.custom").to_string());
        titled_status_list(
            &t!("adaptive_engine.status"),
            None,
            vec![
                (
                    None,
                    t!("adaptive_engine.power_mode").to_string(),
                    power_mode,
                ),
                (
                    None,
                    t!("adaptive_engine.processor_policy").to_string(),
                    if let Some(profile) = &self.workload_engine_status.adaptive_power_profile {
                        t!(
                            "adaptive_engine.processor_policy_adaptive",
                            profile = adaptive_power_profile_label(profile)
                        )
                        .to_string()
                    } else if matches!(
                        selected_preset,
                        Some(PowerModePreset::Performance) | Some(PowerModePreset::Speed)
                    ) {
                        t!("adaptive_engine.processor_policy_fixed").to_string()
                    } else if self.settings.adaptive_engine.enabled
                        && self.settings.adaptive_engine.processor_policy_enabled
                    {
                        t!("adaptive_engine.processor_policy_dynamic").to_string()
                    } else {
                        t!("common.disabled").to_string()
                    },
                ),
                (
                    None,
                    t!("adaptive_engine.background_efficiency").to_string(),
                    if self.settings.background_efficiency.enabled {
                        format!(
                            "{} {}",
                            self.background_efficiency_status.throttled_processes,
                            t!("background_efficiency.throttled_processes")
                        )
                    } else {
                        t!("common.disabled").to_string()
                    },
                ),
                (
                    None,
                    t!("adaptive_engine.timer_ignored").to_string(),
                    format!(
                        "{} {}",
                        self.background_efficiency_status
                            .timer_resolution_ignored_processes
                            + self
                                .workload_engine_status
                                .timer_resolution_ignored_processes,
                        t!("adaptive_engine.audio_guarded")
                    ),
                ),
                (
                    None,
                    t!("adaptive_engine.workload_engine").to_string(),
                    if self.settings.workload_engine.enabled {
                        format!(
                            "{} {}",
                            self.workload_engine_status.background_adjusted_processes,
                            t!("workload_engine.background_adjusted")
                        )
                    } else {
                        t!("common.disabled").to_string()
                    },
                ),
                (
                    None,
                    t!("adaptive_engine.restrained").to_string(),
                    localized_runtime_status(&self.workload_engine_status.workload_engine_message),
                ),
            ],
            None,
        )
    }

    pub(in crate::ui::app) fn render_workload_engine_tunables(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
        enabled: bool,
    ) -> gpui::Div {
        feature_body(enabled).child(self.render_workload_engine_advanced_cards(window, cx))
    }

    pub(in crate::ui::app) fn render_power_mode_advanced_settings_toggle(
        &self,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        setting_action_card_with_help(
            "power-mode-advanced-settings-enabled",
            t!("adaptive_engine.advanced_settings").to_string(),
            t!("adaptive_engine.advanced_settings_help").to_string(),
            switch_toggle_action(
                "power-mode-advanced-settings-toggle",
                self.settings
                    .workload_engine
                    .workload_engine_advanced_settings_enabled,
                cx.listener(|app, checked, _, cx| {
                    begin_expandable_motion("expanded-child-power-mode-advanced-cards", *checked);
                    app.settings
                        .workload_engine
                        .workload_engine_advanced_settings_enabled = *checked;
                    cx.notify();
                }),
            ),
        )
        .into_any_element()
    }

    pub(in crate::ui::app) fn render_adaptive_engine_processor_policy_group(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        setting_group_with_help(
            SettingGroupTarget::AdaptiveEngineProcessorPolicy,
            (
                t!("adaptive_engine.processor_policy").to_string(),
                t!("adaptive_engine.processor_policy_help").to_string(),
            ),
            setting_group_switch_action(
                "adaptive-engine-processor-policy-toggle",
                self.settings.adaptive_engine.processor_policy_enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings.adaptive_engine.processor_policy_enabled = *checked;
                    cx.notify();
                }),
            ),
            self.is_setting_group_collapsed(SettingGroupTarget::AdaptiveEngineProcessorPolicy),
            vec![
                self.render_adaptive_engine_processor_policy_row(
                    AdaptiveEngineProcessorPolicyField::CoreParkingMin,
                    "adaptive-engine-processor-policy-core-parking-min",
                    t!("processor_power.core_parking_min").to_string(),
                    cx,
                ),
                self.render_adaptive_engine_processor_policy_row(
                    AdaptiveEngineProcessorPolicyField::PerformanceMin,
                    "adaptive-engine-processor-policy-performance-min",
                    t!("processor_power.processor_min").to_string(),
                    cx,
                ),
                self.render_adaptive_engine_processor_policy_row(
                    AdaptiveEngineProcessorPolicyField::PerformanceMax,
                    "adaptive-engine-processor-policy-performance-max",
                    t!("processor_power.processor_max").to_string(),
                    cx,
                ),
                self.render_adaptive_engine_processor_policy_row(
                    AdaptiveEngineProcessorPolicyField::BoostPolicy,
                    "adaptive-engine-processor-policy-boost-policy",
                    t!("processor_power.boost_policy").to_string(),
                    cx,
                ),
                setting_group_action_row(
                    "adaptive-engine-processor-policy-boost-mode",
                    t!("processor_power.boost_mode").to_string(),
                    self.render_adaptive_engine_processor_boost_mode_picker(window, cx),
                    true,
                )
                .into_any_element(),
            ],
            window,
            cx,
        )
        .into_any_element()
    }

    pub(in crate::ui::app) fn render_adaptive_engine_processor_policy_row(
        &self,
        field: AdaptiveEngineProcessorPolicyField,
        id: &'static str,
        title: String,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let value = u64::from(self.adaptive_engine_processor_policy_percent(field));
        setting_group_stepper_row_u64(
            id,
            title,
            value,
            self.render_numeric_value(
                NumericField::AdaptiveEngineProcessorPolicy(field),
                format!("{value}%"),
                value.to_string(),
                cx,
            ),
            true,
            cx.listener(move |app, change: &StepChange<u64>, _, cx| {
                let current = u64::from(app.adaptive_engine_processor_policy_percent(field));
                let value = apply_u64_step(current, change, 0, 100);
                app.set_adaptive_engine_processor_policy_percent(field, value);
                cx.notify();
            }),
        )
    }

    pub(in crate::ui::app) fn render_adaptive_engine_processor_boost_mode_picker(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let picker_id = "adaptive-engine-processor-policy-boost-mode-picker";
        let is_open = self.active_power_plan_picker.as_deref() == Some(picker_id);
        let placement = self.dropdown_placement(
            picker_id,
            dropdown_list_height(ProcessorBoostMode::ALL.len()),
            window,
        );
        let selected = self
            .settings
            .adaptive_engine
            .processor_policy_values
            .normalized()
            .boost_mode;
        let mut options = dropdown_surface(cx, placement.max_height);
        for boost_mode in ProcessorBoostMode::ALL {
            options = options.child(
                dropdown_option_row(
                    SharedString::from(format!(
                        "adaptive-engine-processor-policy-boost-mode-option-{boost_mode:?}"
                    )),
                    processor_boost_mode_label(boost_mode),
                    selected == boost_mode,
                    cx,
                )
                .on_click(cx.listener(move |app, _: &gpui::ClickEvent, _, cx| {
                    let mut values = app
                        .settings
                        .adaptive_engine
                        .processor_policy_values
                        .normalized();
                    values.boost_mode = boost_mode;
                    app.settings.adaptive_engine.processor_policy_values = values;
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

    pub(in crate::ui::app) fn render_adaptive_engine_cpu_scheduling_group(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        setting_group_with_help(
            SettingGroupTarget::AdaptiveEngineCpuScheduling,
            (
                t!("adaptive_engine.cpu_scheduling").to_string(),
                t!("adaptive_engine.cpu_scheduling_help").to_string(),
            ),
            setting_group_switch_action(
                "adaptive-engine-workload-engine-toggle",
                self.settings.workload_engine.enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings.workload_engine.enabled = *checked;
                    cx.notify();
                }),
            ),
            self.is_setting_group_collapsed(SettingGroupTarget::AdaptiveEngineCpuScheduling),
            vec![setting_group_action_row_with_help(
                "adaptive-engine-cpu-scheduling-target",
                t!("adaptive_engine.cpu_scheduling_target").to_string(),
                t!("adaptive_engine.cpu_scheduling_target_help").to_string(),
                self.render_workload_engine_target_preset_selector(window, cx),
                true,
            )
            .into_any_element()],
            window,
            cx,
        )
        .into_any_element()
    }

    pub(in crate::ui::app) fn render_workload_engine_target_preset_selector(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected_preset = WorkloadEnginePreset::ALL
            .iter()
            .copied()
            .find(|preset| workload_engine_matches_preset(&self.settings.workload_engine, *preset));
        self.render_dropdown_select(
            "adaptive-engine-cpu-scheduling-target-preset",
            selected_preset
                .map(workload_engine_preset_label)
                .unwrap_or_else(|| t!("common.custom").to_string()),
            true,
            DropdownSelectWidth::Standard,
            WorkloadEnginePreset::ALL.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for preset in WorkloadEnginePreset::ALL {
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!(
                                "adaptive-engine-cpu-scheduling-target-option-{preset:?}"
                            )),
                            workload_engine_preset_label(preset),
                            selected_preset == Some(preset),
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            apply_workload_engine_preset(&mut app.settings.workload_engine, preset);
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                options
            },
        )
    }

    pub(in crate::ui::app) fn render_workload_engine_efficiency_group(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let settings = &self.settings.workload_engine;
        let enabled = settings.enabled;
        let auto_efficiency_controls_enabled = enabled
            && !self.settings.background_efficiency.enabled
            && settings.workload_engine_background_efficiency_enabled;
        let efficiency_action = if self.settings.background_efficiency.enabled {
            value_pill(t!("workload_engine.background_efficiency_handled").to_string())
                .into_any_element()
        } else {
            setting_group_switch_action(
                "workload-engine-auto-efficiency-switch",
                settings.workload_engine_background_efficiency_enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings
                        .workload_engine
                        .workload_engine_background_efficiency_enabled = *checked;
                    cx.notify();
                }),
            )
        };

        setting_group_with_help(
            SettingGroupTarget::WorkloadEngineEfficiency,
            (
                t!("workload_engine.auto_background_efficiency").to_string(),
                t!("workload_engine.auto_background_efficiency_help").to_string(),
            ),
            efficiency_action,
            self.is_setting_group_collapsed(SettingGroupTarget::WorkloadEngineEfficiency),
            vec![setting_group_action_row_with_help(
                "workload-engine-auto-efficiency-level",
                t!("workload_engine.auto_efficiency_level").to_string(),
                t!("workload_engine.auto_efficiency_level_help").to_string(),
                self.render_background_efficiency_aggressiveness_picker(
                    self.settings.background_efficiency.aggressiveness,
                    auto_efficiency_controls_enabled,
                    window,
                    cx,
                ),
                true,
            )
            .when(!auto_efficiency_controls_enabled, |row| {
                row.opacity(0.42).cursor_default()
            })
            .into_any_element()],
            window,
            cx,
        )
        .into_any_element()
    }

    pub(in crate::ui::app) fn render_workload_engine_advanced_cards(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let settings = &self.settings.workload_engine;
        let process_priority_action = if self.settings.background_efficiency.enabled {
            value_pill(t!("workload_engine.background_efficiency_handled").to_string())
                .into_any_element()
        } else {
            setting_group_switch_action(
                "workload-engine-lower-background-toggle",
                settings.lower_background_apps,
                cx.listener(|app, checked, _, cx| {
                    app.settings.workload_engine.lower_background_apps = *checked;
                    cx.notify();
                }),
            )
        };
        let mut affinity_rows = vec![
            setting_group_action_row(
                "workload-engine-auto-escalation-tuning",
                t!("workload_engine.workload_engine_escalation_tuning").to_string(),
                self.render_workload_engine_escalation_tuning_selector(window, cx),
                true,
            )
            .into_any_element(),
            setting_group_action_row(
                "workload-engine-auto-affinity-mode",
                t!("workload_engine.workload_engine_affinity_mode").to_string(),
                if settings.lower_background_auto_cpu_percent {
                    value_pill(format!(
                        "{} ({})",
                        cpu_restriction_mode_label(CpuRestrictionMode::SoftCpuSets),
                        t!("workload_engine.priority_auto")
                    ))
                    .into_any_element()
                } else {
                    self.render_workload_engine_affinity_mode_selector(window, cx)
                },
                true,
            )
            .into_any_element(),
        ];
        if !settings.lower_background_auto_cpu_percent {
            affinity_rows.push(
                setting_group_stepper_row_u64(
                    "workload-engine-auto-cpu-percent",
                    t!("workload_engine.minimum_cpu_share").to_string(),
                    u64::from(settings.workload_engine_cpu_percent),
                    self.render_numeric_value(
                        NumericField::WorkloadEngineCpuPercent,
                        format!("{}%", settings.workload_engine_cpu_percent),
                        settings.workload_engine_cpu_percent.to_string(),
                        cx,
                    ),
                    true,
                    cx.listener(|app, change: &StepChange<u64>, _, cx| {
                        let current =
                            u64::from(app.settings.workload_engine.workload_engine_cpu_percent);
                        app.settings.workload_engine.workload_engine_cpu_percent = apply_u64_step(
                            current,
                            change,
                            WORKLOAD_ENGINE_THRESHOLD_MIN_PERCENT,
                            WORKLOAD_ENGINE_THRESHOLD_MAX_PERCENT,
                        )
                            as u8;
                        cx.notify();
                    }),
                )
                .into_any_element(),
            );
        }
        let rows = vec![
            setting_group_with_help(
                SettingGroupTarget::WorkloadEngineAffinity,
                (
                    t!("workload_engine.auto_affinity_escalation").to_string(),
                    t!("workload_engine.auto_affinity_escalation_help").to_string(),
                ),
                setting_group_switch_action(
                    "workload-engine-auto-affinity-escalation-switch",
                    settings.workload_engine_affinity_escalation_enabled,
                    cx.listener(|app, checked, _, cx| {
                        app.settings
                            .workload_engine
                            .workload_engine_affinity_escalation_enabled = *checked;
                        cx.notify();
                    }),
                ),
                self.is_setting_group_collapsed(SettingGroupTarget::WorkloadEngineAffinity),
                affinity_rows,
                window,
                cx,
            )
            .into_any_element(),
            setting_group_with_help(
                SettingGroupTarget::WorkloadEngineBehaviourTuning,
                (
                    t!("workload_engine.workload_engine_behaviour_tuning").to_string(),
                    t!("workload_engine.workload_engine_behaviour_tuning_help").to_string(),
                ),
                div().into_any_element(),
                self.is_setting_group_collapsed(SettingGroupTarget::WorkloadEngineBehaviourTuning),
                vec![
                    setting_group_stepper_row_u64_with_help(
                        "workload-engine-auto-total-threshold",
                        t!("workload_engine.workload_engine_total_threshold").to_string(),
                        t!("workload_engine.workload_engine_total_threshold_help").to_string(),
                        u64::from(settings.workload_engine_total_threshold_percent),
                        self.render_numeric_value(
                            NumericField::WorkloadEngineTotalThreshold,
                            format!("{}%", settings.workload_engine_total_threshold_percent),
                            settings.workload_engine_total_threshold_percent.to_string(),
                            cx,
                        ),
                        true,
                        cx.listener(|app, change: &StepChange<u64>, _, cx| {
                            let current = u64::from(
                                app.settings
                                    .workload_engine
                                    .workload_engine_total_threshold_percent,
                            );
                            app.settings
                                .workload_engine
                                .workload_engine_total_threshold_percent = apply_u64_step(
                                current,
                                change,
                                WORKLOAD_ENGINE_THRESHOLD_MIN_PERCENT,
                                WORKLOAD_ENGINE_THRESHOLD_MAX_PERCENT,
                            )
                                as u8;
                            cx.notify();
                        }),
                    ),
                    setting_group_stepper_row_u64_with_help(
                        "workload-engine-auto-threshold",
                        t!("workload_engine.workload_engine_threshold").to_string(),
                        t!("workload_engine.workload_engine_threshold_help").to_string(),
                        u64::from(settings.workload_engine_threshold_percent),
                        self.render_numeric_value(
                            NumericField::WorkloadEngineThreshold,
                            format!("{}%", settings.workload_engine_threshold_percent),
                            settings.workload_engine_threshold_percent.to_string(),
                            cx,
                        ),
                        true,
                        cx.listener(|app, change: &StepChange<u64>, _, cx| {
                            let current = u64::from(
                                app.settings
                                    .workload_engine
                                    .workload_engine_threshold_percent,
                            );
                            app.settings
                                .workload_engine
                                .workload_engine_threshold_percent = apply_u64_step(
                                current,
                                change,
                                WORKLOAD_ENGINE_THRESHOLD_MIN_PERCENT,
                                WORKLOAD_ENGINE_THRESHOLD_MAX_PERCENT,
                            )
                                as u8;
                            cx.notify();
                        }),
                    ),
                    setting_group_stepper_row_u64_with_help(
                        "workload-engine-auto-restore-threshold",
                        t!("workload_engine.workload_engine_restore_threshold").to_string(),
                        t!("workload_engine.workload_engine_restore_threshold_help").to_string(),
                        u64::from(settings.workload_engine_restore_threshold_percent),
                        self.render_numeric_value(
                            NumericField::WorkloadEngineRestoreThreshold,
                            format!("{}%", settings.workload_engine_restore_threshold_percent),
                            settings
                                .workload_engine_restore_threshold_percent
                                .to_string(),
                            cx,
                        ),
                        true,
                        cx.listener(|app, change: &StepChange<u64>, _, cx| {
                            let current = u64::from(
                                app.settings
                                    .workload_engine
                                    .workload_engine_restore_threshold_percent,
                            );
                            app.settings
                                .workload_engine
                                .workload_engine_restore_threshold_percent = apply_u64_step(
                                current,
                                change,
                                WORKLOAD_ENGINE_THRESHOLD_MIN_PERCENT,
                                WORKLOAD_ENGINE_THRESHOLD_MAX_PERCENT,
                            )
                                as u8;
                            cx.notify();
                        }),
                    ),
                    setting_group_stepper_row_u64_with_help(
                        "workload-engine-auto-max-targets",
                        t!("workload_engine.workload_engine_max_targeted_processes").to_string(),
                        t!("workload_engine.workload_engine_max_targeted_processes_help")
                            .to_string(),
                        u64::from(settings.workload_engine_max_targeted_processes),
                        self.render_numeric_value(
                            NumericField::WorkloadEngineMaxTargetedProcesses,
                            settings.workload_engine_max_targeted_processes.to_string(),
                            settings.workload_engine_max_targeted_processes.to_string(),
                            cx,
                        ),
                        true,
                        cx.listener(|app, change: &StepChange<u64>, _, cx| {
                            let current = u64::from(
                                app.settings
                                    .workload_engine
                                    .workload_engine_max_targeted_processes,
                            );
                            app.settings
                                .workload_engine
                                .workload_engine_max_targeted_processes = apply_u64_step(
                                current,
                                change,
                                WORKLOAD_ENGINE_TARGET_LIMIT_MIN,
                                WORKLOAD_ENGINE_TARGET_LIMIT_MAX,
                            )
                                as u8;
                            cx.notify();
                        }),
                    ),
                    setting_group_stepper_row_u64_with_help(
                        "workload-engine-auto-sustain",
                        t!("workload_engine.workload_engine_sustain").to_string(),
                        t!("workload_engine.workload_engine_sustain_help").to_string(),
                        settings.workload_engine_sustain_seconds,
                        self.render_numeric_value(
                            NumericField::WorkloadEngineSustain,
                            ui::duration_label(settings.workload_engine_sustain_seconds),
                            settings.workload_engine_sustain_seconds.to_string(),
                            cx,
                        ),
                        true,
                        cx.listener(|app, change: &StepChange<u64>, _, cx| {
                            app.settings.workload_engine.workload_engine_sustain_seconds =
                                apply_u64_step(
                                    app.settings.workload_engine.workload_engine_sustain_seconds,
                                    change,
                                    WORKLOAD_ENGINE_SECONDS_MIN,
                                    WORKLOAD_ENGINE_SECONDS_MAX,
                                );
                            cx.notify();
                        }),
                    ),
                    setting_group_stepper_row_u64_with_help(
                        "workload-engine-auto-minimum-restraint",
                        t!("workload_engine.workload_engine_minimum_restraint").to_string(),
                        t!("workload_engine.workload_engine_minimum_restraint_help").to_string(),
                        settings.workload_engine_minimum_restraint_seconds,
                        self.render_numeric_value(
                            NumericField::WorkloadEngineMinimumRestraint,
                            ui::duration_label(settings.workload_engine_minimum_restraint_seconds),
                            settings
                                .workload_engine_minimum_restraint_seconds
                                .to_string(),
                            cx,
                        ),
                        true,
                        cx.listener(|app, change: &StepChange<u64>, _, cx| {
                            app.settings
                                .workload_engine
                                .workload_engine_minimum_restraint_seconds = apply_u64_step(
                                app.settings
                                    .workload_engine
                                    .workload_engine_minimum_restraint_seconds,
                                change,
                                WORKLOAD_ENGINE_SECONDS_MIN,
                                WORKLOAD_ENGINE_SECONDS_MAX,
                            );
                            cx.notify();
                        }),
                    ),
                    setting_group_stepper_row_u64_with_help(
                        "workload-engine-auto-cooldown",
                        t!("workload_engine.workload_engine_cooldown").to_string(),
                        t!("workload_engine.workload_engine_cooldown_help").to_string(),
                        settings.workload_engine_cooldown_seconds,
                        self.render_numeric_value(
                            NumericField::WorkloadEngineCooldown,
                            ui::duration_label(settings.workload_engine_cooldown_seconds),
                            settings.workload_engine_cooldown_seconds.to_string(),
                            cx,
                        ),
                        true,
                        cx.listener(|app, change: &StepChange<u64>, _, cx| {
                            app.settings
                                .workload_engine
                                .workload_engine_cooldown_seconds = apply_u64_step(
                                app.settings
                                    .workload_engine
                                    .workload_engine_cooldown_seconds,
                                change,
                                WORKLOAD_ENGINE_SECONDS_MIN,
                                WORKLOAD_ENGINE_SECONDS_MAX,
                            );
                            cx.notify();
                        }),
                    ),
                ],
                window,
                cx,
            )
            .into_any_element(),
            section_title_text(t!("adaptive_engine.priority_control").to_string())
                .into_any_element(),
            setting_group_with_help(
                SettingGroupTarget::WorkloadEngineProcessPriority,
                (
                    t!("workload_engine.auto_process_priority").to_string(),
                    t!("workload_engine.auto_process_priority_help").to_string(),
                ),
                process_priority_action,
                self.is_setting_group_collapsed(SettingGroupTarget::WorkloadEngineProcessPriority),
                vec![
                    setting_group_action_row(
                        "workload-engine-auto-background-process-priority",
                        t!("workload_engine.background_priority").to_string(),
                        self.render_workload_engine_background_priority_selector(window, cx),
                        true,
                    )
                    .into_any_element(),
                    self.render_foreground_boost_selector(window, cx),
                ],
                window,
                cx,
            )
            .into_any_element(),
            self.render_workload_engine_priority_assist_group(window, cx),
            setting_group_with_help(
                SettingGroupTarget::WorkloadEngineMemoryPriority,
                (
                    t!("workload_engine.auto_memory_priority").to_string(),
                    t!("workload_engine.auto_memory_priority_help").to_string(),
                ),
                setting_group_switch_action(
                    "workload-engine-auto-memory-priority-switch",
                    settings.workload_engine_memory_priority_enabled,
                    cx.listener(|app, checked, _, cx| {
                        app.settings
                            .workload_engine
                            .workload_engine_memory_priority_enabled = *checked;
                        cx.notify();
                    }),
                ),
                self.is_setting_group_collapsed(SettingGroupTarget::WorkloadEngineMemoryPriority),
                vec![
                    setting_group_action_row(
                        "workload-engine-auto-foreground-memory-priority-level",
                        t!("workload_engine.workload_engine_foreground_memory_priority_level")
                            .to_string(),
                        self.render_workload_engine_foreground_memory_priority_selector(window, cx),
                        true,
                    )
                    .into_any_element(),
                    setting_group_action_row(
                        "workload-engine-auto-memory-priority-level",
                        t!("workload_engine.workload_engine_memory_priority_level").to_string(),
                        self.render_workload_engine_memory_priority_selector(window, cx),
                        true,
                    )
                    .into_any_element(),
                ],
                window,
                cx,
            )
            .into_any_element(),
        ];

        v_flex().gap_2().children(rows).into_any_element()
    }

    pub(in crate::ui::app) fn render_workload_engine_priority_assist_group(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let settings = &self.settings.workload_engine;
        let io_enabled = settings.workload_engine_io_priority.enabled;
        let thread_enabled = settings.workload_engine_thread_priority.enabled;
        let boost_enabled = settings.workload_engine_dynamic_priority_boost.enabled;
        let gpu_enabled = settings.workload_engine_gpu_priority.enabled;
        v_flex()
            .w_full()
            .min_w(px(0.0))
            .gap_2()
            .child(
                setting_group_with_help(
                    SettingGroupTarget::WorkloadEngineThreadPriority,
                    (
                        t!("workload_engine.auto_thread_priority").to_string(),
                        t!("thread_priority.intro_1").to_string(),
                    ),
                    setting_group_switch_action(
                        "workload-engine-auto-thread-enabled-switch",
                        thread_enabled,
                        cx.listener(|app, checked, _, cx| {
                            app.settings
                                .workload_engine
                                .workload_engine_thread_priority
                                .enabled = *checked;
                            cx.notify();
                        }),
                    ),
                    self.is_setting_group_collapsed(
                        SettingGroupTarget::WorkloadEngineThreadPriority,
                    ),
                    vec![
                        setting_group_action_row(
                            "workload-engine-auto-thread-foreground-priority",
                            t!("workload_engine.priority_foreground_value").to_string(),
                            self.render_workload_engine_thread_priority_selector(
                                ThreadPriorityDefaultTarget::Foreground,
                                settings.workload_engine_thread_priority.foreground_priority,
                                thread_enabled,
                                window,
                                cx,
                            ),
                            false,
                        )
                        .into_any_element(),
                        setting_group_action_row(
                            "workload-engine-auto-thread-background-priority",
                            t!("workload_engine.priority_background_value").to_string(),
                            self.render_workload_engine_thread_priority_selector(
                                ThreadPriorityDefaultTarget::Background,
                                settings.workload_engine_thread_priority.background_priority,
                                thread_enabled,
                                window,
                                cx,
                            ),
                            false,
                        )
                        .into_any_element(),
                    ],
                    window,
                    cx,
                )
                .into_any_element(),
            )
            .child(
                setting_group_with_help(
                    SettingGroupTarget::WorkloadEngineDynamicPriorityBoost,
                    (
                        t!("workload_engine.auto_dynamic_priority_boost").to_string(),
                        t!("dynamic_priority_boost.intro_1").to_string(),
                    ),
                    setting_group_switch_action(
                        "workload-engine-auto-boost-enabled-switch",
                        boost_enabled,
                        cx.listener(|app, checked, _, cx| {
                            app.settings
                                .workload_engine
                                .workload_engine_dynamic_priority_boost
                                .enabled = *checked;
                            cx.notify();
                        }),
                    ),
                    self.is_setting_group_collapsed(
                        SettingGroupTarget::WorkloadEngineDynamicPriorityBoost,
                    ),
                    vec![
                        setting_group_action_row(
                            "workload-engine-auto-boost-foreground",
                            t!("workload_engine.priority_foreground_value").to_string(),
                            self.render_workload_engine_dynamic_priority_boost_selector(
                                DynamicPriorityBoostDefaultTarget::Foreground,
                                settings
                                    .workload_engine_dynamic_priority_boost
                                    .foreground_boost,
                                boost_enabled,
                                window,
                                cx,
                            ),
                            false,
                        )
                        .into_any_element(),
                        setting_group_action_row(
                            "workload-engine-auto-boost-background",
                            t!("workload_engine.priority_background_value").to_string(),
                            self.render_workload_engine_dynamic_priority_boost_selector(
                                DynamicPriorityBoostDefaultTarget::Background,
                                settings
                                    .workload_engine_dynamic_priority_boost
                                    .background_boost,
                                boost_enabled,
                                window,
                                cx,
                            ),
                            false,
                        )
                        .into_any_element(),
                    ],
                    window,
                    cx,
                )
                .into_any_element(),
            )
            .child(
                setting_group_with_help(
                    SettingGroupTarget::WorkloadEngineIoPriority,
                    (
                        t!("workload_engine.auto_io_priority").to_string(),
                        t!("workload_engine.auto_io_priority_help").to_string(),
                    ),
                    setting_group_switch_action(
                        "workload-engine-auto-io-enabled-switch",
                        io_enabled,
                        cx.listener(|app, checked, _, cx| {
                            app.settings
                                .workload_engine
                                .workload_engine_io_priority
                                .enabled = *checked;
                            if *checked {
                                app.settings
                                    .workload_engine
                                    .lower_background_io_priority_enabled = false;
                            }
                            cx.notify();
                        }),
                    ),
                    self.is_setting_group_collapsed(SettingGroupTarget::WorkloadEngineIoPriority),
                    vec![
                        setting_group_action_row(
                            "workload-engine-auto-io-foreground-priority",
                            t!("workload_engine.priority_foreground_value").to_string(),
                            self.render_workload_engine_io_priority_selector(
                                IoPriorityDefaultTarget::Foreground,
                                settings.workload_engine_io_priority.foreground_priority,
                                io_enabled,
                                window,
                                cx,
                            ),
                            false,
                        )
                        .into_any_element(),
                        setting_group_action_row(
                            "workload-engine-auto-io-background-priority",
                            t!("workload_engine.priority_background_value").to_string(),
                            self.render_workload_engine_io_priority_selector(
                                IoPriorityDefaultTarget::Background,
                                settings.workload_engine_io_priority.background_priority,
                                io_enabled,
                                window,
                                cx,
                            ),
                            false,
                        )
                        .into_any_element(),
                    ],
                    window,
                    cx,
                )
                .into_any_element(),
            )
            .child(
                setting_group_with_help(
                    SettingGroupTarget::WorkloadEngineGpuPriority,
                    (
                        t!("workload_engine.auto_gpu_priority").to_string(),
                        t!("gpu_priority.intro_1").to_string(),
                    ),
                    setting_group_switch_action(
                        "workload-engine-auto-gpu-enabled-switch",
                        gpu_enabled,
                        cx.listener(|app, checked, _, cx| {
                            app.settings
                                .workload_engine
                                .workload_engine_gpu_priority
                                .enabled = *checked;
                            cx.notify();
                        }),
                    ),
                    self.is_setting_group_collapsed(SettingGroupTarget::WorkloadEngineGpuPriority),
                    vec![
                        setting_group_action_row(
                            "workload-engine-auto-gpu-foreground-priority",
                            t!("workload_engine.priority_foreground_value").to_string(),
                            self.render_workload_engine_gpu_priority_selector(
                                GpuPriorityDefaultTarget::Foreground,
                                settings.workload_engine_gpu_priority.foreground_priority,
                                gpu_enabled,
                                window,
                                cx,
                            ),
                            false,
                        )
                        .into_any_element(),
                        setting_group_action_row(
                            "workload-engine-auto-gpu-background-priority",
                            t!("workload_engine.priority_background_value").to_string(),
                            self.render_workload_engine_gpu_priority_selector(
                                GpuPriorityDefaultTarget::Background,
                                settings.workload_engine_gpu_priority.background_priority,
                                gpu_enabled,
                                window,
                                cx,
                            ),
                            false,
                        )
                        .into_any_element(),
                    ],
                    window,
                    cx,
                )
                .into_any_element(),
            )
            .into_any_element()
    }

    pub(in crate::ui::app) fn render_workload_engine_io_priority_selector(
        &self,
        target: IoPriorityDefaultTarget,
        selected_priority: ProcessIoPrioritySetting,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = match target {
            IoPriorityDefaultTarget::Background => "workload-engine-io-background-priority",
            IoPriorityDefaultTarget::Foreground => "workload-engine-io-foreground-priority",
        };
        let priorities: &[ProcessIoPrioritySetting] =
            if self.settings.advanced.expose_all_priority_values {
                &ProcessIoPrioritySetting::ADVANCED_ALL
            } else {
                &ProcessIoPrioritySetting::ALL
            };
        self.render_dropdown_select(
            id,
            process_io_priority_setting_label(selected_priority),
            enabled,
            DropdownSelectWidth::Standard,
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
                            match target {
                                IoPriorityDefaultTarget::Background => {
                                    app.settings
                                        .workload_engine
                                        .workload_engine_io_priority
                                        .background_priority = priority;
                                }
                                IoPriorityDefaultTarget::Foreground => {
                                    app.settings
                                        .workload_engine
                                        .workload_engine_io_priority
                                        .foreground_priority = priority;
                                }
                            }
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                options
            },
        )
    }

    pub(in crate::ui::app) fn render_workload_engine_thread_priority_selector(
        &self,
        target: ThreadPriorityDefaultTarget,
        selected_priority: ProcessThreadPrioritySetting,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = match target {
            ThreadPriorityDefaultTarget::Background => "workload-engine-thread-background-priority",
            ThreadPriorityDefaultTarget::Foreground => "workload-engine-thread-foreground-priority",
        };
        let priorities: &[ProcessThreadPrioritySetting] =
            if self.settings.advanced.expose_all_priority_values {
                &ProcessThreadPrioritySetting::ADVANCED_ALL
            } else {
                &ProcessThreadPrioritySetting::ALL
            };
        self.render_dropdown_select(
            id,
            process_thread_priority_setting_label(selected_priority),
            enabled,
            DropdownSelectWidth::Standard,
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
                            match target {
                                ThreadPriorityDefaultTarget::Background => {
                                    app.settings
                                        .workload_engine
                                        .workload_engine_thread_priority
                                        .background_priority = priority;
                                }
                                ThreadPriorityDefaultTarget::Foreground => {
                                    app.settings
                                        .workload_engine
                                        .workload_engine_thread_priority
                                        .foreground_priority = priority;
                                }
                            }
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                options
            },
        )
    }

    pub(in crate::ui::app) fn render_workload_engine_dynamic_priority_boost_selector(
        &self,
        target: DynamicPriorityBoostDefaultTarget,
        selected_boost: ProcessDynamicPriorityBoostSetting,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = match target {
            DynamicPriorityBoostDefaultTarget::Background => "workload-engine-boost-background",
            DynamicPriorityBoostDefaultTarget::Foreground => "workload-engine-boost-foreground",
        };
        self.render_dropdown_select(
            id,
            process_dynamic_priority_boost_setting_label(selected_boost),
            enabled,
            DropdownSelectWidth::Standard,
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
                            match target {
                                DynamicPriorityBoostDefaultTarget::Background => {
                                    app.settings
                                        .workload_engine
                                        .workload_engine_dynamic_priority_boost
                                        .background_boost = boost;
                                }
                                DynamicPriorityBoostDefaultTarget::Foreground => {
                                    app.settings
                                        .workload_engine
                                        .workload_engine_dynamic_priority_boost
                                        .foreground_boost = boost;
                                }
                            }
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                options
            },
        )
    }

    pub(in crate::ui::app) fn render_workload_engine_gpu_priority_selector(
        &self,
        target: GpuPriorityDefaultTarget,
        selected_priority: ProcessGpuPrioritySetting,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = match target {
            GpuPriorityDefaultTarget::Background => "workload-engine-gpu-background-priority",
            GpuPriorityDefaultTarget::Foreground => "workload-engine-gpu-foreground-priority",
        };
        let priorities: &[ProcessGpuPrioritySetting] =
            if self.settings.advanced.expose_all_priority_values {
                &ProcessGpuPrioritySetting::ADVANCED_ALL
            } else {
                &ProcessGpuPrioritySetting::ALL
            };
        self.render_dropdown_select(
            id,
            process_gpu_priority_setting_label(selected_priority),
            enabled,
            DropdownSelectWidth::Standard,
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
                            match target {
                                GpuPriorityDefaultTarget::Background => {
                                    app.settings
                                        .workload_engine
                                        .workload_engine_gpu_priority
                                        .background_priority = priority;
                                }
                                GpuPriorityDefaultTarget::Foreground => {
                                    app.settings
                                        .workload_engine
                                        .workload_engine_gpu_priority
                                        .foreground_priority = priority;
                                }
                            }
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                options
            },
        )
    }

    pub(in crate::ui::app) fn render_workload_engine_exclusions_section(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
        input_value: &str,
        enabled: bool,
    ) -> AnyElement {
        v_flex()
            .w_full()
            .min_w(px(0.0))
            .gap_2()
            .child(
                section_header(
                    &t!("workload_engine.workload_engine_exclusions"),
                    t!("workload_engine.workload_engine_exclusions_help").to_string(),
                )
                .into_any_element(),
            )
            .child(
                h_flex()
                    .gap_2()
                    .items_start()
                    .flex_wrap()
                    .child(self.render_process_picker(
                        "workload-engine-exclusion-suggestion",
                        &self.inputs.workload_engine_process,
                        SuggestionTarget::WorkloadEngine,
                        window,
                        cx,
                    ))
                    .child(
                        primary_control_button(Button::new("add-workload-engine-exclusion"), cx)
                            .label(t!("common.add").to_string())
                            .disabled(
                                !enabled
                                    || !can_add_workload_engine_exclusion(
                                        &self.settings.workload_engine,
                                        input_value,
                                    ),
                            )
                            .on_click(cx.listener(|app, _, window, cx| {
                                let process = app
                                    .inputs
                                    .workload_engine_process
                                    .read(cx)
                                    .value()
                                    .to_string();
                                if can_add_workload_engine_exclusion(
                                    &app.settings.workload_engine,
                                    &process,
                                ) {
                                    app.settings
                                        .workload_engine
                                        .workload_engine_exclusions
                                        .push(new_process_exclusion_rule(&process));
                                    clear_input(&app.inputs.workload_engine_process, window, cx);
                                }
                                cx.notify();
                            })),
                    ),
            )
            .child(self.render_workload_engine_exclusions(cx))
            .into_any_element()
    }

    pub(in crate::ui::app) fn render_foreground_boost_selector(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = self.settings.workload_engine.foreground_boost;
        let boost_enabled = self.settings.workload_engine.boost_foreground_app;
        let boost_options: [Option<ForegroundBoostPriority>; 4] = [
            None,
            Some(ForegroundBoostPriority::ALL[0]),
            Some(ForegroundBoostPriority::ALL[1]),
            Some(ForegroundBoostPriority::ALL[2]),
        ];
        let dropdown = self.render_dropdown_select(
            "foreground-boost-priority-select",
            if boost_enabled {
                foreground_boost_priority_label(selected)
            } else {
                t!("common.none").to_string()
            },
            true,
            DropdownSelectWidth::Standard,
            boost_options.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for option in boost_options {
                    let selected_option = match option {
                        Some(priority) => boost_enabled && selected == priority,
                        None => !boost_enabled,
                    };
                    let label = option
                        .map(foreground_boost_priority_label)
                        .unwrap_or_else(|| t!("common.none").to_string());
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!("foreground-boost-option-{option:?}")),
                            label,
                            selected_option,
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            match option {
                                Some(priority) => {
                                    app.settings.workload_engine.boost_foreground_app = true;
                                    app.settings.workload_engine.foreground_boost = priority;
                                }
                                None => {
                                    app.settings.workload_engine.boost_foreground_app = false;
                                }
                            }
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                options
            },
        );
        setting_action_card_with_help(
            "foreground-boost-priority",
            t!("workload_engine.foreground_boost").to_string(),
            t!("workload_engine.foreground_boost_help").to_string(),
            dropdown,
        )
        .into_any_element()
    }

    pub(in crate::ui::app) fn render_workload_engine_background_priority_selector(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = self
            .settings
            .workload_engine
            .workload_engine_background_priority;
        let priorities = [
            ProcessPriority::Normal,
            ProcessPriority::BelowNormal,
            ProcessPriority::Idle,
        ];
        self.render_dropdown_select(
            "workload-engine-background-process-priority-select",
            process_priority_label(selected),
            self.settings.workload_engine.enabled,
            DropdownSelectWidth::Standard,
            priorities.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for priority in priorities {
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!(
                                "workload-engine-background-process-priority-option-{priority:?}"
                            )),
                            process_priority_label(priority),
                            selected == priority,
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            app.settings
                                .workload_engine
                                .workload_engine_background_priority = priority;
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                options
            },
        )
    }

    pub(in crate::ui::app) fn render_workload_engine_escalation_tuning_selector(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected_auto = self
            .settings
            .workload_engine
            .lower_background_auto_cpu_percent;
        self.render_dropdown_select(
            "workload-engine-auto-escalation-tuning",
            workload_engine_escalation_tuning_label(selected_auto),
            true,
            DropdownSelectWidth::Standard,
            2,
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for auto in [true, false] {
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!(
                                "workload-engine-auto-escalation-tuning-option-{auto}"
                            )),
                            workload_engine_escalation_tuning_label(auto),
                            selected_auto == auto,
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            app.settings
                                .workload_engine
                                .lower_background_auto_cpu_percent = auto;
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                options
            },
        )
    }

    pub(in crate::ui::app) fn render_workload_engine_affinity_mode_selector(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = self.settings.workload_engine.workload_engine_affinity_mode;
        self.render_dropdown_select(
            "workload-engine-auto-affinity-mode",
            cpu_restriction_mode_label(selected),
            true,
            DropdownSelectWidth::Standard,
            CpuRestrictionMode::ALL.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for mode in CpuRestrictionMode::ALL {
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!(
                                "workload-engine-auto-affinity-mode-option-{mode:?}"
                            )),
                            cpu_restriction_mode_label(mode),
                            selected == mode,
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            app.settings.workload_engine.workload_engine_affinity_mode = mode;
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                options
            },
        )
    }

    pub(in crate::ui::app) fn render_workload_engine_memory_priority_selector(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = self
            .settings
            .workload_engine
            .workload_engine_memory_priority;
        self.render_dropdown_select(
            "workload-engine-auto-memory-priority-level",
            process_memory_priority_label(selected),
            true,
            DropdownSelectWidth::Standard,
            ProcessMemoryPriority::ALL.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for priority in ProcessMemoryPriority::ALL {
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!(
                                "workload-engine-auto-memory-priority-option-{priority:?}"
                            )),
                            process_memory_priority_label(priority),
                            selected == priority,
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            app.settings.workload_engine.workload_engine_memory_priority = priority;
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                options
            },
        )
    }

    pub(in crate::ui::app) fn render_workload_engine_foreground_memory_priority_selector(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let selected = self
            .settings
            .workload_engine
            .workload_engine_foreground_memory_priority;
        self.render_dropdown_select(
            "workload-engine-auto-foreground-memory-priority-level",
            process_memory_priority_setting_label(selected),
            true,
            DropdownSelectWidth::Standard,
            ProcessMemoryPrioritySetting::ALL.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for priority in ProcessMemoryPrioritySetting::ALL {
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!(
                                "workload-engine-auto-foreground-memory-priority-option-{priority:?}"
                            )),
                            process_memory_priority_setting_label(priority),
                            selected == priority,
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            app.settings
                                .workload_engine
                                .workload_engine_foreground_memory_priority = priority;
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                options
            },
        )
    }

    pub(in crate::ui::app) fn render_workload_engine_exclusions(
        &self,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.render_process_exclusion_list(
            &self.settings.workload_engine.workload_engine_exclusions,
            ListItemRemovalKind::WorkloadEngineExclusion,
            "workload-engine-exclusion",
            text_muted(t!("workload_engine.no_workload_engine_exclusions").to_string())
                .into_any_element(),
            cx,
        )
    }
}
