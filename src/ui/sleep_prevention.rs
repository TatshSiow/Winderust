use super::{
    app_picker, design,
    feature_tabs::{FeatureTab, CONTENT_ID},
    widgets,
};
use crate::application::SettingsEditor;
use crate::config::{SleepPreventionRule, SleepPreventionSettings};
use crate::control::instance_limits::validate_path;
use crate::foreground::process_list::normalize_path_slashes;
use crate::sleep_prevention::{
    TemporarySleepPrevention, TemporarySleepPreventionDuration, TemporarySleepPreventionTimers,
};
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
    durations: [Option<String>; 2],
    pub(super) temporary_pending: bool,
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
    TemporaryDuration(TemporarySleepPrevention, String),
    Temporary(
        TemporarySleepPrevention,
        Option<TemporarySleepPreventionDuration>,
    ),
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
        let current = &settings.global().sleep_prevention;
        match message {
            Message::Enabled(value) if current.enabled != value => {
                settings.edit_global(|settings| settings.sleep_prevention.enabled = value);
            }
            Message::Path(value) => self.path = value,
            Message::TemporaryDuration(kind, duration) => {
                self.durations[kind as usize] = Some(duration)
            }
            Message::Add => {
                let path = exact_path_input(&self.path);
                if Self::can_add(current, path) {
                    settings.edit_global(|settings| {
                        settings.sleep_prevention.rules.push(SleepPreventionRule {
                            executable_path: normalize_path_slashes(path).into_owned(),
                            enabled: true,
                            keep_display_on: false,
                        });
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
                settings
                    .edit_global(|settings| settings.sleep_prevention.rules[index].enabled = value);
            }
            Message::KeepDisplayOn(index, value)
                if current
                    .rules
                    .get(index)
                    .is_some_and(|rule| rule.keep_display_on != value) =>
            {
                settings.edit_global(|settings| {
                    settings.sleep_prevention.rules[index].keep_display_on = value
                });
            }
            Message::Remove(index) if index < current.rules.len() => {
                settings.edit_global(|settings| settings.sleep_prevention.rules.remove(index));
            }
            Message::Enabled(_)
            | Message::Browse
            | Message::Temporary(..)
            | Message::RuleEnabled(..)
            | Message::KeepDisplayOn(..)
            | Message::Remove(_) => {}
        }
    }

    pub(super) fn view<'a>(
        &'a self,
        settings: &'a SleepPreventionSettings,
        candidates: &'a [app_picker::Candidate],
        temporary: TemporarySleepPreventionTimers,
        tab: FeatureTab,
    ) -> Element<'a, Message> {
        let body = if tab == FeatureTab::Setup {
            column![
                widgets::toggle_card(
                    "sleep_prevention.enable",
                    settings.enabled,
                    Some(Message::Enabled)
                ),
                widgets::settings_section(
                    "sleep_prevention.temporary_controls",
                    widgets::settings_card(widgets::settings_rows([
                        self.temporary_row(TemporarySleepPrevention::KeepAwake, temporary),
                        self.temporary_row(TemporarySleepPrevention::KeepDisplayOn, temporary),
                    ])),
                ),
            ]
        } else {
            self.custom_rules(settings, candidates)
        };
        super::scrolling::scrollable(body.spacing(widgets::CARD_GAP))
            .id(CONTENT_ID)
            .height(Fill)
            .into()
    }

    fn custom_rules<'a>(
        &'a self,
        settings: &'a SleepPreventionSettings,
        candidates: &'a [app_picker::Candidate],
    ) -> iced::widget::Column<'a, Message> {
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
                    None,
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
        let mut picker = column![widgets::help_text("sleep_prevention.rules_help")]
            .spacing(design::space::SMALL);
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
        column![
            widgets::settings_card(picker),
            widgets::process_rules_table(
                [t!("sleep_prevention.keep_display_on").to_string()],
                rules,
                false,
            ),
        ]
    }

    fn temporary_row(
        &self,
        kind: TemporarySleepPrevention,
        timers: TemporarySleepPreventionTimers,
    ) -> Element<'_, Message> {
        let title = match kind {
            TemporarySleepPrevention::KeepAwake => "sleep_prevention.temporary_keep_awake",
            TemporarySleepPrevention::KeepDisplayOn => "sleep_prevention.temporary_keep_display_on",
        };
        if let Some(until) = timers.until(kind) {
            let end = SystemTime::now() + until.saturating_duration_since(Instant::now());
            let local = chrono::DateTime::<chrono::Local>::from(end);
            let status = t!(
                "sleep_prevention.temporary_ends_at",
                time = local.format("%H:%M")
            );
            let stop = widgets::action_button(t!("sleep_prevention.temporary_stop"))
                .on_press_maybe(
                    (!self.temporary_pending).then_some(Message::Temporary(kind, None)),
                );
            return widgets::setting_row(
                title,
                row![
                    text(status.into_owned()).size(design::typography::SECONDARY),
                    stop
                ]
                .spacing(design::space::SMALL)
                .align_y(iced::Center),
            )
            .into();
        }
        let duration = self.durations[kind as usize].as_deref().unwrap_or("15");
        let minutes = t!("sleep_prevention.timer_minutes").into_owned();
        let start_message = (!self.temporary_pending)
            .then(|| {
                duration
                    .parse()
                    .ok()
                    .and_then(TemporarySleepPreventionDuration::from_minutes)
            })
            .flatten()
            .map(|duration| Message::Temporary(kind, Some(duration)));
        let duration_field = widgets::stepper(
            duration,
            1..=u64::from(TemporarySleepPreventionDuration::MAX_MINUTES),
            1,
            Some(move |duration| Message::TemporaryDuration(kind, duration)),
        );
        let actions = row![
            duration_field,
            text(minutes).size(design::typography::SECONDARY),
            widgets::action_button(t!("sleep_prevention.temporary_start"))
                .on_press_maybe(start_message),
        ]
        .align_y(iced::Center)
        .spacing(design::space::SMALL);
        widgets::setting_row(title, actions).into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{PowerSourceProfile, Settings};

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
            .global()
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
}
