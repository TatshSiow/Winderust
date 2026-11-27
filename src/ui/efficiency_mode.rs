use super::design;
use super::feature_tabs::{FeatureTab, CONTENT_ID};
use super::priority_control::{ChildProtection, Tier};
use super::widgets::{self, checkbox, pick_list};
use crate::application::SettingsEditor;
use crate::config::{EfficiencyModeRule, EfficiencyModeSettings, ProcessRuleMode, Settings};
use crate::foreground::process_list::normalize_path_slashes;
use crate::ui::process_rules::{
    can_add_exact_process_candidate, exact_path_input, picked_path_input,
};
use crate::ui::scrolling::scrollable;
use iced::widget::column;
use iced::{Element, Fill};
use rust_i18n::t;

#[derive(Default)]
pub(super) struct Editor {
    path: String,
}
#[derive(Debug, Clone)]
pub(super) enum Message {
    Enabled(bool),
    Detection(Tier, bool),
    ProtectChildren(ChildProtection, bool),
    Default(Tier, bool),
    Path(String),
    Browse,
    Add,
    RuleEnabled(usize, bool),
    RuleMode(usize, Tier, ProcessRuleMode),
    Remove(String),
}
impl Editor {
    pub(super) fn update(&mut self, settings: &mut SettingsEditor, message: Message) {
        let current = &settings.efficiency_mode;
        match message {
            Message::Enabled(value) if current.enabled != value => {
                settings.efficiency_mode.enabled = value;
            }
            Message::Detection(Tier::Focus, value)
                if current.foreground_detection_enabled != value =>
            {
                settings.efficiency_mode.foreground_detection_enabled = value;
            }
            Message::Detection(Tier::VisibleWindow, value)
                if current.visible_window_detection_enabled != value =>
            {
                settings.efficiency_mode.visible_window_detection_enabled = value;
            }
            Message::ProtectChildren(protection, value) => {
                if protection.enabled(current.child_protection) != value {
                    protection.set(&mut settings.efficiency_mode.child_protection, value);
                }
            }
            Message::Default(Tier::Focus, value) if current.foreground_efficiency_mode != value => {
                settings.efficiency_mode.foreground_efficiency_mode = value;
            }
            Message::Default(Tier::VisibleWindow, value)
                if current.visible_window_efficiency_mode != value =>
            {
                settings.efficiency_mode.visible_window_efficiency_mode = value;
            }
            Message::Default(Tier::Background, value)
                if current.background_efficiency_mode != value =>
            {
                settings.efficiency_mode.background_efficiency_mode = value;
            }
            Message::Path(value) => self.path = value,
            Message::Browse => {} // The application owns the native executable picker.
            Message::Add => {
                let path = exact_path_input(&self.path);
                if can_add(current, path) {
                    settings
                        .efficiency_mode
                        .custom_rules
                        .push(EfficiencyModeRule {
                            enabled: true,
                            executable_path: normalize_path_slashes(path).into_owned(),
                            focus_efficiency_mode: ProcessRuleMode::Default,
                            visible_window_efficiency_mode: ProcessRuleMode::Default,
                            background_efficiency_mode: ProcessRuleMode::Default,
                        });
                    self.path.clear();
                }
            }
            Message::RuleEnabled(index, value) => {
                if current
                    .custom_rules
                    .get(index)
                    .is_some_and(|rule| rule.enabled != value)
                {
                    settings.efficiency_mode.custom_rules[index].enabled = value;
                }
            }
            Message::RuleMode(index, tier, mode) => {
                if current
                    .custom_rules
                    .get(index)
                    .is_some_and(|rule| rule_mode(rule, tier) != mode)
                {
                    let rule = &mut settings.efficiency_mode.custom_rules[index];
                    *match tier {
                        Tier::Focus => &mut rule.focus_efficiency_mode,
                        Tier::VisibleWindow => &mut rule.visible_window_efficiency_mode,
                        Tier::Background => &mut rule.background_efficiency_mode,
                    } = mode;
                }
            }
            Message::Remove(path) => {
                if current
                    .custom_rules
                    .iter()
                    .any(|rule| rule.executable_path == path)
                {
                    settings
                        .efficiency_mode
                        .custom_rules
                        .retain(|rule| rule.executable_path != path);
                }
            }
            Message::Enabled(_) | Message::Detection(..) | Message::Default(..) => {}
        }
    }
    pub(super) fn view<'a>(
        &'a self,
        settings: &'a Settings,
        candidates: &'a [super::app_picker::Candidate],
        tab: FeatureTab,
    ) -> Element<'a, Message> {
        let settings = &settings.efficiency_mode;
        if tab == FeatureTab::CustomRules {
            return self.rules_view(settings, candidates);
        }
        let mut body = column![widgets::toggle_card(
            "efficiency_mode.enable",
            settings.enabled,
            Some(Message::Enabled),
        )]
        .spacing(widgets::CARD_GAP);
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
            let detection_row = tier.detection_key().map(|label| {
                widgets::setting_row(
                    label,
                    widgets::switch(
                        detection,
                        Some(move |value| Message::Detection(tier, value)),
                    ),
                )
                .into()
            });
            let choices = [
                widgets::Choice(false, t!("common.disabled").to_string()),
                widgets::Choice(true, t!("common.enabled").to_string()),
            ];
            let selected = choices[usize::from(value)].clone();
            let setting = tier.setting_row(
                "efficiency_mode.tier_setting",
                pick_list(choices, Some(selected), move |v| {
                    Message::Default(tier, v.0)
                })
                .width(design::SELECT_WIDTH),
            );
            let protection = tier.child_protection().map(|protection| {
                protection
                    .row(settings.child_protection, move |value| {
                        Message::ProtectChildren(protection, value)
                    })
                    .into()
            });
            let rows = detection_row
                .into_iter()
                .chain([setting.into()])
                .chain(protection);
            body = body.push(widgets::settings_section(
                tier.label_key(),
                widgets::settings_card(widgets::settings_rows(rows)),
            ));
        }
        scrollable(body).id(CONTENT_ID).height(Fill).into()
    }

    fn rules_view<'a>(
        &'a self,
        settings: &'a EfficiencyModeSettings,
        candidates: &'a [super::app_picker::Candidate],
    ) -> Element<'a, Message> {
        let picker = column![
            widgets::help_text("efficiency_mode.custom_rules_help"),
            super::app_picker::view_with_path_selection(
                &self.path,
                candidates,
                true,
                (Message::Path, |path| Message::Path(picked_path_input(path))),
                Message::Browse,
                (can_add(settings, exact_path_input(&self.path))).then_some(Message::Add),
                |path| can_add(settings, path).then_some(true),
            ),
        ]
        .spacing(design::space::SMALL);
        let mut rule_cards = Vec::new();
        for (index, rule) in settings.custom_rules.iter().enumerate() {
            let mut controls = Vec::new();
            for tier in Tier::ALL {
                let control: Element<'_, Message> = {
                    pick_list(
                        ProcessRuleMode::ALL.map(Mode),
                        Some(Mode(rule_mode(rule, tier))),
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
        scrollable(
            column![
                super::widgets::settings_card(picker),
                super::widgets::process_rules_table(
                    Tier::ALL.map(|tier| tier.label()),
                    rule_cards,
                    false,
                ),
            ]
            .spacing(super::widgets::CARD_GAP),
        )
        .id(CONTENT_ID)
        .height(Fill)
        .into()
    }
}
fn rule_mode(rule: &EfficiencyModeRule, tier: Tier) -> ProcessRuleMode {
    match tier {
        Tier::Focus => rule.focus_efficiency_mode,
        Tier::VisibleWindow => rule.visible_window_efficiency_mode,
        Tier::Background => rule.background_efficiency_mode,
    }
}
fn can_add(settings: &EfficiencyModeSettings, path: &str) -> bool {
    can_add_exact_process_candidate(
        path,
        |path| settings.contains_custom_rule(path),
        crate::efficiency_mode::is_builtin_excluded,
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
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disabled_feature_rules_remain_editable_and_validate_candidates() {
        let mut settings = SettingsEditor::with_settings(Settings::default());
        settings.select_power_source(crate::config::PowerSourceProfile::OnBattery);
        let mut editor = Editor::default();
        editor.update(&mut settings, Message::Enabled(false));
        editor.update(&mut settings, Message::Path(r"C:\Apps\editor.exe".into()));
        assert!(settings.global().on_battery.is_none());
        editor.update(&mut settings, Message::Add);
        assert_eq!(settings.efficiency_mode.custom_rules.len(), 1);
        assert!(settings.global().efficiency_mode.custom_rules.is_empty());
        assert_eq!(
            settings.efficiency_mode.custom_rules[0].background_efficiency_mode,
            ProcessRuleMode::Default
        );
        editor.update(
            &mut settings,
            Message::RuleMode(0, Tier::VisibleWindow, ProcessRuleMode::Disabled),
        );
        assert_eq!(
            settings.efficiency_mode.custom_rules[0].focus_efficiency_mode,
            ProcessRuleMode::Default
        );
        assert_eq!(
            settings.efficiency_mode.custom_rules[0].visible_window_efficiency_mode,
            ProcessRuleMode::Disabled
        );
        editor.update(&mut settings, Message::Path(r"C:\Apps\EDITOR.exe".into()));
        editor.update(&mut settings, Message::Add);
        assert_eq!(settings.efficiency_mode.custom_rules.len(), 1);
        editor.update(&mut settings, Message::Path("relative.exe".into()));
        editor.update(&mut settings, Message::Add);
        assert_eq!(settings.efficiency_mode.custom_rules.len(), 1);
        assert!(!settings.efficiency_mode.enabled);
    }
}
