use super::{app_picker, design, widgets};
use crate::config::{SleepPreventionRule, SleepPreventionSettings};
use crate::control::instance_limits::validate_path;
use crate::sleep_prevention::SleepPreventionSnapshot;
use crate::ui::process_rules::{can_add_process_candidate, process_setting_matches};
use iced::{
    widget::{column, text},
    Element, Fill,
};
use rust_i18n::t;

#[derive(Default)]
pub(super) struct Editor {
    pub(super) path: String,
}

#[derive(Debug, Clone)]
pub(super) enum Message {
    Enabled(bool),
    Path(String),
    Browse,
    Add,
    RuleEnabled(usize, bool),
    KeepDisplayOn(usize, bool),
    Remove(usize),
}

impl Editor {
    fn can_add(settings: &SleepPreventionSettings, path: &str) -> bool {
        validate_path(path.trim()).is_ok()
            && can_add_process_candidate(
                path,
                |path| settings.contains_rule_for(path),
                crate::features::power_plan_control::by_running_app::is_builtin_excluded,
            )
    }

    pub(super) fn update(&mut self, settings: &mut SleepPreventionSettings, message: Message) {
        match message {
            Message::Enabled(value) => settings.enabled = value,
            Message::Path(value) => self.path = value,
            Message::Add if Self::can_add(settings, &self.path) => {
                settings.rules.push(SleepPreventionRule {
                    executable_path: crate::foreground::executable_path_key(std::path::Path::new(
                        self.path.trim(),
                    )),
                    enabled: true,
                    keep_display_on: false,
                });
                self.path.clear();
            }
            Message::RuleEnabled(index, value) => {
                if let Some(rule) = settings.rules.get_mut(index) {
                    rule.enabled = value;
                }
            }
            Message::KeepDisplayOn(index, value) => {
                if let Some(rule) = settings.rules.get_mut(index) {
                    rule.keep_display_on = value;
                }
            }
            Message::Remove(index) if index < settings.rules.len() => {
                settings.rules.remove(index);
            }
            Message::Add | Message::Browse | Message::Remove(_) => {}
        }
    }

    pub(super) fn view<'a>(
        &'a self,
        settings: &'a SleepPreventionSettings,
        status: &SleepPreventionSnapshot,
        candidates: &'a [app_picker::Candidate],
    ) -> Element<'a, Message> {
        let rules = settings
            .rules
            .iter()
            .enumerate()
            .map(|(index, rule)| {
                let header = widgets::process_rule_header(
                    &rule.executable_path,
                    candidates,
                    widgets::checkbox(rule.enabled)
                        .on_toggle(move |value| Message::RuleEnabled(index, value))
                        .into(),
                    Some(
                        text(indicator(status, rule))
                            .size(design::typography::SECONDARY)
                            .into(),
                    ),
                    vec![widgets::checkbox(rule.keep_display_on)
                        .on_toggle(move |value| Message::KeepDisplayOn(index, value))
                        .into()],
                    iced::widget::container(widgets::rule_delete_button(Some(Message::Remove(
                        index,
                    ))))
                    .center_x(80)
                    .into(),
                );
                let mut row = column![header];
                if validate_path(rule.executable_path.trim()).is_err() {
                    row = row.push(app_picker::path_readd_hint());
                }
                (
                    widgets::stable_key(&rule.executable_path),
                    row.push(widgets::settings_divider()).into(),
                )
            })
            .collect();
        let can_add = Self::can_add(settings, &self.path);
        let mut picker = column![app_picker::view(
            &self.path,
            candidates,
            true,
            Message::Path,
            Message::Browse,
            can_add.then_some(Message::Add),
            |path| Self::can_add(settings, path).then_some(true),
        )]
        .spacing(design::space::SMALL);
        if !can_add
            && app_picker::is_path_query(&self.path)
            && validate_path(self.path.trim()).is_err()
        {
            picker = picker.push(app_picker::path_format_hint());
        }
        super::scrolling::scrollable(
            column![
                widgets::toggle_card(
                    "sleep_prevention.enable",
                    settings.enabled,
                    Some(Message::Enabled)
                ),
                widgets::setting_title("common.rules"),
                picker,
                widgets::process_rules_table_with_actions(
                    [t!("sleep_prevention.keep_display_on").to_string()],
                    rules,
                    t!("sleep_prevention.no_rules").to_string(),
                    80,
                    true,
                ),
            ]
            .spacing(widgets::CARD_GAP),
        )
        .height(Fill)
        .into()
    }
}

fn indicator(status: &SleepPreventionSnapshot, rule: &SleepPreventionRule) -> String {
    let contains = |paths: &[String]| {
        paths
            .iter()
            .any(|path| process_setting_matches(path, &rule.executable_path))
    };
    let key = if !status.enabled || !rule.enabled {
        "common.off"
    } else if !status.observation_known {
        "common.unknown"
    } else if status.system_requested && contains(&status.requesting_apps) {
        "sleep_prevention.request_accepted"
    } else if contains(&status.suppressed_apps) {
        "sleep_prevention.requests_paused"
    } else if contains(&status.running_apps) {
        "common.waiting"
    } else {
        "sleep_prevention.not_running"
    };
    t!(key).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adding_a_rule_requires_a_full_drive_or_network_path() {
        let mut settings = SleepPreventionSettings::default();
        let mut editor = Editor::default();
        for path in [
            r"C:\Apps\.\worker.exe",
            r"C:\Apps\child\..\worker.exe",
            r"\\?\C:\Apps\worker.exe",
            r" C:\Apps\worker.exe ",
            r" \\server\share\worker.exe ",
        ] {
            editor.update(&mut settings, Message::Path(path.into()));
            editor.update(&mut settings, Message::Add);
        }
        let paths: Vec<_> = settings
            .rules
            .iter()
            .map(|rule| rule.executable_path.as_str())
            .collect();
        assert_eq!(paths, [r"C:\Apps\worker.exe", r"\\server\share\worker.exe"]);
    }

    #[test]
    fn rule_status_distinguishes_accepted_requests_and_sleep_suppression() {
        let rule = SleepPreventionRule {
            executable_path: r"C:\Apps\test.exe".into(),
            enabled: true,
            keep_display_on: false,
        };
        let mut status = SleepPreventionSnapshot {
            enabled: true,
            observation_known: true,
            running_apps: vec![rule.executable_path.clone()],
            requesting_apps: vec![rule.executable_path.clone()],
            ..Default::default()
        };
        assert_eq!(indicator(&status, &rule), t!("common.waiting"));
        status.system_requested = true;
        assert_eq!(
            indicator(&status, &rule),
            t!("sleep_prevention.request_accepted")
        );
        status.observation_known = false;
        assert_eq!(indicator(&status, &rule), t!("common.unknown"));
        status.observation_known = true;
        status.suppressed_apps.push(rule.executable_path.clone());
        assert_eq!(
            indicator(&status, &rule),
            t!("sleep_prevention.request_accepted")
        );
        status.requesting_apps.clear();
        assert_eq!(
            indicator(&status, &rule),
            t!("sleep_prevention.requests_paused")
        );
    }
}
