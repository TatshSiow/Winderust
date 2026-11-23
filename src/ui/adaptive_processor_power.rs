use super::adaptive_engine::{parse_number, Choice, NumberDraft, NumberInputs};
use super::widgets::pick_list;
use super::{design, widgets};
use crate::application::SettingsEditor;
use crate::config::{AdaptiveEngineSettings, PowerSourceProfile};
use crate::power::{ProcessorBoostMode, ProcessorPowerValues};
use crate::ui::scrolling::scrollable;
use iced::widget::{column, container, row, text};
use iced::{Element, Fill};
use rust_i18n::t;

mod presets;
pub(super) use presets::BuiltInPreset;
use presets::{PresetChoice, PresetDialog, PresetDraft, PresetSelection};

#[cfg(test)]
mod editor_tests;

const PROCESSOR_MIN_FIELD: &str = stringify!(adaptive_engine.base_processor_policy.performance_min);
pub(super) const PROCESSOR_MAX_FIELD: &str =
    stringify!(adaptive_engine.base_processor_policy.performance_max);
pub(super) const PRESET_NAME_INPUT_ID: &str = "adaptive-processor-power-preset-name";

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) enum Tab {
    #[default]
    BasePolicy,
    DemandProfiles,
}

#[derive(Default)]
pub(super) struct Editor {
    numbers: NumberInputs,
    tab: Tab,
    preset_dialog: Option<PresetDialog>,
    selected_presets: [PresetSelection; 2],
}

#[derive(Debug, Clone)]
pub(super) enum Message {
    Tab(Tab),
    PreviewBuiltIn(BuiltInPreset),
    PreviewCustom(usize),
    UsePreset,
    NewPreset,
    EditPreset(usize),
    PresetName(String),
    UseCurrentPresetValues(bool),
    SavePreset,
    CancelPreset,
    DeletePreset(usize),
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

impl Editor {
    pub(super) fn update(&mut self, settings: &mut SettingsEditor, message: Message) {
        let numbers = &mut self.numbers;
        match message {
            Message::Tab(tab) => self.tab = tab,
            Message::PreviewBuiltIn(preset) => {
                self.preview_preset(settings, PresetChoice::BuiltIn(preset));
            }
            Message::PreviewCustom(index) => {
                self.preview_preset(settings, PresetChoice::Custom(index));
            }
            Message::UsePreset => self.use_preset(settings),
            Message::NewPreset => self.preset_dialog = Some(PresetDialog::Edit(PresetDraft::new())),
            Message::EditPreset(index) => {
                if let Some(preset) = settings
                    .global()
                    .adaptive_processor_power_presets
                    .get(index)
                {
                    self.preset_dialog = Some(PresetDialog::Edit(PresetDraft::edit(
                        index,
                        preset.name.clone(),
                    )));
                }
            }
            Message::PresetName(name) => {
                if let Some(PresetDialog::Edit(draft)) = &mut self.preset_dialog {
                    draft.name = name;
                }
            }
            Message::UseCurrentPresetValues(value) => {
                if let Some(PresetDialog::Edit(draft)) = &mut self.preset_dialog {
                    draft.use_current_values = value;
                }
            }
            Message::SavePreset => self.save_preset(settings),
            Message::CancelPreset => self.preset_dialog = None,
            Message::DeletePreset(index) => {
                if let Some(preset) = settings
                    .global()
                    .adaptive_processor_power_presets
                    .get(index)
                {
                    self.replace_selected_preset_name(&preset.name, None);
                    settings.edit_global(|settings| {
                        settings.adaptive_processor_power_presets.remove(index);
                    });
                }
            }
            Message::Enabled(enabled) => edit_settings(settings, |target| {
                target.adaptive_processor_power_enabled = enabled;
            }),
            Message::BoostMode(update, mode) => {
                edit_settings(settings, |target| update(target, mode))
            }
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

    pub(super) fn validation_error(&self) -> Option<String> {
        self.numbers.validation_error()
    }

    pub(super) fn reset_drafts(&mut self) {
        self.numbers.drafts.clear();
        self.preset_dialog = None;
    }

    pub(super) fn view<'a>(&'a self, s: &'a SettingsEditor) -> Element<'a, Message> {
        let numbers = &self.numbers;
        macro_rules! number {
            ($label:literal, $help:literal, $min:expr, adaptive_engine.$($field:ident).+) => {{
                let key = stringify!(adaptive_engine.$($field).+);
                let min = $min;
                let formatted = s.adaptive_engine.$($field).+.to_string();
                let value = numbers
                    .drafts
                    .get(key)
                    .map_or(formatted.as_str(), |draft| draft.text.as_str());
                row![
                    widgets::setting_title_with_help($label, "%", $help).width(Fill),
                    widgets::stepper(
                        value,
                        min..=100,
                        1,
                        Some(move |value| Message::Number(
                            |settings, number| settings.$($field).+ = number as _,
                            value,
                            min,
                            100,
                            key,
                            $label,
                        )),
                    ),
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
        let error = self.validation_error();
        let source = s.selected_power_source();
        let tabs = row![
            widgets::panel_tab(
                t!("adaptive_processor_power.base_processor_policy").to_string(),
                self.tab == Tab::BasePolicy,
                Message::Tab(Tab::BasePolicy),
            ),
            widgets::panel_tab(
                t!("adaptive_processor_power.demand_profiles").to_string(),
                self.tab == Tab::DemandProfiles,
                Message::Tab(Tab::DemandProfiles),
            ),
        ];
        let help_key = match self.tab {
            Tab::BasePolicy => "adaptive_processor_power.base_processor_policy_help",
            Tab::DemandProfiles => "adaptive_processor_power.demand_profiles_help",
        };
        let mut body = column![
            widgets::toggle_card(
                "adaptive_processor_power.enable",
                s.adaptive_engine.adaptive_processor_power_enabled,
                Some(Message::Enabled),
            ),
            container(tabs.wrap()).style(widgets::tab_bar),
            text(t!(help_key).to_string())
                .size(design::typography::SECONDARY)
                .style(text::secondary)
                .width(Fill),
        ]
        .spacing(widgets::CARD_GAP);
        match self.tab {
            Tab::BasePolicy => {
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
            }

            Tab::DemandProfiles => {
                let (background, focus_and_launch) = match source {
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
            }
        }
        if let Some(error) = error {
            body = body.push(text(error).style(text::danger));
        }
        scrollable(body)
            .id("adaptive-processor-power-settings")
            .width(Fill)
            .height(Fill)
            .into()
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
            let mut editor = Editor {
                tab: Tab::DemandProfiles,
                ..Editor::default()
            };
            let messages = {
                let mut view = editor.view(&settings);
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
                editor.update(&mut settings, message);
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
        let mut editor = Editor::default();
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
        editor.update(&mut settings, set_max(String::new()));
        assert!(editor.validation_error().is_some());
        assert!(!settings.has_unsaved_changes());
        assert!(settings.global().on_battery.is_none());
        editor.update(
            &mut settings,
            set_max(before.base_processor_policy.performance_max.to_string()),
        );
        assert!(editor.validation_error().is_none());
        assert!(!settings.has_unsaved_changes());
        assert!(settings.global().on_battery.is_none());
        editor.update(&mut settings, set_max("80".into()));
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
        editor.update(
            &mut settings,
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

    #[test]
    fn reapplying_preset_clears_invalid_input_without_staging_changes() {
        let mut initial = Settings::default();
        BuiltInPreset::Balanced.apply(&mut initial.adaptive_engine, PowerSourceProfile::PluggedIn);
        let mut settings = SettingsEditor::with_settings(initial);
        let mut editor = Editor::default();
        editor.update(
            &mut settings,
            Message::Number(
                |target, value| target.base_processor_policy.performance_max = value as u32,
                String::new(),
                0,
                100,
                PROCESSOR_MAX_FIELD,
                "processor_power.processor_max",
            ),
        );
        editor.update(&mut settings, Message::Tab(Tab::DemandProfiles));
        assert!(editor.validation_error().is_some());
        assert!(!settings.has_unsaved_changes());
        editor.update(
            &mut settings,
            Message::PreviewBuiltIn(BuiltInPreset::Balanced),
        );
        editor.update(&mut settings, Message::UsePreset);
        assert!(editor.validation_error().is_none());
        assert!(!settings.has_unsaved_changes());
    }
}
