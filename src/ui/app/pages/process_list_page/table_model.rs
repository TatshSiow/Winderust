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
                direction: if matches!(
                    column,
                    ProcessListSortColumn::Column(
                        ProcessListColumn::CpuUsage | ProcessListColumn::MemoryUsage
                    )
                ) {
                    ProcessListSortDirection::Descending
                } else {
                    ProcessListSortDirection::Ascending
                },
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
    pub(in crate::ui::app) executable_path: String,
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
    pub(in crate::ui::app) process_id: u32,
    pub(in crate::ui::app) process_name: &'a str,
    pub(in crate::ui::app) executable_path: &'a str,
    pub(in crate::ui::app) process_count: usize,
    pub(in crate::ui::app) user_label: &'a str,
    pub(in crate::ui::app) user_unavailable: bool,
    pub(in crate::ui::app) protected: bool,
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
        process_id: u32,
        process_name: String,
        executable_path: String,
        process_count: usize,
        user_label: String,
        user_unavailable: bool,
        protected: bool,
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
    pub(in crate::ui::app) active: bool,
}

pub(in crate::ui::app) fn process_list_groups(
    processes: &[ProcessInfo],
) -> Vec<ProcessListGroup<'_>> {
    let mut groups = Vec::<ProcessListGroup<'_>>::with_capacity(processes.len());
    let mut group_indexes = HashMap::<String, usize>::with_capacity(processes.len());

    for process in processes {
        let (executable_path, key) = match process.image_path.as_deref() {
            Some(path) => (
                executable_path_key(path),
                process_list_executable_path_group_key(path),
            ),
            None => {
                let executable_path = format!("{}#{}", process.name, process.id);
                let key = process_list_group_key(&executable_path);
                (executable_path, key)
            }
        };
        if let Some(index) = group_indexes.get(&key).copied() {
            groups[index].processes.push(process);
        } else {
            group_indexes.insert(key, groups.len());
            groups.push(ProcessListGroup {
                display_name: process.name.clone(),
                executable_path,
                processes: vec![process],
            });
        }
    }

    let mut name_counts = HashMap::new();
    for group in &groups {
        *name_counts
            .entry(group.display_name.to_ascii_lowercase())
            .or_insert(0usize) += 1;
    }
    for group in &mut groups {
        if name_counts
            .get(&group.display_name.to_ascii_lowercase())
            .is_some_and(|count| *count > 1)
        {
            if let Some(parent) = Path::new(&group.executable_path).parent() {
                group.display_name = format!("{} — {}", group.display_name, parent.display());
            }
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
    process_icons_by_path: &HashMap<String, &Arc<Image>>,
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
        let icon = process_icons_by_path
            .get(&process_list_executable_path_group_key(Path::new(
                &group.executable_path,
            )))
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

        let collapsed = is_group_collapsed(&group.executable_path);
        let process_id = group
            .processes
            .iter()
            .filter(|candidate| {
                !group
                    .processes
                    .iter()
                    .any(|process| Some(process.id) == candidate.parent_id)
            })
            .map(|process| process.id)
            .min()
            .unwrap_or(group.processes[0].id);
        rendered_rows.push(ProcessListRenderedRow::Group {
            process_id,
            process_name: group.display_name.clone(),
            executable_path: group.executable_path.clone(),
            process_count: group.processes.len(),
            user_label: process_list_group_user_label(&group.processes),
            user_unavailable: process_list_group_user_unavailable(&group.processes),
            protected: process_list_group_is_protected(&group.processes),
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

pub(in crate::ui::app) fn process_list_render_data(
    app: &WinderustApp,
    window: &Window,
    search_query: &str,
) -> ProcessListRenderData {
    let visible_processes = app
        .running_processes
        .iter()
        .filter(|process| {
            !app.hide_inaccessible_processes || !process_list_process_is_inaccessible(process)
        })
        .filter(|process| process_list_matches_search(process, search_query))
        .cloned()
        .collect::<Vec<_>>();
    let process_count = visible_processes.len();
    let mut process_groups = process_list_groups(&visible_processes);
    for group in &mut process_groups {
        process_list_sort_group_processes(group, app.process_list_sort);
    }
    let mut process_summaries = Vec::with_capacity(process_groups.len());
    for group in &process_groups {
        let mut summary = process_policy_summary(&app.settings, &app.plans, &group.executable_path);
        let usages = group
            .processes
            .iter()
            .filter_map(|process| app.process_resource_usage.get(&process.id));
        let (cpu_percent, memory_bytes, efficiency_mode) =
            usages.fold((None, None, false), |totals, usage| {
                (
                    usage
                        .cpu_percent
                        .map(|value| totals.0.unwrap_or(0.0) + value)
                        .or(totals.0),
                    usage
                        .working_set_bytes
                        .map(|value| totals.1.unwrap_or(0_u64).saturating_add(value))
                        .or(totals.1),
                    totals.2 || usage.efficiency_mode == Some(true),
                )
            });
        summary.status = if process_list_group_is_protected(&group.processes) {
            t!("process_list.status_protected_system_process").to_string()
        } else {
            process_list_status_label(
                &app.app_suspension_status,
                None,
                &group.executable_path,
                efficiency_mode,
            )
        };
        summary.cpu_percent = cpu_percent.map(|percent| percent.clamp(0.0, 100.0));
        summary.memory_bytes = memory_bytes;
        process_summaries.push(summary);
    }
    let mut column_layout =
        process_list_column_layout(&app.settings, &process_groups, &process_summaries);
    let available_width = (window.viewport_size().width
        - px(NAV_PANE_WIDTH + 48.0 + PROCESS_LIST_SCROLLBAR_GUTTER))
    .min(px(CONTENT_MAX_WIDTH - PROCESS_LIST_SCROLLBAR_GUTTER))
    .max(Pixels::ZERO);
    stretch_process_list_layout(&mut column_layout, available_width);
    let process_rows =
        process_list_sorted_rows(process_groups, process_summaries, app.process_list_sort);
    let table_width = process_list_table_width(&column_layout);
    let process_icons_by_path = process_list_icons_by_path(&app.process_candidates);
    let rows =
        process_list_rendered_rows(&process_rows, &process_icons_by_path, |executable_path| {
            app.is_process_list_group_collapsed(executable_path)
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

pub(in crate::ui::app) fn process_list_matches_search(process: &ProcessInfo, query: &str) -> bool {
    let query = query.trim().to_ascii_lowercase();
    query.is_empty()
        || process.name.to_ascii_lowercase().contains(&query)
        || process.id.to_string().contains(&query)
        || process
            .image_path
            .as_ref()
            .is_some_and(|path| path.to_string_lossy().to_ascii_lowercase().contains(&query))
}

pub(in crate::ui::app) fn process_list_icons_by_path(
    candidates: &[ProcessCandidate],
) -> HashMap<String, &Arc<Image>> {
    candidates
        .iter()
        .filter_map(|candidate| {
            candidate.icon.as_ref().map(|icon| {
                (
                    process_list_executable_path_group_key(&candidate.image_path),
                    icon,
                )
            })
        })
        .collect()
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
        ProcessListSortColumn::Column(ProcessListColumn::CpuUsage) => left_summary
            .cpu_percent
            .partial_cmp(&right_summary.cpu_percent)
            .unwrap_or(CmpOrdering::Equal),
        ProcessListSortColumn::Column(ProcessListColumn::MemoryUsage) => {
            left_summary.memory_bytes.cmp(&right_summary.memory_bytes)
        }
        ProcessListSortColumn::Column(ProcessListColumn::Status) => {
            process_list_text_sort_cmp(&left_summary.status, &right_summary.status)
        }
        ProcessListSortColumn::Column(ProcessListColumn::User) => process_list_text_sort_cmp(
            &process_list_group_user_label(&left_group.processes),
            &process_list_group_user_label(&right_group.processes),
        ),
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
        ProcessListSortColumn::Column(ProcessListColumn::User) => process_list_text_sort_cmp(
            &process_list_user_label(left.user_name.as_deref(), left.session_id, left.id),
            &process_list_user_label(right.user_name.as_deref(), right.session_id, right.id),
        ),
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

pub(in crate::ui::app) fn process_list_user_label(
    user_name: Option<&str>,
    session_id: Option<u32>,
    process_id: u32,
) -> String {
    if process_id <= 4 {
        return t!("process_list.windows_kernel").to_string();
    }

    match (user_name, session_id) {
        (Some(user_name), _) => user_name.to_owned(),
        (None, Some(session_id)) => {
            format!("{} · S{session_id}", t!("process_list.user_unavailable"))
        }
        (None, None) => t!("process_list.user_unavailable").to_string(),
    }
}

pub(in crate::ui::app) fn process_list_process_is_protected(process: &ProcessInfo) -> bool {
    process.is_critical == Some(true)
        || contains_process_name(CORE_BUILT_IN_PROCESS_EXCLUSIONS, &process.name)
}

pub(in crate::ui::app) fn process_list_process_is_inaccessible(process: &ProcessInfo) -> bool {
    process.image_path.is_none()
        || process.id == 0
        || process.id == std::process::id()
        || process.is_critical.is_none()
        || process_list_process_is_protected(process)
}

pub(in crate::ui::app) fn process_list_group_is_protected(processes: &[&ProcessInfo]) -> bool {
    processes
        .iter()
        .any(|process| process_list_process_is_protected(process))
}

pub(in crate::ui::app) fn process_list_group_user_unavailable(processes: &[&ProcessInfo]) -> bool {
    !processes.is_empty()
        && processes
            .iter()
            .all(|process| process.user_name.is_none() && process.id > 4)
}

pub(in crate::ui::app) fn process_list_group_user_label(processes: &[&ProcessInfo]) -> String {
    let first = processes
        .first()
        .map(|process| (process.user_name.as_deref(), process.session_id));
    if processes
        .iter()
        .all(|process| Some((process.user_name.as_deref(), process.session_id)) == first)
    {
        first.map_or_else(
            || t!("process_list.user_unavailable").to_string(),
            |(user_name, session_id)| {
                process_list_user_label(user_name, session_id, processes[0].id)
            },
        )
    } else {
        t!("process_list.multiple_users").to_string()
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

pub(in crate::ui::app) fn process_list_status_label(
    snapshot: &AppSuspensionSnapshot,
    process_id: Option<u32>,
    executable_path: &str,
    efficiency_mode: bool,
) -> String {
    let suspended = process_id.map_or_else(
        || {
            !executable_path.is_empty()
                && snapshot.suspended_apps.iter().any(|suspended_path| {
                    same_executable_path(Path::new(suspended_path), Path::new(executable_path))
                })
        },
        |process_id| snapshot.suspended_process_ids.contains(&process_id),
    );
    if suspended {
        t!("process_list.status_suspended").to_string()
    } else if efficiency_mode {
        t!("process_list.status_efficiency_mode").to_string()
    } else {
        t!("process_list.status_active").to_string()
    }
}

pub(in crate::ui::app) fn process_list_group_key(process_name: &str) -> String {
    process_name.trim().to_ascii_lowercase()
}

pub(in crate::ui::app) fn process_list_executable_path_group_key(path: &Path) -> String {
    executable_path_key(path)
}
pub(in crate::ui::app) fn process_list_column_min_width(column: ProcessListColumn) -> f32 {
    match column {
        ProcessListColumn::Pid => PROCESS_LIST_PID_MIN_WIDTH,
        ProcessListColumn::Status => 120.0,
        ProcessListColumn::CpuUsage => 88.0,
        ProcessListColumn::MemoryUsage => 112.0,
        ProcessListColumn::User => 120.0,
        _ => PROCESS_LIST_COLUMN_MIN_WIDTH,
    }
}
pub(in crate::ui::app) fn process_list_column_max_width(column: ProcessListColumn) -> f32 {
    match column {
        ProcessListColumn::Pid => PROCESS_LIST_PID_MAX_WIDTH,
        ProcessListColumn::Status => 210.0,
        ProcessListColumn::CpuUsage => 110.0,
        ProcessListColumn::MemoryUsage => 150.0,
        ProcessListColumn::User => 200.0,
        _ => PROCESS_LIST_COLUMN_MAX_WIDTH,
    }
}

pub(in crate::ui::app) fn process_list_column_label(
    column: ProcessListColumn,
    settings: &Settings,
) -> String {
    match column {
        ProcessListColumn::Pid => t!("process_list.pid").to_string(),
        ProcessListColumn::Status => t!("process_list.status").to_string(),
        ProcessListColumn::CpuUsage => t!("process_list.cpu_usage").to_string(),
        ProcessListColumn::MemoryUsage => t!("process_list.memory_usage").to_string(),
        ProcessListColumn::User => t!("process_list.user").to_string(),
        ProcessListColumn::PowerPlanForeground => {
            t!("process_list.power_plan_foreground").to_string()
        }
        ProcessListColumn::PowerPlanRunning => t!("process_list.power_plan_running").to_string(),
        ProcessListColumn::AdaptiveEngine => t!("process_list.adaptive_engine").to_string(),
        ProcessListColumn::BackgroundEfficiency => {
            t!("process_list.background_efficiency").to_string()
        }
        ProcessListColumn::ProcessPriority => t!("process_list.process_priority").to_string(),
        ProcessListColumn::ThreadPriority => t!("process_list.thread_priority").to_string(),
        ProcessListColumn::DynamicPriorityBoost => {
            t!("process_list.dynamic_priority_boost").to_string()
        }
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
    for column in PROCESS_LIST_OVERVIEW_COLUMNS {
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
        } else if column == ProcessListColumn::User {
            for group in groups {
                width = width.max(process_list_estimated_cell_width(
                    &process_list_group_user_label(&group.processes),
                    PROCESS_LIST_TEXT_CELL_HORIZONTAL_PADDING,
                ));
                for process in &group.processes {
                    width = width.max(process_list_estimated_cell_width(
                        &process_list_user_label(
                            process.user_name.as_deref(),
                            process.session_id,
                            process.id,
                        ),
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

pub(in crate::ui::app) fn process_list_table_width(layout: &ProcessListColumnLayout) -> Pixels {
    let visible_column_count = 1 + PROCESS_LIST_OVERVIEW_COLUMNS.len();
    let data_width = layout.name_width
        + PROCESS_LIST_OVERVIEW_COLUMNS
            .iter()
            .copied()
            .map(|column| layout.column_width(column))
            .sum::<f32>();
    let gap_count = visible_column_count.saturating_sub(1) as f32;

    px(data_width + PROCESS_LIST_ROW_HORIZONTAL_PADDING + PROCESS_LIST_COLUMN_GAP * gap_count)
}

pub(in crate::ui::app) fn stretch_process_list_layout(
    layout: &mut ProcessListColumnLayout,
    available_width: Pixels,
) {
    let extra_width = available_width - process_list_table_width(layout);
    if extra_width <= Pixels::ZERO {
        return;
    }
    layout.name_width += extra_width / px(1.0);
}
