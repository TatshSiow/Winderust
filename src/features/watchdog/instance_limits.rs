use std::time::{Duration, Instant};

use crate::{config::InstanceLimitsSettings, control::instance_limits::CountObservation};

pub(crate) const MAX_SAMPLE_AGE: Duration = Duration::from_secs(3);

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum InstanceLimitsEvent {
    Excess(usize),
    Recovered(usize),
    Notify(usize),
    Error(usize, String),
}

#[derive(Default)]
struct RuleState {
    sampled_at: Option<Instant>,
    excess_since: Option<Instant>,
    notification_since: Option<Instant>,
    recovery_since: Option<Instant>,
    incident: bool,
    notification_attempted: bool,
    last_error: Option<String>,
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
                        .position(|(previous_rule, _)| previous_rule.same_monitor(rule))
                    {
                        let (previous_rule, mut state) = previous.swap_remove(index);
                        if rule.notify && !previous_rule.notify {
                            state.notification_since = None;
                        }
                        return state;
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
            if !state.notification_attempted {
                state.excess_since = None;
                state.notification_since = None;
            }
        }
    }

    pub(crate) fn requalify_rule_notifications(&mut self, paths: &[String]) {
        for (rule, state) in self.settings.rules.iter().zip(&mut self.rules) {
            if paths.contains(&rule.executable_path) {
                state.notification_since = None;
            }
        }
    }

    pub(crate) fn invalidate(&mut self) {
        for state in &mut self.rules {
            state.sampled_at = None;
            state.excess_since = None;
            state.notification_since = None;
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
                state.notification_since = None;
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
            if fresh {
                state.sampled_at = Some(now);
            }
            if excess {
                state.recovery_since = None;
                if !state.incident && qualifying_sample(&mut state.excess_since, now) {
                    state.incident = true;
                    events.push(InstanceLimitsEvent::Excess(index));
                }
                if self.settings.notify
                    && rule.notify
                    && !state.notification_attempted
                    && qualifying_sample(&mut state.notification_since, now)
                    && state.incident
                {
                    state.notification_attempted = true;
                    events.push(InstanceLimitsEvent::Notify(index));
                }
            } else {
                state.excess_since = None;
                state.notification_since = None;
                if complete_within_limit && state.incident {
                    if qualifying_sample(&mut state.recovery_since, now) {
                        state.incident = false;
                        state.notification_attempted = false;
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
