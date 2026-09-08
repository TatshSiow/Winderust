use super::priority_control::Tier;
use crate::config::{
    BackgroundEfficiencyAggressiveness, BackgroundEfficiencyRule, BackgroundEfficiencySettings,
    ProcessRuleMode, Settings,
};
use crate::ui::process_rules::can_add_process_candidate;
use iced::widget::{
    button, checkbox, column, container, pick_list, row, scrollable, text, text_input,
};
use iced::{Element, Fill};
use rust_i18n::t;
use std::path::Path;

#[derive(Default)]
pub(super) struct Editor {
    path: String,
    removing: Option<String>,
    deleting: Option<(usize, BackgroundEfficiencyRule)>,
    collapsed: [bool; 3],
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
    ConfirmRemove,
    Removed(String),
    Collapse(Tier),
    CancelRemove,
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
                if settings.enabled && can_add(settings, &self.path) {
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
            Message::Remove(path) => self.removing = Some(path),
            Message::Collapse(tier) => {
                let collapsed = &mut self.collapsed[tier as usize];
                *collapsed = !*collapsed;
            }
            Message::CancelRemove => self.removing = None,
            Message::ConfirmRemove => {
                if let Some(path) = self.removing.take() {
                    if let Some(index) = settings
                        .custom_rules
                        .iter()
                        .position(|rule| rule.executable_path == path)
                    {
                        self.deleting = Some((index, settings.custom_rules.remove(index)));
                    }
                }
            }
            Message::Removed(path) => {
                if self
                    .deleting
                    .as_ref()
                    .is_some_and(|(_, rule)| rule.executable_path == path)
                {
                    self.deleting = None;
                }
            }
        }
    }
    pub(super) fn view<'a>(
        &'a self,
        settings: &'a Settings,
        candidates: &[String],
        motion_enabled: bool,
    ) -> Element<'a, Message> {
        let settings = &settings.background_efficiency;
        let enabled = settings.enabled;
        let mut body = column![
            checkbox(enabled)
                .label(t!("background_efficiency.enable").to_string())
                .on_toggle(Message::Enabled),
            text(t!("background_efficiency.intro_1").to_string())
                .width(Fill)
                .style(text::secondary),
            text(t!("background_efficiency.intro_2").to_string())
                .width(Fill)
                .style(text::secondary),
            text(t!("background_efficiency.intro_3").to_string())
                .width(Fill)
                .style(text::secondary)
        ]
        .spacing(12);
        let aggressiveness: Element<'_, Message> = if enabled {
            pick_list(
                BackgroundEfficiencyAggressiveness::ALL.map(Aggressiveness),
                Some(Aggressiveness(settings.aggressiveness)),
                |value| Message::Aggressiveness(value.0),
            )
            .into()
        } else {
            text(Aggressiveness(settings.aggressiveness).to_string()).into()
        };
        body = body
            .push(
                row![
                    text(t!("background_efficiency.aggressiveness").to_string()).width(200),
                    aggressiveness
                ]
                .spacing(8),
            )
            .push(text(
                t!("background_efficiency.aggressiveness_help").to_string(),
            ));
        for (tier, detection, value) in [
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
            (Tier::Background, true, settings.background_efficiency_mode),
        ] {
            let mut group = column![].spacing(8);
            if tier != Tier::Background {
                let label = if tier == Tier::Focus {
                    t!("background_efficiency.foreground_detection")
                } else {
                    t!("common.visible_window_detection")
                };
                group = group.push(
                    checkbox(detection)
                        .label(label.to_string())
                        .on_toggle_maybe(
                            enabled.then_some(move |value| Message::Detection(tier, value)),
                        ),
                );
            }
            group = group.push(
                checkbox(value)
                    .label(t!("process_list.efficiency_mode").to_string())
                    .on_toggle_maybe(
                        (enabled && detection)
                            .then_some(move |value| Message::Default(tier, value)),
                    ),
            );
            body = body.push(
                container(
                    column![
                        button(text(format!(
                            "{} {}",
                            if self.collapsed[tier as usize] {
                                ">"
                            } else {
                                "v"
                            },
                            tier.label()
                        )))
                        .on_press(Message::Collapse(tier))
                        .style(button::text),
                        super::motion::reveal(
                            group,
                            !self.collapsed[tier as usize],
                            motion_enabled
                        )
                    ]
                    .spacing(8),
                )
                .padding(12)
                .width(Fill)
                .style(iced::widget::container::bordered_box),
            );
        }
        body = body
            .push(text(t!("background_efficiency.custom_rules").to_string()).size(16))
            .push(text(
                t!("background_efficiency.custom_rules_help").to_string(),
            ));
        body = body.push(
            row![
                text_input(&t!("process_list.executable_path"), &self.path).on_input(Message::Path),
                button(text(t!("common.browse_executable").to_string()))
                    .on_press_maybe(enabled.then_some(Message::Browse)),
                button(text(t!("common.add").to_string())).on_press_maybe(
                    (enabled && can_add(settings, &self.path)).then_some(Message::Add)
                )
            ]
            .spacing(8),
        );
        let filter = self.path.to_lowercase();
        let candidates: Vec<_> = candidates
            .iter()
            .filter(|path| path.to_lowercase().contains(&filter) && can_add(settings, path))
            .cloned()
            .collect();
        if enabled && !candidates.is_empty() {
            body = body.push(
                pick_list(candidates, None::<String>, Message::Path)
                    .placeholder(t!("process_list.app_name").to_string()),
            );
        }
        let mut rule_cards = Vec::new();
        let mut visible_rules: Vec<_> = settings.custom_rules.iter().enumerate().collect();
        if let Some((index, rule)) = &self.deleting {
            visible_rules.insert((*index).min(visible_rules.len()), (usize::MAX, rule));
        }
        for (index, rule) in visible_rules {
            let mut card = column![row![
                checkbox(rule.enabled)
                    .label(rule.executable_path.clone())
                    .on_toggle_maybe(
                        enabled.then_some(move |value| Message::RuleEnabled(index, value))
                    ),
                button(text(t!("common.remove").to_string())).on_press_maybe(
                    enabled.then_some(Message::Remove(rule.executable_path.clone()))
                )
            ]
            .spacing(8)]
            .spacing(8);
            for (tier, mode) in Tier::ALL.into_iter().zip([
                rule.focus_efficiency_mode,
                rule.visible_window_efficiency_mode,
                rule.background_efficiency_mode,
            ]) {
                let control: Element<'_, Message> = if enabled {
                    pick_list(
                        ProcessRuleMode::ALL.map(Mode),
                        Some(Mode(mode)),
                        move |mode| Message::RuleMode(index, tier, mode.0),
                    )
                    .into()
                } else {
                    text(Mode(mode).to_string()).into()
                };
                card = card.push(row![text(tier.label()).width(180), control].spacing(8));
            }
            if self.removing.as_deref() == Some(rule.executable_path.as_str()) {
                card = card.push(
                    row![
                        button(text(t!("common.remove").to_string()))
                            .on_press(Message::ConfirmRemove),
                        button(text(t!("common.cancel").to_string()))
                            .on_press(Message::CancelRemove)
                    ]
                    .spacing(8),
                );
            }
            rule_cards.push((
                super::motion::key(&rule.executable_path),
                super::motion::removal(
                    container(card)
                        .padding(12)
                        .width(Fill)
                        .style(iced::widget::container::bordered_box),
                    index == usize::MAX,
                    motion_enabled,
                    Message::Removed(rule.executable_path.clone()),
                ),
            ));
        }
        body = body.push(iced::widget::keyed_column(rule_cards).spacing(8));
        if settings.custom_rules.is_empty() {
            body = body.push(text(
                t!("background_efficiency.no_custom_rules").to_string(),
            ));
        }
        scrollable(body).spacing(10).height(Fill).into()
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
    fn rules_inherit_defaults_and_edit_tiers_independently() {
        let mut settings = Settings::default();
        let mut editor = Editor::default();
        editor.update(&mut settings, Message::Enabled(true));
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
    }
}
