use chrono::Utc;

use crate::{
    activity::ActivityState,
    config::{ManualOverride, PowerPlanSettings, Settings},
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
    PerformanceMode,
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
    pub performance_mode: Option<PerformanceModeDecision>,
    pub schedule: Option<ScheduleDecision>,
    pub cpu_usage: Option<CpuUsageDecision>,
}

#[derive(Debug, Clone)]
pub struct PerformanceModeDecision {
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
                if rule.enabled && rule.process_name.trim().eq_ignore_ascii_case(app.trim()) {
                    if let Some(power_plan_guid) = rule.power_plan_guid.clone() {
                        return DecisionOutcome::with_target(
                            Some(power_plan_guid),
                            DecisionState::ForegroundRule,
                            format!("{app} matched foreground rule '{}'.", rule.name),
                        );
                    }
                    break;
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

        if let Some(performance_mode) = input.performance_mode {
            return DecisionOutcome::with_target(
                Some(performance_mode.power_plan_guid),
                DecisionState::PerformanceMode,
                format!(
                    "{} is running and matched Running App Power Plan rule '{}'.",
                    performance_mode.process_name, performance_mode.rule_name
                ),
            );
        }

        if let Some(schedule) = input.schedule {
            return DecisionOutcome::with_target(
                schedule.power_plan_guid,
                DecisionState::ScheduledRule,
                format!("Time rule '{}' is active.", schedule.rule_name),
            );
        }

        if let Some(cpu_usage) = input.cpu_usage {
            return DecisionOutcome::with_target(
                cpu_usage.power_plan_guid,
                DecisionState::CpuLoadRule,
                format!(
                    "CPU load is {:.1}% and matched rule '{}'.",
                    cpu_usage.usage_percent, cpu_usage.rule_name
                ),
            );
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
            Self::PerformanceMode => "Running App Power Plans",
            Self::ScheduledRule => "Time rule",
            Self::CpuLoadRule => "CPU load rule",
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
        config::{ForegroundRule, ForegroundRules, PowerPlanSettings, Settings},
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
                performance_mode: None,
                schedule: Some(ScheduleDecision {
                    rule_name: "Work hours".to_owned(),
                    power_plan_guid: Some("schedule-custom".to_owned()),
                }),
                cpu_usage: None,
            },
        );

        assert_eq!(outcome.state, DecisionState::ForegroundForcePowerSave);
        assert_eq!(outcome.target_guid.as_deref(), Some("idle-guid"));
    }

    #[test]
    fn schedule_rule_overrides_cpu_and_activity() {
        let outcome = DecisionEngine.decide(
            &test_settings(),
            DecisionInput {
                activity_state: ActivityState::Idle,
                foreground_app: None,
                plugged_in: None,
                performance_mode: None,
                schedule: Some(ScheduleDecision {
                    rule_name: "Work hours".to_owned(),
                    power_plan_guid: Some("schedule-custom".to_owned()),
                }),
                cpu_usage: Some(CpuUsageDecision {
                    rule_name: "Low CPU".to_owned(),
                    power_plan_guid: Some("cpu-low-guid".to_owned()),
                    usage_percent: 10.0,
                }),
            },
        );

        assert_eq!(outcome.state, DecisionState::ScheduledRule);
        assert_eq!(outcome.target_guid.as_deref(), Some("schedule-custom"));
    }

    #[test]
    fn cpu_usage_overrides_activity() {
        let outcome = DecisionEngine.decide(
            &test_settings(),
            DecisionInput {
                activity_state: ActivityState::Idle,
                foreground_app: None,
                plugged_in: None,
                performance_mode: None,
                schedule: None,
                cpu_usage: Some(CpuUsageDecision {
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
    fn activity_applies_without_foreground_or_schedule() {
        let outcome = DecisionEngine.decide(
            &test_settings(),
            DecisionInput {
                activity_state: ActivityState::Idle,
                foreground_app: None,
                plugged_in: None,
                performance_mode: None,
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
                performance_mode: None,
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
                performance_mode: None,
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
        settings.foreground_rules.power_plans = PowerPlanSettings {
            power_save_guid: Some("foreground-idle".to_owned()),
            performance_guid: Some("foreground-active".to_owned()),
        };
        settings.foreground_rules.rules = vec![ForegroundRule {
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
                performance_mode: None,
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
                performance_mode: None,
                schedule: Some(ScheduleDecision {
                    rule_name: "Quiet".to_owned(),
                    power_plan_guid: Some("schedule-custom".to_owned()),
                }),
                cpu_usage: None,
            },
        );
        assert_eq!(schedule.target_guid.as_deref(), Some("schedule-custom"));

        let cpu = DecisionEngine.decide(
            &settings,
            DecisionInput {
                activity_state: ActivityState::Idle,
                foreground_app: None,
                plugged_in: None,
                performance_mode: None,
                schedule: None,
                cpu_usage: Some(CpuUsageDecision {
                    rule_name: "High CPU".to_owned(),
                    power_plan_guid: Some("cpu-custom".to_owned()),
                    usage_percent: 90.0,
                }),
            },
        );
        assert_eq!(cpu.target_guid.as_deref(), Some("cpu-custom"));

        let activity = DecisionEngine.decide(
            &settings,
            DecisionInput {
                activity_state: ActivityState::Idle,
                foreground_app: None,
                plugged_in: None,
                performance_mode: None,
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
                performance_mode: None,
                schedule: None,
                cpu_usage: None,
            },
        );

        assert_eq!(outcome.state, DecisionState::ForegroundRule);
        assert_eq!(outcome.target_guid.as_deref(), Some("balanced-guid"));
    }

    #[test]
    fn foreground_rule_without_power_plan_combines_with_schedule_rule() {
        let mut settings = test_settings();
        settings.foreground_rules.rules = vec![ForegroundRule {
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
                performance_mode: None,
                schedule: Some(ScheduleDecision {
                    rule_name: "Work hours".to_owned(),
                    power_plan_guid: Some("schedule-custom".to_owned()),
                }),
                cpu_usage: Some(CpuUsageDecision {
                    rule_name: "High CPU".to_owned(),
                    power_plan_guid: Some("cpu-high-guid".to_owned()),
                    usage_percent: 90.0,
                }),
            },
        );

        assert_eq!(outcome.state, DecisionState::ScheduledRule);
        assert_eq!(outcome.target_guid.as_deref(), Some("schedule-custom"));
    }

    #[test]
    fn foreground_rule_without_power_plan_combines_with_cpu_rule() {
        let mut settings = test_settings();
        settings.foreground_rules.rules = vec![ForegroundRule {
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
                performance_mode: None,
                schedule: None,
                cpu_usage: Some(CpuUsageDecision {
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
    fn performance_mode_overrides_schedule_cpu_and_activity() {
        let outcome = DecisionEngine.decide(
            &test_settings(),
            DecisionInput {
                activity_state: ActivityState::Idle,
                foreground_app: None,
                plugged_in: None,
                performance_mode: Some(PerformanceModeDecision {
                    rule_name: "Game".to_owned(),
                    process_name: "game.exe".to_owned(),
                    power_plan_guid: "performance-mode-guid".to_owned(),
                }),
                schedule: Some(ScheduleDecision {
                    rule_name: "Work hours".to_owned(),
                    power_plan_guid: Some("schedule-custom".to_owned()),
                }),
                cpu_usage: Some(CpuUsageDecision {
                    rule_name: "High CPU".to_owned(),
                    power_plan_guid: Some("cpu-high-guid".to_owned()),
                    usage_percent: 90.0,
                }),
            },
        );

        assert_eq!(outcome.state, DecisionState::PerformanceMode);
        assert_eq!(
            outcome.target_guid.as_deref(),
            Some("performance-mode-guid")
        );
    }

    #[test]
    fn foreground_rule_overrides_performance_mode_when_both_match() {
        let mut settings = test_settings();
        settings.foreground_rules.rules = vec![ForegroundRule {
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
                performance_mode: Some(PerformanceModeDecision {
                    rule_name: "Game running".to_owned(),
                    process_name: "game.exe".to_owned(),
                    power_plan_guid: "performance-mode-guid".to_owned(),
                }),
                schedule: None,
                cpu_usage: None,
            },
        );

        assert_eq!(outcome.state, DecisionState::ForegroundRule);
        assert_eq!(outcome.target_guid.as_deref(), Some("foreground-guid"));
    }

    #[test]
    fn disabled_foreground_rule_is_ignored() {
        let mut settings = test_settings();
        settings.foreground_rules.rules = vec![ForegroundRule {
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
                performance_mode: None,
                schedule: None,
                cpu_usage: None,
            },
        );

        assert_eq!(outcome.state, DecisionState::IdlePowerSave);
        assert_eq!(outcome.target_guid.as_deref(), Some("idle-guid"));
    }
}
