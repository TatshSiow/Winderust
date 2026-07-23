use std::time::Instant;

use crate::config::ByCpuLoadSettings;

#[derive(Debug, Clone)]
pub struct ByCpuLoadDecision {
    pub rule_name: String,
    pub power_plan_guid: Option<String>,
    pub usage_percent: f32,
}

#[derive(Debug, Default)]
pub struct ByCpuLoadScheduler {
    matched_since: Vec<Option<Instant>>,
}

impl ByCpuLoadScheduler {
    pub fn current_decision(
        &mut self,
        settings: &ByCpuLoadSettings,
        usage_percent: Option<f32>,
    ) -> Option<ByCpuLoadDecision> {
        if !settings.enabled || settings.rules.is_empty() {
            self.matched_since.clear();
            return None;
        }

        let Some(usage_percent) = usage_percent else {
            self.matched_since.clear();
            return None;
        };
        self.matched_since.resize(settings.rules.len(), None);
        let now = Instant::now();
        let mut else_decision = None;

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
            if matched_since.elapsed().as_secs() >= rule.duration_seconds {
                return Some(ByCpuLoadDecision {
                    rule_name: rule.name.clone(),
                    power_plan_guid: rule.power_plan_guid.clone(),
                    usage_percent,
                });
            }
        }

        else_decision.map(|(rule_name, power_plan_guid)| ByCpuLoadDecision {
            rule_name,
            power_plan_guid,
            usage_percent,
        })
    }
}

fn else_decision_for_rule(rule: &crate::config::ByCpuLoadRule) -> Option<(String, Option<String>)> {
    if rule.else_enabled {
        return Some((
            format!("{} else", rule.name),
            rule.else_power_plan_guid.clone(),
        ));
    }

    if rule.is_else() {
        return Some((rule.name.clone(), rule.power_plan_guid.clone()));
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
