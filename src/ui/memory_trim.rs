use super::design;
use super::widgets::{checkbox, text_input};
use crate::config::MemoryTrimSettings;
use crate::ui::process_rules::{can_add_memory_trim_exclusion, new_process_exclusion_rule};
use iced::widget::{column, scrollable, text};
use iced::{Element, Fill};
use rust_i18n::t;

#[derive(Default)]
pub(super) struct Editor {
    pub(super) path: String,
    collapsed: [bool; 3],
}
#[derive(Debug, Clone)]
pub(super) enum Message {
    Enabled(bool),
    Collapse(usize),
    Path(String),
    Add,
    Load(String),
    WorkingSet(String),
    Idle(String),
    RuleEnabled(usize, bool),
    Remove(usize),

    TrimNow,
    Browse,
}
impl Editor {
    pub(super) fn update(&mut self, settings: &mut MemoryTrimSettings, message: Message) {
        match message {
            Message::Collapse(i) => {
                if let Some(value) = self.collapsed.get_mut(i) {
                    *value = !*value;
                }
            }
            Message::Enabled(v) => settings.enabled = v,
            Message::Path(v) => self.path = v,
            Message::Add
                if settings.enabled && can_add_memory_trim_exclusion(settings, &self.path) =>
            {
                settings
                    .exclusions
                    .push(new_process_exclusion_rule(&self.path));
                self.path.clear();
            }
            Message::Load(v) => {
                if let Ok(v) = v.parse::<u8>() {
                    if (1..=100).contains(&v) {
                        settings.system_memory_load_threshold_percent = v;
                    }
                }
            }
            Message::WorkingSet(v) => {
                if let Ok(v) = v.parse::<u64>() {
                    if (1..=1_048_576).contains(&v) {
                        settings.process_working_set_threshold_mb = v;
                    }
                }
            }
            Message::Idle(v) => {
                if let Ok(v) = v.parse::<u64>() {
                    if (1..=86_400).contains(&v) {
                        settings.process_idle_seconds = v;
                    }
                }
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
        candidates: &[super::app_picker::Candidate],
    ) -> Element<'a, Message> {
        let mut body = column![super::widgets::settings_card(super::widgets::setting_row(
            "memory_trim.enable",
            super::widgets::switch(settings.enabled, Some(Message::Enabled))
        )),]
        .spacing(super::widgets::CARD_GAP);
        let field = |label: &str, unit: &str, value: String, change: fn(String) -> Message| {
            super::widgets::setting_row_with_unit(
                label,
                unit,
                text_input("", &value)
                    .on_input_maybe(settings.enabled.then_some(change))
                    .width(design::STANDALONE_NUMERIC_WIDTH),
            )
        };
        for (index, label, content) in [
            (
                0,
                "memory_trim.category_thresholds",
                column![
                    field(
                        "memory_trim.memory_threshold",
                        "%",
                        settings.system_memory_load_threshold_percent.to_string(),
                        Message::Load
                    ),
                    field(
                        "memory_trim.working_set_threshold",
                        "MiB",
                        settings.process_working_set_threshold_mb.to_string(),
                        Message::WorkingSet
                    )
                ]
                .spacing(design::space::MEDIUM),
            ),
            (
                1,
                "memory_trim.category_when_to_trim",
                column![field(
                    "memory_trim.idle_time",
                    "s",
                    settings.process_idle_seconds.to_string(),
                    Message::Idle
                )],
            ),
        ] {
            body = body.push(super::widgets::setting_group(
                label.to_string(),
                !self.collapsed[index],
                Message::Collapse(index),
                iced::widget::Space::new(),
                content,
            ));
        }
        let mut safety = column![
            text(t!("memory_trim.category_safety_help").to_string()).style(text::secondary),
            super::app_picker::view(
                &self.path,
                candidates,
                settings.enabled,
                Message::Path,
                Message::Browse,
                (settings.enabled && can_add_memory_trim_exclusion(settings, &self.path))
                    .then_some(Message::Add),
                |path| can_add_memory_trim_exclusion(settings, path).then_some(true)
            )
        ]
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
                            .on_toggle_maybe(
                                settings
                                    .enabled
                                    .then_some(move |v| Message::RuleEnabled(i, v)),
                            )
                            .into(),
                        vec![],
                        Some(Message::Remove(i)),
                    ),
                )
            })
            .collect();
        safety = safety.push(super::widgets::process_rules_table(
            [],
            cards,
            t!("memory_trim.no_exclusions").to_string(),
        ));

        body = body.push(super::widgets::setting_group(
            "memory_trim.category_safety".to_string(),
            !self.collapsed[2],
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
    fn groups_collapse_independently_without_changing_settings() {
        let mut editor = Editor::default();
        let mut settings = MemoryTrimSettings::default();
        let before = settings.clone();
        editor.update(&mut settings, Message::Collapse(0));
        editor.update(&mut settings, Message::Collapse(2));
        assert_eq!(editor.collapsed, [true, false, true]);
        editor.update(&mut settings, Message::Collapse(0));
        assert_eq!(editor.collapsed, [false, false, true]);
        assert_eq!(settings, before);
    }
    #[test]
    fn invalid_thresholds_and_missing_removals_do_not_mutate_settings() {
        let mut e = Editor::default();
        let mut s = crate::config::Settings::default().memory_trim;
        let before = s.clone();
        e.update(&mut s, Message::Load("0".into()));
        e.update(&mut s, Message::Idle("86401".into()));
        e.update(&mut s, Message::WorkingSet("-1".into()));
        e.update(&mut s, Message::Remove(0));
        assert_eq!(s, before);
        let path = r"C:\Apps\test.exe";
        s.exclusions.push(new_process_exclusion_rule(path));
        e.update(&mut s, Message::Remove(0));
        assert!(s.exclusions.is_empty());
    }
}
