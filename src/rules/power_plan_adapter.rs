#![allow(dead_code)]

use crate::rules::{
    model::{
        Action, AppMatcher, Rule, RuleId, Trigger, PRIORITY_ACTIVITY, PRIORITY_CPU_LOAD,
        PRIORITY_FALLBACK, PRIORITY_FOCUSED_APP, PRIORITY_MANUAL_OVERRIDE, PRIORITY_RUNNING_APP,
        PRIORITY_SCHEDULE,
    },
    DecisionInput, DecisionOutcome, DecisionState, PriorityResolver, ResolvedAction,
};
use crate::{
    activity::ActivityState,
    config::{PowerPlanSettings, Settings},
};
use chrono::Utc;

pub fn resolved_power_plan_action_for_context(
    settings: &Settings,
    input: &DecisionInput,
) -> Option<Action> {
    resolved_power_plan_resolved_action_for_context(settings, input).map(|resolved| resolved.action)
}

pub fn resolved_power_plan_resolved_action_for_context(
    settings: &Settings,
    input: &DecisionInput,
) -> Option<ResolvedAction> {
    let rules = active_power_plan_rules_for_context(settings, input);
    let resolved = PriorityResolver.resolve(&rules);

    resolved.into_iter().find_map(|resolved| {
        matches!(resolved.action, Action::SwitchPowerPlan { .. }).then_some(resolved)
    })
}

pub fn resolved_power_plan_guid_for_context(
    settings: &Settings,
    input: &DecisionInput,
) -> Option<String> {
    resolved_power_plan_action_for_context(settings, input).and_then(|action| match action {
        Action::SwitchPowerPlan { plan_guid } => Some(plan_guid),
        _ => None,
    })
}

pub fn active_power_plan_rules_for_context(
    settings: &Settings,
    input: &DecisionInput,
) -> Vec<Rule> {
    if !settings.general.enabled {
        return Vec::new();
    }

    let now = Utc::now().timestamp();
    if settings.general.manual_override.is_active(now)
        || (settings.general.pause_power_plan_switching_while_plugged_in
            && input.plugged_in == Some(true))
    {
        return Vec::new();
    }

    let mut rules = Vec::new();
    let foreground = input.foreground_app.as_deref().map(str::to_ascii_lowercase);

    if let Some(app) = foreground
        .as_deref()
        .filter(|_| settings.foreground_rules.enabled)
    {
        for rule in &settings.foreground_rules.rules {
            if rule.enabled && rule.process_name.trim().eq_ignore_ascii_case(app.trim()) {
                if let Some(power_plan_guid) = rule.power_plan_guid.clone() {
                    rules.push(power_plan_rule(
                        "power-plan.foreground-rule",
                        rule.name.clone(),
                        PRIORITY_FOCUSED_APP,
                        Trigger::AppFocused {
                            app: AppMatcher::ProcessName(app.to_owned()),
                        },
                        power_plan_guid,
                    ));
                }
                break;
            }
        }

        if contains_process(&settings.foreground_rules.force_power_save, app) {
            if let Some(plan_guid) = idle_plan(&settings.foreground_rules.power_plans, settings) {
                rules.push(power_plan_rule(
                    "power-plan.foreground-force-power-save",
                    "Power plan foreground power saver",
                    PRIORITY_FOCUSED_APP,
                    Trigger::AppFocused {
                        app: AppMatcher::ProcessName(app.to_owned()),
                    },
                    plan_guid,
                ));
            }
        }

        if contains_process(&settings.foreground_rules.whitelist, app) {
            if let Some(plan_guid) = active_plan(&settings.foreground_rules.power_plans, settings) {
                rules.push(power_plan_rule(
                    "power-plan.foreground-force-active",
                    "Power plan foreground active",
                    PRIORITY_FOCUSED_APP,
                    Trigger::AppFocused {
                        app: AppMatcher::ProcessName(app.to_owned()),
                    },
                    plan_guid,
                ));
            }
        }
    }

    if let Some(performance_mode) = &input.performance_mode {
        rules.push(power_plan_rule(
            "power-plan.performance-mode",
            format!(
                "Power plan running app performance mode: {}",
                performance_mode.rule_name
            ),
            PRIORITY_RUNNING_APP,
            Trigger::AppRunning {
                app: AppMatcher::ProcessName(performance_mode.process_name.clone()),
            },
            performance_mode.power_plan_guid.clone(),
        ));
    }

    if let Some(cpu_usage) = &input.cpu_usage {
        if let Some(plan_guid) = cpu_usage.power_plan_guid.clone() {
            rules.push(power_plan_rule(
                "power-plan.cpu-load",
                format!("Power plan CPU load: {}", cpu_usage.rule_name),
                PRIORITY_CPU_LOAD,
                Trigger::CpuLoadAbove {
                    percent: cpu_usage.usage_percent.clamp(0.0, 100.0) as u8,
                    duration_secs: 0,
                },
                plan_guid,
            ));
        }
    }

    if settings.activity_mode.enabled {
        match input.activity_state {
            ActivityState::Idle => {
                if let Some(plan_guid) = idle_plan(&settings.activity_mode.power_plans, settings) {
                    rules.push(power_plan_rule(
                        "power-plan.idle",
                        "Power plan idle",
                        PRIORITY_ACTIVITY,
                        Trigger::UserIdle {
                            duration_secs: settings.activity_mode.idle_timeout_seconds,
                        },
                        plan_guid,
                    ));
                }
            }
            ActivityState::Active
                if settings.activity_mode.switch_to_performance_on_resume
                    && settings.activity_mode.input_detection.any_enabled() =>
            {
                if let Some(plan_guid) = active_plan(&settings.activity_mode.power_plans, settings)
                {
                    rules.push(power_plan_rule(
                        "power-plan.active",
                        "Power plan active",
                        PRIORITY_ACTIVITY,
                        Trigger::UserActive,
                        plan_guid,
                    ));
                }
            }
            ActivityState::Active | ActivityState::Unknown => {}
        }

        return rules;
    }

    if let Some(schedule) = &input.schedule {
        if let Some(plan_guid) = schedule.power_plan_guid.clone() {
            rules.push(power_plan_rule(
                "power-plan.schedule",
                format!("Power plan schedule: {}", schedule.rule_name),
                PRIORITY_SCHEDULE,
                Trigger::Schedule {
                    schedule_id: schedule.rule_name.clone(),
                },
                plan_guid,
            ));
        }
    }

    if rules.is_empty() {
        if let Some(plan_guid) = active_plan(&settings.activity_mode.power_plans, settings) {
            rules.push(power_plan_rule(
                "power-plan.active",
                "Power plan active",
                PRIORITY_ACTIVITY,
                Trigger::UserActive,
                plan_guid,
            ));
        }
    }

    rules
}

fn power_plan_rule(
    id: impl Into<String>,
    name: impl Into<String>,
    priority: i32,
    trigger: Trigger,
    plan_guid: String,
) -> Rule {
    Rule {
        id: RuleId(id.into()),
        name: name.into(),
        enabled: true,
        priority,
        trigger,
        actions: vec![Action::SwitchPowerPlan { plan_guid }],
        restore_actions: Vec::new(),
        cooldown_secs: 0,
    }
}

fn contains_process(list: &[String], app: &str) -> bool {
    list.iter()
        .any(|entry| entry.trim().eq_ignore_ascii_case(app.trim()))
}

fn idle_plan(power_plans: &PowerPlanSettings, settings: &Settings) -> Option<String> {
    power_plans
        .power_save_guid
        .clone()
        .or_else(|| settings.power_plans.power_save_guid.clone())
}

fn active_plan(power_plans: &PowerPlanSettings, settings: &Settings) -> Option<String> {
    power_plans
        .performance_guid
        .clone()
        .or_else(|| settings.power_plans.performance_guid.clone())
}

pub fn resolved_power_plan_guid_for_decision(outcome: &DecisionOutcome) -> Option<String> {
    resolved_power_plan_action_for_decision(outcome).and_then(|action| match action {
        Action::SwitchPowerPlan { plan_guid } => Some(plan_guid),
        _ => None,
    })
}

pub fn resolved_power_plan_action_for_decision(outcome: &DecisionOutcome) -> Option<Action> {
    let rule = decision_outcome_to_rule(outcome)?;
    let resolved = PriorityResolver.resolve(&[rule]);

    resolved
        .into_iter()
        .find_map(|resolved| match &resolved.action {
            Action::SwitchPowerPlan { .. } => Some(resolved.action),
            _ => None,
        })
}

pub fn decision_outcome_to_rule(outcome: &DecisionOutcome) -> Option<Rule> {
    let plan_guid = outcome.target_guid.clone()?;
    let trigger = trigger_for_state(outcome.state);
    let mut rule = Rule {
        id: RuleId(rule_id_for_state(outcome.state).to_owned()),
        name: rule_name_for_state(outcome.state).to_owned(),
        enabled: true,
        priority: priority_for_state(outcome.state),
        trigger,
        actions: vec![Action::SwitchPowerPlan { plan_guid }],
        restore_actions: Vec::new(),
        cooldown_secs: 0,
    };

    if matches!(outcome.state, DecisionState::NoTargetPlan) {
        rule.enabled = false;
    }

    Some(rule)
}

fn trigger_for_state(state: DecisionState) -> Trigger {
    match state {
        DecisionState::ManualOverride => Trigger::ManualOverride,
        DecisionState::ForegroundRule
        | DecisionState::ForegroundForceActive
        | DecisionState::ForegroundForcePowerSave => Trigger::AppFocused {
            app: AppMatcher::Pattern("*".to_owned()),
        },
        DecisionState::PerformanceMode => Trigger::AppRunning {
            app: AppMatcher::Pattern("*".to_owned()),
        },
        DecisionState::ScheduledRule => Trigger::Schedule {
            schedule_id: "current".to_owned(),
        },
        DecisionState::CpuLoadRule => Trigger::CpuLoadAbove {
            percent: 0,
            duration_secs: 0,
        },
        DecisionState::IdlePowerSave => Trigger::UserIdle { duration_secs: 0 },
        DecisionState::ActivePerformance => Trigger::UserActive,
        DecisionState::Disabled | DecisionState::PluggedInPause | DecisionState::NoTargetPlan => {
            Trigger::SafetyProtection
        }
    }
}

fn priority_for_state(state: DecisionState) -> i32 {
    match state {
        DecisionState::ManualOverride => PRIORITY_MANUAL_OVERRIDE,
        DecisionState::ForegroundRule
        | DecisionState::ForegroundForceActive
        | DecisionState::ForegroundForcePowerSave => PRIORITY_FOCUSED_APP,
        DecisionState::PerformanceMode => PRIORITY_RUNNING_APP,
        DecisionState::CpuLoadRule => PRIORITY_CPU_LOAD,
        DecisionState::IdlePowerSave | DecisionState::ActivePerformance => PRIORITY_ACTIVITY,
        DecisionState::ScheduledRule => PRIORITY_SCHEDULE,
        DecisionState::Disabled | DecisionState::PluggedInPause | DecisionState::NoTargetPlan => {
            PRIORITY_FALLBACK
        }
    }
}

fn rule_id_for_state(state: DecisionState) -> &'static str {
    match state {
        DecisionState::Disabled => "power-plan.disabled",
        DecisionState::ManualOverride => "power-plan.manual-override",
        DecisionState::PluggedInPause => "power-plan.plugged-in-pause",
        DecisionState::ForegroundRule => "power-plan.foreground-rule",
        DecisionState::ForegroundForceActive => "power-plan.foreground-force-active",
        DecisionState::ForegroundForcePowerSave => "power-plan.foreground-force-power-save",
        DecisionState::PerformanceMode => "power-plan.performance-mode",
        DecisionState::ScheduledRule => "power-plan.schedule",
        DecisionState::CpuLoadRule => "power-plan.cpu-load",
        DecisionState::IdlePowerSave => "power-plan.idle",
        DecisionState::ActivePerformance => "power-plan.active",
        DecisionState::NoTargetPlan => "power-plan.no-target",
    }
}

fn rule_name_for_state(state: DecisionState) -> &'static str {
    match state {
        DecisionState::Disabled => "Power plan disabled",
        DecisionState::ManualOverride => "Power plan manual override",
        DecisionState::PluggedInPause => "Power plan plugged-in pause",
        DecisionState::ForegroundRule => "Power plan foreground rule",
        DecisionState::ForegroundForceActive => "Power plan foreground active",
        DecisionState::ForegroundForcePowerSave => "Power plan foreground power saver",
        DecisionState::PerformanceMode => "Power plan running app performance mode",
        DecisionState::ScheduledRule => "Power plan schedule",
        DecisionState::CpuLoadRule => "Power plan CPU load",
        DecisionState::IdlePowerSave => "Power plan idle",
        DecisionState::ActivePerformance => "Power plan active",
        DecisionState::NoTargetPlan => "Power plan no target",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        config::{ForegroundRule, ForegroundRules},
        rules::{ConflictGroup, DecisionEngine, PriorityResolver},
        scheduler::{CpuUsageDecision, ScheduleDecision},
    };

    fn test_settings() -> Settings {
        let mut settings = Settings::default();
        settings.power_plans.power_save_guid = Some("idle-guid".to_owned());
        settings.power_plans.performance_guid = Some("active-guid".to_owned());
        settings.foreground_rules = ForegroundRules {
            enabled: true,
            rules: vec![ForegroundRule {
                enabled: true,
                name: "Game".to_owned(),
                process_name: "game.exe".to_owned(),
                power_plan_guid: Some("foreground-guid".to_owned()),
            }],
            whitelist: Vec::new(),
            force_power_save: Vec::new(),
            power_plans: PowerPlanSettings::default(),
        };
        settings
    }

    fn decision_input() -> DecisionInput {
        DecisionInput {
            activity_state: ActivityState::Active,
            foreground_app: None,
            plugged_in: None,
            performance_mode: None,
            schedule: None,
            cpu_usage: None,
        }
    }

    fn assert_context_matches_decision_engine(settings: &Settings, input: DecisionInput) {
        let decision = DecisionEngine.decide(settings, input.clone());

        assert_eq!(
            resolved_power_plan_guid_for_context(settings, &input),
            decision.target_guid,
            "context adapter should match DecisionEngine for {:?}",
            decision.state
        );
    }

    #[test]
    fn outcome_without_target_produces_no_rule() {
        let outcome = DecisionOutcome {
            target_guid: None,
            state: DecisionState::PluggedInPause,
            reason: "Paused while plugged in.".to_owned(),
        };

        assert_eq!(decision_outcome_to_rule(&outcome), None);
    }

    #[test]
    fn foreground_decision_maps_to_switch_power_plan_action() {
        let outcome = DecisionOutcome {
            target_guid: Some("foreground-guid".to_owned()),
            state: DecisionState::ForegroundRule,
            reason: "Matched foreground app.".to_owned(),
        };

        let rule = decision_outcome_to_rule(&outcome).expect("rule");

        assert_eq!(rule.id.0, "power-plan.foreground-rule");
        assert_eq!(rule.priority, PRIORITY_FOCUSED_APP);
        assert!(matches!(rule.trigger, Trigger::AppFocused { .. }));
        assert_eq!(rule.actions.len(), 1);
        assert_eq!(rule.actions[0].conflict_group(), ConflictGroup::PowerPlan);
        assert!(matches!(
            &rule.actions[0],
            Action::SwitchPowerPlan { plan_guid } if plan_guid == "foreground-guid"
        ));
    }

    #[test]
    fn resolved_power_plan_action_returns_generic_switch_action() {
        let outcome = DecisionOutcome {
            target_guid: Some("foreground-guid".to_owned()),
            state: DecisionState::ForegroundRule,
            reason: "Matched foreground app.".to_owned(),
        };

        assert!(matches!(
            resolved_power_plan_action_for_decision(&outcome),
            Some(Action::SwitchPowerPlan { plan_guid }) if plan_guid == "foreground-guid"
        ));
    }

    #[test]
    fn cpu_load_decision_maps_to_cpu_priority() {
        let outcome = DecisionOutcome {
            target_guid: Some("cpu-guid".to_owned()),
            state: DecisionState::CpuLoadRule,
            reason: "CPU matched.".to_owned(),
        };

        let rule = decision_outcome_to_rule(&outcome).expect("rule");

        assert_eq!(rule.priority, PRIORITY_CPU_LOAD);
        assert!(matches!(rule.trigger, Trigger::CpuLoadAbove { .. }));
    }

    #[test]
    fn schedule_decision_maps_to_schedule_priority() {
        let outcome = DecisionOutcome {
            target_guid: Some("schedule-guid".to_owned()),
            state: DecisionState::ScheduledRule,
            reason: "Schedule matched.".to_owned(),
        };

        let rule = decision_outcome_to_rule(&outcome).expect("rule");

        assert_eq!(rule.priority, PRIORITY_SCHEDULE);
        assert!(matches!(rule.trigger, Trigger::Schedule { .. }));
    }

    #[test]
    fn adapted_power_plan_rules_resolve_like_current_precedence() {
        let schedule = decision_outcome_to_rule(&DecisionOutcome {
            target_guid: Some("schedule-guid".to_owned()),
            state: DecisionState::ScheduledRule,
            reason: "Schedule matched.".to_owned(),
        })
        .expect("schedule rule");
        let foreground = decision_outcome_to_rule(&DecisionOutcome {
            target_guid: Some("foreground-guid".to_owned()),
            state: DecisionState::ForegroundRule,
            reason: "Foreground matched.".to_owned(),
        })
        .expect("foreground rule");

        let resolved = PriorityResolver.resolve(&[schedule, foreground]);

        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].rule_id.0, "power-plan.foreground-rule");
        assert!(matches!(
            &resolved[0].action,
            Action::SwitchPowerPlan { plan_guid } if plan_guid == "foreground-guid"
        ));
    }

    #[test]
    fn resolved_power_plan_guid_matches_decision_target_for_all_states() {
        let cases = [
            (DecisionState::Disabled, None),
            (DecisionState::ManualOverride, None),
            (DecisionState::PluggedInPause, None),
            (
                DecisionState::ForegroundRule,
                Some("ForegroundRule-guid".to_owned()),
            ),
            (
                DecisionState::ForegroundForceActive,
                Some("ForegroundForceActive-guid".to_owned()),
            ),
            (
                DecisionState::ForegroundForcePowerSave,
                Some("ForegroundForcePowerSave-guid".to_owned()),
            ),
            (
                DecisionState::PerformanceMode,
                Some("PerformanceMode-guid".to_owned()),
            ),
            (
                DecisionState::ScheduledRule,
                Some("ScheduledRule-guid".to_owned()),
            ),
            (
                DecisionState::CpuLoadRule,
                Some("CpuLoadRule-guid".to_owned()),
            ),
            (
                DecisionState::IdlePowerSave,
                Some("IdlePowerSave-guid".to_owned()),
            ),
            (
                DecisionState::ActivePerformance,
                Some("ActivePerformance-guid".to_owned()),
            ),
            (DecisionState::NoTargetPlan, None),
        ];

        for (state, target_guid) in cases {
            let outcome = DecisionOutcome {
                target_guid,
                state,
                reason: format!("{state:?} test"),
            };

            assert_eq!(
                resolved_power_plan_guid_for_decision(&outcome),
                outcome.target_guid,
                "{state:?} should preserve the target GUID through the generic resolver"
            );
        }
    }

    #[test]
    fn resolved_power_plan_guid_is_none_without_decision_target() {
        let outcome = DecisionOutcome {
            target_guid: None,
            state: DecisionState::ForegroundRule,
            reason: "No target selected.".to_owned(),
        };

        assert_eq!(resolved_power_plan_guid_for_decision(&outcome), None);
    }

    #[test]
    fn context_adapter_matches_foreground_over_schedule() {
        let settings = test_settings();
        let mut input = decision_input();
        input.foreground_app = Some("game.exe".to_owned());
        input.schedule = Some(ScheduleDecision {
            rule_name: "Work".to_owned(),
            power_plan_guid: Some("schedule-guid".to_owned()),
        });

        assert_context_matches_decision_engine(&settings, input);
    }

    #[test]
    fn context_adapter_matches_cpu_over_activity() {
        let settings = test_settings();
        let mut input = decision_input();
        input.activity_state = ActivityState::Idle;
        input.cpu_usage = Some(CpuUsageDecision {
            rule_name: "High".to_owned(),
            power_plan_guid: Some("cpu-guid".to_owned()),
            usage_percent: 95.0,
        });

        assert_context_matches_decision_engine(&settings, input);
    }

    #[test]
    fn context_adapter_matches_activity_suppressing_schedule() {
        let settings = test_settings();
        let mut input = decision_input();
        input.activity_state = ActivityState::Idle;
        input.schedule = Some(ScheduleDecision {
            rule_name: "Work".to_owned(),
            power_plan_guid: Some("schedule-guid".to_owned()),
        });

        assert_context_matches_decision_engine(&settings, input);
    }

    #[test]
    fn context_adapter_matches_no_target_active_activity() {
        let mut settings = test_settings();
        settings.activity_mode.switch_to_performance_on_resume = false;
        let mut input = decision_input();
        input.activity_state = ActivityState::Active;
        input.schedule = Some(ScheduleDecision {
            rule_name: "Work".to_owned(),
            power_plan_guid: Some("schedule-guid".to_owned()),
        });

        assert_context_matches_decision_engine(&settings, input);
    }

    #[test]
    fn context_adapter_matches_schedule_when_activity_disabled() {
        let mut settings = test_settings();
        settings.activity_mode.enabled = false;
        let mut input = decision_input();
        input.schedule = Some(ScheduleDecision {
            rule_name: "Work".to_owned(),
            power_plan_guid: Some("schedule-guid".to_owned()),
        });

        assert_context_matches_decision_engine(&settings, input);
    }

    #[test]
    fn context_adapter_matches_default_active_when_activity_disabled() {
        let mut settings = test_settings();
        settings.activity_mode.enabled = false;

        assert_context_matches_decision_engine(&settings, decision_input());
    }
}
