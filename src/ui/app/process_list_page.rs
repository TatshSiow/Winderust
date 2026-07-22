use super::*;

impl WinderustApp {
    pub(super) fn render_process_list_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let render_data = process_list_render_data(self);
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
        let refresh_button = control_button(Button::new("refresh-process-list"))
            .label(t!("settings.refresh").to_string())
            .on_click(cx.listener(|app, _, _, cx| {
                let changed_candidates = app.refresh_process_candidates(false);
                let changed_processes = app.refresh_running_processes(true);
                if changed_candidates || changed_processes {
                    cx.notify();
                }
            }));
        let column_visibility_button =
            self.render_process_list_column_visibility_dropdown(window, cx);

        let header = process_list_scroll_content(table_width).child(process_list_header_row(
            &self.settings,
            &self.hidden_process_list_columns,
            &column_layout,
            self.process_list_sort,
            cx,
        ));
        let rows = if rendered_rows.is_empty() {
            process_list_scroll_content(table_width)
                .child(process_list_empty_row(
                    t!("common.no_running_apps_loaded").to_string(),
                ))
                .into_any_element()
        } else {
            let hidden_columns = self.hidden_process_list_columns.clone();
            let column_layout = column_layout.clone();
            let rows = Rc::clone(&rendered_rows);

            v_virtual_list(
                cx.entity(),
                "process-list-rows",
                item_sizes,
                move |app, visible_range, window, cx| {
                    let row_layout = ProcessListRenderLayout {
                        hidden_columns: &hidden_columns,
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

        self.page_shell(Page::ProcessList, cx)
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
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.0))
                            .truncate()
                            .child(text_muted(process_list_toolbar_label(self, process_count))),
                    )
                    .child(column_visibility_button)
                    .child(refresh_button),
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
            .into_any_element()
    }

    pub(super) fn render_process_list_column_visibility_dropdown(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let id = PROCESS_LIST_COLUMN_VISIBILITY_DROPDOWN_ID;
        let is_open = self.active_power_plan_picker.as_deref() == Some(id);
        let placement = self.dropdown_placement(
            id,
            dropdown_list_height(PROCESS_LIST_OPTIONAL_COLUMNS.len()),
            window,
        );
        let phase = dropdown_popup_phase(id, is_open, cx);
        let button_id = SharedString::from(format!("{id}-button"));
        let toggle_id = id.to_owned();
        let button = dropdown_select_control(
            button_id,
            t!("process_list.column_visibility").to_string(),
            true,
            is_open,
            phase,
            cx,
        )
        .on_click(cx.listener(move |app, _, _, cx| {
            app.active_power_plan_picker = (app.active_power_plan_picker.as_deref()
                != Some(toggle_id.as_str()))
            .then_some(toggle_id.clone());
            cx.stop_propagation();
            cx.notify();
        }));
        let options = process_list_column_visibility_dropdown_options(
            &self.hidden_process_list_columns,
            &self.settings,
            placement.max_height,
            cx,
        );

        v_flex()
            .w(px(PROCESS_LIST_COLUMN_VISIBILITY_DROPDOWN_WIDTH))
            .min_w(px(PROCESS_LIST_COLUMN_VISIBILITY_DROPDOWN_WIDTH))
            .max_w(px(PROCESS_LIST_COLUMN_VISIBILITY_DROPDOWN_WIDTH))
            .flex_shrink_0()
            .relative()
            .min_h(px(DROPDOWN_CONTROL_HEIGHT))
            .child(button)
            .child(dropdown_anchor_sensor(
                id.to_owned(),
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
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ProcessListSortColumn {
    ProcessName,
    Column(ProcessListColumn),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ProcessListSortDirection {
    Ascending,
    Descending,
}

impl ProcessListSortDirection {
    pub(super) fn toggled(self) -> Self {
        match self {
            Self::Ascending => Self::Descending,
            Self::Descending => Self::Ascending,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct ProcessListSort {
    pub(super) column: ProcessListSortColumn,
    pub(super) direction: ProcessListSortDirection,
}

impl ProcessListSort {
    pub(super) fn toggled_for(self, column: ProcessListSortColumn) -> Self {
        if self.column == column {
            Self {
                column,
                direction: self.direction.toggled(),
            }
        } else {
            Self {
                column,
                direction: ProcessListSortDirection::Ascending,
            }
        }
    }
}

impl Default for ProcessListSort {
    fn default() -> Self {
        Self {
            column: ProcessListSortColumn::ProcessName,
            direction: ProcessListSortDirection::Ascending,
        }
    }
}

pub(super) struct ProcessListGroup<'a> {
    pub(super) display_name: String,
    pub(super) processes: Vec<&'a ProcessInfo>,
}

#[derive(Clone)]
pub(super) struct ProcessListColumnLayout {
    pub(super) name_width: f32,
    pub(super) column_widths: HashMap<ProcessListColumn, f32>,
}

impl ProcessListColumnLayout {
    pub(super) fn column_width(&self, column: ProcessListColumn) -> f32 {
        self.column_widths
            .get(&column)
            .copied()
            .unwrap_or_else(|| process_list_column_min_width(column))
    }
}

#[derive(Clone, Copy)]
pub(super) struct ProcessListRenderLayout<'a> {
    pub(super) hidden_columns: &'a HashSet<ProcessListColumn>,
    pub(super) column_layout: &'a ProcessListColumnLayout,
}

#[derive(Clone, Copy)]
pub(super) struct ProcessListGroupRowState {
    pub(super) collapsed: bool,
    pub(super) divided: bool,
}

#[derive(Clone, Copy)]
pub(super) struct ProcessListEntryRowState {
    pub(super) divided: bool,
    pub(super) nested: bool,
    pub(super) editable: bool,
}

#[derive(Clone, Copy)]
pub(super) struct ProcessListGroupRowData<'a> {
    pub(super) process_name: &'a str,
    pub(super) process_count: usize,
}

#[derive(Clone)]
pub(super) enum ProcessListRenderedRow {
    Entry {
        process: ProcessInfo,
        summary: Arc<ProcessPolicySummary>,
        icon: Option<Arc<Image>>,
        state: ProcessListEntryRowState,
    },
    Group {
        process_name: String,
        process_count: usize,
        summary: Arc<ProcessPolicySummary>,
        icon: Option<Arc<Image>>,
        state: ProcessListGroupRowState,
    },
}

pub(super) struct ProcessListRenderData {
    pub(super) process_count: usize,
    pub(super) column_layout: ProcessListColumnLayout,
    pub(super) table_width: Pixels,
    pub(super) rows: Rc<Vec<ProcessListRenderedRow>>,
    pub(super) item_sizes: Rc<Vec<gpui::Size<Pixels>>>,
}

#[derive(Clone, Copy)]
pub(super) struct ProcessListEditContext<'a> {
    pub(super) app: &'a WinderustApp,
    pub(super) window: &'a Window,
}

#[derive(Clone, Copy)]
pub(super) struct ProcessListPolicyCellTarget<'a> {
    pub(super) process_name: &'a str,
    pub(super) column: ProcessListColumn,
    pub(super) editable: bool,
}

pub(super) fn process_list_groups(processes: &[ProcessInfo]) -> Vec<ProcessListGroup<'_>> {
    let mut groups = Vec::<ProcessListGroup<'_>>::with_capacity(processes.len());
    let mut group_indexes = HashMap::<String, usize>::with_capacity(processes.len());

    for process in processes {
        let key = process_list_group_key(&process.name);
        if let Some(index) = group_indexes.get(&key).copied() {
            groups[index].processes.push(process);
        } else {
            group_indexes.insert(key, groups.len());
            groups.push(ProcessListGroup {
                display_name: process.name.clone(),
                processes: vec![process],
            });
        }
    }

    groups
}

pub(super) fn process_list_sorted_rows<'a>(
    groups: Vec<ProcessListGroup<'a>>,
    summaries: Vec<ProcessPolicySummary>,
    sort: ProcessListSort,
) -> Vec<(ProcessListGroup<'a>, ProcessPolicySummary)> {
    let mut rows = Vec::with_capacity(groups.len().min(summaries.len()));
    for row in groups.into_iter().zip(summaries) {
        rows.push(row);
    }
    rows.sort_by(|(left_group, left_summary), (right_group, right_summary)| {
        process_list_group_sort_cmp(left_group, left_summary, right_group, right_summary, sort)
    });
    rows
}

pub(super) fn process_list_rendered_rows(
    rows: &[(ProcessListGroup<'_>, ProcessPolicySummary)],
    process_icons_by_name: &HashMap<&str, &Arc<Image>>,
    is_group_collapsed: impl Fn(&str) -> bool,
) -> Vec<ProcessListRenderedRow> {
    let max_rendered_rows = rows
        .iter()
        .map(|(group, _)| {
            if group.processes.len() == 1 {
                1
            } else {
                1 + group.processes.len()
            }
        })
        .sum();
    let mut rendered_rows = Vec::with_capacity(max_rendered_rows);
    let mut row_index = 0usize;

    for (group, summary) in rows {
        let icon = process_icons_by_name
            .get(group.display_name.as_str())
            .copied()
            .map(Arc::clone);
        let summary = Arc::new(summary.clone());
        let divided = row_index > 0;

        if group.processes.len() == 1 {
            rendered_rows.push(ProcessListRenderedRow::Entry {
                process: group.processes[0].to_owned(),
                summary,
                icon,
                state: ProcessListEntryRowState {
                    divided,
                    nested: false,
                    editable: true,
                },
            });
            row_index += 1;
            continue;
        }

        let collapsed = is_group_collapsed(&group.display_name);
        rendered_rows.push(ProcessListRenderedRow::Group {
            process_name: group.display_name.clone(),
            process_count: group.processes.len(),
            summary: Arc::clone(&summary),
            icon: icon.clone(),
            state: ProcessListGroupRowState { collapsed, divided },
        });
        row_index += 1;

        if !collapsed {
            for process in &group.processes {
                rendered_rows.push(ProcessListRenderedRow::Entry {
                    process: (*process).to_owned(),
                    summary: Arc::clone(&summary),
                    icon: icon.clone(),
                    state: ProcessListEntryRowState {
                        divided: true,
                        nested: true,
                        editable: false,
                    },
                });
                row_index += 1;
            }
        }
    }

    rendered_rows
}

pub(super) fn process_list_render_data(app: &WinderustApp) -> ProcessListRenderData {
    let process_count = app.running_processes.len();
    let mut process_groups = process_list_groups(&app.running_processes);
    for group in &mut process_groups {
        process_list_sort_group_processes(group, app.process_list_sort);
    }
    let mut process_summaries = Vec::with_capacity(process_groups.len());
    for group in &process_groups {
        process_summaries.push(process_policy_summary(
            &app.settings,
            &app.plans,
            &group.display_name,
        ));
    }
    let column_layout =
        process_list_column_layout(&app.settings, &process_groups, &process_summaries);
    let process_rows =
        process_list_sorted_rows(process_groups, process_summaries, app.process_list_sort);
    let table_width = process_list_table_width(&app.hidden_process_list_columns, &column_layout);
    let process_icons_by_name = app
        .process_candidates
        .iter()
        .filter_map(|candidate| {
            candidate
                .icon
                .as_ref()
                .map(|icon| (candidate.name.as_str(), icon))
        })
        .collect::<HashMap<_, _>>();
    let rows = process_list_rendered_rows(&process_rows, &process_icons_by_name, |process_name| {
        app.is_process_list_group_collapsed(process_name)
    });
    let item_sizes = Rc::new(vec![
        size(table_width, px(PROCESS_LIST_ROW_HEIGHT));
        rows.len()
    ]);

    ProcessListRenderData {
        process_count,
        column_layout,
        table_width,
        rows: Rc::new(rows),
        item_sizes,
    }
}

pub(super) fn process_list_sort_group_processes(
    group: &mut ProcessListGroup<'_>,
    sort: ProcessListSort,
) {
    group
        .processes
        .sort_by(|left, right| process_list_process_sort_cmp(left, right, sort));
}

pub(super) fn process_list_group_sort_cmp(
    left_group: &ProcessListGroup<'_>,
    left_summary: &ProcessPolicySummary,
    right_group: &ProcessListGroup<'_>,
    right_summary: &ProcessPolicySummary,
    sort: ProcessListSort,
) -> CmpOrdering {
    let primary = match sort.column {
        ProcessListSortColumn::ProcessName => {
            process_list_text_sort_cmp(&left_group.display_name, &right_group.display_name)
        }
        ProcessListSortColumn::Column(ProcessListColumn::Pid) => {
            process_list_group_sort_pid(left_group, sort.direction)
                .cmp(&process_list_group_sort_pid(right_group, sort.direction))
        }
        ProcessListSortColumn::Column(column) => process_list_text_sort_cmp(
            process_list_column_value(left_summary, column).as_ref(),
            process_list_column_value(right_summary, column).as_ref(),
        ),
    };

    process_list_directional_cmp(primary, sort.direction)
        .then_with(|| {
            process_list_text_sort_cmp(&left_group.display_name, &right_group.display_name)
        })
        .then_with(|| {
            process_list_group_min_pid(left_group).cmp(&process_list_group_min_pid(right_group))
        })
}

pub(super) fn process_list_process_sort_cmp(
    left: &ProcessInfo,
    right: &ProcessInfo,
    sort: ProcessListSort,
) -> CmpOrdering {
    let primary = match sort.column {
        ProcessListSortColumn::ProcessName => process_list_text_sort_cmp(&left.name, &right.name),
        ProcessListSortColumn::Column(ProcessListColumn::Pid) => left.id.cmp(&right.id),
        ProcessListSortColumn::Column(_) => CmpOrdering::Equal,
    };

    process_list_directional_cmp(primary, sort.direction)
        .then_with(|| process_list_text_sort_cmp(&left.name, &right.name))
        .then_with(|| left.id.cmp(&right.id))
}

pub(super) fn process_list_directional_cmp(
    ordering: CmpOrdering,
    direction: ProcessListSortDirection,
) -> CmpOrdering {
    match direction {
        ProcessListSortDirection::Ascending => ordering,
        ProcessListSortDirection::Descending => ordering.reverse(),
    }
}

pub(super) fn process_list_group_min_pid(group: &ProcessListGroup<'_>) -> u32 {
    group
        .processes
        .iter()
        .map(|process| process.id)
        .min()
        .unwrap_or_default()
}

pub(super) fn process_list_group_sort_pid(
    group: &ProcessListGroup<'_>,
    direction: ProcessListSortDirection,
) -> u32 {
    let pids = group.processes.iter().map(|process| process.id);
    match direction {
        ProcessListSortDirection::Ascending => pids.min(),
        ProcessListSortDirection::Descending => pids.max(),
    }
    .unwrap_or_default()
}

pub(super) fn process_list_text_sort_cmp(left: &str, right: &str) -> CmpOrdering {
    left.bytes()
        .map(|byte| byte.to_ascii_lowercase())
        .cmp(right.bytes().map(|byte| byte.to_ascii_lowercase()))
        .then_with(|| left.cmp(right))
}

pub(super) fn process_list_group_key(process_name: &str) -> String {
    process_name.trim().to_ascii_lowercase()
}

pub(super) fn process_list_column_visible(
    hidden_columns: &HashSet<ProcessListColumn>,
    column: ProcessListColumn,
) -> bool {
    !hidden_columns.contains(&column)
}

pub(super) fn process_list_column_min_width(column: ProcessListColumn) -> f32 {
    match column {
        ProcessListColumn::Pid => PROCESS_LIST_PID_MIN_WIDTH,
        ProcessListColumn::CoreLimiter
        | ProcessListColumn::CoreSteering
        | ProcessListColumn::MemoryTrim => 86.0,
        ProcessListColumn::BackgroundEfficiency
        | ProcessListColumn::ProcessPriority
        | ProcessListColumn::AppSuspension
        | ProcessListColumn::TimerResolution => 112.0,
        ProcessListColumn::PowerPlanForeground
        | ProcessListColumn::PowerPlanRunning
        | ProcessListColumn::BackgroundCpuRestriction
        | ProcessListColumn::IoPriority
        | ProcessListColumn::GpuPriority
        | ProcessListColumn::MemoryPriority => PROCESS_LIST_COLUMN_MIN_WIDTH,
    }
}

pub(super) fn process_list_column_max_width(column: ProcessListColumn) -> f32 {
    match column {
        ProcessListColumn::Pid => PROCESS_LIST_PID_MAX_WIDTH,
        ProcessListColumn::CoreLimiter
        | ProcessListColumn::CoreSteering
        | ProcessListColumn::ProcessPriority
        | ProcessListColumn::MemoryTrim
        | ProcessListColumn::AppSuspension
        | ProcessListColumn::TimerResolution => 170.0,
        ProcessListColumn::BackgroundEfficiency => 190.0,
        ProcessListColumn::PowerPlanForeground
        | ProcessListColumn::PowerPlanRunning
        | ProcessListColumn::BackgroundCpuRestriction
        | ProcessListColumn::IoPriority
        | ProcessListColumn::GpuPriority
        | ProcessListColumn::MemoryPriority => PROCESS_LIST_COLUMN_MAX_WIDTH,
    }
}

pub(super) fn process_list_column_label(column: ProcessListColumn, settings: &Settings) -> String {
    match column {
        ProcessListColumn::Pid => t!("process_list.pid").to_string(),
        ProcessListColumn::PowerPlanForeground => {
            t!("process_list.power_plan_foreground").to_string()
        }
        ProcessListColumn::PowerPlanRunning => t!("process_list.power_plan_running").to_string(),
        ProcessListColumn::BackgroundEfficiency => {
            t!("process_list.background_efficiency").to_string()
        }
        ProcessListColumn::CoreLimiter => t!("process_list.core_limiter").to_string(),
        ProcessListColumn::BackgroundCpuRestriction => {
            t!("process_list.background_cpu_restriction").to_string()
        }
        ProcessListColumn::CoreSteering => t!("process_list.core_steering").to_string(),
        ProcessListColumn::ProcessPriority => t!("process_list.process_priority").to_string(),
        ProcessListColumn::IoPriority => process_list_priority_header_label(
            t!("process_list.io_priority").to_string(),
            io_priority_has_foreground_background_split(&settings.io_priority),
        ),
        ProcessListColumn::GpuPriority => process_list_priority_header_label(
            t!("process_list.gpu_priority").to_string(),
            gpu_priority_has_foreground_background_split(&settings.gpu_priority),
        ),
        ProcessListColumn::MemoryPriority => process_list_priority_header_label(
            t!("process_list.memory_priority").to_string(),
            memory_priority_has_foreground_background_split(&settings.memory_priority),
        ),
        ProcessListColumn::MemoryTrim => t!("process_list.memory_trim").to_string(),
        ProcessListColumn::AppSuspension => t!("process_list.app_suspension").to_string(),
        ProcessListColumn::TimerResolution => t!("process_list.timer_resolution").to_string(),
    }
}

pub(super) fn process_list_column_layout(
    settings: &Settings,
    groups: &[ProcessListGroup<'_>],
    summaries: &[ProcessPolicySummary],
) -> ProcessListColumnLayout {
    let process_name_label = t!("process_list.process_name").to_string();
    let mut name_width = process_list_estimated_cell_width(
        &process_name_label,
        process_list_header_cell_non_text_width(),
    );
    for group in groups {
        name_width = name_width.max(process_list_estimated_cell_width(
            &group.display_name,
            PROCESS_LIST_NAME_CELL_NON_TEXT_WIDTH,
        ));
        if group.processes.len() > 1 {
            name_width = name_width.max(process_list_estimated_cell_width(
                &format!("{} x{}", group.display_name, group.processes.len()),
                PROCESS_LIST_NAME_CELL_NON_TEXT_WIDTH,
            ));
        }
        for process in &group.processes {
            name_width = name_width.max(process_list_estimated_cell_width(
                &process.name,
                PROCESS_LIST_NAME_CELL_NON_TEXT_WIDTH,
            ));
        }
    }
    let name_width = name_width.clamp(PROCESS_LIST_NAME_MIN_WIDTH, PROCESS_LIST_NAME_MAX_WIDTH);

    let mut column_widths = HashMap::new();
    for column in PROCESS_LIST_OPTIONAL_COLUMNS {
        let mut width = process_list_estimated_cell_width(
            &process_list_column_label(column, settings),
            process_list_header_cell_non_text_width(),
        );

        if column == ProcessListColumn::Pid {
            for group in groups {
                if group.processes.len() > 1 {
                    width = width.max(process_list_estimated_cell_width(
                        &process_list_pid_count_label(group.processes.len()),
                        PROCESS_LIST_TEXT_CELL_HORIZONTAL_PADDING,
                    ));
                }
                for process in &group.processes {
                    width = width.max(process_list_estimated_cell_width(
                        &process.id.to_string(),
                        PROCESS_LIST_TEXT_CELL_HORIZONTAL_PADDING,
                    ));
                }
            }
        } else {
            for summary in summaries {
                let value = process_list_column_value(summary, column);
                width = width.max(process_list_estimated_policy_value_width(
                    column,
                    value.as_ref(),
                ));
            }
        }

        column_widths.insert(
            column,
            width.clamp(
                process_list_column_min_width(column),
                process_list_column_max_width(column),
            ),
        );
    }

    ProcessListColumnLayout {
        name_width,
        column_widths,
    }
}

pub(super) fn process_list_estimated_cell_width(text: &str, extra_width: f32) -> f32 {
    process_list_estimated_text_width(text) + extra_width
}

pub(super) fn process_list_estimated_policy_value_width(
    column: ProcessListColumn,
    value: &str,
) -> f32 {
    if process_list_column_uses_split_priority_display(column) {
        if let Some((foreground, background)) = process_list_split_policy_value(value) {
            let lane_extra_width = PROCESS_LIST_TEXT_CELL_HORIZONTAL_PADDING
                + PROCESS_LIST_SPLIT_LABEL_WIDTH
                + PROCESS_LIST_SPLIT_LABEL_GAP;
            return process_list_estimated_cell_width(foreground, lane_extra_width).max(
                process_list_estimated_cell_width(background, lane_extra_width),
            );
        }
    }

    process_list_estimated_cell_width(value, PROCESS_LIST_TEXT_CELL_HORIZONTAL_PADDING)
}

pub(super) fn process_list_header_cell_non_text_width() -> f32 {
    PROCESS_LIST_TEXT_CELL_HORIZONTAL_PADDING
        + PROCESS_LIST_SORT_ICON_WIDTH
        + PROCESS_LIST_SORT_HEADER_GAP
}

pub(super) fn process_list_estimated_text_width(text: &str) -> f32 {
    text.chars().map(process_list_estimated_char_width).sum()
}

pub(super) fn process_list_estimated_char_width(character: char) -> f32 {
    if !character.is_ascii() {
        return 13.0;
    }

    match character {
        'i' | 'l' | 'I' | '|' | '!' | '.' | ',' | ':' | ';' => 3.8,
        ' ' => 4.0,
        '/' | '\\' | '-' | '_' | '(' | ')' => 5.0,
        'm' | 'w' | 'M' | 'W' => 10.0,
        character if character.is_ascii_uppercase() || character.is_ascii_digit() => 7.4,
        _ => 6.8,
    }
}

pub(super) fn process_list_table_width(
    hidden_columns: &HashSet<ProcessListColumn>,
    layout: &ProcessListColumnLayout,
) -> Pixels {
    let visible_columns = PROCESS_LIST_OPTIONAL_COLUMNS
        .iter()
        .copied()
        .filter(|column| process_list_column_visible(hidden_columns, *column))
        .collect::<Vec<_>>();
    let visible_column_count = 1 + visible_columns.len();
    let data_width = layout.name_width
        + visible_columns
            .iter()
            .copied()
            .map(|column| layout.column_width(column))
            .sum::<f32>();
    let gap_count = visible_column_count.saturating_sub(1) as f32;

    px(data_width + PROCESS_LIST_ROW_HORIZONTAL_PADDING + PROCESS_LIST_COLUMN_GAP * gap_count)
}

pub(super) fn process_list_scroll_height(window: &Window) -> Pixels {
    let reserved_height = TITLE_BAR_HEIGHT
        + PAGE_HEADER_HEIGHT
        + PAGE_CONTENT_VERTICAL_PADDING * 2.0
        + PROCESS_LIST_TOOLBAR_HEIGHT
        + PROCESS_LIST_VERTICAL_GAP_TOTAL;

    (window.viewport_size().height - px(reserved_height)).max(Pixels::ZERO)
}

pub(super) fn process_list_surface() -> gpui::Div {
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

pub(super) fn process_list_scroll_content(table_width: Pixels) -> gpui::Div {
    v_flex().w(table_width).min_w(table_width)
}

pub(super) fn process_list_header_row(
    settings: &Settings,
    hidden_columns: &HashSet<ProcessListColumn>,
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

    for column in PROCESS_LIST_OPTIONAL_COLUMNS {
        if process_list_column_visible(hidden_columns, column) {
            row = row.child(process_list_header_cell(
                layout.column_width(column),
                process_list_column_label(column, settings),
                ProcessListSortColumn::Column(column),
                sort,
                cx,
            ));
        }
    }

    row
}

pub(super) fn process_list_priority_header_label(
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

pub(super) fn process_list_foreground_short_label() -> &'static str {
    "FG"
}

pub(super) fn process_list_background_short_label() -> &'static str {
    "BG"
}

pub(super) fn process_list_header_cell(
    width: f32,
    label: String,
    column: ProcessListSortColumn,
    sort: ProcessListSort,
    cx: &mut Context<WinderustApp>,
) -> AnyElement {
    let active = sort.column == column;

    h_flex()
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
        }))
        .child(div().flex_1().min_w(px(0.0)).truncate().child(label))
        .child(process_list_sort_icon(active, sort.direction, cx))
        .into_any_element()
}

pub(super) fn process_list_sort_column_id(column: ProcessListSortColumn) -> &'static str {
    match column {
        ProcessListSortColumn::ProcessName => "process-name",
        ProcessListSortColumn::Column(ProcessListColumn::Pid) => "pid",
        ProcessListSortColumn::Column(ProcessListColumn::PowerPlanForeground) => {
            "power-plan-foreground"
        }
        ProcessListSortColumn::Column(ProcessListColumn::PowerPlanRunning) => "power-plan-running",
        ProcessListSortColumn::Column(ProcessListColumn::BackgroundEfficiency) => {
            "background-efficiency"
        }
        ProcessListSortColumn::Column(ProcessListColumn::CoreLimiter) => "core-limiter",
        ProcessListSortColumn::Column(ProcessListColumn::BackgroundCpuRestriction) => {
            "background-cpu-restriction"
        }
        ProcessListSortColumn::Column(ProcessListColumn::CoreSteering) => "core-steering",
        ProcessListSortColumn::Column(ProcessListColumn::ProcessPriority) => "process-priority",
        ProcessListSortColumn::Column(ProcessListColumn::IoPriority) => "io-priority",
        ProcessListSortColumn::Column(ProcessListColumn::GpuPriority) => "gpu-priority",
        ProcessListSortColumn::Column(ProcessListColumn::MemoryPriority) => "memory-priority",
        ProcessListSortColumn::Column(ProcessListColumn::MemoryTrim) => "memory-trim",
        ProcessListSortColumn::Column(ProcessListColumn::AppSuspension) => "app-suspension",
        ProcessListSortColumn::Column(ProcessListColumn::TimerResolution) => "timer-resolution",
    }
}

pub(super) fn process_list_sort_icon(
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

pub(super) fn process_list_column_visibility_dropdown_options(
    hidden_columns: &HashSet<ProcessListColumn>,
    settings: &Settings,
    max_height: Pixels,
    cx: &mut Context<WinderustApp>,
) -> Scrollable<gpui::Div> {
    let mut options = dropdown_surface(cx, max_height);

    for column in PROCESS_LIST_OPTIONAL_COLUMNS {
        let checked = process_list_column_visible(hidden_columns, column);
        let label = process_list_column_label(column, settings);

        options = options.child(
            h_flex()
                .id(SharedString::from(format!(
                    "process-list-column-visibility-option-{}",
                    column as usize
                )))
                .min_h(px(DROPDOWN_OPTION_ROW_HEIGHT))
                .items_center()
                .px_2()
                .rounded(px(BRAND_RADIUS_CONTROL))
                .hover(|style| style.bg(rgb(dropdown_option_hover_color())))
                .child(checkbox(
                    SharedString::from(format!(
                        "process-list-column-visibility-{}",
                        column as usize
                    )),
                    label,
                    checked,
                    cx.listener(move |app, checked, _, cx| {
                        app.set_process_list_column_visible(column, *checked, cx);
                    }),
                )),
        );
    }

    options
}

pub(super) fn process_list_empty_row(message: impl Into<SharedString>) -> gpui::Div {
    h_flex()
        .w_full()
        .min_w(px(0.0))
        .h(px(CARD_ROW_HEIGHT))
        .items_center()
        .px_4()
        .py_3()
        .child(text_muted(message.into()))
}

pub(super) fn process_list_rendered_row(
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
            process_name,
            process_count,
            summary,
            icon,
            state,
        } => process_list_group_row(
            ProcessListGroupRowData {
                process_name: process_name.as_str(),
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

pub(super) fn process_list_entry_row(
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
    let selected = edit_context.app.selected_process_id == Some(process_id);
    let app_entity = cx.entity().clone();
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
        .hover(|style| style.bg(rgb(settings_card_hover_color())))
        .cursor_pointer()
        .on_click(cx.listener(move |app, _, _, cx| {
            app.selected_process_id = Some(process_id);
            cx.notify();
        }))
        .context_menu(move |menu, _, _| {
            let mut menu = menu;
            let action_target = capture_process_action_target(process_id, &process_name);
            for priority in [
                ProcessPrioritySetting::BelowNormal,
                ProcessPrioritySetting::Normal,
                ProcessPrioritySetting::AboveNormal,
            ] {
                let app_entity = app_entity.clone();
                let process_name = process_name.clone();
                let action_target = action_target.clone();
                menu = menu.item(
                    PopupMenuItem::new(t!(
                        "process_list.apply_once_priority",
                        priority = process_priority_setting_label(priority)
                    ))
                    .on_click(move |_, _, cx| {
                        app_entity.update(cx, |app, cx| {
                            app.selected_process_id = Some(process_id);
                            app.apply_process_priority_once(
                                action_target.clone(),
                                &process_name,
                                priority,
                                cx,
                            );
                        });
                    }),
                );
            }
            for priority in [
                ProcessPrioritySetting::BelowNormal,
                ProcessPrioritySetting::Normal,
                ProcessPrioritySetting::AboveNormal,
            ] {
                let app_entity = app_entity.clone();
                let process_name = process_name.clone();
                menu = menu.item(
                    PopupMenuItem::new(t!(
                        "process_list.save_rule_priority",
                        priority = process_priority_setting_label(priority)
                    ))
                    .on_click(move |_, _, cx| {
                        app_entity.update(cx, |app, cx| {
                            app.selected_process_id = Some(process_id);
                            app.save_process_priority_rule(&process_name, priority, cx);
                        });
                    }),
                );
            }
            for priority in [
                ProcessMemoryPrioritySetting::Low,
                ProcessMemoryPrioritySetting::Normal,
            ] {
                let app_entity = app_entity.clone();
                let process_name = process_name.clone();
                let action_target = action_target.clone();
                menu = menu.item(
                    PopupMenuItem::new(t!(
                        "process_list.apply_once_memory",
                        priority = process_memory_priority_setting_label(priority)
                    ))
                    .on_click(move |_, _, cx| {
                        app_entity.update(cx, |app, cx| {
                            app.selected_process_id = Some(process_id);
                            app.apply_memory_priority_once(
                                action_target.clone(),
                                &process_name,
                                priority,
                                cx,
                            );
                        });
                    }),
                );
            }
            for priority in [
                ProcessMemoryPrioritySetting::Low,
                ProcessMemoryPrioritySetting::Normal,
            ] {
                let app_entity = app_entity.clone();
                let process_name = process_name.clone();
                menu = menu.item(
                    PopupMenuItem::new(t!(
                        "process_list.save_rule_memory",
                        priority = process_memory_priority_setting_label(priority)
                    ))
                    .on_click(move |_, _, cx| {
                        app_entity.update(cx, |app, cx| {
                            app.selected_process_id = Some(process_id);
                            app.save_memory_priority_rule(&process_name, priority, cx);
                        });
                    }),
                );
            }
            menu
        })
        .child(process_list_name_cell(
            process.name.clone(),
            icon,
            state.nested,
            layout.column_layout.name_width,
            cx,
        ));

    if process_list_column_visible(layout.hidden_columns, ProcessListColumn::Pid) {
        row = row.child(process_list_text_cell(
            layout.column_layout.column_width(ProcessListColumn::Pid),
            process.id.to_string(),
        ));
    }

    row.children(process_list_policy_cells(
        &process.name,
        summary,
        layout,
        state.editable,
        edit_context,
        cx,
    ))
    .into_any_element()
}

pub(super) fn process_list_group_row(
    data: ProcessListGroupRowData<'_>,
    summary: &ProcessPolicySummary,
    icon: Option<&Arc<Image>>,
    state: ProcessListGroupRowState,
    layout: ProcessListRenderLayout<'_>,
    edit_context: ProcessListEditContext<'_>,
    cx: &mut Context<WinderustApp>,
) -> gpui::Stateful<gpui::Div> {
    let process_name = data.process_name.to_string();
    let row_id = SharedString::from(format!(
        "process-list-group-{}",
        process_list_group_key(&process_name)
    ));
    let toggle_name = process_name.clone();

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
            app.toggle_process_list_group(toggle_name.clone(), cx);
        }))
        .child(process_list_group_name_cell(
            &process_name,
            data.process_count,
            icon,
            state.collapsed,
            layout.column_layout.name_width,
            cx,
        ));

    if process_list_column_visible(layout.hidden_columns, ProcessListColumn::Pid) {
        row = row.child(process_list_text_cell(
            layout.column_layout.column_width(ProcessListColumn::Pid),
            process_list_pid_count_label(data.process_count),
        ));
    }

    row.children(process_list_policy_cells(
        &process_name,
        summary,
        layout,
        true,
        edit_context,
        cx,
    ))
}

pub(super) fn process_list_name_cell(
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

pub(super) fn process_list_group_name_cell(
    process_name: &str,
    process_count: usize,
    icon: Option<&Arc<Image>>,
    collapsed: bool,
    width: f32,
    cx: &mut Context<WinderustApp>,
) -> gpui::Div {
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
                .child(collapsible_chevron_icon(
                    format!(
                        "process-list-group-{}",
                        process_list_group_key(process_name)
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

pub(super) fn process_list_policy_cells(
    process_name: &str,
    summary: &ProcessPolicySummary,
    layout: ProcessListRenderLayout<'_>,
    editable: bool,
    edit_context: ProcessListEditContext<'_>,
    cx: &mut Context<WinderustApp>,
) -> Vec<AnyElement> {
    PROCESS_LIST_OPTIONAL_COLUMNS
        .iter()
        .copied()
        .filter(|column| *column != ProcessListColumn::Pid)
        .filter(|column| process_list_column_visible(layout.hidden_columns, *column))
        .map(|column| {
            process_list_policy_cell(
                layout.column_layout.column_width(column),
                ProcessListPolicyCellTarget {
                    process_name,
                    column,
                    editable,
                },
                process_list_column_value(summary, column),
                summary.uses_custom_rule(column),
                edit_context,
                cx,
            )
        })
        .collect()
}

pub(super) fn process_list_column_value(
    summary: &ProcessPolicySummary,
    column: ProcessListColumn,
) -> SharedString {
    match column {
        ProcessListColumn::Pid => SharedString::new_static(""),
        ProcessListColumn::PowerPlanForeground => summary.power_plan_foreground.clone().into(),
        ProcessListColumn::PowerPlanRunning => summary.power_plan_running.clone().into(),
        ProcessListColumn::BackgroundEfficiency => summary.background_efficiency.clone().into(),
        ProcessListColumn::CoreLimiter => summary.core_limiter.clone().into(),
        ProcessListColumn::BackgroundCpuRestriction => {
            summary.background_cpu_restriction.clone().into()
        }
        ProcessListColumn::CoreSteering => summary.core_steering.clone().into(),
        ProcessListColumn::ProcessPriority => summary.process_priority.clone().into(),
        ProcessListColumn::IoPriority => summary.io_priority.clone().into(),
        ProcessListColumn::GpuPriority => summary.gpu_priority.clone().into(),
        ProcessListColumn::MemoryPriority => summary.memory_priority.clone().into(),
        ProcessListColumn::MemoryTrim => summary.memory_trim.clone().into(),
        ProcessListColumn::AppSuspension => summary.app_suspension.clone().into(),
        ProcessListColumn::TimerResolution => summary.timer_resolution.clone().into(),
    }
}

pub(super) fn process_list_text_cell(width: f32, value: impl Into<SharedString>) -> gpui::Div {
    process_list_text_cell_with_color(width, value, false, muted_text_color())
}

pub(super) fn process_list_text_cell_with_color(
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

pub(super) fn process_list_policy_value_content(
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

pub(super) fn process_list_split_policy_value_row(
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

pub(super) fn process_list_column_uses_split_priority_display(column: ProcessListColumn) -> bool {
    matches!(
        column,
        ProcessListColumn::IoPriority
            | ProcessListColumn::GpuPriority
            | ProcessListColumn::MemoryPriority
    )
}

pub(super) fn process_list_split_policy_value(value: &str) -> Option<(&str, &str)> {
    let (foreground, background) = value.split_once(" / ")?;
    Some((foreground.trim(), background.trim()))
}

pub(super) fn process_list_policy_cell(
    width: f32,
    target: ProcessListPolicyCellTarget<'_>,
    value: impl Into<SharedString>,
    emphasized: bool,
    edit_context: ProcessListEditContext<'_>,
    cx: &mut Context<WinderustApp>,
) -> AnyElement {
    let value = value.into();
    let text_color = if process_list_policy_value_active(value.as_ref(), emphasized) {
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
pub(super) fn process_list_editable_policy_cell(
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

pub(super) fn process_list_column_editable(column: ProcessListColumn) -> bool {
    matches!(
        column,
        ProcessListColumn::PowerPlanForeground
            | ProcessListColumn::PowerPlanRunning
            | ProcessListColumn::BackgroundEfficiency
            | ProcessListColumn::CoreLimiter
            | ProcessListColumn::BackgroundCpuRestriction
            | ProcessListColumn::GpuPriority
            | ProcessListColumn::MemoryTrim
            | ProcessListColumn::AppSuspension
            | ProcessListColumn::TimerResolution
    )
}

pub(super) fn process_list_policy_cell_editable(
    row_editable: bool,
    column: ProcessListColumn,
) -> bool {
    row_editable && process_list_column_editable(column)
}

pub(super) fn process_list_cell_editor_id(process_name: &str, column: ProcessListColumn) -> String {
    format!(
        "process-list-cell-editor-{}-{}",
        process_list_group_key(process_name),
        process_list_sort_column_id(ProcessListSortColumn::Column(column))
    )
}

pub(super) fn process_list_cell_editor_option_count(
    column: ProcessListColumn,
    app: &WinderustApp,
) -> usize {
    match column {
        ProcessListColumn::PowerPlanForeground | ProcessListColumn::PowerPlanRunning => {
            1 + app.plans.len()
        }
        ProcessListColumn::BackgroundEfficiency
        | ProcessListColumn::BackgroundCpuRestriction
        | ProcessListColumn::GpuPriority
        | ProcessListColumn::MemoryTrim
        | ProcessListColumn::AppSuspension => 2,
        ProcessListColumn::CoreLimiter => 5,
        ProcessListColumn::TimerResolution => process_list_timer_resolution_options(app).len(),
        ProcessListColumn::Pid
        | ProcessListColumn::CoreSteering
        | ProcessListColumn::ProcessPriority
        | ProcessListColumn::IoPriority
        | ProcessListColumn::MemoryPriority => 0,
    }
}

pub(super) fn process_list_cell_editor_options(
    process_name: &str,
    column: ProcessListColumn,
    app: &WinderustApp,
    max_height: Pixels,
    cx: &mut Context<WinderustApp>,
) -> Scrollable<gpui::Div> {
    let mut options = dropdown_surface(cx, max_height)
        .w(px(PROCESS_LIST_CELL_EDITOR_WIDTH))
        .min_w(px(PROCESS_LIST_CELL_EDITOR_WIDTH));
    let process_name = process_name.to_owned();

    match column {
        ProcessListColumn::PowerPlanForeground => {
            let selected_guid =
                foreground_power_plan_override_guid(&app.settings.by_foreground, &process_name);
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
            let selected_guid = by_running_app_power_plan_override_guid(
                &app.settings.by_running_app,
                &process_name,
            );
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
        ProcessListColumn::BackgroundCpuRestriction => {
            let included = !app
                .settings
                .background_cpu_restriction
                .exclusion_enabled_for(&process_name);
            options = process_list_include_exclude_editor_options(
                options,
                &process_name,
                column,
                included,
                cx,
            );
        }
        ProcessListColumn::CoreLimiter => {
            let selected = core_limiter_override_percent(&app.settings.core_limiter, &process_name);
            options = options.child(process_list_core_limiter_editor_option(
                &process_name,
                None,
                selected.is_none(),
                cx,
            ));
            for percent in [25_u8, 50, 75, 100] {
                options = options.child(process_list_core_limiter_editor_option(
                    &process_name,
                    Some(percent),
                    selected == Some(percent),
                    cx,
                ));
            }
        }
        ProcessListColumn::GpuPriority => {
            let included = !app
                .settings
                .gpu_priority
                .exclusion_enabled_for(&process_name);
            options = process_list_include_exclude_editor_options(
                options,
                &process_name,
                column,
                included,
                cx,
            );
        }
        ProcessListColumn::MemoryTrim => {
            let included = !app
                .settings
                .memory_trim
                .exclusion_enabled_for(&process_name);
            options = process_list_include_exclude_editor_options(
                options,
                &process_name,
                column,
                included,
                cx,
            );
        }
        ProcessListColumn::AppSuspension => {
            let included = app
                .settings
                .app_suspension
                .suspendable_app_enabled_for(&process_name);
            options = process_list_include_exclude_editor_options(
                options,
                &process_name,
                column,
                included,
                cx,
            );
        }
        ProcessListColumn::TimerResolution => {
            let selected = app
                .settings
                .timer_resolution
                .desired_resolution_for_foreground(&process_name)
                .map(|(_, desired_100ns)| desired_100ns);
            for desired_100ns in process_list_timer_resolution_options(app) {
                options = options.child(process_list_timer_resolution_editor_option(
                    &process_name,
                    desired_100ns,
                    selected == desired_100ns,
                    cx,
                ));
            }
        }
        ProcessListColumn::Pid
        | ProcessListColumn::CoreSteering
        | ProcessListColumn::ProcessPriority
        | ProcessListColumn::IoPriority
        | ProcessListColumn::MemoryPriority => {}
    }

    options
}

pub(super) fn process_list_include_exclude_editor_options(
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

pub(super) fn process_list_editor_option_id(
    process_name: &str,
    column: ProcessListColumn,
    suffix: impl std::fmt::Display,
) -> SharedString {
    SharedString::from(format!(
        "{}-{suffix}",
        process_list_cell_editor_id(process_name, column)
    ))
}

pub(super) fn process_list_power_plan_editor_option(
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

pub(super) fn process_list_include_exclude_editor_option(
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
                ProcessListColumn::BackgroundEfficiency => {
                    app.set_process_list_background_efficiency(process_name.clone(), included, cx)
                }
                ProcessListColumn::BackgroundCpuRestriction => app
                    .set_process_list_background_cpu_restriction(
                        process_name.clone(),
                        included,
                        cx,
                    ),
                ProcessListColumn::GpuPriority => {
                    app.set_process_list_gpu_priority_included(process_name.clone(), included, cx)
                }
                ProcessListColumn::MemoryTrim => {
                    app.set_process_list_memory_trim(process_name.clone(), included, cx)
                }
                ProcessListColumn::AppSuspension => {
                    app.set_process_list_app_suspension(process_name.clone(), included, cx)
                }
                _ => {}
            }
            cx.stop_propagation();
        }))
        .into_any_element()
}

pub(super) fn process_list_core_limiter_editor_option(
    process_name: &str,
    percent: Option<u8>,
    selected: bool,
    cx: &mut Context<WinderustApp>,
) -> AnyElement {
    let process_name = process_name.to_owned();
    let label = percent
        .map(|percent| process_list_include_value_label(format!("{percent}%")))
        .unwrap_or_else(process_list_exclude_label);
    let option_id = process_list_editor_option_id(
        &process_name,
        ProcessListColumn::CoreLimiter,
        percent
            .map(|percent| percent.to_string())
            .unwrap_or_else(|| "exclude".to_owned()),
    );
    dropdown_option_row(option_id, label, selected, cx)
        .on_click(cx.listener(move |app, _, _, cx| {
            app.set_process_list_core_limiter(process_name.clone(), percent, cx);
            cx.stop_propagation();
        }))
        .into_any_element()
}

pub(super) fn process_list_timer_resolution_editor_option(
    process_name: &str,
    desired_100ns: Option<u32>,
    selected: bool,
    cx: &mut Context<WinderustApp>,
) -> AnyElement {
    let process_name = process_name.to_owned();
    let label = desired_100ns
        .map(timer_resolution::format_resolution_ms)
        .unwrap_or_else(process_list_default_label);
    let option_id = process_list_editor_option_id(
        &process_name,
        ProcessListColumn::TimerResolution,
        desired_100ns
            .map(|value| value.to_string())
            .unwrap_or_else(|| "default".to_owned()),
    );
    dropdown_option_row(option_id, label, selected, cx)
        .on_click(cx.listener(move |app, _, _, cx| {
            app.set_process_list_timer_resolution(process_name.clone(), desired_100ns, cx);
            cx.stop_propagation();
        }))
        .into_any_element()
}

pub(super) fn process_list_policy_value_active(value: &str, emphasized: bool) -> bool {
    if let Some(enabled) = process_list_state_enabled(value) {
        return enabled;
    }

    emphasized && !process_list_policy_value_inactive(value)
}

pub(super) fn process_list_policy_value_inactive(value: &str) -> bool {
    let value = value.trim();
    value.is_empty()
        || value == "Default"
        || value == process_list_default_label().as_str()
        || value == "Off"
        || value == "Exclude"
        || value == t!("common.none").to_string().as_str()
}

pub(super) fn process_list_state_enabled(value: &str) -> Option<bool> {
    match value {
        "On" | "Include" => Some(true),
        "Off" | "Exclude" => Some(false),
        value if value.starts_with("Include (") => Some(true),
        _ => None,
    }
}

pub(super) fn process_list_count_label(count: usize) -> String {
    t!("process_list.count", count = count).to_string()
}

pub(super) fn process_list_toolbar_label(app: &WinderustApp, process_count: usize) -> String {
    let Some(process) = app.selected_process_id.and_then(|id| {
        app.running_processes
            .iter()
            .find(|process| process.id == id)
    }) else {
        return process_list_count_label(process_count);
    };
    let custom_count = process_policy_summary(&app.settings, &app.plans, &process.name)
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

pub(super) fn process_list_pid_count_label(count: usize) -> String {
    t!("common.pid_count", count = count).to_string()
}
