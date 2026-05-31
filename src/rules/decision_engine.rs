use chrono::Utc;

use crate::{
    activity::ActivityState,
    config::{CpuUsageTarget, ManualOverride, PowerPlanSettings, Settings},
    scheduler::{CpuUsageDecision, ScheduleDecision},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionState {
    Disabled,
    ManualOverride,
    PluggedInPause,
    ForegroundRule,
    ForegroundForceActive,
    ForegroundForcePowerSave,
    ScheduledPowerSave,
    ScheduledPerformance,
    CpuUsagePowerSave,
    CpuUsagePerformance,
    IdlePowerSave,
    ActivePerformance,
    NoTargetPlan,
}

#[derive(Debug, Clone)]
pub struct DecisionInput {
    pub activity_state: ActivityState,
    pub foreground_app: Option<String>,
    pub plugged_in: Option<bool>,
    pub schedule: Option<ScheduleDecision>,
    pub cpu_usage: Option<CpuUsageDecision>,
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

        let now = Utc::now().timestamp();
        if settings.general.manual_override.is_active(now) {
            return DecisionOutcome::without_target(
                DecisionState::ManualOverride,
                "Manual override is active.",
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
            .filter(|_| settings.foreground_rules.enabled)
        {
            for rule in &settings.foreground_rules.rules {
                if rule.process_name.trim().eq_ignore_ascii_case(app.trim()) {
                    return DecisionOutcome::with_target(
                        rule.power_plan_guid.clone(),
                        DecisionState::ForegroundRule,
                        format!("{app} matched foreground rule '{}'.", rule.name),
                    );
                }
            }

            if contains_process(&settings.foreground_rules.force_power_save, app) {
                return DecisionOutcome::with_target(
                    idle_plan(&settings.foreground_rules.power_plans, settings),
                    DecisionState::ForegroundForcePowerSave,
                    format!("{app} is configured to force the Idle plan."),
                );
            }

            if contains_process(&settings.foreground_rules.whitelist, app) {
                return DecisionOutcome::with_target(
                    active_plan(&settings.foreground_rules.power_plans, settings),
                    DecisionState::ForegroundForceActive,
                    format!("{app} is configured to force the Active plan."),
                );
            }
        }

        if let Some(schedule) = input.schedule {
            if schedule.inside_power_save_period {
                return DecisionOutcome::with_target(
                    idle_plan(&settings.schedule_mode.power_plans, settings),
                    DecisionState::ScheduledPowerSave,
                    format!("Schedule '{}' is in its Idle period.", schedule.rule_name),
                );
            }

            return DecisionOutcome::with_target(
                active_plan(&settings.schedule_mode.power_plans, settings),
                DecisionState::ScheduledPerformance,
                "No schedule Idle period is active.",
            );
        }

        if let Some(cpu_usage) = input.cpu_usage {
            return match cpu_usage.target {
                CpuUsageTarget::Idle => DecisionOutcome::with_target(
                    idle_plan(&settings.cpu_usage_mode.power_plans, settings),
                    DecisionState::CpuUsagePowerSave,
                    format!(
                        "CPU usage is {:.1}% and matched rule '{}'.",
                        cpu_usage.usage_percent, cpu_usage.rule_name
                    ),
                ),
                CpuUsageTarget::Active => DecisionOutcome::with_target(
                    active_plan(&settings.cpu_usage_mode.power_plans, settings),
                    DecisionState::CpuUsagePerformance,
                    format!(
                        "CPU usage is {:.1}% and matched rule '{}'.",
                        cpu_usage.usage_percent, cpu_usage.rule_name
                    ),
                ),
            };
        }

        if settings.activity_mode.enabled {
            return match input.activity_state {
                ActivityState::Idle => DecisionOutcome::with_target(
                    idle_plan(&settings.activity_mode.power_plans, settings),
                    DecisionState::IdlePowerSave,
                    "User input has been idle past the configured timeout.",
                ),
                ActivityState::Active
                    if settings.activity_mode.switch_to_performance_on_resume
                        && settings.activity_mode.input_detection.any_enabled() =>
                {
                    DecisionOutcome::with_target(
                        active_plan(&settings.activity_mode.power_plans, settings),
                        DecisionState::ActivePerformance,
                        "Recent user input detected; using the Active plan.",
                    )
                }
                ActivityState::Active if settings.activity_mode.switch_to_performance_on_resume => {
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

        DecisionOutcome::with_target(
            active_plan(&settings.activity_mode.power_plans, settings),
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

impl DecisionState {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Disabled => "Disabled",
            Self::ManualOverride => "Manual override",
            Self::PluggedInPause => "Plugged in",
            Self::ForegroundRule => "Foreground rule",
            Self::ForegroundForceActive => "Foreground force Active plan",
            Self::ForegroundForcePowerSave => "Foreground force Idle plan",
            Self::ScheduledPowerSave => "Scheduled Idle plan",
            Self::ScheduledPerformance => "Scheduled Active plan",
            Self::CpuUsagePowerSave => "CPU usage Idle plan",
            Self::CpuUsagePerformance => "CPU usage Active plan",
            Self::IdlePowerSave => "Idle plan",
            Self::ActivePerformance => "Active plan",
            Self::NoTargetPlan => "Needs setup",
        }
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

#[allow(dead_code)]
fn _manual_override_is_used(override_state: &ManualOverride) -> bool {
    !matches!(override_state, ManualOverride::None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        activity::ActivityState,
        config::{CpuUsageTarget, ForegroundRule, ForegroundRules, PowerPlanSettings, Settings},
        scheduler::{CpuUsageDecision, ScheduleDecision},
    };

    fn test_settings() -> Settings {
        let mut settings = Settings::default();
        settings.power_plans.power_save_guid = Some("idle-guid".to_owned());
        settings.power_plans.performance_guid = Some("active-guid".to_owned());
        settings.schedule_mode.enabled = false;
        settings.schedule_mode.rules.clear();
        settings.foreground_rules = ForegroundRules {
            enabled: true,
            rules: Vec::new(),
            whitelist: vec!["game.exe".to_owned()],
            force_power_save: vec!["backup.exe".to_owned()],
            power_plans: PowerPlanSettings::default(),
        };
        settings
    }

    #[test]
    fn foreground_force_idle_overrides_schedule_and_activity() {
        let outcome = DecisionEngine.decide(
            &test_settings(),
            DecisionInput {
                activity_state: ActivityState::Active,
                foreground_app: Some("backup.exe".to_owned()),
                plugged_in: None,
                schedule: Some(ScheduleDecision {
                    rule_name: "Work hours".to_owned(),
                    inside_power_save_period: false,
                }),
                cpu_usage: None,
            },
        );

        assert_eq!(outcome.state, DecisionState::ForegroundForcePowerSave);
        assert_eq!(outcome.target_guid.as_deref(), Some("idle-guid"));
    }

    #[test]
    fn schedule_overrides_activity() {
        let outcome = DecisionEngine.decide(
            &test_settings(),
            DecisionInput {
                activity_state: ActivityState::Idle,
                foreground_app: None,
                plugged_in: None,
                schedule: Some(ScheduleDecision {
                    rule_name: "Outside schedule".to_owned(),
                    inside_power_save_period: false,
                }),
                cpu_usage: Some(CpuUsageDecision {
                    rule_name: "Low CPU".to_owned(),
                    target: CpuUsageTarget::Idle,
                    usage_percent: 10.0,
                }),
            },
        );

        assert_eq!(outcome.state, DecisionState::ScheduledPerformance);
        assert_eq!(outcome.target_guid.as_deref(), Some("active-guid"));
    }

    #[test]
    fn cpu_usage_overrides_activity() {
        let outcome = DecisionEngine.decide(
            &test_settings(),
            DecisionInput {
                activity_state: ActivityState::Idle,
                foreground_app: None,
                plugged_in: None,
                schedule: None,
                cpu_usage: Some(CpuUsageDecision {
                    rule_name: "High CPU".to_owned(),
                    target: CpuUsageTarget::Active,
                    usage_percent: 90.0,
                }),
            },
        );

        assert_eq!(outcome.state, DecisionState::CpuUsagePerformance);
        assert_eq!(outcome.target_guid.as_deref(), Some("active-guid"));
    }

    #[test]
    fn activity_applies_without_foreground_or_schedule() {
        let outcome = DecisionEngine.decide(
            &test_settings(),
            DecisionInput {
                activity_state: ActivityState::Idle,
                foreground_app: None,
                plugged_in: None,
                schedule: None,
                cpu_usage: None,
            },
        );

        assert_eq!(outcome.state, DecisionState::IdlePowerSave);
        assert_eq!(outcome.target_guid.as_deref(), Some("idle-guid"));
    }

    #[test]
    fn disabled_foreground_rules_are_ignored() {
        let mut settings = test_settings();
        settings.foreground_rules.enabled = false;

        let outcome = DecisionEngine.decide(
            &settings,
            DecisionInput {
                activity_state: ActivityState::Idle,
                foreground_app: Some("backup.exe".to_owned()),
                plugged_in: None,
                schedule: None,
                cpu_usage: None,
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
                schedule: None,
                cpu_usage: None,
            },
        );

        assert_eq!(outcome.state, DecisionState::PluggedInPause);
        assert_eq!(outcome.target_guid, None);
    }

    #[test]
    fn decisions_use_their_own_power_plan_mappings() {
        let mut settings = test_settings();
        settings.activity_mode.power_plans = PowerPlanSettings {
            power_save_guid: Some("activity-idle".to_owned()),
            performance_guid: Some("activity-active".to_owned()),
        };
        settings.schedule_mode.power_plans = PowerPlanSettings {
            power_save_guid: Some("schedule-idle".to_owned()),
            performance_guid: Some("schedule-active".to_owned()),
        };
        settings.cpu_usage_mode.power_plans = PowerPlanSettings {
            power_save_guid: Some("cpu-idle".to_owned()),
            performance_guid: Some("cpu-active".to_owned()),
        };
        settings.foreground_rules.power_plans = PowerPlanSettings {
            power_save_guid: Some("foreground-idle".to_owned()),
            performance_guid: Some("foreground-active".to_owned()),
        };
        settings.foreground_rules.rules = vec![ForegroundRule {
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
                schedule: None,
                cpu_usage: None,
            },
        );
        assert_eq!(foreground.target_guid.as_deref(), Some("foreground-custom"));

        let schedule = DecisionEngine.decide(
            &settings,
            DecisionInput {
                activity_state: ActivityState::Active,
                foreground_app: None,
                plugged_in: None,
                schedule: Some(ScheduleDecision {
                    rule_name: "Quiet".to_owned(),
                    inside_power_save_period: true,
                }),
                cpu_usage: None,
            },
        );
        assert_eq!(schedule.target_guid.as_deref(), Some("schedule-idle"));

        let cpu = DecisionEngine.decide(
            &settings,
            DecisionInput {
                activity_state: ActivityState::Idle,
                foreground_app: None,
                plugged_in: None,
                schedule: None,
                cpu_usage: Some(CpuUsageDecision {
                    rule_name: "High CPU".to_owned(),
                    target: CpuUsageTarget::Active,
                    usage_percent: 90.0,
                }),
            },
        );
        assert_eq!(cpu.target_guid.as_deref(), Some("cpu-active"));

        let activity = DecisionEngine.decide(
            &settings,
            DecisionInput {
                activity_state: ActivityState::Idle,
                foreground_app: None,
                plugged_in: None,
                schedule: None,
                cpu_usage: None,
            },
        );
        assert_eq!(activity.target_guid.as_deref(), Some("activity-idle"));
    }

    #[test]
    fn foreground_rule_can_target_any_power_plan() {
        let mut settings = test_settings();
        settings.foreground_rules.rules = vec![ForegroundRule {
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
                schedule: None,
                cpu_usage: None,
            },
        );

        assert_eq!(outcome.state, DecisionState::ForegroundRule);
        assert_eq!(outcome.target_guid.as_deref(), Some("balanced-guid"));
    }
}
