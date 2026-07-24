use super::*;

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
