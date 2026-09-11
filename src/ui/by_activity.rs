use super::design;
use super::widgets::slider;
use iced::widget::{column, row, scrollable};
use iced::{Element, Fill};

use crate::config::{Settings, CHECK_INTERVAL_MAX_MS, CHECK_INTERVAL_MIN_MS};
use crate::power::PowerPlan;

#[derive(Debug, Clone)]
pub(super) enum Message {
    Enabled(bool),
    Keyboard(bool),
    Mouse(bool),
    Controller(bool),
    IdleTimeout(String),
    CheckInterval(String),
    IdlePlan(Option<String>),
    ActivePlan(Option<String>),
}

pub(super) fn update(settings: &mut Settings, message: Message) {
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
            if let Ok(value) = value.parse::<u64>() {
                if (1..=3600).contains(&value) {
                    activity.idle_timeout_seconds = value;
                }
            }
        }
        Message::CheckInterval(value) => {
            if let Ok(value) = value.parse::<u64>() {
                if (CHECK_INTERVAL_MIN_MS..=CHECK_INTERVAL_MAX_MS).contains(&value) {
                    settings.general.check_interval_ms = value;
                }
            }
        }
        Message::IdlePlan(guid) => activity.power_plans.power_save_guid = guid,
        Message::ActivePlan(guid) => activity.power_plans.performance_guid = guid,
    }
    if input_changed {
        activity.switch_to_performance_on_resume = activity.input_detection.any_enabled();
    }
}

#[derive(Default)]
pub(super) struct Inputs {
    idle: Option<String>,
    interval: Option<String>,
}

impl Inputs {
    pub(super) fn edit(&mut self, message: &Message) {
        match message {
            Message::IdleTimeout(value) => self.idle = Some(value.clone()),
            Message::CheckInterval(value) => self.interval = Some(value.clone()),
            _ => {}
        }
    }

    pub(super) fn valid(&self) -> bool {
        self.idle.as_ref().is_none_or(|value| {
            value
                .parse::<u64>()
                .is_ok_and(|value| (1..=3600).contains(&value))
        }) && self.interval.as_ref().is_none_or(|value| {
            value
                .parse::<u64>()
                .is_ok_and(|value| (CHECK_INTERVAL_MIN_MS..=CHECK_INTERVAL_MAX_MS).contains(&value))
        })
    }
}

pub(super) fn view<'a>(
    settings: &'a Settings,
    plans: &[PowerPlan],
    inputs: &Inputs,
) -> Element<'a, Message> {
    let activity = &settings.by_activity;
    let plan_picker = |guid: &Option<String>, action: fn(Option<String>) -> Message| {
        super::widgets::plan(guid.clone(), plans, action)
    };
    let flag = |key: &str, value, action: fn(bool) -> Message, enabled: bool| {
        super::widgets::settings_card(super::widgets::setting_row(
            key,
            super::widgets::switch(value, enabled.then_some(action)),
        ))
    };
    let number = |key: &str,
                  value: String,
                  range: std::ops::RangeInclusive<u64>,
                  step: u64,
                  unit: &str,
                  action: fn(String) -> Message| {
        super::widgets::settings_card(super::widgets::setting_row_with_unit(
            key,
            unit,
            row![
                slider(
                    *range.start() as u32..=*range.end() as u32,
                    value.parse::<u32>().unwrap_or(*range.start() as u32),
                    move |v| action(v.to_string())
                )
                .step(step as u32)
                .width(180),
                super::widgets::stepper(&value, range, step, Some(action))
            ]
            .spacing(design::space::SMALL)
            .align_y(iced::Center),
        ))
    };
    scrollable(
        column![
            flag(
                "by_activity.enable",
                activity.enabled,
                Message::Enabled,
                true
            ),
            super::widgets::settings_card(super::widgets::setting_row(
                "by_activity.idle_plan",
                iced::widget::container(plan_picker(
                    &activity.power_plans.power_save_guid,
                    Message::IdlePlan
                ))
                .width(design::SELECT_WIDTH)
            )),
            super::widgets::settings_card(super::widgets::setting_row(
                "by_activity.active_plan",
                iced::widget::container(plan_picker(
                    &activity.power_plans.performance_guid,
                    Message::ActivePlan
                ))
                .width(design::SELECT_WIDTH)
            )),
            flag(
                "by_activity.keyboard_input",
                activity.input_detection.keyboard,
                Message::Keyboard,
                activity.enabled
            ),
            flag(
                "by_activity.mouse_input",
                activity.input_detection.mouse,
                Message::Mouse,
                activity.enabled
            ),
            flag(
                "by_activity.controller_input",
                activity.input_detection.controller,
                Message::Controller,
                activity.enabled
            ),
            number(
                "by_activity.idle_timeout",
                inputs
                    .idle
                    .clone()
                    .unwrap_or_else(|| activity.idle_timeout_seconds.to_string()),
                1..=3600,
                1,
                "s",
                Message::IdleTimeout
            ),
            number(
                "by_activity.check_interval",
                inputs
                    .interval
                    .clone()
                    .unwrap_or_else(|| settings.general.check_interval_ms.to_string()),
                CHECK_INTERVAL_MIN_MS..=CHECK_INTERVAL_MAX_MS,
                250,
                "ms",
                Message::CheckInterval
            )
        ]
        .spacing(super::widgets::CARD_GAP),
    )
    .height(Fill)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn activity_keeps_a_wake_source_and_owns_its_plans() {
        let mut settings = Settings::default();
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
        let interval = settings.general.check_interval_ms;
        update(&mut settings, Message::CheckInterval("0".into()));
        assert_eq!(settings.general.check_interval_ms, interval);
        let mut inputs = Inputs::default();
        inputs.edit(&Message::CheckInterval(String::new()));
        assert!(!inputs.valid());
        inputs.edit(&Message::CheckInterval("250".into()));
        assert!(inputs.valid());
    }
}
