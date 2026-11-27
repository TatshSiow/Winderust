use super::{app_picker, design, widgets};
use crate::{
    action_log::ActionLogResult,
    automation::{InstanceBalancingDetail, InstanceBalancingSnapshot},
    config::{InstanceBalancingRule, InstanceBalancingSettings},
    control::instance_limits::{paths_equal, validate_path},
};
use iced::{
    widget::{column, container, row, text, tooltip},
    Element, Fill,
};
use rust_i18n::t;
use std::sync::Arc;

#[derive(Default)]
pub(super) struct Editor {
    path: String,
    draft: Option<(usize, String)>,
    browse_request: Option<Arc<()>>,
}

#[derive(Debug, Clone)]
pub(super) enum Message {
    Enabled(bool),
    Path(String),
    Browse,
    Add,
    Edit(usize),
    Remove(usize),
    RuleEnabled(usize, bool),
    SaveRule,
    CancelRule,
}

impl Editor {
    pub(super) fn has_pending_editor(&self) -> bool {
        self.draft.is_some()
    }

    pub(super) fn begin_browse(&mut self) -> Arc<()> {
        let request = Arc::new(());
        self.browse_request = Some(Arc::clone(&request));
        request
    }

    pub(super) fn take_browse(&mut self, request: &Arc<()>) -> bool {
        self.browse_request
            .take_if(|active| Arc::ptr_eq(active, request))
            .is_some()
    }

    pub(super) fn update(&mut self, settings: &mut InstanceBalancingSettings, message: Message) {
        match message {
            Message::Enabled(value) => settings.enabled = value,
            Message::Path(path) => {
                self.browse_request = None;
                if let Some((_, draft)) = &mut self.draft {
                    *draft = path;
                } else {
                    self.path = path;
                }
            }
            Message::Add
                if self.draft.is_none() && path_error(settings, &self.path, None).is_none() =>
            {
                self.browse_request = None;
                settings.rules.push(InstanceBalancingRule {
                    enabled: true,
                    executable_path: std::mem::take(&mut self.path),
                });
            }
            Message::Edit(index) => {
                self.browse_request = None;
                self.draft = settings
                    .rules
                    .get(index)
                    .map(|rule| (index, rule.executable_path.clone()));
            }
            Message::Remove(index) if index < settings.rules.len() => {
                settings.rules.remove(index);
                self.draft = None;
                self.browse_request = None;
            }
            Message::RuleEnabled(index, enabled) => {
                if let Some(rule) = settings.rules.get_mut(index) {
                    rule.enabled = enabled;
                }
            }
            Message::SaveRule => {
                if let Some((index, path)) = &self.draft {
                    if path_error(settings, path, Some(*index)).is_none() {
                        if let Some(rule) = settings.rules.get_mut(*index) {
                            rule.executable_path.clone_from(path);
                            self.draft = None;
                            self.browse_request = None;
                        }
                    }
                }
            }
            Message::CancelRule => {
                self.draft = None;
                self.browse_request = None;
            }
            Message::Add | Message::Remove(_) | Message::Browse => {}
        }
    }

    pub(super) fn view<'a>(
        &'a self,
        settings: &'a InstanceBalancingSettings,
        status: &'a InstanceBalancingSnapshot,
        master_enabled: bool,
        candidates: &'a [app_picker::Candidate],
    ) -> Element<'a, Message> {
        let rules = settings
            .rules
            .iter()
            .enumerate()
            .map(|(index, rule)| {
                let cell = status_cell(
                    rule.enabled && settings.enabled && master_enabled,
                    status.rule_result(rule),
                );
                (
                    widgets::stable_key(&rule.executable_path),
                    column![
                        widgets::process_rule_header(
                            &rule.executable_path,
                            candidates,
                            widgets::checkbox(rule.enabled)
                                .on_toggle(move |value| Message::RuleEnabled(index, value))
                                .into(),
                            Some(cell),
                            Vec::new(),
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
        let is_path = app_picker::is_path_query(&self.path);
        let error = is_path
            .then(|| path_error(settings, &self.path, None))
            .flatten();
        let mut picker = column![
            widgets::help_text("instance_balancing.rules_help"),
            app_picker::view(
                &self.path,
                candidates,
                true,
                Message::Path,
                Message::Browse,
                (is_path && error.is_none()).then_some(Message::Add),
                |path| path_error(settings, path, None).is_none().then_some(true),
            ),
        ]
        .spacing(design::space::SMALL);
        if let Some(error) = error {
            picker = picker.push(text(error).style(text::danger));
        }
        super::scrolling::scrollable(
            column![
                widgets::toggle_card(
                    "instance_balancing.enable",
                    settings.enabled,
                    Some(Message::Enabled),
                ),
                widgets::settings_section(
                    "common.rules",
                    column![
                        widgets::settings_card(picker),
                        widgets::process_rules_table([], rules, true),
                    ]
                    .spacing(widgets::CARD_GAP),
                ),
            ]
            .spacing(widgets::CARD_GAP),
        )
        .height(Fill)
        .into()
    }

    pub(super) fn modal<'a>(
        &'a self,
        settings: &InstanceBalancingSettings,
        candidates: &[app_picker::Candidate],
    ) -> Option<Element<'a, Message>> {
        let (index, path) = self.draft.as_ref()?;
        let error = path_error(settings, path, Some(*index));
        let valid = error.is_none();
        let mut fields = column![
            widgets::setting_title("process_list.executable_path"),
            row![
                widgets::text_input("", path)
                    .on_input(Message::Path)
                    .on_submit(Message::SaveRule)
                    .width(Fill),
                widgets::action_button(t!("common.browse_local_executable").to_string())
                    .style(widgets::tertiary_button)
                    .on_press(Message::Browse),
            ]
            .spacing(design::space::SMALL)
            .align_y(iced::Center),
        ]
        .spacing(design::space::SMALL);
        if let Some(error) = error {
            fields = fields.push(text(error).style(text::danger));
        }
        let body = column![
            app_picker::app_name(path, candidates),
            widgets::setting_title("common.rule_details"),
            widgets::settings_card(fields),
        ]
        .spacing(widgets::CARD_GAP);
        Some(widgets::rule_dialog(
            t!("instance_balancing.edit_rule").to_string(),
            super::scrolling::scrollable_pane(body).height(Fill),
            Message::CancelRule,
            valid.then_some(Message::SaveRule),
            (760, 460),
        ))
    }
}

fn status_cell(
    enabled: bool,
    result: Option<(ActionLogResult, &InstanceBalancingDetail)>,
) -> Element<'_, Message> {
    let key = rule_status_key(enabled, result);
    let chip = container(text(t!(key).to_string()))
        .padding([4, 8])
        .style(move |theme| widgets::rule_status_chip(theme, key));
    if let Some((_, detail)) = result.filter(|_| enabled) {
        widgets::tooltip(
            chip,
            text(detail.message(&[]))
                .width(240)
                .wrapping(text::Wrapping::WordOrGlyph),
            tooltip::Position::Top,
        )
        .into()
    } else {
        chip.into()
    }
}

fn rule_status_key(
    enabled: bool,
    result: Option<(ActionLogResult, &InstanceBalancingDetail)>,
) -> &'static str {
    use InstanceBalancingDetail::{
        Assigned, Diagnostic, InsufficientCapacity, WaitingForInstances, WaitingForStableCohort,
    };

    if !enabled {
        return "common.inactive";
    }
    match result {
        Some((ActionLogResult::Applied, Assigned(_))) => "common.applied",
        Some((ActionLogResult::Failed, _)) => "common.error",
        Some((_, WaitingForInstances | WaitingForStableCohort | InsufficientCapacity)) => {
            "common.waiting"
        }
        Some((_, Diagnostic(_))) => "common.unavailable",
        _ => "common.unknown",
    }
}

fn path_error(
    settings: &InstanceBalancingSettings,
    path: &str,
    editing: Option<usize>,
) -> Option<String> {
    if editing.is_some_and(|index| index >= settings.rules.len()) {
        return Some(t!("instance_balancing.invalid").to_string());
    }
    if validate_path(path).is_err() {
        return Some(t!("common.executable_path_format_hint").to_string());
    }
    for (index, rule) in settings.rules.iter().enumerate() {
        if Some(index) != editing {
            match paths_equal(&rule.executable_path, path) {
                Ok(true) => return Some(t!("instance_balancing.conflict").to_string()),
                Err(error) => {
                    return Some(
                        t!("common.executable_path_check_failed", error = error).to_string(),
                    )
                }
                Ok(false) => {}
            }
        }
    }
    None
}

#[cfg(test)]
mod tests;
