use super::design;
use super::widgets::{self, checkbox, Choice};
use super::widgets::{button, pick_list, slider, text_input};
use crate::{
    config::{ByCpuLoadRule, ByTimeRule, CpuUsageComparison, Settings, WeekdaySetting},
    power::PowerPlan,
};
use iced::widget::{column, container, row, scrollable, text};
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
}
impl Editor {
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
            Message::Add | Message::Edit(_) => {
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
                self.draft = None;
                self.inputs.clear();
                return;
            }
            Message::Save => {
                if !self.valid() {
                    return;
                }
                if let Some(draft) = self.draft.take() {
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
        _plans: &[PowerPlan],
    ) -> Element<'a, Message> {
        let (enabled, label) = match kind {
            Kind::Time => (s.by_time.enabled, "by_time.enable"),
            Kind::CpuLoad => (s.by_cpu_load.enabled, "by_cpu_load.enable"),
        };
        let mut body = column![
            super::widgets::settings_card(super::widgets::setting_row(
                label,
                super::widgets::switch(enabled, Some(Message::Enabled))
            )),
            text(t!("common.power_plan_priority").to_string()),
            text(t!("common.power_plan_pause_priority").to_string())
        ]
        .spacing(super::widgets::CARD_GAP);
        if kind == Kind::Time {
            body = body.push(text(
                crate::features::power_plan_control::next_by_time_switch_label(&s.by_time),
            ));
        }
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

        let mut cards = Vec::new();
        for (id, index, rule) in rules {
            let (rule_enabled, name) = match &rule {
                RuleRef::Time(r) => (r.enabled, &r.name),
                RuleRef::Cpu(r) => (r.enabled, &r.name),
            };
            let header = row![
                container(
                    checkbox(rule_enabled)
                        .on_toggle_maybe(Some(move |v| Message::RuleEnabled(index, v)))
                )
                .width(48),
                text(name.clone()).width(Fill),
                row![
                    iced::widget::tooltip(
                        button(super::navigation::glyph("icons/pencil.svg"))
                            .padding(7)
                            .width(32)
                            .height(32)
                            .style(widgets::quiet)
                            .on_press(Message::Edit(index)),
                        text(t!("common.edit").to_string()),
                        iced::widget::tooltip::Position::Top
                    ),
                    widgets::rule_delete_button(Some(Message::Remove(index)))
                ]
                .width(80)
                .align_y(iced::Center),
            ]
            .spacing(design::space::MEDIUM)
            .align_y(iced::Center);
            cards.push((
                id,
                column![
                    container(header).padding(widgets::CARD_PADDING as u16),
                    iced::widget::rule::horizontal(1)
                ]
                .into(),
            ));
        }
        let header = row![
            text(t!("common.active").to_string()).width(48),
            text(t!("common.rule_name").to_string()).width(Fill),
            text(t!("common.actions").to_string()).width(80),
        ]
        .spacing(design::space::MEDIUM);
        let rows = if cards.is_empty() {
            container(text(t!("common.no_custom_rules").to_string()).style(text::secondary))
                .padding(design::space::LARGE as u16)
                .into()
        } else {
            iced::widget::keyed_column(cards).into()
        };
        body = body.push(widgets::rules_table(header, rows));
        scrollable(body).height(Fill).into()
    }
    pub(super) fn modal<'a>(
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
        let mut controls = column![].spacing(design::space::COMPACT);
        match rule {
            RuleRef::Time(r) => {
                let mut days = row![].spacing(design::space::SMALL);
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
                controls = controls.push(widgets::settings_card(days)).push(
                    row![
                        container(widgets::settings_card(
                            column![
                                text(t!("by_time.start").to_string()),
                                text_input(
                                    "HH:MM",
                                    &self.input(id, Field::Start, r.start_time.clone())
                                )
                                .on_input(move |v| Message::Start(index, v))
                                .width(Fill)
                            ]
                            .spacing(design::space::SMALL)
                        ))
                        .width(Fill),
                        container(widgets::settings_card(
                            column![
                                text(t!("by_time.end").to_string()),
                                text_input(
                                    "HH:MM",
                                    &self.input(id, Field::End, r.end_time.clone())
                                )
                                .on_input(move |v| Message::End(index, v))
                                .width(Fill)
                            ]
                            .spacing(design::space::SMALL)
                        ))
                        .width(Fill)
                    ]
                    .spacing(design::space::MEDIUM),
                );
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
                controls = controls
                    .push(widgets::settings_card(widgets::setting_row(
                        "by_cpu_load.if",
                        pick_list(choices, selected, move |v| Message::Comparison(index, v.0))
                            .width(design::SELECT_WIDTH),
                    )))
                    .push(self.number(
                        (id, index),
                        Field::Threshold,
                        "by_cpu_load.threshold",
                        u32::from(r.threshold_percent),
                        100,
                        Message::Threshold,
                    ));
                let upper = self.number(
                    (id, index),
                    Field::Upper,
                    "by_cpu_load.upper_threshold",
                    u32::from(r.upper_threshold_percent.unwrap_or(100)),
                    100,
                    Message::Upper,
                );
                controls = controls
                    .push(super::widgets::optional_content(
                        upper,
                        r.comparison == CpuUsageComparison::Between,
                    ))
                    .push(self.number(
                        (id, index),
                        Field::Duration,
                        "by_cpu_load.duration",
                        r.duration_seconds.min(86400) as u32,
                        86400,
                        Message::Duration,
                    ))
                    .push(widgets::settings_card(widgets::setting_row(
                        "by_running_app.power_plan",
                        widgets::plan(guid.clone(), plans, move |v| Message::Plan(index, v)),
                    )))
                    .push(widgets::settings_card(
                        column![
                            widgets::setting_row(
                                "by_cpu_load.else",
                                widgets::switch(
                                    r.else_enabled,
                                    Some(move |v| Message::Else(index, v))
                                )
                            ),
                            widgets::optional_content(
                                widgets::setting_row(
                                    "by_running_app.power_plan",
                                    widgets::plan(
                                        r.else_power_plan_guid.clone(),
                                        plans,
                                        move |v| Message::ElsePlan(index, v)
                                    )
                                ),
                                r.else_enabled
                            )
                        ]
                        .spacing(design::space::MEDIUM),
                    ));
            }
        }

        if kind == Kind::Time {
            controls = controls.push(widgets::settings_card(widgets::setting_row(
                "by_running_app.power_plan",
                widgets::plan(guid.clone(), plans, move |v| Message::Plan(index, v)),
            )));
        }
        let content = column![
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
            button(super::navigation::glyph("icons/x.svg")).on_press(Message::Cancel)
        ]
        .align_y(iced::Center);
        let footer = row![
            iced::widget::Space::new().width(Fill),
            button(text(t!("common.cancel").to_string()))
                .style(widgets::tertiary_button)
                .on_press(Message::Cancel),
            button(text(t!("common.save").to_string()))
                .style(widgets::primary_button)
                .on_press_maybe((self.valid() && !name.trim().is_empty()).then_some(Message::Save))
        ]
        .spacing(design::space::SMALL);
        Some(widgets::modal_frame(
            header,
            scrollable(content).height(Fill),
            footer,
            (800, 760),
        ))
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
        widgets::settings_card(
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
            .align_y(iced::Center),
        )
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
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rule_modal_saves_and_cancels_without_mutating_live_rules() {
        for kind in [Kind::Time, Kind::CpuLoad] {
            let mut editor = Editor::default();
            let mut settings = Settings::default();
            settings.by_time.enabled = true;
            settings.by_cpu_load.enabled = true;
            let before = settings.clone();
            editor.update(kind, &mut settings, &[], Message::Add);
            assert_eq!(settings, before);
            editor.update(kind, &mut settings, &[], Message::Cancel);
            assert_eq!(settings, before);
            editor.update(kind, &mut settings, &[], Message::Add);
            let index = editor.editing;
            editor.update(
                kind,
                &mut settings,
                &[],
                Message::Name(index, "New rule".into()),
            );
            editor.update(kind, &mut settings, &[], Message::Save);
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
