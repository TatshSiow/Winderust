use super::design;
use super::widgets::{self, checkbox, icon_button, Choice};
use super::widgets::{action_button, pick_list, slider, text_input};
use crate::ui::scrolling::scrollable;
use crate::{
    config::{
        ByCpuLoadRule, ByCpuLoadStability, ByTimeRule, CpuLoadPercent, CpuUsageComparison,
        RuleTime, Settings, WeekdaySetting,
    },
    power::PowerPlan,
};
use iced::widget::{column, container, row, text};
use iced::{Element, Fill};
use rust_i18n::t;
use std::collections::BTreeMap;
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Kind {
    Time,
    CpuLoad,
}
#[derive(Debug, Clone)]
pub(super) enum Message {
    Enabled(bool),
    Add,
    RuleEnabled(usize, bool),
    Name(usize, String),
    Plan(usize, Option<String>),
    Remove(usize),

    Edit(usize),
    Save,
    Cancel,
    Day(usize, WeekdaySetting, bool),
    Start(usize, String),
    End(usize, String),
    OpenTimePicker(bool),
    CancelTimePicker,
    TimePicked(bool, chrono::NaiveTime),
    PickerHour(String),
    PickerMinute(String),
    PickerPart(super::pickers::TimePart),
    PickerDial(super::pickers::TimePart, u32, bool),
    Comparison(usize, CpuUsageComparison),
    Threshold(usize, String),
    Upper(usize, String),
    Duration(usize, String),
    Stability(usize, bool),
    ExitThreshold(usize, String),
    ExitUpper(usize, String),
    MinimumHold(usize, String),
    Else(usize, bool),
    ElsePlan(usize, Option<String>),
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Field {
    Start,
    End,
    Threshold,
    Upper,
    Duration,
    ExitThreshold,
    ExitUpper,
    MinimumHold,
}

#[derive(Clone, Copy)]
enum RuleRef<'a> {
    Time(&'a ByTimeRule),
    Cpu(&'a ByCpuLoadRule),
}
#[derive(Default)]
pub(super) struct Editor {
    ids: Vec<u64>,
    next_id: u64,
    inputs: BTreeMap<(u64, Field), String>,
    draft: Option<Settings>,
    editing: usize,
    creating: bool,
    time_picker: Option<bool>,
    picker_time: super::pickers::TimeDraft,
}
impl Editor {
    pub(super) fn picker_open(&self) -> bool {
        self.time_picker.is_some()
    }

    pub(super) fn dismiss_picker(&mut self) {
        self.time_picker = None;
    }

    pub(super) fn has_pending_editor(&self) -> bool {
        self.draft.is_some()
    }

    fn sync_ids(&mut self, kind: Kind, s: &Settings) {
        let count = match kind {
            Kind::Time => s.by_time.rules.len(),
            Kind::CpuLoad => s.by_cpu_load.rules.len(),
        };
        self.ids.truncate(count);
        while self.ids.len() < count {
            self.ids.push(self.next_id);
            self.next_id = self.next_id.wrapping_add(1);
        }
    }
    pub(super) fn valid(&self) -> bool {
        self.inputs
            .iter()
            .all(|((_, field), value)| valid_input(*field, value))
            && self
                .draft
                .as_ref()
                .is_none_or(|draft| draft.validate_by_cpu_load_rules().is_ok())
    }
    pub(super) fn update(
        &mut self,
        kind: Kind,
        s: &mut Settings,
        plans: &[PowerPlan],
        message: Message,
    ) {
        match message {
            Message::OpenTimePicker(start) => {
                if kind != Kind::Time || self.draft.is_none() {
                    return;
                }
                let Some(rule) = self
                    .draft
                    .as_ref()
                    .and_then(|draft| draft.by_time.rules.get(self.editing))
                else {
                    return;
                };
                let value = if start {
                    rule.start_time
                } else {
                    rule.end_time
                };
                self.picker_time = super::pickers::TimeDraft::new(value.as_naive_time());
                self.time_picker = Some(start);
                return;
            }
            Message::PickerHour(value) => {
                self.picker_time.hour = value;
                self.picker_time.part = super::pickers::TimePart::Hour;
                return;
            }
            Message::PickerMinute(value) => {
                self.picker_time.minute = value;
                self.picker_time.part = super::pickers::TimePart::Minute;
                return;
            }
            Message::PickerPart(part) => {
                self.picker_time.part = part;
                return;
            }
            Message::PickerDial(part, value, complete) => {
                if self.time_picker.is_some() {
                    self.picker_time.select(part, value, complete);
                }
                return;
            }
            Message::CancelTimePicker => {
                self.time_picker = None;
                return;
            }
            Message::TimePicked(start, time) => {
                if self.time_picker != Some(start) || self.draft.is_none() || kind != Kind::Time {
                    return;
                }
                self.time_picker = None;
                let value = time.format("%H:%M").to_string();
                let message = if start {
                    Message::Start(self.editing, value)
                } else {
                    Message::End(self.editing, value)
                };
                self.update(kind, s, plans, message);
                return;
            }
            Message::Add | Message::Edit(_) => {
                self.time_picker = None;
                self.inputs.clear();
                let mut draft = s.clone();
                self.creating = matches!(message, Message::Add);
                self.editing = match message {
                    Message::Edit(index) => index,
                    _ => {
                        let index = match kind {
                            Kind::Time => s.by_time.rules.len(),
                            Kind::CpuLoad => s.by_cpu_load.rules.len(),
                        };
                        self.update_fields(kind, &mut draft, plans, Message::Add);
                        index
                    }
                };
                self.sync_ids(kind, &draft);
                self.draft = Some(draft);
                return;
            }
            Message::Cancel => {
                self.time_picker = None;
                self.draft = None;
                self.inputs.clear();
                return;
            }
            Message::Save => {
                if !self.valid() {
                    return;
                }
                if let Some(draft) = self.draft.take() {
                    self.time_picker = None;
                    match kind {
                        Kind::Time => {
                            if let Some(rule) = draft.by_time.rules.get(self.editing) {
                                if self.creating {
                                    s.by_time.rules.push(rule.clone());
                                } else if let Some(target) = s.by_time.rules.get_mut(self.editing) {
                                    *target = rule.clone();
                                }
                            }
                        }
                        Kind::CpuLoad => {
                            if let Some(rule) = draft.by_cpu_load.rules.get(self.editing) {
                                if self.creating {
                                    s.by_cpu_load.rules.push(rule.clone());
                                } else if let Some(target) =
                                    s.by_cpu_load.rules.get_mut(self.editing)
                                {
                                    *target = rule.clone();
                                }
                            }
                        }
                    }
                }
                self.inputs.clear();
                self.sync_ids(kind, s);
                return;
            }
            _ => {}
        }
        if let Some(mut draft) = self.draft.take() {
            self.update_fields(kind, &mut draft, plans, message);
            self.draft = Some(draft);
        } else {
            self.update_fields(kind, s, plans, message);
        }
    }
    fn update_fields(
        &mut self,
        kind: Kind,
        s: &mut Settings,
        plans: &[PowerPlan],
        message: Message,
    ) {
        self.sync_ids(kind, s);
        let input = match &message {
            Message::Start(i, v) => Some((*i, Field::Start, v.clone())),
            Message::End(i, v) => Some((*i, Field::End, v.clone())),
            Message::Threshold(i, v) => Some((*i, Field::Threshold, v.clone())),
            Message::Upper(i, v) => Some((*i, Field::Upper, v.clone())),
            Message::Duration(i, v) => Some((*i, Field::Duration, v.clone())),
            Message::ExitThreshold(i, v) => Some((*i, Field::ExitThreshold, v.clone())),
            Message::ExitUpper(i, v) => Some((*i, Field::ExitUpper, v.clone())),
            Message::MinimumHold(i, v) => Some((*i, Field::MinimumHold, v.clone())),
            _ => None,
        };
        if let Some((i, field, value)) = input {
            let Some(id) = self.ids.get(i).copied() else {
                return;
            };
            let valid = valid_input(field, &value);
            self.inputs.insert((id, field), value);
            if !valid {
                return;
            }
        }
        let active = || plans.iter().find(|p| p.active).map(|p| p.guid.clone());
        match message {
            Message::Enabled(v) => match kind {
                Kind::Time => s.by_time.enabled = v,
                Kind::CpuLoad => s.by_cpu_load.enabled = v,
            },
            Message::Add => match kind {
                Kind::Time => s.by_time.rules.push(ByTimeRule {
                    enabled: true,
                    name: t!("by_time.new_rule").to_string(),
                    days: WeekdaySetting::all().to_vec(),
                    power_plan_guid: active(),
                    ..ByTimeRule::default()
                }),
                Kind::CpuLoad => s.by_cpu_load.rules.push(ByCpuLoadRule {
                    name: t!("by_cpu_load.new_rule").to_string(),
                    power_plan_guid: active(),
                    else_power_plan_guid: active(),
                    ..ByCpuLoadRule::default()
                }),
            },
            Message::Remove(i) => {
                if i < self.ids.len() {
                    let id = self.ids.remove(i);
                    match kind {
                        Kind::Time => {
                            s.by_time.rules.remove(i);
                        }
                        Kind::CpuLoad => {
                            s.by_cpu_load.rules.remove(i);
                        }
                    }
                    self.inputs.retain(|(key, _), _| *key != id);
                }
            }

            Message::Edit(_) | Message::Save | Message::Cancel => {}
            Message::OpenTimePicker(_)
            | Message::CancelTimePicker
            | Message::TimePicked(..)
            | Message::PickerHour(_)
            | Message::PickerMinute(_)
            | Message::PickerPart(_)
            | Message::PickerDial(..) => {
                unreachable!("time picker messages are handled by the rule editor")
            }
            Message::RuleEnabled(i, v) => match kind {
                Kind::Time => {
                    if let Some(r) = s.by_time.rules.get_mut(i) {
                        r.enabled = v
                    }
                }
                Kind::CpuLoad => {
                    if let Some(r) = s.by_cpu_load.rules.get_mut(i) {
                        r.enabled = v
                    }
                }
            },
            Message::Name(i, v) => match kind {
                Kind::Time => {
                    if let Some(r) = s.by_time.rules.get_mut(i) {
                        r.name = v
                    }
                }
                Kind::CpuLoad => {
                    if let Some(r) = s.by_cpu_load.rules.get_mut(i) {
                        r.name = v
                    }
                }
            },
            Message::Plan(i, v) => match kind {
                Kind::Time => {
                    if let Some(r) = s.by_time.rules.get_mut(i) {
                        r.power_plan_guid = v
                    }
                }
                Kind::CpuLoad => {
                    if let Some(r) = s.by_cpu_load.rules.get_mut(i) {
                        r.power_plan_guid = v
                    }
                }
            },
            Message::Day(i, day, v) => {
                if let Some(r) = s.by_time.rules.get_mut(i) {
                    if v && !r.days.contains(&day) {
                        r.days.push(day);
                    } else if !v {
                        r.days.retain(|d| *d != day);
                    }
                }
            }
            Message::Start(i, v) => {
                if let (Some(r), Ok(v)) = (s.by_time.rules.get_mut(i), v.parse::<RuleTime>()) {
                    r.start_time = v
                }
            }
            Message::End(i, v) => {
                if let (Some(r), Ok(v)) = (s.by_time.rules.get_mut(i), v.parse::<RuleTime>()) {
                    r.end_time = v
                }
            }
            Message::Comparison(i, v) => {
                if let Some(r) = s.by_cpu_load.rules.get_mut(i) {
                    if r.comparison == v {
                        return;
                    }
                    r.comparison = v;
                    if v == CpuUsageComparison::Between {
                        r.upper_threshold_percent.get_or_insert(CpuLoadPercent::MAX);
                    }
                    if let Some(stability) = &r.stability {
                        r.stability = (v != CpuUsageComparison::Else).then(|| ByCpuLoadStability {
                            minimum_hold_seconds: stability.minimum_hold_seconds,
                            ..initial_stability(r)
                        });
                    }
                    if let Some(id) = self.ids.get(i) {
                        self.inputs.retain(|(key, field), _| {
                            key != id
                                || match field {
                                    Field::ExitThreshold | Field::ExitUpper => false,
                                    Field::Upper => v == CpuUsageComparison::Between,
                                    Field::MinimumHold => v != CpuUsageComparison::Else,
                                    _ => true,
                                }
                        });
                    }
                }
            }
            Message::Threshold(i, v) => {
                if let (Some(r), Some(v)) = (
                    s.by_cpu_load.rules.get_mut(i),
                    v.parse::<u8>()
                        .ok()
                        .and_then(|value| CpuLoadPercent::new(value).ok()),
                ) {
                    r.threshold_percent = v
                }
            }
            Message::Upper(i, v) => {
                if let (Some(r), Some(v)) = (
                    s.by_cpu_load.rules.get_mut(i),
                    v.parse::<u8>()
                        .ok()
                        .and_then(|value| CpuLoadPercent::new(value).ok()),
                ) {
                    r.upper_threshold_percent = Some(v)
                }
            }
            Message::Duration(i, v) => {
                if let (Some(r), Ok(v)) = (s.by_cpu_load.rules.get_mut(i), v.parse::<u64>()) {
                    r.duration_seconds = v
                }
            }
            Message::Stability(i, enabled) => {
                if let Some(r) = s.by_cpu_load.rules.get_mut(i) {
                    r.stability = (enabled && r.comparison != CpuUsageComparison::Else)
                        .then(|| initial_stability(r));
                    if let Some(id) = self.ids.get(i) {
                        self.inputs.retain(|(key, field), _| {
                            key != id
                                || !matches!(
                                    field,
                                    Field::ExitThreshold | Field::ExitUpper | Field::MinimumHold
                                )
                        });
                    }
                }
            }
            Message::ExitThreshold(i, v) => {
                if let (Some(stability), Some(v)) = (
                    s.by_cpu_load
                        .rules
                        .get_mut(i)
                        .and_then(|r| r.stability.as_mut()),
                    v.parse::<u8>()
                        .ok()
                        .and_then(|value| CpuLoadPercent::new(value).ok()),
                ) {
                    stability.exit_threshold_percent = v;
                }
            }
            Message::ExitUpper(i, v) => {
                if let (Some(stability), Some(v)) = (
                    s.by_cpu_load
                        .rules
                        .get_mut(i)
                        .and_then(|r| r.stability.as_mut()),
                    v.parse::<u8>()
                        .ok()
                        .and_then(|value| CpuLoadPercent::new(value).ok()),
                ) {
                    stability.exit_upper_threshold_percent = Some(v);
                }
            }
            Message::MinimumHold(i, v) => {
                if let (Some(stability), Ok(v)) = (
                    s.by_cpu_load
                        .rules
                        .get_mut(i)
                        .and_then(|r| r.stability.as_mut()),
                    v.parse::<u64>(),
                ) {
                    stability.minimum_hold_seconds = v;
                }
            }
            Message::Else(i, v) => {
                if let Some(r) = s.by_cpu_load.rules.get_mut(i) {
                    r.else_enabled = v;
                    if v && r.else_power_plan_guid.is_none() {
                        r.else_power_plan_guid = active();
                    }
                }
            }
            Message::ElsePlan(i, v) => {
                if let Some(r) = s.by_cpu_load.rules.get_mut(i) {
                    r.else_power_plan_guid = v
                }
            }
        }
        self.sync_ids(kind, s);
    }
    pub(super) fn view<'a>(
        &'a self,
        kind: Kind,
        s: &'a Settings,
        status: &crate::control::power_plan::PowerPlanStatus,
    ) -> Element<'a, Message> {
        let (enabled, label) = match kind {
            Kind::Time => (s.by_time.enabled, "by_time.enable"),
            Kind::CpuLoad => (s.by_cpu_load.enabled, "by_cpu_load.enable"),
        };
        let mut body = column![super::widgets::toggle_card(
            label,
            enabled,
            Some(Message::Enabled)
        )]
        .spacing(super::widgets::CARD_GAP);
        let rules = match kind {
            Kind::Time => s
                .by_time
                .rules
                .iter()
                .enumerate()
                .map(|(i, r)| {
                    (
                        self.ids.get(i).copied().unwrap_or(i as u64),
                        i,
                        RuleRef::Time(r),
                    )
                })
                .collect::<Vec<_>>(),
            Kind::CpuLoad => s
                .by_cpu_load
                .rules
                .iter()
                .enumerate()
                .map(|(i, r)| {
                    (
                        self.ids.get(i).copied().unwrap_or(i as u64),
                        i,
                        RuleRef::Cpu(r),
                    )
                })
                .collect::<Vec<_>>(),
        };

        let empty = rules.is_empty();
        let header = |key| text(t!(key).to_string()).style(text::secondary);
        let keyed = |id, content: Element<'a, Message>| iced::widget::keyed_column([(id, content)]);
        let table = iced::widget::table(
            [
                iced::widget::table::column(
                    header("common.enable"),
                    move |(id, index, rule): (u64, usize, RuleRef<'a>)| {
                        let enabled = match rule {
                            RuleRef::Time(rule) => rule.enabled,
                            RuleRef::Cpu(rule) => rule.enabled,
                        };
                        keyed(
                            id,
                            checkbox(enabled)
                                .on_toggle(move |v| Message::RuleEnabled(index, v))
                                .into(),
                        )
                    },
                )
                .width(48)
                .align_y(iced::Center),
                iced::widget::table::column(
                    header("common.status"),
                    move |(id, index, rule): (u64, usize, RuleRef<'a>)| {
                        let key = rule_status(
                            rule,
                            s.general.enabled && enabled,
                            status.rule_index == Some(index),
                            status,
                        );
                        keyed(
                            id,
                            container(text(t!(key).to_string()))
                                .padding([4, 8])
                                .width(Fill)
                                .style(move |theme| widgets::rule_status_chip(theme, key))
                                .into(),
                        )
                    },
                )
                .width(140)
                .align_y(iced::Center),
                iced::widget::table::column(
                    header("common.rule_name"),
                    move |(id, _, rule): (u64, usize, RuleRef<'a>)| {
                        let name = match rule {
                            RuleRef::Time(rule) => &rule.name,
                            RuleRef::Cpu(rule) => &rule.name,
                        };
                        keyed(id, text(name.clone()).into())
                    },
                )
                .width(Fill)
                .align_y(iced::Center),
                iced::widget::table::column(
                    header("common.actions").align_x(iced::Center),
                    move |(id, index, _): (u64, usize, RuleRef<'a>)| {
                        keyed(
                            id,
                            row![
                                super::widgets::tooltip(
                                    icon_button("icons/pencil.svg").on_press(Message::Edit(index)),
                                    text(t!("common.edit").to_string()),
                                    iced::widget::tooltip::Position::Top
                                ),
                                widgets::rule_delete_button(Some(Message::Remove(index)))
                            ]
                            .into(),
                        )
                    },
                )
                .width(80)
                .align_x(iced::Center)
                .align_y(iced::Center),
            ],
            rules,
        )
        .width(Fill)
        .padding_x(design::space::CONTROL as f32)
        .padding_y(widgets::CARD_PADDING as f32)
        .separator_x(0)
        .separator_y(1);
        let mut contents = column![table];
        if empty {
            contents = contents.push(widgets::settings_divider()).push(
                container(text(t!("common.no_custom_rules").to_string()).style(text::secondary))
                    .padding(widgets::CARD_PADDING as u16),
            );
        }
        body = body.push(widgets::setting_title("common.rules")).push(
            container(super::scrolling::horizontal_table(
                container(contents)
                    .padding([0, (widgets::CARD_PADDING - design::space::CONTROL) as u16])
                    .width(Fill),
                600.0,
            ))
            .width(Fill)
            .clip(true)
            .style(widgets::surface),
        );
        scrollable(body).height(Fill).into()
    }
    pub(super) fn modal<'a>(
        &'a self,
        kind: Kind,
        plans: &'a [PowerPlan],
    ) -> Option<Element<'a, Message>> {
        if kind != Kind::Time {
            return self.modal_content(kind, plans);
        }
        self.draft.as_ref()?.by_time.rules.get(self.editing)?;
        Some(
            container(iced::widget::responsive(move |size| {
                // Own the popup outside the scroll body so viewport clipping cannot displace it.
                iced_aw::DropDown::new(
                    self.modal_content(kind, plans)
                        .expect("Open By Time editor has a rule draft"),
                    super::pickers::time(&self.picker_time, self.time_picker.unwrap_or(true)),
                    self.time_picker.is_some(),
                )
                .width(368)
                .alignment(iced_aw::drop_down::Alignment::End)
                .offset([-size.width / 2.0 - 184.0, 0.0])
                .on_dismiss(Message::CancelTimePicker)
                .into()
            }))
            .width(Fill)
            .max_width(800)
            .height(Fill)
            .max_height(760)
            .into(),
        )
    }

    fn modal_content<'a>(
        &'a self,
        kind: Kind,
        plans: &[PowerPlan],
    ) -> Option<Element<'a, Message>> {
        let s = self.draft.as_ref()?;
        let index = self.editing;
        let id = self.ids.get(index).copied().unwrap_or(index as u64);
        let rule = match kind {
            Kind::Time => RuleRef::Time(s.by_time.rules.get(index)?),
            Kind::CpuLoad => RuleRef::Cpu(s.by_cpu_load.rules.get(index)?),
        };
        let (name, guid) = match rule {
            RuleRef::Time(r) => (&r.name, &r.power_plan_guid),
            RuleRef::Cpu(r) => (&r.name, &r.power_plan_guid),
        };
        let mut controls = column![].spacing(design::space::MEDIUM);
        match rule {
            RuleRef::Time(r) => {
                let mut days = row![];
                for (day, key) in WeekdaySetting::all().into_iter().zip([
                    "weekday.mon",
                    "weekday.tue",
                    "weekday.wed",
                    "weekday.thu",
                    "weekday.fri",
                    "weekday.sat",
                    "weekday.sun",
                ]) {
                    days = days.push(widgets::panel_tab(
                        t!(key).to_string(),
                        r.days.contains(&day),
                        Message::Day(index, day, !r.days.contains(&day)),
                    ));
                }
                let days = container(days).style(widgets::tab_bar);
                controls = controls
                    .push(widgets::heading(
                        t!("by_time.schedule").to_string(),
                        design::typography::BODY,
                    ))
                    .push(widgets::settings_card(widgets::settings_rows([
                        days.into(),
                        row![
                            container(
                                column![
                                    text(t!("by_time.start").to_string()),
                                    self.time_field(index, id, true, r.start_time)
                                ]
                                .spacing(design::space::SMALL)
                            )
                            .width(Fill),
                            container(
                                column![
                                    text(t!("by_time.end").to_string()),
                                    self.time_field(index, id, false, r.end_time)
                                ]
                                .spacing(design::space::SMALL)
                            )
                            .width(Fill)
                        ]
                        .spacing(design::space::MEDIUM)
                        .into(),
                    ])));
            }
            RuleRef::Cpu(r) => {
                let choices = vec![
                    Choice(
                        CpuUsageComparison::AtOrBelow,
                        t!("by_cpu_load.comparison_at_or_below").to_string(),
                    ),
                    Choice(
                        CpuUsageComparison::AtOrAbove,
                        t!("by_cpu_load.comparison_at_or_above").to_string(),
                    ),
                    Choice(
                        CpuUsageComparison::Between,
                        t!("by_cpu_load.comparison_between").to_string(),
                    ),
                ];
                let selected = choices.iter().find(|c| c.0 == r.comparison).cloned();
                let condition = widgets::settings_rows([
                    widgets::setting_row(
                        "by_cpu_load.if",
                        pick_list(choices, selected, move |v| Message::Comparison(index, v.0))
                            .width(design::SELECT_WIDTH),
                    )
                    .into(),
                    self.number(
                        (id, index),
                        Field::Threshold,
                        "by_cpu_load.threshold",
                        u64::from(r.threshold_percent.get()),
                        100,
                        Message::Threshold,
                    ),
                ]);
                let upper = self.number(
                    (id, index),
                    Field::Upper,
                    "by_cpu_load.upper_threshold",
                    u64::from(
                        r.upper_threshold_percent
                            .unwrap_or(CpuLoadPercent::MAX)
                            .get(),
                    ),
                    100,
                    Message::Upper,
                );
                let mut condition = condition
                    .push(widgets::optional_content(
                        column![widgets::settings_divider(), upper].spacing(design::space::MEDIUM),
                        r.comparison == CpuUsageComparison::Between,
                    ))
                    .push(widgets::settings_divider())
                    .push(self.number(
                        (id, index),
                        Field::Duration,
                        "by_cpu_load.duration",
                        r.duration_seconds,
                        86400,
                        Message::Duration,
                    ));
                if r.comparison != CpuUsageComparison::Else {
                    condition =
                        condition
                            .push(widgets::settings_divider())
                            .push(widgets::setting_row(
                                "by_cpu_load.stability",
                                widgets::switch(
                                    r.stability.is_some(),
                                    Some(move |v| Message::Stability(index, v)),
                                ),
                            ));
                }
                if let Some(stability) = &r.stability {
                    condition = condition
                        .push(widgets::settings_divider())
                        .push(self.number(
                            (id, index),
                            Field::ExitThreshold,
                            if r.comparison == CpuUsageComparison::AtOrBelow {
                                "by_cpu_load.release_above"
                            } else {
                                "by_cpu_load.release_below"
                            },
                            u64::from(stability.exit_threshold_percent.get()),
                            100,
                            Message::ExitThreshold,
                        ));
                    if r.comparison == CpuUsageComparison::Between {
                        condition = condition.push(widgets::settings_divider()).push(
                            self.number(
                                (id, index),
                                Field::ExitUpper,
                                "by_cpu_load.release_above",
                                u64::from(
                                    stability
                                        .exit_upper_threshold_percent
                                        .unwrap_or(CpuLoadPercent::MAX)
                                        .get(),
                                ),
                                100,
                                Message::ExitUpper,
                            ),
                        );
                    }
                    condition = condition
                        .push(widgets::settings_divider())
                        .push(self.number(
                            (id, index),
                            Field::MinimumHold,
                            "by_cpu_load.minimum_hold",
                            stability.minimum_hold_seconds,
                            86400,
                            Message::MinimumHold,
                        ));
                }
                let mut plan_controls = widgets::settings_rows([
                    widgets::setting_row(
                        "by_cpu_load.target_plan",
                        widgets::plan(guid.as_deref(), plans, move |v| Message::Plan(index, v)),
                    )
                    .into(),
                    widgets::setting_row(
                        "by_cpu_load.else",
                        widgets::switch(r.else_enabled, Some(move |v| Message::Else(index, v))),
                    )
                    .into(),
                ]);
                plan_controls = plan_controls.push(widgets::optional_content(
                    column![
                        widgets::settings_divider(),
                        widgets::setting_row(
                            "by_cpu_load.else_plan",
                            widgets::plan(r.else_power_plan_guid.as_deref(), plans, move |v| {
                                Message::ElsePlan(index, v)
                            })
                        )
                    ]
                    .spacing(design::space::MEDIUM),
                    r.else_enabled,
                ));
                controls = controls
                    .push(widgets::heading(
                        t!("by_cpu_load.condition").to_string(),
                        design::typography::BODY,
                    ))
                    .push(widgets::settings_card(condition))
                    .push(widgets::optional_content(
                        text(t!("by_cpu_load.invalid_range").to_string()).style(text::danger),
                        r.comparison == CpuUsageComparison::Between
                            && r.threshold_percent
                                > r.upper_threshold_percent.unwrap_or(CpuLoadPercent::MAX),
                    ))
                    .push(widgets::optional_content(
                        text(t!("by_cpu_load.invalid_stability").to_string()).style(text::danger),
                        r.validate_stability().is_err()
                            || self.inputs.iter().any(|((key, field), value)| {
                                *key == id
                                    && matches!(
                                        field,
                                        Field::ExitThreshold
                                            | Field::ExitUpper
                                            | Field::MinimumHold
                                    )
                                    && !valid_input(*field, value)
                            }),
                    ))
                    .push(widgets::heading(
                        t!("common.power_plan_selection").to_string(),
                        design::typography::BODY,
                    ))
                    .push(widgets::settings_card(plan_controls));
            }
        }

        if kind == Kind::Time {
            controls = controls
                .push(widgets::heading(
                    t!("common.power_plan_selection").to_string(),
                    design::typography::BODY,
                ))
                .push(widgets::settings_card(widgets::setting_row(
                    "by_running_app.power_plan",
                    widgets::plan(guid.as_deref(), plans, move |v| Message::Plan(index, v)),
                )));
        }
        let content = column![
            widgets::heading(
                t!("common.rule_details").to_string(),
                design::typography::BODY
            ),
            widgets::settings_card(
                column![
                    text(t!("common.rule_name").to_string()),
                    text_input(&t!("common.rule_name"), name)
                        .on_input(move |v| Message::Name(index, v))
                        .width(Fill)
                ]
                .spacing(design::space::SMALL)
            ),
            controls
        ]
        .spacing(design::space::MEDIUM);
        let header = row![
            text(
                t!(if self.creating {
                    "common.create"
                } else {
                    "common.edit"
                })
                .to_string()
            )
            .width(Fill),
            icon_button("icons/x.svg").on_press(Message::Cancel)
        ]
        .align_y(iced::Center);
        let footer = row![
            iced::widget::Space::new().width(Fill),
            action_button(t!("common.cancel").to_string())
                .style(widgets::tertiary_button)
                .on_press(Message::Cancel),
            action_button(t!("common.save").to_string())
                .style(widgets::primary_button)
                .on_press_maybe((self.valid() && !name.trim().is_empty()).then_some(Message::Save))
        ]
        .spacing(design::space::SMALL);
        let modal = widgets::modal_frame(
            header,
            scrollable(content)
                .id(match kind {
                    Kind::Time => "by-time-rule-details",
                    Kind::CpuLoad => "by-cpu-load-rule-details",
                })
                .height(Fill),
            footer,
            (800, 760),
        );
        Some(modal)
    }

    fn time_field<'a>(
        &'a self,
        index: usize,
        id: u64,
        start: bool,
        current: RuleTime,
    ) -> Element<'a, Message> {
        let label = if start {
            "by_time.start"
        } else {
            "by_time.end"
        };
        let field = if start { Field::Start } else { Field::End };
        let input = text_input("HH:MM", &self.input(id, field, current.to_string()))
            .on_input(move |value| {
                if start {
                    Message::Start(index, value)
                } else {
                    Message::End(index, value)
                }
            })
            .width(Fill);
        let clock = widgets::tooltip(
            icon_button("icons/clock.svg").on_press(Message::OpenTimePicker(start)),
            text(t!(label).to_string()),
            iced::widget::tooltip::Position::Top,
        );
        row![input, clock].spacing(design::space::SMALL).into()
    }

    fn input(&self, id: u64, field: Field, current: String) -> String {
        self.inputs.get(&(id, field)).cloned().unwrap_or(current)
    }
    fn number(
        &self,
        target: (u64, usize),
        field: Field,
        label: &str,
        value: u64,
        max: u32,
        action: fn(usize, String) -> Message,
    ) -> Element<'static, Message> {
        let (id, index) = target;
        row![
            text(widgets::label_with_unit(
                &t!(label),
                if matches!(field, Field::Duration | Field::MinimumHold) {
                    "s"
                } else {
                    "%"
                }
            ))
            .width(180),
            slider(0..=max, value.min(u64::from(max)) as u32, move |v| action(
                index,
                v.to_string()
            )),
            text_input("", &self.input(id, field, value.to_string()))
                .on_input(move |v| action(index, v))
                .width(design::NUMERIC_WIDTH)
        ]
        .spacing(design::space::COMPACT)
        .height(widgets::SETTING_ROW_HEIGHT)
        .align_y(iced::Center)
        .into()
    }
}
fn initial_stability(rule: &ByCpuLoadRule) -> ByCpuLoadStability {
    ByCpuLoadStability {
        exit_threshold_percent: rule.threshold_percent,
        exit_upper_threshold_percent: (rule.comparison == CpuUsageComparison::Between)
            .then_some(rule.upper_threshold_percent.unwrap_or(CpuLoadPercent::MAX)),
        minimum_hold_seconds: 0,
    }
}
fn valid_input(field: Field, value: &str) -> bool {
    match field {
        Field::Start | Field::End => value.parse::<RuleTime>().is_ok(),
        Field::Threshold | Field::Upper | Field::ExitThreshold | Field::ExitUpper => value
            .parse::<u8>()
            .is_ok_and(|value| CpuLoadPercent::new(value).is_ok()),
        Field::Duration | Field::MinimumHold => value
            .parse::<u64>()
            .is_ok_and(ByCpuLoadRule::duration_is_valid),
    }
}
fn rule_status(
    rule: RuleRef<'_>,
    enabled: bool,
    selected: bool,
    status: &crate::control::power_plan::PowerPlanStatus,
) -> &'static str {
    let (rule_enabled, configured, state, plans) = match rule {
        RuleRef::Time(rule) => (
            rule.enabled,
            !rule.days.is_empty(),
            crate::rules::DecisionState::ByTime,
            [rule.power_plan_guid.as_deref(), None],
        ),
        RuleRef::Cpu(rule) => (
            rule.enabled,
            true,
            crate::rules::DecisionState::ByCpuLoad,
            [
                rule.power_plan_guid.as_deref(),
                rule.else_enabled
                    .then_some(rule.else_power_plan_guid.as_deref())
                    .flatten(),
            ],
        ),
    };
    power_rule_status(
        enabled && rule_enabled,
        configured,
        selected,
        state,
        plans,
        status,
    )
}

pub(super) fn power_rule_status(
    enabled: bool,
    configured: bool,
    selected: bool,
    state: crate::rules::DecisionState,
    plans: [Option<&str>; 2],
    status: &crate::control::power_plan::PowerPlanStatus,
) -> &'static str {
    if !enabled {
        "common.inactive"
    } else if !configured || !plans.iter().flatten().any(|guid| !guid.trim().is_empty()) {
        "common.unknown"
    } else if selected
        && status.owner == Some(crate::control::power_plan::PowerPlanOwner::OrdinaryAutomation)
        && status.decision_state == Some(state)
        && status.apply_failed
        && plans.iter().flatten().any(|guid| {
            status
                .target_guid
                .as_deref()
                .is_some_and(|target| target.eq_ignore_ascii_case(guid))
        })
    {
        "common.error"
    } else if selected
        && status.owner == Some(crate::control::power_plan::PowerPlanOwner::OrdinaryAutomation)
        && status.decision_state == Some(state)
        && plans.iter().flatten().any(|guid| {
            status
                .target_guid
                .as_deref()
                .is_some_and(|target| target.eq_ignore_ascii_case(guid))
                && status
                    .current_guid
                    .as_deref()
                    .is_some_and(|current| current.eq_ignore_ascii_case(guid))
        })
    {
        "common.applied"
    } else if status.current_guid.is_none() {
        "common.unknown"
    } else {
        "common.waiting"
    }
}

#[cfg(test)]
mod tests;
