use super::design;
use super::widgets::{checkbox, text_input};
use crate::application::SettingsEditor;
use crate::config::MemoryTrimSettings;
use crate::memory_trim::{MemoryTrimSnapshot, MemoryTrimStatus};
use crate::ui::process_rules::{
    can_add_memory_trim_exclusion, exact_path_input, new_process_exclusion_rule, picked_path_input,
};
use crate::ui::scrolling::scrollable;
use iced::widget::column;
use iced::{Element, Fill};
use rust_i18n::t;

pub(super) fn manual_result(result: MemoryTrimSnapshot) -> Result<(), String> {
    let key = match result.status {
        MemoryTrimStatus::AutomationDisabled => "memory_trim.manual_automation_disabled",
        MemoryTrimStatus::Disabled => "memory_trim.manual_disabled",
        MemoryTrimStatus::WaitingForPowerSource { .. } => "memory_trim.manual_power_source_unknown",
        MemoryTrimStatus::ForegroundUnknown => "memory_trim.manual_foreground_unknown",
        MemoryTrimStatus::ManualCompleted => {
            if result.failed_processes == 0 {
                return result.last_error.map_or(Ok(()), Err);
            }
            let summary = t!(
                "memory_trim.manual_failed",
                trimmed = result.trimmed_processes,
                failed = result.failed_processes
            );
            return Err(match result.last_error {
                Some(error) => format!("{summary}\n{error}"),
                None => summary.into_owned(),
            });
        }
        MemoryTrimStatus::Error(error) => return Err(error),
        MemoryTrimStatus::WaitingForMemoryLoad { .. } | MemoryTrimStatus::Active => {
            "memory_trim.manual_result_unavailable"
        }
    };
    Err(t!(key).into_owned())
}

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

    fn value(self, settings: &MemoryTrimSettings) -> u64 {
        match self {
            Self::Load => u64::from(settings.system_memory_load_threshold_percent),
            Self::WorkingSet => settings.process_working_set_threshold_mb,
            Self::Idle => settings.process_idle_seconds,
        }
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

    pub(super) fn update(&mut self, settings: &mut SettingsEditor, message: Message) {
        match message {
            Message::Collapse(i) => {
                if let Some(value) = self.expanded.get_mut(i) {
                    *value = !*value;
                }
            }
            Message::Enabled(v) if settings.memory_trim.enabled != v => {
                settings.memory_trim.enabled = v;
            }
            Message::Path(v) => self.path = v,
            Message::Add => {
                let path = exact_path_input(&self.path);
                if can_add_memory_trim_exclusion(&settings.memory_trim, path) {
                    settings
                        .memory_trim
                        .exclusions
                        .push(new_process_exclusion_rule(path));
                    self.path.clear();
                }
            }
            Message::Threshold(field, raw) => {
                if let Some(value) = field
                    .parse(&raw)
                    .filter(|&value| value != field.value(&settings.memory_trim))
                {
                    let trim = &mut settings.memory_trim;
                    match field {
                        Threshold::Load => trim.system_memory_load_threshold_percent = value as u8,
                        Threshold::WorkingSet => trim.process_working_set_threshold_mb = value,
                        Threshold::Idle => trim.process_idle_seconds = value,
                    }
                }
                self.thresholds[field as usize] = Some(raw);
            }
            Message::RuleEnabled(i, v)
                if settings
                    .memory_trim
                    .exclusions
                    .get(i)
                    .is_some_and(|r| r.enabled != v) =>
            {
                settings.memory_trim.exclusions[i].enabled = v;
            }
            Message::Remove(i) if i < settings.memory_trim.exclusions.len() => {
                settings.memory_trim.exclusions.remove(i);
            }
            Message::Enabled(_)
            | Message::RuleEnabled(..)
            | Message::Remove(_)
            | Message::TrimNow
            | Message::Browse => {}
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
        let field = |field: Threshold, unit: &str| {
            let formatted;
            let raw = if let Some(raw) = &self.thresholds[field as usize] {
                raw
            } else {
                formatted = field.value(settings).to_string();
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
                    field(Threshold::Load, "%").into(),
                    field(Threshold::WorkingSet, "MB").into(),
                ]),
            ),
            (
                1,
                "memory_trim.category_when_to_trim",
                column![field(Threshold::Idle, "s")],
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
        let mut safety = column![super::app_picker::view_with_path_selection(
            &self.path,
            candidates,
            true,
            (Message::Path, |path| Message::Path(picked_path_input(path))),
            Message::Browse,
            can_add_memory_trim_exclusion(settings, exact_path_input(&self.path))
                .then_some(Message::Add),
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
    use crate::config::{PowerSourceProfile, Settings};

    #[test]
    fn picked_memory_exclusion_preserves_a_distinct_executable_path_on_save() {
        let mut editor = Editor::default();
        let ordinary = r"C:\Apps\test.exe";
        let literal = concat!(r"C:\Apps\test.exe", " ");
        let mut settings = Settings::default();
        settings
            .memory_trim
            .exclusions
            .push(new_process_exclusion_rule(ordinary));
        let mut settings = SettingsEditor::with_settings(settings);
        editor.update(
            &mut settings,
            Message::Path(picked_path_input(literal.into())),
        );
        editor.update(&mut settings, Message::Add);

        let saved = toml::to_string(&*settings).unwrap();
        let reloaded: Settings = toml::from_str(&saved).unwrap();
        let rules = &reloaded.memory_trim.exclusions;
        assert_eq!(rules.len(), 2);
        assert_eq!(rules[0].executable_path, ordinary);
        assert_eq!(rules[1].executable_path, literal);
    }

    #[test]
    fn groups_start_collapsed_and_toggle_without_changing_settings() {
        let mut editor = Editor::default();
        let mut settings = SettingsEditor::with_settings(Settings::default());
        settings.select_power_source(PowerSourceProfile::OnBattery);
        assert_eq!(editor.expanded, [false; 3]);
        editor.update(&mut settings, Message::Collapse(0));
        editor.update(&mut settings, Message::Collapse(2));
        assert_eq!(editor.expanded, [true, false, true]);
        editor.update(&mut settings, Message::Collapse(0));
        assert_eq!(editor.expanded, [false, false, true]);
        assert!(!settings.has_unsaved_changes());
    }
    #[test]
    fn invalid_thresholds_and_missing_removals_do_not_mutate_settings() {
        let mut e = Editor::default();
        let mut s = SettingsEditor::with_settings(Settings::default());
        s.select_power_source(PowerSourceProfile::OnBattery);
        let before = s.memory_trim.clone();
        e.update(&mut s, Message::Threshold(Threshold::Load, "0".into()));
        e.update(&mut s, Message::Threshold(Threshold::Idle, "86401".into()));
        e.update(
            &mut s,
            Message::Threshold(Threshold::WorkingSet, "-1".into()),
        );
        e.update(&mut s, Message::Remove(0));
        assert_eq!(s.memory_trim, before);
        assert!(!s.has_unsaved_changes());
        assert_eq!(e.invalid_threshold(), Some(Threshold::Load));
        e.update(
            &mut s,
            Message::Threshold(
                Threshold::Load,
                format!("0{}", before.system_memory_load_threshold_percent),
            ),
        );
        assert!(!s.has_unsaved_changes());
        assert_eq!(e.invalid_threshold(), Some(Threshold::WorkingSet));
        e.update(&mut s, Message::Threshold(Threshold::Load, "100".into()));
        assert_eq!(s.memory_trim.system_memory_load_threshold_percent, 100);
        e.update(
            &mut s,
            Message::Threshold(Threshold::WorkingSet, "1".into()),
        );
        assert_eq!(s.memory_trim.process_working_set_threshold_mb, 1);
        assert_eq!(e.invalid_threshold(), Some(Threshold::Idle));
        e.update(&mut s, Message::Threshold(Threshold::Idle, "10".into()));
        assert_eq!(s.memory_trim.process_idle_seconds, 10);
        assert_eq!(e.invalid_threshold(), None);
        let path = r"C:\Apps\test.exe";
        s.memory_trim
            .exclusions
            .push(new_process_exclusion_rule(path));
        e.update(&mut s, Message::Remove(0));
        assert!(s.memory_trim.exclusions.is_empty());
    }
}
