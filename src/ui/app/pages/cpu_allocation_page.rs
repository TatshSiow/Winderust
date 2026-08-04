use crate::config::CpuAllocationSettings;
use crate::ui::app::*;

#[derive(Clone, Copy)]
enum CpuAllocationPage {
    CpuSetsSoft,
    ProcessorAffinityHard,
}

impl CpuAllocationPage {
    fn page(self) -> Page {
        match self {
            Self::CpuSetsSoft => Page::CpuSetsSoft,
            Self::ProcessorAffinityHard => Page::ProcessorAffinityHard,
        }
    }

    fn suggestion_target(self) -> SuggestionTarget {
        match self {
            Self::CpuSetsSoft => SuggestionTarget::CpuSetsSoft,
            Self::ProcessorAffinityHard => SuggestionTarget::ProcessorAffinityHard,
        }
    }

    fn removal_kind(self) -> ListItemRemovalKind {
        match self {
            Self::CpuSetsSoft => ListItemRemovalKind::CpuSetsSoftRule,
            Self::ProcessorAffinityHard => ListItemRemovalKind::ProcessorAffinityHardRule,
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::CpuSetsSoft => "cpu_sets_soft",
            Self::ProcessorAffinityHard => "processor_affinity_hard",
        }
    }
}

impl WinderustApp {
    pub(in crate::ui::app) fn render_cpu_sets_soft_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.render_cpu_allocation_page(CpuAllocationPage::CpuSetsSoft, window, cx)
    }

    pub(in crate::ui::app) fn render_processor_affinity_hard_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.render_cpu_allocation_page(CpuAllocationPage::ProcessorAffinityHard, window, cx)
    }

    fn render_cpu_allocation_page(
        &self,
        kind: CpuAllocationPage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let enabled = cpu_allocation_enabled(&self.settings, kind);
        let input = kind.suggestion_target().input(&self.inputs);
        let input_value = self.process_picker_path(kind.suggestion_target(), input, cx);
        let key = kind.key();
        let mut body = feature_body(enabled)
            .child(feature_toggle_switch_with_help(
                SharedString::from(format!("{key}-foreground")),
                t!("common.protect_foreground_app").to_string(),
                t!("common.protect_foreground_app_help").to_string(),
                cpu_allocation_protects_foreground(&self.settings, kind),
                cx.listener(move |app, checked, _, cx| {
                    set_cpu_allocation_protects_foreground(&mut app.settings, kind, *checked);
                    cx.notify();
                }),
            ))
            .child(feature_toggle_switch_with_help(
                SharedString::from(format!("{key}-visible-windows")),
                t!("common.protect_visible_window_apps").to_string(),
                t!("common.protect_visible_window_apps_help").to_string(),
                cpu_allocation_protects_visible_window_apps(&self.settings, kind),
                cx.listener(move |app, checked, _, cx| {
                    set_cpu_allocation_protects_visible_window_apps(
                        &mut app.settings,
                        kind,
                        *checked,
                    );
                    cx.notify();
                }),
            ))
            .child(section_header(
                &t!("cpu_allocation.rules"),
                t!("cpu_allocation.rules_help").to_string(),
            ));
        if matches!(kind, CpuAllocationPage::CpuSetsSoft)
            && cpu_allocation::has_multiple_processor_groups()
        {
            body = body.child(text_warning(t!("cpu_sets_soft.warning").to_string()));
        } else if matches!(kind, CpuAllocationPage::ProcessorAffinityHard) {
            body = body.child(text_warning(
                t!("processor_affinity_hard.warning").to_string(),
            ));
        }
        body = body
            .child(
                h_flex()
                    .gap_2()
                    .items_start()
                    .flex_wrap()
                    .child(self.render_process_picker(
                        SharedString::from(format!("{key}-suggestion")),
                        input,
                        kind.suggestion_target(),
                        window,
                        cx,
                    ))
                    .child(
                        primary_control_button(
                            Button::new(SharedString::from(format!("add-{key}-process"))),
                            cx,
                        )
                        .label(t!("common.add").to_string())
                        .disabled(
                            !enabled
                                || !can_add_cpu_allocation_process(&self.settings, &input_value),
                        )
                        .on_click(cx.listener(
                            move |app, _, window, cx| {
                                let input = kind.suggestion_target().input(&app.inputs);
                                let process =
                                    app.process_picker_path(kind.suggestion_target(), input, cx);
                                if can_add_cpu_allocation_process(&app.settings, &process) {
                                    cpu_allocation_rules_mut(&mut app.settings, kind)
                                        .push(new_cpu_allocation_rule(&process));
                                    clear_input(input, window, cx);
                                }
                                cx.notify();
                            },
                        )),
                    ),
            )
            .child(self.render_cpu_allocation_rules(kind, window, cx));

        let help = tooltip_lines(vec![
            t!(format!("{key}.intro_1")).to_string(),
            t!(format!("{key}.intro_2")).to_string(),
        ]);
        self.page_shell(kind.page(), cx)
            .child(feature_toggle_switch_with_help(
                SharedString::from(format!("{key}-enabled")),
                t!(format!("{key}.enable")).to_string(),
                help,
                enabled,
                cx.listener(move |app, checked, _, cx| {
                    set_cpu_allocation_enabled(&mut app.settings, kind, *checked);
                    cx.notify();
                }),
            ))
            .child(disabled_feature_body(
                SharedString::from(format!("{key}-body")),
                body,
                enabled,
                cx,
            ))
            .into_any_element()
    }

    fn render_cpu_allocation_rules(
        &self,
        kind: CpuAllocationPage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let rules = cpu_allocation_rules(&self.settings, kind);
        let status = match kind {
            CpuAllocationPage::CpuSetsSoft => &self.cpu_sets_soft_status,
            CpuAllocationPage::ProcessorAffinityHard => &self.processor_affinity_hard_status,
        };
        let processors = cpu_allocation::logical_processors();
        let key = kind.key();
        let mut list = rule_list(process_rule_table_headers());
        for (index, rule) in rules.iter().enumerate() {
            let process = rule.executable_path.clone();
            let indicator = cpu_allocation_indicator(status, &process);
            let card_target = match kind {
                CpuAllocationPage::CpuSetsSoft => RuleCardTarget::CpuSetsSoft(process.clone()),
                CpuAllocationPage::ProcessorAffinityHard => {
                    RuleCardTarget::ProcessorAffinityHard(process.clone())
                }
            };
            let collapsed = self.is_rule_card_collapsed(&card_target);
            let mut card = rule_card(
                self.process_rule_title(&process, cx),
                rule_active_cell(
                    format!("{key}-rule-enabled-{index}"),
                    rule.enabled,
                    cx.listener(move |app, checked, _, cx| {
                        if let Some(rule) =
                            cpu_allocation_rules_mut(&mut app.settings, kind).get_mut(index)
                        {
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
                            format!("{key}-rule-status-{index}"),
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
                    .child(animated_rule_card_body_child_with_height(
                        &card_target,
                        1,
                        cpu_allocation_selector_body_height(processors.len()),
                        rule_card_body_row(vec![self.render_cpu_allocation_core_selector(
                            kind,
                            index,
                            rule.core_mask,
                            &processors,
                            window,
                            cx,
                        )]),
                    ))
                    .child(animated_rule_card_body_child(
                        &card_target,
                        2,
                        1,
                        rule_card_body_action(
                            remove_control_button(Button::new(SharedString::from(format!(
                                "remove-{key}-{index}"
                            ))))
                            .on_click(cx.listener(move |app, _, _, cx| {
                                app.request_list_item_removal(
                                    ListItemRemovalTarget::new(kind.removal_kind(), index),
                                    cx,
                                );
                            }))
                            .into_any_element(),
                        ),
                    ));
            }
            list = list.child(self.animated_list_item(
                ListItemRemovalTarget::new(kind.removal_kind(), index),
                SharedString::from(format!("{key}-rule-{index}")),
                card.into_any_element(),
            ));
        }
        if rules.is_empty() {
            list = list.child(text_muted(t!("cpu_allocation.no_rules").to_string()).p_4());
        }
        list.into_any_element()
    }

    fn render_cpu_allocation_core_selector(
        &self,
        kind: CpuAllocationPage,
        index: usize,
        core_mask: u64,
        processors: &[LogicalProcessorInfo],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let all_mask = cpu_allocation_processors_mask(processors);
        let performance_mask =
            cpu_allocation_processors_kind_mask(processors, LogicalProcessorKind::Performance);
        let efficiency_mask =
            cpu_allocation_processors_kind_mask(processors, LogicalProcessorKind::Efficiency);
        let no_smt_mask = cpu_allocation_processors_no_smt_mask(processors);
        let preset_options = vec![
            (
                t!("cpu_allocation.all").to_string(),
                all_mask,
                all_mask != 0,
            ),
            (
                t!("cpu_allocation.p_cores").to_string(),
                performance_mask,
                performance_mask != 0,
            ),
            (
                t!("cpu_allocation.e_cores").to_string(),
                efficiency_mask,
                efficiency_mask != 0,
            ),
            (
                t!("cpu_allocation.no_smt").to_string(),
                no_smt_mask,
                no_smt_mask != 0 && no_smt_mask != all_mask,
            ),
        ];
        let selected = preset_options
            .iter()
            .find(|(_, mask, available)| *available && core_mask == *mask)
            .map(|(label, _, _)| label.clone())
            .unwrap_or_else(|| t!("cpu_allocation.custom").to_string());
        let key = kind.key();
        let presets = self.render_dropdown_select(
            format!("{key}-core-preset-{index}"),
            selected,
            true,
            DropdownSelectWidth::Standard,
            preset_options.len(),
            window,
            cx,
            move |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for (option_index, (label, mask, available)) in
                    preset_options.into_iter().enumerate()
                {
                    let row = dropdown_option_row(
                        SharedString::from(format!(
                            "{key}-core-preset-{index}-option-{option_index}"
                        )),
                        label,
                        available && core_mask == mask,
                        cx,
                    )
                    .when(!available, |row| row.opacity(0.48).cursor_default());
                    options = options.child(if available {
                        row.on_click(cx.listener(move |app, _, _, cx| {
                            if let Some(rule) =
                                cpu_allocation_rules_mut(&mut app.settings, kind).get_mut(index)
                            {
                                rule.core_mask = mask;
                            }
                            app.active_power_plan_picker = None;
                            cx.notify();
                        }))
                    } else {
                        row
                    });
                }
                options
            },
        );
        let action = match kind {
            CpuAllocationPage::CpuSetsSoft => CoreTileGridAction::CpuSetsSoftRule { index },
            CpuAllocationPage::ProcessorAffinityHard => {
                CoreTileGridAction::ProcessorAffinityHardRule { index }
            }
        };
        v_flex()
            .w_full()
            .min_w(px(0.0))
            .child(
                rule_action_row(
                    format!("{key}-core-presets-row-{index}"),
                    t!("cpu_allocation.core_presets").to_string(),
                    presets,
                )
                .into_any_element(),
            )
            .child(
                setting_group_stacked_action_row(
                    format!("{key}-core-row-{index}"),
                    t!("cpu_allocation.allowed_cpus").to_string(),
                    self.render_core_tile_grid(
                        processors,
                        core_mask,
                        format!("{key}-core-{index}"),
                        action,
                        cx,
                    ),
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
        id_prefix: impl Into<String>,
        action: CoreTileGridAction,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        if processors.is_empty() {
            return text_muted(t!("cpu_allocation.no_logical_cpus").to_string()).into_any_element();
        }
        let id_prefix = id_prefix.into();
        let mut grid = v_flex().w_full().min_w(px(0.0)).gap_1();
        let mut current_row = h_flex().w_full().min_w(px(0.0)).gap_1();
        let mut cells_in_row = 0;
        for processor in processors {
            let core = processor.index;
            let selected = affinity_mask_contains(core_mask, core);
            let foreground: Hsla = if selected {
                cx.theme().primary_foreground
            } else {
                rgb(primary_text_color()).into()
            };
            let muted: Hsla = if selected {
                cx.theme().primary_foreground
            } else {
                rgb(muted_text_color()).into()
            };
            let variant = ButtonCustomVariant::new(cx)
                .color(
                    rgb(if selected {
                        accent_color()
                    } else {
                        settings_card_color()
                    })
                    .into(),
                )
                .foreground(foreground)
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
                        .custom(variant)
                        .rounded(px(4.0))
                        .w_full()
                        .min_w(px(0.0))
                        .h(px(CORE_TILE_HEIGHT))
                        .on_click(cx.listener(move |app, _, _, cx| {
                            let rule = match action {
                                CoreTileGridAction::CpuSetsSoftRule { index } => {
                                    app.settings.cpu_sets_soft.rules.get_mut(index)
                                }
                                CoreTileGridAction::ProcessorAffinityHardRule { index } => {
                                    app.settings.processor_affinity_hard.rules.get_mut(index)
                                }
                            };
                            if let Some(rule) = rule {
                                toggle_affinity_core(&mut rule.core_mask, core);
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
                                        .text_color(muted)
                                        .child(core_tile_kind_label(processor)),
                                )
                                .child(
                                    div()
                                        .text_size(px(TEXT_CONTROL_SIZE))
                                        .line_height(px(TEXT_CONTROL_LINE_HEIGHT))
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(foreground)
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

fn cpu_allocation_enabled(settings: &Settings, kind: CpuAllocationPage) -> bool {
    cpu_allocation_settings(settings, kind).enabled
}

fn set_cpu_allocation_enabled(settings: &mut Settings, kind: CpuAllocationPage, enabled: bool) {
    cpu_allocation_settings_mut(settings, kind).enabled = enabled;
}

fn cpu_allocation_protects_foreground(settings: &Settings, kind: CpuAllocationPage) -> bool {
    cpu_allocation_settings(settings, kind).protect_foreground_app
}

fn set_cpu_allocation_protects_foreground(
    settings: &mut Settings,
    kind: CpuAllocationPage,
    protect: bool,
) {
    cpu_allocation_settings_mut(settings, kind).protect_foreground_app = protect;
}

fn cpu_allocation_protects_visible_window_apps(
    settings: &Settings,
    kind: CpuAllocationPage,
) -> bool {
    cpu_allocation_settings(settings, kind).protect_visible_window_apps
}

fn set_cpu_allocation_protects_visible_window_apps(
    settings: &mut Settings,
    kind: CpuAllocationPage,
    protect: bool,
) {
    cpu_allocation_settings_mut(settings, kind).protect_visible_window_apps = protect;
}

fn cpu_allocation_settings(settings: &Settings, kind: CpuAllocationPage) -> &CpuAllocationSettings {
    match kind {
        CpuAllocationPage::CpuSetsSoft => &settings.cpu_sets_soft,
        CpuAllocationPage::ProcessorAffinityHard => &settings.processor_affinity_hard,
    }
}

fn cpu_allocation_settings_mut(
    settings: &mut Settings,
    kind: CpuAllocationPage,
) -> &mut CpuAllocationSettings {
    match kind {
        CpuAllocationPage::CpuSetsSoft => &mut settings.cpu_sets_soft,
        CpuAllocationPage::ProcessorAffinityHard => &mut settings.processor_affinity_hard,
    }
}

fn cpu_allocation_rules(settings: &Settings, kind: CpuAllocationPage) -> &[CpuAllocationRule] {
    &cpu_allocation_settings(settings, kind).rules
}

fn cpu_allocation_rules_mut(
    settings: &mut Settings,
    kind: CpuAllocationPage,
) -> &mut Vec<CpuAllocationRule> {
    &mut cpu_allocation_settings_mut(settings, kind).rules
}
