use crate::application::{Win32PrioritySeparationError, Win32PrioritySeparationService};
use iced::widget::{button, column, pick_list, row, scrollable, text};
use iced::{Element, Fill};
use rust_i18n::t;
#[derive(Default)]
pub(super) struct Editor {
    service: Win32PrioritySeparationService,
    current: Option<u32>,
    backup: Option<u32>,
    value: u32,
    pub(super) status: String,
}
#[derive(Debug, Clone, Copy)]
pub(super) enum Field {
    Duration,
    Behaviour,
    Boost,
}
#[derive(Debug, Clone)]
pub(super) enum Message {
    Refresh,
    Backup,
    Restore,
    Apply,
    Field(Field, u32),
}
impl Editor {
    pub(super) fn refresh(&mut self) {
        let snapshot: crate::application::Win32PrioritySeparationSnapshot = self.service.snapshot();
        let mut errors = Vec::new();
        self.current = match snapshot.current {
            Ok(v) => v,
            Err(error) => {
                errors.push(error.to_string());
                None
            }
        };
        self.backup = match snapshot.backup {
            Ok(v) => v,
            Err(error) => {
                errors.push(error.to_string());
                None
            }
        };
        if let Some(v) = self.current {
            self.value = normalize(v);
        }
        self.status = if !errors.is_empty() {
            errors.join("\n")
        } else if let Some(v) = self.current {
            t!(
                "settings.win32_priority_separation_loaded",
                value = format_value(v)
            )
            .to_string()
        } else {
            t!("settings.win32_priority_separation_load_failed").to_string()
        };
    }

    pub(super) fn update(&mut self, m: Message) {
        match m {
            Message::Refresh => self.refresh(),
            Message::Field(field, bits) => {
                let (mask, allowed) = match field {
                    Field::Duration => (0x30, &[0x20, 0x10][..]),
                    Field::Behaviour => (0x0c, &[0x04, 0x08][..]),
                    Field::Boost => (0x03, &[0, 1, 2][..]),
                };
                if allowed.contains(&bits) {
                    self.value = (normalize(self.value) & !mask) | bits;
                }
            }
            Message::Backup => match self.service.save_backup() {
                Ok(v) => {
                    self.backup = Some(v);
                    self.status = t!(
                        "settings.win32_priority_separation_backup_saved",
                        value = format_value(v)
                    )
                    .to_string();
                }
                Err(error) => {
                    self.status = t!(
                        "settings.win32_priority_separation_backup_failed",
                        error = error
                    )
                    .to_string()
                }
            },
            Message::Apply => match self.service.apply(normalize(self.value)) {
                Ok(result) => {
                    self.current = Some(result.value);
                    self.value = result.value;
                    self.backup = Some(result.backup);
                    self.status = t!(
                        "settings.win32_priority_separation_saved",
                        value = format_value(result.value)
                    )
                    .to_string();
                }
                Err(error) => {
                    self.status = t!(
                        "settings.win32_priority_separation_save_failed",
                        error = error
                    )
                    .to_string()
                }
            },
            Message::Restore => match self.service.restore_backup() {
                Ok(v) => {
                    self.current = Some(v);
                    self.value = normalize(v);
                    self.status = t!(
                        "settings.win32_priority_separation_restored",
                        value = format_value(v)
                    )
                    .to_string();
                }
                Err(Win32PrioritySeparationError::BackupUnavailable) => {
                    self.backup = None;
                    self.status = t!("settings.win32_priority_separation_no_backup").to_string();
                }
                Err(error) => {
                    self.status = t!(
                        "settings.win32_priority_separation_restore_failed",
                        error = error
                    )
                    .to_string()
                }
            },
        }
    }
    pub(super) fn view(&self) -> Element<'_, Message> {
        let mut body = column![
            text(t!("settings.win32_priority_separation_warning").to_string()),
            text(format!(
                "{}: {}",
                t!("settings.win32_priority_separation_current"),
                self.current
                    .map(format_value)
                    .unwrap_or_else(
                        || t!("settings.win32_priority_separation_unavailable").to_string()
                    )
            )),
            text(format!(
                "{}: {}",
                t!("settings.win32_priority_separation_backup"),
                self.backup
                    .map(format_value)
                    .unwrap_or_else(
                        || t!("settings.win32_priority_separation_no_backup").to_string()
                    )
            ))
        ]
        .spacing(12);
        for (field, key, mask, bits) in [
            (Field::Duration, "quantum_duration", 0x30, vec![0x20, 0x10]),
            (
                Field::Behaviour,
                "quantum_behaviour",
                0x0c,
                vec![0x04, 0x08],
            ),
            (Field::Boost, "foreground_boost", 0x03, vec![0, 1, 2]),
        ] {
            let choices = bits
                .into_iter()
                .map(|bits| Choice(bits, field_label(field, bits)))
                .collect::<Vec<_>>();
            let selected = normalize(self.value) & mask;
            body = body.push(super::widgets::settings_card(
                row![
                    text(
                        {
                            let locale_key = format!("settings.win32_priority_separation_{key}");
                            t!(&locale_key).to_string()
                        }
                        .to_string()
                    )
                    .width(Fill),
                    pick_list(
                        choices,
                        Some(Choice(selected, field_label(field, selected))),
                        move |v| Message::Field(field, v.0)
                    )
                ]
                .spacing(8)
                .align_y(iced::Center),
            ));
            body = body.push(
                text(
                    {
                        let locale_key = format!("settings.win32_priority_separation_{key}_help");
                        t!(&locale_key).to_string()
                    }
                    .to_string(),
                )
                .size(12),
            );
        }
        body = body
            .push(text(format!(
                "{}: {}",
                t!("settings.win32_priority_separation_resulting_value"),
                format_value(normalize(self.value))
            )))
            .push(super::widgets::settings_card(
                row![
                    button(text(t!("settings.refresh").to_string())).on_press(Message::Refresh),
                    button(text(t!("settings.save_backup").to_string())).on_press(Message::Backup),
                    button(text(t!("settings.restore_backup").to_string()))
                        .on_press_maybe(self.backup.map(|_| Message::Restore)),
                    button(text(t!("settings.apply").to_string())).on_press(Message::Apply)
                ]
                .spacing(8)
                .align_y(iced::Center),
            ))
            .push(text(&self.status));
        scrollable(body).spacing(10).height(Fill).into()
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct Choice(u32, String);
impl std::fmt::Display for Choice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.1)
    }
}
fn normalize(v: u32) -> u32 {
    (match v & 0x30 {
        0x10 | 0x20 => v & 0x30,
        _ => 0x20,
    }) | (match v & 0x0c {
        0x04 | 0x08 => v & 0x0c,
        _ => 0x04,
    }) | (match v & 3 {
        0..=2 => v & 3,
        _ => 2,
    })
}
fn field_label(field: Field, bits: u32) -> String {
    let key = match (field, bits) {
        (Field::Duration, 0x10) => "quantum_duration_long",
        (Field::Duration, _) => "quantum_duration_short",
        (Field::Behaviour, 0x08) => "quantum_behaviour_fixed",
        (Field::Behaviour, _) => "quantum_behaviour_variable",
        (Field::Boost, 0) => "foreground_boost_none",
        (Field::Boost, 1) => "foreground_boost_medium",
        (Field::Boost, _) => "foreground_boost_high",
    };
    {
        let locale_key = format!("settings.win32_priority_separation_{key}");
        t!(&locale_key).to_string()
    }
    .to_string()
}
fn format_value(v: u32) -> String {
    let key = match v {
        0x14 => "long_variable_none",
        0x15 => "long_variable_medium",
        0x16 => "long_variable_high",
        0x18 => "long_fixed_none",
        0x19 => "long_fixed_medium",
        0x1a => "long_fixed_high",
        0x24 => "short_variable_none",
        0x25 => "short_variable_medium",
        0x26 => "short_variable_high",
        0x28 => "short_fixed_none",
        0x29 => "short_fixed_medium",
        0x2a => "short_fixed_high",
        _ => "custom",
    };
    format!("0x{v:02X} ({v}) - {}", {
        let locale_key = format!("settings.win32_priority_separation_desc_{key}");
        t!(&locale_key).to_string()
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn field_edits_preserve_other_fields_and_reject_invalid_bits() {
        let mut e = Editor {
            value: 0x26,
            ..Default::default()
        };
        e.update(Message::Field(Field::Duration, 0x10));
        assert_eq!(e.value, 0x16);
        e.update(Message::Field(Field::Boost, 3));
        assert_eq!(e.value, 0x16);
        assert_eq!(normalize(0xff), 0x26);
    }
}
