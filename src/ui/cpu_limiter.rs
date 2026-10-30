use super::widgets::{self, Ui};
use rust_i18n::t;
use std::collections::HashMap;
use windui::prelude::{signal, Element};
use windui::theme::Intent;

use crate::config::{CpuLimiterRule, CpuLimiterSettings, ProcessRuleMode};
use crate::ui::process_rules::{can_add_cpu_limiter_process, new_cpu_limiter_rule};

#[derive(Default)]
pub(super) struct CpuLimiter {
    path: String,
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
    pub(super) fn has_pending_editor(&self) -> bool {
        self.draft.is_some()
    }

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
                if can_add_cpu_limiter_process(settings, &self.path) {
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
    fn limit_row(
        &self,
        index: Option<usize>,
        tier: Tier,
        path: &str,
        value: u8,
        ui: Ui<Message>,
    ) -> Element {
        widgets::setting_row(
            format!("{} (%)", tier.label()),
            ui.number(
                signal(self.number(path, tier, value)),
                1,
                100,
                1,
                move |raw| Message::Number(index, tier, raw),
            ),
        )
    }

    pub(super) fn view(
        &self,
        settings: &CpuLimiterSettings,
        candidates: &[super::app_picker::Candidate],
        master_enabled: bool,
        status: &crate::features::cpu_control::cpu_limiter::CpuLimiterSnapshot,
        ui: Ui<Message>,
    ) -> Element {
        let mut body = Vec::new();
        if self.has_invalid_inputs() {
            body.push(widgets::error(t!("cpu_limiter.invalid_limit").to_string()));
        }
        let mut defaults = vec![widgets::setting_row(
            t!("cpu_limiter.enable").to_string(),
            ui.toggle(settings.enabled, Message::Enabled),
        )];
        for (tier, value) in Tier::ALL.into_iter().zip([
            settings.focus_allowed_cpu_time_percent,
            settings.visible_window_allowed_cpu_time_percent,
            settings.background_allowed_cpu_time_percent,
        ]) {
            defaults.push(Element::divider());
            defaults.push(self.limit_row(None, tier, "", value, ui.clone()));
        }
        body.push(widgets::card("", defaults));
        body.push(
            widgets::setting_title("cpu_limiter.rules")
                .font_size(super::design::typography::SUBTITLE as f32)
                .font_weight(600),
        );
        let candidate_settings = settings.clone();
        body.push(super::app_picker::view(
            &self.path,
            candidates,
            true,
            Message::Path,
            Message::Browse,
            Some(Message::Add),
            move |candidate| {
                can_add_cpu_limiter_process(
                    &candidate_settings,
                    &candidate.info.image_path.to_string_lossy(),
                )
                .then_some(true)
            },
            ui.clone(),
        ));
        let mut rows = Vec::new();
        for (index, rule) in settings.rules.iter().enumerate() {
            let key = rule_status(rule, master_enabled && settings.enabled, status);
            let intent = match key {
                "common.applied" => Intent::Success,
                "common.error" => Intent::Danger,
                "common.waiting" => Intent::Warning,
                _ => Intent::Neutral,
            };
            let mut controls = Vec::new();
            for (tier, mode) in Tier::ALL.into_iter().zip([
                rule.focus_mode,
                rule.visible_window_mode,
                rule.background_mode,
            ]) {
                controls.push(ui.select(
                    ProcessRuleMode::ALL.map(|mode| Mode(mode, mode_label(mode))),
                    Mode(mode, mode_label(mode)),
                    move |mode| Message::RuleMode(index, tier, mode.0),
                ));
            }
            rows.push(widgets::process_rule_header(
                &rule.executable_path,
                candidates,
                ui.checkbox("", rule.enabled, move |value| {
                    Message::RuleEnabled(index, value)
                }),
                Some(Element::badge_intent(t!(key).to_string(), intent)),
                controls,
                widgets::row().children([
                    widgets::icon_button(
                        &ui,
                        "icons/pencil.svg",
                        t!("common.edit").to_string(),
                        Message::Edit(index),
                    ),
                    widgets::rule_delete_button(&ui, Some(Message::Remove(index))),
                ]),
            ));
        }
        body.push(widgets::process_rules_table_with_actions(
            [
                t!("cpu_allocation.focus").to_string(),
                t!("common.visible_window").to_string(),
                t!("common.background_process").to_string(),
            ],
            rows,
            t!("cpu_limiter.no_rules").to_string(),
            96,
            true,
        ));
        widgets::page(body)
    }

    pub(super) fn modal(
        &self,
        candidates: &[super::app_picker::Candidate],
        status: &crate::features::cpu_control::cpu_limiter::CpuLimiterSnapshot,
        ui: Ui<Message>,
    ) -> Option<Element> {
        let (index, settings) = self.draft.as_ref()?;
        let index = *index;
        let rule = settings.rules.get(index)?;
        let numbers = signal(self.numbers.clone());
        let path = rule.executable_path.clone();
        let ui = ui.inspect(move |message: &Message, _refresh| {
            if let Message::Number(_, tier, raw) = message {
                numbers.update(|numbers| {
                    numbers.insert((path.clone(), *tier), raw.clone());
                });
            }
        });
        let key = rule_status(rule, settings.enabled, status);
        let mut body = vec![
            super::app_picker::app_name(&rule.executable_path, candidates),
            widgets::label(rule.executable_path.clone()),
            widgets::label(t!(key).to_string()),
        ];
        body.push(Element::label(numbers.map(|numbers| {
            if numbers.values().any(|raw| {
                raw.parse::<u8>()
                    .ok()
                    .is_none_or(|value| !(1..=100).contains(&value))
            }) {
                t!("cpu_limiter.invalid_limit").to_string()
            } else {
                String::new()
            }
        })));
        let mut native_numbers = Vec::new();
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
            let mut controls = vec![ui.select(
                ProcessRuleMode::ALL.map(|mode| Mode(mode, mode_label(mode))),
                Mode(mode, mode_label(mode)),
                move |mode| Message::RuleMode(index, tier, mode.0),
            )];
            if mode == ProcessRuleMode::Enabled {
                let raw = signal(self.number(&rule.executable_path, tier, value));
                native_numbers.push((tier, raw));
                controls.push(widgets::setting_row(
                    format!("{} (%)", tier.label()),
                    ui.number(raw, 1, 100, 1, move |raw| {
                        Message::Number(Some(index), tier, raw)
                    })
                    .enabled(rule.enabled),
                ));
            }
            body.push(widgets::card(tier.label(), controls));
        }
        let other_numbers_valid = self.numbers.iter().all(|((path, tier), raw)| {
            (path == &rule.executable_path
                && native_numbers.iter().any(|(visible, _)| visible == tier))
                || raw
                    .parse::<u8>()
                    .is_ok_and(|value| (1..=100).contains(&value))
        });
        let footer = widgets::row().children([
            ui.button(t!("common.cancel").to_string(), Message::CancelRule),
            ui.primary_button(t!("common.save").to_string(), Message::SaveRule)
                .enabled_when(move || {
                    other_numbers_valid
                        && native_numbers.iter().all(|(_, raw)| {
                            raw.get()
                                .parse::<u8>()
                                .is_ok_and(|value| (1..=100).contains(&value))
                        })
                }),
        ]);
        Some(widgets::editor_panel(
            t!("common.edit").to_string(),
            widgets::col().children(body),
            footer,
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

fn rule_status(
    rule: &CpuLimiterRule,
    enabled: bool,
    status: &crate::features::cpu_control::cpu_limiter::CpuLimiterSnapshot,
) -> &'static str {
    if !enabled || !rule.enabled {
        "common.inactive"
    } else if rule.executable_path.trim().is_empty() {
        "common.unknown"
    } else if status.failed_apps.iter().any(|path| {
        crate::foreground::same_executable_path(
            std::path::Path::new(path),
            std::path::Path::new(&rule.executable_path),
        )
    }) {
        "common.error"
    } else if status.enabled
        && status.limited_apps.iter().any(|path| {
            crate::foreground::same_executable_path(
                std::path::Path::new(path),
                std::path::Path::new(&rule.executable_path),
            )
        })
    {
        "common.applied"
    } else if !status.enabled {
        "common.unknown"
    } else {
        "common.waiting"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rule_status_uses_confirmed_limited_apps_and_enable_switches() {
        let mut rule = new_cpu_limiter_rule(r"C:\Apps\test.exe");
        rule.enabled = true;
        let mut status = crate::features::cpu_control::cpu_limiter::CpuLimiterSnapshot {
            enabled: true,
            ..Default::default()
        };
        assert_eq!(rule_status(&rule, true, &status), "common.waiting");
        status.limited_apps.push(r"c:\apps\TEST.exe".into());
        assert_eq!(rule_status(&rule, true, &status), "common.applied");
        assert_eq!(rule_status(&rule, false, &status), "common.inactive");
        status.failed_apps.push(rule.executable_path.clone());
        assert_eq!(rule_status(&rule, true, &status), "common.error");
        status.failed_apps.clear();
        status.enabled = false;
        assert_eq!(rule_status(&rule, true, &status), "common.unknown");
        rule.enabled = false;
        assert_eq!(rule_status(&rule, true, &status), "common.inactive");
    }

    #[test]
    fn rule_editor_cancel_and_save_are_isolated() {
        let mut editor = CpuLimiter::default();
        let mut settings = CpuLimiterSettings::default();
        settings
            .rules
            .push(new_cpu_limiter_rule(r"C:\Apps\test.exe"));
        let before = settings.clone();
        editor.update(&mut settings, Message::Edit(0));
        assert!(editor.has_pending_editor());
        editor.update(&mut settings, Message::RuleLimit(0, Tier::Focus, 25));
        assert_eq!(settings, before);
        editor.update(&mut settings, Message::CancelRule);
        assert!(!editor.has_pending_editor());
        assert_eq!(settings, before);
        editor.update(&mut settings, Message::Edit(0));
        assert!(editor.has_pending_editor());
        editor.update(&mut settings, Message::RuleLimit(0, Tier::Focus, 25));
        editor.update(&mut settings, Message::SaveRule);
        assert!(!editor.has_pending_editor());
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
