use std::path::Path;

use super::{app_picker, design, widgets};
use crate::{
    config::{KeepRunningRule, KeepRunningSettings},
    features::advanced_controls::keep_running::{KeepRunningSnapshot, KeepRunningState},
};
use iced::{
    widget::{column, row, text},
    Element, Fill,
};
use rust_i18n::t;

#[derive(Default)]
pub(super) struct Editor {
    path: String,
    draft: Option<(Option<usize>, KeepRunningRule)>,
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
    Arguments(String),
    WorkingDirectory(String),
    SaveRule,
    CancelRule,
    Resume(String),
}

impl Editor {
    fn can_add(settings: &KeepRunningSettings, path: &str) -> bool {
        super::process_rules::can_add_process_candidate(
            path,
            |path| {
                let key =
                    crate::foreground::executable_path_key(Path::new(path)).to_ascii_lowercase();
                settings.rules.iter().any(|rule| {
                    crate::foreground::executable_path_key(Path::new(&rule.executable_path))
                        .to_ascii_lowercase()
                        == key
                })
            },
            |name| name.eq_ignore_ascii_case("winderust.exe"),
        ) && KeepRunningRule::new(path.trim().to_owned())
            .validate()
            .is_ok()
    }

    pub(super) fn has_pending_editor(&self) -> bool {
        self.draft.is_some()
    }

    pub(super) fn update(&mut self, settings: &mut KeepRunningSettings, message: Message) {
        match message {
            Message::Enabled(value) => settings.enabled = value,
            Message::Path(path) => self.path = path,
            Message::Add if Self::can_add(settings, &self.path) => {
                self.draft = Some((
                    None,
                    KeepRunningRule::new(crate::foreground::executable_path_key(Path::new(
                        self.path.trim(),
                    ))),
                ));
            }
            Message::Edit(index) => {
                self.draft = settings
                    .rules
                    .get(index)
                    .cloned()
                    .map(|rule| (Some(index), rule))
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
            Message::Arguments(value) => {
                if let Some((_, rule)) = &mut self.draft {
                    rule.arguments = value;
                }
            }
            Message::WorkingDirectory(value) => {
                if let Some((_, rule)) = &mut self.draft {
                    rule.working_directory = value;
                }
            }
            Message::SaveRule => {
                if let Some((index, rule)) = &self.draft {
                    if rule.validate().is_ok() {
                        if let Some(index) = index {
                            if let Some(target) = settings.rules.get_mut(*index) {
                                *target = rule.clone();
                            }
                        } else {
                            settings.rules.push(rule.clone());
                            self.path.clear();
                        }
                        self.draft = None;
                    }
                }
            }
            Message::CancelRule => self.draft = None,
            Message::Add | Message::Remove(_) | Message::Browse | Message::Resume(_) => {}
        }
    }

    pub(super) fn view<'a>(
        &'a self,
        settings: &'a KeepRunningSettings,
        status: &'a KeepRunningSnapshot,
        candidates: &'a [app_picker::Candidate],
    ) -> Element<'a, Message> {
        let rules = settings
            .rules
            .iter()
            .enumerate()
            .map(|(index, rule)| {
                let runtime = status.rule_status(rule);
                let state = if !settings.enabled || !rule.enabled {
                    Some(KeepRunningState::Disabled)
                } else {
                    runtime.map(|entry| entry.state)
                };
                let state_label = state.map_or("common.unknown", KeepRunningState::label_key);
                let header = widgets::process_rule_header(
                    &rule.executable_path,
                    candidates,
                    widgets::checkbox(rule.enabled)
                        .on_toggle(move |value| Message::RuleEnabled(index, value))
                        .into(),
                    Some(
                        text(t!(state_label).to_string())
                            .size(design::typography::SECONDARY)
                            .into(),
                    ),
                    Vec::new(),
                    row![
                        widgets::tooltip(
                            widgets::icon_button("icons/pencil.svg").on_press(Message::Edit(index)),
                            text(t!("common.edit").to_string()),
                            iced::widget::tooltip::Position::Top
                        ),
                        widgets::rule_delete_button(Some(Message::Remove(index)))
                    ]
                    .spacing(design::space::SMALL)
                    .into(),
                );
                let mut details = column![].spacing(design::space::SMALL);
                if let Some(runtime) = runtime {
                    details = details.push(
                        text(t!("keep_running.attempts", count = runtime.attempts).to_string())
                            .size(design::typography::SECONDARY),
                    );
                    if let Some(seconds) = runtime.next_retry_seconds {
                        details = details.push(
                            text(t!("keep_running.next_retry", seconds = seconds).to_string())
                                .size(design::typography::SECONDARY),
                        );
                    }
                    if let Some(error) = &runtime.last_error {
                        details = details.push(text(error).style(text::danger));
                    }
                }
                if matches!(state, Some(KeepRunningState::Paused(_))) {
                    details = details.push(
                        widgets::action_button(t!("keep_running.resume").to_string())
                            .on_press(Message::Resume(rule.executable_path.clone())),
                    );
                }
                (
                    widgets::stable_key(&rule.executable_path),
                    column![
                        header,
                        iced::widget::container(details).padding(iced::Padding {
                            top: 0.0,
                            right: widgets::CARD_PADDING as f32,
                            bottom: widgets::CARD_PADDING as f32,
                            left: widgets::CARD_PADDING as f32,
                        }),
                        widgets::settings_divider(),
                    ]
                    .into(),
                )
            })
            .collect();
        let mut body = column![
            widgets::toggle_card(
                "keep_running.enable",
                settings.enabled,
                Some(Message::Enabled)
            ),
            widgets::setting_title("common.rules"),
            app_picker::view(
                &self.path,
                candidates,
                true,
                Message::Path,
                Message::Browse,
                Self::can_add(settings, &self.path).then_some(Message::Add),
                |path| Self::can_add(settings, path).then_some(true)
            ),
            widgets::process_rules_table_with_actions(
                [],
                rules,
                t!("keep_running.empty").to_string(),
                80,
                true
            ),
        ]
        .spacing(widgets::CARD_GAP);
        if settings.enabled && !status.observation_known {
            body = body.push(
                text(t!("keep_running.observation_unavailable").to_string()).style(text::secondary),
            );
        }
        if let Some(error) = &status.last_error {
            body = body.push(text(error).style(text::danger));
        }
        super::scrolling::scrollable(body).height(Fill).into()
    }

    pub(super) fn modal<'a>(
        &'a self,
        candidates: &[app_picker::Candidate],
    ) -> Option<Element<'a, Message>> {
        let (index, rule) = self.draft.as_ref()?;
        let valid = rule.validate().is_ok();
        let mut body = column![
            app_picker::app_name(&rule.executable_path, candidates),
            text(rule.executable_path.clone()).style(text::secondary),
            widgets::heading(
                t!("keep_running.launch_settings").to_string(),
                design::typography::BODY
            ),
            widgets::settings_card(
                column![
                    widgets::setting_title("keep_running.arguments"),
                    widgets::text_input("", &rule.arguments).on_input(Message::Arguments),
                    widgets::settings_divider(),
                    widgets::setting_title("keep_running.working_directory"),
                    widgets::text_input("", &rule.working_directory)
                        .on_input(Message::WorkingDirectory),
                ]
                .spacing(design::space::SMALL)
            ),
            text(t!("keep_running.launch_help").to_string()).size(design::typography::SECONDARY),
        ]
        .spacing(widgets::CARD_GAP);
        if !valid {
            body = body.push(text(t!("keep_running.invalid").to_string()).style(text::danger));
        }
        Some(widgets::modal_frame(
            row![
                widgets::heading(
                    t!(if index.is_some() {
                        "keep_running.edit_rule"
                    } else {
                        "keep_running.add_rule"
                    })
                    .to_string(),
                    design::typography::SECTION
                )
                .width(Fill),
                widgets::icon_button("icons/x.svg").on_press(Message::CancelRule)
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
            (760, 620),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drafts_prefill_explicit_cwd_preserve_literal_arguments_and_cancel_without_changes() {
        let mut editor = Editor::default();
        let mut settings = KeepRunningSettings::default();
        editor.update(&mut settings, Message::Path(r"C:\Apps\app.exe".into()));
        editor.update(&mut settings, Message::Add);
        assert_eq!(
            editor.draft.as_ref().unwrap().1.working_directory,
            r"C:\Apps"
        );
        editor.update(
            &mut settings,
            Message::Arguments(r#"--file "literal value""#.into()),
        );
        editor.update(&mut settings, Message::SaveRule);
        assert_eq!(settings.rules[0].arguments, r#"--file "literal value""#);
        assert!(!Editor::can_add(&settings, "c:/apps/APP.exe"));
        editor.update(&mut settings, Message::Edit(0));
        editor.update(&mut settings, Message::WorkingDirectory("relative".into()));
        editor.update(&mut settings, Message::SaveRule);
        assert!(editor.has_pending_editor());
        editor.update(&mut settings, Message::CancelRule);
        assert_eq!(settings.rules[0].working_directory, r"C:\Apps");
    }
}
