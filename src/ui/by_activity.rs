use super::design;
use super::widgets;
use crate::ui::scrolling::scrollable;
use iced::widget::column;
use iced::{Element, Fill};

use crate::config::Settings;
use crate::power::PowerPlan;

const IDLE_TIMEOUT_RANGE: std::ops::RangeInclusive<u32> = 1..=3600;

fn parse_idle_timeout(value: &str) -> Option<u64> {
    value
        .parse::<u32>()
        .ok()
        .filter(|value| IDLE_TIMEOUT_RANGE.contains(value))
        .map(u64::from)
}

#[derive(Debug, Clone)]
pub(super) enum Message {
    Enabled(bool),
    Keyboard(bool),
    Mouse(bool),
    Controller(bool),
    IdleTimeout(String),
    IdlePlan(Option<String>),
    ActivePlan(Option<String>),
}

pub(super) fn update(settings: &mut crate::application::SettingsEditor, message: Message) {
    let input_changed = matches!(
        message,
        Message::Keyboard(_) | Message::Mouse(_) | Message::Controller(_)
    );
    let activity = &mut settings.by_activity;
    match message {
        Message::Enabled(value) => activity.enabled = value,
        Message::Keyboard(value) => {
            if value || activity.input_detection.mouse || activity.input_detection.controller {
                activity.input_detection.keyboard = value;
            }
        }
        Message::Mouse(value) => {
            if value || activity.input_detection.keyboard || activity.input_detection.controller {
                activity.input_detection.mouse = value;
            }
        }
        Message::Controller(value) => {
            if value || activity.input_detection.keyboard || activity.input_detection.mouse {
                activity.input_detection.controller = value;
            }
        }
        Message::IdleTimeout(value) => {
            if let Some(timeout) = parse_idle_timeout(&value) {
                activity.idle_timeout_seconds = timeout;
            }
        }
        Message::IdlePlan(guid) => activity.power_plans.power_save_guid = guid,
        Message::ActivePlan(guid) => activity.power_plans.performance_guid = guid,
    }
    if input_changed {
        settings.by_activity.switch_to_performance_on_resume =
            settings.by_activity.input_detection.any_enabled();
    }
}

#[derive(Default)]
pub(super) struct Inputs {
    idle: Option<String>,
}

impl Inputs {
    pub(super) fn edit(&mut self, message: &Message) {
        if let Message::IdleTimeout(value) = message {
            self.idle = Some(value.clone());
        }
    }

    pub(super) fn valid(&self) -> bool {
        self.idle
            .as_deref()
            .is_none_or(|value| parse_idle_timeout(value).is_some())
    }
}

pub(super) fn view<'a>(
    settings: &'a Settings,
    plans: &[PowerPlan],
    inputs: &Inputs,
) -> Element<'a, Message> {
    let activity = &settings.by_activity;
    let plan_picker = |guid: &Option<String>, action: fn(Option<String>) -> Message| {
        super::widgets::plan(guid.as_deref(), plans, action)
    };
    let flag = |key: &str, value, action: fn(bool) -> Message| {
        widgets::setting_row(key, widgets::switch(value, Some(action)))
    };
    let power_plans = widgets::settings_rows([
        widgets::setting_row(
            "by_activity.idle_plan",
            iced::widget::container(plan_picker(
                &activity.power_plans.power_save_guid,
                Message::IdlePlan,
            ))
            .width(design::SELECT_WIDTH),
        )
        .into(),
        widgets::setting_row(
            "by_activity.active_plan",
            iced::widget::container(plan_picker(
                &activity.power_plans.performance_guid,
                Message::ActivePlan,
            ))
            .width(design::SELECT_WIDTH),
        )
        .into(),
    ]);
    scrollable(
        column![
            widgets::toggle_card(
                "by_activity.enable",
                activity.enabled,
                Some(Message::Enabled)
            ),
            widgets::heading(
                rust_i18n::t!("by_activity.power_plans").to_string(),
                design::typography::BODY
            ),
            widgets::settings_card(power_plans),
            widgets::heading(
                rust_i18n::t!("by_activity.input_detection").to_string(),
                design::typography::BODY
            ),
            widgets::settings_card(widgets::settings_rows([
                flag(
                    "by_activity.keyboard_input",
                    activity.input_detection.keyboard,
                    Message::Keyboard
                )
                .into(),
                flag(
                    "by_activity.mouse_input",
                    activity.input_detection.mouse,
                    Message::Mouse
                )
                .into(),
                flag(
                    "by_activity.controller_input",
                    activity.input_detection.controller,
                    Message::Controller
                )
                .into(),
            ])),
            widgets::settings_card(
                column![
                    widgets::setting_title_with_unit("by_activity.idle_timeout", "s"),
                    widgets::slider_stepper(
                        activity.idle_timeout_seconds,
                        inputs.idle.as_deref(),
                        IDLE_TIMEOUT_RANGE,
                        1,
                        Message::IdleTimeout,
                    ),
                ]
                .spacing(design::space::SMALL),
            ),
        ]
        .spacing(widgets::CARD_GAP),
    )
    .height(Fill)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn idle_timeout_draft_applies_to_the_selected_power_profile() {
        let mut initial = Settings::default();
        initial.by_activity.idle_timeout_seconds = 60;
        initial
            .battery_profile_mut()
            .by_activity
            .idle_timeout_seconds = 120;
        let mut settings = crate::application::SettingsEditor::with_settings(initial.clone());
        settings.select_power_source(crate::config::PowerSourceProfile::OnBattery);
        let mut inputs = Inputs::default();
        for (draft, valid) in [("0", false), ("180", true)] {
            let message = Message::IdleTimeout(draft.into());
            inputs.edit(&message);
            update(&mut settings, message);
            assert_eq!(inputs.valid(), valid);
            assert_eq!(
                settings.by_activity.idle_timeout_seconds,
                if valid { 180 } else { 120 }
            );
            assert_eq!(settings.global().by_activity.idle_timeout_seconds, 60);
        }
        settings.cancel();
        assert_eq!(settings.global(), &initial);
    }

    #[test]
    fn activity_keeps_a_wake_source_and_owns_its_plans() {
        let mut settings = crate::application::SettingsEditor::with_settings(Settings::default());
        update(&mut settings, Message::Keyboard(false));
        update(&mut settings, Message::Mouse(false));
        update(&mut settings, Message::Controller(false));
        assert!(settings.by_activity.input_detection.controller);
        assert!(settings.by_activity.switch_to_performance_on_resume);
        update(&mut settings, Message::IdlePlan(Some("idle".into())));
        update(&mut settings, Message::ActivePlan(Some("active".into())));
        assert_eq!(
            settings.by_activity.power_plans.power_save_guid.as_deref(),
            Some("idle")
        );
        assert_eq!(
            settings.by_activity.power_plans.performance_guid.as_deref(),
            Some("active")
        );
    }
}
