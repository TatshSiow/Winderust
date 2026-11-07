use std::time::Instant;

use crate::config::{ByCpuLoadRule, ByCpuLoadSettings, CpuUsageComparison};

#[derive(Debug, Clone)]
pub struct ByCpuLoadDecision {
    pub rule_index: usize,
    pub rule_name: String,
    pub power_plan_guid: String,
    pub usage_percent: f32,
    pub is_else: bool,
}

#[derive(Debug)]
struct ActiveRule {
    index: usize,
    since: Instant,
}

#[derive(Debug, Default)]
pub struct ByCpuLoadScheduler {
    matched_since: Vec<Option<Instant>>,
    last_rules: Vec<ByCpuLoadRule>,
    active_rule: Option<ActiveRule>,
}

impl ByCpuLoadScheduler {
    pub(crate) fn reset(&mut self) {
        self.matched_since.clear();
        self.last_rules.clear();
        self.active_rule = None;
    }

    fn clear_active_rule(&mut self) {
        if let Some(active) = self.active_rule.take() {
            self.matched_since[active.index] = None;
        }
    }

    pub(crate) fn observe_active_plan(&mut self, active_guid: Option<&str>) {
        if self.active_rule.as_ref().is_some_and(|active| {
            !self.last_rules[active.index]
                .power_plan_guid
                .as_deref()
                .zip(active_guid)
                .is_some_and(|(target, current)| target.eq_ignore_ascii_case(current))
        }) {
            self.clear_active_rule();
        }
    }

    pub(crate) fn confirm_applied_rule(
        &mut self,
        primary_rule_index: Option<usize>,
        active_guid: Option<&str>,
        now: Instant,
    ) {
        let confirmed_index = primary_rule_index.filter(|&index| {
            self.last_rules.get(index).is_some_and(|rule| {
                rule.stability.is_some()
                    && rule
                        .power_plan_guid
                        .as_deref()
                        .zip(active_guid)
                        .is_some_and(|(target, current)| target.eq_ignore_ascii_case(current))
            })
        });
        if self.active_rule.as_ref().map(|active| active.index) != confirmed_index {
            self.clear_active_rule();
            self.active_rule = confirmed_index.map(|index| ActiveRule { index, since: now });
        }
    }

    pub fn current_decision(
        &mut self,
        settings: &ByCpuLoadSettings,
        usage_percent: Option<f32>,
    ) -> Option<ByCpuLoadDecision> {
        self.current_decision_at(settings, usage_percent, Instant::now())
    }

    pub(crate) fn current_decision_at(
        &mut self,
        settings: &ByCpuLoadSettings,
        usage_percent: Option<f32>,
        now: Instant,
    ) -> Option<ByCpuLoadDecision> {
        if !settings.enabled || settings.rules.is_empty() {
            self.reset();
            return None;
        }

        let Some(usage_percent) =
            usage_percent.filter(|usage| usage.is_finite() && (0.0..=100.0).contains(usage))
        else {
            self.reset();
            return None;
        };
        if self.last_rules != settings.rules {
            self.reset();
            self.last_rules.clone_from(&settings.rules);
        }
        self.matched_since.resize(settings.rules.len(), None);
        if self.active_rule.as_ref().is_some_and(|active| {
            let rule = &settings.rules[active.index];
            !rule.stability.as_ref().is_some_and(|stability| {
                now.saturating_duration_since(active.since).as_secs()
                    < stability.minimum_hold_seconds
                    || matches_exit_condition(rule, usage_percent)
            })
        }) {
            self.clear_active_rule();
        }
        let active_index = self.active_rule.as_ref().map(|active| active.index);
        let mut else_decision = None;
        let mut matching_decision = None;

        for (index, rule) in settings.rules.iter().enumerate() {
            if !rule.enabled {
                self.matched_since[index] = None;
                continue;
            }

            if else_decision.is_none() {
                else_decision = else_decision_for_rule(rule).map(|decision| (index, decision));
            }

            if rule.is_else() {
                self.matched_since[index] = None;
                continue;
            }

            let qualified = if rule.matches_usage(usage_percent) {
                let matched_since = self.matched_since[index].get_or_insert(now);
                now.saturating_duration_since(*matched_since).as_secs() >= rule.duration_seconds
            } else {
                self.matched_since[index] = None;
                false
            };

            if (qualified || active_index == Some(index)) && matching_decision.is_none() {
                if let Some(power_plan_guid) = rule.power_plan_guid.clone() {
                    matching_decision = Some(ByCpuLoadDecision {
                        rule_index: index,
                        rule_name: rule.name.clone(),
                        power_plan_guid,
                        usage_percent,
                        is_else: false,
                    });
                }
            }
        }

        matching_decision.or_else(|| {
            else_decision.map(
                |(rule_index, (rule_name, power_plan_guid))| ByCpuLoadDecision {
                    rule_index,
                    rule_name,
                    power_plan_guid,
                    usage_percent,
                    is_else: true,
                },
            )
        })
    }
}

fn matches_exit_condition(rule: &ByCpuLoadRule, usage_percent: f32) -> bool {
    let Some(stability) = rule.stability.as_ref() else {
        return false;
    };
    let lower = f32::from(stability.exit_threshold_percent.get());
    match rule.comparison {
        CpuUsageComparison::AtOrAbove => usage_percent >= lower,
        CpuUsageComparison::AtOrBelow => usage_percent <= lower,
        CpuUsageComparison::Between => stability
            .exit_upper_threshold_percent
            .is_some_and(|upper| (lower..=f32::from(upper.get())).contains(&usage_percent)),
        CpuUsageComparison::Else => false,
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
mod tests;
