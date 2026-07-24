use std::time::Instant;

use crate::config::{ByCpuLoadRule, ByCpuLoadSettings};

#[derive(Debug, Clone)]
pub struct ByCpuLoadDecision {
    pub rule_name: String,
    pub power_plan_guid: Option<String>,
    pub usage_percent: f32,
}

#[derive(Debug, Default)]
pub struct ByCpuLoadScheduler {
    matched_since: Vec<Option<Instant>>,
    last_rules: Vec<ByCpuLoadRule>,
}

impl ByCpuLoadScheduler {
    pub fn current_decision(
        &mut self,
        settings: &ByCpuLoadSettings,
        usage_percent: Option<f32>,
    ) -> Option<ByCpuLoadDecision> {
        if !settings.enabled || settings.rules.is_empty() {
            self.matched_since.clear();
            self.last_rules.clear();
            return None;
        }

        let Some(usage_percent) = usage_percent else {
            self.matched_since.clear();
            self.last_rules.clear();
            return None;
        };
        if self.last_rules != settings.rules {
            self.matched_since.clear();
            self.last_rules.clone_from(&settings.rules);
        }
        self.matched_since.resize(settings.rules.len(), None);
        let now = Instant::now();
        let mut else_decision = None;
        let mut matching_decision = None;

        for (index, rule) in settings.rules.iter().enumerate() {
            if !rule.enabled {
                self.matched_since[index] = None;
                continue;
            }

            if else_decision.is_none() {
                else_decision = else_decision_for_rule(rule);
            }

            if rule.is_else() {
                self.matched_since[index] = None;
                continue;
            }

            if !rule.matches_usage(usage_percent) {
                self.matched_since[index] = None;
                continue;
            }

            let matched_since = self.matched_since[index].get_or_insert(now);
            if matched_since.elapsed().as_secs() >= rule.duration_seconds
                && matching_decision.is_none()
            {
                if let Some(power_plan_guid) = rule.power_plan_guid.clone() {
                    matching_decision = Some(ByCpuLoadDecision {
                        rule_name: rule.name.clone(),
                        power_plan_guid: Some(power_plan_guid),
                        usage_percent,
                    });
                }
            }
        }

        matching_decision.or_else(|| {
            else_decision.map(|(rule_name, power_plan_guid)| ByCpuLoadDecision {
                rule_name,
                power_plan_guid: Some(power_plan_guid),
                usage_percent,
            })
        })
    }
}

fn else_decision_for_rule(rule: &crate::config::ByCpuLoadRule) -> Option<(String, String)> {
    if rule.else_enabled {
        return Some((
            format!("{} else", rule.name),
            rule.else_power_plan_guid.clone()?,
        ));
    }

    if rule.is_else() {
        return Some((rule.name.clone(), rule.power_plan_guid.clone()?));
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{ByCpuLoadRule, CpuUsageComparison};

    #[test]
    fn by_cpu_load_returns_matching_zero_duration_rule() {
        let mut scheduler = ByCpuLoadScheduler::default();
        let settings = ByCpuLoadSettings {
            enabled: true,
            rules: vec![ByCpuLoadRule {
                enabled: true,
                name: "High CPU".to_owned(),
                comparison: CpuUsageComparison::AtOrAbove,
                threshold_percent: 75,
                upper_threshold_percent: None,
                duration_seconds: 0,
                power_plan_guid: Some("high-cpu-guid".to_owned()),
                else_enabled: false,
                else_power_plan_guid: None,
            }],
        };

        let decision = scheduler.current_decision(&settings, Some(80.0)).unwrap();

        assert_eq!(decision.rule_name, "High CPU");
        assert_eq!(decision.power_plan_guid.as_deref(), Some("high-cpu-guid"));
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
                threshold_percent: 20,
                upper_threshold_percent: None,
                duration_seconds: 0,
                power_plan_guid: Some("low-cpu-guid".to_owned()),
                else_enabled: false,
                else_power_plan_guid: None,
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
                threshold_percent: 75,
                upper_threshold_percent: None,
                duration_seconds: 0,
                power_plan_guid: None,
                else_enabled: false,
                else_power_plan_guid: None,
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
                threshold_percent: 30,
                upper_threshold_percent: Some(60),
                duration_seconds: 0,
                power_plan_guid: Some("medium-cpu-guid".to_owned()),
                else_enabled: false,
                else_power_plan_guid: None,
            }],
        };

        let decision = scheduler.current_decision(&settings, Some(45.0)).unwrap();

        assert_eq!(decision.rule_name, "Medium CPU");
        assert_eq!(decision.power_plan_guid.as_deref(), Some("medium-cpu-guid"));
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
                threshold_percent: 75,
                upper_threshold_percent: None,
                duration_seconds: 30,
                power_plan_guid: Some("high-cpu-guid".to_owned()),
                else_enabled: true,
                else_power_plan_guid: Some("else-guid".to_owned()),
            }],
        };

        let waiting_decision = scheduler.current_decision(&settings, Some(80.0)).unwrap();
        assert_eq!(
            waiting_decision.power_plan_guid.as_deref(),
            Some("else-guid")
        );

        let decision = scheduler.current_decision(&settings, Some(40.0)).unwrap();

        assert_eq!(decision.rule_name, "High CPU else");
        assert_eq!(decision.power_plan_guid.as_deref(), Some("else-guid"));
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
                threshold_percent: 75,
                upper_threshold_percent: None,
                duration_seconds: 30,
                power_plan_guid: Some("high-cpu-guid".to_owned()),
                else_enabled: false,
                else_power_plan_guid: None,
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
                    threshold_percent: 75,
                    upper_threshold_percent: None,
                    duration_seconds: 0,
                    power_plan_guid: Some("high-cpu-guid".to_owned()),
                    else_enabled: false,
                    else_power_plan_guid: None,
                },
                ByCpuLoadRule {
                    enabled: true,
                    name: "Low CPU".to_owned(),
                    comparison: CpuUsageComparison::AtOrBelow,
                    threshold_percent: 20,
                    upper_threshold_percent: None,
                    duration_seconds: 30,
                    power_plan_guid: Some("low-cpu-guid".to_owned()),
                    else_enabled: false,
                    else_power_plan_guid: None,
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
                threshold_percent: 75,
                upper_threshold_percent: None,
                duration_seconds: 0,
                power_plan_guid: Some("high-cpu-guid".to_owned()),
                else_enabled: true,
                else_power_plan_guid: Some("else-guid".to_owned()),
            }],
        };

        assert!(scheduler.current_decision(&settings, Some(90.0)).is_none());
        assert!(scheduler.current_decision(&settings, Some(10.0)).is_none());
    }
}
