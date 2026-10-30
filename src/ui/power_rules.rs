use super::widgets::{self, Choice, Ui};
use crate::{
    config::{ByCpuLoadRule, ByTimeRule, CpuUsageComparison, Settings, WeekdaySetting},
    power::PowerPlan,
};
use rust_i18n::t;
use std::collections::BTreeMap;
use windui::prelude::{signal, Element, Signal};
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
                let named = self.draft.as_ref().is_some_and(|draft| match kind {
                    Kind::Time => draft
                        .by_time
                        .rules
                        .get(self.editing)
                        .is_some_and(|rule| !rule.name.trim().is_empty()),
                    Kind::CpuLoad => draft
                        .by_cpu_load
                        .rules
                        .get(self.editing)
                        .is_some_and(|rule| !rule.name.trim().is_empty()),
                });
                if !self.valid() || !named {
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
    pub(super) fn view(
        &self,
        kind: Kind,
        settings: &Settings,
        status: &crate::control::power_plan::PowerPlanStatus,
        ui: Ui<Message>,
    ) -> Element {
        let (enabled, title) = match kind {
            Kind::Time => (settings.by_time.enabled, "by_time.enable"),
            Kind::CpuLoad => (settings.by_cpu_load.enabled, "by_cpu_load.enable"),
        };
        let mut body = vec![
            widgets::setting_row(t!(title).to_string(), ui.toggle(enabled, Message::Enabled)),
            widgets::heading(t!("common.rules").to_string()),
        ];
        let rules = match kind {
            Kind::Time => settings
                .by_time
                .rules
                .iter()
                .map(RuleRef::Time)
                .collect::<Vec<_>>(),
            Kind::CpuLoad => settings
                .by_cpu_load
                .rules
                .iter()
                .map(RuleRef::Cpu)
                .collect::<Vec<_>>(),
        };
        if rules.is_empty() {
            body.push(widgets::label(t!("common.no_custom_rules").to_string()));
        }
        for (index, rule) in rules.into_iter().enumerate() {
            let (rule_enabled, name) = match rule {
                RuleRef::Time(rule) => (rule.enabled, rule.name.clone()),
                RuleRef::Cpu(rule) => (rule.enabled, rule.name.clone()),
            };
            let key = rule_status(
                rule,
                settings.general.enabled && enabled,
                status.rule_index == Some(index),
                status,
            );
            body.push(widgets::card(
                name,
                [
                    ui.checkbox(
                        t!("common.enable").to_string(),
                        rule_enabled,
                        move |value| Message::RuleEnabled(index, value),
                    ),
                    widgets::label(t!(key).to_string()),
                    widgets::row().children([
                        ui.button(t!("common.edit").to_string(), Message::Edit(index)),
                        ui.button(t!("common.remove").to_string(), Message::Remove(index)),
                    ]),
                ],
            ));
        }
        widgets::page(body)
    }

    pub(super) fn modal(
        &self,
        kind: Kind,
        plans: &[PowerPlan],
        ui: Ui<Message>,
    ) -> Option<Element> {
        let settings = self.draft.as_ref()?;
        let index = self.editing;
        let id = self.ids.get(index).copied().unwrap_or(index as u64);
        let rule = match kind {
            Kind::Time => RuleRef::Time(settings.by_time.rules.get(index)?),
            Kind::CpuLoad => RuleRef::Cpu(settings.by_cpu_load.rules.get(index)?),
        };
        let (name, guid) = match rule {
            RuleRef::Time(rule) => (&rule.name, &rule.power_plan_guid),
            RuleRef::Cpu(rule) => (&rule.name, &rule.power_plan_guid),
        };
        let name = signal(name.clone());
        let mut native_inputs = Vec::new();
        let mut controls = vec![widgets::setting_row(
            t!("common.rule_name").to_string(),
            ui.input_signal(name, t!("common.rule_name").to_string(), move |value| {
                Message::Name(index, value)
            }),
        )];
        match rule {
            RuleRef::Time(rule) => {
                let mut days = Vec::new();
                for (day, title) in WeekdaySetting::all().into_iter().zip([
                    "weekday.mon",
                    "weekday.tue",
                    "weekday.wed",
                    "weekday.thu",
                    "weekday.fri",
                    "weekday.sat",
                    "weekday.sun",
                ]) {
                    days.push(ui.checkbox(
                        t!(title).to_string(),
                        rule.days.contains(&day),
                        move |value| Message::Day(index, day, value),
                    ));
                }
                controls.push(widgets::row().children(days));
                let start = signal(self.input(id, Field::Start, rule.start_time.clone()));
                native_inputs.push((Field::Start, start));
                controls.push(widgets::setting_row(
                    t!("by_time.start").to_string(),
                    ui.input_signal(start, "HH:MM", move |value| Message::Start(index, value)),
                ));
                let end = signal(self.input(id, Field::End, rule.end_time.clone()));
                native_inputs.push((Field::End, end));
                controls.push(widgets::setting_row(
                    t!("by_time.end").to_string(),
                    ui.input_signal(end, "HH:MM", move |value| Message::End(index, value)),
                ));
            }
            RuleRef::Cpu(rule) => {
                let choices = [
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
                let selected = choices
                    .iter()
                    .find(|choice| choice.0 == rule.comparison)
                    .cloned()
                    .unwrap_or_else(|| choices[0].clone());
                controls.push(widgets::setting_row(
                    t!("by_cpu_load.if").to_string(),
                    ui.select(choices, selected, move |value| {
                        Message::Comparison(index, value.0)
                    }),
                ));
                controls.push(self.number(
                    (id, index),
                    (Field::Threshold, "by_cpu_load.threshold"),
                    u32::from(rule.threshold_percent),
                    Message::Threshold,
                    &mut native_inputs,
                    ui.clone(),
                ));
                if rule.comparison == CpuUsageComparison::Between {
                    controls.push(self.number(
                        (id, index),
                        (Field::Upper, "by_cpu_load.upper_threshold"),
                        u32::from(rule.upper_threshold_percent.unwrap_or(100)),
                        Message::Upper,
                        &mut native_inputs,
                        ui.clone(),
                    ));
                }
                controls.push(self.number(
                    (id, index),
                    (Field::Duration, "by_cpu_load.duration"),
                    rule.duration_seconds.min(86400) as u32,
                    Message::Duration,
                    &mut native_inputs,
                    ui.clone(),
                ));
                controls.push(widgets::setting_row(
                    t!("by_cpu_load.else").to_string(),
                    ui.toggle(rule.else_enabled, move |value| Message::Else(index, value)),
                ));
                if rule.else_enabled {
                    controls.push(widgets::setting_row(
                        t!("by_running_app.power_plan").to_string(),
                        ui.plan(rule.else_power_plan_guid.clone(), plans, move |value| {
                            Message::ElsePlan(index, value)
                        }),
                    ));
                }
            }
        }
        controls.push(widgets::setting_row(
            t!("by_running_app.power_plan").to_string(),
            ui.plan(guid.clone(), plans, move |value| {
                Message::Plan(index, value)
            }),
        ));
        let other_inputs_valid = self.inputs.iter().all(|((owner, field), raw)| {
            (*owner == id && native_inputs.iter().any(|(visible, _)| visible == field))
                || valid_input(*field, raw)
        });
        let footer = widgets::row().children([
            ui.button(t!("common.cancel").to_string(), Message::Cancel),
            ui.primary_button(t!("common.save").to_string(), Message::Save)
                .enabled_when(move || {
                    !name.get().trim().is_empty()
                        && other_inputs_valid
                        && native_inputs
                            .iter()
                            .all(|(field, raw)| valid_input(*field, &raw.get()))
                }),
        ]);
        Some(widgets::editor_panel(
            t!(if self.creating {
                "common.create"
            } else {
                "common.edit"
            })
            .to_string(),
            widgets::col().children(controls),
            footer,
        ))
    }

    fn input(&self, id: u64, field: Field, current: String) -> String {
        self.inputs.get(&(id, field)).cloned().unwrap_or(current)
    }

    fn number(
        &self,
        target: (u64, usize),
        field: (Field, &str),
        value: u32,
        action: fn(usize, String) -> Message,
        native_inputs: &mut Vec<(Field, Signal<String>)>,
        ui: Ui<Message>,
    ) -> Element {
        let (id, index) = target;
        let (field, title) = field;
        let unit = if field == Field::Duration { "s" } else { "%" };
        let max = if field == Field::Duration { 86400 } else { 100 };
        let raw = signal(self.input(id, field, value.to_string()));
        native_inputs.push((field, raw));
        widgets::setting_row(
            format!("{} ({unit})", t!(title)),
            ui.number(raw, 0, max, 1, move |value| action(index, value)),
        )
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
    fn native_save_keeps_unnamed_rule_drafts_open() {
        for kind in [Kind::Time, Kind::CpuLoad] {
            let mut settings = Settings::default();
            settings.by_time.rules.clear();
            settings.by_cpu_load.rules.clear();
            let mut editor = Editor::default();
            editor.update(kind, &mut settings, &[], Message::Add);
            editor.update(kind, &mut settings, &[], Message::Name(0, "  ".into()));
            editor.update(kind, &mut settings, &[], Message::Save);
            assert!(editor.has_pending_editor());
            assert!(settings.by_time.rules.is_empty());
            assert!(settings.by_cpu_load.rules.is_empty());
            editor.update(kind, &mut settings, &[], Message::Name(0, "Rule".into()));
            editor.update(kind, &mut settings, &[], Message::Save);
            assert!(!editor.has_pending_editor());
            assert_eq!(
                settings.by_time.rules.len() + settings.by_cpu_load.rules.len(),
                1
            );
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
