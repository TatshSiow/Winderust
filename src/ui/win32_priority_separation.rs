use super::design;
use super::widgets::{action_button, pick_list};
use crate::application::{Win32PrioritySeparationError, Win32PrioritySeparationService};
use crate::ui::scrolling::scrollable;
use iced::widget::{column, row, text};
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
impl Field {
    const ALL: [Self; 3] = [Self::Duration, Self::Behaviour, Self::Boost];

    fn descriptor(self) -> (&'static str, u32, &'static [Choice]) {
        macro_rules! key {
            ($suffix:literal) => {
                concat!("settings.win32_priority_separation_", $suffix)
            };
        }
        match self {
            Self::Duration => (
                key!("quantum_duration"),
                0x30,
                &[
                    Choice(0x20, key!("quantum_duration_short")),
                    Choice(0x10, key!("quantum_duration_long")),
                ],
            ),
            Self::Behaviour => (
                key!("quantum_behaviour"),
                0x0c,
                &[
                    Choice(0x04, key!("quantum_behaviour_variable")),
                    Choice(0x08, key!("quantum_behaviour_fixed")),
                ],
            ),
            Self::Boost => (
                key!("foreground_boost"),
                0x03,
                &[
                    Choice(0, key!("foreground_boost_none")),
                    Choice(1, key!("foreground_boost_medium")),
                    Choice(2, key!("foreground_boost_high")),
                ],
            ),
        }
    }
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
                let (_, mask, choices) = field.descriptor();
                if choices.iter().any(|choice| choice.0 == bits) {
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
        let display = |value: String| {
            iced::widget::container(text(value))
                .padding([8, 12])
                .style(|theme: &iced::Theme| {
                    let mut style = super::widgets::surface(theme);
                    style.border.width = 1.0;
                    style.border.color = theme.extended_palette().background.strong.color;
                    style
                })
        };
        let mut body = column![
            super::widgets::heading(
                t!("settings.win32_priority_separation_saved_values").to_string(),
                design::typography::BODY
            ),
            super::widgets::settings_card(super::widgets::settings_rows([
                super::widgets::setting_row(
                    "settings.win32_priority_separation_current",
                    display(self.current.map(format_value).unwrap_or_else(|| {
                        t!("settings.win32_priority_separation_unavailable").to_string()
                    }))
                )
                .into(),
                super::widgets::setting_row(
                    "settings.win32_priority_separation_backup",
                    display(self.backup.map(format_value).unwrap_or_else(|| {
                        t!("settings.win32_priority_separation_no_backup").to_string()
                    }))
                )
                .into(),
            ])),
            super::widgets::heading(
                t!("settings.win32_priority_separation_scheduler_policy").to_string(),
                design::typography::BODY
            )
        ]
        .spacing(super::widgets::CARD_GAP);
        let value = normalize(self.value);
        let policy = Field::ALL.into_iter().map(|field| {
            let (key, mask, choices) = field.descriptor();
            let selected = choices
                .iter()
                .find(|choice| choice.0 == value & mask)
                .copied();
            super::widgets::setting_row(
                key,
                pick_list(choices, selected, move |choice| {
                    Message::Field(field, choice.0)
                })
                .width(design::SELECT_WIDTH),
            )
            .into()
        });
        let policy = policy.chain([super::widgets::setting_row(
            "settings.win32_priority_separation_resulting_value",
            display(format!("0x{value:02X} ({value})")),
        )
        .into()]);
        body = body
            .push(super::widgets::settings_card(
                column![
                    text(t!("settings.win32_priority_separation_warning").to_string())
                        .size(design::typography::SECONDARY)
                        .style(text::secondary),
                    super::widgets::settings_rows(policy),
                ]
                .spacing(design::space::MEDIUM),
            ))
            .push(
                row![
                    iced::widget::Space::new().width(Fill),
                    action_button(t!("settings.refresh").to_string()).on_press(Message::Refresh),
                    action_button(t!("settings.save_backup").to_string()).on_press(Message::Backup),
                    action_button(t!("settings.restore_backup").to_string())
                        .on_press_maybe(self.backup.map(|_| Message::Restore)),
                    action_button(t!("settings.apply").to_string())
                        .style(super::widgets::primary_button)
                        .on_press(Message::Apply)
                ]
                .spacing(design::space::SMALL)
                .align_y(iced::Center),
            )
            .push(text(&self.status).style(text::secondary));
        scrollable(body).height(Fill).into()
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Choice(u32, &'static str);
impl std::fmt::Display for Choice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&t!(self.1))
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
