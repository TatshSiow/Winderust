use super::widgets::{self, Ui};
use crate::app_suspension::{self, AppSuspensionSnapshot};
use crate::config::{AppSuspensionSettings, NetworkThresholdUnit};
use crate::ui::process_rules::{
    can_add_app_suspension_process, new_app_suspension_rule, process_setting_matches,
};
use rust_i18n::t;
use std::collections::HashMap;
use windui::prelude::{signal, Element};
#[derive(Default)]
pub(super) struct Editor {
    pub(super) path: String,
    expanded: [bool; 3],
    numbers: HashMap<Input, String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Delay {
    Background,
    ThawInterval,
    ThawDuration,
    Audio,
    Network,
}
#[derive(Clone, PartialEq, Eq, Hash)]
enum Input {
    Delay(Delay),
    Threshold(String, bool),
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
    pub(super) fn has_invalid_inputs(&self) -> bool {
        self.numbers.iter().any(|(input, raw)| match input {
            Input::Delay(field) => !raw
                .parse::<u64>()
                .is_ok_and(|value| (1..=delay_max(*field)).contains(&value)),
            Input::Threshold(_, _) => !raw
                .parse::<f64>()
                .is_ok_and(|value| value.is_finite() && value >= 0.0),
        })
    }

    fn number(&self, input: &Input, value: impl ToString) -> String {
        self.numbers
            .get(input)
            .cloned()
            .unwrap_or_else(|| value.to_string())
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
            Message::Delay(field, v) => {
                self.numbers.insert(Input::Delay(field), v.clone());
                let max = delay_max(field);
                if let Ok(v) = v.parse::<u64>() {
                    if (1..=max).contains(&v) {
                        *match field {
                            Delay::Background => &mut s.background_delay_seconds,
                            Delay::ThawInterval => &mut s.temporary_thaw_interval_seconds,
                            Delay::ThawDuration => &mut s.temporary_thaw_duration_seconds,
                            Delay::Audio => &mut s.audio_wake_duration_seconds,
                            Delay::Network => &mut s.network_wake_duration_seconds,
                        } = v;
                    }
                }
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
            Message::Threshold(i, upload, v) => {
                if let Some(rule) = s.suspendable_apps.get(i) {
                    self.numbers.insert(
                        Input::Threshold(rule.executable_path.clone(), upload),
                        v.clone(),
                    );
                }
                if let (Some(r), Ok(v)) = (s.suspendable_apps.get_mut(i), v.parse::<f64>()) {
                    if v.is_finite() && v >= 0.0 {
                        if upload {
                            r.network_upload_threshold_bytes = r
                                .network_upload_threshold_unit
                                .threshold_bytes_from_value(v);
                        } else {
                            r.network_download_threshold_bytes = r
                                .network_download_threshold_unit
                                .threshold_bytes_from_value(v);
                        }
                    }
                }
            }
            Message::Unit(i, upload, v) => {
                if let Some(r) = s.suspendable_apps.get_mut(i) {
                    self.numbers
                        .remove(&Input::Threshold(r.executable_path.clone(), upload));
                    if upload {
                        r.network_upload_threshold_unit = v;
                    } else {
                        r.network_download_threshold_unit = v;
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
                    let removed = s.suspendable_apps.remove(i);
                    self.numbers.retain(|input, _| {
                        !matches!(input, Input::Threshold(path, _) if path == &removed.executable_path)
                    });
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
    pub(super) fn view(
        &self,
        settings: &AppSuspensionSettings,
        status: &AppSuspensionSnapshot,
        unavailable: &[String],
        candidates: &[super::app_picker::Candidate],
        ui: Ui<Message>,
    ) -> Element {
        let mut body = vec![
            widgets::setting_row(
                t!("app_suspension.enable").to_string(),
                ui.toggle(settings.enabled, Message::Enabled),
            ),
            delay_row(
                Delay::Background,
                "app_suspension.background_delay",
                self.number(
                    &Input::Delay(Delay::Background),
                    settings.background_delay_seconds,
                ),
                ui.clone(),
            ),
        ];
        for (index, title, enabled, toggle, fields) in [
            (
                0,
                "app_suspension.temporary_thaw",
                settings.temporary_thaw_enabled,
                Message::Thaw as fn(bool) -> Message,
                vec![
                    (
                        Delay::ThawInterval,
                        "app_suspension.thaw_every",
                        settings.temporary_thaw_interval_seconds,
                    ),
                    (
                        Delay::ThawDuration,
                        "app_suspension.thaw_duration",
                        settings.temporary_thaw_duration_seconds,
                    ),
                ],
            ),
            (
                1,
                "app_suspension.audio_detection",
                settings.audio_wake_enabled,
                Message::Audio,
                vec![(
                    Delay::Audio,
                    "app_suspension.audio_refreeze",
                    settings.audio_wake_duration_seconds,
                )],
            ),
            (
                2,
                "app_suspension.network_detection",
                settings.network_wake_enabled,
                Message::Network,
                vec![(
                    Delay::Network,
                    "app_suspension.network_refreeze",
                    settings.network_wake_duration_seconds,
                )],
            ),
        ] {
            let controls = fields.into_iter().map(|(field, key, value)| {
                delay_row(
                    field,
                    key,
                    self.number(&Input::Delay(field), value),
                    ui.clone(),
                )
            });
            body.push(widgets::card(
                t!(title).to_string(),
                [
                    widgets::setting_row(t!(title).to_string(), ui.toggle(enabled, toggle)),
                    ui.collapsible(
                        t!("common.control").to_string(),
                        self.expanded[index],
                        widgets::col().children(controls),
                        move |_| Message::Group(index),
                    ),
                ],
            ));
        }
        body.push(super::app_picker::view(
            &self.path,
            candidates,
            true,
            Message::Path,
            Message::Browse,
            Some(Message::Add),
            |candidate| Some(candidate.info.has_suspendable_instance),
            ui.clone(),
        ));
        for (index, rule) in settings.suspendable_apps.iter().enumerate() {
            let frozen =
                app_suspension::contains_process(&status.suspended_apps, &rule.executable_path);
            let blocked = unavailable
                .iter()
                .any(|path| process_setting_matches(path, &rule.executable_path));
            let mut controls = vec![
                ui.checkbox(
                    t!("common.enable").to_string(),
                    rule.enabled,
                    move |value| Message::RuleEnabled(index, value),
                ),
                widgets::label(indicator(status, &rule.executable_path, blocked)),
                ui.button(
                    t!(if frozen {
                        "app_suspension.thaw"
                    } else {
                        "app_suspension.freeze"
                    })
                    .to_string(),
                    Message::Toggle(index),
                )
                .enabled(frozen || (status.enabled && rule.enabled && !blocked)),
                ui.checkbox(
                    t!("app_suspension.audio").to_string(),
                    rule.audio_wake_enabled,
                    move |value| Message::RuleAudio(index, value),
                ),
                ui.checkbox(
                    t!("app_suspension.network").to_string(),
                    rule.network_wake_enabled,
                    move |value| Message::RuleNetwork(index, value),
                ),
            ];
            for (upload, bytes, unit, title) in [
                (
                    false,
                    rule.network_download_threshold_bytes,
                    rule.network_download_threshold_unit,
                    "app_suspension.download",
                ),
                (
                    true,
                    rule.network_upload_threshold_bytes,
                    rule.network_upload_threshold_unit,
                    "app_suspension.upload",
                ),
            ] {
                controls.push(widgets::setting_row(
                    t!(title).to_string(),
                    widgets::row().children([
                        ui.input(
                            self.number(
                                &Input::Threshold(rule.executable_path.clone(), upload),
                                unit.threshold_value_from_bytes(bytes),
                            ),
                            "0",
                            move |value| Message::Threshold(index, upload, value),
                        ),
                        ui.select(
                            NetworkThresholdUnit::ALL.map(Unit),
                            Unit(unit),
                            move |value| Message::Unit(index, upload, value.0),
                        ),
                    ]),
                ));
            }
            controls.push(ui.button(t!("common.remove").to_string(), Message::Remove(index)));
            body.push(widgets::card(rule.executable_path.clone(), controls));
        }
        if settings.suspendable_apps.is_empty() {
            body.push(widgets::label(
                t!("app_suspension.no_suspendable").to_string(),
            ));
        }
        if let Some(error) = &status.last_error {
            body.push(widgets::error(error.clone()));
        }
        widgets::page(body)
    }
}
fn delay_max(field: Delay) -> u64 {
    match field {
        Delay::Background | Delay::ThawInterval => 86400,
        _ => 3600,
    }
}

fn delay_row(field: Delay, title: &str, value: String, ui: Ui<Message>) -> Element {
    widgets::setting_row(
        format!("{} (s)", t!(title)),
        ui.number(signal(value), 1, delay_max(field), 1, move |value| {
            Message::Delay(field, value)
        }),
    )
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
        "frozen"
    } else if app_suspension::contains_process(&s.temporary_thawed_apps, p) {
        "thawed"
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
    fn invalid_native_drafts_survive_rule_reindexing_and_clear_when_corrected_or_removed() {
        let mut editor = Editor::default();
        let mut settings = AppSuspensionSettings::default();
        let status = AppSuspensionSnapshot::default();
        let original_delay = settings.background_delay_seconds;
        editor.update(
            &mut settings,
            &status,
            &[],
            Message::Delay(Delay::Background, "0".into()),
        );
        assert_eq!(settings.background_delay_seconds, original_delay);
        assert!(editor.has_invalid_inputs());
        assert_eq!(
            editor.number(&Input::Delay(Delay::Background), original_delay),
            "0"
        );
        editor.update(
            &mut settings,
            &status,
            &[],
            Message::Delay(Delay::Background, "15".into()),
        );
        assert!(!editor.has_invalid_inputs());

        settings.suspendable_apps = [r"C:\Apps\first.exe", r"C:\Apps\second.exe"]
            .map(new_app_suspension_rule)
            .to_vec();
        let bytes = settings.suspendable_apps[1].network_download_threshold_bytes;
        editor.update(
            &mut settings,
            &status,
            &[],
            Message::Threshold(1, false, "NaN".into()),
        );
        assert_eq!(
            settings.suspendable_apps[1].network_download_threshold_bytes,
            bytes
        );
        editor.update(&mut settings, &status, &[], Message::Remove(0));
        assert!(editor.has_invalid_inputs());
        assert_eq!(
            editor.number(
                &Input::Threshold(settings.suspendable_apps[0].executable_path.clone(), false),
                bytes
            ),
            "NaN"
        );
        editor.update(
            &mut settings,
            &status,
            &[],
            Message::Threshold(0, false, "1.5".into()),
        );
        assert!(!editor.has_invalid_inputs());
        editor.update(
            &mut settings,
            &status,
            &[],
            Message::Threshold(0, true, "-1".into()),
        );
        assert!(editor.has_invalid_inputs());
        editor.update(&mut settings, &status, &[], Message::Remove(0));
        assert!(!editor.has_invalid_inputs());
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
