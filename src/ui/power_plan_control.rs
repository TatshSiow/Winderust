use iced::widget::{column, container, row, text};
use iced::{Element, Fill};
use rust_i18n::t;

use super::{design, widgets};
use crate::{
    control::power_plan::{PowerPlanOwner, PowerPlanStatus},
    power::{PowerPlan, ADAPTIVE_PLAN_NAME},
    rules::DecisionState,
};

#[derive(Debug, Clone)]
pub(super) enum Message {
    Select(String),
    ProcessorPower,
}

#[derive(Debug, Default)]
pub(super) struct Editor {
    pub(super) selected_guid: Option<String>,
    pub(super) pending: Option<AfterSave>,
}

impl Editor {
    pub(super) fn discard_unavailable_plan(&mut self, plans: &[PowerPlan]) -> bool {
        if self.pending.is_some()
            || !self
                .selected_guid
                .as_deref()
                .is_some_and(|guid| !is_selectable(plans, guid))
        {
            return false;
        }
        self.selected_guid = None;
        true
    }
}

pub(super) fn is_selectable(plans: &[PowerPlan], guid: &str) -> bool {
    plans
        .iter()
        .any(|plan| !plan.managed_adaptive && plan.guid.eq_ignore_ascii_case(guid))
}

#[derive(Debug)]
pub(super) enum AfterSave {
    Stay,
    Exit,
}

pub(super) fn view(
    status: &PowerPlanStatus,
    plans: &[PowerPlan],
    plans_loaded: bool,
    loading: bool,
    editor: &Editor,
) -> Element<'static, Message> {
    let current = current_plan_guid(status, plans);
    let managed = managed_by_adaptive(status, plans);
    let selector = manual_switch(plans, plans_loaded, loading, current, managed, editor);
    if managed {
        return widgets::settings_card(selector).into();
    }
    let (detail, failed) = if editor.pending.is_some() {
        (t!("power_plan_control.switching").to_string(), false)
    } else {
        automation_detail(status, plans, current)
    };
    let status = text(detail)
        .width(Fill)
        .size(design::typography::SECONDARY)
        .style(if failed {
            text::danger
        } else {
            text::secondary
        });
    widgets::settings_card(column![selector, status].spacing(design::space::TINY)).into()
}

pub(super) fn current_plan_guid<'a>(
    status: &'a PowerPlanStatus,
    plans: &'a [PowerPlan],
) -> Option<&'a str> {
    plans
        .iter()
        .find(|plan| plan.active)
        .map(|plan| plan.guid.as_str())
        .or(status.current_guid.as_deref())
}

pub(super) fn managed_by_adaptive(status: &PowerPlanStatus, plans: &[PowerPlan]) -> bool {
    status.owner == Some(PowerPlanOwner::AdaptiveEngine)
        && status
            .target_guid
            .as_deref()
            .zip(current_plan_guid(status, plans))
            .is_some_and(|(target, current)| target.eq_ignore_ascii_case(current))
}

pub(super) fn plan_name(plans: &[PowerPlan], guid: &str) -> String {
    plans
        .iter()
        .find(|plan| plan.guid.eq_ignore_ascii_case(guid))
        .map(|plan| plan.name.clone())
        .unwrap_or_else(|| t!("common.unknown").to_string())
}

fn automation_detail(
    status: &PowerPlanStatus,
    plans: &[PowerPlan],
    current: Option<&str>,
) -> (String, bool) {
    let failed = status.owner == Some(PowerPlanOwner::OrdinaryAutomation) && status.apply_failed;
    let mut detail = rule_description(status, current.is_some());
    if let Some(target) = status
        .target_guid
        .as_deref()
        .filter(|_| status.owner == Some(PowerPlanOwner::OrdinaryAutomation))
        .filter(|target| failed || !current.is_some_and(|guid| guid.eq_ignore_ascii_case(target)))
    {
        let plan = plan_name(plans, target);
        let key = if failed {
            "power_plan_control.automation_switch_failed"
        } else {
            "power_plan_control.requested_plan"
        };
        detail.push_str(&format!(" · {}", t!(key, plan = plan)));
    }
    (detail, failed)
}

fn rule_description(status: &PowerPlanStatus, current_known: bool) -> String {
    if status.restoring_previous {
        return t!("power_plan_control.previous_plan_restoration").to_string();
    }
    let mode = match status.decision_state {
        Some(DecisionState::ByFocus) => "nav.by_focus",
        Some(DecisionState::ByRunningApp) => "nav.by_running_app",
        Some(DecisionState::ByCpuLoad) => "nav.by_cpu_load",
        Some(DecisionState::ByTime) => "nav.by_time",
        Some(state @ (DecisionState::ByActivityIdle | DecisionState::ByActivityActive))
            if status.owner == Some(PowerPlanOwner::OrdinaryAutomation) =>
        {
            let selection = if state == DecisionState::ByActivityIdle {
                t!("by_activity.idle_plan")
            } else {
                t!("by_activity.active_plan")
            };
            return format!("{} · {selection}", t!("nav.by_activity"));
        }
        Some(DecisionState::PausedWhilePluggedIn) => "power_plan_control.paused_on_ac",
        Some(DecisionState::Disabled) => "common.disabled",
        Some(_) => "power_plan_control.no_rule_selects_plan",
        None if current_known => "power_plan_control.no_rule_selects_plan",
        None => "common.unknown",
    };
    let mode = t!(mode);
    match status
        .rule_index
        .filter(|_| status.owner == Some(PowerPlanOwner::OrdinaryAutomation))
    {
        Some(index) => format!("{mode} · {}", t!("common.rule", number = index + 1)),
        None => mode.to_string(),
    }
}

fn manual_switch(
    plans: &[PowerPlan],
    plans_loaded: bool,
    loading: bool,
    current: Option<&str>,
    managed: bool,
    editor: &Editor,
) -> Element<'static, Message> {
    let choices: Vec<_> = plans
        .iter()
        .filter(|plan| !plan.managed_adaptive)
        .map(|plan| widgets::Choice(plan.guid.clone(), plan.name.clone()))
        .collect();
    let selected = editor
        .selected_guid
        .as_deref()
        .or(current)
        .and_then(|guid| {
            choices
                .iter()
                .find(|choice| choice.0.eq_ignore_ascii_case(guid))
                .cloned()
        });
    let eligible =
        plans_loaded && !loading && !managed && editor.pending.is_none() && !choices.is_empty();
    let staged = eligible
        && selected
            .as_ref()
            .is_some_and(|plan| !current.is_some_and(|guid| plan.0.eq_ignore_ascii_case(guid)));
    let current_name = if managed {
        Some(ADAPTIVE_PLAN_NAME.to_owned())
    } else {
        current.map(|guid| plan_name(plans, guid))
    };
    let selector: Element<'static, Message> = if eligible {
        let placeholder = if let Some(name) = current_name.as_ref() {
            name.clone()
        } else {
            t!("power_plan_control.choose_plan").to_string()
        };
        widgets::pick_list(choices, selected, |choice| Message::Select(choice.0))
            .placeholder(placeholder)
            .width(design::SELECT_WIDTH)
            .into()
    } else {
        let message = current_name.clone().unwrap_or_else(|| {
            t!(if loading {
                "common.loading"
            } else {
                "common.unavailable"
            })
            .to_string()
        });
        container(
            row![
                text(message)
                    .size(design::typography::BODY)
                    .style(text::secondary)
                    .width(Fill),
                iced::widget::svg(super::assets::chevron_frame(0.0))
                    .width(design::ICON_SIZE)
                    .height(design::ICON_SIZE)
                    .style(|theme: &iced::Theme, _| iced::widget::svg::Style {
                        color: Some(widgets::muted_color(theme)),
                    }),
            ]
            .spacing(design::space::SMALL)
            .align_y(iced::Center),
        )
        .padding(design::SELECT_PADDING)
        .width(design::SELECT_WIDTH)
        .clip(true)
        .style(|theme| {
            let style = widgets::select_field(theme, iced::widget::pick_list::Status::Active);
            iced::widget::container::Style {
                background: Some(style.background),
                border: style.border,
                ..Default::default()
            }
        })
        .into()
    };
    let mut label = column![widgets::setting_title("common.current_power_plan")]
        .spacing(design::space::TIGHT)
        .width(Fill);
    if staged {
        let current = current_name.unwrap_or_else(|| t!("common.unknown").to_string());
        label = label.push(
            text(t!("power_plan_control.switch_plan_help", plan = current).to_string())
                .size(design::typography::SECONDARY)
                .style(text::secondary),
        );
    }
    let mut controls = row![label];
    if managed {
        controls = controls.push(
            widgets::action_button(t!("power_plan_control.processor_power_settings").to_string())
                .style(widgets::tertiary_button)
                .on_press(Message::ProcessorPower),
        );
    }
    controls
        .push(selector)
        .spacing(design::space::MEDIUM)
        .align_y(iced::Center)
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::advanced::{layout, widget::Tree};

    #[test]
    fn current_plan_and_automation_fit_in_a_compact_card() {
        let renderer = iced::Renderer::new(iced::Font::DEFAULT, iced::Pixels(14.0));
        let plans = [PowerPlan {
            guid: "balanced".into(),
            name: "Balanced".into(),
            active: true,
            managed_adaptive: false,
        }];
        let status = PowerPlanStatus {
            current_guid: Some("balanced".into()),
            ..PowerPlanStatus::default()
        };
        let managed_plans = [PowerPlan {
            guid: "adaptive".into(),
            name: ADAPTIVE_PLAN_NAME.into(),
            active: true,
            managed_adaptive: true,
        }];
        let managed_status = PowerPlanStatus {
            owner: Some(PowerPlanOwner::AdaptiveEngine),
            target_guid: Some("adaptive".into()),
            current_guid: Some("adaptive".into()),
            ..PowerPlanStatus::default()
        };
        for (status, plans, plans_loaded, loading, max_height) in [
            (&status, plans.as_slice(), true, false, 100.0),
            (&status, &[], false, true, 100.0),
            (&status, plans.as_slice(), false, true, 100.0),
            (&managed_status, managed_plans.as_slice(), true, false, 72.0),
            (&managed_status, &[], false, true, 72.0),
        ] {
            let mut card = view(status, plans, plans_loaded, loading, &Editor::default());
            let mut tree = Tree::new(&card);
            let bounds = card
                .as_widget_mut()
                .layout(
                    &mut tree,
                    &renderer,
                    &layout::Limits::new(iced::Size::ZERO, iced::Size::new(740.0, 620.0)),
                )
                .bounds();
            assert!(bounds.height <= max_height, "card bounds {bounds:?}");
        }
    }

    #[test]
    fn unknown_plan_guid_is_not_shown_as_its_name() {
        assert_eq!(plan_name(&[], "unknown-plan-guid"), t!("common.unknown"));
    }

    #[test]
    fn a_fresh_catalog_clears_an_unavailable_draft_after_the_switch_finishes() {
        let mut editor = Editor {
            selected_guid: Some("Balanced".into()),
            pending: Some(AfterSave::Stay),
        };
        assert!(!editor.discard_unavailable_plan(&[]));
        editor.pending = None;

        let plans = [PowerPlan {
            guid: "balanced".into(),
            name: "Balanced".into(),
            active: false,
            managed_adaptive: false,
        }];
        assert!(!editor.discard_unavailable_plan(&plans));
        assert!(editor.discard_unavailable_plan(&[]));
        assert!(editor.selected_guid.is_none());
    }

    #[test]
    fn adaptive_lock_requires_tracked_ownership_and_matching_readback() {
        let mut status = PowerPlanStatus {
            owner: Some(PowerPlanOwner::AdaptiveEngine),
            target_guid: Some("ADAPTIVE".into()),
            current_guid: Some("adaptive".into()),
            ..PowerPlanStatus::default()
        };
        let mut active_plan = PowerPlan {
            guid: "adaptive".into(),
            name: "Winderust Adaptive".into(),
            active: true,
            managed_adaptive: true,
        };
        assert!(managed_by_adaptive(&status, &[]));
        assert!(managed_by_adaptive(
            &status,
            std::slice::from_ref(&active_plan)
        ));
        status.owner = None;
        assert!(!managed_by_adaptive(
            &status,
            std::slice::from_ref(&active_plan)
        ));
        status.owner = Some(PowerPlanOwner::AdaptiveEngine);
        active_plan.guid = "other".into();
        active_plan.managed_adaptive = false;
        assert!(!managed_by_adaptive(&status, &[active_plan]));
    }
}
