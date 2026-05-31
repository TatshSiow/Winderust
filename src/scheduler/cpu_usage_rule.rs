use std::time::Instant;

use crate::config::{CpuUsageModeSettings, CpuUsageTarget};

#[derive(Debug, Clone)]
pub struct CpuUsageDecision {
    pub rule_name: String,
    pub target: CpuUsageTarget,
    pub usage_percent: f32,
}

#[derive(Debug, Default)]
pub struct CpuUsageScheduler {
    matched_since: Vec<Option<Instant>>,
}

impl CpuUsageScheduler {
    pub fn current_decision(
        &mut self,
        settings: &CpuUsageModeSettings,
        usage_percent: Option<f32>,
    ) -> Option<CpuUsageDecision> {
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

        for (index, rule) in settings.rules.iter().enumerate() {
            if !rule.matches_usage(usage_percent) {
                self.matched_since[index] = None;
                continue;
            }

            let matched_since = self.matched_since[index].get_or_insert(now);
            if matched_since.elapsed().as_secs() >= rule.duration_seconds {
                return Some(CpuUsageDecision {
                    rule_name: rule.name.clone(),
                    target: rule.target,
                    usage_percent,
                });
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{CpuUsageComparison, CpuUsageRule};

    #[test]
    fn returns_matching_zero_duration_rule() {
        let mut scheduler = CpuUsageScheduler::default();
        let settings = CpuUsageModeSettings {
            enabled: true,
            rules: vec![CpuUsageRule {
                name: "High CPU".to_owned(),
                comparison: CpuUsageComparison::AtOrAbove,
                threshold_percent: 75,
                duration_seconds: 0,
                target: CpuUsageTarget::Active,
            }],
        };

        let decision = scheduler.current_decision(&settings, Some(80.0)).unwrap();

        assert_eq!(decision.rule_name, "High CPU");
        assert_eq!(decision.target, CpuUsageTarget::Active);
    }

    #[test]
    fn ignores_non_matching_rule() {
        let mut scheduler = CpuUsageScheduler::default();
        let settings = CpuUsageModeSettings {
            enabled: true,
            rules: vec![CpuUsageRule {
                name: "Low CPU".to_owned(),
                comparison: CpuUsageComparison::AtOrBelow,
                threshold_percent: 20,
                duration_seconds: 0,
                target: CpuUsageTarget::Idle,
            }],
        };

        assert!(scheduler.current_decision(&settings, Some(45.0)).is_none());
    }
}
