use super::design;
use super::priority_control::{Kind, Value};
use super::widgets::{action_button, button, checkbox, pick_list, text_input};
use crate::application::SettingsEditor;
use crate::automation::RuntimeHandle;
use crate::config::*;
use crate::foreground::{
    self, ProcessActionTarget, ProcessActionTargetError, ProcessInfo, ProcessResourceSample,
};
use crate::platform::windows::gpu_memory::{self, GpuMemorySample};
use crate::platform::windows::gpu_usage::GpuUsageQuery;
use crate::ui::scrolling::scrollable;
use iced::widget::{column, container, image, mouse_area, responsive, row, text, Space};
use iced::{
    keyboard::{key::Named, Modifiers},
    Element, Fill, Point, Rectangle, Task,
};
use rust_i18n::t;
use std::{
    borrow::Cow,
    collections::{BTreeMap, HashMap, HashSet},
    ffi::OsString,
    ops::Range,
    path::{Path, PathBuf},
    sync::Arc,
};
#[path = "process_details.rs"]
pub(super) mod details;
use super::scrolling;
#[derive(Clone, Copy)]
pub(super) enum PlanCatalog<'a> {
    Loading,
    Unavailable,
    Loaded(&'a [crate::power::PowerPlan]),
}
const HEADER_HEIGHT: f32 = 32.0;
const ROW_HEIGHT: f32 = 36.0;
const PROCESS_TABLE: &str = "process-list";
const REFRESH_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);
const ACTION_TOAST_DURATION: std::time::Duration = std::time::Duration::from_secs(5);
type Icons = HashMap<PathBuf, Option<Arc<image::Handle>>>;
type PopulationReply = iced::futures::channel::oneshot::Sender<Result<Population, String>>;
type GpuPopulationWorker = std::sync::mpsc::Sender<(bool, PopulationReply)>;
#[derive(Debug, Clone)]
pub(super) struct Population {
    processes: Vec<ProcessInfo>,
    samples: BTreeMap<u32, ProcessResourceSample>,
    gpu_memory: BTreeMap<u32, GpuMemorySample>,
    gpu_usage: HashMap<u32, f32>,
    total_memory_bytes: Option<u64>,
}
#[derive(Debug, Clone)]
struct Entry {
    indices: Vec<usize>,
    key: OsString,
    nested: bool,
}

struct FocusedRow {
    key: OsString,
    nested: bool,
    pid: u32,
    creation_time: Option<u64>,
}

#[derive(Debug, Clone)]
pub(super) struct Selection {
    nested: bool,
    processes: Vec<ProcessInfo>,
    name: String,
}
impl Selection {
    fn path(&self) -> Option<&Path> {
        self.processes
            .first()?
            .image_path
            .as_deref()
            .filter(|path| !path.as_os_str().is_empty())
    }

    fn path_text(&self) -> Option<&str> {
        self.path().and_then(Path::to_str)
    }

    fn ruleset_path(&self) -> Option<&str> {
        self.path_text()
            .filter(|_| self.processes.iter().any(|process| !inaccessible(process)))
    }

    fn can_stop(&self) -> bool {
        self.processes.iter().any(|p| !inaccessible(p))
            && self
                .processes
                .iter()
                .all(|p| process_target_identity(p).is_ok())
    }

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

    fn memory_trim_candidates(&self) -> impl Iterator<Item = &ProcessInfo> {
        self.processes.iter().filter(|process| {
            live_action_available(process)
                && !crate::memory_trim::is_builtin_excluded(&process.name)
        })
    }

    fn has_departed_process(&self, processes: &[ProcessInfo]) -> bool {
        self.processes.iter().any(|selected| {
            selected.creation_time.is_some_and(|expected| {
                processes
                    .iter()
                    .find(|current| current.id == selected.id)
                    .is_none_or(|current| {
                        current
                            .creation_time
                            .is_some_and(|actual| actual != expected)
                    })
            })
        })
    }

    fn shares_process_with(&self, other: &Self) -> bool {
        self.processes.iter().any(|process| {
            process.creation_time.is_some()
                && other.processes.iter().any(|other| {
                    other.id == process.id && other.creation_time == process.creation_time
                })
        })
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Sort {
    Name,
    Pid,
    Status,
    Cpu,
    CpuTime,
    GpuUsage,
    WorkingSet,
    PrivateCommit,
    DedicatedGpuMemory,
    SharedGpuMemory,
    User,
}
impl Sort {
    const ALL: [Self; 11] = [
        Self::Name,
        Self::Pid,
        Self::Status,
        Self::Cpu,
        Self::CpuTime,
        Self::GpuUsage,
        Self::WorkingSet,
        Self::PrivateCommit,
        Self::DedicatedGpuMemory,
        Self::SharedGpuMemory,
        Self::User,
    ];
    fn label(self) -> String {
        t!(match self {
            Self::Name => "process_list.process_name",
            Self::Pid => "process_list.pid",
            Self::Status => "process_list.status",
            Self::Cpu => "process_list.cpu_usage",
            Self::CpuTime => "process_list.cpu_time",
            Self::GpuUsage => "process_list.gpu_usage",
            Self::WorkingSet => "process_list.working_set",
            Self::PrivateCommit => "process_list.private_commit",
            Self::DedicatedGpuMemory => "process_list.dedicated_gpu_memory",
            Self::SharedGpuMemory => "process_list.shared_gpu_memory",
            Self::User => "process_list.user",
        })
        .to_string()
    }

    fn is_gpu_memory(self) -> bool {
        matches!(self, Self::DedicatedGpuMemory | Self::SharedGpuMemory)
    }

    fn is_gpu_metric(self) -> bool {
        self == Self::GpuUsage || self.is_gpu_memory()
    }
}
#[derive(Debug, Clone, Copy)]
pub(super) enum Action {
    Priority(Value),
    Efficiency(bool),
    Suspend(bool),
    TrimMemory,
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

#[derive(Clone, Copy, Debug, PartialEq)]
enum ContextAnchor {
    Pointer(Point),
    FocusedRow(Point),
}

impl ContextAnchor {
    fn window_position(self, origin: Point) -> Point {
        match self {
            Self::Pointer(point) => Point::new(origin.x + point.x, origin.y + point.y),
            Self::FocusedRow(point) => point,
        }
    }
}

#[derive(Debug, Clone)]
pub(super) enum Message {
    Refresh,
    Loaded(Result<Population, String>),
    IconsLoaded(Result<Icons, String>),
    Search(String),
    Scrolled(f32),
    ResizeStart(Sort, f32),
    ResizeMoved(f32),
    ResizeEnd,
    AutoSize(Sort),
    HideInaccessible(bool),
    Group(bool),
    MemoryPercentage(bool),
    Expand(OsString),
    Sort(Sort),
    Column(Sort, bool),
    Focus(usize, u64),
    MoveRowFocus(bool, f32),
    OpenFocused(f32),
    ExpandFocused(Option<f32>),
    DeleteFocused(f32),
    Context(usize, u64),
    PointerMoved(iced::Point),
    OpenDetails,
    OpenContextFocused(Rectangle),
    ProcessTab(ProcessTab),
    MenuBranch(MenuBranch),
    Menu(Box<Message>),
    Current(u64, Result<(Vec<Value>, Option<bool>), String>),
    CloseSelection,
    Action(Action),
    Stop(bool),
    ConfirmStop,
    CancelStop,
    TreeReady {
        name: String,
        targets: Result<Vec<ProcessActionTarget>, String>,
    },
    Result {
        name: String,
        result: Result<(), String>,
        readback: Option<Selection>,
        detail_action: Option<u64>,
    },
    DismissActionFeedback,
    CopyExecutablePath,
    OpenLocation,
    Properties,
    Details(details::Message),
}

struct ActionFeedback {
    result: Result<String, String>,
    expires_at: Option<std::time::Instant>,
}

pub(super) struct ProcessList {
    processes: Vec<ProcessInfo>,
    population_loaded: bool,
    rows: Vec<Entry>,
    offsets: Vec<f32>,
    rows_revision: u64,
    samples: BTreeMap<u32, ProcessResourceSample>,
    gpu_memory: BTreeMap<u32, GpuMemorySample>,
    gpu_usage: HashMap<u32, f32>,
    gpu_population_worker: Option<GpuPopulationWorker>,
    total_memory_bytes: Option<u64>,
    cpu: HashMap<u32, f32>,
    suspended_process_instances: Vec<(u32, u64)>,
    icons: Icons,
    search: String,
    offset: std::cell::Cell<f32>,
    refreshing: bool,
    refresh_started_at: Option<std::time::Instant>,
    error: Option<String>,
    action_feedback: Option<ActionFeedback>,
    hide_inaccessible: bool,
    grouped: bool,
    memory_percentage: bool,
    expanded: HashSet<OsString>,
    sort: Sort,
    descending: bool,
    columns: [bool; Sort::ALL.len()],
    column_widths: [Option<f32>; Sort::ALL.len()],
    resizing: Option<(Sort, f32, Option<f32>)>,
    selected: Option<Selection>,
    focused: Option<FocusedRow>,
    pointer: iced::Point,
    context: Option<ContextAnchor>,
    menu_branch: MenuBranch,
    process_tab: ProcessTab,
    detail_action_id: u64,
    detail_feedback: Option<Result<String, String>>,
    rulesets_error: Option<String>,
    ruleset_limiter: super::cpu_limiter::CpuLimiter,
    current_query_id: u64,
    current: Vec<Value>,
    efficiency: Option<bool>,
    stopping: Option<(Selection, bool)>,
}
impl Default for ProcessList {
    fn default() -> Self {
        Self {
            processes: Vec::new(),
            population_loaded: false,
            rows: Vec::new(),
            offsets: vec![0.0],
            rows_revision: 0,
            samples: BTreeMap::new(),
            gpu_memory: BTreeMap::new(),
            gpu_usage: HashMap::new(),
            gpu_population_worker: None,
            total_memory_bytes: None,
            cpu: HashMap::new(),
            suspended_process_instances: Vec::new(),
            icons: HashMap::new(),
            search: String::new(),
            offset: std::cell::Cell::new(0.0),
            refreshing: false,
            refresh_started_at: None,
            error: None,
            action_feedback: None,
            hide_inaccessible: true,
            grouped: true,
            memory_percentage: false,
            expanded: HashSet::new(),
            sort: Sort::Name,
            descending: false,
            columns: Sort::ALL.map(|column| {
                !column.is_gpu_metric() && !matches!(column, Sort::CpuTime | Sort::PrivateCommit)
            }),
            column_widths: [None; Sort::ALL.len()],
            resizing: None,
            selected: None,
            focused: None,
            pointer: iced::Point::ORIGIN,
            context: None,
            menu_branch: MenuBranch::Closed,
            process_tab: ProcessTab::Immediate,
            detail_action_id: 0,
            detail_feedback: None,
            rulesets_error: None,
            ruleset_limiter: Default::default(),
            current_query_id: 0,
            current: Vec::new(),
            efficiency: None,
            stopping: None,
        }
    }
}
impl ProcessList {
    pub(super) fn rulesets_open(&self) -> bool {
        self.process_tab == ProcessTab::Rulesets
            && self
                .selected
                .as_ref()
                .and_then(Selection::ruleset_path)
                .is_some()
    }

    pub(super) fn has_invalid_limiter_inputs(&self) -> bool {
        self.ruleset_limiter.has_invalid_inputs()
    }

    pub(super) fn reset_limiter_inputs(&mut self) {
        self.ruleset_limiter = Default::default();
    }

    fn refresh_if_due(&mut self, now: std::time::Instant) -> Task<Message> {
        self.action_feedback.take_if(|feedback| {
            feedback
                .expires_at
                .is_some_and(|expires_at| now >= expires_at)
        });
        if self.refreshing
            || self
                .refresh_started_at
                .is_some_and(|started| now.saturating_duration_since(started) < REFRESH_INTERVAL)
        {
            return Task::none();
        }
        self.refreshing = true;
        self.refresh_started_at = Some(now);
        let sample_gpu_memory = self.gpu_memory_visible();
        if self.columns[Sort::GpuUsage as usize] {
            if self.gpu_population_worker.is_none() {
                self.gpu_population_worker = start_gpu_population_worker().ok();
            }
            if let Some(worker) = self.gpu_population_worker.as_ref() {
                let (reply, result) = iced::futures::channel::oneshot::channel();
                if worker.send((sample_gpu_memory, reply)).is_ok() {
                    return Task::perform(
                        async move { result.await.map_err(|error| error.to_string())? },
                        Message::Loaded,
                    );
                }
                self.stop_gpu_usage_sampling();
            }
        }
        background(
            move || load_population(sample_gpu_memory, None),
            Message::Loaded,
        )
    }

    pub(super) fn stop_gpu_usage_sampling(&mut self) {
        self.gpu_population_worker = None;
        if !self.gpu_usage.is_empty() {
            self.gpu_usage.clear();
            if self.sort == Sort::GpuUsage {
                self.rebuild();
            }
        }
    }

    fn gpu_memory_visible(&self) -> bool {
        self.columns[Sort::DedicatedGpuMemory as usize]
            || self.columns[Sort::SharedGpuMemory as usize]
    }

    pub(super) fn context_open(&self) -> bool {
        self.context.is_some() && self.selected.is_some()
    }

    #[cfg(feature = "render-smoke")]
    pub(super) fn populate_smoke_rows(&mut self) {
        let now = std::time::Instant::now();
        self.hide_inaccessible = false;
        self.population_loaded = true;
        self.refresh_started_at = Some(now);
        self.processes.clear();
        self.samples.clear();
        self.cpu.clear();
        self.icons.clear();
        for (index, (name, cpu, memory)) in [
            ("browser.exe", 9.3, 1160),
            ("code-editor.exe", 5.4, 719),
            ("build-worker.exe", 4.6, 446),
            ("file-manager.exe", 2.4, 132),
            ("project-sync.exe", 1.6, 98),
            ("terminal.exe", 0.8, 85),
            ("audio-host.exe", 0.7, 56),
            ("desktop-shell.exe", 0.3, 38),
            ("media-viewer.exe", 0.2, 154),
            ("note-editor.exe", 0.1, 67),
            ("update-agent.exe", 0.1, 21),
            ("window-host.exe", 0.0, 45),
        ]
        .into_iter()
        .enumerate()
        {
            let id = 1204 + index as u32 * 44;
            self.processes.push(ProcessInfo {
                id,
                creation_time: None,
                parent_id: None,
                session_id: Some(1),
                user_name: Some("Demo User".into()),
                is_service_account: Some(false),
                is_critical: Some(false),
                can_set_information: true,
                name: name.into(),
                // A relative path and absent creation time cannot authorize a live action.
                image_path: Some(PathBuf::from(name)),
            });
            self.samples.insert(
                id,
                ProcessResourceSample {
                    cpu: crate::cpu::ProcessCpuSample {
                        cpu_time_100ns: 0,
                        sampled_at: now,
                    },
                    creation_time: 0,
                    working_set_bytes: Some(memory * 1024 * 1024),
                    private_commit_bytes: None,
                    efficiency_mode: Some(index >= 9),
                },
            );
            self.cpu.insert(id, cpu);
        }
        self.sort = Sort::Cpu;
        self.descending = true;
        self.rebuild();
    }

    #[cfg(feature = "render-smoke")]
    pub(super) fn show_copy_path_fixture(&mut self, context: bool) {
        self.selected = Some(Selection {
            nested: false,
            processes: vec![ProcessInfo {
                id: u32::MAX,
                creation_time: None,
                parent_id: None,
                session_id: None,
                user_name: None,
                is_service_account: None,
                is_critical: None,
                can_set_information: false,
                name: "example.exe".into(),
                image_path: Some(r"C:\Apps\測試\example.exe".into()),
            }],
            name: "example.exe".into(),
        });
        self.context = context.then_some(ContextAnchor::Pointer(Point::new(130.0, 80.0)));
        self.process_tab = ProcessTab::Immediate;
    }

    pub(super) fn modal_open(&self) -> bool {
        self.stopping.is_some() || (self.selected.is_some() && self.context.is_none())
    }

    pub(super) fn update(
        &mut self,
        message: Message,
        settings: &mut SettingsEditor,
        runtime: &RuntimeHandle,
    ) -> Task<Message> {
        match message {
            Message::Refresh => return self.refresh_if_due(std::time::Instant::now()),
            Message::Loaded(result) => {
                self.refreshing = false;
                match result {
                    Ok(mut data) => {
                        let rebuild_rows = !self.population_loaded
                            || !matches!(self.sort, Sort::Name | Sort::Pid)
                            || self.processes != data.processes;
                        self.population_loaded = true;
                        let mut cpu_usage = crate::cpu::process_cpu_usage_batch();
                        self.cpu = data
                            .samples
                            .iter()
                            .filter_map(|(id, s)| {
                                self.samples
                                    .get(id)
                                    .filter(|old| old.creation_time == s.creation_time)
                                    .and_then(|old| cpu_usage(old.cpu, s.cpu))
                                    .map(|cpu| (*id, cpu))
                            })
                            .collect();
                        if self.gpu_population_worker.is_some() {
                            // A rate spans two collections. Reject new or reused process IDs.
                            data.gpu_usage.retain(|pid, _| {
                                self.samples
                                    .get(pid)
                                    .zip(data.samples.get(pid))
                                    .is_some_and(|(old, new)| {
                                        old.creation_time == new.creation_time
                                    })
                            });
                            self.gpu_usage = data.gpu_usage;
                        }
                        if self.selected.as_ref().is_some_and(|selection| {
                            selection.has_departed_process(&data.processes)
                        }) {
                            self.current_query_id = self.current_query_id.wrapping_add(1);
                            self.current.clear();
                            self.efficiency = None;
                        }
                        self.processes = data.processes;
                        self.samples = data.samples;
                        if self.gpu_memory_visible() {
                            self.gpu_memory = data.gpu_memory;
                        }
                        self.total_memory_bytes = data.total_memory_bytes;
                        let paths = self
                            .processes
                            .iter()
                            .filter_map(|p| p.image_path.as_ref())
                            .collect::<HashSet<_>>();
                        self.icons.retain(|p, _| paths.contains(p));
                        self.error = None;
                        let missing = paths
                            .into_iter()
                            .filter(|path| !self.icons.contains_key(*path))
                            .cloned()
                            .collect::<Vec<_>>();
                        if rebuild_rows {
                            self.rebuild();
                        }
                        if !missing.is_empty() {
                            // Publish rows before extracting executable icons, and keep one refresh in flight.
                            self.refreshing = true;
                            return background(
                                move || {
                                    Ok(missing
                                        .into_iter()
                                        .map(|path| {
                                            let icon =
                                                crate::process_icon::load_process_icon(&path);
                                            (path, icon)
                                        })
                                        .collect())
                                },
                                Message::IconsLoaded,
                            );
                        }
                    }
                    Err(e) => {
                        self.gpu_memory.clear();
                        self.gpu_usage.clear();
                        if self.sort.is_gpu_metric() {
                            self.rebuild();
                        }
                        self.error = Some(e);
                    }
                }
            }
            Message::IconsLoaded(result) => {
                self.refreshing = false;
                match result {
                    Ok(icons) => self.icons.extend(icons),
                    Err(error) => self.error = Some(error),
                }
            }
            Message::Search(v) => {
                self.search = v;
                self.offset.set(0.0);
                self.rebuild();
                return iced::widget::operation::scroll_to(
                    PROCESS_TABLE,
                    iced::widget::scrollable::AbsoluteOffset::<f32>::default(),
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
                    self.descending = sort.is_gpu_metric()
                        || matches!(
                            sort,
                            Sort::Cpu | Sort::CpuTime | Sort::WorkingSet | Sort::PrivateCommit
                        );
                }
                self.rebuild();
            }
            Message::Column(column, visible) => {
                self.columns[column as usize] = visible;
                if column.is_gpu_metric() && !visible {
                    if column == Sort::GpuUsage {
                        self.stop_gpu_usage_sampling();
                    }
                    if !self.gpu_memory_visible() {
                        self.gpu_memory.clear();
                    }
                    if self.sort == column {
                        self.sort = Sort::Name;
                        self.descending = false;
                        self.rebuild();
                    }
                }
                self.fit_visible_columns();
            }
            Message::Menu(message) => {
                let task = self.update(*message, settings, runtime);
                self.selected = None;
                self.rulesets_error = None;
                self.reset_limiter_inputs();
                self.context = None;
                return task;
            }
            Message::PointerMoved(position) => self.pointer = position,
            Message::MenuBranch(menu) => self.menu_branch = menu,
            Message::ProcessTab(tab) => {
                if tab != ProcessTab::Rulesets {
                    self.reset_limiter_inputs();
                }
                self.process_tab = tab;
            }
            Message::OpenDetails => {
                self.context = None;
                self.process_tab = ProcessTab::Rulesets;
            }
            Message::Context(index, revision) => {
                if revision == self.rows_revision && index < self.rows.len() {
                    return self.open_context(
                        index,
                        ContextAnchor::Pointer(self.pointer),
                        settings,
                    );
                }
            }
            Message::OpenContextFocused(bounds) => {
                return self.open_context_focused(bounds, settings)
            }
            Message::Focus(index, revision) => {
                if self.focus_row(index, revision) {
                    return iced::widget::operation::focus(PROCESS_TABLE);
                }
            }
            Message::MoveRowFocus(forward, height) => return self.move_row_focus(forward, height),
            Message::OpenFocused(height) => {
                if self.selected.is_none() && self.stopping.is_none() {
                    if let Some(index) = self.focused_or_first_visible_row() {
                        if let Some(task) = self.reveal_row(index, height) {
                            return task;
                        }
                        self.focus_row(index, self.rows_revision);
                        return self.select(index, settings);
                    }
                }
            }
            Message::ExpandFocused(height) => {
                if let Some(index) = self
                    .focused_index()
                    .filter(|&index| self.rows[index].indices.len() > 1)
                {
                    if let Some(task) = height.and_then(|height| self.reveal_row(index, height)) {
                        return task;
                    }
                    let key = self.rows[index].key.clone();
                    return self.update(Message::Expand(key), settings, runtime);
                }
            }
            Message::DeleteFocused(height) => {
                if self.selected.is_some() || self.stopping.is_some() {
                    return Task::none();
                }
                let Some(index) = self.focused_index() else {
                    return Task::none();
                };
                let selection = self.selection(&self.rows[index]);
                if selection.can_stop() {
                    if let Some(task) = self.reveal_row(index, height) {
                        return task;
                    }
                    self.selected = Some(selection);
                    return self.update(Message::Stop(false), settings, runtime);
                }
            }
            Message::Current(query_id, result) => {
                if query_id == self.current_query_id && self.selected.is_some() {
                    match result {
                        Ok((values, efficiency)) => {
                            self.current = values;
                            self.efficiency = efficiency;
                        }
                        Err(error) => self.error = Some(error),
                    }
                }
            }
            Message::CloseSelection => {
                let return_to_table = matches!(self.context, Some(ContextAnchor::FocusedRow(_)));
                self.selected = None;
                self.context = None;
                self.stopping = None;
                self.reset_detail_feedback();
                self.rulesets_error = None;
                self.reset_limiter_inputs();
                if return_to_table {
                    return iced::widget::operation::focus(PROCESS_TABLE);
                }
            }
            Message::Action(action) => {
                let detail_action = self.begin_detail_action();
                if let Some(selection) = &self.selected {
                    let targets = || selection.targets();
                    let result = match action {
                        Action::Efficiency(v) => {
                            runtime.request_efficiency_mode_action(targets(), v)
                        }
                        Action::Suspend(v) => {
                            runtime.request_app_suspension_process_action(targets(), v)
                        }
                        Action::TrimMemory => runtime.request_process_memory_trim(
                            selection
                                .memory_trim_candidates()
                                .map(snapshot_target)
                                .collect(),
                        ),
                        Action::Priority(value) => {
                            let kind = value_kind(value);
                            if !kind
                                .choices(settings.advanced.expose_all_priority_values)
                                .contains(&value)
                            {
                                self.record_action_result(
                                    Err(t!("process_list.status_access_denied").to_string()),
                                    detail_action,
                                );
                                return Task::none();
                            }
                            let targets = targets();
                            match value {
                                Value::Process(v) => {
                                    runtime.request_process_priority_action(targets, v)
                                }
                                Value::Thread(v) => {
                                    runtime.request_thread_priority_action(targets, v)
                                }
                                Value::Io(v) => {
                                    let Some(priority) = v.priority() else {
                                        return Task::none();
                                    };
                                    runtime.request_io_priority_action(targets, priority)
                                }
                                Value::Gpu(v) => {
                                    let Some(priority) = v.priority() else {
                                        return Task::none();
                                    };
                                    runtime.request_gpu_priority_action(targets, priority)
                                }
                                Value::Memory(v) => {
                                    let Some(priority) = v.priority() else {
                                        return Task::none();
                                    };
                                    runtime.request_memory_priority_action(targets, priority)
                                }
                                Value::DynamicBoost(v) => {
                                    use crate::control::dynamic_priority_boost::DynamicPriorityBoostState;

                                    let state = match v {
                                        ProcessDynamicPriorityBoostSetting::Enabled => {
                                            DynamicPriorityBoostState::Enabled
                                        }
                                        ProcessDynamicPriorityBoostSetting::Disabled => {
                                            DynamicPriorityBoostState::Disabled
                                        }
                                        ProcessDynamicPriorityBoostSetting::Default => {
                                            return Task::none()
                                        }
                                    };
                                    runtime.request_dynamic_priority_boost_action(targets, state)
                                }
                            }
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
                                self.result_message(
                                    matches!(action, Action::Efficiency(_) | Action::Priority(_)),
                                    detail_action,
                                ),
                            )
                        }
                        Err(e) => self.record_action_result(Err(e.to_string()), detail_action),
                    }
                }
            }
            Message::Stop(tree) => {
                if self.selected.as_ref().is_some_and(Selection::can_stop) {
                    self.context = None;
                    self.stopping = self.selected.take().map(|selection| (selection, tree));
                    self.reset_limiter_inputs();
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
                    let name = selection.name;
                    if !tree {
                        return self.stop_targets(name, roots, runtime);
                    }
                    let roots = match roots.and_then(|roots| {
                        runtime
                            .request_stop_tree_intent(&roots)
                            .map_err(|error| error.to_string())?;
                        Ok(roots)
                    }) {
                        Ok(roots) => roots,
                        Err(error) => return self.stop_targets(name, Err(error), runtime),
                    };
                    return background(
                        move || {
                            foreground::process_tree_action_targets(
                                &roots,
                                &foreground::list_processes_with_paths()?,
                            )
                        },
                        move |targets| Message::TreeReady {
                            name: name.clone(),
                            targets,
                        },
                    );
                }
            }
            Message::TreeReady { name, targets } => {
                return self.stop_targets(name, targets, runtime);
            }
            Message::Result {
                name,
                result,
                readback,
                detail_action,
            } => {
                self.record_action_result(
                    result.map(|()| t!("process_list.action_completed", name = name).to_string()),
                    detail_action,
                );
                if let Some(selection) = self.selected.as_ref().filter(|selection| {
                    readback
                        .as_ref()
                        .is_some_and(|origin| selection.shares_process_with(origin))
                }) {
                    return self.read_current(
                        selection.clone(),
                        settings.general.allow_cross_session_process_control,
                    );
                }
            }
            Message::DismissActionFeedback => self.action_feedback = None,
            Message::CopyExecutablePath => {
                if let Some(path) = self.selected.as_ref().and_then(Selection::path_text) {
                    return iced::clipboard::write(path.to_owned());
                }
            }
            Message::Properties => {
                if let Some(path) = self.selected.as_ref().and_then(Selection::path) {
                    let path = path.to_path_buf();
                    let detail_action = self.begin_detail_action();
                    return background(
                        move || foreground::open_process_properties(&path),
                        self.result_message(false, detail_action),
                    );
                }
            }
            Message::OpenLocation => {
                if let Some(path) = self.selected.as_ref().and_then(Selection::path) {
                    let path = path.to_path_buf();
                    let detail_action = self.begin_detail_action();
                    return background(
                        move || foreground::open_process_location(&path),
                        self.result_message(false, detail_action),
                    );
                }
            }
            Message::Details(message) => {
                if let Some(path) = self.selected.as_ref().and_then(Selection::ruleset_path) {
                    self.rulesets_error =
                        details::update(settings, path, message, &mut self.ruleset_limiter);
                }
            }
        }
        Task::none()
    }
    fn read_current(&mut self, selection: Selection, allow: bool) -> Task<Message> {
        use crate::control::dynamic_priority_boost::{
            current_dynamic_priority_boost_state, DynamicPriorityBoostState,
        };
        use crate::control::priority_efficiency::{
            current_efficiency_mode, current_process_priority,
        };

        use crate::control::{
            gpu_priority::current_process_gpu_priority,
            io_priority::current_process_io_priority,
            memory_priority::current_process_memory_priority,
            thread_priority::{current_process_thread_priority, ThreadInventory},
        };

        self.current_query_id = self.current_query_id.wrapping_add(1);
        let query_id = self.current_query_id;
        background(
            move || {
                let targets = selection.targets();
                let mut threads = ThreadInventory::default();
                let values = [
                    shared_value(&targets, |target| {
                        current_process_priority(target, allow)
                            .ok()
                            .map(Value::Process)
                    }),
                    shared_value(&targets, |target| {
                        current_process_thread_priority(target, allow, &mut threads)
                            .ok()
                            .flatten()
                            .map(Value::Thread)
                    }),
                    shared_value(&targets, |target| {
                        current_process_io_priority(target, allow)
                            .ok()
                            .map(|value| Value::Io(value.into()))
                    }),
                    shared_value(&targets, |target| {
                        current_process_gpu_priority(target, allow)
                            .ok()
                            .map(|value| Value::Gpu(value.into()))
                    }),
                    shared_value(&targets, |target| {
                        current_process_memory_priority(target, allow)
                            .ok()
                            .map(|value| Value::Memory(value.into()))
                    }),
                    shared_value(&targets, |target| {
                        current_dynamic_priority_boost_state(target, allow)
                            .ok()
                            .map(|value| {
                                Value::DynamicBoost(match value {
                                    DynamicPriorityBoostState::Enabled => {
                                        ProcessDynamicPriorityBoostSetting::Enabled
                                    }
                                    DynamicPriorityBoostState::Disabled => {
                                        ProcessDynamicPriorityBoostSetting::Disabled
                                    }
                                })
                            })
                    }),
                ]
                .into_iter()
                .flatten()
                .collect();
                let efficiency = shared_value(&targets, |target| {
                    current_efficiency_mode(target, allow).ok()
                });
                Ok((values, efficiency))
            },
            move |result| Message::Current(query_id, result),
        )
    }
    fn reset_detail_feedback(&mut self) {
        self.detail_action_id = self.detail_action_id.wrapping_add(1);
        self.detail_feedback = None;
    }
    fn begin_detail_action(&mut self) -> Option<u64> {
        if self.selected.is_some()
            && self.context.is_none()
            && self.process_tab == ProcessTab::Immediate
        {
            self.reset_detail_feedback();
            Some(self.detail_action_id)
        } else {
            None
        }
    }
    fn record_action_result(&mut self, result: Result<String, String>, detail_action: Option<u64>) {
        if self.selected.is_some()
            && self.context.is_none()
            && detail_action == Some(self.detail_action_id)
        {
            self.detail_feedback = Some(result);
        } else {
            self.action_feedback = Some(ActionFeedback {
                expires_at: result
                    .is_ok()
                    .then(|| std::time::Instant::now() + ACTION_TOAST_DURATION),
                result,
            });
        }
    }
    fn result_message(
        &self,
        readback: bool,
        detail_action: Option<u64>,
    ) -> impl Fn(Result<(), String>) -> Message + Send + 'static {
        let name = self
            .selected
            .as_ref()
            .map(|s| s.name.clone())
            .unwrap_or_default();
        let readback = self.selected.as_ref().filter(|_| readback).cloned();
        move |result| Message::Result {
            name: name.clone(),
            result,
            readback: readback.clone(),
            detail_action,
        }
    }
    fn stop_targets(
        &mut self,
        name: String,
        targets: Result<Vec<ProcessActionTarget>, String>,
        runtime: &RuntimeHandle,
    ) -> Task<Message> {
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
                        .into_termination_result()
                        .map(|_| ())
                },
                move |result| Message::Result {
                    name: name.clone(),
                    result,
                    readback: None,
                    detail_action: None,
                },
            ),
            Err(e) => {
                self.record_action_result(Err(e), None);
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
                Sort::CpuTime
                | Sort::User
                | Sort::WorkingSet
                | Sort::PrivateCommit
                | Sort::DedicatedGpuMemory
                | Sort::SharedGpuMemory => 100.0,
                _ => 80.0,
            };
            self.column_widths[*col as usize] = Some((*width + x - start).max(minimum));
        }
    }

    fn cell_value(&self, entry: &Entry, col: Sort) -> Cow<'_, str> {
        let p = &self.processes[entry.indices[0]];
        match col {
            Sort::Name => Cow::Borrowed(&p.name),
            Sort::Pid => {
                let value = if entry.indices.len() > 1 {
                    format!("{} x", entry.indices.len())
                } else {
                    p.id.to_string()
                };
                Cow::Owned(value)
            }
            Sort::Cpu => self
                .cpu_total(entry)
                .map_or_else(
                    || t!("common.unknown").into_owned(),
                    |usage| format!("{usage:.1}%"),
                )
                .into(),
            Sort::CpuTime => self
                .cpu_time_total(entry)
                .map_or_else(|| t!("common.unknown").into_owned(), format_cpu_time)
                .into(),
            Sort::GpuUsage => self
                .gpu_usage_percent(entry)
                .map_or(Cow::Borrowed("—"), |usage| format!("{usage:.1}%").into()),
            Sort::WorkingSet => self
                .resource_total(entry, |sample| sample.working_set_bytes)
                .map_or_else(
                    || t!("common.unknown").into_owned(),
                    |bytes| {
                        format_memory_usage(bytes, self.total_memory_bytes, self.memory_percentage)
                    },
                )
                .into(),
            Sort::PrivateCommit => self
                .resource_total(entry, |sample| sample.private_commit_bytes)
                .map_or_else(|| t!("common.unknown").into_owned(), format_megabytes)
                .into(),
            Sort::DedicatedGpuMemory | Sort::SharedGpuMemory => {
                match self.gpu_memory_bytes(entry, col) {
                    None => Cow::Borrowed("—"),
                    Some(0) => Cow::Borrowed("0 MB"),
                    Some(1..=49_999) => Cow::Borrowed("<0.1 MB"),
                    Some(bytes) => format_megabytes(bytes).into(),
                }
            }
            Sort::User => self.user(entry).into(),
            Sort::Status => t!(self.status_key(entry)),
        }
    }

    fn auto_width(&self, col: Sort) -> f32 {
        let mut width = measure_column_text(&col.label()) + 44.0;
        if col == Sort::Status {
            width = width.max(column_width(col));
        }
        let mut measured = HashMap::new();
        for entry in &self.rows {
            let value = self.cell_value(entry, col);
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
            let value_width = measured
                .entry(value)
                .or_insert_with_key(|value| measure_column_text(value));
            width = width.max(*value_width + padding);
        }
        width.ceil()
    }

    fn layout_widths(&self, available: f32) -> [f32; Sort::ALL.len()] {
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

    fn is_focused(&self, entry: &Entry) -> bool {
        self.focused.as_ref().is_some_and(|focused| {
            let process = &self.processes[entry.indices[0]];
            focused.key == entry.key
                && focused.nested == entry.nested
                && focused.pid == process.id
                && focused.creation_time == process.creation_time
        })
    }

    fn focused_index(&self) -> Option<usize> {
        self.rows.iter().position(|entry| self.is_focused(entry))
    }

    fn focused_or_first_visible_row(&self) -> Option<usize> {
        self.focused_index().or_else(|| {
            (self.focused.is_none() && !self.rows.is_empty()).then(|| self.first_visible_row())
        })
    }

    fn open_context_focused(
        &mut self,
        bounds: Rectangle,
        settings: &SettingsEditor,
    ) -> Task<Message> {
        if self.selected.is_some() || self.stopping.is_some() {
            return Task::none();
        }
        let Some(index) = self.focused_or_first_visible_row() else {
            return Task::none();
        };
        if let Some(task) = self.reveal_row(index, bounds.height) {
            return task;
        }
        let row_bottom = self.offsets[index + 1] - self.offset.get();
        let position = Point::new(
            bounds.x + design::space::MEDIUM as f32,
            bounds.y + row_bottom.clamp(0.0, bounds.height),
        );
        self.focus_row(index, self.rows_revision);
        Task::batch([
            self.open_context(index, ContextAnchor::FocusedRow(position), settings),
            super::keyboard_focus::move_focus(true, true),
        ])
    }

    fn open_context(
        &mut self,
        index: usize,
        anchor: ContextAnchor,
        settings: &SettingsEditor,
    ) -> Task<Message> {
        let task = self.select(index, settings);
        self.context = Some(anchor);
        self.menu_branch = MenuBranch::Closed;
        task
    }

    fn select(&mut self, index: usize, settings: &SettingsEditor) -> Task<Message> {
        let selection = self.selection(&self.rows[index]);
        self.context = None;
        self.stopping = None;
        self.reset_detail_feedback();
        self.rulesets_error = None;
        self.reset_limiter_inputs();
        self.current.clear();
        self.efficiency = None;
        self.selected = Some(selection.clone());
        self.process_tab = ProcessTab::Immediate;
        self.read_current(
            selection,
            settings.general.allow_cross_session_process_control,
        )
    }

    fn first_visible_row(&self) -> usize {
        self.offsets
            .partition_point(|offset| *offset <= self.offset.get())
            .saturating_sub(1)
            .min(self.rows.len().saturating_sub(1))
    }

    fn move_row_focus(&mut self, forward: bool, height: f32) -> Task<Message> {
        if self.rows.is_empty() || self.selected.is_some() || self.stopping.is_some() {
            return Task::none();
        }
        let index = match self.focused_index() {
            Some(index) if forward => (index + 1).min(self.rows.len() - 1),
            Some(index) => index.saturating_sub(1),
            None => self.first_visible_row(),
        };
        self.focus_row(index, self.rows_revision);
        self.reveal_row(index, height).unwrap_or_else(Task::none)
    }

    fn reveal_row(&self, index: usize, height: f32) -> Option<Task<Message>> {
        let start = self.offsets[index];
        let end = self.offsets[index + 1];
        let max = (self.offsets[self.rows.len()] - height).max(0.0);
        let offset = self
            .offset
            .get()
            .clamp((end - height).min(start), start)
            .clamp(0.0, max);
        (self.offset.replace(offset) != offset).then(|| {
            iced::widget::operation::scroll_to(
                PROCESS_TABLE,
                iced::widget::scrollable::AbsoluteOffset {
                    x: None,
                    y: Some(offset),
                },
            )
        })
    }

    fn focus_row(&mut self, index: usize, revision: u64) -> bool {
        if let Some(entry) = self
            .rows
            .get(index)
            .filter(|_| revision == self.rows_revision)
        {
            let process = &self.processes[entry.indices[0]];
            self.focused = Some(FocusedRow {
                key: entry.key.clone(),
                nested: entry.nested,
                pid: process.id,
                creation_time: process.creation_time,
            });
            return true;
        }
        false
    }

    fn selection(&self, entry: &Entry) -> Selection {
        let process = &self.processes[entry.indices[0]];
        Selection {
            nested: entry.nested,
            processes: entry
                .indices
                .iter()
                .map(|i| self.processes[*i].clone())
                .collect(),
            name: process.name.clone(),
        }
    }

    pub(super) fn sync_suspended_processes(&mut self, instances: &[(u32, u64)]) {
        if self.suspended_process_instances != instances {
            self.suspended_process_instances.clear();
            self.suspended_process_instances
                .extend_from_slice(instances);
            if self.sort == Sort::Status {
                self.rebuild();
            }
        }
    }

    fn is_suspended(&self, process: &ProcessInfo) -> bool {
        process.creation_time.is_some_and(|creation_time| {
            self.suspended_process_instances
                .binary_search(&(process.id, creation_time))
                .is_ok()
        })
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
            .all(|i| !live_action_available(&self.processes[*i]))
        {
            "process_list.status_unavailable"
        } else if entry
            .indices
            .iter()
            .any(|i| self.is_suspended(&self.processes[*i]))
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
        let mut groups = BTreeMap::<OsString, Vec<usize>>::new();
        for (i, p) in self.processes.iter().enumerate() {
            if self.hide_inaccessible && inaccessible(p) {
                continue;
            }
            if !search.is_empty() {
                let mut candidate = format!(
                    "{} {} {} {}",
                    p.name,
                    p.id,
                    p.image_path
                        .as_ref()
                        .map(|p| p.to_string_lossy())
                        .unwrap_or_default(),
                    p.user_name.as_deref().unwrap_or("")
                );
                if candidate.is_ascii() {
                    candidate.make_ascii_lowercase();
                } else {
                    candidate = candidate.to_lowercase();
                }
                if !candidate.contains(&search) {
                    continue;
                }
            }
            let path = p.image_path.as_deref().filter(|_| self.grouped);
            let key = path.map_or_else(
                || format!("pid:{}", p.id).into(),
                foreground::executable_grouping_key,
            );
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
                Sort::Cpu => match (self.cpu_total(a), self.cpu_total(b)) {
                    (Some(left), Some(right)) => left.total_cmp(&right),
                    (left, right) => left.is_some().cmp(&right.is_some()),
                },
                Sort::CpuTime => self.cpu_time_total(a).cmp(&self.cpu_time_total(b)),
                Sort::WorkingSet => self
                    .resource_total(a, |sample| sample.working_set_bytes)
                    .cmp(&self.resource_total(b, |sample| sample.working_set_bytes)),
                Sort::PrivateCommit => self
                    .resource_total(a, |sample| sample.private_commit_bytes)
                    .cmp(&self.resource_total(b, |sample| sample.private_commit_bytes)),
                Sort::GpuUsage | Sort::DedicatedGpuMemory | Sort::SharedGpuMemory => {
                    return self
                        .compare_gpu_metrics(a, b)
                        .then_with(|| a.key.cmp(&b.key));
                }
                Sort::User => self.user(a).cmp(&self.user(b)),
                Sort::Status => self.status_key(a).cmp(self.status_key(b)),
                Sort::Name => compare_process_names(
                    &self.processes[a.indices[0]].name,
                    &self.processes[b.indices[0]].name,
                ),
            };
            let ord = if self.descending { ord.reverse() } else { ord };
            ord.then_with(|| a.key.cmp(&b.key))
        });
        self.rows.clear();
        for group in groups {
            let mut children = if group.indices.len() > 1 && self.expanded.contains(&group.key) {
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
            if self.sort.is_gpu_metric() {
                children.sort_by(|a, b| self.compare_gpu_metrics(a, b));
            }
            self.rows.push(group);
            self.rows.extend(children);
        }
        self.offsets = self.row_offsets();
        self.fit_visible_columns();
    }

    fn fit_visible_columns(&mut self) {
        if !self.rows.is_empty() {
            for col in Sort::ALL {
                if self.columns[col as usize] && self.column_widths[col as usize].is_none() {
                    self.column_widths[col as usize] = Some(self.auto_width(col));
                }
            }
        }
    }
    fn cpu_total(&self, e: &Entry) -> Option<f32> {
        e.indices
            .iter()
            .map(|i| self.cpu.get(&self.processes[*i].id).copied())
            .sum()
    }
    fn cpu_time_total(&self, entry: &Entry) -> Option<u64> {
        self.resource_total(entry, |sample| Some(sample.cpu.cpu_time_100ns))
    }

    fn gpu_memory_bytes(&self, entry: &Entry, column: Sort) -> Option<u64> {
        // Cross-process allocations can be counted for multiple members of a group.
        let [index] = entry.indices.as_slice() else {
            return None;
        };
        let sample = self.gpu_memory.get(&self.processes[*index].id)?;
        match column {
            Sort::DedicatedGpuMemory => sample.dedicated_bytes,
            Sort::SharedGpuMemory => sample.shared_bytes,
            _ => None,
        }
    }

    fn gpu_usage_percent(&self, entry: &Entry) -> Option<f32> {
        let [index] = entry.indices.as_slice() else {
            return None;
        };
        self.gpu_usage.get(&self.processes[*index].id).copied()
    }

    fn compare_gpu_metrics(&self, a: &Entry, b: &Entry) -> std::cmp::Ordering {
        if self.sort == Sort::GpuUsage {
            compare_measurements(
                self.gpu_usage_percent(a),
                self.gpu_usage_percent(b),
                self.descending,
                |a, b| a.total_cmp(&b),
            )
        } else {
            compare_measurements(
                self.gpu_memory_bytes(a, self.sort),
                self.gpu_memory_bytes(b, self.sort),
                self.descending,
                |a, b| a.cmp(&b),
            )
        }
    }

    fn resource_total(
        &self,
        entry: &Entry,
        field: impl Fn(&ProcessResourceSample) -> Option<u64>,
    ) -> Option<u64> {
        entry.indices.iter().try_fold(0u64, |total, index| {
            let sample = self.samples.get(&self.processes[*index].id)?;
            total.checked_add(field(sample)?)
        })
    }
    fn user(&self, e: &Entry) -> String {
        let p = &self.processes[e.indices[0]];
        if e.indices.iter().any(|i| {
            self.processes[*i].user_name != p.user_name
                || self.processes[*i].session_id != p.session_id
        }) {
            return t!("process_list.mixed_accounts_or_sessions").to_string();
        }
        let unavailable: std::borrow::Cow<'static, str> = t!("process_list.user_unavailable");
        let user = p.user_name.as_deref().unwrap_or(&unavailable);
        match p.session_id {
            Some(session) => format!("{user} / S{session}"),
            None => format!("{user} / S?"),
        }
    }
    fn immediate_priorities(&self, settings: &Settings, eligible: bool) -> Element<'_, Message> {
        let mut controls = Vec::new();
        for kind in [
            Kind::Process,
            Kind::Thread,
            Kind::Io,
            Kind::Gpu,
            Kind::Memory,
            Kind::DynamicBoost,
        ] {
            let key = format!("nav.{}", kind.key());
            let current = self
                .current
                .iter()
                .copied()
                .find(|v| value_kind(*v) == kind);
            let control: Element<'_, Message> = if eligible {
                let choices = kind
                    .choices(settings.advanced.expose_all_priority_values)
                    .into_iter()
                    .filter(|v| !is_default(*v))
                    .collect::<Vec<_>>();
                pick_list(choices, current, move |v| {
                    Message::Action(Action::Priority(v))
                })
                .option_color(Value::color)
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
            controls.push(super::widgets::setting_row(&key, control).into());
        }
        super::widgets::settings_card(super::widgets::settings_rows(controls)).into()
    }

    pub(super) fn side_panel(&self) -> Element<'_, Message> {
        let mut filters = column![
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
                    text(Sort::WorkingSet.label()).width(Fill),
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
        scrollable(filters.padding([0, design::space::MEDIUM as u16]))
            .height(Fill)
            .into()
    }

    #[cfg(test)]
    fn view<'a>(
        &'a self,
        settings: &'a SettingsEditor,
        plans: PlanCatalog<'a>,
    ) -> Element<'a, Message> {
        self.view_with_header(settings, plans, None, std::convert::identity)
    }

    pub(super) fn view_with_header<'a, M: Clone + 'a>(
        &'a self,
        settings: &'a SettingsEditor,
        plans: PlanCatalog<'a>,
        header: Option<Element<'a, M>>,
        map: impl Fn(Message) -> M + Copy + 'a,
    ) -> Element<'a, M> {
        let count = if self.population_loaded {
            t!("process_list.count", count = self.processes.len())
        } else if self.refreshing || self.error.is_none() {
            t!("common.loading")
        } else {
            t!("common.unavailable")
        };
        let controls = row![
            super::widgets::search_field(
                text_input(&t!("process_list.search_placeholder"), &self.search)
                    .padding(super::widgets::SEARCH_INPUT_PADDING)
                    .on_input(Message::Search)
                    .width(280),
                (!self.search.is_empty()).then(|| Message::Search(String::new())),
            ),
            Space::new().width(Fill),
            text(count.to_string()),
        ]
        .spacing(design::space::SMALL)
        .align_y(iced::Center);
        let mut body = column![controls.width(Fill)]
            .spacing(design::space::SMALL)
            .height(Fill);
        if let Some(e) = &self.error {
            body = body.push(text(e));
        }
        body = body.push(super::widgets::table_frame(responsive(move |size| {
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
            let body_height = size.height - HEADER_HEIGHT - scrolling::SCROLLBAR_GUTTER;
            let range = visible_range_offsets(offsets, &self.offset, body_height);
            let mut rows = column![Space::new().height(offsets[range.start])];
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
                        button(super::motion::wrap(
                            super::navigation::glyph("icons/chevron-right.svg"),
                            self.expanded.contains(&entry.key),
                            super::motion::Effect::Chevron,
                        ))
                        .padding(0)
                        .width(20)
                        .height(20)
                        .style(super::widgets::quiet)
                        .pointer_only()
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
                name = name.push(
                    text(super::widgets::fitted_text(
                        &p.name,
                        name_width - if entry.nested { 78.0 } else { 58.0 },
                        design::typography::BODY,
                    ))
                    .wrapping(iced::widget::text::Wrapping::None),
                );
                let name: Element<'_, Message> = name.into();
                let mut cells = row![container(name).width(name_width).clip(true)]
                    .spacing(design::space::SMALL)
                    .align_y(iced::Center);
                for col in Sort::ALL.into_iter().skip(1) {
                    if !self.columns[col as usize] {
                        continue;
                    }
                    let cell = if col == Sort::Status {
                        status_cell(self.status_key(entry))
                    } else {
                        let value = self.cell_value(entry, col);
                        text(super::widgets::fitted_text(
                            &value,
                            widths[col as usize] - 2.0,
                            design::typography::BODY,
                        ))
                        .wrapping(iced::widget::text::Wrapping::None)
                        .into()
                    };
                    cells = cells.push(container(cell).width(widths[col as usize]).clip(true));
                }
                let focused = self.is_focused(entry);
                visible_rows.push((
                    (p.id, p.creation_time, entry.nested),
                    mouse_area(
                        iced::widget::button(
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
                            .on_double_click(Message::ExpandFocused(None)),
                        )
                        .padding(0)
                        .width(Fill)
                        .on_press(Message::Focus(row_index, self.rows_revision))
                        .style(move |theme, status| {
                            let contrast = design::is_contrast(theme);
                            let mut style = if focused && contrast {
                                super::widgets::selected(theme, status)
                            } else {
                                super::widgets::quiet(theme, status)
                            };
                            if focused
                                && !contrast
                                && matches!(status, iced::widget::button::Status::Active)
                            {
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
                .id(PROCESS_TABLE)
                .height(Fill)
                .on_scroll(|v| Message::Scrolled(v.absolute_offset().y));
            let table = scrolling::buffered(
                table.into(),
                &self.offset,
                offsets[range.start],
                offsets[range.end],
                range.start == 0,
                range.end == self.rows.len(),
                |message| match message {
                    Message::Scrolled(offset) => Some(*offset),
                    _ => None,
                },
            );
            let table = super::motion::wrap_keys(
                table,
                PROCESS_TABLE,
                !self.rows.is_empty(),
                |key, modifiers, bounds| {
                    let height = bounds.height;
                    Some(match (key, modifiers) {
                        (Named::F10, Modifiers::SHIFT) => {
                            (Message::OpenContextFocused(bounds), false)
                        }
                        (key, modifiers) if modifiers.is_empty() => match key {
                            Named::ArrowUp => (Message::MoveRowFocus(false, height), true),
                            Named::ArrowDown => (Message::MoveRowFocus(true, height), true),
                            Named::Enter => (Message::OpenFocused(height), false),
                            Named::Space => (Message::ExpandFocused(Some(height)), false),
                            Named::Delete => (Message::DeleteFocused(height), false),
                            Named::ContextMenu => (Message::OpenContextFocused(bounds), false),
                            _ => return None,
                        },
                        _ => return None,
                    })
                },
            );
            let visible = Sort::ALL
                .into_iter()
                .filter(|col| *col == Sort::Name || self.columns[*col as usize]);
            let table_width = visible
                .map(|col| widths[col as usize] + design::space::SMALL as f32)
                .sum::<f32>()
                - design::space::SMALL as f32
                + 32.0;
            let table = scrolling::horizontal_scrollable(scrolling::table_surface(
                container(column![container(header).height(HEADER_HEIGHT), table].height(Fill))
                    .width(size.width.max(table_width))
                    .height(Fill),
            ))
            .width(Fill)
            .height(Fill);
            let mut layers = iced::widget::Stack::new().push(table);
            if !self.processes.is_empty() && self.rows.is_empty() {
                let inset = design::space::LARGE as f32;
                layers = layers.push(
                    container(
                        text(t!("process_list.no_filter_matches").to_string())
                            .style(text::secondary),
                    )
                    .padding(iced::Padding {
                        top: HEADER_HEIGHT + inset,
                        right: inset,
                        bottom: 0.0,
                        left: inset,
                    })
                    .width(Fill),
                );
            }
            layers.into()
        })));
        let body = Element::from(mouse_area(body).on_move(Message::PointerMoved)).map(map);
        let body = if let Some(header) = header {
            column![header, body]
                .spacing(super::widgets::CARD_GAP)
                .height(Fill)
                .into()
        } else {
            body
        };
        if let Some((stopping, tree)) = &self.stopping {
            let (title, warning, action) = if *tree {
                (
                    "process_list.stop_tree_confirm_title",
                    "process_list.stop_tree_confirm",
                    "process_list.stop_process_tree",
                )
            } else {
                (
                    "process_list.stop_confirm_title",
                    "process_list.stop_confirm",
                    "process_list.stop_process",
                )
            };
            let dialog = container(
                column![
                    super::widgets::heading(
                        t!(title, name = &stopping.name).to_string(),
                        design::typography::DIALOG_TITLE,
                    ),
                    text(t!(warning, name = &stopping.name).to_string()),
                    row![
                        Space::new().width(Fill),
                        action_button(t!("common.cancel").to_string())
                            .style(crate::ui::widgets::tertiary_button)
                            .on_press(Message::CancelStop),
                        action_button(t!(action).to_string())
                            .style(super::widgets::danger_button)
                            .on_press(Message::ConfirmStop),
                    ]
                    .spacing(design::space::SMALL),
                ]
                .spacing(design::space::LARGE),
            )
            .padding(design::space::LARGE as u16)
            .width(460)
            .style(super::widgets::surface);
            return iced::widget::stack![
                super::modal::background(body),
                iced::widget::opaque(container(Space::new().width(Fill).height(Fill)).style(
                    |_| {
                        container::Style {
                            background: Some(iced::Color::from_rgba(0.0, 0.0, 0.0, 0.45).into()),
                            ..Default::default()
                        }
                    }
                )),
                Element::from(
                    container(iced::widget::opaque(dialog))
                        .padding(design::space::MEDIUM as u16)
                        .center_x(Fill)
                        .center_y(Fill),
                )
                .map(map),
            ]
            .into();
        }
        if let Some(selection) = &self.selected {
            let path = selection.path();
            let can_copy_path = selection.path_text().is_some();
            let eligible = selection.processes.iter().any(live_action_available);
            let can_stop = selection.can_stop();
            let can_trim_memory = selection.memory_trim_candidates().next().is_some();
            let tree = selection.processes.len() > 1;
            let (stop_process, stop_tree) = selection.stop_options(&self.processes);
            let stop_label = if tree {
                "process_list.stop_process_tree"
            } else {
                "process_list.stop_process"
            };
            let suspendable = selection.processes.iter().any(|p| {
                live_action_available(p)
                    && p.session_id.is_some_and(|s| s != 0)
                    && p.is_service_account == Some(false)
                    && !crate::app_suspension::is_builtin_excluded(&p.name)
            });
            let suspended = selection.processes.iter().any(|p| self.is_suspended(p));
            let suspension_label = if suspended {
                "process_list.resume_process"
            } else {
                "process_list.suspend_process"
            };
            if let Some(anchor) = self.context {
                return iced::widget::stack![
                    super::modal::background(body),
                    Element::from(responsive(move |_size| {
                        let menu_branch = |branch| {
                            Message::MenuBranch(if eligible { branch } else { MenuBranch::Closed })
                        };
                        let efficiency = button(
                            row![
                                text(t!("process_list.efficiency_mode").to_string()).width(Fill),
                                super::navigation::glyph("icons/chevron-right.svg"),
                            ]
                            .spacing(design::space::SMALL)
                            .height(Fill)
                            .align_y(iced::Center),
                        )
                        .width(Fill)
                        .height(design::BUTTON_HEIGHT)
                        .padding([0, design::space::MEDIUM as u16])
                        .style(super::widgets::quiet)
                        .on_press_maybe(eligible.then_some(menu_branch(MenuBranch::Efficiency)));
                        let mut menu = column![
                            mouse_area(efficiency).on_enter(menu_branch(MenuBranch::Efficiency)),
                            mouse_area(
                                button(
                                    row![
                                        text(t!("nav.priority_control").to_string()).width(Fill),
                                        super::navigation::glyph("icons/chevron-right.svg")
                                    ]
                                    .spacing(design::space::SMALL)
                                    .height(Fill)
                                    .align_y(iced::Center)
                                )
                                .width(Fill)
                                .height(design::BUTTON_HEIGHT)
                                .padding([0, design::space::MEDIUM as u16])
                                .style(super::widgets::quiet)
                                .on_press_maybe(eligible.then_some(menu_branch(MenuBranch::Types)))
                            )
                            .on_enter(menu_branch(MenuBranch::Types))
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
                            (
                                "process_list.trim_memory",
                                Message::Menu(Box::new(Message::Action(Action::TrimMemory))),
                                can_trim_memory,
                            ),
                            ("process_list.stop_process", Message::Stop(false), can_stop),
                            (
                                "process_list.stop_process_tree",
                                Message::Stop(true),
                                can_stop,
                            ),
                            ("process_list.open_rule_details", Message::OpenDetails, true),
                            (
                                "process_list.open_process_location",
                                Message::Menu(Box::new(Message::OpenLocation)),
                                path.is_some(),
                            ),
                            (
                                "process_list.copy_executable_path",
                                Message::Menu(Box::new(Message::CopyExecutablePath)),
                                can_copy_path,
                            ),
                            (
                                "process_list.properties",
                                Message::Menu(Box::new(Message::Properties)),
                                path.is_some(),
                            ),
                        ] {
                            if (key == "process_list.stop_process" && !stop_process)
                                || (key == "process_list.stop_process_tree" && !stop_tree)
                            {
                                continue;
                            }
                            if key == "process_list.open_rule_details" {
                                menu = menu.push(iced::widget::rule::horizontal(1));
                            }
                            menu = menu.push(
                                mouse_area(
                                    action_button(t!(key).to_string())
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
                                                        super::widgets::accent_text(
                                                            theme,
                                                            style.background,
                                                        )
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
                            iced::widget::opaque(scrolling::repaint_popup(
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
                            ))
                        };
                        let mut cascade =
                            row![panel(menu.width(210).into())].spacing(design::space::TINY);
                        if self.menu_branch == MenuBranch::Efficiency {
                            let mut options = column![].spacing(design::space::TINY);
                            for (enabled, key) in
                                [(true, "common.enabled"), (false, "common.disabled")]
                            {
                                options = options.push(
                                    action_button(t!(key).to_string())
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
                                            .height(Fill)
                                            .align_y(iced::Center),
                                        )
                                        .width(Fill)
                                        .height(design::BUTTON_HEIGHT)
                                        .padding([0, design::space::MEDIUM as u16])
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
                                    let selected = self.current.contains(&value);
                                    levels = levels.push(
                                        action_button(value.to_string())
                                            .width(Fill)
                                            .style(move |theme, status| {
                                                let mut style = if selected {
                                                    super::widgets::selected(theme, status)
                                                } else {
                                                    super::widgets::quiet(theme, status)
                                                };
                                                if let Some(color) = value.color(theme) {
                                                    style.text_color = color;
                                                }
                                                style
                                            })
                                            .on_press_maybe(eligible.then_some(Message::Menu(
                                                Box::new(Message::Action(Action::Priority(value))),
                                            ))),
                                    );
                                }
                                cascade = cascade.push(panel(
                                    scrolling::popup_scrollable(levels)
                                        .width(if kind == Kind::DynamicBoost { 100 } else { 160 })
                                        .height(iced::Shrink)
                                        .into(),
                                ));
                            }
                        }
                        iced::widget::float(cascade)
                            .translate(move |bounds, viewport| {
                                let position = anchor.window_position(bounds.position());
                                let desired = iced::Point::new(
                                    position.x - viewport.x,
                                    position.y - viewport.y,
                                );
                                let point = popup_position(desired, viewport.size(), bounds.size());
                                iced::Vector::new(
                                    viewport.x + point.x - bounds.x,
                                    viewport.y + point.y - bounds.y,
                                )
                            })
                            .into()
                    }))
                    .map(map),
                ]
                .into();
            }
            let icon: Element<'_, Message> = path
                .and_then(|path| self.icons.get(path))
                .and_then(Option::as_ref)
                .map(|icon| image((**icon).clone()).width(20).height(20).into())
                .unwrap_or_else(|| super::navigation::glyph("icons/app-window.svg"));
            let header = container(
                row![
                    icon,
                    column![
                        text(&selection.name),
                        text(path.map(Path::to_string_lossy).unwrap_or_default())
                            .size(design::typography::SECONDARY)
                            .style(iced::widget::text::secondary),
                    ]
                    .width(Fill)
                    .spacing(design::space::TIGHT),
                    super::widgets::tooltip(
                        super::widgets::icon_button("icons/copy.svg")
                            .on_press_maybe(can_copy_path.then_some(Message::CopyExecutablePath)),
                        text(t!("process_list.copy_executable_path").to_string()),
                        iced::widget::tooltip::Position::Bottom,
                    ),
                    super::widgets::icon_button("icons/x.svg").on_press(Message::CloseSelection),
                ]
                .spacing(design::space::MEDIUM)
                .align_y(iced::Center),
            )
            .padding(design::space::LARGE as u16);
            let mut tabs = row![];
            for (tab, key) in [
                (ProcessTab::Immediate, "process_list.immediate_actions"),
                (ProcessTab::Rulesets, "process_list.rulesets"),
            ] {
                tabs = tabs.push(super::widgets::panel_tab(
                    t!(key).to_string(),
                    self.process_tab == tab,
                    Message::ProcessTab(tab),
                ));
            }
            let mut pane = column![].spacing(super::widgets::CARD_GAP);
            match self.process_tab {
                ProcessTab::Rulesets => {
                    if let Some(path) = selection.ruleset_path() {
                        let profile = t!(match settings.selected_power_source() {
                            PowerSourceProfile::PluggedIn => "power_source.plugged_in",
                            PowerSourceProfile::OnBattery => "power_source.on_battery",
                        });
                        pane = pane
                            .push(super::widgets::settings_card(super::widgets::setting_row(
                                "power_source.editing_profile",
                                text(profile.into_owned()),
                            )))
                            .push(
                                details::view(settings, path, plans, &self.ruleset_limiter)
                                    .map(Message::Details),
                            );
                    } else {
                        let key = if path.is_some_and(|path| path.to_str().is_none()) {
                            "process_list.rulesets_unsupported_path"
                        } else {
                            "process_list.status_access_denied"
                        };
                        pane = pane.push(text(t!(key)));
                    }
                }
                ProcessTab::Immediate => {
                    let efficiency: Element<'_, Message> = match self.efficiency {
                        Some(enabled) => super::widgets::toggle_card(
                            "process_list.efficiency_mode",
                            enabled,
                            eligible.then_some(|value| Message::Action(Action::Efficiency(value))),
                        ),
                        None => {
                            let unknown = t!("common.unknown").into_owned();
                            let control: Element<'_, Message> = if eligible {
                                use super::widgets::Choice;
                                pick_list(
                                    [
                                        Choice(false, t!("common.disabled").into_owned()),
                                        Choice(true, t!("common.enabled").into_owned()),
                                    ],
                                    None::<Choice<bool>>,
                                    |choice| Message::Action(Action::Efficiency(choice.0)),
                                )
                                .placeholder(unknown)
                                .width(design::SELECT_WIDTH)
                                .into()
                            } else {
                                text(unknown).into()
                            };
                            super::widgets::settings_card(super::widgets::setting_row(
                                "process_list.efficiency_mode",
                                control,
                            ))
                            .into()
                        }
                    };
                    pane = pane
                        .push(
                            mouse_area(efficiency)
                                .on_enter(Message::MenuBranch(MenuBranch::Closed)),
                        )
                        .push(super::widgets::heading(
                            t!("nav.priority_control").to_string(),
                            design::typography::BODY,
                        ))
                        .push(self.immediate_priorities(settings, eligible))
                        .push(
                            row![
                                action_button(t!(suspension_label).to_string()).on_press_maybe(
                                    suspendable
                                        .then_some(Message::Action(Action::Suspend(!suspended)))
                                ),
                                action_button(t!("process_list.open_process_location").to_string())
                                    .on_press_maybe(path.map(|_| Message::OpenLocation)),
                                Space::new().width(Fill),
                                action_button(t!(stop_label).to_string())
                                    .style(super::widgets::danger_button)
                                    .on_press_maybe(can_stop.then_some(Message::Stop(tree))),
                            ]
                            .spacing(design::space::SMALL)
                            .align_y(iced::Center),
                        );
                }
            }
            let mut tabs = column![container(container(tabs).style(super::widgets::tab_bar))
                .padding([design::space::SMALL as u16, design::space::LARGE as u16]),];
            let feedback = match self.process_tab {
                ProcessTab::Immediate => self.detail_feedback.as_ref().map(|result| match result {
                    Ok(message) => (message, false),
                    Err(message) => (message, true),
                }),
                ProcessTab::Rulesets => self.rulesets_error.as_ref().map(|message| (message, true)),
            };
            if let Some((message, is_error)) = feedback {
                tabs = tabs.push(
                    container(
                        text(message)
                            .size(design::typography::SECONDARY)
                            .wrapping(text::Wrapping::WordOrGlyph)
                            .style(if is_error {
                                text::danger
                            } else {
                                text::success
                            }),
                    )
                    .width(Fill)
                    .padding([design::space::SMALL as u16, design::space::LARGE as u16]),
                );
            }
            let details = column![
                header,
                iced::widget::rule::horizontal(1),
                tabs,
                scrollable(
                    container(super::motion::wrap(
                        pane,
                        true,
                        super::motion::Effect::Content(self.process_tab as u64),
                    ))
                    .padding(design::space::LARGE as u16),
                )
                .height(Fill),
            ];
            return iced::widget::stack![
                super::modal::background(body),
                mouse_area(container(Space::new().width(Fill).height(Fill)).style(|_| {
                    container::Style {
                        background: Some(iced::Color::from_rgba(0.0, 0.0, 0.0, 0.45).into()),
                        ..Default::default()
                    }
                }))
                .on_press(map(Message::CloseSelection))
                .on_right_press(map(Message::CloseSelection)),
                Element::from(
                    container(iced::widget::opaque(
                        container(details)
                            .width(Fill)
                            .max_width(960)
                            .height(Fill)
                            .max_height(680)
                            .style(super::widgets::surface)
                    ))
                    .padding(design::space::MEDIUM as u16)
                    .center_x(Fill)
                    .center_y(Fill),
                )
                .map(map),
            ]
            .into();
        }
        // Keep the list subtree stable when opening or closing either overlay.
        iced::widget::stack![body].into()
    }

    pub(super) fn action_toast(&self) -> Option<Element<'_, Message>> {
        let result = &self.action_feedback.as_ref()?.result;
        let (message, style): (_, fn(&iced::Theme) -> text::Style) = match result {
            Ok(message) => (message, text::success),
            Err(message) => (message, text::danger),
        };
        Some(
            container(
                row![
                    text(message)
                        .width(Fill)
                        .size(design::typography::SECONDARY)
                        .wrapping(text::Wrapping::WordOrGlyph)
                        .style(style),
                    action_button(t!("common.close").into_owned())
                        .style(super::widgets::quiet)
                        .on_press(Message::DismissActionFeedback),
                ]
                .spacing(design::space::SMALL)
                .align_y(iced::Center),
            )
            .padding([design::space::TIGHT as u16, design::space::MEDIUM as u16])
            .width(Fill)
            .max_width(420)
            .style(super::widgets::surface)
            .into(),
        )
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

fn shared_value<T: Eq>(
    targets: &[Result<ProcessActionTarget, ProcessActionTargetError>],
    mut read: impl FnMut(&ProcessActionTarget) -> Option<T>,
) -> Option<T> {
    let (first, remaining) = targets.split_first()?;
    let first = read(first.as_ref().ok()?)?;
    for target in remaining {
        if read(target.as_ref().ok()?)? != first {
            return None;
        }
    }
    Some(first)
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
    let (path, creation_time) = process_target_identity(p)?;
    Ok(ProcessActionTarget {
        id: p.id,
        name: p.name.clone(),
        executable_path: path.to_owned(),
        creation_time,
        session_id: p.session_id,
        is_service_account: p.is_service_account,
    })
}
fn process_target_identity(p: &ProcessInfo) -> Result<(&Path, u64), ProcessActionTargetError> {
    if protected(p) || p.id == std::process::id() {
        return Err(ProcessActionTargetError::ProtectedProcess);
    }
    p.image_path
        .as_deref()
        .filter(|p| p.is_absolute())
        .zip(p.creation_time)
        .ok_or(ProcessActionTargetError::IdentityUnavailable)
}
fn status_cell(key: &'static str) -> Element<'static, Message> {
    let icon = match key {
        "process_list.status_suspended" => "icons/pause.svg",
        "process_list.status_efficiency_mode" => "icons/leaf.svg",
        "process_list.status_active" => "icons/play.svg",
        "process_list.status_system_protected" => "icons/shield.svg",
        "process_list.status_access_denied" => "icons/ban.svg",
        _ => "icons/circle-help.svg",
    };
    let color: fn(&iced::Theme) -> iced::Color = match key {
        "process_list.status_suspended" => |theme| theme.palette().warning,
        "process_list.status_efficiency_mode" => |theme| theme.palette().success,
        "process_list.status_active" => |theme| {
            super::widgets::accent_text(
                theme,
                Some(theme.palette().primary.scale_alpha(0.20).into()),
            )
        },
        _ => super::widgets::muted_color,
    };
    let label: std::borrow::Cow<'static, str> = t!(key);
    let cell = row![
        super::assets::icon(icon, design::ICON_SIZE, color),
        text(label)
            .wrapping(iced::widget::text::Wrapping::None)
            .style(move |theme| text::Style {
                color: Some(color(theme))
            }),
    ]
    .spacing(design::space::CONTROL)
    .align_y(iced::Center);
    let help_key = match key {
        "process_list.status_system_protected" => "process_list.protected_system_process_help",
        "process_list.status_access_denied" => "process_list.access_denied_help",
        _ => return cell.into(),
    };
    super::widgets::tooltip(
        cell,
        text(t!(help_key).to_string()),
        iced::widget::tooltip::Position::Top,
    )
    .into()
}

fn protected(p: &ProcessInfo) -> bool {
    p.is_critical == Some(true)
        || foreground::contains_process_name(foreground::CORE_BUILT_IN_PROCESS_EXCLUSIONS, &p.name)
}
fn compare_process_names(left: &str, right: &str) -> std::cmp::Ordering {
    if left.is_ascii() && right.is_ascii() {
        left.bytes()
            .map(|byte| byte.to_ascii_lowercase())
            .cmp(right.bytes().map(|byte| byte.to_ascii_lowercase()))
    } else {
        left.to_lowercase().cmp(&right.to_lowercase())
    }
}
fn inaccessible(p: &ProcessInfo) -> bool {
    p.image_path.is_none()
        || p.id == 0
        || p.id == std::process::id()
        || !p.can_set_information
        || p.is_critical.is_none()
        || protected(p)
}
fn live_action_available(p: &ProcessInfo) -> bool {
    !inaccessible(p) && process_target_identity(p).is_ok()
}
fn format_memory_usage(bytes: u64, total: Option<u64>, percentage: bool) -> String {
    if percentage {
        match total.filter(|total| *total > 0) {
            Some(total) => format!("{:.1}%", bytes as f64 / total as f64 * 100.0),
            None => t!("common.unknown").to_string(),
        }
    } else {
        format_megabytes(bytes)
    }
}

fn format_megabytes(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / 1_000_000.0)
}

fn compare_measurements<T>(
    left: Option<T>,
    right: Option<T>,
    descending: bool,
    compare: impl FnOnce(T, T) -> std::cmp::Ordering,
) -> std::cmp::Ordering {
    match (left, right) {
        (Some(left), Some(right)) if descending => compare(right, left),
        (Some(left), Some(right)) => compare(left, right),
        (left, right) => right.is_some().cmp(&left.is_some()),
    }
}

fn measure_column_text(content: &str) -> f32 {
    super::widgets::text_width(content, design::typography::BODY)
}

fn format_cpu_time(ticks_100ns: u64) -> String {
    let seconds = ticks_100ns / 10_000_000;
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
}

fn column_width(c: Sort) -> f32 {
    match c {
        Sort::Name => 230.0,
        Sort::Pid => 70.0,
        Sort::Cpu | Sort::CpuTime | Sort::GpuUsage | Sort::WorkingSet => 100.0,
        Sort::PrivateCommit => 130.0,
        Sort::DedicatedGpuMemory | Sort::SharedGpuMemory => 190.0,
        Sort::Status => 180.0,
        Sort::User => 200.0,
    }
}
fn visible_range_offsets(
    offsets: &[f32],
    offset: &std::cell::Cell<f32>,
    height: f32,
) -> Range<usize> {
    let count = offsets.len().saturating_sub(1);
    let height = height.max(0.0);
    let max_offset = (offsets.last().copied().unwrap_or_default() - height).max(0.0);
    let bounded = offset.get().clamp(0.0, max_offset);
    // Keyboard focus uses the same offset even when Iced skips an on_scroll callback.
    offset.set(bounded);
    let start = offsets
        .partition_point(|p| *p <= bounded)
        .saturating_sub(1)
        .min(count)
        .saturating_sub(8);
    let end = offsets
        .partition_point(|p| *p < bounded + height)
        .saturating_add(9)
        .min(count);
    start..end.max(start)
}
#[cfg(test)]
fn visible_range(count: usize, offset: f32, height: f32) -> Range<usize> {
    let offsets = (0..=count)
        .map(|i| i as f32 * ROW_HEIGHT)
        .collect::<Vec<_>>();
    visible_range_offsets(&offsets, &std::cell::Cell::new(offset), height)
}

fn background<T: Send + 'static>(
    work: impl FnOnce() -> Result<T, String> + Send + 'static,
    map: impl Fn(Result<T, String>) -> Message + Send + 'static,
) -> Task<Message> {
    super::tasks::run(work).map(move |result| map(result.and_then(|value| value)))
}

fn start_gpu_population_worker() -> std::io::Result<GpuPopulationWorker> {
    let (worker, requests) = std::sync::mpsc::channel::<(bool, PopulationReply)>();
    std::thread::Builder::new()
        .name("winderust-process-list".into())
        .spawn(move || {
            let mut gpu_usage = None;
            while let Ok((sample_gpu_memory, reply)) = requests.recv() {
                let _ = reply.send(load_population(sample_gpu_memory, Some(&mut gpu_usage)));
            }
        })?;
    Ok(worker)
}

fn sample_gpu_usage(query: &mut Option<GpuUsageQuery>) -> Option<HashMap<u32, f32>> {
    let priming = query.is_none();
    if priming {
        *query = GpuUsageQuery::open().ok();
    }
    match query.as_ref()?.sample_processes() {
        Ok(samples) if !priming => samples,
        Ok(_) => None,
        Err(_) => {
            *query = None;
            None
        }
    }
}

fn load_population(
    sample_gpu_memory: bool,
    gpu_usage_query: Option<&mut Option<GpuUsageQuery>>,
) -> Result<Population, String> {
    let processes = foreground::list_processes_with_paths()?;
    let gpu_usage = gpu_usage_query
        .and_then(sample_gpu_usage)
        .unwrap_or_default();
    let mut gpu_memory = if sample_gpu_memory {
        gpu_memory::sample().unwrap_or_default()
    } else {
        BTreeMap::new()
    };
    let samples = foreground::sample_process_resources(
        &processes,
        foreground::ProcessResourceSampling::WithEfficiencyMode,
    );
    // Verify that the catalog process still owns its PID after the GPU counter snapshot.
    gpu_memory.retain(|pid, _| samples.contains_key(pid));
    Ok(Population {
        total_memory_bytes: crate::dashboard_metrics::sample_memory_usage().total_physical_bytes,
        processes,
        samples,
        gpu_memory,
        gpu_usage,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grouped_resources_require_every_member_sample() {
        let mut list = ProcessList {
            processes: vec![process(10, 1, "app.exe"), process(11, 2, "app.exe")],
            cpu: HashMap::from([(10, 0.0)]),
            ..ProcessList::default()
        };
        let group = Entry {
            key: "app.exe".into(),
            indices: vec![0, 1],
            nested: false,
        };
        let sample =
            |creation_time, working_set_bytes, private_commit_bytes| ProcessResourceSample {
                cpu: crate::cpu::ProcessCpuSample {
                    cpu_time_100ns: 0,
                    sampled_at: std::time::Instant::now(),
                },
                creation_time,
                working_set_bytes,
                private_commit_bytes,
                efficiency_mode: None,
            };
        list.samples.insert(10, sample(1, Some(0), Some(100)));
        list.samples.insert(11, sample(2, None, None));
        assert_eq!(list.cpu_total(&group), None);
        assert_eq!(list.resource_total(&group, |s| s.working_set_bytes), None);
        assert_eq!(
            list.resource_total(&group, |s| s.private_commit_bytes),
            None
        );

        list.cpu.insert(11, 0.0);
        list.samples.insert(11, sample(2, Some(0), Some(42)));
        assert_eq!(list.cpu_total(&group), Some(0.0));
        assert_eq!(
            list.resource_total(&group, |s| s.working_set_bytes),
            Some(0)
        );
        assert_eq!(
            list.resource_total(&group, |s| s.private_commit_bytes),
            Some(142)
        );
    }

    #[test]
    fn cpu_time_totals_use_raw_ticks_and_require_every_group_member() {
        let mut list = ProcessList {
            processes: vec![
                process(10, 1, "z.exe"),
                process(11, 2, "z.exe"),
                process(12, 3, "a.exe"),
            ],
            grouped: true,
            sort: Sort::CpuTime,
            descending: true,
            ..ProcessList::default()
        };
        for (process, cpu_time_100ns) in list
            .processes
            .iter()
            .zip([40_000_002, 40_000_000, 80_000_001])
        {
            list.samples.insert(
                process.id,
                ProcessResourceSample {
                    cpu: crate::cpu::ProcessCpuSample {
                        cpu_time_100ns,
                        sampled_at: std::time::Instant::now(),
                    },
                    creation_time: process.creation_time.unwrap(),
                    working_set_bytes: None,
                    private_commit_bytes: None,
                    efficiency_mode: None,
                },
            );
        }
        list.rebuild();
        assert_eq!(list.rows[0].indices, [0, 1]);
        assert_eq!(list.cell_value(&list.rows[0], Sort::CpuTime), "00:00:08");
        assert_eq!(list.cell_value(&list.rows[1], Sort::CpuTime), "00:00:08");

        list.samples.remove(&11);
        list.rebuild();
        assert_eq!(list.rows[0].indices, [2]);
        assert_eq!(
            list.cell_value(&list.rows[1], Sort::CpuTime),
            t!("common.unknown")
        );
        assert_eq!(format_cpu_time(0), "00:00:00");
        assert_eq!(format_cpu_time(3_661_000_000_000), "101:41:40");
    }

    #[test]
    fn private_commit_sorts_by_bytes_and_remains_in_mb_when_working_set_uses_percent() {
        let mut list = ProcessList {
            processes: vec![
                process(10, 1, "a.exe"),
                process(11, 2, "b.exe"),
                process(12, 3, "c.exe"),
                process(13, 4, "d.exe"),
            ],
            total_memory_bytes: Some(10_000_000),
            memory_percentage: true,
            sort: Sort::PrivateCommit,
            descending: true,
            ..ProcessList::default()
        };
        for (process, private_commit_bytes) in
            list.processes
                .iter()
                .zip([Some(1_000_001), Some(1_000_002), None, Some(0)])
        {
            list.samples.insert(
                process.id,
                ProcessResourceSample {
                    cpu: crate::cpu::ProcessCpuSample {
                        cpu_time_100ns: 0,
                        sampled_at: std::time::Instant::now(),
                    },
                    creation_time: process.creation_time.unwrap(),
                    working_set_bytes: Some(2_000_000),
                    private_commit_bytes,
                    efficiency_mode: None,
                },
            );
        }
        list.rebuild();
        assert_eq!(
            list.rows
                .iter()
                .map(|row| row.indices[0])
                .collect::<Vec<_>>(),
            [1, 0, 3, 2]
        );
        assert_eq!(list.cell_value(&list.rows[0], Sort::WorkingSet), "20.0%");
        assert_eq!(
            list.cell_value(&list.rows[0], Sort::PrivateCommit),
            "1.0 MB"
        );
        assert_eq!(
            list.cell_value(&list.rows[2], Sort::PrivateCommit),
            "0.0 MB"
        );
        assert_eq!(
            list.cell_value(&list.rows[3], Sort::PrivateCommit),
            t!("common.unknown")
        );
    }

    #[test]
    fn gpu_memory_sorts_bytes_and_omits_group_totals_and_unavailable_values() {
        let mut list = ProcessList {
            processes: vec![
                process(10, 1, "a.exe"),
                process(11, 2, "a.exe"),
                process(12, 3, "b.exe"),
                process(13, 4, "c.exe"),
            ],
            gpu_memory: BTreeMap::from([
                (
                    10,
                    GpuMemorySample {
                        dedicated_bytes: Some(1_000_001),
                        shared_bytes: Some(0),
                    },
                ),
                (
                    11,
                    GpuMemorySample {
                        dedicated_bytes: Some(1_000_002),
                        shared_bytes: Some(1),
                    },
                ),
                (
                    12,
                    GpuMemorySample {
                        dedicated_bytes: Some(0),
                        shared_bytes: None,
                    },
                ),
            ]),
            gpu_usage: HashMap::from([(10, 1.001), (11, 1.002), (12, 0.0)]),
            expanded: HashSet::from([foreground::executable_grouping_key(Path::new("a.exe"))]),
            sort: Sort::DedicatedGpuMemory,
            descending: true,
            memory_percentage: true,
            ..ProcessList::default()
        };
        let indices = |list: &ProcessList| {
            list.rows
                .iter()
                .map(|row| row.indices[0])
                .collect::<Vec<_>>()
        };

        list.rebuild();
        assert_eq!(indices(&list), [2, 0, 1, 0, 3]);
        assert_eq!(
            list.cell_value(&list.rows[0], Sort::DedicatedGpuMemory),
            "0 MB"
        );
        assert_eq!(list.cell_value(&list.rows[0], Sort::SharedGpuMemory), "—");
        assert_eq!(
            list.cell_value(&list.rows[1], Sort::DedicatedGpuMemory),
            "—"
        );
        assert_eq!(
            list.cell_value(&list.rows[2], Sort::DedicatedGpuMemory),
            "1.0 MB"
        );
        assert_eq!(
            list.cell_value(&list.rows[2], Sort::SharedGpuMemory),
            "<0.1 MB"
        );
        assert_eq!(
            list.cell_value(&list.rows[3], Sort::SharedGpuMemory),
            "0 MB"
        );

        list.grouped = false;
        list.rebuild();
        assert_eq!(indices(&list), [1, 0, 2, 3]);
        list.descending = false;
        list.rebuild();
        assert_eq!(indices(&list), [2, 0, 1, 3]);

        list.grouped = true;
        list.sort = Sort::GpuUsage;
        list.descending = true;
        list.rebuild();
        assert_eq!(indices(&list), [2, 0, 1, 0, 3]);
        assert_eq!(list.cell_value(&list.rows[0], Sort::GpuUsage), "0.0%");
        assert_eq!(list.cell_value(&list.rows[1], Sort::GpuUsage), "—");
        assert_eq!(list.cell_value(&list.rows[2], Sort::GpuUsage), "1.0%");
        assert_eq!(list.cell_value(&list.rows[4], Sort::GpuUsage), "—");
    }

    #[test]
    fn process_name_order_preserves_unicode_lowercase_and_key_ties() {
        let names = [
            "", "app.exe", "APP.exe", "aa", "İ", "i\u{307}", "Σ", "σ", "ς", "ß", "ss",
        ];
        for left in names {
            for right in names {
                assert_eq!(
                    compare_process_names(left, right),
                    left.to_lowercase().cmp(&right.to_lowercase()),
                    "{left:?} compared with {right:?}"
                );
            }
        }
    }

    #[test]
    fn search_and_name_order_preserve_large_grouped_populations() {
        for count in [50, 500] {
            let names = [
                "aLpHa.exe",
                "ALPHA.exe",
                "İ.exe",
                "i\u{307}.exe",
                "Σ.exe",
                "σ.exe",
                "ß.exe",
                "SS.exe",
            ];
            let processes = (0..count)
                .map(|index| {
                    let name = names[index / 2 % names.len()];
                    let mut item = process(
                        index as u32 + 10,
                        index as u64 + 1,
                        &format!(r"C:\Apps\bucket{:03}\{name}", index / 2),
                    );
                    item.name = name.into();
                    item.user_name = Some(format!("User-{}", index % 3));
                    item.can_set_information = index % 17 != 0;
                    item
                })
                .collect::<Vec<_>>();
            for grouped in [false, true] {
                for hide_inaccessible in [false, true] {
                    for query in ["", "  ", "alpha", "σ", "User-1", "42"] {
                        let search = query.to_lowercase();
                        let visible = processes
                            .iter()
                            .enumerate()
                            .filter(|(_, p)| {
                                (!hide_inaccessible || !inaccessible(p))
                                    && [
                                        p.name.clone(),
                                        p.id.to_string(),
                                        p.image_path
                                            .as_ref()
                                            .unwrap()
                                            .to_string_lossy()
                                            .into_owned(),
                                        p.user_name.clone().unwrap(),
                                    ]
                                    .join(" ")
                                    .to_lowercase()
                                    .contains(&search)
                            })
                            .map(|(index, _)| index)
                            .collect::<Vec<_>>();
                        let expected_ids = visible
                            .iter()
                            .map(|index| processes[*index].id)
                            .collect::<std::collections::BTreeSet<_>>();
                        for sort in Sort::ALL {
                            for descending in [false, true] {
                                let mut list = ProcessList {
                                    processes: processes.clone(),
                                    search: query.into(),
                                    grouped,
                                    hide_inaccessible,
                                    sort,
                                    descending,
                                    column_widths: [Some(100.0); Sort::ALL.len()],
                                    ..ProcessList::default()
                                };
                                list.rebuild();
                                let actual_ids = list
                                    .rows
                                    .iter()
                                    .flat_map(|row| &row.indices)
                                    .map(|index| processes[*index].id)
                                    .collect::<std::collections::BTreeSet<_>>();
                                assert_eq!(actual_ids, expected_ids);
                                if sort == Sort::Name {
                                    let mut groups = BTreeMap::new();
                                    for index in &visible {
                                        let item = &processes[*index];
                                        let key = if grouped {
                                            foreground::executable_path_key(
                                                item.image_path.as_ref().unwrap(),
                                            )
                                        } else {
                                            format!("pid:{}", item.id)
                                        };
                                        groups.entry(key).or_insert(*index);
                                    }
                                    let mut expected = groups.into_iter().collect::<Vec<_>>();
                                    expected.sort_by(|(left_key, left), (right_key, right)| {
                                        let order = processes[*left]
                                            .name
                                            .to_lowercase()
                                            .cmp(&processes[*right].name.to_lowercase());
                                        (if descending { order.reverse() } else { order })
                                            .then_with(|| left_key.cmp(right_key))
                                    });
                                    assert_eq!(
                                        list.rows
                                            .iter()
                                            .map(|row| row.key.to_str().unwrap())
                                            .collect::<Vec<_>>(),
                                        expected
                                            .iter()
                                            .map(|(key, _)| key.as_str())
                                            .collect::<Vec<_>>()
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn navigation_and_ticks_share_one_refresh_deadline() {
        use std::time::{Duration, Instant};

        let mut list = ProcessList::default();
        let start = Instant::now();
        assert_eq!(list.refresh_if_due(start).units(), 1);
        assert!(list.refreshing);

        // A slow population/icon query must remain the only refresh in flight.
        assert_eq!(
            list.refresh_if_due(start + Duration::from_secs(2)).units(),
            0
        );
        assert_eq!(list.refresh_started_at, Some(start));

        // Simulate completion so repeat visits exercise the freshness guard.
        list.refreshing = false;
        for milliseconds in [0, 10, 250, 500, 999] {
            assert_eq!(
                list.refresh_if_due(start + Duration::from_millis(milliseconds))
                    .units(),
                0
            );
        }
        let next = start + REFRESH_INTERVAL;
        assert_eq!(list.refresh_if_due(next).units(), 1);
        assert_eq!(list.refresh_started_at, Some(next));
        list.refreshing = false;
        assert_eq!(
            list.refresh_if_due(next + Duration::from_millis(250))
                .units(),
            0
        );
    }

    #[test]
    fn rulesets_need_a_representable_selected_process_path() {
        use std::{ffi::OsString, os::windows::ffi::OsStringExt};

        let raw_path = PathBuf::from(OsString::from_wide(&[0xd800]));
        let mut list = ProcessList {
            process_tab: ProcessTab::Rulesets,
            ..ProcessList::default()
        };
        assert!(!list.rulesets_open());
        list.selected = Some(Selection {
            nested: false,
            processes: vec![process(1, 1, "app.exe")],
            name: "app.exe".into(),
        });
        assert!(list.rulesets_open());
        list.process_tab = ProcessTab::Immediate;
        assert!(!list.rulesets_open());
        list.process_tab = ProcessTab::Rulesets;
        list.selected.as_mut().unwrap().processes[0].image_path = Some(raw_path);
        assert!(!list.rulesets_open());
    }
    #[test]
    fn dashboard_transitions_keep_repaints_bounded() {
        use crate::automation::RuntimeStatusSnapshot;
        use iced::advanced::{layout, renderer::Renderer as _, widget::Tree, Layout};
        use iced::{mouse, Rectangle, Size};

        let mut list = ProcessList {
            processes: (1..=500)
                .map(|id| {
                    let mut process = process(id, id as u64, &format!("application-{id}.exe"));
                    process.name = format!("application-{id}-long-process-name.exe");
                    process
                })
                .collect(),
            ..ProcessList::default()
        };
        list.rebuild();
        let settings = SettingsEditor::with_settings(Settings::default());
        let mut dashboard = super::super::dashboard::Model::default();
        dashboard.populate_smoke_history();
        let log = super::super::action_log::Editor::default();
        let entries: Vec<_> = (1..=500)
            .map(|id| crate::action_log::ActionLogEntry {
                sequence: id,
                batch_id: id,
                timestamp_epoch_ms: 1_700_000_000_000 + u128::from(id),
                feature: crate::action_log::ActionLogFeature::ProcessPriority,
                process_id: Some(id as u32),
                process_name: format!("application-{id}.exe"),
                result: crate::action_log::ActionLogResult::Applied,
                reason: "Process Priority applied".into(),
            })
            .collect();
        let status = RuntimeStatusSnapshot {
            action_log_entries: Arc::new(entries),
            ..Default::default()
        };
        let mut renderer = iced::Renderer::new(design::typography::FONT, iced::Pixels(14.0));
        let bounds = Rectangle::with_size(Size::new(1400.0, 900.0));
        let mut tree = Tree::empty();
        let mut previous = Vec::new();
        for frame in 0..16 {
            let dock_closed = frame < 8 && frame % 2 == 0;
            let log_page = frame >= 8 && frame % 2 == 0;
            let content = if log_page {
                log.view(&status.action_log_entries, &[])
                    .map(|_| Message::Refresh)
            } else {
                list.view_with_header(
                    &settings,
                    PlanCatalog::Loaded(&[]),
                    Some(dashboard.metrics().map(|_| Message::Refresh)),
                    std::convert::identity,
                )
            };
            let page = super::super::motion::wrap(
                scrolling::repaint_group(content),
                true,
                super::super::motion::Effect::Content(u64::from(log_page)),
            );
            let mut view: Element<'_, Message> = if dock_closed {
                page
            } else {
                let panel = if log_page {
                    log.side_panel(crate::config::ActionLogMode::Full, true, true)
                        .map(|_| Message::Refresh)
                } else {
                    list.side_panel()
                };
                row![
                    container(page).width(Fill).height(Fill),
                    // The production dock groups its surface into one repaint layer.
                    scrolling::repaint_group(container(panel).width(300).height(Fill)),
                ]
                .height(Fill)
                .into()
            };
            tree.diff(&view);
            let node = view.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(Size::ZERO, bounds.size()),
            );
            renderer.reset(bounds);
            view.as_widget().draw(
                &tree,
                &mut renderer,
                &iced::Theme::Light,
                &iced::advanced::renderer::Style::default(),
                Layout::new(&node),
                mouse::Cursor::Unavailable,
                &bounds,
            );
            let damage = iced::advanced::graphics::damage::group(
                iced::advanced::graphics::damage::diff(
                    &previous,
                    renderer.layers(),
                    |layer| vec![layer.bounds],
                    iced_tiny_skia::Layer::damage,
                ),
                bounds,
            );
            previous = renderer.layers().to_vec();
            assert!(
                damage.len() <= 4,
                "Dashboard transition {frame} fragmented into {} repaint regions",
                damage.len()
            );
            let damaged_area: f32 = damage
                .iter()
                .map(|region| region.width * region.height)
                .sum();
            assert!(damaged_area <= bounds.width * bounds.height * 1.25);
        }
    }

    #[test]
    #[ignore = "Measures live Process List opening; run manually with --nocapture"]
    fn process_list_open_timing() {
        use iced::advanced::{layout, widget::Tree};
        use iced::{Rectangle, Size};
        use std::time::Instant;

        let font_start = Instant::now();
        let _ = super::super::widgets::text_width("Dashboard", design::typography::BODY);
        eprintln!("Font warmup: {:?}", font_start.elapsed());
        let start = Instant::now();
        let population = load_population(false, None).unwrap();
        let sampled = start.elapsed();
        let mut list = ProcessList {
            processes: population.processes,
            population_loaded: true,
            samples: population.samples,
            total_memory_bytes: population.total_memory_bytes,
            ..ProcessList::default()
        };
        list.rebuild();
        let rebuilt = start.elapsed();
        let settings = SettingsEditor::with_settings(Settings::default());
        let mut view = list.view(&settings, PlanCatalog::Loaded(&[]));
        let mut tree = Tree::new(&view);
        let renderer = iced::Renderer::new(design::typography::FONT, iced::Pixels(14.0));
        let bounds = Rectangle::with_size(Size::new(1200.0, 800.0));
        let _ = view.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, bounds.size()),
        );
        let ready = start.elapsed();
        drop(view);
        let mut icons = HashMap::new();
        for process in &list.processes {
            if let Some(path) = &process.image_path {
                icons
                    .entry(path.clone())
                    .or_insert_with(|| crate::process_icon::load_process_icon(path));
            }
        }
        eprintln!("Process List: processes={}, rows={}, population={sampled:?}, rebuild={:?}, view/layout={:?}, rows ready={ready:?}, subsequent icons={:?}", list.processes.len(), list.rows.len(), rebuilt - sampled, ready - rebuilt, start.elapsed() - ready);
    }

    #[test]
    fn population_is_visible_before_icons_and_refreshes_do_not_overlap() {
        let mut settings = Settings::default();
        settings.general.enabled = false;
        let mut editor = SettingsEditor::with_settings(settings);
        editor.select_power_source(PowerSourceProfile::OnBattery);
        let runtime = RuntimeHandle::start(&editor.runtime_settings_snapshot());
        let mut list = ProcessList::default();
        assert!(!list.population_loaded);
        let path = PathBuf::from("missing-process-list-test.exe");
        list.icons.insert(PathBuf::from("exited.exe"), None);
        let population = || Population {
            processes: vec![process(10, 100, path.to_str().unwrap())],
            samples: BTreeMap::new(),
            gpu_memory: BTreeMap::new(),
            gpu_usage: HashMap::new(),
            total_memory_bytes: None,
        };
        let icons = list.update(Message::Loaded(Ok(population())), &mut editor, &runtime);
        assert_eq!(list.rows.len(), 1);
        assert!(list.population_loaded);
        assert!(list.refreshing);
        assert!(list.icons.is_empty());
        assert!(icons.units() > 0);
        assert_eq!(
            list.update(Message::Refresh, &mut editor, &runtime).units(),
            0
        );
        drop(icons);

        let _ = list.update(
            Message::IconsLoaded(Ok(HashMap::from([(path.clone(), None)]))),
            &mut editor,
            &runtime,
        );
        assert!(!list.refreshing);
        assert!(list.icons.contains_key(&path));
        list.record_action_result(Ok("Process action completed".into()), None);
        // Cache failed extractions too, so subsequent refreshes do not repeat them.
        assert_eq!(
            list.update(Message::Loaded(Ok(population())), &mut editor, &runtime)
                .units(),
            0
        );
        assert!(!list.refreshing);
        assert_eq!(
            list.action_feedback
                .as_ref()
                .map(|feedback| &feedback.result),
            Some(&Ok("Process action completed".into()))
        );
        let _ = list.update(
            Message::IconsLoaded(Err("worker unavailable".into())),
            &mut editor,
            &runtime,
        );
        assert!(!list.refreshing);
        assert!(list.population_loaded);
        let _ = list.update(Message::DismissActionFeedback, &mut editor, &runtime);
        assert!(list.action_feedback.is_none());
        assert_eq!(list.error.as_deref(), Some("worker unavailable"));
        assert!(editor.global().on_battery.is_none());
    }

    #[test]
    fn header_stays_clickable_after_scrolling_process_rows() {
        use iced::advanced::{layout, widget::Tree, Layout, Shell};
        use iced::{mouse, Event, Point, Rectangle, Size};
        let mut list = ProcessList {
            processes: (1..101)
                .map(|id| process(id, id as u64, "application.exe"))
                .collect(),
            ..ProcessList::default()
        };
        list.grouped = false;
        list.rebuild();
        let settings = SettingsEditor::with_settings(Settings::default());
        let mut view = list.view(&settings, PlanCatalog::Loaded(&[]));
        let mut tree = Tree::new(&view);
        let renderer = iced::Renderer::new(design::typography::FONT, iced::Pixels(14.0));
        let bounds = Rectangle::with_size(Size::new(1000.0, 500.0));
        let node = view.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, bounds.size()),
        );
        let mut messages = Vec::new();
        for (event, cursor) in [
            (
                mouse::Event::WheelScrolled {
                    delta: mouse::ScrollDelta::Pixels { x: 0.0, y: -144.0 },
                },
                bounds.center(),
            ),
            (
                mouse::Event::ButtonPressed(mouse::Button::Left),
                Point::new(50.0, 50.0),
            ),
            (
                mouse::Event::ButtonReleased(mouse::Button::Left),
                Point::new(50.0, 50.0),
            ),
        ] {
            view.as_widget_mut().update(
                &mut tree,
                &Event::Mouse(event),
                Layout::new(&node),
                mouse::Cursor::Available(cursor),
                &renderer,
                &mut iced::advanced::clipboard::Null,
                &mut Shell::new(&mut messages),
                &bounds,
            );
        }
        assert!(list.offset.get() > 0.0);
        assert!(messages
            .iter()
            .any(|message| matches!(message, Message::Sort(Sort::Name))));
    }

    #[test]
    fn process_scroll_damage_stays_bounded() {
        let mut list = ProcessList {
            processes: (1..2001)
                .map(|id| process(id, id as u64, &format!("application-{id}.exe")))
                .collect(),
            ..ProcessList::default()
        };
        list.rebuild();
        let settings = SettingsEditor::with_settings(Settings::default());
        scrolling::check_scroll_damage(
            list.view(&settings, PlanCatalog::Loaded(&[])),
            iced::Rectangle::with_size(iced::Size::new(1920.0, 1080.0)),
        );
    }

    #[test]
    fn stop_confirmation_preserves_scope_labels_and_cancel() {
        use iced::advanced::{
            layout,
            widget::{Id, Operation, Tree},
            Layout,
        };

        struct Labels(Vec<String>);
        impl Operation for Labels {
            fn traverse(&mut self, operate: &mut dyn FnMut(&mut dyn Operation)) {
                operate(self);
            }
            fn text(&mut self, _id: Option<&Id>, _bounds: iced::Rectangle, text: &str) {
                self.0.push(text.to_owned());
            }
        }

        let mut settings = Settings::default();
        settings.general.enabled = false;
        let mut editor = SettingsEditor::with_settings(settings);
        let runtime = RuntimeHandle::start(&editor.runtime_settings_snapshot());
        let selection = Selection {
            nested: false,
            processes: vec![
                process(10, 100, r"C:\Apps\app.exe"),
                process(11, 200, r"C:\Apps\app.exe"),
            ],
            name: "app.exe".into(),
        };
        let mut list = ProcessList {
            processes: selection.processes.clone(),
            ..ProcessList::default()
        };
        list.rebuild();
        assert!(list.focus_row(0, list.rows_revision));
        assert!(!list.modal_open());
        list.processes.push(process(12, 300, r"C:\Apps\app.exe"));
        list.rebuild();
        let _ = list.update(Message::DeleteFocused(200.0), &mut editor, &runtime);
        assert!(list.modal_open());
        let (pending, tree) = list.stopping.as_ref().unwrap();
        assert_eq!(pending.processes.len(), 3);
        assert!(!tree);
        let _ = list.update(Message::CancelStop, &mut editor, &runtime);
        assert!(list.stopping.is_none());
        assert!(!list.modal_open());
        for tree in [false, true] {
            list.selected = Some(selection.clone());
            assert!(list.modal_open());
            list.context = Some(ContextAnchor::Pointer(Point::ORIGIN));
            assert!(!list.modal_open());
            list.context = None;
            let _ = list.update(Message::Stop(tree), &mut editor, &runtime);
            assert_eq!(list.stopping.as_ref().unwrap().1, tree);
            {
                let mut view = list.view(&editor, PlanCatalog::Loaded(&[]));
                let mut state = Tree::new(&view);
                let renderer = iced::Renderer::new(design::typography::FONT, iced::Pixels(14.0));
                let node = view.as_widget_mut().layout(
                    &mut state,
                    &renderer,
                    &layout::Limits::new(iced::Size::ZERO, iced::Size::new(900.0, 600.0)),
                );
                let mut labels = Labels(Vec::new());
                view.as_widget_mut().operate(
                    &mut state,
                    Layout::new(&node),
                    &renderer,
                    &mut labels,
                );
                let (title, action) = if tree {
                    ("Stop app.exe and its child processes?", "Stop process tree")
                } else {
                    ("Stop app.exe?", "Stop process")
                };
                assert!(
                    labels.0.iter().any(|label| label == title),
                    "{:#?}",
                    labels.0
                );
                assert!(
                    labels.0.ends_with(&["Cancel".into(), action.into()]),
                    "{:#?}",
                    labels.0
                );
            }
            let _ = list.update(Message::CancelStop, &mut editor, &runtime);
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
    fn trim_memory_captures_verified_members_of_the_selected_row() {
        let path = r"C:\Apps\app.exe";
        let mut selection = Selection {
            nested: true,
            processes: vec![process(10, 100, path)],
            name: "app.exe".into(),
        };
        let target = snapshot_target(selection.memory_trim_candidates().next().unwrap()).unwrap();
        assert_eq!(target.id, 10);
        assert_eq!(target.creation_time, 100);
        assert_eq!(target.executable_path, Path::new(path));

        selection.nested = false;
        selection.processes.extend([
            process(11, 200, path),
            ProcessInfo {
                name: "audiodg.exe".into(),
                ..process(12, 300, path)
            },
            ProcessInfo {
                creation_time: None,
                ..process(13, 400, path)
            },
        ]);
        let targets = selection
            .memory_trim_candidates()
            .map(|process| snapshot_target(process).unwrap().id)
            .collect::<Vec<_>>();
        assert_eq!(targets, [10, 11]);
    }

    #[test]
    fn stop_action_uses_tree_for_groups_or_children_but_not_reused_parent_ids() {
        let parent = process(10, 100, "parent.exe");
        let mut child = process(11, 200, "child.exe");
        child.parent_id = Some(parent.id);
        let mut selection = Selection {
            nested: false,
            processes: vec![parent.clone()],
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
        let settings = SettingsEditor::with_settings(Settings::default());
        let mut list = ProcessList::default();
        let view = list.view(&settings, PlanCatalog::Loaded(&[]));
        let mut tree = iced::advanced::widget::Tree::new(&view);
        let root_tag = tree.tag;
        let list_tag = tree.children[0].tag;
        drop(view);
        list.selected = Some(Selection {
            nested: false,
            processes: vec![process(10, 100, "app.exe")],
            name: "app.exe".into(),
        });
        for context in [Some(ContextAnchor::Pointer(Point::ORIGIN)), None] {
            list.context = context;
            let view = list.view(&settings, PlanCatalog::Loaded(&[]));
            tree.diff(&view);
            assert_eq!(tree.tag, root_tag);
            assert_eq!(tree.children[0].tag, list_tag);
        }
        list.selected = None;
        tree.diff(list.view(&settings, PlanCatalog::Loaded(&[])));
        assert_eq!(tree.tag, root_tag);
        assert_eq!(tree.children[0].tag, list_tag);
    }

    #[test]
    fn status_sort_matches_group_labels_and_updates_when_suspension_changes() {
        let mut list = ProcessList {
            processes: vec![
                process(10, 1, r"C:\Apps\a.exe"),
                process(11, 1, r"C:\Apps\a.exe"),
                process(12, 1, r"C:\Apps\b.exe"),
                process(13, 1, r"C:\Apps\c.exe"),
                process(14, 1, r"C:\Apps\d.exe"),
                process(15, 1, r"C:\Apps\e.exe"),
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
                private_commit_bytes: None,
                efficiency_mode: Some(true),
            },
        );
        list.sync_suspended_processes(&[(11, 1)]);
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
        for creation_time in [Some(2), None] {
            list.processes[1].creation_time = creation_time;
            list.rebuild();
            assert_eq!(ids(&list), vec![15, 12, 10, 13, 14]);
        }
        list.processes[1].creation_time = Some(1);
        list.rebuild();
        list.sync_suspended_processes(&[]);
        assert_eq!(ids(&list), vec![15, 12, 10, 13, 14]);
    }

    #[test]
    fn focusing_a_row_tracks_its_identity_without_opening_details() {
        let mut list = ProcessList {
            processes: vec![
                process(10, 100, "app.exe"),
                process(11, 200, "app.exe"),
                process(12, 300, "other.exe"),
            ],
            sort: Sort::Pid,
            ..ProcessList::default()
        };
        list.rebuild();
        let revision = list.rows_revision;
        assert!(list.focus_row(0, revision));
        assert_eq!(list.focused_index(), Some(0));
        assert!(list.selected.is_none());
        let focused = list.focused.as_ref().unwrap();
        assert!(!focused.nested);
        assert_eq!(focused.pid, 10);
        list.descending = true;
        list.rebuild();
        assert_eq!(list.focused_index(), Some(1));
        list.processes[0].creation_time = Some(400);
        list.rebuild();
        assert!(!list.focus_row(0, revision));
        assert_eq!(list.focused_index(), None);
        assert_eq!(list.focused.as_ref().unwrap().creation_time, Some(100));
    }

    #[test]
    fn process_table_accepts_arrows_and_context_shortcuts_when_focused() {
        use iced::advanced::{layout, widget::Tree, Layout, Shell};
        use iced::{keyboard, mouse, Event, Rectangle, Renderer, Size};

        let mut list = ProcessList {
            processes: vec![process(10, 100, "app.exe")],
            ..ProcessList::default()
        };
        list.rebuild();
        let settings = SettingsEditor::with_settings(Settings::default());
        let renderer = Renderer::new(design::typography::FONT, iced::Pixels(14.0));
        let viewport = Rectangle::with_size(Size::new(900.0, 620.0));
        let mut view = list.view(&settings, PlanCatalog::Loaded(&[]));
        let mut tree = Tree::new(&view);
        let node = view.as_widget_mut().layout(
            &mut tree,
            &renderer,
            &layout::Limits::new(Size::ZERO, viewport.size()),
        );
        for focused in [false, true] {
            if focused {
                view.as_widget_mut().operate(
                    &mut tree,
                    Layout::new(&node),
                    &renderer,
                    &mut iced::advanced::widget::operation::focusable::focus(PROCESS_TABLE.into()),
                );
            }
            for (key, modifiers, repeat, handled) in [
                (Named::ArrowDown, Modifiers::empty(), false, true),
                (Named::F10, Modifiers::SHIFT, false, true),
                (Named::ContextMenu, Modifiers::empty(), false, true),
                (Named::F10, Modifiers::SHIFT, true, true),
                (Named::F10, Modifiers::empty(), false, false),
                (Named::F10, Modifiers::SHIFT | Modifiers::CTRL, false, false),
            ] {
                let event = Event::Keyboard(keyboard::Event::KeyPressed {
                    key: keyboard::Key::Named(key),
                    modified_key: keyboard::Key::Named(key),
                    physical_key: keyboard::key::Physical::Unidentified(
                        keyboard::key::NativeCode::Unidentified,
                    ),
                    location: keyboard::Location::Standard,
                    modifiers,
                    text: None,
                    repeat,
                });
                let mut messages = Vec::new();
                let mut shell = Shell::new(&mut messages);
                view.as_widget_mut().update(
                    &mut tree,
                    &event,
                    Layout::new(&node),
                    mouse::Cursor::Unavailable,
                    &renderer,
                    &mut iced::advanced::clipboard::Null,
                    &mut shell,
                    &viewport,
                );
                assert_eq!(shell.is_event_captured(), focused && handled);
                if !focused || !handled || repeat {
                    assert!(messages.is_empty());
                } else if key == Named::ArrowDown {
                    assert!(
                        matches!(messages.as_slice(), [Message::MoveRowFocus(true, height)] if *height > 0.0)
                    );
                } else {
                    assert!(
                        matches!(messages.as_slice(), [Message::OpenContextFocused(bounds)] if bounds.height > 0.0 && bounds.y > 0.0)
                    );
                }
            }
        }
    }

    #[test]
    fn keyboard_context_requires_a_visible_and_still_identical_row() {
        let mut list = ProcessList {
            processes: (1..=10)
                .map(|id| process(id, u64::from(id), &format!("app{id}.exe")))
                .collect(),
            sort: Sort::Pid,
            ..ProcessList::default()
        };
        list.rebuild();
        let settings = SettingsEditor::with_settings(Settings::default());
        let bounds = Rectangle {
            x: 100.0,
            y: 150.0,
            width: 540.0,
            height: 2.0 * ROW_HEIGHT,
        };
        assert!(list.focus_row(8, list.rows_revision));
        let _ = list.open_context_focused(bounds, &settings);
        assert!(!list.context_open());
        assert_eq!(list.offset.get(), 7.0 * ROW_HEIGHT);

        // A real row departure must never retarget the old focus to a reused PID.
        list.processes[8].creation_time = Some(100);
        list.rebuild();
        let _ = list.open_context_focused(bounds, &settings);
        assert!(!list.context_open());

        assert!(list.focus_row(8, list.rows_revision));
        let _ = list.open_context_focused(bounds, &settings);
        assert!(list.context_open());
        assert_eq!(list.selected.as_ref().unwrap().processes[0].id, 9);
        assert_eq!(
            list.context,
            Some(ContextAnchor::FocusedRow(Point::new(
                bounds.x + design::space::MEDIUM as f32,
                bounds.y + bounds.height,
            )))
        );
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
        assert_eq!(
            list.column_width(Sort::WorkingSet),
            column_width(Sort::WorkingSet)
        );
    }

    #[test]
    fn last_visible_column_fills_spare_width_and_auto_size_fits_names() {
        let mut list = ProcessList::default();
        assert!(!list.columns[Sort::CpuTime as usize]);
        assert!(!list.columns[Sort::PrivateCommit as usize]);
        assert!(!list.gpu_memory_visible());
        assert!(!list.columns[Sort::GpuUsage as usize]);
        let widths = list.layout_widths(1200.0);
        let visible_width = |list: &ProcessList, widths: [f32; Sort::ALL.len()]| {
            widths
                .into_iter()
                .zip(list.columns)
                .filter_map(|(width, visible)| visible.then_some(width))
                .sum::<f32>()
        };
        assert_eq!(visible_width(&list, widths) + 5.0 * 8.0 + 32.0, 1200.0);
        list.columns[Sort::PrivateCommit as usize] = true;
        list.columns[Sort::User as usize] = false;
        let widths = list.layout_widths(1200.0);
        assert_eq!(visible_width(&list, widths) + 5.0 * 8.0 + 32.0, 1200.0);
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
        assert_eq!(list.column_widths, [None; Sort::ALL.len()]);
        list.processes.push(process(10, 100, r"C:\Fixture\app.exe"));
        list.rebuild();
        assert!(list.column_width(Sort::Status) >= column_width(Sort::Status));
        for col in Sort::ALL {
            assert_eq!(
                list.column_widths[col as usize],
                list.columns[col as usize].then(|| list.auto_width(col))
            );
        }
        list.columns[Sort::CpuTime as usize] = true;
        list.fit_visible_columns();
        assert_eq!(
            list.column_width(Sort::CpuTime),
            list.auto_width(Sort::CpuTime)
        );
        list.column_widths[Sort::Name as usize] = Some(400.0);
        list.rebuild();
        assert_eq!(list.column_width(Sort::Name), 400.0);
    }

    #[test]
    fn auto_fit_keeps_the_largest_padding_for_repeated_names() {
        let mut list = ProcessList {
            processes: vec![process(10, 100, "app.exe"), process(11, 200, "app.exe")],
            ..ProcessList::default()
        };
        for process in &mut list.processes {
            process.name = "a_long_executable_name_shared_by_the_group.exe".into();
        }
        list.rebuild();
        list.expanded.insert(list.rows[0].key.clone());
        list.rebuild();
        assert_eq!(list.rows.len(), 3);
        assert_eq!(
            list.auto_width(Sort::Name),
            (measure_column_text(&list.processes[0].name) + 76.0).ceil()
        );
    }

    #[test]
    fn memory_metrics_show_value_or_share_of_physical_memory() {
        assert_eq!(
            format_memory_usage(1_000_000, Some(4_000_000), false),
            "1.0 MB"
        );
        assert_eq!(format_memory_usage(1_500_000, None, false), "1.5 MB");
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
                nested: false,
                processes: vec![],
                name: "original.exe".into(),
            }),
            ..Default::default()
        };
        let complete = list.result_message(true, None);
        list.selected = None;
        assert!(matches!(
            complete(Ok(())),
            Message::Result { name, result: Ok(()), readback: Some(selection), detail_action: None }
            if name == "original.exe" && selection.name == "original.exe"
        ));
    }

    #[test]
    fn detail_feedback_is_scoped_to_the_active_attempt() {
        let mut list = ProcessList {
            selected: Some(Selection {
                nested: false,
                processes: vec![],
                name: "app.exe".into(),
            }),
            rulesets_error: Some("Rulesets failure".into()),
            ..Default::default()
        };
        let first = list.begin_detail_action().unwrap();
        list.record_action_result(Err("First failure".into()), Some(first));
        assert!(list.detail_feedback.is_some());
        let latest = list.begin_detail_action().unwrap();
        assert!(list.detail_feedback.is_none());
        assert_eq!(list.rulesets_error.as_deref(), Some("Rulesets failure"));
        list.record_action_result(Ok("Latest success".into()), Some(latest));
        list.record_action_result(Err("Delayed failure".into()), Some(first));
        assert_eq!(list.detail_feedback, Some(Ok("Latest success".into())));
        assert_eq!(
            list.action_feedback
                .as_ref()
                .map(|feedback| &feedback.result),
            Some(&Err("Delayed failure".into()))
        );

        list.context = Some(ContextAnchor::Pointer(Point::ORIGIN));
        assert_eq!(list.begin_detail_action(), None);
        list.record_action_result(Err("Context failure".into()), None);
        assert_eq!(
            list.action_feedback
                .as_ref()
                .map(|feedback| &feedback.result),
            Some(&Err("Context failure".into()))
        );
    }

    #[test]
    fn action_toast_expires_a_success_and_retains_an_error() {
        let mut list = ProcessList {
            refreshing: true,
            ..Default::default()
        };
        list.record_action_result(Ok("Action completed".into()), None);
        let expires_at = list.action_feedback.as_ref().unwrap().expires_at.unwrap();
        let before_expiry = expires_at - std::time::Duration::from_millis(1);
        assert_eq!(list.refresh_if_due(before_expiry).units(), 0);
        assert!(list.action_toast().is_some());
        assert_eq!(list.refresh_if_due(expires_at).units(), 0);
        assert!(list.action_toast().is_none());

        list.record_action_result(Err("Access denied".into()), None);
        assert_eq!(
            list.refresh_if_due(expires_at + ACTION_TOAST_DURATION)
                .units(),
            0
        );
        assert!(list.action_toast().is_some());
    }

    #[test]
    fn grouped_priority_requires_matching_verified_values() {
        let first = process(10, 100, r"C:\A\app.exe");
        let mut second = process(11, 200, r"C:\A\app.exe");
        let same = Value::Process(ProcessPrioritySetting::Normal);
        let targets = [snapshot_target(&first), snapshot_target(&second)];
        assert_eq!(shared_value(&targets, |_| Some(same)), Some(same));
        assert_eq!(
            shared_value(&targets, |target| (target.id == 10).then_some(same)),
            None
        );
        assert_eq!(
            shared_value(&targets, |target| Some(if target.id == 10 {
                same
            } else {
                Value::Process(ProcessPrioritySetting::Idle)
            })),
            None
        );
        second.creation_time = None;
        let targets = [snapshot_target(&first), snapshot_target(&second)];
        assert_eq!(shared_value(&targets, |_| Some(same)), None);
    }

    #[test]
    fn current_controls_ignore_older_queries_and_closed_selections() {
        let mut settings = Settings::default();
        settings.general.enabled = false;
        let mut editor = SettingsEditor::with_settings(settings.clone());
        let runtime = RuntimeHandle::start(&editor.runtime_settings_snapshot());
        let mut list = ProcessList {
            selected: Some(Selection {
                nested: false,
                processes: vec![process(10, 100, r"C:\A\app.exe")],
                name: "app.exe".into(),
            }),
            current_query_id: 2,
            ..Default::default()
        };
        let value = Value::Process(ProcessPrioritySetting::Idle);
        for message in [
            Message::Current(2, Ok((vec![value], Some(true)))),
            Message::Current(1, Ok((Vec::new(), Some(false)))),
            Message::Current(1, Err("obsolete query".into())),
            Message::CloseSelection,
            Message::Current(2, Err("closed selection".into())),
        ] {
            let _ = list.update(message, &mut editor, &runtime);
        }
        assert_eq!(list.current, [value]);
        assert_eq!(list.efficiency, Some(true));
        assert!(list.error.is_none());
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
        let offsets = (0..=11).map(|i| i as f32 * ROW_HEIGHT).collect::<Vec<_>>();
        let offset = std::cell::Cell::new(1500.0);
        assert_eq!(visible_range_offsets(&offsets, &offset, 450.0), 0..11);
        assert_eq!(offset.get(), 0.0);
    }
    #[test]
    fn grouping_uses_exact_path_and_expands_members() {
        let mut list = ProcessList {
            processes: vec![
                process(10, 1, r"C:\A\app.exe"),
                process(11, 2, "C:/A/app.exe"),
                process(12, 3, "C:\\A\\app.exe\u{3000}"),
                process(13, 4, r"C:\B\app.exe"),
            ],
            ..ProcessList::default()
        };
        list.rebuild();
        assert_eq!(
            list.offsets,
            vec![0.0, ROW_HEIGHT, 2.0 * ROW_HEIGHT, 3.0 * ROW_HEIGHT]
        );
        let revision = list.rows_revision;
        assert_eq!(list.rows.len(), 3);
        let targets = list.selection(&list.rows[0]).targets();
        assert_eq!(
            targets
                .into_iter()
                .map(|target| target.unwrap().id)
                .collect::<Vec<_>>(),
            [10, 11]
        );
        list.expanded.insert(list.rows[0].key.clone());
        list.rebuild();
        assert_eq!(list.rows.len(), 5);
        assert!(list.rows[1].nested);
        assert_ne!(list.rows_revision, revision);
        assert_eq!(list.offsets.last(), Some(&(5.0 * ROW_HEIGHT)));
        list.processes.clear();
        list.rebuild();
        assert_eq!(list.offsets, vec![0.0]);
    }
    #[test]
    fn grouping_keeps_distinct_native_paths_with_the_same_display_text() {
        use std::os::windows::ffi::OsStringExt;

        let path = "C:\\A\\\u{fffd}\\app.exe";
        let mut raw = process(11, 2, path);
        let units = path
            .encode_utf16()
            .map(|unit| if unit == 0xfffd { 0xd800 } else { unit })
            .collect::<Vec<_>>();
        raw.image_path = Some(OsString::from_wide(&units).into());
        let mut list = ProcessList {
            processes: vec![raw, process(10, 1, path)],
            ..ProcessList::default()
        };
        list.rebuild();
        assert_eq!(list.rows.len(), 2);
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
        assert!(visible_range_offsets(&list.offsets, &list.offset, 600.0).len() <= 42);
        list.expanded.remove(&key);
        list.rebuild();
        assert_eq!(list.rows.len(), 1);
        assert_eq!(list.offsets, vec![0.0, ROW_HEIGHT]);
    }
    #[test]
    fn copy_path_preserves_the_group_path_and_requires_exact_text() {
        use std::os::windows::ffi::OsStringExt;

        let path = concat!(r"\\server\share\Applications\測試.exe", " ");
        let mut list = ProcessList {
            processes: vec![process(10, 1, path), process(11, 2, path)],
            ..ProcessList::default()
        };
        list.rebuild();
        let mut selected = list.selection(&list.rows[0]);
        assert_eq!(selected.processes.len(), 2);
        assert_eq!(selected.path_text(), Some(path));

        let mut settings = Settings::default();
        settings.general.enabled = false;
        let mut editor = SettingsEditor::with_settings(settings);
        let runtime = RuntimeHandle::start(&editor.runtime_settings_snapshot());
        list.selected = Some(selected.clone());
        list.context = Some(ContextAnchor::Pointer(Point::ORIGIN));
        let clipboard = list.update(
            Message::Menu(Box::new(Message::CopyExecutablePath)),
            &mut editor,
            &runtime,
        );
        assert_eq!(clipboard.units(), 1);
        assert!(list.selected.is_none());
        assert!(!list.context_open());

        selected.processes[0].image_path = Some(PathBuf::from(OsString::from_wide(&[0xD800])));
        assert!(selected.path_text().is_none());
        selected.processes[0].image_path = None;
        assert!(selected.path_text().is_none());
        list.selected = Some(selected);
        assert_eq!(
            list.update(Message::CopyExecutablePath, &mut editor, &runtime)
                .units(),
            0
        );
    }

    #[test]
    fn selected_targets_retain_creation_time_across_refresh() {
        let old = process(10, 100, r"C:\A\app.exe");
        let selected = Selection {
            nested: false,
            processes: vec![old],
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
        assert!(!live_action_available(&p));
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
