use super::design;
use super::widgets;
use crate::ui::scrolling::scrollable;
use iced::widget::column;
use iced::{Element, Fill};

use crate::config::{InputDetectionSettings, Settings};
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

fn input_change(
    current: &InputDetectionSettings,
    message: &Message,
) -> Option<InputDetectionSettings> {
    let mut changed = current.clone();
    match *message {
        Message::Keyboard(value) => changed.keyboard = value,
        Message::Mouse(value) => changed.mouse = value,
        Message::Controller(value) => changed.controller = value,
        _ => return None,
    }
    changed.any_enabled().then_some(changed)
}

pub(super) fn update(settings: &mut crate::application::SettingsEditor, message: Message) {
    let activity = &mut settings.by_activity;
    match message {
        Message::Enabled(value) => activity.enabled = value,
        Message::Keyboard(_) | Message::Mouse(_) | Message::Controller(_) => {
            if let Some(changed) = input_change(&activity.input_detection, &message) {
                activity.input_detection = changed;
            }
            activity.switch_to_performance_on_resume = activity.input_detection.any_enabled();
        }
        Message::IdleTimeout(value) => {
            if let Some(timeout) = parse_idle_timeout(&value) {
                activity.idle_timeout_seconds = timeout;
            }
        }
        Message::IdlePlan(guid) => activity.power_plans.power_save_guid = guid,
        Message::ActivePlan(guid) => activity.power_plans.performance_guid = guid,
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
    inputs: &'a Inputs,
) -> Element<'a, Message> {
    let activity = &settings.by_activity;
    let plan_row = |key: &str, guid: Option<&str>, action: fn(Option<String>) -> Message| {
        widgets::setting_row(
            key,
            iced::widget::container(widgets::plan(guid, plans, action)).width(design::SELECT_WIDTH),
        )
    };
    let flag = |key: &str, value: bool, action: fn(bool) -> Message| {
        let available = input_change(&activity.input_detection, &action(!value)).is_some();
        widgets::setting_row(key, widgets::switch(value, available.then_some(action)))
    };
    let power_plans = widgets::settings_rows([
        plan_row(
            "by_activity.idle_plan",
            activity.power_plans.power_save_guid.as_deref(),
            Message::IdlePlan,
        )
        .into(),
        plan_row(
            "by_activity.active_plan",
            activity.power_plans.performance_guid.as_deref(),
            Message::ActivePlan,
        )
        .into(),
    ]);
    let idle_timeout = widgets::slider_setting("by_activity.idle_timeout", "s", move || {
        widgets::slider_stepper(
            activity.idle_timeout_seconds,
            inputs.idle.as_deref(),
            IDLE_TIMEOUT_RANGE,
            1,
            Message::IdleTimeout,
        )
    });
    scrollable(
        column![
            widgets::toggle_card(
                "by_activity.enable",
                activity.enabled,
                Some(Message::Enabled)
            ),
            widgets::settings_section(
                "by_activity.power_plans",
                widgets::settings_card(power_plans),
            ),
            widgets::settings_section(
                "by_activity.input_detection",
                widgets::settings_card(widgets::settings_rows([
                    flag(
                        "by_activity.keyboard_input",
                        activity.input_detection.keyboard,
                        Message::Keyboard,
                    )
                    .into(),
                    flag(
                        "by_activity.mouse_input",
                        activity.input_detection.mouse,
                        Message::Mouse,
                    )
                    .into(),
                    flag(
                        "by_activity.controller_input",
                        activity.input_detection.controller,
                        Message::Controller,
                    )
                    .into(),
                    idle_timeout,
                ])),
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

    #[test]
    fn only_the_last_selected_input_is_unavailable_to_toggle() {
        use iced::advanced::{
            layout,
            widget::{operation::Focusable, Id, Operation, Tree},
            Layout,
        };
        use iced::{Rectangle, Size};

        #[derive(Default)]
        struct FocusableCount(usize);
        impl Operation for FocusableCount {
            fn traverse(&mut self, visit: &mut dyn FnMut(&mut dyn Operation)) {
                visit(self);
            }

            fn focusable(&mut self, _: Option<&Id>, _: Rectangle, _: &mut dyn Focusable) {
                self.0 += 1;
            }
        }

        let renderer = iced::Renderer::new(design::typography::FONT, iced::Pixels(14.0));
        let limits = layout::Limits::new(Size::ZERO, Size::new(466.0, 320.0));
        let inputs = Inputs::default();
        let count_focusable = |[keyboard, mouse, controller]: [bool; 3]| {
            let mut settings = Settings::default();
            settings.by_activity.input_detection = InputDetectionSettings {
                keyboard,
                mouse,
                controller,
            };
            let mut view = view(&settings, &[], &inputs);
            let mut tree = Tree::new(&view);
            let node = view.as_widget_mut().layout(&mut tree, &renderer, &limits);
            let mut count = FocusableCount::default();
            view.as_widget_mut()
                .operate(&mut tree, Layout::new(&node), &renderer, &mut count);
            count.0
        };
        let all_available = count_focusable([true, true, true]);
        for selected in [
            [true, false, false],
            [false, true, false],
            [false, false, true],
        ] {
            assert_eq!(count_focusable(selected) + 1, all_available);
        }
        assert_eq!(count_focusable([true, false, true]), all_available);
        assert_eq!(count_focusable([false, false, false]), all_available);
    }
}
