use chrono::{DateTime, Datelike, Local, NaiveTime};

use crate::config::{ScheduleModeSettings, ScheduleRule, WeekdaySetting};

#[derive(Debug, Clone)]
pub struct ScheduleDecision {
    pub rule_name: String,
    pub power_plan_guid: Option<String>,
}

#[derive(Debug, Default)]
pub struct Scheduler;

impl Scheduler {
    pub fn current_decision(&self, settings: &ScheduleModeSettings) -> Option<ScheduleDecision> {
        if !settings.enabled {
            return None;
        }

        let now = Local::now();
        if let Some(rule) = settings.rules.iter().find(|rule| rule_applies(rule, now)) {
            return Some(ScheduleDecision {
                rule_name: rule.name.clone(),
                power_plan_guid: rule.power_plan_guid.clone(),
            });
        }

        None
    }

    pub fn next_switch_label(&self, settings: &ScheduleModeSettings) -> String {
        if !settings.enabled || settings.rules.is_empty() {
            return "No active By Time rules".to_owned();
        }

        "Configured By Time rules active".to_owned()
    }
}

fn rule_applies(rule: &ScheduleRule, now: DateTime<Local>) -> bool {
    if !rule.enabled {
        return false;
    }

    let Some((start, end)) = rule.parsed_times() else {
        return false;
    };

    let today = WeekdaySetting::from_chrono(now.weekday());
    let now_time = now.time();

    if start <= end {
        rule.days.contains(&today) && time_in_range(now_time, start, end)
    } else {
        let yesterday = WeekdaySetting::from_chrono(now.weekday().pred());
        (rule.days.contains(&today) && now_time >= start)
            || (rule.days.contains(&yesterday) && now_time < end)
    }
}

fn time_in_range(now: NaiveTime, start: NaiveTime, end: NaiveTime) -> bool {
    now >= start && now < end
}

#[cfg(test)]
mod tests {
    use chrono::{Local, TimeZone};

    use super::*;

    #[test]
    fn overnight_rule_applies_after_midnight_from_previous_day() {
        let rule = ScheduleRule {
            enabled: true,
            name: "Night".to_owned(),
            days: vec![WeekdaySetting::Fri],
            start_time: "22:00".to_owned(),
            end_time: "08:00".to_owned(),
            power_plan_guid: None,
            power_save_guid: None,
            performance_guid: None,
        };
        let now = Local.with_ymd_and_hms(2026, 5, 30, 2, 0, 0).unwrap();

        assert!(rule_applies(&rule, now));
    }

    #[test]
    fn disabled_rule_does_not_apply() {
        let rule = ScheduleRule {
            enabled: false,
            name: "Night".to_owned(),
            days: vec![WeekdaySetting::Fri],
            start_time: "22:00".to_owned(),
            end_time: "08:00".to_owned(),
            power_plan_guid: None,
            power_save_guid: None,
            performance_guid: None,
        };
        let now = Local.with_ymd_and_hms(2026, 5, 30, 2, 0, 0).unwrap();

        assert!(!rule_applies(&rule, now));
    }
}
