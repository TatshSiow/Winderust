use super::design;
use super::widgets::{button, pick_list, slider, text_input};
use crate::application::AdvancedPowerPlanTuningService;
use crate::config::AdvancedPowerPlanTuningPreset;
use crate::power::{
    EffectivePowerMode, PowerPlan, PowerPlanPersonality, ProcessorBoostMode, ProcessorPowerPreset,
    ProcessorPowerSourceValues, ProcessorPowerValues,
};
use iced::widget::{column, row, scrollable, text};
use iced::{Element, Fill};
use rust_i18n::t;

#[derive(Default)]
pub(super) struct Editor {
    service: AdvancedPowerPlanTuningService,
    target: Option<String>,
    values: Option<ProcessorPowerSourceValues>,
    personality: Option<PowerPlanPersonality>,
    pub(super) dirty: bool,
    pub(super) status: String,
    preset: Option<PresetEditor>,
    collapsed: [bool; 2],
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Source {
    Ac,
    Battery,
}
#[derive(Debug, Clone, Copy)]
pub(super) enum Field {
    Parking,
    Minimum,
    Maximum,
    Boost,
}
#[derive(Debug, Clone, Copy)]
pub(super) enum PresetTarget {
    BuiltIn(ProcessorPowerPreset),
    Custom(Option<usize>),
}
struct PresetEditor {
    target: PresetTarget,
    name: String,
    values: ProcessorPowerValues,
}
#[derive(Debug, Clone)]
pub(super) enum Message {
    Plan(String),
    Group(Source),
    Refresh,
    Apply,
    Value(Source, Field, u32),
    ValueText(Source, Field, String),
    Boost(Source, ProcessorBoostMode),
    Load(Source, ProcessorPowerValues),
    OpenPreset(PresetTarget),
    PresetName(String),
    PresetValue(Field, u32),
    PresetValueText(Field, String),
    PresetBoost(ProcessorBoostMode),
    SavePreset,
    ClosePreset,
    Remove(usize),
}
impl Editor {
    pub(super) fn has_pending_editor(&self) -> bool {
        self.preset
            .as_ref()
            .is_some_and(|p| matches!(p.target, PresetTarget::Custom(_)))
    }
    pub(super) fn discard_editor(&mut self) {
        self.preset = None;
    }

    pub(super) fn ensure_plan(&mut self, plans: &[PowerPlan]) {
        if self.dirty {
            return;
        }
        if !plans.iter().any(|p| Some(&p.guid) == self.target.as_ref()) {
            self.target = plans
                .iter()
                .find(|p| p.active)
                .or_else(|| plans.first())
                .map(|p| p.guid.clone());
            self.refresh();
        } else if self.values.is_none() {
            self.refresh();
        }
    }
    pub(super) fn refresh(&mut self) {
        let Some(guid) = &self.target else {
            self.values = None;
            self.status = t!("processor_power.no_active_plan").to_string();
            return;
        };
        self.personality = match self.service.read_personality(guid) {
            Ok(v) => Some(v),
            Err(error) => {
                self.status = error.to_string();
                None
            }
        };
        match self.service.read_values(guid) {
            Ok(v) => {
                self.values = Some(v);
                self.dirty = false;
                self.status = t!("processor_power.loaded_values", plan = guid).to_string();
            }
            Err(error) => {
                self.status = error.to_string();
            }
        }
    }
    pub(super) fn apply(&mut self) -> bool {
        let (Some(guid), Some(values)) = (&self.target, self.values) else {
            self.status = t!("processor_power.no_active_plan").to_string();
            return false;
        };
        let outcome = self.service.apply_values(guid, values);
        if let Some(actual) = outcome.actual_values {
            self.values = Some(actual);
            self.dirty = false;
        }
        match outcome.error {
            Some(error) => {
                self.status = error.to_string();
                false
            }
            None => {
                self.dirty = false;
                self.status = t!("processor_power.applied_custom", plan = guid).to_string();
                true
            }
        }
    }
    pub(super) fn update(
        &mut self,
        presets: &mut Vec<AdvancedPowerPlanTuningPreset>,
        plans: &[PowerPlan],
        message: Message,
    ) {
        match message {
            Message::Group(source) => {
                self.collapsed[source as usize] = !self.collapsed[source as usize];
            }
            Message::Plan(guid) => {
                if self.target.as_ref() == Some(&guid) {
                    return;
                }
                if self.dirty {
                    self.status = t!("processor_power.save_before_changing_plan").to_string();
                } else if plans.iter().any(|p| p.guid.eq_ignore_ascii_case(&guid)) {
                    self.target = Some(guid);
                    self.values = None;
                    self.refresh();
                }
            }
            Message::Refresh => self.refresh(),
            Message::Apply => {
                self.apply();
            }
            Message::Value(source, field, value) => {
                if let Some(values) = self.values.as_mut() {
                    set_field(source_values(values, source), field, value);
                    self.dirty = true;
                }
            }
            Message::ValueText(source, field, value) => {
                if let Ok(v) = value.parse::<u32>() {
                    if v <= 100 {
                        self.update(presets, plans, Message::Value(source, field, v));
                    }
                }
            }
            Message::Boost(source, value) => {
                if let Some(values) = self.values.as_mut() {
                    source_values(values, source).boost_mode = value;
                    self.dirty = true;
                }
            }
            Message::Load(source, value) => {
                if let Some(values) = self.values.as_mut() {
                    *source_values(values, source) = value.normalized();
                    self.dirty = true;
                }
            }
            Message::OpenPreset(target) => {
                let (name, values) = match target {
                    PresetTarget::BuiltIn(p) => {
                        (preset_label(p), ProcessorPowerValues::for_preset(p))
                    }
                    PresetTarget::Custom(Some(i)) => {
                        let Some(p) = presets.get(i) else {
                            return;
                        };
                        (p.name.clone(), p.values.normalized())
                    }
                    PresetTarget::Custom(None) => (
                        String::new(),
                        self.values.map(|v| v.ac).unwrap_or_else(|| {
                            ProcessorPowerValues::for_preset(ProcessorPowerPreset::Balanced)
                        }),
                    ),
                };
                self.preset = Some(PresetEditor {
                    target,
                    name,
                    values,
                });
            }
            Message::PresetName(v) => {
                if let Some(p) = self
                    .preset
                    .as_mut()
                    .filter(|p| matches!(p.target, PresetTarget::Custom(_)))
                {
                    p.name = v;
                }
            }
            Message::PresetValue(field, v) => {
                if let Some(p) = self
                    .preset
                    .as_mut()
                    .filter(|p| matches!(p.target, PresetTarget::Custom(_)))
                {
                    set_field(&mut p.values, field, v);
                }
            }
            Message::PresetValueText(field, v) => {
                if let Ok(v) = v.parse::<u32>() {
                    if v <= 100 {
                        self.update(presets, plans, Message::PresetValue(field, v));
                    }
                }
            }
            Message::PresetBoost(v) => {
                if let Some(p) = self
                    .preset
                    .as_mut()
                    .filter(|p| matches!(p.target, PresetTarget::Custom(_)))
                {
                    p.values.boost_mode = v;
                }
            }
            Message::SavePreset => {
                if let Some(p) = &self.preset {
                    if let PresetTarget::Custom(index) = p.target {
                        if valid_name(presets, index, &p.name) {
                            let value = AdvancedPowerPlanTuningPreset {
                                name: p.name.trim().to_string(),
                                values: p.values.normalized(),
                            };
                            match index {
                                Some(i) => {
                                    let Some(existing) = presets.get_mut(i) else {
                                        return;
                                    };
                                    *existing = value;
                                }
                                None => presets.push(value),
                            }
                            self.preset = None;
                        }
                    }
                }
            }
            Message::ClosePreset => self.preset = None,
            Message::Remove(i) => {
                if i < presets.len() {
                    presets.remove(i);
                    self.preset = None;
                }
            }
        }
    }
    pub(super) fn view<'a>(
        &'a self,
        presets: &'a [AdvancedPowerPlanTuningPreset],
        plans: &[PowerPlan],
        mode: EffectivePowerMode,
    ) -> Element<'a, Message> {
        let choices = plans
            .iter()
            .map(|p| PlanChoice(p.guid.clone(), p.display_name()))
            .collect::<Vec<_>>();
        let selected = choices
            .iter()
            .find(|p| Some(&p.0) == self.target.as_ref())
            .cloned();
        let mut body = column![
            super::widgets::settings_card(
                row![
                    text(t!("processor_power.target_plan").to_string()).width(Fill),
                    pick_list(choices, selected, |p| Message::Plan(p.0))
                ]
                .spacing(design::space::MEDIUM)
                .align_y(iced::Center)
            ),
            text(
                t!(
                    "processor_power.effective_power_mode",
                    mode = mode_label(mode)
                )
                .to_string()
            )
        ]
        .spacing(super::widgets::CARD_GAP);
        if let Some(plan) = plans.iter().find(|p| Some(&p.guid) == self.target.as_ref()) {
            if !plan.active {
                body = body.push(text(
                    t!(
                        "processor_power.target_plan_inactive",
                        plan = plans
                            .iter()
                            .find(|p| p.active)
                            .map(|p| p.display_name())
                            .unwrap_or_else(|| t!("processor_power.no_active_plan").to_string())
                    )
                    .to_string(),
                ));
            } else if self.personality == Some(PowerPlanPersonality::Balanced)
                && !matches!(
                    mode,
                    EffectivePowerMode::Unknown | EffectivePowerMode::Balanced
                )
            {
                body = body.push(text(t!("processor_power.overlay_warning").to_string()));
            }
        }
        if let Some(values) = self.values {
            for (source, title, values) in [
                (Source::Ac, "processor_power.ac_preset", values.ac),
                (
                    Source::Battery,
                    "processor_power.battery_preset",
                    values.battery,
                ),
            ] {
                let mut options = BUILT_INS
                    .into_iter()
                    .map(|p| PresetChoice(preset_label(p), ProcessorPowerValues::for_preset(p)))
                    .collect::<Vec<_>>();
                options.extend(
                    presets
                        .iter()
                        .map(|p| PresetChoice(p.name.clone(), p.values.normalized())),
                );
                let selected = options
                    .iter()
                    .rev()
                    .find(|p| p.1 == values.normalized())
                    .cloned();
                let action = pick_list(options, selected, move |p| Message::Load(source, p.1))
                    .placeholder(t!("common.custom").to_string())
                    .width(280);
                let mut controls = column![].spacing(design::space::SMALL);
                for (field, label, value) in fields(values) {
                    controls = controls.push(
                        row![
                            text(t!(label).to_string()).width(Fill),
                            slider(0..=100, value, move |v| Message::Value(source, field, v))
                                .width(180),
                            super::widgets::stepper(
                                &value.to_string(),
                                0..=100,
                                1,
                                "%",
                                Some(move |v| Message::ValueText(source, field, v))
                            )
                        ]
                        .spacing(design::space::SMALL)
                        .height(46)
                        .align_y(iced::Center),
                    );
                }
                controls = controls.push(
                    row![
                        text(t!("processor_power.boost_mode").to_string()).width(Fill),
                        pick_list(
                            ProcessorBoostMode::ALL.map(BoostChoice),
                            Some(BoostChoice(values.boost_mode)),
                            move |v| Message::Boost(source, v.0)
                        )
                        .width(280)
                    ]
                    .spacing(design::space::SMALL),
                );
                body = body.push(super::widgets::setting_group(
                    title.to_string(),
                    !self.collapsed[source as usize],
                    Message::Group(source),
                    action,
                    controls,
                ));
            }
        }
        body = body
            .push(super::widgets::settings_card(
                row![
                    button(text(t!("processor_power.refresh_values").to_string()))
                        .on_press_maybe(self.target.as_ref().map(|_| Message::Refresh)),
                    button(text(t!("settings.apply").to_string())).on_press_maybe(
                        (self.values.is_some() && self.dirty).then_some(Message::Apply)
                    )
                ]
                .spacing(design::space::SMALL)
                .align_y(iced::Center),
            ))
            .push(text(&self.status));
        if let Some(p) = &self.preset {
            let editable = matches!(p.target, PresetTarget::Custom(_));
            let mut form = column![text_input(&t!("processor_power.preset_name"), &p.name)
                .on_input_maybe(editable.then_some(Message::PresetName))]
            .spacing(design::space::SMALL);
            for (field, label, value) in fields(p.values) {
                form = form.push(
                    row![
                        text(t!(label).to_string()).width(180),
                        text_input("", &value.to_string())
                            .on_input_maybe(
                                editable.then_some(move |v| Message::PresetValueText(field, v))
                            )
                            .width(design::NUMERIC_WIDTH)
                    ]
                    .spacing(design::space::SMALL),
                );
            }
            form = if editable {
                form.push(pick_list(
                    ProcessorBoostMode::ALL.map(BoostChoice),
                    Some(BoostChoice(p.values.boost_mode)),
                    |v| Message::PresetBoost(v.0),
                ))
            } else {
                form.push(text(format!(
                    "{}: {}",
                    t!("processor_power.boost_mode"),
                    BoostChoice(p.values.boost_mode)
                )))
            };
            if let PresetTarget::Custom(index) = p.target {
                form = form.push(
                    text(t!("processor_power.preset_name_help").to_string())
                        .width(Fill)
                        .style(text::secondary),
                );
                if !p.name.trim().is_empty() && !valid_name(presets, index, &p.name) {
                    form = form.push(text(
                        t!("processor_power.duplicate_preset_name").to_string(),
                    ));
                }
                form = form.push(button(text(t!("common.save").to_string())).on_press_maybe(
                    valid_name(presets, index, &p.name).then_some(Message::SavePreset),
                ));
            }
            form = form
                .push(button(text(t!("common.cancel").to_string())).on_press(Message::ClosePreset));
            body = body.push(form);
        }
        scrollable(body).height(Fill).width(Fill).into()
    }
    pub(super) fn side_panel<'a>(
        &'a self,
        presets: &'a [AdvancedPowerPlanTuningPreset],
    ) -> Element<'a, Message> {
        let mut rail = column![
            text(t!("processor_power.presets").to_string()).size(design::typography::SUBTITLE),
            text(t!("processor_power.built_in_presets").to_string())
        ]
        .spacing(design::space::SMALL);
        for p in BUILT_INS {
            rail = rail.push(
                button(text(preset_label(p)))
                    .width(Fill)
                    .style(super::widgets::quiet)
                    .on_press(Message::OpenPreset(PresetTarget::BuiltIn(p))),
            );
        }
        rail = rail.push(text(t!("processor_power.custom_presets").to_string()));
        let mut preset_rows = Vec::new();
        for (i, p) in presets.iter().enumerate() {
            let card = row![
                button(text(p.name.clone()))
                    .on_press(Message::OpenPreset(PresetTarget::Custom(Some(i)))),
                button(text(t!("common.remove").to_string())).on_press(Message::Remove(i))
            ]
            .spacing(design::space::SMALL);
            preset_rows.push((
                super::widgets::stable_key(&p.name),
                super::widgets::settings_card(card).into(),
            ));
        }
        rail = rail.push(iced::widget::keyed_column(preset_rows).spacing(super::widgets::CARD_GAP));
        rail = rail.push(
            button(text(t!("processor_power.add_preset").to_string()))
                .on_press(Message::OpenPreset(PresetTarget::Custom(None))),
        );

        scrollable(rail).height(Fill).width(Fill).into()
    }
}
const BUILT_INS: [ProcessorPowerPreset; 3] = [
    ProcessorPowerPreset::Performance,
    ProcessorPowerPreset::Balanced,
    ProcessorPowerPreset::Saver,
];
fn source_values(
    values: &mut ProcessorPowerSourceValues,
    source: Source,
) -> &mut ProcessorPowerValues {
    match source {
        Source::Ac => &mut values.ac,
        Source::Battery => &mut values.battery,
    }
}
fn fields(v: ProcessorPowerValues) -> [(Field, &'static str, u32); 4] {
    [
        (
            Field::Parking,
            "processor_power.core_parking_min",
            v.core_parking_min,
        ),
        (
            Field::Minimum,
            "processor_power.processor_min",
            v.performance_min,
        ),
        (
            Field::Maximum,
            "processor_power.processor_max",
            v.performance_max,
        ),
        (Field::Boost, "processor_power.boost_policy", v.boost_policy),
    ]
}
fn set_field(values: &mut ProcessorPowerValues, field: Field, value: u32) {
    *match field {
        Field::Parking => &mut values.core_parking_min,
        Field::Minimum => &mut values.performance_min,
        Field::Maximum => &mut values.performance_max,
        Field::Boost => &mut values.boost_policy,
    } = value.min(100);
}
fn valid_name(presets: &[AdvancedPowerPlanTuningPreset], index: Option<usize>, name: &str) -> bool {
    !name.trim().is_empty()
        && !presets
            .iter()
            .enumerate()
            .any(|(i, p)| Some(i) != index && p.name.trim().eq_ignore_ascii_case(name.trim()))
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct PlanChoice(String, String);
impl std::fmt::Display for PlanChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.1)
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
struct PresetChoice(String, ProcessorPowerValues);
impl std::fmt::Display for PresetChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct BoostChoice(ProcessorBoostMode);
impl std::fmt::Display for BoostChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let key = match self.0 {
            ProcessorBoostMode::Disabled => "disabled",
            ProcessorBoostMode::Enabled => "enabled",
            ProcessorBoostMode::Aggressive => "aggressive",
            ProcessorBoostMode::EfficientEnabled => "efficient_enabled",
            ProcessorBoostMode::EfficientAggressive => "efficient_aggressive",
            ProcessorBoostMode::AggressiveAtGuaranteed => "aggressive_at_guaranteed",
            ProcessorBoostMode::EfficientAggressiveAtGuaranteed => {
                "efficient_aggressive_at_guaranteed"
            }
        };
        f.write_str(&{
            let locale_key = format!("processor_power.boost_{key}");
            t!(&locale_key).to_string()
        })
    }
}
fn preset_label(p: ProcessorPowerPreset) -> String {
    t!(match p {
        ProcessorPowerPreset::Performance => "processor_power.performance",
        ProcessorPowerPreset::Balanced => "processor_power.balanced",
        ProcessorPowerPreset::Saver => "processor_power.saver",
    })
    .to_string()
}
fn mode_label(mode: EffectivePowerMode) -> String {
    let key = match mode {
        EffectivePowerMode::Unknown => "unknown",
        EffectivePowerMode::BatterySaver => "battery_saver",
        EffectivePowerMode::BetterBattery => "better_battery",
        EffectivePowerMode::Balanced => "balanced",
        EffectivePowerMode::HighPerformance => "high_performance",
        EffectivePowerMode::MaxPerformance => "max_performance",
        EffectivePowerMode::GameMode => "game_mode",
        EffectivePowerMode::MixedReality => "mixed_reality",
    };
    {
        let locale_key = format!("processor_power.mode_{key}");
        t!(&locale_key).to_string()
    }
    .to_string()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn presets_are_unique_readonly_and_change_only_requested_source() {
        let mut e = Editor::default();
        let mut presets = vec![];
        let balanced = ProcessorPowerValues::for_preset(ProcessorPowerPreset::Balanced);
        e.values = Some(ProcessorPowerSourceValues::same(balanced));
        e.update(
            &mut presets,
            &[],
            Message::Load(
                Source::Ac,
                ProcessorPowerValues::for_preset(ProcessorPowerPreset::Saver),
            ),
        );
        assert_eq!(e.values.unwrap().battery, balanced);
        e.update(
            &mut presets,
            &[],
            Message::OpenPreset(PresetTarget::Custom(None)),
        );
        e.update(&mut presets, &[], Message::PresetName(" Custom ".into()));
        e.update(&mut presets, &[], Message::SavePreset);
        assert_eq!(presets[0].name, "Custom");
        assert!(!valid_name(&presets, None, "CUSTOM"));
        e.update(
            &mut presets,
            &[],
            Message::OpenPreset(PresetTarget::BuiltIn(ProcessorPowerPreset::Balanced)),
        );
        e.update(&mut presets, &[], Message::PresetValue(Field::Parking, 99));
        assert_eq!(e.preset.as_ref().unwrap().values, balanced);
    }
}
