use crate::{
    activity::ActivityState,
    config::Settings,
    features::power_plan_control::{ByCpuLoadDecision, ByTimeDecision},
    foreground::same_executable_path,
};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionState {
    Disabled,
    PausedWhilePluggedIn,
    ByFocus,
    ByRunningApp,
    ByTime,
    ByCpuLoad,
    ByActivityIdle,
    ByActivityActive,
    NoPowerPlanSelected,
}

#[derive(Debug, Clone)]
pub struct DecisionInput {
    pub activity_state: ActivityState,
    pub foreground_executable_path: Option<String>,
    pub foreground_process_id: Option<u32>,
    pub by_running_app_known: bool,
    pub cpu_load_known: bool,
    pub plugged_in: Option<bool>,
    pub by_running_app: Option<ByRunningAppDecision>,
    pub by_time: Option<ByTimeDecision>,
    pub by_cpu_load: Option<ByCpuLoadDecision>,
}

#[derive(Debug, Clone)]
pub struct ByRunningAppDecision {
    pub rule_index: usize,
    pub rule_name: String,
    pub process_name: String,
    pub power_plan_guid: String,
}

#[derive(Debug, Clone)]
pub struct DecisionOutcome {
    pub rule_index: Option<usize>,
    pub power_plan_guid: Option<String>,
    pub state: DecisionState,
    pub reason: String,
    pub focus: FocusDecision,
    pub restore_previous_plan: bool,
    pub running_app_known: bool,
    pub cpu_load_known: bool,
    pub activity_known: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct FocusDecision {
    pub enabled: bool,
    pub known: bool,
    pub process_id: Option<u32>,
}

pub fn decide(settings: &Settings, input: DecisionInput) -> DecisionOutcome {
    let focus = FocusDecision {
        enabled: settings.by_focus.enabled
            && settings.by_focus.rules.iter().any(|rule| {
                rule.enabled
                    && rule.power_plan_guid.is_some()
                    && !rule.executable_path.trim().is_empty()
            }),
        known: input.foreground_executable_path.is_some(),
        process_id: input.foreground_process_id,
    };
    let running_app_known = input.by_running_app_known
        || !settings.by_running_app.enabled
        || !settings.by_running_app.rules.iter().any(|rule| {
            rule.enabled
                && rule.power_plan_guid.is_some()
                && !rule.executable_path.trim().is_empty()
        });
    let cpu_load_known = input.cpu_load_known
        || !settings.by_cpu_load.enabled
        || !settings
            .by_cpu_load
            .rules
            .iter()
            .any(|rule| rule.enabled && rule.power_plan_guid.is_some());
    let activity_known = input.activity_state != ActivityState::Unknown
        || !settings.by_activity.enabled
        || settings.by_activity.power_plans.power_save_guid.is_none();
    let cpu_restore = input.by_cpu_load.as_ref().is_some_and(|decision| {
        !decision.is_else
            && settings
                .by_cpu_load
                .rules
                .get(decision.rule_index)
                .is_some_and(|rule| {
                    rule.else_enabled
                        && rule.else_action
                            == crate::config::PowerPlanReturnAction::RestorePreviousPlan
                })
    });
    let mut outcome = decide_plan(settings, input);
    outcome.focus = focus;
    outcome.running_app_known = running_app_known;
    outcome.cpu_load_known = cpu_load_known;
    outcome.activity_known = activity_known;
    outcome.restore_previous_plan = match outcome.state {
        DecisionState::ByFocus => settings.by_focus.restore_previous_plan,
        DecisionState::ByRunningApp => settings.by_running_app.restore_previous_plan,
        DecisionState::ByTime => settings.by_time.restore_previous_plan,
        DecisionState::ByCpuLoad => cpu_restore,
        DecisionState::ByActivityIdle => {
            settings.by_activity.resume_action
                == crate::config::PowerPlanReturnAction::RestorePreviousPlan
        }
        _ => false,
    };
    outcome
}

fn decide_plan(settings: &Settings, input: DecisionInput) -> DecisionOutcome {
    if !settings.general.enabled {
        return DecisionOutcome::without_power_plan(
            DecisionState::Disabled,
            "Automation is disabled.",
        );
    }

    if settings.general.pause_power_plan_switching_while_plugged_in
        && input.plugged_in == Some(true)
    {
        return DecisionOutcome::without_power_plan(
            DecisionState::PausedWhilePluggedIn,
            "Power-plan switching is paused while plugged in.",
        );
    }

    let foreground_executable_path = input.foreground_executable_path.as_deref();

    if let Some(foreground_executable_path) =
        foreground_executable_path.filter(|_| settings.by_focus.enabled)
    {
        for (rule_index, rule) in settings.by_focus.rules.iter().enumerate() {
            if rule.enabled
                && same_executable_path(
                    Path::new(&rule.executable_path),
                    Path::new(foreground_executable_path),
                )
            {
                if let Some(power_plan_guid) = rule.power_plan_guid.clone() {
                    let mut outcome = DecisionOutcome::with_power_plan(
                        Some(power_plan_guid),
                        DecisionState::ByFocus,
                        format!(
                            "{foreground_executable_path} matched By Focus rule '{}'.",
                            rule.name
                        ),
                    );
                    outcome.rule_index = Some(rule_index);
                    return outcome;
                }
                break;
            }
        }
    }

    if let Some(by_running_app) = input.by_running_app {
        let mut outcome = DecisionOutcome::with_power_plan(
            Some(by_running_app.power_plan_guid),
            DecisionState::ByRunningApp,
            format!(
                "{} is running and matched By Running App rule '{}'.",
                by_running_app.process_name, by_running_app.rule_name
            ),
        );
        outcome.rule_index = Some(by_running_app.rule_index);
        return outcome;
    }

    if let Some(by_cpu_load_decision) = input.by_cpu_load {
        let mut outcome = DecisionOutcome::with_power_plan(
            Some(by_cpu_load_decision.power_plan_guid),
            DecisionState::ByCpuLoad,
            format!(
                "CPU load is {:.1}% and selected By CPU Load rule '{}'.",
                by_cpu_load_decision.usage_percent, by_cpu_load_decision.rule_name
            ),
        );
        outcome.rule_index = Some(by_cpu_load_decision.rule_index);
        return outcome;
    }

    let deferred_activity_outcome = if settings.by_activity.enabled {
        let outcome = match input.activity_state {
            ActivityState::Idle => DecisionOutcome::with_power_plan(
                settings.by_activity.power_plans.power_save_guid.clone(),
                DecisionState::ByActivityIdle,
                "User input has been idle past the configured timeout.",
            ),
            ActivityState::Active
                if settings.by_activity.resume_action
                    == crate::config::PowerPlanReturnAction::RestorePreviousPlan =>
            {
                DecisionOutcome::without_power_plan(
                    DecisionState::ByActivityActive,
                    "Recent user input detected; releasing the Idle plan.",
                )
            }
            ActivityState::Active
                if settings.by_activity.switch_to_performance_on_resume
                    && settings.by_activity.input_detection.any_enabled() =>
            {
                DecisionOutcome::with_power_plan(
                    settings.by_activity.power_plans.performance_guid.clone(),
                    DecisionState::ByActivityActive,
                    "Recent user input detected; using the Active plan.",
                )
            }
            ActivityState::Active if settings.by_activity.switch_to_performance_on_resume => {
                DecisionOutcome::without_power_plan(
                    DecisionState::ByActivityActive,
                    "Recent user input detected, but no input detection types are enabled.",
                )
            }
            ActivityState::Active => DecisionOutcome::without_power_plan(
                DecisionState::ByActivityActive,
                "Recent user input detected; Active plan switching is disabled.",
            ),
            ActivityState::Unknown => DecisionOutcome::without_power_plan(
                DecisionState::NoPowerPlanSelected,
                "Input activity could not be detected.",
            ),
        };
        if outcome.power_plan_guid.is_some() {
            return outcome;
        }
        Some(outcome)
    } else {
        None
    };

    if let Some(by_time_decision) = input.by_time {
        let mut outcome = DecisionOutcome::with_power_plan(
            Some(by_time_decision.power_plan_guid),
            DecisionState::ByTime,
            format!("By Time rule '{}' is active.", by_time_decision.rule_name),
        );
        outcome.rule_index = Some(by_time_decision.rule_index);
        return outcome;
    }

    if let Some(outcome) = deferred_activity_outcome {
        return outcome;
    }
    DecisionOutcome::without_power_plan(
        DecisionState::NoPowerPlanSelected,
        "No enabled power-plan rule currently applies.",
    )
}

impl DecisionOutcome {
    fn with_power_plan(
        power_plan_guid: Option<String>,
        mut state: DecisionState,
        reason: impl Into<String>,
    ) -> Self {
        let mut reason = reason.into();
        if power_plan_guid.is_none() {
            state = DecisionState::NoPowerPlanSelected;
            reason.push_str(" Select the required power plan first.");
        }
        Self {
            rule_index: None,
            power_plan_guid,
            state,
            reason,
            focus: FocusDecision::default(),
            restore_previous_plan: false,
            running_app_known: true,
            cpu_load_known: true,
            activity_known: true,
        }
    }

    fn without_power_plan(state: DecisionState, reason: impl Into<String>) -> Self {
        Self {
            rule_index: None,
            power_plan_guid: None,
            state,
            reason: reason.into(),
            focus: FocusDecision::default(),
            restore_previous_plan: false,
            running_app_known: true,
            cpu_load_known: true,
            activity_known: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        activity::ActivityState,
        config::{ByFocusRule, ByFocusSettings, PowerPlanSettings, Settings},
        features::power_plan_control::{ByCpuLoadDecision, ByTimeDecision},
    };

    fn test_settings() -> Settings {
        let mut settings = Settings::default();
        settings.by_activity.enabled = true;
        settings.by_activity.power_plans.power_save_guid = Some("idle-guid".to_owned());
        settings.by_activity.power_plans.performance_guid = Some("active-guid".to_owned());
        settings.by_time.enabled = false;
        settings.by_time.rules.clear();
        settings.by_focus = ByFocusSettings {
            enabled: true,
            restore_previous_plan: false,
            rules: Vec::new(),
        };
        settings
    }

    fn inactive_input() -> DecisionInput {
        DecisionInput {
            activity_state: ActivityState::Active,
            foreground_executable_path: None,
            foreground_process_id: None,
            by_running_app_known: true,
            cpu_load_known: true,
            plugged_in: None,
            by_running_app: None,
            by_time: None,
            by_cpu_load: None,
        }
    }

    #[test]
    fn restoration_policy_follows_the_selected_running_time_or_cpu_mode() {
        for restore in [false, true] {
            let mut settings = test_settings();
            settings.by_activity.enabled = false;
            settings.by_running_app.restore_previous_plan = restore;
            settings.by_time.restore_previous_plan = restore;
            settings.by_cpu_load.rules = vec![crate::config::ByCpuLoadRule {
                else_enabled: restore,
                else_action: crate::config::PowerPlanReturnAction::RestorePreviousPlan,
                ..crate::config::ByCpuLoadRule::default()
            }];
            for state in [
                DecisionState::ByRunningApp,
                DecisionState::ByTime,
                DecisionState::ByCpuLoad,
            ] {
                let mut input = inactive_input();
                match state {
                    DecisionState::ByRunningApp => {
                        input.by_running_app = Some(ByRunningAppDecision {
                            rule_index: 0,
                            rule_name: "App".into(),
                            process_name: "app.exe".into(),
                            power_plan_guid: "app-plan".into(),
                        })
                    }
                    DecisionState::ByTime => {
                        input.by_time = Some(ByTimeDecision {
                            rule_index: 0,
                            rule_name: "Window".into(),
                            power_plan_guid: "time-plan".into(),
                        })
                    }
                    DecisionState::ByCpuLoad => {
                        input.by_cpu_load = Some(ByCpuLoadDecision {
                            rule_index: 0,
                            rule_name: "Load".into(),
                            power_plan_guid: "cpu-plan".into(),
                            usage_percent: 80.0,
                            is_else: false,
                        })
                    }
                    _ => unreachable!(),
                }
                let outcome = decide(&settings, input.clone());
                assert_eq!(outcome.state, state);
                assert_eq!(outcome.restore_previous_plan, restore);
                if let Some(cpu) = &mut input.by_cpu_load {
                    cpu.is_else = true;
                    assert!(!decide(&settings, input).restore_previous_plan);
                }
            }
        }
    }

    #[test]
    fn activity_restore_captures_only_idle_and_ignores_the_dormant_active_plan() {
        let mut settings = test_settings();
        settings.by_activity.resume_action =
            crate::config::PowerPlanReturnAction::RestorePreviousPlan;
        for state in [
            ActivityState::Idle,
            ActivityState::Active,
            ActivityState::Unknown,
        ] {
            let mut input = inactive_input();
            input.activity_state = state;
            let outcome = decide(&settings, input);
            assert_eq!(outcome.restore_previous_plan, state == ActivityState::Idle);
            assert_eq!(outcome.activity_known, state != ActivityState::Unknown);
            assert_eq!(
                outcome.power_plan_guid.as_deref(),
                (state == ActivityState::Idle).then_some("idle-guid")
            );
        }
        settings.by_activity.resume_action = crate::config::PowerPlanReturnAction::SelectedPlan;
        let outcome = decide(&settings, inactive_input());
        assert_eq!(outcome.power_plan_guid.as_deref(), Some("active-guid"));
        assert!(!outcome.restore_previous_plan);
    }

    #[test]
    fn focus_context_distinguishes_unreadable_windows_from_edited_or_disabled_rules() {
        let mut settings = test_settings();
        settings.by_activity.enabled = false;
        settings.by_focus.restore_previous_plan = true;
        settings.by_focus.rules.push(ByFocusRule {
            enabled: true,
            executable_path: "focused.exe".into(),
            power_plan_guid: Some("focused-plan".into()),
            ..ByFocusRule::default()
        });
        for (path, expected_state, known) in [
            (Some("focused.exe"), DecisionState::ByFocus, true),
            (Some("other.exe"), DecisionState::NoPowerPlanSelected, true),
            (None, DecisionState::NoPowerPlanSelected, false),
        ] {
            let outcome = decide(
                &settings,
                DecisionInput {
                    activity_state: ActivityState::Active,
                    foreground_executable_path: path.map(str::to_owned),
                    foreground_process_id: Some(10),
                    by_running_app_known: true,
                    cpu_load_known: true,
                    plugged_in: None,
                    by_running_app: None,
                    by_time: None,
                    by_cpu_load: None,
                },
            );
            assert_eq!(outcome.state, expected_state);
            assert!(outcome.focus.enabled);
            assert_eq!(outcome.focus.known, known);
            assert_eq!(outcome.focus.process_id, Some(10));
            assert_eq!(
                outcome.restore_previous_plan,
                expected_state == DecisionState::ByFocus
            );
        }
        settings.by_focus.rules[0].power_plan_guid = None;
        let outcome = decide(
            &settings,
            DecisionInput {
                activity_state: ActivityState::Active,
                foreground_executable_path: None,
                foreground_process_id: Some(10),
                by_running_app_known: true,
                cpu_load_known: true,
                plugged_in: None,
                by_running_app: None,
                by_time: None,
                by_cpu_load: None,
            },
        );
        assert!(!outcome.focus.enabled);
    }

    #[test]
    fn cpu_usage_overrides_activity_and_schedule() {
        let outcome = decide(
            &test_settings(),
            DecisionInput {
                activity_state: ActivityState::Idle,
                foreground_executable_path: None,
                foreground_process_id: None,
                by_running_app_known: true,
                cpu_load_known: true,
                plugged_in: None,
                by_running_app: None,
                by_time: Some(ByTimeDecision {
                    rule_index: 0,
                    rule_name: "Work hours".to_owned(),
                    power_plan_guid: "schedule-custom".to_owned(),
                }),
                by_cpu_load: Some(ByCpuLoadDecision {
                    is_else: false,
                    rule_index: 0,
                    rule_name: "Low CPU".to_owned(),
                    power_plan_guid: "cpu-low-guid".to_owned(),
                    usage_percent: 10.0,
                }),
            },
        );

        assert_eq!(outcome.state, DecisionState::ByCpuLoad);
        assert_eq!(outcome.power_plan_guid.as_deref(), Some("cpu-low-guid"));
    }

    #[test]
    fn activity_overrides_schedule_when_cpu_does_not_match() {
        let outcome = decide(
            &test_settings(),
            DecisionInput {
                activity_state: ActivityState::Idle,
                foreground_executable_path: None,
                foreground_process_id: None,
                by_running_app_known: true,
                cpu_load_known: true,
                plugged_in: None,
                by_running_app: None,
                by_time: Some(ByTimeDecision {
                    rule_index: 0,
                    rule_name: "Work hours".to_owned(),
                    power_plan_guid: "schedule-custom".to_owned(),
                }),
                by_cpu_load: None,
            },
        );

        assert_eq!(outcome.state, DecisionState::ByActivityIdle);
        assert_eq!(outcome.power_plan_guid.as_deref(), Some("idle-guid"));
    }

    #[test]
    fn schedule_applies_when_activity_and_cpu_do_not_match() {
        let mut settings = test_settings();
        settings.by_activity.enabled = false;

        let outcome = decide(
            &settings,
            DecisionInput {
                activity_state: ActivityState::Active,
                foreground_executable_path: None,
                foreground_process_id: None,
                by_running_app_known: true,
                cpu_load_known: true,
                plugged_in: None,
                by_running_app: None,
                by_time: Some(ByTimeDecision {
                    rule_index: 0,
                    rule_name: "Work hours".to_owned(),
                    power_plan_guid: "schedule-custom".to_owned(),
                }),
                by_cpu_load: None,
            },
        );

        assert_eq!(outcome.state, DecisionState::ByTime);
        assert_eq!(outcome.power_plan_guid.as_deref(), Some("schedule-custom"));
    }

    #[test]
    fn schedule_applies_when_activity_has_no_plan_for_current_state() {
        let mut settings = test_settings();
        settings.by_activity.power_plans.power_save_guid = None;

        let outcome = decide(
            &settings,
            DecisionInput {
                activity_state: ActivityState::Idle,
                foreground_executable_path: None,
                foreground_process_id: None,
                by_running_app_known: true,
                cpu_load_known: true,
                plugged_in: None,
                by_running_app: None,
                by_time: Some(ByTimeDecision {
                    rule_index: 0,
                    rule_name: "Work hours".to_owned(),
                    power_plan_guid: "schedule-custom".to_owned(),
                }),
                by_cpu_load: None,
            },
        );

        assert_eq!(outcome.state, DecisionState::ByTime);
        assert_eq!(outcome.power_plan_guid.as_deref(), Some("schedule-custom"));
    }

    #[test]
    fn activity_applies_without_focus_or_schedule() {
        let mut settings = test_settings();
        let input = DecisionInput {
            activity_state: ActivityState::Idle,
            foreground_executable_path: None,
            foreground_process_id: None,
            by_running_app_known: true,
            cpu_load_known: true,
            plugged_in: None,
            by_running_app: None,
            by_time: None,
            by_cpu_load: None,
        };
        let outcome = decide(&settings, input.clone());

        assert_eq!(outcome.state, DecisionState::ByActivityIdle);
        assert_eq!(outcome.power_plan_guid.as_deref(), Some("idle-guid"));
        assert_eq!(
            outcome.reason,
            "User input has been idle past the configured timeout."
        );

        settings.by_activity.power_plans.power_save_guid = None;
        let outcome = decide(&settings, input);
        assert_eq!(outcome.state, DecisionState::NoPowerPlanSelected);
        assert_eq!(outcome.power_plan_guid, None);
        assert_eq!(outcome.rule_index, None);
        assert_eq!(
            outcome.reason,
            "User input has been idle past the configured timeout. Select the required power plan first."
        );
    }

    #[test]
    fn disabled_by_focus_are_ignored() {
        let mut settings = test_settings();
        settings.by_focus.enabled = false;
        settings.by_focus.rules.push(ByFocusRule {
            enabled: true,
            name: "Backup".to_owned(),
            executable_path: "backup.exe".to_owned(),
            power_plan_guid: Some("backup-guid".to_owned()),
        });

        let outcome = decide(
            &settings,
            DecisionInput {
                activity_state: ActivityState::Idle,
                foreground_executable_path: Some("backup.exe".to_owned()),
                foreground_process_id: None,
                by_running_app_known: true,
                cpu_load_known: true,
                plugged_in: None,
                by_running_app: None,
                by_time: None,
                by_cpu_load: None,
            },
        );

        assert_eq!(outcome.state, DecisionState::ByActivityIdle);
        assert_eq!(outcome.power_plan_guid.as_deref(), Some("idle-guid"));
    }

    #[test]
    fn plugged_in_pause_prevents_power_plan_target() {
        let mut settings = test_settings();
        settings.general.pause_power_plan_switching_while_plugged_in = true;

        let outcome = decide(
            &settings,
            DecisionInput {
                activity_state: ActivityState::Idle,
                foreground_executable_path: None,
                foreground_process_id: None,
                by_running_app_known: true,
                cpu_load_known: true,
                plugged_in: Some(true),
                by_running_app: None,
                by_time: None,
                by_cpu_load: None,
            },
        );

        assert_eq!(outcome.state, DecisionState::PausedWhilePluggedIn);
        assert_eq!(outcome.power_plan_guid, None);
    }

    #[test]
    fn decisions_use_their_own_power_plan_mappings() {
        let mut settings = test_settings();
        settings.by_activity.power_plans = PowerPlanSettings {
            power_save_guid: Some("activity-idle".to_owned()),
            performance_guid: Some("activity-active".to_owned()),
        };
        settings.by_focus.rules = vec![ByFocusRule {
            enabled: true,
            name: "Game".to_owned(),
            executable_path: "game.exe".to_owned(),
            power_plan_guid: Some("focus-custom".to_owned()),
        }];

        let focus = decide(
            &settings,
            DecisionInput {
                activity_state: ActivityState::Active,
                foreground_executable_path: Some("game.exe".to_owned()),
                foreground_process_id: None,
                by_running_app_known: true,
                cpu_load_known: true,
                plugged_in: None,
                by_running_app: None,
                by_time: None,
                by_cpu_load: None,
            },
        );
        assert_eq!(focus.power_plan_guid.as_deref(), Some("focus-custom"));

        settings.by_activity.enabled = false;
        let by_time_decision = decide(
            &settings,
            DecisionInput {
                activity_state: ActivityState::Active,
                foreground_executable_path: None,
                foreground_process_id: None,
                by_running_app_known: true,
                cpu_load_known: true,
                plugged_in: None,
                by_running_app: None,
                by_time: Some(ByTimeDecision {
                    rule_index: 0,
                    rule_name: "Quiet".to_owned(),
                    power_plan_guid: "schedule-custom".to_owned(),
                }),
                by_cpu_load: None,
            },
        );
        assert_eq!(
            by_time_decision.power_plan_guid.as_deref(),
            Some("schedule-custom")
        );

        settings.by_activity.enabled = false;
        let cpu = decide(
            &settings,
            DecisionInput {
                activity_state: ActivityState::Idle,
                foreground_executable_path: None,
                foreground_process_id: None,
                by_running_app_known: true,
                cpu_load_known: true,
                plugged_in: None,
                by_running_app: None,
                by_time: None,
                by_cpu_load: Some(ByCpuLoadDecision {
                    is_else: false,
                    rule_index: 0,
                    rule_name: "High CPU".to_owned(),
                    power_plan_guid: "cpu-custom".to_owned(),
                    usage_percent: 90.0,
                }),
            },
        );
        assert_eq!(cpu.power_plan_guid.as_deref(), Some("cpu-custom"));

        settings.by_activity.enabled = true;
        let activity = decide(
            &settings,
            DecisionInput {
                activity_state: ActivityState::Idle,
                foreground_executable_path: None,
                foreground_process_id: None,
                by_running_app_known: true,
                cpu_load_known: true,
                plugged_in: None,
                by_running_app: None,
                by_time: None,
                by_cpu_load: None,
            },
        );
        assert_eq!(activity.power_plan_guid.as_deref(), Some("activity-idle"));
    }

    #[test]
    fn focus_rule_can_target_any_power_plan() {
        let mut settings = test_settings();
        settings.by_focus.rules = vec![ByFocusRule {
            enabled: true,
            name: "Editing".to_owned(),
            executable_path: "editor.exe".to_owned(),
            power_plan_guid: Some("balanced-guid".to_owned()),
        }];

        let outcome = decide(
            &settings,
            DecisionInput {
                activity_state: ActivityState::Idle,
                foreground_executable_path: Some("editor.exe".to_owned()),
                foreground_process_id: None,
                by_running_app_known: true,
                cpu_load_known: true,
                plugged_in: None,
                by_running_app: None,
                by_time: None,
                by_cpu_load: None,
            },
        );

        assert_eq!(outcome.state, DecisionState::ByFocus);
        assert_eq!(outcome.rule_index, Some(0));
        assert_eq!(outcome.power_plan_guid.as_deref(), Some("balanced-guid"));
    }

    #[test]
    fn focus_rule_without_power_plan_combines_with_cpu_before_activity() {
        let mut settings = test_settings();
        settings.by_focus.rules = vec![ByFocusRule {
            enabled: true,
            name: "Editing".to_owned(),
            executable_path: "editor.exe".to_owned(),
            power_plan_guid: None,
        }];

        let outcome = decide(
            &settings,
            DecisionInput {
                activity_state: ActivityState::Idle,
                foreground_executable_path: Some("editor.exe".to_owned()),
                foreground_process_id: None,
                by_running_app_known: true,
                cpu_load_known: true,
                plugged_in: None,
                by_running_app: None,
                by_time: Some(ByTimeDecision {
                    rule_index: 0,
                    rule_name: "Work hours".to_owned(),
                    power_plan_guid: "schedule-custom".to_owned(),
                }),
                by_cpu_load: Some(ByCpuLoadDecision {
                    is_else: false,
                    rule_index: 0,
                    rule_name: "High CPU".to_owned(),
                    power_plan_guid: "cpu-high-guid".to_owned(),
                    usage_percent: 90.0,
                }),
            },
        );

        assert_eq!(outcome.state, DecisionState::ByCpuLoad);
        assert_eq!(outcome.power_plan_guid.as_deref(), Some("cpu-high-guid"));
    }

    #[test]
    fn focus_rule_without_power_plan_combines_with_cpu_rule() {
        let mut settings = test_settings();
        settings.by_focus.rules = vec![ByFocusRule {
            enabled: true,
            name: "Rendering".to_owned(),
            executable_path: "render.exe".to_owned(),
            power_plan_guid: None,
        }];

        let outcome = decide(
            &settings,
            DecisionInput {
                activity_state: ActivityState::Idle,
                foreground_executable_path: Some("render.exe".to_owned()),
                foreground_process_id: None,
                by_running_app_known: true,
                cpu_load_known: true,
                plugged_in: None,
                by_running_app: None,
                by_time: None,
                by_cpu_load: Some(ByCpuLoadDecision {
                    is_else: false,
                    rule_index: 0,
                    rule_name: "High CPU".to_owned(),
                    power_plan_guid: "cpu-high-guid".to_owned(),
                    usage_percent: 90.0,
                }),
            },
        );

        assert_eq!(outcome.state, DecisionState::ByCpuLoad);
        assert_eq!(outcome.power_plan_guid.as_deref(), Some("cpu-high-guid"));
    }

    #[test]
    fn by_running_app_overrides_cpu_activity_and_schedule() {
        let outcome = decide(
            &test_settings(),
            DecisionInput {
                activity_state: ActivityState::Idle,
                foreground_executable_path: None,
                foreground_process_id: None,
                by_running_app_known: true,
                cpu_load_known: true,
                plugged_in: None,
                by_running_app: Some(ByRunningAppDecision {
                    rule_index: 0,
                    rule_name: "Game".to_owned(),
                    process_name: "game.exe".to_owned(),
                    power_plan_guid: "by-running-app-guid".to_owned(),
                }),
                by_time: Some(ByTimeDecision {
                    rule_index: 0,
                    rule_name: "Work hours".to_owned(),
                    power_plan_guid: "schedule-custom".to_owned(),
                }),
                by_cpu_load: Some(ByCpuLoadDecision {
                    is_else: false,
                    rule_index: 0,
                    rule_name: "High CPU".to_owned(),
                    power_plan_guid: "cpu-high-guid".to_owned(),
                    usage_percent: 90.0,
                }),
            },
        );

        assert_eq!(outcome.state, DecisionState::ByRunningApp);
        assert_eq!(outcome.rule_index, Some(0));
        assert_eq!(
            outcome.power_plan_guid.as_deref(),
            Some("by-running-app-guid")
        );
    }

    #[test]
    fn focus_rule_overrides_by_running_app_when_both_match() {
        let mut settings = test_settings();
        settings.by_focus.rules = vec![ByFocusRule {
            enabled: true,
            name: "Game focus".to_owned(),
            executable_path: "game.exe".to_owned(),
            power_plan_guid: Some("foreground-guid".to_owned()),
        }];

        let outcome = decide(
            &settings,
            DecisionInput {
                activity_state: ActivityState::Idle,
                foreground_executable_path: Some("game.exe".to_owned()),
                foreground_process_id: None,
                by_running_app_known: true,
                cpu_load_known: true,
                plugged_in: None,
                by_running_app: Some(ByRunningAppDecision {
                    rule_index: 0,
                    rule_name: "Game running".to_owned(),
                    process_name: "game.exe".to_owned(),
                    power_plan_guid: "by-running-app-guid".to_owned(),
                }),
                by_time: None,
                by_cpu_load: None,
            },
        );

        assert_eq!(outcome.state, DecisionState::ByFocus);
        assert_eq!(outcome.rule_index, Some(0));
        assert_eq!(outcome.power_plan_guid.as_deref(), Some("foreground-guid"));
    }

    #[test]
    fn disabled_focus_rule_is_ignored() {
        let mut settings = test_settings();
        settings.by_focus.rules = vec![ByFocusRule {
            enabled: false,
            name: "Editing".to_owned(),
            executable_path: "editor.exe".to_owned(),
            power_plan_guid: Some("balanced-guid".to_owned()),
        }];

        let outcome = decide(
            &settings,
            DecisionInput {
                activity_state: ActivityState::Idle,
                foreground_executable_path: Some("editor.exe".to_owned()),
                foreground_process_id: None,
                by_running_app_known: true,
                cpu_load_known: true,
                plugged_in: None,
                by_running_app: None,
                by_time: None,
                by_cpu_load: None,
            },
        );

        assert_eq!(outcome.state, DecisionState::ByActivityIdle);
        assert_eq!(outcome.power_plan_guid.as_deref(), Some("idle-guid"));
    }

    #[test]
    fn unmatched_rules_do_not_use_disabled_activity_plans() {
        let mut settings = test_settings();
        settings.by_activity.enabled = false;
        settings.by_focus.rules.push(ByFocusRule {
            enabled: true,
            name: "Editor".to_owned(),
            executable_path: "editor.exe".to_owned(),
            power_plan_guid: Some("editor-guid".to_owned()),
        });

        for activity_state in [
            ActivityState::Active,
            ActivityState::Idle,
            ActivityState::Unknown,
        ] {
            let outcome = decide(
                &settings,
                DecisionInput {
                    activity_state,
                    foreground_executable_path: Some("other.exe".to_owned()),
                    foreground_process_id: None,
                    by_running_app_known: true,
                    cpu_load_known: true,
                    plugged_in: None,
                    by_running_app: None,
                    by_time: None,
                    by_cpu_load: None,
                },
            );

            assert_eq!(outcome.state, DecisionState::NoPowerPlanSelected);
            assert_eq!(outcome.power_plan_guid, None);
            assert_eq!(outcome.rule_index, None);
        }
    }
}
