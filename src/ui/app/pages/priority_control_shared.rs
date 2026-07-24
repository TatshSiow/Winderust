use crate::ui::app::*;

impl WinderustApp {
    pub(in crate::ui::app) fn render_io_priority_exclusions(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.render_priority_exclusion_table(
            "io-priority-exclusion",
            &self.settings.io_priority.exclusions,
            ListItemRemovalKind::IoPriorityExclusion,
            t!("io_priority.no_exclusions").to_string(),
            window,
            cx,
        )
    }

    pub(in crate::ui::app) fn render_priority_exclusion_table(
        &self,
        id_prefix: &'static str,
        rules: &[ProcessExclusionRule],
        kind: ListItemRemovalKind,
        empty_message: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let mut table = v_flex()
            .w_full()
            .min_w(px(0.0))
            .overflow_hidden()
            .rounded(px(BRAND_RADIUS_SURFACE))
            .border_1()
            .border_color(rgb(border_color()))
            .bg(rgb(settings_card_color()))
            .child(
                h_flex()
                    .w_full()
                    .min_w(px(0.0))
                    .h(px(32.0))
                    .items_center()
                    .gap_2()
                    .px_4()
                    .border_b_1()
                    .border_color(rgb(border_color()))
                    .text_size(px(TEXT_LABEL_SIZE))
                    .line_height(px(TEXT_LABEL_LINE_HEIGHT))
                    .text_color(rgb(muted_text_color()))
                    .child(rule_table_active_header())
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .truncate()
                            .child(t!("process_list.process_name").to_string()),
                    )
                    .child(priority_exclusion_table_cell(
                        t!("process_list.foreground").to_string(),
                    ))
                    .child(priority_exclusion_table_cell(
                        t!("process_list.background").to_string(),
                    ))
                    .child(rule_table_action_header()),
            );

        for (index, rule) in rules.iter().enumerate() {
            let process = rule.process_name.clone();
            let target = ListItemRemovalTarget::new(kind, index);
            let row = h_flex()
                .id(SharedString::from(format!("{id_prefix}-row-{index}")))
                .w_full()
                .min_w(px(0.0))
                .h(px(CARD_ROW_HEIGHT))
                .items_center()
                .gap_2()
                .px_4()
                .border_b_1()
                .border_color(rgb(border_color()))
                .text_size(px(TEXT_BODY_SIZE))
                .line_height(px(TEXT_BODY_LINE_HEIGHT))
                .child(rule_active_cell(
                    format!("{id_prefix}-enabled-{index}"),
                    rule.enabled,
                    cx.listener(move |app, checked, _, cx| {
                        app.set_priority_exclusion_enabled(kind, index, *checked);
                        cx.notify();
                    }),
                ))
                .child(self.process_rule_title(&process, cx))
                .child(self.render_priority_exclusion_dropdown(kind, index, true, window, cx))
                .child(self.render_priority_exclusion_dropdown(kind, index, false, window, cx))
                .child(rule_table_action_cell(
                    danger_control_button(Button::new(SharedString::from(format!(
                        "remove-{id_prefix}-{index}"
                    ))))
                    .with_size(px(32.0))
                    .icon(Icon::new(NavIcon::Trash2).with_size(px(14.0)))
                    .tooltip(t!("common.remove").to_string())
                    .on_click(cx.listener(move |app, _, _, cx| {
                        app.request_list_item_removal(target, cx);
                    }))
                    .into_any_element(),
                ));

            table = table.child(self.animated_list_item(
                target,
                SharedString::from(format!("{id_prefix}-{index}")),
                row.into_any_element(),
            ));
        }

        if rules.is_empty() {
            table = table.child(text_muted(empty_message).p_4());
        }

        table.into_any_element()
    }

    pub(in crate::ui::app) fn render_priority_exclusion_dropdown(
        &self,
        kind: ListItemRemovalKind,
        index: usize,
        foreground: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match kind {
            ListItemRemovalKind::ProcessPriorityExclusion => {
                let selected = self.settings.process_priority.exclusions[index]
                    .process_priority_override(foreground);
                let priorities: &[ProcessPrioritySetting] =
                    if self.settings.advanced.expose_all_priority_values {
                        &ProcessPrioritySetting::CUSTOM_RULE_ADVANCED_ALL
                    } else {
                        &ProcessPrioritySetting::CUSTOM_RULE_ALL
                    };
                self.render_priority_rule_dropdown(
                    "process-priority-exclusion",
                    index,
                    foreground,
                    selected,
                    priorities,
                    process_priority_setting_label,
                    |app, index, foreground, priority| {
                        if let Some(rule) = app.settings.process_priority.exclusions.get_mut(index)
                        {
                            rule.set_process_priority_override(foreground, priority);
                        }
                    },
                    window,
                    cx,
                )
            }
            ListItemRemovalKind::ThreadPriorityExclusion => {
                let selected = self.settings.thread_priority.exclusions[index]
                    .thread_priority_override(foreground);
                let priorities: &[ProcessThreadPrioritySetting] =
                    if self.settings.advanced.expose_all_priority_values {
                        &ProcessThreadPrioritySetting::CUSTOM_RULE_ADVANCED_ALL
                    } else {
                        &ProcessThreadPrioritySetting::CUSTOM_RULE_ALL
                    };
                self.render_priority_rule_dropdown(
                    "thread-priority-exclusion",
                    index,
                    foreground,
                    selected,
                    priorities,
                    process_thread_priority_setting_label,
                    |app, index, foreground, priority| {
                        if let Some(rule) = app.settings.thread_priority.exclusions.get_mut(index) {
                            rule.set_thread_priority_override(foreground, priority);
                        }
                    },
                    window,
                    cx,
                )
            }
            ListItemRemovalKind::DynamicPriorityBoostExclusion => {
                let selected = self.settings.dynamic_priority_boost.exclusions[index]
                    .dynamic_priority_boost_override(foreground);
                self.render_priority_rule_dropdown(
                    "dynamic-priority-boost-exclusion",
                    index,
                    foreground,
                    selected,
                    &ProcessDynamicPriorityBoostSetting::CUSTOM_RULE_ALL,
                    process_dynamic_priority_boost_setting_label,
                    |app, index, foreground, boost| {
                        if let Some(rule) = app
                            .settings
                            .dynamic_priority_boost
                            .exclusions
                            .get_mut(index)
                        {
                            rule.set_dynamic_priority_boost_override(foreground, boost);
                        }
                    },
                    window,
                    cx,
                )
            }
            ListItemRemovalKind::IoPriorityExclusion => {
                let selected =
                    self.settings.io_priority.exclusions[index].io_priority_override(foreground);
                let priorities: &[ProcessIoPrioritySetting] =
                    if self.settings.advanced.expose_all_priority_values {
                        &ProcessIoPrioritySetting::CUSTOM_RULE_ADVANCED_ALL
                    } else {
                        &ProcessIoPrioritySetting::CUSTOM_RULE_ALL
                    };
                self.render_priority_rule_dropdown(
                    "io-priority-exclusion",
                    index,
                    foreground,
                    selected,
                    priorities,
                    process_io_priority_setting_label,
                    |app, index, foreground, priority| {
                        if let Some(rule) = app.settings.io_priority.exclusions.get_mut(index) {
                            rule.set_io_priority_override(foreground, priority);
                        }
                    },
                    window,
                    cx,
                )
            }
            ListItemRemovalKind::GpuPriorityExclusion => {
                let selected =
                    self.settings.gpu_priority.exclusions[index].gpu_priority_override(foreground);
                let priorities: &[ProcessGpuPrioritySetting] =
                    if self.settings.advanced.expose_all_priority_values {
                        &ProcessGpuPrioritySetting::CUSTOM_RULE_ADVANCED_ALL
                    } else {
                        &ProcessGpuPrioritySetting::CUSTOM_RULE_ALL
                    };
                self.render_priority_rule_dropdown(
                    "gpu-priority-exclusion",
                    index,
                    foreground,
                    selected,
                    priorities,
                    process_gpu_priority_setting_label,
                    |app, index, foreground, priority| {
                        if let Some(rule) = app.settings.gpu_priority.exclusions.get_mut(index) {
                            rule.set_gpu_priority_override(foreground, priority);
                        }
                    },
                    window,
                    cx,
                )
            }
            ListItemRemovalKind::MemoryPriorityExclusion => {
                let selected = self.settings.memory_priority.exclusions[index]
                    .memory_priority_override(foreground);
                self.render_priority_rule_dropdown(
                    "memory-priority-exclusion",
                    index,
                    foreground,
                    selected,
                    &ProcessMemoryPrioritySetting::CUSTOM_RULE_ALL,
                    process_memory_priority_setting_label,
                    |app, index, foreground, priority| {
                        if let Some(rule) = app.settings.memory_priority.exclusions.get_mut(index) {
                            rule.set_memory_priority_override(foreground, priority);
                        }
                    },
                    window,
                    cx,
                )
            }
            _ => priority_exclusion_table_cell(t!("common.none").to_string()),
        }
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "keeps six custom-rule dropdowns on one rendering path"
    )]
    fn render_priority_rule_dropdown<T>(
        &self,
        id_prefix: &'static str,
        index: usize,
        foreground: bool,
        selected: T,
        values: &[T],
        label: impl Fn(T) -> String + Copy + 'static,
        set: impl Fn(&mut Self, usize, bool, T) + Copy + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement
    where
        T: Copy + PartialEq + std::fmt::Debug + 'static,
    {
        let side = if foreground {
            "foreground"
        } else {
            "background"
        };
        self.render_dropdown_select(
            format!("{id_prefix}-{side}-{index}"),
            label(selected),
            true,
            DropdownSelectWidth::Table,
            values.len(),
            window,
            cx,
            |max_height, cx| {
                let mut options = dropdown_surface(cx, max_height);
                for value in values.iter().copied() {
                    options = options.child(
                        dropdown_option_row(
                            SharedString::from(format!("{id_prefix}-{side}-{index}-{value:?}")),
                            label(value),
                            selected == value,
                            cx,
                        )
                        .on_click(cx.listener(move |app, _, _, cx| {
                            set(app, index, foreground, value);
                            app.active_power_plan_picker = None;
                            cx.notify();
                        })),
                    );
                }
                options
            },
        )
    }

    pub(in crate::ui::app) fn set_priority_exclusion_enabled(
        &mut self,
        kind: ListItemRemovalKind,
        index: usize,
        enabled: bool,
    ) {
        let rule = match kind {
            ListItemRemovalKind::ProcessPriorityExclusion => {
                self.settings.process_priority.exclusions.get_mut(index)
            }
            ListItemRemovalKind::ThreadPriorityExclusion => {
                self.settings.thread_priority.exclusions.get_mut(index)
            }
            ListItemRemovalKind::DynamicPriorityBoostExclusion => self
                .settings
                .dynamic_priority_boost
                .exclusions
                .get_mut(index),
            ListItemRemovalKind::IoPriorityExclusion => {
                self.settings.io_priority.exclusions.get_mut(index)
            }
            ListItemRemovalKind::GpuPriorityExclusion => {
                self.settings.gpu_priority.exclusions.get_mut(index)
            }
            ListItemRemovalKind::MemoryPriorityExclusion => {
                self.settings.memory_priority.exclusions.get_mut(index)
            }
            _ => None,
        };

        if let Some(rule) = rule {
            rule.enabled = enabled;
        }
    }

    pub(in crate::ui::app) fn render_io_priority_default_selector(
        &self,
        target: IoPriorityDefaultTarget,
        selected_priority: ProcessIoPrioritySetting,
        enabled: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = match target {
            IoPriorityDefaultTarget::Background => "io-priority-background-default",
            IoPriorityDefaultTarget::Foreground => "io-priority-foreground-default",
        };
        let priorities: &[ProcessIoPrioritySetting] =
            if self.settings.advanced.expose_all_priority_values {
                &ProcessIoPrioritySetting::ADVANCED_ALL
            } else {
                &ProcessIoPrioritySetting::ALL
            };
        let dropdown = self.render_dropdown_select(
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
                                    app.settings.io_priority.background_priority = priority;
                                }
                                IoPriorityDefaultTarget::Foreground => {
                                    app.settings.io_priority.foreground_priority = priority;
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
        dropdown
    }
}
