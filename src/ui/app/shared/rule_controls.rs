use crate::ui::app::*;

impl WinderustApp {
    pub(in crate::ui::app) fn render_process_exclusion_list(
        &self,
        rules: &[ProcessExclusionRule],
        removal_kind: ListItemRemovalKind,
        id_prefix: &'static str,
        empty_state: AnyElement,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut list = rule_list(process_rule_table_headers());
        for (index, rule) in rules.iter().enumerate() {
            let process = rule.executable_path.clone();
            let row = compact_rule_row(format!("{id_prefix}-row-{index}"))
                .child(rule_active_cell(
                    format!("{id_prefix}-enabled-{index}"),
                    rule.enabled,
                    cx.listener(move |app, checked, _, cx| {
                        app.set_process_exclusion_enabled(removal_kind, index, *checked);
                        cx.notify();
                    }),
                ))
                .child(self.process_rule_title(&process, cx))
                .child(rule_table_action_cell(
                    remove_control_button(Button::new(SharedString::from(format!(
                        "remove-{id_prefix}-{index}"
                    ))))
                    .on_click(cx.listener(move |app, _, _, cx| {
                        app.request_list_item_removal(
                            ListItemRemovalTarget::new(removal_kind, index),
                            cx,
                        );
                    }))
                    .into_any_element(),
                ));
            list = list.child(self.animated_list_item(
                ListItemRemovalTarget::new(removal_kind, index),
                SharedString::from(format!("{id_prefix}-{index}")),
                row.into_any_element(),
            ));
        }
        if rules.is_empty() {
            list = list.child(empty_state);
        }
        list.into_any_element()
    }

    fn set_process_exclusion_enabled(
        &mut self,
        kind: ListItemRemovalKind,
        index: usize,
        enabled: bool,
    ) {
        let rule = match kind {
            ListItemRemovalKind::WorkloadEngineExclusion => self
                .settings
                .workload_engine
                .workload_engine_exclusions
                .get_mut(index),
            ListItemRemovalKind::MemoryTrimExclusion => {
                self.settings.memory_trim.exclusions.get_mut(index)
            }
            _ => unreachable!("unsupported process exclusion kind: {kind:?}"),
        };
        if let Some(rule) = rule {
            rule.enabled = enabled;
        }
    }

    pub(in crate::ui::app) fn render_numeric_value(
        &self,
        field: NumericField,
        display_value: String,
        edit_value: String,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let width = numeric_value_width(field);
        if self.editing_numeric == Some(field) {
            return h_flex()
                .id(SharedString::from(format!("numeric-editor-{field:?}")))
                .w(px(width))
                .items_center()
                .on_click(|_, _, cx| {
                    cx.stop_propagation();
                })
                .on_action(cx.listener(|app, _: &InputEscape, _, cx| {
                    app.finish_numeric_edit(cx);
                }))
                .on_mouse_down_out(cx.listener(|app, _: &gpui::MouseDownEvent, _, cx| {
                    app.finish_numeric_edit(cx);
                }))
                .child(app_input(&self.inputs.numeric_value, true, cx))
                .into_any_element();
        }

        h_flex()
            .id(SharedString::from(format!("numeric-value-{field:?}")))
            .w(px(width))
            .cursor_pointer()
            .on_click(cx.listener(move |app, _: &gpui::ClickEvent, window, cx| {
                app.begin_numeric_edit(field, edit_value.clone(), window, cx);
            }))
            .child(value_pill(display_value).w_full())
            .into_any_element()
    }

    pub(in crate::ui::app) fn render_network_threshold_cell(
        &self,
        field: ThresholdField,
        threshold_bytes: u64,
        unit: NetworkThresholdUnit,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let value = unit.threshold_value_from_bytes(threshold_bytes);
        let value_label = if threshold_bytes == 0 {
            "∞".to_owned()
        } else {
            network_threshold_value_label(value)
        };

        h_flex()
            .w(px(172.0))
            .min_w(px(0.0))
            .flex_shrink_0()
            .gap_1()
            .items_center()
            .justify_center()
            .when(!enabled, |cell| cell.opacity(0.42))
            .child(if enabled {
                self.render_numeric_value(
                    NumericField::NetworkThreshold(field),
                    value_label,
                    network_threshold_edit_value(threshold_bytes, unit),
                    cx,
                )
            } else {
                h_flex()
                    .w(px(numeric_value_width(NumericField::NetworkThreshold(
                        field,
                    ))))
                    .child(value_pill(value_label).w_full())
                    .into_any_element()
            })
            .child(self.render_network_unit_picker(field, unit, enabled, window, cx))
            .into_any_element()
    }

    pub(in crate::ui::app) fn threshold_rule_mut(
        &mut self,
        field: ThresholdField,
    ) -> Option<&mut AppSuspensionRule> {
        let index = match field {
            ThresholdField::Download(index) | ThresholdField::Upload(index) => index,
        };
        self.settings.app_suspension.suspendable_apps.get_mut(index)
    }

    pub(in crate::ui::app) fn render_network_unit_picker(
        &self,
        field: ThresholdField,
        selected: NetworkThresholdUnit,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let picker_id = format!("network-unit-{field:?}");
        let is_open =
            enabled && self.active_power_plan_picker.as_deref() == Some(picker_id.as_str());
        let placement = self.dropdown_placement(
            &picker_id,
            dropdown_list_height(NetworkThresholdUnit::ALL.len()),
            window,
        );
        let mut options = dropdown_surface(cx, placement.max_height);

        for unit in NetworkThresholdUnit::ALL {
            options = options.child(
                dropdown_option_row(
                    SharedString::from(format!("{picker_id}-{}", unit.label())),
                    unit.label().to_string(),
                    selected == unit,
                    cx,
                )
                .on_click(cx.listener(move |app, _, _, cx| {
                    if let Some(rule) = app.threshold_rule_mut(field) {
                        match field {
                            ThresholdField::Download(_) => {
                                rule.network_download_threshold_unit = unit
                            }
                            ThresholdField::Upload(_) => rule.network_upload_threshold_unit = unit,
                        }
                    }
                    app.active_power_plan_picker = None;
                    cx.notify();
                })),
            );
        }

        let control_id = SharedString::from(format!("{picker_id}-control"));
        let toggle_picker_id = picker_id.clone();
        let phase = dropdown_popup_phase(picker_id.as_str(), is_open, cx);

        dropdown_select_container(DropdownSelectWidth::Compact)
            .w(px(NETWORK_UNIT_PICKER_WIDTH))
            .min_w(px(NETWORK_UNIT_PICKER_WIDTH))
            .max_w(px(NETWORK_UNIT_PICKER_WIDTH))
            .child(
                dropdown_select_control(
                    control_id,
                    selected.label().to_string(),
                    enabled,
                    is_open,
                    phase,
                    cx,
                )
                .when(enabled, |control| {
                    control.on_click(cx.listener(move |app, _: &gpui::ClickEvent, _, cx| {
                        app.active_power_plan_picker = (app.active_power_plan_picker.as_deref()
                            != Some(toggle_picker_id.as_str()))
                        .then_some(toggle_picker_id.clone());
                        cx.notify();
                    }))
                }),
            )
            .child(dropdown_anchor_sensor(
                picker_id.clone(),
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

    pub(in crate::ui::app) fn render_inline_power_plan_picker(
        &self,
        id: impl Into<String>,
        selected_guid: Option<String>,
        field: PowerPlanField,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = id.into();
        let is_open = self.active_power_plan_picker.as_deref() == Some(id.as_str());
        let option_count = self.plans.len().max(1);
        let placement = self.dropdown_placement(&id, dropdown_list_height(option_count), window);
        let selected_text = match selected_guid.as_deref() {
            Some(guid) => self
                .plans
                .iter()
                .find(|plan| plan.guid.eq_ignore_ascii_case(guid))
                .map(PowerPlan::display_name)
                .unwrap_or_else(|| t!("common.selected_plan_unavailable").to_string()),
            None => t!("common.selected_plan_unavailable").to_string(),
        };

        let mut options = dropdown_surface(cx, placement.max_height);

        if self.plans.is_empty() {
            options = options.child(dropdown_empty_row(
                t!("common.no_power_plans_loaded").to_string(),
                cx,
            ));
        } else {
            for plan in &self.plans {
                let selected = selected_guid
                    .as_deref()
                    .is_some_and(|selected| selected.eq_ignore_ascii_case(&plan.guid));
                options = options.child(power_plan_option_row(
                    format!("{id}-{}", plan.guid),
                    plan.display_name(),
                    selected,
                    Some(plan.guid.clone()),
                    field,
                    cx,
                ));
            }
        }

        let control_id = id.clone();
        let phase = dropdown_popup_phase(id.as_str(), is_open, cx);
        dropdown_select_container(DropdownSelectWidth::Standard)
            .child(
                dropdown_select_control(
                    SharedString::from(format!("{id}-select-control")),
                    selected_text,
                    true,
                    is_open,
                    phase,
                    cx,
                )
                .on_click(cx.listener(move |app, _: &gpui::ClickEvent, _, cx| {
                    app.refresh_power_plans();
                    app.active_power_plan_picker = (app.active_power_plan_picker.as_deref()
                        != Some(control_id.as_str()))
                    .then_some(control_id.clone());
                    cx.notify();
                })),
            )
            .child(dropdown_anchor_sensor(
                id.clone(),
                Rc::clone(&self.dropdown_anchor_bounds),
            ))
            .child(dropdown_popup_or_empty(
                SharedString::from(id),
                phase,
                placement,
                options,
                cx,
            ))
            .into_any_element()
    }

    pub(in crate::ui::app) fn set_power_plan_field(
        &mut self,
        field: PowerPlanField,
        guid: Option<String>,
    ) {
        match field {
            PowerPlanField::ActivityKind(PowerPlanKind::Idle) => {
                self.settings.by_activity.power_plans.power_save_guid = guid
            }
            PowerPlanField::ActivityKind(PowerPlanKind::Active) => {
                self.settings.by_activity.power_plans.performance_guid = guid
            }
            PowerPlanField::ByForegroundRule(index) => {
                if let Some(rule) = self.settings.by_foreground.rules.get_mut(index) {
                    rule.power_plan_guid = guid;
                }
            }
            PowerPlanField::ByRunningAppRule(index) => {
                if let Some(rule) = self.settings.by_running_app.rules.get_mut(index) {
                    rule.power_plan_guid = guid;
                }
            }
            PowerPlanField::ByTimeRule(index) => {
                if let Some(rule) = self.settings.by_time.rules.get_mut(index) {
                    rule.power_plan_guid = guid;
                }
            }
            PowerPlanField::CpuRule(index) => {
                if let Some(rule) = self.settings.by_cpu_load.rules.get_mut(index) {
                    rule.power_plan_guid = guid;
                }
            }
            PowerPlanField::CpuRuleElse(index) => {
                if let Some(rule) = self.settings.by_cpu_load.rules.get_mut(index) {
                    rule.else_power_plan_guid = guid;
                }
            }
            PowerPlanField::ProcessorPowerTarget => {
                self.set_processor_power_target_plan_option(guid);
            }
        }
    }

    pub(in crate::ui::app) fn render_process_suggestions(
        &self,
        id: impl Into<String>,
        query: &str,
        target: SuggestionTarget,
        max_height: Pixels,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = id.into();
        let query = query.trim().to_ascii_lowercase();
        let mut matches = self
            .process_candidates
            .iter()
            .filter(|process| {
                query.is_empty()
                    || process.name.to_ascii_lowercase().contains(query.as_str())
                    || process
                        .image_path
                        .to_string_lossy()
                        .to_ascii_lowercase()
                        .contains(query.as_str())
            })
            .filter(|process| process_candidate_is_visible(target, &self.settings, process))
            .collect::<Vec<_>>();
        matches.sort_by(|left, right| left.name.cmp(&right.name));
        let selected_index = matches
            .iter()
            .position(|process| process_candidate_can_accept(target, &self.settings, process));
        let mut suggestions = dropdown_surface(cx, max_height);
        if matches.is_empty() {
            suggestions = suggestions.child(dropdown_empty_row(
                process_load_state_message(&self.process_candidate_load_state).unwrap_or_else(
                    || {
                        if self.process_candidates.is_empty() {
                            t!("common.no_running_apps_loaded").to_string()
                        } else {
                            t!("common.no_matching_apps").to_string()
                        }
                    },
                ),
                cx,
            ));
        }
        for (count, process) in matches.into_iter().enumerate() {
            let executable_path = process.image_path.to_string_lossy().into_owned();
            let enabled = process_candidate_can_accept(target, &self.settings, process);
            let row = dropdown_process_option_row(
                SharedString::from(format!("{id}-{count}")),
                process,
                selected_index == Some(count),
                !enabled,
                cx,
            );
            suggestions = suggestions.child(if enabled {
                row.on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |app, _: &gpui::MouseDownEvent, window, cx| {
                        cx.stop_propagation();
                        app.apply_process_suggestion(target, &executable_path, window, cx);
                        window.blur();
                        cx.notify();
                    }),
                )
            } else {
                row
            });
        }

        suggestions.into_any_element()
    }

    pub(in crate::ui::app) fn process_icon_for_path(&self, process: &str) -> Option<&Arc<Image>> {
        let process = Path::new(process.trim());
        self.process_candidates
            .iter()
            .find(|candidate| same_executable_path(&candidate.image_path, process))
            .and_then(|candidate| candidate.icon.as_ref())
    }

    pub(in crate::ui::app) fn process_rule_title(
        &self,
        process: &str,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let display_name = Path::new(process)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(process)
            .to_owned();
        let status_id = SharedString::from(format!("process-rule-status-{process}"));
        let running_count =
            (self.running_process_load_state == ProcessLoadState::Loaded).then(|| {
                self.running_processes
                    .iter()
                    .filter(|running| {
                        running
                            .image_path
                            .as_deref()
                            .is_some_and(|path| same_executable_path(path, Path::new(process)))
                    })
                    .count()
            });
        h_flex()
            .flex_1()
            .min_w(px(0.0))
            .overflow_hidden()
            .items_center()
            .gap_2()
            .child(process_icon_cell(self.process_icon_for_path(process), cx))
            .when_some(running_count, |row, count| {
                let (color, tooltip) = if count == 0 {
                    (
                        dim_text_color(),
                        t!("common.process_rule_not_running").to_string(),
                    )
                } else {
                    (
                        success_text_color(),
                        t!("common.process_rule_running", count = count).to_string(),
                    )
                };
                row.child(
                    div()
                        .id(status_id)
                        .size(px(7.0))
                        .flex_shrink_0()
                        .rounded_full()
                        .bg(rgb(color))
                        .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx)),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.0))
                    .overflow_hidden()
                    .whitespace_nowrap()
                    .text_size(px(TEXT_HEADER_SIZE))
                    .line_height(px(TEXT_HEADER_LINE_HEIGHT))
                    .child(display_name),
            )
            .child(self.process_rule_path(process))
            .into_any_element()
    }

    pub(in crate::ui::app) fn process_rule_path(&self, process: &str) -> AnyElement {
        let executable_path = process.to_owned();
        div()
            .id(SharedString::from(format!(
                "process-rule-path-{executable_path}"
            )))
            .flex_1()
            .min_w(px(0.0))
            .truncate()
            .text_color(rgb(dim_text_color()))
            .child(executable_path.clone())
            .tooltip(move |window, cx| Tooltip::new(executable_path.clone()).build(window, cx))
            .into_any_element()
    }

    pub(in crate::ui::app) fn render_process_picker(
        &self,
        id: impl Into<String>,
        input: &Entity<InputState>,
        target: SuggestionTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = id.into();
        let (query, is_open) = {
            let input = input.read(cx);
            (
                input.value().to_string(),
                input.focus_handle(cx).is_focused(window),
            )
        };
        let normalized_query = query.trim().to_ascii_lowercase();
        let suggestion_count = self
            .process_candidates
            .iter()
            .filter(|process| {
                normalized_query.is_empty()
                    || process
                        .name
                        .to_ascii_lowercase()
                        .contains(normalized_query.as_str())
                    || process
                        .image_path
                        .to_string_lossy()
                        .to_ascii_lowercase()
                        .contains(normalized_query.as_str())
            })
            .filter(|process| process_candidate_is_visible(target, &self.settings, process))
            .count()
            .max(1);
        let selected_path = self.process_picker_path(target, input, cx);
        let input_detail = if !selected_path.is_empty() {
            Some(selected_path)
        } else {
            process_load_state_message(&self.process_candidate_load_state)
        };
        let placement =
            self.dropdown_placement(&id, dropdown_list_height(suggestion_count), window);

        v_flex()
            .w_full()
            .max_w(px(372.0))
            .min_w(px(0.0))
            .relative()
            .min_h(px(32.0))
            .child(app_input_with_detail(input, is_open, input_detail, cx))
            .child(dropdown_anchor_sensor(
                id.clone(),
                Rc::clone(&self.dropdown_anchor_bounds),
            ))
            .child(if is_open {
                deferred(
                    dropdown_popup_layer(placement, true)
                        .on_mouse_down_out(cx.listener(
                            |_, _: &gpui::MouseDownEvent, window, cx| {
                                window.blur();
                                cx.notify();
                            },
                        ))
                        .child(self.render_process_suggestions(
                            id,
                            &query,
                            target,
                            placement.max_height,
                            cx,
                        )),
                )
                .with_priority(PROCESS_PICKER_LAYER_PRIORITY)
                .into_any_element()
            } else {
                div().into_any_element()
            })
            .into_any_element()
    }

    pub(in crate::ui::app) fn apply_process_suggestion(
        &mut self,
        target: SuggestionTarget,
        process: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let display_name = Path::new(process)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(process);
        self.selected_process_paths
            .insert(target, process.to_owned());
        let input = target.input(&self.inputs).clone();
        clear_input_to(&input, display_name, window, cx);
    }

    pub(in crate::ui::app) fn process_picker_path(
        &self,
        target: SuggestionTarget,
        input: &Entity<InputState>,
        cx: &mut Context<Self>,
    ) -> String {
        let display_name = input.read(cx).value();
        self.selected_process_paths
            .get(&target)
            .filter(|path| process_path_matches_display_name(path, display_name.as_ref()))
            .cloned()
            .unwrap_or_default()
    }
}

fn process_candidate_can_accept(
    target: SuggestionTarget,
    settings: &Settings,
    process: &ProcessCandidate,
) -> bool {
    process_target_can_accept(
        target,
        settings,
        process.image_path.to_string_lossy().as_ref(),
        process.has_suspendable_instance,
    )
}

fn process_candidate_is_visible(
    target: SuggestionTarget,
    settings: &Settings,
    process: &ProcessCandidate,
) -> bool {
    target == SuggestionTarget::AppSuspension
        || process_candidate_can_accept(target, settings, process)
}
