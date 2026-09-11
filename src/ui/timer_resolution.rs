use super::design;
use super::widgets::{checkbox, text_input};
use crate::config::TimerResolutionSettings;
use crate::timer_resolution::TimerResolutionSnapshot;
use crate::ui::process_rules::{can_add_timer_resolution_process, new_timer_resolution_rule};
use iced::widget::{column, scrollable, text};
use iced::{Element, Fill};
use rust_i18n::t;
#[derive(Default)]
pub(super) struct Editor {
    pub(super) path: String,
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
            Message::Add if can_add_timer_resolution_process(s, &self.path) => {
                s.rules
                    .push(new_timer_resolution_rule(&self.path, s.desired_100ns));
                self.path.clear();
            }
            Message::RuleEnabled(i, v) => {
                if let Some(r) = s.rules.get_mut(i) {
                    r.enabled = v;
                }
            }
            Message::Resolution(i, v) => {
                self.editing = Some((i, v.clone()));
                self.update(s, status, Message::Commit(i));
                self.editing = Some((i, v));
            }
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
            Message::Remove(i) => {
                if i < s.rules.len() {
                    s.rules.remove(i);
                    self.editing = None;
                }
            }

            Message::Add | Message::Browse => {}
        }
    }
    pub(super) fn view<'a>(
        &'a self,
        s: &'a TimerResolutionSettings,
        status: &TimerResolutionSnapshot,
        candidates: &[super::app_picker::Candidate],
    ) -> Element<'a, Message> {
        let mut body = column![
            super::widgets::settings_card(super::widgets::setting_row(
                "timer_resolution.enable",
                super::widgets::switch(s.enabled, Some(Message::Enabled))
            )),
            text(t!("timer_resolution.warning").to_string()).style(text::secondary),
            super::widgets::setting_title("common.rules"),
            super::app_picker::view(
                &self.path,
                candidates,
                true,
                Message::Path,
                Message::Browse,
                (can_add_timer_resolution_process(s, &self.path)).then_some(Message::Add),
                |path| can_add_timer_resolution_process(s, path).then_some(true)
            )
        ]
        .spacing(super::widgets::CARD_GAP);
        let mut cards = Vec::new();
        for (i, r) in s.rules.iter().enumerate() {
            let value = self
                .editing
                .as_ref()
                .filter(|(index, _)| *index == i)
                .map(|(_, v)| v.clone())
                .unwrap_or_else(|| {
                    crate::timer_resolution::format_resolution_ms(r.desired_100ns)
                        .trim_end_matches(" ms")
                        .to_owned()
                });
            let controls = text_input("", &value)
                .align_x(iced::alignment::Horizontal::Center)
                .on_input_maybe(Some(move |v| Message::Resolution(i, v)))
                .on_submit(Message::Commit(i))
                .width(design::STANDALONE_NUMERIC_WIDTH);
            cards.push((
                super::widgets::stable_key(&r.executable_path),
                super::widgets::process_rule_row(
                    &r.executable_path,
                    candidates,
                    checkbox(r.enabled)
                        .on_toggle_maybe(Some(move |v| Message::RuleEnabled(i, v)))
                        .into(),
                    vec![controls.into()],
                    Some(Message::Remove(i)),
                ),
            ));
        }
        body = body.push(super::widgets::process_rules_table(
            [super::widgets::label_with_unit(
                &t!("timer_resolution.requested"),
                "ms",
            )],
            cards,
            t!("timer_resolution.no_rules").to_string(),
        ));

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
    fn valid_edits_update_the_draft_without_an_apply_button() {
        let mut editor = Editor::default();
        let mut settings = TimerResolutionSettings::default();
        settings
            .rules
            .push(new_timer_resolution_rule(r"C:\Apps\test.exe", 10_000));
        let status = TimerResolutionSnapshot::default();
        editor.update(
            &mut settings,
            &status,
            Message::Resolution(0, "2.00".into()),
        );
        assert_eq!(settings.rules[0].desired_100ns, 20_000);
        editor.update(&mut settings, &status, Message::Resolution(0, "".into()));
        assert_eq!(settings.rules[0].desired_100ns, 20_000);
    }
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
