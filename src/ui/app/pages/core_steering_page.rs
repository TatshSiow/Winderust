use crate::ui::app::*;

impl WinderustApp {
    pub(in crate::ui::app) fn render_core_steering_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let input_value = self
            .inputs
            .core_steering_process
            .read(cx)
            .value()
            .to_string();
        let enabled = self.settings.core_steering.enabled;
        let body = feature_body(enabled)
            .child(feature_toggle_switch_with_help(
                "core-steering-foreground",
                t!("core_steering.focus_detection").to_string(),
                t!("core_steering.focus_detection_help").to_string(),
                self.settings.core_steering.exclude_foreground_app,
                cx.listener(|app, checked, _, cx| {
                    app.settings.core_steering.exclude_foreground_app = *checked;
                    cx.notify();
                }),
            ))
            .child(section_header(
                &t!("core_steering.rules"),
                t!("core_steering.rules_help").to_string(),
            ))
            .child(
                h_flex()
                    .gap_2()
                    .items_start()
                    .flex_wrap()
                    .child(self.render_process_picker(
                        "affinity-suggestion",
                        &self.inputs.core_steering_process,
                        SuggestionTarget::CoreSteering,
                        window,
                        cx,
                    ))
                    .child(
                        primary_control_button(Button::new("add-affinity-process"), cx)
                            .label(t!("common.add").to_string())
                            .disabled(
                                !enabled
                                    || !can_add_core_steering_process(
                                        &self.settings.core_steering,
                                        &input_value,
                                    ),
                            )
                            .on_click(cx.listener(|app, _, window, cx| {
                                let process = app
                                    .inputs
                                    .core_steering_process
                                    .read(cx)
                                    .value()
                                    .to_string();
                                if can_add_core_steering_process(
                                    &app.settings.core_steering,
                                    &process,
                                ) {
                                    app.settings
                                        .core_steering
                                        .rules
                                        .push(new_core_steering_rule(&process));
                                    clear_input(&app.inputs.core_steering_process, window, cx);
                                }
                                cx.notify();
                            })),
                    ),
            )
            .child(self.render_core_steering_rules(window, cx));

        let help = tooltip_lines(vec![
            t!("core_steering.intro_1").to_string(),
            t!("core_steering.intro_2").to_string(),
            t!("core_steering.intro_3").to_string(),
        ]);

        self.page_shell(Page::CoreSteering, cx)
            .child(feature_toggle_switch_with_help(
                "core-steering-enabled",
                t!("core_steering.enable").to_string(),
                help,
                enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings.core_steering.enabled = *checked;
                    cx.notify();
                }),
            ))
            .child(disabled_feature_body(
                "core-steering-body",
                body,
                enabled,
                cx,
            ))
            .into_any_element()
    }

    pub(in crate::ui::app) fn render_core_steering_rules(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut list = rule_list(process_rule_table_headers());
        for (index, rule) in self.settings.core_steering.rules.iter().enumerate() {
            let process = rule.process_name.clone();
            let indicator = core_steering_indicator(&self.core_steering_status, &process);
            let card_target = RuleCardTarget::CoreSteering(process.clone());
            let collapsed = self.is_rule_card_collapsed(&card_target);
            let mut card = rule_card(
                self.process_rule_title(&process, cx),
                rule_active_cell(
                    format!("affinity-rule-enabled-{index}"),
                    rule.enabled,
                    cx.listener(move |app, checked, _, cx| {
                        if let Some(rule) = app.settings.core_steering.rules.get_mut(index) {
                            rule.enabled = *checked;
                        }
                        cx.notify();
                    }),
                ),
                rule_card_collapse_indicator(card_target.clone(), collapsed),
                card_target.clone(),
                collapsed,
                cx,
            );
            if rule_card_body_visible(&card_target, collapsed, window) {
                card = card
                    .child(animated_rule_card_body_child(
                        &card_target,
                        0,
                        1,
                        rule_card_body_row(vec![rule_action_row(
                            format!("affinity-rule-status-{index}"),
                            t!("common.status").to_string(),
                            h_flex()
                                .items_center()
                                .justify_end()
                                .gap_2()
                                .min_w(px(0.0))
                                .flex_wrap()
                                .child(status_pill(indicator.label, indicator.bg, indicator.fg))
                                .child(text_muted(indicator.hover))
                                .into_any_element(),
                        )
                        .into_any_element()]),
                    ))
                    .child(animated_rule_card_body_child(
                        &card_target,
                        1,
                        1,
                        rule_card_body_row(vec![
                            self.render_core_steering_mode_selector(index, rule.mode, window, cx)
                        ]),
                    ));
                let mut body_index = 2;
                if rule.mode != CoreSteeringMode::EfficiencyOff {
                    card = card.child(animated_rule_card_body_child_with_height(
                        &card_target,
                        body_index,
                        core_steering_selector_body_height(),
                        rule_card_body_row(vec![self.render_core_steering_core_selector(
                            index,
                            rule.core_mask,
                            window,
                            cx,
                        )]),
                    ));
                    body_index += 1;
                }
                card = card.child(animated_rule_card_body_child(
                    &card_target,
                    body_index,
                    1,
                    rule_card_body_action(
                        remove_control_button(Button::new(SharedString::from(format!(
                            "remove-affinity-{index}"
                        ))))
                        .on_click(cx.listener({
                            move |app, _, _, cx| {
                                app.request_list_item_removal(
                                    ListItemRemovalTarget::new(
                                        ListItemRemovalKind::CoreSteeringRule,
                                        index,
                                    ),
                                    cx,
                                );
                            }
                        }))
                        .into_any_element(),
                    ),
                ));
            }
            list = list.child(self.animated_list_item(
                ListItemRemovalTarget::new(ListItemRemovalKind::CoreSteeringRule, index),
                SharedString::from(format!("affinity-rule-{index}")),
                card.into_any_element(),
            ));
        }
        if self.settings.core_steering.rules.is_empty() {
            list = list.child(text_muted(t!("core_steering.no_rules").to_string()).p_4());
        }
        list.into_any_element()
    }

    pub(in crate::ui::app) fn render_core_steering_mode_selector(
        &self,
        index: usize,
        selected_mode: CoreSteeringMode,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let dropdown = self.render_dropdown_select(
            format!("affinity-mode-{index}"),
            core_steering_mode_label(selected_mode),
            true,
            DropdownSelectWidth::Standard,
            CoreSteeringMode::ALL.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for mode in CoreSteeringMode::ALL {
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!("affinity-mode-{index}-option-{mode:?}")),
                            core_steering_mode_label(mode),
                            selected_mode == mode,
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            if let Some(rule) = app.settings.core_steering.rules.get_mut(index) {
                                rule.mode = mode;
                            }
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                options
            },
        );
        rule_action_row(
            format!("affinity-mode-row-{index}"),
            t!("core_steering.mode").to_string(),
            dropdown,
        )
        .into_any_element()
    }

    pub(in crate::ui::app) fn render_core_steering_core_selector(
        &self,
        index: usize,
        core_mask: u64,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let processors = core_steering::logical_processors();
        let all_mask = core_steering_processors_mask(&processors);
        let performance_mask =
            core_steering_processors_kind_mask(&processors, LogicalProcessorKind::Performance);
        let efficiency_mask =
            core_steering_processors_kind_mask(&processors, LogicalProcessorKind::Efficiency);
        let no_smt_mask = core_steering_processors_no_smt_mask(&processors);

        let preset_options = vec![
            (t!("core_steering.all").to_string(), all_mask, all_mask != 0),
            (
                t!("core_steering.p_cores").to_string(),
                performance_mask,
                performance_mask != 0,
            ),
            (
                t!("core_steering.e_cores").to_string(),
                efficiency_mask,
                efficiency_mask != 0,
            ),
            (
                t!("core_steering.no_smt").to_string(),
                no_smt_mask,
                no_smt_mask != 0 && no_smt_mask != all_mask,
            ),
        ];
        let selected_preset_label = preset_options
            .iter()
            .find(|(_, mask, enabled)| *enabled && core_mask == *mask)
            .map(|(label, _, _)| label.clone())
            .unwrap_or_else(|| "Custom".to_owned());
        let preset_count = preset_options.len();
        let presets_dropdown = self.render_dropdown_select(
            format!("affinity-core-preset-{index}"),
            selected_preset_label,
            true,
            DropdownSelectWidth::Standard,
            preset_count,
            window,
            cx,
            move |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for (option_index, (label, mask, enabled)) in preset_options.into_iter().enumerate()
                {
                    let row = dropdown_option_row(
                        SharedString::from(format!(
                            "affinity-core-preset-{index}-option-{option_index}"
                        )),
                        label,
                        enabled && core_mask == mask,
                        cx,
                    )
                    .when(!enabled, |row| row.opacity(0.48).cursor_default());
                    let row = if enabled {
                        row.on_click(cx.listener(move |app, _, _, cx| {
                            if mask != 0 {
                                if let Some(rule) = app.settings.core_steering.rules.get_mut(index)
                                {
                                    rule.core_mask = mask;
                                }
                                app.active_power_plan_picker = None;
                                cx.notify();
                            }
                        }))
                    } else {
                        row
                    };
                    options = options.child(row);
                }
                options
            },
        );

        let core_grid = self.render_core_tile_grid(
            &processors,
            core_mask,
            true,
            format!("affinity-core-{index}"),
            CoreTileGridAction::CoreSteeringRule { index },
            cx,
        );

        v_flex()
            .w_full()
            .min_w(px(0.0))
            .child(
                rule_action_row(
                    format!("affinity-core-presets-row-{index}"),
                    t!("core_steering.core_presets").to_string(),
                    presets_dropdown,
                )
                .into_any_element(),
            )
            .child(
                setting_group_stacked_action_row(
                    format!("affinity-core-row-{index}"),
                    t!("core_steering.allowed_cpus").to_string(),
                    core_grid,
                    true,
                )
                .into_any_element(),
            )
            .into_any_element()
    }

    pub(in crate::ui::app) fn render_core_tile_grid(
        &self,
        processors: &[LogicalProcessorInfo],
        core_mask: u64,
        enabled: bool,
        id_prefix: impl Into<String>,
        action: CoreTileGridAction,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if processors.is_empty() {
            return text_muted(t!("core_steering.no_logical_cpus").to_string()).into_any_element();
        }

        let id_prefix = id_prefix.into();
        let mut grid = v_flex().w_full().min_w(px(0.0)).gap_1();
        let mut current_row = h_flex().w_full().min_w(px(0.0)).gap_1();
        let mut cells_in_row = 0;

        for processor in processors {
            let core = processor.index;
            let selected = affinity_mask_contains(core_mask, core);
            let tile_text_color: Hsla = if selected {
                cx.theme().primary_foreground
            } else {
                rgb(primary_text_color()).into()
            };
            let tile_muted_text_color: Hsla = if selected {
                cx.theme().primary_foreground
            } else {
                rgb(muted_text_color()).into()
            };
            let tile_variant = ButtonCustomVariant::new(cx)
                .color(
                    rgb(if selected {
                        accent_color()
                    } else {
                        settings_card_color()
                    })
                    .into(),
                )
                .foreground(tile_text_color)
                .border(
                    rgb(if selected {
                        accent_color()
                    } else {
                        border_color()
                    })
                    .into(),
                )
                .hover(if selected {
                    cx.theme().primary_hover
                } else {
                    cx.theme().secondary_hover
                })
                .active(if selected {
                    cx.theme().primary_active
                } else {
                    cx.theme().secondary_active
                });
            current_row = current_row.child(
                div().flex_1().min_w(px(0.0)).child(
                    Button::new(SharedString::from(format!("{id_prefix}-{core}")))
                        .custom(tile_variant)
                        .rounded(px(4.0))
                        .w_full()
                        .min_w(px(0.0))
                        .h(px(CORE_TILE_HEIGHT))
                        .disabled(!enabled)
                        .on_click(cx.listener(move |app, _, _, cx| {
                            match action {
                                CoreTileGridAction::BackgroundCpuRestriction { available_mask } => {
                                    toggle_affinity_core_with_available_mask(
                                        &mut app.settings.background_cpu_restriction.core_mask,
                                        core,
                                        available_mask,
                                    );
                                }
                                CoreTileGridAction::CoreSteeringRule { index } => {
                                    if let Some(rule) =
                                        app.settings.core_steering.rules.get_mut(index)
                                    {
                                        toggle_affinity_core(&mut rule.core_mask, core);
                                    }
                                }
                            }
                            cx.notify();
                        }))
                        .child(
                            v_flex()
                                .items_center()
                                .justify_center()
                                .gap(px(1.0))
                                .child(
                                    div()
                                        .text_size(px(10.0))
                                        .line_height(px(12.0))
                                        .text_color(tile_muted_text_color)
                                        .child(core_tile_kind_label(processor)),
                                )
                                .child(
                                    div()
                                        .text_size(px(TEXT_CONTROL_SIZE))
                                        .line_height(px(TEXT_CONTROL_LINE_HEIGHT))
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(tile_text_color)
                                        .child(format!("CPU {}", processor.index)),
                                ),
                        ),
                ),
            );
            cells_in_row += 1;
            if cells_in_row == CORE_TILE_GRID_COLUMNS {
                grid = grid.child(current_row);
                current_row = h_flex().w_full().min_w(px(0.0)).gap_1();
                cells_in_row = 0;
            }
        }

        if cells_in_row > 0 {
            for _ in cells_in_row..CORE_TILE_GRID_COLUMNS {
                current_row = current_row.child(div().flex_1().min_w(px(0.0)));
            }
            grid = grid.child(current_row);
        }

        grid.into_any_element()
    }
}
