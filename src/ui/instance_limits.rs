use super::{app_picker, design, widgets};
use crate::{
    config::{InstanceLimitsRule, InstanceLimitsSettings},
    control::instance_limits::{paths_equal, validate_path},
};
use iced::{
    widget::{column, text},
    Element, Fill,
};
use rust_i18n::t;

#[derive(Default)]
pub(super) struct Editor {
    path: String,
    settings_expanded: bool,
    draft: Option<(Option<usize>, InstanceLimitsRule, String)>,
}

#[derive(Debug, Clone)]
pub(super) enum Message {
    Enabled(bool),
    ToggleSettings,
    Notify(bool),
    Path(String),
    Browse,
    Add,
    Edit(usize),
    Remove(usize),
    RuleEnabled(usize, bool),
    RuleNotify(usize, bool),
    Maximum(String),
    SaveRule,
    CancelRule,
}

impl Editor {
    fn can_add(settings: &InstanceLimitsSettings, path: &str) -> bool {
        InstanceLimitsRule::new(path.to_owned()).validate().is_ok()
            && settings
                .rules
                .iter()
                .all(|rule| matches!(paths_equal(&rule.executable_path, path), Ok(false)))
    }

    pub(super) fn has_pending_editor(&self) -> bool {
        self.draft.is_some()
    }

    fn validated_draft(&self) -> Option<InstanceLimitsRule> {
        let (_, rule, maximum) = self.draft.as_ref()?;
        let mut rule = rule.clone();
        rule.max_instances = maximum.parse().ok()?;
        rule.validate().ok()?;
        Some(rule)
    }

    pub(super) fn toggle_settings(&mut self) {
        self.settings_expanded = !self.settings_expanded;
    }

    pub(super) fn update(&mut self, settings: &mut InstanceLimitsSettings, message: Message) {
        match message {
            Message::ToggleSettings => self.toggle_settings(),
            Message::Enabled(value) => settings.enabled = value,
            Message::Notify(value) => settings.notify = value,
            Message::Path(path) => self.path = path,
            Message::Add if Self::can_add(settings, &self.path) => {
                self.draft = Some((None, InstanceLimitsRule::new(self.path.clone()), "1".into()));
            }
            Message::Edit(index) => {
                self.draft = settings.rules.get(index).cloned().map(|rule| {
                    let maximum = rule.max_instances.to_string();
                    (Some(index), rule, maximum)
                });
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
            Message::Maximum(value) => {
                if let Some((_, _, maximum)) = &mut self.draft {
                    *maximum = value;
                }
            }
            Message::SaveRule => {
                if let Some(rule) = self.validated_draft() {
                    if let Some((Some(index), _, _)) = &self.draft {
                        if let Some(target) = settings.rules.get_mut(*index) {
                            *target = rule;
                        }
                    } else if Self::can_add(settings, &rule.executable_path) {
                        settings.rules.push(rule);
                        self.path.clear();
                    }
                    self.draft = None;
                }
            }
            Message::CancelRule => self.draft = None,
            Message::Add | Message::Remove(_) | Message::Browse => {}
        }
    }

    pub(super) fn view<'a>(
        &'a self,
        settings: &'a InstanceLimitsSettings,
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
                        widgets::process_rule_header_with_notify(
                            &rule.executable_path,
                            candidates,
                            widgets::checkbox(rule.enabled)
                                .on_toggle(move |value| Message::RuleEnabled(index, value))
                                .into(),
                            widgets::rule_notify_checkbox(rule.notify, move |value| {
                                Message::RuleNotify(index, value)
                            }),
                            None,
                            vec![text(rule.max_instances.to_string()).into()],
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
        let can_add = Self::can_add(settings, &self.path);
        let mut picker = column![
            widgets::setting_title_with_help("common.rules", "", "instance_limits.rules_help"),
            app_picker::view(
                &self.path,
                candidates,
                true,
                Message::Path,
                Message::Browse,
                can_add.then_some(Message::Add),
                |path| Self::can_add(settings, path).then_some(true)
            ),
        ]
        .spacing(design::space::SMALL);
        if !can_add && app_picker::is_path_query(&self.path) {
            if validate_path(&self.path).is_err() {
                picker = picker.push(
                    text(t!("common.executable_path_format_hint").to_string()).style(text::danger),
                );
            } else if let Err(error) = InstanceLimitsRule::new(self.path.clone()).validate() {
                picker = picker
                    .push(text(t!("instance_limits.invalid").to_string()).style(text::danger))
                    .push(text(error).size(design::typography::SECONDARY));
            }
        }
        super::scrolling::scrollable(
            column![
                widgets::setting_group(
                    "instance_limits.enable".to_string(),
                    self.settings_expanded,
                    Message::ToggleSettings,
                    widgets::switch(settings.enabled, Some(Message::Enabled)),
                    column![widgets::setting_row(
                        "instance_limits.notify",
                        widgets::switch(settings.notify, Some(Message::Notify)),
                    )],
                ),
                widgets::settings_card(picker),
                widgets::process_rules_table_with_notify(
                    [t!("instance_limits.maximum").into_owned()],
                    rules,
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
        candidates: &[app_picker::Candidate],
    ) -> Option<Element<'a, Message>> {
        let (index, rule, maximum) = self.draft.as_ref()?;
        let valid = self.validated_draft().is_some();
        let mut fields = column![widgets::numeric_setting(
            "instance_limits.maximum",
            "",
            move || {
                widgets::stepper(maximum, 1..=u64::from(u32::MAX), 1, Some(Message::Maximum))
            }
        )]
        .spacing(design::space::SMALL);
        if !valid {
            fields =
                fields.push(text(t!("instance_limits.invalid").to_string()).style(text::danger));
        }
        let body = column![
            app_picker::app_name(&rule.executable_path, candidates),
            text(rule.executable_path.clone()).style(text::secondary),
            widgets::heading(
                t!("common.rule_details").to_string(),
                design::typography::BODY
            ),
            widgets::settings_card(fields),
        ]
        .spacing(widgets::CARD_GAP);
        Some(widgets::rule_dialog(
            t!(if index.is_some() {
                "instance_limits.edit_rule"
            } else {
                "instance_limits.add_rule"
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
    fn invalid_maximum_stays_in_draft_and_cancel_preserves_saved_rule() {
        let mut editor = Editor::default();
        let mut settings = InstanceLimitsSettings::default();
        editor.update(&mut settings, Message::Path(r"C:\Apps\app.exe".into()));
        editor.update(&mut settings, Message::Add);
        for value in ["0", "4294967296", "", "-1"] {
            editor.update(&mut settings, Message::Maximum(value.into()));
            editor.update(&mut settings, Message::SaveRule);
            assert!(settings.rules.is_empty());
            assert!(editor.has_pending_editor());
        }
        editor.update(&mut settings, Message::Maximum("2".into()));
        editor.update(&mut settings, Message::SaveRule);
        assert_eq!(settings.rules[0].max_instances, 2);
        assert!(!Editor::can_add(&settings, "c:/apps/APP.exe"));
        editor.update(&mut settings, Message::Edit(0));
        editor.update(&mut settings, Message::Maximum("3".into()));
        editor.update(&mut settings, Message::CancelRule);
        assert_eq!(settings.rules[0].max_instances, 2);
    }
}
