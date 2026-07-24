use crate::ui::app::*;

impl WinderustApp {
    pub(in crate::ui::app) fn render_background_cpu_restriction_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let input_value = self
            .inputs
            .background_cpu_exclusion
            .read(cx)
            .value()
            .to_string();
        let settings = &self.settings.background_cpu_restriction;
        let enabled = settings.enabled;
        let processors = core_steering::logical_processors();
        let has_efficiency_cores =
            core_steering_processors_kind_mask(&processors, LogicalProcessorKind::Efficiency) != 0;
        let has_multiple_processors = processors.len() > 1;
        let selected = self.effective_background_cpu_restriction_strategy();
        let restriction_enabled = selected != CpuRestrictionStrategy::Off;
        let available_mask = core_steering_processors_mask(&processors);

        let selected_mode = settings.mode;
        let mode_dropdown = self.render_dropdown_select(
            "background-cpu-mode",
            cpu_restriction_mode_label(selected_mode),
            restriction_enabled,
            DropdownSelectWidth::Standard,
            CpuRestrictionMode::ALL.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for mode in CpuRestrictionMode::ALL {
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!("background-cpu-mode-option-{mode:?}")),
                            cpu_restriction_mode_label(mode),
                            selected_mode == mode,
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            app.settings.background_cpu_restriction.mode = mode;
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                options
            },
        );

        let strategy_options = [
            CpuRestrictionStrategy::Auto,
            CpuRestrictionStrategy::PreferEfficiencyCores,
            CpuRestrictionStrategy::LimitLogicalCpus,
        ];
        let strategy_dropdown = self.render_dropdown_select(
            "background-cpu-strategy",
            cpu_restriction_strategy_label(selected),
            restriction_enabled,
            DropdownSelectWidth::Wide,
            strategy_options.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for strategy in strategy_options {
                    let option_enabled = match strategy {
                        CpuRestrictionStrategy::PreferEfficiencyCores => has_efficiency_cores,
                        CpuRestrictionStrategy::LimitLogicalCpus => has_multiple_processors,
                        CpuRestrictionStrategy::Auto | CpuRestrictionStrategy::Off => true,
                    };
                    let row = dropdown_option_row(
                        SharedString::from(format!("background-cpu-strategy-option-{strategy:?}")),
                        cpu_restriction_strategy_label(strategy),
                        selected == strategy,
                        cx,
                    )
                    .when(!option_enabled, |row| row.opacity(0.48).cursor_default());
                    let row = if option_enabled {
                        row.on_click(cx.listener(move |app, _, _, cx| {
                            app.settings.background_cpu_restriction.strategy = strategy;
                            if app.settings.background_cpu_restriction.control_style
                                == CpuRestrictionControlStyle::CoreToggle
                            {
                                let processors = core_steering::logical_processors();
                                let mask =
                                    background_efficiency_strategy_core_mask(&processors, strategy);
                                if mask != 0 {
                                    app.settings.background_cpu_restriction.core_mask = mask;
                                }
                            }
                            app.active_power_plan_picker = None;
                            cx.notify();
                        }))
                    } else {
                        row
                    };
                    options = options.child(row);
                }
                options
            },
        );

        let selected_style = settings.control_style;
        let style_dropdown = self.render_dropdown_select(
            "background-cpu-style",
            cpu_restriction_control_style_label(selected_style),
            restriction_enabled,
            DropdownSelectWidth::Standard,
            CpuRestrictionControlStyle::ALL.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for style in CpuRestrictionControlStyle::ALL {
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!("background-cpu-style-option-{style:?}")),
                            cpu_restriction_control_style_label(style),
                            selected_style == style,
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            app.settings.background_cpu_restriction.control_style = style;
                            if style == CpuRestrictionControlStyle::CoreToggle
                                && app.settings.background_cpu_restriction.core_mask == 0
                            {
                                let processors = core_steering::logical_processors();
                                let strategy = app.effective_background_cpu_restriction_strategy();
                                app.settings.background_cpu_restriction.core_mask =
                                    background_efficiency_strategy_core_mask(&processors, strategy);
                            }
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                options
            },
        );

        let percent = settings.percent.clamp(1, 100);
        let percentage_control = self.render_numeric_value(
            NumericField::BackgroundCpuRestrictionPercent,
            format!("{percent}%"),
            percent.to_string(),
            cx,
        );

        let mut rows = vec![
            setting_group_action_row(
                "background-core-steering-control",
                t!("background_cpu.core_affinity_control").to_string(),
                mode_dropdown,
                true,
            )
            .into_any_element(),
            setting_group_action_row(
                "background-cpu-suppression-rule",
                t!("background_cpu.core_suppression_rule").to_string(),
                strategy_dropdown,
                true,
            )
            .into_any_element(),
            setting_group_action_row(
                "background-cpu-control-style",
                t!("background_cpu.control_style").to_string(),
                style_dropdown,
                true,
            )
            .into_any_element(),
        ];
        rows.push(match settings.control_style {
            CpuRestrictionControlStyle::Percentage => setting_group_action_row(
                "background-cpu-percent",
                t!("background_cpu.core_allocation_percentage").to_string(),
                percentage_control,
                true,
            )
            .into_any_element(),
            CpuRestrictionControlStyle::CoreToggle => setting_group_stacked_action_row(
                "background-cpu-core-toggle-list",
                t!("background_cpu.selected_cores").to_string(),
                self.render_core_tile_grid(
                    &processors,
                    settings.core_mask,
                    restriction_enabled,
                    "background-cpu-core-toggle",
                    CoreTileGridAction::BackgroundCpuRestriction { available_mask },
                    cx,
                ),
                true,
            )
            .into_any_element(),
        });
        let body_animation_height = match settings.control_style {
            CpuRestrictionControlStyle::Percentage => CARD_ROW_HEIGHT * rows.len().max(1) as f32,
            CpuRestrictionControlStyle::CoreToggle => setting_group_core_grid_body_height(3),
        };

        let body = feature_body(enabled)
            .child(feature_toggle_switch_with_help(
                "background-cpu-foreground",
                t!("background_cpu.focus_detection").to_string(),
                t!("background_cpu.focus_detection_help").to_string(),
                settings.exclude_foreground_app,
                cx.listener(|app, checked, _, cx| {
                    app.settings
                        .background_cpu_restriction
                        .exclude_foreground_app = *checked;
                    cx.notify();
                }),
            ))
            .child(setting_group_with_title_element_with_body_height(
                SettingGroupTarget::BackgroundCpuRestriction,
                div()
                    .min_w(px(0.0))
                    .truncate()
                    .child(t!("background_cpu.cpu_restriction").to_string())
                    .into_any_element(),
                setting_group_switch_action(
                    "background-cpu-restriction-enabled",
                    restriction_enabled,
                    cx.listener(|app, checked, _, cx| {
                        app.settings.background_cpu_restriction.strategy = if *checked {
                            CpuRestrictionStrategy::Auto
                        } else {
                            CpuRestrictionStrategy::Off
                        };
                        cx.notify();
                    }),
                ),
                SettingGroupBody {
                    collapsed: self
                        .is_setting_group_collapsed(SettingGroupTarget::BackgroundCpuRestriction),
                    rows,
                    animation_height: Some(body_animation_height),
                },
                window,
                cx,
            ))
            .child(stat_grid(vec![
                (
                    t!("background_cpu.adjusted_processes").to_string(),
                    self.background_cpu_restriction_status
                        .adjusted_processes
                        .to_string(),
                ),
                (
                    t!("background_cpu.scanned_processes").to_string(),
                    self.background_cpu_restriction_status
                        .scanned_processes
                        .to_string(),
                ),
                (
                    t!("background_cpu.skipped_processes").to_string(),
                    self.background_cpu_restriction_status
                        .skipped_processes
                        .to_string(),
                ),
                (
                    t!("background_cpu.failed_actions").to_string(),
                    self.background_cpu_restriction_status
                        .failed_processes
                        .to_string(),
                ),
            ]))
            .child(section_header(
                &t!("background_cpu.exclusions"),
                t!("background_cpu.exclusions_help").to_string(),
            ))
            .child(
                h_flex()
                    .gap_2()
                    .items_start()
                    .flex_wrap()
                    .child(self.render_process_picker(
                        "background-cpu-exclusion",
                        &self.inputs.background_cpu_exclusion,
                        SuggestionTarget::BackgroundCpu,
                        window,
                        cx,
                    ))
                    .child(
                        primary_control_button(Button::new("add-background-cpu-exclusion"), cx)
                            .label(t!("common.add").to_string())
                            .disabled(
                                !enabled
                                    || !can_add_background_cpu_exclusion(
                                        &self.settings.background_cpu_restriction,
                                        &input_value,
                                    ),
                            )
                            .on_click(cx.listener(|app, _, window, cx| {
                                let process = app
                                    .inputs
                                    .background_cpu_exclusion
                                    .read(cx)
                                    .value()
                                    .to_string();
                                if can_add_background_cpu_exclusion(
                                    &app.settings.background_cpu_restriction,
                                    &process,
                                ) {
                                    app.settings
                                        .background_cpu_restriction
                                        .exclusions
                                        .push(new_process_exclusion_rule(&process));
                                    clear_input(&app.inputs.background_cpu_exclusion, window, cx);
                                }
                                cx.notify();
                            })),
                    ),
            )
            .child(self.render_background_cpu_exclusions(cx));

        self.page_shell(Page::BackgroundCpuRestriction, cx)
            .child(feature_toggle_switch_with_help(
                "background-cpu-enabled",
                t!("background_cpu.enable").to_string(),
                tooltip_lines(vec![
                    t!("background_cpu.intro_1").to_string(),
                    t!("background_cpu.intro_2").to_string(),
                ]),
                enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings.background_cpu_restriction.enabled = *checked;
                    cx.notify();
                }),
            ))
            .child(disabled_feature_body(
                "background-cpu-body",
                body,
                enabled,
                cx,
            ))
            .into_any_element()
    }

    pub(in crate::ui::app) fn render_background_cpu_exclusions(
        &self,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.render_process_exclusion_list(
            &self.settings.background_cpu_restriction.exclusions,
            ListItemRemovalKind::BackgroundCpuExclusion,
            "background-cpu-exclusion",
            text_muted(t!("background_cpu.no_exclusions").to_string())
                .p_4()
                .into_any_element(),
            cx,
        )
    }
}
