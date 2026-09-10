use super::design;
use super::widgets::{button, checkbox, pick_list, text_input};
use crate::config::TimerResolutionSettings;
use crate::timer_resolution::TimerResolutionSnapshot;
use crate::ui::process_rules::{can_add_timer_resolution_process, new_timer_resolution_rule};
use iced::widget::{column, row, scrollable, text};
use iced::{Element, Fill};
use rust_i18n::t;
#[derive(Default)]
pub(super) struct Editor {
    pub(super) path: String,
    removing: Option<usize>,
    editing: Option<(usize, String)>,
}
#[derive(Debug, Clone)]
pub(super) enum Message {
    Enabled(bool),
    Path(String),
    Add,
    RuleEnabled(usize, bool),
    Resolution(usize, String),
    Commit(usize),
    Remove(usize),
    ConfirmRemove,
    CancelRemove,
    Browse,
}
impl Editor {
    pub(super) fn update(
        &mut self,
        s: &mut TimerResolutionSettings,
        status: &TimerResolutionSnapshot,
        m: Message,
    ) {
        match m {
            Message::Enabled(v) => s.enabled = v,
            Message::Path(v) => self.path = v,
            Message::Add if s.enabled && can_add_timer_resolution_process(s, &self.path) => {
                s.rules
                    .push(new_timer_resolution_rule(&self.path, s.desired_100ns));
                self.path.clear();
            }
            Message::RuleEnabled(i, v) => {
                if let Some(r) = s.rules.get_mut(i) {
                    r.enabled = v;
                }
            }
            Message::Resolution(i, v) => self.editing = Some((i, v)),
            Message::Commit(i) => {
                if let Some((index, value)) = &self.editing {
                    if *index == i {
                        if let (Some(r), Some(v)) = (
                            s.rules.get_mut(i),
                            parse_resolution(
                                value,
                                status.minimum_100ns.unwrap_or(1_000),
                                status.maximum_100ns.unwrap_or(10_000_000),
                            ),
                        ) {
                            r.desired_100ns = v;
                            self.editing = None;
                        }
                    }
                }
            }
            Message::Remove(i) => self.removing = Some(i),
            Message::CancelRemove => self.removing = None,
            Message::ConfirmRemove => {
                if let Some(i) = self.removing.take().filter(|i| *i < s.rules.len()) {
                    s.rules.remove(i);
                }
            }

            Message::Add | Message::Browse => {}
        }
    }
    pub(super) fn view<'a>(
        &'a self,
        s: &'a TimerResolutionSettings,
        status: &TimerResolutionSnapshot,
        candidates: &[String],
    ) -> Element<'a, Message> {
        let mut body = column![
            super::widgets::settings_card(super::widgets::setting_row(
                "timer_resolution.enable",
                super::widgets::switch(s.enabled, Some(Message::Enabled))
            )),
            text(t!("timer_resolution.warning").to_string()),
            super::widgets::settings_card(
                row![
                    text_input(&t!("process_list.executable_path"), &self.path)
                        .on_input(Message::Path),
                    button(text(t!("common.browse_executable").to_string()))
                        .on_press(Message::Browse),
                    button(text(t!("common.add").to_string())).on_press_maybe(
                        (s.enabled && can_add_timer_resolution_process(s, &self.path))
                            .then_some(Message::Add)
                    )
                ]
                .spacing(design::space::SMALL)
                .align_y(iced::Center)
            )
        ]
        .spacing(super::widgets::CARD_GAP);
        let matching = candidates
            .iter()
            .filter(|p| p.to_lowercase().contains(&self.path.to_lowercase()))
            .cloned()
            .collect::<Vec<_>>();
        if s.enabled && !matching.is_empty() {
            body = body.push(
                pick_list(matching, None::<String>, Message::Path)
                    .placeholder(t!("common.running").to_string()),
            );
        }
        let mut cards = Vec::new();
        for (i, r) in s.rules.iter().enumerate() {
            let value = self
                .editing
                .as_ref()
                .filter(|(index, _)| *index == i)
                .map(|(_, v)| v.clone())
                .unwrap_or_else(|| format!("{}", f64::from(r.desired_100ns) / 10_000.0));
            cards.push((
                super::widgets::stable_key(&r.executable_path),
                super::widgets::settings_card(
                    row![
                        checkbox(r.enabled)
                            .label(r.executable_path.clone())
                            .on_toggle_maybe(
                                s.enabled.then_some(move |v| Message::RuleEnabled(i, v))
                            ),
                        text_input(&t!("timer_resolution.requested"), &value)
                            .on_input(move |v| Message::Resolution(i, v))
                            .on_submit(Message::Commit(i))
                            .width(100),
                        text("ms"),
                        button(text(t!("settings.apply").to_string())).on_press(Message::Commit(i)),
                        button(text(t!("common.remove").to_string())).on_press(Message::Remove(i))
                    ]
                    .spacing(design::space::SMALL)
                    .align_y(iced::Center),
                )
                .into(),
            ));
        }
        body = body.push(iced::widget::keyed_column(cards).spacing(super::widgets::CARD_GAP));
        if s.rules.is_empty() {
            body = body.push(text(t!("timer_resolution.no_rules").to_string()));
        }
        if self.removing.is_some() {
            body = body.push(super::widgets::settings_card(
                row![
                    button(text(t!("common.remove").to_string())).on_press(Message::ConfirmRemove),
                    button(text(t!("common.cancel").to_string())).on_press(Message::CancelRemove)
                ]
                .spacing(design::space::SMALL)
                .align_y(iced::Center),
            ));
        }
        if let Some(value) = status.requested_100ns {
            body = body.push(text(format!(
                "{}: {}",
                t!("timer_resolution.requested"),
                crate::timer_resolution::format_resolution_ms(value)
            )));
        }
        if let Some(error) = &status.last_error {
            body = body.push(text(error.clone()));
        }
        scrollable(body).height(Fill).into()
    }
}
fn parse_resolution(value: &str, min: u32, max: u32) -> Option<u32> {
    let value = value.trim();
    let value = value
        .strip_suffix("ms")
        .or_else(|| value.strip_suffix("MS"))
        .or_else(|| value.strip_suffix("Ms"))
        .or_else(|| value.strip_suffix("mS"))
        .unwrap_or(value)
        .trim()
        .parse::<f64>()
        .ok()?;
    if !value.is_finite() || value <= 0.0 {
        return None;
    }
    Some(crate::timer_resolution::normalize_desired_resolution(
        (value.clamp(0.1, 1000.0) * 10_000.0).round() as u32,
        min,
        max,
    ))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn timer_input_is_bounded_and_rejects_nonfinite() {
        assert_eq!(parse_resolution("0.5 ms", 10_000, 160_000), Some(10_000));
        assert_eq!(
            parse_resolution("15.625 MS", 10_000, 160_000),
            Some(160_000)
        );
        assert_eq!(parse_resolution("NaN", 10_000, 160_000), None);
    }
}
