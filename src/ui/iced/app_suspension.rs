use crate::app_suspension::{self, AppSuspensionSnapshot};
use crate::config::{AppSuspensionSettings, NetworkThresholdUnit};
use crate::ui::process_rules::{
    can_add_app_suspension_process, new_app_suspension_rule, process_setting_matches,
};
use iced::widget::{button, checkbox, column, pick_list, row, scrollable, text, text_input};
use iced::{Element, Fill};
use rust_i18n::t;
#[derive(Default)]
pub(super) struct Editor {
    pub(super) path: String,
    removing: Option<usize>,
    removed: Option<(usize, crate::config::AppSuspensionRule)>,
    collapsed: [bool; 3],
}
#[derive(Debug, Clone, Copy)]
pub(super) enum Delay {
    Background,
    ThawInterval,
    ThawDuration,
    Audio,
    Network,
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
    ConfirmRemove,
    Removed(String),
    CancelRemove,
}
impl Editor {
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
                if let Some(v) = self.collapsed.get_mut(i) {
                    *v = !*v;
                }
            }
            Message::Path(v) => self.path = v,
            Message::Add if s.enabled && self.can_add(s, unavailable) => {
                self.removed = None;
                s.suspendable_apps.push(new_app_suspension_rule(&self.path));
                self.path.clear();
            }
            Message::Delay(field, v) => {
                let max = match field {
                    Delay::Background | Delay::ThawInterval => 86_400,
                    _ => 3600,
                };
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
            Message::Remove(i) => self.removing = Some(i),
            Message::CancelRemove => self.removing = None,
            Message::ConfirmRemove => {
                if let Some(i) = self
                    .removing
                    .take()
                    .filter(|i| *i < s.suspendable_apps.len())
                {
                    self.removed = Some((i, s.suspendable_apps.remove(i)));
                }
            }
            Message::Removed(path) => {
                if self
                    .removed
                    .as_ref()
                    .is_some_and(|(_, r)| r.executable_path == path)
                {
                    self.removed = None;
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
        candidates: &[String],
        motion_enabled: bool,
    ) -> Element<'a, Message> {
        let mut body = column![
            checkbox(s.enabled)
                .label(t!("app_suspension.enable").to_string())
                .on_toggle(Message::Enabled),
            text(t!("app_suspension.intro_1").to_string())
        ]
        .spacing(12);
        body = body.push(delay_row(
            Delay::Background,
            "app_suspension.background_delay",
            s.background_delay_seconds,
            s.enabled,
        ));
        for (i, key, enabled, toggle, rows) in [
            (
                0,
                "app_suspension.temporary_thaw",
                s.temporary_thaw_enabled,
                Message::Thaw as fn(bool) -> Message,
                vec![
                    (
                        Delay::ThawInterval,
                        "app_suspension.thaw_every",
                        s.temporary_thaw_interval_seconds,
                    ),
                    (
                        Delay::ThawDuration,
                        "app_suspension.thaw_duration",
                        s.temporary_thaw_duration_seconds,
                    ),
                ],
            ),
            (
                1,
                "app_suspension.audio_detection",
                s.audio_wake_enabled,
                Message::Audio,
                vec![(
                    Delay::Audio,
                    "app_suspension.audio_refreeze",
                    s.audio_wake_duration_seconds,
                )],
            ),
            (
                2,
                "app_suspension.network_detection",
                s.network_wake_enabled,
                Message::Network,
                vec![(
                    Delay::Network,
                    "app_suspension.network_refreeze",
                    s.network_wake_duration_seconds,
                )],
            ),
        ] {
            let controls =
                iced::widget::Column::with_children(rows.into_iter().map(
                    |(field, label, value)| delay_row(field, label, value, s.enabled && enabled),
                ))
                .spacing(8);
            body = body.push(
                column![
                    row![
                        button(text(t!(key).to_string())).on_press(Message::Group(i)),
                        checkbox(enabled)
                            .label(t!("common.enabled").to_string())
                            .on_toggle_maybe(s.enabled.then_some(toggle))
                    ]
                    .spacing(8),
                    super::motion::reveal(controls, !self.collapsed[i], motion_enabled)
                ]
                .spacing(8),
            );
        }
        body = body
            .push(text(t!("app_suspension.suspendable_help").to_string()))
            .push(
                row![
                    text_input(&t!("process_list.executable_path"), &self.path)
                        .on_input(Message::Path),
                    button(text(t!("common.browse_executable").to_string()))
                        .on_press(Message::Browse),
                    button(text(t!("common.add").to_string())).on_press_maybe(
                        (s.enabled && self.can_add(s, unavailable)).then_some(Message::Add)
                    )
                ]
                .spacing(8),
            );
        if s.enabled && !candidates.is_empty() {
            let query = self.path.to_lowercase();
            let mut choices = column![].spacing(3);
            for path in candidates
                .iter()
                .filter(|p| p.to_lowercase().contains(&query))
            {
                let blocked = unavailable.iter().any(|p| process_setting_matches(p, path));
                let label = if blocked {
                    format!("{} - {}", path, t!("app_suspension.indicator.unavailable"))
                } else {
                    path.clone()
                };
                choices = choices.push(
                    button(text(label))
                        .on_press_maybe((!blocked).then_some(Message::Path(path.clone()))),
                );
            }
            body = body.push(scrollable(choices).height(130));
        }
        let mut cards = Vec::new();
        let mut rules = s.suspendable_apps.iter().enumerate().collect::<Vec<_>>();
        if let Some((i, rule)) = &self.removed {
            rules.insert((*i).min(rules.len()), (*i, rule));
        }
        for (i, r) in rules {
            let frozen =
                app_suspension::contains_process(&status.suspended_apps, &r.executable_path);
            let blocked = unavailable
                .iter()
                .any(|p| process_setting_matches(p, &r.executable_path));
            let mut card = column![
                row![
                    checkbox(r.enabled)
                        .label(r.executable_path.clone())
                        .on_toggle_maybe(s.enabled.then_some(move |v| Message::RuleEnabled(i, v))),
                    text(indicator(status, &r.executable_path, blocked)),
                    button(text(
                        t!(if frozen {
                            "app_suspension.thaw"
                        } else {
                            "app_suspension.freeze"
                        })
                        .to_string()
                    ))
                    .on_press_maybe(
                        (frozen || (status.enabled && r.enabled && !blocked))
                            .then_some(Message::Toggle(i))
                    ),
                    button(text(t!("common.remove").to_string())).on_press(Message::Remove(i))
                ]
                .spacing(8),
                row![
                    checkbox(r.audio_wake_enabled)
                        .label(t!("app_suspension.audio").to_string())
                        .on_toggle_maybe(s.enabled.then_some(move |v| Message::RuleAudio(i, v))),
                    checkbox(r.network_wake_enabled)
                        .label(t!("app_suspension.network").to_string())
                        .on_toggle_maybe(s.enabled.then_some(move |v| Message::RuleNetwork(i, v)))
                ]
                .spacing(12)
            ]
            .spacing(8);
            for (upload, bytes, unit, label) in [
                (
                    false,
                    r.network_download_threshold_bytes,
                    r.network_download_threshold_unit,
                    "app_suspension.download",
                ),
                (
                    true,
                    r.network_upload_threshold_bytes,
                    r.network_upload_threshold_unit,
                    "app_suspension.upload",
                ),
            ] {
                let value = unit.threshold_value_from_bytes(bytes).to_string();
                card = card.push(
                    row![
                        text(t!(label).to_string()).width(150),
                        text_input("", &value)
                            .on_input_maybe(
                                (s.enabled
                                    && r.enabled
                                    && s.network_wake_enabled
                                    && r.network_wake_enabled)
                                    .then_some(move |v| Message::Threshold(i, upload, v))
                            )
                            .width(140),
                        pick_list(
                            NetworkThresholdUnit::ALL.map(Unit),
                            Some(Unit(unit)),
                            move |v| Message::Unit(i, upload, v.0)
                        )
                    ]
                    .spacing(8),
                );
            }
            cards.push((
                super::motion::key(&r.executable_path),
                super::motion::removal(
                    card,
                    self.removed
                        .as_ref()
                        .is_some_and(|(_, removed)| removed.executable_path == r.executable_path),
                    motion_enabled,
                    Message::Removed(r.executable_path.clone()),
                ),
            ));
        }
        body = body.push(iced::widget::keyed_column(cards).spacing(8));
        if s.suspendable_apps.is_empty() {
            body = body.push(text(t!("app_suspension.no_suspendable").to_string()));
        }
        if self.removing.is_some() {
            body = body.push(
                row![
                    button(text(t!("common.remove").to_string())).on_press(Message::ConfirmRemove),
                    button(text(t!("common.cancel").to_string())).on_press(Message::CancelRemove)
                ]
                .spacing(8),
            );
        }
        if let Some(error) = &status.last_error {
            body = body.push(text(error.clone()));
        }
        scrollable(body).height(Fill).into()
    }
}
fn delay_row(field: Delay, label: &str, value: u64, enabled: bool) -> Element<'static, Message> {
    row![
        text(t!(label).to_string()).width(Fill),
        text_input("", &value.to_string())
            .on_input_maybe(enabled.then_some(move |v| Message::Delay(field, v)))
            .width(130),
        text("sec")
    ]
    .spacing(8)
    .into()
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
