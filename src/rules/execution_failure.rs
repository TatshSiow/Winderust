#![allow(dead_code)]

pub const DEFAULT_EXECUTION_FAILURE_SUPPRESSION_THRESHOLD: u8 = 3;

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
        self.attempts >= DEFAULT_EXECUTION_FAILURE_SUPPRESSION_THRESHOLD
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
