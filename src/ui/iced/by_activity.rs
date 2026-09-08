use iced::widget::{checkbox, column, row, scrollable, slider, text, text_input};
use iced::{Element, Fill};
use rust_i18n::t;

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
    scrollable(
        column![
            checkbox(activity.enabled)
                .label(t!("by_activity.enable").to_string())
                .on_toggle(Message::Enabled),
            text(t!("by_activity.intro_1").to_string()),
            text(t!("common.power_plan_priority").to_string()),
            text(t!("common.power_plan_pause_priority").to_string()),
            row![
                text(t!("by_activity.idle_plan").to_string()).width(240),
                plan_picker(&activity.power_plans.power_save_guid, Message::IdlePlan)
            ]
            .spacing(12),
            row![
                text(t!("by_activity.active_plan").to_string()).width(240),
                plan_picker(&activity.power_plans.performance_guid, Message::ActivePlan)
            ]
            .spacing(12),
            checkbox(activity.input_detection.keyboard)
                .label(t!("by_activity.keyboard_input").to_string())
                .on_toggle_maybe(activity.enabled.then_some(Message::Keyboard)),
            checkbox(activity.input_detection.mouse)
                .label(t!("by_activity.mouse_input").to_string())
                .on_toggle_maybe(activity.enabled.then_some(Message::Mouse)),
            checkbox(activity.input_detection.controller)
                .label(t!("by_activity.controller_input").to_string())
                .on_toggle_maybe(activity.enabled.then_some(Message::Controller)),
            row![
                text(t!("by_activity.idle_timeout").to_string()).width(180),
                slider(1..=3600, activity.idle_timeout_seconds as u32, |v| {
                    Message::IdleTimeout(v.to_string())
                }),
                text_input(
                    "1–3600 s",
                    &inputs
                        .idle
                        .clone()
                        .unwrap_or_else(|| activity.idle_timeout_seconds.to_string())
                )
                .on_input(Message::IdleTimeout)
                .width(130)
            ]
            .spacing(12),
            row![
                text(t!("by_activity.check_interval").to_string()).width(180),
                slider(
                    CHECK_INTERVAL_MIN_MS as u32..=CHECK_INTERVAL_MAX_MS as u32,
                    settings.general.check_interval_ms as u32,
                    |v| Message::CheckInterval(v.to_string())
                )
                .step(250u32),
                text_input(
                    "250–60000 ms",
                    &inputs
                        .interval
                        .clone()
                        .unwrap_or_else(|| settings.general.check_interval_ms.to_string())
                )
                .on_input(Message::CheckInterval)
                .width(130)
            ]
            .spacing(12),
        ]
        .spacing(16),
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
