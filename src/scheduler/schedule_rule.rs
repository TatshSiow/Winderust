use std::time::Duration;

use chrono::{
    DateTime, Datelike, Duration as ChronoDuration, Local, LocalResult, NaiveDate, NaiveTime,
    TimeZone,
};

use crate::config::{ScheduleModeSettings, ScheduleRule, WeekdaySetting};

const SCHEDULE_LOOKAHEAD_DAYS: i64 = 8;
const MIN_SCHEDULE_DELAY: Duration = Duration::from_secs(1);

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
        active_rule(settings, now).map(|rule| ScheduleDecision {
            rule_name: rule.name.clone(),
            power_plan_guid: rule.power_plan_guid.clone(),
        })
    }

    pub fn next_change_delay(&self, settings: &ScheduleModeSettings) -> Option<Duration> {
        let now = Local::now();
        next_change_at(settings, now)
            .and_then(|next| next.signed_duration_since(now).to_std().ok())
            .map(|delay| delay.max(MIN_SCHEDULE_DELAY))
    }

    pub fn next_switch_label(&self, settings: &ScheduleModeSettings) -> String {
        if !settings.enabled || settings.rules.is_empty() {
            return "No active By Time rules".to_owned();
        }

        let now = Local::now();
        if let Some(rule) = active_rule(settings, now) {
            if let Some(ends_at) = next_rule_end_after(rule, now) {
                return format!(
                    "By Time '{}' active until {}.",
                    rule_display_name(rule),
                    switch_time_label(ends_at, now)
                );
            }

            return format!("By Time '{}' active.", rule_display_name(rule));
        }

        if let Some((rule_name, starts_at)) = next_rule_start_after(settings, now) {
            return format!(
                "Next By Time rule '{}' at {}.",
                rule_name,
                switch_time_label(starts_at, now)
            );
        }

        "No upcoming By Time rules".to_owned()
    }
}

fn active_rule(settings: &ScheduleModeSettings, now: DateTime<Local>) -> Option<&ScheduleRule> {
    settings.rules.iter().find(|rule| rule_applies(rule, now))
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

fn next_change_at(
    settings: &ScheduleModeSettings,
    now: DateTime<Local>,
) -> Option<DateTime<Local>> {
    if !settings.enabled {
        return None;
    }

    let mut next = None;
    for rule in &settings.rules {
        let Some((start, end)) = enabled_rule_times(rule) else {
            continue;
        };

        for day_offset in -1..=SCHEDULE_LOOKAHEAD_DAYS {
            let Some(start_date) = now
                .date_naive()
                .checked_add_signed(ChronoDuration::days(day_offset))
            else {
                continue;
            };
            if !rule
                .days
                .contains(&WeekdaySetting::from_chrono(start_date.weekday()))
            {
                continue;
            }

            update_next_datetime(&mut next, local_datetime(start_date, start), now);
            let end_date = if start <= end {
                start_date
            } else if let Some(date) = start_date.checked_add_signed(ChronoDuration::days(1)) {
                date
            } else {
                continue;
            };
            update_next_datetime(&mut next, local_datetime(end_date, end), now);
        }
    }
    next
}

fn next_rule_start_after(
    settings: &ScheduleModeSettings,
    now: DateTime<Local>,
) -> Option<(String, DateTime<Local>)> {
    let mut next = None;
    for rule in &settings.rules {
        let Some((start, _end)) = enabled_rule_times(rule) else {
            continue;
        };

        for day_offset in 0..=SCHEDULE_LOOKAHEAD_DAYS {
            let Some(start_date) = now
                .date_naive()
                .checked_add_signed(ChronoDuration::days(day_offset))
            else {
                continue;
            };
            if !rule
                .days
                .contains(&WeekdaySetting::from_chrono(start_date.weekday()))
            {
                continue;
            }

            let Some(starts_at) = local_datetime(start_date, start) else {
                continue;
            };
            if starts_at <= now {
                continue;
            }
            if next
                .as_ref()
                .is_none_or(|(_name, next_at)| starts_at < *next_at)
            {
                next = Some((rule_display_name(rule).to_owned(), starts_at));
            }
        }
    }
    next
}

fn next_rule_end_after(rule: &ScheduleRule, now: DateTime<Local>) -> Option<DateTime<Local>> {
    let (start, end) = enabled_rule_times(rule)?;
    let mut next = None;
    for day_offset in -1..=1 {
        let Some(start_date) = now
            .date_naive()
            .checked_add_signed(ChronoDuration::days(day_offset))
        else {
            continue;
        };
        if !rule
            .days
            .contains(&WeekdaySetting::from_chrono(start_date.weekday()))
        {
            continue;
        }

        let end_date = if start <= end {
            start_date
        } else if let Some(date) = start_date.checked_add_signed(ChronoDuration::days(1)) {
            date
        } else {
            continue;
        };
        update_next_datetime(&mut next, local_datetime(end_date, end), now);
    }
    next
}

fn enabled_rule_times(rule: &ScheduleRule) -> Option<(NaiveTime, NaiveTime)> {
    rule.enabled.then(|| rule.parsed_times()).flatten()
}

fn update_next_datetime(
    next: &mut Option<DateTime<Local>>,
    candidate: Option<DateTime<Local>>,
    now: DateTime<Local>,
) {
    let Some(candidate) = candidate else {
        return;
    };
    if candidate > now && next.is_none_or(|next| candidate < next) {
        *next = Some(candidate);
    }
}

fn local_datetime(date: NaiveDate, time: NaiveTime) -> Option<DateTime<Local>> {
    match Local.from_local_datetime(&date.and_time(time)) {
        LocalResult::Single(datetime) => Some(datetime),
        LocalResult::Ambiguous(earliest, _) => Some(earliest),
        LocalResult::None => None,
    }
}

fn rule_display_name(rule: &ScheduleRule) -> &str {
    let name = rule.name.trim();
    if name.is_empty() {
        "Unnamed rule"
    } else {
        name
    }
}

fn switch_time_label(at: DateTime<Local>, now: DateTime<Local>) -> String {
    let time = at.format("%H:%M");
    let today = now.date_naive();
    let tomorrow = today.checked_add_signed(ChronoDuration::days(1));
    if at.date_naive() == today {
        time.to_string()
    } else if Some(at.date_naive()) == tomorrow {
        format!("tomorrow {time}")
    } else {
        format!(
            "{} {time}",
            WeekdaySetting::from_chrono(at.weekday()).short_label()
        )
    }
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
        };
        let now = Local.with_ymd_and_hms(2026, 5, 30, 2, 0, 0).unwrap();

        assert!(!rule_applies(&rule, now));
    }

    #[test]
    fn next_change_wakes_at_overnight_rule_end() {
        let settings = ScheduleModeSettings {
            enabled: true,
            rules: vec![ScheduleRule {
                enabled: true,
                name: "Night".to_owned(),
                days: vec![WeekdaySetting::Fri],
                start_time: "22:00".to_owned(),
                end_time: "08:00".to_owned(),
                power_plan_guid: None,
            }],
            power_plans: Default::default(),
        };
        let now = Local.with_ymd_and_hms(2026, 5, 30, 2, 0, 0).unwrap();

        let next = next_change_at(&settings, now).unwrap();

        assert_eq!(next.date_naive(), now.date_naive());
        assert_eq!(next.time(), NaiveTime::from_hms_opt(8, 0, 0).unwrap());
    }

    #[test]
    fn next_rule_start_finds_next_enabled_day() {
        let settings = ScheduleModeSettings {
            enabled: true,
            rules: vec![ScheduleRule {
                enabled: true,
                name: "Work".to_owned(),
                days: vec![WeekdaySetting::Mon],
                start_time: "09:00".to_owned(),
                end_time: "17:00".to_owned(),
                power_plan_guid: None,
            }],
            power_plans: Default::default(),
        };
        let now = Local.with_ymd_and_hms(2026, 5, 31, 12, 0, 0).unwrap();

        let (name, starts_at) = next_rule_start_after(&settings, now).unwrap();

        assert_eq!(name, "Work");
        assert_eq!(starts_at.weekday(), chrono::Weekday::Mon);
        assert_eq!(starts_at.time(), NaiveTime::from_hms_opt(9, 0, 0).unwrap());
    }
}
