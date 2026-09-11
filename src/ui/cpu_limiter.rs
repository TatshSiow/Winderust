use super::design;
use super::widgets::{button, checkbox, pick_list, slider, text_input};
use iced::widget::{column, row, scrollable, text};
use iced::{Element, Fill};
use rust_i18n::t;
use std::collections::HashMap;

use crate::config::{CpuLimiterRule, CpuLimiterSettings, ProcessRuleMode};
use crate::ui::process_rules::{can_add_cpu_limiter_process, new_cpu_limiter_rule};

#[derive(Default)]
pub(super) struct CpuLimiter {
    path: String,
    collapsed: bool,
    draft: Option<(usize, CpuLimiterSettings)>,
    numbers: HashMap<(String, Tier), String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Tier {
    Focus,
    VisibleWindow,
    Background,
}

impl Tier {
    const ALL: [Self; 3] = [Self::Focus, Self::VisibleWindow, Self::Background];

    fn key(self) -> &'static str {
        match self {
            Self::Focus => "cpu_limiter.focus_allowed_cpu_time",
            Self::VisibleWindow => "cpu_limiter.visible_window_allowed_cpu_time",
            Self::Background => "cpu_limiter.background_allowed_cpu_time",
        }
    }
    fn label(self) -> String {
        t!(self.key()).to_string()
    }

    fn rule_fields(self, rule: &mut CpuLimiterRule) -> (&mut ProcessRuleMode, &mut u8) {
        match self {
            Self::Focus => (
                &mut rule.focus_mode,
                &mut rule.focus_allowed_cpu_time_percent,
            ),
            Self::VisibleWindow => (
                &mut rule.visible_window_mode,
                &mut rule.visible_window_allowed_cpu_time_percent,
            ),
            Self::Background => (
                &mut rule.background_mode,
                &mut rule.background_allowed_cpu_time_percent,
            ),
        }
    }
}

#[derive(Debug, Clone)]
pub(super) enum Message {
    Enabled(bool),
    Path(String),
    Browse,
    Number(Option<usize>, Tier, String),
    Collapse,
    Edit(usize),
    SaveRule,
    CancelRule,
    Add,
    Default(Tier, u8),
    RuleEnabled(usize, bool),
    RuleMode(usize, Tier, ProcessRuleMode),
    RuleLimit(usize, Tier, u8),
    Remove(usize),
}

impl CpuLimiter {
    pub(super) fn update(&mut self, settings: &mut CpuLimiterSettings, message: Message) {
        match message {
            Message::Edit(index) => {
                if settings.rules.get(index).is_some() {
                    self.draft = Some((index, settings.clone()));
                }
                return;
            }
            Message::CancelRule => {
                if let Some((index, draft)) = self.draft.take() {
                    let path = &draft.rules[index].executable_path;
                    self.numbers.retain(|(p, _), _| p != path);
                }
                return;
            }
            Message::SaveRule => {
                if self.has_invalid_inputs() {
                    return;
                }
                if let Some((index, draft)) = self.draft.take() {
                    if let Some(rule) = settings.rules.get_mut(index) {
                        *rule = draft.rules[index].clone();
                    }
                }
                return;
            }
            _ => {}
        }
        if let Some((index, mut draft)) = self.draft.take() {
            self.update_fields(&mut draft, message);
            self.draft = Some((index, draft));
        } else {
            self.update_fields(settings, message);
        }
    }
    fn update_fields(&mut self, settings: &mut CpuLimiterSettings, message: Message) {
        match message {
            Message::Enabled(value) => {
                settings.enabled = value;
                if !value {
                    self.numbers.clear();
                }
            }
            Message::Path(path) => self.path = path,
            Message::Browse => {}
            Message::Collapse => self.collapsed = !self.collapsed,
            Message::Edit(_) | Message::SaveRule | Message::CancelRule => {}
            Message::Number(index, tier, raw) => {
                let path = match index {
                    Some(index) => match settings.rules.get(index) {
                        Some(rule) => rule.executable_path.clone(),
                        None => return,
                    },
                    None => String::new(),
                };
                let parsed = raw
                    .parse::<u8>()
                    .ok()
                    .filter(|value| (1..=100).contains(value));
                if let Some(value) = parsed {
                    self.update(
                        settings,
                        match index {
                            Some(index) => Message::RuleLimit(index, tier, value),
                            None => Message::Default(tier, value),
                        },
                    );
                }
                self.numbers.insert((path, tier), raw);
            }
            Message::Add => {
                if settings.enabled && can_add_cpu_limiter_process(settings, &self.path) {
                    settings.rules.push(new_cpu_limiter_rule(&self.path));
                    self.path.clear();
                }
            }
            Message::Default(tier, value) => {
                self.numbers.remove(&(String::new(), tier));
                let field = match tier {
                    Tier::Focus => &mut settings.focus_allowed_cpu_time_percent,
                    Tier::VisibleWindow => &mut settings.visible_window_allowed_cpu_time_percent,
                    Tier::Background => &mut settings.background_allowed_cpu_time_percent,
                };
                *field = value.clamp(1, 100);
            }
            Message::RuleEnabled(index, value) => {
                if let Some(rule) = settings.rules.get_mut(index) {
                    rule.enabled = value;
                }
            }
            Message::RuleMode(index, tier, value) => {
                if let Some(rule) = settings.rules.get_mut(index) {
                    self.numbers.remove(&(rule.executable_path.clone(), tier));
                    *tier.rule_fields(rule).0 = value;
                }
            }
            Message::RuleLimit(index, tier, value) => {
                if let Some(rule) = settings.rules.get_mut(index) {
                    self.numbers.remove(&(rule.executable_path.clone(), tier));
                    *tier.rule_fields(rule).1 = value.clamp(1, 100);
                }
            }
            Message::Remove(index) => {
                if index < settings.rules.len() {
                    let rule = settings.rules.remove(index);
                    self.numbers
                        .retain(|(path, _), _| path != &rule.executable_path);
                }
            }
        }
    }

    pub(super) fn has_invalid_inputs(&self) -> bool {
        self.numbers.values().any(|raw| {
            raw.parse::<u8>()
                .ok()
                .is_none_or(|value| !(1..=100).contains(&value))
        })
    }
    fn number(&self, path: &str, tier: Tier, value: u8) -> String {
        self.numbers
            .get(&(path.to_owned(), tier))
            .cloned()
            .unwrap_or_else(|| value.to_string())
    }
    fn limit_control(
        &self,
        index: Option<usize>,
        tier: Tier,
        path: &str,
        value: u8,
        enabled: bool,
    ) -> Element<'_, Message> {
        let change = move |value| match index {
            Some(index) => Message::RuleLimit(index, tier, value),
            None => Message::Default(tier, value),
        };
        row![
            button(super::navigation::glyph("icons/minus.svg"))
                .padding(7)
                .height(32)
                .on_press_maybe((enabled && value > 1).then(|| change(value.saturating_sub(1)))),
            {
                let control: Element<'_, Message> =
                    slider(1..=100, value, change).width(Fill).into();
                if enabled {
                    control
                } else {
                    iced::widget::stack![
                        control,
                        iced::widget::opaque(iced::widget::Space::new().width(Fill).height(Fill))
                    ]
                    .width(Fill)
                    .height(design::SLIDER_HEIGHT)
                    .into()
                }
            },
            button(super::navigation::glyph("icons/plus.svg"))
                .padding(7)
                .height(32)
                .on_press_maybe((enabled && value < 100).then(|| change(value.saturating_add(1)))),
            text_input("1-100", &self.number(path, tier, value))
                .on_input_maybe(enabled.then_some(move |raw| Message::Number(index, tier, raw)))
                .width(design::NUMERIC_WIDTH)
        ]
        .spacing(design::space::SMALL)
        .align_y(iced::Center)
        .into()
    }
    fn limit_row(
        &self,
        index: Option<usize>,
        tier: Tier,
        path: &str,
        value: u8,
        enabled: bool,
    ) -> Element<'_, Message> {
        row![
            text(super::widgets::label_with_unit(&tier.label(), "%")).width(Fill),
            iced::widget::container(self.limit_control(index, tier, path, value, enabled))
                .width(420)
        ]
        .spacing(design::space::MEDIUM)
        .height(super::widgets::SETTING_ROW_HEIGHT)
        .align_y(iced::Center)
        .into()
    }
    pub(super) fn view<'a>(
        &'a self,
        settings: &'a CpuLimiterSettings,
        candidates: &[super::app_picker::Candidate],
    ) -> Element<'a, Message> {
        let mut body = column![].spacing(super::widgets::CARD_GAP);
        body = body.push(text(t!("cpu_limiter.intro_4").to_string()).style(text::warning));
        if self.has_invalid_inputs() {
            body = body.push(
                text(t!("cpu_limiter.intro_2").to_string())
                    .width(Fill)
                    .style(text::danger),
            );
        }
        let mut defaults = column![].spacing(design::space::MEDIUM);
        for (tier, value) in Tier::ALL.into_iter().zip([
            settings.focus_allowed_cpu_time_percent,
            settings.visible_window_allowed_cpu_time_percent,
            settings.background_allowed_cpu_time_percent,
        ]) {
            defaults = defaults.push(self.limit_row(None, tier, "", value, settings.enabled));
        }
        body = body.push(super::widgets::setting_group(
            "cpu_limiter.enable".to_string(),
            !self.collapsed,
            Message::Collapse,
            super::widgets::switch(settings.enabled, Some(Message::Enabled)),
            defaults,
        ));
        body = body
            .push(super::widgets::setting_title("cpu_limiter.rules"))
            .push(
                text(t!("cpu_limiter.rules_help").to_string())
                    .width(Fill)
                    .style(text::secondary),
            )
            .push(super::app_picker::view(
                &self.path,
                candidates,
                settings.enabled,
                Message::Path,
                Message::Browse,
                (settings.enabled && can_add_cpu_limiter_process(settings, &self.path))
                    .then_some(Message::Add),
                |path| can_add_cpu_limiter_process(settings, path).then_some(true),
            ));
        let mut cards = Vec::new();
        for (index, rule) in settings.rules.iter().enumerate() {
            let mut cells = Vec::new();
            for (tier, (mode, _value)) in Tier::ALL.into_iter().zip([
                (rule.focus_mode, rule.focus_allowed_cpu_time_percent),
                (
                    rule.visible_window_mode,
                    rule.visible_window_allowed_cpu_time_percent,
                ),
                (
                    rule.background_mode,
                    rule.background_allowed_cpu_time_percent,
                ),
            ]) {
                let choices: Vec<_> = ProcessRuleMode::ALL
                    .into_iter()
                    .map(|mode| Mode(mode, mode_label(mode)))
                    .collect();
                let selector: Element<'_, Message> = if settings.enabled {
                    pick_list(choices, Some(Mode(mode, mode_label(mode))), move |mode| {
                        Message::RuleMode(index, tier, mode.0)
                    })
                    .width(Fill)
                    .into()
                } else {
                    text(mode_label(mode)).into()
                };
                cells.push(selector);
            }

            let header = super::widgets::process_rule_header(
                &rule.executable_path,
                candidates,
                checkbox(rule.enabled)
                    .on_toggle_maybe(
                        settings
                            .enabled
                            .then_some(move |v| Message::RuleEnabled(index, v)),
                    )
                    .into(),
                cells,
                row![
                    iced::widget::tooltip(
                        button(super::navigation::glyph("icons/pencil.svg"))
                            .padding(7)
                            .width(32)
                            .height(32)
                            .style(super::widgets::quiet)
                            .on_press(Message::Edit(index)),
                        text(t!("common.edit").to_string()),
                        iced::widget::tooltip::Position::Top
                    ),
                    super::widgets::rule_delete_button(
                        settings.enabled.then_some(Message::Remove(index))
                    )
                ]
                .width(80)
                .align_y(iced::Center)
                .into(),
            );
            cards.push((
                super::widgets::stable_key(&rule.executable_path),
                column![header, iced::widget::rule::horizontal(1)].into(),
            ));
        }
        body = body.push(super::widgets::process_rules_table_with_actions(
            [
                t!("cpu_allocation.focus").to_string(),
                t!("common.visible_window").to_string(),
                t!("common.background_process").to_string(),
            ],
            cards,
            t!("cpu_limiter.no_rules").to_string(),
            80,
        ));
        scrollable(body).height(Fill).into()
    }
    pub(super) fn modal<'a>(
        &'a self,
        candidates: &[super::app_picker::Candidate],
        status: &crate::features::cpu_control::cpu_limiter::CpuLimiterSnapshot,
    ) -> Option<Element<'a, Message>> {
        let (index, settings) = self.draft.as_ref()?;
        let index = *index;
        let rule = settings.rules.get(index)?;
        let limited = status.limited_apps.iter().any(|path| {
            crate::foreground::same_executable_path(
                std::path::Path::new(path),
                std::path::Path::new(&rule.executable_path),
            )
        });
        let mut details = column![row![
            text(t!("common.status").to_string()).width(Fill),
            iced::widget::container(text(
                t!(if limited {
                    "cpu_limiter.indicator_limited"
                } else {
                    "common.off"
                })
                .to_string()
            ))
            .padding([4, 8])
            .style(move |theme| super::widgets::indicator_chip(theme, limited))
        ]
        .align_y(iced::Center)]
        .spacing(24);

        details = details.push(super::widgets::settings_card(
            column![
                super::app_picker::app_name(&rule.executable_path, candidates),
                text(rule.executable_path.clone()).style(text::secondary),
            ]
            .spacing(design::space::SMALL),
        ));
        for (tier, (mode, value)) in Tier::ALL.into_iter().zip([
            (rule.focus_mode, rule.focus_allowed_cpu_time_percent),
            (
                rule.visible_window_mode,
                rule.visible_window_allowed_cpu_time_percent,
            ),
            (
                rule.background_mode,
                rule.background_allowed_cpu_time_percent,
            ),
        ]) {
            let mut card = column![super::widgets::setting_row(
                tier.key(),
                pick_list(
                    ProcessRuleMode::ALL
                        .into_iter()
                        .map(|m| Mode(m, mode_label(m)))
                        .collect::<Vec<_>>(),
                    Some(Mode(mode, mode_label(mode))),
                    move |m| Message::RuleMode(index, tier, m.0),
                )
                .width(design::SELECT_WIDTH),
            )]
            .spacing(design::space::MEDIUM);
            if mode == ProcessRuleMode::Enabled {
                card = card.push(self.limit_row(
                    Some(index),
                    tier,
                    &rule.executable_path,
                    value,
                    true,
                ));
            }
            details = details.push(super::widgets::settings_card(card));
        }
        let header = row![
            text(t!("common.edit").to_string()).width(Fill),
            button(super::navigation::glyph("icons/x.svg")).on_press(Message::CancelRule)
        ]
        .align_y(iced::Center);
        let footer = row![
            iced::widget::Space::new().width(Fill),
            button(text(t!("common.cancel").to_string()))
                .style(super::widgets::tertiary_button)
                .on_press(Message::CancelRule),
            button(text(t!("common.save").to_string()))
                .style(super::widgets::primary_button)
                .on_press_maybe((!self.has_invalid_inputs()).then_some(Message::SaveRule))
        ]
        .spacing(design::space::SMALL);
        Some(super::widgets::modal_frame(
            header,
            scrollable(details).height(Fill),
            footer,
            (900, 680),
        ))
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Mode(ProcessRuleMode, String);

impl std::fmt::Display for Mode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.1)
    }
}

fn mode_label(mode: ProcessRuleMode) -> String {
    match mode {
        ProcessRuleMode::Default => t!("cpu_limiter.follow_default"),
        ProcessRuleMode::Enabled => t!("cpu_limiter.custom"),
        ProcessRuleMode::Disabled => t!("cpu_limiter.unlimited"),
    }
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rule_editor_cancel_and_save_are_isolated() {
        let mut editor = CpuLimiter::default();
        let mut settings = CpuLimiterSettings::default();
        settings
            .rules
            .push(new_cpu_limiter_rule(r"C:\Apps\test.exe"));
        let before = settings.clone();
        editor.update(&mut settings, Message::Edit(0));
        editor.update(&mut settings, Message::RuleLimit(0, Tier::Focus, 25));
        assert_eq!(settings, before);
        editor.update(&mut settings, Message::CancelRule);
        assert_eq!(settings, before);
        editor.update(&mut settings, Message::Edit(0));
        editor.update(&mut settings, Message::RuleLimit(0, Tier::Focus, 25));
        editor.update(&mut settings, Message::SaveRule);
        assert_eq!(settings.rules[0].focus_allowed_cpu_time_percent, 25);
    }
    #[test]
    fn invalid_number_drafts_cannot_change_settings_and_removal_is_immediate() {
        let mut editor = CpuLimiter::default();
        let mut settings = crate::config::Settings::default().cpu_limiter;
        settings
            .rules
            .push(new_cpu_limiter_rule(r"C:\Apps\test.exe"));
        let prior = settings.focus_allowed_cpu_time_percent;
        editor.update(&mut settings, Message::Number(None, Tier::Focus, "".into()));
        assert!(editor.has_invalid_inputs());
        assert_eq!(settings.focus_allowed_cpu_time_percent, prior);
        editor.update(&mut settings, Message::Default(Tier::Focus, 34));
        assert!(!editor.has_invalid_inputs());
        editor.update(&mut settings, Message::Remove(0));
        assert!(settings.rules.is_empty());
    }
    #[test]
    fn rule_edits_keep_tiers_independent_and_limits_valid() {
        let mut editor = CpuLimiter::default();
        let mut settings = crate::config::Settings::default().cpu_limiter;
        settings
            .rules
            .push(new_cpu_limiter_rule(r"C:\Apps\test.exe"));
        editor.update(&mut settings, Message::RuleLimit(0, Tier::Focus, 0));
        editor.update(
            &mut settings,
            Message::RuleMode(0, Tier::Focus, ProcessRuleMode::Enabled),
        );
        assert_eq!(settings.rules[0].focus_allowed_cpu_time_percent, 1);
        assert_eq!(settings.rules[0].background_mode, ProcessRuleMode::Default);
        assert!(settings.rules[0].has_valid_allowed_cpu_time());
        editor.update(&mut settings, Message::Remove(0));
        assert!(settings.rules.is_empty());
    }
}
