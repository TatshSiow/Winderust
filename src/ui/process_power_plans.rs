use super::widgets::{self, Ui};
use crate::{config::Settings, power::PowerPlan, ui::process_rules::*};
use rust_i18n::t;
use windui::prelude::Element;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Foreground,
    RunningApp,
}

#[derive(Debug, Clone)]
pub(super) enum Message {
    Enabled(bool),
    Path(String),
    Browse,
    Add,
    RuleEnabled(usize, bool),
    Plan(usize, Option<String>),
    Remove(usize),
}

#[derive(Default)]
pub(super) struct Editor {
    path: String,
}

impl Editor {
    pub(super) fn update(
        &mut self,
        kind: Kind,
        settings: &mut Settings,
        plans: &[PowerPlan],
        message: Message,
    ) {
        // Both pages have the same rule editor, but keep their distinct typed settings and policies.
        macro_rules! update_rules {
            ($field:ident, $can_add:ident, $new_rule:ident) => {{
                let settings = &mut settings.$field;
                match message {
                    Message::Enabled(value) => settings.enabled = value,
                    Message::Path(path) => self.path = path,
                    Message::Browse => {}
                    Message::Add => {
                        if $can_add(settings, &self.path) {
                            settings.rules.push($new_rule(
                                &self.path,
                                plans
                                    .iter()
                                    .find(|plan| plan.active)
                                    .map(|plan| plan.guid.clone()),
                            ));
                            self.path.clear();
                        }
                    }
                    Message::RuleEnabled(index, value) => {
                        if let Some(rule) = settings.rules.get_mut(index) {
                            rule.enabled = value;
                        }
                    }
                    Message::Plan(index, value) => {
                        if let Some(rule) = settings.rules.get_mut(index) {
                            rule.power_plan_guid = value;
                        }
                    }
                    Message::Remove(index) => {
                        if index < settings.rules.len() {
                            settings.rules.remove(index);
                        }
                    }
                }
            }};
        }
        match kind {
            Kind::Foreground => update_rules!(
                by_foreground,
                can_add_foreground_process,
                new_foreground_rule
            ),
            Kind::RunningApp => update_rules!(
                by_running_app,
                can_add_by_running_app_process,
                new_by_running_app_rule
            ),
        }
    }

    pub(super) fn view(
        &self,
        kind: Kind,
        settings: &Settings,
        plans: &[PowerPlan],
        candidates: &[super::app_picker::Candidate],
        status: &crate::control::power_plan::PowerPlanStatus,
        ui: Ui<Message>,
    ) -> Element {
        let state = match kind {
            Kind::Foreground => crate::rules::DecisionState::ByForeground,
            Kind::RunningApp => crate::rules::DecisionState::ByRunningApp,
        };
        macro_rules! render_rules {
            ($field:ident, $can_add:ident, $enable:literal) => {{
                let feature = &settings.$field;
                let candidate_settings = feature.clone();
                let mut body = vec![
                    widgets::setting_row(
                        t!($enable).to_string(),
                        ui.toggle(feature.enabled, Message::Enabled),
                    ),
                    widgets::heading(t!("common.rules").to_string()),
                    super::app_picker::view(
                        &self.path,
                        candidates,
                        true,
                        Message::Path,
                        Message::Browse,
                        Some(Message::Add),
                        move |candidate| {
                            $can_add(
                                &candidate_settings,
                                &candidate.info.image_path.to_string_lossy(),
                            )
                            .then_some(true)
                        },
                        ui.clone(),
                    ),
                ];
                for (index, rule) in feature.rules.iter().enumerate() {
                    let key = super::power_rules::power_rule_status(
                        settings.general.enabled && feature.enabled && rule.enabled,
                        !rule.executable_path.trim().is_empty(),
                        status.rule_index == Some(index),
                        state,
                        [rule.power_plan_guid.as_deref(), None],
                        status,
                    );
                    body.push(widgets::card(
                        rule.executable_path.clone(),
                        [
                            ui.checkbox(
                                t!("common.enable").to_string(),
                                rule.enabled,
                                move |value| Message::RuleEnabled(index, value),
                            ),
                            widgets::label(t!(key).to_string()),
                            widgets::setting_row(
                                t!("by_running_app.power_plan").to_string(),
                                ui.plan(rule.power_plan_guid.clone(), plans, move |value| {
                                    Message::Plan(index, value)
                                }),
                            ),
                            ui.button(t!("common.remove").to_string(), Message::Remove(index)),
                        ],
                    ));
                }
                if feature.rules.is_empty() {
                    body.push(widgets::label(t!("common.no_custom_rules").to_string()));
                }
                widgets::page(body)
            }};
        }
        match kind {
            Kind::Foreground => render_rules!(
                by_foreground,
                can_add_foreground_process,
                "by_foreground.enable"
            ),
            Kind::RunningApp => render_rules!(
                by_running_app,
                can_add_by_running_app_process,
                "by_running_app.enable"
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn process_plan_status_requires_selected_rule_and_confirmed_plan() {
        use crate::{
            control::power_plan::{PowerPlanOwner, PowerPlanStatus},
            rules::DecisionState,
        };
        for state in [DecisionState::ByForeground, DecisionState::ByRunningApp] {
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
        let mut settings = Settings::default();
        let mut editor = Editor::default();
        editor.update(Kind::Foreground, &mut settings, &[], Message::Enabled(true));
        for _ in 0..2 {
            editor.update(
                Kind::Foreground,
                &mut settings,
                &[],
                Message::Path(r"C:\Apps\game.exe".into()),
            );
            editor.update(Kind::Foreground, &mut settings, &[], Message::Add);
        }
        assert_eq!(settings.by_foreground.rules.len(), 1);
        assert!(settings.by_running_app.rules.is_empty());
        assert_eq!(settings.by_foreground.rules[0].power_plan_guid, None);
        editor.update(
            Kind::Foreground,
            &mut settings,
            &[],
            Message::Plan(0, Some("selected".into())),
        );
        assert_eq!(
            settings.by_foreground.rules[0].power_plan_guid.as_deref(),
            Some("selected")
        );
    }
}
