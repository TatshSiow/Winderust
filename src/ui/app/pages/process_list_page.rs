use crate::ui::app::*;
use gpui::rgba;

mod editing;
mod table_model;

pub(in crate::ui::app) use table_model::*;

impl WinderustApp {
    pub(in crate::ui::app) fn render_process_list_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let search_query = self.inputs.process_list_search.read(cx).value().to_string();
        let render_data = process_list_render_data(self, window, &search_query);
        let process_count = render_data.process_count;
        let table_scroll_height = process_list_scroll_height(window);
        let column_layout = render_data.column_layout;
        let table_width = render_data.table_width;
        let rendered_rows = render_data.rows;
        let item_sizes = render_data.item_sizes;
        let horizontal_scroll_handle = window
            .use_keyed_state("process-list-horizontal-scroll", cx, |_, _| {
                ScrollHandle::default()
            })
            .read(cx)
            .clone();
        let vertical_scroll_handle = window
            .use_keyed_state("process-list-virtual-scroll", cx, |_, _| {
                VirtualListScrollHandle::new()
            })
            .read(cx)
            .clone();
        let refresh_in_progress = self.process_refresh_in_progress;
        let search_focused = self
            .inputs
            .process_list_search
            .read(cx)
            .focus_handle(cx)
            .is_focused(window);
        let search_input = div().w(px(280.0)).max_w_full().child(app_input(
            &self.inputs.process_list_search,
            search_focused,
            cx,
        ));
        let refresh_button = control_button(Button::new("refresh-process-list"))
            .label(t!("settings.refresh").to_string())
            .disabled(refresh_in_progress)
            .on_click(cx.listener(|app, _, _, cx| {
                if app.refresh_running_processes(true, cx) {
                    cx.notify();
                }
            }));
        let hide_limited_access = checkbox(
            "hide-limited-access-processes",
            t!("process_list.hide_limited_access_items").to_string(),
            self.hide_limited_access_processes,
            cx.listener(|app, checked, _, cx| {
                app.hide_limited_access_processes = *checked;
                cx.notify();
            }),
        );
        let header = process_list_scroll_content(table_width).child(process_list_header_row(
            &self.settings,
            &column_layout,
            self.process_list_sort,
            cx,
        ));
        let rows = if rendered_rows.is_empty() {
            let message = if search_query.trim().is_empty() {
                process_load_state_message(&self.running_process_load_state)
                    .unwrap_or_else(|| t!("common.no_running_apps_loaded").to_string())
            } else {
                t!("process_list.no_matches").to_string()
            };
            process_list_scroll_content(table_width)
                .child(process_list_empty_row(message))
                .into_any_element()
        } else {
            let column_layout = column_layout.clone();
            let rows = Rc::clone(&rendered_rows);

            v_virtual_list(
                cx.entity(),
                "process-list-rows",
                item_sizes,
                move |app, visible_range, window, cx| {
                    let row_layout = ProcessListRenderLayout {
                        column_layout: &column_layout,
                    };
                    let edit_context = ProcessListEditContext { app, window };

                    visible_range
                        .filter_map(|row_index| {
                            rows.get(row_index).map(|row| {
                                process_list_rendered_row(row, row_layout, edit_context, cx)
                            })
                        })
                        .collect::<Vec<_>>()
                },
            )
            .track_scroll(&vertical_scroll_handle)
            .into_any_element()
        };

        let details_modal = self
            .process_details
            .as_ref()
            .map(|_| self.render_process_details_modal(window, cx));

        self.page_shell(Page::ProcessList, cx)
            .relative()
            .flex_1()
            .h_full()
            .min_h(px(0.0))
            .overflow_hidden()
            .child(
                h_flex()
                    .w_full()
                    .min_w(px(0.0))
                    .flex_shrink_0()
                    .items_center()
                    .justify_between()
                    .gap_2()
                    .flex_wrap()
                    .child(div().flex_1().min_w(px(0.0)).child(search_input))
                    .child(
                        h_flex()
                            .flex_none()
                            .items_center()
                            .gap_2()
                            .child(text_muted(process_list_toolbar_label(self, process_count)))
                            .child(div().flex_none().child(hide_limited_access))
                            .child(refresh_button),
                    ),
            )
            .child(
                div()
                    .w_full()
                    .min_w(px(0.0))
                    .h(table_scroll_height)
                    .max_h(table_scroll_height)
                    .min_h(px(0.0))
                    .relative()
                    .overflow_hidden()
                    .child(
                        process_list_surface()
                            .child(
                                div()
                                    .id("process-list-header-scroll-area")
                                    .w_full()
                                    .h(px(PROCESS_LIST_HEADER_HEIGHT))
                                    .rounded_t(px(BRAND_RADIUS_SURFACE))
                                    .bg(rgb(panel_active_color()))
                                    .border_b_1()
                                    .border_color(rgb(border_color()))
                                    .overflow_scroll()
                                    .track_scroll(&horizontal_scroll_handle)
                                    .child(header),
                            )
                            .child(
                                div()
                                    .id("process-list-rows-viewport")
                                    .relative()
                                    .flex_1()
                                    .min_h(px(0.0))
                                    .overflow_hidden()
                                    .child(
                                        div()
                                            .id("process-list-horizontal-scroll-area")
                                            .size_full()
                                            .overflow_scroll()
                                            .track_scroll(&horizontal_scroll_handle)
                                            .child(
                                                div()
                                                    .id("process-list-vertical-scroll-area")
                                                    .w(table_width)
                                                    .min_w(table_width)
                                                    .h_full()
                                                    .min_h(px(0.0))
                                                    .child(rows),
                                            ),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .absolute()
                            .top_0()
                            .right_0()
                            .bottom(px(PROCESS_LIST_SCROLLBAR_GUTTER))
                            .w(px(PROCESS_LIST_SCROLLBAR_GUTTER))
                            .child(Scrollbar::vertical(&vertical_scroll_handle)),
                    )
                    .child(
                        div()
                            .absolute()
                            .left_0()
                            .right(px(PROCESS_LIST_SCROLLBAR_GUTTER))
                            .bottom_0()
                            .h(px(PROCESS_LIST_SCROLLBAR_GUTTER))
                            .child(Scrollbar::horizontal(&horizontal_scroll_handle)),
                    ),
            )
            .when_some(details_modal, |page, modal| page.child(modal))
            .into_any_element()
    }

    fn render_process_details_modal(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let details = self
            .process_details
            .as_ref()
            .expect("details view requires a draft");
        let path = details.executable_path.clone();
        let summary = process_policy_summary(&self.settings, &self.plans, &path);
        let icon = self
            .process_icon_cache
            .get(Path::new(&path))
            .and_then(Option::as_ref);
        let groups = [
            (
                t!("process_list.details_exclusions").to_string(),
                &[
                    ProcessListColumn::AdaptiveEngine,
                    ProcessListColumn::BackgroundEfficiency,
                ][..],
            ),
            (
                t!("process_list.details_power").to_string(),
                &[
                    ProcessListColumn::PowerPlanForeground,
                    ProcessListColumn::PowerPlanRunning,
                ][..],
            ),
            (
                t!("process_list.details_priority").to_string(),
                &[
                    ProcessListColumn::ProcessPriority,
                    ProcessListColumn::ThreadPriority,
                    ProcessListColumn::DynamicPriorityBoost,
                    ProcessListColumn::IoPriority,
                    ProcessListColumn::GpuPriority,
                    ProcessListColumn::MemoryPriority,
                ][..],
            ),
        ];

        let mut content = page_body_shell().gap_4().p_4();
        for (title, columns) in groups {
            let mut group = page_body_shell().child(section_title_text(title));
            for column in columns {
                let value = process_list_column_value(&summary, *column);
                let dropdown_id = process_list_cell_editor_id(&path, *column);
                let dropdown = self.render_dropdown_select(
                    dropdown_id.clone(),
                    value,
                    true,
                    DropdownSelectWidth::Standard,
                    process_list_cell_editor_option_count(*column, self),
                    window,
                    cx,
                    |max_height, cx| {
                        process_list_cell_editor_options(&path, *column, self, max_height, cx)
                    },
                );
                group = group.child(
                    setting_action_card(
                        format!("{dropdown_id}-card"),
                        process_list_column_label(*column, &self.settings),
                        dropdown,
                    )
                    .into_any_element(),
                );
            }
            content = content.child(group);
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
            .on_any_mouse_down(|_, _, cx| {
                cx.stop_propagation();
            })
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
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w(px(0.0))
                            .gap_3()
                            .child(process_icon_cell(icon, cx))
                            .child(
                                v_flex()
                                    .flex_1()
                                    .min_w(px(0.0))
                                    .child(section_title_text(details.display_name.clone()))
                                    .child(text_muted(path.clone()).truncate()),
                            ),
                    )
                    .child(
                        control_button(Button::new("close-process-details"))
                            .icon(Icon::new(NavIcon::X).with_size(px(14.0)))
                            .on_click(cx.listener(|app, _, _, cx| {
                                app.save_process_details(cx);
                            })),
                    ),
            )
            .child(
                div()
                    .id("process-details-scroll")
                    .flex_1()
                    .min_h(px(0.0))
                    .overflow_y_scrollbar()
                    .child(content),
            );

        let modal = with_optional_motion(
            modal,
            "process-details-modal-open",
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
                app.save_process_details(cx);
            }))
            .child(modal);

        with_optional_motion(
            backdrop,
            "process-details-backdrop-open",
            MotionSpeed::Fast,
            |backdrop| backdrop,
            |backdrop, delta| backdrop.opacity(delta),
        )
    }
}

pub(in crate::ui::app) fn process_list_scroll_height(window: &Window) -> Pixels {
    let reserved_height = TITLE_BAR_HEIGHT
        + PAGE_HEADER_HEIGHT
        + PAGE_CONTENT_VERTICAL_PADDING * 2.0
        + PROCESS_LIST_TOOLBAR_HEIGHT
        + PROCESS_LIST_VERTICAL_GAP_TOTAL;

    (window.viewport_size().height - px(reserved_height)).max(Pixels::ZERO)
}

pub(in crate::ui::app) fn process_list_surface() -> gpui::Div {
    v_flex()
        .size_full()
        .min_w(px(0.0))
        .min_h(px(0.0))
        .relative()
        .overflow_hidden()
        .rounded(px(BRAND_RADIUS_SURFACE))
        .bg(rgb(settings_card_color()))
        .text_color(rgb(primary_text_color()))
        .text_size(px(TEXT_BODY_SIZE))
        .line_height(px(TEXT_BODY_LINE_HEIGHT))
}

pub(in crate::ui::app) fn process_list_scroll_content(table_width: Pixels) -> gpui::Div {
    v_flex().w(table_width).min_w(table_width)
}

pub(in crate::ui::app) fn process_list_header_row(
    settings: &Settings,
    layout: &ProcessListColumnLayout,
    sort: ProcessListSort,
    cx: &mut Context<WinderustApp>,
) -> gpui::Div {
    let mut row = h_flex()
        .w_full()
        .min_w(px(0.0))
        .h(px(PROCESS_LIST_HEADER_HEIGHT))
        .items_center()
        .gap_3()
        .px_4()
        .py_2()
        .text_size(px(TEXT_LABEL_SIZE))
        .line_height(px(TEXT_LABEL_LINE_HEIGHT))
        .text_color(rgb(muted_text_color()))
        .child(process_list_header_cell(
            layout.name_width,
            t!("process_list.process_name").to_string(),
            ProcessListSortColumn::ProcessName,
            sort,
            cx,
        ));

    for column in PROCESS_LIST_OVERVIEW_COLUMNS {
        row = row.child(process_list_header_cell(
            layout.column_width(column),
            process_list_column_label(column, settings),
            ProcessListSortColumn::Column(column),
            sort,
            cx,
        ));
    }

    row
}

pub(in crate::ui::app) fn process_list_priority_header_label(
    label: String,
    has_foreground_background_split: bool,
) -> String {
    if has_foreground_background_split {
        format!(
            "{} ({}/{})",
            label,
            process_list_foreground_short_label(),
            process_list_background_short_label()
        )
    } else {
        label
    }
}

pub(in crate::ui::app) fn process_list_foreground_short_label() -> &'static str {
    "FG"
}

pub(in crate::ui::app) fn process_list_background_short_label() -> &'static str {
    "BG"
}

pub(in crate::ui::app) fn process_list_header_cell(
    width: f32,
    label: String,
    column: ProcessListSortColumn,
    sort: ProcessListSort,
    cx: &mut Context<WinderustApp>,
) -> AnyElement {
    let active = sort.column == column;
    let numeric = matches!(
        column,
        ProcessListSortColumn::Column(ProcessListColumn::CpuUsage | ProcessListColumn::MemoryUsage)
    );

    let header = h_flex()
        .id(SharedString::from(format!(
            "process-list-sort-header-{}",
            process_list_sort_column_id(column)
        )))
        .w(px(width))
        .min_w(px(0.0))
        .flex_shrink_0()
        .items_center()
        .gap(px(PROCESS_LIST_SORT_HEADER_GAP))
        .rounded(px(BRAND_RADIUS_CONTROL))
        .text_color(rgb(if active {
            accent_color()
        } else {
            muted_text_color()
        }))
        .cursor_pointer()
        .hover(|style| style.text_color(rgb(primary_text_color())))
        .on_click(cx.listener(move |app, _, _, cx| {
            app.toggle_process_list_sort(column, cx);
        }));
    let label = div()
        .flex_1()
        .min_w(px(0.0))
        .truncate()
        .when(numeric, |label| label.text_right().pr_2())
        .child(label);
    let sort_icon = process_list_sort_icon(active, sort.direction, cx);

    if numeric {
        header.child(sort_icon).child(label).into_any_element()
    } else {
        header.child(label).child(sort_icon).into_any_element()
    }
}

pub(in crate::ui::app) fn process_list_sort_column_id(
    column: ProcessListSortColumn,
) -> &'static str {
    match column {
        ProcessListSortColumn::ProcessName => "process-name",
        ProcessListSortColumn::Column(ProcessListColumn::Pid) => "pid",
        ProcessListSortColumn::Column(ProcessListColumn::Status) => "status",
        ProcessListSortColumn::Column(ProcessListColumn::CpuUsage) => "cpu-usage",
        ProcessListSortColumn::Column(ProcessListColumn::MemoryUsage) => "memory-usage",
        ProcessListSortColumn::Column(ProcessListColumn::PowerPlanForeground) => {
            "power-plan-foreground"
        }
        ProcessListSortColumn::Column(ProcessListColumn::PowerPlanRunning) => "power-plan-running",
        ProcessListSortColumn::Column(ProcessListColumn::AdaptiveEngine) => "adaptive-engine",
        ProcessListSortColumn::Column(ProcessListColumn::BackgroundEfficiency) => {
            "background-efficiency"
        }
        ProcessListSortColumn::Column(ProcessListColumn::ProcessPriority) => "process-priority",
        ProcessListSortColumn::Column(ProcessListColumn::ThreadPriority) => "thread-priority",
        ProcessListSortColumn::Column(ProcessListColumn::DynamicPriorityBoost) => {
            "dynamic-priority-boost"
        }
        ProcessListSortColumn::Column(ProcessListColumn::IoPriority) => "io-priority",
        ProcessListSortColumn::Column(ProcessListColumn::GpuPriority) => "gpu-priority",
        ProcessListSortColumn::Column(ProcessListColumn::MemoryPriority) => "memory-priority",
    }
}

pub(in crate::ui::app) fn process_list_sort_icon(
    active: bool,
    direction: ProcessListSortDirection,
    cx: &mut Context<WinderustApp>,
) -> gpui::Div {
    let turns = match direction {
        ProcessListSortDirection::Ascending => 180.0 / 360.0,
        ProcessListSortDirection::Descending => 0.0,
    };
    let mut icon = div()
        .w(px(PROCESS_LIST_SORT_ICON_WIDTH))
        .min_w(px(PROCESS_LIST_SORT_ICON_WIDTH))
        .flex_shrink_0()
        .flex()
        .items_center()
        .justify_center();

    if active {
        icon = icon.child(
            Icon::new(NavIcon::ChevronDown)
                .with_size(px(12.0))
                .text_color(cx.theme().accent)
                .rotate(percentage(turns)),
        );
    }

    icon
}

pub(in crate::ui::app) fn process_list_empty_row(message: impl Into<SharedString>) -> gpui::Div {
    h_flex()
        .w_full()
        .min_w(px(0.0))
        .h(px(CARD_ROW_HEIGHT))
        .items_center()
        .px_4()
        .py_3()
        .child(text_muted(message.into()))
}

pub(in crate::ui::app) fn process_list_rendered_row(
    row: &ProcessListRenderedRow,
    layout: ProcessListRenderLayout<'_>,
    edit_context: ProcessListEditContext<'_>,
    cx: &mut Context<WinderustApp>,
) -> AnyElement {
    match row {
        ProcessListRenderedRow::Entry {
            process,
            summary,
            icon,
            state,
        } => process_list_entry_row(
            process,
            summary.as_ref(),
            icon.as_ref(),
            *state,
            layout,
            edit_context,
            cx,
        ),
        ProcessListRenderedRow::Group {
            process_id,
            process_name,
            executable_path,
            process_count,
            summary,
            icon,
            state,
        } => process_list_group_row(
            ProcessListGroupRowData {
                process_id: *process_id,
                process_name: process_name.as_str(),
                executable_path: executable_path.as_str(),
                process_count: *process_count,
            },
            summary.as_ref(),
            icon.as_ref(),
            *state,
            layout,
            edit_context,
            cx,
        )
        .into_any_element(),
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "menu construction needs the selected process context"
)]
fn process_list_context_menu(
    mut menu: PopupMenu,
    app_entity: Entity<WinderustApp>,
    process_id: u32,
    process_name: String,
    executable_path: String,
    suspended: bool,
    show_suspend: bool,
    expose_all_priorities: bool,
    hide_stop_process: bool,
    window: &mut Window,
    menu_cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let target = capture_process_action_target(process_id, Path::new(&executable_path));
    let mutations_disabled = target.as_ref().map_or(true, |target| {
        ensure_process_action_target_mutable(target).is_err()
    });
    let suspension_disabled = mutations_disabled
        || target
            .as_ref()
            .is_ok_and(|target| app_suspension::is_builtin_excluded(&target.name));
    let efficiency_disabled = mutations_disabled
        || target
            .as_ref()
            .is_ok_and(|target| background_efficiency::is_builtin_excluded(&target.name));

    menu = menu.item(process_list_rule_details_menu_item(
        app_entity.clone(),
        Some(process_id),
        process_name.clone(),
        executable_path.clone(),
    ));
    menu = menu.separator();
    for (tree, label_key) in [
        (false, "process_list.stop_process"),
        (true, "process_list.stop_process_tree"),
    ] {
        if hide_stop_process && !tree {
            continue;
        }
        let app_entity = app_entity.clone();
        let process_name = process_name.clone();
        let target = target.clone();
        menu = menu.item(
            process_list_value_menu_item(
                t!(label_key).to_string(),
                ProcessListMenuItemTone::Danger,
                mutations_disabled,
            )
            .disabled(mutations_disabled)
            .on_click(move |_, window, cx| {
                let description = t!(
                    if tree {
                        "process_list.stop_tree_confirm"
                    } else {
                        "process_list.stop_confirm"
                    },
                    name = process_name.as_str()
                )
                .to_string();
                let answers = [
                    PromptButton::ok(t!("common.yes").to_string()),
                    PromptButton::cancel(t!("common.no").to_string()),
                ];
                let answer = window.prompt(
                    PromptLevel::Warning,
                    t!("process_list.stop_confirm_title").as_ref(),
                    Some(&description),
                    &answers,
                    cx,
                );
                let app_entity = app_entity.clone();
                let process_name = process_name.clone();
                let target = target.clone();
                cx.spawn(async move |cx| {
                    if answer.await != Ok(0) {
                        return;
                    }
                    let _ = app_entity.update(cx, |app, cx| {
                        let result = target
                            .map_err(|error| error.to_string())
                            .and_then(|target| {
                                if tree {
                                    terminate_process_tree(&target, &app.running_processes)
                                        .map(|_| ())
                                } else {
                                    terminate_process(&target)
                                }
                            });
                        app.finish_process_quick_action(
                            &process_name,
                            t!(label_key).as_ref(),
                            result,
                            cx,
                        );
                    });
                })
                .detach();
            }),
        );
    }

    if show_suspend {
        let app_entity = app_entity.clone();
        let process_name = process_name.clone();
        let target = target.clone();
        let suspend = !suspended;
        let label_key = if suspend {
            "process_list.suspend_process"
        } else {
            "process_list.resume_process"
        };
        menu = menu.item(
            process_list_value_menu_item(
                t!(label_key).to_string(),
                if suspend {
                    ProcessListMenuItemTone::Warning
                } else {
                    ProcessListMenuItemTone::Default
                },
                suspension_disabled,
            )
            .disabled(suspension_disabled)
            .on_click(move |_, _, cx| {
                app_entity.update(cx, |app, cx| {
                    let result = target
                        .clone()
                        .map_err(|error| error.to_string())
                        .map(|target| {
                            app.background_automation
                                .request_app_suspension_process_action(target, suspend);
                        });
                    app.status_message = match result {
                        Ok(()) => t!(
                            "process_list.quick_action_queued",
                            action = t!(label_key).as_ref(),
                            name = process_name.as_str()
                        )
                        .to_string(),
                        Err(error) => t!(
                            "process_list.quick_action_failed",
                            action = t!(label_key).as_ref(),
                            name = process_name.as_str(),
                            error = error
                        )
                        .to_string(),
                    };
                    cx.notify();
                });
            }),
        );
    }

    let queried_efficiency = target
        .as_ref()
        .ok()
        .and_then(|target| background_efficiency::current_efficiency_mode(target).ok());
    let cached_efficiency = target.as_ref().ok().and_then(|target| {
        app_entity
            .read(menu_cx)
            .process_efficiency_mode_overrides
            .get(&target.id)
            .filter(|(creation_time, _)| *creation_time == target.creation_time)
            .map(|(_, enabled)| *enabled)
    });
    let efficiency_enabled = queried_efficiency.or(cached_efficiency).unwrap_or(false);
    menu = menu.item({
        let app_entity = app_entity.clone();
        let process_name = process_name.clone();
        let target = target.clone();
        process_list_value_menu_item(
            t!("process_list.efficiency_mode").to_string(),
            if efficiency_enabled {
                ProcessListMenuItemTone::Success
            } else {
                ProcessListMenuItemTone::Default
            },
            efficiency_disabled,
        )
        .disabled(efficiency_disabled)
        .on_click(move |_, _, cx| {
            app_entity.update(cx, |app, cx| {
                let enabled = !efficiency_enabled;
                let result = target
                    .clone()
                    .map_err(|error| error.to_string())
                    .and_then(|target| {
                        let result =
                            background_efficiency::apply_efficiency_mode_once(&target, enabled);
                        if result.is_ok() {
                            app.process_efficiency_mode_overrides
                                .insert(target.id, (target.creation_time, enabled));
                        }
                        result
                    });
                app.finish_process_quick_action(
                    &process_name,
                    t!("process_list.efficiency_mode").as_ref(),
                    result,
                    cx,
                );
            });
        })
    });

    menu = process_list_priority_controls_submenu(
        menu,
        app_entity.clone(),
        process_name.clone(),
        target.clone(),
        expose_all_priorities,
        window,
        menu_cx,
    );
    menu = menu.separator();
    menu.item(process_list_open_location_menu_item(
        app_entity,
        process_name,
        executable_path,
    ))
}

fn process_list_priority_controls_submenu(
    menu: PopupMenu,
    app_entity: Entity<WinderustApp>,
    process_name: String,
    target: Result<ProcessActionTarget, ProcessActionTargetError>,
    expose_all_priorities: bool,
    window: &mut Window,
    menu_cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    if target.as_ref().map_or(true, |target| {
        ensure_process_action_target_mutable(target).is_err()
    }) {
        return menu.item(
            process_list_value_menu_item(
                t!("process_list.priority_controls").to_string(),
                ProcessListMenuItemTone::Default,
                true,
            )
            .disabled(true),
        );
    }
    menu.submenu(
        t!("process_list.priority_controls").to_string(),
        window,
        menu_cx,
        move |menu, window, menu_cx| {
            let current_process = target
                .as_ref()
                .ok()
                .and_then(|target| process_priority::current_priority(target).ok());
            let process_priority_available = current_process.is_some()
                && current_process != Some(ProcessPrioritySetting::Realtime);
            let process_options = if expose_all_priorities {
                &ProcessPrioritySetting::ADVANCED_ALL[..]
            } else {
                &ProcessPrioritySetting::ALL[..]
            }
            .iter()
            .copied()
            .filter(|priority| *priority != ProcessPrioritySetting::Default)
            .collect();
            let menu = process_list_priority_value_submenu(
                menu,
                t!("process_list.process_priority").to_string(),
                process_options,
                current_process,
                process_priority_available,
                process_priority::can_apply_once,
                app_entity.clone(),
                process_name.clone(),
                target.clone(),
                process_priority_setting_label,
                quick_apply_process_priority,
                window,
                menu_cx,
            );

            let current_thread = target
                .as_ref()
                .ok()
                .and_then(|target| thread_priority::current_priority(target).ok());
            let thread_priority_available = current_thread.is_some();
            let current_thread = current_thread.flatten();
            let thread_options = if expose_all_priorities {
                &ProcessThreadPrioritySetting::ADVANCED_ALL[..]
            } else {
                &ProcessThreadPrioritySetting::ALL[..]
            }
            .iter()
            .copied()
            .filter(|priority| *priority != ProcessThreadPrioritySetting::Default)
            .collect();
            let menu = process_list_priority_value_submenu(
                menu,
                t!("process_list.thread_priority").to_string(),
                thread_options,
                current_thread,
                thread_priority_available,
                process_list_priority_option_available,
                app_entity.clone(),
                process_name.clone(),
                target.clone(),
                process_thread_priority_setting_label,
                quick_apply_thread_priority,
                window,
                menu_cx,
            );

            let current_boost = target
                .as_ref()
                .ok()
                .and_then(|target| dynamic_priority_boost::current_boost_disabled(target).ok());
            let menu = process_list_priority_value_submenu(
                menu,
                t!("process_list.dynamic_priority_boost").to_string(),
                vec![false, true],
                current_boost,
                current_boost.is_some(),
                process_list_priority_option_available,
                app_entity.clone(),
                process_name.clone(),
                target.clone(),
                dynamic_boost_quick_label,
                dynamic_priority_boost::apply_once,
                window,
                menu_cx,
            );

            let current_io = target
                .as_ref()
                .ok()
                .and_then(|target| io_priority::current_priority(target).ok());
            let io_options = if expose_all_priorities {
                &ProcessIoPrioritySetting::ADVANCED_ALL[..]
            } else {
                &ProcessIoPrioritySetting::ALL[..]
            }
            .iter()
            .filter_map(|priority| priority.priority())
            .collect();
            let menu = process_list_priority_value_submenu(
                menu,
                t!("process_list.io_priority").to_string(),
                io_options,
                current_io,
                current_io.is_some(),
                process_list_priority_option_available,
                app_entity.clone(),
                process_name.clone(),
                target.clone(),
                io_priority_quick_label,
                io_priority::apply_once,
                window,
                menu_cx,
            );

            let current_gpu = target
                .as_ref()
                .ok()
                .and_then(|target| gpu_priority::current_priority(target).ok());
            let gpu_options = if expose_all_priorities {
                &ProcessGpuPrioritySetting::ADVANCED_ALL[..]
            } else {
                &ProcessGpuPrioritySetting::ALL[..]
            }
            .iter()
            .filter_map(|priority| priority.priority())
            .collect();
            let menu = process_list_priority_value_submenu(
                menu,
                t!("process_list.gpu_priority").to_string(),
                gpu_options,
                current_gpu,
                current_gpu.is_some(),
                process_list_priority_option_available,
                app_entity.clone(),
                process_name.clone(),
                target.clone(),
                gpu_priority_quick_label,
                gpu_priority::apply_once,
                window,
                menu_cx,
            );

            let current_memory = target
                .as_ref()
                .ok()
                .and_then(|target| memory_priority::current_priority(target).ok());
            let memory_options = ProcessMemoryPrioritySetting::ALL
                .into_iter()
                .filter(|priority| *priority != ProcessMemoryPrioritySetting::Default)
                .collect();
            process_list_priority_value_submenu(
                menu,
                t!("process_list.memory_priority").to_string(),
                memory_options,
                current_memory,
                current_memory.is_some(),
                process_list_priority_option_available,
                app_entity.clone(),
                process_name.clone(),
                target.clone(),
                process_memory_priority_setting_label,
                quick_apply_memory_priority,
                window,
                menu_cx,
            )
        },
    )
}

fn process_list_rule_details_menu_item(
    app_entity: Entity<WinderustApp>,
    process_id: Option<u32>,
    process_name: String,
    executable_path: String,
) -> PopupMenuItem {
    PopupMenuItem::new(t!("process_list.open_rule_details").to_string()).on_click(
        move |_, _, cx| {
            app_entity.update(cx, |app, cx| {
                app.selected_process_id = process_id;
                app.open_process_details(process_name.clone(), executable_path.clone(), cx);
            });
        },
    )
}

fn process_list_open_location_menu_item(
    app_entity: Entity<WinderustApp>,
    process_name: String,
    executable_path: String,
) -> PopupMenuItem {
    process_list_value_menu_item(
        t!("process_list.open_process_location").to_string(),
        ProcessListMenuItemTone::Default,
        false,
    )
    .on_click(move |_, _, cx| {
        app_entity.update(cx, |app, cx| {
            let result = open_process_location(Path::new(&executable_path));
            app.finish_process_quick_action(
                &process_name,
                t!("process_list.open_process_location").as_ref(),
                result,
                cx,
            );
        });
    })
}

#[expect(
    clippy::too_many_arguments,
    reason = "submenu items need live process action context"
)]
fn process_list_priority_value_submenu<T: Copy + PartialEq + 'static>(
    menu: PopupMenu,
    title: String,
    options: Vec<T>,
    current: Option<T>,
    available: bool,
    option_available: fn(T) -> bool,
    app_entity: Entity<WinderustApp>,
    process_name: String,
    target: Result<ProcessActionTarget, ProcessActionTargetError>,
    label: fn(T) -> String,
    apply: fn(&ProcessActionTarget, T) -> Result<(), String>,
    window: &mut Window,
    menu_cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    menu.submenu(title.clone(), window, menu_cx, move |mut menu, _, _| {
        for option in &options {
            let option = *option;
            let disabled = !available || !option_available(option);
            let app_entity = app_entity.clone();
            let process_name = process_name.clone();
            let target = target.clone();
            let action = format!("{}: {}", title, label(option));
            menu = menu.item(
                process_list_value_menu_item(
                    label(option),
                    if current == Some(option) {
                        ProcessListMenuItemTone::Success
                    } else {
                        ProcessListMenuItemTone::Muted
                    },
                    disabled,
                )
                .disabled(disabled)
                .on_click(move |_, _, cx| {
                    app_entity.update(cx, |app, cx| {
                        let result = target
                            .clone()
                            .map_err(|error| error.to_string())
                            .and_then(|target| apply(&target, option));
                        app.finish_process_quick_action(&process_name, &action, result, cx);
                    });
                }),
            );
        }
        menu
    })
}

fn process_list_priority_option_available<T>(_: T) -> bool {
    true
}

#[derive(Clone, Copy)]
enum ProcessListMenuItemTone {
    Default,
    Muted,
    Success,
    Warning,
    Danger,
}

fn process_list_value_menu_item(
    label: String,
    tone: ProcessListMenuItemTone,
    disabled: bool,
) -> PopupMenuItem {
    PopupMenuItem::element(move |_, cx| {
        div()
            .flex_1()
            .when(disabled, |item| item.opacity(0.48))
            .text_color(if disabled {
                cx.theme().muted_foreground
            } else {
                match tone {
                    ProcessListMenuItemTone::Default => cx.theme().popover_foreground,
                    ProcessListMenuItemTone::Muted => cx.theme().muted_foreground,
                    ProcessListMenuItemTone::Success => cx.theme().success_foreground,
                    ProcessListMenuItemTone::Warning => rgb(warning_text_color()).into(),
                    ProcessListMenuItemTone::Danger => cx.theme().danger_foreground,
                }
            })
            .child(label.clone())
    })
}

fn quick_apply_process_priority(
    target: &ProcessActionTarget,
    priority: ProcessPrioritySetting,
) -> Result<(), String> {
    process_priority::apply_once(target, priority).map(|_| ())
}

fn quick_apply_thread_priority(
    target: &ProcessActionTarget,
    priority: ProcessThreadPrioritySetting,
) -> Result<(), String> {
    thread_priority::apply_once(target, priority).map(|_| ())
}

fn quick_apply_memory_priority(
    target: &ProcessActionTarget,
    priority: ProcessMemoryPrioritySetting,
) -> Result<(), String> {
    memory_priority::apply_once(target, priority).map(|_| ())
}

fn dynamic_boost_quick_label(disabled: bool) -> String {
    if disabled {
        t!("common.disabled").to_string()
    } else {
        t!("common.enabled").to_string()
    }
}

fn io_priority_quick_label(priority: ProcessIoPriority) -> String {
    io_priority::io_priority_label(priority).to_owned()
}

fn gpu_priority_quick_label(priority: ProcessGpuPriority) -> String {
    gpu_priority::gpu_priority_label(priority).to_owned()
}

pub(in crate::ui::app) fn process_list_entry_row(
    process: &ProcessInfo,
    summary: &ProcessPolicySummary,
    icon: Option<&Arc<Image>>,
    state: ProcessListEntryRowState,
    layout: ProcessListRenderLayout<'_>,
    edit_context: ProcessListEditContext<'_>,
    cx: &mut Context<WinderustApp>,
) -> AnyElement {
    let row_id = SharedString::from(format!("process-list-entry-{}", process.id));
    let process_id = process.id;
    let process_name = process.name.clone();
    let executable_path = process
        .image_path
        .as_deref()
        .map(|path| path.to_string_lossy().into_owned());
    let details_name = process_name.clone();
    let details_path = executable_path.clone();
    let selected = edit_context.app.selected_process_id == Some(process_id);
    let app_entity = cx.entity();
    let limited_access = executable_path.is_none();
    let suspended = edit_context
        .app
        .app_suspension_status
        .suspended_process_ids
        .contains(&process_id);

    let expose_all_priorities = edit_context
        .app
        .settings
        .advanced
        .expose_all_priority_values;
    let mut row = h_flex()
        .id(row_id)
        .w_full()
        .min_w(px(0.0))
        .h(px(PROCESS_LIST_ROW_HEIGHT))
        .items_center()
        .gap_3()
        .px_4()
        .py_2()
        .text_size(px(TEXT_BODY_SIZE))
        .line_height(px(TEXT_BODY_LINE_HEIGHT))
        .when(state.divided, |row| {
            row.border_t_1().border_color(rgb(border_color()))
        })
        .when(selected, |row| row.bg(rgb(panel_active_color())))
        .when(limited_access, |row| {
            row.opacity(0.65).tooltip(|window, cx| {
                Tooltip::new(t!("process_list.limited_access_help").to_string()).build(window, cx)
            })
        })
        .when(!limited_access, |row| {
            row.hover(|style| style.bg(rgb(settings_card_hover_color())))
                .cursor_pointer()
                .on_click(cx.listener(move |app, _, _, cx| {
                    app.selected_process_id = Some(process_id);
                    let Some(executable_path) = details_path.clone() else {
                        return;
                    };
                    app.open_process_details(details_name.clone(), executable_path, cx);
                }))
        })
        .context_menu(move |menu, window, menu_cx| {
            if executable_path.is_none() {
                return menu;
            }
            process_list_context_menu(
                menu,
                app_entity.clone(),
                process_id,
                process_name.clone(),
                executable_path.clone().unwrap_or_default(),
                suspended,
                false,
                expose_all_priorities,
                state.nested,
                window,
                menu_cx,
            )
        })
        .child(process_list_name_cell(
            process.name.clone(),
            icon,
            state.nested,
            layout.column_layout.name_width,
            cx,
        ));

    row = row.child(process_list_text_cell(
        layout.column_layout.column_width(ProcessListColumn::Pid),
        process.id.to_string(),
    ));

    let executable_path = process
        .image_path
        .as_deref()
        .map(|path| path.to_string_lossy());
    let mut process_summary = summary.clone();
    if executable_path.is_none() {
        process_summary.status = t!("process_list.status_limited_access").to_string();
    } else if let Some(usage) = edit_context.app.process_resource_usage.get(&process.id) {
        process_summary.cpu_percent = usage.cpu_percent;
        process_summary.memory_bytes = usage.working_set_bytes;
        process_summary.status = process_list_status_label(
            &edit_context.app.app_suspension_status,
            Some(process.id),
            executable_path.as_deref().unwrap_or_default(),
            usage.efficiency_mode == Some(true),
        );
    } else {
        process_summary.status = process_list_status_label(
            &edit_context.app.app_suspension_status,
            Some(process.id),
            executable_path.as_deref().unwrap_or_default(),
            false,
        );
    }
    row.children(process_list_policy_cells(
        executable_path.as_deref().unwrap_or_default(),
        &process_summary,
        layout,
        state.editable && executable_path.is_some(),
        edit_context,
        cx,
    ))
    .into_any_element()
}

pub(in crate::ui::app) fn process_list_group_row(
    data: ProcessListGroupRowData<'_>,
    summary: &ProcessPolicySummary,
    icon: Option<&Arc<Image>>,
    state: ProcessListGroupRowState,
    layout: ProcessListRenderLayout<'_>,
    edit_context: ProcessListEditContext<'_>,
    cx: &mut Context<WinderustApp>,
) -> AnyElement {
    let process_name = data.process_name.to_string();
    let executable_path = data.executable_path.to_string();
    let row_id = SharedString::from(format!(
        "process-list-group-{}",
        process_list_executable_path_group_key(Path::new(&executable_path))
    ));
    let details_name = process_name.clone();
    let details_path = executable_path.clone();
    let menu_process_name = process_name.clone();
    let menu_executable_path = executable_path.clone();
    let app_entity = cx.entity();
    let suspended = edit_context
        .app
        .app_suspension_status
        .suspended_process_ids
        .contains(&data.process_id);
    let expose_all_priorities = edit_context
        .app
        .settings
        .advanced
        .expose_all_priority_values;

    let mut row = h_flex()
        .id(row_id)
        .w_full()
        .min_w(px(0.0))
        .h(px(PROCESS_LIST_ROW_HEIGHT))
        .items_center()
        .gap_3()
        .px_4()
        .py_2()
        .text_size(px(TEXT_BODY_SIZE))
        .line_height(px(TEXT_BODY_LINE_HEIGHT))
        .when(state.divided, |row| {
            row.border_t_1().border_color(rgb(border_color()))
        })
        .hover(|style| style.bg(rgb(settings_card_hover_color())))
        .cursor_pointer()
        .on_click(cx.listener(move |app, _, _, cx| {
            app.open_process_details(details_name.clone(), details_path.clone(), cx);
        }))
        .context_menu(move |menu, window, menu_cx| {
            process_list_context_menu(
                menu,
                app_entity.clone(),
                data.process_id,
                menu_process_name.clone(),
                menu_executable_path.clone(),
                suspended,
                true,
                expose_all_priorities,
                true,
                window,
                menu_cx,
            )
        })
        .child(process_list_group_name_cell(
            &process_name,
            &executable_path,
            data.process_count,
            icon,
            state.collapsed,
            layout.column_layout.name_width,
            cx,
        ));

    row = row.child(process_list_text_cell(
        layout.column_layout.column_width(ProcessListColumn::Pid),
        process_list_pid_count_label(data.process_count),
    ));

    row.children(process_list_policy_cells(
        &executable_path,
        summary,
        layout,
        true,
        edit_context,
        cx,
    ))
    .into_any_element()
}

pub(in crate::ui::app) fn process_list_name_cell(
    name: impl Into<SharedString>,
    icon: Option<&Arc<Image>>,
    nested: bool,
    width: f32,
    cx: &mut Context<WinderustApp>,
) -> gpui::Div {
    h_flex()
        .w(px(width))
        .min_w(px(0.0))
        .flex_shrink_0()
        .items_center()
        .gap_2()
        .when(nested, |cell| cell.pl_4())
        .child(div().w(px(PROCESS_LIST_TREE_TOGGLE_WIDTH)).flex_shrink_0())
        .child(process_icon_cell(icon, cx))
        .child(div().flex_1().min_w(px(0.0)).truncate().child(name.into()))
}

pub(in crate::ui::app) fn process_list_group_name_cell(
    process_name: &str,
    executable_path: &str,
    process_count: usize,
    icon: Option<&Arc<Image>>,
    collapsed: bool,
    width: f32,
    cx: &mut Context<WinderustApp>,
) -> gpui::Div {
    let toggle_name = executable_path.to_owned();
    h_flex()
        .w(px(width))
        .min_w(px(0.0))
        .flex_shrink_0()
        .items_center()
        .gap_2()
        .child(
            div()
                .w(px(PROCESS_LIST_TREE_TOGGLE_WIDTH))
                .flex_shrink_0()
                .id(SharedString::from(format!(
                    "process-list-group-toggle-{}",
                    process_list_executable_path_group_key(Path::new(executable_path))
                )))
                .cursor_pointer()
                .on_click(cx.listener(move |app, _, _, cx| {
                    app.toggle_process_list_group(toggle_name.clone(), cx);
                    cx.stop_propagation();
                }))
                .child(collapsible_chevron_icon(
                    format!(
                        "process-list-group-{}",
                        process_list_executable_path_group_key(Path::new(executable_path))
                    ),
                    collapsed,
                )),
        )
        .child(process_icon_cell(icon, cx))
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .truncate()
                .child(process_name.to_string()),
        )
        .child(
            text_muted(format!("x{process_count}"))
                .flex_shrink_0()
                .text_size(px(TEXT_LABEL_SIZE)),
        )
}

pub(in crate::ui::app) fn process_list_policy_cells(
    process_name: &str,
    summary: &ProcessPolicySummary,
    layout: ProcessListRenderLayout<'_>,
    editable: bool,
    edit_context: ProcessListEditContext<'_>,
    cx: &mut Context<WinderustApp>,
) -> Vec<AnyElement> {
    PROCESS_LIST_OVERVIEW_COLUMNS
        .iter()
        .copied()
        .filter(|column| *column != ProcessListColumn::Pid)
        .map(|column| {
            process_list_policy_cell(
                layout.column_layout.column_width(column),
                ProcessListPolicyCellTarget {
                    process_name,
                    column,
                    editable,
                    active: summary.value_is_active(column),
                },
                process_list_column_value(summary, column),
                summary.uses_custom_rule(column),
                edit_context,
                cx,
            )
        })
        .collect()
}

pub(in crate::ui::app) fn process_list_column_value(
    summary: &ProcessPolicySummary,
    column: ProcessListColumn,
) -> SharedString {
    match column {
        ProcessListColumn::Pid => SharedString::new_static(""),
        ProcessListColumn::Status => summary.status.clone().into(),
        ProcessListColumn::CpuUsage
            if summary.status == t!("process_list.status_limited_access") =>
        {
            SharedString::new_static("\u{2014}")
        }
        ProcessListColumn::CpuUsage => summary
            .cpu_percent
            .map(|percent| format!("{percent:.1}%"))
            .unwrap_or_else(|| t!("common.unknown").to_string())
            .into(),
        ProcessListColumn::MemoryUsage
            if summary.status == t!("process_list.status_limited_access") =>
        {
            SharedString::new_static("\u{2014}")
        }
        ProcessListColumn::MemoryUsage => summary
            .memory_bytes
            .map(format_memory_capacity)
            .unwrap_or_else(|| t!("common.unknown").to_string())
            .into(),
        ProcessListColumn::PowerPlanForeground => summary.power_plan_foreground.clone().into(),
        ProcessListColumn::PowerPlanRunning => summary.power_plan_running.clone().into(),
        ProcessListColumn::AdaptiveEngine => summary.adaptive_engine.clone().into(),
        ProcessListColumn::BackgroundEfficiency => summary.background_efficiency.clone().into(),
        ProcessListColumn::ProcessPriority => summary.process_priority.clone().into(),
        ProcessListColumn::ThreadPriority => summary.thread_priority.clone().into(),
        ProcessListColumn::DynamicPriorityBoost => summary.dynamic_priority_boost.clone().into(),
        ProcessListColumn::IoPriority => summary.io_priority.clone().into(),
        ProcessListColumn::GpuPriority => summary.gpu_priority.clone().into(),
        ProcessListColumn::MemoryPriority => summary.memory_priority.clone().into(),
    }
}

pub(in crate::ui::app) fn process_list_text_cell(
    width: f32,
    value: impl Into<SharedString>,
) -> gpui::Div {
    process_list_text_cell_with_color(width, value, false, muted_text_color())
}

pub(in crate::ui::app) fn process_list_text_cell_with_color(
    width: f32,
    value: impl Into<SharedString>,
    emphasized: bool,
    text_color: u32,
) -> gpui::Div {
    let value = value.into();
    h_flex()
        .w(px(width))
        .min_w(px(0.0))
        .flex_shrink_0()
        .text_color(rgb(text_color))
        .child(process_list_policy_value_content(
            None, value, emphasized, text_color,
        ))
}

pub(in crate::ui::app) fn process_list_policy_value_content(
    column: Option<ProcessListColumn>,
    value: SharedString,
    emphasized: bool,
    text_color: u32,
) -> AnyElement {
    if column.is_some_and(process_list_column_uses_split_priority_display) {
        if let Some((foreground, background)) = process_list_split_policy_value(value.as_ref()) {
            return v_flex()
                .flex_1()
                .min_w(px(0.0))
                .gap(px(1.0))
                .child(process_list_split_policy_value_row(
                    process_list_foreground_short_label(),
                    foreground,
                    emphasized,
                    text_color,
                ))
                .child(process_list_split_policy_value_row(
                    process_list_background_short_label(),
                    background,
                    emphasized,
                    text_color,
                ))
                .into_any_element();
        }
    }

    div()
        .flex_1()
        .min_w(px(0.0))
        .truncate()
        .text_color(rgb(text_color))
        .when(emphasized, |cell| cell.font_weight(gpui::FontWeight::BOLD))
        .child(value)
        .into_any_element()
}

pub(in crate::ui::app) fn process_list_split_policy_value_row(
    label: &'static str,
    value: &str,
    emphasized: bool,
    text_color: u32,
) -> gpui::Div {
    h_flex()
        .w_full()
        .min_w(px(0.0))
        .gap(px(PROCESS_LIST_SPLIT_LABEL_GAP))
        .text_size(px(TEXT_LABEL_SIZE))
        .line_height(px(TEXT_LABEL_LINE_HEIGHT))
        .child(
            div()
                .w(px(PROCESS_LIST_SPLIT_LABEL_WIDTH))
                .flex_shrink_0()
                .text_color(rgb(dim_text_color()))
                .child(SharedString::new_static(label)),
        )
        .child(
            div()
                .flex_1()
                .min_w(px(0.0))
                .truncate()
                .text_color(rgb(text_color))
                .when(emphasized, |cell| cell.font_weight(gpui::FontWeight::BOLD))
                .child(value.to_owned()),
        )
}

pub(in crate::ui::app) fn process_list_column_uses_split_priority_display(
    column: ProcessListColumn,
) -> bool {
    matches!(
        column,
        ProcessListColumn::IoPriority
            | ProcessListColumn::GpuPriority
            | ProcessListColumn::MemoryPriority
    )
}

pub(in crate::ui::app) fn process_list_split_policy_value(value: &str) -> Option<(&str, &str)> {
    let (foreground, background) = value.split_once(" / ")?;
    Some((foreground.trim(), background.trim()))
}

pub(in crate::ui::app) fn process_list_policy_cell(
    width: f32,
    target: ProcessListPolicyCellTarget<'_>,
    value: impl Into<SharedString>,
    emphasized: bool,
    edit_context: ProcessListEditContext<'_>,
    cx: &mut Context<WinderustApp>,
) -> AnyElement {
    let value = value.into();
    if target.column == ProcessListColumn::Status {
        let limited_access = value == t!("process_list.status_limited_access").to_string();
        let suspended = value == t!("process_list.status_suspended").to_string();
        let efficiency_mode = value == t!("process_list.status_efficiency_mode").to_string();
        let (icon, color) = if limited_access {
            (NavIcon::OctagonMinus, muted_text_color())
        } else if suspended {
            (NavIcon::CirclePause, warning_text_color())
        } else if efficiency_mode {
            (NavIcon::Leaf, success_text_color())
        } else {
            (
                NavIcon::Play,
                if ui_is_dark() { 0x78b7ff } else { 0x1f5fae },
            )
        };
        return h_flex()
            .w(px(width))
            .min_w(px(0.0))
            .flex_shrink_0()
            .gap_1()
            .text_color(rgb(color))
            .child(Icon::new(icon).with_size(px(14.0)))
            .child(value)
            .into_any_element();
    }
    if matches!(
        target.column,
        ProcessListColumn::CpuUsage | ProcessListColumn::MemoryUsage
    ) {
        return h_flex()
            .w(px(width))
            .min_w(px(0.0))
            .flex_shrink_0()
            .justify_end()
            .pr_2()
            .text_color(rgb(primary_text_color()))
            .child(value)
            .into_any_element();
    }
    let text_color = if target.active {
        success_text_color()
    } else {
        dim_text_color()
    };

    if !process_list_policy_cell_editable(target.editable, target.column) {
        return h_flex()
            .w(px(width))
            .min_w(px(0.0))
            .flex_shrink_0()
            .text_color(rgb(text_color))
            .child(process_list_policy_value_content(
                Some(target.column),
                value,
                emphasized,
                text_color,
            ))
            .into_any_element();
    }

    process_list_editable_policy_cell(
        width,
        target.process_name,
        target.column,
        value,
        emphasized,
        text_color,
        edit_context.app,
        edit_context.window,
        cx,
    )
}

#[expect(
    clippy::too_many_arguments,
    reason = "cell rendering needs table, row, and dropdown context"
)]
pub(in crate::ui::app) fn process_list_editable_policy_cell(
    width: f32,
    process_name: &str,
    column: ProcessListColumn,
    value: SharedString,
    emphasized: bool,
    text_color: u32,
    app: &WinderustApp,
    window: &Window,
    cx: &mut Context<WinderustApp>,
) -> AnyElement {
    let id = process_list_cell_editor_id(process_name, column);
    let is_open = app.active_power_plan_picker.as_deref() == Some(id.as_str());
    let toggle_id = id.clone();
    let popup_id = id.clone();

    let cell = v_flex()
        .id(SharedString::from(format!("{id}-cell")))
        .w(px(width))
        .min_w(px(0.0))
        .flex_shrink_0()
        .relative()
        .child(
            div()
                .id(SharedString::from(format!("{id}-trigger")))
                .w_full()
                .min_w(px(0.0))
                .flex()
                .items_center()
                .text_color(rgb(text_color))
                .when(emphasized, |cell| cell.font_weight(gpui::FontWeight::BOLD))
                .hover(|style| style.bg(rgb(settings_card_hover_color())))
                .cursor_pointer()
                .on_click(cx.listener(move |app, _, _, cx| {
                    app.active_power_plan_picker = (app.active_power_plan_picker.as_deref()
                        != Some(toggle_id.as_str()))
                    .then_some(toggle_id.clone());
                    cx.stop_propagation();
                    cx.notify();
                }))
                .child(process_list_policy_value_content(
                    Some(column),
                    value,
                    emphasized,
                    text_color,
                )),
        );
    let cell = if is_open {
        cell.child(dropdown_anchor_sensor(
            id.clone(),
            Rc::clone(&app.dropdown_anchor_bounds),
        ))
    } else {
        cell
    };

    cell.child(dropdown_popup_or_empty_lazy(
        is_open,
        SharedString::from(id),
        || {
            let option_count = process_list_cell_editor_option_count(column, app);
            app.dropdown_placement(&popup_id, dropdown_list_height(option_count), window)
        },
        |max_height, cx| {
            process_list_cell_editor_options(process_name, column, app, max_height, cx)
        },
        cx,
    ))
    .into_any_element()
}

pub(in crate::ui::app) fn process_list_column_editable(column: ProcessListColumn) -> bool {
    matches!(
        column,
        ProcessListColumn::PowerPlanForeground
            | ProcessListColumn::PowerPlanRunning
            | ProcessListColumn::AdaptiveEngine
            | ProcessListColumn::BackgroundEfficiency
            | ProcessListColumn::ProcessPriority
            | ProcessListColumn::ThreadPriority
            | ProcessListColumn::DynamicPriorityBoost
            | ProcessListColumn::IoPriority
            | ProcessListColumn::GpuPriority
            | ProcessListColumn::MemoryPriority
    )
}

pub(in crate::ui::app) fn process_list_policy_cell_editable(
    row_editable: bool,
    column: ProcessListColumn,
) -> bool {
    row_editable && process_list_column_editable(column)
}

pub(in crate::ui::app) fn process_list_cell_editor_id(
    process_name: &str,
    column: ProcessListColumn,
) -> String {
    format!(
        "process-list-cell-editor-{}-{}",
        process_list_executable_path_group_key(Path::new(process_name)),
        process_list_sort_column_id(ProcessListSortColumn::Column(column))
    )
}

pub(in crate::ui::app) fn process_list_cell_editor_option_count(
    column: ProcessListColumn,
    app: &WinderustApp,
) -> usize {
    match column {
        ProcessListColumn::PowerPlanForeground | ProcessListColumn::PowerPlanRunning => {
            1 + app.plans.len()
        }
        ProcessListColumn::BackgroundEfficiency | ProcessListColumn::AdaptiveEngine => 2,
        ProcessListColumn::ProcessPriority => {
            if app.settings.advanced.expose_all_priority_values {
                ProcessPrioritySetting::CUSTOM_RULE_ADVANCED_ALL.len()
            } else {
                ProcessPrioritySetting::CUSTOM_RULE_ALL.len()
            }
        }
        ProcessListColumn::ThreadPriority => {
            if app.settings.advanced.expose_all_priority_values {
                ProcessThreadPrioritySetting::CUSTOM_RULE_ADVANCED_ALL.len()
            } else {
                ProcessThreadPrioritySetting::CUSTOM_RULE_ALL.len()
            }
        }
        ProcessListColumn::DynamicPriorityBoost => {
            ProcessDynamicPriorityBoostSetting::CUSTOM_RULE_ALL.len()
        }
        ProcessListColumn::IoPriority => {
            if app.settings.advanced.expose_all_priority_values {
                ProcessIoPrioritySetting::CUSTOM_RULE_ADVANCED_ALL.len()
            } else {
                ProcessIoPrioritySetting::CUSTOM_RULE_ALL.len()
            }
        }
        ProcessListColumn::GpuPriority => {
            if app.settings.advanced.expose_all_priority_values {
                ProcessGpuPrioritySetting::CUSTOM_RULE_ADVANCED_ALL.len()
            } else {
                ProcessGpuPrioritySetting::CUSTOM_RULE_ALL.len()
            }
        }
        ProcessListColumn::MemoryPriority => ProcessMemoryPrioritySetting::CUSTOM_RULE_ALL.len(),
        ProcessListColumn::Pid
        | ProcessListColumn::Status
        | ProcessListColumn::CpuUsage
        | ProcessListColumn::MemoryUsage => 0,
    }
}

pub(in crate::ui::app) fn process_list_cell_editor_options(
    process_name: &str,
    column: ProcessListColumn,
    app: &WinderustApp,
    max_height: Pixels,
    cx: &mut Context<WinderustApp>,
) -> Scrollable<gpui::Div> {
    let settings = &app.settings;
    let mut options = dropdown_surface(cx, max_height)
        .w(px(PROCESS_LIST_CELL_EDITOR_WIDTH))
        .min_w(px(PROCESS_LIST_CELL_EDITOR_WIDTH));
    let process_name = process_name.to_owned();

    match column {
        ProcessListColumn::PowerPlanForeground => {
            let selected_guid =
                foreground_power_plan_override_guid(&settings.by_foreground, &process_name);
            options = options.child(process_list_power_plan_editor_option(
                &process_name,
                column,
                process_list_default_label(),
                selected_guid.is_none(),
                None,
                cx,
            ));
            for plan in &app.plans {
                let guid = plan.guid.clone();
                let selected = selected_guid
                    .as_deref()
                    .is_some_and(|selected| selected.eq_ignore_ascii_case(&guid));
                options = options.child(process_list_power_plan_editor_option(
                    &process_name,
                    column,
                    plan.name.clone(),
                    selected,
                    Some(guid),
                    cx,
                ));
            }
        }
        ProcessListColumn::PowerPlanRunning => {
            let selected_guid =
                by_running_app_power_plan_override_guid(&settings.by_running_app, &process_name);
            options = options.child(process_list_power_plan_editor_option(
                &process_name,
                column,
                process_list_default_label(),
                selected_guid.is_none(),
                None,
                cx,
            ));
            for plan in &app.plans {
                let guid = plan.guid.clone();
                let selected = selected_guid
                    .as_deref()
                    .is_some_and(|selected| selected.eq_ignore_ascii_case(&guid));
                options = options.child(process_list_power_plan_editor_option(
                    &process_name,
                    column,
                    plan.name.clone(),
                    selected,
                    Some(guid),
                    cx,
                ));
            }
        }
        ProcessListColumn::BackgroundEfficiency => {
            let included = !app
                .settings
                .background_efficiency
                .custom_rule_enabled_for(&process_name);
            options = process_list_include_exclude_editor_options(
                options,
                &process_name,
                column,
                included,
                cx,
            );
        }
        ProcessListColumn::AdaptiveEngine => {
            let included = !settings
                .workload_engine
                .workload_engine_exclusion_enabled_for(&process_name);
            options = process_list_include_exclude_editor_options(
                options,
                &process_name,
                column,
                included,
                cx,
            );
        }
        ProcessListColumn::ProcessPriority => {
            let selected = settings
                .process_priority
                .override_for(&process_name, true)
                .flatten()
                .unwrap_or_default();
            let values = if app.settings.advanced.expose_all_priority_values {
                &ProcessPrioritySetting::CUSTOM_RULE_ADVANCED_ALL[..]
            } else {
                &ProcessPrioritySetting::CUSTOM_RULE_ALL[..]
            };
            options = process_list_priority_editor_options(
                options,
                &process_name,
                column,
                selected,
                values,
                process_priority_setting_label,
                WinderustApp::set_process_list_process_priority,
                cx,
            );
        }
        ProcessListColumn::ThreadPriority => {
            let selected = settings
                .thread_priority
                .override_for(&process_name, true)
                .flatten()
                .unwrap_or_default();
            let values = if app.settings.advanced.expose_all_priority_values {
                &ProcessThreadPrioritySetting::CUSTOM_RULE_ADVANCED_ALL[..]
            } else {
                &ProcessThreadPrioritySetting::CUSTOM_RULE_ALL[..]
            };
            options = process_list_priority_editor_options(
                options,
                &process_name,
                column,
                selected,
                values,
                process_thread_priority_setting_label,
                WinderustApp::set_process_list_thread_priority,
                cx,
            );
        }
        ProcessListColumn::DynamicPriorityBoost => {
            let selected = settings
                .dynamic_priority_boost
                .override_for(&process_name, true)
                .flatten()
                .unwrap_or_default();
            options = process_list_priority_editor_options(
                options,
                &process_name,
                column,
                selected,
                &ProcessDynamicPriorityBoostSetting::CUSTOM_RULE_ALL,
                process_dynamic_priority_boost_setting_label,
                WinderustApp::set_process_list_dynamic_priority_boost,
                cx,
            );
        }
        ProcessListColumn::IoPriority => {
            let selected = settings
                .io_priority
                .override_for(&process_name, true)
                .flatten()
                .unwrap_or_default();
            let values = if app.settings.advanced.expose_all_priority_values {
                &ProcessIoPrioritySetting::CUSTOM_RULE_ADVANCED_ALL[..]
            } else {
                &ProcessIoPrioritySetting::CUSTOM_RULE_ALL[..]
            };
            options = process_list_priority_editor_options(
                options,
                &process_name,
                column,
                selected,
                values,
                process_io_priority_setting_label,
                WinderustApp::set_process_list_io_priority,
                cx,
            );
        }
        ProcessListColumn::GpuPriority => {
            let selected = settings
                .gpu_priority
                .override_for(&process_name, true)
                .flatten()
                .unwrap_or_default();
            let values = if app.settings.advanced.expose_all_priority_values {
                &ProcessGpuPrioritySetting::CUSTOM_RULE_ADVANCED_ALL[..]
            } else {
                &ProcessGpuPrioritySetting::CUSTOM_RULE_ALL[..]
            };
            options = process_list_priority_editor_options(
                options,
                &process_name,
                column,
                selected,
                values,
                process_gpu_priority_setting_label,
                WinderustApp::set_process_list_gpu_priority,
                cx,
            );
        }
        ProcessListColumn::MemoryPriority => {
            let selected = settings
                .memory_priority
                .override_for(&process_name, true)
                .flatten()
                .unwrap_or_default();
            options = process_list_priority_editor_options(
                options,
                &process_name,
                column,
                selected,
                &ProcessMemoryPrioritySetting::CUSTOM_RULE_ALL,
                process_memory_priority_setting_label,
                WinderustApp::set_process_list_memory_priority,
                cx,
            );
        }
        ProcessListColumn::Pid
        | ProcessListColumn::Status
        | ProcessListColumn::CpuUsage
        | ProcessListColumn::MemoryUsage => {}
    }

    options
}

pub(in crate::ui::app) fn process_list_include_exclude_editor_options(
    mut options: Scrollable<gpui::Div>,
    process_name: &str,
    column: ProcessListColumn,
    included: bool,
    cx: &mut Context<WinderustApp>,
) -> Scrollable<gpui::Div> {
    options = options.child(process_list_include_exclude_editor_option(
        process_name,
        column,
        true,
        included,
        cx,
    ));
    options.child(process_list_include_exclude_editor_option(
        process_name,
        column,
        false,
        !included,
        cx,
    ))
}

#[expect(
    clippy::too_many_arguments,
    reason = "keeps six typed priority selectors on one rendering path"
)]
pub(in crate::ui::app) fn process_list_priority_editor_options<T>(
    mut options: Scrollable<gpui::Div>,
    process_name: &str,
    column: ProcessListColumn,
    selected: T,
    values: &[T],
    label: fn(T) -> String,
    apply: fn(&mut WinderustApp, String, T, &mut Context<WinderustApp>),
    cx: &mut Context<WinderustApp>,
) -> Scrollable<gpui::Div>
where
    T: Copy + PartialEq + 'static,
{
    for value in values.iter().copied() {
        let process_name = process_name.to_owned();
        let value_label = label(value);
        let option_id = process_list_editor_option_id(&process_name, column, &value_label);
        options = options.child(
            dropdown_option_row(option_id, value_label, selected == value, cx).on_click(
                cx.listener(move |app, _, _, cx| {
                    apply(app, process_name.clone(), value, cx);
                    cx.stop_propagation();
                }),
            ),
        );
    }
    options
}

pub(in crate::ui::app) fn process_list_editor_option_id(
    process_name: &str,
    column: ProcessListColumn,
    suffix: impl std::fmt::Display,
) -> SharedString {
    SharedString::from(format!(
        "{}-{suffix}",
        process_list_cell_editor_id(process_name, column)
    ))
}

pub(in crate::ui::app) fn process_list_power_plan_editor_option(
    process_name: &str,
    column: ProcessListColumn,
    label: String,
    selected: bool,
    guid: Option<String>,
    cx: &mut Context<WinderustApp>,
) -> AnyElement {
    let process_name = process_name.to_owned();
    let option_id =
        process_list_editor_option_id(&process_name, column, guid.as_deref().unwrap_or("default"));
    dropdown_option_row(option_id, label, selected, cx)
        .on_click(cx.listener(move |app, _, _, cx| {
            match column {
                ProcessListColumn::PowerPlanForeground => app
                    .set_process_list_foreground_power_plan(process_name.clone(), guid.clone(), cx),
                ProcessListColumn::PowerPlanRunning => {
                    app.set_process_list_running_power_plan(process_name.clone(), guid.clone(), cx)
                }
                _ => {}
            }
            cx.stop_propagation();
        }))
        .into_any_element()
}

pub(in crate::ui::app) fn process_list_include_exclude_editor_option(
    process_name: &str,
    column: ProcessListColumn,
    included: bool,
    selected: bool,
    cx: &mut Context<WinderustApp>,
) -> AnyElement {
    let process_name = process_name.to_owned();
    let label = process_list_include_exclude_label(included);
    let option_id = process_list_editor_option_id(&process_name, column, label.as_str());
    dropdown_option_row(option_id, label, selected, cx)
        .on_click(cx.listener(move |app, _, _, cx| {
            match column {
                ProcessListColumn::AdaptiveEngine => {
                    app.set_process_list_adaptive_engine(process_name.clone(), included, cx)
                }
                ProcessListColumn::BackgroundEfficiency => {
                    app.set_process_list_background_efficiency(process_name.clone(), included, cx)
                }
                _ => {}
            }
            cx.stop_propagation();
        }))
        .into_any_element()
}
pub(in crate::ui::app) fn process_list_count_label(count: usize) -> String {
    t!("process_list.count", count = count).to_string()
}

pub(in crate::ui::app) fn process_list_toolbar_label(
    app: &WinderustApp,
    process_count: usize,
) -> String {
    let Some(process) = app.selected_process_id.and_then(|id| {
        app.running_processes
            .iter()
            .find(|process| process.id == id)
    }) else {
        return process_list_count_label(process_count);
    };
    let custom_count = process_policy_summary(
        &app.settings,
        &app.plans,
        process
            .image_path
            .as_deref()
            .map(|path| path.to_string_lossy())
            .as_deref()
            .unwrap_or_default(),
    )
    .custom_columns
    .len();

    t!(
        "process_list.selected_summary",
        name = process.name.clone(),
        pid = process.id,
        count = custom_count
    )
    .to_string()
}

pub(in crate::ui::app) fn process_list_pid_count_label(count: usize) -> String {
    t!("common.pid_count", count = count).to_string()
}

#[cfg(test)]
mod tests;
