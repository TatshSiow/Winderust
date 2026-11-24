use std::{collections::BTreeMap, path::Path, sync::Arc};

use super::{
    app_picker, design,
    process_rules::{exact_path_input, picked_path_input, MatchBy},
    widgets,
};
use crate::config::{
    ResourceThresholdsAction, ResourceThresholdsComparison, ResourceThresholdsMemoryUnit,
    ResourceThresholdsMetric, ResourceThresholdsResource, ResourceThresholdsRule,
    ResourceThresholdsScope, ResourceThresholdsSelector, ResourceThresholdsSettings,
    RESOURCE_THRESHOLDS_DURATION_RANGE,
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

impl std::fmt::Display for ResourceThresholdsMetric {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&t!(self.label_key()))
    }
}
impl std::fmt::Display for ResourceThresholdsMemoryUnit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.metric(), f)
    }
}

impl std::fmt::Display for ResourceThresholdsAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&t!(match self {
            Self::None => "resource_thresholds.no_action",
            Self::Terminate => "resource_thresholds.terminate",
            Self::Restart => "resource_thresholds.restart",
        }))
    }
}

impl std::fmt::Display for ResourceThresholdsComparison {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&t!(match self {
            Self::AtOrAbove => "resource_thresholds.at_or_above",
            Self::AtOrBelow => "resource_thresholds.at_or_below",
        }))
    }
}

impl std::fmt::Display for ResourceThresholdsScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&t!(match self {
            Self::Combined => "resource_thresholds.scope_combined",
            Self::PerInstance => "resource_thresholds.scope_per_instance",
        }))
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
    duration: String,
    tab: DraftTab,
    monitors: [ThresholdInputs; 2],
    monitor_settings_expanded: [bool; 2],
    match_preview: MatchPreview,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DraftTab {
    Monitor(ResourceThresholdsResource),
    MatchPreview,
}

impl DraftTab {
    fn content_key(self) -> u64 {
        match self {
            Self::Monitor(resource) => resource as u64,
            Self::MatchPreview => 2,
        }
    }
}

enum MatchPreview {
    Unrequested,
    Loading(Arc<()>),
    Loaded(Result<Vec<preview::MatchRow>, String>),
}

struct ThresholdInputs {
    warning: String,
    critical: String,
}

impl Draft {
    fn new(index: Option<usize>, rule: ResourceThresholdsRule) -> Self {
        Self {
            index,
            duration: rule
                .sustained_seconds
                .map_or_else(String::new, |seconds| seconds.to_string()),
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
            rule,
            monitor_settings_expanded: [false; 2],
            match_preview: MatchPreview::Unrequested,
        }
    }

    fn parsed_duration(&self) -> Option<Option<u64>> {
        if self.duration.is_empty() {
            Some(None)
        } else {
            parse_duration(&self.duration).map(Some)
        }
    }

    fn validated_rule(&self) -> Option<ResourceThresholdsRule> {
        let mut rule = self.rule.clone();
        rule.sustained_seconds = self.parsed_duration()?;
        for (resource, raw) in ResourceThresholdsResource::ALL
            .into_iter()
            .zip(&self.monitors)
        {
            let metric = rule.metric(resource);
            let monitor = rule.monitor_mut(resource);
            if let (Ok(warning), Ok(critical)) = (raw.warning.parse(), raw.critical.parse()) {
                let mut edited = monitor.clone();
                edited.warning.threshold = warning;
                edited.critical.threshold = critical;
                if monitor.enabled || edited.thresholds_valid(metric) {
                    *monitor = edited;
                }
            } else if monitor.enabled {
                return None;
            }
        }
        rule.validate().ok()?;
        Some(rule)
    }

    fn clear_unavailable_actions(&mut self, resource: ResourceThresholdsResource) {
        if !self.rule.allows_actions(resource) {
            let monitor = self.rule.monitor_mut(resource);
            monitor.warning.action = ResourceThresholdsAction::None;
            monitor.critical.action = ResourceThresholdsAction::None;
        }
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
    PreviewMatches,
    MatchPreviewLoaded(Arc<()>, Result<Vec<preview::MatchRow>, String>),
    Scope(ResourceThresholdsScope),
    MonitorEnabled(ResourceThresholdsResource, bool),
    ToggleMonitorSettings(ResourceThresholdsResource),
    Comparison(ResourceThresholdsResource, ResourceThresholdsComparison),
    MemoryUnit(ResourceThresholdsMemoryUnit),
    Duration(String),
    RuleDuration(String),
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
                matches!(
                    &rule.selector,
                    ResourceThresholdsSelector::ExactName { executable_name }
                        if same_executable_name(executable_name, name)
                )
            })
        {
            return None;
        }
        let selector = ResourceThresholdsSelector::ExactName {
            executable_name: name.into(),
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

    pub(super) fn preview_matches(
        &mut self,
        settings: &ResourceThresholdsSettings,
    ) -> iced::Task<Message> {
        let Some(draft) = &mut self.draft else {
            return iced::Task::none();
        };
        draft.tab = DraftTab::MatchPreview;
        if matches!(draft.match_preview, MatchPreview::Loading(_)) {
            return iced::Task::none();
        }
        let request = Arc::new(());
        draft.match_preview = MatchPreview::Loading(request.clone());
        let selector = draft.rule.selector.clone();
        let others = preview::other_enabled_rules(&settings.rules, draft.index);
        super::tasks::run(move || preview::load(&selector, &others)).map(move |result| {
            Message::MatchPreviewLoaded(request.clone(), result.and_then(|result| result))
        })
    }

    pub(super) fn complete_match_preview(
        &mut self,
        request: Arc<()>,
        result: Result<Vec<preview::MatchRow>, String>,
    ) {
        let Some(draft) = &mut self.draft else {
            return;
        };
        if matches!(&draft.match_preview, MatchPreview::Loading(active) if Arc::ptr_eq(active, &request))
        {
            draft.match_preview = MatchPreview::Loaded(result);
        }
    }

    #[cfg(feature = "render-smoke")]
    pub(super) fn set_smoke_match_preview(&mut self) {
        let draft = self
            .draft
            .as_mut()
            .expect("preview fixture has an open rule");
        draft.tab = DraftTab::MatchPreview;
        draft.match_preview = MatchPreview::Loaded(Ok([
            (41, r"C:\Apps\worker.exe"),
            (42, r"C:\Apps\worker.exe"),
            (81, r"D:\Tools\worker.exe"),
        ]
        .into_iter()
        .map(|(pid, path)| preview::MatchRow {
            pid,
            path: Some(path.into()),
            kind: preview::MatchKind::Matched(path.into()),
            other_matching_rules: Vec::new(),
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
                            match &rule.selector {
                                ResourceThresholdsSelector::ExactPath { .. } => self.path.clear(),
                                ResourceThresholdsSelector::ExactName { .. } => self.name.clear(),
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
            Message::Scope(scope) => {
                if let Some(draft) = &mut self.draft {
                    draft.rule.scope = scope;
                    for resource in ResourceThresholdsResource::ALL {
                        draft.clear_unavailable_actions(resource);
                    }
                }
            }
            Message::MonitorEnabled(resource, enabled) => {
                if let Some(draft) = &mut self.draft {
                    draft.rule.monitor_mut(resource).enabled = enabled;
                }
            }
            Message::Comparison(resource, comparison) => {
                if let Some(draft) = self
                    .draft
                    .as_mut()
                    .filter(|draft| draft.rule.monitor(resource).comparison != comparison)
                {
                    let monitor = draft.rule.monitor_mut(resource);
                    monitor.comparison = comparison;
                    std::mem::swap(
                        &mut monitor.warning.threshold,
                        &mut monitor.critical.threshold,
                    );
                    let raw = &mut draft.monitors[resource as usize];
                    std::mem::swap(&mut raw.warning, &mut raw.critical);
                    draft.clear_unavailable_actions(resource);
                }
            }
            Message::MemoryUnit(unit) => {
                if let Some(draft) = &mut self.draft {
                    if draft.rule.memory_unit != unit {
                        draft.rule.memory_unit = unit;
                        let (warning, critical) = match unit {
                            ResourceThresholdsMemoryUnit::Percent => (70, 90),
                            ResourceThresholdsMemoryUnit::Megabytes
                            | ResourceThresholdsMemoryUnit::PrivateCommitMb => (1000, 2000),
                        };
                        let memory = &mut draft.rule.memory;
                        let (warning, critical) = match memory.comparison {
                            ResourceThresholdsComparison::AtOrAbove => (warning, critical),
                            ResourceThresholdsComparison::AtOrBelow => (critical, warning),
                        };
                        memory.warning.threshold = warning;
                        memory.critical.threshold = critical;
                        draft.monitors[ResourceThresholdsResource::Memory as usize] =
                            ThresholdInputs {
                                warning: warning.to_string(),
                                critical: critical.to_string(),
                            };
                    }
                }
            }
            Message::Duration(value) => {
                if let Some(seconds) = parse_duration(&value) {
                    settings.sustained_seconds = seconds;
                }
                self.duration = Some(value);
            }
            Message::RuleDuration(value) => {
                if let Some(draft) = &mut self.draft {
                    draft.duration = value;
                }
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
                    .filter(|draft| draft.rule.allows_actions(resource))
                {
                    response(draft.rule.monitor_mut(resource), critical).action = value;
                }
            }
            Message::Remove(_)
            | Message::Browse
            | Message::PreviewMatches
            | Message::MatchPreviewLoaded(..) => {}
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
                let (app, target) = widgets::process_rule_target(&rule.selector, candidates);
                let label = t!("common.rule", number = index + 1);
                let label = match rule.scope {
                    ResourceThresholdsScope::Combined => label.into_owned(),
                    ResourceThresholdsScope::PerInstance => format!("{label} · {}", rule.scope),
                };
                (
                    widgets::stable_key(&format!("{index}:{value}")),
                    column![
                        widgets::process_rule_header_with_target_and_notify(
                            column![
                                text(label)
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
                                    let warning = monitor.warning.threshold;
                                    let critical = monitor.critical.threshold;
                                    let comparison = match monitor.comparison {
                                        ResourceThresholdsComparison::AtOrAbove => "",
                                        ResourceThresholdsComparison::AtOrBelow => monitor.comparison.symbol(),
                                    };
                                    text(if !monitor.enabled {
                                        t!("common.off").to_string()
                                    } else if resource == ResourceThresholdsResource::Memory {
                                        let metric = rule.metric(resource);
                                        format!("{metric}\n{comparison}{warning} / {comparison}{critical}")
                                    } else {
                                        format!("{comparison}{warning}% / {comparison}{critical}%")
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
        let mut duration = column![widgets::setting_row_with_unit(
            "resource_thresholds.duration",
            "s",
            widgets::text_input(
                "",
                self.duration
                    .as_deref()
                    .unwrap_or(&settings.sustained_seconds.to_string()),
            )
            .on_input(Message::Duration)
            .width(design::STANDALONE_NUMERIC_WIDTH),
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
            MatchBy::ExecutableName => {
                let add = self.new_name_selector(settings).map(|_| Message::Add);
                column![
                    widgets::setting_title("common.executable_name"),
                    row![
                        widgets::text_input("app.exe", &self.name)
                            .on_input(Message::Name)
                            .on_submit_maybe(add.clone())
                            .width(Fill),
                        widgets::rule_add_button(add),
                    ]
                    .spacing(design::space::SMALL)
                    .align_y(iced::Center),
                ]
                .spacing(design::space::SMALL)
                .into()
            }
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
        let mut measurement = vec![widgets::setting_row(
            "resource_thresholds.comparison",
            widgets::pick_list(
                [
                    ResourceThresholdsComparison::AtOrAbove,
                    ResourceThresholdsComparison::AtOrBelow,
                ],
                Some(monitor.comparison),
                move |value| Message::Comparison(resource, value),
            )
            .width(180),
        )
        .into()];
        if resource == ResourceThresholdsResource::Memory {
            measurement.push(
                widgets::setting_row(
                    "resource_thresholds.memory_metric",
                    widgets::pick_list(
                        ResourceThresholdsMemoryUnit::ALL,
                        Some(rule.memory_unit),
                        Message::MemoryUnit,
                    )
                    .width(180),
                )
                .into(),
            );
        }
        // Keep the measurement and response sections in separate repaint regions.
        // The Memory tab adds a metric row that shifts both response sections.
        let mut rows = vec![super::scrolling::repaint_group(widgets::settings_rows(
            measurement,
        ))];
        let raw = &draft.monitors[resource as usize];
        let metric = rule.metric(resource);
        for (critical, response, threshold) in [
            (false, &monitor.warning, &raw.warning),
            (true, &monitor.critical, &raw.critical),
        ] {
            let mut details = column![
                widgets::heading(
                    format!(
                        "{} · {}",
                        metric,
                        t!(if critical {
                            "resource_thresholds.critical"
                        } else {
                            "resource_thresholds.warning"
                        })
                    ),
                    design::typography::BODY,
                ),
                widgets::setting_row_with_unit(
                    "resource_thresholds.threshold",
                    metric.unit(),
                    widgets::text_input("", threshold)
                        .on_input(move |value| Message::Threshold(resource, critical, value))
                        .on_submit(Message::SaveRule)
                        .width(design::STANDALONE_NUMERIC_WIDTH),
                ),
            ]
            .spacing(design::space::MEDIUM);
            if rule.allows_actions(resource) {
                details = details
                    .push(widgets::settings_divider())
                    .push(widgets::setting_row(
                        "resource_thresholds.action",
                        widgets::pick_list(
                            [
                                ResourceThresholdsAction::None,
                                ResourceThresholdsAction::Terminate,
                                ResourceThresholdsAction::Restart,
                            ],
                            Some(response.action),
                            move |value| Message::Action(resource, critical, value),
                        )
                        .width(180),
                    ));
            }
            rows.push(super::scrolling::repaint_group(details));
        }
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
            widgets::settings_rows(rows),
        )]
    }

    fn match_preview_details(draft: &Draft) -> Column<'_, Message> {
        use preview::{MatchKind, UnavailableReason};

        let mut details = column![widgets::action_button(t!("settings.refresh").to_string())
            .style(widgets::secondary_button)
            .on_press_maybe(
                (!matches!(draft.match_preview, MatchPreview::Loading(_)))
                    .then_some(Message::PreviewMatches),
            )]
        .spacing(widgets::CARD_GAP);
        let matches = match &draft.match_preview {
            MatchPreview::Unrequested => return details,
            MatchPreview::Loading(_) => {
                return details.push(text(t!("resource_thresholds.preview_loading").to_string()));
            }
            MatchPreview::Loaded(Err(error)) => {
                return details.push(
                    text(t!("resource_thresholds.preview_failed", error = error).to_string())
                        .style(text::danger)
                        .wrapping(text::Wrapping::WordOrGlyph),
                );
            }
            MatchPreview::Loaded(Ok(matches)) => matches,
        };

        let combined = draft.rule.scope == ResourceThresholdsScope::Combined;
        let mut path_groups = BTreeMap::<&Path, Vec<_>>::new();
        let mut matched = Vec::new();
        let mut same_name = Vec::new();
        let mut unavailable = Vec::new();
        for entry in matches {
            let (rows, grouped, reason) = match &entry.kind {
                MatchKind::Matched(path) if combined => {
                    (path_groups.entry(path).or_default(), true, None)
                }
                MatchKind::Matched(_) => (&mut matched, false, None),
                MatchKind::SameName => (&mut same_name, false, None),
                MatchKind::Unavailable(reason) => (&mut unavailable, false, Some(*reason)),
            };
            let mut metadata = column![].spacing(design::space::TINY).width(Fill);
            if !grouped {
                let path = entry.path.as_ref().map_or_else(
                    || t!(UnavailableReason::PathUnavailable.label_key()).to_string(),
                    |path| path.display().to_string(),
                );
                metadata = metadata.push(
                    text(path)
                        .style(text::secondary)
                        .wrapping(text::Wrapping::WordOrGlyph),
                );
            }
            let detail = match reason.filter(|_| entry.path.is_some()) {
                Some(reason) => Some(t!(reason.label_key())),
                None if !entry.other_matching_rules.is_empty() => {
                    let numbers = entry
                        .other_matching_rules
                        .iter()
                        .map(|index| (index + 1).to_string())
                        .collect::<Vec<_>>()
                        .join(", ");
                    Some(t!(
                        "resource_thresholds.preview_other_enabled_rules",
                        numbers = numbers
                    ))
                }
                None => None,
            };
            if let Some(detail) = detail {
                metadata = metadata.push(
                    text(detail.to_string())
                        .size(design::typography::CAPTION)
                        .style(text::secondary)
                        .wrapping(text::Wrapping::WordOrGlyph),
                );
            }
            rows.push(
                row![
                    text(t!("resource_thresholds.preview_pid", pid = entry.pid).to_string())
                        .width(112),
                    metadata,
                ]
                .spacing(design::space::MEDIUM)
                .width(Fill)
                .into(),
            );
        }
        if matched.is_empty() && path_groups.is_empty() {
            details = details.push(
                text(t!("resource_thresholds.preview_no_matches").to_string())
                    .style(text::secondary),
            );
        }
        if !path_groups.is_empty() {
            let groups = path_groups.into_iter().map(|(path, rows)| {
                column![
                    widgets::heading(path.display().to_string(), design::typography::SECONDARY)
                        .wrapping(text::Wrapping::WordOrGlyph),
                    Column::with_children(rows).spacing(design::space::SMALL),
                ]
                .spacing(design::space::SMALL)
                .into()
            });
            details = details.push(Self::preview_group(
                "resource_thresholds.scope_combined",
                Some("resource_thresholds.preview_combined_scope"),
                groups,
            ));
        }
        for (rows, heading, scope_help) in [
            (
                matched,
                "resource_thresholds.preview_matches",
                Some("resource_thresholds.preview_instance_scope"),
            ),
            (same_name, "resource_thresholds.preview_same_name", None),
            (unavailable, "resource_thresholds.preview_unavailable", None),
        ] {
            if rows.is_empty() {
                continue;
            }
            details = details.push(Self::preview_group(heading, scope_help, rows));
        }
        details
    }

    fn preview_group<'a>(
        heading: &str,
        scope_help: Option<&str>,
        rows: impl IntoIterator<Item = Element<'a, Message>>,
    ) -> Element<'a, Message> {
        widgets::settings_card(
            column![widgets::heading(
                t!(heading).to_string(),
                design::typography::BODY
            )]
            .extend(scope_help.map(|key| {
                text(t!(key).into_owned())
                    .size(design::typography::CAPTION)
                    .style(text::secondary)
                    .wrapping(text::Wrapping::WordOrGlyph)
                    .into()
            }))
            .push(Column::with_children(rows).spacing(design::space::SMALL))
            .spacing(design::space::MEDIUM),
        )
        .into()
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
        };
        let duration_valid = draft.parsed_duration().is_some();
        let valid = duration_valid && draft.validated_rule().is_some();
        let mut duration = column![widgets::setting_row_with_unit(
            "resource_thresholds.rule_duration",
            "s",
            widgets::text_input(&t!("common.default"), &draft.duration)
                .on_input(Message::RuleDuration)
                .on_submit(Message::SaveRule)
                .width(design::STANDALONE_NUMERIC_WIDTH),
        )]
        .spacing(design::space::SMALL)
        .width(280);
        if !duration_valid {
            duration = duration.push(
                text(t!("resource_thresholds.invalid_duration").to_string()).style(text::danger),
            );
        }
        let validation = (duration_valid && !valid).then(|| {
            text(t!("resource_thresholds.invalid").to_string())
                .style(text::danger)
                .into()
        });
        let (details, scroll_id) = match draft.tab {
            DraftTab::Monitor(resource) => (
                Self::monitor_details(draft, resource),
                "resource-thresholds-rule-details",
            ),
            DraftTab::MatchPreview => (
                Self::match_preview_details(draft),
                "resource-thresholds-match-preview",
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
                row![
                    column![
                        app_picker::app_name(rule.selector.value(), candidates),
                        text(target)
                            .style(text::secondary)
                            .wrapping(text::Wrapping::WordOrGlyph),
                    ]
                    .spacing(design::space::SMALL)
                    .width(Fill),
                    duration,
                ]
                .spacing(design::space::MEDIUM)
                .align_y(iced::Center),
                widgets::settings_card(widgets::setting_row(
                    "resource_thresholds.scope",
                    widgets::pick_list(
                        [
                            ResourceThresholdsScope::Combined,
                            ResourceThresholdsScope::PerInstance
                        ],
                        Some(rule.scope),
                        Message::Scope,
                    )
                    .width(180),
                )),
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
                        t!("resource_thresholds.match_preview").to_string(),
                        draft.tab == DraftTab::MatchPreview,
                        Message::PreviewMatches
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
        for invalid in ["*.exe", r"C:\Apps\worker.exe"] {
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
    fn per_instance_scope_clears_draft_actions_and_rejects_action_messages() {
        let mut rule = ResourceThresholdsRule::new(r"C:\Apps\worker.exe".into());
        for resource in ResourceThresholdsResource::ALL {
            let monitor = rule.monitor_mut(resource);
            monitor.warning.action = ResourceThresholdsAction::Terminate;
            monitor.critical.action = ResourceThresholdsAction::Restart;
        }
        let mut settings = ResourceThresholdsSettings {
            rules: vec![rule.clone()],
            ..Default::default()
        };
        let mut editor = Editor::default();
        editor.update(&mut settings, Message::Edit(0));
        editor.update(
            &mut settings,
            Message::Scope(ResourceThresholdsScope::PerInstance),
        );
        assert_eq!(settings.rules[0], rule);

        editor.update(&mut settings, Message::CancelRule);
        assert_eq!(settings.rules[0], rule);
        editor.update(&mut settings, Message::Edit(0));
        editor.update(
            &mut settings,
            Message::Scope(ResourceThresholdsScope::PerInstance),
        );
        for resource in ResourceThresholdsResource::ALL {
            editor.update(
                &mut settings,
                Message::Action(resource, true, ResourceThresholdsAction::Restart),
            );
        }
        editor.update(&mut settings, Message::SaveRule);
        assert!(!editor.has_pending_editor());
        assert_eq!(
            settings.rules[0].scope,
            ResourceThresholdsScope::PerInstance
        );
        for resource in ResourceThresholdsResource::ALL {
            let monitor = settings.rules[0].monitor(resource);
            assert_eq!(monitor.warning.action, ResourceThresholdsAction::None);
            assert_eq!(monitor.critical.action, ResourceThresholdsAction::None);
        }
    }

    #[test]
    fn low_usage_comparison_edits_only_its_monitor() {
        let mut rule = ResourceThresholdsRule::new(r"C:\Apps\test.exe".into());
        rule.memory.enabled = true;
        rule.cpu.critical.action = ResourceThresholdsAction::Restart;
        rule.memory.warning.action = ResourceThresholdsAction::Terminate;
        let cpu = rule.cpu.clone();
        let mut settings = ResourceThresholdsSettings {
            rules: vec![rule],
            ..Default::default()
        };
        let saved = settings.clone();
        let mut editor = Editor::default();
        let memory = ResourceThresholdsResource::Memory;
        let low = Message::Comparison(memory, ResourceThresholdsComparison::AtOrBelow);

        editor.update(&mut settings, Message::Edit(0));
        editor.update(&mut settings, low.clone());
        editor.update(&mut settings, Message::CancelRule);
        assert_eq!(settings, saved);

        editor.update(&mut settings, Message::Edit(0));
        editor.update(
            &mut settings,
            Message::Threshold(memory, false, "60".into()),
        );
        editor.update(&mut settings, low);
        editor.update(
            &mut settings,
            Message::Action(memory, true, ResourceThresholdsAction::Restart),
        );
        editor.update(&mut settings, Message::SaveRule);
        assert!(!editor.has_pending_editor());
        let rule = &settings.rules[0];
        assert_eq!(rule.cpu, cpu);
        assert_eq!(
            rule.memory.comparison,
            ResourceThresholdsComparison::AtOrBelow
        );
        assert_eq!(rule.memory.warning.threshold, 90);
        assert_eq!(rule.memory.critical.threshold, 60);
        assert_eq!(rule.memory.warning.action, ResourceThresholdsAction::None);
        assert_eq!(rule.memory.critical.action, ResourceThresholdsAction::None);

        editor.update(&mut settings, Message::Edit(0));
        editor.update(
            &mut settings,
            Message::MemoryUnit(ResourceThresholdsMemoryUnit::PrivateCommitMb),
        );
        editor.update(&mut settings, Message::MonitorEnabled(memory, false));
        editor.update(&mut settings, Message::SaveRule);
        editor.update(&mut settings, Message::Edit(0));
        editor.update(&mut settings, Message::MonitorEnabled(memory, true));
        editor.update(&mut settings, Message::SaveRule);
        assert!(!editor.has_pending_editor());
        let rule = &settings.rules[0];
        assert_eq!(rule.cpu, cpu);
        assert_eq!(
            rule.memory_unit,
            ResourceThresholdsMemoryUnit::PrivateCommitMb
        );
        assert!(rule.memory.enabled);
        assert_eq!(
            rule.memory.comparison,
            ResourceThresholdsComparison::AtOrBelow
        );
        assert_eq!(rule.memory.warning.threshold, 2000);
        assert_eq!(rule.memory.critical.threshold, 1000);
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
    fn monitor_disclosure_is_independent_of_tabs_and_enablement() {
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
            [false, false]
        );
        editor.update(&mut settings, Message::ToggleMonitorSettings(Cpu));
        editor.update(&mut settings, Message::MonitorEnabled(Cpu, false));
        editor.update(&mut settings, Message::Tab(Memory));
        editor.update(&mut settings, Message::Tab(Cpu));
        assert_eq!(
            editor.draft.as_ref().unwrap().monitor_settings_expanded,
            [true, false]
        );
        assert_eq!(settings.rules[0], rule);

        editor.update(&mut settings, Message::SaveRule);
        rule.cpu.enabled = false;
        assert_eq!(settings.rules[0], rule);
        editor.update(&mut settings, Message::Edit(0));
        assert_eq!(
            editor.draft.as_ref().unwrap().monitor_settings_expanded,
            [false, false]
        );
    }

    #[test]
    fn closing_and_reopening_a_rule_discards_its_pending_match_preview() {
        let mut editor = Editor::default();
        let mut settings = ResourceThresholdsSettings {
            rules: vec![ResourceThresholdsRule::new(r"C:\Apps\test.exe".into())],
            ..Default::default()
        };
        let saved = settings.clone();
        let previous = Arc::new(());
        editor.update(&mut settings, Message::Edit(0));
        editor.draft.as_mut().unwrap().match_preview = MatchPreview::Loading(previous.clone());

        editor.update(&mut settings, Message::CancelRule);
        editor.update(&mut settings, Message::Edit(0));
        let current = Arc::new(());
        editor.draft.as_mut().unwrap().match_preview = MatchPreview::Loading(current.clone());
        editor.complete_match_preview(previous, Err("obsolete".into()));
        assert!(matches!(
            &editor.draft.as_ref().unwrap().match_preview,
            MatchPreview::Loading(request) if Arc::ptr_eq(request, &current)
        ));

        editor.complete_match_preview(current, Ok(Vec::new()));
        assert!(matches!(
            &editor.draft.as_ref().unwrap().match_preview,
            MatchPreview::Loaded(Ok(rows)) if rows.is_empty()
        ));
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
    fn rule_duration_stays_in_the_draft_until_saved_and_can_inherit_again() {
        let mut editor = Editor::default();
        let mut settings = ResourceThresholdsSettings {
            rules: vec![ResourceThresholdsRule::new(r"C:\Apps\test.exe".into())],
            ..Default::default()
        };
        editor.update(&mut settings, Message::Edit(0));
        for value in ["0", "3601", "invalid", "18446744073709551616"] {
            editor.update(&mut settings, Message::RuleDuration(value.into()));
            editor.update(&mut settings, Message::SaveRule);
            assert_eq!(editor.draft.as_ref().unwrap().duration, value);
            assert_eq!(settings.rules[0].sustained_seconds, None);
        }
        editor.update(&mut settings, Message::RuleDuration("10".into()));
        editor.update(
            &mut settings,
            Message::Tab(ResourceThresholdsResource::Memory),
        );
        assert_eq!(settings.rules[0].sustained_seconds, None);
        editor.update(&mut settings, Message::SaveRule);
        assert_eq!(settings.rules[0].sustained_seconds, Some(10));
        assert_eq!(settings.sustained_seconds, 5);
        assert!(!editor.has_pending_editor());

        editor.update(&mut settings, Message::Edit(0));
        assert_eq!(editor.draft.as_ref().unwrap().duration, "10");
        editor.update(&mut settings, Message::RuleDuration(String::new()));
        editor.update(&mut settings, Message::CancelRule);
        assert_eq!(settings.rules[0].sustained_seconds, Some(10));
        editor.update(&mut settings, Message::Edit(0));
        editor.update(&mut settings, Message::RuleDuration(String::new()));
        editor.update(&mut settings, Message::SaveRule);
        assert_eq!(settings.rules[0].sustained_seconds, None);
        assert!(!editor.has_pending_editor());
    }

    #[test]
    fn cpu_memory_tab_transitions_keep_repaints_bounded() {
        use iced::advanced::{layout, renderer::Renderer as _, widget::Tree, Layout};
        use iced::{mouse, Rectangle, Size};
        use std::time::Instant;

        let mut rule = ResourceThresholdsRule::new(r"C:\Apps\test.exe".into());
        rule.memory.enabled = true;
        rule.memory_unit = ResourceThresholdsMemoryUnit::PrivateCommitMb;
        let mut settings = ResourceThresholdsSettings {
            rules: vec![rule],
            ..Default::default()
        };
        let mut editor = Editor::default();
        editor.update(&mut settings, Message::Edit(0));
        for resource in ResourceThresholdsResource::ALL {
            editor.update(&mut settings, Message::ToggleMonitorSettings(resource));
        }
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
                    "partial repaint left stale interior pixels on tab frame {frame}: {x},{y}"
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
            Message::MemoryUnit(ResourceThresholdsMemoryUnit::PrivateCommitMb),
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
        editor.update(&mut settings, Message::Tab(ResourceThresholdsResource::Cpu));
        editor.update(&mut settings, Message::SaveRule);
        assert!(!editor.has_pending_editor());
        assert!(settings.rules[0].cpu.enabled && settings.rules[0].memory.enabled);
        assert_eq!(
            settings.rules[0].memory_unit,
            ResourceThresholdsMemoryUnit::PrivateCommitMb
        );
        assert_eq!(settings.rules[0].cpu.warning.threshold, 65);
        assert_eq!(settings.rules[0].memory.warning.threshold, 1000);
        assert_eq!(settings.rules[0].memory.critical.threshold, 2000);
        assert_eq!(
            settings.rules[0].cpu.critical.action,
            ResourceThresholdsAction::None
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
            Message::MemoryUnit(ResourceThresholdsMemoryUnit::Percent),
        );
        editor.update(&mut settings, Message::SaveRule);
        assert_eq!(settings.rules[0].cpu.warning.threshold, 65);
        assert_eq!(settings.rules[0].memory.warning.threshold, 70);
        assert_eq!(settings.rules[0].memory.critical.threshold, 90);
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
