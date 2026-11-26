use std::{path::Path, sync::Arc};

use super::{
    app_picker, design,
    process_rules::{exact_path_input, picked_path_input, MatchBy},
    widgets,
};
use crate::config::{
    ExecutableNamePattern, ResourceThresholdsAction, ResourceThresholdsMemoryUnit,
    ResourceThresholdsMetric, ResourceThresholdsResource, ResourceThresholdsRule,
    ResourceThresholdsSelector, ResourceThresholdsSettings, RESOURCE_THRESHOLDS_DURATION_RANGE,
};
use crate::foreground::{
    process_list::{normalize_path_slashes, same_executable_name},
    same_executable_path,
};
use iced::{
    widget::{column, row, text, Column},
    Element, Fill,
};
use rust_i18n::t;

mod preview;

impl std::fmt::Display for ResourceThresholdsAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&t!(match self {
            Self::None => "resource_thresholds.no_action",
            Self::Alert => "resource_thresholds.alert",
            Self::TrimMemory => "resource_thresholds.trim_memory",
            Self::Terminate => "resource_thresholds.terminate",
            Self::Restart => "resource_thresholds.restart",
        }))
    }
}

impl std::fmt::Display for ResourceThresholdsMemoryUnit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&t!(self.metric().label_key()))
    }
}

#[derive(Default)]
pub(super) struct Editor {
    match_by: MatchBy,
    settings_expanded: bool,
    path: String,
    name: String,
    duration: Option<String>,
    draft: Option<Draft>,
}

struct Draft {
    index: Option<usize>,
    rule: ResourceThresholdsRule,
    tab: DraftTab,
    monitors: [ThresholdInputs; 2],
    memory_unit: ResourceThresholdsMemoryUnit,
    alternate_memory: ThresholdInputs,
    monitor_settings_expanded: [bool; 2],
    preview: PreviewMatches,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DraftTab {
    Monitor(ResourceThresholdsResource),
    PreviewMatches,
}

impl DraftTab {
    fn content_key(self) -> u64 {
        match self {
            Self::Monitor(resource) => resource as u64,
            Self::PreviewMatches => 2,
        }
    }
}

#[derive(Default)]
struct PreviewMatches {
    pending: Option<Arc<()>>,
    result: Option<preview::MatchResult>,
}

#[derive(Default)]
struct ThresholdInputs {
    warning: String,
    critical: String,
}

impl Draft {
    fn new(index: Option<usize>, rule: ResourceThresholdsRule) -> Self {
        Self {
            index,
            tab: DraftTab::Monitor(if rule.cpu.enabled {
                ResourceThresholdsResource::Cpu
            } else {
                ResourceThresholdsResource::Memory
            }),
            monitors: ResourceThresholdsResource::ALL.map(|resource| {
                let monitor = rule.monitor(resource);
                ThresholdInputs {
                    warning: monitor.warning.threshold.to_string(),
                    critical: monitor.critical.threshold.to_string(),
                }
            }),
            memory_unit: rule.memory_unit,
            alternate_memory: ThresholdInputs::default(),
            monitor_settings_expanded: [rule.cpu.enabled, rule.memory.enabled],
            rule,
            preview: PreviewMatches::default(),
        }
    }

    fn select_memory_unit(&mut self, unit: ResourceThresholdsMemoryUnit) {
        if unit != self.memory_unit {
            std::mem::swap(
                &mut self.monitors[ResourceThresholdsResource::Memory as usize],
                &mut self.alternate_memory,
            );
            self.memory_unit = unit;
        }
    }

    fn metric(&self, resource: ResourceThresholdsResource) -> ResourceThresholdsMetric {
        match resource {
            ResourceThresholdsResource::Cpu => ResourceThresholdsMetric::CpuPercent,
            ResourceThresholdsResource::Memory => self.memory_unit.metric(),
        }
    }

    fn validated_rule(&self) -> Option<ResourceThresholdsRule> {
        let mut rule = self.rule.clone();
        let memory_unit_changed = self.memory_unit != rule.memory_unit;
        rule.memory_unit = self.memory_unit;
        for (resource, raw) in ResourceThresholdsResource::ALL
            .into_iter()
            .zip(&self.monitors)
        {
            let metric = rule.metric(resource);
            let monitor = rule.monitor_mut(resource);
            let needs_thresholds = monitor.enabled
                || (resource == ResourceThresholdsResource::Memory && memory_unit_changed);
            if let (Ok(warning), Ok(critical)) = (raw.warning.parse(), raw.critical.parse()) {
                let mut edited = monitor.clone();
                edited.warning.threshold = warning;
                edited.critical.threshold = critical;
                if edited.thresholds_valid(metric) {
                    *monitor = edited;
                    continue;
                }
            }
            if needs_thresholds {
                return None;
            }
        }
        rule.validate().ok()?;
        Some(rule)
    }
}

#[derive(Debug, Clone)]
pub(super) enum Message {
    Enabled(bool),
    ToggleSettings,
    MatchBy(MatchBy),
    Path(String),
    Name(String),
    Browse,
    Add,
    Remove(usize),
    Edit(usize),
    SaveRule,
    CancelRule,
    RuleEnabled(usize, bool),
    RuleNotify(usize, bool),
    Tab(ResourceThresholdsResource),
    OpenPreviewMatches,
    PreviewMatchesLoaded(Arc<()>, preview::MatchResult),
    MonitorEnabled(ResourceThresholdsResource, bool),
    ToggleMonitorSettings(ResourceThresholdsResource),
    MemoryUnit(ResourceThresholdsMemoryUnit),
    Duration(String),
    Threshold(ResourceThresholdsResource, bool, String),
    Notify(bool),
    Action(ResourceThresholdsResource, bool, ResourceThresholdsAction),
}

impl Editor {
    fn can_add_path(settings: &ResourceThresholdsSettings, path: &str) -> bool {
        super::process_rules::can_add_exact_process_candidate(
            path,
            |path| {
                settings.rules.iter().any(|rule| {
                    matches!(
                        &rule.selector,
                        ResourceThresholdsSelector::ExactPath { executable_path }
                            if same_executable_path(Path::new(executable_path), Path::new(path))
                    )
                })
            },
            |name| name.eq_ignore_ascii_case("winderust.exe"),
        )
    }

    fn new_name_selector(
        &self,
        settings: &ResourceThresholdsSettings,
    ) -> Option<ResourceThresholdsSelector> {
        let name = self.name.trim();
        if name.eq_ignore_ascii_case("winderust.exe")
            || settings.rules.iter().any(|rule| {
                rule.selector.exact_path().is_none()
                    && same_executable_name(rule.selector.value(), name)
            })
        {
            return None;
        }
        let selector = if name.contains(['*', '?']) {
            ResourceThresholdsSelector::NamePattern {
                executable_name_pattern: ExecutableNamePattern::try_from(name.to_owned()).ok()?,
            }
        } else {
            ResourceThresholdsSelector::ExactName {
                executable_name: name.into(),
            }
        };
        selector.validate().ok()?;
        Some(selector)
    }

    fn new_rule(&self, settings: &ResourceThresholdsSettings) -> Option<ResourceThresholdsRule> {
        match self.match_by {
            MatchBy::ExecutablePath => {
                let path = exact_path_input(&self.path);
                Self::can_add_path(settings, path)
                    .then(|| ResourceThresholdsRule::new(normalize_path_slashes(path).into_owned()))
            }
            MatchBy::ExecutableName => self
                .new_name_selector(settings)
                .map(ResourceThresholdsRule::with_selector),
        }
    }

    pub(super) fn has_pending_editor(&self) -> bool {
        self.draft.is_some()
    }

    pub(super) fn has_invalid_inputs(&self) -> bool {
        self.duration
            .as_deref()
            .is_some_and(|value| parse_duration(value).is_none())
    }

    pub(super) fn select_tab(&mut self, resource: ResourceThresholdsResource) {
        if let Some(draft) = &mut self.draft {
            draft.tab = DraftTab::Monitor(resource);
        }
    }

    pub(super) fn toggle_monitor_settings(&mut self, resource: ResourceThresholdsResource) {
        if let Some(draft) = &mut self.draft {
            let expanded = &mut draft.monitor_settings_expanded[resource as usize];
            *expanded = !*expanded;
        }
    }

    pub(super) fn open_preview_matches(&mut self) -> iced::Task<Message> {
        if let Some(draft) = &mut self.draft {
            draft.tab = DraftTab::PreviewMatches;
        }
        self.poll_preview_matches()
    }

    pub(super) fn preview_matches_open(&self) -> bool {
        self.draft
            .as_ref()
            .is_some_and(|draft| draft.tab == DraftTab::PreviewMatches)
    }

    pub(super) fn poll_preview_matches(&mut self) -> iced::Task<Message> {
        let Some(draft) = self.draft.as_mut().filter(|draft| {
            draft.tab == DraftTab::PreviewMatches && draft.preview.pending.is_none()
        }) else {
            return iced::Task::none();
        };
        let request = Arc::new(());
        draft.preview.pending = Some(request.clone());
        let selector = draft.rule.selector.clone();
        let previous = draft
            .preview
            .result
            .as_ref()
            .and_then(|result| result.as_ref().ok())
            .cloned();
        super::tasks::run(move || preview::load(&selector, previous.as_deref().unwrap_or(&[]))).map(
            move |result| {
                Message::PreviewMatchesLoaded(request.clone(), result.and_then(|result| result))
            },
        )
    }

    pub(super) fn complete_preview_matches(
        &mut self,
        request: Arc<()>,
        result: preview::MatchResult,
    ) {
        let Some(draft) = &mut self.draft else {
            return;
        };
        if draft
            .preview
            .pending
            .as_ref()
            .is_some_and(|active| Arc::ptr_eq(active, &request))
        {
            draft.preview.pending = None;
            draft.preview.result = Some(result);
        }
    }

    #[cfg(feature = "render-smoke")]
    pub(super) fn set_smoke_preview_matches(&mut self) {
        let draft = self
            .draft
            .as_mut()
            .expect("preview fixture has an open rule");
        draft.tab = DraftTab::PreviewMatches;
        draft.preview.result = Some(Ok([
            (41, r"C:\Apps\worker.exe"),
            (42, r"C:\Apps\worker.exe"),
            (81, r"D:\Tools\worker.exe"),
        ]
        .into_iter()
        .map(|(pid, path)| preview::MatchRow {
            pid,
            name: "worker.exe".into(),
            path: path.into(),
            icon: None,
        })
        .collect()));
    }

    pub(super) fn toggle_settings(&mut self) {
        self.settings_expanded = !self.settings_expanded;
    }

    pub(super) fn update(&mut self, settings: &mut ResourceThresholdsSettings, message: Message) {
        match message {
            Message::ToggleSettings => self.toggle_settings(),
            Message::Enabled(value) => settings.enabled = value,
            Message::MatchBy(value) => self.match_by = value,
            Message::Path(value) => self.path = value,
            Message::Name(value) => self.name = value,
            Message::Add => {
                if let Some(rule) = self.new_rule(settings) {
                    self.draft = Some(Draft::new(None, rule));
                }
            }
            Message::Edit(index) => {
                self.draft = settings
                    .rules
                    .get(index)
                    .cloned()
                    .map(|rule| Draft::new(Some(index), rule));
            }
            Message::SaveRule => {
                if let Some(draft) = &self.draft {
                    if let Some(rule) = draft.validated_rule() {
                        if let Some(index) = draft.index {
                            if let Some(target) = settings.rules.get_mut(index) {
                                *target = rule;
                            }
                        } else {
                            if rule.selector.exact_path().is_some() {
                                self.path.clear();
                            } else {
                                self.name.clear();
                            }
                            settings.rules.push(rule);
                        }
                        self.draft = None;
                    }
                }
            }
            Message::CancelRule => self.draft = None,
            Message::Tab(resource) => self.select_tab(resource),
            Message::ToggleMonitorSettings(resource) => self.toggle_monitor_settings(resource),
            Message::Remove(index) if index < settings.rules.len() => {
                settings.rules.remove(index);
                self.draft = None;
            }
            Message::RuleEnabled(index, value) => {
                if let Some(rule) = settings.rules.get_mut(index) {
                    rule.enabled = value;
                }
            }
            Message::RuleNotify(index, value) => {
                if let Some(rule) = settings.rules.get_mut(index) {
                    rule.notify = value;
                }
            }
            Message::MonitorEnabled(resource, enabled) => {
                if let Some(draft) = &mut self.draft {
                    let monitor = draft.rule.monitor_mut(resource);
                    if enabled && !monitor.enabled {
                        draft.monitor_settings_expanded[resource as usize] = true;
                    }
                    monitor.enabled = enabled;
                }
            }
            Message::MemoryUnit(unit) => {
                if let Some(draft) = &mut self.draft {
                    draft.select_memory_unit(unit);
                }
            }
            Message::Duration(value) => {
                if let Some(seconds) = parse_duration(&value) {
                    settings.sustained_seconds = seconds;
                }
                self.duration = Some(value);
            }
            Message::Threshold(resource, critical, value) => {
                if let Some(draft) = &mut self.draft {
                    if critical {
                        draft.monitors[resource as usize].critical = value;
                    } else {
                        draft.monitors[resource as usize].warning = value;
                    }
                }
            }
            Message::Notify(value) => settings.notify = value,
            Message::Action(resource, critical, value) => {
                if let Some(draft) = self
                    .draft
                    .as_mut()
                    .filter(|draft| draft.rule.allows_action(resource, value))
                {
                    response(draft.rule.monitor_mut(resource), critical).action = value;
                }
            }
            Message::Remove(_)
            | Message::Browse
            | Message::OpenPreviewMatches
            | Message::PreviewMatchesLoaded(..) => {}
        }
    }

    pub(super) fn view<'a>(
        &'a self,
        settings: &'a ResourceThresholdsSettings,
        candidates: &'a [app_picker::Candidate],
    ) -> Element<'a, Message> {
        let rules = settings
            .rules
            .iter()
            .enumerate()
            .map(|(index, rule)| {
                let value = rule.selector.value();
                let name_match_key = match rule.selector {
                    ResourceThresholdsSelector::ExactPath { .. } => None,
                    ResourceThresholdsSelector::ExactName { .. } => Some("common.executable_name"),
                    ResourceThresholdsSelector::NamePattern { .. } => {
                        Some("common.executable_name_pattern")
                    }
                };
                let (app, target) = widgets::process_rule_target(value, name_match_key, candidates);
                let label = t!("common.rule", number = index + 1);
                (
                    widgets::stable_key(&format!("{index}:{value}")),
                    column![
                        widgets::process_rule_header_with_target_and_notify(
                            column![
                                text(label.into_owned())
                                    .size(design::typography::CAPTION)
                                    .style(text::secondary),
                                app,
                            ]
                            .spacing(design::space::TINY)
                            .into(),
                            target,
                            widgets::checkbox(rule.enabled)
                                .on_toggle(move |value| Message::RuleEnabled(index, value))
                                .into(),
                            Some(widgets::rule_notify_checkbox(rule.notify, move |value| {
                                Message::RuleNotify(index, value)
                            })),
                            None,
                            ResourceThresholdsResource::ALL
                                .into_iter()
                                .map(|resource| {
                                    let monitor = rule.monitor(resource);
                                    text(if monitor.enabled {
                                        let warning = monitor.warning.threshold;
                                        let critical = monitor.critical.threshold;
                                        let unit = rule.metric(resource).unit();
                                        let gap = if unit == "%" { "" } else { " " };
                                        format!("{warning}{gap}{unit} / {critical}{gap}{unit}")
                                    } else {
                                        t!("common.off").into_owned()
                                    })
                                    .size(design::typography::SECONDARY)
                                    .into()
                                })
                                .collect(),
                            widgets::rule_actions(
                                Some(Message::Edit(index)),
                                Some(Message::Remove(index)),
                            ),
                        ),
                        widgets::settings_divider(),
                    ]
                    .into(),
                )
            })
            .collect();
        let mut duration = column![widgets::numeric_setting(
            "resource_thresholds.duration",
            "s",
            move || {
                widgets::stepper(
                    self.duration
                        .as_deref()
                        .unwrap_or(&settings.sustained_seconds.to_string()),
                    RESOURCE_THRESHOLDS_DURATION_RANGE,
                    1,
                    Some(Message::Duration),
                )
            }
        )]
        .spacing(design::space::SMALL);
        if self.has_invalid_inputs() {
            duration = duration.push(
                text(t!("resource_thresholds.invalid_duration").to_string()).style(text::danger),
            );
        }
        super::scrolling::scrollable(
            column![
                widgets::setting_group(
                    "resource_thresholds.enable".to_string(),
                    self.settings_expanded,
                    Message::ToggleSettings,
                    widgets::switch(settings.enabled, Some(Message::Enabled)),
                    widgets::settings_rows([
                        duration.into(),
                        widgets::setting_row(
                            "resource_thresholds.notify",
                            widgets::switch(settings.notify, Some(Message::Notify)),
                        )
                        .into(),
                    ]),
                ),
                widgets::setting_title("common.rules"),
                self.add_rule_card(settings, candidates),
                widgets::process_rules_table_with_notify(
                    [
                        "resource_thresholds.cpu_tab",
                        "resource_thresholds.memory_tab"
                    ]
                    .map(|key| t!(key).to_string()),
                    rules,
                    false,
                ),
            ]
            .spacing(widgets::CARD_GAP),
        )
        .height(Fill)
        .into()
    }

    fn add_rule_card<'a>(
        &self,
        settings: &ResourceThresholdsSettings,
        candidates: &'a [app_picker::Candidate],
    ) -> Element<'a, Message> {
        let target = match self.match_by {
            MatchBy::ExecutablePath => app_picker::view_with_path_selection(
                &self.path,
                candidates,
                true,
                (Message::Path, |path| Message::Path(picked_path_input(path))),
                Message::Browse,
                Self::can_add_path(settings, exact_path_input(&self.path)).then_some(Message::Add),
                |path| Self::can_add_path(settings, path).then_some(true),
            ),
            MatchBy::ExecutableName => app_picker::executable_name_input(
                &t!("resource_thresholds.name_placeholder"),
                &self.name,
                Message::Name,
                self.new_name_selector(settings).map(|_| Message::Add),
            ),
        };
        widgets::settings_card(widgets::settings_rows([
            widgets::setting_row(
                "common.match_by",
                widgets::pick_list(
                    vec![MatchBy::ExecutablePath, MatchBy::ExecutableName],
                    Some(self.match_by),
                    Message::MatchBy,
                )
                .width(design::SELECT_WIDTH),
            )
            .into(),
            target,
        ]))
        .into()
    }

    fn monitor_details(draft: &Draft, resource: ResourceThresholdsResource) -> Column<'_, Message> {
        let rule = &draft.rule;
        let monitor = rule.monitor(resource);
        let raw = &draft.monitors[resource as usize];
        let unit = draft.metric(resource).unit();
        let memory_unit = (resource == ResourceThresholdsResource::Memory).then(|| {
            widgets::setting_row(
                "resource_thresholds.memory_unit",
                widgets::pick_list(
                    ResourceThresholdsMemoryUnit::ALL,
                    Some(draft.memory_unit),
                    Message::MemoryUnit,
                )
                .width(180),
            )
            .into()
        });
        let rows = [
            (false, &monitor.warning, &raw.warning),
            (true, &monitor.critical, &raw.critical),
        ]
        .map(|(critical, response, threshold)| {
            let label = match (resource, critical) {
                (ResourceThresholdsResource::Cpu, false) => "resource_thresholds.cpu_warning",
                (ResourceThresholdsResource::Cpu, true) => "resource_thresholds.cpu_critical",
                (ResourceThresholdsResource::Memory, false) => "resource_thresholds.memory_warning",
                (ResourceThresholdsResource::Memory, true) => "resource_thresholds.memory_critical",
            };
            let actions = [
                ResourceThresholdsAction::None,
                ResourceThresholdsAction::Alert,
                ResourceThresholdsAction::TrimMemory,
                ResourceThresholdsAction::Restart,
                ResourceThresholdsAction::Terminate,
            ]
            .into_iter()
            .filter(|action| rule.allows_action(resource, *action))
            .collect::<Vec<_>>();
            widgets::settings_rows([
                widgets::numeric_setting(label, unit, move || {
                    widgets::text_input("", threshold)
                        .align_x(iced::alignment::Horizontal::Center)
                        .on_input(move |value| Message::Threshold(resource, critical, value))
                        .on_submit(Message::SaveRule)
                        .width(design::STANDALONE_NUMERIC_WIDTH)
                        .into()
                }),
                widgets::setting_row(
                    if critical {
                        "resource_thresholds.critical_action"
                    } else {
                        "resource_thresholds.warning_action"
                    },
                    widgets::pick_list(actions, Some(response.action), move |value| {
                        Message::Action(resource, critical, value)
                    })
                    .width(180),
                )
                .into(),
            ])
            .into()
        });
        column![widgets::setting_group(
            match resource {
                ResourceThresholdsResource::Cpu => "resource_thresholds.monitor_cpu",
                ResourceThresholdsResource::Memory => "resource_thresholds.monitor_memory",
            }
            .into(),
            draft.monitor_settings_expanded[resource as usize],
            Message::ToggleMonitorSettings(resource),
            widgets::switch(
                monitor.enabled,
                Some(move |enabled| Message::MonitorEnabled(resource, enabled)),
            ),
            widgets::settings_rows(memory_unit.into_iter().chain(rows)),
        )]
    }

    fn preview_matches_details(draft: &Draft) -> Column<'_, Message> {
        let matches = match &draft.preview.result {
            None => return column![text(t!("resource_thresholds.preview_loading").to_string())],
            Some(Err(error)) => {
                return column![text(
                    t!("resource_thresholds.preview_failed", error = error).to_string()
                )
                .style(text::danger)
                .wrapping(text::Wrapping::WordOrGlyph),];
            }
            Some(Ok(matches)) => matches,
        };

        if matches.is_empty() {
            return column![
                text(t!("resource_thresholds.preview_no_matches").to_string())
                    .style(text::secondary),
            ];
        }

        let rows = matches.iter().map(|entry| {
            let icon = entry.icon.as_deref().map_or_else(
                || super::navigation::glyph("icons/app-window.svg"),
                |icon| {
                    iced::widget::image(icon.clone())
                        .width(design::ICON_SIZE)
                        .height(design::ICON_SIZE)
                        .into()
                },
            );
            row![
                icon,
                text(t!("resource_thresholds.preview_pid", pid = entry.pid).to_string()).width(96),
                column![
                    text(entry.name.as_str()).wrapping(text::Wrapping::WordOrGlyph),
                    text(entry.path.to_string_lossy().into_owned())
                        .size(design::typography::SECONDARY)
                        .style(text::secondary)
                        .wrapping(text::Wrapping::WordOrGlyph),
                ]
                .spacing(design::space::TINY)
                .width(Fill),
            ]
            .spacing(design::space::MEDIUM)
            .align_y(iced::Center)
            .into()
        });
        column![widgets::settings_card(widgets::settings_rows(rows))]
    }

    pub(super) fn modal<'a>(
        &'a self,
        candidates: &[app_picker::Candidate],
    ) -> Option<Element<'a, Message>> {
        let draft = self.draft.as_ref()?;
        let rule = &draft.rule;
        let target = match &rule.selector {
            ResourceThresholdsSelector::ExactPath { executable_path } => executable_path.clone(),
            ResourceThresholdsSelector::ExactName { .. } => {
                t!("resource_thresholds.name_rule_mode").to_string()
            }
            ResourceThresholdsSelector::NamePattern { .. } => {
                t!("resource_thresholds.pattern_rule_mode").to_string()
            }
        };
        let valid = draft.validated_rule().is_some();
        let validation = (!valid).then(|| {
            text(t!("resource_thresholds.invalid").to_string())
                .style(text::danger)
                .into()
        });
        let (details, scroll_id) = match draft.tab {
            DraftTab::Monitor(resource) => (
                Self::monitor_details(draft, resource),
                "resource-thresholds-rule-details",
            ),
            DraftTab::PreviewMatches => (
                Self::preview_matches_details(draft),
                "resource-thresholds-preview-matches",
            ),
        };
        let frame = widgets::rule_dialog(
            t!(if draft.index.is_some() {
                "resource_thresholds.edit_rule"
            } else {
                "resource_thresholds.add_rule"
            })
            .to_string(),
            column![
                column![
                    app_picker::app_name(rule.selector.value(), candidates),
                    text(target)
                        .style(text::secondary)
                        .wrapping(text::Wrapping::WordOrGlyph),
                ]
                .spacing(design::space::SMALL)
                .width(Fill),
                iced::widget::container(row![
                    widgets::panel_tab(
                        t!("resource_thresholds.cpu_tab").to_string(),
                        draft.tab == DraftTab::Monitor(ResourceThresholdsResource::Cpu),
                        Message::Tab(ResourceThresholdsResource::Cpu)
                    ),
                    widgets::panel_tab(
                        t!("resource_thresholds.memory_tab").to_string(),
                        draft.tab == DraftTab::Monitor(ResourceThresholdsResource::Memory),
                        Message::Tab(ResourceThresholdsResource::Memory)
                    ),
                    widgets::panel_tab(
                        t!("common.preview_matches").to_string(),
                        draft.tab == DraftTab::PreviewMatches,
                        Message::OpenPreviewMatches
                    ),
                ])
                .style(widgets::tab_bar),
            ]
            .extend(validation)
            .push(super::motion::wrap(
                super::scrolling::scrollable_pane(details)
                    .id(scroll_id)
                    .height(Fill),
                true,
                super::motion::Effect::Content(draft.tab.content_key()),
            ))
            .spacing(widgets::CARD_GAP)
            .height(Fill),
            Message::CancelRule,
            valid.then_some(Message::SaveRule),
            (760, 840),
        );
        Some(super::modal::dismiss_on_escape(frame, Message::CancelRule))
    }
}

fn parse_duration(value: &str) -> Option<u64> {
    value
        .parse()
        .ok()
        .filter(|value| RESOURCE_THRESHOLDS_DURATION_RANGE.contains(value))
}

fn response(
    rule: &mut crate::config::ResourceThresholdsMonitor,
    critical: bool,
) -> &mut crate::config::ResourceThresholdsResponse {
    if critical {
        &mut rule.critical
    } else {
        &mut rule.warning
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_rules_reject_automatic_actions_and_can_coexist_with_exact_path_rules() {
        let mut editor = Editor::default();
        let mut settings = ResourceThresholdsSettings::default();
        editor.update(&mut settings, Message::MatchBy(MatchBy::ExecutableName));
        for invalid in [r"C:\Apps\*.exe", r"C:\Apps\worker.exe"] {
            editor.update(&mut settings, Message::Name(invalid.into()));
            editor.update(&mut settings, Message::Add);
            assert!(!editor.has_pending_editor());
        }

        editor.update(&mut settings, Message::Name("worker.exe".into()));
        editor.update(&mut settings, Message::Add);
        assert!(settings.rules.is_empty());
        editor.update(
            &mut settings,
            Message::Action(
                ResourceThresholdsResource::Cpu,
                false,
                ResourceThresholdsAction::Terminate,
            ),
        );
        editor.update(
            &mut settings,
            Message::Action(
                ResourceThresholdsResource::Cpu,
                true,
                ResourceThresholdsAction::Restart,
            ),
        );
        editor.update(
            &mut settings,
            Message::Action(
                ResourceThresholdsResource::Memory,
                false,
                ResourceThresholdsAction::TrimMemory,
            ),
        );
        editor.update(&mut settings, Message::SaveRule);
        assert!(!editor.has_pending_editor());
        assert_eq!(
            settings.rules[0],
            ResourceThresholdsRule::with_selector(ResourceThresholdsSelector::ExactName {
                executable_name: "worker.exe".into(),
            })
        );

        editor.update(&mut settings, Message::Name("WORKER.EXE".into()));
        editor.update(&mut settings, Message::Add);
        assert!(!editor.has_pending_editor());
        editor.update(&mut settings, Message::MatchBy(MatchBy::ExecutablePath));
        let path = r"C:\Apps\worker.exe";
        editor.update(&mut settings, Message::Path(path.into()));
        editor.update(&mut settings, Message::Add);
        editor.update(&mut settings, Message::SaveRule);
        assert_eq!(settings.rules[1], ResourceThresholdsRule::new(path.into()));
        for (input, expected) in [
            (picked_path_input(format!("{path} ")), format!("{path} ")),
            (format!("{path}\u{3000}"), format!("{path}\u{3000}")),
        ] {
            editor.update(&mut settings, Message::Path(input));
            editor.update(&mut settings, Message::Add);
            editor.update(&mut settings, Message::SaveRule);
            assert_eq!(
                settings.rules.last(),
                Some(&ResourceThresholdsRule::new(expected))
            );
        }
    }

    #[test]
    fn executable_name_input_stages_a_pattern_and_rejects_case_duplicate() {
        let mut editor = Editor::default();
        let mut settings = ResourceThresholdsSettings::default();
        editor.update(&mut settings, Message::MatchBy(MatchBy::ExecutableName));
        editor.update(&mut settings, Message::Name("abc*def.exe".into()));
        editor.update(&mut settings, Message::Add);
        editor.update(
            &mut settings,
            Message::Action(
                ResourceThresholdsResource::Cpu,
                true,
                ResourceThresholdsAction::Restart,
            ),
        );
        editor.update(&mut settings, Message::SaveRule);
        let [rule] = settings.rules.as_slice() else {
            panic!("expected one Resource Thresholds rule");
        };
        assert!(matches!(
            &rule.selector,
            ResourceThresholdsSelector::NamePattern { .. }
        ));
        assert_eq!(rule.selector.value(), "abc*def.exe");
        assert_eq!(rule.cpu.critical.action, ResourceThresholdsAction::Alert);
        assert!(editor.name.is_empty());

        editor.update(&mut settings, Message::Name("ABC*DEF.EXE".into()));
        editor.update(&mut settings, Message::Add);
        assert!(!editor.has_pending_editor());
    }

    #[test]
    fn disabled_monitor_keeps_valid_thresholds_and_ignores_incomplete_pairs() {
        let mut rule = ResourceThresholdsRule::new(r"C:\Apps\worker.exe".into());
        rule.cpu.enabled = false;
        rule.memory.enabled = true;
        let mut draft = Draft::new(None, rule);
        for (warning, expected) in [("65", 65), ("", 70), ("90", 70)] {
            draft.monitors[ResourceThresholdsResource::Cpu as usize].warning = warning.into();
            let saved = draft
                .validated_rule()
                .expect("memory monitor remains valid");
            assert!(!saved.cpu.enabled);
            assert_eq!(saved.cpu.warning.threshold, expected);
            assert_eq!(saved.cpu.critical.threshold, 90);
        }
    }

    #[test]
    fn enabled_monitors_open_expanded_and_tabs_preserve_disclosure() {
        use ResourceThresholdsResource::{Cpu, Memory};

        let mut rule = ResourceThresholdsRule::new(r"C:\Apps\test.exe".into());
        rule.memory.enabled = true;
        rule.cpu.critical.action = ResourceThresholdsAction::Restart;
        let mut settings = ResourceThresholdsSettings {
            rules: vec![rule.clone()],
            ..Default::default()
        };
        let mut editor = Editor::default();
        editor.update(&mut settings, Message::Edit(0));
        assert_eq!(
            editor.draft.as_ref().unwrap().monitor_settings_expanded,
            [true, true]
        );
        editor.update(&mut settings, Message::ToggleMonitorSettings(Cpu));
        editor.update(&mut settings, Message::MonitorEnabled(Cpu, false));
        editor.update(&mut settings, Message::Tab(Memory));
        editor.update(&mut settings, Message::Tab(Cpu));
        assert_eq!(
            editor.draft.as_ref().unwrap().monitor_settings_expanded,
            [false, true]
        );
        assert_eq!(settings.rules[0], rule);

        editor.update(&mut settings, Message::SaveRule);
        rule.cpu.enabled = false;
        assert_eq!(settings.rules[0], rule);
        editor.update(&mut settings, Message::Edit(0));
        assert_eq!(
            editor.draft.as_ref().unwrap().monitor_settings_expanded,
            [false, true]
        );
        editor.update(&mut settings, Message::MonitorEnabled(Cpu, true));
        assert_eq!(
            editor.draft.as_ref().unwrap().monitor_settings_expanded,
            [true, true]
        );
    }

    #[test]
    fn changing_memory_unit_requires_explicit_thresholds_in_that_unit() {
        use ResourceThresholdsMemoryUnit::{Megabytes, Percent};
        use ResourceThresholdsResource::Memory;

        let rule = ResourceThresholdsRule::new(r"C:\Apps\test.exe".into());
        let mut settings = ResourceThresholdsSettings {
            rules: vec![rule.clone()],
            ..Default::default()
        };
        let mut editor = Editor::default();
        editor.update(&mut settings, Message::Edit(0));
        editor.update(&mut settings, Message::MemoryUnit(Megabytes));
        editor.update(&mut settings, Message::SaveRule);
        assert!(editor.has_pending_editor());
        assert_eq!(settings.rules[0], rule);

        for (critical, value) in [(false, "512"), (true, "1024")] {
            editor.update(
                &mut settings,
                Message::Threshold(Memory, critical, value.into()),
            );
        }
        editor.update(&mut settings, Message::MemoryUnit(Percent));
        assert_eq!(editor.draft.as_ref().unwrap().validated_rule(), Some(rule));
        editor.update(&mut settings, Message::MemoryUnit(Megabytes));
        editor.update(&mut settings, Message::MonitorEnabled(Memory, true));
        editor.update(&mut settings, Message::SaveRule);
        assert!(!editor.has_pending_editor());
        let rule = &settings.rules[0];
        assert_eq!(rule.memory_unit, Megabytes);
        assert_eq!(rule.memory.warning.threshold, 512);
        assert_eq!(rule.memory.critical.threshold, 1024);
        assert!(rule.memory.enabled);
    }

    #[test]
    fn preview_matches_polls_only_the_active_tab_and_discards_stale_results() {
        let mut editor = Editor::default();
        let mut settings = ResourceThresholdsSettings {
            rules: vec![ResourceThresholdsRule::new(r"C:\Apps\test.exe".into())],
            ..Default::default()
        };
        let saved = settings.clone();
        editor.update(&mut settings, Message::Edit(0));
        drop(editor.poll_preview_matches());
        assert!(editor.draft.as_ref().unwrap().preview.pending.is_none());

        drop(editor.open_preview_matches());
        assert!(editor.preview_matches_open());
        let previous = editor
            .draft
            .as_ref()
            .unwrap()
            .preview
            .pending
            .clone()
            .unwrap();

        editor.update(&mut settings, Message::CancelRule);
        editor.update(&mut settings, Message::Edit(0));
        drop(editor.open_preview_matches());
        let request = |editor: &Editor| {
            editor
                .draft
                .as_ref()
                .unwrap()
                .preview
                .pending
                .clone()
                .unwrap()
        };
        let current = request(&editor);
        editor.complete_preview_matches(previous, Err("obsolete".into()));
        drop(editor.poll_preview_matches());
        assert!(Arc::ptr_eq(&request(&editor), &current));

        editor.complete_preview_matches(current.clone(), Ok(Arc::from([])));
        drop(editor.poll_preview_matches());
        assert!(!Arc::ptr_eq(&request(&editor), &current));
        assert!(matches!(
            &editor.draft.as_ref().unwrap().preview.result,
            Some(Ok(rows)) if rows.is_empty()
        ));
        editor.select_tab(ResourceThresholdsResource::Cpu);
        assert!(!editor.preview_matches_open());
        assert_eq!(settings, saved);
    }

    #[test]
    fn duration_keeps_temporary_text_and_only_updates_settings_when_valid() {
        let mut editor = Editor::default();
        let mut settings = ResourceThresholdsSettings::default();
        assert!(!editor.has_invalid_inputs());
        for value in ["", "0", "3601", "invalid", "18446744073709551616"] {
            editor.update(&mut settings, Message::Duration(value.into()));
            assert_eq!(editor.duration.as_deref(), Some(value));
            assert!(editor.has_invalid_inputs());
            assert_eq!(settings.sustained_seconds, 5);
        }
        for (value, expected) in [("1", 1), ("10", 10), ("3600", 3600)] {
            editor.update(&mut settings, Message::Duration(value.into()));
            assert_eq!(editor.duration.as_deref(), Some(value));
            assert!(!editor.has_invalid_inputs());
            assert_eq!(settings.sustained_seconds, expected);
        }
        editor.update(&mut settings, Message::Duration(String::new()));
        assert!(editor.has_invalid_inputs());
        assert_eq!(settings.sustained_seconds, 3600);
        editor = Editor::default();
        assert!(editor.duration.is_none());
        assert!(!editor.has_invalid_inputs());
    }

    #[test]
    fn cpu_memory_tab_transitions_keep_repaints_bounded() {
        use iced::advanced::{layout, renderer::Renderer as _, widget::Tree, Layout};
        use iced::{mouse, Rectangle, Size};
        use std::time::Instant;

        let mut rule = ResourceThresholdsRule::new(r"C:\Apps\test.exe".into());
        rule.memory.enabled = true;
        let mut settings = ResourceThresholdsSettings {
            rules: vec![rule],
            ..Default::default()
        };
        let mut editor = Editor::default();
        editor.update(&mut settings, Message::Edit(0));
        let mut renderer = iced::Renderer::new(design::typography::FONT, iced::Pixels(14.0));
        let bounds = Rectangle::with_size(Size::new(1400.0, 900.0));
        let physical = Size::new(2100, 1350);
        let viewport = iced::advanced::graphics::Viewport::with_physical_size(physical, 1.5);
        let mut pixels = tiny_skia::Pixmap::new(physical.width, physical.height).unwrap();
        let mut mask = tiny_skia::Mask::new(physical.width, physical.height).unwrap();
        let mut tree = Tree::empty();
        let mut previous = Vec::new();
        for frame in 0..12 {
            let start = Instant::now();
            editor.update(
                &mut settings,
                Message::Tab(ResourceThresholdsResource::ALL[frame % 2]),
            );
            let mut view: Element<'_, Message> = iced::widget::stack![
                editor.view(&settings, &[]),
                super::super::scrolling::modal_backdrop(),
                iced::widget::container(editor.modal(&[]).unwrap())
                    .padding(16)
                    .center_x(Fill)
                    .center_y(Fill),
            ]
            .into();
            tree.diff(&view);
            let node = view.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(Size::ZERO, bounds.size()),
            );
            let laid_out = start.elapsed();
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
            let recorded = start.elapsed();
            let damage = if frame == 0 {
                vec![bounds]
            } else {
                iced::advanced::graphics::damage::group(
                    iced::advanced::graphics::damage::diff(
                        &previous,
                        renderer.layers(),
                        |layer| vec![layer.bounds],
                        iced_tiny_skia::Layer::damage,
                    ),
                    bounds,
                )
            };
            previous = renderer.layers().to_vec();
            assert!(
                damage.len() <= 4,
                "tab switch fragmented repaint: {damage:?}"
            );
            if frame > 0 {
                assert!(
                    damage
                        .iter()
                        .all(|region| region.x >= 320.0 && region.x + region.width <= 1080.0),
                    "tab switch repainted the page behind the dialog: {damage:?}"
                );
            }
            renderer.draw(
                &mut pixels.as_mut(),
                &mut mask,
                &viewport,
                &damage,
                iced::Color::WHITE,
            );
            eprintln!("ResourceThresholds tab {frame}: layout={laid_out:?}, draw={:?}, raster={:?}, total={:?}, regions={}",
                recorded-laid_out, start.elapsed()-recorded, start.elapsed(), damage.len());
            let mut expected = tiny_skia::Pixmap::new(physical.width, physical.height).unwrap();
            renderer.draw(
                &mut expected.as_mut(),
                &mut mask,
                &viewport,
                &[bounds],
                iced::Color::WHITE,
            );
            // A clipped rounded card uses a different antialias path in TinySkia.
            // Normalize the damaged regions while retaining the full redraw elsewhere.
            renderer.draw(
                &mut expected.as_mut(),
                &mut mask,
                &viewport,
                &damage,
                iced::Color::WHITE,
            );

            // Upstream damage-edge antialias coverage differs from a full redraw.
            // Compare the interiors to catch stale content without masking that known edge rounding.
            for (index, (actual, expected)) in pixels
                .data()
                .as_chunks::<4>()
                .0
                .iter()
                .zip(expected.data().as_chunks::<4>().0)
                .enumerate()
            {
                if actual
                    .iter()
                    .zip(expected)
                    .all(|(a, b)| a.abs_diff(*b) <= 1)
                {
                    continue;
                }
                let x = (index % physical.width as usize) as f32 + 0.5;
                let y = (index / physical.width as usize) as f32 + 0.5;
                assert!(
                    damage.iter().any(|r| {
                        let r = *r * 1.5;
                        ((x - r.x).abs() <= 1.0 || (x - r.x - r.width).abs() <= 1.0)
                            && y >= r.y - 1.0
                            && y <= r.y + r.height + 1.0
                            || ((y - r.y).abs() <= 1.0 || (y - r.y - r.height).abs() <= 1.0)
                                && x >= r.x - 1.0
                                && x <= r.x + r.width + 1.0
                    }),
                    "partial repaint left stale interior pixels on tab frame {frame}: {x},{y}; damage {damage:?}"
                );
            }
        }
    }

    #[test]
    fn both_resources_edit_independently_and_dialog_remains_transactional() {
        let mut editor = Editor::default();
        let mut settings = ResourceThresholdsSettings::default();
        editor.update(&mut settings, Message::Path(r"C:\Apps\test.exe".into()));
        editor.update(&mut settings, Message::Add);
        assert!(editor.has_pending_editor());
        assert!(settings.rules.is_empty());
        editor.update(&mut settings, Message::CancelRule);
        assert!(settings.rules.is_empty());
        editor.update(&mut settings, Message::Add);
        editor.update(
            &mut settings,
            Message::MonitorEnabled(ResourceThresholdsResource::Memory, true),
        );
        editor.update(
            &mut settings,
            Message::Tab(ResourceThresholdsResource::Memory),
        );
        editor.update(
            &mut settings,
            Message::Threshold(ResourceThresholdsResource::Memory, true, "95".into()),
        );
        editor.update(
            &mut settings,
            Message::Threshold(ResourceThresholdsResource::Cpu, false, "65".into()),
        );
        editor.update(
            &mut settings,
            Message::Action(
                ResourceThresholdsResource::Memory,
                true,
                ResourceThresholdsAction::Restart,
            ),
        );
        for resource in [
            ResourceThresholdsResource::Cpu,
            ResourceThresholdsResource::Memory,
        ] {
            editor.update(
                &mut settings,
                Message::Action(resource, false, ResourceThresholdsAction::TrimMemory),
            );
        }
        editor.update(&mut settings, Message::Tab(ResourceThresholdsResource::Cpu));
        editor.update(&mut settings, Message::SaveRule);
        assert!(!editor.has_pending_editor());
        assert!(settings.rules[0].cpu.enabled && settings.rules[0].memory.enabled);
        assert_eq!(settings.rules[0].cpu.warning.threshold, 65);
        assert_eq!(settings.rules[0].memory.warning.threshold, 70);
        assert_eq!(settings.rules[0].memory.critical.threshold, 95);
        assert_eq!(
            settings.rules[0].cpu.warning.action,
            ResourceThresholdsAction::Alert
        );
        assert_eq!(
            settings.rules[0].memory.warning.action,
            ResourceThresholdsAction::TrimMemory
        );
        assert_eq!(
            settings.rules[0].memory.critical.action,
            ResourceThresholdsAction::Restart
        );
        let saved = settings.clone();
        editor.update(&mut settings, Message::Edit(0));
        editor.update(
            &mut settings,
            Message::Threshold(ResourceThresholdsResource::Memory, true, "".into()),
        );
        editor.update(&mut settings, Message::SaveRule);
        assert!(editor.has_pending_editor());
        assert_eq!(settings, saved);
        editor.update(&mut settings, Message::CancelRule);
        assert_eq!(settings, saved);
        editor.update(&mut settings, Message::Edit(0));
        editor.update(
            &mut settings,
            Message::MonitorEnabled(ResourceThresholdsResource::Cpu, false),
        );
        editor.update(
            &mut settings,
            Message::MonitorEnabled(ResourceThresholdsResource::Memory, false),
        );
        editor.update(&mut settings, Message::SaveRule);
        assert!(editor.has_pending_editor());
        editor.update(&mut settings, Message::CancelRule);
        editor.update(&mut settings, Message::Path(r"C:\Apps\test.exe".into()));
        editor.update(&mut settings, Message::Add);
        assert!(!editor.has_pending_editor());
    }
}
