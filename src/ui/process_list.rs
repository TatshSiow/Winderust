use super::design;
use super::priority_control::{Kind, Value};
use super::tasks::Task;
use super::widgets::{self, Ui};
use crate::automation::{RuntimeHandle, RuntimeStatusSnapshot};
use crate::config::*;
use crate::foreground::{
    self, ProcessActionTarget, ProcessActionTargetError, ProcessInfo, ProcessResourceSample,
};
use crate::platform::windows::resident_shell::ui_pointer_capture_active;
use rust_i18n::t;
use std::{
    cell::RefCell,
    collections::{BTreeMap, HashMap, HashSet},
    path::PathBuf,
    rc::Rc,
    sync::Arc,
};
use windui::{
    event::MenuItem,
    prelude::{signal, Element, Role, Signal},
};
#[path = "process_details.rs"]
pub(super) mod details;
#[derive(Clone, Copy)]
pub(super) enum PlanCatalog<'a> {
    Loading,
    Unavailable,
    Loaded(&'a [crate::power::PowerPlan]),
}
const ROW_HEIGHT: f32 = 36.0;
#[derive(Debug, Clone)]
pub(super) struct Population {
    processes: Vec<ProcessInfo>,
    samples: BTreeMap<u32, ProcessResourceSample>,
    total_memory_bytes: Option<u64>,
    icons: HashMap<PathBuf, Option<Arc<crate::process_icon::ProcessIcon>>>,
}
#[derive(Debug, Clone)]
struct Entry {
    indices: Vec<usize>,
    key: String,
    nested: bool,
}
#[derive(Debug, Clone)]
pub(super) struct Selection {
    nested: bool,
    processes: Vec<ProcessInfo>,
    path: String,
    name: String,
}
impl Selection {
    fn stop_options(&self, population: &[ProcessInfo]) -> (bool, bool) {
        (
            self.processes.len() == 1,
            self.nested || self.has_tree(population),
        )
    }

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
#[derive(Debug, Clone)]
pub(super) enum Message {
    Noop,
    PausePopulation(bool),
    Refresh,
    Loaded(Result<Population, String>),
    Search(String),
    ColumnWidth(Sort, f32),
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
    RowAction(usize, u64, Box<Message>),
    OpenDetails,
    OpenFocused,
    ProcessTab(ProcessTab),
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
    pub(super) population_paused: bool,
    processes: Vec<ProcessInfo>,
    rows: Vec<Entry>,
    rows_revision: u64,
    samples: BTreeMap<u32, ProcessResourceSample>,
    total_memory_bytes: Option<u64>,
    cpu: HashMap<u32, f32>,
    suspended_process_ids: Vec<u32>,
    icons: HashMap<PathBuf, Option<Arc<crate::process_icon::ProcessIcon>>>,
    search: String,
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
    selected: Option<Selection>,
    focused: Option<(String, bool, Selection)>,
    native: Rc<RefCell<NativeRows>>,
    process_tab: ProcessTab,
    current: Vec<Value>,
    efficiency: Option<bool>,
    stopping: Option<(Selection, bool)>,
}
#[cfg(feature = "render-smoke")]
#[derive(Debug, Clone, PartialEq)]
pub(super) struct SmokeResourceState {
    pub population: usize,
    pub icons: usize,
    pub native_rows: usize,
    pub refreshing: bool,
    pub search: String,
    pub grouped: bool,
    pub sort: Sort,
    pub descending: bool,
    pub column_widths: [Option<f32>; 6],
}
impl Default for ProcessList {
    fn default() -> Self {
        Self {
            processes: Vec::new(),
            rows: Vec::new(),
            rows_revision: 0,
            samples: BTreeMap::new(),
            total_memory_bytes: None,
            cpu: HashMap::new(),
            suspended_process_ids: Vec::new(),
            icons: HashMap::new(),
            search: String::new(),
            refreshing: false,
            population_paused: false,
            error: None,
            hide_inaccessible: true,
            grouped: true,
            memory_percentage: false,
            expanded: HashSet::new(),
            sort: Sort::Name,
            descending: false,
            columns: [true; 6],
            column_widths: [None; 6],
            selected: None,
            focused: None,
            native: Rc::new(RefCell::new(NativeRows::default())),
            process_tab: ProcessTab::Immediate,
            current: Vec::new(),
            efficiency: None,
            stopping: None,
        }
    }
}
impl ProcessList {
    #[cfg(feature = "render-smoke")]
    pub(super) fn smoke_resources(&self) -> SmokeResourceState {
        SmokeResourceState {
            population: self.processes.len(),
            icons: self.icons.len(),
            native_rows: self.native.borrow().rows.len(),
            refreshing: self.refreshing,
            search: self.search.clone(),
            grouped: self.grouped,
            sort: self.sort,
            descending: self.descending,
            column_widths: self.column_widths,
        }
    }

    #[cfg(feature = "render-smoke")]
    pub(super) fn smoke_population_result(&self) -> Population {
        Population {
            processes: self.processes.clone(),
            samples: self.samples.clone(),
            total_memory_bytes: self.total_memory_bytes,
            icons: self.icons.clone(),
        }
    }

    /// Synthetic targets are only rendered; smoke fixtures never dispatch OS actions.
    #[cfg(feature = "render-smoke")]
    pub(super) fn smoke_population(&mut self) {
        self.processes = (0..24)
            .map(|index| ProcessInfo {
                id: 900_000 + index,
                creation_time: Some(10_000 + u64::from(index)),
                parent_id: None,
                session_id: Some(1),
                user_name: Some("Smoke user".into()),
                is_service_account: Some(false),
                is_critical: Some(false),
                can_set_information: true,
                name: format!("Example {:02}.exe", index + 1),
                image_path: Some(PathBuf::from(format!(
                    r"C:\Apps\Example{}\example.exe",
                    index + 1
                ))),
            })
            .collect();
        self.refreshing = false;
        self.icons = self
            .processes
            .iter()
            .filter_map(|process| {
                process.image_path.clone().map(|path| {
                    (
                        path,
                        Some(Arc::new(crate::process_icon::ProcessIcon {
                            width: 16,
                            height: 16,
                            rgba: (0..256)
                                .flat_map(|pixel| {
                                    if (pixel / 16 + pixel % 16) % 2 == 0 {
                                        [30, 160, 220, 255]
                                    } else {
                                        [80, 210, 160, 255]
                                    }
                                })
                                .collect(),
                        })),
                    )
                })
            })
            .collect();
        self.rebuild();
    }

    #[cfg(feature = "render-smoke")]
    pub(super) fn smoke_selection(&self) -> Message {
        Message::Select(0, self.rows_revision)
    }

    pub(super) fn discard(&mut self, message: Message) {
        if matches!(message, Message::Loaded(_)) {
            self.refreshing = false;
        }
        self.clear();
    }
    /// A late query may finish after the window closes; keep any paused snapshot.
    pub(super) fn discard_population_result(&mut self) {
        self.refreshing = false;
    }
    pub(super) fn clear(&mut self) {
        self.rows_revision = self.rows_revision.wrapping_add(1);
        self.processes.clear();
        self.rows.clear();
        self.samples.clear();
        self.cpu.clear();
        self.icons.clear();
        self.expanded.clear();
        self.focused = None;
        self.selected = None;
        self.stopping = None;
        self.refresh_view();
    }
    /// Release data that can be rebuilt when the settings window returns.
    /// A paused population is a user-owned snapshot and must survive hiding.
    pub(super) fn release_presentation(&mut self) {
        *self.native.borrow_mut() = NativeRows::default();
        if self.population_paused {
            return;
        }
        self.rows_revision = self.rows_revision.wrapping_add(1);
        self.processes = Vec::new();
        self.rows = Vec::new();
        self.samples = BTreeMap::new();
        self.cpu = HashMap::new();
        self.icons = HashMap::new();
        self.total_memory_bytes = None;
    }
    pub(super) fn update(
        &mut self,
        message: Message,
        settings: &mut Settings,
        runtime: &RuntimeHandle,
    ) -> Task<Message> {
        match message {
            Message::Noop => {}
            Message::PausePopulation(value) => self.population_paused = value,
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
                self.rebuild();
            }
            Message::ColumnWidth(col, width) => {
                self.column_widths[col as usize] = Some(width.max(minimum_column_width(col)));
            }
            Message::AutoSize(col) => {
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
                return task;
            }
            Message::ProcessTab(tab) => self.process_tab = tab,
            Message::OpenDetails => {
                self.process_tab = ProcessTab::Rulesets;
            }
            Message::RowAction(index, revision, action) => {
                if revision != self.rows_revision || index >= self.rows.len() {
                    return Task::none();
                }
                let selected = self.update(Message::Select(index, revision), settings, runtime);
                let action = self.update(*action, settings, runtime);
                return Task::batch([selected, action]);
            }
            Message::OpenFocused => {
                if let Some((_, _, selection)) = &self.focused {
                    self.selected = Some(selection.clone());
                    self.process_tab = ProcessTab::Rulesets;
                }
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
                    nested: entry.nested,
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
    fn result_message(&self) -> impl Fn(Result<(), String>) -> Message + Send + Sync + 'static {
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
                // Native table widths are relative weights; reserve room for full process IDs.
                Sort::Pid => 76.0,
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
                    nested: entry.nested,
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
    /// Update native rows and metadata while keeping scroll and search input alive.
    pub(super) fn refresh_view(&self) {
        if ui_pointer_capture_active() {
            self.native.borrow_mut().refresh_pending = true;
            return;
        }
        let columns = Sort::ALL
            .into_iter()
            .filter(|col| *col == Sort::Name || self.columns[*col as usize])
            .collect::<Vec<_>>();
        let mut children = HashMap::<u32, Vec<Option<u64>>>::new();
        for process in &self.processes {
            if let Some(parent) = process.parent_id.filter(|parent| *parent != process.id) {
                children
                    .entry(parent)
                    .or_default()
                    .push(process.creation_time);
            }
        }
        let rows = self
            .rows
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                let process = &self.processes[entry.indices[0]];
                let selection = Selection {
                    nested: entry.nested,
                    processes: entry
                        .indices
                        .iter()
                        .map(|index| self.processes[*index].clone())
                        .collect(),
                    path: process
                        .image_path
                        .as_ref()
                        .map(|path| path.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    name: process.name.clone(),
                };
                let has_tree = selection.nested
                    || selection.processes.len() > 1
                    || selection.processes.iter().any(|parent| {
                        children.get(&parent.id).is_some_and(|children| {
                            children
                                .iter()
                                .any(|child| match (parent.creation_time, *child) {
                                    (Some(parent), Some(child)) => child >= parent,
                                    _ => true,
                                })
                        })
                    });
                let stop_options = (selection.processes.len() == 1, has_tree);
                NativeRow {
                    index,
                    status: self.status_key(entry),
                    revision: self.rows_revision,
                    key: entry.key.clone(),
                    expanded: self.expanded.contains(&entry.key),
                    selection,
                    icon: process
                        .image_path
                        .as_ref()
                        .and_then(|path| self.icons.get(path))
                        .and_then(Clone::clone),
                    suspended: entry.indices.iter().any(|index| {
                        self.suspended_process_ids
                            .contains(&self.processes[*index].id)
                    }),
                    stop_options,
                }
            })
            .collect::<Vec<_>>();
        let values = self
            .rows
            .iter()
            .map(|entry| {
                columns
                    .iter()
                    .map(|col| {
                        let value = self.cell_value(entry, *col);
                        if *col == Sort::Status {
                            t!(&value).to_string()
                        } else {
                            value
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let mut native = self.native.borrow_mut();
        native.refresh_pending = false;
        native.rows = rows;
        native.columns = columns;
        native.values = values.clone();
        if let Some(rows) = native.signal.filter(Signal::is_alive) {
            rows.set(values);
        }
        if let Some(count) = native.count.filter(Signal::is_alive) {
            count.set(t!("process_list.count", count = self.processes.len()).to_string());
        }
        if let Some(error) = native.error.filter(Signal::is_alive) {
            error.set(self.error.iter().cloned().collect());
        }
        if let Some(actions) = native.actions.filter(Signal::is_alive) {
            let focused = self.focused.as_ref().and_then(|(key, nested, selection)| {
                native.rows.iter().find(|row| {
                    row.key == *key
                        && row.selection.nested == *nested
                        && row
                            .selection
                            .processes
                            .iter()
                            .map(|p| (p.id, p.creation_time))
                            .eq(selection.processes.iter().map(|p| (p.id, p.creation_time)))
                })
            });
            actions.set(focused.map_or([false; 3], |row| {
                [
                    true,
                    !row.selection.nested && row.selection.processes.len() > 1,
                    self.selected.is_none()
                        && self.stopping.is_none()
                        && row.selection.processes.iter().any(|p| !inaccessible(p)),
                ]
            }));
        }
    }

    pub(super) fn flush_pending_refresh(&self) {
        if self.native.borrow().refresh_pending {
            self.refresh_view();
        }
    }

    fn immediate_priorities(
        &self,
        settings: &Settings,
        eligible: bool,
        ui: Ui<Message>,
    ) -> Element {
        let mut controls = widgets::col();
        for kind in [
            Kind::Process,
            Kind::Thread,
            Kind::Io,
            Kind::Gpu,
            Kind::Memory,
            Kind::DynamicBoost,
        ] {
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
            // A missing query result remains explicitly unknown; choosing a default
            // value here would falsely imply the process already had that value.
            let mut values = vec![widgets::Choice(None, t!("common.unknown").to_string())];
            values.extend(
                choices
                    .into_iter()
                    .map(|v| widgets::Choice(Some(v), v.to_string())),
            );
            let selected = widgets::Choice(
                current,
                current
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| t!("common.unknown").to_string()),
            );
            let control = ui
                .select(values, selected, move |value| match value.0 {
                    Some(value) => Message::Action(Action::Priority(value)),
                    None => Message::Noop,
                })
                .enabled(eligible);
            let key = format!("nav.{}", kind.key());
            controls = controls.child(widgets::setting_row(t!(&key).to_string(), control));
            if let Some(key) = current.and_then(Value::warning) {
                controls = controls.child(widgets::help(key));
            }
        }
        controls
    }

    pub(super) fn side_panel(&self, ui: Ui<Message>) -> Element {
        let mut controls = widgets::col()
            .child(widgets::heading(t!("process_list.filter").to_string()))
            .child(ui.checkbox(
                t!("process_list.hide_inaccessible_processes").to_string(),
                self.hide_inaccessible,
                Message::HideInaccessible,
            ))
            .child(widgets::heading(t!("process_list.grouping").to_string()))
            .child(ui.checkbox(
                t!("process_list.group_by_app").to_string(),
                self.grouped,
                Message::Group,
            ))
            .child(widgets::heading(t!("process_list.columns").to_string()));
        for column in Sort::ALL {
            let title = if column == Sort::Name {
                widgets::label(column.label()).font_weight(600)
            } else {
                ui.checkbox(
                    column.label(),
                    self.columns[column as usize],
                    move |value| Message::Column(column, value),
                )
            };
            let width = self.column_width(column).round() as u32;
            let widths = (80u32..=1200)
                .step_by(20)
                .chain(std::iter::once(width))
                .collect::<Vec<_>>();
            controls = controls.child(
                widgets::col().spacing(6).children([
                    title,
                    widgets::row().children([
                        ui.select(widths, width, move |width| {
                            Message::ColumnWidth(column, width as f32)
                        })
                        .weight(1.0),
                        ui.button(
                            t!("process_list.column_auto_size").to_string(),
                            Message::AutoSize(column),
                        ),
                    ]),
                ]),
            );
        }
        let value = t!("process_list.metric_value").to_string();
        let percentage = t!("process_list.metric_percentage").to_string();
        let selected = if self.memory_percentage {
            percentage.clone()
        } else {
            value.clone()
        };
        controls = controls
            .child(widgets::heading(t!("process_list.metrics").to_string()))
            .child(widgets::col().spacing(6).children([
                widgets::label(t!("process_list.memory_usage").to_string()),
                ui.select([value, percentage.clone()], selected, move |value| {
                    Message::MemoryPercentage(value == percentage)
                }),
            ]));
        super::scrolling::scrollable(controls.padding(12)).fill()
    }

    pub(super) fn view(
        &self,
        settings: &Settings,
        _status: &RuntimeStatusSnapshot,
        _plans: PlanCatalog<'_>,
        ui: Ui<Message>,
    ) -> Element {
        let count = signal(t!("process_list.count", count = self.processes.len()).to_string());
        let error = signal(self.error.iter().cloned().collect::<Vec<_>>());
        let available_actions = signal([false; 3]);
        {
            let mut native = self.native.borrow_mut();
            native.count = Some(count);
            native.error = Some(error);
            native.actions = Some(available_actions);
        }
        self.refresh_view();
        let mut body = widgets::col().fill().padding(20);
        let controls = widgets::col()
            .child(ui.input(
                self.search.clone(),
                t!("process_list.search_placeholder").to_string(),
                Message::Search,
            ))
            .child(
                Element::flow()
                    .width_match()
                    .spacing(8)
                    .run_spacing(8)
                    .children([
                        Element::label(count)
                            .font_size(design::typography::SECONDARY as f32)
                            .fg_role(Role::TextMuted),
                        ui.button(
                            t!(if self.population_paused {
                                "process_list.resume_population"
                            } else {
                                "process_list.pause_population"
                            })
                            .to_string(),
                            Message::PausePopulation(!self.population_paused),
                        ),
                        ui.button(t!("settings.refresh").to_string(), Message::Refresh),
                    ]),
            );
        let sort = widgets::row()
            .child(ui.select(Sort::ALL, self.sort, Message::Sort).width(200))
            .child(ui.button(
                if self.descending {
                    "\u{2193}"
                } else {
                    "\u{2191}"
                },
                Message::Sort(self.sort),
            ));
        body = body
            .child(controls)
            .child(sort)
            .child(Element::host_signal(error, widgets::error).width_match());
        let columns = self.native.borrow().columns.clone();
        let widths = self.layout_widths(0.0);
        let rows = signal(self.native.borrow().values.clone());
        self.native.borrow_mut().signal = Some(rows);
        let cell_rows = Rc::clone(&self.native);
        let cells_ui = ui.clone();
        let activation = Rc::clone(&self.native);
        let activation_ui = ui.clone();
        let actions = Rc::clone(&self.native);
        let actions_ui = ui.clone();
        let menus = Rc::clone(&self.native);
        let menus_ui = ui.clone();
        let advanced = settings.advanced.expose_all_priority_values;
        let table = Element::table_virtual(
            columns
                .iter()
                .map(|column| (column.label(), widths[*column as usize]))
                .collect(),
            rows,
            ROW_HEIGHT as i32,
        )
        .cell_render(move |row_index, column_index, value| {
            let native = cell_rows.borrow();
            let row = native.rows.get(row_index)?;
            if native.columns.get(column_index) == Some(&Sort::Status) {
                let role = match row.status {
                    "process_list.status_suspended" => Role::Warning,
                    "process_list.status_efficiency_mode" => Role::Success,
                    "process_list.status_active" => Role::TextMuted,
                    _ => Role::TextMuted,
                };
                return Some(widgets::label(value).fg_role(role).tooltip(value));
            }
            if native.columns.get(column_index) != Some(&Sort::Name) {
                return Some(widgets::label(value).tooltip(value));
            }
            let mut cell = widgets::row();
            if row.selection.processes.len() > 1 {
                cell = cell.child(
                    cells_ui
                        .button(
                            if row.expanded { "\u{25be}" } else { "\u{25b8}" },
                            Message::Expand(row.key.clone()),
                        )
                        .width(24),
                );
            } else if row.selection.nested {
                cell = cell.child(Element::col().width(24));
            }
            if let Some(icon) = &row.icon {
                cell = cell.child(
                    Element::image_rgba(icon.width, icon.height, &icon.rgba)
                        .width(20)
                        .height(20),
                );
            }
            let focus = cells_ui.clone();
            let index = row.index;
            let revision = row.revision;
            Some(
                cell.child(
                    Element::row()
                        .width_match()
                        .height(28)
                        .cross(windui::prelude::Align::Center)
                        .padding_xy(4, 0)
                        .corner(design::CONTROL_RADIUS)
                        .child(
                            widgets::label(row.selection.name.clone())
                                .width_match()
                                .max_lines(1),
                        )
                        .tooltip(row.selection.name.clone())
                        .clickable()
                        .on_click(move |ctx| {
                            widgets::flush_bindings(ctx);
                            focus.edit(Message::Focus(index, revision));
                        })
                        .weight(1.0),
                ),
            )
        })
        .on_row_activate(move |ctx, row_index| {
            widgets::flush_bindings(ctx);
            if let Some(row) = activation.borrow().rows.get(row_index) {
                activation_ui.emit(Message::Select(row.index, row.revision));
            }
        })
        .actions(t!("common.actions").to_string(), 100.0, move |row_index| {
            let native = actions.borrow();
            let Some(row) = native.rows.get(row_index) else {
                return Element::col();
            };
            actions_ui
                .button(
                    t!("common.actions").to_string(),
                    Message::Select(row.index, row.revision),
                )
                .small()
        })
        .on_row_context_menu(move |index| {
            menus
                .borrow()
                .rows
                .get(index)
                .map(|row| context_items(row, advanced, menus_ui.clone()))
                .unwrap_or_default()
        })
        .fill()
        .weight(1.0);
        body = body.child(super::scrolling::preserve_table_rows(table));
        body = body.child(
            Element::flow()
                .width_match()
                .spacing(8)
                .run_spacing(8)
                .children([
                    ui.button(
                        t!("process_list.open_rule_details").to_string(),
                        Message::OpenFocused,
                    )
                    .enabled_signal(available_actions.map(|available| available[0])),
                    ui.button(
                        t!("process_list.grouping").to_string(),
                        Message::ExpandFocused,
                    )
                    .enabled_signal(available_actions.map(|available| available[1])),
                    ui.button(
                        t!("process_list.stop_process").to_string(),
                        Message::DeleteFocused,
                    )
                    .danger()
                    .outline_soft()
                    .enabled_signal(available_actions.map(|available| available[2])),
                ]),
        );
        body
    }

    pub(super) fn shows_immediate_selection(&self) -> bool {
        self.selected.is_some() && self.process_tab == ProcessTab::Immediate
    }

    /// Native modal presentation belongs to the whole application viewport.
    pub(super) fn modal(
        &self,
        settings: &Settings,
        status: &RuntimeStatusSnapshot,
        plans: PlanCatalog<'_>,
        ui: Ui<Message>,
    ) -> Option<Element> {
        if self.stopping.is_none() && self.selected.is_none() {
            return None;
        }
        let mut stack = Element::stack().fill();
        if let Some((selection, tree)) = &self.stopping {
            let close = ui.clone();
            let show = signal(true);
            let panel = widgets::col()
                .max_width(460)
                .padding(20)
                .bg_role(Role::Surface)
                .child(widgets::heading(
                    t!("process_list.kill_confirm_title", name = &selection.name).to_string(),
                ))
                .child(widgets::label(
                    t!(
                        if *tree {
                            "process_list.stop_tree_confirm"
                        } else {
                            "process_list.stop_confirm"
                        },
                        name = &selection.name
                    )
                    .to_string(),
                ))
                .child(
                    widgets::row()
                        .child(
                            ui.button(t!("process_list.kill").to_string(), Message::ConfirmStop)
                                .danger(),
                        )
                        .child(ui.button(t!("common.cancel").to_string(), Message::CancelStop)),
                );
            let dialog = Element::dialog_closable(show, panel, move |ctx| {
                widgets::flush_bindings(ctx);
                close.emit(Message::CancelStop);
            });
            let dismiss = ui.clone();
            return Some(
                stack.child(
                    widgets::observe_action(dialog.weight(1.0), show, move |open| {
                        if !open {
                            dismiss.emit(Message::CancelStop);
                        }
                    })
                    .fill(),
                ),
            );
        }
        if let Some(selection) = &self.selected {
            let eligible = selection
                .processes
                .iter()
                .any(|process| !inaccessible(process));
            let suspended = selection.processes.iter().any(|process| {
                status
                    .feature_status
                    .app_suspension
                    .suspended_process_ids
                    .contains(&process.id)
            });
            let suspendable = selection.processes.iter().any(suspendable);
            let mut pane = widgets::col()
                .child(widgets::label(selection.path.clone()))
                .child(widgets::label(
                    selection
                        .processes
                        .iter()
                        .map(|process| process.id.to_string())
                        .collect::<Vec<_>>()
                        .join(", "),
                ))
                .child(
                    widgets::row()
                        .child(ui.button(
                            t!("process_list.immediate_actions").to_string(),
                            Message::ProcessTab(ProcessTab::Immediate),
                        ))
                        .child(ui.button(
                            t!("process_list.rulesets").to_string(),
                            Message::ProcessTab(ProcessTab::Rulesets),
                        )),
                );
            match self.process_tab {
                ProcessTab::Rulesets if eligible && !selection.path.is_empty() => {
                    pane = pane.child(details::view(
                        settings,
                        &selection.path,
                        plans,
                        ui.map(Message::Details),
                    ))
                }
                ProcessTab::Rulesets => {
                    pane = pane.child(widgets::help("process_list.status_access_denied"))
                }
                ProcessTab::Immediate => {
                    let efficiency = match self.efficiency {
                        Some(value) => ui
                            .toggle(value, |value| Message::Action(Action::Efficiency(value)))
                            .enabled(eligible),
                        None => widgets::help("common.unknown"),
                    };
                    pane = pane
                        .child(widgets::setting_row(
                            t!("process_list.efficiency_mode").to_string(),
                            efficiency,
                        ))
                        .child(widgets::card(
                            t!("nav.priority_control").to_string(),
                            [self.immediate_priorities(settings, eligible, ui.clone())],
                        ))
                        .child(
                            ui.button(
                                t!(if suspended {
                                    "process_list.resume_process"
                                } else {
                                    "process_list.suspend_process"
                                })
                                .to_string(),
                                Message::Action(Action::Suspend(!suspended)),
                            )
                            .enabled(suspendable),
                        );
                }
            }
            let (stop_process, stop_tree) = selection.stop_options(&self.processes);
            let mut footer = widgets::col()
                .child(
                    ui.button(
                        t!("process_list.open_process_location").to_string(),
                        Message::OpenLocation,
                    )
                    .enabled(!selection.path.is_empty()),
                )
                .child(
                    ui.button(
                        t!("process_list.properties").to_string(),
                        Message::Properties,
                    )
                    .enabled(!selection.path.is_empty()),
                );
            if stop_process {
                footer = footer.child(
                    ui.button(
                        t!("process_list.stop_process").to_string(),
                        Message::Stop(false),
                    )
                    .danger()
                    .enabled(eligible),
                );
            }
            if stop_tree {
                footer = footer.child(
                    ui.button(
                        t!("process_list.stop_process_tree").to_string(),
                        Message::Stop(true),
                    )
                    .danger()
                    .enabled(eligible),
                );
            }
            footer =
                footer.child(ui.button(t!("common.close").to_string(), Message::CloseSelection));
            let close = ui.clone();
            let show = signal(true);
            let panel = widgets::col()
                .fill()
                .max_width(960)
                .padding(20)
                .bg_role(Role::Surface)
                .child(widgets::heading(selection.name.clone()))
                .child(super::scrolling::scrollable(pane).fill().weight(1.0))
                .child(footer);
            let dialog = Element::dialog_closable(show, panel, move |ctx| {
                widgets::flush_bindings(ctx);
                close.emit(Message::CloseSelection);
            });
            let dismiss = ui.clone();
            stack = stack.child(
                widgets::observe_action(dialog.weight(1.0), show, move |open| {
                    if !open {
                        dismiss.emit(Message::CloseSelection);
                    }
                })
                .fill(),
            );
        }
        Some(stack)
    }
}

#[derive(Clone)]
struct NativeRow {
    index: usize,
    revision: u64,
    status: &'static str,
    key: String,
    expanded: bool,
    selection: Selection,
    icon: Option<Arc<crate::process_icon::ProcessIcon>>,
    suspended: bool,
    stop_options: (bool, bool),
}

#[derive(Default)]
struct NativeRows {
    refresh_pending: bool,
    rows: Vec<NativeRow>,
    columns: Vec<Sort>,
    values: Vec<Vec<String>>,
    signal: Option<Signal<Vec<Vec<String>>>>,
    count: Option<Signal<String>>,
    error: Option<Signal<Vec<String>>>,
    actions: Option<Signal<[bool; 3]>>,
}

fn suspendable(process: &ProcessInfo) -> bool {
    !inaccessible(process)
        && process.session_id.is_some_and(|session| session != 0)
        && process.is_service_account == Some(false)
        && !crate::app_suspension::is_builtin_excluded(&process.name)
}

fn context_items(row: &NativeRow, advanced: bool, ui: Ui<Message>) -> Vec<MenuItem> {
    let eligible = row
        .selection
        .processes
        .iter()
        .any(|process| !inaccessible(process));
    let action = |key: &str, message: Message, enabled: bool| {
        let ui = ui.clone();
        let index = row.index;
        let revision = row.revision;
        MenuItem::run(
            t!(key).to_string(),
            move |ctx| {
                widgets::flush_bindings(ctx);
                ui.emit(Message::RowAction(
                    index,
                    revision,
                    Box::new(message.clone()),
                ))
            },
            false,
        )
        .enabled(enabled)
    };
    let mut items = vec![MenuItem::submenu(
        t!("process_list.efficiency_mode").to_string(),
        vec![
            action(
                "common.enabled",
                Message::Menu(Box::new(Message::Action(Action::Efficiency(true)))),
                eligible,
            ),
            action(
                "common.disabled",
                Message::Menu(Box::new(Message::Action(Action::Efficiency(false)))),
                eligible,
            ),
        ],
    )];
    let priorities = [
        Kind::Process,
        Kind::Thread,
        Kind::Io,
        Kind::Gpu,
        Kind::Memory,
        Kind::DynamicBoost,
    ]
    .into_iter()
    .map(|kind| {
        let levels = kind
            .choices(advanced)
            .into_iter()
            .filter(|value| !is_default(*value))
            .map(|value| {
                let ui = ui.clone();
                let index = row.index;
                let revision = row.revision;
                let mut item = MenuItem::run(
                    value.to_string(),
                    move |ctx| {
                        widgets::flush_bindings(ctx);
                        ui.emit(Message::RowAction(
                            index,
                            revision,
                            Box::new(Message::Menu(Box::new(Message::Action(Action::Priority(
                                value,
                            ))))),
                        ))
                    },
                    false,
                )
                .enabled(eligible);
                if let Some(key) = value.warning() {
                    item = item.subtitle(t!(key).to_string());
                }
                item
            })
            .collect();
        let key = format!("nav.{}", kind.key());
        MenuItem::submenu(t!(&key).to_string(), levels)
    })
    .collect();
    items.push(MenuItem::submenu(
        t!("nav.priority_control").to_string(),
        priorities,
    ));
    items.push(action(
        if row.suspended {
            "process_list.resume_process"
        } else {
            "process_list.suspend_process"
        },
        Message::Menu(Box::new(Message::Action(Action::Suspend(!row.suspended)))),
        row.selection.processes.iter().any(suspendable),
    ));
    if row.stop_options.0 {
        items.push(action(
            "process_list.stop_process",
            Message::Stop(false),
            eligible,
        ));
    }
    if row.stop_options.1 {
        items.push(action(
            "process_list.stop_process_tree",
            Message::Stop(true),
            eligible,
        ));
    }
    items.push(MenuItem::separator());
    items.push(action(
        "process_list.open_rule_details",
        Message::OpenDetails,
        true,
    ));
    items.push(action(
        "process_list.open_process_location",
        Message::Menu(Box::new(Message::OpenLocation)),
        !row.selection.path.is_empty(),
    ));
    items.push(action(
        "process_list.properties",
        Message::Menu(Box::new(Message::Properties)),
        !row.selection.path.is_empty(),
    ));
    items
}

impl std::fmt::Display for Sort {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.label())
    }
}

fn minimum_column_width(column: Sort) -> f32 {
    match column {
        Sort::Name => 160.0,
        Sort::Status => 140.0,
        Sort::User | Sort::Memory => 100.0,
        _ => 80.0,
    }
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
    use windui::text::TextEngine;
    thread_local! {
        static TEXT: RefCell<windui::text::PlatformTextEngine> = RefCell::new(windui::text::PlatformTextEngine::new());
    }
    TEXT.with(|text| {
        text.borrow_mut()
            .measure(
                content,
                &windui::text::TextStyle::new(design::typography::BODY as f32),
                None,
            )
            .w as f32
    })
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
fn background<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
    map: impl Fn(Result<T, String>) -> Message + Send + Sync + 'static,
) -> Task<Message> {
    super::tasks::run(work).map(move |result| map(result.and_then(|value| value)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn focused_actions_follow_exact_visible_targets_without_rebuilding_the_page() {
        let mut list = ProcessList {
            processes: vec![process(10, 100, "app.exe"), process(11, 200, "app.exe")],
            hide_inaccessible: false,
            ..Default::default()
        };
        list.rebuild();
        let mut scope = windui::signal::SignalScope::new();
        scope.collect(|| {
            let actions = signal([true; 3]);
            list.native.borrow_mut().actions = Some(actions);
            list.refresh_view();
            assert_eq!(actions.get(), [false; 3]);

            list.focus_row(0, list.rows_revision);
            list.flush_pending_refresh();
            assert_eq!(actions.get(), [false; 3]);
            list.native.borrow_mut().refresh_pending = true;
            list.flush_pending_refresh();
            assert_eq!(actions.get(), [true, true, true]);
            assert!(!list.native.borrow().refresh_pending);

            list.processes[0].creation_time = Some(300);
            list.rebuild();
            list.refresh_view();
            assert_eq!(actions.get(), [false; 3]);

            for process in &mut list.processes {
                process.is_critical = Some(true);
            }
            list.rebuild();
            list.focus_row(0, list.rows_revision);
            list.refresh_view();
            assert_eq!(actions.get(), [true, true, false]);

            list.clear();
            assert_eq!(actions.get(), [false; 3]);
        });
    }

    #[test]
    fn presentation_release_drops_live_population_but_keeps_editor_preferences() {
        let mut list = ProcessList {
            processes: vec![process(10, 100, r"C:\A\app.exe")],
            search: "app".into(),
            grouped: false,
            sort: Sort::Memory,
            descending: true,
            column_widths: [Some(250.0); 6],
            selected: Some(Selection {
                nested: false,
                processes: vec![process(10, 100, r"C:\A\app.exe")],
                path: r"C:\A\app.exe".into(),
                name: "app.exe".into(),
            }),
            ..Default::default()
        };
        list.rebuild();
        list.refresh_view();
        let revision = list.rows_revision;
        assert!(!list.native.borrow().rows.is_empty());
        list.release_presentation();
        assert!(list.processes.is_empty());
        assert_eq!(list.processes.capacity(), 0);
        assert_eq!(list.rows.capacity(), 0);
        assert_eq!(list.icons.capacity(), 0);
        assert!(list.native.borrow().rows.is_empty());
        assert_ne!(list.rows_revision, revision);
        assert_eq!(list.search, "app");
        assert!(!list.grouped);
        assert_eq!(list.sort, Sort::Memory);
        assert!(list.descending);
        assert_eq!(list.column_widths, [Some(250.0); 6]);
        assert!(list.selected.is_some());
    }

    #[test]
    fn presentation_release_preserves_a_paused_population_snapshot() {
        let mut list = ProcessList {
            population_paused: true,
            refreshing: true,
            processes: vec![process(10, 100, r"C:\A\app.exe")],
            ..Default::default()
        };
        list.rebuild();
        list.refresh_view();
        let revision = list.rows_revision;
        list.release_presentation();
        list.discard_population_result();
        assert!(!list.refreshing);
        assert_eq!(list.processes.len(), 1);
        assert_eq!(list.rows.len(), 1);
        assert_eq!(list.rows_revision, revision);
        assert!(list.native.borrow().rows.is_empty());
        list.refresh_view();
        assert_eq!(list.native.borrow().rows.len(), 1);
        assert_eq!(list.native.borrow().rows[0].selection.processes[0].id, 10);
    }

    #[test]
    fn native_population_metadata_updates_without_replacing_focused_search() {
        use windui::{
            core::Tree,
            event::{Key, KeyEvent},
            prelude::Size,
            signal::SignalScope,
            text::NullTextEngine,
        };

        let mut list = ProcessList::default();
        let mut scope = SignalScope::new();
        scope.collect(|| {
            let mut tree = Tree::new();
            let root = list
                .view(
                    &Settings::default(),
                    &RuntimeStatusSnapshot::default(),
                    PlanCatalog::Loading,
                    Ui::new(|_: Message, _| {}),
                )
                .build(&mut tree);
            tree.root = Some(root);
            tree.layout_root(Size::new(600, 620), &mut NullTextEngine);
            let search = tree.focusable_order()[0];
            tree.set_focused(Some(search), None);
            tree.dispatch_key(
                KeyEvent {
                    key: Key::Char('a'),
                    pressed: true,
                    shift: false,
                    ctrl: false,
                    alt: false,
                    meta: false,
                },
                Some(search),
            );
            let error_host = tree.get(root).unwrap().children[2];
            assert!(tree.get(error_host).unwrap().children.is_empty());
            let count = list.native.borrow().count.unwrap();
            assert_eq!(count.get(), t!("process_list.count", count = 0));

            // Async population and action completions publish data, never a new page.
            list.processes = vec![
                process(10, 100, r"C:\A\app.exe"),
                process(11, 200, r"C:\B\app.exe"),
            ];
            list.error = Some(String::from("Access was denied"));
            list.rebuild();
            list.refresh_view();
            tree.layout_root(Size::new(600, 620), &mut NullTextEngine);
            assert_eq!(count.get(), t!("process_list.count", count = 2));
            assert_eq!(
                list.native.borrow().error.unwrap().get(),
                vec!["Access was denied"]
            );
            assert_eq!(tree.get(error_host).unwrap().children.len(), 1);
            assert_eq!(tree.focused_node(), Some(search));
            assert_eq!(tree.focusable_order()[0], search);
            assert_eq!(tree.ime_text_of(search).as_deref(), Some("a"));

            list.processes.clear();
            list.error = None;
            list.rebuild();
            list.refresh_view();
            tree.layout_root(Size::new(600, 620), &mut NullTextEngine);
            assert_eq!(count.get(), t!("process_list.count", count = 0));
            assert!(tree.get(error_host).unwrap().children.is_empty());
            assert_eq!(tree.focused_node(), Some(search));
        });
        drop(scope);
        // Destroyed windows also dispose their metadata signals safely.
        list.refresh_view();
    }

    #[test]
    fn native_table_updates_search_and_identity_without_recreating_the_input() {
        let mut list = ProcessList {
            processes: vec![
                process(10, 100, r"C:\A\app.exe"),
                process(11, 200, r"C:\B\app.exe"),
            ],
            ..Default::default()
        };
        list.rebuild();
        let mut scope = windui::signal::SignalScope::new();
        let source = scope.collect(|| {
            list.refresh_view();
            let source = signal(list.native.borrow().values.clone());
            list.native.borrow_mut().signal = Some(source);
            source
        });
        assert_eq!(source.get().len(), 2);
        let old_revision = list.rows_revision;
        list.search = String::from(r"C:\B");
        list.rebuild();
        list.refresh_view();
        assert_eq!(source.get().len(), 1);
        let native = list.native.borrow();
        assert_ne!(native.rows[0].revision, old_revision);
        assert_eq!(native.rows[0].selection.processes[0].id, 11);
        assert_eq!(
            native.rows[0].selection.processes[0].creation_time,
            Some(200)
        );
        drop(native);
        drop(scope);
        assert!(!source.is_alive());
        // A closed window owns no live signal; later population updates remain safe.
        list.refresh_view();
    }

    #[test]
    fn native_context_menu_keeps_protected_actions_disabled() {
        let mut protected = process(10, 100, r"C:\A\app.exe");
        protected.is_critical = Some(true);
        let mut list = ProcessList {
            hide_inaccessible: false,
            processes: vec![protected],
            ..Default::default()
        };
        list.rebuild();
        list.refresh_view();
        let native = list.native.borrow();
        let menu = context_items(&native.rows[0], false, Ui::new(|_: Message, _| {}));
        assert!(menu[0].submenu.iter().all(|item| !item.enabled));
        assert!(menu[1]
            .submenu
            .iter()
            .flat_map(|item| &item.submenu)
            .all(|item| !item.enabled));
        assert!(
            !menu
                .iter()
                .find(|item| item.label == t!("process_list.stop_process"))
                .unwrap()
                .enabled
        );
    }

    #[test]
    fn delete_keeps_selected_process_scope_and_cancel_clears_confirmation() {
        let mut settings = Settings::default();
        settings.general.enabled = false;
        let mut editor = crate::application::SettingsEditor::with_settings(settings.clone());
        let runtime = RuntimeHandle::start(&editor.runtime_settings_snapshot());
        let selection = Selection {
            nested: false,
            processes: vec![process(10, 100, "app.exe"), process(11, 200, "app.exe")],
            path: String::new(),
            name: "app.exe".into(),
        };
        let mut list = ProcessList {
            focused: Some(("app.exe".into(), false, selection.clone())),
            ..ProcessList::default()
        };
        let _ = list.update(Message::DeleteFocused, &mut settings, &runtime);
        let (pending, tree) = list.stopping.as_ref().unwrap();
        assert_eq!(pending.processes.len(), 2);
        assert!(!tree);
        let _ = list.update(Message::CancelStop, &mut settings, &runtime);
        assert!(list.stopping.is_none());
        for tree in [false, true] {
            list.selected = Some(selection.clone());
            let _ = list.update(Message::Stop(tree), &mut settings, &runtime);
            assert_eq!(list.stopping.as_ref().unwrap().1, tree);
            let _ = list.update(Message::CancelStop, &mut settings, &runtime);
            assert!(list.stopping.is_none());
        }
        runtime.shutdown().unwrap();
    }

    #[test]
    fn stop_menu_distinguishes_groups_children_and_nested_rows() {
        let parent = process(10, 100, "parent.exe");
        let mut child = process(11, 200, "child.exe");
        child.parent_id = Some(parent.id);
        let mut selection = Selection {
            nested: false,
            processes: vec![parent.clone()],
            path: String::new(),
            name: parent.name.clone(),
        };
        assert_eq!(selection.stop_options(&[]), (true, false));
        assert_eq!(
            selection.stop_options(&[parent, child.clone()]),
            (true, true)
        );
        selection.nested = true;
        assert_eq!(selection.stop_options(&[]), (true, true));
        selection.nested = false;
        selection.processes.push(child);
        assert_eq!(selection.stop_options(&[]), (false, true));
    }

    #[test]
    fn stop_action_uses_tree_for_groups_or_children_but_not_reused_parent_ids() {
        let parent = process(10, 100, "parent.exe");
        let mut child = process(11, 200, "child.exe");
        child.parent_id = Some(parent.id);
        let mut selection = Selection {
            nested: false,
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
    fn action_feedback_retains_the_original_selection() {
        let mut list = ProcessList {
            selected: Some(Selection {
                nested: false,
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
        let revision = list.rows_revision;
        assert_eq!(list.rows.len(), 2);
        assert_eq!(list.rows[0].indices.len(), 2);
        list.expanded.insert(list.rows[0].key.clone());
        list.rebuild();
        assert_eq!(list.rows.len(), 4);
        assert!(list.rows[1].nested);
        assert_ne!(list.rows_revision, revision);
        list.clear();
    }
    #[test]
    fn expanded_groups_collapse_immediately() {
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
        list.expanded.remove(&key);
        list.rebuild();
        assert_eq!(list.rows.len(), 1);
    }
    #[test]
    fn selected_targets_retain_creation_time_across_refresh() {
        let old = process(10, 100, r"C:\A\app.exe");
        let selected = Selection {
            nested: false,
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
