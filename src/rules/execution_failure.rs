use std::{
    borrow::Borrow,
    collections::{BTreeMap, BTreeSet},
    ffi::OsStr,
    sync::atomic::{AtomicU8, Ordering},
};

use crate::foreground::process_failure_key as normalize_process_failure_key;

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

    fn suppress_permanently(&mut self) {
        self.attempts = execution_failure_suppression_threshold();
        self.suppression_logged = true;
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
pub struct ExecutionFailureTracker<K = String> {
    states: BTreeMap<K, ExecutionFailureState>,
}

impl<K: Ord> ExecutionFailureTracker<K> {
    pub fn clear(&mut self) {
        self.states.clear();
    }

    pub fn retain_keys(&mut self, active_keys: &BTreeSet<K>) {
        self.states.retain(|key, _| active_keys.contains(key));
    }

    pub fn key_suppression<Q: Ord + ?Sized>(&mut self, key: &Q) -> ExecutionSuppression
    where
        K: Borrow<Q>,
    {
        let Some(state) = self.states.get_mut(key) else {
            return ExecutionSuppression::default();
        };
        if !state.is_suppressed() {
            return ExecutionSuppression::default();
        }

        ExecutionSuppression::active(state.mark_suppression_logged())
    }

    pub fn has_key_failure<Q: Ord + ?Sized>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
    {
        self.states.contains_key(key)
    }

    pub fn is_key_suppressed<Q: Ord + ?Sized>(&self, key: &Q) -> bool
    where
        K: Borrow<Q>,
    {
        self.states
            .get(key)
            .is_some_and(ExecutionFailureState::is_suppressed)
    }

    pub fn record_key_failure<Q>(&mut self, key: &Q) -> bool
    where
        Q: Ord + AsRef<OsStr> + ToOwned<Owned = K> + ?Sized,
        K: Borrow<Q>,
    {
        self.update_key_failure(key, ExecutionFailureState::record_failure)
    }

    pub fn suppress_key_failure<Q>(&mut self, key: &Q) -> bool
    where
        Q: Ord + AsRef<OsStr> + ToOwned<Owned = K> + ?Sized,
        K: Borrow<Q>,
    {
        self.update_key_failure(key, ExecutionFailureState::suppress_permanently)
    }

    fn update_key_failure<Q>(&mut self, key: &Q, update: fn(&mut ExecutionFailureState)) -> bool
    where
        Q: Ord + AsRef<OsStr> + ToOwned<Owned = K> + ?Sized,
        K: Borrow<Q>,
    {
        if key.as_ref().is_empty() {
            return false;
        }
        let is_new = !self.states.contains_key(key);
        update(self.states.entry(key.to_owned()).or_default());
        is_new
    }

    pub fn clear_key_failure<Q: Ord + ?Sized>(&mut self, key: &Q)
    where
        K: Borrow<Q>,
    {
        self.states.remove(key);
    }
}

impl ExecutionFailureTracker {
    pub fn process_suppression(&mut self, process_name: &str) -> ExecutionSuppression {
        let Some(key) = process_failure_key(process_name) else {
            return ExecutionSuppression::default();
        };
        self.key_suppression(&key)
    }

    pub fn record_process_failure(&mut self, process_name: &str) -> bool {
        process_failure_key(process_name).is_some_and(|key| self.record_key_failure(&key))
    }

    pub fn suppress_process_failure(&mut self, process_name: &str) -> bool {
        process_failure_key(process_name).is_some_and(|key| self.suppress_key_failure(&key))
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

fn process_failure_key(process_identity: &str) -> Option<String> {
    (!process_identity.trim().is_empty()).then(|| normalize_process_failure_key(process_identity))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracker_keeps_exact_paths_and_same_named_binaries_separate() {
        let mut tracker = ExecutionFailureTracker::default();

        tracker.record_process_failure(r"C:\Games\app.exe");
        tracker.record_process_failure(r"C:/Games/app.exe");
        assert!(tracker.record_process_failure(r"C:\Tools\app.exe"));
        assert!(!tracker.process_suppression(r"C:\Games\app.exe").suppressed);

        tracker.record_process_failure(r"C:\Games\app.exe");
        let first = tracker.process_suppression(r"C:\Games\app.exe");
        let second = tracker.process_suppression(r"C:/Games/app.exe");

        assert_eq!(first, ExecutionSuppression::active(true));
        assert_eq!(second, ExecutionSuppression::active(false));
        assert!(!tracker.process_suppression(r"C:\Tools\app.exe").suppressed);
        assert!(!tracker.process_suppression(r"C:\Games\APP.exe").suppressed);
    }

    #[test]
    fn permanent_process_failure_suppresses_immediately() {
        let mut tracker = ExecutionFailureTracker::default();

        assert!(tracker.suppress_process_failure(r"C:\Apps\service.exe"));
        assert_eq!(
            tracker.process_suppression(r"C:\Apps\service.exe"),
            ExecutionSuppression::active(false)
        );
    }
}
