use super::design;
use super::widgets::{checkbox, icon_button, pick_list, slider, text_input};
use crate::ui::scrolling::scrollable;
use iced::widget::{column, row, text, Row};
use iced::{Element, Fill};
use rust_i18n::t;
use std::collections::HashMap;
use std::ops::{Deref, DerefMut};

use crate::application::SettingsEditor;
use crate::config::{CpuLimiterRule, CpuLimiterSettings, ProcessRuleMode};
use crate::control::cpu_limiter::CpuLimiterActivation;
use crate::features::cpu_control::cpu_limiter::CpuLimiterSnapshot;
use crate::ui::process_rules::{
    can_add_cpu_limiter_process, exact_path_input, new_cpu_limiter_rule, picked_path_input,
};

pub(super) fn limit_slider<'a, M: Clone + 'a>(
    value: u8,
    change: impl Fn(u8) -> M + 'a,
) -> Row<'a, M> {
    let decrease = (value > 1).then(|| change(value.saturating_sub(1)));
    let increase = (value < 100).then(|| change(value.saturating_add(1)));
    row![
        icon_button("icons/minus.svg")
            .style(super::widgets::control_button)
            .on_press_maybe(decrease),
        slider(1..=100, value, change).width(Fill),
        icon_button("icons/plus.svg")
            .style(super::widgets::control_button)
            .on_press_maybe(increase),
    ]
    .spacing(design::space::SMALL)
    .align_y(iced::Center)
}

#[derive(Default)]
pub(super) struct CpuLimiter {
    path: String,
    draft: Option<(usize, CpuLimiterSettings)>,
    numbers: HashMap<(String, Tier), String>,
}

struct SelectedLimiter<'a>(&'a mut SettingsEditor);

impl Deref for SelectedLimiter<'_> {
    type Target = CpuLimiterSettings;

    fn deref(&self) -> &Self::Target {
        &self.0.cpu_limiter
    }
}

impl DerefMut for SelectedLimiter<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0.cpu_limiter
    }
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

    fn default_limit(self, settings: &CpuLimiterSettings) -> u8 {
        match self {
            Self::Focus => settings.focus_allowed_cpu_time_percent,
            Self::VisibleWindow => settings.visible_window_allowed_cpu_time_percent,
            Self::Background => settings.background_allowed_cpu_time_percent,
        }
    }

    fn rule_values(self, rule: &CpuLimiterRule) -> (ProcessRuleMode, u8) {
        match self {
            Self::Focus => (rule.focus_mode, rule.focus_allowed_cpu_time_percent),
            Self::VisibleWindow => (
                rule.visible_window_mode,
                rule.visible_window_allowed_cpu_time_percent,
            ),
            Self::Background => (
                rule.background_mode,
                rule.background_allowed_cpu_time_percent,
            ),
        }
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

    pub(super) fn update_profile(&mut self, settings: &mut SettingsEditor, message: Message) {
        self.update(SelectedLimiter(settings), message);
    }

    pub(super) fn update(
        &mut self,
        mut settings: impl DerefMut<Target = CpuLimiterSettings>,
        message: Message,
    ) {
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
                let Some((index, draft)) = &self.draft else {
                    return;
                };
                let edited = &draft.rules[*index];
                if self.has_invalid_inputs_for(Some(&edited.executable_path)) {
                    return;
                }
                if settings
                    .rules
                    .get(*index)
                    .is_some_and(|rule| rule != edited)
                {
                    settings.rules[*index] = edited.clone();
                }
                self.draft = None;
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
    fn update_fields(
        &mut self,
        mut settings: impl DerefMut<Target = CpuLimiterSettings>,
        message: Message,
    ) {
        match message {
            Message::Enabled(value) => {
                if settings.enabled != value {
                    settings.enabled = value;
                }
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
                    self.update_fields(
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
                let path = exact_path_input(&self.path);
                if can_add_cpu_limiter_process(&settings, path) {
                    settings.rules.push(new_cpu_limiter_rule(path));
                    self.path.clear();
                }
            }
            Message::Default(tier, value) => {
                self.numbers.remove(&(String::new(), tier));
                let value = value.clamp(1, 100);
                if tier.default_limit(&settings) == value {
                    return;
                }
                let field = match tier {
                    Tier::Focus => &mut settings.focus_allowed_cpu_time_percent,
                    Tier::VisibleWindow => &mut settings.visible_window_allowed_cpu_time_percent,
                    Tier::Background => &mut settings.background_allowed_cpu_time_percent,
                };
                *field = value;
            }
            Message::RuleEnabled(index, value) => {
                if settings
                    .rules
                    .get(index)
                    .is_some_and(|rule| rule.enabled != value)
                {
                    settings.rules[index].enabled = value;
                }
            }
            Message::RuleMode(index, tier, value) => {
                if let Some(rule) = settings.rules.get(index) {
                    self.numbers.remove(&(rule.executable_path.clone(), tier));
                    if tier.rule_values(rule).0 != value {
                        *tier.rule_fields(&mut settings.rules[index]).0 = value;
                    }
                }
            }
            Message::RuleLimit(index, tier, value) => {
                if let Some(rule) = settings.rules.get(index) {
                    self.numbers.remove(&(rule.executable_path.clone(), tier));
                    let value = value.clamp(1, 100);
                    if tier.rule_values(rule).1 != value {
                        *tier.rule_fields(&mut settings.rules[index]).1 = value;
                    }
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
        self.has_invalid_inputs_for(None)
    }

    fn has_invalid_inputs_for(&self, executable_path: Option<&str>) -> bool {
        self.numbers.iter().any(|((path, _), raw)| {
            executable_path.is_none_or(|target| path == target)
                && raw
                    .parse::<u8>()
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
    ) -> Element<'_, Message> {
        let change = move |value| match index {
            Some(index) => Message::RuleLimit(index, tier, value),
            None => Message::Default(tier, value),
        };
        row![
            limit_slider(value, change),
            text_input("1-100", &self.number(path, tier, value))
                .on_input(move |raw| Message::Number(index, tier, raw))
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
    ) -> Element<'_, Message> {
        column![
            text(super::widgets::label_with_unit(&tier.label(), "%")).width(Fill),
            self.limit_control(index, tier, path, value)
        ]
        .spacing(design::space::SMALL)
        .into()
    }
    pub(super) fn view<'a>(
        &'a self,
        settings: &'a CpuLimiterSettings,
        candidates: &'a [super::app_picker::Candidate],
        master_enabled: bool,
        status: Option<&CpuLimiterSnapshot>,
    ) -> Element<'a, Message> {
        let mut body = column![super::widgets::toggle_card(
            "cpu_limiter.enable",
            settings.enabled,
            Some(Message::Enabled),
        )]
        .spacing(super::widgets::CARD_GAP);
        if self.has_invalid_inputs() {
            body = body.push(
                text(t!("cpu_limiter.invalid_limit").to_string())
                    .width(Fill)
                    .style(text::danger),
            );
        }
        let defaults =
            Tier::ALL.map(|tier| self.limit_row(None, tier, "", tier.default_limit(settings)));
        body = body
            .push(super::widgets::heading(
                t!("cpu_limiter.limits").to_string(),
                design::typography::BODY,
            ))
            .push(super::widgets::settings_card(
                super::widgets::settings_rows(defaults),
            ))
            .push(super::widgets::setting_title_with_help(
                "common.rules",
                "",
                "cpu_limiter.rules_help",
            ))
            .push(super::widgets::settings_card(
                super::app_picker::view_with_path_selection(
                    &self.path,
                    candidates,
                    true,
                    (Message::Path, |path| Message::Path(picked_path_input(path))),
                    Message::Browse,
                    (can_add_cpu_limiter_process(settings, exact_path_input(&self.path)))
                        .then_some(Message::Add),
                    |path| can_add_cpu_limiter_process(settings, path).then_some(true),
                ),
            ));
        let mut cards = Vec::new();
        for (index, rule) in settings.rules.iter().enumerate() {
            let mut cells = Vec::new();
            for tier in Tier::ALL {
                let (mode, _) = tier.rule_values(rule);
                let choices: Vec<_> = ProcessRuleMode::ALL
                    .into_iter()
                    .map(|mode| Mode(mode, mode_label(mode)))
                    .collect();
                let selector: Element<'_, Message> = {
                    pick_list(choices, Some(Mode(mode, mode_label(mode))), move |mode| {
                        Message::RuleMode(index, tier, mode.0)
                    })
                    .width(Fill)
                    .into()
                };
                cells.push(selector);
            }

            let key = rule_status(rule, master_enabled && settings.enabled, status);
            let chip = iced::widget::container(text(t!(key).to_string()))
                .padding([4, 8])
                .style(move |theme| super::widgets::rule_status_chip(theme, key));
            let header = super::widgets::process_rule_header(
                &rule.executable_path,
                candidates,
                checkbox(rule.enabled)
                    .on_toggle_maybe(Some(move |v| Message::RuleEnabled(index, v)))
                    .into(),
                Some(chip.into()),
                cells,
                super::widgets::rule_actions(
                    Some(Message::Edit(index)),
                    Some(Message::Remove(index)),
                ),
            );
            cards.push((
                super::widgets::stable_key(&rule.executable_path),
                column![header, super::widgets::settings_divider()].into(),
            ));
        }
        body = body.push(super::widgets::process_rules_table(
            [
                t!("cpu_allocation.focus").to_string(),
                t!("common.visible_window").to_string(),
                t!("common.background_process").to_string(),
            ],
            cards,
            true,
        ));
        scrollable(body).height(Fill).into()
    }
    pub(super) fn modal<'a>(
        &'a self,
        candidates: &[super::app_picker::Candidate],
        master_enabled: bool,
        status: Option<&CpuLimiterSnapshot>,
    ) -> Option<Element<'a, Message>> {
        let (index, settings) = self.draft.as_ref()?;
        let index = *index;
        let rule = settings.rules.get(index)?;
        let key = rule_status(rule, master_enabled && settings.enabled, status);
        let chip = iced::widget::container(text(t!(key).to_string()))
            .padding([4, 8])
            .style(move |theme| super::widgets::rule_status_chip(theme, key));
        let mut status_rows = vec![super::widgets::setting_row("common.status", chip).into()];
        if let Some(limit) = status
            .filter(|_| key == "common.applied")
            .and_then(|status| active_limit_label(rule.executable_path.trim_start(), status))
        {
            status_rows
                .push(super::widgets::setting_row("cpu_limiter.active_target", text(limit)).into());
        }
        let mut details = column![
            super::app_picker::app_name(&rule.executable_path, candidates),
            text(rule.executable_path.as_str())
                .style(text::secondary)
                .wrapping(text::Wrapping::WordOrGlyph),
            super::widgets::setting_title("common.rule_details"),
            super::widgets::settings_card(super::widgets::settings_rows(status_rows)),
        ]
        .spacing(super::widgets::CARD_GAP);
        let mut limits = Vec::new();
        for tier in Tier::ALL {
            let (mode, value) = tier.rule_values(rule);
            let mut controls = column![super::widgets::setting_row_with_unit(
                tier.key(),
                "%",
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
            .spacing(design::space::SMALL);
            if mode == ProcessRuleMode::Enabled {
                controls = controls.push(self.limit_control(
                    Some(index),
                    tier,
                    &rule.executable_path,
                    value,
                ));
            }
            limits.push(controls.into());
        }
        details = details
            .push(super::widgets::heading(
                t!("cpu_limiter.limits").to_string(),
                design::typography::BODY,
            ))
            .push(super::widgets::settings_card(
                super::widgets::settings_rows(limits),
            ));
        Some(super::widgets::rule_dialog(
            t!("common.edit_rule").to_string(),
            scrollable(details).height(Fill),
            Message::CancelRule,
            (!self.has_invalid_inputs_for(Some(&rule.executable_path)))
                .then_some(Message::SaveRule),
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

fn rule_status(
    rule: &CpuLimiterRule,
    enabled: bool,
    status: Option<&CpuLimiterSnapshot>,
) -> &'static str {
    if !enabled || !rule.enabled {
        return "common.inactive";
    }
    let rule_path = rule.executable_path.trim_start();
    let Some(status) = status.filter(|_| !rule_path.is_empty()) else {
        return "common.unknown";
    };
    let matches_rule = |path: &String| matches_rule_path(rule_path, path);
    if status.failed_apps.iter().any(matches_rule) {
        "common.error"
    } else if status.observation_paused {
        "sidebar_status.paused"
    } else if !status.enabled {
        "common.unknown"
    } else if status.restore_pending_apps.iter().any(matches_rule) {
        "cpu_limiter.restore_pending"
    } else if matching_active_limits(rule_path, status).next().is_some() {
        "common.applied"
    } else {
        "common.waiting"
    }
}

fn matches_rule_path(rule_path: &str, actual_path: &str) -> bool {
    crate::foreground::same_executable_path(
        std::path::Path::new(rule_path),
        std::path::Path::new(actual_path),
    )
}

fn matching_active_limits<'a>(
    rule_path: &'a str,
    status: &'a CpuLimiterSnapshot,
) -> impl Iterator<Item = CpuLimiterActivation> + 'a {
    status
        .active_limits
        .iter()
        .filter_map(move |(path, limit)| matches_rule_path(rule_path, path).then_some(*limit))
}

fn active_limit_label(rule_path: &str, status: &CpuLimiterSnapshot) -> Option<String> {
    use CpuLimiterActivation::{AncestorJob, Job, ThreadFallback};

    let mut limits = matching_active_limits(rule_path, status);
    let first = limits.next()?;
    let label = if limits.all(|limit| limit == first) {
        match first {
            Job(percent) => t!("cpu_limiter.active_app_group", percent = percent),
            AncestorJob(percent) => t!("cpu_limiter.active_inherited", percent = percent),
            ThreadFallback(percent) => t!("cpu_limiter.active_thread_fallback", percent = percent),
        }
    } else {
        t!("cpu_limiter.active_varies")
    };
    Some(label.into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn limit_labels_and_controls_fit_beside_the_dock() {
        use iced::advanced::{layout, widget::Tree};
        let renderer = iced::Renderer::new(iced::Font::DEFAULT, iced::Pixels(14.0));
        let editor = CpuLimiter::default();
        for tier in Tier::ALL {
            let mut view = editor.limit_row(None, tier, "", 50);
            let mut tree = Tree::new(&view);
            let node = view.as_widget_mut().layout(
                &mut tree,
                &renderer,
                &layout::Limits::new(iced::Size::ZERO, iced::Size::new(400.0, 1000.0)),
            );
            let label = node.children()[0].bounds();
            let control = node.children()[1].bounds();
            assert_eq!(label.width, 400.0);
            assert_eq!(control.width, 400.0);
            assert!(control.y >= label.y + label.height);
            assert!(control.height <= 40.0);
        }
    }

    #[test]
    fn rule_status_uses_confirmed_limited_apps_and_enable_switches() {
        let mut rule = new_cpu_limiter_rule(r"C:\Apps\test.exe");
        rule.executable_path.insert(0, ' ');
        rule.enabled = true;
        let mut status = CpuLimiterSnapshot {
            enabled: true,
            ..Default::default()
        };
        assert_eq!(rule_status(&rule, true, Some(&status)), "common.waiting");
        status.active_limits.push((
            concat!(r"C:\Apps\test.exe", "\u{3000}").into(),
            CpuLimiterActivation::Job(40),
        ));
        assert_eq!(rule_status(&rule, true, Some(&status)), "common.waiting");
        status
            .active_limits
            .push((r"c:\apps\TEST.exe".into(), CpuLimiterActivation::Job(40)));
        assert_eq!(rule_status(&rule, true, Some(&status)), "common.applied");
        status.restore_pending_apps.push(r"c:\apps\TEST.exe".into());
        assert_eq!(
            rule_status(&rule, true, Some(&status)),
            "cpu_limiter.restore_pending"
        );
        assert_eq!(rule_status(&rule, false, Some(&status)), "common.inactive");
        status.observation_paused = true;
        status.failed_apps.push(r"C:\Apps\test.exe".into());
        assert_eq!(rule_status(&rule, true, Some(&status)), "common.error");
        status.failed_apps.clear();
        assert_eq!(
            rule_status(&rule, true, Some(&status)),
            "sidebar_status.paused"
        );
        status.observation_paused = false;
        status.enabled = false;
        assert_eq!(rule_status(&rule, true, Some(&status)), "common.unknown");
        rule.enabled = false;
        assert_eq!(rule_status(&rule, true, Some(&status)), "common.inactive");
    }

    #[test]
    fn runtime_limit_label_uses_matching_targets_and_reports_different_limits() {
        use CpuLimiterActivation::{AncestorJob, Job, ThreadFallback};

        let path = r"C:\Apps\test.exe";
        let mut status = CpuLimiterSnapshot {
            active_limits: vec![(r"C:\Other\test.exe".into(), Job(55))],
            ..Default::default()
        };
        assert_eq!(active_limit_label(path, &status), None);

        let matching = r"c:\apps\TEST.exe";
        status.active_limits.push((matching.into(), Job(40)));
        status.active_limits.push((matching.into(), Job(40)));
        assert_eq!(
            active_limit_label(path, &status).as_deref(),
            Some(t!("cpu_limiter.active_app_group", percent = 40).as_ref())
        );
        status.active_limits[2].1 = Job(25);
        assert_eq!(
            active_limit_label(path, &status).as_deref(),
            Some(t!("cpu_limiter.active_varies").as_ref())
        );

        status.active_limits.pop();
        status.active_limits[1].1 = ThreadFallback(40);
        assert_eq!(
            active_limit_label(path, &status).as_deref(),
            Some(t!("cpu_limiter.active_thread_fallback", percent = 40).as_ref())
        );
        status.active_limits[1].1 = AncestorJob(60);
        assert_eq!(
            active_limit_label(path, &status).as_deref(),
            Some(t!("cpu_limiter.active_inherited", percent = 60).as_ref())
        );
    }

    #[test]
    fn rule_editor_cancel_and_save_are_isolated() {
        let mut editor = CpuLimiter::default();
        let mut settings = crate::config::Settings::default();
        settings
            .cpu_limiter
            .rules
            .push(new_cpu_limiter_rule(r"C:\Apps\test.exe"));
        let before = settings.clone();
        let mut settings = SettingsEditor::with_settings(settings);
        settings.select_power_source(crate::config::PowerSourceProfile::OnBattery);
        editor.update_profile(&mut settings, Message::Number(None, Tier::Focus, "".into()));
        editor.update_profile(&mut settings, Message::Edit(0));
        editor.update_profile(&mut settings, Message::SaveRule);
        assert_eq!(settings.global(), &before);
        editor.update_profile(&mut settings, Message::Edit(0));
        assert!(editor.has_pending_editor());
        editor.update_profile(&mut settings, Message::RuleLimit(0, Tier::Focus, 25));
        assert_eq!(settings.global(), &before);
        editor.update_profile(&mut settings, Message::CancelRule);
        assert!(!editor.has_pending_editor());
        assert!(editor.has_invalid_inputs());
        assert_eq!(settings.global(), &before);
        editor.update_profile(&mut settings, Message::Edit(0));
        editor.update_profile(
            &mut settings,
            Message::Number(Some(0), Tier::Focus, "101".into()),
        );
        editor.update_profile(&mut settings, Message::SaveRule);
        assert!(editor.has_pending_editor());
        assert_eq!(settings.global(), &before);
        editor.update_profile(
            &mut settings,
            Message::Number(Some(0), Tier::Focus, "25".into()),
        );
        editor.update_profile(&mut settings, Message::SaveRule);
        assert!(!editor.has_pending_editor());
        assert_eq!(
            settings.cpu_limiter.rules[0].focus_allowed_cpu_time_percent,
            25
        );
        assert_eq!(settings.global().cpu_limiter, before.cpu_limiter);
        assert!(editor.has_invalid_inputs());
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
