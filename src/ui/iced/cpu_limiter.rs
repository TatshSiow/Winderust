use iced::widget::{
    button, checkbox, column, pick_list, row, scrollable, slider, text, text_input,
};
use iced::{Element, Fill};
use rust_i18n::t;
use std::collections::HashMap;

use crate::config::{CpuLimiterRule, CpuLimiterSettings, ProcessRuleMode};
use crate::ui::process_rules::{can_add_cpu_limiter_process, new_cpu_limiter_rule};

#[derive(Default)]
pub(super) struct CpuLimiter {
    path: String,
    removing: Option<String>,
    deleting: Option<(usize, CpuLimiterRule)>,
    collapsed: bool,
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

    fn label(self) -> String {
        match self {
            Self::Focus => t!("cpu_limiter.focus_allowed_cpu_time"),
            Self::VisibleWindow => t!("cpu_limiter.visible_window_allowed_cpu_time"),
            Self::Background => t!("cpu_limiter.background_allowed_cpu_time"),
        }
        .to_string()
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
    Add,
    Default(Tier, u8),
    RuleEnabled(usize, bool),
    RuleMode(usize, Tier, ProcessRuleMode),
    RuleLimit(usize, Tier, u8),
    Remove(usize),
    ConfirmRemove,
    Removed(String),
    CancelRemove,
}

impl CpuLimiter {
    pub(super) fn update(&mut self, settings: &mut CpuLimiterSettings, message: Message) {
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
                self.removing = settings
                    .rules
                    .get(index)
                    .map(|rule| rule.executable_path.clone())
            }
            Message::CancelRemove => self.removing = None,
            Message::ConfirmRemove => {
                if let Some(path) = self.removing.take() {
                    if let Some(index) = settings
                        .rules
                        .iter()
                        .position(|rule| rule.executable_path == path)
                    {
                        self.deleting = Some((index, settings.rules.remove(index)));
                    }
                    self.numbers.retain(|(target, _), _| target != &path);
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
        if !enabled {
            return text(format!("{value}%")).into();
        }
        let change = move |value| match index {
            Some(index) => Message::RuleLimit(index, tier, value),
            None => Message::Default(tier, value),
        };
        row![
            button(text("- ")).on_press_maybe((value > 1).then(|| change(value.saturating_sub(1)))),
            slider(1..=100, value, change),
            button(text("+"))
                .on_press_maybe((value < 100).then(|| change(value.saturating_add(1)))),
            text_input("1-100", &self.number(path, tier, value))
                .on_input(move |raw| Message::Number(index, tier, raw))
                .width(65),
            text("%")
        ]
        .spacing(8)
        .into()
    }
    pub(super) fn view<'a>(
        &'a self,
        settings: &'a CpuLimiterSettings,
        candidates: &[String],
        motion_enabled: bool,
    ) -> Element<'a, Message> {
        let mut body = column![checkbox(settings.enabled)
            .label(t!("cpu_limiter.enable").to_string())
            .on_toggle(Message::Enabled)]
        .spacing(16);
        body = body
            .push(
                text(t!("cpu_limiter.intro_1").to_string())
                    .width(Fill)
                    .style(text::secondary),
            )
            .push(
                text(t!("cpu_limiter.intro_2").to_string())
                    .width(Fill)
                    .style(text::secondary),
            )
            .push(
                text(t!("cpu_limiter.intro_3").to_string())
                    .width(Fill)
                    .style(text::secondary),
            )
            .push(text(t!("cpu_limiter.intro_4").to_string()))
            .push(text(t!("cpu_limiter.intro_5").to_string()));
        if self.has_invalid_inputs() {
            body = body.push(
                text(t!("cpu_limiter.intro_2").to_string())
                    .width(Fill)
                    .style(text::danger),
            );
        }
        body = body.push(
            button(text(format!(
                "{} {}",
                if self.collapsed { ">" } else { "v" },
                t!("common.default")
            )))
            .on_press(Message::Collapse)
            .style(button::text),
        );
        let mut defaults = column![].spacing(12);
        for (tier, value) in Tier::ALL.into_iter().zip([
            settings.focus_allowed_cpu_time_percent,
            settings.visible_window_allowed_cpu_time_percent,
            settings.background_allowed_cpu_time_percent,
        ]) {
            defaults = defaults.push(
                column![
                    text(tier.label()),
                    self.limit_control(None, tier, "", value, settings.enabled)
                ]
                .spacing(6),
            );
        }
        body = body.push(super::motion::reveal(
            defaults,
            !self.collapsed,
            motion_enabled,
        ));
        body = body
            .push(
                text(t!("cpu_limiter.rules_help").to_string())
                    .width(Fill)
                    .style(text::secondary),
            )
            .push(
                row![
                    text_input(&t!("process_list.executable_path"), &self.path)
                        .on_input(Message::Path),
                    button(text(t!("common.browse_executable").to_string()))
                        .on_press_maybe(settings.enabled.then_some(Message::Browse)),
                    button(text(t!("common.add").to_string())).on_press_maybe(
                        (settings.enabled && can_add_cpu_limiter_process(settings, &self.path))
                            .then_some(Message::Add)
                    ),
                ]
                .spacing(8),
            );
        let filter = self.path.to_lowercase();
        let candidates: Vec<_> = candidates
            .iter()
            .filter(|path| {
                path.to_lowercase().contains(&filter) && can_add_cpu_limiter_process(settings, path)
            })
            .cloned()
            .collect();
        if settings.enabled && !candidates.is_empty() {
            body = body.push(
                pick_list(candidates, None::<String>, Message::Path)
                    .placeholder(t!("common.search_running_apps").to_string()),
            );
        }
        let mut cards = Vec::new();
        let mut visible_rules: Vec<_> = settings.rules.iter().enumerate().collect();
        if let Some((index, rule)) = &self.deleting {
            visible_rules.insert((*index).min(visible_rules.len()), (usize::MAX, rule));
        }
        for (index, rule) in visible_rules {
            let mut card = column![row![
                checkbox(rule.enabled)
                    .label(rule.executable_path.clone())
                    .on_toggle_maybe(
                        settings
                            .enabled
                            .then_some(move |value| Message::RuleEnabled(index, value))
                    ),
                button(text(t!("common.remove").to_string()))
                    .on_press_maybe(settings.enabled.then_some(Message::Remove(index))),
            ]
            .spacing(8)]
            .spacing(8);
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
                let choices: Vec<_> = ProcessRuleMode::ALL
                    .into_iter()
                    .map(|mode| Mode(mode, mode_label(mode)))
                    .collect();
                let selector: Element<'_, Message> = if settings.enabled {
                    pick_list(choices, Some(Mode(mode, mode_label(mode))), move |mode| {
                        Message::RuleMode(index, tier, mode.0)
                    })
                    .into()
                } else {
                    text(mode_label(mode)).into()
                };
                let mut controls = row![text(tier.label()).width(210), selector].spacing(8);
                if mode == ProcessRuleMode::Enabled {
                    controls = controls.push(self.limit_control(
                        Some(index),
                        tier,
                        &rule.executable_path,
                        value,
                        settings.enabled,
                    ));
                }
                card = card.push(controls);
            }
            if self.removing.as_deref() == Some(rule.executable_path.as_str()) {
                card = card.push(
                    row![
                        button(text(t!("common.remove").to_string()))
                            .on_press(Message::ConfirmRemove),
                        button(text(t!("common.cancel").to_string()))
                            .on_press(Message::CancelRemove),
                    ]
                    .spacing(8),
                );
            }
            cards.push((
                super::motion::key(&rule.executable_path),
                super::motion::removal(
                    iced::widget::container(card)
                        .padding(12)
                        .width(Fill)
                        .style(iced::widget::container::rounded_box),
                    index == usize::MAX,
                    motion_enabled,
                    Message::Removed(rule.executable_path.clone()),
                ),
            ));
        }
        body = body.push(iced::widget::keyed_column(cards).spacing(8));
        if settings.rules.is_empty() && self.deleting.is_none() {
            body = body.push(text(t!("cpu_limiter.no_rules").to_string()));
        }
        scrollable(body).spacing(10).height(Fill).into()
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
        editor.update(&mut settings, Message::ConfirmRemove);
        assert!(settings.rules.is_empty());
        assert!(editor.deleting.is_some());
        let path = editor.deleting.as_ref().unwrap().1.executable_path.clone();
        editor.update(&mut settings, Message::Removed(path));
        assert!(editor.deleting.is_none());
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
        editor.update(&mut settings, Message::CancelRemove);
        editor.update(&mut settings, Message::ConfirmRemove);
        assert_eq!(settings.rules.len(), 1);
    }
}
