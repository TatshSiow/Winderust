use crate::ui::app::*;

impl WinderustApp {
    pub(in crate::ui::app) fn render_by_time_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let enabled = self.settings.by_time.enabled;
        let help = tooltip_lines(vec![
            t!("by_time.intro_1").to_string(),
            t!("by_time.intro_2").to_string(),
            t!("common.power_plan_priority").to_string(),
            t!("common.power_plan_pause_priority").to_string(),
        ]);
        let mut content = self
            .page_shell(Page::ByTime, cx)
            .child(feature_toggle_switch_with_help(
                "schedule-enabled",
                t!("by_time.enable").to_string(),
                help,
                enabled,
                cx.listener(|app, checked, _, cx| {
                    app.settings.by_time.enabled = *checked;
                    cx.notify();
                }),
            ));

        let mut body =
            feature_body(enabled).child(section_title_text(t!("common.rules").to_string()));
        body = body.child(create_rule_card(
            "create-time-rule-card",
            t!("by_time.rule_title").to_string(),
            primary_control_button(Button::new("add-time-rule"), cx)
                .label(t!("common.create").to_string())
                .disabled(!enabled)
                .on_click(cx.listener(|app, _, window, cx| {
                    app.settings.by_time.rules.push(ByTimeRule {
                        enabled: true,
                        name: t!("by_time.new_rule").to_string(),
                        days: WeekdaySetting::all().to_vec(),
                        start_time: "22:00".to_owned(),
                        end_time: "08:00".to_owned(),
                        power_plan_guid: app.current_plan.as_ref().map(|plan| plan.guid.clone()),
                    });
                    app.inputs.ensure_for_settings(window, cx, &app.settings);
                    cx.notify();
                }))
                .into_any_element(),
        ));
        let mut rules = rule_list(vec![
            rule_table_active_header(),
            rule_table_title_input_header(t!("common.rule_name").to_string()),
            priority_exclusion_table_cell(t!("by_time.days").to_string()),
            rule_table_centered_header(t!("by_time.start").to_string(), 96.0),
            rule_table_centered_header(t!("by_time.end").to_string(), 96.0),
            rule_table_centered_header(
                t!("by_time.target_power_plan").to_string(),
                DROPDOWN_SELECT_STANDARD_WIDTH,
            ),
            rule_table_action_header(),
        ]);
        for (index, rule) in self.settings.by_time.rules.iter().enumerate() {
            rules = rules.child(self.animated_list_item(
                ListItemRemovalTarget::new(ListItemRemovalKind::ByTimeRule, index),
                SharedString::from(format!("schedule-rule-{index}")),
                self.render_by_time_rule(index, rule, window, cx),
            ));
        }
        if self.settings.by_time.rules.is_empty() {
            rules = rules.child(text_muted(t!("common.no_custom_rules").to_string()).p_4());
        }
        body = body.child(rules);
        content = content.child(disabled_feature_body("schedule-body", body, enabled, cx));

        content.into_any_element()
    }

    pub(in crate::ui::app) fn render_by_time_rule(
        &self,
        index: usize,
        rule: &ByTimeRule,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let Some(name_input) = self.inputs.by_time_rule_names.get(index).cloned() else {
            return syncing_rule_card(index);
        };
        let name_focused = name_input.read(cx).focus_handle(cx).is_focused(window);
        let start_input = self.inputs.schedule_start_times.get(index).cloned();
        let end_input = self.inputs.schedule_end_times.get(index).cloned();

        compact_rule_row(format!("schedule-rule-row-{index}"))
            .child(rule_active_cell(
                format!("schedule-rule-enabled-{index}"),
                rule.enabled,
                cx.listener(move |app, checked, _, cx| {
                    if let Some(rule) = app.settings.by_time.rules.get_mut(index) {
                        rule.enabled = *checked;
                    }
                    cx.notify();
                }),
            ))
            .child(rule_table_title_input_cell(app_input(
                &name_input,
                name_focused,
                cx,
            )))
            .child(self.render_by_time_days_dropdown(index, &rule.days, window, cx))
            .child(match start_input {
                Some(input) => rule_table_input_cell(input, 96.0, window, cx).into_any_element(),
                None => text_muted(t!("common.unknown").to_string()).into_any_element(),
            })
            .child(match end_input {
                Some(input) => rule_table_input_cell(input, 96.0, window, cx).into_any_element(),
                None => text_muted(t!("common.unknown").to_string()).into_any_element(),
            })
            .child(self.render_inline_power_plan_picker(
                format!("schedule-rule-plan-{index}"),
                rule.power_plan_guid.clone(),
                PowerPlanField::ByTimeRule(index),
                window,
                cx,
            ))
            .child(rule_table_action_cell(
                remove_control_button(Button::new(SharedString::from(format!(
                    "remove-schedule-rule-{index}"
                ))))
                .on_click(cx.listener(move |app, _, _, cx| {
                    app.request_list_item_removal(
                        ListItemRemovalTarget::new(ListItemRemovalKind::ByTimeRule, index),
                        cx,
                    );
                }))
                .into_any_element(),
            ))
            .into_any_element()
    }

    pub(in crate::ui::app) fn render_by_time_days_dropdown(
        &self,
        index: usize,
        days: &[WeekdaySetting],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.render_dropdown_select(
            format!("schedule-days-{index}"),
            schedule_days_label(days),
            true,
            DropdownSelectWidth::Table,
            WeekdaySetting::all().len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for day in WeekdaySetting::all() {
                    let selected = days.contains(&day);
                    let option_id = SharedString::from(format!("schedule-days-{index}-{day:?}"));
                    let motion_id = format!("checkbox-{option_id}");
                    let progress = control_motion_progress(&motion_id, selected);
                    options = options.child(
                        h_flex()
                            .id(option_id.clone())
                            .relative()
                            .min_h(px(40.0))
                            .items_center()
                            .gap_2()
                            .pl_3()
                            .pr_3()
                            .rounded(px(BRAND_RADIUS_CONTROL))
                            .text_size(px(TEXT_CONTROL_SIZE))
                            .line_height(px(TEXT_CONTROL_LINE_HEIGHT))
                            .text_color(cx.theme().popover_foreground)
                            .hover(|style| style.bg(rgb(dropdown_option_hover_color())))
                            .cursor_pointer()
                            .child(checkbox_box(
                                SharedString::from(format!("{option_id}-box")),
                                16.0,
                                SharedString::from(format!("{option_id}-mark")),
                                accent_glyph_color(accent_color()),
                                progress,
                            ))
                            .child(weekday_short_label(day))
                            .on_click(cx.listener(move |app, _, _, cx| {
                                if let Some(rule) = app.settings.by_time.rules.get_mut(index) {
                                    let next = !rule.days.contains(&day);
                                    begin_control_motion(motion_id.clone(), next, cx);
                                    if next {
                                        rule.days.push(day);
                                    } else {
                                        rule.days.retain(|existing| *existing != day);
                                    }
                                }
                                cx.notify();
                            })),
                    );
                }
                options
            },
        )
    }
}
