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
const ROW_HEIGHT: f32 = 52.0;
#[derive(Debug, Clone)]
pub(super) struct Population {
    processes: Vec<ProcessInfo>,
    samples: BTreeMap<u32, ProcessResourceSample>,
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
#[derive(Debug, Clone)]
pub(super) enum Message {
    Refresh,
    Loaded(Result<Population, String>),
    Search(String),
    Scrolled(f32),
    HideInaccessible(bool),
    Group(bool),
    Expand(String),
    Sort(Sort),
    Column(Sort, bool),
    ToggleColumns,
    Select(usize, u64),
    Current(Result<(Selection, Vec<Value>, Option<bool>), String>),
    CloseSelection,
    Action(Action),
    Stop(bool),
    ConfirmStop,
    CancelStop,
    TreeReady(Result<Vec<ProcessActionTarget>, String>),
    Result(String, Result<(), String>),
    OpenLocation,
    Details(details::Message),
}

pub(super) struct ProcessList {
    processes: Vec<ProcessInfo>,
    rows: Vec<Entry>,
    offsets: Vec<f32>,
    rows_revision: u64,
    samples: BTreeMap<u32, ProcessResourceSample>,
    cpu: HashMap<u32, f32>,
    icons: HashMap<PathBuf, Option<Arc<image::Handle>>>,
    search: String,
    offset: std::cell::Cell<f32>,
    refreshing: bool,
    error: Option<String>,
    hide_inaccessible: bool,
    grouped: bool,
    expanded: HashSet<String>,
    sort: Sort,
    descending: bool,
    columns: [bool; 6],
    show_columns: bool,
    selected: Option<Selection>,
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
            cpu: HashMap::new(),
            icons: HashMap::new(),
            search: String::new(),
            offset: std::cell::Cell::new(0.0),
            refreshing: false,
            error: None,
            hide_inaccessible: true,
            grouped: true,
            expanded: HashSet::new(),
            sort: Sort::Name,
            descending: false,
            columns: [true; 6],
            show_columns: false,
            selected: None,
            current: Vec::new(),
            efficiency: None,
            stopping: None,
        }
    }
}
impl ProcessList {
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
        self.selected = None;
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
            Message::HideInaccessible(v) => {
                self.hide_inaccessible = v;
                self.rebuild();
            }
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
            Message::ToggleColumns => self.show_columns = !self.show_columns,
            Message::Column(column, v) => self.columns[column as usize] = v,
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
                self.current.clear();
                self.efficiency = None;
                self.selected = Some(selection.clone());
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
            Message::CloseSelection => self.selected = None,
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
                if let Some(selection) = &self.selected {
                    self.stopping = Some((selection.clone(), tree));
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
                Sort::Status => protected(&self.processes[a.indices[0]])
                    .cmp(&protected(&self.processes[b.indices[0]])),
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
    pub(super) fn view<'a>(
        &'a self,
        settings: &'a Settings,
        status: &'a RuntimeStatusSnapshot,
        plans: &'a [crate::power::PowerPlan],
    ) -> Element<'a, Message> {
        let mut controls = row![
            text_input(&t!("process_list.search_placeholder"), &self.search)
                .on_input(Message::Search)
                .width(280),
            button(text(t!("settings.refresh").to_string()))
                .on_press_maybe((!self.refreshing).then_some(Message::Refresh))
                .style(super::widgets::quiet),
            checkbox(self.hide_inaccessible)
                .label(t!("process_list.hide_inaccessible_processes").to_string())
                .on_toggle(Message::HideInaccessible),
            checkbox(self.grouped)
                .label(t!("process_list.app_name").to_string())
                .on_toggle(Message::Group)
        ]
        .spacing(design::space::SMALL);
        controls = controls.push(text(
            t!("process_list.count", count = self.processes.len()).to_string(),
        ));
        let mut columns = row![].spacing(design::space::SMALL);
        for col in Sort::ALL.into_iter().skip(1) {
            columns = columns.push(
                checkbox(self.columns[col as usize])
                    .label(col.label())
                    .on_toggle(move |v| Message::Column(col, v)),
            );
        }
        controls = controls.push(
            button(super::navigation::glyph("icons/settings.svg"))
                .style(super::widgets::quiet)
                .on_press(Message::ToggleColumns),
        );
        let mut body = column![controls.wrap()]
            .spacing(design::space::SMALL)
            .height(Fill);
        if self.show_columns {
            body = body.push(columns);
        }
        if let Some(e) = &self.error {
            body = body.push(text(e));
        }
        body = body.push(responsive(move |size| {
            let name_width = (size.width
                - Sort::ALL
                    .into_iter()
                    .skip(1)
                    .filter(|col| self.columns[*col as usize])
                    .map(|col| column_width(col) + 8.0)
                    .sum::<f32>()
                - 24.0)
                .max(230.0);
            let mut header = row![button(text(Sort::Name.label()))
                .on_press(Message::Sort(Sort::Name))
                .style(super::widgets::quiet)
                .width(name_width)]
            .spacing(design::space::SMALL);
            for col in Sort::ALL.into_iter().skip(1) {
                if self.columns[col as usize] {
                    header = header.push(
                        button(text(col.label()))
                            .on_press(Message::Sort(col))
                            .style(super::widgets::quiet)
                            .width(column_width(col)),
                    );
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
                        container(super::navigation::glyph(
                            if self.expanded.contains(&entry.key) {
                                "icons/chevron-down.svg"
                            } else {
                                "icons/chevron-right.svg"
                            },
                        ))
                        .width(20)
                        .height(20)
                        .center_y(20),
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
                let name: Element<'_, Message> = if entry.indices.len() > 1 {
                    button(name)
                        .padding(0)
                        .width(Fill)
                        .style(super::widgets::quiet)
                        .on_press(Message::Expand(entry.key.clone()))
                        .into()
                } else {
                    name.into()
                };
                let mut cells = row![container(name).width(name_width).clip(true)]
                    .spacing(design::space::SMALL)
                    .align_y(iced::Center);
                for col in Sort::ALL.into_iter().skip(1) {
                    if !self.columns[col as usize] {
                        continue;
                    }
                    let value = match col {
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
                                format!("{:.1} MiB", self.memory_total(entry) as f64 / 1048576.0)
                            } else {
                                t!("common.unknown").to_string()
                            }
                        }
                        Sort::User => self.user(entry),
                        Sort::Status => {
                            let key =
                                if entry.indices.iter().all(|i| protected(&self.processes[*i])) {
                                    "process_list.status_protected_system_process"
                                } else if entry
                                    .indices
                                    .iter()
                                    .all(|i| inaccessible(&self.processes[*i]))
                                {
                                    "process_list.status_access_denied"
                                } else if entry.indices.iter().any(|i| {
                                    status
                                        .feature_status
                                        .app_suspension
                                        .suspended_process_ids
                                        .contains(&self.processes[*i].id)
                                }) {
                                    "process_list.status_suspended"
                                } else if entry.indices.iter().any(|i| {
                                    self.samples
                                        .get(&self.processes[*i].id)
                                        .is_some_and(|s| s.efficiency_mode == Some(true))
                                }) {
                                    "process_list.status_efficiency_mode"
                                } else {
                                    "process_list.status_active"
                                };
                            t!(key).to_string()
                        }
                    };
                    cells = cells.push(
                        container(
                            text(value)
                                .wrapping(iced::widget::text::Wrapping::None)
                                .style(if col == Sort::Status {
                                    text::primary
                                } else {
                                    text::default
                                }),
                        )
                        .width(column_width(col))
                        .clip(true),
                    );
                }
                visible_rows.push((
                    (p.id, p.creation_time, entry.nested),
                    mouse_area(
                        container(column![
                            container(cells)
                                .padding([
                                    design::space::MEDIUM as u16,
                                    design::space::SMALL as u16
                                ])
                                .height(ROW_HEIGHT - 1.0),
                            iced::widget::rule::horizontal(1)
                        ])
                        .height(height)
                        .clip(true),
                    )
                    .on_press(Message::Select(row_index, self.rows_revision))
                    .on_right_press(Message::Select(row_index, self.rows_revision))
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
        if let Some(selection) = &self.selected {
            let mut pane = column![
                row![
                    text(&selection.name).size(design::typography::SUBTITLE),
                    button(text(t!("common.done").to_string())).on_press(Message::CloseSelection)
                ]
                .spacing(design::space::SMALL),
                text(&selection.path)
            ]
            .spacing(design::space::COMPACT);
            let eligible = selection.processes.iter().any(|p| !inaccessible(p));
            let suspendable = selection.processes.iter().any(|p| {
                !inaccessible(p)
                    && p.session_id.is_some_and(|s| s != 0)
                    && p.is_service_account == Some(false)
                    && !crate::app_suspension::is_builtin_excluded(&p.name)
            });
            pane = pane.push(
                row![
                    button(text(t!("process_list.stop_process").to_string()))
                        .on_press_maybe(eligible.then_some(Message::Stop(false))),
                    button(text(t!("process_list.stop_process_tree").to_string()))
                        .on_press_maybe(eligible.then_some(Message::Stop(true)))
                ]
                .spacing(design::space::TIGHT),
            );
            pane = pane.push(
                row![
                    button(text(t!("process_list.suspend_process").to_string())).on_press_maybe(
                        suspendable.then_some(Message::Action(Action::Suspend(true)))
                    ),
                    button(text(t!("process_list.resume_process").to_string())).on_press_maybe(
                        suspendable.then_some(Message::Action(Action::Suspend(false)))
                    )
                ]
                .spacing(design::space::TIGHT),
            );
            pane = pane.push(
                row![
                    text(format!(
                        "{}: {}",
                        t!("process_list.efficiency_mode"),
                        t!(match self.efficiency {
                            Some(true) => "common.on",
                            Some(false) => "common.off",
                            None => "common.unknown",
                        })
                    )),
                    button(text(t!("common.on").to_string())).on_press_maybe(
                        eligible.then_some(Message::Action(Action::Efficiency(true)))
                    ),
                    button(text(t!("common.off").to_string())).on_press_maybe(
                        eligible.then_some(Message::Action(Action::Efficiency(false)))
                    )
                ]
                .spacing(design::space::TIGHT),
            );
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
                    .placeholder(t!("settings.apply").to_string())
                    .into()
                } else {
                    text(
                        current
                            .map(|v| v.to_string())
                            .unwrap_or_else(|| t!("common.unknown").to_string()),
                    )
                    .into()
                };
                pane = pane.push(
                    row![text(t!(&key).to_string()).width(Fill), control]
                        .spacing(design::space::SMALL),
                );
            }
            pane = pane.push(
                button(text(t!("process_list.open_process_location").to_string()))
                    .on_press(Message::OpenLocation),
            );
            if eligible && !selection.path.is_empty() {
                pane = pane
                    .push(details::view(settings, &selection.path, plans).map(Message::Details));
            }
            if let Some((stopping, tree)) = &self.stopping {
                pane = pane
                    .push(text(
                        t!(
                            if *tree {
                                "process_list.stop_tree_confirm"
                            } else {
                                "process_list.stop_confirm"
                            },
                            name = &stopping.name
                        )
                        .to_string(),
                    ))
                    .push(
                        row![
                            button(text(t!("process_list.stop_process").to_string()))
                                .on_press(Message::ConfirmStop),
                            button(text(t!("common.cancel").to_string()))
                                .on_press(Message::CancelStop)
                        ]
                        .spacing(design::space::SMALL),
                    );
            }
            return row![body.width(Fill), scrollable(pane).width(380)]
                .spacing(design::space::MEDIUM)
                .into();
        }
        body.into()
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
