use super::*;
use crate::config::{ByCpuLoadRule, ByCpuLoadStability, CpuLoadPercent, CpuUsageComparison};
use std::time::Duration;

fn rule_with_else(comparison: CpuUsageComparison) -> ByCpuLoadRule {
    ByCpuLoadRule {
        enabled: true,
        name: "CPU load".into(),
        comparison,
        threshold_percent: CpuLoadPercent::new(75).unwrap(),
        upper_threshold_percent: Some(CpuLoadPercent::new(90).unwrap()),
        duration_seconds: 0,
        power_plan_guid: Some("matched-plan".into()),
        else_enabled: true,
        else_power_plan_guid: Some("else-plan".into()),
        stability: None,
    }
}

#[test]
fn unavailable_cpu_observations_cannot_select_a_condition_or_else_plan() {
    for comparison in [
        CpuUsageComparison::AtOrAbove,
        CpuUsageComparison::AtOrBelow,
        CpuUsageComparison::Between,
        CpuUsageComparison::Else,
    ] {
        let settings = ByCpuLoadSettings {
            enabled: true,
            rules: vec![rule_with_else(comparison)],
        };
        let mut scheduler = ByCpuLoadScheduler::default();
        for usage in [
            None,
            Some(f32::NAN),
            Some(f32::INFINITY),
            Some(f32::NEG_INFINITY),
            Some(-0.01),
            Some(100.01),
        ] {
            assert!(scheduler.current_decision(&settings, usage).is_none());
            assert!(scheduler.matched_since.is_empty());
            assert!(scheduler.last_rules.is_empty());
        }
        for usage in [0.0, 100.0] {
            let decision = scheduler.current_decision(&settings, Some(usage)).unwrap();
            assert_eq!(decision.usage_percent, usage);
        }
    }
}

#[test]
fn unavailable_cpu_observations_restart_the_sustained_condition_interval() {
    let mut rule = rule_with_else(CpuUsageComparison::AtOrAbove);
    rule.duration_seconds = 30;
    let settings = ByCpuLoadSettings {
        enabled: true,
        rules: vec![rule],
    };
    for unavailable in [None, Some(f32::NAN), Some(101.0)] {
        let mut scheduler = ByCpuLoadScheduler::default();
        let start = Instant::now();
        assert_eq!(
            scheduler
                .current_decision_at(&settings, Some(80.0), start)
                .unwrap()
                .power_plan_guid,
            "else-plan"
        );
        assert_eq!(
            scheduler
                .current_decision_at(
                    &settings,
                    Some(80.0),
                    start + std::time::Duration::from_secs(30),
                )
                .unwrap()
                .power_plan_guid,
            "matched-plan"
        );
        assert!(scheduler
            .current_decision_at(
                &settings,
                unavailable,
                start + std::time::Duration::from_secs(31),
            )
            .is_none());
        assert_eq!(
            scheduler
                .current_decision_at(
                    &settings,
                    Some(80.0),
                    start + std::time::Duration::from_secs(60),
                )
                .unwrap()
                .power_plan_guid,
            "else-plan"
        );
    }
}

#[test]
fn selected_rule_index_survives_else_branch_and_arbitration() {
    let mut settings = crate::config::Settings::default();
    settings.general.enabled = true;
    settings.general.pause_power_plan_switching_while_plugged_in = false;
    settings.by_cpu_load.enabled = true;
    settings.by_cpu_load.rules = vec![
        settings.by_cpu_load.rules[0].clone(),
        ByCpuLoadRule {
            enabled: true,
            name: "Second".into(),
            comparison: CpuUsageComparison::AtOrAbove,
            threshold_percent: CpuLoadPercent::new(75).unwrap(),
            duration_seconds: 0,
            power_plan_guid: Some("main".into()),
            else_enabled: true,
            else_power_plan_guid: Some("else".into()),
            stability: None,
            upper_threshold_percent: None,
        },
    ];
    settings.by_cpu_load.rules[0].enabled = false;
    let mut scheduler = ByCpuLoadScheduler::default();
    for (usage, plan) in [(10.0, "else"), (90.0, "main")] {
        let decision = scheduler
            .current_decision(&settings.by_cpu_load, Some(usage))
            .unwrap();
        assert_eq!(decision.rule_index, 1);
        let outcome = crate::rules::decide(
            &settings,
            crate::rules::DecisionInput {
                activity_state: crate::activity::ActivityState::Unknown,
                foreground_executable_path: None,
                plugged_in: Some(false),
                by_running_app: None,
                by_time: None,
                by_cpu_load: Some(decision),
            },
        );
        assert_eq!(outcome.rule_index, Some(1));
        assert_eq!(outcome.power_plan_guid.as_deref(), Some(plan));
    }
}

#[test]
fn by_cpu_load_returns_matching_zero_duration_rule() {
    let mut scheduler = ByCpuLoadScheduler::default();
    let settings = ByCpuLoadSettings {
        enabled: true,
        rules: vec![ByCpuLoadRule {
            enabled: true,
            name: "High CPU".to_owned(),
            comparison: CpuUsageComparison::AtOrAbove,
            threshold_percent: CpuLoadPercent::new(75).unwrap(),
            upper_threshold_percent: None,
            duration_seconds: 0,
            power_plan_guid: Some("high-cpu-guid".to_owned()),
            else_enabled: false,
            else_power_plan_guid: None,
            stability: None,
        }],
    };

    let decision = scheduler.current_decision(&settings, Some(80.0)).unwrap();

    assert_eq!(decision.rule_name, "High CPU");
    assert_eq!(decision.power_plan_guid, "high-cpu-guid");
}

#[test]
fn ignores_non_matching_rule() {
    let mut scheduler = ByCpuLoadScheduler::default();
    let settings = ByCpuLoadSettings {
        enabled: true,
        rules: vec![ByCpuLoadRule {
            enabled: true,
            name: "Low CPU".to_owned(),
            comparison: CpuUsageComparison::AtOrBelow,
            threshold_percent: CpuLoadPercent::new(20).unwrap(),
            upper_threshold_percent: None,
            duration_seconds: 0,
            power_plan_guid: Some("low-cpu-guid".to_owned()),
            else_enabled: false,
            else_power_plan_guid: None,
            stability: None,
        }],
    };

    assert!(scheduler.current_decision(&settings, Some(45.0)).is_none());
}

#[test]
fn matching_rule_without_selected_plan_does_not_decide() {
    let mut scheduler = ByCpuLoadScheduler::default();
    let settings = ByCpuLoadSettings {
        enabled: true,
        rules: vec![ByCpuLoadRule {
            enabled: true,
            name: "High CPU".to_owned(),
            comparison: CpuUsageComparison::AtOrAbove,
            threshold_percent: CpuLoadPercent::new(75).unwrap(),
            upper_threshold_percent: None,
            duration_seconds: 0,
            power_plan_guid: None,
            else_enabled: false,
            else_power_plan_guid: None,
            stability: None,
        }],
    };

    assert!(scheduler.current_decision(&settings, Some(80.0)).is_none());
}

#[test]
fn returns_between_rule_when_usage_is_in_range() {
    let mut scheduler = ByCpuLoadScheduler::default();
    let settings = ByCpuLoadSettings {
        enabled: true,
        rules: vec![ByCpuLoadRule {
            enabled: true,
            name: "Medium CPU".to_owned(),
            comparison: CpuUsageComparison::Between,
            threshold_percent: CpuLoadPercent::new(30).unwrap(),
            upper_threshold_percent: Some(CpuLoadPercent::new(60).unwrap()),
            duration_seconds: 0,
            power_plan_guid: Some("medium-cpu-guid".to_owned()),
            else_enabled: false,
            else_power_plan_guid: None,
            stability: None,
        }],
    };

    let decision = scheduler.current_decision(&settings, Some(45.0)).unwrap();

    assert_eq!(decision.rule_name, "Medium CPU");
    assert_eq!(decision.power_plan_guid, "medium-cpu-guid");
}

#[test]
fn else_branch_applies_until_condition_duration_is_met() {
    let mut scheduler = ByCpuLoadScheduler::default();
    let settings = ByCpuLoadSettings {
        enabled: true,
        rules: vec![ByCpuLoadRule {
            enabled: true,
            name: "High CPU".to_owned(),
            comparison: CpuUsageComparison::AtOrAbove,
            threshold_percent: CpuLoadPercent::new(75).unwrap(),
            upper_threshold_percent: None,
            duration_seconds: 30,
            power_plan_guid: Some("high-cpu-guid".to_owned()),
            else_enabled: true,
            else_power_plan_guid: Some("else-guid".to_owned()),
            stability: None,
        }],
    };

    let waiting_decision = scheduler.current_decision(&settings, Some(80.0)).unwrap();
    assert_eq!(waiting_decision.power_plan_guid, "else-guid");

    let decision = scheduler.current_decision(&settings, Some(40.0)).unwrap();

    assert_eq!(decision.rule_name, "High CPU else");
    assert_eq!(decision.power_plan_guid, "else-guid");
}

#[test]
fn editing_a_rule_resets_its_duration_timer() {
    let mut scheduler = ByCpuLoadScheduler::default();
    let mut settings = ByCpuLoadSettings {
        enabled: true,
        rules: vec![ByCpuLoadRule {
            enabled: true,
            name: "High CPU".to_owned(),
            comparison: CpuUsageComparison::AtOrAbove,
            threshold_percent: CpuLoadPercent::new(75).unwrap(),
            upper_threshold_percent: None,
            duration_seconds: 30,
            power_plan_guid: Some("high-cpu-guid".to_owned()),
            else_enabled: false,
            else_power_plan_guid: None,
            stability: None,
        }],
    };
    scheduler.last_rules.clone_from(&settings.rules);
    scheduler.matched_since = vec![Some(Instant::now() - std::time::Duration::from_secs(30))];

    settings.rules[0].name = "Edited High CPU".to_owned();

    assert!(scheduler.current_decision(&settings, Some(80.0)).is_none());
}

#[test]
fn selected_rule_does_not_leave_later_rule_timers_stale() {
    let mut scheduler = ByCpuLoadScheduler::default();
    let settings = ByCpuLoadSettings {
        enabled: true,
        rules: vec![
            ByCpuLoadRule {
                enabled: true,
                name: "High CPU".to_owned(),
                comparison: CpuUsageComparison::AtOrAbove,
                threshold_percent: CpuLoadPercent::new(75).unwrap(),
                upper_threshold_percent: None,
                duration_seconds: 0,
                power_plan_guid: Some("high-cpu-guid".to_owned()),
                else_enabled: false,
                else_power_plan_guid: None,
                stability: None,
            },
            ByCpuLoadRule {
                enabled: true,
                name: "Low CPU".to_owned(),
                comparison: CpuUsageComparison::AtOrBelow,
                threshold_percent: CpuLoadPercent::new(20).unwrap(),
                upper_threshold_percent: None,
                duration_seconds: 30,
                power_plan_guid: Some("low-cpu-guid".to_owned()),
                else_enabled: false,
                else_power_plan_guid: None,
                stability: None,
            },
        ],
    };
    scheduler.last_rules.clone_from(&settings.rules);
    scheduler.matched_since = vec![
        Some(Instant::now()),
        Some(Instant::now() - std::time::Duration::from_secs(30)),
    ];

    let decision = scheduler.current_decision(&settings, Some(90.0)).unwrap();

    assert_eq!(decision.rule_name, "High CPU");
    assert!(scheduler.matched_since[1].is_none());
}
#[test]
fn disabled_rule_is_ignored() {
    let mut scheduler = ByCpuLoadScheduler::default();
    let settings = ByCpuLoadSettings {
        enabled: true,
        rules: vec![ByCpuLoadRule {
            enabled: false,
            name: "High CPU".to_owned(),
            comparison: CpuUsageComparison::AtOrAbove,
            threshold_percent: CpuLoadPercent::new(75).unwrap(),
            upper_threshold_percent: None,
            duration_seconds: 0,
            power_plan_guid: Some("high-cpu-guid".to_owned()),
            else_enabled: true,
            else_power_plan_guid: Some("else-guid".to_owned()),
            stability: None,
        }],
    };

    assert!(scheduler.current_decision(&settings, Some(90.0)).is_none());
    assert!(scheduler.current_decision(&settings, Some(10.0)).is_none());
}

fn stable_settings() -> ByCpuLoadSettings {
    let mut rule = rule_with_else(CpuUsageComparison::AtOrAbove);
    rule.stability = Some(ByCpuLoadStability {
        exit_threshold_percent: CpuLoadPercent::new(65).unwrap(),
        exit_upper_threshold_percent: None,
        minimum_hold_seconds: 0,
    });
    ByCpuLoadSettings {
        enabled: true,
        rules: vec![rule],
    }
}

fn apply_at(
    scheduler: &mut ByCpuLoadScheduler,
    settings: &ByCpuLoadSettings,
    usage: f32,
    now: Instant,
) -> ByCpuLoadDecision {
    let decision = scheduler
        .current_decision_at(settings, Some(usage), now)
        .unwrap();
    scheduler.confirm_applied_rule(
        (!decision.is_else).then_some(decision.rule_index),
        Some(&decision.power_plan_guid),
        now,
    );
    decision
}

#[test]
fn exit_bands_retain_only_qualified_active_rules_and_include_their_bounds() {
    for (comparison, entry, upper, exit, exit_upper, inside, outside) in [
        (
            CpuUsageComparison::AtOrAbove,
            75,
            None,
            65,
            None,
            65.0,
            64.9,
        ),
        (
            CpuUsageComparison::AtOrBelow,
            25,
            None,
            35,
            None,
            35.0,
            35.1,
        ),
        (
            CpuUsageComparison::Between,
            40,
            Some(60),
            30,
            Some(70),
            30.0,
            29.9,
        ),
        (
            CpuUsageComparison::Between,
            40,
            Some(60),
            30,
            Some(70),
            70.0,
            70.1,
        ),
    ] {
        let mut settings = stable_settings();
        let rule = &mut settings.rules[0];
        rule.comparison = comparison;
        rule.threshold_percent = CpuLoadPercent::new(entry).unwrap();
        rule.upper_threshold_percent = upper.map(|v| CpuLoadPercent::new(v).unwrap());
        rule.duration_seconds = 5;
        let stability = rule.stability.as_mut().unwrap();
        stability.exit_threshold_percent = CpuLoadPercent::new(exit).unwrap();
        stability.exit_upper_threshold_percent =
            exit_upper.map(|v| CpuLoadPercent::new(v).unwrap());
        settings.rules[0].validate().unwrap();
        let mut scheduler = ByCpuLoadScheduler::default();
        let start = Instant::now();
        assert!(apply_at(&mut scheduler, &settings, inside, start).is_else);
        let entry_usage = f32::from(entry);
        assert!(apply_at(&mut scheduler, &settings, entry_usage, start).is_else);
        assert!(
            !apply_at(
                &mut scheduler,
                &settings,
                entry_usage,
                start + Duration::from_secs(5)
            )
            .is_else
        );
        for seconds in 6..10 {
            let usage = if seconds % 2 == 0 {
                inside
            } else {
                entry_usage
            };
            assert!(
                !apply_at(
                    &mut scheduler,
                    &settings,
                    usage,
                    start + Duration::from_secs(seconds)
                )
                .is_else
            );
        }
        assert!(
            apply_at(
                &mut scheduler,
                &settings,
                outside,
                start + Duration::from_secs(10)
            )
            .is_else
        );
    }
}

#[test]
fn minimum_hold_starts_at_confirmation_and_is_not_extended_by_rechecks() {
    let mut settings = stable_settings();
    settings.rules[0]
        .stability
        .as_mut()
        .unwrap()
        .minimum_hold_seconds = 30;
    let mut scheduler = ByCpuLoadScheduler::default();
    let start = Instant::now();
    assert!(
        !scheduler
            .current_decision_at(&settings, Some(80.0), start)
            .unwrap()
            .is_else
    );
    assert!(
        apply_at(
            &mut scheduler,
            &settings,
            20.0,
            start + Duration::from_secs(10)
        )
        .is_else
    );
    let applied_at = start + Duration::from_secs(25);
    assert!(!apply_at(&mut scheduler, &settings, 80.0, applied_at).is_else);
    assert!(
        !apply_at(
            &mut scheduler,
            &settings,
            20.0,
            applied_at + Duration::from_secs(29)
        )
        .is_else
    );
    assert!(
        apply_at(
            &mut scheduler,
            &settings,
            20.0,
            applied_at + Duration::from_secs(30)
        )
        .is_else
    );
}

#[test]
fn earlier_qualified_cpu_rule_preempts_a_held_rule_even_with_the_same_plan() {
    let mut settings = stable_settings();
    settings.rules[0]
        .stability
        .as_mut()
        .unwrap()
        .minimum_hold_seconds = 30;
    let mut earlier = settings.rules[0].clone();
    earlier.threshold_percent = CpuLoadPercent::new(90).unwrap();
    earlier.duration_seconds = 5;
    earlier.stability.as_mut().unwrap().exit_threshold_percent = CpuLoadPercent::new(85).unwrap();
    settings.rules.insert(0, earlier);
    let mut scheduler = ByCpuLoadScheduler::default();
    let start = Instant::now();
    assert_eq!(
        apply_at(&mut scheduler, &settings, 80.0, start).rule_index,
        1
    );
    assert_eq!(
        apply_at(
            &mut scheduler,
            &settings,
            95.0,
            start + Duration::from_secs(1)
        )
        .rule_index,
        1
    );
    assert_eq!(
        apply_at(
            &mut scheduler,
            &settings,
            95.0,
            start + Duration::from_secs(6)
        )
        .rule_index,
        0
    );
    // The earlier rule's own hold starts at the handoff, despite an unchanged GUID.
    assert_eq!(
        apply_at(
            &mut scheduler,
            &settings,
            20.0,
            start + Duration::from_secs(31)
        )
        .rule_index,
        0
    );
    assert!(
        apply_at(
            &mut scheduler,
            &settings,
            20.0,
            start + Duration::from_secs(36)
        )
        .is_else
    );
}

#[test]
fn same_plan_else_branch_never_starts_or_inherits_a_primary_hold() {
    let mut settings = stable_settings();
    settings.rules[0].else_power_plan_guid = settings.rules[0].power_plan_guid.clone();
    settings.rules[0]
        .stability
        .as_mut()
        .unwrap()
        .minimum_hold_seconds = 10;
    let mut scheduler = ByCpuLoadScheduler::default();
    let start = Instant::now();
    assert!(apply_at(&mut scheduler, &settings, 20.0, start).is_else);
    assert!(
        !apply_at(
            &mut scheduler,
            &settings,
            80.0,
            start + Duration::from_secs(1)
        )
        .is_else
    );
    assert!(
        apply_at(
            &mut scheduler,
            &settings,
            20.0,
            start + Duration::from_secs(11)
        )
        .is_else
    );
    assert!(
        apply_at(
            &mut scheduler,
            &settings,
            70.0,
            start + Duration::from_secs(12)
        )
        .is_else
    );
}

#[test]
fn interruption_or_settings_invalidation_cannot_reuse_stability() {
    let start = Instant::now();
    #[derive(Debug)]
    enum Interruption {
        Missing,
        Invalid,
        Reset,
        Preemption,
        ExternalPlan,
        Edit,
        Disable,
    }
    for interruption in [
        Interruption::Missing,
        Interruption::Invalid,
        Interruption::Reset,
        Interruption::Preemption,
        Interruption::ExternalPlan,
        Interruption::Edit,
        Interruption::Disable,
    ] {
        let mut settings = stable_settings();
        settings.rules[0]
            .stability
            .as_mut()
            .unwrap()
            .minimum_hold_seconds = 120;
        let mut scheduler = ByCpuLoadScheduler::default();
        apply_at(&mut scheduler, &settings, 80.0, start);
        match interruption {
            Interruption::Missing => {
                scheduler.current_decision_at(&settings, None, start + Duration::from_secs(1));
            }
            Interruption::Invalid => {
                scheduler.current_decision_at(
                    &settings,
                    Some(f32::NAN),
                    start + Duration::from_secs(1),
                );
            }
            Interruption::Reset => scheduler.reset(),
            Interruption::Preemption => scheduler.confirm_applied_rule(
                None,
                Some("matched-plan"),
                start + Duration::from_secs(1),
            ),
            Interruption::ExternalPlan => scheduler.observe_active_plan(Some("external-plan")),
            Interruption::Edit => {
                settings.rules[0].name = "Edited".into();
            }
            Interruption::Disable => {
                settings.enabled = false;
                assert!(scheduler
                    .current_decision_at(&settings, Some(80.0), start + Duration::from_secs(1))
                    .is_none());
                settings.enabled = true;
            }
        }
        assert!(
            apply_at(
                &mut scheduler,
                &settings,
                70.0,
                start + Duration::from_secs(2)
            )
            .is_else,
            "{interruption:?}"
        );
    }
}

#[test]
fn absent_stability_keeps_existing_entry_duration_and_release_behavior() {
    let mut settings = stable_settings();
    settings.rules[0].stability = None;
    settings.rules[0].duration_seconds = 5;
    let mut scheduler = ByCpuLoadScheduler::default();
    let start = Instant::now();
    assert!(apply_at(&mut scheduler, &settings, 80.0, start).is_else);
    // A higher-priority feature's selection does not erase legacy entry qualification.
    scheduler.confirm_applied_rule(
        None,
        Some("foreground-plan"),
        start + Duration::from_secs(1),
    );
    assert!(
        !apply_at(
            &mut scheduler,
            &settings,
            80.0,
            start + Duration::from_secs(5)
        )
        .is_else
    );
    assert!(
        apply_at(
            &mut scheduler,
            &settings,
            74.9,
            start + Duration::from_secs(6)
        )
        .is_else
    );
}
