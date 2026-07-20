use crate::{
    activity::ActivityState,
    config::{PowerPlanSettings, Settings},
    foreground::same_process_name,
    scheduler::{ByCpuLoadDecision, ByTimeDecision},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionState {
    Disabled,
    PluggedInPause,
    ByForegroundRule,
    ByRunningApp,
    ScheduledRule,
    CpuLoadRule,
    IdlePowerSave,
    ActivePerformance,
    NoTargetPlan,
}

#[derive(Debug, Clone)]
pub struct DecisionInput {
    pub activity_state: ActivityState,
    pub foreground_app: Option<String>,
    pub plugged_in: Option<bool>,
    pub by_running_app: Option<ByRunningAppDecision>,
    pub by_time: Option<ByTimeDecision>,
    pub by_cpu_load: Option<ByCpuLoadDecision>,
}

#[derive(Debug, Clone)]
pub struct ByRunningAppDecision {
    pub rule_name: String,
    pub process_name: String,
    pub power_plan_guid: String,
}

#[derive(Debug, Clone)]
pub struct DecisionOutcome {
    pub target_guid: Option<String>,
    pub state: DecisionState,
    pub reason: String,
}

#[derive(Debug, Default)]
pub struct DecisionEngine;

impl DecisionEngine {
    pub fn decide(&self, settings: &Settings, input: DecisionInput) -> DecisionOutcome {
        if !settings.general.enabled {
            return DecisionOutcome::without_target(
                DecisionState::Disabled,
                "Automation is disabled.",
            );
        }

        if settings.general.pause_power_plan_switching_while_plugged_in
            && input.plugged_in == Some(true)
        {
            return DecisionOutcome::without_target(
                DecisionState::PluggedInPause,
                "Power-plan switching is paused while plugged in.",
            );
        }

        let foreground = input.foreground_app.as_deref().map(str::to_ascii_lowercase);

        if let Some(app) = foreground
            .as_deref()
            .filter(|_| settings.by_foreground.enabled)
        {
            for rule in &settings.by_foreground.rules {
                if rule.enabled && same_process_name(&rule.process_name, app) {
                    if let Some(power_plan_guid) = rule.power_plan_guid.clone() {
                        return DecisionOutcome::with_target(
                            Some(power_plan_guid),
                            DecisionState::ByForegroundRule,
                            format!("{app} matched foreground rule '{}'.", rule.name),
                        );
                    }
                    break;
                }
            }
        }

        if let Some(by_running_app) = input.by_running_app {
            return DecisionOutcome::with_target(
                Some(by_running_app.power_plan_guid),
                DecisionState::ByRunningApp,
                format!(
                    "{} is running and matched By Running App rule '{}'.",
                    by_running_app.process_name, by_running_app.rule_name
                ),
            );
        }

        if let Some(cpu_usage) = input.by_cpu_load {
            return DecisionOutcome::with_target(
                cpu_usage.power_plan_guid,
                DecisionState::CpuLoadRule,
                format!(
                    "CPU load is {:.1}% and matched rule '{}'.",
                    cpu_usage.usage_percent, cpu_usage.rule_name
                ),
            );
        }

        if settings.by_activity.enabled {
            return match input.activity_state {
                ActivityState::Idle => DecisionOutcome::with_target(
                    idle_plan(&settings.by_activity.power_plans),
                    DecisionState::IdlePowerSave,
                    "User input has been idle past the configured timeout.",
                ),
                ActivityState::Active
                    if settings.by_activity.switch_to_performance_on_resume
                        && settings.by_activity.input_detection.any_enabled() =>
                {
                    DecisionOutcome::with_target(
                        active_plan(&settings.by_activity.power_plans),
                        DecisionState::ActivePerformance,
                        "Recent user input detected; using the Active plan.",
                    )
                }
                ActivityState::Active if settings.by_activity.switch_to_performance_on_resume => {
                    DecisionOutcome::without_target(
                        DecisionState::ActivePerformance,
                        "Recent user input detected, but no input detection types are enabled.",
                    )
                }
                ActivityState::Active => DecisionOutcome::without_target(
                    DecisionState::ActivePerformance,
                    "Recent user input detected; Active plan switching is disabled.",
                ),
                ActivityState::Unknown => DecisionOutcome::without_target(
                    DecisionState::NoTargetPlan,
                    "Input activity could not be detected.",
                ),
            };
        }

        if let Some(schedule) = input.by_time {
            return DecisionOutcome::with_target(
                schedule.power_plan_guid,
                DecisionState::ScheduledRule,
                format!("By Time rule '{}' is active.", schedule.rule_name),
            );
        }

        DecisionOutcome::with_target(
            active_plan(&settings.by_activity.power_plans),
            DecisionState::ActivePerformance,
            "Using the configured default Active plan.",
        )
    }
}

impl DecisionOutcome {
    fn with_target(
        target_guid: Option<String>,
        state: DecisionState,
        reason: impl Into<String>,
    ) -> Self {
        let reason = reason.into();
        if target_guid.is_some() {
            Self {
                target_guid,
                state,
                reason,
            }
        } else {
            Self {
                target_guid: None,
                state: DecisionState::NoTargetPlan,
                reason: format!("{reason} Select the required power plan first."),
            }
        }
    }

    fn without_target(state: DecisionState, reason: impl Into<String>) -> Self {
        Self {
            target_guid: None,
            state,
            reason: reason.into(),
        }
    }
}

fn idle_plan(power_plans: &PowerPlanSettings) -> Option<String> {
    power_plans.power_save_guid.clone()
}

fn active_plan(power_plans: &PowerPlanSettings) -> Option<String> {
    power_plans.performance_guid.clone()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        activity::ActivityState,
        config::{ByForegroundRule, ByForegroundSettings, PowerPlanSettings, Settings},
        scheduler::{ByCpuLoadDecision, ByTimeDecision},
    };

    fn test_settings() -> Settings {
        let mut settings = Settings::default();
        settings.by_activity.power_plans.power_save_guid = Some("idle-guid".to_owned());
        settings.by_activity.power_plans.performance_guid = Some("active-guid".to_owned());
        settings.by_time.enabled = false;
        settings.by_time.rules.clear();
        settings.by_foreground = ByForegroundSettings {
            enabled: true,
            rules: Vec::new(),
        };
        settings
    }

    #[test]
    fn cpu_usage_overrides_activity_and_schedule() {
        let outcome = DecisionEngine.decide(
            &test_settings(),
            DecisionInput {
                activity_state: ActivityState::Idle,
                foreground_app: None,
                plugged_in: None,
                by_running_app: None,
                by_time: Some(ByTimeDecision {
                    rule_name: "Work hours".to_owned(),
                    power_plan_guid: Some("schedule-custom".to_owned()),
                }),
                by_cpu_load: Some(ByCpuLoadDecision {
                    rule_name: "Low CPU".to_owned(),
                    power_plan_guid: Some("cpu-low-guid".to_owned()),
                    usage_percent: 10.0,
                }),
            },
        );

        assert_eq!(outcome.state, DecisionState::CpuLoadRule);
        assert_eq!(outcome.target_guid.as_deref(), Some("cpu-low-guid"));
    }

    #[test]
    fn activity_overrides_schedule_when_cpu_does_not_match() {
        let outcome = DecisionEngine.decide(
            &test_settings(),
            DecisionInput {
                activity_state: ActivityState::Idle,
                foreground_app: None,
                plugged_in: None,
                by_running_app: None,
                by_time: Some(ByTimeDecision {
                    rule_name: "Work hours".to_owned(),
                    power_plan_guid: Some("schedule-custom".to_owned()),
                }),
                by_cpu_load: None,
            },
        );

        assert_eq!(outcome.state, DecisionState::IdlePowerSave);
        assert_eq!(outcome.target_guid.as_deref(), Some("idle-guid"));
    }

    #[test]
    fn schedule_applies_when_activity_and_cpu_do_not_match() {
        let mut settings = test_settings();
        settings.by_activity.enabled = false;

        let outcome = DecisionEngine.decide(
            &settings,
            DecisionInput {
                activity_state: ActivityState::Active,
                foreground_app: None,
                plugged_in: None,
                by_running_app: None,
                by_time: Some(ByTimeDecision {
                    rule_name: "Work hours".to_owned(),
                    power_plan_guid: Some("schedule-custom".to_owned()),
                }),
                by_cpu_load: None,
            },
        );

        assert_eq!(outcome.state, DecisionState::ScheduledRule);
        assert_eq!(outcome.target_guid.as_deref(), Some("schedule-custom"));
    }

    #[test]
    fn activity_applies_without_foreground_or_schedule() {
        let outcome = DecisionEngine.decide(
            &test_settings(),
            DecisionInput {
                activity_state: ActivityState::Idle,
                foreground_app: None,
                plugged_in: None,
                by_running_app: None,
                by_time: None,
                by_cpu_load: None,
            },
        );

        assert_eq!(outcome.state, DecisionState::IdlePowerSave);
        assert_eq!(outcome.target_guid.as_deref(), Some("idle-guid"));
    }

    #[test]
    fn disabled_by_foreground_are_ignored() {
        let mut settings = test_settings();
        settings.by_foreground.enabled = false;

        let outcome = DecisionEngine.decide(
            &settings,
            DecisionInput {
                activity_state: ActivityState::Idle,
                foreground_app: Some("backup.exe".to_owned()),
                plugged_in: None,
                by_running_app: None,
                by_time: None,
                by_cpu_load: None,
            },
        );

        assert_eq!(outcome.state, DecisionState::IdlePowerSave);
        assert_eq!(outcome.target_guid.as_deref(), Some("idle-guid"));
    }

    #[test]
    fn plugged_in_pause_prevents_power_plan_target() {
        let mut settings = test_settings();
        settings.general.pause_power_plan_switching_while_plugged_in = true;

        let outcome = DecisionEngine.decide(
            &settings,
            DecisionInput {
                activity_state: ActivityState::Idle,
                foreground_app: None,
                plugged_in: Some(true),
                by_running_app: None,
                by_time: None,
                by_cpu_load: None,
            },
        );

        assert_eq!(outcome.state, DecisionState::PluggedInPause);
        assert_eq!(outcome.target_guid, None);
    }

    #[test]
    fn decisions_use_their_own_power_plan_mappings() {
        let mut settings = test_settings();
        settings.by_activity.power_plans = PowerPlanSettings {
            power_save_guid: Some("activity-idle".to_owned()),
            performance_guid: Some("activity-active".to_owned()),
        };
        settings.by_foreground.rules = vec![ByForegroundRule {
            enabled: true,
            name: "Game".to_owned(),
            process_name: "game.exe".to_owned(),
            power_plan_guid: Some("foreground-custom".to_owned()),
        }];

        let foreground = DecisionEngine.decide(
            &settings,
            DecisionInput {
                activity_state: ActivityState::Active,
                foreground_app: Some("game.exe".to_owned()),
                plugged_in: None,
                by_running_app: None,
                by_time: None,
                by_cpu_load: None,
            },
        );
        assert_eq!(foreground.target_guid.as_deref(), Some("foreground-custom"));

        settings.by_activity.enabled = false;
        let schedule = DecisionEngine.decide(
            &settings,
            DecisionInput {
                activity_state: ActivityState::Active,
                foreground_app: None,
                plugged_in: None,
                by_running_app: None,
                by_time: Some(ByTimeDecision {
                    rule_name: "Quiet".to_owned(),
                    power_plan_guid: Some("schedule-custom".to_owned()),
                }),
                by_cpu_load: None,
            },
        );
        assert_eq!(schedule.target_guid.as_deref(), Some("schedule-custom"));

        settings.by_activity.enabled = false;
        let cpu = DecisionEngine.decide(
            &settings,
            DecisionInput {
                activity_state: ActivityState::Idle,
                foreground_app: None,
                plugged_in: None,
                by_running_app: None,
                by_time: None,
                by_cpu_load: Some(ByCpuLoadDecision {
                    rule_name: "High CPU".to_owned(),
                    power_plan_guid: Some("cpu-custom".to_owned()),
                    usage_percent: 90.0,
                }),
            },
        );
        assert_eq!(cpu.target_guid.as_deref(), Some("cpu-custom"));

        settings.by_activity.enabled = true;
        let activity = DecisionEngine.decide(
            &settings,
            DecisionInput {
                activity_state: ActivityState::Idle,
                foreground_app: None,
                plugged_in: None,
                by_running_app: None,
                by_time: None,
                by_cpu_load: None,
            },
        );
        assert_eq!(activity.target_guid.as_deref(), Some("activity-idle"));
    }

    #[test]
    fn foreground_rule_can_target_any_power_plan() {
        let mut settings = test_settings();
        settings.by_foreground.rules = vec![ByForegroundRule {
            enabled: true,
            name: "Editing".to_owned(),
            process_name: "editor.exe".to_owned(),
            power_plan_guid: Some("balanced-guid".to_owned()),
        }];

        let outcome = DecisionEngine.decide(
            &settings,
            DecisionInput {
                activity_state: ActivityState::Idle,
                foreground_app: Some("editor.exe".to_owned()),
                plugged_in: None,
                by_running_app: None,
                by_time: None,
                by_cpu_load: None,
            },
        );

        assert_eq!(outcome.state, DecisionState::ByForegroundRule);
        assert_eq!(outcome.target_guid.as_deref(), Some("balanced-guid"));
    }

    #[test]
    fn foreground_rule_without_power_plan_combines_with_cpu_before_activity() {
        let mut settings = test_settings();
        settings.by_foreground.rules = vec![ByForegroundRule {
            enabled: true,
            name: "Editing".to_owned(),
            process_name: "editor.exe".to_owned(),
            power_plan_guid: None,
        }];

        let outcome = DecisionEngine.decide(
            &settings,
            DecisionInput {
                activity_state: ActivityState::Idle,
                foreground_app: Some("editor.exe".to_owned()),
                plugged_in: None,
                by_running_app: None,
                by_time: Some(ByTimeDecision {
                    rule_name: "Work hours".to_owned(),
                    power_plan_guid: Some("schedule-custom".to_owned()),
                }),
                by_cpu_load: Some(ByCpuLoadDecision {
                    rule_name: "High CPU".to_owned(),
                    power_plan_guid: Some("cpu-high-guid".to_owned()),
                    usage_percent: 90.0,
                }),
            },
        );

        assert_eq!(outcome.state, DecisionState::CpuLoadRule);
        assert_eq!(outcome.target_guid.as_deref(), Some("cpu-high-guid"));
    }

    #[test]
    fn foreground_rule_without_power_plan_combines_with_cpu_rule() {
        let mut settings = test_settings();
        settings.by_foreground.rules = vec![ByForegroundRule {
            enabled: true,
            name: "Rendering".to_owned(),
            process_name: "render.exe".to_owned(),
            power_plan_guid: None,
        }];

        let outcome = DecisionEngine.decide(
            &settings,
            DecisionInput {
                activity_state: ActivityState::Idle,
                foreground_app: Some("render.exe".to_owned()),
                plugged_in: None,
                by_running_app: None,
                by_time: None,
                by_cpu_load: Some(ByCpuLoadDecision {
                    rule_name: "High CPU".to_owned(),
                    power_plan_guid: Some("cpu-high-guid".to_owned()),
                    usage_percent: 90.0,
                }),
            },
        );

        assert_eq!(outcome.state, DecisionState::CpuLoadRule);
        assert_eq!(outcome.target_guid.as_deref(), Some("cpu-high-guid"));
    }

    #[test]
    fn by_running_app_overrides_cpu_activity_and_schedule() {
        let outcome = DecisionEngine.decide(
            &test_settings(),
            DecisionInput {
                activity_state: ActivityState::Idle,
                foreground_app: None,
                plugged_in: None,
                by_running_app: Some(ByRunningAppDecision {
                    rule_name: "Game".to_owned(),
                    process_name: "game.exe".to_owned(),
                    power_plan_guid: "by-running-app-guid".to_owned(),
                }),
                by_time: Some(ByTimeDecision {
                    rule_name: "Work hours".to_owned(),
                    power_plan_guid: Some("schedule-custom".to_owned()),
                }),
                by_cpu_load: Some(ByCpuLoadDecision {
                    rule_name: "High CPU".to_owned(),
                    power_plan_guid: Some("cpu-high-guid".to_owned()),
                    usage_percent: 90.0,
                }),
            },
        );

        assert_eq!(outcome.state, DecisionState::ByRunningApp);
        assert_eq!(outcome.target_guid.as_deref(), Some("by-running-app-guid"));
    }

    #[test]
    fn foreground_rule_overrides_by_running_app_when_both_match() {
        let mut settings = test_settings();
        settings.by_foreground.rules = vec![ByForegroundRule {
            enabled: true,
            name: "Game focus".to_owned(),
            process_name: "game.exe".to_owned(),
            power_plan_guid: Some("foreground-guid".to_owned()),
        }];

        let outcome = DecisionEngine.decide(
            &settings,
            DecisionInput {
                activity_state: ActivityState::Idle,
                foreground_app: Some("game.exe".to_owned()),
                plugged_in: None,
                by_running_app: Some(ByRunningAppDecision {
                    rule_name: "Game running".to_owned(),
                    process_name: "game.exe".to_owned(),
                    power_plan_guid: "by-running-app-guid".to_owned(),
                }),
                by_time: None,
                by_cpu_load: None,
            },
        );

        assert_eq!(outcome.state, DecisionState::ByForegroundRule);
        assert_eq!(outcome.target_guid.as_deref(), Some("foreground-guid"));
    }

    #[test]
    fn disabled_foreground_rule_is_ignored() {
        let mut settings = test_settings();
        settings.by_foreground.rules = vec![ByForegroundRule {
            enabled: false,
            name: "Editing".to_owned(),
            process_name: "editor.exe".to_owned(),
            power_plan_guid: Some("balanced-guid".to_owned()),
        }];

        let outcome = DecisionEngine.decide(
            &settings,
            DecisionInput {
                activity_state: ActivityState::Idle,
                foreground_app: Some("editor.exe".to_owned()),
                plugged_in: None,
                by_running_app: None,
                by_time: None,
                by_cpu_load: None,
            },
        );

        assert_eq!(outcome.state, DecisionState::IdlePowerSave);
        assert_eq!(outcome.target_guid.as_deref(), Some("idle-guid"));
    }
}
