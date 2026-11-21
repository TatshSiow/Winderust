mod preview;

pub(super) use preview::PreviewMessage;

use super::{
    app_picker, design,
    widgets::{self, checkbox},
};
use crate::ui::scrolling::scrollable;
use crate::{
    application::SettingsEditor,
    config::{ByFocusRule, ByRunningAppRule, ExecutableSelector, Settings},
    control::instance_limits::validate_path,
    power::PowerPlan,
    ui::process_rules::*,
};
use iced::widget::{column, row, text};
use iced::{Element, Fill};
use rust_i18n::t;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Focus,
    RunningApp,
}

#[derive(Debug, Clone)]
pub(super) enum Message {
    Enabled(bool),
    Path(String),
    MatchBy(MatchBy),
    Name(String),
    Browse,
    Add,
    AddName,
    RuleEnabled(usize, bool),
    Plan(usize, Option<String>),
    Remove(usize),
    Preview(PreviewMessage),
}

#[derive(Default)]
pub(super) struct Editor {
    path: String,
    match_by: MatchBy,
    name: String,
    preview: preview::Preview,
}

impl Editor {
    pub(super) fn update_preview(
        &mut self,
        settings: &Settings,
        message: PreviewMessage,
    ) -> iced::Task<Message> {
        self.preview.update(settings, message).map(Message::Preview)
    }

    pub(super) fn preview_open(&self) -> bool {
        self.preview.is_open()
    }

    pub(super) fn close_preview(&mut self) {
        self.preview.close();
    }

    pub(super) fn preview_modal<'a>(&'a self, plans: &[PowerPlan]) -> Option<Element<'a, Message>> {
        self.preview
            .view(plans)
            .map(|modal| modal.map(Message::Preview))
    }

    pub(super) fn update(
        &mut self,
        kind: Kind,
        settings: &mut SettingsEditor,
        plans: &[PowerPlan],
        message: Message,
    ) {
        self.close_preview();
        let active_plan = || {
            plans
                .iter()
                .find(|plan| plan.active && !plan.managed_adaptive)
                .map(|plan| plan.guid.clone())
        };
        // Both pages have the same rule editor, but keep their distinct typed settings and policies.
        macro_rules! update_rules {
            ($field:ident, $can_add_path:ident, $can_add_name:ident, $new_rule:ident, $rule:ident) => {{
                match message {
                    Message::Enabled(value) => {
                        if settings.$field.enabled != value {
                            settings.$field.enabled = value;
                        }
                    }
                    Message::Path(path) => self.path = path,
                    Message::MatchBy(match_by) => self.match_by = match_by,
                    Message::Name(name) => self.name = name,
                    Message::Browse => {}
                    Message::Preview(_) => unreachable!("preview messages do not edit rules"),
                    Message::Add => {
                        if $can_add_path(&settings.$field, &self.path) {
                            settings
                                .$field
                                .rules
                                .push($new_rule(self.path.trim(), active_plan()));
                            self.path.clear();
                        }
                    }
                    Message::AddName => {
                        if $can_add_name(&settings.$field, &self.name) {
                            settings.$field.rules.push($rule {
                                enabled: true,
                                name: self.name.clone(),
                                selector: ExecutableSelector::ExactName {
                                    executable_name: std::mem::take(&mut self.name),
                                },
                                power_plan_guid: active_plan(),
                            });
                        }
                    }
                    Message::RuleEnabled(index, value) => {
                        if settings
                            .$field
                            .rules
                            .get(index)
                            .is_some_and(|rule| rule.enabled != value)
                        {
                            settings.$field.rules[index].enabled = value;
                        }
                    }
                    Message::Plan(index, value) => {
                        if settings
                            .$field
                            .rules
                            .get(index)
                            .is_some_and(|rule| rule.power_plan_guid != value)
                        {
                            settings.$field.rules[index].power_plan_guid = value;
                        }
                    }
                    Message::Remove(index) => {
                        if index < settings.$field.rules.len() {
                            settings.$field.rules.remove(index);
                        }
                    }
                }
            }};
        }
        match kind {
            Kind::Focus => update_rules!(
                by_focus,
                can_add_by_focus_process,
                can_add_by_focus_name,
                new_by_focus_rule,
                ByFocusRule
            ),
            Kind::RunningApp => update_rules!(
                by_running_app,
                can_add_by_running_app_process,
                can_add_by_running_app_name,
                new_by_running_app_rule,
                ByRunningAppRule
            ),
        }
    }

    fn name_input(&self, can_add: bool) -> Element<'static, Message> {
        let add = can_add.then_some(Message::AddName);
        row![
            widgets::text_input("app.exe", &self.name)
                .on_input(Message::Name)
                .on_submit_maybe(add.clone())
                .width(Fill),
            widgets::action_button(t!("common.add").to_string())
                .style(widgets::primary_button)
                .on_press_maybe(add),
        ]
        .align_y(iced::Center)
        .spacing(design::space::SMALL)
        .into()
    }

    pub(super) fn view<'a>(
        &'a self,
        kind: Kind,
        settings: &'a Settings,
        plans: &[PowerPlan],
        candidates: &'a [app_picker::Candidate],
        runtime_settings: Option<&Settings>,
        status: &crate::control::power_plan::PowerPlanStatus,
    ) -> Element<'a, Message> {
        let master_enabled = settings.general.enabled;
        let state = match kind {
            Kind::Focus => crate::rules::DecisionState::ByFocus,
            Kind::RunningApp => crate::rules::DecisionState::ByRunningApp,
        };
        macro_rules! render_rules {
            ($field:ident, $can_add_path:ident, $can_add_name:ident, $enable:literal) => {{
                let feature = &settings.$field;
                let selected_index = super::power_rules::runtime_rule_index_in_draft(
                    runtime_settings.map(|settings| settings.$field.rules.as_slice()),
                    &feature.rules,
                    status.rule_index,
                );
                let mut body = column![super::widgets::toggle_card(
                    $enable,
                    feature.enabled,
                    Some(Message::Enabled)
                ),]
                .spacing(super::widgets::CARD_GAP);
                let input = if self.match_by == MatchBy::ExecutableName {
                    self.name_input($can_add_name(feature, &self.name))
                } else {
                    let can_add = $can_add_path(feature, &self.path);
                    let mut picker = column![app_picker::view(
                        &self.path,
                        candidates,
                        true,
                        Message::Path,
                        Message::Browse,
                        can_add.then_some(Message::Add),
                        |path| $can_add_path(feature, path).then_some(true),
                    )]
                    .spacing(design::space::SMALL);
                    if !can_add
                        && app_picker::is_path_query(&self.path)
                        && validate_path(self.path.trim()).is_err()
                    {
                        picker = picker.push(app_picker::path_format_hint());
                    }
                    picker.into()
                };
                let input = widgets::settings_card(widgets::settings_rows([
                    widgets::setting_row(
                        "common.match_by",
                        widgets::pick_list(
                            vec![MatchBy::ExecutablePath, MatchBy::ExecutableName],
                            Some(self.match_by),
                            Message::MatchBy,
                        )
                        .width(design::SELECT_WIDTH),
                    )
                    .into(),
                    input,
                ]));
                let mut rules_body = column![input].spacing(widgets::CARD_GAP);
                let mut cards = Vec::new();
                for (index, rule) in feature.rules.iter().enumerate() {
                    let target = rule.selector.value();
                    let is_path = rule.selector.exact_path().is_some();
                    let selector =
                        widgets::plan(rule.power_plan_guid.as_deref(), plans, move |guid| {
                            Message::Plan(index, guid)
                        });
                    let key = super::power_rules::power_rule_status(
                        master_enabled && feature.enabled && rule.enabled,
                        !target.trim().is_empty(),
                        selected_index == Some(index),
                        state,
                        [rule.power_plan_guid.as_deref(), None],
                        status,
                    );
                    let chip = iced::widget::container(iced::widget::text(t!(key).to_string()))
                        .padding([4, 8])
                        .style(move |theme| widgets::rule_status_chip(theme, key));
                    let (app, location) = if is_path {
                        (app_picker::app_name(target, candidates), target.to_owned())
                    } else {
                        (
                            text(target.to_owned()).into(),
                            t!("common.any_path").to_string(),
                        )
                    };
                    let mut card = column![widgets::process_rule_header_with_target(
                        app,
                        location,
                        checkbox(rule.enabled)
                            .on_toggle_maybe(Some(move |value| {
                                Message::RuleEnabled(index, value)
                            }))
                            .into(),
                        Some(chip.into()),
                        vec![selector],
                        iced::widget::container(widgets::rule_delete_button(Some(
                            Message::Remove(index)
                        )))
                        .center_x(80)
                        .into(),
                    )];
                    if is_path && validate_path(target).is_err() {
                        card = card.push(app_picker::path_readd_hint());
                    }
                    cards.push((
                        widgets::stable_key(&(is_path, target)),
                        card.push(widgets::settings_divider()).into(),
                    ));
                }
                rules_body = rules_body.push(widgets::process_rules_table_with_actions(
                    [t!("by_running_app.power_plan").to_string()],
                    cards,
                    t!("common.no_custom_rules").to_string(),
                    80,
                    true,
                ));
                let preview = match kind {
                    Kind::Focus => PreviewMessage::OpenFocus,
                    Kind::RunningApp => PreviewMessage::Load,
                };
                let heading = widgets::setting_row(
                    "common.rules",
                    widgets::action_button(t!("power_plan_control.preview").to_string())
                        .style(widgets::secondary_button)
                        .on_press_maybe(
                            (!feature.rules.is_empty()).then_some(Message::Preview(preview)),
                        ),
                );
                body = body.push(heading).push(rules_body);
                scrollable(body).height(Fill).into()
            }};
        }
        match kind {
            Kind::Focus => render_rules!(
                by_focus,
                can_add_by_focus_process,
                can_add_by_focus_name,
                "by_focus.enable"
            ),
            Kind::RunningApp => render_rules!(
                by_running_app,
                can_add_by_running_app_process,
                can_add_by_running_app_name,
                "by_running_app.enable"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_path_rules_require_full_drive_or_network_paths() {
        for kind in [Kind::Focus, Kind::RunningApp] {
            let mut settings = SettingsEditor::with_settings(Settings::default());
            settings.select_power_source(crate::config::PowerSourceProfile::OnBattery);
            let mut editor = Editor::default();
            for path in [
                r"C:\Apps\.\worker.exe",
                r"C:\Apps\old\..\worker.exe",
                r"\\?\C:\Apps\worker.exe",
                r"C:\Apps\worker.exe.",
                r"C:\Apps \worker.exe",
                r"C:\Apps\",
            ] {
                editor.update(kind, &mut settings, &[], Message::Path(path.into()));
                editor.update(kind, &mut settings, &[], Message::Add);
                assert!(settings.global().on_battery.is_none());
            }
            for path in [r" C:\Apps\worker.exe ", r" \\server\share\worker.exe "] {
                editor.update(kind, &mut settings, &[], Message::Path(path.into()));
                editor.update(kind, &mut settings, &[], Message::Add);
            }
            let paths: Vec<_> = match kind {
                Kind::Focus => settings
                    .by_focus
                    .rules
                    .iter()
                    .map(|rule| rule.selector.value())
                    .collect(),
                Kind::RunningApp => settings
                    .by_running_app
                    .rules
                    .iter()
                    .map(|rule| rule.selector.value())
                    .collect(),
            };
            assert_eq!(paths, [r"C:\Apps\worker.exe", r"\\server\share\worker.exe"]);
            assert!(settings.global().by_focus.rules.is_empty());
            assert!(settings.global().by_running_app.rules.is_empty());
        }
    }

    #[test]
    fn name_rules_allow_overlapping_paths_and_reject_duplicate_names() {
        let path = r"C:\Apps\worker.exe";
        for kind in [Kind::Focus, Kind::RunningApp] {
            let mut settings = SettingsEditor::with_settings(Settings::default());
            let mut editor = Editor::default();
            for message in [
                Message::Path(path.into()),
                Message::Add,
                Message::MatchBy(MatchBy::ExecutableName),
                Message::Name(path.into()),
                Message::AddName,
                Message::Name("worker.exe".into()),
                Message::AddName,
                Message::Name("WORKER.EXE".into()),
                Message::AddName,
            ] {
                editor.update(kind, &mut settings, &[], message);
            }
            let selectors: Vec<_> = match kind {
                Kind::Focus => {
                    assert!(settings.by_running_app.rules.is_empty());
                    settings
                        .by_focus
                        .rules
                        .iter()
                        .map(|rule| &rule.selector)
                        .collect()
                }
                Kind::RunningApp => {
                    assert!(settings.by_focus.rules.is_empty());
                    settings
                        .by_running_app
                        .rules
                        .iter()
                        .map(|rule| &rule.selector)
                        .collect()
                }
            };
            assert_eq!(selectors.len(), 2);
            assert!(selector_path_matches(selectors[0], path));
            assert_eq!(selectors[1].exact_path(), None);
            assert_eq!(selectors[1].value(), "worker.exe");
        }
    }

    #[test]
    fn process_plan_status_requires_selected_rule_and_confirmed_plan() {
        use crate::{
            control::power_plan::{PowerPlanOwner, PowerPlanStatus},
            rules::DecisionState,
        };
        for state in [DecisionState::ByFocus, DecisionState::ByRunningApp] {
            let mut status = PowerPlanStatus {
                owner: Some(PowerPlanOwner::OrdinaryAutomation),
                decision_state: Some(state),
                current_guid: Some("PLAN".into()),
                target_guid: Some("plan".into()),
                ..Default::default()
            };
            let label = |enabled, selected, status: &PowerPlanStatus| {
                super::super::power_rules::power_rule_status(
                    enabled,
                    true,
                    selected,
                    state,
                    [Some("plan"), None],
                    status,
                )
            };
            assert_eq!(label(true, true, &status), "common.applied");
            assert_eq!(label(false, true, &status), "common.inactive");
            assert_eq!(label(true, false, &status), "common.waiting");
            status.current_guid = Some("other".into());
            assert_eq!(label(true, true, &status), "common.waiting");
        }
    }

    #[test]
    fn rules_stay_with_their_feature_and_duplicates_are_rejected() {
        let mut settings = SettingsEditor::with_settings(Settings::default());
        let mut editor = Editor::default();
        editor.update(Kind::Focus, &mut settings, &[], Message::Enabled(true));
        for _ in 0..2 {
            editor.update(
                Kind::Focus,
                &mut settings,
                &[],
                Message::Path(r"C:\Apps\game.exe".into()),
            );
            editor.update(Kind::Focus, &mut settings, &[], Message::Add);
        }
        assert_eq!(settings.by_focus.rules.len(), 1);
        assert!(settings.by_running_app.rules.is_empty());
        assert_eq!(settings.by_focus.rules[0].power_plan_guid, None);
        editor.update(
            Kind::Focus,
            &mut settings,
            &[],
            Message::Plan(0, Some("selected".into())),
        );
        assert_eq!(
            settings.by_focus.rules[0].power_plan_guid.as_deref(),
            Some("selected")
        );
    }
}
