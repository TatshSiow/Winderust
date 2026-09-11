use super::design;
use super::priority_control::Tier;
use super::widgets::{checkbox, pick_list};
use crate::config::{
    BackgroundEfficiencyAggressiveness, BackgroundEfficiencyRule, BackgroundEfficiencySettings,
    ProcessRuleMode, Settings,
};
use crate::ui::process_rules::can_add_process_candidate;
use iced::widget::{column, scrollable};
use iced::{Element, Fill};
use rust_i18n::t;
use std::path::Path;

#[derive(Default)]
pub(super) struct Editor {
    path: String,
    expanded: [bool; 3],
}
#[derive(Debug, Clone)]
pub(super) enum Message {
    Enabled(bool),
    Detection(Tier, bool),
    Default(Tier, bool),
    Aggressiveness(BackgroundEfficiencyAggressiveness),
    Path(String),
    Browse,
    Add,
    RuleEnabled(usize, bool),
    RuleMode(usize, Tier, ProcessRuleMode),
    Remove(String),

    Collapse(Tier),
}
impl Editor {
    pub(super) fn update(&mut self, settings: &mut Settings, message: Message) {
        let settings = &mut settings.background_efficiency;
        match message {
            Message::Enabled(value) => settings.enabled = value,
            Message::Detection(tier, value) => match tier {
                Tier::Focus => settings.foreground_detection_enabled = value,
                Tier::VisibleWindow => settings.visible_window_detection_enabled = value,
                Tier::Background => {}
            },
            Message::Default(tier, value) => {
                *match tier {
                    Tier::Focus => &mut settings.foreground_efficiency_mode,
                    Tier::VisibleWindow => &mut settings.visible_window_efficiency_mode,
                    Tier::Background => &mut settings.background_efficiency_mode,
                } = value
            }
            Message::Aggressiveness(value) => settings.aggressiveness = value,
            Message::Path(value) => self.path = value,
            Message::Browse => {} // The application owns the native executable picker.
            Message::Add => {
                if can_add(settings, &self.path) {
                    settings.custom_rules.push(BackgroundEfficiencyRule {
                        enabled: true,
                        executable_path: crate::foreground::executable_path_key(Path::new(
                            self.path.trim(),
                        )),
                        focus_efficiency_mode: ProcessRuleMode::Default,
                        visible_window_efficiency_mode: ProcessRuleMode::Default,
                        background_efficiency_mode: ProcessRuleMode::Default,
                    });
                    self.path.clear();
                }
            }
            Message::RuleEnabled(index, value) => {
                if let Some(rule) = settings.custom_rules.get_mut(index) {
                    rule.enabled = value;
                }
            }
            Message::RuleMode(index, tier, mode) => {
                if let Some(rule) = settings.custom_rules.get_mut(index) {
                    *match tier {
                        Tier::Focus => &mut rule.focus_efficiency_mode,
                        Tier::VisibleWindow => &mut rule.visible_window_efficiency_mode,
                        Tier::Background => &mut rule.background_efficiency_mode,
                    } = mode;
                }
            }
            Message::Remove(path) => {
                settings
                    .custom_rules
                    .retain(|rule| rule.executable_path != path);
            }
            Message::Collapse(tier) => {
                let expanded = &mut self.expanded[tier as usize];
                *expanded = !*expanded;
            }
        }
    }
    pub(super) fn view<'a>(
        &'a self,
        settings: &'a Settings,
        candidates: &[super::app_picker::Candidate],
    ) -> Element<'a, Message> {
        let settings = &settings.background_efficiency;
        let enabled = settings.enabled;
        let mut body = column![].spacing(super::widgets::CARD_GAP);
        for (tier, detection, value) in [
            (Tier::Background, true, settings.background_efficiency_mode),
            (
                Tier::Focus,
                settings.foreground_detection_enabled,
                settings.foreground_efficiency_mode,
            ),
            (
                Tier::VisibleWindow,
                settings.visible_window_detection_enabled,
                settings.visible_window_efficiency_mode,
            ),
        ] {
            let mut group = column![];
            let label = match tier {
                Tier::Background => "background_efficiency.enable",
                Tier::Focus => "background_efficiency.foreground_detection",
                Tier::VisibleWindow => "common.visible_window_detection",
            }
            .to_string();
            let action: Element<'_, Message> = if tier == Tier::Background {
                super::widgets::switch(enabled, Some(Message::Enabled))
            } else {
                super::widgets::switch(
                    detection,
                    Some(move |value| Message::Detection(tier, value)),
                )
            };
            let choices = [
                super::widgets::Choice(false, t!("common.disabled").to_string()),
                super::widgets::Choice(true, t!("common.enabled").to_string()),
            ];
            let selected = choices[usize::from(value)].clone();
            group = group.push(super::widgets::setting_row(
                "process_list.efficiency_mode",
                pick_list(choices, Some(selected), move |v| {
                    Message::Default(tier, v.0)
                })
                .width(design::SELECT_WIDTH),
            ));
            if tier == Tier::Background {
                let aggressiveness = pick_list(
                    BackgroundEfficiencyAggressiveness::ALL.map(Aggressiveness),
                    Some(Aggressiveness(settings.aggressiveness)),
                    move |value| Message::Aggressiveness(value.0),
                )
                .width(design::SELECT_WIDTH);
                group = group.push(super::widgets::setting_row(
                    "background_efficiency.aggressiveness",
                    aggressiveness,
                ));
            }
            body = body.push(super::widgets::setting_group(
                label,
                self.expanded[tier as usize],
                Message::Collapse(tier),
                action,
                group,
            ));
        }
        body = body.push(super::widgets::setting_title(
            "background_efficiency.custom_rules",
        ));
        body = body.push(super::app_picker::view(
            &self.path,
            candidates,
            true,
            Message::Path,
            Message::Browse,
            (can_add(settings, &self.path)).then_some(Message::Add),
            |path| can_add(settings, path).then_some(true),
        ));
        let mut rule_cards = Vec::new();
        for (index, rule) in settings.custom_rules.iter().enumerate() {
            let mut controls = Vec::new();
            for (tier, mode) in Tier::ALL.into_iter().zip([
                rule.focus_efficiency_mode,
                rule.visible_window_efficiency_mode,
                rule.background_efficiency_mode,
            ]) {
                let control: Element<'_, Message> = {
                    pick_list(
                        ProcessRuleMode::ALL.map(Mode),
                        Some(Mode(mode)),
                        move |mode| Message::RuleMode(index, tier, mode.0),
                    )
                    .width(Fill)
                    .into()
                };
                controls.push(control);
            }
            rule_cards.push((
                super::widgets::stable_key(&rule.executable_path),
                super::widgets::process_rule_row(
                    &rule.executable_path,
                    candidates,
                    checkbox(rule.enabled)
                        .on_toggle_maybe(Some(move |v| Message::RuleEnabled(index, v)))
                        .into(),
                    controls,
                    Some(Message::Remove(rule.executable_path.clone())),
                ),
            ));
        }
        body = body.push(super::widgets::process_rules_table(
            Tier::ALL.map(|tier| tier.label()),
            rule_cards,
            t!("background_efficiency.no_custom_rules").to_string(),
        ));
        scrollable(body).height(Fill).into()
    }
}
fn can_add(settings: &BackgroundEfficiencySettings, path: &str) -> bool {
    can_add_process_candidate(
        path,
        |path| settings.contains_custom_rule(path),
        crate::background_efficiency::is_builtin_excluded,
    )
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Mode(ProcessRuleMode);
impl std::fmt::Display for Mode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&match self.0 {
            ProcessRuleMode::Default => t!("common.default"),
            ProcessRuleMode::Enabled => t!("common.enabled"),
            ProcessRuleMode::Disabled => t!("common.disabled"),
        })
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Aggressiveness(BackgroundEfficiencyAggressiveness);
impl std::fmt::Display for Aggressiveness {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&match self.0 {
            BackgroundEfficiencyAggressiveness::Safe => {
                t!("background_efficiency.aggressiveness_safe")
            }
            BackgroundEfficiencyAggressiveness::Balanced => {
                t!("background_efficiency.aggressiveness_balanced")
            }
            BackgroundEfficiencyAggressiveness::Aggressive => {
                t!("background_efficiency.aggressiveness_aggressive")
            }
        })
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disabled_feature_rules_remain_editable_and_validate_candidates() {
        let mut settings = Settings::default();
        let mut editor = Editor::default();
        editor.update(&mut settings, Message::Enabled(false));
        assert_eq!(editor.expanded, [false; 3]);
        editor.update(&mut settings, Message::Path(r"C:\Apps\editor.exe".into()));
        editor.update(&mut settings, Message::Add);
        assert_eq!(settings.background_efficiency.custom_rules.len(), 1);
        assert_eq!(
            settings.background_efficiency.custom_rules[0].background_efficiency_mode,
            ProcessRuleMode::Default
        );
        editor.update(
            &mut settings,
            Message::RuleMode(0, Tier::VisibleWindow, ProcessRuleMode::Disabled),
        );
        assert_eq!(
            settings.background_efficiency.custom_rules[0].focus_efficiency_mode,
            ProcessRuleMode::Default
        );
        assert_eq!(
            settings.background_efficiency.custom_rules[0].visible_window_efficiency_mode,
            ProcessRuleMode::Disabled
        );
        editor.update(&mut settings, Message::Path(r"C:\Apps\EDITOR.exe".into()));
        editor.update(&mut settings, Message::Add);
        assert_eq!(settings.background_efficiency.custom_rules.len(), 1);
        editor.update(&mut settings, Message::Path("relative.exe".into()));
        editor.update(&mut settings, Message::Add);
        assert_eq!(settings.background_efficiency.custom_rules.len(), 1);
        assert!(!settings.background_efficiency.enabled);
    }
}
