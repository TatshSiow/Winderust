use super::design;
use super::priority_control::{Kind, Value};
use super::widgets::{button, checkbox, pick_list, text_input};
use crate::automation::{RuntimeHandle, RuntimeStatusSnapshot};
use crate::config::*;
use crate::foreground::{
    self, ProcessActionTarget, ProcessActionTargetError, ProcessInfo, ProcessResourceSample,
};
use iced::widget::{
    column, container, image, mouse_area, responsive, row, scrollable, text, Space,
};
use iced::{Element, Fill, Task};
use rust_i18n::t;
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    ops::Range,
    path::PathBuf,
    sync::Arc,
};
#[path = "process_details.rs"]
mod details;
#[path = "process_viewport.rs"]
mod viewport;
const ROW_HEIGHT: f32 = 36.0;
#[derive(Debug, Clone)]
pub(super) struct Population {
    processes: Vec<ProcessInfo>,
    samples: BTreeMap<u32, ProcessResourceSample>,
    total_memory_bytes: Option<u64>,
    icons: HashMap<PathBuf, Option<Arc<image::Handle>>>,
}
#[derive(Debug, Clone)]
struct Entry {
    indices: Vec<usize>,
    key: String,
    nested: bool,
}
#[derive(Debug, Clone)]
pub(super) struct Selection {
    processes: Vec<ProcessInfo>,
    path: String,
    name: String,
}
impl Selection {
    fn has_tree(&self, population: &[ProcessInfo]) -> bool {
        self.processes.len() > 1
            || self.processes.iter().any(|parent| {
                population.iter().any(|child| {
                    child.id != parent.id
                        && child.parent_id == Some(parent.id)
                        && match (parent.creation_time, child.creation_time) {
                            (Some(parent), Some(child)) => child >= parent,
                            // Let the existing tree validation reject an unverifiable relationship.
                            _ => true,
                        }
                })
            })
    }

    fn targets(&self) -> Vec<Result<ProcessActionTarget, ProcessActionTargetError>> {
        self.processes.iter().map(snapshot_target).collect()
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Sort {
    Name,
    Pid,
    Status,
    Cpu,
    Memory,
    User,
}
impl Sort {
    const ALL: [Self; 6] = [
        Self::Name,
        Self::Pid,
        Self::Status,
        Self::Cpu,
        Self::Memory,
        Self::User,
    ];
    fn label(self) -> String {
        t!(match self {
            Self::Name => "process_list.process_name",
            Self::Pid => "process_list.pid",
            Self::Status => "process_list.status",
            Self::Cpu => "process_list.cpu_usage",
            Self::Memory => "process_list.memory_usage",
            Self::User => "process_list.user",
        })
        .to_string()
    }
}
#[derive(Debug, Clone, Copy)]
pub(super) enum Action {
    Priority(Value),
    Efficiency(bool),
    Suspend(bool),
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum ProcessTab {
    #[default]
    Immediate,
    Rulesets,
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum MenuBranch {
    Efficiency,
    #[default]
    Closed,
    Types,
    Levels(Kind),
}
#[derive(Debug, Clone)]
pub(super) enum Message {
    Refresh,
    Loaded(Result<Population, String>),
    Search(String),
    Scrolled(f32),
    ResizeStart(Sort, f32),
    ResizeMoved(f32),
    ResizeEnd,
    AutoSize(Sort),
    HideInaccessible(bool),
    Group(bool),
    MemoryPercentage(bool),
    Expand(String),
    Sort(Sort),
    Column(Sort, bool),
    Select(usize, u64),
    Focus(usize, u64),
    ExpandFocused,
    DeleteFocused,
    Context(usize, u64),
    PointerMoved(iced::Point),
    OpenDetails,
    ProcessTab(ProcessTab),
    MenuBranch(MenuBranch),
    Menu(Box<Message>),
    Current(Result<(Selection, Vec<Value>, Option<bool>), String>),
    CloseSelection,
    Action(Action),
    Stop(bool),
    ConfirmStop,
    CancelStop,
    TreeReady(Result<Vec<ProcessActionTarget>, String>),
    Result(String, Result<(), String>),
    OpenLocation,
    Properties,
    Details(details::Message),
}

pub(super) struct ProcessList {
    processes: Vec<ProcessInfo>,
    rows: Vec<Entry>,
    offsets: Vec<f32>,
    rows_revision: u64,
    samples: BTreeMap<u32, ProcessResourceSample>,
    total_memory_bytes: Option<u64>,
    cpu: HashMap<u32, f32>,
    suspended_process_ids: Vec<u32>,
    icons: HashMap<PathBuf, Option<Arc<image::Handle>>>,
    search: String,
    offset: std::cell::Cell<f32>,
    refreshing: bool,
    error: Option<String>,
    hide_inaccessible: bool,
    grouped: bool,
    memory_percentage: bool,
    expanded: HashSet<String>,
    sort: Sort,
    descending: bool,
    columns: [bool; 6],
    column_widths: [Option<f32>; 6],
    resizing: Option<(Sort, f32, Option<f32>)>,
    selected: Option<Selection>,
    focused: Option<(String, bool, Selection)>,
    pointer: iced::Point,
    context: Option<iced::Point>,
    menu_branch: MenuBranch,
    process_tab: ProcessTab,
    current: Vec<Value>,
    efficiency: Option<bool>,
    stopping: Option<(Selection, bool)>,
}
impl Default for ProcessList {
    fn default() -> Self {
        Self {
            processes: Vec::new(),
            rows: Vec::new(),
            offsets: vec![0.0],
            rows_revision: 0,
            samples: BTreeMap::new(),
            total_memory_bytes: None,
            cpu: HashMap::new(),
            suspended_process_ids: Vec::new(),
            icons: HashMap::new(),
            search: String::new(),
            offset: std::cell::Cell::new(0.0),
            refreshing: false,
            error: None,
            hide_inaccessible: true,
            grouped: true,
            memory_percentage: false,
            expanded: HashSet::new(),
            sort: Sort::Name,
            descending: false,
            columns: [true; 6],
            column_widths: [None; 6],
            resizing: None,
            selected: None,
            focused: None,
            pointer: iced::Point::ORIGIN,
            context: None,
            menu_branch: MenuBranch::Closed,
            process_tab: ProcessTab::Immediate,
            current: Vec::new(),
            efficiency: None,
            stopping: None,
        }
    }
}
impl ProcessList {
    pub(super) fn context_open(&self) -> bool {
        self.context.is_some() && self.selected.is_some()
    }

    pub(super) fn discard(&mut self, message: Message) {
        if matches!(message, Message::Loaded(_)) {
            self.refreshing = false;
        }
        self.clear();
    }
    pub(super) fn clear(&mut self) {
        self.rows_revision = self.rows_revision.wrapping_add(1);
        self.processes.clear();
        self.rows.clear();
        self.offsets = vec![0.0];
        self.samples.clear();
        self.cpu.clear();
        self.icons.clear();
        self.expanded.clear();
        self.focused = None;
        self.selected = None;
        self.context = None;
        self.stopping = None;
        self.offset.set(0.0);
    }
    pub(super) fn update(
        &mut self,
        message: Message,
        settings: &mut Settings,
        runtime: &RuntimeHandle,
    ) -> Task<Message> {
        match message {
            Message::Refresh if !self.refreshing => {
                self.refreshing = true;
                let cached = self.icons.keys().cloned().collect::<HashSet<_>>();
                return background(
                    move || {
                        let processes = foreground::list_processes_with_paths()?;
                        let mut samples = foreground::sample_process_resources(&processes);
                        let identities = processes
                            .iter()
                            .map(|p| (p.id, p.creation_time))
                            .collect::<HashMap<_, _>>();
                        samples.retain(|id, sample| {
                            identities.get(id) == Some(&Some(sample.creation_time))
                        });
                        let mut icons = HashMap::new();
                        for p in &processes {
                            if let Some(path) = &p.image_path {
                                if !cached.contains(path) && !icons.contains_key(path) {
                                    icons.insert(
                                        path.clone(),
                                        crate::process_icon::load_process_icon(path),
                                    );
                                }
                            }
                        }
                        Ok(Population {
                            total_memory_bytes: crate::dashboard_metrics::sample_memory_usage()
                                .total_physical_bytes,
                            processes,
                            samples,
                            icons,
                        })
                    },
                    Message::Loaded,
                );
            }
            Message::Refresh => {}
            Message::Loaded(result) => {
                self.refreshing = false;
                match result {
                    Ok(data) => {
                        self.cpu = data
                            .samples
                            .iter()
                            .filter_map(|(id, s)| {
                                self.samples
                                    .get(id)
                                    .filter(|old| old.creation_time == s.creation_time)
                                    .and_then(|old| {
                                        crate::cpu::process_cpu_usage_percent(old.cpu, s.cpu)
                                    })
                                    .map(|cpu| (*id, cpu))
                            })
                            .collect();
                        self.processes = data.processes;
                        self.samples = data.samples;
                        self.total_memory_bytes = data.total_memory_bytes;
                        self.icons.extend(data.icons);
                        let paths = self
                            .processes
                            .iter()
                            .filter_map(|p| p.image_path.as_ref())
                            .collect::<HashSet<_>>();
                        self.icons.retain(|p, _| paths.contains(p));
                        self.error = None;
                        self.rebuild();
                    }
                    Err(e) => self.error = Some(e),
                }
            }
            Message::Search(v) => {
                self.search = v;
                self.offset.set(0.0);
                self.rebuild();
                return iced::widget::operation::scroll_to(
                    "process-list",
                    scrollable::AbsoluteOffset::<f32>::default(),
                );
            }
            Message::Scrolled(v) => self.offset.set(v),
            Message::ResizeStart(col, width) => self.resizing = Some((col, width, None)),
            Message::ResizeMoved(x) => self.resize_column(x),
            Message::ResizeEnd => self.resizing = None,
            Message::AutoSize(col) => {
                self.resizing = None;
                self.column_widths[col as usize] = Some(self.auto_width(col));
            }
            Message::HideInaccessible(v) => {
                self.hide_inaccessible = v;
                self.rebuild();
            }
            Message::MemoryPercentage(value) => self.memory_percentage = value,
            Message::Group(v) => {
                self.grouped = v;
                self.rebuild();
            }
            Message::Expand(key) => {
                if !self.expanded.remove(&key) {
                    self.expanded.insert(key);
                }
                self.rebuild();
            }
            Message::Sort(sort) => {
                if self.sort == sort {
                    self.descending = !self.descending;
                } else {
                    self.sort = sort;
                    self.descending = matches!(sort, Sort::Cpu | Sort::Memory);
                }
                self.rebuild();
            }
            Message::Column(column, v) => self.columns[column as usize] = v,
            Message::Menu(message) => {
                let task = self.update(*message, settings, runtime);
                self.selected = None;
                self.context = None;
                return task;
            }
            Message::PointerMoved(position) => self.pointer = position,
            Message::MenuBranch(menu) => self.menu_branch = menu,
            Message::ProcessTab(tab) => self.process_tab = tab,
            Message::OpenDetails => {
                self.context = None;
                self.process_tab = ProcessTab::Rulesets;
            }
            Message::Context(index, revision) => {
                if revision != self.rows_revision || index >= self.rows.len() {
                    return Task::none();
                }
                let task = self.update(Message::Select(index, revision), settings, runtime);
                self.context = Some(self.pointer);
                self.menu_branch = MenuBranch::Closed;
                return task;
            }
            Message::Focus(index, revision) => self.focus_row(index, revision),
            Message::ExpandFocused => {
                if let Some((key, false, selection)) = &self.focused {
                    if selection.processes.len() > 1 {
                        return self.update(Message::Expand(key.clone()), settings, runtime);
                    }
                }
            }
            Message::DeleteFocused => {
                if self.selected.is_none() && self.stopping.is_none() {
                    if let Some((_, _, selection)) = &self.focused {
                        if selection.processes.iter().any(|p| !inaccessible(p)) {
                            self.selected = Some(selection.clone());
                            return self.update(Message::Stop(false), settings, runtime);
                        }
                    }
                }
            }
            Message::Select(index, revision) => {
                let Some(entry) = self
                    .rows
                    .get(index)
                    .filter(|_| revision == self.rows_revision)
                else {
                    return Task::none();
                };
                let p = &self.processes[entry.indices[0]];
                let selection = Selection {
                    processes: entry
                        .indices
                        .iter()
                        .map(|i| self.processes[*i].clone())
                        .collect(),
                    path: p
                        .image_path
                        .as_ref()
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    name: p.name.clone(),
                };
                self.context = None;
                self.stopping = None;
                self.current.clear();
                self.efficiency = None;
                self.selected = Some(selection.clone());
                self.process_tab = ProcessTab::Immediate;
                let allow = settings.general.allow_cross_session_process_control;
                return background(
                    move || {
                        let targets = selection
                            .targets()
                            .into_iter()
                            .filter_map(Result::ok)
                            .collect::<Vec<_>>();
                        let mut values = Vec::new();
                        if let Some(v) = targets.iter().find_map(|t| {
                            crate::control::priority_efficiency::current_process_priority(t, allow)
                                .ok()
                        }) {
                            values.push(Value::Process(v));
                        }
                        if let Some(v) = targets.iter().find_map(|t| {
                            crate::control::thread_priority::current_process_thread_priority(
                                t, allow,
                            )
                            .ok()
                            .flatten()
                        }) {
                            values.push(Value::Thread(v));
                        }
                        if let Some(v) = targets.iter().find_map(|t| {
                            crate::control::io_priority::current_process_io_priority(t, allow).ok()
                        }) {
                            values.push(Value::Io(v.into()));
                        }
                        if let Some(v) = targets.iter().find_map(|t| {
                            crate::control::gpu_priority::current_process_gpu_priority(t, allow)
                                .ok()
                        }) {
                            values.push(Value::Gpu(v.into()));
                        }
                        if let Some(v) = targets.iter().find_map(|t| {
                            crate::control::memory_priority::current_process_memory_priority(
                                t, allow,
                            )
                            .ok()
                        }) {
                            values.push(Value::Memory(v.into()));
                        }
                        if let Some(v)=targets.iter().find_map(|t|crate::control::dynamic_priority_boost::current_dynamic_priority_boost_state(t).ok()){values.push(Value::DynamicBoost(match v{crate::control::dynamic_priority_boost::DynamicPriorityBoostState::Enabled=>ProcessDynamicPriorityBoostSetting::Enabled,crate::control::dynamic_priority_boost::DynamicPriorityBoostState::Disabled=>ProcessDynamicPriorityBoostSetting::Disabled}));}
                        let efficiencies = targets
                            .iter()
                            .filter_map(|t| {
                                crate::control::priority_efficiency::current_efficiency_mode(
                                    t, allow,
                                )
                                .ok()
                            })
                            .collect::<Vec<_>>();
                        let efficiency =
                            (!efficiencies.is_empty()).then(|| efficiencies.iter().all(|v| *v));
                        Ok((selection, values, efficiency))
                    },
                    Message::Current,
                );
            }
            Message::Current(result) => match result {
                Ok((selection, values, efficiency)) => {
                    if self
                        .selected
                        .as_ref()
                        .is_some_and(|s| s.processes == selection.processes)
                    {
                        self.current = values;
                        self.efficiency = efficiency;
                    }
                }
                Err(e) => self.error = Some(e),
            },
            Message::CloseSelection => {
                self.selected = None;
                self.context = None;
                self.stopping = None;
            }
            Message::Action(action) => {
                if let Some(selection) = &self.selected {
                    let targets = selection.targets();
                    let result = match action {
                        Action::Efficiency(v) => runtime.request_efficiency_mode_action(targets, v),
                        Action::Suspend(v) => {
                            runtime.request_app_suspension_process_action(targets, v)
                        }
                        Action::Priority(value) => {
                            let kind = value_kind(value);
                            if !kind
                                .choices(settings.advanced.expose_all_priority_values)
                                .contains(&value)
                            {
                                self.error =
                                    Some(t!("process_list.status_access_denied").to_string());
                                return Task::none();
                            }
                            match value{Value::Process(v)=>runtime.request_process_priority_action(targets,v),Value::Thread(v)=>runtime.request_thread_priority_action(targets,v),Value::Io(v)=>if let Some(v)=v.priority(){runtime.request_io_priority_action(targets,v)}else{return Task::none()},Value::Gpu(v)=>if let Some(v)=v.priority(){runtime.request_gpu_priority_action(targets,v)}else{return Task::none()},Value::Memory(v)=>if let Some(v)=v.priority(){runtime.request_memory_priority_action(targets,v)}else{return Task::none()},Value::DynamicBoost(v)=>match v{ProcessDynamicPriorityBoostSetting::Enabled=>runtime.request_dynamic_priority_boost_action(targets,crate::control::dynamic_priority_boost::DynamicPriorityBoostState::Enabled),ProcessDynamicPriorityBoostSetting::Disabled=>runtime.request_dynamic_priority_boost_action(targets,crate::control::dynamic_priority_boost::DynamicPriorityBoostState::Disabled),ProcessDynamicPriorityBoostSetting::Default=>return Task::none()}}
                        }
                    };
                    match result {
                        Ok(receiver) => {
                            return background(
                                move || {
                                    receiver
                                        .recv()
                                        .map_err(|e| e.to_string())?
                                        .map_err(|e| e.to_string())?
                                        .into_process_list_result()
                                },
                                self.result_message(),
                            )
                        }
                        Err(e) => self.error = Some(e.to_string()),
                    }
                }
            }
            Message::Stop(tree) => {
                self.context = None;
                if let Some(selection) = self.selected.take() {
                    self.stopping = Some((selection, tree));
                }
            }
            Message::CancelStop => self.stopping = None,
            Message::ConfirmStop => {
                if let Some((selection, tree)) = self.stopping.take() {
                    let roots = selection
                        .targets()
                        .into_iter()
                        .collect::<Result<Vec<_>, _>>()
                        .map_err(|e| e.to_string());
                    if tree {
                        return background(
                            move || {
                                foreground::process_tree_action_targets(
                                    &roots?,
                                    &foreground::list_processes_with_paths()?,
                                )
                            },
                            Message::TreeReady,
                        );
                    }
                    return self.stop_targets(roots, runtime);
                }
            }
            Message::TreeReady(targets) => return self.stop_targets(targets, runtime),
            Message::Result(name, result) => {
                self.error = Some(match result {
                    Ok(()) => t!(
                        "process_list.quick_action_applied",
                        action = t!("common.actions"),
                        name = name
                    )
                    .to_string(),
                    Err(e) => e,
                })
            }
            Message::Properties => {
                if let Some(selection) = &self.selected {
                    let path = PathBuf::from(&selection.path);
                    return background(
                        move || foreground::open_process_properties(&path),
                        self.result_message(),
                    );
                }
            }
            Message::OpenLocation => {
                if let Some(selection) = &self.selected {
                    let path = PathBuf::from(&selection.path);
                    return background(
                        move || foreground::open_process_location(&path),
                        self.result_message(),
                    );
                }
            }
            Message::Details(message) => {
                if let Some(selection) = &self.selected {
                    details::update(settings, &selection.path, message);
                }
            }
        }
        Task::none()
    }
    fn result_message(&self) -> impl Fn(Result<(), String>) -> Message + Send + 'static {
        let name = self
            .selected
            .as_ref()
            .map(|s| s.name.clone())
            .unwrap_or_default();
        move |result| Message::Result(name.clone(), result)
    }
    fn stop_targets(
        &mut self,
        targets: Result<Vec<ProcessActionTarget>, String>,
        runtime: &RuntimeHandle,
    ) -> Task<Message> {
        let name = targets
            .as_ref()
            .ok()
            .and_then(|targets| targets.first())
            .map(|target| target.name.clone())
            .unwrap_or_default();
        match targets.and_then(|targets| {
            runtime
                .request_process_termination(targets)
                .map_err(|e| e.to_string())
        }) {
            Ok(receiver) => background(
                move || {
                    receiver
                        .recv()
                        .map_err(|e| e.to_string())?
                        .map_err(|e| e.to_string())?
                        .into_process_list_result()
                },
                move |result| Message::Result(name.clone(), result),
            ),
            Err(e) => {
                self.error = Some(e);
                Task::none()
            }
        }
    }
    fn row_offsets(&self) -> Vec<f32> {
        (0..=self.rows.len())
            .map(|i| i as f32 * ROW_HEIGHT)
            .collect()
    }
    pub(super) fn resizing_columns(&self) -> bool {
        self.resizing.is_some()
    }

    fn resize_column(&mut self, x: f32) {
        if let Some((col, width, anchor)) = &mut self.resizing {
            let start = *anchor.get_or_insert(x);
            let minimum = match col {
                Sort::Name => 160.0,
                Sort::Status => 140.0,
                Sort::User | Sort::Memory => 100.0,
                _ => 80.0,
            };
            self.column_widths[*col as usize] = Some((*width + x - start).max(minimum));
        }
    }

    fn cell_value(&self, entry: &Entry, col: Sort) -> String {
        let p = &self.processes[entry.indices[0]];
        match col {
            Sort::Name => p.name.clone(),
            Sort::Pid => {
                if entry.indices.len() > 1 {
                    format!("{} x", entry.indices.len())
                } else {
                    p.id.to_string()
                }
            }
            Sort::Cpu => {
                if entry
                    .indices
                    .iter()
                    .any(|i| self.cpu.contains_key(&self.processes[*i].id))
                {
                    format!("{:.1}%", self.cpu_total(entry))
                } else {
                    t!("common.unknown").to_string()
                }
            }
            Sort::Memory => {
                if entry.indices.iter().any(|i| {
                    self.samples
                        .get(&self.processes[*i].id)
                        .is_some_and(|sample| sample.working_set_bytes.is_some())
                }) {
                    format_memory_usage(
                        self.memory_total(entry),
                        self.total_memory_bytes,
                        self.memory_percentage,
                    )
                } else {
                    t!("common.unknown").to_string()
                }
            }
            Sort::User => self.user(entry),
            Sort::Status => self.status_key(entry).to_string(),
        }
    }

    fn auto_width(&self, col: Sort) -> f32 {
        let mut width = measure_column_text(&col.label()) + 44.0;
        for entry in &self.rows {
            let value = self.cell_value(entry, col);
            let value = if col == Sort::Status {
                t!(&value).to_string()
            } else {
                value
            };
            let padding = match col {
                Sort::Name => {
                    if entry.nested {
                        76.0
                    } else {
                        56.0
                    }
                }
                Sort::Status => 32.0,
                _ => 12.0,
            };
            width = width.max(measure_column_text(&value) + padding);
        }
        width.ceil()
    }

    fn layout_widths(&self, available: f32) -> [f32; 6] {
        let mut widths = Sort::ALL.map(|col| self.column_width(col));
        let visible = Sort::ALL
            .into_iter()
            .filter(|col| *col == Sort::Name || self.columns[*col as usize])
            .collect::<Vec<_>>();
        if let Some(last) = visible.last() {
            let used = visible.iter().map(|col| widths[*col as usize]).sum::<f32>()
                + (visible.len() - 1) as f32 * design::space::SMALL as f32
                + 32.0;
            widths[*last as usize] += (available - used).max(0.0);
        }
        widths
    }

    fn column_width(&self, col: Sort) -> f32 {
        self.column_widths[col as usize].unwrap_or_else(|| column_width(col))
    }

    fn focus_row(&mut self, index: usize, revision: u64) {
        if let Some(entry) = self
            .rows
            .get(index)
            .filter(|_| revision == self.rows_revision)
        {
            let p = &self.processes[entry.indices[0]];
            self.focused = Some((
                entry.key.clone(),
                entry.nested,
                Selection {
                    processes: entry
                        .indices
                        .iter()
                        .map(|i| self.processes[*i].clone())
                        .collect(),
                    path: p
                        .image_path
                        .as_ref()
                        .map(|p| p.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    name: p.name.clone(),
                },
            ));
        }
    }

    pub(super) fn sync_suspended_processes(&mut self, ids: &[u32]) {
        if self.suspended_process_ids != ids {
            self.suspended_process_ids.clear();
            self.suspended_process_ids.extend_from_slice(ids);
            if self.sort == Sort::Status {
                self.rebuild();
            }
        }
    }

    fn status_key(&self, entry: &Entry) -> &'static str {
        if entry.indices.iter().all(|i| protected(&self.processes[*i])) {
            "process_list.status_system_protected"
        } else if entry
            .indices
            .iter()
            .all(|i| !self.processes[*i].can_set_information)
        {
            "process_list.status_access_denied"
        } else if entry
            .indices
            .iter()
            .all(|i| inaccessible(&self.processes[*i]))
        {
            "process_list.status_unavailable"
        } else if entry
            .indices
            .iter()
            .any(|i| self.suspended_process_ids.contains(&self.processes[*i].id))
        {
            "process_list.status_suspended"
        } else if entry.indices.iter().any(|i| {
            self.samples
                .get(&self.processes[*i].id)
                .is_some_and(|s| s.efficiency_mode == Some(true))
        }) {
            "process_list.status_efficiency_mode"
        } else {
            "process_list.status_active"
        }
    }

    fn rebuild(&mut self) {
        self.rows_revision = self.rows_revision.wrapping_add(1);
        let search = self.search.to_lowercase();
        let mut groups = BTreeMap::<String, Vec<usize>>::new();
        for (i, p) in self.processes.iter().enumerate() {
            if self.hide_inaccessible && inaccessible(p) {
                continue;
            }
            if !format!(
                "{} {} {} {}",
                p.name,
                p.id,
                p.image_path
                    .as_ref()
                    .map(|p| p.to_string_lossy())
                    .unwrap_or_default(),
                p.user_name.as_deref().unwrap_or("")
            )
            .to_lowercase()
            .contains(&search)
            {
                continue;
            }
            let key = if self.grouped {
                p.image_path
                    .as_ref()
                    .map(|p| foreground::executable_path_key(p))
                    .unwrap_or_else(|| format!("pid:{}", p.id))
            } else {
                format!("pid:{}", p.id)
            };
            groups.entry(key).or_default().push(i);
        }
        let mut groups = groups
            .into_iter()
            .map(|(key, indices)| Entry {
                key,
                indices,
                nested: false,
            })
            .collect::<Vec<_>>();
        groups.sort_by(|a, b| {
            let ord = match self.sort {
                Sort::Pid => self.processes[a.indices[0]]
                    .id
                    .cmp(&self.processes[b.indices[0]].id),
                Sort::Cpu => self.cpu_total(a).total_cmp(&self.cpu_total(b)),
                Sort::Memory => self.memory_total(a).cmp(&self.memory_total(b)),
                Sort::User => self.user(a).cmp(&self.user(b)),
                Sort::Status => self.status_key(a).cmp(self.status_key(b)),
                Sort::Name => self.processes[a.indices[0]]
                    .name
                    .to_lowercase()
                    .cmp(&self.processes[b.indices[0]].name.to_lowercase()),
            };
            let ord = if self.descending { ord.reverse() } else { ord };
            ord.then_with(|| a.key.cmp(&b.key))
        });
        self.rows.clear();
        for group in groups {
            let children = if group.indices.len() > 1 && self.expanded.contains(&group.key) {
                group
                    .indices
                    .iter()
                    .map(|i| Entry {
                        key: group.key.clone(),
                        indices: vec![*i],
                        nested: true,
                    })
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            };
            self.rows.push(group);
            self.rows.extend(children);
        }
        self.offsets = self.row_offsets();
        if !self.rows.is_empty() {
            for col in Sort::ALL {
                if self.column_widths[col as usize].is_none() {
                    self.column_widths[col as usize] = Some(self.auto_width(col));
                }
            }
        }
    }
    fn cpu_total(&self, e: &Entry) -> f32 {
        e.indices
            .iter()
            .filter_map(|i| self.cpu.get(&self.processes[*i].id))
            .sum()
    }
    fn memory_total(&self, e: &Entry) -> u64 {
        e.indices
            .iter()
            .filter_map(|i| {
                self.samples
                    .get(&self.processes[*i].id)
                    .and_then(|s| s.working_set_bytes)
            })
            .sum()
    }
    fn user(&self, e: &Entry) -> String {
        let p = &self.processes[e.indices[0]];
        if e.indices.iter().any(|i| {
            self.processes[*i].user_name != p.user_name
                || self.processes[*i].session_id != p.session_id
        }) {
            return t!("process_list.multiple_users").to_string();
        }
        format!(
            "{} / S{}",
            p.user_name
                .as_deref()
                .unwrap_or(&t!("process_list.user_unavailable")),
            p.session_id
                .map(|s| s.to_string())
                .unwrap_or_else(|| "?".into())
        )
    }
    fn immediate_priorities(&self, settings: &Settings, eligible: bool) -> Element<'_, Message> {
        let mut controls = column![].spacing(design::space::SMALL);
        for kind in [
            Kind::Process,
            Kind::Thread,
            Kind::Io,
            Kind::Gpu,
            Kind::Memory,
            Kind::DynamicBoost,
        ] {
            let key = format!("nav.{}", kind.key());
            let choices = kind
                .choices(settings.advanced.expose_all_priority_values)
                .into_iter()
                .filter(|v| !is_default(*v))
                .collect::<Vec<_>>();
            let current = self
                .current
                .iter()
                .copied()
                .find(|v| value_kind(*v) == kind);
            let control: Element<'_, Message> = if eligible {
                pick_list(choices, current, move |v| {
                    Message::Action(Action::Priority(v))
                })
                .placeholder(t!("common.unknown").to_string())
                .width(design::SELECT_WIDTH)
                .into()
            } else {
                text(
                    current
                        .map(|v| v.to_string())
                        .unwrap_or_else(|| t!("common.unknown").to_string()),
                )
                .into()
            };
            controls = controls.push(super::widgets::settings_card(super::widgets::setting_row(
                &key, control,
            )));
        }
        controls.into()
    }

    pub(super) fn side_panel(&self) -> Element<'_, Message> {
        let mut filters = column![
            super::widgets::heading(t!("nav.settings").to_string(), design::typography::SUBTITLE),
            super::widgets::heading(
                t!("process_list.filter").to_string(),
                design::typography::SECONDARY
            ),
            checkbox(self.hide_inaccessible)
                .label(t!("process_list.hide_inaccessible_processes").to_string())
                .on_toggle(Message::HideInaccessible),
            super::widgets::heading(
                t!("process_list.grouping").to_string(),
                design::typography::SECONDARY
            ),
            checkbox(self.grouped)
                .label(t!("process_list.group_by_app").to_string())
                .on_toggle(Message::Group),
            super::widgets::heading(
                t!("process_list.columns").to_string(),
                design::typography::SECONDARY
            ),
        ]
        .spacing(design::space::MEDIUM);
        for col in Sort::ALL.into_iter().skip(1) {
            filters = filters.push(
                checkbox(self.columns[col as usize])
                    .label(col.label())
                    .on_toggle(move |value| Message::Column(col, value)),
            );
        }
        let value_label = t!("process_list.metric_value").to_string();
        let percentage_label = t!("process_list.metric_percentage").to_string();
        let selected = if self.memory_percentage {
            percentage_label.clone()
        } else {
            value_label.clone()
        };
        filters = filters
            .push(super::widgets::heading(
                t!("process_list.metrics").to_string(),
                design::typography::SECONDARY,
            ))
            .push(
                row![
                    text(t!("process_list.memory_usage").to_string()).width(Fill),
                    pick_list(
                        vec![value_label, percentage_label.clone()],
                        Some(selected),
                        move |value| { Message::MemoryPercentage(value == percentage_label) }
                    )
                    .width(140),
                ]
                .spacing(design::space::SMALL)
                .align_y(iced::Center),
            );
        scrollable(filters).height(Fill).into()
    }

    pub(super) fn view<'a>(
        &'a self,
        settings: &'a Settings,
        status: &'a RuntimeStatusSnapshot,
        plans: &'a [crate::power::PowerPlan],
    ) -> Element<'a, Message> {
        let controls = row![
            text_input(&t!("process_list.search_placeholder"), &self.search)
                .on_input(Message::Search)
                .width(280),
            Space::new().width(Fill),
            text(t!("process_list.count", count = self.processes.len()).to_string()),
            iced::widget::tooltip(
                button(super::navigation::glyph("icons/refresh-cw.svg"))
                    .style(super::widgets::quiet)
                    .on_press(Message::Refresh),
                text(t!("settings.refresh").to_string()),
                iced::widget::tooltip::Position::Bottom,
            ),
        ]
        .spacing(design::space::SMALL)
        .align_y(iced::Center);
        let mut body = column![controls.width(Fill)]
            .spacing(design::space::SMALL)
            .height(Fill);
        if let Some(e) = &self.error {
            body = body.push(text(e));
        }
        body = body.push(responsive(move |size| {
            let widths = self.layout_widths(size.width);
            let name_width = widths[Sort::Name as usize];
            let sort_header = |col: Sort, width| {
                let mut label = row![text(col.label())]
                    .spacing(design::space::SMALL)
                    .align_y(iced::Center);
                if self.sort == col {
                    label = label.push(super::navigation::glyph(if self.descending {
                        "icons/chevron-down.svg"
                    } else {
                        "icons/chevron-up.svg"
                    }));
                }
                row![
                    button(label)
                        .on_press(Message::Sort(col))
                        .style(super::widgets::quiet)
                        .width(Fill),
                    mouse_area(
                        container(iced::widget::rule::vertical(1))
                            .width(6)
                            .height(24)
                            .center_x(6)
                    )
                    .interaction(iced::mouse::Interaction::ResizingHorizontally)
                    .on_press(Message::ResizeStart(col, width))
                    .on_double_click(Message::AutoSize(col)),
                ]
                .width(width)
                .align_y(iced::Center)
            };
            let mut header =
                row![sort_header(Sort::Name, name_width)].spacing(design::space::SMALL);
            for col in Sort::ALL.into_iter().skip(1) {
                if self.columns[col as usize] {
                    header = header.push(sort_header(col, widths[col as usize]));
                }
            }
            let offsets = &self.offsets;
            let range = visible_range_offsets(offsets, self.offset.get() - 32.0, size.height);
            let mut rows = column![
                container(header).height(32).style(super::widgets::surface),
                Space::new().height(offsets[range.start])
            ];
            let mut visible_rows = Vec::with_capacity(range.len());
            for row_index in range.clone() {
                let height = offsets[row_index + 1] - offsets[row_index];
                if height <= 0.0 {
                    continue;
                }
                let entry = &self.rows[row_index];
                let p = &self.processes[entry.indices[0]];
                let mut name = row![].spacing(design::space::SMALL).align_y(iced::Center);
                if entry.indices.len() > 1 {
                    name = name.push(
                        button(super::navigation::glyph(
                            if self.expanded.contains(&entry.key) {
                                "icons/chevron-down.svg"
                            } else {
                                "icons/chevron-right.svg"
                            },
                        ))
                        .padding(0)
                        .width(20)
                        .height(20)
                        .style(super::widgets::quiet)
                        .on_press(Message::Expand(entry.key.clone())),
                    );
                } else {
                    name = name.push(Space::new().width(if entry.nested { 40 } else { 20 }));
                }
                if let Some(icon) = p
                    .image_path
                    .as_ref()
                    .and_then(|p| self.icons.get(p))
                    .and_then(Option::as_ref)
                {
                    name = name.push(image((**icon).clone()).width(20).height(20));
                } else {
                    name = name.push(
                        container(super::navigation::glyph("icons/app-window.svg"))
                            .width(20)
                            .height(20)
                            .center_x(20)
                            .center_y(20),
                    );
                }
                name = name.push(text(p.name.clone()).wrapping(iced::widget::text::Wrapping::None));
                let name: Element<'_, Message> = name.into();
                let mut cells = row![container(name).width(name_width).clip(true)]
                    .spacing(design::space::SMALL)
                    .align_y(iced::Center);
                for col in Sort::ALL.into_iter().skip(1) {
                    if !self.columns[col as usize] {
                        continue;
                    }
                    let value = self.cell_value(entry, col);
                    let cell = if col == Sort::Status {
                        status_cell(&value)
                    } else {
                        text(value)
                            .wrapping(iced::widget::text::Wrapping::None)
                            .into()
                    };
                    cells = cells.push(container(cell).width(widths[col as usize]).clip(true));
                }
                let focused = self
                    .focused
                    .as_ref()
                    .is_some_and(|(key, nested, selection)| {
                        *key == entry.key
                            && *nested == entry.nested
                            && selection.processes.first().is_some_and(|selected| {
                                selected.id == p.id && selected.creation_time == p.creation_time
                            })
                    });
                visible_rows.push((
                    (p.id, p.creation_time, entry.nested),
                    mouse_area(
                        button(
                            mouse_area(
                                container(column![
                                    container(cells)
                                        .padding([0, design::space::SMALL as u16])
                                        .center_y(ROW_HEIGHT - 1.0),
                                    iced::widget::rule::horizontal(1)
                                ])
                                .height(height)
                                .clip(true),
                            )
                            .on_press(Message::Focus(row_index, self.rows_revision))
                            .on_double_click(Message::ExpandFocused),
                        )
                        .padding(0)
                        .width(Fill)
                        .on_press(Message::Focus(row_index, self.rows_revision))
                        .style(move |theme, status| {
                            let mut style = super::widgets::quiet(theme, status);
                            if focused && matches!(status, iced::widget::button::Status::Active) {
                                style.background =
                                    Some(theme.palette().primary.scale_alpha(0.15).into());
                            }
                            style.border.radius = 0.0.into();
                            style.text_color = theme.palette().text;
                            style
                        }),
                    )
                    .on_right_press(Message::Context(row_index, self.rows_revision))
                    .into(),
                ));
            }
            rows = rows.push(iced::widget::keyed_column(visible_rows).width(Fill));
            rows = rows.push(
                Space::new()
                    .height(offsets.last().copied().unwrap_or_default() - offsets[range.end]),
            );
            let table = scrollable(container(rows).width(Fill))
                .id("process-list")
                .height(Fill)
                .direction(scrollable::Direction::Both {
                    vertical: scrollable::Scrollbar::default(),
                    horizontal: scrollable::Scrollbar::default(),
                })
                .on_scroll(|v| Message::Scrolled(v.absolute_offset().y));
            viewport::buffered(
                container(table)
                    .width(Fill)
                    .height(Fill)
                    .style(super::widgets::surface)
                    .into(),
                &self.offset,
                offsets[range.start] + 32.0,
                offsets[range.end] + 32.0,
                range.start == 0,
                range.end == self.rows.len(),
            )
        }));
        let body = mouse_area(body).on_move(Message::PointerMoved);
        if let Some((stopping, tree)) = &self.stopping {
            let dialog = container(
                column![
                    super::widgets::heading(
                        t!("process_list.kill_confirm_title", name = &stopping.name).to_string(),
                        design::typography::DIALOG_TITLE,
                    ),
                    text(
                        t!(
                            if *tree {
                                "process_list.stop_tree_confirm"
                            } else {
                                "process_list.stop_confirm"
                            },
                            name = &stopping.name
                        )
                        .to_string()
                    ),
                    row![
                        Space::new().width(Fill),
                        button(text(t!("process_list.kill").to_string()))
                            .style(super::widgets::danger_button)
                            .on_press(Message::ConfirmStop),
                        button(text(t!("common.cancel").to_string()))
                            .style(crate::ui::widgets::tertiary_button)
                            .on_press(Message::CancelStop),
                    ]
                    .spacing(design::space::SMALL),
                ]
                .spacing(design::space::LARGE),
            )
            .padding(design::space::LARGE as u16)
            .width(460)
            .style(super::widgets::surface);
            return iced::widget::stack![
                body,
                iced::widget::opaque(container(Space::new().width(Fill).height(Fill)).style(
                    |_| {
                        container::Style {
                            background: Some(iced::Color::from_rgba(0.0, 0.0, 0.0, 0.45).into()),
                            ..Default::default()
                        }
                    }
                )),
                container(iced::widget::opaque(dialog))
                    .padding(design::space::MEDIUM as u16)
                    .center_x(Fill)
                    .center_y(Fill),
            ]
            .into();
        }
        if let Some(selection) = &self.selected {
            let eligible = selection.processes.iter().any(|p| !inaccessible(p));
            let tree = selection.has_tree(&self.processes);
            let stop_label = if tree {
                "process_list.stop_process_tree"
            } else {
                "process_list.stop_process"
            };
            let suspendable = selection.processes.iter().any(|p| {
                !inaccessible(p)
                    && p.session_id.is_some_and(|s| s != 0)
                    && p.is_service_account == Some(false)
                    && !crate::app_suspension::is_builtin_excluded(&p.name)
            });
            let suspended = selection.processes.iter().any(|p| {
                status
                    .feature_status
                    .app_suspension
                    .suspended_process_ids
                    .contains(&p.id)
            });
            let suspension_label = if suspended {
                "process_list.resume_process"
            } else {
                "process_list.suspend_process"
            };
            if let Some(position) = self.context {
                return iced::widget::stack![
                    body,
                    responsive(move |_size| {
                        let efficiency = button(
                            row![
                                text(t!("process_list.efficiency_mode").to_string()).width(Fill),
                                super::navigation::glyph("icons/chevron-right.svg"),
                            ]
                            .spacing(design::space::SMALL)
                            .align_y(iced::Center),
                        )
                        .width(Fill)
                        .style(super::widgets::quiet)
                        .on_press(Message::MenuBranch(MenuBranch::Efficiency));
                        let mut menu = column![
                            mouse_area(efficiency)
                                .on_enter(Message::MenuBranch(MenuBranch::Efficiency)),
                            mouse_area(
                                button(
                                    row![
                                        text(t!("nav.priority_control").to_string()).width(Fill),
                                        super::navigation::glyph("icons/chevron-right.svg")
                                    ]
                                    .spacing(design::space::SMALL)
                                    .align_y(iced::Center)
                                )
                                .width(Fill)
                                .style(super::widgets::quiet)
                                .on_press(Message::MenuBranch(MenuBranch::Types))
                            )
                            .on_enter(Message::MenuBranch(MenuBranch::Types))
                        ]
                        .spacing(design::space::TINY);
                        for (key, message, enabled) in [
                            (
                                suspension_label,
                                Message::Menu(Box::new(Message::Action(Action::Suspend(
                                    !suspended,
                                )))),
                                suspendable,
                            ),
                            (stop_label, Message::Stop(tree), eligible),
                            ("process_list.open_rule_details", Message::OpenDetails, true),
                            (
                                "process_list.open_process_location",
                                Message::Menu(Box::new(Message::OpenLocation)),
                                !selection.path.is_empty(),
                            ),
                            (
                                "process_list.properties",
                                Message::Menu(Box::new(Message::Properties)),
                                !selection.path.is_empty(),
                            ),
                        ] {
                            if key == "process_list.open_rule_details" {
                                menu = menu.push(iced::widget::rule::horizontal(1));
                            }
                            menu = menu.push(
                                mouse_area(
                                    button(text(t!(key).to_string()))
                                        .width(Fill)
                                        .style(move |theme, status| {
                                            let mut style = super::widgets::quiet(theme, status);
                                            if status != iced::widget::button::Status::Disabled {
                                                style.text_color = match key {
                                                    "process_list.suspend_process" => {
                                                        theme
                                                            .extended_palette()
                                                            .warning
                                                            .strong
                                                            .color
                                                    }
                                                    "process_list.resume_process" => {
                                                        if theme.extended_palette().is_dark {
                                                            iced::Color::from_rgb8(0x70, 0xb7, 0xff)
                                                        } else {
                                                            iced::Color::from_rgb8(0x00, 0x67, 0xb0)
                                                        }
                                                    }
                                                    "process_list.stop_process"
                                                    | "process_list.stop_process_tree" => {
                                                        theme.palette().danger
                                                    }
                                                    _ => style.text_color,
                                                };
                                            }
                                            style
                                        })
                                        .on_press_maybe(enabled.then_some(message)),
                                )
                                .on_enter(Message::MenuBranch(MenuBranch::Closed)),
                            );
                        }
                        let panel = |content: Element<'a, Message>| {
                            iced::widget::opaque(
                                container(content)
                                    .padding(design::space::SMALL as u16)
                                    .style(|theme| {
                                        let mut style = super::widgets::surface(theme);
                                        style.border.color =
                                            theme.extended_palette().background.strong.color;
                                        style.border.width = 1.0;
                                        if theme.extended_palette().is_dark {
                                            style.background = Some(
                                                theme
                                                    .extended_palette()
                                                    .background
                                                    .neutral
                                                    .color
                                                    .into(),
                                            );
                                        }
                                        style
                                    }),
                            )
                        };
                        let mut cascade =
                            row![panel(menu.width(210).into())].spacing(design::space::TINY);
                        if self.menu_branch == MenuBranch::Efficiency {
                            let mut options = column![].spacing(design::space::TINY);
                            for (enabled, key) in
                                [(true, "common.enabled"), (false, "common.disabled")]
                            {
                                options = options.push(
                                    button(text(t!(key).to_string()))
                                        .width(Fill)
                                        .style(if self.efficiency == Some(enabled) {
                                            super::widgets::selected
                                        } else {
                                            super::widgets::quiet
                                        })
                                        .on_press_maybe(eligible.then_some(Message::Menu(
                                            Box::new(Message::Action(Action::Efficiency(enabled))),
                                        ))),
                                );
                            }
                            cascade = cascade.push(panel(options.width(100).into()));
                        }
                        if matches!(self.menu_branch, MenuBranch::Types | MenuBranch::Levels(_)) {
                            let mut types = column![].spacing(design::space::TINY);
                            for kind in [
                                Kind::Process,
                                Kind::Thread,
                                Kind::Io,
                                Kind::Gpu,
                                Kind::Memory,
                                Kind::DynamicBoost,
                            ] {
                                let key = format!("nav.{}", kind.key());
                                types = types.push(
                                    mouse_area(
                                        button(
                                            row![
                                                text(t!(&key).to_string())
                                                    .wrapping(iced::widget::text::Wrapping::None)
                                                    .width(Fill),
                                                container(super::navigation::glyph(
                                                    "icons/chevron-right.svg"
                                                ))
                                                .width(design::ICON_SIZE)
                                            ]
                                            .spacing(design::space::SMALL)
                                            .align_y(iced::Center),
                                        )
                                        .width(Fill)
                                        .style(if self.menu_branch == MenuBranch::Levels(kind) {
                                            super::widgets::selected
                                        } else {
                                            super::widgets::quiet
                                        })
                                        .on_press(Message::MenuBranch(MenuBranch::Levels(kind))),
                                    )
                                    .on_enter(Message::MenuBranch(MenuBranch::Levels(kind))),
                                );
                            }
                            cascade = cascade.push(panel(types.width(210).into()));
                            if let MenuBranch::Levels(kind) = self.menu_branch {
                                let mut levels = column![].spacing(design::space::TINY);
                                for value in kind
                                    .choices(settings.advanced.expose_all_priority_values)
                                    .into_iter()
                                    .filter(|value| !is_default(*value))
                                {
                                    levels = levels.push(
                                        button(text(value.to_string()))
                                            .width(Fill)
                                            .style(if self.current.contains(&value) {
                                                super::widgets::selected
                                            } else {
                                                super::widgets::quiet
                                            })
                                            .on_press_maybe(eligible.then_some(Message::Menu(
                                                Box::new(Message::Action(Action::Priority(value))),
                                            ))),
                                    );
                                }
                                cascade = cascade.push(panel(
                                    scrollable(levels)
                                        .width(if kind == Kind::DynamicBoost { 100 } else { 160 })
                                        .height(iced::Shrink)
                                        .into(),
                                ));
                            }
                        }
                        iced::widget::float(cascade)
                            .translate(move |bounds, viewport| {
                                let desired = iced::Point::new(
                                    bounds.x + position.x - viewport.x,
                                    bounds.y + position.y - viewport.y,
                                );
                                let point = popup_position(desired, viewport.size(), bounds.size());
                                iced::Vector::new(
                                    viewport.x + point.x - bounds.x,
                                    viewport.y + point.y - bounds.y,
                                )
                            })
                            .into()
                    })
                ]
                .into();
            }
            let icon: Element<'_, Message> = selection
                .processes
                .first()
                .and_then(|p| p.image_path.as_ref())
                .and_then(|path| self.icons.get(path))
                .and_then(Option::as_ref)
                .map(|icon| image((**icon).clone()).width(20).height(20).into())
                .unwrap_or_else(|| super::navigation::glyph("icons/app-window.svg"));
            let header = container(
                row![
                    icon,
                    column![
                        text(&selection.name),
                        text(&selection.path)
                            .size(design::typography::SECONDARY)
                            .style(iced::widget::text::secondary),
                    ]
                    .width(Fill)
                    .spacing(design::space::TIGHT),
                    button(text("\u{00d7}")).on_press(Message::CloseSelection),
                ]
                .spacing(design::space::MEDIUM)
                .align_y(iced::Center),
            )
            .padding(design::space::LARGE as u16);
            let mut tabs = row![].spacing(design::space::SMALL);
            for (tab, key) in [
                (ProcessTab::Immediate, "process_list.immediate_actions"),
                (ProcessTab::Rulesets, "process_list.rulesets"),
            ] {
                tabs = tabs.push(
                    button(text(t!(key).to_string()))
                        .style(if self.process_tab == tab {
                            super::widgets::selected_control
                        } else {
                            super::widgets::quiet
                        })
                        .on_press(Message::ProcessTab(tab)),
                );
            }
            let mut pane = column![].spacing(super::widgets::CARD_GAP);
            match self.process_tab {
                ProcessTab::Rulesets => {
                    if eligible && !selection.path.is_empty() {
                        pane = pane.push(
                            details::view(settings, &selection.path, plans).map(Message::Details),
                        );
                    } else {
                        pane = pane.push(text(t!("process_list.status_access_denied").to_string()));
                    }
                }
                ProcessTab::Immediate => {
                    let efficiency: Element<'_, Message> = match self.efficiency {
                        Some(enabled) => super::widgets::switch(
                            enabled,
                            eligible.then_some(|value| Message::Action(Action::Efficiency(value))),
                        ),
                        None => text(t!("common.unknown").to_string()).into(),
                    };
                    pane = pane
                        .push(super::widgets::settings_card(super::widgets::setting_row(
                            "process_list.efficiency_mode",
                            mouse_area(efficiency)
                                .on_enter(Message::MenuBranch(MenuBranch::Closed)),
                        )))
                        .push(super::widgets::heading(
                            t!("nav.priority_control").to_string(),
                            design::typography::BODY,
                        ))
                        .push(self.immediate_priorities(settings, eligible))
                        .push(
                            row![
                                button(text(t!(suspension_label).to_string())).on_press_maybe(
                                    suspendable
                                        .then_some(Message::Action(Action::Suspend(!suspended)))
                                ),
                                button(text(t!("process_list.open_process_location").to_string()))
                                    .on_press_maybe(
                                        (!selection.path.is_empty())
                                            .then_some(Message::OpenLocation)
                                    ),
                                Space::new().width(Fill),
                                button(text(t!(stop_label).to_string()))
                                    .style(super::widgets::danger_button)
                                    .on_press_maybe(eligible.then_some(Message::Stop(tree))),
                            ]
                            .spacing(design::space::SMALL)
                            .align_y(iced::Center),
                        );
                }
            }
            return iced::widget::stack![
                body,
                mouse_area(container(Space::new().width(Fill).height(Fill)).style(|_| {
                    container::Style {
                        background: Some(iced::Color::from_rgba(0.0, 0.0, 0.0, 0.45).into()),
                        ..Default::default()
                    }
                }))
                .on_press(Message::CloseSelection)
                .on_right_press(Message::CloseSelection),
                container(iced::widget::opaque(
                    container(column![
                        header,
                        iced::widget::rule::horizontal(1),
                        container(tabs)
                            .padding([design::space::SMALL as u16, design::space::LARGE as u16]),
                        scrollable(container(pane).padding(design::space::LARGE as u16))
                            .height(Fill),
                    ])
                    .width(Fill)
                    .max_width(960)
                    .height(Fill)
                    .max_height(680)
                    .style(|theme| container::Style {
                        background: Some(theme.palette().background.into()),
                        border: iced::Border {
                            color: theme.extended_palette().background.strong.color,
                            width: 1.0,
                            radius: design::CARD_RADIUS.into(),
                        },
                        ..Default::default()
                    })
                ))
                .padding(design::space::MEDIUM as u16)
                .center_x(Fill)
                .center_y(Fill),
            ]
            .into();
        }
        // Keep the list subtree stable when opening or closing either overlay.
        iced::widget::stack![body].into()
    }
}
fn popup_position(position: iced::Point, bounds: iced::Size, popup: iced::Size) -> iced::Point {
    iced::Point::new(
        position.x.clamp(0.0, (bounds.width - popup.width).max(0.0)),
        position
            .y
            .clamp(0.0, (bounds.height - popup.height).max(0.0)),
    )
}

fn value_kind(v: Value) -> Kind {
    match v {
        Value::Process(_) => Kind::Process,
        Value::Thread(_) => Kind::Thread,
        Value::Io(_) => Kind::Io,
        Value::Gpu(_) => Kind::Gpu,
        Value::Memory(_) => Kind::Memory,
        Value::DynamicBoost(_) => Kind::DynamicBoost,
    }
}
fn is_default(v: Value) -> bool {
    matches!(
        v,
        Value::Process(ProcessPrioritySetting::Default)
            | Value::Thread(ProcessThreadPrioritySetting::Default)
            | Value::Io(ProcessIoPrioritySetting::Default)
            | Value::Gpu(ProcessGpuPrioritySetting::Default)
            | Value::Memory(ProcessMemoryPrioritySetting::Default)
            | Value::DynamicBoost(ProcessDynamicPriorityBoostSetting::Default)
    )
}
fn snapshot_target(p: &ProcessInfo) -> Result<ProcessActionTarget, ProcessActionTargetError> {
    if protected(p) || p.id == std::process::id() {
        return Err(ProcessActionTargetError::ProtectedProcess);
    }
    let path = p
        .image_path
        .clone()
        .filter(|p| p.is_absolute())
        .ok_or(ProcessActionTargetError::IdentityUnavailable)?;
    Ok(ProcessActionTarget {
        id: p.id,
        name: p.name.clone(),
        executable_path: path,
        creation_time: p
            .creation_time
            .ok_or(ProcessActionTargetError::IdentityUnavailable)?,
        session_id: p.session_id,
        is_service_account: p.is_service_account,
    })
}
fn status_cell(key: &str) -> Element<'static, Message> {
    let (icon, dark, light) = match key {
        "process_list.status_suspended" => ("icons/pause.svg", 0xe8b45b, 0x886000),
        "process_list.status_efficiency_mode" => ("icons/leaf.svg", 0xa4db61, 0x477d23),
        "process_list.status_active" => ("icons/play.svg", 0x70b7ff, 0x0067b0),
        "process_list.status_system_protected" => ("icons/shield.svg", 0x9aa0a6, 0x666666),
        "process_list.status_access_denied" => ("icons/ban.svg", 0x9aa0a6, 0x666666),
        _ => ("icons/circle-help.svg", 0x9aa0a6, 0x666666),
    };
    let color = move |theme: &iced::Theme| {
        let rgb = if theme.extended_palette().is_dark {
            dark
        } else {
            light
        };
        iced::Color::from_rgb8((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
    };
    let label = t!(key);
    let cell = row![
        iced::widget::svg(crate::ui::assets::iced_icon(icon).expect("Status icon is bundled"))
            .width(design::ICON_SIZE)
            .height(design::ICON_SIZE)
            .style(move |theme, _| iced::widget::svg::Style {
                color: Some(color(theme))
            }),
        text(label.to_string())
            .wrapping(iced::widget::text::Wrapping::None)
            .style(move |theme| text::Style {
                color: Some(color(theme))
            }),
    ]
    .spacing(design::space::CONTROL)
    .align_y(iced::Center);
    cell.into()
}

fn protected(p: &ProcessInfo) -> bool {
    p.is_critical == Some(true)
        || foreground::contains_process_name(foreground::CORE_BUILT_IN_PROCESS_EXCLUSIONS, &p.name)
}
fn inaccessible(p: &ProcessInfo) -> bool {
    p.image_path.is_none()
        || p.id == 0
        || p.id == std::process::id()
        || !p.can_set_information
        || p.is_critical.is_none()
        || protected(p)
}
fn format_memory_usage(bytes: u64, total: Option<u64>, percentage: bool) -> String {
    if percentage {
        match total.filter(|total| *total > 0) {
            Some(total) => format!("{:.1}%", bytes as f64 / total as f64 * 100.0),
            None => t!("common.unknown").to_string(),
        }
    } else {
        format!("{:.1} MiB", bytes as f64 / 1048576.0)
    }
}

fn measure_column_text(content: &str) -> f32 {
    use iced::advanced::text::{Paragraph, Renderer, Text};
    <iced::Renderer as Renderer>::Paragraph::with_text(Text {
        content,
        bounds: iced::Size::INFINITE,
        size: (design::typography::BODY as f32).into(),
        line_height: Default::default(),
        font: design::typography::FONT,
        align_x: Default::default(),
        align_y: iced::alignment::Vertical::Center,
        shaping: iced::advanced::text::Shaping::Advanced,
        wrapping: iced::advanced::text::Wrapping::None,
    })
    .min_width()
}

fn column_width(c: Sort) -> f32 {
    match c {
        Sort::Name => 230.0,
        Sort::Pid => 70.0,
        Sort::Cpu | Sort::Memory => 100.0,
        Sort::Status => 180.0,
        Sort::User => 200.0,
    }
}
fn visible_range_offsets(offsets: &[f32], offset: f32, height: f32) -> Range<usize> {
    let count = offsets.len().saturating_sub(1);
    let offset = offset.max(0.0);
    let start = offsets
        .partition_point(|p| *p <= offset)
        .saturating_sub(1)
        .min(count)
        .saturating_sub(8);
    let end = offsets
        .partition_point(|p| *p < offset + height.max(0.0))
        .saturating_add(9)
        .min(count);
    start..end.max(start)
}
#[cfg(test)]
fn visible_range(count: usize, offset: f32, height: f32) -> Range<usize> {
    let offsets = (0..=count)
        .map(|i| i as f32 * ROW_HEIGHT)
        .collect::<Vec<_>>();
    visible_range_offsets(&offsets, offset, height)
}

fn background<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
    map: impl Fn(Result<T, String>) -> Message + Send + 'static,
) -> Task<Message> {
    super::tasks::run(work).map(move |result| map(result.and_then(|value| value)))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stop_action_uses_tree_for_groups_or_children_but_not_reused_parent_ids() {
        let parent = process(10, 100, "parent.exe");
        let mut child = process(11, 200, "child.exe");
        child.parent_id = Some(parent.id);
        let mut selection = Selection {
            processes: vec![parent.clone()],
            path: String::new(),
            name: parent.name.clone(),
        };
        assert!(!selection.has_tree(std::slice::from_ref(&parent)));
        assert!(selection.has_tree(&[parent.clone(), child.clone()]));
        child.creation_time = Some(50);
        assert!(!selection.has_tree(&[parent.clone(), child.clone()]));
        child.creation_time = None;
        assert!(selection.has_tree(&[parent.clone(), child.clone()]));
        selection.processes.push(child);
        assert!(selection.has_tree(&[parent]));
    }

    #[test]
    fn selection_overlays_preserve_the_list_widget_tree() {
        let settings = Settings::default();
        let status = RuntimeStatusSnapshot::default();
        let mut list = ProcessList::default();
        let view = list.view(&settings, &status, &[]);
        let mut tree = iced::advanced::widget::Tree::new(&view);
        let root_tag = tree.tag;
        let list_tag = tree.children[0].tag;
        drop(view);
        list.selected = Some(Selection {
            processes: vec![process(10, 100, "app.exe")],
            path: String::new(),
            name: "app.exe".into(),
        });
        for context in [Some(iced::Point::ORIGIN), None] {
            list.context = context;
            let view = list.view(&settings, &status, &[]);
            tree.diff(&view);
            assert_eq!(tree.tag, root_tag);
            assert_eq!(tree.children[0].tag, list_tag);
        }
        list.selected = None;
        tree.diff(list.view(&settings, &status, &[]));
        assert_eq!(tree.tag, root_tag);
        assert_eq!(tree.children[0].tag, list_tag);
    }

    #[test]
    fn status_sort_matches_group_labels_and_updates_when_suspension_changes() {
        let mut list = ProcessList {
            processes: vec![
                process(10, 1, "a.exe"),
                process(11, 1, "a.exe"),
                process(12, 1, "b.exe"),
                process(13, 1, "c.exe"),
                process(14, 1, "d.exe"),
                process(15, 1, "e.exe"),
            ],
            sort: Sort::Status,
            hide_inaccessible: false,
            ..ProcessList::default()
        };
        list.processes[4].can_set_information = false;
        list.processes[5].is_critical = Some(true);
        list.processes.push(process(16, 1, "f.exe"));
        list.processes[6].image_path = None;
        assert_eq!(
            list.status_key(&Entry {
                key: "pid:16".into(),
                indices: vec![6],
                nested: false,
            }),
            "process_list.status_unavailable"
        );
        list.processes.pop();
        list.samples.insert(
            12,
            ProcessResourceSample {
                cpu: crate::cpu::ProcessCpuSample {
                    cpu_time_100ns: 0,
                    sampled_at: std::time::Instant::now(),
                },
                creation_time: 1,
                working_set_bytes: None,
                efficiency_mode: Some(true),
            },
        );
        list.sync_suspended_processes(&[11]);
        let ids = |list: &ProcessList| {
            list.rows
                .iter()
                .map(|row| list.processes[row.indices[0]].id)
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(&list), vec![14, 13, 12, 10, 15]);
        list.descending = true;
        list.rebuild();
        assert_eq!(ids(&list), vec![15, 10, 12, 13, 14]);
        list.sync_suspended_processes(&[]);
        assert_eq!(ids(&list), vec![15, 12, 10, 13, 14]);
    }

    #[test]
    fn focusing_a_row_preserves_exact_targets_without_opening_details() {
        let mut list = ProcessList {
            processes: vec![process(10, 100, "app.exe"), process(11, 200, "app.exe")],
            ..ProcessList::default()
        };
        list.rebuild();
        let revision = list.rows_revision;
        list.focus_row(0, revision);
        assert!(list.selected.is_none());
        let (_, nested, selection) = list.focused.as_ref().unwrap();
        assert!(!nested);
        assert_eq!(selection.processes.len(), 2);
        assert_eq!(selection.processes[0].creation_time, Some(100));
        list.processes[0].creation_time = Some(300);
        list.rebuild();
        list.focus_row(0, revision);
        assert_eq!(
            list.focused.as_ref().unwrap().2.processes[0].creation_time,
            Some(100)
        );
        list.clear();
        assert!(list.focused.is_none());
    }

    #[test]
    fn column_resizing_clamps_width_and_preserves_it_across_refreshes() {
        let mut list = ProcessList {
            resizing: Some((Sort::Status, 180.0, None)),
            ..ProcessList::default()
        };
        list.resize_column(300.0);
        list.resize_column(350.0);
        assert_eq!(list.column_width(Sort::Status), 230.0);
        list.resize_column(-500.0);
        assert_eq!(list.column_width(Sort::Status), 140.0);
        list.resize_column(320.0);
        assert_eq!(list.column_width(Sort::Status), 200.0);
        list.resizing = None;
        list.resize_column(400.0);
        list.rebuild();
        assert_eq!(list.column_width(Sort::Status), 200.0);
        assert_eq!(list.column_width(Sort::Memory), column_width(Sort::Memory));
    }

    #[test]
    fn last_visible_column_fills_spare_width_and_auto_size_fits_names() {
        let mut list = ProcessList::default();
        let widths = list.layout_widths(1200.0);
        assert_eq!(widths.iter().sum::<f32>() + 5.0 * 8.0 + 32.0, 1200.0);
        list.columns[Sort::User as usize] = false;
        let widths = list.layout_widths(1200.0);
        assert_eq!(widths[..5].iter().sum::<f32>() + 4.0 * 8.0 + 32.0, 1200.0);
        assert_eq!(widths[Sort::Name as usize], column_width(Sort::Name));
        let mut app = process(10, 100, "app.exe");
        app.name = "a_very_long_executable_name_that_must_fit_in_the_column.exe".into();
        list.processes.push(app);
        list.rebuild();
        let fitted = list.auto_width(Sort::Name);
        assert!(fitted >= measure_column_text(&list.processes[0].name) + 56.0);
    }

    #[test]
    fn initial_population_fits_columns_and_refresh_preserves_manual_widths() {
        let mut list = ProcessList::default();
        list.rebuild();
        assert_eq!(list.column_widths, [None; 6]);
        list.processes.push(process(10, 100, "app.exe"));
        list.rebuild();
        for col in Sort::ALL {
            assert_eq!(list.column_width(col), list.auto_width(col));
        }
        list.column_widths[Sort::Name as usize] = Some(400.0);
        list.rebuild();
        assert_eq!(list.column_width(Sort::Name), 400.0);
    }

    #[test]
    fn memory_metrics_show_value_or_share_of_physical_memory() {
        assert_eq!(
            format_memory_usage(1048576, Some(4194304), false),
            "1.0 MiB"
        );
        assert_eq!(format_memory_usage(1048576, Some(4194304), true), "25.0%");
        assert_eq!(format_memory_usage(0, Some(4194304), true), "0.0%");
        assert_eq!(
            format_memory_usage(1, Some(0), true),
            t!("common.unknown").to_string()
        );
    }

    #[test]
    fn context_menu_stays_inside_the_viewport() {
        let bounds = iced::Size::new(640.0, 480.0);
        let menu = iced::Size::new(320.0, 300.0);
        assert_eq!(
            popup_position(iced::Point::new(630.0, 470.0), bounds, menu),
            iced::Point::new(320.0, 180.0)
        );
        assert_eq!(
            popup_position(iced::Point::new(20.0, 30.0), bounds, menu),
            iced::Point::new(20.0, 30.0)
        );
        assert_eq!(
            popup_position(
                iced::Point::new(-5.0, -5.0),
                iced::Size::new(100.0, 100.0),
                menu
            ),
            iced::Point::ORIGIN
        );
    }

    #[test]
    fn action_feedback_retains_the_original_selection() {
        let mut list = ProcessList {
            selected: Some(Selection {
                processes: vec![],
                path: String::new(),
                name: "original.exe".into(),
            }),
            ..Default::default()
        };
        let complete = list.result_message();
        list.selected = None;
        assert!(matches!(complete(Ok(())),Message::Result(name,Ok(())) if name=="original.exe"));
    }
    fn process(id: u32, created: u64, path: &str) -> ProcessInfo {
        ProcessInfo {
            id,
            creation_time: Some(created),
            parent_id: None,
            session_id: Some(1),
            user_name: Some("User".into()),
            is_service_account: Some(false),
            is_critical: Some(false),
            can_set_information: true,
            name: "app.exe".into(),
            image_path: Some(PathBuf::from(path)),
        }
    }
    #[test]
    fn virtualized_rows_bound_large_lists_and_stale_offsets() {
        assert_eq!(visible_range(0, 1000.0, 600.0), 0..0);
        assert!(visible_range(10_000, 120_000.0, 600.0).len() <= 42);
        assert_eq!(visible_range(3, 120_000.0, 600.0), 0..3);
    }
    #[test]
    fn grouping_uses_exact_path_and_expands_members() {
        let mut list = ProcessList {
            processes: vec![
                process(10, 1, r"C:\A\app.exe"),
                process(11, 2, r"C:\A\app.exe"),
                process(12, 3, r"C:\B\app.exe"),
            ],
            ..ProcessList::default()
        };
        list.rebuild();
        assert_eq!(list.offsets, vec![0.0, ROW_HEIGHT, 2.0 * ROW_HEIGHT]);
        let revision = list.rows_revision;
        assert_eq!(list.rows.len(), 2);
        assert_eq!(list.rows[0].indices.len(), 2);
        list.expanded.insert(list.rows[0].key.clone());
        list.rebuild();
        assert_eq!(list.rows.len(), 4);
        assert!(list.rows[1].nested);
        assert_ne!(list.rows_revision, revision);
        assert_eq!(list.offsets.last(), Some(&(4.0 * ROW_HEIGHT)));
        list.clear();
        assert_eq!(list.offsets, vec![0.0]);
    }
    #[test]
    fn expanded_groups_keep_virtual_rows_bounded_and_collapse_immediately() {
        let mut list = ProcessList {
            hide_inaccessible: false,
            processes: (10..1010)
                .map(|id| process(id, u64::from(id), r"C:\A\app.exe"))
                .collect(),
            ..ProcessList::default()
        };
        list.rebuild();
        let key = list.rows[0].key.clone();
        list.expanded.insert(key.clone());
        list.rebuild();
        assert_eq!(list.rows.len(), 1001);
        assert!(visible_range_offsets(&list.offsets, 0.0, 600.0).len() <= 42);
        list.expanded.remove(&key);
        list.rebuild();
        assert_eq!(list.rows.len(), 1);
        assert_eq!(list.offsets, vec![0.0, ROW_HEIGHT]);
    }
    #[test]
    fn selected_targets_retain_creation_time_across_refresh() {
        let old = process(10, 100, r"C:\A\app.exe");
        let selected = Selection {
            processes: vec![old],
            path: r"C:\A\app.exe".into(),
            name: "app.exe".into(),
        };
        let replacement = process(10, 200, r"C:\A\app.exe");
        let roots = selected
            .targets()
            .into_iter()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(roots[0].creation_time, 100);
        assert!(foreground::process_tree_action_targets(&roots, &[replacement]).is_err());
    }
    #[test]
    fn unknown_identity_never_becomes_an_action_target() {
        let mut p = process(10, 100, r"C:\A\app.exe");
        p.creation_time = None;
        assert_eq!(
            snapshot_target(&p),
            Err(ProcessActionTargetError::IdentityUnavailable)
        );
        p.creation_time = Some(100);
        p.is_critical = Some(true);
        assert_eq!(
            snapshot_target(&p),
            Err(ProcessActionTargetError::ProtectedProcess)
        );
    }
}
