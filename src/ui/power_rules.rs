use super::design;
use super::widgets::{self, checkbox, icon_button, Choice};
use super::widgets::{action_button, pick_list, slider, text_input};
use crate::ui::scrolling::scrollable;
use crate::{
    config::{ByCpuLoadRule, ByTimeRule, CpuUsageComparison, Settings, WeekdaySetting},
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
                    &rule.start_time
                } else {
                    &rule.end_time
                };
                self.picker_time = super::pickers::TimeDraft::new(
                    chrono::NaiveTime::parse_from_str(value, "%H:%M")
                        .unwrap_or(chrono::NaiveTime::MIN),
                );
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
                    start_time: "22:00".into(),
                    end_time: "08:00".into(),
                    power_plan_guid: active(),
                }),
                Kind::CpuLoad => s.by_cpu_load.rules.push(ByCpuLoadRule {
                    enabled: true,
                    name: t!("by_cpu_load.new_rule").to_string(),
                    comparison: CpuUsageComparison::AtOrBelow,
                    threshold_percent: 20,
                    upper_threshold_percent: None,
                    duration_seconds: 30,
                    power_plan_guid: active(),
                    else_enabled: false,
                    else_power_plan_guid: active(),
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
                if let Some(r) = s.by_time.rules.get_mut(i) {
                    r.start_time = v
                }
            }
            Message::End(i, v) => {
                if let Some(r) = s.by_time.rules.get_mut(i) {
                    r.end_time = v
                }
            }
            Message::Comparison(i, v) => {
                if let Some(r) = s.by_cpu_load.rules.get_mut(i) {
                    r.comparison = v;
                    if v == CpuUsageComparison::Between {
                        r.upper_threshold_percent.get_or_insert(100);
                    }
                }
            }
            Message::Threshold(i, v) => {
                if let (Some(r), Ok(v)) = (s.by_cpu_load.rules.get_mut(i), v.parse::<u8>()) {
                    r.threshold_percent = v
                }
            }
            Message::Upper(i, v) => {
                if let (Some(r), Ok(v)) = (s.by_cpu_load.rules.get_mut(i), v.parse::<u8>()) {
                    r.upper_threshold_percent = Some(v)
                }
            }
            Message::Duration(i, v) => {
                if let (Some(r), Ok(v)) = (s.by_cpu_load.rules.get_mut(i), v.parse::<u64>()) {
                    r.duration_seconds = v
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
                                    self.time_field(index, id, true, &r.start_time)
                                ]
                                .spacing(design::space::SMALL)
                            )
                            .width(Fill),
                            container(
                                column![
                                    text(t!("by_time.end").to_string()),
                                    self.time_field(index, id, false, &r.end_time)
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
                        u32::from(r.threshold_percent),
                        100,
                        Message::Threshold,
                    ),
                ]);
                let upper = self.number(
                    (id, index),
                    Field::Upper,
                    "by_cpu_load.upper_threshold",
                    u32::from(r.upper_threshold_percent.unwrap_or(100)),
                    100,
                    Message::Upper,
                );
                let condition = condition
                    .push(widgets::optional_content(
                        column![widgets::settings_divider(), upper].spacing(design::space::MEDIUM),
                        r.comparison == CpuUsageComparison::Between,
                    ))
                    .push(widgets::settings_divider())
                    .push(self.number(
                        (id, index),
                        Field::Duration,
                        "by_cpu_load.duration",
                        r.duration_seconds.min(86400) as u32,
                        86400,
                        Message::Duration,
                    ));
                let plans = widgets::settings_rows([
                    widgets::setting_row(
                        "by_running_app.power_plan",
                        widgets::plan(guid.clone(), plans, move |v| Message::Plan(index, v)),
                    )
                    .into(),
                    widgets::setting_row(
                        "by_cpu_load.else",
                        widgets::switch(r.else_enabled, Some(move |v| Message::Else(index, v))),
                    )
                    .into(),
                ])
                .push(widgets::optional_content(
                    column![
                        widgets::settings_divider(),
                        widgets::setting_row(
                            "by_running_app.power_plan",
                            widgets::plan(r.else_power_plan_guid.clone(), plans, move |v| {
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
                    .push(widgets::heading(
                        t!("common.power_plan_selection").to_string(),
                        design::typography::BODY,
                    ))
                    .push(widgets::settings_card(plans));
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
                    widgets::plan(guid.clone(), plans, move |v| Message::Plan(index, v)),
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
        let modal =
            widgets::modal_frame(header, scrollable(content).height(Fill), footer, (800, 760));
        Some(modal)
    }

    fn time_field<'a>(
        &'a self,
        index: usize,
        id: u64,
        start: bool,
        current: &str,
    ) -> Element<'a, Message> {
        let label = if start {
            "by_time.start"
        } else {
            "by_time.end"
        };
        let field = if start { Field::Start } else { Field::End };
        let input = text_input("HH:MM", &self.input(id, field, current.to_owned()))
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
        value: u32,
        max: u32,
        action: fn(usize, String) -> Message,
    ) -> Element<'static, Message> {
        let (id, index) = target;
        row![
            text(widgets::label_with_unit(
                &t!(label),
                if field == Field::Duration { "s" } else { "%" }
            ))
            .width(180),
            slider(0..=max, value, move |v| action(index, v.to_string())),
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
fn valid_input(field: Field, value: &str) -> bool {
    match field {
        Field::Start | Field::End => chrono::NaiveTime::parse_from_str(value, "%H:%M").is_ok(),
        Field::Threshold | Field::Upper => value.parse::<u32>().is_ok_and(|v| v <= 100),
        Field::Duration => value.parse::<u32>().is_ok_and(|v| v <= 86400),
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
            !rule.days.is_empty() && rule.parsed_times().is_some(),
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
mod tests {
    use super::*;

    #[test]
    fn time_picker_preserves_typed_drafts_and_only_saves_submitted_times() {
        let mut editor = Editor::default();
        let mut settings = Settings::default();
        settings.by_time.rules.clear();
        let update = |editor: &mut Editor, settings: &mut Settings, message| {
            editor.update(Kind::Time, settings, &[], message);
        };
        update(&mut editor, &mut settings, Message::Add);
        update(&mut editor, &mut settings, Message::Start(0, "2:".into()));
        assert!(!editor.valid());
        update(&mut editor, &mut settings, Message::OpenTimePicker(true));
        update(&mut editor, &mut settings, Message::CancelTimePicker);
        assert!(!editor.valid());
        assert_eq!(
            editor.draft.as_ref().unwrap().by_time.rules[0].start_time,
            "22:00"
        );
        let time = chrono::NaiveTime::from_hms_opt(0, 5, 0).unwrap();
        update(&mut editor, &mut settings, Message::TimePicked(true, time));
        assert!(
            !editor.valid(),
            "dismissed picker must not replace typed text"
        );
        update(&mut editor, &mut settings, Message::OpenTimePicker(true));
        assert_eq!(editor.picker_time.hour, "22");
        update(&mut editor, &mut settings, Message::TimePicked(true, time));
        assert!(
            editor.valid(),
            "drafts: {:?}, ids: {:?}",
            editor.inputs,
            editor.ids
        );
        assert!(settings.by_time.rules.is_empty());
        assert_eq!(
            editor.draft.as_ref().unwrap().by_time.rules[0].start_time,
            "00:05"
        );
        update(&mut editor, &mut settings, Message::OpenTimePicker(false));
        let time = chrono::NaiveTime::from_hms_opt(23, 59, 0).unwrap();
        update(&mut editor, &mut settings, Message::TimePicked(false, time));
        update(&mut editor, &mut settings, Message::Save);
        assert_eq!(settings.by_time.rules[0].start_time, "00:05");
        assert_eq!(settings.by_time.rules[0].end_time, "23:59");
        update(&mut editor, &mut settings, Message::Edit(0));
        update(&mut editor, &mut settings, Message::OpenTimePicker(true));
        editor.dismiss_picker();
        update(&mut editor, &mut settings, Message::TimePicked(true, time));
        update(&mut editor, &mut settings, Message::Cancel);
        assert_eq!(settings.by_time.rules[0].start_time, "00:05");
    }

    #[test]
    fn by_time_dialog_stays_centered_with_picker_open_or_closed() {
        use iced::advanced::{layout, widget::Tree, Layout};
        use iced::{Rectangle, Size};
        fn find_frame(layout: Layout<'_>, size: Size) -> Option<Rectangle> {
            if layout.bounds().size() == size {
                return Some(layout.bounds());
            }
            layout.children().find_map(|child| find_frame(child, size))
        }
        let renderer = iced::Renderer::new(design::typography::FONT, iced::Pixels(14.0));
        let mut editor = Editor::default();
        let mut settings = Settings::default();
        editor.update(Kind::Time, &mut settings, &[], Message::Edit(0));
        for (width, height) in [(900.0f32, 620.0f32), (1400.0, 1000.0)] {
            for open in [false, true] {
                if open {
                    editor.update(
                        Kind::Time,
                        &mut settings,
                        &[],
                        Message::OpenTimePicker(true),
                    );
                } else {
                    editor.dismiss_picker();
                }
                let mut view: Element<'_, Message> =
                    container(editor.modal(Kind::Time, &[]).unwrap())
                        .padding(16)
                        .center_x(Fill)
                        .center_y(Fill)
                        .into();
                let mut tree = Tree::new(&view);
                let node = view.as_widget_mut().layout(
                    &mut tree,
                    &renderer,
                    &layout::Limits::new(Size::ZERO, Size::new(width, height)),
                );
                let frame = find_frame(
                    Layout::new(&node),
                    Size::new((width - 32.0).min(800.0), (height - 32.0).min(760.0)),
                )
                .expect("visible dialog frame");
                assert_eq!(frame.center(), iced::Point::new(width / 2.0, height / 2.0));
            }
        }
    }

    #[test]
    fn rule_tables_align_columns_and_keep_controls_working_at_both_widths() {
        use iced::advanced::{
            layout,
            renderer::{Headless as _, Renderer as _},
            widget::Tree,
            Layout, Shell,
        };
        use iced::{mouse, Event, Rectangle, Size};
        fn find_table(layout: Layout<'_>, count: usize) -> Option<Layout<'_>> {
            if layout.children().len() == count {
                Some(layout)
            } else {
                layout.children().find_map(|child| find_table(child, count))
            }
        }
        let editor = Editor::default();
        let mut settings = Settings::default();
        settings.by_time.rules = vec![
            ByTimeRule {
                enabled: true,
                name: "Morning profile".into(),
                ..Default::default()
            },
            ByTimeRule {
                enabled: true,
                name: "A longer rule name that should stay inside its column".into(),
                ..Default::default()
            },
        ];
        settings.by_cpu_load.rules = ["CPU rule", "Second CPU rule"]
            .into_iter()
            .map(|name| ByCpuLoadRule {
                enabled: true,
                name: name.into(),
                comparison: CpuUsageComparison::AtOrAbove,
                threshold_percent: 50,
                upper_threshold_percent: None,
                duration_seconds: 10,
                power_plan_guid: Some("plan".into()),
                else_enabled: false,
                else_power_plan_guid: None,
            })
            .collect();
        let status = crate::control::power_plan::PowerPlanStatus::default();
        for (name, theme) in [
            (
                "light",
                iced::Theme::custom_with_fn(
                    "probe",
                    design::palette(true, None),
                    design::extended_palette,
                ),
            ),
            (
                "dark",
                iced::Theme::custom_with_fn(
                    "probe",
                    design::palette(false, None),
                    design::extended_palette,
                ),
            ),
            ("amoled", design::amoled_theme(None)),
        ] {
            for width in [430, 1000] {
                for kind in [Kind::Time, Kind::CpuLoad] {
                    for count in [0, 2] {
                        let mut settings = settings.clone();
                        settings.by_time.rules.truncate(count);
                        settings.by_cpu_load.rules.truncate(count);
                        let mut renderer =
                            iced::Renderer::new(design::typography::FONT, iced::Pixels(14.0));
                        let mut view = editor.view(kind, &settings, &status);
                        let mut tree = Tree::new(&view);
                        let bounds = Rectangle::with_size(Size::new(width as f32, 400.0));
                        let node = view.as_widget_mut().layout(
                            &mut tree,
                            &renderer,
                            &layout::Limits::new(Size::ZERO, bounds.size()),
                        );
                        assert_eq!(node.bounds().width, width as f32);
                        let table = find_table(Layout::new(&node), 4 * (count + 1))
                            .expect("header and rule rows");
                        let cells: Vec<_> = table.children().collect();
                        assert_eq!(
                            table.bounds().width,
                            (width as f32 - 14.0).max(600.0) - 20.0
                        );
                        for row in (4..cells.len()).step_by(4) {
                            for column in 0..3 {
                                assert_eq!(
                                    cells[column].bounds().x,
                                    cells[row + column].bounds().x
                                );
                            }
                            assert_eq!(cells[row].bounds().width, 48.0);
                            assert_eq!(cells[row + 1].bounds().width, 140.0);
                            assert_eq!(cells[row + 3].bounds().width, 80.0);
                        }
                        view.as_widget_mut().update(
                            &mut tree,
                            &Event::Window(iced::window::Event::RedrawRequested(
                                iced::time::Instant::now(),
                            )),
                            Layout::new(&node),
                            mouse::Cursor::Unavailable,
                            &renderer,
                            &mut iced::advanced::clipboard::Null,
                            &mut Shell::new(&mut Vec::new()),
                            &bounds,
                        );
                        renderer.reset(bounds);
                        view.as_widget().draw(
                            &tree,
                            &mut renderer,
                            &theme,
                            &iced::advanced::renderer::Style {
                                text_color: theme.palette().text,
                            },
                            Layout::new(&node),
                            mouse::Cursor::Unavailable,
                            &bounds,
                        );
                        let pixels = renderer.screenshot(
                            Size::new(width, 400),
                            1.0,
                            theme.palette().background,
                        );
                        std::fs::create_dir_all("target/ui-preview").unwrap();
                        image::save_buffer(
                            format!(
                                "target/ui-preview/rule-table-{kind:?}-{name}-{width}-{count}.png"
                            ),
                            &pixels,
                            width,
                            400,
                            image::ColorType::Rgba8,
                        )
                        .unwrap();
                        if count == 0 {
                            continue;
                        }
                        let toggle = cells[4].children().next().unwrap().bounds().center();
                        let edit = cells[11]
                            .children()
                            .next()
                            .unwrap()
                            .children()
                            .next()
                            .unwrap()
                            .bounds()
                            .center();
                        let remove = cells[7]
                            .children()
                            .next()
                            .unwrap()
                            .children()
                            .last()
                            .unwrap()
                            .bounds()
                            .center();
                        for (cursor, action) in [(toggle, 0), (edit, 1), (remove, 2)] {
                            if action != 0 && width == 430 {
                                continue;
                            }
                            let mut messages = Vec::new();
                            for event in [
                                mouse::Event::ButtonPressed(mouse::Button::Left),
                                mouse::Event::ButtonReleased(mouse::Button::Left),
                            ] {
                                view.as_widget_mut().update(
                                    &mut tree,
                                    &Event::Mouse(event),
                                    Layout::new(&node),
                                    mouse::Cursor::Available(cursor),
                                    &renderer,
                                    &mut iced::advanced::clipboard::Null,
                                    &mut Shell::new(&mut messages),
                                    &bounds,
                                );
                            }
                            assert!(
                                matches!(
                                    (action, messages.as_slice()),
                                    (0, [Message::RuleEnabled(0, false)])
                                        | (1, [Message::Edit(1)])
                                        | (2, [Message::Remove(0)])
                                ),
                                "unexpected rule action: {messages:?}"
                            );
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn cpu_rule_status_tracks_selected_else_plan_and_failed_switches() {
        use crate::control::power_plan::{PowerPlanOwner, PowerPlanStatus};
        let mut rule = ByCpuLoadRule {
            enabled: true,
            power_plan_guid: Some("main".into()),
            else_enabled: true,
            else_power_plan_guid: Some("else".into()),
            name: "Rule".into(),
            comparison: CpuUsageComparison::AtOrAbove,
            threshold_percent: 75,
            upper_threshold_percent: None,
            duration_seconds: 0,
        };
        let mut status = PowerPlanStatus {
            owner: Some(PowerPlanOwner::OrdinaryAutomation),
            decision_state: Some(crate::rules::DecisionState::ByCpuLoad),
            target_guid: Some("else".into()),
            current_guid: Some("ELSE".into()),
            ..Default::default()
        };
        assert_eq!(
            rule_status(RuleRef::Cpu(&rule), true, true, &status),
            "common.applied"
        );
        assert_eq!(
            rule_status(RuleRef::Cpu(&rule), true, false, &status),
            "common.waiting"
        );
        assert_eq!(
            rule_status(RuleRef::Cpu(&rule), false, true, &status),
            "common.inactive"
        );
        status.current_guid = Some("other".into());
        assert_eq!(
            rule_status(RuleRef::Cpu(&rule), true, true, &status),
            "common.waiting"
        );
        rule.power_plan_guid = None;
        rule.else_enabled = false;
        assert_eq!(
            rule_status(RuleRef::Cpu(&rule), true, true, &status),
            "common.unknown"
        );
    }

    #[test]
    fn time_rule_status_requires_enabled_and_confirmed_runtime_plan() {
        use crate::control::power_plan::{PowerPlanOwner, PowerPlanStatus};
        let rule = ByTimeRule {
            enabled: true,
            days: vec![WeekdaySetting::from_chrono(chrono::Weekday::Fri)],
            start_time: "22:00".into(),
            end_time: "08:00".into(),
            power_plan_guid: Some("plan".into()),
            ..Default::default()
        };
        let mut status = PowerPlanStatus {
            owner: Some(PowerPlanOwner::OrdinaryAutomation),
            decision_state: Some(crate::rules::DecisionState::ByTime),
            target_guid: Some("plan".into()),
            current_guid: Some("other".into()),
            ..Default::default()
        };
        assert_eq!(
            rule_status(RuleRef::Time(&rule), true, true, &status),
            "common.waiting"
        );
        status.apply_failed = true;
        assert_eq!(
            rule_status(RuleRef::Time(&rule), true, true, &status),
            "common.error"
        );
        status.apply_failed = false;
        status.current_guid = Some("PLAN".into());
        assert_eq!(
            rule_status(RuleRef::Time(&rule), true, true, &status),
            "common.applied"
        );
        assert_eq!(
            rule_status(RuleRef::Time(&rule), false, true, &status),
            "common.inactive"
        );
        assert_eq!(
            rule_status(RuleRef::Time(&rule), true, false, &status),
            "common.waiting"
        );
        status.owner = Some(PowerPlanOwner::AdaptiveEngine);
        assert_eq!(
            rule_status(RuleRef::Time(&rule), true, true, &status),
            "common.waiting"
        );
        status.owner = Some(PowerPlanOwner::OrdinaryAutomation);
        status.decision_state = Some(crate::rules::DecisionState::Disabled);
        assert_eq!(
            rule_status(RuleRef::Time(&rule), true, true, &status),
            "common.waiting"
        );
    }

    #[test]
    fn rule_modal_saves_and_cancels_without_mutating_live_rules() {
        for kind in [Kind::Time, Kind::CpuLoad] {
            let mut editor = Editor::default();
            let mut settings = Settings::default();
            settings.by_time.enabled = true;
            settings.by_cpu_load.enabled = true;
            let before = settings.clone();
            editor.update(kind, &mut settings, &[], Message::Add);
            assert!(editor.has_pending_editor());
            assert_eq!(settings, before);
            editor.update(kind, &mut settings, &[], Message::Cancel);
            assert!(!editor.has_pending_editor());
            assert_eq!(settings, before);
            editor.update(kind, &mut settings, &[], Message::Add);
            assert!(editor.has_pending_editor());
            let index = editor.editing;
            editor.update(
                kind,
                &mut settings,
                &[],
                Message::Name(index, "New rule".into()),
            );
            editor.update(kind, &mut settings, &[], Message::Save);
            assert!(!editor.has_pending_editor());
            let saved = settings.clone();
            assert_ne!(saved, before);
            editor.update(kind, &mut settings, &[], Message::Edit(index));
            editor.update(
                kind,
                &mut settings,
                &[],
                Message::Name(index, "Changed".into()),
            );
            assert_eq!(settings, saved);
            editor.update(kind, &mut settings, &[], Message::Cancel);
            assert!(!editor.has_pending_editor());
            assert_eq!(settings, saved);
        }
    }

    #[test]
    fn drafts_do_not_publish_invalid_values_and_confirmed_removal_is_immediate() {
        let mut e = Editor::default();
        let mut s = Settings::default();
        let old = s.by_time.rules[0].start_time.clone();
        e.update(Kind::Time, &mut s, &[], Message::Start(0, "25:00".into()));
        assert!(!e.valid());
        assert_eq!(s.by_time.rules[0].start_time, old);
        e.update(Kind::Time, &mut s, &[], Message::Start(0, "23:30".into()));
        assert!(e.valid());
        e.update(Kind::Time, &mut s, &[], Message::Remove(0));
        assert!(s.by_time.rules.is_empty());
    }
    #[test]
    fn cpu_rules_keep_per_rule_plans_and_typed_threshold_limits() {
        let mut e = Editor::default();
        let mut s = Settings::default();
        s.by_cpu_load.enabled = true;
        s.by_cpu_load.rules.clear();
        e.update(Kind::CpuLoad, &mut s, &[], Message::Add);
        e.update(Kind::CpuLoad, &mut s, &[], Message::Save);
        e.update(
            Kind::CpuLoad,
            &mut s,
            &[],
            Message::Threshold(0, "101".into()),
        );
        assert_eq!(s.by_cpu_load.rules[0].threshold_percent, 20);
        assert!(!e.valid());
        e.update(
            Kind::CpuLoad,
            &mut s,
            &[],
            Message::Threshold(0, "75".into()),
        );
        assert!(e.valid());
        e.update(
            Kind::CpuLoad,
            &mut s,
            &[],
            Message::Plan(0, Some("main".into())),
        );
        e.update(
            Kind::CpuLoad,
            &mut s,
            &[],
            Message::ElsePlan(0, Some("else".into())),
        );
        assert_eq!(
            s.by_cpu_load.rules[0].power_plan_guid.as_deref(),
            Some("main")
        );
        assert_eq!(
            s.by_cpu_load.rules[0].else_power_plan_guid.as_deref(),
            Some("else")
        );
    }
}
