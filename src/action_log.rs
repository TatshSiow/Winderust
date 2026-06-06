use std::{
    collections::VecDeque,
    time::{SystemTime, UNIX_EPOCH},
};

pub const DEFAULT_ACTION_LOG_CAPACITY: usize = 512;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionLogEntry {
    pub sequence: u64,
    pub timestamp_epoch_ms: u128,
    pub feature: ActionLogFeature,
    pub process_id: Option<u32>,
    pub process_name: String,
    pub action: ActionLogAction,
    pub result: ActionLogResult,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionLogFeature {
    AppSuspension,
    CoreSteering,
    EcoQos,
    CpuLimiter,
    PerformanceMode,
    Watchdog,
    ForegroundResponsiveness,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionLogAction {
    Apply,
    Restore,
    Skip,
    Fail,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActionLogResult {
    Applied,
    Restored,
    Skipped,
    Failed,
}

pub struct ActionLog {
    entries: VecDeque<ActionLogEntry>,
    next_sequence: u64,
    capacity: usize,
}

impl ActionLog {
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: VecDeque::with_capacity(capacity.max(1)),
            next_sequence: 1,
            capacity: capacity.max(1),
        }
    }

    pub fn record(
        &mut self,
        feature: ActionLogFeature,
        process_id: Option<u32>,
        process_name: impl Into<String>,
        action: ActionLogAction,
        result: ActionLogResult,
        reason: impl Into<String>,
    ) {
        if self.entries.len() == self.capacity {
            self.entries.pop_front();
        }

        let entry = ActionLogEntry {
            sequence: self.next_sequence,
            timestamp_epoch_ms: timestamp_epoch_ms(),
            feature,
            process_id,
            process_name: process_name.into(),
            action,
            result,
            reason: reason.into(),
        };
        self.next_sequence = self.next_sequence.saturating_add(1);
        self.entries.push_back(entry);
    }

    pub fn entries(&self) -> Vec<ActionLogEntry> {
        self.entries.iter().cloned().collect()
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.len()
    }
}

impl Default for ActionLog {
    fn default() -> Self {
        Self::new(DEFAULT_ACTION_LOG_CAPACITY)
    }
}

fn timestamp_epoch_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn action_log_keeps_bounded_history() {
        let mut log = ActionLog::new(2);

        log.record(
            ActionLogFeature::CpuLimiter,
            Some(1),
            "a.exe",
            ActionLogAction::Apply,
            ActionLogResult::Applied,
            "first",
        );
        log.record(
            ActionLogFeature::CpuLimiter,
            Some(2),
            "b.exe",
            ActionLogAction::Apply,
            ActionLogResult::Applied,
            "second",
        );
        log.record(
            ActionLogFeature::CpuLimiter,
            Some(3),
            "c.exe",
            ActionLogAction::Apply,
            ActionLogResult::Applied,
            "third",
        );

        let entries = log.entries();
        assert_eq!(log.len(), 2);
        assert_eq!(entries[0].sequence, 2);
        assert_eq!(entries[1].sequence, 3);
        assert_eq!(entries[1].process_name, "c.exe");
    }
}
