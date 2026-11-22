use std::{path::Path, sync::Arc};

use super::{app_picker, design, process_rules::MatchBy, widgets};
use crate::config::{
    WatchdogAction, WatchdogComparison, WatchdogMemoryUnit, WatchdogMetric, WatchdogResource,
    WatchdogRule, WatchdogScope, WatchdogSelector, WatchdogSettings, WATCHDOG_DURATION_RANGE,
};
use crate::foreground::{
    executable_path_key, process_list::same_executable_name, same_executable_path,
};
use iced::{
    widget::{column, row, text, Column},
    Element, Fill,
};
use rust_i18n::t;

mod preview;

impl std::fmt::Display for WatchdogMetric {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&t!(self.label_key()))
    }
}
impl std::fmt::Display for WatchdogMemoryUnit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        std::fmt::Display::fmt(&self.metric(), f)
    }
}

impl std::fmt::Display for WatchdogAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&t!(match self {
            Self::None => "watchdog.no_action",
            Self::Terminate => "watchdog.terminate",
            Self::Restart => "watchdog.restart",
        }))
    }
}

impl std::fmt::Display for WatchdogComparison {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&t!(match self {
            Self::AtOrAbove => "watchdog.at_or_above",
            Self::AtOrBelow => "watchdog.at_or_below",
        }))
    }
}

impl std::fmt::Display for WatchdogScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&t!(match self {
            Self::Combined => "watchdog.scope_combined",
            Self::PerInstance => "watchdog.scope_per_instance",
        }))
    }
}

#[derive(Default)]
pub(super) struct Editor {
    match_by: MatchBy,
    path: String,
    name: String,
    duration: Option<String>,
    draft: Option<Draft>,
}

struct Draft {
    index: Option<usize>,
    rule: WatchdogRule,
    duration: String,
    tab: DraftTab,
    monitors: [ThresholdInputs; 2],
    match_preview: MatchPreview,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum DraftTab {
    Monitor(WatchdogResource),
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
    fn new(index: Option<usize>, rule: WatchdogRule) -> Self {
        Self {
            index,
            duration: rule
                .sustained_seconds
                .map_or_else(String::new, |seconds| seconds.to_string()),
            tab: DraftTab::Monitor(if rule.cpu.enabled {
                WatchdogResource::Cpu
            } else {
                WatchdogResource::Memory
            }),
            monitors: WatchdogResource::ALL.map(|resource| {
                let monitor = rule.monitor(resource);
                ThresholdInputs {
                    warning: monitor.warning.threshold.to_string(),
                    critical: monitor.critical.threshold.to_string(),
                }
            }),
            rule,
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

    fn validated_rule(&self) -> Option<WatchdogRule> {
        let mut rule = self.rule.clone();
        rule.sustained_seconds = self.parsed_duration()?;
        for (resource, raw) in WatchdogResource::ALL.into_iter().zip(&self.monitors) {
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

    fn clear_unavailable_actions(&mut self, resource: WatchdogResource) {
        if !self.rule.allows_actions(resource) {
            let monitor = self.rule.monitor_mut(resource);
            monitor.warning.action = WatchdogAction::None;
            monitor.critical.action = WatchdogAction::None;
        }
    }
}

#[derive(Debug, Clone)]
pub(super) enum Message {
    Enabled(bool),
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
    Tab(WatchdogResource),
    PreviewMatches,
    MatchPreviewLoaded(Arc<()>, Result<Vec<preview::MatchRow>, String>),
    Scope(WatchdogScope),
    MonitorEnabled(WatchdogResource, bool),
    Comparison(WatchdogResource, WatchdogComparison),
    MemoryUnit(WatchdogMemoryUnit),
    Duration(String),
    RuleDuration(String),
    Threshold(WatchdogResource, bool, String),
    Notify(bool),
    Action(WatchdogResource, bool, WatchdogAction),
}

impl Editor {
    fn can_add_path(settings: &WatchdogSettings, path: &str) -> bool {
        super::process_rules::can_add_process_candidate(
            path,
            |path| {
                settings.rules.iter().any(|rule| {
                    matches!(
                        &rule.selector,
                        WatchdogSelector::ExactPath { executable_path }
                            if same_executable_path(Path::new(executable_path), Path::new(path))
                    )
                })
            },
            |name| name.eq_ignore_ascii_case("winderust.exe"),
        )
    }

    fn new_name_selector(&self, settings: &WatchdogSettings) -> Option<WatchdogSelector> {
        let name = self.name.trim();
        if name.eq_ignore_ascii_case("winderust.exe")
            || settings.rules.iter().any(|rule| {
                matches!(
                    &rule.selector,
                    WatchdogSelector::ExactName { executable_name }
                        if same_executable_name(executable_name, name)
                )
            })
        {
            return None;
        }
        let selector = WatchdogSelector::ExactName {
            executable_name: name.into(),
        };
        selector.validate().ok()?;
        Some(selector)
    }

    fn new_rule(&self, settings: &WatchdogSettings) -> Option<WatchdogRule> {
        match self.match_by {
            MatchBy::ExecutablePath => Self::can_add_path(settings, &self.path)
                .then(|| WatchdogRule::new(executable_path_key(Path::new(self.path.trim())))),
            MatchBy::ExecutableName => self
                .new_name_selector(settings)
                .map(WatchdogRule::with_selector),
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

    pub(super) fn select_tab(&mut self, resource: WatchdogResource) {
        if let Some(draft) = &mut self.draft {
            draft.tab = DraftTab::Monitor(resource);
        }
    }

    pub(super) fn preview_matches(&mut self) -> iced::Task<Message> {
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
        super::tasks::run(move || preview::load(&selector)).map(move |result| {
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

    pub(super) fn update(&mut self, settings: &mut WatchdogSettings, message: Message) {
        match message {
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
                                WatchdogSelector::ExactPath { .. } => self.path.clear(),
                                WatchdogSelector::ExactName { .. } => self.name.clear(),
                            }
                            settings.rules.push(rule);
                        }
                        self.draft = None;
                    }
                }
            }
            Message::CancelRule => self.draft = None,
            Message::Tab(resource) => self.select_tab(resource),
            Message::Remove(index) if index < settings.rules.len() => {
                settings.rules.remove(index);
                self.draft = None;
            }
            Message::RuleEnabled(index, value) => {
                if let Some(rule) = settings.rules.get_mut(index) {
                    rule.enabled = value;
                }
            }
            Message::Scope(scope) => {
                if let Some(draft) = &mut self.draft {
                    draft.rule.scope = scope;
                    for resource in WatchdogResource::ALL {
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
                            WatchdogMemoryUnit::Percent => (70, 90),
                            WatchdogMemoryUnit::Megabytes | WatchdogMemoryUnit::PrivateCommitMb => {
                                (1000, 2000)
                            }
                        };
                        let memory = &mut draft.rule.memory;
                        let (warning, critical) = match memory.comparison {
                            WatchdogComparison::AtOrAbove => (warning, critical),
                            WatchdogComparison::AtOrBelow => (critical, warning),
                        };
                        memory.warning.threshold = warning;
                        memory.critical.threshold = critical;
                        draft.monitors[WatchdogResource::Memory as usize] = ThresholdInputs {
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
        settings: &'a WatchdogSettings,
        candidates: &'a [app_picker::Candidate],
    ) -> Element<'a, Message> {
        let rules = settings
            .rules
            .iter()
            .enumerate()
            .map(|(index, rule)| {
                let value = rule.selector.value();
                let target = match &rule.selector {
                    WatchdogSelector::ExactPath { executable_path } => executable_path.clone(),
                    WatchdogSelector::ExactName { .. } => t!("common.any_path").to_string(),
                };
                let target = match rule.scope {
                    WatchdogScope::Combined => target,
                    WatchdogScope::PerInstance => format!("{} · {target}", rule.scope),
                };
                (
                    widgets::stable_key(&format!("{index}:{value}")),
                    column![
                        widgets::process_rule_header_with_target(
                            app_picker::app_name(value, candidates),
                            target,
                            widgets::checkbox(rule.enabled)
                                .on_toggle(move |value| Message::RuleEnabled(index, value))
                                .into(),
                            None,
                            WatchdogResource::ALL
                                .into_iter()
                                .map(|resource| {
                                    let monitor = rule.monitor(resource);
                                    let warning = monitor.warning.threshold;
                                    let critical = monitor.critical.threshold;
                                    let comparison = match monitor.comparison {
                                        WatchdogComparison::AtOrAbove => "",
                                        WatchdogComparison::AtOrBelow => monitor.comparison.symbol(),
                                    };
                                    text(if !monitor.enabled {
                                        t!("common.off").to_string()
                                    } else if resource == WatchdogResource::Memory {
                                        let metric = rule.metric(resource);
                                        format!("{metric}\n{comparison}{warning} / {comparison}{critical}")
                                    } else {
                                        format!("{comparison}{warning}% / {comparison}{critical}%")
                                    })
                                    .size(design::typography::SECONDARY)
                                    .into()
                                })
                                .collect(),
                            row![
                                super::widgets::tooltip(
                                    widgets::icon_button("icons/pencil.svg")
                                        .on_press(Message::Edit(index)),
                                    text(t!("common.edit").to_string()),
                                    iced::widget::tooltip::Position::Top,
                                ),
                                widgets::rule_delete_button(Some(Message::Remove(index)))
                            ]
                            .spacing(design::space::SMALL)
                            .into(),
                        ),
                        widgets::settings_divider(),
                    ]
                    .into(),
                )
            })
            .collect();
        let mut duration = column![widgets::setting_row_with_unit(
            "watchdog.duration",
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
            duration = duration
                .push(text(t!("watchdog.invalid_duration").to_string()).style(text::danger));
        }
        super::scrolling::scrollable(
            column![
                widgets::toggle_card("watchdog.enable", settings.enabled, Some(Message::Enabled)),
                widgets::heading(t!("watchdog.shared").to_string(), design::typography::BODY),
                widgets::settings_card(widgets::settings_rows([
                    duration.into(),
                    widgets::setting_row(
                        "watchdog.notify",
                        widgets::switch(settings.notify, Some(Message::Notify)),
                    )
                    .into(),
                ])),
                widgets::setting_title("watchdog.rules"),
                self.add_rule_card(settings, candidates),
                widgets::process_rules_table_with_actions(
                    ["watchdog.cpu_tab", "watchdog.memory_tab"].map(|key| t!(key).to_string()),
                    rules,
                    t!("watchdog.empty").to_string(),
                    80,
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
        settings: &WatchdogSettings,
        candidates: &'a [app_picker::Candidate],
    ) -> Element<'a, Message> {
        let target = match self.match_by {
            MatchBy::ExecutablePath => app_picker::view(
                &self.path,
                candidates,
                true,
                Message::Path,
                Message::Browse,
                Self::can_add_path(settings, &self.path).then_some(Message::Add),
                |path| Self::can_add_path(settings, path).then_some(true),
            ),
            MatchBy::ExecutableName => {
                let add = self.new_name_selector(settings).map(|_| Message::Add);
                column![
                    widgets::setting_title("common.executable_name"),
                    row![
                        widgets::text_input("worker.exe", &self.name)
                            .on_input(Message::Name)
                            .on_submit_maybe(add.clone())
                            .width(Fill),
                        widgets::action_button(t!("common.add").to_string())
                            .style(widgets::primary_button)
                            .on_press_maybe(add),
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
                .width(180),
            )
            .into(),
            target,
        ]))
        .into()
    }

    fn monitor_details(draft: &Draft, resource: WatchdogResource) -> Column<'_, Message> {
        let rule = &draft.rule;
        let monitor = rule.monitor(resource);
        let mut details = column![widgets::toggle_card(
            match resource {
                WatchdogResource::Cpu => "watchdog.monitor_cpu",
                WatchdogResource::Memory => "watchdog.monitor_memory",
            },
            monitor.enabled,
            Some(move |enabled| Message::MonitorEnabled(resource, enabled)),
        )]
        .spacing(widgets::CARD_GAP);
        if !monitor.enabled {
            return details;
        }
        if resource == WatchdogResource::Memory {
            details = details
                .push(widgets::heading(
                    t!("watchdog.measurement").to_string(),
                    design::typography::BODY,
                ))
                .push(widgets::settings_card(widgets::setting_row(
                    "watchdog.memory_metric",
                    widgets::pick_list(
                        WatchdogMemoryUnit::ALL,
                        Some(rule.memory_unit),
                        Message::MemoryUnit,
                    )
                    .width(180),
                )));
        }
        let raw = &draft.monitors[resource as usize];
        let metric = rule.metric(resource);
        for (critical, response, threshold) in [
            (false, &monitor.warning, &raw.warning),
            (true, &monitor.critical, &raw.critical),
        ] {
            let threshold = widgets::setting_row_with_unit(
                "watchdog.threshold",
                metric.unit(),
                widgets::text_input("", threshold)
                    .on_input(move |value| Message::Threshold(resource, critical, value))
                    .on_submit(Message::SaveRule)
                    .width(design::STANDALONE_NUMERIC_WIDTH),
            )
            .into();
            let action = rule.allows_actions(resource).then(|| {
                widgets::setting_row(
                    "watchdog.action",
                    widgets::pick_list(
                        [
                            WatchdogAction::None,
                            WatchdogAction::Terminate,
                            WatchdogAction::Restart,
                        ],
                        Some(response.action),
                        move |value| Message::Action(resource, critical, value),
                    )
                    .width(180),
                )
                .into()
            });
            details = details
                .push(widgets::heading(
                    format!(
                        "{} · {}",
                        metric,
                        t!(if critical {
                            "watchdog.critical"
                        } else {
                            "watchdog.warning"
                        })
                    ),
                    design::typography::BODY,
                ))
                .push(widgets::settings_card(widgets::settings_rows(
                    std::iter::once(threshold).chain(action),
                )));
        }
        details
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
                return details.push(text(t!("watchdog.preview_loading").to_string()));
            }
            MatchPreview::Loaded(Err(error)) => {
                return details.push(
                    text(t!("watchdog.preview_failed", error = error).to_string())
                        .style(text::danger)
                        .wrapping(text::Wrapping::WordOrGlyph),
                );
            }
            MatchPreview::Loaded(Ok(matches)) => matches,
        };

        let mut matched = Vec::new();
        let mut same_name = Vec::new();
        let mut unavailable = Vec::new();
        for entry in matches {
            let (rows, reason) = match entry.kind {
                MatchKind::Matched => (&mut matched, None),
                MatchKind::SameName => (&mut same_name, None),
                MatchKind::Unavailable(reason) => (&mut unavailable, Some(reason)),
            };
            let path = entry.path.as_ref().map_or_else(
                || t!(UnavailableReason::PathUnavailable.label_key()).to_string(),
                |path| path.display().to_string(),
            );
            let mut metadata = column![text(path)
                .style(text::secondary)
                .wrapping(text::Wrapping::WordOrGlyph)]
            .spacing(design::space::TINY)
            .width(Fill);
            if let Some(reason) = reason.filter(|_| entry.path.is_some()) {
                metadata = metadata.push(
                    text(t!(reason.label_key()).to_string())
                        .size(design::typography::CAPTION)
                        .style(text::secondary)
                        .wrapping(text::Wrapping::WordOrGlyph),
                );
            }
            rows.push(
                row![
                    text(t!("watchdog.preview_pid", pid = entry.pid).to_string()).width(112),
                    metadata,
                ]
                .spacing(design::space::MEDIUM)
                .width(Fill)
                .into(),
            );
        }
        if matched.is_empty() {
            details = details
                .push(text(t!("watchdog.preview_no_matches").to_string()).style(text::secondary));
        }
        for (rows, heading) in [
            (matched, "watchdog.preview_matches"),
            (same_name, "watchdog.preview_same_name"),
            (unavailable, "watchdog.preview_unavailable"),
        ] {
            if rows.is_empty() {
                continue;
            }
            details = details.push(widgets::settings_card(
                column![
                    widgets::heading(t!(heading).to_string(), design::typography::BODY),
                    Column::with_children(rows).spacing(design::space::SMALL),
                ]
                .spacing(design::space::MEDIUM),
            ));
        }
        details
    }

    pub(super) fn modal<'a>(
        &'a self,
        candidates: &[app_picker::Candidate],
    ) -> Option<Element<'a, Message>> {
        let draft = self.draft.as_ref()?;
        let rule = &draft.rule;
        let target = match &rule.selector {
            WatchdogSelector::ExactPath { executable_path } => executable_path.clone(),
            WatchdogSelector::ExactName { .. } => t!("watchdog.name_rule_mode").to_string(),
        };
        let duration_valid = draft.parsed_duration().is_some();
        let valid = duration_valid && draft.validated_rule().is_some();
        let mut duration = column![widgets::setting_row_with_unit(
            "watchdog.rule_duration",
            "s",
            widgets::text_input(&t!("common.default"), &draft.duration)
                .on_input(Message::RuleDuration)
                .on_submit(Message::SaveRule)
                .width(design::STANDALONE_NUMERIC_WIDTH),
        )]
        .spacing(design::space::SMALL)
        .width(280);
        if !duration_valid {
            duration = duration
                .push(text(t!("watchdog.invalid_duration").to_string()).style(text::danger));
        }
        let validation = (duration_valid && !valid).then(|| {
            text(t!("watchdog.invalid").to_string())
                .style(text::danger)
                .into()
        });
        let (details, scroll_id) = match draft.tab {
            DraftTab::Monitor(resource) => (
                Self::monitor_details(draft, resource),
                "watchdog-rule-details",
            ),
            DraftTab::MatchPreview => {
                (Self::match_preview_details(draft), "watchdog-match-preview")
            }
        };
        let comparison = match draft.tab {
            DraftTab::Monitor(resource) if rule.monitor(resource).enabled => Some(
                row![
                    widgets::tooltip(
                        text(t!("watchdog.comparison").to_string()).style(text::secondary),
                        text(t!("watchdog.comparison_help").to_string()).width(280),
                        iced::widget::tooltip::Position::Top,
                    ),
                    widgets::pick_list(
                        [WatchdogComparison::AtOrAbove, WatchdogComparison::AtOrBelow],
                        Some(rule.monitor(resource).comparison),
                        move |value| Message::Comparison(resource, value),
                    )
                    .width(180),
                ]
                .spacing(design::space::SMALL)
                .align_y(iced::Center),
            ),
            _ => None,
        };
        Some(widgets::modal_frame(
            row![
                widgets::heading(
                    t!(if draft.index.is_some() {
                        "watchdog.edit_rule"
                    } else {
                        "watchdog.add_rule"
                    })
                    .to_string(),
                    design::typography::SECTION,
                )
                .width(Fill),
                widgets::icon_button("icons/x.svg").on_press(Message::CancelRule),
            ]
            .align_y(iced::Center),
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
                    "watchdog.scope",
                    widgets::pick_list(
                        [WatchdogScope::Combined, WatchdogScope::PerInstance],
                        Some(rule.scope),
                        Message::Scope,
                    )
                    .width(180),
                )),
                row![
                    iced::widget::container(row![
                        widgets::panel_tab(
                            t!("watchdog.cpu_tab").to_string(),
                            draft.tab == DraftTab::Monitor(WatchdogResource::Cpu),
                            Message::Tab(WatchdogResource::Cpu)
                        ),
                        widgets::panel_tab(
                            t!("watchdog.memory_tab").to_string(),
                            draft.tab == DraftTab::Monitor(WatchdogResource::Memory),
                            Message::Tab(WatchdogResource::Memory)
                        ),
                        widgets::panel_tab(
                            t!("watchdog.match_preview").to_string(),
                            draft.tab == DraftTab::MatchPreview,
                            Message::PreviewMatches
                        ),
                    ])
                    .style(widgets::tab_bar),
                    iced::widget::Space::new().width(Fill)
                ]
                .extend(comparison.map(Into::into))
                .spacing(design::space::SMALL)
                .align_y(iced::Center),
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
            row![
                iced::widget::Space::new().width(Fill),
                widgets::action_button(t!("common.cancel").to_string())
                    .style(widgets::tertiary_button)
                    .on_press(Message::CancelRule),
                widgets::action_button(t!("common.ok").to_string())
                    .style(widgets::primary_button)
                    .on_press_maybe(valid.then_some(Message::SaveRule)),
            ]
            .spacing(design::space::SMALL),
            (760, 840),
        ))
    }
}

fn parse_duration(value: &str) -> Option<u64> {
    value
        .parse()
        .ok()
        .filter(|value| WATCHDOG_DURATION_RANGE.contains(value))
}

fn response(
    rule: &mut crate::config::WatchdogMonitor,
    critical: bool,
) -> &mut crate::config::WatchdogResponse {
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
        let mut settings = WatchdogSettings::default();
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
            Message::Action(WatchdogResource::Cpu, false, WatchdogAction::Terminate),
        );
        editor.update(
            &mut settings,
            Message::Action(WatchdogResource::Cpu, true, WatchdogAction::Restart),
        );
        editor.update(&mut settings, Message::SaveRule);
        assert!(!editor.has_pending_editor());
        assert_eq!(
            settings.rules[0],
            WatchdogRule::with_selector(WatchdogSelector::ExactName {
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
        assert_eq!(settings.rules[1], WatchdogRule::new(path.into()));
    }

    #[test]
    fn per_instance_scope_clears_draft_actions_and_rejects_action_messages() {
        let mut rule = WatchdogRule::new(r"C:\Apps\worker.exe".into());
        for resource in WatchdogResource::ALL {
            let monitor = rule.monitor_mut(resource);
            monitor.warning.action = WatchdogAction::Terminate;
            monitor.critical.action = WatchdogAction::Restart;
        }
        let mut settings = WatchdogSettings {
            rules: vec![rule.clone()],
            ..Default::default()
        };
        let mut editor = Editor::default();
        editor.update(&mut settings, Message::Edit(0));
        editor.update(&mut settings, Message::Scope(WatchdogScope::PerInstance));
        assert_eq!(settings.rules[0], rule);

        editor.update(&mut settings, Message::CancelRule);
        assert_eq!(settings.rules[0], rule);
        editor.update(&mut settings, Message::Edit(0));
        editor.update(&mut settings, Message::Scope(WatchdogScope::PerInstance));
        for resource in WatchdogResource::ALL {
            editor.update(
                &mut settings,
                Message::Action(resource, true, WatchdogAction::Restart),
            );
        }
        editor.update(&mut settings, Message::SaveRule);
        assert!(!editor.has_pending_editor());
        assert_eq!(settings.rules[0].scope, WatchdogScope::PerInstance);
        for resource in WatchdogResource::ALL {
            let monitor = settings.rules[0].monitor(resource);
            assert_eq!(monitor.warning.action, WatchdogAction::None);
            assert_eq!(monitor.critical.action, WatchdogAction::None);
        }
    }

    #[test]
    fn low_usage_comparison_edits_only_its_monitor() {
        let mut rule = WatchdogRule::new(r"C:\Apps\test.exe".into());
        rule.memory.enabled = true;
        rule.cpu.critical.action = WatchdogAction::Restart;
        rule.memory.warning.action = WatchdogAction::Terminate;
        let cpu = rule.cpu.clone();
        let mut settings = WatchdogSettings {
            rules: vec![rule],
            ..Default::default()
        };
        let saved = settings.clone();
        let mut editor = Editor::default();
        let memory = WatchdogResource::Memory;
        let low = Message::Comparison(memory, WatchdogComparison::AtOrBelow);

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
            Message::Action(memory, true, WatchdogAction::Restart),
        );
        editor.update(&mut settings, Message::SaveRule);
        assert!(!editor.has_pending_editor());
        let rule = &settings.rules[0];
        assert_eq!(rule.cpu, cpu);
        assert_eq!(rule.memory.comparison, WatchdogComparison::AtOrBelow);
        assert_eq!(rule.memory.warning.threshold, 90);
        assert_eq!(rule.memory.critical.threshold, 60);
        assert_eq!(rule.memory.warning.action, WatchdogAction::None);
        assert_eq!(rule.memory.critical.action, WatchdogAction::None);

        editor.update(&mut settings, Message::Edit(0));
        editor.update(
            &mut settings,
            Message::MemoryUnit(WatchdogMemoryUnit::PrivateCommitMb),
        );
        editor.update(&mut settings, Message::MonitorEnabled(memory, false));
        editor.update(&mut settings, Message::SaveRule);
        editor.update(&mut settings, Message::Edit(0));
        editor.update(&mut settings, Message::MonitorEnabled(memory, true));
        editor.update(&mut settings, Message::SaveRule);
        assert!(!editor.has_pending_editor());
        let rule = &settings.rules[0];
        assert_eq!(rule.cpu, cpu);
        assert_eq!(rule.memory_unit, WatchdogMemoryUnit::PrivateCommitMb);
        assert!(rule.memory.enabled);
        assert_eq!(rule.memory.comparison, WatchdogComparison::AtOrBelow);
        assert_eq!(rule.memory.warning.threshold, 2000);
        assert_eq!(rule.memory.critical.threshold, 1000);
    }

    #[test]
    fn disabled_monitor_keeps_valid_thresholds_and_ignores_incomplete_pairs() {
        let mut rule = WatchdogRule::new(r"C:\Apps\worker.exe".into());
        rule.cpu.enabled = false;
        rule.memory.enabled = true;
        let mut draft = Draft::new(None, rule);
        for (warning, expected) in [("65", 65), ("", 70), ("90", 70)] {
            draft.monitors[WatchdogResource::Cpu as usize].warning = warning.into();
            let saved = draft
                .validated_rule()
                .expect("memory monitor remains valid");
            assert!(!saved.cpu.enabled);
            assert_eq!(saved.cpu.warning.threshold, expected);
            assert_eq!(saved.cpu.critical.threshold, 90);
        }
    }

    #[test]
    fn closing_and_reopening_a_rule_discards_its_pending_match_preview() {
        let mut editor = Editor::default();
        let mut settings = WatchdogSettings {
            rules: vec![WatchdogRule::new(r"C:\Apps\test.exe".into())],
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
        let mut settings = WatchdogSettings::default();
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
        let mut settings = WatchdogSettings {
            rules: vec![WatchdogRule::new(r"C:\Apps\test.exe".into())],
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
        editor.update(&mut settings, Message::Tab(WatchdogResource::Memory));
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

        let mut rule = WatchdogRule::new(r"C:\Apps\test.exe".into());
        rule.memory.enabled = true;
        rule.memory_unit = WatchdogMemoryUnit::PrivateCommitMb;
        let mut settings = WatchdogSettings {
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
                Message::Tab(WatchdogResource::ALL[frame % 2]),
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
            eprintln!("Watchdog tab {frame}: layout={laid_out:?}, draw={:?}, raster={:?}, total={:?}, regions={}",
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
        let mut settings = WatchdogSettings::default();
        editor.update(&mut settings, Message::Path(r"C:\Apps\test.exe".into()));
        editor.update(&mut settings, Message::Add);
        assert!(editor.has_pending_editor());
        assert!(settings.rules.is_empty());
        editor.update(&mut settings, Message::CancelRule);
        assert!(settings.rules.is_empty());
        editor.update(&mut settings, Message::Add);
        editor.update(
            &mut settings,
            Message::MonitorEnabled(WatchdogResource::Memory, true),
        );
        editor.update(&mut settings, Message::Tab(WatchdogResource::Memory));
        editor.update(
            &mut settings,
            Message::MemoryUnit(WatchdogMemoryUnit::PrivateCommitMb),
        );
        editor.update(
            &mut settings,
            Message::Threshold(WatchdogResource::Cpu, false, "65".into()),
        );
        editor.update(
            &mut settings,
            Message::Action(WatchdogResource::Memory, true, WatchdogAction::Restart),
        );
        editor.update(&mut settings, Message::Tab(WatchdogResource::Cpu));
        editor.update(&mut settings, Message::SaveRule);
        assert!(!editor.has_pending_editor());
        assert!(settings.rules[0].cpu.enabled && settings.rules[0].memory.enabled);
        assert_eq!(
            settings.rules[0].memory_unit,
            WatchdogMemoryUnit::PrivateCommitMb
        );
        assert_eq!(settings.rules[0].cpu.warning.threshold, 65);
        assert_eq!(settings.rules[0].memory.warning.threshold, 1000);
        assert_eq!(settings.rules[0].memory.critical.threshold, 2000);
        assert_eq!(settings.rules[0].cpu.critical.action, WatchdogAction::None);
        assert_eq!(
            settings.rules[0].memory.critical.action,
            WatchdogAction::Restart
        );
        let saved = settings.clone();
        editor.update(&mut settings, Message::Edit(0));
        editor.update(
            &mut settings,
            Message::Threshold(WatchdogResource::Memory, true, "".into()),
        );
        editor.update(&mut settings, Message::SaveRule);
        assert!(editor.has_pending_editor());
        assert_eq!(settings, saved);
        editor.update(&mut settings, Message::CancelRule);
        assert_eq!(settings, saved);
        editor.update(&mut settings, Message::Edit(0));
        editor.update(
            &mut settings,
            Message::MemoryUnit(WatchdogMemoryUnit::Percent),
        );
        editor.update(&mut settings, Message::SaveRule);
        assert_eq!(settings.rules[0].cpu.warning.threshold, 65);
        assert_eq!(settings.rules[0].memory.warning.threshold, 70);
        assert_eq!(settings.rules[0].memory.critical.threshold, 90);
        editor.update(&mut settings, Message::Edit(0));
        editor.update(
            &mut settings,
            Message::MonitorEnabled(WatchdogResource::Cpu, false),
        );
        editor.update(
            &mut settings,
            Message::MonitorEnabled(WatchdogResource::Memory, false),
        );
        editor.update(&mut settings, Message::SaveRule);
        assert!(editor.has_pending_editor());
        editor.update(&mut settings, Message::CancelRule);
        editor.update(&mut settings, Message::Path(r"C:\Apps\test.exe".into()));
        editor.update(&mut settings, Message::Add);
        assert!(!editor.has_pending_editor());
    }
}
