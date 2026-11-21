use std::time::Duration;

use chrono::{
    DateTime, Datelike, Duration as ChronoDuration, Local, LocalResult, NaiveDate, NaiveTime,
    TimeZone,
};

use crate::config::{ByTimeRule, ByTimeSettings, WeekdaySetting};

const BY_TIME_LOOKAHEAD_DAYS: i64 = 8;
const MIN_BY_TIME_DELAY: Duration = Duration::from_secs(1);

#[derive(Debug, Clone)]
pub struct ByTimeDecision {
    pub rule_index: usize,
    pub rule_name: String,
    pub power_plan_guid: String,
}

pub(crate) fn current_decision_at<Tz: TimeZone>(
    settings: &ByTimeSettings,
    now: DateTime<Tz>,
) -> Option<ByTimeDecision>
where
    Tz::Offset: Copy,
{
    if !settings.enabled {
        return None;
    }

    settings
        .rules
        .iter()
        .enumerate()
        .find_map(|(rule_index, rule)| {
            let power_plan_guid = matching_plan(rule, now)?;
            Some(ByTimeDecision {
                rule_index,
                rule_name: rule.name.clone(),
                power_plan_guid: power_plan_guid.to_owned(),
            })
        })
}

pub(crate) fn next_change_delay(
    settings: &ByTimeSettings,
    now: DateTime<Local>,
) -> Option<Duration> {
    next_change_at(settings, now)
        .and_then(|next| next.signed_duration_since(now).to_std().ok())
        .map(|delay| delay.max(MIN_BY_TIME_DELAY))
}

fn matching_plan<Tz: TimeZone>(rule: &ByTimeRule, now: DateTime<Tz>) -> Option<&str> {
    let plan = rule.configured_plan()?;
    let start = rule.start_time.as_naive_time();
    let end = rule.end_time.as_naive_time();

    let today = WeekdaySetting::from_chrono(now.weekday());
    let now_time = now.time();

    let matches = if start < end {
        rule.days.contains(&today) && now_time >= start && now_time < end
    } else {
        let yesterday = WeekdaySetting::from_chrono(now.weekday().pred());
        (rule.days.contains(&today) && now_time >= start)
            || (rule.days.contains(&yesterday) && now_time < end)
    };
    matches.then_some(plan)
}

fn next_change_at(settings: &ByTimeSettings, now: DateTime<Local>) -> Option<DateTime<Local>> {
    if !settings.enabled {
        return None;
    }

    let mut next = None;
    for rule in &settings.rules {
        if rule.configured_plan().is_none() {
            continue;
        }
        let start = rule.start_time.as_naive_time();
        let end = rule.end_time.as_naive_time();

        for day_offset in -1..=BY_TIME_LOOKAHEAD_DAYS {
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

            update_next_datetime(&mut next, local_datetime_after(start_date, start, now), now);
            let end_date = if start < end {
                start_date
            } else if let Some(date) = start_date.checked_add_signed(ChronoDuration::days(1)) {
                date
            } else {
                continue;
            };
            update_next_datetime(&mut next, local_datetime_after(end_date, end, now), now);
        }
    }
    next
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

fn local_datetime_after(
    date: NaiveDate,
    time: NaiveTime,
    now: DateTime<Local>,
) -> Option<DateTime<Local>> {
    resolve_local_datetime_after(Local.from_local_datetime(&date.and_time(time)), now)
}

fn resolve_local_datetime_after<Tz: TimeZone>(
    candidate: LocalResult<DateTime<Tz>>,
    now: DateTime<Tz>,
) -> Option<DateTime<Tz>> {
    match candidate {
        LocalResult::Single(datetime) => (datetime > now).then_some(datetime),
        // Both wall-clock occurrences can change rule applicability during a rollback.
        LocalResult::Ambiguous(first, second) => [first, second]
            .into_iter()
            .filter(|candidate| candidate > &now)
            .min(),
        LocalResult::None => None,
    }
}

#[cfg(test)]
mod tests {
    use chrono::{Local, TimeZone};

    use super::*;

    #[test]
    fn clock_rollback_retains_the_next_occurrence_and_skips_exact_boundaries() {
        let first = DateTime::parse_from_rfc3339("2026-11-01T01:15:00-04:00").unwrap();
        let second = DateTime::parse_from_rfc3339("2026-11-01T01:15:00-05:00").unwrap();
        assert_eq!(first.naive_local(), second.naive_local());
        assert_eq!(second - first, ChronoDuration::hours(1));
        for (now, expected) in [
            (first - ChronoDuration::seconds(1), Some(first)),
            (first, Some(second)),
            (first + ChronoDuration::minutes(30), Some(second)),
            (second - ChronoDuration::seconds(1), Some(second)),
            (second, None),
            (second + ChronoDuration::seconds(1), None),
        ] {
            assert_eq!(
                resolve_local_datetime_after(LocalResult::Ambiguous(first, second), now),
                expected
            );
        }
    }

    #[test]
    fn clock_rollback_rechecks_rules_during_both_clock_occurrences() {
        let settings = ByTimeSettings {
            enabled: true,
            rules: vec![ByTimeRule {
                enabled: true,
                name: "Repeated hour".into(),
                days: vec![WeekdaySetting::Sun],
                start_time: "01:15".parse().unwrap(),
                end_time: "01:30".parse().unwrap(),
                power_plan_guid: Some("scheduled-plan".into()),
            }],
        };
        for offset in ["-04:00", "-05:00"] {
            for (time, active) in [("01:14:59", false), ("01:15:00", true), ("01:30:00", false)] {
                let now =
                    DateTime::parse_from_rfc3339(&format!("2026-11-01T{time}{offset}")).unwrap();
                let decision = current_decision_at(&settings, now);
                assert_eq!(decision.is_some(), active);
                if let Some(decision) = decision {
                    assert_eq!(decision.power_plan_guid, "scheduled-plan");
                }
            }
        }
    }

    #[test]
    fn ordinary_boundaries_are_future_only_and_nonexistent_clocks_stay_unavailable() {
        let boundary = DateTime::parse_from_rfc3339("2026-03-08T03:15:00-04:00").unwrap();
        for (now, expected) in [
            (boundary - ChronoDuration::seconds(1), Some(boundary)),
            (boundary, None),
            (boundary + ChronoDuration::seconds(1), None),
        ] {
            assert_eq!(
                resolve_local_datetime_after(LocalResult::Single(boundary), now),
                expected
            );
        }
        assert_eq!(
            resolve_local_datetime_after(LocalResult::None, boundary),
            None
        );
    }

    #[test]
    fn decisions_skip_unconfigured_rules_and_require_the_selected_plan() {
        let rule = ByTimeRule {
            enabled: true,
            days: vec![WeekdaySetting::Fri],
            start_time: "09:00".parse().unwrap(),
            end_time: "17:00".parse().unwrap(),
            ..Default::default()
        };
        let settings = ByTimeSettings {
            enabled: true,
            rules: vec![
                rule.clone(),
                ByTimeRule {
                    name: "Work".into(),
                    power_plan_guid: Some("work-plan".into()),
                    ..rule
                },
            ],
        };
        let now = Local.with_ymd_and_hms(2026, 5, 29, 12, 0, 0).unwrap();
        let decision = current_decision_at(&settings, now).unwrap();
        assert_eq!(decision.rule_index, 1);
        assert_eq!(decision.rule_name, "Work");
        assert_eq!(decision.power_plan_guid, "work-plan");
    }

    #[test]
    fn overnight_rule_applies_after_midnight_from_previous_day() {
        let rule = ByTimeRule {
            enabled: true,
            name: "Night".to_owned(),
            days: vec![WeekdaySetting::Fri],
            start_time: "22:00".parse().unwrap(),
            end_time: "08:00".parse().unwrap(),
            power_plan_guid: Some("plan-guid".to_owned()),
        };
        let now = Local.with_ymd_and_hms(2026, 5, 30, 2, 0, 0).unwrap();

        assert!(matching_plan(&rule, now).is_some());
    }

    #[test]
    fn disabled_rule_does_not_apply() {
        let rule = ByTimeRule {
            enabled: false,
            name: "Night".to_owned(),
            days: vec![WeekdaySetting::Fri],
            start_time: "22:00".parse().unwrap(),
            end_time: "08:00".parse().unwrap(),
            power_plan_guid: Some("plan-guid".to_owned()),
        };
        let now = Local.with_ymd_and_hms(2026, 5, 30, 2, 0, 0).unwrap();

        assert!(matching_plan(&rule, now).is_none());
    }

    #[test]
    fn rule_without_selected_plan_does_not_apply_or_schedule() {
        let rule = ByTimeRule {
            enabled: true,
            name: "Unconfigured".to_owned(),
            days: vec![WeekdaySetting::Fri],
            start_time: "00:00".parse().unwrap(),
            end_time: "23:59".parse().unwrap(),
            power_plan_guid: None,
        };
        let settings = ByTimeSettings {
            enabled: true,
            rules: vec![rule.clone()],
        };
        let now = Local.with_ymd_and_hms(2026, 5, 29, 12, 0, 0).unwrap();

        assert!(matching_plan(&rule, now).is_none());
        assert!(next_change_at(&settings, now).is_none());
    }
    #[test]
    fn next_change_wakes_at_overnight_rule_end() {
        let settings = ByTimeSettings {
            enabled: true,
            rules: vec![ByTimeRule {
                enabled: true,
                name: "Night".to_owned(),
                days: vec![WeekdaySetting::Fri],
                start_time: "22:00".parse().unwrap(),
                end_time: "08:00".parse().unwrap(),
                power_plan_guid: Some("plan-guid".to_owned()),
            }],
        };
        let now = Local.with_ymd_and_hms(2026, 5, 30, 2, 0, 0).unwrap();

        let next = next_change_at(&settings, now).unwrap();

        assert_eq!(next.date_naive(), now.date_naive());
        assert_eq!(next.time(), NaiveTime::from_hms_opt(8, 0, 0).unwrap());
    }

    #[test]
    fn next_change_finds_next_enabled_day() {
        let settings = ByTimeSettings {
            enabled: true,
            rules: vec![ByTimeRule {
                enabled: true,
                name: "Work".to_owned(),
                days: vec![WeekdaySetting::Mon],
                start_time: "09:00".parse().unwrap(),
                end_time: "17:00".parse().unwrap(),
                power_plan_guid: Some("plan-guid".to_owned()),
            }],
        };
        let now = Local.with_ymd_and_hms(2026, 5, 31, 12, 0, 0).unwrap();

        let starts_at = next_change_at(&settings, now).unwrap();

        assert_eq!(
            starts_at,
            Local.with_ymd_and_hms(2026, 6, 1, 9, 0, 0).unwrap()
        );
        assert_eq!(
            current_decision_at(&settings, starts_at).unwrap().rule_name,
            "Work"
        );
    }
    #[test]
    fn clock_changes_recheck_rules_without_waiting_for_monotonic_deadline() {
        use crate::runtime::scheduler::{RefreshDomain, RefreshScheduler, SchedulerEvent};
        use std::time::Instant;

        let settings = ByTimeSettings {
            enabled: true,
            rules: vec![ByTimeRule {
                enabled: true,
                name: "Morning".into(),
                days: vec![WeekdaySetting::Fri],
                start_time: "09:00".parse().unwrap(),
                end_time: "10:00".parse().unwrap(),
                power_plan_guid: Some("plan".into()),
            }],
        };
        let monotonic = Instant::now();
        let mut scheduler = RefreshScheduler::new(monotonic);
        // Wall time crosses both boundaries in both directions while monotonic time stays fixed.
        for (hour, minute, active) in [
            (8, 30, false),
            (9, 30, true),
            (10, 30, false),
            (9, 30, true),
            (8, 30, false),
        ] {
            scheduler.schedule_after(
                RefreshDomain::PowerPlanCheck,
                monotonic,
                Duration::from_secs(1800),
            );
            assert!(!scheduler.is_due(RefreshDomain::PowerPlanCheck, monotonic));
            scheduler.invalidate(SchedulerEvent::ClockChanged, monotonic);
            assert!(scheduler.is_due(RefreshDomain::PowerPlanCheck, monotonic));
            let wall = Local
                .with_ymd_and_hms(2026, 5, 29, hour, minute, 0)
                .unwrap();
            assert_eq!(current_decision_at(&settings, wall).is_some(), active);
            let next = next_change_at(&settings, wall).unwrap();
            assert!(next > wall);
            scheduler.schedule_after(
                RefreshDomain::PowerPlanCheck,
                monotonic,
                (next - wall).to_std().unwrap(),
            );
            assert!(!scheduler.is_due(RefreshDomain::PowerPlanCheck, monotonic));
        }
    }
}
