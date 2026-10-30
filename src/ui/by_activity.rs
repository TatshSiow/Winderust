use super::widgets::{self, Ui};
use windui::prelude::{signal, Element};

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
            if let Ok(value) = value.parse::<u64>() {
                if (1..=3600).contains(&value) {
                    activity.idle_timeout_seconds = value;
                }
            }
        }
        Message::CheckInterval(value) => {
            if let Ok(value) = value.parse::<u64>() {
                if (CHECK_INTERVAL_MIN_MS..=CHECK_INTERVAL_MAX_MS).contains(&value) {
                    settings.edit_global(|settings| settings.general.check_interval_ms = value);
                }
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

pub(super) fn view(
    settings: &Settings,
    plans: &[PowerPlan],
    inputs: &Inputs,
    ui: Ui<Message>,
) -> Element {
    let activity = &settings.by_activity;
    widgets::page([
        widgets::setting_row(
            rust_i18n::t!("by_activity.enable").to_string(),
            ui.toggle(activity.enabled, Message::Enabled),
        ),
        widgets::setting_row(
            rust_i18n::t!("by_activity.idle_plan").to_string(),
            ui.plan(
                activity.power_plans.power_save_guid.clone(),
                plans,
                Message::IdlePlan,
            ),
        ),
        widgets::setting_row(
            rust_i18n::t!("by_activity.active_plan").to_string(),
            ui.plan(
                activity.power_plans.performance_guid.clone(),
                plans,
                Message::ActivePlan,
            ),
        ),
        widgets::setting_row(
            rust_i18n::t!("by_activity.keyboard_input").to_string(),
            ui.toggle(activity.input_detection.keyboard, Message::Keyboard),
        ),
        widgets::setting_row(
            rust_i18n::t!("by_activity.mouse_input").to_string(),
            ui.toggle(activity.input_detection.mouse, Message::Mouse),
        ),
        widgets::setting_row(
            rust_i18n::t!("by_activity.controller_input").to_string(),
            ui.toggle(activity.input_detection.controller, Message::Controller),
        ),
        widgets::setting_row(
            format!("{} (s)", rust_i18n::t!("by_activity.idle_timeout")),
            ui.number(
                signal(
                    inputs
                        .idle
                        .clone()
                        .unwrap_or_else(|| activity.idle_timeout_seconds.to_string()),
                ),
                1,
                3600,
                1,
                Message::IdleTimeout,
            ),
        ),
        widgets::setting_row(
            format!("{} (ms)", rust_i18n::t!("by_activity.check_interval")),
            ui.number(
                signal(
                    inputs
                        .interval
                        .clone()
                        .unwrap_or_else(|| settings.general.check_interval_ms.to_string()),
                ),
                CHECK_INTERVAL_MIN_MS,
                CHECK_INTERVAL_MAX_MS,
                250,
                Message::CheckInterval,
            ),
        ),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn battery_interval_messages_edit_shared_settings_without_flattening_policies() {
        for existing_battery in [false, true] {
            let mut initial = Settings::default();
            initial.by_activity.idle_timeout_seconds = 60;
            if existing_battery {
                initial
                    .battery_profile_mut()
                    .by_activity
                    .idle_timeout_seconds = 120;
            }
            let mut settings = crate::application::SettingsEditor::with_settings(initial.clone());
            settings.select_power_source(crate::config::PowerSourceProfile::OnBattery);
            update(&mut settings, Message::CheckInterval("1234".into()));
            assert_eq!(settings.global().general.check_interval_ms, 1234);
            assert_eq!(settings.general.check_interval_ms, 1234);
            assert_eq!(settings.global().by_activity.idle_timeout_seconds, 60);
            assert_eq!(
                settings.by_activity.idle_timeout_seconds,
                if existing_battery { 120 } else { 60 }
            );
            for invalid in ["", "0", "invalid", "18446744073709551615"] {
                update(&mut settings, Message::CheckInterval(invalid.into()));
                assert_eq!(settings.global().general.check_interval_ms, 1234);
            }
            update(&mut settings, Message::IdleTimeout("180".into()));
            assert_eq!(settings.by_activity.idle_timeout_seconds, 180);
            assert_eq!(settings.global().by_activity.idle_timeout_seconds, 60);
            settings.cancel();
            assert_eq!(settings.global(), &initial);
        }
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
