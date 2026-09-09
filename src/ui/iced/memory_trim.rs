use crate::config::MemoryTrimSettings;
use crate::ui::process_rules::{can_add_memory_trim_exclusion, new_process_exclusion_rule};
use iced::widget::{button, checkbox, column, pick_list, row, scrollable, text, text_input};
use iced::{Element, Fill};
use rust_i18n::t;

#[derive(Default)]
pub(super) struct Editor {
    pub(super) path: String,
    collapsed: [bool; 3],
    removing: Option<usize>,
    removed: Option<(usize, crate::config::ProcessExclusionRule)>,
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
    ConfirmRemove,
    Removed(String),
    CancelRemove,
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
                self.removed = None;
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
            Message::Remove(i) => self.removing = Some(i),
            Message::CancelRemove => self.removing = None,
            Message::ConfirmRemove => {
                if let Some(i) = self
                    .removing
                    .take()
                    .filter(|i| *i < settings.exclusions.len())
                {
                    self.removed = Some((i, settings.exclusions.remove(i)));
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

            Message::Add | Message::TrimNow | Message::Browse => {}
        }
    }
    pub(super) fn view<'a>(
        &'a self,
        settings: &'a MemoryTrimSettings,
        candidates: &[String],
        motion_enabled: bool,
    ) -> Element<'a, Message> {
        let mut body = column![super::widgets::settings_card(super::widgets::setting_row(
            "memory_trim.enable",
            super::widgets::switch(settings.enabled, Some(Message::Enabled))
        )),]
        .spacing(12);
        let field = |label: &str, value: String, change: fn(String) -> Message| {
            row![
                text(t!(label).to_string()).width(Fill),
                text_input("", &value)
                    .on_input_maybe(settings.enabled.then_some(change))
                    .width(140)
            ]
            .spacing(12)
            .align_y(iced::Center)
        };
        for (index, label, content) in [
            (
                0,
                "memory_trim.category_thresholds",
                column![
                    field(
                        "memory_trim.memory_threshold",
                        settings.system_memory_load_threshold_percent.to_string(),
                        Message::Load
                    ),
                    field(
                        "memory_trim.working_set_threshold",
                        settings.process_working_set_threshold_mb.to_string(),
                        Message::WorkingSet
                    )
                ]
                .spacing(12),
            ),
            (
                1,
                "memory_trim.category_when_to_trim",
                column![field(
                    "memory_trim.idle_time",
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
                motion_enabled,
            ));
        }
        let mut safety = column![
            text(t!("memory_trim.category_safety_help").to_string()).style(text::secondary),
            row![
                text_input(&t!("process_list.executable_path"), &self.path).on_input(Message::Path),
                button(text(t!("common.browse_executable").to_string())).on_press(Message::Browse),
                button(text(t!("common.add").to_string())).on_press_maybe(
                    (settings.enabled && can_add_memory_trim_exclusion(settings, &self.path))
                        .then_some(Message::Add)
                )
            ]
            .spacing(8)
            .align_y(iced::Center)
        ]
        .spacing(12);
        let matching = candidates
            .iter()
            .filter(|p| p.to_lowercase().contains(&self.path.to_lowercase()))
            .cloned()
            .collect::<Vec<_>>();
        if settings.enabled && !matching.is_empty() {
            safety = safety.push(
                pick_list(matching, None::<String>, Message::Path)
                    .placeholder(t!("common.running").to_string()),
            );
        }
        let mut cards = Vec::new();
        let mut rules = settings.exclusions.iter().enumerate().collect::<Vec<_>>();
        if let Some((i, rule)) = &self.removed {
            rules.insert((*i).min(rules.len()), (*i, rule));
        }
        for (i, r) in rules {
            cards.push((
                super::motion::key(&r.executable_path),
                super::motion::removal(
                    super::widgets::settings_card(
                        row![
                            checkbox(r.enabled)
                                .label(r.executable_path.clone())
                                .on_toggle_maybe(
                                    settings
                                        .enabled
                                        .then_some(move |v| Message::RuleEnabled(i, v))
                                ),
                            button(text(t!("common.remove").to_string()))
                                .on_press(Message::Remove(i))
                        ]
                        .spacing(8)
                        .align_y(iced::Center),
                    ),
                    self.removed
                        .as_ref()
                        .is_some_and(|(_, removed)| removed.executable_path == r.executable_path),
                    motion_enabled,
                    Message::Removed(r.executable_path.clone()),
                ),
            ));
        }
        safety = safety.push(iced::widget::keyed_column(cards).spacing(8));
        if settings.exclusions.is_empty() {
            safety = safety.push(text(t!("memory_trim.no_exclusions").to_string()));
        }
        if self.removing.is_some() {
            safety = safety.push(super::widgets::settings_card(
                row![
                    button(text(t!("common.remove").to_string())).on_press(Message::ConfirmRemove),
                    button(text(t!("common.cancel").to_string())).on_press(Message::CancelRemove)
                ]
                .spacing(8)
                .align_y(iced::Center),
            ));
        }
        body = body.push(super::widgets::setting_group(
            "memory_trim.category_safety".to_string(),
            !self.collapsed[2],
            Message::Collapse(2),
            iced::widget::Space::new(),
            safety,
            motion_enabled,
        ));
        body = body.push(
            button(text(t!("memory_trim.trim_now").to_string()))
                .on_press_maybe(settings.enabled.then_some(Message::TrimNow)),
        );
        scrollable(body).spacing(10).height(Fill).into()
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
    fn invalid_thresholds_and_cancel_do_not_mutate_settings() {
        let mut e = Editor::default();
        let mut s = crate::config::Settings::default().memory_trim;
        let before = s.clone();
        e.update(&mut s, Message::Load("0".into()));
        e.update(&mut s, Message::Idle("86401".into()));
        e.update(&mut s, Message::WorkingSet("-1".into()));
        e.update(&mut s, Message::Remove(0));
        e.update(&mut s, Message::CancelRemove);
        e.update(&mut s, Message::ConfirmRemove);
        assert_eq!(s, before);
        let path = r"C:\Apps\test.exe";
        s.exclusions.push(new_process_exclusion_rule(path));
        e.update(&mut s, Message::Remove(0));
        e.update(&mut s, Message::ConfirmRemove);
        assert!(s.exclusions.is_empty());
        assert!(e.removed.is_some());
        e.update(&mut s, Message::Removed("wrong.exe".into()));
        assert!(e.removed.is_some());
        e.update(&mut s, Message::Removed(path.into()));
        assert!(s.exclusions.is_empty());
        assert!(e.removed.is_none());
    }
}
