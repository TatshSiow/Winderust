use super::priority_control::Tier;
use super::widgets::{self, Ui};
use crate::config::{
    BackgroundEfficiencyAggressiveness, BackgroundEfficiencyRule, BackgroundEfficiencySettings,
    ProcessRuleMode, Settings,
};
use crate::ui::process_rules::can_add_process_candidate;
use rust_i18n::t;
use std::path::Path;
use windui::prelude::Element;

#[derive(Default)]
pub(super) struct Editor {
    path: String,
    expanded: bool,
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

    Collapse(bool),
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
            Message::Collapse(expanded) => self.expanded = expanded,
        }
    }
    pub(super) fn view(
        &self,
        settings: &Settings,
        candidates: &[super::app_picker::Candidate],
        ui: Ui<Message>,
    ) -> Element {
        let settings = &settings.background_efficiency;
        let mut defaults = vec![widgets::setting_row(
            t!("background_efficiency.enable").to_string(),
            ui.toggle(settings.enabled, Message::Enabled),
        )];
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
            defaults.push(Element::divider());
            let title = match tier {
                Tier::Background => "common.background_process",
                Tier::Focus => "background_efficiency.foreground_detection",
                Tier::VisibleWindow => "common.visible_window_detection",
            };
            let selector = ui.select(
                [
                    widgets::Choice(false, t!("common.disabled").to_string()),
                    widgets::Choice(true, t!("common.enabled").to_string()),
                ],
                widgets::Choice(
                    value,
                    t!(if value {
                        "common.enabled"
                    } else {
                        "common.disabled"
                    })
                    .to_string(),
                ),
                move |value| Message::Default(tier, value.0),
            );
            let (control, help) = if tier == Tier::Background {
                (selector, None)
            } else {
                let help = match tier {
                    Tier::Focus => "background_efficiency.foreground_detection_help",
                    Tier::VisibleWindow => "common.visible_window_detection_help",
                    Tier::Background => unreachable!("Background has no detection switch"),
                };
                let help = t!(help).to_string();
                (
                    widgets::row().children([
                        selector.weight(1.0),
                        ui.toggle(detection, move |value| Message::Detection(tier, value))
                            .tooltip(help.clone()),
                    ]),
                    Some(help),
                )
            };
            defaults.push(match help {
                None => widgets::setting_row(t!(title).to_string(), control),
                Some(help) => widgets::col().spacing(2).children([
                    widgets::setting_row_with_help(
                        t!(title).to_string(),
                        help,
                        control.max_width(340),
                    ),
                    widgets::help("background_efficiency.mode_detection_help"),
                ]),
            });
        }
        defaults.push(Element::divider());
        defaults.push(ui.collapsible(
            t!("settings.advanced").to_string(),
            self.expanded,
            widgets::setting(
                "background_efficiency.aggressiveness",
                ui.select(
                    BackgroundEfficiencyAggressiveness::ALL.map(Aggressiveness),
                    Aggressiveness(settings.aggressiveness),
                    |value| Message::Aggressiveness(value.0),
                ),
            ),
            Message::Collapse,
        ));
        let mut body = vec![
            widgets::card("", defaults),
            widgets::setting_title("background_efficiency.custom_rules")
                .font_size(super::design::typography::SUBTITLE as f32)
                .font_weight(600),
        ];
        let candidate_settings = settings.clone();
        body.push(super::app_picker::view(
            &self.path,
            candidates,
            true,
            Message::Path,
            Message::Browse,
            Some(Message::Add),
            move |candidate| {
                can_add(
                    &candidate_settings,
                    &candidate.info.image_path.to_string_lossy(),
                )
                .then_some(true)
            },
            ui.clone(),
        ));
        let mut rows = Vec::new();
        for (index, rule) in settings.custom_rules.iter().enumerate() {
            let mut controls = Vec::new();
            for (tier, mode) in Tier::ALL.into_iter().zip([
                rule.focus_efficiency_mode,
                rule.visible_window_efficiency_mode,
                rule.background_efficiency_mode,
            ]) {
                controls.push(ui.select(
                    ProcessRuleMode::ALL.map(Mode),
                    Mode(mode),
                    move |value| Message::RuleMode(index, tier, value.0),
                ));
            }
            rows.push(widgets::process_rule_row(
                &rule.executable_path,
                candidates,
                ui.checkbox("", rule.enabled, move |value| {
                    Message::RuleEnabled(index, value)
                }),
                controls,
                widgets::rule_delete_button(
                    &ui,
                    Some(Message::Remove(rule.executable_path.clone())),
                ),
            ));
        }
        body.push(widgets::process_rules_table(
            Tier::ALL.map(|tier| tier.label()),
            rows,
            t!("background_efficiency.no_custom_rules").to_string(),
        ));
        widgets::page(body)
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
        assert!(!editor.expanded);
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
