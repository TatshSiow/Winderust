use crate::config::{CpuAllocationPreset, CpuAllocationRule, CpuAllocationSettings};
use crate::ui::app::*;

const CPU_ALLOCATION_ACTION_COLUMN_WIDTH: f32 = 48.0;

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

impl ProcessRuleTier {
    fn cpu_allocation_label(self) -> String {
        match self {
            Self::Focus => t!("cpu_allocation.focus").to_string(),
            Self::VisibleWindow => t!("common.visible_window").to_string(),
            Self::Background => t!("common.background_process").to_string(),
        }
    }

    fn cpu_allocation_core_mask(self, rule: &CpuAllocationRule) -> u64 {
        match self {
            Self::Focus => rule.focus_core_mask,
            Self::VisibleWindow => rule.visible_window_core_mask,
            Self::Background => rule.background_core_mask,
        }
    }

    fn set_cpu_allocation_core_mask(self, rule: &mut CpuAllocationRule, core_mask: u64) {
        match self {
            Self::Focus => rule.focus_core_mask = core_mask,
            Self::VisibleWindow => rule.visible_window_core_mask = core_mask,
            Self::Background => rule.background_core_mask = core_mask,
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

    pub(in crate::ui::app) fn render_process_details_cpu_controls(
        &self,
        process: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut group =
            page_body_shell().child(section_title_text(t!("nav.cpu_control").to_string()));
        let processors = cpu_allocation::logical_processors();

        for kind in [
            CpuAllocationPage::CpuSetsSoft,
            CpuAllocationPage::ProcessorAffinityHard,
        ] {
            let key = kind.key();
            let title = t!(format!("nav.{key}")).to_string();
            let mut help_lines = vec![
                t!(format!("{key}.intro_2")).to_string(),
                t!("cpu_allocation.rules_help").to_string(),
            ];
            if matches!(kind, CpuAllocationPage::ProcessorAffinityHard)
                || cpu_allocation::has_multiple_processor_groups()
            {
                help_lines.push(t!(format!("{key}.warning")).to_string());
            }
            let help = tooltip_lines(help_lines);
            let rules = cpu_allocation_rules(&self.settings, kind);
            let rule_index = rules
                .iter()
                .position(|rule| process_setting_matches(&rule.executable_path, process));

            if let Some(index) = rule_index {
                let rule = &rules[index];
                let remove_target = ListItemRemovalTarget::new(kind.removal_kind(), index);
                let action = h_flex()
                    .items_center()
                    .gap_2()
                    .child(setting_group_switch_action(
                        format!("process-details-{key}-enabled"),
                        rule.enabled,
                        cx.listener(move |app, checked, _, cx| {
                            if let Some(rule) =
                                cpu_allocation_rules_mut(&mut app.settings, kind).get_mut(index)
                            {
                                rule.enabled = *checked;
                            }
                            cx.notify();
                        }),
                    ))
                    .child(
                        remove_control_button(Button::new(SharedString::from(format!(
                            "process-details-remove-{key}"
                        ))))
                        .on_click(cx.listener(move |app, _, _, cx| {
                            app.request_list_item_removal(remove_target, cx);
                        })),
                    )
                    .into_any_element();
                let mut card = v_flex()
                    .w_full()
                    .min_w(px(0.0))
                    .overflow_hidden()
                    .rounded(px(BRAND_RADIUS_SURFACE))
                    .bg(rgb(settings_card_color()))
                    .child(
                        setting_group_action_row_with_help(
                            format!("process-details-{key}-header"),
                            title,
                            help,
                            action,
                            false,
                        )
                        .border_b_1()
                        .border_color(rgb(border_color())),
                    );
                for tier in ProcessRuleTier::ALL {
                    card = card.child(
                        rule_action_row(
                            format!("process-details-{key}-{}-preset-row", tier.key()),
                            tier.cpu_allocation_label(),
                            self.render_cpu_allocation_preset_selector(
                                kind,
                                index,
                                tier,
                                tier.cpu_allocation_core_mask(rule),
                                &processors,
                                DropdownSelectWidth::Standard,
                                window,
                                cx,
                            ),
                        )
                        .into_any_element(),
                    );
                }
                group = group.child(card);
            } else {
                let process = process.to_owned();
                let can_enable = can_add_cpu_allocation_process(&self.settings, &process);
                let toggle_id = format!("process-details-{key}-enabled");
                let toggle = if can_enable {
                    setting_group_switch_action(
                        toggle_id,
                        false,
                        cx.listener(move |app, checked, _, cx| {
                            if *checked && can_add_cpu_allocation_process(&app.settings, &process) {
                                cpu_allocation_rules_mut(&mut app.settings, kind)
                                    .push(new_cpu_allocation_rule(&process));
                            }
                            cx.notify();
                        }),
                    )
                } else {
                    h_flex()
                        .id(SharedString::from(format!(
                            "process-details-{key}-disabled-toggle"
                        )))
                        .opacity(0.48)
                        .tooltip(|window, cx| {
                            Tooltip::new(t!("cpu_allocation.rules_help").to_string())
                                .build(window, cx)
                        })
                        .child(switch_indicator(SharedString::from(toggle_id), false))
                        .into_any_element()
                };
                group = group.child(
                    setting_action_card_with_help(
                        format!("process-details-{key}-add-card"),
                        title,
                        help,
                        toggle,
                    )
                    .into_any_element(),
                );
            }
        }

        group.into_any_element()
    }

    fn render_cpu_allocation_page(
        &self,
        kind: CpuAllocationPage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let enabled = cpu_allocation_settings(&self.settings, kind).enabled;
        let input = kind.suggestion_target().input(&self.inputs);
        let input_value = self.process_picker_path(kind.suggestion_target(), input, cx);
        let key = kind.key();
        let mut body = feature_body(enabled).child(section_header(
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
        let body =
            disabled_feature_body(SharedString::from(format!("{key}-body")), body, enabled, cx);

        self.page_shell(kind.page(), cx)
            .child(feature_toggle_switch_with_help(
                SharedString::from(format!("{key}-enabled")),
                t!(format!("{key}.enable")).to_string(),
                help,
                enabled,
                cx.listener(move |app, checked, _, cx| {
                    cpu_allocation_settings_mut(&mut app.settings, kind).enabled = *checked;
                    cx.notify();
                }),
            ))
            .child(body)
            .into_any_element()
    }

    pub(in crate::ui::app) fn render_cpu_allocation_side_panel(
        &self,
        page: Page,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let status_selected = self.cpu_allocation_side_panel_tab == PresetSidePanelTab::Status;
        let presets_selected = !status_selected;
        let selected_background = cx.theme().secondary_active;
        let hover_background = cx.theme().secondary_hover;
        let header = h_flex()
            .min_h(px(48.0))
            .gap_1()
            .px_3()
            .child(
                div()
                    .id("cpu-allocation-status-tab")
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
                        app.cpu_allocation_side_panel_tab = PresetSidePanelTab::Status;
                        cx.notify();
                    }))
                    .child(t!("common.status").to_string()),
            )
            .child(
                div()
                    .id("cpu-allocation-presets-tab")
                    .flex_1()
                    .h(px(32.0))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(BRAND_RADIUS_CONTROL))
                    .text_size(px(TEXT_CONTROL_SIZE))
                    .cursor_pointer()
                    .when(presets_selected, |tab| tab.bg(selected_background))
                    .hover(move |style| style.bg(hover_background))
                    .on_click(cx.listener(|app, _, _, cx| {
                        app.cpu_allocation_side_panel_tab = PresetSidePanelTab::Presets;
                        cx.notify();
                    }))
                    .child(t!("cpu_allocation.presets").to_string()),
            )
            .into_any_element();

        let body = if status_selected {
            v_flex()
                .flex_1()
                .min_h(px(0.0))
                .overflow_y_scrollbar()
                .p_3()
                .child(
                    self.render_normalized_feature_status(page)
                        .expect("CPU allocation pages always have normalized runtime status"),
                )
                .into_any_element()
        } else {
            self.render_cpu_allocation_presets_content(cx)
        };

        page_side_panel(header, body)
    }

    fn render_cpu_allocation_presets_content(&self, cx: &mut Context<Self>) -> AnyElement {
        let processors = cpu_allocation::logical_processors();
        let row_hover = cx.theme().secondary_hover;
        let mut presets = v_flex()
            .flex_1()
            .min_h(px(0.0))
            .w_full()
            .min_w(px(0.0))
            .overflow_y_scrollbar()
            .gap_4()
            .p_3();
        let mut core_presets = v_flex().w_full().gap_1().child(
            text_muted(t!("cpu_allocation.core_presets").to_string())
                .px_1()
                .pb_1(),
        );
        for (index, (label, _, available)) in cpu_allocation_core_presets(&processors)
            .into_iter()
            .enumerate()
        {
            core_presets = core_presets.child(
                h_flex()
                    .id(SharedString::from(format!(
                        "cpu-allocation-core-preset-{index}"
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
                            .when(!available, |label| label.opacity(0.48))
                            .child(label),
                    )
                    .child(
                        control_button(
                            Button::new(SharedString::from(format!(
                                "view-cpu-allocation-core-preset-{index}"
                            )))
                            .ghost(),
                        )
                        .with_size(px(28.0))
                        .icon(Icon::new(NavIcon::Info).with_size(px(12.0)))
                        .tooltip(t!("cpu_allocation.view_preset").to_string())
                        .on_click(cx.listener(
                            move |app, _, window, cx| {
                                app.open_cpu_allocation_preset_dialog(
                                    CpuAllocationPresetEditorTarget::Core(index),
                                    window,
                                    cx,
                                );
                            },
                        )),
                    ),
            );
        }
        presets = presets.child(core_presets);

        let mut custom_presets = v_flex().w_full().gap_1().child(
            text_muted(t!("cpu_allocation.custom_presets").to_string())
                .px_1()
                .pb_1(),
        );
        for (index, preset) in self.settings.cpu_allocation_presets.iter().enumerate() {
            let removal_target =
                ListItemRemovalTarget::new(ListItemRemovalKind::CpuAllocationPreset, index);
            let row = h_flex()
                .id(SharedString::from(format!(
                    "cpu-allocation-custom-preset-row-{index}"
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
                        .child(cpu_allocation_preset_label(&preset.name)),
                )
                .child(
                    h_flex()
                        .flex_shrink_0()
                        .gap_1()
                        .child(
                            control_button(
                                Button::new(SharedString::from(format!(
                                    "edit-cpu-allocation-preset-{index}"
                                )))
                                .ghost(),
                            )
                            .with_size(px(28.0))
                            .icon(Icon::new(NavIcon::SquarePen).with_size(px(12.0)))
                            .tooltip(t!("common.edit").to_string())
                            .on_click(cx.listener(
                                move |app, _, window, cx| {
                                    app.open_cpu_allocation_preset_dialog(
                                        CpuAllocationPresetEditorTarget::Custom(Some(index)),
                                        window,
                                        cx,
                                    );
                                },
                            )),
                        )
                        .child(
                            remove_control_button(Button::new(SharedString::from(format!(
                                "remove-cpu-allocation-preset-{index}"
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
                SharedString::from(format!("cpu-allocation-preset-{index}")),
                row,
            ));
        }
        if self.settings.cpu_allocation_presets.is_empty() {
            custom_presets = custom_presets.child(
                text_muted(t!("cpu_allocation.no_custom_presets").to_string())
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
                        primary_control_button(Button::new("add-cpu-allocation-preset"), cx)
                            .w_full()
                            .icon(Icon::new(NavIcon::Plus).with_size(px(14.0)))
                            .label(t!("cpu_allocation.add_preset").to_string())
                            .on_click(cx.listener(|app, _, window, cx| {
                                app.open_cpu_allocation_preset_dialog(
                                    CpuAllocationPresetEditorTarget::Custom(None),
                                    window,
                                    cx,
                                );
                            })),
                    ),
            )
            .into_any_element()
    }

    fn open_cpu_allocation_preset_dialog(
        &mut self,
        target: CpuAllocationPresetEditorTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let processors = cpu_allocation::logical_processors();
        let available_mask = cpu_allocation_processors_mask(&processors);
        let (name, core_mask) = match target {
            CpuAllocationPresetEditorTarget::Core(index) => {
                let Some((_, core_mask, _)) = cpu_allocation_core_presets(&processors)
                    .into_iter()
                    .nth(index)
                else {
                    return;
                };
                (None, core_mask)
            }
            CpuAllocationPresetEditorTarget::Custom(Some(index)) => {
                let Some(preset) = self.settings.cpu_allocation_presets.get(index) else {
                    return;
                };
                let core_mask = preset.core_mask & available_mask;
                (Some(preset.name.clone()), core_mask)
            }
            CpuAllocationPresetEditorTarget::Custom(None) => (Some(String::new()), available_mask),
        };

        self.cpu_allocation_preset_editor = Some(CpuAllocationPresetEditor { target, core_mask });
        self.active_power_plan_picker = None;
        if let Some(name) = name {
            clear_input_to(&self.inputs.cpu_allocation_preset_name, &name, window, cx);
            self.inputs
                .cpu_allocation_preset_name
                .read(cx)
                .focus_handle(cx)
                .focus(window);
        }
        cx.notify();
    }

    pub(in crate::ui::app) fn close_cpu_allocation_preset_editor(
        &mut self,
        cx: &mut Context<Self>,
    ) {
        self.cpu_allocation_preset_editor = None;
        cx.notify();
    }

    fn save_cpu_allocation_preset(&mut self, cx: &mut Context<Self>) {
        let Some(editor) = self.cpu_allocation_preset_editor else {
            return;
        };
        let CpuAllocationPresetEditorTarget::Custom(index) = editor.target else {
            return;
        };
        let name = self
            .inputs
            .cpu_allocation_preset_name
            .read(cx)
            .value()
            .to_string();
        let saved = upsert_cpu_allocation_preset(
            &mut self.settings.cpu_allocation_presets,
            index,
            &name,
            editor.core_mask,
        );
        if saved {
            self.close_cpu_allocation_preset_editor(cx);
        }
    }

    pub(in crate::ui::app) fn render_cpu_allocation_preset_modal(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let editor = self
            .cpu_allocation_preset_editor
            .expect("CPU allocation preset modal requires an editor");
        let input = &self.inputs.cpu_allocation_preset_name;
        let name = input.read(cx).value().to_string();
        let custom_index = match editor.target {
            CpuAllocationPresetEditorTarget::Core(_) => None,
            CpuAllocationPresetEditorTarget::Custom(index) => index,
        };
        let edits_custom_preset =
            matches!(editor.target, CpuAllocationPresetEditorTarget::Custom(_));
        let duplicate_name = edits_custom_preset
            && cpu_allocation_preset_name_exists(
                &self.settings.cpu_allocation_presets,
                custom_index,
                &name,
            );
        let can_save = editor.core_mask != 0 && !name.trim().is_empty() && !duplicate_name;
        let name_focused = input.read(cx).focus_handle(cx).is_focused(window);
        let processors = cpu_allocation::logical_processors();
        let title = match editor.target {
            CpuAllocationPresetEditorTarget::Core(index) => {
                let view = t!("cpu_allocation.view_preset").to_string();
                cpu_allocation_core_presets(&processors)
                    .get(index)
                    .map_or(view.clone(), |(label, _, _)| format!("{view}: {label}"))
            }
            CpuAllocationPresetEditorTarget::Custom(Some(_)) => {
                t!("cpu_allocation.edit_preset").to_string()
            }
            CpuAllocationPresetEditorTarget::Custom(None) => {
                t!("cpu_allocation.add_preset").to_string()
            }
        };
        let name_help = if duplicate_name {
            text_warning(t!("cpu_allocation.duplicate_preset_name").to_string())
        } else {
            text_muted(t!("cpu_allocation.preset_name_help").to_string())
        };
        let mut content = v_flex().w_full().min_w(px(0.0)).gap_3().p_4();
        if edits_custom_preset {
            content = content.child(
                branded_panel()
                    .child(setting_group_stacked_action_row(
                        "cpu-allocation-preset-name-row",
                        t!("cpu_allocation.preset_name").to_string(),
                        app_input(input, name_focused, cx).into_any_element(),
                        false,
                    ))
                    .child(name_help.px_4().pb_3()),
            );
        }
        content = content.child(branded_panel().child(setting_group_stacked_action_row(
            "cpu-allocation-preset-cores-row",
            t!("cpu_allocation.allowed_cpus").to_string(),
            self.render_core_tile_grid(
                &processors,
                editor.core_mask,
                "cpu-allocation-preset-core",
                edits_custom_preset,
                |app, core| {
                    if let Some(editor) = app.cpu_allocation_preset_editor.as_mut() {
                        toggle_affinity_core(&mut editor.core_mask, core);
                    }
                },
                cx,
            ),
            false,
        )));

        let mut footer = h_flex()
            .w_full()
            .flex_shrink_0()
            .items_center()
            .justify_end()
            .gap_2()
            .p_4()
            .border_t_1()
            .border_color(rgb(border_color()));
        if edits_custom_preset {
            footer = footer
                .child(
                    control_button(Button::new("cancel-cpu-allocation-preset"))
                        .label(t!("common.cancel").to_string())
                        .on_click(cx.listener(|app, _, _, cx| {
                            app.close_cpu_allocation_preset_editor(cx);
                        })),
                )
                .child(
                    primary_control_button(Button::new("save-cpu-allocation-preset"), cx)
                        .label(t!("common.save").to_string())
                        .disabled(!can_save)
                        .on_click(cx.listener(|app, _, _, cx| {
                            app.save_cpu_allocation_preset(cx);
                        })),
                );
        } else {
            footer = footer.child(
                primary_control_button(Button::new("close-core-preset-view"), cx)
                    .label(t!("common.done").to_string())
                    .on_click(cx.listener(|app, _, _, cx| {
                        app.close_cpu_allocation_preset_editor(cx);
                    })),
            );
        }

        let modal = v_flex()
            .w_full()
            .max_w(px(760.0))
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
                        control_button(Button::new("close-cpu-allocation-preset"))
                            .with_size(px(32.0))
                            .icon(Icon::new(NavIcon::X).with_size(px(14.0)))
                            .on_click(cx.listener(|app, _, _, cx| {
                                app.close_cpu_allocation_preset_editor(cx);
                            })),
                    ),
            )
            .child(
                div()
                    .id("cpu-allocation-preset-scroll")
                    .flex_1()
                    .min_h(px(0.0))
                    .overflow_y_scrollbar()
                    .child(content),
            )
            .child(footer);
        let modal = with_optional_motion(
            modal,
            "cpu-allocation-preset-modal-open",
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
                app.close_cpu_allocation_preset_editor(cx);
            }))
            .child(modal);

        with_optional_motion(
            backdrop,
            "cpu-allocation-preset-backdrop-open",
            MotionSpeed::Fast,
            |backdrop| backdrop,
            |backdrop, delta| backdrop.opacity(delta),
        )
    }

    fn render_cpu_allocation_rules(
        &self,
        kind: CpuAllocationPage,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let rules = cpu_allocation_rules(&self.settings, kind);
        let processors = cpu_allocation::logical_processors();
        let key = kind.key();
        let mut list = rule_list(vec![
            rule_table_active_header(),
            rule_table_title_header(t!("process_list.app_name").to_string()),
            rule_table_title_header(t!("process_list.executable_path").to_string()),
            rule_table_centered_header(
                t!("cpu_allocation.focus").to_string(),
                DROPDOWN_SELECT_COMPACT_WIDTH,
            ),
            rule_table_centered_header(
                t!("common.visible_window").to_string(),
                DROPDOWN_SELECT_COMPACT_WIDTH,
            ),
            rule_table_centered_header(
                t!("common.background_process").to_string(),
                DROPDOWN_SELECT_COMPACT_WIDTH,
            ),
            rule_table_centered_header(
                t!("common.actions").to_string(),
                CPU_ALLOCATION_ACTION_COLUMN_WIDTH,
            ),
        ]);
        for (index, rule) in rules.iter().enumerate() {
            let process = rule.executable_path.clone();
            let mut row = compact_rule_row(format!("{key}-rule-row-{index}"))
                .child(rule_active_cell(
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
                ))
                .child(self.process_rule_title(&process, cx));
            for tier in ProcessRuleTier::ALL {
                row = row.child(self.render_cpu_allocation_preset_selector(
                    kind,
                    index,
                    tier,
                    tier.cpu_allocation_core_mask(rule),
                    &processors,
                    DropdownSelectWidth::Compact,
                    window,
                    cx,
                ));
            }
            row = row.child(
                h_flex()
                    .w(px(CPU_ALLOCATION_ACTION_COLUMN_WIDTH))
                    .min_w(px(0.0))
                    .flex_shrink_0()
                    .justify_center()
                    .child(
                        remove_control_button(Button::new(SharedString::from(format!(
                            "remove-{key}-{index}"
                        ))))
                        .on_click(cx.listener(move |app, _, _, cx| {
                            app.request_list_item_removal(
                                ListItemRemovalTarget::new(kind.removal_kind(), index),
                                cx,
                            );
                        })),
                    ),
            );
            list = list.child(self.animated_list_item(
                ListItemRemovalTarget::new(kind.removal_kind(), index),
                SharedString::from(format!("{key}-rule-{index}")),
                row.into_any_element(),
            ));
        }
        if rules.is_empty() {
            list = list.child(text_muted(t!("cpu_allocation.no_rules").to_string()).p_4());
        }
        list.into_any_element()
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "the selector needs rule identity, shared processor data, width, and GPUI contexts"
    )]
    fn render_cpu_allocation_preset_selector(
        &self,
        kind: CpuAllocationPage,
        index: usize,
        tier: ProcessRuleTier,
        core_mask: u64,
        processors: &[LogicalProcessorInfo],
        width: DropdownSelectWidth,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let all_mask = cpu_allocation_processors_mask(processors);
        let mut preset_options = cpu_allocation_core_presets(processors);
        let custom_start = preset_options.len();
        preset_options.extend(self.settings.cpu_allocation_presets.iter().map(|preset| {
            let mask = usable_cpu_allocation_preset_mask(preset.core_mask, all_mask);
            (
                cpu_allocation_preset_label(&preset.name),
                mask.unwrap_or(0),
                mask.is_some(),
            )
        }));
        let selected_index = (custom_start..preset_options.len())
            .find(|index| {
                let (_, mask, available) = &preset_options[*index];
                *available && core_mask == *mask
            })
            .or_else(|| {
                (0..custom_start).find(|index| {
                    let (_, mask, available) = &preset_options[*index];
                    *available && core_mask == *mask
                })
            });
        let selected = selected_index
            .and_then(|index| preset_options.get(index))
            .map(|(label, _, _)| label.clone())
            .unwrap_or_else(|| t!("cpu_allocation.custom").to_string());
        let key = kind.key();
        let tier_key = tier.key();
        self.render_dropdown_select(
            format!("{key}-{tier_key}-core-preset-{index}"),
            selected,
            true,
            width,
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
                            "{key}-{tier_key}-core-preset-{index}-option-{option_index}"
                        )),
                        label,
                        selected_index == Some(option_index),
                        cx,
                    )
                    .when(!available, |row| row.opacity(0.48).cursor_default());
                    options = options.child(if available {
                        row.on_click(cx.listener(move |app, _, _, cx| {
                            if let Some(rule) =
                                cpu_allocation_rules_mut(&mut app.settings, kind).get_mut(index)
                            {
                                tier.set_cpu_allocation_core_mask(rule, mask);
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
        )
    }

    pub(in crate::ui::app) fn render_core_tile_grid(
        &self,
        processors: &[LogicalProcessorInfo],
        core_mask: u64,
        id_prefix: impl Into<String>,
        editable: bool,
        on_toggle: impl Fn(&mut Self, usize) + Clone + 'static,
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
            let on_toggle = on_toggle.clone();
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
            let background: Hsla = rgb(if selected {
                accent_color()
            } else {
                settings_card_color()
            })
            .into();
            let variant = ButtonCustomVariant::new(cx)
                .color(background)
                .foreground(foreground)
                .border(
                    rgb(if selected {
                        accent_color()
                    } else {
                        border_color()
                    })
                    .into(),
                )
                .hover(if !editable {
                    background
                } else if selected {
                    cx.theme().primary_hover
                } else {
                    cx.theme().secondary_hover
                })
                .active(if !editable {
                    background
                } else if selected {
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
                        .when(editable, |button| {
                            button.on_click(cx.listener(move |app, _, _, cx| {
                                on_toggle(app, core);
                                cx.notify();
                            }))
                        })
                        .when(!editable, |button| button.cursor_default())
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

fn cpu_allocation_core_presets(processors: &[LogicalProcessorInfo]) -> Vec<(String, u64, bool)> {
    let all_mask = cpu_allocation_processors_mask(processors);
    let performance_mask =
        cpu_allocation_processors_kind_mask(processors, LogicalProcessorKind::Performance);
    let efficiency_mask =
        cpu_allocation_processors_kind_mask(processors, LogicalProcessorKind::Efficiency);
    let no_smt_mask = cpu_allocation_processors_no_smt_mask(processors);
    let performance_no_smt_mask = performance_mask & no_smt_mask;

    vec![
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
            t!("cpu_allocation.all_cores_no_smt").to_string(),
            no_smt_mask,
            no_smt_mask != 0 && no_smt_mask != all_mask,
        ),
        (
            t!("cpu_allocation.p_cores_no_smt").to_string(),
            performance_no_smt_mask,
            performance_no_smt_mask != 0 && performance_no_smt_mask != performance_mask,
        ),
    ]
}

fn cpu_allocation_preset_label(name: &str) -> String {
    let name = name.trim();
    if name.is_empty() {
        t!("cpu_allocation.unnamed_preset").to_string()
    } else {
        name.to_owned()
    }
}

fn usable_cpu_allocation_preset_mask(core_mask: u64, available_mask: u64) -> Option<u64> {
    let core_mask = core_mask & available_mask;
    (core_mask != 0).then_some(core_mask)
}

fn cpu_allocation_preset_name_exists(
    presets: &[CpuAllocationPreset],
    editing_index: Option<usize>,
    name: &str,
) -> bool {
    let name = name.trim();
    !name.is_empty()
        && presets.iter().enumerate().any(|(index, preset)| {
            Some(index) != editing_index && preset.name.trim().eq_ignore_ascii_case(name)
        })
}

fn upsert_cpu_allocation_preset(
    presets: &mut Vec<CpuAllocationPreset>,
    editing_index: Option<usize>,
    name: &str,
    core_mask: u64,
) -> bool {
    let name = name.trim();
    if name.is_empty()
        || core_mask == 0
        || cpu_allocation_preset_name_exists(presets, editing_index, name)
    {
        return false;
    }

    let preset = CpuAllocationPreset {
        name: name.to_owned(),
        core_mask,
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
    fn core_cpu_presets_follow_processor_topology() {
        let processors = vec![
            LogicalProcessorInfo {
                index: 0,
                core_index: 0,
                kind: LogicalProcessorKind::Performance,
                efficiency_class: 1,
            },
            LogicalProcessorInfo {
                index: 1,
                core_index: 0,
                kind: LogicalProcessorKind::Performance,
                efficiency_class: 1,
            },
            LogicalProcessorInfo {
                index: 2,
                core_index: 1,
                kind: LogicalProcessorKind::Efficiency,
                efficiency_class: 0,
            },
            LogicalProcessorInfo {
                index: 3,
                core_index: 2,
                kind: LogicalProcessorKind::Efficiency,
                efficiency_class: 0,
            },
        ];

        let presets = cpu_allocation_core_presets(&processors);
        assert_eq!(
            presets
                .iter()
                .map(|(_, mask, available)| (*mask, *available))
                .collect::<Vec<_>>(),
            vec![
                (0b1111, true),
                (0b0011, true),
                (0b1100, true),
                (0b1101, true),
                (0b0001, true)
            ]
        );
    }

    #[test]
    fn custom_cpu_presets_add_edit_and_reject_duplicate_names() {
        let mut presets = Vec::new();

        assert!(upsert_cpu_allocation_preset(
            &mut presets,
            None,
            "  Gaming  ",
            0b0011,
        ));
        assert_eq!(presets[0].name, "Gaming");
        assert!(!upsert_cpu_allocation_preset(
            &mut presets,
            None,
            "gaming",
            0b1100,
        ));
        assert!(upsert_cpu_allocation_preset(
            &mut presets,
            Some(0),
            "Productivity",
            0b1100,
        ));
        assert_eq!(presets[0].name, "Productivity");
        assert_eq!(presets[0].core_mask, 0b1100);
    }

    #[test]
    fn custom_cpu_preset_uses_only_available_logical_cpus() {
        assert_eq!(
            usable_cpu_allocation_preset_mask(0b1100, 0b0110),
            Some(0b0100)
        );
        assert_eq!(usable_cpu_allocation_preset_mask(0b1000, 0b0111), None);
    }
}
