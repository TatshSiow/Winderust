use super::design;
use super::widgets::{
    action_button, button, icon_button, pick_list, power_plan_label, slider, text_input,
};
use crate::application::{
    AdvancedPowerPlanTuningApplyError, AdvancedPowerPlanTuningError,
    AdvancedPowerPlanTuningService, SettingsEditor,
};
use crate::config::AdvancedPowerPlanTuningPreset;
use crate::power::{
    AdvancedPowerPlanTuningReadback, AdvancedPowerPlanTuningSourceValues,
    AdvancedPowerPlanTuningValues, EditableTuningValues, EffectivePowerMode, PowerPlan,
    PowerPlanPersonality, PowerSettingReadback, ProcessorBoostMode, ProcessorPowerPreset,
    ProcessorPowerValues, ProcessorSchedulingPolicy,
};
use crate::ui::scrolling::scrollable;
use iced::widget::{column, row, text};
use iced::{Element, Fill};
use rust_i18n::t;
use std::collections::BTreeMap;

fn tuning_plans(plans: &[PowerPlan]) -> impl Iterator<Item = &PowerPlan> {
    plans.iter().filter(|plan| !plan.managed_adaptive)
}

#[derive(Default)]
pub(super) struct Editor {
    service: AdvancedPowerPlanTuningService,
    target: Option<String>,
    readback: Option<AdvancedPowerPlanTuningReadback>,
    values: Option<AdvancedPowerPlanTuningSourceValues>,
    invalid_percentages: [InvalidPercentages; 2],
    personality: Option<PowerPlanPersonality>,
    pub(super) dirty: bool,
    pub(super) status: String,
    preset: Option<PresetEditor>,
    review: Option<ApplyReview>,
    restore_points: BTreeMap<String, RestorePoint>,
    expanded: [bool; 2],
    selected_presets: [Option<PresetChoice>; 2],
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Source {
    Ac,
    Battery,
}

impl Source {
    fn label(self) -> &'static str {
        match self {
            Self::Ac => "power_source.plugged_in",
            Self::Battery => "power_source.on_battery",
        }
    }
}
#[derive(Debug, Clone, Copy)]
pub(super) enum Field {
    Parking,
    Minimum,
    Maximum,
    Boost,
}

#[derive(Default)]
struct InvalidPercentages([Option<String>; 4]);

impl InvalidPercentages {
    fn edit(&mut self, field: Field, raw: String) -> Option<u32> {
        let value = raw.parse::<u32>().ok().filter(|value| *value <= 100);
        self.0[field as usize] = value.is_none().then_some(raw);
        value
    }

    fn text(&self, field: Field, value: u32) -> String {
        self.0[field as usize]
            .clone()
            .unwrap_or_else(|| value.to_string())
    }

    fn any(&self) -> bool {
        self.0.iter().any(Option::is_some)
    }

    fn feedback(&self, field: Field) -> Option<iced::widget::Text<'static>> {
        self.0[field as usize]
            .as_ref()
            .map(|_| text(t!("processor_power.invalid_percentage").to_string()).style(text::danger))
    }
}
#[derive(Debug, Clone, Copy)]
pub(super) enum SchedulingField {
    LongRunning,
    ShortRunning,
}
#[derive(Debug, Clone, Copy)]
pub(super) enum PresetTarget {
    BuiltIn(ProcessorPowerPreset),
    Custom(Option<usize>),
}
struct PresetEditor {
    target: PresetTarget,
    name: String,
    values: AdvancedPowerPlanTuningValues,
    invalid_percentages: InvalidPercentages,
}

impl PresetEditor {
    fn has_inverted_range(&self) -> bool {
        self.values.processor.performance_max < self.values.processor.performance_min
    }

    fn can_save(&self, presets: &[AdvancedPowerPlanTuningPreset], index: Option<usize>) -> bool {
        valid_name(presets, index, &self.name)
            && !self.invalid_percentages.any()
            && !self.has_inverted_range()
    }

    fn feedback(&self, field: Field) -> Option<iced::widget::Text<'static>> {
        self.invalid_percentages.feedback(field).or_else(|| {
            (matches!(field, Field::Maximum)
                && self.invalid_percentages.0[Field::Minimum as usize].is_none()
                && self.has_inverted_range())
            .then(|| {
                text(
                    t!(
                        "processor_power.invalid_preset_processor_max",
                        minimum = self.values.processor.performance_min
                    )
                    .to_string(),
                )
                .style(text::danger)
            })
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ApplyOrigin {
    Page,
    Save,
    Restore,
}

#[derive(Clone, Copy)]
struct RestorePoint {
    previous: AdvancedPowerPlanTuningReadback,
    applied: AdvancedPowerPlanTuningReadback,
}

impl RestorePoint {
    fn capture(
        previous: AdvancedPowerPlanTuningReadback,
        applied: AdvancedPowerPlanTuningReadback,
    ) -> Option<Self> {
        let values = previous.writable();
        fn can_restore<T: Copy + Eq>(
            before: PowerSettingReadback<T>,
            after: PowerSettingReadback<T>,
        ) -> bool {
            before == after || (before.known().is_some() && after.known().is_some())
        }
        (values == values.normalized()
            && [
                (previous.ac, applied.ac),
                (previous.battery, applied.battery),
            ]
            .into_iter()
            .all(|(before, after)| {
                can_restore(before.processor.boost_mode, after.processor.boost_mode)
                    && can_restore(before.scheduling_policy, after.scheduling_policy)
                    && can_restore(
                        before.short_scheduling_policy,
                        after.short_scheduling_policy,
                    )
            }))
        .then_some(Self { previous, applied })
    }
}

struct ApplyReview {
    guid: String,
    name: String,
    current: AdvancedPowerPlanTuningReadback,
    requested: AdvancedPowerPlanTuningSourceValues,
    origin: ApplyOrigin,
}

impl ApplyReview {
    fn request(
        current: AdvancedPowerPlanTuningReadback,
        draft: AdvancedPowerPlanTuningSourceValues,
    ) -> AdvancedPowerPlanTuningSourceValues {
        let current = current.writable();
        AdvancedPowerPlanTuningSourceValues {
            ac: with_supported_settings(draft.ac, current.ac),
            battery: with_supported_settings(draft.battery, current.battery),
        }
        .normalized()
    }

    fn changes(&self, source: Source) -> impl Iterator<Item = (&'static str, String, String)> {
        readback_changes(
            self.current,
            self.current.with_requested(self.requested),
            source,
        )
    }
}

#[derive(Debug, Clone)]
pub(super) enum Message {
    Plan(String),
    Group(Source),
    Refresh,
    ReviewApply,
    ReviewRestore,
    ConfirmApply,
    CancelApply,
    Value(Source, Field, u32),
    ValueText(Source, Field, String),
    Boost(Source, ProcessorBoostMode),
    Scheduling(Source, SchedulingField, ProcessorSchedulingPolicy),
    Load(Source, PresetChoice),
    OpenPreset(PresetTarget),
    PresetName(String),
    PresetValueText(Field, String),
    PresetBoost(ProcessorBoostMode),
    PresetScheduling(SchedulingField, Option<ProcessorSchedulingPolicy>),
    SavePreset,
    ClosePreset,
    Remove(usize),
}
impl Editor {
    fn selected_preset(
        &self,
        source: Source,
        values: EditableTuningValues,
        options: &[PresetChoice],
    ) -> Option<PresetChoice> {
        self.selected_presets[source as usize]
            .as_ref()
            .filter(|choice| preset_matches(choice.1, values) && options.contains(choice))
            .or_else(|| {
                options
                    .iter()
                    .rev()
                    .find(|choice| preset_matches(choice.1, values))
            })
            .cloned()
    }

    pub(super) fn has_pending_editor(&self) -> bool {
        self.review.is_some()
            || self
                .preset
                .as_ref()
                .is_some_and(|p| matches!(p.target, PresetTarget::Custom(_)))
    }
    pub(super) fn discard_editor(&mut self) {
        self.preset = None;
        self.cancel_apply_review();
        self.invalid_percentages = Default::default();
    }

    pub(super) fn has_invalid_inputs(&self) -> bool {
        self.invalid_percentages.iter().any(InvalidPercentages::any)
    }

    pub(super) fn ensure_plan(&mut self, plans: &[PowerPlan]) {
        if self.dirty || self.has_invalid_inputs() {
            return;
        }
        if self.selected_plan(plans).is_none() {
            let target = tuning_plans(plans)
                .find(|p| p.active)
                .or_else(|| tuning_plans(plans).next())
                .map(|p| p.guid.clone());
            self.select_plan(target);
        } else if self.values.is_none() {
            self.read_selected_plan();
        }
    }

    pub(super) fn selected_plan<'a>(&self, plans: &'a [PowerPlan]) -> Option<&'a PowerPlan> {
        let guid = self.target.as_deref()?;
        tuning_plans(plans).find(|plan| plan.guid.eq_ignore_ascii_case(guid))
    }

    fn select_plan(&mut self, target: Option<String>) {
        self.target = target;
        self.personality = None;
        self.selected_presets = [None, None];
        self.read_selected_plan();
    }

    pub(super) fn refresh(&mut self, plans: &[PowerPlan]) {
        self.dirty = false;
        self.invalid_percentages = Default::default();
        if self.selected_plan(plans).is_some() {
            self.read_selected_plan();
        } else {
            self.ensure_plan(plans);
        }
    }

    fn read_selected_plan(&mut self) {
        self.cancel_apply_review();
        let Some(guid) = &self.target else {
            self.set_readback(None);
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
                self.status = t!("processor_power.loaded_values", plan = guid).to_string();
                self.set_readback(Some(v));
                self.dirty = false;
            }
            Err(error) => {
                self.set_readback(None);
                self.dirty = false;
                self.status = error.to_string();
            }
        }
    }

    fn set_readback(&mut self, readback: Option<AdvancedPowerPlanTuningReadback>) {
        self.invalid_percentages = Default::default();
        self.readback = readback;
        self.values = readback.map(AdvancedPowerPlanTuningReadback::writable);
    }

    pub(super) fn review_apply(
        &mut self,
        plans: &[PowerPlan],
        origin: ApplyOrigin,
    ) -> Result<(), String> {
        if self.has_invalid_inputs() {
            return Err(t!("processor_power.invalid_percentage").to_string());
        }
        if self.review.is_some() || !self.dirty {
            return Ok(());
        }
        let (Some(guid), Some(values)) = (&self.target, self.values) else {
            return Err(t!("processor_power.no_active_plan").to_string());
        };
        let plan = self
            .selected_plan(plans)
            .ok_or_else(|| t!("common.selected_plan_unavailable").to_string())?;
        let current = self
            .service
            .read_values(guid)
            .map_err(|error| error.to_string())?;
        self.begin_apply_review(ApplyReview {
            guid: guid.clone(),
            name: plan.name.clone(),
            current,
            requested: ApplyReview::request(current, values),
            origin,
        })
    }

    fn begin_apply_review(&mut self, review: ApplyReview) -> Result<(), String> {
        if self.readback != Some(review.current) {
            return Err(t!("processor_power.review_refresh_required").to_string());
        }
        self.review = Some(review);
        Ok(())
    }

    pub(super) fn review_origin(&self) -> Option<ApplyOrigin> {
        self.review.as_ref().map(|review| review.origin)
    }

    pub(super) fn cancel_apply_review(&mut self) {
        self.review = None;
    }

    fn restore_point_for<'a>(
        &self,
        plans: &'a [PowerPlan],
    ) -> Option<(&'a PowerPlan, RestorePoint)> {
        if self.dirty || self.has_invalid_inputs() || self.review.is_some() {
            return None;
        }
        let plan = self.selected_plan(plans)?;
        let point = *self.restore_points.get(&plan.guid)?;
        (self.readback == Some(point.applied)).then_some((plan, point))
    }

    fn review_restore(&mut self, plans: &[PowerPlan]) -> Result<(), String> {
        let Some((plan, point)) = self.restore_point_for(plans) else {
            return Ok(());
        };
        let current = self
            .service
            .read_values(&plan.guid)
            .map_err(|error| error.to_string())?;
        if current != point.applied {
            self.set_readback(Some(current));
            return Err(t!("processor_power.restore_values_changed").to_string());
        }
        self.review = Some(ApplyReview {
            guid: plan.guid.clone(),
            name: plan.name.clone(),
            current,
            requested: point.previous.writable(),
            origin: ApplyOrigin::Restore,
        });
        Ok(())
    }

    fn remember_verified_write(
        &mut self,
        review: &ApplyReview,
        actual: AdvancedPowerPlanTuningReadback,
    ) -> bool {
        if review.origin == ApplyOrigin::Restore {
            if !self
                .restore_points
                .get(&review.guid)
                .is_some_and(|point| actual == point.previous)
            {
                return false;
            }
            self.restore_points.remove(&review.guid);
        } else if actual != review.current {
            match RestorePoint::capture(review.current, actual) {
                Some(point) => {
                    self.restore_points.insert(review.guid.clone(), point);
                }
                None => {
                    self.restore_points.remove(&review.guid);
                }
            }
        }
        true
    }

    pub(super) fn confirm_apply(&mut self, plans: &[PowerPlan]) -> bool {
        if self.has_invalid_inputs() {
            self.cancel_apply_review();
            self.status = t!("processor_power.invalid_percentage").to_string();
            return false;
        }
        let Some(review) = self.review.take() else {
            return false;
        };
        let restoring = review.origin == ApplyOrigin::Restore;
        let draft_matches = if restoring {
            !self.dirty
                && self.readback == Some(review.current)
                && self.restore_points.get(&review.guid).is_some_and(|point| {
                    point.applied == review.current && point.previous.writable() == review.requested
                })
        } else {
            self.values.is_some_and(|draft| {
                ApplyReview::request(review.current, draft) == review.requested
            })
        };
        if self.target.as_ref() != Some(&review.guid) || !draft_matches {
            self.status = t!("processor_power.review_stale").to_string();
            return false;
        }
        if self.selected_plan(plans).is_none() {
            self.status = t!("common.selected_plan_unavailable").to_string();
            return false;
        }
        let outcome =
            match self
                .service
                .apply_reviewed_values(&review.guid, review.current, review.requested)
            {
                Ok(outcome) => outcome,
                Err(AdvancedPowerPlanTuningError::ValuesChanged) => {
                    self.status = if restoring {
                        self.set_readback(self.service.read_values(&review.guid).ok());
                        t!("processor_power.restore_values_changed").to_string()
                    } else {
                        t!("processor_power.review_refresh_required").to_string()
                    };
                    return false;
                }
                Err(error) => {
                    self.status = error.to_string();
                    return false;
                }
            };
        self.set_readback(outcome.actual_values);
        self.dirty = !restoring && self.values.is_some() && !outcome.applied;
        let mismatch_summary = if restoring {
            "processor_power.restore_not_verified"
        } else {
            "processor_power.apply_not_verified"
        };
        let error = match (outcome.error, self.readback) {
            (Some(AdvancedPowerPlanTuningApplyError::Verification), _) => {
                Some(t!(mismatch_summary).to_string())
            }
            (Some(error), _) => Some(error.to_string()),
            (None, Some(actual)) if self.remember_verified_write(&review, actual) => None,
            (None, _) => Some(t!(mismatch_summary).to_string()),
        };
        if let Some(mut message) = error {
            let expected = if restoring {
                self.restore_points
                    .get(&review.guid)
                    .map(|point| point.previous)
            } else {
                self.readback
                    .map(|actual| actual.with_requested(review.requested))
            };
            if let Some((expected, actual)) = expected.zip(self.readback) {
                append_readback_differences(&mut message, expected, actual);
            }
            self.status = message;
            return false;
        }
        self.status = if restoring {
            t!("processor_power.restored_previous", plan = &review.name).to_string()
        } else {
            t!("processor_power.applied_custom", plan = &review.guid).to_string()
        };
        true
    }
    pub(super) fn update(
        &mut self,
        settings: &mut SettingsEditor,
        plans: &[PowerPlan],
        message: Message,
    ) {
        match message {
            Message::Group(source) => {
                self.expanded[source as usize] = !self.expanded[source as usize];
            }
            Message::Plan(guid) => {
                if self
                    .target
                    .as_ref()
                    .is_some_and(|target| target.eq_ignore_ascii_case(&guid))
                {
                    return;
                }
                if self.dirty || self.has_invalid_inputs() {
                    self.status = t!("processor_power.save_before_changing_plan").to_string();
                } else if let Some(plan) =
                    tuning_plans(plans).find(|plan| plan.guid.eq_ignore_ascii_case(&guid))
                {
                    self.select_plan(Some(plan.guid.clone()));
                }
            }
            Message::Refresh => self.refresh(plans),
            Message::ReviewApply => {
                if let Err(error) = self.review_apply(plans, ApplyOrigin::Page) {
                    self.status = error;
                }
            }
            Message::ReviewRestore => {
                if let Err(error) = self.review_restore(plans) {
                    self.status = error;
                }
            }
            Message::ConfirmApply => {
                self.confirm_apply(plans);
            }
            Message::CancelApply => self.cancel_apply_review(),
            Message::Value(source, field, value) => {
                if let Some(values) = self.values.as_mut() {
                    self.invalid_percentages[source as usize].0[field as usize] = None;
                    set_field(source_values(values, source), field, value);
                    self.dirty = true;
                }
            }
            Message::ValueText(source, field, value) => {
                if self.values.is_some() {
                    if let Some(value) =
                        self.invalid_percentages[source as usize].edit(field, value)
                    {
                        self.update(settings, plans, Message::Value(source, field, value));
                    }
                }
            }
            Message::Boost(source, value) => {
                if let Some(values) = self.values.as_mut() {
                    if let Some(current) =
                        source_values(values, source).processor.boost_mode.as_mut()
                    {
                        *current = value;
                        self.dirty = true;
                    }
                }
            }
            Message::Load(source, choice) => {
                if let Some(values) = self.values.as_mut() {
                    self.invalid_percentages[source as usize] = Default::default();
                    let current = source_values(values, source);
                    let preset = choice.1.normalized();
                    *current = with_supported_settings(
                        preset.with_boost_mode(Some(preset.processor.boost_mode)),
                        *current,
                    );
                    self.selected_presets[source as usize] = Some(choice);
                    self.dirty = true;
                }
            }
            Message::Scheduling(source, field, value) => {
                if let Some(values) = self.values.as_mut() {
                    if let Some(current) =
                        scheduling_field(source_values(values, source), field).as_mut()
                    {
                        *current = value;
                        self.dirty = true;
                    }
                }
            }
            Message::OpenPreset(target) => {
                let (name, values) = match target {
                    PresetTarget::BuiltIn(p) => (
                        preset_label(p),
                        AdvancedPowerPlanTuningValues::for_preset(p),
                    ),
                    PresetTarget::Custom(Some(i)) => {
                        let Some(p) = settings.global().advanced_power_plan_tuning_presets.get(i)
                        else {
                            return;
                        };
                        (p.name.clone(), p.values.normalized())
                    }
                    PresetTarget::Custom(None) => {
                        let balanced = AdvancedPowerPlanTuningValues::for_preset(
                            ProcessorPowerPreset::Balanced,
                        );
                        let values = self.values.map_or(balanced, |values| {
                            values.ac.with_boost_mode(
                                values
                                    .ac
                                    .processor
                                    .boost_mode
                                    .unwrap_or(balanced.processor.boost_mode),
                            )
                        });
                        (String::new(), values)
                    }
                };
                self.preset = Some(PresetEditor {
                    target,
                    name,
                    values,
                    invalid_percentages: Default::default(),
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
            Message::PresetValueText(field, raw) => {
                if let Some(p) = self
                    .preset
                    .as_mut()
                    .filter(|p| matches!(p.target, PresetTarget::Custom(_)))
                {
                    if let Some(value) = p.invalid_percentages.edit(field, raw) {
                        set_field(&mut p.values, field, value);
                    }
                }
            }
            Message::PresetBoost(v) => {
                if let Some(p) = self
                    .preset
                    .as_mut()
                    .filter(|p| matches!(p.target, PresetTarget::Custom(_)))
                {
                    p.values.processor.boost_mode = v;
                }
            }
            Message::PresetScheduling(field, value) => {
                if let Some(preset) = self
                    .preset
                    .as_mut()
                    .filter(|p| matches!(p.target, PresetTarget::Custom(_)))
                {
                    *scheduling_field(&mut preset.values, field) = value;
                }
            }
            Message::SavePreset => {
                let Some(p) = &self.preset else {
                    return;
                };
                let PresetTarget::Custom(index) = p.target else {
                    return;
                };
                let presets = &settings.global().advanced_power_plan_tuning_presets;
                if !p.can_save(presets, index) || index.is_some_and(|i| i >= presets.len()) {
                    return;
                }
                let value = AdvancedPowerPlanTuningPreset {
                    name: p.name.trim().to_string(),
                    values: p.values.normalized(),
                };
                settings.edit_global(|settings| {
                    let presets = &mut settings.advanced_power_plan_tuning_presets;
                    match index {
                        Some(i) => presets[i] = value,
                        None => presets.push(value),
                    }
                });
                self.preset = None;
            }
            Message::ClosePreset => self.preset = None,
            Message::Remove(i) => {
                if i < settings.global().advanced_power_plan_tuning_presets.len() {
                    settings.edit_global(|settings| {
                        settings.advanced_power_plan_tuning_presets.remove(i);
                    });
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
        let choices = tuning_plans(plans)
            .map(|p| PlanChoice(p.guid.clone(), power_plan_label(p)))
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
        if let Some(plan) = self.selected_plan(plans) {
            if !plan.active {
                body = body.push(text(
                    t!(
                        "processor_power.target_plan_inactive",
                        plan = plans
                            .iter()
                            .find(|p| p.active)
                            .map(power_plan_label)
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
        if let (Some(values), Some(readback)) = (self.values, self.readback) {
            for (source, title, values, readback) in [
                (
                    Source::Ac,
                    "processor_power.ac_preset",
                    values.ac,
                    readback.ac,
                ),
                (
                    Source::Battery,
                    "processor_power.battery_preset",
                    values.battery,
                    readback.battery,
                ),
            ] {
                let mut options = BUILT_INS
                    .into_iter()
                    .map(|p| {
                        PresetChoice(
                            preset_label(p),
                            AdvancedPowerPlanTuningValues::for_preset(p),
                        )
                    })
                    .collect::<Vec<_>>();
                options.extend(
                    presets
                        .iter()
                        .map(|p| PresetChoice(p.name.clone(), p.values.normalized())),
                );
                let selected = self.selected_preset(source, values, &options);
                let action = pick_list(options, selected, move |p| Message::Load(source, p))
                    .placeholder(t!("common.custom").to_string())
                    .width(280);
                let mut controls = Vec::new();
                for (field, label, value) in fields(values.processor) {
                    controls.push(
                        column![row![
                            text(super::widgets::label_with_unit(&t!(label), "%")).width(Fill),
                            slider(0..=100, value, move |v| Message::Value(source, field, v))
                                .width(180),
                            super::widgets::stepper(
                                &self.invalid_percentages[source as usize].text(field, value),
                                0..=100,
                                1,
                                Some(move |v| Message::ValueText(source, field, v))
                            )
                        ]
                        .spacing(design::space::SMALL)
                        .height(46)
                        .align_y(iced::Center)]
                        .spacing(design::space::SMALL)
                        .extend(
                            self.invalid_percentages[source as usize]
                                .feedback(field)
                                .map(Element::from),
                        )
                        .into(),
                    );
                }
                let boost = tuning_choice(
                    readback.processor.boost_mode,
                    values.processor.boost_mode,
                    |mode| {
                        pick_list(
                            ProcessorBoostMode::ALL.map(BoostChoice),
                            Some(BoostChoice(mode)),
                            move |v| Message::Boost(source, v.0),
                        )
                        .width(280)
                        .into()
                    },
                );
                controls
                    .push(super::widgets::setting_row("processor_power.boost_mode", boost).into());
                for (field, label, policy) in scheduling_fields(values) {
                    let status = match field {
                        SchedulingField::LongRunning => readback.scheduling_policy,
                        SchedulingField::ShortRunning => readback.short_scheduling_policy,
                    };
                    let control = tuning_choice(status, policy, |policy| {
                        pick_list(
                            ProcessorSchedulingPolicy::ALL.map(|v| SchedulingChoice(Some(v))),
                            Some(SchedulingChoice(Some(policy))),
                            move |v| {
                                Message::Scheduling(
                                    source,
                                    field,
                                    v.0.expect("available scheduling choices contain a policy"),
                                )
                            },
                        )
                        .width(280)
                        .into()
                    });
                    controls.push(super::widgets::setting_row(label, control).into());
                }
                body = body.push(super::widgets::setting_group(
                    title.to_string(),
                    self.expanded[source as usize],
                    Message::Group(source),
                    action,
                    super::widgets::settings_rows(controls),
                ));
            }
        }
        let mut actions = row![
            action_button(t!("processor_power.refresh_values").to_string())
                .on_press_maybe(self.target.as_ref().map(|_| Message::Refresh)),
            action_button(t!("settings.apply").to_string())
                .style(super::widgets::primary_button)
                .on_press_maybe(
                    (self.values.is_some()
                        && self.dirty
                        && !self.has_invalid_inputs()
                        && self.selected_plan(plans).is_some())
                    .then_some(Message::ReviewApply)
                )
        ]
        .spacing(design::space::SMALL)
        .align_y(iced::Center);
        if self.restore_point_for(plans).is_some() {
            actions = actions.push(
                action_button(t!("processor_power.restore_previous").to_string())
                    .on_press(Message::ReviewRestore),
            );
        }
        body = body
            .push(super::widgets::settings_card(actions))
            .push(text(&self.status).wrapping(text::Wrapping::WordOrGlyph));
        scrollable(body).height(Fill).width(Fill).into()
    }
    pub(super) fn apply_review_modal(&self) -> Option<Element<'_, Message>> {
        use super::widgets;
        let review = self.review.as_ref()?;
        let review_row = |label: &str, value: String| {
            row![
                widgets::heading(t!(label).to_string(), design::typography::BODY).width(Fill),
                text(value)
                    .width(iced::Length::FillPortion(2))
                    .align_x(iced::Right)
                    .wrapping(text::Wrapping::WordOrGlyph)
            ]
            .spacing(design::space::MEDIUM)
            .align_y(iced::Center)
        };
        let mut body = column![widgets::settings_card(review_row(
            "processor_power.target_plan",
            review.name.clone()
        ))]
        .spacing(widgets::CARD_GAP);
        let mut has_changes = false;
        for source in [Source::Ac, Source::Battery] {
            let rows = review
                .changes(source)
                .map(|(label, before, after)| {
                    review_row(label, format!("{before} → {after}")).into()
                })
                .collect::<Vec<Element<'_, Message>>>();
            if !rows.is_empty() {
                has_changes = true;
                body = body
                    .push(widgets::heading(
                        t!(source.label()).to_string(),
                        design::typography::BODY,
                    ))
                    .push(widgets::settings_card(widgets::settings_rows(rows)));
            }
        }
        if !has_changes {
            body = body
                .push(text(t!("processor_power.no_changes").to_string()).style(text::secondary));
        }
        let (title, action) = if review.origin == ApplyOrigin::Restore {
            (
                t!("processor_power.restore_previous"),
                t!("processor_power.restore"),
            )
        } else {
            (t!("processor_power.review_changes"), t!("settings.apply"))
        };
        Some(widgets::modal_frame(
            row![
                widgets::heading(title.to_string(), design::typography::DIALOG_TITLE).width(Fill),
                icon_button("icons/x.svg").on_press(Message::CancelApply)
            ]
            .align_y(iced::Center),
            scrollable(body).height(Fill),
            row![
                action_button(t!("common.cancel").to_string())
                    .style(widgets::tertiary_button)
                    .on_press(Message::CancelApply),
                action_button(action.to_string())
                    .style(widgets::primary_button)
                    .on_press(Message::ConfirmApply)
            ]
            .spacing(design::space::SMALL),
            (720, 620),
        ))
    }

    pub(super) fn preset_modal<'a>(
        &'a self,
        presets: &'a [AdvancedPowerPlanTuningPreset],
    ) -> Option<Element<'a, Message>> {
        use super::widgets;
        let p = self.preset.as_ref()?;
        let editable = matches!(p.target, PresetTarget::Custom(_));
        let title = match p.target {
            PresetTarget::BuiltIn(_) => p.name.clone(),
            PresetTarget::Custom(None) => t!("processor_power.add_preset").to_string(),
            PresetTarget::Custom(Some(_)) => t!("common.edit").to_string(),
        };
        let header = row![
            text(title).width(Fill),
            icon_button("icons/x.svg").on_press(Message::ClosePreset)
        ]
        .align_y(iced::Center);
        let mut form = column![
            widgets::heading(
                t!("common.preset_details").to_string(),
                design::typography::BODY
            ),
            widgets::settings_card(
                column![
                    text(t!("processor_power.preset_name").to_string()),
                    text_input(&t!("processor_power.preset_name_placeholder"), &p.name)
                        .on_input_maybe(editable.then_some(Message::PresetName)),
                    text(t!("processor_power.preset_name_help").to_string()).style(text::secondary)
                ]
                .spacing(design::space::SMALL)
            )
        ]
        .spacing(widgets::CARD_GAP);
        let mut values = Vec::new();
        for (field, label, value) in fields(p.values.processor) {
            values.push(
                column![widgets::setting_row_with_unit(
                    label,
                    "%",
                    widgets::stepper(
                        &p.invalid_percentages.text(field, value),
                        0..=100,
                        1,
                        editable.then_some(move |v| Message::PresetValueText(field, v)),
                    ),
                )]
                .spacing(design::space::SMALL)
                .extend(p.feedback(field).map(Element::from))
                .into(),
            );
        }
        let boost: Element<'_, Message> = if editable {
            pick_list(
                ProcessorBoostMode::ALL.map(BoostChoice),
                Some(BoostChoice(p.values.processor.boost_mode)),
                |v| Message::PresetBoost(v.0),
            )
            .width(design::SELECT_WIDTH)
            .into()
        } else {
            text(BoostChoice(p.values.processor.boost_mode).to_string()).into()
        };
        values.push(widgets::setting_row("processor_power.boost_mode", boost).into());
        for (field, label, policy) in scheduling_fields(p.values) {
            let control: Element<'_, Message> = if editable {
                let options = std::iter::once(SchedulingChoice(None))
                    .chain(ProcessorSchedulingPolicy::ALL.map(|v| SchedulingChoice(Some(v))))
                    .collect::<Vec<_>>();
                pick_list(options, Some(SchedulingChoice(policy)), move |v| {
                    Message::PresetScheduling(field, v.0)
                })
                .width(design::SELECT_WIDTH)
                .into()
            } else {
                text(SchedulingChoice(policy).to_string()).into()
            };
            values.push(widgets::setting_row(label, control).into());
        }
        form = form
            .push(widgets::heading(
                t!("processor_power.processor_settings").to_string(),
                design::typography::BODY,
            ))
            .push(widgets::settings_card(widgets::settings_rows(values)));
        let mut footer = row![
            iced::widget::Space::new().width(Fill),
            action_button(
                t!(if editable {
                    "common.cancel"
                } else {
                    "common.close"
                })
                .to_string(),
            )
            .style(widgets::tertiary_button)
            .on_press(Message::ClosePreset)
        ]
        .spacing(design::space::SMALL);
        if let PresetTarget::Custom(index) = p.target {
            if !p.name.trim().is_empty() && !valid_name(presets, index, &p.name) {
                form = form.push(
                    text(t!("processor_power.duplicate_preset_name").to_string())
                        .style(text::danger),
                );
            }
            footer = footer.push(
                action_button(t!("common.save").to_string())
                    .style(widgets::primary_button)
                    .on_press_maybe(p.can_save(presets, index).then_some(Message::SavePreset)),
            );
        }
        Some(widgets::modal_frame(
            header,
            scrollable(form).height(Fill),
            footer,
            (800, 680),
        ))
    }
    pub(super) fn presets_panel<'a>(
        &'a self,
        presets: &'a [AdvancedPowerPlanTuningPreset],
    ) -> Element<'a, Message> {
        let mut rail = column![super::widgets::heading(
            t!("processor_power.built_in_presets").to_string(),
            design::typography::SECONDARY
        )]
        .spacing(design::space::MEDIUM)
        .padding([0, design::space::MEDIUM as u16]);
        for p in BUILT_INS {
            rail = rail.push(
                row![
                    button(
                        iced::widget::container(text(preset_label(p)))
                            .center_y(design::NAVIGATION_ROW_HEIGHT - 10)
                    )
                    .width(Fill)
                    .style(super::widgets::quiet)
                    .on_press(Message::OpenPreset(PresetTarget::BuiltIn(p))),
                    icon_button("icons/info.svg")
                        .on_press(Message::OpenPreset(PresetTarget::BuiltIn(p)))
                ]
                .spacing(design::space::TIGHT)
                .align_y(iced::Center),
            );
        }
        rail = rail.push(super::widgets::heading(
            t!("processor_power.custom_presets").to_string(),
            design::typography::SECONDARY,
        ));
        for (i, p) in presets.iter().enumerate() {
            rail = rail.push(
                row![
                    button(text(p.name.clone()))
                        .width(Fill)
                        .style(super::widgets::quiet)
                        .on_press(Message::OpenPreset(PresetTarget::Custom(Some(i)))),
                    super::widgets::tooltip(
                        icon_button("icons/pencil.svg")
                            .on_press(Message::OpenPreset(PresetTarget::Custom(Some(i)))),
                        text(t!("common.edit").to_string()),
                        iced::widget::tooltip::Position::Top
                    ),
                    super::widgets::rule_delete_button(Some(Message::Remove(i)))
                ]
                .spacing(design::space::TIGHT)
                .align_y(iced::Center),
            );
        }
        column![
            scrollable(rail).height(Fill).width(Fill),
            super::widgets::preset_footer(
                t!("processor_power.add_preset").to_string(),
                Message::OpenPreset(PresetTarget::Custom(None))
            )
        ]
        .spacing(design::space::MEDIUM)
        .height(Fill)
        .into()
    }
}
const BUILT_INS: [ProcessorPowerPreset; 3] = [
    ProcessorPowerPreset::Performance,
    ProcessorPowerPreset::Balanced,
    ProcessorPowerPreset::Saver,
];
fn source_values(
    values: &mut AdvancedPowerPlanTuningSourceValues,
    source: Source,
) -> &mut EditableTuningValues {
    match source {
        Source::Ac => &mut values.ac,
        Source::Battery => &mut values.battery,
    }
}
fn preset_matches(preset: AdvancedPowerPlanTuningValues, actual: EditableTuningValues) -> bool {
    let preset = preset.normalized();
    let boost_mode = actual
        .processor
        .boost_mode
        .map(|_| preset.processor.boost_mode);
    preset.processor.with_boost_mode(boost_mode) == actual.processor
        && preset
            .scheduling_policy
            .is_none_or(|policy| Some(policy) == actual.scheduling_policy)
        && preset
            .short_scheduling_policy
            .is_none_or(|policy| Some(policy) == actual.short_scheduling_policy)
}

fn with_supported_settings(
    mut draft: EditableTuningValues,
    current: EditableTuningValues,
) -> EditableTuningValues {
    draft.processor.boost_mode = current
        .processor
        .boost_mode
        .map(|mode| draft.processor.boost_mode.unwrap_or(mode));
    draft.scheduling_policy = current
        .scheduling_policy
        .map(|value| draft.scheduling_policy.unwrap_or(value));
    draft.short_scheduling_policy = current
        .short_scheduling_policy
        .map(|value| draft.short_scheduling_policy.unwrap_or(value));
    draft
}

fn scheduling_fields<B>(
    values: AdvancedPowerPlanTuningValues<B>,
) -> [(
    SchedulingField,
    &'static str,
    Option<ProcessorSchedulingPolicy>,
); 2] {
    [
        (
            SchedulingField::LongRunning,
            "processor_power.scheduling_policy",
            values.scheduling_policy,
        ),
        (
            SchedulingField::ShortRunning,
            "processor_power.short_scheduling_policy",
            values.short_scheduling_policy,
        ),
    ]
}

fn scheduling_field<B>(
    values: &mut AdvancedPowerPlanTuningValues<B>,
    field: SchedulingField,
) -> &mut Option<ProcessorSchedulingPolicy> {
    match field {
        SchedulingField::LongRunning => &mut values.scheduling_policy,
        SchedulingField::ShortRunning => &mut values.short_scheduling_policy,
    }
}

fn tuning_choice<'a, T>(
    readback: PowerSettingReadback<T>,
    draft: Option<T>,
    pick: impl FnOnce(T) -> Element<'a, Message>,
) -> Element<'a, Message> {
    show_setting_readback(
        readback,
        |value| pick(draft.unwrap_or(value)),
        |status| text(status).style(text::secondary).into(),
    )
}

fn show_setting_readback<T, R>(
    readback: PowerSettingReadback<T>,
    known: impl FnOnce(T) -> R,
    unavailable: impl FnOnce(String) -> R,
) -> R {
    let status = match readback {
        PowerSettingReadback::Known(value) => return known(value),
        PowerSettingReadback::Missing => t!("processor_power.setting_missing"),
        PowerSettingReadback::Unreadable => t!("processor_power.setting_unreadable"),
        PowerSettingReadback::Unknown(value) => {
            t!("processor_power.setting_unknown", value = value)
        }
    };
    unavailable(status.to_string())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SchedulingChoice(Option<ProcessorSchedulingPolicy>);
impl std::fmt::Display for SchedulingChoice {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&t!(match self.0 {
            None => "processor_power.keep_scheduling_policy",
            Some(ProcessorSchedulingPolicy::AllProcessors) => "processor_power.schedule_all",
            Some(ProcessorSchedulingPolicy::PerformanceProcessors) =>
                "processor_power.schedule_performance",
            Some(ProcessorSchedulingPolicy::PreferPerformanceProcessors) =>
                "processor_power.schedule_prefer_performance",
            Some(ProcessorSchedulingPolicy::EfficiencyProcessors) =>
                "processor_power.schedule_efficiency",
            Some(ProcessorSchedulingPolicy::PreferEfficiencyProcessors) =>
                "processor_power.schedule_prefer_efficiency",
            Some(ProcessorSchedulingPolicy::Automatic) => "processor_power.schedule_automatic",
        }))
    }
}

fn fields<B>(v: ProcessorPowerValues<B>) -> [(Field, &'static str, u32); 4] {
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

fn readback_changes(
    before: AdvancedPowerPlanTuningReadback,
    after: AdvancedPowerPlanTuningReadback,
    source: Source,
) -> impl Iterator<Item = (&'static str, String, String)> {
    let (before, after) = match source {
        Source::Ac => (before.ac, after.ac),
        Source::Battery => (before.battery, after.battery),
    };
    let percentages = fields(before.processor)
        .into_iter()
        .zip(fields(after.processor))
        .filter(|((_, _, before), (_, _, after))| before != after)
        .map(|((_, label, before), (_, _, after))| {
            (label, format!("{before}%"), format!("{after}%"))
        });
    let scheduling_label = |policy| SchedulingChoice(Some(policy)).to_string();
    percentages.chain(
        [
            readback_setting_change(
                "processor_power.boost_mode",
                before.processor.boost_mode,
                after.processor.boost_mode,
                |mode| BoostChoice(mode).to_string(),
            ),
            readback_setting_change(
                "processor_power.scheduling_policy",
                before.scheduling_policy,
                after.scheduling_policy,
                scheduling_label,
            ),
            readback_setting_change(
                "processor_power.short_scheduling_policy",
                before.short_scheduling_policy,
                after.short_scheduling_policy,
                scheduling_label,
            ),
        ]
        .into_iter()
        .flatten(),
    )
}

fn readback_setting_change<T: Eq>(
    label: &'static str,
    before: PowerSettingReadback<T>,
    after: PowerSettingReadback<T>,
    known: impl Fn(T) -> String,
) -> Option<(&'static str, String, String)> {
    (before != after).then(|| {
        let value = |status| show_setting_readback(status, &known, std::convert::identity);
        (label, value(before), value(after))
    })
}

fn append_readback_differences(
    message: &mut String,
    expected: AdvancedPowerPlanTuningReadback,
    actual: AdvancedPowerPlanTuningReadback,
) {
    for source in [Source::Ac, Source::Battery] {
        for (setting, requested, actual) in readback_changes(expected, actual, source) {
            message.push('\n');
            message.push_str(&t!(
                "processor_power.readback_difference",
                source = t!(source.label()),
                setting = t!(setting),
                requested = requested,
                actual = actual
            ));
        }
    }
}
fn set_field<B>(values: &mut AdvancedPowerPlanTuningValues<B>, field: Field, value: u32) {
    let values = &mut values.processor;
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
pub(super) struct PresetChoice(String, AdvancedPowerPlanTuningValues);
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

    fn test_settings() -> SettingsEditor {
        SettingsEditor::with_settings(crate::config::Settings::default())
    }

    fn saved_presets(settings: &SettingsEditor) -> &[AdvancedPowerPlanTuningPreset] {
        &settings.global().advanced_power_plan_tuning_presets
    }

    #[test]
    fn invalid_percentages_block_review_and_confirmation_until_resolved_or_reset() {
        let values = AdvancedPowerPlanTuningValues::for_preset(ProcessorPowerPreset::Balanced);
        let current: AdvancedPowerPlanTuningReadback =
            AdvancedPowerPlanTuningSourceValues::same(values).into();
        let mut editor = Editor {
            target: Some("missing-plan".into()),
            readback: Some(current),
            values: Some(current.writable()),
            ..Default::default()
        };
        let mut settings = test_settings();
        editor.update(
            &mut settings,
            &[],
            Message::ValueText(Source::Ac, Field::Parking, "".into()),
        );
        editor.update(
            &mut settings,
            &[],
            Message::ValueText(Source::Battery, Field::Boost, "101".into()),
        );
        editor.ensure_plan(&[]);
        assert!(
            editor.has_invalid_inputs(),
            "navigation must preserve the draft"
        );
        assert!(!editor.dirty);
        assert_eq!(editor.values, Some(current.writable()));
        assert_eq!(editor.invalid_percentages[0].text(Field::Parking, 100), "");
        let error = t!("processor_power.invalid_percentage").to_string();
        assert_eq!(
            editor.review_apply(&[], ApplyOrigin::Save),
            Err(error.clone())
        );
        editor.review = Some(ApplyReview {
            guid: "missing-plan".into(),
            name: "Fixture".into(),
            current,
            requested: current.writable(),
            origin: ApplyOrigin::Save,
        });
        assert!(!editor.confirm_apply(&[]));
        assert!(editor.review.is_none());
        assert_eq!(editor.status, error);
        editor.update(
            &mut settings,
            &[],
            Message::Value(Source::Ac, Field::Parking, 0),
        );
        assert!(!editor.invalid_percentages[0].any());
        assert!(editor.invalid_percentages[1].any());
        editor.update(
            &mut settings,
            &[],
            Message::Load(Source::Battery, PresetChoice("Balanced".into(), values)),
        );
        assert!(!editor.has_invalid_inputs());
        let invalidate = |editor: &mut Editor| {
            editor.update(
                &mut test_settings(),
                &[],
                Message::ValueText(Source::Ac, Field::Parking, "".into()),
            );
        };
        invalidate(&mut editor);
        editor.discard_editor();
        assert!(!editor.has_invalid_inputs());
        invalidate(&mut editor);
        editor.set_readback(Some(current));
        assert!(!editor.has_invalid_inputs());
        invalidate(&mut editor);
        editor.refresh(&[]);
        assert!(!editor.has_invalid_inputs());
        assert!(editor.values.is_none());
        assert!(editor.target.is_none());
    }

    #[test]
    fn invalid_preset_values_refuse_save_without_changing_the_page_draft() {
        let mut editor = Editor::default();
        let mut settings = test_settings();
        editor.update(
            &mut settings,
            &[],
            Message::OpenPreset(PresetTarget::Custom(None)),
        );
        editor.update(&mut settings, &[], Message::PresetName("Fixture".into()));
        editor.invalid_percentages[0].edit(Field::Parking, "".into());
        editor.update(
            &mut settings,
            &[],
            Message::PresetValueText(Field::Boost, "".into()),
        );
        editor.update(&mut settings, &[], Message::SavePreset);
        assert!(saved_presets(&settings).is_empty());
        assert!(editor.preset.is_some());
        for (field, value) in [
            (Field::Boost, "80"),
            (Field::Minimum, "25"),
            (Field::Maximum, "10"),
        ] {
            editor.update(
                &mut settings,
                &[],
                Message::PresetValueText(field, value.into()),
            );
        }
        editor.update(&mut settings, &[], Message::SavePreset);
        assert!(saved_presets(&settings).is_empty());
        assert_eq!(
            editor
                .preset
                .as_ref()
                .unwrap()
                .values
                .processor
                .performance_max,
            10
        );
        editor.update(
            &mut settings,
            &[],
            Message::PresetValueText(Field::Minimum, "10".into()),
        );
        editor.update(&mut settings, &[], Message::SavePreset);
        assert_eq!(
            saved_presets(&settings)[0].values.processor.boost_policy,
            80
        );
        assert_eq!(
            saved_presets(&settings)[0].values.processor.performance_min,
            10
        );
        assert_eq!(
            saved_presets(&settings)[0].values.processor.performance_max,
            10
        );
        assert!(
            editor.has_invalid_inputs(),
            "preset Save must leave the page draft alone"
        );
    }

    #[test]
    fn readback_details_include_requested_values_and_exact_restore_differences() {
        let values = AdvancedPowerPlanTuningSourceValues::same(
            AdvancedPowerPlanTuningValues::for_preset(ProcessorPowerPreset::Balanced),
        );
        let mut previous: AdvancedPowerPlanTuningReadback = values.into();
        previous.ac.scheduling_policy = PowerSettingReadback::Unknown(30);
        let mut actual = previous;
        actual.ac.scheduling_policy = PowerSettingReadback::Unknown(31);
        actual.battery.processor.boost_mode = PowerSettingReadback::Unreadable;

        let battery_difference = t!(
            "processor_power.readback_difference",
            source = t!("power_source.on_battery"),
            setting = t!("processor_power.boost_mode"),
            requested = BoostChoice(ProcessorBoostMode::Enabled).to_string(),
            actual = t!("processor_power.setting_unreadable"),
        );
        let mut apply = String::from("Write failed");
        append_readback_differences(&mut apply, actual.with_requested(values), actual);
        assert_eq!(apply, format!("Write failed\n{battery_difference}"));

        let ac_difference = t!(
            "processor_power.readback_difference",
            source = t!("power_source.plugged_in"),
            setting = t!("processor_power.scheduling_policy"),
            requested = t!("processor_power.setting_unknown", value = 30),
            actual = t!("processor_power.setting_unknown", value = 31),
        );
        let mut restore = t!("processor_power.restore_not_verified").to_string();
        append_readback_differences(&mut restore, previous, actual);
        assert_eq!(
            restore,
            format!(
                "{}\n{ac_difference}\n{battery_difference}",
                t!("processor_power.restore_not_verified")
            ),
        );
    }

    #[test]
    fn review_shows_only_normalized_changes_for_each_power_source() {
        let mut current = AdvancedPowerPlanTuningSourceValues::same(
            AdvancedPowerPlanTuningValues::for_preset(ProcessorPowerPreset::Balanced),
        );
        current.ac.processor.performance_max = 70;
        current.battery.short_scheduling_policy = Some(ProcessorSchedulingPolicy::Automatic);
        let mut draft = current;
        draft.ac.processor.performance_min = 80;
        draft.ac.scheduling_policy = Some(ProcessorSchedulingPolicy::Automatic);
        draft.battery.processor.boost_policy = 35;
        draft.battery.processor.boost_mode = Some(ProcessorBoostMode::Disabled);
        draft.battery.short_scheduling_policy = Some(ProcessorSchedulingPolicy::AllProcessors);
        let mut current: AdvancedPowerPlanTuningReadback = current.into();
        current.ac.scheduling_policy = PowerSettingReadback::Unknown(10);
        current.ac.processor.boost_mode = PowerSettingReadback::Unreadable;
        let review = ApplyReview {
            guid: "plan".into(),
            name: "Balanced".into(),
            current,
            requested: ApplyReview::request(current, draft),
            origin: ApplyOrigin::Page,
        };
        assert_eq!(
            review.changes(Source::Ac).collect::<Vec<_>>(),
            vec![
                ("processor_power.processor_min", "5%".into(), "80%".into()),
                ("processor_power.processor_max", "70%".into(), "80%".into())
            ]
        );
        assert_eq!(
            review
                .changes(Source::Battery)
                .map(|(label, _, _)| label)
                .collect::<Vec<_>>(),
            vec![
                "processor_power.boost_policy",
                "processor_power.boost_mode",
                "processor_power.short_scheduling_policy"
            ]
        );
    }

    #[test]
    fn stale_plan_readback_blocks_review_and_preserves_the_battery_draft() {
        let mut baseline: AdvancedPowerPlanTuningReadback =
            AdvancedPowerPlanTuningSourceValues::same(AdvancedPowerPlanTuningValues::for_preset(
                ProcessorPowerPreset::Balanced,
            ))
            .into();
        baseline.ac.scheduling_policy = PowerSettingReadback::Unknown(10);
        let mut draft = baseline.writable();
        draft.battery.processor.boost_policy = 35;
        let mut editor = Editor {
            target: Some("plan".into()),
            readback: Some(baseline),
            values: Some(draft),
            dirty: true,
            ..Default::default()
        };
        let review = |current, origin| ApplyReview {
            guid: "plan".into(),
            name: "Balanced".into(),
            current,
            requested: ApplyReview::request(current, draft),
            origin,
        };
        let mut changed_percentage = baseline;
        changed_percentage.ac.processor.core_parking_min = 75;
        let mut changed_unknown_policy = baseline;
        changed_unknown_policy.ac.scheduling_policy = PowerSettingReadback::Unknown(11);
        assert_eq!(baseline.writable(), changed_unknown_policy.writable());
        for (current, origin) in [
            (changed_percentage, ApplyOrigin::Page),
            (changed_unknown_policy, ApplyOrigin::Save),
        ] {
            assert_eq!(
                editor
                    .begin_apply_review(review(current, origin))
                    .unwrap_err(),
                t!("processor_power.review_refresh_required")
            );
            assert!(editor.review.is_none());
            assert!(editor.dirty);
            assert_eq!(editor.readback, Some(baseline));
            assert_eq!(editor.values, Some(draft));
            assert_eq!(editor.target.as_deref(), Some("plan"));
        }

        editor
            .begin_apply_review(review(baseline, ApplyOrigin::Save))
            .unwrap();
        let review = editor.review.unwrap();
        assert_eq!(review.origin, ApplyOrigin::Save);
        assert!(review.changes(Source::Ac).next().is_none());
        assert_eq!(
            review
                .changes(Source::Battery)
                .map(|(label, _, _)| label)
                .collect::<Vec<_>>(),
            ["processor_power.boost_policy"]
        );
    }

    #[test]
    fn apply_requires_current_review_and_cancel_keeps_draft() {
        let current = AdvancedPowerPlanTuningSourceValues::same(
            AdvancedPowerPlanTuningValues::for_preset(ProcessorPowerPreset::Balanced),
        );
        let mut draft = current;
        draft.ac.processor.performance_min = 35;
        let mut editor = Editor {
            target: Some("first-plan".into()),
            values: Some(draft),
            dirty: true,
            ..Default::default()
        };
        assert!(!editor.confirm_apply(&[]));
        let review = || ApplyReview {
            guid: "first-plan".into(),
            name: "Balanced".into(),
            current: current.into(),
            requested: draft,
            origin: ApplyOrigin::Save,
        };

        editor.review = Some(review());
        editor.update(&mut test_settings(), &[], Message::CancelApply);
        assert!(!editor.confirm_apply(&[]));
        assert_eq!(editor.values, Some(draft));
        assert!(editor.dirty);

        editor.review = Some(review());
        editor.target = Some("second-plan".into());
        assert!(!editor.confirm_apply(&[]));
        assert_eq!(editor.status, t!("processor_power.review_stale"));
        assert_eq!(editor.values, Some(draft));
        assert!(editor.dirty);

        editor.review = Some(review());
        editor.target = Some("first-plan".into());
        editor.values = Some(current);
        assert!(!editor.confirm_apply(&[]));
        assert_eq!(editor.status, t!("processor_power.review_stale"));
        assert_eq!(editor.values, Some(current));
    }

    #[test]
    fn temporary_adaptive_plan_is_unavailable_for_tuning_even_after_review() {
        let mut plans = [
            PowerPlan {
                guid: "adaptive".into(),
                name: "Winderust Adaptive".into(),
                active: true,
                managed_adaptive: true,
            },
            PowerPlan {
                guid: "saved-plan".into(),
                name: "Saved plan".into(),
                active: false,
                managed_adaptive: false,
            },
        ];
        let mut editor = Editor::default();
        editor.ensure_plan(&plans);
        assert_eq!(editor.selected_plan(&plans), Some(&plans[1]));
        editor.update(
            &mut test_settings(),
            &plans,
            Message::Plan(plans[0].guid.clone()),
        );
        assert_eq!(editor.selected_plan(&plans), Some(&plans[1]));

        let current = AdvancedPowerPlanTuningSourceValues::same(
            AdvancedPowerPlanTuningValues::for_preset(ProcessorPowerPreset::Balanced),
        );
        let mut draft = current;
        draft.ac.processor.core_parking_min = 25;
        let review = ApplyReview {
            guid: plans[1].guid.clone(),
            name: plans[1].name.clone(),
            current: current.into(),
            requested: draft,
            origin: ApplyOrigin::Save,
        };
        editor.dirty = true;
        editor.values = Some(draft);
        editor.review = Some(review);
        plans[1].managed_adaptive = true;
        editor.ensure_plan(&plans);
        assert!(!editor.confirm_apply(&plans));
        assert_eq!(editor.status, t!("common.selected_plan_unavailable"));
        assert!(editor.review.is_none());
        assert_eq!(editor.values, Some(draft));
        assert!(editor.dirty);

        editor.refresh(&plans);
        assert!(editor.target.is_none() && editor.values.is_none() && !editor.dirty);
    }

    #[test]
    fn restore_requires_the_selected_plan_and_exact_values() {
        let mut previous: AdvancedPowerPlanTuningReadback =
            AdvancedPowerPlanTuningSourceValues::same(AdvancedPowerPlanTuningValues::for_preset(
                ProcessorPowerPreset::Balanced,
            ))
            .into();
        previous.ac.short_scheduling_policy = PowerSettingReadback::Unknown(12);
        previous.ac.processor.boost_mode = PowerSettingReadback::Unknown(13);
        let mut applied = previous;
        applied.battery.processor.boost_mode =
            PowerSettingReadback::Known(ProcessorBoostMode::Disabled);
        let mut plans = ["first-plan", "second-plan"].map(|guid| PowerPlan {
            guid: guid.into(),
            name: guid.into(),
            active: false,
            managed_adaptive: false,
        });
        let mut editor = Editor {
            target: Some(plans[0].guid.clone()),
            ..Default::default()
        };
        editor.set_readback(Some(applied));
        let mut review = ApplyReview {
            guid: plans[0].guid.clone(),
            name: plans[0].name.clone(),
            current: previous,
            requested: applied.writable(),
            origin: ApplyOrigin::Page,
        };
        assert!(editor.remember_verified_write(&review, applied));

        // A no-op Apply and a canceled Restore both keep the last changed values.
        review.current = applied;
        assert!(editor.remember_verified_write(&review, applied));
        review.requested = previous.writable();
        review.origin = ApplyOrigin::Restore;
        editor.review = Some(review);
        assert!(editor.restore_point_for(&plans).is_none());
        editor.discard_editor();
        let (_, restore_point) = editor.restore_point_for(&plans).unwrap();
        assert_eq!(restore_point.previous, previous);

        editor.dirty = true;
        assert!(editor.restore_point_for(&plans).is_none());
        editor.dirty = false;
        editor.target = Some(plans[1].guid.clone());
        assert!(editor.restore_point_for(&plans).is_none());
        editor.target = Some(plans[0].guid.clone());
        assert!(editor.restore_point_for(&plans[1..]).is_none());
        plans[0].managed_adaptive = true;
        assert!(editor.restore_point_for(&plans).is_none());
        plans[0].managed_adaptive = false;

        let review = ApplyReview {
            guid: plans[0].guid.clone(),
            name: plans[0].name.clone(),
            current: applied,
            requested: previous.writable(),
            origin: ApplyOrigin::Restore,
        };
        let mut partial = previous;
        partial.ac.processor.boost_mode = PowerSettingReadback::Unknown(14);
        assert!(!editor.remember_verified_write(&review, partial));
        editor.set_readback(Some(partial));
        assert!(editor.restore_point_for(&plans).is_none());
        editor.set_readback(Some(applied));
        assert!(editor.restore_point_for(&plans).is_some());

        assert!(editor.remember_verified_write(&review, previous));
        assert!(editor.restore_points.is_empty());
    }

    #[test]
    fn restore_refuses_previous_values_the_writer_cannot_round_trip() {
        let mut previous = AdvancedPowerPlanTuningSourceValues::same(
            AdvancedPowerPlanTuningValues::for_preset(ProcessorPowerPreset::Balanced),
        );
        let mut applied = previous;
        applied.battery.scheduling_policy = Some(ProcessorSchedulingPolicy::Automatic);
        assert!(RestorePoint::capture(previous.into(), applied.into()).is_none());

        previous.ac.processor.performance_min = 75;
        previous.ac.processor.performance_max = 50;
        assert!(RestorePoint::capture(previous.into(), previous.normalized().into()).is_none());
    }

    #[test]
    fn preset_matching_does_not_normalize_actual_readback() {
        let mut preset = AdvancedPowerPlanTuningValues::for_preset(ProcessorPowerPreset::Balanced);
        preset.processor.performance_min = 75;
        preset.processor.performance_max = 75;
        let editable = preset.with_boost_mode(Some(preset.processor.boost_mode));
        let mut actual = editable;
        actual.processor.performance_max = 50;
        assert!(!preset_matches(preset, actual));
        assert!(preset_matches(preset, editable));
        assert!(preset_matches(preset, editable.with_boost_mode(None)));
    }

    #[test]
    fn scheduling_edits_and_presets_preserve_power_source_and_availability() {
        let mut values = AdvancedPowerPlanTuningValues::for_preset(ProcessorPowerPreset::Balanced);
        values.scheduling_policy = Some(ProcessorSchedulingPolicy::Automatic);
        values.short_scheduling_policy = Some(ProcessorSchedulingPolicy::AllProcessors);
        let mut loaded = AdvancedPowerPlanTuningSourceValues::same(values);
        loaded.ac.processor.boost_mode = None;
        let mut editor = Editor {
            values: Some(loaded),
            ..Default::default()
        };
        editor.update(
            &mut test_settings(),
            &[],
            Message::Boost(Source::Ac, ProcessorBoostMode::Disabled),
        );
        assert!(!editor.dirty);
        editor.update(
            &mut test_settings(),
            &[],
            Message::Scheduling(
                Source::Ac,
                SchedulingField::LongRunning,
                ProcessorSchedulingPolicy::PerformanceProcessors,
            ),
        );
        assert_eq!(editor.values.unwrap().battery, loaded.battery);
        assert_eq!(
            editor.values.unwrap().ac.scheduling_policy,
            Some(ProcessorSchedulingPolicy::PerformanceProcessors)
        );
        editor.update(
            &mut test_settings(),
            &[],
            Message::Load(
                Source::Ac,
                PresetChoice(
                    "Saver".into(),
                    AdvancedPowerPlanTuningValues::for_preset(ProcessorPowerPreset::Saver),
                ),
            ),
        );
        assert_eq!(
            editor.values.unwrap().ac.scheduling_policy,
            Some(ProcessorSchedulingPolicy::PerformanceProcessors)
        );
        assert_eq!(
            editor.values.unwrap().ac.short_scheduling_policy,
            values.short_scheduling_policy
        );
        assert_eq!(editor.values.unwrap().ac.processor.boost_mode, None);
        let mut settings = test_settings();
        editor.update(
            &mut settings,
            &[],
            Message::OpenPreset(PresetTarget::Custom(None)),
        );
        editor.update(&mut settings, &[], Message::PresetName("Scheduling".into()));
        editor.update(
            &mut settings,
            &[],
            Message::PresetScheduling(
                SchedulingField::ShortRunning,
                Some(ProcessorSchedulingPolicy::PreferEfficiencyProcessors),
            ),
        );
        editor.update(&mut settings, &[], Message::SavePreset);
        assert_eq!(
            saved_presets(&settings)[0].values.short_scheduling_policy,
            Some(ProcessorSchedulingPolicy::PreferEfficiencyProcessors)
        );
        let choice = PresetChoice("Scheduling".into(), saved_presets(&settings)[0].values);
        editor.update(
            &mut settings,
            &[],
            Message::Load(Source::Battery, choice.clone()),
        );
        assert_eq!(
            editor.values.unwrap().battery.short_scheduling_policy,
            saved_presets(&settings)[0].values.short_scheduling_policy
        );
        editor.values.as_mut().unwrap().battery.scheduling_policy = None;
        editor.dirty = false;
        editor.update(
            &mut settings,
            &[],
            Message::Scheduling(
                Source::Battery,
                SchedulingField::LongRunning,
                ProcessorSchedulingPolicy::Automatic,
            ),
        );
        assert!(!editor.dirty);
        editor.update(&mut settings, &[], Message::Load(Source::Battery, choice));
        assert_eq!(editor.values.unwrap().battery.scheduling_policy, None);
        editor.update(
            &mut settings,
            &[],
            Message::OpenPreset(PresetTarget::BuiltIn(ProcessorPowerPreset::Balanced)),
        );
        editor.update(
            &mut settings,
            &[],
            Message::PresetScheduling(
                SchedulingField::LongRunning,
                Some(ProcessorSchedulingPolicy::EfficiencyProcessors),
            ),
        );
        assert_eq!(
            editor.preset.as_ref().unwrap().values.scheduling_policy,
            None
        );
    }

    #[test]
    fn retarget_read_failure_cannot_reuse_previous_plan_values() {
        let values = AdvancedPowerPlanTuningValues::for_preset(ProcessorPowerPreset::Performance);
        let replacement = PowerPlan {
            guid: "invalid-replacement-guid".into(),
            name: "Replacement".into(),
            active: true,
            managed_adaptive: false,
        };
        for automatic in [true, false] {
            let mut editor = Editor {
                target: Some("previous-plan".into()),
                values: Some(AdvancedPowerPlanTuningSourceValues::same(values)),
                personality: Some(PowerPlanPersonality::HighPerformance),
                selected_presets: [Some(PresetChoice("Old selection".into(), values)), None],
                ..Default::default()
            };
            let plans = [replacement.clone()];
            if automatic {
                editor.ensure_plan(&plans);
            } else {
                editor.update(
                    &mut test_settings(),
                    &plans,
                    Message::Plan(replacement.guid.clone()),
                );
            }
            assert_eq!(editor.target.as_deref(), Some(replacement.guid.as_str()));
            assert!(editor.values.is_none());
            assert!(editor.personality.is_none());
            assert_eq!(editor.selected_presets, [None, None]);
            editor.update(
                &mut test_settings(),
                &plans,
                Message::Value(Source::Ac, Field::Minimum, 25),
            );
            assert!(!editor.dirty);
            assert!(!editor.confirm_apply(&plans));
        }
    }

    #[test]
    fn identical_presets_keep_explicit_selection_for_each_source() {
        let mut editor = Editor::default();
        let values = AdvancedPowerPlanTuningValues::for_preset(ProcessorPowerPreset::Balanced);
        let loaded = AdvancedPowerPlanTuningSourceValues::same(values);
        editor.values = Some(loaded);
        let options = vec![
            PresetChoice("Balanced".into(), values),
            PresetChoice("Mine".into(), values),
        ];
        editor.update(
            &mut test_settings(),
            &[],
            Message::Load(Source::Ac, options[0].clone()),
        );
        editor.update(
            &mut test_settings(),
            &[],
            Message::Load(Source::Battery, options[1].clone()),
        );
        assert_eq!(
            editor.selected_preset(Source::Ac, loaded.ac, &options),
            Some(options[0].clone())
        );
        assert_eq!(
            editor.selected_preset(Source::Battery, loaded.battery, &options),
            Some(options[1].clone())
        );
        editor.update(
            &mut test_settings(),
            &[],
            Message::Load(Source::Ac, options[1].clone()),
        );
        editor.discard_editor();
        assert_eq!(
            editor.selected_preset(Source::Ac, loaded.ac, &options),
            Some(options[1].clone())
        );
        assert_eq!(
            editor.selected_preset(Source::Ac, loaded.ac, &options[..1]),
            Some(options[0].clone())
        );
        let changed = AdvancedPowerPlanTuningValues::for_preset(ProcessorPowerPreset::Saver);
        assert_eq!(
            editor.selected_preset(
                Source::Ac,
                changed.with_boost_mode(Some(changed.processor.boost_mode)),
                &options
            ),
            None
        );
    }

    #[test]
    fn presets_are_unique_readonly_and_change_only_requested_source() {
        let mut e = Editor::default();
        let mut settings = test_settings();
        settings.select_power_source(crate::config::PowerSourceProfile::OnBattery);
        let balanced = AdvancedPowerPlanTuningValues::for_preset(ProcessorPowerPreset::Balanced);
        let loaded = AdvancedPowerPlanTuningSourceValues::same(balanced);
        e.values = Some(loaded);
        e.update(
            &mut settings,
            &[],
            Message::Load(
                Source::Ac,
                PresetChoice(
                    "Saver".into(),
                    AdvancedPowerPlanTuningValues::for_preset(ProcessorPowerPreset::Saver),
                ),
            ),
        );
        assert_eq!(e.values.unwrap().battery, loaded.battery);
        e.update(
            &mut settings,
            &[],
            Message::OpenPreset(PresetTarget::Custom(None)),
        );
        e.update(&mut settings, &[], Message::PresetName(" Custom ".into()));
        e.update(&mut settings, &[], Message::SavePreset);
        assert_eq!(saved_presets(&settings)[0].name, "Custom");
        assert!(settings.global().on_battery.is_none());
        assert!(!valid_name(saved_presets(&settings), None, "CUSTOM"));
        e.update(
            &mut settings,
            &[],
            Message::OpenPreset(PresetTarget::BuiltIn(ProcessorPowerPreset::Balanced)),
        );
        e.update(
            &mut settings,
            &[],
            Message::PresetValueText(Field::Parking, "99".into()),
        );
        assert_eq!(e.preset.as_ref().unwrap().values, balanced);
    }
}
