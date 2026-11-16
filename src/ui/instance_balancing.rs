use super::{app_picker, design, widgets};
use crate::{
    config::{InstanceBalancingRule, InstanceBalancingSettings},
    control::instance_limits::{paths_equal, validate_path},
};
use iced::{
    widget::{column, row, text},
    Element, Fill,
};
use rust_i18n::t;

#[derive(Default)]
pub(super) struct Editor {
    path: String,
    draft: Option<(usize, String)>,
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

    pub(super) fn update(&mut self, settings: &mut InstanceBalancingSettings, message: Message) {
        match message {
            Message::Enabled(value) => settings.enabled = value,
            Message::Path(path) => {
                if let Some((_, draft)) = &mut self.draft {
                    *draft = path;
                } else {
                    self.path = path;
                }
            }
            Message::Add
                if self.draft.is_none() && path_error(settings, &self.path, None).is_none() =>
            {
                settings.rules.push(InstanceBalancingRule {
                    enabled: true,
                    executable_path: std::mem::take(&mut self.path),
                });
            }
            Message::Edit(index) => {
                self.draft = settings
                    .rules
                    .get(index)
                    .map(|rule| (index, rule.executable_path.clone()));
            }
            Message::Remove(index) if index < settings.rules.len() => {
                settings.rules.remove(index);
                self.draft = None;
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
                        }
                    }
                }
            }
            Message::CancelRule => self.draft = None,
            Message::Add | Message::Remove(_) | Message::Browse => {}
        }
    }

    pub(super) fn view<'a>(
        &'a self,
        settings: &'a InstanceBalancingSettings,
        candidates: &'a [app_picker::Candidate],
    ) -> Element<'a, Message> {
        let rules = settings
            .rules
            .iter()
            .enumerate()
            .map(|(index, rule)| {
                (
                    widgets::stable_key(&rule.executable_path),
                    column![
                        widgets::process_rule_header(
                            &rule.executable_path,
                            candidates,
                            widgets::checkbox(rule.enabled)
                                .on_toggle(move |value| Message::RuleEnabled(index, value))
                                .into(),
                            None,
                            Vec::new(),
                            row![
                                widgets::tooltip(
                                    widgets::icon_button("icons/pencil.svg")
                                        .on_press(Message::Edit(index)),
                                    text(t!("common.edit").to_string()),
                                    iced::widget::tooltip::Position::Top,
                                ),
                                widgets::rule_delete_button(Some(Message::Remove(index))),
                            ]
                            .spacing(design::space::SMALL)
                            .width(80)
                            .align_y(iced::Center)
                            .into(),
                        ),
                        widgets::settings_divider(),
                    ]
                    .into(),
                )
            })
            .collect();
        let mut picker = column![app_picker::view(
            &self.path,
            candidates,
            true,
            Message::Path,
            Message::Browse,
            path_error(settings, &self.path, None)
                .is_none()
                .then_some(Message::Add),
            |path| path_error(settings, path, None).is_none().then_some(true),
        )]
        .spacing(design::space::SMALL);
        if !self.path.is_empty() {
            if let Some(error) = path_error(settings, &self.path, None) {
                picker = picker
                    .push(text(t!("instance_balancing.invalid").to_string()).style(text::danger))
                    .push(text(error).size(design::typography::SECONDARY));
            }
        }
        super::scrolling::scrollable(
            column![
                widgets::toggle_card(
                    "instance_balancing.enable",
                    settings.enabled,
                    Some(Message::Enabled),
                ),
                widgets::setting_title("common.rules"),
                picker,
                widgets::process_rules_table_with_actions(
                    [],
                    rules,
                    t!("instance_balancing.empty").to_string(),
                    80,
                    false,
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
        let mut body = column![
            app_picker::app_name(path, candidates),
            widgets::setting_title("process_list.executable_path"),
            widgets::text_input("", path)
                .on_input(Message::Path)
                .on_submit(Message::SaveRule),
            widgets::action_button(t!("common.browse_local_executable").to_string())
                .style(widgets::tertiary_button)
                .on_press(Message::Browse),
        ]
        .spacing(widgets::CARD_GAP);
        if let Some(error) = error {
            body = body
                .push(text(t!("instance_balancing.invalid").to_string()).style(text::danger))
                .push(text(error).size(design::typography::SECONDARY));
        }
        Some(widgets::modal_frame(
            row![
                widgets::heading(
                    t!("instance_balancing.edit_rule").to_string(),
                    design::typography::SECTION,
                )
                .width(Fill),
                widgets::icon_button("icons/x.svg").on_press(Message::CancelRule),
            ]
            .align_y(iced::Center),
            super::scrolling::scrollable_pane(body).height(Fill),
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
            (760, 460),
        ))
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
    if let Err(error) = validate_path(path) {
        return Some(error);
    }
    for (index, rule) in settings.rules.iter().enumerate() {
        if Some(index) != editing {
            match paths_equal(&rule.executable_path, path) {
                Ok(true) => return Some(t!("instance_balancing.conflict").to_string()),
                Err(error) => return Some(error),
                Ok(false) => {}
            }
        }
    }
    None
}

#[cfg(test)]
mod tests;
