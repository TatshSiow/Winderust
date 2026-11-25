use super::process_rules::{can_add_exact_process_candidate, exact_path_input, picked_path_input};
use super::{app_picker, design, widgets};
use crate::{
    config::{KeepRunningRule, KeepRunningSettings},
    features::watchdog::keep_running::{KeepRunningSnapshot, KeepRunningState},
    foreground::process_list::normalize_path_slashes,
};
use iced::{
    widget::{column, text},
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
    RuleNotify(usize, bool),
    Arguments(String),
    WorkingDirectory(String),
    SaveRule,
    CancelRule,
    Resume(String),
}

impl Editor {
    fn addable_rule(settings: &KeepRunningSettings, path: &str) -> Option<KeepRunningRule> {
        if !can_add_exact_process_candidate(
            path,
            |path| {
                let key = normalize_path_slashes(path);
                settings.rules.iter().any(|rule| {
                    normalize_path_slashes(&rule.executable_path).eq_ignore_ascii_case(&key)
                })
            },
            |name| name.eq_ignore_ascii_case("winderust.exe"),
        ) {
            return None;
        }
        let rule = KeepRunningRule::new(normalize_path_slashes(path).into_owned());
        rule.validate().ok()?;
        Some(rule)
    }

    pub(super) fn has_pending_editor(&self) -> bool {
        self.draft.is_some()
    }

    pub(super) fn update(&mut self, settings: &mut KeepRunningSettings, message: Message) {
        match message {
            Message::Enabled(value) => settings.enabled = value,
            Message::Path(path) => self.path = path,
            Message::Add => {
                if let Some(rule) = Self::addable_rule(settings, exact_path_input(&self.path)) {
                    self.draft = Some((None, rule));
                }
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
            Message::RuleNotify(index, notify) => {
                if let Some(rule) = settings.rules.get_mut(index) {
                    rule.notify = notify;
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
            Message::Remove(_) | Message::Browse | Message::Resume(_) => {}
        }
    }

    pub(super) fn view<'a>(
        &'a self,
        settings: &'a KeepRunningSettings,
        status: &'a KeepRunningSnapshot,
        candidates: &'a [app_picker::Candidate],
        settings_are_pending: bool,
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
                let header = widgets::process_rule_header_with_notify(
                    &rule.executable_path,
                    candidates,
                    widgets::checkbox(rule.enabled)
                        .on_toggle(move |value| Message::RuleEnabled(index, value))
                        .into(),
                    widgets::rule_notify_checkbox(rule.notify, move |value| {
                        Message::RuleNotify(index, value)
                    }),
                    Some(
                        text(t!(state_label).to_string())
                            .size(design::typography::SECONDARY)
                            .into(),
                    ),
                    Vec::new(),
                    widgets::rule_actions(Some(Message::Edit(index)), Some(Message::Remove(index))),
                );
                let mut row = column![header];
                if let Some(runtime) = runtime {
                    let mut details = column![text(
                        t!("keep_running.attempts", count = runtime.attempts).to_string()
                    )
                    .size(design::typography::SECONDARY)]
                    .spacing(design::space::SMALL);
                    if let Some(seconds) = runtime.next_retry_seconds {
                        details = details.push(
                            text(t!("keep_running.next_retry", seconds = seconds).to_string())
                                .size(design::typography::SECONDARY),
                        );
                    }
                    if let Some(error) = &runtime.last_error {
                        details = details.push(text(error).style(text::danger));
                    }
                    if matches!(state, Some(KeepRunningState::Paused(_))) {
                        let resume = widgets::action_button(t!("keep_running.resume").to_string())
                            .on_press_maybe(
                                (!settings_are_pending)
                                    .then(|| Message::Resume(rule.executable_path.clone())),
                            );
                        details = if settings_are_pending {
                            details.push(widgets::tooltip(
                                resume,
                                text(t!("keep_running.save_before_resume").to_string())
                                    .width(240)
                                    .wrapping(text::Wrapping::WordOrGlyph),
                                iced::widget::tooltip::Position::Top,
                            ))
                        } else {
                            details.push(resume)
                        };
                    }
                    row = row.push(iced::widget::container(details).padding(iced::Padding {
                        top: 0.0,
                        right: widgets::CARD_PADDING as f32,
                        bottom: widgets::CARD_PADDING as f32,
                        left: widgets::CARD_PADDING as f32,
                    }));
                }
                (
                    widgets::stable_key(&rule.executable_path),
                    row.push(widgets::settings_divider()).into(),
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
            widgets::settings_card(app_picker::view_with_path_selection(
                &self.path,
                candidates,
                true,
                (Message::Path, |path| Message::Path(picked_path_input(path))),
                Message::Browse,
                Self::addable_rule(settings, exact_path_input(&self.path))
                    .is_some()
                    .then_some(Message::Add),
                |path| Self::addable_rule(settings, path).is_some().then_some(true)
            )),
            widgets::process_rules_table_with_notify([], rules, true),
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
        let mut fields = widgets::settings_rows([
            column![
                widgets::setting_title("keep_running.arguments"),
                widgets::text_input("", &rule.arguments).on_input(Message::Arguments),
            ]
            .spacing(design::space::SMALL)
            .into(),
            column![
                widgets::setting_title("keep_running.working_directory"),
                widgets::text_input("", &rule.working_directory)
                    .on_input(Message::WorkingDirectory),
            ]
            .spacing(design::space::SMALL)
            .into(),
        ])
        .push(
            text(t!("keep_running.launch_help").to_string())
                .size(design::typography::SECONDARY)
                .style(text::secondary),
        );
        if !valid {
            fields = fields.push(text(t!("keep_running.invalid").to_string()).style(text::danger));
        }
        let body = column![
            app_picker::app_name(&rule.executable_path, candidates),
            text(rule.executable_path.clone()).style(text::secondary),
            widgets::heading(
                t!("keep_running.launch_settings").to_string(),
                design::typography::BODY
            ),
            widgets::settings_card(fields),
        ]
        .spacing(widgets::CARD_GAP);
        Some(widgets::rule_dialog(
            t!(if index.is_some() {
                "keep_running.edit_rule"
            } else {
                "keep_running.add_rule"
            })
            .to_string(),
            super::scrolling::scrollable_pane(body).height(Fill),
            Message::CancelRule,
            valid.then_some(Message::SaveRule),
            (760, 460),
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
        editor.update(
            &mut settings,
            Message::Path(picked_path_input(concat!(r"C:\Apps\app.exe", " ").into())),
        );
        editor.update(&mut settings, Message::Add);
        assert!(!editor.has_pending_editor());
        editor.update(&mut settings, Message::SaveRule);
        assert!(settings.rules.is_empty());
        editor.update(&mut settings, Message::Path(" C:\\Apps\\app.exe ".into()));
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
        assert!(Editor::addable_rule(&settings, "c:/apps/APP.exe").is_none());
        editor.update(&mut settings, Message::Edit(0));
        editor.update(&mut settings, Message::WorkingDirectory("relative".into()));
        editor.update(&mut settings, Message::SaveRule);
        assert!(editor.has_pending_editor());
        editor.update(&mut settings, Message::CancelRule);
        assert_eq!(settings.rules[0].working_directory, r"C:\Apps");
    }
}
