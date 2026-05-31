use chrono::Utc;

use crate::{
    activity::ActivityState,
    config::{CpuUsageTarget, ManualOverride, Settings},
    scheduler::{CpuUsageDecision, ScheduleDecision},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionState {
    Disabled,
    ManualOverride,
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

        let foreground = input.foreground_app.as_deref().map(str::to_ascii_lowercase);

        if let Some(app) = foreground
            .as_deref()
            .filter(|_| settings.foreground_rules.enabled)
        {
            if contains_process(&settings.foreground_rules.force_power_save, app) {
                return DecisionOutcome::with_target(
                    settings.power_plans.power_save_guid.clone(),
                    DecisionState::ForegroundForcePowerSave,
                    format!("{app} is configured to force the Idle plan."),
                );
            }

            if contains_process(&settings.foreground_rules.whitelist, app) {
                return DecisionOutcome::with_target(
                    settings.power_plans.performance_guid.clone(),
                    DecisionState::ForegroundForceActive,
                    format!("{app} is configured to force the Active plan."),
                );
            }
        }

        if let Some(schedule) = input.schedule {
            if schedule.inside_power_save_period {
                return DecisionOutcome::with_target(
                    settings.power_plans.power_save_guid.clone(),
                    DecisionState::ScheduledPowerSave,
                    format!("Schedule '{}' is in its Idle period.", schedule.rule_name),
                );
            }

            return DecisionOutcome::with_target(
                settings.power_plans.performance_guid.clone(),
                DecisionState::ScheduledPerformance,
                "No schedule Idle period is active.",
            );
        }

        if let Some(cpu_usage) = input.cpu_usage {
            return match cpu_usage.target {
                CpuUsageTarget::Idle => DecisionOutcome::with_target(
                    settings.power_plans.power_save_guid.clone(),
                    DecisionState::CpuUsagePowerSave,
                    format!(
                        "CPU usage is {:.1}% and matched rule '{}'.",
                        cpu_usage.usage_percent, cpu_usage.rule_name
                    ),
                ),
                CpuUsageTarget::Active => DecisionOutcome::with_target(
                    settings.power_plans.performance_guid.clone(),
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
                    settings.power_plans.power_save_guid.clone(),
                    DecisionState::IdlePowerSave,
                    "User input has been idle past the configured timeout.",
                ),
                ActivityState::Active
                    if settings.activity_mode.switch_to_performance_on_resume
                        && settings.activity_mode.input_detection.any_enabled() =>
                {
                    DecisionOutcome::with_target(
                        settings.power_plans.performance_guid.clone(),
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
            settings.power_plans.performance_guid.clone(),
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

#[allow(dead_code)]
fn _manual_override_is_used(override_state: &ManualOverride) -> bool {
    !matches!(override_state, ManualOverride::None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        activity::ActivityState,
        config::{CpuUsageTarget, ForegroundRules, Settings},
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
            whitelist: vec!["game.exe".to_owned()],
            force_power_save: vec!["backup.exe".to_owned()],
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
                schedule: None,
                cpu_usage: None,
            },
        );

        assert_eq!(outcome.state, DecisionState::IdlePowerSave);
        assert_eq!(outcome.target_guid.as_deref(), Some("idle-guid"));
    }
}
