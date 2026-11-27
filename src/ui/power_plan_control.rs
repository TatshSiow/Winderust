use iced::widget::{column, container, responsive, row, text};
use iced::{Element, Fill};
use rust_i18n::t;
use std::borrow::Cow;

use super::{design, widgets};
use crate::{
    application::SettingsEditor,
    config::{CHECK_INTERVAL_MAX_MS, CHECK_INTERVAL_MIN_MS},
    control::power_plan::{AdaptiveProcessorPowerError, PowerPlanOwner, PowerPlanStatus},
    features::power_plan_control::ByCpuLoadMatch,
    power::{PowerPlan, ADAPTIVE_PLAN_NAME},
    rules::DecisionState,
};

#[derive(Debug, Clone)]
pub(super) enum Message {
    Select(String),
    FallbackPollingInterval(String),
    AdaptiveProcessorPower,
}

#[derive(Debug, Default)]
pub(super) struct Editor {
    pub(super) pending: bool,
    fallback_polling_interval: Option<String>,
}

impl Editor {
    pub(super) fn edit_fallback_polling_interval(
        &mut self,
        settings: &mut SettingsEditor,
        value: String,
    ) {
        if let Some(interval) = parse_fallback_polling_interval(&value) {
            settings.edit_global(|settings| settings.general.check_interval_ms = interval);
        }
        self.fallback_polling_interval = Some(value);
    }

    pub(super) fn fallback_polling_interval_valid(&self) -> bool {
        self.fallback_polling_interval
            .as_deref()
            .is_none_or(|value| parse_fallback_polling_interval(value).is_some())
    }

    pub(super) fn reset_fallback_polling_interval_draft(&mut self) {
        self.fallback_polling_interval = None;
    }

    pub(super) fn fallback_polling_interval_field(&self, interval: u64) -> Element<'_, Message> {
        widgets::slider_setting(
            "power_plan_control.fallback_polling_interval",
            "ms",
            move || {
                widgets::slider_stepper(
                    interval,
                    self.fallback_polling_interval.as_deref(),
                    CHECK_INTERVAL_MIN_MS as u32..=CHECK_INTERVAL_MAX_MS as u32,
                    250,
                    Message::FallbackPollingInterval,
                )
            },
        )
    }
}

fn parse_fallback_polling_interval(value: &str) -> Option<u64> {
    value
        .parse::<u64>()
        .ok()
        .filter(|value| (CHECK_INTERVAL_MIN_MS..=CHECK_INTERVAL_MAX_MS).contains(value))
}

pub(super) fn is_selectable(plans: &[PowerPlan], guid: &str) -> bool {
    plans
        .iter()
        .any(|plan| !plan.managed_adaptive && plan.guid.eq_ignore_ascii_case(guid))
}

pub(super) fn view(
    status: &PowerPlanStatus,
    plans: &[PowerPlan],
    plans_loaded: bool,
    loading: bool,
    automation_enabled: bool,
    editor: &Editor,
) -> Element<'static, Message> {
    if managed_by_adaptive_processor_power(status, plans) {
        return widgets::settings_card(managed_switch()).into();
    }
    let current = current_plan_guid(status, plans);
    let can_select = plans_loaded
        && !loading
        && !editor.pending
        && plans.iter().any(|plan| !plan.managed_adaptive);
    let help = if editor.pending {
        Some("power_plan_control.switching")
    } else {
        can_select.then_some("power_plan_control.switch_plan_help")
    };
    let selector = manual_switch(plans, can_select, loading, current, help);
    let (detail, failed) = if status.active_plan_read_error.is_some() {
        (
            t!("power_plan_control.active_plan_read_failed").into_owned(),
            true,
        )
    } else {
        automation_detail(status, plans, current, automation_enabled)
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

pub(super) fn status_view<M: 'static>(
    status: &PowerPlanStatus,
    plans: &[PowerPlan],
    automation_enabled: bool,
) -> Element<'static, M> {
    let current = current_plan_guid(status, plans);
    let (automation, failed) = status_automation_detail(status, plans, current, automation_enabled);
    let mut summary = column![
        widgets::setting_title("common.current_power_plan"),
        text(current_plan_name(status, plans)).wrapping(text::Wrapping::WordOrGlyph),
        widgets::setting_title("power_plan_control.automation"),
        text(automation)
            .style(if failed { text::danger } else { text::default })
            .wrapping(text::Wrapping::WordOrGlyph),
    ]
    .spacing(design::space::SMALL);
    if let Some(error) = status
        .active_plan_read_error
        .as_deref()
        .map(Cow::Borrowed)
        .or_else(|| {
            status
                .adaptive_processor_power_error
                .as_ref()
                .map(adaptive_error_message)
        })
    {
        summary = summary
            .push(widgets::setting_title("sidebar_status.current_error"))
            .push(text(error.into_owned()).wrapping(text::Wrapping::WordOrGlyph));
    }
    let mut body = column![widgets::settings_card(summary)].spacing(widgets::CARD_GAP);
    if !status.competing_modes.is_empty() {
        let mut matches = column![widgets::setting_title("power_plan_control.other_matches")]
            .spacing(design::space::MEDIUM);
        for candidate in &status.competing_modes {
            let Some(mode) = plan_mode_label(candidate.state) else {
                continue;
            };
            let label = indexed_mode_label(mode, candidate.rule_index);
            let plan = plan_name(plans, &candidate.target_guid);
            let plan = if let Some(reason) = cpu_load_match_label(candidate.cpu_load_match) {
                format!("{reason} · {plan}")
            } else {
                plan
            };
            matches = matches.push(
                column![
                    text(label).wrapping(text::Wrapping::WordOrGlyph),
                    text(plan)
                        .style(text::secondary)
                        .wrapping(text::Wrapping::WordOrGlyph),
                ]
                .spacing(design::space::TIGHT)
                .width(Fill),
            );
        }
        body = body.push(widgets::settings_card(matches));
    }
    super::scrolling::scrollable(body).height(Fill).into()
}

pub(super) fn adaptive_error_message(error: &AdaptiveProcessorPowerError) -> Cow<'_, str> {
    match error {
        AdaptiveProcessorPowerError::Operation(detail) => Cow::Borrowed(detail),
        AdaptiveProcessorPowerError::CleanupRetry(detail) => {
            t!(
                "adaptive_processor_power.cleanup_retry_failed",
                detail = detail
            )
        }
    }
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

pub(super) fn managed_by_adaptive_processor_power(
    status: &PowerPlanStatus,
    plans: &[PowerPlan],
) -> bool {
    status.owner == Some(PowerPlanOwner::AdaptiveProcessorPower)
        && status
            .target_guid
            .as_deref()
            .zip(current_plan_guid(status, plans))
            .is_some_and(|(target, current)| target.eq_ignore_ascii_case(current))
}

pub(super) fn current_plan_name(status: &PowerPlanStatus, plans: &[PowerPlan]) -> String {
    if managed_by_adaptive_processor_power(status, plans) {
        return ADAPTIVE_PLAN_NAME.to_owned();
    }
    current_plan_guid(status, plans).map_or_else(
        || t!("common.unknown").to_string(),
        |guid| plan_name(plans, guid),
    )
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
    automation_enabled: bool,
) -> (String, bool) {
    if !status.restoring_previous
        && (!automation_enabled || status.decision_state == Some(DecisionState::Disabled))
    {
        return (
            t!("power_plan_control.automation_disabled").into_owned(),
            false,
        );
    }
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

fn status_automation_detail(
    status: &PowerPlanStatus,
    plans: &[PowerPlan],
    current: Option<&str>,
    automation_enabled: bool,
) -> (String, bool) {
    if managed_by_adaptive_processor_power(status, plans) {
        return (t!("nav.adaptive_processor_power").to_string(), false);
    }
    let (mut detail, failed) = automation_detail(status, plans, current, automation_enabled);
    let cpu_load_match = status.cpu_load_match.filter(|_| automation_enabled);
    if let Some(reason) = cpu_load_match_label(cpu_load_match) {
        detail.push_str(&format!(" · {reason}"));
    }
    (detail, failed)
}

fn cpu_load_match_label(cpu_load_match: Option<ByCpuLoadMatch>) -> Option<String> {
    let key = match cpu_load_match? {
        ByCpuLoadMatch::Condition => return None,
        ByCpuLoadMatch::MinimumHold => "by_cpu_load.minimum_hold",
        ByCpuLoadMatch::ReleaseThreshold => "by_cpu_load.within_release_thresholds",
        ByCpuLoadMatch::Else => "by_cpu_load.else_plan",
    };
    Some(t!(key).to_string())
}

fn rule_description(status: &PowerPlanStatus, current_known: bool) -> String {
    if status.restoring_previous {
        return t!("power_plan_control.previous_plan_restoration").to_string();
    }
    let ordinary = status.owner == Some(PowerPlanOwner::OrdinaryAutomation);
    let mode = status.decision_state.and_then(|state| match state {
        DecisionState::ByActivityIdle | DecisionState::ByActivityActive if !ordinary => None,
        _ => plan_mode_label(state),
    });
    if let Some(mode) = mode {
        return indexed_mode_label(mode, status.rule_index.filter(|_| ordinary));
    }
    t!(match status.decision_state {
        Some(DecisionState::PausedWhilePluggedIn) => "power_plan_control.paused_on_ac",
        Some(DecisionState::WaitingForObservation) => {
            "power_plan_control.waiting_for_observation"
        }
        Some(_) => "power_plan_control.no_rule_selects_plan",
        None if current_known => "power_plan_control.no_rule_selects_plan",
        None => "common.unknown",
    })
    .to_string()
}

fn plan_mode_label(state: DecisionState) -> Option<String> {
    let key = match state {
        DecisionState::ByFocus => "nav.by_focus",
        DecisionState::ByRunningApp => "nav.by_running_app",
        DecisionState::ByCpuLoad => "nav.by_cpu_load",
        DecisionState::ByTime => "nav.by_time",
        DecisionState::ByActivityIdle | DecisionState::ByActivityActive => {
            let selection = if state == DecisionState::ByActivityIdle {
                t!("by_activity.idle_plan")
            } else {
                t!("by_activity.active_plan")
            };
            return Some(format!("{} · {selection}", t!("nav.by_activity")));
        }
        _ => return None,
    };
    Some(t!(key).to_string())
}

fn indexed_mode_label(mode: String, rule_index: Option<usize>) -> String {
    match rule_index {
        Some(index) => format!("{mode} · {}", t!("common.rule", number = index + 1)),
        None => mode,
    }
}

fn manual_switch(
    plans: &[PowerPlan],
    can_select: bool,
    loading: bool,
    current: Option<&str>,
    help: Option<&'static str>,
) -> Element<'static, Message> {
    let current_name = current.map(|guid| plan_name(plans, guid));
    let selector: Element<'static, Message> = if can_select {
        let choices: Vec<_> = plans
            .iter()
            .filter(|plan| !plan.managed_adaptive)
            .map(|plan| widgets::Choice(plan.guid.clone(), plan.name.clone()))
            .collect();
        let selected = current.and_then(|guid| {
            choices
                .iter()
                .find(|choice| choice.0.eq_ignore_ascii_case(guid))
                .cloned()
        });
        let placeholder =
            current_name.unwrap_or_else(|| t!("power_plan_control.choose_plan").to_string());
        widgets::pick_list(choices, selected, |choice| Message::Select(choice.0))
            .placeholder(placeholder)
            .width(design::SELECT_WIDTH)
            .into()
    } else {
        let message = current_name.unwrap_or_else(|| {
            t!(if loading {
                "common.loading"
            } else {
                "common.unavailable"
            })
            .to_string()
        });
        disabled_plan_selector(message)
    };
    match help {
        Some(key) => widgets::setting_row_with_title(
            widgets::setting_title_with_help("common.current_power_plan", "", key),
            selector,
        ),
        None => widgets::setting_row("common.current_power_plan", selector),
    }
    .into()
}

fn managed_switch() -> Element<'static, Message> {
    const SINGLE_ROW_MIN_WIDTH: f32 = 560.0;

    responsive(|size| {
        let selector = disabled_plan_selector(ADAPTIVE_PLAN_NAME.to_owned());
        let shortcut = widgets::action_button(t!("nav.adaptive_processor_power").to_string())
            .style(widgets::tertiary_button)
            .on_press(Message::AdaptiveProcessorPower);
        if size.width < SINGLE_ROW_MIN_WIDTH {
            column![
                widgets::setting_row("common.current_power_plan", selector),
                shortcut,
            ]
            .into()
        } else {
            widgets::setting_row(
                "common.current_power_plan",
                row![shortcut, selector]
                    .spacing(design::space::MEDIUM)
                    .align_y(iced::Center),
            )
            .into()
        }
    })
    .height(iced::Shrink)
    .into()
}

fn disabled_plan_selector(message: String) -> Element<'static, Message> {
    container(
        row![
            text(message)
                .size(design::typography::BODY)
                .style(text::secondary)
                .width(Fill),
            super::assets::icon(
                "icons/chevron-right.svg",
                design::ICON_SIZE,
                widgets::muted_color,
            ),
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::advanced::{layout, widget::Tree};

    #[test]
    fn fallback_polling_interval_draft_is_shared_and_discardable() {
        use crate::config::{PowerSourceProfile, Settings};

        for existing_battery in [false, true] {
            let mut initial = Settings::default();
            initial.by_activity.idle_timeout_seconds = 60;
            if existing_battery {
                initial
                    .battery_profile_mut()
                    .by_activity
                    .idle_timeout_seconds = 120;
            }
            let mut settings = SettingsEditor::with_settings(initial.clone());
            settings.select_power_source(PowerSourceProfile::OnBattery);
            let mut editor = Editor::default();
            for interval in [CHECK_INTERVAL_MIN_MS, CHECK_INTERVAL_MAX_MS, 1234] {
                editor.edit_fallback_polling_interval(&mut settings, interval.to_string());
                assert!(editor.fallback_polling_interval_valid());
                assert_eq!(settings.global().general.check_interval_ms, interval);
                assert_eq!(settings.general.check_interval_ms, interval);
            }
            assert_eq!(settings.global().by_activity, initial.by_activity);
            assert_eq!(
                settings.by_activity.idle_timeout_seconds,
                if existing_battery { 120 } else { 60 }
            );
            for invalid in ["", "249", "60001", "invalid", "18446744073709551615"] {
                editor.edit_fallback_polling_interval(&mut settings, invalid.into());
                assert!(!editor.fallback_polling_interval_valid());
                assert_eq!(settings.global().general.check_interval_ms, 1234);
            }
            settings.cancel();
            editor.reset_fallback_polling_interval_draft();
            assert!(editor.fallback_polling_interval_valid());
            assert_eq!(settings.global(), &initial);
        }
    }

    #[test]
    fn current_plan_summary_explains_when_automation_is_inactive() {
        for (enabled, restoring_previous, decision_state, key) in [
            (false, false, None, "power_plan_control.automation_disabled"),
            (
                false,
                true,
                None,
                "power_plan_control.previous_plan_restoration",
            ),
            (
                true,
                false,
                Some(DecisionState::Disabled),
                "power_plan_control.automation_disabled",
            ),
            (
                true,
                false,
                Some(DecisionState::PausedWhilePluggedIn),
                "power_plan_control.paused_on_ac",
            ),
            (true, false, None, "power_plan_control.no_rule_selects_plan"),
        ] {
            let status = PowerPlanStatus {
                decision_state,
                restoring_previous,
                ..PowerPlanStatus::default()
            };
            assert_eq!(
                automation_detail(&status, &[], Some("balanced"), enabled),
                (t!(key).into_owned(), false),
            );
        }
    }

    #[test]
    fn status_explains_cpu_load_selection_beyond_the_main_card_summary() {
        let mut status = PowerPlanStatus {
            owner: Some(PowerPlanOwner::OrdinaryAutomation),
            decision_state: Some(DecisionState::ByCpuLoad),
            rule_index: Some(0),
            ..PowerPlanStatus::default()
        };
        let (summary, _) = automation_detail(&status, &[], None, true);
        for (selection, key) in [
            (ByCpuLoadMatch::Condition, None),
            (
                ByCpuLoadMatch::MinimumHold,
                Some("by_cpu_load.minimum_hold"),
            ),
            (
                ByCpuLoadMatch::ReleaseThreshold,
                Some("by_cpu_load.within_release_thresholds"),
            ),
            (ByCpuLoadMatch::Else, Some("by_cpu_load.else_plan")),
        ] {
            status.cpu_load_match = Some(selection);
            assert_eq!(
                automation_detail(&status, &[], None, true),
                (summary.clone(), false)
            );
            let expected =
                key.map_or_else(|| summary.clone(), |key| format!("{summary} · {}", t!(key)));
            assert_eq!(
                status_automation_detail(&status, &[], None, true),
                (expected, false)
            );
        }
    }

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
            owner: Some(PowerPlanOwner::AdaptiveProcessorPower),
            target_guid: Some("adaptive".into()),
            current_guid: Some("adaptive".into()),
            ..PowerPlanStatus::default()
        };
        for (status, plans, plans_loaded, loading, width, max_height) in [
            (&status, plans.as_slice(), true, false, 740.0, 100.0),
            (&status, &[], false, true, 740.0, 100.0),
            (&status, plans.as_slice(), false, true, 740.0, 100.0),
            (
                &managed_status,
                managed_plans.as_slice(),
                true,
                false,
                740.0,
                72.0,
            ),
            (&managed_status, &[], false, true, 740.0, 72.0),
            (
                &managed_status,
                managed_plans.as_slice(),
                true,
                false,
                450.0,
                105.0,
            ),
        ] {
            let mut card = view(
                status,
                plans,
                plans_loaded,
                loading,
                true,
                &Editor::default(),
            );
            let mut tree = Tree::new(&card);
            let bounds = card
                .as_widget_mut()
                .layout(
                    &mut tree,
                    &renderer,
                    &layout::Limits::new(iced::Size::ZERO, iced::Size::new(width, 620.0)),
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
    fn adaptive_lock_requires_tracked_ownership_and_matching_readback() {
        let mut status = PowerPlanStatus {
            owner: Some(PowerPlanOwner::AdaptiveProcessorPower),
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
        assert!(managed_by_adaptive_processor_power(&status, &[]));
        assert!(managed_by_adaptive_processor_power(
            &status,
            std::slice::from_ref(&active_plan)
        ));
        status.owner = None;
        assert!(!managed_by_adaptive_processor_power(
            &status,
            std::slice::from_ref(&active_plan)
        ));
        status.owner = Some(PowerPlanOwner::AdaptiveProcessorPower);
        active_plan.guid = "other".into();
        active_plan.managed_adaptive = false;
        assert!(!managed_by_adaptive_processor_power(
            &status,
            &[active_plan]
        ));
    }
}
