use super::{app_picker, design, widgets};
use crate::application::SettingsEditor;
use crate::config::{
    same_rule_executable_path, Settings, SleepPreventionRule, SleepPreventionSettings,
};
use crate::control::instance_limits::validate_path;
use crate::foreground::process_list::normalize_path_slashes;
use crate::sleep_prevention::{keep_awake_enabled, KeepAwakeDuration, SleepPreventionSnapshot};
use crate::ui::process_rules::{
    can_add_exact_process_candidate, exact_path_input, picked_path_input,
};
use iced::{
    widget::{column, row, text},
    Element, Fill,
};
use rust_i18n::t;
use std::time::{Instant, SystemTime};

#[derive(Default)]
pub(super) struct Editor {
    pub(super) path: String,
    keep_awake_duration: KeepAwakeDuration,
    pub(super) keep_awake_pending: bool,
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
    KeepAwakeDuration(KeepAwakeDuration),
    KeepAwake(Option<KeepAwakeDuration>),
}

pub(super) struct KeepAwakeView {
    until: Option<Instant>,
    availability: Result<(), String>,
}

impl KeepAwakeView {
    pub(super) fn new(
        persisted: &Settings,
        plugged_in: Option<bool>,
        status: &SleepPreventionSnapshot,
    ) -> Self {
        let availability = match plugged_in {
            None => Err(t!("sleep_prevention.power_source_unavailable").into_owned()),
            Some(plugged_in) => {
                let (profile, source) = if plugged_in {
                    (persisted, t!("power_source.plugged_in"))
                } else {
                    (persisted.battery_profile(), t!("power_source.on_battery"))
                };
                if keep_awake_enabled(profile) {
                    Ok(())
                } else {
                    Err(t!("sleep_prevention.keep_awake_disabled", source = source).into_owned())
                }
            }
        };
        Self {
            until: status.keep_awake_until,
            availability,
        }
    }
}

impl Editor {
    pub(super) fn reset_drafts(&mut self) {
        self.path.clear();
    }

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
            Message::KeepAwakeDuration(duration) => self.keep_awake_duration = duration,
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
            | Message::KeepAwake(_)
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
        keep_awake: KeepAwakeView,
    ) -> Element<'a, Message> {
        let mut requests_paused = false;
        let rules = settings
            .rules
            .iter()
            .enumerate()
            .map(|(index, rule)| {
                let status_key = indicator_key(status, rule);
                requests_paused |= status_key == "sleep_prevention.requests_paused";
                let header = widgets::process_rule_header(
                    &rule.executable_path,
                    candidates,
                    widgets::checkbox(rule.enabled)
                        .on_toggle(move |value| Message::RuleEnabled(index, value))
                        .into(),
                    Some(
                        text(t!(status_key).into_owned())
                            .size(design::typography::SECONDARY)
                            .into(),
                    ),
                    vec![widgets::checkbox(rule.keep_display_on)
                        .on_toggle(move |value| Message::KeepDisplayOn(index, value))
                        .into()],
                    widgets::rule_actions(None, Some(Message::Remove(index))),
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
        let mut picker = column![widgets::setting_title_with_help(
            "common.rules",
            "",
            "sleep_prevention.rules_help"
        )]
        .spacing(design::space::SMALL);
        if requests_paused {
            picker = picker.push(
                text(t!("sleep_prevention.paused_help").into_owned())
                    .size(design::typography::SECONDARY)
                    .style(text::secondary),
            );
        }
        picker = picker.push(app_picker::view_with_path_selection(
            &self.path,
            candidates,
            true,
            (Message::Path, |path| Message::Path(picked_path_input(path))),
            Message::Browse,
            can_add.then_some(Message::Add),
            |path| Self::can_add(settings, path).then_some(true),
        ));
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
                self.keep_awake_card(keep_awake),
                widgets::settings_card(picker),
                widgets::process_rules_table(
                    [t!("sleep_prevention.keep_display_on").to_string()],
                    rules,
                    true,
                ),
            ]
            .spacing(widgets::CARD_GAP),
        )
        .height(Fill)
        .into()
    }

    fn keep_awake_card(&self, view: KeepAwakeView) -> Element<'_, Message> {
        let (status, action): (_, Element<'_, _>) = if let Some(until) = view.until {
            let end = SystemTime::now() + until.saturating_duration_since(Instant::now());
            let local = chrono::DateTime::<chrono::Local>::from(end);
            (
                t!(
                    "sleep_prevention.keep_awake_active",
                    time = local.format("%H:%M")
                )
                .into_owned(),
                widgets::action_button(t!("sleep_prevention.keep_awake_stop"))
                    .on_press_maybe((!self.keep_awake_pending).then_some(Message::KeepAwake(None)))
                    .into(),
            )
        } else {
            let can_start = view.availability.is_ok() && !self.keep_awake_pending;
            let start = widgets::action_button(t!("sleep_prevention.keep_awake_start"))
                .on_press_maybe(
                    can_start.then_some(Message::KeepAwake(Some(self.keep_awake_duration))),
                );
            let status = view
                .availability
                .err()
                .unwrap_or_else(|| t!("common.off").into_owned());
            (
                status,
                row![
                    widgets::pick_list(
                        KeepAwakeDuration::OPTIONS,
                        Some(self.keep_awake_duration),
                        Message::KeepAwakeDuration,
                    )
                    .width(iced::Shrink),
                    start
                ]
                .spacing(design::space::SMALL)
                .align_y(iced::Center)
                .into(),
            )
        };
        widgets::settings_card(
            column![
                widgets::setting_row("sleep_prevention.keep_awake", action),
                text(status)
                    .size(design::typography::SECONDARY)
                    .style(text::secondary)
                    .width(Fill),
            ]
            .spacing(design::space::SMALL),
        )
        .into()
    }
}

fn indicator_key(
    status: Option<&SleepPreventionSnapshot>,
    rule: &SleepPreventionRule,
) -> &'static str {
    if !rule.enabled {
        return "common.off";
    }
    let Some(status) = status else {
        return "common.unknown";
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
    if !status.enabled {
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
    } else if contains(&status.suspended_apps) {
        "app_suspension.indicator.suspended"
    } else if contains(&status.running_apps) {
        "common.waiting"
    } else if status.observed_rule_paths.as_deref().is_some_and(contains) {
        "sleep_prevention.not_running"
    } else {
        "common.unknown"
    }
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
        assert_eq!(indicator_key(None, &rule), "common.unknown");
        assert_eq!(indicator_key(Some(&status), &rule), "common.waiting");
        status.system_requested = true;
        assert_eq!(
            indicator_key(Some(&status), &rule),
            "sleep_prevention.request_accepted"
        );
        rule.keep_display_on = true;
        assert_eq!(
            indicator_key(Some(&status), &rule),
            "sleep_prevention.system_request_accepted"
        );
        status.display_requested = true;
        assert_eq!(
            indicator_key(Some(&status), &rule),
            "sleep_prevention.system_request_accepted"
        );
        status.requesting_apps[0].keep_display_on = true;
        status.display_requested = false;
        rule.keep_display_on = false;
        assert_eq!(
            indicator_key(Some(&status), &rule),
            "sleep_prevention.system_request_accepted"
        );
        status.display_requested = true;
        status.observation_known = false;
        assert_eq!(indicator_key(Some(&status), &rule), "common.unknown");
        status.observation_known = true;
        status.suppressed_apps.push(rule.executable_path.clone());
        assert_eq!(
            indicator_key(Some(&status), &rule),
            "sleep_prevention.request_accepted"
        );
        status.requesting_apps.clear();
        assert_eq!(
            indicator_key(Some(&status), &rule),
            "sleep_prevention.requests_paused"
        );
        status.suspended_apps.push(rule.executable_path.clone());
        status.suppressed_apps.clear();
        assert_eq!(
            indicator_key(Some(&status), &rule),
            "app_suspension.indicator.suspended"
        );
        rule.enabled = false;
        assert_eq!(indicator_key(None, &rule), "common.off");
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
        assert_eq!(indicator_key(Some(&status), &rule), "common.unknown");
        status.observed_rule_paths = Some([r"c:/apps/TEST.exe".into()].into());
        assert_eq!(
            indicator_key(Some(&status), &rule),
            "sleep_prevention.not_running"
        );
    }
}
