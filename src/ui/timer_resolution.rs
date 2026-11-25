use super::design;
use super::widgets::{checkbox, text_input};
use crate::application::SettingsEditor;
use crate::config::TimerResolutionSettings;
use crate::timer_resolution::TimerResolutionSnapshot;
use crate::ui::process_rules::{
    can_add_timer_resolution_process, exact_path_input, new_timer_resolution_rule,
    picked_path_input,
};
use crate::ui::scrolling::PageScroll;
use iced::widget::{column, text};
use iced::{Element, Fill};
use rust_i18n::t;
#[derive(Default)]
pub(super) struct Editor {
    pub(super) path: String,
    drafts: Vec<Option<String>>,
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
    pub(super) fn has_invalid_inputs(&self) -> bool {
        self.drafts
            .iter()
            .flatten()
            .any(|value| draft_resolution_is_invalid(value))
    }

    pub(super) fn update(
        &mut self,
        s: &mut SettingsEditor,
        status: &TimerResolutionSnapshot,
        m: Message,
    ) {
        let parse = |value: &str| {
            parse_resolution(
                value,
                status.minimum_100ns.unwrap_or(1_000),
                status.maximum_100ns.unwrap_or(10_000_000),
            )
        };
        match m {
            Message::Enabled(v) if s.timer_resolution.enabled != v => {
                s.timer_resolution.enabled = v;
            }
            Message::Path(v) => self.path = v,
            Message::Add => {
                let path = exact_path_input(&self.path);
                if can_add_timer_resolution_process(&s.timer_resolution, path) {
                    let timer = &mut s.timer_resolution;
                    timer
                        .rules
                        .push(new_timer_resolution_rule(path, timer.desired_100ns));
                    self.path.clear();
                }
            }
            Message::RuleEnabled(i, v)
                if s.timer_resolution
                    .rules
                    .get(i)
                    .is_some_and(|r| r.enabled != v) =>
            {
                s.timer_resolution.rules[i].enabled = v;
            }
            Message::Resolution(i, value) => {
                let rule_count = s.timer_resolution.rules.len();
                if let Some(rule) = s.timer_resolution.rules.get(i) {
                    if let Some(resolution) = parse(&value).filter(|&v| v != rule.desired_100ns) {
                        s.timer_resolution.rules[i].desired_100ns = resolution;
                    }
                    self.drafts.resize(rule_count, None);
                    self.drafts[i] = Some(value);
                }
            }
            Message::Commit(i) => {
                if let (Some(rule), Some(draft)) =
                    (s.timer_resolution.rules.get(i), self.drafts.get_mut(i))
                {
                    if let Some(resolution) = draft.as_deref().and_then(parse) {
                        if rule.desired_100ns != resolution {
                            s.timer_resolution.rules[i].desired_100ns = resolution;
                        }
                        *draft = None;
                    }
                }
            }
            Message::Remove(i) if i < s.timer_resolution.rules.len() => {
                s.timer_resolution.rules.remove(i);
                if i < self.drafts.len() {
                    self.drafts.remove(i);
                }
            }
            Message::Enabled(_)
            | Message::RuleEnabled(..)
            | Message::Remove(_)
            | Message::Browse => {}
        }
    }
    pub(super) fn view<'a>(
        &'a self,
        s: &'a TimerResolutionSettings,
        status: &TimerResolutionSnapshot,
        candidates: &'a [super::app_picker::Candidate],
        page_scroll: PageScroll,
    ) -> Element<'a, Message> {
        let mut body = column![
            super::widgets::toggle_card(
                "timer_resolution.enable",
                s.enabled,
                Some(Message::Enabled)
            ),
            super::widgets::setting_title_with_help("common.rules", "", "timer_resolution.intro_2"),
            super::widgets::settings_card(super::app_picker::view_with_path_selection(
                &self.path,
                candidates,
                true,
                (Message::Path, |path| Message::Path(picked_path_input(path))),
                Message::Browse,
                can_add_timer_resolution_process(s, exact_path_input(&self.path))
                    .then_some(Message::Add),
                |path| can_add_timer_resolution_process(s, path).then_some(true)
            ))
        ]
        .spacing(super::widgets::CARD_GAP);
        let mut cards = Vec::new();
        for (i, r) in s.rules.iter().enumerate() {
            let formatted;
            let draft = self.drafts.get(i).and_then(Option::as_deref);
            let value = if let Some(draft) = draft {
                draft
            } else {
                formatted = crate::timer_resolution::format_resolution_ms(r.desired_100ns);
                formatted.trim_end_matches(" ms")
            };
            let controls = text_input("", value)
                .align_x(iced::alignment::Horizontal::Center)
                .on_input(move |v| Message::Resolution(i, v))
                .on_submit(Message::Commit(i))
                .width(design::STANDALONE_NUMERIC_WIDTH);
            let mut controls = column![controls].spacing(design::space::SMALL);
            if draft.is_some_and(draft_resolution_is_invalid) {
                controls = controls.push(
                    text(t!("timer_resolution.invalid_request").to_string())
                        .wrapping(text::Wrapping::WordOrGlyph)
                        .style(text::danger),
                );
            }
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
            false,
        ));

        if let Some(error) = &status.last_error {
            body = body.push(text(error.clone()));
        }
        page_scroll.scrollable(body).height(Fill).into()
    }
}
fn draft_resolution_is_invalid(value: &str) -> bool {
    parse_resolution(value, 1_000, 10_000_000).is_none()
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
    use crate::config::{PowerSourceProfile, Settings};

    #[test]
    fn picked_timer_rule_preserves_a_distinct_executable_path_on_save() {
        let mut editor = Editor::default();
        let ordinary = r"C:\Apps\test.exe";
        let literal = concat!(r"C:\Apps\test.exe", " ");
        let mut settings = Settings::default();
        settings
            .timer_resolution
            .rules
            .push(new_timer_resolution_rule(ordinary, 10_000));
        let mut settings = SettingsEditor::with_settings(settings);
        let status = TimerResolutionSnapshot::default();
        editor.update(
            &mut settings,
            &status,
            Message::Path(picked_path_input(literal.into())),
        );
        editor.update(&mut settings, &status, Message::Add);

        let saved = toml::to_string(&*settings).unwrap();
        let reloaded: Settings = toml::from_str(&saved).unwrap();
        let rules = &reloaded.timer_resolution.rules;
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].executable_path, ordinary);
        assert_eq!(rules[1].executable_path, literal);
    }

    #[test]
    fn valid_edits_update_the_draft_without_an_apply_button() {
        let mut editor = Editor::default();
        let mut settings = Settings::default();
        settings
            .timer_resolution
            .rules
            .push(new_timer_resolution_rule(r"C:\Apps\test.exe", 10_000));
        let mut settings = SettingsEditor::with_settings(settings);
        settings.select_power_source(PowerSourceProfile::OnBattery);
        let status = TimerResolutionSnapshot::default();
        editor.update(&mut settings, &status, Message::Commit(0));
        editor.update(
            &mut settings,
            &status,
            Message::Resolution(0, "01.00 ms".into()),
        );
        assert_eq!(editor.drafts[0].as_deref(), Some("01.00 ms"));
        editor.update(&mut settings, &status, Message::Commit(0));
        assert_eq!(editor.drafts[0], None);
        assert!(!settings.has_unsaved_changes());
        editor.update(
            &mut settings,
            &status,
            Message::Resolution(0, "2.00".into()),
        );
        assert_eq!(settings.timer_resolution.rules[0].desired_100ns, 20_000);
        assert!(!editor.has_invalid_inputs());
        editor.update(&mut settings, &status, Message::Resolution(0, "".into()));
        assert!(editor.has_invalid_inputs());
        assert_eq!(settings.timer_resolution.rules[0].desired_100ns, 20_000);
    }
    #[test]
    fn invalid_input_survives_other_rule_edits_and_removal() {
        let mut editor = Editor::default();
        let mut settings = Settings::default();
        settings.timer_resolution.rules.extend([
            new_timer_resolution_rule(r"C:\Apps\first.exe", 20_000),
            new_timer_resolution_rule(r"C:\Apps\middle.exe", 30_000),
            new_timer_resolution_rule(r"C:\Apps\last.exe", 40_000),
        ]);
        let mut settings = SettingsEditor::with_settings(settings);
        let status = TimerResolutionSnapshot::default();
        editor.update(&mut settings, &status, Message::Resolution(1, "".into()));
        editor.update(&mut settings, &status, Message::Resolution(2, "5.5".into()));
        assert!(editor.has_invalid_inputs());
        assert_eq!(settings.timer_resolution.rules[1].desired_100ns, 30_000);
        assert_eq!(settings.timer_resolution.rules[2].desired_100ns, 60_000);

        editor.update(&mut settings, &status, Message::Remove(2));
        editor.update(&mut settings, &status, Message::Remove(0));
        assert!(editor.has_invalid_inputs());
        assert_eq!(editor.drafts[0].as_deref(), Some(""));
        assert_eq!(settings.timer_resolution.rules[0].desired_100ns, 30_000);

        editor.update(&mut settings, &status, Message::Resolution(0, "7.5".into()));
        assert!(!editor.has_invalid_inputs());
        assert_eq!(settings.timer_resolution.rules[0].desired_100ns, 80_000);
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
