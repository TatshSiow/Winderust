use super::design;
use super::widgets::{action_button, checkbox, pick_list, text_input};
use crate::app_suspension::{self, AppSuspensionSnapshot};
use crate::config::{AppSuspensionSettings, NetworkThresholdUnit};
use crate::ui::process_rules::{
    can_add_app_suspension_process, new_app_suspension_rule, process_setting_matches,
};
use crate::ui::scrolling::scrollable;
use iced::widget::{column, text};
use iced::{Element, Fill};
use rust_i18n::t;
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct Editor {
    pub(super) path: String,
    expanded: [bool; 3],
    delay_drafts: BTreeMap<Delay, String>,
    threshold_drafts: Vec<[Option<String>; 2]>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Delay {
    Background,
    ThawInterval,
    ThawDuration,
    Audio,
    Network,
}
impl Delay {
    fn label(self) -> &'static str {
        match self {
            Self::Background => "app_suspension.background_delay",
            Self::ThawInterval => "app_suspension.resume_every",
            Self::ThawDuration => "app_suspension.resume_duration",
            Self::Audio => "app_suspension.suspend_after_audio",
            Self::Network => "app_suspension.network_suspension_delay",
        }
    }

    fn maximum(self) -> u64 {
        match self {
            Self::Background | Self::ThawInterval => AppSuspensionSettings::MAX_DELAY_SECONDS,
            _ => AppSuspensionSettings::MAX_WAKE_DURATION_SECONDS,
        }
    }

    fn parse(self, value: &str) -> Option<u64> {
        value
            .parse()
            .ok()
            .filter(|value| (1..=self.maximum()).contains(value))
    }

    pub(super) fn error_message(self) -> String {
        t!(
            "app_suspension.invalid_delay",
            setting = t!(self.label()),
            maximum = self.maximum()
        )
        .to_string()
    }
}
#[derive(Debug, Clone)]
pub(super) enum Message {
    Enabled(bool),
    Group(usize),
    Path(String),
    Add,
    Browse,
    Delay(Delay, String),
    Thaw(bool),
    Audio(bool),
    Network(bool),
    RuleEnabled(usize, bool),
    RuleAudio(usize, bool),
    RuleNetwork(usize, bool),
    Threshold(usize, bool, String),
    Unit(usize, bool, NetworkThresholdUnit),
    Toggle(usize),
    Remove(usize),
}
impl Editor {
    pub(super) fn invalid_delay(&self) -> Option<Delay> {
        self.delay_drafts
            .iter()
            .find_map(|(&field, value)| field.parse(value).is_none().then_some(field))
    }

    pub(super) fn has_invalid_thresholds(&self) -> bool {
        self.threshold_drafts
            .iter()
            .flatten()
            .flatten()
            .any(|value| parse_threshold(value).is_none())
    }

    pub(super) fn update(
        &mut self,
        s: &mut AppSuspensionSettings,
        status: &AppSuspensionSnapshot,
        unavailable: &[String],
        m: Message,
    ) -> Option<(String, bool)> {
        match m {
            Message::Enabled(v) => s.enabled = v,
            Message::Group(i) => {
                if let Some(v) = self.expanded.get_mut(i) {
                    *v = !*v;
                }
            }
            Message::Path(v) => self.path = v,
            Message::Add if self.can_add(s, unavailable) => {
                s.suspendable_apps.push(new_app_suspension_rule(&self.path));
                self.path.clear();
            }
            Message::Delay(field, value) => {
                if let Some(seconds) = field.parse(&value) {
                    *match field {
                        Delay::Background => &mut s.background_delay_seconds,
                        Delay::ThawInterval => &mut s.temporary_thaw_interval_seconds,
                        Delay::ThawDuration => &mut s.temporary_thaw_duration_seconds,
                        Delay::Audio => &mut s.audio_wake_duration_seconds,
                        Delay::Network => &mut s.network_wake_duration_seconds,
                    } = seconds;
                }
                self.delay_drafts.insert(field, value);
            }
            Message::Thaw(v) => s.temporary_thaw_enabled = v,
            Message::Audio(v) => s.audio_wake_enabled = v,
            Message::Network(v) => s.network_wake_enabled = v,
            Message::RuleEnabled(i, v) => {
                if let Some(r) = s.suspendable_apps.get_mut(i) {
                    r.enabled = v;
                }
            }
            Message::RuleAudio(i, v) => {
                if let Some(r) = s.suspendable_apps.get_mut(i) {
                    r.audio_wake_enabled = v;
                }
            }
            Message::RuleNetwork(i, v) => {
                if let Some(r) = s.suspendable_apps.get_mut(i) {
                    r.network_wake_enabled = v;
                }
            }
            Message::Threshold(i, upload, value) => {
                let rule_count = s.suspendable_apps.len();
                if let Some(r) = s.suspendable_apps.get_mut(i) {
                    if let Some(threshold) = parse_threshold(&value) {
                        if upload {
                            r.network_upload_threshold_bytes = r
                                .network_upload_threshold_unit
                                .threshold_bytes_from_value(threshold);
                        } else {
                            r.network_download_threshold_bytes = r
                                .network_download_threshold_unit
                                .threshold_bytes_from_value(threshold);
                        }
                    }
                    self.threshold_drafts.resize(rule_count, [None, None]);
                    self.threshold_drafts[i][usize::from(upload)] = Some(value);
                }
            }
            Message::Unit(i, upload, v) => {
                if let Some(r) = s.suspendable_apps.get_mut(i) {
                    let unit = if upload {
                        &mut r.network_upload_threshold_unit
                    } else {
                        &mut r.network_download_threshold_unit
                    };
                    if *unit != v {
                        *unit = v;
                        if let Some(row) = self.threshold_drafts.get_mut(i) {
                            let draft = &mut row[usize::from(upload)];
                            if draft.as_deref().and_then(parse_threshold).is_some() {
                                *draft = None;
                            }
                        }
                    }
                }
            }
            Message::Toggle(i) => {
                if let Some(r) = s.suspendable_apps.get(i) {
                    let frozen = app_suspension::contains_process(
                        &status.suspended_apps,
                        &r.executable_path,
                    );
                    if frozen
                        || (status.enabled
                            && r.enabled
                            && !unavailable
                                .iter()
                                .any(|p| process_setting_matches(p, &r.executable_path)))
                    {
                        return Some((r.executable_path.clone(), !frozen));
                    }
                }
            }
            Message::Remove(i) => {
                if i < s.suspendable_apps.len() {
                    s.suspendable_apps.remove(i);
                    if i < self.threshold_drafts.len() {
                        self.threshold_drafts.remove(i);
                    }
                }
            }

            Message::Add | Message::Browse => {}
        }
        None
    }
    fn can_add(&self, s: &AppSuspensionSettings, unavailable: &[String]) -> bool {
        can_add_app_suspension_process(s, &self.path)
            && !unavailable
                .iter()
                .any(|p| process_setting_matches(p, &self.path))
    }
    pub(super) fn view<'a>(
        &'a self,
        s: &'a AppSuspensionSettings,
        status: &AppSuspensionSnapshot,
        unavailable: &[String],
        candidates: &'a [super::app_picker::Candidate],
    ) -> Element<'a, Message> {
        let mut body = column![
            super::widgets::heading(
                t!("app_suspension.behavior").to_string(),
                design::typography::BODY
            ),
            super::widgets::settings_card(super::widgets::settings_rows([
                super::widgets::setting_row(
                    "app_suspension.enable",
                    super::widgets::switch(s.enabled, Some(Message::Enabled))
                )
                .into(),
                self.delay_row(Delay::Background, s.background_delay_seconds),
            ])),
        ]
        .spacing(super::widgets::CARD_GAP);
        for (i, key, enabled, toggle, rows) in [
            (
                0,
                "app_suspension.temporary_resume",
                s.temporary_thaw_enabled,
                Message::Thaw as fn(bool) -> Message,
                vec![
                    (Delay::ThawInterval, s.temporary_thaw_interval_seconds),
                    (Delay::ThawDuration, s.temporary_thaw_duration_seconds),
                ],
            ),
            (
                1,
                "app_suspension.audio_detection",
                s.audio_wake_enabled,
                Message::Audio,
                vec![(Delay::Audio, s.audio_wake_duration_seconds)],
            ),
            (
                2,
                "app_suspension.network_detection",
                s.network_wake_enabled,
                Message::Network,
                vec![(Delay::Network, s.network_wake_duration_seconds)],
            ),
        ] {
            let controls = super::widgets::settings_rows(
                rows.into_iter()
                    .map(|(field, value)| self.delay_row(field, value)),
            );
            body = body.push(super::widgets::setting_group(
                key.to_string(),
                self.expanded[i],
                Message::Group(i),
                super::widgets::switch(enabled, Some(toggle)),
                controls,
            ));
        }
        body = body.push(super::widgets::heading(
            t!("common.rules").to_string(),
            design::typography::BODY,
        ));
        body = body.push(super::app_picker::view(
            &self.path,
            candidates,
            true,
            Message::Path,
            Message::Browse,
            (self.can_add(s, unavailable)).then_some(Message::Add),
            |path| {
                Some(
                    !unavailable
                        .iter()
                        .any(|blocked| process_setting_matches(blocked, path)),
                )
            },
        ));
        let mut cards = Vec::new();
        for (i, r) in s.suspendable_apps.iter().enumerate() {
            let frozen =
                app_suspension::contains_process(&status.suspended_apps, &r.executable_path);
            let blocked = unavailable
                .iter()
                .any(|p| process_setting_matches(p, &r.executable_path));
            let state = column![
                text(indicator(status, &r.executable_path, blocked)),
                action_button(
                    t!(if frozen {
                        "app_suspension.resume_temporarily"
                    } else {
                        "app_suspension.suspend"
                    })
                    .to_string()
                )
                .on_press_maybe(
                    (frozen || (status.enabled && r.enabled && !blocked))
                        .then_some(Message::Toggle(i))
                ),
            ]
            .spacing(design::space::SMALL);
            let mut controls: Vec<Element<'_, Message>> = vec![
                checkbox(r.audio_wake_enabled)
                    .on_toggle_maybe(Some(move |v| Message::RuleAudio(i, v)))
                    .into(),
                checkbox(r.network_wake_enabled)
                    .on_toggle_maybe(Some(move |v| Message::RuleNetwork(i, v)))
                    .into(),
            ];
            for (upload, bytes, unit) in [
                (
                    false,
                    r.network_download_threshold_bytes,
                    r.network_download_threshold_unit,
                ),
                (
                    true,
                    r.network_upload_threshold_bytes,
                    r.network_upload_threshold_unit,
                ),
            ] {
                let draft = self
                    .threshold_drafts
                    .get(i)
                    .and_then(|row| row[usize::from(upload)].as_deref());
                let formatted;
                let value = if let Some(draft) = draft {
                    draft
                } else {
                    formatted = unit.threshold_value_from_bytes(bytes).to_string();
                    &formatted
                };
                controls.push(
                    column![
                        text_input("", value)
                            .on_input(move |v| Message::Threshold(i, upload, v))
                            .width(65),
                        pick_list(
                            NetworkThresholdUnit::ALL.map(Unit),
                            Some(Unit(unit)),
                            move |v| Message::Unit(i, upload, v.0)
                        )
                    ]
                    .spacing(design::space::SMALL)
                    .into(),
                );
            }
            cards.push((
                super::widgets::stable_key(&r.executable_path),
                column![
                    super::widgets::process_rule_header(
                        &r.executable_path,
                        candidates,
                        checkbox(r.enabled)
                            .on_toggle_maybe(Some(move |v| Message::RuleEnabled(i, v)))
                            .into(),
                        Some(state.into()),
                        controls,
                        iced::widget::container(super::widgets::rule_delete_button(Some(
                            Message::Remove(i)
                        )))
                        .center_x(80)
                        .into(),
                    ),
                    super::widgets::settings_divider()
                ]
                .into(),
            ));
        }
        body = body.push(super::widgets::process_rules_table_with_actions(
            [
                "app_suspension.audio",
                "app_suspension.network",
                "app_suspension.download",
                "app_suspension.upload",
            ]
            .map(|key| t!(key).to_string()),
            cards,
            t!("app_suspension.no_suspendable").to_string(),
            80,
            true,
        ));

        if let Some(error) = &status.last_error {
            body = body.push(text(error.clone()));
        }
        scrollable(body).height(Fill).into()
    }

    fn delay_row(&self, field: Delay, seconds: u64) -> Element<'static, Message> {
        let formatted;
        let value = if let Some(draft) = self.delay_drafts.get(&field) {
            draft
        } else {
            formatted = seconds.to_string();
            &formatted
        };
        super::widgets::setting_row_with_unit(
            field.label(),
            "s",
            super::widgets::stepper(
                value,
                1..=field.maximum(),
                1,
                Some(move |v| Message::Delay(field, v)),
            ),
        )
        .into()
    }
}

fn parse_threshold(value: &str) -> Option<f64> {
    let value: f64 = value.parse().ok()?;
    (value.is_finite() && value >= 0.0).then_some(value)
}

fn indicator(s: &AppSuspensionSnapshot, p: &str, unavailable: bool) -> String {
    let key = if app_suspension::is_builtin_excluded(p) {
        "protected"
    } else if unavailable {
        "unavailable"
    } else if app_suspension::contains_process(&s.network_wake_apps, p) {
        "network"
    } else if app_suspension::contains_process(&s.audio_wake_apps, p) {
        "audio"
    } else if app_suspension::contains_process(&s.suspended_apps, p) {
        "suspended"
    } else if app_suspension::contains_process(&s.temporary_thawed_apps, p) {
        "temporarily_resumed"
    } else if app_suspension::contains_process(&s.background_grace_apps, p) {
        "waiting"
    } else if s.status_unknown {
        "unknown"
    } else if app_suspension::contains_process(&s.running_apps, p) {
        "running"
    } else if s.enabled {
        "not_running"
    } else {
        "off"
    };
    {
        let locale_key = format!("app_suspension.indicator.{key}");
        t!(&locale_key).to_string()
    }
    .to_string()
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Unit(NetworkThresholdUnit);
impl std::fmt::Display for Unit {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0.label())
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_delays_keep_independent_text_and_bounds() {
        let mut editor = Editor::default();
        let mut settings = AppSuspensionSettings::default();
        let status = AppSuspensionSnapshot::default();
        for (field, input) in [(Delay::Background, ""), (Delay::ThawDuration, "3601")] {
            editor.update(
                &mut settings,
                &status,
                &[],
                Message::Delay(field, input.into()),
            );
        }
        assert_eq!(editor.invalid_delay(), Some(Delay::Background));
        assert_eq!(editor.delay_drafts[&Delay::Background], "");
        assert!(settings.validate().is_ok());

        editor.update(
            &mut settings,
            &status,
            &[],
            Message::Delay(Delay::Background, "86400".into()),
        );
        assert_eq!(editor.invalid_delay(), Some(Delay::ThawDuration));
        assert_eq!(settings.background_delay_seconds, 86_400);

        editor.update(
            &mut settings,
            &status,
            &[],
            Message::Delay(Delay::ThawDuration, "3600".into()),
        );
        assert_eq!(editor.invalid_delay(), None);
        assert_eq!(settings.temporary_thaw_duration_seconds, 3600);
    }

    #[test]
    fn threshold_edits_follow_their_rule_and_keep_canonical_bytes() {
        let mut editor = Editor::default();
        let mut settings = AppSuspensionSettings::default();
        let status = AppSuspensionSnapshot::default();
        settings.suspendable_apps.extend([
            new_app_suspension_rule(r"C:\Apps\first.exe"),
            new_app_suspension_rule(r"C:\Apps\second.exe"),
        ]);
        for message in [
            Message::Unit(1, true, NetworkThresholdUnit::Kilobytes),
            Message::Threshold(1, true, "1.5".into()),
            Message::Threshold(1, false, "".into()),
            Message::Remove(0),
            Message::Unit(0, true, NetworkThresholdUnit::Megabytes),
            Message::Unit(0, false, NetworkThresholdUnit::Gigabytes),
        ] {
            editor.update(&mut settings, &status, &[], message);
        }
        assert!(editor.has_invalid_thresholds());
        assert_eq!(editor.threshold_drafts[0], [Some(String::new()), None]);
        assert_eq!(
            settings.suspendable_apps[0].network_upload_threshold_bytes,
            1500
        );
        assert_eq!(
            settings.suspendable_apps[0].network_download_threshold_bytes,
            1
        );

        editor.update(
            &mut settings,
            &status,
            &[],
            Message::Threshold(0, false, "0".into()),
        );
        assert!(!editor.has_invalid_thresholds());
        assert_eq!(
            settings.suspendable_apps[0].network_download_threshold_bytes,
            0
        );
    }

    #[test]
    fn unavailable_targets_cannot_be_added_or_frozen_but_can_thaw() {
        let mut e = Editor::default();
        let mut s = crate::config::Settings::default().app_suspension;
        s.enabled = true;
        let p = r"C:\Apps\test.exe".to_owned();
        let unavailable = vec![p.clone()];
        let mut status = AppSuspensionSnapshot {
            enabled: true,
            ..Default::default()
        };
        e.update(&mut s, &status, &unavailable, Message::Path(p.clone()));
        e.update(&mut s, &status, &unavailable, Message::Add);
        assert!(s.suspendable_apps.is_empty());
        s.suspendable_apps.push(new_app_suspension_rule(&p));
        assert!(e
            .update(&mut s, &status, &unavailable, Message::Toggle(0))
            .is_none());
        status.suspended_apps.push(p.clone());
        assert_eq!(
            e.update(&mut s, &status, &unavailable, Message::Toggle(0)),
            Some((p, false))
        );
    }
}
