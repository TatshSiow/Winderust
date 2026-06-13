#![allow(dead_code)]

use std::sync::atomic::{AtomicU8, Ordering};

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
pub struct ExecutionFailureState {
    pub attempts: u8,
    pub suppression_logged: bool,
}

impl ExecutionFailureState {
    pub fn record_failure(&mut self) {
        self.attempts = self.attempts.saturating_add(1);
    }

    pub fn is_suppressed(&self) -> bool {
        self.is_suppressed_at(execution_failure_suppression_threshold())
    }

    pub fn is_suppressed_at(&self, threshold: u8) -> bool {
        self.attempts >= threshold
    }

    pub fn mark_suppression_logged(&mut self) -> bool {
        if self.suppression_logged {
            false
        } else {
            self.suppression_logged = true;
            true
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
}
