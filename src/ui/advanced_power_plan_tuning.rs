use super::widgets::{self, Ui};
use crate::application::AdvancedPowerPlanTuningService;
use crate::config::AdvancedPowerPlanTuningPreset;
use crate::power::{
    EffectivePowerMode, PowerPlan, PowerPlanPersonality, ProcessorBoostMode, ProcessorPowerPreset,
    ProcessorPowerSourceValues, ProcessorPowerValues,
};
use rust_i18n::t;
use std::collections::HashMap;
use windui::prelude::{signal, Element};

#[derive(Default)]
pub(super) struct Editor {
    service: AdvancedPowerPlanTuningService,
    target: Option<String>,
    values: Option<ProcessorPowerSourceValues>,
    personality: Option<PowerPlanPersonality>,
    pub(super) dirty: bool,
    pub(super) status: String,
    preset: Option<PresetEditor>,
    expanded: [bool; 2],
    selected_presets: [Option<PresetChoice>; 2],
    numbers: HashMap<(Source, Field), String>,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Source {
    Ac,
    Battery,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
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
    numbers: HashMap<Field, String>,
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
    Load(Source, PresetChoice),
    OpenPreset(PresetTarget),
    PresetName(String),
    PresetValueText(Field, String),
    PresetBoost(ProcessorBoostMode),
    SavePreset,
    ClosePreset,
    Remove(usize),
}
impl Editor {
    fn selected_preset(
        &self,
        source: Source,
        values: ProcessorPowerValues,
        options: &[PresetChoice],
    ) -> Option<PresetChoice> {
        self.selected_presets[source as usize]
            .as_ref()
            .filter(|choice| choice.1 == values.normalized() && options.contains(choice))
            .or_else(|| {
                options
                    .iter()
                    .rev()
                    .find(|choice| choice.1 == values.normalized())
            })
            .cloned()
    }

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
            let target = plans
                .iter()
                .find(|p| p.active)
                .or_else(|| plans.first())
                .map(|p| p.guid.clone());
            self.select_plan(target);
        } else if self.values.is_none() {
            self.refresh();
        }
    }
    fn select_plan(&mut self, target: Option<String>) {
        self.target = target;
        self.values = None;
        self.personality = None;
        self.selected_presets = [None, None];
        self.numbers.clear();
        self.refresh();
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
                self.numbers.clear();
                self.status = t!("processor_power.loaded_values", plan = guid).to_string();
            }
            Err(error) => {
                self.status = error.to_string();
            }
        }
    }
    pub(super) fn apply(&mut self) -> bool {
        if !self.numbers.values().all(|raw| valid_percent(raw)) {
            self.status = "0..100 (%)".into();
            return false;
        }
        let (Some(guid), Some(values)) = (&self.target, self.values) else {
            self.status = t!("processor_power.no_active_plan").to_string();
            return false;
        };
        let outcome = self.service.apply_values(guid, values);
        if let Some(actual) = outcome.actual_values {
            self.values = Some(actual);
            self.dirty = !outcome.applied;
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
                self.expanded[source as usize] = !self.expanded[source as usize];
            }
            Message::Plan(guid) => {
                if self.target.as_ref() == Some(&guid) {
                    return;
                }
                if self.dirty {
                    self.status = t!("processor_power.save_before_changing_plan").to_string();
                } else if plans.iter().any(|p| p.guid.eq_ignore_ascii_case(&guid)) {
                    self.select_plan(Some(guid));
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
                self.numbers.insert((source, field), value);
                self.dirty = true;
            }
            Message::Boost(source, value) => {
                if let Some(values) = self.values.as_mut() {
                    source_values(values, source).boost_mode = value;
                    self.dirty = true;
                }
            }
            Message::Load(source, choice) => {
                if let Some(values) = self.values.as_mut() {
                    self.numbers.retain(|(owner, _), _| *owner != source);
                    *source_values(values, source) = choice.1.normalized();
                    self.selected_presets[source as usize] = Some(choice);
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
                    numbers: HashMap::new(),
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
            Message::PresetValueText(field, v) => {
                if let Some(preset) = self
                    .preset
                    .as_mut()
                    .filter(|preset| matches!(preset.target, PresetTarget::Custom(_)))
                {
                    if let Ok(value) = v.parse::<u32>() {
                        if value <= 100 {
                            set_field(&mut preset.values, field, value);
                        }
                    }
                    preset.numbers.insert(field, v);
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
                        if valid_name(presets, index, &p.name)
                            && p.numbers.values().all(|raw| valid_percent(raw))
                        {
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
    pub(super) fn view(
        &self,
        presets: &[AdvancedPowerPlanTuningPreset],
        plans: &[PowerPlan],
        mode: EffectivePowerMode,
        ui: Ui<Message>,
    ) -> Element {
        let numbers = signal(self.numbers.clone());
        let ui = ui.inspect(move |message: &Message, _refresh| {
            if let Message::ValueText(source, field, raw) = message {
                numbers.update(|numbers| {
                    numbers.insert((*source, *field), raw.clone());
                });
            }
        });
        let mut body = vec![
            widgets::setting_row(
                t!("processor_power.target_plan").to_string(),
                ui.required_plan(self.target.clone(), plans, Message::Plan),
            ),
            widgets::label(
                t!(
                    "processor_power.effective_power_mode",
                    mode = mode_label(mode)
                )
                .to_string(),
            ),
        ];
        if let Some(plan) = plans
            .iter()
            .find(|plan| Some(&plan.guid) == self.target.as_ref())
        {
            if !plan.active {
                body.push(widgets::label(
                    t!(
                        "processor_power.target_plan_inactive",
                        plan = plans
                            .iter()
                            .find(|plan| plan.active)
                            .map(|plan| plan.display_name())
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
                body.push(widgets::label(
                    t!("processor_power.overlay_warning").to_string(),
                ));
            }
        }
        let mut native_numbers = Vec::new();
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
                    .map(|preset| {
                        PresetChoice(
                            preset_label(preset),
                            ProcessorPowerValues::for_preset(preset),
                        )
                    })
                    .collect::<Vec<_>>();
                options.extend(
                    presets.iter().map(|preset| {
                        PresetChoice(preset.name.clone(), preset.values.normalized())
                    }),
                );
                let selected = self
                    .selected_preset(source, values, &options)
                    .unwrap_or_else(|| PresetChoice(t!("common.custom").to_string(), values));
                let mut controls = Vec::new();
                for (field, title, value) in fields(values) {
                    let raw = signal(
                        self.numbers
                            .get(&(source, field))
                            .cloned()
                            .unwrap_or_else(|| value.to_string()),
                    );
                    native_numbers.push((raw, raw.get()));
                    controls.push(widgets::setting_row(
                        format!("{} (%)", t!(title)),
                        ui.number(raw, 0, 100, 1, move |value| {
                            Message::ValueText(source, field, value)
                        }),
                    ));
                }
                controls.push(widgets::setting_row(
                    t!("processor_power.boost_mode").to_string(),
                    ui.select(
                        ProcessorBoostMode::ALL.map(BoostChoice),
                        BoostChoice(values.boost_mode),
                        move |value| Message::Boost(source, value.0),
                    ),
                ));
                body.push(widgets::card(
                    t!(title).to_string(),
                    [
                        ui.select(options, selected, move |value| Message::Load(source, value)),
                        ui.collapsible(
                            t!("common.control").to_string(),
                            self.expanded[source as usize],
                            widgets::col().children(controls),
                            move |_| Message::Group(source),
                        ),
                    ],
                ));
            }
        }
        body.push(Element::label(numbers.map(|numbers| {
            if numbers.values().all(|raw| valid_percent(raw)) {
                String::new()
            } else {
                "0..100 (%)".into()
            }
        })));
        let loaded = self.values.is_some();
        let already_dirty = self.dirty;
        body.push(
            widgets::row().children([
                ui.button(
                    t!("processor_power.refresh_values").to_string(),
                    Message::Refresh,
                )
                .enabled(self.target.is_some()),
                ui.button(t!("settings.apply").to_string(), Message::Apply)
                    .enabled_when(move || {
                        loaded
                            && (already_dirty
                                || native_numbers
                                    .iter()
                                    .any(|(raw, original)| raw.get() != *original))
                            && native_numbers
                                .iter()
                                .all(|(raw, _)| valid_percent(&raw.get()))
                    }),
            ]),
        );
        body.push(widgets::label(self.status.clone()));
        widgets::page(body)
    }

    pub(super) fn preset_modal(
        &self,
        presets: &[AdvancedPowerPlanTuningPreset],
        ui: Ui<Message>,
    ) -> Option<Element> {
        let preset = self.preset.as_ref()?;
        let editable = matches!(preset.target, PresetTarget::Custom(_));
        let title = match preset.target {
            PresetTarget::BuiltIn(_) => preset.name.clone(),
            PresetTarget::Custom(None) => t!("processor_power.add_preset").to_string(),
            PresetTarget::Custom(Some(_)) => t!("common.edit").to_string(),
        };
        let name = signal(preset.name.clone());
        let numbers = signal(preset.numbers.clone());
        let ui = ui.inspect(move |message: &Message, _refresh| {
            if let Message::PresetValueText(field, raw) = message {
                numbers.update(|numbers| {
                    numbers.insert(*field, raw.clone());
                });
            }
        });
        let mut form = vec![
            widgets::setting_row(
                t!("processor_power.preset_name").to_string(),
                ui.input_signal(
                    name,
                    t!("processor_power.preset_name_placeholder").to_string(),
                    Message::PresetName,
                )
                .enabled(editable),
            ),
            widgets::help("processor_power.preset_name_help"),
        ];
        let mut native_numbers = Vec::new();
        for (field, title, value) in fields(preset.values) {
            let raw = signal(
                preset
                    .numbers
                    .get(&field)
                    .cloned()
                    .unwrap_or_else(|| value.to_string()),
            );
            native_numbers.push(raw);
            form.push(widgets::setting_row(
                format!("{} (%)", t!(title)),
                ui.number(raw, 0, 100, 1, move |value| {
                    Message::PresetValueText(field, value)
                })
                .enabled(editable),
            ));
        }
        form.push(widgets::setting_row(
            t!("processor_power.boost_mode").to_string(),
            ui.select(
                ProcessorBoostMode::ALL.map(BoostChoice),
                BoostChoice(preset.values.boost_mode),
                |value| Message::PresetBoost(value.0),
            )
            .enabled(editable),
        ));
        let mut actions = Vec::new();
        if let PresetTarget::Custom(index) = preset.target {
            let presets = presets.to_vec();
            let validation_presets = presets.clone();
            form.push(Element::label(name.map(move |name| {
                if !name.trim().is_empty() && !valid_name(&validation_presets, index, name) {
                    t!("processor_power.duplicate_preset_name").to_string()
                } else {
                    String::new()
                }
            })));
            form.push(Element::label(numbers.map(|numbers| {
                if numbers.values().all(|raw| valid_percent(raw)) {
                    String::new()
                } else {
                    "0..100 (%)".into()
                }
            })));
            actions.push(
                ui.button(t!("common.save").to_string(), Message::SavePreset)
                    .enabled_when(move || {
                        valid_name(&presets, index, &name.get())
                            && native_numbers.iter().all(|raw| valid_percent(&raw.get()))
                    }),
            );
        }
        actions.push(ui.button(t!("common.cancel").to_string(), Message::ClosePreset));
        Some(widgets::editor_panel(
            title,
            widgets::col().children(form),
            widgets::row().children(actions),
        ))
    }

    pub(super) fn side_panel(
        &self,
        presets: &[AdvancedPowerPlanTuningPreset],
        ui: Ui<Message>,
    ) -> Element {
        let mut body = vec![widgets::heading(
            t!("processor_power.built_in_presets").to_string(),
        )];
        for preset in BUILT_INS {
            body.push(ui.button(
                preset_label(preset),
                Message::OpenPreset(PresetTarget::BuiltIn(preset)),
            ));
        }
        body.push(widgets::heading(
            t!("processor_power.custom_presets").to_string(),
        ));
        for (index, preset) in presets.iter().enumerate() {
            body.push(widgets::row().children([
                ui.button(
                    preset.name.clone(),
                    Message::OpenPreset(PresetTarget::Custom(Some(index))),
                ),
                ui.button(t!("common.remove").to_string(), Message::Remove(index)),
            ]));
        }
        body.push(ui.button(
            t!("processor_power.add_preset").to_string(),
            Message::OpenPreset(PresetTarget::Custom(None)),
        ));
        widgets::page(body)
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
fn valid_percent(raw: &str) -> bool {
    raw.parse::<u32>().is_ok_and(|value| value <= 100)
}
fn valid_name(presets: &[AdvancedPowerPlanTuningPreset], index: Option<usize>, name: &str) -> bool {
    !name.trim().is_empty()
        && !presets
            .iter()
            .enumerate()
            .any(|(i, p)| Some(i) != index && p.name.trim().eq_ignore_ascii_case(name.trim()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct PresetChoice(String, ProcessorPowerValues);
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
    fn retarget_read_failure_cannot_reuse_previous_plan_values() {
        let values = ProcessorPowerValues::for_preset(ProcessorPowerPreset::Performance);
        let replacement = PowerPlan {
            guid: "invalid-replacement-guid".into(),
            name: "Replacement".into(),
            active: true,
        };
        for automatic in [true, false] {
            let mut editor = Editor {
                target: Some("previous-plan".into()),
                values: Some(ProcessorPowerSourceValues::same(values)),
                personality: Some(PowerPlanPersonality::HighPerformance),
                selected_presets: [Some(PresetChoice("Old selection".into(), values)), None],
                ..Default::default()
            };
            let plans = [replacement.clone()];
            if automatic {
                editor.ensure_plan(&plans);
            } else {
                editor.update(&mut vec![], &plans, Message::Plan(replacement.guid.clone()));
            }
            assert_eq!(editor.target.as_deref(), Some(replacement.guid.as_str()));
            assert!(editor.values.is_none());
            assert!(editor.personality.is_none());
            assert_eq!(editor.selected_presets, [None, None]);
            editor.update(
                &mut vec![],
                &plans,
                Message::Value(Source::Ac, Field::Minimum, 25),
            );
            assert!(!editor.dirty);
            assert!(!editor.apply());
        }
    }

    #[test]
    fn identical_presets_keep_explicit_selection_for_each_source() {
        let mut editor = Editor::default();
        let values = ProcessorPowerValues::for_preset(ProcessorPowerPreset::Balanced);
        editor.values = Some(ProcessorPowerSourceValues::same(values));
        let options = vec![
            PresetChoice("Balanced".into(), values),
            PresetChoice("Mine".into(), values),
        ];
        editor.update(
            &mut vec![],
            &[],
            Message::Load(Source::Ac, options[0].clone()),
        );
        editor.update(
            &mut vec![],
            &[],
            Message::Load(Source::Battery, options[1].clone()),
        );
        assert_eq!(
            editor.selected_preset(Source::Ac, values, &options),
            Some(options[0].clone())
        );
        assert_eq!(
            editor.selected_preset(Source::Battery, values, &options),
            Some(options[1].clone())
        );
        editor.update(
            &mut vec![],
            &[],
            Message::Load(Source::Ac, options[1].clone()),
        );
        editor.discard_editor();
        assert_eq!(
            editor.selected_preset(Source::Ac, values, &options),
            Some(options[1].clone())
        );
        assert_eq!(
            editor.selected_preset(Source::Ac, values, &options[..1]),
            Some(options[0].clone())
        );
        let changed = ProcessorPowerValues::for_preset(ProcessorPowerPreset::Saver);
        assert_eq!(editor.selected_preset(Source::Ac, changed, &options), None);
    }

    #[test]
    fn invalid_native_numeric_drafts_block_apply_and_preset_publication() {
        let balanced = ProcessorPowerValues::for_preset(ProcessorPowerPreset::Balanced);
        let mut editor = Editor {
            target: Some("unused-test-plan".into()),
            values: Some(ProcessorPowerSourceValues::same(balanced)),
            ..Editor::default()
        };
        let mut presets = Vec::new();
        editor.update(
            &mut presets,
            &[],
            Message::ValueText(Source::Ac, Field::Parking, "invalid".into()),
        );
        assert_eq!(editor.values.unwrap().ac, balanced);
        assert!(editor.dirty);
        assert!(!editor.apply());
        assert_eq!(editor.numbers[&(Source::Ac, Field::Parking)], "invalid");

        editor.update(
            &mut presets,
            &[],
            Message::OpenPreset(PresetTarget::Custom(None)),
        );
        editor.update(&mut presets, &[], Message::PresetName("Custom".into()));
        editor.update(
            &mut presets,
            &[],
            Message::PresetValueText(Field::Parking, "101".into()),
        );
        editor.update(&mut presets, &[], Message::SavePreset);
        assert!(editor.has_pending_editor());
        assert!(presets.is_empty());
        editor.update(
            &mut presets,
            &[],
            Message::PresetValueText(Field::Parking, "35".into()),
        );
        editor.update(&mut presets, &[], Message::SavePreset);
        assert!(!editor.has_pending_editor());
        assert_eq!(presets[0].values.core_parking_min, 35);
    }

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
                PresetChoice(
                    "Saver".into(),
                    ProcessorPowerValues::for_preset(ProcessorPowerPreset::Saver),
                ),
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
        e.update(
            &mut presets,
            &[],
            Message::PresetValueText(Field::Parking, "99".into()),
        );
        assert_eq!(e.preset.as_ref().unwrap().values, balanced);
    }
}
