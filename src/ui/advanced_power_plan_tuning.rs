use super::design;
use super::widgets::{action_button, button, icon_button, pick_list, slider, text_input};
use crate::application::{AdvancedPowerPlanTuningError, AdvancedPowerPlanTuningService};
use crate::config::AdvancedPowerPlanTuningPreset;
use crate::power::{
    AdvancedPowerPlanTuningSourceValues, AdvancedPowerPlanTuningValues, EffectivePowerMode,
    PowerPlan, PowerPlanPersonality, ProcessorBoostMode, ProcessorPowerPreset,
    ProcessorSchedulingPolicy,
};
use crate::ui::scrolling::scrollable;
use iced::widget::{column, row, text};
use iced::{Element, Fill};
use rust_i18n::t;

#[derive(Default)]
pub(super) struct Editor {
    service: AdvancedPowerPlanTuningService,
    target: Option<String>,
    values: Option<AdvancedPowerPlanTuningSourceValues>,
    personality: Option<PowerPlanPersonality>,
    pub(super) dirty: bool,
    pub(super) status: String,
    preset: Option<PresetEditor>,
    review: Option<ApplyReview>,
    expanded: [bool; 2],
    selected_presets: [Option<PresetChoice>; 2],
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ApplyOrigin {
    Page,
    Save,
}

struct ApplyReview {
    guid: String,
    name: String,
    current: AdvancedPowerPlanTuningSourceValues,
    requested: AdvancedPowerPlanTuningSourceValues,
    origin: ApplyOrigin,
}

impl ApplyReview {
    fn changes(&self, source: Source) -> impl Iterator<Item = (&'static str, String, String)> {
        let (current, requested) = match source {
            Source::Ac => (self.current.ac, self.requested.ac),
            Source::Battery => (self.current.battery, self.requested.battery),
        };
        let percentages = fields(current)
            .into_iter()
            .zip(fields(requested))
            .filter(|((_, _, before), (_, _, after))| before != after)
            .map(|((_, label, before), (_, _, after))| {
                (label, format!("{before}%"), format!("{after}%"))
            });
        let (before, after) = (current.processor.boost_mode, requested.processor.boost_mode);
        let boost = (before != after).then(|| {
            (
                "processor_power.boost_mode",
                BoostChoice(before).to_string(),
                BoostChoice(after).to_string(),
            )
        });
        let scheduling = scheduling_fields(current)
            .into_iter()
            .zip(scheduling_fields(requested))
            .filter_map(|((_, label, before), (_, _, after))| {
                let after = after?;
                (before != Some(after)).then(|| {
                    let before = before
                        .map(|value| SchedulingChoice(Some(value)).to_string())
                        .unwrap_or_else(|| t!("common.unavailable").to_string());
                    (label, before, SchedulingChoice(Some(after)).to_string())
                })
            });
        percentages.chain(boost).chain(scheduling)
    }
}

#[derive(Debug, Clone)]
pub(super) enum Message {
    Plan(String),
    Group(Source),
    Refresh,
    ReviewApply,
    ConfirmApply,
    CancelApply,
    Value(Source, Field, u32),
    ValueText(Source, Field, String),
    Boost(Source, ProcessorBoostMode),
    Scheduling(Source, SchedulingField, ProcessorSchedulingPolicy),
    Load(Source, PresetChoice),
    OpenPreset(PresetTarget),
    PresetName(String),
    PresetValue(Field, u32),
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
        values: AdvancedPowerPlanTuningValues,
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
        self.refresh();
    }

    pub(super) fn refresh(&mut self) {
        self.cancel_apply_review();
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
                self.values = None;
                self.dirty = false;
                self.status = error.to_string();
            }
        }
    }
    pub(super) fn review_apply(
        &mut self,
        plans: &[PowerPlan],
        origin: ApplyOrigin,
    ) -> Result<(), String> {
        if self.review.is_some() || !self.dirty {
            return Ok(());
        }
        let (Some(guid), Some(values)) = (&self.target, self.values) else {
            return Err(t!("processor_power.no_active_plan").to_string());
        };
        let name = plans
            .iter()
            .find(|plan| plan.guid.eq_ignore_ascii_case(guid))
            .map(|plan| plan.name.clone())
            .ok_or_else(|| t!("processor_power.no_active_plan").to_string())?;
        let current = self
            .service
            .read_values(guid)
            .map_err(|error| error.to_string())?;
        self.review = Some(ApplyReview {
            guid: guid.clone(),
            name,
            current,
            requested: values.normalized(),
            origin,
        });
        Ok(())
    }

    pub(super) fn review_origin(&self) -> Option<ApplyOrigin> {
        self.review.as_ref().map(|review| review.origin)
    }

    pub(super) fn cancel_apply_review(&mut self) {
        self.review = None;
    }

    pub(super) fn confirm_apply(&mut self) -> bool {
        let Some(review) = self.review.take() else {
            return false;
        };
        if self.target.as_ref() != Some(&review.guid)
            || self
                .values
                .map(AdvancedPowerPlanTuningSourceValues::normalized)
                != Some(review.requested)
        {
            self.status = t!("processor_power.review_stale").to_string();
            return false;
        }
        let outcome =
            match self
                .service
                .apply_reviewed_values(&review.guid, review.current, review.requested)
            {
                Ok(outcome) => outcome,
                Err(AdvancedPowerPlanTuningError::ValuesChanged) => {
                    self.status = t!("processor_power.review_stale").to_string();
                    return false;
                }
                Err(error) => {
                    self.status = error.to_string();
                    return false;
                }
            };
        self.values = outcome.actual_values;
        self.dirty = self.values.is_some() && !outcome.applied;
        match outcome.error {
            Some(error) => {
                self.status = error.to_string();
                false
            }
            None => {
                self.dirty = false;
                self.status = t!("processor_power.applied_custom", plan = &review.guid).to_string();
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
            Message::ReviewApply => {
                if let Err(error) = self.review_apply(plans, ApplyOrigin::Page) {
                    self.status = error;
                }
            }
            Message::ConfirmApply => {
                self.confirm_apply();
            }
            Message::CancelApply => self.cancel_apply_review(),
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
                    source_values(values, source).processor.boost_mode = value;
                    self.dirty = true;
                }
            }
            Message::Load(source, choice) => {
                if let Some(values) = self.values.as_mut() {
                    let current = source_values(values, source);
                    let mut preset = choice.1.normalized();
                    preset.scheduling_policy = current
                        .scheduling_policy
                        .map(|value| preset.scheduling_policy.unwrap_or(value));
                    preset.short_scheduling_policy = current
                        .short_scheduling_policy
                        .map(|value| preset.short_scheduling_policy.unwrap_or(value));
                    *current = preset;
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
                        let Some(p) = presets.get(i) else {
                            return;
                        };
                        (p.name.clone(), p.values.normalized())
                    }
                    PresetTarget::Custom(None) => (
                        String::new(),
                        self.values.map(|v| v.ac).unwrap_or_else(|| {
                            AdvancedPowerPlanTuningValues::for_preset(
                                ProcessorPowerPreset::Balanced,
                            )
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
                for (field, label, value) in fields(values) {
                    controls.push(
                        row![
                            text(super::widgets::label_with_unit(&t!(label), "%")).width(Fill),
                            slider(0..=100, value, move |v| Message::Value(source, field, v))
                                .width(180),
                            super::widgets::stepper(
                                &value.to_string(),
                                0..=100,
                                1,
                                Some(move |v| Message::ValueText(source, field, v))
                            )
                        ]
                        .spacing(design::space::SMALL)
                        .height(46)
                        .align_y(iced::Center)
                        .into(),
                    );
                }
                controls.push(
                    row![
                        text(t!("processor_power.boost_mode").to_string()).width(Fill),
                        pick_list(
                            ProcessorBoostMode::ALL.map(BoostChoice),
                            Some(BoostChoice(values.processor.boost_mode)),
                            move |v| Message::Boost(source, v.0)
                        )
                        .width(280)
                    ]
                    .spacing(design::space::SMALL)
                    .into(),
                );
                for (field, label, policy) in scheduling_fields(values) {
                    let control: Element<'_, Message> = if let Some(policy) = policy {
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
                    } else {
                        text(t!("common.unavailable").to_string())
                            .style(text::secondary)
                            .into()
                    };
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
        body = body
            .push(super::widgets::settings_card(
                row![
                    action_button(t!("processor_power.refresh_values").to_string())
                        .on_press_maybe(self.target.as_ref().map(|_| Message::Refresh)),
                    action_button(t!("settings.apply").to_string())
                        .style(super::widgets::primary_button)
                        .on_press_maybe(
                            (self.values.is_some() && self.dirty).then_some(Message::ReviewApply)
                        )
                ]
                .spacing(design::space::SMALL)
                .align_y(iced::Center),
            ))
            .push(text(&self.status));
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
        for (source, title) in [
            (Source::Ac, "power_source.plugged_in"),
            (Source::Battery, "power_source.on_battery"),
        ] {
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
                        t!(title).to_string(),
                        design::typography::BODY,
                    ))
                    .push(widgets::settings_card(widgets::settings_rows(rows)));
            }
        }
        if !has_changes {
            body = body
                .push(text(t!("processor_power.no_changes").to_string()).style(text::secondary));
        }
        Some(widgets::modal_frame(
            row![
                widgets::heading(
                    t!("processor_power.review_changes").to_string(),
                    design::typography::DIALOG_TITLE
                )
                .width(Fill),
                icon_button("icons/x.svg").on_press(Message::CancelApply)
            ]
            .align_y(iced::Center),
            scrollable(body).height(Fill),
            row![
                action_button(t!("common.cancel").to_string())
                    .style(widgets::tertiary_button)
                    .on_press(Message::CancelApply),
                action_button(t!("settings.apply").to_string())
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
        for (field, label, value) in fields(p.values) {
            values.push(
                widgets::setting_row_with_unit(
                    label,
                    "%",
                    widgets::stepper(
                        &value.to_string(),
                        0..=100,
                        1,
                        editable.then_some(move |v| Message::PresetValueText(field, v)),
                    ),
                )
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
                    .on_press_maybe(
                        valid_name(presets, index, &p.name).then_some(Message::SavePreset),
                    ),
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
) -> &mut AdvancedPowerPlanTuningValues {
    match source {
        Source::Ac => &mut values.ac,
        Source::Battery => &mut values.battery,
    }
}
fn preset_matches(
    preset: AdvancedPowerPlanTuningValues,
    actual: AdvancedPowerPlanTuningValues,
) -> bool {
    let preset = preset.normalized();
    preset.processor == actual.processor
        && preset
            .scheduling_policy
            .is_none_or(|policy| Some(policy) == actual.scheduling_policy)
        && preset
            .short_scheduling_policy
            .is_none_or(|policy| Some(policy) == actual.short_scheduling_policy)
}

fn scheduling_fields(
    values: AdvancedPowerPlanTuningValues,
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

fn scheduling_field(
    values: &mut AdvancedPowerPlanTuningValues,
    field: SchedulingField,
) -> &mut Option<ProcessorSchedulingPolicy> {
    match field {
        SchedulingField::LongRunning => &mut values.scheduling_policy,
        SchedulingField::ShortRunning => &mut values.short_scheduling_policy,
    }
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

fn fields(v: AdvancedPowerPlanTuningValues) -> [(Field, &'static str, u32); 4] {
    let v = v.processor;
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
fn set_field(values: &mut AdvancedPowerPlanTuningValues, field: Field, value: u32) {
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

    #[test]
    fn review_shows_only_normalized_changes_for_each_power_source() {
        let mut current = AdvancedPowerPlanTuningSourceValues::same(
            AdvancedPowerPlanTuningValues::for_preset(ProcessorPowerPreset::Balanced),
        );
        current.ac.processor.performance_max = 70;
        current.ac.scheduling_policy = Some(ProcessorSchedulingPolicy::Automatic);
        let mut draft = current;
        draft.ac.processor.performance_min = 80;
        draft.ac.scheduling_policy = None;
        draft.battery.processor.boost_policy = 35;
        draft.battery.processor.boost_mode = ProcessorBoostMode::Disabled;
        draft.battery.short_scheduling_policy = Some(ProcessorSchedulingPolicy::AllProcessors);
        let review = ApplyReview {
            guid: "plan".into(),
            name: "Balanced".into(),
            current,
            requested: draft.normalized(),
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
        assert!(!editor.confirm_apply());
        let review = || ApplyReview {
            guid: "first-plan".into(),
            name: "Balanced".into(),
            current,
            requested: draft,
            origin: ApplyOrigin::Save,
        };

        editor.review = Some(review());
        editor.update(&mut vec![], &[], Message::CancelApply);
        assert!(!editor.confirm_apply());
        assert_eq!(editor.values, Some(draft));
        assert!(editor.dirty);

        editor.review = Some(review());
        editor.target = Some("second-plan".into());
        assert!(!editor.confirm_apply());
        assert_eq!(editor.status, t!("processor_power.review_stale"));
        assert_eq!(editor.values, Some(draft));
        assert!(editor.dirty);

        editor.review = Some(review());
        editor.target = Some("first-plan".into());
        editor.values = Some(current);
        assert!(!editor.confirm_apply());
        assert_eq!(editor.status, t!("processor_power.review_stale"));
        assert_eq!(editor.values, Some(current));
    }

    #[test]
    fn preset_matching_does_not_normalize_actual_readback() {
        let mut preset = AdvancedPowerPlanTuningValues::for_preset(ProcessorPowerPreset::Balanced);
        preset.processor.performance_min = 75;
        preset.processor.performance_max = 75;
        let mut actual = preset;
        actual.processor.performance_max = 50;
        assert!(!preset_matches(preset, actual));
        assert!(preset_matches(preset, preset));
    }

    #[test]
    fn scheduling_edits_and_presets_preserve_power_source_and_availability() {
        let mut values = AdvancedPowerPlanTuningValues::for_preset(ProcessorPowerPreset::Balanced);
        values.scheduling_policy = Some(ProcessorSchedulingPolicy::Automatic);
        values.short_scheduling_policy = Some(ProcessorSchedulingPolicy::AllProcessors);
        let mut editor = Editor {
            values: Some(AdvancedPowerPlanTuningSourceValues::same(values)),
            ..Default::default()
        };
        editor.update(
            &mut vec![],
            &[],
            Message::Scheduling(
                Source::Ac,
                SchedulingField::LongRunning,
                ProcessorSchedulingPolicy::PerformanceProcessors,
            ),
        );
        assert_eq!(editor.values.unwrap().battery, values);
        assert_eq!(
            editor.values.unwrap().ac.scheduling_policy,
            Some(ProcessorSchedulingPolicy::PerformanceProcessors)
        );
        editor.update(
            &mut vec![],
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
        let mut presets = vec![];
        editor.update(
            &mut presets,
            &[],
            Message::OpenPreset(PresetTarget::Custom(None)),
        );
        editor.update(&mut presets, &[], Message::PresetName("Scheduling".into()));
        editor.update(
            &mut presets,
            &[],
            Message::PresetScheduling(
                SchedulingField::ShortRunning,
                Some(ProcessorSchedulingPolicy::PreferEfficiencyProcessors),
            ),
        );
        editor.update(&mut presets, &[], Message::SavePreset);
        assert_eq!(
            presets[0].values.short_scheduling_policy,
            Some(ProcessorSchedulingPolicy::PreferEfficiencyProcessors)
        );
        let choice = PresetChoice("Scheduling".into(), presets[0].values);
        editor.update(
            &mut presets,
            &[],
            Message::Load(Source::Battery, choice.clone()),
        );
        assert_eq!(
            editor.values.unwrap().battery.short_scheduling_policy,
            presets[0].values.short_scheduling_policy
        );
        editor.values.as_mut().unwrap().battery.scheduling_policy = None;
        editor.dirty = false;
        editor.update(
            &mut presets,
            &[],
            Message::Scheduling(
                Source::Battery,
                SchedulingField::LongRunning,
                ProcessorSchedulingPolicy::Automatic,
            ),
        );
        assert!(!editor.dirty);
        editor.update(&mut presets, &[], Message::Load(Source::Battery, choice));
        assert_eq!(editor.values.unwrap().battery.scheduling_policy, None);
        editor.update(
            &mut presets,
            &[],
            Message::OpenPreset(PresetTarget::BuiltIn(ProcessorPowerPreset::Balanced)),
        );
        editor.update(
            &mut presets,
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
            assert!(!editor.confirm_apply());
        }
    }

    #[test]
    fn identical_presets_keep_explicit_selection_for_each_source() {
        let mut editor = Editor::default();
        let values = AdvancedPowerPlanTuningValues::for_preset(ProcessorPowerPreset::Balanced);
        editor.values = Some(AdvancedPowerPlanTuningSourceValues::same(values));
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
        let changed = AdvancedPowerPlanTuningValues::for_preset(ProcessorPowerPreset::Saver);
        assert_eq!(editor.selected_preset(Source::Ac, changed, &options), None);
    }

    #[test]
    fn presets_are_unique_readonly_and_change_only_requested_source() {
        let mut e = Editor::default();
        let mut presets = vec![];
        let balanced = AdvancedPowerPlanTuningValues::for_preset(ProcessorPowerPreset::Balanced);
        e.values = Some(AdvancedPowerPlanTuningSourceValues::same(balanced));
        e.update(
            &mut presets,
            &[],
            Message::Load(
                Source::Ac,
                PresetChoice(
                    "Saver".into(),
                    AdvancedPowerPlanTuningValues::for_preset(ProcessorPowerPreset::Saver),
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
        e.update(&mut presets, &[], Message::PresetValue(Field::Parking, 99));
        assert_eq!(e.preset.as_ref().unwrap().values, balanced);
    }
}
