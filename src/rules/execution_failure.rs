use std::{
    collections::{BTreeMap, BTreeSet},
    sync::atomic::{AtomicU8, Ordering},
};

use crate::foreground::process_name_key;

pub const DEFAULT_EXECUTION_FAILURE_SUPPRESSION_THRESHOLD: u8 = 3;
pub const MIN_EXECUTION_FAILURE_SUPPRESSION_THRESHOLD: u8 = 1;
pub const MAX_EXECUTION_FAILURE_SUPPRESSION_THRESHOLD: u8 = 100;

static EXECUTION_FAILURE_SUPPRESSION_THRESHOLD: AtomicU8 =
    AtomicU8::new(DEFAULT_EXECUTION_FAILURE_SUPPRESSION_THRESHOLD);

pub fn normalize_execution_failure_suppression_threshold(threshold: u8) -> u8 {
    threshold.clamp(
        MIN_EXECUTION_FAILURE_SUPPRESSION_THRESHOLD,
        MAX_EXECUTION_FAILURE_SUPPRESSION_THRESHOLD,
    )
}

pub fn set_execution_failure_suppression_threshold(threshold: u8) {
    EXECUTION_FAILURE_SUPPRESSION_THRESHOLD.store(
        normalize_execution_failure_suppression_threshold(threshold),
        Ordering::Relaxed,
    );
}

pub fn execution_failure_suppression_threshold() -> u8 {
    EXECUTION_FAILURE_SUPPRESSION_THRESHOLD.load(Ordering::Relaxed)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct ExecutionFailureState {
    pub attempts: u8,
    pub suppression_logged: bool,
}

impl ExecutionFailureState {
    fn record_failure(&mut self) {
        self.attempts = self.attempts.saturating_add(1);
    }

    fn is_suppressed(&self) -> bool {
        self.attempts >= execution_failure_suppression_threshold()
    }

    fn mark_suppression_logged(&mut self) -> bool {
        if self.suppression_logged {
            false
        } else {
            self.suppression_logged = true;
            true
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExecutionFailureTracker {
    states: BTreeMap<String, ExecutionFailureState>,
}

impl ExecutionFailureTracker {
    pub fn clear(&mut self) {
        self.states.clear();
    }

    pub fn retain_keys(&mut self, active_keys: &BTreeSet<String>) {
        self.states.retain(|key, _| active_keys.contains(key));
    }

    pub fn key_suppression(&mut self, key: &str) -> ExecutionSuppression {
        let Some(state) = self.states.get_mut(key) else {
            return ExecutionSuppression::default();
        };
        if !state.is_suppressed() {
            return ExecutionSuppression::default();
        }

        ExecutionSuppression::active(state.mark_suppression_logged())
    }

    pub fn is_key_suppressed(&self, key: &str) -> bool {
        self.states
            .get(key)
            .is_some_and(ExecutionFailureState::is_suppressed)
    }

    pub fn process_suppression(&mut self, process_name: &str) -> ExecutionSuppression {
        let Some(key) = process_failure_key(process_name) else {
            return ExecutionSuppression::default();
        };
        self.key_suppression(&key)
    }

    pub fn record_key_failure(&mut self, key: &str) -> bool {
        if key.is_empty() {
            return false;
        }
        let is_new = !self.states.contains_key(key);
        self.states
            .entry(key.to_owned())
            .or_default()
            .record_failure();
        is_new
    }

    pub fn record_process_failure(&mut self, process_name: &str) -> bool {
        let Some(key) = process_failure_key(process_name) else {
            return false;
        };
        self.record_key_failure(&key)
    }

    pub fn clear_key_failure(&mut self, key: &str) {
        self.states.remove(key);
    }

    pub fn clear_process_failure(&mut self, process_name: &str) {
        if let Some(key) = process_failure_key(process_name) {
            self.clear_key_failure(&key);
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ExecutionSuppression {
    pub suppressed: bool,
    pub newly_suppressed: bool,
}

impl ExecutionSuppression {
    pub const fn active(newly_suppressed: bool) -> Self {
        Self {
            suppressed: true,
            newly_suppressed,
        }
    }
}

fn process_failure_key(process_name: &str) -> Option<String> {
    let process_name = process_name_key(process_name);
    (!process_name.is_empty()).then_some(process_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn three_continuous_failures_suppress_execution() {
        let mut state = ExecutionFailureState::default();

        state.record_failure();
        state.record_failure();
        assert!(!state.is_suppressed());

        state.record_failure();
        assert!(state.is_suppressed());
    }

    #[test]
    fn suppression_log_marker_is_set_once() {
        let mut state = ExecutionFailureState::default();

        assert!(state.mark_suppression_logged());
        assert!(!state.mark_suppression_logged());
    }

    #[test]
    fn tracker_normalizes_process_names() {
        let mut tracker = ExecutionFailureTracker::default();

        tracker.record_process_failure("APP.exe");
        tracker.record_process_failure("app.exe");
        assert!(!tracker.process_suppression("app.exe").suppressed);

        tracker.record_process_failure("app.exe");
        let first = tracker.process_suppression("APP.exe");
        let second = tracker.process_suppression("app.exe");

        assert_eq!(first, ExecutionSuppression::active(true));
        assert_eq!(second, ExecutionSuppression::active(false));
    }
}
