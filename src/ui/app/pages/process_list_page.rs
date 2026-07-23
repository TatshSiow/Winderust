use crate::ui::app::*;

impl WinderustApp {
    pub(in crate::ui::app) fn render_process_list_page(
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

    pub(in crate::ui::app) fn render_process_list_column_visibility_dropdown(
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
pub(in crate::ui::app) enum ProcessListSortColumn {
    ProcessName,
    Column(ProcessListColumn),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::ui::app) enum ProcessListSortDirection {
    Ascending,
    Descending,
}

impl ProcessListSortDirection {
    pub(in crate::ui::app) fn toggled(self) -> Self {
        match self {
            Self::Ascending => Self::Descending,
            Self::Descending => Self::Ascending,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::ui::app) struct ProcessListSort {
    pub(in crate::ui::app) column: ProcessListSortColumn,
    pub(in crate::ui::app) direction: ProcessListSortDirection,
}

impl ProcessListSort {
    pub(in crate::ui::app) fn toggled_for(self, column: ProcessListSortColumn) -> Self {
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

pub(in crate::ui::app) struct ProcessListGroup<'a> {
    pub(in crate::ui::app) display_name: String,
    pub(in crate::ui::app) processes: Vec<&'a ProcessInfo>,
}

#[derive(Clone)]
pub(in crate::ui::app) struct ProcessListColumnLayout {
    pub(in crate::ui::app) name_width: f32,
    pub(in crate::ui::app) column_widths: HashMap<ProcessListColumn, f32>,
}

impl ProcessListColumnLayout {
    pub(in crate::ui::app) fn column_width(&self, column: ProcessListColumn) -> f32 {
        self.column_widths
            .get(&column)
            .copied()
            .unwrap_or_else(|| process_list_column_min_width(column))
    }
}

#[derive(Clone, Copy)]
pub(in crate::ui::app) struct ProcessListRenderLayout<'a> {
    pub(in crate::ui::app) hidden_columns: &'a HashSet<ProcessListColumn>,
    pub(in crate::ui::app) column_layout: &'a ProcessListColumnLayout,
}

#[derive(Clone, Copy)]
pub(in crate::ui::app) struct ProcessListGroupRowState {
    pub(in crate::ui::app) collapsed: bool,
    pub(in crate::ui::app) divided: bool,
}

#[derive(Clone, Copy)]
pub(in crate::ui::app) struct ProcessListEntryRowState {
    pub(in crate::ui::app) divided: bool,
    pub(in crate::ui::app) nested: bool,
    pub(in crate::ui::app) editable: bool,
}

#[derive(Clone, Copy)]
pub(in crate::ui::app) struct ProcessListGroupRowData<'a> {
    pub(in crate::ui::app) process_name: &'a str,
    pub(in crate::ui::app) process_count: usize,
}

#[derive(Clone)]
pub(in crate::ui::app) enum ProcessListRenderedRow {
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

pub(in crate::ui::app) struct ProcessListRenderData {
    pub(in crate::ui::app) process_count: usize,
    pub(in crate::ui::app) column_layout: ProcessListColumnLayout,
    pub(in crate::ui::app) table_width: Pixels,
    pub(in crate::ui::app) rows: Rc<Vec<ProcessListRenderedRow>>,
    pub(in crate::ui::app) item_sizes: Rc<Vec<gpui::Size<Pixels>>>,
}

#[derive(Clone, Copy)]
pub(in crate::ui::app) struct ProcessListEditContext<'a> {
    pub(in crate::ui::app) app: &'a WinderustApp,
    pub(in crate::ui::app) window: &'a Window,
}

#[derive(Clone, Copy)]
pub(in crate::ui::app) struct ProcessListPolicyCellTarget<'a> {
    pub(in crate::ui::app) process_name: &'a str,
    pub(in crate::ui::app) column: ProcessListColumn,
    pub(in crate::ui::app) editable: bool,
}

pub(in crate::ui::app) fn process_list_groups(
    processes: &[ProcessInfo],
) -> Vec<ProcessListGroup<'_>> {
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

pub(in crate::ui::app) fn process_list_sorted_rows<'a>(
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

pub(in crate::ui::app) fn process_list_rendered_rows(
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

pub(in crate::ui::app) fn process_list_render_data(app: &WinderustApp) -> ProcessListRenderData {
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

pub(in crate::ui::app) fn process_list_sort_group_processes(
    group: &mut ProcessListGroup<'_>,
    sort: ProcessListSort,
) {
    group
        .processes
        .sort_by(|left, right| process_list_process_sort_cmp(left, right, sort));
}

pub(in crate::ui::app) fn process_list_group_sort_cmp(
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

pub(in crate::ui::app) fn process_list_process_sort_cmp(
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

pub(in crate::ui::app) fn process_list_directional_cmp(
    ordering: CmpOrdering,
    direction: ProcessListSortDirection,
) -> CmpOrdering {
    match direction {
        ProcessListSortDirection::Ascending => ordering,
        ProcessListSortDirection::Descending => ordering.reverse(),
    }
}

pub(in crate::ui::app) fn process_list_group_min_pid(group: &ProcessListGroup<'_>) -> u32 {
    group
        .processes
        .iter()
        .map(|process| process.id)
        .min()
        .unwrap_or_default()
}

pub(in crate::ui::app) fn process_list_group_sort_pid(
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

pub(in crate::ui::app) fn process_list_text_sort_cmp(left: &str, right: &str) -> CmpOrdering {
    left.bytes()
        .map(|byte| byte.to_ascii_lowercase())
        .cmp(right.bytes().map(|byte| byte.to_ascii_lowercase()))
        .then_with(|| left.cmp(right))
}

pub(in crate::ui::app) fn process_list_group_key(process_name: &str) -> String {
    process_name.trim().to_ascii_lowercase()
}

pub(in crate::ui::app) fn process_list_column_visible(
    hidden_columns: &HashSet<ProcessListColumn>,
    column: ProcessListColumn,
) -> bool {
    !hidden_columns.contains(&column)
}

pub(in crate::ui::app) fn process_list_column_min_width(column: ProcessListColumn) -> f32 {
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

pub(in crate::ui::app) fn process_list_column_max_width(column: ProcessListColumn) -> f32 {
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

pub(in crate::ui::app) fn process_list_column_label(
    column: ProcessListColumn,
    settings: &Settings,
) -> String {
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

pub(in crate::ui::app) fn process_list_column_layout(
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

pub(in crate::ui::app) fn process_list_estimated_cell_width(text: &str, extra_width: f32) -> f32 {
    process_list_estimated_text_width(text) + extra_width
}

pub(in crate::ui::app) fn process_list_estimated_policy_value_width(
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

pub(in crate::ui::app) fn process_list_header_cell_non_text_width() -> f32 {
    PROCESS_LIST_TEXT_CELL_HORIZONTAL_PADDING
        + PROCESS_LIST_SORT_ICON_WIDTH
        + PROCESS_LIST_SORT_HEADER_GAP
}

pub(in crate::ui::app) fn process_list_estimated_text_width(text: &str) -> f32 {
    text.chars().map(process_list_estimated_char_width).sum()
}

pub(in crate::ui::app) fn process_list_estimated_char_width(character: char) -> f32 {
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

pub(in crate::ui::app) fn process_list_table_width(
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

pub(in crate::ui::app) fn process_list_sort_column_id(
    column: ProcessListSortColumn,
) -> &'static str {
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

pub(in crate::ui::app) fn process_list_column_visibility_dropdown_options(
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

pub(in crate::ui::app) fn process_list_group_row(
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

pub(in crate::ui::app) fn process_list_policy_cells(
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

pub(in crate::ui::app) fn process_list_column_value(
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
            | ProcessListColumn::BackgroundEfficiency
            | ProcessListColumn::CoreLimiter
            | ProcessListColumn::BackgroundCpuRestriction
            | ProcessListColumn::GpuPriority
            | ProcessListColumn::MemoryTrim
            | ProcessListColumn::AppSuspension
            | ProcessListColumn::TimerResolution
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
        process_list_group_key(process_name),
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

pub(in crate::ui::app) fn process_list_cell_editor_options(
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

pub(in crate::ui::app) fn process_list_core_limiter_editor_option(
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

pub(in crate::ui::app) fn process_list_timer_resolution_editor_option(
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

pub(in crate::ui::app) fn process_list_policy_value_active(value: &str, emphasized: bool) -> bool {
    if let Some(enabled) = process_list_state_enabled(value) {
        return enabled;
    }

    emphasized && !process_list_policy_value_inactive(value)
}

pub(in crate::ui::app) fn process_list_policy_value_inactive(value: &str) -> bool {
    let value = value.trim();
    value.is_empty()
        || value == "Default"
        || value == process_list_default_label().as_str()
        || value == "Off"
        || value == "Exclude"
        || value == t!("common.none").to_string().as_str()
}

pub(in crate::ui::app) fn process_list_state_enabled(value: &str) -> Option<bool> {
    match value {
        "On" | "Include" => Some(true),
        "Off" | "Exclude" => Some(false),
        value if value.starts_with("Include (") => Some(true),
        _ => None,
    }
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

pub(in crate::ui::app) fn process_list_pid_count_label(count: usize) -> String {
    t!("common.pid_count", count = count).to_string()
}

impl WinderustApp {
    fn is_process_list_group_collapsed(&self, process_name: &str) -> bool {
        !self
            .expanded_process_list_groups
            .contains(&process_list_group_key(process_name))
    }

    fn toggle_process_list_group(&mut self, process_name: String, cx: &mut Context<Self>) {
        let key = process_list_group_key(&process_name);
        let expanded = if self.expanded_process_list_groups.remove(&key) {
            false
        } else {
            self.expanded_process_list_groups.insert(key.clone());
            true
        };
        begin_expandable_motion(format!("process-list-group-{key}"), expanded);
        cx.notify();
    }

    fn set_process_list_column_visible(
        &mut self,
        column: ProcessListColumn,
        visible: bool,
        cx: &mut Context<Self>,
    ) {
        let changed = if visible {
            self.hidden_process_list_columns.remove(&column)
        } else {
            self.hidden_process_list_columns.insert(column)
        };

        let sort_changed =
            !visible && self.process_list_sort.column == ProcessListSortColumn::Column(column);
        if sort_changed {
            self.process_list_sort = ProcessListSort::default();
        }

        if changed || sort_changed {
            cx.notify();
        }
    }

    fn toggle_process_list_sort(&mut self, column: ProcessListSortColumn, cx: &mut Context<Self>) {
        self.process_list_sort = self.process_list_sort.toggled_for(column);
        cx.notify();
    }

    fn finish_process_list_edit(&mut self, cx: &mut Context<Self>) {
        self.active_power_plan_picker = None;
        cx.notify();
    }

    fn set_process_list_foreground_power_plan(
        &mut self,
        process_name: String,
        power_plan_guid: Option<String>,
        cx: &mut Context<Self>,
    ) {
        set_foreground_power_plan_override(
            &mut self.settings.by_foreground,
            &process_name,
            power_plan_guid,
        );
        self.finish_process_list_edit(cx);
    }

    fn set_process_list_running_power_plan(
        &mut self,
        process_name: String,
        power_plan_guid: Option<String>,
        cx: &mut Context<Self>,
    ) {
        set_by_running_app_power_plan_override(
            &mut self.settings.by_running_app,
            &process_name,
            power_plan_guid,
        );
        self.finish_process_list_edit(cx);
    }

    fn set_process_list_background_efficiency(
        &mut self,
        process_name: String,
        included: bool,
        cx: &mut Context<Self>,
    ) {
        set_background_efficiency_custom_rule(
            &mut self.settings.background_efficiency,
            &process_name,
            !included,
        );
        self.finish_process_list_edit(cx);
    }

    fn set_process_list_background_cpu_restriction(
        &mut self,
        process_name: String,
        included: bool,
        cx: &mut Context<Self>,
    ) {
        set_process_exclusion(
            &mut self.settings.background_cpu_restriction.exclusions,
            &process_name,
            !included,
        );
        self.finish_process_list_edit(cx);
    }

    fn set_process_list_core_limiter(
        &mut self,
        process_name: String,
        max_logical_processors: Option<u8>,
        cx: &mut Context<Self>,
    ) {
        set_core_limiter_override(
            &mut self.settings.core_limiter,
            &process_name,
            max_logical_processors,
        );
        self.finish_process_list_edit(cx);
    }

    fn set_process_list_gpu_priority_included(
        &mut self,
        process_name: String,
        included: bool,
        cx: &mut Context<Self>,
    ) {
        set_process_exclusion(
            &mut self.settings.gpu_priority.exclusions,
            &process_name,
            !included,
        );
        self.finish_process_list_edit(cx);
    }

    fn set_process_list_memory_trim(
        &mut self,
        process_name: String,
        included: bool,
        cx: &mut Context<Self>,
    ) {
        set_process_exclusion(
            &mut self.settings.memory_trim.exclusions,
            &process_name,
            !included,
        );
        self.finish_process_list_edit(cx);
    }

    fn set_process_list_app_suspension(
        &mut self,
        process_name: String,
        included: bool,
        cx: &mut Context<Self>,
    ) {
        set_app_suspension_override(&mut self.settings.app_suspension, &process_name, included);
        self.finish_process_list_edit(cx);
    }

    fn set_process_list_timer_resolution(
        &mut self,
        process_name: String,
        desired_100ns: Option<u32>,
        cx: &mut Context<Self>,
    ) {
        set_timer_resolution_override(
            &mut self.settings.timer_resolution,
            &process_name,
            desired_100ns,
        );
        self.finish_process_list_edit(cx);
    }

    fn apply_process_priority_once(
        &mut self,
        target: Result<ProcessActionTarget, ProcessActionTargetError>,
        process_name: &str,
        priority: ProcessPrioritySetting,
        cx: &mut Context<Self>,
    ) {
        self.status_message = match target
            .map_err(|error| error.to_string())
            .and_then(|target| process_priority::apply_once(&target, priority))
        {
            Ok(priority) => t!(
                "process_list.applied_once",
                name = process_name,
                priority = priority
            )
            .to_string(),
            Err(error) => t!(
                "process_list.apply_once_failed",
                name = process_name,
                error = error
            )
            .to_string(),
        };
        cx.notify();
    }

    fn save_process_priority_rule(
        &mut self,
        process_name: &str,
        priority: ProcessPrioritySetting,
        cx: &mut Context<Self>,
    ) {
        set_process_priority_rule(&mut self.settings.process_priority, process_name, priority);
        if self.save_settings() {
            let key = if self.settings.process_priority.enabled {
                "process_list.saved_priority_rule"
            } else {
                "process_list.saved_priority_rule_disabled"
            };
            self.status_message = t!(
                key,
                name = process_name,
                priority = process_priority_setting_label(priority)
            )
            .to_string();
        }
        cx.notify();
    }

    fn apply_memory_priority_once(
        &mut self,
        target: Result<ProcessActionTarget, ProcessActionTargetError>,
        process_name: &str,
        priority: ProcessMemoryPrioritySetting,
        cx: &mut Context<Self>,
    ) {
        self.status_message = match target
            .map_err(|error| error.to_string())
            .and_then(|target| memory_priority::apply_once(&target, priority))
        {
            Ok(priority) => t!(
                "process_list.applied_memory_once",
                name = process_name,
                priority = priority
            )
            .to_string(),
            Err(error) => t!(
                "process_list.apply_memory_once_failed",
                name = process_name,
                error = error
            )
            .to_string(),
        };
        cx.notify();
    }

    fn save_memory_priority_rule(
        &mut self,
        process_name: &str,
        priority: ProcessMemoryPrioritySetting,
        cx: &mut Context<Self>,
    ) {
        set_memory_priority_rule(&mut self.settings.memory_priority, process_name, priority);
        if self.save_settings() {
            let key = if self.settings.memory_priority.enabled {
                "process_list.saved_memory_rule"
            } else {
                "process_list.saved_memory_rule_disabled"
            };
            self.status_message = t!(
                key,
                name = process_name,
                priority = process_memory_priority_setting_label(priority)
            )
            .to_string();
        }
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn process_list_column_layout_fits_headers_and_values() {
        let settings = Settings::default();
        let processes = vec![
            ProcessInfo {
                id: 1234,
                parent_id: None,
                name: "editor.exe".to_owned(),
            },
            ProcessInfo {
                id: 12345,
                parent_id: None,
                name: "worker.exe".to_owned(),
            },
        ];
        let groups = process_list_groups(&processes);
        let summaries = groups
            .iter()
            .map(|_| default_process_policy_summary())
            .collect::<Vec<_>>();

        let layout = process_list_column_layout(&settings, &groups, &summaries);

        assert!(layout.column_width(ProcessListColumn::Pid) < PROCESS_LIST_PID_MAX_WIDTH);
        assert!(layout.column_width(ProcessListColumn::MemoryTrim) < 140.0);
        assert!(
            layout.column_width(ProcessListColumn::PowerPlanForeground)
                >= process_list_estimated_cell_width(
                    &process_list_column_label(ProcessListColumn::PowerPlanForeground, &settings),
                    PROCESS_LIST_TEXT_CELL_HORIZONTAL_PADDING,
                )
        );
    }

    #[test]
    fn process_icon_cache_drops_stale_paths() {
        let kept_path = PathBuf::from("C:\\Apps\\kept.exe");
        let stale_path = PathBuf::from("C:\\Apps\\stale.exe");
        let mut cache = HashMap::from([(kept_path.clone(), None), (stale_path.clone(), None)]);
        let candidates = vec![ProcessCandidate {
            name: "kept.exe".to_owned(),
            image_path: Some(kept_path.clone()),
            icon: None,
        }];

        WinderustApp::retain_current_process_icons(&mut cache, &candidates);

        assert!(cache.contains_key(&kept_path));
        assert!(!cache.contains_key(&stale_path));
    }

    #[test]
    fn process_list_sort_orders_groups_by_name_direction() {
        let processes = vec![
            ProcessInfo {
                id: 1,
                parent_id: None,
                name: "editor.exe".to_owned(),
            },
            ProcessInfo {
                id: 2,
                parent_id: None,
                name: "worker.exe".to_owned(),
            },
        ];
        let groups = process_list_groups(&processes);
        let summaries = groups
            .iter()
            .map(|_| default_process_policy_summary())
            .collect::<Vec<_>>();
        let rows = process_list_sorted_rows(
            groups,
            summaries,
            ProcessListSort {
                column: ProcessListSortColumn::ProcessName,
                direction: ProcessListSortDirection::Descending,
            },
        );

        assert_eq!(rows[0].0.display_name, "worker.exe");
        assert_eq!(rows[1].0.display_name, "editor.exe");
    }

    #[test]
    fn process_list_text_sort_cmp_matches_ascii_lowercase_sorting() {
        for (left, right) in [
            ("Alpha.exe", "alpha.exe"),
            ("worker.exe", "Editor.exe"),
            ("z.exe", "é.exe"),
        ] {
            let expected = left
                .to_ascii_lowercase()
                .cmp(&right.to_ascii_lowercase())
                .then_with(|| left.cmp(right));
            assert_eq!(process_list_text_sort_cmp(left, right), expected);
        }
    }

    #[test]
    fn process_list_sort_orders_groups_and_children_by_pid() {
        let processes = vec![
            ProcessInfo {
                id: 30,
                parent_id: None,
                name: "editor.exe".to_owned(),
            },
            ProcessInfo {
                id: 10,
                parent_id: None,
                name: "worker.exe".to_owned(),
            },
            ProcessInfo {
                id: 20,
                parent_id: None,
                name: "editor.exe".to_owned(),
            },
        ];
        let sort = ProcessListSort {
            column: ProcessListSortColumn::Column(ProcessListColumn::Pid),
            direction: ProcessListSortDirection::Ascending,
        };
        let mut groups = process_list_groups(&processes);
        for group in &mut groups {
            process_list_sort_group_processes(group, sort);
        }
        let summaries = groups
            .iter()
            .map(|_| default_process_policy_summary())
            .collect::<Vec<_>>();
        let rows = process_list_sorted_rows(groups, summaries, sort);

        assert_eq!(rows[0].0.display_name, "worker.exe");
        assert_eq!(rows[1].0.display_name, "editor.exe");
        assert_eq!(rows[1].0.processes[0].id, 20);
        assert_eq!(rows[1].0.processes[1].id, 30);

        let sort = ProcessListSort {
            column: ProcessListSortColumn::Column(ProcessListColumn::Pid),
            direction: ProcessListSortDirection::Descending,
        };
        let mut groups = process_list_groups(&processes);
        for group in &mut groups {
            process_list_sort_group_processes(group, sort);
        }
        let summaries = groups
            .iter()
            .map(|_| default_process_policy_summary())
            .collect::<Vec<_>>();
        let rows = process_list_sorted_rows(groups, summaries, sort);

        assert_eq!(rows[0].0.display_name, "editor.exe");
        assert_eq!(rows[0].0.processes[0].id, 30);
        assert_eq!(rows[0].0.processes[1].id, 20);
        assert_eq!(rows[1].0.display_name, "worker.exe");
    }

    #[test]
    fn process_list_sort_orders_groups_by_policy_column_value() {
        let processes = vec![
            ProcessInfo {
                id: 1,
                parent_id: None,
                name: "editor.exe".to_owned(),
            },
            ProcessInfo {
                id: 2,
                parent_id: None,
                name: "worker.exe".to_owned(),
            },
        ];
        let groups = process_list_groups(&processes);
        let mut low = default_process_policy_summary();
        low.process_priority = "Idle".to_owned();
        let mut high = default_process_policy_summary();
        high.process_priority = "Normal".to_owned();
        let rows = process_list_sorted_rows(
            groups,
            vec![high, low],
            ProcessListSort {
                column: ProcessListSortColumn::Column(ProcessListColumn::ProcessPriority),
                direction: ProcessListSortDirection::Ascending,
            },
        );

        assert_eq!(rows[0].0.display_name, "worker.exe");
        assert_eq!(rows[1].0.display_name, "editor.exe");
    }

    #[test]
    fn process_list_policy_value_active_tracks_state_and_custom_values() {
        assert!(process_list_policy_value_active("Include", false));
        assert!(process_list_policy_value_active("Include (50%)", false));
        assert!(!process_list_policy_value_active("Exclude", true));
        assert!(!process_list_policy_value_active(
            process_list_default_label().as_str(),
            true
        ));
        assert!(!process_list_policy_value_active("Balanced", false));
        assert!(process_list_policy_value_active("Balanced", true));
    }

    #[test]
    fn process_list_split_policy_value_parses_foreground_background_pairs() {
        assert_eq!(
            process_list_split_policy_value("Normal / Very low"),
            Some(("Normal", "Very low"))
        );
        assert_eq!(
            process_list_split_policy_value("  Above normal / Idle  "),
            Some(("Above normal", "Idle"))
        );
        assert_eq!(process_list_split_policy_value("Default"), None);
    }

    #[test]
    fn process_list_policy_cell_editing_respects_row_editability() {
        assert!(!process_list_policy_cell_editable(
            true,
            ProcessListColumn::ProcessPriority
        ));
        assert!(!process_list_policy_cell_editable(
            false,
            ProcessListColumn::ProcessPriority
        ));
        assert!(!process_list_policy_cell_editable(
            true,
            ProcessListColumn::CoreSteering
        ));
    }

    #[test]
    fn process_policy_summary_matches_exact_process_rule() {
        let mut settings = Settings::default();
        settings.core_steering.enabled = true;
        settings.core_steering.rules.push(CoreSteeringRule {
            enabled: true,
            mode: CoreSteeringMode::Soft,
            process_name: "Editor.EXE".to_owned(),
            core_mask: 0b1011,
        });

        let matching = process_policy_summary(&settings, &[], "editor.exe");
        assert_eq!(matching.core_steering, "0-1, 3");
        assert!(matching.uses_custom_rule(ProcessListColumn::CoreSteering));

        let non_matching = process_policy_summary(&settings, &[], "browser.exe");
        assert_eq!(
            non_matching.power_plan_foreground,
            process_list_default_label()
        );
        assert_eq!(
            non_matching.power_plan_running,
            process_list_default_label()
        );
        assert_eq!(non_matching.core_steering, default_core_steering_label());
        assert_eq!(
            non_matching.process_priority,
            process_priority_setting_label(ProcessPrioritySetting::Default)
        );
        assert!(!non_matching.uses_custom_rule(ProcessListColumn::CoreSteering));
    }

    #[test]
    fn process_policy_summary_reports_priority_policy_values() {
        let mut settings = Settings::default();
        settings.io_priority.enabled = true;
        settings.gpu_priority.enabled = true;
        settings.memory_priority.enabled = true;

        let summary = process_policy_summary(&settings, &[], "editor.exe");

        assert_eq!(
            summary.io_priority,
            io_priority_policy_label(&settings.io_priority)
        );
        assert_eq!(
            summary.gpu_priority,
            gpu_priority_policy_label(&settings.gpu_priority)
        );
        assert_eq!(
            summary.memory_priority,
            memory_priority_policy_label(&settings.memory_priority)
        );
    }

    #[test]
    fn process_policy_summary_reports_process_rule_columns() {
        let mut settings = Settings::default();
        settings.by_foreground.enabled = true;
        settings.by_foreground.rules.push(ByForegroundRule {
            enabled: true,
            name: "Editor".to_owned(),
            process_name: "editor.exe".to_owned(),
            power_plan_guid: Some("balanced-guid".to_owned()),
        });
        settings.by_running_app.enabled = true;
        settings.by_running_app.rules.push(ByRunningAppRule {
            enabled: true,
            name: "Editor".to_owned(),
            process_name: "editor.exe".to_owned(),
            power_plan_guid: Some("performance-guid".to_owned()),
        });
        settings.core_limiter.enabled = true;
        settings.core_limiter.rules.push(CoreLimiterRule {
            enabled: true,
            process_name: "editor.exe".to_owned(),
            threshold_percent: 80,
            sustain_seconds: 5,
            cooldown_seconds: 30,
            max_logical_processors: 50,
        });
        settings.app_suspension.enabled = true;
        settings
            .app_suspension
            .suspendable_apps
            .push(AppSuspensionRule {
                enabled: true,
                process_name: "editor.exe".to_owned(),
                network_wake_enabled: true,
                audio_wake_enabled: true,
                network_download_threshold_bytes: 1,
                network_download_threshold_unit: NetworkThresholdUnit::Bytes,
                network_upload_threshold_bytes: 0,
                network_upload_threshold_unit: NetworkThresholdUnit::Bytes,
            });
        settings.timer_resolution.enabled = true;
        settings.timer_resolution.rules.push(TimerResolutionRule {
            enabled: true,
            process_name: "editor.exe".to_owned(),
            desired_100ns: 10_000,
        });
        let plans = vec![
            PowerPlan {
                guid: "balanced-guid".to_owned(),
                name: "Balanced".to_owned(),
                active: false,
            },
            PowerPlan {
                guid: "performance-guid".to_owned(),
                name: "Performance".to_owned(),
                active: false,
            },
        ];

        let summary = process_policy_summary(&settings, &plans, "editor.exe");

        assert_eq!(summary.power_plan_foreground, "Balanced");
        assert_eq!(summary.power_plan_running, "Performance");
        assert_eq!(
            summary.core_limiter,
            process_list_include_value_label("50%")
        );
        assert_eq!(summary.app_suspension, process_list_include_label());
        assert_eq!(summary.timer_resolution, "1.00 ms");
        assert!(summary.uses_custom_rule(ProcessListColumn::PowerPlanForeground));
        assert!(summary.uses_custom_rule(ProcessListColumn::PowerPlanRunning));
        assert!(summary.uses_custom_rule(ProcessListColumn::CoreLimiter));
        assert!(summary.uses_custom_rule(ProcessListColumn::AppSuspension));
        assert!(summary.uses_custom_rule(ProcessListColumn::TimerResolution));
    }

    #[test]
    fn process_policy_summary_reports_include_exclude_columns() {
        let mut settings = Settings::default();
        settings
            .background_efficiency
            .custom_rules
            .push(new_background_efficiency_rule("editor.exe"));
        settings
            .memory_trim
            .exclusions
            .push(new_process_exclusion_rule("editor.exe"));
        settings
            .app_suspension
            .suspendable_apps
            .push(AppSuspensionRule {
                enabled: true,
                process_name: "editor.exe".to_owned(),
                network_wake_enabled: true,
                audio_wake_enabled: true,
                network_download_threshold_bytes: 1,
                network_download_threshold_unit: NetworkThresholdUnit::Bytes,
                network_upload_threshold_bytes: 0,
                network_upload_threshold_unit: NetworkThresholdUnit::Bytes,
            });

        let summary = process_policy_summary(&settings, &[], "editor.exe");

        assert_eq!(summary.background_efficiency, process_list_exclude_label());
        assert_eq!(summary.core_limiter, process_list_exclude_label());
        assert_eq!(summary.memory_trim, process_list_exclude_label());
        assert_eq!(summary.app_suspension, process_list_include_label());
        assert_eq!(summary.timer_resolution, process_list_default_label());
        assert!(summary.uses_custom_rule(ProcessListColumn::BackgroundEfficiency));
        assert!(!summary.uses_custom_rule(ProcessListColumn::CoreLimiter));
        assert!(summary.uses_custom_rule(ProcessListColumn::MemoryTrim));
        assert!(summary.uses_custom_rule(ProcessListColumn::AppSuspension));
        assert!(!summary.uses_custom_rule(ProcessListColumn::TimerResolution));
    }

    #[test]
    fn process_policy_summary_reports_priority_exclusions_as_exclude() {
        let mut settings = Settings::default();
        settings
            .io_priority
            .exclusions
            .push(new_process_exclusion_rule("editor.exe"));
        settings
            .gpu_priority
            .exclusions
            .push(new_process_exclusion_rule("editor.exe"));
        settings
            .memory_priority
            .exclusions
            .push(new_process_exclusion_rule("editor.exe"));

        let summary = process_policy_summary(&settings, &[], "editor.exe");

        assert_eq!(summary.io_priority, process_list_exclude_label());
        assert_eq!(summary.gpu_priority, process_list_exclude_label());
        assert_eq!(summary.memory_priority, process_list_exclude_label());
        assert!(summary.uses_custom_rule(ProcessListColumn::IoPriority));
        assert!(summary.uses_custom_rule(ProcessListColumn::GpuPriority));
        assert!(summary.uses_custom_rule(ProcessListColumn::MemoryPriority));
    }

    #[test]
    fn process_list_rule_edit_helpers_update_process_overrides() {
        let mut settings = Settings::default();

        set_foreground_power_plan_override(
            &mut settings.by_foreground,
            "Editor.EXE",
            Some("balanced-guid".to_owned()),
        );
        let summary = process_policy_summary(&settings, &[], "editor.exe");
        assert_eq!(summary.power_plan_foreground, "balanced-guid");
        assert!(summary.uses_custom_rule(ProcessListColumn::PowerPlanForeground));

        set_foreground_power_plan_override(&mut settings.by_foreground, "editor.exe", None);
        let summary = process_policy_summary(&settings, &[], "editor.exe");
        assert_eq!(summary.power_plan_foreground, process_list_default_label());
        assert!(!summary.uses_custom_rule(ProcessListColumn::PowerPlanForeground));

        set_core_limiter_override(&mut settings.core_limiter, "editor.exe", Some(50));
        let summary = process_policy_summary(&settings, &[], "editor.exe");
        assert_eq!(
            summary.core_limiter,
            process_list_include_value_label("50%")
        );
        assert!(summary.uses_custom_rule(ProcessListColumn::CoreLimiter));

        set_core_limiter_override(&mut settings.core_limiter, "editor.exe", None);
        let summary = process_policy_summary(&settings, &[], "editor.exe");
        assert_eq!(summary.core_limiter, process_list_exclude_label());
        assert!(!summary.uses_custom_rule(ProcessListColumn::CoreLimiter));
    }

    #[test]
    fn process_list_rule_edit_helpers_update_timer_overrides() {
        let mut settings = Settings::default();

        set_timer_resolution_override(&mut settings.timer_resolution, "editor.exe", Some(20_000));
        let summary = process_policy_summary(&settings, &[], "editor.exe");
        assert_eq!(
            summary.timer_resolution,
            timer_resolution::format_resolution_ms(20_000)
        );
        assert!(summary.uses_custom_rule(ProcessListColumn::TimerResolution));

        set_timer_resolution_override(&mut settings.timer_resolution, "editor.exe", None);
        let summary = process_policy_summary(&settings, &[], "editor.exe");
        assert_eq!(summary.timer_resolution, process_list_default_label());
        assert!(!summary.uses_custom_rule(ProcessListColumn::TimerResolution));
    }

    #[test]
    fn process_policy_summary_reports_default_power_plan_when_unset() {
        let mut settings = Settings::default();
        settings.by_foreground.enabled = true;
        settings.by_foreground.rules.push(ByForegroundRule {
            enabled: true,
            name: "Editor".to_owned(),
            process_name: "editor.exe".to_owned(),
            power_plan_guid: None,
        });
        settings.by_running_app.enabled = true;
        settings.by_running_app.rules.push(ByRunningAppRule {
            enabled: true,
            name: "Editor".to_owned(),
            process_name: "editor.exe".to_owned(),
            power_plan_guid: None,
        });

        let summary = process_policy_summary(&settings, &[], "editor.exe");

        assert_eq!(summary.power_plan_foreground, process_list_default_label());
        assert_eq!(summary.power_plan_running, process_list_default_label());
        assert_eq!(summary.timer_resolution, process_list_default_label());
        assert!(summary.uses_custom_rule(ProcessListColumn::PowerPlanForeground));
        assert!(summary.uses_custom_rule(ProcessListColumn::PowerPlanRunning));
        assert!(!summary.uses_custom_rule(ProcessListColumn::TimerResolution));
    }

    #[test]
    fn process_policy_summary_reports_configured_rules_when_feature_disabled() {
        let mut settings = Settings::default();
        settings.by_foreground.enabled = false;
        settings.by_foreground.rules.push(ByForegroundRule {
            enabled: true,
            name: "Editor".to_owned(),
            process_name: "editor.exe".to_owned(),
            power_plan_guid: Some("balanced-guid".to_owned()),
        });
        settings.by_running_app.enabled = false;
        settings.by_running_app.rules.push(ByRunningAppRule {
            enabled: true,
            name: "Editor".to_owned(),
            process_name: "editor.exe".to_owned(),
            power_plan_guid: Some("performance-guid".to_owned()),
        });
        settings.core_limiter.enabled = false;
        settings.core_limiter.rules.push(CoreLimiterRule {
            enabled: true,
            process_name: "editor.exe".to_owned(),
            threshold_percent: 80,
            sustain_seconds: 5,
            cooldown_seconds: 30,
            max_logical_processors: 25,
        });
        settings.timer_resolution.enabled = false;
        settings.timer_resolution.rules.push(TimerResolutionRule {
            enabled: true,
            process_name: "editor.exe".to_owned(),
            desired_100ns: 10_000,
        });
        let plans = vec![
            PowerPlan {
                guid: "balanced-guid".to_owned(),
                name: "Balanced".to_owned(),
                active: false,
            },
            PowerPlan {
                guid: "performance-guid".to_owned(),
                name: "Performance".to_owned(),
                active: false,
            },
        ];

        let summary = process_policy_summary(&settings, &plans, "editor.exe");

        assert_eq!(summary.power_plan_foreground, "Balanced");
        assert_eq!(summary.power_plan_running, "Performance");
        assert_eq!(
            summary.core_limiter,
            process_list_include_value_label("25%")
        );
        assert_eq!(summary.timer_resolution, "1.00 ms");
        assert!(summary.uses_custom_rule(ProcessListColumn::PowerPlanForeground));
        assert!(summary.uses_custom_rule(ProcessListColumn::PowerPlanRunning));
        assert!(summary.uses_custom_rule(ProcessListColumn::CoreLimiter));
        assert!(summary.uses_custom_rule(ProcessListColumn::TimerResolution));
    }

    #[test]
    fn cpu_mask_formatter_uses_ranges() {
        assert_eq!(format_cpu_mask(0), t!("common.none").to_string());
        assert_eq!(format_cpu_mask(0b1111), "0-3");
        assert_eq!(format_cpu_mask(0b101101), "0, 2-3, 5");
    }

    #[test]
    fn no_smt_mask_selects_one_logical_cpu_per_physical_core() {
        let processors = vec![
            LogicalProcessorInfo {
                index: 0,
                core_index: 0,
                kind: LogicalProcessorKind::Standard,
                efficiency_class: 0,
            },
            LogicalProcessorInfo {
                index: 1,
                core_index: 0,
                kind: LogicalProcessorKind::Standard,
                efficiency_class: 0,
            },
            LogicalProcessorInfo {
                index: 2,
                core_index: 1,
                kind: LogicalProcessorKind::Standard,
                efficiency_class: 0,
            },
            LogicalProcessorInfo {
                index: 3,
                core_index: 1,
                kind: LogicalProcessorKind::Standard,
                efficiency_class: 0,
            },
        ];

        assert_eq!(core_steering_processors_no_smt_mask(&processors), 0b0101);
    }

    #[test]
    fn topology_aware_core_toggle_keeps_one_available_cpu_selected() {
        let mut mask = (1_u64 << 63) | 0b0001;
        toggle_affinity_core_with_available_mask(&mut mask, 0, 0b0011);

        assert_eq!(mask, 0b0001);

        toggle_affinity_core_with_available_mask(&mut mask, 1, 0b0011);
        assert_eq!(mask, 0b0011);

        toggle_affinity_core_with_available_mask(&mut mask, 0, 0b0011);
        assert_eq!(mask, 0b0010);
    }

    #[test]
    fn new_core_steering_rules_default_to_soft_cpu_sets() {
        let rule = new_core_steering_rule("game.exe");

        assert_eq!(rule.mode, CoreSteeringMode::Soft);
    }
}
