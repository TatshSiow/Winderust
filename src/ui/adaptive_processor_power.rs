use super::adaptive_engine::{parse_number, Choice, NumberDraft, NumberInputs};
use super::widgets::pick_list;
use super::{design, widgets};
use crate::application::SettingsEditor;
use crate::config::{AdaptiveEngineSettings, PowerSourceProfile};
use crate::power::{ProcessorBoostMode, ProcessorPowerValues};
use crate::ui::scrolling::scrollable;
use iced::widget::{column, row, text};
use iced::{Element, Fill};

pub(super) const PROCESSOR_MIN_FIELD: &str =
    stringify!(adaptive_engine.base_processor_policy.performance_min);
pub(super) const PROCESSOR_MAX_FIELD: &str =
    stringify!(adaptive_engine.base_processor_policy.performance_max);

#[derive(Debug, Clone)]
pub(super) enum Message {
    Enabled(bool),
    Number(
        fn(&mut AdaptiveEngineSettings, u64),
        String,
        u64,
        u64,
        &'static str,
        &'static str,
    ),
    BoostMode(
        fn(&mut AdaptiveEngineSettings, ProcessorBoostMode),
        ProcessorBoostMode,
    ),
}

pub(super) fn update(settings: &mut SettingsEditor, numbers: &mut NumberInputs, message: Message) {
    match message {
        Message::Enabled(enabled) => edit_settings(settings, |target| {
            target.adaptive_processor_power_enabled = enabled;
        }),
        Message::BoostMode(update, mode) => edit_settings(settings, |target| update(target, mode)),
        Message::Number(update, text, minimum, maximum, key, label) => {
            let before = settings.adaptive_engine.base_processor_policy;
            let minimum = if key == PROCESSOR_MAX_FIELD {
                u64::from(before.performance_min.min(100))
            } else {
                minimum
            };
            let value = parse_number(&text, minimum, maximum);
            numbers.drafts.insert(
                key,
                NumberDraft {
                    text,
                    invalid: value.is_none().then_some((minimum, maximum, label)),
                },
            );
            if let Some(value) = value {
                edit_settings(settings, |target| {
                    update(target, value);
                    normalize_number(target, numbers, key, before);
                });
            }
        }
    }
}

fn edit_settings(settings: &mut SettingsEditor, update: impl FnOnce(&mut AdaptiveEngineSettings)) {
    let mut edited = settings.adaptive_engine.clone();
    update(&mut edited);
    if edited == settings.adaptive_engine {
        return;
    }
    let source = settings.selected_power_source();
    settings.edit_global(|settings| {
        settings.battery_profile_mut();
        let target = match source {
            PowerSourceProfile::PluggedIn => settings,
            PowerSourceProfile::OnBattery => settings.battery_profile_mut(),
        };
        target.adaptive_engine = edited;
    });
}

pub(super) fn view<'a>(s: &'a SettingsEditor, numbers: &'a NumberInputs) -> Element<'a, Message> {
    let mut body = column![].spacing(widgets::CARD_GAP);
    macro_rules! number {
        ($label:literal, $help:literal, $min:expr, adaptive_engine.$($field:ident).+) => {{
            let key = stringify!(adaptive_engine.$($field).+);
            let min = $min;
            let formatted = s.adaptive_engine.$($field).+.to_string();
            let value = numbers.drafts.get(key).map_or(formatted.as_str(), |draft| draft.text.as_str());
            row![
                widgets::setting_title_with_help($label, "%", $help).width(Fill),
                widgets::stepper(value, min..=100, 1, Some(move |value| Message::Number(
                    |settings, number| settings.$($field).+ = number as _,
                    value, min, 100, key, $label,
                ))),
            ]
            .spacing(design::space::SMALL)
            .padding([design::space::CONTROL as u16, 0])
            .align_y(iced::Center)
        }};
    }
    macro_rules! boost {
        ($help:literal, adaptive_engine.$($field:ident).+) => {
            boost_mode_row($help, s.adaptive_engine.$($field).+, |settings, mode| {
                settings.$($field).+ = mode;
            })
        };
    }
    macro_rules! profile_card {
        ($profile:ident, $policy:ident, $mode:ident) => {
            widgets::settings_card(widgets::settings_rows([
                number!(
                    "processor_power.boost_policy",
                    "adaptive_processor_power.profile_boost_policy_help",
                    0,
                    adaptive_engine.$profile.$policy
                )
                .into(),
                boost!(
                    "adaptive_processor_power.profile_boost_mode_help",
                    adaptive_engine.$profile.$mode
                ),
            ]))
        };
    }
    body = body.push(widgets::toggle_card(
        "adaptive_processor_power.enable",
        s.adaptive_engine.adaptive_processor_power_enabled,
        Some(Message::Enabled),
    ));
    body = body.push(widgets::setting_title(
        "adaptive_processor_power.base_processor_policy",
    ));
    body = body.push(widgets::settings_card(widgets::settings_rows([
        number!(
            "processor_power.core_parking_min",
            "adaptive_processor_power.core_parking_min_help",
            0,
            adaptive_engine.base_processor_policy.core_parking_min
        )
        .into(),
        number!(
            "processor_power.processor_min",
            "adaptive_processor_power.processor_min_help",
            0,
            adaptive_engine.base_processor_policy.performance_min
        )
        .into(),
        number!(
            "processor_power.processor_max",
            "adaptive_processor_power.processor_max_help",
            u64::from(
                s.adaptive_engine
                    .base_processor_policy
                    .performance_min
                    .min(100)
            ),
            adaptive_engine.base_processor_policy.performance_max
        )
        .into(),
        number!(
            "processor_power.boost_policy",
            "adaptive_processor_power.base_boost_policy_help",
            0,
            adaptive_engine.base_processor_policy.boost_policy
        )
        .into(),
        boost!(
            "adaptive_processor_power.base_boost_mode_help",
            adaptive_engine.base_processor_policy.boost_mode
        ),
    ])));

    let (background, focus_and_launch) = match s.selected_power_source() {
        PowerSourceProfile::PluggedIn => (
            profile_card!(background_pressure_profile, ac_policy, ac_mode),
            profile_card!(focus_and_launch_profile, ac_policy, ac_mode),
        ),
        PowerSourceProfile::OnBattery => (
            profile_card!(background_pressure_profile, battery_policy, battery_mode),
            profile_card!(focus_and_launch_profile, battery_policy, battery_mode),
        ),
    };
    body = body
        .push(widgets::setting_title(
            "adaptive_processor_power.background_pressure_profile",
        ))
        .push(background)
        .push(widgets::setting_title(
            "adaptive_processor_power.focus_and_launch_profile",
        ))
        .push(focus_and_launch);
    if let Some(error) = numbers.validation_error() {
        body = body.push(text(error).style(text::danger));
    }
    scrollable(body)
        .id("adaptive-processor-power-settings")
        .width(Fill)
        .height(Fill)
        .into()
}

fn normalize_number(
    settings: &mut AdaptiveEngineSettings,
    numbers: &mut NumberInputs,
    key: &str,
    before: ProcessorPowerValues,
) {
    let policy = &mut settings.base_processor_policy;
    *policy = policy.normalized();
    if before.performance_min != policy.performance_min {
        revalidate_processor_max(numbers, policy);
    }
    for (field, prior, current) in [
        (
            PROCESSOR_MIN_FIELD,
            before.performance_min,
            policy.performance_min,
        ),
        (
            PROCESSOR_MAX_FIELD,
            before.performance_max,
            policy.performance_max,
        ),
    ] {
        if field != key
            && prior != current
            && numbers.drafts.get(field).is_some_and(|draft| {
                draft.invalid.is_none() && draft.text.parse::<u32>().ok() != Some(current)
            })
        {
            numbers.drafts.remove(field);
        }
    }
}

fn revalidate_processor_max(numbers: &mut NumberInputs, policy: &mut ProcessorPowerValues) {
    let Some(draft) = numbers.drafts.get_mut(PROCESSOR_MAX_FIELD) else {
        return;
    };
    let Some((_, max, label)) = draft.invalid else {
        return;
    };
    let min = u64::from(policy.performance_min);
    let value = parse_number(&draft.text, min, max);
    if let Some(value) = value {
        policy.performance_max = value as u32;
    }
    draft.invalid = value.is_none().then_some((min, max, label));
}

fn boost_mode_row(
    help: &str,
    selected: ProcessorBoostMode,
    update: fn(&mut AdaptiveEngineSettings, ProcessorBoostMode),
) -> Element<'static, Message> {
    let label_mode = |mode| widgets::processor_boost_mode_label(mode).into_owned();
    let choices: Vec<_> = ProcessorBoostMode::ALL
        .into_iter()
        .map(|mode| Choice(mode, label_mode(mode)))
        .collect();
    let selected = Some(Choice(selected, label_mode(selected)));
    row![
        widgets::setting_title_with_help("processor_power.boost_mode", "", help).width(Fill),
        pick_list(choices, selected, move |choice| Message::BoostMode(
            update, choice.0
        ))
        .width(280),
    ]
    .spacing(design::space::SMALL)
    .padding([design::space::CONTROL as u16, 0])
    .align_y(iced::Center)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Settings;

    #[test]
    fn visible_boost_rows_edit_only_the_selected_source() {
        use iced::advanced::{layout, widget::Tree, Shell};
        use iced::{mouse, Event, Rectangle, Size};

        fn unwrap_single_child(mut layout: layout::Layout<'_>) -> layout::Layout<'_> {
            while layout.children().len() == 1 {
                layout = layout.children().next().unwrap();
            }
            layout
        }

        let size = Size::new(760.0, 1200.0);
        let renderer = iced::Renderer::new(iced::Font::DEFAULT, iced::Pixels(14.0));
        let mut expected = Settings::default();
        expected.adaptive_engine.focus_and_launch_profile.ac_policy = 95;
        expected
            .battery_profile_mut()
            .adaptive_engine
            .focus_and_launch_profile
            .battery_policy = 75;
        let mut settings = SettingsEditor::with_settings(expected.clone());

        for source in [PowerSourceProfile::PluggedIn, PowerSourceProfile::OnBattery] {
            settings.select_power_source(source);
            let mut numbers = NumberInputs::default();
            let messages = {
                let mut view = view(&settings, &numbers);
                let mut tree = Tree::new(&view);
                let node = view.as_widget_mut().layout(
                    &mut tree,
                    &renderer,
                    &layout::Limits::new(Size::ZERO, size),
                );
                let layout = layout::Layout::new(&node);
                let body = unwrap_single_child(layout);
                let mut messages = Vec::new();
                for card in body.children().skip(4).step_by(2) {
                    let rows = unwrap_single_child(card);
                    assert_eq!(rows.children().len(), 3, "one policy, divider and mode");
                    let policy = rows.children().next().unwrap();
                    let stepper = policy.children().last().unwrap();
                    let increment = stepper.children().last().unwrap().bounds().center();
                    for event in [
                        mouse::Event::ButtonPressed(mouse::Button::Left),
                        mouse::Event::ButtonReleased(mouse::Button::Left),
                    ] {
                        view.as_widget_mut().update(
                            &mut tree,
                            &Event::Mouse(event),
                            layout,
                            mouse::Cursor::Available(increment),
                            &renderer,
                            &mut iced::advanced::clipboard::Null,
                            &mut Shell::new(&mut messages),
                            &Rectangle::with_size(size),
                        );
                    }
                }
                assert_eq!(messages.len(), 2);
                messages
            };
            for message in messages {
                update(&mut settings, &mut numbers, message);
            }

            match source {
                PowerSourceProfile::PluggedIn => {
                    expected
                        .adaptive_engine
                        .background_pressure_profile
                        .ac_policy += 1;
                    expected.adaptive_engine.focus_and_launch_profile.ac_policy += 1;
                }
                PowerSourceProfile::OnBattery => {
                    let battery = &mut expected.battery_profile_mut().adaptive_engine;
                    battery.background_pressure_profile.battery_policy += 1;
                    battery.focus_and_launch_profile.battery_policy += 1;
                }
            }
            assert_eq!(settings.global(), &expected);
        }
    }

    #[test]
    fn numeric_and_boost_edits_isolate_profiles_without_dirtying_invalid_or_unchanged_input() {
        let mut settings = SettingsEditor::with_settings(Settings::default());
        let before = settings.adaptive_engine.clone();
        let mut numbers = NumberInputs::default();
        let set_max = |text: String| {
            Message::Number(
                |target, value| target.base_processor_policy.performance_max = value as u32,
                text,
                0,
                100,
                PROCESSOR_MAX_FIELD,
                "processor_power.processor_max",
            )
        };
        update(&mut settings, &mut numbers, set_max(String::new()));
        assert!(numbers.validation_error().is_some());
        assert!(!settings.has_unsaved_changes());
        assert!(settings.global().on_battery.is_none());
        update(
            &mut settings,
            &mut numbers,
            set_max(before.base_processor_policy.performance_max.to_string()),
        );
        assert!(numbers.validation_error().is_none());
        assert!(!settings.has_unsaved_changes());
        assert!(settings.global().on_battery.is_none());
        update(&mut settings, &mut numbers, set_max("80".into()));
        assert_eq!(
            settings
                .adaptive_engine
                .base_processor_policy
                .performance_max,
            80
        );
        assert_eq!(settings.global().battery_profile().adaptive_engine, before);
        let ac = settings.adaptive_engine.clone();
        settings.select_power_source(PowerSourceProfile::OnBattery);
        update(
            &mut settings,
            &mut numbers,
            Message::BoostMode(
                |target, mode| target.base_processor_policy.boost_mode = mode,
                ProcessorBoostMode::Aggressive,
            ),
        );
        assert_eq!(
            settings.adaptive_engine.base_processor_policy.boost_mode,
            ProcessorBoostMode::Aggressive
        );
        assert_eq!(settings.global().adaptive_engine, ac);
    }
}
