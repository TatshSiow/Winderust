use super::design;
use super::widgets::{checkbox, text_input};
use crate::config::MemoryTrimSettings;
use crate::ui::process_rules::{can_add_memory_trim_exclusion, new_process_exclusion_rule};
use crate::ui::scrolling::scrollable;
use iced::widget::column;
use iced::{Element, Fill};
use rust_i18n::t;

#[derive(Default)]
pub(super) struct Editor {
    pub(super) path: String,
    expanded: [bool; 3],
    thresholds: [Option<String>; 3],
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Threshold {
    Load,
    WorkingSet,
    Idle,
}
impl Threshold {
    const ALL: [Self; 3] = [Self::Load, Self::WorkingSet, Self::Idle];

    fn label_and_maximum(self) -> (&'static str, u64) {
        match self {
            Self::Load => ("memory_trim.memory_threshold", 100),
            Self::WorkingSet => ("memory_trim.working_set_threshold", 1_000_000),
            Self::Idle => ("memory_trim.idle_time", 86_400),
        }
    }

    fn parse(self, raw: &str) -> Option<u64> {
        raw.parse()
            .ok()
            .filter(|value| (1..=self.label_and_maximum().1).contains(value))
    }

    pub(super) fn error_message(self) -> String {
        let (label, maximum) = self.label_and_maximum();
        t!(
            "memory_trim.invalid_threshold",
            setting = t!(label),
            maximum = maximum
        )
        .to_string()
    }
}
#[derive(Debug, Clone)]
pub(super) enum Message {
    Enabled(bool),
    Collapse(usize),
    Path(String),
    Add,
    Threshold(Threshold, String),
    RuleEnabled(usize, bool),
    Remove(usize),

    TrimNow,
    Browse,
}
impl Editor {
    pub(super) fn invalid_threshold(&self) -> Option<Threshold> {
        Threshold::ALL.into_iter().find(|&field| {
            self.thresholds[field as usize]
                .as_deref()
                .is_some_and(|raw| field.parse(raw).is_none())
        })
    }

    pub(super) fn update(&mut self, settings: &mut MemoryTrimSettings, message: Message) {
        match message {
            Message::Collapse(i) => {
                if let Some(value) = self.expanded.get_mut(i) {
                    *value = !*value;
                }
            }
            Message::Enabled(v) => settings.enabled = v,
            Message::Path(v) => self.path = v,
            Message::Add if can_add_memory_trim_exclusion(settings, &self.path) => {
                settings
                    .exclusions
                    .push(new_process_exclusion_rule(&self.path));
                self.path.clear();
            }
            Message::Threshold(field, raw) => {
                if let Some(value) = field.parse(&raw) {
                    match field {
                        Threshold::Load => {
                            settings.system_memory_load_threshold_percent = value as u8
                        }
                        Threshold::WorkingSet => settings.process_working_set_threshold_mb = value,
                        Threshold::Idle => settings.process_idle_seconds = value,
                    }
                }
                self.thresholds[field as usize] = Some(raw);
            }
            Message::RuleEnabled(i, v) => {
                if let Some(r) = settings.exclusions.get_mut(i) {
                    r.enabled = v;
                }
            }
            Message::Remove(i) => {
                if i < settings.exclusions.len() {
                    settings.exclusions.remove(i);
                }
            }

            Message::Add | Message::TrimNow | Message::Browse => {}
        }
    }
    pub(super) fn view<'a>(
        &'a self,
        settings: &'a MemoryTrimSettings,
        candidates: &'a [super::app_picker::Candidate],
    ) -> Element<'a, Message> {
        let mut body = column![super::widgets::toggle_card(
            "memory_trim.enable",
            settings.enabled,
            Some(Message::Enabled)
        ),]
        .spacing(super::widgets::CARD_GAP);
        let field = |field: Threshold, unit: &str, value: u64| {
            let formatted;
            let raw = if let Some(raw) = &self.thresholds[field as usize] {
                raw
            } else {
                formatted = value.to_string();
                &formatted
            };
            super::widgets::setting_row_with_unit(
                field.label_and_maximum().0,
                unit,
                text_input("", raw)
                    .on_input(move |raw| Message::Threshold(field, raw))
                    .width(design::STANDALONE_NUMERIC_WIDTH),
            )
        };
        for (index, label, content) in [
            (
                0,
                "memory_trim.category_thresholds",
                super::widgets::settings_rows([
                    field(
                        Threshold::Load,
                        "%",
                        u64::from(settings.system_memory_load_threshold_percent),
                    )
                    .into(),
                    field(
                        Threshold::WorkingSet,
                        "MB",
                        settings.process_working_set_threshold_mb,
                    )
                    .into(),
                ]),
            ),
            (
                1,
                "memory_trim.category_when_to_trim",
                column![field(Threshold::Idle, "s", settings.process_idle_seconds,)],
            ),
        ] {
            body = body.push(super::widgets::setting_group(
                label.to_string(),
                self.expanded[index],
                Message::Collapse(index),
                iced::widget::Space::new(),
                content,
            ));
        }
        let mut safety = column![super::app_picker::view(
            &self.path,
            candidates,
            true,
            Message::Path,
            Message::Browse,
            (can_add_memory_trim_exclusion(settings, &self.path)).then_some(Message::Add),
            |path| can_add_memory_trim_exclusion(settings, path).then_some(true)
        )]
        .spacing(design::space::MEDIUM);
        let cards = settings
            .exclusions
            .iter()
            .enumerate()
            .map(|(i, r)| {
                (
                    super::widgets::stable_key(&r.executable_path),
                    super::widgets::process_rule_row(
                        &r.executable_path,
                        candidates,
                        checkbox(r.enabled)
                            .on_toggle_maybe(Some(move |v| Message::RuleEnabled(i, v)))
                            .into(),
                        vec![],
                        Some(Message::Remove(i)),
                    ),
                )
            })
            .collect();
        safety = safety.push(super::widgets::settings_divider()).push(
            super::widgets::process_rules_table(
                [],
                cards,
                t!("memory_trim.no_exclusions").to_string(),
            ),
        );

        body = body.push(super::widgets::setting_group(
            "memory_trim.category_safety".to_string(),
            self.expanded[2],
            Message::Collapse(2),
            iced::widget::Space::new(),
            safety,
        ));
        scrollable(body).height(Fill).into()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn groups_start_collapsed_and_toggle_without_changing_settings() {
        let mut editor = Editor::default();
        let mut settings = MemoryTrimSettings::default();
        let before = settings.clone();
        assert_eq!(editor.expanded, [false; 3]);
        editor.update(&mut settings, Message::Collapse(0));
        editor.update(&mut settings, Message::Collapse(2));
        assert_eq!(editor.expanded, [true, false, true]);
        editor.update(&mut settings, Message::Collapse(0));
        assert_eq!(editor.expanded, [false, false, true]);
        assert_eq!(settings, before);
    }
    #[test]
    fn invalid_thresholds_and_missing_removals_do_not_mutate_settings() {
        let mut e = Editor::default();
        let mut s = crate::config::Settings::default().memory_trim;
        let before = s.clone();
        e.update(&mut s, Message::Threshold(Threshold::Load, "0".into()));
        e.update(&mut s, Message::Threshold(Threshold::Idle, "86401".into()));
        e.update(
            &mut s,
            Message::Threshold(Threshold::WorkingSet, "-1".into()),
        );
        e.update(&mut s, Message::Remove(0));
        assert_eq!(s, before);
        assert_eq!(e.invalid_threshold(), Some(Threshold::Load));
        e.update(&mut s, Message::Threshold(Threshold::Load, "100".into()));
        assert_eq!(s.system_memory_load_threshold_percent, 100);
        assert_eq!(e.invalid_threshold(), Some(Threshold::WorkingSet));
        e.update(
            &mut s,
            Message::Threshold(Threshold::WorkingSet, "1".into()),
        );
        assert_eq!(s.process_working_set_threshold_mb, 1);
        assert_eq!(e.invalid_threshold(), Some(Threshold::Idle));
        e.update(&mut s, Message::Threshold(Threshold::Idle, "10".into()));
        assert_eq!(s.process_idle_seconds, 10);
        assert_eq!(e.invalid_threshold(), None);
        let path = r"C:\Apps\test.exe";
        s.exclusions.push(new_process_exclusion_rule(path));
        e.update(&mut s, Message::Remove(0));
        assert!(s.exclusions.is_empty());
    }
}
