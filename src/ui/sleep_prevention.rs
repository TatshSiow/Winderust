use super::{app_picker, design, widgets};
use crate::application::SettingsEditor;
use crate::config::{same_rule_executable_path, SleepPreventionRule, SleepPreventionSettings};
use crate::control::instance_limits::validate_path;
use crate::foreground::process_list::normalize_path_slashes;
use crate::sleep_prevention::SleepPreventionSnapshot;
use crate::ui::process_rules::{
    can_add_exact_process_candidate, exact_path_input, picked_path_input,
};
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
        validate_path(path).is_ok()
            && can_add_exact_process_candidate(
                path,
                |path| settings.contains_rule_for(path),
                crate::features::power_plan_control::by_running_app::is_builtin_excluded,
            )
    }

    pub(super) fn update(&mut self, settings: &mut SettingsEditor, message: Message) {
        let current = &settings.sleep_prevention;
        match message {
            Message::Enabled(value) if current.enabled != value => {
                settings.sleep_prevention.enabled = value;
            }
            Message::Path(value) => self.path = value,
            Message::Add => {
                let path = exact_path_input(&self.path);
                if Self::can_add(current, path) {
                    settings.sleep_prevention.rules.push(SleepPreventionRule {
                        executable_path: normalize_path_slashes(path).into_owned(),
                        enabled: true,
                        keep_display_on: false,
                    });
                    self.path.clear();
                }
            }
            Message::RuleEnabled(index, value)
                if current
                    .rules
                    .get(index)
                    .is_some_and(|rule| rule.enabled != value) =>
            {
                settings.sleep_prevention.rules[index].enabled = value;
            }
            Message::KeepDisplayOn(index, value)
                if current
                    .rules
                    .get(index)
                    .is_some_and(|rule| rule.keep_display_on != value) =>
            {
                settings.sleep_prevention.rules[index].keep_display_on = value;
            }
            Message::Remove(index) if index < current.rules.len() => {
                settings.sleep_prevention.rules.remove(index);
            }
            Message::Enabled(_)
            | Message::Browse
            | Message::RuleEnabled(..)
            | Message::KeepDisplayOn(..)
            | Message::Remove(_) => {}
        }
    }

    pub(super) fn view<'a>(
        &'a self,
        settings: &'a SleepPreventionSettings,
        status: Option<&SleepPreventionSnapshot>,
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
        let path = exact_path_input(&self.path);
        let can_add = Self::can_add(settings, path);
        let mut picker = column![app_picker::view_with_path_selection(
            &self.path,
            candidates,
            true,
            (Message::Path, |path| Message::Path(picked_path_input(path))),
            Message::Browse,
            can_add.then_some(Message::Add),
            |path| Self::can_add(settings, path).then_some(true),
        )]
        .spacing(design::space::SMALL);
        if !can_add && app_picker::is_path_query(&self.path) && validate_path(path).is_err() {
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

fn indicator(status: Option<&SleepPreventionSnapshot>, rule: &SleepPreventionRule) -> String {
    if !rule.enabled {
        return t!("common.off").to_string();
    }
    let Some(status) = status else {
        return t!("common.unknown").to_string();
    };
    let matches = |path: &str| same_rule_executable_path(&rule.executable_path, path);
    let contains = |paths: &[String]| paths.iter().any(|path| matches(path));
    let accepted_request = || {
        if !status.system_requested {
            return None;
        }
        status
            .requesting_apps
            .iter()
            .find(|request| matches(&request.path))
    };
    let key = if !status.enabled {
        "common.off"
    } else if !status.observation_known {
        "common.unknown"
    } else if let Some(request) = accepted_request() {
        let display_expected = request.keep_display_on || rule.keep_display_on;
        let display_accepted = request.keep_display_on && status.display_requested;
        if display_expected && !display_accepted {
            "sleep_prevention.system_request_accepted"
        } else {
            "sleep_prevention.request_accepted"
        }
    } else if contains(&status.suppressed_apps) {
        "sleep_prevention.requests_paused"
    } else if contains(&status.running_apps) {
        "common.waiting"
    } else if status.observed_rule_paths.as_deref().is_some_and(contains) {
        "sleep_prevention.not_running"
    } else {
        "common.unknown"
    };
    t!(key).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{PowerSourceProfile, Settings};
    use crate::sleep_prevention::RequestingApp;

    #[test]
    fn adding_a_rule_requires_a_full_drive_or_network_path() {
        let mut settings = SettingsEditor::with_settings(Settings::default());
        settings.select_power_source(PowerSourceProfile::OnBattery);
        let mut editor = Editor::default();
        for path in [
            r"C:\Apps\.\worker.exe",
            r"C:\Apps\child\..\worker.exe",
            r"\\?\C:\Apps\worker.exe",
            r#""C:\Apps\worker.exe ""#,
        ] {
            editor.update(&mut settings, Message::Path(path.into()));
            editor.update(&mut settings, Message::Add);
        }
        assert!(!settings.has_unsaved_changes());
        for path in [
            r" C:\Apps\worker.exe ",
            "C:\\Apps\\worker.exe\u{3000}",
            r" \\server\share\worker.exe ",
        ] {
            editor.update(&mut settings, Message::Path(path.into()));
            editor.update(&mut settings, Message::Add);
        }
        let paths: Vec<_> = settings
            .sleep_prevention
            .rules
            .iter()
            .map(|rule| rule.executable_path.as_str())
            .collect();
        assert_eq!(
            paths,
            [
                r"C:\Apps\worker.exe",
                "C:\\Apps\\worker.exe\u{3000}",
                r"\\server\share\worker.exe"
            ]
        );
    }

    #[test]
    fn rule_status_distinguishes_accepted_requests_and_sleep_suppression() {
        let mut rule = SleepPreventionRule {
            executable_path: r"C:\Apps\test.exe".into(),
            enabled: true,
            keep_display_on: false,
        };
        let mut status = SleepPreventionSnapshot {
            enabled: true,
            observation_known: true,
            running_apps: vec![rule.executable_path.clone()],
            requesting_apps: vec![RequestingApp {
                path: rule.executable_path.as_str().into(),
                keep_display_on: false,
            }],
            ..Default::default()
        };
        assert_eq!(indicator(None, &rule), t!("common.unknown"));
        assert_eq!(indicator(Some(&status), &rule), t!("common.waiting"));
        status.system_requested = true;
        assert_eq!(
            indicator(Some(&status), &rule),
            t!("sleep_prevention.request_accepted")
        );
        rule.keep_display_on = true;
        assert_eq!(
            indicator(Some(&status), &rule),
            t!("sleep_prevention.system_request_accepted")
        );
        status.display_requested = true;
        assert_eq!(
            indicator(Some(&status), &rule),
            t!("sleep_prevention.system_request_accepted")
        );
        status.requesting_apps[0].keep_display_on = true;
        status.display_requested = false;
        rule.keep_display_on = false;
        assert_eq!(
            indicator(Some(&status), &rule),
            t!("sleep_prevention.system_request_accepted")
        );
        status.display_requested = true;
        status.observation_known = false;
        assert_eq!(indicator(Some(&status), &rule), t!("common.unknown"));
        status.observation_known = true;
        status.suppressed_apps.push(rule.executable_path.clone());
        assert_eq!(
            indicator(Some(&status), &rule),
            t!("sleep_prevention.request_accepted")
        );
        status.requesting_apps.clear();
        assert_eq!(
            indicator(Some(&status), &rule),
            t!("sleep_prevention.requests_paused")
        );
        rule.enabled = false;
        assert_eq!(indicator(None, &rule), t!("common.off"));
    }

    #[test]
    fn rule_absence_requires_an_observation_of_its_path() {
        let rule = SleepPreventionRule {
            executable_path: r"C:\Apps\test.exe".into(),
            enabled: true,
            keep_display_on: false,
        };
        let mut status = SleepPreventionSnapshot {
            enabled: true,
            observation_known: true,
            observed_rule_paths: Some([r"C:\Other\test.exe".into()].into()),
            ..Default::default()
        };
        assert_eq!(indicator(Some(&status), &rule), t!("common.unknown"));
        status.observed_rule_paths = Some([r"c:/apps/TEST.exe".into()].into());
        assert_eq!(
            indicator(Some(&status), &rule),
            t!("sleep_prevention.not_running")
        );
    }
}
