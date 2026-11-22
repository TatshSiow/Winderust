use std::time::{Duration, Instant};

use crate::{config::InstanceLimitsSettings, control::instance_limits::CountObservation};

pub(crate) const MAX_SAMPLE_AGE: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct InstanceLimitsSnapshot {
    pub(crate) enabled: bool,
    pub(crate) rules: Vec<InstanceLimitsRuleSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct InstanceLimitsRuleSnapshot {
    pub(crate) executable_path: String,
    pub(crate) max_instances: u32,
    pub(crate) enabled: bool,
    pub(crate) count: CountObservation,
    pub(crate) sampled_at: Option<Instant>,
    pub(crate) incident: bool,
    pub(crate) notification: NotificationStatus,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) enum NotificationStatus {
    #[default]
    NotAttempted,
    Pending,
    Accepted,
    Failed(String),
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum InstanceLimitsEvent {
    Excess(usize),
    Recovered(usize),
    Notify(usize),
    Error(usize, String),
}

struct RuleState {
    count: CountObservation,
    sampled_at: Option<Instant>,
    excess_since: Option<Instant>,
    recovery_since: Option<Instant>,
    incident: bool,
    notification: NotificationStatus,
    last_error: Option<String>,
}

impl Default for RuleState {
    fn default() -> Self {
        Self {
            count: CountObservation::Unavailable {
                reason: "Waiting for a fresh process count.".into(),
            },
            sampled_at: None,
            excess_since: None,
            recovery_since: None,
            incident: false,
            notification: NotificationStatus::NotAttempted,
            last_error: None,
        }
    }
}

#[derive(Default)]
pub(crate) struct InstanceLimitsManager {
    enabled: bool,
    settings: InstanceLimitsSettings,
    rules: Vec<RuleState>,
}

impl InstanceLimitsManager {
    pub(crate) fn sync(&mut self, settings: &InstanceLimitsSettings, master_enabled: bool) {
        let enabled = master_enabled && settings.enabled;
        if enabled == self.enabled && settings == &self.settings {
            return;
        }
        if settings.notify && !self.settings.notify {
            self.requalify_notifications();
        }
        let mut previous: Vec<_> = self
            .settings
            .rules
            .iter()
            .zip(std::mem::take(&mut self.rules))
            .collect();
        self.rules = settings
            .rules
            .iter()
            .map(|rule| {
                if enabled && self.enabled {
                    if let Some(index) = previous
                        .iter()
                        .position(|(previous_rule, _)| *previous_rule == rule)
                    {
                        return previous.swap_remove(index).1;
                    }
                }
                RuleState::default()
            })
            .collect();
        self.enabled = enabled;
        self.settings = settings.clone();
    }

    pub(crate) fn requalify_notifications(&mut self) {
        for state in &mut self.rules {
            if state.notification == NotificationStatus::NotAttempted {
                state.excess_since = None;
            }
        }
    }

    pub(crate) fn invalidate(&mut self, reason: &str) {
        for state in &mut self.rules {
            state.count = CountObservation::Unavailable {
                reason: reason.into(),
            };
            state.sampled_at = None;
            state.excess_since = None;
            state.recovery_since = None;
        }
    }

    pub(crate) fn reset_paths(&mut self, paths: &[String]) {
        for (rule, state) in self.settings.rules.iter().zip(&mut self.rules) {
            if paths.contains(&rule.executable_path) {
                *state = RuleState::default();
            }
        }
    }

    pub(crate) fn observe(
        &mut self,
        counts: &[CountObservation],
        now: Instant,
    ) -> Vec<InstanceLimitsEvent> {
        if !self.enabled {
            return Vec::new();
        }
        let mut events = Vec::new();
        let matching_rule_count = counts.len() == self.settings.rules.len();
        for (index, (rule, state)) in self.settings.rules.iter().zip(&mut self.rules).enumerate() {
            if !rule.enabled {
                continue;
            }
            if state
                .sampled_at
                .is_some_and(|previous| now.saturating_duration_since(previous) > MAX_SAMPLE_AGE)
            {
                state.excess_since = None;
                state.recovery_since = None;
            }
            let fresh = state.sampled_at.is_none_or(|previous| now > previous);
            let count = if rule.max_instances == 0 {
                CountObservation::Unavailable {
                    reason: "Instance Limits received an invalid zero maximum.".into(),
                }
            } else if !fresh {
                CountObservation::Unavailable {
                    reason: "Instance Limits received a duplicate or out-of-order sample.".into(),
                }
            } else if matching_rule_count {
                counts[index].clone()
            } else {
                CountObservation::Unavailable {
                    reason: "Instance Limits received a mismatched rule count.".into(),
                }
            };
            let error = match &count {
                CountObservation::Complete(_) => None,
                CountObservation::Incomplete { reason, .. }
                | CountObservation::Unavailable { reason } => Some(reason.clone()),
                CountObservation::Conflict => Some(
                    "Equivalent executable paths have conflicting Instance Limits rules.".into(),
                ),
            };
            if error != state.last_error {
                if let Some(error) = &error {
                    events.push(InstanceLimitsEvent::Error(index, error.clone()));
                }
                state.last_error = error;
            }
            let excess = match count {
                CountObservation::Complete(n) => n > rule.max_instances,
                CountObservation::Incomplete {
                    known_lower_bound, ..
                } => known_lower_bound > rule.max_instances,
                CountObservation::Unavailable { .. } | CountObservation::Conflict => false,
            };
            let complete_within_limit =
                matches!(count, CountObservation::Complete(n) if n <= rule.max_instances);
            state.count = count;
            if fresh {
                state.sampled_at = Some(now);
            }
            if excess {
                state.recovery_since = None;
                if qualifying_sample(&mut state.excess_since, now) {
                    if !state.incident {
                        state.incident = true;
                        events.push(InstanceLimitsEvent::Excess(index));
                    }
                    if self.settings.notify
                        && state.notification == NotificationStatus::NotAttempted
                    {
                        state.notification = NotificationStatus::Pending;
                        events.push(InstanceLimitsEvent::Notify(index));
                    }
                }
            } else {
                state.excess_since = None;
                if complete_within_limit && state.incident {
                    if qualifying_sample(&mut state.recovery_since, now) {
                        state.incident = false;
                        state.notification = NotificationStatus::NotAttempted;
                        state.recovery_since = None;
                        events.push(InstanceLimitsEvent::Recovered(index));
                    }
                } else {
                    state.recovery_since = None;
                }
            }
        }
        events
    }

    pub(crate) fn notification_result(
        &mut self,
        index: usize,
        result: Result<(), String>,
    ) -> Result<(), String> {
        let state = self
            .rules
            .get_mut(index)
            .ok_or("Instance Limits notification refers to a missing rule.")?;
        if state.notification != NotificationStatus::Pending {
            return Err("Instance Limits notification has no pending attempt.".into());
        }
        state.notification = match result {
            Ok(()) => NotificationStatus::Accepted,
            Err(error) => NotificationStatus::Failed(error),
        };
        Ok(())
    }

    pub(crate) fn snapshot(&self, _now: Instant) -> InstanceLimitsSnapshot {
        InstanceLimitsSnapshot {
            enabled: self.enabled,
            rules: self
                .settings
                .rules
                .iter()
                .zip(&self.rules)
                .map(|(rule, state)| InstanceLimitsRuleSnapshot {
                    executable_path: rule.executable_path.clone(),
                    max_instances: rule.max_instances,
                    enabled: rule.enabled,
                    count: state.count.clone(),
                    sampled_at: state.sampled_at,
                    incident: state.incident,
                    notification: state.notification.clone(),
                })
                .collect(),
        }
    }
}

fn qualifying_sample(since: &mut Option<Instant>, now: Instant) -> bool {
    match *since {
        Some(since) => now.saturating_duration_since(since) >= Duration::from_secs(1),
        None => {
            *since = Some(now);
            false
        }
    }
}

#[cfg(test)]
mod tests;
