use super::{design, edit_settings, scrollable, widgets, Editor, Message, PRESET_NAME_INPUT_ID};
use crate::application::SettingsEditor;
use crate::config::{AdaptiveEngineSettings, AdaptiveProcessorPowerPreset, PowerSourceProfile};
use crate::power::{AdaptivePowerBoostValues, ProcessorBoostMode, ProcessorPowerValues};
use iced::widget::{column, row, text};
use iced::{Element, Fill};
use rust_i18n::t;

pub(super) struct PresetDraft {
    index: Option<usize>,
    pub(super) name: String,
    pub(super) use_current_values: bool,
}

pub(super) enum PresetDialog {
    Edit(PresetDraft),
    Preview(PresetPreview),
}

pub(super) struct PresetPreview {
    preset: AdaptiveProcessorPowerPreset,
    source: PowerSourceProfile,
    built_in: Option<BuiltInPreset>,
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum PresetChoice<Name> {
    BuiltIn(BuiltInPreset),
    Custom(Name),
}

#[derive(Default)]
pub(super) struct PresetSelection {
    current: Option<PresetChoice<String>>,
    before_edit: Option<Option<PresetChoice<String>>>,
}

impl PresetSelection {
    fn choose(&mut self, choice: Option<PresetChoice<String>>, staged: bool) {
        if staged {
            self.before_edit.get_or_insert_with(|| self.current.take());
        } else {
            self.before_edit = None;
        }
        self.current = choice;
    }
}

impl PresetDraft {
    pub(super) fn new() -> Self {
        Self {
            index: None,
            name: String::new(),
            use_current_values: true,
        }
    }

    pub(super) fn edit(index: usize, name: String) -> Self {
        Self {
            index: Some(index),
            name,
            use_current_values: false,
        }
    }

    fn name_conflicts(&self, presets: &[AdaptiveProcessorPowerPreset]) -> bool {
        presets.iter().enumerate().any(|(index, preset)| {
            self.index != Some(index) && same_preset_name(&self.name, &preset.name)
        })
    }

    fn can_save(&self, settings: &SettingsEditor, invalid_values: bool) -> bool {
        let presets = &settings.global().adaptive_processor_power_presets;
        !self.name.trim().is_empty()
            && !self.name_conflicts(presets)
            && self.index.is_none_or(|index| index < presets.len())
            && (!self.use_current_values || !invalid_values)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::ui) enum BuiltInPreset {
    PowerSave,
    Balanced,
    Performance,
}

impl BuiltInPreset {
    pub(super) const ALL: [Self; 3] = [Self::PowerSave, Self::Balanced, Self::Performance];

    pub(super) fn label(self) -> String {
        match self {
            Self::PowerSave => t!("adaptive_engine.preset_power_save"),
            Self::Balanced => t!("processor_power.balanced"),
            Self::Performance => t!("processor_power.performance"),
        }
        .to_string()
    }

    pub(in crate::ui) fn apply(
        self,
        settings: &mut AdaptiveEngineSettings,
        source: PowerSourceProfile,
    ) {
        settings.base_processor_policy = match self {
            Self::PowerSave => {
                ProcessorPowerValues::new_with_boost_mode(0, 5, 45, 0, ProcessorBoostMode::Disabled)
            }
            Self::Balanced => ProcessorPowerValues::new_with_boost_mode(
                25,
                5,
                match source {
                    PowerSourceProfile::PluggedIn => 100,
                    PowerSourceProfile::OnBattery => 95,
                },
                60,
                ProcessorBoostMode::EfficientEnabled,
            ),
            Self::Performance => ProcessorPowerValues::new_with_boost_mode(
                100,
                25,
                100,
                85,
                ProcessorBoostMode::EfficientAggressive,
            ),
        };
        for (profile, defaults) in [
            (
                &mut settings.background_pressure_profile,
                AdaptivePowerBoostValues::BACKGROUND_PRESSURE,
            ),
            (
                &mut settings.focus_and_launch_profile,
                AdaptivePowerBoostValues::FOCUS_AND_LAUNCH,
            ),
        ] {
            match source {
                PowerSourceProfile::PluggedIn => {
                    profile.ac_policy = defaults.ac_policy;
                    profile.ac_mode = defaults.ac_mode;
                }
                PowerSourceProfile::OnBattery => {
                    profile.battery_policy = defaults.battery_policy;
                    profile.battery_mode = defaults.battery_mode;
                }
            }
        }
    }

    pub(super) fn matches(
        self,
        settings: &AdaptiveEngineSettings,
        source: PowerSourceProfile,
    ) -> bool {
        let mut preset = settings.clone();
        self.apply(&mut preset, source);
        preset == *settings
    }
}

impl Editor {
    pub(in crate::ui) fn preset_open(&self) -> bool {
        self.preset_dialog.is_some()
    }

    pub(super) fn preview_preset(
        &mut self,
        settings: &SettingsEditor,
        choice: PresetChoice<usize>,
    ) {
        let source = settings.selected_power_source();
        let (preset, built_in) = match choice {
            PresetChoice::BuiltIn(preset) => {
                let mut values = settings.adaptive_engine.clone();
                preset.apply(&mut values, source);
                (
                    AdaptiveProcessorPowerPreset::capture(preset.label(), &values, source),
                    Some(preset),
                )
            }
            PresetChoice::Custom(index) => {
                let Some(preset) = settings
                    .global()
                    .adaptive_processor_power_presets
                    .get(index)
                else {
                    return;
                };
                (preset.clone(), None)
            }
        };
        self.preset_dialog = Some(PresetDialog::Preview(PresetPreview {
            preset,
            source,
            built_in,
        }));
    }

    pub(super) fn use_preset(&mut self, settings: &mut SettingsEditor) {
        let Some(PresetDialog::Preview(preview)) = &self.preset_dialog else {
            return;
        };
        let source = preview.source;
        if source != settings.selected_power_source() {
            return;
        }
        let saved = settings.persisted();
        let saved_target = match source {
            PowerSourceProfile::PluggedIn => saved,
            PowerSourceProfile::OnBattery => saved.battery_profile(),
        };
        let staged = !preview
            .preset
            .matches(&saved_target.adaptive_engine, source)
            || (preview.built_in.is_none()
                && !saved
                    .adaptive_processor_power_presets
                    .contains(&preview.preset));
        self.numbers.drafts.clear();
        edit_settings(settings, |target| preview.preset.apply(target, source));
        let selected = preview.built_in.map_or_else(
            || PresetChoice::Custom(preview.preset.name.clone()),
            PresetChoice::BuiltIn,
        );
        self.preset_dialog = None;
        self.selected_presets[source as usize].choose(Some(selected), staged);
    }

    pub(super) fn save_preset(&mut self, settings: &mut SettingsEditor) {
        let Some(PresetDialog::Edit(draft)) = &self.preset_dialog else {
            return;
        };
        if !draft.can_save(settings, self.validation_error().is_some()) {
            return;
        }
        let name = draft.name.trim().to_string();
        let preset = match draft.index {
            Some(index) if !draft.use_current_values => {
                let mut preset = settings.global().adaptive_processor_power_presets[index].clone();
                preset.name = name;
                preset
            }
            _ => AdaptiveProcessorPowerPreset::capture(
                name,
                &settings.adaptive_engine,
                settings.selected_power_source(),
            ),
        };
        let index = draft.index;
        if let Some(index) = index {
            let previous = &settings.global().adaptive_processor_power_presets[index].name;
            self.replace_selected_preset_name(previous, Some(&preset.name));
        }
        settings.edit_global(|settings| {
            let presets = &mut settings.adaptive_processor_power_presets;
            if let Some(index) = index {
                presets[index] = preset;
            } else {
                presets.push(preset);
            }
        });
        self.preset_dialog = None;
    }

    pub(super) fn replace_selected_preset_name(&mut self, previous: &str, name: Option<&str>) {
        if name == Some(previous) {
            return;
        }
        for selected in &mut self.selected_presets {
            if matches!(selected.current.as_ref(), Some(PresetChoice::Custom(current)) if same_preset_name(current, previous))
            {
                selected.choose(name.map(|name| PresetChoice::Custom(name.to_owned())), true);
            }
        }
    }

    pub(in crate::ui) fn commit_preset_selection(&mut self) {
        for selected in &mut self.selected_presets {
            selected.before_edit = None;
        }
    }

    pub(in crate::ui) fn discard_preset_selection(&mut self) {
        for selected in &mut self.selected_presets {
            if let Some(before) = selected.before_edit.take() {
                selected.current = before;
            }
        }
    }

    pub(super) fn active_preset<'a>(
        &self,
        settings: &'a SettingsEditor,
    ) -> Option<PresetChoice<&'a str>> {
        if self.validation_error().is_some() {
            return None;
        }
        let source = settings.selected_power_source();
        let presets = &settings.global().adaptive_processor_power_presets;
        let builtin = |preset: BuiltInPreset| {
            preset
                .matches(&settings.adaptive_engine, source)
                .then_some(PresetChoice::BuiltIn(preset))
        };
        let custom = |preset: &'a AdaptiveProcessorPowerPreset| {
            preset
                .matches(&settings.adaptive_engine, source)
                .then_some(PresetChoice::Custom(preset.name.as_str()))
        };
        let selected = match self.selected_presets[source as usize].current.as_ref() {
            Some(PresetChoice::BuiltIn(preset)) => builtin(*preset),
            Some(PresetChoice::Custom(current)) => presets
                .iter()
                .find(|preset| same_preset_name(current, &preset.name))
                .and_then(custom),
            None => None,
        };
        selected
            .or_else(|| BuiltInPreset::ALL.into_iter().find_map(builtin))
            .or_else(|| presets.iter().find_map(custom))
    }

    pub(in crate::ui) fn presets_panel<'a>(
        &'a self,
        settings: &'a SettingsEditor,
    ) -> Element<'a, Message> {
        let active = self.active_preset(settings);
        let heading = |label| widgets::heading(label, design::typography::SECONDARY);
        let mut list = column![
            text(t!("adaptive_processor_power.preset_help").to_string())
                .size(design::typography::SECONDARY)
                .style(text::secondary)
                .width(Fill),
            heading(t!("adaptive_processor_power.built_in_presets").to_string()),
        ]
        .spacing(design::space::SMALL)
        .padding([0, design::space::MEDIUM as u16]);
        for preset in BuiltInPreset::ALL {
            list = list.push(preset_button(
                preset.label(),
                active == Some(PresetChoice::BuiltIn(preset)),
                Message::PreviewBuiltIn(preset),
            ));
        }
        let presets = &settings.global().adaptive_processor_power_presets;
        list = list.push(heading(
            t!("adaptive_processor_power.custom_presets").to_string(),
        ));
        if presets.is_empty() {
            list = list.push(
                text(t!("adaptive_processor_power.no_custom_presets").to_string())
                    .size(design::typography::SECONDARY)
                    .style(text::secondary),
            );
        }
        for (index, preset) in presets.iter().enumerate() {
            list = list.push(
                row![
                    preset_button(
                        preset.name.as_str(),
                        active == Some(PresetChoice::Custom(preset.name.as_str())),
                        Message::PreviewCustom(index),
                    ),
                    widgets::tooltip(
                        widgets::icon_button("icons/pencil.svg")
                            .on_press(Message::EditPreset(index)),
                        text(t!("common.edit").to_string()),
                        iced::widget::tooltip::Position::Top,
                    ),
                    widgets::rule_delete_button(Some(Message::DeletePreset(index))),
                ]
                .spacing(design::space::TIGHT)
                .align_y(iced::Center),
            );
        }
        column![
            scrollable(list).width(Fill).height(Fill),
            widgets::preset_footer(
                t!("adaptive_processor_power.add_preset").to_string(),
                Message::NewPreset,
            ),
        ]
        .spacing(design::space::MEDIUM)
        .height(Fill)
        .into()
    }

    pub(in crate::ui) fn preset_modal(
        &self,
        settings: &SettingsEditor,
    ) -> Option<Element<'_, Message>> {
        let draft = match self.preset_dialog.as_ref()? {
            PresetDialog::Edit(draft) => draft,
            PresetDialog::Preview(preview) => return Some(preview.view()),
        };
        let editing = draft.index.is_some();
        let validation_error = self.validation_error();
        let save = draft
            .can_save(settings, validation_error.is_some())
            .then_some(Message::SavePreset);
        let header = row![
            text(
                t!(if editing {
                    "adaptive_processor_power.edit_preset"
                } else {
                    "adaptive_processor_power.add_preset"
                })
                .to_string()
            )
            .width(Fill),
            widgets::icon_button("icons/x.svg").on_press(Message::CancelPreset),
        ]
        .align_y(iced::Center);
        let mut body = column![
            text(t!("adaptive_processor_power.preset_name").to_string()),
            widgets::text_input(
                &t!("adaptive_processor_power.preset_name_placeholder"),
                &draft.name,
            )
            .id(PRESET_NAME_INPUT_ID)
            .on_input(Message::PresetName)
            .on_submit_maybe(save.clone()),
        ]
        .spacing(design::space::SMALL);
        if editing {
            body = body.push(
                widgets::checkbox(draft.use_current_values)
                    .label(t!("adaptive_processor_power.use_current_settings").to_string())
                    .on_toggle(Message::UseCurrentPresetValues),
            );
        }
        if draft.use_current_values {
            body = body.push(
                text(t!("adaptive_processor_power.preset_creator_help").to_string())
                    .size(design::typography::SECONDARY)
                    .style(text::secondary)
                    .width(Fill),
            );
            if let Some(error) = validation_error {
                body = body.push(text(error).style(text::danger));
            }
        }
        if draft.name_conflicts(&settings.global().adaptive_processor_power_presets) {
            body = body.push(
                text(t!("adaptive_processor_power.duplicate_preset_name").to_string())
                    .style(text::danger),
            );
        }
        let footer = row![
            widgets::action_button(t!("common.cancel").to_string())
                .style(widgets::tertiary_button)
                .on_press(Message::CancelPreset),
            widgets::action_button(t!("common.save").to_string())
                .style(widgets::primary_button)
                .on_press_maybe(save),
        ]
        .spacing(design::space::SMALL);
        Some(super::super::modal::dismiss_on_escape(
            widgets::modal_frame(
                header,
                scrollable(body).width(Fill).height(Fill),
                footer,
                (500, 310),
            ),
            Message::CancelPreset,
        ))
    }
}

impl PresetPreview {
    fn view(&self) -> Element<'_, Message> {
        let mut values = AdaptiveEngineSettings::default();
        self.preset.apply(&mut values, self.source);
        let base = values.base_processor_policy;
        let demand_rows = |boost: AdaptivePowerBoostValues| {
            let (policy, mode) = match self.source {
                PowerSourceProfile::PluggedIn => (boost.ac_policy, boost.ac_mode),
                PowerSourceProfile::OnBattery => (boost.battery_policy, boost.battery_mode),
            };
            [
                preview_percent("processor_power.boost_policy", policy),
                preview_boost_mode(mode),
            ]
        };
        let source = match self.source {
            PowerSourceProfile::PluggedIn => "power_source.plugged_in",
            PowerSourceProfile::OnBattery => "power_source.on_battery",
        };
        let body = column![
            text(self.preset.name.as_str()).wrapping(text::Wrapping::WordOrGlyph),
            preview_row("power_source.editing_profile", t!(source).into_owned()),
            preview_group(
                "adaptive_processor_power.base_processor_policy",
                [
                    preview_percent("processor_power.core_parking_min", base.core_parking_min),
                    preview_percent("processor_power.processor_min", base.performance_min),
                    preview_percent("processor_power.processor_max", base.performance_max),
                    preview_percent("processor_power.boost_policy", base.boost_policy),
                    preview_boost_mode(base.boost_mode),
                ]
            ),
            preview_group(
                "adaptive_processor_power.background_pressure_profile",
                demand_rows(values.background_pressure_profile)
            ),
            preview_group(
                "adaptive_processor_power.focus_and_launch_profile",
                demand_rows(values.focus_and_launch_profile)
            ),
        ]
        .spacing(design::space::MEDIUM);
        let header = row![
            text(t!("adaptive_processor_power.preset_preview").into_owned()).width(Fill),
            widgets::icon_button("icons/x.svg").on_press(Message::CancelPreset),
        ]
        .align_y(iced::Center);
        let footer = row![
            widgets::action_button(t!("common.cancel").into_owned())
                .style(widgets::tertiary_button)
                .on_press(Message::CancelPreset),
            widgets::action_button(t!("adaptive_processor_power.use_preset").into_owned())
                .style(widgets::primary_button)
                .on_press(Message::UsePreset),
        ]
        .spacing(design::space::SMALL);
        widgets::modal_frame(
            header,
            scrollable(body).width(Fill).height(Fill),
            footer,
            (540, 520),
        )
    }
}

fn preview_group(
    label: &str,
    rows: impl IntoIterator<Item = Element<'static, Message>>,
) -> Element<'static, Message> {
    column![
        widgets::heading(t!(label).into_owned(), design::typography::SECONDARY),
        widgets::settings_card(widgets::settings_rows(rows).spacing(design::space::TIGHT)),
    ]
    .spacing(design::space::SMALL)
    .into()
}

fn preview_percent(label: &str, percent: u32) -> Element<'static, Message> {
    preview_row(label, format!("{percent}%"))
}

fn preview_boost_mode(mode: ProcessorBoostMode) -> Element<'static, Message> {
    preview_row(
        "processor_power.boost_mode",
        widgets::processor_boost_mode_label(mode).into_owned(),
    )
}

fn preview_row(label: &str, value: String) -> Element<'static, Message> {
    row![text(t!(label).into_owned()).width(Fill), text(value)]
        .spacing(design::space::MEDIUM)
        .align_y(iced::Center)
        .into()
}

fn same_preset_name(left: &str, right: &str) -> bool {
    left.trim().eq_ignore_ascii_case(right.trim())
}

fn preset_button<'a>(
    label: impl iced::widget::text::IntoFragment<'a>,
    matches: bool,
    message: Message,
) -> Element<'a, Message> {
    widgets::button(
        text(label)
            .width(Fill)
            .wrapping(text::Wrapping::WordOrGlyph),
    )
    .width(Fill)
    .style(move |theme, status| widgets::tab_style(theme, status, matches))
    .on_press(message)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_builtin_has_a_distinct_normalized_policy() {
        for preset in BuiltInPreset::ALL {
            let mut settings = AdaptiveEngineSettings::default();
            preset.apply(&mut settings, PowerSourceProfile::PluggedIn);
            let policy = settings.base_processor_policy;
            assert_eq!(policy, policy.normalized());
            for other in BuiltInPreset::ALL {
                assert_eq!(
                    other.matches(&settings, PowerSourceProfile::PluggedIn),
                    other == preset,
                );
            }
        }
    }

    #[test]
    fn presets_replace_selected_boost_values_and_preserve_other_settings() {
        let custom = AdaptivePowerBoostValues::new(
            10,
            ProcessorBoostMode::Disabled,
            20,
            ProcessorBoostMode::Disabled,
        );
        let preset = BuiltInPreset::Balanced;
        for (source, performance_max, enabled, processor_power_enabled) in [
            (PowerSourceProfile::PluggedIn, 100, true, false),
            (PowerSourceProfile::OnBattery, 95, false, true),
        ] {
            let mut settings = AdaptiveEngineSettings::default();
            settings.enabled = enabled;
            settings.adaptive_processor_power_enabled = processor_power_enabled;
            settings.background_pressure_profile = custom;
            settings.focus_and_launch_profile = custom;
            preset.apply(&mut settings, source);

            assert_eq!(
                settings.base_processor_policy.performance_max,
                performance_max
            );
            assert_eq!(settings.enabled, enabled);
            assert_eq!(
                settings.adaptive_processor_power_enabled,
                processor_power_enabled
            );
            assert!(preset.matches(&settings, source));
            for (profile, defaults) in [
                (
                    settings.background_pressure_profile,
                    AdaptivePowerBoostValues::BACKGROUND_PRESSURE,
                ),
                (
                    settings.focus_and_launch_profile,
                    AdaptivePowerBoostValues::FOCUS_AND_LAUNCH,
                ),
            ] {
                let expected = match source {
                    PowerSourceProfile::PluggedIn => AdaptivePowerBoostValues {
                        ac_policy: defaults.ac_policy,
                        ac_mode: defaults.ac_mode,
                        ..custom
                    },
                    PowerSourceProfile::OnBattery => AdaptivePowerBoostValues {
                        battery_policy: defaults.battery_policy,
                        battery_mode: defaults.battery_mode,
                        ..custom
                    },
                };
                assert_eq!(profile, expected);
            }

            settings.focus_and_launch_profile = custom;
            assert!(!preset.matches(&settings, source));
        }
    }
}
